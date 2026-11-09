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

- [x] Workspace skeleton, porting rules.
- [ ] Oracle build (background).
- [ ] Third-party equivalents research -> `docs/THIRD_PARTY_EQUIVALENTS.md`.
- [ ] Subsystem analysis docs -> `docs/analysis/`, board model design -> `docs/design/`.
- [ ] Foundation crates: sexpr, geom, core, libeval, font.
- [ ] Board model + parser + writer.
- [ ] Connectivity, plot, fab, drc.
- [ ] Zone filler, custom rules, remaining providers.
- [ ] CLI, wasm, demo.
- [ ] Differential test campaign + adversarial review.
