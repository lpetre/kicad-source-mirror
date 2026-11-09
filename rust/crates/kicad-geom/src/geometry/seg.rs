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
// Copyright (C) 2013 CERN
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>

//! Port of `libs/kimath/include/geometry/seg.h` and `libs/kimath/src/geometry/seg.cpp`.
//!
//! Naming of the overloads (see `PORTING.md`): where the C++ has an overload taking a point
//! and one taking a segment, the point version keeps the plain name and the segment version
//! gets a `_seg` suffix (`nearest_point` / `nearest_point_seg`, `distance` / `distance_seg`,
//! `squared_distance` / `squared_distance_seg`, `contains` / `contains_seg`).  Where the C++
//! has default arguments, the plain name applies the defaults and a `_with...` variant takes
//! them explicitly.

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};

use crate::geometry::eda_angle::EdaAngle;
use crate::math::util::{isqrt, ki_round, ki_round_i64, rescale};
use crate::math::vector2::{Vector2, Vector2I, Vector2L, ki_round_xy};
use crate::math::wide_int::{cross_wide, to_double};

/// Port of `OPT_VECTOR2I`.
pub type OptVector2I = Option<Vector2I>;

/// Port of `SEG::ecoord` (`VECTOR2I::extended_type`).
pub type Ecoord = i64;

/// Port of the file-local `sgn` template in `seg.cpp`.
#[inline]
fn sgn(a_val: Ecoord) -> Ecoord {
    (0 < a_val) as Ecoord - (a_val < 0) as Ecoord
}

/// Port of `SEG`: a line segment between two integer points.
///
/// Equality, ordering and hashing only look at the end points (`A`, `B`), not at the index
/// within a parent shape, exactly like the C++ `operator==`, `operator!=` and `operator<`.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    /// Start of the segment (C++ member `A`). Public, to make access simpler.
    pub a: Vector2I,
    /// End of the segment (C++ member `B`). Public, to make access simpler.
    pub b: Vector2I,
    /// index within the parent shape (used when m_is_local == false)
    index: i32,
}

impl Default for Seg {
    /// Port of `SEG::SEG()`: create an empty (0, 0) segment.
    #[inline]
    fn default() -> Self {
        Seg {
            a: Vector2I::new(0, 0),
            b: Vector2I::new(0, 0),
            index: -1,
        }
    }
}

impl PartialEq for Seg {
    /// Port of `SEG::operator==`.
    #[inline]
    fn eq(&self, a_seg: &Seg) -> bool {
        self.a == a_seg.a && self.b == a_seg.b
    }
}

impl Eq for Seg {}

impl Hash for Seg {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.a.hash(state);
        self.b.hash(state);
    }
}

impl PartialOrd for Seg {
    #[inline]
    fn partial_cmp(&self, other: &Seg) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Seg {
    /// Port of `SEG::operator<`: order by `A`, then by `B`.
    #[inline]
    fn cmp(&self, a_seg: &Seg) -> Ordering {
        if self.a == a_seg.a {
            return self.b.cmp(&a_seg.b);
        }

        self.a.cmp(&a_seg.a)
    }
}

impl fmt::Display for Seg {
    /// Port of `operator<<( std::ostream&, const SEG& )`: `[ [ ax | ay ] - [ bx | by ] ]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[ {} - {} ]",
            self.a.to_stream_string(),
            self.b.to_stream_string()
        )
    }
}

impl Seg {
    /// Port of `SEG::SEG( const VECTOR2I& aA, const VECTOR2I& aB )`: create a segment between
    /// (aA) and (aB).
    #[inline]
    pub const fn new(a_a: Vector2I, a_b: Vector2I) -> Self {
        Seg {
            a: a_a,
            b: a_b,
            index: -1,
        }
    }

    /// Port of `SEG::SEG( int aX1, int aY1, int aX2, int aY2 )`: create a segment between
    /// (aX1, aY1) and (aX2, aY2).
    #[inline]
    pub const fn from_coords(a_x1: i32, a_y1: i32, a_x2: i32, a_y2: i32) -> Self {
        Seg {
            a: Vector2I::new(a_x1, a_y1),
            b: Vector2I::new(a_x2, a_y2),
            index: -1,
        }
    }

    /// Port of `SEG::SEG( const VECTOR2I& aA, const VECTOR2I& aB, int aIndex )`.
    ///
    /// Create a segment between (aA) and (aB), referenced to a multi-segment shape.
    ///
    /// * `a_a` - the start point in the parent shape
    /// * `a_b` - the end point in the parent shape
    /// * `a_index` - index of the segment within the parent shape
    #[inline]
    pub const fn with_index(a_a: Vector2I, a_b: Vector2I, a_index: i32) -> Self {
        Seg {
            a: a_a,
            b: a_b,
            index: a_index,
        }
    }

    /// Port of `SEG::Square()`.
    #[inline]
    pub const fn square(a: i32) -> Ecoord {
        a as Ecoord * a as Ecoord
    }

    /// Port of `SEG::LineProject()`.
    ///
    /// Compute the perpendicular projection point of aP on a line passing through
    /// ends of the segment.
    pub fn line_project(&self, a_p: Vector2I) -> Vector2I {
        let d: Vector2I = self.b - self.a;
        let l_squared: Ecoord = d.dot(d);

        if l_squared == 0 {
            return self.a;
        }

        let t: Ecoord = d.dot(a_p - self.a);

        let xp: Ecoord = rescale(t, d.x as Ecoord, l_squared);
        let yp: Ecoord = rescale(t, d.y as Ecoord, l_squared);

        Vector2::<Ecoord>::new(self.a.x as Ecoord + xp, self.a.y as Ecoord + yp).cast()
    }

    /// Port of `SEG::Side()`.
    ///
    /// Determine on which side of directed line passing via segment ends point aP lies.
    ///
    /// Returns: < 0: left, 0 : on the line, > 0 : right
    #[inline]
    pub fn side(&self, a_p: Vector2I) -> i32 {
        let det: Ecoord = (self.b - self.a).cross(a_p - self.a);

        if det < 0 {
            -1
        } else if det > 0 {
            1
        } else {
            0
        }
    }

    /// Port of `SEG::LineDistance( aP )` with `aDetermineSide = false`.
    ///
    /// Return the closest Euclidean distance between point aP and the line defined by
    /// the ends of segment (this).
    #[inline]
    pub fn line_distance(&self, a_p: Vector2I) -> i32 {
        self.line_distance_sided(a_p, false)
    }

    /// Port of `SEG::LineDistance( aP, aDetermineSide )`.
    ///
    /// Return the closest Euclidean distance between point aP and the line defined by
    /// the ends of segment (this).
    ///
    /// * `a_determine_side` - when true, the sign of the returned value indicates the side of
    ///   the line at which we are (negative = left)
    pub fn line_distance_sided(&self, a_p: Vector2I, a_determine_side: bool) -> i32 {
        let p: Ecoord = self.a.y as Ecoord - self.b.y as Ecoord;
        let q: Ecoord = self.b.x as Ecoord - self.a.x as Ecoord;
        let r: Ecoord = -p * self.a.x as Ecoord - q * self.a.y as Ecoord;
        let l: Ecoord = p * p + q * q;
        let det: Ecoord = p * a_p.x as Ecoord + q * a_p.y as Ecoord + r;
        let mut dist_sq: Ecoord = 0;

        if l > 0 {
            dist_sq = rescale(det, det, l);
        }

        let dist: Ecoord = isqrt(dist_sq);

        (if a_determine_side {
            sgn(det) * dist
        } else {
            dist.abs()
        }) as i32
    }

    /// Port of `SEG::Angle()`.
    ///
    /// Determine the smallest angle between two segments.
    pub fn angle(&self, a_other: &Seg) -> EdaAngle {
        let this_angle: EdaAngle = EdaAngle::from_vector(self.a - self.b).normalize180();
        let other_angle: EdaAngle = EdaAngle::from_vector(a_other.a - a_other.b).normalize180();

        (this_angle - other_angle).normalize180().abs()
    }

    /// Port of `SEG::NearestPoint( const VECTOR2I& aP )`.
    ///
    /// Compute a point on the segment (this) that is closest to point `a_p`.
    pub fn nearest_point(&self, a_p: Vector2I) -> Vector2I {
        // Inlined for performance reasons
        let d = Vector2L::new(
            self.b.x as i64 - self.a.x as i64,
            self.b.y as i64 - self.a.y as i64,
        );

        let l_squared: Ecoord = d.x * d.x + d.y * d.y;

        if l_squared == 0 {
            return self.a;
        }

        // Inlined for performance reasons
        let pa = Vector2L::new(
            a_p.x as i64 - self.a.x as i64,
            a_p.y as i64 - self.a.y as i64,
        );

        let t: Ecoord = d.dot(pa);

        if t < 0 {
            return self.a;
        } else if t > l_squared {
            return self.b;
        }

        let xp: Ecoord = rescale(t, d.x, l_squared);
        let yp: Ecoord = rescale(t, d.y, l_squared);

        Vector2::<Ecoord>::new(self.a.x as Ecoord + xp, self.a.y as Ecoord + yp).cast()
    }

    /// Port of `SEG::NearestPoint( const SEG& aSeg )`.
    ///
    /// Compute a point on the segment (this) that is closest to any point on `a_seg`.
    pub fn nearest_point_seg(&self, a_seg: &Seg) -> Vector2I {
        if let Some(p) = self.intersect(a_seg) {
            return p;
        }

        let pts_origin: [Vector2I; 4] = [
            a_seg.nearest_point(self.a),
            a_seg.nearest_point(self.b),
            self.nearest_point(a_seg.a),
            self.nearest_point(a_seg.b),
        ];

        let pts_out: [Vector2I; 4] = [self.a, self.b, pts_origin[2], pts_origin[3]];

        let pts_dist: [Ecoord; 4] = [
            (pts_origin[0] - self.a).squared_euclidean_norm(),
            (pts_origin[1] - self.b).squared_euclidean_norm(),
            (pts_origin[2] - a_seg.a).squared_euclidean_norm(),
            (pts_origin[3] - a_seg.b).squared_euclidean_norm(),
        ];

        let mut min_i: usize = 0;

        for i in 0..4 {
            if pts_dist[i] < pts_dist[min_i] {
                min_i = i;
            }
        }

        pts_out[min_i]
    }

    /// Port of `SEG::NearestPoints()`.
    ///
    /// Compute closest points between this segment and `a_seg`.
    ///
    /// Returns `( aPtA, aPtB, aDistSq )`: the point on this segment, the point on the other
    /// segment and the squared distance between the points.  (The C++ function returns
    /// `true` unconditionally and passes the results through out-parameters.)
    pub fn nearest_points(&self, a_seg: &Seg) -> (Vector2I, Vector2I, i64) {
        if let Some(p) = self.intersect(a_seg) {
            return (p, p, 0);
        }

        let pts_origin: [Vector2I; 4] = [
            a_seg.nearest_point(self.a),
            a_seg.nearest_point(self.b),
            self.nearest_point(a_seg.a),
            self.nearest_point(a_seg.b),
        ];

        let pts_a_out: [Vector2I; 4] = [self.a, self.b, pts_origin[2], pts_origin[3]];

        let pts_b_out: [Vector2I; 4] = [pts_origin[0], pts_origin[1], a_seg.a, a_seg.b];

        let pts_dist: [Ecoord; 4] = [
            (pts_origin[0] - self.a).squared_euclidean_norm(),
            (pts_origin[1] - self.b).squared_euclidean_norm(),
            (pts_origin[2] - a_seg.a).squared_euclidean_norm(),
            (pts_origin[3] - a_seg.b).squared_euclidean_norm(),
        ];

        let mut min_i: usize = 0;

        for i in 0..4 {
            if pts_dist[i] < pts_dist[min_i] {
                min_i = i;
            }
        }

        (pts_a_out[min_i], pts_b_out[min_i], pts_dist[min_i])
    }

    /// Port of `SEG::ReflectPoint()`.
    ///
    /// Reflect a point using this segment as axis.
    pub fn reflect_point(&self, a_p: Vector2I) -> Vector2I {
        let d: Vector2I = self.b - self.a;
        let l_squared: Ecoord = d.dot(d);
        let t: Ecoord = d.dot(a_p - self.a);
        let c: Vector2<Ecoord> = if l_squared == 0 {
            Vector2::new(a_p.x as Ecoord, a_p.y as Ecoord)
        } else {
            Vector2::new(
                self.a.x as Ecoord + rescale(t, d.x as Ecoord, l_squared),
                self.a.y as Ecoord + rescale(t, d.y as Ecoord, l_squared),
            )
        };

        Vector2::<Ecoord>::new(2 * c.x - a_p.x as Ecoord, 2 * c.y - a_p.y as Ecoord).cast()
    }

    /// Port of `SEG::Intersect( aSeg )` with the default arguments
    /// (`aIgnoreEndpoints = false`, `aLines = false`).
    #[inline]
    pub fn intersect(&self, a_seg: &Seg) -> OptVector2I {
        self.intersect_with(a_seg, false, false)
    }

    /// Port of `SEG::Intersect( aSeg, aIgnoreEndpoints, aLines )`.
    ///
    /// Compute intersection point of segment (this) with segment `a_seg`.
    ///
    /// * `a_seg` - segment to intersect with
    /// * `a_ignore_endpoints` - don't treat corner cases (i.e. end of one segment touching
    ///   the other) as intersections.
    /// * `a_lines` - treat segments as infinite lines
    ///
    /// Returns the intersection point, if exists.
    pub fn intersect_with(
        &self,
        a_seg: &Seg,
        a_ignore_endpoints: bool,
        a_lines: bool,
    ) -> OptVector2I {
        let mut ip = Vector2I::new(0, 0);

        if self.intersects_impl(a_seg, a_ignore_endpoints, a_lines, Some(&mut ip)) {
            Some(ip)
        } else {
            None
        }
    }

    /// Port of `SEG::Intersects()`.
    #[inline]
    pub fn intersects(&self, a_seg: &Seg) -> bool {
        self.intersects_impl(a_seg, false, false, None)
    }

    /// Port of `SEG::IntersectLines()`.
    ///
    /// Compute the intersection point of lines passing through ends of (this) and `a_seg`.
    #[inline]
    pub fn intersect_lines(&self, a_seg: &Seg) -> OptVector2I {
        self.intersect_with(a_seg, false, true)
    }

    /// Port of `SEG::IntersectsLine()`.
    ///
    /// Check if this segment intersects a line defined by slope `a_slope` and offset
    /// `a_offset`.
    ///
    /// Returns the intersection point if the segment intersects the line, `None` otherwise
    /// (the C++ returns a `bool` and writes the point to the `aIntersection` out-parameter).
    pub fn intersects_line(&self, a_slope: f64, a_offset: f64) -> OptVector2I {
        let seg_a: Vector2L = Vector2L::from(self.a);
        let seg_b: Vector2L = Vector2L::from(self.b);
        let seg_dir: Vector2L = seg_b - seg_a;

        // Handle vertical segment case
        if seg_dir.x == 0 {
            // Vertical segment: x = A.x, find y on the line
            let intersect_y: f64 = a_slope * self.a.x as f64 + a_offset;

            // Compare before rounding.  The line is unbounded, so a miss lands anywhere, and
            // narrowing it first would fault on a value this is about to discard
            let seg_min_y: i32 = self.a.y.min(self.b.y);
            let seg_max_y: i32 = self.a.y.max(self.b.y);

            if intersect_y >= seg_min_y as f64 && intersect_y <= seg_max_y as f64 {
                return Some(Vector2I::new(self.a.x, ki_round(intersect_y)));
            }

            return None;
        }

        let line_dir: Vector2L = Vector2L::new(1000, (a_slope * 1000.0) as Ecoord);
        let cross_product: Ecoord = seg_dir.cross(line_dir);

        if cross_product == 0 {
            // Parallel lines - check if segment lies on the line
            let expected_y: f64 = a_slope * self.a.x as f64 + a_offset;
            let diff: f64 = (self.a.y as f64 - expected_y).abs();

            if diff < 0.5 {
                // Collinear: segment lies on the line, return midpoint
                return Some((self.a + self.b) / 2);
            }

            return None; // Parallel but not collinear
        }

        // Find intersection using parametric equations
        // Segment: P = segA + t * segDir
        // Line: y = aSlope * x + aOffset
        //
        // At intersection: segA.y + t * segDir.y = aSlope * (segA.x + t * segDir.x) + aOffset
        // Solving for t: t = (aSlope * segA.x + aOffset - segA.y) / (segDir.y - aSlope * segDir.x)

        let numerator: f64 = a_slope * seg_a.x as f64 + a_offset - seg_a.y as f64;
        let denominator: f64 = seg_dir.y as f64 - a_slope * seg_dir.x as f64;

        let t: f64 = numerator / denominator;

        // Check if intersection is within segment bounds
        if (0.0..=1.0).contains(&t) {
            return Some(ki_round_xy(
                seg_a.x as f64 + t * seg_dir.x as f64,
                seg_a.y as f64 + t * seg_dir.y as f64,
            ));
        }

        None
    }

    /// Port of `SEG::PerpendicularSeg()`.
    ///
    /// Compute a segment perpendicular to this one, passing through point `a_p`.
    pub fn perpendicular_seg(&self, a_p: Vector2I) -> Seg {
        let slope: Vector2I = self.b - self.a;
        let end_point: Vector2I = slope.perpendicular() + a_p;

        Seg::new(a_p, end_point)
    }

    /// Port of `SEG::ParallelSeg()`.
    ///
    /// Compute a segment parallel to this one, passing through point `a_p`.
    pub fn parallel_seg(&self, a_p: Vector2I) -> Seg {
        let slope: Vector2I = self.b - self.a;
        let end_point: Vector2I = slope + a_p;

        Seg::new(a_p, end_point)
    }

    /// Port of `SEG::Collide( aSeg, aClearance )` (without the `aActual` out-parameter).
    #[inline]
    pub fn collide(&self, a_seg: &Seg, a_clearance: i32) -> bool {
        self.collide_actual(a_seg, a_clearance).0
    }

    /// Port of `SEG::Collide( aSeg, aClearance, aActual )`.
    ///
    /// Returns `( collides, actual )` where `actual` is the value the C++ stores to
    /// `*aActual` (every code path sets it): the actual distance between the segments, 0 for
    /// a negative clearance.
    pub fn collide_actual(&self, a_seg: &Seg, a_clearance: i32) -> (bool, i32) {
        // Handle negative clearance
        if a_clearance < 0 {
            return (false, 0);
        }

        // Handle zero-length segments (points) specially.
        // The intersects() check below doesn't handle this case correctly because
        // the cross product with a zero vector is always zero, causing false positives.
        if self.a == self.b {
            let dist: i32 = a_seg.distance(self.a);

            return (dist == 0 || dist < a_clearance, dist);
        }

        if a_seg.a == a_seg.b {
            let dist: i32 = self.distance(a_seg.a);

            return (dist == 0 || dist < a_clearance, dist);
        }

        // Check for exact intersection first
        if self.intersects_impl(a_seg, false, false, None) {
            return (true, 0);
        }

        let clearance_sq: Ecoord = a_clearance as Ecoord * a_clearance as Ecoord;
        let mut min_dist_sq: Ecoord = Vector2I::ECOORD_MAX;

        let mut check_distance = |dist: Ecoord| -> bool {
            if dist == 0 {
                return true;
            }

            min_dist_sq = min_dist_sq.min(dist);
            false // Continue checking
        };

        // There are 4 points to check: start and end of this segment, and
        // start and end of the other segment.
        if check_distance(self.squared_distance(a_seg.a))
            || check_distance(self.squared_distance(a_seg.b))
            || check_distance(a_seg.squared_distance(self.a))
            || check_distance(a_seg.squared_distance(self.b))
        {
            return (true, 0);
        }

        if min_dist_sq < clearance_sq {
            return (true, isqrt(min_dist_sq) as i32);
        }

        (false, isqrt(min_dist_sq) as i32)
    }

    /// Port of `SEG::SquaredDistance( const SEG& aSeg )`.
    pub fn squared_distance_seg(&self, a_seg: &Seg) -> Ecoord {
        // Handle zero-length segments (points) specially.
        // The Intersects() check below doesn't handle this case correctly because
        // the cross product with a zero vector is always zero, causing false positives.
        if self.a == self.b {
            return a_seg.squared_distance(self.a);
        }

        if a_seg.a == a_seg.b {
            return self.squared_distance(a_seg.a);
        }

        if self.intersects(a_seg) {
            return 0;
        }

        let pts: [Vector2I; 4] = [
            a_seg.nearest_point(self.a) - self.a,
            a_seg.nearest_point(self.b) - self.b,
            self.nearest_point(a_seg.a) - a_seg.a,
            self.nearest_point(a_seg.b) - a_seg.b,
        ];

        let mut m: Ecoord = Vector2I::ECOORD_MAX;

        for pt in pts {
            m = m.min(pt.squared_euclidean_norm());
        }

        m
    }

    /// Port of `SEG::Distance( const SEG& aSeg )`.
    ///
    /// Compute minimum Euclidean distance to segment `a_seg`.
    #[inline]
    pub fn distance_seg(&self, a_seg: &Seg) -> i32 {
        isqrt(self.squared_distance_seg(a_seg)) as i32
    }

    /// Port of `SEG::SquaredDistance( const VECTOR2I& aP )`.
    pub fn squared_distance(&self, a_p: Vector2I) -> Ecoord {
        let ab = Vector2::<Ecoord>::new(
            self.b.x as Ecoord - self.a.x as Ecoord,
            self.b.y as Ecoord - self.a.y as Ecoord,
        );
        let ap = Vector2::<Ecoord>::new(
            a_p.x as Ecoord - self.a.x as Ecoord,
            a_p.y as Ecoord - self.a.y as Ecoord,
        );

        let e: Ecoord = ap.dot(ab);

        if e <= 0 {
            return ap.squared_euclidean_norm();
        }

        let f: Ecoord = ab.squared_euclidean_norm();

        if e >= f {
            let bp = Vector2::<Ecoord>::new(
                a_p.x as Ecoord - self.b.x as Ecoord,
                a_p.y as Ecoord - self.b.y as Ecoord,
            );

            return bp.dot(bp);
        }

        // Lagrange identity keeps the small result free of cancellation
        let cross: f64 = to_double(cross_wide(ab, ap));
        let g: f64 = cross * cross / f as f64;

        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(g >= 0.0) || g > Ecoord::MAX as f64 {
            return 0;
        }

        ki_round_i64(g)
    }

    /// Port of `SEG::Distance( const VECTOR2I& aP )`.
    ///
    /// Compute minimum Euclidean distance to point `a_p`.
    #[inline]
    pub fn distance(&self, a_p: Vector2I) -> i32 {
        isqrt(self.squared_distance(a_p)) as i32
    }

    /// Port of `SEG::CanonicalCoefs()`: returns `( qA, qB, qC )`.
    #[inline]
    pub fn canonical_coefs(&self) -> (Ecoord, Ecoord, Ecoord) {
        let q_a: Ecoord = self.a.y as Ecoord - self.b.y as Ecoord;
        let q_b: Ecoord = self.b.x as Ecoord - self.a.x as Ecoord;
        let q_c: Ecoord = -q_a * self.a.x as Ecoord - q_b * self.a.y as Ecoord;

        (q_a, q_b, q_c)
    }

    /// Port of `SEG::Collinear()`.
    ///
    /// Check if segment aSeg lies on the same line as (this).
    pub fn collinear(&self, a_seg: &Seg) -> bool {
        let (qa, qb, qc) = self.canonical_coefs();

        let d1: Ecoord = (a_seg.a.x as Ecoord * qa + a_seg.a.y as Ecoord * qb + qc).abs();
        let d2: Ecoord = (a_seg.b.x as Ecoord * qa + a_seg.b.y as Ecoord * qb + qc).abs();

        d1 <= 1 && d2 <= 1
    }

    /// Port of `SEG::ApproxCollinear( aSeg )` with the default `aDistanceThreshold = 1`.
    #[inline]
    pub fn approx_collinear(&self, a_seg: &Seg) -> bool {
        self.approx_collinear_with_threshold(a_seg, 1)
    }

    /// Port of `SEG::ApproxCollinear( aSeg, aDistanceThreshold )`.
    pub fn approx_collinear_with_threshold(&self, a_seg: &Seg, a_distance_threshold: i32) -> bool {
        let threshold_squared: Ecoord = Seg::square(a_distance_threshold);

        let Some((d1_sq, d2_sq)) = self.mutual_distance_squared(a_seg) else {
            return false;
        };

        d1_sq.abs() <= threshold_squared && d2_sq.abs() <= threshold_squared
    }

    /// Port of `SEG::ApproxParallel( aSeg )` with the default `aDistanceThreshold = 1`.
    #[inline]
    pub fn approx_parallel(&self, a_seg: &Seg) -> bool {
        self.approx_parallel_with_threshold(a_seg, 1)
    }

    /// Port of `SEG::ApproxParallel( aSeg, aDistanceThreshold )`.
    pub fn approx_parallel_with_threshold(&self, a_seg: &Seg, a_distance_threshold: i32) -> bool {
        let threshold_squared: Ecoord = Seg::square(a_distance_threshold);

        let Some((d1_sq, d2_sq)) = self.mutual_distance_squared(a_seg) else {
            return false;
        };

        (d1_sq - d2_sq).abs() <= threshold_squared
    }

    /// Port of `SEG::ApproxPerpendicular()`.
    pub fn approx_perpendicular(&self, a_seg: &Seg) -> bool {
        let perp: Seg = self.perpendicular_seg(self.a);

        a_seg.approx_parallel(&perp)
    }

    /// Port of `SEG::Overlaps()`.
    pub fn overlaps(&self, a_seg: &Seg) -> bool {
        if a_seg.a == a_seg.b {
            // single point corner case
            if self.a == a_seg.a || self.b == a_seg.a {
                return false;
            }

            return self.contains(a_seg.a);
        }

        if !self.collinear(a_seg) {
            return false;
        }

        if self.contains(a_seg.a) || self.contains(a_seg.b) {
            return true;
        }

        if a_seg.contains(self.a) || a_seg.contains(self.b) {
            return true;
        }

        false
    }

    /// Port of `SEG::ApproxOverlaps( aSeg )` with the default `aThreshold = 3`.
    #[inline]
    pub fn approx_overlaps(&self, a_seg: &Seg) -> bool {
        self.approx_overlaps_with_threshold(a_seg, 3)
    }

    /// Port of `SEG::ApproxOverlaps( aSeg, aThreshold )`.
    pub fn approx_overlaps_with_threshold(&self, a_seg: &Seg, a_threshold: i32) -> bool {
        if self.a == self.b {
            // single point corner case
            if a_seg.a == self.a || a_seg.b == self.a {
                return false;
            }

            return a_seg.contains_with_threshold(self.a, a_threshold);
        }

        if !a_seg.approx_collinear_with_threshold(self, a_threshold) {
            return false;
        }

        if a_seg.contains_with_threshold(self.a, a_threshold)
            || a_seg.contains_with_threshold(self.b, a_threshold)
        {
            return true;
        }

        if self.contains_with_threshold(a_seg.a, a_threshold)
            || self.contains_with_threshold(a_seg.b, a_threshold)
        {
            return true;
        }

        false
    }

    /// Port of `SEG::Contains( const SEG& aSeg )`.
    pub fn contains_seg(&self, a_seg: &Seg) -> bool {
        if a_seg.a == a_seg.b {
            // single point corner case
            return self.contains(a_seg.a);
        }

        if !self.collinear(a_seg) {
            return false;
        }

        if self.contains(a_seg.a) && self.contains(a_seg.b) {
            return true;
        }

        false
    }

    /// Port of `SEG::Length()`: return the length (this).
    #[inline]
    pub fn length(&self) -> i32 {
        (self.a - self.b).euclidean_norm()
    }

    /// Port of `SEG::SquaredLength()`.
    #[inline]
    pub fn squared_length(&self) -> Ecoord {
        (self.a - self.b).squared_euclidean_norm()
    }

    /// Port of `SEG::TCoef()`.
    #[inline]
    pub fn t_coef(&self, a_p: Vector2I) -> Ecoord {
        let d: Vector2I = self.b - self.a;

        d.dot(a_p - self.a)
    }

    /// Port of `SEG::Index()`.
    ///
    /// Return the index of this segment in its parent shape (applicable only to non-local
    /// segments).
    #[inline]
    pub const fn index(&self) -> i32 {
        self.index
    }

    /// Port of `SEG::Contains( const VECTOR2I& aP )` with the default
    /// `aSqDistanceThreshold = 3`.
    #[inline]
    pub fn contains(&self, a_p: Vector2I) -> bool {
        self.contains_with_threshold(a_p, 3)
    }

    /// Port of `SEG::Contains( const VECTOR2I& aP, int aSqDistanceThreshold )`.
    pub fn contains_with_threshold(&self, a_p: Vector2I, a_sq_distance_threshold: i32) -> bool {
        /* Warning. This code does work in most - but not all of the cases.
        Take the segment (0,0) - (10,0) and the point (9, 1). With the distance threshold of 3,
        the point is assumed as contained in by the segment:

              (9,1)\
        (0,0) ------ (10, 0)

        I've made the distance threshold configurable (using the default of value of 3),
        to not break any existing code that relies on the current rounding behaviour, but we need to
        think of more accurate (and degeneracy-free) solution. */

        self.squared_distance(a_p) <= a_sq_distance_threshold as Ecoord
    }

    /// Port of `SEG::Reverse()`.
    #[inline]
    pub fn reverse(&mut self) {
        core::mem::swap(&mut self.a, &mut self.b);
    }

    /// Port of `SEG::Reversed()`.  Like the C++, the result has no parent index.
    #[inline]
    pub fn reversed(&self) -> Seg {
        Seg::new(self.b, self.a)
    }

    /// Port of `SEG::Center()`: returns the center point of the line.
    #[inline]
    pub fn center(&self) -> Vector2I {
        self.a + (self.b - self.a) / 2
    }

    /// Port of `SEG::checkCollinearOverlap`.
    fn check_collinear_overlap(
        &self,
        a_seg: &Seg,
        use_x_axis: bool,
        a_ignore_endpoints: bool,
        a_pt: Option<&mut Vector2I>,
    ) -> bool {
        // Extract coordinates based on the chosen axis
        let (seg1_start, seg1_end, seg2_start, seg2_end): (i32, i32, i32, i32);
        let (coord1_start, coord1_end): (i32, i32); // For calculating other axis coordinate

        if use_x_axis {
            seg1_start = self.a.x;
            seg1_end = self.b.x;
            seg2_start = a_seg.a.x;
            seg2_end = a_seg.b.x;
            coord1_start = self.a.y;
            coord1_end = self.b.y;
        } else {
            seg1_start = self.a.y;
            seg1_end = self.b.y;
            seg2_start = a_seg.a.y;
            seg2_end = a_seg.b.y;
            coord1_start = self.a.x;
            coord1_end = self.b.x;
        }

        // Find segment ranges on the projection axis
        let seg1_min: i32 = seg1_start.min(seg1_end);
        let seg1_max: i32 = seg1_start.max(seg1_end);
        let seg2_min: i32 = seg2_start.min(seg2_end);
        let seg2_max: i32 = seg2_start.max(seg2_end);

        // Check for overlap
        let overlaps: bool = seg1_max >= seg2_min && seg2_max >= seg1_min;

        if !overlaps {
            return false;
        }

        // Check if intersection is only at endpoints when aIgnoreEndpoints is true
        if a_ignore_endpoints {
            // Calculate overlap region
            let overlap_start: i32 = seg1_min.max(seg2_min);
            let overlap_end: i32 = seg1_max.min(seg2_max);

            // If overlap region has zero length, segments only touch at endpoint
            if overlap_start == overlap_end {
                // Check if this endpoint touching involves actual segment endpoints
                // (not just projected endpoints due to min/max calculation)
                let mut is_endpoint_touch: bool = false;

                // Check if the touch point corresponds to actual segment endpoints
                if overlap_start == seg1_min || overlap_start == seg1_max {
                    // Touch point is at seg1's endpoint, check if it's also at seg2's endpoint
                    if overlap_start == seg2_min || overlap_start == seg2_max {
                        is_endpoint_touch = true;
                    }
                }

                if is_endpoint_touch {
                    return false; // Ignore endpoint-only intersection
                }
            }
        }

        // Calculate intersection point if requested
        if let Some(a_pt) = a_pt {
            // Find midpoint of overlap region
            let overlap_start: i32 = seg1_min.max(seg2_min);
            let overlap_end: i32 = seg1_max.min(seg2_max);
            let intersection_proj: i32 = (overlap_start + overlap_end) / 2;

            // Calculate corresponding coordinate on the other axis
            let intersection_other: i32 = if seg1_end != seg1_start {
                // Use this segment's line equation to find other coordinate
                coord1_start
                    + rescale(
                        intersection_proj - seg1_start,
                        coord1_end - coord1_start,
                        seg1_end - seg1_start,
                    )
            } else {
                // Degenerate segment (point) or perpendicular to projection axis
                coord1_start
            };

            // Set result based on projection axis
            if use_x_axis {
                *a_pt = Vector2I::new(intersection_proj, intersection_other);
            } else {
                *a_pt = Vector2I::new(intersection_other, intersection_proj);
            }
        }

        true
    }

    /// Port of the private `SEG::intersects`.
    fn intersects_impl(
        &self,
        a_seg: &Seg,
        a_ignore_endpoints: bool,
        a_lines: bool,
        a_pt: Option<&mut Vector2I>,
    ) -> bool {
        // Quick rejection: check if segment bounding boxes overlap
        // (Skip for line mode since infinite lines can intersect anywhere)
        if !a_lines {
            let this_min_x: i32 = self.a.x.min(self.b.x);
            let this_max_x: i32 = self.a.x.max(self.b.x);
            let this_min_y: i32 = self.a.y.min(self.b.y);
            let this_max_y: i32 = self.a.y.max(self.b.y);

            let other_min_x: i32 = a_seg.a.x.min(a_seg.b.x);
            let other_max_x: i32 = a_seg.a.x.max(a_seg.b.x);
            let other_min_y: i32 = a_seg.a.y.min(a_seg.b.y);
            let other_max_y: i32 = a_seg.a.y.max(a_seg.b.y);

            if this_max_x < other_min_x
                || other_max_x < this_min_x
                || this_max_y < other_min_y
                || other_max_y < this_min_y
            {
                return false;
            }
        }

        // Calculate direction vectors and offset vector using VECTOR2 operations
        // Using parametric form: P₁ = A + t*dir1, P₂ = aSeg.A + s*dir2
        let dir1: Vector2L = Vector2L::from(self.b) - self.a; // direction vector e
        let dir2: Vector2L = Vector2L::from(a_seg.b) - a_seg.a; // direction vector f
        let offset: Vector2L = Vector2L::from(a_seg.a) - self.a; // offset vector ac
        let determinant: Ecoord = dir2.cross(dir1);

        // Handle parallel/collinear case
        if determinant == 0 {
            // Check if lines are collinear (not just parallel) using cross product
            // Lines are collinear if offset vector is also parallel to direction vector
            let collinear_test: Ecoord = dir1.cross(offset);

            if collinear_test != 0 {
                return false; // Parallel but not collinear
            }

            // Lines are collinear - for infinite lines, they always intersect
            if a_lines {
                // For infinite collinear lines, intersection point is ambiguous
                // Use the midpoint between the two segment start points as a reasonable choice
                if let Some(a_pt) = a_pt {
                    // If aSeg is degenerate (point), use its start point
                    if a_seg.a == a_seg.b {
                        *a_pt = a_seg.a;
                    } else if self.a == self.b {
                        // If this segment is degenerate (point), use its start point
                        *a_pt = self.a;
                    } else {
                        let midpoint: Vector2I = (self.a + a_seg.a) / 2;
                        *a_pt = midpoint;
                    }
                }

                return true;
            }

            // For segments, check overlap using the axis with larger coordinate range
            let use_x_axis: bool = dir1.x.abs() >= dir1.y.abs();

            return self.check_collinear_overlap(a_seg, use_x_axis, a_ignore_endpoints, a_pt);
        }

        // param2_num = f × ac (parameter for second segment: s = p/d)
        // param1_num = e × ac (parameter for first segment: t = q/d)
        let param2_num: Ecoord = dir2.cross(offset);
        let param1_num: Ecoord = dir1.cross(offset);

        // For segments (not infinite lines), check if intersection is within both segments
        if !a_lines {
            // Parameters must be in [0,1] for intersection within segments
            // Since we're comparing t = q/d and s = p/d to [0,1], we need to handle sign of d
            if determinant > 0 {
                // d > 0: check 0 ≤ q ≤ d and 0 ≤ p ≤ d
                if param1_num < 0
                    || param1_num > determinant
                    || param2_num < 0
                    || param2_num > determinant
                {
                    return false;
                }
            } else {
                // d < 0: check d ≤ q ≤ 0 and d ≤ p ≤ 0
                if param1_num > 0
                    || param1_num < determinant
                    || param2_num > 0
                    || param2_num < determinant
                {
                    return false;
                }
            }

            // Optionally exclude endpoint intersections (when segments share vertices)
            if a_ignore_endpoints
                && (param1_num == 0 || param1_num == determinant)
                && (param2_num == 0 || param2_num == determinant)
            {
                return false;
            }
        }

        if let Some(a_pt) = a_pt {
            // Use parametric equation: intersection = aSeg.A + (q/d) * f
            let scaled_dir2: Vector2L = Vector2L::new(
                rescale(param1_num, dir2.x, determinant),
                rescale(param1_num, dir2.y, determinant),
            );
            let result: Vector2L = Vector2L::from(a_seg.a) + scaled_dir2;

            // Verify result fits in coordinate type range
            const MAX_COORD: Ecoord = i32::MAX as Ecoord;
            const MIN_COORD: Ecoord = i32::MIN as Ecoord;

            if result.x > MAX_COORD
                || result.x < MIN_COORD
                || result.y > MAX_COORD
                || result.y < MIN_COORD
            {
                return false; // Intersection exists but coordinates overflow
            }

            *a_pt = Vector2I::new(result.x as i32, result.y as i32);
        }

        true
    }

    /// Port of `SEG::mutualDistanceSquared`: returns `( aD1, aD2 )`, or `None` where the C++
    /// returns `false`.
    fn mutual_distance_squared(&self, a_seg: &Seg) -> Option<(Ecoord, Ecoord)> {
        let mut a: Seg = *self;
        let mut b: Seg = *a_seg;

        if a.squared_length() < b.squared_length() {
            core::mem::swap(&mut a, &mut b);
        }

        let p: Ecoord = a.a.y as Ecoord - a.b.y as Ecoord;
        let q: Ecoord = a.b.x as Ecoord - a.a.x as Ecoord;
        let r: Ecoord = -p * a.a.x as Ecoord - q * a.a.y as Ecoord;

        let l: Ecoord = p * p + q * q;

        if l == 0 {
            return None;
        }

        let det1: Ecoord = p * b.a.x as Ecoord + q * b.a.y as Ecoord + r;
        let det2: Ecoord = p * b.b.x as Ecoord + q * b.b.y as Ecoord + r;

        let dsq1: Ecoord = rescale(det1, det1, l);
        let dsq2: Ecoord = rescale(det2, det2, l);

        Some((sgn(det1) * dsq1, sgn(det2) * dsq2))
    }
}
