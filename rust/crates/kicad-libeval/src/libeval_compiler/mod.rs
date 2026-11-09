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

//! Port of `include/libeval_compiler/libeval_compiler.h`,
//! `common/libeval_compiler/libeval_compiler.cpp` and
//! `common/libeval_compiler/grammar.lemon` (C++ namespace `LIBEVAL`).
//!
//! The C++ header declares everything in one file; here the classes live in sub-modules
//! (all re-exported from this module):
//!
//! | C++ | Rust |
//! |---|---|
//! | `TR_OP_*` / `TR_UOP_*` macros, `COMPILATION_STAGE`, `ERROR_STATUS`, `VAR_TYPE_T`, `TOKEN_TYPE_T`, `T_TOKEN_VALUE`, `T_TOKEN`, `TREE_NODE`, `UNIT_RESOLVER`, `EscapeStringLiteral` | this file |
//! | `VALUE` (+ the hooks that replace its virtual methods) | [`value`] |
//! | `CONTEXT` | [`context`] |
//! | `VAR_REF`, `FUNC_CALL_REF`, `UOP`, `UCODE` | [`ucode`] |
//! | `TOKENIZER` | [`tokenizer`] |
//! | `COMPILER` | [`compiler`] |
//! | `grammar.lemon` | [`grammar`] |
//!
//! # How the C++ class hierarchy is mapped
//!
//! In C++ a client (pcbnew's `PCBEXPR_*` classes) specialises LIBEVAL by *subclassing*:
//! `UCODE::CreateVarRef` / `CreateFuncCall` are virtual, `VAR_REF` is an interface,
//! `CONTEXT` is subclassed to carry the items under test, `VALUE` is subclassed to override
//! `EqualTo` & co, and functions get an untyped `void* self`.  In Rust all of this is
//! expressed with one binding trait, [`UcodeBinder`], and its associated types:
//!
//! * `UcodeBinder::create_var_ref` / `create_func_call` are the virtual methods of `UCODE`;
//!   the binder value lives inside [`Ucode`] like the members of a `UCODE` subclass,
//! * `UcodeBinder::VarRef` is the client's `VAR_REF` type, handed to functions as a typed
//!   `Option<&B::VarRef>` instead of `void*`,
//! * `UcodeBinder::Env<'e>` is the client data of the `CONTEXT` subclass (it may borrow the
//!   board for `'e` without tying the compiled [`Ucode`] to that borrow),
//! * `UcodeBinder::Custom<'e>` is the payload of "subclassed" values and implements
//!   [`CustomValue`], the overridable part of `VALUE`.
//!
//! No `unsafe`, no `dyn Any` down-casts.

pub mod compiler;
pub mod context;
pub(crate) mod grammar;
pub mod tokenizer;
pub mod ucode;
pub mod value;

pub use compiler::Compiler;
pub use context::{Context, ValueId};
pub use tokenizer::Tokenizer;
pub use ucode::{DefaultBinder, FuncCallRef, NoVarRef, Ucode, UcodeBinder, Uop, UopValue, VarRef};
pub use value::{CustomValue, NoCustomValue, Value};

use crate::eda_units::EdaUnits;

/// `TR_OP_BINARY_MASK`
pub const TR_OP_BINARY_MASK: i32 = 0x200;
/// `TR_OP_UNARY_MASK`
pub const TR_OP_UNARY_MASK: i32 = 0x100;

/// `TR_OP_MUL`
pub const TR_OP_MUL: i32 = 0x201;
/// `TR_OP_DIV`
pub const TR_OP_DIV: i32 = 0x202;
/// `TR_OP_ADD`
pub const TR_OP_ADD: i32 = 0x203;
/// `TR_OP_SUB`
pub const TR_OP_SUB: i32 = 0x204;
/// `TR_OP_LESS`
pub const TR_OP_LESS: i32 = 0x205;
/// `TR_OP_GREATER`
pub const TR_OP_GREATER: i32 = 0x206;
/// `TR_OP_LESS_EQUAL`
pub const TR_OP_LESS_EQUAL: i32 = 0x207;
/// `TR_OP_GREATER_EQUAL`
pub const TR_OP_GREATER_EQUAL: i32 = 0x208;
/// `TR_OP_EQUAL`
pub const TR_OP_EQUAL: i32 = 0x209;
/// `TR_OP_NOT_EQUAL`
pub const TR_OP_NOT_EQUAL: i32 = 0x20a;
/// `TR_OP_BOOL_AND`
pub const TR_OP_BOOL_AND: i32 = 0x20b;
/// `TR_OP_BOOL_OR`
pub const TR_OP_BOOL_OR: i32 = 0x20c;
/// `TR_OP_BOOL_NOT`
pub const TR_OP_BOOL_NOT: i32 = 0x100;
/// `TR_OP_FUNC_CALL`
pub const TR_OP_FUNC_CALL: i32 = 24;
/// `TR_OP_METHOD_CALL`
pub const TR_OP_METHOD_CALL: i32 = 25;
/// `TR_UOP_PUSH_VAR`
pub const TR_UOP_PUSH_VAR: i32 = 1;
/// `TR_UOP_PUSH_VALUE`
pub const TR_UOP_PUSH_VALUE: i32 = 2;

// Short-circuit jumps for && / ||.  They peek (do not pop) the left-hand result already on the
// stack and, when it already decides the boolean, jump past the right-hand operand's microcode so
// it is never executed.  This is the compile-time pruning the SSA-style evaluator performs.
// The opcodes deliberately sit outside both TR_OP_UNARY_MASK and TR_OP_BINARY_MASK so they are
// never misclassified as arithmetic operators by a mask test.
/// `TR_OP_JZ`: jump if top == 0 (left side of `&&`)
pub const TR_OP_JZ: i32 = 0x401;
/// `TR_OP_JNZ`: jump if top != 0 (left side of `||`)
pub const TR_OP_JNZ: i32 = 0x402;

/// Port of `enum COMPILATION_STAGE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum CompilationStage {
    /// `CST_PARSE`
    #[default]
    Parse = 0,
    /// `CST_CODEGEN`
    Codegen = 1,
    /// `CST_RUNTIME`
    Runtime = 2,
}

/// Port of `struct ERROR_STATUS`.
///
/// C++ leaves `stage` and `srcPos` uninitialised until the first error; here they start as
/// `Parse` / 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorStatus {
    /// `pendingError`
    pub pending_error: bool,
    /// `stage`
    pub stage: CompilationStage,
    /// `message`
    pub message: String,
    /// `srcPos`: offset into the source expression, counted in characters.
    pub src_pos: i32,
}

/// Port of `enum VAR_TYPE_T`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum VarType {
    /// `VT_STRING`
    String = 1,
    /// `VT_NUMERIC`
    Numeric = 2,
    /// `VT_NUMERIC_DOUBLE`
    NumericDouble = 3,
    /// `VT_UNDEFINED`
    Undefined = 4,
    /// `VT_PARSE_ERROR`
    ParseError = 5,
    /// `VT_NULL`
    Null = 6,
}

/// `TR_UNDEFINED` (port of `enum TOKEN_TYPE_T`; kept as integer constants because tree node
/// `op` fields hold these and the `TR_OP_*` codes interchangeably).
pub const TR_UNDEFINED: i32 = 0;
/// `TR_NUMBER`
pub const TR_NUMBER: i32 = 1;
/// `TR_IDENTIFIER`
pub const TR_IDENTIFIER: i32 = 2;
/// `TR_ASSIGN`
pub const TR_ASSIGN: i32 = 3;
/// `TR_STRUCT_REF`
pub const TR_STRUCT_REF: i32 = 4;
/// `TR_STRING`
pub const TR_STRING: i32 = 5;
/// `TR_UNIT`
pub const TR_UNIT: i32 = 6;
/// `TR_ARG_LIST`
pub const TR_ARG_LIST: i32 = 7;
/// `TR_NULL`
pub const TR_NULL: i32 = 8;

/// Port of `struct T_TOKEN_VALUE`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TTokenValue {
    /// `str` (C++: a `wxString*` owned by the compiler's garbage list; `nullptr` -> `None`)
    pub str: Option<String>,
    /// `num`
    pub num: f64,
    /// `idx`
    pub idx: i32,
}

/// Port of `struct T_TOKEN`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TToken {
    /// `token`: one of the `G_*` token codes of the grammar.
    pub token: i32,
    /// `value`
    pub value: TTokenValue,
}

/// Handle of a [`TreeNode`] inside its [`Compiler`] (C++: `TREE_NODE*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub(crate) usize);

/// Port of `class TREE_NODE`: a node of the syntax tree built by the parser.
///
/// The nodes of one compilation are owned by the [`Compiler`] (C++: its `m_gcItems` list)
/// and refer to each other by [`NodeId`].  The C++ member `uop` is transient code generation
/// state and is kept in a side table by `Compiler::generate_ucode` instead.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// `value`
    pub value: TTokenValue,
    /// `op`: a `TR_*` token type or `TR_OP_*` operator code.
    pub op: i32,
    /// `leaf`
    pub leaf: [Option<NodeId>; 2],
    /// `valid`
    pub valid: bool,
    /// `isTerminal`
    pub is_terminal: bool,
    /// `isVisited`
    pub is_visited: bool,
    /// `srcPos`: the source position of the parser when the node was created, i.e. the start
    /// of the token following the node's text (see `Compiler::compile`).
    pub src_pos: i32,
}

/// Port of `class UNIT_RESOLVER`: tells the compiler which unit suffixes exist and converts
/// literals carrying one.  The provided methods are the C++ base class implementation
/// (no units at all).
pub trait UnitResolver {
    /// Port of `GetSupportedUnits`: the unit suffixes, in matching priority order.
    fn supported_units(&self) -> &[&str] {
        &[]
    }

    /// Port of `GetSupportedUnitsTypes`: the unit type of each entry of
    /// [`UnitResolver::supported_units`].
    fn supported_units_types(&self) -> &[EdaUnits] {
        &[]
    }

    /// Port of `GetSupportedUnitsMessage`: the hint appended to the "Missing units" error;
    /// an empty string disables that error.
    fn supported_units_message(&self) -> String {
        String::new()
    }

    /// Port of `Convert`: convert the literal `string` carrying unit number `unit_type`
    /// (an index into [`UnitResolver::supported_units`]).
    fn convert(&self, _string: &str, _unit_type: i32) -> f64 {
        0.0
    }
}

/// The C++ base class `LIBEVAL::UNIT_RESOLVER` itself: no units.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultUnitResolver;

impl UnitResolver for DefaultUnitResolver {}

/// Port of `LIBEVAL::EscapeStringLiteral`: escape text for use between the quotes of an
/// expression string.
///
/// The tokenizer's only escape is `\'`, so text ending in a backslash has no encoding.
///
/// Returns the escaped text without its quotes, or `None` if the text cannot be written.
pub fn escape_string_literal(text: &str) -> Option<String> {
    if text.ends_with('\\') {
        return None;
    }

    Some(text.replace('\'', "\\'"))
}
