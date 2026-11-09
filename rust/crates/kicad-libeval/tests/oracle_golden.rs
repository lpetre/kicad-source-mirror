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

//! Differential tests against the C++ implementation.
//!
//! `tests/oracle/oracle_libeval.cpp` links KiCad's unmodified `libeval_compiler.cpp` (with the
//! Lemon-generated parser), `string_utils.cpp` and `eda_units.cpp` and prints, for every
//! expression of a case file, everything a client can observe: the result of `Compile()`,
//! the pending error (stage, offset, message), every error callback invocation with its
//! offset, the calls made to `CreateVarRef` / `CreateFuncCall`, the microcode dump, the
//! result of `Run()` with and without a run-time error callback, the run-time errors and the
//! side effects of deferred evaluations.  The tests below replay the same cases through the
//! Rust port with the twin binding in `tests/common` and require identical output.
//!
//! * `curated_cases_match_cpp`: the hand-written cases of `tests/golden/libeval_cases.txt`
//!   against the full C++ output in `tests/golden/libeval_golden.txt`.
//! * `generated_cases_match_cpp`: about fifteen thousand generated cases (every token
//!   sequence of up to three tokens, random token sequences, random well-formed
//!   expressions) against one 32 bit hash of the C++ output per case in
//!   `tests/golden/libeval_fuzz_hashes.txt`.
//!
//! See `tests/oracle/README.md` for how to regenerate the golden files.

mod common;

use common::{fnv1a32, run_case};

fn golden_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

#[test]
fn curated_cases_match_cpp() {
    let golden = std::fs::read_to_string(golden_path("libeval_golden.txt")).unwrap();
    let cases = std::fs::read_to_string(golden_path("libeval_cases.txt")).unwrap();

    // The golden file must cover exactly the cases of the case file, in order.
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
    assert!(checked > 1500);

    assert!(
        failures.is_empty(),
        "{} of {} cases differ from the C++ implementation:\n\n{}",
        failures.len(),
        checked,
        failures
            .iter()
            .take(15)
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

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const RESOLVERS: [&str; 3] = ["none", "pcb", "pcbdeg"];

/// Tokens for the exhaustive enumeration of short sequences.
const SHORT_TOKENS: [&str; 20] = [
    "1", "A", "x", "'s'", "mm", "+", "-", "*", "<", "==", "&&", "||", "!", "(", ")", ".", ",",
    "Num", "one", ";",
];

/// Tokens for the random sequences.
const TOKENS: [&str; 44] = [
    "1", "2.5", "0", "10", "A", "B", "L", "x", "null", "'s'", "'h*'", "'5'", "''", "mm", "mil",
    "deg", "+", "-", "*", "/", "<", ">", "<=", ">=", "==", "!=", "&&", "||", "!", "(", ")", ".",
    ",", ";", "Num", "Str", "Undef", "Parent", "one", "isFoo", "add", "lazy", "$", "=",
];

const ATOMS: [&str; 40] = [
    "0",
    "1",
    "2",
    "7",
    "0.5",
    "10",
    "1mm",
    "2mil",
    "1in",
    "45deg",
    "'s'",
    "'hello'",
    "'h*'",
    "'5'",
    "'4k7'",
    "''",
    "'World'",
    "A",
    "B",
    "null",
    "A.Num",
    "B.Num",
    "L.Num",
    "A.Frac",
    "A.Str",
    "B.Str",
    "A.NumStr",
    "B.NumStr",
    "A.Empty",
    "A.Undef",
    "A.Null",
    "A.Mm",
    "B.Mm",
    "A.Wild",
    "A.Code",
    "B.Code",
    "A.Alias",
    "A.Parent.Num",
    "A.Bogus",
    "x",
];

const BINARY_OPS: [&str; 13] = [
    "+", "-", "*", "/", "<", ">", "<=", ">=", "==", "!=", "&&", "||", ",",
];

const UNARY_OPS: [&str; 3] = ["!", "-", "+"];

const CALLS: [(&str, usize); 10] = [
    ("A.one", 0),
    ("A.isFoo", 1),
    ("A.add", 2),
    ("B.cat", 2),
    ("A.self", 0),
    ("A.Parent.self", 0),
    ("A.lazy", 0),
    ("A.lazyStr", 1),
    ("A.nopush", 1),
    ("A.unknown", 1),
];

fn random_expr(rng: &mut Rng, depth: usize) -> String {
    let choice = if depth == 0 { 0 } else { rng.below(10) };

    match choice {
        0..=2 => rng.pick(&ATOMS).to_string(),
        3..=6 => {
            let op = rng.pick(&BINARY_OPS);
            format!(
                "{} {} {}",
                random_expr(rng, depth - 1),
                op,
                random_expr(rng, depth - 1)
            )
        }
        7 => format!("{}{}", rng.pick(&UNARY_OPS), random_expr(rng, depth - 1)),
        8 => format!("({})", random_expr(rng, depth - 1)),
        _ => {
            let (name, arity) = CALLS[rng.below(CALLS.len())];
            let args: Vec<String> = (0..arity).map(|_| random_expr(rng, depth - 1)).collect();
            format!("{}({})", name, args.join(" , "))
        }
    }
}

/// The generated case list.  Changing anything here requires regenerating
/// `tests/golden/libeval_fuzz_hashes.txt` with the C++ reference driver.
fn generated_cases() -> Vec<String> {
    let mut cases = Vec::new();
    let mut resolver = 0;
    let mut next_resolver = || {
        resolver += 1;
        RESOLVERS[resolver % RESOLVERS.len()]
    };

    // 1. Every sequence of one to three tokens.
    for a in SHORT_TOKENS {
        cases.push(format!("{}\t{}", next_resolver(), a));

        for b in SHORT_TOKENS {
            cases.push(format!("{}\t{} {}", next_resolver(), a, b));

            for c in SHORT_TOKENS {
                cases.push(format!("{}\t{} {} {}", next_resolver(), a, b, c));
            }
        }
    }

    // 2. Random token sequences, with and without separating spaces.
    let mut rng = Rng(0x9E3779B97F4A7C15);

    for i in 0..4000 {
        let len = 4 + rng.below(8);
        let tokens: Vec<&str> = (0..len).map(|_| rng.pick(&TOKENS)).collect();
        let separator = if i % 4 == 3 { "" } else { " " };
        cases.push(format!("{}\t{}", next_resolver(), tokens.join(separator)));
    }

    // 3. Random well-formed expressions.
    for _ in 0..3000 {
        let depth = 1 + rng.below(4);
        cases.push(format!(
            "{}\t{}",
            next_resolver(),
            random_expr(&mut rng, depth)
        ));
    }

    cases
}

#[test]
fn generated_cases_match_cpp() {
    let cases = generated_cases();
    let hashes = std::fs::read_to_string(golden_path("libeval_fuzz_hashes.txt")).unwrap();
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

        if format!("{:08x}", fnv1a32(&actual)) != *expected {
            failures.push(format!(
                "case: {case}\n--- Rust (C++ output hash {expected}) ---\n{actual}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} generated cases differ from the C++ implementation (run them through \
         tests/oracle/oracle_libeval to see the expected output):\n\n{}",
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
/// cargo test -p kicad-libeval --test oracle_golden -- --ignored dump_generated_cases
/// ```
///
/// creates `<rust>/target/libeval-oracle/fuzz_cases.txt` (see `tests/oracle/README.md`).
#[test]
#[ignore = "helper that exports the generated cases for the C++ reference driver"]
fn dump_generated_cases() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/libeval-oracle");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fuzz_cases.txt"),
        generated_cases().join("\n") + "\n",
    )
    .unwrap();
}
