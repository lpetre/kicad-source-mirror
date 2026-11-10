# kicad-session port status

Owner object of the headless port (`docs/design/board-model.md` 14 and 17): `Session` (board +
`.kicad_pro` / `.kicad_dru` texts + file name), the job structs and one runner per job. No
filesystem, process or clock access; `kicad-cli` and `kicad-wasm` are the hosts.

## Mapping

| C++ | Rust | State |
|---|---|---|
| `include/cli/exit_codes.h` | `exit_codes.rs` (+ `NOT_YET_PORTED` = `ERR_UNKNOWN`, new) | ported |
| `common/build_version.cpp` `GetMajorMinorPatchVersion`, `GetMajorMinorVersion`, `GetCommitHash`, `GetBuildVersion` | `build_version.rs` (values of the reference build's `kicad_build_version.h`) | ported |
| `GetVersionInfoData` | `build_version::version_info_data` | partial: application and version lines only (see Deviations) |
| `common/jobs/job.h/.cpp` `JOB`, `JOB_OUTPUT` | `jobs/job.rs` | ported except `JOB_PARAM*` JSON (skipped: jobsets) and env/`~` expansion (host) |
| `JOB::ResolveOutputPath`, `GetFullOutputPath`, `SetConfiguredOutputPath`, `PrependDirectoryToPath` | `Job::resolve_output_path`, `full_output_path`, ... | ported (text vars: title block, then project vars) |
| `JOB::AppendDefaultFileName` | - | skipped: needs `wxFileName::DirExists` (filesystem); no caller in the ported jobs |
| `common/jobs/job_rc.h/.cpp` | `jobs/job_rc.rs` | ported (+ JSON tokens) |
| `common/jobs/job_pcb_drc.h/.cpp` | `jobs/job_pcb_drc.rs` | ported |
| `common/jobs/job_export_pcb_plot.h/.cpp` (`ResolvePlotLayers`) | `jobs/job_export_pcb_plot.rs` | ported; `FromJson` skipped (jobsets) |
| `common/jobs/job_export_pcb_gerber.h/.cpp`, `job_export_pcb_gerbers.h/.cpp` | `jobs/job_export_pcb_gerber.rs` | ported |
| `common/jobs/job_export_pcb_drill.h/.cpp` | `jobs/job_export_pcb_drill.rs` | ported |
| `common/jobs/job_export_pcb_svg.h/.cpp` | `jobs/job_export_pcb_svg.rs` | ported |
| `common/jobs/job_pcb_upgrade.h/.cpp`, `job_fp_upgrade.h/.cpp` | `jobs/job_upgrade.rs` | ported |
| `PCBNEW_JOBS_HANDLER::convertLayerArg` | `jobs_handler::convert_layer_arg` | ported, oracle-verified |
| `JobExportGerbers` layer selection (L1784-1830) | `jobs_handler::gerbers_layers_to_plot` | ported, oracle-verified |
| `preparePlotLayers` | `jobs_handler::prepare_plot_layers` | ported (zone refill done by the caller first, as in C++) |
| `resolveJobOutputPath` | `jobs_handler::resolve_job_output_path` | partial: `loadOverrideDrawingSheet` and `BOARD::SynchronizeProperties` not called yet; project vars = the `-D` overrides |
| `JobExportDrc` | `run_drc` | stub after the output-name logic: `NotYetPorted("kicad-drc ...")` |
| `JobExportGerbers` | `run_export_gerbers` | stub after layer selection: `NotYetPorted("kicad-fab ...")` |
| `JobExportDrill` | `run_export_drill` | stub after the report path logic: `NotYetPorted("kicad-fab ...")` |
| `JobExportSvg` (+ `resolvePlotOutputPath`) | `run_export_svg` | stub after layer preparation: `NotYetPorted("kicad-fab ...")` |
| `JobUpgrade` | `run_pcb_upgrade` | ported (`RefreshGeneratedTables` skipped, see Deviations); oracle-verified |
| `JobExportFpUpgrade` | `run_fp_upgrade` + the filesystem checks in `kicad-cli` | stub: footprint library parser/writer pending |
| `getBoard` / `BOARD_LOADER::Load` | `Session::load_board` | partial: parser (`kicad_pcb::io::parse_board`; constructs whose parser package is pending give `NotYetPorted`); `initializeLoadedBoard` steps listed in `PENDING_LOAD_STEPS` |
| `PCB_IO_KICAD_SEXPR::CanReadBoard` + `IsValidBoardHeader` | `session::can_read_board` | ported (move to `kicad_pcb::io` when the parser lands) |
| `REPORTER` / `CLI_REPORTER` | `reporter.rs` (recorded lines; the host prints) | ported |
| item access (`nets`, `tracks`, `add_track`, `add_via`, `remove`) | `Session` methods on the kicad-pcb model | new API over ported model code |

## Tests

* `tests/gerbers_layer_args.rs`: differential test against the reference `kicad-cli`
  (`tests/oracle/gen_gerbers_layer_args.sh`, golden `tests/golden/gerbers_layer_args.txt`):
  22 `--layers` arguments (groups, GUI and file names, whitespace, empty tokens, bad names,
  disabled layers) - messages and plotted layers in `UIOrder` identical.
* `tests/upgrade_oracle.rs`: `Session::load_board` + `run_pcb_upgrade` on
  `qa/data/pcbnew/issue10697.kicad_pcb` against the reference `kicad-cli pcb upgrade --force`
  (`tests/oracle/gen_upgrade_golden.sh`, goldens `tests/golden/upgrade/`): re-saved board
  byte-identical, message and exit code equal, with and without `--force`; the second run on
  the upgraded board (`Board file was not updated`) is checked once the parser reads
  `embedded_fonts` (PCBIO-11).
* unit tests: job path resolution, default output names, tokenizer, item access on an empty
  board, pending parser.

## Deviations

* `GetVersionInfoData` (`kicad-cli version --format about`): the C++ lists host platform,
  wxWidgets/FreeType/... versions, build date and locale; the port prints the application and
  version lines and the ported commit (no host queries in library crates).
* The commit hash is the pinned commit of the C++ tree the port follows, not the Rust
  workspace's git state.
* Output paths are `/`-joined strings; `ExpandEnvVarSubstitutions` and `~` are left to the host.
* `JobUpgrade`: `RefreshGeneratedTables` is not run (generated tables not refreshed in tier 1,
  board-model.md 17), so a board whose only change would be a refreshed chart is reported as not
  updated.
* `run_*` record the reporter messages and return the files; the host writes the files after the
  run (C++ creates the output directory before running the job and writes during it).
* Item access (`add_track` / `add_via`) appends (`ADD_MODE::APPEND`); the via width/drill are set
  through the padstack exactly as `PCB_VIA::SetWidth( w, ALL_LAYERS )` / `SetDrill` do (those
  two methods are not in kicad-pcb yet).

## Known gaps

* Everything behind the parser: no job can run end to end yet; `NotYetPorted` names the
  subsystem (`kicad-pcb` parser/writer, `kicad-drc`, `kicad-fab`, `kicad-zone`,
  `kicad-connectivity`).
* Variants (`BOARD::SetCurrentVariant`) and the override drawing sheet are not wired.
* Footprint library loading for DRC library parity is out of scope.
