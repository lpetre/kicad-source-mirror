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

//! Package M26 differential oracle: generated tables (drill charts, stackup tables) and the
//! drill symbol profile after `RefreshGeneratedTables`, against the reference `kicad-cli pcb
//! upgrade --force` (golden `tests/golden/m26_generated_tables.txt`, written by
//! `tests/oracle/m26_generated_tables.sh`; cases `tests/data/m26/cases.txt`).
//!
//! Each case is parsed, refreshed (`kicad_pcb::board_tables::refresh_generated_tables`) and
//! written; the generated blocks of the output, with the UUIDs of new cells normalised, must
//! hash like the reference's (every cell text, cell geometry, font, margins, column width and
//! row height, row keys, row shapes and the committed profile). Each output is refreshed once
//! more and must reproduce the reference's second upgrade (a fixed point where the reference
//! has one).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kicad_pcb::board_tables::refresh_generated_tables;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn data(name: &str) -> String {
    std::fs::read_to_string(crate_dir().join("tests/data/m26").join(name)).unwrap()
}

fn qa(rel: &str) -> String {
    std::fs::read_to_string(kicad_qa::pcbnew_data_dir().join(rel)).unwrap()
}

fn fnv1a64(text: &str) -> String {
    let mut h: u64 = 0xCBF29CE484222325;

    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001B3);
    }

    format!("{h:016x}")
}

/// `normalise` of the oracle script: new UUIDs (not in `original`) become
/// `00000000-0000-4000-8000-<ordinal>`, in order of appearance.
fn normalise(text: &str, original: &str) -> String {
    let mut n = 0;
    let mut lines = Vec::new();

    for line in text.split('\n') {
        let t = line.trim_start_matches('\t');

        if t.starts_with("(uuid \"") && !original.contains(t) {
            n += 1;
            lines.push(format!(
                "{}(uuid \"00000000-0000-4000-8000-{n:012}\")",
                &line[..line.len() - t.len()]
            ));
        } else {
            lines.push(line.to_string());
        }
    }

    lines.join("\n")
}

/// `blocks` of the oracle script: the top-level drill_chart / stackup_table blocks and the
/// setup's drill_symbol_profile, each with its trailing newline.
fn blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<(Vec<&str>, String)> = None;

    for line in text.split('\n') {
        match &mut cur {
            None => {
                if line.starts_with("\t(drill_chart")
                    || line.starts_with("\t(stackup_table")
                    || line.starts_with("\t\t(drill_symbol_profile")
                {
                    let ind = &line[..line.len() - line.trim_start_matches('\t').len()];
                    cur = Some((vec![line], format!("{ind})")));
                }
            }
            Some((lines, end)) => {
                lines.push(line);

                if line == end {
                    out.push(lines.join("\n") + "\n");
                    cur = None;
                }
            }
        }
    }

    out
}

/// `summary` of the oracle script.
fn summary(name: &str, out: &str, original: &str) -> (Vec<String>, Vec<String>, String) {
    let norm = normalise(out, original);
    let bl = blocks(&norm);
    let mut lines = Vec::new();

    for (i, b) in bl.iter().enumerate() {
        let kind = b
            .trim()
            .lines()
            .next()
            .unwrap()
            .trim()
            .trim_start_matches('(');
        lines.push(format!("block|{name}|{i}|{kind}|fnv={}", fnv1a64(b)));

        for key in ["column_widths", "row_heights"] {
            let tag = format!("\n\t\t({key} ");

            if let Some(start) = b.find(&tag) {
                let rest = &b[start + tag.len()..];
                let value: Vec<&str> = rest[..rest.find(')').unwrap()].split_whitespace().collect();
                lines.push(format!("{key}|{name}|{i}|{}", value.join(" ")));
            }
        }

        let mut texts = Vec::new();
        let mut rest = b.as_str();

        while let Some(start) = rest.find("(table_cell \"") {
            let s = &rest[start + 13..];
            let mut end = 0;
            let bytes = s.as_bytes();

            while bytes[end] != b'"' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }

            texts.push(&s[..end]);
            rest = &s[end..];
        }

        if !texts.is_empty() {
            lines.push(format!("texts|{name}|{i}|{}", texts.join("\t")));
        }
    }

    (lines, bl, norm)
}

/// `stage1_input` of the oracle script.
fn stage1_input(index: usize, base: &str, profile: &str) -> String {
    let mut text = qa(base).trim_end().to_string();

    if profile != "-" {
        let prof = data(profile);
        let idx = text.find("(setup").unwrap() + "(setup".len();
        text.insert_str(idx, &format!("\n{}", prof.trim_end_matches('\n')));
    }

    let mut tables = data("tables.txt");

    for k in 0..10 {
        tables = tables.replace(
            &format!("@UUID{k}@"),
            &format!("{:08x}-0000-4000-8000-{k:012}", 0x26000000 + index),
        );
    }

    format!("{}{tables})\n", &text[..text.len() - 1])
}

/// Python's `re.sub(r"\t\t\(row_keys\n(\t\t\t\(key [^\n]*\n)*\t\t\)\n", "", tables)`.
fn strip_row_keys(tables: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;

    for line in tables.split_inclusive('\n') {
        if !skipping && line == "\t\t(row_keys\n" {
            skipping = true;
            continue;
        }

        if skipping {
            if line.starts_with("\t\t\t(key ") {
                continue;
            }

            assert_eq!(line, "\t\t)\n");
            skipping = false;
            continue;
        }

        out.push_str(line);
    }

    out
}

/// `stage2_input` of the oracle script.
fn stage2_input(base: &str, mode: &str, gen1_blocks: &[String]) -> String {
    let mut tables: String = gen1_blocks
        .iter()
        .filter(|b| !b.starts_with("\t\t("))
        .cloned()
        .collect();

    match mode {
        "legacy" => tables = strip_row_keys(&tables),
        "styled" => {
            tables = tables.replace("(size 1.27 1.27)", "(size 2 2)\n\t\t\t\t\t\t(bold yes)");
            tables = tables.replace(
                "(margins 1.0025 1.0025 1.0025 1.0025)",
                "(margins 0.7 0.2 0.3 0.4)",
            );
        }
        _ => {}
    }

    let text = qa(base);
    let text = text.trim_end();
    format!("{}{tables})\n", &text[..text.len() - 1])
}

/// The Rust upgrade of the generated tables: parse, `RefreshGeneratedTables`, write.
fn refresh(text: &str, name: &str) -> (String, i32) {
    let (mut board, _) = parse_board_report(text, name, &ParseOptions::default())
        .unwrap_or_else(|e| panic!("{name}: {}", e.what()));
    let count = refresh_generated_tables(&mut board);
    let out = format_board(&board, &FormatOptions::default())
        .unwrap_or_else(|e| panic!("{name}: {}", e.what()));
    (out, count)
}

#[test]
fn generated_tables_match_the_reference_upgrade() {
    let golden =
        std::fs::read_to_string(crate_dir().join("tests/golden/m26_generated_tables.txt")).unwrap();

    // The golden by case: its lines and whether the reference's second upgrade was a fixed point
    let mut expected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut fixed: BTreeMap<String, bool> = BTreeMap::new();

    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let mut parts = line.split('|');
        let tag = parts.next().unwrap();
        let name = parts.next().unwrap().to_string();

        if tag == "case" {
            fixed.insert(name, line.ends_with("fixed_point=yes"));
        } else {
            expected.entry(name).or_default().push(line.to_string());
        }
    }

    let mut gen1: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut compared = 0;

    for (index, line) in data("cases.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .enumerate()
    {
        let f: Vec<&str> = line.split('|').map(str::trim).collect();
        let (name, base, profile, mode) = (f[0], f[1], f[2], f[3]);

        let input = match mode {
            "tables" => stage1_input(index, base, profile),
            "-" => qa(base),
            _ => stage2_input(base, mode, &gen1["m26_backdrill"]),
        };

        let (o1, count) = refresh(&input, name);
        let (lines, bl, norm) = summary(name, &o1, &input);
        assert!(count > 0, "{name}: nothing refreshed");
        assert_eq!(lines, expected[name], "{name}: first refresh");
        gen1.insert(name.to_string(), bl);

        // The second refresh: the reference's O2 is O1 for a fixed point, else its summary is
        // in the golden under "<name>@o2"
        let (o2, _) = refresh(&o1, name);

        // M26_DUMP=<dir>: keep the normalised outputs for a diff against the oracle script's
        if let Ok(dir) = std::env::var("M26_DUMP") {
            let dir = Path::new(&dir);
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{name}.o1.kicad_pcb")), &norm).unwrap();
            std::fs::write(
                dir.join(format!("{name}.o2.kicad_pcb")),
                normalise(&o2, &input),
            )
            .unwrap();
        }

        if fixed[name] {
            assert_eq!(
                normalise(&o2, &o1),
                o1,
                "{name}: second refresh is no fixed point"
            );
        } else {
            let o2_name = format!("{name}@o2");
            assert_eq!(
                summary(&o2_name, &o2, &input).0,
                expected[&o2_name],
                "{name}: second refresh"
            );
        }

        compared += 1;
    }

    assert_eq!(compared, fixed.len());
}
