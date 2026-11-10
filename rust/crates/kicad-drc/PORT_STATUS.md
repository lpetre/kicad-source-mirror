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

## DRC-11: markers, severities, exclusions, report writers

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_item.cpp/.h`: the static `DRC_ITEM`s (code, title, settings key), `allItemTypes` with headings, `GetItemsWithSeverities`, `Create( int )`, `Create( key )`, `GetViolatingRuleDesc`, `GetAuxItem2ID/3ID`, `ConvertPadCheckToDrcCode` | `src/item.rs` (`ALL_ITEM_TYPES`, `items_with_severities`, `create`, `create_from_key`, ...) | ported (codes and keys from `kicad_pcb::drc_api`; item data is `kicad_pcb::pcb_marker::RcItem`) |
| `DRC_ITEMS_PROVIDER` (`SetSeverities`, `GetCount( aSeverity )`, `GetItem`) | `src/item.rs` (`DrcItemsProvider`, holds `MarkerId`s) | ported; `DeleteItem` skipped (GUI) |
| `BOARD_RC_ITEM_RESOLVER` | `src/item.rs` (`BoardRcItemResolver`) | ported |
| `DRC_ITEM::GetViolationLayers` | - | skipped: GUI layer focus |
| `common/rc_item.cpp` / `include/rc_item.h`: `RC_ITEM_RESOLVER` (miss cache), `ShowReport`, `GetJsonViolation`, `getSeverityString`, `showCoord`, `getItemDescription` | `src/rc_item.rs` (`RcItemResolver`, `show_report`, `json_violation`) | ported; `RC_ITEM::m_parent` is passed as `RcItemParent`; `RC_TREE_MODEL` skipped (GUI) |
| `include/rc_json_schema.h` (`RC_JSON::COORDINATE`, `AFFECTED_ITEM`, `VIOLATION` with its custom `to_json`, `IGNORED_CHECK`, `DRC_REPORT`) | `src/rc_item.rs` (`Json*`) | ported (to_json; `from_json` and the ERC structs not needed) |
| `common/widgets/wx_data_view_hyperlink_renderer.cpp`: `StripMarkup`, `ParseRuns`, `tryParseMarkdown` | `src/rc_item.rs` (`strip_markup`, `parse_runs`) | ported; `IsSafeUrl` skipped (only decides whether a GUI run is clickable) |
| `pcbnew/drc/drc_report.cpp/.h`: `WriteTextReport`, `WriteJsonReport` | `src/report.rs` (`DrcReport`) | ported; return the file content, date and version are parameters (`KICAD_MAJOR_MINOR_PATCH_VERSION` = "10.99.0", confirmed by the oracle) |
| `pcbnew/pcb_marker.cpp`: `ToProto`, `FromProto`, `FromLegacyString`; `wxSplit` (wx 3.2) | `src/marker.rs` (`to_proto`, `from_proto`, `from_legacy_string`, `wx_split`) | ported (the `PCB_MARKER` type, constructor, severity: `kicad_pcb::pcb_marker`) |
| `api/proto/board/board_rules.proto` `DrcMarker`, `DrcExclusion`; `Vector2`, `KIID`; `api_pcb_enums.cpp` / `api_enums.cpp` enum conversions (`DrcErrorType`, `BoardLayer`, `MarkerType`, `RuleSeverity`); protobuf 3.21 wire format, `MessageToJsonString`, `JsonStringToMessage` | `src/marker/proto.rs` | ported by hand (no protobuf dependency), only these messages |
| `pcbnew/drc/drc_exclusion.cpp/.h` (`DRC_EXCLUSION`: `FromMarker`, `FromProto`, `FromLegacyStrings`, `GetSortKey`, `to_json`, `from_json`, canonicalisation) | `src/exclusion.rs` | ported; produces `kicad_pcb::board_design_settings::DrcExclusion`; `KiapiDrcExclusionCodec` implements the settings reader's `DrcExclusionCodec` |
| `board.cpp` `ResolveDRCExclusions`, `RecordDRCExclusions` | `src/exclusion.rs` (`resolve_drc_exclusions`, `record_drc_exclusions`) on top of `Board::resolve_drc_exclusions_with` / `record_drc_exclusions_with` (kicad-pcb M22) | ported |

### Verification

Differential oracle `tests/oracle/drc_report_oracle.cpp` (own driver, built by
`tests/oracle/build_report_oracle.sh` against the reference build like pcb-dump; binary
`/home/user/kicad-oracle/bin/drc-report-oracle`). It runs the scenarios `tests/data/reports/*.scn`
(board, design-settings severities, exclusions as JSON or legacy strings, markers with
messages / rules / exclusion flags, `ResolveDRCExclusions( true|false )`, `RecordDRCExclusions`,
both report writers with the three providers of `JobExportDrc`, `FromLegacyString`,
`StripMarkup`) with the real C++ code and prints every intermediate value (marker fields,
severities, exclusion sort keys (bytes, or FNV-1a), ProtoJSON, the design-settings set in order,
provider counts, both reports verbatim with the date replaced). Golden:
`tests/golden/report_oracle.txt` (`tests/oracle/gen_report_goldens.sh`, deterministic over
runs). Rust twin: `tests/report_oracle.rs::report_oracle_replay`.

| Scenario | What | Result |
|---|---|---|
| `legacy_strip.scn` | 17 legacy exclusion strings (escaped pipes, `strtol` overflow / garbage, short forms, every special code, empty key = heading), 8 `StripMarkup` inputs | identical |
| `report_sections_noitems.scn` | 15 markers (all marker types, rule severities incl. exclusion, generic codes, excluded with / without comment, markup in messages, UTF-8), severity overrides, 5 reports (mm / mils / in, masks 48, 52, 4, 0, 32), ignored-check lists | identical (text and JSON) |
| `exclusions_noitems.scn` | 39 exclusions: canonical ProtoJSON, lowerCamel names, numbers / strings for int64 and enums, nulls, unknown fields / enum names (rejected), floats, spaces, `+`, overflow, items given as an object, unknown enum numbers, missing marker, legacy strings; matching against 7 markers; `ResolveDRCExclusions( true )` (created markers, set order), record, reports | identical |
| `exclusions_cli_noitems.scn` | the same with `ResolveDRCExclusions( false )` (the CLI flow) | identical |
| `report_sections.scn`, `exclusions.scn`, `pending_items.scn` | the same on `qa/data/pcbnew/issue8003.kicad_pcb`, whose items resolve (item lines `@(x, y): description`, item JSON, creation of exclusion markers whose items exist) | pending: the Rust board parser still skips footprints and tracks (PCBIO-03, PCBIO-06), and the footprint / shape descriptions are `port_pending!`; the test reports them as pending and compares them automatically once the board loads completely |

Unit tests (`src/*.rs`) cover the item table against `drc_api` and the `allItemTypes` order of
`drc.md` 3.6, the report line formats derived from the C++ format strings, the JSON layout
(sorted keys, 4-space indent, `.0` doubles), the wire format, the enum tables and the round trips.

Reference `kicad-cli` reports: `tests/golden/reports/` (10 boards from `qa/data/pcbnew`, text and
JSON, mm / mils / in, severity masks all / error / warning / exclusions; 70 KB). Command lines in
`tests/golden/reports/cases.txt`, recorded by `tests/oracle/gen_cli_reports.sh` on copies.
`report_oracle.rs::reference_cli_reports` (`#[ignore]`d) compares them with the Rust DRC once the
engine (DRC-03 and later) and the CLI flow (DRC-12) exist. The reference build orders some
violations differently from run to run (thread pool; issue8003, issue7267: same set, different
order), so the comparison sorts the entries inside each section; boards whose violation *set*
changes between runs (soldermask_bridge_all_tracks, issue10916) were left out. issue17429 with
`--severity-exclusions` exercises the legacy exclusions of a real project file (3 violations and
7 unconnected items excluded).

### Ported C++ tests

None yet: `test_drc_unconnected_save.cpp` and `test_drc_unconnected_items_exclusion_loss.cpp`
(and the exclusion parts of `test_drc_regressions.cpp` / `test_custom_rule_severities.cpp`) run
the whole DRC on boards and save projects; they need the engine and the project writer.

### Deviations

- **Sort key representation**: `DRC_EXCLUSION::GetSortKey()` is a byte string (protobuf wire
  format); `DrcExclusion::sort_key` stores each byte as the `char` of the same value, which keeps
  the C++ (`unsigned char`) order. Equality and order are those of C++; the text itself is not
  meant to be read.
- **No protobuf library**: the four messages, their wire format and ProtoJSON are written by
  hand after protobuf 3.21.12 (the reference build's version: on a JSON parse error the message
  is left empty). The oracle confirms the accept / reject decisions listed above; inputs outside
  them (e.g. duplicate fields under both names, exotic number spellings) follow the documented
  3.21 converter behaviour without oracle confirmation.
- `IsSafeUrl` is not ported: `parse_runs` returns the parsed href for every link.
- The report writers return strings; failing to open the output file is the caller's error.
- `JsonStringToMessage` input is the `serde_json` value, not `aJson.dump()` text: identical,
  since nlohmann's dump of a parsed document re-parses to the same numbers and strings.
- `resources/schemas/drc.v1.json` is not valid JSON (trailing comma, line 92), so no schema
  validation is done; the JSON writer is compared byte for byte with the C++ writer instead.

### Known gaps

- Item lines of the reports (`@(x, y): description`, item positions / UUIDs in JSON) are verified
  by unit tests only until the board parser loads footprints and tracks (PCBIO-03 / 06) and the
  item descriptions of footprints, vias, zones, shapes, tables and dimensions are ported (model
  packages, currently `port_pending!`).
- The CLI flow (providers per marker type, severities from the job, messages, exit codes) is
  DRC-12; the reference reports wait for it.
