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

//! FAB-11 verification of `run_export_svg` (`PCBNEW_JOBS_HANDLER::JobExportSvg` through the
//! session, as `kicad-rs pcb export svg` runs it: `PCB_PLOTTER::Plot`, drill marks, colour
//! themes) against the reference `kicad-cli pcb export svg`:
//!
//! * `corpus_replay`: every board under `qa/data` and `demos` below 300 KB and the crafted
//!   boards of kicad-fab with the set `default` (multi-file, 16 layers, colour, full drill
//!   marks), and five boards with every other option set (single file, common layers, black
//!   and white, negative, mirror, drill marks 0 / 1, page size modes 0 / 1 / 2, fit to board,
//!   classic and unknown colour theme, subtracted mask, scale, DNP cross-out, the drawing
//!   sheet). Golden `tests/golden/svg_job/corpus.txt` (`generate.py`): exit code, messages
//!   and every file after normalising the date of `<title>`. Boards the port cannot load yet
//!   and plots that reach a pending stub are counted, not failed; the `sheet` set stops with
//!   the documented `NotPorted` error of the drawing sheet (counted) or is compared. `KICAD_SVG_JOB_REPORT=1`
//!   prints the per-run result, `KICAD_SVG_JOB_DUMP=<dir>` writes the differing files.
//! * `cli_test_pcb_export_svg`: `qa/tests/cli/test_pcb.py` `test_pcb_export_svg` for
//!   `ZoneFill-4.0.7.kicad_pcb` (F.Cu and B.Cu, `--page-size-mode 1 --exclude-drawing-sheet
//!   --black-and-white`), compared byte for byte with the reference kicad-cli output (the sets
//!   `cli_test_*` of the corpus golden) instead of the C++ test's rendered-image comparison with
//!   the older committed `.svg` files. The `ZoneFill-Legacy.brd` case needs the legacy board
//!   reader (not in the port).

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use kicad_core::plot_enums::DrillMarks;
use kicad_core::report_severity::Severity;
use kicad_pcb::project::LoadPath;
use kicad_session::jobs::{JobExportPcbSvg, SvgGenMode};
use kicad_session::{JobOutcome, LoadInputs, Reporter, Session, run_export_svg};

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

/// `PCB_EXPORT_SVG_COMMAND::doPerform` for the flags the goldens use (the CLI defaults:
/// `--drill-shape-opt 2`, `--page-size-mode 0`, `--scale 1`); the output is `out/` or, in
/// single mode, `out/<board name>.svg`.
fn cli_job(args: &[&str], stem: &str) -> JobExportPcbSvg {
    let mut job = JobExportPcbSvg::new();
    let flag = |f: &str| args.contains(&f);
    let (mut layers, mut common_layers, mut theme) = (String::new(), String::new(), String::new());
    let (mut drill_shape, mut page_size, mut scale) = (2, 0, 1.0);
    let mut it = args.iter();

    while let Some(&a) = it.next() {
        match a {
            "-l" | "--layers" => layers = it.next().unwrap().to_string(),
            "--cl" | "--common-layers" => common_layers = it.next().unwrap().to_string(),
            "-t" | "--theme" => theme = it.next().unwrap().to_string(),
            "--drill-shape-opt" => drill_shape = it.next().unwrap().parse().unwrap(),
            "--page-size-mode" => page_size = it.next().unwrap().parse().unwrap(),
            "--scale" => scale = it.next().unwrap().parse().unwrap(),
            _ => {}
        }
    }

    let plot = &mut job.plot;
    plot.mirror = flag("--mirror");
    plot.black_and_white = flag("--black-and-white");
    plot.negative = flag("--negative");
    plot.sketch_pads_on_fab_layers = flag("--sp");
    plot.plot_pad_numbers = plot.sketch_pads_on_fab_layers && flag("--spn");
    plot.hide_dnp_fps_on_fab_layers = flag("--hdnp");
    plot.sketch_dnp_fps_on_fab_layers = flag("--sdnp");
    plot.crossout_dnp_fps_on_fab_layers = flag("--cdnp");
    plot.drill_shape_option = match drill_shape {
        0 => DrillMarks::NoDrillShape,
        1 => DrillMarks::SmallDrillShape,
        _ => DrillMarks::FullDrillShape,
    };
    plot.subtract_solder_mask_from_silk = flag("--subtract-soldermask");
    plot.scale = scale;
    job.fit_page_to_board = flag("--fit-page-to-board");

    // legacy compat, should eliminate this arg eventually
    if page_size == 0 {
        job.plot.plot_drawing_sheet = true;
    } else if page_size == 1 {
        job.plot.plot_drawing_sheet = false;
    } else if page_size == 2 {
        job.fit_page_to_board = true;
        job.plot.plot_drawing_sheet = false;
    }

    let single = flag("--mode-single");
    let plot = &mut job.plot;
    let out = if single {
        format!("out/{stem}.svg")
    } else {
        "out/".to_string()
    };
    plot.job.set_configured_output_path(&out);
    plot.color_theme = theme;
    plot.plot_drawing_sheet = !flag("--exclude-drawing-sheet");
    plot.arg_layers = Some(layers);
    plot.arg_common_layers = Some(common_layers);
    job.gen_mode = if single {
        SvgGenMode::Single
    } else {
        SvgGenMode::Multi
    };
    job
}

/// What a run produced, ready to compare.
struct Run {
    exit_code: i32,
    /// The reporter text as `CLI_REPORTER` prints it to stdout (a newline added where the
    /// message has none; errors go to stderr).
    messages: String,
    /// `(file name, normalised text)` in order.
    files: Vec<(String, String)>,
}

/// `run_export_svg`; `Err` when it reaches a pending part of the port.
fn run(session: &mut Session, args: &[&str], stem: &str) -> Result<Run, String> {
    let mut job = cli_job(args, stem);
    let mut reporter = Reporter::new();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_export_svg(session, &mut job, &mut reporter)
    }));

    let outcome: JobOutcome = match result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(payload) => return Err(format!("pending: {}", common::panic_text(payload.as_ref()))),
    };

    let mut messages = String::new();

    for line in reporter.lines() {
        if line.severity == Severity::Error || line.severity == Severity::Debug {
            continue;
        }

        messages.push_str(&line.text);

        if !line.text.ends_with('\n') {
            messages.push('\n');
        }
    }

    let files = outcome
        .files
        .iter()
        .map(|f| {
            (
                f.name.rsplit('/').next().unwrap().to_string(),
                normalise(std::str::from_utf8(&f.contents).unwrap()),
            )
        })
        .collect();

    Ok(Run {
        exit_code: outcome.exit_code,
        messages,
        files,
    })
}

/// Replaces the date of the `<title>` line by `<normalised>` (the rule of
/// `tests/golden/svg_job/generate.py`).
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);

        if body.starts_with("<title>SVG Image created as ")
            && body.ends_with(" </title>")
            && let Some(pos) = body.rfind(" date ")
        {
            out.push_str(&body[..pos + 6]);
            out.push_str("<normalised> </title>");
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

/// The option sets of `tests/golden/svg_job/generate.py`.
fn set_args(set: &str) -> Vec<&'static str> {
    const ALL: &str = "F.Cu,In1.Cu,B.Cu,F.Paste,B.Paste,F.SilkS,B.SilkS,F.Mask,B.Mask,Edge.Cuts,\
                       F.Fab,B.Fab,F.CrtYd,User.Drawings,User.Comments,User.1";
    let base = || -> Vec<&'static str> {
        vec![
            "--mode-multi",
            "--exclude-drawing-sheet",
            "-l",
            "F.Cu,B.Cu,F.Mask,F.SilkS,B.SilkS,Edge.Cuts,F.Fab",
        ]
    };
    let with = |extra: &[&'static str]| -> Vec<&'static str> {
        let mut v = base();
        v.extend_from_slice(extra);
        v
    };

    match set {
        "default" => vec!["--mode-multi", "--exclude-drawing-sheet", "-l", ALL],
        "single" => vec![
            "--mode-single",
            "--exclude-drawing-sheet",
            "-l",
            "F.Cu,B.Cu,F.SilkS,Edge.Cuts",
        ],
        "single_fit" => vec![
            "--mode-single",
            "--exclude-drawing-sheet",
            "--fit-page-to-board",
            "-l",
            "B.Cu,F.Fab",
        ],
        "common" => vec![
            "--mode-multi",
            "--exclude-drawing-sheet",
            "-l",
            "F.Cu,B.Cu",
            "--cl",
            "Edge.Cuts,F.Fab",
        ],
        "bw" => with(&["--black-and-white"]),
        "negative" => with(&["--negative"]),
        "mirror" => with(&["--mirror"]),
        "drill0" => with(&["--drill-shape-opt", "0"]),
        "drill1" => with(&["--drill-shape-opt", "1"]),
        "psm0" => with(&["--page-size-mode", "0"]),
        "psm1" => with(&["--page-size-mode", "1"]),
        "psm2" => with(&["--page-size-mode", "2"]),
        "fit" => with(&["--fit-page-to-board"]),
        "classic" => with(&["-t", "_builtin_classic"]),
        "unknown_theme" => with(&["-t", "no_such_theme"]),
        "subtract" => with(&["--subtract-soldermask"]),
        "scale" => with(&["--scale", "2"]),
        "cdnp" => with(&["--cdnp"]),
        "bw_negative_mirror" => with(&[
            "--black-and-white",
            "--negative",
            "--mirror",
            "--drill-shape-opt",
            "1",
        ]),
        "sheet" => vec!["--mode-multi", "-l", "F.Cu"],
        "cli_test_F.Cu" | "cli_test_B.Cu" => vec![
            "--mode-single",
            "--page-size-mode",
            "1",
            "--exclude-drawing-sheet",
            "--black-and-white",
            "--layers",
            if set == "cli_test_F.Cu" {
                "F.Cu"
            } else {
                "B.Cu"
            },
        ],
        _ => panic!("unknown set {set}"),
    }
}

/// One `B` block of `corpus.txt`.
struct CorpusCase {
    set: String,
    exit_code: i32,
    stdout_hash: String,
    path: String,
    files: Vec<String>,
}

fn corpus_cases() -> Vec<CorpusCase> {
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/svg_job/corpus.txt"),
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
            });
        } else {
            cases.last_mut().unwrap().files.push(line.to_string());
        }
    }

    cases
}

/// `(board path, set, file index)` whose difference is explained in `PORT_STATUS.md` (a model
/// or plot gap of another package); remove an entry when it starts matching.
const KNOWN_DIFFERENCES: &[(&str, &str, usize)] = &[];

/// The result of comparing one corpus run.
enum Verdict {
    NotLoadable,
    Pending(String),
    /// The drawing sheet: the documented `NotPorted` error.
    SheetNotPorted,
    Compared {
        ok: bool,
    },
}

#[derive(Default)]
struct Tally {
    not_loadable: usize,
    pending: usize,
    sheet: usize,
    runs_matched: usize,
    files_matched: usize,
    files_known: usize,
    failures: Vec<String>,
}

fn compare(case: &CorpusCase, session: Option<&mut Session>, tally: &mut Tally) -> Verdict {
    let Some(session) = session else {
        tally.not_loadable += 1;
        return Verdict::NotLoadable;
    };

    let stem = Path::new(&case.path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let got = match run(session, &set_args(&case.set), &stem) {
        Ok(got) => got,
        Err(e) => {
            if case.set == "sheet" && e.contains("PlotDrawingSheet") {
                tally.sheet += 1;
                return Verdict::SheetNotPorted;
            }

            tally.pending += 1;
            return Verdict::Pending(e.lines().next().unwrap_or("").to_string());
        }
    };

    // Once the drawing sheet is ported the `sheet` set is compared like the others.

    let mut ok = true;

    if got.exit_code != case.exit_code {
        tally.failures.push(format!(
            "{} {}: exit code {} != {}",
            case.path, case.set, got.exit_code, case.exit_code
        ));
        ok = false;
    }

    if h8(&got.messages) != case.stdout_hash {
        tally.failures.push(format!(
            "{} {}: messages differ:\n{}",
            case.path, case.set, got.messages
        ));
        ok = false;
    }

    if got.files.len() != case.files.len() {
        tally.failures.push(format!(
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
            tally.files_matched += 1;
        } else if KNOWN_DIFFERENCES
            .iter()
            .any(|&(p, s, n)| p == case.path && s == case.set && n == i)
        {
            tally.files_known += 1;
            ok = false;
        } else {
            if let Some(dir) = std::env::var_os("KICAD_SVG_JOB_DUMP") {
                let _ = std::fs::write(Path::new(&dir).join(format!("{}-{name}", case.set)), text);
            }
            tally
                .failures
                .push(format!("{} {} #{i} {name}: differs", case.path, case.set));
            ok = false;
        }
    }

    if ok {
        tally.runs_matched += 1;
    }

    Verdict::Compared { ok }
}

#[test]
fn corpus_replay() {
    let report = std::env::var_os("KICAD_SVG_JOB_REPORT").is_some();
    let cases = corpus_cases();
    assert!(cases.len() >= 400, "{} cases", cases.len());

    let mut tally = Tally::default();
    let mut current: Option<(String, Option<Session>)> = None;

    for case in &cases {
        // One session per board (the runs of a board are adjacent); each run plots into the
        // loaded board as kicad-cli does into a freshly loaded one, and the SVG job does not
        // modify the board apart from RefreshGeneratedTables, which is idempotent.
        if current
            .as_ref()
            .map(|(p, _)| p != &case.path)
            .unwrap_or(true)
        {
            current = Some((case.path.clone(), load(&kicad_dir().join(&case.path)).ok()));
        }

        let session = current.as_mut().unwrap().1.as_mut();
        let verdict = compare(case, session, &mut tally);

        if report {
            let text = match verdict {
                Verdict::NotLoadable => "not loadable".to_string(),
                Verdict::Pending(e) => e,
                Verdict::SheetNotPorted => "drawing sheet: NotPorted (documented)".to_string(),
                Verdict::Compared { ok } => (if ok { "identical" } else { "DIFFERS" }).to_string(),
            };
            println!("{} {}: {text}", case.path, case.set);
        }
    }

    println!(
        "svg job corpus: {} runs, {} not loadable, {} pending, {} drawing sheet (not ported), \
         {} identical runs; {} files identical, {} known differences, {} failures",
        cases.len(),
        tally.not_loadable,
        tally.pending,
        tally.sheet,
        tally.runs_matched,
        tally.files_matched,
        tally.files_known,
        tally.failures.len()
    );

    assert!(tally.failures.is_empty(), "{}", tally.failures.join("\n"));
}

#[test]
fn cli_test_pcb_export_svg() {
    let cases: Vec<CorpusCase> = corpus_cases()
        .into_iter()
        .filter(|c| c.set.starts_with("cli_test_"))
        .collect();
    assert_eq!(cases.len(), 2);

    let mut tally = Tally::default();
    let mut session = load(&kicad_dir().join(&cases[0].path)).ok();
    assert!(session.is_some(), "ZoneFill-4.0.7 loads");

    for case in &cases {
        match compare(case, session.as_mut(), &mut tally) {
            Verdict::Compared { ok: true } => {}
            Verdict::Pending(e) => println!("{}: pending: {e}", case.set),
            _ => {}
        }
    }

    assert!(tally.failures.is_empty(), "{}", tally.failures.join("\n"));
    assert_eq!(tally.runs_matched + tally.pending, 2);
}
