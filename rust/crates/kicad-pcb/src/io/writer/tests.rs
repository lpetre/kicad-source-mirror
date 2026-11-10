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

//! Tests of the writer core (PCBIO-02): helper semantics, the staging stubs, the differential
//! oracle against `kicad-cli pcb upgrade --force` (`tests/oracle/pcbio02_resave_golden.sh`,
//! goldens in `tests/golden/pcbio02/`), the corpus header oracle
//! (`tests/oracle/pcbio02_corpus_golden.sh`) and the port of `qa/tests/pcbnew/test_prettifier.cpp`.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::{Lset, PcbLayerId};
use kicad_geom::{ShapeArc, ShapeLineChain, Vector2I};
use kicad_sexpr::{FormatMode, OutputFormatter, StringFormatter, prettify};

use super::*;
use crate::board::Board;
use crate::io::{ParseOptions, parse_board, parse_board_report, parse_footprint_file};

/// The prefix of a `port_pending!` panic.
const PENDING: &str = "kicad-pcb: port pending: ";

/// Run `f`; a `port_pending!` panic of a not yet ported model function becomes `Err(name)`.
fn model<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned());

            match msg {
                Some(m) if m.starts_with(PENDING) => Err(m[PENDING.len()..].to_string()),
                _ => resume_unwind(payload),
            }
        }
    }
}

fn staging_parse() -> ParseOptions {
    ParseOptions {
        allow_pending: true,
    }
}

fn staging() -> FormatOptions {
    FormatOptions {
        allow_pending: true,
        ..FormatOptions::default()
    }
}

/// A writer over `board` printing into a plain string formatter.
fn with_writer<R>(
    board: &Board,
    with_board: bool,
    f: impl FnOnce(&mut PcbWriter<'_>) -> R,
) -> (R, String) {
    let mut out = StringFormatter::new();
    let r = {
        let mut writer = PcbWriter::new(&mut out, board, &staging());

        if with_board {
            writer.board = Some(board);
        }

        f(&mut writer)
    };
    (r, out.into_string())
}

// ---- helpers ------------------------------------------------------------------------------

/// `formatLayers`: the wildcard rules, with and without a board (copper count of the board vs.
/// all 32 copper layers), and the zone `F&B.Cu` branch. Expectations derived from the C++
/// (`pcb_io_kicad_sexpr.cpp` `formatLayers`); the oracle check of the bytes comes with the pad,
/// track and zone packages, which are its callers.
#[test]
fn format_layers_wildcards() {
    let mut board = Board::new();
    board.set_copper_layer_count(4);
    let all_cu4 = Lset::all_cu_mask_n(4);
    let fb = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]);
    let tech = Lset::from_layers(&[
        PcbLayerId::FMask,
        PcbLayerId::BMask,
        PcbLayerId::FPaste,
        PcbLayerId::FSilkS,
        PcbLayerId::BSilkS,
        PcbLayerId::FFab,
        PcbLayerId::BFab,
        PcbLayerId::FCrtYd,
        PcbLayerId::BCrtYd,
        PcbLayerId::FAdhes,
        PcbLayerId::BAdhes,
        PcbLayerId::BPaste,
    ]);

    let cases: Vec<(Lset, bool, bool, bool, &str)> = vec![
        // mask, with board, enumerate, zone, expected
        (
            &all_cu4 | &Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]),
            true,
            false,
            false,
            "(layers  \"*.Cu\" \"*.Mask\")",
        ),
        // all 32 copper layers needed without a board
        (
            all_cu4.clone(),
            false,
            false,
            false,
            "(layers  \"F.Cu\" \"B.Cu\" \"In1.Cu\" \"In2.Cu\")",
        ),
        (
            Lset::all_cu_mask().clone(),
            false,
            false,
            false,
            "(layers  \"*.Cu\")",
        ),
        // internal copper beyond the board is cleared with the wildcard
        (
            Lset::all_cu_mask().clone(),
            true,
            false,
            false,
            "(layers  \"*.Cu\")",
        ),
        (fb.clone(), true, false, false, "(layers  \"*.Cu\")"),
        (fb.clone(), true, false, true, "(layers  \"F&B.Cu\")"),
        (fb.clone(), true, true, true, "(layers  \"F.Cu\" \"B.Cu\")"),
        (
            &fb | &Lset::from_layers(&[PcbLayerId::In1Cu]),
            true,
            false,
            false,
            "(layers  \"F.Cu\" \"B.Cu\" \"In1.Cu\")",
        ),
        (
            tech.clone(),
            true,
            false,
            false,
            "(layers  \"*.Adhes\" \"*.Paste\" \"*.SilkS\" \"*.Mask\" \"*.CrtYd\" \"*.Fab\")",
        ),
        (
            tech.clone(),
            true,
            true,
            false,
            "(layers  \"F.Mask\" \"B.Mask\" \"F.SilkS\" \"B.SilkS\" \"F.Adhes\" \"B.Adhes\" \
             \"F.Paste\" \"B.Paste\" \"B.CrtYd\" \"F.CrtYd\" \"B.Fab\" \"F.Fab\")",
        ),
        (
            Lset::from_layers(&[PcbLayerId::FPaste, PcbLayerId::FCu, PcbLayerId::User1]),
            true,
            false,
            false,
            "(layers  \"F.Cu\" \"F.Paste\" \"User.1\")",
        ),
        (Lset::new(), true, false, false, "(layers )"),
    ];

    for (mask, with_board, enumerate, zone, expected) in cases {
        let (r, text) = with_writer(&board, with_board, |w| {
            w.format_layers(mask.clone(), enumerate, zone)
        });
        r.unwrap();
        assert_eq!(
            text, expected,
            "{mask:?} board={with_board} enum={enumerate} zone={zone}"
        );
    }
}

/// `formatLayer`: the knockout word, or an empty word (double space before `)` is removed by
/// Prettify).
#[test]
fn format_layer_knockout() {
    let board = Board::new();
    let (r, text) = with_writer(&board, true, |w| {
        w.format_layer(PcbLayerId::FSilkS, false)?;
        w.format_layer(PcbLayerId::BSilkS, true)
    });
    r.unwrap();
    assert_eq!(text, "(layer \"F.SilkS\" )(layer \"B.SilkS\" knockout)");
    assert_eq!(
        prettify(&format!("(x {text})"), FormatMode::Normal),
        "(x\n\t(layer \"F.SilkS\")\n\t(layer \"B.SilkS\" knockout)\n)\n"
    );
}

/// `formatPolyPts`: plain points as `(xy ..)`, every run of points of one arc as one
/// `(arc (start ..) (mid ..) (end ..))`, small values in the `%.10f` branch.
#[test]
fn format_poly_pts_arcs() {
    let board = Board::new();
    let mut chain = ShapeLineChain::new();
    chain.append_xy(0, 0, false);
    chain.append_xy(1_000_000, 0, false);
    chain.append_arc(&ShapeArc::from_start_mid_end(
        Vector2I::new(1_000_000, 0),
        Vector2I::new(1_707_107, 707_107),
        Vector2I::new(2_000_000, 1_500_000),
        0,
    ));
    chain.append_xy(50, -2_500_000, false);

    let (r, text) = with_writer(&board, true, |w| w.format_poly_pts(&chain, None));
    r.unwrap();
    assert_eq!(
        text,
        "(pts(xy 0 0)(arc (start 1 0) (mid 1.707107 0.707107) (end 2 1.5))(xy 0.00005 -2.5))"
    );
}

/// `unbakeSize`, `unbakeSizeUniform`, `unbakeLinear` without a footprint are the identity.
#[test]
fn unbake_without_footprint() {
    let v = Vector2I::new(123, -456);
    assert_eq!(unbake_size(v, None), v);
    assert_eq!(unbake_size_uniform(v, None), v);
    assert_eq!(unbake_linear(789, None), 789);
    assert_eq!(format_internal_units_pt_fp(v, None), "0.000123 -0.000456");
    assert_eq!(format_iu(1_600_000), "1.6");
    assert_eq!(format_iu(-1), "-0.000001");
}

/// `std::set( begin, end )`: comparator order, an element equivalent to an earlier one is
/// dropped (the earlier one is kept).
#[test]
fn sorted_set_is_std_set() {
    let items = [(3, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
    let set = sorted_set(items, |a, b| a.0.cmp(&b.0));
    assert_eq!(set, vec![(1, 'b'), (2, 'd'), (3, 'a')]);
}

/// The CTL combinations of `pcb_io_kicad_sexpr.h`.
#[test]
fn ctl_flags() {
    assert_eq!(CTL_FOR_BOARD, 0x500);
    assert_eq!(CTL_FOR_LIBRARY, 0xb6);
    assert_eq!(CTL_FOR_CLIPBOARD, 0x400);
    assert_eq!(FormatOptions::default().ctl, CTL_FOR_BOARD);
}

// ---- staging stubs -------------------------------------------------------------------------

/// A save that reaches a staging stub fails with its "port pending" text. Every item of a
/// board file is ported (footprints PCBIO-06, pads PCBIO-07): a board with a footprint saves
/// in both modes with nothing skipped; `FormatFootprintLibFile` (PCBIO-14) has landed too.
#[test]
fn staging_stubs() {
    let mut board = Board::new();
    let fp = board.create_footprint();
    board.attach_footprint(fp, crate::board::AddMode::Append);
    let strict = format_board(&board, &FormatOptions::default()).unwrap();
    let (text, report) = format_board_report(&board, &staging()).unwrap();
    assert_eq!(text, strict);
    assert!(report.skipped.is_empty());

    // FormatFootprintLibFile (PCBIO-14) is ported too.
    let mut holder = Board::new();
    let fp = holder.create_footprint();
    let (_, report) = format_footprint_lib_file(&holder, fp, &staging()).unwrap();
    assert!(report.skipped.is_empty());
}

/// The skeleton of a default board (`BOARD::BOARD()`), compared with what the reference build
/// writes for the minimal current-format file whose header equals the defaults
/// (`current_min.kicad_pcb` re-saved; its setup section belongs to PCBIO-04).
#[test]
fn default_board_skeleton() {
    let board = Board::new();
    let text = match model(|| format_board(&board, &staging())) {
        Ok(r) => r.unwrap(),
        Err(pending) => {
            println!("default_board_skeleton: pending on {pending}");
            return;
        }
    };
    assert!(text.starts_with(
        "(kicad_pcb\n\t(version 20261031)\n\t(generator \"pcbnew\")\n\t(generator_version \"10.99\")\n\t(general\n\t\t(thickness 1.6)\n\t\t(legacy_teardrops no)\n\t)\n\t(paper \"A4\")\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(2 \"B.Cu\" signal)\n"
    ), "{text}");
    assert!(text.ends_with("\t)\n\t(embedded_fonts no)\n)\n"), "{text}");
}

// ---- differential oracle: kicad-cli pcb upgrade --force -------------------------------------

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pcbio02")
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pcbio02")
}

/// The state that parsers of other, still pending packages would have read from the inputs
/// (PCBIO-11: `(variants ..)`, `(embedded_fonts ..)`), set by hand so that the writer of this
/// package is checked on them. Idempotent once those parsers land.
fn apply_pending_parser_state(name: &str, board: &mut Board) {
    match name {
        "variants.kicad_pcb" => {
            board.add_variant("Lite");
            board.set_variant_description("Lite", "No radio");
            board.add_variant("Pro");
            board.add_variant("ALPHA");
            board.set_variant_description("ALPHA", "a \"b\"");
        }
        "fonts_embedded.kicad_pcb" => board.embedded_files.set_are_fonts_embedded(true),
        _ => {}
    }
}

/// Parse + format of `text` (staging), compared with `expected` byte for byte.
fn check_resave(label: &str, name: &str, text: &str, expected: &str) -> Result<(), String> {
    model(|| {
        let (mut board, parse_report) =
            parse_board_report(text, name, &staging_parse()).map_err(|e| e.what().to_string())?;

        for skipped in &parse_report.skipped {
            assert!(
                matches!(
                    skipped.token.as_str(),
                    "setup" | "variants" | "embedded_fonts"
                ),
                "{label}: unexpected skipped construct {skipped:?}"
            );
        }

        apply_pending_parser_state(name, &mut board);

        let (out, report) =
            format_board_report(&board, &staging()).map_err(|e| e.what().to_string())?;

        // Nothing these boards contain is a staging stub any more (the setup section of
        // PCBIO-04 is ported), so the whole file is compared.
        assert_eq!(report.skipped, Vec::<SkippedFormat>::new(), "{label}");

        assert_eq!(out, expected, "{label}: bytes differ from the oracle");
        Ok(())
    })
    .unwrap_or_else(|pending| {
        println!("{label}: pending on model stub {pending}");
        Ok(())
    })
}

/// Differential oracle: for every hand-made board of `tests/data/pcbio02/` (header, general,
/// paper, title block, layers incl. user-defined and user-named layers, legacy net
/// declarations, properties, variants, embedded-fonts flag; no items), parse + format must
/// reproduce the bytes of the reference `kicad-cli pcb upgrade --force` (the whole file,
/// including the `(setup ..)` section of PCBIO-04).
/// The oracle output itself is replayed too (parse + format of O1 must give O1, or the stored
/// second re-save `*_o2.kicad_pcb` where the reference build changes it once more).
#[test]
fn oracle_resave() {
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(data_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    inputs.sort();
    assert!(inputs.len() >= 8, "{inputs:?}");

    let mut failures = Vec::new();

    for input in &inputs {
        let name = input.file_name().unwrap().to_str().unwrap().to_string();
        let text = std::fs::read_to_string(input).unwrap();
        let golden = std::fs::read_to_string(golden_dir().join(&name)).unwrap();

        if let Err(e) = check_resave(&format!("{name} (input)"), &name, &text, &golden) {
            failures.push(format!("{name} (input): {e}"));
        }

        let o2_path = golden_dir().join(name.replace(".kicad_pcb", "_o2.kicad_pcb"));
        let o2 = std::fs::read_to_string(&o2_path).unwrap_or_else(|_| golden.clone());

        if let Err(e) = check_resave(&format!("{name} (O1)"), &name, &golden, &o2) {
            failures.push(format!("{name} (O1): {e}"));
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- corpus header oracle -------------------------------------------------------------------

/// FNV-1a 64 of `bytes` (the hash of `pcb-dump --hash` and of the corpus golden script).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}

/// The skeleton regions of a saved board: (A) everything from the start to the end of the
/// top-level `(layers ..)` block (header, general, paper, title block, layers), (B) the
/// top-level `(property ..)` / `(variants ..)` blocks right after it and after the `(setup ..)`
/// block (skipped if present: absent in the Rust output while PCBIO-04 is pending), (C) the
/// `\t(embedded_fonts ..)` line. Same definition as in `pcbio02_corpus_golden.sh`.
fn skeleton_regions(text: &str) -> [String; 3] {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut a = String::new();
    let mut b = String::new();
    let mut c = String::new();
    let mut i = 0;

    while i < lines.len() && lines[i] != "\t(layers\n" {
        a.push_str(lines[i]);
        i += 1;
    }

    while i < lines.len() && lines[i] != "\t)\n" {
        a.push_str(lines[i]);
        i += 1;
    }

    if i < lines.len() {
        a.push_str(lines[i]);
        i += 1;
    }

    if i < lines.len() && lines[i] == "\t(setup\n" {
        while i < lines.len() && lines[i] != "\t)\n" {
            i += 1;
        }

        i += 1;
    }

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("\t(property ") {
            b.push_str(line);
            i += 1;
        } else if line == "\t(variants\n" {
            while i < lines.len() && lines[i] != "\t)\n" {
                b.push_str(lines[i]);
                i += 1;
            }

            if i < lines.len() {
                b.push_str(lines[i]);
            }

            i += 1;
        } else {
            break;
        }
    }

    for line in &lines {
        if line.starts_with("\t(embedded_fonts ") {
            c.push_str(line);
        }
    }

    [a, b, c]
}

/// Corpus oracle: for the corpus boards listed in `tests/golden/pcbio02_corpus.txt` (written
/// by `tests/oracle/pcbio02_corpus_golden.sh` from `kicad-cli pcb upgrade --force` of each
/// board copied alone), parse (staging) + format (staging) must give the same skeleton regions
/// ([`skeleton_regions`]) as the reference re-save. Boards whose parse reaches a model stub are
/// reported as pending; boards the reference build rejects are skipped (the parser package
/// checks the errors). A differing region is accepted (and reported) only when the parse
/// skipped a construct whose pending parser feeds that region (legacy teardrop zones for the
/// `(general ..)` flag, `(variants ..)`, `(embedded_fonts ..)`).
#[test]
fn oracle_corpus_skeleton() {
    let golden = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/pcbio02_corpus.txt"),
    )
    .unwrap();
    let root = kicad_qa::kicad_source_root();
    let (mut compared, mut pending, mut pending_parsers) = (0, 0, 0);
    let mut failures = Vec::new();

    for line in golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let fields: Vec<&str> = line.split('|').collect();
        let (path, expected) = (fields[0], &fields[1..]);

        if expected[0] == "error" {
            continue;
        }

        let bytes = std::fs::read(root.join(path)).unwrap();
        let text = String::from_utf8_lossy(&bytes);

        let result = model(|| {
            let (board, report) = parse_board_report(&text, path, &staging_parse())
                .map_err(|e| e.what().to_string())?;
            let out = format_board(&board, &staging()).map_err(|e| e.what().to_string())?;
            Ok::<_, String>((out, report))
        });

        match result {
            Err(stub) => {
                pending += 1;
                println!("{path}: pending on model stub {stub}");
            }
            Ok(Err(e)) => failures.push(format!("{path}: {e}")),
            Ok(Ok((out, report))) => {
                compared += 1;
                let regions = skeleton_regions(&out);
                let skipped = |token: &str| report.skipped.iter().any(|s| s.token == token);

                for (k, region) in regions.iter().enumerate() {
                    let hash = format!("{:016x}", fnv1a64(region.as_bytes()));

                    if hash == expected[k] {
                        continue;
                    }

                    // Constructs read by still pending parsers that feed a skeleton region:
                    // `parseZONE` sets the legacy teardrops flag of `(general ..)` (PCBIO-08),
                    // `(variants ..)` and `(embedded_fonts ..)` are PCBIO-11.
                    let explained = match k {
                        0 => skipped("zone"),
                        1 => skipped("variants"),
                        _ => skipped("embedded_fonts"),
                    };
                    let what = ["header", "properties/variants", "embedded_fonts"][k];

                    if explained {
                        pending_parsers += 1;
                        println!("{path}: region {what} differs, explained by a pending parser");
                    } else {
                        failures.push(format!("{path}: region {what} differs:\n{region}"));
                    }
                }
            }
        }
    }

    println!(
        "oracle_corpus_skeleton: {compared} compared, {pending} pending on model stubs, \
         {pending_parsers} regions differing because of pending parsers"
    );
    assert!(compared > 0);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- qa/tests/pcbnew/test_prettifier.cpp ----------------------------------------------------

/// Port of `BoardAndFootprintPrettifier`: the input parses, `Prettify( input )` matches the
/// golden `*_formatted.*` byte for byte, and the prettified text parses the same way. "Parses
/// the same way" (`*original == *prettified`, needs `BOARD::operator==`) is checked as equal
/// output of the writer for both parses. The two footprint cases need the footprint library
/// parser (PCBIO-14) and the footprint writer (PCBIO-06): until then they check the golden
/// only and report the parse as pending.
#[test]
fn board_and_footprint_prettifier() {
    let cases = [
        "Reverb_BTDR-1V.kicad_mod",
        "Samtec_HLE-133-02-xx-DV-PE-LC_2x33_P2.54mm_Horizontal.kicad_mod",
        "group_and_image.kicad_pcb",
    ];

    for test_case in cases {
        let dir = kicad_qa::pcbnew_data_dir().join("prettifier");
        let in_data = std::fs::read_to_string(dir.join(test_case)).unwrap();
        let prettified = prettify(&in_data, FormatMode::Normal);

        let (base, ext) = test_case.rsplit_once('.').unwrap();
        let golden = std::fs::read_to_string(dir.join(format!("{base}_formatted.{ext}"))).unwrap();
        assert!(
            golden == prettified,
            "{test_case}: Formatting result doesn't match golden!"
        );

        if ext == "kicad_mod" {
            // `*original == *prettified` (with the footprint UUID hack) as: both parse and
            // write the same library file text.
            let r = model(|| {
                let lib = |text: &str, source: &str| {
                    let (holder, _) = parse_footprint_file(text, source, &staging_parse()).unwrap();
                    let fp = holder.footprints().next().map(|(id, _)| id).unwrap();
                    format_footprint_lib_file(&holder, fp, &staging())
                        .unwrap()
                        .0
                };
                assert!(
                    lib(&in_data, "input file") == lib(&prettified, "prettified file"),
                    "{test_case}: Formatted version of original board item does not parse the same way!"
                );
            });

            if let Err(pending) = r {
                println!("{test_case}: pending on model stub {pending}");
            }
            continue;
        }

        let r = model(|| {
            let original = parse_board(&in_data, "input file", &staging_parse()).unwrap();
            let again = parse_board(&prettified, "prettified file", &staging_parse()).unwrap();
            let a = format_board(&original, &staging()).unwrap();
            let b = format_board(&again, &staging()).unwrap();
            assert!(
                a == b,
                "{test_case}: Formatted version of original board item does not parse the same way!"
            );
        });

        if let Err(pending) = r {
            println!("{test_case}: pending on model stub {pending}");
        }
    }
}

/// The writer prints through `&mut dyn OutputFormatter`: the raw token stream of a header is
/// what the C++ `Print` calls produce (before Prettify).
#[test]
fn raw_token_stream() {
    let mut board = Board::new();
    board.properties.insert("b".into(), "2".into());
    board.properties.insert("a".into(), "x\"y".into());
    let mut out = StringFormatter::new();
    let report = model(|| format_board_to_formatter(&mut out, &board, &staging()));

    if let Ok(report) = report {
        report.unwrap();
        let raw = out.string();
        assert!(
            raw.starts_with(
                "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")\
             (general(thickness 1.6)(legacy_teardrops no))(paper \"A4\")(layers(0 \"F.Cu\" signal )"
            ),
            "{raw}"
        );
        assert!(
            raw.ends_with("(property \"a\" \"x\\\"y\")(property \"b\" \"2\")(embedded_fonts no))"),
            "{raw}"
        );
        let _ = out.quote_char();
    }
}
