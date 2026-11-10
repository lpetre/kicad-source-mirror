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

//! Regression cases of package FIX-PAD-1 against the reference `pcb-dump`:
//!
//! * `PAD::SetLocalSolderMaskMargin` / `SetLocalSolderPasteMargin` /
//!   `SetLocalSolderPasteMarginRatio` write both outer sides (`padstack.back_outer.*`,
//!   `solder_mask_expansion.B.Mask`, `solder_paste_margin.B.Paste`, `shape.B.Mask` of pads
//!   with margin overrides in a footprint on the back side);
//! * `PAD::AddPrimitive`'s `SetParent( this )`: the custom pad's primitives reach the board
//!   through the pad, so their effective shapes (bezier, rounded rectangle) use the board's
//!   `maxError` (0.05 mm from the `.kicad_pro`), not the null-board `ARC_HIGH_DEF`.
//!
//! Golden: `tests/golden/fixpad1/back_margins_primitives.txt` (`tests/oracle/fixpad1_golden.sh`,
//! hashed, QA load path, sections item / pad / bbox).

#![cfg(feature = "dump")]

use std::path::{Path, PathBuf};

use kicad_pcb::dump::compare::{Kind, compare};
use kicad_pcb::dump::{DumpOptions, dump_board, load_board_for_dump, scan_file_uuids};

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// `own_clearance.*` comes from the DRC engine the C++ QA load path builds (a session step the
/// Rust dump loader does not run, `PCBIO-15`); every other line must match.
fn without_rule_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !l.contains("|own_clearance."))
        .map(|l| format!("{l}\n"))
        .collect()
}

#[test]
fn back_side_margins_and_primitive_parents_match_reference() {
    let path = crate_dir().join("tests/data/fixpad1/back_margins_primitives.kicad_pcb");
    let golden = std::fs::read_to_string(
        crate_dir().join("tests/golden/fixpad1/back_margins_primitives.txt"),
    )
    .unwrap();

    let board = load_board_for_dump(&path).unwrap();
    let opts = DumpOptions {
        sections: ["item", "pad", "bbox"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        hash: true,
        file_uuids: Some(scan_file_uuids(&std::fs::read(&path).unwrap())),
    };
    let rust = format!(
        "file|back_margins_primitives.kicad_pcb|\n{}",
        dump_board(&board, &opts)
    );

    let cmp = compare(&without_rule_lines(&golden), &without_rule_lines(&rust));
    assert_eq!(cmp.failures(), 0, "{}", cmp.report(20));
    assert_eq!(cmp.total(Kind::Pending), 0, "{}", cmp.report(20));
    assert!(cmp.total(Kind::Match) > 700, "{}", cmp.report(20));

    // Every primitive of every pad has the pad as its parent (C++ `SetParent( this )`).
    let mut primitives = 0;

    for (fp, _) in board.footprints() {
        for (pad_id, pad) in board.footprint_pads(fp) {
            for layer in pad.lib_padstack().unique_layers() {
                for primitive in pad.lib_padstack().primitives(layer) {
                    assert_eq!(
                        primitive.base.item.parent.item(),
                        Some(kicad_pcb::ids::ItemId::from(pad_id))
                    );
                    primitives += 1;
                }
            }
        }
    }

    assert_eq!(primitives, 3);
}
