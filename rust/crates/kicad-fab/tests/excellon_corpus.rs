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
//! FAB-GLUE: Excellon drill files and the drill report (`EXCELLON_WRITER`,
//! `GENDRILL_WRITER_BASE::GenDrillReportFile`) from parsed boards through the real drill
//! enumeration (`pcbnew/drill/drill_enumerator.cpp`) and the drill job
//! (`PCBNEW_JOBS_HANDLER::JobExportDrill`), against the reference kicad-cli
//! (`kicad-cli pcb export drill --format excellon <set>`, golden
//! `tests/golden/excellon_corpus/corpus.txt` made by `generate.py` there: every corpus board
//! under 300 KB and the crafted boards of `tests/data/gerber_drill/` and
//! `tests/data/excellon_corpus/`, twelve option sets).
//!
//! File names, order and contents (FNV-1a of the normalised text) must be identical; the
//! crafted boards are also compared in full. `KICAD_FAB_EXCELLON_DUMP=<dir>` writes differing
//! files there.

mod plot_common;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use kicad_fab::VecReporter;
use kicad_fab::exporters::gendrill_report::BoardHoleSource;
use kicad_fab::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillFormat, DrillOrigin, DrillUnits, JobExportPcbDrill, ZerosFormat,
};
use kicad_fab::jobs_handler::job_export_drill_from;
use kicad_pcb::{Board, BoardCtx};
use plot_common::{fnv, load_board};

const CORPUS: &str = include_str!("golden/excellon_corpus/corpus.txt");

/// One run of `generate.py`: option set, exit code, board, created files with hashes.
struct Run {
    set: String,
    code: i32,
    path: String,
    files: Vec<(String, String)>,
}

fn corpus() -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();

    for line in CORPUS.lines() {
        let f: Vec<&str> = line.splitn(4, ' ').collect();
        match f[0] {
            "B" => runs.push(Run {
                set: f[1].to_string(),
                code: f[2].parse().unwrap(),
                path: f[3].to_string(),
                files: Vec::new(),
            }),
            "F" => {
                let (name, hash) = line[2..].rsplit_once(' ').unwrap();
                runs.last_mut()
                    .unwrap()
                    .files
                    .push((name.to_string(), hash.to_string()));
            }
            other => panic!("corpus line {other}"),
        }
    }

    runs
}

/// The normalisation of `generate.py`: version / date lines of the Excellon header and the
/// report's `Created on` line.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let replace = (body.starts_with("; DRILL file KiCad ") && body.contains(" date "))
            || body.starts_with("; #@! TF.CreationDate,")
            || body.starts_with("; #@! TF.GenerationSoftware,")
            || body.starts_with("Created on ");

        if replace {
            out.push_str("<normalised>");
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
        }
    }

    out
}

/// `kicad-cli pcb export drill --format excellon <set>` (`command_pcb_export_drill.cpp`:
/// absolute origin, decimal zeros, alternate (G85) ovals, mm, PTH and NPTH merged).
fn cli_job(set: &str) -> JobExportPcbDrill {
    let mut job = JobExportPcbDrill {
        format: DrillFormat::Excellon,
        drill_origin: DrillOrigin::Abs,
        drill_units: DrillUnits::Mm,
        zero_format: ZerosFormat::Decimal,
        excellon_oval_drill_route: false,
        excellon_combine_pth_npth: true,
        ..JobExportPcbDrill::default()
    };

    match set {
        "default" => {}
        "separate_th" => job.excellon_combine_pth_npth = false,
        "oval_route" => job.excellon_oval_drill_route = true,
        "inch" => job.drill_units = DrillUnits::Inch,
        "zeros_sl" => job.zero_format = ZerosFormat::SuppressLeading,
        "zeros_st" => job.zero_format = ZerosFormat::SuppressTrailing,
        "zeros_keep" => job.zero_format = ZerosFormat::KeepZeros,
        "plot" => job.drill_origin = DrillOrigin::Plot,
        "min_header" => job.excellon_minimal_header = true,
        "mirror" => job.excellon_mirror_y = true,
        "report" => job.generate_report = true,
        "report_sep" => {
            job.generate_report = true;
            job.excellon_combine_pth_npth = false;
        }
        other => panic!("set {other}"),
    }

    job
}

fn drill_env() -> DrillExportEnv {
    let env = plot_common::env();
    DrillExportEnv {
        build_version: env.build_version,
        creation_date: env.creation_date,
    }
}

/// One port run: file names (stdout "Created file" order, then the unannounced files, i.e.
/// the report, by name, as `generate.py` lists them), hash of the normalised text, text.
type PortFiles = Vec<(String, String, String)>;

fn run_port(board: &Board, set: &str) -> PortFiles {
    let mut rep = VecReporter::default();
    let out = job_export_drill_from(
        &BoardHoleSource,
        BoardCtx::bare(board),
        &cli_job(set),
        "out/",
        &drill_env(),
        None,
        Some(&mut rep),
    );
    assert!(out.success, "{}: {:?}", board.file_name, out.error);
    assert_eq!(
        rep.messages.last().map(|m| m.0.as_str()),
        Some("Done."),
        "{}",
        board.file_name
    );

    let mut names: Vec<String> = rep
        .messages
        .iter()
        .filter_map(|(m, _)| m.strip_prefix("Created file 'out/"))
        .map(|m| m.strip_suffix("'.").unwrap().to_string())
        .collect();
    let mut rest: Vec<String> = out
        .files
        .iter()
        .map(|f| f.name.strip_prefix("out/").unwrap().to_string())
        .filter(|n| !names.contains(n))
        .collect();
    rest.sort();
    names.extend(rest);

    let mut files = Vec::new();

    for name in names {
        let file = out
            .files
            .iter()
            .find(|f| f.name == format!("out/{name}"))
            .unwrap_or_else(|| panic!("{name} reported but not produced"));
        let text = normalise(&String::from_utf8(file.contents.clone()).unwrap());
        files.push((name, fnv(&text)[..8].to_string(), text));
    }

    assert_eq!(files.len(), out.files.len(), "{}", board.file_name);
    files
}

/// Compares a port run with a golden run; writes the differing files to `dump` if set.
fn same(run: &Run, got: &PortFiles, dump: Option<&Path>) -> bool {
    let names_got: Vec<&str> = got.iter().map(|f| f.0.as_str()).collect();
    let names_exp: Vec<&str> = run.files.iter().map(|f| f.0.as_str()).collect();

    if names_got != names_exp {
        println!(
            "{} [{}]: files differ:\n  port {names_got:?}\n  cli  {names_exp:?}",
            run.path, run.set
        );
        return false;
    }

    let mut ok = true;

    for ((name, hash, text), (_, expected)) in got.iter().zip(&run.files) {
        if hash != expected {
            println!("{} [{}]: {name} differs", run.path, run.set);
            ok = false;

            if let Some(dir) = dump {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{}-{name}", run.set)), text);
            }
        }
    }

    ok
}

fn kicad_root() -> PathBuf {
    kicad_qa::kicad_source_root()
}

/// Every golden board through the parser, the real drill enumeration and the drill job.
#[test]
fn corpus_boards_match_cli() {
    let runs = corpus();
    let dump = std::env::var_os("KICAD_FAB_EXCELLON_DUMP").map(PathBuf::from);
    let root = kicad_root();

    #[derive(Default)]
    struct Score {
        runs: usize,
        identical: usize,
        identical_files: usize,
        not_loadable: usize,
        pending: usize,
        cli_failed: usize,
        differ: Vec<String>,
    }

    let mut score = Score::default();
    let mut pending_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_set: BTreeMap<&str, usize> = BTreeMap::new();
    let mut boards: BTreeMap<&str, Option<Board>> = BTreeMap::new();

    // The pending stubs panic by design; keep the output readable.
    std::panic::set_hook(Box::new(|_| {}));

    for run in &runs {
        score.runs += 1;

        if run.code != 0 {
            score.cli_failed += 1;
            continue;
        }

        let board = boards.entry(run.path.as_str()).or_insert_with(|| {
            match catch_unwind(|| load_board(&root.join(&run.path))) {
                Ok(Ok(board)) => Some(board),
                _ => None,
            }
        });

        let Some(board) = board.as_ref() else {
            score.not_loadable += 1;
            continue;
        };

        match catch_unwind(AssertUnwindSafe(|| run_port(board, &run.set))) {
            Ok(got) => {
                if same(run, &got, dump.as_deref()) {
                    score.identical += 1;
                    score.identical_files += got.len();
                    *per_set.entry(run.set.as_str()).or_default() += 1;
                } else {
                    score.differ.push(format!("{} [{}]", run.path, run.set));
                }
            }
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();

                if msg.contains("pending") {
                    score.pending += 1;
                    let key: String = msg.chars().take(80).collect();
                    *pending_reasons.entry(key).or_default() += 1;
                } else {
                    score
                        .differ
                        .push(format!("{} [{}]: panic {msg}", run.path, run.set));
                }
            }
        }
    }

    let _ = std::panic::take_hook();

    println!(
        "excellon corpus: {} runs: {} identical ({} files), {} pending, {} board not loadable, \
         {} kicad-cli failures, {} differ",
        score.runs,
        score.identical,
        score.identical_files,
        score.pending,
        score.not_loadable,
        score.cli_failed,
        score.differ.len()
    );
    println!("  identical per set: {per_set:?}");

    for (reason, n) in &pending_reasons {
        println!("  pending x{n}: {reason}");
    }

    assert!(score.differ.is_empty(), "differ: {:#?}", score.differ);
    assert_eq!(score.not_loadable, 0, "every golden board loads");
    assert_eq!(score.pending, 0, "no run reaches pending code");
}

/// The crafted boards' full normalised goldens (`<stem>/<set>/`) equal the port's files: a
/// readable diff where the hash comparison only says "differs".
#[test]
fn crafted_boards_full_text() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/excellon_corpus");
    let mut compared = 0;

    for run in corpus().iter().filter(|r| r.path.starts_with("rust/")) {
        let stem = Path::new(&run.path).file_stem().unwrap().to_str().unwrap();
        let board = load_board(&kicad_root().join(&run.path)).unwrap();
        let got = run_port(&board, &run.set);

        for (name, _, text) in &got {
            let expected = std::fs::read_to_string(here.join(stem).join(&run.set).join(name))
                .unwrap_or_else(|e| panic!("{stem}/{}/{name}: {e}", run.set));
            assert_eq!(text, &expected, "{stem} [{}] {name}", run.set);
            compared += 1;
        }
    }

    assert!(compared > 0);
}
