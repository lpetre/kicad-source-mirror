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

//! Port of `pcbnew/pcb_target.h` and `pcbnew/pcb_target.cpp` (`PCB_TARGET`, the layer
//! alignment target), `board-model-types.md` section 10. Behaviour: package M23.

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `DEFAULT_COPPER_LINE_WIDTH` (include/board_design_settings_defaults.h), in mm.
const DEFAULT_COPPER_LINE_WIDTH: f64 = 0.2;

/// `PCB_TARGET`.
#[derive(Clone, Debug)]
pub struct PcbTarget {
    pub base: BoardItemBase,
    pub shape: i32,      // m_shape (bit 0: 0 = +, 1 = X)
    pub size: i32,       // m_size
    pub line_width: i32, // m_lineWidth
    pub pos: Vector2I,   // m_pos
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbTarget {
    /// `PCB_TARGET( aParent )` (`pcb_target.cpp`): shape 0 (+), size 5 mm, line width
    /// `DEFAULT_COPPER_LINE_WIDTH`, layer `Edge_Cuts` ("a target is on all layers"), position
    /// (0, 0); nil UUID.
    fn default() -> Self {
        let mut base = BoardItemBase::default();
        base.layer = PcbLayerId::EdgeCuts;
        PcbTarget {
            base,
            shape: 0,
            size: PCB_IU_SCALE.mm_to_iu(5.0), // Gives a decent size
            line_width: PCB_IU_SCALE.mm_to_iu(DEFAULT_COPPER_LINE_WIDTH),
            pos: Vector2I::default(),
        }
    }
}

impl PcbTarget {
    /// `new PCB_TARGET( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut target = PcbTarget::default();
        target.base.uuid = Kiid::new(kiid_generator);
        target
    }

    /// `new PCB_TARGET( aParent, aShape, aLayer, aPos, aSize, aWidth )`.
    pub fn with_params(
        kiid_generator: &mut KiidGenerator,
        shape: i32,
        layer: PcbLayerId,
        pos: Vector2I,
        size: i32,
        width: i32,
    ) -> Self {
        let mut target = PcbTarget::new(kiid_generator);
        target.shape = shape;
        target.base.layer = layer;
        target.pos = pos;
        target.size = size;
        target.line_width = width;
        target
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbTarget {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbTarget {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbTargetT
    }
    fn class_name(&self) -> &'static str {
        "PCB_TARGET"
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTarget::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbTarget::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbTarget::effective_shape(self, ctx, layer, flash, usage)
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
        PcbTarget::transform_shape_to_polygon(
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
        PcbTarget::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbTarget::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbTarget::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbTarget {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbTarget::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbTarget::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbTarget::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTarget::flip(self, env, centre, dir)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbTarget {
    /// `GetShape()`.
    pub fn shape(&self) -> i32 {
        self.shape
    }

    /// `SetShape()`.
    pub fn set_shape(&mut self, shape: i32) {
        self.shape = shape;
    }

    /// `GetSize()`.
    pub fn size(&self) -> i32 {
        self.size
    }

    /// `SetSize()`.
    pub fn set_size(&mut self, size: i32) {
        self.size = size;
    }

    /// `GetWidth()`.
    pub fn width(&self) -> i32 {
        self.line_width
    }

    /// `SetWidth()`.
    pub fn set_width(&mut self, width: i32) {
        self.line_width = width;
    }

    /// `PCB_TARGET::GetPosition()`: `m_pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_TARGET::SetPosition()`: `m_pos = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.pos = pos;
    }

    /// `PCB_TARGET::Move()`: `m_pos += aMoveVector`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.pos += delta;
    }

    /// `PCB_TARGET::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_TARGET::Rotate", env, centre, angle)
    }

    /// `PCB_TARGET::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TARGET::Flip", env, centre, dir)
    }

    /// `PCB_TARGET::GetBoundingBox()`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TARGET::GetBoundingBox", ctx)
    }

    /// `PCB_TARGET::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_TARGET::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TARGET::TransformShapeToPolygon()`.
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
            "PCB_TARGET::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_TARGET::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TARGET::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_TARGET::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TARGET::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `PCB_TARGET::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TARGET::GetItemDescription", ctx, units, full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PCB_TARGET::PCB_TARGET( BOARD_ITEM* aParent )` (pcbnew/pcb_target.cpp).
    #[test]
    fn target_constructor_defaults() {
        let t = PcbTarget::default();
        assert_eq!(t.shape(), 0);
        assert_eq!(t.size(), 5_000_000);
        assert_eq!(t.width(), 200_000);
        assert_eq!(t.base.layer, PcbLayerId::EdgeCuts);
        assert_eq!(t.pos, Vector2I::new(0, 0));
        assert!(t.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&t), "PCB_TARGET");

        let mut kiid_generator = KiidGenerator::default();
        let t = PcbTarget::with_params(
            &mut kiid_generator,
            1,
            PcbLayerId::FCu,
            Vector2I::new(1, 2),
            3,
            4,
        );
        assert_eq!(
            (t.shape, t.base.layer, t.pos, t.size, t.line_width),
            (1, PcbLayerId::FCu, Vector2I::new(1, 2), 3, 4)
        );
        assert!(t.base.uuid != Kiid::default());
    }

    /// The trivial mutators through the trait.
    #[test]
    fn target_move() {
        let mut t = PcbTarget::default();
        let env = MutEnv::detached();
        BoardItemMut::set_position(&mut t, &env, Vector2I::new(10, 20));
        BoardItemMut::move_by(&mut t, &env, Vector2I::new(1, 2));
        assert_eq!(t.pos, Vector2I::new(11, 22));
    }
}
