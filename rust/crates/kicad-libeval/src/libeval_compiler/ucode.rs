// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007 Michael Geselbracht, mgeselbracht3@gmail.com
// Copyright (C) 2017 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `LIBEVAL::VAR_REF`, `LIBEVAL::FUNC_CALL_REF`, `LIBEVAL::UOP` and `LIBEVAL::UCODE`
//! (the stack virtual machine) from `include/libeval_compiler/libeval_compiler.h` and
//! `common/libeval_compiler/libeval_compiler.cpp`.

use std::fmt;
use std::sync::Arc;

use super::context::{Context, ValueId};
use super::value::{CustomValue, NoCustomValue, Value};
use super::{
    TR_OP_ADD, TR_OP_BINARY_MASK, TR_OP_BOOL_AND, TR_OP_BOOL_NOT, TR_OP_BOOL_OR, TR_OP_DIV,
    TR_OP_EQUAL, TR_OP_FUNC_CALL, TR_OP_GREATER, TR_OP_GREATER_EQUAL, TR_OP_JNZ, TR_OP_JZ,
    TR_OP_LESS, TR_OP_LESS_EQUAL, TR_OP_METHOD_CALL, TR_OP_MUL, TR_OP_NOT_EQUAL, TR_OP_SUB,
    TR_OP_UNARY_MASK, TR_UOP_PUSH_VALUE, TR_UOP_PUSH_VAR, VarType,
};
use crate::crt;
use crate::eda_units::EdaUnits;
use crate::string_utils::quacks_like_a_duck;

/// Port of `class LIBEVAL::VAR_REF`: a compiled reference to a variable or to a property of
/// a variable (`A`, `A.Width`, `A.Parent.Reference`...).
pub trait VarRef<B: UcodeBinder> {
    /// Port of `VAR_REF::GetType()`.  A reference of type [`VarType::ParseError`] makes the
    /// compiler report "Unrecognized property".
    fn type_(&self) -> VarType;

    /// Port of `VAR_REF::GetValue( CONTEXT* aCtx )`: the value of the variable for the items
    /// the context currently holds.  The returned value is handed to the context (C++
    /// `CONTEXT::StoreValue`).
    fn value<'e>(&self, ctx: &mut Context<'e, B>) -> Value<'e, B>;
}

/// The [`UcodeBinder::VarRef`] type of binders without variables: it has no instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoVarRef {}

impl<B: UcodeBinder> VarRef<B> for NoVarRef {
    fn type_(&self) -> VarType {
        match *self {}
    }

    fn value<'e>(&self, _ctx: &mut Context<'e, B>) -> Value<'e, B> {
        match *self {}
    }
}

/// The signature of the callables wrapped by [`FuncCallRef`].
type FuncCallFn<B> = dyn for<'c, 'e> Fn(&'c mut Context<'e, B>, Option<&'c <B as UcodeBinder>::VarRef>)
    + Send
    + Sync;

/// Port of `LIBEVAL::FUNC_CALL_REF` (`std::function<void( CONTEXT*, void* )>`): a function
/// callable from expressions as `receiver.name( args )`.
///
/// When called, the function must pop its arguments from the context's stack and push
/// exactly one result.  The second parameter is the receiver (C++: `void* self`, which every
/// implementation casts back to its `VAR_REF` subclass); it is `None` when the receiver is
/// not a recognised item.
///
/// Argument order: at run time the *first* call argument is on top of the stack (it is
/// popped first).  During the compile-time pre-flight call the literal arguments are pushed
/// in source order, so there the *last* argument is popped first.  This asymmetry is the C++
/// behaviour.
pub struct FuncCallRef<B: UcodeBinder>(Arc<FuncCallFn<B>>);

impl<B: UcodeBinder> FuncCallRef<B> {
    /// Wrap a function or closure.
    pub fn new<F>(func: F) -> Self
    where
        F: for<'c, 'e> Fn(&'c mut Context<'e, B>, Option<&'c B::VarRef>) + Send + Sync + 'static,
    {
        FuncCallRef(Arc::new(func))
    }

    /// Invoke the function (C++: `m_func( ctx, m_ref.get() )`).
    pub fn call<'e>(&self, ctx: &mut Context<'e, B>, self_: Option<&B::VarRef>) {
        (self.0)(ctx, self_)
    }
}

impl<B: UcodeBinder> Clone for FuncCallRef<B> {
    fn clone(&self) -> Self {
        FuncCallRef(Arc::clone(&self.0))
    }
}

impl<B: UcodeBinder> fmt::Debug for FuncCallRef<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FuncCallRef(..)")
    }
}

/// The client side of LIBEVAL: everything pcbnew's `PCBEXPR_UCODE`, `PCBEXPR_VAR_REF`,
/// `PCBEXPR_CONTEXT` and `PCBEXPR_*_VALUE` classes provide by subclassing.
///
/// A binder value is stored inside the [`Ucode`] it compiles for (like the data members of a
/// `UCODE` subclass), so `create_var_ref` / `create_func_call` can record facts about the
/// expression (pcbnew: "references item B", "has geometry dependent functions") that the
/// caller reads back through [`Ucode::binder`].
///
/// The provided methods are the base class behaviour of `LIBEVAL::UCODE`: nothing is
/// recognised.
pub trait UcodeBinder: Sized + 'static {
    /// The client data of an evaluation [`Context`] (the members of a C++ `CONTEXT`
    /// subclass); `'e` is the lifetime of whatever it borrows.
    type Env<'e>;

    /// Payload of specialised values (the C++ subclasses of `VALUE`); use [`NoCustomValue`]
    /// if there are none.
    type Custom<'e>: CustomValue<'e, Self>;

    /// The variable reference type (the C++ subclasses of `VAR_REF`); use [`NoVarRef`] if
    /// the binding has no variables.
    type VarRef: VarRef<Self>;

    /// Port of the virtual `UCODE::CreateVarRef( const wxString& var, const wxString& field )`.
    ///
    /// `var` is the item name (a dotted chain such as `A.Parent` for navigation), `field`
    /// the property name or an empty string for a reference to the item itself.  `None`
    /// (C++ `nullptr`) makes the compiler report "Unrecognized item".
    fn create_var_ref(&mut self, _var: &str, _field: &str) -> Option<Self::VarRef> {
        None
    }

    /// Port of the virtual `UCODE::CreateFuncCall( const wxString& name )`.  `None` (C++: an
    /// empty `std::function`) makes the compiler report "Unrecognized function".
    fn create_func_call(&mut self, _name: &str) -> Option<FuncCallRef<Self>> {
        None
    }
}

/// The plain `LIBEVAL::UCODE` base class as a binder: no variables, no functions, no client
/// data.  Enough for constant expressions such as `"0.2mm + 1mil"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultBinder;

impl UcodeBinder for DefaultBinder {
    type Env<'e> = ();
    type Custom<'e> = NoCustomValue;
    type VarRef = NoVarRef;
}

/// The constant owned by a `TR_UOP_PUSH_VALUE` op (C++: `std::unique_ptr<VALUE> m_value`,
/// which is always either `VALUE( double )` with units or `VALUE( wxString, bool )`).
#[derive(Clone, Debug, PartialEq)]
pub enum UopValue {
    /// A numeric literal, already converted to internal units.
    Numeric {
        /// The value.
        value: f64,
        /// The units the literal was written in.
        units: EdaUnits,
    },
    /// A string literal.
    String {
        /// The text.
        value: String,
        /// True if the text contains `?` or `*`.
        is_wildcard: bool,
    },
}

/// Port of `class LIBEVAL::UOP`: one instruction of the microcode.
pub struct Uop<B: UcodeBinder> {
    op: i32,
    jump_target: i32,
    func: Option<FuncCallRef<B>>,
    ref_: Option<B::VarRef>,
    value: Option<UopValue>,
}

impl<B: UcodeBinder> Uop<B> {
    /// Port of `UOP( int op, std::unique_ptr<VALUE> value )`.
    pub fn with_value(op: i32, value: Option<UopValue>) -> Self {
        Uop {
            op,
            jump_target: -1,
            func: None,
            ref_: None,
            value,
        }
    }

    /// Port of `UOP( int op, std::unique_ptr<VAR_REF> vref )`.
    pub fn with_ref(op: i32, vref: Option<B::VarRef>) -> Self {
        Uop {
            op,
            jump_target: -1,
            func: None,
            ref_: vref,
            value: None,
        }
    }

    /// Port of `UOP( int op, FUNC_CALL_REF func, std::unique_ptr<VAR_REF> vref = nullptr )`.
    pub fn with_func(op: i32, func: Option<FuncCallRef<B>>, vref: Option<B::VarRef>) -> Self {
        Uop {
            op,
            jump_target: -1,
            func,
            ref_: vref,
            value: None,
        }
    }

    /// Port of `explicit UOP( int op )`: bare op (used for the short-circuit jumps, whose only
    /// payload is a jump target).
    pub fn new(op: i32) -> Self {
        Uop {
            op,
            jump_target: -1,
            func: None,
            ref_: None,
            value: None,
        }
    }

    /// The opcode (`TR_UOP_*` / `TR_OP_*`).
    pub fn op(&self) -> i32 {
        self.op
    }

    /// The jump target of a short-circuit jump, -1 if unset.
    pub fn jump_target(&self) -> i32 {
        self.jump_target
    }

    /// Port of `UOP::SetJumpTarget`.
    pub fn set_jump_target(&mut self, target: i32) {
        self.jump_target = target;
    }

    /// Port of `UOP::Exec( CONTEXT* ctx )`: execute the op.  Returns the next instruction
    /// index to run, or -1 to fall through to the following instruction (the common case).
    pub fn exec<'e>(&self, ctx: &mut Context<'e, B>) -> i32 {
        match self.op {
            TR_OP_JZ => {
                // Short-circuit && when the left side is false, then jump past the right-hand
                // microcode.
                let top = ctx.top();

                if ctx.value(top).as_double(ctx) == 0.0 {
                    ctx.pop();

                    // Normalized boolean results pushed when && / || short-circuits, so the
                    // skipped combine op's output contract (an UNSCALED numeric 1/0) is met for
                    // any enclosing operator.  (C++ pushes a shared static constant.)
                    let v = ctx.store_value(Value::from_double(0.0));
                    ctx.push(v);
                    return self.jump_target;
                }

                return -1;
            }

            TR_OP_JNZ => {
                // Short-circuit || when the left side is true, then jump past the right-hand
                // microcode.
                let top = ctx.top();

                if ctx.value(top).as_double(ctx) != 0.0 {
                    ctx.pop();

                    let v = ctx.store_value(Value::from_double(1.0));
                    ctx.push(v);
                    return self.jump_target;
                }

                return -1;
            }

            TR_UOP_PUSH_VAR => {
                let value = match &self.ref_ {
                    Some(vref) => {
                        let v = vref.value(ctx);
                        ctx.store_value(v)
                    }
                    None => ctx.alloc_value(),
                };

                ctx.push(value);

                // C++ leaves the switch with "break" here and falls into the operator code
                // below, where neither mask matches TR_UOP_PUSH_VAR.
                return -1;
            }

            TR_UOP_PUSH_VALUE => {
                // C++ pushes a per-thread copy of string literals (their wxString has a
                // mutable conversion cache) and a pointer to the op's own constant for
                // numbers.  The copy is made with VALUE::Set( const VALUE& ), which does not
                // copy m_stringIsLiteral: a string literal on the stack is therefore *not*
                // flagged as literal.
                match &self.value {
                    Some(UopValue::String { value, is_wildcard }) => {
                        let copy = ctx.alloc_value();
                        ctx.value_mut(copy).set_string_copy(value, *is_wildcard);
                        ctx.push(copy);
                    }
                    Some(UopValue::Numeric { value, units }) => {
                        let mut v = Value::from_double(*value);
                        v.set_units(*units);
                        let v = ctx.store_value(v);
                        ctx.push(v);
                    }
                    None => {
                        // C++ would push a null pointer; the compiler never generates this.
                        let v = ctx.alloc_value();
                        ctx.push(v);
                    }
                }

                return -1;
            }

            TR_OP_METHOD_CALL => {
                if let Some(func) = &self.func {
                    func.call(ctx, self.ref_.as_ref());
                }

                return -1;
            }

            _ => {}
        }

        if self.op & TR_OP_BINARY_MASK != 0 {
            let arg2_id = ctx.pop();
            let arg1_id = ctx.pop();

            let (result, result_units) = {
                let ctx: &Context<'e, B> = ctx;
                let auto_converted: Value<'e, B>;
                let mut arg1 = ctx.value(arg1_id);
                let mut arg2 = ctx.value(arg2_id);

                if arg1.type_() == VarType::String && arg2.type_() == VarType::Numeric {
                    let double_val = if !arg1.string_is_literal() {
                        quacks_like_a_duck(arg1.as_string(ctx))
                    } else {
                        None
                    };

                    if let Some(double_val) = double_val {
                        auto_converted = Value::from_double(double_val);
                        arg1 = &auto_converted;
                    } else if ctx.has_error_callback() {
                        ctx.report_error(&format!(
                            "Type mismatch between '{}' and {}",
                            arg1.as_string(ctx),
                            crt::format_f(arg2.as_double(ctx), 6)
                        ));
                    }
                } else if arg1.type_() == VarType::Numeric && arg2.type_() == VarType::String {
                    // NB: the C++ code passes arg1 (the number, whose string is empty) to
                    // QuacksLikeADuck here, so the right-hand string is never converted.
                    let double_val = if !arg2.string_is_literal() {
                        quacks_like_a_duck(arg1.as_string(ctx))
                    } else {
                        None
                    };

                    if let Some(double_val) = double_val {
                        auto_converted = Value::from_double(double_val);
                        arg2 = &auto_converted;
                    } else if ctx.has_error_callback() {
                        ctx.report_error(&format!(
                            "Type mismatch between {} and '{}'",
                            crt::format_f(arg1.as_double(ctx), 6),
                            arg2.as_string(ctx)
                        ));
                    }
                }

                // TODO: This doesn't fully calculate units correctly yet. We really need to work out the dimensional analysis
                // TODO: (and do this independently of unit specifics - e.g. MM + INCH needs to return one of the dimension
                // TODO: types, but our units framework doesn't currently allow this. Therefore, use some heuristics to
                // TODO: determine the resulting operation unit type for now
                let get_op_result_units = |val1: &Value<'e, B>, val2: &Value<'e, B>| -> EdaUnits {
                    // This condition can occur in, e.g., a unary negation operation
                    if val1.units() == EdaUnits::Unscaled && val2.units() != EdaUnits::Unscaled {
                        return val2.units();
                    }

                    if val1.units() != EdaUnits::Unscaled && val2.units() == EdaUnits::Unscaled {
                        return val1.units();
                    }

                    val2.units()
                };

                let bool_to_double = |b: bool| -> f64 { if b { 1.0 } else { 0.0 } };

                let mut result_units = EdaUnits::Unscaled;

                let result = match self.op {
                    TR_OP_ADD => {
                        result_units = get_op_result_units(arg1, arg2);
                        arg1.as_double(ctx) + arg2.as_double(ctx)
                    }
                    TR_OP_SUB => {
                        result_units = get_op_result_units(arg1, arg2);
                        arg1.as_double(ctx) - arg2.as_double(ctx)
                    }
                    TR_OP_MUL => {
                        result_units = get_op_result_units(arg1, arg2);
                        arg1.as_double(ctx) * arg2.as_double(ctx)
                    }
                    TR_OP_DIV => {
                        result_units = get_op_result_units(arg1, arg2);
                        arg1.as_double(ctx) / arg2.as_double(ctx)
                    }
                    TR_OP_LESS_EQUAL => bool_to_double(arg1.as_double(ctx) <= arg2.as_double(ctx)),
                    TR_OP_GREATER_EQUAL => {
                        bool_to_double(arg1.as_double(ctx) >= arg2.as_double(ctx))
                    }
                    TR_OP_LESS => bool_to_double(arg1.as_double(ctx) < arg2.as_double(ctx)),
                    TR_OP_GREATER => bool_to_double(arg1.as_double(ctx) > arg2.as_double(ctx)),
                    TR_OP_EQUAL => {
                        if arg2.type_() == VarType::Undefined {
                            bool_to_double(arg2.equal_to(ctx, arg1))
                        } else {
                            bool_to_double(arg1.equal_to(ctx, arg2))
                        }
                    }
                    TR_OP_NOT_EQUAL => {
                        if arg2.type_() == VarType::Undefined {
                            bool_to_double(arg2.not_equal_to(ctx, arg1))
                        } else {
                            bool_to_double(arg1.not_equal_to(ctx, arg2))
                        }
                    }
                    TR_OP_BOOL_AND => {
                        bool_to_double(arg1.as_double(ctx) != 0.0 && arg2.as_double(ctx) != 0.0)
                    }
                    TR_OP_BOOL_OR => {
                        bool_to_double(arg1.as_double(ctx) != 0.0 || arg2.as_double(ctx) != 0.0)
                    }
                    _ => 0.0,
                };

                (result, result_units)
            };

            let rp = ctx.alloc_value();
            ctx.value_mut(rp).set_double(result);
            ctx.value_mut(rp).set_units(result_units);
            ctx.push(rp);
            return -1;
        } else if self.op & TR_OP_UNARY_MASK != 0 {
            let arg1 = ctx.pop();
            let arg1_value = ctx.value(arg1).as_double(ctx);
            let result_units = ctx.value(arg1).units();

            let result = match self.op {
                TR_OP_BOOL_NOT => {
                    if arg1_value != 0.0 {
                        0.0
                    } else {
                        1.0
                    }
                }
                _ => {
                    if arg1_value != 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
            };

            let rp = ctx.alloc_value();
            ctx.value_mut(rp).set_double(result);
            ctx.value_mut(rp).set_units(result_units);
            ctx.push(rp);
            return -1;
        }

        -1
    }

    /// Port of `UOP::Format()`: a one-line disassembly of the op.
    ///
    /// The address printed for `PUSH VAR` is that of the Rust variable reference (`(nil)` if
    /// there is none) and has no meaning beyond telling the two cases apart.
    pub fn format(&self) -> String {
        match self.op {
            TR_UOP_PUSH_VAR => match &self.ref_ {
                Some(vref) => format!("PUSH VAR [{:p}]", vref as *const B::VarRef),
                None => "PUSH VAR [(nil)]".to_string(),
            },

            TR_UOP_PUSH_VALUE => match &self.value {
                None => "PUSH nullptr".to_string(),
                Some(UopValue::Numeric { value, .. }) => {
                    format!("PUSH NUM [{}]", crt::format_f(*value, 10))
                }
                Some(UopValue::String { value, .. }) => format!("PUSH STR [{value}]"),
            },

            TR_OP_METHOD_CALL => "MCALL".to_string(),
            TR_OP_FUNC_CALL => "FCALL".to_string(),
            TR_OP_JZ => format!("JZ -> {}", self.jump_target),
            TR_OP_JNZ => format!("JNZ -> {}", self.jump_target),
            _ => format!("{} {}", format_op_name(self.op), self.op),
        }
    }
}

impl<B: UcodeBinder> fmt::Debug for Uop<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

/// Port of the file-static `formatOpName`.
pub(crate) fn format_op_name(op: i32) -> &'static str {
    const SIMPLE_OPS: &[(i32, &str)] = &[
        (TR_OP_MUL, "MUL"),
        (TR_OP_DIV, "DIV"),
        (TR_OP_ADD, "ADD"),
        (TR_OP_SUB, "SUB"),
        (TR_OP_LESS, "LESS"),
        (TR_OP_GREATER, "GREATER"),
        (TR_OP_LESS_EQUAL, "LESS_EQUAL"),
        (TR_OP_GREATER_EQUAL, "GREATER_EQUAL"),
        (TR_OP_EQUAL, "EQUAL"),
        (TR_OP_NOT_EQUAL, "NEQUAL"),
        (TR_OP_BOOL_AND, "AND"),
        (TR_OP_BOOL_OR, "OR"),
        (TR_OP_BOOL_NOT, "NOT"),
    ];

    for (simple_op, mnemonic) in SIMPLE_OPS {
        if *simple_op == op {
            return mnemonic;
        }
    }

    "???"
}

/// Port of `class LIBEVAL::UCODE`: a compiled expression (a sequence of [`Uop`]s) together
/// with the binder that resolved its variables and functions.
pub struct Ucode<B: UcodeBinder> {
    ucode: Vec<Uop<B>>,
    /// any short-circuit jumps? lets `run()` skip the jump loop
    has_jumps: bool,
    binder: B,
}

impl<B: UcodeBinder + Default> Default for Ucode<B> {
    fn default() -> Self {
        Self::new(B::default())
    }
}

impl<B: UcodeBinder> Ucode<B> {
    /// Port of `UCODE::UCODE()`; `binder` provides the overridable `CreateVarRef` /
    /// `CreateFuncCall` behaviour and any per-expression state.
    pub fn new(binder: B) -> Self {
        Ucode {
            ucode: Vec::new(),
            has_jumps: false,
            binder,
        }
    }

    /// The binder (C++: the data members of the `UCODE` subclass).
    pub fn binder(&self) -> &B {
        &self.binder
    }

    /// The binder, mutably.
    pub fn binder_mut(&mut self) -> &mut B {
        &mut self.binder
    }

    /// Port of `UCODE::AddOp`.
    pub fn add_op(&mut self, uop: Uop<B>) {
        self.ucode.push(uop);
    }

    /// Port of `UCODE::MarkHasJumps`: flag that this ucode contains short-circuit jumps, so
    /// `run()` uses the jump-aware loop.
    pub fn mark_has_jumps(&mut self) {
        self.has_jumps = true;
    }

    /// Port of `UCODE::GetSize`: index of the next op to be added (used to backpatch
    /// short-circuit jump targets).
    pub fn size(&self) -> i32 {
        self.ucode.len() as i32
    }

    /// The ops, in execution order.
    pub fn ops(&self) -> &[Uop<B>] {
        &self.ucode
    }

    /// Backpatch the jump target of the op at `index` (C++ keeps the `UOP*` and calls
    /// `UOP::SetJumpTarget` on it).
    pub fn set_jump_target(&mut self, index: usize, target: i32) {
        if let Some(uop) = self.ucode.get_mut(index) {
            uop.set_jump_target(target);
        }
    }

    /// Port of the virtual `UCODE::CreateVarRef`; forwards to the binder.
    pub fn create_var_ref(&mut self, var: &str, field: &str) -> Option<B::VarRef> {
        self.binder.create_var_ref(var, field)
    }

    /// Port of the virtual `UCODE::CreateFuncCall`; forwards to the binder.
    pub fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<B>> {
        self.binder.create_func_call(name)
    }

    /// Port of `UCODE::Run( CONTEXT* ctx )`: execute the microcode and return the handle of
    /// the result, which is owned by `ctx` (read it with [`Context::value`]).
    ///
    /// The context is *not* reset first; as in C++ the caller decides when to
    /// [`Context::reset`].
    pub fn run<'e>(&self, ctx: &mut Context<'e, B>) -> ValueId {
        // (The C++ code wraps this in try/catch because bound functions may throw; Rust
        // functions cannot.)
        if self.has_jumps {
            let mut ip = 0usize;

            while ip < self.ucode.len() {
                let next = self.ucode[ip].exec(ctx);

                // Backpatched jump targets are always forward, past the combine op; a non-forward
                // or stale target would spin or read out of range, so reject it.
                debug_assert!(next < 0 || next as usize > ip);

                ip = if next < 0 || next as usize <= ip {
                    ip + 1
                } else {
                    next as usize
                };
            }
        } else {
            for op in &self.ucode {
                op.exec(ctx);
            }
        }

        if ctx.sp() == 1 {
            ctx.pop()
        } else {
            // If stack is corrupted after execution it suggests a problem with the compiler, not
            // the rule....

            // non-well-formed rules should not be fired on a release build
            ctx.store_value(Value::from_double(0.0))
        }
    }

    /// Port of `UCODE::Dump()`: the disassembly, one op per line.
    pub fn dump(&self) -> String {
        let mut rv = String::new();

        for op in &self.ucode {
            rv += &op.format();
            rv += "\n";
        }

        rv
    }
}

impl<B: UcodeBinder> fmt::Debug for Ucode<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ucode")
            .field("ucode", &self.ucode)
            .field("has_jumps", &self.has_jumps)
            .finish()
    }
}
