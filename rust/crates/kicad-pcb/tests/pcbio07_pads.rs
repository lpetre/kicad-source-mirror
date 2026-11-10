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

//! PCBIO-07 differential oracle replay (board file pads and padstacks): every board in
//! `tests/data/pcbio07` (hand-made boards covering every pad token and version branch, and
//! corpus boards reduced to their footprints with pads by `tests/oracle/pcbio07_strip.py`)
//! is parsed and formatted by the port and compared byte for byte with the reference
//! `kicad-cli pcb upgrade --force` re-save in `tests/golden/pcbio07` (O1), and O1 with its own
//! re-save (O2). Goldens: `tests/oracle/pcbio07_golden.sh`.

#[path = "io_corpus/resave.rs"]
mod resave;

use std::path::{Path, PathBuf};

use kicad_pcb::io::parse_board_report;
use kicad_pcb::io::parser::ParseOptions;
use kicad_qa::uuid_canon::{canonicalise_uuids, canonicalise_uuids_sorted, count_random_uuids};
use resave::{Resave, rust_resave};

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn first_difference(expected: &str, actual: &str) -> Option<String> {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();

    for i in 0..e.len().max(a.len()) {
        if e.get(i) != a.get(i) {
            let from = i.saturating_sub(3);
            let ctx = |v: &[&str]| v[from.min(v.len())..(i + 3).min(v.len())].join("\n");
            return Some(format!(
                "line {}\n--- oracle\n{}\n--- port\n{}",
                i + 1,
                ctx(&e),
                ctx(&a)
            ));
        }
    }

    None
}

#[test]
fn golden_pads() {
    let data = crate_dir().join("tests").join("data").join("pcbio07");
    let golden = crate_dir().join("tests").join("golden").join("pcbio07");
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(&data)
        .expect("tests/data/pcbio07")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
    inputs.sort();

    let mut failures = Vec::new();
    let mut compared = 0;

    for input in inputs {
        let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&input).unwrap();
        let o1 = std::fs::read_to_string(golden.join(format!("{stem}.kicad_pcb"))).unwrap();
        let o2 = std::fs::read_to_string(golden.join(format!("{stem}_o2.kicad_pcb")))
            .unwrap_or_else(|_| o1.clone());

        for (label, source, expected) in [("input", &text, &o1), ("O1", &o1, &o2)] {
            let actual = match rust_resave(source, &stem, false) {
                Resave::Written { text, .. } => text,
                other => {
                    failures.push(format!("{stem} ({label}): re-save failed: {other:?}"));
                    continue;
                }
            };

            // PCBIO07_DUMP=<dir>: keep the port's re-saves for inspection.
            if let Some(dir) = std::env::var_os("PCBIO07_DUMP") {
                let _ = std::fs::write(
                    Path::new(&dir).join(format!("{stem}_{label}.kicad_pcb")),
                    &actual,
                );
            }

            compared += 1;

            // UUIDs the C++ draws at random (items without a UUID in a legacy input) are
            // masked; when such UUIDs decide an order (the footprint sort), sibling blocks
            // holding them are sorted too.
            let masked_expected = canonicalise_uuids(expected, source);
            let masked_actual = canonicalise_uuids(&actual, source);

            if let Some(diff) = first_difference(&masked_expected, &masked_actual) {
                if count_random_uuids(expected, source) > 0
                    && canonicalise_uuids_sorted(expected, source)
                        == canonicalise_uuids_sorted(&actual, source)
                {
                    continue;
                }

                failures.push(format!("{stem} ({label}): {diff}"));
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert!(compared >= 40, "only {compared} re-saves compared");
}

/// The zero-sized pad of `handmade_pads_current` (`(size 0 0.5)`, three footprints) is pinned
/// to 1 um with one parse warning each (`parsePAD`, "Invalid zero-sized pad pinned to ...").
#[test]
fn zero_sized_pad_warning() {
    let path = crate_dir()
        .join("tests")
        .join("data")
        .join("pcbio07")
        .join("handmade_pads_current.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();
    let (_, report) = parse_board_report(
        &text,
        "handmade_pads_current.kicad_pcb",
        &ParseOptions::default(),
    )
    .expect("parse");
    let warnings: Vec<&String> = report
        .warnings
        .iter()
        .filter(|w| w.starts_with("Invalid zero-sized pad pinned to 1µm in\nfile: "))
        .collect();
    assert_eq!(warnings.len(), 3, "{:?}", report.warnings);
}
