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

//! PROP-1: the property tables (`kicad_pcb::properties`, port of `PROPERTY_MANAGER` and the
//! `*_DESC` registrations) against the reference build, through the reference dump tool
//! (`rust/tools/pcb-dump/dump_props.cpp`). Goldens: `tests/golden/props_table.txt` (`pcb-dump
//! props-table --hash`) and `tests/golden/props_values.txt` (`pcb-dump board --hash --sections
//! props` on copies of the boards in `tests/oracle/props_boards.txt`), both made by
//! `tests/oracle/props_golden.sh`.
//!
//! The static table must match exactly. The per-item values must have no mismatch and no
//! rust-error; `missing` lines (items the Rust loader does not build yet) and `pending` lines
//! (`port_pending!` stubs, getters without a Rust API) are tolerated and counted.

#![cfg(feature = "dump")]

use kicad_pcb::dump::compare::{Kind, compare};
use kicad_pcb::dump::{
    DumpOptions, dump_board, dump_props_table, load_board_for_dump, scan_file_uuids,
};

#[test]
fn props_table_matches_cxx_golden() {
    let golden = include_str!("golden/props_table.txt");
    let rust = dump_props_table(true);
    let c = compare(golden, &rust);
    println!("{}", c.report(30));
    assert!(c.total(Kind::Match) > 1000);
    assert_eq!(c.failures(), 0, "props table differs:\n{}", c.report(30));
    assert_eq!(c.total(Kind::Pending), 0);
}

/// The golden split into (file name, lines) per board.
fn golden_boards() -> Vec<(String, String)> {
    let mut boards: Vec<(String, String)> = Vec::new();

    for line in include_str!("golden/props_values.txt").lines() {
        if let Some(name) = line.strip_prefix("file|") {
            boards.push((name.trim_end_matches('|').to_string(), String::new()));
        } else if let Some((_, text)) = boards.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }

    boards
}

#[test]
fn props_values_match_cxx_golden() {
    let list = include_str!("oracle/props_boards.txt");
    let golden = golden_boards();
    let mut matched = 0;

    for (rel, (name, reference)) in list.lines().filter(|l| !l.is_empty()).zip(&golden) {
        let path = kicad_qa::pcbnew_data_dir().join(rel);
        assert!(path.ends_with(name), "{rel} vs {name}");

        let board = load_board_for_dump(&path).expect("board loads");
        let mut opts = DumpOptions {
            hash: true,
            ..DumpOptions::default()
        };
        opts.sections.insert("props".to_string());
        opts.file_uuids = Some(scan_file_uuids(&std::fs::read(&path).expect("read")));
        let rust = dump_board(&board, &opts);

        let c = compare(reference, &rust);
        println!("{name}:\n{}", c.report(5));
        matched += c.total(Kind::Match);

        for kind in [Kind::Mismatch, Kind::RustError, Kind::Extra] {
            assert_eq!(
                c.total(kind),
                0,
                "{name}: {}\n{}",
                kind.name(),
                c.report(20)
            );
        }
    }

    assert_eq!(golden.len(), list.lines().filter(|l| !l.is_empty()).count());
    assert!(matched > 1500, "only {matched} matching values");
}
