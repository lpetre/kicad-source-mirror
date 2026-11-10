# kicad-bench: port status

Not a port of a C++ file: the benchmark harness of the performance phase (package PERF-1).
It changes no library code and drives the port through the same `kicad-session` calls as
`kicad-rs` (`host::get_board`, `run_drc`, `run_export_gerbers`, `run_export_drill`,
`run_pcb_upgrade`, `Session::refill_zones`). Results and the hotspot list: `docs/PERF.md`.

## PERF-1 (harness and baseline)

| Part | Location | State |
|---|---|---|
| per-stage timer (parse, each `BOARD_LOADER` step through `load_guarded`, session init) | `src/main.rs` `load` | done |
| `pcb drc` stage + instrumented second `RunTests` (cache generator phases, every provider) | `src/main.rs` `run_pipeline`, `drc_phases` | done |
| Gerber (all enabled layers, CLI defaults), drill (CLI defaults), writer (`upgrade --force`) | `src/main.rs` `run_pipeline` "fab" | done |
| zone refill (`FillAllZones`) + DRC on the refilled board | `src/main.rs` `run_pipeline` "refill" | done |
| peak RSS per stage (fresh child process per pipeline, `VmHWM` reset by `clear_refs`) | `src/main.rs` `cmd_mem`, `child_peaks` | done |
| allocation calls / bytes per stage (`count-alloc` feature, `stats_alloc`) | `src/main.rs` | done |
| cold (first repetition of a process) samples, `<stage> [cold]` | `src/main.rs` `cmd_run` | done |
| board set runner, TSV appended to `docs/PERF.md` | `scripts/bench.sh` | done |
| reference kicad-cli and kicad-rs end-to-end process times | `scripts/ref_baseline.py` | done |
| wasm stages under node (`web/pkg-node`) | `scripts/wasm_bench.mjs` | done |

## Verification

The outputs of the bench stages were compared byte for byte with `kicad-rs` run on the same
copies with the same fixed date / UUID seed (`KICAD_BENCH_DUMP=<dir>` writes them): issue10697
and complex_hierarchy, Gerber (27 / 21 files), drill, re-saved board and DRC report identical
(file names aside). The bench therefore measures the code paths kicad-rs runs.

## Deviations

- The DRC phase split (`drc2.*`) comes from a second `RunTests` on the same session right after
  the `pcb drc` job run (after `RecordDRCExclusions` / `DeleteMARKERs`, as the job does), with
  a progress and a log reporter attached; `drc2.run_tests` is its total, `drc` the job stage.
  Progress phases inside a provider are attributed to that provider.
- `parse` includes reading the project file (`load_project`), which precedes the parser.
- The `fab` pipeline runs Gerber, drill and the writer on one session; kicad-cli loads the
  board once per command.

## Known gaps

- No `perf` in the container: profiles are valgrind/callgrind instruction counts (`Ir`), not
  time, and include no cache-miss data.
- wasm: the node script measures wall-clock per JS call only (no per-stage RSS, no load-step
  split; the API has no hook for it).
