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

//! Port of `pcbnew/pcb_point.h` and `pcbnew/pcb_point.cpp` (`PCB_POINT`),
//! `board-model-types.md` section 10.
//!
//! Frames (`board-model.md` 5): `lib_pos` is the library-frame position and authoritative;
//! `pos` is the baked board-frame copy, refreshed by `on_footprint_transformed`
//! (`recomputePosition`). `PCB_POINT::SetParent` also recomputes: in Rust the parent is written
//! by `Board`, which calls `on_footprint_transformed` after adding a point to a footprint
//! (M1 / M19). Behaviour: package M23.

use std::cmp::Ordering;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::ids::ItemId;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `DEFAULT_PT_SIZE_MM` (pcbnew/pcb_point.cpp).
const DEFAULT_PT_SIZE_MM: f64 = 1.0;

/// `PCB_POINT`.
#[derive(Clone, Debug)]
pub struct PcbPoint {
    pub base: BoardItemBase,
    pub(crate) lib_pos: Vector2I, // m_libPos
    /// m_pos (baked board position, refreshed by on_footprint_transformed).
    pub(crate) pos: Vector2I,
    pub size: i32, // m_size
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbPoint {
    /// `PCB_POINT( aParent )` (`pcb_point.cpp`): lib and board position (0, 0), size
    /// `DEFAULT_PT_SIZE_MM` (1 mm), layer `F_Cu`; `recomputePosition()` without a parent
    /// footprint copies `m_libPos`; nil UUID.
    fn default() -> Self {
        PcbPoint {
            base: BoardItemBase::default(),
            lib_pos: Vector2I::new(0, 0),
            pos: Vector2I::new(0, 0),
            size: PCB_IU_SCALE.mm_to_iu(DEFAULT_PT_SIZE_MM),
        }
    }
}

impl PcbPoint {
    /// `new PCB_POINT( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut point = PcbPoint::default();
        point.base.uuid = Kiid::new(kiid_generator);
        point
    }

    /// `new PCB_POINT( aParent, aPos, aSize )`: `SetPosition( aPos )` against the parent frame
    /// that `env` describes (`MutEnv::detached()` for a board-level point).
    pub fn with_position(
        kiid_generator: &mut KiidGenerator,
        env: &MutEnv,
        pos: Vector2I,
        size: i32,
    ) -> Self {
        let mut point = PcbPoint::new(kiid_generator);
        point.size = size;
        point.set_position(env, pos);
        point
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbPoint {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbPoint {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbPointT
    }
    fn class_name(&self) -> &'static str {
        "PCB_POINT"
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbPoint::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbPoint::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbPoint::effective_shape(self, ctx, layer, flash, usage)
    }
    fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        PcbPoint::transform_shape_to_polygon(
            self,
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        )
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbPoint::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbPoint::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbPoint::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbPoint {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbPoint::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbPoint::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbPoint::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbPoint::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbPoint::mirror(self, env, centre, dir)
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        PcbPoint::on_footprint_transformed(self, env)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbPoint {
    /// `GetLibraryPosition()`: `m_libPos` (library frame; the writer reads it).
    pub fn lib_pos(&self) -> Vector2I {
        self.lib_pos
    }

    /// `GetSize()`.
    pub fn size(&self) -> i32 {
        self.size
    }

    /// `SetSize()`.
    pub fn set_size(&mut self, size: i32) {
        self.size = size;
    }

    /// `PCB_POINT::GetPosition()`: the baked board position `m_pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_POINT::SetPosition()`: `m_libPos` from the parent footprint's inverse transform
    /// (or `aPos` without one), then `recomputePosition()`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PCB_POINT::SetPosition", env, pos)
    }

    /// `PCB_POINT::Move()`: `SetPosition( GetPosition() + aMoveVector )`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let pos = self.pos + delta;
        self.set_position(env, pos);
    }

    /// `PCB_POINT::OnFootprintTransformed()`: `recomputePosition()` (`m_pos` = the parent
    /// footprint's transform applied to `m_libPos`, or `m_libPos` without one).
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_POINT::OnFootprintTransformed", env)
    }

    /// `PCB_POINT::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_POINT::Rotate", env, centre, angle)
    }

    /// `PCB_POINT::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_POINT::Flip", env, centre, dir)
    }

    /// `PCB_POINT::Mirror( aCentre, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_POINT::Mirror", env, centre, dir)
    }

    /// `PCB_POINT::GetBoundingBox()`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_POINT::GetBoundingBox", ctx)
    }

    /// `PCB_POINT::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_POINT::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_POINT::TransformShapeToPolygon()`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PCB_POINT::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_POINT::HitTest( aPosition, aAccuracy )`: on the bars of the X.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_POINT::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_POINT::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_POINT::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `PCB_POINT::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_POINT::GetItemDescription", ctx, units, full)
    }
}

/// `PCB_POINT::cmp_points` (save order): layer, position x, y, size, UUID, then pointer order
/// (`ItemId` order, `board-model.md` 3.1). `a` and `b` are points.
pub fn pcb_point_cmp_points(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    crate::port_pending!("PCB_POINT::cmp_points", ctx, a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_POINT::PCB_POINT( BOARD_ITEM* aParent )` (pcbnew/pcb_point.cpp).
    #[test]
    fn point_constructor_defaults() {
        let p = PcbPoint::default();
        assert_eq!(p.lib_pos(), Vector2I::new(0, 0));
        assert_eq!(p.pos, Vector2I::new(0, 0));
        assert_eq!(p.size(), 1_000_000);
        assert_eq!(p.base.layer, PcbLayerId::FCu);
        assert!(p.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&p), "PCB_POINT");

        let mut kiid_generator = KiidGenerator::default();
        assert!(PcbPoint::new(&mut kiid_generator).base.uuid != Kiid::default());
    }

    /// A point in the rotated / flipped / scaled fixture footprint reads its baked position.
    #[test]
    fn point_in_footprint_reads_baked_position() {
        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Point).typed::<PcbPoint>().unwrap();
        let mut p = PcbPoint::default();
        p.lib_pos = Vector2I::new(1_000, 2_000);
        p.pos = Vector2I::new(7, 8);
        p.set_size(500);
        fx.set_child(id, p);
        fx.board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&fx.board);
        let p: &dyn BoardItem = &fx.board[id];
        assert_eq!(p.position(&ctx), Vector2I::new(7, 8));
        assert_eq!(fx.board[id].lib_pos(), Vector2I::new(1_000, 2_000));
        assert_eq!(fx.board[id].size(), 500);
    }
}
