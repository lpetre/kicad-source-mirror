# kicad-cli (`kicad-rs`) port status

Native binary mirroring the reference `kicad-cli` for the ported commands. clap 4 parses; the
jobs run in `kicad-session`; this crate does the file I/O and the terminal output.

## Mapping

| C++ | Rust | State |
|---|---|---|
| `kicad/kicad_cli.cpp` `PGM_KICAD::OnPgmRun` (command tree, `--version`, help, parse error output, exit codes) | `main.rs` | ported for `version`, `pcb drc`, `pcb export gerbers/drill/svg`, `pcb upgrade`, `fp upgrade` |
| `kicad/cli/command.h/.cpp` (`addCommonArgs`, `addDrawingSheetArg`, `addDefineArg`, `addVariantsArg`, `Perform`) | `command.rs` | ported |
| `wxStringSplit` (`common/string_utils.cpp`) | `command::wx_string_split` | ported |
| argparse `scan<'i'>` / `scan<'g'>` number parsers and error texts | `argparse_compat.rs` | ported (decimal/hex/binary/octal ints, strtod-like doubles) |
| `command_version.cpp` | `command_version.rs` | ported |
| `command_pcb_drc.cpp` | `command_pcb_drc.rs` | ported |
| `command_pcb_export_base.cpp` (`addLayerArg`, `addCommonLayersArg`) | `command_pcb_export_gerbers.rs` | ported (fab-export args of `addFabExportArgs` not needed: no IPC/ODB commands) |
| `command_pcb_export_gerber.cpp` (`populateJob`), `command_pcb_export_gerbers.cpp` | `command_pcb_export_gerbers.rs` | ported |
| `command_pcb_export_drill.cpp` | `command_pcb_export_drill.rs` | ported |
| `command_pcb_export_svg.cpp` | `command_pcb_export_svg.rs` | ported |
| `command_pcb_upgrade.cpp`, `command_fp_upgrade.cpp` | `command_upgrade.rs` | ported |
| `PCBNEW_JOBS_HANDLER::getBoard` (project / rules lookup), `JobExportFpUpgrade` filesystem checks, `GuessPluginTypeFromLibPath` for `.pretty` | `host.rs`, `command_upgrade.rs` | ported (KiCad s-expression libraries only) |
| `CLI_REPORTER::Report` | `host::print_report` | ported |

## Tests (`tests/cli.rs`, oracle-aware; skip the oracle parts when it is absent)

* `version`, `--version`, `-v`, `--format commit|about|bogus` - stdout and exit codes equal.
* help: `--help`, bare `pcb` / `pcb export` / `fp`, `-h` after other args: exit 0 (oracle too).
* every option name, short name and alias of the seven commands is present.
* 26 argument-error cases: exit code and argparse's first stdout line equal the oracle
  (`input: 1 argument(s) expected...`, `Unknown argument`, `Duplicate argument`, number parse
  errors, `did you mean`, `Zero positional arguments expected...`), plus the C++ stderr texts.
* board jobs (drc, drc json/mils, gerbers, gerbers -l, drill excellon/gerber, svg multi,
  upgrade with and without `--force`) on four corpus boards: a job that cannot run yet stops
  with `kicad-rs: not yet ported: <subsystem>` and exit 2; a job that runs through is compared
  with the oracle (exit code, stdout, produced file set, and the input board file byte for
  byte). Today `pcb upgrade` on `qa/data/pcbnew/issue10697.kicad_pcb` runs through and is
  byte-identical; the other boards stop at the parser (footprints), the other jobs at
  kicad-drc / kicad-fab.
* `fp upgrade`: the parser-free checks (output conflict, non-KiCad library without output) equal
  the oracle.

## Deviations

* Help layout and usage lines are clap's (option semantics, defaults and names are the C++
  ones). argparse's `[nargs=0..1]` is reproduced: an option with a default given without a
  value keeps the default.
* "did you mean" suggestions are computed over the ported subcommands only, so the suggestion
  can differ from the reference for names close to an unported command.
* The reference loses the texts it writes with `wxFprintf( stderr, ... )` after wxLog has
  written to stderr (`Invalid units specified`, `Invalid format`, ...: exit code right, text
  missing). `kicad-rs` prints them, as the C++ source intends.
* `--drill-shape-opt` values outside 0..2 (an invalid `DRILL_MARKS` in C++) map to
  `FULL_DRILL_SHAPE`.
* argparse hex-float input for `--scale` is not supported (rejected as not matching).
* Exit code 2 (`ERR_UNKNOWN`) plus `kicad-rs: not yet ported: <subsystem>` on stderr when a run
  needs a missing subsystem; `.brd` legacy boards and non-KiCad footprint libraries report this
  too.
* Output files are written after the job returns (C++ writes during the job).
* The `.kicad_prl` side file and lock files the reference writes are not produced.

## Known gaps

* Only the commands above; everything else of the reference CLI is out of scope.
* `pcb export svg` stops at kicad-fab (FAB-11 is wiring it: drawing sheet pending).
* DRC differences inside kicad-drc (see the scoreboard below).

## Package WIRE-1: host inputs and the end-to-end differential test

Host (`host.rs`):
* `getBoard`: the project path made absolute (`wxFileName::MakeAbsolute`, `.`/`..`
  normalised) and the `.kicad_prl` passed (`LoadInputs::project_path`, `local_settings_json`);
  the board path stays as given (C++ `SetFileName( aFileName )`).
* `CLI_REPORTER::Report`: a `\n` is appended to a message without one (the drill writer's
  "Created file ..." / "Done.").
* Parser warnings (`LOAD_INFO_REPORTER`) on stderr as `HH:MM:SS: Warning: <text>` (wxLog).
* Outline font fallbacks (FONT-RC-1) on stderr as `Warning: text uses font ...` (new; the
  reference has no counterpart; the e2e comparison ignores these lines).
* Clock and UUIDs: `JobPcbDrc::report_date` (ISO 8601), the Gerber / drill creation date, the
  `CURRENT_DATE` / `CURRENT_TIME_*` clock from the system time; the UUID generator and the DRC
  marker seed from `/dev/urandom`. `KICAD_RS_DATE=YYYY-MM-DDTHH:MM:SS` and
  `KICAD_RS_UUID_SEED=<u32>` fix them for reproducible runs (environment variables rather than
  options so that the argparse-compatible option set and help stay those of the reference).
* `JobPcbDrc::schematic_exists`: `<board>.kicad_sch` / `.sch` next to the absolute board path.

End-to-end test: `tests/oracle/e2e_cli.py` (reference results cached under
`rust/target/e2e/ref/`), golden `tests/golden/e2e_cli.txt` (board, command, status, hash of
the normalised reference outputs), replay `tests/e2e_cli.rs` (default: the `identical` pairs of
the boards under 60 KB for upgrade / gerbers / drill, 1005 pairs; `--ignored`: all 1263
`identical` pairs incl. DRC). Normalisations: dates (ISO, Gerber `G04` date, wxLog time
stamps), the scratch directory; masks (status `identical-masked`): the port's list of DRC checks
it cannot run, the DRC report's item line of the board itself (`@(..): PCB`, kicad-drc DRC-23
gap: no `BOARD::m_Uuid`), random UUIDs of legacy files (FIX-RESAVE-1).

Scoreboard (308 corpus boards under 300 KB, every one loads; exit code, stdout, stderr and every
output file compared):

| command | identical | identical after masking | differing | not runnable |
|---|---|---|---|---|
| `pcb upgrade --force` | 253 | 54 (random UUIDs) | 1 | 0 |
| `pcb drc --format report` | 43 | 79 | 186 | 0 |
| `pcb drc --format json --severity-all` | 43 | 20 | 245 | 0 |
| `pcb export gerbers` | 308 | 0 | 0 | 0 |
| `pcb export drill` (Excellon) | 308 | 0 | 0 | 0 |
| `pcb export drill --format gerber` | 308 | 0 | 0 | 0 |
| `pcb export svg --mode-multi` | 0 | 0 | 0 | 308 (kicad-fab SVG, FAB-11) |

Differences (all inside subsystems): `upgrade` `issue25580` (teardrop zone order, a random
UUID per reference run, FIX-RESAVE-1); DRC: 175 boards whose reference reports
`lib_footprint_issues` / `lib_footprint_mismatch` (library parity, DRC-34, e.g.
`demos/ecc83/ecc83-pp.kicad_pcb`), the JSON item of the board itself (DRC-23, e.g.
`qa/data/pcbnew/drc_courtyard/invalid/empty_board.kicad_pcb`), and 10 boards with other
entry / count differences (`connection_width_rules`, `issue5854`, `issue24525`, `issue24935`,
`microwave`, `severities`, `issue24853`, `issue25439`, `two_segment_teardrop`, `issue24078`).
