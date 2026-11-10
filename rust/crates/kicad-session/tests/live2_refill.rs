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
//! twice from the same checkpoint, with `refill_zones_affected( commit damage )` (the
//! session's own record of the damage plus the commits' reports) and with the full
//! `refill_zones`, and the two results compared: every zone's fill vertex for
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
    let (mut refills, mut fulls) = (0usize, 0usize);

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
            refills += 1;
            fulls += usize::from(r.full);

            s.restore(cp);
            s.refill_zones().expect("full refill");
            let full = fill_state(&s);
            let full_text = s.save().expect("save");

            if affected != full && std::env::var("LIVE2_DEBUG").is_ok() {
                let dir =
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/live2");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("affected.txt"), affected.join("\n"));
                let _ = std::fs::write(dir.join("full.txt"), full.join("\n"));
                let refilled: Vec<String> = r.refilled.iter().map(|h| h.to_string()).collect();
                let _ = std::fs::write(dir.join("refilled.txt"), refilled.join("\n"));
                let mut info = String::new();
                for z in s.zones_info(false) {
                    let bb = z.outline.iter().flat_map(|p| p.outline.iter()).fold(
                        [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
                        |b, p| {
                            [
                                b[0].min(p[0]),
                                b[1].min(p[1]),
                                b[2].max(p[0]),
                                b[3].max(p[1]),
                            ]
                        },
                    );
                    info.push_str(&format!(
                        "{} id={} refilled={} net={} layers={:?} prio={} td={} island={} bbox={bb:?}\n",
                        z.uuid, z.id, r.refilled.contains(&z.id), z.net, z.layers, z.priority,
                        z.teardrop, z.island_mode
                    ));
                }
                info.push_str(&format!("damage {:?}\n", damage));
                let _ = std::fs::write(dir.join("zones.txt"), info);
            }

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
        "live2 refill: {boards} boards, {commits} commits, {refills} refills ({fulls} full), \
         {refilled} of {total} zone fills recomputed"
    );
    assert!(boards > 0, "no board with zones found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The boards, restricted to those containing `LIVE2_FILTER` when it is set.
fn boards_list() -> Vec<String> {
    let filter = std::env::var("LIVE2_FILTER").unwrap_or_default();
    boards()
        .into_iter()
        .filter(|b| b.contains(&filter))
        .collect()
}

/// Without changes the affected refill keeps every fill: equal to a second full refill.
#[test]
fn affected_refill_without_changes_equals_full_refill() {
    for rel in boards_list() {
        let Some(mut s) = load(&rel) else { continue };

        if !has_copper_zones(&s) {
            continue;
        }

        s.refill_zones().expect("refill");
        let cp = s.checkpoint();
        let r = s
            .refill_zones_affected(&kicad_session::board_api::refill::ChangeDamage::default())
            .expect("affected");
        let affected = fill_state(&s);
        s.restore(cp);
        s.refill_zones().expect("refill");
        let full = fill_state(&s);
        let bad = affected.iter().zip(&full).filter(|(a, b)| a != b).count();
        assert_eq!(
            bad,
            0,
            "{rel}: {bad} zones differ; refilled {}",
            r.refilled.len()
        );
    }
}

// ------------------------------------------------------------------------------------------
// Targeted cases (from the independent review of LIVE-2): each compares the affected refill
// with the full refill exactly (fill state of every zone and the saved board) on a small
// synthetic board; plus the reviewer's campaigns on other boards and seeds, which run only
// with their environment variables (`LIVE2R_LIST`, `LIVE2R_LOCAL_LIST`, `LIVE2R_BATCHES`,
// `LIVE2R_SEED`).
// ------------------------------------------------------------------------------------------

mod review_cases {
    use kicad_session::board_api::check::{
        CHECK_ITEMS_PROVIDERS, CheckItemsOptions, QuickDrcOptions,
    };
    use kicad_session::board_api::commit::Transaction;
    use kicad_session::board_api::edit::{ItemChange, NewItem, NewTrack, NewVia};
    use kicad_session::board_api::polygons;
    use kicad_session::board_api::refill::{ChangeDamage, RefillReport};
    use kicad_session::{LoadInputs, Session};

    use super::live1_common::*;

    // ------------------------------------------------------------------------------------------
    // Comparison
    // ------------------------------------------------------------------------------------------

    /// Every board zone's label and fill state (as `live2_refill.rs`).
    fn fill_state(s: &Session) -> Vec<(String, String)> {
        s.board
            .zones()
            .map(|(_, z)| {
                let f = z.fill();
                let label = format!(
                    "{}[{} net {} td={}]",
                    z.zone_name,
                    z.base.item.uuid.as_string(),
                    z.base.net.0,
                    z.is_teardrop_area()
                );
                let mut line = format!(
                    "filled={} need={} flags={:?} islands={:?} hash={:?}",
                    f.is_filled,
                    f.need_refill,
                    f.fill_flags.seq(),
                    f.insulated_islands,
                    f.filled_polys_hash,
                );

                for (layer, poly) in &f.filled_polys_list {
                    line.push_str(&format!(" {layer:?}={:?}", polygons(poly)));
                }

                (label, line)
            })
            .collect()
    }

    /// The polygons only (what the board looks like), for "did the full refill change this zone".
    fn fill_shapes(s: &Session) -> Vec<(String, String)> {
        s.board
            .zones()
            .map(|(_, z)| {
                let f = z.fill();
                let mut line = String::new();

                for (layer, poly) in &f.filled_polys_list {
                    line.push_str(&format!(" {layer:?}={:?}", polygons(poly)));
                }

                (z.base.item.uuid.as_string(), line)
            })
            .collect()
    }

    struct Outcome {
        equal: bool,
        text_equal: bool,
        /// Zones whose state differs between the affected and the full refill.
        differing: Vec<String>,
        /// Zones (by UUID) whose polygons the full refill changed relative to the state before.
        changed_in_full: usize,
        /// The full refill's connectivity builds gave an item another net (`RefillReport::damage`
        /// of the reference path: damage for the next refill).
        full_refill_renetted: bool,
        report: RefillReport,
    }

    impl Outcome {
        fn describe(&self) -> String {
            format!(
                "equal={} saved_equal={} differing={:?} (full refill changed {} zones, re-netted items: {}; affected: full={} refilled={} kept={} reasons={:?})",
                self.equal,
                self.text_equal,
                self.differing,
                self.changed_in_full,
                self.full_refill_renetted,
                self.report.full,
                self.report.refilled.len(),
                self.report.kept,
                self.report.reasons
            )
        }
    }

    /// Affected refill and full refill from the same state; the session is left full-refilled.
    fn compare(s: &mut Session, damage: &ChangeDamage) -> Outcome {
        let before: std::collections::HashMap<String, String> =
            fill_shapes(s).into_iter().collect();
        let cp = s.checkpoint();
        let report = s.refill_zones_affected(damage).expect("affected refill");
        let affected = fill_state(s);
        let affected_text = s.save().expect("save");
        s.restore(cp);
        let nets_before = item_nets(s);
        s.refill_zones().expect("full refill");
        let full_refill_renetted = item_nets(s) != nets_before;
        let full = fill_state(s);
        let full_text = s.save().expect("save");

        let mut differing = Vec::new();

        if affected.len() != full.len() {
            differing.push(format!("zone count {} vs {}", affected.len(), full.len()));
        }

        for (a, f) in affected.iter().zip(&full) {
            if a != f {
                differing.push(a.0.clone());
            }
        }

        let changed_in_full = fill_shapes(s)
            .into_iter()
            .filter(|(u, shape)| before.get(u).is_some_and(|b| b != shape))
            .count();

        Outcome {
            equal: differing.is_empty(),
            text_equal: affected_text == full_text,
            differing,
            changed_in_full,
            full_refill_renetted,
            report,
        }
    }

    /// Handle -> net of every track, arc, via and zone.
    fn item_nets(s: &Session) -> Vec<(u64, i32)> {
        let mut v: Vec<(u64, i32)> = s.route_items().into_iter().map(|r| (r.id, r.net)).collect();
        v.extend(s.zones_info(false).into_iter().map(|z| (z.id, z.net)));
        v.sort();
        v
    }

    // ------------------------------------------------------------------------------------------
    // Synthetic boards
    // ------------------------------------------------------------------------------------------

    #[derive(Clone, Copy)]
    enum Pad {
        /// SMD pad on F.Cu.
        SmdFront,
        /// Plated through hole on every copper layer.
        Thru,
        /// Non-plated hole with mask layers only.
        Npth,
    }

    struct Synth {
        nets: Vec<String>,
        items: String,
        seq: u32,
        four_layers: bool,
    }

    fn mm(x: f64, y: f64) -> [i32; 2] {
        [(x * 1e6).round() as i32, (y * 1e6).round() as i32]
    }

    impl Synth {
        fn new(nets: &[&str], four_layers: bool) -> Synth {
            Synth {
                nets: nets.iter().map(|n| n.to_string()).collect(),
                items: String::new(),
                seq: 0,
                four_layers,
            }
        }

        fn uuid(&mut self) -> String {
            self.seq += 1;
            format!("aaaaaaaa-0000-4000-8000-{:012x}", self.seq)
        }

        fn net_name(&self, net: usize) -> String {
            if net == 0 {
                String::new()
            } else {
                self.nets[net - 1].clone()
            }
        }

        fn edge(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
            let u = self.uuid();
            self.items.push_str(&format!(
                "\t(gr_rect (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.1) (type default)) (fill no) (layer \"Edge.Cuts\") (uuid \"{u}\"))\n"
            ));
        }

        fn pad(&mut self, reference: &str, kind: Pad, x: f64, y: f64, net: usize) {
            let (fu, ru, vu, pu) = (self.uuid(), self.uuid(), self.uuid(), self.uuid());
            let name = self.net_name(net);
            let pad = match kind {
                Pad::SmdFront => format!(
                    "(pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\" \"F.Paste\") (net {net} \"{name}\") (uuid \"{pu}\"))"
                ),
                Pad::Thru => format!(
                    "(pad \"1\" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8) (layers \"*.Cu\" \"*.Mask\") (remove_unused_layers no) (net {net} \"{name}\") (uuid \"{pu}\"))"
                ),
                Pad::Npth => format!(
                    "(pad \"\" np_thru_hole circle (at 0 0) (size 3 3) (drill 3) (layers \"*.Mask\") (uuid \"{pu}\"))"
                ),
            };
            let attr = match kind {
                Pad::SmdFront => "smd",
                _ => "through_hole",
            };
            self.items.push_str(&format!(
                "\t(footprint \"synth:P\"\n\t\t(layer \"F.Cu\")\n\t\t(uuid \"{fu}\")\n\t\t(at {x} {y})\n\
                 \t\t(property \"Reference\" \"{reference}\" (at 0 -2 0) (layer \"F.SilkS\") (uuid \"{ru}\") (effects (font (size 1 1) (thickness 0.15))))\n\
                 \t\t(property \"Value\" \"V\" (at 0 2 0) (layer \"F.Fab\") (uuid \"{vu}\") (effects (font (size 1 1) (thickness 0.15))))\n\
                 \t\t(attr {attr})\n\t\t{pad}\n\t)\n"
            ));
        }

        fn track(
            &mut self,
            a: (f64, f64),
            b: (f64, f64),
            w: f64,
            layer: &str,
            net: usize,
        ) -> String {
            let u = self.uuid();
            self.items.push_str(&format!(
                "\t(segment (start {} {}) (end {} {}) (width {w}) (layer \"{layer}\") (net {net}) (uuid \"{u}\"))\n",
                a.0, a.1, b.0, b.1
            ));
            u
        }

        fn via(&mut self, at: (f64, f64), net: usize, extra: &str) -> String {
            let u = self.uuid();
            self.items.push_str(&format!(
                "\t(via (at {} {}) (size 0.8) (drill 0.4) (layers \"F.Cu\" \"B.Cu\") {extra} (net {net}) (uuid \"{u}\"))\n",
                at.0, at.1
            ));
            u
        }

        /// A solid-connection copper zone on a rectangle. `island_mode`: 0 always, 1 never.
        #[allow(clippy::too_many_arguments)]
        fn zone(
            &mut self,
            name: &str,
            net: usize,
            layers: &[&str],
            priority: u32,
            clearance: f64,
            island_mode: u32,
            r: (f64, f64, f64, f64),
            extra: &str,
        ) {
            let u = self.uuid();
            let net_name = self.net_name(net);
            let layer_token = if layers.len() == 1 {
                format!("(layer \"{}\")", layers[0])
            } else {
                format!(
                    "(layers {})",
                    layers
                        .iter()
                        .map(|l| format!("\"{l}\""))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            self.items.push_str(&format!(
                "\t(zone\n\t\t(net {net})\n\t\t(net_name \"{net_name}\")\n\t\t{layer_token}\n\t\t(uuid \"{u}\")\n\t\t(name \"{name}\")\n\
                 \t\t(hatch edge 0.5)\n\t\t(priority {priority})\n\t\t(connect_pads yes (clearance {clearance}))\n\
                 \t\t(min_thickness 0.25)\n\t\t(filled_areas_thickness no)\n\
                 \t\t(fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode {island_mode}))\n\
                 \t\t(polygon (pts (xy {} {}) (xy {} {}) (xy {} {}) (xy {} {})))\n{extra}\t)\n",
                r.0, r.1, r.2, r.1, r.2, r.3, r.0, r.3
            ));
        }

        /// A rule area that allows everything (only its name matters, for `intersectsArea`).
        fn rule_area(&mut self, name: &str, layer: &str, r: (f64, f64, f64, f64)) {
            let u = self.uuid();
            self.items.push_str(&format!(
                "\t(zone\n\t\t(net 0)\n\t\t(net_name \"\")\n\t\t(layer \"{layer}\")\n\t\t(uuid \"{u}\")\n\t\t(name \"{name}\")\n\
                 \t\t(hatch edge 0.5)\n\t\t(connect_pads (clearance 0))\n\t\t(min_thickness 0.25)\n\t\t(filled_areas_thickness no)\n\
                 \t\t(keepout (tracks allowed) (vias allowed) (pads allowed) (copperpour allowed) (footprints allowed))\n\
                 \t\t(placement (enabled no) (sheetname \"\"))\n\
                 \t\t(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))\n\
                 \t\t(polygon (pts (xy {} {}) (xy {} {}) (xy {} {}) (xy {} {})))\n\t)\n",
                r.0, r.1, r.2, r.1, r.2, r.3, r.0, r.3
            ));
        }

        fn text(&self) -> String {
            let mut t = String::from(
                "(kicad_pcb\n\t(version 20241229)\n\t(generator \"pcbnew\")\n\t(generator_version \"9.0\")\n\
                 \t(general\n\t\t(thickness 1.6)\n\t\t(legacy_teardrops no)\n\t)\n\t(paper \"A4\")\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n",
            );

            if self.four_layers {
                t.push_str("\t\t(4 \"In1.Cu\" signal)\n\t\t(6 \"In2.Cu\" signal)\n");
            }

            t.push_str(
                "\t\t(2 \"B.Cu\" signal)\n\t\t(9 \"F.Adhes\" user \"F.Adhesive\")\n\t\t(11 \"B.Adhes\" user \"B.Adhesive\")\n\
                 \t\t(13 \"F.Paste\" user)\n\t\t(15 \"B.Paste\" user)\n\t\t(5 \"F.SilkS\" user \"F.Silkscreen\")\n\
                 \t\t(7 \"B.SilkS\" user \"B.Silkscreen\")\n\t\t(1 \"F.Mask\" user)\n\t\t(3 \"B.Mask\" user)\n\
                 \t\t(17 \"Dwgs.User\" user \"User.Drawings\")\n\t\t(19 \"Cmts.User\" user \"User.Comments\")\n\
                 \t\t(21 \"Eco1.User\" user \"User.Eco1\")\n\t\t(23 \"Eco2.User\" user \"User.Eco2\")\n\
                 \t\t(25 \"Edge.Cuts\" user)\n\t\t(27 \"Margin\" user)\n\t\t(31 \"F.CrtYd\" user \"F.Courtyard\")\n\
                 \t\t(29 \"B.CrtYd\" user \"B.Courtyard\")\n\t\t(35 \"F.Fab\" user)\n\t\t(33 \"B.Fab\" user)\n\t)\n\
                 \t(setup\n\t\t(pad_to_mask_clearance 0)\n\t)\n\t(net 0 \"\")\n",
            );

            for (i, n) in self.nets.iter().enumerate() {
                t.push_str(&format!("\t(net {} \"{n}\")\n", i + 1));
            }

            t.push_str(&self.items);
            t.push_str(")\n");
            t
        }

        /// The board loaded through the kicad-cli load sequence (DRC engine with `rules`).
        fn load(&self, rules: Option<&str>) -> Session {
            let text = self.text();
            Session::load_board(&text, "/synth/review.kicad_pcb", None, rules)
                .expect("synthetic board")
        }
    }

    fn zone_handle(s: &Session, name: &str) -> u64 {
        s.zones_info(false)
            .into_iter()
            .find(|z| z.name == name)
            .unwrap_or_else(|| panic!("zone {name}"))
            .id
    }

    fn new_track(a: (f64, f64), b: (f64, f64), layer: &str, net: i32) -> NewItem {
        NewItem::Track(NewTrack {
            start: mm(a.0, a.1),
            end: mm(b.0, b.1),
            width: Some(250_000),
            layer: layer.to_string(),
            net,
            ..NewTrack::default()
        })
    }

    // ------------------------------------------------------------------------------------------
    // Targeted cases
    // ------------------------------------------------------------------------------------------

    /// GND zone on the left; a GND track runs from inside the zone 30 mm out of it; a pad of net A
    /// far right. A commit joins the pad and the far end of the track with a track of net A: the
    /// net propagation gives the long GND track net A, so the zone must knock it out.
    fn propagation_board() -> Session {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.track((30.0, 25.0), (70.0, 25.0), 0.25, "F.Cu", 1);
        b.pad("PA", Pad::SmdFront, 90.0, 25.0, 2);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        s
    }

    /// Control of the next test: with the commit's own propagation the changed track is reported.
    #[test]
    fn propagation_by_commit_is_covered() {
        let mut s = propagation_board();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((90.0, 25.0), (70.0, 25.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        eprintln!("net changes: {:?}", r.net_changes);
        let out = compare(&mut s, &r.damage);
        eprintln!("propagation_by_commit_is_covered: {}", out.describe());
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// `Transaction::skip_connectivity` (wasm `skipConnectivity = true`): the commit does not
    /// propagate nets, the refill's own connectivity build does, and the item it re-nets (the long
    /// track inside the zone) is in no damage.
    #[test]
    fn skip_connectivity_commit_then_affected_refill() {
        let mut s = propagation_board();
        let mut txn = Transaction::begin();
        txn.skip_connectivity = true;
        txn.stage_add(new_track((90.0, 25.0), (70.0, 25.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "skip_connectivity_commit_then_affected_refill: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// A zone with a 10 mm local clearance sets the board's worst clearance and knocks 10 mm out
    /// of a lower-priority zone 8 mm away. Removing it (a transaction removal by handle) changes
    /// the worst clearance to 0.2 mm; the lower zone is then outside the seed margin.
    #[test]
    fn removing_the_zone_that_sets_the_worst_clearance() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZH", 2, &["F.Cu"], 2, 10.0, 1, (10.0, 10.0, 30.0, 40.0), "");
        b.pad("PA", Pad::SmdFront, 20.0, 25.0, 2);
        b.zone("ZL", 1, &["F.Cu"], 1, 0.2, 1, (38.0, 10.0, 70.0, 40.0), "");
        b.pad("PG", Pad::SmdFront, 65.0, 25.0, 1);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");

        let mut txn = Transaction::begin();
        txn.stage_remove(zone_handle(&s, "ZH"));
        let r = s.commit(&txn).expect("commit");
        assert_eq!(r.edit.removed.len(), 1, "zone not removed");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "removing_the_zone_that_sets_the_worst_clearance: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// The same with a second commit between the removal and the refill (merged damage): any
    /// cached worst clearance of the board is gone by then.
    #[test]
    fn removing_the_zone_that_sets_the_worst_clearance_then_another_commit() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZH", 2, &["F.Cu"], 2, 10.0, 1, (10.0, 10.0, 30.0, 40.0), "");
        b.pad("PA", Pad::SmdFront, 20.0, 25.0, 2);
        b.zone("ZL", 1, &["F.Cu"], 1, 0.2, 1, (38.0, 10.0, 70.0, 40.0), "");
        b.pad("PG", Pad::SmdFront, 65.0, 25.0, 1);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");

        let mut txn = Transaction::begin();
        txn.stage_remove(zone_handle(&s, "ZH"));
        let r = s.commit(&txn).expect("commit");
        let mut damage = r.damage.clone();

        // a track far from everything (bottom right), on the other layer
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((90.0, 50.0), (95.0, 50.0), "B.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        damage.merge(&r.damage);

        // a DRC call in between, as a router loop does
        let summary = s
            .check_items(&r.edit.added, &CheckItemsOptions::default())
            .expect("check_items");
        damage.merge(&summary.damage);

        let out = compare(&mut s, &damage);
        eprintln!(
            "removing_the_zone_that_sets_the_worst_clearance_then_another_commit: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// A non-plated hole whose pad is on the mask layers only knocks its hole out of every copper
    /// layer (`knockoutThermalReliefs`: "NPTH pads with a drill hole affect all copper layers").
    /// Removing it by handle records a damage area on the mask layers.
    #[test]
    fn removing_an_npth_pad_on_mask_layers() {
        let mut b = Synth::new(&["GND"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.pad("H1", Pad::Npth, 25.0, 25.0, 0);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");

        let pad = s
            .pads(false)
            .into_iter()
            .find(|p| p.reference == "H1")
            .expect("pad")
            .id;
        let mut txn = Transaction::begin();
        txn.stage_remove(pad);
        let r = s.commit(&txn).expect("commit");
        eprintln!(
            "removing_an_npth_pad_on_mask_layers: removed {:?}, damage {:?}, pads left {}",
            r.edit.removed,
            r.damage,
            s.pads(false).len()
        );
        let out = compare(&mut s, &r.damage);
        eprintln!("removing_an_npth_pad_on_mask_layers: {}", out.describe());
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// Island removal reads the connectivity of the whole net: the right half of a pour (split by
    /// a foreign track) hangs on a via and a long B.Cu route to a pad 40 mm away. Removing the
    /// last B.Cu segment (far from the pour, on another layer) orphans the half; adding it back
    /// reconnects it.
    #[test]
    fn far_track_splits_and_joins_the_cluster_of_an_island() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 0, (10.0, 10.0, 40.0, 40.0), "");
        b.track((25.0, 5.0), (25.0, 45.0), 0.5, "F.Cu", 2);
        b.pad("P1", Pad::SmdFront, 15.0, 25.0, 1);
        b.via((35.0, 25.0), 1, "");
        b.track((35.0, 25.0), (80.0, 25.0), 0.25, "B.Cu", 1);
        let last = b.track((80.0, 25.0), (80.0, 50.0), 0.25, "B.Cu", 1);
        b.pad("P2", Pad::Thru, 80.0, 50.0, 1);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        let handles = uuid_handles(&s);

        let mut txn = Transaction::begin();
        txn.stage_remove(handles[&last]);
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("split: {}", out.describe());
        assert!(
            out.changed_in_full > 0,
            "split: the case does not probe anything"
        );
        assert!(out.equal && out.text_equal, "split: {}", out.describe());

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((80.0, 25.0), (80.0, 50.0), "B.Cu", 1));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("join: {}", out.describe());
        assert!(
            out.changed_in_full > 0,
            "join: the case does not probe anything"
        );
        assert!(out.equal && out.text_equal, "join: {}", out.describe());
    }

    /// An inner-layer pour connected to anything only through one via; a second pour of the same
    /// net on another inner layer hangs on the first through a second via.
    #[test]
    fn removing_the_one_via_of_an_inner_pour() {
        let mut b = Synth::new(&["GND", "A"], true);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone(
            "ZI1",
            1,
            &["In1.Cu"],
            0,
            0.3,
            0,
            (10.0, 10.0, 40.0, 40.0),
            "",
        );
        b.zone(
            "ZI2",
            1,
            &["In2.Cu"],
            0,
            0.3,
            0,
            (30.0, 10.0, 90.0, 40.0),
            "",
        );
        // an outline of ZI2 of its own, cut off by a foreign track on In2
        b.track((60.0, 5.0), (60.0, 45.0), 0.5, "In2.Cu", 2);
        b.pad("P1", Pad::SmdFront, 15.0, 25.0, 1);
        let v1 = b.via((15.0, 25.0), 1, "");
        b.via((35.0, 25.0), 1, "");
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        let handles = uuid_handles(&s);

        let mut txn = Transaction::begin();
        txn.stage_remove(handles[&v1]);
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("removing_the_one_via_of_an_inner_pour: {}", out.describe());
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// Three zones in a priority chain, each overlapping the next. The middle zone's body is an
    /// island (removed) until a track from its pad, which lies under the highest zone, reaches it;
    /// then it knocks out the lowest zone 20 mm from the edit.
    fn chain_board() -> Session {
        let mut b = Synth::new(&["A", "B", "C"], false);
        b.edge(0.0, 0.0, 120.0, 60.0);
        b.zone("Z1", 1, &["F.Cu"], 3, 0.3, 0, (10.0, 10.0, 30.0, 40.0), "");
        b.zone("Z2", 2, &["F.Cu"], 2, 0.3, 0, (25.0, 10.0, 65.0, 40.0), "");
        b.zone("Z3", 3, &["F.Cu"], 1, 0.3, 0, (60.0, 10.0, 100.0, 40.0), "");
        b.pad("PA", Pad::SmdFront, 15.0, 25.0, 1);
        b.pad("PB", Pad::SmdFront, 27.0, 25.0, 2);
        b.pad("PC", Pad::SmdFront, 95.0, 25.0, 3);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        s
    }

    #[test]
    fn edit_under_a_higher_zone_reaches_a_zone_two_zones_away() {
        let mut s = chain_board();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((27.0, 25.0), (40.0, 25.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "edit_under_a_higher_zone_reaches_a_zone_two_zones_away: {}",
            out.describe()
        );
        assert!(
            out.changed_in_full >= 2,
            "the case does not probe the chain: {}",
            out.describe()
        );
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// The wasm `Board` bookkeeping, replayed natively: damage merged per commit, saved with a
    /// checkpoint, reset by a refill, restored by a rollback.
    #[test]
    fn damage_before_a_checkpoint_and_refill_after_a_rollback() {
        let mut s = chain_board();
        let mut damage = ChangeDamage::default(); // after the full refill of chain_board()

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((27.0, 25.0), (40.0, 25.0), "F.Cu", 2));
        damage.merge(&s.commit(&txn).expect("commit").damage);

        let cp = (s.checkpoint(), damage.clone()); // Board::checkpoint

        let r = s.refill_zones_affected(&damage).expect("refill"); // Board::refillZonesAffected
        damage = r.damage.clone();

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((90.0, 15.0), (95.0, 15.0), "F.Cu", 3));
        damage.merge(&s.commit(&txn).expect("commit").damage);
        let r = s.refill_zones_affected(&damage).expect("refill");
        damage = r.damage.clone();
        assert!(damage.is_empty(), "unexpected net damage of a refill");

        s.rollback(&cp.0); // Board::rollback
        damage = cp.1.clone();

        let out = compare(&mut s, &damage);
        eprintln!(
            "damage_before_a_checkpoint_and_refill_after_a_rollback: {}",
            out.describe()
        );
        assert!(out.changed_in_full >= 2, "{}", out.describe());
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// A custom rule gives every item that touches the rule area `KO` a 2 mm clearance. A track
    /// that ends in `KO` crosses a pour 30 mm away. Removing the rule area (transaction removal
    /// by handle; recorded as a local damage area) changes the track's clearance in the pour.
    #[test]
    fn removing_a_rule_area_a_custom_rule_names() {
        let rules = "(version 1)\n(rule big\n  (condition \"A.intersectsArea('KO')\")\n  (constraint clearance (min 2mm)))\n";
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.track((25.0, 25.0), (75.0, 25.0), 0.25, "F.Cu", 2);
        b.rule_area("KO", "F.Cu", (70.0, 20.0, 80.0, 30.0));
        let mut s = b.load(Some(rules));
        assert!(s.drc_init_error.is_none(), "{:?}", s.drc_init_error);
        s.refill_zones().expect("refill");

        let mut txn = Transaction::begin();
        txn.stage_remove(zone_handle(&s, "KO"));
        let r = s.commit(&txn).expect("commit");
        assert_eq!(r.edit.removed.len(), 1, "rule area not removed");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "removing_a_rule_area_a_custom_rule_names: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// A custom rule gives the items on the path between two pads (`fromTo`) a larger clearance.
    /// The path is completed by a track 25 mm from the pour its first segment crosses. `fromTo`
    /// reads the from-to cache, which a DRC run rebuilds (`checkItems` in the router loop).
    fn from_to_board() -> Session {
        from_to_board_spelled("fromTo")
    }

    /// [`from_to_board`] with the function name spelled `name` (function names are
    /// case-insensitive in rule conditions).
    fn from_to_board_spelled(name: &str) -> Session {
        let rules = format!(
            "(version 1)\n(rule ft\n  (condition \"A.{name}('R1-1', 'R2-1')\")\n  (constraint clearance (min 1.5mm)))\n"
        );
        let mut b = Synth::new(&["GND", "N"], false);
        b.edge(0.0, 0.0, 100.0, 80.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (20.0, 40.0, 45.0, 60.0), "");
        b.pad("G1", Pad::SmdFront, 22.0, 42.0, 1);
        b.pad("G2", Pad::SmdFront, 22.0, 58.0, 1);
        b.pad("R1", Pad::SmdFront, 10.0, 50.0, 2);
        b.pad("R2", Pad::SmdFront, 90.0, 50.0, 2);
        b.track((10.0, 50.0), (50.0, 50.0), 0.25, "F.Cu", 2);
        b.track((50.0, 50.0), (70.0, 50.0), 0.25, "F.Cu", 2);
        let mut s = b.load(Some(&rules));
        assert!(s.drc_init_error.is_none(), "{:?}", s.drc_init_error);
        s.refill_zones().expect("refill");
        s
    }

    /// [`from_to_rule_with_a_drc_run_between_commit_and_refill`] with the other letter cases of
    /// the function name: each must give a full refill equal to the reference.
    #[test]
    fn from_to_rule_in_any_letter_case() {
        for name in ["fromto", "FROMTO", "FromTo"] {
            let mut s = from_to_board_spelled(name);
            s.check_items(&[], &CheckItemsOptions::default())
                .expect("check_items");
            let mut damage = ChangeDamage::default();
            let mut txn = Transaction::begin();
            txn.stage_add(new_track((70.0, 50.0), (90.0, 50.0), "F.Cu", 2));
            let r = s.commit(&txn).expect("commit");
            damage.merge(&r.damage);
            let summary = s
                .check_items(&r.edit.added, &CheckItemsOptions::default())
                .expect("check_items");
            damage.merge(&summary.damage);

            let out = compare(&mut s, &damage);
            eprintln!("from_to_rule_in_any_letter_case {name}: {}", out.describe());
            assert!(out.report.full, "{name}: the refill must be a full one");
            assert!(out.equal && out.text_equal, "{name}: {}", out.describe());
        }
    }

    #[test]
    fn from_to_rule_with_a_drc_run_between_commit_and_refill() {
        let mut s = from_to_board();
        // the router loop's order: checkItems of the base state, commit, checkItems, refill
        let base = s
            .check_items(&[], &CheckItemsOptions::default())
            .expect("check_items");
        let mut damage = base.damage.clone();
        let out = compare(&mut s, &damage);
        eprintln!("from_to base: {}", out.describe());

        damage = ChangeDamage::default();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((70.0, 50.0), (90.0, 50.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        damage.merge(&r.damage);
        let summary = s
            .check_items(&r.edit.added, &CheckItemsOptions::default())
            .expect("check_items");
        damage.merge(&summary.damage);

        let out = compare(&mut s, &damage);
        eprintln!(
            "from_to_rule_with_a_drc_run_between_commit_and_refill: {}",
            out.describe()
        );
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    #[test]
    fn from_to_rule_without_a_drc_run() {
        let mut s = from_to_board();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((70.0, 50.0), (90.0, 50.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("from_to_rule_without_a_drc_run: {}", out.describe());
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// A via that removes unused layers: its flashing on an inner layer follows a track on that
    /// layer; a pour of another net on a second inner layer and one of its own net on a third are
    /// around it.
    #[test]
    fn conditional_via_flashing_follows_a_track_on_another_layer() {
        let mut b = Synth::new(&["GND", "A"], true);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone(
            "ZA",
            2,
            &["In1.Cu"],
            0,
            0.3,
            1,
            (10.0, 10.0, 40.0, 40.0),
            "",
        );
        b.zone(
            "ZG",
            1,
            &["In2.Cu"],
            0,
            0.3,
            1,
            (10.0, 10.0, 40.0, 40.0),
            "",
        );
        b.zone("ZB", 2, &["B.Cu"], 0, 0.3, 1, (24.0, 24.0, 60.0, 40.0), "");
        b.pad("PA", Pad::Thru, 15.0, 15.0, 2);
        b.pad("PG", Pad::Thru, 35.0, 35.0, 1);
        let v = b.via(
            (25.0, 25.0),
            1,
            "(remove_unused_layers yes) (keep_end_layers no)",
        );
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        let handles = uuid_handles(&s);
        assert!(handles.contains_key(&v));

        // a GND track on In1 that touches the via and leaves the pours quickly
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((25.0, 25.0), (25.0, 5.0), "In1.Cu", 1));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("flash add: {}", out.describe());
        assert!(out.equal && out.text_equal, "add: {}", out.describe());

        // and one on B.Cu, then the In1 one removed again
        let added = r.edit.added.clone();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((25.0, 25.0), (5.0, 25.0), "B.Cu", 1));
        txn.stage_remove(added[0]);
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!("flash swap: {}", out.describe());
        assert!(out.equal && out.text_equal, "swap: {}", out.describe());
    }

    /// API hazard, not asserted: the fills of a loaded board are the file's. An affected refill
    /// with the damage of the first commit (instead of `ChangeDamage::full()`) keeps them.
    #[test]
    fn hazard_affected_refill_on_file_fills() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone(
            "ZG",
            1,
            &["F.Cu"],
            0,
            0.3,
            1,
            (10.0, 10.0, 40.0, 40.0),
            "\t\t(filled_polygon (layer \"F.Cu\") (pts (xy 11 11) (xy 20 11) (xy 20 20)))\n",
        );
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.pad("PA", Pad::SmdFront, 90.0, 25.0, 2);
        let mut s = b.load(None);

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((90.0, 25.0), (80.0, 25.0), "B.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "hazard_affected_refill_on_file_fills (precondition violated on purpose): {}",
            out.describe()
        );
        assert!(
            out.report.full,
            "an affected refill on the file's fills must be a full one"
        );
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// Control of `skip_connectivity_commit_then_affected_refill`: with `finalize_tracked()` (wasm
    /// `finalize()`) between the commit and the refill the propagated nets are damage.
    #[test]
    fn skip_connectivity_commit_then_finalize_is_covered() {
        let mut s = propagation_board();
        let mut txn = Transaction::begin();
        txn.skip_connectivity = true;
        txn.stage_add(new_track((90.0, 25.0), (70.0, 25.0), "F.Cu", 2));
        let mut damage = s.commit(&txn).expect("commit").damage;
        damage.merge(&s.finalize_tracked());
        let out = compare(&mut s, &damage);
        eprintln!(
            "skip_connectivity_commit_then_finalize_is_covered: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// The session's damage record (the precondition of the identity): known after a refill,
    /// kept by commits, DRC runs, saves and checkpoints, unknown (the next affected refill is a
    /// full one) after every operation that does not report its damage.
    #[test]
    fn untracked_operations_make_the_next_affected_refill_full() {
        use kicad_session::board_api::edit::EditBatch;

        let fresh = || {
            let (s, _) = free_via_board();
            assert!(!s.fill_damage().full, "a refill leaves a known record");
            s
        };

        // tracked: commit, check_items, save, checkpoint / rollback
        let mut s = fresh();
        let mut txn = Transaction::begin();
        txn.stage_add(new_track((80.0, 50.0), (85.0, 50.0), "B.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        s.check_items(&r.edit.added, &CheckItemsOptions::default())
            .expect("check_items");
        s.save().expect("save");
        let cp = s.checkpoint();
        assert!(!s.fill_damage().full && !s.fill_damage().is_empty());
        // This board has no teardrop target: the update changes nothing (no time stamp
        // bump), so the record stays known. On a board with a teardrop it becomes full.
        s.update_teardrops().expect("teardrops");
        assert!(!s.fill_damage().full, "update_teardrops without a target");
        s.rollback(&cp);
        assert!(!s.fill_damage().full, "rollback restores the record");
        let report = s
            .refill_zones_affected(&ChangeDamage::default())
            .expect("refill");
        assert!(!report.full);

        // apply_edits is begin + stage + commit, so it is tracked like a commit
        let mut s = fresh();
        s.apply_edits(&EditBatch {
            add: vec![new_track((80.0, 50.0), (85.0, 50.0), "B.Cu", 2)],
            ..EditBatch::default()
        })
        .expect("apply_edits");
        assert!(!s.fill_damage().full && !s.fill_damage().is_empty(), "apply_edits");
        assert!(
            !s.refill_zones_affected(&ChangeDamage::default())
                .expect("refill")
                .full
        );

        // untracked
        let mut s = fresh();
        s.rules_text = Some("(version 1)\n".to_string());
        assert!(s.fill_damage().full, "rules text");

        let mut s = fresh();
        s.board.design_settings_mut();
        assert!(s.fill_damage().full, "design settings");

        // a load: the fills are the file's
        let s = free_via_board_unfilled();
        assert!(s.fill_damage().full, "load");
    }

    /// The positive case of `update_teardrops` in
    /// [`untracked_operations_make_the_next_affected_refill_full`]: on a board with a teardrop
    /// target (a via with teardrops enabled and a track ending on it) the update removes and
    /// re-creates the teardrop zone the refill before it made, bumps the time stamp, and the
    /// record becomes unknown.
    #[test]
    fn update_teardrops_with_a_target_makes_the_record_full() {
        const TD: &str = "(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) \
             (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) \
             (allow_two_segments yes) (prefer_zone_connections yes))";
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.track((60.0, 30.0), (75.0, 30.0), 0.25, "F.Cu", 2);
        b.via((75.0, 30.0), 2, TD);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        assert!(!s.fill_damage().full, "a refill leaves a known record");

        let outcome = s.update_teardrops().expect("teardrops");
        assert!(
            outcome.teardrops > 0,
            "the case has no teardrop target: {outcome:?}"
        );
        assert!(s.fill_damage().full, "update_teardrops with a target");
        assert!(
            s.refill_zones_affected(&ChangeDamage::default())
                .expect("refill")
                .full
        );
    }

    fn free_via_board_unfilled() -> Session {
        let mut b = Synth::new(&["GND"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.load(None)
    }

    /// GND pour on the left, a GND track from inside it to 30 mm outside (no pad on it), pads of
    /// net A far right. The edit of the tests below: a FREE via of net A on the far end of the
    /// track, by an ordinary commit.
    fn free_via_board() -> (Session, u64) {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        let t = b.track((30.0, 25.0), (70.0, 25.0), 0.25, "F.Cu", 1);
        b.pad("PA", Pad::SmdFront, 90.0, 50.0, 2);
        b.pad("PB", Pad::SmdFront, 95.0, 50.0, 2);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");
        let t = uuid_handles(&s)[&t];
        (s, t)
    }

    fn free_via(at: (f64, f64), net: i32) -> NewItem {
        NewItem::Via(NewVia {
            at: mm(at.0, at.1),
            diameter: Some(600_000),
            drill: Some(300_000),
            net,
            free: true,
            ..NewVia::default()
        })
    }

    /// A free via keeps its net. The commit's incremental propagation takes the track's net as
    /// the origin of the pad-less cluster { track, via } (the track is first in the item list) and
    /// changes nothing; a full connectivity build (`BULK_INSERT` puts the new via at the front of
    /// `BOARD::Tracks()`) takes the via's net and gives it to the track, which lies in the pour.
    /// The refill starts with that full build; the track is in no damage.
    #[test]
    fn free_via_on_a_dangling_track_far_from_the_pour() {
        let (mut s, track) = free_via_board();
        let net_before = item_nets(&s).iter().find(|(h, _)| *h == track).unwrap().1;
        let mut txn = Transaction::begin();
        txn.stage_add(free_via((70.0, 25.0), 2));
        let r = s.commit(&txn).expect("commit");
        let net_live = item_nets(&s).iter().find(|(h, _)| *h == track).unwrap().1;
        let out = compare(&mut s, &r.damage);
        let net_after = item_nets(&s).iter().find(|(h, _)| *h == track).unwrap().1;
        eprintln!(
            "free_via_on_a_dangling_track_far_from_the_pour: track net {net_before} -> {net_live} (commit; net changes {:?}) -> {net_after} (refill); {}",
            r.net_changes,
            out.describe()
        );
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// The same with a `checkItems` between the commit and the refill: the DRC's full
    /// connectivity build re-nets the track and reports it (`DrcSummary::damage`), which hides
    /// the case above in a loop that checks before every refill.
    #[test]
    fn free_via_on_a_dangling_track_with_a_drc_run_is_covered() {
        let (mut s, _) = free_via_board();
        let mut txn = Transaction::begin();
        txn.stage_add(free_via((70.0, 25.0), 2));
        let r = s.commit(&txn).expect("commit");
        let mut damage = r.damage.clone();
        let summary = s
            .check_items(&r.edit.added, &CheckItemsOptions::default())
            .expect("check_items");
        damage.merge(&summary.damage);
        let out = compare(&mut s, &damage);
        eprintln!(
            "free_via_on_a_dangling_track_with_a_drc_run_is_covered: drc damage {:?}; {}",
            summary.damage,
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// `check_items` reads the nets of the given items before the DRC run, whose connectivity
    /// build can give them another net (same board): the `unconnected_items` entries "of the
    /// items' nets" are then those of the old net.
    #[test]
    fn check_items_unconnected_entries_follow_the_net_after_the_run() {
        let (mut s, track) = free_via_board();
        let mut txn = Transaction::begin();
        txn.stage_add(free_via((70.0, 25.0), 2));
        s.commit(&txn).expect("commit");

        let scoped = s
            .check_items(&[track], &CheckItemsOptions::default())
            .expect("check_items");
        let net_now = item_nets(&s).iter().find(|(h, _)| *h == track).unwrap().1;
        let mut nets: std::collections::HashMap<u64, i32> = item_nets(&s).into_iter().collect();
        nets.extend(s.pads(false).into_iter().map(|p| (p.id, p.net)));

        let full = s
            .quick_drc(&QuickDrcOptions {
                providers: CHECK_ITEMS_PROVIDERS
                    .iter()
                    .map(|p| p.to_string())
                    .collect(),
                all_track_errors: true,
                severities: Vec::new(),
            })
            .expect("quick_drc");
        let expected = full
            .violations
            .iter()
            .filter(|v| {
                v.violation_type == "unconnected_items"
                    && v.items
                        .iter()
                        .any(|i| i.id.and_then(|id| nets.get(&id)) == Some(&net_now))
            })
            .count();
        let got = scoped
            .violations
            .iter()
            .filter(|v| v.violation_type == "unconnected_items")
            .count();
        eprintln!(
            "check_items_unconnected_entries_follow_the_net_after_the_run: track net now {net_now}, unconnected entries of that net in the full run {expected}, returned {got}"
        );
        assert!(expected > 0, "the case does not probe anything");
        assert_eq!(got, expected);
    }

    /// Control of the two zone-removal counterexamples: removing a zone whose effects are local
    /// (the top zone of the chain) is covered by its damage area.
    #[test]
    fn removing_a_zone_with_local_effects_is_covered() {
        let mut s = chain_board();
        let mut txn = Transaction::begin();
        txn.stage_remove(zone_handle(&s, "Z1"));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "removing_a_zone_with_local_effects_is_covered: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    // ------------------------------------------------------------------------------------------
    // Second review pass (cases added after the first reviewer's)
    // ------------------------------------------------------------------------------------------

    impl Synth {
        /// An SMD pad on F.Cu with extra pad tokens (e.g. a local clearance).
        fn pad_with(&mut self, reference: &str, x: f64, y: f64, net: usize, extra: &str) {
            let (fu, ru, vu, pu) = (self.uuid(), self.uuid(), self.uuid(), self.uuid());
            let name = self.net_name(net);
            self.items.push_str(&format!(
                "\t(footprint \"synth:P\"\n\t\t(layer \"F.Cu\")\n\t\t(uuid \"{fu}\")\n\t\t(at {x} {y})\n\
                 \t\t(property \"Reference\" \"{reference}\" (at 0 -2 0) (layer \"F.SilkS\") (uuid \"{ru}\") (effects (font (size 1 1) (thickness 0.15))))\n\
                 \t\t(property \"Value\" \"V\" (at 0 2 0) (layer \"F.Fab\") (uuid \"{vu}\") (effects (font (size 1 1) (thickness 0.15))))\n\
                 \t\t(attr smd)\n\t\t(pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\" \"F.Paste\") (net {net} \"{name}\") {extra} (uuid \"{pu}\"))\n\t)\n"
            ));
        }
    }

    /// The worst clearance of the board comes from a pad's local clearance (8 mm); the pad sits
    /// 6.5 mm from a pour of another net and knocks 1.5 mm out of it. Removing the pad (a
    /// transaction removal by handle, recorded as a local damage area) leaves a board whose worst
    /// clearance is the default 0.2 mm, so the pour is outside the seed margin computed from it.
    #[test]
    fn removing_the_pad_that_sets_the_worst_clearance() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.pad_with("PX", 31.0, 25.0, 2, "(clearance 8)");
        b.zone("ZL", 1, &["F.Cu"], 1, 0.2, 1, (38.0, 10.0, 70.0, 40.0), "");
        b.pad("PG", Pad::SmdFront, 65.0, 25.0, 1);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");

        let pad = s
            .pads(false)
            .into_iter()
            .find(|p| p.reference == "PX")
            .expect("pad")
            .id;
        let mut txn = Transaction::begin();
        txn.stage_remove(pad);
        let r = s.commit(&txn).expect("commit");
        assert_eq!(r.edit.removed.len(), 1, "pad not removed");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "removing_the_pad_that_sets_the_worst_clearance: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// After the wrong affected refill of `free_via_on_a_dangling_track_far_from_the_pour` the
    /// report carries the re-netted track as damage "for the next affected refill". Is the state
    /// right after that next refill (the wasm `Board` passes `RefillReport::damage` on)?
    #[test]
    fn free_via_case_heals_with_the_next_affected_refill() {
        let (mut s, _) = free_via_board();
        let mut txn = Transaction::begin();
        txn.stage_add(free_via((70.0, 25.0), 2));
        let r = s.commit(&txn).expect("commit");
        let first = s.refill_zones_affected(&r.damage).expect("refill");
        eprintln!(
            "free_via_case_heals_with_the_next_affected_refill: first refill refilled {} kept {} damage {:?}",
            first.refilled.len(),
            first.kept,
            first.damage
        );
        assert!(
            !first.damage.is_empty(),
            "the first refill reports no net damage"
        );
        let out = compare(&mut s, &first.damage);
        eprintln!(
            "free_via_case_heals_with_the_next_affected_refill: second refill {}",
            out.describe()
        );
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    /// `check_items` keeps the entries that NAME a given item. A track of another net that cuts a
    /// pour in two leaves an island (island mode "never": kept); the connectivity provider reports
    /// it as `isolated_copper` naming the zone only (`drc_test_provider_connectivity.cpp`:
    /// `drcItem->SetItems( zone )`); the scoped check keeps it (the zone meets the track).
    #[test]
    fn check_items_keeps_isolated_copper_the_item_causes() {
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 60.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 25.0, 1);
        b.pad("PA", Pad::SmdFront, 35.0, 5.0, 2);
        b.pad("PB", Pad::SmdFront, 35.0, 45.0, 2);
        let mut s = b.load(None);
        s.refill_zones().expect("refill");

        let count = |s: &mut Session, ids: Option<&[u64]>| -> (usize, usize) {
            let v = match ids {
                Some(ids) => {
                    s.check_items(ids, &CheckItemsOptions::default())
                        .expect("check_items")
                        .violations
                }
                None => {
                    s.quick_drc(&QuickDrcOptions {
                        providers: CHECK_ITEMS_PROVIDERS
                            .iter()
                            .map(|p| p.to_string())
                            .collect(),
                        all_track_errors: true,
                        severities: Vec::new(),
                    })
                    .expect("quick_drc")
                    .violations
                }
            };
            (
                v.iter()
                    .filter(|x| x.violation_type == "isolated_copper")
                    .count(),
                v.len(),
            )
        };

        let (before, _) = count(&mut s, None);

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((35.0, 5.0), (35.0, 45.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        s.refill_zones_affected(&r.damage).expect("refill");

        let (full_iso, full_all) = count(&mut s, None);
        let added = r.edit.added.clone();
        let (scoped_iso, scoped_all) = count(&mut s, Some(&added));
        eprintln!(
            "check_items_keeps_isolated_copper_the_item_causes: isolated_copper before the track {before}, after it: full run {full_iso} (of {full_all} entries), check_items(track) {scoped_iso} (of {scoped_all})"
        );
        assert_eq!(before, 0);
        assert!(full_iso > 0, "the track does not create an isolated island");
        assert_eq!(
            scoped_iso, full_iso,
            "check_items drops the zone-only entry"
        );
    }

    /// Control: a custom rule gives net A a 5 mm clearance (the board's worst value, read by the
    /// seed margin). A track of net A added 4 mm outside a pour knocks 1 mm out of it.
    #[test]
    fn custom_rule_clearance_reaches_a_pour_from_outside_is_covered() {
        let rules = "(version 1)\n(rule wide\n  (condition \"A.NetName == 'A'\")\n  (constraint clearance (min 5mm)))\n";
        let mut b = Synth::new(&["GND", "A"], false);
        b.edge(0.0, 0.0, 100.0, 60.0);
        b.zone("ZG", 1, &["F.Cu"], 0, 0.3, 1, (10.0, 10.0, 40.0, 40.0), "");
        b.pad("P1", Pad::SmdFront, 15.0, 15.0, 1);
        b.pad("PA", Pad::SmdFront, 44.5, 50.0, 2);
        let mut s = b.load(Some(rules));
        assert!(s.drc_init_error.is_none(), "{:?}", s.drc_init_error);
        s.refill_zones().expect("refill");

        let mut txn = Transaction::begin();
        txn.stage_add(new_track((44.125, 50.0), (44.125, 20.0), "F.Cu", 2));
        let r = s.commit(&txn).expect("commit");
        let out = compare(&mut s, &r.damage);
        eprintln!(
            "custom_rule_clearance_reaches_a_pour_from_outside_is_covered: {}",
            out.describe()
        );
        assert!(out.changed_in_full > 0, "the case does not probe anything");
        assert!(out.equal && out.text_equal, "{}", out.describe());
    }

    // ------------------------------------------------------------------------------------------
    // The random campaign on other boards with other seeds
    // ------------------------------------------------------------------------------------------

    fn env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    /// The kicad-cli load (project and custom rules next to the board), else the QA load of the
    /// author's campaign.
    fn load_full(rel: &str) -> Option<(Session, &'static str)> {
        let path = board_path(rel);
        let text = std::fs::read_to_string(&path).ok()?;
        let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
        let dru = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
        let name = path.to_string_lossy().to_string();

        if std::env::var("LIVE2R_QA_LOAD").is_err() {
            let loaded = std::panic::catch_unwind(|| {
                Session::load_board_with(&LoadInputs {
                    text: &text,
                    source_name: &name,
                    project_json: pro.as_deref(),
                    rules_text: dru.as_deref(),
                    ..LoadInputs::default()
                })
            });

            if let Ok(Ok(s)) = loaded {
                return Some((s, if dru.is_some() { "cli+dru" } else { "cli" }));
            }
        }

        load(rel).map(|s| (s, "qa"))
    }

    /// `LIVE2R_LIST`: comma separated board paths relative to the KiCad source root. Without it
    /// the test does nothing.
    #[test]
    fn review_campaign() {
        let Ok(list) = std::env::var("LIVE2R_LIST") else {
            return;
        };
        let n = env_usize("LIVE2R_BATCHES", 100);
        let seed = env_usize("LIVE2R_SEED", 0x5eed_7700) as u64;
        let fixpoint = std::env::var("LIVE2R_FIXPOINT").is_ok();
        let mut failures = Vec::new();
        let (mut commits, mut refills, mut refilled, mut total, mut fulls) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut not_fixed = 0usize;

        for (bi, rel) in list.split(',').map(str::trim).enumerate() {
            let t0 = std::time::Instant::now();
            let Some((mut s, how)) = load_full(rel) else {
                failures.push(format!("{rel}: load failed"));
                continue;
            };

            s.refill_zones().expect("full refill");
            let zones = s.board.zones().count();
            let mut handles = uuid_handles(&s);
            let mut generator = Gen::new(&s, seed + 977 * bi as u64);
            let (mut b_commits, mut b_refills, mut b_refilled, mut b_total) =
                (0, 0, 0usize, 0usize);

            let mut carry_full = false;

            'batches: for k in 0..n {
                // the reference (full) refill of the step before re-netted items: its
                // `RefillReport::damage` is damage for this refill; taken as full damage here
                let mut damage = if carry_full {
                    ChangeDamage::full()
                } else {
                    ChangeDamage::default()
                };

                // one commit, every other step two, every fifth three before the refill
                let rounds = 1 + (k % 2) + usize::from(k % 5 == 4);

                for round in 0..rounds {
                    let ops = generator.batch();
                    let txn = transaction(&ops, &handles);

                    match s.commit(&txn) {
                        Ok(r) => {
                            for (u, &h) in r.edit.added_uuids.iter().zip(&r.edit.added) {
                                handles.insert(u.clone(), h);
                            }

                            damage.merge(&r.damage);
                            b_commits += 1;
                        }
                        Err(e) => {
                            failures.push(format!("{rel} batch {k}.{round}: commit failed: {e}"));
                            break 'batches;
                        }
                    }
                }

                if fixpoint {
                    // would a full rebuild give items other nets than the commits did?
                    let cp = s.checkpoint();
                    let d = s.finalize_tracked();
                    s.restore(cp);

                    if !d.is_empty() {
                        not_fixed += 1;
                        eprintln!(
                            "{rel} batch {k}: a full rebuild re-nets {} areas (nets {:?})",
                            d.areas.len(),
                            d.nets
                        );
                    }
                }

                let out = compare(&mut s, &damage);
                carry_full = out.full_refill_renetted;

                if carry_full {
                    eprintln!(
                        "{rel} batch {k}: the full refill re-netted items (carried as full damage)"
                    );
                }

                b_refills += 1;
                b_refilled += out.report.refilled.len();
                b_total += s.board.zones().count();
                fulls += usize::from(out.report.full);

                if !out.equal || !out.text_equal {
                    failures.push(format!("{rel} batch {k}: {}", out.describe()));
                    break;
                }
            }

            eprintln!(
                "live2 review: {rel} [{how}] {zones} zones: {b_commits} commits, {b_refills} refills, {b_refilled} of {b_total} zone fills recomputed, {:.0} s",
                t0.elapsed().as_secs_f64()
            );
            commits += b_commits;
            refills += b_refills;
            refilled += b_refilled;
            total += b_total;
        }

        eprintln!(
            "live2 review campaign: {commits} commits, {refills} refills ({fulls} fell back to full), {refilled} of {total} zone fills recomputed, {not_fixed} states a full rebuild re-nets, {} failures",
            failures.len()
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ------------------------------------------------------------------------------------------
    // A campaign of small, local edits (most zones are kept, so a missed dependency shows)
    // ------------------------------------------------------------------------------------------

    struct LocalItem {
        id: u64,
        via: bool,
        start: [i32; 2],
        end: [i32; 2],
        net: i32,
    }

    struct LocalGen {
        rng: Rng,
        items: Vec<LocalItem>,
        pads: Vec<([i32; 2], i32)>,
        nets: Vec<i32>,
        layers: Vec<String>,
        /// `LIVE2R_CLEAN`: only edits a router makes on purpose (own net, no free vias, no net
        /// changes).
        clean: bool,
    }

    impl LocalGen {
        fn new(s: &Session, seed: u64) -> LocalGen {
            LocalGen {
                rng: Rng::new(seed),
                clean: std::env::var("LIVE2R_CLEAN").is_ok(),
                items: Vec::new(),
                pads: s
                    .pads(false)
                    .into_iter()
                    .map(|p| (p.position, p.net))
                    .collect(),
                nets: s.net_details().into_iter().map(|n| n.code).collect(),
                layers: s
                    .board_info()
                    .copper_layers
                    .into_iter()
                    .map(|l| {
                        kicad_session::board_api::layer_name(kicad_core::PcbLayerId::from(
                            l.layer_id,
                        ))
                    })
                    .collect(),
            }
        }

        fn sync(&mut self, s: &Session) {
            self.items = s
                .route_items()
                .into_iter()
                .map(|r| LocalItem {
                    id: r.id,
                    via: r.kind == "via",
                    start: r.start,
                    end: r.end,
                    net: r.net,
                })
                .collect();
        }

        fn anchor(&mut self) -> ([i32; 2], i32) {
            if self.items.is_empty() || (self.rng.chance(40) && !self.pads.is_empty()) {
                if self.pads.is_empty() {
                    return ([0, 0], 0);
                }

                self.pads[self.rng.below(self.pads.len())]
            } else {
                let i = self.rng.below(self.items.len());
                let it = &self.items[i];
                let p = if self.rng.chance(50) {
                    it.start
                } else {
                    it.end
                };
                (p, it.net)
            }
        }

        fn net_for(&mut self, own: i32) -> i32 {
            if self.clean {
                return own;
            }

            match self.rng.below(10) {
                0..=6 => own,
                7..=8 if !self.nets.is_empty() => self.nets[self.rng.below(self.nets.len())],
                _ => 0,
            }
        }

        /// One to three small edits around one or two anchors.
        fn batch(&mut self, s: &Session) -> Transaction {
            self.sync(s);
            let mut txn = Transaction::begin();
            let mut used: Vec<u64> = Vec::new();

            for _ in 0..1 + self.rng.below(3) {
                match self.rng.below(20) {
                    0..=8 => {
                        let (a, net) = self.anchor();
                        let b = [
                            a[0] + self.rng.offset(1_500_000),
                            a[1] + self.rng.offset(1_500_000),
                        ];
                        let net = self.net_for(net);
                        let layer = self.layers[self.rng.below(self.layers.len())].clone();
                        txn.stage_add(NewItem::Track(NewTrack {
                            start: a,
                            end: b,
                            width: Some(200_000),
                            layer,
                            net,
                            ..NewTrack::default()
                        }));
                    }
                    9..=12 => {
                        let (a, net) = self.anchor();
                        let net = self.net_for(net);
                        let free = self.rng.chance(40) && !self.clean;
                        txn.stage_add(NewItem::Via(NewVia {
                            at: a,
                            diameter: Some(600_000),
                            drill: Some(300_000),
                            net,
                            free,
                            ..NewVia::default()
                        }));
                    }
                    13..=16 => {
                        if self.items.is_empty() {
                            continue;
                        }

                        let i = self.rng.below(self.items.len());
                        let id = self.items[i].id;

                        if !used.contains(&id) {
                            used.push(id);
                            txn.stage_remove(id);
                        }
                    }
                    17..=18 => {
                        let vias: Vec<usize> = (0..self.items.len())
                            .filter(|&i| self.items[i].via)
                            .collect();

                        if vias.is_empty() {
                            continue;
                        }

                        let i = vias[self.rng.below(vias.len())];
                        let id = self.items[i].id;

                        if !used.contains(&id) {
                            used.push(id);
                            let p = self.items[i].start;
                            txn.stage_change(ItemChange {
                                id,
                                start: Some([
                                    p[0] + self.rng.offset(500_000),
                                    p[1] + self.rng.offset(500_000),
                                ]),
                                ..ItemChange::default()
                            });
                        }
                    }
                    _ => {
                        if self.items.is_empty() || self.nets.is_empty() || self.clean {
                            continue;
                        }

                        let i = self.rng.below(self.items.len());
                        let id = self.items[i].id;

                        if !used.contains(&id) {
                            used.push(id);
                            let net = self.nets[self.rng.below(self.nets.len())];
                            txn.stage_change(ItemChange {
                                id,
                                net: Some(net),
                                ..ItemChange::default()
                            });
                        }
                    }
                }
            }

            txn
        }
    }

    /// `LIVE2R_LOCAL_LIST`: comma separated board paths. Small edits, one or two commits per
    /// refill, the damage carried as the wasm `Board` does (commit damage merged, the reference
    /// refill's net changes taken as full damage).
    #[test]
    fn review_local_campaign() {
        let Ok(list) = std::env::var("LIVE2R_LOCAL_LIST") else {
            return;
        };
        let n = env_usize("LIVE2R_BATCHES", 100);
        let seed = env_usize("LIVE2R_SEED", 0x10ca_1e55) as u64;
        let mut failures = Vec::new();
        let (mut commits, mut refills, mut refilled, mut total, mut renetted) =
            (0usize, 0usize, 0usize, 0usize, 0usize);

        for (bi, rel) in list.split(',').map(str::trim).enumerate() {
            let t0 = std::time::Instant::now();
            let Some((mut s, how)) = load_full(rel) else {
                failures.push(format!("{rel}: load failed"));
                continue;
            };

            s.refill_zones().expect("full refill");
            let zones = s.board.zones().count();
            let mut generator = LocalGen::new(&s, seed + 7919 * bi as u64);
            let (mut b_commits, mut b_refills, mut b_refilled, mut b_total) =
                (0, 0, 0usize, 0usize);
            let mut carry_full = false;

            'batches: for k in 0..n {
                let mut damage = if carry_full {
                    ChangeDamage::full()
                } else {
                    ChangeDamage::default()
                };

                for round in 0..1 + (k % 2) {
                    let txn = generator.batch(&s);

                    match s.commit(&txn) {
                        Ok(r) => {
                            damage.merge(&r.damage);
                            b_commits += 1;
                        }
                        Err(e) => {
                            failures.push(format!("{rel} batch {k}.{round}: commit failed: {e}"));
                            break 'batches;
                        }
                    }
                }

                let cp = s.checkpoint();
                let d = s.finalize_tracked();
                s.restore(cp);
                let not_fixed = !d.is_empty();
                renetted += usize::from(not_fixed);

                let out = compare(&mut s, &damage);
                carry_full = out.full_refill_renetted;
                b_refills += 1;
                b_refilled += out.report.refilled.len();
                b_total += s.board.zones().count();

                if !out.equal || !out.text_equal {
                    failures.push(format!(
                        "{rel} batch {k} (a full rebuild re-nets items: {not_fixed}): {}",
                        out.describe()
                    ));

                    if std::env::var("LIVE2R_STOP").is_ok() {
                        break;
                    }

                    // the session is full-refilled: go on from there
                }
            }

            eprintln!(
                "live2 review local: {rel} [{how}] {zones} zones: {b_commits} commits, {b_refills} refills, {b_refilled} of {b_total} zone fills recomputed, {:.0} s",
                t0.elapsed().as_secs_f64()
            );
            commits += b_commits;
            refills += b_refills;
            refilled += b_refilled;
            total += b_total;
        }

        eprintln!(
            "live2 review local campaign: {commits} commits, {refills} refills, {refilled} of {total} zone fills recomputed, {renetted} states a full rebuild re-nets, {} failures",
            failures.len()
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
