# DRC-21: PCB expression binding for custom rules (PCBEXPR_* on kicad-libeval)

Crate: kicad-drc

C++: pcbnew/pcbexpr_evaluator.cpp + pcbexpr_evaluator.h complete: PCBEXPR_CONTEXT (items A / B, layer, constraint, the error reporter), PCBEXPR_VAR_REF with the property lookup through the property tables (kicad_pcb::properties, package PROP-1, completed just before you in this chain: read its section in rust/crates/kicad-pcb/PORT_STATUS.md) incl. the type matrix (GetValue for ints / doubles / strings / enums / layer / netclass / netname special values PCBEXPR_NETCLASS_VALUE, PCBEXPR_COMPONENT_CLASS_VALUE, PCBEXPR_NET_VALUE, PCBEXPR_LAYER_VALUE with their EqualTo / NotEqualTo semantics incl. wildcard layer and composite netclass matching), PCBEXPR_UCODE::CreateVarRef / CreateFuncCall (item names A, B, L, the 'null' handling, property-not-found and type errors with their exact texts), PCBEXPR_UNIT_RESOLVER and the unitless resolver, PCBEXPR_COMPILER / PCBEXPR_EVALUATOR, the built-in function table registration (the function BODIES of pcbnew/pcbexpr_functions.cpp are the next package, DRC-22: register every name with a stub that reports 'function not yet ported' as a rule error). Bind it to kicad-libeval through its UcodeBinder trait (rust/crates/kicad-libeval/PORT_STATUS.md and tests/binding_example.rs show how; kicad-libeval carries 20 board-dependent C++ test cases from qa/tests/pcbnew/test_libeval_compiler.cpp as #[ignore]d data: port them here against real items), and plug it into the rule-condition interface that DRC-20 defined and that the engine (DRC-03 / 04) evaluates user rules through, so that .kicad_dru conditions and constraint value expressions now compile and evaluate. Verification additionally: for every .kicad_dru in the corpus, every condition must compile exactly as in C++ (same accept / reject, same error text and offset: extend the `rules` command of the reference dump tool or its engine section to print the compile result per rule), and the EvalRules oracle of DRC-04 must now pass on boards with custom rules that do not call functions.

DRC package rules: specification rust/docs/analysis/drc.md (precedence note at the top; read the sections that concern your package and your row in the work-package table of section 10, which lists the C++ files; section 4 lists every place where iteration order or thread scheduling can change the output - the port is sequential and must report violations in the same order as a single-threaded C++ run where that order is defined, and say where it is not) and rust/docs/design/board-model.md sections 7 and 13 "DRC". You work in rust/crates/kicad-drc: read its PORT_STATUS.md first (rule file parser DRC-20, report writers DRC-11, r-tree / zone index / provider base DRC-02, engine core DRC-03, EvalRules DRC-04, cache generator and DrcBoardCaches DRC-05 - their notes describe the APIs you build on). You own the NEW module files you add for your package (one module per C++ file) plus your section of PORT_STATUS.md; minimal additive edits to the provider registry / lib.rs mod list are fine. Each provider is ported statement by statement from its drc_test_provider_*.cpp: the same item iteration order, the same constraint queries, the same violation positions and the same message texts (byte-exact: they appear in the reports) through the provider base helpers.

Verification is against the reference kicad-cli DRC reports (rust/docs/ORACLE.md; always on COPIES): `kicad-cli pcb drc --format json --severity-all [--all-track-errors]` and `--format report`. The comparison unit is the SET of violations of the error codes your package produces, each identified by (error code, description text, position, the UUIDs and descriptions of its items), plus their ORDER in the report where the C++ order is deterministic. Use the differential harness of the crate if it exists (look for a `drc_corpus` test / tools/drc-corpus; if no package has built it yet, build the minimal shared version: run the reference on a board copy, cache the JSON under rust/target/drc-corpus/, run the Rust engine with only your providers enabled, filter both sides to your error codes, diff, print a scoreboard), on every corpus board the Rust loader can load completely at the time (kicad_pcb::io::parse_board + the project binding of src/project; footprints and pads are landing right now - the set of loadable boards grows; leave the replay test ready so it tightens automatically). Also port the C++ tests under qa/tests/pcbnew/drc that exercise your providers, with their boards and expected counts (drc.md section 9 has the tables). Report: boards compared, violations matched / missing / extra per error code, and the cause of every difference.

## Orchestrator notes (added after the first provider packages reported)

- Connectivity is finished (kicad-connectivity CN-04). The reference CLI builds connectivity
  at load (BOARD::BuildConnectivity) and DRC runs with it: conditionally flashed pads / vias
  (FlashLayer -> IsConnectedOnLayer) depend on it. The shared harness tests/drc_harness
  currently runs with a `BoardCtx` WITHOUT connectivity (DRC-06 / DRC-27 reported this). If
  that is still the case when you start, change the harness load path (additively, it is
  shared) to `ConnectivityData::build_connectivity(&mut board)` after the project binding and
  to run providers with `ctx.with_connectivity(&data)`, as the CLI load sequence does
  (kicad-session PCBIO-15 is porting that sequence: reuse it if it has landed), and confirm
  that the corpus results of the providers already ported do not get worse.
- The cache generator: `DrcEngine::run_tests` could not yet run the cache generator and hand
  `&DrcBoardCaches` to providers (DRC-06 / DRC-27 change requests; solder_mask takes the
  caches in its constructor as a workaround). If your provider needs the caches and the
  wiring is still missing, you are sanctioned to add it in the engine files with a minimal,
  atomic edit (a reviewer agent, REV-DRC, is reading and possibly fixing the engine core
  files at the same time: re-read a file right before you edit it).
- Footprints and pads: the parser packages PCBIO-06 / PCBIO-07 and footprint geometry M20
  are landing now. As soon as boards with footprints load, the pad paths of every provider
  become comparable: run your corpus replay again at the end and report pad-related
  mismatches of YOUR error codes with their cause.

- From the Fable reviews of the DRC core and of connectivity (docs/design/review-drc-core.md,
  docs/design/review-connectivity.md - read their "notes" sections if your provider touches
  the topic):
  - Run sequence: `prepare_run`, then `DrcCacheGenerator::run`, then construct providers,
    then `run_providers` in `engine::PROVIDER_ORDER` (nothing enforces the order; it decides
    report order and which provider loses at shared error limits). `DrcEngine::run_tests` is
    not usable with cache-borrowing providers.
  - `DrcConstraint` carries rule copies (`DrcRuleInfo`: name, severity, implicit). Never use
    `DrcRuleInfo` equality as rule identity (same-named rules are equal); a `rule_index`
    field is coming (package FIX-DRC-1).
  - The reference is NOT reproducible in these places, so compare them as sets / by count /
    tie-aware and say so in your report instead of chasing a mismatch: providers the C++ runs
    on the thread pool when a board exceeds an error limit (199; 499 for clearance and
    unconnected items); consumers of `DRC_RTREE::GetObjectsAt` (hash order in C++); maps
    keyed by pointer (`m_CopperZoneRTreeCache`, the isolated-islands map, rule-keyed maps);
    everything that takes "the first" of `connected_items()` / `connected_tracks` /
    `connected_pads` / `net_items` / `zone_island_connections` (heap order in C++, changes
    between the load build and the DRC rebuild and between runs: e.g. the second item of
    `track_not_centered_on_via` / `track_on_post_machined_layer`, the far end of an
    unconnected-items violation under ties - package FIX-CN-1 adds a tie flag to ratsnest
    edges for exactly this comparison).
  - `for_each_geometry_item`: track callbacks cannot stop the walk (as in C++); every other
    item kind can.
