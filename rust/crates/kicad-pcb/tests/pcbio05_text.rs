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

//! Package PCBIO-05 (board file text: effects, render cache, `gr_text`, `gr_text_box`):
//! differential tests against the reference `kicad-cli pcb upgrade --force`.
//!
//! - `golden_text_blocks`: the board `tests/golden/pcbio05/texts.kicad_pcb` (hand-written to
//!   cover the text tokens across format versions: legacy bare `hide` / `locked`, `%V` / `%R`,
//!   old overbar notation, missing text size, knockout, justify / mirror, line spacing, bold
//!   pen-width migration, text boxes with `start` / `end` and `pts`, `border` / `margins` /
//!   legacy stroke-only borders, `angle`) is parsed and written by the port; every top-level
//!   `gr_text` / `gr_text_box` block must equal the oracle's re-save
//!   (`tests/golden/pcbio05/<name>.expected.kicad_pcb`, written by
//!   `tests/oracle/pcbio05_golden.sh`) byte for byte.
//! - `corpus_text_blocks` (ignored; needs the oracle cache of `tools/io-corpus/run.sh`): the
//!   same block comparison over every corpus board.
//! - Ports of the I/O parts of `qa/tests/pcbnew/test_pcb_textbox.cpp` and of
//!   `FootprintUserFieldKeepsAutoThickness` (`qa/tests/pcbnew/test_footprint_load_save.cpp`)
//!   as far as they do not need the footprint parser (PCBIO-06).

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

/// `block` with the glyph polygons of its `(render_cache ..)` lists removed, the header kept
/// (`(render_cache "text" angle)`, as the golden script writes it). Package FONT-RC-1: the port
/// writes the glyphs stored in the input file, the oracle those of its host fonts (fontconfig
/// substitutes for fonts it lacks), so only the header is comparable.
fn with_cache_glyphs_masked(block: &str) -> String {
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
        let cache = &block[s..e];
        let header = cache.find("(polygon").map_or(cache, |i| &cache[..i]);
        out.push_str(&block[last..s]);
        out.push_str(header.trim_end());
        out.push(')');
        last = e + 1;
    }

    out.push_str(&block[last..]);
    out
}

/// The text keywords of this package.
const TEXT_KEYWORDS: [&str; 2] = ["gr_text", "gr_text_box"];

/// Outcome of the block comparison of one board.
#[derive(Debug, Default)]
struct BlockCompare {
    blocks: usize,
    identical: usize,
    identical_modulo_cache: usize,
    first_mismatch: Option<(String, String)>,
}

fn compare_blocks(expected: &str, actual: &str) -> Result<BlockCompare, String> {
    let exp = top_level_blocks(expected, &TEXT_KEYWORDS);
    let act = top_level_blocks(actual, &TEXT_KEYWORDS);

    if exp.len() != act.len() {
        return Err(format!(
            "{} text blocks expected, {} written",
            exp.len(),
            act.len()
        ));
    }

    let mut r = BlockCompare {
        blocks: exp.len(),
        ..BlockCompare::default()
    };

    for (e, a) in exp.iter().zip(act.iter()) {
        if e == a || with_cache_glyphs_masked(e) == with_cache_glyphs_masked(a) {
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

/// Every board of `tests/data/pcbio05`: the port's re-save writes each `gr_text` /
/// `gr_text_box` exactly as the oracle's re-save (`tests/golden/pcbio05/<name>.kicad_pcb`,
/// glyph polygons of outline-font render caches removed, see `pcbio05_golden.sh`). Boards that
/// reach a `port_pending!` model stub (text boxes until M11 lands) are reported and skipped.
#[test]
fn golden_text_blocks() {
    let data = crate_dir().join("tests").join("data").join("pcbio05");
    let golden = crate_dir().join("tests").join("golden").join("pcbio05");
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(&data)
        .expect("tests/data/pcbio05")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
    inputs.sort();

    let mut compared = 0;

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
            other => panic!("{stem}: re-save failed: {other:?}"),
        };

        // PCBIO05_DUMP=<dir>: keep the port's re-saves for inspection.
        if let Some(dir) = std::env::var_os("PCBIO05_DUMP") {
            let _ = std::fs::write(Path::new(&dir).join(format!("{stem}.kicad_pcb")), &actual);
        }

        let r = compare_blocks(&expected, &actual).unwrap_or_else(|e| panic!("{stem}: {e}"));

        if let Some((e, a)) = &r.first_mismatch {
            panic!("{stem}: text block differs\n--- oracle\n{e}\n--- port\n{a}");
        }

        assert!(r.blocks > 0, "{stem}: no text blocks");
        assert_eq!(r.identical, r.blocks, "{stem}: render caches differ");
        compared += 1;
    }

    assert!(compared >= 8, "only {compared} golden boards compared");
}

/// The oracle's re-save `O2` (or `O1` for a fixed point) of every cached corpus board against
/// the port's re-save of `O1`, compared block by block for `gr_text` / `gr_text_box`.
#[test]
#[ignore = "needs the io-corpus oracle cache (tools/io-corpus/run.sh)"]
fn corpus_text_blocks() {
    let boards_dir = kicad_qa::io_corpus::io_corpus_dir().join("boards");
    let Ok(entries) = std::fs::read_dir(&boards_dir) else {
        eprintln!("no oracle cache in {}", boards_dir.display());
        return;
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    dirs.sort();

    let (mut boards, mut boards_identical, mut boards_modulo_cache, mut blocks) = (0, 0, 0, 0);
    let (mut identical, mut modulo_cache) = (0, 0);
    let mut failures: Vec<String> = Vec::new();
    let mut not_compared: Vec<String> = Vec::new();

    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(o1) = std::fs::read_to_string(dir.join("o1.kicad_pcb")) else {
            continue;
        };
        let expected = std::fs::read_to_string(dir.join("o2.kicad_pcb")).unwrap_or(o1.clone());

        if top_level_blocks(&expected, &TEXT_KEYWORDS).is_empty() {
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
                modulo_cache += r.identical_modulo_cache;

                if r.identical == r.blocks {
                    boards_identical += 1;
                } else if r.identical + r.identical_modulo_cache == r.blocks {
                    boards_modulo_cache += 1;
                }

                if let Some((e, a)) = r.first_mismatch {
                    failures.push(format!("{name}:\n--- oracle\n{e}\n--- port\n{a}"));
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
        "boards with text compared: {boards}; all blocks identical: {boards_identical}; \
         identical except outline-font render caches: {boards_modulo_cache}; blocks: {blocks}, \
         identical {identical}, identical modulo render cache {modulo_cache}; mismatching \
         boards: {}; not compared (model pending): {}",
        failures.len(),
        not_compared.len()
    );

    assert!(failures.is_empty(), "{} boards mismatch", failures.len());
}

/// Port of the I/O side of `NonCardinalRotationKeepsLibShapeRectangle`
/// (`qa/tests/pcbnew/test_pcb_textbox.cpp`): "the on-disk form must stay a rectangle even after
/// a tilted rotation". A board text box rotated by 30 degrees is still written with `(start ..)
/// (end ..)` and the text angle. Skipped (reported) while the `PCB_TEXTBOX` model (M11) is
/// pending.
#[test]
fn non_cardinal_rotation_keeps_lib_shape_rectangle() {
    use kicad_geom::{EdaAngle, Vector2I};
    use kicad_pcb::AnyItem;
    use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board};

    let input = std::fs::read_to_string(
        crate_dir()
            .join("tests")
            .join("data")
            .join("pcbio05")
            .join("textboxes_current.kicad_pcb"),
    )
    .unwrap();

    resave::quiet_pending_panics();
    let run = std::panic::catch_unwind(|| {
        let options = ParseOptions {
            allow_pending: true,
        };
        let mut board = parse_board(&input, "textboxes_current", &options).unwrap();
        let ids: Vec<_> = board
            .drawings()
            .filter(|(_, item)| matches!(item, AnyItem::Textbox(_)))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids.len(), 3);

        for id in ids {
            board.rotate_item(id, Vector2I::new(0, 0), EdaAngle::from_degrees(30.0));
        }

        let options = FormatOptions {
            allow_pending: true,
            ..FormatOptions::default()
        };
        format_board(&board, &options).unwrap()
    });

    let text = match run {
        Ok(text) => text,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            assert!(msg.starts_with(resave::PENDING_PANIC), "{msg}");
            eprintln!("not run: {msg}");
            return;
        }
    };

    for block in top_level_blocks(&text, &["gr_text_box"]) {
        assert!(
            block.contains("(start ") && block.contains("(end "),
            "{block}"
        );
        assert!(!block.contains("(pts"), "{block}");
        assert!(block.contains("(angle "), "{block}");
    }
}
