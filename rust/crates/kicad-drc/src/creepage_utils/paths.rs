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

//! Port of `PATH_CONNECTION` (`pcbnew/drc/drc_creepage_utils.h`) and of every
//! `<shape>::Paths( <shape> )` overload of `drc_creepage_utils.cpp` (lines 272-606 and
//! 1075-1961): the candidate creepage paths (tangents, projections) between two shapes, with
//! the C++ floating-point expression order.
//!
//! The C++ selects the receiver virtually and the argument overload statically. Every call in
//! these bodies passes an object whose static type is its dynamic type, so the port uses one
//! free function per (receiver class, argument class) pair, `paths_<receiver>_<argument>`, and
//! [`CreepShape::paths`] dispatches on both dynamic kinds (the virtual table, including the
//! overloads that are not overridden and answer nothing, and the ones that reverse the
//! argument's answer). The C++ `aMaxWeight` / `aMaxSquaredWeight` parameters keep their names.

use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::cmath;
use kicad_geom::geometry::rtree::PackedRTree;
use kicad_geom::{EdaAngle, Seg, ShapePolySet, Vector2D, Vector2I};
use kicad_pcb::BoardCtx;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::pcb_track::Track;

use super::pending::segment_intersects_board;
use super::shapes::{
    BeShapeArc, BeShapeCircle, BeShapePoint, CreepItem, CreepShape, CuShapeArc, CuShapeCircle,
    CuShapeSegment, segment_intersects_arc, to_int, vd, vi,
};

/// Port of `CREEPAGE_TRACK_ENTRY`: a track segment in the [`TrackRtree`].
#[derive(Clone, Debug, PartialEq)]
pub struct CreepageTrackEntry {
    /// `segment`
    pub segment: Seg,
    /// `layer`
    pub layer: PcbLayerId,
    /// `halfWidth`
    pub half_width: i32,
    /// `track` (`const PCB_TRACK*`)
    pub track: CreepItem,
}

/// Port of `TRACK_RTREE` (`KIRTREE::PACKED_RTREE<CREEPAGE_TRACK_ENTRY*, int, 2>`). The entries
/// are stored by value; the C++ null-entry check has no counterpart.
pub type TrackRtree = PackedRTree<CreepageTrackEntry, 2>;

/// Port of `PATH_CONNECTION`: a candidate creepage path between two points.
#[derive(Clone, Debug, PartialEq)]
pub struct PathConnection {
    /// `a1`
    pub a1: Vector2D,
    /// `a2`
    pub a2: Vector2D,
    /// `weight`
    pub weight: f64,
    /// `m_show`
    pub show: bool,
    /// `m_forceA1concavityCheck`
    pub force_a1_concavity_check: bool,
    /// `m_forceA2concavityCheck`
    pub force_a2_concavity_check: bool,
}

impl Default for PathConnection {
    /// The default member initialisers: weight -1, shown.
    fn default() -> Self {
        PathConnection {
            a1: Vector2D::new(0.0, 0.0),
            a2: Vector2D::new(0.0, 0.0),
            weight: -1.0,
            show: true,
            force_a1_concavity_check: false,
            force_a2_concavity_check: false,
        }
    }
}

impl PathConnection {
    /// Port of `PATH_CONNECTION::isValid()`: test if a path is valid, i.e. whether it does not
    /// cross the board edge or a track.
    ///
    /// * `ctx` - the board (used as fallback if no track index is provided)
    /// * `a_layer` - the layer to check
    /// * `a_board_edges` / `a_owned_edges` - the board edge items (`m_boardEdge`) and the
    ///   transient shapes they refer to (`m_ownedBoardEdges`)
    /// * `a_ignore_for_test` - items to ignore in intersection tests
    /// * `a_outline` - board outline polygon
    /// * `a_test_local_concavity` - concavity test flags (the C++ test is disabled)
    /// * `a_min_groove_width` - minimum groove width
    /// * `a_track_index` - optional spatial index for tracks (if `None`, falls back to a
    ///   linear search)
    #[allow(clippy::too_many_arguments)]
    pub fn is_valid(
        &self,
        ctx: &BoardCtx<'_>,
        a_layer: PcbLayerId,
        a_board_edges: &[CreepItem],
        a_owned_edges: &[PcbShape],
        a_ignore_for_test: &[CreepItem],
        a_outline: Option<&ShapePolySet>,
        a_test_local_concavity: (bool, bool),
        a_min_groove_width: i32,
        a_track_index: Option<&TrackRtree>,
    ) -> bool {
        let Some(outline) = a_outline else {
            return true; // We keep the segment if there is a problem
        };

        if !segment_intersects_board(
            ctx,
            vi(self.a1),
            vi(self.a2),
            a_board_edges,
            a_owned_edges,
            a_ignore_for_test,
            a_min_groove_width,
        ) {
            return false;
        }

        // The mid point should be inside the board.
        // Tolerance of 100nm.

        let mid_point: Vector2I = vi((self.a1 + self.a2) / 2.0);
        let tolerance = 100;

        let contained = outline.contains(mid_point, -1, tolerance, false)
            || outline.point_on_edge(mid_point, tolerance);

        if !contained {
            return false;
        }

        // The local concavity test is disabled in C++ (`if( false && ... )`): not ported.
        let _ = a_test_local_concavity;

        if a_layer != PcbLayerId::EdgeCuts {
            let seg_path = Seg::new(vi(self.a1), vi(self.a2));

            // A creepage path endpoint sitting strictly inside another track's copper body
            // is not a real surface point: the visible copper is the enclosing track, and
            // the path would appear to start/end "inside" the copper in the UI. Reject
            // connections whose endpoint lies inside any track interior (regardless of
            // net). The endpoint track itself is in aIgnoreForTest and is skipped.
            //
            // The tolerance lets endpoints that sit exactly on a neighbor track's boundary
            // (for example, two same-net tracks meeting at a shared corner) pass.
            const INTERIOR_TOLERANCE: i32 = 100; // 100 nm

            let endpoint_inside = |seg_track: &Seg, half_width: i32| -> bool {
                seg_track.distance(vi(self.a1)) + INTERIOR_TOLERANCE < half_width
                    || seg_track.distance(vi(self.a2)) + INTERIOR_TOLERANCE < half_width
            };

            if let Some(track_index) = a_track_index {
                let min_x = to_int(self.a1.x).min(to_int(self.a2.x));
                let min_y = to_int(self.a1.y).min(to_int(self.a2.y));
                let max_x = to_int(self.a1.x).max(to_int(self.a2.x));
                let max_y = to_int(self.a1.y).max(to_int(self.a2.y));

                let search_min = [min_x, min_y];
                let search_max = [max_x, max_y];

                let mut failed = false;

                track_index.search(&search_min, &search_max, |entry: &CreepageTrackEntry| {
                    if entry.layer != a_layer {
                        return true;
                    }

                    if seg_path.intersects(&entry.segment) {
                        failed = true;
                        return false;
                    }

                    if !a_ignore_for_test.contains(&entry.track)
                        && endpoint_inside(&entry.segment, entry.half_width)
                    {
                        failed = true;
                        return false;
                    }

                    true
                });

                if failed {
                    return false;
                }
            } else {
                for (track_id, track) in ctx.board.tracks() {
                    // Only PCB_TRACE_T: its GetEffectiveShape() is always an SH_SEGMENT
                    let Track::Trace(trace) = track else {
                        continue;
                    };

                    if !trace.is_on_layer(ctx, a_layer) {
                        continue;
                    }

                    let seg_track = Seg::new(trace.start, trace.end);

                    if seg_path.intersects(&seg_track) {
                        return false;
                    }

                    if !a_ignore_for_test.contains(&CreepItem::Item(track_id.into()))
                        && endpoint_inside(&seg_track, trace.width / 2)
                    {
                        return false;
                    }
                }
            }
        }

        true
    }
}

/// Port of `CREEP_SHAPE::ReversePaths()`: swap the ends of every path.
pub fn reverse_paths(a_v: Vec<PathConnection>) -> Vec<PathConnection> {
    a_v.into_iter()
        .map(|mut pc| {
            std::mem::swap(&mut pc.a1, &mut pc.a2);
            pc
        })
        .collect()
}

impl CreepShape {
    /// The virtual `Paths( aS2, aMaxWeight, aMaxSquaredWeight )` of `self` with the overload of
    /// the dynamic class of `a_s2` (what `GetPaths` selects with its `dynamic_cast` chain).
    pub fn paths(
        &self,
        a_s2: &CreepShape,
        a_max_weight: f64,
        a_max_squared_weight: f64,
    ) -> Vec<PathConnection> {
        use CreepShape as S;
        let (w, w2) = (a_max_weight, a_max_squared_weight);

        match (self, a_s2) {
            (S::BePoint(s), S::BePoint(o)) => paths_be_point_be_point(s, o, w, w2),
            (S::BePoint(s), S::BeCircle(o)) => paths_be_point_be_circle(s, o, w, w2),
            (S::BePoint(s), S::BeArc(o)) => paths_be_point_be_arc(s, o, w, w2),
            (S::BePoint(s), S::CuSegment(o)) => {
                reverse_paths(paths_cu_segment_be_point(o, s, w, w2))
            }
            (S::BePoint(s), S::CuCircle(o)) => reverse_paths(paths_cu_circle_be_point(o, s, w, w2)),
            (S::BePoint(s), S::CuArc(o)) => reverse_paths(paths_cu_arc_be_point(o, s, w, w2)),

            (S::BeCircle(s), S::BePoint(o)) => reverse_paths(paths_be_point_be_circle(o, s, w, w2)),
            (S::BeCircle(s), S::BeCircle(o)) => paths_be_circle_be_circle(s, o, w, w2),
            (S::BeCircle(s), S::BeArc(o)) => paths_be_circle_be_arc(s, o, w, w2),
            (S::BeCircle(s), S::CuSegment(o)) => {
                reverse_paths(paths_cu_segment_be_circle(o, s, w, w2))
            }
            (S::BeCircle(s), S::CuCircle(o)) => {
                reverse_paths(paths_cu_circle_be_circle(o, s, w, w2))
            }
            // Not overridden by BE_SHAPE_CIRCLE: CREEP_SHAPE::Paths answers nothing
            (S::BeCircle(_), S::CuArc(_)) => Vec::new(),

            (S::BeArc(s), S::BePoint(o)) => reverse_paths(paths_be_point_be_arc(o, s, w, w2)),
            (S::BeArc(s), S::BeCircle(o)) => reverse_paths(paths_be_circle_be_arc(o, s, w, w2)),
            (S::BeArc(s), S::BeArc(o)) => paths_be_arc_be_arc(s, o, w, w2),
            (S::BeArc(s), S::CuSegment(o)) => reverse_paths(paths_cu_segment_be_arc(o, s, w, w2)),
            (S::BeArc(s), S::CuCircle(o)) => reverse_paths(paths_cu_circle_be_arc(o, s, w, w2)),
            (S::BeArc(s), S::CuArc(o)) => reverse_paths(paths_cu_arc_be_arc(o, s, w, w2)),

            (S::CuSegment(s), S::BePoint(o)) => paths_cu_segment_be_point(s, o, w, w2),
            (S::CuSegment(s), S::BeCircle(o)) => paths_cu_segment_be_circle(s, o, w, w2),
            (S::CuSegment(s), S::BeArc(o)) => paths_cu_segment_be_arc(s, o, w, w2),
            (S::CuSegment(s), S::CuSegment(o)) => paths_cu_segment_cu_segment(s, o, w, w2),
            (S::CuSegment(s), S::CuCircle(o)) => paths_cu_segment_cu_circle(s, o, w, w2),
            (S::CuSegment(s), S::CuArc(o)) => paths_cu_segment_cu_arc(s, o, w, w2),

            (S::CuCircle(s), S::BePoint(o)) => paths_cu_circle_be_point(s, o, w, w2),
            (S::CuCircle(s), S::BeCircle(o)) => paths_cu_circle_be_circle(s, o, w, w2),
            (S::CuCircle(s), S::BeArc(o)) => paths_cu_circle_be_arc(s, o, w, w2),
            (S::CuCircle(s), S::CuSegment(o)) => {
                reverse_paths(paths_cu_segment_cu_circle(o, s, w, w2))
            }
            (S::CuCircle(s), S::CuCircle(o)) => paths_cu_circle_cu_circle(s, o, w, w2),
            (S::CuCircle(s), S::CuArc(o)) => paths_cu_circle_cu_arc(s, o, w, w2),

            (S::CuArc(s), S::BePoint(o)) => paths_cu_arc_be_point(s, o, w, w2),
            (S::CuArc(s), S::BeCircle(o)) => paths_cu_arc_be_circle(s, o, w, w2),
            (S::CuArc(s), S::BeArc(o)) => paths_cu_arc_be_arc(s, o, w, w2),
            (S::CuArc(s), S::CuSegment(o)) => reverse_paths(paths_cu_segment_cu_arc(o, s, w, w2)),
            (S::CuArc(s), S::CuCircle(o)) => reverse_paths(paths_cu_circle_cu_arc(o, s, w, w2)),
            (S::CuArc(s), S::CuArc(o)) => paths_cu_arc_cu_arc(s, o, w, w2),
        }
    }
}

/// `CU_SHAPE_ARC::Paths( const CU_SHAPE_CIRCLE& )`: `ReversePaths( aS2.Paths( *this ) )`.
fn paths_cu_arc_cu_circle(
    s: &CuShapeArc,
    a_s2: &CuShapeCircle,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    reverse_paths(paths_cu_circle_cu_arc(a_s2, s, a_max_weight, a_max_squared_weight))
}

/// `BE_SHAPE_ARC::Paths( const BE_SHAPE_CIRCLE& )`: `ReversePaths( aS2.Paths( *this ) )`.
fn paths_be_arc_be_circle(
    s: &BeShapeArc,
    a_s2: &BeShapeCircle,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    reverse_paths(paths_be_circle_be_arc(a_s2, s, a_max_weight, a_max_squared_weight))
}

/// `BE_SHAPE_CIRCLE::Paths( const BE_SHAPE_POINT& )`: `ReversePaths( aS2.Paths( *this ) )`.
fn paths_be_circle_be_point(
    s: &BeShapeCircle,
    a_s2: &BeShapePoint,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    reverse_paths(paths_be_point_be_circle(a_s2, s, a_max_weight, a_max_squared_weight))
}

/// Port of `BE_SHAPE_POINT::Paths( const BE_SHAPE_POINT& )`.
pub fn paths_be_point_be_point(
    s: &BeShapePoint,
    a_s2: &BeShapePoint,
    _a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let weight = (s.pos() - a_s2.pos()).squared_euclidean_norm() as f64;

    if weight > a_max_squared_weight {
        return result;
    }

    let pc = PathConnection {
        a1: vd(s.pos()),
        a2: vd(a_s2.pos()),
        weight: weight.sqrt(),
        ..PathConnection::default()
    };

    result.push(pc);
    result
}

/// Port of `BE_SHAPE_POINT::Paths( const BE_SHAPE_CIRCLE& )`: the two tangents from the point
/// to the circle.
pub fn paths_be_point_be_circle(
    s: &BeShapePoint,
    a_s2: &BeShapeCircle,
    _a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let radius = a_s2.radius();
    let point_pos = s.pos();
    let circle_center = a_s2.pos();

    if radius <= 0 {
        return result;
    }

    let point_to_center_distance_squared =
        (point_pos - circle_center).squared_euclidean_norm() as f64;
    let weight_squared =
        point_to_center_distance_squared - ((radius as f32) * (radius as f32)) as f64;

    if weight_squared > a_max_squared_weight {
        return result;
    }

    let mut direction1 = Vector2D::new(
        (point_pos.x - circle_center.x) as f64,
        (point_pos.y - circle_center.y) as f64,
    );
    direction1 = direction1.resize(1.0);

    let direction2 = direction1.perpendicular();

    let radius_squared = radius as f64 * radius as f64;

    let distance = point_to_center_distance_squared.sqrt();
    let value1 = radius_squared / distance;
    let value2 = (radius_squared - value1 * value1).sqrt();

    let mut pc = PathConnection {
        a1: vd(point_pos),
        weight: weight_squared.sqrt(),
        ..PathConnection::default()
    };

    let result_point = direction1 * value1 + direction2 * value2 + vd(circle_center);
    pc.a2.x = to_int(result_point.x) as f64;
    pc.a2.y = to_int(result_point.y) as f64;
    result.push(pc.clone());

    let result_point = direction1 * value1 - direction2 * value2 + vd(circle_center);
    pc.a2.x = to_int(result_point.x) as f64;
    pc.a2.y = to_int(result_point.y) as f64;
    result.push(pc);

    result
}

/// Port of `BE_SHAPE_POINT::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_be_point_be_arc(
    s: &BeShapePoint,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let center = a_s2.pos();
    let radius: f64 = a_s2.radius() as f64;

    // First path tries to connect to start point
    // Second path tries to connect to end point
    let behaves_like_circle = a_s2.is_there_a_tangent_passing_through_point(s);

    if behaves_like_circle.0 && behaves_like_circle.1 {
        let csc = BeShapeCircle::new(center, to_int(radius));
        return paths_be_point_be_circle(s, &csc, a_max_weight, a_max_squared_weight);
    }

    if behaves_like_circle.0 {
        let csc = BeShapeCircle::new(center, to_int(radius));
        let paths = paths_be_point_be_circle(s, &csc, a_max_weight, a_max_squared_weight);

        if paths.len() > 1 {
            // Point to circle creates either 0 or 2 connections
            result.push(paths[1].clone());
        }
    } else {
        let csp1 = BeShapePoint::new(a_s2.start_point());
        result.extend(paths_be_point_be_point(s, &csp1, a_max_weight, a_max_squared_weight));
    }

    if behaves_like_circle.1 {
        let csc = BeShapeCircle::new(center, to_int(radius));
        let paths = paths_be_point_be_circle(s, &csc, a_max_weight, a_max_squared_weight);

        if paths.len() > 1 {
            // Point to circle creates either 0 or 2 connections
            result.push(paths[0].clone());
        }
    } else {
        let csp1 = BeShapePoint::new(a_s2.end_point());
        result.extend(paths_be_point_be_point(s, &csp1, a_max_weight, a_max_squared_weight));
    }

    result
}

/// Port of `BE_SHAPE_CIRCLE::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_be_circle_be_arc(
    s: &BeShapeCircle,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let circle_center = s.pos();
    let circle_radius = s.radius() as f64;
    let arc_center = a_s2.pos();
    let arc_radius = a_s2.radius() as f64;
    let arc_start_angle = a_s2.start_angle();
    let arc_end_angle = a_s2.end_angle();

    let center_distance = (circle_center - arc_center).euclidean_norm() as f64;

    if center_distance + arc_radius < circle_radius {
        // The arc is inside the circle
        return result;
    }

    let csp1 = BeShapePoint::new(a_s2.start_point());
    let csp2 = BeShapePoint::new(a_s2.end_point());
    let csc = BeShapeCircle::new(arc_center, to_int(arc_radius));

    for pc in paths_be_circle_be_circle(s, &csc, a_max_weight, a_max_squared_weight) {
        let point_angle = a_s2.angle_between_start_and_end(vi(pc.a2));

        if point_angle <= a_s2.end_angle() {
            result.push(pc);
        }
    }

    if result.len() == 4 {
        // It behaved as a circle
        return result;
    }

    for csp in [&csp1, &csp2] {
        for pc in paths_be_circle_be_point(s, csp, a_max_weight, a_max_squared_weight) {
            if !segment_intersects_arc(
                vi(pc.a1),
                vi(pc.a2),
                arc_center,
                arc_radius,
                arc_start_angle,
                arc_end_angle,
                None,
            ) {
                result.push(pc);
            }
        }
    }

    result
}

/// Port of `BE_SHAPE_ARC::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_be_arc_be_arc(
    s: &BeShapeArc,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let circle_center = s.pos();
    let circle_radius = s.radius() as f64;
    let arc_center = a_s2.pos();
    let arc_radius = a_s2.radius() as f64;

    let center_distance = (circle_center - arc_center).euclidean_norm() as f64;

    if center_distance + arc_radius < circle_radius {
        // The arc is inside the circle
        return result;
    }

    // (csp1, csp2 and csc are constructed and unused in C++)

    let as_circle = BeShapeCircle::new(a_s2.pos(), a_s2.radius());

    for pc in paths_be_arc_be_circle(s, &as_circle, a_max_weight, a_max_squared_weight) {
        let point_angle = a_s2.angle_between_start_and_end(vi(pc.a2));

        if point_angle <= a_s2.end_angle() {
            result.push(pc);
        }
    }

    let this_circle = BeShapeCircle::new(s.pos(), s.radius());

    for pc in paths_be_circle_be_arc(&this_circle, a_s2, a_max_weight, a_max_squared_weight) {
        let point_angle = s.angle_between_start_and_end(vi(pc.a1));

        if point_angle <= s.end_angle() {
            result.push(pc);
        }
    }

    result
}

/// Port of `BE_SHAPE_CIRCLE::Paths( const BE_SHAPE_CIRCLE& )`: the straight and the crossed
/// tangents of two circles.
pub fn paths_be_circle_be_circle(
    s: &BeShapeCircle,
    a_s2: &BeShapeCircle,
    _a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let p1 = s.pos();
    let p2 = a_s2.pos();

    let dist_squared = Vector2D::new((p2 - p1).x as f64, (p2 - p1).y as f64);
    let weight_squared = dist_squared.squared_euclidean_norm();

    let r1 = s.radius() as f64;
    let r2 = a_s2.radius() as f64;

    let rdiff = (r1 - r2).abs();
    let rsum = r1 + r2;

    // "Straight" paths
    let weight_squared1 = weight_squared - rdiff * rdiff;
    // "Crossed" paths
    let weight_squared2 = weight_squared - rsum * rsum;

    if weight_squared1 <= a_max_squared_weight {
        let mut direction1 = Vector2D::new((p2.x - p1.x) as f64, (p2.y - p1.y) as f64);
        direction1 = direction1.resize(1.0);
        let direction2 = direction1.perpendicular();

        let d = weight_squared.sqrt();
        let ratio1 = (r1 - r2) / d;
        let ratio2 = (1.0 - ratio1 * ratio1).sqrt();

        let mut pc = PathConnection {
            weight: weight_squared1.sqrt(),
            ..PathConnection::default()
        };

        pc.a1 = vd(p1) + direction1 * r1 * ratio1 + direction2 * r1 * ratio2;
        pc.a2 = vd(p2) + direction1 * r2 * ratio1 + direction2 * r2 * ratio2;

        result.push(pc.clone());

        pc.a1 = vd(p1) + direction1 * r1 * ratio1 - direction2 * r1 * ratio2;
        pc.a2 = vd(p2) + direction1 * r2 * ratio1 - direction2 * r2 * ratio2;

        result.push(pc);
    }

    if weight_squared2 <= a_max_squared_weight {
        let mut direction1 = Vector2D::new((p2.x - p1.x) as f64, (p2.y - p1.y) as f64);
        direction1 = direction1.resize(1.0);
        let direction2 = direction1.perpendicular();

        let d = weight_squared.sqrt();
        let ratio1 = (r1 + r2) / d;
        let ratio2 = (1.0 - ratio1 * ratio1).sqrt();

        let mut pc = PathConnection {
            weight: weight_squared2.sqrt(),
            ..PathConnection::default()
        };

        pc.a1 = vd(p1) + direction1 * r1 * ratio1 + direction2 * r1 * ratio2;
        pc.a2 = vd(p2) - direction1 * r2 * ratio1 - direction2 * r2 * ratio2;

        result.push(pc.clone());

        pc.a1 = vd(p1) + direction1 * r1 * ratio1 - direction2 * r1 * ratio2;
        pc.a2 = vd(p2) - direction1 * r2 * ratio1 + direction2 * r2 * ratio2;

        result.push(pc);
    }

    result
}

/// The direction angle of a track, `EDA_ANGLE trackAngle( end - start )`, and the projection
/// of `point` on it, as `CU_SHAPE_SEGMENT::Paths` computes them.
fn track_projection(start: Vector2I, end: Vector2I, point: Vector2I) -> f64 {
    let track_angle = EdaAngle::from_vector(end - start);

    cmath::cos(track_angle.as_radians()) * (point.x - start.x) as f64
        + cmath::sin(track_angle.as_radians()) * (point.y - start.y) as f64
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const BE_SHAPE_POINT& )`.
pub fn paths_cu_segment_be_point(
    s: &CuShapeSegment,
    a_s2: &BeShapePoint,
    _a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let start = s.start();
    let end = s.end();
    let half_width = s.width() / 2.0;
    let point_pos = a_s2.pos();

    let length = (start - end).euclidean_norm() as f64;
    let projected_pos = track_projection(start, end, point_pos);

    let new_point: Vector2I;

    if projected_pos <= 0.0 {
        new_point = start + (point_pos - start).resize(to_int(half_width));
    } else if projected_pos >= length {
        new_point = end + (point_pos - end).resize(to_int(half_width));
    } else {
        let mut pos_on_segment: f64 = ((start - point_pos).squared_euclidean_norm()
            - (end - point_pos).squared_euclidean_norm())
            as f64;
        pos_on_segment = pos_on_segment / (2.0 * length) + length / 2.0;

        let mut p = start + (end - start).resize(to_int(pos_on_segment));
        p += (point_pos - p).resize(to_int(half_width));
        new_point = p;
    }

    let weight_squared = (point_pos - new_point).squared_euclidean_norm() as f64;

    if weight_squared > a_max_squared_weight {
        return result;
    }

    let pc = PathConnection {
        a1: vd(new_point),
        a2: vd(point_pos),
        weight: weight_squared.sqrt(),
        ..PathConnection::default()
    };

    result.push(pc);
    result
}

/// `PointOnTrack += ( end - start ).Perpendicular().Resize( halfWidth ) * trackSide`: the
/// `VECTOR2D` product converted back to `VECTOR2I` for `operator+=`.
fn side_offset(start: Vector2I, end: Vector2I, half_width: f64, track_side: f64) -> Vector2I {
    vi(vd((end - start).perpendicular().resize(to_int(half_width))) * track_side)
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const BE_SHAPE_CIRCLE& )`.
pub fn paths_cu_segment_be_circle(
    s: &CuShapeSegment,
    a_s2: &BeShapeCircle,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let start = s.start();
    let end = s.end();
    let half_width = s.width() / 2.0;

    let circle_radius = a_s2.radius() as f64;
    let circle_center = a_s2.pos();
    let length = (start - end).euclidean_norm() as f64;

    let mut weight_squared: f64;
    let mut point_on_track: Vector2I;
    let mut point_on_circle: Vector2I;

    // There are two possible paths
    // First the one on the side of the start of the track.
    let mut projected_pos1 = track_projection(start, end, circle_center);
    let projected_pos2 = projected_pos1 + circle_radius;
    projected_pos1 -= circle_radius;

    let track_side: f64 = if (end - start).cross(circle_center - start) > 0 {
        1.0
    } else {
        -1.0
    };

    if projected_pos1 < 0.0 && projected_pos2 < 0.0 {
        let csc = CuShapeCircle::new(start, half_width);
        result.extend(paths_cu_circle_be_circle(&csc, a_s2, a_max_weight, a_max_squared_weight));
    } else if projected_pos1 > length && projected_pos2 > length {
        let csc = CuShapeCircle::new(end, half_width);
        result.extend(paths_cu_circle_be_circle(&csc, a_s2, a_max_weight, a_max_squared_weight));
    } else if (projected_pos1 >= 0.0)
        && (projected_pos1 <= length)
        && (projected_pos2 >= 0.0)
        && (projected_pos2 <= length)
    {
        // Both point connects to the segment part of the track
        point_on_track = start;
        point_on_track += (end - start).resize(to_int(projected_pos1));
        point_on_track += side_offset(start, end, half_width, track_side);
        point_on_circle = circle_center - (end - start).resize(to_int(circle_radius));
        weight_squared = (point_on_circle - point_on_track).squared_euclidean_norm() as f64;

        if weight_squared < a_max_squared_weight {
            let mut pc = PathConnection {
                a1: vd(point_on_track),
                a2: vd(point_on_circle),
                weight: weight_squared.sqrt(),
                ..PathConnection::default()
            };

            result.push(pc.clone());

            point_on_track = start;
            point_on_track += (end - start).resize(to_int(projected_pos2));
            point_on_track += side_offset(start, end, half_width, track_side);
            point_on_circle = circle_center + (end - start).resize(to_int(circle_radius));

            pc.a1 = vd(point_on_track);
            pc.a2 = vd(point_on_circle);

            result.push(pc);
        }
    } else if ((projected_pos1 >= 0.0) && (projected_pos1 <= length))
        && ((projected_pos2 > length) || projected_pos2 < 0.0)
    {
        let csc = CuShapeCircle::new(end, half_width);
        let pcs = paths_cu_circle_be_circle(&csc, a_s2, a_max_weight, a_max_squared_weight);

        if pcs.len() < 2 {
            return result;
        }

        result.push(pcs[if track_side == 1.0 { 1 } else { 0 }].clone());

        point_on_track = start;
        point_on_track += (end - start).resize(to_int(projected_pos1));
        point_on_track += side_offset(start, end, half_width, track_side);
        point_on_circle = circle_center - (end - start).resize(to_int(circle_radius));
        weight_squared = (point_on_circle - point_on_track).squared_euclidean_norm() as f64;

        if weight_squared < a_max_squared_weight {
            let pc = PathConnection {
                a1: vd(point_on_track),
                a2: vd(point_on_circle),
                weight: weight_squared.sqrt(),
                ..PathConnection::default()
            };

            result.push(pc);
        }
    } else if ((projected_pos2 >= 0.0) && (projected_pos2 <= length))
        && ((projected_pos1 > length) || projected_pos1 < 0.0)
    {
        let csc = CuShapeCircle::new(start, half_width);
        let pcs = paths_cu_circle_be_circle(&csc, a_s2, a_max_weight, a_max_squared_weight);

        if pcs.len() < 2 {
            return result;
        }

        result.push(pcs[if track_side == 1.0 { 0 } else { 1 }].clone());

        point_on_track = start;
        point_on_track += (end - start).resize(to_int(projected_pos2));
        point_on_track += side_offset(start, end, half_width, track_side);
        point_on_circle = circle_center + (end - start).resize(to_int(circle_radius));
        weight_squared = (point_on_circle - point_on_track).squared_euclidean_norm() as f64;

        if weight_squared < a_max_squared_weight {
            let pc = PathConnection {
                a1: vd(point_on_track),
                a2: vd(point_on_circle),
                weight: weight_squared.sqrt(),
                ..PathConnection::default()
            };

            result.push(pc);
        }
    } else if projected_pos1 < 0.0 && projected_pos2 > length {
        // The circle projects past both ends of the track, so neither tangent lands on the
        // track flank and each end cap carries one side of the path
        let csc_start = CuShapeCircle::new(start, half_width);
        let start_pcs =
            paths_cu_circle_be_circle(&csc_start, a_s2, a_max_weight, a_max_squared_weight);

        if start_pcs.len() >= 2 {
            result.push(start_pcs[if track_side == 1.0 { 0 } else { 1 }].clone());
        }

        let csc_end = CuShapeCircle::new(end, half_width);
        let end_pcs = paths_cu_circle_be_circle(&csc_end, a_s2, a_max_weight, a_max_squared_weight);

        if end_pcs.len() >= 2 {
            result.push(end_pcs[if track_side == 1.0 { 1 } else { 0 }].clone());
        }
    }

    result
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_cu_segment_be_arc(
    s: &CuShapeSegment,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let bsc = BeShapeCircle::new(a_s2.pos(), a_s2.radius());

    for pc in paths_cu_segment_be_circle(s, &bsc, a_max_weight, a_max_squared_weight) {
        let test_angle = a_s2.angle_between_start_and_end(vi(pc.a2));

        if test_angle < a_s2.end_angle() {
            result.push(pc);
        }
    }

    if result.len() < 2 {
        let bsp1 = BeShapePoint::new(a_s2.start_point());
        let bsp2 = BeShapePoint::new(a_s2.end_point());

        let be_arc_pos = a_s2.pos();
        let be_arc_radius = a_s2.radius();
        let be_arc_start_angle = a_s2.start_angle();
        let be_arc_end_angle = a_s2.end_angle();

        for bsp in [&bsp1, &bsp2] {
            for pc in paths_cu_segment_be_point(s, bsp, a_max_weight, a_max_squared_weight) {
                if !segment_intersects_arc(
                    vi(pc.a1),
                    vi(pc.a2),
                    be_arc_pos,
                    be_arc_radius as f64,
                    be_arc_start_angle,
                    be_arc_end_angle,
                    None,
                ) {
                    result.push(pc);
                }
            }
        }
    }

    result
}

/// Port of `CU_SHAPE_CIRCLE::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_cu_circle_be_arc(
    s: &CuShapeCircle,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let be_arc_pos = a_s2.pos();
    let be_arc_radius = a_s2.radius();
    let be_arc_start_angle = a_s2.start_angle();
    let be_arc_end_angle = a_s2.end_angle();

    let bsc = BeShapeCircle::new(be_arc_pos, be_arc_radius);

    for pc in paths_cu_circle_be_circle(s, &bsc, a_max_weight, a_max_squared_weight) {
        let test_angle = a_s2.angle_between_start_and_end(vi(pc.a2));

        if test_angle < a_s2.end_angle() {
            result.push(pc);
        }
    }

    if result.len() < 2 {
        let bsp1 = BeShapePoint::new(a_s2.start_point());
        let bsp2 = BeShapePoint::new(a_s2.end_point());

        for bsp in [&bsp1, &bsp2] {
            for pc in paths_cu_circle_be_point(s, bsp, a_max_weight, a_max_squared_weight) {
                if !segment_intersects_arc(
                    vi(pc.a1),
                    vi(pc.a2),
                    be_arc_pos,
                    be_arc_radius as f64,
                    be_arc_start_angle,
                    be_arc_end_angle,
                    None,
                ) {
                    result.push(pc);
                }
            }
        }
    }

    result
}

/// Port of `CU_SHAPE_ARC::Paths( const BE_SHAPE_CIRCLE& )`. (The board edge circle itself is
/// not used by the C++ body.)
pub fn paths_cu_arc_be_circle(
    s: &CuShapeArc,
    _a_s2: &BeShapeCircle,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let csc = CuShapeCircle::new(s.pos(), s.radius() as f64 + s.width() / 2.0);

    for pc in paths_cu_arc_cu_circle(s, &csc, a_max_weight, a_max_squared_weight) {
        let test_angle = s.angle_between_start_and_end(vi(pc.a2));

        if test_angle < s.end_angle() {
            result.push(pc);
        }
    }

    if result.len() < 2 {
        let csc1 = CuShapeCircle::new(s.start_point(), s.width() / 2.0);
        let csc2 = CuShapeCircle::new(s.end_point(), s.width() / 2.0);

        result.extend(paths_cu_arc_cu_circle(s, &csc1, a_max_weight, a_max_squared_weight));
        result.extend(paths_cu_arc_cu_circle(s, &csc2, a_max_weight, a_max_squared_weight));
    }

    result
}

/// Port of `CU_SHAPE_ARC::Paths( const BE_SHAPE_ARC& )`.
pub fn paths_cu_arc_be_arc(
    s: &CuShapeArc,
    a_s2: &BeShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let be_arc_pos = a_s2.pos();
    let be_arc_radius = a_s2.radius();
    let be_arc_start_angle = a_s2.start_angle();
    let be_arc_end_angle = a_s2.end_angle();

    let bsc = BeShapeCircle::new(a_s2.pos(), a_s2.radius());

    for pc in paths_cu_arc_be_circle(s, &bsc, a_max_weight, a_max_squared_weight) {
        let test_angle = a_s2.angle_between_start_and_end(vi(pc.a2));

        if test_angle < a_s2.end_angle() {
            result.push(pc);
        }
    }

    if result.len() < 2 {
        let bsp1 = BeShapePoint::new(a_s2.start_point());
        let bsp2 = BeShapePoint::new(a_s2.end_point());

        for bsp in [&bsp1, &bsp2] {
            for pc in paths_cu_arc_be_point(s, bsp, a_max_weight, a_max_squared_weight) {
                if !segment_intersects_arc(
                    vi(pc.a1),
                    vi(pc.a2),
                    be_arc_pos,
                    be_arc_radius as f64,
                    be_arc_start_angle,
                    be_arc_end_angle,
                    None,
                ) {
                    result.push(pc);
                }
            }
        }
    }

    result
}

/// Port of `CU_SHAPE_CIRCLE::Paths( const BE_SHAPE_POINT& )`.
pub fn paths_cu_circle_be_point(
    s: &CuShapeCircle,
    a_s2: &BeShapePoint,
    a_max_weight: f64,
    _a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let r = s.radius() as f64;
    let center = s.pos();
    let point = a_s2.pos();
    let weight = (center - point).euclidean_norm() as f64 - r;

    if weight > a_max_weight {
        return result;
    }

    let pc = PathConnection {
        weight: weight.max(0.0),
        a2: vd(point),
        a1: vd(center + (point - center).resize(to_int(r))),
        ..PathConnection::default()
    };

    result.push(pc);
    result
}

/// Port of `CU_SHAPE_CIRCLE::Paths( const CU_SHAPE_CIRCLE& )`.
pub fn paths_cu_circle_cu_circle(
    s: &CuShapeCircle,
    a_s2: &CuShapeCircle,
    a_max_weight: f64,
    _a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let r1 = s.radius() as f64;
    let r2 = a_s2.radius() as f64;
    let c1 = s.pos();
    let c2 = a_s2.pos();

    if ((c1 - c2).squared_euclidean_norm() as f64) < (r1 - r2) * (r1 - r2) {
        // One of the circles is inside the other
        return result;
    }

    let weight = (c1 - c2).euclidean_norm() as f64 - r1 - r2;

    if weight > a_max_weight || weight < 0.0 {
        return result;
    }

    let pc = PathConnection {
        weight: weight.max(0.0),
        a1: vd((c2 - c1).resize(to_int(r1)) + c1),
        a2: vd((c1 - c2).resize(to_int(r2)) + c2),
        ..PathConnection::default()
    };

    result.push(pc);
    result
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const CU_SHAPE_CIRCLE& )`.
pub fn paths_cu_segment_cu_circle(
    s: &CuShapeSegment,
    a_s2: &CuShapeCircle,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let s_start = s.start();
    let s_end = s.end();
    let half_width = s.width() / 2.0;

    let point_pos = a_s2.pos();

    let length = (s_start - s_end).euclidean_norm() as f64;
    let projected_pos = track_projection(s_start, s_end, point_pos);

    if (projected_pos <= 0.0) || (s_start == s_end) {
        let csc = CuShapeCircle::new(s_start, half_width);
        return paths_cu_circle_cu_circle(&csc, a_s2, a_max_weight, a_max_squared_weight);
    }

    if projected_pos >= length {
        let csc = CuShapeCircle::new(s_end, half_width);
        return paths_cu_circle_cu_circle(&csc, a_s2, a_max_weight, a_max_squared_weight);
    }

    let radius = a_s2.radius() as f64;
    let track_side: f64 = if (s_end - s_start).cross(point_pos - s_start) > 0 {
        1.0
    } else {
        -1.0
    };

    let mut pc = PathConnection::default();
    pc.a1 = vd(s_start + (s_end - s_start).resize(to_int(projected_pos)))
        + vd((s_end - s_start).perpendicular().resize(to_int(half_width))) * track_side;
    pc.a2 = (pc.a1 - vd(point_pos)).resize(radius) + vd(point_pos);
    pc.weight = (pc.a2 - pc.a1).squared_euclidean_norm();

    if pc.weight <= a_max_squared_weight {
        pc.weight = pc.weight.sqrt();
        result.push(pc);
    }

    result
}

/// Port of `CU_SHAPE_CIRCLE::Paths( const CU_SHAPE_ARC& )`.
pub fn paths_cu_circle_cu_arc(
    s: &CuShapeCircle,
    a_s2: &CuShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let circle_pos = s.pos();
    let arc_pos = a_s2.pos();

    let circle_radius = s.radius() as f64;
    let arc_radius = a_s2.radius() as f64;

    let start_point = a_s2.start_point();
    let end_point = a_s2.end_point();

    let csc = CuShapeCircle::new(arc_pos, arc_radius + a_s2.width() / 2.0);

    if (circle_pos - arc_pos).euclidean_norm() as f64 > arc_radius + circle_radius {
        let pcs = paths_cu_circle_cu_circle(s, &csc, a_max_weight, a_max_squared_weight);

        if pcs.len() == 1 {
            let test_angle = a_s2.angle_between_start_and_end(vi(pcs[0].a2));

            if test_angle < a_s2.end_angle() {
                result.push(pcs[0].clone());
                return result;
            }
        }
    }

    let csc1 = CuShapeCircle::new(start_point, a_s2.width() / 2.0);
    let csc2 = CuShapeCircle::new(end_point, a_s2.width() / 2.0);

    let mut best_path: Option<PathConnection> = None;

    let pcs1 = paths_cu_circle_cu_circle(s, &csc1, a_max_weight, a_max_squared_weight);
    let pcs2 = paths_cu_circle_cu_circle(s, &csc2, a_max_weight, a_max_squared_weight);

    for pc in pcs1.iter().chain(pcs2.iter()) {
        if best_path
            .as_ref()
            .is_none_or(|b| (b.weight > pc.weight) && (pc.weight > 0.0))
        {
            best_path = Some(pc.clone());
        }
    }

    // If the circle center is insde the arc ring

    if ((circle_pos - arc_pos).squared_euclidean_norm() as f64) < arc_radius * arc_radius {
        if circle_pos != arc_pos {
            // The best path is already found otherwise
            let test_angle = a_s2.angle_between_start_and_end(circle_pos);

            if test_angle < a_s2.end_angle() {
                let pc3 = PathConnection {
                    weight: (arc_radius
                        - (circle_pos - arc_pos).euclidean_norm() as f64
                        - circle_radius)
                        .max(0.0),
                    a1: vd(circle_pos + (circle_pos - arc_pos).resize(to_int(circle_radius))),
                    a2: vd(arc_pos
                        + (circle_pos - arc_pos)
                            .resize(to_int(arc_radius - a_s2.width() / 2.0))),
                    ..PathConnection::default()
                };

                if best_path
                    .as_ref()
                    .is_none_or(|b| (b.weight > pc3.weight) && (pc3.weight > 0.0))
                {
                    best_path = Some(pc3);
                }
            }
        }
    }

    if let Some(best) = best_path
        && best.weight > 0.0
    {
        result.push(best);
    }

    result
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const CU_SHAPE_ARC& )`.
///
/// The C++ keeps `bestPath` pointing into the temporary vectors of the range-for loops after
/// they are destroyed (a dangling pointer); the port keeps the value the pointer referred to
/// when it was taken.
pub fn paths_cu_segment_cu_arc(
    s: &CuShapeSegment,
    a_s2: &CuShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let s_start = s.start();
    let s_end = s.end();
    let half_width1 = s.width() / 2.0;

    let arc_pos = a_s2.pos();
    let arc_radius = a_s2.radius() as f64;
    let half_width2 = a_s2.width() / 2.0;

    let csc = CuShapeCircle::new(arc_pos, arc_radius + half_width2);

    let pcs = paths_cu_segment_cu_circle(s, &csc, a_max_weight, a_max_squared_weight);

    if pcs.is_empty() {
        return result;
    }

    let test_angle = a_s2.angle_between_start_and_end(vi(pcs[0].a1));

    if test_angle < a_s2.end_angle() {
        result.push(pcs[0].clone());
        return result;
    }

    let csc1 = CuShapeCircle::new(a_s2.start_point(), half_width2);
    let csc2 = CuShapeCircle::new(a_s2.end_point(), half_width2);
    let mut best_path: Option<PathConnection> = None;

    let mut consider = |pcs: Vec<PathConnection>| {
        for pc in pcs {
            if best_path.as_ref().is_none_or(|b| b.weight > pc.weight) {
                best_path = Some(pc);
            }
        }
    };

    consider(paths_cu_segment_cu_circle(s, &csc1, a_max_weight, a_max_squared_weight));
    consider(paths_cu_segment_cu_circle(s, &csc2, a_max_weight, a_max_squared_weight));

    let csc3 = CuShapeCircle::new(s_start, half_width1);
    let csc4 = CuShapeCircle::new(s_end, half_width1);

    consider(paths_cu_circle_cu_arc(&csc3, a_s2, a_max_weight, a_max_squared_weight));
    consider(paths_cu_circle_cu_arc(&csc4, a_s2, a_max_weight, a_max_squared_weight));

    if let Some(best) = best_path {
        result.push(best);
    }

    result
}

/// Port of `closestPointOnSegment()`: the projection of `p` onto the segment `a b`.
pub fn closest_point_on_segment(a: Vector2I, b: Vector2I, p: Vector2I) -> Vector2I {
    if a == b {
        return a;
    }

    if a == p {
        return a;
    }

    let ab = b - a;
    let ap = p - a;

    let mut t: f64 = ((ab.dot(ap) as f32) / (ab.squared_euclidean_norm() as f32)) as f64;

    // Clamp t to the range [0, 1] to restrict the projection to the segment
    t = 0.0_f64.max(1.0_f64.min(t));

    vi(vd(a) + vd(ab) * t)
}

/// Port of `CU_SHAPE_SEGMENT::Paths( const CU_SHAPE_SEGMENT& )`.
pub fn paths_cu_segment_cu_segment(
    s: &CuShapeSegment,
    a_s2: &CuShapeSegment,
    a_max_weight: f64,
    _a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let a = s.start();
    let b = s.end();
    let half_width1 = s.width() / 2.0;

    let c = a_s2.start();
    let d = a_s2.end();
    let half_width2 = a_s2.width() / 2.0;

    let p1 = closest_point_on_segment(a, b, c);
    let p2 = closest_point_on_segment(a, b, d);
    let p3 = closest_point_on_segment(c, d, a);
    let p4 = closest_point_on_segment(c, d, b);

    // Calculate all possible squared distances between the segments
    let dist1 = (p1 - c).squared_euclidean_norm() as f64;
    let dist2 = (p2 - d).squared_euclidean_norm() as f64;
    let dist3 = (p3 - a).squared_euclidean_norm() as f64;
    let dist4 = (p4 - b).squared_euclidean_norm() as f64;

    // Find the minimum squared distance and update closest points
    let mut min_dist = dist1;
    let mut closest1 = p1;
    let mut closest2 = c;

    if dist2 < min_dist {
        min_dist = dist2;
        closest1 = p2;
        closest2 = d;
    }

    if dist3 < min_dist {
        min_dist = dist3;
        closest1 = a;
        closest2 = p3;
    }

    if dist4 < min_dist {
        min_dist = dist4;
        closest1 = b;
        closest2 = p4;
    }

    let pc = PathConnection {
        a1: vd(closest1 + (closest2 - closest1).resize(to_int(half_width1))),
        a2: vd(closest2 + (closest1 - closest2).resize(to_int(half_width2))),
        weight: (min_dist.sqrt() - half_width1 - half_width2).max(0.0),
        ..PathConnection::default()
    };

    if pc.weight <= a_max_weight {
        result.push(pc);
    }

    result
}

/// Port of `CU_SHAPE_CIRCLE::Paths( const BE_SHAPE_CIRCLE& )`.
pub fn paths_cu_circle_be_circle(
    s: &CuShapeCircle,
    a_s2: &BeShapeCircle,
    a_max_weight: f64,
    _a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let r1 = s.radius() as f64;
    let r2 = a_s2.radius() as f64;
    let center1 = s.pos();
    let center2 = a_s2.pos();
    let dist = (center1 - center2).euclidean_norm() as f64;

    // Prune on the tangent sqrt(dist^2 - R2^2) - R1, which is much shorter than the centre
    // distance beside a large hole
    let reach = a_max_weight + r1;

    if dist == 0.0 || dist * dist > reach * reach + r2 * r2 {
        return result;
    }

    let circle_angle = EdaAngle::from_vector(center2 - center1).as_radians();

    if dist <= r2 {
        // Copper circle center is inside the board-edge circle so external tangent lines
        // don't exist. The nearest gap is the radial distance between circle boundaries.
        let weight = (r2 - dist - r1).max(0.0);

        if weight > a_max_weight {
            return result;
        }

        let radial_angle = circle_angle + std::f64::consts::PI;
        let cx = cmath::cos(radial_angle);
        let cy = cmath::sin(radial_angle);
        let p_end = center2 + Vector2I::new(to_int(r2 * cx), to_int(r2 * cy));
        let p_start = center1 + Vector2I::new(to_int(r1 * cx), to_int(r1 * cy));

        let pc = PathConnection {
            a1: vd(p_start),
            a2: vd(p_end),
            weight,
            ..PathConnection::default()
        };

        // Callers expect two entries (one per tangent side) and select by index.
        result.push(pc.clone());
        result.push(pc);

        return result;
    }

    let weight = (dist * dist - r2 * r2).sqrt() - r1;
    let theta = cmath::asin(r2 / dist);
    let psi = cmath::acos(r2 / dist);

    if weight > a_max_weight {
        return result;
    }

    let mut pc = PathConnection {
        weight: weight.max(0.0),
        ..PathConnection::default()
    };

    let mut p_start = Vector2I::new(
        to_int(r1 * cmath::cos(theta + circle_angle)),
        to_int(r1 * cmath::sin(theta + circle_angle)),
    );
    p_start += center1;
    let mut p_end = Vector2I::new(
        to_int(-r2 * cmath::cos(psi - circle_angle)),
        to_int(r2 * cmath::sin(psi - circle_angle)),
    );
    p_end += center2;

    pc.a1 = vd(p_start);
    pc.a2 = vd(p_end);
    result.push(pc.clone());

    p_start = Vector2I::new(
        to_int(r1 * cmath::cos(-theta + circle_angle)),
        to_int(r1 * cmath::sin(-theta + circle_angle)),
    );
    p_start += center1;
    p_end = Vector2I::new(
        to_int(-r2 * cmath::cos(-psi - circle_angle)),
        to_int(r2 * cmath::sin(-psi - circle_angle)),
    );
    p_end += center2;

    pc.a1 = vd(p_start);
    pc.a2 = vd(p_end);

    result.push(pc);
    result
}

/// Port of `CU_SHAPE_ARC::Paths( const BE_SHAPE_POINT& )`.
pub fn paths_cu_arc_be_point(
    s: &CuShapeArc,
    a_s2: &BeShapePoint,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();
    let point = a_s2.pos();
    let arc_center = s.pos();

    let radius = s.radius() as f64;
    let width = s.width();

    let angle = s.angle_between_start_and_end(point);

    if angle < s.end_angle() {
        if ((point - arc_center).squared_euclidean_norm() as f64) > radius * radius {
            let circle = CuShapeCircle::new(arc_center, radius + width / 2.0);
            paths_cu_circle_be_point(&circle, a_s2, a_max_weight, a_max_squared_weight)
        } else {
            let pc = PathConnection {
                weight: ((radius - width / 2.0) - (point - arc_center).euclidean_norm() as f64)
                    .max(0.0),
                a1: vd((point - arc_center).resize(to_int(radius - width / 2.0)) + arc_center),
                a2: vd(point),
                ..PathConnection::default()
            };

            if pc.weight > 0.0 && pc.weight < a_max_weight {
                result.push(pc);
            }

            result
        }
    } else {
        let nearest_point = if (point - s.start_point()).squared_euclidean_norm()
            > (point - s.end_point()).squared_euclidean_norm()
        {
            s.end_point()
        } else {
            s.start_point()
        };

        let circle = CuShapeCircle::new(nearest_point, width / 2.0);
        paths_cu_circle_be_point(&circle, a_s2, a_max_weight, a_max_squared_weight)
    }
}

/// Port of `CU_SHAPE_ARC::Paths( const CU_SHAPE_ARC& )`.
pub fn paths_cu_arc_cu_arc(
    s: &CuShapeArc,
    a_s2: &CuShapeArc,
    a_max_weight: f64,
    a_max_squared_weight: f64,
) -> Vec<PathConnection> {
    let mut result = Vec::new();

    let r1 = s.radius() as f64;
    let r2 = a_s2.radius() as f64;

    let c1 = s.pos();
    let c2 = a_s2.pos();

    let mut best_path = PathConnection {
        weight: f64::INFINITY,
        ..PathConnection::default()
    };
    let csc1 = CuShapeCircle::new(c1, r1 + s.width() / 2.0);
    let csc2 = CuShapeCircle::new(c2, r2 + a_s2.width() / 2.0);

    let csc3 = CuShapeCircle::new(s.start_point(), s.width() / 2.0);
    let csc4 = CuShapeCircle::new(s.end_point(), s.width() / 2.0);
    let csc5 = CuShapeCircle::new(a_s2.start_point(), a_s2.width() / 2.0);
    let csc6 = CuShapeCircle::new(a_s2.end_point(), a_s2.width() / 2.0);

    let (w, w2) = (a_max_weight, a_max_squared_weight);

    for pcs in [
        paths_cu_circle_cu_circle(&csc1, &csc2, w, w2),
        paths_cu_arc_cu_circle(s, &csc2, w, w2),
        paths_cu_circle_cu_arc(&csc1, a_s2, w, w2),
    ] {
        for pc in pcs {
            let test_angle1 = s.angle_between_start_and_end(vi(pc.a1));
            let test_angle2 = a_s2.angle_between_start_and_end(vi(pc.a2));

            if test_angle1 < s.end_angle()
                && test_angle2 < a_s2.end_angle()
                && best_path.weight > pc.weight
            {
                best_path = pc;
            }
        }
    }

    for pcs in [
        paths_cu_arc_cu_circle(s, &csc5, w, w2),
        paths_cu_arc_cu_circle(s, &csc6, w, w2),
        paths_cu_circle_cu_arc(&csc3, a_s2, w, w2),
        paths_cu_circle_cu_arc(&csc4, a_s2, w, w2),
    ] {
        for pc in pcs {
            if best_path.weight > pc.weight {
                best_path = pc;
            }
        }
    }

    if best_path.weight != f64::INFINITY {
        result.push(best_path);
    }

    result
}
