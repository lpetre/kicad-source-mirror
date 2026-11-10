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

//! FIX-FAB-2: replay of the fabrication option matrix of REV-FAB
//! (`rust/docs/design/review-fab.md`) without the oracle.
//!
//! `tests/golden/fab_matrix/generate.py` runs the reference `kicad-cli` and `kicad-rs` on copies
//! of small boards (optionally mutated: stored plot parameters, DNP footprints) for
//! `pcb export gerbers` / `pcb export drill` option sets and records, per case, the FNV-1a hash
//! (low 32 bits) of every normalised reference output (exit code, stdout, stderr, each file
//! under `out/`) in `tests/golden/fab_matrix/cases.txt`, plus the status kicad-rs had then.
//!
//! This test runs `kicad-rs` again on every case recorded as `identical` and checks every hash.
//! Cases that were not identical at generation time (pending subsystems: drawing sheet, drill
//! maps, out-of-port plot formats) are counted, not run. One test per case group.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXED_DATE: &str = "2026-01-02T03:04:05";

/// One case of `cases.txt`.
struct Case {
    group: String,
    tag: String,
    kind: String,
    board: String,
    mutation: Vec<String>,
    board_hash: String,
    status: String,
    options: Vec<String>,
    /// key -> hash of the reference output
    hashes: BTreeMap<String, String>,
}

fn parse_cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();

    for line in include_str!("golden/fab_matrix/cases.txt").lines() {
        let cols: Vec<&str> = line.split('\t').collect();

        match cols[0] {
            "C" => out.push(Case {
                group: cols[1].to_string(),
                tag: cols[2].to_string(),
                kind: cols[3].to_string(),
                board: cols[4].to_string(),
                mutation: if cols[5] == "-" {
                    Vec::new()
                } else {
                    cols[5].split(' ').map(str::to_string).collect()
                },
                board_hash: cols[6].to_string(),
                status: cols[7].to_string(),
                options: cols[8..].iter().map(|s| s.to_string()).collect(),
                hashes: BTreeMap::new(),
            }),
            "H" => {
                let case = out.last_mut().expect("H line after a C line");
                case.hashes.insert(cols[2].to_string(), cols[1].to_string());
            }
            _ => {}
        }
    }

    out
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    h
}

/// `h8()` of `generate.py`: the low 32 bits of FNV-1a 64 as 8 hex digits.
fn h8(data: &[u8]) -> String {
    format!("{:08x}", fnv1a64(data) & 0xffff_ffff)
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

/// One `re.sub` pass of the date patterns of `generate.py` (as `e2e_cli.rs`).
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

fn replace(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if !from.is_empty() && data[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }

    out
}

fn normalise(data: &[u8], work: &str) -> Vec<u8> {
    let d = sub_dates(data, b'T', true);
    let d = sub_dates(&d, b' ', false);
    let d = sub_times(&d);
    replace(&d, work.as_bytes(), b"<WORK>")
}

/// `block_end()` of `generate.py`: the index of the `)` closing the list that opens at `start`
/// (quoted strings with backslash escapes skipped).
fn block_end(t: &[u8], start: usize) -> usize {
    let mut depth = 0;
    let mut i = start;

    loop {
        match t[i] {
            b'"' => {
                i += 1;

                while t[i] != b'"' {
                    if t[i] == b'\\' {
                        i += 1;
                    }

                    i += 1;
                }
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;

                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }

        i += 1;
    }
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Python `\s` on ASCII.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_line_start(t: &[u8], i: usize) -> bool {
    i == 0 || t[i - 1] == b'\n'
}

/// First match of `\(<key>\s[^()]*\)` in `block`: its byte range.
fn find_entry(block: &[u8], key: &str) -> Option<(usize, usize)> {
    let head = format!("({key}");
    let head = head.as_bytes();
    let mut from = 0;

    while let Some(rel) = find(&block[from..], head) {
        let at = from + rel;
        let mut i = at + head.len();

        if i < block.len() && is_space(block[i]) {
            i += 1;

            while i < block.len() && block[i] != b'(' && block[i] != b')' {
                i += 1;
            }

            if i < block.len() && block[i] == b')' {
                return Some((at, i + 1));
            }
        }

        from = at + 1;
    }

    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// First line-start occurrence of `needle` in `t` (Python `(?m)^needle`).
fn find_at_line_start(t: &[u8], needle: &[u8]) -> Option<usize> {
    let mut from = 0;

    while let Some(rel) = find(&t[from..], needle) {
        let at = from + rel;

        if is_line_start(t, at) {
            return Some(at);
        }

        from = at + 1;
    }

    None
}

/// Port of `mutate()` of `generate.py` (`review-fab/tools/mutate.py`): `key=value` replaces
/// the first `(key ...)` entry of the `pcbplotparams` block or appends one; `@dnp=N` marks
/// every third footprint DNP, up to N.
fn mutate(text: &[u8], tokens: &[String]) -> Vec<u8> {
    let mut start = 0;

    loop {
        let rel = find(&text[start..], b"(pcbplotparams").expect("pcbplotparams block");
        start += rel;
        let after = start + b"(pcbplotparams".len();

        if after >= text.len() || !is_word(text[after]) {
            break;
        }

        start += 1;
    }

    let end = block_end(text, start);
    let mut block = text[start..=end].to_vec();
    let mut dnp = 0;

    for kv in tokens {
        if let Some(n) = kv.strip_prefix("@dnp=") {
            dnp = n.parse::<usize>().unwrap();
            continue;
        }

        let (k, v) = kv.split_once('=').unwrap();
        let entry = format!("({k} {v})");

        match find_entry(&block, k) {
            Some((a, b)) => {
                block.splice(a..b, entry.bytes());
            }
            None => {
                block.pop();
                block.extend_from_slice(format!("\t{entry}\n\t\t)").as_bytes());
            }
        }
    }

    let mut t = text[..start].to_vec();
    t.extend_from_slice(&block);
    t.extend_from_slice(&text[end + 1..]);

    if dnp == 0 {
        return t;
    }

    // re.split( r"(?m)^(\t\(footprint )", t )
    let sep: &[u8] = b"\t(footprint ";
    let mut cuts = Vec::new();
    let mut from = 0;

    while let Some(rel) = find(&t[from..], sep) {
        let at = from + rel;

        if is_line_start(&t, at) {
            cuts.push(at);
        }

        from = at + 1;
    }

    let mut out = t[..cuts.first().copied().unwrap_or(t.len())].to_vec();
    let mut n = 0;

    for (idx, &cut) in cuts.iter().enumerate() {
        let body_start = cut + sep.len();
        let body_end = cuts.get(idx + 1).copied().unwrap_or(t.len());
        let mut body = t[body_start..body_end].to_vec();

        if idx % 3 == 0 && n < dnp {
            // (?m)^\t\t\(attr [^)]*\)
            let attr = find_at_line_start(&body, b"\t\t(attr ").and_then(|at| {
                body[at..]
                    .iter()
                    .position(|&c| c == b')')
                    .map(|rel| (at, at + rel))
            });

            match attr {
                Some((at, close)) => {
                    if find(&body[at..close], b" dnp").is_none() {
                        body.splice(close..close, b" dnp".iter().copied());
                    }
                }
                None => {
                    if let Some(at) = find_at_line_start(&body, b"\t\t(property ") {
                        body.splice(at..at, b"\t\t(attr dnp)\n".iter().copied());
                    }
                }
            }

            n += 1;
        }

        out.extend_from_slice(sep);
        out.extend_from_slice(&body);
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

/// Runs `kicad-rs` on the case as `generate.py` does: the board hash and the output hashes.
fn rs_hashes(case: &Case, work: &Path) -> (String, BTreeMap<String, String>) {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    let src = kicad_qa::kicad_source_root().join(&case.board);
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let mut data = std::fs::read(&src).unwrap();

    if !case.mutation.is_empty() {
        data = mutate(&data, &case.mutation);
    }

    std::fs::write(work.join(&name), &data).unwrap();

    for ext in ["kicad_pro", "kicad_dru"] {
        let f = src.with_extension(ext);

        if f.exists() {
            std::fs::copy(&f, work.join(f.file_name().unwrap())).unwrap();
        }
    }

    let output = Command::new(env!("CARGO_BIN_EXE_kicad-rs"))
        .args(["pcb", "export", case.kind.as_str()])
        .args(&case.options)
        .args(["-o", "out/", name.as_str()])
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
    collect(&work.join("out"), work, &mut res);

    let w = work.to_string_lossy().into_owned();
    let res: BTreeMap<String, Vec<u8>> = res
        .into_iter()
        .map(|(k, v)| {
            let v = normalise(&v, &w);
            (k, v)
        })
        .collect();

    let hashes = if case.group == "drillx" {
        let mut all = Vec::new();

        for (k, v) in &res {
            all.extend_from_slice(k.as_bytes());
            all.push(0);
            all.extend_from_slice(v);
            all.push(0);
        }

        BTreeMap::from([("*".to_string(), h8(&all))])
    } else {
        // `~` stands for `file:out/<board name>` (generate.py)
        let stem = format!("file:out/{}", name.strip_suffix(".kicad_pcb").unwrap_or(&name));

        res.iter()
            .map(|(k, v)| {
                let key = match k.strip_prefix(&stem) {
                    Some(rest) => format!("~{rest}"),
                    None => k.clone(),
                };
                (key, h8(v))
            })
            .collect()
    };

    (h8(&data), hashes)
}

fn replay(group: &str) {
    let cases = parse_cases();
    let work_root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fab_matrix")
        .join(group);
    let mut checked = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for case in cases.iter().filter(|c| c.group == group) {
        if case.status != "identical" {
            skipped += 1;
            continue;
        }

        let (board_hash, got) = rs_hashes(case, &work_root);
        checked += 1;

        assert_eq!(
            board_hash, case.board_hash,
            "{} {}: the mutated board differs from generate.py's",
            case.tag, case.board
        );

        if got != case.hashes {
            let keys: Vec<&String> = got
                .keys()
                .chain(case.hashes.keys())
                .filter(|k| got.get(*k) != case.hashes.get(*k))
                .collect();
            failures.push(format!(
                "{} {} {}: {:?}",
                case.tag,
                case.board,
                case.options.join(" "),
                keys
            ));
        }
    }

    eprintln!("fab matrix {group}: {checked} cases checked, {skipped} not identical at generation");
    assert!(checked > 0, "no identical case in group {group}");
    assert!(
        failures.is_empty(),
        "{} cases no longer identical to the reference (rerun generate.py --filter <tag>):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fab_matrix_gerbers_options() {
    replay("gerbers");
}

#[test]
fn fab_matrix_gerbers_dnp() {
    replay("dnp");
}

#[test]
fn fab_matrix_gerbers_stored_plot_params() {
    replay("stored");
}

#[test]
fn fab_matrix_gerbers_define_vars() {
    replay("define");
}

#[test]
fn fab_matrix_gerbers_variants() {
    replay("variant");
}

#[test]
fn fab_matrix_drill_excellon_factorial() {
    replay("drillx");
}

#[test]
fn fab_matrix_drill_other() {
    replay("drill");
}

#[test]
fn mutation_matches_script() {
    let board = b"(kicad_pcb\n\t(setup\n\t\t(pcbplotparams\n\t\t\t(mirror no)\n\t\t\t(mirrorx \"(\")\n\t\t)\n\t)\n\t(footprint \"a\"\n\t\t(attr smd)\n\t\t(property \"R\")\n\t)\n\t(footprint \"b\"\n\t\t(property \"R\")\n\t)\n\t(footprint \"c\"\n\t\t(property \"R\")\n\t)\n\t(footprint \"d\"\n\t\t(property \"R\")\n\t)\n)\n";
    let tokens: Vec<String> = ["mirror=yes", "psnegative=yes", "@dnp=8"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let got = String::from_utf8(mutate(board, &tokens)).unwrap();
    assert_eq!(
        got,
        "(kicad_pcb\n\t(setup\n\t\t(pcbplotparams\n\t\t\t(mirror yes)\n\t\t\t(mirrorx \"(\")\n\t\t\t(psnegative yes)\n\t\t)\n\t)\n\t(footprint \"a\"\n\t\t(attr smd dnp)\n\t\t(property \"R\")\n\t)\n\t(footprint \"b\"\n\t\t(property \"R\")\n\t)\n\t(footprint \"c\"\n\t\t(property \"R\")\n\t)\n\t(footprint \"d\"\n\t\t(attr dnp)\n\t\t(property \"R\")\n\t)\n)\n"
    );
}
