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

//! Package ZF-06 (new code, test support): the saved board of
//! `kicad-cli pcb drc --refill-zones --save-board` (`PCBNEW_JOBS_HANDLER::JobExportDrc`:
//! `BOARD_LOADER::Load`, `ZONE_FILLER_TOOL::FillAllZones( nullptr, reporter, true )`, the DRC
//! run, `BOARD_LOADER::SaveBoard`) against the reference, byte for byte, over the zone-fill
//! corpus cached by `rust/tools/zone-corpus/run.sh` (`rust/target/zone-corpus/<name>/in` and
//! `ref`).
//!
//! The Rust side runs [`Session::load_board_with`] (the kicad-cli load sequence),
//! [`Session::refill_zones`] and [`drc_save_board`]; the DRC run between the fill and the save
//! (`DRC_ENGINE::RunTests`, package DRC-12) is not wired into the session yet. It only adds
//! markers, which the board writer does not save.
//!
//! Ignored in the default run (it needs the corpus cache):
//! `cargo test -p kicad-session --release --test zf06_refill_save -- --ignored --nocapture`.
//! Writes `rust/target/zone-corpus/save_report.txt`.

mod common;

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use kicad_pcb::project::LoadPath;
use kicad_qa::sexpr_text::first_difference;
use kicad_session::jobs::JobPcbDrc;
use kicad_session::jobs_handler::drc_save_board;
use kicad_session::{JobOutcome, LoadInputs, Reporter, SessionError};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zone-corpus")
}

fn board_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
}

/// One board: `Ok(None)` identical, `Ok(Some(first difference))`, `Err(not run: reason)`.
fn run_board(dir: &Path) -> Result<Option<String>, String> {
    let (Some(input), Some(reference)) = (board_in(&dir.join("in")), board_in(&dir.join("ref")))
    else {
        return Err("no reference".to_string());
    };

    let text = std::fs::read_to_string(&input).map_err(|e| e.to_string())?;
    let pro = std::fs::read_to_string(input.with_extension("kicad_pro")).ok();
    let dru = std::fs::read_to_string(input.with_extension("kicad_dru")).ok();
    let expected = std::fs::read_to_string(&reference).map_err(|e| e.to_string())?;
    // The reference ran on the copy in ref/: same absolute file name.
    let source_name = reference.to_str().unwrap().to_string();

    let inputs = LoadInputs {
        text: &text,
        source_name: &source_name,
        project_json: pro.as_deref(),
        rules_text: dru.as_deref(),
        ..LoadInputs::default()
    };

    let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
        let (mut session, _) = common::load_staged(LoadPath::Cli, &inputs).map_err(|e| match e {
            SessionError::NotYetPorted { subsystem } => format!("not loadable: {subsystem}"),
            other => format!("load error: {other}"),
        })?;

        let mut job = JobPcbDrc::new();
        job.rc.filename = source_name.clone();
        job.refill_zones = true;
        job.save_board = true;

        session.refill_zones().map_err(|e| format!("refill: {e}"))?;

        let mut reporter = Reporter::new();
        let mut outcome = JobOutcome::default();

        if let Some(code) = drc_save_board(&mut session, &job, &mut reporter, &mut outcome)
            .map_err(|e| format!("save: {e}"))?
        {
            return Err(format!("save failed ({code}): {}", reporter.text()));
        }

        assert_eq!(reporter.text(), "Saved board\n");
        let [file] = outcome.files.as_slice() else {
            return Err("no saved board".to_string());
        };
        assert_eq!(file.name, source_name);
        Ok(String::from_utf8_lossy(&file.contents).into_owned())
    }));

    let actual = match result {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return Err(e),
        Err(p) => return Err(format!("panic: {}", common::panic_text(p.as_ref()))),
    };

    Ok(first_difference(&expected, &actual, 1).map(|d| {
        format!(
            "line {} ({}): expected {:?}, actual {:?}",
            d.line,
            d.path.join(" > "),
            d.expected.unwrap_or_default(),
            d.actual.unwrap_or_default()
        )
    }))
}

/// The saved-board scoreboard of `pcb drc --refill-zones --save-board` over the zone corpus.
#[test]
#[ignore = "needs the zone-corpus reference cache (tools/zone-corpus/run.sh --refs-only)"]
fn zf06_refill_save_corpus() {
    let Ok(entries) = std::fs::read_dir(corpus_dir()) else {
        eprintln!("no zone corpus cache; skipped");
        return;
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("ref").is_dir())
        .collect();
    dirs.sort();

    let (mut identical, mut differing, mut not_run) = (0, 0, 0);
    let mut report = String::new();

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy();
        let line = match run_board(dir) {
            Ok(None) => {
                identical += 1;
                "identical".to_string()
            }
            Ok(Some(d)) => {
                differing += 1;
                format!("DIFFERENT: {d}")
            }
            Err(e) => {
                not_run += 1;
                format!("not run: {e}")
            }
        };
        let _ = writeln!(report, "{name}: {line}");
    }

    let _ = writeln!(
        report,
        "TOTAL saved boards identical {identical}, different {differing}, not run {not_run}"
    );
    print!("{report}");
    let _ = std::fs::write(corpus_dir().join("save_report.txt"), &report);
}
