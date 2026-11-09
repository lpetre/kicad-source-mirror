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

//! Test support, reusable by the tests of this crate and of the crates built on it.
//!
//! Ports of the C++ QA helpers that the kimath unit tests share:
//!
//! * `qa/qa_utils/include/qa_utils/numeric.h` (`KI_TEST::IsWithin` and friends),
//! * `qa/qa_utils/include/qa_utils/geometry/geometry.h` (`KI_TEST::IsVecWithinTol`,
//!   `KI_TEST::IsBoxWithinTol`),
//! * `qa/qa_utils/geometry/seg_construction.cpp` (`KI_TEST::BuildHSeg`, `BuildVSeg`),
//! * `qa/tests/libs/kimath/geometry/geom_test_utils.h` (namespace `GEOM_TEST`),
//! * the point and segment data of `qa/tests/libs/kimath/geometry/fixtures_geometry.h`
//!   (`KI_TEST::CommonTestData`).
//!
//! The helpers that need shapes which are not ported yet (`ChainsAreCyclicallyEqual`,
//! `FilletPolySet`, `IsOutlineValid`, `IsPolySetValid`, the polygon sets of
//! `CommonTestData`, ...) are left to the shapes work package.
//!
//! It also contains the standard Mersenne Twister engines (`std::mt19937`,
//! `std::mt19937_64`) and replicas of the libstdc++ distributions, so that randomised C++
//! tests can be ported with the very same pseudo-random input data.
//!
//! This module is plain library code without dependencies (it also builds for wasm); it is
//! not meant to be used outside of tests.

use core::ops::{Add, Sub};

use crate::geometry::eda_angle::{ANGLE_90, ANGLE_180, EdaAngle};
use crate::geometry::seg::Seg;
use crate::math::box2::{Box2D, Box2I, Box2L};
use crate::math::vector2::{Coord, Vector2, Vector2D, Vector2I};

/// Port of `KI_TEST::IsWithinWrapped`.
///
/// Check if a value is within a tolerance of a nominal value, wrapping to a given val.
///
/// Returns value is in [( aNominal - aError ) % aWrap, ( aNominal + aError ) % aWrap]
pub fn is_within_wrapped<T>(a_value: T, a_nominal: T, a_wrap: T, a_error: T) -> bool
where
    T: Copy + Sub<Output = T> + Into<f64>,
{
    let wrap: f64 = a_wrap.into();

    // Compute shortest signed distance on a ring
    let mut diff: f64 = (a_value - a_nominal).into() % wrap;

    if diff > wrap / 2.0 {
        diff -= wrap;
    } else if diff < -wrap / 2.0 {
        diff += wrap;
    }

    diff.abs() <= a_error.into()
}

/// Port of `KI_TEST::IsWithin`.
///
/// Check if a value is within a tolerance of a nominal value.
///
/// Returns value is in [aNominal - aError, aNominal + aError]
pub fn is_within<T>(a_value: T, a_nominal: T, a_error: T) -> bool
where
    T: Copy + PartialOrd + Add<Output = T> + Sub<Output = T>,
{
    (a_value >= a_nominal - a_error) && (a_value <= a_nominal + a_error)
}

/// Port of `KI_TEST::IsWithinBounds`.
///
/// Check if a value is within a tolerance of a nominal value,
/// with different allowances for errors above and below.
///
/// Returns value is in [aNominal - aErrorBelow, aNominal + aErrorAbove]
pub fn is_within_bounds<T>(a_value: T, a_nominal: T, a_error_above: T, a_error_below: T) -> bool
where
    T: Copy + PartialOrd + Add<Output = T> + Sub<Output = T>,
{
    (a_value >= a_nominal - a_error_below) && (a_value <= a_nominal + a_error_above)
}

/// Port of `KI_TEST::IsWithinAndBelow`.
///
/// value is in range [aNominal - aErrorBelow, aNominal]
pub fn is_within_and_below<T>(a_value: T, a_nominal: T, a_error_below: T) -> bool
where
    T: Copy + PartialOrd + Add<Output = T> + Sub<Output = T> + Default,
{
    is_within_bounds(a_value, a_nominal, T::default(), a_error_below)
}

/// Port of `KI_TEST::IsWithinAndAbove`.
///
/// value is in range [aNominal, aNominal + aErrorAbove]
pub fn is_within_and_above<T>(a_value: T, a_nominal: T, a_error_above: T) -> bool
where
    T: Copy + PartialOrd + Add<Output = T> + Sub<Output = T> + Default,
{
    is_within_bounds(a_value, a_nominal, a_error_above, T::default())
}

/// Port of `KI_TEST::IsVecWithinTol`.
///
/// Check that both x and y of a vector are within expected error.
///
/// Note that the C++ predicate is a template over the vector type: a call such as
/// `IsVecWithinTol<VECTOR2I>( vecD, expD, 0.000001 )` first converts all its arguments to
/// `VECTOR2I` / `int`.  Ported tests reproduce that with [`Vector2::cast`].
pub fn is_vec_within_tol<T>(a_vec: Vector2<T>, a_exp: Vector2<T>, a_tol: T) -> bool
where
    T: Coord + Add<Output = T> + Sub<Output = T>,
{
    is_within(a_vec.x, a_exp.x, a_tol) && is_within(a_vec.y, a_exp.y, a_tol)
}

/// The box types `KI_TEST::IsBoxWithinTol` can be instantiated with.
pub trait BoxWithinTol {
    /// `BOX::coord_type`.
    type CoordType;

    /// See [`is_box_within_tol`].
    fn is_box_within_tol(&self, a_exp: &Self, a_tol: Self::CoordType) -> bool;
}

macro_rules! impl_box_within_tol {
    ($b:ty, $c:ty) => {
        impl BoxWithinTol for $b {
            type CoordType = $c;

            fn is_box_within_tol(&self, a_exp: &Self, a_tol: $c) -> bool {
                // using VEC = VECTOR2<typename BOX::coord_type>: the sizes are converted
                is_vec_within_tol::<$c>(self.position(), a_exp.position(), a_tol)
                    && is_vec_within_tol::<$c>(
                        self.size().cast(),
                        a_exp.size().cast(),
                        a_tol * (2 as $c),
                    )
            }
        }
    };
}

impl_box_within_tol!(Box2I, i32);
impl_box_within_tol!(Box2L, i64);
impl_box_within_tol!(Box2D, f64);

/// Port of `KI_TEST::IsBoxWithinTol`.
///
/// Check that a box is close enough to another box.
pub fn is_box_within_tol<B: BoxWithinTol>(a_box: &B, a_exp: &B, a_tol: B::CoordType) -> bool {
    a_box.is_box_within_tol(a_exp, a_tol)
}

/// Port of `KI_TEST::BuildHSeg`.
///
/// Build a horizontal segment from a point with a length.
pub fn build_h_seg(a_start: Vector2I, a_length: i32) -> Seg {
    Seg::new(
        Vector2I::new(a_start.x, a_start.y),
        Vector2I::new(a_start.x + a_length, a_start.y),
    )
}

/// Port of `KI_TEST::BuildVSeg`.
///
/// Build a vertical segment from a point with a length.
pub fn build_v_seg(a_start: Vector2I, a_length: i32) -> Seg {
    Seg::new(
        Vector2I::new(a_start.x, a_start.y),
        Vector2I::new(a_start.x, a_start.y + a_length),
    )
}

/// Port of `GEOM_TEST::QUADRANT`.
///
/// Geometric quadrants, from top-right, anti-clockwise
///
/// ```text
///     ^ y
///     |
///  Q2 | Q1
///  -------> x
///  Q3 | Q4
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quadrant {
    /// x and y both >= 0
    Q1,
    /// x <= 0, y >= 0
    Q2,
    /// x and y both <= 0
    Q3,
    /// x >= 0, y <= 0
    Q4,
}

/// Port of `GEOM_TEST::IsInQuadrant`.
pub fn is_in_quadrant<T: Coord>(a_point: Vector2<T>, a_quadrant: Quadrant) -> bool {
    let zero = T::ZERO;

    match a_quadrant {
        Quadrant::Q1 => a_point.x >= zero && a_point.y >= zero,
        Quadrant::Q2 => a_point.x <= zero && a_point.y >= zero,
        Quadrant::Q3 => a_point.x <= zero && a_point.y <= zero,
        Quadrant::Q4 => a_point.x >= zero && a_point.y <= zero,
    }
}

/// Port of `GEOM_TEST::SegmentCompletelyInQuadrant`.
///
/// Check if both ends of a segment are in the quadrant.
pub fn segment_completely_in_quadrant(a_seg: &Seg, a_quadrant: Quadrant) -> bool {
    is_in_quadrant(a_seg.a, a_quadrant) && is_in_quadrant(a_seg.b, a_quadrant)
}

/// Port of `GEOM_TEST::SegmentEndsInQuadrant`.
///
/// Check if at least one end of the segment is in the quadrant.
pub fn segment_ends_in_quadrant(a_seg: &Seg, a_quadrant: Quadrant) -> bool {
    is_in_quadrant(a_seg.a, a_quadrant) || is_in_quadrant(a_seg.b, a_quadrant)
}

/// Port of `GEOM_TEST::SegmentCompletelyWithinRadius`.
///
/// Check if a segment is entirely within a certain radius of a point.
pub fn segment_completely_within_radius(a_seg: &Seg, a_pt: Vector2I, a_radius: i32) -> bool {
    // This is true iff both ends of the segment are within the radius
    ((a_seg.a - a_pt).euclidean_norm() < a_radius) && ((a_seg.b - a_pt).euclidean_norm() < a_radius)
}

/// Port of `GEOM_TEST::IsPointAtDistance`.
///
/// Check that two points are the given distance apart, within the given tolerance.
///
/// (The C++ template stores the distance in an `int`, so it is only usable with
/// `T = int`.)
pub fn is_point_at_distance(
    a_pt_a: Vector2I,
    a_pt_b: Vector2I,
    a_exp_dist: i32,
    a_tol: i32,
) -> bool {
    let dist: i32 = (a_pt_b - a_pt_a).euclidean_norm();

    is_within(dist, a_exp_dist, a_tol)
}

/// Port of `GEOM_TEST::ArePointsNearCircle`.
///
/// Predicate for checking a set of points is within a certain tolerance of a circle.
///
/// * `a_points` - the points to check
/// * `a_centre` - the circle centre
/// * `a_rad` - the circle radius
/// * `a_tol` - the circle tolerance
pub fn are_points_near_circle(
    a_points: &[Vector2I],
    a_centre: Vector2I,
    a_rad: i32,
    a_tol: i32,
) -> bool {
    let mut ok = true;

    for point in a_points {
        if !is_point_at_distance(*point, a_centre, a_rad, a_tol) {
            ok = false;
        }
    }

    ok
}

/// Port of `GEOM_TEST::ArePerpendicular`.
///
/// Check if two vectors are perpendicular.
///
/// * `a_tolerance` - the allowed deviation from PI/2 (e.g. when rounding)
pub fn are_perpendicular(
    a: impl Into<Vector2D>,
    b: impl Into<Vector2D>,
    a_tolerance: EdaAngle,
) -> bool {
    let mut angle: EdaAngle = (EdaAngle::from_vector(a) - EdaAngle::from_vector(b)).abs();

    // Normalise: angles of 3*pi/2 are also perpendicular
    if angle > ANGLE_180 {
        angle -= ANGLE_180;
    }

    is_within(
        angle.as_radians(),
        ANGLE_90.as_radians(),
        a_tolerance.as_radians(),
    )
}

/// Port of `GEOM_TEST::SegmentsHaveSameEndPoints`.
///
/// Check that two SEGs have the same end points, in either order.
///
/// That is to say SEG(A, B) == SEG(A, B), but also SEG(A, B) == SEG(B, A)
pub fn segments_have_same_end_points(a_seg1: &Seg, a_seg2: &Seg) -> bool {
    (a_seg1.a == a_seg2.a && a_seg1.b == a_seg2.b) || (a_seg1.a == a_seg2.b && a_seg1.b == a_seg2.a)
}

/// Port of `GEOM_TEST::TriangleMinAngleDeg`.
///
/// The smallest interior angle of a triangle, in degrees; near zero for a sliver.
pub fn triangle_min_angle_deg(a: Vector2I, b: Vector2I, c: Vector2I) -> f64 {
    let angle_at = |v: Vector2I, p: Vector2I, q: Vector2I| -> f64 {
        let u = Vector2D::new((p.x - v.x) as f64, (p.y - v.y) as f64);
        let w = Vector2D::new((q.x - v.x) as f64, (q.y - v.y) as f64);

        crate::cmath::atan2((u.x * w.y - u.y * w.x).abs(), u.x * w.x + u.y * w.y) * 180.0
            / std::f64::consts::PI
    };

    let a1 = angle_at(a, b, c);
    let a2 = angle_at(b, c, a);
    let a3 = angle_at(c, a, b);

    crate::math::util::std_min(crate::math::util::std_min(a1, a2), a3)
}

/// The point and segment data of `KI_TEST::CommonTestData`
/// (`qa/tests/libs/kimath/geometry/fixtures_geometry.h`).
///
/// The polygon sets of the C++ fixture are built from this data by the shapes work package.
#[derive(Clone, Debug)]
pub struct CommonTestData {
    /// `uniquePoints`: the single vertex of `uniqueVertexPolySet`.
    pub unique_points: Vec<Vector2I>,
    /// `holeyPoints`: a square outline, a pentagon hole and a triangle hole (12 points).
    pub holey_points: Vec<Vector2I>,
    /// `holeySegments`: the segments of `holeyPolySet`.
    pub holey_segments: Vec<Seg>,
}

impl Default for CommonTestData {
    fn default() -> Self {
        Self::new()
    }
}

impl CommonTestData {
    /// Port of the vector-populating part of `CommonTestData::CommonTestData()`.
    pub fn new() -> Self {
        // UniqueVertexPolySet shall have a unique vertex
        let unique_points = vec![Vector2I::new(100, 50)];

        // Populate the holey polygon set points with 12 points
        let holey_points = vec![
            // Square
            Vector2I::new(100, 100),
            Vector2I::new(0, 100),
            Vector2I::new(0, 0),
            Vector2I::new(100, 0),
            // Pentagon
            Vector2I::new(10, 10),
            Vector2I::new(10, 20),
            Vector2I::new(15, 15),
            Vector2I::new(20, 20),
            Vector2I::new(20, 10),
            // Triangle
            Vector2I::new(40, 10),
            Vector2I::new(40, 20),
            Vector2I::new(60, 10),
        ];

        // Save the segments of the holeyPolySet.
        let p = &holey_points;
        let holey_segments = vec![
            Seg::new(p[0], p[1]),
            Seg::new(p[1], p[2]),
            Seg::new(p[2], p[3]),
            Seg::new(p[3], p[0]),
            // Pentagon segments
            Seg::new(p[4], p[5]),
            Seg::new(p[5], p[6]),
            Seg::new(p[6], p[7]),
            Seg::new(p[7], p[8]),
            Seg::new(p[8], p[4]),
            // Triangle segments
            Seg::new(p[9], p[10]),
            Seg::new(p[10], p[11]),
            Seg::new(p[11], p[9]),
        ];

        CommonTestData {
            unique_points,
            holey_points,
            holey_segments,
        }
    }
}

/// `std::mt19937`: the 32-bit Mersenne Twister engine, whose output sequence is fully
/// specified by the C++ standard.
#[derive(Clone)]
pub struct Mt19937 {
    mt: [u32; Self::N],
    index: usize,
}

impl Mt19937 {
    const N: usize = 624;
    const M: usize = 397;

    /// `std::mt19937 rng( seed )`.
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; Self::N];
        mt[0] = seed;

        for i in 1..Self::N {
            mt[i] = 1812433253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }

        Mt19937 { mt, index: Self::N }
    }

    /// `rng()`: the next 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= Self::N {
            for i in 0..Self::N {
                let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % Self::N] & 0x7fff_ffff);
                let mut next = self.mt[(i + Self::M) % Self::N] ^ (y >> 1);

                if y & 1 != 0 {
                    next ^= 0x9908_b0df;
                }

                self.mt[i] = next;
            }

            self.index = 0;
        }

        let mut y = self.mt[self.index];
        self.index += 1;

        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
}

/// `std::mt19937_64`: the 64-bit Mersenne Twister engine, whose output sequence is fully
/// specified by the C++ standard.
#[derive(Clone)]
pub struct Mt19937_64 {
    mt: [u64; Self::N],
    index: usize,
}

impl Mt19937_64 {
    const N: usize = 312;
    const M: usize = 156;

    /// `std::mt19937_64 rng( seed )`.
    pub fn new(seed: u64) -> Self {
        let mut mt = [0u64; Self::N];
        mt[0] = seed;

        for i in 1..Self::N {
            mt[i] = 6364136223846793005u64
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 62))
                .wrapping_add(i as u64);
        }

        Mt19937_64 { mt, index: Self::N }
    }

    /// `rng()`: the next 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        if self.index >= Self::N {
            for i in 0..Self::N {
                let y = (self.mt[i] & 0xffff_ffff_8000_0000)
                    | (self.mt[(i + 1) % Self::N] & 0x7fff_ffff);
                let mut next = self.mt[(i + Self::M) % Self::N] ^ (y >> 1);

                if y & 1 != 0 {
                    next ^= 0xb502_6f5a_a966_19e9;
                }

                self.mt[i] = next;
            }

            self.index = 0;
        }

        let mut y = self.mt[self.index];
        self.index += 1;

        y ^= (y >> 29) & 0x5555_5555_5555_5555;
        y ^= (y << 17) & 0x71d6_7fff_eda6_0000;
        y ^= (y << 37) & 0xfff7_eee0_0000_0000;
        y ^= y >> 43;
        y
    }
}

/// `std::uniform_int_distribution<T>( a, b )( rng )` for a `std::mt19937` engine, as
/// implemented by libstdc++ (GCC 11 and later; Lemire's nearly divisionless algorithm).
///
/// The distributions of the C++ standard library are implementation-defined; this replica
/// reproduces the reference (GCC/Linux) build so that ported randomised tests see the same
/// data.  Only ranges narrower than 2^32 are supported, which is all the ported tests use.
pub fn uniform_int_libstdcxx(rng: &mut Mt19937, a: i64, b: i64) -> i64 {
    let urange: u64 = (b as u64).wrapping_sub(a as u64);

    assert!(
        urange < 0xffff_ffff,
        "only ranges narrower than the engine range are supported"
    );

    let uerange: u32 = (urange + 1) as u32;

    // _S_nd<uint64_t>( urng, uerange )
    let mut product: u64 = rng.next_u32() as u64 * uerange as u64;
    let mut low: u32 = product as u32;

    if low < uerange {
        let threshold: u32 = uerange.wrapping_neg() % uerange;

        while low < threshold {
            product = rng.next_u32() as u64 * uerange as u64;
            low = product as u32;
        }
    }

    ((product >> 32) as i64).wrapping_add(a)
}

/// `std::uniform_real_distribution<double>( a, b )( rng )` for a `std::mt19937` engine, as
/// implemented by libstdc++ (`std::generate_canonical<double, 53>` drawing two 32-bit
/// values).  See [`uniform_int_libstdcxx`] for why this exists.
pub fn uniform_real_libstdcxx(rng: &mut Mt19937, a: f64, b: f64) -> f64 {
    let r: f64 = 4294967296.0;
    let mut sum: f64 = 0.0;
    let mut tmp: f64 = 1.0;

    for _ in 0..2 {
        sum += rng.next_u32() as f64 * tmp;
        tmp *= r;
    }

    let mut ret: f64 = sum / tmp;

    if ret >= 1.0 {
        // std::nextafter( 1.0, 0.0 )
        ret = 1.0 - f64::EPSILON / 2.0;
    }

    (ret * (b - a)) + a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mersenne_twister_reference_values() {
        // The C++ standard requires the 10000th consecutive invocation of a
        // default-constructed std::mt19937 (seed 5489) to produce 4123659995, and of
        // std::mt19937_64 to produce 9981545732273789042.
        let mut rng = Mt19937::new(5489);
        let mut last = 0;

        for _ in 0..10000 {
            last = rng.next_u32();
        }

        assert_eq!(last, 4123659995);

        let mut rng64 = Mt19937_64::new(5489);
        let mut last64 = 0;

        for _ in 0..10000 {
            last64 = rng64.next_u64();
        }

        assert_eq!(last64, 9981545732273789042);
    }

    #[test]
    fn within_predicates() {
        assert!(is_within(10, 12, 2));
        assert!(!is_within(9, 12, 2));
        assert!(is_within(1.0, 1.1, 0.11));
        assert!(is_within_bounds(13, 12, 1, 0));
        assert!(!is_within_bounds(11, 12, 1, 0));
        assert!(is_within_and_below(11, 12, 1));
        assert!(!is_within_and_below(13, 12, 1));
        assert!(is_within_and_above(13, 12, 1));
        assert!(!is_within_and_above(11, 12, 1));
        assert!(is_within_wrapped(359.0, 1.0, 360.0, 2.0));
        assert!(!is_within_wrapped(350.0, 1.0, 360.0, 2.0));
        assert!(is_within_wrapped(1.0, 359.0, 360.0, 2.0));

        assert!(is_vec_within_tol(
            Vector2I::new(10, 10),
            Vector2I::new(11, 9),
            1
        ));
        assert!(!is_vec_within_tol(
            Vector2I::new(10, 10),
            Vector2I::new(12, 9),
            1
        ));

        let a = Box2I::new(Vector2I::new(0, 0), Vector2I::new(10, 10));
        let b = Box2I::new(Vector2I::new(1, -1), Vector2I::new(12, 8));
        assert!(is_box_within_tol(&a, &b, 1));
        assert!(!is_box_within_tol(
            &a,
            &Box2I::new(Vector2I::new(1, -1), Vector2I::new(13, 8)),
            1
        ));
    }

    #[test]
    fn geometry_predicates() {
        assert!(is_in_quadrant(Vector2I::new(1, 1), Quadrant::Q1));
        assert!(is_in_quadrant(Vector2I::new(-1, 1), Quadrant::Q2));
        assert!(is_in_quadrant(Vector2I::new(-1, -1), Quadrant::Q3));
        assert!(is_in_quadrant(Vector2I::new(1, -1), Quadrant::Q4));
        assert!(is_in_quadrant(Vector2I::new(0, 0), Quadrant::Q3));
        assert!(!is_in_quadrant(Vector2I::new(1, 1), Quadrant::Q3));

        let seg = build_h_seg(Vector2I::new(-5, 3), 10);
        assert_eq!(seg, Seg::from_coords(-5, 3, 5, 3));
        assert!(!segment_completely_in_quadrant(&seg, Quadrant::Q1));
        assert!(segment_ends_in_quadrant(&seg, Quadrant::Q1));
        assert!(segment_completely_within_radius(
            &seg,
            Vector2I::new(0, 0),
            7
        ));
        assert!(!segment_completely_within_radius(
            &seg,
            Vector2I::new(0, 0),
            5
        ));
        assert_eq!(
            build_v_seg(Vector2I::new(1, 2), 3),
            Seg::from_coords(1, 2, 1, 5)
        );

        assert!(is_point_at_distance(
            Vector2I::new(0, 0),
            Vector2I::new(3, 4),
            5,
            0
        ));
        assert!(are_points_near_circle(
            &[
                Vector2I::new(10, 0),
                Vector2I::new(0, -10),
                Vector2I::new(7, 7)
            ],
            Vector2I::new(0, 0),
            10,
            0
        ));
        assert!(!are_points_near_circle(
            &[Vector2I::new(10, 0), Vector2I::new(5, 5)],
            Vector2I::new(0, 0),
            10,
            1
        ));

        let tol = EdaAngle::new(0.001, crate::geometry::eda_angle::DEGREES_T);
        assert!(are_perpendicular(
            Vector2I::new(10, 0),
            Vector2I::new(0, 7),
            tol
        ));
        assert!(are_perpendicular(
            Vector2I::new(10, 0),
            Vector2I::new(0, -7),
            tol
        ));
        assert!(!are_perpendicular(
            Vector2I::new(10, 0),
            Vector2I::new(7, 7),
            tol
        ));

        assert!(segments_have_same_end_points(&seg, &seg.reversed()));
        assert!(!segments_have_same_end_points(
            &seg,
            &build_v_seg(Vector2I::new(-5, 3), 10)
        ));

        let min_angle = triangle_min_angle_deg(
            Vector2I::new(0, 0),
            Vector2I::new(10, 0),
            Vector2I::new(0, 10),
        );
        assert!((min_angle - 45.0).abs() < 1e-9);

        let data = CommonTestData::new();
        assert_eq!(data.unique_points.len(), 1);
        assert_eq!(data.holey_points.len(), 12);
        assert_eq!(data.holey_segments.len(), 12);
        assert_eq!(data.holey_segments[3], Seg::from_coords(100, 0, 100, 100));
    }
}

/// Port of `KI_TEST::CheckUnorderedMatches()` (`qa_utils/wx_utils/unit_test_utils.h`):
/// check that every expected item is matched by a found item.
///
/// Returns the descriptions of the failures (empty when everything matches); the Boost
/// version reports them with `BOOST_CHECK_MESSAGE`.
pub fn check_unordered_matches<E: std::fmt::Debug, F>(
    a_expected: &[E],
    a_found: &[F],
    a_match_predicate: impl Fn(&E, &F) -> bool,
) -> Vec<String> {
    // set of object we've already found
    let mut matched = vec![false; a_expected.len()];

    // fill the set of object that match
    for found in a_found {
        for (i, expected) in a_expected.iter().enumerate() {
            if a_match_predicate(expected, found) {
                matched[i] = true;
                break;
            }
        }
    }

    // check every expected object was "found" (the second loop of the C++ checks that every
    // matched object was expected, which holds by construction)
    a_expected
        .iter()
        .zip(matched)
        .filter(|(_, m)| !m)
        .map(|(e, _)| format!("Expected item was not found. Expected: {e:?}"))
        .collect()
}

/// `BOOST_CHECK_CLOSE( a, b, tol )`: `a` and `b` are within `a_tolerance_pct` percent of each
/// other, relative to both values (Boost's "strong" check; a zero value only matches zero).
pub fn is_close_pct(a: f64, b: f64, a_tolerance_pct: f64) -> bool {
    if a == b {
        return true;
    }

    let diff = (a - b).abs();
    let safe_div = |x: f64, d: f64| -> f64 {
        if d == 0.0 { if x == 0.0 { 0.0 } else { f64::MAX } } else { x / d }
    };
    let tol = a_tolerance_pct / 100.0;

    safe_div(diff, b.abs()) <= tol && safe_div(diff, a.abs()) <= tol
}

/// `BOOST_CHECK_SMALL( v, tol )`: `|v| <= tol`.
pub fn is_small(v: f64, a_tolerance: f64) -> bool {
    v.abs() <= a_tolerance
}

/// `pcbIUScale.mmToIU()` (`include/base_units.h`) for the tests of this crate:
/// `(int) ( mm < 0 ? ( mm * IU_PER_MM - 0.5 ) : ( mm * IU_PER_MM + 0.5 ) )` with
/// `IU_PER_MM = 1e6`.
pub fn pcb_mm_to_iu(mm: f64) -> i32 {
    const IU_PER_MM: f64 = 1e6;

    (if mm < 0.0 { mm * IU_PER_MM - 0.5 } else { mm * IU_PER_MM + 0.5 }) as i32
}
