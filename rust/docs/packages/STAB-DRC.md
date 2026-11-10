# STAB-DRC: DRC against the reference with pads - every provider, every corpus board

Crate: kicad-drc (all files; every provider package is finished when you start - read
PORT_STATUS.md, rust/docs/design/review-drc-core.md and review-connectivity.md first: they
list the places where the reference itself is not reproducible and must be compared as a
set / by count / tie-aware)

The providers were ported while no board with footprints or pads could be loaded; each was
proven on the footprint-free part of the corpus (about 100 of 365 boards) and on synthetic
boards. Pads parse since PCBIO-07, and with them the first real comparisons arrived: several
provider tests went red (drc07 connectivity corpus, drc24 courtyard cases, drc25 synthetic,
drc27 solder mask, drc08a). Your job is the fix loop until the whole crate is green and the
corpus scoreboard is clean.

1. Baseline: `cargo test -p kicad-drc --no-fail-fast` and every corpus replay (the shared
   harness tests/drc_harness prints a scoreboard per error code: boards compared, matched /
   missing / extra / order). Record it. If the DRC job (DRC-12: `kicad-rs pcb drc`, the full
   provider registry in engine::PROVIDER_ORDER with the cache generator and connectivity) has
   landed, also run the whole-report comparison: `kicad-cli pcb drc --format json
   --severity-all` and `--format report` vs the port on every corpus board under 5 MB
   (reference results are cached under rust/target/drc-corpus/; COPIES; one at a time).
2. For every difference find the cause down to the statement:
   - a provider defect (fix, regression case = the board + error code);
   - an engine / EvalRules defect in a pad branch (the review lists the pad branches that
     were unverified: R-06) - rerun the EvalRules and cache-generator oracles with pads
     (`pcb-dump board --sections evalrules,engine` goldens: regenerate hashed for boards with
     pads) and fix;
   - a kicad-pcb model defect (pad shapes, footprint courtyards, text boxes ...): prove it
     with pcb-dump vs pcb-dump-rs on the smallest board and fix it there if it is local and
     nobody is editing the file (package STAB-PCB is doing the model scoreboard at the same
     time: re-read before editing; otherwise report it in change_requests);
   - a kicad-connectivity defect (package STAB-CN is doing the same for connectivity):
     report with the board;
   - a documented non-reproducible site: make the comparison set-based / tie-aware there
     (FIX-CN-1 rule for unconnected items) and list it;
   - stale test expectations from the time before pads: update them.
3. Harness: the shared harness must load boards the way the CLI does (project binding, rule
   file, BuildConnectivity, cache generator) - if packages left private wrappers that do the
   wiring per test (ConnectivityWithCaches and the like), consolidate them.
4. Order: where the C++ order is deterministic the port must match it; report every place
   where you had to fall back to set comparison and why.

Report: baseline and final scoreboard per error code (boards compared, matched, missing,
extra, order differences); every fixed defect in one line with its board; every open
difference with the smallest reproducing board and your diagnosis.

## Notes collected from the packages that ran before you (orchestrator)

- The shared harness still loads with the stand-in `SyntaxBinder` and skips every board with
  user rule conditions: switch it to the real expression binding (`engine.init_engine`, as
  tests/evalrules_oracle.rs does since DRC-21) and skip only what is still unported. This
  adds the boards with user rules to every provider's corpus comparison.
- Rule-condition evaluation gaps found by the copper clearance provider (DRC-08b):
  `A.Pad_Shape == 'Oval'` does not match an oval pad (qa/data/pcbnew/issue16566: 4 clearance
  entries missing); `A.Reference == 'J1' && B.Reference == 'J1'` does not match a pad against
  the NPTH pad of J1 in the hole test (issue23469: 4 extra hole_clearance entries, the
  parent-footprint fallback for the hole proxy or for B?). Remove the boards from
  KNOWN_RULE_GAPS in tests/drc08a_copper_clearance.rs when fixed.
- Zone fill found EvalRules returning the zone clearance instead of a custom rule's value
  on issue23339_zone_layer_rules and issue17967 while conditions (hasNetclass, NetClass ==,
  layer outer) were unported: recheck with the real binding and the DRC-22 functions.
- Local copies to replace by the model functions that exist now: providers/text_mirroring.rs
  `board_is_layer_visible` -> `Board::is_layer_visible`; providers/via_stack.rs
  `is_span_valid` -> `ViaStackData::is_span_valid`; providers/misc.rs collector walk ->
  `convert_shape_list_to_polygon::collect_board_shapes`; providers/solder_mask.rs can write
  the bridge zone's fill through `Zone::fill_mut`; `Pad::is_free_pad(ctx)`.
- Rule identity: `DrcRuleInfo::rule_index` exists (FIX-DRC-1); nothing may use
  `DrcRuleInfo` equality as identity.
- The DRC-23 outline entries list the board as an item in the reference (description 'PCB',
  a random UUID per run): the port has no `Board::uuid` yet - decide the comparison rule
  (mask that UUID) and report what the model needs.
- kicad-zone re-expresses EvalZoneConnection over `RuleResolver::eval_rules` because the
  trait has no such method: add `eval_zone_connection` to the trait (kicad-pcb ctx.rs, one
  method with a default that panics or forwards - check board-model.md 7) implemented by
  `DrcEngine::eval_zone_connection`, and tell kicad-zone's owner in change_requests.

## More notes (after DRC-10, DRC-22, FONT-RC-1 and the reviews)

- Expression caches: the C++ keeps memo caches on BOARD that change results (the
  layer-insensitive courtyard memo, the per-area memo that ignores the constraint, errors
  suppressed on a memo hit, aliasing between an item and its hole proxy). The port has them
  in `pcbexpr::functions::PcbExprBoardCaches`, active only inside
  `PcbExprCacheScope::enter(&board)`. No production run path enters the scope yet: the DRC
  run driver (DRC-12's job code and the harness) must hold it from InitEngine through
  RunTests on the evaluating thread. Check that DRC-12 did it; if not, do it.
- Harness load path: real binder (`PcbExprUcode`), triangulated zone fills
  (`kicad_connectivity::data::cache_board_triangulation` or the full BuildConnectivity), as
  KI_TEST::LoadBoard / the CLI do. DRC-22 saw 4 zone categories of issue11814 mismatch only
  because the fills were not triangulated.
- Red tests at hand-over and what is known about them:
  drc07 `connectivity_matches_reference_corpus`: issue25580 unconnected_items names a
  different teardrop UUID at the same position (teardrop zones are saved without a uuid and
  get a random KIID at every load - also in the reference: compare such items by position /
  description, not UUID); drc24 `courtyard_invalid_cases` and
  `courtyard_clearance_matches_reference_corpus`: malformed_courtyard violations missing
  (3-4 entries; expected ("U1", 34) not found) - footprint courtyards exist since M20:
  find out whether the provider or `Footprint::build_courtyard_caches` / the MALFORMED flags
  are wrong (M20 notes: the flags read as cleared after the cache was dropped; C++ readers
  build the cache just before reading); drc25 `synthetic_boards_match_reference`: 1 extra
  starved_thermal since pads parse from the file instead of the sidecar table; drc27
  `drc_solder_mask_bridging_test` 3 vs 4 and its corpus replay.
- ORCHESTRATOR DECISION (outline fonts, package FONT-RC-1): the port uses the glyphs stored
  in the board file as stand-in for outline-font text. Stored glyphs are fractured (holes
  turned into slits); the reference measures unfractured font outlines. For the TrueType
  branch of checkTextThickness only, unfracture a COPY of the stand-in glyphs before the
  deflate / area test (FONT-RC-1 measured: this matches the reference at every tested
  threshold on the crafted board with installed fonts; plot and save keep the stored
  polygons untouched). Implement it, extend the crafted case
  (kicad-pcb tests/data/font_rc1/frc.kicad_pcb: thresholds 0.1 / 0.15 / 0.2 / 0.25 mm must
  all be identical to the reference), and update tests/drc24_courtyard_text.rs:
  `text_providers_match_reference_corpus` records issue22475 text_thickness on
  ff749b73-2383-44e8-88dd-1d9c1b0c1a89 as an expected difference (the font is missing on the
  oracle machine: the reference measures fontconfig's substitute, the port the stored
  glyphs); `text_thickness_truetype_render_cache_issue22475` must test
  `eda_text::render_cache(text, ctx, &shown, 0).is_some()` instead of the live cache.
- Raw zone_connect values (STAB-PCB): library parity
  (drc_test_provider_library_parity.cpp:294 / 990) and the PrintZoneConnection report texts
  must use the `*_raw` accessors; the enum accessors return FULL for out-of-range values.
- fromTo() returns 0 until the from-to cache (CN-05) is ported: list the corpus rules that
  use it.
