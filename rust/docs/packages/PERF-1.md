# PERF-1: Benchmark harness and baseline (performance phase, step 1 and 2)

Crate: kicad-bench (NEW crate rust/crates/kicad-bench: create a valid Cargo.toml and
src/main.rs first; binary only, no library crate depends on it), rust/docs/PERF.md

Read rust/docs/PLAN.md "Performance phase": the goal of the phase is to speed up the port
WITHOUT changing a single output byte; this package builds the measuring instrument and the
ranked hotspot list. It changes no library code.

1. Harness `kicad-bench`: for a fixed board set - small (demos/ecc83/ecc83-pp,
   qa/data/pcbnew/issue10697), medium (demos/complex_hierarchy, demos/interf_u,
   demos/StickHub, demos/kit-dev-coldfire-xilinx_5213), large (the two large demos the
   machine rules allow one at a time: check sizes; never more than one large board in
   memory) - time each stage separately through the kicad-session API (the same calls
   kicad-rs makes): parse, project binding + load sequence, connectivity build, zone refill
   (all zones), DRC run (and its phases: cache generator, each provider), Gerber export
   (all layers), drill export, board write. Wall-clock with warm-up and N repetitions
   (median, min, spread), peak RSS (read /proc/self/status VmHWM per stage in a fresh
   process), allocation counts if cheap to get (a counting global allocator behind a cargo
   feature). Output: one TSV line per board x stage, appended to rust/docs/PERF.md by a
   script (table per commit hash).
2. Reference baseline on the same machine: `kicad-cli` wall-clock for the same operations
   (`pcb upgrade --force`, `pcb drc`, `pcb drc --refill-zones`, `pcb export gerbers`,
   `pcb export drill`; COPIES, ORACLE.md; note that the reference uses a thread pool with
   the machine's 4 cores and the port is sequential - record `KICAD_...`/taskset
   single-core numbers too if the reference can be pinned with `taskset -c 0`).
3. wasm: the same stages under node through rust/web (the wasm-release profile), one
   small and one medium board.
4. Profiles: `perf` if available (check; otherwise a sampling approach that works in the
   container: e.g. cargo flamegraph is not installed - use `perf record` / `perf script` if
   the kernel allows it, else valgrind --tool=callgrind on a small board if installed, else
   manual stage timers with finer granularity). Produce for DRC, zone fill, Gerber export
   and parse on a medium board: the top 25 functions by self time and by inclusive time.
5. docs/PERF.md: the baseline tables, the ranked hotspot list with a one-line hypothesis
   per hotspot (allocation churn, repeated work, cache misses, lock overhead ... - the plan
   lists known suspects: KeyedCache mutex + Arc clone per read, `indexable_subshapes()`
   allocating a Vec per call, `Box<dyn Shape>` churn in collisions, the board time stamp
   invalidating caches, netclass matchers rebuilt per lookup), and for each the files it
   would touch, so the next step can hand out disjoint optimisation tasks.

The machine is shared with other agents while you run (about ten): timings are noisy.
Take each measurement at least 7 times, report the minimum and the median, and say in
PERF.md that the baseline will be re-measured on a quiet machine before any optimisation
is judged. Build with the release profile (`cargo build --release -p kicad-bench`: check
disk space first - PORTING.md section 7 - the release target directory costs about 1 GB).
