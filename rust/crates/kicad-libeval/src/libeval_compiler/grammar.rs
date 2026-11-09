// This file is part of libeval, a simple math expression evaluator
//
// Copyright (C) 2017 Michael Geselbracht, mgeselbracht3@gmail.com
// Copyright (C) 2019-2020 KiCad Developers, see AUTHORS.txt for contributors.
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

//! Port of `common/libeval_compiler/grammar.lemon`, the LALR(1) grammar of the expression
//! language, in the form Lemon compiles it to.
//!
//! The grammar file declares these precedences (lowest first):
//!
//! ```text
//! %nonassoc G_IDENTIFIER G_ASSIGN G_SEMCOL.
//! %left G_COMMA.
//! %left G_BOOL_AND.
//! %left G_BOOL_OR.
//! %left G_BOOL_XOR.
//! %nonassoc G_LESS_THAN G_GREATER_THAN G_LESS_EQUAL_THAN G_GREATER_EQUAL_THAN G_EQUAL G_NOT_EQUAL.
//! %right G_BOOL_NOT.
//! %left G_PLUS G_MINUS.
//! %left G_DIVIDE G_MULT.
//! %nonassoc G_STRUCT_REF.
//! %nonassoc G_UNIT.
//! ```
//!
//! Some consequences that differ from C-like languages and that the tables reproduce
//! faithfully: `&&` binds *looser* than `||`; comparisons do not chain (`a < b < c` and
//! `a == b == c` are syntax errors); `!` binds tighter than comparisons but looser than
//! arithmetic (`!a + b` is `!(a + b)`); unary minus has the precedence of binary minus
//! (`-2 * 3` is `-(2 * 3)`).  Lemon takes the precedence of a rule from its *first* terminal
//! with a precedence (yacc uses the last), and it turns states whose only action is a reduce
//! into unconditional reductions: that is why `a.b.c` parses as `(a.b).c` although
//! `G_STRUCT_REF` is declared `%nonassoc` (the error entry of that state is never
//! consulted).  `tests/libeval_grammar.rs` pins down the complete precedence/associativity
//! matrix.
//!
//! Rather than re-deriving a recursive-descent parser from that description, the port embeds
//! the parse tables that KiCad's own `thirdparty/lemon` generates from the unmodified grammar
//! file and drives them with the ported driver in [`crate::lemon`].  The tables between the
//! `BEGIN GENERATED TABLES` / `END GENERATED TABLES` markers are produced by
//! `tests/oracle/gen_tables.py` (see `tests/oracle/README.md`); the reduce actions below
//! them are the grammar's C++ action blocks, ported by hand in the order and with the
//! stack offsets of the generated `yy_reduce()`.

use std::marker::PhantomData;

use super::compiler::Compiler;
use super::{
    NodeId, TR_ARG_LIST, TR_IDENTIFIER, TR_NULL, TR_NUMBER, TR_OP_ADD, TR_OP_BOOL_AND,
    TR_OP_BOOL_NOT, TR_OP_BOOL_OR, TR_OP_DIV, TR_OP_EQUAL, TR_OP_FUNC_CALL, TR_OP_GREATER,
    TR_OP_GREATER_EQUAL, TR_OP_LESS, TR_OP_LESS_EQUAL, TR_OP_MUL, TR_OP_NOT_EQUAL, TR_OP_SUB,
    TR_STRING, TR_STRUCT_REF, TR_UNIT, TToken, TTokenValue,
};
use crate::lemon::{Grammar, YyStackEntry, YyTables};

// ---- BEGIN GENERATED TABLES (tests/oracle/gen_tables.py) ----
// Token codes (grammar.h).
pub(crate) const G_IDENTIFIER: i32 = 1;
pub(crate) const G_ASSIGN: i32 = 2;
pub(crate) const G_SEMCOL: i32 = 3;
pub(crate) const G_COMMA: i32 = 4;
pub(crate) const G_BOOL_AND: i32 = 5;
pub(crate) const G_BOOL_OR: i32 = 6;
pub(crate) const G_BOOL_XOR: i32 = 7;
pub(crate) const G_LESS_THAN: i32 = 8;
pub(crate) const G_GREATER_THAN: i32 = 9;
pub(crate) const G_LESS_EQUAL_THAN: i32 = 10;
pub(crate) const G_GREATER_EQUAL_THAN: i32 = 11;
pub(crate) const G_EQUAL: i32 = 12;
pub(crate) const G_NOT_EQUAL: i32 = 13;
pub(crate) const G_BOOL_NOT: i32 = 14;
pub(crate) const G_PLUS: i32 = 15;
pub(crate) const G_MINUS: i32 = 16;
pub(crate) const G_DIVIDE: i32 = 17;
pub(crate) const G_MULT: i32 = 18;
pub(crate) const G_STRUCT_REF: i32 = 19;
pub(crate) const G_UNIT: i32 = 20;
pub(crate) const G_ENDS: i32 = 21;
pub(crate) const G_VALUE: i32 = 22;
pub(crate) const G_STRING: i32 = 23;
pub(crate) const G_PARENL: i32 = 24;
pub(crate) const G_PARENR: i32 = 25;

// Rule names (yyRuleName[]), for reference:
//   (0) stmt ::= nt G_ENDS
//   (1) nt ::= G_VALUE
//   (2) nt ::= G_VALUE G_UNIT
//   (3) nt ::= G_STRING
//   (4) nt ::= G_IDENTIFIER
//   (5) nt ::= nt G_COMMA nt
//   (6) nt ::= nt G_LESS_THAN nt
//   (7) nt ::= nt G_GREATER_THAN nt
//   (8) nt ::= nt G_LESS_EQUAL_THAN nt
//   (9) nt ::= nt G_GREATER_EQUAL_THAN nt
//   (10) nt ::= nt G_NOT_EQUAL nt
//   (11) nt ::= nt G_BOOL_AND nt
//   (12) nt ::= nt G_BOOL_OR nt
//   (13) nt ::= G_BOOL_NOT nt
//   (14) nt ::= nt G_PLUS nt
//   (15) nt ::= G_PLUS nt
//   (16) nt ::= G_MINUS nt
//   (17) nt ::= nt G_MINUS nt
//   (18) nt ::= nt G_MULT nt
//   (19) nt ::= nt G_DIVIDE nt
//   (20) nt ::= nt G_EQUAL nt
//   (21) nt ::= nt G_STRUCT_REF nt
//   (22) nt ::= G_PARENL nt G_PARENR
//   (23) nt ::= G_IDENTIFIER G_PARENL nt G_PARENR
//   (24) nt ::= G_IDENTIFIER G_PARENL G_PARENR
//   (25) main ::= in
//   (26) in ::= stmt
//   (27) in ::= in stmt
//   (28) stmt ::= G_ENDS

pub(crate) static TABLES: YyTables = YyTables {
    yynocode: 30,
    yystackdepth: 100,
    yynstate: 43,
    yynrule: 29,
    yynrule_with_action: 25,
    yyntoken: 26,
    yy_max_shift: 42,
    yy_min_shiftreduce: 52,
    yy_max_shiftreduce: 80,
    yy_error_action: 81,
    yy_accept_action: 82,
    yy_no_action: 83,
    yy_min_reduce: 84,
    yy_max_reduce: 112,
    yy_acttab_count: 184,
    yy_shift_count: 42,
    yy_reduce_count: 20,
    yy_action: &[
        23, 82, 1, 1, 20, 14, 13, 22, 19, 18, 17, 16,
        8, 15, 21, 12, 11, 9, 10, 7, 34, 35, 20, 14,
        13, 74, 19, 18, 17, 16, 8, 15, 33, 12, 11, 9,
        10, 7, 105, 26, 20, 14, 13, 75, 19, 18, 17, 16,
        8, 15, 39, 12, 11, 9, 10, 7, 40, 52, 14, 13,
        36, 19, 18, 17, 16, 8, 15, 37, 12, 11, 9, 10,
        7, 13, 27, 19, 18, 17, 16, 8, 15, 25, 12, 11,
        9, 10, 7, 81, 81, 81, 81, 81, 81, 81, 12, 11,
        9, 10, 7, 19, 18, 17, 16, 8, 15, 7, 12, 11,
        9, 10, 7, 109, 41, 12, 11, 9, 10, 7, 9, 10,
        7, 23, 28, 29, 111, 6, 5, 4, 41, 30, 31, 32,
        80, 42, 55, 3, 24, 2, 54, 83, 83, 6, 5, 4,
        41, 83, 83, 83, 80, 42, 55, 3, 83, 83, 83, 83,
        83, 6, 5, 4, 41, 83, 83, 83, 83, 42, 55, 3,
        76, 83, 83, 83, 83, 6, 5, 4, 83, 83, 83, 83,
        83, 42, 55, 3,
    ],
    yy_lookahead: &[
        26, 27, 28, 29, 4, 5, 6, 26, 8, 9, 10, 11,
        12, 13, 26, 15, 16, 17, 18, 19, 26, 26, 4, 5,
        6, 25, 8, 9, 10, 11, 12, 13, 26, 15, 16, 17,
        18, 19, 26, 26, 4, 5, 6, 25, 8, 9, 10, 11,
        12, 13, 26, 15, 16, 17, 18, 19, 26, 21, 5, 6,
        26, 8, 9, 10, 11, 12, 13, 26, 15, 16, 17, 18,
        19, 6, 26, 8, 9, 10, 11, 12, 13, 26, 15, 16,
        17, 18, 19, 8, 9, 10, 11, 12, 13, 19, 15, 16,
        17, 18, 19, 8, 9, 10, 11, 12, 13, 19, 15, 16,
        17, 18, 19, 0, 1, 15, 16, 17, 18, 19, 17, 18,
        19, 26, 26, 26, 29, 14, 15, 16, 1, 26, 26, 26,
        21, 22, 23, 24, 26, 24, 20, 30, 30, 14, 15, 16,
        1, 30, 30, 30, 21, 22, 23, 24, 30, 30, 30, 30,
        30, 14, 15, 16, 1, 30, 30, 30, 30, 22, 23, 24,
        25, 30, 30, 30, 30, 14, 15, 16, 30, 30, 30, 30,
        30, 22, 23, 24, 30, 30, 30, 26, 26, 26, 26, 26,
        26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26,
        26, 26, 26, 26, 26, 26,
    ],
    yy_shift_ofst: &[
        127, 111, 143, 159, 159, 159, 159, 159, 159, 159, 159, 159,
        159, 159, 159, 159, 159, 159, 159, 159, 159, 0, 18, 36,
        53, 67, 79, 91, 79, 79, 79, 79, 79, 98, 101, 101,
        101, 101, 74, 86, 86, 113, 118,
    ],
    yy_reduce_ofst: &[
        -26, 95, -19, -12, -6, -5, 6, 12, 13, 24, 30, 34,
        41, 48, 55, 96, 97, 103, 104, 105, 110,
    ],
    yy_default: &[
        81, 81, 81, 81, 81, 81, 81, 81, 81, 81, 81, 81,
        81, 81, 81, 81, 81, 81, 81, 81, 81, 81, 81, 81,
        89, 95, 104, 96, 94, 93, 92, 91, 90, 97, 100, 99,
        101, 98, 105, 103, 102, 88, 85,
    ],
    yy_rule_info_lhs: &[
        29, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26,
        26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26,
        26, 27, 28, 28, 29,
    ],
    yy_rule_info_nrhs: &[
        -2, -1, -2, -1, -1, -3, -3, -3, -3, -3, -3, -3,
        -3, -2, -3, -2, -2, -3, -3, -3, -3, -3, -3, -4,
        -3, -1, -1, -2, -1,
    ],
};
// ---- END GENERATED TABLES ----

/// `YYMINORTYPE` of the generated parser: the semantic value of a stack entry.
#[derive(Clone, Debug, Default)]
pub(crate) enum YyMinor {
    /// `yyinit`: nothing stored yet.
    #[default]
    None,
    /// `yy0`: the value of a terminal symbol (`%token_type { LIBEVAL::T_TOKEN }`).
    Token(TToken),
    /// `yy11`: the value of the non-terminal `nt` (`%type nt {LIBEVAL::TREE_NODE*}`).
    Node(NodeId),
}

/// The grammar of `common/libeval_compiler/grammar.lemon`; the `%extra_argument` is the
/// compiler (`LIBEVAL::COMPILER* pEval`).
pub(crate) struct CompilerGrammar<'a>(PhantomData<fn(&'a ())>);

/// `yymsp[offset].minor.yy0.value`
fn token_value(entry: &YyStackEntry<YyMinor>) -> TTokenValue {
    match &entry.minor {
        YyMinor::Token(token) => token.value.clone(),
        _ => TTokenValue::default(),
    }
}

/// `yymsp[offset].minor.yy11`
fn node(entry: &YyStackEntry<YyMinor>) -> Option<NodeId> {
    match &entry.minor {
        YyMinor::Node(id) => Some(*id),
        _ => None,
    }
}

/// The action shared by all binary operator rules:
///
/// ```text
/// A = newNode( pEval, <op> );
/// A->leaf[0] = B;
/// A->leaf[1] = C;
/// ```
fn binary(p_eval: &mut Compiler<'_>, op: i32, stack: &mut [YyStackEntry<YyMinor>], yymsp: usize) {
    let b = node(&stack[yymsp - 2]);
    let c = node(&stack[yymsp]);
    let a = p_eval.new_node(op, &TTokenValue::default());

    p_eval.node_mut(a).leaf = [b, c];
    stack[yymsp - 2].minor = YyMinor::Node(a);
}

/// The action shared by `nt ::= G_PLUS nt` and `nt ::= G_PARENL nt G_PARENR`: a copy of B
/// (same op, value and leaves) created at the current source position:
///
/// ```text
/// A = newNode( pEval, B->op, B->value );
/// A->leaf[0] = B->leaf[0];
/// A->leaf[1] = B->leaf[1];
/// ```
fn copy_of(p_eval: &mut Compiler<'_>, b: Option<NodeId>) -> YyMinor {
    let Some(b) = b else {
        return YyMinor::None;
    };

    let (op, value, leaf) = {
        let b = p_eval.node(b);
        (b.op, b.value.clone(), b.leaf)
    };

    let a = p_eval.new_node(op, &value);
    p_eval.node_mut(a).leaf = leaf;

    YyMinor::Node(a)
}

impl<'a> Grammar for CompilerGrammar<'a> {
    type Token = TToken;
    type Minor = YyMinor;
    type Extra = Compiler<'a>;

    fn tables() -> &'static YyTables {
        &TABLES
    }

    fn minor_from_token(token: TToken) -> YyMinor {
        YyMinor::Token(token)
    }

    fn reduce(
        p_eval: &mut Compiler<'a>,
        yyruleno: u32,
        stack: &mut [YyStackEntry<YyMinor>],
        yymsp: usize,
    ) {
        match yyruleno {
            // stmt ::= nt G_ENDS
            0 => {
                p_eval.set_root(node(&stack[yymsp - 1]));
            }

            // nt ::= G_VALUE
            1 => {
                let a = p_eval.new_node(TR_NUMBER, &token_value(&stack[yymsp]));
                stack[yymsp].minor = YyMinor::Node(a);
            }

            // nt ::= G_VALUE G_UNIT
            2 => {
                let a = p_eval.new_node(TR_NUMBER, &token_value(&stack[yymsp - 1]));
                let unit = p_eval.new_node(TR_UNIT, &token_value(&stack[yymsp]));
                p_eval.node_mut(a).leaf[0] = Some(unit);
                stack[yymsp - 1].minor = YyMinor::Node(a);
            }

            // nt ::= G_STRING
            3 => {
                let a = p_eval.new_node(TR_STRING, &token_value(&stack[yymsp]));
                stack[yymsp].minor = YyMinor::Node(a);
            }

            // nt ::= G_IDENTIFIER
            4 => {
                let a = p_eval.new_node(TR_IDENTIFIER, &token_value(&stack[yymsp]));
                stack[yymsp].minor = YyMinor::Node(a);
            }

            // nt ::= nt G_COMMA nt
            5 => binary(p_eval, TR_ARG_LIST, stack, yymsp),
            // nt ::= nt G_LESS_THAN nt
            6 => binary(p_eval, TR_OP_LESS, stack, yymsp),
            // nt ::= nt G_GREATER_THAN nt
            7 => binary(p_eval, TR_OP_GREATER, stack, yymsp),
            // nt ::= nt G_LESS_EQUAL_THAN nt
            8 => binary(p_eval, TR_OP_LESS_EQUAL, stack, yymsp),
            // nt ::= nt G_GREATER_EQUAL_THAN nt
            9 => binary(p_eval, TR_OP_GREATER_EQUAL, stack, yymsp),
            // nt ::= nt G_NOT_EQUAL nt
            10 => binary(p_eval, TR_OP_NOT_EQUAL, stack, yymsp),
            // nt ::= nt G_BOOL_AND nt
            11 => binary(p_eval, TR_OP_BOOL_AND, stack, yymsp),
            // nt ::= nt G_BOOL_OR nt
            12 => binary(p_eval, TR_OP_BOOL_OR, stack, yymsp),

            // nt ::= G_BOOL_NOT nt
            13 => {
                let b = node(&stack[yymsp]);
                let a = p_eval.new_node(TR_OP_BOOL_NOT, &TTokenValue::default());
                p_eval.node_mut(a).leaf[0] = b;
                stack[yymsp - 1].minor = YyMinor::Node(a);
            }

            // nt ::= nt G_PLUS nt
            14 => binary(p_eval, TR_OP_ADD, stack, yymsp),

            // nt ::= G_PLUS nt
            15 => {
                stack[yymsp - 1].minor = copy_of(p_eval, node(&stack[yymsp]));
            }

            // nt ::= G_MINUS nt
            16 => {
                let b = node(&stack[yymsp]);
                let a = p_eval.new_node(TR_OP_SUB, &TTokenValue::default());
                let zero = p_eval.new_node(TR_NUMBER, &TTokenValue::default());
                p_eval.node_mut(a).leaf = [Some(zero), b];
                stack[yymsp - 1].minor = YyMinor::Node(a);
            }

            // nt ::= nt G_MINUS nt
            17 => binary(p_eval, TR_OP_SUB, stack, yymsp),
            // nt ::= nt G_MULT nt
            18 => binary(p_eval, TR_OP_MUL, stack, yymsp),
            // nt ::= nt G_DIVIDE nt
            19 => binary(p_eval, TR_OP_DIV, stack, yymsp),
            // nt ::= nt G_EQUAL nt
            20 => binary(p_eval, TR_OP_EQUAL, stack, yymsp),
            // nt ::= nt G_STRUCT_REF nt
            21 => binary(p_eval, TR_STRUCT_REF, stack, yymsp),

            // nt ::= G_PARENL nt G_PARENR
            22 => {
                stack[yymsp - 2].minor = copy_of(p_eval, node(&stack[yymsp - 1]));
            }

            // nt ::= G_IDENTIFIER G_PARENL nt G_PARENR
            23 => {
                let b = node(&stack[yymsp - 1]);
                let a = p_eval.new_node(TR_OP_FUNC_CALL, &TTokenValue::default());
                let name = p_eval.new_node(TR_IDENTIFIER, &token_value(&stack[yymsp - 3]));
                p_eval.node_mut(a).leaf = [Some(name), b];
                stack[yymsp - 3].minor = YyMinor::Node(a);
            }

            // nt ::= G_IDENTIFIER G_PARENL G_PARENR
            24 => {
                let a = p_eval.new_node(TR_OP_FUNC_CALL, &TTokenValue::default());
                let name = p_eval.new_node(TR_IDENTIFIER, &token_value(&stack[yymsp - 2]));
                let null = p_eval.new_node(TR_NULL, &TTokenValue::default());
                p_eval.node_mut(a).leaf = [Some(name), Some(null)];
                stack[yymsp - 2].minor = YyMinor::Node(a);
            }

            // (25) main ::= in
            // (26) in ::= stmt (OPTIMIZED OUT)
            // (27) in ::= in stmt
            // (28) stmt ::= G_ENDS
            _ => {}
        }
    }

    fn syntax_error(p_eval: &mut Compiler<'a>, _yymajor: i32) {
        p_eval.parse_error("Syntax error");
    }

    fn parse_accept(p_eval: &mut Compiler<'a>) {
        p_eval.parse_ok();
    }
}
