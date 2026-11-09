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

//! Port of `LIBEVAL::COMPILER` from `include/libeval_compiler/libeval_compiler.h` and
//! `common/libeval_compiler/libeval_compiler.cpp`: the lexer, the parse driver and the code
//! generator.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::context::Context;
use super::grammar::{
    CompilerGrammar, G_BOOL_AND, G_BOOL_NOT, G_BOOL_OR, G_COMMA, G_DIVIDE, G_ENDS, G_EQUAL,
    G_GREATER_EQUAL_THAN, G_GREATER_THAN, G_IDENTIFIER, G_LESS_EQUAL_THAN, G_LESS_THAN, G_MINUS,
    G_MULT, G_NOT_EQUAL, G_PARENL, G_PARENR, G_PLUS, G_SEMCOL, G_STRING, G_STRUCT_REF, G_UNIT,
    G_VALUE,
};
use super::tokenizer::Tokenizer;
use super::ucode::{Ucode, UcodeBinder, Uop, UopValue, VarRef, format_op_name};
use super::{
    CompilationStage, DefaultUnitResolver, ErrorStatus, NodeId, TR_ARG_LIST, TR_IDENTIFIER,
    TR_NULL, TR_NUMBER, TR_OP_BINARY_MASK, TR_OP_BOOL_AND, TR_OP_BOOL_NOT, TR_OP_BOOL_OR,
    TR_OP_FUNC_CALL, TR_OP_JNZ, TR_OP_JZ, TR_OP_METHOD_CALL, TR_STRING, TR_STRUCT_REF, TR_UNIT,
    TR_UOP_PUSH_VALUE, TR_UOP_PUSH_VAR, TToken, TTokenValue, TreeNode, UnitResolver, VarType,
};
use crate::crt;
use crate::eda_units::{EdaUnits, double_value_from_string};
use crate::lemon::YyParser;

/// Port of `enum COMPILER::LEXER_STATE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LexerState {
    /// `LS_DEFAULT`
    Default,
    /// `LS_STRING`
    String,
}

/// The error callback type of [`Compiler`] (C++:
/// `std::function<void( const wxString& aMessage, int aOffset )>`).
type ErrorCallback<'a> = Box<dyn FnMut(&str, i32) + 'a>;

/// Port of `class LIBEVAL::COMPILER`: compiles an expression string into [`Ucode`].
///
/// The lifetime `'a` is that of the error callback and the unit resolver.
///
/// As in C++, an error is *sticky*: `m_errorStatus.pendingError` is never reset, so once any
/// error (including a code generation error, for which [`Compiler::compile`] still returns
/// true) has been reported, every later non-empty `compile` on the same object fails after
/// its first token.  Use a fresh compiler per expression, as all C++ call sites do.
pub struct Compiler<'a> {
    lexer_state: LexerState,

    // Token state for input string.
    /// the current lemon parser state machine
    parser: Option<Box<YyParser<CompilerGrammar<'a>>>>,
    tokenizer: Tokenizer,
    locale_decimal_separator: char,

    unit_resolver: Box<dyn UnitResolver + 'a>,

    source_pos: i32,
    parse_finished: bool,
    error_status: ErrorStatus,

    error_callback: Option<ErrorCallback<'a>>,

    tree: Option<NodeId>,

    /// C++: `m_gcItems`, the list owning every node created since the last `Clear()`.
    /// (`m_gcStrings` has no equivalent: token strings are owned values here.)
    gc_items: Vec<TreeNode>,
}

impl Default for Compiler<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Compiler<'a> {
    /// Port of `COMPILER::COMPILER()`.
    pub fn new() -> Self {
        Compiler {
            lexer_state: LexerState::Default,
            parser: Some(Box::new(YyParser::new())),
            tokenizer: Tokenizer::default(),
            locale_decimal_separator: crt::LOCALE_DECIMAL_POINT,
            unit_resolver: Box::new(DefaultUnitResolver),
            source_pos: 0,
            parse_finished: false,
            error_status: ErrorStatus::default(),
            error_callback: None,
            tree: None,
            gc_items: Vec::new(),
        }
    }

    /// A compiler with a specific unit resolver.  C++ clients subclass `COMPILER` to replace
    /// the protected member `m_unitResolver` (see `PCBEXPR_COMPILER`).
    pub fn with_unit_resolver(unit_resolver: Box<dyn UnitResolver + 'a>) -> Self {
        let mut compiler = Self::new();
        compiler.unit_resolver = unit_resolver;
        compiler
    }

    /// Replace the unit resolver (C++: `m_unitResolver.reset( aUnitResolver )` in a subclass).
    pub fn set_unit_resolver(&mut self, unit_resolver: Box<dyn UnitResolver + 'a>) {
        self.unit_resolver = unit_resolver;
    }

    /// Port of `COMPILER::Clear()`.
    ///
    /// clear() should be invoked by the client if a new input string is to be processed. It
    /// will reset the parser. User defined variables are retained.
    pub fn clear(&mut self) {
        self.tokenizer.clear();
        self.tree = None;
        self.gc_items.clear();
    }

    /// Port of `COMPILER::parseError` (used by the lemon parser).
    pub(crate) fn parse_error(&mut self, s: &str) {
        self.report_error(CompilationStage::Parse, s, -1);
    }

    /// Port of `COMPILER::parseOk` (used by the lemon parser).
    pub(crate) fn parse_ok(&mut self) {
        self.parse_finished = true;
    }

    /// Port of `COMPILER::GetSourcePos()`.
    pub fn source_pos(&self) -> i32 {
        self.source_pos
    }

    /// Port of `COMPILER::setRoot`.
    pub(crate) fn set_root(&mut self, root: Option<NodeId>) {
        self.tree = root;
    }

    /// Port of the free function `LIBEVAL::newNode( COMPILER*, int op, const T_TOKEN_VALUE& )`.
    pub(crate) fn new_node(&mut self, op: i32, value: &TTokenValue) -> NodeId {
        let t2 = TreeNode {
            valid: true,
            value: TTokenValue {
                str: value.str.clone(),
                num: value.num,
                idx: value.idx,
            },
            op,
            leaf: [None, None],
            is_terminal: false,
            is_visited: false,
            src_pos: self.source_pos(),
        };

        self.gc_items.push(t2);
        NodeId(self.gc_items.len() - 1)
    }

    /// The root of the syntax tree of the last successful parse (C++: the protected member
    /// `m_tree`); `None` for an empty expression or after a parse error.
    pub fn tree(&self) -> Option<NodeId> {
        self.tree
    }

    /// The node behind a handle, or `None` if the handle does not belong to the current
    /// expression.
    pub fn node(&self, id: NodeId) -> Option<&TreeNode> {
        self.gc_items.get(id.0)
    }

    /// Mutable access to a node created during the current compilation.
    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut TreeNode {
        &mut self.gc_items[id.0]
    }

    /// Shared access to a node of the current tree.
    fn n(&self, id: NodeId) -> &TreeNode {
        &self.gc_items[id.0]
    }

    /// `Parse( m_parser, token, tok, this )`
    fn parse(&mut self, major: i32, tok: TToken) {
        // The parser is a member of the compiler but its actions call back into the compiler,
        // so it is moved out for the duration of the call.
        if let Some(mut parser) = self.parser.take() {
            parser.parse(major, tok, self);
            self.parser = Some(parser);
        }
    }

    /// Port of `COMPILER::Compile( const wxString& aString, UCODE* aCode, CONTEXT* aPreflightContext )`.
    ///
    /// Returns false on a lexical or syntax error.  Code generation errors (unknown items,
    /// properties, functions, units...) are reported through the error callback and
    /// [`Compiler::error`] / [`Compiler::is_error_pending`] but do *not* make the function
    /// return false; microcode is still generated (with safe placeholder ops), exactly as in
    /// C++.  The ops are appended to `code`.
    ///
    /// `preflight_context` is used to call every function whose arguments are all literals
    /// once at compile time (with the arguments as string values and without items) so that
    /// functions can validate their arguments; the errors they report are reported as code
    /// generation errors.  Its error callback is replaced during those calls and removed
    /// afterwards (C++ leaves a callback with dangling captures installed).
    pub fn compile<'e, B: UcodeBinder>(
        &mut self,
        string: &str,
        code: &mut Ucode<B>,
        preflight_context: &mut Context<'e, B>,
    ) -> bool {
        // Feed parser token after token until end of input.

        self.new_string(string);

        self.tree = None;
        self.parse_finished = false;

        if string.is_empty() {
            self.parse_finished = true;
            return self.generate_ucode(code, preflight_context);
        }

        loop {
            self.source_pos = self.tokenizer.pos() as i32;

            let tok = self.get_token();
            let token = tok.token;

            self.parse(token, tok);

            if self.error_status.pending_error {
                return false;
            }

            if self.parse_finished || token == G_ENDS {
                // Reset parser by passing zero as token ID, value is ignored.
                self.parse(0, TToken::default());
                break;
            }

            if token == 0 {
                break;
            }
        }

        self.generate_ucode(code, preflight_context)
    }

    /// Port of `COMPILER::SetErrorCallback`.  The callback receives every error message with
    /// its character offset into the expression; `None` removes it.
    pub fn set_error_callback(&mut self, callback: Option<ErrorCallback<'a>>) {
        self.error_callback = callback;
    }

    /// Port of `COMPILER::IsErrorPending()`.
    pub fn is_error_pending(&self) -> bool {
        self.error_status.pending_error
    }

    /// Port of `COMPILER::GetError()`: the most recent error.
    pub fn error(&self) -> &ErrorStatus {
        &self.error_status
    }

    /// Port of `COMPILER::reportError( COMPILATION_STAGE stage, const wxString& aErrorMsg, int aPos = -1 )`.
    fn report_error(&mut self, stage: CompilationStage, error_msg: &str, mut pos: i32) {
        if pos == -1 {
            pos = self.source_pos;
        }

        self.error_status.pending_error = true;
        self.error_status.stage = stage;
        self.error_status.message = error_msg.to_string();
        self.error_status.src_pos = pos;

        if let Some(callback) = self.error_callback.as_mut() {
            callback(error_msg, pos);
        }
    }

    /// Port of `COMPILER::newString`: begin processing of a new input string.
    fn new_string(&mut self, string: &str) {
        self.clear();

        self.lexer_state = LexerState::Default;
        self.tokenizer.restart(string);
        self.parse_finished = false;
    }

    /// Port of `COMPILER::getToken`: next token/value taken from input string.
    fn get_token(&mut self) -> TToken {
        // rv.value = defaultTokenValue;
        let mut rv = TToken::default();

        loop {
            let done = match self.lexer_state {
                LexerState::Default => self.lex_default(&mut rv),
                LexerState::String => self.lex_string(&mut rv),
            };

            if done {
                break;
            }
        }

        rv
    }

    /// Port of `COMPILER::lexString`.
    fn lex_string(&mut self, token: &mut TToken) -> bool {
        token.token = G_STRING;
        token.value.str = Some(self.tokenizer.get_string());

        self.lexer_state = LexerState::Default;
        true
    }

    /// Port of `COMPILER::resolveUnits`: if the input continues with one of the resolver's
    /// unit names (followed by a non-alphanumeric character or the end), consume it and
    /// return its index; -1 otherwise.
    fn resolve_units(&mut self) -> i32 {
        for (unit_id, unit_name) in self.unit_resolver.supported_units().iter().enumerate() {
            if self.tokenizer.match_ahead(unit_name, |c| !crt::is_alnum(c)) {
                self.tokenizer.next_char(unit_name.chars().count());
                return unit_id as i32;
            }
        }

        -1
    }

    /// Port of `COMPILER::lexDefault`.
    fn lex_default(&mut self, token: &mut TToken) -> bool {
        let mut retval = TToken {
            token: G_ENDS,
            value: TTokenValue {
                str: None,
                num: 0.0,
                idx: -1,
            },
        };
        let mut current = String::new();

        if self.tokenizer.done() {
            *token = retval;
            return true;
        }

        let locale_decimal_separator = self.locale_decimal_separator;
        let is_decimal_separator =
            move |ch: char| -> bool { ch == locale_decimal_separator || ch == '.' || ch == ',' };

        // Start processing of first/next token: Remove whitespace
        let mut ch;

        loop {
            ch = self.tokenizer.get_char();

            if ch == ' ' {
                self.tokenizer.next_char(1);
            } else {
                break;
            }
        }

        if ch == '\0' {
            /* End of input */
        } else if crt::is_digit(ch) {
            // VALUE
            // Lambda extractNumber: get value as string, store into current and update the
            // current index.
            let mut have_separator = false;
            let mut ch = self.tokenizer.get_char();

            loop {
                if is_decimal_separator(ch) && have_separator {
                    break;
                }

                current.push(ch);

                if is_decimal_separator(ch) {
                    have_separator = true;
                }

                self.tokenizer.next_char(1);
                ch = self.tokenizer.get_char();

                if !(crt::is_digit(ch) || is_decimal_separator(ch)) {
                    break;
                }
            }

            // Ensure that the systems decimal separator is used
            current = current
                .chars()
                .map(|c| {
                    if is_decimal_separator(c) {
                        locale_decimal_separator
                    } else {
                        c
                    }
                })
                .collect();

            retval.token = G_VALUE;
            retval.value.str = Some(current);
        } else if let convert_from @ 0.. = self.resolve_units() {
            // UNIT
            // Units are appended to a VALUE.
            // Determine factor to default unit if unit for value is given.
            // Example: Default is mm, unit is inch: factor is 25.4
            // The factor is assigned to the terminal UNIT. The actual
            // conversion is done within a parser action.
            retval.token = G_UNIT;
            retval.value.idx = convert_from;
        } else if ch == '\'' {
            // string literal
            self.lexer_state = LexerState::String;
            self.tokenizer.next_char(1);
            return false;
        } else if crt::is_alpha(ch) || ch == '_' {
            current = self.tokenizer.get_chars(|c| crt::is_alnum(c) || c == '_');
            self.tokenizer.next_char(current.chars().count());
            retval.token = G_IDENTIFIER;
            retval.value.str = Some(current);
        } else if self.tokenizer.match_ahead("==", |c| c != '=') {
            retval.token = G_EQUAL;
            self.tokenizer.next_char(2);
        } else if self.tokenizer.match_ahead("!=", |c| c != '=') {
            retval.token = G_NOT_EQUAL;
            self.tokenizer.next_char(2);
        } else if self.tokenizer.match_ahead("<=", |c| c != '=') {
            retval.token = G_LESS_EQUAL_THAN;
            self.tokenizer.next_char(2);
        } else if self.tokenizer.match_ahead(">=", |c| c != '=') {
            retval.token = G_GREATER_EQUAL_THAN;
            self.tokenizer.next_char(2);
        } else if self.tokenizer.match_ahead("&&", |c| c != '&') {
            retval.token = G_BOOL_AND;
            self.tokenizer.next_char(2);
        } else if self.tokenizer.match_ahead("||", |c| c != '|') {
            retval.token = G_BOOL_OR;
            self.tokenizer.next_char(2);
        } else {
            // Single char tokens
            match ch {
                '+' => retval.token = G_PLUS,
                '!' => retval.token = G_BOOL_NOT,
                '-' => retval.token = G_MINUS,
                '*' => retval.token = G_MULT,
                '/' => retval.token = G_DIVIDE,
                '<' => retval.token = G_LESS_THAN,
                '>' => retval.token = G_GREATER_THAN,
                '(' => retval.token = G_PARENL,
                ')' => retval.token = G_PARENR,
                ';' => retval.token = G_SEMCOL,
                '.' => retval.token = G_STRUCT_REF,
                ',' => retval.token = G_COMMA,

                _ => {
                    if self.tokenizer.match_ahead("${", |c| c != '{') {
                        self.report_error(
                            CompilationStage::Parse,
                            "Unresolved text variable reference",
                            self.source_pos + 2,
                        );
                    } else {
                        self.report_error(
                            CompilationStage::Parse,
                            &format!("Unrecognized character '{}'", char_cast(ch)),
                            -1,
                        );
                    }
                }
            }

            self.tokenizer.next_char(1);
        }

        *token = retval;
        true
    }

    /// Port of the file-static `formatNode`.
    fn format_node(&self, node: NodeId) -> String {
        self.n(node).value.str.clone().unwrap_or_default()
    }

    /// A compact rendering of the syntax tree of the last parse, for tests and diagnostics.
    ///
    /// This is not a port of the C++ trace helper `dumpNode` (which prints node addresses and
    /// skips some node kinds); the format is: `N[text]` number (`N[text u<idx>]` with a unit,
    /// `N[]` for the implicit zero of a unary minus), `S[text]` string, `I[name]`
    /// identifier, `NULL` empty argument list, and `(OP left right)` for everything else
    /// with OP one of the microcode mnemonics, `NOT`, `SREF`, `ARGS` or `CALL`.
    pub fn format_tree(&self) -> String {
        enum Work {
            Node(Option<NodeId>),
            Text(&'static str),
        }

        let mut out = String::new();
        let mut work = vec![Work::Node(self.tree)];

        while let Some(item) = work.pop() {
            let id = match item {
                Work::Text(text) => {
                    out.push_str(text);
                    continue;
                }
                Work::Node(None) => {
                    out.push('-');
                    continue;
                }
                Work::Node(Some(id)) => id,
            };

            let Some(node) = self.node(id) else {
                out.push('?');
                continue;
            };

            let text = node.value.str.as_deref().unwrap_or("");

            match node.op {
                TR_NUMBER => {
                    out.push_str("N[");
                    out.push_str(text);

                    if let Some(unit) = node.leaf[0].and_then(|leaf| self.node(leaf)) {
                        if unit.op == TR_UNIT {
                            out.push_str(&format!(" u{}", unit.value.idx));
                        }
                    }

                    out.push(']');
                }
                TR_STRING => out.push_str(&format!("S[{text}]")),
                TR_IDENTIFIER => out.push_str(&format!("I[{text}]")),
                TR_NULL => out.push_str("NULL"),
                op => {
                    let name = match op {
                        TR_OP_BOOL_NOT => "NOT",
                        TR_STRUCT_REF => "SREF",
                        TR_ARG_LIST => "ARGS",
                        TR_OP_FUNC_CALL => "CALL",
                        TR_UNIT => "UNIT",
                        _ => format_op_name(op),
                    };

                    out.push('(');
                    out.push_str(name);
                    work.push(Work::Text(")"));

                    if node.leaf[1].is_some() || op & TR_OP_BINARY_MASK != 0 {
                        work.push(Work::Node(node.leaf[1]));
                        work.push(Work::Text(" "));
                    }

                    work.push(Work::Node(node.leaf[0]));
                    work.push(Work::Text(" "));
                }
            }
        }

        out
    }

    /// Port of the file-static `prepareTree`: clear the visited flags of the tree.
    ///
    /// (Iterative instead of recursive so that very deep trees cannot overflow the stack.)
    fn prepare_tree(&mut self, root: NodeId) {
        let mut pending = vec![root];

        while let Some(node) = pending.pop() {
            self.node_mut(node).is_visited = false;

            // fixme: for reasons I don't understand the lemon parser isn't initializing the
            // leaf node pointers of function name nodes.  -JY
            if self.n(node).op == TR_OP_FUNC_CALL {
                if let Some(name) = self.n(node).leaf[0] {
                    self.node_mut(name).leaf = [None, None];
                }
            }

            let leaf = self.n(node).leaf;

            if let Some(leaf1) = leaf[1] {
                pending.push(leaf1);
            }

            if let Some(leaf0) = leaf[0] {
                pending.push(leaf0);
            }
        }
    }

    /// Port of the file-static `squashParamList`: the argument nodes of a call, in source
    /// order.
    fn squash_param_list(&self, root: Option<NodeId>) -> Vec<NodeId> {
        let mut args = Vec::new();

        let Some(root) = root else {
            return args;
        };

        if self.n(root).op != TR_ARG_LIST && self.n(root).op != TR_NULL {
            args.push(root);
        } else {
            let mut n = Some(root);

            // do { ... } while( n && n->op == TR_ARG_LIST );
            while let Some(current) = n {
                if let Some(leaf1) = self.n(current).leaf[1] {
                    args.push(leaf1);
                }

                n = self.n(current).leaf[0];

                if !matches!(n, Some(next) if self.n(next).op == TR_ARG_LIST) {
                    break;
                }
            }

            if let Some(last) = n {
                args.push(last);
            }
        }

        args.reverse();

        args
    }

    /// Port of the file-static `flattenVarChain`.
    ///
    /// Flatten a pure identifier chain (e.g. "A" or "A.Parent") that forms the receiver of a
    /// property or method access into a dotted variable name.  Returns `None` when the receiver
    /// is anything else (for example the result of another method call), which is unsupported.
    /// Every node consumed is marked visited so the code generator does not re-process it.
    ///
    /// (Iterative instead of recursive; nodes are marked only on success, like in C++ where
    /// the marking happens after the recursive call has succeeded.)
    fn flatten_var_chain(&mut self, node: NodeId) -> Option<String> {
        let mut chain = Vec::new();
        let mut current = node;

        loop {
            if self.n(current).op == TR_IDENTIFIER {
                break;
            }

            let leaf = self.n(current).leaf;

            match (self.n(current).op, leaf[0], leaf[1]) {
                (TR_STRUCT_REF, Some(leaf0), Some(leaf1)) if self.n(leaf1).op == TR_IDENTIFIER => {
                    chain.push((current, leaf1));
                    current = leaf0;
                }
                _ => return None,
            }
        }

        let mut result = self.format_node(current);
        self.node_mut(current).is_visited = true;

        for (sref, field) in chain.into_iter().rev() {
            result.push('.');
            result.push_str(&self.format_node(field));
            self.node_mut(field).is_visited = true;
            self.node_mut(sref).is_visited = true;
        }

        Some(result)
    }

    /// Port of `COMPILER::generateUCode( UCODE* aCode, CONTEXT* aPreflightContext )`.
    fn generate_ucode<'e, B: UcodeBinder>(
        &mut self,
        code: &mut Ucode<B>,
        preflight_context: &mut Context<'e, B>,
    ) -> bool {
        let mut stack: Vec<NodeId> = Vec::new();
        let mut numeric_value_count = 0;
        let mut missing_units_msg = String::new();
        let mut missing_units_src_pos = 0;

        // Short-circuit jumps for && / || awaiting their target backpatched once the right-hand
        // operand's microcode has been emitted.  (C++ maps the node to the UOP*; here the value
        // is the index of the jump op in `code`.)
        let mut sc_jumps: BTreeMap<NodeId, usize> = BTreeMap::new();

        let Some(tree) = self.tree else {
            // Empty expression returns true
            code.add_op(push_value_uop(1.0, EdaUnits::Unscaled));
            return true;
        };

        self.prepare_tree(tree);

        stack.push(tree);

        // C++ stores the op being built for a node in TREE_NODE::uop.
        let mut uops: Vec<Option<Uop<B>>> = Vec::new();
        uops.resize_with(self.gc_items.len(), || None);

        while let Some(&node) = stack.last() {
            let leaf = self.n(node).leaf;

            // process terminal nodes first
            match self.n(node).op {
                TR_OP_FUNC_CALL => {
                    // Function call's uop was generated inside TR_STRUCT_REF
                    if uops[node.0].is_none() {
                        // This function call is bare so we don't know who to apply it to
                        // Set a safe default value to exit gracefully with an error
                        let src_pos = self.n(node).src_pos;
                        self.report_error(
                            CompilationStage::Codegen,
                            "Unknown parent of function parameters",
                            src_pos,
                        );
                        uops[node.0] = Some(push_value_uop(0.0, EdaUnits::Unscaled));
                    }

                    self.node_mut(node).is_terminal = true;
                }

                TR_STRUCT_REF => 'struct_ref: {
                    // leaf[0]: object (a variable name, or a "." chain such as "A.Parent")
                    // leaf[1]: field (TR_IDENTIFIER) or TR_OP_FUNC_CALL

                    // The grammar always sets both leaves of a TR_STRUCT_REF node.
                    let (Some(leaf0), Some(leaf1)) = (leaf[0], leaf[1]) else {
                        uops[node.0] = Some(push_value_uop(0.0, EdaUnits::Unscaled));
                        self.node_mut(node).is_terminal = true;
                        break 'struct_ref;
                    };

                    let Some(item_name) = self.flatten_var_chain(leaf0) else {
                        let mut pos = self.n(leaf0).src_pos;

                        if self.n(leaf0).value.str.is_some() {
                            pos -= self.format_node(leaf0).chars().count() as i32;
                        }

                        self.report_error(
                            CompilationStage::Codegen,
                            "Unknown parent of property",
                            pos,
                        );

                        self.node_mut(leaf0).is_visited = true;
                        self.node_mut(leaf1).is_visited = true;

                        uops[node.0] = Some(push_value_uop(0.0, EdaUnits::Unscaled));
                        self.node_mut(node).is_terminal = true;
                        break 'struct_ref;
                    };

                    let item_name_len = item_name.chars().count() as i32;

                    match self.n(leaf1).op {
                        TR_IDENTIFIER => {
                            // leaf[0]: object
                            // leaf[1]: field

                            let prop_name = self.format_node(leaf1);
                            let vref = code.create_var_ref(&item_name, &prop_name);

                            match &vref {
                                None => {
                                    let msg = format!("Unrecognized item '{item_name}'");
                                    let pos = self.n(leaf0).src_pos - item_name_len;
                                    self.report_error(CompilationStage::Codegen, &msg, pos);
                                }
                                Some(vref) if vref.type_() == VarType::ParseError => {
                                    let msg = format!("Unrecognized property '{prop_name}'");
                                    let pos =
                                        self.n(leaf1).src_pos - prop_name.chars().count() as i32;
                                    self.report_error(CompilationStage::Codegen, &msg, pos);
                                }
                                Some(_) => {}
                            }

                            self.node_mut(leaf1).is_visited = true;

                            uops[node.0] = Some(Uop::with_ref(TR_UOP_PUSH_VAR, vref));
                            self.node_mut(node).is_terminal = true;
                        }

                        TR_OP_FUNC_CALL => {
                            // leaf[0]: object
                            // leaf[1]: TR_OP_FUNC_CALL
                            //    leaf[0]: function name
                            //    leaf[1]: parameter

                            let vref = code.create_var_ref(&item_name, "");

                            if vref.is_none() {
                                let msg = format!("Unrecognized item '{item_name}'");
                                let pos = self.n(leaf0).src_pos - item_name_len;
                                self.report_error(CompilationStage::Codegen, &msg, pos);
                            }

                            let name_node = self.n(leaf1).leaf[0];
                            let params_root = self.n(leaf1).leaf[1];

                            let function_name =
                                name_node.map(|n| self.format_node(n)).unwrap_or_default();
                            let func = code.create_func_call(&function_name);
                            let params = self.squash_param_list(params_root);

                            if func.is_none() {
                                let msg = format!("Unrecognized function '{function_name}'");
                                let pos = self.n(leaf0).src_pos + 1;
                                self.report_error(CompilationStage::Codegen, &msg, pos);
                            }

                            // Expressions have no value until runtime; formatNode() would turn
                            // them into empty strings and falsely report missing arguments
                            let literal_args = params.iter().all(|param| {
                                self.n(*param).op == TR_STRING || self.n(*param).op == TR_NUMBER
                            });

                            if let (Some(func), true) = (&func, literal_args) {
                                for pnode in &params {
                                    let param = preflight_context.alloc_value();
                                    let text = self.format_node(*pnode);
                                    preflight_context.value_mut(param).set_string(text);
                                    preflight_context.push(param);
                                }

                                // C++ installs a callback that calls reportError() directly.
                                // The compiler cannot be borrowed by the context here, so the
                                // messages are collected and reported, in order, right after
                                // the call.
                                let messages: Rc<RefCell<Vec<String>>> =
                                    Rc::new(RefCell::new(Vec::new()));
                                let sink = Rc::clone(&messages);

                                preflight_context.set_error_callback(Some(Box::new(
                                    move |message: &str, _offset: i32| {
                                        sink.borrow_mut().push(message.to_string());
                                    },
                                )));

                                func.call(preflight_context, vref.as_ref());
                                preflight_context.pop(); // return value

                                preflight_context.set_error_callback(None);

                                let loc = params_root.map(|p| self.n(p).src_pos).unwrap_or(0);

                                for message in messages.take() {
                                    self.report_error(CompilationStage::Codegen, &message, loc - 1);
                                }
                            }

                            self.node_mut(leaf0).is_visited = true;
                            self.node_mut(leaf1).is_visited = true;

                            if let Some(name_node) = name_node {
                                self.node_mut(name_node).is_visited = true;
                            }

                            if let Some(params_root) = params_root {
                                self.node_mut(params_root).is_visited = true;
                            }

                            // Our non-terminal-node stacking algorithm can't handle doubly-nested
                            // structures so we need to pop a level by replacing the TR_STRUCT_REF with
                            // a TR_OP_FUNC_CALL and its function parameter
                            stack.pop();
                            stack.push(leaf1);

                            for pnode in &params {
                                stack.push(*pnode);
                            }

                            uops[leaf1.0] = Some(Uop::with_func(TR_OP_METHOD_CALL, func, vref));
                            self.node_mut(node).is_terminal = false;
                        }

                        _ => {
                            // leaf[0]: object
                            // leaf[1]: malformed syntax

                            let prop_name = self.format_node(leaf1);
                            let vref = code.create_var_ref(&item_name, &prop_name);

                            if vref.is_none() {
                                let msg = format!("Unrecognized item '{item_name}'");
                                let pos = self.n(leaf0).src_pos - item_name_len;
                                self.report_error(CompilationStage::Codegen, &msg, pos);
                            }

                            let msg = format!("Unrecognized property '{prop_name}'");
                            let pos = self.n(leaf0).src_pos + 1;
                            self.report_error(CompilationStage::Codegen, &msg, pos);

                            self.node_mut(leaf0).is_visited = true;
                            self.node_mut(leaf1).is_visited = true;

                            uops[node.0] = Some(push_value_uop(0.0, EdaUnits::Unscaled));
                            self.node_mut(node).is_terminal = true;
                        }
                    }
                }

                TR_NUMBER => {
                    let son = leaf[0];
                    let value;
                    let mut units_type = EdaUnits::Unscaled;

                    if self.n(node).value.str.is_none() {
                        value = 0.0;
                    } else if let Some(son) = son.filter(|son| self.n(*son).op == TR_UNIT) {
                        if self.unit_resolver.supported_units().is_empty() {
                            let msg = format!("Unexpected units for '{}'", self.format_node(node));
                            let src_pos = self.n(node).src_pos;
                            self.report_error(CompilationStage::Codegen, &msg, src_pos);
                        }

                        let units = self.n(son).value.idx;

                        // C++ uses std::vector::at(), which throws if the resolver's two lists
                        // disagree; fall back to UNSCALED instead.
                        units_type = usize::try_from(units)
                            .ok()
                            .and_then(|i| self.unit_resolver.supported_units_types().get(i))
                            .copied()
                            .unwrap_or(EdaUnits::Unscaled);
                        value = self.unit_resolver.convert(&self.format_node(node), units);
                        self.node_mut(son).is_visited = true;
                    } else {
                        let units_message = self.unit_resolver.supported_units_message();

                        if !units_message.is_empty() {
                            missing_units_msg = format!(
                                "Missing units for '{}'| ({})",
                                self.format_node(node),
                                units_message
                            );
                            missing_units_src_pos = self.n(node).src_pos;
                        }

                        value = double_value_from_string(&self.format_node(node));
                    }

                    uops[node.0] = Some(push_value_uop(value, units_type));
                    self.node_mut(node).is_terminal = true;
                    numeric_value_count += 1;
                }

                TR_STRING => {
                    let str = self.format_node(node);
                    let is_wildcard = str.contains('?') || str.contains('*');
                    uops[node.0] = Some(Uop::with_value(
                        TR_UOP_PUSH_VALUE,
                        Some(UopValue::String {
                            value: str,
                            is_wildcard,
                        }),
                    ));
                    self.node_mut(node).is_terminal = true;
                }

                TR_IDENTIFIER => {
                    let name = self.format_node(node);
                    let vref = code.create_var_ref(&name, "");

                    if vref.is_none() {
                        let msg = format!("Unrecognized item '{name}'");
                        let pos = self.n(node).src_pos - name.chars().count() as i32;
                        self.report_error(CompilationStage::Codegen, &msg, pos);
                    }

                    uops[node.0] = Some(Uop::with_ref(TR_UOP_PUSH_VAR, vref));
                    self.node_mut(node).is_terminal = true;
                }

                op => {
                    uops[node.0] = Some(Uop::with_ref(op, None));

                    let is_terminal = leaf[0].is_none_or(|l| self.n(l).is_visited)
                        && leaf[1].is_none_or(|l| self.n(l).is_visited);
                    self.node_mut(node).is_terminal = is_terminal;
                }
            }

            if !self.n(node).is_terminal {
                if let Some(leaf0) = leaf[0].filter(|l| !self.n(*l).is_visited) {
                    stack.push(leaf0);
                    self.node_mut(leaf0).is_visited = true;
                } else if let Some(leaf1) = leaf[1].filter(|l| !self.n(*l).is_visited) {
                    // The left operand's microcode is now fully emitted.  For && / || insert the
                    // short-circuit jump here, before the right operand, so the right side can be
                    // skipped at run time when the left already decides the result.
                    let op = self.n(node).op;

                    if op == TR_OP_BOOL_AND || op == TR_OP_BOOL_OR {
                        let jump = Uop::new(if op == TR_OP_BOOL_AND {
                            TR_OP_JZ
                        } else {
                            TR_OP_JNZ
                        });
                        sc_jumps.insert(node, code.size() as usize);
                        code.add_op(jump);
                        code.mark_has_jumps();
                    }

                    stack.push(leaf1);
                    self.node_mut(leaf1).is_visited = true;
                }

                continue;
            }

            self.node_mut(node).is_visited = true;

            if let Some(uop) = uops[node.0].take() {
                code.add_op(uop);

                // Backpatch the matching short-circuit jump to land just past this combine op.
                if let Some(jump) = sc_jumps.get(&node) {
                    code.set_jump_target(*jump, code.size());
                }
            }

            stack.pop();
        }

        // Report the common error condition of a single numeric value with no units (which will result in
        // nanometers, and rarely leads to anything useful).
        // Reporting missing units with multiple numeric values is far more complicated as we have to
        // separate comparison operators from multiplicative operators from additive operators.  Consider:
        //   2 * 1.5mm
        //   2mm + 105um
        //   2mm > 20um
        //   (2mm + 1.5mm) * 3
        if !missing_units_msg.is_empty() && numeric_value_count == 1 {
            self.report_error(
                CompilationStage::Codegen,
                &missing_units_msg,
                missing_units_src_pos,
            );
        }

        true
    }
}

/// `node->SetUop( TR_UOP_PUSH_VALUE, aValue, aUnits )` (port of
/// `TREE_NODE::SetUop( int aOp, double aValue, EDA_UNITS aUnits )`).
fn push_value_uop<B: UcodeBinder>(value: f64, units: EdaUnits) -> Uop<B> {
    Uop::with_value(TR_UOP_PUSH_VALUE, Some(UopValue::Numeric { value, units }))
}

/// What `wxString::Format( "%c", (char) ch )` prints for a character: the C++ code truncates
/// the code point to its low byte, and wxWidgets turns a byte above 0x7F that is not a valid
/// character on its own (always the case in a UTF-8 locale) into `?`.
fn char_cast(ch: char) -> char {
    let byte = (ch as u32 & 0xFF) as u8;

    if byte < 0x80 { byte as char } else { '?' }
}
