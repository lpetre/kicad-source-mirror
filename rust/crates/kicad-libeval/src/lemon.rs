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
//
// The driver this file ports, `thirdparty/lemon/lempar.c`, carries this notice:
//
//   2000-05-29
//
//   The author disclaims copyright to this source code.  In place of
//   a legal notice, here is a blessing:
//
//      May you do good and not evil.
//      May you find forgiveness for yourself and forgive others.
//      May you share freely, never taking more than you give.

//! Port of `thirdparty/lemon/lempar.c`, the driver template of the LEMON LALR(1) parser
//! generator.
//!
//! KiCad's two expression grammars (`common/libeval_compiler/grammar.lemon` and
//! `common/libeval/grammar.lemon`) are compiled by Lemon into parse tables plus this driver.
//! To accept *exactly* the same language, build exactly the same trees and report syntax
//! errors at exactly the same tokens, the Rust port keeps that structure: the tables Lemon
//! generates from the unmodified grammar files are embedded verbatim (see
//! `libeval_compiler::grammar` and `numeric_evaluator::grammar`, and
//! `tests/oracle/gen_tables.py` for how they are produced) and this module is a statement by
//! statement port of the driver, configured the way both grammars configure it:
//!
//! * `YYSTACKDEPTH` is 100 (fixed-size stack, overflow handling included),
//! * no `%fallback`, no `%wildcard`, no `error` symbol (`YYERRORSYMBOL` undefined) and
//!   `YYNOERRORRECOVERY` undefined, so syntax errors follow the "report, discard the token,
//!   suppress further reports until three tokens have been shifted" branch,
//! * no destructors (`yy_destructor` is empty for both grammars).
//!
//! The tracing code (`#ifndef NDEBUG` / `yyTraceFILE`) and the coverage instrumentation are
//! not ported.

/// The numeric tables and control constants Lemon writes into the generated parser
/// (the "control #defines" and "parsing tables" sections of `grammar.c`).
///
/// Field names are the lower-case spelling of the C macros/arrays they hold.
///
/// Not every constant is consulted by the ported driver (some only size C arrays or feed the
/// tracing code); they are all kept so the tables can be pasted from the generator as is.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct YyTables {
    /// A symbol code that is not used for any terminal or non-terminal.
    pub yynocode: u16,
    /// Maximum depth of the parser's stack.
    pub yystackdepth: usize,
    /// The combined number of states.
    pub yynstate: u16,
    /// The number of rules in the grammar.
    pub yynrule: u16,
    /// Number of rules that have a reduce action attached.
    pub yynrule_with_action: u16,
    /// Number of terminal symbols.
    pub yyntoken: u16,
    /// Maximum value for shift actions.
    pub yy_max_shift: u16,
    /// Minimum value for shift-reduce actions.
    pub yy_min_shiftreduce: u16,
    /// Maximum value for shift-reduce actions.
    pub yy_max_shiftreduce: u16,
    /// The `yy_action[]` code for syntax error.
    pub yy_error_action: u16,
    /// The `yy_action[]` code for accept.
    pub yy_accept_action: u16,
    /// The `yy_action[]` code for no-op.
    pub yy_no_action: u16,
    /// Minimum value for reduce actions.
    pub yy_min_reduce: u16,
    /// Maximum value for reduce actions.
    pub yy_max_reduce: u16,
    /// Number of meaningful entries of `yy_action`.
    pub yy_acttab_count: usize,
    /// Highest state number that has a shift offset.
    pub yy_shift_count: u16,
    /// Highest state number that has a reduce offset.
    pub yy_reduce_count: u16,
    /// A single table containing all actions.
    pub yy_action: &'static [u16],
    /// The lookahead for each entry of `yy_action`; used to detect hash collisions.
    pub yy_lookahead: &'static [u16],
    /// For each state, the offset into `yy_action` for shifting terminals.
    pub yy_shift_ofst: &'static [i32],
    /// For each state, the offset into `yy_action` for shifting non-terminals after a reduce.
    pub yy_reduce_ofst: &'static [i32],
    /// Default action for each state.
    pub yy_default: &'static [u16],
    /// For rule J, the symbol on the left-hand side of that rule.
    pub yy_rule_info_lhs: &'static [u16],
    /// For rule J, the negative of the number of symbols on the right-hand side of that rule.
    pub yy_rule_info_nrhs: &'static [i8],
}

/// Port of `struct yyStackEntry`: a single element of the parser's stack.
///
/// After the "shift" half of a SHIFTREDUCE action, `stateno` actually contains the reduce
/// action for the second half of the SHIFTREDUCE.
#[derive(Clone, Debug, Default)]
pub(crate) struct YyStackEntry<M> {
    /// The state number, or reduce action in SHIFTREDUCE.
    pub stateno: u16,
    /// The major token value: the code number for the token at this stack level.
    pub major: u16,
    /// The user-supplied minor token value (the semantic value).
    pub minor: M,
}

/// The grammar-specific parts Lemon inserts into the driver template: the tables, the
/// reduce actions and the `%syntax_error` / `%parse_accept` / `%parse_failure` /
/// `%stack_overflow` code blocks.
pub(crate) trait Grammar {
    /// `ParseTOKENTYPE`: the minor type of terminal symbols.
    type Token;
    /// `YYMINORTYPE`: the union of all minor types.  Rust needs a real value in every stack
    /// slot, hence `Default` (C leaves unused slots uninitialised).
    type Minor: Default;
    /// The type of the `%extra_argument`.
    type Extra;

    /// The generated parse tables.
    fn tables() -> &'static YyTables;

    /// Store a terminal's value into a stack slot (`yytos->minor.yy0 = yyMinor`).
    fn minor_from_token(token: Self::Token) -> Self::Minor;

    /// The body of the `switch( yyruleno )` in `yy_reduce`.  `yymsp` is the index of the top
    /// of the stack; `stack[yymsp - k]` is C's `yymsp[-k]`.
    fn reduce(
        extra: &mut Self::Extra,
        yyruleno: u32,
        stack: &mut [YyStackEntry<Self::Minor>],
        yymsp: usize,
    );

    /// The `%syntax_error` code block.
    fn syntax_error(extra: &mut Self::Extra, yymajor: i32);

    /// The `%parse_accept` code block.
    fn parse_accept(extra: &mut Self::Extra);

    /// The `%parse_failure` code block (empty for both KiCad grammars).
    fn parse_failure(_extra: &mut Self::Extra) {}

    /// The `%stack_overflow` code block (empty for both KiCad grammars).
    fn stack_overflow(_extra: &mut Self::Extra) {}
}

/// Port of `struct yyParser`: the complete state of the parser.
pub(crate) struct YyParser<G: Grammar> {
    /// Index of the top element of the stack (C: pointer `yytos`).
    yytos: usize,
    /// Shifts left before out of the error.
    yyerrcnt: i32,
    /// The parser's stack: `YYSTACKDEPTH` entries, `yystack[0]` is the bottom marker.
    yystack: Vec<YyStackEntry<G::Minor>>,
}

impl<G: Grammar> YyParser<G> {
    /// Port of `ParseAlloc` + `ParseInit`: allocate and initialise a new parser.
    pub(crate) fn new() -> Self {
        let depth = G::tables().yystackdepth;
        let mut yystack = Vec::with_capacity(depth);

        for _ in 0..depth {
            yystack.push(YyStackEntry {
                stateno: 0,
                major: 0,
                minor: G::Minor::default(),
            });
        }

        // yypParser->yyerrcnt = -1;
        // yypParser->yytos = yypParser->yystack;
        // yypParser->yystack[0].stateno = 0;
        // yypParser->yystack[0].major = 0;
        YyParser {
            yytos: 0,
            yyerrcnt: -1,
            yystack,
        }
    }

    /// Index of the last entry in the stack (C: `yystackEnd`).
    fn yystack_end(&self) -> usize {
        self.yystack.len() - 1
    }

    /// Port of `yy_pop_parser_stack`: pop the parser's stack once.  Neither grammar defines
    /// destructors, so nothing else happens.
    fn yy_pop_parser_stack(&mut self) {
        debug_assert!(self.yytos > 0);
        self.yytos -= 1;
    }

    /// Port of `yy_find_shift_action`: find the appropriate action for a parser given the
    /// terminal look-ahead token `i_look_ahead`.
    fn yy_find_shift_action(i_look_ahead: u16, stateno: u16) -> u16 {
        let t = G::tables();

        if stateno > t.yy_max_shift {
            return stateno;
        }

        debug_assert!(stateno <= t.yy_shift_count);

        let mut i = t.yy_shift_ofst[stateno as usize];

        debug_assert!(i >= 0);
        debug_assert!(i as usize <= t.yy_acttab_count);
        debug_assert!(i as usize + t.yyntoken as usize <= t.yy_lookahead.len());
        debug_assert!(i_look_ahead != t.yynocode);
        debug_assert!(i_look_ahead < t.yyntoken);

        i += i_look_ahead as i32;

        debug_assert!((i as usize) < t.yy_lookahead.len());

        if t.yy_lookahead[i as usize] != i_look_ahead {
            // No YYFALLBACK, no YYWILDCARD.
            t.yy_default[stateno as usize]
        } else {
            debug_assert!(i >= 0 && (i as usize) < t.yy_action.len());
            t.yy_action[i as usize]
        }
    }

    /// Port of `yy_find_reduce_action`: find the appropriate action for a parser given the
    /// non-terminal look-ahead token `i_look_ahead`.
    fn yy_find_reduce_action(stateno: u16, i_look_ahead: u16) -> u16 {
        let t = G::tables();

        debug_assert!(stateno <= t.yy_reduce_count);

        let mut i = t.yy_reduce_ofst[stateno as usize];

        debug_assert!(i_look_ahead != t.yynocode);

        i += i_look_ahead as i32;

        debug_assert!(i >= 0 && (i as usize) < t.yy_acttab_count);
        debug_assert!(t.yy_lookahead[i as usize] == i_look_ahead);

        t.yy_action[i as usize]
    }

    /// Port of `yyStackOverflow`: called if the stack overflows.
    fn yy_stack_overflow(&mut self, extra: &mut G::Extra) {
        while self.yytos > 0 {
            self.yy_pop_parser_stack();
        }

        // Here code is inserted which will execute if the parser stack ever overflows.
        G::stack_overflow(extra);
    }

    /// Port of `yy_shift`: perform a shift action.
    fn yy_shift(
        &mut self,
        mut yy_new_state: u16,
        yy_major: u16,
        yy_minor: G::Token,
        extra: &mut G::Extra,
    ) {
        let t = G::tables();

        self.yytos += 1;

        if self.yytos > self.yystack_end() {
            self.yytos -= 1;
            self.yy_stack_overflow(extra);
            return;
        }

        if yy_new_state > t.yy_max_shift {
            yy_new_state += t.yy_min_reduce - t.yy_min_shiftreduce;
        }

        let yytos = &mut self.yystack[self.yytos];
        yytos.stateno = yy_new_state;
        yytos.major = yy_major;
        yytos.minor = G::minor_from_token(yy_minor);
    }

    /// Port of `yy_reduce`: perform a reduce action and the shift that must immediately
    /// follow the reduce.
    fn yy_reduce(&mut self, yyruleno: u32, extra: &mut G::Extra) -> u16 {
        let t = G::tables();
        let mut yymsp = self.yytos;

        // Beginning here are the reduction cases.
        G::reduce(extra, yyruleno, &mut self.yystack, yymsp);

        debug_assert!((yyruleno as usize) < t.yy_rule_info_lhs.len());

        let yygoto = t.yy_rule_info_lhs[yyruleno as usize];
        let yysize = t.yy_rule_info_nrhs[yyruleno as usize] as isize;
        let base = (yymsp as isize + yysize) as usize;
        let yyact = Self::yy_find_reduce_action(self.yystack[base].stateno, yygoto);

        // There are no SHIFTREDUCE actions on nonterminals because the table
        // generator has simplified them to pure REDUCE actions.
        debug_assert!(!(yyact > t.yy_max_shift && yyact <= t.yy_max_shiftreduce));

        // It is not possible for a REDUCE to be followed by an error
        debug_assert!(yyact != t.yy_error_action);

        yymsp = base + 1;
        self.yytos = yymsp;
        self.yystack[yymsp].stateno = yyact;
        self.yystack[yymsp].major = yygoto;

        yyact
    }

    /// Port of `yy_parse_failed`: executes when the parse fails.
    fn yy_parse_failed(&mut self, extra: &mut G::Extra) {
        while self.yytos > 0 {
            self.yy_pop_parser_stack();
        }

        // Here code is inserted which will be executed whenever the parser fails
        G::parse_failure(extra);
    }

    /// Port of `yy_accept`: executed when the parser accepts.
    fn yy_accept(&mut self, extra: &mut G::Extra) {
        self.yyerrcnt = -1;

        debug_assert!(self.yytos == 0);

        // Here code is inserted which will be executed whenever the parser accepts
        G::parse_accept(extra);
    }

    /// Port of `Parse`, the main parser program.
    ///
    /// `yymajor` is the major token number (0 at the end of the input), `yyminor` the value
    /// of the token and `extra` the grammar's `%extra_argument`.
    pub(crate) fn parse(&mut self, yymajor: i32, yyminor: G::Token, extra: &mut G::Extra) {
        let t = G::tables();

        // yyminor is consumed by at most one shift; C passes the struct by value everywhere.
        let mut yyminor = Some(yyminor);

        // True if we are at the end of input
        let yyendofinput = yymajor == 0;

        let mut yyact = self.yystack[self.yytos].stateno;

        loop {
            // Exit by "break"
            debug_assert!(yyact == self.yystack[self.yytos].stateno);

            yyact = Self::yy_find_shift_action(yymajor as u16, yyact);

            if yyact >= t.yy_min_reduce {
                // Reduce by this rule
                let yyruleno = (yyact - t.yy_min_reduce) as u32;

                // Check that the stack is large enough to grow by a single entry
                // if the RHS of the rule is empty.  This ensures that there is room
                // enough on the stack to push the LHS value
                if t.yy_rule_info_nrhs[yyruleno as usize] == 0 && self.yytos >= self.yystack_end() {
                    self.yy_stack_overflow(extra);
                    break;
                }

                yyact = self.yy_reduce(yyruleno, extra);
            } else if yyact <= t.yy_max_shiftreduce {
                if let Some(minor) = yyminor.take() {
                    self.yy_shift(yyact, yymajor as u16, minor, extra);
                }

                self.yyerrcnt -= 1;
                break;
            } else if yyact == t.yy_accept_action {
                self.yytos -= 1;
                self.yy_accept(extra);
                return;
            } else {
                debug_assert!(yyact == t.yy_error_action);

                // This is what we do if the grammar does not define ERROR:
                //
                //  * Report an error message, and throw away the input token.
                //
                //  * If the input token is $, then fail the parse.
                //
                // As before, subsequent error messages are suppressed until
                // three input tokens have been successfully shifted.
                if self.yyerrcnt <= 0 {
                    G::syntax_error(extra, yymajor);
                }

                self.yyerrcnt = 3;

                if yyendofinput {
                    self.yy_parse_failed(extra);
                    self.yyerrcnt = -1;
                }

                break;
            }
        }
    }
}
