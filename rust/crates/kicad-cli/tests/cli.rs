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

//! Integration tests of `kicad-rs` against the reference `kicad-cli` (`docs/ORACLE.md`).
//!
//! * Argument handling: exit code and the first stdout line (argparse's message) of every
//!   error case equal the oracle's; the expected values are also written down here so the
//!   tests run without the oracle.
//! * Board jobs on corpus boards: while a needed subsystem is missing, `kicad-rs` must exit
//!   with `NOT_YET_PORTED` and name it on stderr ([`Outcome::Pending`]). As soon as a job runs
//!   through, the same test compares exit code, stdout and the produced files with the oracle
//!   ([`compare_with_oracle`]); nothing needs to change in this file for that switch.
//!
//! The oracle writes side files next to its input, so every run works on copies in a private
//! scratch directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_kicad-rs");

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_in(dir: &Path, exe: &Path, args: &[&str]) -> Run {
    let out = Command::new(exe)
        .args(args)
        .current_dir(dir)
        .env("KICAD_ORACLE_HOME", dir.join(".oracle-home"))
        .output()
        .expect("run");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter(|l| !l.contains("schema file"))
            .map(|l| format!("{l}\n"))
            .collect(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("kicad-cli-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copy a corpus board (and its project / rules) into `dir`; returns the file name.
fn copy_board(dir: &Path, rel: &str) -> String {
    let src = kicad_qa::kicad_source_root().join(rel);
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::copy(&src, dir.join(&name)).unwrap();

    for ext in ["kicad_pro", "kicad_dru"] {
        let side = src.with_extension(ext);

        if side.is_file() {
            std::fs::copy(&side, dir.join(side.file_name().unwrap())).unwrap();
        }
    }

    name
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

#[test]
fn version_commands() {
    let dir = scratch("version");
    let exe = Path::new(BIN);

    let r = run_in(&dir, exe, &["version"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "10.99.0\n"));
    let r = run_in(&dir, exe, &["--version"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "10.99.0\n"));
    let r = run_in(&dir, exe, &["-v"]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "10.99.0\n"));
    let r = run_in(&dir, exe, &["version", "--format", "commit"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout.trim().len(), 40);
    let r = run_in(&dir, exe, &["version", "--format", "bogus"]);
    assert_eq!((r.code, r.stderr.as_str()), (1, "Invalid format\n"));
    let r = run_in(&dir, exe, &["version", "--format", "about"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("Version: 10.99.0-unknown, release build"));

    if let Some(oracle) = kicad_qa::oracle_cli() {
        for args in [
            &["version"][..],
            &["--version"],
            &["version", "--format", "commit"],
        ] {
            let a = run_in(&dir, exe, args);
            let b = run_in(&dir, &oracle, args);
            assert_eq!((a.code, &a.stdout), (b.code, &b.stdout), "{args:?}");
        }
    }
}

#[test]
fn help_and_bare_commands() {
    let dir = scratch("help");
    let exe = Path::new(BIN);

    assert_eq!(run_in(&dir, exe, &[]).code, 1);
    for args in [
        &["--help"][..],
        &["pcb"],
        &["pcb", "export"],
        &["fp"],
        &["pcb", "drc", "--help"],
        &["pcb", "drc", "--help", "x.kicad_pcb"],
        &["--version", "--help"],
        &["pcb", "export", "gerbers", "-h"],
    ] {
        let r = run_in(&dir, exe, args);
        assert_eq!(r.code, 0, "{args:?}");
        assert!(r.stdout.contains("Usage"), "{args:?}: {}", r.stdout);

        if let Some(oracle) = kicad_qa::oracle_cli() {
            assert_eq!(run_in(&dir, &oracle, args).code, 0, "oracle {args:?}");
        }
    }
}

/// Every option of the ported commands is accepted (names, short names, aliases).
#[test]
fn all_options_are_known() {
    let dir = scratch("options");
    let exe = Path::new(BIN);
    let help = |args: &[&str]| run_in(&dir, exe, args).stdout;

    let gerbers = help(&["pcb", "export", "gerbers", "--help"]);
    for opt in [
        "--output",
        "--layers",
        "--common-layers",
        "--cl",
        "--drawing-sheet",
        "--define-var",
        "--exclude-refdes",
        "--erd",
        "--exclude-value",
        "--ev",
        "--include-border-title",
        "--ibt",
        "--sketch-pads-on-fab-layers",
        "--sp",
        "--sketch-pad-numbers",
        "--spn",
        "--hide-DNP-footprints-on-fab-layers",
        "--hdnp",
        "--sketch-DNP-footprints-on-fab-layers",
        "--sdnp",
        "--crossout-DNP-footprints-on-fab-layers",
        "--cdnp",
        "--no-x2",
        "--no-netlist",
        "--subtract-soldermask",
        "--disable-aperture-macros",
        "--use-drill-file-origin",
        "--precision",
        "--no-protel-ext",
        "--check-zones",
        "--variant",
        "--board-plot-params",
    ] {
        assert!(gerbers.contains(opt), "gerbers {opt}");
    }

    let drill = help(&["pcb", "export", "drill", "--help"]);
    for opt in [
        "--format",
        "--drill-origin",
        "--excellon-zeros-format",
        "--excellon-oval-format",
        "--excellon-units",
        "-u",
        "--excellon-mirror-y",
        "--excellon-min-header",
        "--excellon-separate-th",
        "--generate-map",
        "--generate-report",
        "--report-path",
        "--generate-tenting",
        "--map-format",
        "--gerber-precision",
    ] {
        assert!(drill.contains(opt), "drill {opt}");
    }

    let svg = help(&["pcb", "export", "svg", "--help"]);
    for opt in [
        "--subtract-soldermask",
        "--mirror",
        "-m",
        "--theme",
        "-t",
        "--negative",
        "-n",
        "--black-and-white",
        "--page-size-mode",
        "--fit-page-to-board",
        "--exclude-drawing-sheet",
        "--drill-shape-opt",
        "--mode-single",
        "--mode-multi",
        "--scale",
        "--check-zones",
        "--variant",
    ] {
        assert!(svg.contains(opt), "svg {opt}");
    }

    let drc = help(&["pcb", "drc", "--help"]);
    for opt in [
        "--format",
        "--all-track-errors",
        "--schematic-parity",
        "--units",
        "--severity-all",
        "--severity-error",
        "--severity-warning",
        "--severity-exclusions",
        "--exit-code-violations",
        "--refill-zones",
        "--save-board",
        "-D",
    ] {
        assert!(drc.contains(opt), "drc {opt}");
    }

    assert!(help(&["pcb", "upgrade", "--help"]).contains("--force"));
    assert!(help(&["fp", "upgrade", "--help"]).contains("--force"));
}

/// Argument errors: (args, exit code, first stdout line, stderr). `B` is replaced by a copy
/// of a corpus board, `MISSING` stays a file that does not exist.
const ARG_CASES: &[(&[&str], i32, &str, &str)] = &[
    (
        &["pcb", "drc"],
        1,
        "input: 1 argument(s) expected. 0 provided.",
        "",
    ),
    (
        &["pcb", "drc", "-o"],
        1,
        "input: 1 argument(s) expected. 0 provided.",
        "",
    ),
    (
        &["pcb", "drc", "--bogus", "B"],
        1,
        "Unknown argument: --bogus",
        "",
    ),
    (
        &["pcb", "drc", "B", "extra"],
        1,
        "Maximum number of positional arguments exceeded, failed to parse 'extra'",
        "",
    ),
    (
        &["pcb", "drc", "--format", "json", "--format", "report", "B"],
        1,
        "Duplicate argument --format",
        "",
    ),
    (
        &["pcb", "export", "gerbers", "--format", "x", "B"],
        1,
        "Unknown argument: --format",
        "",
    ),
    (
        &["pcb", "export", "gerbers", "--precision", "abc", "B"],
        1,
        "Failed to parse 'abc' as decimal integer: pattern 'abc' not found",
        "",
    ),
    (
        &["pcb", "export", "svg", "--drill-shape-opt", "1.5", "B"],
        1,
        "Failed to parse '1.5' as decimal integer: pattern '1.5' does not match to the end",
        "",
    ),
    (
        &["pcb", "export", "svg", "--scale", "abc", "B"],
        1,
        "Failed to parse 'abc' as number: pattern 'abc' does not match to the end",
        "",
    ),
    (
        &["pcb", "foo"],
        1,
        "Failed to parse 'foo', did you mean 'drc'",
        "",
    ),
    (
        &["version", "extra"],
        1,
        "Zero positional arguments expected, did you mean --format VAR",
        "",
    ),
    // The oracle loses these stderr texts (wide-oriented stderr after wxLog, see
    // PORT_STATUS.md); only the exit code and stdout are compared with it.
    (
        &["pcb", "drc", "--units", "furlong", "B"],
        1,
        "",
        "Invalid units specified\n",
    ),
    (
        &["pcb", "drc", "--format", "xml", "B"],
        1,
        "",
        "Invalid report format\n",
    ),
    (&["pcb", "drc", "-D", "a=b", "-D", "c", "B"], 1, "", ""),
    (
        &["pcb", "export", "gerbers", "--precision", "7", "B"],
        1,
        "",
        "Gerber coordinate precision should be either 5 or 6\n",
    ),
    (
        &["pcb", "export", "gerbers", "MISSING.kicad_pcb"],
        3,
        "",
        "Board file does not exist or is not accessible\n",
    ),
    (
        &["pcb", "export", "drill", "--format", "foo", "B"],
        1,
        "",
        "Invalid drill format\n",
    ),
    (
        &["pcb", "export", "drill", "-u", "cm", "B"],
        1,
        "",
        "Invalid units specified\n",
    ),
    (
        &[
            "pcb",
            "export",
            "drill",
            "--excellon-zeros-format",
            "x",
            "B",
        ],
        1,
        "",
        "Invalid zeros format specified\n",
    ),
    (
        &["pcb", "export", "drill", "--excellon-oval-format", "x", "B"],
        1,
        "",
        "Invalid oval drill format specified\n",
    ),
    (
        &["pcb", "export", "drill", "--map-format", "x", "B"],
        1,
        "",
        "Invalid map format specified\n",
    ),
    (
        &["pcb", "export", "drill", "--drill-origin", "x", "B"],
        1,
        "",
        "Invalid origin mode specified\n",
    ),
    (
        &["pcb", "export", "drill", "--gerber-precision", "4", "B"],
        1,
        "",
        "Gerber coordinate precision should be either 5 or 6\n",
    ),
    (
        &["pcb", "drc", "MISSING.kicad_pcb"],
        3,
        "",
        "Failed to load board\n",
    ),
    (
        &["pcb", "upgrade", "MISSING.kicad_pcb"],
        3,
        "",
        "Board file does not exist or is not accessible\n",
    ),
    (
        &["pcb", "export", "svg", "MISSING.kicad_pcb"],
        3,
        "",
        "Board file does not exist or is not accessible\n",
    ),
];

#[test]
fn argument_errors() {
    let dir = scratch("args");
    let board = copy_board(&dir, "qa/data/pcbnew/issue7241.kicad_pcb");
    let exe = Path::new(BIN);

    for (args, code, line, stderr) in ARG_CASES {
        let args: Vec<&str> = args
            .iter()
            .map(|a| if *a == "B" { board.as_str() } else { a })
            .collect();
        let r = run_in(&dir, exe, &args);
        assert_eq!(r.code, *code, "{args:?}: {}{}", r.stdout, r.stderr);

        if !line.is_empty() {
            assert_eq!(first_line(&r.stdout), *line, "{args:?}");
        } else {
            assert_eq!(r.stdout, "", "{args:?}");
        }

        assert_eq!(r.stderr, *stderr, "{args:?}");

        if let Some(oracle) = kicad_qa::oracle_cli() {
            let o = run_in(&dir, &oracle, &args);
            assert_eq!(o.code, *code, "oracle {args:?}");
            assert_eq!(
                first_line(&o.stdout),
                first_line(&r.stdout),
                "oracle {args:?}"
            );
        }
    }
}

/// What a board job did.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// The job needs a subsystem that has not landed (named on stderr).
    Pending(String),
    /// The job ran through and matched the oracle (or no oracle is installed).
    Compared,
}

/// All files below `dir` except the inputs and the oracle's side files, with their contents.
fn produced_files(dir: &Path, inputs: &[&str]) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();

            if rel.starts_with(".oracle-home") {
                continue;
            }

            if p.is_dir() {
                stack.push(p);
            } else if !inputs.contains(&rel.as_str()) && !rel.ends_with(".kicad_prl") {
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }

    out
}

/// Run `args` (with `B` = the board) with `kicad-rs`; while the job is pending assert the
/// pending contract, else compare exit code, stdout and the produced files with the oracle.
fn compare_with_oracle(name: &str, board_rel: &str, args: &[&str]) -> Outcome {
    let mine = scratch(&format!("{name}-rs"));
    let board = copy_board(&mine, board_rel);
    let args: Vec<&str> = args
        .iter()
        .map(|a| if *a == "B" { board.as_str() } else { a })
        .collect();
    let inputs = [board.as_str()];
    let r = run_in(&mine, Path::new(BIN), &args);

    if r.stderr.contains("kicad-rs: not yet ported: ") {
        assert_eq!(
            r.code,
            kicad_session::exit_codes::NOT_YET_PORTED,
            "{args:?}"
        );
        let subsystem = r.stderr.lines().last().unwrap_or("").to_string();
        return Outcome::Pending(subsystem);
    }

    let Some(oracle) = kicad_qa::oracle_cli() else {
        return Outcome::Compared;
    };

    let theirs = scratch(&format!("{name}-oracle"));
    copy_board(&theirs, board_rel);
    let o = run_in(&theirs, &oracle, &args);

    assert_eq!(r.code, o.code, "{args:?} exit code");
    assert_eq!(r.stdout, o.stdout, "{args:?} stdout");
    // Generated files carry dates and versions; the normalising comparison per format belongs
    // to the fab / drc packages. Until then the set of files must match.
    let a = produced_files(&mine, &inputs);
    let b = produced_files(&theirs, &inputs);
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "{args:?} files"
    );
    // The input board itself: unchanged by the export jobs, re-saved in place by
    // `pcb upgrade` (the writer has no dates, so the bytes must be equal).
    assert!(
        std::fs::read(mine.join(&board)).unwrap() == std::fs::read(theirs.join(&board)).unwrap(),
        "{args:?} board file after the run"
    );
    Outcome::Compared
}

const BOARDS: &[&str] = &[
    // Parsed and written in full today: `pcb upgrade` is compared byte for byte.
    "qa/data/pcbnew/issue10697.kicad_pcb",
    "qa/data/pcbnew/issue7241.kicad_pcb",
    "qa/data/pcbnew/via_stacks.kicad_pcb",
    "demos/pic_programmer/pic_programmer.kicad_pcb",
];

#[test]
fn board_jobs_pending_or_equal() {
    let jobs: &[(&str, &[&str])] = &[
        ("drc", &["pcb", "drc", "B"]),
        (
            "drc-json",
            &["pcb", "drc", "--format", "json", "--units", "mils", "B"],
        ),
        ("gerbers", &["pcb", "export", "gerbers", "-o", "gbr", "B"]),
        (
            "gerbers-layers",
            &[
                "pcb",
                "export",
                "gerbers",
                "-o",
                "gbr",
                "-l",
                "F.Cu,Edge.Cuts",
                "B",
            ],
        ),
        ("drill", &["pcb", "export", "drill", "-o", "drl", "B"]),
        (
            "drill-gerber",
            &[
                "pcb", "export", "drill", "--format", "gerber", "-o", "drl", "B",
            ],
        ),
        (
            "svg",
            &[
                "pcb",
                "export",
                "svg",
                "--mode-multi",
                "-l",
                "F.Cu",
                "-o",
                "svg",
                "B",
            ],
        ),
        ("upgrade", &["pcb", "upgrade", "--force", "B"]),
        ("upgrade-noforce", &["pcb", "upgrade", "B"]),
    ];

    for (i, board) in BOARDS.iter().enumerate() {
        for (name, args) in jobs {
            let outcome = compare_with_oracle(&format!("{name}-{i}"), board, args);

            if let Outcome::Pending(subsystem) = &outcome {
                // A job that cannot run yet names the missing subsystem.
                assert!(subsystem.contains("kicad-"), "{name}: {subsystem}");
            }
        }
    }
}

#[test]
fn fp_upgrade_pending_or_equal() {
    let dir = scratch("fp-upgrade");
    let lib = kicad_qa::kicad_source_root().join("qa/data/pcbnew/plugins/kicad_sexpr/fp.pretty");
    let copy = dir.join("fp.pretty");
    std::fs::create_dir_all(&copy).unwrap();

    for e in std::fs::read_dir(&lib).unwrap().flatten() {
        std::fs::copy(e.path(), copy.join(e.file_name())).unwrap();
    }

    let r = run_in(
        &dir,
        Path::new(BIN),
        &["fp", "upgrade", "-o", "out.pretty", "fp.pretty"],
    );

    if r.stderr.contains("not yet ported") {
        assert_eq!(r.code, kicad_session::exit_codes::NOT_YET_PORTED);
    } else {
        assert_eq!(r.code, 0, "{}", r.stderr);
    }

    // The checks that need no parser behave like the C++.
    let r = run_in(
        &dir,
        Path::new(BIN),
        &["fp", "upgrade", "-o", "fp.pretty", "fp.pretty"],
    );
    assert_eq!(
        (r.code, r.stderr.as_str()),
        (4, "Output path must not conflict with existing path\n")
    );
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let r = run_in(&dir, Path::new(BIN), &["fp", "upgrade", "empty"]);
    assert_eq!(
        (r.code, r.stderr.as_str()),
        (
            4,
            "Output path must be specified to convert legacy and non-KiCad libraries\n"
        )
    );

    if let Some(oracle) = kicad_qa::oracle_cli() {
        let o = run_in(
            &dir,
            &oracle,
            &["fp", "upgrade", "-o", "fp.pretty", "fp.pretty"],
        );
        assert_eq!(o.code, 4);
        let o = run_in(&dir, &oracle, &["fp", "upgrade", "empty"]);
        assert_eq!(o.code, 4);
    }
}
