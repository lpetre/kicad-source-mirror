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

//! Port of `libs/kimath/include/geometry/geometry_utils.h` and
//! `libs/kimath/src/geometry/geometry_utils.cpp`: a few functions useful in geometry
//! calculations.
//!
//! Also holds `ARC_HIGH_DEF` / `ARC_LOW_DEF` from `include/base_units.h`, which the kimath
//! shapes use as their default approximation accuracy.

use std::cell::Cell;

use crate::geometry::eda_angle::EdaAngle;
use crate::geometry::shape::{Shape, ShapeRef};
use crate::geometry::shape_line_chain::ShapeLineChain;
use crate::geometry::shape_rect::ShapeRect;
use crate::geometry::shape_simple::ShapeSimple;
use crate::math::box2::Box2I;
use crate::math::util::{ki_round, ki_round_to, std_clamp};
use crate::math::vector2::{Vector2, Vector2D, Vector2I, Vector2L};

/// `ARC_LOW_DEF_MM` from `include/base_units.h`.
pub const ARC_LOW_DEF_MM: f64 = 0.02;
/// `ARC_HIGH_DEF_MM` from `include/base_units.h`.
pub const ARC_HIGH_DEF_MM: f64 = 0.005;
/// `ARC_LOW_DEF` (`pcbIUScale.mmToIU( ARC_LOW_DEF_MM )`) from `include/base_units.h`.
pub const ARC_LOW_DEF: i32 = 20000;
/// `ARC_HIGH_DEF` (`pcbIUScale.mmToIU( ARC_HIGH_DEF_MM )`) from `include/base_units.h`.
pub const ARC_HIGH_DEF: i32 = 5000;

/// Port of `LEADER_MODE`: the kind of the leader line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LeaderMode {
    /// `DIRECT`: unconstrained point-to-point
    Direct,
    /// `DEG45`: 45 Degree only
    Deg45,
    /// `DEG90`: 90 Degree only
    Deg90,
}

// To approximate a circle by segments, a minimal seg count is mandatory.
// Note that this is rarely used as the maxError constraint will yield a higher
// segment count on everything but very small circles.  (Even a 0.125mm track
// with a 0.01mm maximum deviation yields 11 segments.)
const MIN_SEGCOUNT_FOR_CIRCLE: i32 = 8;

/// Port of `GetArcToSegmentCount()`: the number of segments to approximate an arc by segments
/// with a given max error (this number is >= 1; in practice >= 2).
///
/// `a_error_max` is the max distance between the middle of a segment and the circle.
pub fn get_arc_to_segment_count(a_radius: i32, a_error_max: i32, a_arc_angle: EdaAngle) -> i32 {
    // calculate the number of segments to approximate a circle by segments
    // given the max distance between the middle of a segment and the circle

    // avoid divide-by-zero
    let a_radius = 1.max(a_radius);
    let a_error_max = 1.max(a_error_max);

    // error relative to the radius value:
    let rel_error: f64 = a_error_max as f64 / a_radius as f64;
    // minimal arc increment in degrees:
    let mut arc_increment: f64 = 180.0 / std::f64::consts::PI * crate::cmath::acos(1.0 - rel_error) * 2.0;

    // Ensure a minimal arc increment reasonable value for a circle
    // (360.0 degrees). For very small radius values, this is mandatory.
    arc_increment = crate::math::util::std_min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE as f64, arc_increment);

    let seg_count: i32 = ki_round(a_arc_angle.as_degrees().abs() / arc_increment);

    // Ensure at least two segments are used for algorithmic safety
    seg_count.max(2)
}

/// Port of `CircleToEndSegmentDeltaRadius()`: the radius difference of the circle defined by
/// segments inside the circle and the radius of the circle tangent to the middle of segments
/// (defined by segments outside this circle).
///
/// `a_radius` is the radius of the circle tangent to the middle of segments, `a_seg_count` the
/// segment count used to approximate the circle.
pub fn circle_to_end_segment_delta_radius(a_radius: i32, a_seg_count: i32) -> i32 {
    let mut a_seg_count = a_seg_count;

    // The minimal seg count is 3, otherwise we cannot calculate the result
    // in practice, the min count is clamped to 8 in kicad
    if a_seg_count <= 2 {
        a_seg_count = 3;
    }

    // The angle between the center of the segment and one end of the segment
    // when the circle is approximated by aSegCount segments
    let alpha: f64 = std::f64::consts::PI / a_seg_count as f64;

    // aRadius is the radius of the circle tangent to the middle of each segment
    // and aRadius/cos(aplha) is the radius of the circle defined by seg ends
    ki_round((a_radius as f64 * (1.0 - 1.0 / crate::cmath::cos(alpha))).abs())
}

// When creating polygons to create a clearance polygonal area, the polygon must
// be same or bigger than the original shape.
// Polygons are bigger if the original shape has arcs (round rectangles, ovals,
// circles...).  However, when building the solder mask layer modifying the shapes
// when converting them to polygons is not acceptable (the modification can break
// calculations).
// So one can disable the shape expansion within a particular scope by allocating
// a DISABLE_ARC_CORRECTION.
//
// The C++ flag is a plain static; the port keeps it per thread (the port is single threaded,
// and a thread local needs no unsafe code).
thread_local! {
    static S_DISABLE_ARC_CORRECTION: Cell<bool> = const { Cell::new(false) };
}

/// Port of `DISABLE_ARC_RADIUS_CORRECTION`: while a value of this type is alive,
/// [`get_circle_to_poly_correction`] returns 0.  As in C++, dropping it clears the flag
/// (it does not restore a previous value).
#[derive(Debug)]
pub struct DisableArcRadiusCorrection {
    _private: (),
}

impl DisableArcRadiusCorrection {
    /// Port of the `DISABLE_ARC_RADIUS_CORRECTION` constructor.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        S_DISABLE_ARC_CORRECTION.with(|f| f.set(true));
        DisableArcRadiusCorrection { _private: () }
    }
}

impl Drop for DisableArcRadiusCorrection {
    fn drop(&mut self) {
        S_DISABLE_ARC_CORRECTION.with(|f| f.set(false));
    }
}

/// Port of `GetCircleToPolyCorrection()`: the radius correction to approximate a circle.
///
/// `a_max_error` is the same error value used to calculate the number of segments.
/// When creating a polygon from a circle, the polygon is inside the circle.
/// Only corners are on the circle.
/// This is incorrect when building clearance areas of circles, that need to build
/// the equivalent polygon outside the circle.
pub fn get_circle_to_poly_correction(a_max_error: i32) -> i32 {
    // Push all the error to the outside by increasing the radius
    if S_DISABLE_ARC_CORRECTION.with(|f| f.get()) { 0 } else { a_max_error }
}

/// Coordinate types accepted by [`get_vector_snapped45`], [`get_vector_snapped90`],
/// [`get_clamped_coords`] and [`is_vec2_safe_xy`] (the instantiations of the C++ templates).
pub trait SnapCoord: Copy + PartialOrd {
    /// The value 0.
    fn zero() -> Self;
    /// `std::abs`
    fn abs_(self) -> Self;
    /// `ext_type( self ) * 2 < other` with `other` widened to `ext_type`
    fn twice_ext_lt(self, other: Self) -> bool;
    /// `std::copysign( mag, sign )` converted back to the coordinate type
    fn copysign_(mag: Self, sign: Self) -> Self;
    /// Clamp as `GetClampedCoords` does for this type.
    fn clamp_coord(self, a_padding: u32) -> Self;
    /// `aVec.x > INT_MIN && aVec.x < INT_MAX` as computed in this type.
    fn is_safe_xy(self) -> bool;
}

macro_rules! impl_snap_coord_int {
    ($t:ty) => {
        impl SnapCoord for $t {
            fn zero() -> Self {
                0
            }
            fn abs_(self) -> Self {
                self.abs()
            }
            fn twice_ext_lt(self, other: Self) -> bool {
                (self as i64) * 2 < other as i64
            }
            fn copysign_(mag: Self, sign: Self) -> Self {
                // std::copysign promotes the integers to double
                (mag as f64).copysign(sign as f64) as $t
            }
            fn clamp_coord(self, a_padding: u32) -> Self {
                let max: i64 = i32::MAX as i64 - a_padding as i64;
                let min: i64 = -max;
                std_clamp(self as i64, min, max) as $t
            }
            #[allow(clippy::unnecessary_cast)]
            fn is_safe_xy(self) -> bool {
                self > i32::MIN as $t && self < i32::MAX as $t
            }
        }
    };
}

impl_snap_coord_int!(i32);
impl_snap_coord_int!(i64);

impl SnapCoord for f64 {
    fn zero() -> Self {
        0.0
    }
    fn abs_(self) -> Self {
        self.abs()
    }
    fn twice_ext_lt(self, other: Self) -> bool {
        self * 2.0 < other
    }
    fn copysign_(mag: Self, sign: Self) -> Self {
        mag.copysign(sign)
    }
    fn clamp_coord(self, a_padding: u32) -> Self {
        let max: f64 = i32::MAX as f64 - a_padding as f64;
        let min: f64 = -max;
        std_clamp(self, min, max)
    }
    fn is_safe_xy(self) -> bool {
        self > i32::MIN as f64 && self < i32::MAX as f64
    }
}

/// Port of `GetVectorSnapped45()`: snap a vector onto the nearest 0, 45 or 90 degree line.
///
/// The magnitude of the vector is NOT kept, instead the coordinates are
/// set equal (and/or opposite) or to zero as needed. The effect of this is
/// that if the starting vector is on a square grid, the resulting snapped
/// vector will still be on the same grid.
pub fn get_vector_snapped45<T: SnapCoord + crate::math::vector2::Coord>(a_vec: Vector2<T>, only45: bool) -> Vector2<T> {
    let mut new_vec = a_vec;
    let abs_vec = Vector2::new(a_vec.x.abs_(), a_vec.y.abs_());

    if !only45 && abs_vec.y.twice_ext_lt(abs_vec.x) {
        // snap along x-axis
        new_vec.y = T::zero();
    } else if !only45 && abs_vec.x.twice_ext_lt(abs_vec.y) {
        // snap onto y-axis
        new_vec.x = T::zero();
    } else if abs_vec.x > abs_vec.y {
        // snap away from x-axis towards 45
        new_vec.y = T::copysign_(a_vec.x, a_vec.y);
    } else {
        // snap away from y-axis towards 45
        new_vec.x = T::copysign_(a_vec.y, a_vec.x);
    }

    new_vec
}

/// Port of `GetVectorSnapped90()`: snap a vector onto the nearest horizontal or vertical line.
///
/// The magnitude of the vector is NOT kept; instead one of the coordinates is
/// set to zero as needed.  If the starting vector is on a square grid, the
/// resulting snapped vector will remain on the same grid.
pub fn get_vector_snapped90<T: SnapCoord + crate::math::vector2::Coord>(a_vec: Vector2<T>) -> Vector2<T> {
    let mut new_vec = a_vec;
    let abs_vec = Vector2::new(a_vec.x.abs_(), a_vec.y.abs_());

    if abs_vec.x >= abs_vec.y {
        new_vec.y = T::zero();
    } else {
        new_vec.x = T::zero();
    }

    new_vec
}

/// Port of `GetClampedCoords<in_type>()` with `ret_type = in_type`: clamp a vector to values
/// that can be negated, respecting numeric limits of coordinates data type with specified
/// padding.  Numeric limits are (-2^31 + 1) to (2^31 - 1).
pub fn get_clamped_coords<T: SnapCoord + crate::math::vector2::Coord>(a_coords: Vector2<T>, a_padding: u32) -> Vector2<T> {
    Vector2::new(a_coords.x.clamp_coord(a_padding), a_coords.y.clamp_coord(a_padding))
}

/// Port of `GetClampedCoords<double, int>()`: as [`get_clamped_coords`], then rounded with
/// `KiROUND`.
pub fn get_clamped_coords_to_i(a_coords: Vector2D, a_padding: u32) -> Vector2I {
    let x = a_coords.x.clamp_coord(a_padding);
    let y = a_coords.y.clamp_coord(a_padding);
    Vector2I::new(ki_round_to::<i32, f64>(x), ki_round_to::<i32, f64>(y))
}

/// Port of `GetClampedCoords<int64_t, int>()`.
pub fn get_clamped_coords_l_to_i(a_coords: Vector2L, a_padding: u32) -> Vector2I {
    let x = a_coords.x.clamp_coord(a_padding);
    let y = a_coords.y.clamp_coord(a_padding);
    Vector2I::new(x as i32, y as i32)
}

/// Port of `IsVec2SafeXY()`: check if both coordinates of a vector are within the limits of
/// the integer type.
pub fn is_vec2_safe_xy<T: SnapCoord + crate::math::vector2::Coord>(a_vec: Vector2<T>) -> bool {
    a_vec.x.is_safe_xy() && a_vec.y.is_safe_xy()
}

/// Utility for the line clipping code, returns the boundary code of
/// a point. Bit allocation is arbitrary
fn clip_out_code(a_clip_box: &Box2I, x: i32, y: i32) -> i32 {
    let mut code: i32;

    if y < a_clip_box.y() {
        code = 2;
    } else if y > a_clip_box.bottom() {
        code = 1;
    } else {
        code = 0;
    }

    if x < a_clip_box.x() {
        code |= 4;
    } else if x > a_clip_box.right() {
        code |= 8;
    }

    code
}

/// Port of `ClipLine()`: test if any part of a line falls within the bounds of a rectangle,
/// clipping the line to it (in place, as in C++).
///
/// Please note that this is only accurate for lines that are one pixel wide.
/// Returns false if any part of the line lies within the rectangle.
pub fn clip_line(a_clip_box: &Box2I, x1: &mut i32, y1: &mut i32, x2: &mut i32, y2: &mut i32) -> bool {
    // Stock Cohen-Sutherland algorithm; check *any* CG book for details
    let mut outcode1 = clip_out_code(a_clip_box, *x1, *y1);
    let mut outcode2 = clip_out_code(a_clip_box, *x2, *y2);

    while outcode1 != 0 || outcode2 != 0 {
        // Fast reject
        if outcode1 & outcode2 != 0 {
            return true;
        }

        // Choose a side to clip
        let thisoutcode = if outcode1 != 0 { outcode1 } else { outcode2 };
        let x: i32;
        let y: i32;

        // One clip round
        // Since we use the full range of 32 bit ints, the proportion
        // computation has to be done in 64 bits to avoid horrible
        // results
        if thisoutcode & 1 != 0 {
            // Clip the bottom
            y = a_clip_box.bottom();
            x = (*x1 as i64 + (*x2 - *x1) as i64 * (y - *y1) as i64 / (*y2 - *y1) as i64) as i32;
        } else if thisoutcode & 2 != 0 {
            // Clip the top
            y = a_clip_box.y();
            x = (*x1 as i64 + (*x2 - *x1) as i64 * (y - *y1) as i64 / (*y2 - *y1) as i64) as i32;
        } else if thisoutcode & 8 != 0 {
            // Clip the right
            x = a_clip_box.right();
            y = (*y1 as i64 + (*y2 - *y1) as i64 * (x - *x1) as i64 / (*x2 - *x1) as i64) as i32;
        } else {
            // if( thisoutcode & 4), obviously, clip the left
            x = a_clip_box.x();
            y = (*y1 as i64 + (*y2 - *y1) as i64 * (x - *x1) as i64 / (*x2 - *x1) as i64) as i32;
        }

        // Put the result back and update the boundary code
        // No ambiguity, otherwise it would have been a fast reject
        if thisoutcode == outcode1 {
            *x1 = x;
            *y1 = y;
            outcode1 = clip_out_code(a_clip_box, *x1, *y1);
        } else {
            *x2 = x;
            *y2 = y;
            outcode2 = clip_out_code(a_clip_box, *x2, *y2);
        }
    }

    false
}

/// Port of `KIGEOM::BoxHitTest( const VECTOR2I&, const BOX2I&, int )`: point-to-box hit test.
pub fn box_hit_test_point(a_hit_point: Vector2I, a_hittee: &Box2I, a_accuracy: i32) -> bool {
    let hittee = a_hittee.inflated(a_accuracy);
    hittee.contains(a_hit_point)
}

/// Port of `KIGEOM::BoxHitTest( const BOX2I&, const BOX2I&, bool, int )`: box-to-box hit test.
pub fn box_hit_test_box(a_hitter: &Box2I, a_hittee: &Box2I, a_hittee_contained: bool, a_accuracy: i32) -> bool {
    let hitter = a_hitter.inflated(a_accuracy);

    if a_hittee_contained {
        return hitter.contains_box(a_hittee);
    }

    hitter.intersects(a_hittee)
}

/// Port of `KIGEOM::BoxHitTest( const SHAPE_LINE_CHAIN&, const BOX2I&, bool )`: shape-to-box
/// hit test.
pub fn box_hit_test_shape(a_hitter: &ShapeLineChain, a_hittee: &Box2I, a_hittee_contained: bool) -> bool {
    let bbox = ShapeRect::from_box(a_hittee);

    shape_hit_test(a_hitter, &bbox, a_hittee_contained)
}

/// Port of `KIGEOM::BoxHitTest( const SHAPE_LINE_CHAIN&, const BOX2I&, const EDA_ANGLE&,
/// const VECTOR2I&, bool )`: shape-to-box hit test with a rotated box.
pub fn box_hit_test_shape_rotated(
    a_hitter: &ShapeLineChain,
    a_hittee: &Box2I,
    a_hittee_rotation: EdaAngle,
    a_hittee_rotation_center: Vector2I,
    a_hittee_contained: bool,
) -> bool {
    // Optimization: use SHAPE_RECT collision test if possible
    if a_hittee_rotation.is_zero() {
        return box_hit_test_shape(a_hitter, a_hittee, a_hittee_contained);
    } else if a_hittee_rotation.is_cardinal() {
        let bx = a_hittee.bounding_box_rotated(a_hittee_rotation_center, a_hittee_rotation);
        return box_hit_test_shape(a_hitter, &bx, a_hittee_contained);
    }

    // Non-cardinal angle: convert to simple polygon and rotate
    let corners: Vec<Vector2I> = vec![
        a_hittee.origin(),
        Vector2I::new(a_hittee.right(), a_hittee.top()),
        a_hittee.end(),
        Vector2I::new(a_hittee.left(), a_hittee.bottom()),
    ];

    let mut shape = ShapeSimple::from_line_chain(&ShapeLineChain::from_points(&corners, false));
    shape.rotate(a_hittee_rotation, a_hittee_rotation_center);

    shape_hit_test(a_hitter, &shape, a_hittee_contained)
}

/// Port of `KIGEOM::ShapeHitTest()`: shape-to-shape hit test.
///
/// `a_hitter` is the selection shape that is either hitting or containing the hittee,
/// `a_hittee_contained` is true if the hittee is tested for total containment, false if it is
/// tested for intersection.
pub fn shape_hit_test(a_hitter: &ShapeLineChain, a_hittee: &dyn Shape, a_hittee_contained: bool) -> bool {
    // Check if the selection polygon collides with any of the hittee's subshapes.
    let collides_any = || a_hittee.collide(a_hitter, 0, None, None);

    // Check if the selection polygon collides with all of the hittee's subshapes.
    let collides_all = || {
        if let ShapeRef::Compound(compound_hittee) = a_hittee.as_shape_ref() {
            // If the hittee is a compound shape, all subshapes must collide.
            compound_hittee
                .shapes()
                .iter()
                .all(|subshape| subshape.collide(a_hitter, 0, None, None))
        } else {
            // If the hittee is a simple shape, we can check it directly.
            a_hittee.collide(a_hitter, 0, None, None)
        }
    };

    // Check if the selection polygon outline collides with the hittee's shape.
    let intersects_any = || {
        let count = a_hitter.segment_count();

        for i in 0..count {
            if a_hittee.collide_seg(&a_hitter.c_segment(i), 0, None, None) {
                return true;
            }
        }

        false
    };

    if a_hitter.is_closed() {
        if a_hittee_contained {
            // Containing polygon - all of the subshapes must collide with the selection polygon,
            // but none of them can intersect its outline.
            collides_all() && !intersects_any()
        } else {
            // Touching polygon - any of the subshapes should collide with the selection polygon.
            collides_any()
        }
    } else {
        // Touching (poly)line - any of the subshapes should intersect the selection polyline.
        intersects_any()
    }
}
