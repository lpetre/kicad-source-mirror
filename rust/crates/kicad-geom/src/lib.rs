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

//! Port of KiCad's kimath: integer 2D math, shapes, polygon sets, collisions
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and this crate's
//! `PORT_STATUS.md`.
//!
//! The module tree mirrors `libs/kimath`:
//!
//! | Rust module | C++ |
//! |---|---|
//! | [`math::util`] | `math/util.h`, `math/util.cpp` |
//! | [`math::wide_int`] | `math/wide_int.h` |
//! | [`math::vector2`] | `math/vector2d.h` |
//! | [`math::box2`] | `math/box2.h` |
//! | [`math::box2_minmax`] | `math/box2_minmax.h` |
//! | [`geometry::eda_angle`] | `geometry/eda_angle.h`, `geometry/eda_angle.cpp` |
//! | [`geometry::seg`] | `geometry/seg.h`, `geometry/seg.cpp` |
//! | [`geometry::vector_utils`] | `geometry/vector_utils.h`, `geometry/vector_utils.cpp` |
//! | [`trigo`] | `trigo.h`, `trigo.cpp` |
//! | [`kicore::mirror`] | `include/core/mirror.h` |
//! | [`cmath`] | (support) the libm functions the C++ code calls, in one place |
//! | [`ostream`] | (support) default `std::ostream` number formatting |
//! | [`test_utils`] | (support) ports of the shared C++ QA helpers |
//!
//! The most used items are re-exported at the crate root.

pub mod cmath;
pub mod geometry;
pub mod kicore;
pub mod math;
pub mod ostream;
pub mod test_utils;
pub mod trigo;

pub use crate::geometry::eda_angle::{
    ANGLE_0, ANGLE_45, ANGLE_90, ANGLE_135, ANGLE_180, ANGLE_270, ANGLE_360, ANGLE_HORIZONTAL,
    ANGLE_VERTICAL, DEGREES_T, EdaAngle, EdaAngleT, EdaOrientation, FULL_CIRCLE, RADIANS_T,
    TENTHS_OF_A_DEGREE_T,
};
pub use crate::geometry::seg::{Ecoord, OptVector2I, Seg};
pub use crate::kicore::mirror::{FlipDirection, mirror, mirror_axis, mirror_point, mirror_val};
pub use crate::math::box2::{
    Box2, Box2D, Box2I, Box2L, OptBox2I, box2i_safe, box2i_safe_from_int, box2i_safe_from_pos_size,
};
pub use crate::math::box2_minmax::Box2IMinMax;
pub use crate::math::util::{
    KiRound, equals, equals_eps, isqrt, ki_checked_cast, ki_round, ki_round_i64, ki_round_to,
    rescale, rounded_hypot, sign, std_clamp, std_max, std_min,
};
pub use crate::math::vector2::{
    Coord, SignedCoord, Vector2, Vector2D, Vector2I, Vector2L, equals_vec, equals_vec_eps,
    ki_round_xy, lexicographical_compare, lexicographical_max, lexicographical_min,
};
pub use crate::trigo::{
    Rotatable, calc_arc_center, calc_arc_center_d, calc_arc_center_from_angle, calc_arc_mid,
    deg2rad, get_rotated, get_rotated_around, intercepts_negative_x, intercepts_positive_x,
    is_point_on_segment, is_straight_arc, normalize_angle_pos, normalize_angle180, rad2decideg,
    rad2deg, rotate_point, rotate_point_around, segment_intersects_segment,
    segment_intersects_segment_with_point, test_segment_hit,
};
