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
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Package DRC-22: the built-in functions of `pcbnew/pcbexpr_functions.cpp`.
//!
//! The function cases of `qa/tests/pcbnew/test_libeval_compiler.cpp` (`InNetChainClassWildcard`,
//! `StackedMicroviaExpression`, `DynamicCourtyardArgument`, `FunctionArgumentValidation`) on
//! parsed boards, plus behaviour checks of the other functions (the expected values follow the
//! C++ statements; the compile-time argument validation is checked against the real C++ in the
//! DRC-21 codegen golden `drc21_conditions.txt`, file `arguments.kicad_dru`).

use kicad_core::{KicadT, PcbLayerId};
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::pcbexpr::evaluator::pcbexpr_compiler;
use kicad_drc::pcbexpr::evaluator::{PcbExprContext, PcbExprUcode, PcbExprUnitResolver};
use kicad_drc::pcbexpr::functions::{PcbExprCacheScope, intersects_keepout};
use kicad_drc::rule_condition::DrcRuleCondition;
use kicad_geom::math::vector2::Vector2I;
use kicad_libeval::{Context, Ucode};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItemMut, Board, ItemRef};

const NULL_CONSTRAINT: i32 = DrcConstraintT::NullConstraint as i32;

fn board_text(layers: &str, items: &str) -> String {
    format!(
        r#"(kicad_pcb
	(version 20260410)
	(generator "pcbnew")
	(generator_version "10.99")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers
{layers}
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
	(net 1 "netA")
	(net 2 "netB")
	(net 3 "netC")
	(net 4 "D_P")
	(net 5 "D_N")
	(net 6 "E_+")
{items}
	(embedded_fonts no)
)
"#
    )
}

const TWO_LAYERS: &str = "\t\t(0 \"F.Cu\" signal)\n\t\t(2 \"B.Cu\" signal)";
const FOUR_LAYERS: &str = "\t\t(0 \"F.Cu\" signal)\n\t\t(4 \"In1.Cu\" signal)\n\t\t(6 \"In2.Cu\" \
                           signal)\n\t\t(2 \"B.Cu\" signal)";

fn load(layers: &str, items: &str) -> Board {
    parse_board(
        &board_text(layers, items),
        "test.kicad_pcb",
        &ParseOptions {
            allow_pending: true,
        },
    )
    .expect("test board")
}

fn item<'a>(ctx: &'a BoardCtx<'a>, uuid: &str) -> ItemRef<'a> {
    ctx.resolve(&kicad_core::kiid::Kiid::parse(uuid).unwrap())
        .unwrap_or_else(|| panic!("no item {uuid}"))
}

fn uuid(n: u32) -> String {
    format!("00000000-0000-0000-0000-{n:012x}")
}

/// Port of `testEvalExpr` (`PCBEXPR_CONTEXT( NULL_CONSTRAINT, layer )`, no error callback):
/// the numeric result.
fn eval_on<'e>(
    expr: &str,
    a: Option<ItemRef<'e>>,
    b: Option<ItemRef<'e>>,
    constraint: i32,
    layer: PcbLayerId,
) -> f64 {
    let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
    let mut env = PcbExprContext::new(constraint, layer);
    env.set_items(a, b);
    let mut context: Context<'e, PcbExprUcode> = Context::new(env);
    let mut preflight: Context<'_, PcbExprUcode> =
        Context::new(PcbExprContext::new(constraint, layer));

    assert!(
        compiler.compile(expr, &mut ucode, &mut preflight),
        "{expr}: {:?}",
        compiler.error()
    );
    assert!(
        !compiler.is_error_pending(),
        "{expr}: {:?}",
        compiler.error()
    );

    let result = ucode.run(&mut context);
    context.value(result).as_double(&context)
}

fn eval<'e>(expr: &str, a: ItemRef<'e>, b: ItemRef<'e>) -> f64 {
    eval_on(
        expr,
        Some(a),
        Some(b),
        NULL_CONSTRAINT,
        PcbLayerId::UndefinedLayer,
    )
}

/// The string result of `testEvalExpr`.
fn eval_string<'e>(expr: &str, a: ItemRef<'e>) -> String {
    let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));
    let mut ucode: Ucode<PcbExprUcode> = Ucode::new(PcbExprUcode::default());
    let mut env = PcbExprContext::new(NULL_CONSTRAINT, PcbLayerId::UndefinedLayer);
    env.set_items(Some(a), Some(a));
    let mut context: Context<'e, PcbExprUcode> = Context::new(env);
    let mut preflight: Context<'_, PcbExprUcode> = Context::new(PcbExprContext::new(
        NULL_CONSTRAINT,
        PcbLayerId::UndefinedLayer,
    ));

    assert!(compiler.compile(expr, &mut ucode, &mut preflight), "{expr}");

    let result = ucode.run(&mut context);
    context.value(result).as_string(&context).to_string()
}

/// `expectCompileError`.
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

/// The run-time messages of `DRC_RULE_CONDITION::EvaluateFor` with a reporter.
fn evaluate_reporting<'e>(
    expr: &str,
    a: ItemRef<'e>,
    b: Option<ItemRef<'e>>,
    layer: PcbLayerId,
) -> (bool, Vec<String>) {
    let mut condition = DrcRuleCondition::new(expr);
    assert!(condition.compile(None, 0, 0), "{expr}");
    let mut reporter = kicad_drc::reporter::WxStringReporter::new();
    let result = condition.evaluate_for(Some(a), b, NULL_CONSTRAINT, layer, Some(&mut reporter));
    let messages = reporter.messages().iter().map(|(m, _)| m.clone()).collect();
    (result, messages)
}

/// `InNetChainClassWildcard`.
#[test]
fn in_net_chain_class_wildcard() {
    let mut board = load(
        TWO_LAYERS,
        &format!(
            r#"	(net_chains (net_chain (name "ChainOrphan") (members (net 1)))
		(net_chain (name "ChainHS") (members (net 2))))
	(segment (start 0 0) (end 10 0) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
	(segment (start 0 5) (end 10 5) (width 0.25) (layer "F.Cu") (net 2) (uuid "{}"))
	(segment (start 0 9) (end 10 9) (width 0.25) (layer "F.Cu") (net 3) (uuid "{}"))
"#,
            uuid(1),
            uuid(2),
            uuid(3)
        ),
    );

    board
        .design_settings_mut()
        .net_settings
        .set_net_chain_class("ChainHS", "HighSpeed");

    let ctx = BoardCtx::bare(&board);
    let unclassified = item(&ctx, &uuid(1));
    let classified = item(&ctx, &uuid(2));
    let no_chain = item(&ctx, &uuid(3));

    assert_eq!(unclassified.net().unwrap().net_chain(), "ChainOrphan");

    // A chain with no class assignment must not match any inNetChainClass() pattern,
    // including the '*' wildcard.
    let u = unclassified;
    assert_eq!(eval("A.inNetChainClass('*')", u, u), 0.0);
    assert_eq!(eval("A.inNetChainClass('HighSpeed')", u, u), 0.0);

    // Net with no chain at all must not match either.
    assert_eq!(eval("A.inNetChainClass('*')", no_chain, no_chain), 0.0);

    // Properly classified chain must match both wildcard and exact patterns.
    let c = classified;
    assert_eq!(eval("A.inNetChainClass('*')", c, c), 1.0);
    assert_eq!(eval("A.inNetChainClass('HighSpeed')", c, c), 1.0);
    assert_eq!(eval("A.inNetChainClass('High*')", c, c), 1.0);
    assert_eq!(eval("A.inNetChainClass('LowSpeed')", c, c), 0.0);

    // inNetChain() / hasNetChain() on the same board.
    assert_eq!(eval("A.inNetChain('Chain*')", u, u), 1.0);
    assert_eq!(eval("A.inNetChain('ChainHS')", u, u), 0.0);
    assert_eq!(eval("A.hasNetChain()", c, c), 1.0);
    assert_eq!(eval("A.hasNetChain()", no_chain, no_chain), 0.0);
    assert_eq!(eval("A.inNetChain('*')", no_chain, no_chain), 0.0);
}

/// `StackedMicroviaExpression`.
#[test]
fn stacked_microvia_expression() {
    let via = |n: u32, x: f64, top: &str, bottom: &str| {
        format!(
            "\t(via micro (at {x} 0) (size 0.25) (drill 0.1) (layers \"{top}\" \"{bottom}\") (net 0) \
             (uuid \"{}\"))\n",
            uuid(n)
        )
    };

    let mut items = String::new();
    items += &via(1, 0.0, "F.Cu", "In1.Cu"); // upperHop
    items += &via(2, 0.0, "In1.Cu", "In2.Cu"); // lowerHop
    items += &via(3, 10.0, "F.Cu", "In1.Cu"); // lone
    items += &format!(
        "\t(via (at 0 0) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 0) (uuid \"{}\"))\n",
        uuid(4)
    ); // through
    // The C++ test adds the landing via later; here it waits far away and is moved under
    // `lone` (a modification bumps the time stamp like `brd.IncrementTimeStamp()`).
    items += &via(5, 50.0, "In1.Cu", "In2.Cu");

    let mut board = load(FOUR_LAYERS, &items);

    // The relation is cached for the whole board (inside a cache scope), so an edit has to
    // drop it.
    let _scope = PcbExprCacheScope::enter(&board);

    {
        let ctx = BoardCtx::bare(&board);
        let upper = item(&ctx, &uuid(1));
        let lower = item(&ctx, &uuid(2));
        let lone = item(&ctx, &uuid(3));
        let through = item(&ctx, &uuid(4));

        // Both hops of a stack are in it, not just the one on top.
        assert_eq!(eval("A.isStackedVia()", upper, upper), 1.0);
        assert_eq!(eval("A.isStackedVia()", lower, lower), 1.0);

        // A microvia landing on nothing is not a stack.
        assert_eq!(eval("A.isStackedVia()", lone, lone), 0.0);

        // A through via sharing the position of a stack is not part of it.
        assert_eq!(eval("A.isStackedVia()", through, through), 0.0);

        // The predicate composes with the rest of the language.
        assert_eq!(eval("A.isMicroVia() && !A.isStackedVia()", lone, lone), 1.0);

        // The via type predicates.
        assert_eq!(eval("A.isMicroVia()", through, through), 0.0);
        assert_eq!(eval("A.isBlindBuriedVia()", lone, lone), 0.0);
        assert_eq!(eval("A.isPlated()", through, through), 1.0);
    }

    let landing_id = item(&BoardCtx::bare(&board), &uuid(5)).id().unwrap();
    let stamp = board.time_stamp();
    board.modify(landing_id, |it, env| {
        let AnyItemMut::Track(Track::Via(via)) = it else {
            unreachable!()
        };
        via.set_position(env, Vector2I::new(10_000_000, 0));
    });
    assert_ne!(board.time_stamp(), stamp);

    let ctx = BoardCtx::bare(&board);
    let lone = item(&ctx, &uuid(3));
    let landing = item(&ctx, &uuid(5));
    assert_eq!(eval("A.isStackedVia()", lone, lone), 1.0);
    assert_eq!(eval("A.isStackedVia()", landing, landing), 1.0);
}

/// Two footprints at the origin: U1 with a 4 mm x 4 mm front courtyard and a text, U2 with
/// a 1 mm graphic on F.Fab and a text (`DynamicCourtyardArgument`).
fn courtyard_board() -> Board {
    load(
        TWO_LAYERS,
        &format!(
            r#"	(footprint "Lib:Target" (layer "F.Cu") (uuid "{}") (at 0 0)
		(property "Reference" "U1" (at 0 0 0) (layer "F.SilkS") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "T" (at 0 0 0) (layer "F.Fab") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(fp_rect (start 0 0) (end 4 4) (stroke (width 0.05) (type solid)) (fill no)
			(layer "F.CrtYd") (uuid "{}"))
		(fp_text user "t" (at 0 2 0) (layer "F.Fab") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(embedded_fonts no))
	(footprint "Other:Lib" (layer "F.Cu") (uuid "{}") (at 0 0)
		(property "Reference" "U2" (at 0 0 0) (layer "F.SilkS") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(property "Value" "O" (at 0 0 0) (layer "F.Fab") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(fp_line (start 0 0) (end 1 0) (stroke (width 0.05) (type solid)) (layer "F.Fab")
			(uuid "{}"))
		(fp_text user "o" (at 0 2 0) (layer "F.Fab") (uuid "{}")
			(effects (font (size 1 1) (thickness 0.15))))
		(embedded_fonts no))
	(segment (start 2 2) (end 3 2) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
	(segment (start 20 20) (end 21 20) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
"#,
            uuid(0x10),
            uuid(0x11),
            uuid(0x12),
            uuid(0x13),
            uuid(0x14),
            uuid(0x20),
            uuid(0x21),
            uuid(0x22),
            uuid(0x23),
            uuid(0x24),
            uuid(0x30),
            uuid(0x31)
        ),
    )
}

/// `DynamicCourtyardArgument`.
#[test]
fn dynamic_courtyard_argument() {
    let board = courtyard_board();
    let ctx = BoardCtx::bare(&board);
    let graphic = item(&ctx, &uuid(0x23));
    let target_child = item(&ctx, &uuid(0x14));
    let other_child = item(&ctx, &uuid(0x24));

    for expression in [
        "A.intersectsFrontCourtyard(B.Parent)",
        "A.intersectsCourtyard(B.Parent.Reference)",
        "A.intersectsFrontCourtyard(B.Parent.getField('Reference'))",
    ] {
        let run = |b: ItemRef<'_>| {
            eval_on(
                expression,
                Some(graphic),
                Some(b),
                NULL_CONSTRAINT,
                PcbLayerId::FFab,
            )
        };

        assert_eq!(run(target_child), 1.0, "{expression}");
        assert_eq!(run(other_child), 0.0, "{expression}");
        assert_eq!(run(target_child), 1.0, "{expression}");
    }
}

/// Selectors and sides of the courtyard predicates, with and without the board caches.
#[test]
fn courtyard_selectors() {
    let board = courtyard_board();

    for cached in [false, true] {
        let _scope = cached.then(|| PcbExprCacheScope::enter(&board));
        let ctx = BoardCtx::bare(&board);
        let inside = item(&ctx, &uuid(0x30));
        let outside = item(&ctx, &uuid(0x31));
        let u1 = item(&ctx, &uuid(0x10));
        let u2 = item(&ctx, &uuid(0x20));
        let on = |expr: &str, a: ItemRef<'_>, b: ItemRef<'_>| {
            eval_on(expr, Some(a), Some(b), NULL_CONSTRAINT, PcbLayerId::FCu)
        };

        assert_eq!(on("A.intersectsCourtyard('U1')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsCourtyard('U*')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsCourtyard('U2')", inside, inside), 0.0);
        assert_eq!(on("A.intersectsCourtyard('U1')", outside, outside), 0.0);
        assert_eq!(on("A.insideCourtyard('U1')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsFrontCourtyard('U1')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsBackCourtyard('U1')", inside, inside), 0.0);
        assert_eq!(on("A.intersectsCourtyard('Lib:*')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsCourtyard('Other:*')", inside, inside), 0.0);
        assert_eq!(on("A.intersectsCourtyard('B')", inside, u1), 1.0);
        assert_eq!(on("A.intersectsCourtyard('B')", inside, u2), 0.0);
        assert_eq!(on("A.intersectsCourtyard('B')", inside, inside), 0.0);
        assert_eq!(on("B.intersectsCourtyard('A')", u1, inside), 1.0);
        // Repeated: memo hits give the same answers.
        assert_eq!(on("A.intersectsCourtyard('U1')", inside, inside), 1.0);
        assert_eq!(on("A.intersectsCourtyard('U2')", inside, inside), 0.0);

        // memberOfFootprint / getField / memberOfSheet on the footprint children.
        let text = item(&ctx, &uuid(0x14));
        assert_eq!(eval("A.memberOfFootprint('U1')", text, text), 1.0);
        assert_eq!(eval("A.memberOfFootprint('U2')", text, text), 0.0);
        assert_eq!(eval("A.memberOfFootprint('Lib:T*')", text, text), 1.0);
        assert_eq!(eval("A.memberOfFootprint('U1')", u1, u1), 0.0);
        assert_eq!(eval_string("A.getField('Reference')", u1), "U1");
        assert_eq!(eval_string("A.getField('Value')", u1), "T");
        assert_eq!(eval_string("A.getField('Nope')", u1), "");
        assert_eq!(eval_string("A.getField('Reference')", text), "");
        assert_eq!(eval("A.memberOfSheet('/')", text, text), 1.0);
        assert_eq!(eval("A.memberOfSheet('/a')", inside, inside), 0.0);
        assert_eq!(eval("A.memberOfSheetOrChildren('/')", u1, u1), 1.0);
        assert_eq!(eval("A.memberOfSheetOrChildren('/a')", u1, u1), 0.0);
    }
}

/// `FunctionArgumentValidation`.
#[test]
fn function_argument_validation() {
    assert!(compile_error_pending("A.intersectsFrontCourtyard()"));
    assert!(compile_error_pending("A.intersectsFrontCourtyard('')"));
    assert!(compile_error_pending("A.memberOfFootprint('')"));
    assert!(compile_error_pending("A.fromTo('U1-1')"));
    assert!(compile_error_pending(
        "A.intersectsFrontCourtyard(B.UnknownProperty)"
    ));
    assert!(compile_error_pending(
        "A.intersectsFrontCourtyard(B.Parent.unknownFunction())"
    ));

    assert!(!compile_error_pending("A.existsOnLayer('')"));
    assert!(!compile_error_pending("A.customProperty('')"));
    assert!(!compile_error_pending("A.fromTo('U1-1', 'U2-1')"));
}

/// Rule areas: a keepout `K` on F.Cu (0..10 mm), a named rule area `R` on both copper
/// layers, tracks inside, crossing and outside.
fn area_board() -> Board {
    load(
        TWO_LAYERS,
        &format!(
            r#"	(segment (start 2 2) (end 4 2) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
	(segment (start 8 5) (end 14 5) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
	(segment (start 30 30) (end 34 30) (width 0.25) (layer "F.Cu") (net 1) (uuid "{}"))
	(segment (start 2 2) (end 4 2) (width 0.25) (layer "B.Cu") (net 1) (uuid "{}"))
	(segment (start 0 0) (end 1 0) (width 0.25) (layer "F.Cu") (net 4) (uuid "{}"))
	(segment (start 0 1) (end 1 1) (width 0.25) (layer "F.Cu") (net 5) (uuid "{}"))
	(segment (start 0 2) (end 1 2) (width 0.25) (layer "F.Cu") (net 6) (uuid "{}"))
	(zone (net 0) (net_name "") (layer "F.Cu") (uuid "{}") (name "K") (hatch edge 0.5)
		(connect_pads (clearance 0)) (min_thickness 0.25) (filled_areas_thickness no)
		(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed)
			(footprints allowed))
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy 0 0) (xy 10 0) (xy 10 10) (xy 0 10))))
	(zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (uuid "{}") (name "R") (hatch edge 0.5)
		(connect_pads (clearance 0)) (min_thickness 0.25) (filled_areas_thickness no)
		(keepout (tracks allowed) (vias allowed) (pads allowed) (copperpour allowed)
			(footprints allowed))
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy 1 1) (xy 5 1) (xy 5 3) (xy 1 3))))
"#,
            uuid(0x40),
            uuid(0x41),
            uuid(0x42),
            uuid(0x43),
            uuid(0x44),
            uuid(0x45),
            uuid(0x46),
            uuid(0x50),
            uuid(0x51)
        ),
    )
}

/// `intersectsArea` / `intersectsKeepout` / `insideArea` / `enclosedByArea`, by name, by UUID
/// and by `A` / `B`, on a defined and an undefined layer.
#[test]
fn rule_area_predicates() {
    let board = area_board();

    for cached in [false, true] {
        let _scope = cached.then(|| PcbExprCacheScope::enter(&board));
        let ctx = BoardCtx::bare(&board);
        let inside = item(&ctx, &uuid(0x40));
        let crossing = item(&ctx, &uuid(0x41));
        let outside = item(&ctx, &uuid(0x42));
        let back = item(&ctx, &uuid(0x43));
        let keepout = item(&ctx, &uuid(0x50));
        let area = item(&ctx, &uuid(0x51));
        let any = PcbLayerId::UndefinedLayer;
        let on = |expr: &str, a: ItemRef<'_>, b: ItemRef<'_>, layer: PcbLayerId| {
            eval_on(expr, Some(a), Some(b), NULL_CONSTRAINT, layer)
        };

        for layer in [any, PcbLayerId::FCu] {
            assert_eq!(on("A.intersectsArea('K')", inside, inside, layer), 1.0);
            assert_eq!(on("A.intersectsArea('K')", crossing, crossing, layer), 1.0);
            assert_eq!(on("A.intersectsArea('K')", outside, outside, layer), 0.0);
            assert_eq!(on("A.intersectsKeepout('K')", inside, inside, layer), 1.0);
            assert_eq!(on("A.insideArea('K')", inside, inside, layer), 1.0);
            assert_eq!(on("A.intersectsArea('?')", inside, inside, layer), 1.0);
            assert_eq!(on("A.enclosedByArea('K')", inside, inside, layer), 1.0);
            assert_eq!(on("A.enclosedByArea('K')", crossing, crossing, layer), 0.0);
            assert_eq!(on("A.enclosedByArea('R')", inside, inside, layer), 1.0);
            assert_eq!(on("A.intersectsArea('B')", inside, keepout, layer), 1.0);
            assert_eq!(on("A.intersectsArea('B')", outside, keepout, layer), 0.0);
            assert_eq!(on("A.intersectsArea('B')", inside, inside, layer), 0.0);
            // The area itself is never its own hit.
            assert_eq!(on("A.intersectsArea('K')", keepout, keepout, layer), 0.0);
            // A zone item: the area outline against the zone outline.
            assert_eq!(on("A.intersectsArea('R')", keepout, keepout, layer), 1.0);
            assert_eq!(on("A.enclosedByArea('K')", area, area, layer), 1.0);
        }

        // The keepout is on F.Cu only.
        assert_eq!(on("A.intersectsArea('K')", back, back, any), 0.0);
        assert_eq!(on("A.intersectsArea('R')", back, back, any), 1.0);
        assert_eq!(
            on("A.intersectsArea('R')", back, back, PcbLayerId::BCu),
            1.0
        );
        // The context layer is tested even when the item is not on it (a track's effective
        // shape does not depend on the layer): the C++ answers true here as well.
        assert_eq!(
            on("A.intersectsArea('R')", inside, inside, PcbLayerId::BCu),
            1.0
        );

        let by_uuid = format!("A.intersectsArea('{}')", uuid(0x50));
        assert_eq!(on(&by_uuid, inside, inside, any), 1.0);
        assert_eq!(on(&by_uuid, outside, outside, any), 0.0);
        let by_unknown_uuid = format!("A.intersectsArea('{}')", uuid(0x99));
        assert_eq!(on(&by_unknown_uuid, inside, inside, any), 0.0);

        // The native form of the implicit keepout condition.
        let selector = uuid(0x50);
        assert!(intersects_keepout(
            Some(inside),
            None,
            &selector,
            DrcConstraintT::DisallowConstraint as i32,
            PcbLayerId::FCu
        ));
        assert!(!intersects_keepout(
            Some(outside),
            None,
            &selector,
            DrcConstraintT::DisallowConstraint as i32,
            PcbLayerId::FCu
        ));
    }
}

/// `existsOnLayer`, `hasNetclass`, `hasExactNetclass`, diff pairs, custom properties and the
/// run-time messages of the interpreted `existsOnLayer`.
#[test]
fn item_predicates() {
    let board = area_board();
    let ctx = BoardCtx::bare(&board);
    let front = item(&ctx, &uuid(0x40));
    let back = item(&ctx, &uuid(0x43));
    let p = item(&ctx, &uuid(0x44));
    let n = item(&ctx, &uuid(0x45));
    let lone = item(&ctx, &uuid(0x46));
    let keepout = item(&ctx, &uuid(0x50));

    assert_eq!(eval("A.existsOnLayer('F.Cu')", front, front), 1.0);
    assert_eq!(eval("A.existsOnLayer('B.Cu')", front, front), 0.0);
    assert_eq!(eval("A.existsOnLayer('*.Cu')", back, back), 1.0);
    assert_eq!(eval("A.existsOnLayer('X.Cu')", back, back), 0.0);
    assert_eq!(eval("A.existsOnLayer('F.Cu')", keepout, keepout), 1.0);

    let (result, messages) =
        evaluate_reporting("A.existsOnLayer('X.Cu')", front, None, PcbLayerId::FCu);
    assert!(!result);
    assert_eq!(messages, ["ERROR: Unrecognized layer 'X.Cu'"]);

    let (result, messages) =
        evaluate_reporting("A.existsOnLayer('F.*')", front, None, PcbLayerId::FCu);
    assert!(result);
    assert!(messages.is_empty(), "{messages:?}");

    assert_eq!(eval("A.hasNetclass('Default')", front, front), 1.0);
    // ContainsNetclassWithName() matches wildcards, NameEquals() does not.
    assert_eq!(eval("A.hasNetclass('Def*')", front, front), 1.0);
    assert_eq!(eval("A.hasExactNetclass('Default')", front, front), 1.0);
    assert_eq!(eval("A.hasExactNetclass('default')", front, front), 0.0);
    // ZONE::IsConnected() is false for a rule area.
    assert_eq!(eval("A.hasNetclass('Default')", keepout, keepout), 0.0);

    // D_P / D_N are a pair, E_+ has no partner.
    assert_eq!(eval("A.inDiffPair('D')", p, p), 1.0);
    assert_eq!(eval("A.inDiffPair('D')", n, n), 1.0);
    assert_eq!(eval("A.inDiffPair('D_')", n, n), 1.0);
    assert_eq!(eval("A.inDiffPair('*')", lone, lone), 0.0);
    assert_eq!(eval("A.inDiffPair('*')", front, front), 0.0);
    assert_eq!(eval("A.isCoupledDiffPair()", p, n), 1.0);
    assert_eq!(eval("A.isCoupledDiffPair()", p, p), 0.0);
    assert_eq!(eval("A.isCoupledDiffPair()", front, n), 0.0);

    let gap = DrcConstraintT::DiffPairGapConstraint as i32;
    let coupled =
        |a: ItemRef<'_>| eval_on("A.isCoupledDiffPair()", Some(a), None, gap, PcbLayerId::FCu);
    assert_eq!(coupled(p), 1.0);
    assert_eq!(coupled(lone), 0.0);

    assert_eq!(eval("A.hasCustomProperty('MPN')", front, front), 0.0);
    assert_eq!(eval_string("A.customProperty('MPN')", front), "");
    assert_eq!(eval("A.memberOfGroup('*')", front, front), 0.0);
    assert_eq!(eval("A.hasComponentClass('*')", front, front), 0.0);
    assert_eq!(eval("A.fromTo('U1-1', 'U2-1')", front, front), 0.0);
    assert_eq!(front.type_(), KicadT::PcbTraceT);
}
