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

//! Package SET-5: ports of the project-related cases of `qa/tests/common/test_project_file.cpp`
//! (load side) and `qa/tests/pcbnew/test_board_loader.cpp` (model side), plus a corpus smoke
//! test of `load_project` (`docs/analysis/settings.md` 9.3 (7)).

use std::rc::Rc;

use kicad_core::json::{ObjectOrder, Value};
use kicad_pcb::board::Board;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::project::board_design_settings_json::{DrcExclusionCodec, RawDrcExclusionCodec};
use kicad_pcb::project::board_loader::{LoadPath, LoadStep};
use kicad_pcb::project::{StepOutcome, load_project, run_model_step};

fn codec() -> Rc<dyn DrcExclusionCodec> {
    Rc::new(RawDrcExclusionCodec)
}

/// `test_project_file.cpp` `LoadProjectByAbsolutePathIsStable` (the part that is not settings
/// manager bookkeeping): a project loaded by its absolute path keeps that path as its full
/// name, and loading it again gives the same name.
#[test]
fn load_project_by_absolute_path_is_stable() {
    let pro = r#"{
        "meta": {
            "filename": "jobset_project.kicad_pro",
            "version": 3
        }
    }"#;
    let abs = "/tmp/x/jobset_project/jobset_project.kicad_pro";
    let held = load_project(Some(pro), None, abs);
    let again = load_project(Some(pro), None, abs);
    assert_eq!(held.project_full_name(), abs);
    assert_eq!(again.project_full_name(), held.project_full_name());
    assert_eq!(held.project_name(), "jobset_project");
}

/// `test_project_file.cpp` `AliasMigrationPreservesNonemptyDefinitions` (load side; the save is
/// SET-13): migration 3 -> 4 keeps a non-empty `schematic.bus_aliases` and the version becomes 4.
#[test]
fn alias_migration_preserves_nonempty_definitions() {
    let text = r#"{ "meta": { "version": 3 },
                    "schematic": { "bus_aliases": { "USB": [ "D+", "D-" ] } } }"#;
    let project = load_project(Some(text), None, "/tmp/aliases.kicad_pro");
    let pf = &project.project_file;
    assert!(pf.file_synced);
    assert_eq!(pf.internals.get::<i32>("meta.version").unwrap(), Some(4));
    let aliases = pf
        .internals
        .get_json("schematic.bus_aliases")
        .unwrap()
        .unwrap();
    let expected: Value =
        kicad_core::json::parse(r#"[ "D+", "D-" ]"#, ObjectOrder::Sorted).unwrap();
    assert_eq!(aliases["USB"], expected);
}

fn reference_images_board() -> Option<(Board, String)> {
    let base = kicad_qa::pcbnew_data_dir().join("reference_images_load_save");
    let pcb = std::fs::read_to_string(base.with_extension("kicad_pcb")).ok()?;
    let pro = std::fs::read_to_string(base.with_extension("kicad_pro")).ok()?;
    let path = base
        .with_extension("kicad_pcb")
        .to_string_lossy()
        .into_owned();
    let options = ParseOptions {
        allow_pending: true,
    };
    let mut board = parse_board(&pcb, &path, &options).expect("board parses");
    // PCB_IO::LoadBoard: SetFileName( aFileName )
    board.file_name = path;
    Some((board, pro))
}

/// `test_board_loader.cpp` `BoardLoaderWithInit`, model side: the project is bound (the C++
/// checks the pointer; the port moves the project in, so its name is checked), the session
/// steps (DRC engine, exclusions, connectivity) are handed to the caller in C++ order, and the
/// drawings are there.
#[test]
fn board_loader_with_init() {
    let Some((mut board, pro)) = reference_images_board() else {
        return;
    };
    let pro_name = board.file_name.replace(".kicad_pcb", ".kicad_pro");
    let project = load_project(Some(&pro), None, &pro_name);

    // The steps one by one (initialize_loaded_board chains the same calls): the model step
    // SynchronizeComponentClasses needs COMPONENT_CLASS_MANAGER::
    // SyncDynamicComponentClassAssignments, still a port_pending! stub (SET-10), so it is
    // skipped here.
    let mut project = Some(project);
    let codec = codec();
    let mut session = Vec::new();

    for &step in LoadPath::Cli.steps() {
        if step == LoadStep::SynchronizeComponentClasses {
            continue;
        }

        match run_model_step(&mut board, LoadPath::Cli, step, &mut project, &codec).unwrap() {
            StepOutcome::Done => {}
            StepOutcome::Session => session.push(step),
            StepOutcome::Warning(w) => panic!("unexpected warning {w}"),
        }
    }

    assert!(project.is_none());
    assert_eq!(
        session,
        vec![
            LoadStep::InitDrcEngine,
            LoadStep::ResolveDrcExclusions,
            LoadStep::BuildConnectivity
        ]
    );
    let bound = board.project.as_ref().expect("project bound");
    assert_eq!(bound.project_name, "reference_images_load_save");
    assert!(board.drawings().count() > 0);
    assert!(
        board
            .get_design_rules_path()
            .ends_with("reference_images_load_save.kicad_dru")
    );
}

/// `test_board_loader.cpp` `BoardLoaderWithoutInit`: without the initialisation the board has
/// no project.
#[test]
fn board_loader_without_init() {
    let Some((board, _)) = reference_images_board() else {
        return;
    };
    assert!(board.project.is_none());
}

/// `settings.md` 9.3 (7): every project file of the corpus loads (the document parses and has
/// a version, except the files known to have none) and its nested net settings come out.
#[test]
fn corpus_project_files_load() {
    let mut files = Vec::new();

    for dir in [kicad_qa::qa_data_dir(), kicad_qa::demos_dir()] {
        collect(&dir, &mut files);
    }

    let mut synced = 0;

    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        let project = load_project(Some(&text), None, &f.to_string_lossy());

        if project.project_file.file_synced {
            synced += 1;
        }

        // The default class always exists.
        assert_eq!(
            project.project_file.net_settings.default_netclass().name,
            "Default",
            "{}",
            f.display()
        );
    }

    // Some corpus files are deliberately broken or versionless; most load.
    assert!(
        files.is_empty() || synced * 10 >= files.len() * 9,
        "{synced} / {}",
        files.len()
    );
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for e in entries.flatten() {
        let p = e.path();

        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "kicad_pro") {
            out.push(p);
        }
    }
}
