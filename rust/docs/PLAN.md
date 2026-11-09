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
