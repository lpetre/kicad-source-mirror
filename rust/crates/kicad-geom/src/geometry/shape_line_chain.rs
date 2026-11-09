// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2013-2017 CERN
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>
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

//! Port of `libs/kimath/include/geometry/shape_line_chain.h` and
//! `libs/kimath/src/geometry/shape_line_chain.cpp` (`SHAPE_LINE_CHAIN`, `CLIPPER_Z_VALUE`,
//! `SHAPE_LINE_CHAIN::INTERSECTION`, `POINT_INSIDE_TRACKER`, `compareOriginDistance`).
//!
//! The `SHAPE_LINE_CHAIN_BASE` methods defined in `shape_line_chain.cpp` are the default
//! methods of [`ShapeLineChainBase`] in [`crate::geometry::shape`].
//!
//! Overloads (see `PORTING.md` section 2):
//!
//! | C++ | Rust |
//! |---|---|
//! | `SHAPE_LINE_CHAIN( std::vector<int> )` / `( std::vector<VECTOR2I>, bool )` / `( SHAPE_ARC, bool, optional<int> )` / `( Path64, zvalues, arcs )` | `from_coords` / `from_points` / `from_arc` / `from_clipper2` |
//! | `Append( x, y, dup )` / `( VECTOR2I, dup )` / `( SHAPE_LINE_CHAIN )` / `( SHAPE_ARC )` / `( SHAPE_ARC, int )` | `append_xy` / `append_point` / `append_chain` / `append_arc` / `append_arc_with_error` |
//! | `Insert( v, VECTOR2I )` / `( v, SHAPE_ARC )` / `( v, SHAPE_ARC, int )` | `insert_point` / `insert_arc` / `insert_arc_with_error` |
//! | `Replace( s, e, VECTOR2I )` / `( s, e, SHAPE_LINE_CHAIN )` | `replace_point` / `replace_chain` |
//! | `Remove( s, e )` / `Remove( i )` | `remove_range` / `remove` |
//! | `Split( VECTOR2I, bool )` / `Split( start, end, pre, mid, post )` | `split` / `split_range` |
//! | `Slice( s, e )` / `Slice( s, e, int )` | `slice` / `slice_with_error` |
//! | `Intersects( SEG )` / `( SHAPE_LINE_CHAIN )` | `intersects_seg` / `intersects_chain` |
//! | `Intersect( SEG, ... )` / `( SHAPE_LINE_CHAIN, ... )` | `intersect_seg` / `intersect_chain` |
//! | `NearestPoint( VECTOR2I, bool )` / `( SEG, int& )` | `nearest_point` / `nearest_point_seg` |
//! | `ClosestPoints( chain, ... )` / static `ClosestPoints( iterators ... )` | `closest_points` / `closest_points_ranges` |
//! | `Mirror( VECTOR2I, FLIP_DIRECTION )` / `Mirror( SEG )` | `mirror` / `mirror_seg` |
//! | `Distance( VECTOR2I, bool )` (and `SHAPE::Distance( VECTOR2I )`) | `distance_outline` (and [`Shape::distance`]) |
//! | `operator==` / `operator!=` | `PartialEq` |
//!
//! Out-parameters become return values where the C++ always writes them, and `&mut` in/out
//! parameters where it may leave them untouched (`nearest_point_seg`).  Iterator ranges of the
//! static helpers are slices.

use std::collections::{BTreeMap, BTreeSet};

use crate::cmath::{atan2, f64_to_i32_x86, f64_to_i64_x86};
use crate::geometry::approximation::ErrorLoc;
use crate::geometry::circle::Circle;
use crate::geometry::eda_angle::{DEGREES_T, EdaAngle};
use crate::geometry::seg::{Ecoord, Seg};
use crate::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use crate::geometry::shape_arc::ShapeArc;
use crate::geometry::shape_poly_set::ShapePolySet;
use crate::kicore::mirror::FlipDirection;
use crate::math::box2::Box2I;
use crate::math::box2_minmax::Box2IMinMax;
use crate::math::util::{ki_round, rescale};
use crate::math::vector2::{Vector2D, Vector2I, Vector2L};
use crate::stdsort::std_sort;
use crate::trigo::{rotate_point_around, test_segment_hit};

/// Port of `getArcPolygonizationMaxError()`.
///
/// This polyline will only be used for display.  The native arc is still used for output.
/// We therefore want to use a higher definition than the typical maxError.
pub fn get_arc_polygonization_max_error() -> i32 {
    ShapeArc::default_accuracy_for_pcb() / 5
}

/// Port of `CLIPPER_Z_VALUE`: holds information on each point of a `SHAPE_LINE_CHAIN` that is
/// retrievable after an operation with Clipper2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipperZValue {
    /// `m_FirstArcIdx`
    pub first_arc_idx: isize,
    /// `m_SecondArcIdx`
    pub second_arc_idx: isize,
}

impl Default for ClipperZValue {
    /// Port of `CLIPPER_Z_VALUE()`.
    fn default() -> Self {
        ClipperZValue {
            first_arc_idx: -1,
            second_arc_idx: -1,
        }
    }
}

impl ClipperZValue {
    /// Port of `CLIPPER_Z_VALUE( const std::pair<ssize_t, ssize_t> aShapeIndices,
    /// ssize_t aOffset )`: non-negative indices are shifted by `a_offset`.
    pub fn from_shape_indices(a_shape_indices: (isize, isize), a_offset: isize) -> Self {
        let offset_val = |a_val: isize| if a_val >= 0 { a_val + a_offset } else { a_val };

        ClipperZValue {
            first_arc_idx: offset_val(a_shape_indices.0),
            second_arc_idx: offset_val(a_shape_indices.1),
        }
    }
}

/// A Clipper2 `Point64` (built with `USINGZ`), seen by the line chain conversion functions.
///
/// The vendored Clipper2 port implements this for its point type; the line chain module itself
/// does not depend on it.  `(x, y, z)` triples implement it as well.
pub trait ClipperPoint64 {
    /// `Point64::x`
    fn x(&self) -> i64;
    /// `Point64::y`
    fn y(&self) -> i64;
    /// `Point64::z`
    fn z(&self) -> i64;
}

impl ClipperPoint64 for (i64, i64, i64) {
    fn x(&self) -> i64 {
        self.0
    }

    fn y(&self) -> i64 {
        self.1
    }

    fn z(&self) -> i64 {
        self.2
    }
}

/// Port of `SHAPE_LINE_CHAIN::INTERSECTION`: represents an intersection between two line
/// segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intersection {
    /// Point of intersection between our and their.
    pub p: Vector2I,

    /// Index of the intersecting corner/segment in the 'our' (== this) line.
    pub index_our: i32,

    /// index of the intersecting corner/segment in the 'their' (Intersect() method
    /// parameter) line.
    pub index_their: i32,

    /// When true, the corner [index_our] of the 'our' line lies exactly on 'their' line.
    pub is_corner_our: bool,

    /// When true, the corner [index_their] of the 'their' line lies exactly on 'our' line.
    /// Note that when both is_corner_our and is_corner_their are set, the line chains touch
    /// with with corners.
    pub is_corner_their: bool,

    /// Auxiliary flag to avoid copying intersection info to intersection refining code, used
    /// by the refining code (e.g. hull handling stuff in the P&S) to reject false positives.
    pub valid: bool,
}

impl Default for Intersection {
    fn default() -> Self {
        Intersection {
            p: Vector2I::default(),
            index_our: -1,
            index_their: -1,
            is_corner_our: false,
            is_corner_their: false,
            valid: false,
        }
    }
}

/// Port of `SHAPE_LINE_CHAIN::compareOriginDistance`: orders intersections by the (rounded)
/// distance of their points from an origin.
///
/// This nested functor (using `EuclideanNorm()`) is the one `Intersect( const SEG& )` uses:
/// inside the member function, unqualified lookup finds the class member before the file-scope
/// `compareOriginDistance` of `shape_line_chain.cpp` (which compares squared norms and is
/// therefore never used).
#[derive(Clone, Copy, Debug)]
pub struct CompareOriginDistance {
    /// `m_origin`
    pub origin: Vector2I,
}

impl CompareOriginDistance {
    /// Port of `compareOriginDistance( const VECTOR2I& aOrigin )`.
    pub fn new(a_origin: Vector2I) -> Self {
        CompareOriginDistance { origin: a_origin }
    }

    /// Port of `compareOriginDistance::operator()`.
    pub fn compare(&self, a_a: &Intersection, a_b: &Intersection) -> bool {
        (self.origin - a_a.p).euclidean_norm() < (self.origin - a_b.p).euclidean_norm()
    }
}

/// Port of `SHAPE_LINE_CHAIN::POINT_INSIDE_TRACKER`: a dynamic state checking if a point lies
/// within the area defined by a set of line chains joined end to end.
#[derive(Clone, Copy, Debug)]
pub struct PointInsideTracker {
    point: Vector2I,
    last_point: Vector2I,
    first_point: Vector2I,
    finished: bool,
    state: i32,
    count: i32,
}

impl PointInsideTracker {
    /// Port of `POINT_INSIDE_TRACKER( const VECTOR2I& aPoint )`.
    pub fn new(a_point: Vector2I) -> Self {
        PointInsideTracker {
            point: a_point,
            last_point: Vector2I::default(),
            first_point: Vector2I::default(),
            finished: false,
            state: 0,
            count: 0,
        }
    }

    /// Port of `POINT_INSIDE_TRACKER::processVertex()`.
    fn process_vertex(&mut self, ip: Vector2I, ip_next: Vector2I) -> bool {
        let m = self.point;

        if ip_next.y == m.y
            && ((ip_next.x == m.x) || (ip.y == m.y && ((ip_next.x > m.x) == (ip.x < m.x))))
        {
            self.finished = true;
            self.state = -1;
            return false;
        }

        if (ip.y < m.y) != (ip_next.y < m.y) {
            if ip.x >= m.x {
                if ip_next.x > m.x {
                    self.state = 1 - self.state;
                } else {
                    let d: f64 = (ip.x - m.x) as f64 * (ip_next.y - m.y) as f64
                        - (ip_next.x - m.x) as f64 * (ip.y - m.y) as f64;

                    if d == 0.0 {
                        self.finished = true;
                        self.state = -1;
                        return false;
                    }

                    if (d > 0.0) == (ip_next.y > ip.y) {
                        self.state = 1 - self.state;
                    }
                }
            } else if ip_next.x > m.x {
                let d: f64 = (ip.x - m.x) as f64 * (ip_next.y - m.y) as f64
                    - (ip_next.x - m.x) as f64 * (ip.y - m.y) as f64;

                if d == 0.0 {
                    self.finished = true;
                    self.state = -1;
                    return false;
                }

                if (d > 0.0) == (ip_next.y > ip.y) {
                    self.state = 1 - self.state;
                }
            }
        }

        true
    }

    /// Port of `POINT_INSIDE_TRACKER::AddPolyline()`.
    pub fn add_polyline(&mut self, a_polyline: &ShapeLineChain) {
        if self.count == 0 {
            self.last_point = a_polyline.c_point(0);
            self.first_point = a_polyline.c_point(0);
        }

        self.count += a_polyline.point_count();

        for i in 1..a_polyline.point_count() {
            let p = a_polyline.c_point(i);

            if !self.process_vertex(self.last_point, p) {
                return;
            }

            self.last_point = p;
        }
    }

    /// Port of `POINT_INSIDE_TRACKER::IsInside()`.
    pub fn is_inside(&mut self) -> bool {
        self.process_vertex(self.last_point, self.first_point);
        self.state > 0
    }
}

/// Compact bounding-box record used inside `SHAPE_LINE_CHAIN::Intersect` (`SEG_EXTENT`).
#[derive(Clone, Copy, Debug)]
struct SegExtent {
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    seg_idx: i32,
}

/// `SHAPE_KEY` of `SelfIntersectingWithArcs()`.
#[derive(Clone, Copy, Debug)]
struct ShapeKey {
    first_idx: i32,
    arc_idx: i32,
    bbox: Box2IMinMax,
}

/// A whitespace-separated token reader with the `std::istream >>` semantics that
/// `SHAPE_LINE_CHAIN::Parse()` relies on (the C++ takes a `std::stringstream&`).
///
/// Once an extraction fails, the stream is in the failed state and every further extraction
/// fails and yields 0 (C++11 writes 0 on a failed numeric extraction; after that the sentry
/// fails and the variable keeps its - possibly uninitialised - value).
#[derive(Clone, Debug)]
pub struct StringStream<'a> {
    text: &'a str,
    pos: usize,
    failed: bool,
}

impl<'a> StringStream<'a> {
    /// A stream reading `a_text` from its beginning.
    pub fn new(a_text: &'a str) -> Self {
        StringStream {
            text: a_text,
            pos: 0,
            failed: false,
        }
    }

    /// `aStream.str().size()`: the length of the whole underlying string.
    pub fn str_size(&self) -> usize {
        self.text.len()
    }

    /// Whether an extraction has failed (`fail()`).
    pub fn fail(&self) -> bool {
        self.failed
    }

    /// The not yet consumed rest of the text.
    pub fn remaining(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn next_token(&mut self) -> Option<&'a str> {
        if self.failed {
            return None;
        }

        let rest = &self.text[self.pos..];
        let trimmed = rest.trim_start();
        let start = self.pos + (rest.len() - trimmed.len());
        let len = trimmed
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(trimmed.len());

        if len == 0 {
            self.pos = start;
            self.failed = true;
            return None;
        }

        self.pos = start + len;
        Some(&self.text[start..start + len])
    }

    /// `>>` into an `int`.
    pub fn read_i32(&mut self) -> i32 {
        match self.next_token().and_then(|t| t.parse::<i32>().ok()) {
            Some(v) => v,
            None => {
                self.failed = true;
                0
            }
        }
    }

    /// `>>` into a `ssize_t`.
    pub fn read_isize(&mut self) -> isize {
        match self.next_token().and_then(|t| t.parse::<isize>().ok()) {
            Some(v) => v,
            None => {
                self.failed = true;
                0
            }
        }
    }

    /// `>>` into a `size_t`.
    pub fn read_usize(&mut self) -> usize {
        match self.next_token().and_then(|t| t.parse::<usize>().ok()) {
            Some(v) => v,
            None => {
                self.failed = true;
                0
            }
        }
    }

    /// `>>` into a `bool` (without `boolalpha`: `0` or `1`).
    pub fn read_bool(&mut self) -> bool {
        match self.next_token().and_then(|t| t.parse::<i64>().ok()) {
            Some(0) => false,
            Some(1) => true,
            _ => {
                self.failed = true;
                false
            }
        }
    }

    /// `>>` into a `double`.
    pub fn read_f64(&mut self) -> f64 {
        match self.next_token().and_then(|t| t.parse::<f64>().ok()) {
            Some(v) => v,
            None => {
                self.failed = true;
                0.0
            }
        }
    }
}

/// Port of `SHAPE_LINE_CHAIN`: a polyline (open or closed), possibly containing arcs.
///
/// Represents a polyline containing arcs as well as line segments: A chain of connected line
/// and/or arc segments.  The arc shapes are stored in `arcs` and every point of the chain
/// that belongs to an arc refers to it by index in `shapes`.
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
    /// Port of `SHAPE_LINE_CHAIN::operator==` (and `operator!=`): compares the points only.
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

/// `alg::run_on_pair( pair, f )`: `f` on the first element, then on the second.
#[inline]
fn run_on_pair(a_pair: &mut (isize, isize), mut a_f: impl FnMut(&mut isize)) {
    a_f(&mut a_pair.0);
    a_f(&mut a_pair.1);
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

    /// Port of `SHAPE_LINE_CHAIN( const Clipper2Lib::Path64& aPath,
    /// const std::vector<CLIPPER_Z_VALUE>& aZValueBuffer,
    /// const std::vector<SHAPE_ARC>& aArcBuffer )`: a closed chain from a Clipper2 result
    /// path whose z values index `a_z_value_buffer`.
    ///
    /// The coordinates are narrowed to `int` as in C++.  Where the C++ would throw
    /// (`std::vector::at` with an arc index outside `a_arc_buffer`) or write outside the shape
    /// array (a duplicate point dropped by `Append()`), the port leaves the point a plain point.
    pub fn from_clipper2<P: ClipperPoint64>(
        a_path: &[P],
        a_z_value_buffer: &[ClipperZValue],
        a_arc_buffer: &[ShapeArc],
    ) -> Self {
        let mut chain = ShapeLineChain {
            closed: true,
            ..Self::default()
        };

        let mut loaded_arcs: BTreeMap<isize, isize> = BTreeMap::new();
        chain.points.reserve(a_path.len());
        chain.shapes.reserve(a_path.len());

        let mut load_arc = |arcs: &mut Vec<ShapeArc>, a_arc_index: isize| -> isize {
            if a_arc_index == Self::SHAPE_IS_PT {
                return Self::SHAPE_IS_PT;
            }

            if let Some(&idx) = loaded_arcs.get(&a_arc_index) {
                return idx;
            }

            match usize::try_from(a_arc_index)
                .ok()
                .and_then(|i| a_arc_buffer.get(i))
            {
                Some(arc) => {
                    let idx = arcs.len() as isize;
                    loaded_arcs.insert(a_arc_index, idx);
                    arcs.push(arc.clone());
                    idx
                }
                None => {
                    debug_assert!(false, "arc index outside the arc buffer");
                    Self::SHAPE_IS_PT
                }
            }
        };

        for (ii, pt) in a_path.iter().enumerate() {
            chain.append_xy(pt.x() as i32, pt.y() as i32, false);

            // Add arc info (if exists)
            let idx_z: i32 = pt.z() as i32;

            if idx_z < 0 || idx_z >= a_z_value_buffer.len() as i32 {
                continue;
            }

            let z = a_z_value_buffer[idx_z as usize];

            if ii >= chain.shapes.len() {
                continue;
            }

            chain.shapes[ii].0 = load_arc(&mut chain.arcs, z.first_arc_idx);
            chain.shapes[ii].1 = load_arc(&mut chain.arcs, z.second_arc_idx);
        }

        // Clipper shouldn't return duplicate contiguous points. if it did, these would be
        // removed during Append() and we would have different number of shapes to points
        debug_assert!(chain.shapes.len() == chain.points.len());

        // Clipper might mess up the rotation of the indices such that an arc can be split between
        // the end point and wrap around to the start point. Lets fix the indices up now
        chain.fix_indices_rotation();
        chain
    }

    /// Port of `SHAPE_LINE_CHAIN::convertToClipper2()`: create a new Clipper2 path from the
    /// line chain, oriented as requested; the arc information is appended to the two buffers
    /// and referenced by the z values.
    pub fn convert_to_clipper2(
        &self,
        a_required_orientation: bool,
        a_z_value_buffer: &mut Vec<ClipperZValue>,
        a_arc_buffer: &mut Vec<ShapeArc>,
    ) -> Vec<(i64, i64, i64)> {
        let orientation = self.area(false) >= 0.0;
        let shape_offset = a_arc_buffer.len() as isize;

        let input = if orientation != a_required_orientation {
            self.reverse()
        } else {
            self.clone()
        };

        let point_count = input.point_count();
        let mut c_path = Vec::with_capacity(point_count as usize);

        for i in 0..point_count {
            let vertex = input.c_point(i);

            let z_value = ClipperZValue::from_shape_indices(input.shapes[i as usize], shape_offset);
            let z_value_ptr = a_z_value_buffer.len();
            a_z_value_buffer.push(z_value);

            c_path.push((vertex.x as i64, vertex.y as i64, z_value_ptr as i64));
        }

        a_arc_buffer.extend(input.arcs.iter().cloned());

        c_path
    }

    /// Port of `SHAPE_LINE_CHAIN::fixIndicesRotation()`: fix indices of this chain to ensure
    /// arcs are not split between the end and start indices.
    pub fn fix_indices_rotation(&mut self) {
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

    /// Port of `SHAPE_LINE_CHAIN::convertArc()`: convert an arc to only a point chain by
    /// removing the arc and references.  Negative indices count from the end.
    pub fn convert_arc(&mut self, a_arc_index: isize) {
        let mut a_arc_index = a_arc_index;

        if a_arc_index < 0 {
            a_arc_index += self.arcs.len() as isize;
        }

        // (a still negative index - no arcs at all - would erase before begin() in C++)
        if a_arc_index < 0 || a_arc_index >= self.arcs.len() as isize {
            return;
        }

        // Clear the shapes references
        for sh in &mut self.shapes {
            run_on_pair(sh, |a_shape_index| {
                if *a_shape_index == a_arc_index {
                    *a_shape_index = Self::SHAPE_IS_PT;
                }

                if *a_shape_index > a_arc_index {
                    *a_shape_index -= 1;
                }
            });

            if sh.1 != Self::SHAPE_IS_PT && sh.0 == Self::SHAPE_IS_PT {
                std::mem::swap(&mut sh.0, &mut sh.1);
            }
        }

        self.arcs.remove(a_arc_index as usize);
    }

    /// Port of `SHAPE_LINE_CHAIN::amendArc()`.
    pub fn amend_arc(&mut self, a_arc_index: usize, a_new_start: Vector2I, a_new_end: Vector2I) {
        if a_arc_index >= self.arcs.len() {
            // wxCHECK_MSG: "Invalid arc index requested."
            return;
        }

        let the_arc = &self.arcs[a_arc_index];

        // Try to preseve the centre of the original arc
        let mut new_arc = ShapeArc::new();
        new_arc.construct_from_start_end_center(
            a_new_start,
            a_new_end,
            the_arc.get_center(),
            the_arc.is_clockwise(),
            0.0,
        );

        self.arcs[a_arc_index] = new_arc;
    }

    /// Port of `SHAPE_LINE_CHAIN::amendArcStart()`.
    pub fn amend_arc_start(&mut self, a_arc_index: usize, a_new_start: Vector2I) {
        let p1 = self.arcs[a_arc_index].get_p1();
        self.amend_arc(a_arc_index, a_new_start, p1);
    }

    /// Port of `SHAPE_LINE_CHAIN::amendArcEnd()`.
    pub fn amend_arc_end(&mut self, a_arc_index: usize, a_new_end: Vector2I) {
        let p0 = self.arcs[a_arc_index].get_p0();
        self.amend_arc(a_arc_index, p0, a_new_end);
    }

    /// Port of `SHAPE_LINE_CHAIN::splitArc()`: split an arc into two arcs at `a_pt_index`.
    /// If `a_coincident` is set, the point is shared by the two arcs.
    pub fn split_arc(&mut self, a_pt_index: isize, a_coincident: bool) {
        let mut a_pt_index = a_pt_index;

        if a_pt_index < 0 {
            a_pt_index += self.shapes.len() as isize;
        }

        // (size_t) conversions of the C++ calls
        let pu = a_pt_index as usize;

        if !self.is_shared_pt(pu) && self.is_arc_start(pu) {
            return; // Nothing to do
        }

        if !self.is_pt_on_arc(pu) {
            return; // Nothing to do
        }

        if a_pt_index >= self.shapes.len() as isize {
            // wxCHECK_MSG: "Invalid point index requested."
            return;
        }

        if self.is_shared_pt(pu) || self.is_arc_end(pu) {
            if a_coincident || a_pt_index == 0 {
                return; // nothing to do
            }

            let first_arc_index = self.shapes[pu].0;

            let new_start = self.arcs[first_arc_index as usize].get_p0(); // don't amend the start
            let new_end = self.points[pu - 1];
            self.amend_arc(first_arc_index as usize, new_start, new_end);

            if self.is_shared_pt(pu) {
                self.shapes[pu].0 = self.shapes[pu].1;
                self.shapes[pu].1 = Self::SHAPE_IS_PT;
            } else {
                self.shapes[pu] = Self::SHAPES_ARE_PT;
            }

            return;
        }

        let curr_arc_idx = self.arc_index(pu);
        let current_arc = self.arcs[curr_arc_idx as usize].clone();

        let mut new_arc1 = ShapeArc::new();
        let mut new_arc2 = ShapeArc::new();

        let arc1_end = if a_coincident {
            self.points[pu]
        } else {
            self.points[pu - 1]
        };
        let arc2_start = self.points[pu];

        new_arc1.construct_from_start_end_center(
            current_arc.get_p0(),
            arc1_end,
            current_arc.get_center(),
            current_arc.is_clockwise(),
            0.0,
        );

        new_arc2.construct_from_start_end_center(
            arc2_start,
            current_arc.get_p1(),
            current_arc.get_center(),
            current_arc.is_clockwise(),
            0.0,
        );

        if !a_coincident && self.arc_index(pu - 1) != curr_arc_idx {
            //Ignore newArc1 as it has zero points
            self.arcs[curr_arc_idx as usize] = new_arc2;
        } else {
            self.arcs[curr_arc_idx as usize] = new_arc1;
            self.arcs.insert(curr_arc_idx as usize + 1, new_arc2);

            if a_coincident {
                self.shapes[pu].1 = curr_arc_idx + 1;
                a_pt_index += 1;
            }

            // Only change the arc indices for the second half of the point range
            for i in a_pt_index..self.point_count() as isize {
                run_on_pair(&mut self.shapes[i as usize], |a_index| {
                    if *a_index != Self::SHAPE_IS_PT {
                        *a_index += 1;
                    }
                });
            }
        }
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

    /// Port of `SHAPE_LINE_CHAIN::ShapeCount()`: the number of shapes (line segments or arcs)
    /// in this line chain.  This is equal to `segment_count()` if the chain has no arcs.
    pub fn shape_count(&self) -> i32 {
        if self.points.len() != self.shapes.len() {
            // wxCHECK2_MSG: "Invalid chain!"
            return 0;
        }

        if self.points.len() < 2 {
            return 0;
        }

        let mut num_shapes = 1;
        let mut i = self.next_shape(0);

        while i != -1 {
            num_shapes += 1;
            i = self.next_shape(i);
        }

        num_shapes
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

    /// Port of `SHAPE_LINE_CHAIN::NextShape()`: the vertex index of the next shape in the
    /// chain, or -1 if `a_point_index` is the last shape.
    ///
    /// If `a_point_index` is the start of a segment, this will be ( `a_point_index` + 1 ).  If
    /// it is the start of an arc, the next index will be the one immediately following the arc
    /// end.
    pub fn next_shape(&self, a_point_index: i32) -> i32 {
        let mut a_point_index = a_point_index;

        if a_point_index < 0 {
            a_point_index += self.point_count();
        }

        if a_point_index < 0 {
            return -1;
        }

        let last_index = self.point_count() - 1;

        // Last point?
        if a_point_index >= last_index {
            return -1; // we don't want to wrap around
        }

        if self.shapes[a_point_index as usize] == Self::SHAPES_ARE_PT {
            if a_point_index == last_index - 1 {
                if self.closed {
                    return last_index;
                } else {
                    return -1;
                }
            } else {
                return a_point_index + 1;
            }
        }

        let arc_start = a_point_index;

        // The second element should only get populated when the point is shared between two shapes.
        // If not a shared point, then the index should always go on the first element.
        if self.shapes[a_point_index as usize].0 == Self::SHAPE_IS_PT {
            // wxCHECK2_MSG: "malformed chain!"
            return -1;
        }

        let current_arc_idx = self.arc_index(a_point_index as usize);

        // Now skip the rest of the arc
        while a_point_index < last_index
            && self.arc_index(a_point_index as usize) == current_arc_idx
        {
            a_point_index += 1;
        }

        let sh = self.shapes[a_point_index as usize];
        let index_still_on_arc = sh.0 == current_arc_idx || sh.1 == current_arc_idx;

        // We want the last vertex of the arc if the initial point was the start of one
        // Well-formed arcs should generate more than one point to travel above
        if a_point_index - arc_start > 1 && !index_still_on_arc {
            a_point_index -= 1;
        }

        if a_point_index == last_index {
            if !self.closed || self.is_arc_segment(a_point_index as usize) {
                return -1; //no shape
            } else {
                return last_index; // Segment between last point and the start of the chain
            }
        }

        a_point_index
    }

    /// Port of `SHAPE_LINE_CHAIN::SetPoint()`: move a point to a specific location.  An arc
    /// the point belongs to is converted to plain points (only the first one of a shared
    /// point: the conversion renumbers the second, as in C++).
    pub fn set_point(&mut self, a_index: i32, a_pos: Vector2I) {
        let mut a_index = a_index;

        if a_index < 0 {
            a_index += self.point_count();
        } else if a_index >= self.point_count() {
            a_index -= self.point_count();
        }

        let idx = a_index as usize;
        self.points[idx] = a_pos;

        // alg::run_on_pair( m_shapes[aIndex], ... ): the lambda sees the element after the
        // first conversion has modified it
        let first = self.shapes[idx].0;

        if first != Self::SHAPE_IS_PT {
            self.convert_arc(first);
        }

        let second = self.shapes[idx].1;

        if second != Self::SHAPE_IS_PT {
            self.convert_arc(second);
        }
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

    /// Port of `SHAPE_LINE_CHAIN::Reverse()`: a copy of the line chain with the order of
    /// points (and arcs) reversed.
    pub fn reverse(&self) -> ShapeLineChain {
        let mut a = self.clone();

        a.points.reverse();
        a.shapes.reverse();
        a.arcs.reverse();

        let arc_count = a.arcs.len() as isize;

        for sh in &mut a.shapes {
            if *sh != Self::SHAPES_ARE_PT {
                run_on_pair(sh, |a_shape_index| {
                    if *a_shape_index != Self::SHAPE_IS_PT {
                        *a_shape_index = arc_count - *a_shape_index - 1;
                    }
                });

                if sh.1 != Self::SHAPE_IS_PT {
                    // If the second element is populated, the first one should be too!
                    debug_assert!(sh.0 != Self::SHAPE_IS_PT);

                    // Switch round first and second in shared points, as part of reversing the chain
                    std::mem::swap(&mut sh.0, &mut sh.1);
                }
            }
        }

        for arc in &mut a.arcs {
            arc.reverse();
        }

        a.closed = self.closed;

        a
    }

    /// Port of `SHAPE_LINE_CHAIN::ClearArcs()`: remove all arc references in the line chain,
    /// resulting in a chain formed only of straight segments.  Any arcs in the chain are
    /// removed and only the polygonal approximation is left.
    pub fn clear_arcs(&mut self) {
        let mut arc_index = self.arcs.len() as isize - 1;

        while arc_index >= 0 {
            self.convert_arc(arc_index);
            arc_index -= 1;
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Length()`: the length (this is the sum of all segment and
    /// arc lengths).
    pub fn length(&self) -> i64 {
        let mut l: i64 = 0;

        for i in 0..self.segment_count() {
            // Only include segments that aren't part of arc shapes
            if !self.is_arc_segment(i as usize) {
                l += self.c_segment(i).length() as i64;
            }
        }

        for arc in &self.arcs {
            // long long += double: evaluated in double, converted back
            l = f64_to_i64_x86(l as f64 + arc.get_length());
        }

        l
    }

    /// Port of `SHAPE_LINE_CHAIN::ReservePoints()`: allocate a number of points all at once
    /// (for performance).
    pub fn reserve_points(&mut self, a_size: usize) {
        self.points.reserve(a_size);
        self.shapes.reserve(a_size);
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
            let mut retval = a_shape_indices;

            run_on_pair(&mut retval, |a_index| {
                if *a_index != Self::SHAPE_IS_PT {
                    *a_index += num_arcs;
                }
            });

            retval
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
            let mut arc = a_arc.clone();
            arc.set_width(0);
            chain.arcs.push(arc);

            for sh in &mut chain.shapes {
                sh.0 = 0;
            }
        }

        self.append_chain(&chain);

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Insert( size_t aVertex, const VECTOR2I& aP )`.
    pub fn insert_point(&mut self, a_vertex: usize, a_p: Vector2I) {
        if a_vertex == self.points.len() {
            self.append_point(a_p, false);
            return;
        }

        if a_vertex >= self.points.len() {
            // wxCHECK
            return;
        }

        if a_vertex > 0 && self.is_pt_on_arc(a_vertex) {
            self.split_arc(a_vertex as isize, false);
        }

        //@todo need to check we aren't creating duplicate points
        self.points.insert(a_vertex, a_p);
        self.shapes.insert(a_vertex, Self::SHAPES_ARE_PT);

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Insert( size_t aVertex, const SHAPE_ARC& aArc )`.
    pub fn insert_arc(&mut self, a_vertex: usize, a_arc: &ShapeArc) {
        self.insert_arc_with_error(a_vertex, a_arc, get_arc_polygonization_max_error());
    }

    /// Port of `SHAPE_LINE_CHAIN::Insert( size_t aVertex, const SHAPE_ARC& aArc,
    /// int aMaxError )`.
    ///
    /// The C++ searches the position of the new arc with a reverse iteration over the shapes
    /// that ends at `m_shapes.rend() + aVertex`, i.e. it reads `aVertex` elements before the
    /// beginning of the array (undefined behaviour) for every `a_vertex > 0`.  The port only
    /// visits the elements of the array (exactly the C++ behaviour for `a_vertex == 0`); see
    /// `PORT_STATUS.md`.
    pub fn insert_arc_with_error(&mut self, a_vertex: usize, a_arc: &ShapeArc, a_max_error: i32) {
        if a_vertex >= self.points.len() {
            // wxCHECK
            return;
        }

        if a_vertex > 0 && self.is_pt_on_arc(a_vertex) {
            self.split_arc(a_vertex as isize, false);
        }

        // Step 1: Find the position for the new arc in the existing arc vector
        let mut arc_pos = self.arcs.len() as isize;

        for sh in self.shapes.iter().rev() {
            if *sh != Self::SHAPES_ARE_PT {
                arc_pos = sh.0.max(sh.1);
                arc_pos += 1;
            }
        }

        //Increment all arc indices before inserting the new arc
        for sh in &mut self.shapes {
            run_on_pair(sh, |a_index| {
                if *a_index >= arc_pos {
                    *a_index += 1;
                }
            });
        }

        let mut arc_copy = a_arc.clone();
        arc_copy.set_width(0);
        self.arcs.insert(arc_pos as usize, arc_copy);

        // Step 2: Add the arc polyline points to the chain
        //@todo need to check we aren't creating duplicate points at start or end
        let chain = a_arc.convert_to_polyline(a_max_error);
        self.points
            .splice(a_vertex..a_vertex, chain.c_points().iter().copied());

        // Step 3: Add the vector of indices to the shape vector
        //@todo need to check we aren't creating duplicate points at start or end
        let new_points = vec![(arc_pos, Self::SHAPE_IS_PT); chain.point_count() as usize];
        self.shapes.splice(a_vertex..a_vertex, new_points);

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Replace( int aStartIndex, int aEndIndex,
    /// const VECTOR2I& aP )`: replace points with indices in range [start_index, end_index]
    /// with a single point `a_p`.
    pub fn replace_point(&mut self, a_start_index: i32, a_end_index: i32, a_p: Vector2I) {
        self.remove_range(a_start_index, a_end_index);
        // Insert( size_t ): a negative index converts to a huge one
        self.insert_point(a_start_index as usize, a_p);
        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Replace( int aStartIndex, int aEndIndex,
    /// const SHAPE_LINE_CHAIN& aLine )`: replace points with indices in range
    /// [start_index, end_index] with the points from line chain `a_line`.
    pub fn replace_chain(&mut self, a_start_index: i32, a_end_index: i32, a_line: &ShapeLineChain) {
        let mut a_start_index = a_start_index;
        let mut a_end_index = a_end_index;

        if a_end_index < 0 {
            a_end_index += self.point_count();
        }

        if a_start_index < 0 {
            a_start_index += self.point_count();
        }

        // We only process lines in order in this house
        debug_assert!(a_start_index <= a_end_index);
        debug_assert!(a_end_index < self.points.len() as i32);

        let mut new_line = a_line.clone();

        // Zero points to add?
        if new_line.point_count() == 0 {
            self.remove_range(a_start_index, a_end_index);
            return;
        }

        // Remove coincident points in the new line
        if new_line.points[0] == self.points[a_start_index as usize] {
            a_start_index += 1;
            new_line.remove(0);

            // Zero points to add?
            if new_line.point_count() == 0 {
                self.remove_range(a_start_index, a_end_index);
                return;
            }
        }

        if *new_line.points.last().unwrap() == self.points[a_end_index as usize] && a_end_index > 0
        {
            a_end_index -= 1;
            new_line.remove(-1);
        }

        self.remove_range(a_start_index, a_end_index);

        // Zero points to add?
        if new_line.point_count() == 0 {
            return;
        }

        // The total new arcs index is added to the new arc indices
        let prev_arc_count = self.arcs.len() as isize;
        let mut new_shapes = new_line.shapes.clone();

        for shape_pair in &mut new_shapes {
            run_on_pair(shape_pair, |a_shape| {
                if *a_shape != Self::SHAPE_IS_PT {
                    *a_shape += prev_arc_count;
                }
            });
        }

        // (For a closed chain whose first and last points were merged by the removal, the C++
        // inserts past the end of the arrays: undefined behaviour; the port appends.)
        let at = (a_start_index as usize).min(self.points.len());
        self.shapes.splice(at..at, new_shapes);
        self.points.splice(at..at, new_line.points.iter().copied());
        self.arcs.extend(new_line.arcs.iter().cloned());

        debug_assert!(self.shapes.len() == self.points.len());
    }

    /// Port of `SHAPE_LINE_CHAIN::Remove( int aStartIndex, int aEndIndex )`: remove the range
    /// of points [start_index, end_index] from the line chain.  Negative indices count from the
    /// end.
    ///
    /// (A start index that stays negative after the adjustment makes the C++ access the arrays
    /// out of bounds; the port returns without change.)
    pub fn remove_range(&mut self, a_start_index: i32, a_end_index: i32) {
        if self.shapes.len() != self.points.len() {
            // wxCHECK
            return;
        }

        // Unwrap the chain first (correctly handling removing arc at
        // end of chain coincident with start)
        let closed_state = self.is_closed();
        self.set_closed(false);

        let mut a_start_index = a_start_index;
        let mut a_end_index = a_end_index;

        if a_end_index < 0 {
            a_end_index += self.point_count();
        }

        if a_start_index < 0 {
            a_start_index += self.point_count();
        }

        if a_start_index >= self.point_count()
            || a_end_index >= self.point_count()
            || a_start_index > a_end_index
            || a_start_index < 0
        {
            self.set_closed(closed_state);
            return;
        }

        // Split arcs, making arcs coincident
        if !self.is_arc_start(a_start_index as usize) && self.is_pt_on_arc(a_start_index as usize) {
            self.split_arc(a_start_index as isize, false);
        }

        if self.is_shared_pt(a_start_index as usize) {
            // Don't delete the shared point
            a_start_index += 1;
        }

        if !self.is_arc_end(a_end_index as usize)
            && self.is_pt_on_arc(a_end_index as usize)
            && a_end_index < self.point_count() - 1
        {
            self.split_arc(a_end_index as isize + 1, true);
        }

        if self.is_shared_pt(a_end_index as usize) {
            // Don't delete the shared point
            a_end_index -= 1;
        }

        if a_start_index > a_end_index {
            self.set_closed(closed_state);
            return;
        }

        let mut extra_arcs: BTreeSet<usize> = BTreeSet::new();
        let mut log_arc_idx_removal = |a_shape_index: isize| {
            if a_shape_index != Self::SHAPE_IS_PT {
                extra_arcs.insert(a_shape_index as usize);
            }
        };

        // Remove any overlapping arcs in the point range
        for i in a_start_index..=a_end_index {
            let iu = i as usize;

            if self.is_shared_pt(iu) {
                if i == a_start_index {
                    // Only remove the arc on the second index
                    log_arc_idx_removal(self.shapes[iu].1);

                    // Ensure that m_shapes has been built correctly.
                    debug_assert!(i < a_end_index || self.shapes[iu + 1].0 == self.shapes[iu].1);

                    continue;
                } else if i == a_end_index {
                    // Only remove the arc on the first index
                    log_arc_idx_removal(self.shapes[iu].0);
                    continue;
                }
            } else {
                log_arc_idx_removal(self.shapes[iu].0);
                log_arc_idx_removal(self.shapes[iu].1);
            }
        }

        // (in increasing order; each conversion renumbers the arcs after it, as in C++)
        for arc in extra_arcs {
            self.convert_arc(arc as isize);
        }

        let range = a_start_index as usize..a_end_index as usize + 1;
        self.shapes.drain(range.clone());
        self.points.drain(range);
        debug_assert!(self.shapes.len() == self.points.len());

        self.set_closed(closed_state);
    }

    /// Port of `SHAPE_LINE_CHAIN::Remove( int aIndex )`: remove the `a_index`-th point from the
    /// line chain.  Negative indices count from the end.
    pub fn remove(&mut self, a_index: i32) {
        self.remove_range(a_index, a_index);
    }

    /// Port of `SHAPE_LINE_CHAIN::RemoveShape()`: remove the shape at the given index from the
    /// line chain.  If the given index is inside an arc, the entire arc will be removed.
    pub fn remove_shape(&mut self, a_point_index: i32) {
        let mut a_point_index = a_point_index;

        if a_point_index < 0 {
            a_point_index += self.point_count();
        }

        if a_point_index >= self.point_count() || a_point_index < 0 {
            return; // Invalid index, fail gracefully
        }

        if self.shapes[a_point_index as usize] == Self::SHAPES_ARE_PT {
            self.remove(a_point_index);
            return;
        }

        let mut start = a_point_index;
        let mut end = a_point_index;
        let arc_idx = self.arc_index(a_point_index as usize) as i32;

        if !self.is_arc_start(start as usize) {
            // aPointIndex is not a shared point, so iterate backwards to find the start of the arc
            while start > 0 && self.arc_index(start as usize - 1) as i32 == arc_idx {
                start -= 1;
            }
        }

        if !self.is_arc_end(end as usize) || start == end {
            end = self.next_shape(end); // can be -1 to indicate end of chain
        }

        self.remove_range(start, end);
    }

    /// Port of `SHAPE_LINE_CHAIN::Split( const VECTOR2I& aP, bool aExact )`: insert the point
    /// `a_p` belonging to one of our segments, splitting the adjacent segment in two.
    ///
    /// Returns the index of the newly inserted point (or a pre-existing point that matches),
    /// or -1 if the point is not on any segment.
    pub fn split(&mut self, a_p: Vector2I, a_exact: bool) -> i32 {
        let mut ii: i32 = -1;
        let mut min_dist: i32 = 2;

        let found_index = self.find(a_p, 0);

        if found_index >= 0 && a_exact {
            return found_index;
        }

        for s in 0..self.segment_count() {
            let seg = self.c_segment(s);
            let dist = seg.distance(a_p);

            // make sure we are not producing a 'slightly concave' primitive. This might happen
            // if aP lies very close to one of already existing points.
            if dist < min_dist && seg.a != a_p && seg.b != a_p {
                min_dist = dist;

                if found_index < 0 || s < found_index {
                    ii = s;
                }
            }
        }

        if ii < 0 {
            ii = found_index;
        }

        if ii >= 0 {
            // Don't create duplicate points
            if self.get_point(ii) == a_p {
                return ii;
            }

            let new_index = ii as usize + 1;

            if self.is_arc_segment(ii as usize) {
                self.points.insert(new_index, a_p);
                let arc = self.arc_index(ii as usize);
                self.shapes.insert(new_index, (arc, Self::SHAPE_IS_PT));
                self.split_arc(new_index as isize, true); // Make the inserted point a shared point
            } else {
                self.insert_point(new_index, a_p);
            }

            return new_index as i32;
        }

        -1
    }

    /// Port of `SHAPE_LINE_CHAIN::Find()`: search for point `a_p` (within `a_threshold`).
    /// Returns the index of the corresponding point, or -1 if `a_p` is not part of this line
    /// chain.
    pub fn find(&self, a_p: Vector2I, a_threshold: i32) -> i32 {
        for s in 0..self.point_count() {
            if a_threshold == 0 {
                if self.c_point(s) == a_p {
                    return s;
                }
            } else if (self.c_point(s) - a_p).euclidean_norm() <= a_threshold {
                return s;
            }
        }

        -1
    }

    /// Port of `SHAPE_LINE_CHAIN::FindSegment()`: search for segment containing point `a_p`.
    /// (The C++ default threshold is 1.)
    pub fn find_segment(&self, a_p: Vector2I, a_threshold: i32) -> i32 {
        for s in 0..self.segment_count() {
            if self.c_segment(s).distance(a_p) <= a_threshold {
                return s;
            }
        }

        -1
    }

    /// Port of `SHAPE_LINE_CHAIN::Slice( int aStartIndex, int aEndIndex )`: a subset of this
    /// line chain containing the [start_index, end_index] range of points.
    pub fn slice(&self, a_start_index: i32, a_end_index: i32) -> ShapeLineChain {
        self.slice_with_error(
            a_start_index,
            a_end_index,
            get_arc_polygonization_max_error(),
        )
    }

    /// Port of `SHAPE_LINE_CHAIN::Slice( int aStartIndex, int aEndIndex, int aMaxError )`.
    pub fn slice_with_error(
        &self,
        a_start_index: i32,
        a_end_index: i32,
        a_max_error: i32,
    ) -> ShapeLineChain {
        let mut rv = ShapeLineChain::new();
        let mut a_start_index = a_start_index;
        let mut a_end_index = a_end_index;

        if a_end_index < 0 {
            a_end_index += self.point_count();
        }

        if a_start_index < 0 {
            a_start_index += self.point_count();
        }

        // Bad programmer checks
        if a_start_index < 0
            || a_end_index < 0
            || a_start_index >= self.point_count()
            || a_end_index >= self.point_count()
            || a_end_index < a_start_index
        {
            // wxCHECK
            return ShapeLineChain::new();
        }

        let num_points = self.point_count();

        if self.is_arc_segment(a_start_index as usize) && !self.is_arc_start(a_start_index as usize)
        {
            if a_start_index == a_end_index {
                rv.append_point(self.points[a_start_index as usize], false);
                return rv;
            }

            // Cutting in middle of an arc, lets split it
            let arc_to_split_index = self.arc_index(a_start_index as usize);
            let arc_to_split = self.arc(arc_to_split_index as usize);

            // Copy the points as arc points
            let mut i = a_start_index;

            while i <= a_end_index && arc_to_split_index == self.arc_index(i as usize) {
                rv.points.push(self.points[i as usize]);
                rv.shapes.push((rv.arcs.len() as isize, Self::SHAPE_IS_PT));
                rv.bbox.merge_point(self.points[i as usize]);
                i += 1;
            }

            // Create a new arc from the existing one, with different start point.
            let mut new_arc = ShapeArc::new();
            let new_arc_start = self.points[a_start_index as usize];

            // The slice may also finish inside this arc
            let new_arc_end = *rv.points.last().unwrap();

            new_arc.construct_from_start_end_center(
                new_arc_start,
                new_arc_end,
                arc_to_split.get_center(),
                arc_to_split.is_clockwise(),
                0.0,
            );

            rv.arcs.push(new_arc);

            a_start_index += rv.point_count();
        }

        let mut i = a_start_index;

        while i <= a_end_index && i < num_points {
            if i == -1 {
                return rv; // NextShape reached the end
            }

            let next_shape = self.next_shape(i);
            let is_last_shape = next_shape < 0;

            if self.is_arc_start(i as usize) {
                if (is_last_shape && a_end_index != (num_points - 1)) || (next_shape > a_end_index)
                {
                    if i == a_end_index {
                        // Single point on an arc, just append the single point
                        rv.append_point(self.points[i as usize], false);
                        return rv;
                    }

                    // Cutting in middle of an arc, lets split it
                    let arc_index = self.arc_index(i as usize);
                    let current_arc = self.arc(arc_index as usize);

                    // Copy the points as arc points
                    while i <= a_end_index && i < num_points {
                        if arc_index != self.arc_index(i as usize) {
                            break;
                        }

                        rv.points.push(self.points[i as usize]);
                        rv.shapes.push((rv.arcs.len() as isize, Self::SHAPE_IS_PT));
                        rv.bbox.merge_point(self.points[i as usize]);
                        i += 1;
                    }

                    // Create a new arc from the existing one, with different end point.
                    let mut new_arc = ShapeArc::new();
                    let new_arc_end = self.points[a_end_index as usize];

                    new_arc.construct_from_start_end_center(
                        current_arc.get_p0(),
                        new_arc_end,
                        current_arc.get_center(),
                        current_arc.is_clockwise(),
                        0.0,
                    );

                    rv.arcs.push(new_arc);

                    return rv;
                } else {
                    // append the whole arc
                    let current_arc = self.arc(self.arc_index(i as usize) as usize);
                    rv.append_arc_with_error(current_arc, a_max_error);
                }

                if is_last_shape {
                    return rv;
                }
            } else {
                debug_assert!(
                    !self.is_arc_segment(i as usize),
                    "Still on an arc segment, we missed something..."
                );

                if i == a_start_index {
                    rv.append_point(self.points[i as usize], false);
                }

                let next_point_is_arc = if is_last_shape {
                    false
                } else {
                    self.is_arc_segment(next_shape as usize)
                };

                if !next_point_is_arc && i < self.segment_count() && i < a_end_index {
                    rv.append_point(self.get_segment(i).b, false);
                }
            }

            i = self.next_shape(i);
        }

        debug_assert!(rv.points.len() == rv.shapes.len());

        rv
    }

    /// Port of `SHAPE_LINE_CHAIN::Intersects( const SEG& )`: check if the line chain
    /// intersects the segment.
    pub fn intersects_seg(&self, a_seg: &Seg) -> bool {
        let seg_count = self.segment_count();
        let pt_count = self.points.len() as i32;

        let seg_min_x = a_seg.a.x.min(a_seg.b.x);
        let seg_max_x = a_seg.a.x.max(a_seg.b.x);
        let seg_min_y = a_seg.a.y.min(a_seg.b.y);
        let seg_max_y = a_seg.a.y.max(a_seg.b.y);

        for s in 0..seg_count {
            let pt_a = self.points[s as usize];
            let pt_b = self.points[if s + 1 < pt_count { s as usize + 1 } else { 0 }];

            if pt_a.x.max(pt_b.x) < seg_min_x
                || pt_a.x.min(pt_b.x) > seg_max_x
                || pt_a.y.max(pt_b.y) < seg_min_y
                || pt_a.y.min(pt_b.y) > seg_max_y
            {
                continue;
            }

            if Seg::new(pt_a, pt_b).intersects(a_seg) {
                return true;
            }
        }

        false
    }

    /// Port of `SHAPE_LINE_CHAIN::Intersects( const SHAPE_LINE_CHAIN& )`: check if the line
    /// chain intersects the other line chain.
    pub fn intersects_chain(&self, a_chain: &ShapeLineChain) -> bool {
        let mut dummy: Vec<Intersection> = Vec::new();
        self.intersect_chain(a_chain, &mut dummy, false, None) != 0
    }

    /// Port of `SHAPE_LINE_CHAIN::Intersect( const SEG& aSeg, std::vector<INTERSECTION>& aIp )`:
    /// find all intersection points between our line chain and the segment `a_seg`.
    ///
    /// The intersections are appended to `a_ip`, then the whole vector is sorted by distance
    /// from `a_seg.a` (libstdc++ `std::sort`).  Returns the size of `a_ip`.
    pub fn intersect_seg(&self, a_seg: &Seg, a_ip: &mut Vec<Intersection>) -> i32 {
        let seg_count = self.segment_count();
        let pt_count = self.points.len() as i32;

        let seg_min_x = a_seg.a.x.min(a_seg.b.x);
        let seg_max_x = a_seg.a.x.max(a_seg.b.x);
        let seg_min_y = a_seg.a.y.min(a_seg.b.y);
        let seg_max_y = a_seg.a.y.max(a_seg.b.y);

        for s in 0..seg_count {
            let pt_a = self.points[s as usize];
            let pt_b = self.points[if s + 1 < pt_count { s as usize + 1 } else { 0 }];

            if pt_a.x.max(pt_b.x) < seg_min_x
                || pt_a.x.min(pt_b.x) > seg_max_x
                || pt_a.y.max(pt_b.y) < seg_min_y
                || pt_a.y.min(pt_b.y) > seg_max_y
            {
                continue;
            }

            if let Some(p) = Seg::with_index(pt_a, pt_b, s).intersect(a_seg) {
                a_ip.push(Intersection {
                    valid: true,
                    index_our: s,
                    index_their: -1,
                    is_corner_our: false,
                    is_corner_their: false,
                    p,
                });
            }
        }

        let comp = CompareOriginDistance::new(a_seg.a);
        std_sort(a_ip, |a, b| comp.compare(a, b));

        a_ip.len() as i32
    }

    /// Port of `SHAPE_LINE_CHAIN::Intersect( const SHAPE_LINE_CHAIN& aChain,
    /// std::vector<INTERSECTION>& aIp, bool aExcludeColinearAndTouching, BOX2I* aChainBBox )`:
    /// find all intersection points between our line chain and the line chain `a_chain`.
    ///
    /// The intersections are appended to `a_ip`; returns the size of `a_ip`.
    pub fn intersect_chain(
        &self,
        a_chain: &ShapeLineChain,
        a_ip: &mut Vec<Intersection>,
        a_exclude_colinear_and_touching: bool,
        a_chain_bbox: Option<&Box2I>,
    ) -> i32 {
        let our_seg_count = self.segment_count();
        let their_seg_count = a_chain.segment_count();

        if our_seg_count == 0 || their_seg_count == 0 {
            return 0;
        }

        let bb_other = match a_chain_bbox {
            Some(b) => Box2IMinMax::from_box(b),
            None => Box2IMinMax::from_box(&Shape::bbox(a_chain, 0)),
        };
        let our_pt_count = self.points.len() as i32;
        let their_pts = a_chain.c_points();
        let their_pt_count = their_pts.len() as i32;

        // Pre-build SEGs for the other chain so each is constructed exactly once
        let mut their_segs: Vec<Seg> = Vec::with_capacity(their_seg_count as usize);

        for i in 0..their_seg_count {
            let pa = their_pts[i as usize];
            let pb = their_pts[if i + 1 < their_pt_count {
                i as usize + 1
            } else {
                0
            }];
            their_segs.push(Seg::with_index(pa, pb, i));
        }

        // Compact extent array for cache-friendly scanning, sorted by minX
        let mut sorted: Vec<SegExtent> = their_segs
            .iter()
            .enumerate()
            .map(|(i, s)| SegExtent {
                min_x: s.a.x.min(s.b.x),
                max_x: s.a.x.max(s.b.x),
                min_y: s.a.y.min(s.b.y),
                max_y: s.a.y.max(s.b.y),
                seg_idx: i as i32,
            })
            .collect();

        std_sort(&mut sorted, |a, b| a.min_x < b.min_x);

        for s1 in 0..our_seg_count {
            let a1 = self.points[s1 as usize];
            let b1 = self.points[if s1 + 1 < our_pt_count {
                s1 as usize + 1
            } else {
                0
            }];

            let our_min_x = a1.x.min(b1.x);
            let our_max_x = a1.x.max(b1.x);
            let our_min_y = a1.y.min(b1.y);
            let our_max_y = a1.y.max(b1.y);

            if our_max_x < bb_other.left
                || our_min_x > bb_other.right
                || our_max_y < bb_other.top
                || our_min_y > bb_other.bottom
            {
                continue;
            }

            let a = Seg::with_index(a1, b1, s1);

            // Find the right boundary in the sorted extents: first entry where minX > ourMaxX.
            // Everything past this point is too far right to overlap.
            let right_end = sorted.partition_point(|e| !(our_max_x < e.min_x));

            for jt in &sorted[..right_end] {
                if jt.max_x < our_min_x || jt.max_y < our_min_y || jt.min_y > our_max_y {
                    continue;
                }

                let b = &their_segs[jt.seg_idx as usize];

                let mut is = Intersection {
                    index_our: s1,
                    index_their: jt.seg_idx,
                    is_corner_our: false,
                    is_corner_their: false,
                    valid: true,
                    ..Intersection::default()
                };

                let p = a.intersect(b);

                if !a_exclude_colinear_and_touching && a.collinear(b) {
                    if a.contains(b.a) {
                        is.p = b.a;
                        is.is_corner_their = true;
                        a_ip.push(is);
                    }

                    if a.contains(b.b) {
                        is.p = b.b;
                        is.index_their += 1;
                        is.is_corner_their = true;
                        a_ip.push(is);
                    }

                    if b.contains(a.a) {
                        is.p = a.a;
                        is.is_corner_our = true;
                        a_ip.push(is);
                    }

                    if b.contains(a.b) {
                        is.p = a.b;
                        is.index_our += 1;
                        is.is_corner_our = true;
                        a_ip.push(is);
                    }
                } else if let Some(p) = p {
                    is.p = p;
                    is.is_corner_our = false;
                    is.is_corner_their = false;

                    if p == a.a {
                        is.is_corner_our = true;
                    }

                    if p == a.b {
                        is.is_corner_our = true;
                        is.index_our += 1;
                    }

                    if p == b.a {
                        is.is_corner_their = true;
                    }

                    if p == b.b {
                        is.is_corner_their = true;
                        is.index_their += 1;
                    }

                    a_ip.push(is);
                }
            }
        }

        a_ip.len() as i32
    }

    /// Port of `SHAPE_LINE_CHAIN::PathLength()`: compute the walk path length from the
    /// beginning of the line chain and the point `a_p` belonging to our line.  If `a_index`
    /// is not negative, it selects the segment of `a_p` instead of a search.
    ///
    /// Returns the path length in Euclidean metric or -1 if `a_p` does not belong to the line
    /// chain.
    pub fn path_length(&self, a_p: Vector2I, a_index: i32) -> i32 {
        let mut sum: i32 = 0;

        for i in 0..self.segment_count() {
            let seg = self.c_segment(i);
            let index_match;

            if a_index >= 0 {
                if a_index == self.segment_count() {
                    index_match = i == self.segment_count() - 1;
                } else {
                    index_match = i == a_index;
                }
            } else {
                index_match = seg.contains(a_p);
            }

            if index_match {
                sum += (a_p - seg.a).euclidean_norm();
                return sum;
            } else {
                sum += seg.length();
            }
        }

        -1
    }

    /// Port of `SHAPE_LINE_CHAIN::CheckClearance()`: check if point `a_p` is closer to (or on)
    /// an edge or vertex of the line chain.
    pub fn check_clearance(&self, a_p: Vector2I, a_dist: i32) -> bool {
        if self.point_count() == 0 {
            return false;
        } else if self.point_count() == 1 {
            return self.points[0].distance(a_p) <= a_dist as f64;
        }

        for i in 0..self.segment_count() {
            let s = self.c_segment(i);

            if s.a == a_p || s.b == a_p {
                return true;
            }

            if s.distance(a_p) <= a_dist {
                return true;
            }
        }

        false
    }

    /// Port of `SHAPE_LINE_CHAIN::SelfIntersecting()`: check if the line chain is
    /// self-intersecting.  Returns the first intersection found (or `None`).
    pub fn self_intersecting(&self) -> Option<Intersection> {
        let seg_count = self.segment_count();
        let pt_count = self.points.len() as i32;
        let closed = self.closed;

        if seg_count < 2 {
            return None;
        }

        // Index of the second endpoint of segment i, handling the closed-chain wrap
        let end_idx = |i: i32| -> usize {
            let next = i + 1;

            if next < pt_count { next as usize } else { 0 }
        };

        for s1 in 0..seg_count {
            let a1 = self.points[s1 as usize];
            let b1 = self.points[end_idx(s1)];

            // Expand by 2 to account for Contains() tolerance (SquaredDistance <= 3)
            let s1_min_x = a1.x.min(b1.x) - 2;
            let s1_max_x = a1.x.max(b1.x) + 2;
            let s1_min_y = a1.y.min(b1.y) - 2;
            let s1_max_y = a1.y.max(b1.y) + 2;

            for s2 in s1 + 1..seg_count {
                let a2 = self.points[s2 as usize];
                let b2 = self.points[end_idx(s2)];

                let s2_min_x = a2.x.min(b2.x);
                let s2_max_x = a2.x.max(b2.x);

                if s1_max_x < s2_min_x || s2_max_x < s1_min_x {
                    continue;
                }

                let s2_min_y = a2.y.min(b2.y);
                let s2_max_y = a2.y.max(b2.y);

                if s1_max_y < s2_min_y || s2_max_y < s1_min_y {
                    continue;
                }

                let seg1 = Seg::with_index(a1, b1, s1);

                if s1 + 1 != s2 && seg1.contains(a2) {
                    return Some(Intersection {
                        index_our: s1,
                        index_their: s2,
                        p: a2,
                        ..Intersection::default()
                    });
                } else if seg1.contains(b2)
                    // for closed polylines, the ending point of the
                    // last segment == starting point of the first segment
                    // this is a normal case, not self intersecting case
                    && !(closed && s1 == 0 && s2 == seg_count - 1)
                {
                    return Some(Intersection {
                        index_our: s1,
                        index_their: s2,
                        p: b2,
                        ..Intersection::default()
                    });
                } else if let Some(p) =
                    seg1.intersect_with(&Seg::with_index(a2, b2, s2), true, false)
                {
                    return Some(Intersection {
                        index_our: s1,
                        index_their: s2,
                        p,
                        ..Intersection::default()
                    });
                }
            }
        }

        None
    }

    /// Port of `SHAPE_LINE_CHAIN::SelfIntersectingWithArcs()`: check if the line chain is
    /// self-intersecting, testing arcs as true arcs.
    pub fn self_intersecting_with_arcs(&self) -> Option<Intersection> {
        let points_close = |a_pt1: Vector2I, a_pt2: Vector2I| -> bool {
            (Vector2D::from(a_pt1) - Vector2D::from(a_pt2)).squared_euclidean_norm() <= 2.0
        };

        let collide_arc_seg =
            |a_arc: &ShapeArc, a_seg: &Seg, a_clearance: i32, a_location: &mut Vector2I| -> bool {
                let center = a_arc.get_center();
                // CIRCLE( center, double radius ): implicit conversion to int
                let circle = Circle::new(center, f64_to_i32_x86(a_arc.get_radius()));

                let candidate_pts = circle.intersect_seg(a_seg);

                for &candidate in &candidate_pts {
                    // Skip shared points
                    if a_arc.get_p1() == a_seg.a && points_close(candidate, a_seg.a) {
                        continue;
                    }

                    if a_seg.b == a_arc.get_p0() && points_close(candidate, a_seg.b) {
                        continue;
                    }

                    let collides =
                        a_arc.collide_point(candidate, a_clearance, None, Some(&mut *a_location));

                    if collides {
                        return true;
                    }
                }

                false
            };

        let collide_arc_arc = |a_arc1: &ShapeArc,
                               a_arc2: &ShapeArc,
                               a_location: &mut Vector2I|
         -> bool {
            let mut candidate_pts: Vec<Vector2I> = Vec::new();

            a_arc1.intersect_arc(a_arc2, &mut candidate_pts);

            for &candidate in &candidate_pts {
                // Skip shared points
                if a_arc1.get_p1() == a_arc2.get_p0() && points_close(candidate, a_arc1.get_p1()) {
                    continue;
                }

                if a_arc2.get_p1() == a_arc1.get_p0() && points_close(candidate, a_arc2.get_p1()) {
                    continue;
                }

                *a_location = candidate;

                return true;
            }

            false
        };

        let collide_seg_seg = |s1: i32, s2: i32, is: &mut Intersection| -> bool {
            let seg1 = self.c_segment(s1);
            let seg2 = self.c_segment(s2);

            let s2a = seg2.a;
            let s2b = seg2.b;

            if s1 + 1 != s2 && seg1.contains(s2a) {
                is.index_our = s1;
                is.index_their = s2;
                is.p = s2a;
                return true;
            } else if seg1.contains(s2b)
                // for closed polylines, the ending point of the
                // last segment == starting point of the first segment
                // this is a normal case, not self intersecting case
                && !(self.is_closed() && s1 == 0 && s2 == self.segment_count() - 1)
            {
                is.index_our = s1;
                is.index_their = s2;
                is.p = s2b;
                return true;
            } else if let Some(p) = seg1.intersect_with(&seg2, true, false) {
                is.index_our = s1;
                is.index_their = s2;
                is.p = p;
                return true;
            }

            false
        };

        let mut is = Intersection::default();

        let mut shape_cache: Vec<ShapeKey> = Vec::new();
        let mut si = 0;

        while si != -1 {
            let arci = self.arc_index(si as usize) as i32;

            shape_cache.push(ShapeKey {
                first_idx: si,
                arc_idx: arci,
                bbox: if arci == -1 {
                    Box2IMinMax::from_seg(&self.c_segment(si))
                } else {
                    Box2IMinMax::from_arc(self.arc(arci as usize))
                },
            });

            si = self.next_shape(si);
        }

        for sk1 in 0..shape_cache.len() {
            for sk2 in sk1 + 1..shape_cache.len() {
                let mut loc = Vector2I::default();
                let k1 = shape_cache[sk1];
                let k2 = shape_cache[sk2];

                if !k1.bbox.intersects(&k2.bbox) {
                    continue;
                }

                if k1.arc_idx == -1 && k2.arc_idx == -1 {
                    if collide_seg_seg(k1.first_idx, k2.first_idx, &mut is) {
                        return Some(is);
                    }
                } else if k1.arc_idx != -1 && k2.arc_idx == -1 {
                    if collide_arc_seg(
                        self.arc(k1.arc_idx as usize),
                        &self.c_segment(k2.first_idx),
                        0,
                        &mut loc,
                    ) {
                        is.index_our = k1.first_idx;
                        is.index_their = k2.first_idx;
                        is.p = loc;
                        return Some(is);
                    }
                } else if k1.arc_idx == -1 && k2.arc_idx != -1 {
                    if collide_arc_seg(
                        self.arc(k2.arc_idx as usize),
                        &self.c_segment(k1.first_idx),
                        0,
                        &mut loc,
                    ) {
                        is.index_our = k1.first_idx;
                        is.index_their = k2.first_idx;
                        is.p = loc;
                        return Some(is);
                    }
                } else if k1.arc_idx != -1
                    && k2.arc_idx != -1
                    && collide_arc_arc(
                        self.arc(k1.arc_idx as usize),
                        self.arc(k2.arc_idx as usize),
                        &mut loc,
                    )
                {
                    is.index_our = k1.first_idx;
                    is.index_their = k2.first_idx;
                    is.p = loc;
                    return Some(is);
                }
            }
        }

        None
    }

    /// Port of `SHAPE_LINE_CHAIN::NearestPoint( const VECTOR2I& aP,
    /// bool aAllowInternalShapePoints )`: find a point on the line chain that is closest to
    /// point `a_p`.
    ///
    /// If `a_allow_internal_shape_points` is false, a point inside an arc snaps to the nearer
    /// end of the arc.  (The C++ default is true.)
    pub fn nearest_point(&self, a_p: Vector2I, a_allow_internal_shape_points: bool) -> Vector2I {
        if self.point_count() == 0 {
            // The only right answer here is "don't crash".
            return Vector2I::new(0, 0);
        }

        let mut min_d: i32 = i32::MAX;
        let mut nearest: i32 = 0;

        for i in 0..self.segment_count() {
            let d = self.c_segment(i).distance(a_p);

            if d < min_d {
                min_d = d;
                nearest = i;
            }
        }

        if !a_allow_internal_shape_points {
            //Snap to arc end points if the closest found segment is part of an arc segment
            if nearest > 0 && nearest < self.point_count() && self.is_arc_segment(nearest as usize)
            {
                let pt_to_seg_start = self.c_segment(nearest).a - a_p;
                let pt_to_seg_end = self.c_segment(nearest).b - a_p;

                if pt_to_seg_start.euclidean_norm() > pt_to_seg_end.euclidean_norm() {
                    nearest += 1;
                }

                // The closing segment of a closed chain ends at point 0: the C++ reads
                // m_shapes[PointCount()] here (out of bounds); the port uses point 0
                if nearest == self.point_count() {
                    nearest = 0;
                }

                // Is this the start or end of an arc?  If so, return it directly
                if self.is_arc_start(nearest as usize) || self.is_arc_end(nearest as usize) {
                    return self.points[nearest as usize];
                } else {
                    let nearest_arc = self.arc(self.arc_index(nearest as usize) as usize);
                    let pt_to_arc_start = nearest_arc.get_p0() - a_p;
                    let pt_to_arc_end = nearest_arc.get_p1() - a_p;

                    if pt_to_arc_start.euclidean_norm() > pt_to_arc_end.euclidean_norm() {
                        return nearest_arc.get_p1();
                    } else {
                        return nearest_arc.get_p0();
                    }
                }
            }
        }

        self.c_segment(nearest).nearest_point(a_p)
    }

    /// Port of `SHAPE_LINE_CHAIN::NearestPoint( const SEG& aSeg, int& dist )`: find a point on
    /// the line chain that is closest to the line defined by the points of segment `a_seg`,
    /// also returning the distance in `dist` (which is left untouched for an empty chain, as
    /// in C++).
    pub fn nearest_point_seg(&self, a_seg: &Seg, dist: &mut i32) -> Vector2I {
        if self.point_count() == 0 {
            // The only right answer here is "don't crash".
            return Vector2I::new(0, 0);
        }

        let mut nearest: i32 = 0;

        *dist = i32::MAX;

        for i in 0..self.point_count() {
            let d = a_seg.line_distance(self.c_point(i));

            if d < *dist {
                *dist = d;
                nearest = i;
            }
        }

        self.c_point(nearest)
    }

    /// Port of `SHAPE_LINE_CHAIN::NearestSegment()`: find the segment nearest the given point.
    pub fn nearest_segment(&self, a_p: Vector2I) -> i32 {
        let mut min_d: i32 = i32::MAX;
        let mut nearest: i32 = 0;

        for i in 0..self.segment_count() {
            let d = self.c_segment(i).distance(a_p);

            if d < min_d {
                min_d = d;
                nearest = i;
            }
        }

        nearest
    }

    /// Port of `SHAPE_LINE_CHAIN::CompareGeometry()`: compare the (simplified) points of two
    /// chains within `a_epsilon`; with `a_cyclical_compare` the vertices are sorted by angle
    /// around their centroid first, so the starting vertex and direction do not matter.
    /// (The C++ defaults are `false` and 0.)
    pub fn compare_geometry(
        &self,
        a_other: &ShapeLineChain,
        a_cyclical_compare: bool,
        a_epsilon: i32,
    ) -> bool {
        let mut a = self.clone();
        let mut b = a_other.clone();
        a.simplify(0);
        b.simplify(0);

        if a.points.len() != b.points.len() {
            return false;
        }

        if a_cyclical_compare {
            let mut a_verts = a.points.clone();
            let mut b_verts = b.points.clone();

            let centroid = |pts: &[Vector2I]| -> (f64, f64) {
                let mut sx = 0.0;
                let mut sy = 0.0;

                for p in pts {
                    sx += p.x as f64;
                    sy += p.y as f64;
                }

                (sx / pts.len() as f64, sy / pts.len() as f64)
            };

            let a_c = centroid(&a_verts);
            let b_c = centroid(&b_verts);

            let angle_cmp = |c: (f64, f64), p1: &Vector2I, p2: &Vector2I| -> bool {
                let a1 = atan2(p1.y as f64 - c.1, p1.x as f64 - c.0);
                let a2 = atan2(p2.y as f64 - c.1, p2.x as f64 - c.0);
                a1 < a2
            };

            // sort by angle around centroid so that cyclic vertex order doesn't matter
            std_sort(&mut a_verts, |p1, p2| angle_cmp(a_c, p1, p2));
            std_sort(&mut b_verts, |p1, p2| angle_cmp(b_c, p1, p2));

            for i in 0..a_verts.len() {
                if (a_verts[i].x - b_verts[i].x).abs() > a_epsilon
                    || (a_verts[i].y - b_verts[i].y).abs() > a_epsilon
                {
                    return false;
                }
            }
        } else {
            for i in 0..a.point_count() {
                if (a.c_point(i).x - b.c_point(i).x).abs() > a_epsilon
                    || (a.c_point(i).y - b.c_point(i).y).abs() > a_epsilon
                {
                    return false;
                }
            }
        }

        true
    }

    /// Port of `SHAPE_LINE_CHAIN::Parse()`: read `n_pts closed n_arcs`, then `x y arc_index`
    /// per point and `center.x center.y start.x start.y angle` per arc from `a_stream`.
    ///
    /// As in C++, only the point array is cleared before reading (the shape and arc arrays are
    /// appended to), and the result of failed extractions is not checked (see
    /// [`StringStream`]).  Returns false if one of the counts exceeds the length of the text.
    pub fn parse(&mut self, a_stream: &mut StringStream<'_>) -> bool {
        self.points.clear();
        let n_pts = a_stream.read_usize();

        // Rough sanity check, just make sure the loop bounds aren't absolutely outlandish
        if n_pts > a_stream.str_size() {
            return false;
        }

        self.closed = a_stream.read_bool();
        let n_arcs = a_stream.read_usize();

        if n_arcs > a_stream.str_size() {
            return false;
        }

        for _ in 0..n_pts {
            let x = a_stream.read_i32();
            let y = a_stream.read_i32();
            self.points.push(Vector2I::new(x, y));

            let ind = a_stream.read_isize();
            self.shapes.push((ind, Self::SHAPE_IS_PT));
        }

        for _ in 0..n_arcs {
            let pcx = a_stream.read_i32();
            let pcy = a_stream.read_i32();
            let p0x = a_stream.read_i32();
            let p0y = a_stream.read_i32();
            let angle = a_stream.read_f64();

            self.arcs.push(ShapeArc::from_center_start_angle(
                Vector2I::new(pcx, pcy),
                Vector2I::new(p0x, p0y),
                EdaAngle::new(angle, DEGREES_T),
                0,
            ));
        }

        true
    }

    /// Port of `SHAPE_LINE_CHAIN::PointAlong()`: the point at the given path length from the
    /// start of the chain (or the last point).
    pub fn point_along(&self, a_path_length: i32) -> Vector2I {
        let mut total: i32 = 0;

        if a_path_length == 0 {
            return self.c_point(0);
        }

        for i in 0..self.segment_count() {
            let s = self.c_segment(i);
            let l = s.length();

            if total + l >= a_path_length {
                let d = s.b - s.a;
                return s.a + d.resize(a_path_length - total);
            }

            total += l;
        }

        self.c_last_point()
    }

    /// Port of `SHAPE_LINE_CHAIN::Area()`: the area of a closed chain (0 for an open one);
    /// with `a_absolute` false, the sign gives the orientation.  (The C++ default is true.)
    pub fn area(&self, a_absolute: bool) -> f64 {
        // see https://www.mathopenref.com/coordpolygonarea2.html

        if !self.closed {
            return 0.0;
        }

        let mut area = 0.0;
        let size = self.points.len() as i32;
        let mut j = size - 1;

        for i in 0..size {
            let pj = self.points[j as usize];
            let pi = self.points[i as usize];
            area += (pj.x as f64 + pi.x as f64) * (pj.y as f64 - pi.y as f64);
            j = i;
        }

        if a_absolute {
            (area * 0.5).abs() // The result would be negative if points are anti-clockwise
        } else {
            -area * 0.5 // The result would be negative if points are anti-clockwise
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::RemoveDuplicatePoints()`: remove the duplicate points from
    /// the line chain.
    pub fn remove_duplicate_points(&mut self) {
        // Always try to keep at least 2 points otherwise, we're not really a line
        if self.point_count() < 3 {
            return;
        } else if self.point_count() == 3 {
            if self.points[0] == self.points[1] || self.points[1] == self.points[2] {
                self.remove(1);
            }

            return;
        }

        let (pts_unique, shapes_unique) = self.unique_points();

        self.points = pts_unique;
        self.shapes = shapes_unique;
    }

    /// Stage "eliminate duplicate vertices" shared by `RemoveDuplicatePoints()` and
    /// `Simplify2()`.
    fn unique_points(&self) -> (Vec<Vector2I>, Vec<(isize, isize)>) {
        let mut pts_unique: Vec<Vector2I> = Vec::new();
        let mut shapes_unique: Vec<(isize, isize)> = Vec::new();

        let np = self.point_count();
        let mut i: i32 = 0;

        while i < np {
            let mut j = i + 1;
            let (iu, pt) = (i as usize, Self::SHAPES_ARE_PT);

            // We can eliminate duplicate vertices as long as they are part of the same shape, OR if
            // one of them is part of a shape and one is not.
            while j < np
                && self.points[iu] == self.points[j as usize]
                && (self.shapes[iu] == self.shapes[j as usize]
                    || self.shapes[iu] == pt
                    || self.shapes[j as usize] == pt)
            {
                j += 1;
            }

            let mut shape_to_keep = self.shapes[iu];

            if shape_to_keep == pt {
                shape_to_keep = self.shapes[j as usize - 1];
            }

            debug_assert!(shape_to_keep.0 < self.arcs.len() as isize);
            debug_assert!(shape_to_keep.1 < self.arcs.len() as isize);

            pts_unique.push(self.c_point(i));
            shapes_unique.push(shape_to_keep);

            i = j;
        }

        (pts_unique, shapes_unique)
    }

    /// Port of `SHAPE_LINE_CHAIN::Simplify()`: simplify the line chain by removing colinear
    /// adjacent segments and duplicate vertices.  Points within `a_tolerance` of the segment
    /// spanning them are removed; only points that are not on an arc can be removed (the end
    /// points of the spanning segment may be arc points).  (The C++ default tolerance is 0.)
    pub fn simplify(&mut self, a_tolerance: i32) {
        if self.point_count() < 3 {
            return;
        }

        let n = self.points.len();
        let mut new_points: Vec<Vector2I> = Vec::with_capacity(n);
        let mut new_shapes: Vec<(isize, isize)> = Vec::with_capacity(n);

        let mut start_idx: usize = 0;

        while start_idx < n {
            new_points.push(self.points[start_idx]);
            new_shapes.push(self.shapes[start_idx]);

            // If the line is not closed, we need at least 3 points before simplifying
            if !self.closed && start_idx == n - 2 {
                break;
            }

            // Initialize the end index to be two points ahead of start
            let mut end_idx = (start_idx + 2) % n;
            let mut can_simplify = true;

            while can_simplify && end_idx != start_idx && (end_idx > start_idx || self.closed) {
                // Test all points between start_idx and end_idx
                let mut test_idx = (start_idx + 1) % n;

                while test_idx != end_idx {
                    // Only intermediate test points must be regular points (not arcs).
                    // Allow start or end to be arc endpoints so collinear segments
                    // adjacent to arcs can still be simplified while preserving arcs.
                    if self.shapes[test_idx].0 != Self::SHAPE_IS_PT {
                        can_simplify = false;
                        break;
                    }

                    // Test if the point is within the allowed error
                    if !test_segment_hit(
                        self.points[test_idx],
                        self.points[start_idx],
                        self.points[end_idx],
                        a_tolerance,
                    ) {
                        can_simplify = false;
                        break;
                    }

                    test_idx = (test_idx + 1) % n;
                }

                if can_simplify {
                    // If we can simplify, move end_idx one further
                    end_idx = (end_idx + 1) % n;
                }
            }

            // If we couldn't simplify at all, move to the next point
            if end_idx == (start_idx + 2) % n {
                start_idx += 1;
            } else {
                // Otherwise, jump to the last point we could include in the simplification
                let new_start_idx = (end_idx + n - 1) % n;

                // If we looped all the way around, we're done
                if new_start_idx <= start_idx {
                    break;
                }

                start_idx = new_start_idx;
            }
        }

        // If we have only one point, we need to add a second point to make a line
        if new_points.len() == 1 {
            new_points.push(*self.points.last().unwrap());
            new_shapes.push(*self.shapes.last().unwrap());
        }

        // If we are not closed, then the start and end points of the original line need to
        // be the start and end points of the new line.
        if !self.closed && self.points.last() != new_points.last() {
            new_points.push(*self.points.last().unwrap());
            new_shapes.push(*self.shapes.last().unwrap());
        }

        self.points = new_points;
        self.shapes = new_shapes;
    }

    /// Port of `SHAPE_LINE_CHAIN::Simplify2()`: simplify the line chain by removing colinear
    /// adjacent segments and duplicate vertices.  (The C++ default for `a_remove_colinear` is
    /// true.)
    pub fn simplify2(&mut self, a_remove_colinear: bool) -> &mut Self {
        // Always try to keep at least 2 points otherwise, we're not really a line
        if self.point_count() < 3 {
            return self;
        } else if self.point_count() == 3 {
            if self.points[0] == self.points[1] || self.points[1] == self.points[2] {
                self.remove(1);
            }

            return self;
        }

        // stage 1: eliminate duplicate vertices
        let (pts_unique, shapes_unique) = self.unique_points();

        self.points.clear();
        self.shapes.clear();
        let np = pts_unique.len() as i32;

        let mut i: i32 = 0;

        // stage 2: eliminate colinear segments
        while i < np - 2 {
            let p0 = pts_unique[i as usize];
            let mut n = i;

            if a_remove_colinear
                && shapes_unique[i as usize] == Self::SHAPES_ARE_PT
                && shapes_unique[i as usize + 1] == Self::SHAPES_ARE_PT
            {
                while n < np - 2
                    && (Seg::new(p0, pts_unique[n as usize + 2])
                        .line_distance(pts_unique[n as usize + 1])
                        <= 1
                        || Seg::new(p0, pts_unique[n as usize + 2])
                            .collinear(&Seg::new(p0, pts_unique[n as usize + 1])))
                {
                    n += 1;
                }
            }

            self.points.push(p0);
            self.shapes.push(shapes_unique[i as usize]);

            if n > i {
                i = n;
            }

            if n == np - 2 {
                self.points.push(pts_unique[np as usize - 1]);
                self.shapes.push(shapes_unique[np as usize - 1]);
                return self;
            }

            i += 1;
        }

        if np > 1 {
            self.points.push(pts_unique[np as usize - 2]);
            self.shapes.push(shapes_unique[np as usize - 2]);
        }

        self.points.push(pts_unique[np as usize - 1]);
        self.shapes.push(shapes_unique[np as usize - 1]);

        debug_assert!(self.points.len() == self.shapes.len());

        self
    }

    /// Port of `SHAPE_LINE_CHAIN::Split( const VECTOR2I& aStart, const VECTOR2I& aEnd,
    /// SHAPE_LINE_CHAIN& aPre, SHAPE_LINE_CHAIN& aMid, SHAPE_LINE_CHAIN& aPost )`: split the
    /// chain at the points nearest to `a_start` and `a_end`; returns `( pre, mid, post )`.
    pub fn split_range(
        &self,
        a_start: Vector2I,
        a_end: Vector2I,
    ) -> (ShapeLineChain, ShapeLineChain, ShapeLineChain) {
        let cp = a_end;

        let n = self.nearest_point(cp, false);
        let m = self.nearest_point(a_start, false);

        let mut l = self.clone();
        l.split(n, true);
        l.split(m, true);

        let mut i_start = l.find(m, 0);
        let mut i_end = l.find(n, 0);

        if i_start > i_end {
            l = l.reverse();
            i_start = l.find(m, 0);
            i_end = l.find(n, 0);
        }

        let pre = l.slice(0, i_start);
        let post = l.slice(i_end, -1);
        let mid = l.slice(i_start, i_end);

        (pre, mid, post)
    }

    /// The part of `SHAPE_LINE_CHAIN::OffsetLine()` after `SHAPE_POLY_SET::OffsetLineChain()`:
    /// given `a_poly`, the result of offsetting this chain by the requested amount, return the
    /// left and right offset lines (or `None` where the C++ returns false).
    ///
    /// `OffsetLine()` itself (the offset of the chain into a polygon set) belongs to the
    /// polygon set port: `offset_line( amount, corner_strategy, max_error, simplify )` is
    /// `self.offset_line_from_poly( &offset_line_chain( self, ... ) )`; see `PORT_STATUS.md`.
    pub fn offset_line_from_poly(
        &self,
        a_poly: &ShapePolySet,
    ) -> Option<(ShapeLineChain, ShapeLineChain)> {
        if self.point_count() < 2 {
            return None;
        }

        if a_poly.outline_count() != 1 {
            return None;
        }

        if a_poly.c_outline(0).point_count() < 3 {
            return None;
        }

        // HasHoles()
        if (0..a_poly.outline_count()).any(|i| a_poly.c_polygon(i).len() > 1) {
            return None;
        }

        let mut outline = a_poly.c_outline(0).clone();

        debug_assert!(outline.is_closed());

        let start = self.c_point(0);
        let end = self.c_last_point();

        outline.split(start, true);
        outline.split(end, true);

        let id_a = outline.find(start, 0);
        let id_b = outline.find(end, 0);

        if id_a == -1 || id_b == -1 {
            return None;
        }

        let mut a_left = ShapeLineChain::new();
        let mut a_right = ShapeLineChain::new();

        let mut i = id_a;

        loop {
            a_left.append_point(outline.c_point(i), false);

            i = (i + 1) % outline.point_count();

            if i == id_b {
                a_left.append_point(outline.c_point(i), false);
                break;
            }

            if i == id_a {
                return None;
            }
        }

        if a_left.point_count() < 2 {
            return None;
        }

        let mut i = id_b;

        loop {
            a_right.append_point(outline.c_point(i), false);

            i = (i + 1) % outline.point_count();

            if i == id_a {
                a_right.append_point(outline.c_point(i), false);
                break;
            }

            if i == id_b {
                return None;
            }
        }

        if a_right.point_count() < 2 {
            return None;
        }

        if a_left.c_point(0) != start {
            a_left = a_left.reverse();
            debug_assert!(a_left.c_point(0) == start);
        }

        if a_right.c_point(0) != start {
            a_right = a_right.reverse();
            debug_assert!(a_right.c_point(0) == start);
        }

        let base = Seg::new(self.c_point(0), self.c_point(1));
        let side_left = base.side(a_left.c_point(1));
        let side_right = base.side(a_right.c_point(1));

        if side_left == 0 || side_right == 0 {
            return None;
        }

        if side_left == side_right {
            return None;
        }

        if side_left > 0 && side_right < 0 {
            std::mem::swap(&mut a_left, &mut a_right);
        }

        if a_left.point_count() < 4 {
            return None;
        }

        if a_right.point_count() < 4 {
            return None;
        }

        a_left.remove(0);
        a_left.remove(a_left.point_count() - 1);

        a_right.remove(0);
        a_right.remove(a_right.point_count() - 1);

        Some((a_left, a_right))
    }

    /// Port of `SHAPE_LINE_CHAIN::ClosestPoints( const SHAPE_LINE_CHAIN& aOther,
    /// VECTOR2I& aPt0, VECTOR2I& aPt1 )`: the closest pair of vertices of the two chains.
    pub fn closest_points(&self, a_other: &ShapeLineChain) -> Option<(Vector2I, Vector2I)> {
        Self::closest_points_ranges(&self.points, &a_other.points).map(|(a, b, _)| (a, b))
    }

    /// Port of the static `SHAPE_LINE_CHAIN::ClosestPoints( aMyStart, aMyEnd, aOtherStart,
    /// aOtherEnd, aPt0, aPt1, aDistSq )` (iterator ranges are slices): the closest pair of
    /// points and their squared distance, `None` where the C++ returns false (it then leaves
    /// the points untouched).
    pub fn closest_points_ranges(
        a_mine: &[Vector2I],
        a_other: &[Vector2I],
    ) -> Option<(Vector2I, Vector2I, i64)> {
        let mut closest_dist_sq: i64 = Vector2I::ECOORD_MAX;
        let mut pts = (Vector2I::default(), Vector2I::default());

        for &pt_a in a_mine {
            for &pt_b in a_other {
                let dx: Ecoord = pt_b.x as Ecoord - pt_a.x as Ecoord;
                let dy: Ecoord = pt_b.y as Ecoord - pt_a.y as Ecoord;

                let dist_sq: Ecoord = dx * dx + dy * dy;

                if dist_sq < closest_dist_sq {
                    closest_dist_sq = dist_sq;
                    pts = (pt_a, pt_b);
                }
            }
        }

        if closest_dist_sq != Vector2I::ECOORD_MAX {
            Some((pts.0, pts.1, closest_dist_sq))
        } else {
            None
        }
    }

    /// Port of the static `SHAPE_LINE_CHAIN::ClosestSegments()`: the closest points of the
    /// segments `( prev, mine[0] ), ( mine[0], mine[1] ), ...` and the corresponding segments
    /// of the other range, and their squared distance.  `None` where the C++ returns false.
    pub fn closest_segments(
        a_my_prev_pt: Vector2I,
        a_mine: &[Vector2I],
        a_other_prev_pt: Vector2I,
        a_other: &[Vector2I],
    ) -> Option<(Vector2I, Vector2I, i64)> {
        if a_mine.is_empty() || a_other.is_empty() {
            return None;
        }

        let mut closest_dist_sq: i64 = Vector2I::ECOORD_MAX;
        let mut pts = (Vector2I::default(), Vector2I::default());
        let mut last_pt_a = a_my_prev_pt;

        for &pt_a in a_mine {
            let mut last_pt_b = a_other_prev_pt;

            for &pt_b in a_other {
                let seg_a = Seg::new(last_pt_a, pt_a);
                let seg_b = Seg::new(last_pt_b, pt_b);

                // SEG::NearestPoints always returns true
                let (nearest_a, nearest_b, dist_sq) = seg_a.nearest_points(&seg_b);

                if dist_sq < closest_dist_sq {
                    closest_dist_sq = dist_sq;
                    pts = (nearest_a, nearest_b);
                }

                last_pt_b = pt_b;
            }

            last_pt_a = pt_a;
        }

        if closest_dist_sq != Vector2I::ECOORD_MAX {
            Some((pts.0, pts.1, closest_dist_sq))
        } else {
            None
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::ClosestSegmentsFast()`: finds the closest points between the
    /// segments of two (closed) chains, testing only the 5 closest pairs of point groups.
    pub fn closest_segments_fast(&self, a_other: &ShapeLineChain) -> Option<(Vector2I, Vector2I)> {
        let my_pts = &self.points;
        let other_pts = &a_other.points;

        const C_MAX_BOXES: usize = 100;
        const C_MIN_PTS_PER_BOX: i32 = 20;

        let my_points_per_box = C_MIN_PTS_PER_BOX.max((my_pts.len() / C_MAX_BOXES) as i32 + 1);
        let other_points_per_box =
            C_MIN_PTS_PER_BOX.max((other_pts.len() / C_MAX_BOXES) as i32 + 1);

        let my_num_boxes = my_pts.len().div_ceil(my_points_per_box as usize) as i32;
        let other_num_boxes = other_pts.len().div_ceil(other_points_per_box as usize) as i32;

        #[derive(Clone, Copy, Default)]
        struct BoxRec {
            bbox: Box2IMinMax,
            center: Vector2I,
            radius: i32,
            valid: bool,
        }

        let mut my_boxes = vec![BoxRec::default(); my_num_boxes as usize];
        let mut other_boxes = vec![BoxRec::default(); other_num_boxes as usize];

        // Calculate bounding boxes
        let fill = |boxes: &mut [BoxRec], pts: &[Vector2I], per_box: i32| {
            for (i, &pt) in pts.iter().enumerate() {
                let bx = &mut boxes[i / per_box as usize];

                if bx.valid {
                    bx.bbox.merge(pt);
                } else {
                    bx.bbox = Box2IMinMax::from_point(pt);
                    bx.valid = true;
                }
            }

            // Store centers and radiuses
            for bx in boxes.iter_mut() {
                bx.center = bx.bbox.center();
                bx.radius = f64_to_i32_x86(bx.bbox.diameter() / 2.0);
            }
        };

        fill(&mut my_boxes, my_pts, my_points_per_box);
        fill(&mut other_boxes, other_pts, other_points_per_box);

        // Find closest pairs
        #[derive(Clone, Copy)]
        struct DistPair {
            dist: i64,
            id_a: usize,
            id_b: usize,
        }

        let mut pairs_to_test: Vec<DistPair> = Vec::new();

        for (ia, ca) in my_boxes.iter().enumerate() {
            for (ib, cb) in other_boxes.iter().enumerate() {
                if !ca.valid || !cb.valid {
                    continue;
                }

                let p_a = Vector2L::from(ca.center);
                let p_b = Vector2L::from(cb.center);

                let mut dist: i64 = (p_b - p_a).euclidean_norm();

                dist -= ca.radius as i64;
                dist -= cb.radius as i64;

                pairs_to_test.push(DistPair {
                    dist,
                    id_a: ia,
                    id_b: ib,
                });
            }
        }

        std_sort(&mut pairs_to_test, |a, b| a.dist < b.dist);

        const C_POLY_PAIRS_LIMIT: usize = 5;

        // Find closest segments in tested pairs
        let mut total_closest_dist_sq: i64 = Vector2I::ECOORD_MAX;
        let mut result = (Vector2I::default(), Vector2I::default());

        for pair in pairs_to_test.iter().take(C_POLY_PAIRS_LIMIT) {
            let my_start_id = pair.id_a * my_points_per_box as usize;
            let my_end_id = (my_start_id + my_points_per_box as usize).min(my_pts.len());

            let my_prev_pt = my_pts[if my_start_id == 0 {
                my_pts.len() - 1
            } else {
                my_start_id - 1
            }];

            let other_start_id = pair.id_b * other_points_per_box as usize;
            let other_end_id =
                (other_start_id + other_points_per_box as usize).min(other_pts.len());

            let other_prev_pt = other_pts[if other_start_id == 0 {
                other_pts.len() - 1
            } else {
                other_start_id - 1
            }];

            if let Some((pt_a, pt_b, dist_sq)) = Self::closest_segments(
                my_prev_pt,
                &my_pts[my_start_id..my_end_id],
                other_prev_pt,
                &other_pts[other_start_id..other_end_id],
            ) && dist_sq < total_closest_dist_sq
            {
                total_closest_dist_sq = dist_sq;
                result = (pt_a, pt_b);
            }
        }

        if total_closest_dist_sq != Vector2I::ECOORD_MAX {
            Some(result)
        } else {
            None
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Mirror( const VECTOR2I& aRef, FLIP_DIRECTION )`: mirror the
    /// line points about `a_ref`.
    pub fn mirror(&mut self, a_ref: Vector2I, a_flip_direction: FlipDirection) {
        for pt in &mut self.points {
            if a_flip_direction == FlipDirection::LeftRight {
                pt.x = -pt.x + 2 * a_ref.x;
            } else {
                pt.y = -pt.y + 2 * a_ref.y;
            }
        }

        for arc in &mut self.arcs {
            arc.mirror(a_ref, a_flip_direction);
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::Mirror( const SEG& axis )`: mirror the line points using
    /// a given axis.
    pub fn mirror_seg(&mut self, axis: &Seg) {
        for pt in &mut self.points {
            *pt = axis.reflect_point(*pt);
        }

        for arc in &mut self.arcs {
            arc.mirror_seg(axis);
        }
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

    /// Port of `SHAPE_LINE_CHAIN::reversedArcIndex()`.
    pub fn reversed_arc_index(&self, a_segment: usize) -> isize {
        if self.is_shared_pt(a_segment) {
            self.shapes[a_segment].0
        } else {
            self.shapes[a_segment].1
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

    /// Port of `SHAPE_LINE_CHAIN::Distance( const VECTOR2I&, bool aOutlineOnly )`:
    /// `sqrt( SquaredDistance() )` converted to `int` (`INT_MIN` for an empty chain, as on the
    /// reference build).
    pub fn distance_outline(&self, a_p: Vector2I, a_outline_only: bool) -> i32 {
        f64_to_i32_x86((self.lcb_squared_distance(a_p, a_outline_only) as f64).sqrt())
    }
}

impl Shape for ShapeLineChain {
    fn type_(&self) -> ShapeType {
        ShapeType::LineChain
    }

    fn as_shape_ref(&self) -> ShapeRef<'_> {
        ShapeRef::LineChain(self)
    }

    /// Port of `SHAPE_LINE_CHAIN::Clone()`.
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

        // Direct access to m_points avoids virtual GetPoint() dispatch and per-call bounds checking
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

    /// Port of `SHAPE_LINE_CHAIN::Rotate()`: rotate all vertices by a given angle around
    /// `a_center`.
    fn rotate(&mut self, a_angle: EdaAngle, a_center: Vector2I) {
        for pt in &mut self.points {
            rotate_point_around(pt, a_center, a_angle);
        }

        for arc in &mut self.arcs {
            arc.rotate(a_angle, a_center);
        }
    }

    /// Port of `SHAPE_LINE_CHAIN::SetWidth()`: set the width of all segments in the chain.
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

    /// Port of `SHAPE_LINE_CHAIN::Format()`: the points and the closed flag as C++ source
    /// text (`aCplusPlus` is ignored; arcs are not written, as in C++).
    fn format(&self, _a_cplusplus: bool) -> String {
        let mut ss = String::from("SHAPE_LINE_CHAIN( { ");

        for i in 0..self.point_count() {
            let p = self.points[i as usize];
            ss.push_str(&format!("VECTOR2I( {}, {})", p.x, p.y));

            if i != self.point_count() - 1 {
                ss.push_str(", ");
            }
        }

        ss.push_str("}, ");
        ss.push_str(if self.closed { "true" } else { "false" });
        ss.push_str(" );");

        ss
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
