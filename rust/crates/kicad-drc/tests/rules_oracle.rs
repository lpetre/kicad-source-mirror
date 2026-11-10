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

//! Differential oracle of the rule parser: replays the goldens written by
//! `tests/oracle/gen_rules_goldens.py` from the C++ `pcb-dump rules` command (the real
//! `DRC_RULES_PARSER`, `rust/tools/pcb-dump/dump_rules.cpp`) and compares them with the Rust
//! twin of that dump (`tests/common/mod.rs`).
//!
//! * `corpus`: every `.kicad_dru` under `<kicad>/qa` and `<kicad>/demos`;
//! * `broken_files`: `tests/data/rules/*.kicad_dru`, one per error message / branch;
//! * `component_class_rules`: `tests/data/rules/cc/*.kicad_dru`.
//!
//! Each in both parser modes (with a reporter, and without one, i.e. throwing).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

/// Golden file -> map of `file|<name>|` to its lines.
fn read_golden(name: &str) -> BTreeMap<String, Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("golden file");
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = None;

    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }

        if let Some(file) = line.strip_prefix("file|").and_then(|l| l.strip_suffix('|')) {
            current = Some(file.to_string());
            map.insert(file.to_string(), Vec::new());
        } else if let Some(file) = &current {
            map.get_mut(file).unwrap().push(line.to_string());
        }
    }

    map
}

fn read_text(path: &Path) -> String {
    // wxString::FromUTF8: empty for invalid UTF-8
    String::from_utf8(std::fs::read(path).expect("rule file")).unwrap_or_default()
}

/// Compare every golden entry; `make` produces the Rust lines of one file.
fn check(
    golden: &BTreeMap<String, Vec<String>>,
    mut make: impl FnMut(&str) -> (Vec<String>, Vec<String>),
) {
    let mut failures = Vec::new();

    for (file, expected) in golden {
        let (actual, full) = make(file);

        if &actual != expected {
            failures.push(format!(
                "{file}:\n  expected {expected:#?}\n  actual {actual:#?}\n  full Rust dump:\n{}",
                full.join("\n")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} files differ:\n{}",
        failures.len(),
        golden.len(),
        failures.join("\n")
    );
}

#[test]
fn corpus() {
    let root = kicad_qa::kicad_source_root();
    let golden = read_golden("rules_corpus.txt");

    assert!(
        golden.len() >= 70,
        "corpus golden has {} files",
        golden.len()
    );

    check(&golden, |rel| {
        let path = root.join(rel);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let lines = common::dump_rules(&read_text(&path), &name, false);
        (common::compact(&lines), lines)
    });
}

#[test]
fn broken_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules");
    let golden = read_golden("rules_errors.txt");

    assert!(
        golden.len() >= 90,
        "error golden has {} files",
        golden.len()
    );

    check(&golden, |name| {
        let lines = common::dump_rules(&read_text(&dir.join(name)), name, false);
        (common::summary(&lines), lines)
    });
}

#[test]
fn component_class_rules() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules/cc");
    let golden = read_golden("rules_cc.txt");

    assert!(golden.len() >= 8);

    check(&golden, |name| {
        let lines = common::dump_rules(&read_text(&dir.join(name)), name, true);
        (common::summary(&lines), lines)
    });
}

/// Every broken file has a golden entry, except the one the C++ cannot parse at all.
#[test]
fn every_test_file_is_in_the_golden() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules");
    let golden = read_golden("rules_errors.txt");

    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();

        if name.ends_with(".kicad_dru") && name != "return_path_unknown_eof.kicad_dru" {
            assert!(golden.contains_key(&name), "{name} missing from the golden");
        }
    }
}

/// `return_path_unknown_eof.kicad_dru`: an unknown `return_path` sub-option cut off by the end
/// of the input makes the C++ loop `while( NextTok() != T_RIGHT );` spin forever (the oracle
/// hangs). The port stops at the end of the input and reports the missing ')' (deviation
/// documented in PORT_STATUS.md).
#[test]
fn return_path_unknown_option_at_eof_terminates() {
    let text = "(version 1)\n(rule a (constraint return_path (foo bar";
    let (rules, reporter, result) = common::parse_with_reporter(text, "t");

    assert!(result.is_ok());
    assert_eq!(rules.len(), 1);
    assert!(rules[0].constraints.is_empty());
    assert!(reporter.messages_text().contains("Missing ')'."));

    let (_, result) = common::parse_throwing(text, "t");
    let error = result.unwrap_err();
    assert_eq!(
        error.what(),
        "ERROR: Missing ')'. in 't', line 3, offset 41." // (EOF counts as a line, as in C++)
    );
}
