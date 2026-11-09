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

//! Port of `qa/tests/common/test_format_units.cpp`, run for each of the three scales the C++
//! test is compiled for (EESCHEMA, GERBVIEW, PCBNEW).

use kicad_core::base_units::{EdaIuScale, GERB_IU_SCALE, PCB_IU_SCALE, SCH_IU_SCALE};
use kicad_core::eda_units::format_internal_units_point as fmt;

fn check(iu_scale: &EdaIuScale, expected: [&str; 4]) {
    let s = fmt(iu_scale, 123456, 52525252);
    let s_zero = fmt(iu_scale, 0, 0);
    let s_neg = fmt(iu_scale, -123456, -52525252);
    let s_odd_neg = fmt(iu_scale, -350000, 0);
    let s_max = fmt(iu_scale, i32::MIN, i32::MAX);

    assert_eq!(s_zero, "0 0");
    assert_eq!(s, expected[0]);
    assert_eq!(s_neg, expected[1]);
    assert_eq!(s_max, expected[2]);
    assert_eq!(s_odd_neg, expected[3]);
}

#[test]
fn vector2i_unit_format() {
    check(
        &SCH_IU_SCALE,
        [
            "12.3456 5252.5252",
            "-12.3456 -5252.5252",
            "-214748.3648 214748.3647",
            "-35 0",
        ],
    );
    check(
        &GERB_IU_SCALE,
        [
            "1.23456 525.25252",
            "-1.23456 -525.25252",
            "-21474.83648 21474.83647",
            "-3.5 0",
        ],
    );
    check(
        &PCB_IU_SCALE,
        [
            "0.123456 52.525252",
            "-0.123456 -52.525252",
            "-2147.483648 2147.483647",
            "-0.35 0",
        ],
    );
}
