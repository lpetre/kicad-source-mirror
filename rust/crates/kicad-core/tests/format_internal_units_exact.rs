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

//! Exactness of the integer fast path of `FormatInternalUnits` for a 1e6 divisor
//! (`push_iu_1e6_exact`) against the statement-by-statement port of the C++ double formatting
//! (`format_internal_units_generic`).  The default test covers dense ranges; the ignored test
//! covers every `i32` (run it in release: `cargo test --release -p kicad-core --test
//! format_internal_units_exact -- --ignored`).

use kicad_core::base_units::{EdaIuScale, PCB_IU_SCALE};
use kicad_core::eda_units::{
    format_internal_units, format_internal_units_generic, format_internal_units_point,
    push_iu_1e6_exact, EdaDataType,
};

fn check(value: i32) {
    let mut fast = String::new();
    push_iu_1e6_exact(&mut fast, value);
    let generic = format_internal_units_generic(&PCB_IU_SCALE, value, EdaDataType::Distance);
    assert_eq!(fast, generic, "value {value}");
}

#[test]
fn dense_ranges_match_generic() {
    for v in -2_000_000..=2_000_000 {
        check(v);
    }

    // Around every power of ten (and its multiples by 2 and 5), both signs.
    let mut p: i64 = 1;

    while p <= i32::MAX as i64 {
        for base in [p, 2 * p, 5 * p] {
            for d in -1000..=1000i64 {
                for s in [1i64, -1] {
                    let v = s * (base + d);

                    if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
                        check(v as i32);
                    }
                }
            }
        }

        p *= 10;
    }

    // Both ends of i32.
    for d in 0..100_000 {
        check(i32::MIN + d);
        check(i32::MAX - d);
    }

    // A stride sample over the whole range (odd stride so all residues mod 10^6 show up).
    let mut v = i32::MIN as i64;

    while v <= i32::MAX as i64 {
        check(v as i32);
        v += 1_000_003;
    }
}

#[test]
fn known_values() {
    let cases: &[(i32, &str)] = &[
        (0, "0"),
        (1, "0.000001"),
        (-1, "-0.000001"),
        (100, "0.0001"),
        (101, "0.000101"),
        (1_000_000, "1"),
        (-1_500_000, "-1.5"),
        (1_234_567, "1.234567"),
        (i32::MAX, "2147.483647"),
        (i32::MIN, "-2147.483648"),
    ];

    for &(v, text) in cases {
        let mut s = String::new();
        push_iu_1e6_exact(&mut s, v);
        assert_eq!(s, text);
        assert_eq!(format_internal_units(&PCB_IU_SCALE, v, EdaDataType::Distance), text);
    }

    assert_eq!(format_internal_units_point(&PCB_IU_SCALE, -1, 2_500_000), "-0.000001 2.5");
}

#[test]
fn other_scales_use_generic_path() {
    let scales = [PCB_IU_SCALE, EdaIuScale::new(1e4), EdaIuScale::new(1e3), EdaIuScale::new(1e5)];
    let types = [
        EdaDataType::Distance,
        EdaDataType::Area,
        EdaDataType::Volume,
        EdaDataType::Unitless,
        EdaDataType::Time,
        EdaDataType::LengthDelay,
        EdaDataType::Angle,
    ];

    for scale in &scales {
        for &t in &types {
            for v in [-123_456_789, -100, -1, 0, 1, 7, 100, 1_000_001, i32::MAX, i32::MIN] {
                assert_eq!(
                    format_internal_units(scale, v, t),
                    format_internal_units_generic(scale, v, t),
                    "value {v} type {t:?}"
                );
            }
        }

        for (x, y) in [(0, 0), (-1, 1), (123_456, -7_000_000), (i32::MIN, i32::MAX)] {
            let generic = format_internal_units_generic(scale, x, EdaDataType::Distance)
                + " "
                + &format_internal_units_generic(scale, y, EdaDataType::Distance);
            assert_eq!(format_internal_units_point(scale, x, y), generic);
        }
    }
}

/// Every i32 value (2^32 comparisons; minutes in release, far longer in dev).
#[test]
#[ignore]
fn every_i32_matches_generic() {
    let mut fast = String::with_capacity(16);
    let mut mismatches = 0u64;

    for v in i32::MIN..=i32::MAX {
        fast.clear();
        push_iu_1e6_exact(&mut fast, v);

        if fast != format_internal_units_generic(&PCB_IU_SCALE, v, EdaDataType::Distance) {
            mismatches += 1;

            if mismatches < 10 {
                eprintln!("mismatch at {v}: {fast}");
            }
        }
    }

    assert_eq!(mismatches, 0);
}
