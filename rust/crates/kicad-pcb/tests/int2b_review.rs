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

//! Review package INT-2b (`rust/docs/design/review-2.md`): regression tests of the review's
//! fixes and the legacy-input corpus comparison (D3 of `docs/analysis/pcb-io.md` 9.3).
//!
//! * `d3_legacy_scoreboard` (ignored; needs the oracle cache of `rust/tools/io-corpus/run.sh`):
//!   for every corpus board the Rust parse + format of the **original** file is compared with
//!   O1, the reference `kicad-cli pcb upgrade --force` of the same file, after
//!   `kicad_qa::uuid_canon::canonicalise_uuids_sorted` on both. This is the only comparison
//!   that exercises the legacy branches of the parser (`m_requiredVersion < ...`); the PCBIO-17
//!   scoreboard reads O1, which is always in the newest format. Writes
//!   `rust/target/int2b/d3.md`.

#[path = "io_corpus/board_file_utils.rs"]
mod board_file_utils;
#[path = "io_corpus/resave.rs"]
mod resave;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use kicad_qa::io_corpus::{io_corpus_boards, io_corpus_dir};
use kicad_qa::uuid_canon::canonicalise_uuids_sorted;
use resave::{Resave, rust_resave};

/// The top-level block (`\t(keyword` at depth one) and the keyword of line `line` of `text`.
fn locate(text: &str, line: usize) -> String {
    let mut block = String::from("(header)");
    let lines: Vec<&str> = text.lines().collect();

    for l in lines.iter().take(line + 1) {
        if let Some(rest) = l.strip_prefix("\t(") {
            if !rest.starts_with('\t') {
                block = rest.split([' ', ')']).next().unwrap_or("").to_string();
            }
        }
    }

    let keyword = lines
        .get(line)
        .map(|l| {
            l.trim_start()
                .trim_start_matches('(')
                .split([' ', ')'])
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();

    format!("{block} > {keyword}")
}

/// D3: `rust_resave( original )` against O1, UUIDs canonicalised.
#[test]
#[ignore = "corpus run with the oracle cache: rust/tools/io-corpus/run.sh first"]
fn d3_legacy_scoreboard() {
    let only = std::env::var("D3_BOARD").ok();
    let mut rows = Vec::new();
    let mut histogram: BTreeMap<String, usize> = BTreeMap::new();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for board in io_corpus_boards() {
        if board.skip.is_some() {
            continue;
        }

        if let Some(only) = &only {
            if !board.name.contains(only.as_str()) {
                continue;
            }
        }

        let o1_path = io_corpus_dir()
            .join("boards")
            .join(board.flat_name())
            .join("o1.kicad_pcb");

        let (Ok(original), Ok(o1)) = (
            std::fs::read(&board.path).map(|b| String::from_utf8_lossy(&b).into_owned()),
            std::fs::read_to_string(&o1_path),
        ) else {
            continue;
        };

        let (status, detail): (&'static str, String) =
            match rust_resave(&original, &board.file_name(), false) {
                Resave::Written { text, .. } => {
                    let rust = canonicalise_uuids_sorted(&text, &original);
                    let reference = canonicalise_uuids_sorted(&o1, &original);

                    if rust == reference {
                        ("identical", String::new())
                    } else {
                        let line = rust
                            .lines()
                            .zip(reference.lines())
                            .position(|(a, b)| a != b)
                            .unwrap_or_else(|| rust.lines().count().min(reference.lines().count()));
                        let place = locate(&reference, line);
                        *histogram.entry(place.clone()).or_default() += 1;

                        if only.is_some() {
                            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                                .join("../../target/int2b/d3");
                            std::fs::create_dir_all(&dir).unwrap();
                            std::fs::write(dir.join("rust.kicad_pcb"), &rust).unwrap();
                            std::fs::write(dir.join("o1.kicad_pcb"), &reference).unwrap();
                        }

                        ("differs", format!("line {}: {place}", line + 1))
                    }
                }
                Resave::ParseError(m) => ("parse-error", m),
                Resave::FormatError { message, .. } => ("format-error", message),
                Resave::ModelPending {
                    stage, function, ..
                } => ("model-pending", format!("{} {function}", stage.label())),
                Resave::Panic { stage, message } => {
                    ("panic", format!("{} {message}", stage.label()))
                }
            };

        *counts.entry(status).or_default() += 1;
        rows.push((board.name.clone(), status, detail));
    }

    let mut out = String::from("# D3: rust_resave( original ) vs O1, UUIDs canonicalised\n\n");

    for (status, n) in &counts {
        writeln!(out, "* {status}: {n}").unwrap();
    }

    out.push_str("\n## First differences\n\n");

    for (place, n) in &histogram {
        writeln!(out, "* {place}: {n}").unwrap();
    }

    out.push_str("\n## Boards that are not identical\n\n");

    for (name, status, detail) in &rows {
        if *status != "identical" {
            let detail: String = detail.chars().take(200).collect();
            writeln!(out, "* {name}: {status} {detail}").unwrap();
        }
    }

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/int2b");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("d3.md"), &out).unwrap();
    println!("{out}");
}

// ---- regression tests of the review's fixes ------------------------------------------------

/// R2-05: a freshly drawn UUID never equals one stored in the file. The board's generator is
/// seeded deterministically (`KIID::SeedGenerator( 5489 )` sequence: 5cbb91d0-..., 2c358220-...,
/// ...); a file written by an equally seeded KiCad holds those very UUIDs. Here the second
/// line carries the UUID the first (UUID-less) line would otherwise draw.
#[test]
fn fresh_uuids_never_collide_with_file_uuids() {
    use kicad_pcb::io::{ParseOptions, parse_board};

    let mut ids = Vec::new();

    // The UUIDs a default-seeded generator hands out first
    let mut generator = kicad_core::KiidGenerator::default();
    for _ in 0..64 {
        ids.push(generator.generate().as_string());
    }

    let mut text = String::from(
        "(kicad_pcb (version 20241229) (generator \"pcbnew\") (generator_version \"9.0\")\n\
         (general (thickness 1.6) (legacy_teardrops no)) (paper \"A4\")\n\
         (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (25 \"Edge.Cuts\" user))\n\
         (setup (pad_to_mask_clearance 0)) (net 0 \"\")\n\
         (gr_line (start 0 0) (end 1 0) (stroke (width 0.1) (type default)) (layer \"Edge.Cuts\"))\n",
    );

    // ... followed by lines that carry every one of those UUIDs
    for (i, id) in ids.iter().enumerate() {
        text.push_str(&format!(
            "(gr_line (start 0 {i}) (end 1 {i}) (stroke (width 0.1) (type default)) \
             (layer \"Edge.Cuts\") (uuid \"{id}\"))\n"
        ));
    }

    text.push(')');

    let board = parse_board(&text, "", &ParseOptions::default()).unwrap();
    let ctx = kicad_pcb::BoardCtx::bare(&board);
    let mut seen = std::collections::BTreeSet::new();

    for (id, _) in board.drawings() {
        let uuid = ctx.item(id).unwrap().uuid().as_string();
        assert!(seen.insert(uuid.clone()), "duplicate UUID {uuid}");
    }

    assert_eq!(seen.len(), 65);

    // and later draws (markers, new items) keep avoiding them
    let mut board = board;
    for _ in 0..64 {
        let fresh = board.kiid_generator.generate().as_string();
        assert!(!ids.contains(&fresh), "{fresh} is a UUID of the file");
    }
}

/// `SECTION|KEY|FIELD` -> value of dump lines.
#[cfg(feature = "dump")]
fn dump_map(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Replay of `tests/golden/int2b_review.txt` (reference `pcb-dump board --no-init --hash`,
/// script `tests/oracle/int2b_golden.sh`):
///
/// * R2-03: `PAD::SetLocalSolderMaskMargin` / `SetLocalSolderPasteMargin` /
///   `SetLocalSolderPasteMarginRatio` write both outer sides; the port wrote the front side
///   only, so the back mask / paste margins, `GetSolderMaskExpansion( B_Mask )`,
///   `GetSolderPasteMargin( B_Paste )` and the back mask shapes of every pad with a local
///   margin were wrong.
/// * R2-04: `bakeTextBoxLib` moves the cells of a table inside a footprint to the board
///   frame (`GetParentFootprint()` walks through the table); the port left them at their
///   library coordinates.
#[cfg(feature = "dump")]
#[test]
fn int2b_matches_oracle() {
    use kicad_pcb::dump::{DumpLoadMode, DumpOptions, dump_board, load_board_for_dump_mode};

    let golden = include_str!("golden/int2b_review.txt");
    let root = kicad_qa::qa_data_dir().join("../..");
    let mut checked = 0;
    let mut expected: Vec<(String, String, Vec<&str>)> = Vec::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("file|") {
            let (path, sections) = rest.split_once("|sections=").unwrap();
            expected.push((path.to_string(), sections.to_string(), Vec::new()));
        } else if let Some(last) = expected.last_mut() {
            last.2.push(line);
        }
    }

    assert_eq!(expected.len(), 4);

    for (path, sections, lines) in expected {
        let file = root.join(&path);
        let bytes = std::fs::read(&file).unwrap();
        let (board, _) = load_board_for_dump_mode(&file, DumpLoadMode::NoInit).unwrap();
        let opts = DumpOptions {
            sections: sections.split(',').map(str::to_string).collect(),
            hash: true,
            file_uuids: Some(kicad_pcb::dump::scan_file_uuids(&bytes)),
        };
        let got = dump_map(&dump_board(&board, &opts));

        for line in lines {
            let (key, value) = line.split_once('=').unwrap();
            assert_eq!(
                got.get(key).map(String::as_str),
                Some(value),
                "{path}: {key}"
            );
            checked += 1;
        }
    }

    assert_eq!(checked, 414);
}
