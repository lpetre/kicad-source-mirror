# CLI-WASM-1: The command line as one WebAssembly call

Crate: kicad-cli (make the command layer a library over an abstract host), kicad-wasm (the
binding), rust/web/test (tests), rust/docs/WASM_API.md (a section)

A client whose own layer is shaped like `kicad-cli` (import, export, DRC, zone refill)
should be able to replace it in the browser with one call:

    runCli(argv: string[], files: { [path]: Uint8Array | string }, options?)
        -> { exitCode, stdout, stderr, files: { [path]: Uint8Array } }

with exactly the behaviour of `kicad-rs` on a real file system: same commands and options
(`pcb drc`, `pcb upgrade`, `pcb export gerbers | drill | svg`, `fp upgrade`, `version`),
same messages, exit codes and output file names, project / rule / local-settings files
found next to the board the same way, footprint library tables found the way kicad-cli's
host finds them.

Work:
1. kicad-cli: the commands reach the outside world through `host.rs` (file reads and
   writes, directory creation and listing, the clock, random UUID seed, environment
   variables, the current directory, stdout / stderr). Put that behind one trait (`Host`)
   with the real implementation for the binary and an in-memory implementation
   (`MemHost`: a path -> bytes map with a working directory, captured stdout / stderr, a
   fixed or caller-supplied clock and seed). Expose the command dispatch as a library
   function `run(argv, &mut dyn Host) -> exit code` (lib target next to the bin target;
   the binary becomes a thin main). No behaviour change for the native binary: the
   end-to-end comparison against the reference (tests/e2e_cli.rs with its golden,
   tests/oracle/e2e_cli.py) must give the same scoreboard before and after.
2. kicad-wasm: `runCli` over `MemHost` (kicad-wasm depends on the kicad-cli library;
   check the dependency direction and that nothing native-only - std::fs, std::env,
   /dev/urandom - is reachable on wasm32: gate it with cfg, not with stubs that panic).
   Options: `date` and `uuidSeed` for reproducible runs (the environment variables
   KICAD_RS_DATE / KICAD_RS_UUID_SEED of the native binary), `env`, `cwd`.
3. Tests: rust/web/test/cli.mjs (node): for five boards and each command, `runCli` in wasm
   vs the native `kicad-rs` binary with the same arguments, date and seed: exit code,
   stdout, stderr and every output file byte-identical; one Playwright page check; a
   Rust test of `run` over `MemHost` against the e2e golden for the small boards.
4. Docs: a "command line in the browser" section in rust/docs/WASM_API.md with a worked
   example (DRC with refill and save; Gerbers + drill into a zip).

Other agents: WASM-API-1 (kicad-session / kicad-wasm API, may still be running or just
finished: read its PORT_STATUS section and WASM_API.md; append, do not restructure) and
GLUE-3 (small edits). Re-read a file right before you edit it.
