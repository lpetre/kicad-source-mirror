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
use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::box_hit_test_box;
use kicad_geom::trigo::rotate_point_around;
use kicad_geom::{
    Box2I, EdaAngle, FlipDirection, Seg, Shape, ShapeCircle, ShapePolySet, ShapeRect, Vector2I,
    mirror_point,
};

use crate::board_item::mirror_axis;

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
        if let Some(fp) = &env.parent {
            self.lib_pos = fp.transform.inverse_apply(pos);
        } else {
            self.lib_pos = pos;
        }

        self.recompute_position(env);
    }

    /// `PCB_POINT::recomputePosition()`: `m_pos` = the parent footprint's transform applied to
    /// `m_libPos`, or `m_libPos` without a parent footprint.
    fn recompute_position(&mut self, env: &MutEnv) {
        if let Some(fp) = &env.parent {
            self.pos = fp.transform.apply(self.lib_pos);
        } else {
            self.pos = self.lib_pos;
        }
    }

    /// `PCB_POINT::Move()`: `SetPosition( GetPosition() + aMoveVector )`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let pos = self.pos + delta;
        self.set_position(env, pos);
    }

    /// `PCB_POINT::OnFootprintTransformed()`: `recomputePosition()` (`m_pos` = the parent
    /// footprint's transform applied to `m_libPos`, or `m_libPos` without one).
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        self.recompute_position(env);
    }

    /// `PCB_POINT::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut new_pos = self.pos;
        rotate_point_around(&mut new_pos, centre, angle);
        self.set_position(env, new_pos);
    }

    /// `PCB_POINT::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        if let Some(fp) = &env.parent {
            // Mirror the library-frame position (rotation-independent).
            let lib_axis = fp.transform.inverse_apply(centre);
            mirror_point(&mut self.lib_pos, lib_axis, dir);
            self.recompute_position(env);
        } else {
            let mut new_pos = self.pos;
            mirror_point(&mut new_pos, centre, dir);
            self.set_position(env, new_pos);
            // SetLayer( GetFlippedLayer( GetLayer() ) ): BOARD_ITEM::SetLayer.
            self.base.layer = env.layers.flip_layer(self.base.layer);
        }
    }

    /// `PCB_POINT::Mirror( aCentre, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_axis(env, &mirror_axis(centre, dir));
    }

    /// `PCB_POINT::Mirror( const SEG& aAxis )`: `SetPosition( aAxis.ReflectPoint( GetPosition() ) )`.
    pub fn mirror_axis(&mut self, env: &MutEnv, axis: &Seg) {
        let reflected = axis.reflect_point(self.pos);
        self.set_position(env, reflected);
    }

    /// `PCB_POINT::GetBoundingBox()`.
    pub fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        Box2I::by_center(self.pos, Vector2I::new(self.size, self.size))
    }

    /// `PCB_POINT::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let _ = (layer, flash);
        Arc::new(ShapeRect::from_box(&self.bounding_box(ctx)))
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
        // Points have no physical representation: nothing is added.
        let _ = (
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        );
    }

    /// `PCB_POINT::HitTest( aPosition, aAccuracy )`: on the bars of the X.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        // Compute the hit on the bars of the X
        let size = self.size / 2;
        let p = self.pos;
        let loc = Seg::new(pos, pos);

        let seg1 = Seg::new(p - Vector2I::new(size, size), p + Vector2I::new(size, size));
        let seg2 = Seg::new(
            p - Vector2I::new(size, -size),
            p + Vector2I::new(size, -size),
        );
        let circle = ShapeCircle::new(p, size / 2);

        seg1.collide(&loc, accuracy)
            || seg2.collide(&loc, accuracy)
            || circle.collide_seg(&loc, accuracy, None, None)
    }

    /// `PCB_POINT::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        box_hit_test_box(rect, &self.bounding_box(ctx), contained, accuracy)
    }

    /// `PCB_POINT::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = (ctx, units);
        "Point".to_string()
    }

    /// `PCB_POINT::operator==( const PCB_POINT& )`: position and size.
    pub fn eq_item(&self, other: &PcbPoint) -> bool {
        self.pos == other.pos && self.size == other.size
    }
}

/// `PCB_POINT::cmp_points` (save order): layer, position x, y, size, UUID, then pointer order
/// (`ItemId` order, `board-model.md` 3.1). `a` and `b` are points.
pub fn pcb_point_cmp_points(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let pa: &PcbPoint = &ctx.board[a.typed::<PcbPoint>().expect("cmp_points: a is not a point")];
    let pb: &PcbPoint = &ctx.board[b.typed::<PcbPoint>().expect("cmp_points: b is not a point")];

    // GetLayer() is the BOARD_ITEM body (m_layer); GetPosition() the baked board position.
    if pa.base.layer != pb.base.layer {
        return pa.base.layer.cmp(&pb.base.layer);
    }

    if pa.pos.x != pb.pos.x {
        return pa.pos.x.cmp(&pb.pos.x);
    }

    if pa.pos.y != pb.pos.y {
        return pa.pos.y.cmp(&pb.pos.y);
    }

    if pa.size != pb.size {
        return pa.size.cmp(&pb.size);
    }

    if pa.base.uuid != pb.base.uuid {
        return pa.base.uuid.cmp(&pb.base.uuid);
    }

    // a < b on the pointers: ItemId order (board-model.md 3.1).
    a.cmp(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SetPosition` / `recomputePosition` / `Flip` in the rotated, flipped and scaled fixture
    /// footprint: the library position is the inverse transform, the baked position the
    /// forward one, and a flip mirrors the library position about the transformed axis.
    #[test]
    fn point_frames_and_transforms() {
        let fx = rotated_flipped_scaled();
        let fp = fx.child(ItemKind::Point).typed::<PcbPoint>().unwrap();
        let env = fx.board.mut_env(fp.into());
        let frame = env.parent.expect("the fixture point is a footprint child");

        let mut p = PcbPoint::default();
        p.set_position(&env, Vector2I::new(5_000_000, -3_000_000));
        assert_eq!(
            p.lib_pos(),
            frame
                .transform
                .inverse_apply(Vector2I::new(5_000_000, -3_000_000))
        );
        assert_eq!(p.pos, frame.transform.apply(p.lib_pos()));

        let lib_before = p.lib_pos();
        p.flip(&env, Vector2I::new(1_000_000, 0), FlipDirection::LeftRight);
        let axis = frame.transform.inverse_apply(Vector2I::new(1_000_000, 0));
        assert_eq!(
            p.lib_pos(),
            Vector2I::new(2 * axis.x - lib_before.x, lib_before.y)
        );
        assert_eq!(p.base.layer, PcbLayerId::FCu); // footprint children keep their layer

        // Board level: position = library position, flip also flips the layer.
        let env = MutEnv::detached();
        let mut q = PcbPoint::default();
        q.set_position(&env, Vector2I::new(10, 20));
        q.rotate(
            &env,
            Vector2I::new(0, 0),
            EdaAngle::new(90.0, kicad_geom::DEGREES_T),
        );
        assert_eq!(q.pos, Vector2I::new(20, -10));
        q.flip(&env, Vector2I::new(0, 0), FlipDirection::TopBottom);
        assert_eq!(
            (q.pos, q.lib_pos()),
            (Vector2I::new(20, 10), Vector2I::new(20, 10))
        );
        assert_eq!(q.base.layer, PcbLayerId::BCu);
        q.mirror(&env, Vector2I::new(5, 0), FlipDirection::LeftRight);
        assert_eq!(q.pos, Vector2I::new(-10, 10));
    }

    /// `PCB_POINT::HitTest` (the bars of the X and the circle) and `cmp_points`.
    #[test]
    fn point_hit_test_and_save_order() {
        let mut board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut p = PcbPoint::default();
        p.set_position(&MutEnv::detached(), Vector2I::new(0, 0));
        p.set_size(1000);
        assert!(p.hit_test(&ctx, Vector2I::new(300, 300), 0)); // on a bar
        assert!(p.hit_test(&ctx, Vector2I::new(249, 0), 0)); // in the circle (r = 250, strict)
        assert!(!p.hit_test(&ctx, Vector2I::new(250, 0), 0));
        assert!(!p.hit_test(&ctx, Vector2I::new(400, 0), 0));
        assert!(!p.hit_test(&ctx, Vector2I::new(400, 0), 150)); // 400 < 250 + 150 is strict
        assert!(p.hit_test(&ctx, Vector2I::new(400, 0), 151));

        let mut add = |layer, x, size, n: u8| {
            let mut q = PcbPoint::default();
            q.base.layer = layer;
            q.set_position(&MutEnv::detached(), Vector2I::new(x, 0));
            q.set_size(size);
            q.base.uuid =
                Kiid::parse(&format!("00000000-0000-0000-0000-0000000000{n:02}")).unwrap();
            ItemId::from(board.add_point(q, crate::board::AddMode::BulkAppend))
        };
        let a = add(PcbLayerId::BCu, 0, 10, 1);
        let b = add(PcbLayerId::FCu, 5, 10, 2);
        let c = add(PcbLayerId::FCu, 5, 9, 3);
        let d = add(PcbLayerId::FCu, 5, 9, 2);
        let ctx = BoardCtx::bare(&board);
        let mut ids = vec![a, b, c, d];
        ids.sort_by(|x, y| pcb_point_cmp_points(&ctx, *x, *y));
        assert_eq!(ids, vec![d, c, b, a]);
    }
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
