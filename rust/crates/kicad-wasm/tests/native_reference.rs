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

//! Native half of the native-vs-wasm equality check, and the C++ check of `plotDemoGerber`.
//!
//! `native_reference_is_current` computes every implemented API call natively and compares the
//! result with `tests/golden/native_reference.json`; `web/test/smoke.mjs` makes the same calls
//! on the WebAssembly module and compares with the same file. So the file pins native == wasm.
//! `KICAD_WASM_UPDATE_GOLDEN=1` rewrites it (then re-run the smoke test).
//!
//! `demo_gerber_matches_cpp`: the Rust Gerber plotter output of `plotDemoGerber` equals the
//! real `GERBER_PLOTTER` running the same calls (`tests/oracle/regen_demo_gerber.sh`).

use kicad_session::{Session, TrackSpec, ViaSpec};
use kicad_wasm::api;
use serde_json::{Value, json};

/// Corpus files (relative to the KiCad source root) used for `prettify` and `tokenStats`.
const CORPUS: &[&str] = &[
    "qa/data/pcbnew/via_stacks.kicad_pcb",
    "qa/data/pcbnew/issue7241.kicad_pcb",
    "demos/pic_programmer/pic_programmer.kicad_pcb",
];

const STROKE_TEXTS: &[(&str, f64)] = &[
    ("KiCad", 1.0),
    ("Rust + WebAssembly", 1.5),
    ("R1 ~{RESET} x^{2}", 2.0),
    ("Multi\nline", 1.27),
];

const DEMO_DATE: &str = "2026-01-01 00:00:00";

/// Corpus boards the parser and writer support in full today: `loadBoard` + `nets`, `tracks`,
/// `save` (the save equals the reference `kicad-cli pcb upgrade --force` output, see
/// `loaded_board_save_matches_cpp`).
const LOADABLE: &[&str] = &["qa/data/pcbnew/issue10697.kicad_pcb"];

fn hashed(text: &str) -> Value {
    json!({ "length": text.len(), "fnv": format!("{:016x}", api::fnv1a64(text.as_bytes())) })
}

/// What `web/test/smoke.mjs` does on `newBoard('demo.kicad_pcb')`: a track and a via, the
/// track list, then the track removed and the board saved.
fn item_board() -> Value {
    let mut s = Session::new_empty("demo.kicad_pcb");
    let t = s
        .add_track(&TrackSpec {
            start: [0, 0],
            end: [1_000_000, 0],
            width: 250_000,
            layer: "F.Cu".to_string(),
            net: 0,
        })
        .unwrap();
    s.add_via(&ViaSpec {
        at: [1_000_000, 0],
        diameter: 600_000,
        drill: 300_000,
        net: 0,
    })
    .unwrap();
    let tracks = serde_json::to_value(s.tracks()).unwrap();
    assert!(s.remove(t).unwrap());
    let saved = s.save().unwrap();
    json!({ "nets": serde_json::to_value(s.nets()).unwrap(), "tracks": tracks,
            "save": hashed(&saved) })
}

fn loaded_boards() -> Vec<Value> {
    let root = kicad_qa::kicad_source_root();
    LOADABLE
        .iter()
        .map(|rel| {
            let text = std::fs::read_to_string(root.join(rel)).expect("corpus file");
            let name = rel.rsplit('/').next().unwrap();
            let mut s = Session::load_board(&text, name, None, None).expect("loads");
            let saved = s.save().expect("saves");
            json!({ "file": rel, "name": name,
                    "nets": serde_json::to_value(s.nets()).unwrap(),
                    "tracks": serde_json::to_value(s.tracks()).unwrap(),
                    "save": hashed(&saved) })
        })
        .collect()
}

fn square(x0: i32, y0: i32, s: i32) -> Vec<[i32; 2]> {
    vec![[x0, y0], [x0 + s, y0], [x0 + s, y0 + s], [x0, y0 + s]]
}

fn boolean_cases() -> Vec<(Vec<api::JsPolygon>, Vec<api::JsPolygon>, &'static str)> {
    let a = vec![vec![square(0, 0, 10_000_000)]];
    let b = vec![vec![square(5_000_000, 5_000_000, 10_000_000)]];
    let ring = vec![vec![
        square(-20_000_000, -20_000_000, 40_000_000),
        square(-10_000_000, -10_000_000, 20_000_000),
    ]];
    let tri = vec![vec![vec![
        [0, -30_000_000],
        [26_000_000, 15_000_000],
        [-26_000_000, 15_000_000],
    ]]];

    let mut cases = Vec::new();

    for op in ["union", "difference", "intersection", "xor"] {
        cases.push((a.clone(), b.clone(), op));
        cases.push((ring.clone(), tri.clone(), op));
    }

    cases
}

fn reference() -> Value {
    let root = kicad_qa::kicad_source_root();
    let mut prettify = Vec::new();
    let mut token_stats = Vec::new();

    for rel in CORPUS {
        let text = std::fs::read_to_string(root.join(rel)).expect("corpus file");
        let pretty = api::prettify(&text);
        prettify.push(json!({
            "file": rel,
            "length": pretty.len(),
            "fnv": format!("{:016x}", api::fnv1a64(pretty.as_bytes())),
        }));
        token_stats.push(json!({
            "file": rel,
            "stats": serde_json::to_value(api::token_stats(&text, rel).expect("lexes")).unwrap(),
        }));
    }

    let stroke: Vec<Value> = STROKE_TEXTS
        .iter()
        .map(|(t, s)| {
            json!({ "text": t, "sizeMm": s,
                    "result": serde_json::to_value(api::stroke_text(t, *s)).unwrap() })
        })
        .collect();

    let booleans: Vec<Value> = boolean_cases()
        .into_iter()
        .map(|(a, b, op)| {
            let r = api::polygon_boolean(&a, &b, op).unwrap();
            json!({ "a": a, "b": b, "op": op, "result": r })
        })
        .collect();

    json!({
        "version": api::version(),
        "versionInfo": serde_json::to_value(api::version_info()).unwrap(),
        "prettify": prettify,
        "tokenStats": token_stats,
        "strokeText": stroke,
        "polygonBoolean": booleans,
        "demoGerber": {
            "date": DEMO_DATE,
            "text": api::plot_demo_gerber(kicad_session::build_version::build_version(), DEMO_DATE),
        },
        "itemBoard": item_board(),
        "loadBoard": loaded_boards(),
    })
}

#[test]
fn native_reference_is_current() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/native_reference.json");
    let current = serde_json::to_string(&reference()).unwrap() + "\n";

    if std::env::var_os("KICAD_WASM_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &current).unwrap();
        return;
    }

    let golden = std::fs::read_to_string(&path)
        .expect("tests/golden/native_reference.json missing: run with KICAD_WASM_UPDATE_GOLDEN=1");
    assert!(
        golden == current,
        "native results differ from tests/golden/native_reference.json (re-run with \
         KICAD_WASM_UPDATE_GOLDEN=1 after checking the change, then web/test/smoke.mjs)"
    );
}

#[test]
fn demo_gerber_matches_cpp() {
    let golden = include_str!("golden/demo_gerber_cpp.gbr");
    assert_eq!(api::plot_demo_gerber("@VERSION@", "@DATE@"), golden);
}

#[test]
fn loaded_board_save_matches_cpp() {
    // `board.save()` of a loaded board is `PCB_IO_KICAD_SEXPR::SaveBoard`: the reference
    // `kicad-cli pcb upgrade --force` output (kicad-session's oracle golden).
    let root = kicad_qa::kicad_source_root();
    let goldens = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kicad-session/tests/golden/upgrade");

    for rel in LOADABLE {
        let name = rel.rsplit('/').next().unwrap();
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        let mut s = Session::load_board(&text, name, None, None).unwrap();
        let golden = std::fs::read_to_string(goldens.join(name)).unwrap();
        assert!(s.save().unwrap() == golden, "{rel}");
    }
}

#[test]
fn demo_gerber_script_matches_oracle_input() {
    // The script the C++ oracle ran is the one documented in the API.
    let script = include_str!("oracle/demo_gerber_script.txt");
    assert_eq!(script, api::DEMO_GERBER_SCRIPT);
}

#[test]
fn token_stats_counts() {
    let s = api::token_stats("(kicad_pcb (version 20240108) (net 1 \"GND\") foo)", "x").unwrap();
    assert_eq!(s.lists, 3);
    assert_eq!(s.closes, 3);
    assert_eq!(s.keywords, 3);
    assert_eq!(s.numbers, 2);
    assert_eq!(s.strings, 1);
    assert_eq!(s.symbols, 1);
    assert_eq!(s.max_depth, 2);
    assert!(api::token_stats("(a \"unterminated", "x").is_err());
}

#[test]
fn polygon_boolean_area() {
    let a = vec![vec![square(0, 0, 10)]];
    let b = vec![vec![square(5, 5, 10)]];
    let i = api::polygon_boolean(&a, &b, "intersection").unwrap();
    assert_eq!(i.len(), 1);
    assert_eq!(i[0][0].len(), 4);
    assert!(api::polygon_boolean(&a, &b, "nope").is_err());
}
