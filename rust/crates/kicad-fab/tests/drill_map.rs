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

//! FAB-12: drill maps (`GENDRILL_WRITER_BASE::genDrillMapFile` / `plotDrillMarks`,
//! `pcbnew/exporters/gendrill_writer_base.cpp`) against the reference kicad-cli
//! (`kicad-cli pcb export drill --generate-map`, golden `tests/golden/drill_map/corpus.txt`
//! made by `generate.py` there: corpus boards under 300 KB and the crafted boards of
//! `tests/data/drill_map/` and `tests/data/excellon_corpus/`; Gerber X2 and SVG maps of
//! Excellon (merged, separate, plot origin) and Gerber drill exports).
//!
//! * `corpus_maps_match_cli`: every golden board through the parser, the real enumeration and
//!   the drill job; the map files (names, order, normalised contents) must equal kicad-cli's.
//!   Boards or parts reaching pending code are counted, not failed; any difference fails.
//!   `KICAD_FAB_DRILL_MAP_DUMP=<dir>` writes the differing files there.
//! * `crafted_maps_full_text`: the full normalised maps kept for the crafted boards.
//! * `unsupported_map_formats`: PDF / PostScript / DXF maps fail with the documented error.

mod plot_common;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kicad_fab::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillFormat, DrillOrigin, DrillUnits, JobExportPcbDrill, MapFormat, ZerosFormat,
};
use kicad_fab::jobs_handler::job_export_drill;
use kicad_fab::{FabError, VecReporter};
use kicad_pcb::{Board, BoardCtx};

use plot_common::{fnv, load_board, normalise};

const CORPUS: &str = include_str!("golden/drill_map/corpus.txt");

/// One run of `generate.py`: option set, exit code, board, created maps with hashes.
struct Run {
    set: String,
    code: i32,
    path: String,
    files: Vec<(String, String)>,
}

fn corpus() -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut path = String::new();

    for line in CORPUS.lines() {
        let f: Vec<&str> = line.splitn(3, ' ').collect();
        match f[0] {
            "B" => path = line[2..].to_string(),
            "S" => runs.push(Run {
                set: f[1].to_string(),
                code: f[2].parse().unwrap(),
                path: path.clone(),
                files: Vec::new(),
            }),
            "F" => {
                // The file name may contain spaces; the hash is the last field.
                let (name, hash) = line[2..].rsplit_once(' ').unwrap();
                runs.last_mut()
                    .unwrap()
                    .files
                    .push((name.to_string(), hash.to_string()));
            }
            other => panic!("bad golden line kind {other}"),
        }
    }

    runs
}

/// The job of `kicad-cli pcb export drill --generate-map <set>` (CLI defaults otherwise).
fn cli_job(set: &str) -> JobExportPcbDrill {
    let mut job = JobExportPcbDrill {
        format: DrillFormat::Excellon,
        drill_origin: DrillOrigin::Abs,
        drill_units: DrillUnits::Mm,
        zero_format: ZerosFormat::Decimal,
        excellon_oval_drill_route: false,
        excellon_combine_pth_npth: true,
        generate_map: true,
        ..JobExportPcbDrill::default()
    };

    match set {
        "exc_gbr" => job.map_format = MapFormat::GerberX2,
        "exc_gbr_sep" => {
            job.map_format = MapFormat::GerberX2;
            job.excellon_combine_pth_npth = false;
        }
        "exc_gbr_plot" => {
            job.map_format = MapFormat::GerberX2;
            job.drill_origin = DrillOrigin::Plot;
        }
        "exc_svg" => job.map_format = MapFormat::Svg,
        "exc_svg_sep" => {
            job.map_format = MapFormat::Svg;
            job.excellon_combine_pth_npth = false;
        }
        "gbr_gbr" => {
            job.format = DrillFormat::Gerber;
            job.map_format = MapFormat::GerberX2;
        }
        "gbr_svg" => {
            job.format = DrillFormat::Gerber;
            job.map_format = MapFormat::Svg;
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

/// `normalise` plus the date of the SVG `<title>` line (`generate.py`).
fn normalise_map(text: &str) -> String {
    let text = normalise(text);
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);

        if body.starts_with("<title>SVG Image created as ") && body.ends_with(" </title>") {
            if let Some(i) = body.rfind(" date ") {
                out.push_str(&body[..i + 6]);
                out.push_str("<date> </title>");
                if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }

        out.push_str(line);
    }

    out
}

/// The maps of one port run (names as stdout reports them, without the output directory),
/// with the hash of the normalised contents and the contents.
type PortFiles = Vec<(String, String, String)>;

fn run_port(board: &Board, set: &str) -> PortFiles {
    let mut rep = VecReporter::default();
    let out = job_export_drill(
        BoardCtx::bare(board),
        &cli_job(set),
        "out/",
        &drill_env(),
        None,
        Some(&mut rep),
    );
    assert!(out.success, "{}: {:?}", board.file_name, out.error);

    let mut files = Vec::new();

    for (msg, _) in &rep.messages {
        let Some(name) = msg
            .strip_prefix("Created file 'out/")
            .and_then(|m| m.strip_suffix("'."))
        else {
            continue;
        };

        if !name.contains("-drl_map.") {
            continue;
        }

        let file = out
            .files
            .iter()
            .find(|f| f.name == format!("out/{name}"))
            .unwrap_or_else(|| panic!("{name} reported but not produced"));
        let text = normalise_map(&String::from_utf8(file.contents.clone()).unwrap());
        files.push((name.to_string(), fnv(&text)[..8].to_string(), text));
    }

    files
}

/// Compares a port run with a golden run; writes the differing files to `dump` if set.
fn same(run: &Run, got: &PortFiles, dump: Option<&Path>) -> bool {
    let names_got: Vec<&str> = got.iter().map(|f| f.0.as_str()).collect();
    let names_exp: Vec<&str> = run.files.iter().map(|f| f.0.as_str()).collect();

    if names_got != names_exp {
        println!(
            "{} [{}]: maps differ:\n  port {names_got:?}\n  cli  {names_exp:?}",
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
                let stem = Path::new(&run.path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let _ = std::fs::write(dir.join(format!("{stem}-{}-{name}", run.set)), text);
            }
        }
    }

    ok
}

fn kicad_root() -> std::path::PathBuf {
    kicad_qa::kicad_source_root()
}

#[test]
fn corpus_maps_match_cli() {
    let runs = corpus();
    let dump = std::env::var_os("KICAD_FAB_DRILL_MAP_DUMP").map(std::path::PathBuf::from);
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
        "drill map corpus: {} runs: {} identical ({} maps), {} pending, {} board not loadable, \
         {} kicad-cli failures, {} differ",
        score.runs,
        score.identical,
        score.identical_files,
        score.pending,
        score.not_loadable,
        score.cli_failed,
        score.differ.len()
    );

    for (reason, n) in &pending_reasons {
        println!("  pending x{n}: {reason}");
    }

    assert!(score.differ.is_empty(), "differ: {:#?}", score.differ);
}

/// The full normalised maps kept for crafted boards (`generate.py` `KEEP`:
/// `golden/drill_map/<board stem>/<set>/`): first differing line on failure.
#[test]
fn crafted_maps_full_text() {
    let root = kicad_root();
    let golden = root.join("rust/crates/kicad-fab/tests/golden/drill_map");
    let data = root.join("rust/crates/kicad-fab/tests/data/drill_map");
    let mut compared = 0;

    let mut stems: Vec<_> = std::fs::read_dir(&golden)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    stems.sort();

    for stem in stems {
        let board = load_board(&data.join(format!("{stem}.kicad_pcb"))).unwrap();
        let mut sets: Vec<_> = std::fs::read_dir(golden.join(&stem))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        sets.sort();

        for set in sets {
            let dir = golden.join(&stem).join(&set);
            let got = run_port(&board, &set);
            let mut expected: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            expected.sort();
            let mut names: Vec<String> = got.iter().map(|f| f.0.clone()).collect();
            names.sort();
            assert_eq!(names, expected, "{stem} [{set}]");

            for (name, _, text) in &got {
                let exp = std::fs::read_to_string(dir.join(name)).unwrap();

                if &exp != text {
                    let first = exp
                        .lines()
                        .zip(text.lines())
                        .position(|(a, b)| a != b)
                        .unwrap_or(exp.lines().count().min(text.lines().count()));
                    panic!(
                        "{stem} [{set}] {name}: first difference at line {}:\n  cli  {:?}\n  port {:?}",
                        first + 1,
                        exp.lines().nth(first),
                        text.lines().nth(first)
                    );
                }

                compared += 1;
            }
        }
    }

    assert!(compared > 0);
}

/// PDF, PostScript and DXF maps have no plot engine in the port: the drill files are kept,
/// the failure message and `Done.` are reported and the job fails with
/// `FabError::Unsupported` naming the format.
#[test]
fn unsupported_map_formats() {
    let root = kicad_root();
    let board = load_board(&root.join("qa/data/cli/basic_test/basic_test.kicad_pcb")).unwrap();

    for (format, name, ext) in [
        (MapFormat::Pdf, "PDF", "pdf"),
        (MapFormat::Postscript, "PostScript", "ps"),
        (MapFormat::Dxf, "DXF", "dxf"),
    ] {
        let job = JobExportPcbDrill {
            map_format: format,
            ..cli_job("exc_gbr")
        };
        let mut rep = VecReporter::default();
        let out = job_export_drill(
            BoardCtx::bare(&board),
            &job,
            "out/",
            &drill_env(),
            None,
            Some(&mut rep),
        );

        assert!(!out.success);
        match &out.error {
            Some(FabError::Unsupported(msg)) => assert!(msg.contains(name), "{msg}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(out.files.len(), 1, "the drill file is kept");
        let msgs: Vec<&str> = rep.messages.iter().map(|m| m.0.as_str()).collect();
        assert_eq!(
            msgs[msgs.len() - 2],
            format!("Failed to create file 'out/basic_test-drl_map.{ext}'.")
        );
        assert_eq!(msgs[msgs.len() - 1], "Done.");
    }
}
