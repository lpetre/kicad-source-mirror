// This file is part of libeval, a simple math expression evaluator
//
// Copyright (C) 2017 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `common/libeval/grammar.lemon`, the LALR(1) grammar of the numeric evaluator, in
//! the form Lemon compiles it to.
//!
//! The grammar file declares these precedences (lowest first):
//!
//! ```text
//! %nonassoc VAR ASSIGN SEMCOL.
//! %left PLUS MINUS.
//! %right UNIT.
//! %left DIVIDE MULT.
//! ```
//!
//! As for the expression compiler (see `libeval_compiler::grammar`), the parse tables that
//! KiCad's own `thirdparty/lemon` generates from the unmodified grammar file are embedded
//! and driven by the ported driver in [`crate::lemon`].  The tables between the
//! `BEGIN GENERATED TABLES` / `END GENERATED TABLES` markers are produced by
//! `tests/oracle/gen_tables.py`; the reduce actions below them are the grammar's C++ action
//! blocks, ported by hand in the order and with the stack offsets of the generated
//! `yy_reduce()`.

use super::NumericEvaluator;
use crate::lemon::{Grammar, YyStackEntry, YyTables};

// ---- BEGIN GENERATED TABLES (tests/oracle/gen_tables.py) ----
// Token codes (grammar.h).
pub(crate) const VAR: i32 = 1;
pub(crate) const ASSIGN: i32 = 2;
pub(crate) const SEMCOL: i32 = 3;
pub(crate) const PLUS: i32 = 4;
pub(crate) const MINUS: i32 = 5;
pub(crate) const UNIT: i32 = 6;
pub(crate) const DIVIDE: i32 = 7;
pub(crate) const MULT: i32 = 8;
pub(crate) const ENDS: i32 = 9;
pub(crate) const VALUE: i32 = 10;
pub(crate) const PARENL: i32 = 11;
pub(crate) const PARENR: i32 = 12;

// Rule names (yyRuleName[]), for reference:
//   (0) stmt ::= expr ENDS
//   (1) stmt ::= expr SEMCOL
//   (2) expr ::= VALUE
//   (3) expr ::= expr UNIT
//   (4) expr ::= MINUS expr
//   (5) expr ::= PLUS expr
//   (6) expr ::= VAR
//   (7) expr ::= VAR ASSIGN expr
//   (8) expr ::= expr PLUS expr
//   (9) expr ::= expr MINUS expr
//   (10) expr ::= expr MULT expr
//   (11) expr ::= expr DIVIDE expr
//   (12) expr ::= PARENL expr PARENR
//   (13) main ::= in
//   (14) in ::= stmt
//   (15) in ::= in stmt
//   (16) stmt ::= ENDS

pub(crate) static TABLES: YyTables = YyTables {
    yynocode: 17,
    yystackdepth: 100,
    yynstate: 18,
    yynrule: 17,
    yynrule_with_action: 13,
    yyntoken: 13,
    yy_max_shift: 17,
    yy_min_shiftreduce: 28,
    yy_max_shiftreduce: 44,
    yy_error_action: 45,
    yy_accept_action: 46,
    yy_no_action: 47,
    yy_min_reduce: 48,
    yy_max_reduce: 64,
    yy_acttab_count: 54,
    yy_shift_count: 17,
    yy_reduce_count: 9,
    yy_action: &[
        61, 17, 63, 10, 4, 5, 11, 12, 13, 44, 30, 2,
        14, 29, 9, 8, 31, 6, 7, 28, 17, 59, 17, 4,
        5, 4, 5, 58, 44, 30, 2, 30, 2, 9, 8, 31,
        6, 7, 31, 6, 7, 40, 9, 8, 31, 6, 7, 46,
        1, 1, 10, 15, 16, 3,
    ],
    yy_lookahead: &[
        0, 1, 15, 16, 4, 5, 16, 16, 16, 9, 10, 11,
        16, 3, 4, 5, 6, 7, 8, 9, 1, 16, 1, 4,
        5, 4, 5, 16, 9, 10, 11, 10, 11, 4, 5, 6,
        7, 8, 6, 7, 8, 12, 4, 5, 6, 7, 8, 13,
        14, 15, 16, 16, 16, 2, 17, 17, 17, 17, 17, 17,
        17, 17, 17, 17, 17, 17, 17,
    ],
    yy_shift_ofst: &[
        19, 0, 21, 21, 21, 21, 21, 21, 21, 21, 10, 29,
        38, 32, 32, 32, 32, 51,
    ],
    yy_reduce_ofst: &[
        34, -13, -10, -9, -8, -4, 5, 11, 35, 36,
    ],
    yy_default: &[
        45, 45, 45, 45, 45, 45, 45, 45, 45, 45, 45, 45,
        55, 53, 52, 57, 56, 54,
    ],
    yy_rule_info_lhs: &[
        15, 15, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16,
        16, 13, 14, 14, 15,
    ],
    yy_rule_info_nrhs: &[
        -2, -2, -1, -2, -2, -2, -1, -3, -3, -3, -3, -3,
        -3, -1, -1, -2, -1,
    ],
};
// ---- END GENERATED TABLES ----

/// Port of `struct numEval::TokenType` (`%token_type`), which is also the only member of the
/// generated parser's `YYMINORTYPE` union.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TokenType {
    /// `dValue` (the union member `iValue` is not used by the C++ code)
    pub d_value: f64,
    /// `valid`
    pub valid: bool,
    /// `text` (C++: `char text[32]`; the tokenizer truncates names to 31 bytes)
    pub text: String,
}

/// The grammar of `common/libeval/grammar.lemon`; the `%extra_argument` is the evaluator
/// (`NUMERIC_EVALUATOR* pEval`).
pub(crate) struct NumEvalGrammar;

impl Grammar for NumEvalGrammar {
    type Token = TokenType;
    type Minor = TokenType;
    type Extra = NumericEvaluator;

    fn tables() -> &'static YyTables {
        &TABLES
    }

    fn minor_from_token(token: TokenType) -> TokenType {
        token
    }

    fn reduce(
        p_eval: &mut NumericEvaluator,
        yyruleno: u32,
        stack: &mut [YyStackEntry<TokenType>],
        yymsp: usize,
    ) {
        // The generated code computes most results in a local `YYMINORTYPE yylhsminor` and
        // then copies it over the left-most right-hand-side slot.  That local is not
        // initialised in C; here its unset members are zero / empty.
        let mut yylhsminor = TokenType::default();

        match yyruleno {
            // stmt ::= expr ENDS
            0 => {
                let b = &stack[yymsp - 1].minor;
                p_eval.parse_set_result(if b.valid { b.d_value } else { f64::NAN });
            }

            // stmt ::= expr SEMCOL
            1 => {
                p_eval.parse_set_result(f64::NAN);
            }

            // expr ::= VALUE
            2 => {
                yylhsminor.d_value = stack[yymsp].minor.d_value;
                yylhsminor.valid = true;
                stack[yymsp].minor = yylhsminor;
            }

            // expr ::= expr UNIT
            3 => {
                yylhsminor.d_value = stack[yymsp - 1].minor.d_value * stack[yymsp].minor.d_value;
                yylhsminor.valid = stack[yymsp - 1].minor.valid;
                stack[yymsp - 1].minor = yylhsminor;
            }

            // expr ::= MINUS expr
            4 => {
                stack[yymsp - 1].minor.d_value = -stack[yymsp].minor.d_value;
                stack[yymsp - 1].minor.valid = stack[yymsp].minor.valid;
            }

            // expr ::= PLUS expr
            5 => {
                stack[yymsp - 1].minor.d_value = stack[yymsp].minor.d_value;
                stack[yymsp - 1].minor.valid = stack[yymsp].minor.valid;
            }

            // expr ::= VAR
            6 => {
                yylhsminor.d_value = p_eval.var(&stack[yymsp].minor.text);
                yylhsminor.valid = true;
                stack[yymsp].minor = yylhsminor;
            }

            // expr ::= VAR ASSIGN expr
            7 => {
                p_eval.set_var(&stack[yymsp - 2].minor.text, stack[yymsp].minor.d_value);
                yylhsminor.d_value = stack[yymsp].minor.d_value;
                yylhsminor.valid = false;
                stack[yymsp - 2].minor = yylhsminor;
            }

            // expr ::= expr PLUS expr
            8 => {
                yylhsminor.d_value = stack[yymsp - 2].minor.d_value + stack[yymsp].minor.d_value;
                yylhsminor.valid = stack[yymsp].minor.valid;
                stack[yymsp - 2].minor = yylhsminor;
            }

            // expr ::= expr MINUS expr
            9 => {
                yylhsminor.d_value = stack[yymsp - 2].minor.d_value - stack[yymsp].minor.d_value;
                yylhsminor.valid = stack[yymsp].minor.valid;
                stack[yymsp - 2].minor = yylhsminor;
            }

            // expr ::= expr MULT expr
            10 => {
                yylhsminor.d_value = stack[yymsp - 2].minor.d_value * stack[yymsp].minor.d_value;
                yylhsminor.valid = stack[yymsp].minor.valid;
                stack[yymsp - 2].minor = yylhsminor;
            }

            // expr ::= expr DIVIDE expr
            11 => {
                if stack[yymsp].minor.d_value != 0.0 {
                    yylhsminor.d_value =
                        stack[yymsp - 2].minor.d_value / stack[yymsp].minor.d_value;
                } else {
                    // The C++ action leaves A.dValue unassigned (indeterminate) here; the
                    // port yields 0.  The evaluation is flagged invalid either way.
                    p_eval.parse_error("Divide by zero");
                }

                yylhsminor.valid = stack[yymsp].minor.valid;
                stack[yymsp - 2].minor = yylhsminor;
            }

            // expr ::= PARENL expr PARENR
            12 => {
                stack[yymsp - 2].minor.d_value = stack[yymsp - 1].minor.d_value;
                stack[yymsp - 2].minor.valid = stack[yymsp - 1].minor.valid;
            }

            // (13) main ::= in
            // (14) in ::= stmt (OPTIMIZED OUT)
            // (15) in ::= in stmt
            // (16) stmt ::= ENDS
            _ => {}
        }
    }

    fn syntax_error(p_eval: &mut NumericEvaluator, _yymajor: i32) {
        p_eval.parse_error("Syntax error");
    }

    fn parse_accept(p_eval: &mut NumericEvaluator) {
        p_eval.parse_ok();
    }
}
