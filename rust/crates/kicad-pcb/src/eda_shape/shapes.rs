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

//! Port of the effective-shape, polygon, hatching and line-ending part of
//! `common/eda_shape.cpp`: `makeEffectiveShapes`, `MakeEffectiveShapesForStroking`,
//! `MakeLineEndingEffectiveShapes`, `makeShortenedBodyShapes`,
//! `MakeEffectiveShapesWithLineEndings`, `TransformShapeToPolygon`,
//! `TransformLineEndingsToPolygon`, `TransformWithLineEndingsToPolygon`,
//! `GetLineEndingsBoundingBox`, `UpdateHatching`, `GetEndingTangents`,
//! `GetLineEndingEndpoints` and the body-shortening helpers. Package M8.
//!
//! Members that read a virtual of `EDA_SHAPE` (`GetWidth`, `GetEffectiveWidth`,
//! `GetHatchLine*`, `IsProxyItem`, `IsFilledForHitTesting`, `getMaxError`,
//! `getHatchingKnockouts`) are generic functions over the host (`board-model.md` 6.2a) with
//! method syntax through [`EdaShapeShapesExt`]; the others are inherent methods of
//! [`EdaShape`]. Where the C++ returns a caller-owned `std::vector<SHAPE*>` the port returns
//! `Vec<Box<dyn Shape>>`, in the same order.

use std::collections::BTreeSet;

use kicad_geom::bezier_curves::{Bezier, BezierPoly};
use kicad_geom::convert_basic_shapes_to_polygon::{
    transform_arc_to_polygon, transform_circle_to_polygon, transform_oval_to_polygon,
    transform_ring_to_polygon, transform_round_chamfered_rect_to_polygon,
};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::roundrect::Roundrect;
use kicad_geom::{
    ANGLE_0, ANGLE_45, ANGLE_90, ANGLE_180, ANGLE_360, Box2I, DEGREES_T, EdaAngle, Shape,
    ShapeArc, ShapeCircle, ShapeLineChain, ShapePolySet, ShapeRect, ShapeSegment, ShapeSimple,
    Vector2D, Vector2I, ki_round, rad2deg, rotate_point_around, std_max, std_min,
};

use super::{EdaShape, EdaShapeHatchCacheData, FillT, ShapeT};
use crate::ctx::BoardCtx;
use crate::line_ending::{LineEnding, LineEndingStyle};
use crate::traits::EdaShapeItem;

/// Caller-owned list of shapes (C++ `std::vector<SHAPE*>`).
pub type ShapeList = Vec<Box<dyn Shape>>;

// ---- makeEffectiveShapes ----------------------------------------------------------------

/// `ROUNDRECT::TransformToPolygon( poly, aMaxError )` followed by `poly.Outline( 0 )`. A
/// degenerate rounded rectangle leaves the set empty (C++: `Outline( 0 )` of an empty set is
/// undefined behaviour); the port then yields an empty chain.
fn roundrect_outline(rr: &Roundrect, max_error: i32) -> ShapeLineChain {
    let mut poly = ShapePolySet::new();
    rr.transform_to_polygon(&mut poly, max_error);

    if poly.outline_count() == 0 {
        return ShapeLineChain::new();
    }

    poly.c_outline(0).clone()
}

/// `EDA_SHAPE::makeEffectiveShapes( aEdgeOnly, aLineChainOnly, aHittesting )`.
pub fn make_effective_shapes_list<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    edge_only: bool,
    line_chain_only: bool,
    hittesting: bool,
) -> ShapeList {
    let es = host.eda_shape();
    let mut effective_shapes: ShapeList = Vec::new();
    let width = host.effective_width(ctx);
    let mut solid_fill = es.is_solid_fill()
        || es.is_hatched_fill()
        || host.is_proxy_item()
        || (hittesting && host.is_filled_for_hit_testing());

    if edge_only {
        solid_fill = false;
    }

    match es.shape {
        ShapeT::Arc => {
            effective_shapes.push(Box::new(ShapeArc::from_center_start_angle(
                es.arc_center,
                es.start,
                es.arc_angle(),
                width,
            )));
        }

        ShapeT::Segment => {
            effective_shapes.push(Box::new(ShapeSegment::new(es.start, es.end, width)));
        }

        ShapeT::Rectangle => {
            if es.corner_radius > 0 {
                let rr = Roundrect::new(
                    ShapeRect::from_pos_size(
                        es.start,
                        es.rectangle_width(),
                        es.rectangle_height(),
                    ),
                    es.corner_radius,
                    false,
                );
                let outline = roundrect_outline(&rr, host.shape_max_error(ctx));

                if solid_fill {
                    effective_shapes.push(Box::new(ShapeSimple::from_line_chain(&outline)));
                }

                if width > 0 || !solid_fill {
                    let mut arcs_handled: BTreeSet<usize> = BTreeSet::new();

                    for ii in 0..outline.segment_count() {
                        if outline.is_arc_segment(ii as usize) {
                            let arc_index = outline.arc_index(ii as usize) as usize;

                            if !arcs_handled.contains(&arc_index) {
                                arcs_handled.insert(arc_index);
                                effective_shapes.push(Box::new(ShapeArc::with_width(
                                    outline.arc(arc_index),
                                    width,
                                )));
                            }
                        } else {
                            effective_shapes
                                .push(Box::new(ShapeSegment::from_seg(outline.segment(ii), width)));
                        }
                    }
                }
            } else {
                let pts = es.rect_corners();

                if solid_fill {
                    effective_shapes.push(Box::new(ShapeSimple::from_line_chain(
                        &ShapeLineChain::from_points(&pts, false),
                    )));
                }

                if width > 0 || !solid_fill {
                    effective_shapes.push(Box::new(ShapeSegment::new(pts[0], pts[1], width)));
                    effective_shapes.push(Box::new(ShapeSegment::new(pts[1], pts[2], width)));
                    effective_shapes.push(Box::new(ShapeSegment::new(pts[2], pts[3], width)));
                    effective_shapes.push(Box::new(ShapeSegment::new(pts[3], pts[0], width)));
                }
            }
        }

        ShapeT::Circle => {
            if solid_fill {
                effective_shapes.push(Box::new(ShapeCircle::new(es.center(), es.radius())));
            }

            if width > 0 || !solid_fill {
                effective_shapes.push(Box::new(ShapeArc::from_center_start_angle(
                    es.center(),
                    es.end,
                    ANGLE_360,
                    width,
                )));
            }
        }

        ShapeT::Bezier => {
            let bezier_points = es.build_bezier_to_segments_points_list(host.shape_max_error(ctx));
            let mut start_pt = bezier_points[0];

            for end_pt in bezier_points.iter().skip(1) {
                effective_shapes.push(Box::new(ShapeSegment::new(start_pt, *end_pt, width)));
                start_pt = *end_pt;
            }
        }

        ShapeT::Poly => {
            if es.poly.outline_count() == 0 {
                // malformed/empty polygon
                return effective_shapes;
            }

            for ii in 0..es.poly.outline_count() {
                let l = es.poly.c_outline(ii);

                if solid_fill {
                    effective_shapes.push(Box::new(ShapeSimple::from_line_chain(l)));
                }

                if width > 0 || !es.is_solid_fill() || edge_only {
                    let mut seg_count = l.segment_count();

                    if line_chain_only && l.is_closed() {
                        seg_count -= 1; // Treat closed chain as open
                    }

                    for jj in 0..seg_count {
                        effective_shapes
                            .push(Box::new(ShapeSegment::from_seg(l.c_segment(jj), width)));
                    }
                }
            }
        }

        ShapeT::Ellipse | ShapeT::EllipseArc => {
            if solid_fill && es.shape == ShapeT::Ellipse {
                // Filled closed ellipse: emit a SHAPE_SIMPLE for the filled interior.
                let e = es.build_shape_ellipse();
                let chain = e.convert_to_polyline(host.shape_max_error(ctx));
                let mut pts: Vec<Vector2I> = Vec::new();

                for ii in 0..chain.point_count() {
                    pts.push(chain.c_point(ii));
                }

                effective_shapes.push(Box::new(ShapeSimple::from_line_chain(
                    &ShapeLineChain::from_points(&pts, false),
                )));
            }

            if width > 0 || !solid_fill {
                let e = es.build_shape_ellipse();
                let chain = e.convert_to_polyline(host.shape_max_error(ctx));

                for ii in 0..chain.segment_count() {
                    effective_shapes.push(Box::new(ShapeSegment::from_seg(
                        chain.c_segment(ii),
                        width,
                    )));
                }
            }
        }

        // UNIMPLEMENTED_FOR( SHAPE_T_asString() )
        ShapeT::Undefined => {}
    }

    effective_shapes
}

/// `EDA_SHAPE::MakeEffectiveShapesForStroking( aLineWidth )`: the shapes handed to
/// `STROKE_PARAMS::Stroke()` (`crate::stroke_params::stroke`). A Bezier comes back as one
/// line chain and an ellipse as one `SHAPE_ELLIPSE`, so the pattern runs continuously along
/// the curve; a negative `line_width` means `GetEffectiveWidth()`.
pub fn make_effective_shapes_for_stroking<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_width: i32,
) -> ShapeList {
    let es = host.eda_shape();
    let mut line_width = line_width;

    if line_width < 0 {
        line_width = host.effective_width(ctx);
    }

    if es.start_ending.shorten_depth(line_width) > 0 || es.end_ending.shorten_depth(line_width) > 0
    {
        match es.shape {
            // Stroke() has no Bezier primitive, so it gets the flattened polyline.  One chain,
            // not loose segments, or the pattern restarts at every vertex (same reason as the
            // unshortened case below).
            ShapeT::Bezier => {
                let mut pts: Vec<Vector2I> = Vec::new();

                for pt in shortened_bezier_polyline(host, ctx, line_width) {
                    pts.push(Vector2I::from_vec(pt));
                }

                return vec![Box::new(ShapeLineChain::from_points(&pts, false))];
            }

            ShapeT::Segment | ShapeT::Arc | ShapeT::Poly | ShapeT::EllipseArc => {
                return make_shortened_body_shapes(host, ctx, line_width, true);
            }

            // Other shapes have no body shortening; fall through to the standard cases.
            _ => {}
        }
    }

    match es.shape {
        // Stroke() has no Bezier primitive, so it gets the flattened polyline.  One chain, not
        // loose segments, or the pattern restarts at every vertex.  This case goes away if
        // Bezier ever becomes a SHAPE of its own.
        ShapeT::Bezier => vec![Box::new(ShapeLineChain::from_points(
            &es.build_bezier_to_segments_points_list(host.shape_max_error(ctx)),
            false,
        ))],

        ShapeT::Ellipse | ShapeT::EllipseArc => vec![Box::new(es.build_shape_ellipse())],

        // MakeEffectiveShapes( true ) (the EDA_SHAPE body; no PCB class overrides it)
        _ => make_effective_shapes_list(host, ctx, true, false, false),
    }
}

// ---- line endings -------------------------------------------------------------------------

/// `lineEndingClosedChain()` (static): the ending polygon as a closed chain, without a repeated
/// closing point.
fn line_ending_closed_chain(polygon: &[Vector2I]) -> ShapeLineChain {
    let mut chain = ShapeLineChain::new();

    for (ii, pt) in polygon.iter().enumerate() {
        if ii == polygon.len() - 1 && *pt == polygon[0] {
            continue;
        }

        chain.append_point(*pt, false);
    }

    chain.set_closed(true);
    chain
}

/// `hasLineEnding( aStartEnding, aEndEnding )` (static).
fn has_line_ending(start_ending: &LineEnding, end_ending: &LineEnding) -> bool {
    start_ending.style() != LineEndingStyle::None || end_ending.style() != LineEndingStyle::None
}

/// `addLineEndingEffectiveShapes()` (static).
fn add_line_ending_effective_shapes(
    shapes: &mut ShapeList,
    ending: &LineEnding,
    point: Vector2I,
    tangent: EdaAngle,
    line_width: i32,
) {
    if ending.style() == LineEndingStyle::None {
        return;
    }

    let polygon = ending.shapes(point, tangent, line_width);

    if polygon.is_empty() {
        return;
    }

    if ending.style() == LineEndingStyle::ArrowOpen {
        let width = if ending.stroke_width() > 0 {
            ending.stroke_width()
        } else {
            line_width
        };

        if polygon.len() >= 3 {
            shapes.push(Box::new(ShapeSegment::new(
                polygon[0],
                polygon[1],
                std_max(0, width),
            )));
            shapes.push(Box::new(ShapeSegment::new(
                polygon[1],
                polygon[2],
                std_max(0, width),
            )));
        }

        return;
    }

    if polygon.len() < 3 {
        return;
    }

    let outline = line_ending_closed_chain(&polygon);

    if outline.point_count() >= 3 {
        shapes.push(Box::new(ShapeSimple::from_line_chain(&outline)));
    }

    if ending.stroke_width() > 0 {
        for ii in 0..outline.segment_count() {
            shapes.push(Box::new(ShapeSegment::from_seg(
                outline.c_segment(ii),
                ending.stroke_width(),
            )));
        }
    }
}

/// `addLineEndingPolygon()` (static).
#[allow(clippy::too_many_arguments)]
fn add_line_ending_polygon(
    buffer: &mut ShapePolySet,
    ending: &LineEnding,
    point: Vector2I,
    tangent: EdaAngle,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
    line_width: i32,
) {
    if ending.style() == LineEndingStyle::None {
        return;
    }

    let polygon = ending.shapes(point, tangent, line_width);

    if polygon.is_empty() {
        return;
    }

    let add_stroked_segment =
        |buffer: &mut ShapePolySet, start: Vector2I, end: Vector2I, stroke_width: i32| {
            let width = std_max(0, stroke_width) + 2 * clearance;

            if width > 0 {
                transform_oval_to_polygon(buffer, start, end, width, error, error_loc, 0);
            }
        };

    if ending.style() == LineEndingStyle::ArrowOpen {
        let stroke_width = if ending.stroke_width() > 0 {
            ending.stroke_width()
        } else {
            line_width
        };

        if polygon.len() >= 3 {
            add_stroked_segment(buffer, polygon[0], polygon[1], stroke_width);
            add_stroked_segment(buffer, polygon[1], polygon[2], stroke_width);
        }

        return;
    }

    if polygon.len() < 3 {
        return;
    }

    let outline = line_ending_closed_chain(&polygon);

    if outline.point_count() >= 3 {
        let mut fill = ShapePolySet::new();
        fill.new_outline();

        for ii in 0..outline.point_count() {
            fill.append_point(outline.c_point(ii), -1, -1);
        }

        if clearance > 0 {
            let mut inflate = clearance;

            if error_loc == ErrorLoc::Outside {
                inflate += error;
            }

            fill.inflate(inflate, CornerStrategy::RoundAllCorners, error, false);
        }

        buffer.append_set(&fill);
    }

    if ending.stroke_width() > 0 {
        for ii in 0..outline.segment_count() {
            let seg = outline.c_segment(ii);
            add_stroked_segment(buffer, seg.a, seg.b, ending.stroke_width());
        }
    }
}

/// `bezierSpeedAt()` (static): the norm of the curve's derivative at `t`.
fn bezier_speed_at(bezier: &Bezier<f64>, t: f64) -> f64 {
    let t_inv = 1.0 - t;

    let derivative: Vector2D = (3.0 * t_inv * t_inv) * (bezier.c1 - bezier.start)
        + (6.0 * t_inv * t) * (bezier.c2 - bezier.c1)
        + (3.0 * t * t) * (bezier.end - bezier.c2);

    derivative.euclidean_norm()
}

/// `bezierLength()` (static): arc length of the curve between `t0` and `t1`.
fn bezier_length(bezier: &Bezier<f64>, t0: f64, t1: f64) -> f64 {
    if t1 <= t0 {
        return 0.0;
    }

    // 16-point Gauss-Legendre integration.  This avoids allocating and
    // flattening a sub-curve on every binary-search step during redraw.
    const NODES: [f64; 8] = [
        0.0950125098376374,
        0.2816035507792590,
        0.4580167776572274,
        0.6178762444026438,
        0.7554044083550030,
        0.8656312023878318,
        0.9445750230732326,
        0.9894009349916499,
    ];

    const WEIGHTS: [f64; 8] = [
        0.1894506104550685,
        0.1826034150449236,
        0.1691565193950025,
        0.1495959888165767,
        0.1246289712555339,
        0.0951585116824928,
        0.0622535239386479,
        0.0271524594117541,
    ];

    let half_width = (t1 - t0) / 2.0;
    let center = (t0 + t1) / 2.0;
    let mut length = 0.0;

    for ii in 0..NODES.len() {
        let offset = half_width * NODES[ii];
        length += WEIGHTS[ii]
            * (bezier_speed_at(bezier, center - offset) + bezier_speed_at(bezier, center + offset));
    }

    half_width * length
}

/// `findBezierTAtLength()` (static): the curve parameter at a given arc length.
fn find_bezier_t_at_length(bezier: &Bezier<f64>, target_length: f64, total_length: f64) -> f64 {
    if target_length <= 0.0 {
        return 0.0;
    }

    if target_length >= total_length {
        return 1.0;
    }

    let mut low = 0.0;
    let mut high = 1.0;

    // Keep the parameter search tighter than the flattened-length tolerance.
    const PARAM_SEARCH_ITERATIONS: i32 = 24;

    for _ in 0..PARAM_SEARCH_ITERATIONS {
        let mid = (low + high) / 2.0;
        let len = bezier_length(bezier, 0.0, mid);

        if len < target_length {
            low = mid;
        } else {
            high = mid;
        }
    }

    (low + high) / 2.0
}

impl EdaShape {
    /// The source curve `BEZIER<double>{ m_start, m_bezierC1, m_bezierC2, m_end }`.
    fn source_bezier(&self) -> Bezier<f64> {
        Bezier::new(
            self.start.into(),
            self.bezier_c1.into(),
            self.bezier_c2.into(),
            self.end.into(),
        )
    }

    /// `EDA_SHAPE::MakeLineEndingEffectiveShapes( aLineWidth )`: the line-ending geometry.
    pub fn make_line_ending_effective_shapes(&self, line_width: i32) -> ShapeList {
        let mut effective_shapes: ShapeList = Vec::new();

        if !has_line_ending(&self.start_ending, &self.end_ending) {
            return effective_shapes;
        }

        let Some((start_point, end_point)) = self.line_ending_endpoints() else {
            return effective_shapes;
        };

        let (start_tangent, end_tangent) = self.ending_tangents(line_width);

        add_line_ending_effective_shapes(
            &mut effective_shapes,
            &self.start_ending,
            start_point,
            start_tangent,
            line_width,
        );
        add_line_ending_effective_shapes(
            &mut effective_shapes,
            &self.end_ending,
            end_point,
            end_tangent,
            line_width,
        );

        effective_shapes
    }

    /// `EDA_SHAPE::TransformLineEndingsToPolygon( aBuffer, aClearance, aError, aErrorLoc,
    /// aLineWidth )`: append only the line-ending geometry.
    pub fn transform_line_endings_to_polygon(
        &self,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        line_width: i32,
    ) {
        if !has_line_ending(&self.start_ending, &self.end_ending) {
            return;
        }

        let Some((start_point, end_point)) = self.line_ending_endpoints() else {
            return;
        };

        let (start_tangent, end_tangent) = self.ending_tangents(line_width);

        add_line_ending_polygon(
            buffer,
            &self.start_ending,
            start_point,
            start_tangent,
            clearance,
            error,
            error_loc,
            line_width,
        );
        add_line_ending_polygon(
            buffer,
            &self.end_ending,
            end_point,
            end_tangent,
            clearance,
            error,
            error_loc,
            line_width,
        );
    }

    /// `EDA_SHAPE::GetLineEndingsBoundingBox( aBBox, aLineWidth )`: `None` where the C++
    /// returns false.
    pub fn line_endings_bounding_box(&self, line_width: i32) -> Option<Box2I> {
        let mut bbox = Box2I::default();
        let mut has_box = false;

        let mut merge_ending = |ending: &LineEnding, point: Vector2I, tangent: EdaAngle| {
            if ending.style() == LineEndingStyle::None {
                return;
            }

            let polygon = ending.shapes(point, tangent, line_width);

            if polygon.is_empty() {
                return;
            }

            let mut ending_bbox = Box2I::new(polygon[0], Vector2I::new(0, 0));

            for pt in &polygon {
                ending_bbox.merge_point(*pt);
            }

            let mut stroke = ending.stroke_width();

            if ending.style() == LineEndingStyle::ArrowOpen && stroke <= 0 {
                stroke = line_width;
            }

            ending_bbox.inflate(std_max(0, stroke) / 2);

            if !has_box {
                bbox = ending_bbox;
                has_box = true;
            } else {
                bbox.merge(&ending_bbox);
            }
        };

        if !has_line_ending(&self.start_ending, &self.end_ending) {
            return None;
        }

        let (start_point, end_point) = self.line_ending_endpoints()?;
        let (start_tangent, end_tangent) = self.ending_tangents(line_width);

        merge_ending(&self.start_ending, start_point, start_tangent);
        merge_ending(&self.end_ending, end_point, end_tangent);

        if has_box {
            bbox.normalize();
            Some(bbox)
        } else {
            None
        }
    }

    /// `EDA_SHAPE::GetEndingTangents( aStartTangent, aEndTangent, aLineWidth )`: (start, end)
    /// tangent, pointing away from the body.
    pub fn ending_tangents(&self, line_width: i32) -> (EdaAngle, EdaAngle) {
        let mut start_tangent = ANGLE_0;
        let mut end_tangent = ANGLE_0;

        match self.shape {
            ShapeT::Segment => {
                let line_angle = EdaAngle::from_vector(self.end - self.start);
                start_tangent = line_angle + ANGLE_180;
                end_tangent = line_angle;
            }

            ShapeT::EllipseArc => {
                // Don't factor in the shortening.  If I have a cardinal-section arc, then I
                // expect my arrowhead to be cardinally rotated.  Having the arrow be on the
                // tangent of the arc at the back of the arrowhead looks prettier, but can be
                // misleading as to intent.
                let start_angle = EdaAngle::from_vector(self.start - self.center());
                let end_angle = EdaAngle::from_vector(self.end - self.center());

                if self.ellipse.start_angle - self.ellipse.end_angle > ANGLE_0 {
                    start_tangent = start_angle + ANGLE_90;
                    end_tangent = end_angle - ANGLE_90;
                } else {
                    start_tangent = start_angle - ANGLE_90;
                    end_tangent = end_angle + ANGLE_90;
                }
            }

            ShapeT::Arc => {
                // Don't factor in the shortening (see the ELLIPSE_ARC case).
                let arc = ShapeArc::from_start_mid_end(self.start, self.arc_mid(), self.end, 0);

                if arc.get_central_angle() < ANGLE_0 {
                    start_tangent = arc.get_start_angle() + ANGLE_90;
                    end_tangent = arc.get_end_angle() - ANGLE_90;
                } else {
                    start_tangent = arc.get_start_angle() - ANGLE_90;
                    end_tangent = arc.get_end_angle() + ANGLE_90;
                }
            }

            ShapeT::Bezier => {
                // Tangent = (original endpoint - shortened endpoint) so that the end shape
                // aligns with the visible curve endpoint.
                //
                // Does this make more sense here then in the arc case?  Possibly.
                let source_curve = self.source_bezier();
                let mut total_length: Option<f64> = None;
                let shortened = self.shortened_bezier_curve(line_width);
                let bpts = &self.bezier_points;

                let fallback_start_tangent = || -> EdaAngle {
                    if self.start != self.bezier_c1 {
                        return EdaAngle::from_vector(self.start - self.bezier_c1);
                    }

                    if bpts.len() >= 2 {
                        return EdaAngle::from_vector(bpts[0] - bpts[1]);
                    }

                    ANGLE_0
                };

                let fallback_end_tangent = || -> EdaAngle {
                    if self.end != self.bezier_c2 {
                        return EdaAngle::from_vector(self.end - self.bezier_c2);
                    }

                    if bpts.len() >= 2 {
                        return EdaAngle::from_vector(bpts[bpts.len() - 1] - bpts[bpts.len() - 2]);
                    }

                    ANGLE_0
                };

                let mut open_arrow_flex_tangent =
                    |ending: &LineEnding, is_start: bool| -> Option<EdaAngle> {
                        if ending.style() != LineEndingStyle::ArrowOpen {
                            return None;
                        }

                        let depth = ending.curve_orientation_depth(line_width);

                        if depth <= 0 {
                            return None;
                        }

                        let total = *total_length
                            .get_or_insert_with(|| bezier_length(&source_curve, 0.0, 1.0));

                        if total <= 0.0 {
                            return None;
                        }

                        let mut distance_from_start = std_min(depth as f64, total);

                        if !is_start {
                            distance_from_start = total - distance_from_start;
                        }

                        let t = find_bezier_t_at_length(&source_curve, distance_from_start, total);
                        let sample: Vector2D = source_curve.point_at(t);
                        let endpoint: Vector2D = if is_start {
                            self.start.into()
                        } else {
                            self.end.into()
                        };
                        let delta = endpoint - sample;

                        if delta.euclidean_norm() <= 0.0 {
                            return None;
                        }

                        Some(EdaAngle::from_vector(delta))
                    };

                if let Some(shortened) = shortened {
                    let start_delta = Vector2D::from(self.start) - shortened.start;

                    if start_delta.euclidean_norm() > 0.0 {
                        start_tangent = EdaAngle::from_vector(start_delta);
                    } else {
                        start_tangent = fallback_start_tangent();
                    }

                    let end_delta = Vector2D::from(self.end) - shortened.end;

                    if end_delta.euclidean_norm() > 0.0 {
                        end_tangent = EdaAngle::from_vector(end_delta);
                    } else {
                        end_tangent = fallback_end_tangent();
                    }
                } else {
                    start_tangent = fallback_start_tangent();
                    end_tangent = fallback_end_tangent();
                }

                if let Some(start_flex) = open_arrow_flex_tangent(&self.start_ending, true) {
                    start_tangent = start_flex;
                }

                if let Some(end_flex) = open_arrow_flex_tangent(&self.end_ending, false) {
                    end_tangent = end_flex;
                }
            }

            ShapeT::Poly => {
                let poly = &self.poly;

                if poly.outline_count() > 0 {
                    let outline = poly.c_outline(0);
                    let pt_count = outline.point_count();

                    if pt_count >= 2 {
                        start_tangent =
                            EdaAngle::from_vector(outline.c_point(0) - outline.c_point(1));
                        end_tangent = EdaAngle::from_vector(
                            outline.c_point(pt_count - 1) - outline.c_point(pt_count - 2),
                        );
                    }
                }
            }

            _ => {
                start_tangent = ANGLE_0;
                end_tangent = ANGLE_0;
            }
        }

        (start_tangent, end_tangent)
    }

    /// `EDA_SHAPE::GetLineEndingEndpoints( aStartPoint, aEndPoint )`: `None` where the C++
    /// returns false.
    pub fn line_ending_endpoints(&self) -> Option<(Vector2I, Vector2I)> {
        match self.shape {
            ShapeT::Segment | ShapeT::Arc | ShapeT::EllipseArc => Some((self.start, self.end)),

            ShapeT::Poly => {
                let poly = &self.poly;

                if poly.outline_count() == 0 {
                    return None;
                }

                let outline = poly.c_outline(0);

                if outline.point_count() < 2 {
                    return None;
                }

                Some((outline.c_point(0), outline.c_point(outline.point_count() - 1)))
            }

            ShapeT::Bezier => {
                let bpts = &self.bezier_points;

                if bpts.len() < 2 {
                    return None;
                }

                Some((bpts[0], bpts[bpts.len() - 1]))
            }

            _ => None,
        }
    }

    /// `EDA_SHAPE::ShortenSegmentForEndings( aStart, aEnd, aStartEnding, aEndEnding,
    /// aLineWidth )` (static): moves `start` / `end` inwards by the shorten depths; false (and
    /// `end = start`) when the endings consume the whole segment.
    pub fn shorten_segment_for_endings(
        start: &mut Vector2I,
        end: &mut Vector2I,
        start_ending: &LineEnding,
        end_ending: &LineEnding,
        line_width: i32,
    ) -> bool {
        let delta = *end - *start;
        let len = delta.euclidean_norm() as f64;

        if len == 0.0 {
            return true;
        }

        let start_depth = start_ending.shorten_depth(line_width);
        let end_depth = end_ending.shorten_depth(line_width);
        let total_depth = std_max(0, start_depth) + std_max(0, end_depth);

        if total_depth <= 0 {
            return true;
        }

        if total_depth as f64 >= len {
            *end = *start;
            return false;
        }

        let dir = Vector2D::new(delta.x as f64 / len, delta.y as f64 / len);

        if start_depth > 0 {
            start.x += ki_round(dir.x * start_depth as f64);
            start.y += ki_round(dir.y * start_depth as f64);
        }

        if end_depth > 0 {
            end.x -= ki_round(dir.x * end_depth as f64);
            end.y -= ki_round(dir.y * end_depth as f64);
        }

        true
    }

    /// `EDA_SHAPE::ShortenArcForEndings( aStartAngle, aArcAngle, aRadius, aLineWidth )`.
    pub fn shorten_arc_for_endings(
        &self,
        start_angle: &mut EdaAngle,
        arc_angle: &mut EdaAngle,
        radius: f64,
        line_width: i32,
    ) -> bool {
        if radius <= 0.0 {
            return true;
        }

        let start_depth = self.start_ending.shorten_depth(line_width);
        let end_depth = self.end_ending.shorten_depth(line_width);
        let total_depth = std_max(0, start_depth) + std_max(0, end_depth);

        if total_depth <= 0 {
            return true;
        }

        let arc_length = (radius * arc_angle.as_radians()).abs();

        if total_depth as f64 >= arc_length {
            *arc_angle = ANGLE_0;
            return false;
        }

        let start_offset = EdaAngle::new(
            rad2deg(std_max(0, start_depth) as f64 / radius),
            DEGREES_T,
        );
        let total_offset = EdaAngle::new(rad2deg(total_depth as f64 / radius), DEGREES_T);

        if *arc_angle > ANGLE_0 {
            *start_angle += start_offset;
            *arc_angle -= total_offset;
        } else {
            *start_angle -= start_offset;
            *arc_angle += total_offset;
        }

        true
    }

    /// `EDA_SHAPE::ShortenedBezierCurve( aLineWidth )`: the part of the curve left by the
    /// line endings, `None` when nothing is left (or the shape is not a Bezier).
    pub fn shortened_bezier_curve(&self, line_width: i32) -> Option<Bezier<f64>> {
        if self.shape != ShapeT::Bezier {
            return None;
        }

        let curve = self.source_bezier();

        if self.is_closed() {
            return Some(curve);
        }

        let start_depth = self.start_ending().shorten_depth(line_width);
        let end_depth = self.end_ending().shorten_depth(line_width);

        if start_depth <= 0 && end_depth <= 0 {
            return Some(curve);
        }

        let total_length = bezier_length(&curve, 0.0, 1.0);

        if total_length <= 0.0 || (start_depth + end_depth) as f64 >= total_length {
            return None;
        }

        let t0 = find_bezier_t_at_length(&curve, start_depth as f64, total_length);
        let t1 = find_bezier_t_at_length(&curve, total_length - end_depth as f64, total_length);

        if t1 <= t0 {
            return None;
        }

        Some(curve.sub_curve(t0, t1))
    }

    /// `EDA_SHAPE::ShortenPolyForEndings( aFirst, aLast, aSecond, aPenultimate, aLineWidth )`.
    pub fn shorten_poly_for_endings(
        &self,
        first: &mut Vector2D,
        last: &mut Vector2D,
        second: Vector2D,
        penultimate: Vector2D,
        line_width: i32,
    ) -> bool {
        let start_depth = self.start_ending.shorten_depth(line_width);
        let end_depth = self.end_ending.shorten_depth(line_width);
        let total_depth = std_max(0, start_depth) + std_max(0, end_depth);

        if total_depth <= 0 {
            return true;
        }

        let start_seg_len = (second - *first).euclidean_norm();
        let end_seg_len = (*last - penultimate).euclidean_norm();
        let two_point_poly = second == *last && penultimate == *first;

        if two_point_poly && start_seg_len <= total_depth as f64 {
            *last = *first;
            return false;
        }

        if start_depth > 0 {
            let dir = second - *first;

            if start_seg_len > start_depth as f64 {
                *first = *first + dir * (start_depth as f64 / start_seg_len);
            } else {
                *first = second;
            }
        }

        if end_depth > 0 {
            let dir = *last - penultimate;

            if end_seg_len > end_depth as f64 {
                *last = *last - dir * (end_depth as f64 / end_seg_len);
            } else {
                *last = penultimate;
            }
        }

        true
    }

    /// `EDA_SHAPE::ShortenBodyPolyPoints( aPoints, aOutlineIdx, aLineWidth )`: only the first
    /// outline of an open shape is shortened; `points` is cleared when nothing is left.
    pub fn shorten_body_poly_points(
        &self,
        points: &mut Vec<Vector2I>,
        outline_idx: i32,
        line_width: i32,
    ) -> bool {
        if points.len() < 2 {
            return false;
        }

        if self.is_closed() || outline_idx != 0 {
            return true;
        }

        let mut first: Vector2D = points[0].into();
        let mut last: Vector2D = points[points.len() - 1].into();

        if !self.shorten_poly_for_endings(
            &mut first,
            &mut last,
            points[1].into(),
            points[points.len() - 2].into(),
            line_width,
        ) {
            points.clear();
            return false;
        }

        points[0] = Vector2I::from_vec(first);
        let n = points.len();
        points[n - 1] = Vector2I::from_vec(last);

        true
    }

    /// `EDA_SHAPE::GetShortenedBodyPolyPoints( aOutline, aOutlineIdx, aPoints, aLineWidth )`:
    /// the outline's points, shortened; `None` where the C++ returns false (the points it
    /// would leave behind are not used by any caller).
    pub fn shortened_body_poly_points(
        &self,
        outline: &ShapeLineChain,
        outline_idx: i32,
        line_width: i32,
    ) -> Option<Vec<Vector2I>> {
        if outline.point_count() < 2 {
            return None;
        }

        let mut points: Vec<Vector2I> = outline.c_points().to_vec();

        if self.shorten_body_poly_points(&mut points, outline_idx, line_width) {
            Some(points)
        } else {
            None
        }
    }
}

/// `EDA_SHAPE::ShortenedBezierPolyline( aLineWidth )`: the shortened curve flattened with
/// `max( 1, getMaxError() )`; empty when nothing is left.
pub fn shortened_bezier_polyline<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_width: i32,
) -> Vec<Vector2D> {
    let mut pts: Vec<Vector2D> = Vec::new();

    let Some(curve) = host.eda_shape().shortened_bezier_curve(line_width) else {
        return pts;
    };

    let ctrl_pts = vec![curve.start, curve.c1, curve.c2, curve.end];
    let converter = BezierPoly::from_points_d(ctrl_pts);

    converter.get_poly_d(&mut pts, std_max(1, host.shape_max_error(ctx)) as f64);

    pts
}

/// `EDA_SHAPE::makeShortenedBodyShapes( aLineWidth, aEdgeOnly )`: the body shortened for the
/// line endings (reads the stored endings).
pub fn make_shortened_body_shapes<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_width: i32,
    edge_only: bool,
) -> ShapeList {
    let es = host.eda_shape();
    let mut effective_shapes: ShapeList = Vec::new();
    let shorten_body =
        es.start_ending.shorten_depth(line_width) > 0 || es.end_ending.shorten_depth(line_width) > 0;

    if !shorten_body {
        effective_shapes = make_effective_shapes_list(host, ctx, edge_only, false, false);
    } else {
        let width = host.effective_width(ctx);

        match es.shape {
            ShapeT::Segment => {
                let mut start = es.start;
                let mut end = es.end;

                if EdaShape::shorten_segment_for_endings(
                    &mut start,
                    &mut end,
                    &es.start_ending,
                    &es.end_ending,
                    line_width,
                ) {
                    effective_shapes.push(Box::new(ShapeSegment::new(start, end, width)));
                }
            }

            ShapeT::Arc => {
                let (mut start_angle, end_angle) = es.calc_arc_angles();

                let original_start_angle = start_angle;
                let mut arc_angle = end_angle - start_angle;

                if es.shorten_arc_for_endings(
                    &mut start_angle,
                    &mut arc_angle,
                    es.radius() as f64,
                    line_width,
                ) {
                    let mut start_point = es.start;
                    rotate_point_around(
                        &mut start_point,
                        es.arc_center,
                        -(start_angle - original_start_angle),
                    );
                    effective_shapes.push(Box::new(ShapeArc::from_center_start_angle(
                        es.arc_center,
                        start_point,
                        arc_angle,
                        width,
                    )));
                }
            }

            ShapeT::Bezier => {
                let pts = shortened_bezier_polyline(host, ctx, line_width);

                for ii in 1..pts.len() {
                    effective_shapes.push(Box::new(ShapeSegment::new(
                        Vector2I::from_vec(pts[ii - 1]),
                        Vector2I::from_vec(pts[ii]),
                        width,
                    )));
                }
            }

            ShapeT::Poly => {
                if (!edge_only
                    && (es.is_solid_fill() || es.is_hatched_fill() || host.is_proxy_item()))
                    || es.poly.outline_count() == 0
                {
                    return make_effective_shapes_list(host, ctx, edge_only, false, false);
                }

                for ii in 0..es.poly.outline_count() {
                    let outline = es.poly.c_outline(ii);

                    if outline.point_count() < 2 {
                        continue;
                    }

                    if let Some(pts) = es.shortened_body_poly_points(outline, ii, line_width) {
                        for jj in 1..pts.len() {
                            effective_shapes
                                .push(Box::new(ShapeSegment::new(pts[jj - 1], pts[jj], width)));
                        }

                        if outline.is_closed() {
                            effective_shapes.push(Box::new(ShapeSegment::new(
                                pts[pts.len() - 1],
                                pts[0],
                                width,
                            )));
                        }
                    }
                }
            }

            ShapeT::EllipseArc => {
                let chain = es
                    .build_shape_ellipse()
                    .convert_to_polyline(host.shape_max_error(ctx));

                // C++ passes the stroke width (not aLineWidth) as the ending size here.
                if let Some(pts) = es.shortened_body_poly_points(&chain, 0, width) {
                    for jj in 1..pts.len() {
                        effective_shapes
                            .push(Box::new(ShapeSegment::new(pts[jj - 1], pts[jj], width)));
                    }
                }
            }

            _ => {
                effective_shapes = make_effective_shapes_list(host, ctx, edge_only, false, false);
            }
        }
    }

    effective_shapes
}

/// `EDA_SHAPE::MakeEffectiveShapesWithLineEndings( aLineWidth )`: the body shortened for the
/// line endings plus the line-ending geometry itself (what `PCB_SHAPE::GetEffectiveShape`
/// wraps in a `SHAPE_COMPOUND`).
pub fn make_effective_shapes_with_line_endings<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    line_width: i32,
) -> ShapeList {
    let mut effective_shapes = make_shortened_body_shapes(host, ctx, line_width, false);

    effective_shapes.extend(
        host.eda_shape()
            .make_line_ending_effective_shapes(line_width),
    );

    effective_shapes
}

// ---- TransformShapeToPolygon --------------------------------------------------------------

/// `EDA_SHAPE::TransformShapeToPolygon( aBuffer, aClearance, aError, aErrorLoc,
/// ignoreLineWidth, includeFill )`.
#[allow(clippy::too_many_arguments)]
pub fn transform_shape_to_polygon<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
    ignore_line_width: bool,
    include_fill: bool,
) {
    let es = host.eda_shape();
    let solid_fill =
        es.is_solid_fill() || (es.is_hatched_fill() && !include_fill) || host.is_proxy_item();
    let mut width = if ignore_line_width { 0 } else { host.width(ctx) };

    width += 2 * clearance;

    match es.shape {
        ShapeT::Circle => {
            let r = es.radius();

            if solid_fill {
                transform_circle_to_polygon(buffer, es.center(), r + width / 2, error, error_loc, 0);
            } else {
                transform_ring_to_polygon(buffer, es.center(), r, width, error, error_loc);
            }
        }

        ShapeT::Rectangle => {
            if es.corner_radius() > 0 {
                let size = Vector2I::new(es.rectangle_width().abs(), es.rectangle_height().abs());
                let bbox = super::shape_bounding_box(host, ctx);
                let position = bbox.center();

                if solid_fill {
                    transform_round_chamfered_rect_to_polygon(
                        buffer,
                        position,
                        size,
                        ANGLE_0,
                        es.corner_radius(),
                        0.0,
                        0,
                        width / 2,
                        error,
                        error_loc,
                    );
                } else {
                    let rr = es.roundrect();
                    let mut outline = roundrect_outline(&rr, error);
                    outline.set_closed(true);

                    let mut arcs_handled: BTreeSet<usize> = BTreeSet::new();

                    for ii in 0..outline.segment_count() {
                        if outline.is_arc_segment(ii as usize) {
                            let arc_index = outline.arc_index(ii as usize) as usize;

                            if arcs_handled.contains(&arc_index) {
                                continue;
                            }

                            arcs_handled.insert(arc_index);

                            let arc = outline.arc(arc_index);
                            transform_arc_to_polygon(
                                buffer,
                                arc.get_p0(),
                                arc.get_arc_mid(),
                                arc.get_p1(),
                                width,
                                error,
                                error_loc,
                            );
                        } else {
                            let seg = outline.segment(ii);
                            transform_oval_to_polygon(
                                buffer, seg.a, seg.b, width, error, error_loc, 0,
                            );
                        }
                    }
                }
            } else {
                let pts = es.rect_corners();

                if solid_fill {
                    buffer.new_outline();

                    for pt in &pts {
                        buffer.append_point(*pt, -1, -1);
                    }
                }

                if width > 0 || !solid_fill {
                    // Add in segments
                    transform_oval_to_polygon(buffer, pts[0], pts[1], width, error, error_loc, 0);
                    transform_oval_to_polygon(buffer, pts[1], pts[2], width, error, error_loc, 0);
                    transform_oval_to_polygon(buffer, pts[2], pts[3], width, error, error_loc, 0);
                    transform_oval_to_polygon(buffer, pts[3], pts[0], width, error, error_loc, 0);
                }
            }
        }

        ShapeT::Arc => {
            transform_arc_to_polygon(
                buffer,
                es.start,
                es.arc_mid(),
                es.end,
                width,
                error,
                error_loc,
            );
        }

        ShapeT::Segment => {
            transform_oval_to_polygon(buffer, es.start, es.end, width, error, error_loc, 0);
        }

        ShapeT::Poly => {
            if !es.is_poly_shape_valid() {
                return finish_hatched_fill(host, ctx, buffer, include_fill);
            }

            if solid_fill {
                for ii in 0..es.poly.outline_count() {
                    let poly = es.poly.c_outline(ii);
                    let mut tmp = ShapePolySet::new();
                    tmp.new_outline();

                    for jj in 0..poly.point_count() {
                        tmp.append_point(poly.c_point(jj), -1, -1);
                    }

                    if width > 0 {
                        let mut inflate = width / 2;

                        if error_loc == ErrorLoc::Outside {
                            inflate += error;
                        }

                        tmp.inflate(inflate, CornerStrategy::RoundAllCorners, error, false);
                    }

                    buffer.append_set(&tmp);
                }
            } else {
                for ii in 0..es.poly.outline_count() {
                    let poly = es.poly.c_outline(ii);

                    for jj in 0..poly.segment_count() {
                        let seg = poly.segment(jj);
                        transform_oval_to_polygon(buffer, seg.a, seg.b, width, error, error_loc, 0);
                    }
                }
            }
        }

        ShapeT::Bezier => {
            let converter = BezierPoly::new(es.start, es.bezier_c1, es.bezier_c2, es.end);
            let mut poly: Vec<Vector2I> = Vec::new();
            converter.get_poly_i(&mut poly, error);

            for ii in 1..poly.len() {
                transform_oval_to_polygon(
                    buffer,
                    poly[ii - 1],
                    poly[ii],
                    width,
                    error,
                    error_loc,
                    0,
                );
            }
        }

        ShapeT::Ellipse | ShapeT::EllipseArc => {
            let e = es.build_shape_ellipse();
            let chain = e.convert_to_polyline(error);

            if solid_fill && es.shape == ShapeT::Ellipse {
                // Filled closed ellipse, build the outline, inflate for stroke width.
                let mut tmp = ShapePolySet::new();
                tmp.new_outline();

                for ii in 0..chain.point_count() {
                    tmp.append_point(chain.c_point(ii), -1, -1);
                }

                if width > 0 {
                    let mut inflate = width / 2;

                    if error_loc == ErrorLoc::Outside {
                        inflate += error;
                    }

                    tmp.inflate(inflate, CornerStrategy::RoundAllCorners, error, false);
                }

                buffer.append_set(&tmp);
            } else {
                // stroke each tessellated segment as an oval.
                for ii in 0..chain.segment_count() {
                    let seg = chain.c_segment(ii);
                    transform_oval_to_polygon(buffer, seg.a, seg.b, width, error, error_loc, 0);
                }
            }
        }

        // UNIMPLEMENTED_FOR( SHAPE_T_asString() )
        ShapeT::Undefined => {}
    }

    finish_hatched_fill(host, ctx, buffer, include_fill);
}

/// The tail of `TransformShapeToPolygon`: append the hatching outlines when asked to.
fn finish_hatched_fill<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    include_fill: bool,
) {
    if host.eda_shape().is_hatched_fill() && include_fill {
        let hatching = super::hatching_data(host, ctx);

        for ii in 0..hatching.hatching.outline_count() {
            buffer.add_outline(hatching.hatching.c_outline(ii));
        }
    }
}

/// `EDA_SHAPE::TransformWithLineEndingsToPolygon( aBuffer, aClearance, aError, aErrorLoc,
/// ignoreLineWidth )`: the body shortened for the line endings plus the endings.
pub fn transform_with_line_endings_to_polygon<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
    ignore_line_width: bool,
) {
    let es = host.eda_shape();
    let line_width = if ignore_line_width {
        0
    } else {
        host.effective_width(ctx)
    };
    let shorten_body =
        es.start_ending.shorten_depth(line_width) > 0 || es.end_ending.shorten_depth(line_width) > 0;

    if !shorten_body {
        transform_shape_to_polygon(
            host,
            ctx,
            buffer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
            false,
        );
    } else {
        let mut width = if ignore_line_width { 0 } else { host.width(ctx) };
        width += 2 * clearance;

        match es.shape {
            ShapeT::Segment => {
                let mut start = es.start;
                let mut end = es.end;

                if EdaShape::shorten_segment_for_endings(
                    &mut start,
                    &mut end,
                    &es.start_ending,
                    &es.end_ending,
                    line_width,
                ) {
                    transform_oval_to_polygon(buffer, start, end, width, error, error_loc, 0);
                }
            }

            ShapeT::Arc => {
                let (mut start_angle, end_angle) = es.calc_arc_angles();

                let original_start_angle = start_angle;
                let mut arc_angle = end_angle - start_angle;

                if es.shorten_arc_for_endings(
                    &mut start_angle,
                    &mut arc_angle,
                    es.radius() as f64,
                    line_width,
                ) {
                    let mut start_point = es.start;
                    rotate_point_around(
                        &mut start_point,
                        es.arc_center,
                        -(start_angle - original_start_angle),
                    );
                    let arc = ShapeArc::from_center_start_angle(
                        es.arc_center,
                        start_point,
                        arc_angle,
                        width,
                    );

                    transform_arc_to_polygon(
                        buffer,
                        arc.get_p0(),
                        arc.get_arc_mid(),
                        arc.get_p1(),
                        width,
                        error,
                        error_loc,
                    );
                }
            }

            ShapeT::Bezier => {
                let pts = shortened_bezier_polyline(host, ctx, line_width);

                for ii in 1..pts.len() {
                    transform_oval_to_polygon(
                        buffer,
                        Vector2I::from_vec(pts[ii - 1]),
                        Vector2I::from_vec(pts[ii]),
                        width,
                        error,
                        error_loc,
                        0,
                    );
                }
            }

            ShapeT::Poly => {
                let solid_fill = es.is_solid_fill() || es.is_hatched_fill() || host.is_proxy_item();

                if solid_fill || !es.is_poly_shape_valid() {
                    transform_shape_to_polygon(
                        host,
                        ctx,
                        buffer,
                        clearance,
                        error,
                        error_loc,
                        ignore_line_width,
                        false,
                    );
                } else {
                    for ii in 0..es.poly.outline_count() {
                        let outline = es.poly.c_outline(ii);

                        if outline.point_count() < 2 {
                            continue;
                        }

                        let Some(pts) = es.shortened_body_poly_points(outline, ii, line_width)
                        else {
                            continue;
                        };

                        for jj in 1..pts.len() {
                            transform_oval_to_polygon(
                                buffer,
                                pts[jj - 1],
                                pts[jj],
                                width,
                                error,
                                error_loc,
                                0,
                            );
                        }

                        if outline.is_closed() {
                            transform_oval_to_polygon(
                                buffer,
                                pts[pts.len() - 1],
                                pts[0],
                                width,
                                error,
                                error_loc,
                                0,
                            );
                        }
                    }
                }
            }

            ShapeT::Ellipse | ShapeT::EllipseArc => {
                let chain = es
                    .build_shape_ellipse()
                    .convert_to_polyline(host.shape_max_error(ctx));

                // C++ passes the polygon width (incl. clearance) as the ending size here.
                if let Some(pts) = es.shortened_body_poly_points(&chain, 0, width) {
                    for jj in 1..pts.len() {
                        transform_oval_to_polygon(
                            buffer,
                            pts[jj - 1],
                            pts[jj],
                            width,
                            error,
                            error_loc,
                            0,
                        );
                    }

                    if chain.is_closed() {
                        transform_oval_to_polygon(
                            buffer,
                            pts[pts.len() - 1],
                            pts[0],
                            width,
                            error,
                            error_loc,
                            0,
                        );
                    }
                }
            }

            _ => {
                transform_shape_to_polygon(
                    host,
                    ctx,
                    buffer,
                    clearance,
                    error,
                    error_loc,
                    ignore_line_width,
                    false,
                );
            }
        }
    }

    es.transform_line_endings_to_polygon(buffer, clearance, error, error_loc, line_width);
}

// ---- hatching ---------------------------------------------------------------------------

/// The body of `EDA_SHAPE::UpdateHatching()` while the cache is dirty: the hatch lines and
/// their polygon representation. An early return of the C++ (not a hatched fill, zero
/// spacing, an open shape, an empty polygon) gives an empty result here: the C++ keeps its
/// previous (stale) cache in that case, which in this port has already been dropped by the
/// edit that made it dirty (`PORT_STATUS.md`, M8).
pub fn build_hatching<H: EdaShapeItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
) -> EdaShapeHatchCacheData {
    let es = host.eda_shape();
    let mut cache = EdaShapeHatchCacheData::default();

    let slopes: Vec<f64>;
    let line_width = host.hatch_line_width(ctx);
    let mut spacing = host.hatch_line_spacing(ctx);
    let mut shape_buffer = ShapePolySet::new();

    // isMoving(): the interactive edit state is not ported (always false).

    if es.fill_mode() == FillT::CrossHatch {
        slopes = vec![1.0, -1.0];
    } else if es.fill_mode() == FillT::Hatch {
        slopes = vec![-1.0];
    } else if es.fill_mode() == FillT::ReverseHatch {
        slopes = vec![1.0];
    } else {
        return cache;
    }

    if spacing == 0 {
        return cache;
    }

    match es.shape {
        ShapeT::Arc | ShapeT::Segment | ShapeT::Bezier | ShapeT::EllipseArc => return cache,

        ShapeT::Rectangle => {
            let rr = Roundrect::new(
                ShapeRect::from_pos_size(
                    es.position(),
                    es.rectangle_width(),
                    es.rectangle_height(),
                ),
                es.corner_radius(),
                false,
            );
            rr.transform_to_polygon(&mut shape_buffer, host.shape_max_error(ctx));
        }

        ShapeT::Circle => {
            transform_circle_to_polygon(
                &mut shape_buffer,
                es.center(),
                es.radius(),
                host.shape_max_error(ctx),
                ErrorLoc::Inside,
                0,
            );
        }

        ShapeT::Poly => {
            if es.poly.outline_count() == 0 {
                return cache;
            }

            shape_buffer = es.poly.clone_drop_triangulation();

            for ii in 0..shape_buffer.outline_count() {
                let outline = shape_buffer.outline(ii);

                if outline.is_closed() {
                    continue;
                }

                if outline.point_count() < 3 {
                    continue;
                }

                outline.set_closed(true);
            }
        }

        ShapeT::Ellipse => {
            // Hatching only applies to closed, fillable shapes.
            let e = es.build_shape_ellipse();
            let mut chain = e.convert_to_polyline(host.shape_max_error(ctx));
            chain.set_closed(true);
            shape_buffer.add_outline(&chain);
        }

        // UNIMPLEMENTED_FOR( SHAPE_T_asString() )
        ShapeT::Undefined => return cache,
    }

    shape_buffer.clear_arcs();

    // (The cache is cleared only after all validation passes: `cache` starts empty.)

    let extents = shape_buffer.bbox(0);
    let major_axis = std_max(extents.width(), extents.height()) as i32;

    if major_axis / spacing > 100 {
        spacing = major_axis / 100;
    }

    let knockouts = host.hatching_knockouts(ctx);

    if !knockouts.is_empty() {
        shape_buffer.boolean_subtract(&knockouts);
        shape_buffer.fracture(true);
    }

    // Generate hatch lines for stroke-based rendering. All hatch types use line segments.
    let hatch_segs = shape_buffer.generate_hatch_lines(&slopes, spacing, -1);
    cache.hatch_lines = hatch_segs.clone();

    // Also generate polygon representation for exports, 3D viewer, and hit testing
    if es.fill_mode() == FillT::Hatch || es.fill_mode() == FillT::ReverseHatch {
        for seg in &hatch_segs {
            // We don't really need the rounded ends at all, so don't spend any extra time on
            // them
            let max_error = line_width;

            transform_oval_to_polygon(
                &mut cache.hatching,
                seg.a,
                seg.b,
                line_width,
                max_error,
                ErrorLoc::Inside,
                0,
            );
        }

        cache.hatching.fracture(true);
    } else {
        // Generate a grid of holes for a cross-hatch polygon representation.
        // This is used for exports, 3D viewer, and hit testing.
        let gridsize = spacing;
        let hole_size = gridsize - host.hatch_line_width(ctx);

        cache.hatching = shape_buffer.clone_drop_triangulation();
        cache.hatching.rotate(-ANGLE_45, Vector2I::new(0, 0));

        // Build hole shape
        let mut hole_base = ShapeLineChain::new();
        let mut corner = Vector2I::new(0, 0);
        hole_base.append_point(corner, false);
        corner.x += hole_size;
        hole_base.append_point(corner, false);
        corner.y += hole_size;
        hole_base.append_point(corner, false);
        corner.x = 0;
        hole_base.append_point(corner, false);
        hole_base.set_closed(true);

        // Build holes
        let bbox = cache.hatching.bbox(0);
        let mut holes = ShapePolySet::new();

        let x_offset = bbox.x() - (bbox.x()) % gridsize - gridsize;
        let y_offset = bbox.y() - (bbox.y()) % gridsize - gridsize;

        let mut xx = x_offset;

        while xx <= bbox.right() {
            let mut yy = y_offset;

            while yy <= bbox.bottom() {
                let mut hole = hole_base.clone();
                Shape::move_(&mut hole, Vector2I::new(xx, yy));
                holes.add_outline(&hole);
                yy += gridsize;
            }

            xx += gridsize;
        }

        cache.hatching.boolean_subtract(&holes);
        cache.hatching.fracture(true);

        // Must re-rotate after Fracture().  Clipper struggles mightily with fracturing
        // 45-degree holes.
        cache.hatching.rotate(ANGLE_45, Vector2I::new(0, 0));

        if !knockouts.is_empty() {
            cache.hatching.boolean_subtract(&knockouts);
            cache.hatching.fracture(true);
        }
    }

    cache
}

// ---- method syntax ----------------------------------------------------------------------

/// Method syntax for the M8 members that read `EDA_SHAPE` virtuals and are not in
/// [`super::EdaShapeExt`] (whose signatures were fixed by M0c). Blanket-implemented, so the methods
/// cannot be overridden.
pub trait EdaShapeShapesExt: EdaShapeItem {
    /// `MakeEffectiveShapes( aEdgeOnly )` as a caller-owned list (the `EDA_SHAPE` body; no PCB
    /// class overrides it).
    fn make_effective_shapes_owned(&self, ctx: &BoardCtx<'_>, edge_only: bool) -> ShapeList {
        make_effective_shapes_list(self, ctx, edge_only, false, false)
    }

    /// `MakeEffectiveShapesForHitTesting()` (the `EDA_SHAPE` body).
    fn make_effective_shapes_for_hit_testing(&self, ctx: &BoardCtx<'_>) -> ShapeList {
        make_effective_shapes_list(self, ctx, false, false, true)
    }

    /// `MakeEffectiveShapesForStroking( aLineWidth )`.
    fn make_effective_shapes_for_stroking(&self, ctx: &BoardCtx<'_>, line_width: i32) -> ShapeList {
        make_effective_shapes_for_stroking(self, ctx, line_width)
    }

    /// `MakeEffectiveShapesWithLineEndings( aLineWidth )`.
    fn make_effective_shapes_with_line_endings(
        &self,
        ctx: &BoardCtx<'_>,
        line_width: i32,
    ) -> ShapeList {
        make_effective_shapes_with_line_endings(self, ctx, line_width)
    }

    /// `ShortenedBezierPolyline( aLineWidth )`.
    fn shortened_bezier_polyline(&self, ctx: &BoardCtx<'_>, line_width: i32) -> Vec<Vector2D> {
        shortened_bezier_polyline(self, ctx, line_width)
    }

    /// `TransformWithLineEndingsToPolygon( .. )`.
    fn transform_with_line_endings_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        transform_with_line_endings_to_polygon(
            self,
            ctx,
            buffer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        )
    }
}

impl<T: EdaShapeItem + ?Sized> EdaShapeShapesExt for T {}
