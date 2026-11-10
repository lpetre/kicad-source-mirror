# REV-ZF: Review of the zone filler port (reviewer role)

Crate: kicad-zone (review of ZF-01 driver, ZF-03 copper fill / knockouts / thermals /
hatching, ZF-06 FillAllZones; fixes in their files: src/filler.rs, copper_fill.rs,
knockouts.rs, thermal.rs, connect.rs, refill.rs, tool.rs - the teardrop package TD-01 is
working in src/teardrop/ at the same time: do not edit there)

The zone filler was ported by three agents. Its fills are vertex-identical to the
reference `kicad-cli pcb drc --refill-zones --save-board` on 336 of 336 compared zone
layers (142 boards) and 125 of 126 refilled boards re-save byte-identically. That proves
the paths the corpus exercises. Your job is everything else: read the port side by side
with pcbnew/zone_filler.cpp / .h (and zone.cpp, zone_fill... helpers it calls) with a
reviewer's eye and find what the corpus does not show.

Read first: rust/docs/analysis/connectivity-zones.md (zone-fill sections: the pipeline in
execution order, the order-sensitivity table, risks R-8 .. R-13),
rust/docs/design/board-model.md 8.4 and 13, kicad-zone/PORT_STATUS.md,
rust/docs/design/review-connectivity.md and review-drc-core.md (what the earlier reviews
established about heap order and thread-dependent results in the reference).

What to review:
1. Coverage map: for every branch of the C++ fill pipeline (fillCopperZone step by step,
   buildCopperItemClearances per item kind, knockoutThermalReliefs, buildThermalSpokes incl.
   custom pads and the spoke angle / orthogonal logic, hatch fills with their smoothing and
   thermal rings, island removal modes, min-width prune, the iterative refill and
   refillZoneFromCache, zone priorities and same-net / different-net zone interaction,
   keepout handling, rule areas, board outline clipping, teardrop zones, zones in
   footprints, multi-layer zones, non-copper zones) say whether a compared corpus board
   exercises it (instrument a run if needed: a counter per branch behind a test-only cfg,
   not committed if noisy) - and for every branch no board reaches, craft a board that
   does (hand-written .kicad_pcb, refilled by the reference, small, committed under
   tests/data with its golden) and compare. Custom-pad proxy spokes and hatch thermal rings
   around vias are known uncovered (ZF-03's report).
2. Statement-level reading of the geometry calls: every Clipper operation's operand order,
   corner strategy, max error and simplify flags; every clearance value with its epsilons
   (m_worstClearance, extra clearance, the DRC epsilon), rounding and integer division;
   where the C++ reads a constraint through the DRC engine (EvalRules /
   EvalZoneConnection - the port re-expresses EvalZoneConnection in knockouts.rs: compare
   it line by line with DRC_ENGINE::EvalZoneConnection incl. the pad / via / footprint /
   zone override order).
3. Order and determinism: the sequential replacement of the thread pool (dependency DAG in
   Kahn order), every ZONE*-keyed container, the fill of zones that depend on other zones'
   fills (ZF-03 found the reference's DEBUG stage dumps thread-dependent for multi-zone
   boards: is the real fill ever thread-dependent in the reference? if yes, which order
   does the port take and is it one the reference can produce?).
4. State: what Fill() changes on the board besides the fills (zone-layer overrides of pads
   and vias, need-refill flags, fill hashes / "fill is up to date" state that the writer
   and `--check-zones` read, island flags, time stamps and cache invalidation per design
   8.4), and that the session / DRC job / plot paths see the same state as in C++
   (connectivity rebuilt after the fill, DRC caches).
5. Numeric rules of PORTING.md; `unwrap_or` / "treated as empty" where C++ throws.

Findings: write rust/docs/design/review-zone-fill.md (coverage map, findings table: id,
severity, file:line, C++ reference, what differs for which input, status). Fix what you
can prove with a regression board against the reference; larger work becomes
`rust/docs/packages/FIX-ZF-<n>.md`. Verification: the zone corpus (tools/zone-corpus/run.sh
+ tests/zone_fill_corpus.rs) and kicad-session tests/zf06_refill_save.rs before and after:
no count gets worse; new crafted boards identical or listed as findings.
