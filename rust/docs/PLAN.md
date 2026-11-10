# Port plan and status

Orchestrator's working plan. Updated as phases complete.

## Goal

Rust port of the KiCad core features needed to (1) validate routed boards (DRC), (2) produce
fabrication outputs (Gerber, Excellon drill), (3) support routing work (connectivity/ratsnest,
zone refill; interactive router is a stretch goal) - buildable for WebAssembly.

## Scope tiers

- **Tier 1**: `kicad-sexpr`, `kicad-geom`, `kicad-core`, `kicad-pcb` (model + read + write +
  item shapes + project settings), `kicad-connectivity` (+ ratsnest), `kicad-drc` (engine,
  implicit rules, core providers: copper/hole clearance, shorts, unconnected/dangling, edge
  clearance, hole-to-hole, hole size, track width, via diameter, annular width, disallow),
  `kicad-plot` (Gerber + SVG plotters), `kicad-fab` (board layer plotting, drill files, gbrjob),
  `kicad-font` (stroke font), `kicad-cli` (`kicad-rs pcb drc|export gerbers|export drill|...`),
  `kicad-wasm` (wasm-bindgen API + browser/node demo).
- **Tier 2**: custom rules (`.kicad_dru`, `kicad-libeval` + pcb expression functions), zone
  refill (`kicad-zone`), remaining DRC providers (solder mask, silk, courtyard, zone
  connections, text dims, misc, sliver, connection width, diff pair, matched length, ...),
  outline fonts.
- **Tier 3 (stretch)**: push-and-shove router core (`pcbnew/router`), Specctra DSN/SES.

## Crate dependency graph

```
kicad-sexpr   kicad-geom   kicad-core   kicad-libeval
                  \            |
                   kicad-font -+
                  /     |
        kicad-plot    kicad-pcb (sexpr, geom, core, font)
              \         |      \
               \   kicad-connectivity
                \       |        \
               kicad-fab      kicad-drc (libeval)
                                  |
                              kicad-zone
        kicad-cli / kicad-wasm on top of everything
```

## Verification strategy

1. Ported C++ unit tests from `qa/tests` (geometry, sexpr, libeval, DRC regressions).
2. Golden files in `qa/data/cli` (Gerber, drill).
3. Differential testing against a reference `kicad-cli` built from this very source tree
   (`/home/user/kicad-oracle/bin/kicad-cli`, see `docs/ORACLE.md`): board re-save byte equality,
   DRC report equality, Gerber/drill equality over `qa/data/pcbnew` and `demos`.
4. wasm32 build + node/browser smoke tests.

## Status log

- [x] Workspace skeleton, porting rules (PORTING.md, incl. section 9 binding decisions).
- [x] Oracle: reference kicad-cli 10.99.0 built from this tree, see docs/ORACLE.md.
- [x] Research: docs/THIRD_PARTY_EQUIVALENTS.md written (Opus) and its decisions table
      Fable-reviewed and accepted. Orchestrator decisions on its open points: netclass regex
      patterns use `regress` 0.12 (closest to wxRegEx/PCRE2 in the experiment: 2/55 realistic
      patterns differ vs 14/55 for `regex`), wildcard patterns are hand-ported; workspace
      rust-version raised to 1.88 so current ruzstd/sha2 resolve; JSON via serde_json
      (preserve_order + float_roundtrip) with a hand-ported nlohmann-compatible formatter in
      kicad-core; outline fonts (tier 2/3) via harfrust + skrifa; wasm-bindgen pinned =0.2.130
      (CLI installed).
- [x] Analysis docs: pcb-io, drc, plot-fab, settings, connectivity-zones, text-font.
- [x] Board model design: Opus synthesis chose proposal B (typed arenas + Copy handles).
      Dedicated Fable review: approve-with-changes; it found and fixed two blockers (items
      could not identify themselves to connectivity; the parser's detached-footprint plan)
      plus five major issues, deferred machinery not needed for tier 1/2, and split M0 into
      M0a (kernel, reviewed) + M0b-e (four parallel type packages). Design docs are now
      revision 2: docs/design/board-model.md + board-model-types.md.
- [x] Model M0a kernel (Opus) + Fable gate review: approve-with-changes. The review fixed
      9 major issues in the kernel (a trait signature taken from KiCad 9, lost EDA_TEXT /
      EDA_SHAPE overrides for fields/dimensions/table cells, overrides unable to call base
      bodies, shared files that four agents would have edited) and added
      crates/kicad-pcb/tools/check_item_shims.py. Design docs are at revision 2.1.
- [x] Model wave 0b: M0b (pad/footprint/zone), M0c (shape/text/table/dimension), M0d
      (board/settings/nets), M0e (small items) on Opus: all 26 item types pass the shim
      checker; 793 port_pending! stubs carry the final signatures (~27k lines in kicad-pcb).
- [ ] Fable integration of wave 0b (trait change EdaShapeItem::width(ctx), matcher types
      Send+Sync+Clone, checker for generator subclasses, decision review, defaults
      spot-check) -> then M7 EdaShape core, M10 Text (running).
- [ ] Model wave 1 (Opus, running): M1 containers/index/create-attach, M2 item base, M3 nets
      and netclasses, M13 padstack. Queued: M5 design settings, M6 stackup/plot params, M24
      images/embedded files/component classes, then waves 2-5 per board-model.md section 16.
      Policy: at most ~7 agents in kicad-pcb at once (shared crate, transient build breaks).
      Reusable workflow script: model-wave (args: optional integration text + package list).
- [x] pcb-dump: item-level C++ reference dump tool linking the real pcbnew objects
      (rust/tools/pcb-dump, binary /home/user/kicad-oracle/bin/pcb-dump, format in
      docs/PCB_DUMP_FORMAT.md). Sections: item, bbox, track, pad, shape, text, dim, zone,
      footprint, misc, board, order + `defaults`. Runs the whole corpus (375 boards, one
      intentional parse failure), deterministic. Use small boards for goldens (large demos
      produce GBs of output).
- [ ] DUMP-RS (running, Opus): Rust side of the dump inside kicad-pcb (feature `dump`),
      mirroring the C++ tool section by section, with catch_unwind per field so that
      port_pending! stubs print as "pending"; plus tools/pcb-dump/compare.py giving
      match / mismatch / pending counts per section = the scoreboard for the M packages.
- Follow-ups owed to the design review: kicad-geom needs `Shape::indexable_subshape(i)`
  (assign with G5); kicad-drc must split rule resolver from run state.
- Foundation crates:
  - [x] kicad-sexpr (S1) - lexer, formatter, Prettify, tree; oracle goldens over 479 files.
  - [x] kicad-geom G1 math - 1M-case differential oracle bit-exact.
  - [x] kicad-geom G2 basic shapes (Opus; 606k oracle cases bit-exact; Fable-reviewed:
        deviations D11-D16 accepted except D12 - out-of-range casts now follow x86 semantics,
        PORTING.md section 9, applied by G3).
  - [x] kicad-geom G7 stdsort / packed + dynamic r-tree / delaunator / nanoflann (Opus; 10k
        oracle cases bit-exact natively and on wasm; COW_RTREE skipped, router only).
  - [x] kicad-geom G3 SHAPE_LINE_CHAIN (Opus; 222k oracle cases), G4a vendored Clipper2 +
        SHAPE_POLY_SET (Opus; 328k oracle cases, Clipper2 re-validated on 19.7k harness cases),
        G6a bezier / hull / TRANSFORM_TRS / ellipse / hashes (Opus; 540k oracle cases). All
        Fable-reviewed and accepted; orchestrator re-ran the crate: 361 tests pass, clippy
        clean, wasm32 builds. x86 cast rule confirmed by oracle and applied.
  - [ ] kicad-geom G4b triangulation -> G5 collisions/nearest points/shape index (+
        Shape::indexable_subshape), G6b shape conversion utilities (running, Opus).
  - [x] kicad-core C1 ids/layers; [x] C2 units/strings/values (Opus; 152k oracle cases
        bit-exact; Fable-reviewed: deviations C2-1..9 accepted).
  - [x] kicad-libeval (Opus finish; Lemon tables regenerated and identical, 39.5k oracle
        cases; Fable-reviewed: binder API accepted, to be proven by the kicad-drc binding).
  - [x] kicad-font F1 stroke font, layout, markup (Opus; glyph table byte-identical, 8500
        oracle cases bit-exact; Fable-reviewed: API accepted). Follow-ups: apply the x86
        out-of-range cast rule (agent used saturating casts before the rule existed);
        @{...} text evaluation (text_eval) and outline fonts are not ported (tier 2/3).
- [ ] Board model + parser + writer.
- [x] kicad-plot P1 base+metadata, P2 Gerber plotter (2400 scripted files byte-identical),
      P3 text plotting + SVG plotter (1500 scripted files byte-identical); kicad-core C3
      nlohmann-compatible JSON (413 corpus files re-dumped identically), settings PARAM
      mechanics, pattern matcher (all 1899 corpus netclass patterns match), text variables.
      All Opus, Fable-reviewed and accepted; orchestrator re-ran: 253 tests pass, clippy clean.
      Follow-ups once G6b lands: drop the round-rect polygon closures in kicad-plot and use
      kicad-geom directly; SVG rounded Rect; remove the Gerber PlotText debug_assert on
      negative width (C++ only asserts and continues).
- [ ] Connectivity, fab, drc.
- [ ] Zone filler, custom rules, remaining providers.
- [ ] CLI, wasm, demo.
- [ ] Differential test campaign + adversarial review.
- [ ] Performance phase (see below).

## Overnight runbook (the user is asleep: keep going, do not wait for input)

Standing instructions from the user: do not stop; spare no tokens; Opus for the bulk, Fable
reviews every Opus decision before others build on it; commit and push after every workflow.
A heartbeat reminder is scheduled with send_later; re-arm it each time it fires.

Reusable workflow script (invoke with `scriptPath` + `args`, no need to resend the script):
- `/root/.claude/projects/-home-user-kicad-source-mirror/76042f1d-984e-5683-9c58-69f73f0a28c8/workflows/scripts/run-packages-wf_bf883e1d-a68.js`
  - args `{chains: [["ID", "ID2"], ["ID3"]], interrupted: ["ID"], extra: {"ID": "note"},
  models: {"ID": "fable"}}`. Each agent reads its assignment from `rust/docs/packages/<ID>.md`
  (committed). Chains run in parallel (2 per workflow), packages of a chain in order, each
  receiving its predecessor's notes; ids listed in `interrupted` get a "take stock and finish,
  do not start over" instruction. To launch a NEW package: write `docs/packages/<ID>.md`
  (same shape as the existing ones: crate, owned files, C++ to port, verification), then run it.
- (`model-wave-wf_9da70494-9c4.js` with an optional Fable integration step also still exists.)

Recovery after a container restart (it happened once, 2026-11-10 00:55 UTC, probably out of
memory from parallel C++ oracle builds): files survive, processes do not. Commit + push; list
finished packages from the workflow journals (`.../subagents/workflows/*/journal.jsonl`,
`type == "result"`); relaunch every unfinished package with `interrupted`; restart the disk
watchdog (`/home/user/kicad-oracle/bin/disk-watchdog.sh` as a background task). PORTING.md
section 7 now has memory and disk rules (machine-wide flock for C++ builds).

Progress log:
- done (Opus; results read and accepted on Fable unless noted): model M0 (reviewed kernel +
  types), M1-M8, M10, M13, M14, M16, M17, M22, M23, M24, SET-3, SET-4, Fable integration #1
  (design rev 2.2), DUMP-RS, PCBIO-01 parser core, PCBIO-02 writer core, DRC-20 rule file
  parser, DRC-11 report writers, FAB-01 drill model, FAB-02 Excellon writer. kicad-geom,
  kicad-sexpr, kicad-core, kicad-libeval, kicad-font, kicad-plot are complete.
- relaunched after the restart (all were mid-flight; run-packages, Opus): M9 -> M11 and M18
  -> PCBIO-08 (w7nqnu6z5); PCBIO-03 and PCBIO-05 (wb51psfi2); M15 and M12 -> PCBIO-10
  (wxze6tto6); CN-01 -> CN-02 -> CN-03 -> CN-04 and DRC-03 -> DRC-04 (w6koa0hnk); DRC-02 ->
  DRC-05 and PCBIO-04 -> SET-5 (w08hk2mca); W1 and PCBIO-17 -> PCBIO-09 -> PCBIO-11
  (wd75f0o07). Done since: PCBIO-03 tracks/vias/shapes and PCBIO-05 text (wb51psfi2): first
  byte-exact results - track/via/shape blocks identical to the reference re-save on 170
  corpus boards (256 board-kind pairs), shape formatter identical on 276 boards, text blocks
  identical wherever reachable; 0 real differences. Launched: GLUE-1 (wire finished ports
  into stale stubs, dump no_api calls, stale staging-stub tests) and PCBIO-12 other items
  (w3ztkctur). Heartbeat 01:18 UTC: also done M9, M18, M12, M15 (or nearly), CN-01, DRC-02,
  PCBIO-04, PCBIO-09, PCBIO-17 (corpus scoreboard: 375 boards, 34 byte-identical to the
  reference re-save, 159 identical modulo pending constructs, 287 fixed points) and W1
  (session / cli / wasm: `kicad-rs pcb upgrade` byte-identical to kicad-cli on issue10697;
  every library crate builds for wasm32; orchestrator re-ran scripts/build-wasm.sh (5.0 MB
  module) and web/test/smoke.mjs: 49 checks pass, wasm == native). Launched: M19 -> PCBIO-06
  -> PCBIO-07 and M21 -> M20 (w3kdexuvx). Package files exist for PCBIO-13 (launch after
  PCBIO-09/11 chain ends). 01:40 UTC: done DRC-02 r-tree / provider base, DRC-05 cache
  generator, PCBIO-04 setup section, SET-5 project binding + load sequences (w08hk2mca).
  Launched: FAB-03 -> FAB-04 -> FAB-05 -> FAB-06 -> FAB-07 -> FAB-09 -> FAB-08 (board
  plotting chain in kicad-fab) and PCBIO-13 generators (wb9xlvw89). Still running: M11 and
  PCBIO-08 (w7nqnu6z5), PCBIO-10 (wxze6tto6), CN-02.. and DRC-03 -> DRC-04 (w6koa0hnk),
  PCBIO-11 (wd75f0o07), GLUE-1 and PCBIO-12 (w3ztkctur), M19 -> PCBIO-06 -> PCBIO-07 and
  M21 -> M20 (w3kdexuvx). 02:00 UTC: done M15 pad shapes (135 pads, 2820 values, 0
  mismatches against pcb-dump), M12 dimensions (2012 reference lines match), PCBIO-10,
  PCBIO-09, PCBIO-11, CN-01, CN-02, DRC-03. Launched: PROP-1 -> DRC-21 -> DRC-22 (property
  tables, expression binding, functions) and DRC-06 -> DRC-07 -> DRC-08a -> DRC-08b -> DRC-09
  -> DRC-10 -> DRC-12 (providers and the drc job) (w878ykkp2). Package files still to write:
  FAB-10..12 (tier 2). 02:20 UTC: done M9 (2384 values vs pcb-dump, 0 mismatches), M11
  (9403 oracle values equal), M18, PCBIO-08, CN-02, CN-03 (ratsnest: 0 mismatches), DRC-04
  EvalRules (1128 oracle categories match, 0 mismatch; pads pending the pad parser).
  kicad-pcb model stubs: 793 -> 73 (barcode tier 2, footprint geometry M20 running).
  Launched: tier-2 DRC providers DRC-24 -> DRC-26 -> DRC-23 -> DRC-29 and DRC-27 -> DRC-25
  -> DRC-28 (wxpfc9smx).
  02:05 UTC (clock corrected: earlier stamps in this log ran ~25 min fast): done GLUE-1
  (stale stubs wired; `sorted_set` is now an exact libstdc++ RB-tree `std::set` port;
  PCBIO-03 corpus oracle identical on 510 board-kind pairs; defaults scoreboard 3533 match,
  149 pending, 0 mismatch), PCBIO-12 other items (69 blocks byte-identical; barcodes wait for
  M25a), M19 footprint A (cascades, fields, variants, net ties; 110 reference values equal),
  M21 board outline (333-board golden; all 103 loadable boards identical; 2 fixture lines
  differ by the documented pointer-order rule), FAB-03 plot driver (247/247 Gerber files of
  crafted boards byte-identical; corpus: 2840/2840 headers, 874 whole files, 0 differ, rest
  pending item plotting), PCBIO-13 generators / constraints (155 blocks on 22 boards
  byte-identical; template via pending a writer entry point). Launched: Fable integration #2
  (below), M25a barcode model (no symbol encoder yet) and CLEAN-1 foundation consolidation.
  Watch list from the Fable read of these results: (a) M21 keeps the board immutable where
  C++ `RebuildBezierToSegmentsPointsList(aErrorMax)` re-approximates Edge.Cuts beziers in
  place during the outline build - first suspect if DRC / plot output differs on boards with
  bezier outlines; (b) M19 cascades re-stamp patched footprint caches (design 8.5) - check
  with the M20 oracle; (c) `Footprint::component_class` with dynamic rules is pending the
  DRC rule matcher.
  02:20 UTC: done CN-04 connectivity data facade (BuildConnectivity, ratsnest, queries,
  IsConnectedOnLayer: 931 fields on the golden boards + 3052 on synthetic via / zone boards,
  0 mismatches; corpus build 372 boards, 0 panics; pads unverified until the pad parser
  lands). Pointer-order replacements to confirm in the connectivity review: GetNetItems /
  GetConnectedTracks / GetConnectedPads / GetZoneIslandConnections by ItemId,
  GetConnectedPadsAndVias by list index (teardrops depend on it).
  RUNNING NOW: INT-2a -> INT-2b on Fable (wyvjfdjmb: kernel change requests, then the review
  of every decision / deviation in kicad-pcb; writes docs/design/integration-2.md and
  review-2.md, may write FIX-<n> packages to launch); M25a and CLEAN-1 (w2twswu4w);
  PCBIO-15 and ZF-01 -> ZF-03 -> ZF-06 -> TD-01 (w0obou2da); M19(done) -> PCBIO-06 ->
  PCBIO-07 and M21(done) -> M20 (w3kdexuvx); FAB-04 -> .. -> FAB-08 (wb9xlvw89); PROP-1 ->
  DRC-21 -> DRC-22 and DRC-06 -> .. -> DRC-12 (w878ykkp2); DRC-24 -> 26 -> 23 -> 29 and
  DRC-27 -> 25 -> 28 (wxpfc9smx).
  Also running: the dedicated Fable reviews REV-CN (connectivity, pointer-order site table)
  and REV-DRC (DRC engine core: EvalRules, rule parser, report order) (wh0st1sds); they write
  docs/design/review-connectivity.md and review-drc-core.md and may write FIX-CN-<n> /
  FIX-DRC-<n> packages to launch.
  02:50 UTC: the two dedicated Fable reviews are back (wh0st1sds).
  REV-CN (docs/design/review-connectivity.md): 0 blockers. A new C++ driver
  (/home/user/kicad-oracle/bin/cn-order-oracle) measured the reference's heap order at all 25
  pointer-ordered sites: it is neither file order nor stable between runs (2-12 % of the
  connected lists deviate after load, 24-56 % after the DRC rebuild), so there is no reference
  order to reproduce; the port's list / ItemId order stays and such results are compared
  tie-aware. One real bug found and fixed (F-2: SHAPE_SEGMENT collision is asymmetric and the
  C++ tests a dirty pair from both sides; the port missed an odd-width / even-width track
  pair). Open: F-1 ratsnest ends under ties -> FIX-CN-1 (launched, Opus); minor F-6..F-10.
  REV-DRC (docs/design/review-drc-core.md): 0 blockers, no port code had to change; the
  user-rule half of EvalRules got differential coverage (4 review boards x 1570 lines,
  identical). Open: R-01 rule identity behind GetParentRule -> FIX-DRC-1 (launch after INT-2a,
  it needs kernel file drc_api.rs); R-02 DrcEngine::run_tests cannot drive a real run (DRC-12
  settles it); R-06 pad branches unverified until the pad parser lands. Lists of places where
  the reference itself is not reproducible (thread pool at error limits, GetObjectsAt hash
  order, pointer-keyed maps) are in both documents: the campaign compares those as sets.
  Tests of other packages failing at review time (to recheck in the campaign):
  drc21_pcbexpr (8), drc24_courtyard_text (2: courtyard cases once footprints started to
  load).
  03:10 UTC: done FIX-CN-1 (tie records on ratsnest edges + tie-aware comparison rule;
  evidence run of cn-order-oracle on all 365 DRC corpus boards: no board has a
  heap-dependent ratsnest in the reference, so corpus comparisons stay exact).
  03:10 UTC: done (results read on Fable): INT-2a kernel integration (design rev 2.3;
  docs/design/integration-2.md; a reference driver with 354 mutator values, all equal;
  defaults scoreboard 3558 match / 124 pending / 0 mismatch), PCBIO-06 footprint parser +
  writer (46 of 46 re-saves byte-identical; corpus scoreboard 98 identical, pads pending on
  245), M20 footprint geometry / courtyards (48 of 48 reference values), PCBIO-15 loader
  sequence (item level: 21912 match, 0 mismatch; upgrade corpus 111 identical, 0 differing,
  259 waiting for pads), ZF-01 zone fill driver (no fill compared yet: needs ZF-03), FAB-04
  pads / vias / tracks / zones plotting (3118 corpus Gerber files + 26 crafted pad files
  byte-identical), M25a barcode model (no symbol encoder: zint port is a later package),
  PROP-1 property tables (1286 of 1286 table lines identical), CLEAN-1 (std::set tree and
  wxString::Matches moved with oracles, libeval duplicates removed, font x86 casts), DRC
  providers DRC-06, 07, 23, 24, 25, 26, 27, 28 (each 0 differences on every board that loads;
  pad paths wait for the pad parser).
  Known red tests at this point (owners gone; GLUE-2 handles them): parser / writer
  staging_stubs, netclass_embedded oracle_corpus_sections (issue25265), pcbio12
  oracle_corpus_blocks (drill chart cells), drc24_courtyard_text (2), drc21_pcbexpr (in
  progress), drc08a (in progress).
  RUNNING NOW: INT-2b Fable review of kicad-pcb (wyvjfdjmb); GLUE-2 second wiring pass and
  FIX-DRC-1 rule identity (wz048ddhz); CC-1 component class sync - the stub every CLI load
  hits (wu41nwrio); PCBIO-07 pads (w3kdexuvx); ZF-03 -> ZF-06 -> TD-01 (w0obou2da); FAB-05
  -> .. -> FAB-08 (wb9xlvw89); DRC-21 -> DRC-22 and DRC-08a -> .. -> DRC-12 (w878ykkp2);
  DRC-29 (wxpfc9smx).
  03:25 UTC: MILESTONE - PCBIO-07 pads / padstacks done: the io corpus now re-saves 359 of
  370 boards byte-identically to `kicad-cli pcb upgrade --force` (all 287 fixed points; 0
  pending, 0 errors; the 11 differing boards differ only in zone net / priority from the
  reference's post-load steps and in outline-font render caches). Also done: DRC-29 sliver
  and connection width (309 boards compared at item level through a new C++ driver, all
  identical). Orchestrator fix in kicad-geom: `ShapePolySet::is_exterior_waist` wraps like
  the reference (issue25089/odb_circle_edge_cases panicked in debug builds).
  Found by PCBIO-07 in the model: pad margin setters wrote the front side only, pad
  primitives had no parent -> FIX-PAD-1 (launched). With pads loading, several provider
  tests of finished DRC packages went red (drc07 corpus, drc24, drc25 synthetic, drc27):
  expected - first real pad comparisons; a DRC stabilisation package follows once FIX-PAD-1
  and GLUE-2 have landed and the provider chains have finished.
  Launched: FIX-PAD-1 -> PCBIO-14 (footprint library files) and STAB-CN (connectivity with
  pads: rerun every oracle, thirty more boards, net propagation scoreboard) (w16z105v0).
  Next heartbeat 04:25 UTC. Disk watchdog restarted 03:27 UTC as background task b9699ml7v (2 h lifetime: restart about 05:25 UTC).
  03:45 UTC: done CC-1 component class sync (the stub every CLI load hit): `kicad-rs pcb
  upgrade --force` is now byte-identical to the reference end to end; session upgrade corpus
  358 identical / 11 different / 1 not loadable. 10 of 36 footprint classes on the two
  component-class boards wait for the DRC-22 expression functions. Launched: FIX-RESAVE-1
  (the 11 differing boards: zone net / priority, render caches) (w1juzof4g).
  04:00 UTC: done (read on Fable): GLUE-2 (kicad-pcb tests all green again: lib 584 +
  integration; defaults 3564 / 118 / 0), FIX-DRC-1 (rule_index identity), FIX-PAD-1 (41
  boards: zero pad / item mismatches), STAB-CN (NO defect found in connectivity with pads:
  41 more boards in the goldens, all fields equal; net propagation on 368 boards: 84,191
  items, 0 mismatches), ZF-03 zone copper fill (FIRST FILL COMPARISON: 330 zone layers
  vertex-identical to `kicad-cli --refill-zones`, 6 differ on two boards whose custom rules
  needed the expression binding; copper thieving ZF-07 pending; 13 teardrop boards wait for
  TD-01), FAB-05 graphics + FAB-06 text plotting (full Gerber driver: 8438 of 8533 corpus
  files byte-identical, 88 pending on FAB-10 / 13, 7 known differences from model defects:
  3 fixed since by FIX-PAD-1, 2 table-cell geometry in a footprint, 2 barcode layers), DRC-21
  expression binding (EvalRules 1244 categories on 102 boards, 0 mismatches; function bodies
  = DRC-22, running), DRC-08a + 08b copper clearance (332 boards: clearance 2067,
  hole_clearance 1407, shorting 371 matched; differences only on 2 boards over the error
  limit - reference order there is thread-dependent - and 2 boards with rule-condition gaps).
  Launched: STAB-PCB (wfdnc7cds) with the model defects reported so far (table cells in
  footprints, render cache dropped at load, dimension text polygons, fp_poly closing vertex,
  pad primitive lib scale, raw zone_connect values).
  04:10 UTC: INT-2b, the Fable review of every decision in kicad-pcb, is back
  (docs/design/review-2.md): 238 status entries judged (181 ok, 51 documented difference, 3
  wrong, 3 unproven); 16 findings (1 blocker, 4 major, 11 minor), 5 fixed with regression
  cases: pad margin setters wrote one side (blocker; found by a new whole-corpus item sweep
  of 7.3 million values, not by any status entry), table cells in footprints stayed at
  library coordinates, ${PROJECTNAME} / clock text variables not wired, ${VCSHASH} gave ""
  instead of "no hash", fresh UUIDs could collide with UUIDs of the file (the parser now
  reserves the file's UUIDs). Legacy-input re-save scoreboard (D3): 361 of 370 identical,
  the other 9 explained (4 outline-font caches, 4 post-load steps, 1 generated-table
  refresh). Open: R2-06 outline fonts (see decision), R2-07 Bezier side effect (FIX-BEZ-1,
  no corpus board affected), R2-11 private UUID generators in drc / fab, R2-12 generated
  tables not refreshed at load, R2-13 barcode encoder, minor documented ones.
  ORCHESTRATOR DECISION (R2-06): the reference never uses the render cache stored in a
  board file - it regenerates outline-font glyphs from the host's fonts. The port has no
  outline engine, so the stored glyphs become the stand-in (they are what the reference
  produces on a machine that has the font); stroke-font fallback plus a host warning
  otherwise. Package FONT-RC-1 is written; launch after STAB-PCB (same files). A real
  outline engine (FreeType / HarfBuzz equivalents, embedded fonts) stays a later package.
  04:20 UTC: FIX-RESAVE-1 done - all 370 corpus boards of the re-save comparison are
  explained and none of the 11 differing ones is a port defect: 358 byte-identical, 11
  identical after two normalisations that the reference itself needs (teardrop zones are
  saved without a uuid, get a random KIID at each load and the writer orders zones by uuid:
  12 reference runs on one input gave 2 different outputs; render-cache glyphs come from
  whatever font fontconfig substitutes on the oracle machine), 1 not loadable
  (api_kitchen_sink: generated tables refresh, M26). `kicad-rs pcb upgrade` messages and
  exit codes equal the reference in 36 of 36 runs. A default-run regression test over 308
  boards guards it (kicad-session upgrade_corpus_small).
  04:30 UTC: done PCBIO-14 footprint library files and `fp upgrade`: all 426 corpus
  .kicad_mod files re-save like the reference (393 byte-identical, 33 identical after
  masking the fresh UUIDs the reference draws for legacy items, 0 differing); messages and
  exit codes equal in the five CLI cases. The board reader / writer packages are complete.
  Launched: ZINT-1 (new crate kicad-zint: port of the zint subset behind PCB_BARCODE -
  Code 39, Code 128, QR, Micro QR, Data Matrix - with a differential oracle against the
  bundled C library).
  04:45 UTC (heartbeat; next 05:28): done and read on Fable -
  STAB-PCB: whole-corpus model scoreboard (rust/tools/pcb-dump/scoreboard.sh, 344 boards
  under 1 MB, every section): 6,136,472 values match, 64 mismatch - all explained (38
  outline-font text on one board, 16 never-read lib coordinates of table cells, 6 barcode
  boxes, 5 order of UUID-less items) - 15,450 pending (15,436 meander settings, not
  modelled). Fixed on the way: pad primitive lib scale on flip, raw zone_connect values
  (the reference writes 7 back as 7), dimension value mode, dump wiring.
  FAB-07 gerbers job through the whole `kicad-rs pcb export gerbers` path: 760 corpus runs
  (380 boards x 2 option sets): 690 fully identical (exit code, stdout, 15,506 files), 62
  pending on FAB-10 (mask minimum width: 31 boards - next in the fabrication chain), 2 on
  generated tables, 4 barcode files, 0 other differences. The three B.Mask "known
  differences" of the plot corpus were the missing rule resolver, not the model.
  ZF-06 refill + save through the session: 125 of 126 runnable boards byte-identical after
  `--refill-zones --save-board` (the other one is random in the reference), zone layers
  336 of 336 identical; 13 boards wait for teardrops (TD-01, running), 2 for copper
  thieving (ZF-07, to write).
  DRC-09 hole-to-hole / edge clearance and DRC-10 disallow: every entry matches on 332 /
  362 boards except 2 extra on issue23469 (the known rule-condition gap).
  Launched: FONT-RC-1 (render cache stand-in) and M26 (generated tables + refresh)
  (w25gr5ufv). Running: DRC-22, DRC-12, FAB-09 -> FAB-08, TD-01, ZINT-1.
  Next: [FAB-10, FAB-11, FAB-12] when the fabrication chain ends; STAB-DRC when DRC-22 and
  DRC-12 end; WIRE-1 after DRC-12 and FAB-08; ZF-07 after TD-01; GLUE-3 when the chains are
  quiet; then the cross-cutting adversarial review, README and the performance phase.
  04:55 UTC: the first fabrication chain is complete: FAB-09 Gerber job file (704 of 704
  compared .gbrjob files byte-identical) and FAB-08 Gerber drill writer (831 corpus runs
  identical, 0 differ). GAP FOUND: the drill enumerator still used FAB-01's via placeholders,
  so every board with vias stopped in drill export (309 runs pending) - launched FAB-GLUE
  (enumerator on the real via model + the crate's leftovers, drill export verified on every
  corpus board with holes) -> FAB-10 mask minimum width / clipped silk -> FAB-11 SVG plots,
  drill marks, svg job -> FAB-12 drill maps (wvkfmdjv4).
  05:20 UTC: done ZINT-1 - new crate kicad-zint (port of the zint 2.15.0 subset behind
  PCB_BARCODE: Code 39, Code 128, QR, Micro QR, Data Matrix, vector output; BSD-3 / MIT
  notices kept in the files - list them in the final README / third-party document):
  319,061 cases against the reference build's libzint identical (return code, error text,
  bitmap, every rectangle as float bits); barcode polygons, error texts and boxes equal the
  reference dump on 600 of 600 lines; the plot and gerbers-job corpora now have 0 known
  differences. Leftover switches named in its report (dump defaults, test skip lists) go to
  FAB-GLUE / GLUE-3.
  05:25 UTC: done FONT-RC-1 (stored glyphs as stand-in for outline-font text: on a crafted
  board in fonts the oracle machine has, re-save, 8 Gerber layers + job file and the DRC
  text checks are identical to the reference; every corpus outline text (155 on 5 boards)
  has a valid stand-in; the plot_text corpus is now 75 of 75) and DRC-22 (all expression
  functions: 36 argument-check messages equal, EvalRules oracle 2378 categories match on
  349 complete boards, 1 documented layer-map deviation; fromTo() needs the from-to cache).
  ORCHESTRATOR DECISION: the TrueType branch of the DRC text thickness check unfractures a
  copy of the stand-in glyphs (stored glyphs have holes fractured into slits; with the
  unfracture the check equals the reference at every tested threshold); plot and save keep
  the stored polygons. Goes into STAB-DRC.
  Disk watchdog restarted 05:13 UTC as background task bbwthhml5 (restart about 07:10).
  05:35 UTC: launched the next wave - DRC-30 diff pair coupling -> DRC-31 matched length /
  skew / via count / net chains (with the length calculation) -> DRC-34 leftovers, and
  CN-05 from-to cache (wf3foz4r4); DRC-32 -> DRC-33 creepage (w2bvh6638); the owed Fable
  reviews REV-ZF (zone filler: coverage map of every fill branch, crafted boards for the
  uncovered ones) and REV-FAB (fabrication: option and item coverage against the
  reference, statement-level reading of aperture / attribute / number formatting)
  (wktru4kxr). Running besides: DRC-12, TD-01, M26, FAB-GLUE -> FAB-10 -> 11 -> 12.
  05:20 UTC (machine clock; some earlier stamps in this log run up to 20 min fast):
  MILESTONE - DRC-12 done: `kicad-rs pcb drc` runs the real DRC (cache generator, every
  ported provider in registry order, expression cache scope, markers, exclusions, text and
  JSON reports, exit codes). Against the reference kicad-cli on 364 corpus boards: stdout
  counts and exit codes agree on all 364; reports: 47 byte-identical, 254 identical after
  filtering the codes of providers not ported yet (reported as "checks not run", never
  silently omitted), 51 identical as sets (reference order is thread-dependent), 12 differ
  with known causes (7 tuning profiles not loaded, 3 drawing-sheet text variables, 1
  outline font, 1 pointer-order in the reference, 1 barcode clearance to recheck now that
  the encoder exists). QA false-positive regression table: 21 of 21.
  Launched: STAB-DRC (fix loop over every provider and corpus board with pads) and WIRE-1
  (drill job, host inputs, tuning profiles, wasm API, browser demo, e2e scoreboard)
  (w2agdqkgg).
  Still to write: DS-1 drawing sheet model (default sheet + .kicad_wks) for the DRC text
  variable check and `--ibt` plots; ZF-07 copper thieving (after TD-01); FONT-OL-1 (an
  approximate outline engine is possible with ttf-parser + rustybuzz, but the reference
  loads hinted outlines through FreeType, so bit equality is out of reach: low priority).
  05:27 UTC: done TD-01 teardrops (manager + geometry, both analysis rows): 283 of 283
  teardrop zones identical to the reference (25 boards x 6 parameter variants + 2 synthetic
  crossing boards: type, layer, net, priority, every outline vertex); with teardrop
  regeneration before the fill the zone corpus is 656 of 656 zone layers identical and the
  refill-and-save comparison 233 identical / 1 random in the reference / 2 waiting for
  copper thieving. Launched ZF-07 copper thieving (wgcovhd2v).
  05:30 UTC: done M26 generated tables (drill chart, stackup table) and their refresh at
  load / before plot / in upgrade: 13 boards (7 crafted x charts and stackups, 4
  second-generation, api_kitchen_sink, modern_metadata) - every generated block and the
  committed drill symbol profile identical to the reference upgrade, first and second
  pass; api_kitchen_sink (the last unloadable corpus board) now upgrades byte-identically.
  Open from its report (in GLUE-3): the drill chart staleness generation (a second
  refresh in one session counts the chart again: check `pcb upgrade` without --force), the
  table autosize / flip hooks, the duplicated drill model in kicad-pcb and kicad-fab.
  Drill and chart symbol plotting is FAB-13 (to write, after the fabrication chain).
  Cross-crate leftovers for a GLUE-3 pass (package file written; its kicad-fab part moved
  into FAB-GLUE) once the FAB / DRC / ZF chains have finished:
  kicad-fab local pad primitive ports and via_access.rs -> model methods, default round-rect
  generator; kicad-drc local copies -> model; RuleResolver::eval_zone_connection; Board::uuid
  + KiidGenerator seeding; pcb-dump-session -> dump_board_ctx.
  Package files written, to launch when their inputs land: (after the DRC chains: DRC-22 and DRC-09 -> 10 -> 12 are running), WIRE-1 (after DRC-12,
  FAB-07 / 08), FAB-10 / 11 / 12 (after the FAB chain).
  Still to write (tier 3, editor-only paths): FIX-GEN-1 (generator subclass arms of move /
  rotate / flip / mirror / set_layer, via stitch default template), FIX-DIM-1
  (PCB_DIMENSION_BASE::Update after text setters, i32-backed dimension enums, grid item
  radius setters), ZINT-1 (barcode symbol encoder: CODE_39, CODE_128, QR, Micro QR, Data
  Matrix; thirdparty/zint is BSD-3), M26 (generated tables refresh, drill chart).
  READY TO LAUNCH (package files written): after PCBIO-07 finishes (workflow w3kdexuvx):
  [PCBIO-14]; after the FAB chain (wb9xlvw89): [FAB-10], [FAB-11], [FAB-12] (one agent at a
  time in kicad-fab: chain them); after DRC-12, FAB-07 / 08 and PCBIO-15: [WIRE-1]
  (session / CLI / wasm end-to-end wiring + e2e differential scoreboard + browser demo). After pads parse: the dedicated Fable reviews (connectivity, DRC resolver /
  EvalRules, zone filler) and a rerun of every oracle that skips footprint boards today.
  Dedicated Fable reviews are postponed until pads parse (then the EvalRules and
  connectivity oracles cover pads and a review can look at real mismatches).
- next up when their dependencies land: M19 footprint A (after M9, M18), M21 board outline
  (after M9), M20 footprint B; PCBIO-06 footprints -> PCBIO-07 pads (after M19; this unlocks
  real boards end to end), PCBIO-12 / 13 / 14; DRC providers DRC-06 .. DRC-10 and DRC-12
  (after DRC-04 / 05); FAB-03 .. FAB-09 board plotting; PCBIO-15 loader glue in
  kicad-session (after CN-04); zone fill.
- Fable integration #2 (DEFERRED until M9 / M11 / M12 / M15 / M18 are done, because it
  changes trait signatures in files those agents edit): apply the collected change requests
  (regenerate rust/target/orchestrator/change_requests.txt from the workflow journals first)
  - traits.rs (EdaShapeItem::corners_in_sequence(ctx), BoardItemMut::style_from_settings(
  &MutEnv), a SetNet virtual), ctx.rs (LayerEnv LAYER_T, ConnectivityQuery::net_items),
  item.rs (ItemFlags STARTPOINT / ENDPOINT), drc_api.rs (DrcConstraint::name()), dump (LAYER_T
  names; wire every `no_api(..)` whose getter now exists; meander lines), footprint
  functions other packages inlined or stubbed, apply_net_chain_netclasses wiring,
  wx_string_matches to kicad_core, PCB_VIA::ValidateViaParameters, NET_SETTINGS operator== -
  and review the decisions and deviations of every package section in kicad-pcb/PORT_STATUS.md.
- dedicated Fable reviews still owed: connectivity pointer-order replacements (after CN-04),
  DRC resolver / EvalRules (after DRC-04), zone filler.
- note for the user: one pushed commit (58d2d62f) carries a model name in its Co-Authored-By
  trailer; rewording it needs a force push, which was not permitted.

Queue - launch each item as soon as its dependencies have landed (model package rows and
run-time dependencies: docs/design/board-model.md section 16; other packages: the analysis
docs' section 10). Keep at most ~8 agents inside kicad-pcb at once.
1. Model wave 1b: M5 design settings, M6 stackup + plot params, M24 images / embedded files /
   component classes (after M0; start when M1/M2/M3/M13 slots free up).
2. Model wave 2: M4 (after M3), M8 (after M7), M14 (after M13, M2), M16 (after M2), M18
   (after M2, M5), M22 (after M1, M3), M23 (after M2).
3. Model wave 3: M9 (after M8), M12 (after M10, M8), M17 (after M13, M16).
4. Model wave 4: M11 (after M9, M10), M15 (after M14, M9; needs G4-G6), M21 (after M9; needs
   G4, G7), M19 (after M1, M14, M9, M10, M18).  Wave 5: M20 (after M19, M15, M21).
5. Settings loaders (kicad-pcb src/project, see docs/analysis/settings.md SET-3/4/5/8):
   NET_SETTINGS JSON + migrations (after M4), BOARD_DESIGN_SETTINGS JSON keys + severities
   (after M5), project file subset + SetProject + SynchronizeNetsAndNetClasses (after both),
   text variable resolvers (with M22).
6. Parser / writer fan-out after PCBIO-02: PCBIO-03 tracks/vias/shapes, PCBIO-04 setup,
   PCBIO-05 text, PCBIO-08 zones, PCBIO-09 groups, PCBIO-11 legacy netclasses / embedded
   files; then PCBIO-06 footprints -> PCBIO-07 pads -> PCBIO-10 dimensions; tier 2: PCBIO-12,
   13, 14. PCBIO-15 (BOARD_LOADER glue, re-save command) goes into kicad-session once
   connectivity exists. Drive them with the PCBIO-17 corpus scoreboard and the dump scoreboard.
7. Connectivity (kicad-connectivity, docs/analysis/connectivity-zones.md): CN-01 -> CN-02 ->
   CN-03 -> CN-04 after G5 and model M1-M3, M9, M14, M16-M18 (M15 for contact shapes). A
   Fable review of the pointer-order replacements (R-1, R-2) before DRC builds on it.
8. DRC (kicad-drc, docs/analysis/drc.md): DRC-02 r-tree/provider base, DRC-03 engine core,
   DRC-04 EvalRules, DRC-05 caches (Fable review of the resolver / run-state split and of
   EvalRules), then providers DRC-06, 07, 08a, 08b, 09, 10, CLI flow DRC-12; tier 2: DRC-21
   expression binding, DRC-22 functions, DRC-23..29; tier 3: DRC-30..34.
9. Fabrication (kicad-fab, docs/analysis/plot-fab.md): FAB-03 board plot driver, FAB-04
   standard layer, FAB-05 graphic items, FAB-06 text, FAB-07 gerbers job, FAB-08 Gerber
   drill, FAB-09 gbrjob, FAB-T1 harness; tier 2: FAB-10, 11, 12.
10. Zone fill (kicad-zone): ZF-01..06, TD-01/02 teardrops, after connectivity and EvalRules.
    Fable review of the fill pipeline port (vertex-exactness is the hard part).
11. Wire everything into kicad-session / kicad-cli / kicad-wasm (W1 leaves the hooks), browser
    demo with real DRC + Gerber export, node + Playwright smoke tests.
12. Cleanup pass: kicad-plot round-rect closures -> kicad-geom (after G6b); SVG rounded Rect;
    Gerber PlotText debug_assert; kicad-font x86 cast rule + FontRef into kicad-font;
    kicad-libeval duplicates -> kicad-core; Pad::default RECT_CHAMFER_POSITIONS literal;
    `rg foundation_stub` empty; Shape::indexable_subshape (G5).
13. Differential test campaign: io corpus byte equality, dump scoreboard to zero mismatches on
    the small-board set, DRC report equality (text + JSON) on qa DRC boards and demos, Gerber
    and drill byte equality, qa DRC regression tables (drc.md) as Rust tests; fix loops until
    dry; cross-cutting adversarial Fable review; final rust/README.md (scope, how to build
    native + wasm, what matches KiCad and what does not, AGENTS.md policy note).
14. Performance phase (below).

## Performance phase (requested by the user; after the differential test campaign)

Goal: hill-climb the speed of the Rust port without changing a single output byte.

1. **Harness first.** A `kicad-bench` crate (or `benches/` in kicad-cli) timing, on a fixed
   set of corpus boards (small / medium / the two large demos): parse, write, connectivity
   build, DRC, zone refill, Gerber + drill export; natively (release profile) and as wasm
   under node. Record the reference `kicad-cli` wall-clock for the same operations on the same
   machine as the baseline. Results go to `docs/PERF.md` (table per commit).
2. **Profile.** `perf` / flamegraphs natively; for wasm, node's profiler. Rank hotspots.
3. **Loop until dry** (workflow): each round, Opus agents take the top hotspots (one each,
   disjoint files), propose and implement one optimisation, and a change is kept only if
   (a) every differential oracle and golden test still passes bit for bit and (b) the
   benchmark improves beyond noise. Stop after two consecutive rounds with no kept change.
   Fable reviews any change that touches a pinned behaviour.
4. **Pinned, not negotiable:** libstdc++ sort tie order, r-tree visit order, Clipper2 output,
   floating-point expression order, iteration orders listed in the analysis docs. Wins come
   from allocation, caching, data layout and avoiding repeated work - not different algorithms.
5. **Known suspects** (from reviews so far): `KeyedCache` costs a mutex lock + `Arc` clone per
   read on hot paths (pad shapes in DRC and plotting); `Shape::indexable_subshapes()` allocates
   a Vec per call; `Box<dyn Shape>` churn in collisions; the board time stamp invalidates all
   stamp-keyed caches on every edit; netclass pattern matchers rebuilt per lookup; dev profile
   uses opt-level 1.
6. **Parallelism.** KiCad runs DRC, zone fill and connectivity on a thread pool; the port is
   sequential by rule. Add optional `rayon` behind the non-default `parallel` feature only
   where output order can be kept deterministic (collect per item, merge in item order).
   wasm stays single-threaded.
7. **wasm build tuning:** opt-level s vs 3, LTO, `wasm-opt`, panic=abort; report size and speed.

## Notes for the orchestrator

- Separate verify agents were dropped for packages whose implementer built a differential
  oracle against the real C++ (cheaper and stronger). A cross-cutting adversarial review is
  planned at the end.
- Known duplication to consolidate later: printf-style formatting lives in
  kicad-sexpr/src/formatter/cformat.rs; kicad-libeval carries local copies of eda_units /
  string_utils helpers.

## Model policy (standing instruction from the user)

- Port, analysis and report agents run on Opus to save the Fable quota.
- **Every decision an Opus agent makes is reviewed by Fable before others build on it**: the
  board-model design choice, crate/dependency recommendations, each package's listed deviations
  and known gaps, public API shapes that downstream crates will depend on, and any expectation
  an agent changed in a ported test. The orchestrator (Fable) reads every structured result;
  anything non-trivial gets a dedicated Fable review agent that checks the decision against the
  C++ and either confirms it or sends it back. Reviews target decisions, not line-by-line code.
- A package moves to Fable outright only if Opus gets stuck on it.
