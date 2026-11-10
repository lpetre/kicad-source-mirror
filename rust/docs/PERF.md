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
(several hundred MB, threaded) serves all seeds in turn. For the port this means the numbers to
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

## Quiet baseline at 63a725d7

Taken by the perf gate package (2026-11-10, 08:57 to 09:49 UTC). This is the baseline that
optimisations are judged against; the busy-machine numbers of PERF-1 are superseded.

* **Code measured**: commit `63a725d7`, built from a clean `git archive` of that commit (the
  working tree had other agents' uncommitted connectivity edits at the time), release profile,
  rustc 1.97.0. `63a725d7` includes the fix of the matched-length deadlock found while taking
  this baseline (see "Found on the way" below); without it `pcb drc` on vme-wren never ends.
* **Machine state**: every block started with the 1-minute load below 1.0 (0.79 to 0.98; the
  driver waited for it before each block). During the large boards another agent's test run
  occasionally used one of the 4 cores (load 1.5 to 2.6, our single-threaded bench included);
  the reference's thread pool runs are the most exposed to that. Spreads: medians are within
  5 % of the minimum for most stages; exceptions are marked by a wide (min) in the tables
  (e.g. coldfire `zone_refill` 1472 (1245)).
* **Repetitions**: port (kicad-bench) small / medium 7 reps + 1 warm-up, large 3 + 1, plus one
  rep of the count-alloc build; reference (ref_baseline.py) small / medium 7 + 1, large 3 + 0,
  modes `default` (4-thread pool) and `threads1` (MaximumThreads=1). Board copies, fixed date
  and UUID seed as in section 1. Raw TSVs: `target/bench/quiet/` (`rs-*.tsv`, `ref-*.tsv`,
  `driver.log`); tables made by a scratch script from them.
* Output check: the bench's per-stage output hashes (stderr) are the same as in PERF-1's runs
  (e.g. jetson gerbers `bf129adf66ede036`, write `b1e3d92fe3c515a9`).

#### Port stage times (kicad-bench, release): median ms (min) [peak RSS MB]

| board | n | parse | load.BuildConnectivity | load_total | drc | zone_refill | drc_after_refill | gerbers | drill | write |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| ecc83-pp | 7 | 3.9 (3.5) [12.2] | 2.1 (1.9) [14.3] | 6.6 (6.0) [14.4] | 21.8 (18.6) [123.7] | 26.3 (25.0) [124.2] | 20.3 (19.1) [124.2] | 3.5 (3.4) [122.1] | 0.0 (0.0) [122.1] | 2.8 (2.7) [122.5] |
| issue10697 | 7 | 0.8 (0.7) [10.3] | 0.1 (0.1) [12.4] | 1.1 (1.0) [12.6] | 3.9 (3.6) [14.6] | 0.2 (0.2) [13.5] | 3.9 (3.6) [15.1] | 0.7 (0.6) [13.2] | 0.0 (0.0) [13.2] | 0.2 (0.2) [13.5] |
| complex_hierarchy | 7 | 19.1 (17.6) [16.7] | 20.9 (19.3) [19.7] | 41.1 (39.2) [19.8] | 167.6 (157.8) [133.2] | 230.7 (226.8) [134.1] | 177.6 (159.3) [133.9] | 16.8 (15.4) [129.1] | 0.1 (0.1) [129.1] | 16.3 (14.9) [131.2] |
| interf_u | 7 | 13.7 (12.1) [122.3] | 14.5 (13.4) [125.6] | 29.8 (27.0) [125.8] | 126.0 (109.0) [132.7] | 244.0 (232.6) [135.5] | 111.2 (110.4) [134.1] | 21.3 (20.0) [127.9] | 0.3 (0.2) [127.9] | 12.0 (11.3) [129.6] |
| StickHub | 7 | 25.7 (23.0) [17.8] | 20.8 (18.4) [21.0] | 48.8 (42.6) [21.2] | 222.7 (204.2) [134.0] | 235.4 (216.9) [134.4] | 224.6 (209.4) [135.3] | 22.3 (19.1) [130.0] | 0.1 (0.1) [130.0] | 20.2 (17.3) [132.6] |
| kit-dev-coldfire-xilinx_5213 | 7 | 52.9 (48.1) [25.8] | 86.7 (81.2) [34.4] | 140.8 (132.4) [34.7] | 400.6 (387.1) [158.6] | 1471.7 (1245.2) [165.9] | 434.0 (392.8) [167.4] | 55.4 (53.4) [146.9] | 0.4 (0.3) [146.9] | 52.9 (51.9) [154.1] |
| vme-wren | 3 | 1472.7 (1370.8) [505.6] | 2874.0 (2820.1) [570.7] | 4557.1 (4324.1) [574.4] | 19643.7 (18774.6) [756.8] | 41187.4 (39861.1) [1019.6] | 31108.3 (31042.3) [903.8] | 2331.1 (2304.9) [614.6] | 5.0 (4.4) [614.6] | 1176.1 (1111.8) [741.8] |
| jetson-agx-thor-baseboard | 3 | 2334.5 (1972.3) [727.7] | 1176.5 (1095.7) [732.4] | 3635.6 (3150.1) [742.8] | 16945.8 (16674.9) [941.8] | 24021.2 (23661.0) [965.9] | 18125.4 (16925.4) [996.6] | 3365.3 (3080.2) [819.2] | 3.6 (3.5) [819.2] | 1605.9 (1434.0) [968.8] |

#### Port allocations per stage (count-alloc build, one repetition): calls / MB

| board | parse | load.BuildConnectivity | load_total | drc | zone_refill | drc_after_refill | gerbers | drill | write |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| ecc83-pp | 10,432 / 2.7 | 1,629 / 1.1 | 15,661 / 4.2 | 479,107 / 123.0 | 22,158 / 26.1 | 62,272 / 15.9 | 37,997 / 2.5 | 254 / 0.0 | 31,599 / 1.2 |
| issue10697 | 6,512 / 0.3 | 428 / 0.1 | 9,236 / 0.5 | 38,246 / 4.9 | 1,144 / 0.2 | 38,234 / 4.9 | 6,169 / 0.5 | 295 / 0.0 | 1,226 / 0.1 |
| complex_hierarchy | 29,566 / 12.9 | 15,315 / 7.2 | 53,992 / 20.9 | 867,161 / 195.6 | 368,359 / 151.3 | 441,053 / 88.4 | 182,851 / 10.4 | 924 / 0.1 | 175,569 / 7.4 |
| interf_u | 22,800 / 6.2 | 30,089 / 8.7 | 65,899 / 15.8 | 373,676 / 78.7 | 296,665 / 152.6 | 372,430 / 78.1 | 193,893 / 10.8 | 2,107 / 0.2 | 142,426 / 5.1 |
| StickHub | 43,837 / 13.6 | 53,652 / 10.1 | 102,738 / 24.2 | 1,046,643 / 211.1 | 305,309 / 162.5 | 637,071 / 107.7 | 169,480 / 11.4 | 526 / 0.0 | 197,983 / 8.5 |
| kit-dev-coldfire-xilinx_5213 | 56,815 / 28.2 | 85,911 / 39.2 | 166,712 / 69.0 | 1,664,179 / 379.5 | 1,369,044 / 783.2 | 1,244,028 / 273.8 | 556,209 / 30.7 | 2,763 / 0.3 | 660,257 / 22.2 |
| vme-wren | 2,391,248 / 739.4 | 625,986 / 580.6 | 4,879,286 / 1529.3 | 33,842,388 / 6344.2 | 49,214,899 / 15534.7 | 34,495,719 / 7100.8 | 45,970,625 / 1635.5 | 24,733 / 2.5 | 12,334,222 / 525.2 |
| jetson-agx-thor-baseboard | 2,167,271 / 1889.9 | 616,972 / 355.0 | 2,916,331 / 2264.5 | 30,915,102 / 5795.7 | 17,829,737 / 9629.4 | 30,942,374 / 5887.2 | 12,430,846 / 798.5 | 16,877 / 1.3 | 14,594,253 / 634.7 |

#### DRC split (second instrumented RunTests): top phases, median ms

| board | drc2.run_tests | top phases |
|---|---:|---|
| ecc83-pp | 21.6 | provider.copper width 4.3, provider.sliver checker 3.9, provider.silk_clearance 3.1, provider.solder_mask_issues 2.9, gen.Tessellating copper zones 1.8, provider.edge_clearance 1.2, provider.zone connections 0.8, provider.footprint checks 0.8 |
| issue10697 | 2.96 | provider.copper width 1.2, provider.sliver checker 0.8, provider.clearance 0.2, gen.Tessellating copper zones 0.1, gen.Gathering copper items 0.1, provider.hole_size 0.1, provider.diameter 0.1, provider.annular_width 0.1 |
| complex_hierarchy | 162.79 | provider.clearance 47.3, provider.sliver checker 34.6, provider.copper width 25.3, provider.silk_clearance 16.6, provider.solder_mask_issues 10.1, gen.Tessellating copper zones 8.4, provider.zone connections 5.9, provider.edge_clearance 4.8 |
| interf_u | 142.44 | provider.sliver checker 52.3, provider.solder_mask_issues 22.5, gen.Tessellating copper zones 12.3, provider.zone connections 12.1, provider.clearance 11.4, provider.silk_clearance 9.6, provider.edge_clearance 4.8, provider.footprint checks 4.1 |
| StickHub | 214.8 | provider.clearance 51.5, provider.sliver checker 36.2, provider.copper width 33.3, provider.edge_clearance 22.1, gen.Tessellating copper zones 20.9, provider.solder_mask_issues 17.2, provider.zone connections 17.1, provider.diff_pair_coupling 3.5 |
| kit-dev-coldfire-xilinx_5213 | 393.28 | provider.copper width 137.6, provider.clearance 56.9, gen.Tessellating copper zones 46.6, provider.silk_clearance 40.5, provider.solder_mask_issues 39.0, provider.zone connections 36.3, provider.edge_clearance 6.3, provider.footprint checks 4.3 |
| vme-wren | 17558.62 | provider.sliver checker 6252.0, provider.copper width 4808.0, provider.clearance 2413.2, provider.edge_clearance 1709.0, gen.Tessellating copper zones 586.4, provider.solder_mask_issues 455.7, provider.zone connections 399.4, provider.length 245.0 |
| jetson-agx-thor-baseboard | 11095.73 | provider.sliver checker 2509.4, provider.copper width 1964.7, provider.clearance 1818.9, provider.disallow 1115.5, provider.silk_clearance 728.3, provider.edge_clearance 559.1, gen.Tessellating copper zones 512.6, provider.solder_mask_issues 477.9 |

#### Reference kicad-cli, end-to-end process wall-clock: median ms (min) [peak RSS MB]

| board | mode | startup | upgrade | drc | drc_refill | gerbers | drill |
|---|---|---:|---:|---:|---:|---:|---:|
| ecc83-pp | default | 104 (96) [59.3] | 238 (215) [113.6] | 372 (341) [224.4] | 440 (419) [225.7] | 383 (362) [234.8] | 214 (207) [113.3] |
| ecc83-pp | threads1 | 104 (96) [59.2] | 243 (213) [113.3] | 353 (340) [223.3] | 436 (423) [223.8] | 380 (356) [234.9] | 232 (215) [113.4] |
| issue10697 | default | 131 (97) [59.1] | 212 (204) [111.9] | 222 (207) [115.2] | 215 (205) [115.6] | 327 (284) [133.0] | 209 (200) [112.1] |
| issue10697 | threads1 | 96 (93) [59.2] | 207 (194) [111.9] | 226 (205) [115.0] | 209 (207) [115.4] | 339 (282) [132.9] | 253 (214) [111.9] |
| complex_hierarchy | default | 118 (106) [59.4] | 283 (263) [120.2] | 630 (582) [241.0] | 896 (849) [247.0] | 447 (429) [240.8] | 261 (250) [119.2] |
| complex_hierarchy | threads1 | 103 (94) [59.5] | 285 (273) [119.3] | 703 (645) [236.4] | 855 (802) [239.6] | 453 (408) [240.2] | 266 (257) [118.4] |
| interf_u | default | 109 (102) [59.2] | 366 (340) [224.2] | 513 (474) [239.7] | 676 (635) [248.7] | 453 (399) [240.8] | 345 (312) [223.9] |
| interf_u | threads1 | 107 (95) [59.4] | 362 (339) [223.7] | 498 (486) [236.8] | 681 (640) [238.7] | 433 (391) [240.2] | 355 (327) [223.7] |
| StickHub | default | 102 (95) [59.5] | 275 (260) [120.6] | 576 (509) [237.7] | 788 (752) [241.1] | 467 (419) [242.2] | 310 (249) [120.4] |
| StickHub | threads1 | 96 (89) [59.4] | 302 (287) [120.3] | 602 (585) [234.6] | 1017 (918) [236.3] | 485 (461) [241.6] | 302 (275) [120.1] |
| kit-dev-coldfire-xilinx_5213 | default | 99 (90) [59.4] | 373 (355) [139.0] | 850 (796) [274.7] | 1535 (1520) [307.0] | 550 (530) [257.6] | 313 (307) [135.8] |
| kit-dev-coldfire-xilinx_5213 | threads1 | 98 (89) [59.4] | 446 (416) [132.9] | 980 (929) [267.2] | 2227 (2178) [282.0] | 576 (561) [254.2] | 359 (342) [132.3] |
| vme-wren | default | 112 (94) [59.4] | 4438 (4365) [909.3] | 13349 (12973) [1085.4] | 41614 (39639) [1409.2] | 5183 (5166) [733.9] | 3455 (3245) [691.7] |
| vme-wren | threads1 | 110 (92) [59.3] | 6470 (6344) [856.3] | 23917 (23716) [934.3] | 79266 (78495) [1241.7] | 7531 (7226) [681.5] | 5749 (5540) [638.2] |
| jetson-agx-thor-baseboard | default | 104 (100) [59.2] | 5761 (5164) [990.6] | 12802 (12743) [1146.4] | 25612 (24872) [1228.5] | 8941 (8897) [816.5] | 3294 (3029) [778.4] |
| jetson-agx-thor-baseboard | threads1 | 96 (93) [59.3] | 6465 (6142) [963.7] | 16904 (16891) [1075.2] | 43821 (41919) [1125.3] | 10071 (9126) [788.8] | 4059 (4009) [751.7] |

#### Port / reference ratio per operation (median; < 1 = port faster)

Port = `load_total` + the operation's stages of kicad-bench (in-process, warm); reference = the kicad-cli process median minus its `startup` (`kicad-cli version`) median of the same mode. `1t` = reference with MaximumThreads=1, `pool` = default 4-thread pool.

| board | upgrade 1t | upgrade pool | drc 1t | drc pool | drc_refill 1t | drc_refill pool | gerbers 1t | gerbers pool | drill 1t | drill pool |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| ecc83-pp | 0.07 | 0.07 | 0.11 | 0.11 | 0.16 | 0.16 | 0.04 | 0.04 | 0.05 | 0.06 |
| issue10697 | 0.01 | 0.02 | 0.04 | 0.06 | 0.05 | 0.06 | 0.01 | 0.01 | 0.01 | 0.01 |
| complex_hierarchy | 0.32 | 0.35 | 0.35 | 0.41 | 0.60 | 0.58 | 0.17 | 0.18 | 0.25 | 0.29 |
| interf_u | 0.16 | 0.16 | 0.40 | 0.38 | 0.67 | 0.68 | 0.16 | 0.15 | 0.12 | 0.13 |
| StickHub | 0.34 | 0.40 | 0.54 | 0.57 | 0.55 | 0.74 | 0.18 | 0.19 | 0.24 | 0.24 |
| kit-dev-coldfire-xilinx_5213 | 0.56 | 0.71 | 0.61 | 0.72 | 0.96 | 1.43 | 0.41 | 0.44 | 0.54 | 0.66 |
| vme-wren | 0.90 | 1.33 | 1.02 | 1.83 | 0.97 | 1.85 | 0.93 | 1.36 | 0.81 | 1.36 |
| jetson-agx-thor-baseboard | 0.82 | 0.93 | 1.22 | 1.62 | 1.05 | 1.79 | 0.70 | 0.79 | 0.92 | 1.14 |

#### Port ms behind each ratio (load_total + stages, median)

| board | upgrade | drc | drc_refill | gerbers | drill |
|---|---:|---:|---:|---:|---:|
| ecc83-pp | 9.4 | 28.4 | 53.3 | 10.2 | 6.7 |
| issue10697 | 1.3 | 5.1 | 5.2 | 1.8 | 1.2 |
| complex_hierarchy | 57.4 | 208.7 | 449.3 | 58.0 | 41.2 |
| interf_u | 41.8 | 155.8 | 385.0 | 51.2 | 30.1 |
| StickHub | 69.0 | 271.5 | 508.8 | 71.1 | 48.9 |
| kit-dev-coldfire-xilinx_5213 | 193.7 | 541.4 | 2046.5 | 196.2 | 141.2 |
| vme-wren | 5733.3 | 24200.8 | 76852.8 | 6888.2 | 4562.1 |
| jetson-agx-thor-baseboard | 5241.4 | 20581.3 | 45782.1 | 7000.9 | 3639.1 |

Reading: on the small and medium boards the port beats the reference process in every
operation (ratios 0.01 to 0.72; the reference pays about 100 ms of start-up, which the ratio
already subtracts, plus its own load per command), except refill + DRC on coldfire against
the thread pool (1.43; 0.96 against one thread). On the large boards the port is slower than
the reference's thread pool on vme-wren in every operation (1.3 to 1.9x) and on jetson in DRC
(1.6x), refill + DRC (1.8x) and drill (1.1x); against the single-threaded reference it is
0.7 to 1.2x. Zone refill and the DRC dominate everywhere; on the large boards parse + load
(3.6 to 4.6 s) comes next.

### Ranked hotspot list (callgrind, 63a725d7)

Instruction counts (`Ir`, valgrind 3.22 callgrind, no `perf` in the container) of
`kicad-bench run --reps 1 --warmup 0 --no-mem --pipelines fill|drc|fab` with the release
binary of 63a725d7 on `kit-dev-coldfire-xilinx_5213` (medium) and
`jetson-agx-thor-baseboard` (large). Percentages are inclusive and relative to the whole
pipeline process (`fill` = load + `FillAllZones`; `drc` = load + the `pcb drc` job + the
instrumented second `RunTests`; `fab` = load + Gerbers + drill + writer). Profiles:
`target/bench/prof-quiet/cg-<board>-<pipeline>.out`.

Ranked by the time they carry in the quiet tables and by how plausible a byte-identical win
is (caching, fewer copies / allocations, cheaper lookups; no algorithm changes):

| rank | area | hotspot | coldfire | jetson | what to look at |
|---|---|---|---|---|---|
| 1 | refill | `ShapePolySet::inflate` -> `execute_offset` -> `ClipperOffset::union` / `execute` (Clipper2 offsets of the filler: clearance knock-outs, min-width prune, `post_knockout_min_width_prune`, `refill_zone_from_cache`) | 33 % of fill | 37 % (prune 31 %, cache refill 23 %) | Clipper2's `execute_internal` is 34 to 36 % *self* cost (inlined scanbeam loop, same algorithm as the reference's Clipper2). Wins only around it: the same polygon offset by the same amount several times per zone / layer, conversions `ShapePolySet` <-> Clipper paths (`input::add_paths`, `memcpy` 2 %), engine reuse instead of re-allocation. Results must stay vertex-identical (zone corpus). |
| 2 | refill, load | zone fill triangulation: `ZoneFill::cache_triangulation` -> `tesselate_chain` -> `earcut_list` / `VertexSet::is_ear` | 16 % of fill; 44 % of the load (521 M of 1174 M Ir) | 14 % of fill; 34 % of the load (8.5 G of 25.2 G) | Runs at load (`BuildConnectivity` -> `cache_board_triangulation`) and again after the refill. `is_ear` is 8 to 10 % *self* in every profile: a tight loop (bounds checks, vertex layout). Check whether triangulations are rebuilt for fills that did not change. |
| 3 | DRC | **done in round 1** (`R1-drc-marker-uuid-scan`, jetson `drc` -5.2 s): `Board::resolve_item` misses falling back to the full linear scan (`resolve_item_scan`), called from `run_drc_job` | 1.6 % | 226 calls = 10.8 G Ir, 13 % of the jetson DRC job | Every call is a miss (48 M Ir each): find which UUIDs the job resolves (markers / exclusions) and cache the scan result as the C++ does (`BOARD::ResolveItem` caches scan hits) or index those items. Pure lookup, output-neutral. |
| 4 | DRC | rule evaluation `DrcEngine::eval_rules_reporting` -> `EvalState::process_all` / `check_condition` -> `EngineCondition::evaluate_for` -> ucode `run` | 3.5 % | 16 % of the drc pipeline | Called per item x constraint type (matched length asks 6 constraint types for every track / via / pad). Look for repeated evaluation of identical (rule, item, layer) queries inside one run and for allocations in the evaluator. |
| 5 | DRC | copper width (`connection_width`) and sliver checker: `ShapePolySet::simplify` / `fracture` / `split_collinear_outlines` (`SegmentIndex::new` + its r-tree searches), `PolygonTest::kink_candidate` | width 27 % of drc pipeline | sliver 18 %, width 16 %; `split_collinear_outlines` searches 28 % | `SegmentIndex::new` and `PackedRTreeBuilder::build` rebuilt per polygon; `simplify` of the same layer polygons by both providers. Round 1 (`R1-split-collinear-fastpath`) took 10 to 30 % off both providers; the shared `simplify` and the kink search remain. |
| 6 | DRC | copper clearance `DrcRtree::query_colliding_prepared_copper` (r-tree search + `collide_shapes`), `ShapeLineChain::point_inside` (5.5 % self, jetson) | 12 % | 31 % (search) | Same visit order is pinned; candidates: per-query allocations, repeated shape construction for the same item, `Board::get` (4.5 % self on jetson). |
| 7 | DRC fixed cost | every `RunTests` rebuilds connectivity (`ConnectivityData::build`, 12 % of coldfire drc pipeline, with triangulation) and the DRC r-trees (`DrcRtree::insert_on` 5.5 %); `disallow` 8 % on jetson | yes | yes | Matters most for the router loop (PERF.md note above): one DRC per candidate pays it again. |
| 8 | connectivity in the filler | `search_clusters_ex`, `fill_isolated_islands_map`, `cn_visit`, `check_zone_item_connection`; `connect_nearby_polys` (jetson 6.8 %) | 11 % of fill | 8 % of fill | Island removal re-runs the cluster search over the whole board. |
| 9 | writer (upgrade) | `format_internal_units_point` -> `cformat::format_g` -> `core::fmt` float formatting (`grisu::format_exact_opt`, `float_to_exponential_common_exact`) and `format::format_inner` (allocating `format!`) | 16 % of fab pipeline; writer 24 % | 15 %; writer 25 % | `%g` emulation goes through std formatting into a fresh `String` per number; write into one buffer, avoid the intermediate allocations (the digits must stay exactly the glibc `%g` digits). `prettify_bytes` 3.5 to 4.3 % self. |
| 10 | parse | `PcbParser::parse_board` (`parse_footprint`, `parse_pcb_shape`, `parse_zone` / `parse_outline_points`), `DsnLexer::next_tok_i32` / `find_token` / `lookup_keyword`, `file_uuids` iterator | 18 % of fab pipeline | 24 % of fab pipeline (15.1 G) | Lexer and keyword lookup per token; `parse_double_expected` 4 %. |
| 11 | Gerber | `plot_standard_layer` per layer: `plot_footprint_graphic_items` (jetson 13 %) iterating `Board::footprint_drawings` (flatten iterator 4.4 % incl.) for every layer, `Lset::from_layers` / `BaseSet` `&=` (1.6 + 1.7 % self), text via stroke glyphs (`plot_footprint_text_items` 8 %), `GerberPlotter::end_plot` 2.4 %; one-time stroke font load 182 M Ir (cold start) and `Board::compute_bounding_box` with text boxes (8 % of coldfire fab) | 16 % | 27 % | Per-layer re-scans of all footprint items and layer-set construction per item. |
| 12 | allocations | refill 1.37 M calls / 783 MB (coldfire), 17.8 M / 9.6 GB (jetson), 49 M / 15.5 GB (vme); DRC 1.66 M / 380 MB (coldfire), 31 M / 5.8 GB (jetson) | | | `malloc` / `free` / `realloc` / `finish_grow` 5 to 8 % self in the DRC profiles. |

Allocation counts are from the count-alloc table above. Stage times for each area: quiet
port table (e.g. jetson: refill 24.0 s, DRC job 16.9 s, parse 2.3 s, connectivity build
1.2 s, Gerbers 3.4 s, writer 1.6 s).

### Found on the way

* **Deadlock in the matched-length provider** (fixed, committed in 63a725d7): `run` held the
  from-to cache's `MutexGuard` for the whole rule-evaluation loop, and a `fromTo()` rule
  condition locks the same mutex in `ConnectivityData::is_on_from_to_path`; std's mutex is
  not re-entrant, so `pcb drc` hung on every board with a `fromTo` length rule (vme-wren; the
  QA boards `via_stack_*` only escape because their `A.NetClass == ...` short-circuits). Fix:
  the lock is taken only around each `query_from_to_path` (`crates/kicad-drc/src/providers/
  matched_length.rs`; no computation changes, as in C++ where `ftCache` is a plain pointer).
  Regression test: `kicad-drc/tests/drc31_matched_length.rs`
  `from_to_rule_condition_does_not_deadlock` (fails with a 20 s timeout before the fix).
* **bench.sh `--alloc`** wrote through the hard link `target/release/kicad-bench` into
  cargo's `deps/` binary, so the second `--alloc` run used a timing binary under the alloc
  name and recorded no allocations. Fixed (`cp --remove-destination`).

### The gate

`rust/scripts/perf-gate.sh full | changed <crate>... | list [--only <id>] [--bless]`: 30
entries (every crate's default test suite in the dev profile, `kicad-pcb` with `--features
dump`, plus the ignored corpus replays: sexpr big corpus, zint driver, pcb corpus_large /
vcu118 / io_corpus scoreboard / text and dimension blocks / d3 legacy, connectivity cn04
corpus, zone corpus (strict) and teardrop corpus, session upgrade corpus and zf06
refill+save, cli e2e all). Entries reading caches (`target/zone-corpus`,
`target/io-corpus`, the reference kicad-cli, the zint driver) fail when the cache is
missing. Scoreboard replays that do not assert identity are compared with committed status
snapshots (`scripts/perf-gate/expected/`). First run at 03b6a6fd (63a725d7 + LIVE work):
30 / 30 PASS in 28 min (`target/perf/gate-20261110-094933.log`).

## Round 1 (judged 2026-11-10, round base 85314f75)

Kept: all three changes. Reverted: none.

| id | what | files | gain (min ms, A/B below) |
|---|---|---|---|
| `R1-wasm-opt3` | `[profile.wasm-release]` opt-level `"s"` -> `3`; optional `wasm-opt -Os` -> `-O3` | `Cargo.toml`, `scripts/build-wasm.sh` | wasm, medium boards: `load_total` -21 to -26 %, `drc` -19 to -23 %, `zone_refill` -11 to -19 %, `drc_after_refill` -19 to -25 %, `gerbers` -15 to -25 %, `write` -3 to -19 %; router loop: `live.commit` -17 to -27 %, `live2.check_items` -16 to -23 %, `live2.refill_affected` -13 %. Package 8.71 -> 10.65 MB. Native unaffected. |
| `R1-drc-marker-uuid-scan` | `UuidPresence` answers `resolve_item(..).is_some()` for the fresh marker UUIDs of `run_drc_job` without the per-marker linear board scan; net-tie group map memoised per footprint for one copper-clearance run | `kicad-pcb/src/board/index.rs`, `kicad-drc/src/run_flow.rs`, `kicad-drc/src/providers/copper_clearance.rs` | jetson `drc` 15915 -> 10729 (-33 %), `drc_after_refill` 15500 -> 10742; vme-wren `drc` -12 %; coldfire `drc2.provider.clearance` 55.6 -> 45.2 (-19 %; spread 0.1), `drc` -7 %. Neutral on the other medium boards (few markers, no net ties). |
| `R1-split-collinear-fastpath` | `split_collinear_outlines`: a box-overlap pre-check of the necessary conditions skips building the segment r-tree when no pair can split; lazily built extent filter in front of `point_inside` | `kicad-geom/src/geometry/shape_poly_set.rs` | `drc2.provider.copper width` -13 to -19 % (medium), `sliver checker` -10 to -14 % (medium), -18 % jetson, -30 % vme-wren; `drc` -3 to -7 % on medium, vme-wren -17 %; `zone_refill` within noise. |

Verification (all output byte-identical):

* Gate: `perf-gate.sh changed kicad-geom kicad-pcb kicad-drc` at 12981c84 (23 entries incl.
  pcb uuid_presence test, geom split_collinear tests, zone corpus strict, zf06, drc oracles,
  cli e2e all in the dev profile, i.e. with overflow checks, incl. `issue25089/
  odb_circle_edge_cases`): 23 / 23 PASS, `target/perf/gate-20261110-130025.log`. The round's
  files are unchanged in a37201e4.
* Native A/B: four release `kicad-bench` binaries from `git archive` of 12981c84 with the
  round's files at base / one change / the other / all (everything else identical, incl.
  `connection_width.rs`). Per-stage output hashes (drc, drc_after_refill, gerbers, drill,
  write) equal between base and head on the 6 bench boards, vme-wren, jetson (e.g. jetson
  drc `64f7cd1b1a143b7d`, drc_after_refill `24fdd0c1d63e345e`, write `b1e3d92fe3c515a9`) and
  the net-tie / edge-case boards `net_tie_hole_clearance`, `issue14008`, `issue14412`,
  `issue19956`, `issue24974`, `test_copper_graphics`, `issue25089/odb_circle_edge_cases`
  (their drc reports are also identical to the reference kicad-cli in e2e_cli).
* wasm: three nodejs packages from rustc 1.97.0 - base code at opt-level `s`, round code at
  `s`, round code at 3 - hashed through the JS API (uuidSeed 1, fixed date): `save()`,
  `runDrc({})` report and JSON, the save after it, a track committed after `runDrc` (its
  UUID shows the generator advanced identically), `exportGerbers`, `exportDrill` excellon and
  gerber, `refillZones()` + save + both DRC reports, the live (incremental and full) and live2
  (affected and full refill, checkItems) saved boards: 128 hashes equal across all three
  packages on the 6 bench boards; vme-wren alone, `s` vs 3: 22 hashes equal, no
  `WebAssembly.RuntimeError`. `build-wasm.sh --test` on the O3 package: smoke.mjs 58 checks
  (wasm == native reference), browser.mjs OK; `web/test/router_loop.mjs` 20 checks.
  wasm-opt is not installed here: the `-O3` branch did not run and no wasm-opt'ed package was
  compared; whoever ships one must repeat the hash comparison and record the binaryen version.

Measurement: the machine never got quiet (other agents' tests and builds; waited 20 min, the
1-minute load stayed 2.2 to 6.0 during the runs, logged per run in
`target/bench/round1/driver.log`). To beat that, every A/B is interleaved: small / medium
7 reps + 1 warm-up per binary, 2 alternating rounds (fab re-check: 15 reps, 3 rounds);
jetson / vme-wren `drc` 2 reps + 1, `refill` 1 rep + 1 (single samples: treat +-10 % as
noise there). Values are minima. Spread (difference between the two rounds' minima of the same
binary) on medium boards: 1 to 3 % for the DRC providers and `drc`, up to 10 % for
`zone_refill` and the writer. A fab-only re-run (complex_hierarchy, interf_u, StickHub)
shows `load_total`, `gerbers` and `write` within 0.5 ms base vs head. Raw TSVs:
`target/bench/round1/` (`native-sm.tsv`, `native-lg.tsv`, `native-fab.tsv`, `wasm-def.tsv`,
`wasm-live.tsv`).

#### Native stage times after round 1: quiet baseline 63a725d7 / round base -> round head, min ms

| board | load_total | drc | zone_refill | drc_after_refill | gerbers | write |
|---|---:|---:|---:|---:|---:|---:|
| ecc83-pp | 6.0 / 6.1 -> 6.2 | 18.6 / 19.8 -> 18.0 (-9 %) | 25.0 / 26.1 -> 24.6 | 19.1 / 18.6 -> 18.1 | 3.4 / 3.3 -> 3.5 | 2.7 / 2.7 -> 2.6 |
| issue10697 | 1.0 / 0.9 -> 1.0 | 3.6 / 3.5 -> 3.0 (-14 %) | 0.2 / 0.2 -> 0.2 | 3.6 / 3.6 -> 3.2 (-12 %) | 0.6 / 0.6 -> 0.7 | 0.2 / 0.2 -> 0.2 |
| complex_hierarchy | 39.2 / 38.6 -> 38.5 | 157.8 / 156.4 -> 154.0 (-2 %) | 226.8 / 224.7 -> 212.5 | 159.3 / 158.5 -> 150.9 (-5 %) | 15.4 / 14.1 -> 14.2 | 14.9 / 14.1 -> 14.0 |
| interf_u | 27.0 / 27.5 -> 26.8 | 109.0 / 106.1 -> 104.5 (-2 %) | 232.6 / 232.9 -> 237.2 | 110.4 / 107.9 -> 100.5 (-7 %) | 20.0 / 17.4 -> 17.2 | 11.3 / 10.8 -> 10.8 |
| StickHub | 42.6 / 42.2 -> 42.2 | 204.2 / 200.1 -> 183.4 (-8 %) | 216.9 / 222.1 -> 212.9 | 209.4 / 205.5 -> 191.8 (-7 %) | 19.1 / 17.9 -> 17.4 | 17.3 / 17.6 -> 17.5 |
| kit-dev-coldfire-xilinx_5213 | 132.4 / 133.1 -> 133.1 | 387.1 / 388.4 -> 354.2 (-9 %) | 1245.2 / 1241.1 -> 1194.5 | 392.8 / 398.7 -> 352.6 (-12 %) | 53.4 / 50.5 -> 50.5 | 51.9 / 51.8 -> 50.9 |
| vme-wren | 4324 / 4300 -> 4331 | 18775 / 19868 -> 15185 (-24 %) | 39861 / 41221 -> 38784 | 31042 / 31831 -> 25542 (-20 %) | - | - |
| jetson-agx-thor-baseboard | 3150 / 3384 -> 3214 | 16675 / 15915 -> 10152 (-36 %) | 23661 / 23074 -> 23028 | 16925 / 15500 -> 11050 (-29 %) | - | - |

(gerbers / write of the medium boards from the 15-rep fab re-run.)

#### Remaining gap to the reference: port / reference, round base -> round head

Port = min of `load_total` + stages (kicad-bench); reference = the kicad-cli medians of the
quiet baseline minus start-up, as in the ratio table above.

| board | drc pool | drc 1t | drc_refill pool | drc_refill 1t |
|---|---:|---:|---:|---:|
| ecc83-pp | 0.10 -> 0.09 | 0.10 -> 0.10 | 0.15 -> 0.15 | 0.15 -> 0.15 |
| issue10697 | 0.05 -> 0.04 | 0.03 -> 0.03 | 0.06 -> 0.05 | 0.04 -> 0.04 |
| complex_hierarchy | 0.38 -> 0.38 | 0.33 -> 0.32 | 0.54 -> 0.52 | 0.56 -> 0.53 |
| interf_u | 0.33 -> 0.32 | 0.34 -> 0.34 | 0.65 -> 0.64 | 0.64 -> 0.64 |
| StickHub | 0.51 -> 0.48 | 0.48 -> 0.45 | 0.68 -> 0.65 | 0.51 -> 0.49 |
| kit-dev-coldfire-xilinx_5213 | 0.69 -> 0.65 | 0.59 -> 0.55 | 1.23 -> 1.17 | 0.83 -> 0.79 |
| vme-wren | 1.83 -> 1.47 | 1.02 -> 0.82 | 1.86 -> 1.65 | 0.98 -> 0.87 |
| jetson-agx-thor-baseboard | 1.52 -> 1.05 | 1.15 -> 0.80 | 1.64 -> 1.46 | 0.96 -> 0.85 |

The port now beats the single-threaded reference everywhere and the thread pool everywhere
except refill + DRC on coldfire (1.17) and the large boards (DRC 1.05 / 1.47, refill + DRC
1.46 / 1.65), where zone refill is the gap (hotspots 1, 2).

#### wasm after round 1 (wasm_bench.mjs, node 22, 4 reps + 1, 2 alternating rounds): opt-level s -> 3, min ms

| board | load_total | drc | zone_refill | drc_after_refill | gerbers | write |
|---|---:|---:|---:|---:|---:|---:|
| ecc83-pp | 9.9 -> 7.9 | 28.9 -> 29.3 | 34.4 -> 32.0 | 28.4 -> 28.1 | 8.6 -> 7.6 | 5.5 -> 5.7 |
| issue10697 | 1.5 -> 1.2 | 6.0 -> 4.3 | 0.4 -> 0.3 | 6.3 -> 4.5 | 1.5 -> 1.1 | 0.3 -> 0.3 |
| complex_hierarchy | 59.3 -> 46.9 | 238.5 -> 192.3 | 294.7 -> 254.3 | 234.2 -> 184.3 | 32.7 -> 26.3 | 27.7 -> 25.1 |
| interf_u | 44.7 -> 32.9 | 167.7 -> 129.5 | 326.6 -> 264.0 | 157.8 -> 127.3 | 44.4 -> 33.4 | 21.6 -> 18.7 |
| StickHub | 72.1 -> 55.4 | 317.5 -> 255.2 | 308.9 -> 257.4 | 324.1 -> 255.0 | 37.5 -> 31.9 | 33.3 -> 32.4 |
| kit-dev-coldfire-xilinx_5213 | 210.2 -> 165.6 | 570.4 -> 449.6 | 1651.2 -> 1462.5 | 579.4 -> 433.7 | 105.2 -> 83.3 | 103.6 -> 84.4 |

Router loop (complex_hierarchy / interf_u): `live.commit` 0.62 -> 0.45 / 1.09 -> 0.90,
`live.commit_full` 4.99 -> 3.96 / 6.30 -> 5.63, `live2.check_items` 103.7 -> 86.7 / 45.0 ->
34.8, `live2.refill_affected` 262.6 -> 228.1 / 315.7 -> 275.2, `live2.zone_refill` 291.1 ->
249.3 / 308.4 -> 267.1.

Hotspot list: rank 3 is done; rank 5 is smaller; the order of the rest is unchanged (not
re-profiled with callgrind this round).
