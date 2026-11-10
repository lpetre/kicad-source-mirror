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
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, Box2I, EdaAngle, Seg, ShapePolySet, Vector2I,
    rotate_point_around,
};

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
        if self.is_closed() {
            LineEnding::default()
        } else {
            self.start_ending.clone()
        }
    }

    /// `GetEndEnding()`: a blank ending for closed shapes.
    pub fn end_ending(&self) -> LineEnding {
        if self.is_closed() {
            LineEnding::default()
        } else {
            self.end_ending.clone()
        }
    }

    /// `IsClosed()`: false for undefined shapes (C++ `UNIMPLEMENTED_FOR`).
    pub fn is_closed(&self) -> bool {
        match self.shape {
            ShapeT::Circle | ShapeT::Rectangle | ShapeT::Ellipse => true,
            ShapeT::Arc | ShapeT::Segment | ShapeT::EllipseArc => false,
            ShapeT::Poly => {
                if self.poly.is_empty() {
                    false
                } else {
                    self.poly.c_outline(0).is_closed()
                }
            }
            ShapeT::Bezier => {
                if self.bezier_points.len() < 3 {
                    false
                } else {
                    self.bezier_points[0] == self.bezier_points[self.bezier_points.len() - 1]
                }
            }
            ShapeT::Undefined => false,
        }
    }

    /// `SHAPE_T_asString()`.
    pub fn shape_t_as_string(&self) -> String {
        match self.shape {
            ShapeT::Segment => "S_SEGMENT",
            ShapeT::Rectangle => "S_RECT",
            ShapeT::Arc => "S_ARC",
            ShapeT::Circle => "S_CIRCLE",
            ShapeT::Poly => "S_POLYGON",
            ShapeT::Bezier => "S_CURVE",
            ShapeT::Ellipse => "S_ELLIPSE",
            ShapeT::EllipseArc => "S_ELLIPSE_ARC",
            ShapeT::Undefined => "UNDEFINED",
        }
        .to_string()
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
        self.fill = fill;
        self.set_hatching_dirty();
    }

    /// `SetFillColor()`.
    pub fn set_fill_color(&mut self, color: Color4D) {
        self.fill_color = color;
    }

    /// `SetWidth()` (the `EDA_SHAPE` body).
    pub fn set_width(&mut self, width: i32) {
        self.stroke.set_width(width);
        self.set_hatching_dirty();
    }

    /// `SetCornerRadius()` (the `EDA_SHAPE` body).
    pub fn set_corner_radius(&mut self, radius: i32) {
        if self.shape == ShapeT::Rectangle {
            let width = self.rectangle_width().abs();
            let height = self.rectangle_height().abs();
            let max_radius = width.min(height) / 2;

            self.corner_radius = radius.clamp(0, max_radius);
        } else {
            self.corner_radius = radius;
        }

        self.set_hatching_dirty();
    }

    /// `SetIsProxyItem()` (the `EDA_SHAPE` body).
    pub fn set_is_proxy_item(&mut self, is_proxy: bool) {
        self.proxy_item = is_proxy;
    }

    /// `SetPolyShape()` (the `EDA_SHAPE` body).
    pub fn set_poly_shape(&mut self, poly: ShapePolySet) {
        self.poly = poly;

        for ii in 0..self.poly.outline_count() {
            if self.poly.hole_count(ii) != 0 {
                self.poly.fracture(true);
                break;
            }
        }

        self.set_hatching_dirty();
    }

    /// `SetArcGeometry( aStart, aMid, aEnd )` (the `EDA_SHAPE` body).
    pub fn set_arc_geometry(&mut self, start: Vector2I, mid: Vector2I, end: Vector2I) {
        self.set_arc_geometry_impl(start, mid, end);
    }

    /// `SetCenter()` (the `EDA_SHAPE` body).
    pub fn set_center(&mut self, center: Vector2I) {
        match self.shape {
            ShapeT::Arc => self.arc_center = center,
            ShapeT::Circle => {
                // Route through SetStart / SetEnd so subclasses sync lib coords.
                let delta = center - self.start;
                self.set_end(self.end + delta);
                self.set_start(center);
                self.set_hatching_dirty();
            }
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                self.ellipse.center = center;
                self.set_hatching_dirty();
                self.recalc_ellipse_arc_endpoints();
            }
            _ => {}
        }
    }

    /// `SetArcAngleAndEnd( aAngle, aCheckNegativeAngle )`.
    pub fn set_arc_angle_and_end(&mut self, angle: EdaAngle, check_negative_angle: bool) {
        self.set_arc_angle_and_end_impl(angle, check_negative_angle);
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
pub fn top_left<H: EdaShapeItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> Vector2I {
    host.eda_shape().start
}

/// `EDA_SHAPE::GetBotRight()`.
pub fn bot_right<H: EdaShapeItem + ?Sized>(host: &H, _ctx: &BoardCtx<'_>) -> Vector2I {
    host.eda_shape().end
}

/// `EDA_SHAPE::GetCornersInSequence( aAngle )`. `getBoundingBox()` reads the virtual
/// `GetWidth()` (`PCB_SHAPE`'s is scaled by the parent footprint), hence the context.
pub fn corners_in_sequence<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    mut angle: EdaAngle,
) -> Vec<Vector2I> {
    let es = host.eda_shape();
    let mut pts: Vec<Vector2I> = Vec::new();

    angle.normalize();

    let mut bbox = shape_bounding_box(host, ctx);
    bbox.normalize();

    let tl = Vector2I::new(bbox.left(), bbox.top());
    let tr = Vector2I::new(bbox.right(), bbox.top());
    let br = Vector2I::new(bbox.right(), bbox.bottom());
    let bl = Vector2I::new(bbox.left(), bbox.bottom());

    if angle.is_cardinal() {
        if angle == ANGLE_0 {
            pts.extend([tl, tr, br, bl]);
        } else if angle == ANGLE_90 {
            pts.extend([bl, tl, tr, br]);
        } else if angle == ANGLE_180 {
            pts.extend([br, bl, tl, tr]);
        } else if angle == ANGLE_270 {
            pts.extend([tr, br, bl, tl]);
        }
    } else if es.shape == ShapeT::Rectangle {
        // Axis-aligned rectangle with non-cardinal rotation (used by textboxes).
        let center = bbox.center();
        let (mut tl, mut tr, mut br, mut bl) = (tl, tr, br, bl);

        rotate_point_around(&mut tl, center, angle);
        rotate_point_around(&mut tr, center, angle);
        rotate_point_around(&mut br, center, angle);
        rotate_point_around(&mut bl, center, angle);

        pts.extend([tl, tr, br, bl]);
    } else {
        // This function was originally located in pcb_textbox.cpp and was later moved to
        // eda_shape.cpp. As a result of this move, access to getCorners was lost, since it is
        // defined in the PCB_SHAPE class within pcb_shape.cpp and is not available in the
        // current context.
        //
        // Additionally, GetRectCorners() cannot be used here, as it assumes the rectangle is
        // rotated by a cardinal angle. In non-cardinal cases, it returns incorrect values
        // (e.g., (0, 0)).
        //
        // To address this, a portion of the getCorners implementation for SHAPE_T::POLY
        // elements has been replicated here to restore the correct behavior.
        let mut corners: Vec<Vector2I> = Vec::new();

        for ii in 0..es.poly.outline_count() {
            corners.extend_from_slice(es.poly.c_outline(ii).c_points());
        }

        if corners.is_empty() {
            return pts;
        }

        while corners.len() < 4 {
            let last = corners[corners.len() - 1];
            corners.push(last + Vector2I::new(10, 10));
        }

        let mut min_x = corners[0];
        let mut max_x = corners[0];
        let mut min_y = corners[0];
        let mut max_y = corners[0];

        for corner in &corners {
            if corner.x < min_x.x {
                min_x = *corner;
            }

            if corner.x > max_x.x {
                max_x = *corner;
            }

            if corner.y < min_y.y {
                min_y = *corner;
            }

            if corner.y > max_y.y {
                max_y = *corner;
            }
        }

        if angle < ANGLE_90 {
            pts.extend([min_x, min_y, max_x, max_y]);
        } else if angle < ANGLE_180 {
            pts.extend([max_y, min_x, min_y, max_x]);
        } else if angle < ANGLE_270 {
            pts.extend([max_x, max_y, min_x, min_y]);
        } else {
            pts.extend([min_y, max_x, max_y, min_x]);
        }
    }

    pts
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
    shapes::make_effective_shapes_list(host, ctx, edge_only, line_chain_only, hittesting)
        .into_iter()
        .map(SharedShape::from)
        .collect()
}

/// `EDA_SHAPE::UpdateHatching()` + `hatching()` / `hatchLines()`: the hatching cache,
/// rebuilt when dirty.
pub fn hatching_data<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
) -> std::sync::Arc<EdaShapeHatchCacheData> {
    // `GetHatching()` / `GetHatchLines()` call `EDA_SHAPE::UpdateHatching()` non-virtually:
    // rebuilt only while dirty (the cache is dropped by every edit that sets it dirty).
    host.eda_shape()
        .hatching_cache
        .get_or_build(|_| true, || shapes::build_hatching(host, ctx))
}

/// `EDA_SHAPE::getPosition()`.
pub fn shape_position<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    host.eda_shape().position()
}

/// `EDA_SHAPE::getCenter()`.
pub fn shape_center<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    host.eda_shape().center()
}

/// `EDA_SHAPE::getBoundingBox()`.
pub fn shape_bounding_box<H: EdaShapeItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Box2I {
    let mut bbox = host.eda_shape().bounding_box_core();

    bbox.inflate(0.max(host.width(ctx)) / 2);
    bbox.normalize();

    bbox
}

/// `EDA_SHAPE::hitTest( aPosition, aAccuracy )`.
pub fn shape_hit_test<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    pos: Vector2I,
    accuracy: i32,
) -> bool {
    geometry::hit_test_point(host, ctx, pos, accuracy)
}

/// `EDA_SHAPE::hitTest( aRect, aContained, aAccuracy )`.
pub fn shape_hit_test_rect<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    rect: &Box2I,
    contained: bool,
    accuracy: i32,
) -> bool {
    geometry::hit_test_rect(host, ctx, rect, contained, accuracy)
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
    shapes::transform_shape_to_polygon(
        host,
        ctx,
        buffer,
        clearance,
        error,
        error_loc,
        ignore_line_width,
        include_fill,
    )
}

/// `EDA_SHAPE::GetLength()`.
pub fn shape_length<H: EdaShapeItem + ?Sized>(host: &H) -> f64 {
    host.eda_shape().length()
}

/// `EDA_SHAPE::GetRectangleWidth()` / `GetRectangleHeight()`: (width, height).
pub fn rectangle_size<H: EdaShapeItem + ?Sized>(host: &H) -> (i32, i32) {
    let es = host.eda_shape();
    (es.rectangle_width(), es.rectangle_height())
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

    /// `EDA_SHAPE_MOCK` of qa/tests/common/test_eda_shape.cpp: an `EDA_SHAPE` host with no
    /// overrides.
    struct Mock(EdaShape);

    impl EdaShapeItem for Mock {
        fn eda_shape(&self) -> &EdaShape {
            &self.0
        }

        fn eda_shape_mut(&mut self) -> &mut EdaShape {
            &mut self.0
        }
    }

    fn mock(shape: ShapeT) -> EdaShape {
        EdaShape::new(shape, 0, FillT::NoFill)
    }

    fn v(x: i32, y: i32) -> Vector2I {
        Vector2I::new(x, y)
    }

    fn within(a: Vector2I, b: Vector2I, tol: i32) -> bool {
        (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
    }

    /// `SHAPE_ARC::DefaultAccuracyForPCB()`.
    const ACC: i32 = 5000;

    /// `SetAngleAndEnd` (qa/tests/common/test_eda_shape.cpp).
    #[test]
    fn set_angle_and_end() {
        let cases = [
            // Issue 13626: clockwise semicircle
            (
                v(-428880000, 117229160),
                v(-430060565, 113472820),
                180.0,
                v(-431241130, 109716480),
                false,
            ),
            // Issue 13626: anticlockwise arc
            (
                v(-431241130, 109716480),
                v(-434923630, 112954230),
                -138.46654568595355,
                v(-439827050, 112936200),
                true,
            ),
        ];

        for (start, center, angle, expected_end, swapped) in cases {
            let mut shape = mock(ShapeT::Arc);
            shape.set_start(start);
            shape.set_center(center);
            shape.set_arc_angle_and_end(EdaAngle::from_degrees(angle), true);
            assert_eq!(shape.ends_swapped(), swapped);
            let new_end = if shape.ends_swapped() {
                shape.start()
            } else {
                shape.end()
            };
            assert!(within(new_end, expected_end, ACC));
        }
    }

    /// `SetArcGeometry` (qa/tests/common/test_eda_shape.cpp).
    #[test]
    fn set_arc_geometry() {
        let cases = [
            // Issue 15694: clockwise arc
            (
                v(10000000, 0),
                v(0, 10000000),
                v(-10000000, 0),
                false,
                v(-10000000, 0),
            ),
            // Issue 15694: anticlockwise arc
            (
                v(-10000000, 0),
                v(0, 10000000),
                v(10000000, 0),
                true,
                v(10000000, 0),
            ),
        ];

        for (start, mid, end, swapped, end_after) in cases {
            let mut shape = mock(ShapeT::Arc);
            shape.set_arc_geometry(start, mid, end);
            assert!(within(shape.center(), v(0, 0), ACC));
            assert!((shape.radius() - 10000000).abs() <= ACC);
            assert_eq!(shape.ends_swapped(), swapped);
            let new_end = if shape.ends_swapped() {
                shape.start()
            } else {
                shape.end()
            };
            assert!(within(new_end, end_after, ACC));
            let angle = shape.arc_angle().as_degrees();
            let d = (angle - 180.0).rem_euclid(360.0);
            assert!(d.min(360.0 - d) <= 0.1);
        }
    }

    /// `SetArcGeometryKeepsMidWhenEndsSwap` (issue 10442).
    #[test]
    fn set_arc_geometry_keeps_mid_when_ends_swap() {
        let mid = v(6000000, 8000000);

        for swap in [false, true] {
            let mut shape = mock(ShapeT::Arc);

            if swap {
                shape.set_arc_geometry(v(-10000000, 0), mid, v(10000000, 0));
            } else {
                shape.set_arc_geometry(v(10000000, 0), mid, v(-10000000, 0));
            }

            assert_eq!(shape.ends_swapped(), swap);
            assert_eq!(shape.arc_mid(), mid);
        }
    }

    /// `GetPolyPointsPreservesOrderAcrossOutlines`.
    #[test]
    fn get_poly_points_preserves_order_across_outlines() {
        let mut shape = mock(ShapeT::Poly);
        let poly = &mut shape.poly;
        poly.new_outline();
        poly.append_point(v(0, 0), -1, -1);
        poly.append_point(v(1000, 0), -1, -1);
        poly.append_point(v(1000, 1000), -1, -1);
        poly.new_outline();
        poly.append_point(v(5000, 5000), -1, -1);
        poly.append_point(v(6000, 5000), -1, -1);

        assert_eq!(
            shape.poly_points(),
            vec![
                v(0, 0),
                v(1000, 0),
                v(1000, 1000),
                v(5000, 5000),
                v(6000, 5000)
            ]
        );
    }

    /// `EllipseBasicAccessors`, `EllipseArcIsOpenCurve`, `EllipsePerimeterForCircleCase`.
    #[test]
    fn ellipse_accessors_closedness_and_perimeter() {
        let mut e = mock(ShapeT::Ellipse);
        e.set_ellipse_center(v(100, 200));
        e.set_ellipse_major_radius(500);
        e.set_ellipse_minor_radius(300);
        e.set_ellipse_rotation(EdaAngle::from_degrees(30.0));
        assert_eq!(e.ellipse().center, v(100, 200));
        assert_eq!(
            (e.ellipse().major_radius, e.ellipse().minor_radius),
            (500, 300)
        );
        assert!((e.ellipse().rotation.as_degrees() - 30.0).abs() < 1e-9);
        assert!(e.is_closed());
        // recalcEllipseArcEndpoints: start = centre, end = major-axis end point
        assert_eq!(e.start(), v(100, 200));
        assert_eq!(e.end(), v(100 + 433, 200 + 250));

        let mut arc = mock(ShapeT::EllipseArc);
        arc.set_ellipse_center(v(0, 0));
        arc.set_ellipse_major_radius(500);
        arc.set_ellipse_minor_radius(300);
        arc.set_ellipse_rotation(EdaAngle::from_degrees(0.0));
        arc.set_ellipse_start_angle(EdaAngle::from_degrees(0.0));
        arc.set_ellipse_end_angle(EdaAngle::from_degrees(180.0));
        assert!(!arc.is_closed());
        assert_eq!((arc.start(), arc.end()), (v(500, 0), v(-500, 0)));

        let mut c = mock(ShapeT::Ellipse);
        c.set_ellipse_center(v(0, 0));
        c.set_ellipse_major_radius(1000);
        c.set_ellipse_minor_radius(1000);
        c.set_ellipse_rotation(EdaAngle::from_degrees(0.0));
        let expected = 2.0 * std::f64::consts::PI * 1000.0;
        assert!((c.length() - expected).abs() <= expected * 1e-8);
    }

    /// `PCBShapeCornerRadius` (qa/tests/pcbnew/test_shape_corner_radius.cpp), on the
    /// `EDA_SHAPE` body that `PCB_SHAPE::SetCornerRadius` calls.
    #[test]
    fn corner_radius_is_clamped() {
        let mut shape = mock(ShapeT::Rectangle);
        shape.set_start(v(0, 0));
        shape.set_end(v(1000, 600));
        shape.set_corner_radius(200);
        assert_eq!(shape.corner_radius(), 200);
        shape.set_corner_radius(400);
        assert_eq!(shape.corner_radius(), 300);
        shape.set_corner_radius(-10);
        assert_eq!(shape.corner_radius(), 0);
    }

    /// Oracle: qa/data/pcbnew/issue2512.kicad_pcb, `pcb-dump board --sections shape,bbox`,
    /// gr_arc 6ba5cc1c-0aa9-403c-8c78-baeb88b53289 (a legacy arc: centre + start + angle,
    /// loaded through SetCenter / SetStart / SetArcAngleAndEnd): fields start, end, center,
    /// mid, radius, arc_angle, arc_angles, clockwise, length, bbox.
    #[test]
    fn oracle_issue2512_arc() {
        let mut s = EdaShape::new(ShapeT::Arc, 100000, FillT::NoFill);
        s.set_center(v(143103600, 129946400));
        s.set_start(v(142240001, 129387600));
        s.set_arc_angle_and_end(EdaAngle::from_degrees(350.9270568), true);

        assert_eq!(s.start(), v(142240001, 129387600));
        assert_eq!(s.end(), v(142162688, 129530774));
        assert_eq!(s.center(), v(143103600, 129946400));
        assert_eq!(s.arc_mid(), v(144008691, 130435144));
        assert_eq!(s.radius(), 1028621);
        assert_eq!(
            format!("{:?}", s.arc_angle().as_degrees()),
            "350.92705684512566"
        );
        let (a0, a1) = s.calc_arc_angles();
        assert_eq!(
            format!("{:?},{:?}", a0.as_degrees(), a1.as_degrees()),
            "-147.09472681706927,203.83233002805642"
        );
        assert!(!s.is_clockwise_arc());
        assert_eq!(format!("{:?}", s.length()), "6300131.409446479");

        let board = crate::board::Board::default();
        let ctx = BoardCtx::bare(&board);
        let bb = Mock(s).shape_bounding_box(&ctx);
        assert_eq!(
            (bb.x(), bb.y(), bb.width(), bb.height()),
            (142024979, 128867779, 2157242, 2157242)
        );
    }

    /// Oracle: qa/data/pcbnew/issue18839.kicad_pcb, arc 00000000-0000-0000-0000-000000000001
    /// (mid, arc_angles, length, bbox) and issue2512.kicad_pcb gr_rect
    /// 2ff03438-48dd-4ff9-86ab-4aae55d6a1a4 (bbox, rect_size, corners) and gr_poly
    /// abd486ea-b329-4192-afc3-c44eb6e87cea (bbox).
    #[test]
    fn oracle_bboxes() {
        let board = crate::board::Board::default();
        let ctx = BoardCtx::bare(&board);
        let tuple = |b: Box2I| (b.x(), b.y(), b.width(), b.height());

        let mut arc = EdaShape::new(ShapeT::Arc, 100000, FillT::NoFill);
        arc.set_start(v(105000000, 100000000));
        arc.set_end(v(100000000, 105000000));
        arc.set_center(v(100000000, 100000000));
        assert_eq!(arc.arc_mid(), v(103535534, 103535534));
        let (a0, a1) = arc.calc_arc_angles();
        assert_eq!((a0.as_degrees(), a1.as_degrees()), (0.0, 90.0));
        assert_eq!(format!("{:?}", arc.length()), "7853981.633974483");
        assert_eq!(
            tuple(Mock(arc).shape_bounding_box(&ctx)),
            (99950000, 99950000, 5100000, 5100000)
        );

        let mut rect = EdaShape::new(ShapeT::Rectangle, 10000, FillT::NoFill);
        rect.set_start(v(136245600, 125577600));
        rect.set_end(v(147864000, 131757600));
        let host = Mock(rect);
        assert_eq!(
            tuple(host.shape_bounding_box(&ctx)),
            (136240600, 125572600, 11628400, 6190000)
        );
        assert_eq!(host.rectangle_size(), (11618400, 6180000));
        assert_eq!(
            host.eda_shape().rect_corners(),
            vec![
                v(136245600, 125577600),
                v(147864000, 125577600),
                v(147864000, 131757600),
                v(136245600, 131757600)
            ]
        );

        let mut poly = EdaShape::new(ShapeT::Poly, 100000, FillT::FilledShape);
        poly.set_poly_points(&[
            v(144373600, 127304800),
            v(142595600, 127304800),
            v(144373600, 128270000),
            v(144373600, 128574800),
            v(142087600, 128574800),
            v(142087600, 126288800),
            v(144373600, 126288800),
        ]);
        poly.poly.outline(0).set_closed(true);
        assert!(poly.is_closed());
        assert_eq!(poly.point_count(), 7);
        assert_eq!(
            tuple(Mock(poly).shape_bounding_box(&ctx)),
            (142037600, 126238800, 2386000, 2386000)
        );
    }

    /// Transforms, `Compare` and `operator==` on hand-checked values (`move`, `rotate`,
    /// `flip`, `mirror`, `scale` of common/eda_shape.cpp).
    #[test]
    fn transforms_compare_and_equality() {
        let mut seg = mock(ShapeT::Segment);
        seg.set_start(v(0, 0));
        seg.set_end(v(1000, 0));
        let orig = seg.clone();

        seg.move_(v(10, 20));
        assert_eq!((seg.start(), seg.end()), (v(10, 20), v(1010, 20)));
        assert_eq!(seg.compare(&orig), 10);
        assert!(seg != orig);

        seg.move_(v(-10, -20));
        assert_eq!(seg.compare(&orig), 0);
        assert!(seg == orig);

        seg.rotate(v(0, 0), EdaAngle::from_degrees(90.0), 100);
        assert_eq!(seg.end(), v(0, -1000));

        seg.flip(v(0, 0), kicad_geom::FlipDirection::TopBottom, 100);
        assert_eq!(seg.end(), v(0, 1000));

        seg.scale(2.0, 100);
        assert_eq!(seg.end(), v(0, 2000));

        // Non-cardinal rotation turns a rectangle into a polygon.
        let mut rect = mock(ShapeT::Rectangle);
        rect.set_start(v(0, 0));
        rect.set_end(v(1000, 500));
        rect.rotate(v(0, 0), EdaAngle::from_degrees(45.0), 100);
        assert_eq!(rect.shape(), ShapeT::Poly);
        assert_eq!(rect.point_count(), 4);

        // Mirroring an arc keeps its geometry consistent.
        let mut arc = mock(ShapeT::Arc);
        arc.set_arc_geometry(v(10000000, 0), v(0, 10000000), v(-10000000, 0));
        arc.mirror(&Seg::new(v(0, 0), v(0, 1)), 100);
        assert!(within(arc.center(), v(0, 0), 1));
        assert_eq!(arc.radius(), 10000000);
        assert!(within(arc.arc_mid(), v(0, 10000000), 1));

        assert_eq!(mock(ShapeT::Poly).shape_t_as_string(), "S_POLYGON");
        assert_eq!(arc.similarity(&arc.clone()), 1.0);
    }

    /// `hitTest( aPosition )` / `hitTest( aRect )` on a segment and a ring.
    #[test]
    fn hit_tests() {
        let board = crate::board::Board::default();
        let ctx = BoardCtx::bare(&board);

        let mut seg = EdaShape::new(ShapeT::Segment, 100, FillT::NoFill);
        seg.set_start(v(0, 0));
        seg.set_end(v(1000, 0));
        let seg = Mock(seg);
        assert!(seg.shape_hit_test(&ctx, v(500, 50), 0));
        assert!(!seg.shape_hit_test(&ctx, v(500, 60), 0));
        assert!(seg.shape_hit_test(&ctx, v(500, 60), 10));
        let r = Box2I::new(v(-10, -10), Vector2I::new(1020, 20));
        assert!(seg.shape_hit_test_rect(&ctx, &r, true, 0));
        let r = Box2I::new(v(400, 100), Vector2I::new(100, 100));
        assert!(!seg.shape_hit_test_rect(&ctx, &r, false, 0));
        assert!(seg.shape_hit_test_rect(&ctx, &r, false, 60));

        let mut circle = EdaShape::new(ShapeT::Circle, 0, FillT::NoFill);
        circle.set_start(v(0, 0));
        circle.set_end(v(1000, 0));
        let circle = Mock(circle);
        assert!(circle.shape_hit_test(&ctx, v(0, 1000), 0));
        assert!(!circle.shape_hit_test(&ctx, v(0, 0), 0));
        assert_eq!(circle.shape_center(), v(0, 0));
        assert_eq!(
            circle.corners_in_sequence(&ctx, EdaAngle::from_degrees(0.0)),
            vec![
                v(-1000, -1000),
                v(1000, -1000),
                v(1000, 1000),
                v(-1000, 1000)
            ]
        );
    }
}
