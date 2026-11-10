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

//! DRC-LP: the library parity test (`drc_test_provider_library_parity.cpp`) on the crafted
//! project of tests/data/drc_lp (made by tests/oracle/gen_drc_lp_cases.py from demos/ecc83)
//! against the reference kicad-cli report (tests/golden/drc_lp_library_parity.json: every
//! `lib_footprint_*` entry, in report order). The project's tables, libraries and the global
//! table are given to the session through an in-memory library source laid out as the
//! reference run saw them (project in `/w/drc_lp`, global table in the config directory).

use std::path::Path;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::json::Value;
use kicad_session::footprint_libraries::MemoryLibrarySource;
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat, RcUnits};
use kicad_session::{LibraryHost, LoadInputs, Reporter, Session, run_drc};

const PROJECT_DIR: &str = "/w/drc_lp";
const CONFIG_DIR: &str = "/home/u/.config/kicad/10.99";

fn data_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/drc_lp")
}

/// Every file of `dir` (recursively) under `prefix`.
fn add_tree(src: &mut MemoryLibrarySource, dir: &Path, prefix: &str) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}/{name}");

        if entry.file_type().unwrap().is_dir() {
            add_tree(src, &entry.path(), &path);
        } else {
            src.insert(&path, std::fs::read(entry.path()).unwrap());
        }
    }
}

/// The `lib_footprint_*` violations of the JSON report.
fn lib_entries(report: &str) -> Vec<Value> {
    let doc: Value = kicad_core::json::parse(report, kicad_core::json::ObjectOrder::Insertion)
        .expect("json report");

    doc["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .filter(|v| {
            v["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("lib_footprint"))
        })
        .cloned()
        .collect()
}

#[test]
fn library_parity_matches_reference() {
    let data = data_dir();
    let board_path = format!("{PROJECT_DIR}/drc_lp.kicad_pcb");
    let text = std::fs::read_to_string(data.join("drc_lp.kicad_pcb")).unwrap();
    let project_path = format!("{PROJECT_DIR}/drc_lp.kicad_pro");

    let inputs = LoadInputs {
        text: &text,
        source_name: &board_path,
        project_path: Some(&project_path),
        ..LoadInputs::default()
    };
    let mut session = Session::load_board_with(&inputs).expect("load");

    let mut src = MemoryLibrarySource::default();

    for entry in std::fs::read_dir(&data).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();

        if name == "global" {
            continue;
        }

        if entry.file_type().unwrap().is_dir() {
            add_tree(&mut src, &entry.path(), &format!("{PROJECT_DIR}/{name}"));
        } else {
            src.insert(
                &format!("{PROJECT_DIR}/{name}"),
                std::fs::read(entry.path()).unwrap(),
            );
        }
    }

    src.insert(
        &format!("{CONFIG_DIR}/fp-lib-table"),
        std::fs::read(data.join("global/fp-lib-table")).unwrap(),
    );

    let host = LibraryHost {
        cwd: PROJECT_DIR.to_string(),
        home: "/home/u".to_string(),
        user_settings_path: CONFIG_DIR.to_string(),
        stock_footprints_path: Some("/usr/share/kicad/footprints/".to_string()),
        ..Default::default()
    };
    session.load_footprint_libraries(Arc::new(src), &host, &inputs.parse_options);

    let mut job = JobPcbDrc::new();
    job.rc.filename = board_path.clone();
    job.rc.format = RcOutputFormat::Json;
    job.rc.units = RcUnits::Mm;
    job.rc.severity =
        Severity::Error.as_i32() | Severity::Warning.as_i32() | Severity::Exclusion.as_i32();
    job.report_date = "<DATE>".to_string();

    let mut reporter = Reporter::new();
    let outcome = run_drc(&mut session, &mut job, &mut reporter).expect("drc");
    let report = outcome.report_text.expect("report");
    let actual = lib_entries(&report);

    let golden: Value = kicad_core::json::parse(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/drc_lp_library_parity.json"),
        )
        .unwrap(),
        kicad_core::json::ObjectOrder::Insertion,
    )
    .unwrap();
    let expected = golden.as_array().unwrap();

    let fmt = |v: &Value| {
        format!(
            "{} | {}",
            v["items"][0]["description"].as_str().unwrap_or(""),
            v["description"].as_str().unwrap_or("")
        )
    };
    let actual_lines: Vec<String> = actual.iter().map(fmt).collect();
    let expected_lines: Vec<String> = expected.iter().map(fmt).collect();

    assert_eq!(
        actual_lines, expected_lines,
        "entries (footprint | message)"
    );
    assert_eq!(&actual, expected, "entries (items, positions, severity)");
}
