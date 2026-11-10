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

//! Port of the geometry part of `common/stroke_params.cpp`: `STROKE_PARAMS::Stroke`, the
//! dash / dot pattern generation along a shape. Package M8.
//!
//! `StrokeParams` itself, `GetLineStyleToken`, `Format`, `operator<` and the parser are in
//! `kicad_core::stroke_params`. `Stroke` reads three lengths from a `KIGFX::RENDER_SETTINGS`
//! (`GetDashLength`, `GetDotLength`, `GetGapLength` for the stroke width, ported in
//! `kicad_plot::RenderSettings`); this crate cannot depend on the plotter, so the caller
//! passes the three values as [`StrokePatternLengths`].

use kicad_core::LineStyle;
use kicad_geom::geometry::geometry_utils::clip_line;
use kicad_geom::{
    ANGLE_0, ANGLE_360, Box2I, DEGREES_T, EdaAngle, RADIANS_T, Shape, ShapeLineChainBase, ShapeRef,
    ShapeSegment, Vector2D, Vector2I, cmath, ki_round, ki_round_xy, std_min,
};

/// The three pattern lengths `STROKE_PARAMS::Stroke` reads from the render settings, already
/// evaluated for the stroke width: `aRenderSettings->GetDashLength( aWidth )`,
/// `GetDotLength( aWidth )` and `GetGapLength( aWidth )`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePatternLengths {
    pub dash: f64,
    pub dot: f64,
    pub gap: f64,
}

/// `STROKE_PARAMS::Stroke( aShape, aLineStyle, aWidth, aRenderSettings, aStroker )` (static):
/// call `stroker` with every drawn piece of `shape` in the dash / dot pattern of `line_style`.
///
/// Supported shapes are those of the C++: `SH_RECT`, `SH_SIMPLE`, `SH_LINE_CHAIN` (the pattern
/// carries across the vertices), `SH_SEGMENT`, `SH_ARC` and `SH_ELLIPSE`; anything else
/// (`SH_CIRCLE`: a circle is always filled, a ring is a 360 degree arc) draws nothing
/// (`UNIMPLEMENTED_FOR`).
pub fn stroke(
    shape: &dyn Shape,
    line_style: LineStyle,
    width: i32,
    lengths: &StrokePatternLengths,
    stroker: &mut dyn FnMut(Vector2I, Vector2I),
) {
    let w = width as f64;
    let mut strokes: [f64; 6] = [w * 1.0, w * 1.0, w * 1.0, w * 1.0, w * 1.0, w * 1.0];
    let wrap_around: usize;

    match line_style {
        LineStyle::Dash => {
            strokes[0] = lengths.dash;
            strokes[1] = lengths.gap;
            wrap_around = 2;
        }
        LineStyle::Dot => {
            strokes[0] = lengths.dot;
            strokes[1] = lengths.gap;
            wrap_around = 2;
        }
        LineStyle::DashDot => {
            strokes[0] = lengths.dash;
            strokes[1] = lengths.gap;
            strokes[2] = lengths.dot;
            strokes[3] = lengths.gap;
            wrap_around = 4;
        }
        LineStyle::DashDotDot => {
            strokes[0] = lengths.dash;
            strokes[1] = lengths.gap;
            strokes[2] = lengths.dot;
            strokes[3] = lengths.gap;
            strokes[4] = lengths.dot;
            strokes[5] = lengths.gap;
            wrap_around = 6;
        }
        // UNIMPLEMENTED_FOR( lineTypeNames.at( aLineStyle ).name ): SOLID logs and draws the
        // pattern of six width-long elements. DEFAULT has no entry in lineTypeNames, so the
        // C++ throws std::out_of_range there; the port treats it like SOLID (PORT_STATUS.md).
        LineStyle::Solid | LineStyle::Default => {
            wrap_around = 6;
        }
    }

    match shape.as_shape_ref() {
        ShapeRef::Rect(rect) => {
            let outline = rect.outline();
            let mut arcs_handled = std::collections::BTreeSet::new();

            for ii in 0..outline.segment_count() {
                if outline.is_arc_segment(ii as usize) {
                    let arc_index = outline.arc_index(ii as usize) as usize;

                    if !arcs_handled.contains(&arc_index) {
                        arcs_handled.insert(arc_index);
                        let arc = outline.arc(arc_index);
                        stroke(arc, line_style, width, lengths, stroker);
                    }
                } else {
                    let seg = outline.segment(ii);
                    let line = ShapeSegment::new(seg.a, seg.b, 0);
                    stroke(&line, line_style, width, lengths, stroker);
                }
            }

            // Note: the C++ strokes every arc of the outline a second time here.
            for jj in 0..outline.arc_count() {
                let arc = outline.arc(jj);
                stroke(arc, line_style, width, lengths, stroker);
            }
        }

        ShapeRef::Simple(poly) => {
            for ii in 0..poly.get_segment_count() {
                let seg = poly.get_segment(ii as i32);
                let line = ShapeSegment::new(seg.a, seg.b, 0);
                stroke(&line, line_style, width, lengths, stroker);
            }
        }

        ShapeRef::LineChain(chain) => {
            let mut pattern_length = 0.0;

            for s in strokes.iter().take(wrap_around) {
                pattern_length += s;
            }

            // A zero-width shape makes every element zero long, and the walk would never
            // advance.
            if pattern_length <= 0.0 {
                return;
            }

            let mut element: usize = 0;
            let mut remaining = strokes[0];

            for ii in 0..chain.segment_count() {
                let seg = chain.c_segment(ii);
                let seg_vec: Vector2D = (seg.b - seg.a).into();
                let seg_length = seg_vec.euclidean_norm();

                if seg_length == 0.0 {
                    continue;
                }

                // The pattern carries across the vertices, so calculations MUST be done in
                // doubles to keep from accumulating rounding errors along the chain.
                let step = seg_vec / seg_length;
                let mut start: Vector2D = seg.a.into();
                let mut walked = 0.0;

                while walked < seg_length {
                    let length = std_min(remaining, seg_length - walked);
                    let next = start + step * length;

                    if element % 2 == 0 {
                        stroker(ki_round(start), ki_round(next));
                    }

                    walked += length;
                    remaining -= length;
                    start = next;

                    if remaining <= 0.0 {
                        element += 1;
                        remaining = strokes[element % wrap_around];
                    }
                }
            }
        }

        ShapeRef::Segment(line) => {
            let mut start: Vector2D = line.get_seg().a.into();
            let end: Vector2D = line.get_seg().b.into();
            let mut clip = Box2I::new(
                Vector2I::from_vec(start),
                ki_round_xy(end.x - start.x, end.y - start.y),
            );
            clip.normalize();

            let theta = cmath::atan2(end.y - start.y, end.x - start.x);

            for i in 0..10000usize {
                // Calculations MUST be done in doubles to keep from accumulating rounding
                // errors as we go.
                let next = Vector2D::new(
                    start.x + strokes[i % wrap_around] * cmath::cos(theta),
                    start.y + strokes[i % wrap_around] * cmath::sin(theta),
                );

                // Drawing each segment can be done rounded to ints.
                let mut a = ki_round(start);
                let mut b = ki_round(next);

                if clip_line(&clip, &mut a.x, &mut a.y, &mut b.x, &mut b.y) {
                    break;
                } else if i % 2 == 0 {
                    stroker(a, b);
                }

                start = next;
            }
        }

        ShapeRef::Arc(arc) => {
            let r = arc.get_radius();
            let c = 2.0 * std::f64::consts::PI * r;
            let center = arc.get_center();
            let start_radial: Vector2D = (arc.get_p0() - center).into();
            let mut start_angle = EdaAngle::from_vector(start_radial);
            let end_radial: Vector2D = (arc.get_p1() - center).into();
            let mut arc_end_angle = EdaAngle::from_vector(end_radial);

            if arc_end_angle == start_angle {
                arc_end_angle = start_angle + ANGLE_360; // ring, not null
            }

            if start_angle > arc_end_angle {
                if arc_end_angle < ANGLE_0 {
                    arc_end_angle = arc_end_angle.normalize();
                } else {
                    start_angle = start_angle.normalize() - ANGLE_360;
                }
            }

            debug_assert!(start_angle < arc_end_angle);

            let angle_increment = EdaAngle::new(0.5, DEGREES_T);

            let mut i: usize = 0;

            while i < 10000 && start_angle < arc_end_angle {
                let theta = ANGLE_360 * strokes[i % wrap_around] / c;
                let end_angle = std_min(start_angle + theta, arc_end_angle);

                if i % 2 == 0 {
                    if ((line_style == LineStyle::DashDot || line_style == LineStyle::DashDotDot)
                        && i % wrap_around == 0)
                        || line_style == LineStyle::Dash
                    {
                        let mut current_angle = start_angle;

                        while current_angle < end_angle {
                            let a = Vector2I::new(
                                center.x + ki_round(r * current_angle.cos()),
                                center.y + ki_round(r * current_angle.sin()),
                            );

                            // Calculate the next angle step, ensuring it doesn't exceed the
                            // endAngle
                            let mut next_angle = current_angle + angle_increment;

                            if next_angle > end_angle {
                                next_angle = end_angle; // Set nextAngle to endAngle if it exceeds
                            }

                            let b = Vector2I::new(
                                center.x + ki_round(r * next_angle.cos()),
                                center.y + ki_round(r * next_angle.sin()),
                            );

                            stroker(a, b); // Draw the segment as an arc

                            current_angle += angle_increment;
                        }
                    } else {
                        let a = Vector2I::new(
                            center.x + ki_round(r * start_angle.cos()),
                            center.y + ki_round(r * start_angle.sin()),
                        );
                        let b = Vector2I::new(
                            center.x + ki_round(r * end_angle.cos()),
                            center.y + ki_round(r * end_angle.sin()),
                        );

                        stroker(a, b);
                    }
                }

                start_angle = end_angle;
                i += 1;
            }
        }

        ShapeRef::Ellipse(ellipse) => {
            let center = ellipse.get_center();
            let a = ellipse.get_major_radius() as f64;
            let b = ellipse.get_minor_radius() as f64;
            let rotation = ellipse.get_rotation();
            let cos_rot = rotation.cos();
            let sin_rot = rotation.sin();

            let mut start_angle = if ellipse.is_arc() {
                ellipse.get_start_angle()
            } else {
                ANGLE_0
            };
            let mut end_angle = if ellipse.is_arc() {
                ellipse.get_end_angle()
            } else {
                ANGLE_360
            };

            if end_angle == start_angle {
                end_angle = start_angle + ANGLE_360;
            }

            if start_angle > end_angle {
                if end_angle < ANGLE_0 {
                    end_angle = end_angle.normalize();
                } else {
                    start_angle = start_angle.normalize() - ANGLE_360;
                }
            }

            let eval_rad = |theta_rad: f64| -> Vector2D {
                let x = a * cmath::cos(theta_rad);
                let y = b * cmath::sin(theta_rad);
                Vector2D::new(
                    center.x as f64 + x * cos_rot - y * sin_rot,
                    center.y as f64 + x * sin_rot + y * cos_rot,
                )
            };

            let eval_angle = |theta: EdaAngle| -> Vector2I {
                let x = a * theta.cos();
                let y = b * theta.sin();
                Vector2I::new(
                    center.x + ki_round(x * cos_rot - y * sin_rot),
                    center.y + ki_round(x * sin_rot + y * cos_rot),
                )
            };

            // Place strokes by arclength rather than angle so the pattern stays
            // visually uniform on highly curved ellipses.
            let start_rad = start_angle.as_radians();
            let end_rad = end_angle.as_radians();
            const LUT_SIZE: i32 = 1024;
            let mut arc_lens: Vec<f64> = vec![0.0; LUT_SIZE as usize + 1];

            arc_lens[0] = 0.0;
            let mut prev = eval_rad(start_rad);

            for j in 1..=LUT_SIZE {
                let t = start_rad + (end_rad - start_rad) * j as f64 / LUT_SIZE as f64;
                let curr = eval_rad(t);
                arc_lens[j as usize] = arc_lens[j as usize - 1] + (curr - prev).euclidean_norm();
                prev = curr;
            }

            let total_len = arc_lens[LUT_SIZE as usize];

            let angle_at_len = |s: f64| -> f64 {
                if s >= total_len {
                    return end_rad;
                }

                if s <= 0.0 {
                    return start_rad;
                }

                // std::upper_bound( arcLens.begin(), arcLens.end(), s ) - 1 (NaN-exact form)
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                let idx = arc_lens.partition_point(|x| !(s < *x)) as i32 - 1;

                let seg = arc_lens[idx as usize + 1] - arc_lens[idx as usize];
                let frac = if seg > 0.0 {
                    (s - arc_lens[idx as usize]) / seg
                } else {
                    0.0
                };

                let t0 = start_rad + (end_rad - start_rad) * idx as f64 / LUT_SIZE as f64;
                let t1 = start_rad + (end_rad - start_rad) * (idx + 1) as f64 / LUT_SIZE as f64;

                t0 + frac * (t1 - t0)
            };

            let angle_increment = EdaAngle::new(0.5, DEGREES_T);
            let mut current_len = 0.0;
            let mut i: usize = 0;

            while i < 10000 && current_len < total_len {
                let target_len = std_min(current_len + strokes[i % wrap_around], total_len);

                if i % 2 == 0 {
                    let elem_start = EdaAngle::new(angle_at_len(current_len), RADIANS_T);
                    let elem_end = EdaAngle::new(angle_at_len(target_len), RADIANS_T);

                    let mut c = elem_start;

                    while c < elem_end {
                        let n = std_min(c + angle_increment, elem_end);
                        stroker(eval_angle(c), eval_angle(n));
                        c += angle_increment;
                    }
                }

                current_len = target_len;
                i += 1;
            }
        }

        // SH_CIRCLE: a circle is always filled. A ring is represented by a 360 degree arc.
        // UNIMPLEMENTED_FOR( SHAPE_TYPE_asString( aShape->Type() ) )
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
    use kicad_geom::{ShapeArc, ShapeLineChain, ShapeRect, ShapeSimple, Vector2D};

    use crate::eda_shape::{EdaShape, FillT, ShapeT};

    /// `RENDER_SETTINGS::GetDashLength` / `GetDotLength` / `GetGapLength` with the default
    /// ratios (dash 12, gap 3, `correction` 1.0), as `KIGFX::PCB_RENDER_SETTINGS` and the
    /// minimal test settings of the C++ tests use them.
    fn iso_lengths(width: i32) -> StrokePatternLengths {
        const CORRECTION: f64 = 1.0;
        let w = width as f64;
        StrokePatternLengths {
            dash: std_max_f(12.0 - CORRECTION, 1.0) * w,
            dot: std_max_f(1.0 - CORRECTION, 0.2) * w,
            gap: std_max_f(3.0 + CORRECTION, 1.0) * w,
        }
    }

    fn std_max_f(a: f64, b: f64) -> f64 {
        kicad_geom::std_max(a, b)
    }

    fn v(x: i32, y: i32) -> Vector2I {
        Vector2I::new(x, y)
    }

    fn fnv(s: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;

        for c in s.bytes() {
            h ^= u64::from(c);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }

        h
    }

    /// The shapes of tests/oracle/m8_stroke_oracle.cpp, in the same order.
    fn oracle_shapes() -> Vec<(&'static str, Box<dyn Shape>)> {
        let mut chain = ShapeLineChain::new();
        for p in [
            v(0, 0),
            v(1000000, 0),
            v(1500000, 700000),
            v(1500000, 700000),
            v(1700000, 2500000),
            v(-300000, 2600000),
        ] {
            chain.append_point(p, false);
        }

        let mut simple = ShapeSimple::new();
        simple.append(v(0, 0));
        simple.append(v(3000000, 200000));
        simple.append(v(1000000, 2000000));

        let mut rrect = ShapeRect::from_pos_size(v(0, 0), 5000000, 3000000);
        rrect.set_radius(800000);

        let deg = EdaAngle::from_degrees;

        vec![
            (
                "seg",
                Box::new(ShapeSegment::new(v(100000, 200000), v(5300000, 2900000), 0)),
            ),
            (
                "seg_short",
                Box::new(ShapeSegment::new(v(0, 0), v(0, -700000), 0)),
            ),
            ("chain", Box::new(chain)),
            ("simple", Box::new(simple)),
            (
                "rect",
                Box::new(ShapeRect::from_pos_size(
                    v(-1000000, 500000),
                    4000000,
                    2500000,
                )),
            ),
            ("rrect", Box::new(rrect)),
            (
                "arc",
                Box::new(ShapeArc::from_center_start_angle(
                    v(0, 0),
                    v(3000000, 0),
                    deg(135.0),
                    0,
                )),
            ),
            (
                "arc_cw",
                Box::new(ShapeArc::from_center_start_angle(
                    v(1000000, 0),
                    v(1000000, 2000000),
                    deg(-200.0),
                    0,
                )),
            ),
            (
                "ring",
                Box::new(ShapeArc::from_center_start_angle(
                    v(0, 0),
                    v(2500000, 0),
                    ANGLE_360,
                    0,
                )),
            ),
            (
                "ellipse",
                Box::new(ShapeEllipse::new(
                    v(500000, -300000),
                    4000000,
                    1500000,
                    deg(25.0),
                )),
            ),
            (
                "ellipse_arc",
                Box::new(ShapeEllipse::new_arc(
                    v(0, 0),
                    3000000,
                    2000000,
                    deg(-10.0),
                    deg(200.0),
                    deg(30.0),
                )),
            ),
        ]
    }

    /// Differential test against the reference build: tests/golden/m8_stroke.txt, made by
    /// tests/oracle/m8_stroke_build_oracle.sh (every piece of every case, FNV-1a hashed).
    #[test]
    fn stroke_matches_oracle() {
        let golden = include_str!("../tests/golden/m8_stroke.txt");
        let shapes = oracle_shapes();
        let styles = [
            ("solid", LineStyle::Solid),
            ("dash", LineStyle::Dash),
            ("dot", LineStyle::Dot),
            ("dashdot", LineStyle::DashDot),
            ("dashdotdot", LineStyle::DashDotDot),
        ];
        let mut lines = golden.lines();
        let mut cases = 0;

        for width in [100000, 37000] {
            for (sname, shape) in &shapes {
                for (lname, style) in &styles {
                    let mut all = String::new();
                    let mut head = String::new();
                    let mut count = 0;

                    stroke(
                        shape.as_ref(),
                        *style,
                        width,
                        &iso_lengths(width),
                        &mut |a, b| {
                            let p = format!("{},{}>{},{} ", a.x, a.y, b.x, b.y);
                            all.push_str(&p);

                            if count < 3 {
                                head.push_str(&p);
                            }

                            count += 1;
                        },
                    );

                    let actual =
                        format!("{sname}|{lname}|{width}|{count}|{:016x}|{head}", fnv(&all));
                    assert_eq!(actual, lines.next().unwrap());
                    cases += 1;
                }
            }
        }

        assert_eq!(cases, 110);
        assert!(lines.next().is_none());
    }

    /// `DASH_RUNS` / `collectRuns` of qa/tests/pcbnew/test_issue25110_bezier_line_style.cpp:
    /// touching pieces glued back into runs; (drawn lengths, gaps between runs).
    fn collect_runs(shape: &dyn Shape, style: LineStyle, width: i32) -> (Vec<f64>, Vec<f64>) {
        let mut pieces: Vec<(Vector2I, Vector2I)> = Vec::new();

        stroke(shape, style, width, &iso_lengths(width), &mut |a, b| {
            pieces.push((a, b))
        });

        let norm = |a: Vector2I, b: Vector2I| Vector2D::from(b - a).euclidean_norm();
        let mut drawn = Vec::new();
        let mut gaps = Vec::new();
        let mut ii = 0;

        while ii < pieces.len() {
            let mut length = norm(pieces[ii].0, pieces[ii].1);
            let mut end = pieces[ii].1;
            let mut jj = ii + 1;

            while jj < pieces.len() && pieces[jj].0 == end {
                length += norm(pieces[jj].0, pieces[jj].1);
                end = pieces[jj].1;
                jj += 1;
            }

            drawn.push(length);

            if jj < pieces.len() {
                gaps.push(norm(end, pieces[jj].0));
            }

            ii = jj;
        }

        (drawn, gaps)
    }

    /// `makeBezierChain()` of test_issue25110_bezier_line_style.cpp.
    fn make_bezier_chain() -> ShapeLineChain {
        let mm = |v: f64| (v * 1_000_000.0) as i32;
        let mut shape = EdaShape::new(ShapeT::Bezier, 0, FillT::NoFill);

        shape.set_start(v(0, 0));
        shape.set_bezier_c1(v(mm(5.0), mm(-8.0)));
        shape.set_bezier_c2(v(mm(15.0), mm(8.0)));
        shape.set_end(v(mm(20.0), 0));
        shape.rebuild_bezier_to_segments_points_list(5000); // ARC_HIGH_DEF

        ShapeLineChain::from_points(shape.bezier_points(), false)
    }

    /// `BezierDashedLineHasGaps` (qa/tests/pcbnew/test_issue25110_bezier_line_style.cpp).
    #[test]
    fn bezier_dashed_line_has_gaps() {
        let width = 100000;
        let chain = make_bezier_chain();
        let mut curve_length = 0.0;

        for ii in 0..chain.segment_count() {
            let seg = chain.c_segment(ii);
            curve_length += Vector2D::from(seg.b - seg.a).euclidean_norm();
        }

        let lengths = iso_lengths(width);
        let (drawn, _) = collect_runs(&chain, LineStyle::Dash, width);

        assert!(drawn.len() > 5, "{} run(s)", drawn.len());

        let drawn: f64 = drawn.iter().sum();
        let expected = curve_length * lengths.dash / (lengths.dash + lengths.gap);

        assert!((drawn - expected).abs() < 0.02 * curve_length);
    }

    /// `BezierDottedLineIsEvenlySpaced` (qa/tests/pcbnew/test_issue25110_bezier_line_style.cpp).
    #[test]
    fn bezier_dotted_line_is_evenly_spaced() {
        let width = 100000;
        let chain = make_bezier_chain();
        let gap_length = iso_lengths(width).gap;
        let (_, gaps) = collect_runs(&chain, LineStyle::Dot, width);

        assert!(gaps.len() > 5);

        for (ii, gap) in gaps.iter().enumerate() {
            assert!(
                (gap - gap_length).abs() < 0.03 * gap_length,
                "dot {ii} sits {gap} IU from the previous one, expected {gap_length}"
            );
        }
    }

    /// A zero-width line chain does not loop forever (the `patternLength <= 0` guard).
    #[test]
    fn zero_width_chain_draws_nothing() {
        let chain = make_bezier_chain();
        let mut count = 0;

        stroke(&chain, LineStyle::Dash, 0, &iso_lengths(0), &mut |_, _| {
            count += 1
        });

        assert_eq!(count, 0);
    }
}
