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

//! `kicad-rs pcb upgrade --force` (`Session::load_board`, the full `BOARD_LOADER::Load`
//! sequence, + `run_pcb_upgrade`) against the reference `kicad-cli pcb upgrade --force` over
//! the board corpus, through the oracle cache of the io-corpus harness
//! (`rust/tools/io-corpus`, `kicad_qa::io_corpus`): the input is the cached `O1` (the oracle's
//! re-save of the board alone, without project or rule file), the expectation `O2` (the
//! oracle's re-save of `O1`). Package PCBIO-15 (new code, test support).
//!
//! Ignored in the default run (it needs the oracle and its cache):
//! `cargo test -p kicad-session --test upgrade_corpus -- --ignored --nocapture`.
//! Writes `rust/target/io-corpus/session_scoreboard.md`.
//!
//! The parser runs strictly (no staging): a board with a construct whose parser package has
//! not landed is `not-loadable`. Steps reaching a known `port_pending!` stub are skipped
//! (`common::KNOWN_PENDING_STEPS`).

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_pcb::project::LoadPath;
use kicad_qa::io_corpus::{OracleResult, ensure_oracle_cache, io_corpus_boards, io_corpus_dir};
use kicad_qa::sexpr_text::first_difference;
use kicad_session::jobs::JobPcbUpgrade;
use kicad_session::{LoadInputs, Reporter, SessionError, run_pcb_upgrade};

/// The outcome for one board.
enum Outcome {
    Identical,
    /// First difference: keyword path, line, expected, actual; whether the Rust text equals O1.
    Differs {
        path: String,
        line: usize,
        expected: String,
        actual: String,
        matches_o1: bool,
    },
    NotLoadable(String),
    Failed(String),
}

fn upgrade(o1: &str, name: &str) -> Result<String, String> {
    let inputs = LoadInputs {
        text: o1,
        source_name: name,
        ..LoadInputs::default()
    };

    let (mut session, _) = common::load_staged(LoadPath::Cli, &inputs).map_err(|e| match e {
        SessionError::NotYetPorted { subsystem } => format!("not-loadable: {subsystem}"),
        other => other.to_string(),
    })?;
    let mut job = JobPcbUpgrade::new();
    job.filename = name.to_string();
    job.force = true;
    let mut reporter = Reporter::new();
    let outcome = run_pcb_upgrade(&mut session, &mut job, &mut reporter)
        .map_err(|e| format!("not-loadable: {e}"))?;

    match outcome.files.as_slice() {
        [f] => Ok(String::from_utf8_lossy(&f.contents).into_owned()),
        _ => Err(format!("no output: {}", reporter.text())),
    }
}

fn evaluate(o1: &str, o2: &str, name: &str) -> Outcome {
    let result = catch_unwind(AssertUnwindSafe(|| upgrade(o1, name)));

    match result {
        Ok(Ok(text)) => match first_difference(o2, &text, 2) {
            None => Outcome::Identical,
            Some(d) => Outcome::Differs {
                path: d.path.join(" > "),
                line: d.line,
                expected: d.expected.unwrap_or_default(),
                actual: d.actual.unwrap_or_default(),
                matches_o1: text == o1,
            },
        },
        Ok(Err(e)) if e.starts_with("not-loadable: ") => {
            Outcome::NotLoadable(e["not-loadable: ".len()..].to_string())
        }
        Ok(Err(e)) => Outcome::Failed(e),
        Err(payload) => {
            let text = common::panic_text(payload.as_ref());

            if text.contains("port pending") {
                Outcome::NotLoadable(text)
            } else {
                Outcome::Failed(format!("panic: {text}"))
            }
        }
    }
}

/// The scoreboard of `kicad-rs pcb upgrade --force` vs the reference over the corpus.
#[test]
#[ignore = "needs the reference kicad-cli and the io-corpus oracle cache"]
fn upgrade_corpus_scoreboard() {
    common::quiet_pending_panics();
    let boards = io_corpus_boards();
    let refs: Vec<_> = boards.iter().collect();
    let Some(cache) = ensure_oracle_cache(&refs, 1, &|_, _, _| {}) else {
        eprintln!("no oracle: skipped");
        return;
    };
    let cache = cache.expect("oracle cache");

    let mut identical = Vec::new();
    let mut differs = Vec::new();
    let mut not_loadable: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut failed = Vec::new();

    for entry in &cache {
        let OracleResult::Saved { o1, o2, .. } = &entry.result else {
            continue;
        };
        let o1 = std::fs::read_to_string(o1).unwrap();
        let o2 = std::fs::read_to_string(o2).unwrap();
        let file_name = entry.name.rsplit('/').next().unwrap().to_string();

        match evaluate(&o1, &o2, &file_name) {
            Outcome::Identical => identical.push(entry.name.clone()),
            Outcome::Differs {
                path,
                line,
                expected,
                actual,
                matches_o1,
            } => differs.push((entry.name.clone(), path, line, expected, actual, matches_o1)),
            Outcome::NotLoadable(why) => {
                // Group by the stub / package, not by the position in the file.
                let key = why.split(" at line").next().unwrap_or(&why).to_string();
                not_loadable
                    .entry(key)
                    .or_default()
                    .push(entry.name.clone());
            }
            Outcome::Failed(e) => failed.push((entry.name.clone(), e)),
        }
    }

    let mut md = String::new();
    let _ = writeln!(
        md,
        "# kicad-rs pcb upgrade --force vs kicad-cli (PCBIO-15)\n\nInput O1, expected O2 \
         (io-corpus oracle cache); strict parser.\n"
    );
    let _ = writeln!(md, "| status | boards |\n|---|---:|");
    let _ = writeln!(md, "| identical | {} |", identical.len());
    let _ = writeln!(md, "| differs | {} |", differs.len());
    let _ = writeln!(
        md,
        "| not loadable | {} |",
        not_loadable.values().map(Vec::len).sum::<usize>()
    );
    let _ = writeln!(md, "| failed | {} |\n", failed.len());

    let _ = writeln!(md, "## Differences\n");

    for (name, path, line, expected, actual, matches_o1) in &differs {
        let _ = writeln!(
            md,
            "- `{name}` line {line} `{path}`{}\n  - expected `{}`\n  - actual   `{}`",
            if *matches_o1 { " (Rust == O1)" } else { "" },
            expected.trim(),
            actual.trim()
        );
    }

    let _ = writeln!(md, "\n## Not loadable\n");

    for (why, names) in &not_loadable {
        let _ = writeln!(md, "- {} x `{why}`", names.len());
    }

    let _ = writeln!(md, "\n## Failed\n");

    for (name, e) in &failed {
        let _ = writeln!(md, "- `{name}`: {e}");
    }

    let _ = writeln!(md, "\n## Identical\n");

    for name in &identical {
        let _ = writeln!(md, "- `{name}`");
    }

    let path = io_corpus_dir().join("session_scoreboard.md");
    let _ = std::fs::write(&path, &md);
    println!("{md}");
    println!("written to {}", path.display());
}
