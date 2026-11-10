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

//! DUMP-RS: the Rust `defaults` dump against the C++ golden
//! (`tests/golden/pcb_dump_defaults.txt`, made with `/home/user/kicad-oracle/bin/pcb-dump
//! defaults`). Fails on any mismatch / rust-error / missing / extra line; pending fields (stubs
//! not ported yet) are tolerated and counted. Run with `--features dump -- --nocapture` to
//! see the scoreboard.

#![cfg(feature = "dump")]

use kicad_pcb::dump::compare::{Kind, compare};
use kicad_pcb::dump::dump_defaults;

#[test]
fn defaults_match_cxx_golden() {
    let golden = include_str!("golden/pcb_dump_defaults.txt");
    let rust = dump_defaults(false);
    let c = compare(golden, &rust);
    println!("{}", c.report(30));
    assert!(c.total(Kind::Match) > 0);
    assert_eq!(
        c.failures(),
        0,
        "defaults dump differs from the C++ golden:\n{}",
        c.report(30)
    );
}

#[test]
fn defaults_hash_mode_hashes_values_only() {
    let plain = dump_defaults(false);
    let hashed = dump_defaults(true);
    assert_eq!(plain.lines().count(), hashed.lines().count());

    for (p, h) in plain.lines().zip(hashed.lines()) {
        if p.starts_with("pending|") || p.starts_with("rust-error|") {
            assert_eq!(p, h);
            continue;
        }

        let (pk, pv) = p.split_once('=').expect("value");
        let (hk, hv) = h.split_once('=').expect("value");
        assert_eq!(pk, hk);
        assert_eq!(
            hv,
            format!("#{:016x}", kicad_pcb::dump::fnv1a64(pv.as_bytes()))
        );
    }
}
