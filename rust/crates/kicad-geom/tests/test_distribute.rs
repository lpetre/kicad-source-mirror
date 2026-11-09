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

//! Port of `qa/tests/libs/kimath/geometry/test_distribute.cpp` (suite `Distribute`).

use kicad_geom::geometry::distribute::{
    get_deltas_for_distribute_by_gaps, get_deltas_for_distribute_by_points,
};

fn do_distribute_gaps_test_checks(a_extents: &[(i32, i32)], a_expected_deltas: &[i32]) {
    let deltas = get_deltas_for_distribute_by_gaps(a_extents);

    assert_eq!(deltas.len(), a_expected_deltas.len());

    // First and last items should not be moved
    assert_eq!(*deltas.first().unwrap(), 0);
    assert_eq!(*deltas.last().unwrap(), 0);
    assert_eq!(deltas, a_expected_deltas);
}

fn do_distribute_points_test_checks(a_points: &[i32], a_expected_deltas: &[i32]) {
    let deltas = get_deltas_for_distribute_by_points(a_points);

    assert_eq!(deltas.len(), a_expected_deltas.len());

    // First and last items should not be moved
    assert_eq!(*deltas.first().unwrap(), 0);
    assert_eq!(*deltas.last().unwrap(), 0);
    assert_eq!(deltas, a_expected_deltas);
}

#[test]
fn distribute_gaps_no_change_needed() {
    // Already evenly spaced (100 gaps)
    do_distribute_gaps_test_checks(&[(0, 100), (200, 300), (400, 500)], &[0, 0, 0]);
}

#[test]
fn distribute_gaps_simple_shift_needed() {
    // Need to move item 1 51 to the right
    do_distribute_gaps_test_checks(&[(0, 100), (149, 249), (400, 500)], &[0, 51, 0]);
}

#[test]
fn distribute_gaps_rounding() {
    // Have to fit 3 gaps into total sum of gaps of 100
    // so 33.333333 per gap
    // (note one rounds up, the other down)
    do_distribute_gaps_test_checks(
        &[
            (-100, 0),
            (0, 100), // Move this to 33 .. 133
            (0, 100), // Move this to 167 .. 267
            (300, 400),
        ],
        &[0, 33, 167, 0],
    );
}

#[test]
fn distribute_points_no_change_needed() {
    // Already evenly spaced (100 gaps)
    do_distribute_points_test_checks(&[0, 100, 200, 300, 400], &[0, 0, 0, 0, 0]);
}

#[test]
fn distribute_points_simple_shift_needed() {
    // Need to move item 1 51 to the right
    do_distribute_points_test_checks(&[0, 49, 200], &[0, 51, 0]);
}

#[test]
fn distribute_points_rounding() {
    // Have to fit 3 gaps into total sum of gaps of 100
    // so 33.333333 per gap
    // (note one rounds up, the other down)
    do_distribute_points_test_checks(&[0, 0, 0, 100], &[0, 33, 67, 0]);
}
