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
| `newBoard`, `Board.nets/tracks/addTrack/addVia/remove` | `kicad_session::Session` on the kicad-pcb model | native unit test + node smoke test |

Pending (typed `NotYetPortedError`): `loadBoard` (parser), `runDrc`, `exportGerbers`,
`exportDrill`, `plotSvg`, `save`, `ratsnest`, `refillZones`.

## Pipeline

`rust/scripts/build-wasm.sh [--test]`: `cargo build -p kicad-wasm --target
wasm32-unknown-unknown --profile wasm-release`, `wasm-bindgen` 0.2.130 for `--target nodejs`
(`web/pkg-node`) and `--target web` (`web/pkg`), sizes, optional `wasm-opt`, then
`web/test/smoke.mjs` (43 checks) and `web/test/browser.mjs` (Playwright/Chromium on a local
static server, `web/index.html` + `web/app.js`).

Size (wasm-release, no wasm-opt): 4,070,213 bytes from cargo, 3,592,509 after wasm-bindgen.

## Deviations / notes

* libm: on wasm32 Rust's libm replaces glibc (PORTING.md 9); none of the reference results
  differ today.
* With `panic = "abort"` (wasm-release profile) a call that reaches a kicad-pcb `port_pending!`
  stub traps (`WebAssembly.RuntimeError`); the message goes to `console.error` through
  `console_error_panic_hook`. `exportGerbers` / `plotSvg` on a `newBoard()` board do this today
  (`BOARD::GetLayerName` is pending).
* No `getrandom`: UUIDs come from the board's deterministic `KiidGenerator`.
* `wasm-bindgen` warns that the `target_features` section is missing because the workspace
  `wasm-release` profile sets `strip = true` (harmless; see change requests).

## Known gaps

* `wasm-opt` is not installed on the build machine; size optimisation is skipped.
