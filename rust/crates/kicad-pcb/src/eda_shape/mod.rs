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

//! Port of `include/eda_shape.h` and `common/eda_shape.cpp` (`EDA_SHAPE`, `SHAPE_T`, `FILL_T`,
//! `ARC_MID`).
//!
//! `EdaShape` is data only (`board-model.md` 6.2a): everything `EDA_SHAPE` computes through its
//! virtuals goes through the host's `EdaShapeItem` impl. The base bodies of those virtuals and
//! the non-virtual members that call virtuals are generic functions over the host here and in
//! `geometry.rs` / `shapes.rs`, with method syntax through the blanket extension trait
//! `EdaShapeExt` (signatures: package M0c; bodies: M7 geometry, M8 effective shapes and
//! hatching).

pub mod geometry;
pub mod shapes;

use kicad_core::color4d::Color4D;
use kicad_core::{LineStyle, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::ellipse::Ellipse;
use kicad_geom::{Box2I, EdaAngle, Seg, ShapePolySet, Vector2I};

use crate::cache::KeyedCache;
use crate::ctx::BoardCtx;
use crate::item::SharedShape;
use crate::line_ending::LineEnding;
use crate::traits::EdaShapeItem;

/// `SHAPE_T`; the values are part of the file format history.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ShapeT {
    #[default]
    Undefined = -1,
    Segment = 0,
    /// `RECTANGLE` (named so to avoid a Windows header collision).
    Rectangle,
    Arc,
    Circle,
    Poly,
    Bezier,
    Ellipse,
    EllipseArc,
}

/// `FILL_T` (`include/eda_fill.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum FillT {
    #[default]
    NoFill = 1,
    /// Fill with object color.
    FilledShape,
    /// Fill with background body color.
    FilledWithBgBodycolor,
    /// Fill with a separate color.
    FilledWithColor,
    Hatch,
    ReverseHatch,
    CrossHatch,
}

/// `ARC_MID`: holding struct to keep the originating midpoint.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ArcMid {
    pub mid: Vector2I,
    pub start: Vector2I,
    pub end: Vector2I,
    pub center: Vector2I,
}

/// `EDA_SHAPE_HATCH_CACHE_DATA`.
#[derive(Clone, Debug, Default)]
pub struct EdaShapeHatchCacheData {
    pub hatching: ShapePolySet,
    pub hatch_lines: Vec<Seg>,
}

/// `EDA_SHAPE` data (`include/eda_shape.h`). Board frame (the "baked" geometry of a footprint
/// child). Dropped: `m_blankEnding` (a constant: `LineEnding::default()`), `m_editState`
/// (interactive drawing).
#[derive(Clone, Debug)]
pub struct EdaShape {
    /// m_endsSwapped: true if start/end were swapped, e.g. by SetArcAngleAndEnd.
    pub ends_swapped: bool,
    pub shape: ShapeT,            // m_shape
    pub stroke: StrokeParams,     // m_stroke
    pub start_ending: LineEnding, // m_startEnding
    pub end_ending: LineEnding,   // m_endEnding
    pub fill: FillT,              // m_fill
    pub fill_color: Color4D,      // m_fillColor
    pub rectangle_height: i64,    // m_rectangleHeight (long long)
    pub rectangle_width: i64,     // m_rectangleWidth (long long)
    pub corner_radius: i32,       // m_cornerRadius
    /// m_start: line start point or circle centre.
    pub start: Vector2I,
    /// m_end: line end point or circle 3 o'clock point.
    pub end: Vector2I,
    pub arc_center: Vector2I,         // m_arcCenter
    pub arc_mid_data: ArcMid,         // m_arcMidData
    pub bezier_c1: Vector2I,          // m_bezierC1
    pub bezier_c2: Vector2I,          // m_bezierC2
    pub bezier_points: Vec<Vector2I>, // m_bezierPoints
    /// m_ellipse: used only for ELLIPSE / ELLIPSE_ARC.
    pub ellipse: Ellipse<i32>,
    /// m_poly (C++: a lazily allocated `unique_ptr`; empty here until used).
    pub poly: ShapePolySet,
    /// m_proxyItem: a shape storing proxy information (pad number box, thermal spoke template).
    pub proxy_item: bool,
    /// m_hatchingCache + m_hatchingDirty: `invalidate()` is `SetHatchingDirty()`.
    pub(crate) hatching_cache: KeyedCache<EdaShapeHatchCacheData>,
}

impl Default for EdaShape {
    /// `EDA_SHAPE` has no default constructor; this is `EDA_SHAPE( SHAPE_T::UNDEFINED, 0,
    /// FILL_T::NO_FILL )`. Item constructors use [`EdaShape::new`].
    fn default() -> Self {
        EdaShape::new(ShapeT::Undefined, 0, FillT::NoFill)
    }
}

impl EdaShape {
    /// `EDA_SHAPE( aType, aLineWidth, aFill )`.
    pub fn new(shape: ShapeT, line_width: i32, fill: FillT) -> Self {
        EdaShape {
            ends_swapped: false,
            shape,
            stroke: StrokeParams::new(line_width, LineStyle::Default, Color4D::UNSPECIFIED),
            start_ending: LineEnding::default(),
            end_ending: LineEnding::default(),
            fill,
            fill_color: Color4D::UNSPECIFIED,
            rectangle_height: 0,
            rectangle_width: 0,
            corner_radius: 0,
            start: Vector2I::default(),
            end: Vector2I::default(),
            arc_center: Vector2I::default(),
            arc_mid_data: ArcMid::default(),
            bezier_c1: Vector2I::default(),
            bezier_c2: Vector2I::default(),
            bezier_points: Vec::new(),
            ellipse: Ellipse::default(),
            poly: ShapePolySet::default(),
            proxy_item: false,
            hatching_cache: KeyedCache::default(),
        }
    }

    /// Drop the derived hatching (C++ `m_hatchingDirty = true` / cache reset).
    pub fn invalidate_caches(&mut self) {
        self.hatching_cache.clear();
    }

    /// `GetShape()`.
    pub fn shape(&self) -> ShapeT {
        self.shape
    }

    /// `GetFillMode()`.
    pub fn fill_mode(&self) -> FillT {
        self.fill
    }

    /// `IsAnyFill()`.
    pub fn is_any_fill(&self) -> bool {
        self.fill != FillT::NoFill
    }

    /// `IsSolidFill()`.
    pub fn is_solid_fill(&self) -> bool {
        matches!(
            self.fill,
            FillT::FilledShape | FillT::FilledWithColor | FillT::FilledWithBgBodycolor
        )
    }

    /// `IsHatchedFill()`.
    pub fn is_hatched_fill(&self) -> bool {
        matches!(
            self.fill,
            FillT::Hatch | FillT::ReverseHatch | FillT::CrossHatch
        )
    }

    /// `SetHatchingDirty()`.
    pub fn set_hatching_dirty(&self) {
        self.hatching_cache.invalidate();
    }

    /// `GetFillColor()`.
    pub fn fill_color(&self) -> &Color4D {
        &self.fill_color
    }

    /// `GetStart()`.
    pub fn start(&self) -> Vector2I {
        self.start
    }

    /// `GetEnd()`.
    pub fn end(&self) -> Vector2I {
        self.end
    }

    /// `EndsSwapped()`.
    pub fn ends_swapped(&self) -> bool {
        self.ends_swapped
    }

    /// `GetCornerRadius()`.
    pub fn corner_radius(&self) -> i32 {
        self.corner_radius
    }

    /// `GetBezierC1()`.
    pub fn bezier_c1(&self) -> Vector2I {
        self.bezier_c1
    }

    /// `GetBezierC2()`.
    pub fn bezier_c2(&self) -> Vector2I {
        self.bezier_c2
    }

    /// `GetBezierPoints()`.
    pub fn bezier_points(&self) -> &[Vector2I] {
        &self.bezier_points
    }

    /// `GetEllipse()`.
    pub fn ellipse(&self) -> &Ellipse<i32> {
        &self.ellipse
    }

    /// `GetPolyShape()`.
    pub fn poly_shape(&self) -> &ShapePolySet {
        &self.poly
    }

    /// `GetStartEnding()`: a blank ending for closed shapes.
    pub fn start_ending(&self) -> LineEnding {
        crate::port_pending!("EDA_SHAPE::GetStartEnding")
    }

    /// `GetEndEnding()`: a blank ending for closed shapes.
    pub fn end_ending(&self) -> LineEnding {
        crate::port_pending!("EDA_SHAPE::GetEndEnding")
    }

    /// `IsClosed()`.
    pub fn is_closed(&self) -> bool {
        crate::port_pending!("EDA_SHAPE::IsClosed")
    }

    /// `SHAPE_T_asString()`.
    pub fn shape_t_as_string(&self) -> String {
        crate::port_pending!("EDA_SHAPE::SHAPE_T_asString")
    }

    /// `SetStart()` (the `EDA_SHAPE` body: no lib-frame sync).
    pub fn set_start(&mut self, start: Vector2I) {
        self.start = start;
        self.ends_swapped = false;
        self.set_hatching_dirty();
    }

    /// `SetEnd()` (the `EDA_SHAPE` body).
    pub fn set_end(&mut self, end: Vector2I) {
        self.end = end;
        self.ends_swapped = false;
        self.set_hatching_dirty();
    }

    /// `SetShape()` (the `EDA_SHAPE` body).
    pub fn set_shape(&mut self, shape: ShapeT) {
        self.shape = shape;
    }

    /// `SetFillMode()`.
    pub fn set_fill_mode(&mut self, fill: FillT) {
        crate::port_pending!("EDA_SHAPE::SetFillMode", fill)
    }

    /// `SetFillColor()`.
    pub fn set_fill_color(&mut self, color: Color4D) {
        self.fill_color = color;
    }

    /// `SetWidth()` (the `EDA_SHAPE` body).
    pub fn set_width(&mut self, width: i32) {
        crate::port_pending!("EDA_SHAPE::SetWidth", width)
    }

    /// `SetCornerRadius()` (the `EDA_SHAPE` body).
    pub fn set_corner_radius(&mut self, radius: i32) {
        crate::port_pending!("EDA_SHAPE::SetCornerRadius", radius)
    }

    /// `SetIsProxyItem()` (the `EDA_SHAPE` body).
    pub fn set_is_proxy_item(&mut self, is_proxy: bool) {
        self.proxy_item = is_proxy;
    }

    /// `SetPolyShape()` (the `EDA_SHAPE` body).
    pub fn set_poly_shape(&mut self, poly: ShapePolySet) {
        crate::port_pending!("EDA_SHAPE::SetPolyShape", poly)
    }

    /// `SetArcGeometry( aStart, aMid, aEnd )` (the `EDA_SHAPE` body).
    pub fn set_arc_geometry(&mut self, start: Vector2I, mid: Vector2I, end: Vector2I) {
        crate::port_pending!("EDA_SHAPE::SetArcGeometry", start, mid, end)
    }

    /// `SetCenter()` (the `EDA_SHAPE` body).
    pub fn set_center(&mut self, center: Vector2I) {
        crate::port_pending!("EDA_SHAPE::SetCenter", center)
    }

    /// `SetArcAngleAndEnd( aAngle, aCheckNegativeAngle )`.
    pub fn set_arc_angle_and_end(&mut self, angle: EdaAngle, check_negative_angle: bool) {
        crate::port_pending!("EDA_SHAPE::SetArcAngleAndEnd", angle, check_negative_angle)
    }
}

// ---- base bodies of the EDA_SHAPE virtuals (EdaShapeItem defaults) -----------------------

/// `EDA_SHAPE::GetWidth()`: `m_stroke.GetWidth()` (the context is for the `PCB_SHAPE`
/// override, which scales by the parent footprint).
pub fn width<H: EdaShapeItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> i32 {
    host.eda_shape().stroke.width()
}

/// `EDA_SHAPE::IsProxyItem()`: `m_proxyItem`.
pub fn is_proxy_item<H: EdaShapeItem + ?Sized>(host: &H) -> bool {
    host.eda_shape().proxy_item
}

/// `EDA_SHAPE::IsFilledForHitTesting()`: `IsSolidFill()`.
pub fn is_filled_for_hit_testing<H: EdaShapeItem + ?Sized>(host: &H) -> bool {
    host.eda_shape().is_solid_fill()
}

/// `EDA_SHAPE::GetTopLeft()`.
pub fn top_left<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::GetTopLeft", host)
}

/// `EDA_SHAPE::GetBotRight()`.
pub fn bot_right<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::GetBotRight", host)
}

/// `EDA_SHAPE::GetCornersInSequence( aAngle )`.
pub fn corners_in_sequence<H: EdaShapeItem + ?Sized>(host: &H, angle: EdaAngle) -> Vec<Vector2I> {
    crate::port_pending!("EDA_SHAPE::GetCornersInSequence", host, angle)
}

// ---- non-virtual EDA_SHAPE members that call virtuals (EdaShapeExt) ----------------------

/// `EDA_SHAPE::makeEffectiveShapes( aEdgeOnly, aLineChainOnly, aHittesting )`.
pub fn make_effective_shapes<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    edge_only: bool,
    line_chain_only: bool,
    hittesting: bool,
) -> Vec<SharedShape> {
    crate::port_pending!(
        "EDA_SHAPE::makeEffectiveShapes",
        host,
        ctx,
        edge_only,
        line_chain_only,
        hittesting
    )
}

/// `EDA_SHAPE::UpdateHatching()` + `hatching()` / `hatchLines()`: the hatching cache,
/// rebuilt when dirty.
pub fn hatching_data<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
) -> std::sync::Arc<EdaShapeHatchCacheData> {
    crate::port_pending!("EDA_SHAPE::UpdateHatching", host, ctx)
}

/// `EDA_SHAPE::getPosition()`.
pub fn shape_position<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::getPosition", host)
}

/// `EDA_SHAPE::getCenter()`.
pub fn shape_center<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::getCenter", host)
}

/// `EDA_SHAPE::getBoundingBox()`.
pub fn shape_bounding_box<H: EdaShapeItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Box2I {
    crate::port_pending!("EDA_SHAPE::getBoundingBox", host, ctx)
}

/// `EDA_SHAPE::hitTest( aPosition, aAccuracy )`.
pub fn shape_hit_test<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    pos: Vector2I,
    accuracy: i32,
) -> bool {
    crate::port_pending!("EDA_SHAPE::hitTest", host, ctx, pos, accuracy)
}

/// `EDA_SHAPE::hitTest( aRect, aContained, aAccuracy )`.
pub fn shape_hit_test_rect<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    rect: &Box2I,
    contained: bool,
    accuracy: i32,
) -> bool {
    crate::port_pending!(
        "EDA_SHAPE::hitTest (rect)",
        host,
        ctx,
        rect,
        contained,
        accuracy
    )
}

/// `EDA_SHAPE::TransformShapeToPolygon( aBuffer, aClearance, aError, aErrorLoc,
/// ignoreLineWidth, includeFill )`.
#[allow(clippy::too_many_arguments)]
pub fn shape_transform_to_polygon<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
    ignore_line_width: bool,
    include_fill: bool,
) {
    crate::port_pending!(
        "EDA_SHAPE::TransformShapeToPolygon",
        host,
        ctx,
        buffer,
        clearance,
        error,
        error_loc,
        ignore_line_width,
        include_fill
    )
}

/// `EDA_SHAPE::GetLength()`.
pub fn shape_length<H: EdaShapeItem + ?Sized>(host: &H) -> f64 {
    crate::port_pending!("EDA_SHAPE::GetLength", host)
}

/// `EDA_SHAPE::GetRectangleWidth()` / `GetRectangleHeight()`: (width, height).
pub fn rectangle_size<H: EdaShapeItem + ?Sized>(host: &H) -> (i32, i32) {
    crate::port_pending!("EDA_SHAPE::GetRectangleWidth", host)
}

/// The non-virtual `EDA_SHAPE` members that call the virtuals, with method syntax. Defined
/// here and blanket-implemented so they cannot be overridden (`board-model.md` 6.2a).
pub trait EdaShapeExt: EdaShapeItem {
    /// `makeEffectiveShapes( aEdgeOnly, aLineChainOnly, aHittesting )`.
    fn make_effective_shapes(
        &self,
        ctx: &BoardCtx<'_>,
        edge_only: bool,
        line_chain_only: bool,
        hittesting: bool,
    ) -> Vec<SharedShape> {
        make_effective_shapes(self, ctx, edge_only, line_chain_only, hittesting)
    }

    /// `GetHatching()` (after `UpdateHatching()`).
    fn hatching(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        hatching_data(self, ctx).hatching.clone()
    }

    /// `GetHatchLines()` (after `UpdateHatching()`).
    fn hatch_lines(&self, ctx: &BoardCtx<'_>) -> Vec<Seg> {
        hatching_data(self, ctx).hatch_lines.clone()
    }

    /// `getPosition()`.
    fn shape_position(&self) -> Vector2I {
        shape_position(self)
    }

    /// `getCenter()`.
    fn shape_center(&self) -> Vector2I {
        shape_center(self)
    }

    /// `getBoundingBox()`.
    fn shape_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        shape_bounding_box(self, ctx)
    }

    /// `hitTest( aPosition, aAccuracy )`.
    fn shape_hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        shape_hit_test(self, ctx, pos, accuracy)
    }

    /// `hitTest( aRect, aContained, aAccuracy )`.
    fn shape_hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        shape_hit_test_rect(self, ctx, rect, contained, accuracy)
    }

    /// `EDA_SHAPE::TransformShapeToPolygon( .. )`.
    #[allow(clippy::too_many_arguments)]
    fn shape_transform_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
        include_fill: bool,
    ) {
        shape_transform_to_polygon(
            self,
            ctx,
            buffer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
            include_fill,
        )
    }

    /// `GetLength()`.
    fn shape_length(&self) -> f64 {
        shape_length(self)
    }

    /// `GetRectangleWidth()`, `GetRectangleHeight()`.
    fn rectangle_size(&self) -> (i32, i32) {
        rectangle_size(self)
    }
}

impl<T: EdaShapeItem + ?Sized> EdaShapeExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_core::PCB_IU_SCALE;

    /// `EDA_SHAPE( aType, aLineWidth, aFill )` (common/eda_shape.cpp) as PCB_SHAPE calls it.
    #[test]
    fn constructor_values() {
        let s = EdaShape::new(ShapeT::Segment, PCB_IU_SCALE.mm_to_iu(0.1), FillT::NoFill);
        assert_eq!(s.shape, ShapeT::Segment);
        assert_eq!(s.stroke.width(), 100_000);
        assert_eq!(s.stroke.line_style(), LineStyle::Default);
        assert_eq!(s.fill, FillT::NoFill);
        assert_eq!(s.fill_color, Color4D::UNSPECIFIED);
        assert_eq!(
            (s.rectangle_height, s.rectangle_width, s.corner_radius),
            (0, 0, 0)
        );
        assert!(!s.ends_swapped && !s.proxy_item);
        assert_eq!(s.start_ending, LineEnding::default());
        assert_eq!(
            (ShapeT::Undefined as i32, ShapeT::EllipseArc as i32),
            (-1, 7)
        );
        assert_eq!((FillT::NoFill as i32, FillT::CrossHatch as i32), (1, 7));
    }
}
