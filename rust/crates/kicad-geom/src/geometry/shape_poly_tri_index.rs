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

//! Port of `libs/kimath/include/geometry/shape_poly_tri_index.h`: `TRIANGULATION_INDEX`, a
//! packed r-tree over the triangles of a cached triangulation, and the `AnyTriangleHit()`
//! collision helpers built on it.
//!
//! The C++ index stores raw `const TRI*` pointers into the triangulation it does not own; the
//! port stores `( polygon, triangle )` index pairs and every query takes the triangulation it
//! was built from.  Whoever owns the triangles must drop the index whenever they are rebuilt,
//! discarded or moved (as in C++).

use crate::geometry::rtree::packed_rtree::PackedRTree;
use crate::geometry::shape::Shape;
use crate::geometry::shape_poly_set::{Tri, TriangulatedPolygon};
use crate::math::box2::Box2I;
use crate::math::vector2::Vector2I;

/// Port of `TRIANGULATION_INDEX`: a spatial index over the triangles of a cached
/// triangulation, used to prune collision tests.  Visitation order is the r-tree's.
#[derive(Clone, Debug, Default)]
pub struct TriangulationIndex {
    /// `m_tree` (payload: index into `tris`)
    tree: PackedRTree<u32, 2>,
    /// `m_tris`: ( polygon index, triangle index ) of every indexed triangle
    tris: Vec<(u32, u32)>,
}

impl TriangulationIndex {
    /// Port of `TRIANGULATION_INDEX( polys )`: index every triangle of every polygon.
    pub fn new(a_polys: &[TriangulatedPolygon]) -> Self {
        let mut index = TriangulationIndex::default();

        let count: usize = a_polys.iter().map(|p| p.triangle_count()).sum();

        if count == 0 {
            return index;
        }

        index.tris.reserve(count);

        let mut builder = PackedRTree::<u32, 2>::builder();
        builder.reserve(count);

        for (pi, poly) in a_polys.iter().enumerate() {
            for (ti, tri) in poly.triangles.iter().enumerate() {
                let bbox = tri.bbox(0);
                let lo = [bbox.x(), bbox.y()];
                let hi = [bbox.right(), bbox.bottom()];

                builder.add(&lo, &hi, index.tris.len() as u32);
                index.tris.push((pi as u32, ti as u32));
            }
        }

        index.tree = builder.build();
        index
    }

    /// The number of indexed triangles.
    pub fn len(&self) -> usize {
        self.tris.len()
    }

    /// Whether no triangle is indexed.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Port of `TRIANGULATION_INDEX::Query()`: visit every triangle of `a_polys` (the
    /// triangulation the index was built from) whose bounding box overlaps `a_box`;
    /// `a_visitor` returns false to stop the search.
    pub fn query<'p, F>(&self, a_polys: &'p [TriangulatedPolygon], a_box: &Box2I, mut a_visitor: F)
    where
        F: FnMut(&'p Tri) -> bool,
    {
        if self.tris.is_empty() {
            return;
        }

        let lo = [a_box.x(), a_box.y()];
        let hi = [a_box.right(), a_box.bottom()];

        self.tree.search(&lo, &hi, |&a_index| {
            let (pi, ti) = self.tris[a_index as usize];
            a_visitor(&a_polys[pi as usize].triangles[ti as usize])
        });
    }
}

/// Port of `VisitTriangleCandidates()`: visit the triangles that could collide with something
/// inside `a_box`, through `a_index` when one exists, or by scanning every polygon otherwise.
/// `a_visitor` returns false to stop the walk.
///
/// Returns false if the visitor stopped the walk, true if it ran to completion.
pub fn visit_triangle_candidates<'p, F>(
    a_index: Option<&TriangulationIndex>,
    a_polys: &'p [TriangulatedPolygon],
    a_box: &Box2I,
    mut a_visitor: F,
) -> bool
where
    F: FnMut(&'p Tri) -> bool,
{
    if let Some(index) = a_index {
        let mut stopped = false;

        index.query(a_polys, a_box, |a_tri| {
            if a_visitor(a_tri) {
                return true;
            }

            stopped = true;
            false
        });

        return !stopped;
    }

    for poly in a_polys {
        for tri in &poly.triangles {
            if !a_visitor(tri) {
                return false;
            }
        }
    }

    true
}

/// Port of `AnyTriangleHit( index, polys, const SHAPE* aShape, int aClearance, int* aActual,
/// VECTOR2I* aLocation )`: whether any candidate triangle collides with `a_shape` within
/// `a_clearance`.
///
/// With `a_actual` or `a_location`, every candidate is visited and the least collision
/// distance and where it occurs are reported; without them the walk stops at the first hit.
pub fn any_triangle_hit(
    a_index: Option<&TriangulationIndex>,
    a_polys: &[TriangulatedPolygon],
    a_shape: &dyn Shape,
    a_clearance: i32,
    a_actual: Option<&mut i32>,
    a_location: Option<&mut Vector2I>,
) -> bool {
    // NOT BBox( aClearance ): SHAPE_COMPOUND::BBox() ignores its argument, and inflating
    // separately keeps near-miss triangles in the candidate set.
    let mut qbox = a_shape.bbox(0);

    if a_clearance > 0 {
        qbox.inflate(a_clearance);
    }

    if a_actual.is_none() && a_location.is_none() {
        return !visit_triangle_candidates(a_index, a_polys, &qbox, |a_tri| {
            !a_shape.collide(a_tri, a_clearance, None, None)
        });
    }

    let mut actual = i32::MAX;
    let mut location = Vector2I::default();

    visit_triangle_candidates(a_index, a_polys, &qbox, |a_tri| {
        let mut tri_actual = 0;
        let mut tri_location = Vector2I::default();

        if a_shape.collide(
            a_tri,
            a_clearance,
            Some(&mut tri_actual),
            Some(&mut tri_location),
        ) && tri_actual < actual
        {
            actual = tri_actual;
            location = tri_location;
        }

        true // Finding a minimum needs every candidate
    });

    if actual == i32::MAX {
        return false;
    }

    if let Some(a) = a_actual {
        *a = actual;
    }

    if let Some(l) = a_location {
        *l = location;
    }

    true
}

/// Port of `AnyTriangleHit( index, polys, const VECTOR2I& aPt )`: whether any candidate
/// triangle contains `a_pt`.
pub fn any_triangle_hit_point(
    a_index: Option<&TriangulationIndex>,
    a_polys: &[TriangulatedPolygon],
    a_pt: Vector2I,
) -> bool {
    !visit_triangle_candidates(
        a_index,
        a_polys,
        &Box2I::new(a_pt, Vector2I::new(0, 0)),
        |a_tri| !a_tri.collide_point(a_pt, 0, None, None),
    )
}
