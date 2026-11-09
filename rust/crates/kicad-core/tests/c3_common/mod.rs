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

//! Helpers shared by the C3 oracle replays (mirrored in `tests/oracle/json_oracle.cpp` and
//! `tests/oracle/pattern_oracle.cpp`).

#![allow(dead_code)]

use std::collections::BTreeMap;

/// The pseudo-random generator of the C++ drivers (`struct LCG`).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    pub fn u64(&mut self) -> u64 {
        let a = self.next();
        let b = self.next();
        let c = self.next();
        (a << 42) ^ (b << 21) ^ c
    }

    pub fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
}

pub const FNV_OFFSET: u64 = 1469598103934665603;

pub fn fnv_bytes(s: &[u8], mut h: u64) -> u64 {
    for &c in s {
        h ^= c as u64;
        h = h.wrapping_mul(1099511628211);
    }

    h
}

pub fn hex64(v: u64) -> String {
    format!("{v:016x}")
}

/// `esc()` of the C++ drivers: bytes outside printable ASCII and `\` as `\xHH`.
pub fn esc(s: &[u8]) -> String {
    let mut o = String::new();

    for &c in s {
        if !(0x20..0x7f).contains(&c) || c == b'\\' {
            o += &format!("\\x{c:02X}");
        } else {
            o.push(c as char);
        }
    }

    o
}

/// Output lines grouped in sections, like `begin()` / `out()` of the C++ drivers.
#[derive(Default)]
pub struct Out {
    pub sections: Vec<(String, Vec<String>)>,
}

impl Out {
    pub fn begin(&mut self, name: &str) {
        self.sections.push((name.to_string(), Vec::new()));
    }

    pub fn out(&mut self, line: String) {
        self.sections.last_mut().unwrap().1.push(line);
    }
}

fn fnv_lines(lines: &[String]) -> u64 {
    let mut h = FNV_OFFSET;

    for line in lines {
        h = fnv_bytes(line.as_bytes(), h);
        h = fnv_bytes(b"\n", h);
    }

    h
}

/// Compares `o` with a golden file of `E <section> <line>` / `S <section> <count> <hash>`
/// lines. On a mismatch all Rust lines are written to `target/core-oracle/<full_name>` for a
/// diff against the C++ driver's `full` output.
pub fn check_golden(golden_path: &str, o: &Out, full_name: &str) {
    let golden = std::fs::read_to_string(golden_path).expect("golden file");
    let mut expected_lines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut expected_summary: BTreeMap<String, (usize, u64)> = BTreeMap::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("E ") {
            let (section, text) = rest.split_once(' ').unwrap_or((rest, ""));
            expected_lines.entry(section.to_string()).or_default().push(text.to_string());
        } else if let Some(rest) = line.strip_prefix("S ") {
            let parts: Vec<&str> = rest.split(' ').collect();
            expected_summary.insert(
                parts[0].to_string(),
                (parts[1].parse().unwrap(), u64::from_str_radix(parts[2], 16).unwrap()),
            );
        }
    }

    let mut failures = Vec::new();

    if o.sections.len() != expected_summary.len() {
        failures.push(format!("section count {} (C++ {})", o.sections.len(), expected_summary.len()));
    }

    for (name, lines) in &o.sections {
        let Some(&(count, hash)) = expected_summary.get(name) else {
            failures.push(format!("unexpected section {name}"));
            continue;
        };

        for (i, exp) in expected_lines.get(name).into_iter().flatten().enumerate() {
            if lines.get(i) != Some(exp) {
                failures.push(format!("{name} line {i}:\n  C++ : {exp}\n  Rust: {:?}", lines.get(i)));
            }
        }

        if lines.len() != count || fnv_lines(lines) != hash {
            failures.push(format!("{name}: {} lines (C++ {count}), hash mismatch", lines.len()));
        }
    }

    if !failures.is_empty() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/core-oracle");
        let _ = std::fs::create_dir_all(dir);
        let full: String = o
            .sections
            .iter()
            .flat_map(|(name, lines)| lines.iter().map(move |l| format!("E {name} {l}\n")))
            .collect();
        let _ = std::fs::write(format!("{dir}/{full_name}"), full);

        panic!(
            "{} mismatches (diff target/core-oracle/{full_name} against the C++ driver's `full` output):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

/// The file list of `build_c3_oracle.sh`: every `.kicad_pro`, `.kicad_prl`, `.json` and
/// `.gbrjob` file under `qa/data` and `demos`, relative to the source root, in byte order.
pub fn json_files() -> Vec<String> {
    let root = kicad_qa::kicad_source_root();
    let mut out = Vec::new();

    for dir in [kicad_qa::qa_data_dir(), kicad_qa::demos_dir()] {
        for ext in ["kicad_pro", "kicad_prl", "json", "gbrjob"] {
            for p in kicad_qa::files_with_extension(&dir, ext) {
                let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
                out.push(rel);
            }
        }
    }

    out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    out
}
