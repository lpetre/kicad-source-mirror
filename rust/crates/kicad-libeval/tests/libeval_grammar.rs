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

//! Tests of the expression grammar (`common/libeval_compiler/grammar.lemon`): the syntax
//! tree built for every construct, and operator precedence and associativity for all pairs
//! of operators.
//!
//! The expected trees are derived here from the precedence declarations of the grammar file
//! and Lemon's conflict resolution rules, independently of the parse tables the
//! implementation runs on; `tests/oracle_golden.rs` additionally checks the same operator
//! matrix against the C++ implementation.

use kicad_libeval::{Compiler, Context, DefaultBinder, Ucode};

/// Parse `expr` and return the rendering of its syntax tree (see `Compiler::format_tree`),
/// or the position of the syntax error.
fn parse(expr: &str) -> Result<String, i32> {
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut preflight = Context::new(());

    if compiler.compile(expr, &mut code, &mut preflight) {
        Ok(compiler.format_tree())
    } else {
        assert_eq!(compiler.error().message, "Syntax error", "{expr}");
        Err(compiler.error().src_pos)
    }
}

fn tree(expr: &str) -> String {
    parse(expr).unwrap_or_else(|pos| panic!("syntax error at {pos} in '{expr}'"))
}

/* --------------------------------------------------------------------------------------------
 * One test per grammar rule
 */

#[test]
fn terminals() {
    // nt ::= G_VALUE
    assert_eq!(tree("1"), "N[1]");
    assert_eq!(tree("1.5"), "N[1.5]");
    // both separators are normalised to the locale's decimal separator
    assert_eq!(tree("1,5"), "N[1.5]");
    // nt ::= G_STRING
    assert_eq!(tree("'abc'"), "S[abc]");
    assert_eq!(tree("''"), "S[]");
    // nt ::= G_IDENTIFIER
    assert_eq!(tree("A"), "I[A]");
    assert_eq!(tree("_a1"), "I[_a1]");
}

#[test]
fn value_with_unit() {
    // nt ::= G_VALUE G_UNIT needs a unit resolver that knows the unit; the index of the unit
    // in the resolver's list is stored in the TR_UNIT leaf.
    struct Resolver;

    impl kicad_libeval::UnitResolver for Resolver {
        fn supported_units(&self) -> &[&str] {
            &["mil", "mm"]
        }

        fn supported_units_types(&self) -> &[kicad_libeval::EdaUnits] {
            &[kicad_libeval::EdaUnits::Mils, kicad_libeval::EdaUnits::Mm]
        }

        fn convert(&self, string: &str, unit_type: i32) -> f64 {
            let v: f64 = string.parse().unwrap();
            if unit_type == 0 { v * 25400.0 } else { v * 1e6 }
        }
    }

    let parse = |expr: &str| {
        let mut compiler = Compiler::with_unit_resolver(Box::new(Resolver));
        let mut code = Ucode::new(DefaultBinder);
        let mut preflight = Context::new(());

        compiler
            .compile(expr, &mut code, &mut preflight)
            .then(|| compiler.format_tree())
    };

    assert_eq!(parse("1mm").as_deref(), Some("N[1 u1]"));
    assert_eq!(parse("1 mm").as_deref(), Some("N[1 u1]"));
    assert_eq!(parse("2mil").as_deref(), Some("N[2 u0]"));
    assert_eq!(
        parse("1mm + 2mil").as_deref(),
        Some("(ADD N[1 u1] N[2 u0])")
    );
    assert_eq!(parse("-1mm").as_deref(), Some("(SUB N[] N[1 u1])"));
    // a unit must follow a number directly
    assert_eq!(parse("mm"), None);
    assert_eq!(parse("1mm mm"), None);
    assert_eq!(parse("(1)mm"), None);
    assert_eq!(parse("A mm"), None);
    // "mmx" and "mm1" are identifiers, "mm_" is the unit followed by the identifier "_"
    assert_eq!(parse("mmx").as_deref(), Some("I[mmx]"));
    assert_eq!(parse("mm1").as_deref(), Some("I[mm1]"));
    assert_eq!(parse("1mm_"), None);

    // Without resolver "mm" is just an identifier.
    assert_eq!(tree("mm"), "I[mm]");
    assert_eq!(crate::parse("1mm"), Err(1));
}

#[test]
fn binary_rules() {
    assert_eq!(tree("1 , 2"), "(ARGS N[1] N[2])");
    assert_eq!(tree("1 < 2"), "(LESS N[1] N[2])");
    assert_eq!(tree("1 > 2"), "(GREATER N[1] N[2])");
    assert_eq!(tree("1 <= 2"), "(LESS_EQUAL N[1] N[2])");
    assert_eq!(tree("1 >= 2"), "(GREATER_EQUAL N[1] N[2])");
    assert_eq!(tree("1 != 2"), "(NEQUAL N[1] N[2])");
    assert_eq!(tree("1 && 2"), "(AND N[1] N[2])");
    assert_eq!(tree("1 || 2"), "(OR N[1] N[2])");
    assert_eq!(tree("1 + 2"), "(ADD N[1] N[2])");
    assert_eq!(tree("1 - 2"), "(SUB N[1] N[2])");
    assert_eq!(tree("1 * 2"), "(MUL N[1] N[2])");
    assert_eq!(tree("1 / 2"), "(DIV N[1] N[2])");
    assert_eq!(tree("1 == 2"), "(EQUAL N[1] N[2])");
    assert_eq!(tree("A . B"), "(SREF I[A] I[B])");
    assert_eq!(tree("A.B"), "(SREF I[A] I[B])");
}

#[test]
fn unary_rules() {
    // nt ::= G_BOOL_NOT nt
    assert_eq!(tree("!1"), "(NOT N[1])");
    assert_eq!(tree("!!1"), "(NOT (NOT N[1]))");
    // nt ::= G_MINUS nt builds "0 - nt"; the zero is a number node without text
    assert_eq!(tree("-1"), "(SUB N[] N[1])");
    assert_eq!(tree("--1"), "(SUB N[] (SUB N[] N[1]))");
    // nt ::= G_PLUS nt and nt ::= G_PARENL nt G_PARENR create a copy of the operand
    assert_eq!(tree("+1"), "N[1]");
    assert_eq!(tree("++1"), "N[1]");
    assert_eq!(tree("(1)"), "N[1]");
    assert_eq!(tree("((1))"), "N[1]");
    assert_eq!(tree("(1 + 2)"), "(ADD N[1] N[2])");
    assert_eq!(tree("+(1 + 2)"), "(ADD N[1] N[2])");
    assert_eq!(tree("-(1 + 2)"), "(SUB N[] (ADD N[1] N[2]))");
    assert_eq!(tree("!-+1"), "(NOT (SUB N[] N[1]))");
}

#[test]
fn function_call_rules() {
    // nt ::= G_IDENTIFIER G_PARENL G_PARENR
    assert_eq!(tree("f()"), "(CALL I[f] NULL)");
    assert_eq!(tree("f ( )"), "(CALL I[f] NULL)");
    // nt ::= G_IDENTIFIER G_PARENL nt G_PARENR
    assert_eq!(tree("f(1)"), "(CALL I[f] N[1])");
    assert_eq!(tree("f('a' , 'b')"), "(CALL I[f] (ARGS S[a] S[b]))");
    assert_eq!(
        tree("f(1 , 2 , 3)"),
        "(CALL I[f] (ARGS (ARGS N[1] N[2]) N[3]))"
    );
    assert_eq!(tree("f(1 + 2)"), "(CALL I[f] (ADD N[1] N[2]))");
    assert_eq!(tree("f(g())"), "(CALL I[f] (CALL I[g] NULL))");
    // method calls are a struct reference whose right side is a call
    assert_eq!(tree("A.f()"), "(SREF I[A] (CALL I[f] NULL))");
    assert_eq!(tree("A.f('x')"), "(SREF I[A] (CALL I[f] S[x]))");
    assert_eq!(tree("A.B.f()"), "(SREF (SREF I[A] I[B]) (CALL I[f] NULL))");
    assert_eq!(
        tree("A.f().g()"),
        "(SREF (SREF I[A] (CALL I[f] NULL)) (CALL I[g] NULL))"
    );
    // only an identifier can be called
    assert_eq!(parse("1()"), Err(1));
    assert_eq!(parse("'f'()"), Err(3));
    assert_eq!(parse("(f)()"), Err(3));
    assert_eq!(parse("f()()"), Err(3));
    assert_eq!(parse("A.f()()"), Err(5));
    // "1,2" is the number 1.2 (the comma is a decimal separator) ...
    assert_eq!(tree("f(1,2)"), "(CALL I[f] N[1.2])");
    // ... and so is the "1," of "1, 2", which leaves two numbers in a row
    assert_eq!(parse("f(1, 2)"), Err(4));
}

#[test]
fn statements() {
    // stmt ::= G_ENDS: nothing but white space gives no tree at all
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut preflight = Context::new(());

    assert!(compiler.compile("   ", &mut code, &mut preflight));
    assert_eq!(compiler.tree(), None);
    assert_eq!(compiler.format_tree(), "-");
    assert_eq!(code.dump(), "PUSH NUM [1.0000000000]\n");

    // G_SEMCOL is a token no rule uses.
    assert_eq!(parse("1;"), Err(1));
    assert_eq!(parse(";"), Err(0));
    assert_eq!(parse("1 ; 2"), Err(1));
}

/* --------------------------------------------------------------------------------------------
 * Precedence and associativity: all pairs
 */

/// Associativity of a precedence level (`%left`, `%right`, `%nonassoc`).
#[derive(Clone, Copy, PartialEq)]
enum Assoc {
    Left,
    Right,
    None,
}

/// A binary operator: its source text, the mnemonic `format_tree` prints, and its precedence
/// level and associativity as declared in grammar.lemon:
///
/// ```text
/// %nonassoc G_IDENTIFIER G_ASSIGN G_SEMCOL.        (1)
/// %left G_COMMA.                                   (2)
/// %left G_BOOL_AND.                                (3)
/// %left G_BOOL_OR.                                 (4)
/// %left G_BOOL_XOR.                                (5)
/// %nonassoc G_LESS_THAN G_GREATER_THAN G_LESS_EQUAL_THAN G_GREATER_EQUAL_THAN G_EQUAL G_NOT_EQUAL.  (6)
/// %right G_BOOL_NOT.                               (7)
/// %left G_PLUS G_MINUS.                            (8)
/// %left G_DIVIDE G_MULT.                           (9)
/// %nonassoc G_STRUCT_REF.                          (10)
/// %nonassoc G_UNIT.                                (11)
/// ```
struct BinOp {
    text: &'static str,
    name: &'static str,
    prec: u8,
    assoc: Assoc,
}

const BIN_OPS: [BinOp; 14] = [
    BinOp {
        text: ",",
        name: "ARGS",
        prec: 2,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "&&",
        name: "AND",
        prec: 3,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "||",
        name: "OR",
        prec: 4,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "<",
        name: "LESS",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: ">",
        name: "GREATER",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: "<=",
        name: "LESS_EQUAL",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: ">=",
        name: "GREATER_EQUAL",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: "==",
        name: "EQUAL",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: "!=",
        name: "NEQUAL",
        prec: 6,
        assoc: Assoc::None,
    },
    BinOp {
        text: "+",
        name: "ADD",
        prec: 8,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "-",
        name: "SUB",
        prec: 8,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "*",
        name: "MUL",
        prec: 9,
        assoc: Assoc::Left,
    },
    BinOp {
        text: "/",
        name: "DIV",
        prec: 9,
        assoc: Assoc::Left,
    },
    // G_STRUCT_REF is declared %nonassoc, but the state reached after "nt . nt" has the
    // reduction as its only action and Lemon performs such reductions without looking at the
    // next token.  The net effect is left associativity.
    BinOp {
        text: ".",
        name: "SREF",
        prec: 10,
        assoc: Assoc::Left,
    },
];

/// Lemon's resolution of the shift/reduce conflict in "a OP1 b . OP2 c": reduce (true) if the
/// rule's precedence is higher than the lookahead's, or equal and left associative; shift
/// (false) if lower, or equal and right associative; a syntax error (None) if equal and
/// non-associative.
fn reduces_before(rule_prec: u8, rule_assoc: Assoc, lookahead_prec: u8) -> Option<bool> {
    if rule_prec > lookahead_prec {
        Some(true)
    } else if rule_prec < lookahead_prec {
        Some(false)
    } else {
        match rule_assoc {
            Assoc::Left => Some(true),
            Assoc::Right => Some(false),
            Assoc::None => None,
        }
    }
}

/// `a OP1 b OP2 c` for all 14 x 14 pairs of binary operators.
#[test]
fn binary_operator_pairs() {
    for op1 in &BIN_OPS {
        for op2 in &BIN_OPS {
            let expr = format!("7 {} 3 {} 2", op1.text, op2.text);

            let expected = match reduces_before(op1.prec, op1.assoc, op2.prec) {
                Some(true) => Ok(format!("({} ({} N[7] N[3]) N[2])", op2.name, op1.name)),
                Some(false) => Ok(format!("({} N[7] ({} N[3] N[2]))", op1.name, op2.name)),
                // The error is reported at the source position of the second operator's
                // token, which starts at the white space before it.
                None => Err(4 + op1.text.len() as i32),
            };

            assert_eq!(parse(&expr), expected, "{expr}");
        }
    }
}

/// A few rows of the matrix spelled out, as documentation.
#[test]
fn precedence_examples() {
    // multiplication before addition, both left associative
    assert_eq!(tree("1 + 2 * 3"), "(ADD N[1] (MUL N[2] N[3]))");
    assert_eq!(tree("1 * 2 + 3"), "(ADD (MUL N[1] N[2]) N[3])");
    assert_eq!(tree("1 - 2 - 3"), "(SUB (SUB N[1] N[2]) N[3])");
    assert_eq!(tree("8 / 4 / 2"), "(DIV (DIV N[8] N[4]) N[2])");
    assert_eq!(tree("8 / 4 * 2"), "(MUL (DIV N[8] N[4]) N[2])");
    // arithmetic before comparison before boolean operators
    assert_eq!(tree("1 + 2 == 3"), "(EQUAL (ADD N[1] N[2]) N[3])");
    assert_eq!(
        tree("1 < 2 && 3 > 2"),
        "(AND (LESS N[1] N[2]) (GREATER N[3] N[2]))"
    );
    // unlike in C, || binds tighter than &&
    assert_eq!(tree("1 || 2 && 3"), "(AND (OR N[1] N[2]) N[3])");
    assert_eq!(tree("1 && 2 || 3"), "(AND N[1] (OR N[2] N[3]))");
    assert_eq!(
        tree("1 && 2 || 3 && 4"),
        "(AND (AND N[1] (OR N[2] N[3])) N[4])"
    );
    // comparisons do not chain
    assert_eq!(parse("1 < 2 < 3"), Err(5));
    assert_eq!(parse("1 == 2 != 3"), Err(6));
    assert_eq!(tree("(1 < 2) == 1"), "(EQUAL (LESS N[1] N[2]) N[1])");
    // the comma has the lowest precedence
    assert_eq!(tree("1 , 2 && 3"), "(ARGS N[1] (AND N[2] N[3]))");
    assert_eq!(tree("1 , 2 , 3"), "(ARGS (ARGS N[1] N[2]) N[3])");
    // struct references bind tightest and chain to the left
    assert_eq!(tree("A.B + C.D"), "(ADD (SREF I[A] I[B]) (SREF I[C] I[D]))");
    assert_eq!(tree("A.B.C"), "(SREF (SREF I[A] I[B]) I[C])");
    assert_eq!(tree("A.B.C.D"), "(SREF (SREF (SREF I[A] I[B]) I[C]) I[D])");
    assert_eq!(
        tree("A.B == C.D"),
        "(EQUAL (SREF I[A] I[B]) (SREF I[C] I[D]))"
    );
    // parentheses override everything
    assert_eq!(tree("(1 + 2) * 3"), "(MUL (ADD N[1] N[2]) N[3])");
    assert_eq!(tree("1 && (2 , 3)"), "(AND N[1] (ARGS N[2] N[3]))");
    assert_eq!(tree("A.(B.C)"), "(SREF I[A] (SREF I[B] I[C]))");
}

/// Prefix operators against every binary operator: `U a OP b`.
///
/// The rule `nt ::= U nt` has the precedence of its first terminal, so the operand extends
/// over the following binary operator exactly when that operator binds tighter than U:
/// `!` (level 7, %right) captures `+ - * / .`; unary `-` and `+` (level 8, %left) capture
/// `* / .`.
#[test]
fn unary_against_binary_operators() {
    // ( text, precedence, associativity )
    let unary_ops = [
        ("!", 7, Assoc::Right),
        ("-", 8, Assoc::Left),
        ("+", 8, Assoc::Left),
    ];

    let apply = |unary: &str, operand: String| -> String {
        match unary {
            "!" => format!("(NOT {operand})"),
            "-" => format!("(SUB N[] {operand})"),
            // unary plus is replaced by a copy of its operand
            _ => operand,
        }
    };

    for (unary, unary_prec, unary_assoc) in unary_ops {
        for op in &BIN_OPS {
            let expr = format!("{} 7 {} 2", unary, op.text);

            let expected = match reduces_before(unary_prec, unary_assoc, op.prec) {
                Some(true) => format!("({} {} N[2])", op.name, apply(unary, "N[7]".to_string())),
                Some(false) => apply(unary, format!("({} N[7] N[2])", op.name)),
                None => unreachable!(),
            };

            assert_eq!(parse(&expr), Ok(expected), "{expr}");

            // On the right-hand side a prefix operator can always start an operand; what
            // follows it is again governed by the table above.
            let expr = format!("7 {} {} 2", op.text, unary);
            let expected = format!("({} N[7] {})", op.name, apply(unary, "N[2]".to_string()));

            assert_eq!(parse(&expr), Ok(expected), "{expr}");
        }
    }
}

#[test]
fn unary_examples() {
    assert_eq!(tree("-2 * 3"), "(SUB N[] (MUL N[2] N[3]))");
    assert_eq!(tree("-2 + 3"), "(ADD (SUB N[] N[2]) N[3])");
    assert_eq!(tree("-2 - 3"), "(SUB (SUB N[] N[2]) N[3])");
    assert_eq!(tree("-A.B"), "(SUB N[] (SREF I[A] I[B]))");
    assert_eq!(tree("!1 + 2"), "(NOT (ADD N[1] N[2]))");
    assert_eq!(tree("!1 == 2"), "(EQUAL (NOT N[1]) N[2])");
    assert_eq!(tree("!1 && 2"), "(AND (NOT N[1]) N[2])");
    assert_eq!(tree("!A.f()"), "(NOT (SREF I[A] (CALL I[f] NULL)))");
    assert_eq!(tree("1 - -2"), "(SUB N[1] (SUB N[] N[2]))");
    assert_eq!(tree("1 * -2 + 3"), "(ADD (MUL N[1] (SUB N[] N[2])) N[3])");
    assert_eq!(tree("1 + !2 * 3"), "(ADD N[1] (NOT (MUL N[2] N[3])))");
    assert_eq!(tree("1 == !2 + 3"), "(EQUAL N[1] (NOT (ADD N[2] N[3])))");
    assert_eq!(tree("+2 * 3"), "(MUL N[2] N[3])");
}

/* --------------------------------------------------------------------------------------------
 * Limits of the generated parser
 */

/// The Lemon parser stack holds 100 entries (`YYSTACKDEPTH`); when it overflows the stack is
/// silently emptied and parsing continues as if the input started at that point.
#[test]
fn parser_stack_overflow() {
    // Besides its bottom marker the stack has room for 99 symbols: 97 "(", the operand and the
    // first ")" still fit ...
    let deep = format!("{}1{}", "(".repeat(97), ")".repeat(97));
    assert_eq!(tree(&deep), "N[1]");

    // ... with 98 "(" the first ")" overflows the stack: everything parsed so far and that
    // token are dropped, and the second ")" (at offset 100) is a syntax error.
    let too_deep = format!("{}1{}", "(".repeat(98), ")".repeat(98));
    assert_eq!(parse(&too_deep), Err(100));

    // With 100 "(" it is the last "(" that is dropped; "1" is then parsed on its own and the
    // first ")" (at offset 101) is the syntax error.
    let too_deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(parse(&too_deep), Err(101));

    // Prefix operators nest the same way: "-" x 98 and the operand fit ...
    let minus = format!("{}1", "-".repeat(98));
    assert!(tree(&minus).starts_with("(SUB N[] (SUB N[] "));

    // ... and with one more the stack is emptied when the operand arrives and the operand is
    // dropped, which leaves an empty statement (no tree: the expression is "true") ...
    let minus = format!("{}1", "-".repeat(99));
    assert_eq!(tree(&minus), "-");

    // ... while with 100 it is the last "-" that is dropped, leaving a lone "1".
    let minus = format!("{}1", "-".repeat(100));
    assert_eq!(tree(&minus), "N[1]");

    // Left-recursive chains need no stack.
    let long = vec!["1"; 5000].join("+");
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut context = Context::new(());

    assert!(compiler.compile(&long, &mut code, &mut context));
    assert!(!compiler.is_error_pending());

    let result = code.run(&mut context);
    assert_eq!(context.value(result).as_double(&context), 5000.0);
}

/// The nodes carry the source position of the token *following* their text (the parser
/// reduces when it sees the next token, and that token's position is the white space before
/// it).
#[test]
fn node_source_positions() {
    let mut compiler = Compiler::new();
    let mut code = Ucode::new(DefaultBinder);
    let mut preflight = Context::new(());

    //                          0123456789
    assert!(compiler.compile("AB  + CDE", &mut code, &mut preflight));

    let root = compiler.node(compiler.tree().unwrap()).unwrap();
    let left = compiler.node(root.leaf[0].unwrap()).unwrap();
    let right = compiler.node(root.leaf[1].unwrap()).unwrap();

    assert_eq!(left.value.str.as_deref(), Some("AB"));
    assert_eq!(left.src_pos, 2);
    assert_eq!(right.value.str.as_deref(), Some("CDE"));
    assert_eq!(right.src_pos, 9);
    assert_eq!(root.src_pos, 9);
}
