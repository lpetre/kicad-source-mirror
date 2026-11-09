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

//! Port of `qa/tests/libs/kimath/math/test_vector2.cpp` (Boost test suite `VECTOR2TESTS`).

use kicad_geom::math::vector2::{
    Vector2, Vector2D, Vector2I, Vector2L, lexicographical_compare, lexicographical_min,
};

/// Port of `BOOST_AUTO_TEST_CASE( Constexpr )`.
///
/// The C++ test checks most of this with `static_assert`; the port checks at run time
/// (`Vector2::new` is a `const fn`, the arithmetic goes through traits and is not).
#[test]
fn constexpr() {
    const VI_1_2: Vector2I = Vector2I::new(1, 2);
    // Something to test at runtime
    assert!(VI_1_2 == Vector2I::new(1, 2));
    const _: () = assert!(VI_1_2.x == 1);
    const _: () = assert!(VI_1_2.y == 2);

    let vi_3_5: Vector2I = VI_1_2 + Vector2I::new(3, 4) - Vector2I::new(1, 1);

    assert!(vi_3_5 == Vector2I::new(3, 5));

    assert!(vi_3_5.squared_euclidean_norm() == 9 + 25);
    assert!(vi_3_5 > VI_1_2);
    assert!(VI_1_2 < vi_3_5);
    assert!(VI_1_2 != vi_3_5);
    #[allow(clippy::eq_op)]
    {
        assert!(VI_1_2 <= VI_1_2);
        assert!(VI_1_2 >= VI_1_2);
    }

    assert!(VI_1_2.squared_distance(vi_3_5) == 4 + 9);

    assert!(lexicographical_compare(&VI_1_2, &vi_3_5) < 0);
    // Was the copy avoided?
    assert!(core::ptr::eq(
        lexicographical_min(&VI_1_2, &vi_3_5),
        &VI_1_2
    ));
    assert!(core::ptr::eq(
        lexicographical_min(&VI_1_2, &vi_3_5),
        &VI_1_2
    ));

    const VD_1_2: Vector2D = Vector2D::new(1.0, 2.0);
    const VD_3_4: Vector2D = Vector2D::new(3.0, 4.0);

    assert!(VD_1_2 == Vector2D::new(1.0, 2.0));
    assert!(VD_1_2 != VD_3_4);
    assert!(VD_1_2 < VD_3_4);
    assert!(VD_1_2 <= VD_3_4);
    assert!(VD_3_4 > VD_1_2);

    // After C++23
    //static_assert( equals( vd_1_2, vd_3_4, 2.0 ) );
}

/// Port of `BOOST_AUTO_TEST_CASE( test_cross_product )`.
#[test]
fn test_cross_product() {
    let v1 = Vector2I::new(0, 1);
    let v2 = Vector2I::new(1, 0);

    assert!(v2.cross(v1) == 1);
}

/// Port of `BOOST_AUTO_TEST_CASE( test_dot_product )`.
#[test]
fn test_dot_product() {
    let v1 = Vector2I::new(0, 1);
    let v2 = Vector2I::new(1, 0);

    assert!(v2.dot(v1) == 0);
}

/// Port of `BOOST_AUTO_TEST_CASE( test_euclidean_norm_exact )`.
#[test]
fn test_euclidean_norm_exact() {
    let imin: i32 = i32::MIN;
    let imax: i32 = i32::MAX;

    // std::hypot rounds this one to 95556683
    assert_eq!(Vector2I::new(175139, 95556522).euclidean_norm(), 95556682);

    assert_eq!(Vector2I::new(imin, 0).euclidean_norm(), imax);
    assert_eq!(Vector2I::new(imin, imin).euclidean_norm(), imax);
    assert_eq!(Vector2I::new(imin, imin).squared_euclidean_norm(), i64::MAX);
}

/// Port of `BOOST_AUTO_TEST_CASE( test_euclidean_norm_int64_exact )`.
#[test]
fn test_euclidean_norm_int64_exact() {
    let lmin: i64 = i64::MIN;
    let lmax: i64 = i64::MAX;

    // std::hypot rounds this to a multiple of the double spacing, 1414213562373095168
    assert_eq!(
        Vector2L::new(1000000000000000003, 1000000000000000007).euclidean_norm(),
        1414213562373095056
    );

    assert_eq!(Vector2L::new(lmin, 0).euclidean_norm(), lmax);
    assert_eq!(Vector2L::new(lmin, 0).squared_euclidean_norm(), lmax);
}

/// Port of `BOOST_AUTO_TEST_CASE( test_resize )`.
#[test]
fn test_resize() {
    // just some arbitrary vectors
    let v1 = Vector2I::new(4, 3);
    let v2 = Vector2I::new(5, -1);
    let v3 = Vector2I::new(-2, 1);
    let v4 = Vector2I::new(1, 1);
    let v5 = Vector2I::new(-70, -70);

    assert!(v1.resize(8) == Vector2I::new(6, 5));
    assert!(v2.resize(10) == Vector2I::new(10, -2));
    assert!(v3.resize(4) == Vector2I::new(-4, 2));
    assert!(v4.resize(1) == Vector2I::new(1, 1));
    assert!(v5.resize(100) == Vector2I::new(-71, -71));
}

/// Port of `BOOST_AUTO_TEST_CASE( test_casting )`.
///
/// `VECTOR2<T>( other )` (the converting constructor) is `Vector2::<T>::from_vec( other )`;
/// the implicit arithmetic conversions of the mixed-type operators are implemented per
/// operand type pair, see `math/vector2.rs`.
#[test]
#[allow(clippy::assign_op_pattern)] // `v = v - 1` is what the C++ test does
fn test_casting() {
    let mut vint = Vector2I::new(4, 3);
    let mut vdouble = Vector2D::new(4.0, 3.0);
    let mut vlong = Vector2L::new(4, 3);
    let mut vfloat = Vector2::<f32>::new(4.0, 3.0);
    let mut vunsigned = Vector2::<u32>::new(4, 3);

    assert_eq!(vint, Vector2I::from_vec(vdouble));
    assert_eq!(vint, Vector2I::from_vec(vlong));
    assert_eq!(vint, Vector2I::from_vec(vfloat));
    assert_eq!(vint, Vector2I::from_vec(vunsigned));

    assert_eq!(vdouble, Vector2D::from_vec(vint));
    assert_eq!(vdouble, Vector2D::from_vec(vlong));
    assert_eq!(vdouble, Vector2D::from_vec(vfloat));
    assert_eq!(vdouble, Vector2D::from_vec(vunsigned));

    assert_eq!(vlong, Vector2L::from_vec(vint));
    assert_eq!(vlong, Vector2L::from_vec(vdouble));
    assert_eq!(vlong, Vector2L::from_vec(vfloat));
    assert_eq!(vlong, Vector2L::from_vec(vunsigned));

    assert_eq!(vfloat, Vector2::<f32>::from_vec(vint));
    assert_eq!(vfloat, Vector2::<f32>::from_vec(vdouble));
    assert_eq!(vfloat, Vector2::<f32>::from_vec(vlong));
    assert_eq!(vfloat, Vector2::<f32>::from_vec(vunsigned));

    assert_eq!(vunsigned, Vector2::<u32>::from_vec(vint));
    assert_eq!(vunsigned, Vector2::<u32>::from_vec(vdouble));
    assert_eq!(vunsigned, Vector2::<u32>::from_vec(vlong));
    assert_eq!(vunsigned, Vector2::<u32>::from_vec(vfloat));

    // Check that negative values are handled correctly
    vint = vint - 1;
    vdouble = vdouble - 1;
    vlong = vlong - 1;
    vfloat = vfloat - 1;
    vunsigned = vunsigned - 1;

    assert_eq!(vint, Vector2I::new(3, 2));
    assert_eq!(vdouble, Vector2D::new(3.0, 2.0));
    assert_eq!(vlong, Vector2L::new(3, 2));
    assert_eq!(vfloat, Vector2::<f32>::new(3.0, 2.0));
    assert_eq!(vunsigned, Vector2::<u32>::new(3, 2));

    // Check that subtracting unsigned values works correctly
    vint = vint - 1u32;
    vdouble = vdouble - 1u32;
    vlong = vlong - 1u32;
    vfloat = vfloat - 1u32;
    vunsigned = vunsigned - 1u32;

    assert_eq!(vint, Vector2I::new(2, 1));
    assert_eq!(vdouble, Vector2D::new(2.0, 1.0));
    assert_eq!(vlong, Vector2L::new(2, 1));
    assert_eq!(vfloat, Vector2::<f32>::new(2.0, 1.0));
    assert_eq!(vunsigned, Vector2::<u32>::new(2, 1));

    vint = vint - 5.0;
    vdouble = vdouble - 5.0;
    vlong = vlong - 5.0;
    vfloat = vfloat - 5.0;
    vunsigned = vunsigned - 5.0;

    assert_eq!(vint, Vector2I::new(-3, -4));
    assert_eq!(vdouble, Vector2D::new(-3.0, -4.0));
    assert_eq!(vlong, Vector2L::new(-3, -4));
    assert_eq!(vfloat, Vector2::<f32>::new(-3.0, -4.0));
    assert_eq!(vunsigned, Vector2::<u32>::new(4294967293, 4294967292)); // roll over unsigned when explicitly subtracting.

    // Check that negative initial values are handled correctly
    vint = Vector2I::new(-4, -3);
    vdouble = Vector2D::new(-4.0, -3.0);
    vlong = Vector2L::new(-4, -3);
    vfloat = Vector2::<f32>::new(-4.0, -3.0);
    vunsigned = Vector2::<u32>::new(-4i32 as u32, -3i32 as u32);

    vint = vint - 1;
    vdouble = vdouble - 1;
    vlong = vlong - 1;
    vfloat = vfloat - 1;
    vunsigned = vunsigned - 1;

    assert_eq!(vint, Vector2I::new(-5, -4));
    assert_eq!(vdouble, Vector2D::new(-5.0, -4.0));
    assert_eq!(vlong, Vector2L::new(-5, -4));
    assert_eq!(vfloat, Vector2::<f32>::new(-5.0, -4.0));
    assert_eq!(vunsigned, Vector2::<u32>::new(4294967291, 4294967292));

    vint = vint - 1u32;
    vdouble = vdouble - 1u32;
    vlong = vlong - 1u32;
    vfloat = vfloat - 1u32;

    assert_eq!(vint, Vector2I::new(-6, -5));
    assert_eq!(vdouble, Vector2D::new(-6.0, -5.0));
    assert_eq!(vlong, Vector2L::new(-6, -5));
    assert_eq!(vfloat, Vector2::<f32>::new(-6.0, -5.0));

    let add = vint + vdouble;
    assert_eq!(add, Vector2D::new(-12.0, -10.0));

    // (the C++ literal 2 is an int)
    let sub = vint - 2i32 * vlong;
    assert_eq!(sub.x, 6);
    assert_eq!(sub.y, 5);

    vunsigned = Vector2::<u32>::new(u32::MAX, u32::MAX);
    vint = Vector2I::from_vec(vunsigned);
    assert_eq!(vint.x, i32::MAX);

    vunsigned += 1;
    assert_eq!(vunsigned.x, 0);
}
