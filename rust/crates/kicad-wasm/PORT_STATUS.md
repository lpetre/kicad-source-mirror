# kicad-wasm port status

New code (no C++ counterpart): the wasm-bindgen JavaScript API over `kicad-session` and the
ported libraries. API surface and status per call: `README.md`.

## What runs today (wasm == native, pinned by `tests/golden/native_reference.json`)

| JS | Rust | Verified |
|---|---|---|
| `version`, `versionInfo` | `kicad_session::build_version` | equals `kicad-cli version` |
| `prettify` | `kicad_sexpr::prettify` (`KICAD_FORMAT::Prettify`) | kicad-sexpr oracle; 3 corpus boards native == wasm |
| `tokenStats` | `kicad_sexpr::DsnLexer` (`pcb_keys_t`) | 3 corpus boards native == wasm |
| `strokeText` | `kicad_font` `FONT::Draw` + `CALLBACK_GAL` truncation | kicad-font oracle; 4 strings native == wasm |
| `polygonBoolean` | `kicad_geom::ShapePolySet` boolean ops (vendored Clipper2) | kicad-geom oracle; 8 cases native == wasm |
| `plotDemoGerber` | `kicad_plot::GerberPlotter` | byte-identical to the C++ `GERBER_PLOTTER` (`tests/golden/demo_gerber_cpp.gbr`, `tests/oracle/regen_demo_gerber.sh` via the kicad-plot oracle driver); native == wasm |
| `newBoard`, `Board.nets/tracks/addTrack/addVia/remove/save` | `kicad_session::Session` on the kicad-pcb model | item lists and saved text native == wasm |
| `loadBoard` + `nets/tracks/save` | `Session::load_board` (`kicad_pcb::io::parse_board`), `format_board` | `qa/data/pcbnew/issue10697.kicad_pcb`: native == wasm, and the saved text is byte-identical to the reference `kicad-cli pcb upgrade --force` (`loaded_board_save_matches_cpp`) |

| `runDrc`, `exportGerbers`, `exportDrill` (+ `loadBoard` with project / rules, fixed `uuidSeed` / `date`) | `kicad_session::run_drc`, `run_export_gerbers`, `run_export_drill` | report + JSON, every Gerber (with the job file) and drill file of 3 corpus boards native == wasm (`jobs` in the golden, 15 checks in `smoke.mjs`); the browser test compares the page's DRC report and 22 fab files of `demos/ecc83/ecc83-pp.kicad_pcb` with the native `kicad-rs` CLI (which the e2e test compares with the reference) |

`plotSvg` is being wired by FAB-11 (the smoke test accepts SVG text or a typed error).

## Pipeline

`rust/scripts/build-wasm.sh [--test]`: `cargo build -p kicad-wasm --target
wasm32-unknown-unknown --profile wasm-release`, `wasm-bindgen` 0.2.130 for `--target nodejs`
(`web/pkg-node`) and `--target web` (`web/pkg`), sizes, optional `wasm-opt`, then
`web/test/smoke.mjs` (58 checks) and `web/test/browser.mjs` (Playwright/Chromium on a local
static server, `web/index.html` + `web/app.js`).

Size (wasm-release, no wasm-opt): 8,496,478 bytes from cargo, 7,715,653 after wasm-bindgen
(WIRE-1, with DRC, zone fill and the fabrication writers linked in).

## Deviations / notes

* libm: on wasm32 Rust's libm replaces glibc (PORTING.md 9); none of the reference results
  differ today.
* With `panic = "abort"` (wasm-release profile) a call that reaches a kicad-pcb `port_pending!`
  stub traps (`WebAssembly.RuntimeError`); the message goes to `console.error` through
  `console_error_panic_hook`. `exportGerbers` / `plotSvg` on a `newBoard()` board do this today
  (`BOARD::GetLayerName` is pending).
* UUIDs: `loadBoard` / `newBoard` reseed the board's `KiidGenerator` with 32 bits from
  `crypto.getRandomValues` (`Math.random` without `crypto`), or the `uuidSeed` option; the DRC
  marker seed likewise. Dates: the browser clock in local time with its UTC offset
  (`Date.getTimezoneOffset`), or the `date` option.
* `Board.warnings`: parser warnings, then outline font fallbacks.
* `wasm-bindgen` warns that the `target_features` section is missing because the workspace
  `wasm-release` profile sets `strip = true` (harmless; see change requests).

## Known gaps

* `wasm-opt` is not installed on the build machine; size optimisation is skipped.

## Package DRC-LP: footprint libraries for `runDrc`

`DrcOptions::libraryFiles` (`[{ path, content }]`: project-relative or absolute paths of the
project `fp-lib-table`, nested tables and `<lib>.pretty/<name>.kicad_mod` files),
`globalLibraryTable` (text of the global `fp-lib-table`) and `libraryEnv` (URI variables)
feed an in-memory library source (working directory `/`); without files and global table the
library parity test has no tables, as kicad-cli without any `fp-lib-table`.
