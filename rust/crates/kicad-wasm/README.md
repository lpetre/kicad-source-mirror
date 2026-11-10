# kicad-wasm: the JavaScript API

WebAssembly build of the KiCad Rust port (`wasm-bindgen` 0.2.130). It wraps
`kicad_session::Session` (the board, its project and rules) and the ported libraries. All
coordinates are KiCad board units: integer nanometres, y axis pointing down.

```sh
rust/scripts/build-wasm.sh          # web/pkg (ES module) + web/pkg-node (CommonJS)
rust/scripts/build-wasm.sh --test   # ... + web/test/smoke.mjs + web/test/browser.mjs
```

```js
// node
const kicad = require('./web/pkg-node/kicad_wasm.js');
// browser
import init, * as kicad from './pkg/kicad_wasm.js'; await init();
```

## Status legend

* **works** - implemented today on ported code, results equal the native build.
* **pending** - final signature; throws `NotYetPortedError` (property `subsystem` names the
  missing crate / C++ entry point) until the subsystem lands. A call that reaches a
  `port_pending!` stub of the board model instead traps (`WebAssembly.RuntimeError`, the panic
  message goes to `console.error`).

## Module functions

| Function | Returns | Status |
|---|---|---|
| `version()` | `"10.99.0"` (`GetMajorMinorPatchVersion`) | works |
| `versionInfo()` | `{ version, build, commit }` | works |
| `prettify(text)` | the text through `KICAD_FORMAT::Prettify` (what the board writer applies) | works |
| `tokenStats(text, name?)` | `{ tokens, lists, closes, keywords, symbols, numbers, strings, maxDepth, lines, topKeywords: [[name, count]] }` from `DSNLEXER` with the `.kicad_pcb` keywords; throws the `PARSE_ERROR` text on a lexer error | works |
| `strokeText(text, sizeMm)` | `{ path, strokeWidth, segments }`: KiCad stroke font, left/bottom aligned at the origin, SVG path data in nm | works |
| `polygonBoolean(a, b, op)` | polygons: `[[ring0, ring1, ...], ...]`, ring = `[[x, y], ...]`, ring 0 the outline, others holes; `op` = `union`, `difference`, `intersection`, `xor` (`SHAPE_POLY_SET` + vendored Clipper2) | works |
| `plotDemoGerber(date?)` | a small Gerber X2 file from the Rust `GERBER_PLOTTER` (byte-identical to the C++ plotter, `tests/golden/demo_gerber_cpp.gbr`) | works |
| `loadBoard(text, { project, localSettings, rules, name, uuidSeed, date })` | a `Board` after the full `BOARD_LOADER::Load` sequence (project binding, DRC engine with the rules, exclusions, connectivity); `project` / `localSettings` / `rules` are the `.kicad_pro` / `.kicad_prl` / `.kicad_dru` texts, `name` the file name (default output names derive from it); `uuidSeed` fixes the UUIDs the session generates (default: `crypto.getRandomValues`), `date` (`YYYY-MM-DDTHH:MM:SS`) every date the jobs write (default: the browser clock, local time) | works |
| `newBoard(name?, { uuidSeed, date }?)` | an empty `Board` (`new BOARD()`) for clients that build a board item by item | works |

## `Board`

Item handles are `ItemId::to_bits()` numbers (< 2^53, exact as JS numbers), valid for this
`Board` object; UUID strings are what survives `save()` / `loadBoard()`.

| Method | Returns | Status |
|---|---|---|
| `name` (getter) | the board file name | works |
| `warnings` (getter) | the load warnings: parser warnings (`GetParseWarnings()`), then texts drawn with the stroke font because their outline font has no usable stored glyphs | works |
| `nets()` | `[{ code, name, netclass }]` in net code order | works |
| `tracks()` | `[{ id, kind: "track"\|"arc"\|"via", uuid, start, end, mid, width, layer, bottomLayer, drill, net }]` in `BOARD::Tracks()` order; for a via `width` is the diameter | works |
| `addTrack({ start, end, width, layer, net })` | id; `layer` a canonical copper layer name (`"F.Cu"`); a non-copper layer throws `InvalidArgumentError` (`Cannot place Track on non-copper layer`) | works |
| `addVia({ at, diameter, drill, net })` | id (through via F.Cu - B.Cu) | works |
| `remove(id)` | `true` if the item existed | works |
| `runDrc({ units, format, severities, allTrackErrors, refillZones })` | `{ exitCode, files, report, messages }` as `kicad-cli pcb drc` | works (checks kicad-drc cannot run yet are listed in the report) |
| `exportGerbers({ layers, commonLayers, precision, noX2, noNetlist, noProtelExt, disableApertureMacros, useDrillFileOrigin, subtractSoldermask, boardPlotParams, checkZones, variant })` | `{ exitCode, files: [{ name, content }], messages }` as `kicad-cli pcb export gerbers` (with the `.gbrjob` job file) | works |
| `exportDrill({ format, units, separateTh, generateReport, drillOrigin, zerosFormat, ovalFormat, mirrorY, minHeader, gerberPrecision, generateTenting })` | `{ exitCode, files, report, messages }` as `kicad-cli pcb export drill` | works (drill maps are not ported) |
| `plotSvg({ layers, fitPageToBoard, blackAndWhite })` | SVG text (`pcb export svg --mode-single --exclude-drawing-sheet`) | pending (kicad-fab) |
| `save()` | `.kicad_pcb` text (`PCB_IO_KICAD_SEXPR::SaveBoard`; byte-identical to `kicad-cli pcb upgrade --force` on the loadable corpus boards) | works (items whose writer package has not landed: `NotYetPortedError`) |
| `ratsnest()` | `Int32Array` `[x1, y1, x2, y2] * n` | works |
| `refillZones()` | - | works |

`messages` are the C++ `REPORTER` lines: `[{ severity: "error"|"warning"|"info"|"action"|..., text }]`.

## Errors

All failures are JavaScript `Error`s with a `name`:

| `name` | When |
|---|---|
| `NotYetPortedError` | the call needs a subsystem that has not landed; `e.subsystem` names it |
| `LoadError` | the board file could not be loaded (`Failed to load board: ...`) |
| `InvalidArgumentError` | a malformed option object or value |
| `StaleIdError` | an id that does not name a live item |

## Tests

* `crates/kicad-wasm/tests/native_reference.rs`: computes every working call natively and
  pins the results in `tests/golden/native_reference.json`; checks `plotDemoGerber` against the
  C++ golden and `save()` of the loadable corpus boards against the reference `kicad-cli pcb
  upgrade --force` output (kicad-session's golden).
* `web/test/smoke.mjs` (node): the same calls on the wasm module must give the same results;
  `newBoard` + items + `save()` and `loadBoard` + `nets()` / `tracks()` / `save()` equal the
  native results, and so do `runDrc` (report and JSON), `exportGerbers` and `exportDrill`
  (Excellon and Gerber) on three corpus boards with their project and rule files (fixed date
  and UUID seed); `plotSvg` must throw `NotYetPortedError`.
* `web/test/browser.mjs` (Playwright + Chromium): serves `web/`, loads `index.html`, checks the
  page loads the module and computes the same token statistics as the native build, then
  loads `demos/ecc83/ecc83-pp.kicad_pcb` with its project, clicks "Run DRC" and "Export
  Gerbers + drill", and compares the DRC report and every Gerber / drill file with the native
  `kicad-rs` CLI run on the same files (same date and UUID seed).
