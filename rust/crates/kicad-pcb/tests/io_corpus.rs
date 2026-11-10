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

//! Corpus differential harness of the board reader / writer (package PCBIO-17,
//! `docs/analysis/pcb-io.md` 9.3 D2; driver: `rust/tools/io-corpus/run.sh`, see its README).
//!
//! * `corpus_scoreboard` (ignored; needs the oracle): for every corpus board O1 (oracle
//!   `pcb upgrade --force` of the board alone) and O2 (oracle re-save of O1) come from the cache
//!   of `kicad_qa::io_corpus`; the Rust parse + format of O1 with staging (pending constructs
//!   skipped) is compared with O2 and classified (`resave::Status`). Writes
//!   `rust/target/io-corpus/scoreboard.md`. With `IO_CORPUS_BOARD=<name>` only that board is
//!   processed and its report and the paths for `diff -u` are written instead; with
//!   `IO_CORPUS_UPDATE_GOLDEN=1` the golden set below is (re)written.
//! * `golden_staging`: the committed golden set (`tests/golden/io_corpus/`, O1 / O2 of a dozen
//!   small boards of different format versions with their expected status) is re-evaluated
//!   without the oracle; a board may improve but never fall below its recorded status.
//! * `strict_*`: per golden board, parse + format **without** staging must reproduce O2 byte
//!   for byte; ignored (naming the pending packages) while the board waits for packages.
//!
//! `rust_resave` is parse + format only: the post-load initialisation `kicad-cli` runs
//! (`BOARD_LOADER::initializeLoadedBoard`: connectivity, net propagation) is PCBIO-15 /
//! `kicad-connectivity`. Boards where it changes the output show up as `differs` until then.

#[path = "io_corpus/board_file_utils.rs"]
mod board_file_utils;
#[path = "io_corpus/resave.rs"]
mod resave;
#[path = "io_corpus/scoreboard.rs"]
mod scoreboard;

use std::path::{Path, PathBuf};

use kicad_qa::io_corpus::{
    CorpusBoard, OracleResult, ensure_oracle_cache, find_corpus_board, io_corpus_boards,
    io_corpus_dir, oracle_stamp,
};
use resave::{Status, evaluate, pending_summary, rust_resave};
use scoreboard::{BoardResult, caveats_of, render, render_board};

/// The golden set: (stem of the golden files, corpus board). Small boards (< 100 KB) chosen to
/// cover the format versions from 5.99 (2020) to the current development version, and the
/// main item kinds.
const GOLDEN_SET: &[(&str, &str)] = &[
    ("issue4257", "qa/data/pcbnew/issue4257.kicad_pcb"),
    ("custom_pads", "qa/data/pcbnew/custom_pads.kicad_pcb"),
    (
        "tracks_arcs_vias",
        "qa/data/pcbnew/tracks_arcs_vias.kicad_pcb",
    ),
    ("issue7241", "qa/data/pcbnew/issue7241.kicad_pcb"),
    ("issue10906", "qa/data/pcbnew/issue10906.kicad_pcb"),
    ("issue14412", "qa/data/pcbnew/issue14412.kicad_pcb"),
    ("sliver", "qa/data/pcbnew/sliver.kicad_pcb"),
    (
        "groups_load_save",
        "qa/data/pcbnew/groups_load_save.kicad_pcb",
    ),
    (
        "graphics_load_save_v20240108",
        "qa/data/pcbnew/graphics_load_save_v20240108.kicad_pcb",
    ),
    ("issue18839", "qa/data/pcbnew/issue18839.kicad_pcb"),
    (
        "padstacks_complex",
        "qa/data/pcbnew/padstacks_complex.kicad_pcb",
    ),
    (
        "EmptyZone",
        "qa/data/pcbnew/plugins/kicad_sexpr/Issue23125_EmptyZone/EmptyZone.kicad_pcb",
    ),
    ("via_dangling", "qa/data/pcbnew/via_dangling.kicad_pcb"),
];

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/io_corpus")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// ---- corpus scoreboard (oracle) -------------------------------------------------------------

/// Evaluate one corpus board from its cache entry.
fn board_result(board: &CorpusBoard, oracle: Option<&OracleResult>) -> BoardResult {
    let mut r = BoardResult {
        name: board.name.clone(),
        size: board.size,
        fixed_point: None,
        status: Status::Skipped,
        note: board.skip.clone(),
        eval: None,
        caveats: Vec::new(),
    };

    match oracle {
        None => {}
        Some(OracleResult::Failed { message }) => {
            r.status = Status::OracleError;
            r.note = Some(message.clone());
        }
        Some(OracleResult::Saved {
            o1,
            o2,
            fixed_point,
        }) => {
            let o1_text = read(o1);
            let o2_text = if *fixed_point {
                o1_text.clone()
            } else {
                read(o2)
            };
            let ev = evaluate(&o1_text, &o2_text, &board.file_name());
            r.fixed_point = Some(*fixed_point);
            r.status = ev.status;
            r.caveats = caveats_of(&o1_text);
            r.eval = Some(ev);
        }
    }

    r
}

/// Write the golden set from the oracle cache (`IO_CORPUS_UPDATE_GOLDEN=1`).
fn update_golden(boards: &[CorpusBoard], cache: &[(String, OracleResult)]) {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut expected = String::from(
        "# PCBIO-17 golden set: stem|corpus board|expected status|waiting for\n\
         # generated by rust/tools/io-corpus/run.sh --update-golden; a board may improve, never regress\n",
    );

    for (stem, name) in GOLDEN_SET {
        let board = find_corpus_board(boards, name).unwrap();
        let Some((
            _,
            OracleResult::Saved {
                o1,
                o2,
                fixed_point,
            },
        )) = cache.iter().find(|(n, _)| n == name)
        else {
            panic!("{name}: no oracle result");
        };

        let o1_text = read(o1);
        std::fs::write(dir.join(format!("{stem}.kicad_pcb")), &o1_text).unwrap();
        let o2_path = dir.join(format!("{stem}_o2.kicad_pcb"));
        let _ = std::fs::remove_file(&o2_path);

        let o2_text = if *fixed_point {
            o1_text.clone()
        } else {
            let t = read(o2);
            std::fs::write(&o2_path, &t).unwrap();
            t
        };

        let ev = evaluate(&o1_text, &o2_text, &board.file_name());
        expected.push_str(&format!(
            "{stem}|{name}|{}|{}\n",
            ev.status.label(),
            pending_summary(&ev)
        ));
    }

    std::fs::write(dir.join("expected.txt"), expected).unwrap();
    println!("golden set written to {}", dir.display());
}

/// The corpus run (see the module comment). Needs the oracle; skips silently without it.
#[test]
#[ignore = "corpus run with the oracle: rust/tools/io-corpus/run.sh"]
fn corpus_scoreboard() {
    let all = io_corpus_boards();
    let single = std::env::var("IO_CORPUS_BOARD")
        .ok()
        .filter(|s| !s.is_empty());

    let selected: Vec<&CorpusBoard> = match &single {
        Some(q) => vec![find_corpus_board(&all, q).unwrap_or_else(|e| panic!("{e}"))],
        None => all.iter().collect(),
    };

    let jobs = std::env::var("IO_CORPUS_JOBS")
        .ok()
        .and_then(|j| j.parse().ok())
        .unwrap_or(2);
    let progress = |n: usize, total: usize, name: &str| {
        eprintln!("[oracle {n}/{total}] {name}");
    };

    let Some(cache) = ensure_oracle_cache(&selected, jobs, &progress) else {
        println!("corpus_scoreboard: no oracle, skipped");
        return;
    };

    let cache = cache.expect("oracle cache");
    let cache: Vec<(String, OracleResult)> =
        cache.into_iter().map(|c| (c.name, c.result)).collect();
    let stamp = oracle_stamp().map(|s| s.stamp).unwrap_or_default();
    let out_dir = io_corpus_dir();

    let mut results = Vec::new();

    for board in &selected {
        let oracle = cache.iter().find(|(n, _)| *n == board.name).map(|(_, r)| r);
        results.push(board_result(board, oracle));
    }

    if single.is_some() {
        let r = &results[0];
        let report = render_board(r);
        println!("{report}");
        let dir = out_dir.join("board");
        std::fs::create_dir_all(&dir).unwrap();
        let flat = selected[0].flat_name();
        let mut env = format!("BOARD='{}'\n", r.name);

        if let Some((_, OracleResult::Saved { o1, o2, .. })) =
            cache.iter().find(|(n, _)| *n == r.name)
        {
            env.push_str(&format!("O1='{}'\nO2='{}'\n", o1.display(), o2.display()));
        }

        if let Some(text) = r.eval.as_ref().and_then(|e| e.rust_text.as_ref()) {
            let rust = dir.join(format!("{flat}.rust.kicad_pcb"));
            std::fs::write(&rust, text).unwrap();
            env.push_str(&format!("RUST='{}'\n", rust.display()));
        }

        std::fs::write(dir.join("last.env"), env).unwrap();
        std::fs::write(dir.join(format!("{flat}.report.txt")), report).unwrap();
        return;
    }

    let board_md = render(&results, &stamp);
    let path = out_dir.join("scoreboard.md");
    std::fs::write(&path, &board_md).unwrap();
    println!("{board_md}");
    println!("scoreboard written to {}", path.display());

    if std::env::var("IO_CORPUS_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        update_golden(&all, &cache);
    }
}

// ---- golden set (no oracle) -----------------------------------------------------------------

/// One line of `expected.txt`.
struct GoldenEntry {
    stem: String,
    name: String,
    status: Status,
    waiting_for: String,
}

fn golden_entries() -> Vec<GoldenEntry> {
    read(&golden_dir().join("expected.txt"))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.splitn(4, '|').collect();
            assert!(f.len() >= 3, "bad expected.txt line: {l}");
            GoldenEntry {
                stem: f[0].to_string(),
                name: f[1].to_string(),
                status: Status::from_label(f[2]).unwrap_or_else(|| panic!("bad status: {l}")),
                waiting_for: f.get(3).unwrap_or(&"").to_string(),
            }
        })
        .collect()
}

/// (O1, O2) of a golden board.
fn golden_texts(stem: &str) -> (String, String) {
    let dir = golden_dir();
    let o1 = read(&dir.join(format!("{stem}.kicad_pcb")));
    let o2_path = dir.join(format!("{stem}_o2.kicad_pcb"));
    let o2 = if o2_path.is_file() {
        read(&o2_path)
    } else {
        o1.clone()
    };
    (o1, o2)
}

/// The golden set with staging: every board reaches at least its recorded status.
#[test]
fn golden_staging() {
    let entries = golden_entries();
    assert_eq!(
        entries.len(),
        GOLDEN_SET.len(),
        "expected.txt and GOLDEN_SET disagree"
    );
    let mut regressions = Vec::new();

    for e in &entries {
        assert!(GOLDEN_SET.iter().any(|(s, n)| *s == e.stem && *n == e.name));
        let (o1, o2) = golden_texts(&e.stem);
        let ev = evaluate(&o1, &o2, &format!("{}.kicad_pcb", e.stem));
        let now = ev.status;
        println!(
            "{:<30} {:<26} (recorded {}) {}",
            e.stem,
            now.label(),
            e.status.label(),
            pending_summary(&ev)
        );

        if now.rank() < e.status.rank() {
            let diff = ev
                .masked_diff
                .as_ref()
                .or(ev.raw_diff.as_ref())
                .map(|d| format!("line {} ({}):\n{}", d.line, d.path_label(), d.excerpt))
                .or(ev.error.clone())
                .unwrap_or_default();
            regressions.push(format!(
                "{}: {} -> {} (recorded as waiting for: {}) {diff}",
                e.stem,
                e.status.label(),
                now.label(),
                e.waiting_for
            ));
        } else if now.rank() > e.status.rank() {
            println!(
                "  improved: update tests/golden/io_corpus/expected.txt (run.sh --update-golden){}",
                if now == Status::Identical {
                    " and remove the #[ignore] of its strict test"
                } else {
                    ""
                }
            );
        }
    }

    assert!(
        regressions.is_empty(),
        "regressions:\n{}",
        regressions.join("\n")
    );
}

/// Strict re-save of a golden board: parse + format without staging reproduce O2.
fn strict(stem: &str) {
    let (o1, o2) = golden_texts(stem);

    match rust_resave(&o1, &format!("{stem}.kicad_pcb"), false) {
        resave::Resave::Written { text, .. } => {
            if let Some(d) = kicad_qa::sexpr_text::first_difference(&o2, &text, 3) {
                panic!(
                    "{stem}: differs from the oracle at line {} ({}):\n{}",
                    d.line,
                    d.path_label(),
                    d.excerpt
                );
            }
        }
        other => panic!("{stem}: {other:?}"),
    }
}

// One strict test per golden board. The ignore reasons name the packages the board waits for
// (`expected.txt`, column 4); remove the #[ignore] once `golden_staging` reports it identical.

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_issue4257() {
    strict("issue4257");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_custom_pads() {
    strict("custom_pads");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_tracks_arcs_vias() {
    strict("tracks_arcs_vias");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_issue7241() {
    strict("issue7241");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_issue10906() {
    strict("issue10906");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_issue14412() {
    strict("issue14412");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_sliver() {
    strict("sliver");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_groups_load_save() {
    strict("groups_load_save");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_graphics_load_save_v20240108() {
    strict("graphics_load_save_v20240108");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_issue18839() {
    strict("issue18839");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_padstacks_complex() {
    strict("padstacks_complex");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_empty_zone() {
    strict("EmptyZone");
}

#[test]
#[ignore = "pending: see tests/golden/io_corpus/expected.txt"]
fn strict_via_dangling() {
    strict("via_dangling");
}

// ---- helper self-tests ----------------------------------------------------------------------

/// The `KI_TEST` helpers: `RequireBoardFromPcbnewQAData` with staging-free options fails on a
/// pending construct, the staging variant loads; a non-board file gives `None`.
#[test]
fn board_file_utils_helpers() {
    resave::quiet_pending_panics();
    let path = kicad_qa::board_file_utils::pcbnew_qa_data_path("issue18839.kicad_pcb");
    let staged = std::panic::catch_unwind(|| {
        board_file_utils::read_board_from_file_with(
            &path,
            &kicad_pcb::io::ParseOptions {
                allow_pending: true,
            },
        )
    });

    if let Ok(r) = staged {
        assert!(r.expect("staging parse").is_some());
    }

    assert!(matches!(
        board_file_utils::read_board_from_file(Path::new("/nonexistent/board.kicad_pcb")),
        Ok(None)
    ));
}
