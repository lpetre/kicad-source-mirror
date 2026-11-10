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

//! Port of the placement cascades of `pcbnew/footprint.cpp` (`FOOTPRINT::SetPosition`,
//! `SetOrientation`, `Flip`, `SetTransformScale`, `RescaleAroundPoint`,
//! `MoveAnchorPosition`, `SetLayerAndFlip`, `Move`, `Rotate`) as `Board::footprint_*`
//! (`board-model.md` 8.3, package M19). Each updates `Footprint::transform` / `flipped`,
//! builds one `MutEnv` with the new `ParentFrame`, runs the C++ loop over the children in C++
//! order and patches the footprint caches where C++ patches them. They work the same on a
//! created-but-unattached footprint.

use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_geom::geometry::eda_angle::{ANGLE_0, ANGLE_180};
use kicad_geom::trigo::{rotate_point, rotate_point_around};
use kicad_geom::{EdaAngle, FlipDirection, Shape, Vector2I};

use crate::board::Board;
use crate::ctx::{MutEnv, ParentFrame};
use crate::ids::{FieldId, FootprintId, ItemId, ItemKind, PadId, PointId, ZoneId};
use crate::item::ParentRef;
use crate::item_ref::AnyItemMut;
use crate::traits::BoardItemMut;

/// The child lists of a footprint, snapshot before a cascade (the hooks never change them).
struct Children {
    fields: Vec<FieldId>,
    pads: Vec<PadId>,
    zones: Vec<ZoneId>,
    points: Vec<PointId>,
    drawings: Vec<ItemId>,
    bodies: Vec<crate::ids::Body3dId>,
}

/// The drawing kinds whose `FOOTPRINT::SetPosition` / `SetOrientation` / `Flip` branch is
/// `OnFootprintTransformed()` (`PCB_TEXT_T`, `PCB_SHAPE_T`, `PCB_TEXTBOX_T`, `PCB_BARCODE_T`,
/// `PCB_TABLE_T`, `PCB_DIMENSION_T` family); the others (reference images) are moved or
/// rotated in the board frame.
fn follows_transform(kind: ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Text
            | ItemKind::Shape
            | ItemKind::Textbox
            | ItemKind::Barcode
            | ItemKind::Table
            | ItemKind::Dimension
    )
}

impl Board {
    /// The footprint of a handle checked by the caller (`footprint_children`).
    fn cascade_fp(&mut self, fp: FootprintId) -> &mut crate::footprint::Footprint {
        match self.footprints.get_mut(fp) {
            Some(footprint) => footprint,
            None => panic!("stale FootprintId {fp:?}"),
        }
    }

    /// The child lists of `fp` (`None` for a stale handle).
    fn footprint_children(&self, fp: FootprintId) -> Option<Children> {
        let footprint = self.footprints.get(fp)?;
        Some(Children {
            fields: footprint.fields.iter().copied().collect(),
            pads: footprint.pads.iter().copied().collect(),
            zones: footprint.zones.iter().copied().collect(),
            points: footprint.points.iter().copied().collect(),
            drawings: footprint.drawings.iter().copied().collect(),
            bodies: footprint.bodies.iter().copied().collect(),
        })
    }

    /// The `MutEnv` the children of `fp` see now (`GetParentFootprint()` = `fp` in its current
    /// placement). A footprint without a board (`ParentRef::None`) gives the C++ null-board
    /// defaults. Re-caches a stale `LayerEnv` first.
    fn cascade_env(&mut self, fp: FootprintId) -> MutEnv {
        if self.layer_env_stale {
            self.layer_env = Arc::new(self.build_layer_env());
            self.layer_env_stale = false;
        }

        let footprint = &self[fp];
        let frame = ParentFrame {
            footprint: fp,
            transform: footprint.transform,
            layer: footprint.base.layer,
            flipped: footprint.flipped,
        };

        if footprint.base.parent == ParentRef::None {
            return MutEnv {
                parent: Some(frame),
                ..MutEnv::detached()
            };
        }

        MutEnv {
            parent: Some(frame),
            layers: Arc::clone(&self.layer_env),
            max_error: self.design_settings.max_error(),
            board_use: self.board_use,
            pad_primitive_scale: (1.0, 1.0),
        }
    }

    /// Run a leaf mutator on a footprint child without the `modify` bookkeeping (the cascade
    /// does it once for the whole footprint).
    fn with_leaf(&mut self, id: ItemId, f: impl FnOnce(&mut dyn BoardItemMut)) {
        if let Some(mut item) = self.get_mut_raw(id) {
            if let Some(leaf) = item.as_leaf_mut() {
                f(leaf);
            }
        }
    }

    /// The cells of a table (`m_cells`).
    fn table_cell_ids(&self, table: ItemId) -> Vec<ItemId> {
        let Some(table) = table.typed() else {
            return Vec::new();
        };

        self.table_cells(table).map(|(c, _)| c.into()).collect()
    }

    /// `item->OnFootprintTransformed()` on a drawing (`PCB_TABLE::OnFootprintTransformed`:
    /// every cell).
    fn drawing_on_footprint_transformed(&mut self, id: ItemId, env: &MutEnv) {
        if id.kind() == ItemKind::Table {
            for cell in self.table_cell_ids(id) {
                self.with_leaf(cell, |leaf| leaf.on_footprint_transformed(env));
            }
        } else {
            self.with_leaf(id, |leaf| leaf.on_footprint_transformed(env));
        }
    }

    /// The bookkeeping that ends every cascade: the time stamp moves (`board-model.md` 8.5)
    /// and the footprint's time-stamped geometry caches that the C++ kept valid by patching
    /// them (their stamp equals the stamp before the bump) are re-stamped, so that the
    /// patched values are used exactly where C++ uses them.
    fn finish_cascade(&mut self, fp: FootprintId) {
        let old = self.time_stamp;
        self.increment_time_stamp();
        let new = self.time_stamp;

        if let Some(footprint) = self.footprints.get(fp) {
            footprint.geometry_cache.with(|cache| {
                for stamp in [
                    &mut cache.bounding_box_timestamp,
                    &mut cache.text_excluded_bbox_timestamp,
                    &mut cache.hull_timestamp,
                ] {
                    if *stamp != 0 && *stamp == old {
                        *stamp = new;
                    }
                }
            });
        }
    }

    /// `FOOTPRINT::SetPosition( aPos )`.
    pub fn footprint_set_position(&mut self, fp: FootprintId, pos: Vector2I) {
        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        let delta = pos - self.cascade_fp(fp).transform.translate();

        self.cascade_fp(fp).transform.set_translate(pos);
        let env = self.cascade_env(fp);

        for &field in &children.fields {
            self.with_leaf(field.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &pad in &children.pads {
            self.with_leaf(pad.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &zone in &children.zones {
            self.with_leaf(zone.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &item in &children.drawings {
            if follows_transform(item.kind()) {
                self.drawing_on_footprint_transformed(item, &env);
            } else {
                self.with_leaf(item, |leaf| leaf.move_by(&env, delta));
            }
        }

        let footprint = self.cascade_fp(fp);

        footprint.geometry_cache.with(|cache| {
            cache.bounding_box.move_(delta);
            cache.text_excluded_bbox.move_(delta);
            cache.hull.move_(delta);
        });

        // The geometry work has been conserved by using Move(). But the hashes
        // need to be updated, otherwise the cached polygons will still be rebuild.
        footprint.courtyard_cache.update(|cache| {
            cache.back.move_(delta);
            cache.back_hash = cache.back.get_hash();
            cache.front.move_(delta);
            cache.front_hash = cache.front.get_hash();
        });

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::SetOrientation( aNewAngle )`.
    pub fn footprint_set_orientation(&mut self, fp: FootprintId, angle: EdaAngle) {
        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        let angle_change = angle - self.cascade_fp(fp).transform.rotate(); // change in rotation

        let mut new_angle = angle;
        new_angle.normalize180();
        self.cascade_fp(fp).transform.set_rotate(new_angle);

        let rotation_center = self.cascade_fp(fp).transform.translate();
        let env = self.cascade_env(fp);

        for &field in &children.fields {
            self.with_leaf(field.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &pad in &children.pads {
            self.with_leaf(pad.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &zone in &children.zones {
            self.with_leaf(zone.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &item in &children.drawings {
            if follows_transform(item.kind()) {
                self.drawing_on_footprint_transformed(item, &env);
            } else {
                self.with_leaf(item, |leaf| {
                    leaf.rotate(&env, rotation_center, angle_change)
                });
            }
        }

        let footprint = self.cascade_fp(fp);

        footprint
            .geometry_cache
            .with(|cache| cache.text_excluded_bbox_timestamp = 0);

        footprint.courtyard_cache.update(|cache| {
            cache.front.rotate(angle_change, rotation_center);
            cache.front_hash = cache.front.get_hash();

            cache.back.rotate(angle_change, rotation_center);
            cache.back_hash = cache.back.get_hash();
        });

        footprint
            .geometry_cache
            .with(|cache| cache.hull.rotate(angle_change, rotation_center));

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::Flip( aCentre, aFlipDirection )`: mirror the position about the X axis
    /// through `centre`, flip the side, mirror every child's library-frame state
    /// (`TOP_BOTTOM` about the anchor), negate the rotation, refresh the children, patch the
    /// caches, and for `LEFT_RIGHT` rotate 180 degrees about `centre`.
    pub fn footprint_flip(&mut self, fp: FootprintId, centre: Vector2I, dir: FlipDirection) {
        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        // Move footprint to its final position:
        let mut final_pos = self.cascade_fp(fp).transform.translate();

        // Now Flip the footprint.
        // Flipping a footprint is a specific transform: it is not mirrored like a text.
        // We have to change the side, and ensure the footprint rotation is modified according
        // to the transform, because this parameter is used in pick and place files, and when
        // updating the footprint from library.
        // When flipped around the X axis (Y coordinates changed) orientation is negated
        // When flipped around the Y axis (X coordinates changed) orientation is 180 - old
        // orient.
        // Because it is specific to a footprint, we flip around the X axis, and after rotate
        // 180 deg

        final_pos.y = -(final_pos.y - centre.y) + centre.y; // MIRROR( finalPos.y, aCentre.y )

        self.footprint_set_position(fp, final_pos);

        // Flip layer
        let layer = self.cascade_fp(fp).base.layer;
        let flipped_layer = self.cascade_env(fp).layers.flip_layer(layer);
        self.cascade_fp(fp).base.layer = flipped_layer;

        let pos = self.cascade_fp(fp).transform.translate();
        let env = self.cascade_env(fp);

        // Children mirror their lib-frame state directly so the result does not
        // depend on the parent rotation at the time of the call. The parent
        // rotation is negated once at the end.
        for &field in &children.fields {
            self.with_leaf(field.into(), |leaf| {
                leaf.flip(&env, pos, FlipDirection::TopBottom)
            });
        }

        for &pad in &children.pads {
            self.with_leaf(pad.into(), |leaf| {
                leaf.flip(&env, pos, FlipDirection::TopBottom)
            });
        }

        for &zone in &children.zones {
            self.with_leaf(zone.into(), |leaf| {
                leaf.flip(&env, pos, FlipDirection::TopBottom)
            });
        }

        for &item in &children.drawings {
            if item.kind() == ItemKind::Table {
                // PCB_TABLE::Flip (FP-child path: cells, column order, layer)
                self.flip_item(item, pos, FlipDirection::TopBottom);
            } else {
                self.with_leaf(item, |leaf| leaf.flip(&env, pos, FlipDirection::TopBottom));
            }
        }

        // Points move but don't flip layer
        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| {
                leaf.flip(&env, pos, FlipDirection::TopBottom)
            });
        }

        let mut new_orientation = -self.cascade_fp(fp).transform.rotate();
        new_orientation.normalize180();
        self.cascade_fp(fp).transform.set_rotate(new_orientation);

        // Refresh derived caches now that the final rotation is in place.
        let env = self.cascade_env(fp);

        for &field in &children.fields {
            self.with_leaf(field.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &pad in &children.pads {
            self.with_leaf(pad.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &zone in &children.zones {
            self.with_leaf(zone.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        for &item in &children.drawings {
            if follows_transform(item.kind()) {
                self.drawing_on_footprint_transformed(item, &env);
            }
        }

        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        // Swap the courtyard sides, then mirror in the same way as everything else.
        self.cascade_fp(fp).courtyard_cache.update(|cache| {
            std::mem::swap(&mut cache.back, &mut cache.front);
            cache.back.mirror(pos, FlipDirection::TopBottom);
            cache.back_hash = cache.back.get_hash();

            cache.front.mirror(pos, FlipDirection::TopBottom);
            cache.front_hash = cache.front.get_hash();
        });

        for &body in &children.bodies {
            self.with_leaf(body.into(), |leaf| leaf.flip(&env, pos, dir));
        }

        self.cascade_fp(fp)
            .geometry_cache
            .with(|cache| cache.hull.mirror(pos, FlipDirection::TopBottom));

        // Now rotate 180 deg if required
        if dir == FlipDirection::LeftRight {
            self.footprint_rotate(fp, centre, ANGLE_180);
        }

        let footprint = self.cascade_fp(fp);
        footprint
            .geometry_cache
            .with(|cache| cache.text_excluded_bbox_timestamp = 0);

        footprint.flipped = footprint.base.layer == PcbLayerId::BCu;

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::SetTransformScale( aScaleX, aScaleY )`: zero, negative and non-finite
    /// scales are rejected; the children are told the ratios to the old scale.
    pub fn footprint_set_transform_scale(&mut self, fp: FootprintId, scale_x: f64, scale_y: f64) {
        // Reject zero, negative, and non-finite scales: they produce a degenerate transform.
        if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
            return;
        }

        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        let transform = self.cascade_fp(fp).transform;
        let old_sx = transform.scale_x();
        let old_sy = transform.scale_y();
        let ratio_x = scale_x / old_sx;
        let ratio_y = scale_y / old_sy;
        let linear_factor = (scale_x + scale_y) / (old_sx + old_sy);

        self.cascade_fp(fp).transform.set_scale(scale_x, scale_y);

        let anchor = self.cascade_fp(fp).transform.translate();
        let parent_rotate = self.cascade_fp(fp).transform.rotate();
        let env = self.cascade_env(fp);

        let rescale = |leaf: &mut dyn BoardItemMut| {
            leaf.on_footprint_rescaled(&env, ratio_x, ratio_y, linear_factor, anchor, parent_rotate)
        };

        for &pad in &children.pads {
            self.with_leaf(pad.into(), rescale);
        }

        for &field in &children.fields {
            self.with_leaf(field.into(), rescale);
        }

        for &item in &children.drawings {
            if item.kind() == ItemKind::Table {
                // PCB_TABLE::OnFootprintRescaled: every cell
                for cell in self.table_cell_ids(item) {
                    self.with_leaf(cell, rescale);
                }
            } else {
                self.with_leaf(item, rescale);
            }
        }

        for &zone in &children.zones {
            self.with_leaf(zone.into(), rescale);
        }

        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| leaf.on_footprint_transformed(&env));
        }

        let footprint = self.cascade_fp(fp);

        footprint.geometry_cache.with(|cache| {
            cache.bounding_box_timestamp = 0;
            cache.text_excluded_bbox_timestamp = 0;
        });

        footprint.courtyard_cache.clear();

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::RescaleAroundPoint( aCenter, aSx, aSy )`: the transform rescaled about
    /// `centre` (`TRANSFORM_TRS::RescaleAround`), applied as `SetPosition` then
    /// `SetTransformScale`.
    pub fn footprint_rescale_around_point(
        &mut self,
        fp: FootprintId,
        centre: Vector2I,
        sx: f64,
        sy: f64,
    ) {
        let Some(footprint) = self.footprints.get(fp) else {
            return;
        };

        let rescaled = footprint.transform.rescale_around(centre, sx, sy);

        self.footprint_set_position(fp, rescaled.translate());
        self.footprint_set_transform_scale(fp, rescaled.scale_x(), rescaled.scale_y());
    }

    /// `FOOTPRINT::MoveAnchorPosition( aMoveVector )`: move the anchor, keep the children
    /// where they are on the board.
    pub fn footprint_move_anchor_position(&mut self, fp: FootprintId, delta: Vector2I) {
        // Move the reference point of the footprint
        // the footprints elements (pads, outlines, edges .. ) are moved
        // but:
        // - the footprint position is not modified.
        // - the relative (local) coordinates of these items are modified
        // - Draw coordinates are updated
        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        // Update (move) the relative coordinates relative to the new anchor point.
        let mut move_vector = delta;
        rotate_point(&mut move_vector, -self.cascade_fp(fp).transform.rotate());

        let env = self.cascade_env(fp);

        // Update field local coordinates
        for &field in &children.fields {
            self.with_leaf(field.into(), |leaf| leaf.move_by(&env, move_vector));
        }

        // Update the pad local coordinates.
        for &pad in &children.pads {
            self.with_leaf(pad.into(), |leaf| leaf.move_by(&env, move_vector));
        }

        // Update the draw element coordinates.
        for &item in &children.drawings {
            if item.kind() == ItemKind::Table {
                // PCB_TABLE::Move: every cell
                for cell in self.table_cell_ids(item) {
                    self.with_leaf(cell, |leaf| leaf.move_by(&env, move_vector));
                }
            } else {
                self.with_leaf(item, |leaf| leaf.move_by(&env, move_vector));
            }
        }

        // Update the keepout zones
        for &zone in &children.zones {
            self.with_leaf(zone.into(), |leaf| leaf.move_by(&env, move_vector));
        }

        // Update the point local coordinates.
        for &point in &children.points {
            self.with_leaf(point.into(), |leaf| leaf.move_by(&env, move_vector));
        }

        let footprint = self.cascade_fp(fp);

        // Update the 3D models
        for model in &mut footprint.models {
            model.offset.x += PCB_IU_SCALE.iu_to_mm(move_vector.x);
            model.offset.y -= PCB_IU_SCALE.iu_to_mm(move_vector.y);
        }

        footprint.geometry_cache.with(|cache| {
            cache.bounding_box.move_(move_vector);
            cache.text_excluded_bbox.move_(move_vector);
            cache.hull.move_(move_vector);
        });

        // The geometry work have been conserved by using Move(). But the hashes
        // need to be updated, otherwise the cached polygons will still be rebuild.
        footprint.courtyard_cache.update(|cache| {
            cache.back.move_(move_vector);
            cache.back_hash = cache.back.get_hash();
            cache.front.move_(move_vector);
            cache.front_hash = cache.front.get_hash();
        });

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::SetLayerAndFlip( aLayer )`: flip left-right about the position when the
    /// side changes.
    pub fn footprint_set_layer_and_flip(&mut self, fp: FootprintId, layer: PcbLayerId) {
        debug_assert!(layer == PcbLayerId::FCu || layer == PcbLayerId::BCu);

        let Some(footprint) = self.footprints.get(fp) else {
            return;
        };

        if layer != footprint.base.layer {
            let pos = footprint.transform.translate();
            self.footprint_flip(fp, pos, FlipDirection::LeftRight);
        }
    }

    /// `FOOTPRINT::Move( aMoveVector )`.
    pub fn footprint_move(&mut self, fp: FootprintId, delta: Vector2I) {
        if delta.x == 0 && delta.y == 0 {
            return;
        }

        let Some(footprint) = self.footprints.get(fp) else {
            return;
        };

        let newpos = footprint.transform.translate() + delta;
        self.footprint_set_position(fp, newpos);
    }

    /// `FOOTPRINT::Rotate( aRotCentre, aAngle )`: `SetPosition` and `SetOrientation`, then
    /// `KeepUpright()` on the fields and the `PCB_TEXT` drawings.
    pub fn footprint_rotate(&mut self, fp: FootprintId, centre: Vector2I, angle: EdaAngle) {
        if angle == ANGLE_0 {
            return;
        }

        let Some(footprint) = self.footprints.get(fp) else {
            return;
        };

        let orientation = footprint.transform.rotate();
        let new_orientation = orientation + angle;
        let mut newpos = footprint.transform.translate();
        rotate_point_around(&mut newpos, centre, angle);
        self.footprint_set_position(fp, newpos);
        self.footprint_set_orientation(fp, new_orientation);

        let Some(children) = self.footprint_children(fp) else {
            return;
        };

        let env = self.cascade_env(fp);

        for &field in &children.fields {
            if let Some(field) = self.fields.get_mut(field) {
                field.text.keep_upright(&env);
            }
        }

        for &item in &children.drawings {
            if let Some(AnyItemMut::Text(text)) = self.get_mut_raw(item) {
                text.keep_upright(&env);
            }
        }

        self.finish_cascade(fp);
    }

    /// `FOOTPRINT::SetLayer( aLayer )`: the side (`BOARD_ITEM::SetLayer`) and `m_flipped`,
    /// without flipping the children.
    pub fn footprint_set_layer(&mut self, fp: FootprintId, layer: PcbLayerId) {
        let Some(footprint) = self.footprints.get_mut(fp) else {
            return;
        };

        footprint.base.layer = layer;
        footprint.flipped = layer == PcbLayerId::BCu;
        self.increment_time_stamp();
    }
}

#[cfg(test)]
mod tests {
    //! Ports of `qa/tests/pcbnew/test_footprint_transform_sync.cpp` (model cases that need no
    //! file I/O) plus checks of the cache patching and of the C++ child order.

    use super::*;
    use crate::board::AddMode;
    use crate::ctx::BoardCtx;
    use crate::eda_shape::ShapeT;
    use crate::item_ref::OwnedItem;
    use crate::pad::Pad;
    use crate::padstack::{PadAttrib, PadShape};
    use crate::pcb_point::PcbPoint;
    use crate::pcb_shape::PcbShape;
    use crate::pcb_text::PcbText;
    use crate::traits::BoardItem;
    use crate::traits::EdaTextItem;
    use kicad_core::Lset;

    /// `new FOOTPRINT( &board ); board.Add( fp )`.
    fn board_with_fp() -> (Board, FootprintId) {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, AddMode::Append);
        (board, fp)
    }

    /// `CHECK_TRANSFORM_MATCHES_LEGACY`: the transform is the position / orientation with unit
    /// scale.
    fn check_transform_matches_legacy(board: &Board, fp: FootprintId) {
        let ctx = BoardCtx::bare(board);
        let f = &board[fp];
        assert_eq!(f.transform().translate(), f.position(&ctx));
        assert_eq!(f.transform().rotate(), f.orientation());
        assert_eq!(f.transform().scale_x(), 1.0);
        assert_eq!(f.transform().scale_y(), 1.0);
    }

    /// An SMD pad (`PAD::SMDMask()`) of `size` at board position `pos`, added (APPEND).
    fn add_smd_pad(board: &mut Board, fp: FootprintId, pos: Vector2I, size: Vector2I) -> PadId {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pad = Pad::new(&mut board.kiid_generator);
        BoardItem::base_mut(&mut pad).parent = ParentRef::Item(fp.into());
        pad.set_attribute(PadAttrib::Smd);
        pad.set_layer_set(&env, Pad::smd_mask());
        pad.set_shape(PcbLayerId::FCu, PadShape::Circle);
        pad.set_size(&env, PcbLayerId::FCu, size);
        pad.set_position(&env, pos);
        let id = board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
            .unwrap_or_else(|_| panic!("pad refused"));
        id.typed().unwrap()
    }

    #[test]
    fn default_is_identity() {
        let (board, fp) = board_with_fp();
        assert!(board[fp].transform().is_identity());
        check_transform_matches_legacy(&board, fp);
    }

    #[test]
    fn set_position_updates_transform() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, Vector2I::new(1234, -5678));
        check_transform_matches_legacy(&board, fp);
        board.check_invariants().unwrap();
    }

    #[test]
    fn set_orientation_updates_transform() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_orientation(fp, EdaAngle::from_degrees(45.0));
        check_transform_matches_legacy(&board, fp);
        assert_eq!(board[fp].orientation(), EdaAngle::from_degrees(45.0));
    }

    #[test]
    fn move_updates_transform() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, Vector2I::new(100, 200));
        board.footprint_move(fp, Vector2I::new(50, -25));
        check_transform_matches_legacy(&board, fp);
        assert_eq!(board[fp].transform().translate(), Vector2I::new(150, 175));
    }

    #[test]
    fn rotate_updates_transform() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, Vector2I::new(100, 0));
        board.footprint_rotate(fp, Vector2I::new(0, 0), EdaAngle::from_degrees(90.0));
        check_transform_matches_legacy(&board, fp);
        // RotatePoint( (100, 0), (0, 0), 90 deg ): KiCad's y axis points down.
        assert_eq!(board[fp].transform().translate(), Vector2I::new(0, -100));
        assert_eq!(board[fp].orientation(), EdaAngle::from_degrees(90.0));
    }

    #[test]
    fn set_layer_updates_flipped() {
        let (mut board, fp) = board_with_fp();
        assert!(!board[fp].is_flipped());
        board.footprint_set_layer(fp, PcbLayerId::BCu);
        assert!(board[fp].is_flipped() && board[fp].flipped());
        board.footprint_set_layer(fp, PcbLayerId::FCu);
        assert!(!board[fp].is_flipped() && !board[fp].flipped());
    }

    #[test]
    fn pad_effective_shape_follows_footprint_scale() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, Vector2I::new(0, 0));
        let pad = add_smd_pad(
            &mut board,
            fp,
            Vector2I::new(5_000_000, 0),
            Vector2I::new(1_000_000, 1_000_000),
        );

        {
            let ctx = BoardCtx::bare(&board);
            let p = &board[pad];
            assert_eq!(p.shape_pos(&ctx, PcbLayerId::FCu).x, 5_000_000);
            let bbox = BoardItem::effective_shape(
                p,
                &ctx,
                PcbLayerId::FCu,
                kicad_core::Flashing::AlwaysFlashed,
                crate::drc_api::DrcConstraintT::NullConstraint,
            )
            .bbox(0);
            assert_eq!(bbox.centre().x, 5_000_000);
            assert_eq!(bbox.size().x, 1_000_000);
        }

        board.footprint_set_transform_scale(fp, 2.0, 1.0);

        let ctx = BoardCtx::bare(&board);
        let p = &board[pad];
        assert_eq!(p.shape_pos(&ctx, PcbLayerId::FCu).x, 10_000_000);
        let bbox = BoardItem::effective_shape(
            p,
            &ctx,
            PcbLayerId::FCu,
            kicad_core::Flashing::AlwaysFlashed,
            crate::drc_api::DrcConstraintT::NullConstraint,
        )
        .bbox(0);
        assert_eq!(bbox.centre().x, 10_000_000);
        assert_eq!(bbox.size().x, 1_500_000);
        board.check_invariants().unwrap();
    }

    #[test]
    fn pad_bbox_follows_footprint_move_and_rotation() {
        // PadBBoxFollowsFootprintMove and PadBBoxFollowsFootprintRotation
        for rotate in [false, true] {
            let (mut board, fp) = board_with_fp();
            let pad = add_smd_pad(
                &mut board,
                fp,
                Vector2I::new(5_000_000, 0),
                Vector2I::new(1_000_000, 500_000),
            );
            let centre = |board: &Board| {
                let ctx = BoardCtx::bare(board);
                BoardItem::bounding_box(&board[pad], &ctx).centre()
            };
            assert_eq!(centre(&board), Vector2I::new(5_000_000, 0));

            if rotate {
                board.footprint_set_orientation(fp, EdaAngle::from_degrees(90.0));
                assert_eq!(centre(&board), Vector2I::new(0, -5_000_000));
            } else {
                board.footprint_set_position(fp, Vector2I::new(10_000_000, 0));
                assert_eq!(centre(&board).x, 15_000_000);
            }
        }
    }

    #[test]
    fn rescale_around_point() {
        // RescaleAroundPointMovesAnchorByExpectedDelta and RescaleAroundPointComposesScale on
        // a footprint placed by hand (the C++ loads issue18).
        let (mut board, fp) = board_with_fp();
        let orig_anchor = Vector2I::new(120_000_000, 80_000_000);
        board.footprint_set_position(fp, orig_anchor);
        add_smd_pad(
            &mut board,
            fp,
            orig_anchor + Vector2I::new(1_000_000, 0),
            Vector2I::new(500_000, 500_000),
        );

        let centre = Vector2I::new(orig_anchor.x - 50_000, orig_anchor.y + 30_000);
        board.footprint_rescale_around_point(fp, centre, 3.0, 3.0);
        let expected = Vector2I::new(
            kicad_geom::ki_round(centre.x as f64 + 3.0 * (orig_anchor.x - centre.x) as f64),
            kicad_geom::ki_round(centre.y as f64 + 3.0 * (orig_anchor.y - centre.y) as f64),
        );
        assert_eq!(board[fp].transform().translate(), expected);
        assert_eq!(board[fp].transform().scale_x(), 3.0);

        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, orig_anchor);
        let centre = Vector2I::new(orig_anchor.x + 12345, orig_anchor.y - 6789);
        board.footprint_rescale_around_point(fp, centre, 2.0, 2.0);
        board.footprint_rescale_around_point(fp, centre, 0.5, 0.5);
        let t = board[fp].transform();
        assert!((t.scale_x() - 1.0).abs() < 1e-9 && (t.scale_y() - 1.0).abs() < 1e-9);
        assert!((t.translate().x - orig_anchor.x).abs() <= 2);
        assert!((t.translate().y - orig_anchor.y).abs() <= 2);

        // Degenerate scales are rejected.
        board.footprint_set_transform_scale(fp, 0.0, 1.0);
        board.footprint_set_transform_scale(fp, f64::NAN, 1.0);
        assert!((board[fp].transform().scale_x() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn move_anchor_moves_points_like_other_children() {
        let (mut board, fp) = board_with_fp();
        let pad = add_smd_pad(
            &mut board,
            fp,
            Vector2I::new(5_000_000, 2_000_000),
            Vector2I::new(1_000_000, 1_000_000),
        );
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pt = PcbPoint::new(&mut board.kiid_generator);
        pt.base.parent = ParentRef::Item(fp.into());
        pt.set_layer(&env, PcbLayerId::FCu);
        pt.set_position(&env, Vector2I::new(3_000_000, 4_000_000));
        let pt = board
            .footprint_add(fp, OwnedItem::Point(pt), AddMode::Insert)
            .unwrap_or_else(|_| panic!("point refused"));
        let pt: PointId = pt.typed().unwrap();

        let spacing = |board: &Board| {
            let ctx = BoardCtx::bare(board);
            board[pt].position(&ctx) - board[pad].position(&ctx)
        };
        let before = spacing(&board);

        board.footprint_move_anchor_position(fp, Vector2I::new(2_000_000, 1_000_000));

        assert_eq!(spacing(&board), before);
        board.check_invariants().unwrap();
    }

    #[test]
    fn flip_non_cardinal_keeps_pad_and_shape_coincident() {
        let (mut board, fp) = board_with_fp();
        board.footprint_set_position(fp, Vector2I::new(50_000_000, 50_000_000));
        board.footprint_set_orientation(fp, EdaAngle::from_degrees(30.0));

        let coincident = Vector2I::new(53_000_000, 51_000_000);
        let pad = add_smd_pad(
            &mut board,
            fp,
            coincident,
            Vector2I::new(1_000_000, 1_000_000),
        );

        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut seg = PcbShape::new(&mut board.kiid_generator, ShapeT::Segment);
        BoardItem::base_mut(&mut seg).parent = ParentRef::Item(fp.into());
        seg.set_start(&env, coincident);
        seg.set_end(&env, coincident + Vector2I::new(1_000_000, 0));
        seg.set_layer(&env, PcbLayerId::FSilkS);
        let seg = board
            .footprint_add(fp, OwnedItem::Shape(Box::new(seg)), AddMode::Insert)
            .unwrap_or_else(|_| panic!("shape refused"));

        let mut txt = PcbText::for_footprint(Some(PcbLayerId::FCu));
        txt.base.uuid = kicad_core::Kiid::new(&mut board.kiid_generator);
        txt.base.parent = ParentRef::Item(fp.into());
        txt.text.set_text("X");
        EdaTextItem::set_text_pos(&mut txt, &env, coincident);
        let txt = board
            .footprint_add(fp, OwnedItem::Text(Box::new(txt)), AddMode::Insert)
            .unwrap_or_else(|_| panic!("text refused"));

        let mut pt = PcbPoint::new(&mut board.kiid_generator);
        pt.base.parent = ParentRef::Item(fp.into());
        pt.set_layer(&env, PcbLayerId::FCu);
        pt.set_position(&env, coincident);
        let pt = board
            .footprint_add(fp, OwnedItem::Point(pt), AddMode::Insert)
            .unwrap_or_else(|_| panic!("point refused"));

        let positions = |board: &Board| {
            let ctx = BoardCtx::bare(board);
            let seg_start = ctx.item(seg).unwrap().as_shape().unwrap().shape.start();
            let txt_pos = ctx.item(txt).unwrap().as_eda_text().unwrap().text_pos(&ctx);
            let pt_pos = ctx.item(pt).unwrap().position();
            (board[pad].position(&ctx), seg_start, txt_pos, pt_pos)
        };

        let (p, s, t, q) = positions(&board);
        assert_eq!((p, t, q), (s, s, s));

        let pos = board[fp].transform().translate();
        board.footprint_flip(fp, pos, FlipDirection::TopBottom);

        // All footprint children must still coincide after the flip.
        let (p, s, t, q) = positions(&board);
        assert_eq!((p, t, q), (s, s, s));
        assert_eq!(board[fp].base.layer, PcbLayerId::BCu);
        assert!(board[fp].flipped());
        assert_eq!(board[fp].orientation(), EdaAngle::from_degrees(-30.0));

        // DoubleFlipIsIdentity (unscaled): flipping back restores every child.
        board.footprint_flip(fp, pos, FlipDirection::TopBottom);
        let (p2, ..) = positions(&board);
        assert_eq!(p2, coincident);
        assert_eq!(board[fp].base.layer, PcbLayerId::FCu);
        assert_eq!(board[fp].orientation(), EdaAngle::from_degrees(30.0));
        board.check_invariants().unwrap();
    }

    #[test]
    fn boardless_footprint_flip_layers() {
        // BoardlessFootprintFlip (pad, shape, point and the fields; zones, tables, barcodes,
        // dimensions and 3D bodies are covered by their packages' frame tests).
        let (mut board, fp) = board_with_fp();
        let pad = add_smd_pad(
            &mut board,
            fp,
            Vector2I::new(0, 0),
            Vector2I::new(1_000_000, 1_000_000),
        );
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut shape = PcbShape::new(&mut board.kiid_generator, ShapeT::Segment);
        BoardItem::base_mut(&mut shape).parent = ParentRef::Item(fp.into());
        shape.set_layer(&env, PcbLayerId::FSilkS);
        let shape = board
            .footprint_add(fp, OwnedItem::Shape(Box::new(shape)), AddMode::Append)
            .unwrap_or_else(|_| panic!("shape refused"));
        let mut point = PcbPoint::new(&mut board.kiid_generator);
        point.base.parent = ParentRef::Item(fp.into());
        point.set_layer(&env, PcbLayerId::FCrtYd);
        let point = board
            .footprint_add(fp, OwnedItem::Point(point), AddMode::Append)
            .unwrap_or_else(|_| panic!("point refused"));

        board.footprint_flip(fp, Vector2I::new(0, 0), FlipDirection::TopBottom);

        let ctx = BoardCtx::bare(&board);
        assert_eq!(board[fp].layer(&ctx), PcbLayerId::BCu);
        let reference = board[fp]
            .field(&ctx, crate::pcb_field::FieldT::Reference)
            .unwrap();
        assert_eq!(
            ctx.item(reference.into()).unwrap().layer(),
            PcbLayerId::BSilkS
        );
        let pad_layers: Lset = board[pad].layer_set(&ctx);
        for layer in [PcbLayerId::BCu, PcbLayerId::BPaste, PcbLayerId::BMask] {
            assert!(pad_layers.contains(layer));
        }
        assert_eq!(ctx.item(shape).unwrap().layer(), PcbLayerId::BSilkS);
        assert_eq!(ctx.item(point).unwrap().layer(), PcbLayerId::FCrtYd);
    }

    #[test]
    fn cascades_work_on_an_unattached_footprint() {
        // The parser applies (at ..) / (transform ..) before BOARD::Add.
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.footprint_set_position(fp, Vector2I::new(1_000, 2_000));
        board.footprint_set_orientation(fp, EdaAngle::from_degrees(90.0));
        board.footprint_set_transform_scale(fp, 1.5, 0.5);
        board.footprint_flip(fp, Vector2I::new(1_000, 2_000), FlipDirection::LeftRight);
        let t = board[fp].transform();
        assert_eq!(t.translate(), Vector2I::new(1_000, 2_000));
        assert_eq!((t.scale_x(), t.scale_y()), (1.5, 0.5));
        assert!(board[fp].is_flipped());
        board.attach_footprint(fp, AddMode::Append);
        board.check_invariants().unwrap();
    }

    #[test]
    fn set_position_patches_the_caches() {
        // FOOTPRINT::SetPosition moves the cached boxes and the courtyards instead of dropping
        // them; the patched boxes stay valid across the time-stamp bump.
        let (mut board, fp) = board_with_fp();
        let stamp = board.time_stamp();
        board[fp].geometry_cache.with(|c| {
            c.bounding_box = kicad_geom::Box2I::new(Vector2I::new(0, 0), Vector2I::new(10, 10));
            c.bounding_box_timestamp = stamp;
        });
        board.footprint_set_position(fp, Vector2I::new(100, 50));
        let stamp_after = board.time_stamp();
        assert_ne!(stamp, stamp_after);
        board[fp].geometry_cache.with(|c| {
            assert_eq!(c.bounding_box.position(), Vector2I::new(100, 50));
            assert_eq!(c.bounding_box_timestamp, stamp_after);
        });

        board.footprint_set_transform_scale(fp, 2.0, 2.0);
        board[fp]
            .geometry_cache
            .with(|c| assert_eq!(c.bounding_box_timestamp, 0));
    }

    /// One footprint of `tests/data/m19/m19_cascade.kicad_pcb`, rebuilt from its library-frame
    /// children at the identity placement and then placed with the cascades: SetPosition,
    /// SetOrientation, SetTransformScale and, for the back-side copy, Flip( pos, TOP_BOTTOM ).
    /// The library values are the ones the C++ parser stored (the dump's `library_position`;
    /// the text width 1000001 is the parser's round trip through the board frame, deduced from
    /// its `text_size`).
    fn build_cascade_footprint(board: &mut Board, prefix: &str, flip: bool) -> FootprintId {
        let uuid = |n: u32| {
            kicad_core::Kiid::parse(&format!("{prefix}-0000-4000-8000-00000000000{n}")).unwrap()
        };
        let mm = |v: f64| (v * 1_000_000.0).round() as i32;
        let fp = board.create_footprint();
        board.rebind_item_uuid(fp.into(), uuid(0));
        let fpref: ParentRef = ParentRef::Item(fp.into());
        let env = board.child_env(fpref);

        // fields: Reference R1 (0, -2.5), Value V (0, +2.5 front / -2.5 for the flipped copy),
        // board angle 0 in the placed footprint = library angle -30
        let ctx = BoardCtx::bare(board);
        let fields = [
            (
                board[fp]
                    .field(&ctx, crate::pcb_field::FieldT::Reference)
                    .unwrap(),
                "R1",
                -2.5,
            ),
            (
                board[fp]
                    .field(&ctx, crate::pcb_field::FieldT::Value)
                    .unwrap(),
                "V",
                if flip { -2.5 } else { 2.5 },
            ),
        ];

        for (n, (field, text, y)) in fields.into_iter().enumerate() {
            board.rebind_item_uuid(field.into(), uuid(n as u32 + 1));
            board.modify_field(field, |f, _| {
                f.text.text.set_text(text);
                EdaTextItem::set_text_pos(f, &env, Vector2I::new(0, mm(y)));
                EdaTextItem::set_text_size(f, &env, Vector2I::new(1_000_001, 1_000_000), false);
                EdaTextItem::set_text_thickness(f, &env, 150_000);
                EdaTextItem::set_text_angle(f, &env, EdaAngle::from_degrees(330.0));
            });
        }

        let mut seg = PcbShape::new(&mut board.kiid_generator, ShapeT::Segment);
        BoardItem::base_mut(&mut seg).parent = fpref;
        BoardItem::base_mut(&mut seg).uuid = uuid(3);
        seg.set_start(&env, Vector2I::new(0, -1_000_000));
        seg.set_end(&env, Vector2I::new(2_000_000, 1_500_000));
        seg.set_width(&env, 120_000);
        seg.set_layer(&env, PcbLayerId::FSilkS);
        board
            .footprint_add(fp, OwnedItem::Shape(Box::new(seg)), AddMode::Append)
            .unwrap_or_else(|_| panic!("segment refused"));

        let mut circle = PcbShape::new(&mut board.kiid_generator, ShapeT::Circle);
        BoardItem::base_mut(&mut circle).parent = fpref;
        BoardItem::base_mut(&mut circle).uuid = uuid(4);
        circle.set_start(&env, Vector2I::new(-1_000_000, -1_000_000));
        circle.set_end(&env, Vector2I::new(-500_000, -1_000_000));
        circle.set_width(&env, 100_000);
        circle.set_layer(&env, PcbLayerId::FFab);
        board
            .footprint_add(fp, OwnedItem::Shape(Box::new(circle)), AddMode::Append)
            .unwrap_or_else(|_| panic!("circle refused"));

        let mut pad1 = Pad::new(&mut board.kiid_generator);
        BoardItem::base_mut(&mut pad1).parent = fpref;
        BoardItem::base_mut(&mut pad1).uuid = uuid(5);
        pad1.set_number("1");
        pad1.set_attribute(PadAttrib::Smd);
        pad1.set_layer_set(
            &env,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FMask, PcbLayerId::FPaste]),
        );
        pad1.set_shape(PcbLayerId::FCu, PadShape::Rectangle);
        pad1.set_size(&env, PcbLayerId::FCu, Vector2I::new(1_000_000, 600_000));
        pad1.set_position(&env, Vector2I::new(1_000_000, -500_001));
        pad1.set_fp_relative_orientation(&env, EdaAngle::from_degrees(345.0));
        board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad1)), AddMode::Append)
            .unwrap_or_else(|_| panic!("pad refused"));

        let mut pad2 = Pad::new(&mut board.kiid_generator);
        BoardItem::base_mut(&mut pad2).parent = fpref;
        BoardItem::base_mut(&mut pad2).uuid = uuid(6);
        pad2.set_number("2");
        pad2.set_attribute(PadAttrib::Pth);
        pad2.set_layer_set(&env, Pad::pth_mask());
        pad2.set_shape(PcbLayerId::FCu, PadShape::Oval);
        pad2.set_size(&env, PcbLayerId::FCu, Vector2I::new(1_200_000, 800_000));
        pad2.set_drill_shape(crate::padstack::PadDrillShape::Circle);
        pad2.set_drill_size(&env, Vector2I::new(600_000, 600_000));
        pad2.set_position(&env, Vector2I::new(-2_000_000, 1_500_001));
        pad2.set_fp_relative_orientation(&env, EdaAngle::from_degrees(330.0));
        board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad2)), AddMode::Append)
            .unwrap_or_else(|_| panic!("pad refused"));

        let pos = if flip {
            Vector2I::new(120_000_000, 100_000_000)
        } else {
            Vector2I::new(100_000_000, 100_000_000)
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

    /// Differential check against the reference build: `tests/golden/m19_cascade.txt` holds
    /// the `pcb-dump` values of the two footprints of `tests/data/m19/m19_cascade.kicad_pcb`
    /// (front, rotated 30 degrees and scaled 1.5 x 0.75; its back-side copy); the cascades must
    /// reproduce the board-frame (and library-frame) values of every child exactly.
    #[test]
    fn m19_oracle_cascades() {
        let golden = include_str!("../../tests/golden/m19_cascade.txt");
        let mut board = Board::new();
        build_cascade_footprint(&mut board, "aaaaaaaa", false);
        build_cascade_footprint(&mut board, "bbbbbbbb", true);
        board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&board);

        let pt = |v: Vector2I| format!("{},{}", v.x, v.y);
        let angle_eq = |a: EdaAngle, expected: &str| {
            let e: f64 = expected.parse().unwrap();
            let d = (a.as_degrees() - e).rem_euclid(360.0);
            d < 1e-9 || 360.0 - d < 1e-9
        };
        let layer = |l: PcbLayerId| kicad_core::layer_ids::layer_name(l);
        let layers_eq = |set: &Lset, hex: &str| {
            let bits = u128::from_str_radix(hex, 16).unwrap();
            (0..128).all(|i| set.contains(PcbLayerId(i)) == (bits >> i & 1 == 1))
        };
        let mut checked = 0;

        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let (head, value) = line.split_once('=').unwrap();
            let mut parts = head.split('|');
            let (section, key, field) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            let item = ctx.resolve(&kicad_core::Kiid::parse(key).unwrap()).unwrap();
            let ok = match (section, field) {
                ("footprint", _) => {
                    let f = item.as_footprint().unwrap();
                    match field {
                        "position" | "transform.translate" => pt(f.position(&ctx)) == value,
                        "orientation" | "transform.rotate" => angle_eq(f.orientation(), value),
                        "transform.scale" => {
                            let t = f.transform();
                            format!("{},{}", t.scale_x(), t.scale_y()) == value
                        }
                        "layer" => layer(f.layer(&ctx)) == value,
                        "side" => layer(f.side(&ctx)) == value,
                        "flipped" => f.is_flipped().to_string() == value,
                        "likely_attribute" => f.likely_attribute(&ctx).to_string() == value,
                        "reference" => format!("\"{}\"", f.reference(&ctx)) == value,
                        "value" => format!("\"{}\"", f.value(&ctx)) == value,
                        "pad_count" => f.pad_count().to_string() == value,
                        "net_tie" => f.is_net_tie().to_string() == value,
                        _ => continue,
                    }
                }
                ("pad", _) => {
                    let p = item.as_pad().unwrap();
                    match field {
                        "position" => pt(p.position(&ctx)) == value,
                        "library_position" => pt(p.lib_pos()) == value,
                        "orientation" | "padstack.orientation" => {
                            angle_eq(p.orientation(&ctx), value)
                        }
                        "fp_relative_orientation" => {
                            angle_eq(p.fp_relative_orientation(&ctx), value)
                        }
                        "padstack.layerset" => layers_eq(&p.layer_set(&ctx), value),
                        "layer.F.Cu.size" => pt(p.size(&ctx, PcbLayerId::FCu)) == value,
                        "drill_size" => pt(p.drill_size(&ctx)) == value,
                        _ => continue,
                    }
                }
                ("shape", _) => {
                    let s = item.as_shape().unwrap();
                    let segment = s.lib.shape == ShapeT::Segment;
                    match field {
                        "shape" => format!("{:?}", s.shape.shape()).to_uppercase() == value,
                        "start" if segment => pt(s.shape.start()) == value,
                        "end" if segment => pt(s.shape.end()) == value,
                        "width" => crate::traits::EdaShapeItem::width(s, &ctx).to_string() == value,
                        "lib.shape" => format!("{:?}", s.lib.shape).to_uppercase() == value,
                        "lib.start" => pt(s.lib.start) == value,
                        "lib.end" => pt(s.lib.end) == value,
                        "layer" => layer(item.layer()) == value,
                        _ => continue,
                    }
                }
                ("text", _) => {
                    let t = item.as_eda_text().unwrap();
                    match field {
                        "text_pos" => pt(t.text_pos(&ctx)) == value,
                        "text_size" => pt(t.text_size(&ctx)) == value,
                        "text_thickness" => t.text_thickness(&ctx).to_string() == value,
                        "text_angle" => angle_eq(t.text_angle(&ctx), value),
                        "mirrored" => t.eda_text().is_mirrored().to_string() == value,
                        "layer" => layer(item.layer()) == value,
                        _ => continue,
                    }
                }
                _ => continue,
            };

            assert!(ok, "{line}: Rust differs");
            checked += 1;
        }

        assert!(checked >= 90, "only {checked} values compared");
    }
}
