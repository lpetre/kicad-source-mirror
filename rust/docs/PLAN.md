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
- [ ] Board model design: Opus synthesis chose proposal B (typed arenas + Copy handles) with
      grafts from A and C (docs/design/board-model.md + board-model-types.md). Dedicated Fable
      review running; nothing is built on it until that verdict is in. Work packages M0..M26
      (M0 = types-only skeleton, blocks everything).
- Foundation crates:
  - [x] kicad-sexpr (S1) - lexer, formatter, Prettify, tree; oracle goldens over 479 files.
  - [x] kicad-geom G1 math - 1M-case differential oracle bit-exact.
  - [x] kicad-geom G2 basic shapes (Opus; 606k oracle cases bit-exact; Fable-reviewed:
        deviations D11-D16 accepted except D12 - out-of-range casts now follow x86 semantics,
        PORTING.md section 9, applied by G3).
  - [x] kicad-geom G7 stdsort / packed + dynamic r-tree / delaunator / nanoflann (Opus; 10k
        oracle cases bit-exact natively and on wasm; COW_RTREE skipped, router only).
  - [ ] kicad-geom G3 line chain -> G4a clipper vendoring + poly set, G6a helpers (running).
        Then G4b triangulation, G5 collisions, G6b shape utilities.
  - [x] kicad-core C1 ids/layers; [x] C2 units/strings/values (Opus; 152k oracle cases
        bit-exact; Fable-reviewed: deviations C2-1..9 accepted).
  - [x] kicad-libeval (Opus finish; Lemon tables regenerated and identical, 39.5k oracle
        cases; Fable-reviewed: binder API accepted, to be proven by the kicad-drc binding).
  - [ ] kicad-font F1 stroke font (running, Opus).
- [ ] Board model + parser + writer.
- [ ] kicad-plot P1 base+metadata -> P2 Gerber -> P3 text+SVG (running, Opus); kicad-core C3
      json / pattern match / helpers (running, Opus).
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
