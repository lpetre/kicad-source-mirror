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

## DRC-02: DRC r-tree, zone index, provider base

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_rtree.h` `DRC_RTREE`: `Insert` (both overloads: sub-shape decomposition, `SHAPE_NULL` skip, invisible fields, atomic tables, pad hole entry, `recordPadding`), `Build`, `clear`, `CheckColliding`, `QueryColliding` (item / box-with-actual / box yes-no with the single-outline polygon fast path), `QueryCollidingPreparedCopper` (padding credit), `GetObjectsAt`, `QueryCollidingPairs`, `size`, `empty`, `OnLayer`, both `Overlapping` | `src/rtree.rs` (`DrcRtree`, `ItemWithShape`, `LayerPair`, `ATOMIC_TABLES`) | ported |
| `ITEM_WITH_SHAPE` (`parent`, `shape`, `shapeStorage`, `parentShape`) | `ItemWithShape { parent: ItemId, parent_shape: SharedShape, sub: Option<u32>, shape_storage }`, `shape()` via `Shape::indexable_subshape` | ported (payload per `drc.md`; the tree owns `Arc`s, so it is `Send + Sync`) |
| `KIRTREE::PACKED_RTREE<ITEM_WITH_SHAPE*, int, 2>` | `kicad_geom::geometry::rtree::PackedRTree<u32, 2, 16>` (payload = index into the owned entries) | same tree type and visit order as the C++ |
| `pcbnew/drc/drc_zone_index.h`: `CollectOverlappingPairs`, `DRC_ZONE_INDEX` (`Build`, `HasLayer`, `Query`, `Clear`) | `src/zone_index.rs` | ported |
| `pcbnew/drc/drc_test_provider.h/.cpp`: `s_allBasicItems`, `s_allBasicItemsButZones` (`Init`), `GetName`, `reportViolation`, `reportTwoPointGeometry`, `reportTwoShapeGeometry`, `reportTwoItemGeometry`, `reportProgress`, `reportPhase`, `forEachGeometryItem`, `isInvisibleText`, the three `formatMsg` overloads | `src/test_provider.rs` (`DrcTestProviderBase`, `DrcRunHost`, `for_each_geometry_item`, `all_basic_items`, ...) | ported; the engine side is the `DrcRunHost` trait (implemented by the engine run state) |
| `DRC_TEST_PROVIDER_REGISTRY` (static registration, link order) | `test_provider::DRC_TEST_PROVIDERS` (module, `GetName()`) | ported as data, order confirmed by the oracle |
| `DRC_SHOWMATCHES_PROVIDER_REGISTRY`, progress reporter plumbing | - | skipped: GUI; progress hooks are `DrcRunHost` defaults (no-ops returning true) |

### Verification

Differential oracle `tests/oracle/drc_rtree_oracle.cpp` (built by `tests/oracle/build_rtree_oracle.sh`
against the reference build like pcb-dump, under the machine-wide build lock; binary
`/home/user/kicad-oracle/bin/drc-rtree-oracle`). It runs the real `DRC_RTREE`, `DRC_ZONE_INDEX`,
`CollectOverlappingPairs` and `DRC_TEST_PROVIDER` code and prints: the provider registry,
`s_allBasicItems(ButZones)`, `GetName()`, 1096 `formatMsg` results (all units x data types x
value pairs incl. the "equal strings -> precise formatting" branch, plus angles),
`CollectOverlappingPairs` on pseudo-random boxes (empty / negative boxes, absent entries), and per
board: `forEachGeometryItem` for 7 type / layer lists (incl. an early stop), the copper tree as
the cache generator builds it (storage order per layer, size), and for every copper item, layer
and clearance: `QueryColliding` (plain, and with filter + stopping visitor: filter call counts),
the raw search order, `QueryCollidingPreparedCopper` with and without padding credit,
`CheckColliding`, `GetObjectsAt`, `QueryCollidingPairs` (full visit sequence with dedup, and the
visitor / progress stop), then one zone tree per copper zone (triangulated fills) queried with
`QueryColliding( box, shape, layer, clr, &actual, &pos )`, the yes / no overload and its polygon
fast path, and the `DRC_ZONE_INDEX` queries. Golden: `tests/golden/rtree_oracle.txt`
(`tests/oracle/gen_rtree_goldens.sh`, 8886 lines, deterministic over runs). Rust replay:
`tests/rtree_oracle.rs`.

| Input | Result |
|---|---|
| board-independent sections (registry, type lists, 1096 `formatMsg`, 3 `CollectOverlappingPairs` sets) | identical |
| `tests/data/rtree/rtree_small`, `rtree_ties` (Hilbert centre ties), `rtree_4layer` (multi-level trees, blind / buried / micro vias); `gen_rtree_boards.py` | identical (5855 lines) |
| `qa/data/pcbnew` `issue7267`, `issue18203`, `issue16566`, `net_tie_hole_clearance`, `via_stacks`, `connection_width_rules`, `issue24649` (pads, holes, graphics, fields, zones), `two_segment_teardrop`, `issue18142`, `fill_bad`, `issue14334` (filled zones: triangle entries, `actual` / position, polygon fast path) | pending: the board parser does not load footprints / zones yet (PCBIO-06, PCBIO-08); the replay reports them as pending and compares them automatically once they load |

### Ported C++ tests

| C++ | Rust |
|---|---|
| `qa/tests/pcbnew/drc/test_drc_prepared_candidates.cpp` (all 7 cases) | `tests/test_drc_prepared_candidates.rs`: the r-tree cases run on the synthetic boards with a fixed stand-in for `m_DRCMaxClearance` (the C++ fixture runs the whole DRC on QA boards with footprints), the zone index / pair cases on synthetic boxes; `DRCGraphicBoundsDoNotContainTheirEffectiveShape` runs on `issue22102` and reports itself as pending until that board loads |

### Deviations

- **`GetObjectsAt`** returns the parents without duplicates in tree search order (C++: an
  `std::unordered_set`, hash order); the oracle compares it sorted. Callers must not depend on
  the order (`drc.md` 4.12).
- **`QueryCollidingPairs` dedup key**: C++ orders the two parents by address; the port orders by
  `ItemId`. Only the canonical pairing matters (set membership), so results are identical.
- **Null pad shape**: C++ `PAD::GetEffectiveShape` returns a null pointer for a layer without a
  cached shape; kicad-pcb returns a bare `SHAPE_NULL` there. `Insert` and
  `QueryCollidingPreparedCopper` treat a bare null shape of a pad as the C++ null pointer
  (`wxCHECK` early return, so no hole entry either).
- Items without a handle (detached values) cannot be indexed (`Insert` skips them; debug assert).
- `wxCHECK` failures (insert after `Build`, `UNDEFINED_LAYER`) return early with a debug assertion.
- Filters and visitors receive the parent's `ItemId` (C++ `BOARD_ITEM*`); resolve with `ctx.item`.

### Known gaps

- Pads (hole entries), zones (triangulated fills, `actual` / position of the box queries, the
  polygon fast path), graphics, fields and tables are verified only once the parser loads the
  QA boards (PCBIO-06 / PCBIO-08); the code paths are ported statement by statement and the
  goldens already contain them.
- `reportViolation` / `reportTwo*Geometry` are checked by unit tests against a recording host;
  the engine side (`DRC_ENGINE::ReportViolation`, marker creation) is DRC-03.

## DRC-03: DRC engine core (implicit rules, rule compilation, run driver, error limits)

### File / class map

| C++ (`pcbnew/drc/drc_engine.cpp/.h`) | Rust | State |
|---|---|---|
| `DRC_ENGINE` split per `board-model.md` 7: rule set + compiled lists + memo caches | `src/engine/mod.rs` `DrcEngine` (`Sync`, implements `kicad_pcb::RuleResolver`) | ported |
| run state: `m_errorLimits`, `m_violationHandler`, `m_logReporter`, `m_progressReporter`, `m_reportAllTrackErrors`, `m_testFootprints`, user units | `src/engine/mod.rs` `DrcRunState` | ported |
| `InitEngine( const wxFileName& )` (provider creation log, marker rule reset, the clear steps, implicit + file rules, retry with implicit rules only on `PARSE_ERROR`, rethrow) | `DrcEngine::init_engine` / `init_engine_with::<B>` | ported (rule file text passed in) |
| `InitEngine( const std::shared_ptr<DRC_RULE>& )` | `DrcEngine::init_engine_with_rule` | ported (show-matches registry is empty in C++: nothing logged) |
| `createImplicitRule`, `isKeepoutZone`, `loadImplicitRules` (board setup rules 1-2b, barcode, per-netclass rules incl. composites sorted with `std::sort` (`kicad_geom::stdsort`), tuning-profile rules, keepout rules of board and footprint zones) | `src/engine/implicit_rules.rs` | ported |
| `loadRules` | `src/engine/mod.rs` `load_rules` / `load_rules_with::<B>` | ported (DRC-20) |
| `compileRules` (per-type `DRC_ENGINE_CONSTRAINT` lists, `implicitKeepoutZone`, `m_explicitConstraints`, `m_hasExplicitClearanceRules`, `m_hasGeometryDependentRules`, `PARSE_ERROR` on a compile error) | `DrcEngine::compile_rules` | ported |
| implicit rule conditions (`A.hasExactNetclass`, `A.inDiffPair`, `AB.isCoupledDiffPair`, `A.Via_Type ==/!= 'Micro'`, `A.Type == 'Barcode'`, `A.Layer == ...`, `A.intersectsKeepout`) | `src/engine/implicit_condition.rs` (`ImplicitCondition`, `EngineCondition`) | ported natively (analysis 4.8.5); `A.Layer` is `port_pending!` (DRC-21: `PCBEXPR_VAR_REF::GetValue`), `intersectsKeepout` is `port_pending!` (DRC-10) |
| `RunTests` (limits per code: 0 ignored / 499 clearance + unconnected / 199, time stamp, providers in registry order, cancel) | `DrcEngine::prepare_run`, `run_providers`, `run_tests`; `PROVIDER_ORDER` | ported; cache generation / component classes / from-to cache are a caller closure (DRC-05 and others) |
| `IsErrorLimitExceeded`, `GetErrorLimit`, `ReportViolation` (cap enforced in the call, handler, log lines) | `DrcRunState` | ported |
| `KeepRefreshing`, `AdvanceProgress`, `SetMaxProgress`, `ReportProgress`, `ReportPhase`, `IsCancelled`; `PROGRESS_REPORTER` | `DrcRunState`; `ProgressReporter` trait (no-op defaults) | ported |
| `HasRulesForConstraintType`, `QueryWorstConstraint`, `HasConditionalConstraint`, `HasUserDefinedConstraint`, `HasUserDefinedPhysicalConstraint`, `QueryDistinctConstraints`, `ClearClearanceCache`, `RulesValid`, `HasGeometryDependentRules` | `DrcEngine` | ported |
| `MatchDpSuffix`, `IsNetADiffPair`, `IsNetTieExclusion` | `src/engine/diff_pair.rs` | ported |
| `EvalRules`, `EvalZoneConnection`, `EvalClearanceBatch`, `ProcessAssertions`, `GetCachedOwnClearance`, `InvalidateClearanceCache`, `InitializeClearanceCache` | `src/engine/eval_rules.rs` | not this package (DRC-04); `RuleResolver::eval_rules` / `cached_own_clearance` are `port_pending!` |
| `GetItemsMatchingCondition`, `GetItemsMatchingRule`, show-match domains, `GetTestProvider`, debug overlay, drawing sheet, schematic netlist setters | - | skipped: GUI (rule editor) / providers are created per run |

### Verification

Differential oracle against the real `DRC_ENGINE`: opt-in section `pcb-dump board --sections
engine` (`rust/tools/pcb-dump/dump_engine.cpp`, format `rust/docs/PCB_DUMP_FORMAT.md` section
9) prints after the QA load path's `InitEngine` the provider registry order, the engine flags,
every rule (name, implicit source, layers, severity, netclass, item id, condition text, every
constraint with min/opt/max, flags, name, severity), both constraint maps (with condition usage
flags and keepout zones) and the netclass clearance table. Rust twin: `dump_engine` in
`tests/engine_oracle.rs`; compact golden `tests/golden/engine_corpus.txt` (FNV-1a per board,
`tests/oracle/gen_engine_goldens.py`, 367 boards: every `.kicad_pcb` < 5 MB under `qa/data/pcbnew`
and `demos`; re-generation spot-checked identical on 2 boards).

`engine_corpus_matches_cxx`: 0 mismatches. With the parser state of 2026-11-10 309 boards
matched (incl. 85 user rules with conditions, compiled with the DRC-20 stand-in binding
`tests/common::SyntaxBinder` through `init_engine_with::<B>`, and the 10 tuning-profile rules of
issue23868, profiles read from the project file by the test); the rest are reported as pending
with the reason (zones / keepouts / footprints skipped by the parser or `port_pending!` in the
zone parser, missing nets of skipped items, textbox and net-chain stubs of kicad-pcb, legacy
`.pro` projects, legacy netclasses). The provider order (`PROVIDER_ORDER`) is part of every
board's dump, i.e. confirmed by the reference build. The default-board rule list was confirmed
on an empty board file with the reference dump.

Not verified differentially yet: the evaluation of the native implicit conditions (statement-
by-statement review against `pcbexpr_functions.cpp`; the differential check comes with the
`EvalRules` oracle of DRC-04), keepout rules on real boards (all keepout boards wait for the
zone parser; unit-level only), `RunTests` with real providers.

### Ported C++ tests

| C++ | Rust (`tests/engine_oracle.rs`) |
|---|---|
| `qa/tests/pcbnew/drc/test_drc_error_limit.cpp`: `CapIsExact`, `InitEngineResetsLastErrorCode` | `cap_is_exact`, `init_engine_resets_last_error_code` |
| `qa/tests/pcbnew/test_board_design_settings.cpp`: `BiggestClearanceIncludesPhysicalHoleClearance` (real engine as resolver) | `biggest_clearance_includes_physical_hole_clearance` |

Plus `run_tests_error_limits`, `run_tests_provider_order_and_cancel`, `default_board_rule_list`,
`init_engine_clears_marker_rules_and_single_rule_overload` and unit tests in
`src/engine/diff_pair.rs` (`MatchDpSuffix` table) and `src/engine/implicit_condition.rs`.

### Deviations

- **Engine split**: the error limits live in `DrcRunState`; the limit reset at the end of both
  `InitEngine` overloads is `DrcRunState::reset_error_limits` (callers run it with the init).
  Test providers are created per run (`DrcTestProviderRun`), so `InitEngine` only logs their
  names (`PROVIDER_ORDER`).
- **Native implicit conditions**: implicit rules whose condition text is one of the shapes
  `loadImplicitRules` produces are evaluated natively instead of being compiled; anything else
  takes the compiled path. Same expression text (memo key) and usage flags.
- **Binding parameter**: `init_engine_with::<B>` / `load_rules_with::<B>` choose the
  `PcbExprBinder` for user rule conditions (default `PcbExprUcode`); not a C++ API, used by the
  tests while DRC-21 is pending.
- **Marker rule reset**: markers hold a copy of the rule data; it is cleared through
  `Board::modify_marker`, which also bumps the board time stamp (the C++ bumps it right after).
- `ReportViolation` takes the provider name instead of `aItem->GetViolatingTest()`; the
  handler mutex is unnecessary (sequential port).
- `m_netclassClearances` (an `unordered_map` used for lookups only) is a `BTreeMap`.
- Rule file text and tuning profiles are parameters (no file / project access in the library).

### Known gaps

- `A.Layer == '...'` (tuning-profile rules) and `A.intersectsKeepout()` evaluate through
  `port_pending!` stubs until DRC-21 / DRC-10.
- User rule conditions compile only with a stand-in binding until DRC-21 (`init_engine` with the
  default binding panics with `port_pending!` on a rule file with conditions).
- `TUNING_PROFILES` is not part of the Rust project model; callers build `TuningProfiles`.
