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

//! `SHAPE_LINE_CHAIN::PointInside` with a point at `INT_MIN` (a NaN converted by the
//! reference's `cvttsd2si`): the `int` subtractions wrap as on x86-64 instead of panicking
//! (found by the creepage path validation, work package DRC-33).

use kicad_geom::{Shape, ShapeLineChain, Vector2I};

#[test]
fn point_inside_int_min_point() {
    let chain = ShapeLineChain::from_points(
        &[
            Vector2I::new(0, 0),
            Vector2I::new(1_000_000, 0),
            Vector2I::new(1_000_000, 1_000_000),
            Vector2I::new(0, 1_000_000),
        ],
        true,
    );

    let p = Vector2I::new(i32::MIN, i32::MIN);
    assert!(!chain.point_inside(p, 0, false));
}
