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

//! Replays the CC-1 oracle (`tests/oracle/cc1_oracle.cpp`, golden
//! `tests/golden/cc1_component_class.txt`): `COMPONENT_CLASS_ASSIGNMENT_DATA::
//! GetAssignmentInDRCLanguage` and the `COMPONENT_CLASS_SETTINGS` load of `PROJECT_FILE`.

use kicad_pcb::component_class::{
    ComponentClassAssignmentData, ComponentClassSettings, ConditionsOperator,
};
use kicad_pcb::project::load_project;

fn dump_settings(out: &mut Vec<String>, case: usize, settings: &ComponentClassSettings) {
    out.push(format!(
        "{case}|enabled|{}",
        settings.enable_sheet_component_classes as i32
    ));
    let list = &settings.component_class_assignments;
    out.push(format!("{case}|count|{}", list.len()));

    for (i, a) in list.iter().enumerate() {
        out.push(format!("{case}|a{i}.class|{}", a.component_class));
        let op = if a.conditions_operator == ConditionsOperator::All {
            "ALL"
        } else {
            "ANY"
        };
        out.push(format!("{case}|a{i}.op|{op}"));

        for (j, (t, p, s)) in a.conditions.iter().enumerate() {
            out.push(format!(
                "{case}|a{i}.c{j}|{}\t{p}\t{s}",
                ComponentClassAssignmentData::get_condition_name(*t)
            ));
        }

        out.push(format!(
            "{case}|a{i}.drc|{}",
            a.get_assignment_in_drc_language()
        ));
    }
}

fn load(out: &mut Vec<String>, case: usize, text: Option<&str>) {
    let project = load_project(text, None, "/cc1/cc1_case.kicad_pro");
    out.push(format!(
        "{case}|loaded|{}",
        project.project_file.file_synced as i32
    ));
    let settings = ComponentClassSettings::load_from_parent(Some(&project.project_file.internals));
    dump_settings(out, case, &settings);
}

#[test]
fn cc1_component_class_settings_oracle() {
    let here = env!("CARGO_MANIFEST_DIR");
    let cases = std::fs::read_to_string(format!("{here}/tests/oracle/cc1_cases.txt")).unwrap();
    let golden =
        std::fs::read_to_string(format!("{here}/tests/golden/cc1_component_class.txt")).unwrap();
    let root = kicad_qa::qa_data_dir().join("../..");

    let mut out = Vec::new();
    let mut case = 0;

    for line in cases.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let f: Vec<&str> = line.split('\t').collect();
        let c = case;
        case += 1;

        match f[0] {
            "assign" => {
                let mut a = ComponentClassAssignmentData::default();
                a.set_component_class(f[1]);
                a.set_conditions_operation(if f[2] == "ANY" {
                    ConditionsOperator::Any
                } else {
                    ConditionsOperator::All
                });

                let mut k = 3;

                while k + 2 < f.len() {
                    a.add_condition(
                        ComponentClassAssignmentData::get_condition_type(f[k]),
                        f[k + 1],
                        f[k + 2],
                    );
                    k += 3;
                }

                out.push(format!("{c}|drc|{}", a.get_assignment_in_drc_language()));
            }
            "project" => load(&mut out, c, Some(f[1])),
            "file" => {
                let text = std::fs::read_to_string(root.join(f[1])).unwrap();
                load(&mut out, c, Some(&text));
            }
            other => panic!("unknown case kind {other}"),
        }
    }

    let expected: Vec<&str> = golden.lines().collect();
    let mut mismatches = 0;

    for (i, (e, o)) in expected.iter().zip(out.iter()).enumerate() {
        if *e != o.as_str() {
            mismatches += 1;

            if mismatches < 20 {
                eprintln!("line {i}: expected {e:?}\n          got      {o:?}");
            }
        }
    }

    assert_eq!(expected.len(), out.len(), "line count");
    assert_eq!(mismatches, 0, "{mismatches} mismatching lines");
}
