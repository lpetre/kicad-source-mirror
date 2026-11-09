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

//! Shared test support:
//!
//! * [`PcbExprUnitResolver`]: the unit resolver pcbnew gives the compiler
//!   (`PCBEXPR_UNIT_RESOLVER` in `pcbnew/pcbexpr_evaluator.cpp`; the real port will live in
//!   `kicad-drc`),
//! * [`TestBinder`]: a small deterministic binding of variables, properties and functions.
//!   It is the Rust twin of `TEST_UCODE` in `tests/oracle/oracle_libeval.cpp`, the C++
//!   reference driver that produced `tests/golden/libeval_golden.txt`; the two must be kept
//!   in sync,
//! * [`run_case`]: runs one line of a case file and renders every observable in the format
//!   the C++ driver prints.

#![allow(dead_code)]

use std::cell::RefCell;

use kicad_libeval::eda_units::{
    EdaDataType, PCB_IU_SCALE, double_value_from_string, double_value_from_string_with_units,
};
use kicad_libeval::{
    Compiler, Context, CustomValue, EdaUnits, FuncCallRef, Ucode, UcodeBinder, UnitResolver, Value,
    VarRef, VarType,
};

/* --------------------------------------------------------------------------------------------
 * Unit resolvers
 */

/// Test copy of `PCBEXPR_UNIT_RESOLVER` (`pcbnew/pcbexpr_evaluator.cpp`).
pub struct PcbExprUnitResolver {
    // This little hack is required because the first several versions which supported
    // track angle constraints required their values to be specified with no units --
    // though they were interpreted as degrees.
    implied_degrees: bool,
}

impl PcbExprUnitResolver {
    pub fn new(implied_degrees: bool) -> Self {
        PcbExprUnitResolver { implied_degrees }
    }
}

impl UnitResolver for PcbExprUnitResolver {
    fn supported_units(&self) -> &[&str] {
        &["mil", "mm", "in", "deg", "fs", "ps"]
    }

    fn supported_units_types(&self) -> &[EdaUnits] {
        &[
            EdaUnits::Mils,
            EdaUnits::Mm,
            EdaUnits::Inch,
            EdaUnits::Degrees,
            EdaUnits::Fs,
            EdaUnits::Ps,
        ]
    }

    fn supported_units_message(&self) -> String {
        if self.implied_degrees {
            return String::new();
        }

        "must be mm, in, mil, deg, fs, or ps".to_string()
    }

    fn convert(&self, string: &str, unit_id: i32) -> f64 {
        // double v = wxAtof( aString );  (the text is a number token: digits and at most one
        // '.', for which the unit-less conversion gives the same result as atof)
        let v = double_value_from_string(string);
        let iu = |units: EdaUnits| {
            double_value_from_string_with_units(&PCB_IU_SCALE, units, string, EdaDataType::Distance)
        };

        match unit_id {
            0 => iu(EdaUnits::Mils),
            1 => iu(EdaUnits::Mm),
            2 => iu(EdaUnits::Inch),
            3 => v,
            4 => iu(EdaUnits::Fs),
            5 => iu(EdaUnits::Ps),
            _ => v,
        }
    }
}

/// The unit resolver named in a case file: `pcb`, `pcbdeg` or anything else for the default
/// `LIBEVAL::UNIT_RESOLVER`.
pub fn make_compiler<'a>(resolver: &str) -> Compiler<'a> {
    match resolver {
        "pcb" => Compiler::with_unit_resolver(Box::new(PcbExprUnitResolver::new(false))),
        "pcbdeg" => Compiler::with_unit_resolver(Box::new(PcbExprUnitResolver::new(true))),
        _ => Compiler::new(),
    }
}

/* --------------------------------------------------------------------------------------------
 * Test binding (twin of TEST_UCODE / TEST_VAR_REF / TEST_*_VALUE in oracle_libeval.cpp)
 */

/// `TEST_CONTEXT`: the log records side effects of deferred evaluations.
#[derive(Default)]
pub struct TestEnv {
    pub log: RefCell<Vec<String>>,
}

/// `TEST_CODE_VALUE` and `TEST_ALIAS_VALUE`.
pub enum TestCustom {
    /// A value comparing by code like `PCBEXPR_NET_VALUE`, with a lazily computed string like
    /// `PCBEXPR_NETCLASS_VALUE`.
    Code { code: i32, name: &'static str },
    /// A value with only `EqualTo` overridden, like `PCBEXPR_PINTYPE_VALUE`.
    Alias { alias: &'static str },
}

fn is_same_as_no_case(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl<'e> CustomValue<'e, TestBinder> for TestCustom {
    fn as_string(
        &self,
        _value: &Value<'e, TestBinder>,
        _ctx: &Context<'e, TestBinder>,
    ) -> Option<String> {
        match self {
            TestCustom::Code { name, .. } => Some(name.to_string()),
            TestCustom::Alias { .. } => None,
        }
    }

    fn equal_to(
        &self,
        value: &Value<'e, TestBinder>,
        ctx: &Context<'e, TestBinder>,
        b: &Value<'e, TestBinder>,
    ) -> bool {
        match self {
            TestCustom::Code { code, .. } => {
                if let Some(TestCustom::Code { code: b_code, .. }) = b.custom() {
                    code == b_code
                } else {
                    value.base_equal_to(ctx, b)
                }
            }
            TestCustom::Alias { alias } => {
                let this_str = value.as_string(ctx);
                let other_str = b.as_string(ctx);

                if is_same_as_no_case(this_str, other_str) {
                    return true;
                }

                is_same_as_no_case(alias, other_str)
            }
        }
    }

    fn not_equal_to(
        &self,
        value: &Value<'e, TestBinder>,
        ctx: &Context<'e, TestBinder>,
        b: &Value<'e, TestBinder>,
    ) -> bool {
        match self {
            TestCustom::Code { code, .. } => {
                if let Some(TestCustom::Code { code: b_code, .. }) = b.custom() {
                    code != b_code
                } else {
                    value.base_not_equal_to(ctx, b)
                }
            }
            TestCustom::Alias { .. } => value.base_not_equal_to(ctx, b),
        }
    }
}

/// `enum TEST_FIELD`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestField {
    Object,
    NullVar,
    Num,
    Frac,
    Str,
    NumStr,
    Empty,
    Undef,
    Null,
    Mm,
    Wild,
    Code,
    Alias,
    Bad,
}

/// `TEST_VAR_REF`
#[derive(Clone, Debug)]
pub struct TestVarRef {
    pub item: usize,
    pub nav: i32,
    pub field: TestField,
    pub type_: VarType,
}

impl VarRef<TestBinder> for TestVarRef {
    fn type_(&self) -> VarType {
        self.type_
    }

    fn value<'e>(&self, _ctx: &mut Context<'e, TestBinder>) -> Value<'e, TestBinder> {
        const STRS: [&str; 3] = ["hello", "World", "F.Cu"];
        const NUMSTRS: [&str; 3] = ["42", "4k7", "1.5mm"];
        const FRACS: [f64; 3] = [2.5, 0.1, -0.75];

        match self.field {
            TestField::NullVar => Value::make_null_value(),
            TestField::Object => Value::new(),
            TestField::Num => {
                Value::from_double(10.0 * (self.item as f64 + 1.0) + 100.0 * self.nav as f64)
            }
            TestField::Frac => Value::from_double(FRACS[self.item]),
            TestField::Str => Value::from_string(STRS[self.item], false),
            TestField::NumStr => Value::from_string(NUMSTRS[self.item], false),
            TestField::Empty => Value::from_string("", false),
            TestField::Undef => Value::new(),
            TestField::Null => Value::make_null_value(),
            TestField::Mm => {
                let mut v = Value::from_double(1e6 * (self.item as f64 + 1.0));
                v.set_units(EdaUnits::Mm);
                v
            }
            TestField::Wild => Value::from_string("h*", true),
            TestField::Code => Value::from_string("", false).with_custom(TestCustom::Code {
                code: if self.item == 2 { 9 } else { 7 },
                name: match self.item {
                    0 => "seven",
                    1 => "SEVEN-B",
                    _ => "nine",
                },
            }),
            TestField::Alias => Value::from_string(
                if self.item == 0 {
                    "power_in"
                } else {
                    "passive"
                },
                false,
            )
            .with_custom(TestCustom::Alias {
                alias: if self.item == 0 {
                    "Power input"
                } else {
                    "Passive"
                },
            }),
            TestField::Bad => Value::new(),
        }
    }
}

type Ctx<'c, 'e> = &'c mut Context<'e, TestBinder>;

fn fn_one(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let result = ctx.alloc_value();
    ctx.value_mut(result).set_double(1.0);
    ctx.push(result);
}

fn fn_is_foo(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let arg = ctx.pop();
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);

    if ctx.value(arg).as_string(ctx).is_empty() {
        if ctx.has_error_callback() {
            ctx.report_error(&format!("Missing argument to {}.", "isFoo()"));
        }

        return;
    }

    if ctx.value(arg).as_string(ctx) == "foo" {
        ctx.value_mut(result).set_double(1.0);
    }
}

fn fn_add(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let b = ctx.pop();
    let a = ctx.pop();
    let result = ctx.alloc_value();

    let sum = ctx.value(a).as_double(ctx) + ctx.value(b).as_double(ctx);
    ctx.value_mut(result).set_double(sum);
    ctx.push(result);
}

fn fn_cat(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let b = ctx.pop();
    let a = ctx.pop();
    let result = ctx.alloc_value();

    let cat = format!(
        "{}{}",
        ctx.value(a).as_string(ctx),
        ctx.value(b).as_string(ctx)
    );
    ctx.value_mut(result).set_string(cat);
    ctx.push(result);
}

/// Returns the navigation depth of the receiver plus 1 (plus 10 times the item index), or 0
/// without receiver.
fn fn_self(ctx: Ctx<'_, '_>, self_: Option<&TestVarRef>) {
    let result = ctx.alloc_value();

    let value = match self_ {
        Some(vref) => 1.0 + vref.nav as f64 + 10.0 * vref.item as f64,
        None => 0.0,
    };

    ctx.value_mut(result).set_double(value);
    ctx.push(result);
}

fn fn_lazy(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);

    ctx.value_mut(result).set_deferred_eval_double(|ctx| {
        ctx.env()
            .log
            .borrow_mut()
            .push("lazy evaluated".to_string());

        if ctx.has_error_callback() {
            ctx.report_error("lazy() evaluated with error callback");
        }

        5.0
    });
}

fn fn_lazy_str(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let arg = ctx.pop();
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_string("");
    ctx.push(result);

    ctx.value_mut(result).set_deferred_eval_string(move |ctx| {
        ctx.env()
            .log
            .borrow_mut()
            .push("lazyStr evaluated".to_string());
        format!("<{}>", ctx.value(arg).as_string(ctx))
    });
}

fn fn_no_push(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    ctx.pop();
}

fn fn_push2(ctx: Ctx<'_, '_>, _self: Option<&TestVarRef>) {
    let a = ctx.alloc_value();
    ctx.value_mut(a).set_double(1.0);
    ctx.push(a);

    let b = ctx.alloc_value();
    ctx.value_mut(b).set_double(2.0);
    ctx.push(b);
}

/// `TEST_UCODE`: the log records every `CreateVarRef` / `CreateFuncCall` call.
#[derive(Default)]
pub struct TestBinder {
    pub log: Vec<String>,
}

impl UcodeBinder for TestBinder {
    type Env<'e> = TestEnv;
    type Custom<'e> = TestCustom;
    type VarRef = TestVarRef;

    fn create_var_ref(&mut self, var: &str, field: &str) -> Option<TestVarRef> {
        self.log.push(format!("var:{var}:{field}"));

        let vref = |item, nav, field, type_| {
            Some(TestVarRef {
                item,
                nav,
                field,
                type_,
            })
        };

        if var.eq_ignore_ascii_case("null") {
            return vref(0, 0, TestField::NullVar, VarType::Null);
        }

        let mut base_var = var;
        let mut nav = 0;

        if var.contains('.') {
            let mut tokens = var.split('.');
            base_var = tokens.next().unwrap_or("");

            for token in tokens {
                if token.eq_ignore_ascii_case("Parent") {
                    nav += 1;
                } else {
                    return None;
                }
            }

            if base_var != "A" && base_var != "B" {
                return None;
            }
        }

        let item = match base_var {
            "A" => 0,
            "B" => 1,
            "L" => 2,
            _ => return None,
        };

        if field.is_empty() {
            return vref(item, nav, TestField::Object, VarType::Undefined);
        }

        match field.to_lowercase().as_str() {
            "num" => vref(item, nav, TestField::Num, VarType::Numeric),
            "frac" => vref(item, nav, TestField::Frac, VarType::NumericDouble),
            "str" => vref(item, nav, TestField::Str, VarType::String),
            "numstr" => vref(item, nav, TestField::NumStr, VarType::String),
            "empty" => vref(item, nav, TestField::Empty, VarType::String),
            "undef" => vref(item, nav, TestField::Undef, VarType::Numeric),
            "null" => vref(item, nav, TestField::Null, VarType::Numeric),
            "mm" => vref(item, nav, TestField::Mm, VarType::Numeric),
            "wild" => vref(item, nav, TestField::Wild, VarType::String),
            "code" => vref(item, nav, TestField::Code, VarType::String),
            "alias" => vref(item, nav, TestField::Alias, VarType::String),
            _ => vref(item, nav, TestField::Bad, VarType::ParseError),
        }
    }

    fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<Self>> {
        self.log.push(format!("func:{name}"));

        match name.to_lowercase().as_str() {
            "one" => Some(FuncCallRef::new(fn_one)),
            "isfoo" => Some(FuncCallRef::new(fn_is_foo)),
            "add" => Some(FuncCallRef::new(fn_add)),
            "cat" => Some(FuncCallRef::new(fn_cat)),
            "self" => Some(FuncCallRef::new(fn_self)),
            "lazy" => Some(FuncCallRef::new(fn_lazy)),
            "lazystr" => Some(FuncCallRef::new(fn_lazy_str)),
            "nopush" => Some(FuncCallRef::new(fn_no_push)),
            "push2" => Some(FuncCallRef::new(fn_push2)),
            _ => None,
        }
    }
}

/* --------------------------------------------------------------------------------------------
 * Output helpers (same format as oracle_libeval.cpp)
 */

pub fn escape(s: &str) -> String {
    let mut out = String::new();

    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }

    out
}

pub fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

pub fn type_name(type_: VarType) -> &'static str {
    match type_ {
        VarType::String => "STRING",
        VarType::Numeric => "NUMERIC",
        VarType::NumericDouble => "NUMERIC_DOUBLE",
        VarType::Undefined => "UNDEFINED",
        VarType::ParseError => "PARSE_ERROR",
        VarType::Null => "NULL",
    }
}

pub fn bits(value: f64) -> String {
    // All NaNs are equal for the purpose of the comparison.
    if value.is_nan() {
        return "nan".to_string();
    }

    format!("{:016x}", value.to_bits())
}

/// Replace the addresses printed by `Uop::format()` ("PUSH VAR [0x55...]") by a constant.
pub fn normalize_dump(dump: &str) -> String {
    let mut out = String::new();
    let mut rest = dump;

    while let Some(start) = rest.find("PUSH VAR [") {
        let after = &rest[start + 10..];
        let end = after.find(']').unwrap_or(after.len());
        let ptr = &after[..end];

        out.push_str(&rest[..start]);
        out.push_str("PUSH VAR [");
        out.push_str(if ptr == "(nil)" || ptr == "0" || ptr == "0x0" {
            "null"
        } else {
            "ref"
        });
        out.push(']');
        rest = &after[(end + 1).min(after.len())..];
    }

    out.push_str(rest);
    out
}

fn run_once(code: &Ucode<TestBinder>, with_callback: bool, tag: &str, out: &mut String) {
    let errors: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());

    if with_callback {
        ctx.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
            errors.borrow_mut().push(format!("{offset}\t{message}"));
        })));
    }

    let result = code.run(&mut ctx);
    let type_ = ctx.value(result).type_();
    let dbl = ctx.value(result).as_double(&ctx);
    let str_ = ctx.value(result).as_string(&ctx).to_string();
    let units = ctx.value(result).units();

    out.push_str(&format!(
        "{}\t{}\t{}\t{}\t{}\n",
        tag,
        type_name(type_),
        units.as_i32(),
        bits(dbl),
        escape(&str_)
    ));

    for err in errors.borrow().iter() {
        out.push_str(&format!("X\t{}\n", escape(err)));
    }

    for entry in ctx.env().log.borrow().iter() {
        out.push_str(&format!("L\t{}\n", escape(entry)));
    }
}

fn compile_one(
    compiler: &mut Compiler<'_>,
    errors: &RefCell<Vec<String>>,
    expr: &str,
    run: bool,
    out: &mut String,
) {
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    errors.borrow_mut().clear();

    let ok = compiler.compile(expr, &mut ucode, &mut preflight);

    let status = compiler.error();

    if compiler.is_error_pending() {
        out.push_str(&format!(
            "C\t{}\t1\t{}\t{}\t{}\n",
            ok as i32,
            status.stage as i32,
            status.src_pos,
            escape(&status.message)
        ));
    } else {
        out.push_str(&format!("C\t{}\t0\n", ok as i32));
    }

    for err in errors.borrow().iter() {
        out.push_str(&format!("E\t{}\n", escape(err)));
    }

    for entry in &ucode.binder().log {
        out.push_str(&format!("B\t{}\n", escape(entry)));
    }

    for entry in preflight.env().log.borrow().iter() {
        out.push_str(&format!("P\t{}\n", escape(entry)));
    }

    out.push_str(&format!("U\t{}\n", escape(&normalize_dump(&ucode.dump()))));

    if ok && run {
        run_once(&ucode, true, "R", out);
        run_once(&ucode, false, "N", out);
    }
}

/// Run one line of a case file (`<resolver> TAB <expr> [TAB <expr> ...]`; several
/// expressions are compiled in sequence by the same compiler) and render the result exactly
/// like `oracle_libeval`, without the framing `EXPR` / `END` lines.
pub fn run_case(line: &str) -> String {
    let mut parts = line.split('\t');
    let resolver = parts.next().unwrap_or("");
    let exprs: Vec<&str> = parts.collect();

    let errors: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let mut out = String::new();

    {
        let mut compiler = make_compiler(resolver);

        compiler.set_error_callback(Some(Box::new(|message: &str, offset: i32| {
            errors.borrow_mut().push(format!("{offset}\t{message}"));
        })));

        for expr in &exprs {
            compile_one(
                &mut compiler,
                &errors,
                &unescape(expr),
                exprs.len() == 1,
                &mut out,
            );
        }
    }

    out
}

/// FNV-1a, used to store the expected output of bulk cases compactly.
pub fn fnv1a32(text: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;

    for byte in text.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }

    hash
}
