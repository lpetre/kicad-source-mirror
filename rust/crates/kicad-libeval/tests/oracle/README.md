# C++ reference drivers ("oracle") for kicad-libeval

Nothing in this directory is part of the cargo build.  It documents and automates how the
generated/golden artefacts of the crate are produced from the KiCad C++ sources:

| artefact | produced from | by |
|---|---|---|
| parse tables in `src/libeval_compiler/grammar.rs` | `common/libeval_compiler/grammar.lemon` | `thirdparty/lemon` + `gen_tables.py` |
| parse tables in `src/numeric_evaluator/grammar.rs` | `common/libeval/grammar.lemon` | `thirdparty/lemon` + `gen_tables.py` |
| `tests/golden/libeval_golden.txt` | `tests/golden/libeval_cases.txt` | `oracle_libeval` |
| `tests/golden/libeval_fuzz_hashes.txt` | `generated_cases()` in `tests/oracle_golden.rs` | `oracle_libeval` + `hash_golden.py` |
| `tests/golden/numeric_golden.txt` | `tests/golden/numeric_cases.txt` | `oracle_numeric` |
| `tests/golden/numeric_fuzz_hashes.txt` | `generated_cases()` in `tests/test_numeric_evaluator.rs` | `oracle_numeric` + `hash_golden.py` |

## What the drivers are

`build_oracle.sh` compiles KiCad's own Lemon, runs it on the two unmodified grammar files
exactly like the KiCad build does, and links two small command line programs against the
**unmodified** C++ sources `common/libeval_compiler/libeval_compiler.cpp`,
`common/libeval/numeric_evaluator.cpp`, `common/string_utils.cpp` and `common/eda_units.cpp`
(plus wxWidgets "base", fmt and harfbuzz, which those sources need to link):

* `oracle_libeval` (`oracle_libeval.cpp`) compiles and runs expressions with a deterministic
  test binding (`TEST_UCODE`: items `A`, `B`, `L`, a handful of properties and functions,
  two `VALUE` subclasses) and prints every observable: the result of `Compile()`, the pending
  error, each error callback with its offset, the `CreateVarRef` / `CreateFuncCall` calls,
  the microcode dump, the syntax tree, the results of `Run()` with and without a run-time error callback, the
  run-time errors, and the side effects of deferred evaluations.  The Rust twin of the
  binding is `tests/common/mod.rs`; **the two must be changed together**.
* `oracle_numeric` (`oracle_numeric.cpp`) feeds expressions to a `NUMERIC_EVALUATOR`.

Requirements: a C++20 compiler, `wx-config` (wxWidgets 3.2), harfbuzz development files and
python3.  Everything is built in `<kicad>/rust/target/libeval-oracle` (or the directory given
as argument); nothing is written into the source tree.

## Regenerating everything

From `<kicad>/rust`:

```sh
sh crates/kicad-libeval/tests/oracle/build_oracle.sh
O=target/libeval-oracle
G=crates/kicad-libeval/tests/golden
T=crates/kicad-libeval/tests/oracle

# golden files of the hand-written case files
LC_ALL=C.UTF-8 $O/oracle_libeval < $G/libeval_cases.txt > $G/libeval_golden.txt
$O/oracle_numeric < $G/numeric_cases.txt > $G/numeric_golden.txt

# hashes of the generated cases (the Rust tests export their case lists)
cargo test -p kicad-libeval --test oracle_golden -- --ignored dump_generated_cases
cargo test -p kicad-libeval --test test_numeric_evaluator -- --ignored dump_generated_cases
LC_ALL=C.UTF-8 $O/oracle_libeval < $O/fuzz_cases.txt | python3 $T/hash_golden.py > $G/libeval_fuzz_hashes.txt
$O/oracle_numeric < $O/numeric_fuzz_cases.txt | python3 $T/hash_golden.py > $G/numeric_fuzz_hashes.txt

# parse tables: paste the output between the BEGIN/END GENERATED TABLES markers
python3 $T/gen_tables.py $O/gen/libeval_compiler/grammar.c $O/gen/libeval_compiler/grammar.h
python3 $T/gen_tables.py $O/gen/libeval/grammar.c $O/gen/libeval/grammar.h
```

`LC_ALL=C.UTF-8` matters for `oracle_libeval`: case-insensitive string comparison goes through
`towlower()`/`towupper()`, which only handle ASCII in the "C" locale.  The Rust port
implements the UTF-8 locale behaviour (Unicode simple case mapping).

## Rules for case files

The C++ code has undefined behaviour in a few situations.  Such cases must not be put into
the case files (their C++ results are not reproducible); the Rust port handles them
deterministically and has its own tests for them:

* **Reusing a `LIBEVAL::COMPILER` after an error.**  The Lemon parser stack is not reset and
  keeps pointers to tree nodes and token strings that `COMPILER::Clear()` frees at the start
  of the next `Compile()`.  A second compile after a *lexical* error (unknown character), or
  a third non-empty compile after any error, reads freed memory (the reference driver
  crashes on some of these).  One further compile after a syntax or code generation error
  is fine and is covered.  See `reuse_after_error_never_panics` in `tests/libeval_errors.rs`.
* **Division by zero in `NUMERIC_EVALUATOR`.**  The grammar action leaves the result value
  unassigned, so the result *text* is indeterminate (validity is well defined: invalid).
  See `divide_by_zero` in `tests/test_numeric_evaluator.rs`.

When adding cases, run the driver under valgrind once
(`valgrind -q --error-exitcode=9 $O/oracle_libeval < cases.txt > /dev/null`): it must stay
silent.  All committed case files, including the generated lists, were checked this way.

## Case file format

One case per line, fields separated by TAB; `\\`, `\t`, `\n`, `\r` are escapes; lines starting
with `#` are comments.

* `oracle_libeval`: `<resolver> TAB <expr> [TAB <expr> ...]` with resolver `none` (the
  `LIBEVAL::UNIT_RESOLVER` base class), `pcb` (`PCBEXPR_UNIT_RESOLVER`) or `pcbdeg`
  (`PCBEXPR_UNIT_RESOLVER( true )`).  With several expressions they are compiled in turn by
  one compiler and not run.
* `oracle_numeric`: `<EDA_UNITS value> TAB <expr> [TAB <expr> ...]`; all expressions are
  processed in turn by one evaluator that has the variables `MoL = 42` and `pi = 3.14`.

Output: `EXPR <line>`, the result lines, `END`.  Result lines of `oracle_libeval`:

| line | meaning |
|---|---|
| `C ok pending [stage srcPos message]` | return value of `Compile()`, `IsErrorPending()`, `GetError()` |
| `E offset message` | a compiler error callback invocation |
| `B var:<var>:<field>` / `B func:<name>` | a `CreateVarRef` / `CreateFuncCall` call |
| `U dump` | `UCODE::Dump()` with addresses normalised |
| `T tree` | after a successful `Compile()`: the syntax tree (`COMPILER::m_tree`) as the Lemon parser built it and `generateUCode` left it, every node as `(op:srcPos:flags:idx:str leaf0 leaf1)` (flags `v` = `isVisited`, `t` = `isTerminal`; `~` = no string; `-` = no leaf) |
| `R type units bits string` | result of `Run()` with an error callback on the context: `GetType()`, `GetUnits()`, the IEEE bits of `AsDouble()`, `AsString()` |
| `X offset message` | a run-time error callback invocation |
| `L text` | a side effect of a deferred evaluation |
| `N ...` | like `R`, for a run without error callback |
