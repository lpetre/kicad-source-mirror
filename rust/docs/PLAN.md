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

Reusable workflow scripts (invoke with `scriptPath` + `args`, no need to resend the script):
- `.../workflows/scripts/model-wave-wf_9da70494-9c4.js` - args `{integration?: text,
  packages: [{id, title, modules, cpp, notes}]}`: optional Fable integration step, then the
  kicad-pcb model packages in parallel (2 per workflow).
- `.../workflows/scripts/port-packages-wf_122f1b23-a17.js` - args `{chains: [[{id, title,
  crate, body, model?}, ...], ...]}`: chains in parallel, packages of a chain in order, each
  receiving its predecessor's notes. Generic for every non-model package.
(Directory: /root/.claude/projects/-home-user-kicad-source-mirror/76042f1d-984e-5683-9c58-69f73f0a28c8/)

Progress log of launches (workflow task ids):
- done: M1 containers/index/create-attach, M2 item base (wnw19yvig; Opus; Fable-reviewed:
  decisions accepted; create_footprint needs M10's EDA_TEXT::SetVisible at run time).
- running: geometry G4b -> G5 and G6b (wvunbqm6s); Fable integration of wave 0b -> M7, M10
  (wawxgug4p); M3, M13 (w1sw02b68); DUMP-RS (wwiji4o61); PCBIO-01 -> PCBIO-02 -> PCBIO-17
  and DRC-20 -> DRC-11 (wh3q6gcxx); W1 session/cli/wasm skeleton and FAB-01 -> FAB-02
  (wqbof03ge); M5 -> SET-4 (BDS JSON) and M6 (w2szwoq36); M16 -> M17 and M23 (w5kruddbk).
- next up when their dependencies land: M4 (after M3), M8 (after M7), M14 (after M13), M18
  (after M5), M22 (after M3), M24.

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
