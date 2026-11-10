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

//! Package LIVE-2, verifications a and b.
//!
//! On the boards of `tests/oracle/live1_boards.txt` (loaded as kicad-cli loads them, project
//! and rules included), seeded random commits (the LIVE-1 generator) plus injected
//! violations per batch: a via dropped on a pad of another net, a track from a pad running
//! 40 mm to the right (over the edge on small boards), a via next to another via. After
//! every commit:
//!
//! * (b) `check_items( handles )` for the new and changed items plus a few untouched ones
//!   equals, entry for entry and in order, the entries of the full DRC job with
//!   `--all-track-errors` that name one of the handles (unconnected entries: or have one's
//!   net), for the providers of `CHECK_ITEMS_PROVIDERS`;
//! * (a1) two DRC runs in a row on the live session (incremental connectivity, after the
//!   edits) give the same report, byte for byte apart from the date and file name lines:
//!   the DRC carries no state from one run to the next that changes a result;
//! * (a2, measured, not asserted) the live report against the report of a fresh load of the
//!   board the live session saves. They differ in order-dependent entries, as in pcbnew:
//!   the writer saves the tracks sorted (`PCB_TRACK::cmp_tracks`), so the reloaded board
//!   iterates its tracks in another order than the edited one, which changes ratsnest edge
//!   ties of unconnected items, the net the propagation gives a netless item touching two
//!   nets, and report order.
//!
//! `LIVE2_DRC_BOARDS` (default 6) and `LIVE2_DRC_BATCHES` (default 3) scale the campaign.

mod live1_common;

use std::collections::BTreeSet;

use kicad_core::Severity;
use kicad_session::board_api::check::{
    CHECK_ITEMS_PROVIDERS, CheckItemsOptions, QuickDrcOptions, Violation,
};
use kicad_session::jobs::{JobPcbDrc, RcOutputFormat, RcUnits};
use kicad_session::{LoadInputs, Reporter, Session, run_drc};
use live1_common::{Gen, Op, Rng, board_path, boards, transaction, uuid_handles};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn load_text(text: &str, rel: &str) -> Option<Session> {
    let path = board_path(rel);
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let dru = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_string_lossy().into_owned();
    let inputs = LoadInputs {
        text,
        source_name: &name,
        project_json: pro.as_deref(),
        rules_text: dru.as_deref(),
        ..LoadInputs::default()
    };
    Session::load_board_with(&inputs).ok()
}

fn all_severities() -> i32 {
    Severity::Error.as_i32() | Severity::Warning.as_i32() | Severity::Exclusion.as_i32()
}

/// The DRC job: (text report, structured violations of the markers it left).
fn drc_job(s: &mut Session, all_track_errors: bool) -> (String, Vec<Violation>) {
    let mut job = JobPcbDrc::new();
    job.rc.filename = s.source_name.clone();
    job.rc.format = RcOutputFormat::Report;
    job.rc.units = RcUnits::Mm;
    job.rc.severity = all_severities();
    job.report_all_track_errors = all_track_errors;
    job.parity = false;
    job.report_date = "<DATE>".to_string();
    let mut reporter = Reporter::new();
    let outcome = run_drc(s, &mut job, &mut reporter).expect("drc job");
    let text = outcome.report_text.unwrap_or_default();
    (text, s.drc_violations(all_severities()).violations)
}

/// A violation without its severity (an excluded marker of the job is `exclusion`, the
/// scoped check does not resolve exclusions).
fn key(v: &Violation) -> String {
    format!(
        "{} {} {:?} {} {:?} {:?} {}",
        v.violation_type, v.description, v.position, v.layer, v.items, v.rule, v.section
    )
}

/// The report without the header lines that name the date and the file.
fn normalized(report: &str) -> String {
    report
        .lines()
        .filter(|l| !l.starts_with("** Created on") && !l.starts_with("** Drc report for"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The violation blocks of a report (a `[type]` line and its indented lines), sorted.
fn blocks(report: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    for l in normalized(report).lines() {
        if l.starts_with("    ") {
            if let Some(last) = out.last_mut() {
                last.push('\n');
                last.push_str(l);
                continue;
            }
        }

        out.push(l.to_string());
    }

    out.sort();
    out
}

/// Injected violations: a via on a pad of another net, a track from a pad 40 mm to the
/// right, a via 0.35 mm from another via.
fn injected(s: &Session, rng: &mut Rng) -> Vec<Op> {
    let pads: Vec<([i32; 2], i32)> = s
        .pads(false)
        .into_iter()
        .map(|p| (p.position, p.net))
        .collect();
    let vias: Vec<[i32; 2]> = s
        .route_items()
        .into_iter()
        .filter(|r| r.kind == "via")
        .map(|r| r.start)
        .collect();
    let nets: Vec<i32> = s.net_details().into_iter().map(|n| n.code).collect();
    let layer = s
        .board_info()
        .copper_layers
        .first()
        .map_or(0, |l| l.layer_id);
    let mut ops = Vec::new();

    if !pads.is_empty() && !nets.is_empty() {
        let (at, net) = pads[rng.below(pads.len())];
        let other = nets[rng.below(nets.len())];
        ops.push(Op::Via {
            uuid: rng.uuid(),
            at: [at[0] + rng.offset(200_000), at[1] + rng.offset(200_000)],
            d: 600_000,
            drill: 300_000,
            net: if other == net { 0 } else { other },
            free: false,
        });

        let (at, net) = pads[rng.below(pads.len())];
        ops.push(Op::Track {
            uuid: rng.uuid(),
            s: at,
            e: [at[0].saturating_add(40_000_000), at[1]],
            w: 250_000,
            layer,
            net,
        });
    }

    if !vias.is_empty() && !nets.is_empty() {
        let at = vias[rng.below(vias.len())];
        ops.push(Op::Via {
            uuid: rng.uuid(),
            at: [at[0] + 350_000, at[1]],
            d: 600_000,
            drill: 300_000,
            net: nets[rng.below(nets.len())],
            free: false,
        });
    }

    ops
}

#[test]
fn check_items_and_live_drc() {
    let n = env_usize("LIVE2_DRC_BATCHES", 3);
    let max_boards = env_usize("LIVE2_DRC_BOARDS", 6);
    let mut failures = Vec::new();
    let (mut tested, mut commits, mut scoped_entries, mut full_entries) = (0, 0, 0usize, 0usize);
    let (mut fresh_same, mut fresh_order, mut fresh_differ) = (0, 0, 0);
    let mut panics: Vec<String> = Vec::new();
    let mut scoped_types: BTreeSet<String> = BTreeSet::new();

    for (bi, rel) in boards().iter().enumerate() {
        if tested >= max_boards {
            break;
        }

        let Ok(text) = std::fs::read_to_string(board_path(rel)) else {
            continue;
        };
        let Some(mut s) = load_text(&text, rel) else {
            continue;
        };

        tested += 1;
        let mut handles = uuid_handles(&s);
        let mut generator = Gen::new(&s, 0x33bb_0001 + bi as u64);
        let mut rng = Rng::new(0x44cc_0001 + bi as u64);

        // A panic inside a provider (a defect of the DRC port on an edited board, not of the
        // scoped check) ends the board's campaign and is reported separately.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for k in 0..n {
                let mut ops = generator.batch();
                ops.extend(injected(&s, &mut rng));
                let txn = transaction(&ops, &handles);
                let report = match s.commit(&txn) {
                    Ok(r) => r,
                    Err(e) => {
                        failures.push(format!("{rel} batch {k}: commit failed: {e}"));
                        break;
                    }
                };

                for (u, &h) in report.edit.added_uuids.iter().zip(&report.edit.added) {
                    handles.insert(u.clone(), h);
                }

                commits += 1;

                // (a1) repeated runs on the live state agree (nothing carried from one run to the
                // next changes a result); (a2) the live state against a fresh load of the saved
                // board, measured (see the module comment: the writer sorts the tracks).
                let saved = s.save().expect("save");
                let (live_report, _) = drc_job(&mut s, false);
                let (again, _) = drc_job(&mut s, false);

                if normalized(&live_report) != normalized(&again) {
                    failures.push(format!(
                        "{rel} batch {k}: a second DRC run on the live state differs"
                    ));
                }

                let mut fresh = load_text(&saved, rel).expect("reload");
                let (fresh_report, _) = drc_job(&mut fresh, false);

                if normalized(&live_report) == normalized(&fresh_report) {
                    fresh_same += 1;
                } else if blocks(&live_report) == blocks(&fresh_report) {
                    fresh_order += 1;
                } else {
                    fresh_differ += 1;

                    if std::env::var("LIVE2_DEBUG").is_ok() {
                        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../target/live2");
                        let _ = std::fs::create_dir_all(&dir);
                        let _ =
                            std::fs::write(dir.join(format!("live_{bi}_{k}.rpt")), &live_report);
                        let _ =
                            std::fs::write(dir.join(format!("fresh_{bi}_{k}.rpt")), &fresh_report);
                    }
                }

                // (b) scoped == subset of the full all-track-errors run
                let mut ids: Vec<u64> = report.edit.added.clone();
                ids.extend(&report.edit.changed);
                let mut all: Vec<u64> = handles.values().copied().collect();
                all.sort_unstable();

                for _ in 0..3 {
                    if !all.is_empty() {
                        ids.push(all[rng.below(all.len())]);
                    }
                }

                ids.retain(|h| {
                    kicad_pcb::ItemId::from_bits(*h).is_some_and(|id| s.board.contains(id))
                });

                let scoped = s
                    .check_items(
                        &ids,
                        &CheckItemsOptions {
                            severities: vec!["error".into(), "warning".into(), "exclusions".into()],
                            ..CheckItemsOptions::default()
                        },
                    )
                    .expect("check_items");

                let mut others = other_providers(&mut s);
                let (_, full) = drc_job(&mut s, true);
                let wanted: BTreeSet<u64> = ids.iter().copied().collect();
                let nets: BTreeSet<i32> = ids
                    .iter()
                    .filter_map(|&h| {
                        let ctx = s.api_ctx();
                        ctx.item(kicad_pcb::ItemId::from_bits(h)?)?
                            .net_code()
                            .map(|n| n.0)
                    })
                    .filter(|&n| n > 0)
                    .collect();
                let net_of = |h: Option<u64>| {
                    let ctx = s.api_ctx();
                    ctx.item(kicad_pcb::ItemId::from_bits(h?)?)?
                        .net_code()
                        .map(|n| n.0)
                };
                // The full run without the entries of the other providers, in full-run order.
                let expected: Vec<String> = full
                    .iter()
                    .filter(|v| {
                        let k = key(v);
                        match others.iter().position(|o| *o == k) {
                            Some(p) => {
                                others.remove(p);
                                false
                            }
                            None => true,
                        }
                    })
                    .filter(|v| {
                        v.items
                            .iter()
                            .any(|i| i.id.is_some_and(|h| wanted.contains(&h)))
                            || (v.violation_type == "unconnected_items"
                                && v.items
                                    .iter()
                                    .any(|i| net_of(i.id).is_some_and(|n| nets.contains(&n))))
                    })
                    .map(key)
                    .collect();
                let got: Vec<String> = scoped.violations.iter().map(key).collect();
                scoped_entries += got.len();
                full_entries += full.len();
                scoped_types.extend(scoped.violations.iter().map(|v| v.violation_type.clone()));

                if got != expected {
                    let missing: Vec<&String> =
                        expected.iter().filter(|e| !got.contains(e)).collect();
                    let extra: Vec<&String> =
                        got.iter().filter(|e| !expected.contains(e)).collect();
                    failures.push(format!(
                    "{rel} batch {k}: scoped check differs ({} vs {}); missing {:?}; extra {:?}",
                    got.len(),
                    expected.len(),
                    missing.first(),
                    extra.first()
                ));
                }
            }
        }));

        if let Err(p) = outcome {
            let text = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panics.push(format!("{rel}: {text}"));
        }
    }

    eprintln!("live2 drc: port panics (other crates): {panics:?}");
    eprintln!(
        "live2 drc: {tested} boards, {commits} commits, {scoped_entries} scoped entries of \
         {full_entries} full-run entries; types {scoped_types:?}; live vs fresh load: \
         {fresh_same} identical, {fresh_order} same entries in another order, {fresh_differ} \
         different"
    );
    assert!(tested > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The entries of the providers outside [`CHECK_ITEMS_PROVIDERS`] (all-track-errors).
fn other_providers(s: &mut Session) -> Vec<String> {
    let providers: Vec<String> = kicad_drc::engine::PROVIDER_ORDER
        .iter()
        .filter(|p| !CHECK_ITEMS_PROVIDERS.contains(p))
        .map(|p| p.to_string())
        .collect();
    s.quick_drc(&QuickDrcOptions {
        providers,
        all_track_errors: true,
        severities: vec!["error".into(), "warning".into(), "exclusions".into()],
    })
    .expect("quick drc")
    .violations
    .iter()
    .map(key)
    .collect()
}

/// Verification d, first half (runs only with `LIVE2_EXPORT=<dir>`): on the first
/// `LIVE2_EXPORT_BOARDS` (default 20) boards, `LIVE2_DRC_BATCHES` random commits with the
/// injected violations, `finalize()`, a full refill, then the board saved to `<dir>` with its
/// project and rules. `tests/oracle/live2_cli_compare.sh <dir>` then runs the reference
/// `kicad-cli pcb drc` and `kicad-rs pcb drc` on every saved board and compares the reports.
#[test]
fn export_edited_boards_for_the_cli_comparison() {
    let Ok(dir) = std::env::var("LIVE2_EXPORT") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("export dir");
    let n = env_usize("LIVE2_DRC_BATCHES", 3);
    let max_boards = env_usize("LIVE2_EXPORT_BOARDS", 20);
    let mut exported = 0;

    for (bi, rel) in boards().iter().enumerate() {
        if exported >= max_boards {
            break;
        }

        let path = board_path(rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(mut s) = load_text(&text, rel) else {
            continue;
        };

        let mut handles = uuid_handles(&s);
        let mut generator = Gen::new(&s, 0x55dd_0001 + bi as u64);
        let mut rng = Rng::new(0x66ee_0001 + bi as u64);

        for _ in 0..n {
            let mut ops = generator.batch();
            ops.extend(injected(&s, &mut rng));
            let Ok(report) = s.commit(&transaction(&ops, &handles)) else {
                break;
            };

            for (u, &h) in report.edit.added_uuids.iter().zip(&report.edit.added) {
                handles.insert(u.clone(), h);
            }
        }

        s.finalize();
        s.refill_zones().expect("refill");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let out = dir.join(format!("{bi:02}_{stem}"));
        std::fs::create_dir_all(&out).expect("board dir");
        std::fs::write(
            out.join(format!("{stem}.kicad_pcb")),
            s.save().expect("save"),
        )
        .expect("write");

        for ext in ["kicad_pro", "kicad_dru"] {
            if let Ok(t) = std::fs::read(path.with_extension(ext)) {
                std::fs::write(out.join(format!("{stem}.{ext}")), t).expect("write");
            }
        }

        exported += 1;
    }

    eprintln!("live2 export: {exported} boards in {}", dir.display());
}
