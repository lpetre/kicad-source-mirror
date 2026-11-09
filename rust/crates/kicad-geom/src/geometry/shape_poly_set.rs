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

//! Skeleton - full port in package G4.
//!
//! Partial port of `libs/kimath/include/geometry/shape_poly_set.h` and
//! `libs/kimath/src/geometry/shape_poly_set.cpp`: the data model (a set of polygons, each an
//! outline followed by its holes), the constructors, adding outlines / holes / polygons /
//! vertices with the C++ index conventions, the accessors, `TotalVertices`, `IsEmpty`,
//! `RemoveAllContours`, `BBox`, `Move` / `Rotate` of the contours, and the
//! `TRIANGULATED_POLYGON::TRI` shape (without the triangulation itself).

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::math::box2::{Box2D, Box2I};
use crate::math::vector2::Vector2I;

/// Port of `SHAPE_POLY_SET::POLYGON`: the outline (index 0) followed by the holes.
pub type Polygon = Vec<ShapeLineChain>;

/// Port of `SHAPE_POLY_SET::TRIANGULATED_POLYGON::TRI`: a single triangle of a poly-set
/// triangulation (`SH_POLY_SET_TRIANGLE`).
///
/// The C++ triangle refers to the vertices of its parent `TRIANGULATED_POLYGON` through a
/// pointer; this skeleton keeps copies of its three vertices next to the indices (package G4
/// decides on the final representation).
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

/// Port of `SHAPE_POLY_SET::TRIANGULATED_POLYGON` (data only; the triangulation is package G4).
#[derive(Clone, Debug, Default)]
pub struct TriangulatedPolygon {
    /// `m_sourceOutline`
    pub source_outline: i32,
    /// `m_triangles`
    pub triangles: Vec<Tri>,
    /// `m_vertices`
    pub vertices: Vec<Vector2I>,
}

/// Port of `SHAPE_POLY_SET`: a set of polygons, each with an outline and optional holes.
#[derive(Clone, Debug, Default)]
pub struct ShapePolySet {
    polys: Vec<Polygon>,
    triangulated_polys: Vec<TriangulatedPolygon>,
    triangulation_valid: bool,
}

impl ShapePolySet {
    /// Port of `SHAPE_POLY_SET()`: an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `SHAPE_POLY_SET( const BOX2D& aRect )`: a set with a single rectangular outline.
    pub fn from_box2d(a_rect: &Box2D) -> Self {
        // VECTOR2I( double, double ): the coordinates are truncated
        let mut set = Self::new();
        set.new_outline();
        set.append_point(
            Vector2I::new(a_rect.left() as i32, a_rect.top() as i32),
            -1,
            -1,
        );
        set.append_point(
            Vector2I::new(a_rect.right() as i32, a_rect.top() as i32),
            -1,
            -1,
        );
        set.append_point(
            Vector2I::new(a_rect.right() as i32, a_rect.bottom() as i32),
            -1,
            -1,
        );
        set.append_point(
            Vector2I::new(a_rect.left() as i32, a_rect.bottom() as i32),
            -1,
            -1,
        );
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
        debug_assert!(!self.polys.is_empty());

        let mut a_outline = a_outline;

        if a_outline < 0 {
            a_outline += self.polys.len() as i32;
        }

        let idx: usize = if a_hole < 0 { 0 } else { a_hole as usize + 1 };

        debug_assert!((a_outline as usize) < self.polys.len());
        debug_assert!(idx < self.polys[a_outline as usize].len());

        let contour = &mut self.polys[a_outline as usize][idx];
        contour.append_xy(x, y, a_allow_duplication);

        contour.point_count()
    }

    /// Port of `SHAPE_POLY_SET::Append( const VECTOR2I& aP, int aOutline, int aHole )`.
    pub fn append_point(&mut self, a_p: Vector2I, a_outline: i32, a_hole: i32) {
        self.append(a_p.x, a_p.y, a_outline, a_hole, false);
    }

    /// Port of `SHAPE_POLY_SET::Append( const SHAPE_POLY_SET& aSet )`: merge polygons from two
    /// sets.
    pub fn append_set(&mut self, a_set: &ShapePolySet) {
        self.polys.extend(a_set.polys.iter().cloned());
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

    /// Port of `SHAPE_POLY_SET::COutline()`.
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

    /// Port of `SHAPE_POLY_SET::CPolygon()`.
    pub fn c_polygon(&self, a_index: i32) -> &Polygon {
        &self.polys[a_index as usize]
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

    /// Port of `SHAPE_POLY_SET::IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// Port of `SHAPE_POLY_SET::RemoveAllContours()`.
    pub fn remove_all_contours(&mut self) {
        self.polys.clear();
        self.triangulated_polys.clear();
        self.triangulation_valid = false;
    }

    /// Port of `SHAPE_POLY_SET::IsTriangulationUpToDate()` (skeleton: the triangulation is not
    /// built yet, package G4).
    pub fn is_triangulation_up_to_date(&self) -> bool {
        self.triangulation_valid
    }

    /// Port of `SHAPE_POLY_SET::TriangulatedPolyCount()`.
    pub fn triangulated_poly_count(&self) -> u32 {
        self.triangulated_polys.len() as u32
    }

    /// Port of `SHAPE_POLY_SET::TriangulatedPolygon()`.
    pub fn triangulated_polygon(&self, a_index: i32) -> &TriangulatedPolygon {
        &self.triangulated_polys[a_index as usize]
    }
}

impl Shape for ShapePolySet {
    fn type_(&self) -> ShapeType {
        ShapeType::PolySet
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::PolySet(self)
    }

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

    /// `SHAPE_POLY_SET::Collide( const SEG&, ... )`: skeleton - needs
    /// `SquaredDistanceToSeg()` (package G4); always false for now (see `PORT_STATUS.md`).
    fn collide_seg(
        &self,
        _a_seg: &Seg,
        _a_clearance: i32,
        _a_actual: Option<&mut i32>,
        _a_location: Option<&mut Vector2I>,
    ) -> bool {
        false
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

    /// Port of `SHAPE_POLY_SET::Rotate()` (contours only; re-triangulation is package G4).
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        for poly in &mut self.polys {
            for path in poly {
                path.rotate(a_angle, a_center);
            }
        }
    }

    /// Port of `SHAPE_POLY_SET::Move()` (contours only; the triangulation and the hash are
    /// package G4).
    fn move_(&mut self, a_vector: Vector2I) {
        for poly in &mut self.polys {
            for path in poly {
                path.move_(a_vector);
            }
        }
    }

    fn is_solid(&self) -> bool {
        true
    }
}
