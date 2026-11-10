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

//! Drill outputs of the reference `kicad-cli` (`tests/golden/drill/`, made by
//! `tests/golden/drill/generate.sh`: ten corpus boards x five option sets, the four of
//! `qa/tests/cli/test_pcb.py::test_pcb_export_drill` plus `--excellon-separate-th`).
//!
//! * `golden_set_is_consistent`: every `files.txt` names exactly the files of its directory.
//! * `basic_test_goldens_match_repository_goldens`: the oracle output of `basic_test` equals
//!   the goldens committed in `qa/data/cli/basic_test/` after normalisation (checks the
//!   normaliser and the golden generation).
//! * `drill_export_matches_cli_goldens` (ignored): the port's `pcb export drill` against the
//!   goldens; needs `kicad_pcb::io::parse_board` and the drill writers (FAB-02, FAB-08).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The five option sets of `generate.sh` (directory name, kicad-cli arguments).
pub const OPTION_SETS: [(&str, &[&str]); 5] = [
    ("excellon_default", &["--format", "excellon"]),
    ("excellon_inches", &["--format", "excellon", "-u", "in"]),
    (
        "excellon_mirror",
        &["--format", "excellon", "--excellon-mirror-y"],
    ),
    (
        "excellon_separate_th",
        &["--format", "excellon", "--excellon-separate-th"],
    ),
    ("gerber_default", &["--format", "gerber"]),
];

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/drill")
}

/// The corpus boards of `generate.sh` (paths relative to the KiCad source root).
fn corpus_boards() -> Vec<String> {
    let script = include_str!("golden/drill/generate.sh");
    let start = script.find("BOARDS=(").expect("BOARDS list");
    let end = start + script[start..].find("\n)").expect("end of BOARDS list");
    script[start..end]
        .lines()
        .skip(1)
        .map(|l| l.trim().to_string())
        .collect()
}

/// Replaces the header lines that carry dates / versions by fixed tokens
/// (`docs/analysis/plot-fab.md` 9.3), so that outputs of different builds and runs compare.
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let token = if body.starts_with("; DRILL file KiCad ") && body.contains(" date ") {
            Some("; DRILL file KiCad <version> date <date>")
        } else if body.starts_with("; #@! TF.CreationDate,") {
            Some("; #@! TF.CreationDate,<date>")
        } else if body.starts_with("; #@! TF.GenerationSoftware,Kicad,Pcbnew,") {
            Some("; #@! TF.GenerationSoftware,Kicad,Pcbnew,<version>")
        } else if body.starts_with("%TF.GenerationSoftware,KiCad,Pcbnew,") && body.ends_with("*%") {
            Some("%TF.GenerationSoftware,KiCad,Pcbnew,<version>*%")
        } else if body.starts_with("%TF.CreationDate,") && body.ends_with("*%") {
            Some("%TF.CreationDate,<date>*%")
        } else if body.starts_with("G04 Created by KiCad (PCBNEW ")
            && body.contains(") date ")
            && body.ends_with('*')
        {
            Some("G04 Created by KiCad (PCBNEW <version>) date <date>*")
        } else if body.starts_with("G04 #@! TF.GenerationSoftware,") && body.ends_with('*') {
            Some("G04 #@! TF.GenerationSoftware,<version>*")
        } else if body.starts_with("G04 #@! TF.CreationDate,") && body.ends_with('*') {
            Some("G04 #@! TF.CreationDate,<date>*")
        } else {
            None
        };

        match token {
            Some(t) => {
                out.push_str(t);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(line),
        }
    }

    out
}

/// The created file names of one golden directory, in creation order.
fn listed_files(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("files.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            l.strip_prefix("Created file '")
                .and_then(|r| r.strip_suffix('\''))
                .unwrap_or_else(|| panic!("bad files.txt line {l:?}"))
                .to_string()
        })
        .collect()
}

#[test]
fn golden_set_is_consistent() {
    let boards = corpus_boards();
    assert_eq!(boards.len(), 10);

    for board in &boards {
        let stem = Path::new(board).file_stem().unwrap().to_str().unwrap();
        for (set, _) in OPTION_SETS {
            let dir = golden_dir().join(stem).join(set);
            let listed: BTreeSet<String> = listed_files(&dir).into_iter().collect();
            let present: BTreeSet<String> = fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .filter(|n| n != "files.txt")
                .collect();
            assert_eq!(listed, present, "{stem}/{set}");
            // Excellon always writes the main file; Gerber writes no file without holes.
            if set.starts_with("excellon") {
                assert!(!present.is_empty(), "{stem}/{set}: no drill file");
            }
        }
    }
}

#[test]
fn basic_test_goldens_match_repository_goldens() {
    let repo = kicad_qa::qa_data_dir().join("cli/basic_test");
    let ours = golden_dir().join("basic_test");

    for (set, repo_file, our_file) in [
        (
            "excellon_default",
            "basic_test_excellon_default.drl",
            "basic_test.drl",
        ),
        (
            "excellon_inches",
            "basic_test_excellon_inches.drl",
            "basic_test.drl",
        ),
        (
            "excellon_mirror",
            "basic_test_excellon_mirror.drl",
            "basic_test.drl",
        ),
        (
            "gerber_default",
            "basic_test-PTH-drl.gbr",
            "basic_test-PTH-drl.gbr",
        ),
    ] {
        let expected = normalise(&fs::read_to_string(repo.join(repo_file)).unwrap());
        let got = normalise(&fs::read_to_string(ours.join(set).join(our_file)).unwrap());
        assert_eq!(got, expected, "{set}");
    }
}

#[test]
fn normaliser_replaces_only_header_lines() {
    let text = "M48\n; DRILL file KiCad 10.99.0-unknown date 2026-11-09T23:28:38\n\
                ; #@! TF.CreationDate,2026-11-09T23:28:38+00:00\n\
                ; #@! TF.GenerationSoftware,Kicad,Pcbnew,10.99.0-unknown\n\
                ; #@! TF.FileFunction,MixedPlating,1,2\nT1C0.400\n";
    let n = normalise(text);
    assert_eq!(
        n,
        "M48\n; DRILL file KiCad <version> date <date>\n; #@! TF.CreationDate,<date>\n\
         ; #@! TF.GenerationSoftware,Kicad,Pcbnew,<version>\n\
         ; #@! TF.FileFunction,MixedPlating,1,2\nT1C0.400\n"
    );
    let g = "%TF.GenerationSoftware,KiCad,Pcbnew,10.99.0-unknown*%\n%TF.CreationDate,x*%\n%FSLAX46Y46*%";
    assert_eq!(
        normalise(g),
        "%TF.GenerationSoftware,KiCad,Pcbnew,<version>*%\n%TF.CreationDate,<date>*%\n%FSLAX46Y46*%"
    );
}

/// `pcb export drill` through the port, compared with the oracle goldens: same file names in
/// the same order and identical contents after normalisation.
#[test]
#[ignore = "needs kicad_pcb::io::parse_board and the drill writers (FAB-02, FAB-08)"]
fn drill_export_matches_cli_goldens() {
    for board in corpus_boards() {
        let path = kicad_qa::kicad_source_root().join(&board);
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let text = fs::read_to_string(&path).unwrap();

        for (set, args) in OPTION_SETS {
            let dir = golden_dir().join(&stem).join(set);
            let expected_names = listed_files(&dir);
            let outputs = run_port(&path, &text, args);
            let names: Vec<String> = outputs.iter().map(|(n, _)| n.clone()).collect();
            assert_eq!(names, expected_names, "{stem}/{set}: created files");

            for (name, contents) in &outputs {
                let expected = fs::read_to_string(dir.join(name)).unwrap();
                assert_eq!(
                    normalise(contents),
                    normalise(&expected),
                    "{stem}/{set}/{name}"
                );
            }
        }
    }
}

/// Runs the port's drill export for one board and option set; returns `(file name, text)` in
/// creation order. To be wired by FAB-02 / FAB-08 (parse with `kicad_pcb::io::parse_board`,
/// apply the CLI defaults of `docs/analysis/plot-fab.md` 1.3 plus `args`, call the drill job).
fn run_port(path: &Path, text: &str, args: &[&str]) -> Vec<(String, String)> {
    let _ = (path, text, args);
    panic!("drill export not ported yet (FAB-02 Excellon, FAB-08 Gerber, PCBIO parser)");
}
