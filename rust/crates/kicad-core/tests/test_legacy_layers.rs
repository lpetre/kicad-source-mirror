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

//! Differential test of `kicad_core::legacy_layers` against the C++ implementation.
//!
//! `tests/oracle/legacy_oracle.cpp` includes the original source text of
//! `remapLegacyLayerLSET` (`pcbnew/pcb_plot_params.cpp`) and of `leg_layer2new` /
//! `leg_mask2new` (`pcbnew/pcb_io/kicad_legacy/pcb_io_kicad_legacy.cpp`) and prints
//! `tests/golden/legacy_layers_dump.txt`; this test regenerates the dump with the Rust port.
//! [`dump`] mirrors `main()` of the C++ program; keep the two in sync.

use std::fmt::Write as _;

use kicad_core::base_set::BaseSet;
use kicad_core::legacy_layers::*;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn dump() -> Vec<String> {
    let mut out = Vec::new();

    out.push(format!("COUNT {LEGACY_PCB_LAYER_ID_COUNT}"));

    // remapLegacyLayerLSET: single bits, then random sets
    for bit in 0..LEGACY_PCB_LAYER_ID_COUNT {
        let mut legacy = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);
        legacy.set(bit);
        out.push(format!(
            "REMAP1 {} {}",
            bit,
            remap_legacy_layer_lset(&legacy).fmt_hex()
        ));
    }

    let mut rng = Lcg(59);

    for k in 0..24u64 {
        let mut legacy = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);

        for bit in 0..LEGACY_PCB_LAYER_ID_COUNT {
            if rng.next() % 4 < (k % 4) + 1 {
                legacy.set(bit);
            }
        }

        let remapped = remap_legacy_layer_lset(&legacy);
        out.push(format!(
            "REMAP {} {} {}",
            legacy.fmt_hex(),
            remapped.size(),
            remapped.fmt_hex()
        ));
    }

    for hex in [
        "00010fc_ffffffff",
        "0000000_00000001",
        "0000000_80000000",
        "fffffff_ffffffff",
        "0001000_00000000",
        "00000a8_80000001",
    ] {
        let mut legacy = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);
        let consumed = legacy.parse_hex(hex);
        out.push(format!(
            "REMAPHEX {} {} {}",
            hex,
            consumed,
            remap_legacy_layer_lset(&legacy).fmt_hex()
        ));
    }

    // leg_layer2new / is_leg_copperlayer_valid
    for cu in [1, 2, 4, 6, 8, 16] {
        let mut row = String::new();
        let mut valid = String::new();

        for layer in -2..=34 {
            write!(row, " {}", leg_layer2new(cu, layer).0).unwrap();
            valid.push(if is_leg_copperlayer_valid(cu, layer) {
                '1'
            } else {
                '0'
            });
        }

        out.push(format!("L2N {cu}{row} valid={valid}"));
    }

    // leg_mask2new
    for cu in [2, 4, 16] {
        for mask in [
            0u32, 1, 0x8000, 0x8001, 0xFFFF, 0x00C0FFFF, 0x00888000, 0x00440001, 0x10000000,
            0x1FFF0000, 0x1FFFFFFF, 0xFFFFFFFF, 0x20000000, 0x0000FFFE, 0x00000006, 0x00007FFE,
        ] {
            let set = leg_mask2new(cu, mask);
            out.push(format!(
                "M2N {} {:08x} {} {}",
                cu,
                mask,
                set.size(),
                set.fmt_hex()
            ));
        }
    }

    out
}

#[test]
fn rust_port_matches_cpp_dump() {
    let golden = include_str!("golden/legacy_layers_dump.txt");
    let expected: Vec<&str> = golden.lines().collect();
    let actual = dump();

    for (i, line) in actual.iter().enumerate() {
        assert_eq!(
            Some(line.as_str()),
            expected.get(i).copied(),
            "dump line {}",
            i + 1
        );
    }

    assert_eq!(actual.len(), expected.len(), "number of dump lines");
}
