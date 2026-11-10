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

//! Port of the helper classes for `connect_nearby_polys` of `pcbnew/zone_filler.cpp`
//! (`RESULTS`, `VERTEX_CONNECTOR`) and of `ZONE_FILLER::connect_nearby_polys`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use kicad_geom::geometry::vertex_set::{NIL, VertexId, VertexSet};
use kicad_geom::{Box2I, ShapePolySet, Vector2D, Vector2I};

/// `ADVANCED_CFG::m_TriangulateSimplificationLevel` (default 50).
const TRIANGULATE_SIMPLIFICATION_LEVEL: i32 = 50;

/// Port of `RESULTS`: a bridge from vertex `vertex1` of outline `outline1` to vertex
/// `vertex2` of outline `outline2`. The derived order is the C++ `operator<` (outline1,
/// outline2, vertex1, vertex2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Results {
    pub outline1: i32,
    pub outline2: i32,
    pub vertex1: i32,
    pub vertex2: i32,
}

/// Port of `VERTEX_CONNECTOR : protected VERTEX_SET`.
struct VertexConnector {
    set: VertexSet,
    results: BTreeSet<Results>,
    outline_distances: Vec<Vec<f64>>,
    dist: i32,
}

impl VertexConnector {
    /// Port of `VERTEX_CONNECTOR( aBBox, aPolys, aDist )`.
    fn new(bbox: &Box2I, polys: &ShapePolySet, dist: i32) -> Self {
        let mut set = VertexSet::new(TRIANGULATE_SIMPLIFICATION_LEVEL);
        let mut outline_distances: Vec<Vec<f64>> = Vec::new();

        set.set_bounding_box(bbox);
        let mut tail = NIL;

        for i in 0..polys.outline_count() {
            let outline = polys.c_outline(i);
            let mut distances: Vec<f64> = Vec::with_capacity(outline.point_count() as usize + 1);

            distances.push(0.0);

            for j in 0..outline.point_count() {
                let last = *distances.last().expect("non-empty");
                distances.push(
                    last + (outline.c_point(j + 1) - outline.c_point(j)).euclidean_norm() as f64,
                );
            }

            outline_distances.push(distances);
            tail = set.create_list(outline, tail, i as usize);
        }

        if tail != NIL {
            set.update_list(tail);
            // The list does not change from here on. Only reflex vertices can block an ear, and
            // whether one does is all `is_ear` answers.
            set.build_reflex_index(0);
        }

        VertexConnector {
            set,
            results: BTreeSet::new(),
            outline_distances,
            dist,
        }
    }

    /// Port of `VERTEX_CONNECTOR::getPoint( aPt )`: the nearest ear vertex within `m_dist`
    /// of `pt` in z-order range that is not a nearby point along the same contour.
    fn get_point(&mut self, pt: VertexId) -> VertexId {
        let (px, py) = (self.set.v(pt).x, self.set.v(pt).y);
        let dist = self.dist as f64;

        // z-order range for the current point ± limit bounding box
        let max_z = self.set.z_order(px + dist, py + dist);
        let min_z = self.set.z_order(px - dist, py - dist);
        let limit2: i64 = self.dist as i64 * self.dist as i64;

        // first look for points in increasing z-order
        let mut min_dist = i64::MAX;
        let mut retval = NIL;

        let mut check_pt = |this: &mut VertexConnector, p: VertexId| {
            let pt_user = this.set.v(pt).user_data;
            let p_user = this.set.v(p).user_data;

            // A nearby point along the same contour is already connected and would consume
            // the visited-point suppression before a contour-distant point across a neck is
            // considered.
            if p_user == pt_user {
                let distances = &this.outline_distances[p_user];
                let direct_distance = (distances[this.set.v(p).i as usize]
                    - distances[this.set.v(pt).i as usize])
                    .abs();
                let contour_distance =
                    direct_distance.min(distances[distances.len() - 1] - direct_distance);

                if contour_distance < this.dist as f64 {
                    return;
                }
            }

            let diff = Vector2D::new(this.set.v(p).x - px, this.set.v(p).y - py);
            // SEG::ecoord dist2 = diff.SquaredEuclideanNorm() (double -> int64 conversion)
            let dist2 = diff.squared_euclidean_norm() as i64;

            if dist2 > 0 && dist2 < limit2 && dist2 < min_dist && this.set.is_ear(p, true) {
                min_dist = dist2;
                retval = p;
            }
        };

        let mut p = self.set.v(pt).next_z;

        while p != NIL && self.set.v(p).z <= max_z {
            check_pt(self, p);
            p = self.set.v(p).next_z;
        }

        p = self.set.v(pt).prev_z;

        while p != NIL && self.set.v(p).z >= min_z {
            check_pt(self, p);
            p = self.set.v(p).prev_z;
        }

        retval
    }

    /// Port of `VERTEX_CONNECTOR::FindResults()`.
    fn find_results(&mut self) {
        if self.set.vertices.is_empty() {
            return;
        }

        let front: VertexId = 0;
        let mut p = self.set.next(front);
        let mut visited: HashSet<VertexId> = HashSet::new();

        while p != front {
            // Skip points that are concave
            if !self.set.is_ear(p, false) {
                p = self.set.next(p);
                continue;
            }

            if visited.is_empty() || !visited.contains(&p) {
                let q = self.get_point(p);

                if q != NIL {
                    visited.insert(p);

                    if !visited.contains(&q) {
                        let result = Results {
                            outline1: self.set.v(p).user_data as i32,
                            outline2: self.set.v(q).user_data as i32,
                            vertex1: self.set.v(p).i,
                            vertex2: self.set.v(q).i,
                        };

                        if self.results.insert(result) {
                            // We don't want to connect multiple points in the same vicinity,
                            // so skip 2 points before and after each point and match.
                            let s = &self.set;

                            visited.insert(s.prev(p));
                            visited.insert(s.prev(s.prev(p)));
                            visited.insert(s.next(p));
                            visited.insert(s.next(s.next(p)));

                            visited.insert(s.prev(q));
                            visited.insert(s.prev(s.prev(q)));
                            visited.insert(s.next(q));
                            visited.insert(s.next(s.next(q)));

                            visited.insert(q);
                        }
                    }
                }
            }

            p = self.set.next(p);
        }
    }
}

/// Port of `ZONE_FILLER::connect_nearby_polys( aPolys, aDistance )`: connect nearby
/// polygons with zero-width lines (spikes `v1, pt2, pt1`) in order to ensure correct
/// re-inflation.
pub fn connect_nearby_polys(polys: &mut ShapePolySet, distance: f64) {
    if polys.outline_count() < 1 {
        return;
    }

    // VERTEX_CONNECTOR( ..., int aDist ): the double converts to int.
    let mut vs = VertexConnector::new(&polys.bbox_from_caches(), polys, distance as i32);

    vs.find_results();

    // This cannot be a reference because we need to do the comparison below while
    // changing the values
    let mut insertion_points: BTreeMap<i32, Vec<(i32, Vector2I)>> = BTreeMap::new();

    for result in &vs.results {
        let pt1 = polys.c_outline(result.outline1).c_point(result.vertex1);
        let pt2 = polys.c_outline(result.outline2).c_point(result.vertex2);

        // We want to insert the existing point first so that we can place the new point
        // between the two points at the same location.
        let entry = insertion_points.entry(result.outline1).or_default();
        entry.push((result.vertex1, pt1));
        entry.push((result.vertex1, pt2));
    }

    for (outline, vertices) in insertion_points.iter_mut() {
        // Stable sort here because we want to make sure that we are inserting pt1 first and
        // pt2 second but still sorting the rest of the indices from highest to lowest.
        // This allows us to insert into the existing polygon without modifying the future
        // insertion points.
        vertices.sort_by_key(|v| std::cmp::Reverse(v.0));

        let line = polys.outline(*outline);
        let positions: Vec<(usize, Vector2I)> = vertices
            .iter()
            .map(|&(vertex, pt)| ((vertex + 1) as usize, pt))
            .collect();

        if !line.insert_points_descending(&positions) {
            for &(vertex, pt) in &positions {
                line.insert_point(vertex, pt);
            }
        }
    }
}
