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

//! Port of `qa/tests/common/libeval/test_numeric_evaluator.cpp` (test suite
//! `NumericEvaluator` for `NUMERIC_EVALUATOR`), followed by a differential test against the
//! C++ implementation (`tests/golden/numeric_golden.txt`, see `tests/oracle/README.md`).

mod common;

use kicad_libeval::{EdaUnits, NumericEvaluator};

/// `struct NUM_EVAL_FIXTURE`
fn fixture() -> NumericEvaluator {
    NumericEvaluator::new(EdaUnits::Mm)
}

/// Basic class ops: set one up, trivial input, tear it down
#[test]
fn basic() {
    let mut eval = fixture();

    eval.process("1");
    assert_eq!(eval.result(), "1");
}

/// Check that getting/setting vars works
#[test]
#[allow(clippy::approx_constant)] // 3.14 is the value the C++ test uses
fn set_var() {
    let mut eval = fixture();

    eval.set_var("MoL", 42.0);

    eval.process("1 + MoL");
    assert_eq!(eval.var("MoL"), 42.0);
    assert_eq!(eval.result(), "43");

    eval.set_var("MoL", 422.0);

    // have to process again to re-evaluate
    eval.process("1 + MoL");
    assert_eq!(eval.result(), "423");

    // Can remove one var
    eval.set_var("pi", 3.14);
    assert_eq!(eval.var("pi"), 3.14);
    eval.remove_var("pi");
    assert_eq!(eval.var("pi"), 0.0);

    // Other is still there
    assert_eq!(eval.var("MoL"), 422.0);

    // Add another one back
    eval.set_var("piish", 3.1);

    // String clear doesn't clear vars
    eval.clear();
    eval.process("1 + MoL + piish");
    assert_eq!(eval.result(), "426.1");

    // Clear both
    eval.clear_var();
    assert_eq!(eval.var("MoL"), 0.0);
    assert_eq!(eval.var("piish"), 0.0);
}

/// Struct representing a test case for `NUMERIC_EVALUATOR` (`struct EVAL_CASE`)
struct EvalCase {
    input: &'static str,
    exp_result: &'static str,
}

/// A list of valid test strings and the expected results
const EVAL_CASES_VALID: &[EvalCase] = &[
    // Empty case
    EvalCase {
        input: "",
        exp_result: "0",
    },
    // Trivial eval
    EvalCase {
        input: "1",
        exp_result: "1",
    },
    // Decimal separators
    EvalCase {
        input: "1.5",
        exp_result: "1.5",
    },
    EvalCase {
        input: "1,5",
        exp_result: "1.5",
    },
    // Semicolon is valid, but the result is NaN
    EvalCase {
        input: "1;",
        exp_result: "NaN",
    },
    // With own unit
    EvalCase {
        input: "1mm",
        exp_result: "1",
    },
    // Unit that's not the evaluator's unit
    EvalCase {
        input: "1in",
        exp_result: "25.4",
    },
    // Unit with white-space
    EvalCase {
        input: "1 in",
        exp_result: "25.4",
    },
    // Unit-less arithmetic
    EvalCase {
        input: "1+2",
        exp_result: "3",
    },
    // Multiple units
    EvalCase {
        input: "1 + 10mm + 1\" + 1.5in + 500mil",
        exp_result: "87.2",
    },
    // Any White-space is OK
    EvalCase {
        input: "   1 +     2    ",
        exp_result: "3",
    },
    // Decimals are OK in expressions
    EvalCase {
        input: "1.5 + 0.2 + .1",
        exp_result: "1.8",
    },
    // Negatives are OK
    EvalCase {
        input: "3 - 10",
        exp_result: "-7",
    },
    // Lots of operands
    EvalCase {
        input: "1 + 2 + 10 + 1000.05",
        exp_result: "1013.05",
    },
    // Operator precedence
    EvalCase {
        input: "1 + 2 - 4 * 20 / 2",
        exp_result: "-37",
    },
    // Parens
    EvalCase {
        input: "(1)",
        exp_result: "1",
    },
    // Parens affect precedence
    EvalCase {
        input: "-(1 + (2 - 4)) * 20.8 / 2",
        exp_result: "10.4",
    },
    // Unary addition is a sign, not a leading operator
    EvalCase {
        input: "+2 - 1",
        exp_result: "1",
    },
    // Set var in-string
    EvalCase {
        input: "x = 1; 1 + x",
        exp_result: "2",
    },
    // Multiple set vars
    EvalCase {
        input: "x = 1; y = 2; 10 + x - y",
        exp_result: "9",
    },
    // Unicode units - these currently fail
    //    { wxT( "1um" ), "0.001" },      // GREEK SMALL LETTER MU
    //    { wxT( "1µm" ), "0.001" },      // GREEK SMALL LETTER MU
    //    { wxT( "1 µm" ), "0.001" },     // GREEK SMALL LETTER MU
    //    { wxT( "1µm" ), "0.001" },      // MICRO SIGN
    //    { wxT( "1 µm" ), "0.001" },     // MICRO SIGN
];

/// Run through a set of test strings, clearing in between
#[test]
fn results() {
    let mut eval = fixture();

    for c in EVAL_CASES_VALID {
        // Clear for new string input
        eval.clear();

        eval.process(c.input);

        // These are all valid
        assert!(eval.is_valid(), "{} -> {}", c.input, c.exp_result);
        assert_eq!(
            eval.result(),
            c.exp_result,
            "{} -> {}",
            c.input,
            c.exp_result
        );

        // Does original text still match?
        assert_eq!(eval.original_text(), c.input);
    }
}

/// Test unicode parsing of the degree symbol
#[test]
fn unicode_degree() {
    let mut eval = fixture();
    let degree_input = "1\u{00B0}";

    // Set to degrees and make ready for input
    eval.set_default_units(EdaUnits::Degrees);
    eval.clear();

    eval.process(degree_input);

    // These are all valid
    assert!(eval.is_valid());

    // Currently disabled since the parser doesn't parse the unicode correctly
    //BOOST_CHECK_EQUAL( m_eval.Result(), degreeInput );

    // Does original text still match?
    assert_eq!(eval.original_text(), degree_input);
}

/// A list of invalid test strings (`eval_cases_invalid`)
const EVAL_CASES_INVALID: &[&str] = &[
    // Trailing operator
    "1+",
    // Leading operator
    "*2 + 1",
    // No operator
    "1 2",
    "(1)(2)",
    // Unknown operator
    "1 $ 2",
    // Mismatched parens
    "(1 + 2",
    "1 + 2)",
    // random text
    "sdfsdf sdfsd",
    // Div by 0
    "1 / 0",
    // Unknown vars are errors
    "1 + unknown",
    // Semicolons can't be empty or redundant
    ";",
    ";1",
    ";1;",
];

/// Run through a set of invalid test strings, clearing in between
#[test]
fn results_invalid() {
    let mut eval = fixture();

    for input in EVAL_CASES_INVALID {
        // Clear for new string input
        eval.clear();

        eval.process(input);

        // These are all valid
        assert!(!eval.is_valid(), "{input}");

        // Does original text still match?
        assert_eq!(eval.original_text(), *input);
    }
}

/* --------------------------------------------------------------------------------------------
 * Additional tests (not in the C++ suite)
 */

/// State of a fresh evaluator and after `Clear()`.
#[test]
fn initial_and_cleared_state() {
    let mut eval = fixture();

    // Before anything was processed there is no result buffer.
    assert!(eval.is_valid());
    assert_eq!(eval.result(), "");
    assert_eq!(eval.original_text(), "");

    eval.process("2 * 3");
    assert_eq!(eval.result(), "6");

    // Clear() invalidates the evaluator until the next Process().
    eval.clear();
    assert!(!eval.is_valid());
    assert_eq!(eval.result(), "");
    assert_eq!(eval.original_text(), "");

    assert!(eval.process(""));
    assert!(eval.is_valid());
    assert_eq!(eval.result(), "0");
}

/// A division by zero invalidates the evaluation.  (The result text is indeterminate in
/// C++, where the grammar action leaves the value unassigned; the port yields 0.)
#[test]
fn divide_by_zero() {
    let mut eval = fixture();

    for input in [
        "1 / 0",
        "0 / 0",
        "1 / (1 - 1)",
        "5 / 0 + 1",
        "x = 0; 1 / x",
        "1 / 0mm",
    ] {
        assert!(!eval.process(input), "{input}");
        assert!(!eval.is_valid(), "{input}");
    }

    assert!(!eval.process("1 / 0"));
    assert_eq!(eval.result(), "0");

    // The evaluator recovers for the next input.
    assert!(eval.process("1 / 4"));
    assert_eq!(eval.result(), "0.25");
}

/// The static helpers.
#[test]
fn separators() {
    assert_eq!(
        NumericEvaluator::is_old_school_decimal_separator('k'),
        Some(1.0e3)
    );
    assert_eq!(
        NumericEvaluator::is_old_school_decimal_separator('\u{B5}'),
        Some(1.0e-6)
    );
    assert_eq!(
        NumericEvaluator::is_old_school_decimal_separator('\u{3BC}'),
        Some(1.0e-6)
    );
    assert_eq!(NumericEvaluator::is_old_school_decimal_separator('x'), None);
    assert_eq!(
        NumericEvaluator::is_old_school_decimal_separator_byte(b'R'),
        Some(1.0)
    );
    assert_eq!(
        NumericEvaluator::is_old_school_decimal_separator_byte(0xB5),
        None
    );
    assert!(NumericEvaluator::is_decimal_separator(b'.', b'.', false));
    assert!(NumericEvaluator::is_decimal_separator(b',', b'.', false));
    assert!(!NumericEvaluator::is_decimal_separator(b'k', b'.', false));
    assert!(NumericEvaluator::is_decimal_separator(b'k', b'.', true));
    assert!(NumericEvaluator::is_digit(b'7'));
    assert!(!NumericEvaluator::is_digit(b'a'));
}

/// Process all expressions of a case line (`<EDA_UNITS value> TAB <expr> [TAB <expr> ...]`)
/// with one evaluator and render the results like `tests/oracle/oracle_numeric.cpp` does.
#[allow(clippy::approx_constant)] // pi = 3.14 is what the reference driver defines
fn run_case(case: &str) -> String {
    let mut parts = case.split('\t');
    let units: i32 = parts.next().unwrap().parse().unwrap();
    let mut eval = NumericEvaluator::new(EdaUnits::from_i32(units).unwrap());

    eval.set_var("MoL", 42.0);
    eval.set_var("pi", 3.14);

    let mut actual = String::new();

    for expr in parts {
        let ok = eval.process(&common::unescape(expr));
        actual.push_str(&format!(
            "R\t{}\t{}\t{}\n",
            ok as i32,
            eval.is_valid() as i32,
            eval.result()
        ));
    }

    actual
}

/// Every case of `tests/golden/numeric_cases.txt` must give exactly what the C++
/// `NUMERIC_EVALUATOR` gives (`tests/golden/numeric_golden.txt`).
#[test]
fn cases_match_cpp() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    let golden = std::fs::read_to_string(dir.join("numeric_golden.txt")).unwrap();
    let cases = std::fs::read_to_string(dir.join("numeric_cases.txt")).unwrap();

    let case_lines: Vec<&str> = cases
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#') && line.contains('\t'))
        .collect();

    let mut checked = 0;
    let mut failures = Vec::new();
    let mut lines = golden.lines();

    while let Some(header) = lines.next() {
        let case = header
            .strip_prefix("EXPR\t")
            .expect("golden block must start with EXPR");
        let mut expected = String::new();

        for line in lines.by_ref() {
            if line == "END" {
                break;
            }

            expected.push_str(line);
            expected.push('\n');
        }

        assert_eq!(
            case_lines.get(checked).copied(),
            Some(case),
            "golden file out of sync"
        );

        let actual = run_case(case);

        if actual != expected {
            failures.push(format!(
                "case: {case}\n--- C++ ---\n{expected}--- Rust ---\n{actual}"
            ));
        }

        checked += 1;
    }

    assert_eq!(
        checked,
        case_lines.len(),
        "golden file out of sync with the case file"
    );
    assert!(checked > 1300, "{checked}");

    assert!(
        failures.is_empty(),
        "{} of {} cases differ from the C++ implementation:\n\n{}",
        failures.len(),
        checked,
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/* --------------------------------------------------------------------------------------------
 * Generated cases
 */

/// xorshift64*: a tiny deterministic generator, so the case list never changes.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 33) % n as u64) as usize
    }
}

/// INCH, MM, UNSCALED (SI notation), DEGREES, MILS, PS
const GENERATED_UNITS: [i32; 6] = [0, 1, 2, 3, 5, 9];

/// Tokens for the generated sequences.  There is deliberately no division: the result text
/// after a division by zero is indeterminate in C++.
const GENERATED_TOKENS: [&str; 17] = [
    "1", "2.5", "x", "MoL", "mm", "in", "mil", "k", "\"", "+", "-", "*", "(", ")", "=", ";", "4k7",
];

/// The generated case list.  Changing anything here requires regenerating
/// `tests/golden/numeric_fuzz_hashes.txt` with the C++ reference driver.
fn generated_cases() -> Vec<String> {
    let mut cases = Vec::new();
    let mut unit = 0;
    let mut next_unit = || {
        unit += 1;
        GENERATED_UNITS[unit % GENERATED_UNITS.len()]
    };

    // 1. Every sequence of one to three tokens.
    for a in GENERATED_TOKENS {
        cases.push(format!("{}\t{}", next_unit(), a));

        for b in GENERATED_TOKENS {
            cases.push(format!("{}\t{} {}", next_unit(), a, b));

            for c in GENERATED_TOKENS {
                cases.push(format!("{}\t{} {} {}", next_unit(), a, b, c));
            }
        }
    }

    // 2. Random token sequences, with and without separating spaces.
    let mut rng = Rng(0x2545F4914F6CDD1D);

    for i in 0..3000 {
        let len = 4 + rng.below(8);
        let tokens: Vec<&str> = (0..len)
            .map(|_| GENERATED_TOKENS[rng.below(GENERATED_TOKENS.len())])
            .collect();
        let separator = if i % 3 == 2 { "" } else { " " };
        cases.push(format!("{}\t{}", next_unit(), tokens.join(separator)));
    }

    cases
}

/// About eight thousand generated cases against one 32 bit hash of the C++ output per case
/// (`tests/golden/numeric_fuzz_hashes.txt`).
#[test]
fn generated_cases_match_cpp() {
    let cases = generated_cases();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    let hashes = std::fs::read_to_string(dir.join("numeric_fuzz_hashes.txt")).unwrap();
    let hashes: Vec<&str> = hashes
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();

    assert_eq!(
        cases.len(),
        hashes.len(),
        "hash file out of sync with generated_cases()"
    );

    let mut failures = Vec::new();

    for (case, expected) in cases.iter().zip(&hashes) {
        let actual = run_case(case);

        if format!("{:08x}", common::fnv1a32(&actual)) != *expected {
            failures.push(format!(
                "case: {case}\n--- Rust (C++ output hash {expected}) ---\n{actual}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} generated cases differ from the C++ implementation (run them through \
         tests/oracle/oracle_numeric to see the expected output):\n\n{}",
        failures.len(),
        cases.len(),
        failures
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Writes the generated case list for the C++ reference driver:
///
/// ```text
/// cargo test -p kicad-libeval --test test_numeric_evaluator -- --ignored dump_generated_cases
/// ```
///
/// creates `<rust>/target/libeval-oracle/numeric_fuzz_cases.txt` (see
/// `tests/oracle/README.md`).
#[test]
#[ignore = "helper that exports the generated cases for the C++ reference driver"]
fn dump_generated_cases() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/libeval-oracle");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("numeric_fuzz_cases.txt"),
        generated_cases().join("\n") + "\n",
    )
    .unwrap();
}
