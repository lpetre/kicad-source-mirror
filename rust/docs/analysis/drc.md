# DRC (design rule checker) - porting analysis

Analyst document for the port of `pcbnew/drc/` (plus the expression language it uses) to the Rust
crate `kicad-drc`. The C++ tree is the specification; all line numbers refer to the files in
`<kicad>` as checked in (KiCad master, board file format 20261031, `KICAD_SEMANTIC_VERSION`
`10.99.0-unknown`). `drc_engine.cpp:L1033` means line 1033 of `pcbnew/drc/drc_engine.cpp`.

Read `rust/PORTING.md` first. This document gives the map and the work packages; the implementer
of a package reads this document plus the C++ files of that package.

**Reading guide** (the document is long; read what your package needs):

| If you implement | Read |
|---|---|
| any package | 1 (skim), 3.1-3.6, 4.9, 4.12, 8, 10.1 and the R-items of 11 that mention your files |
| DRC-01 (types) | 3 |
| DRC-02 (R-tree, provider base) | 3.7, 4.9 |
| DRC-03 / DRC-04 (engine, `EvalRules`) | 4.1-4.6, 4.8.1, 4.8.5 |
| DRC-05 (cache generator) | 3.8, 4.5 |
| a provider package | 4.9, the provider's subsection of 4.10, 5 |
| DRC-10 (disallow / keepouts) | 4.2 (keepout rules), 4.6 c, 4.8.4, 4.10.2 |
| DRC-11 / DRC-12 (reports, CLI) | 1.2, 3.5, 3.6, 4.11, 6, 7 |
| DRC-20..22 (rule files, expressions) | 4.3, 4.7, 4.8 |
| tests | 9 |

Contents:

1. Purpose and entry points
2. File inventory
3. Data structures
4. Control flow and algorithms (engine, rules, every provider, order sensitivity)
5. What DRC needs from other subsystems
6. Output formats
7. Backward compatibility
8. Multi-threading and the sequential port
9. Existing tests and data, differential tests
10. Rust module layout and work packages
11. Risks, traps, open questions

---

## 1. Purpose and entry points

The DRC subsystem answers two different kinds of question:

* **"What constraint applies here?"** - `DRC_ENGINE::EvalRules( type, a, b, layer )` resolves one
  constraint (clearance, track width, hole size, zone connection style ...) for one or two board
  items from: local overrides on the items, implicit rules generated from the board setup and
  the netclasses, keepout rule areas, and user rules from the `.kicad_dru` file. This query is
  used by DRC itself but also by the zone filler, the router, the painter
  (`BOARD_CONNECTED_ITEM::GetOwnClearance` -> `DRC_ENGINE::GetCachedOwnClearance`) and the plot
  code (solder mask / paste margins). `kicad-zone` therefore depends on `kicad-drc`.
* **"Does the board violate the rules?"** - `DRC_ENGINE::RunTests` builds caches, then runs 30
  *test providers* in a fixed order; each provider walks the board and calls
  `DRC_ENGINE::ReportViolation( DRC_ITEM, position, layer )` for each violation. The caller
  installs a violation handler that turns the items into `PCB_MARKER`s; reports are written from
  the markers.

### 1.1 Callers

| Caller | File | In port scope |
|---|---|---|
| `kicad-cli pcb drc` | `kicad/cli/command_pcb_drc.cpp` -> `JOB_PCB_DRC` -> `PCBNEW_JOBS_HANDLER::JobExportDrc` (`pcbnew/pcbnew_jobs_handler.cpp:L2852-3088`) | yes (tier 1) |
| Board load (creates the engine, loads rules) | `BOARD_LOADER::initializeLoadedBoard` (`pcbnew/board_loader.cpp:L100-170`) | yes |
| Zone filler, router, painter, plotters | call `EvalRules` / `EvalZoneConnection` / `GetCachedOwnClearance` | `EvalRules` yes; own-clearance cache is an optimisation (tier 3) |
| GUI: `DIALOG_DRC`, `DRC_TOOL`, rule editor, "show matches", clearance resolution reports | `pcbnew/dialogs/dialog_drc*.cpp`, `pcbnew/tools/drc_tool.cpp`, `pcbnew/drc/rule_editor/*` | no |
| IPC API (`api_handler_pcb.cpp`), Python | - | no |

### 1.2 The `kicad-cli pcb drc` flow, in execution order

`kicad/cli/command_pcb_drc.cpp:L43-182`. Arguments and defaults:

| Flag | Default | Effect |
|---|---|---|
| positional input | required | board file |
| `-o/--output` | empty -> `<board stem>-drc.rpt` or `<board stem>-drc.json` (`JobExportDrc` L2869-2880), resolved against the cwd/project by `JOB::GetFullOutputPath` | report path |
| `-D/--define-var KEY=VALUE` | none | project text variable overrides (applied in `resolveJobOutputPath`, L3698-3713, i.e. *after* the rules were loaded) |
| `--format` | `report` | `report` (text) or `json`; anything else -> stderr `Invalid report format`, exit 1 |
| `--all-track-errors` | false | `m_reportAllTrackErrors`: report every clearance error of a track instead of stopping at the first |
| `--schematic-parity` | false (the job default `m_parity( true )` in `job_pcb_drc.cpp` is overwritten by the CLI) | run schematic parity (needs eeschema netlist; out of scope) |
| `--units` | `mm` | `mm`, `in`, `mils`; other non-empty -> `Invalid units specified`, exit 1. Used for all values in messages and coordinates |
| `--severity-all` | off | severity mask = error or warning or exclusion |
| `--severity-error`, `--severity-warning`, `--severity-exclusions` | off | OR bits into the mask. If no severity flag is given the job default `RPT_SEVERITY_ERROR or RPT_SEVERITY_WARNING` (`job_rc.cpp`) is kept |
| `--exit-code-violations` | false (job default true is overwritten) | return 5 when anything is reported |
| `--refill-zones` | false | refill all zones before the test |
| `--save-board` | false | only with `--refill-zones`: save the board afterwards |

`PCBNEW_JOBS_HANDLER::JobExportDrc` then does, in this order:

1. `getBoard( path )` (L540-629): load `<stem>.kicad_pro` through the settings manager, then
   `BOARD_LOADER::Load`. Failure -> message `Failed to load board[: reason]`, exit 3
   (`ERR_INVALID_INPUT_FILE`). The loader (`board_loader.cpp:L100-170`) does, in order:
   `SetProject` (links `BOARD_DESIGN_SETTINGS` to the project file: severities, exclusions,
   netclasses, rule minima come from `.kicad_pro`) -> create `DRC_ENGINE` ->
   `InitEngine( <board path with .kicad_dru> )` inside `try { } catch( ... ) { }` (a broken rule
   file is swallowed here; `RulesValid()` stays false) -> `ResolveDRCExclusions( true )` (creates
   excluded markers for the exclusions stored in the project whose items still exist) ->
   `BuildConnectivity` -> `BuildListOfNets` -> `SynchronizeNetsAndNetClasses( true )` ->
   `SynchronizeComponentClasses` -> `SynchronizeTuningProfileProperties`.
2. Footprint libraries are loaded (`adapter->AsyncLoad(); BlockUntilLoaded()`), needed only by
   the library-parity provider (out of scope, see 4.10.25).
3. Output path, units.
4. `if( !drcEngine->RulesValid() ) InitEngine( brd->GetDesignRulesPath() )`; a `PARSE_ERROR` is
   reported as `DRC incomplete: could not compile custom design rules.\n<what>\n` and the job
   returns 3 **without writing a report** (asserted by `qa/tests/cli/test_pcb.py`).
5. `SetDrawingSheet` (only used by the text-variable test on drawing-sheet texts).
6. Schematic parity netlist (only with `--schematic-parity`).
7. `--refill-zones`: `ZONE_FILLER_TOOL::FillAllZones`. Without the flag the fills stored in the
   file are tested as they are.
8. Violation handler: `new PCB_MARKER( item, pos, layer )`, path generator, `commit.Add`.
9. `brd->RecordDRCExclusions()` (remember which existing markers are excluded, into
   `bds.m_DrcExclusions`), `brd->DeleteMARKERs( true, true )` (drop all markers),
   `drcEngine->RunTests( units, reportAllTrackErrors, checkParity )`, `ClearViolationHandler`,
   `commit.Push` (markers are appended to `BOARD::m_markers` in report order).
10. `brd->ResolveDRCExclusions( false )`: every new marker whose identity matches a stored
    exclusion gets `SetExcluded( true, comment )`.
11. Three `DRC_ITEMS_PROVIDER`s over the board markers: `{MARKER_DRC, MARKER_DRAWING_SHEET}`,
    `{MARKER_RATSNEST}`, `{MARKER_PARITY}`; `SetSeverities( job severity mask )` filters and sorts.
12. Reporter messages (stdout): `Found %d violations`, `Found %d unconnected items`,
    (`Found %d schematic parity issues` with parity).
13. `DRC_REPORT::WriteJsonReport` or `WriteTextReport`; failure -> `Unable to save DRC report to
    %s`, exit 4. Then `Saved DRC Report to %s`.
14. `--refill-zones --save-board`: save the board (`Saved board` / `Failed to save board.` exit 4).
15. Exit code: 5 (`ERR_RC_VIOLATIONS`) if `--exit-code-violations` and any of the three counts
    is > 0, else 0. Exit codes are in `include/cli/exit_codes.h` (0 ok, 1 args, 2 unknown,
    3 invalid input, 4 output conflict, 5 violations).

Things the CLI flow does **not** do: it does not re-initialise the engine when the rules were
valid at load; it does not rebuild zone fills; it does not pass a log reporter (so rule file
errors throw instead of being collected, and deprecation warnings are silent).

---

## 2. File inventory

Line counts from `wc -l`. Scope: **T1/T2/T3** = port in that tier, **OUT** = not ported.

### 2.1 Engine, rules, items, report

| File | Lines | Role | Scope |
|---|---|---|---|
| `pcbnew/drc/drc_constraint_types.h` | 98 | `DRC_CONSTRAINT_T`, `DRC_DISALLOW_T`, `DRC_IMPLICIT_SOURCE` | T1 |
| `pcbnew/drc/drc_rule.h` / `.cpp` | 243 / 532 | `DRC_RULE`, `DRC_CONSTRAINT` (`IsUnary`, `Merge`), `IsComponentClassSelector`. `FormatRuleFromProto` / `ToProto` (L145-532) are IPC-API only | T1 (proto parts OUT) |
| `pcbnew/drc/drc_engine.h` / `.cpp` | 411 / 3410 | engine: implicit rules, compile, `EvalRules`, `RunTests`, limits, `ReportViolation`, diff-pair helpers, net-tie exclusion; L2659-3209 "show matches" (GUI); L3212-3410 own-clearance cache | T1 (L2659-3209 OUT, cache T3) |
| `pcbnew/drc/drc_rule_condition.h` / `.cpp` | 55 / 168 | `DRC_RULE_CONDITION`: expression + compiled ucode, `EvaluateFor` (commutative evaluation, teardrop type override) | T1 (native implicit conditions) + T2 (compiled) |
| `pcbnew/drc/drc_rule_parser.h` / `.cpp` | 88 / 1007 | `.kicad_dru` parser (`DRC_RULES_PARSER : DRC_RULES_LEXER`); also component-class assignment rules | T2 |
| `common/drc_rules.keywords` | 76 | keyword list -> generated `drc_rules_lexer.h` (DSNLEXER) | T2 |
| `pcbnew/drc/drc_item.h` / `.cpp` | 362 / 778 | `PCB_DRC_CODE`, `DRC_ITEM` (titles, settings keys, ordered list), `DRC_ITEMS_PROVIDER`, `ConvertPadCheckToDrcCode`, `GetViolationLayers` (GUI) | T1 |
| `include/rc_item.h`, `common/rc_item.cpp` | 437 / 853 | `RC_ITEM` base: ids, `ShowReport`, `GetJsonViolation`; `RC_TREE_MODEL` (L202-853) is GUI | T1 (L37-200 only) |
| `include/rc_json_schema.h` | 150 | JSON report structs | T1 |
| `resources/schemas/drc.v1.json` | - | JSON schema of the report | reference |
| `include/widgets/report_severity.h`, `common/widgets/report_severity.cpp` | 47 | `SEVERITY` bit values, `formatSeverities` | T1 |
| `include/marker_base.h`, `pcbnew/pcb_marker.h/.cpp` | 158 / 180 / 569 | marker: marker type from error code (ctor L52-93), `GetSeverity` (L411-428), exclusion flag/comment, proto (de)serialisation for exclusions (L103-221), legacy exclusion strings (L224-292) | T1 (drawing/GUI parts OUT) |
| `pcbnew/drc/drc_exclusion.h` / `.cpp` | 87 / 178 | exclusion identity (protobuf `DrcMarker` + comment), JSON form in `.kicad_pro`, sort key | T1 (without protobuf, see 6.4) |
| `pcbnew/drc/drc_report.h` / `.cpp` | 51 / 226 | text and JSON report writers | T1 |
| `pcbnew/drc/drc_rtree.h` | 729 | `DRC_RTREE`: per-layer packed R-trees of (item, sub-shape), collision queries | T1 |
| `pcbnew/drc/drc_zone_index.h` | 172 | `CollectOverlappingPairs`, `DRC_ZONE_INDEX` (per-layer zone bbox index with ordered results) | T1 |
| `pcbnew/drc/drc_cache_generator.h` / `.cpp` | 44 / 331 | pseudo-provider run first by `RunTests`: worst clearances, zone lists, copper item R-tree, zone R-trees, courtyard/net-tie caches, connectivity rebuild, isolated islands | T1 |
| `pcbnew/drc/drc_test_provider.h` / `.cpp` | 172 / 466 | provider base: registry, `forEachGeometryItem`, `reportViolation`, two-point/two-shape marker geometry, `formatMsg` | T1 |
| `pcbnew/drc/drc_length_report.h` | 78 | per-net length report for the matched-length provider | T3 |
| `pcbnew/drc/drc_chain_topology.h` / `.cpp` | 97 / 830 | net-chain trunk/stub topology (matched length) | T3 |
| `pcbnew/drc/drc_creepage_utils.h` / `.cpp`, `drc_creepage_engine.h` / `.cpp` | 919 / 3185 / 165 / 345 | creepage graph | T3 |
| `pcbnew/drc/drc_interactive_courtyard_clearance.h` / `.cpp` | 63 / 289 | interactive (move tool) courtyard check | OUT (GUI) |
| `pcbnew/drc/rule_editor/*` (82 files) | - | graphical rule editor | OUT (GUI) |

### 2.2 Expression language

| File | Lines | Role | Scope |
|---|---|---|---|
| `common/libeval_compiler/grammar.lemon` | 221 | Lemon grammar (precedence table!) | `kicad-libeval` |
| `common/libeval_compiler/libeval_compiler.cpp`, `include/libeval_compiler/libeval_compiler.h` | 1501 / 713 | tokenizer, tree -> ucode compiler, `VALUE`, `UOP::Exec`, `UCODE::Run`, `EscapeStringLiteral` | `kicad-libeval` |
| `pcbnew/pcbexpr_evaluator.h` / `.cpp` | 354 / 969 | PCB binding: `PCBEXPR_CONTEXT` (items A/B, layer, constraint), `PCBEXPR_VAR_REF` (property access through `PROPERTY_MANAGER`), special value classes (layer, netclass, net, component class, pin type), unit resolvers, `PCBEXPR_EVALUATOR` (constant expressions in `(min ...)`) | T2 |
| `pcbnew/pcbexpr_functions.cpp` | 1966 | built-in functions (`intersectsArea`, `hasNetclass`, ...) and their board-level caches | T2 (the keepout part L553-959 is needed in T1) |

### 2.3 Test providers (`pcbnew/drc/drc_test_provider_*.cpp`)

Run order = registration order = order of the sources in `PCBNEW_DRC_SRCS`
(`pcbnew/CMakeLists.txt:L328-364`), see 4.5. `GetName()` in quotes.

| # | File | Lines | Name | Error codes | Scope |
|---|---|---|---|---|---|
| 1 | `..._annular_width.cpp` | 465 | "annular_width" | `annular_width` | T1 |
| 2 | `..._disallow.cpp` | 399 | "disallow" | `items_not_allowed`, `text_on_edge_cuts` | T1 |
| 3 | `..._creepage.cpp` | 442 | "creepage" | `creepage` | T3 |
| 4 | `..._connectivity.cpp` | 333 | "connectivity" | `track_dangling`, `via_dangling`, `isolated_copper`, `unconnected_items`, `track_on_post_machined_layer`, `track_not_centered_on_via` | T1 |
| 5 | `..._connection_width.cpp` | 587 | "copper width" | `connection_width` | T2 |
| 6 | `..._copper_clearance.cpp` | 1514 | "clearance" | `clearance`, `hole_clearance`, `shorting_items`, `tracks_crossing` | T1 |
| 7 | `..._physical_clearance.cpp` | 874 | "physical_clearance" | `clearance`, `hole_clearance` (physical rules reuse these codes) | T2 |
| 8 | `..._courtyard_clearance.cpp` | 401 | "courtyard_clearance" | `courtyards_overlap`, `missing_courtyard`, `malformed_courtyard`, `pth_inside_courtyard`, `npth_inside_courtyard` | T2 |
| 9 | `..._edge_clearance.cpp` | 653 | "edge_clearance" | `copper_edge_clearance`, `silk_edge_clearance`, `copper_outside_board` | T1 |
| 10 | `..._footprint_checks.cpp` | 153 | "footprint checks" | `footprint_type_mismatch`, `footprint_scaled_with_pads`, `through_hole_pad_without_hole`, `padstack`, `padstack_invalid`, `shorting_items`, `footprint` | T2 |
| 11 | `..._hole_to_hole.cpp` | 317 | "hole_to_hole_clearance" | `hole_to_hole`, `holes_co_located` | T1 |
| 12 | `..._hole_size.cpp` | 262 | "hole_size" | `drill_out_of_range`, `microvia_drill_out_of_range` | T1 |
| 13 | `..._library_parity.cpp` | 1303 | "library_parity" | `lib_footprint_issues`, `lib_footprint_mismatch` | OUT (stub, see 4.10.25) |
| 14 | `..._schematic_parity.cpp` | 410 | "schematic_parity" | parity codes | OUT |
| 15 | `..._misc.cpp` | 718 | "miscellaneous" | `invalid_outline`, `item_on_disabled_layer`, `unresolved_variable`, `assertion_failure`, `generic_warning`, `generic_error`, `missing_tuning_profile` | T2 (first T2 package: outline test) |
| 16 | `..._net_chain_tuning_profiles.cpp` | 102 | "net chain tuning profiles" | `net_chain_tuning_profiles` | T3 |
| 17 | `..._text_dims.cpp` | 286 | "text_dimensions" | `text_height`, `text_thickness` | T2 |
| 18 | `..._track_angle.cpp` | 195 | "track angle" | `track_angle` | T2 |
| 19 | `..._track_width.cpp` | 164 | "width" | `track_width` | T1 |
| 20 | `..._track_segment_length.cpp` | 171 | "segment_length" | `track_segment_length` | T2 |
| 21 | `..._zone_connections.cpp` | 371 | "zone connections" | `starved_thermal` | T2 |
| 22 | `..._via_diameter.cpp` | 192 | "diameter" | `via_diameter` | T1 |
| 23 | `..._via_stack.cpp` | 467 | "microvia_stack" | microvia stack codes, `hole_aspect_ratio`, `microvia_crosses_core` | T2 |
| 24 | `..._via_stitch.cpp` | 107 | "via_stitch_overlap" | `via_stitch_overlap` | T3 |
| 25 | `..._solder_mask.cpp` | 1090 | "solder_mask_issues" | `solder_mask_bridge`, `silk_over_copper` | T2 |
| 26 | `..._silk_clearance.cpp` | 334 | "silk_clearance" | `silk_overlap`, `silk_over_copper` | T2 |
| 27 | `..._matched_length.cpp` | 1032 | "length" | `length_out_of_range`, `skew_out_of_range`, `too_many_vias`, net-chain codes | T3 |
| 28 | `..._diff_pair_coupling.cpp` | 778 | "diff_pair_coupling" | `diff_pair_gap_out_of_range`, `diff_pair_uncoupled_length_too_long` | T3 |
| 29 | `..._sliver_checker.cpp` | 287 | "sliver checker" | `copper_sliver` | T2 |
| 30 | `..._text_mirroring.cpp` | 133 | "text_mirroring" | `mirrored_text_on_front_layer`, `nonmirrored_text_on_back_layer` | T2 |

(The provider name is only used in log-reporter output, which the CLI does not enable, and by
`DRC_ENGINE::GetTestProvider( name )`.)

---

## 3. Data structures

### 3.1 Constraint kinds (`drc_constraint_types.h`)

`DRC_CONSTRAINT_T` (declaration order matters only as the `std::map` key order of
`m_constraintMap`, which does not influence results): `NULL`, `CLEARANCE`, `CREEPAGE`,
`HOLE_CLEARANCE`, `HOLE_TO_HOLE`, `EDGE_CLEARANCE`, `HOLE_SIZE`, `COURTYARD_CLEARANCE`,
`SILK_CLEARANCE`, `TEXT_HEIGHT`, `TEXT_THICKNESS`, `TRACK_WIDTH`, `TRACK_SEGMENT_LENGTH`,
`ANNULAR_WIDTH`, `ZONE_CONNECTION`, `THERMAL_RELIEF_GAP`, `THERMAL_SPOKE_WIDTH`,
`MIN_RESOLVED_SPOKES`, `SOLDER_MASK_EXPANSION`, `SOLDER_PASTE_ABS_MARGIN`,
`SOLDER_PASTE_REL_MARGIN`, `DISALLOW`, `VIA_DIAMETER`, `LENGTH`, `NET_CHAIN_LENGTH`,
`NET_CHAIN_STUB_LENGTH`, `NET_CHAIN_RETURN_PATH`, `SKEW`, `DIFF_PAIR_GAP`, `MAX_UNCOUPLED`,
`DIFF_PAIR_INTRA_SKEW`, `VIA_COUNT`, `PHYSICAL_CLEARANCE`, `PHYSICAL_HOLE_CLEARANCE`,
`ASSERTION`, `CONNECTION_WIDTH`, `TRACK_ANGLE`, `VIA_DANGLING`, `BRIDGED_MASK`,
`SOLDER_MASK_SLIVER`, `MICROVIA_STACK_DEPTH`, `HOLE_ASPECT_RATIO` (all with the `_CONSTRAINT`
suffix). The numeric value is passed into expressions as the "constraint under test" and is a
cache key component.

`DRC_DISALLOW_T` bit flags: `THROUGH_VIAS` 1, `MICRO_VIAS` 2, `BLIND_VIAS` 4, `BURIED_VIAS` 8,
`TRACKS` 16, `PADS` 32, `ZONES` 64, `TEXTS` 128, `GRAPHICS` 256, `HOLES` 512, `FOOTPRINTS` 1024.
`DRC_DISALLOW_VIAS == DRC_DISALLOW_THROUGH_VIAS` (!), `DRC_DISALLOW_BB_VIAS = BLIND|BURIED`.

`DRC_IMPLICIT_SOURCE`: `NONE`, `BOARD_SETUP_CONSTRAINT`, `BARCODE_DEFAULTS`, `KEEPOUT`,
`NET_CLASS`, `TUNING_PROFILE`.

### 3.2 `MINOPTMAX<int>` (`libs/core/include/core/minoptmax.h`)

Three optional ints. `Min()` returns 0 when unset, `Max()` returns `INT_MAX` when unset,
`Opt()` returns `Min()` when unset. `HasMin/HasOpt/HasMax`. Port exactly (belongs in
`kicad-core`); many providers rely on `Min()==0` for "no constraint".

### 3.3 `DRC_CONSTRAINT` and `DRC_RULE` (`drc_rule.h`)

`DRC_CONSTRAINT`: `m_Type`, `m_Value: MINOPTMAX<int>`, `m_DisallowFlags`, `m_ZoneConnection`
(`ZONE_CONNECTION::INHERITED` default), `m_Test` (assertion expression), `m_ImplicitMin`,
`m_ReferenceLayer`, `m_ReferenceNet` (return-path constraint), private `m_name` (label for
constraints that do not come from a rule, e.g. "board minimum"), `m_parentRule` (last rule
applied), `m_minRule/m_optRule/m_maxRule` (which rule supplied each value), option bitset
(`SKEW_WITHIN_DIFF_PAIRS`, `SPACE_DOMAIN`, `TIME_DOMAIN`).

* `GetName()` = `GetRuleName( m_parentRule )`: implicit rule -> `rule->m_Name`; explicit rule ->
  `rule '<name>'`; no rule -> `m_name`. This string is what appears in violation messages
  (`(netclass 'Default' clearance 0.2000 mm; actual 0.1500 mm)`).
* `GetSeverity()` = parent rule's `m_Severity`, or `RPT_SEVERITY_UNDEFINED` without a rule.
* `SetParentRule( r )` also resets min/opt/max rule to `r` for the values present.

`DRC_RULE`: `m_Unary`, `m_ImplicitItemId` (KIID), `m_ImplicitItem` (pointer to the keepout zone),
`m_Name`, `m_ImplicitNetclass`, `m_LayerSource` (text of the `(layer ...)` clause),
`m_LayerCondition: LSET` (default `LSET::AllLayersMask()`), `m_Condition`
(`DRC_RULE_CONDITION*`, may be null), `m_Constraints: Vec<DRC_CONSTRAINT>`, `m_Severity`
(default `RPT_SEVERITY_UNDEFINED`), implicit source. In Rust: rules live in a `Vec<DrcRule>`
owned by the engine; constraints refer to rules by index (`RuleId`), never by pointer.

### 3.4 `DRC_ENGINE` state (`drc_engine.h:L370-410`)

* `m_rules: Vec<Rc<DRC_RULE>>` in creation order: implicit rules first (order of 4.3), then file
  rules in file order.
* `m_constraintMap: map<DRC_CONSTRAINT_T, Vec<DRC_ENGINE_CONSTRAINT>>`: per constraint type, one
  entry per (rule, constraint) **in rule order**. `DRC_ENGINE_CONSTRAINT` = `layerTest` (copy
  of the rule's layer set), `condition` (null when the rule has none or it is empty),
  `parentRule`, `constraint` (copy), `implicitKeepoutZone` (set for implicit disallow rules whose
  `m_ImplicitItem` is a zone).
* `m_explicitConstraints`: same, restricted to non-implicit rules (only used by the
  `Has...Constraint` queries).
* `m_netclassClearances: unordered_map<netclass name, rule>`: implicit netclass clearance rule
  per netclass (fast path of `EvalRules`); lookups only, never iterated.
* Flags: `m_rulesValid`, `m_hasExplicitClearanceRules`, `m_hasGeometryDependentRules`,
  `m_hasDiffPairClearanceOverrides`, `m_reportAllTrackErrors`, `m_testFootprints`.
* `m_errorLimits: Vec<int>` indexed by error code (size `DRCE_LAST + 1`).
* `m_testProviders`: copied from the global registry in `InitEngine`.
* Handlers: violation handler, log reporter (null in the CLI), progress reporter.
* `m_ownClearanceCache` (uuid, layer) -> clearance: rendering optimisation; never iterated.
* The engine is also a `UNITS_PROVIDER` (IU scale + user units) used to format values in the
  (reporter-only) explanation texts.

### 3.5 Violations: `RC_ITEM` / `DRC_ITEM` / `PCB_MARKER`

`RC_ITEM` (`rc_item.h:L139-`): `m_errorCode`, `m_errorTitle`, `m_settingsKey`,
`m_errorMessage` (empty -> the title is used), `m_ids: Vec<KIID>` (0..4 item ids), `m_parent`
(marker).

* `SetErrorDetail( msg )` = `SetErrorMessage( title + " " + msg )` - this is the normal way
  providers attach `(... clearance X; actual Y)`.
* `SetItems( const EDA_ITEM* a, b, c, d )` pushes only the non-null ids;
  `SetItems( KIID a, KIID b = nil, c = nil, d = nil )` always pushes four ids (nil padded). The
  report only looks at ids 0 and 1 and treats nil as absent, but the exclusion identity uses the
  non-nil ids in order.
* `DRC_ITEM` adds `m_violatingRule` (the rule named in the report, null -> "Local override") and
  `m_violatingTest`. `GetViolatingRuleDesc( false )` = `Rule: <rule->m_Name>` or
  `Local override`.

`PCB_MARKER( item, pos, layer )` (`pcb_marker.cpp:L52-93`): marker type from the error code:
`DRCE_UNCONNECTED_ITEMS` -> `MARKER_RATSNEST`; `MISSING_FOOTPRINT`, `DUPLICATE_FOOTPRINT`,
`EXTRA_FOOTPRINT`, `NET_CONFLICT`, `SCHEMATIC_PARITY`, `SCHEMATIC_FIELDS_PARITY`,
`FOOTPRINT_FILTERS` -> `MARKER_PARITY`; layer `LAYER_DRAWINGSHEET` -> `MARKER_DRAWING_SHEET`;
everything else `MARKER_DRC`. Marker layer = the layer passed by the provider
(`UNDEFINED_LAYER` allowed).

`PCB_MARKER::GetSeverity()` (L411-428), in this order: excluded -> `RPT_SEVERITY_EXCLUSION`;
`DRCE_GENERIC_WARNING` -> warning; `DRCE_GENERIC_ERROR` -> error; violating rule with a severity
other than `UNDEFINED` -> that severity; else `bds.GetSeverity( code )`.
`IsTreatedAsExcluded()` = excluded flag or severity == exclusion (a rule with
`(severity exclusion)`).

The marker "path" (`SetPath`, set through the path generator closure of `ReportViolation`) is
only drawn in the GUI. Keep the two points in the Rust violation record (cheap, useful for the
wasm demo) but nothing in the reports depends on them.

### 3.6 Error codes (`drc_item.h:L45-145`, `drc_item.cpp:L86-492`)

Enum order (numeric value = position, `DRCE_FIRST = 1`). The numeric value is never written to
a file; files use the settings key. Default severity from `BOARD_DESIGN_SETTINGS` ctor
(`pcbnew/board_design_settings.cpp:L169-219`): everything `error` unless listed.

| # | Enum (`DRCE_`) | Settings key | Title | Default severity |
|---|---|---|---|---|
| 1 | UNCONNECTED_ITEMS | `unconnected_items` | Missing connection between items | error |
| 2 | SHORTING_ITEMS | `shorting_items` | Items shorting two nets | error |
| 3 | ALLOWED_ITEMS | `items_not_allowed` | Items not allowed | error |
| 4 | TEXT_ON_EDGECUTS | `text_on_edge_cuts` | Text or graphic on Edge.Cuts layer | error |
| 5 | CLEARANCE | `clearance` | Clearance violation | error |
| 6 | CREEPAGE | `creepage` | Creepage violation | error |
| 7 | TRACKS_CROSSING | `tracks_crossing` | Tracks crossing | error |
| 8 | EDGE_CLEARANCE | `copper_edge_clearance` | Board edge clearance violation | error |
| 9 | ZONES_INTERSECT | (no item defined; `Create` returns null) | - | error |
| 10 | ISOLATED_COPPER | `isolated_copper` | Isolated copper fill | warning |
| 11 | STARVED_THERMAL | `starved_thermal` | Thermal relief connection to zone incomplete | error |
| 12 | DANGLING_VIA | `via_dangling` | Via is not connected or connected on only one layer | warning |
| 13 | DANGLING_TRACK | `track_dangling` | Track has unconnected end | warning |
| 14 | DRILLED_HOLES_TOO_CLOSE | `hole_to_hole` | Drilled hole too close to other hole | warning |
| 15 | DRILLED_HOLES_COLOCATED | `holes_co_located` | Drilled holes co-located | warning |
| 16 | HOLE_CLEARANCE | `hole_clearance` | Hole clearance violation | error |
| 17 | TRACK_WIDTH | `track_width` | Track width | error |
| 18 | TRACK_ANGLE | `track_angle` | Track angle | error |
| 19 | TRACK_SEGMENT_LENGTH | `track_segment_length` | Track segment length | error |
| 20 | ANNULAR_WIDTH | `annular_width` | Annular width | error |
| 21 | CONNECTION_WIDTH | `connection_width` | Copper connection too narrow | warning |
| 22 | DRILL_OUT_OF_RANGE | `drill_out_of_range` | Hole size out of range | error |
| 23 | VIA_DIAMETER | `via_diameter` | Via diameter | error |
| 24 | PADSTACK | `padstack` | Padstack is questionable | warning |
| 25 | PADSTACK_INVALID | `padstack_invalid` | Padstack is not valid | error (not user-editable) |
| 26 | MICROVIA_DRILL_OUT_OF_RANGE | `microvia_drill_out_of_range` | Micro via hole size out of range | error |
| 27 | MALFORMED_MICROVIA_STACK_SPAN | `malformed_microvia_stack_span` | Microvia stack has malformed span | error |
| 28 | MICROVIA_STACK_NOT_FILLED | `microvia_stack_not_filled` | Microvia under microvia not filled | warning |
| 29 | MICROVIA_STACK_DEPTH | `microvia_stack_depth` | Microvia stack too deep | error |
| 30 | HOLE_ASPECT_RATIO | `hole_aspect_ratio` | Hole too deep for its diameter | error |
| 31 | MICROVIA_CROSSES_CORE | `microvia_crosses_core` | Microvia crosses core material | warning |
| 32 | OVERLAPPING_FOOTPRINTS | `courtyards_overlap` | Courtyards overlap | error |
| 33 | MISSING_COURTYARD | `missing_courtyard` | Footprint has no courtyard defined | **ignore** |
| 34 | MALFORMED_COURTYARD | `malformed_courtyard` | Footprint has malformed courtyard | error |
| 35 | PTH_IN_COURTYARD | `pth_inside_courtyard` | PTH inside courtyard | error |
| 36 | NPTH_IN_COURTYARD | `npth_inside_courtyard` | NPTH inside courtyard | error |
| 37 | DISABLED_LAYER_ITEM | `item_on_disabled_layer` | Item on a disabled copper layer | error |
| 38 | INVALID_OUTLINE | `invalid_outline` | Board has malformed outline | error |
| 39 | MISSING_FOOTPRINT | `missing_footprint` | Missing footprint | warning |
| 40 | DUPLICATE_FOOTPRINT | `duplicate_footprints` | Duplicate footprints | warning |
| 41 | EXTRA_FOOTPRINT | `extra_footprint` | Extra footprint | warning |
| 42 | NET_CONFLICT | `net_conflict` | Pad net doesn't match schematic | warning |
| 43 | SCHEMATIC_PARITY | `footprint_symbol_mismatch` | Footprint attributes don't match symbol | warning |
| 44 | FOOTPRINT_FILTERS | `footprint_filters_mismatch` | Footprint doesn't match symbol's footprint filters | **ignore** |
| 45 | FOOTPRINT_TYPE_MISMATCH | `footprint_type_mismatch` | Footprint component type doesn't match footprint pads | **ignore** |
| 46 | LIB_FOOTPRINT_ISSUES | `lib_footprint_issues` | Footprint not found in libraries | warning |
| 47 | LIB_FOOTPRINT_MISMATCH | `lib_footprint_mismatch` | Footprint doesn't match copy in library | warning |
| 48 | PAD_TH_WITH_NO_HOLE | `through_hole_pad_without_hole` | Through hole pad has no hole | error |
| 49 | FOOTPRINT | `footprint` | Footprint is not valid | error |
| 50 | FOOTPRINT_SCALED_WITH_PADS | `footprint_scaled_with_pads` | Footprint with pads is scaled (physical part size unchanged) | warning |
| 51 | UNRESOLVED_VARIABLE | `unresolved_variable` | Unresolved text variable | error |
| 52 | ASSERTION_FAILURE | `assertion_failure` | Assertion failure | error |
| 53 | GENERIC_WARNING | `generic_warning` | Warning | always warning (not user-editable) |
| 54 | GENERIC_ERROR | `generic_error` | Error | always error (not user-editable) |
| 55 | COPPER_SLIVER | `copper_sliver` | Copper sliver | warning |
| 56 | SOLDERMASK_BRIDGE | `solder_mask_bridge` | Solder mask aperture bridges items with different nets | error |
| 57 | SILK_MASK_CLEARANCE | `silk_over_copper` | Silkscreen clipped by solder mask | warning |
| 58 | SILK_EDGE_CLEARANCE | `silk_edge_clearance` | Silkscreen clipped by board edge | warning |
| 59 | SILK_CLEARANCE | `silk_overlap` | Silkscreen clearance | warning |
| 60 | TEXT_HEIGHT | `text_height` | Text height out of range | warning |
| 61 | TEXT_THICKNESS | `text_thickness` | Text thickness out of range | warning |
| 62 | LENGTH_OUT_OF_RANGE | `length_out_of_range` | Track length out of range | error |
| 63 | NET_CHAIN_STUB_TOO_LONG | `net_chain_stub_length` | Net chain stub length out of range | error |
| 64 | NET_CHAIN_RETURN_PATH_BREAK | `net_chain_return_path` | Net chain routed without continuous copper on the required reference layer | error |
| 65 | NET_CHAIN_TUNING_PROFILES | `net_chain_tuning_profiles` | Net chain nets do not have consistent tuning profile assignment | error |
| 66 | SKEW_OUT_OF_RANGE | `skew_out_of_range` | Skew between tracks out of range | error |
| 67 | VIA_COUNT_OUT_OF_RANGE | `too_many_vias` | Too many or too few vias on a connection | error |
| 68 | DP_GAP_OUT_OF_RANGE | `diff_pair_gap_out_of_range` | Differential pair gap out of range | error |
| 69 | DP_UNCOUPLED_LENGTH_TOO_LONG | `diff_pair_uncoupled_length_too_long` | Differential uncoupled length too long | error |
| 70 | MIRRORED_TEXT_ON_FRONT_LAYER | `mirrored_text_on_front_layer` | Mirrored text on front layer | warning |
| 71 | UNMIRRORED_TEXT_ON_BACK_LAYER | `nonmirrored_text_on_back_layer` | Non-Mirrored text on back layer | warning |
| 72 | MISSING_TUNING_PROFILE | `missing_tuning_profile` | Missing tuning profile | warning |
| 73 | TUNING_PROFILE_IMPLICIT_RULES | `tuning_profile_track_geometries` | Tuning profile track geometries (pseudo code: only a severity) | **ignore** |
| 74 | TRACK_ON_POST_MACHINED_LAYER | `track_on_post_machined_layer` | Track connected to post-machined or backdrilled layer | error |
| 75 | TRACK_NOT_CENTERED_ON_VIA | `track_not_centered_on_via` | Track endpoint not centered on via | **ignore** |
| 76 | SCHEMATIC_FIELDS_PARITY | `footprint_symbol_field_mismatch` | Footprint field does not match symbol field | warning |
| 77 | VIA_STITCH_OVERLAP | `via_stitch_overlap` | Via-stitching zones overlap for same net | error |
| 78 | COPPER_OUTSIDE_BOARD | `copper_outside_board` | Copper outside board outline | warning |

`DRC_ITEM::Create( code )` has no case for 9 and 73 (returns null).

**`DRC_ITEM::allItemTypes` order** (`drc_item.cpp:L399-492`) is output-relevant: the "ignored
checks" section of both reports and the `rule_severities` object iterate
`GetItemsWithSeverities()` = this list up to (not including) the "internal" heading; headings
have code 0 and are skipped. Order of settings keys:

```
shorting_items, tracks_crossing, clearance, creepage, via_dangling, track_dangling,
starved_thermal,
copper_edge_clearance, copper_outside_board, hole_clearance, hole_to_hole, holes_co_located,
track_width, track_angle, track_segment_length, annular_width, drill_out_of_range,
microvia_drill_out_of_range, malformed_microvia_stack_span, microvia_stack_not_filled,
microvia_stack_depth, hole_aspect_ratio, microvia_crosses_core, via_diameter,
courtyards_overlap, missing_courtyard, malformed_courtyard, invalid_outline, copper_sliver,
solder_mask_bridge, connection_width, track_on_post_machined_layer, track_not_centered_on_via,
via_stitch_overlap, tuning_profile_track_geometries,
duplicate_footprints, missing_footprint, extra_footprint, footprint_symbol_mismatch,
footprint_symbol_field_mismatch, footprint_filters_mismatch, net_conflict, unconnected_items,
length_out_of_range, net_chain_stub_length, net_chain_return_path, net_chain_tuning_profiles,
skew_out_of_range, too_many_vias, diff_pair_gap_out_of_range,
diff_pair_uncoupled_length_too_long,
silk_overlap, silk_over_copper, silk_edge_clearance, text_height, text_thickness,
mirrored_text_on_front_layer, nonmirrored_text_on_back_layer,
items_not_allowed, text_on_edge_cuts, isolated_copper, footprint, padstack,
pth_inside_courtyard, npth_inside_courtyard, item_on_disabled_layer, unresolved_variable,
assertion_failure, footprint_type_mismatch, lib_footprint_issues, lib_footprint_mismatch,
through_hole_pad_without_hole, footprint_scaled_with_pads, missing_tuning_profile
-- internal (no user severity): padstack_invalid, generic_error, generic_warning
```

`SEVERITY` bit values (`report_severity.h`): `UNDEFINED 0x01`, `INFO 0x02`, `EXCLUSION 0x04`,
`ACTION 0x08`, `WARNING 0x10`, `ERROR 0x20`, `IGNORE 0x40`, `DEBUG 0x80`. The numeric order
is used for sorting (error before warning before exclusion).

### 3.7 `DRC_RTREE` (`drc_rtree.h`)

Non-owning spatial index. For each layer a `KIRTREE::PACKED_RTREE<ITEM_WITH_SHAPE*, int, 2>`
(static Hilbert-packed R-tree, `libs/kimath/include/geometry/rtree/packed_rtree.h`).
`ITEM_WITH_SHAPE { parent: item, shape: sub-shape, shapeStorage, parentShape: whole effective
shape }`.

* `Insert( item, refLayer, targetLayer, constraintType, worstClearance, atomicTables )` (L104-163):
  invisible `PCB_FIELD` skipped; `shape = item->GetEffectiveShape( refLayer, FLASHING::DEFAULT,
  constraintType )`; if `shape->HasIndexableSubshapes()` one entry per sub-shape (compound
  shapes, triangulated zone fills) else one entry; `SHAPE_NULL` sub-shapes skipped; bbox inflated
  by `worstClearance`; **in addition** a pad with a hole gets an entry for its
  `GetEffectiveHoleShape()`. With `atomicTables`, a table cell is entered with its table as
  parent. Entries are staged in per-layer builders (`std::map<int, Builder>`); `Build()`
  finalises; inserting after `Build()` is a programming error. `m_minPadding[layer]` = minimum
  padding of all entries of the layer.
* `QueryColliding( refItem, refLayer, targetLayer, filter, visitor, clearance )` (L231-300):
  query box = `refItem->GetBoundingBox()` merged with the ref shape bbox, inflated; per entry:
  skip own item; skip items already reported colliding; ask `filter` once per item (result
  memoised); narrow phase `refShape->Collide( entry.shape, clearance )`; on hit call
  `visitor( item )`; visitor returning false stops the search. Returns the hit count.
* `QueryCollidingPreparedCopper( refItem, refShape, targetLayer, filter, visitor, clearance,
  usePaddingCredit )` (L312-383): same, but the caller supplies the shape, the visitor receives
  the tree-owned parent shape, and with padding credit the query box is inflated only by
  `max( 0, clearance - m_minPadding[layer] )` (valid only when the item bbox contains the shape
  bbox, otherwise full inflation).
* `QueryColliding( box, refShape, layer, clearance, &actual, &pos )` (L391-443): minimum
  `actual` over all colliding sub-shapes of a single-item tree (zone fill triangles); stops early
  once `actual <= 0`; returns `max( 0, actual )` and the position of the minimum (first minimum
  in search order wins: strict `<`).
* `QueryColliding( box, refShape, layer )` (L448-516): yes/no; special case for a single-outline
  polygon without holes against fill triangles (segment/triangle tests + point-inside).
* `CheckColliding`, `GetObjectsAt( pt, layer, clearance )` (returns an `unordered_set` - see
  4.12), `QueryCollidingPairs( refTree, layerPairs, visitor, maxClearance, progress )`
  (L563-636: collects all bbox-overlapping pairs first, in order layerPair -> ref item (tree
  iteration order) -> search order; then visits them, skipping parent pairs already flagged as
  colliding; the dedup key is the unordered pointer pair), `OnLayer( layer )` (all entries of a
  layer in tree storage order), `Overlapping(...)`.

**The search order of `PACKED_RTREE` is result-relevant** (first hit stops many loops). The
tree sorts entries with `std::sort` on a 64-bit Hilbert value of the box centre
(`packed_rtree.h:L258-268`); entries with equal centres (a pad and its hole, sub-shapes) are in
`std::sort` tie order. See risk R2.

### 3.8 Board-level run-time caches used by DRC (`pcbnew/board.h:L1780-1840`)

All cleared by `BOARD::IncrementTimeStamp()` (`board.cpp:L446-512`). In Rust put them in one
`DrcBoardCaches` value created per `RunTests` (and per `InitEngine` for the predicate caches)
instead of in the board.

| Cache | Type | Built by | Used by |
|---|---|---|---|
| `m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance` | int | cache generator | broad-phase radius |
| `m_DRCZones`, `m_DRCCopperZones` | `Vec<ZONE*>` in board order (board zones, then footprint zones in footprint order), rule areas excluded | cache generator | many providers |
| `m_DRCCopperZonesByLayer` | `map<layer, Vec<ZONE*>>` | cache generator | `DRC_ZONE_INDEX` |
| `m_CopperItemRTreeCache` | `DRC_RTREE` of all copper items | cache generator | copper clearance, connection width |
| `m_CopperZoneRTreeCache` | `unordered_map<ZONE*, DRC_RTREE>` (lookups only) | cache generator | zone collision tests, keepouts |
| `m_ZoneIsolatedIslandsMap` | `std::map<ZONE*, map<layer, ISOLATED_ISLANDS>>` - **keyed by pointer and iterated** | cache generator + connectivity | connectivity provider, zone connections |
| `m_Intersects*Cache`, `m_EnclosedByAreaCache`, `m_*ResultCache`, `m_ItemFieldCache` | sharded hash maps keyed by item pointers (+layer / selector / constraint) | expression functions | expression functions (semantically relevant, see 4.8.4) |
| `m_LayerExpressionCache` | layer pattern -> LSET | expression layer compare | same |
| `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`, `m_StackedMicroviaCache`, `m_footprintCourtyardIndex` | lookups | expression functions | same |
| `m_maxClearanceValue` | optional int | `BOARD::GetMaxClearanceValue` | cache generator |
| `m_SolderMaskBridges` | a scratch `ZONE` holding the whole-board mask | solder mask provider | solder mask, silk |
| Footprint courtyard cache, net-tie cache | per footprint | `BuildCourtyardCaches`, `BuildNetTieCache` (cache generator) | courtyard, EvalRules net-tie |

### 3.9 Provider registry

Each provider `.cpp` ends with `namespace detail { static DRC_REGISTER_TEST_PROVIDER<T> dummy; }`
which appends a heap instance to `DRC_TEST_PROVIDER_REGISTRY::Instance()` during static
initialisation. Providers are therefore **process-wide singletons**; members that are not reset
in `Run()` survive between runs (e.g. `m_largestCourtyardClearance`, `m_largestEdgeClearance`).
In Rust: a fixed array of provider constructors in the order of 4.5, fresh state per run
(document the theoretical difference in `PORT_STATUS.md`; it only matters when a query that
"usually succeeds" fails on a later run in the same process).

---

## 4. Control flow and algorithms

### 4.1 `InitEngine( rulePath )` (`drc_engine.cpp:L858-933`)

1. `m_testProviders` = registry copy; `provider->SetDRCEngine( this )`.
2. Null the violating-rule pointer of all existing markers (they would dangle).
3. Clear rules, `m_rulesValid = false`, constraint map, own-clearance cache, netclass clearance
   map, the three flags, `m_explicitConstraints`; `m_board->IncrementTimeStamp()`.
4. `try { loadImplicitRules(); loadRules( path ); compileRules(); }` - on `PARSE_ERROR`: clear the
   rules, load + compile the implicit rules only, then **rethrow** (so `m_rulesValid` stays
   false, but `EvalRules` still works with the implicit rules).
5. Reset all error limits to 199, `m_rulesValid = true`.

`InitEngine( shared_ptr<DRC_RULE> )` (L797-855) is the GUI "show matches" variant (single rule,
other provider registry); only `test_drc_error_limit.cpp` uses it to reset the limits. OUT, but
keep a `reset_error_limits()`.

### 4.2 Implicit rules: `loadImplicitRules()` (`drc_engine.cpp:L163-679`)

`bds` = board design settings. Rule names are passed through `_()`; the CLI runs in English.
Rules are appended to `m_rules` in exactly this order (rule order = evaluation order, later
rules override earlier ones, see 4.6):

| # | Rule name | Source | Condition | Layers | Constraints |
|---|---|---|---|---|---|
| 1 | `board setup constraints` | BOARD_SETUP | none | all | `TRACK_WIDTH` min=`m_TrackMinWidth`; `CONNECTION_WIDTH` min=`m_MinConn`; `HOLE_SIZE` min=`m_MinThroughDrill`; `ANNULAR_WIDTH` min=`m_ViasMinAnnularWidth`; `VIA_DIAMETER` min=`m_ViasMinSize`; `HOLE_TO_HOLE` min=`m_HoleToHoleMin` |
| 2 | `board setup constraints zone fill strategy` | BOARD_SETUP | none | all | `MIN_RESOLVED_SPOKES` min=`m_MinResolvedSpokes` |
| 3 | `board setup constraints silk` | BOARD_SETUP | none | F.SilkS, B.SilkS | `SILK_CLEARANCE` min=`m_SilkClearance` |
| 4 | `board setup constraints silk text height` | BOARD_SETUP | none | F.SilkS, B.SilkS | `TEXT_HEIGHT` min=`m_MinSilkTextHeight` |
| 5 | `board setup constraints silk text thickness` | BOARD_SETUP | none | F.SilkS, B.SilkS | `TEXT_THICKNESS` min=`m_MinSilkTextThickness` |
| 6 | `board setup constraints hole` | BOARD_SETUP | none | all | `HOLE_CLEARANCE` min=`m_HoleClearance` |
| 7 | `board setup constraints edge` | BOARD_SETUP | none | all | `EDGE_CLEARANCE` min=`m_CopperEdgeClearance` |
| 8 | `board setup constraints courtyard` | BOARD_SETUP | none | all | `COURTYARD_CLEARANCE` with **no value at all** (L233-236: the `SetMin( 0 )` is applied to the wrong local variable after it was copied). `Min()` reads 0, `HasMin()` false. Keep. |
| 9 | `board setup constraints micro-via` | BOARD_SETUP | `A.Via_Type == 'Micro'` | all | `HOLE_SIZE` min=`m_MicroViasMinDrill`; `VIA_DIAMETER` min=`m_MicroViasMinSize` |
| 10 | `barcode visual separation default` | BARCODE_DEFAULTS | `A.Type == 'Barcode'` | all | `PHYSICAL_CLEARANCE` min=1.0 mm |
| 11 | netclass rules (below) | NET_CLASS | | all | |
| 12 | tuning-profile rules (below) | TUNING_PROFILE | | all | |
| 13 | keepout rules (below) | KEEPOUT | | zone layers | `DISALLOW` |

**Netclass rules** (L264-471). `m_board->SynchronizeNetsAndNetClasses( false )` is called first
(this is what materialises the composite netclasses for nets with several pattern-assigned
classes). `makeNetclassRules` is called for the default netclass, then for every entry of
`bds.m_NetSettings->GetNetclasses()` (a `std::map<wxString, ...>`: name order), then for every
entry of `GetCompositeNetclasses()` (`std::map`, name order). A class whose name cannot be
written as a string literal (`LIBEVAL::EscapeStringLiteral` returns nullopt: name ends with a
backslash) is skipped entirely (the misc provider warns about it). `nc` = escaped name used in
the expression; `m_ImplicitNetclass` = unescaped `nc->GetName()`. For an effective (composite)
class each `Has...()` is true when any constituent defines the value, and `Get...Parent()` is the
constituent that supplied it; the rule is named after `Parent->GetHumanReadableName()`.

| If | Rule name | Condition | Constraint | Goes to list |
|---|---|---|---|---|
| `HasClearance` | `netclass '<P>'` | `A.hasExactNetclass('<nc>')` | `CLEARANCE` min=clearance | A (also registered in `m_netclassClearances[ nc->GetName() ]`) |
| `HasTrackWidth` | `netclass '<P>'` | `A.hasExactNetclass('<nc>')` | `TRACK_WIDTH` min=`bds.m_TrackMinWidth`, opt=track width | A (sic) |
| `HasDiffPairWidth` | `netclass '<P>' diff pair` | `A.hasExactNetclass('<nc>') && A.inDiffPair('*')` | `TRACK_WIDTH` min=`m_TrackMinWidth`, opt=dp width | B |
| `HasDiffPairGap` | `netclass '<P>'` | `A.hasExactNetclass('<nc>')` | `DIFF_PAIR_GAP` min=`bds.m_MinClearance`, opt=dp gap | B |
| ... and `HasClearance && gap < clearance` | `netclass '<P>' diff pair` | `A.hasExactNetclass('<nc>') && AB.isCoupledDiffPair()` | `CLEARANCE` min=dp gap; sets `m_hasDiffPairClearanceOverrides` | B |
| `HasViaDiameter` | `netclass '<P>'` | `A.hasExactNetclass('<nc>') && A.Via_Type != 'Micro'` | `VIA_DIAMETER` min=`m_ViasMinSize`, opt=via diameter | B |
| `HasViaDrill` | `netclass '<P>'` | same | `HOLE_SIZE` min=`m_MinThroughDrill`, opt=via drill | B |
| `HasuViaDiameter` | `netclass '<P>' uvia` | `A.hasExactNetclass('<nc>') && A.Via_Type == 'Micro'` | `VIA_DIAMETER` min=`m_MicroViasMinSize`, opt=uvia diameter | B |
| `HasuViaDrill` | `netclass '<P>' uvia` | same | `HOLE_SIZE` min=`m_MicroViasMinDrill`, opt=uvia drill | B |

After all classes: list A is sorted with **`std::sort`** by `m_Constraints[0].m_Value.Min()`
ascending (clearance for clearance rules, `m_TrackMinWidth` for the width rules that share the
list), appended to `m_rules`; then list B is appended in creation order. The comment explains
why: when A and B are in different netclasses the larger clearance must be applied last. Ties
are in `std::sort` order (stable for <= 16 elements with libstdc++, see R2); a tie decides
which netclass is *named* in a message, not the value.

**Tuning-profile rules** (L473-598), only with a project: for the default netclass, the
netclasses and the composites that have a tuning profile (and an escapable name), for each
`DELAY_PROFILE_TRACK_PROPAGATION_ENTRY` with width > 0: single-ended profile -> one rule
`tuning profile '<profile>'`, condition `A.hasExactNetclass('<nc>') && A.Layer == '<layer name>'`,
`TRACK_WIDTH` min=`max( m_TrackMinWidth, w )`, opt=w, max=w; differential profile (also needs
gap > 0) -> the same with `&& A.inDiffPair('*')`, a second rule with `DIFF_PAIR_GAP`
min=`max( m_MinClearance, gap )`, opt=gap, max=gap, and (if the netclass has a clearance larger
than the gap) a `CLEARANCE` min=gap rule with `&& AB.isCoupledDiffPair()`. All have
`m_Severity = bds.m_DRCSeverities[DRCE_TUNING_PROFILE_IMPLICIT_RULES]` (default `ignore`, which
makes providers skip the whole constraint when such a rule was the last one applied). Note this
clearance override does not set `m_hasDiffPairClearanceOverrides`. Tier 3.

**Keepout rules** (L600-678): for every board zone (board order), then for every zone of every
footprint (footprint order), if `isKeepoutZone( zone, true )` (L120-146: rule area with keepout
parameters set and at least one of the five "do not allow" flags):

* name: `keepout area` / `keepout area '<zone name>'` / `keepout area of <ref>` /
  `keepout area '<zone name>' of <ref>` (`DescribeRef( footprint reference )`);
* `m_ImplicitItemId = zone uuid`, `m_ImplicitItem = zone`;
* condition `A.intersectsKeepout('<zone uuid string>')`;
* `m_LayerCondition = zone->GetLayerSet()`;
* `DISALLOW` flags: tracks -> `TRACKS`, vias -> `DRC_DISALLOW_VIAS` (= **through vias only**),
  pads -> `PADS`, zone fills -> `ZONES`, footprints -> `FOOTPRINTS`.

### 4.3 `loadRules` and `compileRules` (`drc_engine.cpp:L682-794`)

`loadRules( path )`: only if the file exists. Each line is passed through
`BOARD::ConvertCrossReferencesToKIIDs` and `ExpandTextVars( line, resolver, INTERNAL )` where the
resolver leaves `${Class:...}` tokens alone and otherwise calls `BOARD::ResolveTextVar`; lines
are re-joined with `\n` and parsed by `DRC_RULES_PARSER( text, path ).Parse( rules, m_logReporter )`
(see 4.7). Rules are appended only if parsing did not throw.

`compileRules()`: for each rule in order: compile the condition if it has a non-empty expression
(error -> `PARSE_ERROR( "Parse error", rule name, expression )`); for each constraint of the rule
append a `DRC_ENGINE_CONSTRAINT` to `m_constraintMap[type]`. Then compute
`m_hasExplicitClearanceRules` (any non-implicit `CLEARANCE` constraint),
`m_hasGeometryDependentRules`, `m_explicitConstraints`.

### 4.4 `RunTests( units, reportAllTrackErrors, testFootprints )` (`drc_engine.cpp:L936-994`)

1. `SetUserUnits( units )`; store the two flags.
2. Error limits: for each code: `bds.Ignore( code )` -> 0; `DRCE_CLEARANCE` and
   `DRCE_UNCONNECTED_ITEMS` -> 499 (`EXTENDED_ERROR_LIMIT`); else 199 (`ERROR_LIMIT`).
3. `DRC_TEST_PROVIDER::Init()` (fills `s_allBasicItems`: every `KICAD_T` except footprint and
   group; `s_allBasicItemsButZones`).
4. `m_board->IncrementTimeStamp()`; run `DRC_CACHE_GENERATOR` (4.5); if it returns false
   (cancelled) stop.
5. `GetComponentClassManager().ForceComponentClassRecalculation()`.
6. `connectivity->GetFromToCache()->Rebuild( board )` (for `fromTo()` expressions).
7. For each provider in registry order: `provider->RunTests( units )`
   (`SetUserUnits` + `Run()`); a provider returning false (cancelled) stops the loop.

**Provider order.** Registration happens in static initialisers of the provider translation
units, which are linked as part of the `pcbnew_kiface_objects` object library in the order of
`PCBNEW_DRC_SRCS`; with GNU ld `.init_array` entries run in link order, so the expected order is
the numbering of table 2.3 (annular_width, disallow, creepage, connectivity, connection_width,
copper_clearance, physical_clearance, courtyard_clearance, edge_clearance, footprint_checks,
hole_to_hole, hole_size, library_parity, schematic_parity, misc, net_chain_tuning_profiles,
text_dims, track_angle, track_width, track_segment_length, zone_connections, via_diameter,
via_stack, via_stitch, solder_mask, silk_clearance, matched_length, diff_pair_coupling,
sliver_checker, text_mirroring). **Verify against the oracle** (run a board with violations of
many kinds and check the order of the violation types in the text report, or run with
`WXTRACE=KICAD_DRC_PROFILE`, which logs `DRC provider '%s' took ...` per provider). The order
decides the order of the report entries within a severity class and, through shared error
limits (e.g. `clearance` is reported by providers 6 and 7, `shorting_items` by 6 and 10,
`silk_over_copper` by 25 and 26), which provider's violations are dropped when a limit is hit.

### 4.5 `DRC_CACHE_GENERATOR::Run()` (`drc_cache_generator.cpp:L33-331`)

1. `IncrementTimeStamp()` again (caches are appended to, so they must start empty).
2. `m_DRCMaxClearance = max( m_DRCMaxClearance, board->GetMaxClearanceValue() )` where
   `GetMaxClearanceValue` (`board.cpp:L1325-1353`) = max of
   `bds.GetBiggestClearanceValue()` (`board_design_settings.cpp:L1779-1810`: max of
   `m_MinClearance`, `m_HoleClearance`, `m_HoleToHoleMin`, `m_CopperEdgeClearance` and
   `QueryWorstConstraint` of `CLEARANCE`, `PHYSICAL_CLEARANCE`, `PHYSICAL_HOLE_CLEARANCE`,
   `HOLE_CLEARANCE`, `EDGE_CLEARANCE`, `HOLE_TO_HOLE`, clipped to `MAXIMUM_CLEARANCE` = 500 mm),
   every board zone's local clearance, every pad's clearance override, every footprint zone's
   local clearance.
3. `m_DRCMaxPhysicalClearance`: worst *unconditional* `PHYSICAL_CLEARANCE` /
   `PHYSICAL_HOLE_CLEARANCE` (`QueryWorstConstraint( type, c, aUnconditionalOnly = true )`); if
   that is <= 0 and a conditional one exists: use it when a user-defined physical rule exists, or
   (implicit barcode rule only) when the board contains at least one barcode.
4. Clamp both to `INT_MAX / 3`.
5. Zone lists: for board zones then footprint zones: all zones go to the local `allZones`
   (`std::set<ZONE*>`, only used to schedule independent work); non-rule-areas go to
   `m_DRCZones`; those with a copper layer of the board (`LSET::AllCuMask( copperLayerCount )`)
   get bbox caches built (outline for board-level zones, each layer's fill) and go to
   `m_DRCCopperZones`; then `m_DRCCopperZonesByLayer[layer]` in the same order.
6. Phase "Gathering copper items...": `m_CopperItemRTreeCache` = new `DRC_RTREE`; for every item
   delivered by `forEachGeometryItem( {TRACE, ARC, VIA, PAD, SHAPE, FIELD, TEXT, TEXTBOX, TABLE,
   TABLECELL, DIMENSION, BARCODE}, boardCopperLayers )` insert it on each of its board copper
   layers (a pad with a hole: on **all** board copper layers) with
   `Insert( item, layer, CLEARANCE_CONSTRAINT, m_DRCMaxClearance )`; `Build()`.
7. Phase "Tessellating copper zones...": for every footprint `BuildCourtyardCaches()` and
   `BuildNetTieCache()`; for every zone: `CacheBoundingBox()`, `CacheTriangulation()`, and for
   non-rule-area copper zones a `DRC_RTREE` with `Insert( zone, layer, CLEARANCE_CONSTRAINT )`
   for each copper layer of the zone -> `m_CopperZoneRTreeCache[zone]`.
8. `m_ZoneIsolatedIslandsMap`: an empty `ISOLATED_ISLANDS` for every (board zone that is not a
   rule area, teardrop or copper-thieving zone) x (each layer of the zone).
9. `board->UpdateBoardOutline()`; build the bbox caches of the outline.
10. `connectivity->ClearRatsnest(); connectivity->Build( board, progress );
    connectivity->FillIsolatedIslandsMap( m_ZoneIsolatedIslandsMap, true )`.

### 4.6 `EvalRules( type, a, b, layer, reporter )` (`drc_engine.cpp:L1033-2241`)

This is the heart of the port. `a` is never null in DRC use; `b` may be null (unary
constraints). The reporter is only non-null in the GUI "constraint resolution" dialogs; every
`REPORT( ... )` line is explanatory text only. **Tier 1 ports the logic with `reporter = None`**
(keep the control flow identical; the message strings are tier 3).

Preparation (L1041-1072):

* `ac`/`bc` = the item as connected item if `IsConnected()`; `acNetclass`/`bcNetclass` =
  `GetEffectiveNetClass()`.
* `a_is_non_copper = a && ( !a->IsOnCopperLayer() || isKeepoutZone( a, false ) )`, same for b.
* For `ZONE_CONNECTION`, `THERMAL_RELIEF_GAP`, `THERMAL_SPOKE_WIDTH`, `SOLDER_MASK_EXPANSION`,
  `SOLDER_PASTE_ABS_MARGIN`, `SOLDER_PASTE_REL_MARGIN`: find `pad` (a, then b overrides) and
  `zone` (a, then b overrides) among the items; `parentFootprint` = pad's footprint.

Resolution order (first rule that returns wins):

1. **Net-tie** (L1142-1178), `CLEARANCE` only: if (exactly one of a/b is a connected item, or
   both are in the same existing footprint) and neither is non-copper: for `ii` in 0..1, if
   `footprints[ii]` exists and the *other* item is connected and
   `footprints[ii]->GetNetTieCache( item[ii] )` contains the other item's net code -> return
   min=0, name `net tie`, no rule.
2. **Local overrides** (L1180-1399):
   * `CLEARANCE` / `HOLE_CLEARANCE`: `overrideA = ac->GetClearanceOverrides()` if `ac` and b is
     not non-copper; `overrideB` likewise. (Overrides exist on pads - own padstack clearance,
     else the parent footprint's clearance - and nowhere else.) `override_val` = A's value if
     present; B's value replaces it if greater. If `override_val != 0`: raise it to the board
     minimum (`bds.m_MinClearance`, name `board minimum`; for hole clearance
     `bds.m_HoleClearance`, name `board minimum hole`) otherwise name = the override source
     (`pad` or `footprint <ref>`); return min=override_val, no rule. An override of exactly 0
     falls through to the rules.
   * `ZONE_CONNECTION`: pad with a local zone connection other than `INHERITED` -> return it.
   * `THERMAL_RELIEF_GAP`: pad local gap > 0 -> return min.
   * `THERMAL_SPOKE_WIDTH`: pad local spoke width > 0 -> return min = max( it, zone min
     thickness if a zone is involved ).
   * `SOLDER_MASK_EXPANSION`: local margin of the pad, or of `a` if it is a `PCB_SHAPE` or a
     track; else (pad only) of the parent footprint -> return **opt**.
   * `SOLDER_PASTE_ABS_MARGIN` / `REL_MARGIN`: pad's local value, else parent footprint's ->
     return opt (`REL`: `KiROUND( ratio * 1000 )`).
3. **Rules.**
   * *Fast path* (L1929-1994) when `type == CLEARANCE`, there are no explicit clearance rules,
     no diff-pair clearance overrides, no reporter, a is copper and (b is null or copper):
     look up A's effective netclass name in `m_netclassClearances`, then B's; B replaces A only
     if its clearance is **strictly greater**. If the result is > 0: min = it,
     `m_ImplicitMin = true`, parent rule = that netclass rule.
   * *General path* (L1996-2023): for every `DRC_ENGINE_CONSTRAINT` of the type, in rule order,
     `processConstraint( c )` (below). Each applicable constraint **overrides** the fields it
     defines, so the **last** applicable rule wins per field (min, opt, max separately; disallow
     flags, zone connection, options and parent rule from the last one).
     For `DIFF_PAIR_GAP` with two items: evaluate `CLEARANCE` for the pair and raise min to it
     (min rule = the clearance's min rule); return.
4. If the last applied rule is **explicit** (not implicit) -> return.
5. **Footprint inheritance** (L2028-2084) for the six pad/zone constraint types when a pad with
   a parent footprint is involved: replace the pad by its footprint in (a, b), clear the
   condition cache, run the general path again; explicit result -> return. Otherwise defaults:
   `SOLDER_MASK_EXPANSION` -> opt=`bds.m_SolderMaskExpansion`, name `board setup`;
   `SOLDER_PASTE_ABS_MARGIN` -> opt=`bds.m_SolderPasteMargin`; `REL` ->
   opt=`KiROUND( bds.m_SolderPasteMarginRatio * 1000 )`; return.
6. **Legacy local values** (L2086-2232):
   * `CLEARANCE`: `clearance` = current min. If `ac->GetLocalClearance()` has a value greater
     than `clearance`: min = it, name = source (`pad`, `zone`), no rule; same for `bc` (compared
     against the possibly updated value). Then, if both are copper and
     `clearance < bds.m_MinClearance`: min = `m_MinClearance`, name `board minimum`, no rule
     (note: compared against the local variable `clearance`, which already includes the local
     maxima). Return in every case.
     (`GetLocalClearance()`: pad -> padstack clearance if set; zone -> 0 for rule areas else the
     zone clearance (always set); tracks/vias/shapes -> none.)
   * `ZONE_CONNECTION`: parent footprint's local zone connection if not `INHERITED` (name
     `footprint`); else the zone's pad connection (name `zone`).
   * `THERMAL_RELIEF_GAP`: zone's thermal relief gap (name `zone`). `THERMAL_SPOKE_WIDTH`: zone's
     spoke width.
7. If no rule was applied: `m_Type = NULL_CONSTRAINT`, `m_DisallowFlags = 0`. Return.

`processConstraint( c )` (L1667-1927), in this order:

a. Implicit `NET_CLASS` rules: for `CLEARANCE`, skip if a or b is non-copper. Skip unless
   `acNetclass` or `bcNetclass` `NameEquals( rule->m_ImplicitNetclass )`. Skip unless the
   condition evaluates true (`checkCondition( c, nullptr )`).
b. `CLEARANCE` (any rule): skip if a or b is non-copper.
c. `DISALLOW`:
   * skip if `c->implicitKeepoutZone` and `a` is the footprint that owns that zone;
   * `mask` from the item: `HOLE_PROXY` flag set -> `HOLES`; via -> `MICRO_VIAS` / `BLIND_VIAS` /
     `BURIED_VIAS` / `THROUGH_VIAS` (`IsMicroVia`, `IsBlindVia`, `IsBuriedVia` in that order);
     trace, arc -> `TRACKS`; pad -> `PADS`; footprint -> `FOOTPRINTS`; shape, barcode ->
     `GRAPHICS`; field, text, textbox, table, drill chart, stackup table -> `TEXTS`; zone ->
     `TRACKS` if teardrop area else `ZONES`; locate hole -> `HOLES`; else 0;
   * skip if `( c.flags & mask ) == 0`;
   * `itemLayers = a->GetLayerSet()`; for a footprint add `LSET::FrontMask()` if it has a front
     courtyard and `LSET::BackMask()` if it has a back courtyard; skip if
     `( c->layerTest & itemLayers )` is empty.
d. Layer test: not applied if ( `IsPcbLayer( layer )` and `!c->layerTest.test( layer )` ) or
   the rule's layers contain no enabled board layer.
e. `HOLE_TO_HOLE`: not applied unless both items have a hole.
f. No condition (or empty expression): apply.
g. Otherwise: for implicit keepout rules without reporter, skip when
   `a->GetBoundingBox()` does not intersect the zone bbox. Then `condMatched` = true for
   implicit netclass rules (already tested in a.), else `checkCondition( c, reporter )`; apply
   if matched.

`checkCondition`: without reporter the result is memoised per `EvalRules` call in a map keyed
by the **expression string** (several rules with the same condition are evaluated once).
`applyConstraint( c )` (L1077-1137): copy min / opt / max if `c` has them (`m_ImplicitMin` =
rule is implicit, set only when `c` has a min); for `CLEARANCE`, `COURTYARD_CLEARANCE`,
`SILK_CLEARANCE`, `HOLE_CLEARANCE`, `EDGE_CLEARANCE`, `PHYSICAL_CLEARANCE`,
`PHYSICAL_HOLE_CLEARANCE` clamp min to `MAXIMUM_CLEARANCE`; copy disallow flags, zone connection,
options; parent rule = c's rule; per-value rules updated.

Other entry points:

* `EvalZoneConnection( a, b, layer )` (L999-1030): `EvalRules( ZONE_CONNECTION )`, then
  `THT_THERMAL` -> `THERMAL` if the pad is PTH else `FULL`.
* `ProcessAssertions( a, failureHandler )` (L2278-2344): for each `ASSERTION` constraint in rule
  order: if no condition or `condition->EvaluateFor( a, nullptr, type, a->GetLayer() )` then
  evaluate `c.m_Test->EvaluateFor( a, nullptr, ... )`; false -> handler. (The layer mismatch
  only produces a report line; it does **not** skip the rule.)
* `QueryWorstConstraint( type, out, unconditionalOnly )` (L2468-2492): max `Min()` over all
  constraints of the type (optionally only those without condition); returns `worst > 0`; `out`
  is only written when a larger value is found. `HasRulesForConstraintType`,
  `HasUserDefinedConstraint`, `QueryDistinctConstraints` (set of all `Min()` values).
* `MatchDpSuffix( netName, &complement, &base )` (L2552-2598): scan from the end over digits
  and `_`; the first other character decides: `+` -> complement `-`, polarity 1; `-` -> `+`,
  -1; `N` -> `P`, -1; `P` -> `N`, 1; anything else -> 0. `base` = name without the scanned
  suffix; complement net name = base + complement char + the skipped suffix.
  `IsNetADiffPair` looks the complement up with `BOARD::FindNet`.
* `IsNetTieExclusion( trackNetCode, layer, collisionPos, collidingItem )` (L2635-2656): true if
  the colliding item's parent footprint is a net-tie and one of its pads that belongs to a
  net-tie group (`MapPadNumbersToNetTieGroups()[ pad number ] >= 0`) with the track's net code
  collides with the position (`pad->GetEffectiveShape( layer )->Collide( pos, epsilon )`).
* `GetCachedOwnClearance( item, layer )`: `EvalRules( CLEARANCE, item, nullptr, layer )` (NPTH
  pad: `HOLE_CLEARANCE`), min or 0. Cache optional.

### 4.7 The `.kicad_dru` rule file (`drc_rule_parser.cpp`) - tier 2

Lexing is DSNLEXER (`common/dsnlexer.cpp`) with the keyword table `common/drc_rules.keywords`:
s-expression tokens, `"` quoted strings with backslash escapes, `#` comment lines. Grammar as
implemented:

```
file      := { '(' 'version' NUMBER ')' | '(' 'rule' NAME { clause } ')' }
clause    := '(' 'constraint' TYPE args ')' | '(' 'condition' STRING ')'
           | '(' 'layer' ( 'outer' | 'inner' | LAYERNAME ) ')'
           | '(' 'severity' ( 'ignore' | 'warning' | 'error' | 'exclusion' ) ')'
args (numeric types) := { '(' ('min'|'opt'|'max') EXPR ')' | '(' 'within_diff_pairs' ')' }
args (disallow)      := { track | via | through_via | blind_via | buried_via | micro_via
                          | pad | zone | text | graphic | hole | footprint }   (may be quoted)
args (zone_connection) := solid | thermal_reliefs | none
args (min_resolved_spokes) := NUMBER
args (assertion)     := STRING
args (return_path)   := { '(' 'layer' NAME ')' | '(' 'net' NAME ')' }
```

* The first statement must be `(version N)`; otherwise `Missing version statement.` is reported
  (once). `DRC_RULE_FILE_VERSION = 20200610`; the parsed number only sets
  `m_requiredVersion` / `m_tooRecent`, which nothing reads. Real files use `(version 1)`.
* Error handling: `reportError` -> with a reporter: collect `ERROR: <a href='line:offset'>msg</a>`
  and continue; **without a reporter (CLI): throw `PARSE_ERROR( "ERROR: " + msg, source, line
  text, line, offset )`** on the first error. `PARSE_ERROR::What()` formats
  `<msg> in '<source>', line <n>, offset <o>.`
* Constraint type keywords -> `DRC_CONSTRAINT_T` (L551-604): `assertion`, `clearance`,
  `creepage`, `hole_clearance`, `edge_clearance`, `hole_size`, `hole_to_hole`,
  `courtyard_clearance`, `silk_clearance`, `text_height`, `text_thickness`, `track_width`,
  `track_angle`, `track_segment_length`, `connection_width`, `annular_width`, `via_diameter`,
  `via_dangling`, `zone_connection`, `thermal_relief_gap`, `thermal_spoke_width`,
  `microvia_stack_depth`, `hole_aspect_ratio`, `min_resolved_spokes`, `solder_mask_expansion`,
  `solder_mask_sliver`, `solder_paste_abs_margin`, `solder_paste_rel_margin`, `disallow`,
  `length`, `net_chain_length`, `stub_length`, `return_path`, `skew`, `via_count`,
  `diff_pair_gap`, `diff_pair_uncoupled`, `physical_clearance`, `physical_hole_clearance`,
  `bridged_mask`. A second constraint of the same type in one rule is an error
  (`Rule already has a '%s' constraint.`).
* Values: the tokens inside `(min ...)` are re-joined with their original separators
  (`parseExpression`) and evaluated as a constant expression by `PCBEXPR_EVALUATOR` with a unit
  resolver (`mil`, `mm`, `in`, `deg`, `fs`, `ps`); result `KiROUND`ed to int IU. Unitless types
  (`via_count`, `min_resolved_spokes`, `microvia_stack_depth`, `via_dangling`, `bridged_mask`,
  `hole_aspect_ratio`) use the unitless resolver; `hole_aspect_ratio` stores
  `KiROUND( value * 1000 )`; `track_angle` accepts a bare number as degrees. Time units are only
  allowed for `length`, `net_chain_length`, `stub_length`, `skew` (sets the `TIME_DOMAIN` /
  `SPACE_DOMAIN` option); mixing unit kinds within a constraint is an error. A single number
  without unit is an error (`Missing units for '%s'| (must be mm, in, mil, deg, fs, or ps)`).
* `(layer outer)` = `LSET::ExternalCuMask()`, `inner` = `LSET::InternalCuMask()`, otherwise every
  layer whose canonical or user name wildcard-matches the given name (via
  `ENUM_MAP<PCB_LAYER_ID>`, filled at board load with both names); no match ->
  `Unrecognized layer '%s'.` and the rule gets layer `Rescue`. A second `layer` clause is an
  error.
* After a rule: if the condition uses `B`/`AB` (`RequiresPairItems`) and a constraint is unary
  (`DRC_CONSTRAINT::IsUnary`, `drc_rule.cpp:L88-118`) -> error `Item 'B' is not available for a
  single-item constraint in rule '%s'.` (exception: a condition that uses only `AB` functions
  with `length`, `net_chain_length`, `skew`). Assertions must not use B at all.
* Unresolved `${...}` text variables left in tokens are errors (`Unresolved text variable`),
  except `${Class:...}` selectors.

### 4.8 Conditions and the expression language

#### 4.8.1 `DRC_RULE_CONDITION` (`drc_rule_condition.cpp`)

`EvaluateFor( a, b, constraint, layer, reporter )`: empty expression -> true; no ucode (compile
failed) -> false. Build a `PCBEXPR_CONTEXT( constraint, layer )`; **teardrop zones are given the
type override `PCB_TRACE_T`** (so `A.Type == 'Track'` matches them); run with items (a, b); if
the result is 0 and b is non-null run again with (b, a) ("conditions are commutative"). Result =
`value->AsDouble() != 0.0`.

#### 4.8.2 Language (`kicad-libeval`; `grammar.lemon`, `libeval_compiler.cpp`)

Covered by the libeval analysis; the points DRC depends on:

* Tokens: numbers (decimal point or comma), units directly after a number, `'string'` literals
  (`\'` escape), identifiers `[A-Za-z_][A-Za-z0-9_]*`, `== != <= >= < > && || ! + - * / . , ( )`.
  Spaces are the only whitespace.
* **Precedence from lowest to highest** (Lemon declaration order): `,` ; `&&` ; `||` ; `^`
  (declared, no rule) ; comparisons (non-associative) ; `!` ; `+ -` ; `* /` ; `.` ; unit. So
  `||` binds tighter than `&&` (`a || b && c` = `(a || b) && c`) and `!a == b` is `!(a == b)`.
* `&&` / `||` short-circuit (jump ops) and normalise to 0/1.
* Values: numeric, string (with "is wildcard" flag when the literal contains `*` or `?`),
  null, **undefined**. `==` / `!=` with an undefined operand are both false. String equality is
  case-insensitive; wildcard compare when the right side is a wildcard literal. Numeric vs
  string: a non-literal string that "quacks like a number" is converted.
* An expression that fails at run time or leaves a wrong stack evaluates to 0.
* Function calls are methods only: `A.func(args)`, `B.func(...)`, `AB.func()`,
  `A.Parent.func()`. During **compilation** each function is called once in a "preflight"
  context with the literal arguments and null items so that argument errors are reported with a
  source position. Deferred evaluation: most functions push a value whose result is computed
  only when read (so a short-circuited operand costs nothing).

#### 4.8.3 PCB binding (`pcbexpr_evaluator.cpp`) - tier 2

Variables: `A`, `B` (items), `AB` (pair, functions only), `L` (layer under test: `L.Layer`),
`null`. `A.Parent[.Parent...]` navigation (`GetParent()`).

Field access `A.<Field>` (`CreateVarRef`, L692-841): `NetClass`, `ComponentClass`, `NetName`,
`Type` (case-insensitive) compile to dedicated references; anything else is a property name:
underscores are replaced by spaces, `Origin X/Y` are aliased to `Start X/Y`, and the name is
looked up **case-insensitively in every `BOARD_ITEM`-derived class registered with the
`PROPERTY_MANAGER`**; all classes that have it must agree on the expression type or the field is
a compile error (`Unrecognized property '%s'`). Property kinds: int, optional int, unsigned,
long long, bool -> numeric; double, optional double, angle (degrees) -> numeric double; string,
enum, colour -> string.

`PCBEXPR_VAR_REF::GetValue` (L462-621): layer reference -> layer value; no item -> undefined;
the item's class does not have the property -> try the parent footprint (so
`A.Reference == 'J1'` matches J1's pads) -> else **undefined** (this is why
`A.Via_Type != 'Micro'` is false for a pad); property not available for this item
(`IsAvailableFor`) -> undefined; footprint variant overrides for DNP / exclude flags; then by
kind. Strings that parse as a number with units become numbers. Enum values are the
untranslated choice label. `Layer`, `Layer Top`, `Layer Bottom` produce a *layer value* whose
equality test wildcard-matches the other side against all canonical and user layer names
(cached per pattern). `NetClass` compares against the effective netclass name and each
constituent name; `NetName` values compare by net code when both sides are net references;
`Type` is `ENUM_MAP<KICAD_T>` (`Footprint`, `Pad`, `Graphic`, `Text`, `Text Box`, `Table`,
`Table Cell`, `Track` (trace and arc), `Via`, `Zone`, `Dimension`, `Leader`, `Barcode`, ...;
`common/eda_item.cpp:L575-611`); via types are `Through`, `Blind`, `Buried`, `Micro`
(`pcb_track.cpp:L3361-3366`).

The property table itself (names, types, availability) is spread over the `*_DESC` structs of
the item classes (about 450 registrations in `pcbnew/*.cpp`, `common/eda_shape.cpp`,
`common/eda_text.cpp`); it has to be provided by `kicad-pcb` as a static table
(see section 5).

#### 4.8.4 Built-in functions (`pcbexpr_functions.cpp:L1917-1966`)

Names are case-insensitive. (G) = geometry dependent.

| Function | Semantics (see the cited lines) |
|---|---|
| `existsOnLayer('pattern')` | item layer set intersects the layers matching the pattern (L102-196) |
| `isPlated()` | PTH pad or via |
| `intersectsCourtyard('sel')`, `intersectsFrontCourtyard`, `intersectsBackCourtyard` (G); deprecated aliases `insideCourtyard`, `insideFrontCourtyard`, `insideBackCourtyard` | item shape collides with the courtyard of a footprint selected by `A`, `B`, reference wildcard, `lib:fp` wildcard or `${Class:x}` (L219-550); candidates from `BOARD::GetFootprintCourtyardIndex()` |
| `intersectsArea('sel')` (G), `intersectsKeepout('sel')` (G), deprecated `insideArea` (= keepout variant) | L553-959, see below |
| `enclosedByArea('sel')` (G) | item polygon minus area outline is empty (L961-1083) |
| `isMicroVia()`, `isBlindVia()`, `isBuriedVia()`, `isBlindBuriedVia()`, `isStackedVia()` | via type tests |
| `memberOfGroup('x')` (deprecated `memberOf`), `memberOfFootprint('sel')`, `memberOfSheet('x')`, `memberOfSheetOrChildren('x')` | L1086-1290 |
| `fromTo('ref-pad','ref-pad')` | `FROM_TO_CACHE::IsOnFromToPath` (connectivity) |
| `isCoupledDiffPair()` | L1395-1438: A's net has a DP suffix; for `DIFF_PAIR_GAP`, `LENGTH`, `NET_CHAIN_LENGTH`, `SKEW` true if the coupled net exists; else true if B's net name is the coupled net |
| `inDiffPair('pattern')` | L1441-1492: connected item whose net has a DP suffix and an existing coupled net, and whose base name (also without a trailing `_`) matches |
| `inNetChain('x')`, `hasNetChain()`, `inNetChainClass('x')` | net chain membership |
| `getField('name')` | footprint field text (variant aware) |
| `hasNetclass('x')` | effective netclass contains a constituent with that name |
| `hasExactNetclass('x')` | L1736-1772: connected item and `GetEffectiveNetClass()->NameEquals( x )` |
| `hasComponentClass('x')`, `customProperty('k')`, `hasCustomProperty('k')` | - |

**`intersectsKeepout` / `intersectsArea`** (`doIntersectsAreaFunc`, L815-946; needed in tier 1
for keepouts):

1. Whole-result memo `board->m_Intersects{Keepout,Area}ResultCache[ (item, selector, layer,
   constraint) ]` unless the selector is `A`/`B`.
2. `searchAreas( board, selector, ... )` (L705-780): `A`/`B` -> that item as zone; a string that
   looks like a UUID (`KIID::SniffTest`) -> `board->ResolveItem( uuid )` if it is a zone; else
   every board zone then footprint zone whose **name** wildcard-matches (cached per name).
3. Per area: reject the item itself and areas whose parent is the item; for
   `SILK_CLEARANCE` the area's layer set is temporarily extended (front/back pairing); reject if
   no common layer or bboxes do not intersect; layers to test = the context layer if defined,
   else all common layers in `LSET::UIOrder()`; per layer consult/update
   `m_Intersects{Keepout,Area}Cache[ (area, item, layer) ]`; any layer colliding -> true.
4. `collidesWithArea( item, layer, ctx, area, forKeepout )` (L580-702): area outline =
   `area->GetBoardOutline()` with arcs cleared, **deflated by the DRC epsilon**
   (`CORNER_STRATEGY::ALLOW_ACUTE_CORNERS`, `ARC_LOW_DEF`; cached per zone) so that mere
   touching is not a collision. Then: item with `HOLE_PROXY` flag -> collide the hole shape (pad
   always; via only if it shares a layer with the area and the context layer is undefined or
   shared); footprint -> front/back courtyard outline 0 against the area if the area has
   front/back layers (malformed courtyards -> false); zone -> for keepouts: only filled zones,
   through the zone's fill R-tree (`QueryColliding( areaBBox, &areaOutline, layer )`), for areas:
   outline vs outline; anything else -> the area must contain the layer and
   `areaOutline.Collide( item->GetEffectiveShape( layer ) )`.

**The caches are semantically visible**: the per-(area, item, layer) cache does not include the
`HOLE_PROXY` flag or the constraint, so a second evaluation for the same item with the hole
proxy flag set returns the first answer. Port the caches with the same keys (deterministic map
types), do not "fix" this.

#### 4.8.5 Tier-1 strategy: native implicit conditions

Tier 1 has no `.kicad_dru` support, but the implicit rules use expressions. The closed set of
implicit condition shapes is small; implement `DrcRuleCondition` as an enum
`{ Compiled( PcbExprUcode ), Implicit( ImplicitCondition ) }` where `ImplicitCondition` is a
conjunction of the atoms below, evaluated with exactly the semantics of the compiled form
(including the (a, b) then (b, a) retry of `EvaluateFor`). `GetExpression()` must still return
the textual expression (it is the memo key in `EvalRules`).

| Atom | Native semantics |
|---|---|
| `A.hasExactNetclass('<n>')` | A is a connected item and its effective netclass `NameEquals( n )` |
| `A.inDiffPair('*')` | A connected, has a net, `MatchDpSuffix( net name )` != 0 and `FindNet( coupled )` exists |
| `AB.isCoupledDiffPair()` | as in the table above (uses A, B and the constraint under test) |
| `A.Via_Type == 'Micro'` | A is a via of type microvia (false for non-vias) |
| `A.Via_Type != 'Micro'` | A is a via and not a microvia (**false for non-vias**) |
| `A.Type == 'Barcode'` | effective type of A is barcode |
| `A.Layer == '<name>'` | A has an available `Layer` property (not vias, not multi-layer pads - check the property availability in `kicad-pcb`) and the layer matches the name pattern (tier 3, tuning profiles only) |
| `A.intersectsKeepout('<uuid>')` | 4.8.4, with the caches |

When tier 2 lands, a test must assert for every implicit rule of every test board that the
native and the compiled evaluation agree; then either form may stay.

### 4.9 `ReportViolation` and error limits (`drc_engine.cpp:L2350-2409`)

`ReportViolation( item, pos, markerLayer, pathGenerator )`: under a mutex, if
`m_errorLimits[code] <= 0` drop the violation, else decrement; then call the violation handler
(serialised by a second mutex); then optional log output. `IsErrorLimitExceeded( code )` =
limit <= 0; `GetErrorLimit( code )` = `max( 0, limit )`. Providers call `IsErrorLimitExceeded`
to skip work, so an `ignore` severity (limit 0 from the start) disables the test entirely. The
cap is exact: at most 199 (499) violations per code reach the handler.

Provider helpers (`drc_test_provider.cpp`):

* `reportViolation( item, pos, layer, pathGenerator )` sets the violating test and forwards.
* `reportTwoPointGeometry( item, markerPos, ptA, ptB, layer )`: path = one segment.
* `reportTwoShapeGeometry( item, markerPos, shape1, shape2, layer, distance )`: distance 0 ->
  degenerate path at the marker; else `shape1->NearestPoints( shape2, ptA, ptB )`.
* `reportTwoItemGeometry(...)`: the same with the items' effective shapes on the layer.
* `formatMsg( fmt, source, constraint, actual [, extra] )`: both numbers formatted with
  `MessageTextFromValue( value, addUnits = true )`; **if the two strings are equal** both are
  re-formatted with the higher precision `StringFromValue( value, true )`. Angle overload
  likewise.
* `forEachGeometryItem( types, layers, func )` (L159-411): the canonical board walk. Order:
  1. `board->Tracks()` in order (trace / via / arc if in the type mask and on one of the
     layers); **the callback's return value is ignored here** (no early exit);
  2. `board->Drawings()`: dimension (all `PCB_DIM_*`), shape, text, textbox, target, table (and,
     if `PCB_TABLECELL_T` is requested, each cell; a drill chart counts as a table), barcode -
     each only if `( item->GetLayerSet() & layers ).any()`;
  3. `board->Zones()` if `PCB_ZONE_T` requested and on the layers;
  4. for each footprint in `board->Footprints()` order: fields (if `PCB_FIELD_T`), pads (if
     `PCB_PAD_T`; a pad with a hole is delivered regardless of the layer mask), graphical items
     (dimension, text, textbox, shape, table/cells, barcode), footprint zones, and finally the
     footprint itself if `PCB_FOOTPRINT_T` is requested.
  From step 2 on, a callback returning false ends the walk. An empty type list means all types.
  Returns the number of items visited.
* `isInvisibleText( item )`: a `PCB_FIELD` that is not visible.

### 4.10 Test providers

For each provider: what it checks, constraints, data, cost, and order sensitivity. "Parallel"
refers to the C++; section 8 explains the sequential port. `eps` = `bds.GetDRCEpsilon()` =
`mmToIU( ADVANCED_CFG::m_DRCEpsilon )` = 500 nm with the default advanced config.

#### 4.10.1 Annular width (`drc_test_provider_annular_width.cpp`) - T1

Skipped if `annular_width` is at its limit or no `ANNULAR_WIDTH` rule exists (rule 1 always
provides one). Walk: all tracks (vias only), then all pads of all footprints.

* Via: for each unique padstack layer (`Padstack().ForEachUniqueLayer`) that is not a ghost
  layer: `EvalRules( ANNULAR_WIDTH, via, nullptr, layer )`; `ptA = pos - ( drill / 2, 0 )`,
  `ptB = pos - ( width( layer ) / 2, 0 )`.
* Pad: only PTH pads with a hole; `sameNumPads = footprint->GetPads( pad number, pad )`; per
  unique layer `getPadAnnulusPts` (L128-282): fast path for offset-free circle / oval / rect /
  roundrect / chamfered rect with ratio <= 0.30 (shorter of the x and y half differences,
  rotated by the pad orientation); slow path via `TransformShapeToPolygon` and
  `SHAPE_POLY_SET::NearestPoints( slot )`; with same-number pads the union of their outlines
  minus the other pads' holes (`BooleanSubtract`) is used; several shortcuts decide when the
  slow path can be skipped (read L171-225 carefully).
* `checkConstraint`: `width = ( ptA - ptB ).EuclideanNorm()`; fail if `width < min` or
  `width > max`; message `(%s min annular width %s; actual %s)` (non-`NORMAL` padstack mode:
  `... on <layer desc>`; layer desc = F.Cu / B.Cu name, `Inner Layers` for
  `FRONT_INNER_BACK`), or `(%s max annular width %s; actual %s)`. Skipped when the constraint
  severity is `ignore`. Items = the via/pad; marker at the item position, layer = tested layer.

Cost O(vias + pads); polygon booleans only for unusual pads. Deterministic, sequential.

#### 4.10.2 Disallow / keepouts (`drc_test_provider_disallow.cpp`) - T1

1. Collect: rule areas with "no zone fills" (`antiCopperKeepouts`), copper zones, and an R-tree
   `antiTrackKeepouts` of rule areas with "no tracks" (inserted on each of their layers with
   `CLEARANCE_CONSTRAINT`, i.e. by outline shape).
2. For every (keepout, copper zone) pair (parallel): if the zone is filled and bboxes intersect,
   deflate the keepout outline by eps and test it against the zone fill R-tree on each keepout
   layer; store the result in `m_IntersectsKeepoutCache[ (keepout, zone, UNDEFINED_LAYER) ]`.
   (The expression function looks the cache up per concrete layer, so this pre-computation is
   never read by it; port it anyway or record the omission - it has no observable effect.)
3. For every item of `forEachGeometryItem( {}, all layers )` (parallel per item):
   * `text_on_edge_cuts`: field, text, textbox, table, barcode or dimension whose
     `GetLayer() == Edge_Cuts` -> violation at the item position, layer Edge.Cuts.
   * Rule areas with keepout parameters are not tested themselves.
   * Tracks and arcs: `antiTrackKeepouts->QueryColliding( track, layer, layer, ..., visitor,
     m_DRCMaxPhysicalClearance )`; for each colliding keepout: outline (`GetBoardOutline()` for
     footprint zones, `Outline()` otherwise) `Collide( track shape, 0, &actual, &pos )` ->
     `items_not_allowed` at `pos`, items = the track, **no violating rule, no detail**, marker
     layer = the track layer. One violation per (track, keepout).
   * `EvalRules( DISALLOW, item, nullptr, UNDEFINED_LAYER )`; if `m_DisallowFlags != 0` and the
     severity is not `ignore`: `reportDisallow`: skipped when there is no parent rule, or for
     tracks/arcs when the rule is implicit (already reported above). Position = item position,
     or for implicit keepouts the collision point of the keepout outline with the item's shape
     on `item->GetLayerSet().ExtractLayer()`; detail `(<constraint name>)`; violating rule set.
   * If the item has a hole: set the `HOLE_PROXY` flag, evaluate and report again, clear the
     flag. In Rust the flag must not be a mutation of the shared board: pass a `hole_proxy: bool`
     in the evaluation context.

Because `EvalRules` keeps only the last matching rule, an item inside several keepouts yields
one violation (naming the last keepout in rule order). Cost O(items x keepouts) with bbox
rejection. Violation set is deterministic; C++ report order is not (parallel).

#### 4.10.3 Connectivity (`drc_test_provider_connectivity.cpp`) - T1

Uses the connectivity rebuilt by the cache generator. Sequential.

1. For each track in board order: `connectivity->TestTrackEndpointDangling( track, true, &pos )`
   -> `track_dangling` (traces, arcs) or `via_dangling` (vias; evaluates
   `EvalRules( VIA_DANGLING, via, nullptr, via->GetLayer() )` only for its severity / rule).
   Marker at `pos`, layer `track->GetLayer()`. Per-code limit checks at the loop head.
2. `track_on_post_machined_layer`: for each trace/arc, the first connected pad/via
   (`GetConnectivityAlgo()->ItemEntry( track ).GetItems().front()->ConnectedItems()`) that is
   backdrilled or post-machined on the track layer; marker at the track end that hits the item
   (else the midpoint). One per track. The order of `ConnectedItems()` decides which item is
   named.
3. `track_not_centered_on_via` (default `ignore`): L181-262.
4. `isolated_copper`: for each `(zone, islands)` of `m_ZoneIsolatedIslandsMap` (**pointer-keyed
   `std::map`: iteration order is by address**), skipping copper-thieving zones, for each copper
   layer (ascending layer id), for each index in `m_IsolatedOutlines`: violation at
   `zone->GetFilledPolysList( layer )->Outline( idx ).CPoint( 0 )`, items = zone, marker layer =
   layer. Port: iterate zones in board order (deviation, see 4.12).
5. `unconnected_items`: `connectivity->RunOnUnconnectedEdges` iterates the ratsnest nets in net
   order and each net's edges in order; per edge: items = (source node parent, target node
   parent), marker at the source node position, layer `UNDEFINED_LAYER`. Stops at the limit
   (499). Edges are reported regardless of their "visible" flag.

#### 4.10.4 Copper clearance (`drc_test_provider_copper_clearance.cpp`) - T1

Not run when `m_DRCMaxClearance <= 0`. `sub_e( c ) = max( 0, c - eps )`: every clearance
comparison is relaxed by eps, messages quote the unrelaxed value. Builds a `DRC_ZONE_INDEX` over
`m_DRCCopperZonesByLayer`; `collectZonesNear( item, layer )` = zones on the layer whose bbox
intersects the item bbox inflated by `m_DRCMaxClearance`, **in list order**. Phases (L138-195;
note the gating on which limits are already exceeded):

**a) `testTrackClearances`** (L635-746), parallel per track index. For each board copper layer
of the track: `m_CopperItemRTreeCache->QueryCollidingPreparedCopper( track, shape, layer,
filter, visitor, m_DRCMaxClearance, usePaddingCredit = true )`.

* Filter: reject items of the same net code; reject trace/arc/via with a smaller UUID
  (`track->m_Uuid > other->m_Uuid`), so each track pair is tested once from the smaller UUID.
* Visitor: **free pads** (`PAD::IsFreePad()`): if the pad shape touches the track, the first
  track net to touch the pad "claims" it in `freePadsUsageMap` and tracks of that net are not
  tested against it (shared, order-dependent state). Then
  `testSingleLayerItemAgainstItem( track, shape, layer, other, preparedOther )`; if it reports
  something and `!reportAllTrackErrors`, stop the query for this track and layer.
* Then `testItemAgainstZone( track, zone, layer )` for the zones near.

`testSingleLayerItemAgainstItem` (L217-403):

1. `testClearance`, `testShorting`, `testHoles` from the limits; same net object -> no
   clearance/shorting test.
2. `other` pad not flashed on the layer -> its shape is the hole shape
   (`GetEffectiveHoleShape( layer, HOLE_CLEARANCE_CONSTRAINT )`), and NPTH -> no
   clearance/shorting test; `other` via not flashed -> hole shape.
3. **Canonical order: if `item->m_Uuid > other->m_Uuid` swap item/other (and shapes, nets).**
4. `EvalRules( CLEARANCE, item, other, layer )`; if severity != ignore and clearance > 0:
   * trace vs trace: `SEG::Intersect` -> `tracks_crossing` at the intersection, return false;
   * `itemShape->Collide( otherShape, sub_e( clearance ), &actual, &pos )`:
     net-tie exclusion (either direction) -> nothing; `actual == 0 && otherNet && testShorting`
     -> `shorting_items` with detail `(nets %s and %s)` (`<no net>` for net code 0), no rule;
     else `clearance` with `(%s clearance %s; actual %s)`, rule set, two-shape geometry. In both
     cases return false unless `reportAllTrackErrors`.
5. Holes (`testHoles` and either has a hole), for (a, b) = (item, other) then (other, item): b's
   hole shape on the layer (via: only if the via's layer set contains the layer); skip on
   net-tie exclusion at the hole centre; `EvalRules( HOLE_CLEARANCE, b, a, layer )`; if severity
   != ignore and `a_shape->Collide( hole, sub_e( clearance ) )` (**tested even when the
   clearance is 0**) -> `hole_clearance`, detail `(%s clearance %s; actual %s)` or
   `(... actual < 0)` when the clearance is 0, items (a, b); return false.

`testItemAgainstZone( item, zone, layer )` (L406-549): zone on the layer; skip same non-zero
net; bbox reject with `m_DRCMaxClearance`; net-tie graphics exemption (L426-461); pad neither
flashed nor plated-hole -> no clearance test; `EvalRules( CLEARANCE, item, zone, layer )`,
`zoneTree->QueryColliding( itemBBox, itemShape, layer, sub_e( clearance ), &actual, &pos )` ->
`clearance` (items item, zone); then the item's hole against the zone fill with
`HOLE_CLEARANCE` (clearance > 0 required here).

**b) `testPadClearances`** (L974-1045), parallel per footprint: for each pad, for each board
copper layer of the pad: prepared-copper query with filter "other pad with smaller UUID is
skipped"; visitor `testPadAgainstItem` (L749-971): disables tests for net-tie groups, same
logical pad, NPTH without copper, tracks/vias (done in a), shapes/textboxes (done in c), same
net; a pair of "same logical pad"s (`SameLogicalPadAs`) is only tested for shorting:
`shorting_items` at the other pad's position unless this pad's net code is 0, the net codes are
equal, or both short net names start with `unconnected-(` - and nothing else is tested for such
a pair; otherwise clearance
(`shorting_items` when `actual == 0` and both have nets, else `clearance`), then hole tests in
both directions and against vias (`hole_clearance`, clearance > 0 required). Then zones near.
A through-hole pad pair can yield one violation per shared copper layer.

**c) `testGraphicClearances`** (L1048-1228), parallel: every board drawing, every footprint
graphic and every footprint field: against zones near on its layer (knockout text is special:
`testKnockoutTextAgainstZone`, L557-632 - it inherits the net of the first zone it collides with
and is then tested against zones of other nets, giving `shorting_items` or `clearance`); copper
shapes and barcodes additionally against the copper item tree (filter: shapes of the same
footprint, tracks/vias, same non-zero net, graphics with smaller UUID are skipped; no padding
credit).

**d) `testZonesToZones`** (L1268-1509): candidate pairs per enabled copper layer in
`LAYER_RANGE( F_Cu, B_Cu, copperLayerCount )` order (physical stack order: F.Cu, inner layers,
B.Cu - not ascending layer id) from `CollectOverlappingPairs` over fill bboxes (pairs in
nested-loop order over `m_DRCCopperZones` indices; teardrops excluded); skip same net (>= 0) and rule areas; per pair:
`EvalRules( CLEARANCE, zoneA, zoneB, layer )`; fill outline segments (normalised so
`A.x <= B.x`) of zone B are in a `PACKED_RTREE<size_t>`; for each segment of zone A in order,
search within the clearance: `actual = floor( sqrt( dist_sq ) + 0.5 )`; the **first** pair with
`actual < clearance` (no eps here) is reported: `clearance`,
`(%s clearance %s; actual %s)` with `max( actual, 0 )`, marker at the point on A's segment.
One violation per zone pair and layer.

**e) `testTeardropClearances`** (L1231-1265): teardrop zones against the zones near through
`testItemAgainstZone`; teardrop-teardrop pairs are de-duplicated by **pointer order**
(`zone < teardrop`), i.e. which teardrop is the "item" is address dependent.

Cost: O(items x neighbours) through the R-tree; the dominant DRC cost on real boards.

#### 4.10.5 Edge clearance (`drc_test_provider_edge_clearance.cpp`) - T1

1. `m_largestEdgeClearance` = worst `EDGE_CLEARANCE` min.
2. Edge tree: for every `PCB_SHAPE` on Edge.Cuts or Margin (`forEachGeometryItem`): a **clone**
   (same UUID) with stroke width forced to 0 on Edge.Cuts; unfilled rectangles are split into 4
   segments (rounded rectangles into their segments and arcs), unfilled polygons into one
   segment per edge of outline 0. Inserted on Edge.Cuts / Margin with
   `EDGE_CLEARANCE_CONSTRAINT` and the worst clearance. Castellated pads are collected.
3. Work units: for every item of `forEachGeometryItem( s_allBasicItems, all layers )` except
   invisible fields, rule areas, castellated pads and `CONN` (edge connector) pads: one unit per
   layer to test (pad/via: `Padstack().UniqueLayers()`; zone: each layer; else
   `UNDEFINED_LAYER`) with `item->GetEffectiveShape( layer )`. Non-zone copper items also go to
   `copperItems`.
4. Per unit (parallel), for test layer in (Edge.Cuts, Margin): copper items ->
   `m_edgesTree.QueryColliding( item, shapeLayer, testLayer, nullptr, visitor,
   m_largestEdgeClearance )` with `testAgainstEdge( ..., EDGE_CLEARANCE_CONSTRAINT,
   DRCE_EDGE_CLEARANCE )`; silk items likewise with `SILK_CLEARANCE_CONSTRAINT` /
   `DRCE_SILK_EDGE_CLEARANCE`.
   `testAgainstEdge` (L212-279): `EvalRules( type, edge, item, UNDEFINED_LAYER )`; if severity !=
   ignore and `minClearance >= 0`: `itemShape->Collide( edgeShape, max( 0, min - eps ), &actual,
   &pos )`; tracks are exempt inside castellated pad holes; de-duplicate on
   (item uuid, edge uuid, pos) across units; violation: detail only when `minClearance > 0`,
   **items (edge, item)**, rule set, marker at `pos` on the unit's shape layer. A non-track item
   stops after its first edge ("one is enough"); tracks continue only with
   `reportAllTrackErrors`.
5. Silk disposition (L597-618, `resolveSilkDisposition` L116-209): a silk item whose parts lie
   on both sides of the board outline gets a `silk_edge_clearance` against the edge nearest to
   the item centre (found with `m_edgesTree.GetObjectsAt` - an unordered set, first `HitTest`
   match wins).
6. `testCopperOutsideBoard` (L282-327): board outline polygon (`GetBoardPolygonOutlines`,
   fractured, triangulated); a copper item none of whose units collide with it ->
   `copper_outside_board` at the item position (default severity warning).

#### 4.10.6 Hole to hole (`drc_test_provider_hole_to_hole.cpp`) - T1

Needs a `HOLE_TO_HOLE` rule with min > 0 (`QueryWorstConstraint`), else skipped. All pads with a
hole and all vias are inserted into `m_holeTree` on the pseudo-layer Edge.Cuts with
`HOLE_CLEARANCE_CONSTRAINT` (the "effective shape on Edge.Cuts" of a pad/via is its hole, see
section 5). Then, sequentially: every non-micro via in track order, then every pad with a hole
in footprint order: `m_holeTree.QueryColliding( item, Edge_Cuts, Edge_Cuts, filter, visitor,
largest )`; the filter de-duplicates unordered pairs across both passes (`checkedPairs`).

`testHoleAgainstHole` (L239-311): two vias that share no copper layer are ignored; centres
closer than eps -> `holes_co_located` (items in UUID order, marker at the hole centre,
`UNDEFINED_LAYER`); else `actual = max( 0, dist( segA, segB ) - wA / 2 - wB / 2 )`
(hole shapes from `GetEffectiveHoleShape( UNDEFINED_LAYER, HOLE_TO_HOLE_CONSTRAINT )`),
`EvalRules( HOLE_TO_HOLE, a, b, UNDEFINED_LAYER )`; violation if severity != ignore,
`min >= 0` and `actual < max( 0, min - eps )`: `hole_to_hole`,
`(%s min %s; actual %s)`, items in UUID order.

#### 4.10.7 Hole size (`drc_test_provider_hole_size.cpp`) - T1

Pads (footprint order): drill minor/major; `EvalRules( HOLE_SIZE, pad, nullptr,
UNDEFINED_LAYER )`; `major > max` or `minor < min` -> `drill_out_of_range`,
`(%s min hole %s; actual %s)` / `(%s max hole %s; actual %s)`; for the min case the name is
replaced by `board setup constraints` when the parent rule is implicit. Vias (track order):
`drill = via->GetDrillValue()`; code `microvia_drill_out_of_range` for microvias; name replaced
when `constraint.m_ImplicitMin`. Marker at the pad/via position, `UNDEFINED_LAYER`.

#### 4.10.8 Track width (`drc_test_provider_track_width.cpp`) - T1

For traces and arcs in track order: `EvalRules( TRACK_WIDTH, item, nullptr, item->GetLayer() )`;
if severity != ignore: `width < min` -> `(%s min width %s; actual %s)` (name replaced by
`board setup constraints` when `m_ImplicitMin`), `width > max` -> `(%s max width %s; actual %s)`.
Marker at the arc start / segment midpoint (`( start + end ) / 2`), layer = track layer.

#### 4.10.9 Via diameter (`drc_test_provider_via_diameter.cpp`) - T1

For vias, per unique non-ghost padstack layer: `EvalRules( VIA_DIAMETER, via, nullptr, layer )`,
`actual = via->GetWidth( layer )`; messages `(%s min diameter %s; actual %s)` (+ ` on %s` for
non-`NORMAL` padstacks), same for max. Marker at the via position, layer = tested layer.

#### 4.10.10 Physical clearance (`..._physical_clearance.cpp`) - T2

Only when `m_DRCMaxPhysicalClearance > 0` (user `physical_clearance` /
`physical_hole_clearance` rules, or barcodes on the board). Own R-tree over all physical layers
(holes pierce everything, footprints by courtyard, edge items on all layers), pairwise
`PHYSICAL_CLEARANCE` / `PHYSICAL_HOLE_CLEARANCE`; then intra-item checks of polygon outlines.
Reports under `clearance` / `hole_clearance`. Pair de-duplication is by pointer order in an
unordered map (lookups only). Sequential. 874 lines.

#### 4.10.11 Courtyard clearance (`..._courtyard_clearance.cpp`) - T2

Part 1: per footprint: malformed courtyard (re-runs `BuildCourtyardCaches( &errorHandler )` to
get messages) -> `malformed_courtyard`; no courtyard and `!AllowMissingCourtyard()` ->
`missing_courtyard` (default ignore). Part 2: footprints **sorted by UUID**, all pairs i < j with
bbox gating: `EvalRules( COURTYARD_CLEARANCE, fpA, fpB, F_Cu / B_Cu )`,
`front.Collide( &frontB, clearance, &actual, &pos )` -> `courtyards_overlap`; PTH / NPTH pad
holes of one footprint inside the other's courtyard -> `pth_inside_courtyard` /
`npth_inside_courtyard`. O(F^2) pairs, cheap rejection. Deterministic. Needs courtyard polygon
construction (`convert_shape_list_to_polygon.cpp`).

#### 4.10.12 Footprint checks (`..._footprint_checks.cpp`) - T2

Thin wrapper over `FOOTPRINT_UTILS::CheckFootprintAttributes`, `CheckPads` (->
`PAD_UTILS::CheckPad`, mapped by `ConvertPadCheckToDrcCode`), `CheckNetTies`,
`CheckNetTiePadGroups` (`pcbnew/footprint_utils.cpp` 443 lines, `pcbnew/pad_utils.cpp` 452
lines) and the unit-scale check. Sequential, deterministic.

#### 4.10.13 Misc (`..._misc.cpp`) - T2 (do the outline test first)

In order: `testUnquotableNetclasses` (generic warning for netclass names ending in a backslash);
`testOutline` (`TestBoardOutlinesGraphicItems` with min size 0.001 mm, then
`BuildBoardPolygonOutlines( board, outline, maxError = 0.05 mm,
board->GetOutlinesChainingEpsilon(), true, &errorHandler )`; each error -> `invalid_outline` with
the builder's message, optionally `(gap %s)`, marker position heuristics L158-233; no error
callback but failure -> `(no edges found on Edge.Cuts layer)` at the board bbox centre);
`testDisabledLayers` (`item_on_disabled_layer`, copper layers only); `testTextVars`
(`${DRC_WARNING ...}` / `${DRC_ERROR ...}` markers in texts -> generic warning/error with the
captured text as the whole message; otherwise unresolved `${...}` in the shown text ->
`unresolved_variable`; also drawing-sheet texts - out of scope); `testAssertions` (parallel per
item; `ProcessAssertions`; `assertion_failure` with `(<rule name>)`); `testMissingTuningProfiles`.
`invalid_outline` is the single most common "real board" violation outside tier 1.

#### 4.10.14 Zone connections (`..._zone_connections.cpp`) - T2

Per (copper zone, layer) (parallel), per pad of the same net flashed on the layer whose bbox
meets the zone: `EvalZoneConnection` must be `THERMAL`; `MIN_RESOLVED_SPOKES` min; pad outline
inflated by half the `THERMAL_RELIEF_GAP`; count intersections of the fill outlines with it
(pairs of unique intersection points = spokes; fills flagged as single-connection islands are
"ignored spokes"); custom-pad spoke primitives; extra spokes from tracks and copper shapes;
`starved_thermal` with `(layer %s; %s min spoke count %d; actual %d)`. Needs zone fills and
`ISOLATED_ISLANDS::m_SingleConnectionOutlines`.

#### 4.10.15 Silk clearance (`..._silk_clearance.cpp`) - T2

Silk tree x target tree (`QueryCollidingPairs` over 18 layer pairs), tented items replaced by
their hole, items outside the board outline skipped, `EvalRules( SILK_CLEARANCE, ref, test,
targetLayer )`; mask layers report `silk_over_copper` (and get an implicit clearance of 0 when
the board has no solder mask min width), others `silk_overlap`. Same-layer pairs in UUID order.
Sequential; visit order = tree order.

#### 4.10.16 Solder mask (`..._solder_mask.cpp`) - T2 (expensive)

Builds the whole-board mask polygon per side into the scratch zone `m_SolderMaskBridges`
(union, optional deflate by the web width, triangulation), an item tree, then silk-to-mask
(`silk_over_copper`) and mask bridge tests (`solder_mask_bridge`; rule name
`board setup solder mask min width`; `BRIDGED_MASK` constraint to waive). Parallel, but the
bridges are collected and **explicitly sorted** (layer, sorted item UUIDs, position) before
being reported - port the comparator as is. 1090 lines plus heavy polygon work.

#### 4.10.17 Remaining providers (cost overview)

| Provider | Lines | Notes |
|---|---|---|
| text_dims | 286 | `TEXT_HEIGHT` / `TEXT_THICKNESS` (implicit rules are silk-only); outline fonts need glyph polygons (tier 2 stroke font only -> skip the outline-font branch) |
| text_mirroring | 133 | trivial flags test on F/B Cu, SilkS, Mask, Fab |
| track_angle | 195 | needs a `track_angle` rule; connectivity `GetConnectedTracks`; parallel |
| track_segment_length | 171 | needs a rule; parallel |
| via_stack | 467 | microvia stack checks, hole aspect ratio (needs stackup thicknesses), `PCB_VIA::CollectMicroviaColumns` |
| via_stitch | 107 | generator items; OUT unless generators are modelled |
| sliver_checker | 287 | per-layer copper union + angle/width scan of the outline; parallel per layer; fracture/simplify heavy |
| connection_width | 587 | only when a min connection width > 2 x (eps + max error + extra clearance) exists; per net and layer polygon union + `VERTEX_SET` neck search; iterates an `unordered_map` and pointer sets (order of reports, and of the contributing items within a report, is address dependent) |
| diff_pair_coupling | 778 | needs `diff_pair_gap` / `diff_pair_uncoupled` constraints (netclass DP gap creates one); sequential |
| matched_length | 1032 + chain topology 927 + length report 78 | needs user rules; uses `LENGTH_DELAY_CALCULATION` |
| creepage | 442 + 4614 | needs `creepage` rules; own graph engine |
| net_chain_tuning_profiles | 102 | iterates an `unordered_map` (order) |
| library_parity | 1303 | see 4.10.25 |
| schematic_parity | 410 | only with `--schematic-parity`; OUT |

#### 4.10.25 Library parity in a library-less port

`DRC_TEST_PROVIDER_LIBRARY_PARITY::Run` (L1154-1297): without a project it does nothing; with a
project, every footprint with a non-empty library nickname is looked up in the footprint library
table. Three early outcomes produce `lib_footprint_issues` (warning) with the **whole message**
replaced (`SetErrorMessage`): `The current configuration does not include the footprint library
'%s'`, `The footprint library '%s' is not enabled in the current configuration`,
`The footprint library '%s' was not found at '%s'`; marker at the footprint centre. The port has
no libraries. Decision for the CLI: expose `LibraryParity::{Skip, NoLibrariesConfigured}`;
`NoLibrariesConfigured` emits the first message for every footprint that has a library nickname
(when the board has a project), which is what a hermetic reference run prints if its library
table is empty. Check what the oracle actually prints (its config may contain a default table)
and filter these two codes in differential tests if in doubt.

### 4.11 After the run: markers, exclusions, selection, ordering

* Markers are appended to the board in `ReportViolation` order.
* `ResolveDRCExclusions( false )` (`board.cpp:L595-640`): for each marker build its exclusion
  identity (6.4) and look it up in the set recorded before the run; on a match
  `SetExcluded( true, comment )` and keep the exclusion in `bds.m_DrcExclusions`.
* `DRC_ITEMS_PROVIDER::SetSeverities( mask )` (`drc_item.cpp:L713-734`): keep markers whose
  marker type is in the provider's list and `( marker->GetSeverity() & mask ) > 0`, in board
  marker order, then **`std::stable_sort` by severity descending** (error 0x20, warning 0x10,
  exclusion 0x04). `GetCount()` = number kept.
* The report severity of an entry (`drc_report.cpp:L83-90`): the marker severity, except that
  an excluded marker shows the severity it would have had by board settings
  (`bds.GetSeverity( code )`), annotated as excluded. Unconnected and parity sections always
  use `bds.GetSeverity( code )`.

### 4.12 Where iteration order can change the output

"Set" = which violations exist / which items they name; "order" = only the order of report
entries. The Rust port is sequential and must be deterministic; where C++ is not, the port
picks the order stated and records it in `PORT_STATUS.md`.

| Place | C++ behaviour | Effect | Port |
|---|---|---|---|
| Provider registration | static-init order = `PCBNEW_DRC_SRCS` order (expected) | order; with shared limits also set | fixed array; verify once with the oracle |
| `ReportViolation` from pool threads (copper clearance, edge clearance, disallow, assertions, zone connections, track angle/length, sliver, connection width) | scheduling dependent | order; **set** when a per-code limit (199/499) is exceeded | index order (the order of the sequential loop) |
| Netclass rule sort | `std::sort`, ties unspecified | which netclass is named when two classes have the same clearance (general path only) | port libstdc++ `std::sort` (R2) |
| `PACKED_RTREE` build | `std::sort` on Hilbert value, ties unspecified | search order -> first hit (tracks without `--all-track-errors`, edge "one is enough", zone-zone first segment pair, `QueryColliding` minimum ties) | same as above, in `kicad-geom` |
| `freePadsUsageMap` (track clearance) | first thread to reach a free pad claims its net | set (only boards with free pads) | track index order |
| `m_emittedEdgeReports` (edge clearance) | first unit to report wins | marker layer of the kept violation | unit order |
| `m_ZoneIsolatedIslandsMap` | `std::map<ZONE*>` by address | order of `isolated_copper`; set beyond the limit | board zone order |
| Teardrop pairs (`zone < teardrop`) | by address | which teardrop is item 1 vs 2 | index order in `m_DRCCopperZones` |
| `checkedPairs` with pointer canonical order (hole-to-hole, physical clearance, solder mask, `QueryCollidingPairs`) | pointer pair as key only | none (dedup only) | any canonical key (item index pair) |
| `DRC_RTREE::GetObjectsAt` | `unordered_set` iteration | nearest-edge choice for silk crossing the edge when several edges hit | collect in tree order |
| Connection width `dataset` (`unordered_map`), `std::set<BOARD_ITEM*>` | hash / address order | order; item order inside a violation | key order (net, layer), board item order |
| net-chain tuning profiles | `unordered_map` iteration | order | sort by chain name |
| `allZones` (`std::set<ZONE*>`) in cache generator | address order | none | - |
| UUID comparisons (`m_Uuid > other->m_Uuid`) | deterministic given the UUIDs | main/aux item order; exclusion identity | byte-wise compare of the 16 UUID bytes. Legacy boards without UUIDs get random ones in C++ - the board reader must assign deterministic ids and differential tests must normalise (R7) |
| `conditionCache` keyed by expression string | per-call memo | none (pure) | `BTreeMap`/`HashMap` lookups only |
| Expression predicate caches keyed by item pointers | lookups only, but the *keys* omit the hole-proxy flag | set (4.8.4) | same keys, with item ids instead of pointers |
| `GetNetclasses()` / `GetCompositeNetclasses()` (`std::map<wxString>`) | name order (code point order) | rule order | `BTreeMap<String, _>` |

---

## 5. What DRC needs from other subsystems

This list drives the API of the lower crates. "(T2)" = only needed by tier-2/3 packages.

### 5.1 `kicad-core`

* `ki_round`; `MinOptMax<i32>` (3.2).
* `Kiid`: 16 raw bytes with `Ord` = byte-wise lexicographic (`boost::uuids::uuid::operator<`),
  `as_string()`, `sniff_test( &str )` (`common/kiid.cpp:L175-200`), `NIL`.
* Units: `EdaUnits`, `EdaDataType`, `EdaIuScale` (`PCB_IU_SCALE`: `mm_to_iu`, `IU_PER_PS`),
  `UnitsProvider { iu_scale, user_units }` with `message_text_from_value( f64, add_units,
  data_type )` (`common/eda_units.cpp:L408-`: `%.4f` mm / inch, `%.2f` mils, scientific
  fallback when a non-zero value prints as zero, unit label appended), `string_from_value`
  (higher precision form), the `EdaAngle` overloads, `message_text_from_unscaled_value`;
  `to_user_unit`, `double_value_from_string( scale, units, text )`,
  `double_value_from_string( text )`, `get_label( units )` (`mm`, `mils`, `in`),
  `format_internal_units`.
* `EdaAngle` (`arccos`, `as_degrees`, constants).
* String matching exactly as wx: `wxString::Matches( pattern )` (glob `*`/`?`, case sensitive)
  and `WildCompareString( pattern, text, case_sensitive )`; case-insensitive equality
  (`IsSameAs( s, false )`, `CmpNoCase`) - define the case-folding rule once (wx uses the C
  library's per-character lower-casing).
* Text variables: `ExpandTextVars( text, resolver, flags )`, `ExpandEnvVarSubstitutions` (T2).
* `DescribeRef`, `UnescapeString`.
* `Severity` bit flags, `format_severities`, a `Reporter` trait (messages + severity,
  `has_message_of_severity`), optional `ProgressReporter` (the port may no-op progress but must
  keep cancellation out of the result path).
* **A port of libstdc++ `std::sort`** (introsort: median-of-3 quicksort to depth `2*log2(n)`,
  heapsort fallback, final insertion sort with threshold 16) and of `std::stable_sort`
  semantics (Rust `sort_by` is stable and fine for the latter). Needed wherever C++ sorts with a
  comparator that has ties and the order is observable: netclass rule sort (4.2),
  `PACKED_RTREE` build.
* Advanced-config constants (`common/advanced_config.cpp`, defaults): `m_DRCEpsilon` 0.0005 mm,
  `m_HoleWallThickness` 0.020 mm, `m_ExtraClearance` 0.0005 mm, `m_SliverWidthTolerance` 0.08 mm,
  `m_SliverMinimumLength` 0.0008 mm, `m_SliverAngleTolerance` 20.0 deg. Expose as a config struct
  with these defaults (C++ reads an optional `kicad_advanced` file; the port takes parameters).
* A JSON writer that reproduces `nlohmann::json::dump( 4 )` (6.2).

### 5.2 `kicad-geom`

* `Vector2I` (`euclidean_norm`, `squared_euclidean_norm`, `squared_distance`, `resize`,
  `perpendicular`), `Vector2D` (`resize`, `dot`), `Box2I` (`inflate`, `intersects`, `contains`,
  `merge`, `normalize`, `centre`, `intersect`, `get_x/get_y/get_right/get_bottom`).
* `Seg`: `intersect( &Seg ) -> Option<Vector2I>` (default arguments as in C++),
  `nearest_points( &Seg ) -> ( ptA, ptB, dist_sq )`, `distance( &Seg )`, `square`, `reverse`.
* `Shape` (trait or enum): `collide( &Shape, clearance ) -> Option<( actual, location )>` (the
  two out-parameters are both used: `actual` in messages, `location` as marker position - the
  exact point chosen by each shape-pair collision routine is output), `collide_point`,
  `nearest_points( &Shape ) -> Option<( ptA, ptB )>`, `bbox`, `centre`, `type_`,
  `has_indexable_subshapes`, `indexable_subshapes`.
  Concrete shapes: segment (`seg`, `width`, `center`), circle, arc (`p0`, `arc_mid`, `p1`,
  `is_effective_line`), rect, simple polygon, line chain, compound (`shapes()`), null, poly-set,
  poly-set triangle (`segment_count`, `segment( i )`, `point_inside`).
* `ShapeLineChain`: `c_point`, `c_segment`, `segment_count`, `nearest_point( pt, false )`,
  `point_inside`, `intersect( &chain, &mut intersections, exclude_colinear, bbox )`, `bbox`,
  `collide_seg`, `append`, `area`.
* `ShapePolySet`: `outline_count`, `hole_count`, `outline( i )`, `is_empty`, `bbox`,
  `build_bbox_caches`, `bbox_from_caches`, `collide( shape | point | polyset, clearance )`,
  `contains( pt )`, `nearest_points( &Shape )`, `iterate_segments_with_holes`,
  `full_point_count`, `fracture`, `simplify`, `inflate` / `deflate( amount, CornerStrategy,
  max_error [, simplify] )`, `boolean_subtract`, `boolean_intersection` (Clipper2 behaviour),
  `append`, `clear_arcs`, `remove_all_contours`, `cache_triangulation`,
  `clone_drop_triangulation`, `area`.
* `PackedRTree<T>` (`libs/kimath/include/geometry/rtree/packed_rtree.h`): `Builder::{reserve,
  add( min, max, data ), build}`, `search( min, max, visitor ) -> count` with early exit,
  iteration in storage order, `is_empty`. **Same visiting order as C++** (Hilbert value
  computation, `std::sort`, fanout 16, child order).
* `rotate_point`, circle/oval/segment to polygon conversions with `ERROR_INSIDE/OUTSIDE`,
  `ARC_LOW_DEF` (0.02 mm), `ARC_HIGH_DEF` (0.005 mm).
* (T2) `VertexSet` (`geometry/vertex_set.h`) for connection width; polygon triangulation.

### 5.3 `kicad-pcb` (board model, settings, project)

Containers and their **order** (the order after loading is the order DRC walks):
`board.tracks()`, `footprints()`, `drawings()`, `zones()`, `markers()`, `generators()`;
`footprint.pads()`, `graphical_items()`, `fields()`, `zones()`.

Board: `design_settings()`, `project()` (text variables, tuning profiles),
`copper_layer_count()`, `enabled_layers()`, `is_layer_enabled`, `is_layer_visible`,
`layer_name( id )` (user name), `layer_id( name )`, the canonical + user layer-name table used by
layer patterns (`ENUM_MAP<PCB_LAYER_ID>` filled in `board_loader.cpp:L122-130`: canonical name
then user name for every layer), `find_net( name )`, `net_info()`, `resolve_item( Kiid )`,
`connectivity()`, `bounding_box()`, `file_name()`, `board_outline()` /
`update_board_outline()` / `board_polygon_outlines( &mut outline, false )` /
`outlines_chaining_epsilon()`, `pad_at( pos, layers )`, `current_variant()`,
`resolve_text_var`, `convert_cross_references_to_kiids` (T2),
`synchronize_nets_and_net_classes( false )`, `max_clearance_value()`, `stackup_or_default()`
(T2), component classes (T2), `LSET` helpers: `all_cu_mask( n )`, `all_layers_mask`,
`external_cu_mask`, `internal_cu_mask`, `front_mask`, `back_mask`, `front_board_tech_mask`,
`back_board_tech_mask`, `physical_layers_mask`, `seq()`, `ui_order()`, `extract_layer()`,
iteration in ascending id, `LSET::Name( id )`; `is_copper_layer`, `is_pcb_layer`,
`is_front_layer`, `is_back_layer`, `is_solder_mask_layer`; `LAYER_RANGE( F_Cu, B_Cu, n )`.

Every item: `type_()` (`KICAD_T`, with `BaseType` for dimensions and tables), `uuid`, `layer()`,
`layer_set()`, `is_on_layer`, `is_on_copper_layer`, `is_connected`, `has_hole`, `is_tented( layer )`,
`is_knockout`, `parent()`, `parent_footprint()`, `parent_group()`, `bounding_box()`,
`position()`, `center()`, `hit_test( pt, accuracy )`,
`effective_shape( layer = UNDEFINED, flashing = DEFAULT, usage = constraint type )`,
`effective_hole_shape( layer, usage )`,
`transform_shape_to_polygon( buffer, layer, clearance, max_error, error_loc )`,
**`item_description( units, full )`** (the text printed for every item in both reports - one
implementation per class: `pcbnew/pad.cpp:L2576`, `pcb_track.cpp:L196/L3129`, `zone.cpp:L1539`,
`footprint.cpp:L3311`, `pcb_shape.cpp:L1744`, `pcb_text.cpp:L672`, `pcb_textbox.cpp:L824`,
`pcb_field.cpp:L246`, `pcb_dimension.cpp:L968`, `pcb_table.cpp:L906`, `pcb_tablecell.cpp:L116`,
`pcb_barcode.cpp:L720`, `pcb_target.cpp:L122`, `pcb_group.cpp:L510`, `board.cpp:L2044`, ...),
`custom_property` (T2).

Effective-shape rules DRC relies on (verify in `pad.cpp:L1124-1210`, `pcb_track.cpp:L1451-1466`
and `L3256-3295`):

* pad on `Edge_Cuts`: a compound containing only the hole shape (PTH/NPTH) or a null shape;
* PTH pad not flashed on the layer: a compound containing the hole shape;
* `usage` selects enlarged holes: `HOLE_TO_HOLE` always uses the largest hole of the padstack
  (backdrill / post-machining); `HOLE_CLEARANCE`, `ANNULAR_WIDTH`, `PHYSICAL_CLEARANCE`,
  `SILK_CLEARANCE` only on backdrilled / post-machined layers;
* via: circle of the pad diameter on flashed layers (on `UNDEFINED_LAYER` the largest non-ghost
  layer diameter), else of the drill diameter; hole = zero-length segment of the drill width;
* zone: the triangulated fill of the layer (indexable sub-shapes = triangles); rule areas: the
  outline;
* solder mask layers add the mask expansion.

Connected items: `net()`, `net_code()`, `net_name()`, `short_net_name()`,
`effective_net_class()`, `clearance_overrides() -> Option<( i32, source )>`,
`local_clearance() -> Option<( i32, source )>` (sources: `pad`, `zone`, `footprint <ref>`).

Netclasses (`include/netclass.h`, `include/project/net_settings.h`): `name()`, `name_equals`,
`human_readable_name()`, `constituent_netclasses()`, `contains_netclass_with_name`, and for each
of clearance, track width, diff-pair width, diff-pair gap, via diameter, via drill, uvia
diameter, uvia drill, tuning profile: `has_x()`, `x()`, `x_parent()`; equality.
`NetSettings`: `default_netclass()`, `netclasses()` and `composite_netclasses()` as
`BTreeMap<String, _>`, effective netclass resolution for a net name (pattern assignments,
composite creation), `net_chain_class` (T3). `NetInfo`: `net_name`, `net_code`, `net_chain`,
`net_class`.

`BoardDesignSettings` (`pcbnew/board_design_settings.cpp`): `m_MinClearance`, `m_MinConn`,
`m_TrackMinWidth`, `m_ViasMinAnnularWidth`, `m_ViasMinSize`, `m_MinThroughDrill`,
`m_MicroViasMinSize`, `m_MicroViasMinDrill`, `m_CopperEdgeClearance`, `m_HoleClearance`,
`m_HoleToHoleMin`, `m_SilkClearance`, `m_MinResolvedSpokes`, `m_MinSilkTextHeight`,
`m_MinSilkTextThickness`, `m_SolderMaskExpansion`, `m_SolderMaskMinWidth`,
`m_SolderMaskToCopperClearance`, `m_SolderPasteMargin`, `m_SolderPasteMarginRatio`,
`m_MaxError`, via tenting flags, `m_DRCSeverities` (`BTreeMap<code, Severity>` with the
defaults of 3.6), `m_DrcExclusions`, `severity( code )`, `ignore( code )`, `drc_epsilon()`,
`hole_plating_thickness()`, `biggest_clearance_value( &engine )`, and the `.kicad_pro` loader
for `board.design_settings.rules.*` (constructor defaults vs. JSON defaults differ, see 7),
`rule_severities`, `drc_exclusions`.

Tracks / vias: `start`, `end`, `mid` (arc), `width`, `local_solder_mask_margin`,
`solder_mask_expansion`; via: `via_type`, `is_micro_via` / `is_blind_via` / `is_buried_via`,
`drill_value()`, `drill()`, `width( layer )`, `padstack()` (`mode()`,
`for_each_unique_layer`, `unique_layers()`, `effective_layer_for`, `max_hole_size()`),
`is_ghost_layer( layer )`, `flash_layer( layer )`, `layer_pair()`,
`is_backdrilled_or_post_machined( layer )`, (T2) `collect_microvia_columns( board )`.

Pads: `attribute` (PTH, SMD, CONN, NPTH, NON_COMPONENT), `property` (CASTELLATED, HEATSINK),
`number`, `drill_size`, `size( layer )`, `offset( layer )`, `shape( layer )`,
`chamfer_rect_ratio( layer )`, `orientation`, `flash_layer( layer )`, `is_free_pad`,
`same_logical_pad_as`, `principal_layer`, `bounding_box_on( layer )`, `max_error`,
`transform_hole_to_polygon`, `primitives( layer )`, local overrides: zone connection, thermal
gap, spoke width, solder mask margin, paste margin and ratio; `solder_mask_expansion( layer )`.

Footprints: `reference`, `fpid` (+ string), `is_net_tie`, `map_pad_numbers_to_net_tie_groups()`
(`BTreeMap<String, i32>`, value -1 = no group; note C++ uses `operator[]`, which yields 0 for a
missing pad number), `net_tie_pads( pad )`, `build_net_tie_cache()` /
`net_tie_cache( item ) -> &BTreeSet<net code>`, `build_courtyard_caches( handler )` /
`courtyard( F_CrtYd | B_CrtYd )` and the `MALFORMED_*_COURTYARD` flags, `allow_missing_courtyard`,
`pads_with_number( number, except )`, local clearance / zone connection / mask / paste
overrides, `is_flipped`, `transform().has_unit_scale()`, (T2) component class, sheet name,
fields, variants, `transform_pads_to_poly_set`, `transform_fp_shapes_to_poly_set`.

Zones: `is_rule_area`, `has_keepout_parameters_set`, the five `do_not_allow_*` flags,
`zone_name`, `is_teardrop_area`, `is_copper_thieving`, `is_filled`, `fill( layer )` /
`filled_polys_list( layer )`, `outline()`, `board_outline()` (footprint zones: outline in board
coordinates), `cache_bounding_box`, `cache_triangulation`, `local_clearance`, `min_thickness`,
`pad_connection`, `thermal_relief_gap`, `thermal_relief_spoke_width`,
`hit_test_filled_area( layer, pt, accuracy )`.

Shapes / text: `PcbShape`: `shape_type`, `stroke`, `is_solid_fill`, `corner_radius`,
`make_effective_shapes( true )`, the ability to derive edge segments/arcs with the same UUID
(4.10.5), `poly_shape`, `width`, `is_proxy_item`, `connection_points`, bezier points, mask
margins. Text items: `text`, `shown_text( FOR_ERC_DRC )`, `text_size`,
`effective_text_pen_width`, `is_visible`, `is_mirrored`, font kind. Tables: `cells()`.

Outline construction (T2, misc and courtyard providers; T1 for copper-outside-board and silk
disposition): `BuildBoardPolygonOutlines`, `TestBoardOutlinesGraphicItems`,
`OUTLINE_ERROR_HANDLER` (`pcbnew/convert_shape_list_to_polygon.cpp`, 1964 lines).

Footprint / pad sanity checks (T2): `pcbnew/footprint_utils.cpp`, `pcbnew/pad_utils.cpp`.

Property table (T2): for every `BOARD_ITEM` class the properties registered with
`PROPERTY_MANAGER` (display name, value kind, enum labels, availability predicate, masks and
inheritance), with case-insensitive name lookup and `Get` by item; plus `ENUM_MAP<KICAD_T>`
labels.

Project file (`.kicad_pro`): `board.design_settings` (rules, `rule_severities`,
`drc_exclusions`), `net_settings` (classes, patterns), `text_variables`, tuning profiles (T3).

### 5.4 `kicad-connectivity`

`build( board )`, `clear_ratsnest()`, `fill_isolated_islands_map( &mut map, true )` with
`IsolatedIslands { isolated_outlines: Vec<i32>, single_connection_outlines: Vec<i32> }`,
`test_track_endpoint_dangling( track, ignore_tracks_in_pads = true ) -> Option<pos>`,
`run_on_unconnected_edges( f )` in C++ order (nets in net order, edges in ratsnest order; edge:
source/target node -> parent item, position, dirty flag), `unconnected_count( false )`,
`connected_tracks( item )`, `connected_items( item, EXCLUDE_ZONES )`, the item entry /
`connected_items()` list of a track in C++ order (used to pick "the first" connected via/pad),
(T2) `from_to_cache().rebuild( board )` / `is_on_from_to_path`.

### 5.5 `kicad-libeval` (T2)

Compiler + VM as in 4.8.2 with host hooks: `create_var_ref( var, field )`,
`create_func_call( name )`, unit resolver, error callback with source offset, preflight call of
functions at compile time, deferred values, `escape_string_literal`.

### 5.6 Others

* DSN-style lexer for `.kicad_dru` (T2): token classes (left, right, number, symbol, quoted
  string), `cur_text`, `cur_line`, `cur_line_number`, `cur_offset`, `cur_separator`, keyword
  lookup of the current string (`GetCurStrAsToken`). `kicad-sexpr` may or may not expose this;
  the parser needs token-level access with source positions.
* `kicad-zone`: `fill_all_zones` for `--refill-zones` (optional; the CLI can reject the flag
  until the filler exists).

---

## 6. Output formats

### 6.1 Text report (`DRC_REPORT::WriteTextReport`, `drc_report.cpp:L57-142`)

Written with C locale. `<units>` formatting = `UNITS_PROVIDER( pcbIUScale, reportUnits )`.

```
** Drc report for <board file name incl. extension> **
** Created on <local time, ISO 8601 "YYYY-MM-DDTHH:MM:SS"> **
** Report includes: <formatSeverities( mask )> **

** Found <n> DRC violations **
<entry>...

** Found <n> unconnected pads **
<entry>...

** Found <n> Footprint errors **
<entry>...

** Ignored checks **
    - <title of each ignored check, allItemTypes order>
    (or "    - None" when nothing is ignored)

** End of Report **
```

`formatSeverities`: `Errors`, `Warnings`, `Exclusions` joined by `, ` in that order, or `None`.
The three sections are the three marker providers of 1.2 step 11, each filtered and sorted as
in 4.11. An **ignored check** is a code from `GetItemsWithSeverities()` with `bds.Ignore( code )`.

Entry (`RC_ITEM::ShowReport`, `common/rc_item.cpp:L107-155`), all lines end with `\n`:

```
[<settings key>]: <error message>
    <rule description>; <severity>[ (excluded)]
    @(<x>, <y>): <main item description>
    @(<x>, <y>): <aux item description>
    <exclusion comment>
```

* error message = `StripMarkup( GetErrorMessage( false ) )` (title, or title + detail);
  `StripMarkup` removes `[label](href)` style hyperlink markup
  (`common/widgets/wx_data_view_hyperlink_renderer.cpp:L119-`).
* rule description = `Rule: <rule name>` or `Local override` (also for violations that have no
  rule at all, e.g. unconnected items).
* severity word: `error`, `warning`, `exclusion`, `info`, `action`, `debug`.
* The coordinate is the **item's `GetPosition()`**, not the marker position:
  `@(%s, %s)` with `MessageTextFromValue( x )`, `MessageTextFromValue( y )` (e.g.
  `@(148.5000 mm, 92.7100 mm)`).
* Only item ids 0 and 1 are printed, each only if it resolves to a board item
  (`BOARD::ResolveItem`); with no resolvable main item only the first two lines are printed.
* The comment line appears only for excluded markers with a non-empty comment.

### 6.2 JSON report (`DRC_REPORT::WriteJsonReport`, `drc_report.cpp:L145-226`)

Schema `resources/schemas/drc.v1.json`. Serialised with `nlohmann::json` (`std::map` object:
**keys sorted alphabetically**), `std::setw( 4 )` (4-space indent), followed by `\n`.

```json
{
    "$schema": "https://schemas.kicad.org/drc.v1.json",
    "coordinate_units": "mm",
    "date": "2026-11-09T12:34:56",
    "ignored_checks": [
        {
            "description": "Footprint has no courtyard defined",
            "key": "missing_courtyard"
        }
    ],
    "included_severities": [
        "error",
        "warning"
    ],
    "kicad_version": "10.99.0",
    "schematic_parity": [],
    "source": "board.kicad_pcb",
    "unconnected_items": [],
    "violations": [
        {
            "description": "Clearance violation (netclass 'Default' clearance 0.2000 mm; actual 0.1500 mm)",
            "items": [
                {
                    "description": "<item description>",
                    "pos": {
                        "x": 148.5,
                        "y": 92.71
                    },
                    "uuid": "<item uuid>"
                }
            ],
            "severity": "error",
            "type": "clearance"
        }
    ]
}
```

* The `type` member of the report base (`"drc"`) is **not** serialised (not in the macro list).
* `coordinate_units` = `EDA_UNIT_UTILS::GetLabel( units )`: `mm`, `mils`, `in`.
* `kicad_version` = `GetMajorMinorPatchVersion()` (`KICAD_MAJOR_MINOR_PATCH_VERSION`, expected
  `10.99.0` for this tree - confirm with the oracle). `date` as in the text report. Both must be
  parameters of the Rust writer.
* `included_severities`: `error`, `warning`, `exclusion` in that order as far as in the mask.
* A violation has `excluded: true` and `comment` only when excluded
  (`rc_json_schema.h:L64-75`).
* `pos` = `ToUserUnit( iuScale, units, item position )` as double. nlohmann prints doubles with
  the shortest round-trip digits (Grisu2), always with a `.0` for integral values (`100.0`),
  fixed notation for decimal exponents in [-4, 15), otherwise `d.ddde-05` style with at least
  two exponent digits. Port `nlohmann::detail::dtoa_impl::format_buffer`; Rust's `{}` / `ryu`
  differ (`100`, `1e-5`).
* Strings: UTF-8 as is, JSON escapes for `"`, `\` and control characters only.
* Items: main then aux, as in the text report.

### 6.3 Messages

Messages are English (`_()` untranslated in the CLI). The complete texts are quoted in 4.10 for
tier 1; for other providers copy them from the source. Values inside messages use the report
units (`formatMsg`, 4.9), i.e. the report text changes with `--units`.

### 6.4 Exclusions in the project file (`drc_exclusion.cpp`, `pcb_marker.cpp:L103-221`)

`.kicad_pro` -> `board.design_settings.drc_exclusions`: array of ProtoJSON objects of
`kiapi.board.DrcExclusion` (`api/proto/board/board_rules.proto:L214-258`, field names
preserved):

```json
{ "marker": { "error_type": "DRCET_CLEARANCE",
              "position": { "x_nm": "123", "y_nm": "-456" },
              "items": [ { "value": "<uuid>" }, { "value": "<uuid>" } ],
              "layer": "BL_F_Cu",
              "marker_type": "MT_DRC" },
  "comment": "..." }
```

(int64 values are JSON strings; default-valued fields are omitted.) The enum names for
`error_type` are in `pcbnew/api/api_pcb_enums.cpp:L2377-2500` (both directions) (`DRCE_x` <-> `DRCET_x`); marker
types `MT_DRC`, `MT_DRAWING_SHEET`, `MT_RATSNEST`, `MT_PARITY`; layers `BL_*`.

Identity of an exclusion / marker (`DRC_EXCLUSION::GetSortKey`): the canonical marker message
with `violating_rule`, `description`, `severity`, `excluded` cleared, i.e. the tuple
(error type, position x/y, the non-nil item ids in order, layer, marker type). The layer is only
stored for `copper_sliver`, `generic_warning`, `generic_error`, `unconnected_items`,
`starved_thermal` (`UNDEFINED_LAYER` stored as `F_Cu`) and is **ignored for
`unconnected_items`**. The C++ set order is the order of the protobuf wire bytes; the port needs
equality only (use a struct key with a deterministic `Ord`; the order is only visible in the
order of exclusions when the project file is rewritten).

No protobuf dependency is needed: implement the tuple and its JSON mapping directly.

---

## 7. Backward compatibility

* **Legacy exclusions** (design-settings schema < 3, `migrateSchema2to3`,
  `board_design_settings.cpp:L1483-1521`): entries may be strings or `[string, comment]` arrays
  of the pipe-separated form `key|x|y|uuid1|uuid2[|...]`; parsed by
  `PCB_MARKER::FromLegacyString` (`pcb_marker.cpp:L224-292`): fewer than 5 fields -> dropped;
  unknown key -> dropped; `copper_sliver` / `generic_*`: one item + layer name;
  `unconnected_items`: 5 fields -> items 3,4; >= 7 fields -> items 5,6 (9.0.4+ added sortable
  ids); `starved_thermal`: optional layer as 6th field; `unresolved_variable` with two empty
  ids -> drawing-sheet marker.
* **Severity keys** (`board_design_settings.cpp:L361-400`): `hole_near_hole` (V8) is read first
  and then overridden by `hole_to_hole`; `legacy_no_courtyard_defined` and
  `legacy_courtyards_overlap` booleans map to `missing_courtyard` / `courtyards_overlap` error or
  ignore (L1545-1578). Unknown keys are ignored; missing keys keep the constructor default.
* **Rule minima defaults**: when the project file has no value, `PARAM_SCALED` defaults apply
  (e.g. `rules.min_via_annular_width` defaults to `DEFAULT_VIASMINSIZE` = 0.5 mm in the
  parameter table, L304-306, while the constructor sets ( 0.5 - 0.3 ) / 2 = 0.1 mm); when there
  is no project at all the constructor values apply. Out-of-range values fall back to the
  default. `min_copper_edge_clearance` may be -0.01 mm ("legacy: use edge cut thickness").
  The project-settings port must reproduce `PARAM_SCALED` semantics exactly; DRC results on
  boards without a `.kicad_pro` depend on it.
* **Rule file**: deprecated constraint names `mechanical_clearance` -> `physical_clearance`,
  `mechanical_hole_clearance` -> `physical_hole_clearance`, `hole` -> `hole_size`
  (warning only with a reporter); deprecated functions `insideCourtyard`,
  `insideFrontCourtyard`, `insideBackCourtyard`, `insideArea` (= `intersectsKeepout`
  semantics), `memberOf`; property aliases `Origin X/Y` -> `Start X/Y`; `track_angle` values
  without unit are degrees; `(version N)` required but N is not interpreted.
* **Old boards**: the regression boards are format versions 20200512 ... 20261022. Items without
  UUID (very old files) get generated ids; see R7. Legacy netclasses stored in the board are
  moved into the project net settings by `BOARD::SetProject` (`board.cpp:L378-417`).
* `DRC_ITEM::Create( key )` returns null for unknown keys ("old-format exclusions are dropped").

---

## 8. Multi-threading in C++ and the sequential port

C++ uses the global `thread_pool` (`GetKiCadThreadPool()`), `submit_loop( 0, n, f [, blocks] )`
/ `submit_task`, and waits on the futures while pumping progress. Shared state is guarded by
mutexes (`m_errorLimitsMutex`, the handler lock, board `m_CachesMutex`, provider-local mutexes)
and a thread-local expression context (`drc_rule_condition.cpp:L59-93`).

| Where | Parallel unit | Shared mutable state |
|---|---|---|
| cache generator | copper R-tree build in one task; one task per zone | board caches (independent per zone) |
| copper clearance: tracks | one task per track | error limits, `freePadsUsageMap` |
| copper clearance: pads | one task per footprint | error limits |
| copper clearance: graphics | one task per board drawing / per footprint | error limits |
| copper clearance: zone-zone | one task per candidate pair | error limits |
| edge clearance | one task per work unit; copper-outside-board per item | error limits, `m_emittedEdgeReports`, `m_silkDisposition` |
| disallow | per (keepout, zone) pair; per item | predicate caches, item `HOLE_PROXY` flag |
| misc: assertions | per item | error limits |
| zone connections | per (zone, layer) | error limits |
| track angle, track segment length | per track | error limits |
| sliver | per layer | - |
| solder mask | per item, results sorted afterwards | pair map, aperture map |
| connection width | per (net, layer) | error limits |
| creepage utils | (4 pool uses) | - |
| `InitializeClearanceCache` | blocks of items | GUI only |

Port rule: replace every `submit_loop( 0, n, f )` by `for i in 0..n { f( i ) }` and every batch
of `submit_task`s by running the closures in submission order. This yields one of the valid C++
interleavings (the one where tasks complete in index order), so the *set* of violations equals
a possible C++ result whenever no limit is exceeded and no order-dependent shared state is
involved; the cases where C++ itself is non-deterministic are listed in 4.12. Consequences for
the design:

* `ReportViolation` needs no lock; error limits are plain counters; the "exact cap" property
  (test `CapIsExact`) holds trivially.
* The expression context can be a reusable scratch object owned by the engine (`&mut`), or be
  created per evaluation; nested evaluation (a predicate that evaluates rules) must not reuse
  the outer context.
* `HOLE_PROXY` becomes a field of the evaluation context instead of an item flag.
* Progress / cancellation: keep `report_phase` / `report_progress` hooks (no-ops by default);
  all "wait 250 ms and report progress" loops disappear.
* Board caches behind `m_CachesMutex` become plain fields of the per-run cache struct; since
  `EvalRules` takes `&self` in many call chains, use interior mutability (`RefCell`) for the
  memo caches or pass `&mut DrcBoardCaches` explicitly. No `Mutex` (wasm).
* An optional `parallel` feature later must reproduce the sequential order by collecting
  per-index results and replaying them in index order (including limit accounting and the
  free-pad map, which then needs a sequential pre-pass).

---

## 9. Existing tests and data; differential tests

### 9.1 How the C++ tests load boards

`KI_TEST::LoadBoard( settingsManager, "<rel>", board )` (`qa/pcbnew_utils/board_test_utils.cpp:L90-190`),
paths relative to `qa/data/pcbnew/`: loads `<rel>.kicad_pro` if present (else `.pro`, else no
project), reads `<rel>.kicad_pcb`, `SetProject`, creates the engine and calls
`InitEngine( <rel>.kicad_dru )` if that file exists (else with an empty path), then
`BuildListOfNets`, `BuildConnectivity`, tuning-profile sync, component-class sync. This differs
slightly from the CLI loader (1.2): no `ResolveDRCExclusions( true )`, no
`SynchronizeNetsAndNetClasses( true )` after connectivity.

`KI_TEST::FillZones( board )` (same file, L303-320) refills all zones with `ZONE_FILLER` and
rebuilds connectivity: **tests that call it need `kicad-zone`** (marked Z below).

All DRC tests run `bds.m_DRCEngine->RunTests( EDA_UNITS::MM, true, false )`
(`reportAllTrackErrors = true`, no schematic parity) with a violation handler that collects
`DRC_ITEM`s, after overriding some severities in `bds.m_DRCSeverities`.

### 9.2 `qa/tests/pcbnew/drc/test_drc_regressions.cpp` - full tables

Column "pro/dru" = a `.kicad_pro` / `.kicad_dru` exists next to the board (a `.kicad_dru` means
the test needs the tier-2 rule parser and expression functions). "ver" = board file version.

**`DRCFalsePositiveRegressions`**: for each board, severities set to ignore:
`invalid_outline`, `unconnected_items`, `copper_sliver`, `starved_thermal`,
`lib_footprint_issues`, `lib_footprint_mismatch`. Counted: violations whose **board-settings**
severity (`bds.GetSeverity( code )`) is error. Expected: 0. No zone refill. Because the
expectation is "no errors", these boards are valid tests for **any subset of providers**.

| Board | ver | pro | dru | Subject |
|---|---|---|---|---|
| `issue4139` | 20200512 | - | - | minimally spaced pads at 45 degrees |
| `issue4774` | 20210807 | y | - | shape collisions missing SH_POLY_SET |
| `issue5978` | 20201002 | y | - | hole clearance with non-copper pad |
| `issue5990` | 20230620 | y | - | edge clearance flagged although respected |
| `issue6443` | 20230620 | - | - | THT pads with selective inner copper layers |
| `issue7567` | 20220621 | y | y | disallow holes also caught SMD pads |
| `issue7975` | 20230620 | y | - | diff pair gap out of range |
| `issue8407` | 20210722 | y | - | arc in diff pair clearance |
| `issue10906` | 20220211 | - | - | solder mask bridge for only one object |
| `issue12609` | 20220914 | y | - | arc collision edge case |
| `issue14412` | 20221018 | y | - | mask bridge between pads of a net-tie group |
| `issue15280` | 20221018 | y | - | very wide spokes counted as one |
| `issue14008` | 20250926 | y | - | net-tie clearance |
| `issue17967/issue17967` | 20240225 | y | y | arc diff-pair coupling |
| `issue18203` | 20240108 | - | - | colliding arc and circle |
| `issue18839` | 20240108 | y | - | edge clearance between concentric arcs |
| `unconnected-netnames/unconnected-netnames` | 20240108 | y | - | false schematic parity error |
| `net_tie_drc` | 20240108 | - | - | net tie bridging solder mask |
| `net_tie_hole_clearance` | 20261022 | - | - | net-tie hole clearance with layer-changing copper |
| `issue24974` | 20260206 | - | - | net-tie graphic copper on last pad, UUID-order independent |
| `diff_pair_uncoupled_tuning_drc` | 20260206 | y | y | tuning pattern length counted as uncoupled |

**`DRCFalseNegativeRegressions`**: severities set to ignore: `copper_sliver`,
`lib_footprint_issues`, `lib_footprint_mismatch`, `track_not_centered_on_via`, plus the per-board
extras. Counted: **every** violation delivered to the handler (errors and warnings alike) that
does not match an exclusion stored in the project (`bds.m_DrcExclusions`, identity of 6.4 built
from `PCB_MARKER( item, pos )`). Expected count must match exactly, so the full provider set is
needed; for tier 1 derive per-type expectations from the oracle (9.4).

| Board | Expected | ver | pro | dru | Extra ignores / subject |
|---|---|---|---|---|---|
| `issue1358` | 2 | 20210807 | y | - | |
| `issue2512` | 5 | 20210812 | y | y | |
| `issue2528` | 1 | 20241228 | y | - | |
| `issue5750` | 4 | 20230620 | - | - | shorting zone fills |
| `issue5854` | 3 | 20210722 | y | - | |
| `issue6879` | 6 | 20210722 | y | y | |
| `issue6945` | 2 | 20210722 | y | y | |
| `issue7241` | 1 | 20210722 | - | - | |
| `issue7267` | 6 | 20210722 | y | - | track wholly inside a footprint slot |
| `issue7325` | 2 | 20250926 | y | y | |
| `issue8003` | 2 | 20210722 | y | - | |
| `issue9081` | 2 | 20210824 | y | - | |
| `issue12109` | 8 | 20211014 | y | - | pads fail annular width |
| `issue14334` | 2 | 20221018 | y | - | thermal spoke to otherwise unconnected island |
| `issue16566` | 6 | 20231231 | y | y | `Pad_Shape` vs `Shape` property |
| `issue18142` | 1 | 20240225 | y | - | ignore `microvia_crosses_core`; blind/buried via to microvia hole-to-hole |
| `reverse_via` | 3 | 20220914 | y | - | via/track ordering |
| `intersectingzones` | 1 | 20230620 | y | - | zones too close |
| `fill_bad` | 1 | 20230620 | y | - | zone max bbox too small |
| `issue18878` | 12 | 20241007 | y | - | all cross-net mask bridge pairs |
| `issue19325/issue19325` | 4 | 20250926 | y | - | ignore `hole_to_hole`; overlapping pad annular ring |
| `issue22102` | 2 | 20250926 | y | - | ignore `unconnected_items`, `track_dangling`; arc-rect, colocated arcs |
| `issue11814` | 2 | 20250401 | y | y | teardrop clearance to pad |

**`DRCZoneFalsePositiveRegressions`** (Z): `issue19090/issue19090` (20240108),
`issue23467/issue23467` (20260206), both with `.kicad_pro`; ignores `invalid_outline`,
`unconnected_items`, `copper_sliver`, lib codes; `starved_thermal` forced to error; expected 0
error-severity violations.

**`DRCTeardropOverCopperField`** (Z): `issue24649`; exactly 1 `shorting_items` violation that
involves a `PCB_FIELD` (the hidden knockout field must not count).

**`DRCHoleToHoleReportsRuleValue`**: `issue22267` (20241229, pro): at least one `hole_to_hole`
violation; every message contains `MessageTextFromValue( bds.m_HoleToHoleMin )` and not the
epsilon-relaxed value.

### 9.3 Other tests in `qa/tests/pcbnew/drc/`

Tier-1 relevant:

| Test file | Board(s) under `qa/data/pcbnew/` | Assertion | Needs |
|---|---|---|---|
| `test_drc_error_limit.cpp` | none | `CapIsExact`: once `IsErrorLimitExceeded( code )` the handler is never called again; `InitEngineResetsLastErrorCode` | engine |
| `test_drc_prepared_candidates.cpp` | `issue12609`, `issue6443`, `issue22102`, `issue5978`, `issue14008`, `reverse_via`, `fill_bad`, `intersectingzones`, `issue5750` | prepared-copper query == legacy `QueryColliding` (same items, same order of filter calls); padding credit; `DRC_ZONE_INDEX::Query` == ordered linear scan; `CollectOverlappingPairs` == nested loop | R-tree, zone index |
| `test_drc_hole_clearance_issue24355.cpp` | `issue24355/issue24355` | 2 `hole_clearance` (independent of pad UUID order) | copper clearance |
| `test_drc_hole_to_hole_npth_slot_issue24541.cpp` | `issue24541/issue24541` | exactly 1 NPTH slot pair reported | hole-to-hole |
| `test_drc_arc_arc_edge_clearance.cpp` | `drc_arc_arc_edge_clearance/...` | >= 1 `copper_edge_clearance`, one involving the track and the cutout | edge clearance |
| `test_drc_copper_outside_board_issue14650.cpp` | `issue14650/KiCAD_Fiducial` | exactly 1 `copper_outside_board`, pad of `FID2` | edge clearance |
| `test_drc_npth_slot_clearance_issue24799.cpp` (Z) | `issue24799/issue24799` | 0 edge clearance, 0 hole clearance | edge, copper clearance |
| `test_drc_annular_overlap_issue24340.cpp` | `issue24340/issue24340` | `annular_width`: 0 on the "bug" footprint, 1 on the control footprint | annular width |
| `test_drc_copper_graphics.cpp` | `test_copper_graphics` (4 violations after exclusions, all codes); `line_ending_drc/line_ending_drc_fail` (123 `clearance` + `shorting_items`), `.../line_ending_drc_pass` (0) (both have `.kicad_dru`); one programmatic board | copper clearance (graphics) |
| `test_drc_keepout_disallow.cpp` | `keepout_disallow/keepout_disallow`: 3 `items_not_allowed` (2 vias, 1 track); `issue24924/keepout_self_exclude`: a footprint's own keepout never flags it (duplicate zone UUID on the board); `keepout_no_clearance/keepout_no_clearance` (dru with a 20 mm physical clearance): keepouts are not inflated | disallow |
| `test_drc_issue24525.cpp` | `issue24525/issue24525` | 5 `text_on_edge_cuts`: text 1, dimension 1, table 2, barcode 1, reference image 0 | disallow |
| `test_drc_copper_thieving.cpp` | `drc_copper_thieving/copper_thieving.kicad_pcb` | 0 `isolated_copper` | connectivity |
| `test_drc_physical_clearance.cpp` | `issue4139` | `m_DRCMaxPhysicalClearance == 0` after a run | cache generator |
| `test_drc_netclass_literal.cpp` | `connection_width_rules` (netclass renamed to end in a backslash) | 1 `generic_warning`; `EvalRules` clearance parent rule has `m_ImplicitNetclass == "Default"` | engine, misc |
| `test_drc_unconnected_save.cpp`, `test_drc_unconnected_items_exclusion_loss.cpp` | programmatic; `issue17429` (10 exclusions), `issue25101` | exclusion identity, record/resolve round trip, project round trip | exclusions |
| `test_drc_violation_layers_issue24941.cpp` | programmatic | `DRC_ITEM::GetViolationLayers` (GUI helper) | optional |

Tier 2 (rule files, other providers):

| Test file | Board(s) | Assertion |
|---|---|---|
| `test_custom_rule_severities.cpp` (Z) | `severities` (dru) | 8 violations (ignoring lib codes, `footprint_type_mismatch`) |
| `test_drc_via_dangling.cpp` | `via_dangling` (dru) | exactly 1 violation, a `via_dangling` |
| `test_drc_multi_netclasses.cpp` (Z) | `multinetclasses_drc` (dru) | 2 (`track_width` forced to error) |
| `test_drc_component_classes.cpp` (Z) | `component_classes_drc` (dru) | 2 (`assertion_failure`) |
| `test_drc_orientation.cpp` (Z) | `orientation_drc` (dru) | 1 (`assertion_failure`) |
| `test_drc_issue23469.cpp` | `issue23469/issue23469` (dru) | 0 edge / hole clearance violations for `J1` |
| `test_drc_issue24211.cpp` | `issue24211/issue24211` (dru, via `BOARD_LOADER`) | component classes assigned at load; both custom rules fire |
| `test_drc_issue24264.cpp` | `issue24264/issue24264` (dru) | `(severity exclusion)` rule: all courtyard markers treated as excluded |
| `test_drc_issue23868.cpp` | `issue23868/issue23868` (dru) | rules valid after load; no violation of rule `diff_propagation_delay` |
| `test_drc_invalid_outline_issue24078.cpp`, `..._issue24241.cpp` | `issue24078/...` (dru), `issue24241/...` | >= 1 `invalid_outline`; marker linked to the degenerate arc |
| `test_drc_text_var_issue24442.cpp` | `issue24442` | 3 `generic_error`, 2 `generic_warning`, 0 `unresolved_variable` |
| `test_drc_courtyard_invalid.cpp`, `test_drc_courtyard_overlap.cpp` | `drc_courtyard/invalid/*.kicad_pcb` (5), `drc_courtyard/overlap/*.kicad_pcb` (9) | expected footprint pairs / malformed footprints per case table in the test |
| `test_footprint_courtyard_chaining.cpp`, `test_footprint_courtyard_index.cpp` | programmatic | courtyard closing; index == brute force |
| `test_drc_incorrect_text_mirror.cpp` (Z) | `incorrect_text_mirroring_drc` | 8 |
| `test_drc_starved_thermal.cpp` (Z) | `test_starved_thermal` 2; `issue19090/issue19090` 0 | |
| `test_drc_silk_rule_area_issue24177.cpp` | `issue24177/silk_rule_area.kicad_pcb` | 0 silk / mask / edge / bridge violations |
| `test_solder_mask_bridging.cpp` (Z) | `solder_mask_bridge_test` 4; `issue24951/issue24951` determinism over repeated runs | |
| `test_solder_mask_bridge_all_tracks.cpp`, `test_solder_mask_track_to_pad.cpp`, `test_drc_solder_mask_npth_bridge.cpp` (Z), `test_drc_solder_mask_expansion.cpp` | `soldermask_bridge_all_tracks` (> 5, >= 4 distinct net-A items), `soldermask_track_to_pad` (>= 1, a track), `npth_mask_bridge_test` (0), `issue22751/issue22751`, `stonehenge` | |
| `test_drc_copper_sliver.cpp` (Z) | `sliver` 1; `issue14449` 0; `issue14549` 0; `issue14549_2` 0; `issue14559` 0 | |
| `test_drc_copper_conn.cpp` (Z) | `issue9870` 12; `connection_width_rules` 3 (dru); `issue12831` 0; `issue14130` 0 | |
| `test_drc_via_stack.cpp` | `drc_via_stack/*.kicad_pcb` + generated rule files | per-case counts of the five via-stack codes |
| `test_drc_tuning_profiles.cpp` (Z) | `drc_missing_tuning_profile` | 2 |

Tier 3: `test_drc_lengths.cpp` (`length_calculations`, `time_calculations`, `issue22536`,
`via_off_center`: 0 each), `test_drc_skew.cpp` (`skew_within_diff_pairs_drc` 2,
`skew_group_matched_drc` 5), `test_drc_skew_arc.cpp`, `test_drc_length_item_order.cpp`,
`test_drc_length_with_chain.cpp`, `test_drc_chain_*.cpp`, `test_drc_stub_length.cpp` (its two
parser-only cases `StubLengthAcceptsTimeDomainUnits` and `ReturnPathDoesNotCarryTimeDomainOption`
belong to the rule-parser package), `test_drc_return_path_*.cpp`,
`test_drc_net_chain_tuning_profiles.cpp` (1), `test_drc_tuner_*.cpp`, all `test_drc_creepage*`
and `test_creepage_engine_*` files, `test_drc_backdrill_postmachining.cpp` (mostly board-model
tests of hole shapes; its DRC cases check `track_on_post_machined_layer`).

Out of scope: `test_drc_rule_editor.cpp` (GUI rule editor).

Related tests elsewhere: `qa/tests/pcbnew/test_libeval_compiler.cpp` (843 lines: expression
tables `simpleExpressions`, `introspectionExpressions`, property kinds, renamed properties,
availability, parent navigation, argument validation, pair-item rules) for the tier-2 expression
packages; `qa/tests/libs/kimath/geometry/test_packed_rtree.cpp` (geometry crate);
`qa/tests/cli/test_pcb.py::test_pcb_drc_rejects_unavailable_second_item` (a rule file using `B`
in a unary rule or in an assertion -> exit code 3, message `Item 'B' is not available`, no
report written); `qa/tests/pcbnew/test_board_design_settings.cpp` (severity / exclusion
persistence); `qa/tests/cli/test_downgrade.py` (JSON shapes of old and new exclusions).

There are **no golden DRC report files** in `qa/data`.

### 9.4 Differential tests against the reference `kicad-cli`

Oracle: `/home/user/kicad-oracle/bin/kicad-cli pcb drc ...` (hermetic HOME, English). Copy the
board with its `.kicad_pro` / `.kicad_dru` / `.kicad_prl` into a scratch directory first (the
CLI may write project side files next to the input).

1. **JSON equality as multisets.** For every board under `qa/data/pcbnew` (and `demos`):
   `pcb drc --format json --severity-all --units mm -o out.json <board>` with both tools.
   Normalise: drop `date`; within each of `violations`, `unconnected_items`, `schematic_parity`
   sort by (`type`, `description`, items' `uuid`s, items' `pos`); compare. Report order itself is
   not comparable (4.12). Run each oracle board twice to detect boards whose C++ result is
   itself unstable and put those on a "count only" list.
2. **Provider subsets.** While providers are missing, filter both sides to the set of `type`s
   already ported; always filter `lib_footprint_issues` / `lib_footprint_mismatch` unless the
   library-parity stub is enabled and shown to match.
3. **Limit handling.** If a `type` has 199 (or 499 for `clearance` / `unconnected_items`)
   entries on either side, compare only the count for that type.
4. **Legacy UUIDs.** For boards where the oracle output contains UUIDs that are not in the board
   file (generated at load), compare items by (`description`, `pos`) instead of `uuid`.
5. **Golden files.** Commit the normalised oracle JSON of the 21 + 23 regression boards and the
   tier-1 test boards of 9.3 under `crates/kicad-drc/tests/golden/` (small), so the default test
   run needs no oracle. From them derive the per-type counts that replace the "total count"
   expectations of `DRCFalseNegativeRegressions` while providers are missing.
6. **Flags.** A few boards additionally with `--all-track-errors`, with `--units in` and
   `--units mils` (number formatting), with `--format report` (parse entries, compare the header
   lines, the three counts and the ignored-checks list byte for byte, entries as a multiset),
   with only `--severity-error`, and with `--exit-code-violations` (exit code 0 vs 5).
7. **Exclusions.** `issue17429`, `issue25101` (projects with stored exclusions): compare with
   `--severity-exclusions` (entries flagged `excluded` with their comment).
8. **Rule file errors (tier 2).** The two rules of `test_pcb.py`: exit code 3, no report file,
   message text.
9. **Provider order.** Once: a board with many violation kinds, text report, check that entries
   of the same severity appear grouped by provider in the order of 4.4.
10. **Zone refill (when `kicad-zone` exists).** `--refill-zones` on the Z boards.

---

## 10. Rust module layout and work packages

### 10.1 Modules (`crates/kicad-drc/src/`, one module per C++ file)

```
lib.rs
constraint_types.rs        drc_constraint_types.h
rule.rs                    drc_rule.h / drc_rule.cpp
rule_condition.rs          drc_rule_condition.h/.cpp (+ ImplicitCondition, 4.8.5)
rule_parser.rs             drc_rule_parser.h/.cpp                         (T2)
rules_lexer.rs             DSN lexer subset + drc_rules.keywords          (T2)
item.rs                    drc_item.h/.cpp  (codes, DrcItem, DrcItemsProvider)
rc_item.rs                 rc_item.h / rc_item.cpp (data, show_report, json_violation)
marker.rs                  marker_base.h, pcb_marker.h/.cpp (violation record, severity)
exclusion.rs               drc_exclusion.h/.cpp (+ legacy strings)
engine/mod.rs              drc_engine.h/.cpp: struct, init, run_tests, limits, report_violation
engine/implicit_rules.rs   loadImplicitRules
engine/eval_rules.rs       EvalRules, EvalZoneConnection, ProcessAssertions, Query*
engine/diff_pair.rs        MatchDpSuffix, IsNetADiffPair, IsNetTieExclusion
caches.rs                  the BOARD run-time caches of 3.8 (DrcBoardCaches)
rtree.rs                   drc_rtree.h
zone_index.rs              drc_zone_index.h
cache_generator.rs         drc_cache_generator.h/.cpp
test_provider.rs           drc_test_provider.h/.cpp (walk, helpers, provider order)
providers/<name>.rs        one per drc_test_provider_<name>.cpp
report.rs                  drc_report.h/.cpp, rc_json_schema.h
pcbexpr/evaluator.rs       pcbnew/pcbexpr_evaluator.h/.cpp                (T2)
pcbexpr/functions.rs       pcbnew/pcbexpr_functions.cpp (keepout part in T1)
length_report.rs, chain_topology.rs, creepage_utils.rs, creepage_engine.rs  (T3)
```

The CLI command lives in `kicad-cli` (`pcb drc`), mirroring `command_pcb_drc.cpp` and
`JobExportDrc`.

Design notes for all packages:

* Items are referred to by the handle type defined in `rust/docs/design/` (not pointers);
  wherever C++ keys a map by pointer use the handle; wherever it *orders* by pointer see 4.12.
* `EvalRules` needs read access to board, design settings, net settings, rules and mutable
  access to the memo caches. Suggested shape: `engine.eval_rules( &ctx, type, a, b, layer )`
  with `ctx: &DrcContext { board, caches: &RefCell<DrcBoardCaches> / explicit &mut }`.
* `loadImplicitRules` calls `SynchronizeNetsAndNetClasses( false )` on the board (a mutation);
  do this in the caller before `init_engine` or give `init_engine` `&mut Board` - decide in the
  board-model design and keep the call order of 1.2.
* The reporter argument of `EvalRules` stays in the signature as `Option<&mut dyn Reporter>`;
  tier 1 only implements the `None` path and marks each skipped `REPORT` with a short comment.

### 10.2 Work packages

Estimated C++ lines to port in parentheses. Tier 1 packages are ordered so that a thin slice
(board -> engine -> track width / via / hole / annular checks -> report -> CLI) works after
DRC-01, 03, 04, 06, 11, 12.

**Tier 1**

| Id | Title | C++ files | Depends on | Validating tests |
|---|---|---|---|---|
| DRC-01 | Types: constraints, rules, items, error-code table, severities (1100) | `drc_constraint_types.h`; `drc_rule.h`; `drc_rule.cpp:L32-142`; `drc_item.h`; `drc_item.cpp:L32-626`; `include/rc_item.h:L139-250`; `report_severity.h/.cpp` | - | unit tests on the code / key / title / default-severity table and `allItemTypes` order; `MinOptMax` semantics |
| DRC-02 | `DRC_RTREE`, zone index, provider base helpers (1540) | `drc_rtree.h`; `drc_zone_index.h`; `drc_test_provider.h/.cpp` | DRC-01 | `test_drc_prepared_candidates.cpp` (all 7 cases) |
| DRC-03 | Engine core: implicit rules, compile, init, limits, `ReportViolation`, `RunTests`, diff-pair and net-tie helpers, native implicit conditions (1500) | `drc_engine.h`; `drc_engine.cpp:L57-679, L724-794, L858-994, L2350-2656`; `drc_rule_condition.h/.cpp` | DRC-01 | `test_drc_error_limit.cpp`; `test_drc_netclass_literal.cpp` (rule part); unit tests: rule list of a board with several netclasses in the order of 4.2, `MatchDpSuffix` table |
| DRC-04 | `EvalRules` and friends (1350, about half is reporter text) | `drc_engine.cpp:L997-2344, L3212-3264` | DRC-03 | unit tests on programmatic boards for each step of 4.6 (net tie, overrides, fast path vs general path equality, last-rule-wins, footprint inheritance defaults, local clearance, board minimum); `issue14008`, `net_tie_hole_clearance` through DRC-08 |
| DRC-05 | Cache generator and run-time caches (500) | `drc_cache_generator.h/.cpp`; `board.cpp:L446-512, L1325-1353`; `board_design_settings.cpp:L1779-1810` | DRC-02, DRC-03 | `test_drc_physical_clearance.cpp`; zone list / R-tree sanity on `fill_bad`, `intersectingzones` |
| DRC-06 | Unary providers: track width, via diameter, hole size, annular width (1083) | `drc_test_provider_track_width.cpp`, `..._via_diameter.cpp`, `..._hole_size.cpp`, `..._annular_width.cpp` | DRC-04 | `test_drc_annular_overlap_issue24340.cpp`; `issue12109` (8), `issue19325/issue19325` (4); oracle per-type diffs |
| DRC-07 | Connectivity provider (333) | `drc_test_provider_connectivity.cpp` | DRC-05 | `test_drc_copper_thieving.cpp`; `issue14334`; oracle diffs of `unconnected_items`, `track_dangling`, `via_dangling`, `isolated_copper` |
| DRC-08a | Copper clearance: item-item and item-zone (1050) | `drc_test_provider_copper_clearance.cpp:L1-551, L635-1046` | DRC-02, DRC-04, DRC-05 | false-positive boards `issue4139`, `4774`, `5978`, `6443`, `12609`, `14008`, `18203`, `net_tie_hole_clearance`, `issue24974`; `test_drc_hole_clearance_issue24355.cpp`; oracle diffs on `issue1358`, `2528`, `5854`, `7241`, `7267`, `8003`, `9081`, `reverse_via` |
| DRC-08b | Copper clearance: graphics, knockout text, zone-zone, teardrops (560) | `..._copper_clearance.cpp:L552-632, L1048-1514` | DRC-08a | `test_drc_copper_graphics.cpp`; `intersectingzones` (1), `fill_bad` (1), `issue5750` (4); `DRCTeardropOverCopperField` (Z) |
| DRC-09 | Hole-to-hole and edge clearance (970) | `drc_test_provider_hole_to_hole.cpp`; `drc_test_provider_edge_clearance.cpp` | DRC-02, DRC-04, DRC-05 | `test_drc_hole_to_hole_npth_slot_issue24541.cpp`; `DRCHoleToHoleReportsRuleValue`; `issue18142`; `test_drc_arc_arc_edge_clearance.cpp`; `issue5990`, `issue18839`; `test_drc_copper_outside_board_issue14650.cpp` |
| DRC-10 | Disallow provider and keepout predicate (800) | `drc_test_provider_disallow.cpp`; `pcbexpr_functions.cpp:L553-959` | DRC-02, DRC-04, DRC-05 | `test_drc_keepout_disallow.cpp`; `test_drc_issue24525.cpp` |
| DRC-11 | Markers, exclusions, item providers, text and JSON reports (1100) | `drc_report.h/.cpp`; `rc_item.cpp:L37-200`; `rc_json_schema.h`; `drc_item.cpp:L713-772`; `pcb_marker.cpp:L52-93, L103-221, L224-292, L411-428`; `drc_exclusion.h/.cpp`; `board.cpp:L569-640`; `wx_data_view_hyperlink_renderer.cpp:L31-67, L119-170` | DRC-01 | `test_drc_unconnected_save.cpp`; `test_drc_unconnected_items_exclusion_loss.cpp`; byte-exact golden reports for a fixed violation list; schema validation against `drc.v1.json` |
| DRC-12 | CLI `pcb drc` (600, in `kicad-cli`) | `kicad/cli/command_pcb_drc.cpp`; `pcbnew_jobs_handler.cpp:L2852-3088`; `common/jobs/job_rc.*`, `job_pcb_drc.*`; `board_loader.cpp:L100-170` | DRC-06, DRC-11 | differential tests 9.4 (1-7) |

**Tier 2**

| Id | Title | C++ files | Depends on | Validating tests |
|---|---|---|---|---|
| DRC-20 | `.kicad_dru` parser and loader (1800) | `drc_rule_parser.h/.cpp`; `common/drc_rules.keywords`; the used part of `common/dsnlexer.cpp`; `drc_engine.cpp:L682-721`; `drc_rule.cpp:L32-36, L88-118` | DRC-21 | parse all 62 `.kicad_dru` files under `qa/data/pcbnew`; parser cases of `test_drc_stub_length.cpp`; `test_pcb.py` (exit 3) |
| DRC-21 | PCB expression binding (1500) | `pcbnew/pcbexpr_evaluator.h/.cpp`; `drc_rule_condition.cpp` (compiled path) | DRC-03, `kicad-libeval`, property table | `qa/tests/pcbnew/test_libeval_compiler.cpp`; native-vs-compiled agreement for all implicit conditions |
| DRC-22 | PCB expression functions (1560) | `pcbexpr_functions.cpp:L49-552, L961-1966` | DRC-21, DRC-10 | function cases of `test_libeval_compiler.cpp`; `issue7567`, `issue16566` (6), `issue6879` (6), `issue6945` (2), `issue7325` (2), `issue2512` (5); `test_custom_rule_severities.cpp`, `test_drc_via_dangling.cpp`, `test_drc_multi_netclasses.cpp`, `test_drc_component_classes.cpp`, `test_drc_orientation.cpp`, `test_drc_issue23469.cpp`, `test_drc_issue24211.cpp`, `test_drc_issue24264.cpp` |
| DRC-23 | Misc provider (outline, disabled layers, text variables, assertions) and footprint checks (1680) | `drc_test_provider_misc.cpp`; `drc_test_provider_footprint_checks.cpp`; `pcbnew/footprint_utils.cpp`; `pcbnew/pad_utils.cpp:L88-452` | DRC-05 (assertions: DRC-21) | `test_drc_invalid_outline_issue24078.cpp`, `..._issue24241.cpp`; `test_drc_text_var_issue24442.cpp`; `test_drc_netclass_literal.cpp` |
| DRC-24 | Courtyard clearance, text dimensions, text mirroring (820) | `..._courtyard_clearance.cpp`; `..._text_dims.cpp`; `..._text_mirroring.cpp` | DRC-04, DRC-05 | `test_drc_courtyard_invalid.cpp`; `test_drc_courtyard_overlap.cpp`; `test_drc_incorrect_text_mirror.cpp` |
| DRC-25 | Zone connections / starved thermals (371) | `..._zone_connections.cpp` | DRC-04, DRC-05 | `test_drc_starved_thermal.cpp`; `issue15280`, `issue14334`; `DRCZoneFalsePositiveRegressions` |
| DRC-26 | Silk clearance and physical clearance (1208) | `..._silk_clearance.cpp`; `..._physical_clearance.cpp` | DRC-02, DRC-04, DRC-05 | `test_drc_silk_rule_area_issue24177.cpp`; `test_drc_physical_clearance.cpp`; `keepout_no_clearance` |
| DRC-27 | Solder mask (1090) | `..._solder_mask.cpp` | DRC-02, DRC-04, DRC-05 | `test_solder_mask_*.cpp`, `test_drc_solder_mask_*.cpp`; `issue10906`, `issue14412`, `net_tie_drc`, `issue18878` (12) |
| DRC-28 | Track angle, track segment length, via stack (833) | `..._track_angle.cpp`; `..._track_segment_length.cpp`; `..._via_stack.cpp` | DRC-04, DRC-05 (rules: DRC-20) | `test_drc_via_stack.cpp` |
| DRC-29 | Copper sliver and connection width (874) | `..._sliver_checker.cpp`; `..._connection_width.cpp` | DRC-02, DRC-05 | `test_drc_copper_sliver.cpp`; `test_drc_copper_conn.cpp` |

**Tier 3**

| Id | Title | C++ files | Depends on | Validating tests |
|---|---|---|---|---|
| DRC-30 | Differential pair coupling (778) | `..._diff_pair_coupling.cpp` | DRC-04, DRC-05 | `issue7975`, `issue8407`, `issue17967/issue17967`, `diff_pair_uncoupled_tuning_drc` |
| DRC-31 | Matched length, skew, via count, net chains (2040) | `..._matched_length.cpp`; `drc_length_report.h`; `drc_chain_topology.h/.cpp` | DRC-04, DRC-05, DRC-20 | `test_drc_lengths.cpp`, `test_drc_skew*.cpp`, `test_drc_chain_*.cpp`, `test_drc_stub_length.cpp`, `test_drc_return_path_*.cpp`, `test_drc_length_*.cpp` |
| DRC-32 | Creepage utilities, part 1 (2500) | `drc_creepage_utils.h`; `drc_creepage_utils.cpp:L1-1600` | DRC-02 | `test_drc_creepage_circle_prune.cpp`, `test_drc_creepage_issue24524.cpp` |
| DRC-33 | Creepage utilities part 2, engine, provider (2540) | `drc_creepage_utils.cpp:L1601-3185`; `drc_creepage_engine.h/.cpp`; `..._creepage.cpp` | DRC-32, DRC-04, DRC-20 | all `test_drc_creepage*.cpp`, `test_creepage_engine_*.cpp` |
| DRC-34 | Leftovers: `EvalRules` reporter texts, own-clearance cache, net-chain tuning profiles, via stitch, library-parity stub, `GetViolationLayers` (900) | `drc_engine.cpp` REPORT branches and `L3267-3410`; `..._net_chain_tuning_profiles.cpp`; `..._via_stitch.cpp`; `..._library_parity.cpp:L1154-1216`; `drc_item.cpp:L628-710` | DRC-04 | `test_drc_net_chain_tuning_profiles.cpp`, `test_drc_violation_layers_issue24941.cpp` |

Not ported: `drc_interactive_courtyard_clearance.*`, `rule_editor/*`, "show matches"
(`drc_engine.cpp:L2659-3209`, `InitEngine( rule )`), `drc_rule.cpp` proto functions, schematic
parity, the library-comparison part of library parity, drawing-sheet text checks.

---

## 11. Risks, traps, open questions

**R1 - Provider order is inferred, not observed.** It comes from static-initialisation order
(4.4). Verify with the oracle before freezing the array; a wrong order changes report order and
which violations survive shared limits.

**R2 - `std::sort` with ties.** `PACKED_RTREE` sorts by Hilbert value with `std::sort`; a pad
and its hole, the sub-shapes of a compound and co-located items share a centre, so their
relative order - and therefore the R-tree visiting order, which decides "first hit" outcomes -
is whatever libstdc++'s introsort produces. The same applies to the netclass rule sort. Port
libstdc++'s `std::sort` once in `kicad-core` and use it wherever C++ uses `std::sort` with a
non-total comparator.

**R3 - The C++ output order is not deterministic** (thread scheduling, pointer-keyed maps,
4.12), and with more than 199 / 499 violations of one kind even the set is not. Differential
tests must compare multisets and special-case saturated types (9.4). The solder-mask provider
shows the upstream direction (collect, sort, then report); do not copy that pattern into other
providers on your own - just iterate in index order.

**R4 - Reports print item descriptions and item positions, not marker positions.** Byte
equality of reports therefore depends on `GetItemDescription` of every item class (5.3) and on
`MessageTextFromValue`. Until those are ported compare structurally (type, uuids). Conversely
the collision point returned by `Collide()` is not in any report, but it is the marker position
and thus part of the **exclusion identity** (6.4): a different-but-valid collision point silently
breaks exclusion matching. The `actual` distance is in the message (4 decimals in mm); when the
formatted constraint and actual are equal, `formatMsg` switches to a higher precision, so
off-by-one-nm differences can surface there.

**R5 - `EvalRules` subtleties**: later rules override earlier ones per field; the fast path must
give the same answer as the general path (it does unless explicit clearance rules or diff-pair
overrides exist - keep the gate exactly); overrides of exactly 0 fall through; the board minimum
is applied only when both items are copper; a tuning-profile rule with severity `ignore` as the
last applied rule disables the check for that item; the implicit courtyard constraint has no
value; `DRC_DISALLOW_VIAS` means through vias only, so implicit keepouts never flag
blind/buried/micro vias; `HOLE_TO_HOLE` with `b == nullptr` dereferences null in C++ (router
batch call only) - treat a missing `b` as "has no hole".

**R6 - Predicate caches are part of the semantics** (4.8.4): keys omit the hole-proxy flag and,
for the per-area cache, the constraint. Reproduce the keys; scope the caches to the same
lifetime (cleared by `IncrementTimeStamp`: at `InitEngine`, at `RunTests` start, in the cache
generator).

**R7 - Items without stored UUIDs** (old boards) get random UUIDs in C++; UUID order decides
which item of a pair is "main" and which pairs are tested from which side (filters such as
`track->m_Uuid > other->m_Uuid`). The set of violations is unaffected, the item order and
exclusion identity are. The Rust reader must generate deterministic ids; differential tests
must not key on such UUIDs.

**R8 - Library parity.** The reference CLI reports `lib_footprint_issues` warnings for
footprints whose library is not in its (hermetic) library table. Decide per 4.10.25 and filter
in tests.

**R9 - Property table for expressions** (tier 2): about 450 registrations with availability
predicates and enum label maps. It must come from `kicad-pcb`; without it custom rules cannot
compile. Extract it systematically (grep the `*_DESC` structs for `AddProperty`, `Mask`,
`OverrideAvailability`, `ENUM_MAP`), and test with `test_libeval_compiler.cpp`
(`ExpressionPropertyTypesAreSupported` iterates all properties).

**R10 - Zone fills.** DRC tests the fills stored in the file. Many C++ tests refill first
(marked Z); they cannot pass before `kicad-zone` exists, and afterwards they also depend on the
filler being exact. Triangulation of fills (`CacheTriangulation`) is needed even in tier 1 for
item-to-zone clearance (the zone's effective shape is its triangle set).

**R11 - Project settings.** DRC results depend on the exact loading of `rules.*` from
`.kicad_pro` including the differing defaults (7), on netclass pattern assignment and composite
netclasses, and on severities. Boards without a project use constructor defaults (`m_MinClearance`
0, `m_TrackMinWidth` 0.2 mm, `m_CopperEdgeClearance` 0.5 mm, `m_HoleClearance` 0.25 mm,
`m_HoleToHoleMin` 0.25 mm, ...; `board_design_settings.cpp:L152-167`).

**R12 - Expression language traps** (tier 2): `||` binds tighter than `&&`; undefined values
make both `==` and `!=` false; a lone numeric literal without unit is a compile error in
conditions as well; `A.Type` of a teardrop zone is `Track`; conditions are tried as (a, b) and
then (b, a); `checkCondition` memoises by expression text within one `EvalRules` call.

**R13 - Rule-file errors are fatal in the CLI** (no reporter): first error throws; the engine
keeps the implicit rules but the job returns 3 without a report. With a reporter (GUI) parsing
continues - the port only needs the throwing mode plus an optional collecting mode.

**R14 - Singleton provider state** (3.9) and `m_board->IncrementTimeStamp()` only clearing when
some cache is non-empty are C++ artefacts; the per-run cache struct removes both. Document in
`PORT_STATUS.md`.

**R15 - Environment**: report date and KiCad version must be injected (no clock in library
crates); advanced-config values are parameters with the defaults of 5.1; messages are English
only (`_()` not ported).

**R16 - Drawing sheet.** `unresolved_variable` on drawing-sheet texts (marker type
`MARKER_DRAWING_SHEET`) is not reproducible without the drawing-sheet subsystem; rare, document
as a known gap.

**Open questions for the orchestrator**

1. Where do the board run-time caches live - in `kicad-drc` (`DrcBoardCaches`, recommended) or in
   the board model? The zone filler and the expression functions both use them.
2. Is `--refill-zones` rejected with a clear error until `kicad-zone` exists, or silently
   ignored? (Recommended: error.)
3. Tier-1 implicit conditions natively (4.8.5, recommended, unblocks tier 1 from the property
   table) or wait for the compiled path?
4. Acceptance criterion for report equality: multiset equality of the normalised JSON
   (recommended) - byte equality of whole reports is not achievable against a non-deterministic
   reference.
5. Should `lib_footprint_issues` be emitted by the Rust CLI by default (to match a library-less
   reference run) or never?

