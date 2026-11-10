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

//! Port of `pcbnew/pcb_grid_item.h` and `pcbnew/pcb_grid_item.cpp` (`PCB_GRID_ITEM`, a local
//! grid: editor / router hint, no artwork), `board-model-types.md` section 10. Behaviour:
//! package M23 (`GRID_GEOMETRY`, `FindActiveGridAt` and the frame-angle helpers are editor /
//! router code and not part of the model).

use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Color4D, KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::cmath;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::box_hit_test_shape;
use kicad_geom::geometry::grid_geometry::{GridGeometry, GridKind};
use kicad_geom::math::util::ki_round_to;
use kicad_geom::trigo::{rotate_point, rotate_point_around};
use kicad_geom::{
    ANGLE_0, Box2I, DEGREES_T, EdaAngle, EdaOrientation, FlipDirection, Seg, Shape, ShapeLineChain,
    ShapeNull, ShapePolySet, Vector2D, Vector2I, mirror_point,
};

use crate::board_item::mirror_axis;

/// `MIRROR_AXIS( aFlipDirection )` (`kimath mirror.h`): the angle of the reflection axis.
fn mirror_axis_angle(dir: FlipDirection) -> EdaAngle {
    kicad_geom::mirror_axis(dir)
}

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `PCB_GRID_ITEM`.
#[derive(Clone, Debug)]
pub struct PcbGridItem {
    pub base: BoardItemBase,
    pub type_: PcbGridType, // m_type
    pub pos: Vector2I,      // m_pos
    /// m_extent (cartesian half x/y; polar (rMax, -)).
    pub extent: Vector2I,
    /// m_spacing (cartesian (dx, dy); polar (dr, -)).
    pub spacing: Vector2I,
    pub orientation: EdaOrientation, // m_orientation
    pub phi_extent: EdaAngle,        // m_phiExtent (polar only)
    pub phi_spacing: EdaAngle,       // m_phiSpacing (polar only)
    pub color: Color4D,              // m_color
    /// m_priority: higher wins in overlap resolution; 0 reserved for the global grid.
    pub priority: u32,
    /// m_tickInterval: 0 = no major ticks; N = every Nth line is major.
    pub tick_interval: u32,
    pub affects: PcbGridAffects, // m_affects
}

/// `PCB_GRID_TYPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PcbGridType {
    #[default]
    Cartesian = 0,
    Polar = 1,
}

/// `PCB_GRID_AFFECTS` (all default true).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PcbGridAffects {
    /// Replace the display grid for cursor snapping inside coverage.
    pub cursor: bool,
    /// Used by the router as a local routing frame.
    pub routing: bool,
    /// Used by edit/move tools for placement snap.
    pub placement: bool,
}

impl Default for PcbGridAffects {
    /// The in-class initialisers: all true.
    fn default() -> Self {
        PcbGridAffects {
            cursor: true,
            routing: true,
            placement: true,
        }
    }
}

impl PcbGridAffects {
    /// `All()`.
    pub fn all(&self) -> bool {
        self.cursor && self.routing && self.placement
    }

    /// `SetAll( aState )`.
    pub fn set_all(&mut self, state: bool) {
        self.cursor = state;
        self.routing = state;
        self.placement = state;
    }
}

/// `std::max( a, b )` for doubles: `( a < b ) ? b : a` (keeps a NaN `a`, unlike `f64::max`).
fn std_max_f64(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbGridItem {
    /// `PCB_GRID_ITEM( aParent )` (`pcb_grid_item.cpp`): cartesian, extent (5 mm, 5 mm),
    /// spacing (1 mm, 1 mm), orientation 0, phi extent 360 degrees, phi spacing 10 degrees,
    /// position (0, 0), `COLOR4D()`, priority 1, tick interval 0, affects everything; layer
    /// `F_Cu` (BOARD_ITEM default); nil UUID.
    fn default() -> Self {
        PcbGridItem {
            base: BoardItemBase::default(),
            type_: PcbGridType::Cartesian,
            pos: Vector2I::default(),
            extent: Vector2I::new(PCB_IU_SCALE.mm_to_iu(5.0), PCB_IU_SCALE.mm_to_iu(5.0)),
            spacing: Vector2I::new(PCB_IU_SCALE.mm_to_iu(1.0), PCB_IU_SCALE.mm_to_iu(1.0)),
            orientation: EdaOrientation::new(ANGLE_0),
            phi_extent: EdaAngle::new(360.0, DEGREES_T),
            phi_spacing: EdaAngle::new(10.0, DEGREES_T),
            color: Color4D::default(),
            priority: 1,
            tick_interval: 0,
            affects: PcbGridAffects::default(),
        }
    }
}

impl PcbGridItem {
    /// `new PCB_GRID_ITEM( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut grid = PcbGridItem::default();
        grid.base.uuid = Kiid::new(kiid_generator);
        grid
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbGridItem {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbGridItem {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbGridItemT
    }
    fn class_name(&self) -> &'static str {
        "PCB_GRIDITEM"
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbGridItem::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbGridItem::layer_set(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbGridItem::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbGridItem::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbGridItem::effective_shape(self, ctx, layer, flash, usage)
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
        PcbGridItem::transform_shape_to_polygon(
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
        PcbGridItem::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbGridItem::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbGridItem::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbGridItem {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbGridItem::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbGridItem::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbGridItem::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbGridItem::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbGridItem::mirror(self, env, centre, dir)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbGridItem::set_layer_set(self, env, layers)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbGridItem {
    /// `GetOrientation()`.
    pub fn orientation(&self) -> EdaAngle {
        self.orientation.angle()
    }

    /// `SetOrientation()` (normalising, as `EDA_ORIENTATION::operator=`).
    pub fn set_orientation(&mut self, angle: EdaAngle) {
        self.orientation = EdaOrientation::new(angle);
    }

    /// `GetGridItemType()`.
    pub fn grid_item_type(&self) -> PcbGridType {
        self.type_
    }

    /// `SetGridItemType()`.
    pub fn set_grid_item_type(&mut self, type_: PcbGridType) {
        self.type_ = type_;
    }

    /// `GetExtent()`.
    pub fn extent(&self) -> Vector2I {
        self.extent
    }

    /// `GetSpacing()`.
    pub fn spacing(&self) -> Vector2I {
        self.spacing
    }

    /// `GetPhiExtent()`.
    pub fn phi_extent(&self) -> EdaAngle {
        self.phi_extent
    }

    /// `GetPhiSpacing()`.
    pub fn phi_spacing(&self) -> EdaAngle {
        self.phi_spacing
    }

    /// `GetAssignedPriority()`.
    pub fn assigned_priority(&self) -> u32 {
        self.priority
    }

    /// `GetTickInterval()`.
    pub fn tick_interval(&self) -> u32 {
        self.tick_interval
    }

    /// `SetTickInterval()`.
    pub fn set_tick_interval(&mut self, interval: u32) {
        self.tick_interval = interval;
    }

    /// `Affects()`.
    pub fn affects(&self) -> &PcbGridAffects {
        &self.affects
    }

    /// `SetExtent( aExtent )`: absolute values.
    pub fn set_extent(&mut self, extent: Vector2I) {
        // std::abs of INT_MIN is INT_MIN on the reference platform
        self.extent = Vector2I::new(extent.x.wrapping_abs(), extent.y.wrapping_abs());
    }

    /// `SetSpacing( aSpacing )`: at least 1 in each direction.
    pub fn set_spacing(&mut self, spacing: Vector2I) {
        self.spacing = Vector2I::new(spacing.x.max(1), spacing.y.max(1));
    }

    /// `GetOrientationDegrees()`.
    pub fn orientation_degrees(&self) -> f64 {
        self.orientation.angle().as_degrees()
    }

    /// `SetOrientationDegrees( aDeg )`: `m_orientation = EDA_ANGLE( aDeg, DEGREES_T )`
    /// (normalising, as `EDA_ORIENTATION::operator=`).
    pub fn set_orientation_degrees(&mut self, degrees: f64) {
        self.set_orientation(EdaAngle::new(degrees, DEGREES_T));
    }

    /// `GetRadiusExtent()`: `m_extent.x`.
    pub fn radius_extent(&self) -> i32 {
        self.extent.x
    }

    /// `SetRadiusExtent( aR )`: `m_extent.x = std::abs( aR )`.
    pub fn set_radius_extent(&mut self, radius: i32) {
        // std::abs of INT_MIN is INT_MIN on the reference platform
        self.extent.x = radius.wrapping_abs();
    }

    /// `GetRadiusSpacing()`: `m_spacing.x`.
    pub fn radius_spacing(&self) -> i32 {
        self.spacing.x
    }

    /// `SetRadiusSpacing( aR )`: `m_spacing.x = std::max( aR, 1 )`.
    pub fn set_radius_spacing(&mut self, radius: i32) {
        self.spacing.x = radius.max(1);
    }

    /// `SetPhiExtentDegrees( aDeg )`: `max( |aDeg|, 0.001 )`.
    pub fn set_phi_extent_degrees(&mut self, degrees: f64) {
        self.phi_extent = EdaAngle::new(std_max_f64(degrees.abs(), 0.001), DEGREES_T);
    }

    /// `SetPhiSpacingDegrees( aDeg )`: `max( |aDeg|, 0.001 )`.
    pub fn set_phi_spacing_degrees(&mut self, degrees: f64) {
        self.phi_spacing = EdaAngle::new(std_max_f64(degrees.abs(), 0.001), DEGREES_T);
    }

    /// `SetAssignedPriority( aPriority )`: 0 becomes 1.
    pub fn set_assigned_priority(&mut self, priority: u32) {
        self.priority = if priority != 0 { priority } else { 1 };
    }

    /// `PCB_GRID_ITEM::GetLayer()`: grids have no layer (`wxFAIL_MSG`), `UNDEFINED_LAYER`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbLayerId::UndefinedLayer
    }

    /// `PCB_GRID_ITEM::GetLayerSet()`: `LSET::AllLayersMask()`.
    pub fn layer_set(&self, _ctx: &BoardCtx<'_>) -> Lset {
        Lset::all_layers_mask().clone()
    }

    /// `PCB_GRID_ITEM::SetLayerSet()`: does nothing.
    pub fn set_layer_set(&mut self, _env: &MutEnv, _layers: Lset) {}

    /// `PCB_GRID_ITEM::GetPosition()`: `m_pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_GRID_ITEM::SetPosition()`: `m_pos = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.pos = pos;
    }

    /// `PCB_GRID_ITEM::Move()`: `m_pos += aMoveVector`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.pos += delta;
    }

    /// `PCB_GRID_ITEM::Rotate()`.
    pub fn rotate(&mut self, _env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        rotate_point_around(&mut self.pos, centre, angle);
        self.orientation += angle;
    }

    /// `PCB_GRID_ITEM::Flip()`.
    pub fn flip(&mut self, _env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        let axis = mirror_axis_angle(dir);

        // Move the location and adjust rotation for the flip.
        mirror_point(&mut self.pos, centre, dir);
        self.orientation = self.orientation.reflect(axis);
    }

    /// `PCB_GRID_ITEM::Mirror( aCentre, aFlipDirection )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_axis(env, &mirror_axis(centre, dir));
    }

    /// `PCB_GRID_ITEM::Mirror( const SEG& aAxis )`.
    pub fn mirror_axis(&mut self, _env: &MutEnv, axis: &Seg) {
        self.pos = axis.reflect_point(self.pos);
        self.orientation = self
            .orientation
            .reflect(-EdaAngle::from_vector(axis.b - axis.a));
    }

    /// `PCB_GRID_ITEM::AsGridGeometry()`: the grid as the snapping engine sees it.
    pub fn as_grid_geometry(&self) -> GridGeometry {
        let mut g = GridGeometry {
            origin: Vector2D::from(self.pos),
            orientation: self.orientation.angle().as_radians(),
            priority: self.priority,
            ..GridGeometry::default()
        };

        match self.type_ {
            PcbGridType::Polar => {
                g.kind = GridKind::Polar;
                g.pitch = Vector2D::new(f64::from(self.spacing.x), self.phi_spacing.as_radians());
                g.extent = Vector2D::new(f64::from(self.extent.x), self.phi_extent.as_radians());
            }
            PcbGridType::Cartesian => {
                g.kind = GridKind::Cartesian;
                g.pitch = Vector2D::from(self.spacing);
                g.extent = Vector2D::from(self.extent);
            }
        }

        g
    }

    /// `PCB_GRID_ITEM::buildOutlineWorld()`: the rotated rectangle or polar wedge, closed.
    fn build_outline_world(&self) -> ShapeLineChain {
        let mut outline = ShapeLineChain::new();
        let orientation = self.orientation.angle();
        let pos = self.pos;

        let mut push_world = |mut local: Vector2I| {
            rotate_point(&mut local, orientation);
            outline.append_point(pos + local, false);
        };

        match self.type_ {
            PcbGridType::Polar => {
                let r = self.extent.x;
                let phi_max = self.phi_extent.as_radians();

                if phi_max + 1e-9 < 2.0 * std::f64::consts::PI {
                    push_world(Vector2I::new(0, 0)); // wedge apex
                }

                const K_ARC_SEGMENTS: i32 = 32;

                for i in 0..=K_ARC_SEGMENTS {
                    let phi = phi_max * f64::from(i) / f64::from(K_ARC_SEGMENTS);
                    push_world(Vector2I::new(
                        ki_round_to::<i32, f64>(f64::from(r) * cmath::cos(phi)),
                        ki_round_to::<i32, f64>(f64::from(r) * cmath::sin(phi)),
                    ));
                }
            }
            PcbGridType::Cartesian => {
                push_world(Vector2I::new(-self.extent.x, -self.extent.y));
                push_world(Vector2I::new(self.extent.x, -self.extent.y));
                push_world(Vector2I::new(self.extent.x, self.extent.y));
                push_world(Vector2I::new(-self.extent.x, self.extent.y));
            }
        }

        outline.set_closed(true);
        outline
    }

    /// `PCB_GRID_ITEM::TransformShapeToPolygon()`: does nothing (no artwork).
    pub fn transform_shape_to_polygon(
        &self,
        _ctx: &BoardCtx<'_>,
        _buffer: &mut ShapePolySet,
        _layer: PcbLayerId,
        _clearance: i32,
        _error: i32,
        _error_loc: ErrorLoc,
        _ignore_line_width: bool,
    ) {
    }

    /// `PCB_GRID_ITEM::GetBoundingBox()`.
    pub fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        self.build_outline_world().bbox(0)
    }

    /// `PCB_GRID_ITEM::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        // No board geometry - return a null shape.
        let _ = (ctx, layer, flash);
        Arc::new(ShapeNull)
    }

    /// `PCB_GRID_ITEM::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        // Use the rotated shape (rect/wedge) rather than the axis-aligned bounding box.
        self.as_grid_geometry()
            .contains(Vector2D::from(pos), f64::from(accuracy))
    }

    /// `PCB_GRID_ITEM::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let _ = ctx;
        let mut arect = *rect;
        arect.inflate(accuracy);

        let outline = self.build_outline_world();

        // polygon inside an axis-aligned box  <=>  polygon.BBox inside the box.
        if contained {
            return arect.contains_box(&outline.bbox(0));
        }

        box_hit_test_shape(&outline, &arect, false)
    }

    /// `PCB_GRID_ITEM::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = (ctx, units);
        "Grid Item".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Geometry of `pcb_grid_item.cpp`: the rotated rectangle / wedge outline box, the
    /// `GRID_GEOMETRY::Contains` hit test, rotation, flip and mirror.
    #[test]
    fn grid_item_geometry() {
        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut g = PcbGridItem::default();
        g.type_ = PcbGridType::Cartesian;
        g.pos = Vector2I::new(1000, 0);
        g.extent = Vector2I::new(200, 100);
        assert_eq!(
            g.bounding_box(&ctx),
            Box2I::new(Vector2I::new(800, -100), Vector2I::new(400, 200))
        );
        assert!(g.hit_test(&ctx, Vector2I::new(1199, 99), 0));
        assert!(!g.hit_test(&ctx, Vector2I::new(1199, 101), 0));
        assert!(g.hit_test(&ctx, Vector2I::new(1199, 101), 1));

        let env = MutEnv::detached();
        g.rotate(&env, Vector2I::new(0, 0), EdaAngle::new(90.0, DEGREES_T));
        assert_eq!(g.pos, Vector2I::new(0, -1000));
        assert_eq!(
            g.bounding_box(&ctx),
            Box2I::new(Vector2I::new(-100, -1200), Vector2I::new(200, 400))
        );
        let r = Box2I::new(Vector2I::new(-150, -1250), Vector2I::new(300, 500));
        assert!(g.hit_test_rect(&ctx, &r, true, 0));
        assert!(!g.hit_test_rect(&ctx, &r, true, -60));

        g.flip(&env, Vector2I::new(10, 0), FlipDirection::LeftRight);
        assert_eq!(g.pos, Vector2I::new(20, -1000));
        g.mirror(&env, Vector2I::new(0, 0), FlipDirection::TopBottom);
        assert_eq!(g.pos, Vector2I::new(20, 1000));

        let units = UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(g.item_description(&ctx, &units, false), "Grid Item");
    }

    /// `PCB_GRID_ITEM::PCB_GRID_ITEM( BOARD_ITEM* aParent )` (pcbnew/pcb_grid_item.cpp) and the
    /// in-class initialisers of pcb_grid_item.h.
    #[test]
    fn grid_item_constructor_defaults() {
        let g = PcbGridItem::default();
        assert_eq!(g.grid_item_type(), PcbGridType::Cartesian);
        assert_eq!(g.extent(), Vector2I::new(5_000_000, 5_000_000));
        assert_eq!(g.spacing(), Vector2I::new(1_000_000, 1_000_000));
        assert_eq!(g.orientation(), ANGLE_0);
        assert_eq!(g.phi_extent().as_degrees(), 360.0);
        assert_eq!(g.phi_spacing().as_degrees(), 10.0);
        assert_eq!(g.pos, Vector2I::new(0, 0));
        assert_eq!(g.color, Color4D::default());
        assert_eq!(g.assigned_priority(), 1);
        assert_eq!(g.tick_interval(), 0);
        assert!(g.affects().all());
        assert_eq!(g.base.layer, PcbLayerId::FCu);
        assert!(g.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&g), "PCB_GRIDITEM");
    }

    /// The trivial overrides through the traits.
    #[test]
    fn grid_item_trivial_virtuals() {
        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut g = PcbGridItem::default();
        let env = MutEnv::detached();
        BoardItemMut::set_position(&mut g, &env, Vector2I::new(3, 4));
        BoardItemMut::move_by(&mut g, &env, Vector2I::new(1, 1));
        BoardItemMut::set_layer_set(&mut g, &env, Lset::new());
        assert_eq!(BoardItem::position(&g, &ctx), Vector2I::new(4, 5));
        assert_eq!(BoardItem::layer(&g, &ctx), PcbLayerId::UndefinedLayer);
        assert!(BoardItem::layer_set(&g, &ctx) == *Lset::all_layers_mask());

        g.set_extent(Vector2I::new(-7, 8));
        g.set_spacing(Vector2I::new(0, -3));
        g.set_assigned_priority(0);
        g.set_phi_extent_degrees(-0.0);
        assert_eq!(g.extent(), Vector2I::new(7, 8));
        assert_eq!(g.spacing(), Vector2I::new(1, 1));
        assert_eq!(g.assigned_priority(), 1);
        assert_eq!(g.phi_extent().as_degrees(), 0.001);
    }
}
