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
#[allow(dead_code)]
mod board_file_utils;
#[path = "io_corpus/resave.rs"]
#[allow(dead_code)]
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
                block = rest
                    .split(|c: char| c == ' ' || c == ')')
                    .next()
                    .unwrap_or("")
                    .to_string();
            }
        }
    }

    let keyword = lines
        .get(line)
        .map(|l| {
            l.trim_start()
                .trim_start_matches('(')
                .split(|c: char| c == ' ' || c == ')')
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
