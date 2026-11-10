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

//! FAB-07 verification of `run_export_gerbers` (`PCBNEW_JOBS_HANDLER::JobExportGerbers` through
//! the session, as `kicad-rs pcb export gerbers` runs it) against the reference
//! `kicad-cli pcb export gerbers`:
//!
//! * `crafted_boards`: the crafted boards of kicad-fab (`tests/data/plot_driver`) x 7 option
//!   sets (golden `kicad-fab/tests/golden/plot_driver/crafted.txt`): file names, order and
//!   normalised contents, and the `Plotted to` messages.
//! * `cli_test_pcb_export_gerber`: the three cases of `qa/tests/cli/test_pcb.py`
//!   `test_pcb_export_gerber` against the goldens in `qa/data/cli/artwork_generation_regressions`
//!   (compared byte for byte after normalisation instead of the C++ test's rendered-image diff).
//! * `corpus_replay`: every board under `qa/data` and `demos` below 300 KB, with the CLI
//!   defaults and with `--no-x2 --use-drill-file-origin` (golden
//!   `tests/golden/gerbers_job/corpus.txt`, `generate.py`): exit code, messages and every
//!   file. Boards the port cannot load yet and plots that reach a pending stub are counted, not
//!   failed, so the test tightens as the port grows; `KICAD_GERBERS_JOB_REPORT=1` prints the
//!   per-board result.
//!
//! The Gerber job file (`.gbrjob`, FAB-09) is compared in `corpus_replay` (its `J` line), after
//! normalising the members `Header.GenerationSoftware.Version` and `Header.CreationDate`; the
//! corpus also holds the crafted boards of kicad-fab (`tests/data/plot_driver`,
//! `tests/data/gerber_jobfile`).

mod common;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use kicad_pcb::project::LoadPath;
use kicad_session::jobs::JobExportPcbGerbers;
use kicad_session::{JobOutcome, LoadInputs, Reporter, Session, exit_codes, run_export_gerbers};

fn kicad_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// `getBoard`: the board with its `.kicad_pro` / `.kicad_dru`, through the kicad-cli load
/// sequence (`BOARD_LOADER::Load`).
fn load(path: &Path) -> Result<Session, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let project = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let rules = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_str().unwrap();
    let inputs = LoadInputs {
        text: &text,
        source_name: name,
        project_json: project.as_deref(),
        rules_text: rules.as_deref(),
        ..LoadInputs::default()
    };

    match catch_unwind(AssertUnwindSafe(|| {
        common::load_staged(LoadPath::Cli, &inputs)
    })) {
        Ok(Ok((session, _))) => Ok(session),
        Ok(Err(e)) => Err(e.to_string()),
        Err(payload) => Err(format!("pending: {}", common::panic_text(payload.as_ref()))),
    }
}

/// `PCB_EXPORT_GERBER_COMMAND::populateJob` + `PCB_EXPORT_GERBERS_COMMAND::doPerform` for the
/// flags the goldens use, output directory `out/`.
fn cli_job(args: &[&str]) -> JobExportPcbGerbers {
    let mut job = JobExportPcbGerbers::new();
    let mut layers = String::new();
    let mut common_layers = String::new();
    let mut precision = 6;
    let flag = |f: &str| args.contains(&f);
    let mut it = args.iter();

    while let Some(&a) = it.next() {
        match a {
            "-l" | "--layers" => layers = it.next().unwrap().to_string(),
            "--cl" | "--common-layers" => common_layers = it.next().unwrap().to_string(),
            "--precision" => precision = it.next().unwrap().parse().unwrap(),
            _ => {}
        }
    }

    let g = &mut job.gerber;
    g.plot.job.set_configured_output_path("out/");
    g.plot.sketch_pads_on_fab_layers = false;
    g.plot.plot_pad_numbers = false;
    g.plot.hide_dnp_fps_on_fab_layers = false;
    g.plot.sketch_dnp_fps_on_fab_layers = false;
    g.plot.crossout_dnp_fps_on_fab_layers = false;
    g.plot.plot_footprint_values = true;
    g.plot.plot_ref_des = true;
    g.plot.plot_drawing_sheet = false;
    g.disable_aperture_macros = flag("--disable-aperture-macros");
    g.plot.subtract_solder_mask_from_silk = flag("--subtract-soldermask");
    g.include_netlist_attributes = !flag("--no-netlist");
    g.use_x2_format = !flag("--no-x2");
    g.plot.use_drill_origin = flag("--use-drill-file-origin");
    g.use_protel_file_extension = !flag("--no-protel-ext");
    g.precision = precision;
    g.plot.arg_layers = Some(layers);
    g.plot.arg_common_layers = Some(common_layers);
    job.use_board_plot_params = flag("--board-plot-params");
    job
}

/// What a run produced, ready to compare.
struct Run {
    exit_code: i32,
    /// The reporter text (C++ stdout), the FAB-09 message removed.
    messages: String,
    /// `(file name, normalised text)` of the Gerber files in order.
    files: Vec<(String, String)>,
    /// `(file name, normalised text)` of the Gerber job file (always the last file).
    job: Option<(String, String)>,
}

/// `run_export_gerbers`; `Err` when it reaches a pending part of the port.
fn run(session: &mut Session, args: &[&str]) -> Result<Run, String> {
    let mut job = cli_job(args);
    let mut reporter = Reporter::new();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_export_gerbers(session, &mut job, &mut reporter)
    }));

    let outcome: JobOutcome = match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(payload) => return Err(format!("pending: {}", common::panic_text(payload.as_ref()))),
    };

    let messages: String = reporter.lines().iter().map(|l| l.text.as_str()).collect();
    let exit_code = outcome.exit_code;
    let mut files = Vec::new();
    let mut job = None;

    for f in &outcome.files {
        let name = f.name.rsplit('/').next().unwrap().to_string();
        let text = std::str::from_utf8(&f.contents).unwrap();

        if name.ends_with(".gbrjob") {
            assert!(job.is_none(), "one Gerber job file");
            job = Some((name, normalise_job(text)));
        } else {
            assert!(job.is_none(), "the Gerber job file comes last");
            files.push((name, normalise(text)));
        }
    }

    Ok(Run {
        exit_code,
        messages,
        files,
        job,
    })
}

/// Replaces the lines carrying the version and the date by `<normalised>` (the rules of
/// kicad-fab's `tests/golden/plot_driver/generate.py`).
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let replace = (body.starts_with("%TF.GenerationSoftware,KiCad,Pcbnew,")
            && body.ends_with("*%"))
            || (body.starts_with("%TF.CreationDate,") && body.ends_with("*%"))
            || (body.starts_with("G04 Created by KiCad (PCBNEW ")
                && body.contains(") date ")
                && body.ends_with('*'))
            || (body.starts_with("G04 #@! TF.GenerationSoftware,") && body.ends_with('*'))
            || (body.starts_with("G04 #@! TF.CreationDate,") && body.ends_with('*'));

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

/// Replaces the Gerber job file members `"Version"` (only in `Header.GenerationSoftware`) and
/// `"CreationDate"` by `<normalised>` (the rule of `tests/golden/gerbers_job/generate.py`).
fn normalise_job(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .trim_start_matches(' ');
        let replace =
            body.starts_with("\"Version\": \"") || body.starts_with("\"CreationDate\": \"");

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

/// FNV-1a 64 as 16 hex digits.
fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in text.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The first 8 hex digits of [`fnv`].
fn h8(text: &str) -> String {
    fnv(text)[..8].to_string()
}

/// The option sets of kicad-fab's `tests/golden/plot_driver/generate.py`.
const CRAFTED_LAYERS: &str = "F.Cu,In1.Cu,B.Cu,F.Mask,F.Paste,F.SilkS,Edge.Cuts,User.1";

fn crafted_sets() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("default", vec![]),
        ("no_x2", vec!["--no-x2", "-l", CRAFTED_LAYERS]),
        ("prec5", vec!["--precision", "5", "-l", CRAFTED_LAYERS]),
        ("no_protel", vec!["--no-protel-ext", "-l", CRAFTED_LAYERS]),
        (
            "drill_origin",
            vec!["--use-drill-file-origin", "-l", CRAFTED_LAYERS],
        ),
        ("board_params", vec!["--board-plot-params"]),
        ("common", vec!["-l", "F.Cu,B.Cu", "--cl", "Edge.Cuts,F.Fab"]),
    ]
}

/// `(board stem, set)` -> `[(file name, normalised text)]` of kicad-fab's `crafted.txt`.
fn crafted_goldens() -> BTreeMap<(String, String), Vec<(String, String)>> {
    let text = std::fs::read_to_string(
        kicad_dir().join("rust/crates/kicad-fab/tests/golden/plot_driver/crafted.txt"),
    )
    .unwrap();
    let mut cases: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    let mut current: Option<(String, String)> = None;

    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("#case ") {
            let (stem, set) = rest.trim_end().rsplit_once(' ').unwrap();
            let key = (stem.to_string(), set.to_string());
            cases.insert(key.clone(), Vec::new());
            current = Some(key);
        } else if let Some(file) = line.strip_prefix("#file ") {
            let files = cases.get_mut(current.as_ref().unwrap()).unwrap();
            files.push((file.trim_end().to_string(), String::new()));
        } else {
            let files = cases.get_mut(current.as_ref().unwrap()).unwrap();
            files.last_mut().unwrap().1.push_str(line);
        }
    }

    cases
}

/// The `Plotted to` messages of `files` written into `out/`.
fn plotted_messages(files: &[(String, String)]) -> String {
    files
        .iter()
        .map(|(name, _)| format!("Plotted to 'out/{name}'.\n"))
        .collect()
}

#[test]
fn crafted_boards() {
    let goldens = crafted_goldens();
    assert_eq!(goldens.len(), 4 * 7, "4 boards x 7 option sets");
    let mut compared = 0;

    for ((stem, set), expected) in &goldens {
        let path = kicad_dir().join(format!(
            "rust/crates/kicad-fab/tests/data/plot_driver/{stem}.kicad_pcb"
        ));
        let args = crafted_sets()
            .into_iter()
            .find(|(name, _)| name == set)
            .unwrap()
            .1;
        let mut session = load(&path).unwrap_or_else(|e| panic!("{stem}: {e}"));
        let got = run(&mut session, &args).unwrap_or_else(|e| panic!("{stem} {set}: {e}"));

        assert_eq!(got.exit_code, exit_codes::OK, "{stem} {set}");
        assert_eq!(got.messages, plotted_messages(expected), "{stem} {set}");
        assert_eq!(got.files.len(), expected.len(), "{stem} {set}");

        for ((name, text), (ename, etext)) in got.files.iter().zip(expected) {
            assert_eq!(name, ename, "{stem} {set}");
            assert!(text == etext, "{stem} {set} {name}: contents differ");
            compared += 1;
        }
    }

    assert_eq!(compared, 247);
}

/// `qa/tests/cli/test_pcb.py::test_pcb_export_gerber`: `--no-x2 --use-drill-file-origin
/// --layers <layer>`, exactly one Gerber file. The C++ test compares the rendered images with
/// the goldens next to the board; here the file is compared byte for byte (normalised) with the
/// reference kicad-cli output: `Issue24143-F_Cu.gbr` is that output, the `ZoneFill-4.0.7`
/// goldens come from an older KiCad (other X1 attributes), so their reference output is
/// checked by its FNV-1a 64 (computed with `generate.py`'s `normalise` / `fnv`).
#[test]
fn cli_test_pcb_export_gerber() {
    let dir = kicad_dir().join("qa/data/cli/artwork_generation_regressions");
    let cases = [
        (
            "ZoneFill-4.0.7",
            "F.Cu",
            "ZoneFill-4.0.7-Dessus.gtl",
            Some("cf848e47fb9a9490"),
        ),
        (
            "ZoneFill-4.0.7",
            "B.Cu",
            "ZoneFill-4.0.7-Dessous.gbl",
            Some("520e4392acae07c1"),
        ),
        ("Issue24143", "F.Cu", "Issue24143-F_Cu.gtl", None),
    ];

    for (stem, layer, file_name, reference_hash) in cases {
        let mut session = load(&dir.join(format!("{stem}.kicad_pcb"))).unwrap();
        let got = run(
            &mut session,
            &["--no-x2", "--use-drill-file-origin", "--layers", layer],
        )
        .unwrap();

        assert_eq!(got.exit_code, exit_codes::OK);
        assert_eq!(got.files.len(), 1, "{stem} {layer}");
        assert_eq!(got.files[0].0, file_name);
        assert_eq!(got.messages, plotted_messages(&got.files));

        match reference_hash {
            Some(hash) => assert_eq!(fnv(&got.files[0].1), hash, "{stem} {layer}"),
            None => {
                let golden_path = dir.join(format!("{stem}-{}.gbr", layer.replace('.', "_")));
                let golden = normalise(&std::fs::read_to_string(golden_path).unwrap());
                assert!(
                    got.files[0].1 == golden,
                    "{stem} {layer}: differs from the golden"
                );
            }
        }
    }

    // The third C++ case, ZoneFill-Legacy.brd, needs the legacy board reader
    // (PCB_IO_KICAD_LEGACY), which is not part of the port.
}

/// One `B` block of `corpus.txt`.
struct CorpusCase {
    set: String,
    exit_code: i32,
    stdout_hash: String,
    path: String,
    files: Vec<String>,
    /// The hash of the normalised Gerber job file (`J` line), when the C++ wrote one.
    job: Option<String>,
}

fn corpus_cases() -> Vec<CorpusCase> {
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/gerbers_job/corpus.txt"),
    )
    .unwrap();
    let mut cases: Vec<CorpusCase> = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("B ") {
            let mut parts = rest.splitn(4, ' ');
            cases.push(CorpusCase {
                set: parts.next().unwrap().to_string(),
                exit_code: parts.next().unwrap().parse().unwrap(),
                stdout_hash: parts.next().unwrap().to_string(),
                path: parts.next().unwrap().to_string(),
                files: Vec::new(),
                job: None,
            });
        } else if let Some(hash) = line.strip_prefix("J ") {
            cases.last_mut().unwrap().job = Some(hash.to_string());
        } else {
            cases.last_mut().unwrap().files.push(line.to_string());
        }
    }

    cases
}

fn corpus_args(set: &str) -> Vec<&'static str> {
    match set {
        "default" => vec![],
        "nox2_drill" => vec!["--no-x2", "--use-drill-file-origin"],
        _ => panic!("unknown set {set}"),
    }
}

/// `(board path, set, file index)` whose difference is explained in `PORT_STATUS.md`
/// (a model or plot gap of another package); remove an entry when it starts matching.
const KNOWN_DIFFERENCES: &[(&str, &str, usize)] = &[
    // barcode without symbol encoder (model, M25a); kicad-fab plot_driver_corpus.rs lists the
    // same layers
    (
        "qa/data/downgrade/regressions/project_barcode/project_barcode.kicad_pcb",
        "default",
        6,
    ),
    (
        "qa/data/downgrade/regressions/project_barcode/project_barcode.kicad_pcb",
        "nox2_drill",
        6,
    ),
    (
        "qa/data/pcbnew/issue24525/issue24525.kicad_pcb",
        "default",
        14,
    ),
    (
        "qa/data/pcbnew/issue24525/issue24525.kicad_pcb",
        "nox2_drill",
        14,
    ),
];

#[test]
fn corpus_replay() {
    let report = std::env::var_os("KICAD_GERBERS_JOB_REPORT").is_some();
    let cases = corpus_cases();
    assert!(cases.len() >= 700, "{} cases", cases.len());

    let (mut not_loadable, mut pending, mut runs_matched) = (0, 0, 0);
    let (mut files_matched, mut files_known, mut jobs_matched) = (0, 0, 0);
    let mut failures: Vec<String> = Vec::new();
    let mut sessions: BTreeMap<String, Option<Session>> = BTreeMap::new();

    for case in &cases {
        let entry = sessions.entry(case.path.clone()).or_insert_with(|| {
            // One session per board: drop the previous ones to bound memory.
            None
        });

        if entry.is_none() {
            *entry = load(&kicad_dir().join(&case.path)).ok();
        }

        let Some(session) = entry.as_mut() else {
            not_loadable += 1;
            if report {
                println!("{} {}: not loadable", case.path, case.set);
            }
            continue;
        };

        let got = match run(session, &corpus_args(&case.set)) {
            Ok(got) => got,
            Err(e) => {
                pending += 1;
                if report {
                    println!(
                        "{} {}: {}",
                        case.path,
                        case.set,
                        e.lines().next().unwrap_or("")
                    );
                }
                continue;
            }
        };

        let mut ok = true;

        if got.exit_code != case.exit_code {
            failures.push(format!(
                "{} {}: exit code {} != {}",
                case.path, case.set, got.exit_code, case.exit_code
            ));
            ok = false;
        }

        if h8(&got.messages) != case.stdout_hash {
            failures.push(format!(
                "{} {}: messages differ:\n{}",
                case.path, case.set, got.messages
            ));
            ok = false;
        }

        if got.files.len() != case.files.len() {
            failures.push(format!(
                "{} {}: {} files != {}",
                case.path,
                case.set,
                got.files.len(),
                case.files.len()
            ));
            ok = false;
        }

        for (i, ((name, text), expected)) in got.files.iter().zip(&case.files).enumerate() {
            if h8(text) == *expected {
                files_matched += 1;
            } else if KNOWN_DIFFERENCES
                .iter()
                .any(|&(p, s, n)| p == case.path && s == case.set && n == i)
            {
                files_known += 1;
                ok = false;
            } else {
                if let Some(dir) = std::env::var_os("KICAD_GERBERS_JOB_DUMP") {
                    let _ = std::fs::write(Path::new(&dir).join(name), text);
                }
                failures.push(format!("{} {} #{i} {name}: differs", case.path, case.set));
                ok = false;
            }
        }

        match (&got.job, &case.job) {
            (Some((name, text)), Some(expected)) => {
                if h8(text) == *expected {
                    jobs_matched += 1;
                } else {
                    if let Some(dir) = std::env::var_os("KICAD_GERBERS_JOB_DUMP") {
                        let _ = std::fs::write(Path::new(&dir).join(name), text);
                    }
                    failures.push(format!("{} {} {name}: differs", case.path, case.set));
                    ok = false;
                }
            }
            (None, None) => {}
            (got_job, expected) => {
                failures.push(format!(
                    "{} {}: job file {} / expected {}",
                    case.path,
                    case.set,
                    got_job.is_some(),
                    expected.is_some()
                ));
                ok = false;
            }
        }

        if ok {
            runs_matched += 1;
        }

        if report {
            println!(
                "{} {}: {}",
                case.path,
                case.set,
                if ok { "identical" } else { "DIFFERS" }
            );
        }

        // Both sets of a board are adjacent: free the board after its last set.
        if case.set == "nox2_drill" {
            sessions.insert(case.path.clone(), None);
        }
    }

    println!(
        "gerbers job corpus: {} runs, {not_loadable} not loadable, {pending} pending, \
         {runs_matched} identical runs; {files_matched} files identical, {files_known} known \
         differences, {jobs_matched} job files identical, {} failures",
        cases.len(),
        failures.len()
    );

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
