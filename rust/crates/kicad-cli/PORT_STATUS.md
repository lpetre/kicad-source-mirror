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
  upgrade) on three corpus boards: today all stop at the board parser with
  `kicad-rs: not yet ported: <subsystem>` and exit 2; once a job runs through, the same test
  compares exit code, stdout and the produced file set with the oracle.
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
* Board jobs cannot complete until the parser, kicad-drc and kicad-fab land.
