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

//! Tests of the PCBIO-01 parser core: helper semantics, error cases (message texts taken from
//! the reference build, `pcb-dump board --no-init`), the corpus (every board of
//! `qa/data/pcbnew` and `demos` parses with `allow_pending`), the parts of
//! `qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp` that concern the skeleton, and
//! the differential oracle `oracle_board_section` (golden written by
//! `tests/oracle/pcbio01_board_golden.sh`).
//!
//! Model functions that are still `port_pending!` stubs panic. The tests run the model-
//! dependent parts under [`model`], which turns exactly those panics into a "pending" result
//! (printed, not a failure); every other panic and every mismatch fails. Once the model
//! packages land the same tests check everything.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::{Lset, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::ShapeLineChain;
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::*;
use crate::board::{Board, LayerT};
use crate::io::{parse_board, parse_board_report};

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

fn staging() -> ParseOptions {
    ParseOptions {
        allow_pending: true,
    }
}

fn strict() -> ParseOptions {
    ParseOptions::default()
}

/// A parser over `text` (source "") positioned before the first token.
fn parser(text: &str) -> PcbParser {
    PcbParser::new(text, "", Board::new(), true, strict())
}

/// The error text of a parse of `text` that must fail (source "", as the QA stream reader).
fn parse_error(text: &str, options: &ParseOptions) -> Result<String, String> {
    model(|| match parse_board(text, "", options) {
        Ok(_) => panic!("parse of {text:?} succeeded"),
        Err(e) => e.what().to_string(),
    })
}

// ---- helpers --------------------------------------------------------------------------------

/// `parseBoardUnits`: mm -> IU with `KiROUND`, clamped to `INT_LIMIT`; scientific notation
/// (`ScientificNotationLoading`: `(mid 4.17 -4.5e-05)` -> 4170000, -45).
#[test]
fn board_units() {
    let mut p = parser("4.17 -4.5e-05 1e30 -1e30 0.0000005 -0.0000005 1.0000004 2.5e-6 x");
    let mut values = Vec::new();

    for _ in 0..8 {
        p.lexer.next_tok().unwrap();
        values.push(p.parse_board_units().unwrap());
    }

    assert_eq!(
        values,
        [
            4_170_000,
            -45,
            2_147_483_637,
            -2_147_483_637,
            1,
            -1,
            1_000_000,
            3
        ]
    );

    // A symbol is not a number.
    p.lexer.next_tok().unwrap();
    assert!(p.parse_board_units().is_err());

    // parseBoardUnits( "expected" ) needs a number token.
    let mut p = parser("(width abc)");
    p.lexer.next_tok().unwrap();
    p.lexer.next_tok().unwrap();
    let e = p.parse_board_units_str("width").unwrap_err();
    assert_eq!(
        e.what(),
        "need a number for 'width' in '', line 1, offset 8."
    );
}

/// `parseMaybeAbsentBool`: `hide`, `(hide)`, `(hide yes)`, `(hide false)`.
#[test]
fn maybe_absent_bool() {
    // "(hide)": the default, ")" consumed.
    let mut p = parser("(hide) (hide yes) (hide false) hide x (hide maybe)");
    for (default, expected) in [(true, true), (false, true), (true, false)] {
        p.lexer.next_tok().unwrap(); // (
        p.lexer.next_tok().unwrap(); // hide
        assert_eq!(p.parse_maybe_absent_bool(default).unwrap(), expected);
        assert_eq!(p.lexer.cur_tok(), T::T_RIGHT);
    }

    // bare "hide": the default, nothing consumed.
    p.lexer.next_tok().unwrap();
    assert!(!p.parse_maybe_absent_bool(false).unwrap());
    assert_eq!(p.lexer.cur_tok(), T::T_hide);
    p.lexer.next_tok().unwrap();
    assert_eq!(p.lexer.cur_text(), "x");

    p.lexer.next_tok().unwrap();
    p.lexer.next_tok().unwrap();
    let e = p.parse_maybe_absent_bool(true).unwrap_err();
    assert_eq!(
        e.what(),
        "Expecting yes or no. Got ''symbol'' in '', line 1, offset 45."
    );
}

/// `parseBool`, `parseOptBool`, `parseFrontBackOptBool` (new and legacy forms).
#[test]
fn bools() {
    let mut p = parser("yes no none (front yes) (back none)) front back) none) (front x)");
    assert!(p.parse_bool().unwrap());
    assert_eq!(p.parse_opt_bool().unwrap(), Some(false));
    assert_eq!(p.parse_opt_bool().unwrap(), None);
    assert_eq!(
        p.parse_front_back_opt_bool(false).unwrap(),
        (Some(true), None)
    );
    assert_eq!(
        p.parse_front_back_opt_bool(true).unwrap(),
        (Some(true), Some(true))
    );
    assert_eq!(p.parse_front_back_opt_bool(true).unwrap(), (None, None));
    assert!(p.parse_front_back_opt_bool(true).is_err());
}

/// `CurStrToKIID`: legacy time stamps, quoted strings (the C++ keeps the trailing quote, which
/// makes the string invalid: a random id), plain UUIDs.
#[test]
fn kiids() {
    let mut p = parser("5C8A7E5B 0f0d2c6c-1e1e-4c8b-9a0f-4f1b8e3a5d21 \"\\\"abc\\\"\"");
    p.lexer.next_tok().unwrap();
    assert_eq!(
        p.cur_str_to_kiid().as_string(),
        "00000000-0000-0000-0000-00005c8a7e5b"
    );
    p.lexer.next_tok().unwrap();
    assert_eq!(
        p.cur_str_to_kiid().as_string(),
        "0f0d2c6c-1e1e-4c8b-9a0f-4f1b8e3a5d21"
    );
    p.lexer.next_tok().unwrap();
    assert_eq!(p.lexer.cur_str(), "\"abc\"");
    let random = p.cur_str_to_kiid();
    assert_ne!(random, kicad_core::NILUUID);
}

/// `GetRequiredVersion`: `wxDateTime::FormatDate()` in the C locale (oracle: 29990101 ->
/// "01/01/99"), and the "Cannot interpret date code" errors.
#[test]
fn required_version() {
    let mut p = parser("");
    for (v, s) in [
        (29990101, "01/01/99"),
        (20240229, "02/29/24"),
        (20261231, "12/31/26"),
    ] {
        p.required_version = v;
        assert_eq!(p.get_required_version().unwrap(), s);
    }

    for v in [20230229, 99999999, 20261300, 20260100, 20260431, 0] {
        p.required_version = v;
        let e = p.get_required_version().unwrap_err();
        assert!(
            e.what()
                .starts_with(&format!("Cannot interpret date code {v} in ''"))
        );
    }
}

/// `lookUpLayer` / `lookUpLayerSet` / `parseBoardItemLayersAsMask` /
/// `parseLayersForCuItemWithSoldermask` on the maps seeded by `init()`.
#[test]
fn layer_lookups() {
    let mut p = parser(
        "F.Cu Foo Rescue UNDEFINED (layers *.Cu F.SilkS Bar) (layers F.Cu B.Mask) (layers In1.Cu F.Mask) (layers B.Cu B.Mask) Inner1.Cu",
    );
    p.lexer.next_tok().unwrap();
    assert_eq!(p.look_up_layer(false), PcbLayerId::FCu);
    p.lexer.next_tok().unwrap();
    assert_eq!(p.look_up_layer(false), PcbLayerId::Rescue);
    p.lexer.next_tok().unwrap();
    assert_eq!(p.look_up_layer(false), PcbLayerId::Rescue);
    p.lexer.next_tok().unwrap();
    assert_eq!(p.look_up_layer(true), PcbLayerId::UndefinedLayer);
    assert_eq!(
        p.undefined_layers.iter().cloned().collect::<Vec<_>>(),
        ["Foo", "Rescue"]
    );

    p.lexer.next_tok().unwrap();
    p.lexer.next_tok().unwrap();
    let mask = p.parse_board_item_layers_as_mask().unwrap();
    let mut expected = Lset::all_cu_mask().clone();
    expected.set(PcbLayerId::FSilkS);
    expected.set(PcbLayerId::Rescue);
    assert_eq!(mask, expected);
    // An unknown name inside (layers ...) is not recorded.
    assert_eq!(p.undefined_layers.len(), 2);

    for ok in [false, false, true] {
        p.lexer.next_tok().unwrap();
        p.lexer.next_tok().unwrap();
        // The whole list is read before the checks.
        assert_eq!(p.parse_layers_for_cu_item_with_soldermask().is_ok(), ok);
        assert_eq!(p.lexer.cur_tok(), T::T_RIGHT);
    }

    p.lexer.next_tok().unwrap();
    // Inner1.Cu = In15_Cu - 2 = In14_Cu (the reversed legacy numbering).
    assert_eq!(
        p.look_up_layer_set(),
        Lset::from_layers(&[PcbLayerId::In14Cu])
    );
}

/// `parseXY` (with and without the `(` read) and `parseOutlinePoints` (`xy` and `arc` in any
/// sub-token order, missing parts rejected).
#[test]
fn outline_points() {
    let mut p = parser(
        "(xy 1 -2) (xy 3 4) (arc (end 2 0) (start 0 0) (mid 1 1)) (arc (start 0 0) (end 1 0)) (pts 1)",
    );
    assert_eq!(p.parse_xy().unwrap(), Vector2I::new(1_000_000, -2_000_000));
    p.lexer.next_tok().unwrap();
    assert_eq!(p.parse_xy().unwrap(), Vector2I::new(3_000_000, 4_000_000));

    let mut chain = ShapeLineChain::new();
    p.parse_outline_points(&mut chain).unwrap();
    assert_eq!(chain.arc_count(), 1);
    assert_eq!(chain.c_point(0), Vector2I::new(0, 0));
    assert_eq!(chain.c_last_point(), Vector2I::new(2_000_000, 0));

    let e = p.parse_outline_points(&mut chain).unwrap_err();
    assert_eq!(
        e.what(),
        "Expecting mid. Got '')'' in '', line 1, offset 84."
    );
    let e = p.parse_outline_points(&mut chain).unwrap_err();
    assert_eq!(
        e.what(),
        "Expecting xy or arc. Got ''pts'' in '', line 1, offset 87."
    );
}

/// `skipCurrent`, `pushValueIntoMap` / `getNetCode`, `parseBoardProperty`, both
/// `parseCustomProperty` forms.
#[test]
fn misc_helpers() {
    let mut p = parser("(a (b (c)) d) e \"k\" \"v\") \"K\" \"w\") \"Key\" \"x\")");
    p.lexer.next_tok().unwrap();
    p.lexer.next_tok().unwrap();
    p.skip_current().unwrap();
    p.lexer.next_tok().unwrap();
    assert_eq!(p.lexer.cur_text(), "e");

    assert_eq!(p.parse_board_property().unwrap(), ("k".into(), "v".into()));

    let mut props = BTreeMap::new();
    p.parse_custom_property_map(&mut props).unwrap();
    assert_eq!(props["K"], "w");

    let mut shape = crate::pcb_shape::PcbShape::default();
    shape.base_mut().set_custom_property("key", "old");
    p.parse_custom_property_item(&mut shape).unwrap();
    // SetCustomProperty matches the key case-insensitively and keeps the stored key.
    assert_eq!(
        shape.base().custom_properties.iter().collect::<Vec<_>>(),
        [(&"key".to_string(), &"x".to_string())]
    );

    p.push_value_into_map(3, 7);
    assert_eq!(p.net_codes, [0, 0, 0, 7]);
    assert_eq!(p.get_net_code(3), 7);
    assert_eq!(p.get_net_code(1), 0);
    assert_eq!(p.get_net_code(9), 9);
    assert_eq!(p.get_net_code(-1), -1);
}

/// The layer maps of `init()`: 128 canonical names, the wildcards, `Inner1.Cu`..`Inner14.Cu`.
#[test]
fn init_maps() {
    let p = parser("");
    assert_eq!(p.layer_indices.len(), PCB_LAYER_ID_COUNT as usize);
    assert_eq!(p.layer_indices["User.45"], PcbLayerId::User45);
    assert_eq!(p.layer_masks["*.Cu"], *Lset::all_cu_mask());
    assert_eq!(p.layer_masks["*In.Cu"], *Lset::internal_cu_mask());
    assert_eq!(
        p.layer_masks["Inner1.Cu"],
        Lset::from_layers(&[PcbLayerId::In14Cu])
    );
    assert_eq!(
        p.layer_masks["Inner14.Cu"],
        Lset::from_layers(&[PcbLayerId(PcbLayerId::In15Cu.0 - 28)])
    );
}

/// The staging stubs: an error without `allow_pending`, a recorded skip with it.
#[test]
fn staging_stubs() {
    let text = "(kicad_pcb (version 20240101) (generator \"pcbnew\")\n  (segment (start 0 0) (end 1 (x)) (net 1))\n  (bogus))";
    let e = parse_error(text, &strict()).unwrap();
    assert_eq!(e, "port pending: segment (PCBIO-03)");

    let e = parse_error(text, &staging()).unwrap();
    assert_eq!(e, "Unknown token 'bogus' in '', line 3, offset 4.");
}

// ---- errors (texts of the reference build) --------------------------------------------------

/// Parse errors before any model call; texts from `pcb-dump board --no-init` of the same files.
#[test]
fn error_messages() {
    let cases: &[(&str, &str)] = &[
        (
            "(kicad_pcb (version 99999999) (foo))",
            "Cannot interpret date code 99999999 in '', line 1, offset 32.",
        ),
        (
            "(kicad_pcb (version 29990101) (foo))",
            "KiCad was unable to open this file because it was created with a more recent version \
             than the one you are running.\n\nTo open it you will need to upgrade KiCad to a \
             version dated 01/01/99 or later.\n\nFull error text:\nUnknown token 'foo' in '', \
             line 1, offset 32.",
        ),
        (
            "(kicad_pcb (version 29990101) (generator \"pcbnew\") (generator_version \"12.0\") (general (thickness 1.6)))",
            "KiCad was unable to open this file because it was created with a more recent version \
             than the one you are running.\n\nTo open it you will need to upgrade KiCad to version \
             12.0 or later (file format dated 29990101 or later).",
        ),
        (
            "(kicad_pcb (version 29990101) (general (thickness 1.6)))",
            "KiCad was unable to open this file because it was created with a more recent version \
             than the one you are running.\n\nTo open it you will need to upgrade KiCad to a \
             version dated 29990101 or later.",
        ),
        (
            "(kicad_pcb (version 20240101) (paper \"A9\"))",
            "Page type 'A9' is not valid. in '', line 1, offset 39.",
        ),
        (
            "(kicad_pcb (version 20240101) (title_block (comment 10 \"x\")))",
            "10 is not a valid title block comment number in '', line 1, offset 53.",
        ),
        (
            "(kicad_pcb (version 20240101) (bogus 1))",
            "Unknown token 'bogus' in '', line 1, offset 32.",
        ),
        ("(foo)", "Unknown token 'foo' in '', line 1, offset 2."),
        ("kicad_pcb", "Expecting ''('' in '', line 1, offset 1."),
        // Not oracle-checked: the reference QA load path does not terminate on empty input.
        ("", "Unexpected ''end of input'' in '', line 1, offset 1."),
        (
            "(kicad_pcb (version 20240101) (general (links (3))))",
            "Expecting symbol or number. Got ''('' in '', line 1, offset 47.",
        ),
    ];

    for (text, expected) in cases {
        assert_eq!(parse_error(text, &strict()).unwrap(), *expected, "{text}");
    }
}

/// Errors whose path calls the layer model (`SetLayerDescr`).
#[test]
fn layer_errors() {
    let cases: &[(&str, &str)] = &[
        (
            "(kicad_pcb (version 20240101) (generator \"pcbnew\") (layers (0 \"F.Cu\" signal)))",
            "1 is not a valid layer count in '', line 1, offset 77.",
        ),
        (
            "(kicad_pcb (version 20240101) (generator \"pcbnew\") (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Foo.Bar\" user)))",
            "Layer 'Foo.Bar' in file '' at line 1 is not in fixed layer hash.",
        ),
    ];

    for (text, expected) in cases {
        match parse_error(text, &strict()) {
            Ok(e) => assert_eq!(e, *expected, "{text}"),
            Err(pending) => println!("layer_errors: pending on {pending}"),
        }
    }
}

/// A footprint file is not a board (`DoLoad`: "This file does not contain a PCB."); with the
/// footprint parser still a stub the skipped footprint gives the same result.
#[test]
fn footprint_is_not_a_board() {
    match parse_error("(footprint \"R\" (version 20240101))", &staging()) {
        Ok(e) => assert_eq!(
            e,
            "This file does not contain a PCB. in '', line 1, offset 34."
        ),
        Err(pending) => println!("footprint_is_not_a_board: pending on {pending}"),
    }
}

// ---- corpus ---------------------------------------------------------------------------------

/// The four very large boards of the corpus (20 MB and more) are run by `corpus_large`.
const LARGE_BYTES: u64 = 20_000_000;

/// Every `.kicad_pcb` under `qa/data/pcbnew` and `demos`, sorted.
fn corpus() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "kicad_pcb") {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(&kicad_qa::pcbnew_data_dir(), &mut files);
    walk(&kicad_qa::demos_dir(), &mut files);
    files.sort();
    files
}

fn is_large(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() >= LARGE_BYTES)
}

/// Parse `files` with `allow_pending`; the only board the oracle rejects is
/// `plugins/fakeboard.kicad_pcb` ("Expecting ''('' ..."). Returns (parsed, pending).
fn run_corpus(files: &[PathBuf]) -> (usize, usize) {
    let (mut parsed, mut pending) = (0, 0);

    for path in files {
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let name = path.file_name().unwrap().to_string_lossy();

        match model(|| parse_board_report(&text, "", &staging())) {
            Err(stub) => {
                pending += 1;
                println!("corpus: {} pending on {stub}", path.display());
            }
            Ok(Err(e)) => {
                assert_eq!(
                    name,
                    "fakeboard.kicad_pcb",
                    "{}: {}",
                    path.display(),
                    e.what()
                );
                assert_eq!(e.what(), "Expecting ''('' in '', line 1, offset 1.");
                parsed += 1;
            }
            Ok(Ok((board, report))) => {
                assert_ne!(name, "fakeboard.kicad_pcb");
                assert!(board.copper_layer_count() >= 2);
                let _ = report;
                parsed += 1;
            }
        }
    }

    (parsed, pending)
}

/// The header, general, page, title block, layers and net declarations of every corpus board
/// (except the four largest) parse with the other constructs skipped.
#[test]
fn corpus_boards() {
    let files: Vec<PathBuf> = corpus().into_iter().filter(|p| !is_large(p)).collect();
    assert!(files.len() >= 360, "corpus has {} boards", files.len());
    let (parsed, pending) = run_corpus(&files);
    println!("corpus_boards: {parsed} parsed, {pending} pending on model stubs");
}

/// `corpus_boards` for the four very large boards.
#[test]
#[ignore = "parses ~230 MB of boards"]
fn corpus_large() {
    let files: Vec<PathBuf> = corpus().into_iter().filter(|p| is_large(p)).collect();
    assert_eq!(files.len(), 4);
    let (parsed, pending) = run_corpus(&files);
    println!("corpus_large: {parsed} parsed, {pending} pending on model stubs");
}

/// `test_kicad_sexpr.cpp` `ScientificNotationLoading`: the file contains 1 arc with
/// scientific notation in its coordinates; its midpoint is (4.17, -4.5e-05) in file units.
/// The arc is a `gr_arc` (PCBIO-03): until that package lands only the parse with staging is
/// checked (the coordinate conversion itself is `board_units`).
#[test]
fn scientific_notation_loading() {
    let path = kicad_qa::pcbnew_data_dir().join("plugins/kicad_sexpr/ScientificNotation.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();

    match model(|| parse_board(&text, "", &strict())) {
        Ok(Ok(board)) => {
            let drawings: Vec<_> = board.drawings().collect();
            assert_eq!(drawings.len(), 1);
            let crate::item_ref::AnyItem::Shape(arc) = drawings[0].1 else {
                panic!("not a shape");
            };
            // arc->GetArcMid() == ( 4170000, -45 ): no model API yet.
            crate::port_pending!("EDA_SHAPE::GetArcMid", arc);
        }
        Ok(Err(e)) => {
            assert!(e.what().starts_with("port pending: "), "{}", e.what());
            assert!(
                model(|| parse_board(&text, "", &staging()).map(|_| ()))
                    .map_or(true, |r| r.is_ok())
            );
        }
        Err(pending) => println!("scientific_notation_loading: pending on {pending}"),
    }
}

// ---- differential oracle ----------------------------------------------------------------------

/// `V( std::string )` of pcb-dump's `format.cpp` (byte-wise escapes of the UTF-8 text).
fn v_str(s: &str) -> String {
    let mut r: Vec<u8> = vec![b'"'];

    for &c in s.as_bytes() {
        match c {
            b'\\' => r.extend_from_slice(b"\\\\"),
            b'"' => r.extend_from_slice(b"\\\""),
            b'\n' => r.extend_from_slice(b"\\n"),
            b'\r' => r.extend_from_slice(b"\\r"),
            b'\t' => r.extend_from_slice(b"\\t"),
            c if c < 0x20 || c == 0x7f => r.extend_from_slice(format!("\\x{c:02x}").as_bytes()),
            _ => r.push(c),
        }
    }

    r.push(b'"');
    String::from_utf8_lossy(&r).into_owned()
}

/// `V( double )`: "0" for zero, else `%.17g`.
fn v_f64(v: f64) -> String {
    if v == 0.0 {
        "0".into()
    } else {
        kicad_sexpr::format_g(v, 17)
    }
}

/// `V( const LSET& )`: 128 bits as 32 hex digits, highest first.
fn v_lset(set: &Lset) -> String {
    (0..PCB_LAYER_ID_COUNT / 4)
        .rev()
        .map(|nib| {
            let v = (0..4).rev().fold(0, |v, b| {
                v * 2 + i32::from(set.contains(PcbLayerId(nib * 4 + b)))
            });
            char::from_digit(v as u32, 16).unwrap()
        })
        .collect()
}

/// `VEnum( LAYER_T )` (magic_enum names).
fn v_layer_t(t: LayerT) -> &'static str {
    match t {
        LayerT::Undefined => "LT_UNDEFINED",
        LayerT::Signal => "LT_SIGNAL",
        LayerT::Power => "LT_POWER",
        LayerT::Mixed => "LT_MIXED",
        LayerT::Jumper => "LT_JUMPER",
        LayerT::Aux => "LT_AUX",
        LayerT::Front => "LT_FRONT",
        LayerT::Back => "LT_BACK",
    }
}

/// The Rust value of one key of the golden board section (`DumpBoardSection`).
fn rust_value(board: &Board, key: &str) -> String {
    let layer_by_name = |name: &str| {
        (0..PCB_LAYER_ID_COUNT)
            .map(PcbLayerId)
            .find(|&l| Lset::name(l) == name)
            .unwrap_or_else(|| panic!("no layer {name}"))
    };

    let tb = &board.titles;

    match key {
        "file_format_version" => board.file_format_version_at_load.to_string(),
        "generator" => v_str(&board.generator),
        "copper_layer_count" => board.copper_layer_count().to_string(),
        "enabled_layers" => v_lset(&board.enabled_layers()),
        "page.type" => v_str(board.paper.type_as_string()),
        "page.size_mils" => format!(
            "{},{}",
            v_f64(board.paper.width_mils()),
            v_f64(board.paper.height_mils())
        ),
        "page.portrait" => board.paper.is_portrait().to_string(),
        "title_block.title" => v_str(tb.title()),
        "title_block.date" => v_str(tb.date()),
        "title_block.revision" => v_str(tb.revision()),
        "title_block.company" => v_str(tb.company()),
        _ => {
            if let Some(i) = key.strip_prefix("title_block.comment.") {
                v_str(tb.comment(i.parse().unwrap()))
            } else if let Some(k) = key.strip_prefix("property.") {
                board
                    .properties
                    .get(k)
                    .map_or("<absent>".into(), |v| v_str(v))
            } else if let Some(rest) = key.strip_prefix("layer.") {
                if let Some(name) = rest.strip_suffix(".name") {
                    v_str(&board.layer_name(layer_by_name(name)))
                } else {
                    let name = rest.strip_suffix(".type").unwrap();
                    v_layer_t(board.layer_type(layer_by_name(name))).into()
                }
            } else if let Some(rest) = key.strip_prefix("net.") {
                let code: i32 = rest.strip_suffix(".name").unwrap().parse().unwrap();
                board
                    .find_net(crate::ids::NetCode(code))
                    .map_or("<absent>".into(), |n| v_str(&n.netname))
            } else {
                panic!("unexpected golden key {key}")
            }
        }
    }
}

/// The differential oracle: the board-skeleton fields of `pcb-dump board --no-init` (the C++
/// parser) for 63 corpus boards (one per file format version) and the hand-made boards in
/// `tests/data/pcbio01`, compared with the Rust parse (`allow_pending`). Fields computed by
/// model stubs count as pending; a net that the Rust board lacks while items were skipped is
/// pending (nets of new-format files come from the items); everything else must match.
#[test]
fn oracle_board_section() {
    let golden = include_str!("../../../tests/golden/pcbio01_board_section.txt");
    let root = kicad_qa::kicad_source_root();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pcbio01");

    let mut boards: Vec<(String, Vec<(String, String)>)> = Vec::new();

    for line in golden.lines() {
        if let Some(file) = line.strip_prefix("file|") {
            boards.push((file.to_string(), Vec::new()));
        } else {
            let (k, v) = line.split_once('=').unwrap();
            boards
                .last_mut()
                .unwrap()
                .1
                .push((k.to_string(), v.to_string()));
        }
    }

    let (mut matched, mut pending, mut mismatches) = (0usize, 0usize, Vec::new());

    for (file, fields) in &boards {
        let path = match file.strip_prefix("inline:") {
            Some(name) => data.join(name),
            None => root.join(file),
        };
        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();

        let result = match model(|| parse_board_report(&text, "", &staging())) {
            Ok(r) => r,
            Err(stub) => {
                println!("oracle: {file}: parse pending on {stub}");
                pending += fields.len();
                continue;
            }
        };

        let (board, report) = match result {
            Ok(r) => r,
            Err(e) => {
                let expected = fields
                    .iter()
                    .find(|(k, _)| k == "error")
                    .map(|(_, v)| v.clone());
                let got = e.what().replace(['\n', '\r'], " ");

                if expected.as_deref() == Some(got.as_str()) {
                    matched += 1;
                } else {
                    mismatches.push(format!("{file}: error {got:?}, oracle {expected:?}"));
                }

                continue;
            }
        };

        for (key, expected) in fields {
            match model(|| rust_value(&board, key)) {
                Err(_) => pending += 1,
                Ok(got) if &got == expected => matched += 1,
                Ok(got)
                    if key.starts_with("net.")
                        && got == "<absent>"
                        && !report.skipped.is_empty() =>
                {
                    pending += 1
                }
                Ok(got) => mismatches.push(format!("{file}: {key}: rust {got}, oracle {expected}")),
            }
        }

        // No property the C++ does not have.
        for k in board.properties.keys() {
            if !fields
                .iter()
                .any(|(key, _)| key == &format!("property.{k}"))
            {
                mismatches.push(format!("{file}: extra property {k}"));
            }
        }
    }

    println!(
        "oracle_board_section: {} boards, {matched} fields match, {pending} pending, {} mismatch",
        boards.len(),
        mismatches.len()
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// `ok_misc`: the parser state after a full skeleton (net code mapping of the legacy
/// declarations: `(net 5 "VCC")` gets the next free code, a duplicate name maps to the existing
/// net; first property value wins; legacy bare `legacy_teardrops`).
#[test]
fn skeleton_state() {
    let text = include_str!("../../../tests/data/pcbio01/ok_misc.kicad_pcb");
    let mut p = PcbParser::new(text, "", Board::new(), true, staging());

    match model(|| p.parse()) {
        Ok(Ok(ParsedItem::Board)) => {
            assert_eq!(p.net_codes, [0, 1, 1, 0, 0, 2, 0, 3]);
            assert_eq!(p.board.properties["b"], "2");
            assert!(p.board.legacy_teardrops);
            assert_eq!(p.board.design_settings().board_thickness, 1_600_000);
            assert!(p.report.skipped.is_empty());
        }
        Ok(other) => panic!("{other:?}"),
        Err(pending) => println!("skeleton_state: pending on {pending}"),
    }

    // IoError is the C++ exception hierarchy.
    let _: Option<IoError> = None;
}
