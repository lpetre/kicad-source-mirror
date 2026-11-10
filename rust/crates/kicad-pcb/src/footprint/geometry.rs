// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of the geometry part of `pcbnew/footprint.cpp`: bounding boxes (cached per board
//! time stamp in `geometry_cache`), hull, courtyard caches, `GetEffectiveShape`, hit tests,
//! `GetArea`, `GetFpPadsLocalBbox` and `FixUpPadsForBoard` (package M20).

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::base_units::{ARC_LOW_DEF, PCB_IU_SCALE};
use kicad_core::layer_ids::{LAYER_FP_REFERENCES, LAYER_FP_TEXT, LAYER_FP_VALUES, is_back_layer};
use kicad_core::{KicadT, Lset, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::convex_hull::build_convex_hull_poly_set;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::eda_angle::ANGLE_0;
use kicad_geom::{Box2I, FlipDirection, Shape, ShapeCompound, ShapePolySet, ShapeSimple, Vector2I};

use crate::board::Board;
use crate::board_design_settings::DEFAULT_COURTYARD_WIDTH;
use crate::board_item;
use crate::convert_shape_list_to_polygon::{OutlineErrorHandler, convert_outline_to_polygon};
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::footprint::{Footprint, FootprintCourtyardCacheData, FootprintStackup};
use crate::ids::{FootprintId, ItemId, ShapeId};
use crate::item::SharedShape;
use crate::padstack::PadAttrib;
use crate::pcb_field::FieldT;
use crate::pcb_shape::PcbShape;
use crate::traits::{BoardItem, EdaTextItem, Flashing};

/// The malformed-courtyard flags of `BuildCourtyardCaches` (C++ item flags
/// `MALFORMED_F_COURTYARD` / `MALFORMED_B_COURTYARD`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CourtyardFlags {
    pub malformed_front: bool,
    pub malformed_back: bool,
}

/// `BOARD::IsLayerVisible( aLayer )`: the layer is enabled and in the visible layers (every
/// layer: the `.kicad_prl` visibility is not modelled, see `Board::visible_layers`).
fn board_layer_visible(board: &Board, layer: PcbLayerId) -> bool {
    board.is_layer_enabled(layer) && board.visible_layers().contains(layer)
}

/// `BOARD::IsElementVisible( aLayer )`: true without a project; with a project the
/// `PROJECT_LOCAL_SETTINGS::m_VisibleItems` default (`GAL_SET::DefaultVisible()`), in which the
/// footprint text, value and reference layers are visible. The `.kicad_prl` `visible_items` are
/// not modelled (`project_local_settings.rs`), so this is always true.
fn board_element_visible(_board: &Board, _layer: i32) -> bool {
    true
}

impl Footprint {
    /// `IsValidLayer( l ) && m_privateLayers.test( l )` (and the plain `m_privateLayers.test`,
    /// which never sees an invalid layer on a footprint child).
    fn is_private_layer(&self, layer: PcbLayerId) -> bool {
        self.private_layers.contains(layer)
    }

    /// `FOOTPRINT::GetBoundingBox()` (texts included).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        self.bounding_box_with_text(ctx, true)
    }

    /// `FOOTPRINT::GetBoundingBox( aIncludeText )`: cached per board time stamp (one entry
    /// with text, one without).
    pub fn bounding_box_with_text(&self, ctx: &BoardCtx<'_>, include_text: bool) -> Box2I {
        let has_board = board_item::has_board(self, ctx);
        let board = ctx.board;

        if has_board {
            let cached = self.geometry_cache.with(|cache| {
                if include_text {
                    (cache.bounding_box_timestamp >= board.time_stamp())
                        .then_some(cache.bounding_box)
                } else {
                    (cache.text_excluded_bbox_timestamp >= board.time_stamp())
                        .then_some(cache.text_excluded_bbox)
                }
            });

            if let Some(bbox) = cached {
                return bbox;
            }
        }

        let mut texts: Vec<ItemId> = Vec::new();
        let is_fp_edit = has_board && board.is_footprint_holder();

        let mut bbox = Box2I::from_pos(self.transform.translate());
        bbox.inflate(PCB_IU_SCALE.mm_to_iu(0.25)); // Give a min size to the bbox

        // Calculate the footprint side
        let footprint_side = self.side(ctx);

        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };
            let layer = item.layer();

            if self.is_private_layer(layer) && !is_fp_edit {
                continue;
            }

            // We want the bitmap bounding box just in the footprint editor
            // so it will start with the correct initial zoom
            if item.type_() == KicadT::PcbReferenceImageT && !is_fp_edit {
                continue;
            }

            // Handle text separately
            if item.type_() == KicadT::PcbTextT {
                texts.push(id);
                continue;
            }

            // If we're not including text then drop annotations as well -- unless, of course,
            // it's an unsided footprint -- in which case it's likely to be nothing *but*
            // annotations.
            if !include_text && footprint_side != PcbLayerId::UndefinedLayer {
                if item.type_().base_type() == KicadT::PcbDimensionT {
                    continue;
                }

                if layer == PcbLayerId::CmtsUser
                    || layer == PcbLayerId::DwgsUser
                    || layer == PcbLayerId::Eco1User
                    || layer == PcbLayerId::Eco2User
                {
                    continue;
                }
            }

            bbox.merge(&item.bounding_box());
        }

        for &id in &self.fields {
            let field = &board[id];

            // Reference and value get their own processing
            if field.is_reference() || field.is_value() {
                continue;
            }

            texts.push(id.into());
        }

        for &id in &self.pads {
            bbox.merge(&board[id].bounding_box(ctx));
        }

        for &id in &self.zones {
            bbox.merge(&board[id].bounding_box(ctx));
        }

        for &id in &self.points {
            bbox.merge(&board[id].bounding_box(ctx));
        }

        let no_draw_items =
            self.drawings.is_empty() && self.pads.is_empty() && self.zones.is_empty();

        // Groups do not contribute to the rect, only their members
        if include_text || no_draw_items {
            // Only PCB_TEXT and PCB_FIELD items are independently selectable; PCB_TEXTBOX
            // items go in with other graphic items above.
            for &id in &texts {
                let Some(text) = ctx.item(id) else { continue };

                if !is_fp_edit && self.is_private_layer(text.layer()) {
                    continue;
                }

                if text.type_() == KicadT::PcbFieldT
                    && !text
                        .as_eda_text()
                        .is_some_and(|t| t.eda_text().is_visible())
                {
                    continue;
                }

                bbox.merge(&text.bounding_box());
            }

            // A footprint is constructed with its mandatory fields, but they can be removed
            // again through the editing dialogs or the scripting API, and the const accessors
            // return nullptr rather than recreating them.
            let value = self.field(ctx, FieldT::Value).map(|id| &board[id]);
            let reference = self.field(ctx, FieldT::Reference).map(|id| &board[id]);

            // This can be further optimized when aIncludeInvisibleText is true, but currently
            // leaving this as is until it's determined there is a noticeable speed hit.
            let mut value_layer_is_visible = true;
            let mut ref_layer_is_visible = true;

            if has_board {
                // The first "&&" conditional handles the user turning layers off as well as
                // layers not being present in the current PCB stackup.  Values, references,
                // and all footprint text can also be turned off via the GAL meta-layers, so
                // the 2nd and 3rd "&&" conditionals handle that.
                if let Some(value) = value {
                    value_layer_is_visible = board_layer_visible(board, value.layer(ctx))
                        && board_element_visible(board, LAYER_FP_VALUES)
                        && board_element_visible(board, LAYER_FP_TEXT);
                }

                if let Some(reference) = reference {
                    ref_layer_is_visible = board_layer_visible(board, reference.layer(ctx))
                        && board_element_visible(board, LAYER_FP_REFERENCES)
                        && board_element_visible(board, LAYER_FP_TEXT);
                }
            }

            if let Some(value) = value {
                if (value.eda_text().is_visible() && value_layer_is_visible) || no_draw_items {
                    bbox.merge(&value.bounding_box(ctx));
                }
            }

            if let Some(reference) = reference {
                if (reference.eda_text().is_visible() && ref_layer_is_visible) || no_draw_items {
                    bbox.merge(&reference.bounding_box(ctx));
                }
            }
        }

        if has_board {
            self.geometry_cache.with(|cache| {
                if include_text || no_draw_items {
                    cache.bounding_box_timestamp = board.time_stamp();
                    cache.bounding_box = bbox;
                } else {
                    cache.text_excluded_bbox_timestamp = board.time_stamp();
                    cache.text_excluded_bbox = bbox;
                }
            });
        }

        bbox
    }

    /// `FOOTPRINT::GetCenter()`: `GetBoundingBox( false ).GetCenter()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.bounding_box_with_text(ctx, false).centre()
    }

    /// `FOOTPRINT::GetLayerBoundingBox( aLayers )`.
    pub fn layer_bounding_box(&self, ctx: &BoardCtx<'_>, layers: &Lset) -> Box2I {
        let board = ctx.board;
        let is_fp_edit = board_item::has_board(self, ctx) && board.is_footprint_holder();

        // Start with an uninitialized bounding box
        let mut bbox = Box2I::default();

        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };

            if self.is_private_layer(item.layer()) && !is_fp_edit {
                continue;
            }

            if (layers.clone() & &item.layer_set()).none() {
                continue;
            }

            // We want the bitmap bounding box just in the footprint editor
            // so it will start with the correct initial zoom
            if item.type_() == KicadT::PcbReferenceImageT && !is_fp_edit {
                continue;
            }

            bbox.merge(&item.bounding_box());
        }

        for &id in &self.pads {
            let pad = &board[id];

            if (layers.clone() & &pad.layer_set(ctx)).none() {
                continue;
            }

            bbox.merge(&pad.bounding_box(ctx));
        }

        for &id in &self.zones {
            let zone = &board[id];

            if (layers.clone() & &zone.layer_set(ctx)).none() {
                continue;
            }

            bbox.merge(&zone.bounding_box(ctx));
        }

        for &id in &self.points {
            let point = &board[id];

            if self.is_private_layer(point.layer(ctx)) && !is_fp_edit {
                continue;
            }

            if (layers.clone() & &point.layer_set(ctx)).none() {
                continue;
            }

            bbox.merge(&point.bounding_box(ctx));
        }

        bbox
    }

    /// `FOOTPRINT::GetFpPadsLocalBbox()`: the bounding box of the pads at position (0, 0),
    /// orientation 0, not flipped. C++ builds a copy of the footprint and places it there; a
    /// footprint cannot exist outside its board here, so the copy is made in a clone of the
    /// board (handles stay valid in a clone) and placed with the same cascades. `Box2I`
    /// default for a footprint that is not in an arena.
    pub fn fp_pads_local_bbox(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut bbox = Box2I::default();

        let Some(id) = self.base.id.and_then(|id| id.typed::<Footprint>()) else {
            return bbox;
        };

        // We want the bounding box of the footprint pads at rot 0, not flipped
        // Create such a image:
        let mut dummy_board = ctx.board.clone();

        dummy_board.footprint_set_position(id, Vector2I::new(0, 0));
        dummy_board.footprint_set_orientation(id, ANGLE_0);

        if dummy_board[id].is_flipped() {
            dummy_board.footprint_flip(id, Vector2I::new(0, 0), FlipDirection::TopBottom);
        }

        let dummy_ctx = BoardCtx {
            board: &dummy_board,
            rules: ctx.rules,
            connectivity: None,
        };

        for (_, pad) in dummy_board.footprint_pads(id) {
            bbox.merge(&pad.bounding_box(&dummy_ctx));
        }

        bbox
    }

    /// `FOOTPRINT::TextOnly()`: every non-private drawing is a field or a text.
    pub fn text_only(&self, ctx: &BoardCtx<'_>) -> bool {
        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };

            if self.private_layers.contains(item.layer()) {
                continue;
            }

            if item.type_() != KicadT::PcbFieldT && item.type_() != KicadT::PcbTextT {
                return false;
            }
        }

        true
    }

    /// `FOOTPRINT::GetArea( aPadding )`: the area of `GetBoundingBox( false )` with the padding
    /// added to both sides.
    pub fn area(&self, ctx: &BoardCtx<'_>, padding: i32) -> f64 {
        let bbox = self.bounding_box_with_text(ctx, false);

        let w = (bbox.width() as f64).abs() + padding as f64;
        let h = (bbox.height() as f64).abs() + padding as f64;
        w * h
    }

    /// `FOOTPRINT::GetBoundingHull()`: the convex hull of the drawings, pads (with holes) and
    /// zone fills; cached per board time stamp.
    pub fn bounding_hull(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        let has_board = board_item::has_board(self, ctx);
        let board = ctx.board;
        let is_fp_edit = has_board && board.is_footprint_holder();

        if has_board {
            let cached = self.geometry_cache.with(|cache| {
                (cache.hull_timestamp >= board.time_stamp()).then(|| cache.hull.clone())
            });

            if let Some(hull) = cached {
                return hull;
            }
        }

        let mut raw_polys = ShapePolySet::default();

        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };

            if !is_fp_edit && self.private_layers.contains(item.layer()) {
                continue;
            }

            if item.type_() != KicadT::PcbFieldT && item.type_() != KicadT::PcbReferenceImageT {
                item.transform_shape_to_polygon(
                    &mut raw_polys,
                    PcbLayerId::UndefinedLayer,
                    0,
                    ARC_LOW_DEF,
                    ErrorLoc::Outside,
                    false,
                );
            }

            // We intentionally exclude footprint fields from the bounding hull.
        }

        for &id in &self.pads {
            let pad = &board[id];

            pad.padstack.for_each_unique_layer(|layer| {
                pad.transform_shape_to_polygon(
                    ctx,
                    &mut raw_polys,
                    layer,
                    0,
                    ARC_LOW_DEF,
                    ErrorLoc::Outside,
                    false,
                );
            });

            // In case hole is larger than pad
            pad.transform_hole_to_polygon(ctx, &mut raw_polys, 0, ARC_LOW_DEF, ErrorLoc::Outside);
        }

        for &id in &self.zones {
            let zone = &board[id];

            for layer in zone.layer_set(ctx).seq().0 {
                let layer_poly = zone.filled_polys_list(layer);

                for ii in 0..layer_poly.outline_count() {
                    raw_polys.add_outline(layer_poly.c_outline(ii));
                }
            }
        }

        // If there are some graphic items, build the actual hull.
        // However if no items, create a minimal polygon (can happen if a footprint
        // is created with no item: it contains only 2 texts.
        if raw_polys.outline_count() == 0 || raw_polys.full_point_count() < 3 {
            // generate a small dummy rectangular outline around the anchor
            let halfsize = PCB_IU_SCALE.mm_to_iu(1.0);
            let pos = self.transform.translate();

            raw_polys.new_outline();

            // add a square:
            raw_polys.append(pos.x - halfsize, pos.y - halfsize, -1, -1, false);
            raw_polys.append(pos.x + halfsize, pos.y - halfsize, -1, -1, false);
            raw_polys.append(pos.x + halfsize, pos.y + halfsize, -1, -1, false);
            raw_polys.append(pos.x - halfsize, pos.y + halfsize, -1, -1, false);
        }

        let mut convex_hull: Vec<Vector2I> = Vec::new();
        build_convex_hull_poly_set(&mut convex_hull, &raw_polys);

        self.geometry_cache.with(|cache| {
            cache.hull.remove_all_contours();
            cache.hull.new_outline();

            for pt in &convex_hull {
                cache.hull.append(pt.x, pt.y, -1, -1, false);
            }

            if has_board {
                cache.hull_timestamp = board.time_stamp();
            }

            cache.hull.clone()
        })
    }

    /// `FOOTPRINT::GetBoundingHull( aLayer )`: the convex hull of what is on `layer` (not
    /// cached).
    pub fn bounding_hull_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> ShapePolySet {
        let board = ctx.board;
        let is_fp_edit = board_item::has_board(self, ctx) && board.is_footprint_holder();

        let mut raw_polys = ShapePolySet::default();
        let mut hull = ShapePolySet::default();

        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };

            if !is_fp_edit && self.private_layers.contains(item.layer()) {
                continue;
            }

            if item.is_on_layer(layer) {
                if item.type_() != KicadT::PcbFieldT && item.type_() != KicadT::PcbReferenceImageT {
                    item.transform_shape_to_polygon(
                        &mut raw_polys,
                        PcbLayerId::UndefinedLayer,
                        0,
                        ARC_LOW_DEF,
                        ErrorLoc::Outside,
                        false,
                    );
                }

                // We intentionally exclude footprint fields from the bounding hull.
            }
        }

        for &id in &self.pads {
            let pad = &board[id];

            if pad.is_on_layer(ctx, layer) {
                pad.transform_shape_to_polygon(
                    ctx,
                    &mut raw_polys,
                    layer,
                    0,
                    ARC_LOW_DEF,
                    ErrorLoc::Outside,
                    false,
                );
            }
        }

        for &id in &self.zones {
            let zone = &board[id];

            if zone.is_rule_area() {
                continue;
            }

            if zone.is_on_layer(ctx, layer) {
                let layer_poly = zone.filled_polys_list(layer);

                for ii in 0..layer_poly.outline_count() {
                    raw_polys.add_outline(layer_poly.c_outline(ii));
                }
            }
        }

        let mut convex_hull: Vec<Vector2I> = Vec::new();
        build_convex_hull_poly_set(&mut convex_hull, &raw_polys);

        hull.new_outline();

        for pt in &convex_hull {
            hull.append(pt.x, pt.y, -1, -1, false);
        }

        hull
    }

    /// `FOOTPRINT::GetEffectiveShape( aLayer, aFlash, aUsage )`: the courtyard outline for
    /// F_CrtYd / B_CrtYd, else the pads and the graphic shapes / barcodes.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        let mut shape = ShapeCompound::new();

        // There are several possible interpretations here:
        // 1) the bounding box (without or without invisible items)
        // 2) just the pads and "edges" (ie: non-text graphic items)
        // 3) the courtyard

        // We'll go with (2) for now, unless the caller is clearly looking for (3)

        if layer == PcbLayerId::FCrtYd || layer == PcbLayerId::BCrtYd {
            let courtyard = self.courtyard(ctx, layer);

            if courtyard.outline_count() == 0 {
                // malformed/empty polygon
                return Arc::new(shape);
            }

            shape.add_shape(Box::new(ShapeSimple::from_line_chain(
                courtyard.c_outline(0),
            )));
        } else {
            for &id in &self.pads {
                let pad_shape = ctx.board[id].effective_shape(ctx, layer, flash, usage);
                shape.add_shape(pad_shape.clone_shape());
            }

            for &id in &self.drawings {
                let Some(item) = ctx.item(id) else { continue };

                if item.type_() == KicadT::PcbShapeT || item.type_() == KicadT::PcbBarcodeT {
                    shape.add_shape(item.effective_shape(layer, flash, usage).clone_shape());
                }
            }
        }

        Arc::new(shape)
    }

    /// `FOOTPRINT::HitTest( aPosition, aAccuracy )`: inside the text-free bounding box.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        let mut rect = self.bounding_box_with_text(ctx, false);
        rect.inflate(accuracy).contains(pos)
    }

    /// `FOOTPRINT::HitTestAccurate( aPosition, aAccuracy )`: on or inside the bounding hull.
    pub fn hit_test_accurate(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        self.bounding_hull(ctx)
            .collide_point(pos, accuracy, None, None)
    }

    /// `FOOTPRINT::HitTestOnLayer( aPosition, aLayer, aAccuracy )`.
    pub fn hit_test_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        pos: Vector2I,
        layer: PcbLayerId,
        accuracy: i32,
    ) -> bool {
        let board = ctx.board;

        for &id in &self.pads {
            let pad = &board[id];

            if pad.is_on_layer(ctx, layer) && pad.hit_test(ctx, pos, accuracy) {
                return true;
            }
        }

        for &id in &self.zones {
            let zone = &board[id];

            if zone.is_on_layer(ctx, layer) && zone.hit_test(ctx, pos, accuracy) {
                return true;
            }
        }

        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };

            if item.type_() != KicadT::PcbTextT
                && item.is_on_layer(layer)
                && item.as_dyn().hit_test(ctx, pos, accuracy)
            {
                return true;
            }
        }

        false
    }

    /// `FOOTPRINT::HitTestOnLayer( aRect, aContained, aLayer, aAccuracy )`.
    pub fn hit_test_rect_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        layer: PcbLayerId,
        accuracy: i32,
    ) -> bool {
        let mut items: Vec<ItemId> = Vec::new();

        for &id in &self.pads {
            if ctx.board[id].is_on_layer(ctx, layer) {
                items.push(id.into());
            }
        }

        for &id in &self.zones {
            if ctx.board[id].is_on_layer(ctx, layer) {
                items.push(id.into());
            }
        }

        for &id in &self.drawings {
            if let Some(item) = ctx.item(id) {
                if item.type_() != KicadT::PcbTextT && item.is_on_layer(layer) {
                    items.push(id);
                }
            }
        }

        // If we require the elements to be contained in the rect and any of them are not,
        // we can return false;
        // Conversely, if we just require any of the elements to have a hit, we can return true
        // when the first one is found.
        for &id in &items {
            let Some(item) = ctx.item(id) else { continue };
            let hit = item.as_dyn().hit_test_rect(ctx, rect, contained, accuracy);

            if !contained && hit {
                return true;
            } else if contained && !hit {
                return false;
            }
        }

        // If we didn't exit in the loop, that means that we did not return false for
        // aContained or we did not return true for !aContained.  So we can just return the
        // bool with a test of whether there were any elements or not.
        !items.is_empty() && contained
    }

    /// `FOOTPRINT::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let board = ctx.board;
        let mut arect = *rect;
        arect.inflate(accuracy);

        if contained {
            arect.contains_box(&self.bounding_box_with_text(ctx, false))
        } else {
            // If the rect does not intersect the bounding box, skip any tests
            if !arect.intersects(&self.bounding_box_with_text(ctx, false)) {
                return false;
            }

            // If there are no pads, zones, or drawings, allow intersection with text
            if self.pads.is_empty() && self.zones.is_empty() && self.drawings.is_empty() {
                return self.bounding_box_with_text(ctx, true).intersects(&arect);
            }

            // Determine if any elements in the FOOTPRINT intersect the rect
            for &id in &self.pads {
                if board[id].hit_test_rect(ctx, &arect, false, 0) {
                    return true;
                }
            }

            for &id in &self.zones {
                if board[id].hit_test_rect(ctx, &arect, false, 0) {
                    return true;
                }
            }

            for &id in &self.points {
                if board[id].hit_test_rect(ctx, &arect, false, 0) {
                    return true;
                }
            }

            // PCB fields are selectable on their own, so they don't get tested

            for &id in &self.drawings {
                let Some(item) = ctx.item(id) else { continue };

                // Text items are selectable on their own, and are therefore excluded from this
                // test.  TextBox items are NOT selectable on their own, and so MUST be included
                // here. Bitmaps aren't selectable since they aren't displayed.
                if item.type_() != KicadT::PcbTextT
                    && item.as_dyn().hit_test_rect(ctx, &arect, false, 0)
                {
                    return true;
                }
            }

            // Groups are not hit-tested; only their members

            // No items were hit
            false
        }
    }

    /// `FOOTPRINT::BuildCourtyardCaches( aErrorHandler )`: build the front / back courtyard
    /// polygons into `courtyard_cache` (a class-1 cache, so `&self`). The MALFORMED flags
    /// live in the cache value (`courtyard_flags`).
    pub fn build_courtyard_caches(
        &self,
        ctx: &BoardCtx<'_>,
        handler: Option<&mut dyn OutlineErrorHandler>,
    ) {
        let data = self.compute_courtyard_caches(ctx, handler);
        self.courtyard_cache.store(data);
    }

    /// The body of `BuildCourtyardCaches` on a fresh cache value.
    fn compute_courtyard_caches(
        &self,
        ctx: &BoardCtx<'_>,
        handler: Option<&mut dyn OutlineErrorHandler>,
    ) -> FootprintCourtyardCacheData {
        let mut handler = handler;
        let mut cache = FootprintCourtyardCacheData::default();

        // Build the courtyard area from graphic items on the courtyard.
        // Only PCB_SHAPE_T have meaning, graphic texts are ignored.
        // Collect items:
        let mut list_front: Vec<ShapeId> = Vec::new();
        let mut list_back: Vec<ShapeId> = Vec::new();
        let mut front_width_histogram: BTreeMap<i32, i32> = BTreeMap::new();
        let mut back_width_histogram: BTreeMap<i32, i32> = BTreeMap::new();

        for &id in &self.drawings {
            let Some(shape_id) = id.typed::<PcbShape>() else {
                continue;
            };
            let shape = &ctx.board[shape_id];
            let layer = shape.base.item.layer;

            if layer == PcbLayerId::BCrtYd {
                list_back.push(shape_id);
                *back_width_histogram
                    .entry(shape.stroke(ctx).width())
                    .or_insert(0) += 1;
            }

            if layer == PcbLayerId::FCrtYd {
                list_front.push(shape_id);
                *front_width_histogram
                    .entry(shape.stroke(ctx).width())
                    .or_insert(0) += 1;
            }
        }

        if list_front.is_empty() && list_back.is_empty() {
            return cache;
        }

        let max_error = PCB_IU_SCALE.mm_to_iu(0.005); // max error for polygonization
        let chaining_epsilon = PCB_IU_SCALE.mm_to_iu(0.02); // max dist from one endPt to next startPt

        // std::max_element: the first entry (in key order) with the largest count.
        let most_used_width = |histogram: &BTreeMap<i32, i32>| -> Option<i32> {
            let mut best: Option<(i32, i32)> = None;

            for (&width, &count) in histogram {
                if best.is_none_or(|(_, best_count)| best_count < count) {
                    best = Some((width, count));
                }
            }

            best.map(|(width, _)| width)
        };

        let sides = [
            (&list_front, &front_width_histogram, true),
            (&list_back, &back_width_histogram, false),
        ];

        for (list, histogram, front) in sides {
            let mut poly = ShapePolySet::default();
            let handler_ref: Option<&mut dyn OutlineErrorHandler> = match &mut handler {
                Some(h) => Some(&mut **h),
                None => None,
            };

            if convert_outline_to_polygon(
                ctx,
                list,
                &mut poly,
                max_error,
                chaining_epsilon,
                true,
                handler_ref,
                false,
            ) {
                let mut width = 0;

                // Touching courtyards, or courtyards -at- the clearance distance are legal.
                // Use maxError here because that is the allowed deviation when transforming
                // arcs/circles to polygons.
                poly.inflate(
                    -max_error,
                    CornerStrategy::ChamferAcuteCorners,
                    max_error,
                    false,
                );

                poly.cache_triangulation(false);

                if let Some(max) = most_used_width(histogram) {
                    width = max;
                }

                if width == 0 {
                    width = PCB_IU_SCALE.mm_to_iu(DEFAULT_COURTYARD_WIDTH);
                }

                if poly.outline_count() > 0 {
                    poly.outline(0).set_width(width);
                }
            } else if front {
                cache.malformed_front = true;
            } else {
                cache.malformed_back = true;
            }

            if front {
                cache.front = poly;
            } else {
                cache.back = poly;
            }
        }

        cache.front_hash = cache.front.get_hash();
        cache.back_hash = cache.back.get_hash();
        cache
    }

    /// `FOOTPRINT::GetCourtyard( aLayer )`: the cached courtyard of F_CrtYd / B_CrtYd
    /// (`IsBackLayer( aLayer )` picks the back one), rebuilt when there is no cache or the
    /// stored hashes are stale.
    pub fn courtyard(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> ShapePolySet {
        let cache = self.courtyard_cache.get_or_build(
            |c| c.front_hash == c.front.get_hash() && c.back_hash == c.back.get_hash(),
            || self.compute_courtyard_caches(ctx, None),
        );

        if is_back_layer(layer) {
            cache.back.clone()
        } else {
            cache.front.clone()
        }
    }

    /// `FOOTPRINT::GetCachedCourtyard( aLayer )`: the cached courtyard without a validity
    /// check (empty when nothing was built yet).
    pub fn cached_courtyard(&self, layer: PcbLayerId) -> ShapePolySet {
        let Some(cache) = self.courtyard_cache.peek() else {
            return ShapePolySet::default();
        };

        if is_back_layer(layer) {
            cache.back.clone()
        } else {
            cache.front.clone()
        }
    }

    /// The MALFORMED_F/B_COURTYARD flags of the courtyard cache (C++ `GetFlags()`): as set by
    /// the last `BuildCourtyardCaches`, none when it never ran (or the cache was dropped).
    pub fn courtyard_flags(&self, _ctx: &BoardCtx<'_>) -> CourtyardFlags {
        self.courtyard_cache
            .peek()
            .map_or(CourtyardFlags::default(), |cache| CourtyardFlags {
                malformed_front: cache.malformed_front,
                malformed_back: cache.malformed_back,
            })
    }
}

impl Board {
    /// `FOOTPRINT::FixUpPadsForBoard( aBoard )`: in `EXPAND_INNER_LAYERS` mode every PTH pad
    /// gets all copper layers of the board. Note: the C++ parser calls it with the parser's
    /// board, which is null when a library footprint is read (`FootprintLoad`): the holder
    /// board of a `.kicad_mod` parse must not call this.
    pub fn footprint_fix_up_pads_for_board(&mut self, fp: FootprintId) {
        let Some(footprint) = self.footprints.get(fp) else {
            return;
        };

        if footprint.stackup_mode != FootprintStackup::ExpandInnerLayers {
            return;
        }

        let board_copper = Lset::all_cu_mask_n(self.copper_layer_count());
        let pads: Vec<_> = footprint.pads.iter().copied().collect();

        for pad in pads {
            if self
                .pads
                .get(pad)
                .is_some_and(|p| p.attribute() == PadAttrib::Pth)
            {
                self.modify_pad(pad, |p, env| {
                    // PAD::GetLayerSet(): the padstack layer set.
                    let mut pad_layers = p.padstack.layer_set.clone();
                    pad_layers |= &board_copper;
                    p.set_layer_set(env, pad_layers);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ports of `qa/tests/pcbnew/test_footprint.cpp` (`FootprintCourtyardAndHull`,
    //! `FootprintBoundingBoxWithoutMandatoryFields` without the field removal),
    //! `test_footprint_pad_fixup.cpp`, `drc/test_footprint_courtyard_chaining.cpp`, and the
    //! oracle replay of `tests/golden/m20_footprints.txt`.

    use super::*;
    use crate::board::AddMode;
    #[cfg(feature = "dump")]
    use crate::dump::format::{v_box, v_f64, v_polyset};
    use crate::eda_shape::ShapeT;
    use crate::item::BoardUse;
    use crate::item::ParentRef;
    use crate::item_ref::OwnedItem;
    use crate::pad::Pad;
    use crate::padstack::PadShape;
    use crate::pcb_text::PcbText;
    use kicad_geom::EdaAngle;

    fn mm(v: f64) -> i32 {
        PCB_IU_SCALE.mm_to_iu(v)
    }

    fn pt(x: f64, y: f64) -> Vector2I {
        Vector2I::new(mm(x), mm(y))
    }

    /// A graphic child of `fp` in the library frame (the footprint is still at the identity
    /// placement while it is filled).
    fn add_shape(
        board: &mut Board,
        fp: FootprintId,
        uuid: Option<&str>,
        shape: ShapeT,
        layer: PcbLayerId,
        width: i32,
        geometry: impl FnOnce(&mut PcbShape, &crate::ctx::MutEnv),
    ) -> ItemId {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut s = PcbShape::new(&mut board.kiid_generator, shape);
        BoardItem::base_mut(&mut s).parent = ParentRef::Item(fp.into());

        if let Some(uuid) = uuid {
            BoardItem::base_mut(&mut s).uuid = kicad_core::Kiid::parse(uuid).unwrap();
        }

        geometry(&mut s, &env);
        s.set_width(&env, width);
        s.set_layer(&env, layer);
        board
            .footprint_add(fp, OwnedItem::Shape(Box::new(s)), AddMode::Insert)
            .unwrap_or_else(|_| panic!("shape refused"))
    }

    fn segment(
        board: &mut Board,
        fp: FootprintId,
        u: &str,
        layer: PcbLayerId,
        w: f64,
        a: Vector2I,
        b: Vector2I,
    ) {
        add_shape(
            board,
            fp,
            Some(u),
            ShapeT::Segment,
            layer,
            mm(w),
            |s, env| {
                s.set_start(env, a);
                s.set_end(env, b);
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn add_pad(
        board: &mut Board,
        fp: FootprintId,
        u: &str,
        number: &str,
        attrib: PadAttrib,
        layers: Lset,
        shape: PadShape,
        size: Vector2I,
        pos: Vector2I,
        angle: f64,
        drill: i32,
    ) {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pad = Pad::new(&mut board.kiid_generator);
        BoardItem::base_mut(&mut pad).parent = ParentRef::Item(fp.into());
        BoardItem::base_mut(&mut pad).uuid = kicad_core::Kiid::parse(u).unwrap();
        pad.set_number(number);
        pad.set_attribute(attrib);
        pad.set_layer_set(&env, layers);
        pad.set_shape(PcbLayerId::FCu, shape);
        pad.set_size(&env, PcbLayerId::FCu, size);

        if shape == PadShape::Roundrect {
            pad.set_round_rect_radius_ratio(PcbLayerId::FCu, 0.25);
        }

        if drill > 0 {
            pad.set_drill_shape(crate::padstack::PadDrillShape::Circle);
            pad.set_drill_size(&env, Vector2I::new(drill, drill));
        }

        pad.set_position(&env, pos);
        pad.set_fp_relative_orientation(&env, EdaAngle::from_degrees(angle));
        board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
            .unwrap_or_else(|_| panic!("pad refused"));
    }

    /// Set up the reference / value fields: text, library position, lib angle, visibility.
    fn set_fields(
        board: &mut Board,
        fp: FootprintId,
        prefix: &str,
        fields: [(&str, f64, f64, bool); 2],
        text_width: i32,
    ) {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let ctx = BoardCtx::bare(board);
        let ids = [
            board[fp].field(&ctx, FieldT::Reference).unwrap(),
            board[fp].field(&ctx, FieldT::Value).unwrap(),
        ];

        for (n, (id, (text, y, angle, visible))) in ids.into_iter().zip(fields).enumerate() {
            let uuid = format!("{prefix}-0000-4000-8000-00000000000{}", n + 1);
            board.rebind_item_uuid(id.into(), kicad_core::Kiid::parse(&uuid).unwrap());
            board.modify_field(id, |f, _| {
                f.text.text.set_text(text);
                f.text.text.set_visible(visible);
                EdaTextItem::set_text_pos(f, &env, Vector2I::new(0, mm(y)));
                EdaTextItem::set_text_size(f, &env, Vector2I::new(text_width, 1_000_000), false);
                EdaTextItem::set_text_thickness(f, &env, 150_000);
                EdaTextItem::set_text_angle(f, &env, EdaAngle::from_degrees(angle));
            });
        }
    }

    fn new_footprint(board: &mut Board, prefix: &str) -> FootprintId {
        let fp = board.create_footprint();
        let uuid = format!("{prefix}-0000-4000-8000-000000000000");
        board.rebind_item_uuid(fp.into(), kicad_core::Kiid::parse(&uuid).unwrap());
        fp
    }

    /// Footprints A and B of `m20_footprints.kicad_pcb`: A on the front, rotated 30 degrees
    /// and scaled 1.5 x 0.75, with a closed F.CrtYd outline (four segments, one arc), a
    /// Cmts.User line and a user text; B its flipped copy without the arc (an open, hence
    /// malformed, courtyard) and without the text.
    fn build_cascade(board: &mut Board, prefix: &str, flip: bool) -> FootprintId {
        let u = |n: u32| format!("{prefix}-0000-4000-8000-0000000000{n:02}");
        let fp = new_footprint(board, prefix);
        let v_angle = if flip { -2.5 } else { 2.5 };
        set_fields(
            board,
            fp,
            prefix,
            [("R1", -2.5, 330.0, true), ("V", v_angle, 330.0, true)],
            1_000_001,
        );

        if !flip {
            let env = board.child_env(ParentRef::Item(fp.into()));
            let mut text = PcbText::new(&mut board.kiid_generator);
            BoardItem::base_mut(&mut text).parent = ParentRef::Item(fp.into());
            BoardItem::base_mut(&mut text).uuid = kicad_core::Kiid::parse(&u(9)).unwrap();
            text.text.set_text("T");
            EdaTextItem::set_text_pos(&mut text, &env, pt(0.5, 0.5));
            EdaTextItem::set_text_size(&mut text, &env, Vector2I::new(1_000_001, 1_000_000), false);
            EdaTextItem::set_text_thickness(&mut text, &env, 150_000);
            EdaTextItem::set_text_angle(&mut text, &env, EdaAngle::from_degrees(330.0));
            crate::traits::BoardItemMut::set_layer(&mut text, &env, PcbLayerId::FSilkS);
            board
                .footprint_add(fp, OwnedItem::Text(Box::new(text)), AddMode::Insert)
                .unwrap_or_else(|_| panic!("text refused"));
        }

        segment(
            board,
            fp,
            &u(3),
            PcbLayerId::FSilkS,
            0.12,
            pt(0.0, -1.0),
            pt(2.0, 1.5),
        );
        add_shape(
            board,
            fp,
            Some(&u(4)),
            ShapeT::Circle,
            PcbLayerId::FFab,
            mm(0.1),
            |s, env| {
                s.set_start(env, pt(-1.0, -1.0));
                s.set_end(env, pt(-0.5, -1.0));
            },
        );
        segment(
            board,
            fp,
            &u(8),
            PcbLayerId::CmtsUser,
            0.1,
            pt(-1.0, 2.0),
            pt(4.0, 2.0),
        );
        let crtyd = PcbLayerId::FCrtYd;
        segment(
            board,
            fp,
            &u(10),
            crtyd,
            0.05,
            pt(-3.0, -1.5),
            pt(2.5, -1.5),
        );
        segment(board, fp, &u(11), crtyd, 0.05, pt(2.5, -1.5), pt(2.5, 1.2));

        if !flip {
            add_shape(
                board,
                fp,
                Some(&u(12)),
                ShapeT::Arc,
                crtyd,
                mm(0.05),
                |s, env| {
                    s.set_arc_geometry(env, pt(2.5, 1.2), pt(2.207107, 1.907107), pt(1.5, 2.2));
                },
            );
        }

        segment(board, fp, &u(13), crtyd, 0.05, pt(1.5, 2.2), pt(-3.0, 2.2));
        segment(
            board,
            fp,
            &u(14),
            crtyd,
            0.06,
            pt(-3.0, 2.2),
            pt(-3.0, -1.5),
        );

        let front = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FMask, PcbLayerId::FPaste]);
        add_pad(
            board,
            fp,
            &u(5),
            "1",
            PadAttrib::Smd,
            front,
            PadShape::Rectangle,
            pt(1.0, 0.6),
            Vector2I::new(1_000_000, -500_001),
            345.0,
            0,
        );
        add_pad(
            board,
            fp,
            &u(6),
            "2",
            PadAttrib::Pth,
            Pad::pth_mask(),
            PadShape::Oval,
            pt(1.2, 0.8),
            Vector2I::new(-2_000_000, 1_500_001),
            330.0,
            mm(0.6),
        );

        let pos = if flip {
            pt(120.0, 100.0)
        } else {
            pt(100.0, 100.0)
        };
        board.footprint_set_position(fp, pos);
        board.footprint_set_orientation(fp, EdaAngle::from_degrees(30.0));
        board.footprint_set_transform_scale(fp, 1.5, 0.75);

        if flip {
            board.footprint_flip(fp, pos, FlipDirection::TopBottom);
        }

        board.attach_footprint(fp, AddMode::Append);
        fp
    }

    /// The `m20_footprints.kicad_pcb` board built by hand (the footprint parser is pending).
    fn build_m20_board() -> Board {
        let mut board = Board::new();
        build_cascade(&mut board, "aaaaaaaa", false);
        build_cascade(&mut board, "bbbbbbbb", true);

        // C: fields only (the "no draw items" paths), value hidden.
        let fp = new_footprint(&mut board, "cccccccc");
        set_fields(
            &mut board,
            fp,
            "cccccccc",
            [("C1", -1.5, 0.0, true), ("VC", 1.5, 0.0, false)],
            1_000_000,
        );
        board.footprint_set_position(fp, pt(140.0, 100.0));
        board.attach_footprint(fp, AddMode::Append);

        // D: rotated 90 degrees, a private-layer line, a circular courtyard, a roundrect pad.
        let fp = new_footprint(&mut board, "dddddddd");
        set_fields(
            &mut board,
            fp,
            "dddddddd",
            [("D1", -3.0, 0.0, true), ("VD", 3.0, 0.0, true)],
            1_000_000,
        );
        board.modify_footprint(fp, |f, _| {
            f.private_layers = Lset::from_layers(&[PcbLayerId::User1])
        });
        segment(
            &mut board,
            fp,
            "dddddddd-0000-4000-8000-000000000003",
            PcbLayerId::User1,
            0.1,
            pt(-5.0, -5.0),
            pt(5.0, 5.0),
        );
        add_shape(
            &mut board,
            fp,
            Some("dddddddd-0000-4000-8000-000000000004"),
            ShapeT::Circle,
            PcbLayerId::FCrtYd,
            mm(0.05),
            |s, env| {
                s.set_start(env, pt(0.0, 0.0));
                s.set_end(env, pt(2.0, 0.0));
            },
        );
        let front = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FMask, PcbLayerId::FPaste]);
        add_pad(
            &mut board,
            fp,
            "dddddddd-0000-4000-8000-000000000005",
            "1",
            PadAttrib::Smd,
            front,
            PadShape::Roundrect,
            pt(1.5, 0.8),
            pt(0.5, 0.25),
            // legacy `(at 0.5 0.25)`: board-frame angle 0 in a footprint rotated 90 degrees
            270.0,
            0,
        );
        board.footprint_set_position(fp, pt(160.0, 100.0));
        board.footprint_set_orientation(fp, EdaAngle::from_degrees(90.0));
        board.attach_footprint(fp, AddMode::Append);

        board.check_invariants().unwrap();
        board
    }

    /// Differential check against the reference build: every footprint-level geometry value
    /// of `tests/golden/m20_footprints.txt` (`pcb-dump board --sections bbox,footprint`).
    #[cfg(feature = "dump")]
    #[test]
    fn m20_oracle_footprint_geometry() {
        let golden = include_str!("../../tests/golden/m20_footprints.txt");
        let board = build_m20_board();
        let ctx = BoardCtx::bare(&board);
        let mut checked = 0;
        let mut mismatches = Vec::new();

        for line in golden
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("order|"))
        {
            let (head, expected) = line.split_once('=').unwrap();
            let mut parts = head.split('|');
            let (_, key, field) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            let item = ctx.resolve(&kicad_core::Kiid::parse(key).unwrap()).unwrap();
            let fp = item.as_footprint().unwrap();
            let layers = |l: PcbLayerId| Lset::from_layers(&[l]);

            let actual = match field {
                "bbox" => v_box(item.bounding_box()),
                "bbox_with_text" => v_box(fp.bounding_box_with_text(&ctx, true)),
                "bbox_without_text" => v_box(fp.bounding_box_with_text(&ctx, false)),
                "fp_pads_local_bbox" => v_box(fp.fp_pads_local_bbox(&ctx)),
                "bounding_hull" => v_polyset(&fp.bounding_hull(&ctx)),
                "layer_bbox.F_Cu" => v_box(fp.layer_bounding_box(&ctx, &layers(PcbLayerId::FCu))),
                "layer_bbox.B_Cu" => v_box(fp.layer_bounding_box(&ctx, &layers(PcbLayerId::BCu))),
                "courtyard.F" => v_polyset(&fp.courtyard(&ctx, PcbLayerId::FCrtYd)),
                "courtyard.B" => v_polyset(&fp.courtyard(&ctx, PcbLayerId::BCrtYd)),
                "hull.F_Cu" => v_polyset(&fp.bounding_hull_on_layer(&ctx, PcbLayerId::FCu)),
                "hull.B_Cu" => v_polyset(&fp.bounding_hull_on_layer(&ctx, PcbLayerId::BCu)),
                "area" => v_f64(fp.area(&ctx, 0)),
                _ => continue,
            };

            if actual != expected {
                mismatches.push(format!("{head}\n   C++ {expected}\n  Rust {actual}"));
            }

            checked += 1;
        }

        assert!(
            mismatches.is_empty(),
            "{} of {checked} differ:\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
        assert_eq!(checked, 48);
    }

    /// `test_footprint_pad_fixup.cpp` `PthPadExpandsToAllCopperLayers`.
    #[test]
    fn pth_pad_expands_to_all_copper_layers() {
        let mut board = Board::new();
        board.board_use = BoardUse::FpHolder;
        board.set_copper_layer_count(4);
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        board.modify_footprint(fp, |f, _| {
            f.set_stackup_mode(FootprintStackup::ExpandInnerLayers)
        });
        add_pad(
            &mut board,
            fp,
            "eeeeeeee-0000-4000-8000-000000000001",
            "1",
            PadAttrib::Pth,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]),
            PadShape::Rectangle,
            pt(1.0, 1.0),
            pt(0.0, 0.0),
            0.0,
            0,
        );

        board.footprint_fix_up_pads_for_board(fp);

        let pad = board.footprint_pads(fp).next().unwrap().1;
        let layers = pad.layer_set(&BoardCtx::bare(&board));
        assert_eq!(layers.count(), 4);

        for l in [
            PcbLayerId::FCu,
            PcbLayerId::In1Cu,
            PcbLayerId::In2Cu,
            PcbLayerId::BCu,
        ] {
            assert!(layers.contains(l));
        }
    }

    /// `KI_TEST::ChainsAreCyclicallyEqual` (`SHAPE_LINE_CHAIN::CompareGeometry( .., true, tol )`).
    fn cyclically_equal(
        a: &kicad_geom::ShapeLineChain,
        b: &kicad_geom::ShapeLineChain,
        tol: i32,
    ) -> bool {
        a.compare_geometry(b, true, tol)
    }

    /// `test_footprint.cpp` `FootprintCourtyardAndHull`: the courtyard and the hull follow
    /// `Move`, `Rotate` and `Flip`.
    #[test]
    fn footprint_courtyard_and_hull() {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);

        let line_w = mm(0.05);
        let courtyard_h = mm(1.0);
        let courtyard_w = mm(2.0);
        let courtyard_points = [
            Vector2I::new(0, 0),
            Vector2I::new(courtyard_w, 0),
            Vector2I::new(courtyard_w, courtyard_h),
            Vector2I::new(0, courtyard_h),
        ];

        add_shape(
            &mut board,
            fp,
            None,
            ShapeT::Poly,
            PcbLayerId::FCrtYd,
            line_w,
            |s, env| {
                let mut poly = ShapePolySet::default();
                poly.add_outline(&kicad_geom::ShapeLineChain::from_points(
                    &courtyard_points,
                    true,
                ));
                s.set_poly_shape(env, poly);
            },
        );

        // The courtyards get a tiny epsilon to handle polygonisaton errors
        let courtyard_epsilon = mm(0.005);
        let assert_courtyard =
            |board: &Board, layer: PcbLayerId, expected: &kicad_geom::ShapeLineChain| {
                let ctx = BoardCtx::bare(board);
                let courtyard = board[fp].courtyard(&ctx, layer);
                assert_eq!(courtyard.outline_count(), 1);
                assert!(cyclically_equal(
                    courtyard.c_outline(0),
                    expected,
                    courtyard_epsilon
                ));
            };
        let assert_no_courtyard = |board: &Board, layer: PcbLayerId| {
            let ctx = BoardCtx::bare(board);
            assert_eq!(board[fp].courtyard(&ctx, layer).outline_count(), 0);
        };
        let assert_hull = |board: &Board, expected: &kicad_geom::ShapeLineChain| {
            let ctx = BoardCtx::bare(board);
            let hull = board[fp].bounding_hull(&ctx);
            assert_eq!(hull.outline_count(), 1);
            assert!(cyclically_equal(hull.c_outline(0), expected, 0));
        };

        // We'll modify this in lock-step with the footprint
        let mut expected_courtyard =
            kicad_geom::ShapeLineChain::from_points(&courtyard_points, true);
        // The hull is hard to calculate - we'll take the initial one as a given
        let mut expected_hull = board[fp]
            .bounding_hull(&BoardCtx::bare(&board))
            .c_outline(0)
            .clone();

        // Initial courtyard
        assert_courtyard(&board, PcbLayerId::FCrtYd, &expected_courtyard);
        assert_no_courtyard(&board, PcbLayerId::BCrtYd);

        let move_vector = Vector2I::new(courtyard_w, 0);
        board.footprint_move(fp, move_vector);
        expected_courtyard.move_(move_vector);
        expected_hull.move_(move_vector);

        // Moved courtyard
        assert_courtyard(&board, PcbLayerId::FCrtYd, &expected_courtyard);
        assert_no_courtyard(&board, PcbLayerId::BCrtYd);
        assert_hull(&board, &expected_hull);

        board.footprint_rotate(fp, Vector2I::new(0, 0), EdaAngle::from_degrees(90.0));
        expected_courtyard.rotate(EdaAngle::from_degrees(90.0), Vector2I::new(0, 0));
        expected_hull.rotate(EdaAngle::from_degrees(90.0), Vector2I::new(0, 0));

        // Rotated courtyard
        assert_courtyard(&board, PcbLayerId::FCrtYd, &expected_courtyard);
        assert_no_courtyard(&board, PcbLayerId::BCrtYd);
        assert_hull(&board, &expected_hull);

        board.footprint_flip(fp, Vector2I::new(0, 0), FlipDirection::LeftRight);
        expected_courtyard.mirror(Vector2I::new(0, 0), FlipDirection::LeftRight);
        expected_hull.mirror(Vector2I::new(0, 0), FlipDirection::LeftRight);

        // Flipped courtyard
        assert_courtyard(&board, PcbLayerId::BCrtYd, &expected_courtyard);
        assert_no_courtyard(&board, PcbLayerId::FCrtYd);
        assert_hull(&board, &expected_hull);
    }

    /// `test_footprint.cpp` `FootprintBoundingBoxWithoutMandatoryFields`, first half: hidden
    /// reference / value fields stay out of the box (removing the fields needs the non-const
    /// `Remove` path of a later package; the text-variable half is M19's).
    #[test]
    fn footprint_bounding_box_with_hidden_mandatory_fields() {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        let shape_size = mm(3.0);

        add_shape(
            &mut board,
            fp,
            None,
            ShapeT::Rectangle,
            PcbLayerId::FCu,
            mm(0.1),
            |s, env| {
                s.set_start(env, Vector2I::new(0, 0));
                s.set_end(env, Vector2I::new(shape_size, shape_size));
            },
        );

        let ctx = BoardCtx::bare(&board);
        let ids = [FieldT::Reference, FieldT::Value].map(|t| board[fp].field(&ctx, t).unwrap());

        for id in ids {
            board.modify_field(id, |f, _| f.text.text.set_visible(false));
        }

        let ctx = BoardCtx::bare(&board);
        let with_fields = board[fp].bounding_box(&ctx);
        let shape_box = ctx.item(board[fp].drawings[0]).unwrap().bounding_box();
        let mut expected = Box2I::from_pos(Vector2I::new(0, 0));
        expected.inflate(mm(0.25));
        expected.merge(&shape_box);
        assert_eq!(with_fields, expected);
        // the cache answers the second call
        assert_eq!(board[fp].bounding_box(&ctx), with_fields);
    }

    /// L5 (Don-Inductor:L_Wuerth_WE-HCI_Flat_1040) from the ticket 2491 board
    /// (`drc/test_footprint_courtyard_chaining.cpp`). Four segments are shorter than the 0.02mm
    /// chaining epsilon.
    const L5_SEGMENTS: [[f64; 4]; 12] = [
        [-6.025, -1.500000, -6.025, 1.500000],
        [-5.502647, -2.214616, -5.500000, -2.225000],
        [-5.502647, 2.214616, -5.500000, 2.225000],
        [-5.500000, -5.250000, -5.500000, -2.225000],
        [-5.500000, 5.250000, -5.500000, 2.225000],
        [-5.250000, -5.500000, 5.250000, -5.500000],
        [-5.250000, 5.500000, 5.250000, 5.500000],
        [5.500000, -5.250000, 5.500000, -2.221321],
        [5.500000, -2.221321, 5.502647, -2.214616],
        [5.500000, 5.250000, 5.500000, 2.225000],
        [5.502647, 2.214616, 5.500000, 2.225000],
        [6.025, 1.500000, 6.025, -1.500000],
    ];

    const L5_ARCS: [[f64; 6]; 8] = [
        [
            -6.025000, -1.500000, -5.880489, -1.942586, -5.502647, -2.214616,
        ],
        [
            -5.502647, 2.214616, -5.880490, 1.942586, -6.025000, 1.500000,
        ],
        [
            -5.500000, -5.250000, -5.426777, -5.426777, -5.250000, -5.500000,
        ],
        [
            -5.250000, 5.500000, -5.426777, 5.426777, -5.500000, 5.250000,
        ],
        [
            5.250000, -5.500000, 5.426777, -5.426777, 5.500000, -5.250000,
        ],
        [5.500000, 5.250000, 5.426777, 5.426777, 5.250000, 5.500000],
        [
            5.502647, -2.214616, 5.880489, -1.942586, 6.025000, -1.500000,
        ],
        [6.025000, 1.500000, 5.880490, 1.942586, 5.502647, 2.214616],
    ];

    /// `checkCourtyardCloses`: the shapes added (`ADD_MODE::INSERT`) in `order`.
    fn check_courtyard_closes(order: &[usize]) {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        let width = mm(0.05);
        let layer = PcbLayerId::BCrtYd;

        for &idx in order {
            if idx < L5_SEGMENTS.len() {
                let s = L5_SEGMENTS[idx];
                add_shape(
                    &mut board,
                    fp,
                    None,
                    ShapeT::Segment,
                    layer,
                    width,
                    |sh, env| {
                        sh.set_start(env, pt(s[0], s[1]));
                        sh.set_end(env, pt(s[2], s[3]));
                    },
                );
            } else {
                let a = L5_ARCS[idx - L5_SEGMENTS.len()];
                add_shape(
                    &mut board,
                    fp,
                    None,
                    ShapeT::Arc,
                    layer,
                    width,
                    |sh, env| {
                        sh.set_arc_geometry(env, pt(a[0], a[1]), pt(a[2], a[3]), pt(a[4], a[5]));
                    },
                );
            }
        }

        let ctx = BoardCtx::bare(&board);
        board[fp].build_courtyard_caches(&ctx, None);
        assert_eq!(
            board[fp].courtyard_flags(&ctx),
            CourtyardFlags::default(),
            "order {order:?}"
        );
        assert_eq!(
            board[fp]
                .courtyard(&ctx, PcbLayerId::BCrtYd)
                .outline_count(),
            1,
            "order {order:?}"
        );
    }

    /// `ClosesFromEverySeedShape`: the courtyard closes whatever order its shapes arrive in.
    /// Each rotation seeds from a different shape.
    #[test]
    fn closes_from_every_seed_shape() {
        let count = L5_SEGMENTS.len() + L5_ARCS.len();
        let mut order: Vec<usize> = (0..count).collect();

        for _ in 0..count {
            check_courtyard_closes(&order);
            order.rotate_left(1);
        }
    }

    /// `ClosesInArbitraryShapeOrder`: 64 shuffles of the whole list. The C++ draws them with
    /// `std::shuffle` over `std::mt19937( 42 )`; this uses a Fisher-Yates shuffle over the same
    /// engine (different permutations, same property under test).
    #[test]
    fn closes_in_arbitrary_shape_order() {
        let count = L5_SEGMENTS.len() + L5_ARCS.len();
        let mut order: Vec<usize> = (0..count).collect();
        let mut rng = kicad_core::mt19937::Mt19937::from_seed(42);

        for _ in 0..64 {
            for i in (1..order.len()).rev() {
                let j = (rng.next_u32() as usize) % (i + 1);
                order.swap(i, j);
            }

            check_courtyard_closes(&order);
        }
    }

    /// An open courtyard sets the malformed flag of its side only, and the flags follow the
    /// last build.
    #[test]
    fn malformed_courtyard_flags() {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        add_shape(
            &mut board,
            fp,
            None,
            ShapeT::Segment,
            PcbLayerId::FCrtYd,
            mm(0.05),
            |s, env| {
                s.set_start(env, pt(0.0, 0.0));
                s.set_end(env, pt(1.0, 0.0));
            },
        );

        let ctx = BoardCtx::bare(&board);
        assert_eq!(board[fp].courtyard_flags(&ctx), CourtyardFlags::default());
        let mut errors = 0;
        let mut handler = |_: &str, _: Option<ItemId>, _: Option<ItemId>, _: Vector2I| errors += 1;
        board[fp].build_courtyard_caches(&ctx, Some(&mut handler));
        let flags = board[fp].courtyard_flags(&ctx);
        assert!(flags.malformed_front && !flags.malformed_back);
        assert!(errors > 0);
        assert_eq!(
            board[fp]
                .courtyard(&ctx, PcbLayerId::FCrtYd)
                .outline_count(),
            0
        );
        // GetEffectiveShape on a courtyard layer of a malformed courtyard: an empty compound
        let shape = board[fp].effective_shape(
            &ctx,
            PcbLayerId::FCrtYd,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        assert!(shape.bbox(0) == ShapeCompound::new().bbox(0));
    }

    /// Save order of the footprint children (`FOOTPRINT::cmp_pads` / `cmp_drawings` /
    /// `cmp_zones` in the writer's `std::set`s) against the `order` lines of
    /// `tests/golden/m20_footprints.txt`.
    #[test]
    fn m20_oracle_save_order() {
        let golden = include_str!("../../tests/golden/m20_footprints.txt");
        let board = build_m20_board();
        let ctx = BoardCtx::bare(&board);
        let mut checked = 0;

        for line in golden.lines().filter(|l| l.starts_with("order|board|fp.")) {
            let (head, expected) = line.split_once('=').unwrap();
            let rest = head.strip_prefix("order|board|fp.").unwrap();
            let (fp_key, list) = rest.split_once('.').unwrap();
            let fp_ref = ctx
                .resolve(&kicad_core::Kiid::parse(fp_key).unwrap())
                .unwrap();
            let fp = fp_ref.as_footprint().unwrap();

            let (mut ids, cmp): (
                Vec<ItemId>,
                fn(&BoardCtx<'_>, ItemId, ItemId) -> std::cmp::Ordering,
            ) = match list {
                "pads" => (
                    fp.pads.iter().map(|&p| p.into()).collect(),
                    crate::footprint::compare::footprint_cmp_pads,
                ),
                "drawings" => (
                    fp.drawings.iter().copied().collect(),
                    crate::footprint::compare::footprint_cmp_drawings,
                ),
                "zones" => (
                    fp.zones.iter().map(|&z| z.into()).collect(),
                    crate::footprint::compare::footprint_cmp_zones,
                ),
                _ => continue,
            };

            ids.sort_by(|&a, &b| cmp(&ctx, a, b));
            let actual: Vec<String> = ids
                .iter()
                .map(|&id| ctx.item(id).unwrap().uuid().to_string())
                .collect();
            assert_eq!(format!("[{}]", actual.join(" ")), expected, "{head}");
            checked += 1;
        }

        assert_eq!(checked, 6);
    }

    /// `drc/test_footprint_courtyard_index.cpp` `MatchesBruteForceScan`.
    #[test]
    fn courtyard_index_matches_brute_force_scan() {
        use std::collections::BTreeSet;

        let mut board = Board::new();

        // addFootprint: a single rectangular front courtyard centred on the origin
        // (KI_TEST::DrawRect, four segments), then SetPosition.
        let add_footprint = |board: &mut Board, pos: Vector2I, size: Vector2I| {
            let fp = board.create_footprint();
            let (x_r, x_l) = (size.x / 2, -size.x / 2);
            let (y_t, y_b) = (size.y / 2, -size.y / 2);
            let segs = [
                ((x_l, y_t), (x_r, y_t)),
                ((x_l, y_b), (x_r, y_b)),
                ((x_l, y_b), (x_l, y_t)),
                ((x_r, y_b), (x_r, y_t)),
            ];

            for ((ax, ay), (bx, by)) in segs {
                add_shape(
                    board,
                    fp,
                    None,
                    ShapeT::Segment,
                    PcbLayerId::FCrtYd,
                    mm(0.1),
                    |s, env| {
                        s.set_start(env, Vector2I::new(ax, ay));
                        s.set_end(env, Vector2I::new(bx, by));
                    },
                );
            }

            board.footprint_set_position(fp, pos);
            board.attach_footprint(fp, AddMode::Insert);
        };

        add_footprint(&mut board, pt(0.0, 0.0), pt(2.0, 2.0));
        add_footprint(&mut board, pt(50.0, 0.0), pt(2.0, 2.0));
        add_footprint(&mut board, pt(100.0, 100.0), pt(2.0, 2.0));

        // A footprint whose courtyard is far larger than its body: indexing the body bounds
        // instead of the courtyard bounds would wrongly exclude it from far-reaching queries.
        add_footprint(&mut board, pt(0.0, 50.0), pt(60.0, 60.0));

        // A footprint with no courtyard graphics must never be returned.
        let no_courtyard = board.create_footprint();
        board.footprint_set_position(no_courtyard, pt(50.0, 50.0));
        board.attach_footprint(no_courtyard, AddMode::Insert);

        let ctx = BoardCtx::bare(&board);
        let index = board.footprint_courtyard_index(&ctx);

        let brute_force = |query: &Box2I| -> BTreeSet<FootprintId> {
            let mut result = BTreeSet::new();

            for (id, footprint) in board.footprints() {
                let mut bbox = Box2I::default();
                let mut has_courtyard = false;

                for side in [PcbLayerId::FCu, PcbLayerId::BCu] {
                    let courtyard = footprint.courtyard(&ctx, side);

                    if courtyard.outline_count() == 0 {
                        continue;
                    }

                    if has_courtyard {
                        bbox.merge(&courtyard.bbox(0));
                    } else {
                        bbox = courtyard.bbox(0);
                    }

                    has_courtyard = true;
                }

                if has_courtyard && bbox.intersects(query) {
                    result.insert(id);
                }
            }

            result
        };

        let queries = [
            Box2I::new(pt(-5.0, -5.0), pt(10.0, 10.0)), // around U1 only
            Box2I::new(pt(200.0, 200.0), pt(10.0, 10.0)), // empty region
            Box2I::new(pt(20.0, 40.0), pt(5.0, 5.0)),   // only U4's large courtyard
            Box2I::new(pt(-50.0, -50.0), pt(250.0, 250.0)), // everything
        ];

        for query in &queries {
            let mut found = BTreeSet::new();
            index.query_overlapping(query, &mut |fp| {
                found.insert(fp);
                true
            });
            assert_eq!(found, brute_force(query), "{query:?}");
        }

        // the index is cached until the next time stamp bump
        assert!(Arc::ptr_eq(&index, &board.footprint_courtyard_index(&ctx)));
    }

    /// `FOOTPRINT::cmp_pads`: pad numbers in `StrNumCmp` order, then position.
    #[test]
    fn cmp_pads_number_and_position() {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        let front = Lset::from_layers(&[PcbLayerId::FCu]);

        for (n, (number, x)) in [("10", 0.0), ("2", 1.0), ("2", 0.5)]
            .into_iter()
            .enumerate()
        {
            let u = format!("eeeeeeee-0000-4000-8000-00000000000{n}");
            add_pad(
                &mut board,
                fp,
                &u,
                number,
                PadAttrib::Smd,
                front.clone(),
                PadShape::Rectangle,
                pt(1.0, 1.0),
                pt(x, 0.0),
                0.0,
                0,
            );
        }

        let ctx = BoardCtx::bare(&board);
        let pads: Vec<ItemId> = board[fp].pads.iter().map(|&p| p.into()).collect();
        let cmp = crate::footprint::compare::footprint_cmp_pads;
        assert_eq!(cmp(&ctx, pads[1], pads[0]), std::cmp::Ordering::Less); // "2" < "10"
        assert_eq!(cmp(&ctx, pads[2], pads[1]), std::cmp::Ordering::Less); // x 0.5 < 1
        assert_eq!(cmp(&ctx, pads[0], pads[0]), std::cmp::Ordering::Equal);
    }
}
