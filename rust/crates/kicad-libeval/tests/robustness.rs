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

//! Robustness: arbitrary input must never panic, whatever state the evaluators are in
//! (`rust/PORTING.md`, section 4), and compiled code must be shareable between threads.

mod common;

use common::{TestBinder, TestEnv, make_compiler};
use kicad_libeval::{Context, DefaultBinder, EdaUnits, NumericEvaluator, Ucode};

/// xorshift64*
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

/// Fragments biased towards the interesting parts of both languages, plus characters that
/// are special somewhere (NUL, control characters, multi-byte and astral code points).
const FRAGMENTS: [&str; 64] = [
    "0", "1", "9", "42", "1.5", "1,5", ".", ",", "..", "1e9", "007", "A", "B", "L", "AB", "x",
    "null", "Num", "Str", "Undef", "Parent", "Code", "one", "isFoo", "add", "cat", "lazy",
    "lazyStr", "nopush", "push2", "self", "mm", "mil", "in", "deg", "um", "k", "M", "'", "''",
    "'a'", "'*'", "\\", "\\'", "+", "-", "*", "/", "<", ">", "=", "==", "!=", "<=", ">=", "!",
    "&&", "||", "(", ")", ";", " ", "\u{0}", "\u{1F600}\u{e9}\t${",
];

fn random_text(rng: &mut Rng) -> String {
    let len = rng.below(14);
    (0..len).map(|_| FRAGMENTS[rng.below(FRAGMENTS.len())]).collect()
}

#[test]
fn compiler_never_panics() {
    let mut rng = Rng(0x0123456789ABCDEF);

    for round in 0..3000 {
        // Each compiler is reused for several expressions, so that it is also exercised in
        // its "sticky error" state with leftovers on the parser stack.
        let mut compiler = make_compiler(["none", "pcb", "pcbdeg"][round % 3]);

        for _ in 0..(1 + rng.below(6)) {
            let text = random_text(&mut rng);
            let mut ucode = Ucode::new(TestBinder::default());
            let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

            let ok = compiler.compile(&text, &mut ucode, &mut preflight);
            let _ = compiler.format_tree();
            let _ = ucode.dump();

            if ok {
                let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());

                if rng.below(2) == 0 {
                    ctx.set_error_callback(Some(Box::new(|_message: &str, _offset: i32| {})));
                }

                for _ in 0..2 {
                    let result = ucode.run(&mut ctx);
                    let _ = ctx.value(result).as_double(&ctx);
                    let _ = ctx.value(result).as_string(&ctx);
                }
            }
        }
    }
}

#[test]
fn numeric_evaluator_never_panics() {
    let mut rng = Rng(0xFEDCBA9876543210);
    let units = [
        EdaUnits::Inch,
        EdaUnits::Mm,
        EdaUnits::Unscaled,
        EdaUnits::Degrees,
        EdaUnits::Percent,
        EdaUnits::Mils,
        EdaUnits::Um,
        EdaUnits::Cm,
        EdaUnits::Fs,
        EdaUnits::Ps,
        EdaUnits::PsPerInch,
        EdaUnits::PsPerCm,
        EdaUnits::PsPerMm,
    ];

    for round in 0..3000 {
        let mut eval = NumericEvaluator::new(units[round % units.len()]);

        for _ in 0..(1 + rng.below(6)) {
            let text = random_text(&mut rng);
            let ok = eval.process(&text);

            assert_eq!(ok, eval.is_valid());
            assert_eq!(eval.original_text(), text);
            let _ = eval.result();

            if rng.below(8) == 0 {
                eval.clear();
            }
        }
    }
}

/// Very long and very deep inputs: no recursion depends on the input.
#[test]
fn large_inputs() {
    let inputs = [
        "(".repeat(100_000),
        ")".repeat(100_000),
        "-".repeat(100_000),
        "!".repeat(100_000) + "1",
        "A".to_string() + &".Parent".repeat(50_000),
        "A".to_string() + &".one()".repeat(20_000),
        vec!["1"; 100_000].join("+"),
        vec!["1"; 50_000].join(" , "),
        vec!["A.Num"; 30_000].join(" && "),
        "A.add(".repeat(30_000) + &")".repeat(30_000),
        "'".to_string() + &"x".repeat(1_000_000),
        "9".repeat(100_000),
        "{".repeat(50_000),
        // a string that QuacksLikeADuck() has to unescape
        "'".to_string() + &"{".repeat(3_000) + "5' + 1",
        "'".to_string() + &"{a".repeat(2_000) + &"}".repeat(2_000) + "' + 1",
    ];

    for input in &inputs {
        let mut compiler = make_compiler("pcb");
        let mut ucode = Ucode::new(TestBinder::default());
        let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

        if compiler.compile(input, &mut ucode, &mut preflight) {
            let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());
            let result = ucode.run(&mut ctx);
            let _ = ctx.value(result).as_double(&ctx);
        }

        let _ = compiler.format_tree();

        let mut eval = NumericEvaluator::new(EdaUnits::Unscaled);
        eval.process(input);
    }
}

/// Compiled microcode is immutable while it runs (all per-run state lives in the context),
/// so it can be evaluated from several threads at once, as pcbnew's DRC does.
#[test]
fn ucode_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<Ucode<DefaultBinder>>();
    assert_send_sync::<Ucode<TestBinder>>();
    assert_send_sync::<kicad_libeval::FuncCallRef<TestBinder>>();

    let mut compiler = make_compiler("none");
    let mut ucode = Ucode::new(TestBinder::default());
    let mut preflight: Context<'_, TestBinder> = Context::new(TestEnv::default());

    assert!(compiler.compile("A.Num + B.Num == 30 && A.isFoo('foo')", &mut ucode, &mut preflight));

    let ucode = &ucode;

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(move || {
                let mut ctx: Context<'_, TestBinder> = Context::new(TestEnv::default());

                for _ in 0..200 {
                    ctx.reset();
                    let result = ucode.run(&mut ctx);
                    assert_eq!(ctx.value(result).as_double(&ctx), 1.0);
                }
            });
        }
    });
}
