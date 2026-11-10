# kicad-drc: port status

Port of KiCad's design rule checker (`pcbnew/drc/`). Module layout: one module per C++ file as
laid out in `rust/docs/analysis/drc.md` section 10.1; modules of packages that have not landed
yet exist as empty files naming their package. Packages append their own section below.

## DRC-20: design rule file (`.kicad_dru`) parser

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_rule_parser.h/.cpp` (`DRC_RULES_PARSER`: `Parse`, `ParseComponentClassAssignmentRules`, every clause, every error message, both error modes) | `src/rule_parser.rs` (`DrcRulesParser`) | ported |
| `DRC_RULES_PARSER::VerifyParseCondition` | - | skipped: declared in the header, never defined in C++ |
| `common/drc_rules.keywords` + generated `drc_rules_lexer.h`, `common/dsnlexer.cpp` | `kicad_sexpr::keywords::drcrule_t`, `kicad_sexpr::DsnLexer`; re-exported by `src/rules_lexer.rs` | ported (in kicad-sexpr) |
| `pcbnew/drc/drc_rule.h/.cpp`: `DRC_RULE`, `IsComponentClassSelector`, `DRC_CONSTRAINT::IsUnary`, `SetParentRule`, `SetValueRules`, `GetName`, `GetRuleName`, `SetOptionsFromOther`, `Merge` | `src/rule.rs` (`DrcRule`, `DrcConstraintExt` on `kicad_pcb::drc_api::DrcConstraint`) | ported |
| `DRC_RULE::FormatRuleFromProto`, `DRC_CONSTRAINT::ToProto` | - | skipped: IPC API |
| `GetConstraint( aItem, bItem, ... )` (`drc_rule.h`) | - | not ported: board helper of the engine (DRC-04) |
| `pcbnew/drc/drc_rule_condition.h/.cpp` (`DRC_RULE_CONDITION`: `Compile`, `EvaluateFor`, flags) | `src/rule_condition.rs` (`DrcRuleCondition`, `CompiledCondition`) | ported; evaluation needs DRC-21 (`PcbExprBinder::item_env`) |
| `pcbnew/pcbexpr_evaluator.*`: `PCBEXPR_UNIT_RESOLVER`, `PCBEXPR_UNITLESS_RESOLVER`, `PCBEXPR_COMPILER`, `PCBEXPR_EVALUATOR` | `src/pcbexpr/evaluator.rs` | ported |
| `pcbnew/pcbexpr_evaluator.*`: `PCBEXPR_UCODE` (`CreateVarRef`, `CreateFuncCall`), `PCBEXPR_CONTEXT`, var refs, value classes | `src/pcbexpr/evaluator.rs` (`PcbExprUcode`, `PcbExprContext`) | stub: `port_pending!` (package DRC-21) |
| `DRC_ENGINE::loadRules` (`drc_engine.cpp` L682-721) | `src/engine/mod.rs` (`prepare_rules_text`, `load_rules`) | ported; depends on the pending `BOARD::ConvertCrossReferencesToKIIDs` / `BOARD::ResolveTextVar` of kicad-pcb |
| `include/reporter.h`, `common/reporter.cpp`: `REPORTER` (`Report`, `HasMessage`, `HasMessageOfSeverity`), `WX_STRING_REPORTER` | `src/reporter.rs` | partial (the members the DRC uses) |
| `ENUM_MAP<PCB_LAYER_ID>` layer choices (`BOARD_ITEM_DESC`, `BOARD_LOADER::Load`) | `rule_parser::default_layer_choices`, `board_layer_choices` | ported (as explicit data, see Deviations) |

### The expression binding boundary (for DRC-21)

The parser, `DrcRuleCondition::compile_with` and `PcbExprEvaluator` are generic over
`B: PcbExprBinder` (default `PcbExprUcode`). `PcbExprBinder` = `kicad_libeval::UcodeBinder`
(the virtuals of `PCBEXPR_UCODE`; `Env` = the members of `PCBEXPR_CONTEXT`) + the usage flags
(`HasGeometryDependentFunctions`, `RequiresPairItems`, `ReferencesItemB`) + two context
constructors (`new_env( constraint, layer )` for preflight / constants, `item_env( a, b,
constraint, layer )` for `EvaluateFor`, which also applies the teardrop type override). Compiled
microcode is stored type-erased (`Box<dyn CompiledCondition>`, `Send + Sync`) so rules do not
carry the binder type. DRC-21 replaces the body of `PcbExprUcode` / `PcbExprContext` and nothing
else changes. Constant values (`(min 0.2mm + 1mil)`) never reach the binder, so they work today;
conditions call `create_var_ref` / `create_func_call` and panic with `port_pending!` until DRC-21.

### Verification

Differential oracle against the real `DRC_RULES_PARSER`: new command `pcb-dump rules
[--component-classes] FILE...` (`rust/tools/pcb-dump/dump_rules.cpp`, format in
`rust/docs/PCB_DUMP_FORMAT.md` section 7) dumps every rule field, every constraint field and
option, the condition texts and their usage flags, every reporter message and the thrown
`PARSE_ERROR` (message, line, offset, input line), with a reporter and without one. Its Rust twin
is `tests/common/mod.rs` (`dump_rules`), also runnable as `cargo run -p kicad-drc --example
rules_dump -- FILE...`. Goldens (`tests/oracle/gen_rules_goldens.py`):

| Golden | Inputs | Result |
|---|---|---|
| `tests/golden/rules_corpus.txt` | all 73 `.kicad_dru` under `qa/` and `demos/` (compact: status, counts, FNV-1a of each mode's dump) | identical (full unhashed dumps also diffed identical: 14571 lines) |
| `tests/golden/rules_errors.txt` | 94 deliberately broken / edge-case files `tests/data/rules/*.kicad_dru` (one per error message and branch) | identical |
| `tests/golden/rules_cc.txt` | 8 component class assignment files `tests/data/rules/cc/` | identical |

The reference kicad-cli confirmed the `loadRules` line doubling (file line 4 reported as line 7,
offset 62) - `tests/test_rule_parser.rs::load_rules_text_doubles_line_numbers`.

Conditions are compiled in the tests with a stand-in binder (`tests/common::SyntaxBinder`) that
ports the variable-name part of `PCBEXPR_UCODE::CreateVarRef` and `CreateFuncCall` (function
registry names and geometry flags) statement by statement and accepts every property. Everything
the parser observes without a reporter (compile result = grammar only, usage flags) is therefore
exact; with a reporter, codegen messages of the real binding (e.g. "Unrecognized property") can
differ, and the test files contain none. Not verifiable until DRC-21: condition codegen
messages, `EvaluateFor`, values that reference items or functions.

### Ported C++ tests

| C++ | Rust |
|---|---|
| `qa/tests/pcbnew/test_libeval_compiler.cpp`: `SingleItemRulePreflight`, `PairConditionsInSingleItemRules`, `PairItemDependency` | `tests/test_rule_parser.rs` (with the stand-in binder) |
| `qa/tests/pcbnew/drc/test_drc_stub_length.cpp`: `StubLengthAcceptsTimeDomainUnits`, `ReturnPathDoesNotCarryTimeDomainOption` | `tests/test_rule_parser.rs` |
| `qa/tests/cli/test_pcb.py::test_pcb_drc_rejects_unavailable_second_item` (parser part) | `tests/test_rule_parser.rs::rejects_unavailable_second_item` |

Not ported: `test_drc_rule_editor.cpp` (GUI rule editor), the remaining `test_libeval_compiler.cpp`
cases (DRC-21), the board-level tests using rule files (DRC-22 and later).

### Deviations

- **Unknown `return_path` sub-option at the end of the input**: the C++ skip loop
  `while( NextTok() != T_RIGHT );` never terminates at EOF (the oracle hangs on
  `tests/data/rules/return_path_unknown_eof.kicad_dru`). The port stops at EOF; the enclosing loop
  then reports "Missing ')'." (`rules_oracle::return_path_unknown_option_at_eof_terminates`).
- **Rule back pointers**: `kicad_pcb::drc_api::DrcConstraint` holds `DrcRuleInfo` copies instead
  of `DRC_RULE*`. The parser refreshes them when a rule is complete
  (`DrcRule::sync_constraint_rule_info`) so that clauses after a constraint (e.g. `(severity ...)`)
  are seen as through the C++ pointer. Code that changes a rule later must call it as well.
- **`m_Test`**: `DrcConstraint` has no assertion member; the condition is kept in
  `DrcRule::constraint_tests[i]` next to `DrcRule::constraints[i]` (`Arc`, shared like the pointer).
- **Layer table**: the C++ parser reads the global `ENUM_MAP<PCB_LAYER_ID>`, whose content depends
  on what was loaded before (canonical names from static initialisation, plus the board's user
  names after `BOARD_LOADER::Load`). The port takes the table explicitly
  (`DrcRulesParser::set_layer_choices`, `load_rules( .., layer_choices, .. )`); default =
  canonical names.
- **Error callbacks**: the C++ value error handler throws from inside the expression evaluator; the
  port collects the evaluator's messages and handles them in order afterwards (report each, or
  return the first). Equivalent because the evaluation has no other observable effect.
  `DrcRuleCondition::evaluate_for` forwards run-time messages to the reporter after the run for
  the same reason.
- `wxAtof` is `strtod` in the C locale (the reference runs with `LC_ALL=C.UTF-8`).
- `KiROUND` overflow of huge values (`(max 1000000mm)`) clamps like the C++; the C++ additionally
  raises a wx assertion (logged in release builds; the dump tool counts it on a `warn|` line).
- `loadRules`: lines longer than `LINE_READER_LINE_DEFAULT_MAX` ("Maximum line length exceeded")
  and invalid UTF-8 are not reproduced; the caller passes the file text.

### Known gaps

- Conditions cannot be compiled with the default binding until DRC-21 (`port_pending!`).
- `load_rules` calls `Board::convert_cross_references_to_kiids` / `resolve_text_var`, which are
  still `port_pending!` in kicad-pcb.
- `Reporter` belongs in a shared crate (`common/reporter.cpp`); it lives here until one exists.
- The native implicit conditions (analysis 4.8.5) are left to DRC-03.
