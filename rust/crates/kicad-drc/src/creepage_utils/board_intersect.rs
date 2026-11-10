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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of `segmentIntersectsCircle()` and `SegmentIntersectsBoard()` of
//! `pcbnew/drc/drc_creepage_utils.cpp` (lines 1964-2249): whether a candidate creepage path
//! crosses the board edges (with the optional groove-width allowance).

use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::intersection::{IntersectableGeom, IntersectionVisitor};
use kicad_geom::{DEGREES_T, EdaAngle, Seg, Vector2I};
use kicad_pcb::BoardCtx;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::pcb_shape::PcbShape;

use super::board_edges::board_edge_shape;
use super::shapes::{CreepItem, segment_intersects_arc, segments_intersect};

/// Port of `segmentIntersectsCircle()`: whether the segment `p1 p2` crosses the circle in its
/// interior (an intersection within 50 nm of either segment end is a touch, not a crossing).
/// The interior crossings are appended to `a_intersect_points` when given.
pub fn segment_intersects_circle(
    p1: Vector2I,
    p2: Vector2I,
    center: Vector2I,
    radius: f64,
    a_intersect_points: Option<&mut Vec<Vector2I>>,
) -> bool {
    let segment = Seg::new(p1, p2);
    // CIRCLE( const VECTOR2I&, int ): the implicit double -> int conversion
    let circle = Circle {
        radius: kicad_geom::cmath::f64_to_i32_x86(radius),
        center,
    };

    let mut intersection_points: Vec<Vector2I> = Vec::new();
    let geom1 = IntersectableGeom::Seg(segment);
    let geom2 = IntersectableGeom::Circle(circle);

    IntersectionVisitor::new(&geom2, &mut intersection_points).visit(&geom1);

    // A path is allowed to end on the circle, so an intersection at either endpoint is a
    // touch, not a crossing. Only interior crossings count.
    let tolerance_sq: i64 = 50 * 50;

    let coincident = |a: Vector2I, b: Vector2I| (a - b).squared_euclidean_norm() <= tolerance_sq;

    let filtered: Vec<Vector2I> = intersection_points
        .iter()
        .copied()
        .filter(|&ip| !coincident(ip, p1) && !coincident(ip, p2))
        .collect();

    let any = !filtered.is_empty();

    if let Some(out) = a_intersect_points {
        out.extend(filtered);
    }

    any
}

/// One corner arc of a rounded rectangle (`CornerArcRange` of `SegmentIntersectsBoard`).
struct CornerArcRange {
    center: Vector2I,
    start_angle: EdaAngle,
    end_angle: EdaAngle,
}

fn deg(a: f64) -> EdaAngle {
    EdaAngle::new(a, DEGREES_T)
}

/// Port of `SegmentIntersectsBoard()`: whether the segment `a_p1 a_p2` stays clear of the board
/// edges `a_be` (`m_boardEdge`; the transient shapes they refer to are in `a_owned`,
/// `m_ownedBoardEdges`), ignoring the items of `a_dont_test_against`. Returns `false` when the
/// segment crosses an edge. With a positive `a_min_groove_width` the crossings are collected
/// instead, and the segment is accepted when every pair of consecutive crossings (along the
/// longer axis of their bounding box) is at most the groove width apart.
pub fn segment_intersects_board(
    ctx: &BoardCtx<'_>,
    a_p1: Vector2I,
    a_p2: Vector2I,
    a_be: &[CreepItem],
    a_owned: &[PcbShape],
    a_dont_test_against: &[CreepItem],
    a_min_groove_width: i32,
) -> bool {
    let mut intersection_points: Vec<Vector2I> = Vec::new();
    let test_groove_width = a_min_groove_width > 0;

    for &be in a_be {
        if a_dont_test_against.contains(&be) {
            continue;
        }

        let Some(d) = board_edge_shape(ctx, a_owned, be) else {
            continue;
        };

        let start = d.shape.start();
        let end = d.shape.end();

        match d.shape.shape() {
            ShapeT::Segment => {
                let intersects =
                    segments_intersect(a_p1, a_p2, start, end, &mut intersection_points);

                if intersects && !test_groove_width {
                    return false;
                }
            }

            ShapeT::Rectangle => {
                let r = d.shape.corner_radius();

                if r > 0 {
                    // Rounded rectangle: four shortened straight sides + four quarter-circle
                    // arcs.
                    let x1 = start.x.min(end.x);
                    let y1 = start.y.min(end.y);
                    let x2 = start.x.max(end.x);
                    let y2 = start.y.max(end.y);

                    // Straight sides (between arc endpoints). Skip zero-length
                    // sides that occur when one dimension equals 2*r (stadium).
                    let w = x2 - x1;
                    let h = y2 - y1;
                    let mut intersects = false;
                    let p = |x: i32, y: i32| Vector2I::new(x, y);

                    if w > 2 * r {
                        intersects |= segments_intersect(
                            a_p1,
                            a_p2,
                            p(x1 + r, y1),
                            p(x2 - r, y1),
                            &mut intersection_points,
                        );
                        intersects |= segments_intersect(
                            a_p1,
                            a_p2,
                            p(x2 - r, y2),
                            p(x1 + r, y2),
                            &mut intersection_points,
                        );
                    }

                    if h > 2 * r {
                        intersects |= segments_intersect(
                            a_p1,
                            a_p2,
                            p(x2, y1 + r),
                            p(x2, y2 - r),
                            &mut intersection_points,
                        );
                        intersects |= segments_intersect(
                            a_p1,
                            a_p2,
                            p(x1, y2 - r),
                            p(x1, y1 + r),
                            &mut intersection_points,
                        );
                    }

                    if intersects && !test_groove_width {
                        return false;
                    }

                    // Corner arcs, matching the decomposition in TransformEdgeToCreepShapes.
                    // Stadium shapes get semicircles instead of four quarter-arcs to
                    // avoid duplicate centers that cause division by zero in Paths().
                    let arcs: Vec<CornerArcRange> = if h == 2 * r {
                        // Horizontal stadium: left and right semicircles. Each cap spans
                        // the outer half of its circle so the modeled boundary matches the
                        // decomposition in TransformEdgeToCreepShapes.
                        vec![
                            CornerArcRange {
                                center: p(x1 + r, y1 + r),
                                start_angle: deg(90.0),
                                end_angle: deg(270.0),
                            },
                            CornerArcRange {
                                center: p(x2 - r, y1 + r),
                                start_angle: deg(-90.0),
                                end_angle: deg(90.0),
                            },
                        ]
                    } else if w == 2 * r {
                        // Vertical stadium: top and bottom semicircles
                        vec![
                            CornerArcRange {
                                center: p(x1 + r, y1 + r),
                                start_angle: deg(-180.0),
                                end_angle: deg(0.0),
                            },
                            CornerArcRange {
                                center: p(x1 + r, y2 - r),
                                start_angle: deg(0.0),
                                end_angle: deg(180.0),
                            },
                        ]
                    } else {
                        vec![
                            CornerArcRange {
                                center: p(x1 + r, y1 + r),
                                start_angle: deg(-180.0),
                                end_angle: deg(-90.0),
                            },
                            CornerArcRange {
                                center: p(x2 - r, y1 + r),
                                start_angle: deg(-90.0),
                                end_angle: deg(0.0),
                            },
                            CornerArcRange {
                                center: p(x2 - r, y2 - r),
                                start_angle: deg(0.0),
                                end_angle: deg(90.0),
                            },
                            CornerArcRange {
                                center: p(x1 + r, y2 - r),
                                start_angle: deg(90.0),
                                end_angle: deg(180.0),
                            },
                        ]
                    };

                    for ca in &arcs {
                        let arc_intersects = segment_intersects_arc(
                            a_p1,
                            a_p2,
                            ca.center,
                            r as f64,
                            ca.start_angle,
                            ca.end_angle,
                            Some(&mut intersection_points),
                        );

                        if arc_intersects && !test_groove_width {
                            return false;
                        }
                    }
                } else {
                    let c1 = start;
                    let c2 = Vector2I::new(start.x, end.y);
                    let c3 = end;
                    let c4 = Vector2I::new(end.x, start.y);

                    let mut intersects = false;
                    intersects |= segments_intersect(a_p1, a_p2, c1, c2, &mut intersection_points);
                    intersects |= segments_intersect(a_p1, a_p2, c2, c3, &mut intersection_points);
                    intersects |= segments_intersect(a_p1, a_p2, c3, c4, &mut intersection_points);
                    intersects |= segments_intersect(a_p1, a_p2, c4, c1, &mut intersection_points);

                    if intersects && !test_groove_width {
                        return false;
                    }
                }
            }

            ShapeT::Poly => {
                let points = d.shape.poly_points();

                if points.len() < 2 {
                    continue;
                }

                let mut prev_point = *points.last().unwrap();
                let mut intersects = false;

                for &p in &points {
                    intersects |=
                        segments_intersect(a_p1, a_p2, prev_point, p, &mut intersection_points);
                    prev_point = p;
                }

                if intersects && !test_groove_width {
                    return false;
                }
            }

            ShapeT::Circle => {
                let center = d.shape.center();
                let radius = d.shape.radius() as f64;

                let intersects = segment_intersects_circle(
                    a_p1,
                    a_p2,
                    center,
                    radius,
                    Some(&mut intersection_points),
                );

                if intersects && !test_groove_width {
                    return false;
                }
            }

            ShapeT::Arc => {
                let center = d.shape.center();
                let radius = d.shape.radius() as f64;

                let (a, b) = d.shape.calc_arc_angles();

                let intersects = segment_intersects_arc(
                    a_p1,
                    a_p2,
                    center,
                    radius,
                    a,
                    b,
                    Some(&mut intersection_points),
                );

                if intersects && !test_groove_width {
                    return false;
                }
            }

            _ => {}
        }
    }

    if intersection_points.is_empty() {
        return true;
    }

    if intersection_points.len() % 2 != 0 {
        return false; // Should not happen if the start and end are both on the board
    }

    let mut minx = intersection_points[0].x;
    let mut maxx = intersection_points[0].x;
    let mut miny = intersection_points[0].y;
    let mut maxy = intersection_points[0].y;

    for v in &intersection_points {
        minx = v.x.min(minx);
        maxx = v.x.max(maxx);
        miny = v.y.min(miny);
        maxy = v.y.max(maxy);
    }

    // abs( int - int ): the coordinates come from intersections inside the board, far from
    // overflow; wrapping keeps the x86-64 behaviour regardless.
    if maxx.wrapping_sub(minx).wrapping_abs() > maxy.wrapping_sub(miny).wrapping_abs() {
        kicad_geom::stdsort::std_sort(&mut intersection_points, |a, b| a.x > b.x);
    } else {
        kicad_geom::stdsort::std_sort(&mut intersection_points, |a, b| a.y > b.y);
    }

    // int GVSquared = aMinGrooveWidth * aMinGrooveWidth: a 32-bit product (it wraps on
    // x86-64 for a groove width above 46340 nm, as the reference executes it)
    let gv_squared = a_min_groove_width.wrapping_mul(a_min_groove_width);

    let mut i = 0;

    while i < intersection_points.len() {
        if intersection_points[i].squared_distance(intersection_points[i + 1]) > gv_squared as i64
        {
            return false;
        }

        i += 2;
    }

    true
}
