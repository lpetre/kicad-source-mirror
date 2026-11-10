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
| `resolveJobOutputPath` | `jobs_handler::resolve_job_output_path` | ported (WIRE-1) except `loadOverrideDrawingSheet` (no drawing sheet model) |
| `JobExportDrc` | `run_drc` | ported (DRC-12): DRC itself in `kicad_drc::run_flow::run_drc_job`; see the DRC-12 section |
| `JobExportGerbers` | `run_export_gerbers` | ported (FAB-07, `kicad_fab::jobs_handler::job_export_gerbers`); the Gerber job file (FAB-09) is missing: layer files + exit `NOT_YET_PORTED`; verified in `tests/gerbers_job.rs` (see kicad-fab `PORT_STATUS.md` FAB-07) |
| `JobExportDrill` | `run_export_drill` | ported (WIRE-1) onto `kicad_fab::jobs_handler::job_export_drill`; e2e-verified (below) |
| `JobExportSvg` (+ `resolvePlotOutputPath`) | `run_export_svg` | stub after layer preparation: `NotYetPorted("kicad-fab ...")` |
| `JobUpgrade` | `run_pcb_upgrade` | ported (a board holding a generated table is `NotYetPorted`: `RefreshGeneratedTables`, M26); oracle-verified over the corpus (PCBIO-15) |
| `JobExportFpUpgrade` | `run_fp_upgrade` + the filesystem checks in `kicad-cli` | stub: footprint library parser/writer pending |
| `getBoard` / `BOARD_LOADER::Load` | `Session::load_board`, `load_board_with`, `board_loader::load` | ported (PCBIO-15, below) |
| `BOARD_LOADER::initializeLoadedBoard`, `KI_TEST::LoadBoard`, `BOARD_LOADER::SaveBoard`, `RefreshGeneratedTables` | `board_loader.rs` | ported (PCBIO-15, below) |
| `PCB_IO_KICAD_SEXPR::CanReadBoard` + `IsValidBoardHeader` | `session::can_read_board` | ported (move to `kicad_pcb::io` when the parser lands) |
| `REPORTER` / `CLI_REPORTER` | `reporter.rs` (recorded lines; the host prints) | ported |
| item access (`nets`, `tracks`, `add_track`, `add_via`, `remove`) | `Session` methods on the kicad-pcb model | new API over ported model code |

## Tests

* `tests/gerbers_layer_args.rs`: differential test against the reference `kicad-cli`
  (`tests/oracle/gen_gerbers_layer_args.sh`, golden `tests/golden/gerbers_layer_args.txt`):
  22 `--layers` arguments (groups, GUI and file names, whitespace, empty tokens, bad names,
  disabled layers) - messages and plotted layers in `UIOrder` identical.
* `tests/gerbers_job.rs` (FAB-07): `run_export_gerbers` against the reference
  `kicad-cli pcb export gerbers` (crafted boards x 7 option sets, the `test_pcb.py` gerber
  cases, the corpus with defaults and `--no-x2 --use-drill-file-origin`; golden
  `tests/golden/gerbers_job/`); results in kicad-fab `PORT_STATUS.md` FAB-07.
  `JobExportPcbPlot::creation_date` (new, default zeros) carries the plot date the C++ reads
  from the clock.
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
* `run_*` record the reporter messages and return the files; the host writes the files after the
  run (C++ creates the output directory before running the job and writes during it).
* Item access (`add_track` / `add_via`) appends (`ADD_MODE::APPEND`); the via width/drill are set
  through the padstack exactly as `PCB_VIA::SetWidth( w, ALL_LAYERS )` / `SetDrill` do (those
  two methods are not in kicad-pcb yet).

## Known gaps

* Everything behind the parser: no job can run end to end yet; `NotYetPorted` names the
  subsystem (`kicad-pcb` parser/writer, `kicad-drc`, `kicad-fab`, `kicad-zone`,
  `kicad-connectivity`).
* The override drawing sheet (`--drawing-sheet`) is not wired.
* Footprint library loading for DRC library parity is out of scope.

## Package PCBIO-15: load / save orchestration

Files: `src/board_loader.rs`, the loader part of `src/session.rs` (`load_board`,
`load_board_with`, `load_board_qa`, `from_loaded`, `PENDING_LOAD_STEPS` now empty, the
`connectivity` / `drc_engine` / `drc_init_error` / `load_warnings` fields), `run_pcb_upgrade`,
`src/bin/pcb_dump_session.rs` (feature `dump`), `tests/oracle/pcbio15_dump_corpus.sh`, `tests/common/mod.rs`,
`tests/upgrade_corpus.rs`, `tests/upgrade_oracle.rs`.

| C++ | Rust | State |
|---|---|---|
| `BOARD_LOADER::Load` (default `OPTIONS`, as `getBoard` calls it; `getProjectForBoard`) | `board_loader::load`, `Session::load_board(_with)` | ported |
| `BOARD_LOADER::initializeLoadedBoard` | `board_loader::initialize_loaded_board(_guarded)`: model steps through `kicad_pcb::project::run_model_step` (SET-5) in `LoadPath::Cli.steps()` order; session steps here: `DRC_ENGINE` + `InitEngine( <board>.kicad_dru )` (errors swallowed, kept in `drc_init_error`), `ResolveDRCExclusions( true )` + `Add( marker )`, `BuildConnectivity` (kicad-connectivity: fill triangulation, build, net propagation applied with `set_item_net`, ratsnest exclusions) | ported |
| `KI_TEST::LoadBoard` | `board_loader::load_qa`, `Session::load_board_qa` | ported (project only with a `.kicad_pro` / `.pro`, no file name, `default_layer_choices`, rule errors escape and stop the sequence) |
| `PCB_IO::LoadBoard` / `PCB_IO_MGR::Load` / `DoLoad` error mapping | `board_loader::parse` + `session::io_error` | ported (`Failed to load board: <What()>`) |
| `BOARD_LOADER::SaveBoard` (pre-steps) | `board_loader::save_board` | ported (no caller yet: `pcb drc --save-board` is DRC-12's) |
| `RefreshGeneratedTables` | `board_loader::refresh_generated_tables` | partial: 0 without generated tables; `NotYetPorted` (M26) when the board holds one |
| `BOARD_LOADER::CreateEmptyBoard` | `Session::new_empty` | partial: no DRC engine (`drc_engine: None`) |
| drawing sheet load, `ENUM_MAP<PCB_LAYER_ID>` fill | - | skipped: no global drawing sheet; the enum only feeds the rule parser (`board_layer_choices`) |

Verification:

- `pcb-dump-session` (this crate's dump driver: `kicad_pcb::dump::dump_board` on a board
  loaded through `load_board_qa` / `load_board_with`; `ds.biggest_clearance` re-evaluated with
  the session's DRC engine; opt-in `connectivity` = the `rn_net` summary `name`, `node_count`,
  `unconnected_count`, `unconnected_visible`) vs the reference `pcb-dump board` (QA) and
  `pcb-dump board --loader`, sections `board,item,track,connectivity`, over every corpus board
  under 500 KB that loads with the strict parser (110 of 329; 218 stop at `pad (PCBIO-07)`,
  `fakeboard` is rejected by both). Identical numbers on both paths, per path: `item` 21 912
  match / 0 mismatch (nets of every connected item), `rn_net` 326 / 0, `board` 82 624 / 9
  (4 950 pending: `PNS::MEANDER_SETTINGS`), `track` 19 576 / 14. The mismatches: 8 boards
  whose `.kicad_dru` conditions reach the DRC-21 stub `PCBEXPR_UCODE::CreateVarRef` in
  `InitEngine` (engine without the user rules, so `ds.biggest_clearance` differs);
  `issue24525` `bbox` (`ComputeBoundingBox`, the same with kicad-pcb's `pcb-dump-rs`: model,
  not load); 14 `flash.In1.Cu` / `shape.In1.Cu` lines of `issue10697` (dump without
  connectivity context, see Known gaps).
- `tests/upgrade_corpus.rs` (`--ignored`): `load_board` + `run_pcb_upgrade( --force )` on the
  io-corpus oracle cache (`O1` in, `O2` expected), strict parser: 111 identical, 0 differing,
  259 not loadable (258 `pad (PCBIO-07)`, 1 `PCB_TABLE::Compare`), 0 failed; 2 of the
  identical boards are not fixed points (`issue18142`, `issue24649`).
- `tests/upgrade_oracle.rs` unchanged expectations, now through the full load sequence.

Deviations (PCBIO-15):

- The project path is derived from `source_name` as given; kicad-cli makes the board path
  absolute first (`MakeAbsolute`), which is the host's job (no cwd in library crates).
- `InitEngine` gets empty `TUNING_PROFILES` (not in the project model yet, SET-11): implicit
  tuning-profile rules are missing from the engine (no effect on the saved board).
- `Session::new_empty` has no DRC engine (`CreateEmptyBoard` creates one).

Known gaps (PCBIO-15):

- The load reaches the `port_pending!` stub
  `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments` in the
  `SynchronizeComponentClasses` step on **every** load (both paths when a project exists): the
  library entry points panic there (kicad-cli reports it as not yet ported). The tests and the
  dump driver load with `board_loader::load_guarded`, which skips steps stopped by a known stub
  (`tests/common::KNOWN_PENDING_STEPS`); remove the entry when the stub lands.
- Net propagation onto tracks (`propagateConnections` needs pads as cluster origins) is not
  exercised by the strictly loadable corpus boards (no pads until PCBIO-07): rerun
  `cargo test -p kicad-session --test upgrade_corpus -- --ignored --nocapture` and the dump
  corpus when the pad parser lands.
- `pcb-dump-session` dumps through `BoardCtx::bare` (`kicad_pcb::dump::dump_board` takes no
  context): via `flash.<layer>` / `shape.<layer>` with `remove_unused_layers` differ from the
  C++ (which flashes through connectivity); those lines are verified by kicad-connectivity's
  CN-04 goldens. A `dump_board` variant taking a `BoardCtx` (kicad-pcb `dump`) would close it.

## Package CC-1: component class synchronisation in the load sequence

`board_loader::initialize_loaded_board_guarded` installs kicad-drc's component class rule
compiler on the board (`kicad_drc::component_class_rules::install`) before the steps, so
`LoadStep::SynchronizeComponentClasses` (`BOARD::SynchronizeComponentClasses`) runs on every
load; the `KNOWN_PENDING_STEPS` entry of `tests/common/mod.rs` is gone.

Tests (`tests/component_classes.rs`):
* `component_classes_match_reference_dump`: `GetComponentClassAsString()` of every footprint of
  `component_classes` and `component_classes_drc` (the corpus boards whose projects assign
  classes) vs the reference `pcb-dump board [--loader] --sections footprint` (golden
  `tests/golden/cc1_component_class.txt`, `tests/oracle/cc1_component_class_golden.sh`), QA and
  CLI path: 26 of 36 identical, the 10 others (5 footprints x 2 paths) need DRC-22 functions
  (`getField`, `memberOfSheet`, `intersectsArea`) and are checked by the ignored
  `component_classes_match_reference_dump_all` only.
* `component_classes`: port of qa/tests/pcbnew/test_component_classes.cpp `ComponentClasses`
  (same table), the same 5 footprints in the ignored `component_classes_all`.
* `upgrade_oracle` (issue10697 byte-identical) and `upgrade_corpus` run the step now.

## Package FIX-RESAVE-1: the last differing boards of the re-save comparison

No code change was needed: none of the 11 boards that differed from the reference is a port
bug. Both causes are things the reference cannot reproduce itself, so `tests/upgrade_corpus.rs`
now compares through a documented normalisation.

- **Render caches** (`tinytapeout-demo`, `issue22475`, and together with zone order
  `RoyalBlue54L-Feather` and `issue14549`; these are the only 4 corpus boards with a
  `(render_cache ..)`). The C++ drops the cache it read for every text. Board text:
  `parsePCB_TEXT` ends with `SetVisible( true )`, which calls `ClearRenderCache`
  (`pcb_io_kicad_sexpr_parser.cpp`, "Hidden PCB text is no longer supported"). Footprint
  text: `SetLibTextSize`, or `SetTextAngle` / `Rotate` in legacy files. At save,
  `formatRenderCache` -> `EDA_TEXT::GetRenderCache` then rebuilds the glyphs with
  `OUTLINE_FONT::GetLinesAsGlyphs`, using whatever font fontconfig picks on the oracle
  machine (none of the requested faces is installed except FreeMono / Liberation Mono:
  Calibri -> Carlito Bold, Ubuntu Sans and Franklin Gothic Medium -> Inter). That is why `O1`
  equals `O2` for every cache, while the glyphs differ from the input file's. The port has no
  outline font engine (`kicad-font` D1). It drops the cache in the same places and writes
  only the `(render_cache "text" angle)` header. The header (resolved text and draw angle)
  equals the reference's on all 83 caches. Rule: glyph polygons are masked; nothing else
  differs on these boards.
- **Zone order** (`complex_hierarchy`, `issue11814`, `issue14559`, `issue21482`,
  `issue25580`, `teardrop_issue_JPC2`, `teardrop_spike`, plus the two boards above). The
  first differences were `zone > net` and `zone > priority`. `format( ZONE )` writes no
  `(uuid ..)` for a teardrop zone, so every load gives it a fresh random `KIID`
  (`boost::uuids::basic_random_generator`, never seeded in kicad-cli). The writer's
  `std::set<BOARD_ITEM*, BOARD_ITEM::ptr_cmp>` orders zones by type, then layer set, then
  UUID, so where a teardrop zone lands among the zones of its layer set is random. Measured:
  12 reference runs on the same `teardrop_spike` `O1` gave 2 different outputs (8 x `O2`,
  4 x `O1`). Nothing renumbers the priorities and no net changes: the zone blocks only move.
  Rule: zones without a deterministic UUID are compared as a set; zones with one keep their
  order.
- Legacy inputs: random UUIDs are masked with `canonicalise_uuids_sorted`.

Results (`upgrade_corpus_scoreboard`, input `O1`, expected `O2`): 358 identical, 11
identical after normalisation, 1 not loadable (`api_kitchen_sink`: `RefreshGeneratedTables`
is still the M26 `port_pending!` stub). So all 370 are explained and 0 differ.

`upgrade_corpus_small` (default run, no oracle): every corpus board under 300 KB. The input
is the repository file itself, with no project, and the expectation is `O1`, kept as hashes
in `tests/golden/upgrade_corpus.txt` (board, status, fnv1a64 of `O1` and of the normalised
`O1`). 252 boards are identical, 56 identical after normalisation (random UUIDs of legacy
files), 0 differ. Each board must reach at least its recorded status. Refresh the golden with
`UPGRADE_CORPUS_UPDATE_GOLDEN=1 cargo test -p kicad-session --test upgrade_corpus -- --ignored`.

`pcb upgrade` CLI behaviour (`kicad-rs` vs reference `kicad-cli`) was checked on 12 inputs:
file versions 20200119, 20200829, 20210126, 20211232, 20221018, 20230410, 20240609,
20241228, 20251101 and 20261022, a current-version (20261031) `O1`, and the known-bad
`fakeboard`. Each was run without `--force`, with `--force`, and again without `--force` on
the result. The message and exit code matched in all 36 runs: "Successfully saved board file
using the latest format" with 0, "Board file was not updated" with 0 for a current-version
board without `--force`, "Failed to load board" with 3. The output files are byte-identical
where the input has UUIDs everywhere. They are identical after normalisation for the 4
pre-2022 boards, where even two reference runs differ from each other.

## Package ZF-06: zone refill in the jobs, `--save-board`

* `Session::refill_zones` / `Session::fill_all_zones`: `ZONE_FILLER_TOOL::FillAllZones( nullptr,
  reporter, true )` through `kicad_zone::fill_all_zones` (teardrop guard, fill, connectivity
  rebuild). Called by `run_drc` (`--refill-zones`), the gerbers and SVG jobs
  (`--check-zones`) and the wasm `refillZones`. A session without a DRC engine
  (`new_empty`) first gets one initialised from `rules_text`, as `ZONE_FILLER::Fill` does.
* `jobs_handler::drc_save_board`: the `m_refillZones && m_saveBoard` block of `JobExportDrc`
  (`BOARD_LOADER::SaveBoard`, "Saved board" / "Failed to save board.", exit code 4); the saved
  board is an `OutputFile` named like the input. Not called yet: `run_drc` still returns
  `NotYetPorted` at the DRC run (DRC-12 calls it after the report).
* `tests/zf06_refill_save.rs` (ignored; needs the zone-corpus cache): 125 of 141 saved boards
  byte-identical with the reference `pcb drc --refill-zones --save-board` (details in
  kicad-zone PORT_STATUS, ZF-06).

## Package DRC-12: `run_drc` (the `pcb drc` job)

* `run_drc` follows `JobExportDrc`: output name, `InitEngine` again when the rules are not
  valid (a `PARSE_ERROR` reports `DRC incomplete: could not compile custom design rules.` +
  the error text and returns 3 without a report), `--schematic-parity` as a build without the
  schematic kiface ("Failed to fetch schematic netlist for parity tests." without a schematic,
  the annotation message with one; parity off), `--refill-zones`, the DRC
  (`kicad_drc::run_flow::run_drc_job`), `Found N violations` / `Found N unconnected items`,
  the report file, `Saved DRC Report to ...`, `--save-board`, `--exit-code-violations` (5).
* Host inputs added to `JobPcbDrc` (not C++ members): `schematic_exists`, `report_date`
  (`GetISO8601CurrentDateTime()`; empty by default), `marker_uuid_seed`. kicad-cli does not
  set them yet (change request: WIRE-1).
* Checks the port cannot run (unported providers) are listed in the report (see kicad-drc
  PORT_STATUS DRC-12) and as one `DRC check '<provider>' was not run: <reason>` warning line
  each on the reporter (stdout), after the C++ messages.
* Not wired: `SetDrawingSheet` (misc provider's drawing-sheet text variables), the project's
  `TUNING_PROFILES` (SET-11: default profiles are passed), footprint library loading.
* Tests: `tests/drc12_regressions.rs` (port of `test_drc_regressions.cpp`),
  `tests/drc12_corpus.rs` (reference report replay).

## Package WIRE-1: end-to-end wiring of the session, CLI and WebAssembly API

* `run_export_drill`: `JobExportDrill` mapped onto `kicad_fab::jobs_handler::job_export_drill`
  (options, `DrillExportEnv { build_version, creation_date }`, the report path through
  `JOB::ResolveOutputPath( .., false, project )`, the fab reporter: "Created file" Action,
  "Done." Info; a writer failure returns `ERR_INVALID_OUTPUT_CONFLICT`, an unported fab part
  `NotYetPorted`). `JobExportPcbDrill::creation_date` (new, host input) carries the clock.
* `resolve_job_output_path`: `PROJECT::ApplyTextVars( -D overrides )` on the bound project
  (they stay applied, so board texts see them), `BOARD::SynchronizeProperties`, the project's
  `TextVarResolver` (PROJECTNAME, CURRENT_DATE, project variables, ...) with the board's
  `text_var_clock`; `Job::resolve_output_path_with` / `full_output_path_with` take a resolver.
* Variants: `--variant` of gerbers / svg calls `BOARD::SetCurrentVariant` (was NotYetPorted).
* `run_drc`: the rule file of the re-init is `BOARD::GetDesignRulesPath()` (project path).
* SET-11: `board_loader::tuning_profiles( board )` reads the project's `TUNING_PROFILES`
  (kicad-pcb `project::tuning_profiles_json`) for `InitEngine` (load step, `run_drc`,
  `fill_all_zones`) and `RunTestsOptions::tuning_profiles`. The 7 `missing_tuning_profile`
  entries of `tests/drc12_corpus.rs` KNOWN_DIFFERENCES are gone (6 boards now exact).
* Load: `LoadInputs::project_path` (`getProjectForBoard`'s absolute `.kicad_pro`; the board's
  file name stays as given, as in C++), `LoadedBoard::parse_warnings` / `Session::
  parse_warnings` (`GetParseWarnings()`, printed by kicad-cli as `wxLogWarning`s).
  `PENDING_LOAD_STEPS` is empty: every `initializeLoadedBoard` step runs; the drawing sheet
  load and the `ENUM_MAP` fill are skipped as documented in PCBIO-15.
* Host inputs: `HostClock` (`host_clock.rs`: `GetISO8601CurrentDateTime`, the
  `CURRENT_DATE` / `CURRENT_TIME_*` values, the Gerber / drill creation date),
  `Session::apply_host_inputs( clock, uuid_seed )` (reseeds the board's `KiidGenerator` in
  place, keeping the reservation of the file's UUIDs), `Session::outline_font_warnings()`.
* `pcb-dump-session` dumps through `kicad_pcb::dump::dump_board_ctx` with connectivity and the
  DRC engine (the `ds.biggest_clearance` rewrite is gone). Checked against the reference
  `pcb-dump board [--loader] --sections board,item,track,connectivity` on issue10697,
  drc_missing_tuning_profile, api_kitchen_sink, component_classes_drc: 0 mismatches on both
  paths (the 14 `flash.In1.Cu` / `shape.In1.Cu` mismatches of issue10697 are gone).

Verification: kicad-cli's end-to-end differential test (`kicad-cli/tests/oracle/e2e_cli.py`,
golden `kicad-cli/tests/golden/e2e_cli.txt`, replay `kicad-cli/tests/e2e_cli.rs`), 308 corpus
boards under 300 KB, scoreboard in kicad-cli `PORT_STATUS.md`.

## Package DRC-LP: footprint libraries of a session

| C++ | Rust | State |
|---|---|---|
| `PGM_KICAD::OnPgmStart` `LoadGlobalTables()`, `SETTINGS_MANAGER::LoadProject` (`KIPRJMOD`, `LIBRARY_MANAGER::ProjectChanged` -> `LoadProjectTables`), `JobExportDrc`'s `AsyncLoad()` + `BlockUntilLoaded()` | `src/footprint_libraries.rs` `Session::load_footprint_libraries`, `Session::library_env`, `LibraryHost` | ported; the host supplies the file access (`LibrarySource`), the environment, the working and home directories, `PATHS::GetUserSettingsPath()` and the default `KICAD10_FOOTPRINT_DIR`; `Session::footprint_libraries` is passed to the DRC (`RunTestsOptions::footprint_libraries`) |

`PROJECT::TextVarResolver` tokens for library URIs: `PROJECTNAME`, the clock tokens, the VCS
tokens (resolved as `BoardProject::text_var_resolver` resolves them), then the project's text
variables. A session without `load_footprint_libraries` runs the library parity test with no
tables (every footprint with a library nickname: "does not include the footprint library").

Tests: `tests/drc_lp_library_parity.rs` (crafted project `tests/data/drc_lp`, reference golden
`tests/golden/drc_lp_library_parity.json` from `tests/oracle/gen_drc_lp_cases.py`): the 37
`lib_footprint_*` entries identical. `tests/drc12_regressions.rs`: the 23 false-negative
counts are asserted now (all pass). `tests/drc12_corpus.rs` no longer filters the
`lib_footprint_*` codes.

## Package FIX-FAB-2: command line leftovers of the fabrication exports (REV-FAB F-01, F-04)

* `JobOutcome::output_dir` (new, `OutputDir`, not serialised): the directory the C++ job
  creates before writing (`PATHS::EnsurePathExists`, built with
  `OutputDir::ensure_path_exists( outPath, aPathToFile )`): gerbers and drill `outPath`
  (directory), DRC the directory of the report file, SVG the directory (multi) or the file's
  directory (single), fp upgrade the library directory alone (`FP_CACHE::Save`:
  `wxFileName::Mkdir()` without `wxPATH_MKDIR_FULL`, failure "Unable to save library",
  `ERR_UNKNOWN`), pcb upgrade none (the board is saved in place). The host creates exactly this
  directory; any other directory a file needs must exist (F-04: a drill report in a missing
  directory now fails as in C++: the drill files are written, the report is not, exit 4).
  The report path is still recorded by `AddOutput` (only the jobset output list sees it; the
  CLI does not use it).
* `run_export_gerbers` with `--board-plot-params` and a stored `(outputformat 0)` (HPGL, F-01):
  `gerbers_start_plot_fails` ports the layer loop of `JobExportGerbers` for a `StartPlotBoard`
  that returns `nullptr` on every layer: per layer the file name is registered in the job file,
  the `wxLogError` "HPGL plotting is no longer supported as of KiCad 10.0" and
  "Failed to plot to '<file>'." are reported (both `RPT_SEVERITY_ERROR`; the log line is
  formatted like kicad-cli's log target, `HH:MM:SS: Error: ...`, with the time of
  `creation_date`, so that it keeps its place on stderr), the exit code is
  `ERR_INVALID_OUTPUT_CONFLICT` and the `.gbrjob` is written. The case runs in the session
  because kicad-fab's `job_export_gerbers` stops at the first layer it cannot plot.
* Decision (F-01, other stored formats): PostScript, DXF, PDF (and PNG) stored in the board's
  plot parameters stay `NotYetPorted` ("kicad-fab: unsupported: plot format .. is not part of
  the port"), exit code 2 (`NOT_YET_PORTED` = `ERR_UNKNOWN`), no file: the port has no plotter
  for them, and every other out-of-port path of the CLI exits with 2 as well. The reference
  writes that format into the `.gbr` files and exits with 0.

Verification: kicad-cli `tests/fab_matrix.rs` (golden `kicad-cli/tests/golden/fab_matrix/`),
see kicad-cli `PORT_STATUS.md` FIX-FAB-2.

## Package WASM-API-1: board access and editing API for external routers

New API over ported model code (`src/board_api/`), bound in kicad-wasm; reference:
`docs/WASM_API.md`.

| Part | Rust | Calls / C++ origin | State |
|---|---|---|---|
| read | `board_api/read.rs` | `board_info`, `net_details`, `netclasses`, `pads`, `footprints_info`, `route_items`, `route_item`, `zones_info`, `copper_obstacles`, `edge_shapes`, `connectivity_info` (model getters; `GetEffectivePolygon`, `GetCourtyard`, `TransformShapeToPolygon`, `RecalculateRatsnest`, `GetClusters`) | done, oracle-verified |
| rules | `board_api/rules.rs` | `constraint` (`DRC_ENGINE::EvalRules`, dummy items for bare nets as `PNS_PCBNEW_RULE_RESOLVER`), `router_sizes` (`PNS_KICAD_IFACE_BASE::ImportSizes` net class branches, `ROUTER_TOOL` micro via `SetSizeFromRules`) | done, `constraint` oracle-verified |
| edit | `board_api/edit.rs` | `apply_edits` (+ single-edit wrappers), `remove_routing`, `rebuild_connectivity`, teardrop settings / per-item switch, `update_teardrops` (`TEARDROP_MANAGER::UpdateTeardrops` full) | done |
| check | `board_api/check.rs` | `drc_violations` (markers of `run_drc`, `DRC_ITEMS_PROVIDER` severity filter), `quick_drc` (`run_tests_selected`) | done |

Tests (all in the default run; oracle goldens committed):

* `tests/board_api_read.rs`: 15 750 API values of 32 boards against the reference
  `pcb-dump board --loader --hash` (golden `tests/golden/board_api_read.txt`, generator
  `tests/oracle/gen_board_api_golden.sh`): identical. Not compared: `flash.<L>` of layers a
  pad is not on (the API lists only the pad's layers), `item.locked` of items outside the API.
* `tests/board_api_rules.rs`: 530 sampled `EvalRules` results (single items and pairs) of 9
  boards with rule files against `pcb-dump --sections evalrules` (golden
  `tests/golden/board_api_rules.txt`, generator `tests/oracle/gen_board_api_rules_golden.sh`):
  identical except `zc` (`m_ZoneConnection`, not returned); `router_sizes` vs the evaluated
  constraint.
* `tests/board_api_round_trip.rs`: record / remove all routing / re-add with UUIDs / save is
  byte-identical to the plain save on 290 corpus boards (3 known gaps below); DRC JSON
  identical on ecc83 and 20 corpus boards.
* `tests/board_api_edit.rs`: `update_teardrops` against the cached reference
  `drc --refill-zones --save-board` boards (`target/zone-corpus`, skipped without it): the
  teardrop zones of 72 boards identical as saved; `quick_drc` equals the full DRC for its
  providers (3 boards); edit calls, defaults from `router_sizes`, error paths.

Decisions: connectivity is rebuilt in full once per edit call (`BOARD::BuildConnectivity`,
board-model.md 8.5) instead of the incremental `CONNECTIVITY_DATA::Add/Remove/Update` of
`BOARD_COMMIT::Push`; net propagation therefore applies as in KiCad. The dummy items of
constraint queries use a scratch UUID generator (the board's UUID sequence is unaffected).
`TeardropOutcome::without` is computed by the API (no C++ value): an enabled, targeted pad /
via with connected tracks and no zone with `teardropUuid( track, item, 0 )`.

Known gaps:

* Re-adding a removed track does not restore its group / generator membership.
* `NewVia` has no per-layer padstack, backdrill or post-machining (round trip differs on
  `issue24758`, `issue25021/backdrill`, `padstacks`).
* Footprint fields on copper layers are not in `copper_obstacles`.
* Right after `update_teardrops`, `zones_info` returns for some teardrop zones of
  `teardrop_issue_JPC2` / `teardrop_qfn_thermal_pad_zone` (7 of 72 cached cases) outlines that
  differ from the same board saved and re-read; the saved board equals the reference. Not
  investigated (model or writer side).
* `Session::ratsnest` (session.rs, not WASM-API-1's file) still returns `NotYetPorted`; the
  wasm `ratsnest()` now uses `connectivity_info`.
