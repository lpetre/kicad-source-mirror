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

//! Package DRC-21: the PCB expression binding (`pcbnew/pcbexpr_evaluator.cpp`).
//!
//! * Differential oracle of the condition compiler: the rule parser goldens of the C++
//!   `pcb-dump rules` command (every `.kicad_dru` of the corpus, the broken files of DRC-20 and
//!   the codegen cases `tests/data/rules/drc21/`, golden `drc21_conditions.txt` written by
//!   `tests/oracle/gen_drc21_goldens.py`) replayed with the real binding [`PcbExprUcode`]:
//!   accept / reject, every codegen message with its text and offset, the usage flags.
//! * The board-dependent cases of `qa/tests/pcbnew/test_libeval_compiler.cpp` against real
//!   items of small boards. Expressions that call a function of `pcbexpr_functions.cpp` need
//!   package DRC-22 and are only compiled here.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use kicad_core::{KicadT, PcbLayerId};
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::pcbexpr::evaluator::{
    PcbExprBinder, PcbExprBuiltinFunctions, PcbExprContext, PcbExprUcode, PcbExprUnitResolver,
    PcbExprValue, pcbexpr_compiler,
};
use kicad_drc::rule_condition::DrcRuleCondition;
use kicad_geom::geometry::eda_angle::EdaAngle;
use kicad_libeval::{Context, Ucode, Value, VarType};
use kicad_pcb::AnyItemMut;
use kicad_pcb::board::BoardProject;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::netclass::Netclass;
use kicad_pcb::{Board, ItemRef};

type PcbValue<'e> = Value<'e, PcbExprUcode>;

const NULL_CONSTRAINT: i32 = DrcConstraintT::NullConstraint as i32;

/* --------------------------------------------------------------------------------------------
 * Condition compiler oracle
 */

fn read_golden(name: &str) -> BTreeMap<String, Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("golden file");
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = String::new();

    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }

        if let Some(file) = line.strip_prefix("file|") {
            current = file.trim_end_matches('|').to_string();
            map.entry(current.clone()).or_default();
        } else {
            map.get_mut(&current).unwrap().push(line.to_string());
        }
    }

    map
}

fn read_text(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).expect("rule file")).unwrap_or_default()
}

fn check(
    golden: &BTreeMap<String, Vec<String>>,
    mut make: impl FnMut(&str) -> (Vec<String>, Vec<String>),
) {
    let mut failures = Vec::new();

    for (file, expected) in golden {
        let (actual, full) = make(file);

        if &actual != expected {
            let diff: Vec<String> = expected
                .iter()
                .zip(actual.iter())
                .filter(|(e, a)| e != a)
                .take(5)
                .map(|(e, a)| format!("    C++  {e}\n    Rust {a}"))
                .collect();
            failures.push(format!(
                "{file}: {} / {} lines\n{}\n  full Rust dump: {} lines",
                expected.len(),
                actual.len(),
                diff.join("\n"),
                full.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} files differ:\n{}",
        failures.len(),
        golden.len(),
        failures.join("\n")
    );
}

/// Every `.kicad_dru` of `qa/` and `demos/` compiles with the real binding exactly as in C++
/// (golden of DRC-20, `rules_corpus.txt`).
#[test]
fn corpus_conditions_compile_as_cxx() {
    let root = kicad_qa::kicad_source_root();
    let golden = read_golden("rules_corpus.txt");

    assert!(golden.len() >= 70);

    check(&golden, |rel| {
        let path = root.join(rel);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let lines = common::dump_rules_with::<PcbExprUcode>(&read_text(&path), &name, false);
        (common::compact(&lines), lines)
    });
}

/// The deliberately broken rule files of DRC-20 (`rules_errors.txt`) with the real binding.
#[test]
fn broken_rule_files_compile_as_cxx() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules");
    let golden = read_golden("rules_errors.txt");

    check(&golden, |name| {
        let lines =
            common::dump_rules_with::<PcbExprUcode>(&read_text(&dir.join(name)), name, false);
        (common::summary(&lines), lines)
    });
}

/// The codegen cases of DRC-21 (`tests/data/rules/drc21/`): unknown items, properties,
/// navigation chains, receivers, type conflicts, functions, `null`, usage flags; every line of
/// the C++ dump.
#[test]
fn codegen_cases_compile_as_cxx() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/rules/drc21");
    let golden = read_golden("drc21_conditions.txt");

    assert!(golden.len() >= 5);

    let on_disk = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "kicad_dru")
        })
        .count();
    assert_eq!(
        on_disk,
        golden.len(),
        "regenerate with gen_drc21_goldens.py"
    );

    check(&golden, |name| {
        let lines =
            common::dump_rules_with::<PcbExprUcode>(&read_text(&dir.join(name)), name, false);
        (lines.clone(), lines)
    });
}

/// `RegisterAllFunctions`: 34 signatures, 10 geometry dependent, lookup by lower-case name.
#[test]
fn builtin_function_registry() {
    let registry = PcbExprBuiltinFunctions::instance();

    assert_eq!(registry.signatures().len(), 34);
    assert_eq!(registry.signatures()[2], "insideCourtyard('x') DEPRECATED");
    assert!(registry.get("insidecourtyard").is_some());
    assert!(registry.get("insideCourtyard").is_none());
    assert!(registry.is_geometry_dependent("enclosedbyarea"));
    assert!(!registry.is_geometry_dependent("ismicrovia"));
    assert!(registry.get("unknownfunction").is_none());
}

/* --------------------------------------------------------------------------------------------
 * qa/tests/pcbnew/test_libeval_compiler.cpp
 */

/// A board of the reference corpus format with the given items.
fn board_text(items: &str) -> String {
    format!(
        r#"(kicad_pcb
	(version 20260410)
	(generator "pcbnew")
	(generator_version "10.99")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers
		(0 "F.Cu" signal)
		(4 "In1.Cu" signal)
		(6 "In2.Cu" signal)
		(2 "B.Cu" signal)
		(13 "F.Paste" user)
		(15 "B.Paste" user)
		(5 "F.SilkS" user "F.Silkscreen")
		(7 "B.SilkS" user "B.Silkscreen")
		(1 "F.Mask" user)
		(3 "B.Mask" user)
		(25 "Edge.Cuts" user)
		(31 "F.CrtYd" user "F.Courtyard")
		(29 "B.CrtYd" user "B.Courtyard")
		(35 "F.Fab" user)
		(33 "B.Fab" user)
	)
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
	(net 1 "net1")
	(net 2 "net2")
{items}
	(embedded_fonts no)
)
"#
    )
}

fn load(items: &str) -> Board {
    parse_board(
        &board_text(items),
        "test.kicad_pcb",
        &ParseOptions {
            // Footprint pads are skipped while the pad parser (PCBIO-07) is pending; the pad
            // cases below run once it has landed.
            allow_pending: true,
        },
    )
    .expect("test board")
}

/// The item with `uuid`.
fn item<'a>(ctx: &'a BoardCtx<'a>, uuid: &str) -> ItemRef<'a> {
    ctx.resolve(&kicad_core::kiid::Kiid::parse(uuid).unwrap())
        .unwrap_or_else(|| panic!("no item {uuid}"))
}

/// `VAL( double )`.
fn num<'e>(v: f64) -> PcbValue<'e> {
    PcbValue::from_double(v)
}

/// `VAL( wxString )`.
fn string<'e>(s: &str) -> PcbValue<'e> {
    PcbValue::from_string(s, false)
}

/// Port of `testEvalExpr` (with `PCBEXPR_CONTEXT( NULL_CONSTRAINT, UNDEFINED_LAYER )`).
fn test_eval_expr<'e>(
    expr: &str,
    expected: PcbValue<'e>,
    expect_error: bool,
    a: Option<ItemRef<'e>>,
    b: Option<ItemRef<'e>>,
) -> bool {
    test_eval_expr_on_layer(
        expr,
        expected,
        expect_error,
        a,
        b,
        PcbLayerId::UndefinedLayer,
    )
}

fn test_eval_expr_on_layer<'e>(
    expr: &str,
    expected: PcbValue<'e>,
    expect_error: bool,
    a: Option<ItemRef<'e>>,
    b: Option<ItemRef<'e>>,
    layer: PcbLayerId,
) -> bool {
    let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
    let mut env = PcbExprContext::new(NULL_CONSTRAINT, layer);
    env.set_items(a, b);
    let mut context: Context<'e, PcbExprUcode> = Context::new(env);
    let mut preflight: Context<'_, PcbExprUcode> =
        Context::new(PcbExprContext::new(NULL_CONSTRAINT, layer));

    let error = !compiler.compile(expr, &mut ucode, &mut preflight);

    assert_eq!(error, expect_error, "{expr}: {:?}", compiler.error());

    if error {
        return true;
    }

    let result = ucode.run(&mut context);
    let result = context.value(result);
    let ok = result.equal_to(&context, &expected);

    if expected.type_() == VarType::Numeric {
        assert_eq!(
            result.as_double(&context),
            expected.as_double(&context),
            "{expr}"
        );
    } else {
        assert_eq!(
            result.as_string(&context),
            expected.as_string(&context),
            "{expr}"
        );
    }

    ok
}

/// `expectCompileError` / `expectCompileSuccess`: the pending-error state after compiling.
fn compile_error_pending(expr: &str) -> bool {
    let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
    let mut preflight: Context<'_, PcbExprUcode> = Context::new(PcbExprContext::new(
        NULL_CONSTRAINT,
        PcbLayerId::UndefinedLayer,
    ));

    compiler.compile(expr, &mut ucode, &mut preflight);
    compiler.is_error_pending()
}

/// Two tracks on net1 (class HV_LINE, 10 mil) and net2 (class otherClass, 20 mil) as in
/// `IntrospectedProperties`.
const TRACKS: &str = r#"
	(segment (start 0 0) (end 10 0) (width 0.254) (layer "F.Cu") (net 1)
		(uuid "00000000-0000-0000-0000-00000000000a"))
	(segment (start 0 5) (end 10 5) (width 0.508) (layer "F.Cu") (net 2)
		(uuid "00000000-0000-0000-0000-00000000000b"))
"#;

/// Assign net classes by pattern (the project binding makes `SynchronizeNetsAndNetClasses`
/// run).
fn assign_netclasses(board: &mut Board, classes: &[&str], patterns: &[(&str, &str)]) {
    board.project = Some(BoardProject::default());
    let ns = &mut board.design_settings_mut().net_settings;

    for class in classes {
        ns.set_netclass(class, std::sync::Arc::new(Netclass::new(class, true)));
    }

    for (pattern, class) in patterns {
        ns.add_single_pattern_assignment(pattern, class);
    }

    board.synchronize_nets_and_netclasses(false);
}

fn tracks_board() -> Board {
    let mut board = load(TRACKS);
    assign_netclasses(
        &mut board,
        &["HV_LINE", "otherClass"],
        &[("net1", "HV_LINE"), ("net2", "otherClass")],
    );
    board
}

/// `IntrospectedProperties`: the expressions without function calls are evaluated, the
/// others (DRC-22) only compiled.
#[test]
fn introspected_properties() {
    let board = tracks_board();
    let ctx = BoardCtx::bare(&board);
    let a = item(&ctx, "00000000-0000-0000-0000-00000000000a");
    let b = item(&ctx, "00000000-0000-0000-0000-00000000000b");

    assert_eq!(a.effective_netclass().unwrap().get_name(), "HV_LINE");

    let cases: Vec<(&str, PcbValue<'_>)> = vec![
        ("A.Width > B.Width", num(0.0)),
        ("A.Width + B.Width", num(254_000.0 + 508_000.0)),
        ("A.Netclass", string("HV_LINE")),
        ("A.Net_Class == 'HV_LINE'", num(1.0)),
        (
            "(A.Netclass == 'HV_LINE') && (B.netclass == 'otherClass') && (B.netclass != 'F.Cu')",
            num(1.0),
        ),
        ("A.Netclass + 1.0", num(1.0)),
        (
            "A.type == 'Track' && B.type == 'Track' && A.layer == 'F.Cu'",
            num(1.0),
        ),
        (
            "(A.type == 'Track') && (B.type == 'Track') && (A.layer == 'F.Cu')",
            num(1.0),
        ),
    ];

    for (expr, expected) in cases {
        assert!(
            test_eval_expr(expr, expected, false, Some(a), Some(b)),
            "{expr}"
        );
    }

    // Function calls (pcbexpr_functions.cpp, package DRC-22): compile only.
    for expr in [
        "A.type == 'Pad' && B.type == 'Pad' && (A.existsOnLayer('F.Cu'))",
        "A.hasNetclass('HV_LINE')",
        "A.hasNetclass('HV_*')",
        "A.existsOnLayer(B.Layer)",
        "A.type == 'Via' && A.isMicroVia()",
    ] {
        assert!(!compile_error_pending(expr), "{expr}");
    }

    // The first one is false whatever the function returns.
    assert!(test_eval_expr(
        "A.type == 'Pad' && B.type == 'Pad' && (A.existsOnLayer('F.Cu'))",
        num(0.0),
        false,
        Some(a),
        Some(b)
    ));
}

/// `IntrospectedExtendedNumericProperties` (zone and footprint parts; the ellipse part is
/// not ported: the Rust board file reader has no ellipse shape yet).
#[test]
fn introspected_extended_numeric_properties() {
    let mut board = load(
        r#"
	(zone (net 0) (net_name "") (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000c1")
		(hatch edge 0.5) (priority 7) (connect_pads (clearance 0.5)) (min_thickness 0.25)
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy 0 0) (xy 10 0) (xy 10 10) (xy 0 10))))
	(footprint "Lib:FP" (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000f1") (at 50 50)
		(solder_paste_margin_ratio 0.125)
		(property "Reference" "J1" (at 0 0 0) (layer "F.SilkS")
			(uuid "00000000-0000-0000-0000-0000000000f2")
			(effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "V" (at 0 0 0) (layer "F.Fab")
			(uuid "00000000-0000-0000-0000-0000000000f3")
			(effects (font (size 1 1) (thickness 0.15))))
		(embedded_fonts no))
"#,
    );

    let zone_id = board.zones().next().unwrap().0;
    board.modify(zone_id.into(), |zone, _env| {
        let AnyItemMut::Zone(zone) = zone else {
            unreachable!()
        };
        zone.set_hatch_orientation(EdaAngle::from_degrees(45.0));
        zone.min_island_area = 123_456_789;
    });

    {
        let ctx = BoardCtx::bare(&board);
        let zone = item(&ctx, "00000000-0000-0000-0000-0000000000c1");
        let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");

        assert!(test_eval_expr(
            "A.Priority == 7",
            num(1.0),
            false,
            Some(zone),
            None
        ));
        assert!(test_eval_expr(
            "A.Hatch_Orientation == 45deg",
            num(1.0),
            false,
            Some(zone),
            None
        ));
        assert!(test_eval_expr(
            "A.Minimum_Island_Area == 123456789",
            num(1.0),
            false,
            Some(zone),
            None
        ));
        assert!(test_eval_expr(
            "A.Solderpaste_Margin_Ratio_Override == 0.125",
            num(1.0),
            false,
            Some(fp),
            None
        ));
    }

    let fp_id = board.footprints().next().unwrap().0;
    board.modify(fp_id.into(), |fp, _env| {
        let AnyItemMut::Footprint(fp) = fp else {
            unreachable!()
        };
        fp.solder_paste_margin_ratio = None;
    });

    let ctx = BoardCtx::bare(&board);
    let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");

    assert!(test_eval_expr(
        "A.Solderpaste_Margin_Ratio_Override == null",
        num(1.0),
        false,
        Some(fp),
        None
    ));
}

/// `RenamedProperties`.
#[test]
fn renamed_properties() {
    let board = load(
        r#"
	(segment (start 1 2) (end 3 4) (width 0.25) (layer "F.Cu") (net 0)
		(uuid "00000000-0000-0000-0000-00000000000a"))
"#,
    );
    let ctx = BoardCtx::bare(&board);
    let track = item(&ctx, "00000000-0000-0000-0000-00000000000a");

    assert!(!compile_error_pending("A.Origin_X == 1mm"));
    assert!(!compile_error_pending("A.Origin_Y == 2mm"));

    for expr in [
        "A.Origin_X == 1mm",
        "A.Origin_Y == 2mm",
        "A.Origin_X != A.End_X",
        "A.origin_y != A.End_Y",
        "A.Start_X == 1mm",
        "A.Start_Y == 2mm",
    ] {
        assert!(
            test_eval_expr(expr, num(1.0), false, Some(track), None),
            "{expr}"
        );
    }
}

/// `ExpressionPropertyTypesAreSupported`.
#[test]
fn expression_property_types_are_supported() {
    use kicad_drc::pcbexpr::evaluator::{PcbExprPropertyKind, PcbExprVarRef};
    use kicad_pcb::properties::{ClassId, property_manager};
    use std::collections::BTreeSet;

    let pm = property_manager();
    let mut seen = BTreeSet::new();
    let mut field_types: BTreeMap<&str, BTreeSet<i32>> = BTreeMap::new();

    for cls in pm.all_classes() {
        if !pm.is_of_type(cls.type_, ClassId::BoardItem) {
            continue;
        }

        for &prop in &cls.properties {
            if !seen.insert((cls.type_, prop)) {
                continue;
            }

            let p = pm.property(prop);
            let kind = PcbExprVarRef::classify_property(p);

            assert_ne!(
                kind,
                PcbExprPropertyKind::Unsupported,
                "Unsupported expression property: class={}, property={}",
                cls.name,
                p.name
            );

            field_types
                .entry(p.name)
                .or_default()
                .insert(PcbExprVarRef::expression_type(kind) as i32);
        }
    }

    // PCB_TARGET::Shape is numeric while EDA_SHAPE::Shape is an enum, so a global
    // expression reference cannot have one stable public type.
    let shape_types = field_types.remove("Shape").expect("Shape");
    assert_eq!(shape_types.len(), 2);

    for (field, types) in &field_types {
        assert_eq!(
            types.len(),
            1,
            "Incompatible expression types for property: {field}"
        );
    }

    assert!(compile_error_pending("A.Shape"));
}

/// `IntrospectedPropertyAvailability`.
#[test]
fn introspected_property_availability() {
    let zone = |rule_area: bool| {
        let keepout = if rule_area {
            "(keepout (tracks not_allowed) (vias allowed) (pads allowed) (copperpour allowed) \
             (footprints allowed))"
        } else {
            ""
        };
        load(&format!(
            r#"
	(zone (net 0) (net_name "") (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000c1")
		(hatch edge 0.5) (priority 7) (connect_pads (clearance 0.5)) (min_thickness 0.25)
		{keepout}
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy 0 0) (xy 10 0) (xy 10 10) (xy 0 10))))
"#
        ))
    };

    let board = zone(true);
    let ctx = BoardCtx::bare(&board);
    let z = item(&ctx, "00000000-0000-0000-0000-0000000000c1");
    assert!(z.as_zone().unwrap().is_rule_area());
    assert!(test_eval_expr(
        "A.Priority == 7",
        num(0.0),
        false,
        Some(z),
        None
    ));

    let board = zone(false);
    let ctx = BoardCtx::bare(&board);
    let z = item(&ctx, "00000000-0000-0000-0000-0000000000c1");
    assert!(test_eval_expr(
        "A.Priority == 7",
        num(1.0),
        false,
        Some(z),
        None
    ));
}

/// A footprint J1 with a text item (and a pad) as in `ParentNavigation` /
/// `PropertyParentFallback` / `BoolPropertyEvaluation` / `VariantAwareEvaluation`.
const FOOTPRINT_J1: &str = r#"
	(footprint "Lib:FP" (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000f1") (at 50 50)
		(property "Reference" "J1" (at 0 0 0) (layer "F.SilkS")
			(uuid "00000000-0000-0000-0000-0000000000f2")
			(effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "BaseVal" (at 0 0 0) (layer "F.Fab")
			(uuid "00000000-0000-0000-0000-0000000000f3")
			(effects (font (size 1 1) (thickness 0.15))))
		(fp_text user "J1" (at 0 2 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-0000000000f4")
			(effects (font (size 1 1) (thickness 0.15))))
		(pad "1" smd circle (at 0 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "net1")
			(pintype "tri_state") (uuid "00000000-0000-0000-0000-0000000000f5"))
		(embedded_fonts no))
"#;

/// `ParentNavigation` (the `getField()` expressions need DRC-22: compile only).
#[test]
fn parent_navigation() {
    let board = load(FOOTPRINT_J1);
    let ctx = BoardCtx::bare(&board);
    let text = item(&ctx, "00000000-0000-0000-0000-0000000000f4");

    assert!(test_eval_expr(
        "A.Parent.Type == 'Footprint'",
        num(1.0),
        false,
        Some(text),
        Some(text)
    ));

    // Regression: the terminal "Parent" string still yields the parent footprint reference,
    // i.e. the object and the string refer to the same parent.
    assert!(test_eval_expr(
        "A.Parent == 'J1'",
        num(1.0),
        false,
        Some(text),
        Some(text)
    ));

    for expr in [
        "A.Parent.getField('Reference') == 'J1'",
        "A.Parent.getField('Reference') == A.Text",
        "A.getField('Reference') == 'J1'",
    ] {
        assert!(!compile_error_pending(expr), "{expr}");
    }

    assert!(compile_error_pending("A.Parent.bogusProperty"));
    assert!(compile_error_pending("A.bogus.Reference == 'J1'"));
    assert!(compile_error_pending("L.Parent.Type == 'Footprint'"));
}

/// `PropertyParentFallback`.
#[test]
fn property_parent_fallback() {
    let board = load(FOOTPRINT_J1);
    let ctx = BoardCtx::bare(&board);
    let text = item(&ctx, "00000000-0000-0000-0000-0000000000f4");

    assert!(test_eval_expr(
        "A.Reference == 'J1'",
        num(1.0),
        false,
        Some(text),
        Some(text)
    ));
    assert!(test_eval_expr(
        "A.Reference == 'J2'",
        num(0.0),
        false,
        Some(text),
        Some(text)
    ));
}

/// `BoolPropertyEvaluation`.
#[test]
fn bool_property_evaluation() {
    let mut board = load(FOOTPRINT_J1);

    {
        let ctx = BoardCtx::bare(&board);
        let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");
        assert!(test_eval_expr(
            "A.Do_not_Populate == 0",
            num(1.0),
            false,
            Some(fp),
            Some(fp)
        ));
    }

    let fp_id = board.footprints().next().unwrap().0;
    board.modify(fp_id.into(), |fp, _env| {
        let AnyItemMut::Footprint(fp) = fp else {
            unreachable!()
        };
        fp.set_dnp(true);
    });

    let ctx = BoardCtx::bare(&board);
    let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");
    assert!(test_eval_expr(
        "A.Do_not_Populate == 1",
        num(1.0),
        false,
        Some(fp),
        Some(fp)
    ));
}

/// `VariantAwareEvaluation` (the DNP part; `getField('Value')` needs DRC-22).
#[test]
fn variant_aware_evaluation() {
    let mut board = load(&FOOTPRINT_J1.replace(
        "(embedded_fonts no))",
        "(variant (name \"V1\") (dnp yes) (field (name \"Value\") (value \"VariantVal\")))\n\t\t(embedded_fonts no))",
    ));
    board.variant_names.push("V1".to_string());
    board.current_variant = "V1".to_string();

    {
        let ctx = BoardCtx::bare(&board);
        let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");
        assert!(fp.as_footprint().unwrap().dnp_for_variant("V1"));
        assert!(test_eval_expr(
            "A.Do_not_Populate == 1",
            num(1.0),
            false,
            Some(fp),
            Some(fp)
        ));
    }

    // Without a current variant the base values apply.
    board.current_variant.clear();

    let ctx = BoardCtx::bare(&board);
    let fp = item(&ctx, "00000000-0000-0000-0000-0000000000f1");
    assert!(test_eval_expr(
        "A.Do_not_Populate == 0",
        num(1.0),
        false,
        Some(fp),
        Some(fp)
    ));
    assert!(!compile_error_pending("A.getField('Value') == 'BaseVal'"));
}

/// `ReceiverValidation`.
#[test]
fn receiver_validation() {
    assert!(!compile_error_pending("L == 'F.Cu'"));
    assert!(!compile_error_pending("AB.isCoupledDiffPair()"));
    assert!(!compile_error_pending("A.Parent.Reference == 'J1'"));
    assert!(!compile_error_pending("A.Width == 1mm"));

    assert!(compile_error_pending("L.Width == 1mm"));
    assert!(compile_error_pending("AB.Width == A.Width"));
    assert!(compile_error_pending("AB.Parent.Reference == 'J1'"));
}

/// `LayerReceiverLayerField`.
#[test]
fn layer_receiver_layer_field() {
    let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
    let mut preflight: Context<'_, PcbExprUcode> = Context::new(PcbExprContext::new(
        NULL_CONSTRAINT,
        PcbLayerId::UndefinedLayer,
    ));

    compiler.compile("L.Layer == '*.Paste'", &mut ucode, &mut preflight);
    assert!(!compiler.is_error_pending());

    let board = tracks_board();
    let ctx = BoardCtx::bare(&board);
    let track = item(&ctx, "00000000-0000-0000-0000-00000000000a");

    for (layer, expected) in [(PcbLayerId::FPaste, 1.0), (PcbLayerId::FCu, 0.0)] {
        let mut env = PcbExprContext::new(NULL_CONSTRAINT, layer);
        env.set_items(Some(track), Some(track));
        let mut context: Context<'_, PcbExprUcode> = Context::new(env);
        let result = ucode.run(&mut context);
        assert_eq!(context.value(result).as_double(&context), expected);
    }
}

/// `PairItemDependency`.
#[test]
fn pair_item_dependency() {
    let requires_pair_items = |expr: &str| {
        let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
        let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
        let mut preflight: Context<'_, PcbExprUcode> = Context::new(PcbExprContext::new(
            NULL_CONSTRAINT,
            PcbLayerId::UndefinedLayer,
        ));

        assert!(compiler.compile(expr, &mut ucode, &mut preflight));
        ucode.binder().requires_pair_items()
    };

    assert!(!requires_pair_items("A.Width == 1mm"));
    assert!(requires_pair_items("B.Width == 1mm"));
    assert!(requires_pair_items("AB.isCoupledDiffPair()"));
    assert!(!requires_pair_items("A.Reference == 'B.Width'"));
}

/* --------------------------------------------------------------------------------------------
 * Value classes and the DRC_RULE_CONDITION path
 */

/// `PCBEXPR_NET_VALUE`, `PCBEXPR_NETCLASS_VALUE` (constituents, wildcards, `!=`),
/// `PCBEXPR_LAYER_VALUE` (wildcards, both operand orders), `PCBEXPR_PINTYPE_VALUE`
/// (netlist token aliases), the parent-footprint fallback and the type matrix.
#[test]
fn value_classes() {
    let mut board = load(&format!("{TRACKS}{FOOTPRINT_J1}"));
    assign_netclasses(
        &mut board,
        &["HV_LINE", "Power"],
        &[("net1", "HV_LINE"), ("net1", "Power")],
    );

    let ctx = BoardCtx::bare(&board);
    let a = item(&ctx, "00000000-0000-0000-0000-00000000000a");
    let b = item(&ctx, "00000000-0000-0000-0000-00000000000b");
    // None while the pad parser (PCBIO-07) is pending: the pad cases are skipped.
    let pad = ctx
        .resolve(&kicad_core::kiid::Kiid::parse("00000000-0000-0000-0000-0000000000f5").unwrap());

    let composite = a.effective_netclass().unwrap().get_name();
    assert!(composite.contains(','), "{composite}");

    let cases: Vec<(&str, f64, Option<ItemRef<'_>>, Option<ItemRef<'_>>)> = vec![
        // nets
        ("A.NetName == 'net1'", 1.0, Some(a), Some(b)),
        ("A.NetName == B.NetName", 0.0, Some(a), Some(b)),
        ("A.NetName != B.NetName", 1.0, Some(a), Some(b)),
        ("A.NetName == B.NetName", 1.0, Some(a), pad),
        ("A.NetName == 'net*'", 1.0, Some(a), Some(b)),
        // composite net classes: constituents and the full name
        ("A.NetClass == 'HV_LINE'", 1.0, Some(a), Some(b)),
        ("A.NetClass == 'power'", 1.0, Some(a), Some(b)),
        ("A.NetClass == 'HV*'", 1.0, Some(a), Some(b)),
        ("A.NetClass != 'HV_LINE'", 0.0, Some(a), Some(b)),
        ("A.NetClass != 'Default'", 1.0, Some(a), Some(b)),
        ("A.NetClass == B.NetClass", 0.0, Some(a), Some(b)),
        ("A.NetClass == B.NetClass", 1.0, Some(a), pad),
        ("B.NetClass == 'Default'", 1.0, Some(a), Some(b)),
        // layers
        ("A.Layer == '*.Cu'", 1.0, Some(a), Some(b)),
        ("A.Layer == 'B.Cu'", 0.0, Some(a), Some(b)),
        ("A.Layer != 'B.Cu'", 1.0, Some(a), Some(b)),
        ("'F.Cu' == A.Layer", 1.0, Some(a), Some(b)),
        // pin type aliases
        ("A.Pin_Type == 'Tri-state'", 1.0, pad, None),
        ("A.Pin_Type == 'tri_state'", 1.0, pad, None),
        ("A.Pin_Type == 'tri*'", 1.0, pad, None),
        ("A.Pin_Type == 'passive'", 0.0, pad, None),
        // parent footprint fallback, undefined properties
        ("A.Reference == 'J1'", 1.0, pad, None),
        ("A.Reference == 'J1'", 0.0, Some(a), None),
        ("A.Reference != 'J1'", 0.0, Some(a), None),
        ("A.Pad_Type == 'SMD'", 1.0, pad, None),
        ("A.Pad_Type == 'SMD'", 0.0, Some(a), None),
        // numbers, strings, the type references
        ("A.Width == 0.254mm", 1.0, Some(a), None),
        (
            "A.Type == 'Pad' && A.Parent.Type == 'Footprint'",
            1.0,
            pad,
            None,
        ),
        ("A.Type == 'track'", 1.0, Some(a), None),
        ("!A.Locked", 1.0, Some(a), None),
        ("A.Pad_Number == '1'", 1.0, pad, None),
        ("A.Pad_Number == 1", 0.0, pad, None),
        ("null == null", 1.0, Some(a), None),
        ("A.Width == null", 0.0, Some(a), None),
    ];

    let mut skipped = 0;

    for (expr, expected, a, b) in cases {
        if a.is_none() || (expr.contains("B.") && b.is_none()) {
            skipped += 1;
            continue;
        }

        let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
        let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
        let mut preflight: Context<'_, PcbExprUcode> =
            Context::new(PcbExprContext::new(NULL_CONSTRAINT, PcbLayerId::FCu));

        compiler.compile(expr, &mut ucode, &mut preflight);
        assert!(
            !compiler.is_error_pending(),
            "{expr}: {:?}",
            compiler.error()
        );

        let mut env = PcbExprContext::new(NULL_CONSTRAINT, PcbLayerId::FCu);
        env.set_items(a, b);
        let mut context: Context<'_, PcbExprUcode> = Context::new(env);
        let result = ucode.run(&mut context);
        assert_eq!(
            context.value(result).as_double(&context),
            expected,
            "{expr}"
        );
    }

    if skipped > 0 {
        eprintln!("value_classes: {skipped} pad cases skipped (pads not loaded yet, PCBIO-07)");
    }

    // The specialised values.
    let mut env = PcbExprContext::new(NULL_CONSTRAINT, PcbLayerId::FCu);
    env.set_items(Some(a), None);
    assert_eq!(env.effective_type(a), KicadT::PcbTraceT);
    let context: Context<'_, PcbExprUcode> = Context::new(env);
    let v = PcbValue::from_string("", false).with_custom(PcbExprValue::Netclass(a));
    assert_eq!(v.as_string(&context), composite);
}

/// `DRC_RULE_CONDITION::EvaluateFor` with the default binding: commutative pairs and
/// run-time messages of the function stubs.
#[test]
fn rule_condition_evaluate_for() {
    let board = tracks_board();
    let ctx = BoardCtx::bare(&board);
    let a = item(&ctx, "00000000-0000-0000-0000-00000000000a");
    let b = item(&ctx, "00000000-0000-0000-0000-00000000000b");

    let mut condition =
        DrcRuleCondition::new("A.NetClass == 'HV_LINE' && B.NetClass == 'otherClass'");
    assert!(condition.compile(None, 0, 0));
    assert!(condition.requires_pair_items());
    assert!(condition.evaluate_for(Some(a), Some(b), NULL_CONSTRAINT, PcbLayerId::FCu, None));
    // Conditions are commutative
    assert!(condition.evaluate_for(Some(b), Some(a), NULL_CONSTRAINT, PcbLayerId::FCu, None));
    assert!(!condition.evaluate_for(Some(a), Some(a), NULL_CONSTRAINT, PcbLayerId::FCu, None));

    let mut stub = DrcRuleCondition::new("A.isPlated()");
    assert!(stub.compile(None, 0, 0));
    let mut reporter = kicad_drc::reporter::WxStringReporter::new();
    assert!(!stub.evaluate_for(
        Some(a),
        None,
        NULL_CONSTRAINT,
        PcbLayerId::FCu,
        Some(&mut reporter)
    ));
    assert!(
        reporter
            .messages()
            .iter()
            .any(|(m, _)| m == "ERROR: Function 'isPlated' is not yet ported (DRC-22)."),
        "{:?}",
        reporter.messages()
    );
}
