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

//! Port of `libs/kimath/include/geometry/shape_poly_set.h` and
//! `libs/kimath/src/geometry/shape_poly_set.cpp` (plus `SHAPE_POLY_SET::chamferFilletPolygon`
//! from `src/geometry/corner_operations.cpp`): a set of polygons, each an outline followed by
//! its holes, with the Clipper2 bridge (booleans, offsets, `PolyTree` import), fracturing,
//! simplification, the vertex index machinery and iterators, distances and collisions.
//!
//! The triangulation (`TRIANGULATED_POLYGON` construction, `CacheTriangulation`,
//! `TRIANGULATED_POLYGON::Refine`, the triangle index and the triangle-based branch of
//! `Collide( SHAPE* )`) is work package G4b; the hooks are marked "G4b" below and listed in
//! `PORT_STATUS.md`.
//!
//! Overloads:
//!
//! | C++ | Rust |
//! |---|---|
//! | `Append( x, y, outline, hole, dup )` / `Append( VECTOR2I, outline, hole )` / `Append( SHAPE_POLY_SET )` / `Append( SHAPE_ARC, outline, hole, maxError )` | `append` / `append_point` / `append_set` / `append_arc` |
//! | `CVertex( index, outline, hole )` / `CVertex( globalIndex )` / `CVertex( VERTEX_INDEX )` | `c_vertex` / `c_vertex_global` / `c_vertex_index` |
//! | `RemoveVertex( int )` / `RemoveVertex( VERTEX_INDEX )`, `SetVertex( int, p )` / `SetVertex( VERTEX_INDEX, p )` | `remove_vertex` / `remove_vertex_index`, `set_vertex` / `set_vertex_index` |
//! | `BooleanAdd( b )` / `BooleanAdd( a, b )` (also `Subtract`, `Intersection`, `Xor`) | `boolean_add` / `boolean_add_sets` |
//! | `Iterate( first, last, holes )`, `Iterate( outline )`, `IterateWithHoles( outline )`, `Iterate()`, `IterateWithHoles()` (and the `CIterate` twins) | `iterate_range`, `iterate_outline`, `iterate_with_holes_outline`, `iterate`, `iterate_with_holes` (`c_iterate_*` are the same functions) |
//! | `IterateSegments( first, last, holes )`, `( polygon )`, `()`, `IterateSegmentsWithHoles()`, `( outline )` | `iterate_segments_range`, `iterate_segments_polygon`, `iterate_segments`, `iterate_segments_with_holes`, `iterate_segments_with_holes_outline` |
//! | `Collide( VECTOR2I )` / `Collide( SEG )` / `Collide( SHAPE* )` | `collide_point` / `collide_seg` / `collide` (trait `Shape`; also `collide_shape`) |
//! | `SquaredDistanceToPolygon( VECTOR2I )` / `( SEG )`, `SquaredDistance( p, outlineOnly, nearest )` | `squared_distance_to_polygon` / `squared_distance_to_polygon_seg`, `squared_distance_nearest` |
//! | `Mirror( ref, dir )` / `Mirror( SEG )` | `mirror` / `mirror_seg` |
//! | the copy constructor / `operator=` | `Clone` (copies the triangulation only while it is up to date, like the C++) |

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::rc::Rc;

use clipper2_rs as c2;

use crate::cmath;
use crate::geometry::approximation::ErrorLoc;
use crate::geometry::corner_strategy::CornerStrategy;
use crate::geometry::eda_angle::{EdaAngle, FULL_CIRCLE, RADIANS_T};
use crate::geometry::geometry_utils::get_arc_to_segment_count;
use crate::geometry::polygon_triangulation::{
    PolygonTriangulation, TRIANGULATE_SIMPLIFICATION_LEVEL,
};
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::segment_index::SegmentIndex;
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_line_chain::{
    ClipperPoint64, ClipperZValue, ShapeLineChain, StringStream,
};
use crate::geometry::shape_poly_set_fracture::{fracture_single_cache_friendly, split_at_bridges};
use crate::geometry::shape_poly_tri_index::{TriangulationIndex, any_triangle_hit};
use crate::hash_128::Hash128;
use crate::kicore::mirror::FlipDirection;
use crate::math::box2::{Box2D, Box2I};
use crate::math::util::{ki_round, ki_round_i64};
use crate::math::vector2::{Vector2D, Vector2I};
use crate::mmh3_hash::Mmh3Hash;
use crate::stdsort::std_sort;

/// `RTREE_THRESHOLD`: below this many elements a direct scan beats building and querying a
/// spatial index.
const RTREE_THRESHOLD: i32 = 32;

/// Port-only: outlines of at most this many points get the exact brute-force pre-check of
/// `split_collinear_outlines` (no `SEGMENT_INDEX` unless some pair is a candidate).
const SPLIT_COLLINEAR_PRECHECK_MAX: i64 = 64;

/// Port-only: extent of one outline's points, used to skip `SHAPE_LINE_CHAIN::PointInside()`
/// calls that provably return false (`ShapePolySet::point_inside_with_extents`).
#[derive(Clone, Copy, Debug)]
struct OutlineExtent {
    min: Vector2I,
    max: Vector2I,
    /// All points lie within +-`EXTENT_BOUND`: the ray-cast `int` arithmetic cannot wrap.
    in_bound: bool,
}

/// Bound for [`OutlineExtent::in_bound`]: differences of two such values (and `p1.x + d`)
/// fit an `i32`.
const EXTENT_BOUND: i32 = (1 << 30) - 1;

impl OutlineExtent {
    fn of(a_points: &[Vector2I]) -> Self {
        let mut ext = OutlineExtent {
            min: Vector2I::new(i32::MAX, i32::MAX),
            max: Vector2I::new(i32::MIN, i32::MIN),
            in_bound: true,
        };

        for p in a_points {
            ext.min.x = ext.min.x.min(p.x);
            ext.min.y = ext.min.y.min(p.y);
            ext.max.x = ext.max.x.max(p.x);
            ext.max.y = ext.max.y.max(p.y);
            ext.in_bound &= Self::in_bound(*p);
        }

        ext
    }

    fn in_bound(a_pt: Vector2I) -> bool {
        (-EXTENT_BOUND..=EXTENT_BOUND).contains(&a_pt.x)
            && (-EXTENT_BOUND..=EXTENT_BOUND).contains(&a_pt.y)
    }
}

/// Port of `SHAPE_POLY_SET::POLYGON`: the outline (index 0) followed by the holes.
pub type Polygon = Vec<ShapeLineChain>;

impl ClipperPoint64 for c2::Point64 {
    fn x(&self) -> i64 {
        self.x
    }

    fn y(&self) -> i64 {
        self.y
    }

    fn z(&self) -> i64 {
        self.z
    }
}

/// `SHAPE_LINE_CHAIN::convertToClipper2()` as a Clipper2 `Path64`.
fn to_path64(
    a_chain: &ShapeLineChain,
    a_required_orientation: bool,
    a_z_values: &mut Vec<ClipperZValue>,
    a_arc_buffer: &mut Vec<ShapeArc>,
) -> c2::Path64 {
    a_chain
        .convert_to_clipper2(a_required_orientation, a_z_values, a_arc_buffer)
        .into_iter()
        .map(|(x, y, z)| c2::Point64::new_z(x, y, z))
        .collect()
}

/// Port of `SHAPE_POLY_SET::TRIANGULATED_POLYGON::TRI`: a single triangle of a poly-set
/// triangulation (`SH_POLY_SET_TRIANGLE`).
///
/// The C++ triangle refers to the vertices of its parent `TRIANGULATED_POLYGON` through a
/// pointer; the port keeps copies of its three vertices next to the indices (D15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tri {
    /// Index of the first vertex in the parent's vertex list
    pub a: i32,
    /// Index of the second vertex
    pub b: i32,
    /// Index of the third vertex
    pub c: i32,
    /// The vertices `parent->m_vertices[a]`, `[b]` and `[c]`
    pub vertices: [Vector2I; 3],
}

impl Tri {
    /// A triangle with the given vertex indices into `a_vertices`.
    pub fn new(a: i32, b: i32, c: i32, a_vertices: &[Vector2I]) -> Self {
        Tri {
            a,
            b,
            c,
            vertices: [
                a_vertices[a as usize],
                a_vertices[b as usize],
                a_vertices[c as usize],
            ],
        }
    }

    /// Port of `TRI::Area()`.
    pub fn area(&self) -> f64 {
        let aa = self.vertices[0];
        let bb = self.vertices[1];
        let cc = self.vertices[2];

        let ba: Vector2D = (bb - aa).cast();
        let cb: Vector2D = (cc - bb).cast();

        (cb.cross(ba) * 0.5).abs()
    }
}

impl Shape for Tri {
    fn type_(&self) -> ShapeType {
        ShapeType::PolySetTriangle
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::PolySetTriangle(self)
    }

    /// `TRI` does not override `SHAPE::Clone()` in C++ (which asserts and returns null); the
    /// port returns a copy.
    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(*self)
    }

    /// Port of `TRIANGULATED_POLYGON::TRI::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let mut bbox = Box2I::from_pos(self.vertices[0]);
        bbox.merge_point(self.vertices[1]);
        bbox.merge_point(self.vertices[2]);

        if a_clearance != 0 {
            bbox.inflate(a_clearance);
        }

        bbox
    }

    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.lcb_collide_point(a_p, a_clearance, a_actual, a_location)
    }

    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.lcb_collide_seg(a_seg, a_clearance, a_actual, a_location)
    }

    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        self.lcb_squared_distance(a_p, a_outline_only)
    }

    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        self.lcb_point_inside(a_pt, a_accuracy, a_use_bbox_cache)
    }

    /// `SHAPE_LINE_CHAIN_BASE::TransformToPolygon()` does nothing.
    fn transform_to_polygon(
        &self,
        _a_buffer: &mut ShapePolySet,
        _a_error: i32,
        _a_error_loc: ErrorLoc,
    ) {
    }

    fn rotate(&mut self, _a_angle: EdaAngle, _a_center: Vector2I) {}

    fn move_(&mut self, _a_vector: Vector2I) {}

    fn is_solid(&self) -> bool {
        true
    }
}

impl ShapeLineChainBase for Tri {
    /// Port of `TRI::GetPoint()`; an invalid index yields the `wxCHECK` fallback (0, 0).
    fn get_point(&self, a_index: i32) -> Vector2I {
        match a_index {
            0..=2 => self.vertices[a_index as usize],
            _ => Vector2I::default(),
        }
    }

    /// Port of `TRI::GetSegment()`; an invalid index yields the `wxCHECK` fallback `SEG()`.
    fn get_segment(&self, a_index: i32) -> Seg {
        match a_index {
            0 => Seg::new(self.vertices[0], self.vertices[1]),
            1 => Seg::new(self.vertices[1], self.vertices[2]),
            2 => Seg::new(self.vertices[2], self.vertices[0]),
            _ => Seg::default(),
        }
    }

    fn get_point_count(&self) -> usize {
        3
    }

    fn get_segment_count(&self) -> usize {
        3
    }

    fn is_closed(&self) -> bool {
        true
    }
}

/// Port of `SHAPE_POLY_SET::TRIANGULATED_POLYGON` (the data and its simple accessors; building
/// a triangulation and `Refine()` are package G4b).
#[derive(Clone, Debug, Default)]
pub struct TriangulatedPolygon {
    /// `m_sourceOutline`
    pub source_outline: i32,
    /// `m_triangles`
    pub triangles: Vec<Tri>,
    /// `m_vertices`
    pub vertices: Vec<Vector2I>,
}

impl TriangulatedPolygon {
    /// Port of `TRIANGULATED_POLYGON( int aSourceOutline )`.
    pub fn new(a_source_outline: i32) -> Self {
        TriangulatedPolygon {
            source_outline: a_source_outline,
            ..Self::default()
        }
    }

    /// Port of `TRIANGULATED_POLYGON::Clear()`.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
    }

    /// Port of `TRIANGULATED_POLYGON::GetTriangle()`.
    pub fn get_triangle(&self, a_index: usize) -> (Vector2I, Vector2I, Vector2I) {
        let tri = &self.triangles[a_index];
        (
            self.vertices[tri.a as usize],
            self.vertices[tri.b as usize],
            self.vertices[tri.c as usize],
        )
    }

    /// Port of `TRIANGULATED_POLYGON::AddTriangle()`.
    pub fn add_triangle(&mut self, a: i32, b: i32, c: i32) {
        let tri = Tri::new(a, b, c, &self.vertices);
        self.triangles.push(tri);
    }

    /// Port of `TRIANGULATED_POLYGON::AddVertex()`.
    pub fn add_vertex(&mut self, a_p: Vector2I) {
        self.vertices.push(a_p);
    }

    /// Port of `TRIANGULATED_POLYGON::GetTriangleCount()`.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Port of `TRIANGULATED_POLYGON::GetSourceOutlineIndex()`.
    pub fn source_outline_index(&self) -> i32 {
        self.source_outline
    }

    /// Port of `TRIANGULATED_POLYGON::SetSourceOutlineIndex()`.
    pub fn set_source_outline_index(&mut self, a_index: i32) {
        self.source_outline = a_index;
    }

    /// Port of `TRIANGULATED_POLYGON::GetVertexCount()`.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Port of `TRIANGULATED_POLYGON::Move()` (the triangles' vertex copies move as well).
    pub fn move_(&mut self, a_vec: Vector2I) {
        for vertex in &mut self.vertices {
            *vertex += a_vec;
        }

        for tri in &mut self.triangles {
            for vertex in &mut tri.vertices {
                *vertex += a_vec;
            }
        }
    }

    /// Port of `TRIANGULATED_POLYGON::SetTriangles()`: the triangles (indices) of
    /// `a_triangles`, referring to this polygon's vertices.
    pub fn set_triangles(&mut self, a_triangles: &[Tri]) {
        self.triangles = a_triangles
            .iter()
            .map(|t| Tri::new(t.a, t.b, t.c, &self.vertices))
            .collect();
    }

    /// Port of `TRIANGULATED_POLYGON::SetVertices()` (the triangles' vertex copies follow).
    pub fn set_vertices(&mut self, a_vertices: &[Vector2I]) {
        self.vertices = a_vertices.to_vec();

        for tri in &mut self.triangles {
            *tri = Tri::new(tri.a, tri.b, tri.c, &self.vertices);
        }
    }

    /// Port of `TRIANGULATED_POLYGON::Refine()`: refine this triangulation in place with
    /// boundary-preserving Lawson edge flips, minimizing the number of sliver triangles and,
    /// on ties, maximizing the minimum angle.  Adds and removes no vertices or triangles.
    pub fn refine(&mut self) {
        use crate::geometry::geometry_predicates::{
            in_circle_delaunay_legal, is_sliver_triangle, orientation_sign,
        };

        let tri_count = self.triangles.len();

        // Indices are int; skip an implausibly huge mesh rather than overflow.
        if tri_count < 2 || tri_count > (i32::MAX as usize) / 6 {
            return;
        }

        let half_edge_count = (3 * tri_count) as i32;

        // heVertex[3k+j] is vertex j of triangle k.
        let mut he_vertex: Vec<i32> = vec![0; half_edge_count as usize];

        for (k, tri) in self.triangles.iter().enumerate() {
            he_vertex[3 * k] = tri.a;
            he_vertex[3 * k + 1] = tri.b;
            he_vertex[3 * k + 2] = tri.c;
        }

        let next_in_triangle = |a_edge: i32| a_edge - a_edge % 3 + (a_edge + 1) % 3;

        // An untwinned edge is a polygon boundary and must never flip. Slots are never
        // tombstoned so probe chains stay intact.
        let mut twin: Vec<i32> = vec![-1; half_edge_count as usize];

        let mut table_size: i32 = 1;

        while table_size < half_edge_count * 2 {
            table_size <<= 1;
        }

        let table_mask = (table_size - 1) as u32;
        let mut slot_edge: Vec<i32> = vec![-1; table_size as usize];

        // A non-manifold edge (three or more triangles, e.g. a hole-bridge pinch) is retired
        // so no triangle straddling it can flip.
        let mut retired: Vec<bool> = vec![false; table_size as usize];
        let mut to_visit: Vec<i32> = Vec::new();
        let mut queued: Vec<bool> = vec![false; half_edge_count as usize];

        let key = |from: i32, to: i32| -> (u32, u32) {
            if from < to {
                (from as u32, to as u32)
            } else {
                (to as u32, from as u32)
            }
        };

        for edge in 0..half_edge_count {
            let from = he_vertex[edge as usize];
            let to = he_vertex[next_in_triangle(edge) as usize];
            let (key_low, key_high) = key(from, to);
            let mut slot =
                (key_low.wrapping_mul(0x9e3779b1) ^ key_high.wrapping_mul(0x85ebca6b)) & table_mask;
            let mut handled = false;

            while slot_edge[slot as usize] != -1 {
                let other = slot_edge[slot as usize];
                let other_from = he_vertex[other as usize];
                let other_to = he_vertex[next_in_triangle(other) as usize];
                let (other_low, other_high) = key(other_from, other_to);

                if other_low == key_low && other_high == key_high {
                    if retired[slot as usize] {
                        // Non-manifold; stays a boundary.
                    } else if twin[other as usize] == -1 {
                        twin[edge as usize] = other;
                        twin[other as usize] = edge;
                        queued[other as usize] = true;
                        to_visit.push(other);
                    } else {
                        // Third half-edge here: retire the edge and unlink the pair already
                        // formed.
                        let mate = twin[other as usize];
                        twin[other as usize] = -1;
                        twin[mate as usize] = -1;
                        retired[slot as usize] = true;
                    }

                    handled = true;
                    break;
                }

                slot = (slot + 1) & table_mask;
            }

            if !handled {
                slot_edge[slot as usize] = edge;
            }
        }

        while let Some(edge) = to_visit.pop() {
            queued[edge as usize] = false;

            let twin_edge = twin[edge as usize];

            if twin_edge == -1 {
                continue;
            }

            let tri = edge - edge % 3;
            let twin_tri = twin_edge - twin_edge % 3;

            let apex_a_edge = tri + (edge + 2) % 3;
            let shared_end_edge = tri + (edge + 1) % 3;
            let apex_b_edge = twin_tri + (twin_edge + 2) % 3;
            let twin_shared_edge = twin_tri + (twin_edge + 1) % 3;

            let apex_a = he_vertex[apex_a_edge as usize];
            let shared_from = he_vertex[edge as usize];
            let shared_to = he_vertex[shared_end_edge as usize];
            let apex_b = he_vertex[apex_b_edge as usize];

            let apex_a_pt = self.vertices[apex_a as usize];
            let shared_from_pt = self.vertices[shared_from as usize];
            let shared_to_pt = self.vertices[shared_to as usize];
            let apex_b_pt = self.vertices[apex_b as usize];

            // A non-convex quad would flip a triangle outside the polygon.
            if orientation_sign(apex_a_pt, shared_from_pt, apex_b_pt) <= 0 {
                continue;
            }

            if orientation_sign(apex_a_pt, apex_b_pt, shared_to_pt) <= 0 {
                continue;
            }

            // Prefer the diagonal with fewer slivers, Delaunay breaks ties. Each flip lowers
            // (slivers, then -min-angle) lexicographically, so the loop terminates.
            let legal =
                in_circle_delaunay_legal(apex_a_pt, shared_from_pt, shared_to_pt, apex_b_pt);

            let cur_slivers = is_sliver_triangle(apex_a_pt, shared_from_pt, shared_to_pt) as i32
                + is_sliver_triangle(apex_b_pt, shared_to_pt, shared_from_pt) as i32;

            // Nothing to gain when already Delaunay and neither triangle is a sliver.
            if legal && cur_slivers == 0 {
                continue;
            }

            let flip_slivers = is_sliver_triangle(apex_a_pt, shared_from_pt, apex_b_pt) as i32
                + is_sliver_triangle(apex_a_pt, apex_b_pt, shared_to_pt) as i32;

            let do_flip = if flip_slivers != cur_slivers {
                flip_slivers < cur_slivers
            } else {
                !legal
            };

            if !do_flip {
                continue;
            }

            he_vertex[edge as usize] = apex_b;
            he_vertex[twin_edge as usize] = apex_a;

            let apex_b_outer_twin = twin[apex_b_edge as usize];
            let apex_a_outer_twin = twin[apex_a_edge as usize];

            twin[edge as usize] = apex_b_outer_twin;

            if apex_b_outer_twin != -1 {
                twin[apex_b_outer_twin as usize] = edge;
            }

            twin[twin_edge as usize] = apex_a_outer_twin;

            if apex_a_outer_twin != -1 {
                twin[apex_a_outer_twin as usize] = twin_edge;
            }

            twin[apex_a_edge as usize] = apex_b_edge;
            twin[apex_b_edge as usize] = apex_a_edge;

            // The flip may have made the quad's four outer edges illegal.
            for outer in [edge, twin_edge, shared_end_edge, twin_shared_edge] {
                if twin[outer as usize] != -1 && !queued[outer as usize] {
                    queued[outer as usize] = true;
                    to_visit.push(outer);
                }
            }
        }

        let refined: Vec<Tri> = (0..tri_count)
            .map(|i| {
                Tri::new(
                    he_vertex[3 * i],
                    he_vertex[3 * i + 1],
                    he_vertex[3 * i + 2],
                    &self.vertices,
                )
            })
            .collect();

        self.triangles = refined;
    }
}

/// Port of `SHAPE_POLY_SET::VERTEX_INDEX`: the relative indices of a vertex (polygon, contour
/// inside the polygon, vertex inside the contour).  All -1 by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VertexIndex {
    /// `m_polygon`: the index of the polygon
    pub polygon: i32,
    /// `m_contour`: the index of the contour relative to the polygon
    pub contour: i32,
    /// `m_vertex`: the index of the vertex relative to the contour
    pub vertex: i32,
}

impl Default for VertexIndex {
    fn default() -> Self {
        VertexIndex {
            polygon: -1,
            contour: -1,
            vertex: -1,
        }
    }
}

/// Port of `SHAPE_POLY_SET::ITERATOR_TEMPLATE` (`ITERATOR` and `CONST_ITERATOR`, which only
/// differ in constness): a cursor over the vertices of a range of polygons, optionally
/// including the holes.  It also implements `Iterator` (yielding the vertices), with the same
/// traversal order and end condition as the C++ `for( it = ...; it; it++ )` loop.
#[derive(Clone, Copy, Debug)]
pub struct PolySetIterator<'a> {
    poly: &'a ShapePolySet,
    current_polygon: i32,
    current_contour: i32,
    current_vertex: i32,
    last_polygon: i32,
    iterate_holes: bool,
}

impl PolySetIterator<'_> {
    /// Port of `ITERATOR_TEMPLATE::IsEndContour()`: whether the current vertex is the last one
    /// of the current contour.
    pub fn is_end_contour(&self) -> bool {
        self.current_vertex + 1
            == self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
                .point_count()
    }

    /// Port of `ITERATOR_TEMPLATE::IsLastPolygon()`.
    pub fn is_last_polygon(&self) -> bool {
        self.current_polygon == self.last_polygon
    }

    /// Port of `ITERATOR_TEMPLATE::operator bool()`: whether there are vertices left.
    ///
    /// As in C++, a cursor whose last polygon is not the last polygon of the set ends as soon
    /// as it reaches its last polygon.
    pub fn is_valid(&self) -> bool {
        if self.current_polygon < self.last_polygon {
            return true;
        }

        if self.current_polygon != self.poly.outline_count() - 1 {
            return false;
        }

        let current_polygon = self.poly.c_polygon(self.current_polygon);

        self.current_contour < current_polygon.len() as i32 - 1
            || self.current_vertex < current_polygon[self.current_contour as usize].point_count()
    }

    /// Port of `ITERATOR_TEMPLATE::Advance()` (`operator++`).
    pub fn advance(&mut self) {
        // Advance vertex index
        self.current_vertex += 1;

        // Check whether the user wants to iterate through the vertices of the holes
        // and behave accordingly
        if self.iterate_holes {
            // If the last vertex of the contour was reached, advance the contour index
            if self.current_vertex
                >= self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
                    .point_count()
            {
                self.current_vertex = 0;
                self.current_contour += 1;

                // If the last contour of the current polygon was reached, advance the
                // outline index
                let total_contours = self.poly.c_polygon(self.current_polygon).len() as i32;

                if self.current_contour >= total_contours {
                    self.current_contour = 0;
                    self.current_polygon += 1;
                }
            }
        } else {
            // If the last vertex of the outline was reached, advance to the following polygon
            if self.current_vertex >= self.poly.c_polygon(self.current_polygon)[0].point_count() {
                self.current_vertex = 0;
                self.current_polygon += 1;
            }
        }
    }

    /// Port of `ITERATOR_TEMPLATE::Get()` (`operator*`).
    pub fn get(&self) -> Vector2I {
        self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
            .c_point(self.current_vertex)
    }

    /// Port of `ITERATOR_TEMPLATE::GetIndex()`.
    pub fn get_index(&self) -> VertexIndex {
        VertexIndex {
            polygon: self.current_polygon,
            contour: self.current_contour,
            vertex: self.current_vertex,
        }
    }
}

impl Iterator for PolySetIterator<'_> {
    type Item = Vector2I;

    fn next(&mut self) -> Option<Vector2I> {
        if !self.is_valid() {
            return None;
        }

        let v = self.get();
        self.advance();
        Some(v)
    }
}

/// Port of `SHAPE_POLY_SET::SEGMENT_ITERATOR_TEMPLATE` (`SEGMENT_ITERATOR` and
/// `CONST_SEGMENT_ITERATOR`): a cursor over the edges of a range of polygons.  Also an
/// `Iterator` yielding the segments.
#[derive(Clone, Copy, Debug)]
pub struct SegmentIterator<'a> {
    poly: &'a ShapePolySet,
    current_polygon: i32,
    current_contour: i32,
    current_segment: i32,
    last_polygon: i32,
    iterate_holes: bool,
}

impl SegmentIterator<'_> {
    /// Port of `SEGMENT_ITERATOR_TEMPLATE::IsLastPolygon()`.
    pub fn is_last_polygon(&self) -> bool {
        self.current_polygon == self.last_polygon
    }

    /// Port of `SEGMENT_ITERATOR_TEMPLATE::operator bool()`.
    pub fn is_valid(&self) -> bool {
        self.current_polygon <= self.last_polygon
    }

    /// Port of `SEGMENT_ITERATOR_TEMPLATE::Advance()` (`operator++`).
    pub fn advance(&mut self) {
        // Advance vertex index
        self.current_segment += 1;

        // Check whether the user wants to iterate through the vertices of the holes
        // and behave accordingly.
        if self.iterate_holes {
            let last = self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
                .segment_count();

            // If the last vertex of the contour was reached, advance the contour index.
            if self.current_segment >= last {
                self.current_segment = 0;
                self.current_contour += 1;

                // If the last contour of the current polygon was reached, advance the
                // outline index.
                let total_contours = self.poly.c_polygon(self.current_polygon).len() as i32;

                if self.current_contour >= total_contours {
                    self.current_contour = 0;
                    self.current_polygon += 1;
                }
            }
        } else {
            let last = self.poly.c_polygon(self.current_polygon)[0].segment_count();

            // If the last vertex of the outline was reached, advance to the following
            // polygon
            if self.current_segment >= last {
                self.current_segment = 0;
                self.current_polygon += 1;
            }
        }
    }

    /// Port of `SEGMENT_ITERATOR_TEMPLATE::Get()` (`operator*`).
    pub fn get(&self) -> Seg {
        self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
            .segment(self.current_segment)
    }

    /// Port of `SEGMENT_ITERATOR_TEMPLATE::GetIndex()` (`m_vertex` is the segment index).
    pub fn get_index(&self) -> VertexIndex {
        VertexIndex {
            polygon: self.current_polygon,
            contour: self.current_contour,
            vertex: self.current_segment,
        }
    }

    /// Port of `SEGMENT_ITERATOR_TEMPLATE::IsAdjacent()`: whether both iterators point to
    /// adjacent segments of the same contour of the same polygon set.
    pub fn is_adjacent(&self, a_other: &SegmentIterator<'_>) -> bool {
        // Check that both iterators point to the same contour of the same polygon of the
        // same polygon set.
        if std::ptr::eq(self.poly, a_other.poly)
            && self.current_polygon == a_other.current_polygon
            && self.current_contour == a_other.current_contour
        {
            // Compute the total number of segments.
            let num_seg = self.poly.c_polygon(self.current_polygon)[self.current_contour as usize]
                .segment_count();

            // Compute the difference of the segment indices. If it is exactly one, they
            // are adjacent. The only missing case where they also are adjacent is when
            // the segments are the first and last one, in which case the difference
            // always equals the total number of segments minus one.
            let index_diff = (self.current_segment - a_other.current_segment).abs();

            return (index_diff == 1) || (index_diff == (num_seg - 1));
        }

        false
    }
}

impl Iterator for SegmentIterator<'_> {
    type Item = Seg;

    fn next(&mut self) -> Option<Seg> {
        if !self.is_valid() {
            return None;
        }

        let s = self.get();
        self.advance();
        Some(s)
    }
}

/// Port of `SHAPE_POLY_SET::CORNER_MODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CornerMode {
    Chamfered,
    Filleted,
}

/// Port of `SHAPE_POLY_SET`: a set of polygons, each with an outline and optional holes.
#[derive(Debug, Default)]
pub struct ShapePolySet {
    polys: Vec<Polygon>,
    triangulated_polys: Vec<TriangulatedPolygon>,
    /// `m_triIndex`: the spatial index over the triangles (dropped whenever they change)
    tri_index: Option<TriangulationIndex>,
    triangulation_valid: bool,
    hash: Hash128,
    hash_valid: bool,
    failed_hash: Hash128,
    failed_hash_valid: bool,
}

impl Clone for ShapePolySet {
    /// Port of the copy constructor `SHAPE_POLY_SET( const SHAPE_POLY_SET& )`: the
    /// triangulation (and the hash) are copied only while the triangulation is up to date.
    fn clone(&self) -> Self {
        let mut set = ShapePolySet {
            polys: self.polys.clone(),
            failed_hash: self.failed_hash,
            failed_hash_valid: self.failed_hash_valid,
            ..ShapePolySet::default()
        };

        if self.is_triangulation_up_to_date() {
            set.triangulated_polys = self.triangulated_polys.clone();
            set.hash = self.get_hash();
            set.hash_valid = true;
            set.triangulation_valid = true;
        }

        set
    }
}

impl ShapePolySet {
    /// Port of `SHAPE_POLY_SET()`: an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `SHAPE_POLY_SET( const BOX2D& aRect )`: a set with a single rectangular outline.
    pub fn from_box2d(a_rect: &Box2D) -> Self {
        // VECTOR2I( double, double ): the coordinates are truncated
        let v = |x: f64, y: f64| Vector2I::new(cmath::f64_to_i32_x86(x), cmath::f64_to_i32_x86(y));
        let mut set = Self::new();
        set.new_outline();
        set.append_point(v(a_rect.left(), a_rect.top()), -1, -1);
        set.append_point(v(a_rect.right(), a_rect.top()), -1, -1);
        set.append_point(v(a_rect.right(), a_rect.bottom()), -1, -1);
        set.append_point(v(a_rect.left(), a_rect.bottom()), -1, -1);
        set.outline(0).set_closed(true);
        set
    }

    /// Port of `SHAPE_POLY_SET( const SHAPE_LINE_CHAIN& aOutline )`.
    pub fn from_outline(a_outline: &ShapeLineChain) -> Self {
        let mut set = Self::new();
        set.add_outline(a_outline);
        set
    }

    /// Port of `SHAPE_POLY_SET( const POLYGON& aPolygon )`.
    pub fn from_polygon(a_polygon: &Polygon) -> Self {
        let mut set = Self::new();
        set.add_polygon(a_polygon);
        set
    }

    /// Port of `SHAPE_POLY_SET::CloneDropTriangulation()`.
    pub fn clone_drop_triangulation(&self) -> Self {
        ShapePolySet {
            polys: self.polys.clone(),
            ..ShapePolySet::default()
        }
    }

    /// The move assignment `*this = std::move( aOther )`: everything but the failed-hash
    /// state is taken from `a_other`.
    fn move_assign(&mut self, a_other: ShapePolySet) {
        self.polys = a_other.polys;
        self.triangulated_polys = a_other.triangulated_polys;
        self.tri_index = a_other.tri_index;
        self.hash = a_other.hash;
        self.hash_valid = a_other.hash_valid;
        self.triangulation_valid = a_other.triangulation_valid;
    }

    /// Port of `SHAPE_POLY_SET::GetRelativeIndices()`: the relative indices of the
    /// `a_global_idx`-th vertex, `None` if it does not exist.
    pub fn get_relative_indices(&self, a_global_idx: i32) -> Option<VertexIndex> {
        let mut current_global_idx = 0;

        for (polygon_idx, current_polygon) in self.polys.iter().enumerate() {
            for (contour_idx, current_contour) in current_polygon.iter().enumerate() {
                let total_points = current_contour.point_count();

                for vertex_idx in 0..total_points {
                    // Check if the current vertex is the globally indexed as aGlobalIdx
                    if current_global_idx == a_global_idx {
                        return Some(VertexIndex {
                            polygon: polygon_idx as i32,
                            contour: contour_idx as i32,
                            vertex: vertex_idx,
                        });
                    }

                    // Advance
                    current_global_idx += 1;
                }
            }
        }

        None
    }

    /// Port of `SHAPE_POLY_SET::GetGlobalIndex()`: the global index of the vertex with the
    /// given relative indices, `None` if the indices make no sense in this set.
    pub fn get_global_index(&self, a_relative_indices: VertexIndex) -> Option<i32> {
        let selected_vertex = a_relative_indices.vertex;
        let selected_contour = a_relative_indices.contour as u32 as usize;
        let selected_polygon = a_relative_indices.polygon as u32 as usize;

        // Check whether the vertex indices make sense in this poly set
        if selected_polygon < self.polys.len()
            && selected_contour < self.polys[selected_polygon].len()
            && selected_vertex < self.polys[selected_polygon][selected_contour].point_count()
        {
            let mut global_idx = 0;

            for current_polygon in &self.polys[..selected_polygon] {
                for contour in current_polygon {
                    global_idx += contour.point_count();
                }
            }

            for contour in &self.polys[selected_polygon][..selected_contour] {
                global_idx += contour.point_count();
            }

            global_idx += selected_vertex;

            Some(global_idx)
        } else {
            None
        }
    }

    /// Port of `SHAPE_POLY_SET::NewOutline()`: creates a new empty (closed) outline and returns
    /// its index.
    pub fn new_outline(&mut self) -> i32 {
        let mut empty_path = ShapeLineChain::new();

        empty_path.set_closed(true);
        self.polys.push(vec![empty_path]);
        self.polys.len() as i32 - 1
    }

    /// Port of `SHAPE_POLY_SET::NewHole()`: creates a new empty (closed) hole in the given
    /// outline (negative: counted from the end, -1 is the last outline) and returns its index.
    pub fn new_hole(&mut self, a_outline: i32) -> i32 {
        let mut empty_path = ShapeLineChain::new();

        empty_path.set_closed(true);

        let mut a_outline = a_outline;

        // Default outline is the last one
        if a_outline < 0 {
            a_outline += self.polys.len() as i32;
        }

        // Add hole to the selected outline
        self.polys[a_outline as usize].push(empty_path);

        // As in C++: the hole count of the *last* polygon
        self.polys.last().unwrap().len() as i32 - 2
    }

    /// The contour addressed by `a_outline` (negative: from the end) and `a_hole` (negative:
    /// the outline itself), as `Append()` / `CVertex()` resolve it.
    fn contour_index(&self, a_outline: i32, a_hole: i32) -> (usize, usize) {
        debug_assert!(!self.polys.is_empty());

        let mut a_outline = a_outline;

        if a_outline < 0 {
            a_outline += self.polys.len() as i32;
        }

        let idx: usize = if a_hole < 0 { 0 } else { a_hole as usize + 1 };

        debug_assert!((a_outline as usize) < self.polys.len());
        debug_assert!(idx < self.polys[a_outline as usize].len());

        (a_outline as usize, idx)
    }

    /// Port of `SHAPE_POLY_SET::Append( int x, int y, int aOutline, int aHole,
    /// bool aAllowDuplication )`: add a vertex to the given outline / hole.
    ///
    /// A negative `a_outline` counts from the end (-1 is the last outline); a negative
    /// `a_hole` means the outline itself.  Returns the number of points of the contour.
    pub fn append(
        &mut self,
        x: i32,
        y: i32,
        a_outline: i32,
        a_hole: i32,
        a_allow_duplication: bool,
    ) -> i32 {
        let (o, idx) = self.contour_index(a_outline, a_hole);
        let contour = &mut self.polys[o][idx];
        contour.append_xy(x, y, a_allow_duplication);

        contour.point_count()
    }

    /// Port of `SHAPE_POLY_SET::Append( const VECTOR2I& aP, int aOutline, int aHole )`.
    pub fn append_point(&mut self, a_p: Vector2I, a_outline: i32, a_hole: i32) {
        self.append(a_p.x, a_p.y, a_outline, a_hole, false);
    }

    /// Port of `SHAPE_POLY_SET::Append( const SHAPE_ARC& aArc, int aOutline, int aHole,
    /// std::optional<int> aMaxError )`: append an arc; returns the number of points of the
    /// contour.
    pub fn append_arc(
        &mut self,
        a_arc: &ShapeArc,
        a_outline: i32,
        a_hole: i32,
        a_max_error: Option<i32>,
    ) -> i32 {
        let (o, idx) = self.contour_index(a_outline, a_hole);
        let contour = &mut self.polys[o][idx];

        match a_max_error {
            Some(err) => contour.append_arc_with_error(a_arc, err),
            None => contour.append_arc(a_arc),
        }

        contour.point_count()
    }

    /// Port of `SHAPE_POLY_SET::Append( const SHAPE_POLY_SET& aSet )`: merge polygons from two
    /// sets.
    pub fn append_set(&mut self, a_set: &ShapePolySet) {
        self.polys.extend(a_set.polys.iter().cloned());
    }

    /// Port of `SHAPE_POLY_SET::InsertVertex()`: adds a vertex in the globally indexed position
    /// `a_global_index` (clamped to 0; at or beyond the end it is appended to the last
    /// outline).
    pub fn insert_vertex(&mut self, a_global_index: i32, a_new_vertex: Vector2I) {
        let a_global_index = a_global_index.max(0);

        if a_global_index >= self.total_vertices() {
            self.append_point(a_new_vertex, -1, -1);
        } else {
            // Assure the position to be inserted exists (always true here)
            let index = self
                .get_relative_indices(a_global_index)
                .expect("aGlobalIndex-th vertex does not exist");
            self.polys[index.polygon as usize][index.contour as usize]
                .insert_point(index.vertex as usize, a_new_vertex);
        }
    }

    /// Port of `SHAPE_POLY_SET::VertexCount()`: the number of vertices of the given outline /
    /// hole (default: the outline of the last polygon); 0 if it does not exist.
    pub fn vertex_count(&self, a_outline: i32, a_hole: i32) -> i32 {
        if self.polys.is_empty() {
            // Empty poly set
            return 0;
        }

        let mut a_outline = a_outline;

        if a_outline < 0 {
            // Use last outline
            a_outline += self.polys.len() as i32;
        }

        let idx: i32 = if a_hole < 0 { 0 } else { a_hole + 1 };

        if a_outline >= self.polys.len() as i32 {
            // not existing outline
            return 0;
        }

        if idx >= self.polys[a_outline as usize].len() as i32 {
            // not existing hole
            return 0;
        }

        self.polys[a_outline as usize][idx as usize].point_count()
    }

    /// Port of `SHAPE_POLY_SET::FullPointCount()`: the number of points in the set.
    pub fn full_point_count(&self) -> i32 {
        let mut full_count = 0;

        for ii in 0..self.outline_count() {
            // the first polygon in m_polys[ii] is the main contour,
            // only others are holes:
            for idx in 0..=self.hole_count(ii) {
                full_count += self.polys[ii as usize][idx as usize].point_count();
            }
        }

        full_count
    }

    /// Port of `SHAPE_POLY_SET::Subset()`: the polygons from `a_first_polygon` (included) to
    /// `a_last_polygon` (excluded).
    pub fn subset(&self, a_first_polygon: i32, a_last_polygon: i32) -> ShapePolySet {
        debug_assert!(a_first_polygon >= 0 && a_last_polygon <= self.outline_count());

        let mut new_poly_set = ShapePolySet::new();

        for index in a_first_polygon..a_last_polygon {
            new_poly_set.polys.push(self.polys[index as usize].clone());
        }

        new_poly_set
    }

    /// Port of `SHAPE_POLY_SET::UnitSet()`.
    pub fn unit_set(&self, a_polygon_index: i32) -> ShapePolySet {
        self.subset(a_polygon_index, a_polygon_index + 1)
    }

    /// Port of `SHAPE_POLY_SET::CVertex( int aIndex, int aOutline, int aHole )`.
    pub fn c_vertex(&self, a_index: i32, a_outline: i32, a_hole: i32) -> Vector2I {
        let (o, idx) = self.contour_index(a_outline, a_hole);
        self.polys[o][idx].c_point(a_index)
    }

    /// Port of `SHAPE_POLY_SET::CVertex( int aGlobalIndex )`.
    ///
    /// # Panics
    ///
    /// If the vertex does not exist (C++ throws `std::out_of_range`).
    pub fn c_vertex_global(&self, a_global_index: i32) -> Vector2I {
        // Assure the passed index references a legal position; abort otherwise
        let index = self
            .get_relative_indices(a_global_index)
            .expect("aGlobalIndex-th vertex does not exist");

        self.polys[index.polygon as usize][index.contour as usize].c_point(index.vertex)
    }

    /// Port of `SHAPE_POLY_SET::CVertex( VERTEX_INDEX )`.
    pub fn c_vertex_index(&self, a_index: VertexIndex) -> Vector2I {
        self.c_vertex(a_index.vertex, a_index.polygon, a_index.contour - 1)
    }

    /// Port of `SHAPE_POLY_SET::GetNeighbourIndexes()`: the global indices of the previous and
    /// the next corner of the `a_global_index`-th corner in its contour, `None` if the corner
    /// does not exist.
    ///
    /// Where the C++ leaves an output uninitialised (a neighbour index outside a contour with
    /// fewer than 3 points) the port returns -1.
    pub fn get_neighbour_indexes(&self, a_global_index: i32) -> Option<(i32, i32)> {
        // If the edge does not exist, throw an exception, it is an illegal access memory error
        let mut index = self.get_relative_indices(a_global_index)?;

        // Calculate the previous and next index of aGlobalIndex, corresponding to
        // the same contour;
        let mut inext = index;
        let lastpoint = self.polys[index.polygon as usize][index.contour as usize].segment_count();

        if index.vertex == 0 {
            index.vertex = lastpoint - 1;
            inext.vertex = 1;
        } else if index.vertex == lastpoint {
            index.vertex -= 1;
            inext.vertex = 0;
        } else {
            inext.vertex += 1;
            index.vertex -= 1;

            if inext.vertex == lastpoint {
                inext.vertex = 0;
            }
        }

        let previous = self.get_global_index(index).unwrap_or(-1);
        let next = self.get_global_index(inext).unwrap_or(-1);

        Some((previous, next))
    }

    /// Port of `SHAPE_POLY_SET::IsPolygonSelfIntersecting()`.
    pub fn is_polygon_self_intersecting(&self, a_polygon_index: i32) -> bool {
        // SEG::Index() counts within one contour, so adjacency needs the contour too
        let mut segments: Vec<(Seg, i32)> = Vec::with_capacity(self.full_point_count() as usize);

        let mut it = self.c_iterate_segments_with_holes_outline(a_polygon_index);

        while it.is_valid() {
            segments.push((it.get(), it.get_index().contour));
            it.advance();
        }

        std_sort(&mut segments, |a_a, a_b| {
            let a = &a_a.0;
            let b = &a_b.0;
            let min_a_x = a.a.x.min(a.b.x);
            let min_b_x = b.a.x.min(b.b.x);

            min_a_x < min_b_x || (min_a_x == min_b_x && a.a.y.min(a.b.y) < b.a.y.min(b.b.y))
        });

        let polygon = &self.polys[a_polygon_index as usize];

        for (i, (first_segment, first_contour)) in segments.iter().enumerate() {
            // Iterate through all remaining segments.
            let max_x = first_segment.a.x.max(first_segment.b.x);
            let max_y = first_segment.a.y.max(first_segment.b.y);

            // Start in the next segment, we don't want to check collision between a segment
            // and itself
            for (second_segment, second_contour) in &segments[i + 1..] {
                let min_x = second_segment.a.x.min(second_segment.b.x);
                let min_y = second_segment.a.y.min(second_segment.b.y);

                // We are ordered in minimum point order, so checking the static max (first
                // segment) against the ordered min will tell us if any of the following
                // segments are withing the BBox
                if max_x < min_x || (max_x == min_x && max_y < min_y) {
                    break;
                }

                let mut adjacent = false;

                if first_contour == second_contour {
                    let index_diff = (first_segment.index() - second_segment.index()).abs();
                    adjacent = (index_diff == 1)
                        || (index_diff == polygon[*first_contour as usize].segment_count() - 1);
                }

                // Check whether the two segments built collide, only when they are not
                // adjacent.
                if !adjacent && first_segment.collide(second_segment, 0) {
                    return true;
                }
            }
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::IsSelfIntersecting()`.
    pub fn is_self_intersecting(&self) -> bool {
        (0..self.polys.len()).any(|polygon| self.is_polygon_self_intersecting(polygon as i32))
    }

    /// Port of `SHAPE_POLY_SET::AddOutline()`: adds a new outline and returns its index.
    ///
    /// A non-closed outline is closed (a `wxCHECK2_MSG` in C++: "Warning: non-closed outline
    /// added to SHAPE_POLY_SET").
    pub fn add_outline(&mut self, a_outline: &ShapeLineChain) -> i32 {
        let mut poly: Polygon = vec![a_outline.clone()];

        // This is an assertion because if it's generated by KiCad code, it probably
        // indicates a bug elsewhere that should be fixed, but we also auto-fix it here
        // for SWIG plugins that might mess it up
        if !a_outline.is_closed() {
            poly.last_mut().unwrap().set_closed(true);
        }

        self.polys.push(poly);

        self.polys.len() as i32 - 1
    }

    /// Port of `SHAPE_POLY_SET::AddHole()`: adds a new hole to the given outline (default: the
    /// last one) and returns its index.
    pub fn add_hole(&mut self, a_hole: &ShapeLineChain, a_outline: i32) -> i32 {
        debug_assert!(!self.polys.is_empty());

        let mut a_outline = a_outline;

        if a_outline < 0 {
            a_outline += self.polys.len() as i32;
        }

        debug_assert!((a_outline as usize) < self.polys.len());

        let poly = &mut self.polys[a_outline as usize];

        debug_assert!(!poly.is_empty());

        poly.push(a_hole.clone());

        poly.len() as i32 - 2
    }

    /// Port of `SHAPE_POLY_SET::AddPolygon()`: adds a polygon and returns its index.
    pub fn add_polygon(&mut self, a_polygon: &Polygon) -> i32 {
        self.polys.push(a_polygon.clone());

        self.polys.len() as i32 - 1
    }

    /// Port of `SHAPE_POLY_SET::Area()`: the area of the outlines minus the area of the holes.
    pub fn area(&self) -> f64 {
        let mut area = 0.0;

        for i in 0..self.outline_count() {
            area += self.c_outline(i).area(true);

            for j in 0..self.hole_count(i) {
                area -= self.c_hole(i, j).area(true);
            }
        }

        area
    }

    /// Port of `SHAPE_POLY_SET::ArcCount()`.
    pub fn arc_count(&self) -> i32 {
        let mut retval = 0;

        for poly in &self.polys {
            for chain in poly {
                retval += chain.arc_count() as i32;
            }
        }

        retval
    }

    /// Port of `SHAPE_POLY_SET::GetArcs()`: appends all the arcs of the set to `a_arc_buffer`.
    pub fn get_arcs(&self, a_arc_buffer: &mut Vec<ShapeArc>) {
        for poly in &self.polys {
            for chain in poly {
                a_arc_buffer.extend(chain.c_arcs().iter().cloned());
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::ClearArcs()`.
    pub fn clear_arcs(&mut self) {
        for poly in &mut self.polys {
            for chain in poly {
                chain.clear_arcs();
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::RebuildHolesFromContours()`: extract all contours from this
    /// polygon set, then recreate polygons with holes.  Essentially XOR'ing, but faster.
    /// Self-intersecting polygons are not supported.
    pub fn rebuild_holes_from_contours(&mut self) {
        let mut contours: Vec<ShapeLineChain> = Vec::new();

        for poly in &self.polys {
            contours.extend(poly.iter().cloned());
        }

        let mut parent_to_children: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut child_to_parents: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();

        for contour in &mut contours {
            contour.generate_bbox_cache();
        }

        for i in 0..contours.len() {
            let outline = &contours[i];

            for j in 0..contours.len() {
                if i == j {
                    continue;
                }

                let candidate = &contours[j];
                let pt0 = candidate.c_point(0);

                if outline.point_inside(pt0, 0, true) {
                    parent_to_children.entry(i).or_default().insert(j);
                    child_to_parents.entry(j).or_default().insert(i);
                }
            }
        }

        let mut top_level_parents: BTreeSet<usize> = BTreeSet::new();

        for i in 0..contours.len() {
            if child_to_parents.entry(i).or_default().is_empty() {
                top_level_parents.insert(i);
            }
        }

        let mut result = ShapePolySet::new();

        fn process(
            my_id: usize,
            parent_outline_id: i32,
            path: &[usize],
            contours: &[ShapeLineChain],
            parent_to_children: &BTreeMap<usize, BTreeSet<usize>>,
            child_to_parents: &BTreeMap<usize, BTreeSet<usize>>,
            result: &mut ShapePolySet,
        ) {
            let mut my_outline = -1;

            let is_outline = path.len().is_multiple_of(2);

            if is_outline {
                let outline_id = result.add_outline(&contours[my_id]);
                my_outline = outline_id;
            } else {
                debug_assert!(parent_outline_id != -1);
                result.add_hole(&contours[my_id], parent_outline_id);
            }

            if let Some(children) = parent_to_children.get(&my_id) {
                let mut this_path = path.to_vec();
                this_path.push(my_id);

                let this_path_set: BTreeSet<usize> = this_path.iter().copied().collect();

                for &child_id in children {
                    let empty = BTreeSet::new();
                    let child_path_set = child_to_parents.get(&child_id).unwrap_or(&empty);

                    if this_path_set != *child_path_set {
                        continue; // Only interested in immediate children
                    }

                    process(
                        child_id,
                        my_outline,
                        &this_path,
                        contours,
                        parent_to_children,
                        child_to_parents,
                        result,
                    );
                }
            }
        }

        for &top_parent_id in &top_level_parents {
            process(
                top_parent_id,
                -1,
                &[],
                &contours,
                &parent_to_children,
                &child_to_parents,
                &mut result,
            );
        }

        self.move_assign(result);
    }

    /// Port of `SHAPE_POLY_SET::booleanOp( ClipType, const SHAPE_POLY_SET& aOtherShape )`.
    fn boolean_op(&mut self, a_type: c2::ClipType, a_other_shape: &ShapePolySet) {
        let (tree, z_values, arc_buffer) = Self::boolean_op_tree(a_type, self, a_other_shape);
        self.import_tree(&tree, &z_values, &arc_buffer);
    }

    /// Port of `SHAPE_POLY_SET::booleanOp( ClipType, const SHAPE_POLY_SET& aShape,
    /// const SHAPE_POLY_SET& aOtherShape )`.
    fn boolean_op_sets(
        &mut self,
        a_type: c2::ClipType,
        a_shape: &ShapePolySet,
        a_other_shape: &ShapePolySet,
    ) {
        let (tree, z_values, arc_buffer) = Self::boolean_op_tree(a_type, a_shape, a_other_shape);
        self.import_tree(&tree, &z_values, &arc_buffer);
    }

    /// Port of the static `SHAPE_POLY_SET::appendBridgeFreePaths()`: append the rings left
    /// after cutting the fracture bridges out of `a_chain`.  Clipper2 resolves every bridge
    /// with a pass over the whole output, which is quadratic on a fractured hatch plane.
    /// Dropping a pair of coincident opposite edges changes no winding number, so a NonZero
    /// boolean sees the same region.
    ///
    /// Returns false, leaving `a_paths` untouched, when `a_chain` holds no bridges.
    fn append_bridge_free_paths(
        a_chain: &ShapeLineChain,
        a_paths: &mut c2::Paths64,
        a_z_values: &mut Vec<ClipperZValue>,
        a_arc_buffer: &mut Vec<ShapeArc>,
    ) -> bool {
        // Small rings are cheap for Clipper and dominate the boolean call count
        const K_MIN_POINTS: i32 = 1024;

        if a_chain.point_count() < K_MIN_POINTS || a_chain.arc_count() > 0 {
            return false;
        }

        let mut rings: Vec<ShapeLineChain> = Vec::new();

        if !split_at_bridges(a_chain, &mut rings) {
            return false;
        }

        // convertToClipper2() orients an outline by reversing the whole ring, so every piece
        // of the ring follows the same flip to keep its winding relative to the others
        let flip = a_chain.area(false) < 0.0;

        for ring in &rings {
            a_paths.push(to_path64(
                ring,
                (ring.area(false) >= 0.0) != flip,
                a_z_values,
                a_arc_buffer,
            ));
        }

        true
    }

    /// The Clipper2 part of `booleanOp()`: runs the operation (fill rule NonZero, with the Z
    /// callback that records arc indices of intersection points) and returns the solution
    /// tree with the Z value and arc buffers.
    fn boolean_op_tree(
        a_type: c2::ClipType,
        a_shape: &ShapePolySet,
        a_other_shape: &ShapePolySet,
    ) -> (c2::PolyTree64, Vec<ClipperZValue>, Vec<ShapeArc>) {
        // C++: wxFAIL_MSG( "Boolean ops on curved polygons are not supported. You should call
        // ClearArcs() before carrying out the boolean operation." ) when a set with more than
        // one outline (or any clip) has arcs; a debug-build assertion only.

        let mut c = c2::Clipper64::new();

        let mut z_values: Vec<ClipperZValue> = Vec::new();
        let mut arc_buffer: Vec<ShapeArc> = Vec::new();

        let mut paths: c2::Paths64 = Vec::new();
        let mut clips: c2::Paths64 = Vec::new();

        for poly in &a_shape.polys {
            if poly.len() == 1
                && Self::append_bridge_free_paths(
                    &poly[0],
                    &mut paths,
                    &mut z_values,
                    &mut arc_buffer,
                )
            {
                continue;
            }

            for (i, chain) in poly.iter().enumerate() {
                paths.push(to_path64(chain, i == 0, &mut z_values, &mut arc_buffer));
            }
        }

        for poly in &a_other_shape.polys {
            if poly.len() == 1
                && Self::append_bridge_free_paths(
                    &poly[0],
                    &mut clips,
                    &mut z_values,
                    &mut arc_buffer,
                )
            {
                continue;
            }

            for (i, chain) in poly.iter().enumerate() {
                clips.push(to_path64(chain, i == 0, &mut z_values, &mut arc_buffer));
            }
        }

        c.add_subject(&paths);
        c.add_clip(&clips);

        // The callback must be 'static: the Z value buffer is shared with it.
        let shared_z = Rc::new(RefCell::new(z_values));
        let cb_z = Rc::clone(&shared_z);

        let callback = move |e1bot: c2::Point64,
                             e1top: c2::Point64,
                             e2bot: c2::Point64,
                             e2top: c2::Point64,
                             pt: &mut c2::Point64| {
            let mut z_values = cb_z.borrow_mut();

            // zValues.at(): an index outside the buffer throws in C++; it cannot happen for
            // points coming from convertToClipper2() or from this callback.  The port treats
            // it as "no arc".
            let at = |z_values: &Vec<ClipperZValue>, a_z: i64| -> Option<ClipperZValue> {
                usize::try_from(a_z)
                    .ok()
                    .and_then(|i| z_values.get(i))
                    .copied()
            };

            let arc_index = |z_values: &Vec<ClipperZValue>, a_zvalue: i64, a_compare_val: isize| {
                let Some(z) = at(z_values, a_zvalue) else {
                    return -1;
                };

                let mut retval = z.second_arc_idx;

                if retval == -1 || (a_compare_val > 0 && retval != a_compare_val) {
                    retval = z.first_arc_idx;
                }

                retval
            };

            let arc_segment = |z_values: &Vec<ClipperZValue>, a_bottom_z: i64, a_top_z: i64| {
                let mut retval = arc_index(z_values, a_bottom_z, -1);

                if retval != -1 && retval != arc_index(z_values, a_top_z, retval) {
                    retval = -1; // Not an arc segment as the two indices do not match
                }

                retval
            };

            let e1_arc_segment_index = arc_segment(&z_values, e1bot.z, e1top.z);
            let e2_arc_segment_index = arc_segment(&z_values, e2bot.z, e2top.z);

            let new_zval = if e1_arc_segment_index != -1 {
                ClipperZValue {
                    first_arc_idx: e1_arc_segment_index,
                    second_arc_idx: e2_arc_segment_index,
                }
            } else {
                ClipperZValue {
                    first_arc_idx: e2_arc_segment_index,
                    second_arc_idx: -1,
                }
            };

            let z_value_ptr = z_values.len();
            z_values.push(new_zval);

            // C++ also records arc intersection points in a map that is never read
            // (newIntersectPoints); nothing to port.

            pt.z = z_value_ptr as i64;
            //@todo amend X,Y values to true intersection between arcs or arc and segment
        };

        c.set_z_callback(Some(Box::new(callback))); // register callback

        // The C++ ignores Execute()'s result and imports whatever the tree holds (empty on
        // failure).
        let solution = c
            .execute_tree(a_type, c2::FillRule::NonZero)
            .map(|(tree, _)| tree)
            .unwrap_or_default();

        drop(c);

        let z_values = Rc::try_unwrap(shared_z)
            .map(RefCell::into_inner)
            .unwrap_or_else(|rc| rc.borrow().clone());

        (solution, z_values, arc_buffer)
    }

    /// Port of `SHAPE_POLY_SET::BooleanAdd( const SHAPE_POLY_SET& b )`: boolean polyset union.
    pub fn boolean_add(&mut self, b: &ShapePolySet) {
        self.boolean_op(c2::ClipType::Union, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanSubtract( const SHAPE_POLY_SET& b )`: boolean polyset
    /// difference.
    pub fn boolean_subtract(&mut self, b: &ShapePolySet) {
        self.boolean_op(c2::ClipType::Difference, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanIntersection( const SHAPE_POLY_SET& b )`.
    pub fn boolean_intersection(&mut self, b: &ShapePolySet) {
        self.boolean_op(c2::ClipType::Intersection, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanXor( const SHAPE_POLY_SET& b )`.
    pub fn boolean_xor(&mut self, b: &ShapePolySet) {
        self.boolean_op(c2::ClipType::Xor, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanAdd( a, b )`: the union of `a` and `b`, stored in self.
    pub fn boolean_add_sets(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_sets(c2::ClipType::Union, a, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanSubtract( a, b )`.
    pub fn boolean_subtract_sets(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_sets(c2::ClipType::Difference, a, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanIntersection( a, b )`.
    pub fn boolean_intersection_sets(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_sets(c2::ClipType::Intersection, a, b);
    }

    /// Port of `SHAPE_POLY_SET::BooleanXor( a, b )`.
    pub fn boolean_xor_sets(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_sets(c2::ClipType::Xor, a, b);
    }

    /// Port of `SHAPE_POLY_SET::InflateWithLinkedHoles()`: outline inflation/deflation of
    /// fractured polygons (unfracture, inflate, fracture).
    pub fn inflate_with_linked_holes(
        &mut self,
        a_factor: i32,
        a_corner_strategy: CornerStrategy,
        a_max_error: i32,
    ) {
        self.unfracture();
        self.inflate(a_factor, a_corner_strategy, a_max_error, false);
        self.fracture(true);
    }

    /// The `CORNER_STRATEGY` to Clipper2 join type / miter limit mapping of `inflate2()` and
    /// `inflateLine2()`.
    ///
    /// N.B. see the Clipper documentation for jtSquare/jtMiter/jtRound.  They are poorly named
    /// and are not what you'd think they are.
    /// http://www.angusj.com/delphi/clipper/documentation/Docs/Units/ClipperLib/Types/JoinType.htm
    fn join_type_for(a_corner_strategy: CornerStrategy) -> (c2::JoinType, f64) {
        match a_corner_strategy {
            // Allows large spikes
            CornerStrategy::AllowAcuteCorners => (c2::JoinType::Miter, 10.0),
            // Acute angles are chamfered
            CornerStrategy::ChamferAcuteCorners => (c2::JoinType::Miter, 2.0),
            // Acute angles are rounded
            CornerStrategy::RoundAcuteCorners => (c2::JoinType::Miter, 2.0),
            // All angles are chamfered.
            CornerStrategy::ChamferAllCorners => (c2::JoinType::Square, 2.0),
            // All angles are rounded.
            CornerStrategy::RoundAllCorners => (c2::JoinType::Round, 2.0),
        }
    }

    /// The arc tolerance coefficient `1.0 - cos( M_PI / aCircleSegCount )` of `inflate2()` /
    /// `inflateLine2()` (the C++ caches it in a thread-local table; the value is the same).
    ///
    /// Calculate the arc tolerance (arc error) from the seg count by circle. The seg count is
    /// nn = M_PI / acos(1.0 - c.ArcTolerance / abs(aAmount))
    /// http://www.angusj.com/delphi/clipper/documentation/Docs/Units/ClipperLib/Classes/ClipperOffset/Properties/ArcTolerance.htm
    fn arc_tolerance_coeff(a_circle_seg_count: i32) -> f64 {
        // avoid incorrect aCircleSegCount values
        let seg_count = a_circle_seg_count.max(6);

        1.0 - cmath::cos(std::f64::consts::PI / seg_count as f64)
    }

    /// The common end of `inflate2()` / `inflateLine2()`: execute the offset, optionally
    /// followed by a union with fill rule Positive (`aSimplify`).
    fn execute_offset(
        c: &mut c2::ClipperOffset,
        a_amount: i32,
        a_simplify: bool,
    ) -> c2::PolyTree64 {
        if a_simplify {
            let paths = c.execute(a_amount as f64).unwrap_or_default();

            // Clipper2Lib::SimplifyPaths( paths, std::abs( aAmount ) * coeff, ... ) is called
            // here in C++, but its result is discarded: it is a no-op.

            let mut c2c = c2::Clipper64::new();
            c2c.set_preserve_collinear(false);
            c2c.set_reverse_solution(false);
            c2c.add_subject(&paths);
            c2c.execute_tree(c2::ClipType::Union, c2::FillRule::Positive)
                .map(|(tree, _)| tree)
                .unwrap_or_default()
        } else {
            c.execute_tree(a_amount as f64).unwrap_or_default()
        }
    }

    /// Port of `SHAPE_POLY_SET::inflate2()`.
    fn inflate2(
        &mut self,
        a_amount: i32,
        a_circle_seg_count: i32,
        a_corner_strategy: CornerStrategy,
        a_simplify: bool,
    ) {
        let mut c = c2::ClipperOffset::default();

        let (join_type, miter_limit) = Self::join_type_for(a_corner_strategy);

        let mut z_values: Vec<ClipperZValue> = Vec::new();
        let mut arc_buffer: Vec<ShapeArc> = Vec::new();

        for poly in &self.polys {
            let mut paths: c2::Paths64 = Vec::new();

            for (i, chain) in poly.iter().enumerate() {
                paths.push(to_path64(chain, i == 0, &mut z_values, &mut arc_buffer));
            }

            c.add_paths(&paths, join_type, c2::EndType::Polygon);
        }

        let coeff = Self::arc_tolerance_coeff(a_circle_seg_count);

        // std::abs( int ): INT_MIN stays INT_MIN on x86
        c.set_arc_tolerance(a_amount.wrapping_abs() as f64 * coeff);
        c.set_miter_limit(miter_limit);

        let tree = Self::execute_offset(&mut c, a_amount, a_simplify);

        self.import_tree(&tree, &z_values, &arc_buffer);
    }

    /// Port of `SHAPE_POLY_SET::inflateLine2()`.
    fn inflate_line2(
        &mut self,
        a_line: &ShapeLineChain,
        a_amount: i32,
        a_circle_seg_count: i32,
        a_corner_strategy: CornerStrategy,
        a_simplify: bool,
    ) {
        let mut c = c2::ClipperOffset::default();

        let (join_type, miter_limit) = Self::join_type_for(a_corner_strategy);

        let mut z_values: Vec<ClipperZValue> = Vec::new();
        let mut arc_buffer: Vec<ShapeArc> = Vec::new();

        let path = to_path64(a_line, true, &mut z_values, &mut arc_buffer);
        c.add_path(&path, join_type, c2::EndType::Butt);

        let coeff = Self::arc_tolerance_coeff(a_circle_seg_count);

        c.set_arc_tolerance(a_amount.wrapping_abs() as f64 * coeff);
        c.set_miter_limit(miter_limit);

        let tree = Self::execute_offset(&mut c, a_amount, a_simplify);

        self.import_tree(&tree, &z_values, &arc_buffer);
    }

    /// Port of `SHAPE_POLY_SET::Inflate()`: outline inflation (positive `a_amount`) or
    /// deflation.
    ///
    /// Polygons can have holes, but not linked holes with main outlines, if aFactor < 0.  For
    /// those use `inflate_with_linked_holes()` to avoid odd corners where the link segments
    /// meet the outline.
    pub fn inflate(
        &mut self,
        a_amount: i32,
        a_corner_strategy: CornerStrategy,
        a_max_error: i32,
        a_simplify: bool,
    ) {
        let seg_count = get_arc_to_segment_count(a_amount.wrapping_abs(), a_max_error, FULL_CIRCLE);

        self.inflate2(a_amount, seg_count, a_corner_strategy, a_simplify);
    }

    /// Port of `SHAPE_POLY_SET::Deflate()`: `Inflate( -aAmount, ... )`.
    pub fn deflate(&mut self, a_amount: i32, a_corner_strategy: CornerStrategy, a_max_error: i32) {
        self.inflate(
            a_amount.wrapping_neg(),
            a_corner_strategy,
            a_max_error,
            false,
        );
    }

    /// Port of `SHAPE_POLY_SET::OffsetLineChain()`: replaces this set with the offset of the
    /// line chain `a_line` (butt ends).
    pub fn offset_line_chain(
        &mut self,
        a_line: &ShapeLineChain,
        a_amount: i32,
        a_corner_strategy: CornerStrategy,
        a_max_error: i32,
        a_simplify: bool,
    ) {
        let seg_count = get_arc_to_segment_count(a_amount.wrapping_abs(), a_max_error, FULL_CIRCLE);

        self.inflate_line2(a_line, a_amount, seg_count, a_corner_strategy, a_simplify);
    }

    /// Port of `SHAPE_POLY_SET::importPolyPath()`: a polygon from an outer node and its
    /// children (holes); the grandchildren are imported (recursively) before it, as in C++.
    fn import_poly_path(
        &mut self,
        a_tree: &c2::PolyTree64,
        a_node: usize,
        a_z_value_buffer: &[ClipperZValue],
        a_arc_buffer: &[ShapeArc],
    ) {
        if !a_tree.is_hole(a_node) {
            let node = a_tree.node(a_node);
            let mut paths: Polygon = Vec::with_capacity(node.children.len() + 1);
            paths.push(ShapeLineChain::from_clipper2(
                &node.polygon,
                a_z_value_buffer,
                a_arc_buffer,
            ));

            for &child in &node.children {
                let child_node = a_tree.node(child);
                paths.push(ShapeLineChain::from_clipper2(
                    &child_node.polygon,
                    a_z_value_buffer,
                    a_arc_buffer,
                ));

                for &grandchild in &child_node.children {
                    self.import_poly_path(a_tree, grandchild, a_z_value_buffer, a_arc_buffer);
                }
            }

            self.polys.push(paths);
        }
    }

    /// Port of `SHAPE_POLY_SET::importTree()`: replaces the polygons with a Clipper2 solution.
    fn import_tree(
        &mut self,
        a_tree: &c2::PolyTree64,
        a_z_value_buffer: &[ClipperZValue],
        a_arc_buffer: &[ShapeArc],
    ) {
        self.polys.clear();

        for &n in a_tree.roots() {
            self.import_poly_path(a_tree, n, a_z_value_buffer, a_arc_buffer);
        }
    }

    /// Port of `SHAPE_POLY_SET::importPaths()` (unused by KiCad): replaces the polygons with
    /// Clipper2 paths, a positive-area path starting a new polygon, the others being its holes.
    pub fn import_paths(
        &mut self,
        a_path: &[c2::Path64],
        a_z_value_buffer: &[ClipperZValue],
        a_arc_buffer: &[ShapeArc],
    ) {
        self.polys.clear();
        let mut path: Polygon = Vec::new();

        for n in a_path {
            if c2::area(n) > 0.0 {
                if !path.is_empty() {
                    self.polys.push(path.clone());
                }

                path.clear();
            } else if path.is_empty() {
                // wxCHECK2_MSG: "Cannot add a hole before an outline"
                continue;
            }

            path.push(ShapeLineChain::from_clipper2(
                n,
                a_z_value_buffer,
                a_arc_buffer,
            ));
        }

        if !path.is_empty() {
            self.polys.push(path);
        }
    }

    /// Port of `SHAPE_POLY_SET::fractureSingle()` (the cache-friendly variant, which is the
    /// default `ADVANCED_CFG` setting).
    fn fracture_single(paths: &mut Polygon) {
        fracture_single_cache_friendly(paths);
    }

    /// Port of `SHAPE_POLY_SET::Fracture()`: convert a set of polygons with holes to a single
    /// outline with "slits"/"fractures" connecting the outer ring to the inner holes.
    ///
    /// With `a_simplify` (the C++ default) `Simplify()` runs first to remove overlapping holes
    /// and degenerate geometry.
    pub fn fracture(&mut self, a_simplify: bool) {
        if a_simplify {
            self.simplify(); // remove overlapping holes/degeneracy
        }

        for paths in &mut self.polys {
            Self::fracture_single(paths);
        }
    }

    /// Port of `SHAPE_POLY_SET::unfractureSingle()`.
    fn unfracture_single(a_poly: &mut Polygon) {
        // C++: assert( aPoly.size() == 1 ), compiled out in release builds; a polygon whose
        // fracture failed still has its holes here.

        let mut lc = a_poly[0].clone();
        lc.simplify(0);

        // Rings are rebuilt from bare vertices, so arcs never survive unfracturing
        lc = ShapeLineChain::from_points(lc.c_points(), true);

        let mut rings: Vec<ShapeLineChain> = Vec::new();

        if !split_at_bridges(&lc, &mut rings) || rings.is_empty() {
            a_poly[0] = lc;
            return;
        }

        // std::max_element: the first ring with the largest absolute area
        let mut outline = 0;

        for i in 1..rings.len() {
            if rings[outline].area(true).abs() < rings[i].area(true).abs() {
                outline = i;
            }
        }

        rings.swap(0, outline);
        *a_poly = rings;
    }

    /// Port of `SHAPE_POLY_SET::HasHoles()`.
    pub fn has_holes(&self) -> bool {
        // Iterate through all the polygons on the set; if any of them has more than one
        // contour, it is a hole.
        self.polys.iter().any(|paths| paths.len() > 1)
    }

    /// Port of `SHAPE_POLY_SET::Unfracture()`: convert single outline slitted ("fractured")
    /// polygons into a set of outlines with holes.
    pub fn unfracture(&mut self) {
        for path in &mut self.polys {
            Self::unfracture_single(path);
        }

        self.simplify(); // remove overlapping holes/degeneracy
    }

    /// Port of `SHAPE_POLY_SET::isExteriorWaist()`: whether the overlapping part of two
    /// (collinear) segments has no polygon material on either side.
    ///
    /// `a_point_inside` stands for `this->PointInside( pt, 0, false )`: the plain
    /// [`Shape::point_inside`] of the set, or [`Self::point_inside_with_extents`], which gives
    /// the same answer.
    fn is_exterior_waist<F>(&self, a_seg_a: &Seg, a_seg_b: &Seg, mut a_point_inside: F) -> bool
    where
        F: FnMut(Vector2I) -> bool,
    {
        let da = a_seg_a.b - a_seg_a.a;

        let axis = if da.x.abs() >= da.y.abs() { 0 } else { 1 };

        let mut pts = [a_seg_a.a, a_seg_a.b, a_seg_b.a, a_seg_b.b];

        // Equal elements are identical points, so the sort algorithm does not matter.
        pts.sort_by(|p, q| {
            if axis == 0 {
                (p.x, p.y).cmp(&(q.x, q.y))
            } else {
                (p.y, p.x).cmp(&(q.y, q.x))
            }
        });

        let s = pts[1];
        let e = pts[2];

        // The `int` sums and differences below wrap in the reference build for coordinates
        // near the `int` limits (qa/data/pcbnew/issue25089/odb_circle_edge_cases reaches this
        // through Simplify; the reference reports no sliver there), so they wrap here too.
        let wadd = |p: Vector2I, q: Vector2I| {
            Vector2I::new(p.x.wrapping_add(q.x), p.y.wrapping_add(q.y))
        };
        let wsub = |p: Vector2I, q: Vector2I| {
            Vector2I::new(p.x.wrapping_sub(q.x), p.y.wrapping_sub(q.y))
        };

        // Check if there is polygon material on either side of the overlapping segments
        // Get the midpoint between s and e for testing
        let midpoint = wadd(s, e) / 2;

        // Create perpendicular offset vector to check both sides
        let seg_dir = wsub(e, s);

        if seg_dir.euclidean_norm() > 25 {
            let perp = seg_dir.perpendicular().resize(10);

            // Both sides must be outside for this to be an exterior waist. Short-circuit if
            // either side is inside the polygon to avoid the second O(N) point-in-polygon test.
            if !a_point_inside(wadd(midpoint, perp)) && !a_point_inside(wsub(midpoint, perp)) {
                return true;
            }
        }

        false
    }

    /// Fills `a_extents` with one [`OutlineExtent`] per outline (`polys[k][0]`), in outline
    /// order.
    fn build_outline_extents(&self, a_extents: &mut Vec<OutlineExtent>) {
        a_extents.clear();
        a_extents.extend(self.polys.iter().map(|paths| OutlineExtent::of(paths[0].c_points())));
    }

    /// `PointInside( a_pt, 0, false )` of the set (any outline's ray-cast parity, holes not
    /// considered), skipping outlines whose extent proves that their parity is even.
    ///
    /// `a_extents` must describe the current outlines (see [`Self::build_outline_extents`]).
    /// The outlines are tested in the same order and with the unchanged
    /// `SHAPE_LINE_CHAIN::PointInside()`; a skipped outline is one for which that call returns
    /// false (accuracy 0, so the result is the crossing parity alone):
    ///
    /// * an edge toggles only if `(p1.y >= pt.y) != (p2.y >= pt.y)`.  With `pt.y <= min_y`
    ///   every term is true, with `pt.y > max_y` every term is false: nothing toggles, whatever
    ///   x is, even with wrapping `int` arithmetic.
    /// * without wrap-around (all coordinates and `pt` within +-(2^30 - 1)) a straddling edge
    ///   toggles iff `pt.x < p1.x + d`, and `rescale` keeps `p1.x + d` in
    ///   `[min(p1.x, p2.x), max(p1.x, p2.x)]`.  So `pt.x >= max_x` toggles no edge and
    ///   `pt.x < min_x` toggles every straddling edge, of which a closed ring has an even
    ///   number.  Both give false.
    fn point_inside_with_extents(&self, a_pt: Vector2I, a_extents: &[OutlineExtent]) -> bool {
        let pt_in_bound = OutlineExtent::in_bound(a_pt);

        self.polys.iter().zip(a_extents).any(|(paths, ext)| {
            if a_pt.y <= ext.min.y || a_pt.y > ext.max.y {
                return false;
            }

            if ext.in_bound && pt_in_bound && (a_pt.x < ext.min.x || a_pt.x >= ext.max.x) {
                return false;
            }

            paths[0].point_inside(a_pt, 0, false)
        })
    }

    /// Port of `SHAPE_POLY_SET::splitCollinearOutlines()`: split outlines at exterior waists
    /// (collinear overlapping edges with no material on either side).
    ///
    /// Two shortcuts that leave the result unchanged (see `split_collinear_tests`):
    ///
    /// * outlines of at most [`SPLIT_COLLINEAR_PRECHECK_MAX`] points first test every segment
    ///   pair with the visitor's order-free conditions (index skips, closed-interval overlap of
    ///   the endpoint boxes - exactly the `SEGMENT_INDEX` candidate set of a zero-padding
    ///   query -, shared-endpoint skips, `ApproxCollinear`).  The visitor can only set `found`
    ///   for such a pair, so when there is none the outline is left untouched without building
    ///   the index; otherwise the r-tree path below runs unchanged and picks the same pair.
    /// * `isExteriorWaist()`'s `PointInside` calls use [`Self::point_inside_with_extents`].
    ///   The extent table is built on the first call and must be dropped whenever an outline
    ///   is replaced or appended.
    fn split_collinear_outlines(&mut self) {
        let mut poly_idx = 0;

        // Endpoint boxes (min x, min y, max x, max y) of the current outline's segments, for
        // the small-outline pre-check; reused across outlines.
        let mut boxes: Vec<[i32; 4]> = Vec::new();

        // Per-outline extents for point_inside_with_extents(); empty = not built yet (the set
        // has at least one outline while this runs, so a built table is never empty).
        let mut extents: Vec<OutlineExtent> = Vec::new();

        while poly_idx < self.polys.len() {
            loop {
                let outline = &self.polys[poly_idx][0];
                let count = outline.point_count() as i64;

                if count <= SPLIT_COLLINEAR_PRECHECK_MAX
                    && !Self::has_collinear_candidate(outline, &mut boxes)
                {
                    break;
                }

                let mut segs: Vec<Seg> = Vec::with_capacity(count as usize);

                for i in 0..count {
                    segs.push(Seg::new(
                        outline.c_point(i as i32),
                        outline.c_point(((i + 1) % count) as i32),
                    ));
                }

                let index = SegmentIndex::new(segs);

                let mut found = false;
                let mut seg_a: i64 = -1;
                let mut seg_b: i64 = -1;

                let mut i: i64 = 0;

                while i < count && !found {
                    let a = outline.c_point(i as i32);
                    let b = outline.c_point(((i + 1) % count) as i32);
                    let seg = Seg::new(a, b);

                    index.visit_candidates(&seg, 0, |j: i32| {
                        let j = j as i64;

                        if j == i || j == ((i + 1) % count) || j == ((i + count - 1) % count) {
                            return true;
                        }

                        let oa = outline.c_point(j as i32);
                        let ob = outline.c_point(((j + 1) % count) as i32);
                        let other = Seg::new(oa, ob);

                        // Skip segments that share start/end points.  This is the case for
                        // fractured segments
                        if oa == a && ob == b {
                            return true;
                        }

                        if oa == b && ob == a {
                            return true;
                        }

                        if seg.approx_collinear_with_threshold(&other, 10)
                            && self.is_exterior_waist(&seg, &other, |pt| {
                                if extents.is_empty() {
                                    self.build_outline_extents(&mut extents);
                                }

                                self.point_inside_with_extents(pt, &extents)
                            })
                        {
                            seg_a = i;
                            seg_b = j;
                            found = true;
                            return false;
                        }

                        true
                    });

                    i += 1;
                }

                if !found {
                    break;
                }

                let n = outline.point_count() as i64;
                let a0 = seg_a;
                let a1 = (seg_a + 1) % n;
                let b0 = seg_b;
                let b1 = (seg_b + 1) % n;

                let mut lc1 = ShapeLineChain::new();
                let mut idx = a1;
                lc1.append_point(outline.c_point(idx as i32), false);

                while idx != b0 {
                    idx = (idx + 1) % n;
                    lc1.append_point(outline.c_point(idx as i32), false);
                }

                lc1.set_closed(true);

                let mut lc2 = ShapeLineChain::new();
                idx = b1;
                lc2.append_point(outline.c_point(idx as i32), false);

                while idx != a0 {
                    idx = (idx + 1) % n;
                    lc2.append_point(outline.c_point(idx as i32), false);
                }

                lc2.set_closed(true);

                // The outlines change: the extent table no longer describes them.
                extents.clear();

                self.polys[poly_idx][0] = lc1;
                self.polys.push(vec![lc2]);
            }

            poly_idx += 1;
        }
    }

    /// Pre-check of [`Self::split_collinear_outlines`]: whether any ordered segment pair
    /// `(i, j)` of `a_outline` passes the visitor's tests other than `isExteriorWaist()`.
    ///
    /// Mirrors the visitor literally: `j` is not `i` or a neighbour of `i` (modulo the point
    /// count), the endpoint boxes overlap as closed intervals on both axes (the candidate set
    /// of `SEGMENT_INDEX::VisitCandidates( seg, 0 )`: the index stores the endpoint min / max
    /// and the packed r-tree reports every item with `!(b_min > q_max || b_max < q_min)`), the
    /// segments do not share both endpoints, and `ApproxCollinear( other, 10 )` holds.
    fn has_collinear_candidate(a_outline: &ShapeLineChain, a_boxes: &mut Vec<[i32; 4]>) -> bool {
        let pts = a_outline.c_points();
        let count = pts.len();

        a_boxes.clear();
        a_boxes.extend((0..count).map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % count];
            [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]
        }));

        for i in 0..count {
            let bi = a_boxes[i];
            let a = pts[i];
            let b = pts[(i + 1) % count];
            let next = (i + 1) % count;
            let prev = (i + count - 1) % count;

            for j in 0..count {
                if j == i || j == next || j == prev {
                    continue;
                }

                let bj = a_boxes[j];

                if bj[0] > bi[2] || bj[2] < bi[0] || bj[1] > bi[3] || bj[3] < bi[1] {
                    continue;
                }

                let oa = pts[j];
                let ob = pts[(j + 1) % count];

                if (oa == a && ob == b) || (oa == b && ob == a) {
                    continue;
                }

                if Seg::new(a, b).approx_collinear_with_threshold(&Seg::new(oa, ob), 10) {
                    return true;
                }
            }
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::splitSelfTouchingOutlines()`: split outline segments at
    /// vertices that lie on them (self-touching polygons).
    ///
    /// This handles the case where a polygon vertex lies on a non-adjacent segment, creating a
    /// "pinch point" where the polygon touches itself.  (Private in C++, used by the
    /// triangulation, package G4b.)
    pub fn split_self_touching_outlines(&mut self) {
        let mut poly_idx = 0;

        while poly_idx < self.polys.len() {
            loop {
                let outline = &self.polys[poly_idx][0];
                let count = outline.point_count();

                if count < 4 {
                    break;
                }

                // Prefix trapezoid terms (SHAPE_LINE_CHAIN::Area()) to sign a candidate's
                // halves in O(1).  Built on demand; most outlines never produce a candidate.
                let prefix: RefCell<Vec<f64>> = RefCell::new(Vec::new());

                let ring_term = |a_from: i32, a_to: i32| -> f64 {
                    let mut prefix = prefix.borrow_mut();

                    if prefix.is_empty() {
                        prefix.resize(count as usize + 1, 0.0);

                        for ii in 0..count {
                            let a = outline.c_point(ii);
                            let b = outline.c_point((ii + 1) % count);

                            prefix[ii as usize + 1] = prefix[ii as usize]
                                + (a.x as f64 + b.x as f64) * (a.y as f64 - b.y as f64);
                        }
                    }

                    let term = if a_from <= a_to {
                        prefix[a_to as usize] - prefix[a_from as usize]
                    } else {
                        prefix[count as usize] - prefix[a_from as usize] + prefix[a_to as usize]
                    };

                    let last = outline.c_point(a_to);
                    let first = outline.c_point(a_from);

                    term + (last.x as f64 + first.x as f64) * (last.y as f64 - first.y as f64)
                };

                let ring_size = |a_from: i32, a_to: i32| -> i32 {
                    if a_from <= a_to {
                        a_to - a_from + 1
                    } else {
                        count - a_from + a_to + 1
                    }
                };

                // Both halves of a real self-touch keep the parent winding.  An inverted half
                // is a Fracture() corridor; splitting there returns the hole ring as an
                // outline.
                let splittable = |a_vert_idx: i32, a_seg_idx: i32| -> bool {
                    let start = (a_seg_idx + 1) % count;

                    if ring_size(start, a_vert_idx) < 3 || ring_size(a_vert_idx, a_seg_idx) < 3 {
                        return false;
                    }

                    let sign = ring_term(0, count - 1) > 0.0;
                    let term1 = ring_term(start, a_vert_idx);
                    let term2 = ring_term(a_vert_idx, a_seg_idx);

                    term1 != 0.0 && term2 != 0.0 && (term1 > 0.0) == sign && (term2 > 0.0) == sign
                };

                let mut insert_seg_idx: i32 = -1;
                let mut insert_vert_idx: i32 = -1;

                // SquaredDistance returns 0 only when pt lies exactly on the segment.
                // Clipper2 rounds corridor-cut vertices to integer coordinates; they can land
                // within 1nm of an endpoint but are not true pinch points.
                let is_pinch = |vert_idx: i32, seg_idx: i32| -> bool {
                    let pt = outline.c_point(vert_idx);
                    let a = outline.c_point(seg_idx);
                    let b = outline.c_point((seg_idx + 1) % count);

                    pt != a
                        && pt != b
                        && Seg::new(a, b).squared_distance(pt) == 0
                        && splittable(vert_idx, seg_idx)
                };

                if count < RTREE_THRESHOLD {
                    let mut vert_idx = 0;

                    while vert_idx < count && insert_seg_idx < 0 {
                        let prev_seg = (vert_idx + count - 1) % count;

                        for seg_idx in 0..count {
                            // Skip adjacent segments
                            if seg_idx == prev_seg || seg_idx == vert_idx {
                                continue;
                            }

                            if is_pinch(vert_idx, seg_idx) {
                                insert_seg_idx = seg_idx;
                                insert_vert_idx = vert_idx;
                                break;
                            }
                        }

                        vert_idx += 1;
                    }
                } else {
                    let mut segs: Vec<Seg> = Vec::with_capacity(count as usize);

                    for i in 0..count {
                        segs.push(Seg::new(
                            outline.c_point(i),
                            outline.c_point((i + 1) % count),
                        ));
                    }

                    let index = SegmentIndex::new(segs);

                    let mut vert_idx = 0;

                    while vert_idx < count && insert_seg_idx < 0 {
                        let pt = outline.c_point(vert_idx);
                        let prev_seg = (vert_idx + count - 1) % count;

                        index.visit_candidates(&Seg::new(pt, pt), 0, |seg_idx: i32| {
                            if seg_idx == prev_seg || seg_idx == vert_idx {
                                return true;
                            }

                            if is_pinch(vert_idx, seg_idx) {
                                insert_seg_idx = seg_idx;
                                insert_vert_idx = vert_idx;
                                return false;
                            }

                            true
                        });

                        vert_idx += 1;
                    }
                }

                if insert_seg_idx < 0 {
                    break;
                }

                // Split the polygon at the pinch point into two separate polygons.
                // Polygon 1: vertices from (insertSegIdx+1) to insertVertIdx
                // Polygon 2: vertices from insertVertIdx to insertSegIdx

                let split_start1 = (insert_seg_idx + 1) % count;
                let size1 = ring_size(split_start1, insert_vert_idx);
                let size2 = ring_size(insert_vert_idx, insert_seg_idx);

                let mut poly1 = ShapeLineChain::new();
                let mut poly2 = ShapeLineChain::new();
                poly1.reserve_points(size1 as usize);
                poly2.reserve_points(size2 as usize);

                let mut idx = split_start1;

                for _ in 0..size1 {
                    poly1.append_point(outline.c_point(idx), false);
                    idx = (idx + 1) % count;
                }

                poly1.set_closed(true);

                idx = insert_vert_idx;

                for _ in 0..size2 {
                    poly2.append_point(outline.c_point(idx), false);
                    idx = (idx + 1) % count;
                }

                poly2.set_closed(true);

                self.polys[poly_idx][0] = poly1;
                self.polys.push(vec![poly2]);
            }

            poly_idx += 1;
        }
    }

    /// Port of `SHAPE_POLY_SET::Simplify()`: merges overlapping polygons, eliminates
    /// degeneracy / self-intersections (a union with an empty set, after splitting outlines
    /// at exterior waists).
    pub fn simplify(&mut self) {
        self.split_collinear_outlines();

        let empty = ShapePolySet::new();

        self.boolean_op(c2::ClipType::Union, &empty);
    }

    /// Port of `SHAPE_POLY_SET::SimplifyOutlines()`: `SHAPE_LINE_CHAIN::Simplify( aTolerance )`
    /// on every contour.
    pub fn simplify_outlines(&mut self, a_tolerance: i32) {
        for paths in &mut self.polys {
            for path in paths {
                path.simplify(a_tolerance);
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::NormalizeAreaOutlines()`: convert a self-intersecting polygon
    /// to one (or more) non self-intersecting polygon(s) and remove null segments.  Returns
    /// the polygon count.
    ///
    /// The C++ reads `Polygon( 0 )` of an empty set (undefined behaviour); the port returns 0.
    pub fn normalize_area_outlines(&mut self) -> i32 {
        if self.polys.is_empty() {
            return 0;
        }

        // We are expecting only one main outline, but this main outline can have holes
        // if holes: combine holes and remove them from the main outline.
        // Note also we are usingin polygon
        // calculations, but it is not mandatory. It is used mainly
        // because there is usually only very few vertices in area outlines
        let mut holes_buffer = ShapePolySet::new();

        // Move holes stored in outline to holesBuffer:
        // The first SHAPE_LINE_CHAIN is the main outline, others are holes
        while self.polys[0].len() > 1 {
            let hole = self.polys[0].pop().unwrap();
            holes_buffer.add_outline(&hole);
        }

        self.simplify();

        // If any hole, subtract it to main outline
        if holes_buffer.outline_count() != 0 {
            holes_buffer.simplify();
            self.boolean_subtract(&holes_buffer);
        }

        // In degenerate cases, simplify might return no outlines
        if self.outline_count() > 0 {
            self.remove_null_segments();
        }

        self.outline_count()
    }

    /// Port of `SHAPE_POLY_SET::Parse()`: reads `polyset <n> { poly <m> { <k> { <x> <y> } } }`
    /// (the counts and coordinates are read as strings and converted with `atoi`).
    pub fn parse(&mut self, a_stream: &mut StringStream<'_>) -> bool {
        // `aStream >> tmp`: a failed extraction leaves the string unchanged
        let mut tmp = String::new();
        let read = |a_stream: &mut StringStream<'_>, tmp: &mut String| {
            if let Some(t) = a_stream.read_token() {
                *tmp = t.to_string();
            }
        };

        read(a_stream, &mut tmp);

        if tmp != "polyset" {
            return false;
        }

        read(a_stream, &mut tmp);

        let n_polys = atoi(&tmp);

        if n_polys < 0 {
            return false;
        }

        for _ in 0..n_polys {
            let mut paths: Polygon = Vec::new();

            read(a_stream, &mut tmp);

            if tmp != "poly" {
                return false;
            }

            read(a_stream, &mut tmp);
            let n_outlines = atoi(&tmp);

            if n_outlines < 0 {
                return false;
            }

            for _ in 0..n_outlines {
                let mut outline = ShapeLineChain::new();

                outline.set_closed(true);

                read(a_stream, &mut tmp);
                let n_vertices = atoi(&tmp);

                for _ in 0..n_vertices {
                    let mut p = Vector2I::default();

                    read(a_stream, &mut tmp);
                    p.x = atoi(&tmp);
                    read(a_stream, &mut tmp);
                    p.y = atoi(&tmp);
                    outline.append_point(p, false);
                }

                paths.push(outline);
            }

            self.polys.push(paths);
        }

        true
    }

    /// Port of `SHAPE_POLY_SET::BBoxFromCaches()`: the bounding box from the outlines' cached
    /// boxes (see `build_bbox_caches()`).
    pub fn bbox_from_caches(&self) -> Box2I {
        let mut bb = Box2I::default();

        for (i, poly) in self.polys.iter().enumerate() {
            let cached = poly[0].get_cached_bbox().unwrap_or_default();

            if i == 0 {
                bb = cached;
            } else {
                bb.merge(&cached);
            }
        }

        bb
    }

    /// Port of `SHAPE_POLY_SET::PointOnEdge()`: whether `a_p` lies on an edge or vertex of
    /// some of the outlines or holes.
    pub fn point_on_edge(&self, a_p: Vector2I, a_accuracy: i32) -> bool {
        // Iterate through all the polygons in the set; iterate through all the line chains in
        // the polygon
        self.polys.iter().any(|polygon| {
            polygon
                .iter()
                .any(|chain| chain.point_on_edge(a_p, a_accuracy))
        })
    }

    /// Port of `SHAPE_POLY_SET::Collide( const SHAPE* aShape, int aClearance, int* aActual,
    /// VECTOR2I* aLocation )`.
    ///
    /// Segments and circles are handled directly.  Every other shape is tested against the
    /// cached triangulation (`AnyTriangleHit`).
    pub fn collide_shape(
        &self,
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        // A couple of simple cases are worth trying before we fall back on triangulation.
        match a_shape.as_shape_ref() {
            ShapeRef::Segment(segment) => {
                let extra = segment.get_width() / 2;
                let mut actual = 0;
                let want_actual = a_actual.is_some();

                if self.collide_seg(
                    segment.get_seg(),
                    a_clearance + extra,
                    if want_actual { Some(&mut actual) } else { None },
                    a_location,
                ) {
                    if let Some(a) = a_actual {
                        *a = 0.max(actual - extra);
                    }

                    return true;
                }

                false
            }
            ShapeRef::Circle(circle) => {
                let extra = circle.get_radius();
                let mut actual = 0;
                let want_actual = a_actual.is_some();

                if self.collide_point(
                    circle.get_center(),
                    a_clearance + extra,
                    if want_actual { Some(&mut actual) } else { None },
                    a_location,
                ) {
                    if let Some(a) = a_actual {
                        *a = 0.max(actual - extra);
                    }

                    return true;
                }

                false
            }
            _ => {
                // C++: const_cast<SHAPE_POLY_SET*>( this )->CacheTriangulation( false, {}, true )
                // followed by AnyTriangleHit( m_triIndex, ... ).  A `&self` method cannot
                // cache, so the triangulation that call would leave behind is used (built
                // into a copy when stale); see `collide_shape_mut()` for the caching variant.
                let up_to_date =
                    self.triangulation_valid && self.hash_valid && self.hash == self.checksum();

                if up_to_date {
                    let built;
                    let index = match &self.tri_index {
                        Some(index) => Some(index),
                        None => {
                            built = Self::build_tri_index(&self.triangulated_polys);
                            built.as_ref()
                        }
                    };

                    return Self::collide_triangles(
                        index,
                        &self.triangulated_polys,
                        a_shape,
                        a_clearance,
                        a_actual,
                        a_location,
                    );
                }

                if self.failed_hash_valid && self.failed_hash == self.checksum() {
                    return Self::collide_triangles(
                        self.tri_index.as_ref(),
                        &self.triangulated_polys,
                        a_shape,
                        a_clearance,
                        a_actual,
                        a_location,
                    );
                }

                let mut copy = ShapePolySet {
                    polys: self.polys.clone(),
                    triangulated_polys: Vec::new(),
                    tri_index: None,
                    triangulation_valid: self.triangulation_valid,
                    hash: self.hash,
                    hash_valid: self.hash_valid,
                    failed_hash: self.failed_hash,
                    failed_hash_valid: self.failed_hash_valid,
                };

                copy.cache_triangulation_with(false, false, true);

                Self::collide_triangles(
                    copy.tri_index.as_ref(),
                    &copy.triangulated_polys,
                    a_shape,
                    a_clearance,
                    a_actual,
                    a_location,
                )
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::Collide( const SHAPE* aShape, ... )` that, like the C++,
    /// caches the triangulation (and its index) in the set: same results as
    /// [`collide_shape()`](Self::collide_shape), but repeated queries do not re-triangulate a
    /// set whose triangulation is stale.
    pub fn collide_shape_mut(
        &mut self,
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if matches!(
            a_shape.as_shape_ref(),
            ShapeRef::Segment(_) | ShapeRef::Circle(_)
        ) {
            return self.collide_shape(a_shape, a_clearance, a_actual, a_location);
        }

        self.cache_triangulation_with(false, false, true);

        Self::collide_triangles(
            self.tri_index.as_ref(),
            &self.triangulated_polys,
            a_shape,
            a_clearance,
            a_actual,
            a_location,
        )
    }

    /// The triangle-based tail of `Collide( const SHAPE* )`.
    fn collide_triangles(
        a_index: Option<&TriangulationIndex>,
        a_polys: &[TriangulatedPolygon],
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if a_actual.is_none() && a_location.is_none() {
            return any_triangle_hit(a_index, a_polys, a_shape, a_clearance, None, None);
        }

        let mut actual = 0;
        let mut location = Vector2I::default();

        if !any_triangle_hit(
            a_index,
            a_polys,
            a_shape,
            a_clearance,
            Some(&mut actual),
            Some(&mut location),
        ) {
            return false;
        }

        if let Some(a) = a_actual {
            *a = 0.max(actual);
        }

        if let Some(l) = a_location {
            *l = location;
        }

        true
    }

    /// The `buildIndexes` lambda of `cacheTriangulation()` as a function: the triangle index,
    /// or `None` below `RTREE_THRESHOLD` triangles.
    fn build_tri_index(a_polys: &[TriangulatedPolygon]) -> Option<TriangulationIndex> {
        let mut tri_count: i32 = 0;

        for tpoly in a_polys {
            tri_count = tri_count.wrapping_add(tpoly.triangle_count() as i32);
        }

        if tri_count < RTREE_THRESHOLD {
            return None;
        }

        Some(TriangulationIndex::new(a_polys))
    }

    /// The `buildIndexes` lambda of `cacheTriangulation()`.
    fn build_indexes(&mut self) {
        if self.tri_index.is_some() {
            return;
        }

        self.tri_index = Self::build_tri_index(&self.triangulated_polys);
    }

    /// Port of `SHAPE_POLY_SET::TriangulationIndex()`: the spatial index over the cached
    /// triangles, if one was built (`CacheTriangulation( ..., aBuildIndexes = true )` or
    /// `collide_shape_mut()` with at least `RTREE_THRESHOLD` triangles).
    pub fn triangulation_index(&self) -> Option<&TriangulationIndex> {
        self.tri_index.as_ref()
    }

    /// The triangulated polygons (`m_triangulatedPolys`), e.g. for `AnyTriangleHit()`.
    pub fn triangulated_polys(&self) -> &[TriangulatedPolygon] {
        &self.triangulated_polys
    }

    /// Port of `SHAPE_POLY_SET::RemoveAllContours()`.
    pub fn remove_all_contours(&mut self) {
        self.tri_index = None;

        self.polys.clear();
        self.triangulated_polys.clear();
        self.triangulation_valid = false;
    }

    /// Port of `SHAPE_POLY_SET::RemoveContour()`: delete the `a_contour_idx`-th contour of the
    /// `a_polygon_idx`-th polygon (negative: from the end, -1 is the last polygon).
    pub fn remove_contour(&mut self, a_contour_idx: i32, a_polygon_idx: i32) {
        let mut a_polygon_idx = a_polygon_idx;

        // Default polygon is the last one
        if a_polygon_idx < 0 {
            a_polygon_idx += self.polys.len() as i32;
        }

        self.polys[a_polygon_idx as usize].remove(a_contour_idx as usize);
    }

    /// Port of `SHAPE_POLY_SET::RemoveOutline()`: delete the `a_outline_idx`-th polygon
    /// including its holes.
    pub fn remove_outline(&mut self, a_outline_idx: i32) {
        self.polys.remove(a_outline_idx as usize);
    }

    /// Port of `SHAPE_POLY_SET::RemoveNullSegments()`: delete the start vertex of every
    /// segment whose ends are the same; returns the number of deleted segments.
    ///
    /// The C++ dereferences the first vertex of an empty set (undefined behaviour); the port
    /// returns 0.
    pub fn remove_null_segments(&mut self) -> i32 {
        let mut removed = 0;
        let mut indices_to_remove: Vec<VertexIndex> = Vec::new();

        {
            let mut iterator = self.iterate_with_holes();

            if !iterator.is_valid() {
                return 0;
            }

            let mut contour_start = iterator.get();

            while iterator.is_valid() {
                // Obtain first point and its index
                let segment_start = iterator.get();
                let index_start = iterator.get_index();
                let segment_end;

                // Obtain last point
                if iterator.is_end_contour() {
                    segment_end = contour_start;

                    // Advance
                    iterator.advance();

                    // If we have rolled into the next contour, remember its position
                    // segmentStart and segmentEnd remain valid for comparison here
                    if iterator.is_valid() {
                        contour_start = iterator.get();
                    }
                } else {
                    // Advance
                    iterator.advance();

                    // If we have reached the end of the SHAPE_POLY_SET, something is broken
                    // here (wxCHECK_MSG: "Invalid polygon.  Reached end without noticing.
                    // Please report this error")
                    if !iterator.is_valid() {
                        return removed;
                    }

                    segment_end = iterator.get();
                }

                // Remove segment start if both points are equal
                if segment_start == segment_end {
                    indices_to_remove.push(index_start);
                    removed += 1;
                }
            }
        }

        // Proceed in reverse direction to remove the vertices because they are stored as
        // absolute indices in a vector.  Removing in reverse order preserves the remaining
        // index values
        for index in indices_to_remove.iter().rev() {
            self.remove_vertex_index(*index);
        }

        removed
    }

    /// Port of `SHAPE_POLY_SET::DeletePolygon()`.
    pub fn delete_polygon(&mut self, a_idx: i32) {
        self.polys.remove(a_idx as usize);
    }

    /// Port of `SHAPE_POLY_SET::DeletePolygonAndTriangulationData()`: delete the `a_idx`-th
    /// polygon and its triangulation data.  If called with `a_update_hash` false, the caller
    /// must call `update_triangulation_data_hash()`.
    pub fn delete_polygon_and_triangulation_data(&mut self, a_idx: i32, a_update_hash: bool) {
        self.tri_index = None;

        self.polys.remove(a_idx as usize);

        if self.triangulation_valid {
            for ii in (0..self.triangulated_polys.len()).rev() {
                let source = self.triangulated_polys[ii].source_outline_index();

                if source == a_idx {
                    self.triangulated_polys.remove(ii);
                } else if source > a_idx {
                    self.triangulated_polys[ii].set_source_outline_index(source - 1);
                }
            }

            if a_update_hash {
                self.hash = self.checksum();
                self.hash_valid = true;
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::UpdateTriangulationDataHash()`.
    pub fn update_triangulation_data_hash(&mut self) {
        self.hash = self.checksum();
        self.hash_valid = true;
    }

    /// Port of `SHAPE_POLY_SET::CollideVertex()`: whether `a_point` lies within `a_clearance`
    /// of a vertex of any contour.
    ///
    /// Without `a_closest_vertex` the first hit returns true; otherwise all vertices are
    /// visited and the closest one (the first of equally close ones after the first hit
    /// within the clearance, as in C++) is stored.
    pub fn collide_vertex(
        &self,
        a_point: Vector2I,
        mut a_closest_vertex: Option<&mut VertexIndex>,
        a_clearance: i32,
    ) -> bool {
        // Shows whether there was a collision
        let mut collision = false;

        let mut clearance_squared: Ecoord = Seg::square(a_clearance);

        let mut iterator = self.c_iterate_with_holes();

        while iterator.is_valid() {
            // Get the difference vector between current vertex and aPoint
            let delta: Vector2D = (iterator.get() - a_point).cast();

            // Compute distance (VECTOR2D::SquaredEuclideanNorm() converted to ecoord)
            let distance_squared: Ecoord = cmath::f64_to_i64_x86(delta.squared_euclidean_norm());

            // Check for collisions
            if distance_squared <= clearance_squared {
                let Some(closest) = a_closest_vertex.as_deref_mut() else {
                    return true;
                };

                collision = true;

                // Update clearance to look for closer vertices
                clearance_squared = distance_squared;

                // Store the indices that identify the vertex
                *closest = iterator.get_index();
            }

            iterator.advance();
        }

        collision
    }

    /// Port of `SHAPE_POLY_SET::CollideEdge()`: whether `a_point` lies within `a_clearance` of
    /// an edge of any contour (see `collide_vertex()` for `a_closest_vertex`).
    pub fn collide_edge(
        &self,
        a_point: Vector2I,
        mut a_closest_vertex: Option<&mut VertexIndex>,
        a_clearance: i32,
    ) -> bool {
        // Shows whether there was a collision
        let mut collision = false;
        let mut clearance_squared: Ecoord = Seg::square(a_clearance);

        let mut iterator = self.c_iterate_segments_with_holes();

        while iterator.is_valid() {
            let current_segment = iterator.get();
            let distance_squared = current_segment.squared_distance(a_point);

            // Check for collisions
            if distance_squared <= clearance_squared {
                let Some(closest) = a_closest_vertex.as_deref_mut() else {
                    return true;
                };

                collision = true;

                // Update clearance to look for closer edges
                clearance_squared = distance_squared;

                // Store the indices that identify the vertex
                *closest = iterator.get_index();
            }

            iterator.advance();
        }

        collision
    }

    /// Port of `SHAPE_POLY_SET::BuildBBoxCaches()`: construct the bounding box caches for
    /// `contains()`.  They must be built before a group of calls to `contains()` with
    /// `a_use_bbox_caches`, and are not kept up to date by editing actions.  (A `const` method
    /// writing `mutable` caches in C++, hence `&mut self`.)
    pub fn build_bbox_caches(&mut self) {
        for polygon in &mut self.polys {
            for chain in polygon {
                chain.generate_bbox_cache();
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::Contains()`: whether the given polygon (`a_subpoly_index` >= 0)
    /// or any polygon (-1) contains `a_p`.  Points on an edge are contained.
    pub fn contains(
        &self,
        a_p: Vector2I,
        a_subpoly_index: i32,
        a_accuracy: i32,
        a_use_bbox_caches: bool,
    ) -> bool {
        if self.polys.is_empty() {
            return false;
        }

        // If there is a polygon specified, check the condition against that polygon
        if a_subpoly_index >= 0 {
            return self.contains_single(a_p, a_subpoly_index, a_accuracy, a_use_bbox_caches);
        }

        // In any other case, check it against all polygons in the set
        (0..self.outline_count())
            .any(|idx| self.contains_single(a_p, idx, a_accuracy, a_use_bbox_caches))
    }

    /// Port of `SHAPE_POLY_SET::RemoveVertex( int aGlobalIndex )`.
    ///
    /// # Panics
    ///
    /// If the vertex does not exist (C++ throws `std::out_of_range`).
    pub fn remove_vertex(&mut self, a_global_index: i32) {
        // Assure the to be removed vertex exists, abort otherwise
        let index = self
            .get_relative_indices(a_global_index)
            .expect("aGlobalIndex-th vertex does not exist");
        self.remove_vertex_index(index);
    }

    /// Port of `SHAPE_POLY_SET::RemoveVertex( VERTEX_INDEX aRelativeIndices )`.
    pub fn remove_vertex_index(&mut self, a_index: VertexIndex) {
        self.polys[a_index.polygon as usize][a_index.contour as usize].remove(a_index.vertex);
    }

    /// Port of `SHAPE_POLY_SET::SetVertex( int aGlobalIndex, const VECTOR2I& aPos )`.
    ///
    /// # Panics
    ///
    /// If the vertex does not exist (C++ throws `std::out_of_range`).
    pub fn set_vertex(&mut self, a_global_index: i32, a_pos: Vector2I) {
        let index = self
            .get_relative_indices(a_global_index)
            .expect("aGlobalIndex-th vertex does not exist");
        self.set_vertex_index(&index, a_pos);
    }

    /// Port of `SHAPE_POLY_SET::SetVertex( const VERTEX_INDEX& aIndex, const VECTOR2I& aPos )`.
    pub fn set_vertex_index(&mut self, a_index: &VertexIndex, a_pos: Vector2I) {
        self.polys[a_index.polygon as usize][a_index.contour as usize]
            .set_point(a_index.vertex, a_pos);
    }

    /// Port of `SHAPE_POLY_SET::containsSingle()`: whether `a_p` is inside (or on an edge of)
    /// the `a_subpoly_index`-th polygon.
    fn contains_single(
        &self,
        a_p: Vector2I,
        a_subpoly_index: i32,
        a_accuracy: i32,
        a_use_bbox_caches: bool,
    ) -> bool {
        let polygon = &self.polys[a_subpoly_index as usize];

        // Check that the point is inside the outline
        if polygon[0].point_inside(a_p, a_accuracy, false) {
            // Check that the point is not in any of the holes
            for hole_idx in 0..self.hole_count(a_subpoly_index) {
                let hole = self.c_hole(a_subpoly_index, hole_idx);

                // If the point is inside a hole it is outside of the polygon.  Do not use
                // aAccuracy here as it's meaning would be inverted.
                if hole.point_inside(a_p, 1, a_use_bbox_caches) {
                    return false;
                }
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::Mirror( const VECTOR2I& aRef, FLIP_DIRECTION )`.
    pub fn mirror(&mut self, a_ref: Vector2I, a_flip_direction: FlipDirection) {
        for poly in &mut self.polys {
            for path in poly {
                path.mirror(a_ref, a_flip_direction);
            }
        }

        if self.triangulation_valid {
            self.cache_triangulation(false);
        }
    }

    /// Port of `SHAPE_POLY_SET::Mirror( const SEG& aAxis )`.
    pub fn mirror_seg(&mut self, a_axis: &Seg) {
        for poly in &mut self.polys {
            for path in poly {
                path.mirror_seg(a_axis);
            }
        }

        if self.triangulation_valid {
            self.cache_triangulation(false);
        }
    }

    /// Port of `SHAPE_POLY_SET::TotalVertices()`: the number of vertices in all contours.
    pub fn total_vertices(&self) -> i32 {
        let mut c: i32 = 0;

        for poly in &self.polys {
            for path in poly {
                c += path.point_count();
            }
        }

        c
    }

    /// Port of `SHAPE_POLY_SET::ChamferPolygon()`: a chamfered version of the `a_index`-th
    /// polygon (removes null segments from the whole set first).
    pub fn chamfer_polygon(&mut self, a_distance: u32, a_index: i32) -> Polygon {
        self.chamfer_fillet_polygon(CornerMode::Chamfered, a_distance, a_index, 0)
    }

    /// Port of `SHAPE_POLY_SET::FilletPolygon()`: a filleted version of the `a_index`-th
    /// polygon.
    pub fn fillet_polygon(&mut self, a_radius: u32, a_error_max: i32, a_index: i32) -> Polygon {
        self.chamfer_fillet_polygon(CornerMode::Filleted, a_radius, a_index, a_error_max)
    }

    /// Port of `SHAPE_POLY_SET::SquaredDistanceToPolygon( VECTOR2I aPoint, int aIndex,
    /// VECTOR2I* aNearest )`: the squared distance from `a_point` to the `a_polygon_index`-th
    /// polygon (0 inside).
    pub fn squared_distance_to_polygon(
        &self,
        a_point: Vector2I,
        a_polygon_index: i32,
        mut a_nearest: Option<&mut Vector2I>,
    ) -> Ecoord {
        // We calculate the min dist between the segment and each outline segment.  However,
        // if the segment to test is inside the outline, and does not cross any edge, it can be
        // seen outside the polygon.  Therefore test if a segment end is inside (testing only
        // one end is enough).  Use an accuracy of "1" to say that we don't care if it's
        // exactly on the edge or not.
        if self.contains_single(a_point, a_polygon_index, 1, false) {
            if let Some(n) = a_nearest {
                *n = a_point;
            }

            return 0;
        }

        let mut iterator = self.c_iterate_segments_with_holes_outline(a_polygon_index);

        let mut min_distance = iterator.get().squared_distance(a_point);

        if let Some(n) = a_nearest.as_deref_mut() {
            *n = iterator.get().nearest_point(a_point);
        }

        iterator.advance();

        while iterator.is_valid() && min_distance > 0 {
            let current_distance = iterator.get().squared_distance(a_point);

            if current_distance < min_distance {
                if let Some(n) = a_nearest.as_deref_mut() {
                    *n = iterator.get().nearest_point(a_point);
                }

                min_distance = current_distance;
            }

            iterator.advance();
        }

        min_distance
    }

    /// Port of `SHAPE_POLY_SET::SquaredDistanceToPolygon( const SEG& aSegment, int aIndex,
    /// VECTOR2I* aNearest )`: the squared distance from `a_segment` to the
    /// `a_polygon_index`-th polygon (0 if it is fully contained or crosses an edge).
    pub fn squared_distance_to_polygon_seg(
        &self,
        a_segment: &Seg,
        a_polygon_index: i32,
        mut a_nearest: Option<&mut Vector2I>,
    ) -> Ecoord {
        // Check if the segment is fully-contained.  If so, its midpoint is a good-enough
        // nearest point.
        if self.contains_single(a_segment.a, a_polygon_index, 1, false)
            && self.contains_single(a_segment.b, a_polygon_index, 1, false)
        {
            if let Some(n) = a_nearest {
                // ( aSegment.A + aSegment.B ) / 2: int addition, wraps in the reference build
                // beyond 2^30 nm (FIX-FAB-3, `coll_far` oracle section)
                *n = Vector2I::new(
                    a_segment.a.x.wrapping_add(a_segment.b.x),
                    a_segment.a.y.wrapping_add(a_segment.b.y),
                ) / 2;
            }

            return 0;
        }

        let mut iterator = self.c_iterate_segments_with_holes_outline(a_polygon_index);
        let mut min_distance = iterator.get().squared_distance_seg(a_segment);

        if let Some(n) = a_nearest.as_deref_mut() {
            *n = iterator.get().nearest_point_seg(a_segment);
        }

        iterator.advance();

        while iterator.is_valid() && min_distance > 0 {
            let current_distance = iterator.get().squared_distance_seg(a_segment);

            if current_distance < min_distance {
                if let Some(n) = a_nearest.as_deref_mut() {
                    *n = iterator.get().nearest_point_seg(a_segment);
                }

                min_distance = current_distance;
            }

            iterator.advance();
        }

        // Return the maximum of minDistance and zero
        if min_distance < 0 { 0 } else { min_distance }
    }

    /// Port of `SHAPE_POLY_SET::SquaredDistance( const VECTOR2I& aPoint, bool aOutlineOnly,
    /// VECTOR2I* aNearest )`: the minimum squared distance between `a_point` and all the
    /// polygons (0 inside).  `a_outline_only` is not supported (a `wxASSERT` in C++) and
    /// ignored.
    pub fn squared_distance_nearest(
        &self,
        a_point: Vector2I,
        _a_outline_only: bool,
        mut a_nearest: Option<&mut Vector2I>,
    ) -> Ecoord {
        let mut min_distance_sq: Ecoord = Vector2I::ECOORD_MAX;
        let mut nearest = Vector2I::default();
        let want = a_nearest.is_some();

        // Iterate through all the polygons and get the minimum distance.
        for polygon_idx in 0..self.polys.len() {
            let current_distance_sq = self.squared_distance_to_polygon(
                a_point,
                polygon_idx as i32,
                if want { Some(&mut nearest) } else { None },
            );

            if current_distance_sq < min_distance_sq {
                if let Some(n) = a_nearest.as_deref_mut() {
                    *n = nearest;
                }

                min_distance_sq = current_distance_sq;
            }
        }

        min_distance_sq
    }

    /// Port of `SHAPE_POLY_SET::SquaredDistanceToSeg()`: the minimum squared distance between
    /// `a_segment` and all the polygons (0 if it is contained or crosses an edge).
    pub fn squared_distance_to_seg(
        &self,
        a_segment: &Seg,
        mut a_nearest: Option<&mut Vector2I>,
    ) -> Ecoord {
        let mut min_distance_sq: Ecoord = Vector2I::ECOORD_MAX;
        let mut nearest = Vector2I::default();
        let want = a_nearest.is_some();

        // Iterate through all the polygons and get the minimum distance.
        for polygon_idx in 0..self.polys.len() {
            let current_distance_sq = self.squared_distance_to_polygon_seg(
                a_segment,
                polygon_idx as i32,
                if want { Some(&mut nearest) } else { None },
            );

            if current_distance_sq < min_distance_sq {
                if let Some(n) = a_nearest.as_deref_mut() {
                    *n = nearest;
                }

                min_distance_sq = current_distance_sq;
            }
        }

        min_distance_sq
    }

    /// Port of `SHAPE_POLY_SET::IsVertexInHole()`: whether the `a_global_idx`-th vertex belongs
    /// to a hole (false if it does not exist).
    pub fn is_vertex_in_hole(&self, a_global_idx: i32) -> bool {
        // Get the polygon and contour where the vertex is. If the vertex does not exist,
        // return false; the contour is a hole if its index is greater than zero
        self.get_relative_indices(a_global_idx)
            .is_some_and(|index| index.contour > 0)
    }

    /// Port of `SHAPE_POLY_SET::Chamfer()`: a chamfered version of the set.
    pub fn chamfer(&mut self, a_distance: i32) -> ShapePolySet {
        let mut chamfered = ShapePolySet::new();

        for idx in 0..self.polys.len() {
            let poly = self.chamfer_polygon(a_distance as u32, idx as i32);
            chamfered.polys.push(poly);
        }

        chamfered
    }

    /// Port of `SHAPE_POLY_SET::Fillet()`: a filleted version of the set.
    pub fn fillet(&mut self, a_radius: i32, a_error_max: i32) -> ShapePolySet {
        let mut filleted = ShapePolySet::new();

        for idx in 0..self.polys.len() {
            let poly = self.fillet_polygon(a_radius as u32, a_error_max, idx as i32);
            filleted.polys.push(poly);
        }

        filleted
    }

    /// Port of `SHAPE_POLY_SET::GetHash()`.
    pub fn get_hash(&self) -> Hash128 {
        if !self.hash_valid {
            return self.checksum();
        }

        self.hash
    }

    /// Port of `SHAPE_POLY_SET::IsTriangulationUpToDate()`.
    pub fn is_triangulation_up_to_date(&self) -> bool {
        if !self.triangulation_valid {
            return false;
        }

        if !self.hash_valid {
            return false;
        }

        self.checksum() == self.hash
    }

    /// Port of `SHAPE_POLY_SET::CacheTriangulation( aSimplify )`: build (or keep, if it is up
    /// to date) the triangulation of the set, without a task submitter and without the
    /// triangle index.
    pub fn cache_triangulation(&mut self, a_simplify: bool) {
        self.cache_triangulation_hint(a_simplify, None, false, false);
    }

    /// Port of `SHAPE_POLY_SET::CacheTriangulation( aSimplify, aSubmitter, aBuildIndexes )`.
    ///
    /// `a_has_submitter` stands for a non-empty `TASK_SUBMITTER`: the C++ then triangulates
    /// the partition leaves of a huge outline as separate tasks and keeps their results in
    /// leaf order, which is what the port does sequentially (the result does not depend on
    /// the threads).  Without a submitter the leaves are queued as outlines instead, which can
    /// give a different triangulation.
    pub fn cache_triangulation_with(
        &mut self,
        a_simplify: bool,
        a_has_submitter: bool,
        a_build_indexes: bool,
    ) {
        self.cache_triangulation_hint(a_simplify, None, a_has_submitter, a_build_indexes);
    }

    /// Port of the protected `SHAPE_POLY_SET::cacheTriangulation( aSimplify, aHintData,
    /// aSubmitter, aBuildIndexes )` (`a_has_submitter`: see `cache_triangulation_with()`).
    pub fn cache_triangulation_hint(
        &mut self,
        a_simplify: bool,
        a_hint_data: Option<&[TriangulatedPolygon]>,
        a_has_submitter: bool,
        a_build_indexes: bool,
    ) {
        if self.triangulation_valid && self.hash_valid && self.hash == self.checksum() {
            if a_build_indexes {
                self.build_indexes();
            }

            return;
        }

        if self.failed_hash_valid && self.failed_hash == self.checksum() {
            return;
        }

        // Invalidate, in case anything goes wrong below
        self.tri_index = None;
        self.triangulation_valid = false;
        self.hash_valid = false;
        self.failed_hash_valid = false;

        self.triangulated_polys.clear();
        let mut tris: Vec<TriangulatedPolygon> = Vec::new();

        let tmp_set;
        let src_set: &ShapePolySet = if self.arc_count() > 0 || a_simplify {
            let mut t = self.clone();
            t.clear_arcs();

            if a_simplify {
                t.simplify();
            }

            tmp_set = t;
            &tmp_set
        } else {
            self
        };

        let mut direct_ok = true;
        let mut ii = 0;

        while ii < src_set.outline_count() && direct_ok {
            let poly = src_set.c_polygon(ii);
            let prev_count = tris.len();

            if poly.len() > 1 {
                tris.push(TriangulatedPolygon::new(ii));

                if PolygonTriangulation::new(tris.last_mut().unwrap()).tesselate_polygon(poly, None)
                {
                    ii += 1;
                    continue;
                }

                // Hole bridging failed; fracture to merge holes into the outline
                tris.truncate(prev_count);

                let mut flat_set = ShapePolySet::from_outline(&poly[0]);

                for hole in poly.iter().skip(1) {
                    flat_set.add_hole(hole, -1);
                }

                flat_set.fracture(true);
                flat_set.split_self_touching_outlines();

                if Self::triangulate(&mut flat_set, ii, &mut tris, None, a_has_submitter) {
                    ii += 1;
                    continue;
                }

                tris.truncate(prev_count);
                direct_ok = false;
                ii += 1;
                continue;
            }

            {
                let mut part_scratch = TriangulatedPolygon::new(-1);
                let part_checker = PolygonTriangulation::new(&mut part_scratch);

                if part_checker.suggested_partition_leaf_count(&poly[0]) > 1 {
                    let mut part_set = ShapePolySet::new();
                    part_set.add_outline(&poly[0]);

                    if Self::triangulate(&mut part_set, ii, &mut tris, None, a_has_submitter) {
                        ii += 1;
                        continue;
                    }

                    tris.truncate(prev_count);
                }
            }

            tris.push(TriangulatedPolygon::new(ii));

            let mut ok =
                PolygonTriangulation::new(tris.last_mut().unwrap()).tesselate_chain(&poly[0], None);

            // Self-touching outlines produce overlapping triangles; detect via area coverage
            if ok {
                let original_area = poly[0].area(true).abs();

                if original_area > 0.0 {
                    let mut tri_area = 0.0;

                    for tri in &tris.last().unwrap().triangles {
                        tri_area += tri.area().abs();
                    }

                    let coverage = tri_area / original_area;

                    if coverage > 1.01 || coverage < 0.99 {
                        ok = false;
                    }
                }
            }

            if !ok {
                tris.truncate(prev_count);

                let mut split_set = ShapePolySet::new();
                split_set.add_outline(&poly[0]);
                split_set.split_self_touching_outlines();

                let mut split_ok = true;
                let mut jj = 0;

                while jj < split_set.outline_count() && split_ok {
                    tris.push(TriangulatedPolygon::new(ii));
                    split_ok = PolygonTriangulation::new(tris.last_mut().unwrap())
                        .tesselate_chain(&split_set.c_polygon(jj)[0], None);
                    jj += 1;
                }

                if !split_ok {
                    direct_ok = false;
                }
            }

            ii += 1;
        }

        if tris.last().is_some_and(|t| t.triangle_count() == 0) {
            tris.pop();
        }

        if direct_ok && !tris.is_empty() {
            self.triangulated_polys = tris;
            self.hash = self.checksum();
            self.hash_valid = true;
            self.triangulation_valid = true;
        } else {
            // Fracture each outline individually to preserve source outline indices
            tris.clear();
            let mut fallback_ok = true;
            let mut ii = 0;

            while ii < src_set.outline_count() && fallback_ok {
                let poly = src_set.c_polygon(ii);
                let mut flat_set = ShapePolySet::from_outline(&poly[0]);

                for hole in poly.iter().skip(1) {
                    flat_set.add_hole(hole, -1);
                }

                flat_set.clear_arcs();
                flat_set.fracture(true);
                flat_set.split_self_touching_outlines();

                if !Self::triangulate(&mut flat_set, ii, &mut tris, None, a_has_submitter) {
                    fallback_ok = false;
                }

                ii += 1;
            }

            let mut valid = false;

            if !fallback_ok {
                // Last resort: flatten everything together (loses outline indices)
                tris.clear();
                let mut fallback_set = self.clone();
                fallback_set.clear_arcs();
                fallback_set.fracture(true);
                fallback_set.split_self_touching_outlines();

                // else: "Failed to triangulate polygon"
                if Self::triangulate(
                    &mut fallback_set,
                    -1,
                    &mut tris,
                    a_hint_data,
                    a_has_submitter,
                ) {
                    valid = true;
                }
            } else {
                valid = true;
            }

            self.triangulated_polys = tris;
            self.triangulation_valid = valid;

            if self.triangulation_valid {
                self.hash = self.checksum();
                self.hash_valid = true;
            } else {
                self.failed_hash = self.checksum();
                self.failed_hash_valid = true;
            }
        }

        if a_build_indexes && self.triangulation_valid {
            self.build_indexes();
        }
    }

    /// The `triangulate` lambda of `cacheTriangulation()`: triangulate the outlines of
    /// `poly_set` (consumed from the front) into `dest`, partitioning huge outlines,
    /// simplifying once on failure.  With `a_has_submitter` the partition leaves are
    /// triangulated as the C++ tasks would be (sequentially, in leaf order).
    fn triangulate(
        poly_set: &mut ShapePolySet,
        for_outline: i32,
        dest: &mut Vec<TriangulatedPolygon>,
        hint_data: Option<&[TriangulatedPolygon]>,
        a_has_submitter: bool,
    ) -> bool {
        let mut triangulation_valid = false;
        let mut pass = 0;
        let mut index = 0;
        let mut hint_data = hint_data;

        if hint_data.is_some_and(|h| h.len() != poly_set.outline_count() as usize) {
            hint_data = None;
        }

        while poly_set.outline_count() > 0 {
            if dest.last().is_some_and(|t| t.triangle_count() == 0) {
                dest.pop();
            }

            {
                let mut partition_scratch = TriangulatedPolygon::new(for_outline);
                let partitioner = PolygonTriangulation::new(&mut partition_scratch);
                let outline = &poly_set.c_polygon(0)[0];
                let partition_leaves = partitioner.suggested_partition_leaf_count(outline);

                if partition_leaves > 1 {
                    let partitions =
                        partitioner.partition_polygon_balanced(outline, partition_leaves);

                    if partitions.len() > 1 {
                        poly_set.delete_polygon(0);

                        if a_has_submitter && partitions.len() > 2 {
                            // The work-stealing tasks: every leaf is triangulated on its own
                            let mut results: Vec<TriangulatedPolygon> = partitions
                                .iter()
                                .map(|_| TriangulatedPolygon::new(for_outline))
                                .collect();
                            let mut all_ok = true;

                            for (result, leaf) in results.iter_mut().zip(partitions.iter()) {
                                let ok =
                                    PolygonTriangulation::new(result).tesselate_chain(leaf, None);
                                all_ok = all_ok && ok;
                            }

                            if all_ok {
                                for r in results {
                                    if r.triangle_count() > 0 {
                                        dest.push(r);
                                    }
                                }

                                triangulation_valid = true;
                                hint_data = None;
                                continue;
                            }
                        }

                        for part in partitions.iter().rev() {
                            poly_set.add_outline(part);
                        }

                        hint_data = None;
                        continue;
                    }
                }
            }

            dest.push(TriangulatedPolygon::new(for_outline));

            // If the tessellation fails, we re-fracture the polygon, which will
            // first simplify the system before fracturing and removing the holes
            // This may result in multiple, disjoint polygons.
            let ok = PolygonTriangulation::new(dest.last_mut().unwrap())
                .tesselate_chain(&poly_set.c_polygon(0)[0], hint_data.map(|h| &h[index]));

            if !ok {
                pass += 1;

                if pass == 1 {
                    poly_set.simplify_outlines(TRIANGULATE_SIMPLIFICATION_LEVEL);
                } else {
                    // In Clipper2, there is only one type of simplification
                    break;
                }

                triangulation_valid = false;
                hint_data = None;
                continue;
            }

            poly_set.delete_polygon(0);
            index += 1;
            triangulation_valid = true;
        }

        triangulation_valid
    }

    /// Port of `SHAPE_POLY_SET::checksum()`: MurmurHash3 of the polygon structure and the
    /// vertex coordinates.
    fn checksum(&self) -> Hash128 {
        let mut hash = Mmh3Hash::new(0x68AF835D); // Arbitrary seed

        // size_t values go through MMH3_HASH::add( int32_t )
        hash.add_i32(self.polys.len() as i32);

        for outline in &self.polys {
            hash.add_i32(outline.len() as i32);

            for lc in outline {
                hash.add_i32(lc.point_count());

                for pt in lc.c_points() {
                    hash.add_i32(pt.x);
                    hash.add_i32(pt.y);
                }
            }
        }

        hash.digest()
    }

    /// Port of `SHAPE_POLY_SET::HasTouchingHoles()`: whether any polygon has contours sharing a
    /// vertex.
    pub fn has_touching_holes(&self) -> bool {
        self.polys.iter().any(Self::has_touching_holes_poly)
    }

    /// Port of `SHAPE_POLY_SET::hasTouchingHoles( const POLYGON& )`.  The point key is the C++
    /// `(long long) pt.x << 32 | pt.y`, with the sign extension of `pt.y`.
    fn has_touching_holes_poly(a_poly: &Polygon) -> bool {
        let mut pt_hashes: BTreeSet<i64> = BTreeSet::new();

        for lc in a_poly {
            for pt in lc.c_points() {
                let pt_hash: i64 = ((pt.x as i64) << 32) | (pt.y as i64);

                if !pt_hashes.insert(pt_hash) {
                    return true;
                }
            }
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::TriangulatedPolyCount()`.
    pub fn triangulated_poly_count(&self) -> u32 {
        self.triangulated_polys.len() as u32
    }

    /// Port of `SHAPE_POLY_SET::TriangulatedPolygon()`.
    pub fn triangulated_polygon(&self, a_index: i32) -> &TriangulatedPolygon {
        &self.triangulated_polys[a_index as usize]
    }

    /// Port of `SHAPE_POLY_SET::Scale()`: scale all vertices about `a_center`.
    pub fn scale(&mut self, a_scale_factor_x: f64, a_scale_factor_y: f64, a_center: Vector2I) {
        for poly in &mut self.polys {
            for path in poly {
                for i in 0..path.point_count() {
                    let mut pt = path.c_point(i);
                    let vec = Vector2D::new(
                        (pt.x - a_center.x) as f64 * a_scale_factor_x,
                        (pt.y - a_center.y) as f64 * a_scale_factor_y,
                    );
                    pt.x = ki_round(a_center.x as f64 + vec.x);
                    pt.y = ki_round(a_center.y as f64 + vec.y);
                    path.set_point(i, pt);
                }
            }
        }

        if self.triangulation_valid {
            self.cache_triangulation(false);
        }
    }

    /// Port of `SHAPE_POLY_SET::BuildPolysetFromOrientedPaths()`: build the set from closed
    /// outlines given in random order (positive orientation = outline, negative = hole), with
    /// the NonZero (default) or EvenOdd fill rule.
    pub fn build_polyset_from_oriented_paths(
        &mut self,
        a_paths: &[ShapeLineChain],
        a_even_odd: bool,
    ) {
        let mut clipper = c2::Clipper64::new();
        let mut paths: c2::Paths64 = Vec::with_capacity(a_paths.len());

        for path in a_paths {
            let lc: c2::Path64 = path
                .c_points()
                .iter()
                .map(|p| c2::Point64::new(p.x as i64, p.y as i64))
                .collect();

            paths.push(lc);
        }

        clipper.add_subject(&paths);
        let tree = clipper
            .execute_tree(
                c2::ClipType::Union,
                if a_even_odd {
                    c2::FillRule::EvenOdd
                } else {
                    c2::FillRule::NonZero
                },
            )
            .map(|(tree, _)| tree)
            .unwrap_or_default();

        let z_values: Vec<ClipperZValue> = Vec::new();
        let arc_buffer: Vec<ShapeArc> = Vec::new();

        self.import_tree(&tree, &z_values, &arc_buffer);
    }

    /// Port of `SHAPE_POLY_SET::GenerateHatchLines()`: hatch lines with the given slopes and
    /// spacing inside the set (`a_line_length` -1: full lines, otherwise two short lines at
    /// the ends of long ones).
    pub fn generate_hatch_lines(
        &self,
        a_slopes: &[f64],
        a_spacing: i32,
        a_line_length: i32,
    ) -> Vec<Seg> {
        let mut hatch_lines: Vec<Seg> = Vec::new();

        if self.outline_count() == 0 || self.total_vertices() == 0 {
            return hatch_lines;
        }

        // define range for hatch lines
        let first = self.c_vertex_global(0);
        let mut min_x = first.x;
        let mut max_x = first.x;
        let mut min_y = first.y;
        let mut max_y = first.y;

        for p in self.c_iterate_with_holes() {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
        }

        for &slope_in in a_slopes {
            // The loop variable is a copy that the long-line branch below overwrites (C++).
            let mut slope = slope_in;
            let max_a: i64;
            let mut min_a: i64;

            if slope > 0.0 {
                max_a = ki_round_i64(max_y as f64 - slope * min_x as f64);
                min_a = ki_round_i64(min_y as f64 - slope * max_x as f64);
            } else {
                max_a = ki_round_i64(max_y as f64 - slope * max_x as f64);
                min_a = ki_round_i64(min_y as f64 - slope * min_x as f64);
            }

            min_a = (min_a / a_spacing as i64) * a_spacing as i64;

            // loop through hatch lines
            let mut pointbuffer: Vec<Vector2I> = Vec::with_capacity(256);

            let mut a = min_a;

            while a < max_a {
                pointbuffer.clear();

                // Iterate through all vertices
                for seg in self.c_iterate_segments_with_holes() {
                    if let Some(pt) = seg.intersects_line(slope, a as f64) {
                        // If the intersection point is outside the polygon, skip it
                        if pt.x < min_x || pt.x > max_x || pt.y < min_y || pt.y > max_y {
                            continue;
                        }

                        // Add the intersection point to the buffer
                        pointbuffer.push(pt);
                    }
                }

                // sort points in order of descending x (if more than 2) to
                // ensure the starting point and the ending point of the same segment
                // are stored one just after the other.
                if pointbuffer.len() > 2 {
                    std_sort(&mut pointbuffer, |r, t| t.x < r.x);
                }

                // creates lines or short segments inside the complex polygon
                let mut ip = 0;

                while ip + 1 < pointbuffer.len() {
                    let p1 = pointbuffer[ip];
                    let p2 = pointbuffer[ip + 1];
                    ip += 1;

                    // Avoid duplicated intersections or segments
                    if p1 == p2 {
                        continue;
                    }

                    let candidate = Seg::new(p1, p2);

                    // int additions: they wrap in the reference build for zones beyond 2^30 nm
                    // (the wrapped midpoint then lies outside the polygon and the line is
                    // dropped), so they wrap here too (FIX-FAB-3)
                    let mid = Vector2I::new(
                        candidate.a.x.wrapping_add(candidate.b.x) / 2,
                        candidate.a.y.wrapping_add(candidate.b.y) / 2,
                    );

                    // Check if segment is inside the polygon by checking its middle point
                    if self.contains(mid, -1, 1, true) {
                        let mut dx = p2.x - p1.x;

                        // Push only one line for diagonal hatch or for small lines < twice
                        // the line length; else push 2 small lines
                        if a_line_length == -1 || dx.abs() < 2 * a_line_length {
                            hatch_lines.push(candidate);
                        } else {
                            let dy = (p2.y - p1.y) as f64;
                            slope = dy / dx as f64;

                            if dx > 0 {
                                dx = a_line_length;
                            } else {
                                dx = -a_line_length;
                            }

                            let x1 = p1.x + dx;
                            let x2 = p2.x - dx;
                            let y1: i32 = ki_round(p1.y as f64 + dx as f64 * slope);
                            let y2: i32 = ki_round(p2.y as f64 - dx as f64 * slope);

                            hatch_lines.push(Seg::from_coords(p1.x, p1.y, x1, y1));

                            hatch_lines.push(Seg::from_coords(p2.x, p2.y, x2, y2));
                        }
                    }
                }

                a += a_spacing as i64;
            }
        }

        hatch_lines
    }

    /// Port of `SHAPE_POLY_SET::chamferFilletPolygon()` (in `corner_operations.cpp`): the
    /// chamfered or filleted version of the `a_index`-th polygon.  Null segments of the whole
    /// set are removed first.
    fn chamfer_fillet_polygon(
        &mut self,
        a_mode: CornerMode,
        a_distance: u32,
        a_index: i32,
        a_error_max: i32,
    ) -> Polygon {
        // Null segments create serious issues in calculations. Remove them:
        self.remove_null_segments();

        let current_poly: Polygon = self.polys[a_index as usize].clone();
        let mut new_poly: Polygon = Vec::new();

        // If the chamfering distance is zero, then the polygon remain intact.
        if a_distance == 0 {
            return current_poly;
        }

        // Iterate through all the contours (outline and holes) of the polygon.
        for curr_contour in &current_poly {
            // Generate a new contour in the new polygon
            let mut new_contour = ShapeLineChain::new();
            let point_count = curr_contour.point_count();

            // Iterate through the vertices of the contour
            for curr_vertex in 0..point_count {
                // Current vertex
                let x1 = curr_contour.c_point(curr_vertex).x;
                let y1 = curr_contour.c_point(curr_vertex).y;

                // Previous and next vertices indices computation. Necessary to manage the edge
                // cases.

                // Previous vertex is the last one if the current vertex is the first one
                let prev_vertex = if curr_vertex == 0 {
                    point_count - 1
                } else {
                    curr_vertex - 1
                };

                // next vertex is the first one if the current vertex is the last one.
                let next_vertex = if curr_vertex == point_count - 1 {
                    0
                } else {
                    curr_vertex + 1
                };

                // Previous vertex computation
                let xa = (curr_contour.c_point(prev_vertex).x - x1) as f64;
                let ya = (curr_contour.c_point(prev_vertex).y - y1) as f64;

                // Next vertex computation
                let xb = (curr_contour.c_point(next_vertex).x - x1) as f64;
                let yb = (curr_contour.c_point(next_vertex).y - y1) as f64;

                // Avoid segments that will generate nans below
                if (xa + xb).abs() < f64::EPSILON && (ya + yb).abs() < f64::EPSILON {
                    continue;
                }

                // Compute the new distances
                let lena = cmath::hypot(xa, ya);
                let lenb = cmath::hypot(xb, yb);

                // Make the final computations depending on the mode selected, chamfered or
                // filleted.
                if a_mode == CornerMode::Chamfered {
                    let mut distance = a_distance as f64;

                    // Chamfer one half of an edge at most
                    if 0.5 * lena < distance {
                        distance = 0.5 * lena;
                    }

                    if 0.5 * lenb < distance {
                        distance = 0.5 * lenb;
                    }

                    let nx1: i32 = ki_round(distance * xa / lena);
                    let ny1: i32 = ki_round(distance * ya / lena);

                    new_contour.append_xy(x1 + nx1, y1 + ny1, false);

                    let nx2: i32 = ki_round(distance * xb / lenb);
                    let ny2: i32 = ki_round(distance * yb / lenb);

                    new_contour.append_xy(x1 + nx2, y1 + ny2, false);
                } else {
                    // CORNER_MODE = FILLETED
                    let cosine = (xa * xb + ya * yb) / (lena * lenb);

                    let mut radius = a_distance as f64;
                    let denom = (2.0 / (1.0 + cosine) - 1.0).sqrt();

                    // Do nothing in case of parallel edges
                    if denom.is_infinite() {
                        continue;
                    }

                    // Limit rounding distance to one half of an edge
                    if 0.5 * lena * denom < radius {
                        radius = 0.5 * lena * denom;
                    }

                    if 0.5 * lenb * denom < radius {
                        radius = 0.5 * lenb * denom;
                    }

                    // Calculate fillet arc absolute center point (xc, yx)
                    let mut k = radius / (0.5 * (1.0 - cosine)).sqrt();
                    let lenab = ((xa / lena + xb / lenb) * (xa / lena + xb / lenb)
                        + (ya / lena + yb / lenb) * (ya / lena + yb / lenb))
                        .sqrt();
                    let xc = x1 as f64 + k * (xa / lena + xb / lenb) / lenab;
                    let yc = y1 as f64 + k * (ya / lena + yb / lenb) / lenab;

                    // Calculate arc start and end vectors
                    k = radius / (2.0 / (1.0 + cosine) - 1.0).sqrt();
                    let xs = x1 as f64 + k * xa / lena - xc;
                    let ys = y1 as f64 + k * ya / lena - yc;
                    let xe = x1 as f64 + k * xb / lenb - xc;
                    let ye = y1 as f64 + k * yb / lenb - yc;

                    // Cosine of arc angle
                    let mut argument = (xs * xe + ys * ye) / (radius * radius);

                    // Make sure the argument is in [-1,1], interval in which the acos function
                    // is defined
                    argument = argument.clamp(-1.0, 1.0);

                    let arc_angle = cmath::acos(argument);
                    // GetArcToSegmentCount( int aRadius, ... ): the double radius is truncated
                    let segments = get_arc_to_segment_count(
                        cmath::f64_to_i32_x86(radius),
                        a_error_max,
                        EdaAngle::new(arc_angle, RADIANS_T),
                    );

                    let mut delta_angle = arc_angle / segments as f64;
                    let start_angle = cmath::atan2(-ys, xs);

                    // Flip arc for inner corners
                    if xa * yb - ya * xb <= 0.0 {
                        delta_angle *= -1.0;
                    }

                    let mut nx = xc + xs;
                    let mut ny = yc + ys;

                    if nx.is_nan() || ny.is_nan() {
                        continue;
                    }

                    new_contour.append_xy(ki_round(nx), ki_round(ny), false);

                    // Store the previous added corner to make a sanity check
                    let mut prev_x: i32 = ki_round(nx);
                    let mut prev_y: i32 = ki_round(ny);

                    for j in 0..segments {
                        nx = xc + cmath::cos(start_angle + (j + 1) as f64 * delta_angle) * radius;
                        ny = yc - cmath::sin(start_angle + (j + 1) as f64 * delta_angle) * radius;

                        if nx.is_nan() || ny.is_nan() {
                            continue;
                        }

                        // Sanity check: the rounding can produce repeated corners; do not add
                        // them.
                        if ki_round(nx) != prev_x || ki_round(ny) != prev_y {
                            new_contour.append_xy(ki_round(nx), ki_round(ny), false);
                            prev_x = ki_round(nx);
                            prev_y = ki_round(ny);
                        }
                    }
                }
            }

            // Close the current contour and add it the new polygon
            new_contour.set_closed(true);
            new_poly.push(new_contour);
        }

        new_poly
    }

    /// Port of `SHAPE_POLY_SET::OutlineCount()`.
    pub fn outline_count(&self) -> i32 {
        self.polys.len() as i32
    }

    /// Port of `SHAPE_POLY_SET::HoleCount()`: the number of holes in a given outline (0 for an
    /// invalid index).
    pub fn hole_count(&self, a_outline: i32) -> i32 {
        if a_outline < 0
            || a_outline >= self.polys.len() as i32
            || self.polys[a_outline as usize].len() < 2
        {
            return 0;
        }

        // the first polygon in m_polys[aOutline] is the main contour,
        // only others are holes:
        self.polys[a_outline as usize].len() as i32 - 1
    }

    /// Port of `SHAPE_POLY_SET::Outline()`: the reference to the `a_index`-th outline.
    pub fn outline(&mut self, a_index: i32) -> &mut ShapeLineChain {
        &mut self.polys[a_index as usize][0]
    }

    /// Port of `SHAPE_POLY_SET::COutline()` (and the `const` `Outline()`).
    pub fn c_outline(&self, a_index: i32) -> &ShapeLineChain {
        &self.polys[a_index as usize][0]
    }

    /// Port of `SHAPE_POLY_SET::Hole()`: the `a_hole`-th hole of the `a_outline`-th outline.
    pub fn hole(&mut self, a_outline: i32, a_hole: i32) -> &mut ShapeLineChain {
        &mut self.polys[a_outline as usize][a_hole as usize + 1]
    }

    /// Port of `SHAPE_POLY_SET::CHole()`.
    pub fn c_hole(&self, a_outline: i32, a_hole: i32) -> &ShapeLineChain {
        &self.polys[a_outline as usize][a_hole as usize + 1]
    }

    /// Port of `SHAPE_POLY_SET::Polygon()`.
    pub fn polygon(&mut self, a_index: i32) -> &mut Polygon {
        &mut self.polys[a_index as usize]
    }

    /// Port of `SHAPE_POLY_SET::CPolygon()` (and the `const` `Polygon()`).
    pub fn c_polygon(&self, a_index: i32) -> &Polygon {
        &self.polys[a_index as usize]
    }

    /// Port of `SHAPE_POLY_SET::CPolygons()`.
    pub fn c_polygons(&self) -> &[Polygon] {
        &self.polys
    }

    /// Port of `SHAPE_POLY_SET::IsEmpty()`: whether the set has no polygons at all.
    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// Port of `SHAPE_POLY_SET::Iterate( int aFirst, int aLast, bool aIterateHoles )` (and
    /// `CIterate`): the vertices of the polygons `a_first` to `a_last` (negative: the last
    /// polygon of the set).
    pub fn iterate_range(
        &self,
        a_first: i32,
        a_last: i32,
        a_iterate_holes: bool,
    ) -> PolySetIterator<'_> {
        PolySetIterator {
            poly: self,
            current_polygon: a_first,
            last_polygon: if a_last < 0 {
                self.outline_count() - 1
            } else {
                a_last
            },
            current_contour: 0,
            current_vertex: 0,
            iterate_holes: a_iterate_holes,
        }
    }

    /// Port of `SHAPE_POLY_SET::Iterate( int aOutline )`: the vertices of the main outline of
    /// the `a_outline`-th polygon.
    pub fn iterate_outline(&self, a_outline: i32) -> PolySetIterator<'_> {
        self.iterate_range(a_outline, a_outline, false)
    }

    /// Port of `SHAPE_POLY_SET::IterateWithHoles( int aOutline )`.
    pub fn iterate_with_holes_outline(&self, a_outline: i32) -> PolySetIterator<'_> {
        self.iterate_range(a_outline, a_outline, true)
    }

    /// Port of `SHAPE_POLY_SET::Iterate()`: the vertices of all outlines (no holes).
    pub fn iterate(&self) -> PolySetIterator<'_> {
        self.iterate_range(0, self.outline_count() - 1, false)
    }

    /// Port of `SHAPE_POLY_SET::IterateWithHoles()`: all vertices.
    pub fn iterate_with_holes(&self) -> PolySetIterator<'_> {
        self.iterate_range(0, self.outline_count() - 1, true)
    }

    /// Port of `SHAPE_POLY_SET::CIterate( aFirst, aLast, aIterateHoles )`.
    pub fn c_iterate_range(
        &self,
        a_first: i32,
        a_last: i32,
        a_iterate_holes: bool,
    ) -> PolySetIterator<'_> {
        self.iterate_range(a_first, a_last, a_iterate_holes)
    }

    /// Port of `SHAPE_POLY_SET::CIterate( aOutline )`.
    pub fn c_iterate_outline(&self, a_outline: i32) -> PolySetIterator<'_> {
        self.iterate_outline(a_outline)
    }

    /// Port of `SHAPE_POLY_SET::CIterateWithHoles( aOutline )`.
    pub fn c_iterate_with_holes_outline(&self, a_outline: i32) -> PolySetIterator<'_> {
        self.iterate_with_holes_outline(a_outline)
    }

    /// Port of `SHAPE_POLY_SET::CIterate()`.
    pub fn c_iterate(&self) -> PolySetIterator<'_> {
        self.iterate()
    }

    /// Port of `SHAPE_POLY_SET::CIterateWithHoles()`.
    pub fn c_iterate_with_holes(&self) -> PolySetIterator<'_> {
        self.iterate_with_holes()
    }

    /// Port of `SHAPE_POLY_SET::IterateFromVertexWithHoles()`: an iterator over all vertices
    /// starting at the `a_global_idx`-th one.
    ///
    /// # Panics
    ///
    /// If the vertex does not exist (C++ throws `std::out_of_range`).
    pub fn iterate_from_vertex_with_holes(&self, a_global_idx: i32) -> PolySetIterator<'_> {
        // Build iterator
        let mut iter = self.iterate_with_holes();

        // Get the relative indices of the globally indexed vertex
        let indices = self
            .get_relative_indices(a_global_idx)
            .expect("aGlobalIndex-th vertex does not exist");

        // Adjust where the iterator is pointing
        iter.current_polygon = indices.polygon;
        iter.current_contour = indices.contour;
        iter.current_vertex = indices.vertex;

        iter
    }

    /// Port of `SHAPE_POLY_SET::IterateSegments( int aFirst, int aLast, bool aIterateHoles )`
    /// (and `CIterateSegments`).
    pub fn iterate_segments_range(
        &self,
        a_first: i32,
        a_last: i32,
        a_iterate_holes: bool,
    ) -> SegmentIterator<'_> {
        SegmentIterator {
            poly: self,
            current_polygon: a_first,
            last_polygon: if a_last < 0 {
                self.outline_count() - 1
            } else {
                a_last
            },
            current_contour: 0,
            current_segment: 0,
            iterate_holes: a_iterate_holes,
        }
    }

    /// Port of `SHAPE_POLY_SET::IterateSegments( int aPolygonIdx )`: the outline edges of the
    /// `a_polygon_idx`-th polygon.
    pub fn iterate_segments_polygon(&self, a_polygon_idx: i32) -> SegmentIterator<'_> {
        self.iterate_segments_range(a_polygon_idx, a_polygon_idx, false)
    }

    /// Port of `SHAPE_POLY_SET::IterateSegments()`: the edges of all outlines (no holes).
    pub fn iterate_segments(&self) -> SegmentIterator<'_> {
        self.iterate_segments_range(0, self.outline_count() - 1, false)
    }

    /// Port of `SHAPE_POLY_SET::IterateSegmentsWithHoles()`: all edges.
    pub fn iterate_segments_with_holes(&self) -> SegmentIterator<'_> {
        self.iterate_segments_range(0, self.outline_count() - 1, true)
    }

    /// Port of `SHAPE_POLY_SET::IterateSegmentsWithHoles( int aOutline )`: the edges of the
    /// `a_outline`-th polygon, holes included.
    pub fn iterate_segments_with_holes_outline(&self, a_outline: i32) -> SegmentIterator<'_> {
        self.iterate_segments_range(a_outline, a_outline, true)
    }

    /// Port of `SHAPE_POLY_SET::CIterateSegments( aFirst, aLast, aIterateHoles )`.
    pub fn c_iterate_segments_range(
        &self,
        a_first: i32,
        a_last: i32,
        a_iterate_holes: bool,
    ) -> SegmentIterator<'_> {
        self.iterate_segments_range(a_first, a_last, a_iterate_holes)
    }

    /// Port of `SHAPE_POLY_SET::CIterateSegments( aPolygonIdx )`.
    pub fn c_iterate_segments_polygon(&self, a_polygon_idx: i32) -> SegmentIterator<'_> {
        self.iterate_segments_polygon(a_polygon_idx)
    }

    /// Port of `SHAPE_POLY_SET::CIterateSegments()`.
    pub fn c_iterate_segments(&self) -> SegmentIterator<'_> {
        self.iterate_segments()
    }

    /// Port of `SHAPE_POLY_SET::CIterateSegmentsWithHoles()`.
    pub fn c_iterate_segments_with_holes(&self) -> SegmentIterator<'_> {
        self.iterate_segments_with_holes()
    }

    /// Port of `SHAPE_POLY_SET::CIterateSegmentsWithHoles( aOutline )`.
    pub fn c_iterate_segments_with_holes_outline(&self, a_outline: i32) -> SegmentIterator<'_> {
        self.iterate_segments_with_holes_outline(a_outline)
    }
}

/// `atoi()` as glibc implements it (`(int) strtol( s, NULL, 10 )`): leading white space, an
/// optional sign, decimal digits; `strtol` saturates at the `long` range and the conversion to
/// `int` keeps the low 32 bits.
fn atoi(a_text: &str) -> i32 {
    let bytes = a_text.as_bytes();
    let mut i = 0;

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }

    let mut negative = false;

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    let mut value: i128 = 0;

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = (value * 10 + (bytes[i] - b'0') as i128).min(i64::MAX as i128 + 1);
        i += 1;
    }

    if negative {
        value = -value;
    }

    let clamped = value.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    clamped as i32
}

impl ShapeLineChain {
    /// Port of `SHAPE_LINE_CHAIN::OffsetLine()`: the left and right lines of the offset of
    /// this chain (`None` where the C++ returns false).  Lives here because it needs
    /// `SHAPE_POLY_SET::OffsetLineChain()`; the rest is `offset_line_from_poly()`.
    pub fn offset_line(
        &self,
        a_amount: i32,
        a_corner_strategy: CornerStrategy,
        a_max_error: i32,
        a_simplify: bool,
    ) -> Option<(ShapeLineChain, ShapeLineChain)> {
        if self.point_count() < 2 {
            return None;
        }

        let mut poly = ShapePolySet::new();
        poly.offset_line_chain(self, a_amount, a_corner_strategy, a_max_error, a_simplify);

        self.offset_line_from_poly(&poly)
    }
}

impl Shape for ShapePolySet {
    fn type_(&self) -> ShapeType {
        ShapeType::PolySet
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::PolySet(self)
    }

    /// Port of `SHAPE_POLY_SET::Clone()`.
    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    /// Port of `SHAPE_POLY_SET::HasIndexableSubshapes()`.
    fn has_indexable_subshapes(&self) -> bool {
        self.is_triangulation_up_to_date()
    }

    /// Port of `SHAPE_POLY_SET::GetIndexableSubshapeCount()`: the number of triangles.
    fn indexable_subshape_count(&self) -> usize {
        self.triangulated_polys
            .iter()
            .map(|t| t.triangles.len())
            .sum()
    }

    /// Port of `SHAPE_POLY_SET::GetIndexableSubshapes()`: the triangles.
    fn indexable_subshapes(&self) -> Vec<&dyn Shape> {
        let mut subshapes: Vec<&dyn Shape> = Vec::with_capacity(self.indexable_subshape_count());

        for tpoly in &self.triangulated_polys {
            for tri in &tpoly.triangles {
                subshapes.push(tri);
            }
        }

        subshapes
    }

    /// The `a_index`-th triangle in [`Shape::indexable_subshapes`] order.
    fn indexable_subshape(&self, mut a_index: usize) -> Option<&dyn Shape> {
        for tpoly in &self.triangulated_polys {
            if a_index < tpoly.triangles.len() {
                return Some(&tpoly.triangles[a_index]);
            }

            a_index -= tpoly.triangles.len();
        }

        None
    }

    /// Port of `SHAPE_POLY_SET::Collide( const VECTOR2I& aP, ... )`: whether `a_p` is inside
    /// or on the edge of the set, or closer than `a_clearance` to it.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.is_empty() || self.vertex_count(-1, -1) == 0 {
            return false;
        }

        let mut nearest = Vector2I::default();
        let want = a_location.is_some();
        let dist_sq =
            self.squared_distance_nearest(a_p, false, if want { Some(&mut nearest) } else { None });

        if dist_sq == 0 || dist_sq < Seg::square(a_clearance) {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = cmath::f64_to_i32_x86((dist_sq as f64).sqrt());
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::Collide( const SEG& aSeg, ... )`: whether `a_seg` collides
    /// with the set (or its edge).
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        let mut nearest = Vector2I::default();
        let want = a_location.is_some();
        let dist_sq =
            self.squared_distance_to_seg(a_seg, if want { Some(&mut nearest) } else { None });

        if dist_sq == 0 || dist_sq < Seg::square(a_clearance) {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = cmath::f64_to_i32_x86((dist_sq as f64).sqrt());
            }

            return true;
        }

        false
    }

    /// Port of `SHAPE_POLY_SET::Collide( const SHAPE* aShape, ... )`, see `collide_shape()`.
    fn collide(
        &self,
        a_shape: &dyn Shape,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        self.collide_shape(a_shape, a_clearance, a_actual, a_location)
    }

    /// Port of `SHAPE_POLY_SET::BBox()`: the bounding box of the outlines.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let mut bb = Box2I::default();

        for (i, poly) in self.polys.iter().enumerate() {
            if i == 0 {
                bb = poly[0].bbox(0);
            } else {
                bb.merge(&poly[0].bbox(0));
            }
        }

        bb.inflate(a_clearance);
        bb
    }

    /// Port of `SHAPE_POLY_SET::SquaredDistance( const VECTOR2I&, bool aOutlineOnly )`.
    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        self.squared_distance_nearest(a_p, a_outline_only, None)
    }

    /// Port of `SHAPE_POLY_SET::PointInside()`: whether `a_pt` is inside any outline (holes are
    /// not considered).
    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        (0..self.outline_count()).any(|idx| {
            self.c_outline(idx)
                .point_inside(a_pt, a_accuracy, a_use_bbox_cache)
        })
    }

    /// Port of `SHAPE_POLY_SET::TransformToPolygon()`: appends the polygons to the buffer.
    fn transform_to_polygon(
        &self,
        a_buffer: &mut ShapePolySet,
        _a_error: i32,
        _a_error_loc: ErrorLoc,
    ) {
        a_buffer.append_set(self);
    }

    /// Port of `SHAPE_POLY_SET::Rotate()`: rotate all vertices (re-triangulating a valid
    /// triangulation, G4b).
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        for poly in &mut self.polys {
            for path in poly {
                path.rotate(a_angle, a_center);
            }
        }

        // Don't re-cache if the triangulation is already invalid
        if self.triangulation_valid {
            self.cache_triangulation(false);
        }
    }

    /// Port of `SHAPE_POLY_SET::Move()`: moves the contours and the triangulation and updates
    /// the hash.
    fn move_(&mut self, a_vector: Vector2I) {
        self.tri_index = None;

        for poly in &mut self.polys {
            for path in poly {
                path.move_(a_vector);
            }
        }

        for tri in &mut self.triangulated_polys {
            tri.move_(a_vector);
        }

        self.hash = self.checksum();
        self.hash_valid = true;
    }

    fn is_solid(&self) -> bool {
        true
    }

    /// Port of `SHAPE_POLY_SET::Format()`: C++ code that rebuilds the set.
    fn format(&self, _a_cplusplus: bool) -> String {
        let mut ss = String::new();

        ss.push_str("SHAPE_LINE_CHAIN poly; \n");

        for poly in &self.polys {
            for (j, chain) in poly.iter().enumerate() {
                let _ = writeln!(ss, "{{ auto tmp = {};", chain.format(true));

                if j == 0 {
                    ss.push_str(" poly.AddOutline(tmp); } \n");
                } else {
                    ss.push_str(" poly.AddHole(tmp); } \n");
                }
            }
        }

        ss
    }
}

#[cfg(test)]
mod split_collinear_tests {
    //! Differential tests of the `split_collinear_outlines` shortcuts (small-outline
    //! pre-check, extent-filtered `PointInside`) against the plain r-tree implementation.

    use super::*;

    /// xorshift64* - deterministic test data without a dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn range(&mut self, lo: i64, hi: i64) -> i64 {
            lo + (self.next() % ((hi - lo + 1) as u64)) as i64
        }
    }

    /// The body of `split_collinear_outlines` before the shortcuts: index for every outline,
    /// plain `PointInside` in `isExteriorWaist()`.
    fn split_reference(set: &mut ShapePolySet) {
        let mut poly_idx = 0;

        while poly_idx < set.polys.len() {
            loop {
                let outline = &set.polys[poly_idx][0];
                let count = outline.point_count() as i64;

                let mut segs: Vec<Seg> = Vec::with_capacity(count as usize);

                for i in 0..count {
                    segs.push(Seg::new(
                        outline.c_point(i as i32),
                        outline.c_point(((i + 1) % count) as i32),
                    ));
                }

                let index = SegmentIndex::new(segs);

                let mut found = false;
                let mut seg_a: i64 = -1;
                let mut seg_b: i64 = -1;
                let mut i: i64 = 0;

                while i < count && !found {
                    let a = outline.c_point(i as i32);
                    let b = outline.c_point(((i + 1) % count) as i32);
                    let seg = Seg::new(a, b);

                    index.visit_candidates(&seg, 0, |j: i32| {
                        let j = j as i64;

                        if j == i || j == ((i + 1) % count) || j == ((i + count - 1) % count) {
                            return true;
                        }

                        let oa = outline.c_point(j as i32);
                        let ob = outline.c_point(((j + 1) % count) as i32);
                        let other = Seg::new(oa, ob);

                        if oa == a && ob == b {
                            return true;
                        }

                        if oa == b && ob == a {
                            return true;
                        }

                        if seg.approx_collinear_with_threshold(&other, 10)
                            && set.is_exterior_waist(&seg, &other, |pt| {
                                set.point_inside(pt, 0, false)
                            })
                        {
                            seg_a = i;
                            seg_b = j;
                            found = true;
                            return false;
                        }

                        true
                    });

                    i += 1;
                }

                if !found {
                    break;
                }

                let n = outline.point_count() as i64;
                let a0 = seg_a;
                let a1 = (seg_a + 1) % n;
                let b0 = seg_b;
                let b1 = (seg_b + 1) % n;

                let mut lc1 = ShapeLineChain::new();
                let mut idx = a1;
                lc1.append_point(outline.c_point(idx as i32), false);

                while idx != b0 {
                    idx = (idx + 1) % n;
                    lc1.append_point(outline.c_point(idx as i32), false);
                }

                lc1.set_closed(true);

                let mut lc2 = ShapeLineChain::new();
                idx = b1;
                lc2.append_point(outline.c_point(idx as i32), false);

                while idx != a0 {
                    idx = (idx + 1) % n;
                    lc2.append_point(outline.c_point(idx as i32), false);
                }

                lc2.set_closed(true);

                set.polys[poly_idx][0] = lc1;
                set.polys.push(vec![lc2]);
            }

            poly_idx += 1;
        }
    }

    fn chain(pts: &[Vector2I]) -> ShapeLineChain {
        let mut lc = ShapeLineChain::new();

        for p in pts {
            lc.append_point(*p, false);
        }

        lc.set_closed(true);
        lc
    }

    /// Star-shaped (simple) polygon with `n` vertices around `c`.
    fn star(rng: &mut Rng, c: Vector2I, n: usize, r: i64) -> Vec<Vector2I> {
        (0..n)
            .map(|k| {
                let ang = (k as f64 + 0.3 * (rng.range(0, 100) as f64 / 100.0))
                    * std::f64::consts::TAU
                    / n as f64;
                let rad = rng.range(r / 3, r) as f64;
                Vector2I::new(
                    c.x.wrapping_add((rad * ang.cos()) as i32),
                    c.y.wrapping_add((rad * ang.sin()) as i32),
                )
            })
            .collect()
    }

    /// Insert spike excursions (waists): v, v + L u, v + L u + w, v + s u + w, back to v's
    /// successor, with `w` a perpendicular offset of 0..=20 (around the 10 nm threshold).
    fn add_spikes(rng: &mut Rng, pts: &[Vector2I], spikes: usize) -> Vec<Vector2I> {
        let mut out = pts.to_vec();

        for _ in 0..spikes {
            if out.len() < 3 {
                break;
            }

            let k = rng.range(0, out.len() as i64 - 1) as usize;
            let v = out[k];
            let dir = match rng.range(0, 3) {
                0 => Vector2I::new(1, 0),
                1 => Vector2I::new(0, 1),
                2 => Vector2I::new(-1, 0),
                _ => Vector2I::new(0, -1),
            };
            let perp = Vector2I::new(-dir.y, dir.x);
            let len = rng.range(40, 400) as i32;
            let s = rng.range(0, len as i64 - 10) as i32;
            let w = [0, 0, 1, 5, 9, 10, 11, 15, 20][rng.range(0, 8) as usize];
            let tip = v + dir * len;
            let ins = [tip, tip + perp * w, v + dir * s + perp * w];
            for (m, p) in ins.iter().enumerate() {
                out.insert(k + 1 + m, *p);
            }
        }

        out
    }

    /// Random points on a coarse grid: self-intersecting outlines with many collinear
    /// overlaps.
    fn grid_poly(rng: &mut Rng, n: usize, step: i64, off: Vector2I) -> Vec<Vector2I> {
        (0..n)
            .map(|_| {
                Vector2I::new(
                    off.x + (rng.range(0, 8) * step) as i32,
                    off.y + (rng.range(0, 8) * step) as i32,
                )
            })
            .collect()
    }

    fn gen_set(rng: &mut Rng, case: usize) -> ShapePolySet {
        let mut set = ShapePolySet::new();
        let outlines = rng.range(1, 4) as usize;

        for o in 0..outlines {
            let c = Vector2I::new((o as i32) * 1500 + rng.range(-300, 300) as i32, 0);
            let n = match case % 5 {
                0 => rng.range(0, 3) as usize,
                1 => rng.range(28, 34) as usize,
                2 => rng.range(55, 70) as usize,
                3 => rng.range(120, 140) as usize,
                _ => rng.range(3, 20) as usize,
            };

            let pts = match rng.range(0, 3) {
                0 => star(rng, c, n.max(3), 600),
                1 => {
                    let base = star(rng, c, n.max(3), 600);
                    let k = rng.range(1, 4) as usize;
                    add_spikes(rng, &base, k)
                }
                2 => grid_poly(rng, n, 50, c),
                _ => {
                    let base = star(rng, c, n.max(3), 600);
                    let mut p = add_spikes(rng, &base, 2);
                    p.truncate(n.max(p.len().min(n)));
                    p
                }
            };

            let pts = if case % 5 == 0 { pts[..n.min(pts.len())].to_vec() } else { pts };
            set.add_outline(&chain(&pts));

            // Holes, fractured below into bridged outlines.
            if case % 3 == 0 && n >= 3 {
                let idx = set.outline_count() - 1;
                let hn = rng.range(3, 12) as usize;
                let hole = star(rng, c, hn, 120);
                let h = chain(&hole).reverse();
                set.add_hole(&h, idx);
            }
        }

        if case % 3 == 0 {
            set.fracture(false);
        }

        set
    }

    fn assert_same(a: &ShapePolySet, b: &ShapePolySet, case: usize) {
        assert_eq!(a.polys.len(), b.polys.len(), "case {case}: outline count");

        for (pa, pb) in a.polys.iter().zip(&b.polys) {
            assert_eq!(pa.len(), pb.len(), "case {case}");

            for (ca, cb) in pa.iter().zip(pb) {
                assert_eq!(ca.c_points(), cb.c_points(), "case {case}");
                assert_eq!(ca.is_closed(), cb.is_closed(), "case {case}");
            }
        }
    }

    #[test]
    fn split_matches_reference() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut splits = 0;

        for case in 0..6000 {
            let set = gen_set(&mut rng, case);
            let mut want = set.clone();
            split_reference(&mut want);
            let mut got = set.clone();
            got.split_collinear_outlines();
            assert_same(&want, &got, case);
            splits += want.polys.len() - set.polys.len();
        }

        // The generator must exercise the split path.
        assert!(splits > 100, "only {splits} splits");
    }

    #[test]
    fn split_matches_reference_near_int_limits() {
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);

        for case in 0..2000 {
            let off = match case % 4 {
                0 => Vector2I::new(EXTENT_BOUND - 200, -EXTENT_BOUND - 100),
                1 => Vector2I::new(i32::MAX - 400, i32::MIN + 10),
                2 => Vector2I::new(i32::MIN + 10, i32::MAX - 400),
                _ => Vector2I::new((1 << 30) - 200, (1 << 30) - 200),
            };
            let mut set = ShapePolySet::new();

            for _ in 0..rng.range(1, 3) {
                let n = rng.range(3, 12) as usize;
                set.add_outline(&chain(&grid_poly(&mut rng, n, 50, off)));
            }

            let mut want = set.clone();
            split_reference(&mut want);
            let mut got = set.clone();
            got.split_collinear_outlines();
            assert_same(&want, &got, case);
        }
    }

    /// The pre-check's box-overlap test is the `SegmentIndex` candidate set.
    #[test]
    fn box_overlap_equals_index_candidates() {
        let mut rng = Rng(0xDEAD_BEEF_0BAD_F00D);

        for case in 0..400 {
            let n = rng.range(1, 150) as usize;
            let pts = if case % 2 == 0 {
                grid_poly(&mut rng, n, 50, Vector2I::new(0, 0))
            } else {
                let base = star(&mut rng, Vector2I::new(0, 0), n.max(3), 600);
                add_spikes(&mut rng, &base, 3)
            };
            let count = pts.len();
            let segs: Vec<Seg> =
                (0..count).map(|i| Seg::new(pts[i], pts[(i + 1) % count])).collect();
            let index = SegmentIndex::new(segs.clone());

            for s in &segs {
                let mut got = Vec::new();
                index.visit_candidates(s, 0, |j| {
                    got.push(j as usize);
                    true
                });
                got.sort_unstable();

                let bx = |t: &Seg| {
                    [t.a.x.min(t.b.x), t.a.y.min(t.b.y), t.a.x.max(t.b.x), t.a.y.max(t.b.y)]
                };
                let bi = bx(s);
                let want: Vec<usize> = (0..count)
                    .filter(|&j| {
                        let bj = bx(&segs[j]);
                        !(bj[0] > bi[2] || bj[2] < bi[0] || bj[1] > bi[3] || bj[3] < bi[1])
                    })
                    .collect();

                assert_eq!(got, want, "case {case}");
            }
        }
    }

    #[test]
    fn extent_filtered_point_inside_matches_plain() {
        let mut rng = Rng(0x0F0F_1234_5555_AAAA);

        for case in 0..600 {
            let off = match case % 5 {
                0 => Vector2I::new(0, 0),
                1 => Vector2I::new(EXTENT_BOUND - 300, EXTENT_BOUND - 300),
                2 => Vector2I::new(-EXTENT_BOUND - 100, -EXTENT_BOUND + 50),
                3 => Vector2I::new(i32::MAX - 400, i32::MIN + 1),
                _ => Vector2I::new(i32::MIN, i32::MAX - 400),
            };
            let mut set = ShapePolySet::new();

            for _ in 0..rng.range(1, 4) {
                let n = rng.range(0, 14) as usize;
                let pts = if rng.range(0, 1) == 0 {
                    grid_poly(&mut rng, n, 50, off)
                } else {
                    star(&mut rng, Vector2I::new(off.x / 2 * 2, off.y), n, 150)
                        .into_iter()
                        .map(|p| Vector2I::new(p.x.max(off.x), p.y))
                        .collect()
                };
                set.add_outline(&chain(&pts));
            }

            let mut extents = Vec::new();
            set.build_outline_extents(&mut extents);

            let mut probes: Vec<Vector2I> = Vec::new();

            for e in &extents {
                if e.min.x > e.max.x {
                    continue;
                }

                for &x in &[e.min.x.wrapping_sub(1), e.min.x, e.max.x, e.max.x.wrapping_add(1)] {
                    for &y in &[e.min.y, e.max.y, e.min.y.wrapping_add(1), e.max.y.wrapping_sub(1)]
                    {
                        probes.push(Vector2I::new(x, y));
                    }
                }
            }

            for _ in 0..200 {
                probes.push(Vector2I::new(
                    off.x.wrapping_add(rng.range(-60, 460) as i32),
                    off.y.wrapping_add(rng.range(-60, 460) as i32),
                ));
            }

            for &x in &[i32::MIN, i32::MAX, EXTENT_BOUND, -EXTENT_BOUND, EXTENT_BOUND + 1] {
                probes.push(Vector2I::new(x, off.y.wrapping_add(100)));
            }

            for pt in probes {
                assert_eq!(
                    set.point_inside_with_extents(pt, &extents),
                    set.point_inside(pt, 0, false),
                    "case {case} pt {pt:?}"
                );
            }
        }
    }
}
