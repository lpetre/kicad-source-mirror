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

//! A miniature of pcbnew's LIBEVAL binding (`pcbnew/pcbexpr_evaluator.{h,cpp}`,
//! `pcbnew/pcbexpr_functions.cpp`, `pcbnew/drc/drc_rule_condition.cpp`) written against this
//! crate's extension points.
//!
//! This is *not* the port of that binding (which belongs to `kicad-drc` and needs the real
//! board model and property system).  It is the proof that everything the binding needs can
//! be expressed without `unsafe` and without down-casts, and the template for the real one:
//!
//! | pcbnew | here |
//! |---|---|
//! | `PCBEXPR_UCODE` (virtual `CreateVarRef` / `CreateFuncCall`, usage flags) | [`PcbBinder`] |
//! | `PCBEXPR_CONTEXT` (items, layer, constraint; borrows the board) | [`PcbEnv`] |
//! | `PCBEXPR_VAR_REF` and its "object code" subclasses, `GetObject()` | [`PcbVarRef`] |
//! | `PCBEXPR_NET_VALUE`, `PCBEXPR_NETCLASS_VALUE`, `PCBEXPR_LAYER_VALUE` | [`PcbValue`] |
//! | `existsOnLayerFunc`, `memberOfGroupFunc`, ... (`void f( CONTEXT*, void* self )`) | plain functions |
//! | `DRC_RULE_CONDITION` (`Compile`, `EvaluateFor`) | [`RuleCondition`] |
//!
//! The compiled [`Ucode`] has no lifetime parameter: a rule compiled once can be evaluated
//! against any borrow of any board.

mod common;

use common::PcbExprUnitResolver;
use kicad_libeval::{
    Compiler, Context, CustomValue, FuncCallRef, Ucode, UcodeBinder, Value, VarRef, VarType,
};

/* --------------------------------------------------------------------------------------------
 * A stand-in board model
 */

struct Item {
    type_name: &'static str,
    net_code: i32,
    net_name: &'static str,
    netclass: &'static str,
    /// in nanometres; `None` for item types without a width
    width: Option<i32>,
    layers: &'static [&'static str],
    group: Option<&'static str>,
    /// index of the parent footprint in `Board::items`
    parent: Option<usize>,
}

struct Board {
    items: Vec<Item>,
    layer_names: &'static [&'static str],
}

fn board() -> Board {
    let item = |type_name, net_code, net_name, netclass, width, layers, group, parent| Item {
        type_name,
        net_code,
        net_name,
        netclass,
        width,
        layers,
        group,
        parent,
    };

    Board {
        items: vec![
            item("Footprint", 0, "", "", None, &["F.Cu"], Some("power"), None),
            item(
                "Track",
                1,
                "VIN",
                "HV_LINE",
                Some(254_000),
                &["F.Cu"],
                None,
                None,
            ),
            item(
                "Track",
                2,
                "GND",
                "Default",
                Some(508_000),
                &["F.Cu"],
                None,
                None,
            ),
            item(
                "Track",
                1,
                "VIN",
                "HV_LINE",
                Some(200_000),
                &["B.Cu"],
                None,
                None,
            ),
            item(
                "Via",
                2,
                "GND",
                "Default",
                None,
                &["F.Cu", "B.Cu"],
                None,
                None,
            ),
            item("Pad", 1, "VIN", "HV_LINE", None, &["F.Cu"], None, Some(0)),
        ],
        layer_names: &["F.Cu", "B.Cu", "F.SilkS"],
    }
}

/* --------------------------------------------------------------------------------------------
 * The binding
 */

/// The members `PCBEXPR_CONTEXT` adds to `LIBEVAL::CONTEXT`.
struct PcbEnv<'e> {
    board: &'e Board,
    items: [Option<&'e Item>; 2],
    layer: &'static str,
    constraint: i32,
}

/// The subclasses of `LIBEVAL::VALUE`.
enum PcbValue<'e> {
    /// `PCBEXPR_NET_VALUE`: compares net codes, the string is the net name.
    Net(&'e Item),
    /// `PCBEXPR_NETCLASS_VALUE`: the string is computed on demand.
    NetClass(&'e Item),
    /// `PCBEXPR_LAYER_VALUE`: equal to every layer name the other side's pattern matches.
    Layer(&'static str),
}

/// `wxString::Matches`: `*` and `?` wildcards, case sensitive.
fn matches(text: &str, pattern: &str) -> bool {
    kicad_libeval::string_utils::wild_compare_string(pattern, text, true)
}

impl<'e> CustomValue<'e, PcbBinder> for PcbValue<'e> {
    fn as_string(
        &self,
        _value: &Value<'e, PcbBinder>,
        _ctx: &Context<'e, PcbBinder>,
    ) -> Option<String> {
        match self {
            PcbValue::Net(item) => Some(item.net_name.to_string()),
            PcbValue::NetClass(item) => Some(item.netclass.to_string()),
            PcbValue::Layer(_) => None,
        }
    }

    fn equal_to(
        &self,
        value: &Value<'e, PcbBinder>,
        ctx: &Context<'e, PcbBinder>,
        b: &Value<'e, PcbBinder>,
    ) -> bool {
        match (self, b.custom()) {
            // C++: if( const PCBEXPR_NET_VALUE* bValue = dynamic_cast<...>( b ) )
            (PcbValue::Net(item), Some(PcbValue::Net(b_item))) => item.net_code == b_item.net_code,
            (PcbValue::Layer(layer), _) => {
                let pattern = b.as_string(ctx);
                ctx.env()
                    .board
                    .layer_names
                    .iter()
                    .any(|name| name == layer && matches(name, pattern))
            }
            _ => value.base_equal_to(ctx, b),
        }
    }

    fn not_equal_to(
        &self,
        value: &Value<'e, PcbBinder>,
        ctx: &Context<'e, PcbBinder>,
        b: &Value<'e, PcbBinder>,
    ) -> bool {
        match (self, b.custom()) {
            (PcbValue::Net(item), Some(PcbValue::Net(b_item))) => item.net_code != b_item.net_code,
            _ => value.base_not_equal_to(ctx, b),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Property {
    /// a reference to the item itself (empty field)
    Object,
    Type,
    NetName,
    NetClass,
    Width,
    Layer,
    Parent,
    Unknown,
}

/// `PCBEXPR_VAR_REF` (and `PCBEXPR_NETCLASS_REF`, `PCBEXPR_NETNAME_REF`, `PCBEXPR_TYPE_REF`).
struct PcbVarRef {
    /// 0 = A, 1 = B, 2 = L (the layer under test)
    item_index: usize,
    /// number of `.Parent` navigation steps
    navigation: usize,
    property: Property,
    type_: VarType,
}

impl PcbVarRef {
    /// `PCBEXPR_VAR_REF::GetObject( const LIBEVAL::CONTEXT* aCtx )`
    fn object<'e>(&self, ctx: &Context<'e, PcbBinder>) -> Option<&'e Item> {
        let env = ctx.env();
        let mut item = env.items.get(self.item_index).copied().flatten();

        for _ in 0..self.navigation {
            item = item?.parent.and_then(|index| env.board.items.get(index));
        }

        item
    }
}

impl VarRef<PcbBinder> for PcbVarRef {
    fn type_(&self) -> VarType {
        self.type_
    }

    fn value<'e>(&self, ctx: &mut Context<'e, PcbBinder>) -> Value<'e, PcbBinder> {
        if self.type_ == VarType::Null {
            return Value::make_null_value();
        }

        if self.item_index == 2 {
            let layer = ctx.env().layer;
            return Value::from_string(layer, false).with_custom(PcbValue::Layer(layer));
        }

        let Some(item) = self.object(ctx) else {
            return Value::new();
        };

        match self.property {
            Property::Type => Value::from_string(item.type_name, false),
            Property::NetName => Value::from_string("", false).with_custom(PcbValue::Net(item)),
            Property::NetClass => {
                Value::from_string("", false).with_custom(PcbValue::NetClass(item))
            }
            // "return an undefined value when the property doesn't appear on a particular
            // object"
            Property::Width => item
                .width
                .map_or_else(Value::new, |w| Value::from_double(w as f64)),
            Property::Layer => {
                let layer = item.layers[0];
                Value::from_string(layer, false).with_custom(PcbValue::Layer(layer))
            }
            Property::Parent => match item.parent.and_then(|i| ctx.env().board.items.get(i)) {
                Some(parent) => Value::from_string(parent.type_name, false),
                None => Value::new(),
            },
            Property::Object | Property::Unknown => Value::new(),
        }
    }
}

/// `existsOnLayerFunc`: deferred, with an "interpreted" branch that reports unknown layers
/// when the context has an error callback.
fn exists_on_layer(ctx: &mut Context<'_, PcbBinder>, self_: Option<&PcbVarRef>) {
    let item = self_.and_then(|vref| vref.object(ctx));
    let arg = ctx.pop();
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);

    let Some(item) = item else {
        return;
    };

    if ctx.value(arg).as_string(ctx).is_empty() {
        if ctx.has_error_callback() {
            ctx.report_error(&format!(
                "Missing layer name argument to {}.",
                "existsOnLayer()"
            ));
        }

        return;
    }

    // C++ captures [item, arg, aCtx]; here the context is handed to the closure when it runs.
    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let layer_name = ctx.value(arg).as_string(ctx);
        let board = ctx.env().board;

        let mut any_match = false;

        for name in board.layer_names {
            if matches(name, layer_name) {
                any_match = true;

                if item.layers.contains(name) {
                    return 1.0;
                }
            }
        }

        if !any_match && ctx.has_error_callback() {
            ctx.report_error(&format!("Unrecognized layer '{layer_name}'"));
        }

        0.0
    });
}

/// `memberOfGroupFunc`
fn member_of_group(ctx: &mut Context<'_, PcbBinder>, self_: Option<&PcbVarRef>) {
    let arg = ctx.pop();
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);

    if ctx.value(arg).as_string(ctx).is_empty() {
        if ctx.has_error_callback() {
            ctx.report_error(&format!(
                "Missing group name argument to {}.",
                "memberOfGroup()"
            ));
        }

        return;
    }

    let Some(item) = self_.and_then(|vref| vref.object(ctx)) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let pattern = ctx.value(arg).as_string(ctx);
        let parent = item
            .parent
            .and_then(|index| ctx.env().board.items.get(index));
        let group = item.group.or(parent.and_then(|p| p.group));

        if group.is_some_and(|name| matches(name, pattern)) {
            1.0
        } else {
            0.0
        }
    });
}

/// `isCoupledDiffPairFunc` reads both items and the constraint from the context.
fn same_net_pair(ctx: &mut Context<'_, PcbBinder>, _self: Option<&PcbVarRef>) {
    let [a, b] = ctx.env().items;
    let constraint = ctx.env().constraint;
    let result = ctx.alloc_value();

    let same = matches!((a, b), (Some(a), Some(b)) if a.net_code == b.net_code);
    ctx.value_mut(result)
        .set_double(if same && constraint >= 0 { 1.0 } else { 0.0 });
    ctx.push(result);
}

/// `PCBEXPR_UCODE`: the binder also records how the expression uses the items.
#[derive(Default)]
struct PcbBinder {
    requires_pair_items: bool,
    references_item_b: bool,
    has_geometry_dependent_functions: bool,
}

impl UcodeBinder for PcbBinder {
    type Env<'e> = PcbEnv<'e>;
    type Custom<'e> = PcbValue<'e>;
    type VarRef = PcbVarRef;

    /// `PCBEXPR_UCODE::CreateVarRef`
    fn create_var_ref(&mut self, var: &str, field: &str) -> Option<PcbVarRef> {
        if var.eq_ignore_ascii_case("null") {
            return Some(PcbVarRef {
                item_index: 0,
                navigation: 0,
                property: Property::Object,
                type_: VarType::Null,
            });
        }

        // The receiver may be a navigation chain such as "A.Parent".
        let mut tokens = var.split('.');
        let base_var = tokens.next().unwrap_or("");
        let mut navigation = 0;

        for token in tokens {
            if token.eq_ignore_ascii_case("Parent") {
                navigation += 1;
            } else {
                return None;
            }
        }

        if navigation > 0 && base_var != "A" && base_var != "B" {
            return None;
        }

        let item_index = match base_var {
            "A" | "AB" => 0,
            "B" => 1,
            "L" => 2,
            _ => return None,
        };

        if !field.is_empty() && (base_var == "AB" || (base_var == "L" && field != "Layer")) {
            return None;
        }

        if base_var == "B" || base_var == "AB" {
            self.requires_pair_items = true;
        }

        if base_var == "B" {
            self.references_item_b = true;
        }

        let (property, type_) = match field.replace('_', " ").to_lowercase().as_str() {
            "" => (Property::Object, VarType::Undefined),
            "type" => (Property::Type, VarType::String),
            "netname" => (Property::NetName, VarType::String),
            "netclass" => (Property::NetClass, VarType::String),
            "width" => (Property::Width, VarType::Numeric),
            "layer" => (Property::Layer, VarType::String),
            "parent" => (Property::Parent, VarType::String),
            _ => (Property::Unknown, VarType::ParseError),
        };

        Some(PcbVarRef {
            item_index,
            navigation,
            property,
            type_,
        })
    }

    /// `PCBEXPR_UCODE::CreateFuncCall`
    fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<Self>> {
        match name.to_lowercase().as_str() {
            "existsonlayer" => Some(FuncCallRef::new(exists_on_layer)),
            "memberofgroup" => Some(FuncCallRef::new(member_of_group)),
            "samenetpair" => {
                self.has_geometry_dependent_functions = true;
                Some(FuncCallRef::new(same_net_pair))
            }
            // closures work as well
            "isvia" => Some(FuncCallRef::new(
                |ctx: &mut Context<'_, PcbBinder>, self_: Option<&PcbVarRef>| {
                    let is_via = self_
                        .and_then(|vref| vref.object(ctx))
                        .is_some_and(|item| item.type_name == "Via");
                    let result = ctx.alloc_value();
                    ctx.value_mut(result)
                        .set_double(if is_via { 1.0 } else { 0.0 });
                    ctx.push(result);
                },
            )),
            _ => None,
        }
    }
}

/* --------------------------------------------------------------------------------------------
 * DRC_RULE_CONDITION
 */

struct RuleCondition {
    expression: String,
    ucode: Option<Ucode<PcbBinder>>,
}

impl RuleCondition {
    fn new(expression: &str) -> Self {
        RuleCondition {
            expression: expression.to_string(),
            ucode: None,
        }
    }

    /// `DRC_RULE_CONDITION::Compile( REPORTER* aReporter, int aSourceLine, int aSourceOffset )`
    fn compile(&mut self, reporter: &mut Vec<String>) -> bool {
        let mut compiler = Compiler::with_unit_resolver(Box::new(PcbExprUnitResolver::new(false)));

        compiler.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
            let (first, rest) = message.split_once('|').unwrap_or((message, ""));
            reporter.push(format!("ERROR: {offset}:{first}{rest}"));
        })));

        let mut ucode = Ucode::new(PcbBinder::default());

        // The pre-flight context has no items; the board is only needed while it lives.
        let empty = Board {
            items: Vec::new(),
            layer_names: &[],
        };
        let mut preflight = Context::new(PcbEnv {
            board: &empty,
            items: [None, None],
            layer: "",
            constraint: 0,
        });

        let ok = compiler.compile(&self.expression, &mut ucode, &mut preflight);

        self.ucode = Some(ucode);
        ok
    }

    /// `DRC_RULE_CONDITION::EvaluateFor( aItemA, aItemB, aConstraint, aLayer, aReporter )`
    fn evaluate_for(
        &self,
        board: &Board,
        a: &Item,
        b: Option<&Item>,
        constraint: i32,
        layer: &'static str,
        reporter: Option<&mut Vec<String>>,
    ) -> bool {
        if self.expression.is_empty() {
            return true;
        }

        let Some(ucode) = &self.ucode else {
            if let Some(reporter) = reporter {
                reporter.push("ERROR in expression.".to_string());
            }

            return false;
        };

        let mut ctx = Context::new(PcbEnv {
            board,
            items: [Some(a), b],
            layer,
            constraint,
        });

        if let Some(reporter) = reporter {
            ctx.set_error_callback(Some(Box::new(|message: &str, _offset: i32| {
                reporter.push(format!("ERROR: {message}"));
            })));
        }

        let result = ucode.run(&mut ctx);

        if ctx.value(result).as_double(&ctx) != 0.0 {
            return true;
        } else if b.is_some() {
            // Conditions are commutative
            ctx.env_mut().items = [b, Some(a)];

            let result = ucode.run(&mut ctx);

            if ctx.value(result).as_double(&ctx) != 0.0 {
                return true;
            }
        }

        false
    }
}

/* --------------------------------------------------------------------------------------------
 * Tests
 */

fn condition(expression: &str) -> RuleCondition {
    let mut reporter = Vec::new();
    let mut condition = RuleCondition::new(expression);

    assert!(
        condition.compile(&mut reporter),
        "{expression}: {reporter:?}"
    );
    assert!(reporter.is_empty(), "{expression}: {reporter:?}");
    condition
}

#[test]
fn conditions_on_items() {
    let board = board();
    let [footprint, track_vin, track_gnd, track_back, via, pad] = &board.items[..] else {
        unreachable!()
    };

    let eval = |expression: &str, a: &Item, b: Option<&Item>| {
        condition(expression).evaluate_for(&board, a, b, 0, "F.Cu", None)
    };

    assert!(eval("A.NetClass == 'HV_LINE'", track_vin, None));
    assert!(eval("A.NetClass == 'hv_*'", track_vin, None));
    assert!(!eval("A.NetClass == 'HV_LINE'", track_gnd, None));
    assert!(eval(
        "A.Type == 'Track' && A.Width > 0.3mm",
        track_gnd,
        None
    ));
    assert!(!eval(
        "A.Type == 'Track' && A.Width > 0.3mm",
        track_vin,
        None
    ));

    // net values compare by net code
    assert!(eval("A.NetName == B.NetName", track_vin, Some(track_back)));
    assert!(!eval("A.NetName == B.NetName", track_vin, Some(track_gnd)));
    assert!(eval("A.NetName != B.NetName", track_vin, Some(track_gnd)));
    assert!(eval("A.NetName == 'VIN'", track_vin, None));

    // conditions are commutative: the pair is tried in both orders
    assert!(eval(
        "A.NetClass == 'HV_LINE' && B.Type == 'Via'",
        via,
        Some(track_vin)
    ));
    assert!(eval(
        "A.NetClass == 'HV_LINE' && B.Type == 'Via'",
        track_vin,
        Some(via)
    ));
    assert!(!eval(
        "A.NetClass == 'HV_LINE' && B.Type == 'Via'",
        track_vin,
        Some(track_gnd)
    ));

    // a property an item does not have is undefined: neither equal nor unequal
    assert!(!eval("A.Width == 0mm", via, None));
    assert!(!eval("A.Width != 0mm", via, None));

    // layer values, the layer under test, functions with literal and computed arguments
    assert!(eval("A.Layer == 'F.Cu'", track_vin, None));
    assert!(eval("A.Layer == '*.Cu'", track_back, None));
    assert!(!eval("A.Layer == 'F.*'", track_back, None));
    assert!(eval("L == 'F.Cu'", track_vin, None));
    assert!(eval("A.existsOnLayer('B.Cu')", via, None));
    assert!(!eval("A.existsOnLayer('B.Cu')", track_vin, None));
    assert!(eval("A.existsOnLayer(B.Layer)", via, Some(track_back)));
    assert!(eval("A.isVia() && !B.isVia()", via, Some(track_vin)));
    assert!(eval("AB.sameNetPair()", track_vin, Some(pad)));
    assert!(!eval("AB.sameNetPair()", track_vin, Some(via)));

    // navigation
    assert!(eval("A.Parent == 'Footprint'", pad, None));
    assert!(eval("A.Parent.Type == 'Footprint'", pad, None));
    assert!(eval("A.Parent.memberOfGroup('pow*')", pad, None));
    assert!(eval("A.memberOfGroup('power')", pad, None));
    assert!(eval("A.memberOfGroup('power')", footprint, None));
    assert!(!eval("A.memberOfGroup('power')", track_vin, None));
    assert!(!eval("A.Parent.Type == 'Footprint'", track_vin, None));
}

#[test]
fn binder_records_usage() {
    let flags = |expression: &str| {
        let condition = condition(expression);
        let binder = condition.ucode.as_ref().unwrap().binder();
        (
            binder.requires_pair_items,
            binder.references_item_b,
            binder.has_geometry_dependent_functions,
        )
    };

    assert_eq!(flags("A.Width == 1mm"), (false, false, false));
    assert_eq!(flags("B.Width == 1mm"), (true, true, false));
    assert_eq!(flags("AB.sameNetPair()"), (true, false, true));
    assert_eq!(flags("A.Type == 'B.Width'"), (false, false, false));
}

#[test]
fn compile_errors_are_reported() {
    let compile = |expression: &str| {
        let mut reporter = Vec::new();
        let ok = RuleCondition::new(expression).compile(&mut reporter);
        (ok, reporter)
    };

    assert_eq!(
        compile("A.Width >"),
        (false, vec!["ERROR: 9:Syntax error".to_string()])
    );
    assert_eq!(
        compile("A.Bogus == 1mm"),
        (
            true,
            vec!["ERROR: 2:Unrecognized property 'Bogus'".to_string()]
        )
    );
    assert_eq!(
        compile("C.Width == 1mm"),
        (true, vec!["ERROR: 0:Unrecognized item 'C'".to_string()])
    );
    assert_eq!(
        compile("AB.Width == A.Width"),
        (true, vec!["ERROR: 0:Unrecognized item 'AB'".to_string()])
    );
    assert_eq!(
        compile("A.Width == 1"),
        (
            true,
            vec![
                "ERROR: 12:Missing units for '1' (must be mm, in, mil, deg, fs, or ps)".to_string()
            ]
        )
    );
    assert_eq!(compile("A.existsOnLayer('')"), (true, vec![]));
    assert_eq!(
        compile("A.memberOfGroup('')"),
        (
            true,
            vec!["ERROR: 17:Missing group name argument to memberOfGroup().".to_string()]
        )
    );
}

/// Run-time errors go to the reporter only when one is given ("interpreted" evaluation).
#[test]
fn runtime_errors_are_reported() {
    let board = board();
    let track = &board.items[1];
    let condition = condition("A.existsOnLayer('Q.Cu') || A.Type == 'Track'");

    assert!(condition.evaluate_for(&board, track, None, 0, "F.Cu", None));

    let mut reporter = Vec::new();
    assert!(condition.evaluate_for(&board, track, None, 0, "F.Cu", Some(&mut reporter)));
    assert_eq!(
        reporter,
        vec!["ERROR: Unrecognized layer 'Q.Cu'".to_string()]
    );

    // An uncompiled condition.
    let mut reporter = Vec::new();
    let uncompiled = RuleCondition::new("A.Type == 'Track'");
    assert!(!uncompiled.evaluate_for(&board, track, None, 0, "F.Cu", Some(&mut reporter)));
    assert_eq!(reporter, vec!["ERROR in expression.".to_string()]);
}

/// The same compiled condition serves any number of boards and borrows.
#[test]
fn compiled_code_outlives_boards() {
    let condition = condition("A.NetClass == 'HV_LINE' && L == 'F.Cu'");

    for _ in 0..3 {
        let board = board();
        assert!(condition.evaluate_for(&board, &board.items[1], None, 0, "F.Cu", None));
        assert!(!condition.evaluate_for(&board, &board.items[1], None, 0, "B.Cu", None));
        assert!(!condition.evaluate_for(&board, &board.items[2], None, 0, "F.Cu", None));
    }
}

/// A reusable context, as `DRC_RULE_CONDITION::EvaluateFor` keeps per thread.
#[test]
fn reusable_context() {
    let board = board();
    let condition = condition("A.Type == 'Track' && A.Width >= 0.25mm");
    let ucode = condition.ucode.as_ref().unwrap();

    // (The binder type cannot be inferred from the environment alone.)
    let mut ctx: Context<'_, PcbBinder> = Context::new(PcbEnv {
        board: &board,
        items: [None, None],
        layer: "F.Cu",
        constraint: 0,
    });
    let mut matching = 0;

    for item in &board.items {
        ctx.reset();
        ctx.env_mut().items = [Some(item), None];

        let result = ucode.run(&mut ctx);

        if ctx.value(result).as_double(&ctx) != 0.0 {
            matching += 1;
        }
    }

    assert_eq!(matching, 2);
}
