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

//! Replay of the end-to-end differential test (WIRE-1) without the oracle.
//!
//! `tests/oracle/e2e_cli.py` runs `kicad-rs` and the reference `kicad-cli` on every corpus board
//! under 300 KB for `pcb upgrade --force`, `pcb drc` (report / JSON), `pcb export gerbers`,
//! `pcb export drill` (Excellon / Gerber) and `pcb export svg`, and records per board and
//! command the status and a hash of the normalised reference outputs (exit code, stdout,
//! stderr, every produced file) in `tests/golden/e2e_cli.txt`.
//!
//! This test runs `kicad-rs` again on every pair recorded as `identical` and checks that its
//! normalised outputs still hash to the reference's. The default run covers the boards under
//! 60 KB; `cargo test -p kicad-cli --test e2e_cli -- --ignored` covers all of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXED_DATE: &str = "2026-01-02T03:04:05";

/// The arguments of each command before the input file (as in `e2e_cli.py`) and whether it
/// writes into `out/`.
fn command_args(cmd: &str) -> Option<(&'static [&'static str], bool)> {
    Some(match cmd {
        "upgrade" => (&["pcb", "upgrade", "--force"], false),
        "drc-report" => (
            &["pcb", "drc", "--format", "report", "-o", "out/drc.rpt"],
            true,
        ),
        "drc-json" => (
            &[
                "pcb",
                "drc",
                "--format",
                "json",
                "--severity-all",
                "-o",
                "out/drc.json",
            ],
            true,
        ),
        "gerbers" => (&["pcb", "export", "gerbers", "-o", "out/"], true),
        "drill-excellon" => (&["pcb", "export", "drill", "-o", "out/"], true),
        "drill-gerber" => (
            &["pcb", "export", "drill", "--format", "gerber", "-o", "out/"],
            true,
        ),
        "svg" => (
            &[
                "pcb",
                "export",
                "svg",
                "--mode-multi",
                "-l",
                "F.Cu,B.Cu,F.Silkscreen,F.Mask,Edge.Cuts",
                "-o",
                "out/",
            ],
            true,
        ),
        _ => return None,
    })
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    h
}

fn digits(b: &[u8], at: usize, n: usize) -> bool {
    at + n <= b.len() && b[at..at + n].iter().all(u8::is_ascii_digit)
}

/// Length of `\d{4}-\d{2}-\d{2}<sep>\d{2}:\d{2}:\d{2}` at `i`, 0 if none.
fn date_time_at(b: &[u8], i: usize, sep: u8) -> usize {
    let ok = digits(b, i, 4)
        && b.get(i + 4) == Some(&b'-')
        && digits(b, i + 5, 2)
        && b.get(i + 7) == Some(&b'-')
        && digits(b, i + 8, 2)
        && b.get(i + 10) == Some(&sep)
        && digits(b, i + 11, 2)
        && b.get(i + 13) == Some(&b':')
        && digits(b, i + 14, 2)
        && b.get(i + 16) == Some(&b':')
        && digits(b, i + 17, 2);

    if ok { 19 } else { 0 }
}

/// One `re.sub` pass of `e2e_cli.py`'s date patterns.
fn sub_dates(b: &[u8], sep: u8, with_offset: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;

    while i < b.len() {
        let n = date_time_at(b, i, sep);

        if n == 0 {
            out.push(b[i]);
            i += 1;
            continue;
        }

        let mut end = i + n;

        if with_offset {
            // ([+-]\d{2}:?\d{2}|Z)?
            if matches!(b.get(end), Some(b'+') | Some(b'-')) && digits(b, end + 1, 2) {
                if b.get(end + 3) == Some(&b':') && digits(b, end + 4, 2) {
                    end += 6;
                } else if digits(b, end + 3, 2) {
                    end += 5;
                }
            } else if b.get(end) == Some(&b'Z') {
                end += 1;
            }
        }

        out.extend_from_slice(b"<DATE>");
        i = end;
    }

    out
}

/// `(?m)^\d{2}:\d{2}:\d{2}: ` -> `<TIME>: `.
fn sub_times(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;

    while i < b.len() {
        let line_start = i == 0 || b[i - 1] == b'\n';

        if line_start
            && digits(b, i, 2)
            && b.get(i + 2) == Some(&b':')
            && digits(b, i + 3, 2)
            && b.get(i + 5) == Some(&b':')
            && digits(b, i + 6, 2)
            && b.get(i + 8) == Some(&b':')
            && b.get(i + 9) == Some(&b' ')
        {
            out.extend_from_slice(b"<TIME>: ");
            i += 10;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }

    out
}

/// `normalise()` of `e2e_cli.py`.
fn normalise(data: &[u8], work: &str) -> Vec<u8> {
    let d = sub_dates(data, b'T', true);
    let d = sub_dates(&d, b' ', false);
    let d = sub_times(&d);
    replace(&d, work.as_bytes(), b"<WORK>")
}

fn replace(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return data.to_vec();
    }

    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if data[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }

    out
}

fn collect(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for e in entries.flatten() {
        let p = e.path();

        if p.is_dir() {
            collect(&p, root, out);
        } else if let Ok(bytes) = std::fs::read(&p) {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            out.insert(format!("file:{rel}"), bytes);
        }
    }
}

/// Runs `kicad-rs` as `e2e_cli.py` does and returns the hash of its normalised outputs.
fn rs_hash(board: &str, cmd: &str, work: &Path) -> u64 {
    let (args, out_dir) = command_args(cmd).expect("known command");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    let src = kicad_qa::kicad_source_root().join(board);

    for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
        let f = src.with_extension(ext);

        if f.exists() {
            std::fs::copy(&f, work.join(f.file_name().unwrap())).unwrap();
        }
    }

    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_kicad-rs"))
        .args(args)
        .arg(&name)
        .current_dir(work)
        .env("KICAD_RS_DATE", FIXED_DATE)
        .env("KICAD_RS_UUID_SEED", "1")
        .output()
        .expect("kicad-rs runs");

    let mut res: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    res.insert(
        "exit".into(),
        output
            .status
            .code()
            .map_or("None".to_string(), |c| c.to_string())
            .into_bytes(),
    );
    res.insert("stdout".into(), output.stdout);

    let stderr: Vec<u8> = output
        .stderr
        .split_inclusive(|b| *b == b'\n')
        .filter(|l| !l.starts_with(b"Warning: text uses font"))
        .flatten()
        .copied()
        .collect();
    res.insert("stderr".into(), stderr);
    res.insert(
        "file:<board>".into(),
        std::fs::read(work.join(&name)).unwrap_or_default(),
    );

    if out_dir {
        collect(&work.join("out"), work, &mut res);
    }

    let w = work.to_string_lossy().into_owned();
    let mut data = Vec::new();

    for (k, v) in &res {
        data.extend_from_slice(k.as_bytes());
        data.push(0);
        data.extend_from_slice(&normalise(v, &w));
        data.push(0);
    }

    fnv1a64(&data)
}

fn replay(tag: &str, max_bytes: u64, commands: &[&str]) {
    let golden = include_str!("golden/e2e_cli.txt");
    let root = kicad_qa::kicad_source_root();
    let work: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("e2e_cli_replay")
        .join(tag);
    let mut checked = 0;
    let mut failures = Vec::new();

    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [board, cmd, status, hash, ..] = cols[..] else {
            continue;
        };

        if status != "identical" || !commands.contains(&cmd) {
            continue;
        }

        let size = std::fs::metadata(root.join(board)).map_or(u64::MAX, |m| m.len());

        if size > max_bytes {
            continue;
        }

        let got = format!("{:016x}", rs_hash(board, cmd, &work.join(cmd)));
        checked += 1;

        if got != hash {
            failures.push(format!("{board} {cmd}: {got} != {hash}"));
        }
    }

    eprintln!("e2e replay: {checked} board/command pairs checked");
    assert!(
        failures.is_empty(),
        "{} pairs no longer identical to the reference (rerun tests/oracle/e2e_cli.py \
         --filter <board> --show-diff):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn e2e_identical_pairs_small_boards() {
    replay(
        "small",
        60_000,
        &["upgrade", "gerbers", "drill-excellon", "drill-gerber"],
    );
}

#[test]
#[ignore = "runs kicad-rs on every corpus board (minutes)"]
fn e2e_identical_pairs_all() {
    replay(
        "all",
        u64::MAX,
        &[
            "upgrade",
            "drc-report",
            "drc-json",
            "gerbers",
            "drill-excellon",
            "drill-gerber",
            "svg",
        ],
    );
}

#[test]
fn normalisation_matches_script() {
    let n = normalise(
        b"date 2026-11-10T05:23:13+00:00 x 2026-11-10 05:24:54*\n05:39:56: Warning: a /w/x",
        "/w",
    );
    assert_eq!(
        String::from_utf8(n).unwrap(),
        "date <DATE> x <DATE>*\n<TIME>: Warning: a <WORK>/x"
    );
    assert_eq!(
        String::from_utf8(normalise(
            b"2026-11-10T05:23:13+0000 2026-11-10T05:23:13Z",
            ""
        ))
        .unwrap(),
        "<DATE> <DATE>"
    );
}
