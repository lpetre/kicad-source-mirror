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

//! Package PCBIO-10 (board file dimensions): differential tests against the reference
//! `kicad-cli pcb upgrade --force`.
//!
//! - `golden_dimension_blocks`: every board of `tests/data/pcbio10` (the hand-made
//!   `dims_v5` / `dims_v6` / `dims_v7` / `dims_current`, covering the legacy pre-20200610 form
//!   with feature / crossbar / arrow lines and units read from the text, the 6.0 order with the
//!   text before the style, bare `locked`, all five types and every format / style token, and
//!   dimensions-only extracts of every corpus board with board-level dimensions) is parsed and
//!   written by the port; every top-level `dimension` block must equal the oracle's re-save
//!   (`tests/golden/pcbio10/<name>.kicad_pcb`, written by `tests/oracle/pcbio10_golden.sh`)
//!   byte for byte.
//! - `corpus_dimension_blocks` (ignored; needs the oracle cache of `tools/io-corpus/run.sh`):
//!   the same block comparison over every cached corpus board (`O1` parsed, compared with `O2`).
//! - Port of `MalformedDimensionTextThrowsCleanly`
//!   (`qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`).

#[path = "io_corpus/resave.rs"]
mod resave;

use std::path::{Path, PathBuf};

use kicad_qa::sexpr_text::{SexprNode, parse_lists};
use resave::{Resave, rust_resave};

/// The top-level lists of `text` (children of `(kicad_pcb ..)`) whose keyword is in `keywords`.
fn top_level_blocks<'t>(text: &'t str, keywords: &[&str]) -> Vec<&'t str> {
    let roots = parse_lists(text);
    let Some(root) = roots.first() else {
        return Vec::new();
    };

    root.children
        .iter()
        .filter(|c| keywords.contains(&c.keyword.as_str()))
        .map(|c| c.text(text))
        .collect()
}

/// `block` with its `(render_cache ..)` lists removed (the oracle regenerates the cached
/// glyphs with the outline font engine from the host fonts, which the port does not have).
fn without_render_cache(block: &str) -> String {
    fn collect(node: &SexprNode, out: &mut Vec<(usize, usize)>) {
        if node.keyword == "render_cache" {
            out.push((node.start, node.end));
        } else {
            for c in &node.children {
                collect(c, out);
            }
        }
    }

    let mut spans = Vec::new();

    for n in parse_lists(block) {
        collect(&n, &mut spans);
    }

    let mut out = String::new();
    let mut last = 0;

    for (s, e) in spans {
        out.push_str(&block[last..s]);
        out.push_str("(render_cache)");
        last = e + 1;
    }

    out.push_str(&block[last..]);
    out
}

/// The keyword of this package.
const DIMENSION_KEYWORDS: [&str; 1] = ["dimension"];

/// Outcome of the block comparison of one board.
#[derive(Debug, Default)]
struct BlockCompare {
    blocks: usize,
    identical: usize,
    identical_modulo_cache: usize,
    first_mismatch: Option<(String, String)>,
}

fn compare_blocks(expected: &str, actual: &str) -> Result<BlockCompare, String> {
    let exp = top_level_blocks(expected, &DIMENSION_KEYWORDS);
    let act = top_level_blocks(actual, &DIMENSION_KEYWORDS);

    if exp.len() != act.len() {
        return Err(format!(
            "{} dimension blocks expected, {} written",
            exp.len(),
            act.len()
        ));
    }

    let mut r = BlockCompare {
        blocks: exp.len(),
        ..BlockCompare::default()
    };

    for (e, a) in exp.iter().zip(act.iter()) {
        if e == a {
            r.identical += 1;
        } else if without_render_cache(e) == without_render_cache(a) {
            r.identical_modulo_cache += 1;
        } else if r.first_mismatch.is_none() {
            r.first_mismatch = Some((e.to_string(), a.to_string()));
        }
    }

    Ok(r)
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every board of `tests/data/pcbio10`: the port's re-save writes each top-level `dimension`
/// exactly as the oracle's re-save (`tests/golden/pcbio10/<name>.kicad_pcb`).
#[test]
fn golden_dimension_blocks() {
    let data = crate_dir().join("tests").join("data").join("pcbio10");
    let golden = crate_dir().join("tests").join("golden").join("pcbio10");
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(&data)
        .expect("tests/data/pcbio10")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
    inputs.sort();

    let mut compared = 0;
    let mut blocks = 0;
    let mut failures: Vec<String> = Vec::new();

    for input in inputs {
        let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&input).unwrap();
        let expected = std::fs::read_to_string(golden.join(format!("{stem}.kicad_pcb"))).unwrap();

        let actual = match rust_resave(&text, &stem, true) {
            Resave::Written { text, .. } => text,
            Resave::ModelPending {
                stage, function, ..
            } => {
                eprintln!(
                    "{stem}: not compared, model pending ({}) {function}",
                    stage.label()
                );
                continue;
            }
            other => {
                failures.push(format!("{stem}: re-save failed: {other:?}"));
                continue;
            }
        };

        // PCBIO10_DUMP=<dir>: keep the port's re-saves for inspection.
        if let Some(dir) = std::env::var_os("PCBIO10_DUMP") {
            let _ = std::fs::write(Path::new(&dir).join(format!("{stem}.kicad_pcb")), &actual);
        }

        match compare_blocks(&expected, &actual) {
            Ok(r) => {
                if let Some((e, a)) = &r.first_mismatch {
                    failures.push(format!(
                        "{stem}: dimension block differs\n--- oracle\n{e}\n--- port\n{a}"
                    ));
                } else if r.identical != r.blocks {
                    failures.push(format!("{stem}: render caches differ"));
                }

                assert!(r.blocks > 0, "{stem}: no dimension blocks");
                blocks += r.blocks;
            }
            Err(e) => failures.push(format!("{stem}: {e}")),
        }

        compared += 1;
    }

    for f in &failures {
        eprintln!("MISMATCH {f}");
    }

    eprintln!("boards compared: {compared}, dimension blocks: {blocks}");
    assert!(failures.is_empty(), "{} boards mismatch", failures.len());
    assert!(compared >= 30, "only {compared} golden boards compared");
}

/// The oracle's re-save `O2` (or `O1` for a fixed point) of every cached corpus board against
/// the port's re-save of `O1`, compared block by block for the top-level `dimension`s.
#[test]
#[ignore = "needs the io-corpus oracle cache (tools/io-corpus/run.sh)"]
fn corpus_dimension_blocks() {
    let boards_dir = kicad_qa::io_corpus::io_corpus_dir().join("boards");
    let Ok(entries) = std::fs::read_dir(&boards_dir) else {
        eprintln!("no oracle cache in {}", boards_dir.display());
        return;
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    dirs.sort();

    let (mut boards, mut boards_identical, mut blocks, mut identical) = (0, 0, 0, 0);
    let mut failures: Vec<String> = Vec::new();
    let mut not_compared: Vec<String> = Vec::new();

    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(o1) = std::fs::read_to_string(dir.join("o1.kicad_pcb")) else {
            continue;
        };
        let expected = std::fs::read_to_string(dir.join("o2.kicad_pcb")).unwrap_or(o1.clone());

        if top_level_blocks(&expected, &DIMENSION_KEYWORDS).is_empty() {
            continue;
        }

        let actual = match rust_resave(&o1, &name, true) {
            Resave::Written { text, .. } => text,
            Resave::ModelPending {
                stage, function, ..
            } => {
                not_compared.push(format!(
                    "{name}: model pending ({}) {function}",
                    stage.label()
                ));
                continue;
            }
            other => {
                failures.push(format!("{name}: re-save failed: {other:?}"));
                continue;
            }
        };

        boards += 1;

        match compare_blocks(&expected, &actual) {
            Ok(r) => {
                blocks += r.blocks;
                identical += r.identical;

                if r.identical == r.blocks {
                    boards_identical += 1;
                }

                if let Some((e, a)) = r.first_mismatch {
                    failures.push(format!("{name}:\n--- oracle\n{e}\n--- port\n{a}"));
                } else if r.identical != r.blocks {
                    failures.push(format!("{name}: render caches differ"));
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }

    for n in &not_compared {
        eprintln!("not compared: {n}");
    }

    for f in &failures {
        eprintln!("MISMATCH {f}");
    }

    eprintln!(
        "boards with dimensions compared: {boards}; all blocks identical: {boards_identical}; \
         blocks: {blocks}, identical {identical}; mismatching boards: {}; not compared (model \
         pending): {}",
        failures.len(),
        not_compared.len()
    );

    assert!(failures.is_empty(), "{} boards mismatch", failures.len());
}

/// Port of `MalformedDimensionTextThrowsCleanly`
/// (`qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`): a dimension whose text has a
/// malformed `(at ..)` fails the load with an `IO_ERROR`, no crash.
#[test]
fn malformed_dimension_text_throws_cleanly() {
    use kicad_pcb::io::{ParseOptions, parse_board};

    let input = "(kicad_pcb (version 20240108) (generator \"test\")\
                 \x20(general (thickness 1.6)) (paper \"A4\")\
                 \x20(layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (36 \"B.SilkS\" user \"b.silkscreen\"))\
                 \x20(dimension (type aligned) (layer \"B.SilkS\")\
                 \x20  (gr_text \"1 mm\" (at broken))))";

    let result = parse_board(input, "malformed_dimension_text", &ParseOptions::default());
    assert!(result.is_err());
}

/// The `parseDIMENSION` error paths: an unknown token in the dimension, in `(format ..)` and in
/// `(style ..)`, a bad arrow direction and a missing `(type ..)` fail with the C++ texts.
#[test]
fn dimension_parse_errors() {
    use kicad_pcb::io::{ParseOptions, parse_board};

    let board = |dim: &str| {
        format!(
            "(kicad_pcb (version 20240108) (generator \"test\") (general (thickness 1.6)) \
             (paper \"A4\") (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal)) {dim})"
        )
    };
    let cases = [
        (
            "(dimension (type aligned) (bogus 1))",
            "layer, tstamp, uuid, gr_text, feature1, feature2, crossbar, arrow1a, arrow1b, \
             arrow2a, arrow2b, locked, or custom_property",
        ),
        (
            "(dimension (type aligned) (format (bogus 1)))",
            "prefix, suffix, units, units_format, precision, override_value, suppress_zeroes",
        ),
        (
            "(dimension (type aligned) (style (bogus 1)))",
            "thickness, arrow_length, arrow_direction, text_position_mode, extension_height, \
             extension_offset",
        ),
        (
            "(dimension (type aligned) (style (arrow_direction sideways)))",
            "inward or outward",
        ),
        ("(dimension (layer \"F.Cu\"))", "type"),
    ];

    for (dim, expected) in cases {
        let err = match parse_board(&board(dim), "errors", &ParseOptions::default()) {
            Ok(_) => panic!("{dim}: parsed"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains(expected), "{dim}: {err}");
    }
}
