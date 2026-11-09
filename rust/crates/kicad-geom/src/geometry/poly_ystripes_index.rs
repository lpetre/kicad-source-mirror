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

//! Port of `libs/kimath/include/geometry/poly_ystripes_index.h` (`POLY_YSTRIPES_INDEX`).
//!
//! Y-stripe spatial index for efficient point-in-polygon containment testing, inspired by
//! the YStripes indexing strategy from the TG geometry library
//! (<https://github.com/tidwall/tg>, MIT license, Copyright (c) 2023 Joshua J Baker).

use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::util::rescale;
use crate::math::vector2::Vector2I;

/// Port of `POLY_YSTRIPES_INDEX::EDGE`.
#[derive(Clone, Copy, Debug)]
struct Edge {
    p1: Vector2I,
    p2: Vector2I,
    outline_idx: i32,
    is_hole: bool,
}

/// Port of `POLY_YSTRIPES_INDEX`.
///
/// Divides the polygon set's Y extent into uniform horizontal stripes and buckets each
/// non-horizontal edge into every stripe its Y range overlaps.  Containment queries ray-cast
/// only against edges in the queried point's stripe.  The ray-crossing algorithm matches
/// `SHAPE_LINE_CHAIN_BASE::PointInside()` exactly, including the accuracy semantics where
/// `a_accuracy > 1` falls back to edge-distance testing.
#[derive(Clone, Debug)]
pub struct PolyYStripesIndex {
    edges: Vec<Edge>,
    stripes: Vec<Vec<i32>>,
    y_min: i64,
    #[allow(dead_code)]
    y_max: i64,
    stripe_height: i64,
    stripe_count: i32,
    outline_count: i32,
}

impl Default for PolyYStripesIndex {
    fn default() -> Self {
        PolyYStripesIndex {
            edges: Vec::new(),
            stripes: Vec::new(),
            y_min: 0,
            y_max: 0,
            stripe_height: 1,
            stripe_count: 0,
            outline_count: 0,
        }
    }
}

impl PolyYStripesIndex {
    /// Port of `POLY_YSTRIPES_INDEX()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `POLY_YSTRIPES_INDEX::Build()`: build the index from a poly set's outlines and
    /// holes.
    ///
    /// Indexes every non-horizontal edge of every outline and hole in the polygon set.  Hole
    /// edges are indexed with the same outline index as their parent outline so that
    /// ray-crossing counts automatically produce even totals for points inside holes.
    pub fn build(&mut self, a_poly_set: &ShapePolySet) {
        self.outline_count = a_poly_set.outline_count();
        self.edges.clear();
        self.stripes.clear();

        let mut total_edges = 0;

        for outline_idx in 0..self.outline_count {
            total_edges += a_poly_set.c_outline(outline_idx).point_count();

            for hole_idx in 0..a_poly_set.hole_count(outline_idx) {
                total_edges += a_poly_set.c_hole(outline_idx, hole_idx).point_count();
            }
        }

        if total_edges == 0 {
            return;
        }

        self.edges.reserve(total_edges as usize);

        let mut y_min: i64 = i32::MAX as i64;
        let mut y_max: i64 = i32::MIN as i64;

        let mut add_chain = |edges: &mut Vec<Edge>, chain: &crate::geometry::shape_line_chain::ShapeLineChain, outline_idx: i32, is_hole: bool| {
            let pt_count = chain.point_count();

            if pt_count < 3 {
                return;
            }

            for j in 0..pt_count {
                let p1 = chain.c_point(j);
                let p2 = chain.c_point((j + 1) % pt_count);

                if p1.y == p2.y {
                    continue;
                }

                edges.push(Edge {
                    p1,
                    p2,
                    outline_idx,
                    is_hole,
                });

                y_min = y_min.min(p1.y.min(p2.y) as i64);
                y_max = y_max.max(p1.y.max(p2.y) as i64);
            }
        };

        for outline_idx in 0..self.outline_count {
            add_chain(&mut self.edges, a_poly_set.c_outline(outline_idx), outline_idx, false);

            for hole_idx in 0..a_poly_set.hole_count(outline_idx) {
                add_chain(&mut self.edges, a_poly_set.c_hole(outline_idx, hole_idx), outline_idx, true);
            }
        }

        if self.edges.is_empty() {
            return;
        }

        // static_cast<int>( std::sqrt( (double) size ) ): in range
        let mut stripe_count = (self.edges.len() as f64).sqrt() as i32;
        stripe_count = 1.max(stripe_count.min(65536));

        self.y_min = y_min;
        self.y_max = y_max;
        let y_range = y_max - y_min;

        // Guard against degenerate case where all edges share the same Y coordinate
        if y_range == 0 {
            stripe_count = 1;
            self.stripe_height = 1;
        } else {
            self.stripe_height = (y_range + stripe_count as i64 - 1) / stripe_count as i64;
        }

        self.stripe_count = stripe_count;
        self.stripes = vec![Vec::new(); stripe_count as usize];

        for i in 0..self.edges.len() {
            let e = self.edges[i];
            let e_y_min = (e.p1.y as i64).min(e.p2.y as i64);
            let e_y_max = (e.p1.y as i64).max(e.p2.y as i64);

            let s_min = self.y_to_stripe(e_y_min);
            let s_max = self.y_to_stripe(e_y_max);

            for s in s_min..=s_max {
                self.stripes[s as usize].push(i as i32);
            }
        }
    }

    /// Port of `POLY_YSTRIPES_INDEX::Contains()`: whether a point is inside the indexed
    /// polygon set (inside any outline or, when `a_accuracy > 1`, within `a_accuracy` of any
    /// outline edge).
    pub fn contains(&self, a_pt: Vector2I, a_accuracy: i32) -> bool {
        if self.edges.is_empty() {
            return false;
        }

        let stripe = self.y_to_stripe(a_pt.y as i64);

        if stripe >= 0 && stripe < self.stripe_count {
            let mut crossings = vec![0i32; (self.outline_count.max(8)) as usize];

            for &idx in &self.stripes[stripe as usize] {
                let seg = &self.edges[idx as usize];
                let p1 = seg.p1;
                let p2 = seg.p2;

                if (p1.y >= a_pt.y) == (p2.y >= a_pt.y) {
                    continue;
                }

                let diff = p2 - p1;
                let d: i32 = rescale(diff.x, a_pt.y - p1.y, diff.y);

                if a_pt.x - p1.x < d {
                    crossings[seg.outline_idx as usize] += 1;
                }
            }

            for &c in crossings.iter().take(self.outline_count as usize) {
                if c & 1 != 0 {
                    return true;
                }
            }
        }

        if a_accuracy > 1 {
            let s_min = self.y_to_stripe(a_pt.y as i64 - a_accuracy as i64);
            let s_max = self.y_to_stripe(a_pt.y as i64 + a_accuracy as i64);
            let accuracy_sq: Ecoord = Seg::square(a_accuracy);

            for s in s_min..=s_max {
                if s < 0 || s >= self.stripe_count {
                    continue;
                }

                for &idx in &self.stripes[s as usize] {
                    let seg = &self.edges[idx as usize];

                    if seg.is_hole {
                        continue;
                    }

                    let edge = Seg::new(seg.p1, seg.p2);

                    if edge.squared_distance(a_pt) <= accuracy_sq {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Port of `POLY_YSTRIPES_INDEX::yToStripe()`.
    fn y_to_stripe(&self, a_y: i64) -> i32 {
        let offset = a_y - self.y_min;
        // static_cast<int>( int64_t ): wrapping narrowing
        let s = (offset / self.stripe_height) as i32;
        0.max(s.min(self.stripe_count - 1))
    }
}
