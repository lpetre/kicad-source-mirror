# STAB-DRC-2: `kicad-rs pcb drc` against the reference, board by board - second pass

Crate: kicad-drc (finished provider and engine files, run_flow.rs, the harness, tests),
kicad-pcb (a board UUID), kicad-session tests

State (read kicad-drc PORT_STATUS.md sections DRC-12 and STAB-DRC, kicad-cli's WIRE-1
section, rust/docs/design/review-drc-core.md, review-connectivity.md): the provider corpus
replays (QA load path, kicad-drc tests) match the reference on 362 boards with 0 missing
and 0 extra entries for every ported error code. The end-to-end comparison of the real
binary (CLI load path; kicad-cli/tests/oracle/e2e_cli.py with its golden, and
kicad-drc/tests/oracle/drc12_cli_compare.py ref / cmp / golden with the replay
kicad-session/tests/drc12_corpus.rs) was last run BEFORE the first stabilisation pass
finished and showed, besides the library parity entries (package DRC-LP, running in
parallel: not yours) these differences:
- the board's own item: `invalid_outline` and similar entries list the board ('PCB' at
  (0,0), a random UUID per run) in the reference; the port has no board UUID: 61 report
  boards (masked) and more in JSON (unmasked);
- entry differences: connection_width_rules (rule name high_current_area vs
  high_current_netclass), issue5854, issue24525, issue24935, demos/microwave
  (shorting_items `<no net>` vs clearance), severities;
- count differences: issue24853 (4 vs 3), issue25439 (3 vs 2), two_segment_teardrop (9 vs
  7), issue24078 (3 vs 1).

Work:
1. Board UUID: BOARD is an EDA_ITEM with m_Uuid (random at construction). Give
   `kicad_pcb::Board` one - drawn so that it does NOT consume from the board's main
   KiidGenerator sequence (existing goldens depend on that sequence; the value is random in
   the reference anyway) and is reserved like file UUIDs - make it resolvable (the report
   item resolver returns the board: description 'PCB', position (0,0); JSON and text
   exactly as the reference prints them: check drc_item / rc_json code), set the items of
   the misc provider's outline entries and wherever else the C++ passes the board
   (grep `SetItems( m_board` / `SetItems( board` in pcbnew/drc), remove the `PCB` masks
   from the comparison tools and replays and compare the line with its UUID normalised.
2. `run_flow::run_tests`: rebuild the from-to cache after the cache generator
   (`connectivity.rebuild_from_to_cache(..)`, drc_engine.cpp:969-970; CN-05 and DRC-30
   asked for it; today only the diff-pair arm rebuilds it) and give the connectivity
   provider's context the connectivity so fromTo() conditions evaluated there work.
3. Rebuild `kicad-rs` (cargo build -p kicad-cli), rerun BOTH whole-report comparisons on
   the full corpus, and for every board that is not identical (after the documented
   normalisations) find the cause down to the statement and fix it: CLI load path vs QA
   load path differences (layer name map, project binding, variants, exclusions,
   severities from the project), report ordering, rule-condition evaluation, provider
   defects. The lists above are the starting point, not the limit. Differences owned by
   running packages (library parity: DRC-LP; matched length: DRC-31; creepage: DRC-33;
   drawing-sheet text variables: DS-1) are counted and named, not fixed.
4. Leftovers named by the first pass and by GLUE-2: providers/misc.rs collector walk ->
   `collect_board_shapes`; providers/solder_mask.rs writes the bridge zone's fill through
   `Zone::fill_mut`; providers/via_stack.rs `is_span_valid` -> `ViaStackData::is_span_valid`;
   providers/text_mirroring.rs -> `Board::is_layer_visible`; drc06 / 26 / 27 / 28 / 29
   tests move to `compare_corpus_flow`; marker and exclusion UUIDs from the board's
   generator (review R2-11); `RuleResolver::eval_zone_connection` in kicad-pcb ctx.rs
   implemented by `DrcEngine::eval_zone_connection` (kicad-zone's knockouts.rs then calls
   it: prove equal results with the zone corpus, tools/zone-corpus/run.sh +
   kicad-zone tests/zone_fill_corpus.rs, before and after); `--severity-exclusions` on the
   corpus projects that store exclusions and `--refill-zones --save-board` through
   `kicad-rs pcb drc` against the reference on twenty boards with zones (DRC-12 did not
   verify them).
5. Update the goldens of both comparison tools and the KNOWN_DIFFERENCES lists; rustfmt and
   clippy of the files you touch.

Other agents: DRC-31 and DRC-33 in their own new provider files, DRC-LP in
library_parity.rs and run_flow.rs's registry (small edits), DS-1 at the misc provider's
drawing-sheet hook: re-read a file right before editing; do not edit their files.

Report: the scoreboard of both comparisons before and after (identical / identical after
which normalisation / differing), every fixed defect in one line with its board, every
remaining difference with the smallest board and the owner.
