# The reference oracle

A `kicad-cli` (version 10.99.0, Release, g++ 13 / libstdc++) built out-of-tree from this very
source tree is the ground truth for differential testing of the Rust port. It is a local build
artifact and is **not** part of the repository.

| What | Where |
|---|---|
| Wrapper to call (hermetic HOME, headless) | `/home/user/kicad-oracle/bin/kicad-cli` |
| Build tree | `/home/user/kicad-oracle/build` (CMake Release, Ninja) |
| Static libs for C++ unit-level harnesses | `build/libs/kimath/libkimath.a`, `build/libs/core/libcore.a`, `build/common/libkicommon.so` |
| Rust helper | `kicad_qa::oracle_cli()` returns the wrapper path if present (override with `KICAD_ORACLE_CLI`) |

## Rules for using it

- **Always copy the input board (and its `.kicad_pro`/`.kicad_dru` if any) to a scratch
  directory first.** KiCad writes side files (`.kicad_prl`, lock files, upgraded boards) next to
  the input. Never run it on files inside the repository.
- It prints `Error: schema file '.../api.v1.schema.json' not found` on stderr on every run.
  That is harmless; ignore it.
- For concurrent runs set `KICAD_ORACLE_HOME` to a private directory per test/agent so the
  config directories do not collide.
- Tests that need the oracle must skip silently when `kicad_qa::oracle_cli()` is `None`. Commit
  small golden files produced with it under the crate's `tests/golden/` so the default test run
  needs no oracle.
- Outputs contain dates, version strings and (for items without one) freshly generated UUIDs:
  normalise those before comparing.

## Commands that matter

```
kicad-cli pcb drc [--format json|report] [--units mm|in|mils] [--all-track-errors]
                  [--schematic-parity] [--severity-all|--severity-error|--severity-warning|
                  --severity-exclusions] [--exit-code-violations] [--refill-zones] [--save-board]
                  [-D KEY=VALUE]... [-o OUTPUT_FILE] INPUT_FILE
kicad-cli pcb export gerbers [-o OUTPUT_DIR] [-l LAYER_LIST] [--cl COMMON_LAYER_LIST]
                  [--erd] [--ev] [--ibt] [--sp] [--spn] [--hdnp] [--sdnp] [--cdnp]
                  [--no-x2] [--no-netlist] [--subtract-soldermask] [--disable-aperture-macros]
                  [--use-drill-file-origin] [--precision 5|6] [--no-protel-ext] [--check-zones]
                  [--variant VAR]... [--board-plot-params] INPUT_FILE
kicad-cli pcb export drill [-o OUTPUT_DIR] [--format excellon|gerber]
                  [--drill-origin absolute|plot]
                  [--excellon-zeros-format decimal|suppressleading|suppresstrailing|keep]
                  [--excellon-oval-format route|alternate] [-u in|mm] [--excellon-mirror-y]
                  [--excellon-min-header] [--excellon-separate-th] [--generate-map]
                  [--generate-report] [--report-path FILE] [--generate-tenting]
                  [--map-format pdf|gerberx2|ps|dxf|svg] [--gerber-precision 5|6] INPUT_FILE
kicad-cli pcb export svg ...          (see --help)
kicad-cli pcb upgrade [--force] INPUT_FILE        re-saves the board IN PLACE in the newest format
kicad-cli fp upgrade [-o OUTPUT_DIR] [--force] INPUT_FILE_OR_DIR
```

`kicad-cli pcb upgrade --force` is the reference for the board writer (it runs the full board
loader first: project binding, connectivity build, generated tables), `fp upgrade` is a pure
parse-and-write reference for `.kicad_mod` files.

A DRC run on `demos/pic_programmer/pic_programmer.kicad_pcb` takes about 1.4 s.

## Unit-level C++ harnesses

For differential tests below the CLI level (geometry, formatting, lexing) compile a small C++
driver against the original sources or against the static libraries in the build tree, print
results in a line-oriented text format, and commit the driver (`tests/oracle/`) together with the
golden output it produced (`tests/golden/`). `kicad-geom` and `kicad-sexpr` have working examples
(`tests/oracle/build_oracle.sh`, stub `wx` headers included).

## Rebuilding

```
cmake -G Ninja -S <kicad> -B /home/user/kicad-oracle/build -DCMAKE_BUILD_TYPE=Release \
      -DKICAD_BUILD_QA_TESTS=OFF -DKICAD_BUILD_I18N=OFF -DKICAD_USE_SENTRY=OFF \
      -DKICAD_INSTALL_DEMOS=OFF
ninja -C /home/user/kicad-oracle/build kicad-cli pcbnew_kiface
```

Build dependencies come from the Ubuntu 24.04 archive (wxWidgets 3.2, OCCT, ngspice, boost,
protobuf, nng, libgit2, ...); see `/home/user/kicad-oracle/logs/` for the exact configure and
apt logs of the build used here.
