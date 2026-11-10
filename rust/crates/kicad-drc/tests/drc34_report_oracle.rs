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

//! Replay of the DRC-34 reporter oracle (`tests/oracle/drc34_report_oracle.cpp`, golden
//! `tests/golden/drc34/evalrules_report.txt`, `tests/oracle/gen_drc34_report_golden.py`): the
//! `REPORTER` texts of `EvalRules`, `EvalZoneConnection` and `ProcessAssertions` for the
//! items and pairs the driver walks, compared per board and category (line count and hash).
//!
//! A board is compared when the port loads it completely (parser report without skipped
//! items); a category whose evaluation reaches a `port_pending!` stub is reported as pending.
//! `DRC34_REPORT_FILTER=<path substring>` limits the boards; `DRC34_REPORT_RAW=1` prints the
//! port's lines of every mismatching category (compare with
//! `gen_drc34_report_golden.py --raw <board>`).

mod drc_harness;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use drc_harness::{fnv1a64, load_board, panic_text, quiet};
use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::UNDEFINED_LAYER;
use kicad_core::{KicadT, Lset, PcbLayerId};
use kicad_drc::DrcConstraintT;
use kicad_drc::engine::DrcEngine;
use kicad_drc::reporter::{Reporter, WxStringReporter};
use kicad_pcb::{Board, BoardCtx, ItemRef};

const K_ITEMS: usize = 6;
const K_PAIRS: usize = 3;
const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;

/// Categories whose texts differ because of code outside this package (see the DRC-34 section
/// of PORT_STATUS.md); they are listed, not failed. Each is "<board>|<category>".
const KNOWN_OTHER_PACKAGES: [(&str, &str); 4] = [
    // The expression evaluator does not report its error callback ("ERROR: Unrecognized
    // layer '*.Silkscreen'" of existsOnLayer) to the reporter.
    (
        "qa/data/pcbnew/issue2512.kicad_pcb|inch",
        "pcbexpr error callback",
    ),
    (
        "qa/data/pcbnew/issue2512.kicad_pcb|pair_zt",
        "pcbexpr error callback",
    ),
    (
        "qa/data/pcbnew/issue2512.kicad_pcb|zone",
        "pcbexpr error callback",
    ),
    // `A.Layer == 'F.Silkscreen'` is true for the gr_text on F.SilkS in the port, false in the
    // reference: the rule result differs too, the known "layer map of the QA load path"
    // deviation of evalrules_oracle.rs.
    (
        "qa/data/pcbnew/issue24735/issue24735.kicad_pcb|drawing",
        "QA load path layer map (evalrules_oracle known deviation)",
    ),
];

/// Every `DRC_CONSTRAINT_T` after `CLEARANCE_CONSTRAINT`, in enum order.
const ALL_TYPES: [DrcConstraintT; 40] = {
    use DrcConstraintT::*;
    [
        CreepageConstraint,
        HoleClearanceConstraint,
        HoleToHoleConstraint,
        EdgeClearanceConstraint,
        HoleSizeConstraint,
        CourtyardClearanceConstraint,
        SilkClearanceConstraint,
        TextHeightConstraint,
        TextThicknessConstraint,
        TrackWidthConstraint,
        TrackSegmentLengthConstraint,
        AnnularWidthConstraint,
        ZoneConnectionConstraint,
        ThermalReliefGapConstraint,
        ThermalSpokeWidthConstraint,
        MinResolvedSpokesConstraint,
        SolderMaskExpansionConstraint,
        SolderPasteAbsMarginConstraint,
        SolderPasteRelMarginConstraint,
        DisallowConstraint,
        ViaDiameterConstraint,
        LengthConstraint,
        NetChainLengthConstraint,
        NetChainStubLengthConstraint,
        NetChainReturnPathConstraint,
        SkewConstraint,
        DiffPairGapConstraint,
        MaxUncoupledConstraint,
        DiffPairIntraSkewConstraint,
        ViaCountConstraint,
        PhysicalClearanceConstraint,
        PhysicalHoleClearanceConstraint,
        AssertionConstraint,
        ConnectionWidthConstraint,
        TrackAngleConstraint,
        ViaDanglingConstraint,
        BridgedMaskConstraint,
        SolderMaskSliverConstraint,
        MicroviaStackDepthConstraint,
        HoleAspectRatioConstraint,
    ]
};

/// `PAIR_TYPES` of the driver.
const PAIR_TYPES: [DrcConstraintT; 11] = {
    use DrcConstraintT::*;
    [
        HoleClearanceConstraint,
        HoleToHoleConstraint,
        DiffPairGapConstraint,
        PhysicalClearanceConstraint,
        PhysicalHoleClearanceConstraint,
        EdgeClearanceConstraint,
        SilkClearanceConstraint,
        CourtyardClearanceConstraint,
        CreepageConstraint,
        SkewConstraint,
        MaxUncoupledConstraint,
    ]
};

/// The lines of one category: count, hash and (for diagnosis) the lines; `Err` once an
/// evaluation panicked.
#[derive(Default)]
struct Cat {
    n: usize,
    hash: u64,
    lines: Vec<String>,
    pending: Option<String>,
}

#[derive(Default)]
struct Out {
    cats: BTreeMap<String, Cat>,
}

impl Out {
    /// One `r|` line: `key|field|messages`; `messages` `None` = the call panicked.
    fn emit(&mut self, cat: &str, key: &str, field: &str, messages: Result<String, String>) {
        let c = self.cats.entry(cat.to_string()).or_insert_with(|| Cat {
            hash: FNV_OFFSET,
            ..Cat::default()
        });

        match messages {
            Ok(m) => {
                let line = format!("{key}|{field}|{}", escape(&m));
                c.n += 1;
                c.hash = fnv1a64(c.hash, format!("{line}\n").as_bytes());
                c.lines.push(line);
            }
            Err(e) => {
                if c.pending.is_none() {
                    c.pending = Some(e);
                }
            }
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

fn field(type_: DrcConstraintT, layer: PcbLayerId) -> String {
    format!("c{}@{}", type_ as i32, layer.0)
}

fn copper_seq(board: &Board, set: Lset) -> Vec<PcbLayerId> {
    let mut s = set;
    s &= &board.enabled_layers();
    s &= Lset::all_cu_mask();
    s.seq().0
}

/// Runs `f` with a fresh `WX_STRING_REPORTER`; the messages, or the panic text.
fn with_reporter(f: impl FnOnce(&mut dyn Reporter)) -> Result<String, String> {
    let mut r = WxStringReporter::new();
    let result = catch_unwind(AssertUnwindSafe(|| f(&mut r)));

    match result {
        Ok(()) => Ok(r.messages_text().to_string()),
        Err(e) => Err(panic_text(&*e)),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_one(
    engine: &DrcEngine,
    ctx: &BoardCtx<'_>,
    out: &mut Out,
    cat: &str,
    key: &str,
    type_: DrcConstraintT,
    a: ItemRef<'_>,
    b: Option<ItemRef<'_>>,
    layer: PcbLayerId,
) {
    let messages = with_reporter(|r| {
        engine.eval_rules_reporting(ctx, type_, Some(a), b, layer, Some(r));
    });
    out.emit(cat, key, &field(type_, layer), messages);
}

fn unary(
    engine: &DrcEngine,
    ctx: &BoardCtx<'_>,
    out: &mut Out,
    cat: &str,
    index: usize,
    item: ItemRef<'_>,
    all_copper_layers: bool,
) {
    let key = index.to_string();
    let clearance = DrcConstraintT::ClearanceConstraint;

    if all_copper_layers {
        for layer in copper_seq(ctx.board, item.layer_set()) {
            eval_one(engine, ctx, out, cat, &key, clearance, item, None, layer);
        }
    } else {
        eval_one(
            engine,
            ctx,
            out,
            cat,
            &key,
            clearance,
            item,
            None,
            item.layer(),
        );
    }

    for type_ in ALL_TYPES {
        if type_ == DrcConstraintT::DisallowConstraint
            || type_ == DrcConstraintT::HoleToHoleConstraint
        {
            continue;
        }

        eval_one(engine, ctx, out, cat, &key, type_, item, None, item.layer());
    }
}

#[allow(clippy::too_many_arguments)]
fn pair(
    engine: &DrcEngine,
    ctx: &BoardCtx<'_>,
    out: &mut Out,
    cat: &str,
    i: usize,
    j: usize,
    a: ItemRef<'_>,
    b: ItemRef<'_>,
    zone_connection: bool,
) {
    let key = format!("{i}:{j}");
    let mut shared = a.layer_set();
    shared &= &b.layer_set();

    for layer in copper_seq(ctx.board, shared) {
        use DrcConstraintT::*;

        eval_one(
            engine,
            ctx,
            out,
            cat,
            &key,
            ClearanceConstraint,
            a,
            Some(b),
            layer,
        );

        if zone_connection {
            let messages = with_reporter(|r| {
                engine.eval_zone_connection(ctx, a, Some(b), layer, Some(r));
            });
            out.emit(cat, &key, &format!("zc@{}", layer.0), messages);

            for type_ in [ThermalReliefGapConstraint, ThermalSpokeWidthConstraint] {
                eval_one(engine, ctx, out, cat, &key, type_, a, Some(b), layer);
            }
        }
    }

    for type_ in PAIR_TYPES {
        eval_one(
            engine,
            ctx,
            out,
            cat,
            &key,
            type_,
            a,
            Some(b),
            UNDEFINED_LAYER,
        );
    }
}

fn net_of(item: &ItemRef<'_>) -> i32 {
    item.net_code().map_or(0, |n| n.0)
}

/// `dumpBoard` of the driver.
fn dump_board(board: &Board, engine: &mut DrcEngine) -> Out {
    let mut out = Out::default();

    let walk = {
        let ctx = BoardCtx::bare(board);
        let mut tracks = Vec::new();

        for (id, _) in board.tracks() {
            let item = ctx.item(id.item_id()).unwrap();
            let cat = match item.type_() {
                KicadT::PcbViaT => "via",
                KicadT::PcbArcT => "arc",
                _ => "track",
            };
            tracks.push((cat, id.item_id()));
        }

        let pads: Vec<_> = board
            .footprints()
            .flat_map(|(fp, _)| board.footprint_pads(fp).map(|(id, _)| id.item_id()))
            .collect();
        let zones: Vec<_> = board.zones().map(|(id, _)| id.item_id()).collect();
        let fp_zones: Vec<_> = board
            .footprints()
            .flat_map(|(fp, _)| board.footprint_zones(fp).map(|(id, _)| id.item_id()))
            .collect();
        let drawings: Vec<_> = board
            .drawings()
            .filter(|(id, _)| {
                let t = ctx.item(*id).unwrap().type_();
                t == KicadT::PcbShapeT || t == KicadT::PcbTextT || t == KicadT::PcbTextboxT
            })
            .map(|(id, _)| id)
            .collect();

        (tracks, pads, zones, fp_zones, drawings)
    };
    let (tracks, pads, zones, fp_zones, drawings) = walk;
    let first_k = |v: &Vec<kicad_pcb::ItemId>| v.iter().copied().take(K_ITEMS).collect::<Vec<_>>();

    let mut lists: Vec<(&str, Vec<kicad_pcb::ItemId>)> =
        vec![("track", vec![]), ("arc", vec![]), ("via", vec![])];

    for (cat, id) in &tracks {
        for (name, list) in lists.iter_mut() {
            if name == cat && list.len() < K_ITEMS {
                list.push(*id);
            }
        }
    }

    lists.push(("pad", first_k(&pads)));
    lists.push(("zone", first_k(&zones)));
    lists.push(("fpzone", first_k(&fp_zones)));
    lists.push(("drawing", first_k(&drawings)));

    {
        let engine_ref: &DrcEngine = engine;
        let ctx = BoardCtx::bare(board).with_rules(engine_ref);
        let item = |id| ctx.item(id).unwrap();

        for (cat, list) in &lists {
            for (i, id) in list.iter().enumerate() {
                unary(engine_ref, &ctx, &mut out, cat, i, item(*id), true);
                eval_one(
                    engine_ref,
                    &ctx,
                    &mut out,
                    "disallow",
                    &format!("{cat}{i}"),
                    DrcConstraintT::DisallowConstraint,
                    item(*id),
                    None,
                    UNDEFINED_LAYER,
                );

                let messages = with_reporter(|r| {
                    engine_ref.process_assertions(item(*id), &mut |_| {}, Some(r));
                });
                out.emit("assert", &format!("{cat}{i}"), "assert", messages);
            }
        }
    }

    engine.set_user_units(EdaUnits::Inch);

    {
        let engine_ref: &DrcEngine = engine;
        let ctx = BoardCtx::bare(board).with_rules(engine_ref);

        for (_, list) in &lists {
            if let Some(id) = list.first() {
                let item = ctx.item(*id).unwrap();
                unary(engine_ref, &ctx, &mut out, "inch", 0, item, false);
            }
        }
    }

    engine.set_user_units(EdaUnits::Mm);

    let engine_ref: &DrcEngine = engine;
    let ctx = BoardCtx::bare(board).with_rules(engine_ref);
    let item = |id| ctx.item(id).unwrap();
    let track_items: Vec<_> = tracks.iter().map(|(_, id)| *id).collect();

    let next_of_other_nets = |out: &mut Out, cat: &str, list: &[kicad_pcb::ItemId]| {
        for i in 0..list.len().min(K_ITEMS) {
            let mut found = 0;

            for j in i + 1..list.len() {
                if found >= K_PAIRS {
                    break;
                }

                if net_of(&item(list[j])) == net_of(&item(list[i])) {
                    continue;
                }

                pair(
                    engine_ref,
                    &ctx,
                    out,
                    cat,
                    i,
                    j,
                    item(list[i]),
                    item(list[j]),
                    false,
                );
                found += 1;
            }
        }
    };

    next_of_other_nets(&mut out, "pair_tt", &track_items);
    next_of_other_nets(&mut out, "pair_pp", &pads);

    if !pads.is_empty() {
        for i in 0..track_items.len().min(K_ITEMS) {
            let mut found = 0;

            for m in 0..3 * K_PAIRS {
                if found >= K_PAIRS {
                    break;
                }

                let p = (7 * i + m) % pads.len();

                if net_of(&item(pads[p])) == net_of(&item(track_items[i])) {
                    continue;
                }

                pair(
                    engine_ref,
                    &ctx,
                    &mut out,
                    "pair_tp",
                    i,
                    p,
                    item(track_items[i]),
                    item(pads[p]),
                    false,
                );
                found += 1;
            }
        }
    }

    for z in 0..zones.len().min(K_ITEMS) {
        let mut found = 0;

        for j in 0..track_items.len() {
            if found >= K_PAIRS {
                break;
            }

            if net_of(&item(track_items[j])) == net_of(&item(zones[z])) {
                continue;
            }

            pair(
                engine_ref,
                &ctx,
                &mut out,
                "pair_zt",
                z,
                j,
                item(zones[z]),
                item(track_items[j]),
                false,
            );
            found += 1;
        }

        for j in 0..pads.len().min(K_PAIRS) {
            pair(
                engine_ref,
                &ctx,
                &mut out,
                "pair_zp",
                z,
                j,
                item(pads[j]),
                item(zones[z]),
                true,
            );
        }

        if z + 1 < zones.len() {
            pair(
                engine_ref,
                &ctx,
                &mut out,
                "pair_zz",
                z,
                z + 1,
                item(zones[z]),
                item(zones[z + 1]),
                false,
            );
        }
    }

    out
}

#[test]
fn evalrules_report_texts_match_cxx() {
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/drc34/evalrules_report.txt");
    let Ok(golden) = std::fs::read_to_string(&golden_path) else {
        println!("no golden {}", golden_path.display());
        return;
    };

    let filter = std::env::var("DRC34_REPORT_FILTER").unwrap_or_default();
    let raw = std::env::var("DRC34_REPORT_RAW").is_ok();
    let root = kicad_qa::kicad_source_root();

    // board -> category -> (n, hash); None: the reference could not load it
    let mut reference: BTreeMap<String, Option<BTreeMap<String, (usize, String)>>> =
        BTreeMap::new();

    for line in golden.lines().filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split('|').collect();

        if parts.len() == 2 {
            reference.insert(parts[0].to_string(), None);
            continue;
        }

        let n: usize = parts[2].trim_start_matches("n=").parse().unwrap();
        let hash = parts[3].trim_start_matches("hash=").to_string();
        reference
            .entry(parts[0].to_string())
            .or_insert_with(|| Some(BTreeMap::new()))
            .as_mut()
            .unwrap()
            .insert(parts[1].to_string(), (n, hash));
    }

    let mut boards = 0;
    let mut matched = 0;
    let mut matched_lines = 0;
    let mut mismatches = Vec::new();
    let mut pending = Vec::new();
    let mut pending_boards = Vec::new();
    let mut known = Vec::new();

    for (rel, cats) in &reference {
        if !rel.contains(&filter) {
            continue;
        }

        let Some(cats) = cats else {
            pending_boards.push(format!("{rel}: reference load error"));
            continue;
        };

        let path = root.join(rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(|| -> Result<Out, String> {
                let mut loaded = load_board(&path)?;

                if !loaded.report.skipped.is_empty() {
                    return Err("parser skipped items".to_string());
                }

                Ok(dump_board(&loaded.board, &mut loaded.engine))
            }))
        });

        let out = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                pending_boards.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                pending_boards.push(format!("{rel}: panic: {}", panic_text(&*e)));
                continue;
            }
        };

        boards += 1;

        let names: std::collections::BTreeSet<&String> =
            cats.keys().chain(out.cats.keys()).collect();

        for name in names {
            let mine = out.cats.get(name);

            if let Some(p) = mine.and_then(|c| c.pending.as_ref()) {
                pending.push(format!("{rel}|{name}: {p}"));
                continue;
            }

            let mine_nh = mine.map(|c| (c.n, format!("{:016x}", c.hash)));
            let theirs = cats.get(name).cloned();

            let id = format!("{rel}|{name}");

            if mine_nh == theirs {
                matched += 1;
                matched_lines += theirs.map_or(0, |t| t.0);
            } else if let Some((_, cause)) = KNOWN_OTHER_PACKAGES.iter().find(|(k, _)| *k == id) {
                known.push(format!("{id}: {cause}"));
            } else {
                mismatches.push(format!(
                    "{rel}|{name}: reference {theirs:?}, port {mine_nh:?}"
                ));

                if raw && let Some(c) = mine {
                    for l in &c.lines {
                        println!("RAW {rel}|{name}|{l}");
                    }
                }
            }
        }
    }

    println!(
        "DRC-34 EvalRules reporter texts: {boards} boards compared, {matched} categories \
         ({matched_lines} calls) match, {} mismatch, {} known (other packages), {} categories \
         pending, {} boards pending",
        mismatches.len(),
        known.len(),
        pending.len(),
        pending_boards.len()
    );

    for m in &mismatches {
        println!("  mismatch: {m}");
    }

    for k in &known {
        println!("  known: {k}");
    }

    for p in pending.iter().take(40) {
        println!("  pending: {p}");
    }

    for p in pending_boards.iter().take(40) {
        println!("  pending board: {p}");
    }

    assert!(
        mismatches.is_empty(),
        "{} mismatching categories",
        mismatches.len()
    );
}
