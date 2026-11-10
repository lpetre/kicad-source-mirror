# Performance phase: baseline and hotspots

Goal of the phase (PLAN.md "Performance phase"): make the port faster **without changing a
single output byte**. This file holds the measuring instrument's description, the baseline
tables (one section per commit) and the ranked hotspot list from which the optimisation
tasks are handed out. Package PERF-1 built the instrument and took the first baseline; it
changed no library code.

> **Noise warning.** Every number below was taken on the shared development machine (4 vCPU
> Intel Xeon @ 2.10 GHz, 15 GB, Linux 6.18, rustc 1.97.0) while about ten other agents were
> compiling and testing (load average 4-9). Each measurement was repeated at least 7 times
> (large boards: see their section) and min and median are reported, but spreads of 20-50 %
> are common. **The baseline will be re-measured on a quiet machine before any optimisation
> is judged**; a change is only kept if it beats the noise on the same quiet machine
> (PLAN.md: and every differential oracle and golden test still passes bit for bit).

## 1. The instrument

| Tool | What it measures |
|---|---|
| `crates/kicad-bench` (`kicad-bench run --board F`) | per-stage wall-clock of the native port through the `kicad-session` calls kicad-rs makes; peak RSS per stage; allocation calls / bytes per stage (`--features count-alloc`) |
| `crates/kicad-bench/scripts/bench.sh` | the fixed board set (below), release build, TSV appended here under the commit hash (`--set small\|medium\|large\|all`, `--alloc`) |
| `crates/kicad-bench/scripts/ref_baseline.py` | end-to-end process wall-clock + peak RSS of the reference `kicad-cli` (modes: 4-thread pool, `MaximumThreads=1`, `taskset -c 0`) and, with `--tool rs`, of `kicad-rs` for the same commands, each run on a fresh copy of the board |
| `crates/kicad-bench/scripts/wasm_bench.mjs` | the same stages through the JS API of `web/pkg-node` (wasm-release profile) under node 22 |
| `crates/kicad-bench/scripts/summarize.py` | the markdown tables of section 2 from those TSVs |

Pipelines of `kicad-bench` (each starts from a fresh load, as every kicad-cli invocation does):

* `drc`: load, `pcb drc` (`run_drc`, report format, no parity), then a second, instrumented
  `RunTests` on the same session: its progress and log reporter messages split it into
  `drc2.prepare`, the cache generator phases `drc2.gen.*` and one interval per provider
  `drc2.provider.<name>` (intervals add up to `drc2.run_tests` within 3 %).
* `fab`: load, `pcb export gerbers` (all enabled layers, CLI defaults), `pcb export drill`
  (CLI defaults), `pcb upgrade --force` (the writer).
* `refill`: load, `FillAllZones`, `pcb drc` on the refilled board (`pcb drc --refill-zones`).
* `fill`: load and `FillAllZones` only (used where the DRC cannot run, see 4).

The load is `getBoard` split into `parse` (project file + board parser), one stage per
`BOARD_LOADER` step (`load.BuildConnectivity` is the connectivity build), `load.session_init`,
`load.font_warnings` and `load_total`. Every repetition after one warm-up is kept; the warm-up
itself is kept as `<stage> [cold]` (first call in a process: it pays the one-time
initialisations that every kicad-rs invocation pays). Peak RSS: before timing, kicad-bench
re-runs itself once per pipeline (fresh process) and resets `VmHWM` (`/proc/self/clear_refs`)
before each stage, so a stage's peak includes the board already in memory.

Fidelity of the instrument: the files produced by the bench stages were compared with the
files kicad-rs writes for the same board copies (fixed date `2026-01-01T00:00:00`, UUID seed
1): on issue10697 and complex_hierarchy the 27 / 21 Gerber files, the drill file, the re-saved
board and the DRC report are byte-identical. The bench therefore times the code kicad-rs runs.

Board set: small = `demos/ecc83/ecc83-pp`, `qa/data/pcbnew/issue10697`; medium =
`demos/complex_hierarchy`, `demos/interf_u`, `demos/stickhub/StickHub`,
`demos/kit-dev-coldfire-xilinx_5213`; large = `demos/vme-wren` (70 MB) and
`demos/jetson-agx-thor-baseboard` (85 MB), always one at a time.

Reproduce: `crates/kicad-bench/scripts/bench.sh --set small --alloc` (likewise `medium`,
`large`); `python3 crates/kicad-bench/scripts/ref_baseline.py BOARD...`;
`python3 crates/kicad-bench/scripts/ref_baseline.py --tool rs --modes default BOARD...`;
`node crates/kicad-bench/scripts/wasm_bench.mjs BOARD...`.

## Note for planners: the workload that matters most (added by the orchestrator)

The first real client of the port is a browser autorouter. Its inner loop, per routing
candidate, is: remove a few hundred tracks, add about a thousand segments and move some
vias (`board_api` `applyEdits`), rebuild connectivity, refill zones, update teardrops, run
DRC (often only clearance + connectivity: `quickDrc`), and at the end save. It runs in
WebAssembly, single-threaded, on small and medium boards (a few hundred pads), several
candidates per run. So besides the batch stages measured above, these are worth a
`kicad-bench` pipeline of their own and worth optimising: the cost of one edit batch incl.
the connectivity rebuild (`board_api` rebuilds connectivity in full after every batch where
KiCad's BOARD_COMMIT updates incrementally; kicad-connectivity has the incremental add /
remove paths), `quickDrc` with few providers (how much fixed cost does a DRC run carry: the
cache generator, r-tree builds, rule compilation on every call?), zone refill after a
routing change, `updateTeardrops`, and the same numbers in wasm (`wasm_bench.mjs`), where
the build profile currently optimises for size.

## LIVE-1: transactions with incremental connectivity (pipeline `live`)

`kicad-bench run --pipelines live` / `wasm_bench.mjs --pipelines live` (not in the default
sets): all routing of the board removed in ten commits and re-added from flat records in ten
commits. "before" = the same commits followed by a full `BuildConnectivity` (what
`applyEdits` cost before LIVE-1, stage `live.commit_full`), "after" = the incremental commit
(`live.commit`). Medians in ms (native: release, 3 reps + 1 warm-up, 60 commits; wasm:
wasm-release under node 22, same counts). Machine shared with other agents.

| board | items | commit before (native / wasm) | commit after (native / wasm) | checkpoint (native / wasm) | rollback (native / wasm) |
|---|---|---|---|---|---|
| ecc83-pp (small) | 59 | 0.61 / 1.25 | 0.06 / 0.18 | 0.21 / 0.37 | 0.33 / 0.64 |
| complex_hierarchy (medium) | 364 | 4.52 / 7.97 | 0.50 / 0.99 | 0.93 / 1.15 | 1.62 / 2.45 |
| interf_u (medium) | 815 | 6.65 / 10.85 | 1.15 / 1.98 | 1.04 / 1.13 | 2.30 / 3.25 |

Checkpoints are snapshots (`Board::clone` + `ConnectivityData::clone`); `rollback` clones the
checkpoint again so that it stays reusable (`Session::restore` consumes it without the copy).
A journal undo in the manner of `BOARD_COMMIT::Revert` was not built: it would have to put
re-added items back into their old arena slots (same handles) and rewind the UUID generator,
which the board model has no API for, while the snapshot is exact by construction and costs
about two incremental commits.

### The client's sessions (added by the orchestrator)

The autorouter drives KiCad as whole sessions on one board, each starting by RE-OPENING
the source board from its file. A candidate check: open, import tracks (remove a few
hundred, add about a thousand, move vias), fill all zones, full DRC (JSON), and when
something is unconnected a GND island query. A stitched check: the same with stitching
vias added before the fill. Finishing: the same with teardrops and a save, then possibly
dangling vias removed, fill, DRC and save again. A baseline: open, fill, DRC. One KiCad instance
(about 900 MB, threaded) serves all seeds in turn. For the port this means the numbers to
watch are: parse + load, an edit batch, a full refill, a full DRC, teardrops and save on
boards of a few hundred pads, in single-threaded wasm; and that checkpoint / rollback
(instead of re-opening), scoped refill and scoped DRC, and one small instance per worker
(checks in parallel) are where the port can beat that setup.

## LIVE-2: scoped check and affected-only refill (pipelines `edit`, `live2`)

`kicad-bench run --pipelines edit` now runs, per round and from the same full-refill state,
the LIVE-2 path (`edit.commit`, `edit.check_items` of the new tracks, `edit.refill_affected`
with the commit's damage) and the batch path (`edit.apply_edits`, `edit.quick_drc` with
clearance + connectivity, full `edit.zone_refill`), and checks that both save the same board;
then `edit.local_*`: one net's tracks re-added (2 to 40 tracks), the router's typical
candidate. `wasm_bench.mjs --pipelines live2` runs the local rounds through the JS API
(`commitBulk`, `checkItems`, `refillZonesAffected` against `refillZones`, saved boards
compared). Medians in ms, one rep, shared machine (load 4 to 8), local rounds:

| board | zones | refilled (3 nets) | check_items native / wasm | quick_drc native | refill affected native / wasm | full refill native / wasm |
|---|---|---|---|---|---|---|
| pic_programmer | 1 | 1, 1, 1 | 20.5 / - | 17.5 | 205.6 / - | 198.2 / - |
| complex_hierarchy | 168 | 38, 3, 5 | 64.1 / 158.5 | 56.6 | 209.6 / 347.8 | 230.2 / 359.8 |
| issue14549 | 44 | 41, 25, 32 | 31.3 / 117.4 | 25.7 | 157.6 / 247.1 | 158.7 / 188.3 |
| issue5990 | 12 | 10, 10, 10 | 37.5 / 106.8 | 14.9 | 243.8 / 361.9 | 245.5 / 352.2 |

Reading: the refill cost is the board-wide pours. Every edit inside a pour refills it (the
zone is refilled in whole; nothing smaller is provably identical), and on these boards the
pours are most of the fill time even when 3 of 168 zones are refilled. The affected refill also
pays the fixed part of `Fill()` (connectivity build, item indexes, both flashing passes over
every via and pad, the teardrop update) plus one extra connectivity build and cluster search
for the selection, so with nothing to skip it is a few ms slower than the full refill. It wins
when the expensive zones are not near the edit (several separate pours, edits between them):
an empty commit refills nothing (issue14549: 6.9 ms against 106 ms). `check_items` costs a run
of its eight providers (it is defined as their full all-track-errors run, filtered), i.e. a bit
more than `quick_drc` with two providers; the DRC keeps no state between runs except the
compiled rules, so every call pays the cache generator (connectivity build and r-trees).
