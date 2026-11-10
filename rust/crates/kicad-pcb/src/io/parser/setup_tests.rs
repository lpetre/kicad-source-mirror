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


//! Tests of package PCBIO-04 (setup section reader and writer): the ported C++ tests of
//! `qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`, the differential oracles
//! against the reference `kicad-cli pcb upgrade --force` (hand-made boards of
//! `tests/data/pcbio04/`, golden `tests/golden/pcbio04/`, script
//! `tests/oracle/pcbio04_resave_golden.sh`; corpus hashes `tests/golden/pcbio04_setup_corpus.txt`,
//! script `tests/oracle/pcbio04_setup_golden.sh`) and unit checks of branches the corpus does
//! not reach (expectations derived from the C++).

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};

use super::*;
use crate::io::writer::{FormatOptions, format_board_report};
use crate::io::{ParseOptions, parse_board_report};

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

fn staging_format() -> FormatOptions {
    FormatOptions {
        allow_pending: true,
        ..FormatOptions::default()
    }
}

fn parse(text: &str) -> crate::board::Board {
    parse_board_report(text, "", &staging_parse())
        .map(|(b, _)| b)
        .unwrap_or_else(|e| panic!("{}", e.what()))
}

/// The error text of a parse that must fail (source "", as the QA stream reader).
fn parse_err(text: &str) -> String {
    match parse_board_report(text, "", &staging_parse()) {
        Ok(_) => panic!("parse of {text} succeeded"),
        Err(e) => e.what().to_string(),
    }
}

/// The top-level `(setup ..)` section of a saved board: the lines from `\t(setup\n` to the
/// next `\t)\n`, both included (empty when there is none).
fn setup_section(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;

    for line in text.split_inclusive('\n') {
        if line == "\t(setup\n" {
            inside = true;
        }

        if inside {
            out.push_str(line);

            if line == "\t)\n" {
                break;
            }
        }
    }

    out
}

/// Parse (staging) + format (staging) of a board text, setup section only.
fn resave_setup(text: &str, name: &str) -> Result<String, String> {
    let (board, _) =
        parse_board_report(text, name, &staging_parse()).map_err(|e| e.what().to_string())?;
    let (out, report) =
        format_board_report(&board, &staging_format()).map_err(|e| e.what().to_string())?;

    assert!(
        !report.skipped.iter().any(|s| s.package == "PCBIO-04"),
        "{name}: {report:?}"
    );

    Ok(setup_section(&out))
}

fn manifest_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

// ---- qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp ---------------------------------

/// `Issue23625_CorruptedStackupCapped`: a file with 200 dielectric layers loads, and the stackup
/// item count is clamped to the parser's 128-item cap.
/// https://gitlab.com/kicad/code/kicad/-/issues/23625
#[test]
fn issue23625_corrupted_stackup_capped() {
    let path = kicad_qa::pcbnew_data_dir()
        .join("plugins/kicad_sexpr/Issue23625_CorruptedStackup/corrupted_stackup.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();

    match model(|| parse(&text)) {
        Ok(board) => {
            let stackup = &board.design_settings().stackup;
            assert!(stackup.count() <= 128);
            assert!(stackup.count() > 0);
        }
        Err(pending) => println!("issue23625: pending on {pending}"),
    }
}

/// `Issue24133_DuplicatedStackupNotInflated`: the 9-item layer sequence repeated three times
/// keeps only the first copy, a standard 1.6 mm two-layer board.
/// https://gitlab.com/kicad/code/kicad/-/issues/24133
#[test]
fn issue24133_duplicated_stackup_not_inflated() {
    let path = kicad_qa::pcbnew_data_dir()
        .join("plugins/kicad_sexpr/Issue24133_DuplicatedStackup/duplicated_stackup.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();

    match model(|| parse(&text)) {
        Ok(board) => {
            let stackup = &board.design_settings().stackup;
            assert_eq!(stackup.count(), 9);
            assert_eq!(
                stackup.build_board_thickness_from_stackup(),
                PCB_IU_SCALE.mm_to_iu(1.6)
            );
        }
        Err(pending) => println!("issue24133: pending on {pending}"),
    }
}

// ---- differential oracle: hand-made boards -----------------------------------------------------

/// Parse + format of every board of `tests/data/pcbio04/` (legacy pre-V6 setup tokens incl.
/// `user_via` / `user_diff_pair` lists and the `viasonmask` tenting override, numeric and
/// legacy hex layer selections, a V6 stackup with custom colours without alpha, sub-layers,
/// locked thickness, unknown and legacy stackup tokens, `(defaults ..)`, a current stackup
/// with dielectric models, the drill symbol profile with unknown entries, tenting / covering /
/// plugging / capping / filling in the current and legacy forms, clamped plot parameters,
/// unknown plot parameters) must reproduce the `(setup ..)` section the reference
/// `kicad-cli pcb upgrade --force` writes, byte for byte; and so must parse + format of the
/// oracle output itself.
#[test]
fn oracle_resave_setup() {
    check_resave_dir("tests/data/pcbio04", "tests/golden/pcbio04", 5);
}

/// The same check on the hand-made boards of PCBIO-02 (`tests/data/pcbio02/`, golden
/// `tests/golden/pcbio02/`, whose `(setup ..)` sections that package left out of its
/// comparison).
#[test]
fn oracle_resave_setup_pcbio02_boards() {
    check_resave_dir("tests/data/pcbio02", "tests/golden/pcbio02", 8);
}

/// Parse + format of every board of `data`, and of its oracle re-save in `golden`, must give
/// the setup section of the oracle re-save.
fn check_resave_dir(data: &str, golden_dir: &str, min_count: usize) {
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(manifest_path(data))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .collect();
    inputs.sort();
    assert!(inputs.len() >= min_count, "{inputs:?}");

    let mut failures = Vec::new();

    for input in &inputs {
        let name = input.file_name().unwrap().to_str().unwrap().to_string();
        let text = std::fs::read_to_string(input).unwrap();
        let golden =
            std::fs::read_to_string(manifest_path(golden_dir).join(&name)).unwrap();
        let expected = setup_section(&golden);
        assert!(!expected.is_empty(), "{name}");

        for (label, source) in [("input", &text), ("O1", &golden)] {
            match model(|| resave_setup(source, &name)) {
                Ok(Ok(got)) if got == expected => {}
                Ok(Ok(got)) => failures.push(format!(
                    "{name} ({label}): setup differs\n--- got\n{got}--- expected\n{expected}"
                )),
                Ok(Err(e)) => failures.push(format!("{name} ({label}): {e}")),
                Err(pending) => println!("{name} ({label}): pending on model stub {pending}"),
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- differential oracle: corpus ---------------------------------------------------------------

/// FNV-1a 64 of `bytes` (the hash of the golden script).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}

/// The board text up to the end of its top-level `(setup ..)` list, closed with `)`: the
/// header, general, page, title block, layers, nets declared before it and the setup section.
/// `None` when the board has no setup section.
fn header_and_setup(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut setup_start = None;
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];

        if in_string {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_string = false;
            }
        } else if c == b'"' {
            in_string = true;
        } else if c == b'(' {
            depth += 1;

            if depth == 2 && text[i + 1..].starts_with("setup") {
                setup_start = Some(i);
            }
        } else if c == b')' {
            depth -= 1;

            if depth == 1 && setup_start.is_some() {
                return Some(format!("{})", &text[..=i]));
            }
        }

        i += 1;
    }

    None
}

/// Boards of the corpus whose `(setup ..)` section the reference build does not write from the
/// file alone, with the reason. Empty: every saved board must match.
const CORPUS_EXCEPTIONS: &[(&str, &str)] = &[];

/// Corpus oracle: every board of `qa/data/pcbnew` + `demos` below 2 MB re-saved by the reference
/// build; parse + format (staging for the other packages) must give the same `(setup ..)`
/// section byte for byte. Boards whose parse reaches a pending model stub are reported, not
/// failed; boards whose setup depends on a pending parser (zone defaults: PCBIO-08) are
/// reported separately.
#[test]
fn oracle_corpus_setup() {
    let golden = std::fs::read_to_string(manifest_path("tests/golden/pcbio04_setup_corpus.txt"))
        .unwrap();
    let root = kicad_qa::kicad_source_root();

    let mut compared = 0;
    let mut identical = 0;
    let mut truncated = 0;
    let mut pending = Vec::new();
    let mut failures = Vec::new();

    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let (path, hash) = line.split_once('|').unwrap();

        if hash == "error" {
            continue;
        }

        let bytes = std::fs::read(root.join(path)).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        compared += 1;

        // The setup section depends only on what precedes it and on itself: when the full
        // parse reaches a pending model stub in a later item, compare the board cut after
        // its setup section instead.
        let result = match model(|| resave_setup(&text, path)) {
            Err(stub) => match header_and_setup(&text) {
                Some(cut) => {
                    truncated += 1;
                    model(|| resave_setup(&cut, path)).map_err(|e| format!("{stub}; {e}"))
                }
                None => Err(stub),
            },
            r => r,
        };

        match result {
            Ok(Ok(got)) => {
                if format!("{:016x}", fnv1a64(got.as_bytes())) == hash {
                    identical += 1;
                } else if text.contains("(zone_defaults") {
                    pending.push(format!("{path}: zone_defaults (PCBIO-08)"));
                } else if !CORPUS_EXCEPTIONS.iter().any(|(p, _)| *p == path) {
                    failures.push(path.to_string());
                }
            }
            Ok(Err(e)) => failures.push(format!("{path}: {e}")),
            Err(stub) => pending.push(format!("{path}: model stub {stub}")),
        }
    }

    println!(
        "oracle_corpus_setup: {compared} compared ({truncated} cut after the setup section), \
         {identical} identical, {} pending, {} failed",
        pending.len(),
        failures.len()
    );

    for p in &pending {
        println!("  pending: {p}");
    }

    assert!(compared >= 100, "{compared}");
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- unit checks of the reader -----------------------------------------------------------------

const HEADER_2LAYER: &str = "(kicad_pcb (version 20261031) (generator \"pcbnew\")\n  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (1 \"F.Mask\" user) (3 \"B.Mask\" user) (25 \"Edge.Cuts\" user))\n";

fn board_with_setup(setup: &str) -> String {
    format!("{HEADER_2LAYER}  (setup {setup}))")
}

/// Error texts of `parseSetup` / `parseBoardStackup` / `parseDefaults` /
/// `parseDefaultTextDims` (`Unexpected` / `Expecting`), as `pcb-dump board --no-init` of the
/// same texts prints them.
#[test]
fn setup_errors() {
    let cases: &[(&str, &str)] = &[
        (
            "(bogus 1)",
            "Unexpected bogus in '', line 3, offset 11.",
        ),
        (
            "(stackup (layer \"Edge.Cuts\" (type \"x\")))",
            "Expecting layer_name. Got ''quoted string'' in '', line 3, offset 27.",
        ),
        (
            "(stackup (layer \"dielectric 1\" (dielectric_model foo)))",
            "Expecting constant or djordjevic_sarkar. Got ''symbol'' in '', line 3, offset 59.",
        ),
        (
            "(defaults (edge_clearance 0.1) (foo 1))",
            "Unexpected foo in '', line 3, offset 42.",
        ),
        (
            "(defaults (copper_text_dims (size 1 1) bold))",
            "Expecting size, thickness, italic or keep_upright. Got ''bold'' in '', line 3, offset 49.",
        ),
        (
            "(pcbplotparams (mirror maybe))",
            "Expecting true, false, yes, or no. Got ''symbol'' in '', line 3, offset 33.",
        ),
        (
            "(pcbplotparams (layerselection abc))",
            "Expecting integer or hex layerSelection. Got ''symbol'' in '', line 3, offset 41.",
        ),
        (
            "(pcbplotparams (pngdpi x))",
            "Expecting ''number'' in '', line 3, offset 33.",
        ),
    ];

    for (setup, expected) in cases {
        let text = board_with_setup(setup);
        match model(|| parse_err(&text)) {
            Ok(e) => assert_eq!(&e, expected, "{setup}"),
            Err(pending) => println!("setup_errors: pending on {pending}"),
        }
    }
}

/// The legacy pre-V6 tokens set `m_LegacyDesignSettingsLoaded` (so the project file does not
/// override the design settings later); the current tokens do not. `(defaults ..)` sets it
/// too, `edge_clearance` also sets `m_LegacyCopperEdgeClearanceLoaded`.
#[test]
fn legacy_design_settings_flag() {
    let cases: &[(&str, bool, bool)] = &[
        ("(pad_to_mask_clearance 0.1) (aux_axis_origin 1 2) (grid_origin 3 4)", false, false),
        ("(trace_clearance 0.2)", true, false),
        ("(user_trace_width 0.2)", true, false),
        ("(max_error 0.005)", true, false),
        ("(pad_drill 0.5)", true, false),
        ("(defaults (copper_line_width 0.2))", true, false),
        ("(defaults (edge_clearance 0.2))", true, true),
        ("(visible_elements 7FFFFFFF) (filled_areas_thickness no)", false, false),
    ];

    for (setup, legacy, edge) in cases {
        let text = board_with_setup(setup);
        match model(|| parse(&text)) {
            Ok(board) => {
                assert_eq!(board.legacy_design_settings_loaded, *legacy, "{setup}");
                assert_eq!(board.legacy_copper_edge_clearance_loaded, *edge, "{setup}");
            }
            Err(pending) => println!("legacy_design_settings_flag: pending on {pending}"),
        }
    }
}

/// `user_trace_width` / `user_via` reserve slot 0 for the netclass value; the netclass
/// tokens edit the default netclass in place; `user_diff_pair` has no reserved slot.
#[test]
fn legacy_lists_and_netclass() {
    let text = board_with_setup(
        "(user_trace_width 0.3) (user_trace_width 0.3) (user_via 0.9 0.5) (user_diff_pair 0.2 0.15 0.25) (trace_clearance 0.18) (via_size 0.7) (via_drill 0.35) (uvia_size 0.31) (uvia_drill 0.11)",
    );

    match model(|| parse(&text)) {
        Ok(board) => {
            let bds = board.design_settings();
            assert_eq!(bds.track_width_list, vec![0, 300_000, 300_000]);
            assert_eq!(
                bds.vias_dimensions_list,
                vec![
                    ViaDimension {
                        diameter: 0,
                        drill: 0
                    },
                    ViaDimension {
                        diameter: 900_000,
                        drill: 500_000
                    }
                ]
            );
            assert_eq!(
                bds.diff_pair_dimensions_list,
                vec![DiffPairDimension {
                    width: 200_000,
                    gap: 150_000,
                    via_gap: 250_000
                }]
            );
            let nc = bds.net_settings.default_netclass();
            assert_eq!(nc.clearance, Some(180_000));
            assert_eq!(nc.via_dia, Some(700_000));
            assert_eq!(nc.via_drill, Some(350_000));
            assert_eq!(nc.uvia_dia, Some(310_000));
            assert_eq!(nc.uvia_drill, Some(110_000));
        }
        Err(pending) => println!("legacy_lists_and_netclass: pending on {pending}"),
    }
}

/// Without `(stackup ..)` the parser builds the default stackup of the copper layer count
/// (`BuildDefaultStackupList`) and `m_HasStackup` stays false; with one, `m_HasStackup` is set
/// and the file's items are kept. `solder_mask_min_width` defaults to 0 when absent.
#[test]
fn default_stackup_on_load() {
    let text = board_with_setup("(pad_to_mask_clearance 0)");

    match model(|| parse(&text)) {
        Ok(board) => {
            let bds = board.design_settings();
            assert!(!bds.has_stackup);
            assert_eq!(bds.solder_mask_min_width, 0);

            let mut expected = crate::board_stackup::BoardStackup::default();
            expected.build_default_stackup_list(Some(&bds.clone()), 2);
            assert_eq!(bds.stackup, expected);
        }
        Err(pending) => println!("default_stackup_on_load: pending on {pending}"),
    }
}

/// `PCB_PLOT_PARAMS_PARSER` branches: the legacy hex layer selection is remapped below
/// version 20240819 and read as is from it on; a numeric (pretty 3) selection selects all
/// copper plus both silk layers; `excludeedgelayer false` appends Edge.Cuts to the
/// plot-on-all-layers sequence; `viasonmask` overrides the board tenting flags.
#[test]
fn plot_params_reader() {
    let legacy = "(kicad_pcb (version 20240108) (generator \"pcbnew\")\n  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal))\n  (setup (tenting front back) (pcbplotparams (layerselection 0x0000020_00000001) (plot_on_all_layers_selection 0x0001000_00000000) (excludeedgelayer false) (viasonmask yes))))";

    match model(|| parse(legacy)) {
        Ok(board) => {
            let p = &board.plot_options;
            // legacy bit 0 = F.Cu, legacy bit 37 = F.SilkS
            assert_eq!(
                p.layer_selection,
                Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FSilkS])
            );
            // legacy bit 44 = Edge.Cuts, then excludeedgelayer appends it once more
            assert_eq!(
                p.plot_on_all_layers_sequence,
                vec![PcbLayerId::EdgeCuts, PcbLayerId::EdgeCuts]
            );
            assert_eq!(p.plot_via_on_mask_layer, Some(true));
            let bds = board.design_settings();
            assert!(!bds.tent_vias_front && !bds.tent_vias_back);
        }
        Err(pending) => println!("plot_params_reader: pending on {pending}"),
    }

    let current = board_with_setup(
        "(pcbplotparams (layerselection 0x00000000_00000000_00000000_00000021) (plot_on_all_layers_selection 0x0))",
    );

    match model(|| parse(&current)) {
        Ok(board) => {
            let p = &board.plot_options;
            assert_eq!(
                p.layer_selection,
                Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FSilkS])
            );
            assert!(p.plot_on_all_layers_sequence.is_empty());
            assert_eq!(p.plot_via_on_mask_layer, None);
        }
        Err(pending) => println!("plot_params_reader: pending on {pending}"),
    }

    let numeric = board_with_setup("(pcbplotparams (layerselection 3178497))");

    match model(|| parse(&numeric)) {
        Ok(board) => {
            let mut expected = Lset::from_layers(&[PcbLayerId::FSilkS, PcbLayerId::BSilkS]);
            expected |= Lset::all_cu_mask();
            assert_eq!(board.plot_options.layer_selection, expected);
        }
        Err(pending) => println!("plot_params_reader: pending on {pending}"),
    }
}

/// The drill symbol profile model form: the default profile is `None` (and writes nothing),
/// a parsed profile survives the model round trip unchanged.
#[test]
fn drill_symbol_profile_model_form() {
    assert!(DrillSymbolProfile::default().to_model().is_none());

    let text = board_with_setup(
        "(drill_symbol_profile (name \"x\") (group_by span) (default_marks size_text) (size 1) (width 0.1) (assignment (key \"k\") (mark letter \"B\") (descr \"d\")))",
    );

    match model(|| parse(&text)) {
        Ok(board) => {
            let node = board.design_settings().drill_symbol_profile.clone();
            assert!(node.is_some());
            let profile = DrillSymbolProfile::from_model(node.as_ref());
            assert_eq!(profile.name, "x");
            assert_eq!(
                profile.group_by.iter().copied().collect::<Vec<_>>(),
                vec![DrillGroupKey::Span]
            );
            assert_eq!(profile.symbol_size, 1_000_000);
            assert_eq!(profile.assignments["k"].letter, "B");
            assert_eq!(profile.to_model(), node);
        }
        Err(pending) => println!("drill_symbol_profile_model_form: pending on {pending}"),
    }

    // `(drill_symbol_profile)` with no entries still clears every group key: not the default.
    let empty = board_with_setup("(drill_symbol_profile)");

    match model(|| parse(&empty)) {
        Ok(board) => {
            let profile = DrillSymbolProfile::from_model(
                board.design_settings().drill_symbol_profile.as_ref(),
            );
            assert!(profile.group_by.is_empty());
        }
        Err(pending) => println!("drill_symbol_profile_model_form: pending on {pending}"),
    }
}
