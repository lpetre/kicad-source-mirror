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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `qa/tests/libs/kimath/math/test_vector3.cpp` (suite `VECTOR3TESTS`).

use kicad_geom::math::vector3::{Vector3, Vector3D, Vector3I};

#[test]
fn test_cross_product() {
    let v1 = Vector3I::new(0, 1, 2);
    let v2 = Vector3I::new(2, 1, 0);

    assert_eq!(v1.cross(&v2), Vector3I::new(-2, 4, -2));
}

#[test]
fn test_dot_product() {
    let v1 = Vector3I::new(0, 1, 2);
    let v2 = Vector3I::new(2, 1, 0);

    assert_eq!(v1.dot(&v2), 1);
}

#[test]
fn test_equality_ops() {
    let v1 = Vector3I::new(1, 1, 1);
    let v2 = Vector3I::new(2, 2, 2);
    let v3 = Vector3I::new(1, 1, 1);

    assert!(v1 == v3);
    assert!(v1 != v2);
}

#[test]
fn test_scalar_multiply() {
    let mut v1 = Vector3I::new(1, 1, 1);
    v1 *= 5;

    assert_eq!(v1, Vector3::new(5, 5, 5));
}

#[test]
fn test_scalar_divide() {
    let mut v1 = Vector3I::new(5, 5, 5);
    v1 /= 5;

    assert_eq!(v1, Vector3::new(1, 1, 1));
}

// Additional checks (not in the C++ suite)

#[test]
fn test_norm_normalize_stream() {
    let mut v = Vector3D::new(3.0, 0.0, 4.0);
    assert_eq!(v.euclidean_norm(), 5.0);
    assert_eq!(v.normalize(), Vector3D::new(0.6, 0.0, 0.8));
    assert_eq!(v.to_string(), "[ 0.6 | 0 | 0.8 ]");

    // Integer normalisation truncates; a zero vector is left unchanged (wxCHECK)
    let mut vi = Vector3I::new(3, 0, 4);
    assert_eq!(vi.euclidean_norm(), 5);
    assert_eq!(vi.normalize(), Vector3I::new(0, 0, 0));
    let mut z = Vector3I::new(0, 0, 0);
    assert_eq!(z.normalize(), Vector3I::new(0, 0, 0));
    assert_eq!(Vector3I::new(1, -2, 3).to_string(), "[ 1 | -2 | 3 ]");
}
