# WIRE-1: End-to-end wiring of the session, CLI and WebAssembly API

Crate: kicad-session, kicad-cli, kicad-wasm (and rust/web, rust/scripts)

W1 built the session / CLI / wasm skeleton with `NotYetPorted` stubs behind every job; since
then the subsystems landed and several packages wired their own job (PCBIO-15 load / save,
DRC-12 `pcb drc`, FAB-07 gerbers, FAB-11 svg). Take stock first: read the PORT_STATUS.md of
kicad-session, kicad-cli, kicad-wasm, kicad-drc (DRC-12), kicad-fab (FAB-02, FAB-07, FAB-08,
FAB-11, FAB-12), kicad-zone and kicad-connectivity, then `rg -n NotYetPorted crates/kicad-session
crates/kicad-cli crates/kicad-wasm` and close every stub whose subsystem exists:

1. `pcb export drill` (pcbnew_jobs_handler.cpp JobExportDrill): map the session job onto
   `kicad_fab::jobs_handler::job_export_drill` (FAB-02's change request describes the exact
   mapping: out path through resolveJobOutputPath, DrillExportEnv { build_version,
   creation_date }, the output path resolver, the reporter adapter - "Created file" messages
   are Severity::Action, "Done." is Info; on !success return ERR_INVALID_OUTPUT_CONFLICT).
2. `--refill-zones` / `--save-board` of `pcb drc`, `--check-zones` of the gerbers job, and
   `preparePlotLayers` zone refill: call kicad-zone's filler if it is ported (else keep the
   NotYetPorted error for that option only and say so).
3. The load path: `Session::load_board` must run every step of the C++ entry point that
   kicad-cli uses (BOARD_LOADER::Load: PENDING_LOAD_STEPS must end up empty or each
   remaining entry justified), incl. the DRC rule file load with its fatal-error path, the
   exclusion codec (`kicad_drc::exclusion::KiapiDrcExclusionCodec` in place of the raw one),
   variants (`--variant`, BOARD::SetCurrentVariant) and the `-D` text variable overrides.
4. The WebAssembly API (crates/kicad-wasm/src/api.rs) and the browser demo (rust/web):
   loadBoard(bytes, project?, rules?) / saveBoard / runDrc(options) -> report text + JSON /
   exportGerbers(options) / exportDrill(options) / exportSvg(options) -> named files, item
   access for an external router (nets, tracks, vias, add / remove, then DRC and export) -
   the use case: a web app validates the result of an autorouter with KiCad's DRC and
   produces the Gerber and drill files for manufacturing, without a KiCad install. Results
   must be the same bytes natively and in wasm (the libm rule of PORTING.md section 9
   applies; report every difference you find). Update rust/web (drop a board, see DRC
   violations, download a zip of Gerbers + drill files, SVG preview per layer) and
   rust/web/test/smoke.mjs (node; currently 49 checks) plus a Playwright page test (chromium
   is preinstalled: require('playwright'), no install step) that loads the demo page, runs
   DRC and an export on a small board and compares with the native CLI output.
5. UUID generation: the model draws fresh UUIDs from a seeded `KiidGenerator` (deterministic
   for tests). The hosts must seed it per run: OS randomness natively, crypto.getRandomValues
   in wasm, with a `--deterministic-uuids <seed>` style switch / API option for reproducible
   runs (DRC-06 found that the default seed reproduces UUIDs stored in QA boards).
   Also host-supplied values the model now reads (review R2-01 / R2-02 / R2-05): set
   `Board::text_var_clock` (CURRENT_DATE / CURRENT_TIME_* text variables), the project
   name, and a VCS hash where the host has one (native: ask git; wasm: API option; default
   "no hash" as the reference outside a repository); hosts must seed with
   `Board::seed_generator` (replacing `board.kiid_generator` loses the reservation of the
   file's UUIDs).
6. `rust/scripts/build-wasm.sh`: keep it working (wasm-bindgen 0.2.130 is pinned; wasm-opt
   is not installed - optional step), report module size.

You own crates/kicad-session, crates/kicad-cli, crates/kicad-wasm, rust/web, rust/scripts.
Anything missing in a subsystem crate goes into change_requests (do not stub over it).

Verification: an end-to-end differential test in kicad-cli (tests/e2e_cli.rs + a script
under tests/oracle that caches the reference results under rust/target/e2e/): for every
corpus board under 300 KB that loads, run `kicad-rs` and the reference `kicad-cli` (COPIES
in a scratch dir, ORACLE.md) for: `pcb upgrade --force`; `pcb drc --format report` and
`--format json --severity-all`; `pcb export gerbers`; `pcb export drill` (excellon and
gerber); `pcb export svg --mode-multi`. Compare stdout, exit code and every output file
byte for byte after the documented normalisations (dates, version strings, GUIDs, paths).
Print a scoreboard per command: boards identical / differing / not runnable (with the
reason: which subsystem reports NotYetPorted or panics), and commit a compact summary under
tests/golden. Differences inside a subsystem are not yours to fix: list each with the
smallest reproducing board in `known_gaps` so the orchestrator can dispatch a fix package.
