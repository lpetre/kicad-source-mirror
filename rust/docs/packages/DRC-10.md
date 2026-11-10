# DRC-10: Disallow provider (keepouts, text on Edge.Cuts) and the keepout / area predicates

Crate: kicad-drc

C++: pcbnew/drc/drc_test_provider_disallow.cpp complete, plus the intersectsArea / intersectsKeepout-style predicates the implicit keepout rules need (drc.md: 'Keepouts need intersectsKeepout (with semantically visible caches) in tier 1'): the engine core (DRC-03) and EvalRules (DRC-04) left hooks for the implicit keepout-rule condition - implement it natively with exactly the semantics and caching of the expression functions in pcbnew/pcbexpr_functions.cpp (intersectsArea / enclosedByArea / intersectsCourtyard as far as keepout rules use them), reusing DrcPredicateCaches of DRC-05. Tests: test_drc_keepout_disallow.cpp and issue7567.

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
