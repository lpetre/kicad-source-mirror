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

//! Package CC-1: dynamic component class assignment through the load sequences.
//!
//! * `component_classes_match_reference_dump`: the effective class of every footprint of the
//!   boards with assignment rules vs the reference `pcb-dump` on the QA and the CLI path
//!   (golden `tests/golden/cc1_component_class.txt`, `tests/oracle/
//!   cc1_component_class_golden.sh`).
//! * Port of qa/tests/pcbnew/test_component_classes.cpp `ComponentClasses`.
//!
//! The footprints of [`NEEDS_DRC22`] get a class from a rule that uses an expression function
//! of package DRC-22 (`getField`, `memberOfSheet`, `intersectsArea`), not ported yet: the
//! evaluator reports "not yet ported" and the condition is false. They are checked by the
//! ignored tests only; remove the list when DRC-22 lands.

use std::collections::BTreeSet;

use kicad_pcb::BoardCtx;
use kicad_session::{LoadInputs, Session};

/// References whose expected class depends on a DRC-22 function (see the module comment).
const NEEDS_DRC22: &[&str] = &["C1", "C2", "R2", "R3", "U2"];

fn load(board: &str, cli: bool) -> Session {
    let dir = kicad_qa::pcbnew_data_dir();
    let path = dir.join(format!("{board}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).unwrap();
    let pro = std::fs::read_to_string(dir.join(format!("{board}.kicad_pro"))).ok();
    let rules = std::fs::read_to_string(dir.join(format!("{board}.kicad_dru"))).ok();
    let name = path.to_str().unwrap().to_string();
    let inputs = LoadInputs {
        text: &text,
        source_name: &name,
        project_json: pro.as_deref(),
        rules_text: rules.as_deref(),
        ..LoadInputs::default()
    };

    let session = if cli {
        Session::load_board_with(&inputs)
    } else {
        Session::load_board_qa(&inputs)
    };

    session.unwrap()
}

/// (reference, `GetComponentClassAsString()`) of every footprint in board order.
fn classes(session: &Session) -> Vec<(String, String)> {
    let ctx = BoardCtx::bare(&session.board);
    session
        .board
        .footprints()
        .map(|(_, fp)| (fp.reference(&ctx), fp.component_class_as_string(&ctx)))
        .collect()
}

fn compare_with_reference_dump(all: bool) {
    let golden = include_str!("golden/cc1_component_class.txt");
    let mut checked = 0;
    let mut mismatches = Vec::new();

    for board in ["component_classes", "component_classes_drc"] {
        for (mode, cli) in [("qa", false), ("loader", true)] {
            let session = load(board, cli);
            assert!(session.pending_load_steps.is_empty());
            let got = classes(&session);
            let prefix = format!("{board}|{mode}|");
            let expected: Vec<(String, String)> = golden
                .lines()
                .filter_map(|l| l.strip_prefix(&prefix))
                .map(|l| {
                    let (r, c) = l.split_once('|').unwrap();
                    (r.to_string(), c.to_string())
                })
                .collect();

            assert_eq!(got.len(), expected.len(), "{board} {mode}: footprint count");

            for ((gr, gc), (er, ec)) in got.iter().zip(expected.iter()) {
                assert_eq!(gr, er, "{board} {mode}: footprint order");

                if !all && NEEDS_DRC22.contains(&gr.as_str()) {
                    continue;
                }

                checked += 1;

                if gc != ec {
                    mismatches.push(format!("{board} {mode} {gr}: {gc:?}, reference {ec:?}"));
                }
            }
        }
    }

    assert!(mismatches.is_empty(), "{mismatches:#?}");
    assert!(checked >= 26);
}

/// The effective class of every footprint vs the reference dump (both load paths), except the
/// footprints of [`NEEDS_DRC22`].
#[test]
fn component_classes_match_reference_dump() {
    compare_with_reference_dump(false);
}

/// As [`component_classes_match_reference_dump`] for every footprint.
#[test]
#[ignore = "needs the DRC-22 expression functions getField, memberOfSheet, intersectsArea"]
fn component_classes_match_reference_dump_all() {
    compare_with_reference_dump(true);
}

/// The expected classes of qa/tests/pcbnew/test_component_classes.cpp `ComponentClasses`.
const EXPECTED: &[(&str, &[&str])] = &[
    ("C1", &["CAP_1", "CLASS_3", "CLASS_4"]),
    ("C2", &["CAP_2", "CLASS_3"]),
    ("C3", &[]),
    ("R8", &["RES_1", "RES_2"]),
    ("R88", &["RES_2"]),
    ("R2", &["CLASS_1", "RES_1", "RES_2", "RES_3"]),
    ("R1", &["CLASS_1", "CLASS_2", "RES_1", "RES_2", "RES_4"]),
    ("U1", &["ANY"]),
    ("U2", &["ANY"]),
    ("U3", &["MULTI_REF"]),
    ("U4", &["MULTI_REF"]),
    ("U55", &["REF_WILDCARD", "REF_WILDCARD2"]),
    ("U555", &["REF_WILDCARD2"]),
    ("R3", &["/SHEET1/", "RES_1", "RES_2"]),
];

fn port_component_classes(all: bool) {
    let session = load("component_classes", false);
    let ctx = BoardCtx::bare(&session.board);

    let all_classes: BTreeSet<&str> = [
        "ANY",
        "CAP_1",
        "CAP_2",
        "CLASS_1",
        "CLASS_2",
        "CLASS_3",
        "CLASS_4",
        "MULTI_REF",
        "REF_WILDCARD",
        "REF_WILDCARD2",
        "RES_1",
        "RES_2",
        "RES_3",
        "RES_4",
    ]
    .into_iter()
    .collect();

    let mut errors = Vec::new();

    for (_, fp) in session.board.footprints() {
        let reference = fp.reference(&ctx);

        let Some((_, assigned)) = EXPECTED.iter().find(|(r, _)| *r == reference) else {
            continue;
        };

        if !all && NEEDS_DRC22.contains(&reference.as_str()) {
            continue;
        }

        let comp_class = fp.component_class(&ctx).unwrap();

        for class_name in assigned.iter() {
            if !comp_class.contains_class_name(class_name) {
                errors.push(format!(
                    "{reference} component class failed ({class_name} expected but not found - \
                     full class {})",
                    comp_class.name()
                ));
            }
        }

        for class_name in all_classes.iter().filter(|c| !assigned.contains(c)) {
            if comp_class.contains_class_name(class_name) {
                errors.push(format!(
                    "{reference} component class failed ({class_name} found but not expected - \
                     full class {})",
                    comp_class.name()
                ));
            }
        }
    }

    assert!(errors.is_empty(), "{errors:#?}");
}

/// qa/tests/pcbnew/test_component_classes.cpp `ComponentClasses` (KI_TEST::LoadBoard), except
/// the footprints of [`NEEDS_DRC22`].
#[test]
fn component_classes() {
    port_component_classes(false);
}

/// qa/tests/pcbnew/test_component_classes.cpp `ComponentClasses`, every footprint.
#[test]
#[ignore = "needs the DRC-22 expression functions getField, memberOfSheet, intersectsArea"]
fn component_classes_all() {
    port_component_classes(true);
}
