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

//! Package LIVE-2, verification c: affected-only refill == full refill.
//!
//! On the boards of `tests/oracle/live1_boards.txt` that have copper zones: one full refill,
//! then seeded random commits (one or, every other step, two before a refill) (the LIVE-1 generator: removals, moves, net changes, new tracks
//! and vias at pads, route ends and inside zones); after every commit the board is refilled
//! twice from the same checkpoint, with `refill_zones_affected( commit damage )` and with
//! the full `refill_zones`, and the two results compared: every zone's fill vertex for
//! vertex per layer, fill flags, `IsFilled`, `NeedRefill`, insulated islands, fill hashes,
//! the zone list, and the saved board byte for byte.
//!
//! `LIVE2_BATCHES` (default 6) sets the commits per board, `LIVE2_BOARDS` (default 12) the
//! number of boards (from the top of the list), `LIVE2_FILTER` a substring the board paths
//! must contain.

mod live1_common;

use kicad_session::Session;
use kicad_session::board_api::polygons;
use live1_common::*;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Every board zone's fill state as text (zone list order).
fn fill_state(s: &Session) -> Vec<String> {
    s.board
        .zones()
        .map(|(_, z)| {
            let f = z.fill();
            let mut line = format!(
                "{} filled={} need={} flags={:?} islands={:?} hash={:?}",
                z.base.item.uuid.as_string(),
                f.is_filled,
                f.need_refill,
                f.fill_flags.seq(),
                f.insulated_islands,
                f.filled_polys_hash,
            );

            for (layer, poly) in &f.filled_polys_list {
                line.push_str(&format!(" {layer:?}={:?}", polygons(poly)));
            }

            line
        })
        .collect()
}

fn has_copper_zones(s: &Session) -> bool {
    s.board
        .zones()
        .any(|(_, z)| !z.is_rule_area() && !z.is_teardrop_area())
}

#[test]
fn affected_refill_equals_full_refill() {
    let n = env_usize("LIVE2_BATCHES", 6);
    let max_boards = env_usize("LIVE2_BOARDS", 12);
    let mut failures = Vec::new();
    let (mut boards, mut commits, mut refilled, mut total) = (0, 0, 0usize, 0usize);

    for (bi, rel) in boards_list().iter().enumerate() {
        if boards >= max_boards {
            break;
        }

        let Some(mut s) = load(rel) else {
            continue;
        };

        if !has_copper_zones(&s) {
            continue;
        }

        boards += 1;
        s.refill_zones().expect("full refill");

        let mut handles = uuid_handles(&s);
        let mut generator = Gen::new(&s, 0x22aa_0001 + bi as u64);

        for k in 0..n {
            let ops = generator.batch();
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
            let mut damage = report.damage.clone();

            // every other step: a second commit before the refill (merged damage)
            if k % 2 == 1 {
                let ops = generator.batch();
                let txn = transaction(&ops, &handles);

                match s.commit(&txn) {
                    Ok(r) => {
                        for (u, &h) in r.edit.added_uuids.iter().zip(&r.edit.added) {
                            handles.insert(u.clone(), h);
                        }

                        damage.merge(&r.damage);
                        commits += 1;
                    }
                    Err(e) => {
                        failures.push(format!("{rel} batch {k}b: commit failed: {e}"));
                        break;
                    }
                }
            }

            let cp = s.checkpoint();

            let r = s.refill_zones_affected(&damage).expect("affected refill");
            let affected = fill_state(&s);
            let affected_text = s.save().expect("save");
            refilled += r.refilled.len();
            total += s.board.zones().count();

            s.restore(cp);
            s.refill_zones().expect("full refill");
            let full = fill_state(&s);
            let full_text = s.save().expect("save");

            if affected != full {
                let bad: Vec<String> = affected
                    .iter()
                    .zip(&full)
                    .filter(|(a, b)| a != b)
                    .map(|(a, _)| a.chars().take(120).collect())
                    .collect();
                failures.push(format!(
                    "{rel} batch {k}: fills differ ({} vs {} zones; first: {:?}; refilled {} of {})",
                    affected.len(),
                    full.len(),
                    bad.first(),
                    r.refilled.len(),
                    s.board.zones().count()
                ));
                break;
            }

            if affected_text != full_text {
                failures.push(format!("{rel} batch {k}: saved boards differ"));
                break;
            }
        }
    }

    eprintln!(
        "live2 refill: {boards} boards, {commits} commits, {refilled} of {total} zone fills recomputed"
    );
    assert!(boards > 0, "no board with zones found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The boards, restricted to those containing `LIVE2_FILTER` when it is set.
fn boards_list() -> Vec<String> {
    let filter = std::env::var("LIVE2_FILTER").unwrap_or_default();
    boards().into_iter().filter(|b| b.contains(&filter)).collect()
}
