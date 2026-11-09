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
// Based on Uniform Plane Subdivision algorithm from Lamot, Marko, and Borut Zalik.
// "A fast polygon triangulation algorithm based on uniform plane subdivision."
// Computers & graphics 27, no. 2 (2003): 239-253.
//
// Code derived from:
// K-3D which is Copyright (c) 2005-2006, Romain Behar, GPL-2, license above
// earcut which is Copyright (c) 2016, Mapbox, ISC
//
// ISC License:
// Permission to use, copy, modify, and/or distribute this software for any purpose
// with or without fee is hereby granted, provided that the above copyright notice
// and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
// REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
// FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
// INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
// OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
// TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
// THIS SOFTWARE.

//! Port of `libs/kimath/include/geometry/polygon_triangulation.h` (`POLYGON_TRIANGULATION`):
//! ear clipping triangulation of a polygon (with holes bridged into the outline), with z-order
//! hashing, simplification and decimation of the ring, the recovery passes (null triangle
//! removal, subdivision, splitting, clipping of the remaining ears), the balanced partition
//! of huge outlines, and the Delaunay / sliver refinement of the result.
//!
//! The `wxLogTrace` diagnostics (`logRemaining`, `logVertices`) are dropped.  The
//! `ADVANCED_CFG` values are the built-in defaults ([`TRIANGULATE_SIMPLIFICATION_LEVEL`],
//! [`TRIANGULATE_MINIMUM_AREA`], [`TRIANGULATE_DELAUNAY_REFINE`]).

use crate::cmath;
use crate::geometry::shape::Shape;
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_poly_set::{Polygon, TriangulatedPolygon};
use crate::geometry::vertex_set::{NIL, VertexId, VertexSet, area_xy};
use crate::math::box2::Box2I;
use crate::math::util::{std_max, std_min};
use crate::math::vector2::Vector2I;
use crate::stdsort::std_sort;

/// `ADVANCED_CFG::m_TriangulateSimplificationLevel` (default): the distance below which
/// consecutive vertices are merged.
pub const TRIANGULATE_SIMPLIFICATION_LEVEL: i32 = 50;

/// `ADVANCED_CFG::m_TriangulateMinimumArea` (default).
pub const TRIANGULATE_MINIMUM_AREA: i32 = 1000;

/// `ADVANCED_CFG::m_TriangulateDelaunayRefine` (default): whether the sliver-minimizing flip
/// post-pass (`TRIANGULATED_POLYGON::Refine()`) runs.
pub const TRIANGULATE_DELAUNAY_REFINE: bool = true;

/// Port of `triangulationRefineEnabled()`.
#[inline]
fn triangulation_refine_enabled() -> bool {
    TRIANGULATE_DELAUNAY_REFINE
}

/// Port of `POLYGON_TRIANGULATION::SCANLINE_HIT`.
#[derive(Clone, Copy, Debug, Default)]
struct ScanlineHit {
    #[allow(dead_code)]
    edge_index: i32,
    point: Vector2I,
}

/// Port of `POLYGON_TRIANGULATION`: triangulates into the `TRIANGULATED_POLYGON` it was
/// created for.  (The C++ class derives from `VERTEX_SET`; the vertex set is the public field
/// [`vs`](Self::vs).)
pub struct PolygonTriangulation<'a> {
    /// The `VERTEX_SET` base
    pub vs: VertexSet,
    vertices_original_size: usize,
    result: &'a mut TriangulatedPolygon,
}

impl<'a> PolygonTriangulation<'a> {
    /// Port of `POLYGON_TRIANGULATION( TRIANGULATED_POLYGON& aResult )`.
    pub fn new(a_result: &'a mut TriangulatedPolygon) -> Self {
        PolygonTriangulation {
            vs: VertexSet::new(TRIANGULATE_SIMPLIFICATION_LEVEL),
            vertices_original_size: 0,
            result: a_result,
        }
    }

    /// The result (`m_result`).
    pub fn result(&self) -> &TriangulatedPolygon {
        self.result
    }

    /// The result (`m_result`), mutable.
    pub fn result_mut(&mut self) -> &mut TriangulatedPolygon {
        self.result
    }

    // Shorthands for the vertex links.
    #[inline]
    fn nx(&self, v: VertexId) -> VertexId {
        self.vs.vertices[v].next
    }

    #[inline]
    fn pv(&self, v: VertexId) -> VertexId {
        self.vs.vertices[v].prev
    }

    #[inline]
    fn vx(&self, v: VertexId) -> f64 {
        self.vs.vertices[v].x
    }

    #[inline]
    fn vy(&self, v: VertexId) -> f64 {
        self.vs.vertices[v].y
    }

    #[inline]
    fn vi(&self, v: VertexId) -> i32 {
        self.vs.vertices[v].i
    }

    #[inline]
    fn area(&self, p: VertexId, q: VertexId, r: VertexId) -> f64 {
        self.vs.area(p, q, r)
    }

    /// Port of `TesselatePolygon( const SHAPE_POLY_SET::POLYGON&, TRIANGULATED_POLYGON* )`:
    /// triangulate a polygon with holes by bridging holes directly into the outer ring's
    /// vertex list, avoiding the `Fracture()` intermediate step.
    pub fn tesselate_polygon(
        &mut self,
        a_polygon: &Polygon,
        a_hint_data: Option<&TriangulatedPolygon>,
    ) -> bool {
        if a_polygon.is_empty() {
            return true;
        }

        if a_polygon.len() == 1 {
            return self.tesselate_chain(&a_polygon[0], a_hint_data);
        }

        let outline = &a_polygon[0];

        self.vs.bbox = outline.bbox(0);

        for hole in a_polygon.iter().skip(1) {
            self.vs.bbox.merge(&hole.bbox(0));
        }

        self.result.clear();

        if self.vs.bbox.width() == 0 || self.vs.bbox.height() == 0 {
            return true;
        }

        for chain in a_polygon {
            for &pt in chain.c_points() {
                self.result.add_vertex(pt);
            }
        }

        let mut base_index = 0;
        let mut outer_ring = self.create_ring(outline, base_index, true);
        base_index += outline.point_count();

        if outer_ring == NIL || self.pv(outer_ring) == self.nx(outer_ring) {
            return true;
        }

        let mut hole_rings: Vec<VertexId> = Vec::new();

        for hole in a_polygon.iter().skip(1) {
            let hole_ring = self.create_ring(hole, base_index, false);
            base_index += hole.point_count();

            // Reject rings collapsed below 3 vertices.  Match the outer-ring guard above so
            // degenerate holes (single point or two-vertex sliver) cannot reach
            // eliminateHoles().
            if hole_ring != NIL && self.pv(hole_ring) != self.nx(hole_ring) {
                hole_rings.push(hole_ring);
            }
        }

        self.vertices_original_size = self.vs.vertices.len();

        if !hole_rings.is_empty() {
            outer_ring = self.eliminate_holes(outer_ring, &hole_rings);

            if outer_ring == NIL {
                // "Hole elimination failed"
                return false;
            }
        }

        let simplified = self.simplify_list(outer_ring);

        if simplified != NIL {
            outer_ring = simplified;
        }

        self.vs.update_list(outer_ring);

        let decimated = self.decimate_list(outer_ring);

        if decimated != NIL {
            outer_ring = decimated;
        }

        let retval = self.earcut_list(outer_ring, 0);

        if retval && triangulation_refine_enabled() {
            self.result.refine();
        }

        self.vs.vertices.clear();
        retval
    }

    /// Port of `TesselatePolygon( const SHAPE_LINE_CHAIN&, TRIANGULATED_POLYGON* )`.
    pub fn tesselate_chain(
        &mut self,
        a_poly: &ShapeLineChain,
        a_hint_data: Option<&TriangulatedPolygon>,
    ) -> bool {
        self.vs.bbox = a_poly.bbox(0);
        self.result.clear();

        if self.vs.bbox.width() == 0 || self.vs.bbox.height() == 0 {
            return true;
        }

        // Place the polygon Vertices into a circular linked list
        // and check for lists that have only 0, 1 or 2 elements and
        // therefore cannot be polygons
        let mut first_vertex = self.vs.create_list(a_poly, NIL, 0);

        for &pt in a_poly.c_points() {
            self.result.add_vertex(pt);
        }

        if first_vertex == NIL || self.pv(first_vertex) == self.nx(first_vertex) {
            return true;
        }

        self.vertices_original_size = self.vs.vertices.len();

        let simplified = self.simplify_list(first_vertex);

        if simplified != NIL {
            first_vertex = simplified;
        }

        self.vs.update_list(first_vertex);

        // If we have hint data, we can skip the tesselation process as long as the
        // hint source did not need to subdivide the polygon.  Hint triangle indices
        // refer to the original vertex list stored in m_result, not the simplified
        // working ring used internally during triangulation.
        if let Some(hint) = a_hint_data
            && hint.vertices.len() == self.result.vertex_count()
        {
            self.result.set_triangles(&hint.triangles);
            return true;
        }

        let decimated = self.decimate_list(first_vertex);

        if decimated != NIL {
            first_vertex = decimated;
        }

        let retval = self.earcut_list(first_vertex, 0);

        if retval && triangulation_refine_enabled() {
            self.result.refine();
        }

        self.vs.vertices.clear();
        retval
    }

    /// Port of `PartitionAreaFractionsForTesting()`: the area fractions of the balanced
    /// partition of `a_poly` into (at most) `a_target_leaves` leaves.
    pub fn partition_area_fractions_for_testing(
        &self,
        a_poly: &ShapeLineChain,
        a_target_leaves: usize,
    ) -> Vec<f64> {
        let partitions = self.partition_polygon_balanced(a_poly, a_target_leaves);
        let mut fractions = Vec::new();
        let total_area = a_poly.area(true).abs();

        if total_area <= 0.0 {
            return fractions;
        }

        fractions.reserve(partitions.len());

        for part in &partitions {
            fractions.push(part.area(true).abs() / total_area);
        }

        fractions
    }

    /// Port of `collectScanlineHits()`: the two crossings of the cut line with the polygon
    /// edges, false unless there are exactly two.
    fn collect_scanline_hits(
        &self,
        a_poly: &ShapeLineChain,
        a_vertical: bool,
        a_cut: i32,
        a_hits: &mut [ScanlineHit; 2],
    ) -> bool {
        let mut count = 0;

        for ii in 0..a_poly.point_count() {
            let a = a_poly.c_point(ii);
            let b = a_poly.c_point((ii + 1) % a_poly.point_count());

            // int arithmetic wraps on x86 where the C++ overflows (extreme coordinates)
            if a_vertical {
                if a.x == b.x || a_cut <= std_min(a.x, b.x) || a_cut >= std_max(a.x, b.x) {
                    continue;
                }

                if count >= 2 {
                    return false;
                }

                let t = a_cut.wrapping_sub(a.x) as f64 / b.x.wrapping_sub(a.x) as f64;
                let y = lround_to_int(a.y as f64 + t * b.y.wrapping_sub(a.y) as f64);
                a_hits[count] = ScanlineHit {
                    edge_index: ii,
                    point: Vector2I::new(a_cut, y),
                };
                count += 1;
            } else {
                if a.y == b.y || a_cut <= std_min(a.y, b.y) || a_cut >= std_max(a.y, b.y) {
                    continue;
                }

                if count >= 2 {
                    return false;
                }

                let t = a_cut.wrapping_sub(a.y) as f64 / b.y.wrapping_sub(a.y) as f64;
                let x = lround_to_int(a.x as f64 + t * b.x.wrapping_sub(a.x) as f64);
                a_hits[count] = ScanlineHit {
                    edge_index: ii,
                    point: Vector2I::new(x, a_cut),
                };
                count += 1;
            }
        }

        count == 2
    }

    /// Port of `createSplitChild()`: the closed, simplified sub-chain from `a_start` to
    /// `a_end`.
    fn create_split_child(
        &self,
        a_poly: &ShapeLineChain,
        a_start: i32,
        a_end: i32,
    ) -> ShapeLineChain {
        let mut child = ShapeLineChain::new();
        let mut idx = a_start;
        let mut guard = 0;
        let count = a_poly.point_count();

        loop {
            child.append_point(a_poly.c_point(idx), false);
            idx = (idx + 1) % count;
            guard += 1;

            if !(idx != (a_end + 1) % count && guard <= count + 2) {
                break;
            }
        }

        child.set_closed(true);
        child.simplify2(true);
        child
    }

    /// Port of `splitPolygonAtCoordinate()`.
    fn split_polygon_at_coordinate(
        &self,
        a_poly: &ShapeLineChain,
        a_vertical: bool,
        a_cut: i32,
    ) -> Option<([ShapeLineChain; 2], f64, f64)> {
        let mut hits = [ScanlineHit::default(); 2];

        if !self.collect_scanline_hits(a_poly, a_vertical, a_cut, &mut hits) {
            return None;
        }

        let mut augmented = a_poly.clone();
        augmented.split(hits[0].point, true);
        augmented.split(hits[1].point, true);

        let idx_a = augmented.find(hits[0].point, 0);
        let idx_b = augmented.find(hits[1].point, 0);

        if idx_a < 0 || idx_b < 0 || idx_a == idx_b {
            return None;
        }

        let children = [
            self.create_split_child(&augmented, idx_a, idx_b),
            self.create_split_child(&augmented, idx_b, idx_a),
        ];

        if children[0].point_count() < 3 || children[1].point_count() < 3 {
            return None;
        }

        let area_a = children[0].area(true).abs();
        let area_b = children[1].area(true).abs();

        if area_a > 0.0 && area_b > 0.0 {
            Some((children, area_a, area_b))
        } else {
            None
        }
    }

    /// Port of `splitPolygonBalanced()`: split `a_poly` into two children of similar area
    /// along the longer bounding box axis (or the other one).
    fn split_polygon_balanced(&self, a_poly: &ShapeLineChain) -> Option<[ShapeLineChain; 2]> {
        let bbox: Box2I = a_poly.bbox(0);
        let vertical_first = bbox.width() >= bbox.height();
        let total_area = a_poly.area(true).abs();

        if total_area <= 0.0 {
            return None;
        }

        let try_axis = |a_vertical: bool| -> Option<[ShapeLineChain; 2]> {
            let low = (if a_vertical { bbox.x() } else { bbox.y() }).wrapping_add(1);
            let high = (if a_vertical {
                bbox.right()
            } else {
                bbox.bottom()
            })
            .wrapping_sub(1);

            if high <= low {
                return None;
            }

            let mut best_imbalance = f64::INFINITY;
            let mut best_children: Option<[ShapeLineChain; 2]> = None;

            const K_SAMPLES: i32 = 15;

            for ii in 1..=K_SAMPLES {
                // int arithmetic: the product wraps on x86 for boxes wider than ~143 mm
                let cut =
                    low.wrapping_add(high.wrapping_sub(low).wrapping_mul(ii) / (K_SAMPLES + 1));

                let Some((candidate, area_a, area_b)) =
                    self.split_polygon_at_coordinate(a_poly, a_vertical, cut)
                else {
                    continue;
                };

                let imbalance = (area_a - area_b).abs() / total_area;

                if imbalance < best_imbalance {
                    best_imbalance = imbalance;
                    best_children = Some(candidate);
                }
            }

            if !best_imbalance.is_finite() || best_imbalance > 0.35 {
                return None;
            }

            // A finite imbalance means a candidate was taken
            best_children
        };

        try_axis(vertical_first).or_else(|| try_axis(!vertical_first))
    }

    /// Port of `partitionPolygonBalanced()`: repeatedly split the largest leaf until there
    /// are `a_target_leaves` leaves or no leaf can be split.
    pub(crate) fn partition_polygon_balanced(
        &self,
        a_poly: &ShapeLineChain,
        a_target_leaves: usize,
    ) -> Vec<ShapeLineChain> {
        let mut leaves = vec![a_poly.clone()];

        if a_target_leaves < 2 {
            return leaves;
        }

        while leaves.len() < a_target_leaves {
            let mut best_leaf: i32 = -1;
            let mut best_area = 0.0;

            for (ii, leaf) in leaves.iter().enumerate() {
                let area = leaf.area(true).abs();

                if area > best_area {
                    best_area = area;
                    best_leaf = ii as i32;
                }
            }

            if best_leaf < 0 {
                break;
            }

            let Some([c0, c1]) = self.split_polygon_balanced(&leaves[best_leaf as usize]) else {
                break;
            };

            leaves[best_leaf as usize] = c0;
            leaves.push(c1);
        }

        leaves
    }

    /// Port of `suggestedPartitionLeafCount()`.
    pub(crate) fn suggested_partition_leaf_count(&self, a_poly: &ShapeLineChain) -> usize {
        const K_VERTICES_PER_LEAF: usize = 50000;
        const K_MAX_LEAVES: usize = 8;
        let mut leaves = 1;

        while leaves < K_MAX_LEAVES
            && (a_poly.point_count() as usize) / leaves > K_VERTICES_PER_LEAF
        {
            leaves *= 2;
        }

        leaves
    }

    /// Port of `simplifyList()`: remove points that are too close to each other.  Returns
    /// `NIL` if no points are removed.
    fn simplify_list(&mut self, a_start: VertexId) -> VertexId {
        if a_start == NIL || self.nx(a_start) == self.pv(a_start) {
            return a_start;
        }

        let mut p = a_start;
        let mut next = self.nx(p);
        let mut retval = a_start;
        let mut count = 0;

        let mut sq_dist = TRIANGULATE_SIMPLIFICATION_LEVEL as f64;
        sq_dist *= sq_dist;

        loop {
            let dx = self.vx(next) - self.vx(p);
            let dy = self.vy(next) - self.vy(p);

            if dx * dx + dy * dy < sq_dist {
                if next == a_start {
                    retval = p;
                    self.vs.remove(a_start);
                    count += 1;
                    break;
                }

                next = self.nx(next);
                let pn = self.nx(p);
                self.vs.remove(pn);
                count += 1;
                retval = p;
            } else {
                p = next;
                next = self.nx(next);
            }

            if !(p != a_start && next != NIL && p != NIL) {
                break;
            }
        }

        if count != 0 { retval } else { NIL }
    }

    /// Port of `decimateList()`: replace near-collinear runs with a single chord.  Returns the
    /// new list head, or `NIL` if nothing was removed.
    ///
    /// A run is absorbed only when every vertex deviates from the chord by at most the
    /// simplification level and the spanned triangle contains no other vertex.  Empty
    /// triangles are what guarantee no self-intersection: a segment can cross the new chord
    /// without crossing the edges it replaces only if an endpoint lies inside that triangle,
    /// so this keeps fracture bridges intact.  Requires a z-sorted list; run after
    /// `update_list()`.
    fn decimate_list(&mut self, a_start: VertexId) -> VertexId {
        let mut a_start = a_start;

        if a_start == NIL || self.nx(a_start) == self.pv(a_start) {
            return NIL;
        }

        let eps = TRIANGULATE_SIMPLIFICATION_LEVEL as f64;
        let eps_sq = eps * eps;

        // Caps the quadratic chord re-validation as a run grows.
        const K_MAX_RUN: usize = 256;

        let mut ring_size: usize = 1;
        let head = a_start;

        // The sentinel is never absorbed; anchor it at the lexicographic max, a hull corner
        // no valid decimation would remove.
        let mut v = self.nx(head);

        while v != head {
            ring_size += 1;

            if self.vx(v) > self.vx(a_start)
                || (self.vx(v) == self.vx(a_start) && self.vy(v) > self.vy(a_start))
            {
                a_start = v;
            }

            v = self.nx(v);
        }

        if ring_size < 8 {
            return NIL;
        }

        // Cumulative signed-area budget bounds net drift even when every removal cuts the same
        // way.  Absolute term caps large rings to the QA coverage tolerance; relative term
        // keeps small rings honest.
        let area_budget = std_min(self.vs.ring_area(a_start, NIL).abs() * 2e-4, 2.5e8);
        let mut area_used = 0.0;

        let in_band = |s: &VertexSet, p: VertexId, a: VertexId, b: VertexId, a_eps_sq: f64| {
            let (p, a, b) = (s.v(p), s.v(a), s.v(b));
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let vx = p.x - a.x;
            let vy = p.y - a.y;
            let len_sq = dx * dx + dy * dy;
            let dot = dx * vx + dy * vy;

            if len_sq <= 0.0 {
                return vx * vx + vy * vy <= a_eps_sq;
            }

            if dot < 0.0 || dot > len_sq {
                return false;
            }

            let cross = dx * vy - dy * vx;

            cross * cross <= a_eps_sq * len_sq
        };

        let mut absorbed: Vec<VertexId> = Vec::with_capacity(K_MAX_RUN);

        let mut removed: usize = 0;
        let mut anchor = a_start;

        loop {
            let mut end = self.nx(anchor);
            absorbed.clear();

            while end != a_start && absorbed.len() < K_MAX_RUN && ring_size - removed >= 4 {
                let try_end = self.nx(end);

                // A degenerate chord would leave coordinate-duplicate neighbors behind.
                if self.vs.eq(anchor, try_end) {
                    break;
                }

                if !self.triangle_is_empty(anchor, end, try_end) {
                    break;
                }

                let delta = 0.5 * self.area(anchor, end, try_end);

                if (area_used + delta).abs() > area_budget {
                    break;
                }

                // Re-validate the whole run against the grown chord, so acceptance never
                // depends on removal order.
                let mut ok = in_band(&self.vs, end, anchor, try_end, eps_sq);

                for &v in &absorbed {
                    if !ok {
                        break;
                    }

                    ok = in_band(&self.vs, v, anchor, try_end, eps_sq);
                }

                if !ok {
                    break;
                }

                area_used += delta;
                absorbed.push(end);
                self.vs.remove(end);
                removed += 1;
                end = try_end;
            }

            anchor = end;

            if anchor == a_start {
                break;
            }
        }

        if removed != 0 { a_start } else { NIL }
    }

    /// The `triangleIsEmpty` lambda of `decimateList()`: boundary-inclusive, mirroring
    /// `isEar()`: fracture-bridge feet land on the triangle boundary and reject the removal.
    fn triangle_is_empty(&self, a_a: VertexId, a_b: VertexId, a_c: VertexId) -> bool {
        let mut a = a_a;
        let mut c = a_c;

        // inTriangle() assumes the negative-area winding isEar() queries with.
        if self.area(a_a, a_b, a_c) > 0.0 {
            std::mem::swap(&mut a, &mut c);
        }

        let vs = &self.vs;
        let (av, bv, cv) = (vs.v(a), vs.v(a_b), vs.v(c));
        let min_tx = std_min(av.x, std_min(bv.x, cv.x));
        let min_ty = std_min(av.y, std_min(bv.y, cv.y));
        let max_tx = std_max(av.x, std_max(bv.x, cv.x));
        let max_ty = std_max(av.y, std_max(bv.y, cv.y));

        let min_z = vs.z_order(min_tx, min_ty);
        let max_z = vs.z_order(max_tx, max_ty);

        let mut p = bv.next_z;

        while p != NIL && vs.v(p).z <= max_z {
            if p != a_a && p != a_c && vs.v(p).in_triangle(av, bv, cv) {
                return false;
            }

            p = vs.v(p).next_z;
        }

        p = bv.prev_z;

        while p != NIL && vs.v(p).z >= min_z {
            if p != a_a && p != a_c && vs.v(p).in_triangle(av, bv, cv) {
                return false;
            }

            p = vs.v(p).prev_z;
        }

        true
    }

    /// The null-area threshold of `removeNullTriangles()`: four times the area of the dummy
    /// triangle (`VERTEX tmp`) formed with the midpoint of `p->prev` and `p->next`.
    fn null_area(&self, p: VertexId) -> f64 {
        let (pp, pn) = (self.pv(p), self.nx(p));
        let tx = 0.5 * (self.vx(pp) + self.vx(pn));
        let ty = 0.5 * (self.vy(pp) + self.vy(pn));
        4.0 * area_xy(self.vx(pp), self.vy(pp), tx, ty, self.vx(pn), self.vy(pn)).abs()
    }

    /// Port of `removeNullTriangles()`: iterate through the list to remove null triangles if
    /// they exist.
    ///
    /// This should only be called as a last resort when tesselation fails as the null
    /// triangles are inserted as Steiner points to improve the triangulation regularity of
    /// polygons.
    fn remove_null_triangles(&mut self, a_start: VertexId) -> VertexId {
        let mut a_start = a_start;
        let mut retval = self.simplify_list(a_start);

        if retval != NIL {
            a_start = retval;
        }

        let mut p = self.nx(a_start);

        while p != a_start && self.nx(p) != NIL && self.pv(p) != NIL {
            // We make a dummy triangle that is actually part of the existing line segment
            // and measure its area.  This will not be exactly zero due to floating point
            // errors.  We then look for areas that are less than 4 times the area of the
            // dummy triangle.  For small triangles, this is a small number
            let null_area = self.null_area(p);

            if self.vs.eq(p, self.nx(p)) || self.area(self.pv(p), p, self.nx(p)).abs() <= null_area
            {
                // This is a spike, remove it, leaving only one point
                if self.vs.eq(self.nx(p), self.pv(p)) {
                    let pn = self.nx(p);
                    self.vs.remove(pn);
                }

                p = self.pv(p);
                let pn = self.nx(p);
                self.vs.remove(pn);
                retval = p;

                if p == self.nx(p) {
                    break;
                }

                // aStart was removed above, so we need to reset it
                if self.nx(a_start) == NIL {
                    a_start = self.pv(p);
                }

                continue;
            }

            p = self.nx(p);
        }

        // We've removed all possible triangles
        if self.nx(p) == NIL || self.nx(p) == p || self.nx(p) == self.pv(p) {
            return p;
        }

        // We needed an end point above that wouldn't be removed, so
        // here we do the final check for this as a Steiner point
        let null_area = self.null_area(p);

        if self.area(self.pv(p), p, self.nx(p)).abs() <= null_area {
            retval = self.nx(p);
            self.vs.remove(p);
        }

        retval
    }

    /// Port of `earcutList()`: walk through the circular linked list starting at `a_point`
    /// and clip the ears (see `earcut_ring`).
    fn earcut_list(&mut self, a_point: VertexId, pass: i32) -> bool {
        let retval = self.earcut_ring(a_point, pass);

        // Other isEar() callers must not scan an index built for a ring they do not walk
        self.vs.reflex_index_valid = false;
        retval
    }

    /// Port of `earcutRing()`.
    ///
    /// For each point, test to see if the adjacent points form a triangle that is completely
    /// enclosed by the remaining polygon (an "ear" sticking off the polygon).  If the three
    /// points form an ear, we log the ear's location and remove the center point from the
    /// linked list.
    ///
    /// This function can be called recursively in the case of difficult polygons.  In cases
    /// where there is an intersection (not technically allowed by KiCad, but could exist in
    /// an edited file), we create a single triangle and remove both vertices before
    /// attempting to.
    fn earcut_ring(&mut self, a_point: VertexId, pass: i32) -> bool {
        const K_MAX_RECURSION: i32 = 64;

        if pass >= K_MAX_RECURSION {
            // "earcutList recursion limit reached; aborting triangulation"
            return false;
        }

        if a_point == NIL {
            return true;
        }

        let mut a_point = a_point;
        let mut stop = a_point;
        let mut internal_pass = 1;
        const K_EAR_LOOKAHEAD: i32 = 2;

        // Below this size, building the index costs more than the ear tests it saves
        const K_REFLEX_INDEX_MIN_VERTICES: usize = 8192;
        let mut ring_size: usize = 0;
        let mut walk = a_point;

        loop {
            ring_size += 1;
            walk = self.nx(walk);

            if !(walk != a_point && ring_size < K_REFLEX_INDEX_MIN_VERTICES) {
                break;
            }
        }

        let use_reflex_index = ring_size >= K_REFLEX_INDEX_MIN_VERTICES;

        while self.pv(a_point) != self.nx(a_point) {
            if use_reflex_index && !self.vs.reflex_index_valid {
                self.vs.build_reflex_index(a_point);
            }

            let prev = self.pv(a_point);
            let next = self.nx(a_point);

            let mut best_ear = NIL;
            let mut best_score = -1.0;
            let mut lookahead = 0;
            let mut candidate = a_point;

            while candidate != NIL && lookahead < K_EAR_LOOKAHEAD {
                if self.vs.is_ear(candidate, false) && !self.is_too_small(candidate) {
                    let score = self.ear_score(self.pv(candidate), candidate, self.nx(candidate));

                    if best_ear == NIL || score > best_score {
                        best_ear = candidate;
                        best_score = score;
                    }
                }

                candidate = self.nx(candidate);
                lookahead += 1;
            }

            if best_ear != NIL {
                let prev = self.pv(best_ear);
                let next = self.nx(best_ear);
                self.result
                    .add_triangle(self.vi(prev), self.vi(best_ear), self.vi(next));
                self.vs.remove(best_ear);

                // A degenerate ear can leave a neighbor reflex that the index never recorded
                if self.is_unindexed_reflex(prev) || self.is_unindexed_reflex(next) {
                    self.vs.reflex_index_valid = false;
                }

                // Skip one vertex as the triangle will account for the prev node
                a_point = self.nx(next);
                stop = self.nx(next);
                continue;
            }

            let next_next = self.nx(next);

            if !self.vs.eq(prev, next_next)
                && self.intersects(prev, a_point, next, next_next)
                && self.vs.locally_inside(prev, next_next)
                && self.vs.locally_inside(next_next, prev)
            {
                // "Local intersection detected.  Merging minor triangle"
                self.result
                    .add_triangle(self.vi(prev), self.vi(a_point), self.vi(next_next));

                // remove two nodes involved
                self.vs.remove(next);
                self.vs.remove(a_point);
                self.vs.reflex_index_valid = false;

                a_point = next_next;
                stop = next_next;

                continue;
            }

            a_point = next;

            // We've searched the entire polygon for available ears and there are still
            // un-sliced nodes remaining.
            if a_point == stop && self.pv(a_point) != self.nx(a_point) {
                // Every recovery step below edits the ring
                self.vs.reflex_index_valid = false;

                // Removing null triangles will remove steiner points as well as colinear points
                // that are three in a row.  Because our next step is to subdivide the polygon,
                // we need to allow it to add the subdivided points first.  This is why we only
                // run the RemoveNullTriangles function after the first pass.
                if internal_pass == 2 {
                    let new_point = self.remove_null_triangles(a_point);

                    if new_point != NIL {
                        // There are no remaining triangles in the list
                        if self.nx(new_point) == self.pv(new_point) {
                            break;
                        }

                        a_point = new_point;
                        stop = new_point;
                        continue;
                    }
                }

                internal_pass += 1;

                // This will subdivide the polygon 2 times.  The first pass will add enough
                // points such that each edge is less than the average edge length.  If this
                // doesn't work The next pass will remove the null triangles (above) and
                // subdivide the polygon again, this time adding one point to each long edge
                // (and thereby changing the locations)
                if internal_pass < 4 {
                    self.subdivide_polygon(a_point, internal_pass);
                    continue;
                }

                // If we don't have any NULL triangles left, cut the polygon in two and try
                // again
                if !self.split_polygon(a_point, pass + 1) {
                    return false;
                }

                break;
            }
        }

        // Check to see if we are left with only three points in the polygon
        if self.nx(a_point) != NIL && self.pv(a_point) == self.nx(self.nx(a_point)) {
            // Three concave points will never be able to be triangulated because they were
            // created by an intersecting polygon, so just drop them.
            if self.area(self.pv(a_point), a_point, self.nx(a_point)) >= 0.0 {
                return true;
            }
        }

        // At this point, our polygon should be fully tessellated.
        if self.pv(a_point) != self.nx(a_point) {
            return self.vs.ring_area(a_point, NIL).abs() > TRIANGULATE_MINIMUM_AREA as f64;
        }

        true
    }

    /// Port of `isUnindexedReflex()`.
    fn is_unindexed_reflex(&self, a_point: VertexId) -> bool {
        !self.vs.v(a_point).in_reflex_index
            && self.area(self.pv(a_point), a_point, self.nx(a_point)) >= 0.0
    }

    /// Port of `isTooSmall()`: whether a given vertex is too small to matter.
    fn is_too_small(&self, a_point: VertexId) -> bool {
        let min_area = TRIANGULATE_MINIMUM_AREA as f64;
        let (p, pp, pn) = (
            self.vs.v(a_point),
            self.vs.v(self.pv(a_point)),
            self.vs.v(self.nx(a_point)),
        );
        let prev_sq_len = (pp.x - p.x) * (pp.x - p.x) + (pp.y - p.y) * (pp.y - p.y);
        let next_sq_len = (pn.x - p.x) * (pn.x - p.x) + (pn.y - p.y) * (pn.y - p.y);
        let opp_sq_len = (pn.x - pp.x) * (pn.x - pp.x) + (pn.y - pp.y) * (pn.y - pp.y);

        prev_sq_len < min_area || next_sq_len < min_area || opp_sq_len < min_area
    }

    /// Port of `earScore()`.
    fn ear_score(&self, a: VertexId, b: VertexId, c: VertexId) -> f64 {
        let (av, bv, cv) = (self.vs.v(a), self.vs.v(b), self.vs.v(c));
        let ab_sq = (av.x - bv.x) * (av.x - bv.x) + (av.y - bv.y) * (av.y - bv.y);
        let bc_sq = (bv.x - cv.x) * (bv.x - cv.x) + (bv.y - cv.y) * (bv.y - cv.y);
        let ca_sq = (cv.x - av.x) * (cv.x - av.x) + (cv.y - av.y) * (cv.y - av.y);
        let norm = ab_sq + bc_sq + ca_sq;

        if norm <= 0.0 {
            return 0.0;
        }

        self.area(a, b, c).abs() / norm
    }

    /// Port of `createRing()`: a vertex ring from a line chain with a global index offset;
    /// the winding direction is controlled by `a_want_ccw`.
    fn create_ring(
        &mut self,
        a_points: &ShapeLineChain,
        a_base_index: i32,
        a_want_ccw: bool,
    ) -> VertexId {
        let mut tail = NIL;
        let mut sum = 0.0;
        let mut last_pt = Vector2I::new(0, 0);
        let mut first = true;
        let n = a_points.point_count();

        for i in 0..n {
            let p1 = a_points.c_point(i);
            let p2 = a_points.c_point((i + 1) % n);
            sum += (p2.x as f64 - p1.x as f64) * (p2.y as f64 + p1.y as f64);
        }

        let is_cw = sum > 0.0;
        let need_reverse = a_want_ccw == is_cw;

        let mut add_vertex = |vs: &mut VertexSet, i: i32| {
            let pt = a_points.c_point(i);

            if first || pt.squared_distance(last_pt) > vs.simplification_level {
                tail = vs.insert_vertex(a_base_index + i, pt, tail, 0);
                last_pt = pt;
                first = false;
            }
        };

        if need_reverse {
            for i in (0..n).rev() {
                add_vertex(&mut self.vs, i);
            }
        } else {
            for i in 0..n {
                add_vertex(&mut self.vs, i);
            }
        }

        // Collapse a final duplicate, but never on a single-vertex ring.  When the
        // simplification pass leaves only one vertex, tail->next == tail and removing
        // it would leave the caller holding a vertex with null next/prev pointers.
        if tail != NIL && self.nx(tail) != tail && self.vs.eq(tail, self.nx(tail)) {
            let n = self.nx(tail);
            self.vs.remove(n);
        }

        tail
    }

    /// Port of `eliminateHoles()`: bridge all hole rings into the outer ring by sorting holes
    /// left-to-right and connecting each hole's leftmost vertex to the nearest visible point
    /// on the outer boundary via `VERTEX::split()`.
    fn eliminate_holes(&mut self, a_outer_ring: VertexId, a_hole_rings: &[VertexId]) -> VertexId {
        let mut a_outer_ring = a_outer_ring;

        // HoleInfo: ( leftmost, leftX )
        let mut holes: Vec<(VertexId, f64)> = Vec::with_capacity(a_hole_rings.len());

        'holes: for &hole in a_hole_rings {
            let mut leftmost = hole;
            let mut p = self.nx(hole);

            if p == NIL {
                // wxFAIL
                break 'holes;
            }

            while p != hole {
                if self.vx(p) < self.vx(leftmost)
                    || (self.vx(p) == self.vx(leftmost) && self.vy(p) < self.vy(leftmost))
                {
                    leftmost = p;
                }

                p = self.nx(p);

                if p == NIL {
                    // wxFAIL
                    break;
                }
            }

            holes.push((leftmost, self.vx(leftmost)));
        }

        std_sort(&mut holes, |a, b| a.1 < b.1);

        for &(leftmost, _) in &holes {
            let bridge = self.find_hole_bridge(leftmost, a_outer_ring);

            if bridge != NIL {
                let bridge_reverse = self.vs.split(bridge, leftmost);
                let n = self.nx(bridge_reverse);
                self.filter_points(bridge_reverse, n);
                let n = self.nx(bridge);
                a_outer_ring = self.filter_points(bridge, n);
            }
            // else: "Failed to find bridge for hole"
        }

        a_outer_ring
    }

    /// Port of `filterPoints()`: remove consecutive duplicate vertices from the linked list.
    fn filter_points(&mut self, a_start: VertexId, a_end: VertexId) -> VertexId {
        if a_start == NIL {
            return a_start;
        }

        let mut a_end = if a_end == NIL { a_start } else { a_end };

        let mut p = a_start;

        loop {
            let mut again = false;

            if self.vs.eq(p, self.nx(p)) {
                let to_remove = self.nx(p);

                if to_remove == a_end {
                    a_end = p;
                }

                self.vs.remove(to_remove);

                if p == self.nx(p) {
                    return p;
                }

                p = self.pv(p);
                again = true;
            } else {
                p = self.nx(p);
            }

            if !(again || p != a_end) {
                break;
            }
        }

        a_end
    }

    /// Port of `findHoleBridge()`: a vertex on the outer ring visible from the hole's leftmost
    /// vertex, found by casting a horizontal ray to the left.
    fn find_hole_bridge(&self, a_hole: VertexId, a_outer_start: VertexId) -> VertexId {
        let mut p = a_outer_start;
        let hx = self.vx(a_hole);
        let hy = self.vy(a_hole);
        let mut qx = f64::NEG_INFINITY;
        let mut m = NIL;

        loop {
            let (px, py) = (self.vx(p), self.vy(p));
            let pn = self.nx(p);
            let (nx, ny) = (self.vx(pn), self.vy(pn));

            if hy <= py && hy >= ny && ny != py {
                let x = px + (hy - py) * (nx - px) / (ny - py);

                if x <= hx && x > qx {
                    qx = x;

                    if x == hx {
                        if hy == py {
                            return p;
                        }

                        if hy == ny {
                            return pn;
                        }
                    }

                    m = if px < nx { p } else { pn };
                }
            }

            p = pn;

            if p == a_outer_start {
                break;
            }
        }

        if m == NIL {
            return NIL;
        }

        if hx == qx {
            return m;
        }

        // Pick the vertex inside the visibility triangle closest to the ray
        let stop = m;
        let mx = self.vx(m);
        let my = self.vy(m);
        let mut tan_min = f64::INFINITY;

        p = m;

        loop {
            let (px, py) = (self.vx(p), self.vy(p));

            if hx >= px && px >= mx && hx != px {
                let inside = if hy < my {
                    tri_area(hx, hy, mx, my, px, py) >= 0.0
                        && tri_area(mx, my, qx, hy, px, py) >= 0.0
                        && tri_area(qx, hy, hx, hy, px, py) >= 0.0
                } else {
                    tri_area(qx, hy, mx, my, px, py) >= 0.0
                        && tri_area(mx, my, hx, hy, px, py) >= 0.0
                        && tri_area(hx, hy, qx, hy, px, py) >= 0.0
                };

                if inside {
                    let t = (hy - py).abs() / (hx - px);

                    if self.vs.locally_inside(p, a_hole)
                        && (t < tan_min
                            || (t == tan_min
                                && (px > self.vx(m)
                                    || (px == self.vx(m) && self.sector_contains_sector(m, p)))))
                    {
                        m = p;
                        tan_min = t;
                    }
                }
            }

            p = self.nx(p);

            if p == stop {
                break;
            }
        }

        m
    }

    /// Port of `sectorContainsSector()`: whether the sector in vertex `m` contains the sector
    /// in vertex `p` in the same coordinate frame.
    fn sector_contains_sector(&self, m: VertexId, p: VertexId) -> bool {
        self.area(self.pv(m), m, self.pv(p)) < 0.0 && self.area(self.nx(p), m, self.nx(m)) < 0.0
    }

    /// Port of `subdividePolygon()`: insert new vertices on the long edges.
    fn subdivide_polygon(&mut self, a_start: VertexId, pass: i32) {
        let mut p = a_start;

        // std::set< std::pair<VERTEX*, double> > ordered by decreasing length; an edge whose
        // length equals one already in the set is not inserted
        let mut longest: Vec<(VertexId, f64)> = Vec::new();
        let mut avg = 0.0;

        loop {
            let pn = self.nx(p);
            let len = (self.vx(p) - self.vx(pn)) * (self.vx(p) - self.vx(pn))
                + (self.vy(p) - self.vy(pn)) * (self.vy(p) - self.vy(pn));
            let pos = longest.partition_point(|e| e.1 > len);

            if pos == longest.len() || longest[pos].1 < len {
                longest.insert(pos, (p, len));
            }

            avg += len;
            p = pn;

            if p == a_start {
                break;
            }
        }

        avg /= longest.len() as f64;

        const K_SUBDIVIDE_THRESHOLD_FACTOR: f64 = 1.1;
        let subdivide_threshold = avg * K_SUBDIVIDE_THRESHOLD_FACTOR;

        for &(a, len) in &longest {
            if len <= subdivide_threshold {
                break;
            }

            let b = self.nx(a);
            let mut last = a;

            // We adjust the number of divisions based on the pass in order to progressively
            // subdivide the polygon when triangulation fails
            let divisions = cmath::f64_to_i32_x86(avg / len + 2.0 + pass as f64);
            let step = 1.0 / divisions as f64;

            for i in 1..divisions {
                let x = self.vx(a) * (1.0 - step * i as f64) + self.vx(b) * (step * i as f64);
                let y = self.vy(a) * (1.0 - step * i as f64) + self.vy(b) * (step * i as f64);
                last = self.insert_tri_vertex(
                    Vector2I::new(cmath::f64_to_i32_x86(x), cmath::f64_to_i32_x86(y)),
                    last,
                );
            }
        }

        // update z-order of the vertices
        self.vs.update_list(a_start);
    }

    /// Port of `splitPolygon()`: if we cannot find an ear to slice in the current polygon
    /// list, we use this to split the polygon into two separate lists and slice them each
    /// independently.  This is assured to generate at least one new ear if the split is
    /// successful.
    fn split_polygon(&mut self, start: VertexId, a_pass: i32) -> bool {
        let mut orig_poly = start;

        // If we have fewer than 4 points, we cannot split the polygon
        if start == NIL
            || self.nx(start) == NIL
            || self.nx(start) == self.pv(start)
            || self.nx(self.nx(start)) == self.pv(start)
        {
            return true;
        }

        // Our first attempts to split the polygon will be at overlapping points.
        // These are natural split points and we only need to switch the loop directions
        // to generate two new loops.  Since they are overlapping, we are do not
        // need to create a new segment to disconnect the two loops.
        loop {
            let mut overlap_points: Vec<VertexId> = Vec::new();
            let mut z_pt = orig_poly;

            while self.vs.v(z_pt).prev_z != NIL && self.vs.eq(self.vs.v(z_pt).prev_z, orig_poly) {
                z_pt = self.vs.v(z_pt).prev_z;
            }

            overlap_points.push(z_pt);

            while self.vs.v(z_pt).next_z != NIL && self.vs.eq(self.vs.v(z_pt).next_z, orig_poly) {
                z_pt = self.vs.v(z_pt).next_z;
                overlap_points.push(z_pt);
            }

            let usable = overlap_points.len() == 2
                && self.nx(overlap_points[0]) != overlap_points[1]
                && self.pv(overlap_points[0]) != overlap_points[1];

            if usable
                && (self.vs.ring_area(overlap_points[0], overlap_points[1]) < 0.0
                    || self.vs.ring_area(overlap_points[1], overlap_points[0]) < 0.0)
            {
                // "Split generated a hole, skipping"
                orig_poly = self.nx(orig_poly);
            } else if usable {
                let (o0, o1) = (overlap_points[0], overlap_points[1]);
                let n0 = self.nx(o0);
                let n1 = self.nx(o1);
                self.vs.vertices[o0].next = n1;
                self.vs.vertices[o1].next = n0;
                let n0 = self.nx(o0);
                self.vs.vertices[n0].prev = o0;
                let n1 = self.nx(o1);
                self.vs.vertices[n1].prev = o1;

                self.vs.update_list(o0);
                self.vs.update_list(o1);

                return self.earcut_list(o0, a_pass) && self.earcut_list(o1, a_pass);
            } else {
                orig_poly = self.nx(orig_poly);
            }

            if orig_poly == start {
                break;
            }
        }

        // If we've made it through the split algorithm and we still haven't found a
        // set of overlapping points, we need to create a new segment to split the polygon
        // into two separate polygons.  We do this by finding the two vertices that form
        // a valid line (does not cross the existing polygon)
        loop {
            let mut marker = self.nx(self.nx(orig_poly));

            while marker != self.pv(orig_poly) {
                // Find a diagonal line that is wholly enclosed by the polygon interior
                if self.nx(orig_poly) != NIL
                    && self.vi(orig_poly) != self.vi(marker)
                    && self.good_split(orig_poly, marker)
                {
                    let new_poly = self.vs.split(orig_poly, marker);

                    self.vs.update_list(orig_poly);
                    self.vs.update_list(new_poly);

                    return self.earcut_list(orig_poly, a_pass)
                        && self.earcut_list(new_poly, a_pass);
                }

                marker = self.nx(marker);
            }

            orig_poly = self.nx(orig_poly);

            if orig_poly == start {
                break;
            }
        }

        // "Could not find a valid split point"
        self.clip_remaining_ears(start)
    }

    /// Port of `clipRemainingEars()`: last resort for a ring that can be neither ear clipped
    /// nor split, typically a hatch sliver whose ears were all rejected by `isTooSmall()`.
    /// Those ears are still valid, and clipping them beats failing the whole polygon over a
    /// few square nanometres.
    fn clip_remaining_ears(&mut self, a_start: VertexId) -> bool {
        let mut p = a_start;
        let mut stop = a_start;

        while self.pv(p) != self.nx(p) {
            if self.vs.is_ear(p, false) {
                let next = self.nx(p);

                self.result
                    .add_triangle(self.vi(self.pv(p)), self.vi(p), self.vi(next));
                self.vs.remove(p);
                p = next;
                stop = next;
                continue;
            }

            p = self.nx(p);

            if p == stop {
                return false;
            }
        }

        true
    }

    /// Port of `goodSplit()`: whether a segment joining two vertices lies fully inside the
    /// polygon.  To do this, we first ensure that the line isn't along the polygon edge.
    /// Next, we know that if the line doesn't intersect the polygon, then it is either fully
    /// inside or fully outside the polygon.  Next, we ensure that the proposed split is inside
    /// the local area of the polygon at both ends and the midpoint. Finally, we check to split
    /// creates two new polygons, each with positive area.
    fn good_split(&self, a: VertexId, b: VertexId) -> bool {
        let vs = &self.vs;
        let (av, bv) = (vs.v(a), vs.v(b));
        let a_on_edge =
            (av.next_z != NIL && vs.eq(a, av.next_z)) || (av.prev_z != NIL && vs.eq(a, av.prev_z));
        let b_on_edge =
            (bv.next_z != NIL && vs.eq(b, bv.next_z)) || (bv.prev_z != NIL && vs.eq(b, bv.prev_z));
        let no_intersect =
            self.vi(av.next) != bv.i && self.vi(av.prev) != bv.i && !self.intersects_polygon(a, b);
        let local_split =
            vs.locally_inside(a, b) && vs.locally_inside(b, a) && vs.middle_inside(a, b);
        let same_dir = self.area(av.prev, a, bv.prev) != 0.0 || self.area(a, bv.prev, b) != 0.0;
        let has_len = vs.eq(a, b)
            && self.area(av.prev, a, av.next) > 0.0
            && self.area(bv.prev, b, bv.next) > 0.0;
        let pos_area = vs.ring_area(a, b) > 0.0 && vs.ring_area(b, a) > 0.0;

        no_intersect && local_split && (same_dir || has_len) && !a_on_edge && !b_on_edge && pos_area
    }

    /// Port of `overlapping()`: if `p`, `q` and `r` are collinear and `r` lies between `p` and
    /// `q`, then return true.
    fn overlapping(&self, p: VertexId, q: VertexId, r: VertexId) -> bool {
        let (p, q, r) = (self.vs.v(p), self.vs.v(q), self.vs.v(r));
        q.x <= std_max(p.x, r.x)
            && q.x >= std_min(p.x, r.x)
            && q.y <= std_max(p.y, r.y)
            && q.y >= std_min(p.y, r.y)
    }

    /// Port of `intersects()`: whether `p1`-`q1` intersects `p2`-`q2`, end points included.
    fn intersects(&self, p1: VertexId, q1: VertexId, p2: VertexId, q2: VertexId) -> bool {
        let sign1 = sign(self.area(p1, q1, p2));
        let sign2 = sign(self.area(p1, q1, q2));
        let sign3 = sign(self.area(p2, q2, p1));
        let sign4 = sign(self.area(p2, q2, q1));

        if sign1 != sign2 && sign3 != sign4 {
            return true;
        }

        if sign1 == 0 && self.overlapping(p1, p2, q1) {
            return true;
        }

        if sign2 == 0 && self.overlapping(p1, q2, q1) {
            return true;
        }

        if sign3 == 0 && self.overlapping(p2, p1, q2) {
            return true;
        }

        if sign4 == 0 && self.overlapping(p2, q1, q2) {
            return true;
        }

        false
    }

    /// Port of `intersectsPolygon()`: whether the segment from vertex `a` to vertex `b`
    /// crosses any of the segments of the polygon of which vertex `a` is a member (the
    /// vertices created before the triangulation started, in creation order).
    fn intersects_polygon(&self, a: VertexId, b: VertexId) -> bool {
        let (ai, bi) = (self.vi(a), self.vi(b));
        let n = self.vertices_original_size;

        for ii in 0..n {
            let p = ii;
            let q = (ii + 1) % n;
            let (pi, qi) = (self.vi(p), self.vi(q));

            if pi == ai || pi == bi || qi == ai || qi == bi {
                continue;
            }

            if self.intersects(p, q, a, b) {
                return true;
            }
        }

        false
    }

    /// Port of `insertTriVertex()`: add a vertex to the result and to the vertex list (after
    /// `last`).
    fn insert_tri_vertex(&mut self, pt: Vector2I, last: VertexId) -> VertexId {
        self.result.add_vertex(pt);
        let index = self.result.vertex_count() as i32 - 1;
        self.vs.insert_vertex(index, pt, last, 0)
    }
}

/// Port of `POLYGON_TRIANGULATION::triArea()`: signed area of the triangle (`ax`, `ay`),
/// (`bx`, `by`), (`cx`, `cy`).
#[inline]
fn tri_area(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Port of `POLYGON_TRIANGULATION::sign()`.
#[inline]
fn sign(a_val: f64) -> i32 {
    (a_val > 0.0) as i32 - (a_val < 0.0) as i32
}

/// `static_cast<int>( std::lround( x ) )`: round half away from zero to `long`, then
/// narrowing to `int` (glibc / x86-64 behaviour out of range).
#[inline]
fn lround_to_int(x: f64) -> i32 {
    cmath::f64_to_i64_x86(x.round()) as i32
}
