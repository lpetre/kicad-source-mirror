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

//! Skeleton - full port in package G3.
//!
//! Partial port of `libs/kimath/include/geometry/shape_line_chain.h` and
//! `libs/kimath/src/geometry/shape_line_chain.cpp`: the data members of `SHAPE_LINE_CHAIN`,
//! its constructors, appending points / chains / arcs, point and segment access, the arc
//! bookkeeping helpers, the bounding box, `Move` / `Rotate` and the `Collide` / `PointInside`
//! overrides.  The `SHAPE_LINE_CHAIN_BASE` methods it inherits are in
//! [`crate::geometry::shape`].

use crate::geometry::approximation::ErrorLoc;
use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::math::box2::Box2I;
use crate::math::util::{ki_round, rescale};
use crate::math::vector2::Vector2I;
use crate::trigo::rotate_point_around;

/// Port of `getArcPolygonizationMaxError()`.
///
/// This polyline will only be used for display.  The native arc is still used for output.
/// We therefore want to use a higher definition than the typical maxError.
pub fn get_arc_polygonization_max_error() -> i32 {
    ShapeArc::default_accuracy_for_pcb() / 5
}

/// Port of `SHAPE_LINE_CHAIN`: a polyline (open or closed), possibly containing arcs.
#[derive(Clone, Debug, Default)]
pub struct ShapeLineChain {
    /// array of vertices
    points: Vec<Vector2I>,

    /// Array of indices that refer to the index of the shape if the point is part of a larger
    /// shape, e.g. arc or spline.
    /// If the value is -1, the point is just a point.
    ///
    /// There can be up to two shapes associated with a single point (e.g. the end point of
    /// one arc might be the start point of another).
    ///
    /// Generally speaking only the first element of the pair will be populated (i.e. with a value
    /// not equal to SHAPE_IS_PT), unless the point is shared between two arc shapes. If the point
    /// is shared, then both the first and second element of the pair should be populated.
    ///
    /// The second element must always be SHAPE_IS_PT if the first element is SHAPE_IS_PT.
    shapes: Vec<(isize, isize)>,

    arcs: Vec<ShapeArc>,

    /// the maxError to use when converting arcs to points
    #[allow(dead_code)]
    accuracy: i32,

    /// is the line chain closed?
    closed: bool,

    /// Width of the segments (for BBox calculations in RTree)
    /// TODO Adjust usage of SHAPE_LINE_CHAIN to account for where we need a width and where not
    /// Alternatively, we could split the class into a LINE_CHAIN (no width) and SHAPE_LINE_CHAIN
    /// that derives from SHAPE as well that does have a width.  Not sure yet on the correct path.
    /// TODO Note that we also have SHAPE_SIMPLE which is a closed, filled SHAPE_LINE_CHAIN.
    width: i32,

    /// cached bounding box (`mutable` in C++; see `generate_bbox_cache`)
    bbox: Box2I,
}

impl PartialEq for ShapeLineChain {
    /// Port of `SHAPE_LINE_CHAIN::operator==`: compares the points only.
    fn eq(&self, a_rhs: &ShapeLineChain) -> bool {
        if self.point_count() != a_rhs.point_count() {
            return false;
        }

        for i in 0..self.point_count() {
            if self.c_point(i) != a_rhs.c_point(i) {
                return false;
            }
        }

        true
    }
}

impl ShapeLineChain {
    /// Port of `SHAPE_LINE_CHAIN::SHAPE_IS_PT`.
    pub const SHAPE_IS_PT: isize = -1;

    /// Port of `SHAPE_LINE_CHAIN::SHAPES_ARE_PT`.
    pub const SHAPES_ARE_PT: (isize, isize) = (Self::SHAPE_IS_PT, Self::SHAPE_IS_PT);

    /// Port of `SHAPE_LINE_CHAIN()`: an empty line chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `SHAPE_LINE_CHAIN( const std::vector<int>& aV )`: a chain from `x, y` pairs.
    /// (For an odd number of values the C++ reads past the end; the port ignores the last one.)
    pub fn from_coords(a_v: &[i32]) -> Self {
        let mut chain = Self::new();

        for pair in a_v.chunks_exact(2) {
            chain.append_xy(pair[0], pair[1], false);
        }

        chain
    }

    /// Port of `SHAPE_LINE_CHAIN( const std::vector<VECTOR2I>& aV, bool aClosed )`.
    ///
    /// As in C++, the cached bounding box is not computed.
    pub fn from_points(a_v: &[Vector2I], a_closed: bool) -> Self {
        let mut chain = ShapeLineChain {
            points: a_v.to_vec(),
            shapes: vec![Self::SHAPES_ARE_PT; a_v.len()],
            ..Self::default()
        };
        chain.set_closed(a_closed);
        chain
    }

    /// Port of `SHAPE_LINE_CHAIN( const SHAPE_ARC& aArc, bool aClosed,
    /// std::optional<int> aMaxError )`.
    pub fn from_arc(a_arc: &ShapeArc, a_closed: bool, a_max_error: Option<i32>) -> Self {
        let mut chain = ShapeLineChain {
            width: a_arc.get_width(),
            ..Self::default()
        };

        match a_max_error {
            Some(max_error) => chain.append_arc_with_error(a_arc, max_error),
            None => chain.append_arc(a_arc),
        }

        chain.set_closed(a_closed);
        chain
    }

    /// Port of `SHAPE_LINE_CHAIN::Clear()`: remove all points from the line chain.
    pub fn clear(&mut self) {
        self.points.clear();
        self.arcs.clear();
        self.shapes.clear();
        self.closed = false;
    }

    /// Port of `SHAPE_LINE_CHAIN::SetClosed()`: mark the line chain as closed (i.e. with a
    /// segment connecting the last point with the first point).
    pub fn set_closed(&mut self, a_closed: bool) {
        self.closed = a_closed;
        self.merge_first_last_point_if_needed();
    }

    /// Port of `SHAPE_LINE_CHAIN::IsClosed()`.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Port of `SHAPE_LINE_CHAIN::Width()`.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Port of `SHAPE_LINE_CHAIN::SegmentCount()`: the number of segments in this line chain.
    pub fn segment_count(&self) -> i32 {
        let mut c: i32 = self.points.len() as i32 - 1;

        if self.closed {
            c += 1;
        }

        0.max(c)
    }

    /// Port of `SHAPE_LINE_CHAIN::PointCount()`.
    pub fn point_count(&self) -> i32 {
        self.points.len() as i32
    }

    /// Port of `SHAPE_LINE_CHAIN::Segment()`: a copy of the `a_index`-th segment (negative
    /// indices count from the end).  An out-of-range index yields the C++ `wxCHECK` fallback:
    /// a zero-length segment at the last point (or at the origin for an empty chain).
    pub fn segment(&self, a_index: i32) -> Seg {
        let seg_count = self.segment_count();
        let mut a_index = a_index;

        if a_index < 0 {
            a_index += seg_count;
        }

        if !(a_index < seg_count && a_index >= 0) {
            return match self.points.last() {
                Some(&p) => Seg::new(p, p),
                None => Seg::from_coords(0, 0, 0, 0),
            };
        }

        if a_index == self.points.len() as i32 - 1 && self.closed {
            Seg::with_index(self.points[a_index as usize], self.points[0], a_index)
        } else {
            Seg::with_index(
                self.points[a_index as usize],
                self.points[a_index as usize + 1],
                a_index,
            )
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::CSegment()`.
    pub fn c_segment(&self, a_index: i32) -> Seg {
        self.segment(a_index)
    }

    /// Port of `SHAPE_LINE_CHAIN::CPoint()`: a point of the line chain.  Negative indices count
    /// from the end (-1 is the last point), and indices up to `2 * point_count() - 1` wrap
    /// around once, as in C++.
    pub fn c_point(&self, a_index: i32) -> Vector2I {
        let mut a_index = a_index;

        if a_index < 0 {
            a_index += self.point_count();
        } else if a_index >= self.point_count() {
            a_index -= self.point_count();
        }

        self.points[a_index as usize]
    }

    /// Port of `SHAPE_LINE_CHAIN::CPoints()`.
    pub fn c_points(&self) -> &[Vector2I] {
        &self.points
    }

    /// Port of `SHAPE_LINE_CHAIN::CLastPoint()`.
    pub fn c_last_point(&self) -> Vector2I {
        self.points[self.points.len() - 1]
    }

    /// Port of `SHAPE_LINE_CHAIN::CArcs()`.
    pub fn c_arcs(&self) -> &[ShapeArc] {
        &self.arcs
    }

    /// Port of `SHAPE_LINE_CHAIN::CShapes()`.
    pub fn c_shapes(&self) -> &[(isize, isize)] {
        &self.shapes
    }

    /// Port of `SHAPE_LINE_CHAIN::GenerateBBoxCache()`.  (The C++ method is `const` and writes
    /// a `mutable` member; here it takes `&mut self`.)
    pub fn generate_bbox_cache(&mut self) {
        self.bbox.compute(&self.points);

        if self.width != 0 {
            self.bbox.inflate(self.width);
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Append( int aX, int aY, bool aAllowDuplication )`.
    pub fn append_xy(&mut self, a_x: i32, a_y: i32, a_allow_duplication: bool) {
        let v = Vector2I::new(a_x, a_y);
        self.append_point(v, a_allow_duplication);
    }

    /// Port of `SHAPE_LINE_CHAIN::Append( const VECTOR2I& aP, bool aAllowDuplication )`: append
    /// a new point at the end of the line chain.  Unless `a_allow_duplication` is set, the
    /// point is skipped if it is the same as the last point.
    pub fn append_point(&mut self, a_p: Vector2I, a_allow_duplication: bool) {
        if self.points.is_empty() {
            self.bbox = Box2I::new(a_p, Vector2I::new(0, 0));
        }

        if self.points.is_empty() || a_allow_duplication || self.c_last_point() != a_p {
            self.points.push(a_p);
            self.shapes.push(Self::SHAPES_ARE_PT);
            self.bbox.merge_point(a_p);
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Append( const SHAPE_LINE_CHAIN& aOtherLine )`: append
    /// another line chain at the end.
    pub fn append_chain(&mut self, a_other_line: &ShapeLineChain) {
        debug_assert!(self.shapes.len() == self.points.len());

        if a_other_line.point_count() == 0 {
            return;
        }

        let num_arcs = self.arcs.len() as isize;
        self.arcs.extend(a_other_line.arcs.iter().cloned());

        let fix_shape_indices = |a_shape_indices: (isize, isize)| -> (isize, isize) {
            let fix = |a_index: isize| {
                if a_index != Self::SHAPE_IS_PT {
                    a_index + num_arcs
                } else {
                    a_index
                }
            };

            (fix(a_shape_indices.0), fix(a_shape_indices.1))
        };

        if self.point_count() == 0 || a_other_line.c_point(0) != self.c_last_point() {
            let p = a_other_line.c_point(0);
            self.points.push(p);
            self.shapes
                .push(fix_shape_indices(a_other_line.c_shapes()[0]));
            self.bbox.merge_point(p);
        } else if a_other_line.is_arc_segment(0) {
            // Associate the new arc shape with the last point of this chain
            let last = self.shapes.len() - 1;

            if self.shapes[last] == Self::SHAPES_ARE_PT {
                self.shapes[last].0 = a_other_line.c_shapes()[0].0 + num_arcs;
            } else {
                self.shapes[last].1 = a_other_line.c_shapes()[0].0 + num_arcs;
            }
        }

        for i in 1..a_other_line.point_count() {
            let p = a_other_line.c_point(i);
            self.points.push(p);

            let arc_index = a_other_line.arc_index(i as usize);

            if arc_index != Self::SHAPE_IS_PT {
                self.shapes
                    .push(fix_shape_indices(a_other_line.shapes[i as usize]));
            } else {
                self.shapes.push(Self::SHAPES_ARE_PT);
            }

            self.bbox.merge_point(p);
        }

        self.merge_first_last_point_if_needed();

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Append( const SHAPE_ARC& aArc )`.
    pub fn append_arc(&mut self, a_arc: &ShapeArc) {
        self.append_arc_with_error(a_arc, get_arc_polygonization_max_error());
    }

    /// Port of `SHAPE_LINE_CHAIN::Append( const SHAPE_ARC& aArc, int aMaxError )`.
    pub fn append_arc_with_error(&mut self, a_arc: &ShapeArc, a_max_error: i32) {
        let mut chain = a_arc.convert_to_polyline(a_max_error);

        if chain.point_count() > 2 {
            chain.arcs.push(a_arc.clone());
            chain.arcs.last_mut().unwrap().set_width(0);

            for sh in &mut chain.shapes {
                sh.0 = 0;
            }
        }

        self.append_chain(&chain);

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::ArcCount()`.
    pub fn arc_count(&self) -> usize {
        self.arcs.len()
    }

    /// Port of `SHAPE_LINE_CHAIN::ArcIndex()`: the arc index for the given segment index.
    pub fn arc_index(&self, a_segment: usize) -> isize {
        if self.is_shared_pt(a_segment) {
            self.shapes[a_segment].1
        } else {
            self.shapes[a_segment].0
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Arc()`.
    pub fn arc(&self, a_arc: usize) -> &ShapeArc {
        &self.arcs[a_arc]
    }

    /// Port of `SHAPE_LINE_CHAIN::IsSharedPt()`: test if a point is shared between multiple
    /// shapes.
    pub fn is_shared_pt(&self, a_index: usize) -> bool {
        a_index < self.shapes.len()
            && self.shapes[a_index].0 != Self::SHAPE_IS_PT
            && self.shapes[a_index].1 != Self::SHAPE_IS_PT
    }

    /// Port of `SHAPE_LINE_CHAIN::IsPtOnArc()`.
    pub fn is_pt_on_arc(&self, a_pt_index: usize) -> bool {
        a_pt_index < self.shapes.len() && self.shapes[a_pt_index] != Self::SHAPES_ARE_PT
    }

    /// Port of `SHAPE_LINE_CHAIN::IsArcSegment()`.
    pub fn is_arc_segment(&self, a_segment: usize) -> bool {
        // A segment is part of an arc except in the special case of two arcs next to each other
        // but without a shared vertex.  Here there is a segment between the end of the first arc
        // and the start of the second arc.
        let mut next_idx = a_segment.wrapping_add(1);

        // size_t arithmetic: m_shapes.size() - 1 wraps for an empty chain
        if next_idx > self.shapes.len().wrapping_sub(1) {
            if next_idx == self.shapes.len() && self.closed && self.is_shared_pt(0) {
                next_idx = 0; // segment between end point and first point
            } else {
                return false;
            }
        }

        self.is_pt_on_arc(a_segment) && (self.arc_index(a_segment) == self.shapes[next_idx].0)
    }

    /// Port of `SHAPE_LINE_CHAIN::IsArcStart()`.
    pub fn is_arc_start(&self, a_index: usize) -> bool {
        if !self.is_arc_segment(a_index) {
            // also does bound checking
            return false;
        }

        if self.is_shared_pt(a_index) {
            return true;
        }

        let arc = self.arc(self.arc_index(a_index) as usize);

        arc.get_p0() == self.points[a_index]
    }

    /// Port of `SHAPE_LINE_CHAIN::IsArcEnd()`.
    pub fn is_arc_end(&self, a_index: usize) -> bool {
        let mut prev_index = a_index.wrapping_sub(1);

        if a_index == 0 {
            prev_index = self.points.len().wrapping_sub(1);
        } else if a_index > self.points.len().wrapping_sub(1) {
            return false; // invalid index requested
        }

        if !self.is_arc_segment(prev_index) {
            return false;
        }

        if self.is_shared_pt(a_index) {
            return true;
        }

        let arc = self.arc(self.arc_index(a_index) as usize);

        arc.get_p1() == self.points[a_index]
    }

    /// Port of `SHAPE_LINE_CHAIN::fixIndicesRotation()`.
    fn fix_indices_rotation(&mut self) {
        if self.shapes.len() != self.points.len() {
            // wxCHECK
            return;
        }

        if self.shapes.len() <= 1 {
            return;
        }

        let mut rotations: usize = 0;

        while self.arc_index(0) != Self::SHAPE_IS_PT && !self.is_arc_start(0) {
            // Rotate right
            self.points.rotate_right(1);
            self.shapes.rotate_right(1);

            // Sanity check - avoid infinite loops  (NB: wxCHECK is not thread-safe)
            let r = rotations;
            rotations += 1;

            if r > self.shapes.len() {
                return;
            }
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::mergeFirstLastPointIfNeeded()`: merge the first and last
    /// point if they are the same and this chain is closed.
    pub fn merge_first_last_point_if_needed(&mut self) {
        if self.closed {
            if self.points.len() > 1 && self.points.first() == self.points.last() {
                let last_arc = self.arc_index(self.shapes.len() - 1);

                if last_arc != Self::SHAPE_IS_PT {
                    self.shapes[0].1 = self.shapes[0].0;
                    self.shapes[0].0 = last_arc;
                }

                self.points.pop();
                self.shapes.pop();

                self.fix_indices_rotation();
            }
        } else if self.points.len() > 1 && self.is_shared_pt(0) {
            // Create a duplicate point at the end
            self.points.push(self.points[0]);
            self.shapes.push((self.shapes[0].0, Self::SHAPE_IS_PT));
            self.shapes[0].0 = self.shapes[0].1;
            self.shapes[0].1 = Self::SHAPE_IS_PT;
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Distance( const VECTOR2I&, bool aOutlineOnly )`.
    pub fn distance_outline(&self, a_p: Vector2I, a_outline_only: bool) -> i32 {
        (self.lcb_squared_distance(a_p, a_outline_only) as f64).sqrt() as i32
    }
}

impl Shape for ShapeLineChain {
    fn type_(&self) -> ShapeType {
        ShapeType::LineChain
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::LineChain(self)
    }

    fn clone_shape(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    /// Port of `SHAPE_LINE_CHAIN::BBox()`.
    fn bbox(&self, a_clearance: i32) -> Box2I {
        let mut bbox = Box2I::default();
        bbox.compute(&self.points);

        if a_clearance != 0 || self.width != 0 {
            bbox.inflate(a_clearance + self.width);
        }

        bbox
    }

    /// Port of `SHAPE_LINE_CHAIN::Collide( const VECTOR2I&, ... )`.
    ///
    /// Note: This is overridden as we want to ensure we test collisions with the arcs in this
    /// chain as true arcs rather than segment approximations.
    fn collide_point(
        &self,
        a_p: Vector2I,
        a_clearance: i32,
        mut a_actual: Option<&mut i32>,
        mut a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.closed && self.point_inside(a_p, a_clearance, false) {
            if let Some(l) = a_location {
                *l = a_p;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        let mut closest_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
        let clearance_sq: Ecoord = Seg::square(a_clearance);
        let mut nearest = Vector2I::default();

        // Collide line segments
        for i in 0..self.segment_count() as usize {
            if self.is_arc_segment(i) {
                continue;
            }

            let s = self.segment(i as i32);
            let pn = s.nearest_point(a_p);
            let dist_sq: Ecoord = (pn - a_p).squared_euclidean_norm();

            if dist_sq < closest_dist_sq {
                nearest = pn;
                closest_dist_sq = dist_sq;

                if closest_dist_sq == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if closest_dist_sq < clearance_sq && a_actual.is_none() {
                    break;
                }
            }
        }

        if closest_dist_sq == 0 || closest_dist_sq < clearance_sq {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = (closest_dist_sq as f64).sqrt() as i32;
            }

            return true;
        }

        // Collide arc segments
        for arc in &self.arcs {
            // The arcs in the chain should have zero width
            debug_assert!(arc.get_width() == 0, "Invalid arc width - should be zero");

            if arc.collide_point(
                a_p,
                a_clearance,
                a_actual.as_deref_mut(),
                a_location.as_deref_mut(),
            ) {
                return true;
            }
        }

        false
    }

    /// Port of `SHAPE_LINE_CHAIN::Collide( const SEG&, ... )`.
    ///
    /// Note: This is overridden as we want to ensure we test collisions with the arcs in this
    /// chain as true arcs rather than segment approximations.
    fn collide_seg(
        &self,
        a_seg: &Seg,
        a_clearance: i32,
        a_actual: Option<&mut i32>,
        a_location: Option<&mut Vector2I>,
    ) -> bool {
        if self.closed && self.point_inside(a_seg.a, 0, false) {
            if let Some(l) = a_location {
                *l = a_seg.a;
            }

            if let Some(a) = a_actual {
                *a = 0;
            }

            return true;
        }

        let mut closest_dist_sq: Ecoord = Vector2I::ECOORD_MAX;
        let clearance_sq: Ecoord = Seg::square(a_clearance);
        let mut nearest = Vector2I::default();

        // Collide line segments
        for i in 0..self.segment_count() {
            if self.is_arc_segment(i as usize) {
                continue;
            }

            let s = self.segment(i);
            let dist_sq: Ecoord = s.squared_distance_seg(a_seg);

            if dist_sq < closest_dist_sq {
                if a_location.is_some() {
                    nearest = s.nearest_point_seg(a_seg);
                }

                closest_dist_sq = dist_sq;

                if closest_dist_sq == 0 {
                    break;
                }

                // If we're not looking for aActual then any collision will do
                if closest_dist_sq < clearance_sq && a_actual.is_none() {
                    break;
                }
            }
        }

        if closest_dist_sq == 0 || closest_dist_sq < clearance_sq {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = (closest_dist_sq as f64).sqrt() as i32;
            }

            return true;
        }

        let mut dist: i32 = i32::MAX;
        let mut closest_dist: Ecoord = i32::MAX as Ecoord;

        // A chain with no straight segments never measured anything, so the sentinel has no root
        if closest_dist_sq != Vector2I::ECOORD_MAX {
            closest_dist = ki_round((closest_dist_sq as f64).sqrt()) as Ecoord;
        }

        let want_actual = a_actual.is_some();
        let want_location = a_location.is_some();

        // Collide arc segments
        for arc in &self.arcs {
            let mut pos = Vector2I::default();

            // The arcs in the chain should have zero width
            debug_assert!(arc.get_width() == 0, "Invalid arc width - should be zero");

            if arc.collide_seg(
                a_seg,
                a_clearance,
                if want_actual || want_location {
                    Some(&mut dist)
                } else {
                    None
                },
                if want_location { Some(&mut pos) } else { None },
            ) {
                if !want_actual {
                    return true;
                }

                if (dist as Ecoord) < closest_dist {
                    closest_dist = dist as Ecoord;
                    nearest = pos;
                }
            }
        }

        if closest_dist == 0 || closest_dist < a_clearance as Ecoord {
            if let Some(l) = a_location {
                *l = nearest;
            }

            if let Some(a) = a_actual {
                *a = closest_dist as i32;
            }

            return true;
        }

        false
    }

    /// `SHAPE_LINE_CHAIN_BASE::SquaredDistance()`.
    fn squared_distance(&self, a_p: Vector2I, a_outline_only: bool) -> Ecoord {
        self.lcb_squared_distance(a_p, a_outline_only)
    }

    /// Port of `SHAPE_LINE_CHAIN::PointInside()`.
    fn point_inside(&self, a_pt: Vector2I, a_accuracy: i32, a_use_bbox_cache: bool) -> bool {
        if a_use_bbox_cache && !self.bbox.contains(a_pt) {
            return false;
        }

        let point_count = self.points.len() as i32;

        if !self.closed || point_count < 3 {
            return false;
        }

        let pts = &self.points;
        let mut inside = false;
        let mut i: i32 = 0;

        while i < point_count {
            let p1 = pts[i as usize];
            i += 1;
            let p2 = pts[if i == point_count { 0 } else { i as usize }];
            let diff = p2 - p1;

            if diff.y == 0 {
                continue;
            }

            let d: i32 = rescale(diff.x, a_pt.y - p1.y, diff.y);

            if ((p1.y >= a_pt.y) != (p2.y >= a_pt.y)) && (a_pt.x - p1.x < d) {
                inside = !inside;
            }
        }

        if a_accuracy <= 1 {
            inside
        } else {
            inside || self.point_on_edge(a_pt, a_accuracy)
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::TransformToPolygon()`.
    fn transform_to_polygon(
        &self,
        a_buffer: &mut ShapePolySet,
        _a_error: i32,
        _a_error_loc: ErrorLoc,
    ) {
        a_buffer.add_outline(self);
    }

    /// Port of `SHAPE_LINE_CHAIN::Rotate()`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        for pt in &mut self.points {
            rotate_point_around(pt, a_center, a_angle);
        }

        for arc in &mut self.arcs {
            arc.rotate(a_angle, a_center);
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::SetWidth()`.
    fn set_width(&mut self, a_width: i32) {
        self.width = a_width;
    }

    /// Port of `SHAPE_LINE_CHAIN::Move()`.
    fn move_(&mut self, a_vector: Vector2I) {
        for pt in &mut self.points {
            *pt += a_vector;
        }

        for arc in &mut self.arcs {
            arc.move_(a_vector);
        }

        self.bbox.move_(a_vector);
    }

    /// Port of `SHAPE_LINE_CHAIN::IsSolid()`.
    fn is_solid(&self) -> bool {
        false
    }
}

impl ShapeLineChainBase for ShapeLineChain {
    fn get_point(&self, a_index: i32) -> Vector2I {
        self.c_point(a_index)
    }

    fn get_segment(&self, a_index: i32) -> Seg {
        self.c_segment(a_index)
    }

    fn get_point_count(&self) -> usize {
        self.point_count() as usize
    }

    fn get_segment_count(&self) -> usize {
        self.segment_count() as usize
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn get_cached_bbox(&self) -> Option<Box2I> {
        Some(self.bbox)
    }
}
