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
| `report_sections.scn`, `exclusions.scn`, `pending_items.scn` | the same on `qa/data/pcbnew/issue8003.kicad_pcb`, whose items resolve (item lines `@(x, y): description`, item JSON, creation of exclusion markers whose items exist) | `pending_items.scn`: identical. `report_sections.scn`, `exclusions.scn`: pending on the pad parser only (PCBIO-07; state of 2026-11-10, REV-DRC); the test reports them as pending and compares them automatically once the board loads completely |

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
| `qa/data/pcbnew` `issue7267`, `issue18203`, `issue16566`, `net_tie_hole_clearance`, `via_stacks`, `connection_width_rules`, `issue24649` (pads, holes, graphics, fields, zones), `two_segment_teardrop`, `issue18142`, `fill_bad`, `issue14334` (filled zones: triangle entries, `actual` / position, polygon fast path) | `connection_width_rules`, `issue24649`, `issue18142`, `two_segment_teardrop`, `via_stacks` (zones, triangulated fills, graphics): identical (1076 lines; state of 2026-11-10, REV-DRC). `issue7267`, `issue18203`, `issue16566`, `net_tie_hole_clearance`, `fill_bad`, `issue14334`: pending on the pad parser (PCBIO-07); the replay reports them as pending and compares them automatically once they load |

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
| `EvalRules`, `EvalZoneConnection`, `EvalClearanceBatch`, `ProcessAssertions`, `GetCachedOwnClearance`, `InvalidateClearanceCache`, `InitializeClearanceCache` | `src/engine/eval_rules.rs` | package DRC-04 (see its section) |
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

## DRC-05: cache generator and per-run board caches

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_cache_generator.h/.cpp` `DRC_CACHE_GENERATOR::Run` | `src/cache_generator.rs` (`DrcCacheGenerator::run`, split into the phase functions `prepare_board`, `compute_max_clearances`, `build_zone_lists`, `build_copper_item_rtree`, `build_courtyard_caches`, `build_zone_caches`, `init_zone_islands_map`) | ported; `run` reaches pending stubs of other packages (below) |
| `CONNECTIVITY_DATA::ClearRatsnest`, `Build`, `FillIsolatedIslandsMap` as called by `Run` | `cache_generator::CacheGeneratorConnectivity` (trait; implemented by the connectivity peer once ported) | interface |
| `BOARD` members `m_DRCZones`, `m_DRCCopperZones`, `m_DRCCopperZonesByLayer`, `m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance`, `m_CopperItemRTreeCache`, `m_CopperZoneRTreeCache` / `GetCopperZoneRTree`, `m_ZoneIsolatedIslandsMap`, `m_SolderMaskBridges` (constructor part of `board.cpp`) | `src/caches.rs` (`DrcBoardCaches`, `ZoneIslandsMap`, `new_solder_mask_bridges`) | ported |
| `BOARD` predicate caches `m_Intersects*Cache`, `m_EnclosedByArea*Cache`, `m_ItemFieldCache`, keys `PTR_PTR_CACHE_KEY`, `PTR_PTR_LAYER_CACHE_KEY`, `ITEM_SELECTOR_LAYER_CACHE_KEY`, `ITEM_FIELD_CACHE_KEY`; `SHARDED_CACHE` | `src/caches.rs` (`DrcPredicateCaches`, `MemoCache`) | ported (data only; owner: see Known gaps) |
| `BOARD::IncrementTimeStamp` (DRC part) | `DrcBoardCaches::clear` / a fresh value; `DrcPredicateCaches::clear` | ported |
| `BOARD::GetMaxClearanceValue`, `BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue`, `m_ZoneBBoxCache`, `m_maxClearanceValue`, courtyard caches | kicad-pcb (`Board::get_max_clearance_value`, `Zone::cache_bounding_box` / `bounding_box`, `Footprint::build_courtyard_caches` / `courtyard`) | used, not owned |

### Verification

Differential oracle: `drc-rtree-oracle --cachegen` (new mode `runCacheGen` in
`tests/oracle/drc_rtree_oracle.cpp`, built by `build_rtree_oracle.sh`; the DRC-02 output is
unchanged, its golden regenerates byte-identical). It loads each board like `KI_TEST::LoadBoard`
(project, `InitEngine` with the rule file, nets, connectivity), runs the real
`DRC_CACHE_GENERATOR::Run()` and prints `GetBiggestClearanceValue`, `GetMaxClearanceValue`,
`m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance`, the three zone lists, the copper item tree
(size, every entry per board copper layer in storage order), per zone the bounding box and the
zone tree (size, per layer entry count and FNV-1a of the entry list) and the islands map keys.
Golden `tests/golden/cachegen_oracle.txt` (`tests/oracle/gen_cachegen_goldens.sh`), replay
`tests/cache_generator_oracle.rs` (runs the phase functions).

| Input | Result |
|---|---|
| `tests/data/rtree/rtree_small`, `rtree_ties`, `rtree_4layer` (tracks, arcs, vias, 2 and 4 layers) | identical |
| `tests/data/cachegen/phys_cond` (only conditional user physical rules: conditional maximum incl. the implicit barcode rule, 1 mm), `phys_uncond` (unconditional 0.3 mm beats conditional 0.9 mm; clearance 2 mm; conditional hole-to-hole 2.5 mm reaches `GetBiggestClearanceValue`), `huge` (600 mm clearance clipped to `MAXIMUM_CLEARANCE`, 800 mm physical clamped to `INT_MAX / 3`) | identical |
| `qa/data/pcbnew` `issue4139`, `fill_bad`, `intersectingzones`, `issue7267`, `issue18203`, `net_tie_hole_clearance`, `via_stacks`, `issue14334`, `connection_width_rules`, `issue24649`, `issue5750` (pads, footprint zones, filled zones, projects, rule file) | `connection_width_rules`, `issue24649`, `via_stacks`: identical (state of 2026-11-10, REV-DRC). The other eight: pending on the pad parser (PCBIO-07); compared automatically once they load |

Not verifiable yet: `DrcCacheGenerator::run` end to end (it calls `Board::board_outline`,
`Footprint::build_courtyard_caches` and `Board::build_net_tie_cache`, all `port_pending!` in
kicad-pcb, and the connectivity rebuild, which has no implementation yet); zone bounding-box
caches, zone trees, islands maps and pad hole entries wait for the QA boards above.

### Ported C++ tests

| C++ | Rust |
|---|---|
| `qa/tests/pcbnew/drc/test_drc_physical_clearance.cpp` (`DRCPhysicalClearanceNotInflatedByConditionalRules`) | `tests/cache_generator_oracle.rs::drc_physical_clearance_not_inflated_by_conditional_rules` (cache-generator part; `issue4139` pending until it loads, same property checked on `rtree_small`) |

### Deviations

- **Phase order**: the derived-state writes of `Run` (outline / fill bounding-box caches, zone
  triangulation, net-tie caches) are done first (`prepare_board`), the reads after. They touch
  only zone and net-tie data, which the clearance values and the copper item tree do not read;
  each zone's bounding-box cache still precedes its triangulation, and the triangulation
  precedes the zone trees. The zone outline writes go through `Board::modify_zone`, which bumps
  the time stamp (no derived-state writer exists for the outline); harmless before the reads.
- **`allZones`** (`std::set<ZONE*>`, address order) is replaced by board zones then footprint
  zones; it only schedules independent per-zone work (`drc.md` 4.12).
- **`m_ZoneIsolatedIslandsMap`** (`std::map<ZONE*>`, iterated): insertion order = board zone
  order (`drc.md` 4.12). **`m_CopperZoneRTreeCache`** (`unordered_map`, iterated by the
  connection width provider): `ZoneId` order.
- **`ITEM_FIELD_CACHE_KEY`** keys by the field name instead of its `std::hash`.
- **Cancellation / progress**: the `IsCancelled()` checks inside the item and zone loops and the
  `reportProgress` calls of the 250 ms wait loops (thread-timing dependent) are not reproduced;
  `reportPhase` returning false cancels as in C++.
- **`m_SolderMaskBridges`** is created lazily in `DrcBoardCaches` instead of in the `BOARD`
  constructor, behind a lock (the solder mask provider writes it during a run).
- **Board outline**: `UpdateBoardOutline()` + `BuildBBoxCaches()` become `Board::board_outline`
  (lazily rebuilt per time stamp by kicad-pcb).

### Known gaps

- The predicate caches need an owner reachable from both rule evaluation and the disallow
  provider (proposed: `DrcEngine`); until then `DrcPredicateCaches` is unused.
- `run` cannot be reached from `DrcEngine::run_tests` yet: its `regenerate_caches` callback
  returns only `bool`, providers get no `&DrcBoardCaches`, and the connectivity is borrowed
  before the rebuild (change requests to DRC-03).

## DRC-04: constraint resolution (`EvalRules` and friends)

### File / class map

| C++ (`pcbnew/drc/drc_engine.cpp` / `.h`) | Rust | State |
|---|---|---|
| `EvalRules` (net-tie exemption, local overrides, netclass fast path, `processConstraint` / `applyConstraint` / `checkCondition` incl. the per-call condition memo, DIFF_PAIR_GAP clearance floor, footprint inheritance and board-setup defaults, legacy local clearances, board minimum, NULL result) | `src/engine/eval_rules.rs` `DrcEngine::eval_rules_reporting` / `eval_rules` | ported (reporter texts: hooks, DRC-34) |
| `EvalZoneConnection` | `DrcEngine::eval_zone_connection` | ported |
| `EvalClearanceBatch`, `DRC_CLEARANCE_BATCH` | `DrcEngine::eval_clearance_batch`, `DrcClearanceBatch` | ported |
| `ProcessAssertions` | `DrcEngine::process_assertions` | ported (assertion tests need DRC-21 to evaluate) |
| `GetCachedOwnClearance`, `ClearClearanceCache`, `InvalidateClearanceCache` | `DrcEngine::cached_own_clearance_impl`, `clear_clearance_cache`, `invalidate_clearance_cache`; `OwnClearanceCache` | ported |
| `InitializeClearanceCache` (GUI pre-population, thread pool) | - | skipped: pure optimisation for the editor's first render |
| `RuleResolver::eval_rules` / `cached_own_clearance` (`board-model.md` 7) | `src/engine/mod.rs` `impl RuleResolver for DrcEngine` | ported |
| `DRC_ENGINE_CONSTRAINT::constraint.m_Test`, `constraint.GetParentRule()` | `DrcEngineConstraint::test`, `rule_info` (filled by `compileRules`) | ported |

### Verification

* C++ oracle: new opt-in section `pcb-dump board --sections evalrules`
  (`rust/tools/pcb-dump/dump_evalrules.cpp`). After the QA load path's `InitEngine` it prints,
  per board track / arc / via / footprint pad / board zone / footprint zone / board shape and
  text, `EvalRules` of CLEARANCE and `GetCachedOwnClearance` per enabled copper layer and 15
  unary types (TRACK_WIDTH, VIA_DIAMETER, HOLE_SIZE, ANNULAR_WIDTH, EDGE_CLEARANCE,
  HOLE_CLEARANCE, PHYSICAL_CLEARANCE, SILK_CLEARANCE, TEXT_HEIGHT, SOLDER_MASK_EXPANSION, both
  SOLDER_PASTE margins, ZONE_CONNECTION, THERMAL_RELIEF_GAP, THERMAL_SPOKE_WIDTH) on the item's
  layer, DISALLOW (own category), and for a deterministic pair sample (track-track, pad-pad,
  track-pad, zone-track, pad-zone with `EvalZoneConnection` / thermal constraints, zone-zone)
  CLEARANCE per shared copper layer plus HOLE_CLEARANCE, HOLE_TO_HOLE and DIFF_PAIR_GAP. Each
  result prints type, min/opt/max presence and values, `GetName()`, the min rule's name, the
  implicit-min flag, zone connection, disallow flags and severity.
* Golden `tests/golden/evalrules_corpus.txt` (`tests/oracle/gen_evalrules_goldens.py`, 368
  corpus boards < 5 MB, `--hash`, streamed): per board and category the key count, line count
  and a hash; replay `tests/evalrules_oracle.rs`. A category is compared when the Rust board
  yields the same number of keys.
* Result (2026-11-10, REV-DRC): **1148 categories match on 99 boards fully, 0 mismatches** (arc
  23, disallow 94, drawing 270, fpzone 18, pair_tt 122, pair_zt 98, pair_zz 90, track 163, via
  122, zone 148). Pending: every pad-based category (620 categories; the parser does not load
  pads yet, PCBIO-07), DISALLOW on 36 boards whose items overlap a keepout
  (`intersectsKeepout`, DRC-10), 2 legacy `.pro` boards. 56 boards with user rule conditions
  are reported as "needs DRC-21" and not compared.
* User rules without conditions: the four review boards of REV-DRC (see its section) are
  compared completely (`evalrules_review_boards_match_cxx`).
* `fast_path_equals_general_path`: on three boards the netclass fast path (no reporter) equals
  the general path (forced by a reporter) for every item alone and two partners per item.
* `last_rule_wins_per_field_and_board_minimum`: per-field override by later rules (min/max from
  rule a, opt from rule b), explicit rule returned before the board minimum, own-clearance
  cache hit without source.

### Ported C++ tests

* `test_drc_solder_mask_expansion.cpp`: `FootprintLevelSolderMaskExpansion` (returns early with
  a "pending" note while the parser does not load pads; tightens automatically).
* Not ported here: `DRCNetclassNameEndingInBackslash` (needs `RunTests` with the providers),
  `DRCCopperGraphicLineEndingAgainstTrack` (needs DRC-08), `SolderMaskExpansionPerformance`
  (a timing test).

### Deviations

* Own-clearance cache invalidation: instead of the C++ `InvalidateClearanceCache` calls from the
  item setters the cache is stamped with `board.time_stamp()` and dropped whenever the stamp
  moved (`board-model.md` 7). It is therefore also dropped at every `RunTests` start; the cached
  value is always what `EvalRules` returns, so only C++ staleness bugs (edits that do not call
  `InvalidateClearanceCache`) could differ.
* `HOLE_TO_HOLE` with `b == None` dereferences null in C++ (router batch call); the port treats
  the missing item as one without a hole (analysis R5). `EvalZoneConnection` with a THT_THERMAL
  result, `a` not a pad and no `b` panics where the C++ dereferences null.
* `GetLocalClearance( &source )` of the model has no virtual with a source; the engine asks the
  two C++ overriders (pad: "pad", zone: "zone") directly.

### Known gaps

* REPORTER texts of `EvalRules` / `EvalZoneConnection` / `ProcessAssertions` (DRC-34): every
  site is marked `REPORT (DRC-34)`; the reporter control flow (no fast path, no condition
  memo, assertion tests evaluated) is in place.
* Footprint courtyards (`FOOTPRINT::GetCourtyard`, model stub) are reached by DISALLOW for a
  footprint item; keepout predicate (DRC-10) and the `Layer` property of tuning-profile rules
  (DRC-21) are reached through the native implicit conditions.
* `InitializeClearanceCache` not ported (editor only).

## DRC-24: courtyard clearance, text dimensions, text mirroring

### File / class map

| C++ | Rust | State |
|---|---|---|
| `drc_test_provider_courtyard_clearance.cpp` `DRC_TEST_PROVIDER_COURTYARD_CLEARANCE` (`Run`, `testFootprintCourtyardDefinitions`, `testCourtyardClearances`, `testPadAgainstCourtyards` lambda) | `providers/courtyard_clearance.rs` `DrcTestProviderCourtyardClearance` | ported (runs once kicad-pcb M20 courtyards land) |
| `drc_test_provider_text_dims.cpp` `DRC_TEST_PROVIDER_TEXT_DIMS` (`Run`, `checkTextHeight` / `checkTextThickness` lambdas) | `providers/text_dims.rs` `DrcTestProviderTextDims` (`check_text_height`, `check_text_thickness` public) | ported; outline-font glyph regeneration pending |
| `drc_test_provider_text_mirroring.cpp` `DRC_TEST_PROVIDER_TEXT_MIRRORING` | `providers/text_mirroring.rs` `DrcTestProviderTextMirroring` | ported |
| `BOARD::IsLayerVisible` | `providers/text_mirroring.rs` `board_is_layer_visible` (local helper; kicad-pcb has no such method yet) | ported |

### Verification

`tests/drc24_courtyard_text.rs` on the shared corpus harness (`tests/drc_harness`, golden
`tests/golden/drc_corpus.txt` from the reference `kicad-cli pcb drc --format json
--severity-all`), set and order per error type. State at the time of writing: 84 corpus boards
load completely and were compared; 281 are pending per test (about 220: the parser still
skips footprint contents; 55: user rule conditions need DRC-21; 2 legacy `.pro`; 2 barcode
stub; 1 failed reference run; in the text test `issue22475`, an outline-font text without
render cache). Result on the compared boards:
`mirrored_text_on_front_layer` 4 / 4 matched, `nonmirrored_text_on_back_layer` 4 / 4 matched,
0 missing, 0 extra, same order (`incorrect_text_mirroring_drc`). No compared board carries
courtyard or text dimension violations yet (every reference violation of those types is on a
board with footprints). On the partially loaded boards all 461 differences are missing entries
of skipped items, no extra entry.

Not verifiable yet:

* the whole courtyard provider: `FOOTPRINT::GetCourtyard`, `BuildCourtyardCaches`, the
  malformed flags and `FOOTPRINT::GetBoundingBox` are `port_pending!` in kicad-pcb (M20), and
  every courtyard board of the corpus has footprints the parser still skips;
* `text_height` / `text_thickness` (stroke font): reference violations are all on footprint
  fields / texts of boards that do not load completely yet;
* the TrueType branch of `checkTextThickness`: `text_thickness_truetype_render_cache_issue22475`
  runs it on the one text of `issue22475` whose render cache is in the file and compares with
  the reference entry (count and hash), but the board loader currently drops that parsed render
  cache (reported to kicad-pcb), so the test reports itself pending.

The tests skip a board (with a note) while it is pending and tighten automatically.

### Ported C++ tests

* `test_drc_courtyard_invalid.cpp` `InvalidCases` -> `courtyard_invalid_cases` (all five
  boards pending: footprints)
* `test_drc_courtyard_overlap.cpp` `OverlapCases` -> `courtyard_overlap_cases` (pending except
  `empty_board`)
* `test_drc_incorrect_text_mirror.cpp` `DRCIncorrectTextMirror` -> `drc_incorrect_text_mirror`
  (passes: 8 violations; the C++ `KI_TEST::FillZones` is not run, it cannot influence the
  provider)

### Deviations

* `GetCourtyard` returns a copy in the port; the `BuildBBoxCaches()` of the first pass are made
  on those copies, which the second pass reuses (`FootprintCourtyards`). Footprints the first
  pass does not keep (malformed, missing, pass skipped) are fetched again in the second pass,
  which is what the C++ shared cache holds then. When every courtyard error type is ignored the
  first pass is skipped and the box caches are those of the copies kicad-pcb returns.
* The outline-font branch of `checkTextThickness` uses the render cache read from the file
  (`eda_text::render_cache`); where C++ would regenerate the glyphs with the outline font engine
  the provider stops with `port_pending!` (no outline font engine in the port).
* The `ii` counter that `checkTextThickness` reuses from `Run` (C++ lambda capture by reference)
  is reproduced; it only affects progress values.
* Order: sequential port of single-threaded providers; the courtyard pairs follow the
  `std::sort` by UUID (libstdc++-exact `std_sort`), as in C++.

### Known gaps

* Outline font glyph generation (`OUTLINE_FONT::GetLinesAsGlyphs`) for texts without a valid
  render cache.

## DRC-06: unary providers (track width, via diameter, hole size, annular width)

### File / class map

| C++ (`pcbnew/drc/`) | Rust | State |
|---|---|---|
| `drc_test_provider_track_width.cpp` (`DRC_TEST_PROVIDER_TRACK_WIDTH`, "width") | `src/providers/track_width.rs` (`DrcTestProviderTrackWidth`) | ported |
| `drc_test_provider_via_diameter.cpp` (`DRC_TEST_PROVIDER_VIA_DIAMETER`, "diameter") | `src/providers/via_diameter.rs` (`DrcTestProviderViaDiameter`; the shared `layerDesc` lambda is `padstack_layer_desc`) | ported |
| `drc_test_provider_hole_size.cpp` (`DRC_TEST_PROVIDER_HOLE_SIZE`: `Run`, `checkPadHole`, `checkViaHole`) | `src/providers/hole_size.rs` (`DrcTestProviderHoleSize`) | ported |
| `drc_test_provider_annular_width.cpp` (`DRC_TEST_PROVIDER_ANNULAR_WIDTH`: `calcEffort`, `getPadAnnulusPts` incl. the same-number / overlapping pad handling of issues 19325 and 24340, `checkConstraint`, `checkAnnularWidth`) | `src/providers/annular_width.rs` (`DrcTestProviderAnnularWidth`) | ported |
| `m_drcEngine` as seen by the provider base during a run; `REPORT_AUX` | `src/providers/run_host.rs` (`RunHost`: `DrcRunHost` over `DrcRunState`; `report_aux`) | ported (shared by all providers) |

Each provider implements `engine::DrcTestProviderRun` (`RunTests( aUnits )` = set the user
units, then `Run()`); `run()` takes the engine (rules), the run state and the board context.
Violations: `item::create( code )`, `set_error_detail( formatMsg(...) )`, item ids = the item's
UUID (`SetItems( item )`), violating rule = `constraint.parent_rule`, then the base helpers
(`report_violation` with no path, `report_two_point_geometry`). All four walk the board
sequentially in C++ as well (track order, footprint x pad order): the violation order is
deterministic and the port reproduces it.

### Verification

Shared differential corpus harness (new, for every provider package):

* `tests/oracle/gen_drc_corpus.py` runs the reference
  `kicad-cli pcb drc --format json --severity-all --units mm` on a copy of every `.kicad_pcb`
  under `qa/data/pcbnew` and `demos` below 5 MB (365 boards; the large demo boards of PORTING.md
  section 7 are left out), sequentially, caches the raw reports under `rust/target/drc-corpus/`
  and writes `tests/golden/drc_corpus.txt` (132 KB): the full entries of the ported error types
  (`PORTED_TYPES` in the script; at most 20 per board and type) and, for everything else, one
  line per board and type with the count and an FNV-1a hash of the entries in report order. A
  provider package adds its types to `PORTED_TYPES` and reruns the script (cached reports make
  that take seconds).
* `tests/drc_harness/mod.rs` (`mod drc_harness;` from a test): loads a board like the CLI
  (parser, project, rule file, `InitEngine`, `ResolveDRCExclusions( true )`), runs `RunTests`
  with the given providers only (`prepare_run` + `run_providers`; no cache generator), turns the
  violations into the report as `JobExportDrc` does (markers, `RecordDRCExclusions`,
  `ResolveDRCExclusions( false )`, `DRC_ITEMS_PROVIDER::SetSeverities`, `DrcReport::json_report`
  of DRC-11), filters both sides to the provider's types and compares per type (matched / missing
  / extra multisets of (type, severity, excluded, description, items' uuid, description,
  position)) plus the order. Summary types compare count + hash; on a mismatch the entries are
  diffed from the cached raw report. `compare_corpus` returns a `Scoreboard` (`print`,
  `assert_no_mismatch`); boards the parser loads only partially (`ParseReport::skipped`), boards
  with user rule conditions (DRC-21), legacy `.pro` projects and load panics are listed as
  pending; differences on partially loaded boards are printed but do not fail.

Result (`tests/drc06_unary_providers.rs::unary_providers_match_reference_corpus`, 2026-11-10):
**84 boards compared completely, 0 differences**: annular_width 74 matched, drill_out_of_range
35, via_diameter 34, track_width 4, microvia_drill_out_of_range 0 (no corpus board has one);
0 missing, 0 extra, report order identical (incl. entries excluded in the project). Pending:
221 boards with skipped footprints (PCBIO-06), 55 boards with user rule conditions (DRC-21), 2
`PCB_BARCODE::SetPosition` stubs, 2 legacy `.pro` projects, 1 board the reference cannot load
(`plugins/fakeboard`). On the partially loaded boards every difference is explained by the
missing footprints: the pad violations are missing, and on 5 `odbpp` boards the 199-entry limit
of `drill_out_of_range` is then filled with via entries instead of pad entries.

Not verified yet: every pad code path (hole size of pads, the pad annulus incl. the slow
polygon path and the overlapping same-number pads of issues 19325 / 24340) - no corpus board
with footprints loads yet. The comparisons tighten automatically when PCBIO-06 lands. Marker
positions / layers are not part of the reports; they are checked only through the exclusion
identities of `issue17429` (positions) for vias.

### Ported C++ tests

| C++ | Rust (`tests/drc06_unary_providers.rs`) |
|---|---|
| `test_drc_annular_overlap_issue24340.cpp` (`AnnularOverlapIssue24340`) | `annular_overlap_issue24340` (pending: footprints not loaded; returns with a note) |
| `test_drc_regressions.cpp` `DRCFalseNegativeRegressions`: `issue12109` (8), `issue19325` (4, `hole_to_hole` ignored) | `false_negative_regressions_unary` with the four providers only: expected = the part of the C++ count the reference attributes to them (`issue12109` 7 of 8, the 8th is `via_dangling`; `issue19325` 4 of 4); pending: footprints not loaded |

`test_drc_multi_netclasses.cpp` (track width with rule conditions and a zone fill) waits for
DRC-21 and the zone filler.

### Deviations

- `RunHost` is a newtype over `&mut DrcRunState` implementing `DrcRunHost` (the engine run
  state does not implement it itself).
- The harness gives markers UUIDs from a private seeded generator: the board's default-seeded
  generator reproduces the UUIDs of boards written by a deterministically seeded KiCad
  (`issue10697`), and a marker with an item's UUID hijacks `ResolveItem`. Marker UUIDs are not
  part of the reports.

### Known gaps

- Pad paths unverified (above).
- The cache generator does not run in the harness (`run_tests` cannot reach it yet, DRC-05);
  providers that need the copper / zone trees must run it themselves until then.

## DRC-27: solder mask provider (silk-to-mask, mask bridges)

### File / class map

| C++ (`pcbnew/drc/drc_test_provider_solder_mask.cpp`) | Rust (`src/providers/solder_mask.rs`) | State |
|---|---|---|
| `DRC_TEST_PROVIDER_SOLDER_MASK` (`Run`, `addItemToRTrees`, `buildRTrees`, `testSilkToMaskClearance`, `testItemAgainstItems`, `testMaskItemAgainstZones`, `testMaskBridges`, `recordMaskAperture`, `maskApertureBridgeExcluded`, `checkItemMask`, `collectBridge` (bounded max-heap via `kicad_geom::stdsort` heap ops), `flushBridgeViolations`, `reportMaskApertureBridges`) | `DrcTestProviderSolderMask<'c>` (implements `DrcTestProviderRun`) | ported |
| static `addItemPolysWithEndings`, `isNPTHPadWithNoCopper`, `isMaskAperture`, `PENDING_BRIDGE::operator<`, `MASK_APERTURE_ITEM` | free functions / private structs of the module | ported |
| `m_fullSolderMaskRTree` (a `DRC_RTREE` holding the `m_SolderMaskBridges` zone) | `FullSolderMaskTree` (same `PackedRTree<_, 2, 16>`, same entry boxes, same `QueryColliding( box, shape, layer, clr, &actual, &pos )` body as `DrcRtree::query_colliding_box_actual`) | ported (see Deviations) |

### Verification

* Synthetic boards without footprints (`tests/data/solder_mask/`, generated with the reference
  goldens by `tests/oracle/gen_drc27_boards.py`): mask-only apertures (`gr_rect`, `gr_circle`,
  `gr_text`, a zone on B.Mask) exposing plain copper of 2-3 nets, exposed tracks / arcs with
  local margins, untented vias (both sides), a copper zone next to exposed copper, silkscreen
  lines / texts / circles over openings, a `bridged_mask` severity-ignore rule on F.Mask and a
  `silk_clearance` rule (message detail). Three boards x with / without `--all-track-errors`:
  **66 of 66 entries identical in report order** (type, severity, description, item UUIDs,
  item descriptions and positions) - `synthetic_boards_match_reference`.
* Marker positions (not in the JSON report; part of the exclusion identity): the Rust markers
  are written as DRC exclusions into a copy of the project and the reference is run on it -
  all **52 markers** (4 runs) come back excluded, a marker moved by 1 nm does not
  (`marker_positions_match_reference_via_exclusions`, needs the reference CLI).
* Shared corpus (`tests/golden/drc_corpus.txt` + cached raw reports): every board the port
  loads completely is compared for `solder_mask_bridge` and the one-item `silk_over_copper`
  entries (the two-item ones come from the silk clearance provider):
  `solder_mask_matches_reference_corpus`. Today 84 boards compare (0 entries on both sides);
  every board with footprints is pending (parser, PCBIO-06), as are those with user rule
  conditions (DRC-21). The test tightens automatically.

### Ported C++ tests (`tests/drc27_solder_mask.rs`)

`test_solder_mask_bridging.cpp` (`DRCSolderMaskBridgingTest`, `DRCSolderMaskBridgeDeterminismTest`),
`test_solder_mask_bridge_all_tracks.cpp` (both cases), `test_solder_mask_track_to_pad.cpp`,
`test_drc_solder_mask_npth_bridge.cpp`, the solder mask boards of `test_drc_regressions.cpp`
(`issue10906`, `issue14412`, `net_tie_drc`: 0; `issue18878`: 12). All their boards have
footprints, so they report themselves as pending today and run once the parser loads them.
The C++ refills zones (`KI_TEST::FillZones`) in three of them; the port runs on the fills saved
in the files until the zone filler can be used here. `test_drc_solder_mask_expansion.cpp`:
`FootprintLevelSolderMaskExpansion` is in DRC-04; the `SILK_MASK_EXPANSION_FIXTURE` cases run
with a web width of 0, i.e. in the silk clearance provider, not here.

### Deviations

* **Scratch zone**: the whole-board mask is kept in the provider (`solder_mask( layer )`)
  instead of being written into the fills of `DrcBoardCaches::solder_mask_bridges` (the fill
  writer of a detached `Zone` is crate-private in kicad-pcb). Only the GUI reads that zone.
  Likewise `m_fullSolderMaskRTree` is a local replica of the two `DRC_RTREE` calls, because
  `DrcRtree::insert` cannot index an item without a handle.
* **Board caches**: `m_DRCCopperZones` / `GetCopperZoneRTree` come from a `&DrcBoardCaches`
  given to `DrcTestProviderSolderMask::new` (`DrcTestProviderRun::run_tests` has no caches
  parameter yet).
* **Thread pool**: `submit_loop` runs in item order (a cancelled run skips the remaining
  items); the 100 ms progress polling of the wait loop is not reproduced. The C++ collects
  and sorts the bridges itself, so the reported set and order do not depend on scheduling.
* `m_checkedPairs` / `reportedPairs` key on `ItemId` pairs instead of addresses (dedup only).
  `m_maskApertureItems` is a `HashMap`; its keys are sorted with the libstdc++ `std::sort` port
  by (UUID, layer) like the C++, so the order differs only for two apertures with the same
  UUID (C++: hash order, not deterministic either).
* `Run` takes `PCB_TRACK::GetSolderMaskExpansion` (not virtual) for vias in `Tracks()`
  (`Track::as_track()`), like the C++; `addItemToRTrees` adds no expansion for arcs (the C++
  only tests `PCB_TRACE_T`).

### Known gaps

* Not verified yet: pads (aperture / net-tie / free-pad / same-logical-pad / NPTH paths,
  `allow_soldermask_bridges`), footprint graphics and footprint zones - every QA board with
  them waits for the footprint parser (PCBIO-06); the ported tests and the corpus replay run
  them automatically once they load.
* The runs use a context without connectivity (like the shared harness): conditionally
  flashed vias / pads are not flashed on unconnected layers there.

## DRC-07: connectivity provider (dangling, isolated copper, unconnected items)

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_test_provider_connectivity.cpp` `DRC_TEST_PROVIDER_CONNECTIVITY::Run` (dangling tracks / vias incl. `VIA_DANGLING_CONSTRAINT`, track on post-machined / backdrilled layer, track not centered on via, isolated copper, unconnected items) | `src/providers/connectivity.rs` `DrcTestProviderConnectivity` | ported |
| `BOARD::GetCopperZoneRTree( zone )->QueryColliding( bbox, shape, layer )` as `TestTrackEndpointDangling` reads it | `connectivity::DrcZoneRtrees` (`kicad_connectivity::data::CopperZoneRtrees` over `DrcBoardCaches`) | ported |

API: `DrcTestProviderConnectivity::new( &mut ConnectivityData, &DrcBoardCaches )` borrows the
run's connectivity (rebuilt by the cache generator: `Build`, then
`FillIsolatedIslandsMap( m_ZoneIsolatedIslandsMap, true )`) and the cache generator's caches
(islands map, copper zone trees). The item shapes of the track loops are read with that
connectivity as `ctx.connectivity` (FlashLayer of vias / pads). `tests/drc07_connectivity.rs`
`ConnectivityWithCaches` shows the wiring until `DrcEngine::run_tests` runs the cache generator
(DRC-05 known gap).

### Verification

* `connectivity_matches_reference_corpus`: the reference `kicad-cli pcb drc --format json
  --severity-all --units mm` reports of the shared corpus (`violations` in
  `tests/golden/drc_corpus.txt`, DRC-07 types added to `gen_drc_corpus.py`; the
  `unconnected_items` array in `tests/golden/drc07_unconnected.txt`,
  `tests/oracle/gen_drc07_unconnected.py`, same format, same cached reports). Run path as the
  CLI: `BuildConnectivity` at load, `prepare_board` + `connectivity->Build`, zone lists / zone
  trees / islands map, the provider, exclusions, the JSON report. State 2026-11-10: 111 boards
  compared (254 pending: 250 footprints skipped by the parser, 2 legacy `.pro`, 1 reference
  failure, 1 board whose `VIA_DANGLING` rules have user conditions, DRC-21; user conditions
  of other constraint types are never evaluated by this provider and do not gate): isolated_copper 167 matched, track_dangling 36, via_dangling 96,
  unconnected_items 30, 0 missing, 0 extra. Report order equal except `isolated_copper` on
  `bad_triangulation_case` and `issue23790`: the zones come in a different order (C++
  `ZONE*` address order, port board order; confirmed against the file order: the entries of
  each zone are identical and contiguous) - the documented `drc.md` 4.12 deviation; the test
  reports it without failing.
* `synthetic_post_machined_board_matches_reference`: `tests/data/drc07/post_machined.kicad_pcb`
  (generator `tests/oracle/gen_drc07_boards.py`, reference report through the same cache):
  backdrilled / tertiary-drilled vias, tracks on drilled and undrilled layers, off-centre ends
  alone on the layer / next to a centred track / both ends in one via, dangling items, an
  unconnected net: 23 entries (4 `track_on_post_machined_layer`, 5
  `track_not_centered_on_via`, 11 `track_dangling`, 2 `via_dangling`, 1 unconnected),
  identical including order. No corpus board the port loads has those two types.

### Ported C++ tests

| C++ | Rust (`tests/drc07_connectivity.rs`) |
|---|---|
| `qa/tests/pcbnew/drc/test_drc_copper_thieving.cpp` `ThievingZoneProducesNoIsolatedCopperViolations` | `thieving_zone_produces_no_isolated_copper_violations` |
| `test_drc_regressions.cpp` `issue14334` (2 violations of all providers) | covered by the corpus replay (the board's connectivity entries match); the all-provider count needs the other providers |

### Deviations

* `ConnectedItems()` order is item-list order instead of `CN_ITEM*` address order
  (kicad-connectivity R-1): it decides which via / pad is named when a track touches several
  post-machined / off-centre candidates (none of the compared boards has such a track).
* `m_ZoneIsolatedIslandsMap` is iterated in board zone order instead of `ZONE*` address order
  (`drc.md` 4.12, see verification).
* `ItemEntry( item )` of an item unknown to the connectivity yields no items (C++
  `operator[]` inserts an empty entry: same result).
* The `wxCHECK`s of the unconnected-edge callback are `debug_assert!` + the same early return.

### Known gaps

* The harness reproduces the cache generator's connectivity rebuild before the run (it mutates
  the board: net propagation), not inside `RunTests`; equivalent because nothing between
  `prepare_run` and the cache generator reads nets.
* Boards with footprints (pads) wait for the footprint parser; the corpus replay compares
  them automatically once they load.

## DRC-26: silk clearance and physical clearance

### File / class map

| C++ | Rust | State |
|---|---|---|
| `drc_test_provider_silk_clearance.cpp` `DRC_TEST_PROVIDER_SILK_CLEARANCE::Run` (silk / target trees, rule-area skip, 18 layer pairs through `QueryCollidingPairs`, tented items by their hole, board-outline filter with the per-subshape collision cache of issue 24007, mask layers as `silk_over_copper`, UUID order on same-layer pairs) | `providers/silk_clearance.rs` `DrcTestProviderSilkClearance` (`Default`, `DrcTestProviderRun`) | ported |
| `drc_test_provider_physical_clearance.cpp` `DRC_TEST_PROVIDER_PHYSICAL_CLEARANCE` (`Run`, `testItemAgainstItem`, `testItemAgainstZones`, `testShapeLineChain`, `testZoneLayer`) | `providers/physical_clearance.rs` `DrcTestProviderPhysicalClearance::new(&DrcBoardCaches)` (`DrcTestProviderRun`) | ported |
| `ADVANCED_CFG::m_SliverAngleTolerance` | `physical_clearance::ADVANCED_CFG_SLIVER_ANGLE_TOLERANCE` (default 20) | constant (no advanced config in the port) |

### Verification

* Synthetic boards against the reference `kicad-cli pcb drc --format json --severity-all`:
  `tests/oracle/gen_drc26_boards.py` writes `tests/data/silk_physical/*` and the goldens
  `tests/golden/drc26/*.txt` (every entry of the two providers in report order: `silk_overlap`,
  two-item `silk_over_copper`, `clearance` / `hole_clearance` naming the physical rules or
  "Internal clearance violation"). Boards: `silk_default`, `silk_setup` (board setup silk
  clearance), `silk_rule_web` (rule + solder mask min width > 0) with silk against silk, every
  target layer, tented / untented vias, mask openings, the board edge and silk outside it, a
  silk rule area (issue 24177), board-level compound graphics; `physical` / `physical_small`
  (unconditional `physical_clearance` / `physical_hole_clearance`) with tracks, vias (hole
  layers), edge cuts, a notched copper polygon, arcs, a hairpin bezier, a zone with two fill
  islands and a fractured hole, a copper text. Result (`synthetic_boards_match_reference`):
  **41 / 41 entries identical, same order** (silk 3 + 3 + 8, physical 24 + 3).
* Corpus (`silk_and_physical_match_reference_corpus`, shared harness golden + cached raw
  reports): 104 boards load completely and were compared: `silk_overlap` 6 / 6 matched,
  `silk_over_copper` (two-item) 1 / 1, physical `clearance` / `hole_clearance` 0 / 0 (no
  loadable corpus board has a physical violation), 0 missing, 0 extra, same order. Pending 261:
  151 panic in the `FOOTPRINT::ResolveTextVar` stub of kicad-pcb, 51 parser skips, 56 need
  DRC-21 (rule conditions; this includes `keepout_no_clearance`, `issue2512` and the other
  physical-rule boards), 2 legacy `.pro`, 1 failed reference run. Physical entries of the
  reference are told apart from the copper / hole clearance providers' by the constraint names
  of the engine's physical constraints in the description.
* Not verified yet: everything that needs footprints or pads (pad holes on all layers, the
  `FlashLayer` branch of `testItemAgainstZones`, footprint courtyards in the physical tree,
  the footprint rotation of `testShapeLineChain` positions, footprint zones via
  `GetBoardOutline`, footprint graphics as compound silk), barcodes (implicit physical rule),
  tables / table cells (atomic tables). These tighten automatically as the boards load.

### Ported C++ tests

* `test_drc_silk_rule_area_issue24177.cpp` `RuleAreaDoesNotCauseSilkOrMaskViolationsIssue24177`
  -> `rule_area_does_not_cause_silk_or_mask_violations_issue24177` (silk provider part: 0
  `silk_overlap` / `silk_over_copper`; the solder mask part is DRC-27's, the silk edge part
  the edge clearance provider's).
* `test_drc_physical_clearance.cpp` is a cache-generator test (DRC-05, already ported).
  `DRCKeepoutNoClearanceInflation` (`keepout_no_clearance`) checks `items_not_allowed` only
  (disallow provider, DRC-10) and its rule needs DRC-21.

### Deviations

* `checkedPairs` (`std::unordered_map<PTR_PTR_CACHE_KEY, LSET>`, lookups only) is keyed by the
  two `ItemId`s in handle order instead of addresses; the silk outline collision cache is keyed
  by the tree entry instead of its `const SHAPE*` (a pure memo; same results).
* Silk: `BoardOutline()->GetOutline()` is `Board::board_outline` (rebuilt per time stamp);
  `BBoxFromCaches()` is taken from a copy with `build_bbox_caches()` (C++: the caches built by
  the cache generator), same box.
* Physical: `RebuildBezierToSegmentsPointsList( errorMax )` runs on a copy of the shape (C++
  rewrites the board item's derived bezier cache; same points).
* `testShapeLineChain` on a chain of one short segment reads past the end of `angles` in C++;
  the value is never compared then (no candidate range), the port uses 0.
* `UNIMPLEMENTED_FOR` (an undefined shape) is a debug assertion.

### Known gaps

* `testItemAgainstItem` / `testItemAgainstZones` reach `FOOTPRINT::GetEffectiveShape` /
  courtyards (kicad-pcb M20) for footprints; physical rules with conditions need DRC-21.

## DRC-25: zone connections (starved thermal reliefs)

### File / class map

| C++ (`pcbnew/drc/drc_test_provider_zone_connections.cpp`) | Rust (`src/providers/zone_connections.rs`) | State |
|---|---|---|
| `DRC_TEST_PROVIDER_ZONE_CONNECTIONS` (`Run`, `testZoneLayer`: quick tests, `EvalZoneConnection`, `MIN_RESOLVED_SPOKES` / `THERMAL_RELIEF_GAP`, spoke counting from the fill outlines with unique intersection points, single-connection islands as ignored spokes, custom-pad proxy spokes, manual spokes from `GetConnectedTracks` / `GetConnectedItems( EXCLUDE_ZONES )` copper shapes, both messages) | `DrcTestProviderZoneConnections<'c>` (implements `DrcTestProviderRun`) | ported |

API: `DrcTestProviderZoneConnections::new(&mut connectivity, &caches)` with the run's
`ConnectivityData` (built; an empty one reproduces a board without connectivity) and a
`DrcBoardCaches` with `build_zone_lists` run and `zone_isolated_islands` filled
(`init_zone_islands_map`, then `FillIsolatedIslandsMap( map, true )`).

### Verification

* Synthetic boards (`tests/data/zone_connections/`, `tests/oracle/gen_drc25_boards.py`): GND
  zones on F.Cu / B.Cu filled by the reference (`--refill-zones --save-board` on a copy), 13
  single-pad footprints (pads inside, on the edges and corner of the zone, just outside it,
  spokes blocked by SIG tracks, a closed SIG ring (island only), pockets closed by the pad's
  antipad, a GND track and a GND copper line as manual spokes, PTH pads on both layers, an
  other-net pad), once with the default spoke count and once with an unconditional
  `min_resolved_spokes 3` rule (the message names the `THERMAL_RELIEF_GAP` constraint, as in
  C++): **9 of 9 `starved_thermal` entries identical** (description, item UUIDs, item
  descriptions and positions), order identical within every (zone, layer) task
  (`synthetic_boards_match_reference`). The pads are added through the model API from a
  sidecar table because the parser does not read pads yet (PCBIO-07).
* Shared corpus (`zone_connections_match_reference_corpus`, `DRC25_FILTER=<substring>`):
  104 boards compare today, none with `starved_thermal` entries on either side; every board
  with pads is pending (pad parser, PCBIO-07) or needs DRC-21. The 10 corpus boards with
  reference `starved_thermal` entries (`test_starved_thermal`, `issue14334`, `issue18`,
  `issue3812`, `interf_u`, ...) compare automatically once they load.
* Not reached by the verification data yet: the "spokes connected to isolated island"
  message (both sides count the pocket boards identically, but no reference entry carries
  it), custom pads with proxy spokes, conditionally flashed pads.

### Ported C++ tests (`tests/drc25_zone_connections.rs`)

`test_drc_starved_thermal.cpp` (`DRCStarvedThermal`: `test_starved_thermal` 2, `issue19090` 0)
and the zone connection boards of `test_drc_regressions.cpp` (`issue14334` 2, `issue19090` 0,
`issue15280` 0 - the latter is the reference CLI's count: the C++ false-positive test ignores
`starved_thermal`). All have pads, so they report themselves as pending until PCBIO-07. The
C++ refills the zones in `DRCStarvedThermal` / `DRCZoneFalsePositiveRegressions`; the port
runs on the saved fills until the zone filler is ported (the reference CLI, which does not
refill, reports the same counts for these boards).

### Deviations

* **Thread pool**: the (zone, layer) tasks run in list order (`m_DRCCopperZones` order, layers
  in `LSET` order), i.e. the single-threaded order. In C++ the interleaving of different
  tasks in the report depends on scheduling (seen on `thermals_rule`: the B.Cu task's entries
  come between the F.Cu task's); the tests compare the set and the order within each task.
  The set can differ only when the per-code error limit is reached. The 250 ms progress
  polling is not reproduced.
* **Connectivity**: `GetConnectedItems` needs `&mut ConnectivityData` in the port (it stores
  its clusters), so the provider takes the connectivity separately from `ctx.connectivity`;
  the tests build a second copy for the context (see change requests). `GetConnectedTracks`
  is ordered by `ItemId` instead of address (only counted).
* **Board caches**: `m_DRCCopperZones` / `m_ZoneIsolatedIslandsMap` come from the
  `&DrcBoardCaches` given to `new` (`DrcTestProviderRun::run_tests` has no caches parameter).

### Known gaps

* Verification of real boards waits for the pad parser (PCBIO-07); DRC-21 for boards with
  rule conditions.

## REV-DRC: review of the engine core

Reviewer pass over DRC-20, DRC-11, DRC-02, DRC-03, DRC-04 and DRC-05 against the C++
(`rust/docs/design/review-drc-core.md`: findings R-01 .. R-09, the table of order-sensitive
sites, the list of unverified pad branches). No port code had to change: the reading found no
deviation that is not already listed in the sections above.

### Files

| File | What |
|---|---|
| `tests/oracle/gen_review_boards.py` | writes the review boards `tests/data/review/rev_{implicit,fastpath,explicit,clearance}.kicad_pcb` / `.kicad_pro` / `.kicad_dru` |
| `tests/oracle/gen_evalrules_goldens.py` | new option `--out GOLDEN` (the corpus golden is unchanged) |
| `tests/golden/evalrules_review.txt` | reference `EvalRules` results of the review boards (compact format of the corpus golden) |
| `tests/evalrules_oracle.rs` | the replay is a function (`replay_golden`) used by the corpus test and by the new `evalrules_review_boards_match_cxx`; `EVALRULES_RAW_DIR=<dir>` writes the unhashed Rust lines per board for diffing against `pcb-dump board --sections evalrules` |

### Verification

`pcb-dump board --hash --sections evalrules` (the DRC-04 oracle) on the four review boards:
one pad-less 4-layer board (36 tracks of nine nets, through / blind / buried / micro vias,
copper and silk graphics and texts, three copper zones with local clearances, two keepouts
that no item reaches), five netclasses assigned by pattern (one net in two classes: a
composite class; equal clearances: `std::sort` ties), and

| Board | Exercises | Result |
|---|---|---|
| `rev_implicit` | implicit rules only, diff-pair gap below the class clearance (the `AB.isCoupledDiffPair()` override, general path) | 8 categories identical |
| `rev_fastpath` | implicit rules only, no override (netclass fast path) | 8 categories identical |
| `rev_explicit` | 23 unconditional user rules of 17 constraint types: `(layer inner / outer / "B.Cu")`, several rules per type (min / max from one rule, opt from another), `(severity ...)`, 600 mm hole clearance (clamped), `disallow` masks and layers, hole-to-hole, diff-pair gap raised to the pair clearance, two rules with the same name | 8 categories identical |
| `rev_clearance` | user clearance rules per layer, 700 mm clamped to `MAXIMUM_CLEARANCE`, an opt-only rule, against zone local clearances and the board minimum | 8 categories identical |

(1570 result lines per board; categories track, via, zone, drawing, disallow, pair_tt,
pair_zt, pair_zz.) This is the first differential coverage of the user-rule half of
`EvalRules`: the corpus boards whose rule files have conditions need the expression binding
and are skipped as a whole.

### Deviations

None added.

### Known gaps (open findings)

- R-01: a constraint's rule copy (`DrcRuleInfo`) has no identity; providers that group by
  `DRC_RULE*` (matched length) cannot tell two same-named rules apart
  (`rust/docs/packages/FIX-DRC-1.md`).
- R-02: `DrcEngine::run_tests` cannot drive a run with providers that borrow the cache
  generator's caches, and nothing enforces `PROVIDER_ORDER` on the provider slice (DRC-12).
- R-06: every pad-related branch (list in the review document, section 4) is unverified until
  PCBIO-07; the oracles compare them by themselves once pads load.

## DRC-28: track angle, track segment length, via stack

### File / class map

| C++ (`pcbnew/drc/`) | Rust | State |
|---|---|---|
| `drc_test_provider_track_angle.cpp` (`DRC_TEST_PROVIDER_TRACK_ANGLE`, "track angle") | `src/providers/track_angle.rs` (`DrcTestProviderTrackAngle::new( &ConnectivityData )`) | ported |
| `drc_test_provider_track_segment_length.cpp` (`DRC_TEST_PROVIDER_TRACK_SEGMENT_LENGTH`, "segment_length") | `src/providers/track_segment_length.rs` (`DrcTestProviderTrackSegmentLength`) | ported |
| `drc_test_provider_via_stack.cpp` (`DRC_TEST_PROVIDER_VIA_STACK`, "microvia_stack": `Run`, `checkSpan`, `checkColumn`, `checkLooseMicrovias`, `stackThickness`, `checkAspectRatio`) | `src/providers/via_stack.rs` (`DrcTestProviderViaStack`) | ported |
| `PCB_VIA_STACK::IsSpanValid` (`pcbnew/generators/pcb_via_stack.cpp`, static) | `providers::via_stack::is_span_valid` | ported here (not in kicad-pcb yet; change request to move it) |

The track angle provider takes the board's connectivity (`BOARD::GetConnectivity()`) in its
constructor; `CONNECTIVITY_DATA::GetConnectedTracks` is `ConnectivityData::connected_tracks`
(needs `&self` only). `PCB_VIA::CollectMicroviaColumns` (kicad-pcb M17), the generator's
`via_stack_data()` (start / end layer), `BOARD::GetStackupOrDefault` and `BOARD::GetPad(
pos, layers )` (`get_pad_at`) come from kicad-pcb.

### Verification

* `tests/oracle/gen_drc28_boards.py` (run with `python3 -I`, reference sequential, on copies)
  writes 30 boards under `tests/data/drc28/` (with `.kicad_dru` / `.kicad_pro`) and the
  reference entries of the 7 DRC-28 types (`kicad-cli pcb drc --format json --severity-all
  --units mm`) to `tests/golden/drc28/`: track angle boards (corners, bends, straight joints,
  T junctions, a three-way node, reversed segments, a joint on a via, an arc in a chain,
  crossing segments of another net, B.Cu, nearly straight / nearly square joints, min+max,
  min only, a joint on an SMD pad), a segment length board (lengths around both limits incl.
  the "equal strings -> precise formatting" branch, arcs, B.Cu, a via), the in-code boards of
  `test_drc_via_stack.cpp` rebuilt as files (explicit stackups with sublayers, cores, thin
  inner foils; generator stacks; PTH / NPTH pads; rule severities in the project), extra via
  stack variants (span naming an absent layer, a gap, a non-microvia stack, a back-side stack)
  and a `%.2f` formatting board, plus the `qa/data/pcbnew/drc_via_stack` boards with the C++
  test's rule files.
* `tests/drc28_track_angle_via_stack.rs::synthetic_boards_match_reference`: **25 of 30 boards
  compared, 94 reference entries, 0 differences** (via stack types in exact report order;
  `track_angle` / `track_segment_length` as a multiset plus the order within one track).
  Pending: `angles_pads`, `vs_pads`, `vs_npth` (the parser skips pads: PCBIO-07),
  `vs_via_type_condition`, `qa_MicroviaStackDepthRuleIsHonoured_cond` (rule conditions:
  DRC-21 binding not used by the harness yet). Their goldens are committed; they are compared
  automatically once they load.
* Shared corpus (`track_angle_via_stack_match_reference_corpus`, 2026-11-10): **104 boards
  compared completely, 0 differences**: `microvia_crosses_core` 24 matched,
  `microvia_stack_not_filled` 11, `malformed_microvia_stack_span` 1; no corpus board has a
  `track_angle`, `track_segment_length`, `microvia_stack_depth` or `hole_aspect_ratio`
  violation (they need rules: covered by the synthetic boards). 202 partially loaded boards
  (pads skipped) show 0 differences as well; 60 boards with rule conditions, 2 legacy `.pro`
  projects and `plugins/fakeboard` are pending.

Not verified yet: the pad paths (the pad exclusion of the track angle test via
`BOARD::GetPad`, the PTH pad aspect ratio loop) and rule conditions - the boards are in place.

### Ported C++ tests (`tests/drc28_track_angle_via_stack.rs`)

| C++ | Rust |
|---|---|
| `test_drc_via_stack.cpp` (`DRCViaStack`, all 35 cases) | `drc_via_stack`: each case's expected counts checked on the reference golden and on the port (13 board-file cases run directly on `qa/data/pcbnew/drc_via_stack`; the rule and in-code cases on the generated boards); `PlatedPadHoleUsesItsNarrowDimension`, `UnplatedHoleIsNotChecked` (pads) and the condition cases (`MicroviaStackDepthRuleIsHonoured` third part, `AViaTypeConditionScopesTheLimit`) check the reference only and print as pending |
| `test_drc_regressions.cpp` `DRCFalseNegativeRegressions` `issue18142` | `issue18142`: with the default severity the port reports exactly the reference's one `microvia_crosses_core` and nothing else of the DRC-28 types (the C++ count of 1 is the `hole_to_hole` violation once `microvia_crosses_core` is ignored) |

### Deviations

- Thread pool: the track angle and segment length tests run on `submit_loop` in C++ (one task
  per track index); the port runs them sequentially in track order. The report order across
  tracks is scheduling dependent in C++ (`drc.md` 4.12); the set is the same. Within one
  track the connected tracks come from a `std::set<PCB_TRACK*>` (address order) in C++ and in
  `ItemId` order (board track order) in the port: the order of several violations of one
  track can differ from a C++ run (the synthetic boards happen to match).
- A lambda task returning false (error limit reached) ends only its own index in C++; the
  port stops the loop, which is equivalent because every later index returns false too.
- `PCB_GROUP::GetBoardItems` (an unordered set in C++) is the member list order; `checkSpan`'s
  outcome does not depend on it.
- `int actual = GetLength()`: `f64_to_i32_x86` (out-of-range lengths of degenerate arcs).

### Known gaps

- Pad paths and rule conditions unverified (above).

## DRC-23: misc provider and footprint checks

### File / class map

| C++ | Rust | State |
|---|---|---|
| `drc_test_provider_misc.cpp` `DRC_TEST_PROVIDER_MISC::Run`, `testUnquotableNetclasses`, `testOutline` (error handler with the marker heuristics, `findClosestOutlineGap`, "(Suspicious items ...)" / "(no edges found ...)"), `testDisabledLayers`, `testTextVars` (incl. the drawing-sheet loop), `testAssertions`, `testMissingTuningProfiles` | `providers/misc.rs` `DrcTestProviderMisc` (`Default`, `new(TuningProfiles)`, fields `drawing_sheet`, `environment`; `DrcTestProviderRun`) | ported |
| `varRefRegEx`, `warningExpr`, `errorExpr` (`wxRegEx`) and the `reportEach` loop | `misc.rs` `var_ref_matches`, `match_marker` (hand-written matchers of these three expressions) | ported |
| `common/common.cpp` `ExpandEnvVarSubstitutions` / `KIwxExpandEnvVars` (no project, Unix), `env_vars.cpp` `IsVersionedEnvVar`, `GetPredefinedEnvVars` | `misc.rs` `expand_env_var_substitutions` (private) | ported (environment passed in) |
| `drc_test_provider_footprint_checks.cpp` `DRC_TEST_PROVIDER_FOOTPRINT_CHECKS::Run` | `providers/footprint_checks.rs` `DrcTestProviderFootprintChecks` | ported |
| `pcbnew/footprint_utils.cpp` `CheckFootprintAttributes`, `CheckPads`, `CheckNetTies`, `CheckNetTiePadGroups` | `src/footprint_utils.rs` | ported |
| `footprint_utils.cpp` `ComputeFootprintShift`, `CheckShortingPads`, `CheckClippedSilk` | - | skipped: no DRC caller (footprint update / footprint editor checker) |

### Verification

* Corpus (`tests/drc23_misc_footprint.rs` `misc_and_footprint_checks_match_reference_corpus`,
  golden `tests/golden/drc23/corpus.txt` written by `tests/oracle/gen_drc23_golden.py` from the
  cached raw reference reports of `gen_drc_corpus.py`; the random board UUID of
  `SetItems( m_board )` is normalised to `board`): 104 boards load completely and were
  compared: `invalid_outline` 62 / 62, `generic_warning` 2 / 2, `generic_error` 3 / 3,
  `missing_tuning_profile` 1 / 1, 0 missing, 0 extra, same order. 6 drawing-sheet
  `unresolved_variable` entries are not compared (no drawing sheet, below). Pending 260: the
  parser skips pads (PCBIO-07) on every board with footprints, 56 boards need DRC-21 (rule
  conditions: all assertion boards), 2 legacy `.pro`. On the partially loaded boards (information
  only, printed by the test) 33 entries match; the 13 missing (`footprint_type_mismatch`,
  `padstack`) and 80 extra (`footprint`: "net-tie pad group contains unknown pad number") are
  all caused by the skipped pads. The test tightens automatically as pads load.
* Text helpers against the reference build (`tests/oracle/build_drc23_text_oracle.sh`, driver
  `drc23_text_oracle.cpp` linking `libkicommon.so`, cases `drc23_text_cases.txt`, golden
  `tests/golden/drc23/text_oracle.txt`, replay `misc::tests::text_helpers_match_reference`):
  `ExpandEnvVarSubstitutions`, `varRefRegEx.Matches` and every `reportEach` match (start,
  length, message) of 48 inputs identical, including the `$125DRC_ERROR` pass-through quirk,
  escapes, unterminated brackets, versioned env var fallbacks and multi-line messages.
* Not verified yet: everything with pads (`footprint_type_mismatch`, pad checks, net ties,
  pads on disabled layers), assertions (DRC-21), `item_on_disabled_layer` (no corpus board has
  one), `footprint_scaled_with_pads`.

### Ported C++ tests (`tests/drc23_misc_footprint.rs`)

* `test_drc_invalid_outline_issue24078.cpp` -> `invalid_outline_bezier_slot_issue24078`
* `test_drc_invalid_outline_issue24241.cpp` -> `degenerate_arc_outline_marker_issue24241`
* `test_drc_text_var_issue24442.cpp` -> `drc_text_var_issue24442`
* `test_drc_netclass_literal.cpp` -> `drc_netclass_name_ending_in_backslash` (the misc part;
  the `EvalRules` `m_ImplicitNetclass` check needs `DrcRuleInfo::implicit_netclass`, engine)
* `test_drc_tuning_profiles.cpp` -> `drc_missing_tuning_profile` (the misc part: exactly the
  one `missing_tuning_profile` of the reference report)
* Not ported: `test_drc_component_classes.cpp`, `test_drc_orientation.cpp` (assertions: need
  DRC-21 conditions).

### Deviations

* `SetItems( m_board )` (outline "no edges" / "suspicious items") uses the nil UUID: the board
  model has no `BOARD::m_Uuid` and `ResolveItem` cannot return the board, so the report entry
  has no items where the reference lists `PCB` (a random UUID per run in C++).
* `testAssertions` runs sequentially in `forEachGeometryItem` order (C++: thread pool; the
  report order of assertion failures is scheduling dependent there).
* `ExpandEnvVarSubstitutions` reads `DrcTestProviderMisc::environment` instead of the process
  environment (empty by default); `wxIsalnum` is ASCII (the C locale of the reference CLI);
  the "missing bracket" `wxLogWarning` is not reproduced.
* The drawing sheet (`DS_DRAW_ITEM_LIST::BuildDrawItemsList`) is not ported: the host passes
  its texts as `DrawingSheetTexts`; `None` (default) skips that loop, so drawing-sheet
  `unresolved_variable` entries of the reference CLI (which sets a drawing sheet) are missing.
* `reportProgress` of the assertion waiting loop is not called (sequential run).

### Known gaps

* `BOARD::m_Uuid` (change request: kicad-pcb board UUID resolving to the board, description
  "PCB"); the drawing sheet engine; pad paths unverified until PCBIO-07 lands.

## DRC-08a: copper clearance, part 1 (track / via / pad item tests, items against zones)

### File / class map

| C++ (`drc_test_provider_copper_clearance.cpp`) | Rust (`src/providers/copper_clearance.rs`) | State |
|---|---|---|
| `DRC_TEST_PROVIDER_COPPER_CLEARANCE` (`m_drcEpsilon`, `m_zoneIndex`, `sub_e`), `Run` (phase gating on the error limits, `ZONE_INDEX_GUARD`) | `DrcTestProviderCopperClearance::new(&DrcBoardCaches)`, `run` (`DrcTestProviderRun`) | ported |
| `collectZonesNear`, `testSingleLayerItemAgainstItem` (UUID canonical order, crossing tracks, shorts, net-tie exclusions, unflashed pad / via hole shapes, `--all-track-errors`, the two-way hole test incl. clearance 0), `testItemAgainstZone` (net-tie graphics exemption, flashed / plated pads, zone hole test) | same names in snake case | ported |
| `testTrackClearances` (prepared-copper query with padding credit, UUID dedup filter, `freePadsUsageMap`), `testPadAgainstItem` (net-tie groups, same logical pad shorts, `unconnected-(` pads, the three hole directions), `testPadClearances` | same names | ported |
| `testGraphicClearances`, `testKnockoutTextAgainstZone`, `testZonesToZones`, `testTeardropClearances` | `test_graphic_clearances`, ... | ported (DRC-08b, see its section) |

The provider borrows the cache
generator's caches (`m_DRCMaxClearance`, `m_DRCCopperZonesByLayer`, copper item tree, zone
trees) and must be run with a context that has the run's connectivity (FlashLayer);
`tests/drc08a_copper_clearance.rs` `CopperWithCaches` shows the wiring.

### Verification

Reference: `kicad-cli pcb drc --format json --severity-all --units mm [--all-track-errors]`.
Goldens (`tests/oracle/gen_drc08a_corpus.py`, `gen_drc08a_boards.py`) hold one line per entry of
`clearance` / `hole_clearance` / `shorting_items` / `tracks_crossing` (entry hash of the shared
corpus format + the last 8 characters of both item UUIDs). The replay attributes each reference
entry to its phase by the types of its items (tracks / vias against anything, pads against
anything but shapes / text boxes, tracks and pads against zones = DRC-08a; the rest = DRC-08b;
entries naming a physical clearance constraint = the physical provider) and compares the DRC-08a
entries as multisets. State 2026-11-10 (pads load now):

| Golden | Boards compared | Result |
|---|---|---|
| `golden/drc08a/corpus.txt` (the 365 cached reports of `gen_drc_corpus.py`) | 306 (56 pending DRC-21 rule conditions, 2 legacy `.pro`, 1 reference failure) | clearance 2018 matched, hole_clearance 1407, shorting_items 239, tracks_crossing 0 (none in the corpus); 0 missing, 0 extra except the two error-limit boards below; 54 reference entries are DRC-08b's, 0 physical |
| `golden/drc08a/all_track_errors.txt` (12 boards incl. `issue17429`, `issue5093`, `issue24355`, `issue8003`, `reverse_via`) | 12 | clearance 3, hole_clearance 31, shorting_items 45 matched; 0 missing, 0 extra |
| `golden/drc08a/synthetic*.txt`: `tests/data/drc08a/items*` (tracks, arcs, through / blind / micro vias, vias with unused layers removed, zone fills, DRC epsilon gaps; no rule file, hole clearance 0.4 mm, hole clearance 0) | 3 + 3 | default 45 / 1 / 3, all-track-errors 48 / 13 / 6 (clearance / hole / shorting) matched; 0 missing, 0 extra |

Not compared: the report **order**. Every C++ phase runs one thread-pool task per track /
footprint, so the reference order changes between runs (three runs of `items.kicad_pcb`: three
orders, one set); the port reports in board order (`drc.md` 4.12). The same scheduling makes
the reported **set** nondeterministic once the 499-entry `clearance` limit is reached:
`issue832` and `stickhub-extra-via` (571 entries differ in each direction, all within the
limit-truncated lists) are listed, not failed. `tracks_crossing` and the `<no net>` short have
no reference entry: on the compared boards crossing tracks always belong to one connectivity
cluster, which gets one net (also on the synthetic boards, where pads are absent).

### Ported C++ tests (`tests/drc08a_copper_clearance.rs`)

| C++ | Rust |
|---|---|
| `test_drc_hole_clearance_issue24355.cpp` `HoleClearanceIssue24355` (2 hole clearance violations, `RunTests( MM, true, false )`) | `hole_clearance_issue24355` |
| `test_drc_regressions.cpp` `DRCFalsePositiveRegressions`, copper clearance part (`issue4139`, `4774`, `5978`, `6443`, `12609`, `14008`, `18203`, `net_tie_hole_clearance`, `issue24974`: no entry of the provider's types) | `false_positive_regressions_copper_clearance` (boards with DRC-21 rule conditions are skipped with a note) |

### Deviations

- Sequential: tracks in track-list order, pads in footprint / pad order; `freePadsUsageMap`
  (first track net to touch a free pad claims it) is claimed in that order (C++: first thread).
- `freePadsUsageMap` is a `BTreeMap` keyed by `ItemId` (C++: `std::map<BOARD_ITEM*, int>`,
  lookups only). The net-tie pad set of `testItemAgainstZone` is a `BTreeSet<PadId>` (C++:
  `std::set<PAD*>`, only used for "any pad collides").
- `GetNet()` pointer comparisons are net code comparisons (`None` for items that are not
  connected), which is the same since every code has one `NETINFO_ITEM`.
- A hole shape the C++ would dereference as null (an item with `HasHole()` but no hole shape)
  is skipped.

### Known gaps

- The provider is not yet wired into a full `DrcEngine::run_tests` (the cache generator
  wiring of DRC-05's known gap).
- Pad-vs-barcode pairs: attributed to DRC-08a by the replay; the C++ also tests them from the
  graphic side (DRC-08b), not present in the corpus.

## DRC-08b: copper clearance, part 2 (copper graphics, knockout text, zone-to-zone, teardrops)

### File / class map

| C++ (`drc_test_provider_copper_clearance.cpp`) | Rust (`src/providers/copper_clearance.rs`) | State |
|---|---|---|
| `testGraphicClearances` (board drawings, footprint graphics, footprint fields; the `testGraphicAgainstZone` and `testCopperGraphic` lambdas: reference images / hidden fields skipped, knockout text, copper item tree filter with footprint-shape, track, same-net and UUID dedup, no padding credit) | `test_graphic_clearances`, `test_graphic_against_zone`, `test_copper_graphic` | ported |
| `testKnockoutTextAgainstZone` (inherited net of the first colliding zone, `shorting_items` / `clearance`, `clearance >= 0`) | `test_knockout_text_against_zone` (`aInheritedNet` = `Option<InheritedNet>`: code and name of the zone net) | ported |
| `testZonesToZones` (teardrops excluded, `LAYER_RANGE( F_Cu, B_Cu )` order, `CollectOverlappingPairs` on fill `BBoxFromCaches`, same-net / rule-area skips, per-(zone, layer) segment `PACKED_RTREE<size_t>` with `A.x <= B.x` normalisation, first segment pair with `floor( sqrt( d2 ) + 0.5 ) < clearance`) | `test_zones_to_zones` | ported |
| `testTeardropClearances` | `test_teardrop_clearances` | ported |

`new_without_pending` and the phase switch of DRC-08a are removed: `DrcTestProviderCopperClearance::new( &caches )` runs all phases of `Run`.

### Verification

Replay (`tests/drc08a_copper_clearance.rs`, now covering both packages; the `origin` classifier still
separates the phases but every non-physical entry is compared):

| Golden | Boards compared | Result |
|---|---|---|
| `golden/drc08a/corpus.txt` | 332 (33 pending: 30 whose rule conditions call functions (DRC-22), 2 legacy `.pro`, 1 reference failure) | DRC-08b entries 129 (pad/shape 10, shape/shape 76, shape/text 4, graphic or text / zone 4, zone/zone 35) all matched; whole provider clearance 2067, hole_clearance 1407, shorting_items 371 matched; 0 missing, 0 extra except the two error-limit boards (`issue832`, `stickhub-extra-via`, as in DRC-08a) and the two boards of `KNOWN_RULE_GAPS` below |
| `golden/drc08b/synthetic.txt` (`tests/oracle/gen_drc08b_boards.py`, boards `tests/data/drc08b/graphics*`: copper graphics against graphics / pads / zones, footprint copper shapes, knockout board text over one / two / same-net zones and 0.1 mm off a zone, knockout reference fields visible and hidden, text boxes, zone pairs on one and two layers, overlapping / same net / no net, teardrop pairs, teardrop against zones; with and without a rule file) | 2 | 40 DRC-08b entries + 2 track entries matched (clearance 22, shorting_items 20); 0 missing, 0 extra |

The replay gate is narrower than before: boards are pending only when a user rule condition calls a
function (the function bodies are DRC-22 stubs), not for every compiled condition; this adds 26
corpus boards (e.g. the `line_ending_drc` matrix, 123 shorts). Two of them differ in DRC-08a phase
entries because their conditions resolve differently from C++ (listed in `KNOWN_RULE_GAPS`, not
failed): `issue16566` (`A.Pad_Shape == 'Oval'` does not match the oval pad: 4 missing
`oval_clearance` entries) and `issue23469` (`A.Reference == 'J1' && B.Reference == 'J1'` does not
match pad against NPTH pad of J1 for hole_clearance: 4 extra entries with the board default).

Not compared: the report order (one thread-pool task per drawing / footprint / zone pair in C++;
the port runs them in board order and candidate order).

### Ported C++ tests (`tests/drc08a_copper_clearance.rs`)

| C++ | Rust |
|---|---|
| `test_drc_copper_graphics.cpp` `DRCCopperGraphicsTest` (4) | `copper_graphics_test` |
| `test_drc_copper_graphics.cpp` `DRCCopperGraphicLineEndingMatrix` (123 / 0) | `copper_graphic_line_ending_matrix` |
| `test_drc_regressions.cpp` `DRCFalseNegativeRegressions`: `intersectingzones` (1), `fill_bad` (1), `issue5750` (4 in all, 3 of this provider's types per the reference report), `issue11814` (2; pending, its rules call functions) | `false_negative_regressions_copper_clearance` |

Not ported: `DRCCopperGraphicLineEndingAgainstTrack` (builds its board in code; the pair is found
by `testTrackClearances`, and the line-ending matrix covers the shapes) and
`DRCTeardropOverCopperField` (`issue24649` needs `KI_TEST::FillZones`; a board refilled and saved by
the reference CLI reports no short either, so the refill is not reproducible through the CLI).

### Deviations

- Teardrop-vs-teardrop pairs are tested once, from the teardrop with the smaller handle (C++:
  `zone < teardrop` on `ZONE*` addresses). The arena handle order is the allocation order, which is
  what the heap order of a freshly loaded board usually is; which teardrop becomes item 1 can differ
  from a particular C++ run (`drc.md` 4.12).
- `testZonesToZones` keeps the segments and the segment tree of a (zone, layer) together in one
  `BTreeMap` (C++: two vectors of maps); a missing entry skips the pair as in C++.

### Known gaps

- Barcodes on copper (`PCB_BARCODE_T` graphics) are ported but no compared board has one.
- `DRCTeardropOverCopperField` (needs the zone filler).

## DRC-21: PCB expression binding (`PCBEXPR_*` on kicad-libeval)

Supersedes the "stub (DRC-21)" / "needs DRC-21" notes of the DRC-20, DRC-03 and DRC-04 sections:
user rule conditions and constraint value expressions now compile and evaluate with the
default binding `PcbExprUcode`.

### File / class map

| C++ (`pcbnew/pcbexpr_evaluator.h/.cpp`, `pcbexpr_functions.cpp`) | Rust | State |
|---|---|---|
| `PCBEXPR_UCODE::CreateVarRef` (`null`, navigation chains `A.Parent...`, `L.Layer`, receivers `A` / `B` / `AB` / `L`, the "object code" fields, `_` -> space, `Origin X/Y` aliases, the class / type matrix with `VT_PARSE_ERROR`), `CreateFuncCall`, usage flags | `src/pcbexpr/evaluator.rs` `PcbExprUcode` (`UcodeBinder` + `PcbExprBinder`) | ported |
| `PCBEXPR_CONTEXT` (items, layer, constraint, type overrides, `GetEffectiveType`, `GetBoard`, `Reset`) | `PcbExprContext` (the binder's `Env`) | ported |
| `PCBEXPR_VAR_REF` (`ClassifyProperty`, `ExpressionType`, `GetObject`, `GetValue` with parent-footprint fallback, `IsAvailableFor`, footprint variant overrides, the kind matrix, "quacks like a duck" strings, layer enums), `PCBEXPR_NETCLASS_REF`, `PCBEXPR_COMPONENT_CLASS_REF`, `PCBEXPR_NETNAME_REF`, `PCBEXPR_TYPE_REF` | `PcbExprVarRef` + `PcbExprRefClass` | ported |
| `PCBEXPR_LAYER_VALUE`, `PCBEXPR_PINTYPE_VALUE`, `PCBEXPR_NETCLASS_VALUE`, `PCBEXPR_COMPONENT_CLASS_VALUE`, `PCBEXPR_NET_VALUE` (`AsString`, `EqualTo`, `NotEqualTo`) | `PcbExprValue` (`CustomValue` payload) | ported |
| `PCBEXPR_BUILTIN_FUNCTIONS` (`Instance`, `Get`, `IsGeometryDependent`, `GetSignatures`, `RegisterFunc`) | `PcbExprBuiltinFunctions` | ported |
| `RegisterAllFunctions` (34 signatures, geometry flags) | `src/pcbexpr/functions.rs` `register_all_functions` | ported; every body is the stub `not_yet_ported` (DRC-22) |
| `DRC_RULE_CONDITION::EvaluateFor` teardrop type override | `PcbExprUcode::item_env` | ported |
| implicit `A.Layer == '...'` (tuning-profile rules) | `evaluator::item_field_equals`, called from `engine/implicit_condition.rs` | ported (was `port_pending!`) |

### Verification

* Condition compiler vs the real C++ (`pcb-dump rules`, real `DRC_RULES_PARSER` +
  `PCBEXPR_UCODE`), replayed with `PcbExprUcode` in `tests/drc21_pcbexpr.rs`: all 73 corpus
  `.kicad_dru` files (`rules_corpus.txt`), all 94 broken files of DRC-20 (`rules_errors.txt`) and
  5 new codegen case files `tests/data/rules/drc21/` (53 rules: unknown items / properties /
  navigation steps / receivers, type conflicts (`A.Shape`), `null`, missing units, unknown
  functions, case-insensitive fields, usage flags; golden `drc21_conditions.txt`, 3221 lines,
  generator `tests/oracle/gen_drc21_goldens.py`): **identical** (accept / reject, every message
  text and offset, both parser modes).
* EvalRules oracle of DRC-04 (`tests/evalrules_oracle.rs`, now loading with the real binding):
  27 corpus boards with user rule conditions that call no function are compared now:
  **1244 categories match on 102 complete boards, 0 mismatches** (before: 1128 / 98; the 4
  tuning-profile categories of `issue24772/fw16_MCIO8i` that hit the implicit `A.Layer`
  stub match too). 28 boards whose rules call functions are counted as "needs DRC-22".
* Ported C++ tests `qa/tests/pcbnew/test_libeval_compiler.cpp` on parsed boards:
  `IntrospectedProperties`, `IntrospectedExtendedNumericProperties` (zone / footprint part),
  `RenamedProperties`, `ExpressionPropertyTypesAreSupported`, `IntrospectedPropertyAvailability`,
  `ParentNavigation`, `PropertyParentFallback`, `BoolPropertyEvaluation`,
  `VariantAwareEvaluation`, `ReceiverValidation`, `LayerReceiverLayerField`,
  `PairItemDependency` (the function-calling expressions of these cases are compile-checked
  only). `SingleItemRulePreflight`, `PairConditionsInSingleItemRules` are in DRC-20's
  `test_rule_parser.rs`. Not ported (need DRC-22 function bodies): `InNetChainClassWildcard`,
  `StackedMicroviaExpression`, `DynamicCourtyardArgument`, `FunctionArgumentValidation`;
  `FieldAliasNormalization` is a GUI dialog helper (skipped). Plus `value_classes` (net /
  composite netclass / layer / pin type / fallback matrix) and `rule_condition_evaluate_for`.
  The kicad-libeval copies stay `#[ignore]`d there (that crate cannot depend on kicad-drc).

### Deviations

- Layer patterns (`PCBEXPR_LAYER_VALUE::EqualTo`) match against `board_layer_choices( board )`
  (canonical + board names = the `ENUM_MAP` after `BOARD_LOADER::Load`, the kicad-cli path),
  recomputed per comparison: `BOARD::m_LayerExpressionCache` is crate-private in kicad-pcb (same
  result, slower). The QA load path of C++ has only canonical names in the map; this differs
  only for a pattern that matches a user layer name but no canonical name. Without a board
  (C++ null dereference) the canonical names are used.
- Function bodies (DRC-22): each registered function is a stub that pops its arguments, pushes
  0 and, during a real evaluation (receiver resolves to an item), reports "Function '<name>' is
  not yet ported (DRC-22)." through the error callback. The pre-flight call reports nothing, so
  the C++ argument validation errors at compile time ("Missing ... argument", unknown layer
  names) are missing until DRC-22. Without a reporter (DRC hot path) such conditions are
  silently false.
- C++ undefined behaviour made defined: `L` as a function receiver reads `m_items[2]` (out of
  bounds) -> no item; a null `GetComponentClass()` compares as the empty name; a null board in
  `GetLayerID` -> `UNDEFINED_LAYER`; `A.Parent` of a board-level item is the BOARD in C++
  (`TYPE_REF` would print `ENUM_MAP<KICAD_T>` of `PCB_T`), in Rust no item (undefined value).
- `GetAllClasses()` is iterated in `ClassId` order (C++ `unordered_map`): the resulting type and
  class set of a valid reference are order independent; only a reference that already failed
  with `VT_PARSE_ERROR` keeps an order-dependent partial class set (never evaluated by the engine,
  which rejects the rule).

### Known gaps

- DRC-22: function bodies. The shared DRC harness `tests/drc_harness` still loads with the
  stand-in `SyntaxBinder` and skips boards with user rule conditions; switching it to the real
  binding (skipping only function-calling rules, as `evalrules_oracle.rs::has_compiled_conditions`
  now does) brings 27 more boards into every provider's comparison.
- `value_classes` skips its pad cases when the board reader cannot load pads (they run since
  PCBIO-07 landed).

## FIX-DRC-1: rule identity behind `DRC_CONSTRAINT::GetParentRule()` (review finding R-01)

### File / class map

| C++ | Rust | State |
|---|---|---|
| `DRC_RULE*` identity (`m_parentRule`, `m_minRule`, `m_optRule`, `m_maxRule`) | `kicad_pcb::drc_api::DrcRuleInfo::rule_index` (index in `DRC_ENGINE::m_rules`, `None` outside an engine; part of `PartialEq`) | ported |
| `compileRules`: `DRC_ENGINE_CONSTRAINT::parentRule` / `constraint` copy | `engine/mod.rs` `compile_rules`: `rule_info` and the four rule slots of `constraint` carry the index (`DrcRule::info_in_engine`) | ported |
| `m_netclassClearances` fast path of `EvalRules` | `engine/eval_rules.rs`: same index as the general path (`netclass_clearance_rule_indexed`) | ported |
| dereferencing the pointer (`matchingRule->m_Name`, ...) | `DrcEngine::rule( index )` | ported |

### Verification

- The engine and EvalRules goldens (`engine_corpus.txt`, `evalrules_corpus.txt`,
  `evalrules_review.txt`) do not print the index and replay unchanged.
- `evalrules_oracle.rs::same_named_rules_have_distinct_identity`: on `rev_explicit` the two
  `"dup name"` rules give a via on F.Cu min 0.45 mm from the second and max 0.7 mm from the
  first (the reference values of `evalrules_review.txt`), with equal names and severities but
  different `rule_index`; every compiled constraint's index names its own `parentRule`.
- `fast_path_equals_general_path` also compares the indices of the four rule slots.

### Deviations

- After a failed `InitEngine` the constraints left over from the first pass keep the indices
  of the discarded rule list (C++: their `shared_ptr` keeps the discarded rules alive), so such
  an index may name a different rule of the current list (documented on `compile_rules`).
- Consumers that group by `DRC_RULE*` in a `std::map` (address order, unspecified) group by
  `rule_index` in a `BTreeMap` instead: rule creation order, the deterministic order of
  `drc.md` 4.12. Such reports are compared as sets.

### Known gaps

- Work item 3 (matched length `itemSets` / `matches`, `LENGTH_DELAY_STATS::matchingRule` in
  `drc_length_report.h`) is not done: `providers/matched_length.rs` and `length_report.rs` are
  still placeholders; their package must group by `rule_index` in a `BTreeMap` and verify a
  board with two same-named length rules against the reference `kicad-cli pcb drc` (set
  comparison). The diff-pair coupling provider (`gapRule` / `uncoupledRule`) only needs the
  info, not grouping.

## DRC-29: copper sliver checker and connection width

### File / class map

| C++ | Rust | State |
|---|---|---|
| `drc_test_provider_sliver_checker.cpp` `DRC_TEST_PROVIDER_SLIVER_CHECKER::Run`, `layerDesc`, static `addItemPolysWithEndings` | `providers/sliver_checker.rs` `DrcTestProviderSliverChecker` (`Default`; `DrcTestProviderRun`), `add_item_polys_with_endings` (`pub(crate)`, shared with connection width) | ported |
| `ADVANCED_CFG::m_SliverWidthTolerance` / `m_SliverMinimumLength` / `m_SliverAngleTolerance` | `sliver_checker::ADVANCED_CFG_SLIVER_*` (defaults 0.08 mm, 0.0008 mm, 20 deg) | constants (no advanced config file) |
| `drc_test_provider_connection_width.cpp` `DRC_TEST_PROVIDER_CONNECTION_WIDTH::Run` (`build_netlayer_polys`, `min_checker`), `layerDesc` | `providers/connection_width.rs` `DrcTestProviderConnectionWidth::new(&DrcBoardCaches)` | ported |
| `POLYGON_TEST` (`FindPairs`, `GetVertices`, `isSubstantial`, `getKink`) over `VERTEX_SET` | `connection_width::PolygonTest` on `kicad_geom::geometry::vertex_set::VertexSet` | ported |
| `ADVANCED_CFG::m_ExtraClearance` | `connection_width::ADVANCED_CFG_EXTRA_CLEARANCE` (0.0005 mm) | constant |
| `NETCODE_LAYER_CACHE_KEY` + `std::hash` | `BTreeMap<(net code, PcbLayerId), _>` key | replaced (order, below) |

The connection width provider reads `m_DRCCopperZones`, `m_CopperItemRTreeCache` and
`m_CopperZoneRTreeCache` from the run's `DrcBoardCaches` (constructor argument, like copper
clearance); the run context must carry the connectivity (`FlashLayer`). The sliver checker
needs no caches.

### Verification

* Item-level oracle (new): `tests/oracle/drc29_oracle.cpp` (built by `build_drc29_oracle.sh`
  like cn-order-oracle, under the C++ lock; binary `/home/user/kicad-oracle/bin/drc29-oracle`)
  loads each board like `KI_TEST::LoadBoard`, ignores every code but `COPPER_SLIVER` and
  `CONNECTION_WIDTH`, runs the real `DRC_ENGINE::RunTests( MM, true, false )` and prints every
  violation: message, marker position, layer, item uuids and marker path (slivers in handler
  order, widths sorted). `tests/oracle/gen_drc29_oracle.sh` ran it on a copy of all 368 corpus
  boards (one process per board) -> `tests/golden/drc29/oracle.txt`. Replay
  `sliver_and_connection_width_match_item_oracle`: 309 boards loaded completely and compared,
  all identical (20 violations: 2 slivers, 18 connection width, incl. positions, paths and
  sliver order); 59 pending (DRC-21 rule conditions on 56, among them `issue16182` with 4
  slivers, `connection_width_rules` and `fw16_MCIO8i`; 2 legacy `.pro`; `odb_circle_edge_cases`
  panics in kicad-geom, below).
* CLI corpus: `tests/golden/drc29/corpus.txt` (`gen_drc29_golden.py` from the cached reports of
  `gen_drc_corpus.py`), replay `sliver_and_connection_width_match_reference_corpus`: same 309
  boards, `connection_width` 18 / 18, `copper_sliver` 2 / 2, 0 missing, 0 extra, sliver order
  identical. Connection width entries are compared as a multiset with sorted items.

### Ported C++ tests (`tests/drc29_sliver_connection_width.rs`)

* `test_drc_copper_sliver.cpp` `DRCCopperSliver` -> `drc_copper_sliver` (all 5 boards load)
* `test_drc_copper_conn.cpp` `DRCCopperConn` -> `drc_copper_conn` (`connection_width_rules`
  pending on DRC-21)
* Both C++ tests call `KI_TEST::FillZones` first. kicad-zone (ZF-03 pending) depends on
  kicad-drc, so the ports run on the stored fills and expect the reference build's counts on
  those (oracle golden): identical to the C++ expectations except `issue9870` (13 stored,
  12 after a refill).

### Deviations

* Sequential: the sliver layer polys are built in layer order (C++: thread pool; only the
  build is parallel, the report loop is sequential, so the order is the C++ one).
* Connection width order (`drc.md` 4.12): C++ iterates an `unordered_map` of (net, layer)
  entries on the thread pool, holds the items of one entry in a `std::set<BOARD_ITEM*>`
  (address order: polygon append order), and collects the items at a neck from
  `DRC_RTREE::GetObjectsAt` (hash order) and `m_CopperZoneRTreeCache` (hash order). The port:
  (net code, layer) key order, items in collection order (copper zones, tracks, pads), tree
  search order then `ZoneId` order. Report order and item order within a violation (which
  also decides which two items `EvalRules` sees) are not reproducible in C++; the set of
  violations matched everywhere.
* `calc_effort` / progress reporting is not ported (progress only). It calls
  `GetFilledPolysList( layer )`, which throws in C++ for a zone layer without a fill entry.
* `POLYGON_TEST::FindPairs` on an outline that yields no vertex returns false (C++:
  `m_vertices.front()` of an empty deque, undefined).

### Known gaps

* Boards with user rule conditions wait for DRC-21 (`issue16182`, `connection_width_rules`).
* `kicad_geom` `ShapePolySet::is_exterior_waist` overflows (`s + e` of huge coordinates, C++
  wraps) inside `Simplify` on `issue25089/odb_circle_edge_cases` (change request filed).
* Refill-based expectations of the C++ tests need the zone filler (ZF-03) in a crate that can
  depend on both.

## CC-1: component class assignment rules (compiler and matcher hooks)

| C++ | Rust | State |
|---|---|---|
| `COMPONENT_CLASS_MANAGER::CompileAssignmentRule` (the `DRC_RULES_PARSER` part: `ParseComponentClassAssignmentRules`, `WX_STRING_REPORTER`, `PARSE_ERROR`, exactly one rule) | `component_class_rules::compile_assignment_rule_source`, `DrcComponentClassRuleCompiler` (implements `kicad_pcb::component_class::ComponentClassRuleCompiler`) | ported |
| `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches` (`EvaluateFor( fp, nullptr, 0, fp->GetSide(), nullptr )`) | `impl ComponentClassRuleCondition for DrcRuleCondition` | ported |
| (load sequence) | `component_class_rules::install( board )`: puts the compiler on the board's manager | new |

Verification: through kicad-session `tests/component_classes.rs` (reference pcb-dump effective
classes of `component_classes` / `component_classes_drc`, QA and CLI paths) and the port of
`test_component_classes.cpp` `ComponentClasses`. Rules using DRC-22 functions (`getField`,
`memberOfSheet`, `intersectsArea`) do not match until DRC-22 lands (those footprints are in
ignored tests). `test_drc_component_classes.cpp` (a DRC run) is not ported here.

## DRC-09: hole-to-hole and board edge clearance

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_test_provider_hole_to_hole.cpp` `DRC_TEST_PROVIDER_HOLE_TO_HOLE` (`Run`, `testHoleAgainstHole`) | `src/providers/hole_to_hole.rs` `DrcTestProviderHoleToHole` | ported |
| `pcbnew/drc/drc_test_provider_edge_clearance.cpp` `DRC_TEST_PROVIDER_EDGE_CLEARANCE` (`Run`, `resolveSilkDisposition`, `testAgainstEdge`, `testCopperOutsideBoard`, `SILK_DISPOSITION`, `WORK_UNIT`, `COPPER_ITEM_UNITS`) | `src/providers/edge_clearance.rs` `DrcTestProviderEdgeClearance` | ported |
| `m_edgesTree` (a `DRC_RTREE` of edge *copies*) | `edge_clearance.rs` `EdgeTree`: the `Insert` / `Build` / `QueryColliding` (no filter) / `GetObjectsAt` paths of a `PCB_SHAPE`, over the same `PackedRTree`, parent identity = index of the copy | ported (see Deviations) |

Neither provider needs the cache generator's caches. Both need the run's connectivity in the
`BoardCtx` (pad / via `FlashLayer`), as the CLI.

### Verification

Corpus replay `tests/drc09_hole_edge.rs` (`hole_to_hole_and_edge_clearance_match_reference_corpus`)
against the shared reference corpus (`golden/drc_corpus.txt` + the cached raw reports under
`rust/target/drc-corpus/`, `kicad-cli pcb drc --format json --severity-all --units mm`), the
board loaded as the CLI (`BuildConnectivity`, `prepare_board`, connectivity rebuild), both
providers in registry order (edge clearance, then hole to hole). Entry identity: type,
severity, description (incl. the rule detail), item UUIDs / descriptions / positions.
`hole_to_hole` / `holes_co_located` (sequential in C++) are compared in report order; the
three edge codes as multisets (thread pool in C++). State of 2026-11-10: 332 boards compared,
33 pending (30 with rule conditions that call functions: DRC-22; 2 legacy `.pro` projects; 1
failed reference run).

| Type | matched | missing | extra |
|---|---|---|---|
| `hole_to_hole` | 3 | 0 | 0 |
| `holes_co_located` | 28 | 0 | 0 |
| `copper_edge_clearance` | 274 | 0 | 2 (`issue23469`, known rule gap) |
| `silk_edge_clearance` | 171 | 0 | 0 |
| `copper_outside_board` | 216 | 0 | 0 |

The two extras of `issue23469` are the rule-evaluation gap already recorded by DRC-08a:
`A.Reference == 'J1'` does not match the pads of J1, so the board setup edge clearance (0.5 mm)
is applied instead of the 0.2 mm rule (`KNOWN_RULE_GAPS` of the test; remove when fixed).
The corpus has no `--all-track-errors` reference for these codes (the `reportAllTrackErrors`
continuation of tracks along several edges is ported but not compared).

### Ported C++ tests (`tests/drc09_hole_edge.rs`)

| C++ | Rust |
|---|---|
| `test_drc_hole_to_hole_npth_slot_issue24541.cpp` `HoleToHoleNpthSlotIssue24541` | `hole_to_hole_npth_slot_issue24541` (passes) |
| `test_drc_arc_arc_edge_clearance.cpp` `DRCArcArcEdgeClearance_InternalContainment` | `drc_arc_arc_edge_clearance_internal_containment` (passes) |
| `test_drc_copper_outside_board_issue14650.cpp` `CopperOutsideBoardIssue14650` | `copper_outside_board_issue14650` (passes) |
| `test_drc_regressions.cpp` `DRCHoleToHoleReportsRuleValue` (`issue22267`) | `drc_hole_to_hole_reports_rule_value` (passes) |
| `test_drc_regressions.cpp` `DRCFalsePositiveRegressions`, hole / edge part (`issue5990`, `issue18839` and the other loadable boards) | `false_positive_regressions_hole_edge`: no error-severity entry of the five codes (passes) |
| `test_drc_regressions.cpp` `DRCFalseNegativeRegressions` `issue18142` (1, the `hole_to_hole` between a blind/buried via and a microvia) | `false_negative_regression_issue18142` with the two codes enabled (passes) |

Not ported: `test_drc_npth_slot_clearance_issue24799.cpp` (needs a zone refill), and
`test_drc_issue23469.cpp` (rule-evaluation gap above).

### Deviations

- **Edge tree**: C++ inserts heap copies of the edge shapes (same UUID) into a `DRC_RTREE`;
  `crate::rtree::DrcRtree` keys entries by board handles, which `detached_copy()` values do not
  have, so the module keeps its own copy-indexed tree (`EdgeTree`, same packed r-tree, same
  entry boxes and visit order). Rule evaluation and marker geometry use the copy
  (`ctx.view(AnyItem::Shape(&copy))`), as the C++ does.
- **Thread pool**: work units and copper-outside-board items run in index order. In C++ the
  report order of all three edge codes, the work unit that wins the
  `m_emittedEdgeReports` de-duplication of a multi-layer pad / via (only the marker layer
  differs) and the set at an exceeded error limit depend on scheduling.
- **`GetObjectsAt`** (nearest edge of a silk item crossing the outline): the first copy in
  tree search order that passes `HitTest` (C++: `std::unordered_set` hash order; matters only
  when several edges pass at the nearest outline point).
- `m_silkDisposition` (`std::map<BOARD_ITEM*>`) and `checkedPairs` (pointer pairs) are lookup
  only; keyed by `ItemId` / canonical `ItemId` pair.

### Known gaps

- Boards whose rule conditions call functions wait for DRC-22 (30 corpus boards, among them
  `severities`, `issue24211`, `issue2512`).
- `issue23469` rule-evaluation gap (above).

## DRC-10: disallow provider (keepouts, text on Edge.Cuts) and the implicit keepout condition

### File / class map

| C++ | Rust | State |
|---|---|---|
| `drc_test_provider_disallow.cpp` `DRC_TEST_PROVIDER_DISALLOW` (`Run`, `checkTextOnEdgeCuts`, `processItem`, `reportDisallow`) | `src/providers/disallow.rs` (`DrcTestProviderDisallow::new(&DrcBoardCaches)`) | ported (sequential; pair pre-computation omitted, see Deviations) |
| implicit keepout rule condition `A.intersectsKeepout('<uuid>')` | `ImplicitAtom::IntersectsKeepout` in `src/engine/implicit_condition.rs` -> `pcbexpr::functions::intersects_keepout` (the `doIntersectsAreaFunc` / `collidesWithArea` / `searchAreas` port of DRC-22, one implementation for the compiled and the native form; the context layer is now passed to the native atoms) | ported |

The provider needs `DrcBoardCaches::max_physical_clearance` (`board->m_DRCMaxPhysicalClearance`,
the search distance of the no-tracks keepout tree query) and a `BoardCtx` with the connectivity
(pad / via `FlashLayer`). `tests/drc10_disallow.rs` (`DisallowWithCaches`) shows the wiring.

### Verification

- `tests/drc10_disallow.rs` `disallow_matches_reference_corpus` (`DRC10_FILTER=<substring>`):
  362 corpus boards compared against the reference kicad-cli JSON reports (3 pending: two
  legacy `.pro` projects, `plugins/fakeboard` whose reference run fails).
  `items_not_allowed`: 66 matched, 0 missing, 0 extra; `text_on_edge_cuts`: 6 matched, 0
  missing, 0 extra (boards with violations: `component_classes`, `issue22475` (42),
  `issue22826`, `issue24772/fw16_MCIO8i`, `issue6945`, `keepout_disallow`,
  `pns_regressions/ultrasound`, `stitch_fp_rulearea`, `issue24525`, `issue7241`). Compared as
  multisets of (code, description, items with UUID / description / position): the C++ runs
  the item loop on the thread pool, so its report order is scheduling dependent.
  User rule conditions are compiled with the real expression binding (the harness
  `load_board` compiles with the syntax-only binder; the test re-runs `InitEngine`).
- The implicit keepout condition is also covered by `evalrules_oracle.rs`: with the hook in
  place no keepout category is pending any more; all 360 `disallow` categories match the C++
  `EvalRules` dump (the one remaining mismatch is the known layer-map deviation of
  `issue24735`).
- The corpus result is the same with and without the board expression caches installed
  (`PcbExprCacheScope`).

### Ported C++ tests (`tests/drc10_disallow.rs`)

| C++ | Rust |
|---|---|
| `test_drc_keepout_disallow.cpp` `DRCKeepoutDisallowViasAndTracks` (2 vias, 1 track, 3 total) | `drc_keepout_disallow_vias_and_tracks` (passes) |
| `test_drc_keepout_disallow.cpp` `DRCKeepoutFootprintExcludesItself` (issue 24924) | `drc_keepout_footprint_excludes_itself` (passes) |
| `test_drc_keepout_disallow.cpp` `DRCKeepoutNoClearanceInflation` | `drc_keepout_no_clearance_inflation` (passes) |
| `test_drc_issue24525.cpp` `DRCIssue24525_TableAndBarcodeOnEdgeCuts` (text 1, dimension 1, table 2, barcode 1, reference image 0) | `drc_issue24525_table_and_barcode_on_edge_cuts` (passes) |
| `test_drc_regressions.cpp` `DRCFalsePositiveRegressions` `issue7567` (disallow part) | `false_positive_regression_issue7567`: no error-severity violation of the two codes (passes) |

### Deviations

- **Keepout / copper zone pre-computation not ported**: the first C++ loop stores
  `m_IntersectsKeepoutCache[ (keepout, zone, UNDEFINED_LAYER) ]` for every (no-zone-fills
  keepout, copper zone) pair; `doIntersectsAreaFunc` reads that cache per concrete layer only,
  so these entries are never read (`drc.md` 4.10.2 allows recording the omission). No
  observable effect.
- **Thread pool**: the item loop runs in `forEachGeometryItem` order. C++ report order (and
  which violations survive an exceeded error limit, 199) is scheduling dependent.
- **`HOLE_PROXY`**: instead of setting / clearing the flag on the shared item, the second
  evaluation uses `ItemRef::as_hole_proxy()`; the proxy reference has the same handle, so the
  expression caches (keyed by item handle, as the C++ ones by pointer) are shared between the
  two evaluations as in C++.
- Progress is reported per item (`reportProgress( i, count, 1 )`, return value ignored as the
  C++ wait loop does); no progress for the omitted pre-computation.

### Known gaps

- The expression caches (`PcbExprCacheScope`) are installed by the test wrapper only; no
  engine run path enters a scope yet, so production runs evaluate keepout / area predicates
  uncached. Results are the same on the whole corpus, but the C++ result-cache aliasing
  between the plain and the hole-proxy evaluation of an item (same `(item, selector, layer,
  constraint)` key) is only reproduced inside a scope (see change requests of DRC-10).

## DRC-22: PCB expression built-in functions (`pcbexpr_functions.cpp`)

Supersedes the "stub (DRC-22)" notes of the DRC-21 section: every registered function has its
C++ body; the `not_yet_ported` stub is gone.

### File / class map

| C++ (`pcbnew/pcbexpr_functions.cpp`) | Rust (`src/pcbexpr/functions.rs`) | State |
|---|---|---|
| `existsOnLayerFunc` (interpreted version with "Unrecognized layer", compiled version with `m_LayerExpressionCache`), `isPlatedFunc` | `exists_on_layer_func`, `is_plated_func` | ported |
| `collidesWithCourtyard`, `testFootprintSelector` (`${CLASS:...}`, reference / LIB_ID wildcards), `searchFootprintsNearItem` (`GetFootprintCourtyardIndex`), `intersects{,Front,Back}CourtyardFunc` (+ `inside*Courtyard`) | `collides_with_courtyard`, `test_footprint_selector`, `search_footprints_near_item`, `intersects_courtyard_impl` | ported |
| `getDeflatedZoneOutline`, `collidesWithArea`, `searchAreas`, `SCOPED_LAYERSET`, `doIntersectsAreaFunc`, `intersectsAreaFunc`, `intersectsKeepoutFunc` (+ `insideArea`), `enclosedByAreaFunc` | `get_deflated_zone_outline`, `collides_with_area`, `search_areas`, `intersects_area_eval` / `do_intersects_area_func`, `enclosed_by_area_func`; native form `intersects_keepout` (used by the implicit keepout atom, DRC-10) | ported |
| `memberOfGroupFunc` (+ `memberOf`), `memberOfSheetFunc`, `memberOfSheetOrChildrenFunc`, `memberOfFootprintFunc` | same names | ported |
| `isMicroVia`, `isBlindVia`, `isBuriedVia`, `isBlindBuriedViaFunc`, `isStackedViaFunc` (`m_StackedMicroviaCache`) | same names | ported |
| `isCoupledDiffPairFunc`, `inDiffPairFunc`, `inNetChainFunc`, `hasNetChainFunc`, `inNetChainClassFunc` | same names | ported |
| `getFieldFunc` (variants, `m_ItemFieldCache`), `hasNetclassFunc`, `hasExactNetclassFunc`, `hasComponentClassFunc`, `customPropertyFunc`, `hasCustomPropertyFunc` | same names | ported |
| `fromToFunc` | `from_to_func` | partial: `FROM_TO_CACHE` (CN-05) is not ported, see Deviations |
| `RegisterAllFunctions` | `register_all_functions` | ported (34 signatures) |
| `BOARD` run-time caches of these functions (`m_Intersects*Cache`, `m_*ResultCache`, `m_EnclosedByAreaCache`, `m_ItemFieldCache`, `m_LayerExpressionCache`, `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`, `m_StackedMicroviaCache`) | `PcbExprBoardCaches` (wraps `DrcPredicateCaches` of DRC-05), installed per thread and board by `PcbExprCacheScope::enter( &board )`, emptied when `board.time_stamp()` moved | ported (owner: see Known gaps) |

### Verification

* Compile-time argument validation against the real C++ (`pcb-dump rules`): new codegen case
  file `tests/data/rules/drc21/arguments.kicad_dru` (42 rules: empty / missing / extra
  arguments of every function, "Malformed expression" from popping an empty stack, the
  `fromTo` argument order, unknown properties / functions inside arguments), golden
  `drc21_conditions.txt` regenerated with `gen_drc21_goldens.py`; replayed by
  `drc21_pcbexpr.rs::codegen_cases_compile_as_cxx`: **identical** (36 messages, texts and
  offsets).
* EvalRules oracle of DRC-04 (`tests/evalrules_oracle.rs`): the "needs DRC-22" skip is gone;
  the 28 corpus boards whose rules call functions are compared (rules use `insideArea`,
  `insideCourtyard`, `isPlated`, `existsOnLayer`, `memberOfFootprint`, `hasNetclass`,
  `hasComponentClass`, `isMicroVia`, `inDiffPair`, `isCoupledDiffPair`, `intersectsArea`, ...).
  Result (with the DRC-10 keepout hook): **2378 categories match (349 boards completely; all
  360 `disallow` categories), 17 pending (parser / legacy project), 1 mismatch** - `issue24735`
  `drawing`: the rule `A.Layer == 'F.Silkscreen'` matches a board layer name, which the QA
  load path's `ENUM_MAP` does not hold (the DRC-21 layer-map deviation, not a function body);
  `pcb-dump board --loader` (kicad-cli path) gives exactly the Rust values. It is listed as a
  known deviation in the test. The replay now triangulates the board zone fills after
  `InitEngine` as `KI_TEST::LoadBoard`'s `BuildConnectivity()` does (`CacheTriangulation`):
  without it the keepout test of a filled zone (`collidesWithArea`, DRC r-tree of fill
  triangles) found no triangles (`issue11814`, 4 zone categories).
  No rule file of the corpus calls a function that is left unsupported (`fromTo` is not used).
* Ported C++ tests (`qa/tests/pcbnew/test_libeval_compiler.cpp`), in `tests/drc22_functions.rs`:
  `InNetChainClassWildcard`, `StackedMicroviaExpression` (the landing via is moved under the
  lone microvia instead of added; the time stamp bump drops the cached relation),
  `DynamicCourtyardArgument`, `FunctionArgumentValidation`; and in `tests/drc21_pcbexpr.rs` the
  function expressions of `IntrospectedProperties`, `ParentNavigation` and
  `VariantAwareEvaluation` are evaluated now (they were compile-only). Plus behaviour tests
  (`courtyard_selectors`, `rule_area_predicates`, `item_predicates`) run with and without the
  board caches.
* Not verifiable here: the DRC regression boards of drc.md section 10 (`issue7567`,
  `issue16566`, `issue6879`, `issue6945`, `issue7325`, `issue2512`, `test_custom_rule_severities`,
  `test_drc_via_dangling`, `test_drc_multi_netclasses`, `test_drc_component_classes`, ...) are
  full `RunTests` cases whose counts depend on the providers; they belong to the provider /
  drc-job packages (their rule conditions now evaluate; the EvalRules part of these boards is
  covered above).

### Deviations

- **Cache owner**: the C++ caches are `BOARD` members; here they live in
  `PcbExprBoardCaches`, installed for one board on the current thread by a
  `PcbExprCacheScope` guard and dropped when the board's time stamp moved (=
  `IncrementTimeStamp`). Without a scope every evaluation is computed afresh. That differs
  from C++ only where the caches are semantically visible: (1) the courtyard memo
  `{ footprint, item }` ignores the layer the item shape was taken on (a pad with different
  per-layer shapes), (2) the per-area memo `{ area, item, layer }` ignores the constraint
  (silk clearance widens the area's layer set), (3) a memo hit does not repeat run-time error
  reports, (4) the hole-proxy evaluation of the disallow provider shares the plain one's
  result-cache entry. Inside a scope all four are reproduced.
- Items without a handle (values outside the board) are never memoised (C++ keys them by
  pointer).
- `SCOPED_LAYERSET`: the C++ temporarily calls `ZONE::SetLayerSet` on the area, which (when a
  silk-clearance evaluation adds a layer) un-fills the area zone and drops its layer
  properties, and the restore does it again. The port passes the widened layer set to
  `collidesWithArea` and leaves the immutable board untouched (only filled non-copper zones
  that are also selected as rule areas would observe the C++ side effect).
- `collidesWithArea` keepout branch: `board->GetCopperZoneRTree( zone )` is not reachable from
  rule evaluation; the fallback branch builds the same single-zone tree for the layer (same
  yes / no answer).
- Layer map of `existsOnLayer`: the kicad-cli `ENUM_MAP` (canonical + board names), as
  `PCBEXPR_LAYER_VALUE` of DRC-21.
- `fromTo()`: `FROM_TO_CACHE` (connectivity package CN-05) is not ported; an evaluation takes the
  C++ branch for a non-existent cache (result 0) and reports "Attempting to call fromTo() with
  non-existent from-to cache." through the error callback (C++: `wxLogWarning`). The argument
  checks are ported.
- `collidesWithCourtyard`'s `aItemShape` is never assigned by any C++ caller; kept as an
  always-empty parameter.

### Known gaps

- No engine run path installs a `PcbExprCacheScope` yet (the tests and the DRC-10 test wrapper
  do): `DrcEngine::init_engine` / the run driver should enter one for the board (change
  request).
- `fromTo()` needs CN-05.
- The shared DRC harness `tests/drc_harness` still compiles with the syntax-only binder
  (DRC-21 known gap); with the function bodies in place it can switch to `PcbExprUcode`.

## DRC-12: the DRC run flow (`RunTests` driver, the `pcb drc` job)

### File / class map

| C++ | Rust | State |
|---|---|---|
| `DRC_ENGINE::RunTests` with `DRC_CACHE_GENERATOR::Run()` and every registry provider in `PROVIDER_ORDER` | `src/run_flow.rs` `run_tests` | ported; providers created per run with the generator's `DrcBoardCaches`; `PcbExprCacheScope` entered for the provider loop (closes the DRC-22 gap) |
| `board->GetConnectivity()` in the cache generator (load-time connectivity for shapes, then `ClearRatsnest` / `Build` / `FillIsolatedIslandsMap`) | `run_flow::GeneratorConnectivity` (implements `CacheGeneratorConnectivity`) | ported; the recorded net propagation of `Build` is applied right after the generator |
| `ForceComponentClassRecalculation`, `GetFromToCache()->Rebuild` | - | not needed / not ported (see Deviations) |
| DRC half of `PCBNEW_JOBS_HANDLER::JobExportDrc` (violation handler -> `PCB_MARKER`, `RecordDRCExclusions`, `DeleteMARKERs( true, true )`, `RunTests`, `commit.Push`, `ResolveDRCExclusions( false )`, the three `DRC_ITEMS_PROVIDER`s, `WriteTextReport` / `WriteJsonReport`) | `run_flow::run_drc_job` | ported; the rest of the job (messages, exit codes, rule error exit 3, parity, refill, save) is `kicad-session` `run_drc` |
| unported registry slots (creepage, library_parity, schematic_parity, net chain tuning profiles, via_stitch_overlap, length, diff_pair_coupling) | `run_flow::UNPORTED_PROVIDERS`, `unported_check_would_run`, `CheckNotRun` | reported, never run (see "Checks not run") |

### Checks not run

An unported registry slot is skipped. It is reported as a `CheckNotRun` unless a sound guard
shows it cannot report anything on this board: every code of the provider ignored (error
limit 0); or the first statements of the C++ `Run()` return early (creepage: no `creepage`
rule; library parity: no project or no footprint; schematic parity: `aTestFootprints` false;
net chain tuning profiles: no net with a net chain; via stitch: no `via_stitch` generator);
or no item can match a constraint (length: no length / skew / via count / net chain rule;
diff pair coupling: no diff pair constraint or no diff-pair net). The reports then carry the
list: text report section `** Checks not run (not ported yet) **` (one `    - <provider>:
<reason>` line each) before `** End of Report **`; JSON key `checks_not_run`
(`[{"provider", "reason"}]`). Without such checks the reports are byte for byte the C++ ones.
kicad-session also prints one `DRC check '<provider>' was not run: <reason>` line each.
In practice library parity is reported on every board with footprints (the reference emits
`lib_footprint_issues` for them, drc.md 4.10.25) and diff pair coupling on boards with
diff-pair nets.

### Deviations

- Marker UUIDs: random KIIDs in C++; here drawn from a generator seeded by the host
  (`DrcJobOptions::marker_uuid_seed`), redrawn while the UUID names a board item or an earlier
  marker (a collision would hijack the report's item resolution). Markers never appear in a
  report by UUID.
- `ForceComponentClassRecalculation` is not called: the Rust footprint component class is a
  ticker-keyed cache recomputed from its inputs on use, and nothing between load and run
  changes a footprint (change request to kicad-pcb for a `Board` hook if that ever changes).
- The from-to cache is not rebuilt (CN-05 not ported; `fromTo()` reports its missing cache).
- The zone connections provider needs `&mut ConnectivityData`; its item shapes see a second
  build of the same connectivity (built only when the board has copper zones).
- Providers run sequentially in registry order; report order of the thread-pool providers
  is the index order (drc.md 4.12) and is compared order-insensitively.

### Verification

`tests/oracle/drc12_cli_compare.py` (`ref`: reference `kicad-cli pcb drc --severity-all --units
mm` in text and JSON on a copy of each of the 365 corpus boards of `drc_corpus.txt`, outputs
cached under `rust/target/drc12-cli/`; `cmp`: the same with `kicad-rs`, comparing the text
report, the JSON report, stdout and the exit code; `golden`: `tests/golden/drc12_cli.txt`).
Normalisation: the date line only for the exact comparison; for the filtered comparison the
entries of the unported providers' codes (`UNPORTED_PROVIDERS`) and the not-run section are
removed (counts recomputed, stdout counts checked against each side's own report), and three
documented reference effects are neutralised: the item line of the board itself (`PCB`: a
random UUID in C++, the port's board has no UUID yet, DRC-23 gap), the item order of
`connection_width` (C++ `std::set` of pointers), types at the error limit (199 / 499:
count only, drc.md 9.4 (3)); JSON item UUIDs absent from the board file are compared as
generated (drc.md 9.4 (4)). Rust replay without the oracle: `kicad-session/tests/
drc12_corpus.rs` (FNV-1a of the filtered text report, in order and with sorted sections).

Result (state of 2026-11-10, 364 boards compared; `plugins/fakeboard` fails in the reference):

| Outcome | Boards |
|---|---|
| byte for byte equal (text, JSON, stdout, exit code) | 47 |
| equal after filtering the unported codes, in report order | 254 |
| equal as sets (order differs: reference thread pool) | 51 |
| different | 12 |

Every difference has a known cause outside this package (`KNOWN_DIFFERENCES` in the replay):
7 boards `missing_tuning_profile` extra (18 entries): `TUNING_PROFILES` is not in the
session's project model (SET-11), so the misc provider and the implicit rules see no
profiles; 3 boards `unresolved_variable` of drawing-sheet texts missing (8): `SetDrawingSheet`
is not wired (drawing sheet model); `issue22475` one `text_thickness` of a TrueType text
missing (outline-font branch of text_dims, DRC-24 gap); `fw16_MCIO8i` one `connection_width`
entry names other items (pointer-set order); `api_kitchen_sink` one `clearance` missing
(barcode 'acode' on F.Cu against an Edge.Cuts polygon, rule `barcode visual separation`;
open, belongs to the clearance providers). Matched per code: every code of the ported
providers on the other 352 boards; stdout and exit codes equal on all 364 boards
(the Rust stdout additionally carries the not-run lines).

Error codes compared (ported providers): all codes of annular_width, disallow, connectivity,
copper width, clearance, physical_clearance, courtyard_clearance, edge_clearance, footprint
checks, hole_to_hole_clearance, hole_size, miscellaneous, text_dimensions, track angle,
width, segment_length, zone connections, diameter, microvia_stack, solder_mask_issues,
silk_clearance, sliver checker, text_mirroring. Not compared: the codes of
`UNPORTED_PROVIDERS` (creepage, lib_footprint_*, schematic parity codes,
net_chain_tuning_profiles, via_stitch_overlap, length / skew / via count / net chain codes,
diff pair codes).

Not verified yet: `--refill-zones` / `--save-board` (kicad-zone path, not run against the
reference), `--units in|mils`, `--severity-error|warning|exclusions`, `--all-track-errors`,
`--exit-code-violations` with the job (the report writers are verified for units and
severities by DRC-11), the rule-file error exit (unit-level only: `run_drc` path), parity
messages.

### Ported C++ tests

| C++ | Rust |
|---|---|
| `qa/tests/pcbnew/drc/test_drc_regressions.cpp` `DRCFalsePositiveRegressions` (21 boards) | `kicad-session/tests/drc12_regressions.rs` `drc_false_positive_regressions`: 21 of 21 pass |
| `DRCFalseNegativeRegressions` (23 boards, exact counts) | `drc_false_negative_regressions`: all 23 counts equal the C++ expectation; asserted only when no check was left out, which currently is never (diff pair coupling runs on every board with diff-pair nets, library parity where its codes are not ignored), so they are reported as pending with the missing providers |
| `DRCHoleToHoleReportsRuleValue` | `drc_hole_to_hole_reports_rule_value`: passes |
| `DRCZoneFalsePositiveRegressions`, `DRCTeardropOverCopperField` | not ported (need `KI_TEST::FillZones`) |

### Known gaps

- Engine change requests of the provider packages not done: `DrcRunHost` for `DrcRunState`
  (the `RunHost` wrapper stays), `DrcEngine::run_tests` still takes a cache closure (the real
  run is `run_flow::run_tests`), `DrcRtree` insert with an explicit parent, `silk_over_copper`
  from two providers (both run; report order follows the registry).
- `TUNING_PROFILES`, the drawing sheet and the footprint libraries are not passed by the
  session (see Verification).
- The shared harness `tests/drc_harness` now builds the connectivity
  (`build_connectivity`) and runs the providers with it (orchestrator note).

## DRC-32: creepage utilities, part 1 (`drc_creepage_utils.h`, `drc_creepage_utils.cpp`)

Module `src/creepage_utils.rs` with `src/creepage_utils/{shapes,paths,board_edges,graph,pending}.rs`.

| C++ | Rust | State |
|---|---|---|
| `CREEP_SHAPE`, `CU_SHAPE*`, `BE_SHAPE*` (classes, virtual accessors) | `shapes::{CreepShape, CuShapeSegment, CuShapeCircle, CuShapeArc, BeShapePoint, BeShapeCircle, BeShapeArc}` | ported |
| `BOARD_ITEM*` of shapes / board edges | `shapes::CreepItem` (`Item(ItemId)` or `OwnedEdge(index into m_ownedBoardEdges)`) | ported |
| `compareShapes`, `areEquivalent`, `segmentIntersectsArc`, `segments_intersect`, `BE_SHAPE_ARC::IsThereATangentPassingThroughPoint`, `AngleBetweenStartAndEnd` | `shapes` | ported |
| `PATH_CONNECTION` (+ `isValid`), `CREEPAGE_TRACK_ENTRY`, `TRACK_RTREE`, `ReversePaths` | `paths::{PathConnection, CreepageTrackEntry, TrackRtree, reverse_paths}` | ported; `is_valid` calls `SegmentIntersectsBoard` (DRC-33, pending) |
| all 23 `<shape>::Paths( <shape> )` bodies (cpp L272-606, L1075-1961, incl. the L1611-1961 ones of the DRC-33 range, needed by the circle-prune test), `closestPointOnSegment` | `paths::paths_<receiver>_<argument>`, `CreepShape::paths` (the virtual table = `GetPaths` dispatch) | ported |
| `BuildCreepageBoardEdges` | `board_edges::build_creepage_board_edges` | ported |
| `GRAPH_NODE`, `GRAPH_CONNECTION` (+ `GetShapes`), `CREEPAGE_GRAPH` model, `TransformEdgeToCreepShapes`, `RemoveDuplicatedShapes`, `TransformCreepShapesToNodes`, `ConnectChildren` (circle, arc), `ShortenChildDueToGV`, `SetTarget`; graph management of cpp L3016-3118 (`detachConnection`, `TruncateToPrefix`, `AddNode`, `AddNodeVirtual`, `AddConnection` x2, `FindNode`) | `graph::{GraphNode, GraphConnection, CreepageGraph}` (`connection_shapes` = `GetShapes`) | ported |
| `SegmentIntersectsBoard` | `pending::segment_intersects_board` | stub (DRC-33) |
| `segmentIntersectsCircle`, `GetPaths`, `Solve`, `Addshape`, `GeneratePaths`, `AddNetElements` | - | DRC-33 (`CreepShape::paths` already is the `GetPaths` dispatch) |

Model: shapes live in an arena (`CreepageGraph::shapes`, `ShapeId` = `CREEP_SHAPE*` identity,
never reordered); `shape_collection` is `m_shapeCollection` (sorted / deduplicated as ids).
Nodes and connections are `Vec`s indexed by `NodeId` / `ConnId`; `truncate_to_prefix` drops the
tails. `m_nodeset` is a `BTreeMap` lookup keyed (type, parent, x, y) that keeps the first equal
node (as the `unordered_set` insert does; relevant for virtual nodes). The graph borrows the
`BoardCtx`; `board_outline` is an owned copy (the C++ points to the caller's set). Board-edge
items resolve to their `PCB_SHAPE` through `board_edges::board_edge_shape` (shape, text box or
table cell base); `GetBoundingBox` is the virtual one (`ItemRef`).

Tests ported: `test_drc_creepage_circle_prune.cpp` (2 cases), `test_drc_creepage_issue24524.cpp`
(`CreepageStadiumCapsArcOutward`; `CreepageStadiumCapsBlockOuterSide` is `#[ignore]` until
DRC-33 ports `SegmentIntersectsBoard`), in `tests/drc32_creepage_utils.rs`.

Verification (`tests/oracle/creepage32_oracle.cpp` + `build_creepage32_oracle.sh`, one driver
linking the reference build; goldens `tests/golden/creepage32_{paths,boards}.txt`, replayed by
`tests/drc32_creepage_utils.rs`):
- `paths`: 42 generated shapes (7 per class: coincident centres, zero radius, degenerate
  segment, arcs across +-180 and over 180 degrees), every ordered pair through the C++ static
  overloads, two max weights: 3528 results, count + FNV hash of the bit-exact points and
  weights. All equal except the 68 segment x arc results of the reference-UB case below (path
  counts equal there).
- `boards`: the 19 QA creepage boards (`creepage`, `creepage_malformed_edge`, `creepage_slots`,
  the `issueNNNNN` boards of the creepage tests): outline flag, board-edge list (items by UUID,
  owned Bezier / NPTH pieces by geometry), overlapping-cutout flag, shapes after
  `RemoveDuplicatedShapes` (libstdc++ sort order), nodes, then for two (target, groove) configs
  child points on every board-edge circle / arc, the engine's connectChildren pair loop
  (`ConnectChildren`, `ShortenChildDueToGV`, the overlapping-cutout outline sampling of 4
  boards), the connections and their `GetShapes`, and `TruncateToPrefix`: 1996 lines, all equal.

Deviations:
- `CU_SHAPE_SEGMENT::Paths( const CU_SHAPE_ARC& )` (fallback branch) keeps `bestPath` pointing
  into the temporary vectors of its range-for loops and reads it after they are destroyed: the
  reference returns freed heap memory (glibc tcache pointers in `a1`, the path of a later
  vector that reused the chunk). The port returns the path the pointer referred to when it was
  taken (the intended value). Reference results there are not reproducible (ASLR); the replay
  compares the path count only for segment x arc pairs. Downstream (DRC-33): creepage paths
  between a track segment and an arc track inherit this.
- `GRAPH_NODE::m_node_conns` is a `std::set` of `shared_ptr` (heap-address order); the port
  orders it by connection index. Whatever iterates it (DRC-33 `Solve`) must treat ties as
  unordered in the reference.
- `BuildCreepageBoardEdges` flattens Bezier edges with `build_bezier_to_segments_points_list`
  instead of rebuilding the board item's cached list (same points; the board stays immutable).
- `ShortenChildDueToGV` calls `asin` on a `float` (`asinf` in the reference): the port uses
  `f32::asin`, the platform `asinf` natively (glibc, matched by the goldens); wasm32 uses
  Rust's libm (the documented wasm-only libm deviation).

Known gaps: `PATH_CONNECTION::isValid` is unverified (needs `SegmentIntersectsBoard`, DRC-33);
the `aExclude` set of `BuildCreepageBoardEdges` and `AddNodeVirtual` / `AddConnection( n1, n2 )`
have no oracle coverage yet (used by DRC-33's engine).

## DRC-30: differential pair coupling (`drc_test_provider_diff_pair_coupling.cpp`)

### File / class map

| C++ | Rust | State |
|---|---|---|
| `DRC_TEST_PROVIDER_DIFF_PAIR_COUPLING::Run` | `src/providers/diff_pair_coupling.rs` `DrcTestProviderDiffPairCoupling::run` (takes the run's `&DrcBoardCaches` for `m_CopperItemRTreeCache`) | ported |
| `commonParallelProjection( SEG, SEG, ... )` / `( PCB_ARC, PCB_ARC, ... )` | `common_parallel_projection_seg` / `common_parallel_projection_arc` | ported |
| `DIFF_PAIR_KEY` (+ `operator<`), `DIFF_PAIR_COUPLED_SEGMENTS`, `DIFF_PAIR_ITEMS` | `DiffPairKey` (+ `key_less`), `DiffPairCoupledSegments`, `DiffPairItems` | ported |
| `std::map<DIFF_PAIR_KEY, DIFF_PAIR_ITEMS> dpRuleMatches` | `DpRuleMatches`: the libstdc++ red-black tree of `operator[]` (`lower_bound`, `_M_emplace_hint_unique`, `_Rb_tree_insert_and_rebalance`) on an index arena | ported (exact) |
| `extractDiffPairCoupledItems`, `isInTuningPattern` | same names, snake_case | ported |
| `GetFromToCache()->Rebuild( m_board )` (first statement of `Run`) | `run_flow::run_provider` calls `ConnectivityData::rebuild_from_to_cache` right before the provider (needs `&mut`) | ported |
| `ADVANCED_CFG::m_MinParallelAngle` | `ADVANCED_CFG_MIN_PARALLEL_ANGLE` (default 0.001 deg) | default only |
| debug overlay (`GetDebugOverlay`) | - | skipped: GUI only |
| registry wiring | `run_flow.rs`: removed from `UNPORTED_PROVIDERS`, `"diff_pair_coupling"` arm in `run_provider` | done |

Rule identity: the key keeps copies of the gap / uncoupled `DrcRuleInfo` (with `rule_index`,
FIX-DRC-1) for `SetViolatingRule`; as in C++ they are not part of the key ordering (names are).
Numeric conversions follow the C++ exactly: `int += double` lengths and `int length` of the
arc case through `f64_to_i32_x86`, `computedGap = sqrt( int64 )` truncated through
`f64_to_i64_x86`, the half-width sum in `int` division, the rescaled clip points `int + int64`
truncated to `int`.

### Verification

- Reference cases (`tests/drc30_diff_pair_coupling.rs`, golden `tests/golden/drc30/reference.txt`
  from `tests/oracle/gen_drc30_golden.py`): the QA diff pair boards `issue7975`, `issue8407`,
  `issue17967/issue17967`, `diff_pair_uncoupled_tuning_drc`, `skew_within_diff_pairs_drc` (each
  with its own rules and with five rule variants appended from `tests/data/drc30/`: tight gap
  only, uncoupled only, both, gap rule restricted to F.Cu + uncoupled, and `mixed`: the gap
  rule ignored on B.Cu + uncoupled) and `issue24772/fw16_MCIO8i` (own rules, layer variant):
  32 cases, 630 entries of `kicad-cli pcb drc --format json --severity-all`, all matched
  (message bytes, items, item positions); 29 cases in exactly the reference order, the other
  3 (issue17967 gap / both / layer) differ only in the order of the negative items of one
  positive item (see deviations).
  The provider runs with the copper item tree of the cache generator and the run's
  connectivity.
- End to end (`kicad-rs pcb drc` vs the reference, 35 cases: the above plus fw16 gap / unc /
  both, where the gap code reaches the 199 error limit): 35 / 35 identical as multisets, 32 in
  reference order.
- `kicad-session/tests/drc12_regressions.rs` `drc_false_positive_regressions` now runs the
  provider (21 / 21 boards pass, the diff pair boards without a "not run" note).
- Arc coupling (issue8407, issue17967, fw16) and the tuning-pattern exclusion
  (`diff_pair_uncoupled_tuning_drc`) are covered by those cases. The `mixed` variant makes
  keys with an empty gap rule name next to keys with one (`DIFF_PAIR_KEY::operator<` is then
  not a strict weak ordering): with a sorted vector instead of the tree emulation issue17967
  gave 3 uncoupled entries instead of the reference's 2; with the tree emulation all match.

### Ported C++ tests

- `test_drc_regressions.cpp` `DRCFalsePositiveRegressions`, the four diff pair boards, provider
  only: `drc_false_positive_regressions_diff_pair` (0 errors each); the whole test runs in
  `kicad-session/tests/drc12_regressions.rs`.
- Unit tests in the module: `DIFF_PAIR_KEY` map order, segment parallel projection.

### Deviations

- `itemsP` / `itemsN` are `std::set<BOARD_CONNECTED_ITEM*>` (heap-address order) in C++ and
  `BTreeSet<ItemId>` (board track order) in the port. Not reproducible in the reference and
  compared tie-aware: the order of the gap entries of one positive item (follows the negative
  items), which two items an uncoupled-length entry shows (the first positive and the first
  negative item: compared by their nets), the order of its remaining items and its marker
  position (position of the first negative item, else of the first positive).

### Known gaps

- `ADVANCED_CFG::m_MinParallelAngle` is fixed at its default (no advanced config in the port).

## STAB-DRC: DRC against the reference with pads (fix loop, shared harness)

### Harness (consolidated)

`tests/drc_harness` now loads boards the way `KI_TEST::LoadBoard` / kicad-session's QA path
does: parser, then `kicad_pcb::project::LoadPath::Qa` steps through `run_model_step`
(`SetProject` with the full project: design / net / component class settings, tuning
profiles), `InitEngine` with the real binding (`PcbExprUcode`), `BuildListOfNets`,
`BuildConnectivity` (zone fill triangulation), `SynchronizeTuningProfileProperties`,
`SynchronizeComponentClasses`. No test skips boards with user rule conditions any more
(the `has_compiled_conditions` / `conditions_call_functions` / DRC-21 skips are gone).
Providers run through the real run path: `run_flow::run_tests_selected` (new, `run_tests`
with a registry-slot filter; `run_tests` calls it with every slot) via
`drc_harness::run_flow_providers` / `cli_report_entries_flow` / `compare_corpus_flow`:
cache generator, connectivity rebuild and net propagation, `PcbExprCacheScope`. The private
per-test wrappers (`ConnectivityWithCaches`, `CopperWithCaches`, `HoleAndEdge`,
`DisallowWithCaches`, `ZoneConnectionsWithCaches`, `FootprintAndMisc`) are removed;
`run_providers` (provider objects, no generator) still holds the expression cache scope.
Remaining test-private wrappers: drc06/26/27/28/29 (they build the generator caches
themselves; equal results) and `ThicknessOfOneItem` (one item).

### Fixed (each with its board / error code)

- courtyard `malformed_courtyard` missing (single_footprint_disjoint/unclosed_courtyard,
  two_footprints_one_malformed, issue2568): the test ran the provider without the cache
  generator, whose `BuildCourtyardCaches` sets the MALFORMED flags the provider reads
  (C++ `drc_cache_generator.cpp:242`); not a provider or model defect.
- `starved_thermal` extra on the drc25 synthetic board `thermals` (TP5 pad 5): the test added
  the side-table pads on top of the pads the parser now reads (every pad twice); the side
  table is now checked against the parsed pads instead.
- `unconnected_items` issue25580: the teardrop zone is saved without a UUID and gets a random
  KIID per load (also in the reference): `drc_harness::mask_generated_uuids` compares report
  items whose UUID is absent from the board file by description and position.
- `text_thickness` outline font: orchestrator decision implemented in `text_dims.rs` (the
  TrueType branch measures an unfractured copy of the stored glyphs). Crafted board
  `kicad-pcb/tests/data/font_rc1/frc.kicad_pcb` at min 0.1 / 0.15 / 0.2 / 0.25 mm: text_height
  and text_thickness entries identical to the reference in order (40 entries,
  `tests/golden/stab_drc_frc.txt`, `tests/oracle/gen_stab_drc_frc.py`,
  `drc24::text_thickness_outline_font_thresholds_match_reference`).
- KNOWN_RULE_GAPS of drc08a (issue16566 `A.Pad_Shape == 'Oval'`, issue23469
  `A.Reference == 'J1'`): match with the real binding; list emptied.
- `clearance` issue24211 (rules on `hasComponentClass('Inductor')`): matched once the harness
  ran `SynchronizeComponentClasses` (harness defect).
- EvalRules oracle: `pair_tp` key count on 15 boards with few pads (the same pad drawn twice
  in a row is one key in `gen_evalrules_goldens.py`): `Cat::key` counts consecutive distinct
  items; all 364 boards match (pair_tp 179 categories) except the known issue24735 deviation.
- Local copies replaced: `text_mirroring::board_is_layer_visible` -> `Board::is_layer_visible`,
  `via_stack::is_span_valid` -> `ViaStackData::is_span_valid`.

### Set / tie-aware comparisons (reference not deterministic)

- `assertion_failure` (misc `testAssertions` runs on the thread pool, `submit_loop`): order
  not compared (component_classes_drc: Rule2/R2 before Rule5/R4 in the reference, file order
  R4 first).
- `connection_width` on `issue24772/fw16_MCIO8i` (`POINTER_ORDER_BOARDS` in drc29): the
  net/layer polygon is merged from a `std::set<BOARD_ITEM*>`, so the neck reported and the
  items hit-tested at it follow heap order; compared without the items (description incl.
  actual width and layer equal).
- Unchanged from the provider packages: copper clearance, edge clearance, disallow, zone
  connections between tasks, isolated_copper between zones (thread pool / `ZONE*` order);
  error-limit types by count (issue832, stickhub-extra-via).

### Scoreboard (provider corpus replays, 365 golden boards)

Baseline: drc07 361 compared (unconnected_items 1325 matched / 1 missing / 1 extra), drc24
malformed_courtyard 0 / 4 missing, text_thickness 10 / 1 missing, drc25 synthetic 1 extra,
drc08a and drc09 332 compared (33 pending: rule conditions), the other replays skipped every
board with a user rule condition; 5 red tests.
Final: every replay compares 362 boards (366 for drc29), pending only issue4257 / issue7004
(legacy `.pro`) and plugins/fakeboard (reference fails). Matched / missing / extra per code:
all 0 missing / 0 extra (e.g. clearance 2084 + error-limit boards by count, hole_clearance
1413, shorting_items 371, unconnected_items 1326, via_dangling 426, track_dangling 532,
isolated_copper 198, drill_out_of_range 1645, via_diameter 1217, track_width 907,
silk_over_copper 918, silk_overlap 658, solder_mask_bridge 668, courtyards_overlap 135,
pth_inside_courtyard 86, malformed_courtyard 4, copper_edge_clearance 283,
silk_edge_clearance 175, copper_outside_board 217, text_height 227, invalid_outline 105,
items_not_allowed 66, starved_thermal 95, connection_width 22, copper_sliver 6), except the
expected difference below. All kicad-drc tests green.

### Open / expected differences

- issue22475 `text_thickness` on ff749b73 (`Franklin Gothic Medium`): font not installed on
  the oracle machine, the reference measures fontconfig's substitute, the port the stored
  glyphs (orchestrator decision; asserted as expected in drc24).
- api_kitchen_sink `clearance` 'acode' vs Edge.Cuts polygon ('barcode visual separation'):
  matches now (kicad-zint landed; drc26 physical clearance replay on the board: 1 / 0 / 0);
  kicad-session's `drc12_corpus` KNOWN_DIFFERENCES entry should be dropped.
- `fromTo()` rules in the corpus (CN-05 pending): via_stack_track_width.kicad_dru,
  via_stack_uvia_rule.kicad_dru, demos/vme-wren/vme-wren.kicad_dru.
- Raw zone_connect values: the `PrintZoneConnection` REPORT texts are not ported (DRC-34);
  `DrcConstraint::zone_connection` is an enum, so an out-of-range pad / footprint value reads
  as FULL (C++ keeps the raw value); not reachable from the corpus.

## DRC-31: matched length, skew, via count, net chains

### File / class map

| C++ | Rust | State |
|---|---|---|
| `pcbnew/drc/drc_test_provider_matched_length.cpp` (`DRC_TEST_PROVIDER_MATCHED_LENGTH`) | `src/providers/matched_length.rs` (`DrcTestProviderMatchedLength::new(&ConnectivityData)`) | ported (delay report mode not ported) |
| `pcbnew/drc/drc_length_report.h` | `src/length_report.rs` (`DrcLengthReport`, `LengthReportEntry`) | ported |
| `pcbnew/drc/drc_chain_topology.h` / `.cpp` (`CHAIN_TOPOLOGY`) | `src/chain_topology.rs` (`ChainTopology`) | ported |
| `pcbnew/net_chain_bridging.cpp` / `.h` | `src/net_chain_bridging.rs` | ported except `PartitionNetChainAroundNet` (tuner / net inspector only) |
| `pcbnew/length_delay_calculation/length_delay_calculation.cpp` / `.h`, `length_delay_calculation_item.cpp` / `.h` | `src/length_delay_calculation.rs` (`LengthDelayCalculation`, `LengthDelayCalculationItem`, `LengthDelayStats`, `PathOptimisations`) | ported (in `kicad-drc`: it needs the connectivity) |
| `pcbnew/length_delay_calculation/tuning_profile_parameters_user_defined.cpp` / `.h` | the tuning-profile methods of `LengthDelayCalculation` (`get_propagation_delays`, `get_via_propagation_delay`, `calculate_length_for_delay`, ...) | ported (the only provider of `TUNING_PROFILE_PARAMETERS_IFACE` the DRC uses) |
| `run_flow` registry slot `"length"` | `run_flow::run_provider` (rebuilds the from-to cache first) | wired; removed from `UNPORTED_PROVIDERS` |

### Verification

- Reference CLI: `tests/drc31_matched_length.rs` replays `tests/golden/drc31/reference.txt`
  (`python3 -I tests/oracle/gen_drc31_golden.py`, ~20 s): 56 cases (`tests/data/drc31/cases.tsv`)
  over 31 QA boards: the boards with their own length / skew rules (`length_calculations`,
  `time_calculations`, `issue22536`, `via_off_center`, `drc_skew`, `skew_within_diff_pairs_drc`,
  `skew_group_matched_drc`, `issue18045`, `issue23868`), rule variants appended (`all`: length +
  skew + via count, `same`: two rules with the same name (FIX-DRC-1 work item 3), `time`: time
  domain, `dp`: skew within diff pairs), the `net_chains/*` fixtures and `issue25065` with net
  chains inserted and chain rules (`chain`, `chaintime`, `lenchain`, return path `rp*`). Result:
  length_out_of_range 161/161, skew_out_of_range 66/66, too_many_vias 58/58,
  net_chain_stub_length 27/27, net_chain_return_path 5/5 matched, 0 missing, 0 extra (all
  descriptions byte-exact, including the measured lengths / delays).
- Comparison rule: entries as a multiset (rules are grouped in `std::map<DRC_RULE*, ...>`, address
  order); the report shows only the first two items of an entry (address order of a
  `std::set<BOARD_CONNECTED_ITEM*>`): items compared by count, the single item of a skew / via
  count entry by its net.
- `CHAIN_TOPOLOGY` is not reachable from the CLI: the load's `BuildListOfNets()`
  (`NETINFO_LIST::buildListOfNets`) clears the terminal pad pointers and restores only their
  UUIDs, so every chain of a loaded board reports `NO_TERMINAL_PADS` and the provider takes the
  legacy fallbacks (the reference agrees: the stub entries all carry "on net"). The topology
  is verified against the C++ QA expectations only (`tests/drc31_chain_topology.rs`, terminal
  pads re-resolved from their UUIDs as the tests' `SetTerminalPad` does), not bit for bit.

### Ported C++ tests

- `test_drc_lengths.cpp`: `DRCLengths` (4 boards, 0 violations: the `#base` cases of the
  reference replay) and `ViaElectricalSpan` (`drc31_chain_topology.rs::via_electrical_span`).
- `test_drc_chain_topology.cpp`: all 5 cases; `test_drc_chain_length_trunk.cpp`: both cases.
- `test_drc_skew.cpp`, `test_drc_skew_arc.cpp`, `test_drc_stub_length.cpp`,
  `test_drc_return_path_*.cpp`, `test_drc_length_with_chain.cpp`,
  `test_drc_chain_bridging_multipad.cpp`: their boards and rules are cases of the reference
  replay (chains assigned through the file instead of in code; the reference decides the
  expectation). Not ported as separate tests: the cases that set terminal pads on single nets
  in code (no mutable net API on a loaded board), the rule parser assertions of
  `test_drc_stub_length.cpp` (DRC-20 territory) and `test_drc_length_item_order.cpp`
  (address-order specific).

### Deviations

- Rule identity: `itemSets` / `matches` are keyed by `DrcRuleInfo::rule_index` (C++: rule
  address); a constraint without a rule index is skipped (C++ would dereference it). Report
  order across rules therefore differs from (the non-reproducible) C++ order.
- Item order: `ItemId` order for every `std::set<BOARD_CONNECTED_ITEM*>` (length calculation
  input order, the first item giving the marker position, the last item of skew / via count
  entries), for `CHAIN_TOPOLOGY`'s node / edge numbering and its `std::map<PAD*, ...>`; the
  first pad of `GetConnectedPads( via )` in `optimiseVias` (review-connectivity F-6).
- `TUNING_PROFILES::GetTuningProfile` of an unknown name returns the null profile whose
  `m_NetChainBridgePropagationDelay` is not initialised in C++; the port uses 0.
- Tuning profiles are read from the bound project when the calculator is constructed (C++:
  cached at `SynchronizeTuningProfileProperties`).

### Known gaps

- `runInternal( aDelayReportMode = true )` (net inspector delay report) is not ported.
- `PartitionNetChainAroundNet` is not ported (not used by DRC).
- The `PNS_TUNE` trace output of the length calculation is not ported.
- The chain topology has no bit-exact C++ oracle (see Verification); a C++ driver building
  `CHAIN_TOPOLOGY` on a board with terminal pads set in code would close this.

## STAB-DRC-2: `kicad-rs pcb drc` against the reference, board by board (second pass)

### Changes

- Board UUID (`BOARD::m_Uuid`): `kicad_pcb::Board::uuid()`, drawn from a generator of its own
  (seed `BOARD_UUID_SEED`; the board's item generator is not consumed, so every existing
  golden keeps its UUIDs) and reserved in `Board::kiid_generator`. The misc provider's two
  `invalid_outline` entries (`SetItems( m_board )`, the only places in `pcbnew/drc`) name it;
  `BoardRcItemResolver` resolves it after every item, as `BOARD::ResolveItem` does
  (`BOARD::GetPosition()` = (0, 0), `GetItemDescription()` = `PCB`). The text report line is
  now byte for byte the reference's; the JSON item carries a UUID that is random per run in
  the reference (normalised by the comparison tools, below).
- `run_flow::run_tests_selected`: `connectivity->GetFromToCache()->Rebuild( m_board )` right
  after the cache generator (`drc_engine.cpp:968-970`); the zone connections provider's second
  connectivity rebuilds its from-to cache too. The connectivity provider already evaluates its
  only rule (`VIA_DANGLING_CONSTRAINT`) with the run's connectivity in its context
  (`check_tracks`), so `fromTo()` conditions see the rebuilt cache there as well. The DRC-12
  deviation "the from-to cache is not rebuilt" is void.
- Marker UUIDs come from the board's generator (review R2-11); `DrcJobOptions::
  marker_uuid_seed` is no longer read (kept for the callers).
- misc provider `findClosestOutlineGap` uses `collect_board_shapes`.
- drc06 / drc26 / drc27 / drc28 / drc29 run their providers through the real run path
  (`run_flow_providers`, `cli_report_entries_flow`, `compare_corpus_flow`); the test-private
  wrappers (`PhysicalAndSilk`, `SolderMaskWithCaches`, `TrackAngleWithConnectivity`,
  `WithCaches`) are gone. All their tests are green with equal results.
- Comparison tools: the `PCB` masks are removed from `drc12_cli_compare.py`, the
  kicad-session replay `drc12_corpus.rs` and `kicad-cli/tests/oracle/e2e_cli.py`; the JSON
  item of the board is compared with its UUID normalised (`<BOARD>`). `e2e_cli.py` applies its
  masks cumulatively (each named only when it changes a differing output) and gained the
  documented entry-order mask of the drc12 tool (sorted entries per section, sorted
  `connection_width` items: thread pool / pointer-set order). The filtered codes of the drc12
  tool and the replay are the same list (unported providers plus the providers of running
  packages: library parity DRC-LP, creepage DRC-33, length / skew / via count / net chain
  DRC-31, diff pair codes).

### Scoreboards

`drc12_cli_compare.py cmp` (364 boards, `--severity-all --units mm`, text + JSON + stdout +
exit code):

| | exact | equal after filtering | equal as sets | differing |
|---|---|---|---|---|
| before (DRC-12 / STAB-DRC state, board without UUID: `PCB` lines masked) | 47 | 254 | 51 | 12 |
| after | 96 | 213 | 53 | 2 |

`e2e_cli.py --commands drc-report,drc-json` (308 boards, default options):

| | identical | identical after masks | differing |
|---|---|---|---|
| drc-report before (golden) | 43 | 79 (60 of them with the `PCB` line mask) | 186 |
| drc-report after | 91 | 42 | 175 |
| drc-json before (golden) | 43 | 20 | 245 |
| drc-json after | 45 | 88 (61 with the board UUID normalised) | 175 |

Every remaining difference is owned by another package (multiset of entries per board,
`target/stab-drc2/e2e_cat.py` logic): `lib_footprint_issues` missing on 166 report / 136+ JSON
boards (library parity, DRC-LP: was not run by the binary compared), `creepage` entries on 6
boards (DRC-33, smallest `creepage/creepage`), `skew_out_of_range` on
`skew_group_matched_drc` (DRC-31), `net_chain_tuning_profiles` on
`drc_net_chain_tuning_profiles` (unported, DRC-34); in the drc12 tool `issue22475`
(`text_thickness`, font not installed on the oracle machine) and `fw16_MCIO8i`
(`connection_width` neck from a pointer-ordered set) as before. The differences named by the
first pass (connection_width_rules rule name, issue5854, issue24525, issue24935,
microwave `shorting_items`, issue24853, issue25439, two_segment_teardrop, issue24078 counts)
no longer exist: all are equal or equal as sets (thread pool order: edge clearance,
copper clearance, isolated copper, connection width) in both tools.

Kicad-session replay `drc12_corpus`: 312 boards equal, 50 equal as sets, 2 known
differences, 1 pending (fakeboard: reference fails); `api_kitchen_sink` dropped from
`KNOWN_DIFFERENCES` (matches).

Options not verified by DRC-12, now run against the reference (`target/stab-drc2/opt_cmp.py`
logic, e2e masks):
- `--severity-exclusions` (text and JSON) on the 5 corpus projects that store exclusions
  (issue17429: 10, solder_mask_bridge_test, skew_within_diff_pairs_drc, skew_group_matched_drc,
  issue14008): 10 of 10 equal (identical or with the not-run list only).
- `--refill-zones --save-board --severity-all --format json` on 20 boards with filled zones
  (upgrade-identical boards of 4 to 34 KB: keepout_no_clearance, LayerEnumerate,
  large_circle_teardrop, teardrop_spike, fill_bad, teardrop_issue_JPC2, issue23790,
  issue2512, issue25521_zone_names, issue19956, issue17559, npth_mask_bridge_test,
  npth_pressfit_slot, issue5750, issue6443, issue24935, issue22983, issue21184, issue5567,
  issue24865): report, stdout, exit code and the saved board byte for byte: 15 identical,
  5 identical after the board UUID / random UUID (teardrop zones without UUID) / order masks
  (library parity entries filtered).

### Not done (change requests / known gaps)

- `RuleResolver::eval_zone_connection` (kicad-pcb `ctx.rs`) and its use in kicad-zone
  `knockouts.rs`: files of other packages (change request); the local copy in knockouts.rs
  is the same body.
- `providers/solder_mask.rs` keeps the whole-board mask as two `ShapePolySet`s instead of a
  detached `Zone` written through `Zone::fill_mut` (no observable difference; the C++
  `m_SolderMaskBridges` zone is read by nothing else in the port).

## DRC-LP: footprint library parity (`drc_test_provider_library_parity.cpp`)

### File / class map

| C++ | Rust | State |
|---|---|---|
| `DRC_TEST_PROVIDER_LIBRARY_PARITY::Run` (three `lib_footprint_issues` messages, `lib_footprint_mismatch`, per-run `libFootprintCache`, error limits, `GetCenter()` positions, `UNDEFINED_LAYER`) | `src/providers/library_parity.rs` `DrcTestProviderLibraryParity` | ported; registry slot wired in `run_flow.rs` (removed from `UNPORTED_PROVIDERS`); the adapter comes in `RunTestsOptions::footprint_libraries` (`None`: no tables) |
| `FOOTPRINT::FootprintNeedsUpdate` (flip / orientation of the `temp` clone, `NormalizeForCompare`, `footprintVsBoardStackup`, attributes, solder mask bridges, overrides (non-DRC flags), stackup, net-tie groups, shapes / barcodes / pads / zones in `cmp_drawings` / `cmp_pads` / `cmp_zones` order, `CHECKPOINT`s) | `library_parity::footprint_needs_update` | ported with both modes (`CompareReporter` `None` = DRC mode, `Some` = report mode with the C++ texts) |
| `TEST` / `TEST_PT` (`EPSILON` 10) / `TEST_D` (`EPSILON_D` 1e-5), `getBoardNormalizedLayerSet`, `boardLayersMatchWithInnerLayerExpansion`, `primitiveNeedsUpdate`, `padHasOverrides`, `padOrientationDiffers`, `padNeedsUpdate`, `barcodeNeedsUpdate`, `shapeNeedsUpdate`, `zoneNeedsUpdate`, `stackupNeedsUpdate`, `footprintVsBoardStackup` | same file, snake_case | ported; pad / footprint zone connections compared on the raw stored integers (`local_zone_connection_raw`) |

`temp->SetParent( GetBoard() )`: the library footprint stays in its holder board
(`FootprintLibraryAdapter::load_footprint`), which takes the board's copper layer count,
enabled layers and layer types before the flip / rotation. The `dummy` copy of the board
footprint is the same footprint in a clone of the board made once per run (on the first
comparison), whose graphic items are normalised.

### Deviations

- Zone `GetPadConnection()` is compared as the stored `ZoneConnection` enum: an out-of-range
  value in a file is already mapped by the parser, so two zones whose raw values differ but
  are both out of range compare equal (C++: different).
- `primitiveNeedsUpdate` / `shapeNeedsUpdate` with the same vertex count but fewer polygons on
  the library side read `CPolygon( poly )` out of range in C++ (undefined); the port treats
  it as a difference.
- `std::set` ties (two drawings / pads / zones with identical sort keys and the same UUID)
  fall back to `ItemId` order instead of heap-pointer order.
- The holder board takes the board's layer count, enabled layers and layer types, not
  custom `LAYER::m_opposite` pairs (they are recomputed from the copper count).
- Tables in footprints are not normalised (`PCB_TABLE::Normalize`); the comparison does not
  read them.

### Verification

- Corpus (`tests/oracle/drc12_cli_compare.py cmp`, lib codes no longer filtered, 364 boards):
  294 exact, 15 filtered, 53 same set, 2 differing (`text_thickness`, `connection_width`, not
  this package), 1 reference failure; no `lib_footprint_*` entry missing or extra. Before:
  175 boards differed only by these codes. `tests/golden/drc12_cli.txt` regenerated.
- Demos with a project `fp-lib-table` (copied with their libraries, reference vs kicad-rs,
  every `lib_footprint_*` entry incl. item, position and order): pic_programmer (58 entries:
  missing `${KICAD10_FOOTPRINT_DIR}` libraries, the project library matching),
  royalblue54L_feather (67), multichannel_mixer-unrouted (114), and 0 entries on
  kit-dev-coldfire-xilinx_5213, stickhub, complex_hierarchy, interf_u (`$(KIPRJMOD)`),
  multichannel_mixer, video (`${KICAD6_FOOTPRINT_DIR}`), ecc83-pp, ecc83-pp_v2,
  sonde xilinx: all identical. Not run (large-board rule): tiny_tapeout, cm5_minima,
  vme-wren.
- Crafted cases: `kicad-session/tests/drc_lp_library_parity.rs` with
  `tests/data/drc_lp` (generator `tests/oracle/gen_drc_lp_cases.py`): 61 crafted
  footprints (pad position / size / drill / shape / layers / number / type / orientation
  (symmetric and not) / overrides / epsilon, graphic moved / width / deleted / reversed
  segment / rect corners swapped, arc mid, polygon vertex, circle, bezier, copper stroke,
  fill, roundrect / chamfer ratio and corners, custom primitive, attributes, net-tie groups,
  texts, 3D model, rotation 30 / 45 / 90, a flip, rule-area priority / corner / keepout / name
  / missing zone, a library-side difference) and library-table situations (not in table,
  disabled, missing directory, corrupt file in the library, unknown type, nested table and
  a missing nested library, `$(KIPRJMOD)`, relative URI, global table rows, a disabled
  global row, a global row shadowed by the project row, escaped nickname, missing
  footprint, a legacy table without version and with unquoted tokens): the 37 reference
  entries identical (message, item, position, order); the same project through
  `kicad-rs` on the file system is identical too.
- QA `DRCFalseNegativeRegressions` (`kicad-session/tests/drc12_regressions.rs`): all 23 counts
  are now asserted (no check left out) and pass; `DRCFalsePositiveRegressions` 21 of 21.

### Known gaps

- Rows of the other plugin types (`Legacy`, `Eagle`, `gEDA / Lepton EDA`, ...) fail to load
  in the port ("not enabled" message). The reference loads e.g. an `Eagle` row whose URI is
  a directory as an empty library ("Footprint '...' not found in library '...'"), and real
  Eagle / legacy libraries with their footprints.

## DRC-33: creepage utilities part 2, creepage engine, creepage provider

Files: `src/creepage_utils/board_intersect.rs`, `src/creepage_utils/solve.rs`,
`src/creepage_engine.rs`, `src/providers/creepage.rs`; tests `tests/drc33_creepage.rs`; oracle
`tests/oracle/creepage33_oracle.cpp` + `build_creepage33_oracle.sh` + `gen_creepage33_golden.py`,
goldens `tests/golden/creepage33_{board,engine}.txt`. Registry: `run_flow` runs the provider
(`"creepage"` left `UNPORTED_PROVIDERS`). Supersedes the DRC-32 rows "`SegmentIntersectsBoard`
stub" and "`segmentIntersectsCircle` ... `AddNetElements` - DRC-33"; `creepage_utils::pending`
is kept as an alias module of `board_intersect` for the (ignored) DRC-32 test that names it.

| C++ | Rust | State |
|---|---|---|
| `segmentIntersectsCircle`, `SegmentIntersectsBoard` | `board_intersect::{segment_intersects_circle, segment_intersects_board}` | ported |
| `GetPaths` | `solve::get_paths` (= `CreepShape::paths`, the most-derived dispatch) | ported |
| `CREEPAGE_GRAPH::Solve`, `Addshape`, `GeneratePaths`, `AddNetElements` | `CreepageGraph::{solve, add_shape, generate_paths, add_net_elements}` (`solve.rs`) | ported |
| `CREEPAGE_RESULT`, `CREEPAGE_ENGINE` (whole-board solve, interactive Begin/Update/End, `populateBoardEdgeGraph`, `connectChildren`, `extractResult`, `addNetsInRegion`, `buildBoardEdgePrefix`, `movingItemsHaveNPTH`) | `creepage_engine::{CreepageResult, CreepageEngine}` | ported |
| `DRC_TEST_PROVIDER_CREEPAGE` (`Run`, both `testCreepage`, `testCreepageV2`, `placeholderConstraint`, `reportCreepageViolation`, `GetMaxConstraint`, `CollectNetCodes`, `CollectBoardEdges`) | `providers::creepage::DrcTestProviderCreepage` | ported |
| `ADVANCED_CFG::m_RealtimeCreepage`, `m_EnableCreepageSlot` | provider fields `realtime_creepage`, `enable_creepage_slot` (default `false`, as the reference) | ported |

Model: the engine keeps no board borrow (the interactive mode needs the board to change
between frames): every method takes the context of the moment and the interactive graph is
parked as `creepage_engine::DetachedGraph` between calls. Constraint function:
`Box<dyn Fn(i32, i32) -> f64>`; moving items: `BTreeSet<ItemId>`.

Order (see the module comment of `solve.rs`): `CREEP_SHAPE*` order = `ShapeId`,
`BOARD_ITEM*` order = `Ord` of `CreepItem` (`ItemId` order, owned edges last),
`GRAPH_NODE*` = `NodeId`, `m_node_conns` = `ConnId` order, pointer-keyed `unordered_map`s =
first insertion. `GeneratePaths` uses the thread pool only when `get_tasks_total() <
get_thread_count() - 4`: inline on the 4-thread reference machine, sequential in the port. The
provider's net pairs are sequential in C++ too.

Tests ported (`tests/drc33_creepage.rs`): `test_drc_creepage.cpp` (3), `test_drc_creepage_issue{20480,
21482,23364,23389,23576,23578,23658,24286,24523,24543,24597,24853,24875 (3 cases),24885,25439}`,
`CreepageStadiumCapsBlockOuterSide` of issue24524 (un-ignored copy of the DRC-32 one),
`test_creepage_engine_parity.cpp` (3 boards), `test_creepage_engine_incremental.cpp` (2),
`qa/tests/pcbnew/test_creepage_groove_order.cpp`. Partial: `CreepageRotatedRectPadIssue24543`
checks the path ends on both pad outlines (the hole-distance check is implied, not ported);
`CreepagePathStartPointIssue23576` takes the end point from the path shapes instead of
`GetErrorLegendShapes()` (same ends without the stops); `CreepageIgnoresRuleAreas` writes the
C++-built board as a board file; `CreepageWholeBoardSolveDeterministic` runs 100 trials, not
1000. Not ported: `test_drc_creepage_issue24544.cpp` (checks the drawn path against the
rounded slot corner geometry; the corpus replay covers the board, distance exact).
`CreepagePerformanceIssue21482` is `#[ignore]`: the reference build needs ~80 s for the board on
this 4-core machine (threshold 20 s assumes the pool); the port takes ~210 s in the test profile.

Verification:
- `creepage33-oracle board` (the legacy provider pipeline with the provider's own statements
  against the reference build, every solve printed) on the 20 creepage QA boards: 1318 solves.
  `drc33_oracle_board_replay` (default run without issue21482, `DRC33_FULL=1` for all): every
  distance bit-identical; 535 solves identical in items, ends and path hash; 783 differ only in
  which of equally short paths is returned, and for each the replay proves both reference
  items lie on a shortest path of the Rust graph (full Dijkstra from both virtual nodes). The
  reference breaks these ties by heap address (queue tie on `GRAPH_NODE*`, `m_node_conns`
  order, node creation order from the pointer sort): not reproducible, compared tie-aware.
- `creepage33-oracle engine` (`SolveNetPairWholeBoard`, 50 mm, every pair on F_Cu / B_Cu, 19
  boards): 350 results, distances all bit-identical, 337 identical, 13 tie ends / items.
- Corpus (`drc33_creepage_corpus`, reference kicad-cli reports, run flow with the creepage
  slot only): 19 boards, 30 `creepage` violations: 28 identical, 2 differ only by a tie (same
  rule, creepage and actual distance; issue20480 and issue24875_capstraight: the other of two
  same-net tracks sharing the end point; proven ties by the replay), 0 missing / extra
  otherwise.
- `test_creepage_groove_order.cpp` covers the groove-width path of `SegmentIntersectsBoard`.

Deviations:
- Paths with a NaN end (overlapping circles: the tangent construction takes the square root of
  a negative number) have NaN weight, so Dijkstra never relaxes them and their `INT_MIN` end
  nodes are dead ends. The C++ validates them with `INT_MIN` coordinates (signed overflow, UB);
  `GeneratePaths` rejects them before validation and `segment_intersects_arc` (DRC-32 file,
  minimal fix) reports a crossing for an `INT_MIN` end. Same shortest paths; only unusable
  nodes / connections are missing.
- `SHAPE_LINE_CHAIN::PointInside` (kicad-geom, minimal fix + test
  `tests/point_inside_wrapping.rs`): the two `int` subtractions wrap as on x86-64 instead of
  panicking.
- Fixed in the DRC-32 files: `std::max( x, 0.0 )` was ported as `f64::max`, which drops a NaN
  (`std::max` returns it): `shapes::std_max` now; this changed issue24885's result (a NaN arc
  weight became 0 and opened a 2.86 mm shortcut; the reference reports 3.0192 mm, now equal).
- `SegmentIntersectsBoard`'s `int GVSquared = w * w` wraps (x86-64) for a groove width above
  46340 nm (only with `enable_creepage_slot`).
- Markers: `reportCreepageViolation` (V2 path) gets item UUIDs from the board only (the
  engine's graph is gone; a copper item is never a transient edge).

Known gaps: creepage paths between a track segment and an arc track inherit the DRC-32
reference-UB deviation (no corpus board has one in a reported path). Performance: ~2.5x the
reference on issue21482 (pad bounding boxes and effective shapes are recomputed per call).

## DRC-34: leftovers (EvalRules reporter texts, own-clearance cache, net chain tuning profiles, via-stitch overlap, `GetViolationLayers`)

Supersedes the "REPORT (DRC-34)" known gap and the `InitializeClearanceCache` "skipped" row of
the DRC-04 section, and the two DRC-34 entries of `run_flow::UNPORTED_PROVIDERS` (DRC-12).

| C++ | Rust | State |
|---|---|---|
| `drc_engine.cpp` `REPORT( ... )` texts of `EvalRules` (net tie, local overrides, `reportConstraintHeader`, `processConstraint`, `testAssertion`, DIFF_PAIR_GAP clearance floor, parent inheritance, local clearances / board minimum, footprint / zone connection and thermal defaults), `EvalZoneConnection`, `ProcessAssertions` | `src/engine/eval_rules.rs` (the sites), `src/engine/eval_rules_report.rs` (`report` = the `REPORT` macro, `report_constraint_header`, `ReportUnits` = the engine's `UNITS_PROVIDER`) | ported |
| `PrintZoneConnection` (`pcbnew/zones.h`) | `engine::eval_rules_report::print_zone_connection` | ported |
| `DRC_ENGINE` as `UNITS_PROVIDER` (`SetUserUnits` / `GetUserUnits`) | `DrcEngine::set_user_units` / `user_units` (`src/engine/mod.rs`, additive) | ported (see deviations) |
| `DRC_ENGINE::InitializeClearanceCache` | `DrcEngine::initialize_clearance_cache` (`eval_rules.rs`) | ported (sequential) |
| `DRC_ENGINE::InvalidateClearanceCache`, `ClearClearanceCache` (L3267-3294) | `invalidate_clearance_cache`, `clear_clearance_cache` (DRC-04) | ported (checked: complete) |
| `drc_test_provider_net_chain_tuning_profiles.cpp` | `providers/net_chain_tuning_profiles.rs` `DrcTestProviderNetChainTuningProfiles`, `StdStringKeyedOrder` (libstdc++ `unordered_map<wxString,..>` node order) | ported |
| `drc_test_provider_via_stitch.cpp` | `providers/via_stitch.rs` `DrcTestProviderViaStitch`, `via_stitch_net_code` (`PCB_VIA_STITCH::GetNetCode` after a load) | ported |
| `drc_item.cpp` `DRC_ITEM::GetViolationLayers` (L628-710) | `src/violation_layers.rs` `violation_layers` -> `ViolationLayers` | ported |
| `GetItemsMatchingCondition`, `GetItemsMatchingRule`, show-match domains (L2690-3210) | - | skipped: GUI (rule editor "show matches"), as recorded by DRC-03 |

Both providers run in `run_flow::run_provider` (registry slots "net chain tuning profiles",
"via_stitch_overlap"); `UNPORTED_PROVIDERS` holds only schematic parity now.

### Verification

* **Reporter texts** (`tests/drc34_report_oracle.rs`, C++ driver
  `tests/oracle/drc34_report_oracle.cpp` + `build_drc34_report_oracle.sh` linking the
  reference build, golden `tests/golden/drc34/evalrules_report.txt` from
  `gen_drc34_report_golden.py`): every corpus board < 300 kB plus the four review boards,
  QA load path; per item kind the first 6 items with CLEARANCE per copper layer and 39 other
  constraint types, DISALLOW, `ProcessAssertions`, an `INCH` pass, and the dump_evalrules
  pairs (CLEARANCE per layer, `EvalZoneConnection`, thermal, 11 pair types), each call with
  its own `WX_STRING_REPORTER`. Result: **307 boards, 2367 categories = 235097 calls
  byte-identical**; 4 categories differ because of code outside this package (listed, not
  failed: `KNOWN_OTHER_PACKAGES`): issue2512 inch / pair_zt / zone - the expression evaluator
  does not send its error callback ("ERROR: Unrecognized layer '*.Silkscreen'" of
  `existsOnLayer`) to the reporter; issue24735 drawing - `A.Layer == 'F.Silkscreen'` is true
  for the board `gr_text` on F.SilkS in the port and false in the reference (a rule result
  difference: the "layer map of the QA load path" known deviation of `evalrules_oracle.rs`).
  Pending: issue4257 (legacy `.pro`), fakeboard (reference
  cannot load it).
* **Net chain tuning profiles**: `DRCNetChainTuningProfiles` ported (1 violation, the
  reference text); the synthetic boards `tests/data/drc34/chains_{3,14,17}` (generated from
  the QA board by `gen_drc34_boards.py`, 3 / 14 / 17 conflicting chains incl. non-ASCII and
  bracketed names, two rehashes) report every chain in the reference order (34 / 34, golden
  `tests/golden/drc34/chains.txt`). Corpus: 362 boards, `net_chain_tuning_profiles` 1 / 1
  matched, 0 missing / extra.
* **Via-stitch overlap**: synthetic `tests/data/drc34/stitch_overlap` (issue25303's stitch
  plus four copies: same net overlapping, other net overlapping, same net far away, same net
  overlapping two) - 3 / 3 violations in the reference order with the reference items and item
  positions (golden `tests/golden/drc34/stitch.txt`). Corpus: 362 boards, no
  `via_stitch_overlap` in the reference, none in the port. The marker position (bbox centre of
  the intersection) is not part of the CLI reports and is verified by review only.
* **`GetViolationLayers`**: `test_drc_violation_layers_issue24941.cpp` (three cases) ported.

### Ported C++ tests

`test_drc_net_chain_tuning_profiles.cpp` (`DRCNetChainTuningProfiles`: only the provider
under test runs; the C++ test counts all error-severity violations of a full run with four
checks ignored), `test_drc_violation_layers_issue24941.cpp` (`EdgeClearanceFocusesEdgeCuts`,
`MarkerLayerTakesPriority`, `FallbackToItemLayers`) in `tests/drc34_leftovers.rs`.

### Deviations

* Engine units: C++ `RunTests( aUnits )` calls `SetUserUnits` on the engine, so texts produced
  after a run are in the run's units; the port keeps the run's units in `DrcRunState`, and the
  texts use `DrcEngine::user_units` (MM until `set_user_units`). No CLI path produces texts.
* `HOLE_TO_HOLE` with a hole in `a` and no `b`: the C++ dereferences null to name the item
  without a hole; the port treats `b` as hole-less (DRC-04) and writes no text for it.
* `InitializeClearanceCache`: the C++ thread blocks overwrite within a block and keep the first
  block's value across blocks; the port keeps the first value of the list. Only duplicate
  (uuid, layer) keys (duplicate UUIDs) can differ, and their value is a block-partition race in
  C++.
* Net chain map: keys beyond the modelled part of `__prime_list` (more than 953 net chains) are
  iterated in name order (not reachable in practice). A net without a netclass (null
  dereference in C++) counts as the empty profile.
* `%.3f` of the hole aspect ratio header uses Rust `{:.3}`; the value is an `int / 1000.0`, never
  an exact decimal tie, so both round the same.

### Known gaps

* The texts reach other packages' code that is compared only through this oracle: the item
  descriptions (`GetItemDescription`) and the expression evaluator's reporter output; see the
  two known causes above.
