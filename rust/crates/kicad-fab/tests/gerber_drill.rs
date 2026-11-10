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

//! FAB-08: Gerber drill files and IPC-4761 via protection files (`GERBER_WRITER`,
//! `pcbnew/exporters/gendrill_gerber_writer.cpp`) against the reference kicad-cli
//! (`kicad-cli pcb export drill --format gerber`, golden `tests/golden/gerber_drill/corpus.txt`
//! made by `generate.py` there: corpus boards under 300 KB and the crafted boards of
//! `tests/data/gerber_drill/`, three option sets).
//!
//! * `replay_of_oracle_hole_lists`: the C++ enumeration of the FAB-01 golden
//!   (`drill_oracle.txt`, `BOARD_LOADER::Load` like kicad-cli) for its ten corpus boards, with
//!   the source items (vias, pads) resolved on the parsed board, through the port's drill job:
//!   file names, order and contents must equal kicad-cli's.
//! * `corpus_boards_match_cli`: every golden board through the parser, the real enumeration
//!   and the drill job. Boards or parts reaching pending code are counted, not failed; any
//!   difference fails. `KICAD_FAB_GERBER_DRILL_DUMP=<dir>` writes the differing files there.
//! * `basic_test_golden`: `qa/data/cli/basic_test/basic_test-PTH-drl.gbr` as
//!   `qa/tests/cli/test_pcb.py` (`test_pcb_export_drill`, gerber case) checks it.

mod plot_common;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use kicad_core::Kiid;
use kicad_core::layer_ids::PcbLayerId;
use kicad_fab::VecReporter;
use kicad_fab::drill::{
    DrillOpKind, DrillOperation, DrillPostMachining, DrillSource, DrillSpan, HoleAttribute,
};
use kicad_fab::exporters::gendrill_report::{BoardHoleSource, HoleListSource};
use kicad_fab::exporters::gendrill_writer_base::GendrillWriterBase;
use kicad_fab::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillFormat, DrillOrigin, JobExportPcbDrill,
};
use kicad_fab::jobs_handler::job_export_drill_from;
use kicad_geom::{EdaAngle, Vector2I};
use kicad_pcb::padstack::PadDrillPostMachiningMode;
use kicad_pcb::{Board, BoardCtx};

use plot_common::{fnv, load_board, normalise};

const CORPUS: &str = include_str!("golden/gerber_drill/corpus.txt");
const FAB01_GOLDEN: &str = include_str!("golden/drill_oracle.txt");

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

/// `kicad-cli pcb export drill --format gerber <set>`.
fn cli_job(set: &str) -> JobExportPcbDrill {
    let mut job = JobExportPcbDrill {
        format: DrillFormat::Gerber,
        ..JobExportPcbDrill::default()
    };

    match set {
        "default" => {}
        "tenting" => job.generate_tenting = true,
        "p5_plot" => {
            job.gerber_precision = 5;
            job.drill_origin = DrillOrigin::Plot;
            job.generate_tenting = true;
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

/// The result of one port run: created file names (as stdout reports them, without the
/// output directory) with the hash of the normalised contents, and the contents.
type PortFiles = Vec<(String, String, String)>;

fn run_port(source: &dyn HoleListSource, board: &Board, set: &str) -> PortFiles {
    let mut rep = VecReporter::default();
    let out = job_export_drill_from(
        source,
        BoardCtx::bare(board),
        &cli_job(set),
        "out/",
        &drill_env(),
        None,
        Some(&mut rep),
    );
    assert!(out.success, "{}: {:?}", board.file_name, out.error);

    let created: Vec<String> = rep
        .messages
        .iter()
        .filter_map(|(m, _)| m.strip_prefix("Created file 'out/"))
        .map(|m| m.strip_suffix("'.").unwrap().to_string())
        .collect();
    assert_eq!(
        rep.messages.last().map(|m| m.0.as_str()),
        Some("Done."),
        "{}",
        board.file_name
    );

    // stdout order (Created file) and AddCreatedFile order agree for this writer except for
    // the backdrill pair file, which is reported before its span file is added.
    let mut files = Vec::new();

    for name in created {
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

// ---- replay of the FAB-01 oracle enumeration ------------------------------------------------

fn hole_attribute(v: i32) -> HoleAttribute {
    [
        HoleAttribute::HoleUnknown,
        HoleAttribute::HoleViaThrough,
        HoleAttribute::HoleViaBuried,
        HoleAttribute::HoleViaBackdrill,
        HoleAttribute::HolePad,
        HoleAttribute::HolePadCastellated,
        HoleAttribute::HolePadPressfit,
        HoleAttribute::HoleMechanical,
        HoleAttribute::HolePadBackdrill,
    ][v as usize]
}

fn pm_mode(v: i32) -> PadDrillPostMachiningMode {
    [
        PadDrillPostMachiningMode::Unknown,
        PadDrillPostMachiningMode::NotPostMachined,
        PadDrillPostMachiningMode::Counterbore,
        PadDrillPostMachiningMode::Countersink,
    ][v as usize]
}

/// The source ordinals of the board (`drill_oracle.cpp` `numberItems`): `V<n>` = index in
/// `Tracks()`, `P<n>` = running index over `Footprints()` x `Pads()`.
fn ordinals(board: &Board) -> BTreeMap<String, DrillSource> {
    let mut map = BTreeMap::new();

    for (n, (id, _)) in board.tracks().enumerate() {
        map.insert(format!("V{n}"), DrillSource::Via(id));
    }

    let mut n = 0;

    for (fp_id, _) in board.footprints() {
        for (pad_id, _) in board.footprint_pads(fp_id) {
            map.insert(format!("P{n}"), DrillSource::Pad(pad_id));
            n += 1;
        }
    }

    map
}

/// Parses the fields of a FAB-01 `raw` line (format of `tests/drill_oracle.rs`).
fn parse_op(f: &[&str], sources: &BTreeMap<String, DrillSource>) -> DrillOperation {
    let i = |k: usize| -> i32 { f[k].parse().unwrap() };
    let b = |k: usize| -> bool { i(k) != 0 };
    let l = |k: usize| -> PcbLayerId { PcbLayerId(i(k)) };
    assert_eq!(f.len(), 32, "op field count: {f:?}");
    assert!(!f[1].contains('!'), "oracle source uuid mismatch: {f:?}");
    DrillOperation {
        kind: [
            DrillOpKind::PrimaryDrill,
            DrillOpKind::SecondaryDrill,
            DrillOpKind::TertiaryDrill,
        ][i(0) as usize],
        source_item: if f[1] == "-" {
            None
        } else {
            Some(
                *sources
                    .get(f[1])
                    .unwrap_or_else(|| panic!("source {} not on the parsed board", f[1])),
            )
        },
        source_id: Kiid::default(),
        position: Vector2I::new(i(2), i(3)),
        size_xy: Vector2I::new(i(4), i(5)),
        orientation: EdaAngle::from_degrees(f[6].parse().unwrap()),
        diameter: i(7),
        is_slot: b(8),
        top_layer: l(9),
        bottom_layer: l(10),
        drill_start: l(11),
        drill_end: l(12),
        not_plated: b(13),
        attribute: hole_attribute(i(14)),
        filled: b(15),
        capped: b(16),
        top_covered: b(17),
        bottom_covered: b(18),
        top_plugged: b(19),
        bottom_plugged: b(20),
        top_tented: b(21),
        bottom_tented: b(22),
        front_post_machining: DrillPostMachining {
            mode: pm_mode(i(23)),
            size: i(24),
            depth: i(25),
            angle: i(26),
        },
        back_post_machining: DrillPostMachining {
            mode: pm_mode(i(27)),
            size: i(28),
            depth: i(29),
            angle: i(30),
        },
        stub_length: if f[31] == "-" {
            None
        } else {
            Some(f[31].parse().unwrap())
        },
    }
}

/// One board of the FAB-01 golden: file name, spans (NPTH pseudo span last), raw lines per
/// case (span index, merge, npth).
#[derive(Default)]
struct Fab01Board {
    file: String,
    spans: Vec<DrillSpan>,
    raw: BTreeMap<(usize, bool, bool), Vec<String>>,
}

fn fab01_boards() -> BTreeMap<String, Fab01Board> {
    let mut boards: BTreeMap<String, Fab01Board> = BTreeMap::new();
    let mut board = String::new();
    let mut case = (0, false, false);

    for line in FAB01_GOLDEN.lines() {
        let f: Vec<&str> = line.split('|').collect();
        match f[0] {
            "board" => {
                board = f[1].to_string();
                boards.insert(
                    board.clone(),
                    Fab01Board {
                        file: f[3].to_string(),
                        ..Fab01Board::default()
                    },
                );
            }
            "span" => {
                let i = |k: usize| -> i32 { f[k].parse().unwrap() };
                boards.get_mut(&board).unwrap().spans.push(DrillSpan::new(
                    PcbLayerId(i(2)),
                    PcbLayerId(i(3)),
                    i(4) != 0,
                    i(5) != 0,
                ));
            }
            "case" => {
                case = (f[1].parse().unwrap(), f[2] == "1", f[3] == "1");
                boards.get_mut(&board).unwrap().raw.insert(case, Vec::new());
            }
            "raw" => boards
                .get_mut(&board)
                .unwrap()
                .raw
                .get_mut(&case)
                .unwrap()
                .push(line[4..].to_string()),
            _ => {}
        }
    }

    boards
}

/// The hole lists recorded by the oracle, served as `EnumerateDrillSpans` / `buildHolesList`
/// serve them.
struct RecordedSource {
    spans: Vec<DrillSpan>,
    raw: BTreeMap<(usize, bool, bool), Vec<DrillOperation>>,
}

impl HoleListSource for RecordedSource {
    fn drill_spans(&self, _writer: &GendrillWriterBase<'_>) -> Vec<DrillSpan> {
        self.spans[..self.spans.len() - 1].to_vec()
    }

    fn build_holes_list(
        &self,
        writer: &mut GendrillWriterBase<'_>,
        span: &DrillSpan,
        generate_npth_list: bool,
    ) {
        let idx = self
            .spans
            .iter()
            .position(|s| s == span)
            .unwrap_or_else(|| panic!("span {span:?} not recorded"));
        let holes = self.raw[&(idx, writer.merge_pth_npth, generate_npth_list)].clone();
        writer.build_tool_list(holes);
    }
}

fn kicad_root() -> std::path::PathBuf {
    kicad_qa::kicad_source_root()
}

/// The ten corpus boards of the FAB-01 golden, replayed from the C++ enumeration for all
/// three option sets: identical to kicad-cli.
#[test]
fn replay_of_oracle_hole_lists() {
    let runs = corpus();
    let dump = std::env::var_os("KICAD_FAB_GERBER_DRILL_DUMP").map(std::path::PathBuf::from);
    let mut compared = 0;
    let mut files = 0;
    let mut failed = Vec::new();

    for (name, fb) in fab01_boards() {
        let Some(run0) = runs.iter().find(|r| {
            Path::new(&r.path).file_name().and_then(|n| n.to_str()) == Some(fb.file.as_str())
        }) else {
            continue; // the synthetic boards have no kicad-cli golden
        };

        let board = load_board(&kicad_root().join(&run0.path))
            .unwrap_or_else(|e| panic!("{name}: board not loadable: {e}"));
        let sources = ordinals(&board);
        let source = RecordedSource {
            spans: fb.spans.clone(),
            raw: fb
                .raw
                .iter()
                .map(|(case, lines)| {
                    let ops = lines
                        .iter()
                        .map(|l| parse_op(&l.split('|').collect::<Vec<_>>(), &sources))
                        .collect();
                    (*case, ops)
                })
                .collect(),
        };

        for run in runs.iter().filter(|r| r.path == run0.path) {
            assert_eq!(run.code, 0, "{}", run.path);
            let got = run_port(&source, &board, &run.set);
            files += got.len();

            if !same(run, &got, dump.as_deref()) {
                failed.push(format!("{name} [{}]", run.set));
            }

            compared += 1;
        }
    }

    println!(
        "replay: {compared} runs, {files} files compared, {} differ",
        failed.len()
    );
    assert_eq!(compared, 30, "10 boards x 3 option sets");
    assert!(failed.is_empty(), "differ: {failed:?}");
}

/// Every golden board through the parser, the real drill enumeration and the drill job.
#[test]
fn corpus_boards_match_cli() {
    let runs = corpus();
    let dump = std::env::var_os("KICAD_FAB_GERBER_DRILL_DUMP").map(std::path::PathBuf::from);
    let root = kicad_root();

    #[derive(Default)]
    struct Score {
        runs: usize,
        identical: usize,
        identical_files: usize,
        without_output: usize,
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

        match catch_unwind(AssertUnwindSafe(|| {
            run_port(&BoardHoleSource, board, &run.set)
        })) {
            Ok(got) => {
                if same(run, &got, dump.as_deref()) {
                    score.identical += 1;
                    score.identical_files += got.len();

                    if got.is_empty() {
                        score.without_output += 1;
                    }
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
        "gerber drill corpus: {} runs: {} identical ({} files; {} without holes), {} pending, \
         {} board not loadable, {} kicad-cli failures, {} differ",
        score.runs,
        score.identical,
        score.identical_files,
        score.without_output,
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

/// `qa/tests/cli/test_pcb.py::test_pcb_export_drill`, gerber case: the first 9 lines are
/// skipped there (version, date, project id); the remainder must be identical. Uses the
/// recorded C++ enumeration of `basic_test` (its vias reach the via model accessors).
#[test]
fn basic_test_golden() {
    let fb = fab01_boards().remove("basic_test").unwrap();
    let path = kicad_root().join("qa/data/cli/basic_test/basic_test.kicad_pcb");
    let board = load_board(&path).unwrap();
    let sources = ordinals(&board);
    let source = RecordedSource {
        spans: fb.spans.clone(),
        raw: fb
            .raw
            .iter()
            .map(|(case, lines)| {
                let ops = lines
                    .iter()
                    .map(|l| parse_op(&l.split('|').collect::<Vec<_>>(), &sources))
                    .collect();
                (*case, ops)
            })
            .collect(),
    };

    let got = run_port(&source, &board, "default");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "basic_test-PTH-drl.gbr");

    let expected =
        std::fs::read_to_string(kicad_root().join("qa/data/cli/basic_test/basic_test-PTH-drl.gbr"))
            .unwrap();
    let skip = |t: &str| t.lines().skip(9).collect::<Vec<_>>().join("\n");
    assert_eq!(skip(&got[0].2), skip(&expected));
}
