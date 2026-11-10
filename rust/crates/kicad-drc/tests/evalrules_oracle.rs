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

//! Differential test of `DRC_ENGINE::EvalRules` (package DRC-04) against the C++ engine: the
//! Rust twin of `pcb-dump board --hash --sections evalrules`
//! (`rust/tools/pcb-dump/dump_evalrules.cpp`) replayed against the compact golden
//! `tests/golden/evalrules_corpus.txt` (`tests/oracle/gen_evalrules_goldens.py`).
//!
//! The load path is the dump tool's QA path (see `engine_oracle.rs`). Per board and key
//! category (unary results per item kind, keepout disallow, sampled pairs) the number of keys
//! and the hash of the hashed lines are compared; a category is compared only when the Rust
//! board yields as many keys as the C++ one, so the test tightens by itself as the board
//! parser grows. Boards with user rule conditions need the expression binding (DRC-21) and
//! are only counted; categories that hit a `port_pending!` stub (keepout predicate DRC-10,
//! footprint courtyards, the `Layer` property of tuning-profile rules) are pending.

mod common;

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use common::{SyntaxBinder, v_str};
use kicad_core::json::{self, ObjectOrder};
use kicad_core::json_settings::JsonSettingsInternals;
use kicad_core::{KicadT, Lset, PcbLayerId};
use kicad_drc::DrcConstraintT;
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_condition::EngineCondition;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::exclusion::KiapiDrcExclusionCodec;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_pcb::board::BoardProject;
use kicad_pcb::drc_api::DrcConstraint;
use kicad_pcb::io::{ParseOptions, ParseReport, parse_board_report};
use kicad_pcb::project::board_design_settings_json::load_board_design_settings;
use kicad_pcb::project::net_settings_json::load_net_settings;
use kicad_pcb::{Board, BoardCtx, ItemId, ItemRef, RuleResolver};

const K_PAIRS: usize = 3;

/// The QA load path (`engine_oracle.rs`, `load_board_qa`).
fn load_board_qa(path: &Path) -> Result<(Board, ParseReport, DrcEngine, bool), String> {
    let mut tuning_profiles = TuningProfiles::default();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let options = ParseOptions {
        allow_pending: true,
    };
    let (mut board, report) =
        parse_board_report(&text, &name, &options).map_err(|e| e.to_string())?;

    let pro = path.with_extension("kicad_pro");

    if pro.exists() {
        let doc = json::parse_bytes(&std::fs::read(&pro).unwrap(), ObjectOrder::Sorted, true)
            .map_err(|e| format!("project: {e:?}"))?;
        let mut parent = JsonSettingsInternals {
            original: doc.clone(),
            doc,
        };

        if !board.legacy_design_settings_loaded {
            let _ = load_board_design_settings(
                &mut parent,
                board.design_settings_mut(),
                std::rc::Rc::new(KiapiDrcExclusionCodec::default()),
            );
        }

        let (net_settings, _) = load_net_settings(Some(&parent));
        board.design_settings_mut().net_settings = net_settings;
        board.project = Some(BoardProject::default());

        tuning_profiles = read_tuning_profiles(&parent.doc);
    } else if path.with_extension("pro").exists() {
        return Err("legacy .pro project (not readable by the port)".to_string());
    }

    let dru = path.with_extension("kicad_dru");
    let dru_text = dru
        .exists()
        .then(|| String::from_utf8_lossy(&std::fs::read(&dru).unwrap()).to_string());
    let dru_path = dru.to_string_lossy().to_string();

    let mut engine = DrcEngine::new();
    // The real expression binding (DRC-21) compiles and evaluates the user rule conditions.
    let result = engine.init_engine(
        &mut board,
        dru_text.as_deref().map(|t| (t, dru_path.as_str())),
        default_layer_choices(),
        &tuning_profiles,
        None,
    );

    // KI_TEST::LoadBoard then calls BOARD::BuildConnectivity(), whose CONNECTIVITY_DATA::Build
    // triangulates the board zone fills (BOARD::CacheTriangulation); the keepout predicates of
    // the expression functions query the fills through those triangles (DRC-22).
    kicad_connectivity::data::cache_board_triangulation(&mut board);

    Ok((board, report, engine, result.is_err()))
}

/// The part of `TUNING_PROFILES::TUNING_PROFILES` (`common/project/tuning_profiles.cpp`, the
/// `tuning_profiles` object of the project file) that `loadImplicitRules` reads: per profile
/// of `tuning_profiles_impedance_geometric` its name, type and the signal layer, width and
/// diff pair gap of each layer entry. The Rust project model does not hold tuning profiles
/// yet, so the test reads them itself.
fn read_tuning_profiles(doc: &json::Value) -> TuningProfiles {
    use kicad_core::{Lset, PcbLayerId};
    use kicad_drc::engine::implicit_rules::{
        DelayProfileTrackPropagationEntry, TuningProfile, TuningProfileType,
    };

    let mut profiles = Vec::new();
    let list = doc
        .pointer("/tuning_profiles/tuning_profiles_impedance_geometric")
        .and_then(|v| v.as_array());

    for entry in list.into_iter().flatten() {
        let int = |v: &json::Value, key: &str| v[key].as_i64().unwrap_or(0) as i32;
        let entries = entry["layer_entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|layer| DelayProfileTrackPropagationEntry {
                signal_layer: PcbLayerId::from(Lset::name_to_layer(
                    layer["signal_layer"].as_str().unwrap_or(""),
                )),
                width: int(layer, "width"),
                diff_pair_gap: int(layer, "diff_pair_gap"),
            })
            .collect();

        profiles.push(TuningProfile {
            profile_name: entry["profile_name"].as_str().unwrap_or("").to_string(),
            type_: if int(entry, "type") == 1 {
                TuningProfileType::Differential
            } else {
                TuningProfileType::Single
            },
            track_propagation_entries: entries,
        });
    }

    TuningProfiles::new(profiles)
}

fn fnv1a64(h: u64, data: &[u8]) -> u64 {
    let mut h = h;

    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn value_text(v: &kicad_core::minoptmax::MinOptMax<i32>) -> String {
    let f = |has: bool, val: i32| {
        if has {
            val.to_string()
        } else {
            "none".to_string()
        }
    };
    format!(
        "{}/{}/{}",
        f(v.has_min(), v.min()),
        f(v.has_opt(), v.opt()),
        f(v.has_max(), v.max())
    )
}

/// `constraintText` of dump_evalrules.cpp.
fn constraint_text(c: &DrcConstraint) -> String {
    format!(
        "t={};v={};n={};mr={};im={};zc={};d={};s={}",
        c.type_ as i32,
        value_text(&c.value),
        v_str(&c.name()),
        c.min_rule
            .as_ref()
            .map_or("-".to_string(), |r| v_str(&c.rule_name(Some(r)))),
        i32::from(c.implicit_min),
        c.zone_connection as i32,
        c.disallow_flags,
        c.severity() as i32
    )
}

fn field(type_: DrcConstraintT, layer: PcbLayerId) -> String {
    format!("c{}@{}", type_ as i32, layer.0)
}

const UNARY_TYPES: [DrcConstraintT; 15] = [
    DrcConstraintT::TrackWidthConstraint,
    DrcConstraintT::ViaDiameterConstraint,
    DrcConstraintT::HoleSizeConstraint,
    DrcConstraintT::AnnularWidthConstraint,
    DrcConstraintT::EdgeClearanceConstraint,
    DrcConstraintT::HoleClearanceConstraint,
    DrcConstraintT::PhysicalClearanceConstraint,
    DrcConstraintT::SilkClearanceConstraint,
    DrcConstraintT::TextHeightConstraint,
    DrcConstraintT::SolderMaskExpansionConstraint,
    DrcConstraintT::SolderPasteAbsMarginConstraint,
    DrcConstraintT::SolderPasteRelMarginConstraint,
    DrcConstraintT::ZoneConnectionConstraint,
    DrcConstraintT::ThermalReliefGapConstraint,
    DrcConstraintT::ThermalSpokeWidthConstraint,
];

/// One key category: key count, line count and the running hash of "field=#hash\n".
#[derive(Default)]
struct Cat {
    keys: usize,
    lines: usize,
    hash: u64,
    /// The unhashed "field=value" lines, kept only while `EVALRULES_RAW_DIR` is set (to diff
    /// a mismatching category against `pcb-dump board --sections evalrules` without `--hash`).
    raw: Option<Vec<String>>,
}

impl Cat {
    fn new() -> Self {
        Cat {
            keys: 0,
            lines: 0,
            hash: FNV_OFFSET,
            raw: std::env::var_os("EVALRULES_RAW_DIR").map(|_| Vec::new()),
        }
    }

    fn key(&mut self) {
        self.keys += 1;
    }

    fn emit(&mut self, field: &str, value: &str) {
        let line = format!("{field}=#{:016x}\n", fnv1a64(FNV_OFFSET, value.as_bytes()));
        self.lines += 1;
        self.hash = fnv1a64(self.hash, line.as_bytes());

        if let Some(raw) = &mut self.raw {
            raw.push(format!("{field}={value}"));
        }
    }
}

fn copper_seq(board: &Board, set: Lset) -> Vec<PcbLayerId> {
    let mut s = set;
    s &= &board.enabled_layers();
    s &= Lset::all_cu_mask();
    s.seq().0
}

/// The item lists of dump_evalrules.cpp: (category, items).
struct Walk {
    tracks: Vec<(&'static str, ItemId)>,
    pads: Vec<ItemId>,
    zones: Vec<ItemId>,
    fp_zones: Vec<ItemId>,
    drawings: Vec<ItemId>,
}

fn walk(board: &Board, ctx: &BoardCtx<'_>) -> Walk {
    let tracks = board
        .tracks()
        .map(|(id, _)| {
            let item = ctx.item(id.item_id()).unwrap();
            let cat = match item.type_() {
                KicadT::PcbViaT => "via",
                KicadT::PcbArcT => "arc",
                _ => "track",
            };
            (cat, id.item_id())
        })
        .collect();
    let pads = board
        .footprints()
        .flat_map(|(fp, _)| board.footprint_pads(fp).map(|(id, _)| id.item_id()))
        .collect();
    let zones = board.zones().map(|(id, _)| id.item_id()).collect();
    let fp_zones = board
        .footprints()
        .flat_map(|(fp, _)| board.footprint_zones(fp).map(|(id, _)| id.item_id()))
        .collect();
    let drawings = board
        .drawings()
        .filter(|(id, _)| {
            let t = ctx.item(*id).unwrap().type_();
            t == KicadT::PcbShapeT || t == KicadT::PcbTextT || t == KicadT::PcbTextboxT
        })
        .map(|(id, _)| id)
        .collect();

    Walk {
        tracks,
        pads,
        zones,
        fp_zones,
        drawings,
    }
}

/// `dumpUnary` without the disallow line.
fn unary(engine: &DrcEngine, ctx: &BoardCtx<'_>, item: ItemRef<'_>, cat: &mut Cat) {
    cat.key();

    for layer in copper_seq(ctx.board, item.layer_set()) {
        let c = engine.eval_rules(
            ctx,
            DrcConstraintT::ClearanceConstraint,
            Some(item),
            None,
            layer,
        );
        cat.emit(
            &field(DrcConstraintT::ClearanceConstraint, layer),
            &constraint_text(&c),
        );
        let (own, _) = engine.cached_own_clearance(ctx, item, layer, false);
        cat.emit(&format!("own@{}", layer.0), &own.to_string());
    }

    let layer = item.layer();

    for type_ in UNARY_TYPES {
        let c = engine.eval_rules(ctx, type_, Some(item), None, layer);
        cat.emit(&field(type_, layer), &constraint_text(&c));
    }
}

fn disallow(engine: &DrcEngine, ctx: &BoardCtx<'_>, item: ItemRef<'_>, cat: &mut Cat) {
    cat.key();
    let layer = kicad_core::layer_ids::UNDEFINED_LAYER;
    let c = engine.eval_rules(
        ctx,
        DrcConstraintT::DisallowConstraint,
        Some(item),
        None,
        layer,
    );
    cat.emit(
        &field(DrcConstraintT::DisallowConstraint, layer),
        &constraint_text(&c),
    );
}

/// `dumpPair`.
fn pair(
    engine: &DrcEngine,
    ctx: &BoardCtx<'_>,
    a: ItemRef<'_>,
    b: ItemRef<'_>,
    zone_connection: bool,
    cat: &mut Cat,
) {
    cat.key();
    let mut shared = a.layer_set();
    shared &= &b.layer_set();

    for layer in copper_seq(ctx.board, shared) {
        let c = engine.eval_rules(
            ctx,
            DrcConstraintT::ClearanceConstraint,
            Some(a),
            Some(b),
            layer,
        );
        cat.emit(
            &field(DrcConstraintT::ClearanceConstraint, layer),
            &constraint_text(&c),
        );

        if zone_connection {
            let c = engine.eval_zone_connection(ctx, a, Some(b), layer, None);
            cat.emit(&format!("zc@{}", layer.0), &constraint_text(&c));

            for type_ in [
                DrcConstraintT::ThermalReliefGapConstraint,
                DrcConstraintT::ThermalSpokeWidthConstraint,
            ] {
                let c = engine.eval_rules(ctx, type_, Some(a), Some(b), layer);
                cat.emit(&field(type_, layer), &constraint_text(&c));
            }
        }
    }

    let layer = kicad_core::layer_ids::UNDEFINED_LAYER;

    for type_ in [
        DrcConstraintT::HoleClearanceConstraint,
        DrcConstraintT::HoleToHoleConstraint,
        DrcConstraintT::DiffPairGapConstraint,
    ] {
        let c = engine.eval_rules(ctx, type_, Some(a), Some(b), layer);
        cat.emit(&field(type_, layer), &constraint_text(&c));
    }
}

fn net_of(item: &ItemRef<'_>) -> i32 {
    item.net_code().map_or(0, |n| n.0)
}

/// Every category of one board (`DumpEvalRulesSection`), each run separately so that a
/// `port_pending!` stub only affects its own category. Err: the panic text.
fn dump_board(board: &Board, engine: &DrcEngine) -> BTreeMap<String, Result<Cat, String>> {
    let ctx = BoardCtx::bare(board).with_rules(engine);
    let w = walk(board, &ctx);
    let mut out = BTreeMap::new();
    let item = |id: ItemId| ctx.item(id).unwrap();

    let mut run = |name: &str, f: &mut dyn FnMut(&mut Cat)| {
        let mut cat = Cat::new();
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut cat)));
        let r = match r {
            Ok(()) => Ok(cat),
            Err(e) => Err(panic_text(&*e)),
        };

        if !matches!(&r, Ok(c) if c.keys == 0) {
            out.insert(name.to_string(), r);
        }
    };

    for name in ["track", "arc", "via"] {
        run(name, &mut |cat| {
            for (c, id) in &w.tracks {
                if *c == name {
                    unary(engine, &ctx, item(*id), cat);
                }
            }
        });
    }

    run("pad", &mut |cat| {
        for id in &w.pads {
            unary(engine, &ctx, item(*id), cat);
        }
    });
    run("zone", &mut |cat| {
        for id in &w.zones {
            unary(engine, &ctx, item(*id), cat);
        }
    });
    run("fpzone", &mut |cat| {
        for id in &w.fp_zones {
            unary(engine, &ctx, item(*id), cat);
        }
    });
    run("drawing", &mut |cat| {
        for id in &w.drawings {
            unary(engine, &ctx, item(*id), cat);
        }
    });
    run("disallow", &mut |cat| {
        let all = w
            .tracks
            .iter()
            .map(|(_, id)| id)
            .chain(&w.pads)
            .chain(&w.zones)
            .chain(&w.fp_zones)
            .chain(&w.drawings);

        for id in all {
            disallow(engine, &ctx, item(*id), cat);
        }
    });

    let tracks: Vec<ItemRef<'_>> = w.tracks.iter().map(|(_, id)| item(*id)).collect();
    let pads: Vec<ItemRef<'_>> = w.pads.iter().map(|id| item(*id)).collect();
    let zones: Vec<ItemRef<'_>> = w.zones.iter().map(|id| item(*id)).collect();

    let next_of_other_nets = |list: &[ItemRef<'_>], cat: &mut Cat| {
        for i in 0..list.len() {
            let mut found = 0;
            let mut j = i + 1;

            while j < list.len() && found < K_PAIRS {
                if net_of(&list[j]) != net_of(&list[i]) {
                    pair(engine, &ctx, list[i], list[j], false, cat);
                    found += 1;
                }

                j += 1;
            }
        }
    };

    run("pair_tt", &mut |cat| next_of_other_nets(&tracks, cat));
    run("pair_pp", &mut |cat| next_of_other_nets(&pads, cat));
    run("pair_tp", &mut |cat| {
        if pads.is_empty() {
            return;
        }

        for (i, track) in tracks.iter().enumerate() {
            let mut found = 0;
            let mut m = 0;

            while m < 3 * K_PAIRS && found < K_PAIRS {
                let pad = pads[(7 * i + m) % pads.len()];

                if net_of(&pad) != net_of(track) {
                    pair(engine, &ctx, *track, pad, false, cat);
                    found += 1;
                }

                m += 1;
            }
        }
    });
    run("pair_zt", &mut |cat| {
        for zone in &zones {
            let mut found = 0;

            for track in &tracks {
                if found >= K_PAIRS {
                    break;
                }

                if net_of(track) != net_of(zone) {
                    pair(engine, &ctx, *zone, *track, false, cat);
                    found += 1;
                }
            }
        }
    });
    run("pair_zp", &mut |cat| {
        for zone in &zones {
            for pad in pads.iter().take(K_PAIRS) {
                pair(engine, &ctx, *pad, *zone, true, cat);
            }
        }
    });
    run("pair_zz", &mut |cat| {
        for z in 0..zones.len().saturating_sub(1) {
            pair(engine, &ctx, zones[z], zones[z + 1], false, cat);
        }
    });

    out
}

/// Silences the default panic output while `port_pending!` stubs are caught.
fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = f();
    std::panic::set_hook(hook);
    r
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// True when the engine holds a user rule whose condition or assertion calls a function of
/// `pcbexpr_functions.cpp` (`A.name( ... )`): the function bodies are package DRC-22 (stubs
/// returning 0 until then), so such boards are only counted. Conditions without function
/// calls are evaluated by the expression binding (DRC-21) and compared.
fn has_compiled_conditions(engine: &DrcEngine) -> bool {
    let calls_function = |expr: &str| {
        let bytes = expr.as_bytes();

        // a '(' that directly follows an identifier which follows a '.'
        (0..bytes.len()).any(|i| {
            if bytes[i] != b'(' {
                return false;
            }

            let mut j = i;

            while j > 0 && (bytes[j - 1] == b' ' || bytes[j - 1] == b'\t') {
                j -= 1;
            }

            let end = j;

            while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_') {
                j -= 1;
            }

            j < end && j > 0 && bytes[j - 1] == b'.'
        })
    };

    engine.constraint_map().values().flatten().any(|c| {
        !c.parent_rule.is_implicit()
            && (matches!(&c.condition, Some(EngineCondition::Compiled(cond))
                    if calls_function(cond.expression()))
                || c.test
                    .as_ref()
                    .is_some_and(|t| calls_function(t.expression())))
    })
}

/// The outcome of replaying one compact golden (`gen_evalrules_goldens.py`).
#[derive(Default)]
struct Replay {
    /// "board: category" of every category whose key count, line count and hash match.
    matched: Vec<String>,
    /// Boards whose user rule conditions need the expression binding (not compared).
    needs_drc21: Vec<String>,
    /// Categories (or boards) that cannot be compared yet, with the reason.
    pending: Vec<String>,
    /// Categories that differ from the C++ dump.
    mismatched: Vec<String>,
    /// Boards whose categories all match.
    matched_boards: usize,
}

/// Replays a compact golden: every board is loaded on the QA path and each category is
/// compared with the C++ numbers.
fn replay_golden(golden: &str) -> Replay {
    let root = kicad_qa::kicad_source_root();

    // board -> category -> (keys, lines, hash)
    let mut boards: BTreeMap<&str, BTreeMap<&str, (usize, usize, String)>> = BTreeMap::new();
    let mut errors: BTreeMap<&str, i32> = BTreeMap::new();

    for line in golden.lines() {
        let fields: Vec<&str> = line.split('|').collect();

        if fields.get(1) == Some(&"load_error") {
            continue;
        }

        let get = |key: &str| {
            fields
                .iter()
                .find_map(|f| f.strip_prefix(&format!("{key}=")))
                .unwrap()
        };

        if fields[1] == "board" {
            errors.insert(fields[0], get("errors").parse().unwrap());
            boards.entry(fields[0]).or_default();
            continue;
        }

        boards.entry(fields[0]).or_default().insert(
            fields[1],
            (
                get("n").parse().unwrap(),
                get("lines").parse().unwrap(),
                get("hash").to_string(),
            ),
        );
    }

    let (mut matched, mut needs_drc21, mut pending, mut mismatched) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut matched_boards = 0;
    let raw_dir = std::env::var_os("EVALRULES_RAW_DIR").map(std::path::PathBuf::from);

    for (rel, cats) in &boards {
        if errors[rel] != 0 {
            pending.push(format!("{rel}: error lines in the C++ dump"));
            continue;
        }

        let path = root.join(rel);
        let loaded = quiet(|| catch_unwind(AssertUnwindSafe(|| load_board_qa(&path))));

        let (board, report, engine, drc_error) = match loaded {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                pending.push(format!("{rel}: {}", panic_text(&*e)));
                continue;
            }
        };

        if drc_error {
            pending.push(format!("{rel}: InitEngine failed"));
            continue;
        }

        // Since DRC-22 these boards are compared like the others; they are only counted.
        if has_compiled_conditions(&engine) {
            needs_drc21.push(rel.to_string());
        }

        // The board-level caches of the expression functions live as long as the C++ BOARD
        // members do during the dump (filled by the evaluations, never cleared in between).
        let _caches = kicad_drc::pcbexpr::functions::PcbExprCacheScope::enter(&board);
        let rust = quiet(|| dump_board(&board, &engine));
        let mut all_matched = true;

        if let Some(dir) = &raw_dir {
            let mut text = String::new();

            for (cat, r) in &rust {
                for line in r
                    .as_ref()
                    .ok()
                    .and_then(|c| c.raw.as_ref())
                    .into_iter()
                    .flatten()
                {
                    text += &format!("{cat}|{line}\n");
                }
            }

            let name = Path::new(rel).file_stem().unwrap().to_string_lossy();
            std::fs::write(dir.join(format!("{name}.rust.txt")), text).unwrap();
        }

        for (cat, (n, lines, hash)) in cats {
            match rust.get(*cat) {
                None => {
                    all_matched = false;
                    pending.push(format!("{rel}: {cat}: no keys (C++ {n})"));
                }
                Some(Err(e)) => {
                    all_matched = false;
                    pending.push(format!("{rel}: {cat}: {e}"));
                }
                Some(Ok(c)) if c.keys != *n => {
                    all_matched = false;
                    pending.push(format!("{rel}: {cat}: {} keys (C++ {n})", c.keys));
                }
                Some(Ok(c)) => {
                    let h = format!("{:016x}", c.hash);

                    if c.lines == *lines && h == *hash {
                        matched.push(format!("{rel}: {cat}"));
                    } else {
                        all_matched = false;
                        let msg = format!(
                            "{rel}: {cat}: lines {} hash {h} (C++ {lines} {hash})",
                            c.lines
                        );

                        if report.skipped.is_empty() {
                            mismatched.push(msg);
                        } else {
                            pending.push(format!("{msg}; parser skipped items"));
                        }
                    }
                }
            }
        }

        for cat in rust.keys() {
            if !cats.contains_key(cat.as_str()) {
                all_matched = false;
                mismatched.push(format!("{rel}: {cat}: not in the C++ dump"));
            }
        }

        if all_matched {
            matched_boards += 1;
        }
    }

    Replay {
        matched,
        needs_drc21,
        pending,
        mismatched,
        matched_boards,
    }
}

#[test]
fn evalrules_corpus_matches_cxx() {
    let Replay {
        matched,
        needs_drc21,
        pending,
        mismatched,
        matched_boards,
    } = replay_golden(include_str!("golden/evalrules_corpus.txt"));

    eprintln!(
        "evalrules corpus: {} categories match ({} boards completely), {} pending, {} mismatch; \
         {} boards call expression functions (compared since DRC-22)",
        matched.len(),
        matched_boards,
        pending.len(),
        mismatched.len(),
        needs_drc21.len()
    );

    let mut per_category: BTreeMap<&str, usize> = BTreeMap::new();

    for m in &matched {
        *per_category
            .entry(m.rsplit(": ").next().unwrap())
            .or_default() += 1;
    }

    eprintln!("matching categories: {per_category:?}");

    for p in &pending {
        eprintln!("pending: {p}");
    }

    for b in &needs_drc21 {
        eprintln!("calls functions: {b}");
    }

    // The golden is dumped on the QA load path, whose ENUM_MAP<PCB_LAYER_ID> holds only the
    // canonical layer names; the port matches layer patterns against the kicad-cli map
    // (canonical and board names, DRC-21 deviation). issue24735's rule "A.Layer ==
    // 'F.Silkscreen'" matches the board name only: `pcb-dump board --loader` gives the Rust
    // values for this category.
    const LAYER_MAP_DEVIATIONS: &[&str] =
        &["qa/data/pcbnew/issue24735/issue24735.kicad_pcb: drawing:"];
    let (known, mismatched): (Vec<String>, Vec<String>) = mismatched
        .into_iter()
        .partition(|m| LAYER_MAP_DEVIATIONS.iter().any(|d| m.starts_with(d)));

    for m in &known {
        eprintln!("known deviation (layer map of the QA load path): {m}");
    }

    assert!(
        mismatched.is_empty(),
        "mismatches:\n{}",
        mismatched.join("\n")
    );
    assert!(!matched.is_empty());
}

/// The REV-DRC review boards (`tests/data/review`, generated by
/// `tests/oracle/gen_review_boards.py`): four small pad-less boards whose rule files hold only
/// unconditional user rules, so that the explicit-rule half of `EvalRules` (layer clauses,
/// per-field merging of several rules, severities, the `MAXIMUM_CLEARANCE` clamp, unconditional
/// disallow masks and layers, the hole-to-hole operand test, explicit rules against local
/// zone clearances and the board minimum) and the implicit netclass rules of several classes
/// (composite classes, diff-pair clearance overrides, the fast path) are compared with the
/// C++ engine without the expression binding. Unlike the corpus replay every category must
/// match: nothing on these boards is pending.
#[test]
fn evalrules_review_boards_match_cxx() {
    let replay = replay_golden(include_str!("golden/evalrules_review.txt"));

    assert!(
        replay.mismatched.is_empty(),
        "mismatches:\n{}",
        replay.mismatched.join("\n")
    );
    assert!(
        replay.pending.is_empty(),
        "pending:\n{}",
        replay.pending.join("\n")
    );
    assert!(replay.needs_drc21.is_empty(), "{:?}", replay.needs_drc21);
    assert_eq!(replay.matched_boards, 4);
}

// ---- unit tests ------------------------------------------------------------------------------

/// Port of `FootprintLevelSolderMaskExpansion` (`qa/tests/pcbnew/drc/
/// test_drc_solder_mask_expansion.cpp`): footprint-level solder mask expansion override is
/// applied to pads without their own local override.
/// Regression test for https://gitlab.com/kicad/code/kicad/-/issues/22751
#[test]
fn footprint_level_solder_mask_expansion() {
    let path = kicad_qa::pcbnew_data_dir().join("issue22751/issue22751.kicad_pcb");
    let (mut board, _, _, _) = load_board_qa(&path).unwrap();
    let mut engine = DrcEngine::new();
    engine
        .init_engine(
            &mut board,
            None,
            default_layer_choices(),
            &TuningProfiles::default(),
            None,
        )
        .unwrap();

    // Board default solder mask expansion is 0.05mm (50000 IU)
    assert_eq!(board.design_settings().solder_mask_expansion, 50000);

    if board
        .footprints()
        .all(|(fp, _)| board.footprint_pads(fp).next().is_none())
    {
        eprintln!("pending: the board parser does not load pads yet (PCBIO-07)");
        return;
    }

    let ctx = BoardCtx::bare(&board).with_rules(&engine);
    let (mut pad_with_fp_margin, mut pad_with_no_margin, mut pad_with_own_margin) =
        (None, None, None);

    for (fp_id, fp) in board.footprints() {
        let fpid = fp.fpid_as_string();
        let pad1 = board
            .footprint_pads(fp_id)
            .find(|(_, pad)| pad.number() == "1")
            .map(|(id, pad)| (id, pad.clone()));

        if fpid == "TestFootprint_WithSolderMaskMargin" {
            assert_eq!(fp.solder_mask_margin, Some(100000));
            let (id, pad) = pad1.unwrap();
            assert!(pad.local_solder_mask_margin().is_none());
            pad_with_fp_margin = Some(id);
        } else if fpid == "TestFootprint_NoSolderMaskMargin" {
            assert!(fp.solder_mask_margin.is_none());
            pad_with_no_margin = pad1.map(|(id, _)| id);
        } else if fpid == "TestFootprint_PadOverridesSolderMaskMargin" {
            assert_eq!(fp.solder_mask_margin, Some(100000));
            let (id, pad) = pad1.unwrap();
            assert_eq!(pad.local_solder_mask_margin(), Some(200000));
            pad_with_own_margin = Some(id);
        }
    }

    let expansion = |id: Option<kicad_pcb::ids::PadId>| {
        let item = ctx.item(id.unwrap().item_id()).unwrap();
        item.as_pad()
            .unwrap()
            .solder_mask_expansion(&ctx, PcbLayerId::FMask)
    };

    // Pad in footprint with solder mask margin should use footprint's margin
    assert_eq!(expansion(pad_with_fp_margin), 100000);
    // Pad in footprint without solder mask margin should use board default
    assert_eq!(expansion(pad_with_no_margin), 50000);
    // Pad with its own solder mask margin overrides the footprint's margin
    assert_eq!(expansion(pad_with_own_margin), 200000);
}

/// The netclass fast path of `EvalRules` (no reporter) gives the same result as the general
/// path (taken whenever a reporter is passed) for every sampled item pair and every item
/// alone, as long as there are no explicit clearance rules (R5 of the DRC analysis).
#[test]
fn fast_path_equals_general_path() {
    for name in [
        "demos/pic_programmer/pic_programmer",
        "demos/ecc83/ecc83-pp",
        "qa/data/pcbnew/issue4139",
    ] {
        let path = kicad_qa::kicad_source_root().join(format!("{name}.kicad_pcb"));

        if !path.exists() {
            continue;
        }

        let (board, _, engine, _) = load_board_qa(&path).unwrap();
        assert!(!engine.has_explicit_clearance_rules());
        let ctx = BoardCtx::bare(&board).with_rules(&engine);
        let w = walk(&board, &ctx);
        let items: Vec<ItemRef<'_>> = w
            .tracks
            .iter()
            .map(|(_, id)| *id)
            .chain(w.pads.iter().copied())
            .chain(w.zones.iter().copied())
            .map(|id| ctx.item(id).unwrap())
            .collect();
        let mut compared = 0;

        for (i, a) in items.iter().enumerate() {
            let partners = [
                None,
                items.get(i + 1).copied(),
                items.get(i * 7 % items.len()).copied(),
            ];

            for b in partners {
                for layer in copper_seq(&board, a.layer_set()) {
                    let fast = engine.eval_rules(
                        &ctx,
                        DrcConstraintT::ClearanceConstraint,
                        Some(*a),
                        b,
                        layer,
                    );
                    let mut reporter = kicad_drc::reporter::WxStringReporter::new();
                    let general = engine.eval_rules_reporting(
                        &ctx,
                        DrcConstraintT::ClearanceConstraint,
                        Some(*a),
                        b,
                        layer,
                        Some(&mut reporter),
                    );
                    assert_eq!(
                        constraint_text(&fast),
                        constraint_text(&general),
                        "{name}: {a:?} / {b:?} on {layer:?}"
                    );
                    // the same rule identity (FIX-DRC-1)
                    let identity = |c: &DrcConstraint| {
                        [&c.parent_rule, &c.min_rule, &c.opt_rule, &c.max_rule]
                            .map(|r| r.as_ref().and_then(|r| r.rule_index))
                    };
                    assert_eq!(
                        identity(&fast),
                        identity(&general),
                        "{name}: {a:?} / {b:?} on {layer:?}"
                    );
                    compared += 1;
                }
            }
        }

        assert!(compared > 0);
    }
}

/// Later rules override earlier ones per field (min / opt / max separately), and the board
/// minimum is applied to a clearance only when both items are copper.
#[test]
fn last_rule_wins_per_field_and_board_minimum() {
    let path = kicad_qa::kicad_source_root().join("demos/pic_programmer/pic_programmer.kicad_pcb");
    let (mut board, _, _, _) = load_board_qa(&path).unwrap();
    let rules = "(version 1)\n\
                 (rule a (constraint track_width (min 0.1mm) (max 2mm)) (constraint clearance (min 0.3mm)))\n\
                 (rule b (constraint track_width (opt 0.5mm)) (constraint clearance (min 0.05mm)))\n";
    let mut engine = DrcEngine::new();
    engine
        .init_engine_with::<SyntaxBinder>(
            &mut board,
            Some((rules, "test.kicad_dru")),
            default_layer_choices(),
            &TuningProfiles::default(),
            None,
        )
        .unwrap();
    board.design_settings_mut().min_clearance = 70000;

    let ctx = BoardCtx::bare(&board).with_rules(&engine);
    let (track_id, _) = board.tracks().find(|(_, t)| t.as_via().is_none()).unwrap();
    let track = ctx.item(track_id.item_id()).unwrap();

    let width = engine.eval_rules(
        &ctx,
        DrcConstraintT::TrackWidthConstraint,
        Some(track),
        None,
        track.layer(),
    );
    assert_eq!(width.value.min(), 100000);
    assert_eq!(width.value.opt(), 500000);
    assert_eq!(width.value.max(), 2000000);
    assert_eq!(width.name(), "rule 'b'");
    assert_eq!(width.rule_name(width.min_rule.as_ref()), "rule 'a'");
    assert_eq!(width.rule_name(width.opt_rule.as_ref()), "rule 'b'");
    assert!(!width.implicit_min);

    // rule b wins (0.05mm), explicit -> returned before the board minimum
    let clearance = engine.eval_rules(
        &ctx,
        DrcConstraintT::ClearanceConstraint,
        Some(track),
        None,
        track.layer(),
    );
    assert_eq!(clearance.value.min(), 50000);
    assert_eq!(clearance.name(), "rule 'b'");

    // the cached own clearance is the same value, and is dropped when the board changes
    assert_eq!(
        engine.cached_own_clearance(&ctx, track, track.layer(), true),
        (50000, Some("rule 'b'".to_string()))
    );
    assert_eq!(
        engine.cached_own_clearance(&ctx, track, track.layer(), true),
        (50000, None)
    );
}

/// FIX-DRC-1: the rule identity behind `DRC_CONSTRAINT::GetParentRule()` and friends. The two
/// rules `"dup name"` of `rev_explicit.kicad_dru` (the reference loads both) have the same
/// name and severity but are different rules: for a via on F.Cu the minimum comes from the
/// second, the maximum from the first.
#[test]
fn same_named_rules_have_distinct_identity() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/review/rev_explicit.kicad_pcb");
    let (board, _, engine, failed) = load_board_qa(&path).unwrap();
    assert!(!failed);

    let ctx = BoardCtx::bare(&board).with_rules(&engine);
    let (via_id, _) = board.tracks().find(|(_, t)| t.as_via().is_some()).unwrap();
    let via = ctx.item(via_id.item_id()).unwrap();

    let c = engine.eval_rules(
        &ctx,
        DrcConstraintT::ViaDiameterConstraint,
        Some(via),
        None,
        PcbLayerId::FCu,
    );
    assert_eq!(c.value.min(), 450000);
    assert_eq!(c.value.max(), 700000);

    let min_rule = c.min_rule.as_ref().unwrap();
    let max_rule = c.max_rule.as_ref().unwrap();
    assert_eq!(min_rule.name, "dup name");
    assert_eq!(max_rule.name, "dup name");
    assert_eq!(min_rule.severity, max_rule.severity);

    let (min_index, max_index) = (min_rule.rule_index.unwrap(), max_rule.rule_index.unwrap());
    assert!(min_index > max_index, "{min_index} / {max_index}");
    assert_ne!(min_rule, max_rule);
    // the parent rule is the last rule applied: the second one
    assert_eq!(c.parent_rule.as_ref().unwrap().rule_index, Some(min_index));

    // the index reaches the engine's rule
    for (index, layer_source) in [(min_index, "F.Cu"), (max_index, "")] {
        let rule = engine.rule(index).unwrap();
        assert_eq!(rule.name, "dup name");
        assert_eq!(rule.layer_source, layer_source);
    }
    assert!(engine.rule(engine.rules().len() as u32).is_none());

    // every compiled constraint carries the index of its rule
    for list in engine.constraint_map().values() {
        for ec in list {
            let index = ec.rule_info.as_ref().unwrap().rule_index.unwrap();
            assert!(std::sync::Arc::ptr_eq(
                engine.rule(index).unwrap(),
                &ec.parent_rule
            ));
            assert_eq!(
                ec.constraint.parent_rule.as_ref().unwrap().rule_index,
                Some(index)
            );
        }
    }
}
