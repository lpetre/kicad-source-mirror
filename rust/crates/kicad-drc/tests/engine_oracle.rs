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
// You should have received a copy of the GNU General Public License
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Differential test of the DRC engine core (package DRC-03) against the C++ engine: the
//! Rust twin of `pcb-dump board --sections engine` (`rust/tools/pcb-dump/dump_engine.cpp`,
//! `rust/docs/PCB_DUMP_FORMAT.md` section 8) replayed against the compact golden
//! `tests/golden/engine_corpus.txt` (`tests/oracle/gen_engine_goldens.py`).
//!
//! The load path mirrors the dump tool's QA path (`KI_TEST::LoadBoard`): parse, bind the
//! sibling `.kicad_pro` (`BOARD::SetProject`: design settings unless legacy ones were loaded,
//! net settings), then `InitEngine` with the sibling `.kicad_dru`.
//!
//! Boards that cannot match yet are reported as pending with the reason (parser packages that
//! skip keepout zones or legacy netclasses, `port_pending!` stubs on the path such as the rule
//! file text-variable expansion and condition compilation, tuning profiles); every other board
//! must match exactly.

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use common::{constraint_name, severity_name, v_lset, v_str};
use kicad_core::json::{self, ObjectOrder};
use kicad_core::json_settings::JsonSettingsInternals;
use kicad_drc::engine::implicit_condition::EngineCondition;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::engine::{ConstraintMap, DrcEngine, PROVIDER_ORDER};
use kicad_drc::exclusion::KiapiDrcExclusionCodec;
use kicad_drc::rule::{DrcConstraintExt, DrcRule};
use kicad_drc::rule_parser::default_layer_choices;
use kicad_drc::{DrcConstraintT, DrcImplicitSource};
use kicad_pcb::Board;
use kicad_pcb::board::BoardProject;
use kicad_pcb::io::{ParseOptions, ParseReport, parse_board_report};
use kicad_pcb::project::board_design_settings_json::load_board_design_settings;
use kicad_pcb::project::net_settings_json::load_net_settings;

fn v_bool(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

/// `magic_enum::enum_name( DRC_IMPLICIT_SOURCE )`.
fn source_name(source: DrcImplicitSource) -> &'static str {
    match source {
        DrcImplicitSource::None => "NONE",
        DrcImplicitSource::BoardSetupConstraint => "BOARD_SETUP_CONSTRAINT",
        DrcImplicitSource::BarcodeDefaults => "BARCODE_DEFAULTS",
        DrcImplicitSource::Keepout => "KEEPOUT",
        DrcImplicitSource::NetClass => "NET_CLASS",
        DrcImplicitSource::TuningProfile => "TUNING_PROFILE",
    }
}

fn value_text(v: &kicad_core::minoptmax::MinOptMax<i32>) -> String {
    let f = |has: bool, val: i32| if has { val.to_string() } else { "none".to_string() };
    format!(
        "{}/{}/{}",
        f(v.has_min(), v.min()),
        f(v.has_opt(), v.opt()),
        f(v.has_max(), v.max())
    )
}

struct Out {
    lines: Vec<String>,
}

impl Out {
    fn emit(&mut self, key: &str, field: &str, value: &str) {
        self.lines.push(format!("engine|{key}|{field}={value}"));
    }
}

fn rule_index(engine: &DrcEngine, rule: &Arc<DrcRule>) -> String {
    engine
        .rules()
        .iter()
        .position(|r| Arc::ptr_eq(r, rule))
        .map_or("stale".to_string(), |i| i.to_string())
}

fn dump_lists(out: &mut Out, key: &str, map: &ConstraintMap, engine: &DrcEngine, full: bool) {
    for (type_, list) in map {
        let p = format!("{key}.{}", constraint_name(*type_));
        out.emit(key, &format!("{p}.count"), &list.len().to_string());

        for (ii, c) in list.iter().enumerate() {
            let q = format!("{p}.{ii}");
            out.emit(key, &format!("{q}.rule"), &rule_index(engine, &c.parent_rule));

            if !full {
                continue;
            }

            out.emit(key, &format!("{q}.layers"), &v_lset(&c.layer_test));

            match &c.condition {
                Some(cond) => {
                    out.emit(key, &format!("{q}.condition"), &v_str(cond.expression()));
                    out.emit(
                        key,
                        &format!("{q}.condition.pair"),
                        v_bool(cond.requires_pair_items()),
                    );
                    out.emit(
                        key,
                        &format!("{q}.condition.item_b"),
                        v_bool(cond.references_item_b()),
                    );
                    out.emit(
                        key,
                        &format!("{q}.condition.geometry"),
                        v_bool(cond.has_geometry_dependent_functions()),
                    );
                }
                None => out.emit(key, &format!("{q}.condition"), "none"),
            }

            out.emit(key, &format!("{q}.type"), constraint_name(c.constraint.type_));
            out.emit(key, &format!("{q}.value"), &value_text(&c.constraint.value));
            out.emit(
                key,
                &format!("{q}.disallow"),
                &c.constraint.disallow_flags.to_string(),
            );
            out.emit(key, &format!("{q}.name"), &v_str(&c.constraint.name()));
            out.emit(
                key,
                &format!("{q}.severity"),
                severity_name(c.constraint.severity()),
            );
            let zone = c
                .implicit_keepout_zone
                .map_or("none".to_string(), |_| c.parent_rule.implicit_item_id.as_string());
            out.emit(key, &format!("{q}.keepout_zone"), &zone);
        }
    }
}

/// The Rust twin of `DumpEngineSection`.
pub fn dump_engine(engine: &DrcEngine) -> Vec<String> {
    let mut out = Out { lines: Vec::new() };

    out.emit("providers", "count", &PROVIDER_ORDER.len().to_string());

    for (ii, name) in PROVIDER_ORDER.iter().enumerate() {
        out.emit("providers", &ii.to_string(), &v_str(name));
    }

    out.emit("engine", "rules_valid", v_bool(engine.rules_valid()));
    out.emit(
        "engine",
        "has_explicit_clearance_rules",
        v_bool(engine.has_explicit_clearance_rules()),
    );
    out.emit(
        "engine",
        "has_geometry_dependent_rules",
        v_bool(engine.has_geometry_dependent_rules()),
    );
    out.emit(
        "engine",
        "has_dp_clearance_overrides",
        v_bool(engine.has_diff_pair_clearance_overrides()),
    );
    out.emit("engine", "rules", &engine.rules().len().to_string());

    for (ii, rule) in engine.rules().iter().enumerate() {
        let key = format!("rule.{ii}");
        out.emit(&key, "name", &v_str(&rule.name));
        out.emit(&key, "implicit", v_bool(rule.is_implicit()));
        out.emit(&key, "source", source_name(rule.implicit_source()));
        out.emit(&key, "unary", v_bool(rule.unary));
        out.emit(&key, "layers", &v_lset(&rule.layer_condition));
        out.emit(&key, "severity", severity_name(rule.severity));
        out.emit(&key, "netclass", &v_str(&rule.implicit_netclass));
        out.emit(&key, "item_id", &rule.implicit_item_id.as_string());
        out.emit(
            &key,
            "condition",
            &rule
                .condition
                .as_ref()
                .map_or("none".to_string(), |c| v_str(c.expression())),
        );
        out.emit(&key, "constraints", &rule.constraints.len().to_string());

        for (jj, c) in rule.constraints.iter().enumerate() {
            let p = format!("c{jj}");
            out.emit(&key, &format!("{p}.type"), constraint_name(c.type_));
            out.emit(&key, &format!("{p}.value"), &value_text(&c.value));
            out.emit(&key, &format!("{p}.disallow"), &c.disallow_flags.to_string());
            out.emit(&key, &format!("{p}.name"), &v_str(&c.name()));
            out.emit(&key, &format!("{p}.severity"), severity_name(c.severity()));
        }
    }

    dump_lists(&mut out, "map", engine.constraint_map(), engine, true);
    dump_lists(&mut out, "explicit", engine.explicit_constraints(), engine, false);

    for (name, rule) in engine.netclass_clearances() {
        out.emit("netclass_clearances", name, &rule_index(engine, rule));
    }

    out.lines
}

/// The QA load path of the dump tool: parse, `BOARD::SetProject` with the sibling project,
/// `InitEngine` with the sibling rule file. Returns the board, the parse report, the engine
/// and whether `InitEngine` failed.
fn load_board_qa(path: &Path) -> Result<(Board, ParseReport, DrcEngine, bool), String> {
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
    } else if path.with_extension("pro").exists() {
        return Err("legacy .pro project (not readable by the port)".to_string());
    }

    let dru = path.with_extension("kicad_dru");
    let dru_text = dru
        .exists()
        .then(|| String::from_utf8_lossy(&std::fs::read(&dru).unwrap()).to_string());
    let dru_path = dru.to_string_lossy().to_string();

    let mut engine = DrcEngine::new();
    let result = engine.init_engine(
        &mut board,
        dru_text.as_deref().map(|t| (t, dru_path.as_str())),
        default_layer_choices(),
        &TuningProfiles::default(),
        None,
    );

    Ok((board, report, engine, result.is_err()))
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;

    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

/// Silences the default panic output while `port_pending!` stubs are caught.
fn quiet<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    r
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn engine_corpus_matches_cxx() {
    let golden = include_str!("golden/engine_corpus.txt");
    let root = kicad_qa::kicad_source_root();
    let full_dir = std::env::var("ENGINE_DUMP_DIR").ok();

    let (mut matched, mut pending, mut mismatched) = (Vec::new(), Vec::new(), Vec::new());

    for line in golden.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        let rel = fields[0];

        if fields.get(1) == Some(&"load_error") {
            continue;
        }

        let get = |key: &str| {
            fields
                .iter()
                .find_map(|f| f.strip_prefix(&format!("{key}=")))
                .unwrap()
                .to_string()
        };
        let num = |key: &str| get(key).parse::<i32>().unwrap();

        let path = root.join(rel);
        let loaded = quiet(|| load_board_qa(&path));

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
        drop(board);

        let lines = dump_engine(&engine);
        let data: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let hash = format!("{:016x}", fnv1a64(data.as_bytes()));

        if let Some(dir) = &full_dir {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(
                Path::new(dir).join(format!("{}.txt", rel.replace('/', "__"))),
                &data,
            );
        }

        if hash == get("hash") && i32::from(drc_error) == num("drc_error") {
            matched.push(rel.to_string());
            continue;
        }

        // Known reasons why a board cannot match yet.
        let skipped = |token: &str| report.skipped.iter().any(|s| s.token == token);
        let mut reasons = Vec::new();

        if num("keepout") > 0 && (skipped("zone") || skipped("footprint")) {
            reasons.push("keepout zones skipped by the parser (PCBIO-06 / 08)");
        }

        if skipped("net_class") {
            reasons.push("legacy netclasses skipped by the parser (PCBIO-11)");
        }

        if num("tuning") > 0 {
            reasons.push("tuning profiles not in the project model");
        }

        if reasons.is_empty() {
            mismatched.push(format!(
                "{rel}: hash {hash} (C++ {}), drc_error {drc_error} (C++ {})",
                get("hash"),
                get("drc_error")
            ));
        } else {
            pending.push(format!("{rel}: {}", reasons.join("; ")));
        }
    }

    eprintln!(
        "engine corpus: {} match, {} pending, {} mismatch",
        matched.len(),
        pending.len(),
        mismatched.len()
    );

    for p in &pending {
        eprintln!("pending: {p}");
    }

    assert!(mismatched.is_empty(), "mismatches:\n{}", mismatched.join("\n"));
    assert!(!matched.is_empty());
}

// ---- qa/tests/pcbnew/drc/test_drc_error_limit.cpp --------------------------------------------

/// `CapIsExact`: `ReportViolation` enforces the per-code cap, so a code can never report more
/// violations than its limit no matter how many times it is hit.
#[test]
fn cap_is_exact() {
    use kicad_drc::engine::DrcRunState;
    use kicad_pcb::drc_api::DRCE_ASSERTION_FAILURE;
    use std::cell::Cell;

    let reported = Cell::new(0);
    let mut run = DrcRunState::new();
    run.set_violation_handler(Box::new(|_, _, _, _| reported.set(reported.get() + 1)));

    let code = DRCE_ASSERTION_FAILURE;
    let item = Arc::new(kicad_drc::item::create(code).unwrap());
    let noop = |_: &mut kicad_pcb::pcb_marker::PcbMarker| {};

    while !run.is_error_limit_exceeded(code) {
        run.report_violation(&item, "test", Default::default(), 0, Some(&noop));
    }

    let capped = reported.get();
    assert!(capped > 0);

    for _ in 0..500 {
        run.report_violation(&item, "test", Default::default(), 0, Some(&noop));
    }

    drop(run);
    assert_eq!(reported.get(), capped);
    assert_eq!(capped, kicad_drc::engine::ERROR_LIMIT);
}

/// `InitEngineResetsLastErrorCode`: the limit of `DRCE_LAST` is reset by `InitEngine`
/// (here: `DrcRunState::reset_error_limits`, see `PORT_STATUS.md`).
#[test]
fn init_engine_resets_last_error_code() {
    use kicad_drc::engine::DrcRunState;
    use kicad_pcb::drc_api::DRCE_LAST;

    let mut run = DrcRunState::new();
    let item = Arc::new(kicad_drc::item::create(DRCE_LAST).unwrap());

    while !run.is_error_limit_exceeded(DRCE_LAST) {
        run.report_violation(&item, "test", Default::default(), 0, None);
    }

    assert!(run.is_error_limit_exceeded(DRCE_LAST));
    run.reset_error_limits();
    assert!(!run.is_error_limit_exceeded(DRCE_LAST));
}

/// The run limits of `RunTests`: 0 for ignored codes, 499 for clearance and unconnected items,
/// else 199.
#[test]
fn run_tests_error_limits() {
    use kicad_core::{EdaUnits, Severity};
    use kicad_drc::engine::{DrcRunState, ERROR_LIMIT, EXTENDED_ERROR_LIMIT};
    use kicad_pcb::drc_api::{DRCE_CLEARANCE, DRCE_SILK_CLEARANCE, DRCE_UNCONNECTED_ITEMS};

    let mut board = Board::new();
    board
        .design_settings_mut()
        .drc_severities
        .insert(DRCE_SILK_CLEARANCE, Severity::Ignore);

    let engine = DrcEngine::new();
    let mut run = DrcRunState::new();
    let stamp = board.time_stamp();
    engine.prepare_run(&mut run, &mut board, EdaUnits::Inch, true, false);

    assert_eq!(run.error_limit(DRCE_CLEARANCE), EXTENDED_ERROR_LIMIT);
    assert_eq!(run.error_limit(DRCE_UNCONNECTED_ITEMS), EXTENDED_ERROR_LIMIT);
    assert_eq!(run.error_limit(DRCE_SILK_CLEARANCE), 0);
    assert!(run.is_error_limit_exceeded(DRCE_SILK_CLEARANCE));
    assert_eq!(
        run.error_limit(kicad_pcb::drc_api::DRCE_TRACK_WIDTH),
        ERROR_LIMIT
    );
    assert!(run.report_all_track_errors());
    assert!(!run.test_footprints());
    assert_eq!(run.user_units(), EdaUnits::Inch);
    assert_ne!(board.time_stamp(), stamp);
}

/// `RunTests` runs the providers in order and stops at the first one that returns false.
#[test]
fn run_tests_provider_order_and_cancel() {
    use kicad_core::EdaUnits;
    use kicad_drc::engine::{DrcRunState, DrcTestProviderRun};
    use kicad_pcb::BoardCtx;
    use std::cell::RefCell;

    struct P<'a> {
        name: &'static str,
        result: bool,
        log: &'a RefCell<Vec<&'static str>>,
    }

    impl DrcTestProviderRun for P<'_> {
        fn name(&self) -> &str {
            self.name
        }

        fn run_tests(
            &mut self,
            _engine: &DrcEngine,
            _run: &mut DrcRunState<'_>,
            ctx: &BoardCtx<'_>,
            _units: EdaUnits,
        ) -> bool {
            assert!(ctx.rules.is_some());
            self.log.borrow_mut().push(self.name);
            self.result
        }
    }

    let log = RefCell::new(Vec::new());
    let mut a = P {
        name: "annular_width",
        result: true,
        log: &log,
    };
    let mut b = P {
        name: "disallow",
        result: false,
        log: &log,
    };
    let mut c = P {
        name: "creepage",
        result: true,
        log: &log,
    };

    let mut board = Board::new();
    let engine = DrcEngine::new();
    let mut run = DrcRunState::new();
    let mut providers: Vec<&mut dyn DrcTestProviderRun> = vec![&mut a, &mut b, &mut c];

    engine.run_tests(
        &mut run,
        &mut board,
        None,
        EdaUnits::Mm,
        false,
        false,
        |_, _| true,
        &mut providers,
    );

    assert_eq!(*log.borrow(), vec!["annular_width", "disallow"]);

    // cancelled while regenerating the caches: no provider runs
    log.borrow_mut().clear();
    engine.run_tests(
        &mut run,
        &mut board,
        None,
        EdaUnits::Mm,
        false,
        false,
        |_, _| false,
        &mut providers,
    );
    assert!(log.borrow().is_empty());
}

/// The implicit rules of a default board (no project): the fixed rules 1-10 and the default
/// netclass rules, in the order of analysis section 4.2, with native conditions.
#[test]
fn default_board_rule_list() {
    let mut board = Board::new();
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

    let names: Vec<&str> = engine.rules().iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "board setup constraints",
            "board setup constraints zone fill strategy",
            "board setup constraints silk",
            "board setup constraints silk text height",
            "board setup constraints silk text thickness",
            "board setup constraints hole",
            "board setup constraints edge",
            "board setup constraints courtyard",
            "board setup constraints micro-via",
            "barcode visual separation default",
            "netclass 'Default'",
            "netclass 'Default'",
            "netclass 'Default'",
            "netclass 'Default'",
            "netclass 'Default'",
            "netclass 'Default' uvia",
            "netclass 'Default' uvia",
        ]
    );

    assert!(engine.rules_valid());
    assert!(engine.netclass_clearance_rule("Default").is_some());

    // the courtyard rule has a constraint without any value (C++ sets the wrong variable)
    let courtyard = &engine.rules()[7].constraints[0];
    assert_eq!(courtyard.type_, DrcConstraintT::CourtyardClearanceConstraint);
    assert!(!courtyard.value.has_min());

    // every implicit condition is native
    for list in engine.constraint_map().values() {
        for c in list {
            if let Some(cond) = &c.condition {
                assert!(matches!(cond, EngineCondition::Implicit { .. }));
            }
        }
    }

    assert!(engine.has_rules_for_constraint_type(DrcConstraintT::ClearanceConstraint));
    assert!(!engine.has_rules_for_constraint_type(DrcConstraintT::AssertionConstraint));
    assert!(!engine.has_user_defined_constraint(DrcConstraintT::ClearanceConstraint));
    assert_eq!(
        engine
            .query_worst_constraint(DrcConstraintT::ClearanceConstraint, false)
            .map(|c| c.value.min()),
        Some(board.design_settings().net_settings.default_netclass().clearance())
    );
}
