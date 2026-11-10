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
use crate::pad::PadAttrib;
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

        let no_draw_items = self.drawings.is_empty() && self.pads.is_empty() && self.zones.is_empty();

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
                    && !text.as_eda_text().is_some_and(|t| t.eda_text().is_visible())
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
                if item.type_() != KicadT::PcbFieldT && item.type_() != KicadT::PcbReferenceImageT
                {
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

            shape.add_shape(Box::new(ShapeSimple::from_line_chain(courtyard.c_outline(0))));
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
        self.bounding_hull(ctx).collide_point(pos, accuracy, None, None)
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
            let Some(shape_id) = id.typed::<PcbShape>() else { continue };
            let shape = &ctx.board[shape_id];
            let layer = shape.base.item.layer;

            if layer == PcbLayerId::BCrtYd {
                list_back.push(shape_id);
                *back_width_histogram.entry(shape.stroke(ctx).width()).or_insert(0) += 1;
            }

            if layer == PcbLayerId::FCrtYd {
                list_front.push(shape_id);
                *front_width_histogram.entry(shape.stroke(ctx).width()).or_insert(0) += 1;
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
            if self.pads.get(pad).is_some_and(|p| p.attribute() == PadAttrib::Pth) {
                self.modify_pad(pad, |p, env| {
                    let mut pad_layers = p.layer_set(&BoardCtx::bare_none()).clone();
                    pad_layers |= &board_copper;
                    p.set_layer_set(env, pad_layers);
                });
            }
        }
    }
}
