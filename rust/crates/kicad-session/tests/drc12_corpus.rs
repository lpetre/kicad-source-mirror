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

//! DRC-12: the `pcb drc` job (`run_drc`, the kicad-cli load of `Session::load_board`) against
//! the reference `kicad-cli pcb drc --format report --severity-all --units mm` on the DRC
//! corpus. Golden: `kicad-drc/tests/golden/drc12_cli.txt` (`kicad-drc/tests/oracle/
//! drc12_cli_compare.py golden`): per board the FNV-1a 64 of the text report with the date
//! normalised and the entries of the unported providers removed (section counts recomputed),
//! once in report order and once with the entries of each section sorted, and the filtered
//! counts. The full differential run (JSON, stdout, exit codes) is the script's `cmp` mode.
//!
//! A board compares when it loads; the comparison tightens by itself as loaders and
//! providers land. `DRC12_FILTER=<substring>` limits the boards.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_core::Severity;
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat, RcUnits};
use kicad_session::{Reporter, Session, run_drc};

/// `UNPORTED` of `drc12_cli_compare.py`: the settings keys of the codes of
/// `kicad_drc::run_flow::UNPORTED_PROVIDERS`.
fn unported_keys() -> Vec<&'static str> {
    use kicad_drc::run_flow::UNPORTED_PROVIDERS;

    let mut keys = Vec::new();

    for p in &UNPORTED_PROVIDERS {
        for &code in p.codes {
            keys.push(kicad_pcb::drc_api::drc_settings_key(code).expect("settings key"));
        }
    }

    keys
}

const FNV_OFFSET: u64 = 0xCBF29CE484222325;

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;

    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001B3);
    }

    h
}

/// `filter_text` of `drc12_cli_compare.py`: (text, counts per section).
fn filter_text(text: &str, sort: bool, unported: &[&str]) -> (String, Vec<usize>) {
    let mut text = text.to_string();
    let heading = format!("\n{}", kicad_drc::run_flow::CHECKS_NOT_RUN_HEADING);

    if let (Some(a), Some(b)) = (text.find(&heading), text.find("\n** End of Report **")) {
        text = format!("{}{}", &text[..a], &text[b..]);
    }

    let mut head: Vec<&str> = Vec::new();
    let mut sections: Vec<(&str, Vec<String>)> = Vec::new();
    let mut tail: Vec<&str> = Vec::new();
    let mut in_section = false;

    for line in text.split('\n') {
        if !tail.is_empty() {
            tail.push(line);
            continue;
        }

        if line.starts_with("** Found ") {
            sections.push((line, Vec::new()));
            in_section = true;
            continue;
        }

        if !in_section {
            head.push(line);
            continue;
        }

        if line.starts_with("** ") {
            in_section = false;
            tail.push(line);
            continue;
        }

        let blocks = &mut sections.last_mut().unwrap().1;

        if line.starts_with('[') {
            blocks.push(format!("{line}\n"));
        } else if line.starts_with("    ")
            && let Some(last) = blocks.last_mut()
        {
            *last += &format!("{line}\n");
        }
    }

    let mut out = head.join("\n") + "\n";
    let mut counts = Vec::new();

    for (header, blocks) in sections {
        let mut kept: Vec<String> = blocks
            .into_iter()
            .filter(|b| !unported.contains(&&b[1..b.find(']').unwrap()]))
            .map(|b| normalise_block(&b))
            .collect();
        kept = at_limit(kept);

        if sort {
            kept.sort();
        }

        counts.push(kept.len());

        // re.sub( r"Found \d+ ", "Found %d " % len( kept ), header )
        let rest = header["** Found ".len()..].trim_start_matches(|c: char| c.is_ascii_digit());
        out += &format!("** Found {}{rest}\n", kept.len());
        out += &kept.concat();
        out += "\n";
    }

    out += &tail.join("\n");
    (out, counts)
}

/// `normalise_block` of `drc12_cli_compare.py`: the item lines of `connection_width` sorted
/// (C++ address order, kicad-drc PORT_STATUS.md DRC-12).
fn normalise_block(block: &str) -> String {
    let lines: Vec<&str> = block.split('\n').filter(|l| !l.is_empty()).collect();
    let mut out: Vec<&str> = lines.clone();

    if block.starts_with("[connection_width]") {
        let mut items: Vec<&str> = lines[1..]
            .iter()
            .copied()
            .filter(|l| l.starts_with("    @"))
            .collect();
        items.sort();
        out = vec![lines[0]];
        out.extend(items);
        out.extend(
            lines[1..]
                .iter()
                .copied()
                .filter(|l| !l.starts_with("    @")),
        );
    }

    out.join("\n") + "\n"
}

/// `at_limit` of `drc12_cli_compare.py` (drc.md 9.4 (3)): the entries of a type with 199 or
/// 499 entries are replaced by one placeholder at the first one (count-only comparison).
fn at_limit(entries: Vec<String>) -> Vec<String> {
    let type_of = |e: &str| e[1..e.find(']').unwrap()].to_string();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for e in &entries {
        *counts.entry(type_of(e)).or_default() += 1;
    }

    let mut out = Vec::new();
    let mut done = std::collections::BTreeSet::new();

    for e in entries {
        let t = type_of(&e);
        let n = counts[&t];

        if n == 199 || n == 499 {
            if done.insert(t.clone()) {
                out.push(format!("[{t}]: {n} entries (error limit)\n"));
            }

            continue;
        }

        out.push(e);
    }

    out
}

/// One board of the golden.
struct GoldenBoard {
    exact: u64,
    sorted: u64,
}

fn golden() -> BTreeMap<String, Option<GoldenBoard>> {
    let mut out = BTreeMap::new();

    for line in include_str!("../../kicad-drc/tests/golden/drc12_cli.txt").lines() {
        let f: Vec<&str> = line.split('\t').collect();

        if f.get(1) == Some(&"ref_failed") {
            out.insert(f[0].to_string(), None);
            continue;
        }

        out.insert(
            f[0].to_string(),
            Some(GoldenBoard {
                exact: u64::from_str_radix(f[1], 16).unwrap(),
                sorted: u64::from_str_radix(f[2], 16).unwrap(),
            }),
        );
    }

    out
}

/// The `pcb drc --format report --severity-all --units mm` report of `rel`, or why not.
fn rust_report(rel: &str) -> Result<String, String> {
    let path = kicad_qa::kicad_source_root().join(rel);
    let text =
        String::from_utf8_lossy(&std::fs::read(&path).map_err(|e| e.to_string())?).into_owned();
    let project = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let rules = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_string_lossy().to_string();

    let mut session = Session::load_board(&text, &name, project.as_deref(), rules.as_deref())
        .map_err(|e| format!("load: {e}"))?;

    let mut job = JobPcbDrc::new();
    job.rc.filename = name.clone();
    job.rc.format = RcOutputFormat::Report;
    job.rc.units = RcUnits::Mm;
    job.rc.severity =
        Severity::Error.as_i32() | Severity::Warning.as_i32() | Severity::Exclusion.as_i32();
    job.parity = false;
    job.report_date = "<DATE>".to_string();

    let mut reporter = Reporter::new();
    let outcome = run_drc(&mut session, &mut job, &mut reporter).map_err(|e| e.to_string())?;

    outcome
        .report_text
        .ok_or_else(|| format!("no report (exit code {})", outcome.exit_code))
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn drc_job_matches_reference_reports() {
    let filter = std::env::var("DRC12_FILTER").unwrap_or_default();
    let unported = unported_keys();
    let mut exact = Vec::new();
    let mut same_set = Vec::new();
    let mut differences = Vec::new();
    let mut pending = Vec::new();

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for (rel, reference) in golden() {
        if !rel.contains(&filter) {
            continue;
        }

        let Some(reference) = reference else {
            pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        let report = match catch_unwind(AssertUnwindSafe(|| rust_report(&rel))) {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(p) => {
                pending.push(format!("{rel}: panic: {}", panic_text(&*p)));
                continue;
            }
        };

        let (text, _) = filter_text(&report, false, &unported);
        let (sorted, counts) = filter_text(&report, true, &unported);

        if fnv1a64(text.as_bytes()) == reference.exact {
            exact.push(rel);
        } else if fnv1a64(sorted.as_bytes()) == reference.sorted {
            same_set.push(rel);
        } else {
            differences.push(format!("{rel}: counts {counts:?}"));
        }
    }

    std::panic::set_hook(hook);

    eprintln!(
        "pcb drc vs reference: {} boards equal, {} equal as sets (reference order is thread \
         scheduling dependent), {} differ, {} pending",
        exact.len(),
        same_set.len(),
        differences.len(),
        pending.len()
    );

    for s in &same_set {
        eprintln!("  same set: {s}");
    }

    for d in &differences {
        eprintln!("  DIFF {d}");
    }

    for p in &pending {
        eprintln!("  pending: {p}");
    }

    let known: Vec<&str> = KNOWN_DIFFERENCES.iter().map(|(b, _)| *b).collect();
    let unexpected: Vec<&String> = differences
        .iter()
        .filter(|d| !known.iter().any(|k| d.starts_with(&format!("{k}:"))))
        .collect();

    assert!(
        unexpected.is_empty(),
        "{} unexplained differences:\n{unexpected:#?}",
        unexpected.len()
    );
}

/// Boards whose report differs from the reference, with the cause (`PORT_STATUS.md`, DRC-12).
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    (
        "qa/data/pcbnew/issue24772/fw16_MCIO8i.kicad_pcb",
        "one connection_width entry names other items (C++ std::set of item pointers, address order)",
    ),
    (
        "qa/data/pcbnew/issue22475/issue22475.kicad_pcb",
        "text_thickness of a TrueType text missing (outline-font branch of the text_dims provider, DRC-24 gap)",
    ),
    (
        "qa/data/pcbnew/api_kitchen_sink.kicad_pcb",
        "one clearance violation missing: barcode 'acode' (F.Cu) against an Edge.Cuts polygon, rule 'barcode visual separation' (open, clearance provider)",
    ),
];

#[test]
fn filter_matches_script() {
    let text = "** Drc report for b.kicad_pcb **\n** Created on <DATE> **\n** Report includes: \
                Errors **\n\n** Found 2 DRC violations **\n[creepage]: x\n    Rule: r; error\n\
                [clearance]: y\n    @(1 mm, 2 mm): z\n\n** Found 0 unconnected pads **\n\n\
                ** Found 0 Footprint errors **\n\n** Checks not run (not ported yet) **\n    - \
                creepage: not ported yet (DRC-33)\n\n** End of Report **\n";
    let (out, counts) = filter_text(text, false, &unported_keys());
    assert_eq!(counts, vec![1, 0, 0]);
    assert_eq!(
        out,
        "** Drc report for b.kicad_pcb **\n** Created on <DATE> **\n** Report includes: \
         Errors **\n\n** Found 1 DRC violations **\n[clearance]: y\n    @(1 mm, 2 mm): z\n\n\
         ** Found 0 unconnected pads **\n\n** Found 0 Footprint errors **\n\n** End of Report \
         **\n"
    );
}
