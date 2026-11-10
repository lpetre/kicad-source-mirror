# kicad-fab port status

Fabrication outputs: board layer plotting, drill files, Gerber job file. Analysis:
`rust/docs/analysis/plot-fab.md` (module layout 10.1, work packages 10.2). The module skeleton
of 10.1 exists; modules of packages that have not started contain only their `//!` header.

| Module | C++ | Package | State |
|---|---|---|---|
| `lib.rs` (`OutputFile`, `FabError`, `Reporter`, `VecReporter`) | `REPORTER::Report` (part) | FAB-01 | done |
| `wx_file_name.rs` | `wxFileName` (Unix: `SplitPath`, `SetName/SetExt/SetPath`, `GetFullName/GetFullPath`) | FAB-01 | done (string part only) |
| `drill/drill_span.rs` | `pcbnew/drill/drill_span.h` | FAB-01 | ported |
| `drill/drill_operation.rs` | `pcbnew/drill/drill_operation.h` | FAB-01 | ported |
| `drill/drill_enumerator.rs` | `pcbnew/drill/drill_enumerator.h/.cpp` | FAB-01, FAB-GLUE | ported; verified end to end (FAB-GLUE) |
| `exporters/gendrill_writer_base.rs` | `gendrill_writer_base.h` (all types), `.cpp` L47-300, L303-355, L358-461, L464-501 | FAB-01 | ported |
| `exporters/gendrill_map.rs` | `gendrill_writer_base.cpp` L506-847, L996-1019 | FAB-12 | ported: Gerber X2 and SVG maps (see "FAB-12" below); PDF / PS / DXF: `FabError::Unsupported` |
| `exporters/gendrill_excellon_writer.rs` | `gendrill_excellon_writer.*` | FAB-02 | ported |
| `exporters/gendrill_report.rs` (`HoleListSource`, `GenDrillReportFile`, `printToolSummary`) | `gendrill_writer_base.cpp` L850-993, L1022-1070 | FAB-02 | ported |
| `jobs/job_export_pcb_drill.rs`, `jobs_handler.rs` (`job_export_drill`) | `job_export_pcb_drill.*` (options), `pcbnew_jobs_handler.cpp` `JobExportDrill` | FAB-02 | ported (Excellon; Gerber branch: FAB-08) |
| `exporters/gendrill_gerber_writer.rs`, the Gerber branch of `jobs_handler::job_export_drill` | `gendrill_gerber_writer.*`, `JobExportDrill` (`DRILL_FORMAT::GERBER`) | FAB-08 | ported (see "FAB-08" below) |
| `exporters/gerber_jobfile_writer.rs` | `gerber_jobfile_writer.*` | FAB-09 | ported (see "FAB-09" below) |
| `pcbplot.rs`, `pcb_plotter.rs` (FAB-03 part), `plot_board_layers.rs` (driver + `PlotStandardLayer` skeleton), `plot_brditems_plotter.rs` (struct, pending entry points) | see "FAB-03" below | FAB-03 | ported (driver); item bodies see FAB-04 and FAB-05/06/10/11/13 |
| `plot_board_layers.rs` (`PlotStandardLayer` L383-930: pads, DNP cross, vias, tracks, zones), `plot_brditems_plotter.rs` (`getColor`, `PlotPadNumber`, `PlotPad`, `PlotZone`) | see "FAB-04" below | FAB-04 | ported |
| `plot_brditems_shapes.rs` (`PlotBoardGraphicItem`, `PlotFootprintGraphicItems`, `PlotShape`, `PlotDimension`, `PlotPcbTarget`, `PlotBarCode`, `PlotTableBorders`), `line_ending_plot.rs` (`LINE_ENDING::Plot`) | see "FAB-05" below | FAB-05 | ported |
| `plot_brditems_text.rs` (`PlotFootprintTextItems`, `PlotText`) | see "FAB-06" below | FAB-06 | ported |
| `jobs/job_export_pcb_plot.rs`, `jobs/job_export_pcb_gerbers.rs`, the Gerber part of `jobs_handler.rs` (`JobExportGerbers`, `PlotJobToPlotOpts`) | see "FAB-07" below | FAB-07 | ported (job file: FAB-09) |
| `plot_layer_poly.rs` (`PlotPolySet`, `PlotSolderMaskLayer` min-width path, `PlotClippedSilkLayer`, `GenerateLayerPoly`) | `plot_board_layers.cpp` L49-125, L1020-1275 | FAB-10 | ported (see "FAB-10" below) |
| `color_settings.rs`, `pcb_plotter.rs` (`plot`), `jobs/job_export_pcb_svg.rs`, the SVG part of `jobs_handler.rs`, `plot_brditems_plotter.rs` (`PlotDrillMarks`, `GetColor`) | see "FAB-11" below | FAB-11 | ported |
| `plot_brditems_plotter.rs` (`PlotDrillSymbols`, `PlotChartSymbols`) | `plot_brditems_plotter.cpp` | FAB-13 | stub |

Drill report (`GenDrillReportFile`, `printToolSummary`, `gendrill_writer_base.cpp` L850-993,
L1022-1070): FAB-02, `exporters/gendrill_report.rs` (an `impl GendrillWriterBase` block).

## FAB-01: drill model, tool list, drill file names

### C++ to Rust

| C++ | Rust |
|---|---|
| `HOLE_ATTRIBUTE`, `DRILL_LAYER_PAIR`, `DRILL_SPAN` (`operator<` / `==`) | `HoleAttribute` (derived `Ord` = declaration order), `DrillLayerPair`, `DrillSpan` (`Ord` = `operator<`, `Eq` on stored members) |
| `DRILL_OP_KIND`, `DRILL_OPERATION_ID`, `DRILL_POST_MACHINING`, `DRILL_OPERATION`, `DRILL_QUERY` | same names in `drill_operation.rs`; `m_SourceItem` -> `source_item: Option<DrillSource>` (`Via(TrackId)` / `Pad(PadId)`) |
| `fromPadstack`, `isBackdrillSet`, `backdrillOperation`, `enumerateVia`, `enumeratePad` | private fns of the same names in `drill_enumerator.rs` |
| `IsDrillSlot`, `EnumerateDrillOperations`, `EnumerateDrillSpans` | `is_drill_slot`, `enumerate_drill_operations`, `enumerate_drill_spans` (`std::set` -> `BTreeSet`) |
| `IPC4761_FEATURES`, `DRILL_TOOL`, `DRILL_PRECISION`, `ZEROS_FMT`, `TYPE_FILE`, `TOOL_SUMMARY`, `USE_ATTRIB_FOR_HOLES` | `Ipc4761Features`, `DrillTool`, `DrillPrecision`, `ZerosFmt`, `TypeFile`, `ToolSummary`, `USE_ATTRIB_FOR_HOLES` |
| `GENDRILL_WRITER_BASE` members, setters / getters | `GendrillWriterBase<'a>` (`pcb: BoardCtx<'a>` = `m_pcb`; derived writers embed it) |
| `cmpHoleSorting`, `buildHolesList` | `cmp_hole_sorting` (with `kicad_geom::stdsort::std_sort`), `build_holes_list` = enumerate + `build_tool_list` (the sort / tool half, public for hand-built lists) |
| `layerName`, `layerPairName` | free fns `layer_name`, `layer_pair_name` (they use no member) |
| `getDrillFileName` (base version), `getProtectionFileName` | `drill_file_name`, `protection_file_name` |
| `CreateMapFilesSet` | `create_map_files_set` (loop and messages ported; the map itself is FAB-12) |
| `BuildFileFunctionAttributeString` | `build_file_function_attribute_string` |
| `diameter_in_inches`, `diameter_in_mm`, `getMarkerBestPenSize`, `getSketchOvalBestPenSize`, `getDefaultPenSize` | same names, snake case |

Not in this tree (named in the work package from older KiCad versions): `getHolesLayerPairs`,
`getUniqueLayerPairs` (replaced by `EnumerateDrillSpans`), Protel-style drill extensions (the
drill writers have none; Protel extensions are a Gerber layer feature, FAB-03), a separate
"micro via" classification (micro / blind / buried vias are distinguished only by their layer
pair, as in C++).

### Model accessors used

* Pad (M14): `Pad::position(ctx)`, `orientation(ctx)`, `drill_size(ctx)`, `has_hole(ctx)`,
  `attribute()`, `property()`, `lib_padstack()` (the C++ `Padstack()`,
  for the drill shape, backdrills and post-machining) are real.
* Via (M17): `PcbVia::drill_value(ctx)`, `layer_pair()`, `resolve_filling_mode(ctx)`,
  `resolve_capping_mode(ctx)`, `resolve_{front,back}_{covering,plugging,tenting}_mode(ctx)`,
  `padstack`, `track.start` (FAB-GLUE switched the enumerator to them and deleted the
  `drill/via_access.rs` placeholders).
* Stackup (M6): `BoardStackup::get_drill_depth`, reached only for via backdrills.
* Board: `tracks()`, `footprints()`, `footprint_pads()`, `copper_layer_count()`,
  `design_settings()`, `file_name` are real.

The enumerator draws no UUID (`DRILL_OPERATION::m_SourceId` is copied from the source item);
its unit test creates its via from the board's `kiid_generator` (review finding R2-11).

### Tests and verification

* `tests/drill_oracle.rs` + `tests/oracle/drill_oracle.cpp` (build and regenerate:
  `tests/oracle/build_oracle.sh`; golden `tests/golden/drill_oracle.txt`): a C++ driver linked
  against the reference pcbnew objects. 14 boards: 4 synthetic ones built in code (2, 4, 6, 32
  copper layers; every via type, backdrills, post-machining, protection modes, pad attributes /
  properties / slots, many coincident holes) and the 10 corpus boards of the drill goldens
  (loaded with `BOARD_LOADER::Load` like kicad-cli). Per span (incl. the NPTH pseudo span) and
  per (merge, npth) combination: the raw enumeration, the sorted list with tool references, the
  tool list, the file-function strings (3 types x NC / Gerber), drill file names (drl / gbr),
  protection file names. The replay feeds the raw list through `build_tool_list` and compares
  everything: **228 cases, all identical**, including the libstdc++ order of tied holes (the
  golden contains cases where `std::sort` differs from a stable sort; a second test asserts
  that so that the test cannot pass with `slice::sort_by`).
* `enumeration_matches_oracle`: enumeration from the corpus boards loaded as kicad-cli loads
  them, against the same raw lists: all identical (FAB-GLUE; the `e2e` feature is gone).
* `tests/drill_golden.rs`: kicad-cli goldens under `tests/golden/drill/<board>/<set>/` for the
  10 corpus boards x 5 option sets (`--format excellon`; `-u in`; `--excellon-mirror-y`;
  `--excellon-separate-th`; `--format gerber`), made by `tests/golden/drill/generate.sh`
  (commands recorded there, run on copies). Checks: golden set consistent; `basic_test` goldens
  equal the repository goldens of `qa/data/cli/basic_test/` after normalisation (all 4 equal);
  normaliser. `drill_export_matches_cli_goldens` is the end-to-end comparison (runs since FAB-GLUE); its
  `run_port` is to be wired by FAB-02 / FAB-08.
* Unit tests (`gendrill_writer_base.rs`, `drill_span.rs`, `drill_enumerator.rs`,
  `wx_file_name.rs`): ordering and merge rules of the tool list, slot / stub / post-machining
  accumulation, the C++ "no tool for a first hole equal to the initial state" corner case,
  naming table (incl. the naming part of `DrillFileLayerOrderInFilename` from
  `qa/tests/pcbnew/test_cam_backdrill.cpp`), protection names, file functions, span order,
  backdrill operation rules, map-file failure message.

### Deviations

* `DRILL_OPERATION::m_SourceId` is default-constructed with a random `KIID` in C++; the port
  uses the nil id (always overwritten by the enumerator; only hand-built lists see it).
* `CreateMapFilesSet` returns `Result<(), FabError>` instead of `bool` (the error says why) and
  collects the files in `GendrillWriterBase::outputs` instead of writing them.
* `wxFileName::SetPath` normalisation of the directory (`a//b` -> `a/b`) is not reproduced;
  only trailing separators are dropped.

### Known gaps

* Drill maps (`genDrillMapFile`, `plotDrillMarks`): FAB-12.

## FAB-02: Excellon writer, drill report, drill job

### C++ to Rust

| C++ | Rust |
|---|---|
| `EXCELLON_WRITER` (ctor, `SetFormat`, `SetOptions`, `SetRouteModeForOvalHoles`, `GetOffset`) | `ExcellonWriter<'a>` (`base: GendrillWriterBase`), same-named snake-case methods |
| `CreateDrillandMapFilesSet` | `create_drill_and_map_files_set` (board), `create_drill_and_map_files_set_from(source, ...)` |
| `createDrillFile`, `writeEXCELLONHeader`, `writeEXCELLONEndOfFile`, `writeHoleAttribute` | private fns of the same names (the file is a `String`; `excellon_header_text` exposes the header for tests) |
| `writeCoordinates` | `write_coordinates(x, y) -> String` (the line incl. `\n`) |
| `writeHoleComments`, `writePostMachiningComment`, `formatLinearValue` | `hole_comments`, `post_machining_comment` (return the text), `format_linear_value` |
| `getBackdrillLayerPairFileName`, `writeBackdrillLayerPairFile` | `backdrill_layer_pair_file_name`, `write_backdrill_layer_pair_file` |
| `GENDRILL_WRITER_BASE::GenDrillReportFile`, `printToolSummary` | `GendrillWriterBase::gen_drill_report_file(path, date, drill_file_name_fn, source, reporter)`, `print_tool_summary`; `ExcellonWriter::gen_drill_report_file[_from]` |
| `EnumerateDrillSpans` + `buildHolesList` as called by the writers / report | trait `HoleListSource` (`BoardHoleSource` = the C++ calls; tests feed recorded C++ enumerations) |
| fmt `{:<25}` (display width), `{:2.3f}` | `fmt_left_aligned` (ports fmt's `display_width_of`), `fmt_fixed_width` (`kicad_core::printf::format_f`) |
| `JOB_EXPORT_PCB_DRILL` (members, enums, defaults) | `jobs::job_export_pcb_drill::JobExportPcbDrill` (options only, see below), `DrillExportEnv` (build version, date) |
| `PCBNEW_JOBS_HANDLER::JobExportDrill` (after `getBoard` / `resolveJobOutputPath`) | `jobs_handler::job_export_drill[_from]` -> `DrillJobOutcome { success, error, files, outputs }`; `default_drill_report_path`; `precisionListFor{Inches,Metric}` |

`kicad-session` already has the full job struct (`JOB` base, JSON); it depends on this crate, so
this crate defines an options-only copy (same members and constructor defaults) that the session
maps onto (see the change request in the FAB-02 result).

### Tests and verification

* `tests/excellon_oracle.rs` + `tests/oracle/excellon_oracle.cpp` (build and regenerate:
  `tests/oracle/build_excellon_oracle.sh`, about 1 minute; golden `tests/golden/excellon_oracle.txt`,
  320 KB). The C++ driver links the reference pcbnew objects, pins the clock (it defines
  `time()`, `TZ=XST-5:30`) and calls the real `EXCELLON_WRITER` (private helpers reached with
  `#define private public`). It covers 21 boards (the 4 synthetic and 10 corpus boards of
  FAB-01, the 6 boards of `test_cam_backdrill.cpp` that use the Excellon writer or the report,
  and a slots board with odd angles / sizes, user and CJK layer names, aux origin) x 12 option
  sets (every zero format in mm and inch, mirror, minimal header, merged / separate, G85 /
  routed slots, aux-origin offset), plus the report merged and separate. The replay feeds the
  C++ enumeration (FAB-01 `raw` lists, or this golden's for its own boards; `rawhash` proves
  they are the oracle's) through `job_export_drill_from` and compares every reporter message,
  `GetCreatedFiles` order and file: **966 drill files (FNV-1a hash, full text for the CLI
  default set) and 42 reports, all identical**. Plus `writeCoordinates` for 53 value pairs x
  4 zero formats x 5 precisions x 2 units (2120 lines, incl. rounding halves, negative zero,
  int overflow clamping), 96 post-machining comments, 18 `formatLinearValue`, 48 headers: all
  identical.
* `basic_test_goldens_from_hand_built_holes`: `qa/data/cli/basic_test/basic_test_excellon_{default,
  inches,mirror}.drl` reproduced from a hand-built 4-via list, byte for byte below the 5 lines
  `test_pcb.py` skips, and fully (normalised) against the kicad-cli goldens.
* `cli_goldens_from_oracle_hole_lists`: the 4 Excellon option sets of `tests/golden/drill/` for
  all 10 corpus boards (kicad-cli output), file names, order and contents, all identical.
* `cam_backdrill_expectations`: the Excellon / report assertions of `BackdrillCamOutputs`,
  `FrontAndBackBackdrillCamOutputs`, `DualBackdrillSameViaCamOutputs`,
  `DrillFileLayerOrderInFilename`, `DrillReportWithTools`, `DrillReportNullReporter`,
  `DrillReportBackdrillHoleCount` (on the oracle's enumeration of the same boards; the C++
  tests build them through the via model, M17).
* `drill_job_outputs_and_pending_parts`: job output order (drill files, then the report),
  report path default / explicit / resolver hook, map and Gerber failures.
* `tests/drill_golden.rs::drill_export_matches_cli_goldens`: board loaded as kicad-cli does,
  CLI defaults, job: all 10 boards x 5 sets identical (FAB-GLUE; see also
  `tests/excellon_corpus.rs` there).
* Unit tests in `gendrill_excellon_writer.rs`, `gendrill_report.rs`, `jobs_handler.rs`.

### Deviations

* No file system: `wxFopen` / `fmt::print` failures ("Failed to create file", "Failed to write
  file", "Error creating / writing drill report file") cannot occur; files go to
  `base.outputs`. `GenDrillReportFile` always returns `true`.
* `REPORTER::ReportTail( "Done." )` is reported with `Reporter::report` (the trait has no tail).
* `CreateDrillandMapFilesSet` returns `Result<(), FabError>` instead of `bool`.
* The drill map part of `create_drill_and_map_files_set_from` still enumerates the board
  (`create_map_files_set`), not the given `HoleListSource`.
* The job function starts after `getBoard` / `resolveJobOutputPath` / `EnsurePathExists`
  (host side) and returns `DrillJobOutcome` instead of an exit code
  (`success == false` <=> `ERR_INVALID_OUTPUT_CONFLICT`).

### Known gaps

* (Done, FAB-12) `--generate-map`: Gerber X2 / SVG maps; see "FAB-12" below.

## FAB-03: board plot driver, Gerber X2 header, file names

### C++ to Rust

| C++ | Rust |
|---|---|
| `GetGerberProtelExtension`, `GetGerberFileFunctionAttribute`, `GetGerberFilePolarityAttribute`, `makeStringCompatX1`, `replaceReservedCharsField` | `pcbplot::gerber_protel_extension`, `gerber_file_function_attribute`, `gerber_file_polarity_attribute` (pub), private `make_string_compat_x1`, `replace_reserved_chars_field` |
| `AddGerberX2Header`, `AddGerberX2Attribute` | `pcbplot::add_gerber_x2_header(plotter, board, env, x1)`, `add_gerber_x2_attribute(plotter, board, layer, env, x1)` |
| `GetBuildVersion()`, `wxDateTime::Now()` | `pcbplot::PlotEnv { build_version, creation_date }` (caller supplied) |
| `PLOT_MIN_SCALE`, `PLOT_MAX_SCALE` | same names in `pcbplot` |
| `PCB_PLOTTER::BuildPlotFileName`, `getPlotSequence`, `copperLayerShouldBeSkipped` | `pcb_plotter::build_plot_file_name`, `get_plot_sequence`, `copper_layer_should_be_skipped` |
| `StartPlotBoard`, `initializePlotter`, `FillNegativeKnockout` | `plot_board_layers::start_plot_board -> Result<Box<dyn Plotter>, FabError>`, private `initialize_plotter`, `fill_negative_knockout` |
| `PlotBoardLayers`, `PlotOneBoardLayer`, `PlotLayer`, `PlotSolderMaskLayer` (min width 0 branch) | `plot_board_layers::plot_board_layers`, `plot_one_board_layer`, private `plot_layer`, `plot_solder_mask_layer` |
| `PlotStandardLayer` (L357-931) | `plot_board_layers::plot_standard_layer`: preamble flags (`StandardLayerFlags`), container order, `StartBlock` / `EndBlock`, the via / track `GBR_METADATA` set-up are ported; the loop bodies are the pending `plot_standard_layer_footprint` / `_via` / `_track` / `_zone` (FAB-04) |
| `BRDITEMS_PLOTTER` (struct, `getFineWidthAdj`, `SetLayerSet`, `hideDNPItems`, `crossoutDNPItems`) | `plot_brditems_plotter::BrditemsPlotter { opts, plotter, ctx, layer_mask }`; every other member is a pending method naming its package (`fab_port_pending!`) |
| `BOARD::DrillSymbolLayers()` | private `drill_symbol_layers` (computed from the drill map items; the model has no `m_drillSymbolLayers`) |
| per-layer body of `JobExportGerbers` (name, `StartPlotBoard`, `PlotBoardLayers`, `EndPlot`) | `plot_board_layers::plot_gerber_layer_file -> OutputFile` (the end-to-end entry point until FAB-07) |
| `TransformRoundChamferedRectToPolygon` as plotter callback | `plot_board_layers::round_chamfered_rect_to_polygon` (installed into the SVG plotter; FAB-04 passes it to `flash_pad_chamfer_round_rect`) |

### Tests and verification

* `tests/plot_driver.rs::crafted_boards_match_cli_goldens`: 4 hand-written boards
  (`tests/data/plot_driver`, re-saved with `kicad-cli pcb upgrade --force`: 4 copper layers
  with renamed layers, a revision with reserved characters and an aux origin used by the
  stored options; 2 layers with a `,`/`%`/non-ASCII file name, a `${REV}` revision from the
  `.kicad_pro`, a negative aux origin, stored `psnegative`; 6 layers with stored X1,
  precision 5, A4 output, negative, macros off; 32 copper layers) x 7 option sets of
  `kicad-cli pcb export gerbers` (defaults, `--no-x2`, `--precision 5`, `--no-protel-ext`,
  `--use-drill-file-origin`, `--board-plot-params`, `--cl Edge.Cuts,F.Fab`). Golden
  `tests/golden/plot_driver/crafted.txt` (`generate.py crafted`). **247 files, all
  byte-identical** after normalising the version / date lines, including file names and order
  (default set), X1 comments, `Fmt 4.5`, `TF.SameCoordinates,PXff676980PYfec731e0`,
  `TF.ProjectId` escaping / GUID, and the negative knockout regions (`--board-plot-params`
  with `psnegative`, incl. the A4 auto-centre offset).
* `tests/plot_driver.rs::svg_start_plot_board_matches_cli`: SVG `StartPlotBoard` +
  `PlotBoardLayers` + `EndPlot` on an empty layer against `kicad-cli pcb export svg
  --mode-single --exclude-drawing-sheet --drill-shape-opt 0` (black and white; colour +
  negative + mirror): identical except the `<title>` date.
* `tests/plot_driver_corpus.rs::corpus_replay`: all 380 boards under `qa/data` and `demos`
  below 300 KB, CLI defaults, compact golden `tests/golden/plot_driver/corpus.txt` (8533
  files: name / header / file hash, `generate.py corpus`). Today: 136 boards load in the
  port (243 are rejected by the parser / reach model stubs, 1 produces no kicad-cli output);
  of their 2840 files **2840 names and 2840 headers match, 874 whole files match** (the
  layers with nothing on them), 1966 are pending (items: FAB-04/05/06), 0 differ. Pending
  parts are counted, not failed, so the test tightens automatically;
  `KICAD_FAB_CORPUS_REPORT=1` prints the per-board result.
* Unit tests: Protel extensions, polarity, X1 conversion, reserved characters, `ToAscii`,
  `BuildPlotFileName` (trim, forbidden characters, empty suffix, ASCII-only trim),
  `getPlotSequence`.

### Deviations

* `StartPlotBoard` returns `Err` instead of `nullptr`: DXF / PostScript / PDF / PNG plotters
  are out of scope (`FabError::Unsupported`), HPGL keeps the C++ message; a requested drawing
  sheet (`plotframeref`) is `FabError::NotPorted` (PLOT-07/08) instead of being plotted.
* The `wxLogError` for an invalid Gerber layer is not emitted (no log); the header is written
  as in C++.
* `PCB_RENDER_SETTINGS::LoadColors` / `SetLayerName` are not applied (colour themes: FAB-11;
  layer name: drawing sheet only).
* `TF.ProjectId` revision: `ExpandTextVars( rev, project )` uses a private copy of the model's
  `PROJECT::TextVarResolver` (change request to expose it); date / time / VCS variables
  resolve to empty strings as in the model.

### Known gaps

* Item plotting (`PlotStandardLayer` bodies, `BRDITEMS_PLOTTER` members): FAB-04, 05, 06;
  solder mask minimum width and clipped silk: FAB-10; drill marks: FAB-11; drill symbols:
  FAB-13. Any board with drawings, footprints, tracks or zones on a plotted layer panics with
  a "port pending" message there.
* `initializePlotter` with autoscale / scale != 1 and a non-empty bounding box, and SVG
  fit-to-board (`PCB_PLOTTER::Plot`, FAB-11), cannot be compared yet (needs item plotting).
* `GerberPlotter::set_viewport` has `debug_assert!(!a_mirror)` (C++ `wxASSERT`, which only
  logs): a board with stored `(mirror yes)` plotted with `--board-plot-params` panics in debug
  builds (change request to kicad-plot); the crafted board therefore stores `mirror no`.

## FAB-04: standard layer items (pads, vias, tracks, zones)

### C++ to Rust

| C++ | Rust |
|---|---|
| `PlotStandardLayer` preamble (L361-371) | `StandardLayerFlags::new` |
| `PlotStandardLayer` L383-930 (footprint pad blocks, via block, track block, zone block) | `plot_standard_layer_pads_vias_tracks_zones` (called by `plot_standard_layer` after the FAB-05 / FAB-06 loops; public for the verification below) |
| footprint loop body L387-740 (sketch pads, colour, `PlotPadNumber`, DNP cross) | `plot_standard_layer_footprint` |
| `plotPadLayer` lambda (L446-681) | private `plot_pad_layer` |
| via / track / zone loop bodies, `getMetadata` lambda | `plot_standard_layer_via`, `plot_standard_layer_track`, `plot_standard_layer_zone`, private `plot_metadata` |
| `PAD::DeletePrimitivesList`, `PAD::AddPrimitivePoly( SHAPE_POLY_SET )` + `AddPrimitive` + `rebakePrimitiveToFootprint` | private `pad_delete_primitives_list`, `pad_add_primitive_poly` (local ports through `Pad::set_padstack`: the model has no mutable primitive access, change request) |
| `BRDITEMS_PLOTTER::getColor`, `PlotPadNumber`, `PlotPad`, `PlotZone` | `BrditemsPlotter::get_color`, `plot_pad_number`, `plot_pad`, `plot_zone` |
| `PCB_PLOT_PARAMS::ColorSettings()->GetColor( layer )` | `BrditemsPlotter::color_settings_color` (see deviations) |

`PAD dummy( *pad )` and the in-place "modify the board pad, plot, restore" of `plotPadLayer`
are both a `detached_copy()` of the pad viewed with `ctx.view` (`board-model.md` 13); the
restore sequence is applied and its result kept in the job's `PadPlotState` (FIX-FAB-1). Setters
take the pad's `board.mut_env(pad)` (its footprint frame). `ZONE dummy( *zone ); SetNet( &nonet
)` is a detached copy with net code -1 (no board net carries it: empty net name).

### Tests and verification

* `tests/plot_items_corpus.rs::corpus_items_replay`: the FAB-04 part on every layer of the
  loadable corpus boards (`tests/golden/plot_driver/corpus.txt`, reference kicad-cli, CLI
  defaults) that carries no FAB-05 / FAB-06 content (no board drawing, no visible footprint
  field, no footprint graphic on the layer; no drill map; no mask minimum width): the driver
  of `plot_gerber_layer_file` around `plot_standard_layer_pads_vias_tracks_zones`, whole file
  compared. Today **3118 files of 157 boards, all byte-identical** (205 layers skipped for
  graphics, 4 for the mask minimum width). The loadable boards have no footprints yet (the pad
  parser PCBIO-07 is pending: 222 corpus boards fail on it): this covers 202 vias (copper and
  mask layers, flashing through connectivity), 111 track segments, 49 arcs and 117 zones
  (Gerber regions with net attributes).
* `crafted_items_replay`: two crafted boards (`tests/data/plot_items`, re-saved with
  `kicad-cli pcb upgrade --force`; goldens `tests/golden/plot_items/crafted.txt` +
  `crafted_files.txt`, `generate.py`): `pads.kicad_pcb` (4 copper layers, 20 pads in two
  footprints at 0 and 90 degrees: every pad shape, PTH / SMD / CONN / NPTH incl. the
  "size == drill" NPTH skip, every fabrication property, pin function, local mask margins
  (rect -> round rect, trapezoid and chamfered rect -> inflated custom copy), paste margin
  and ratio (asymmetric round rect, negative custom), remove-unused-layers) with the CLI
  defaults, and `bp_fab.kicad_pcb` with `--board-plot-params` (sketch pads and pad numbers on
  F.Fab / B.Fab). **26 files, all byte-identical** (copper, mask, paste, adhesive, fab). The
  footprints are built through the model API by `tests/plot_items_fixture` from the same file
  text while PCBIO-07 is pending (the rest of the board goes through the real parser).
* `svg_items_match_cli`: the same through the SVG engine (black and white; F.Cu, F.Mask
  mirrored, F.Paste) against `kicad-cli pcb export svg --mode-single ... --black-and-white`:
  identical except the `<title>` date.
* `tests/plot_pads_qa.rs`: ports of `NoDrillFlashesOnEdgeCuts`,
  `NoDrillMarksOnGerberFromStoredOptions` (`test_plot_edge_cuts_drill_marks.cpp`) and
  `RoundRectGerberMaskApertureHasCorrectRadius` (`test_plot_pad_mask.cpp`): pass on the FAB-04
  part; the literal C++ call (`PlotBoardLayers` / `PlotStandardLayer`) is reported pending
  until FAB-06 (`PlotFootprintTextItems`) lands, then it is asserted too.
* `tests/plot_driver_corpus.rs` now builds connectivity (`BOARD::BuildConnectivity`, as
  `BOARD_LOADER` does) and passes it in the `BoardCtx`: 1202 whole files match, 0 differ.

Not verified yet: the DNP cross (needs `FOOTPRINT::GetCourtyard` / `GetEffectiveShape`, M19
stubs, and a board option), back-side and non-orthogonal footprints (the fixture builds
front footprints only; they are covered automatically by the corpus tests once PCBIO-07
lands), rule-driven mask / paste margins (the tests pass no `RuleResolver`: there is no DRC
engine yet; the C++ `!m_DRCEngine` branches give the same values for boards without
custom rules).

### Deviations

* Colour themes (`COLOR_SETTINGS`, FAB-11) are not ported: `color_settings_color` returns
  BLACK for Gerber and for black and white plots, and is a pending panic for colour plots.
  Gerber ignores colours and `PSLIKE_PLOTTER::SetColor` in black and white only
  distinguishes WHITE from the rest, so the output is identical unless a theme colour is
  exactly WHITE where the C++ does not map it to LIGHTGRAY (pad number text, DNP cross).
* `MarkFootprintPlotted` (PDF bookmarks) is not called (PDF is out of scope).
* DXF branches (`getMetadata` returning the plot options, `PlotZone` DXF fill mode) are out of
  scope: no metadata / pending panic.
* The local `pad_add_primitive_poly` gives the new primitives `ParentRef::None` (C++: the
  copy's address); pad primitives are never transformed by the footprint frame and the
  board's `maxError` comes from the pad's environment, so the shapes are the same.

### Known gaps

* `PlotStandardLayer` as a whole still reaches the FAB-05 / FAB-06 stubs for any board with
  drawings or footprints.
* Change request: `Pad::delete_primitives_list` / `Pad::add_primitive_poly` in the model
  (then the two local ports go).

## FAB-05: graphic items (shapes, dimensions, targets, tables, barcodes, footprint graphics)

### C++ to Rust

| C++ (`pcbnew/plot_brditems_plotter.cpp`) | Rust |
|---|---|
| `PlotBoardGraphicItem` (L489-566) | `BrditemsPlotter::plot_board_graphic_item` (`plot_brditems_shapes.rs`; dispatch on `Type()`) |
| `PlotDimension` (L569-621) | `plot_dimension` (one reused parentless `PcbShape` temp item, as in C++) |
| `PlotPcbTarget` (L624-675) | `plot_pcb_target` |
| `PlotFootprintGraphicItems` (L678-766) | `plot_footprint_graphic_items` (private layers, hidden DNP, layer mask) |
| `plotMetadata` / `getMetadata()` (L769-783) | private `get_metadata` (Gerber: the metadata, SVG: none) |
| `PlotShape` (L960-1350) incl. the `plotStrokeOutline` lambda | `plot_shape`, private `plot_stroke_outline` |
| `PlotBarCode` (L1353-1370) | `plot_bar_code` |
| `PlotTableBorders` (L1373-1406) | `plot_table_borders` (through `PcbTable::draw_borders`) |
| table cell loop `PlotText( cell, ... )` | private `plot_table_cells` (calls the FAB-06 `plot_text`) |
| `LINE_ENDING::Plot` (`common/line_ending.cpp` L240-265) | `line_ending_plot::line_ending_plot` (`GetShapes` is the model's `LineEnding::shapes`) |
| `STROKE_PARAMS::Stroke( ..., m_plotter->RenderSettings(), ... )` | `kicad_pcb::stroke_params::stroke` with `StrokePatternLengths` from the plotter's `RenderSettings` (`dash_length` / `dot_length` / `gap_length` of `GetWidth()`) |

Model calls used: `EdaShape::shorten_segment_for_endings`, `shorten_arc_for_endings`,
`shortened_bezier_curve`, `shortened_body_poly_points`, `ending_tangents`,
`line_ending_endpoints`, `calc_arc_angles`, `build_shape_ellipse`;
`eda_shape::shapes::make_effective_shapes_for_stroking`, `shortened_bezier_polyline`,
`eda_shape::hatching_data` (`GetHatching()`); `PcbShape::has_solder_mask`,
`solder_mask_expansion`; `PcbTextbox::polygonal_box_shape`; `PcbDimension::shapes`;
`BoardItem::transform_shape_to_poly_set` (barcode). `PlotLayerOutlines` (`plot_board_layers.cpp`
L937-1017) is used for DXF only (`PlotLayer`), out of scope like DXF.

### Tests and verification

* `tests/plot_graphics_corpus.rs::corpus_graphics_replay`: the whole `PlotStandardLayer`
  sequence (board graphics, footprint graphics, then the FAB-04 part) inside the CLI driver,
  whole file compared against the reference kicad-cli corpus golden
  (`tests/golden/plot_driver/corpus.txt`, CLI defaults), on all 379 loadable corpus boards
  below 300 KB. The footprint text loop (FAB-06) is left out on layers without visible
  footprint fields / footprint texts / footprint dimensions; board texts, text boxes and
  tables on *other* layers are left out (the pending `PlotText` returns at once there).
  Today: **7891 files byte-identical** (with 5139 board drawings / footprint graphic items on
  their layers: Edge.Cuts, silk, fab, courtyard, user layers, copper graphics), 549 pending
  (402 footprint text loop, 147 `PlotText`: FAB-06), 3 known differences (model, below),
  skipped: 26 drill-map layers (FAB-13), 62 mask layers with a minimum width (FAB-10), 2 layers
  with a barcode (no symbol encoder in the model, M25a). Pending files are counted, not
  failed, so the test tightens when FAB-06 lands.
* `crafted_graphics_replay`: crafted boards `tests/data/plot_graphics` (hand-written,
  re-saved with `kicad-cli pcb upgrade --force`; goldens `tests/golden/plot_graphics/
  crafted.txt` + `crafted_files.txt`, `generate.py`): `graphics.kicad_pcb` (every `SHAPE_T`
  solid / dashed / dotted / dash-dot / dash-dot-dot, filled, hatched and cross-hatched,
  rounded rectangles, zero-width filled shapes, Beziers, ellipses and ellipse arcs, every
  line-ending style incl. explicit sizes and own stroke on segments / arcs / polylines /
  Beziers, copper graphics with and without net, shapes with solder mask and positive /
  negative margins on F.Mask / B.Mask, targets + and X, a rotated front footprint and a back
  footprint with graphics on silk / fab / courtyard / copper / Edge.Cuts) with the CLI
  defaults, `bp_dnp_sketch.kicad_pcb` and `bp_dnp_hide.kicad_pcb` with `--board-plot-params`
  (DNP footprints on fab layers: sketch, hide). **45 files, all byte-identical**; the 31
  files of the FAB-04 crafted boards: 28 identical, 3 pending (visible footprint fields).
* `svg_graphics_match_cli`: `graphics.kicad_pcb` F.Silkscreen, User.Drawings mirrored and
  F.Mask through the SVG engine (native Bezier curves) against `kicad-cli pcb export svg
  --mode-single --exclude-drawing-sheet --drill-shape-opt 0 --black-and-white`
  (`tests/golden/plot_graphics/svg_*.svg`): identical except the `<title>` date.

Not verified yet (no reachable data until FAB-06 / the model gaps close): dimensions, text
boxes with borders and tables (they all plot text first), barcodes (empty polygon), drill
charts (FAB-13), shapes with a DRC rule for the mask expansion (no rule resolver in the
tests).

### Deviations

* `MarkFootprintPlotted` (PDF bookmarks) is not called (PDF is out of scope).
* DXF branches (`ThickPoly` / `ThickRect` in sketch mode, the unfractured DXF fill) are out
  of scope: `plot_shape` reports a pending DXF panic; `StartPlotBoard` never creates a DXF
  plotter.
* `UNIMPLEMENTED_FOR` (unknown shape / item type) only logs in C++; the port does nothing.

### Known gaps

* `PlotChartSymbols` FAB-13 (`PlotText` / `PlotFootprintTextItems`: FAB-06, done).
* (Resolved, FAB-GLUE) The model differences the corpus replay found (pad local mask margin
  on B.Mask, barcodes without the zint encoder) are fixed in the model: `KNOWN_DIFFERENCES`
  is empty and the barcode layers are compared.

## FAB-06: text items (footprint text rules, PlotText)

### C++ to Rust

| C++ (`pcbnew/plot_brditems_plotter.cpp`) | Rust (`plot_brditems_text.rs`, an `impl BrditemsPlotter` block) |
|---|---|
| `PlotFootprintTextItems` (L401-486) | `plot_footprint_text_items`: reference / value with their plot options and visibility, DNP hide / strike-out, other visible fields, then every `PCB_TEXT` of `GraphicalItems()` (text and, through the `PCB_TEXT` base, dimensions); Edge.Cuts / private layers / `${REFERENCE}` / `${VALUE}` rules |
| `PlotText` (L786-899) | `plot_text( item, layer, knockout, &Metrics, strikeout )`: `GetDrawFont( RenderSettings )`, empty / layer checks, `NonConductor` on copper, effective pen width, draw rotation, knockout polygons (`TransformTextToPolySet` of text / field / text box / cell, `Fracture`), outline font from the render cache, multi-line through `GetLinePositions`, single line at `GetTextPos()` |
| `strikeoutText` lambda | private `strikeout_text` |
| `CALLBACK_GAL( stroke, polygon )` + `DrawGlyphs( *GetRenderCache() )` | `kicad_font::CallbackGal` recording the calls (`GlyphOp`), replayed in order onto `ThickSegment` / `PlotPoly`; outline glyphs fractured if they have holes as `CALLBACK_GAL::DrawGlyph` does |
| `getMetadata()` | private `get_metadata` (Gerber: the metadata, SVG: none) |

The engine side (`PLOTTER::PlotText`, stroke font) is kicad-plot / kicad-font. The text
model calls (`GetShownText`, `GetTextPos`, `GetDrawPos`, `GetDrawRotation`,
`GetEffectiveTextPenWidth`, `GetLinePositions`, `GetRenderCache`, `TransformTextToPolySet`)
are the kicad-pcb ones.

### Tests and verification

* `tests/plot_text.rs::crafted_text_matches_cli`: `tests/data/plot_text/text.kicad_pcb`
  (hand-written, re-saved with `kicad-cli pcb upgrade --force`; goldens
  `tests/golden/plot_text/crafted.txt` + `crafted_files.txt` (default set), `generate.py`)
  through `plot_gerber_layer_file`, 15 layers x 5 option sets (CLI defaults, `--erd`, `--ev`,
  `--hdnp`, `--cdnp`). Content: four footprints (front, rotated 33 degrees and scaled 1.5,
  back / mirrored, DNP) with visible / hidden / empty fields, bold and italic fields,
  `${REFERENCE}` / `${VALUE}` texts, footprint text on Edge.Cuts (skipped), knockout and
  copper footprint text, a keep-upright text at 200 degrees, a multi-line footprint text;
  board texts multi-line with left/bottom and right/top justification (incl. an empty line),
  rotated, mirrored, bold italic, auto thickness with non-square size, knockout, `${LAYER}`,
  overbar / subscript / superscript markup, copper text; text boxes: empty knockout,
  rotated multi-line with border, knockout on copper. **65 files byte-identical**, 10 known
  differences (`User.2`, `User.3` in every set: outline fonts, below).
* `tests/plot_graphics_corpus.rs`: the FAB-05 skips for text are gone; every layer runs the
  full `PlotStandardLayer` sequence. Corpus: **8438 files byte-identical** (439 of them with
  footprint text), 0 pending, 5 known differences (3 pad mask margin, 2 footprint table
  cells, model), skipped 26 drill-map (FAB-13), 62 mask min width (FAB-10), 2 barcode
  layers. Crafted FAB-04 / FAB-05 boards: 31 + 45 files, all identical.
* `tests/plot_driver_corpus.rs` (whole `plot_gerber_layer_file`): **8438 of 8533 files
  identical**, 88 pending (drill maps FAB-13, mask min width FAB-10), 7 known differences
  (`KNOWN_DIFFERENCES` there: the 5 above and the 2 barcode layers), 0 failures.
* `tests/plot_pads_qa.rs`: the literal C++ calls now run to the end and are asserted.
* Ports of `ScaledFootprintReferencePlotsScaled`, `ScaledFootprintGraphicTextPlotsScaled`
  (`test_plot_footprint_scaled_text.cpp`) and `EmptyKnockoutPlotsTheBox`
  (`test_pcb_textbox.cpp`) in `tests/plot_text.rs`: the footprints / box come from board
  text instead of the model setters (`(transform (scale 2 2))` = `SetTransformScale`); the
  knockout box is read back from the Gerber regions (mm, 6 decimals = IU) instead of a
  recording plotter. All pass.

Not verified against the reference: outline-font text drawn from a valid render cache (the
loader drops footprint text caches, see below; the code path is the C++ one), knockout or
struck-out dimension text in a footprint (model accessor missing, see below), SVG output of
text (the engine is kicad-plot's; not compared here).

### Deviations

* Outline fonts: without an outline font engine, text whose draw font is an outline font
  is drawn from the render cache only when `GetRenderCache` finds it valid; a stale or
  missing cache (C++ regenerates it with FreeType) and fonts embedded in the board (C++ lays
  out with the outline font) fall back to the stroke font. Board-level texts always lose
  their cache at load (`EDA_TEXT::SetVisible`), so board text in an outline font always
  differs from the reference (crafted `User.2`).
* `MarkFootprintPlotted` (PDF bookmarks) is not called (PDF is out of scope).
* A footprint without a Reference / Value field (impossible in C++, which always has them)
  plots no reference / value.

### Known gaps / change requests

* Knockout or strike-out of a footprint dimension's text reaches a pending panic in
  `plot_brditems_text.rs` (`pcb_text_transform_text_to_poly_set`). The model now has
  `PcbDimension::transform_text_to_poly_set`; switching the call is a change request of
  FAB-GLUE (that file belongs to FONT-RC-1 while it runs).
* The model loader drops the render cache of footprint texts in an outline font (crafted
  `User.3`: C++ plots the cached glyphs, the port gets no cache).
* (Resolved in the model, FAB-GLUE) `embedded_table_rotated_legacy_v10.kicad_pcb` F.Cu /
  B.Cu (footprint table cells) match now.

## FAB-07: the `gerbers` job

### C++ to Rust

| C++ | Rust |
|---|---|
| `JOB_EXPORT_PCB_PLOT` (members, `PLOT_FORMAT`, defaults, `ResolvePlotLayers`) | `jobs::job_export_pcb_plot::{JobExportPcbPlot, JobPlotFormat}` (options only; `configured_output_path`, `var_overrides` stand for the `JOB` base) |
| `JOB_EXPORT_PCB_GERBER`, `JOB_EXPORT_PCB_GERBERS` (members, defaults) | `jobs::job_export_pcb_gerbers::{JobExportPcbGerber, JobExportPcbGerbers}` |
| `PCB_PLOTTER::PlotJobToPlotOpts`, `scaleToSelection` (`pcb_plotter.cpp` L47-57, L426-555) | `jobs_handler::plot_job_to_plot_opts(opts, job, PlotJobFormatOptions, reporter)`, `scale_to_selection` |
| `PCBNEW_JOBS_HANDLER::JobExportGerbers` L1832-1920 (per-layer plot sequence, plot options, extension, `BuildPlotFileName`, `AddGbrFile`, `LAYER` / `SHEETNAME` / `SHEETPATH` overrides, `StartPlotBoard`, `Plotted to '%s'.`, `PlotBoardLayers`, `EndPlot`, `AddOutput`, job file name) | `jobs_handler::job_export_gerbers -> GerbersJobOutcome { success, error, files, outputs, gbr_files, job_file_path }` |
| `JobExportGerbers` L1750-1831 (board, variant, output path, zone refill, `convertLayerArg`, layer selection, `RefreshGeneratedTables`) | `kicad_session::jobs_handler::run_export_gerbers` (the session owns the raw layer arguments; `convert_layer_arg` / `gerbers_layers_to_plot` were ported and oracle-verified there before), which maps its job onto the structs above and calls `job_export_gerbers` |

### Tests and verification

All in `kicad-session/tests/gerbers_job.rs` (the whole `kicad-rs pcb export gerbers` path:
session load, layer selection, this package, kicad-fab plotting), against the reference
`kicad-cli pcb export gerbers`, normalising only the version / date lines:

* `crafted_boards`: the 4 crafted boards of `tests/data/plot_driver` x the 7 option sets of
  `tests/golden/plot_driver/generate.py` (defaults, `--no-x2`, `--precision 5`,
  `--no-protel-ext`, `--use-drill-file-origin` with `-l`, `--board-plot-params`,
  `-l F.Cu,B.Cu --cl Edge.Cuts,F.Fab`): **247 files, all byte-identical**, same names, same
  order, same `Plotted to` messages, exit code 0.
* `cli_test_pcb_export_gerber`: the `qa/tests/cli/test_pcb.py::test_pcb_export_gerber` cases
  (`--no-x2 --use-drill-file-origin --layers L`): `Issue24143` F.Cu byte-identical to the
  committed golden `Issue24143-F_Cu.gbr`; `ZoneFill-4.0.7` F.Cu / B.Cu byte-identical to the
  reference kicad-cli output (hash; the committed `.gbr` goldens are from an older KiCad and
  only pass the C++ test's image diff). `ZoneFill-Legacy.brd` needs the legacy board reader
  (not in the port).
* `corpus_replay`: every corpus board under 300 KB (380), CLI defaults and
  `--no-x2 --use-drill-file-origin` (golden `kicad-session/tests/golden/gerbers_job/corpus.txt`,
  `generate.py` there: exit code, stdout hash, file hashes). Today: **690 of 760 runs
  identical** (exit code, messages, every file: 15 506 files), 2 runs not loadable
  (`fakeboard`, rejected by kicad-cli too), 62 runs reach the FAB-10 stub (mask minimum
  width), 2 reach `RefreshGeneratedTables` (M26), 4 files are known differences (barcodes
  without symbol encoder, model M25a: `project_barcode` F.Silkscreen, `issue24525` Edge.Cuts).
  Note: the three B.Mask files listed as known differences in `plot_driver_corpus.rs`
  (`round_unequal_drill`, `issue7086`, `solder_mask_bridge_test`) are identical here: the
  session plots with the board's DRC engine as rule resolver, those tests with
  `BoardCtx::bare`.
* The `kicad-rs` binary itself was compared on `ecc83-pp`, `issue7086`, `Issue24143`,
  `ZoneFill-4.0.7`: files identical, stdout identical except the FAB-09 line.
* Unit tests (`jobs_handler.rs`): `scaleToSelection`, `PlotJobToPlotOpts` for the CLI
  `gerbers` job and the SVG branch / colour theme warning, and a reduced port of
  `qa/tests/pcbnew/test_plot_job_layers.cpp` `AllCopperLayersSurviveSmallerBoard`
  (`ResolvePlotLayers` with `m_plotAllCopperLayers`; job JSON and loading the two boards are
  left out).

### Deviations

* (Obsolete since FAB-09) The export no longer ends with `NOT_YET_PORTED`: the job file is
  written (`gbr_files` / `job_file_path` hold what `AddGbrFile` and `CreateJobFile` receive).
* An error of `StartPlotBoard` (a format or drawing sheet outside the port) stops the export
  with that error instead of reporting `Failed to plot to '%s'.` and continuing (the C++ only
  fails there when the file cannot be created, which the port cannot).
* `PlotJobToPlotOpts`: the DXF / PDF / PostScript / PNG branches and `SetColorSettings` are
  not ported (formats out of scope; no colour settings in `PcbPlotParams`). The colour theme
  lookup knows only `_builtin_default`, `_builtin_classic` and `user`; an empty theme is the
  PCB editor default `_builtin_default`. The warning cannot fire from kicad-cli (a fresh
  `PCB_PLOT_PARAMS` is black and white when the check runs).
* The creation date is `JobExportPcbPlot::creation_date` of the session job (default all
  zeros); the hosts do not set it from the clock yet (change request).

### Known gaps

* `--check-zones`, `--variant`, `--ibt` (drawing sheet): session / FAB-14 / PLOT-08.
* Boards with mask minimum width (FAB-10) and generated tables (M26) panic / stop in those
  stubs; the corpus test counts them as pending.

## FAB-09: Gerber job file (`.gbrjob`)

### C++ to Rust

| C++ | Rust (`exporters/gerber_jobfile_writer.rs`) |
|---|---|
| `ONSIDE`, `JOBFILE_PARAMS` | `OnSide`, `JobfileParams` |
| `GERBER_JOBFILE_WRITER( aPcb, aReporter )`, `AddGbrFile` | `GerberJobfileWriter::new(ctx, reporter)`, `add_gbr_file` |
| `CreateJobFile`, `WriteJSONJobFile` | `create_job_file(path, env)`, `write_json_job_file(path, env)` -> `OutputFile` |
| `addJSONHeader`, `addJSONGeneralSpecs`, `addJSONDesignRules`, `addJSONFilesAttributes`, `addJSONMaterialStackup` | same names, snake case (private) |
| `mapValue` | `map_value` (private) on top of the public `map_value_mm` (`format_f( v, 4 )` parsed back) |
| `hasSilkLayers`, `hasSolderMasks`, `sideKeyValue`, `formatStringFromUTF32` | `has_silk_layers`, `has_solder_masks`, `side_key_value` (unused in C++ too), `format_string_from_utf32` |
| `BOARD::GetPadWithCastellatedAttrCount`, `GetPadWithPressFitAttrCount` | private copies in this module (not declared by the model; change request) |
| `ExpandTextVars( rev, GetProject(), FOR_GUI )` | private copy of `pcbplot::expand_project_text_vars` (same TEMPORARY mirror; change request) |

The document is a `serde_json::Value` built in the C++ insertion order (nlohmann
`ordered_json` `operator[]` / `+=` semantics through small helpers) and dumped with
`kicad_core::json::dump_stream( 2, Insertion )`. Wired into `jobs_handler::job_export_gerbers`
(writer without reporter, as `JobExportGerbers`): the `.gbrjob` is the last entry of `files` and
`outputs`; kicad-session's former "job file pending" outcome branch is gone.

### Tests and verification

* kicad-session `tests/gerbers_job.rs` `corpus_replay`: golden `tests/golden/gerbers_job/corpus.txt`
  regenerated by `generate.py` with the reference kicad-cli; it now hashes the `.gbrjob` (`J`
  line, normalised: the members `"Version"` and `"CreationDate"`; the GUID is a pure function of
  the board file name and is compared as is) and also runs the crafted boards of
  `tests/data/plot_driver` (4) and `tests/data/gerber_jobfile/jobfile_stackup.kicad_pcb` (+
  `.kicad_pro`, re-saved by `kicad-cli pcb upgrade --force`). Result: 770 runs; **704 of 704
  compared job files identical** byte for byte; 64 runs do not reach the job file (62 in the
  FAB-10 solder-mask stub, 2 boards not loadable) and start comparing automatically.
* `jobfile_stackup` covers: 4 layers with a non-ASCII user layer name (in `Path`, dielectric
  `Name` / `Notes`), every `FileFunction` branch (copper top / inner / bottom, glue, paste,
  legend, mask with `Negative`, `Other,User` for Dwgs and User.1, profile, fab), finish,
  impedance control, castellated and press-fit pads, edge plating, bevelled edge connector,
  revision through a project text variable with non-ASCII text, netclass and pad local
  clearances, inner / outer tracks and zones, a rule area (skipped), dielectric sub-layers,
  user `#RRGGBB` colours, standard colour names mapped by the colour table, mask material with
  epsilon / loss tangent.
* Unit tests: `map_value_mm` rounding (incl. a binary value just below the decimal midpoint),
  `side_key_value`.

### Deviations

* `mapValue` parses the `%.4f` text straight into `f64`; the C++ goes through `long double`
  (`%Lg`). Identical for every value below 10 m (`docs/THIRD_PARTY_EQUIVALENTS.md` 3.4).
* `CreateJobFile` cannot fail (the C++ ignores an unopenable stream as well); "Failed to create
  file" is therefore never reported.
* The FAB-07 deviation "the export ends with `NOT_YET_PORTED` ... (FAB-09)" no longer applies.

### Known gaps

* The "Board stackup settings not up to date." and "Unexpected layer id" reports are ported but
  not observable through kicad-cli (no reporter); not compared.


## FAB-08: Gerber drill files and IPC-4761 via protection files

### C++ to Rust

| C++ | Rust |
|---|---|
| `GERBER_WRITER` (ctor, `SetFormat`, `SetOptions`) | `exporters::gendrill_gerber_writer::GerberWriter<'a>` (`base: GendrillWriterBase`, `build_version`, `creation_date`), same-named snake-case methods |
| `CreateDrillandMapFilesSet( dir, genDrill, genMap, genTenting, reporter )` | `create_drill_and_map_files_set` (board), `create_drill_and_map_files_set_from(source, ...)`; the `writeProtection` lambda is the private `write_protection` |
| `createDrillFile`, `createProtectionFile`, `hasViaType`, `getBackdrillLayerPairFileName`, `writeBackdrillLayerPairFile` | private fns of the same names (one `GerberPlotter` per file, output to `base.outputs`) |
| `getDrillFileName` (override) | `GerberWriter::drill_file_name`, free fn `gerber_drill_file_name` (the virtual call of the drill report) |
| `convertOblong2Segment` (`static`) | `convert_oblong_2_segment(size, orient) -> (start, end)` |
| `GenDrillReportFile` through `GERBER_WRITER` | `GerberWriter::gen_drill_report_file[_from]` (passes the override) |
| `JobExportDrill`, `DRILL_FORMAT::GERBER` branch | `jobs_handler::job_export_drill_from` (same outcome rules as the Excellon branch) |

`dyn_cast<PCB_VIA*>` / `dyn_cast<PAD*>( m_SourceItem )` -> `DrillSource::Via` / `Pad`; the pad
property, the parent footprint reference (`TO.C`) and `PCB_VIA::GetWidth( Top/BottomLayer() )`
(covering / tenting diameters) are read from the board through those handles.

### Tests and verification

* `tests/gerber_drill.rs`, golden `tests/golden/gerber_drill/corpus.txt` (made by `generate.py`
  there with the reference kicad-cli, on copies, about 10 minutes): every `.kicad_pcb` of
  `qa/data` and `demos` under 300 KB plus the crafted board `tests/data/gerber_drill/
  ipc4761_vias.kicad_pcb` (4 layers, aux origin, every via protection feature with board
  defaults and overrides, front/inner/back via sizes, blind / buried / micro vias, castellated /
  pressfit / oblong / rotated pads, NPTH round and oval, a reserved-character reference), each
  with `--format gerber`, `+ --generate-tenting`, `+ --gerber-precision 5 --drill-origin plot`
  (1143 runs; file names, order and FNV-1a of the normalised contents; full normalised files of
  the crafted board under `ipc4761_vias/<set>/`).
  * `replay_of_oracle_hole_lists`: the C++ enumeration of the FAB-01 golden for its 10 corpus
    boards (sources resolved by ordinal on the parsed board) x 3 sets: **30 runs, 103 files
    (PTH / NPTH / blind / buried / backdrill / backdrill pair, filling, tenting front/back),
    all identical**.
  * `corpus_boards_match_cli`: parser + real enumeration + drill job. FAB-08 state: 831 of
    1143 runs identical, 309 pending on the `drill/via_access.rs` placeholders. Since
    FAB-GLUE (enumerator on the M17 via methods, crafted board `tests/data/excellon_corpus/
    drill_features.kicad_pcb` added to `generate.py`): **1143 of 1146 runs identical (1341
    files; 603 runs without holes), 0 pending, 0 differ**, 3 kicad-cli failures
    (`plugins/fakeboard.kicad_pcb`, not a board) skipped. `KICAD_FAB_GERBER_DRILL_DUMP=<dir>`
    writes differing files.
  * `basic_test_golden`: `qa/data/cli/basic_test/basic_test-PTH-drl.gbr` reproduced as
    `test_pcb.py::test_pcb_export_drill` (gerber case, 9 lines skipped) checks it.
* Unit tests in `gendrill_gerber_writer.rs` (`convertOblong2Segment`, names, precision).

### Deviations

* No file system: `OpenFile` failures ("Failed to create file") cannot occur; files go to
  `base.outputs`. `CreateDrillandMapFilesSet` returns `Result<(), FabError>`.
* `REPORTER::ReportTail( "Done." )` is reported with `Reporter::report`.
* A pad source whose pad is not on the board (hand-built lists only) gets no pad attributes.

### Known gaps

* (Done, FAB-12) `--generate-map`: see "FAB-12" below.
* (Done, FAB-GLUE) End-to-end on boards with vias: see "FAB-GLUE" below.

## FAB-GLUE: drill enumeration on the via model, model replacements, stale expectations

### Changes

| Before | After |
|---|---|
| `drill/via_access.rs` placeholders (`port_pending!`) | `PcbVia::drill_value(ctx)`, `layer_pair()`, `resolve_*_mode(ctx)` of the model (M17); file deleted |
| `drill_enumerator.rs` unit test: `KiidGenerator::default()` (R2-11) | the board's `kiid_generator`; the enumerator itself draws no UUID |
| `plot_board_layers::pad_delete_primitives_list` / `pad_add_primitive_poly` | `Pad::delete_primitives_list` / `Pad::add_primitive_poly` (on the `&mut Pad` copy with the pad env) |
| `plot_board_layers::round_chamfered_rect_to_polygon`, `SvgPlotter::set_round_rect_polygon_fn` call, `flash_pad_chamfer_round_rect(.., &fn)` | `GerberPlotter::flash_pad_chamfer_round_rect_default`; the SVG plotter's default generator |
| private `expand_project_text_vars` in `pcbplot.rs` and `gerber_jobfile_writer.rs` | `Board::expand_project_text_vars` (kicad-pcb `board/text_vars.rs`, `ExpandTextVars( s, GetProject(), FOR_GUI )` through the model's `PROJECT::TextVarResolver`) |
| local `pad_with_{castellated,press_fit}_attr_count`, `drill_symbol_layers` | `Board::pad_with_castellated_attr_count`, `pad_with_press_fit_attr_count`, `drill_symbol_layers` (kicad-pcb `board/queries.rs`) |
| `e2e` feature, ignored `enumeration_matches_oracle` / `drill_export_matches_cli_goldens` | run by default |
| corpus plot tests with `BoardCtx::bare` + connectivity | plus the board's DRC engine as `RuleResolver` (`plot_common::drc_engine`, as `BOARD_LOADER::Load` / kicad-session); kicad-drc is a dev-dependency |
| `KNOWN_DIFFERENCES` (plot driver / graphics / items corpus), barcode skip | empty (every file matches); barcode layers compared |

### Verification (all against the reference kicad-cli)

* `tests/excellon_corpus.rs` (new; golden `tests/golden/excellon_corpus/corpus.txt` from
  `generate.py` there, about 50 minutes): every `qa/data` / `demos` board under 300 KB plus the
  crafted boards `tests/data/gerber_drill/ipc4761_vias.kicad_pcb` and
  `tests/data/excellon_corpus/drill_features.kicad_pcb` (new: 10 copper layers, via backdrills
  front and back, tertiary drills, via front countersink / back counterbore, blind / micro /
  buried vias incl. reversed layer order, a rotated footprint with pad backdrill / tertiary /
  post-machining (ignored by the CLI drill export, `m_PadMachining` false), rotated oval PTH,
  castellated / press-fit pads, NPTH round and oval) x 12 option sets (default, which is
  `--excellon-oval-format alternate`; `--excellon-separate-th`; `--excellon-oval-format route`;
  `-u in`; `--excellon-zeros-format` suppressleading / suppresstrailing / keep;
  `--drill-origin plot`; `--excellon-min-header`; `--excellon-mirror-y`; `--generate-report`;
  `--generate-report --excellon-separate-th`), through parser, enumeration and drill job:
  **4572 of 4584 runs identical (7212 files incl. the reports: names, order, normalised
  contents), 0 pending, 0 differ**, 12 kicad-cli failures (`plugins/fakeboard.kicad_pcb`)
  skipped. Full normalised texts of the crafted boards for `default` and `report_sep`.
* `tests/gerber_drill.rs`: 831 of 1143 identical / 309 pending before; **1143 of 1146
  identical, 0 pending, 0 differ** after (the crafted board above added; `default`,
  `--generate-tenting`, `--gerber-precision 5 --drill-origin plot --generate-tenting`).
* `tests/drill_golden.rs::drill_export_matches_cli_goldens`: ignored before; 10 boards x 5
  sets identical. `tests/drill_oracle.rs::enumeration_matches_oracle`: ignored before; the
  enumeration of the 10 corpus boards equals the C++ raw lists.
* Corpus plot replays, before (start of FAB-GLUE) -> after: plot driver 8445 matched / 88
  pending / 0 known -> 8445 / 88 / 0 (`KNOWN_DIFFERENCES` emptied, 7 stale entries); plot
  graphics corpus 8443 matched + 2 barcode skipped -> 8445 matched; crafted 31 + 45 -> same;
  plot items 7334 + 26 -> same; 0 failures throughout. The 88 pending are drill maps (FAB-13)
  and mask minimum width (FAB-10). The DRC-engine resolver changed no result (the B.Mask files
  had already been fixed in the model).

### Deviations

* None new. `${VCSHASH}` / `${VCSSHORTHASH}` in a revision now resolve as the model's
  `PROJECT::TextVarResolver` does ("no hash", as `GetCurrentHash` outside a git repository)
  instead of the former local copies' empty string.

### Known gaps / change requests

* Knockout / strike-out text of a footprint dimension still reaches the pending stub in
  `plot_brditems_text.rs` (owned by FONT-RC-1 while it runs): its
  `pcb_text_transform_text_to_poly_set` should call `PcbDimension::transform_text_to_poly_set`.

## FAB-10: solder mask minimum width and clipped silk

### C++ to Rust

| C++ (`pcbnew/plot_board_layers.cpp`) | Rust |
|---|---|
| `PlotPolySet` (L64-86) | `plot_layer_poly::plot_poly_set` |
| `PlotSolderMaskLayer` (L91-107) | `plot_board_layers::plot_solder_mask_layer` (min width 0 -> `PlotLayer`, as FAB-03 left it) + `plot_layer_poly::plot_solder_mask_layer_min_width` (the rest) |
| `PlotClippedSilkLayer` (L110-125) | `plot_layer_poly::plot_clipped_silk_layer` (called from `plot_one_board_layer`) |
| `GenerateLayerPoly` (L1020-1275) incl. its lambdas `handlePcbShape`, `handleFPTextItem`, `handleTextBoxItem` | `plot_layer_poly::generate_layer_poly`, `LayerPolyGen` |
| `PCB_TABLE::TransformShapeToPolySet` / `TransformGraphicItemsToPolySet` with non-null `aRenderSettings` (dashed borders) | `plot_layer_poly::table_transform_shape_to_poly_set` (private; the kicad-pcb model has only the `nullptr` overload) |

Notes on the port:
* `track->GetSolderMaskExpansion()` is called through `const PCB_TRACK*` and is not virtual,
  so a via runs the `PCB_TRACK` body (rules with the via as `A`, the track-base local margin,
  board setting, clamp to `-m_width / 2` of the base width 0.2 mm): `via.track.solder_mask_expansion`.
* `DISABLE_ARC_RADIUS_CORRECTION` is held to the end of `generate_layer_poly`, as the C++ local
  lives until the function returns (Simplify / Deflate / BooleanAdd run under it).
* `MarkFootprintPlotted` (PDF bookmarks) is not ported (out of scope, as elsewhere).

### Tests and verification (reference kicad-cli)

* kicad-session `tests/gerbers_job.rs` `corpus_replay` (session path: CLI loader, connectivity,
  DRC engine as rule resolver): 770 runs, **766 identical**, 2 pending (FAB-13 chart symbols),
  2 not loadable, 0 failures. All 62 runs of the 31 corpus boards with
  `solder_mask_min_width > 0` (formerly pending on this package) are identical.
* `tests/plot_layer_poly.rs` `crafted_boards` (golden `tests/golden/plot_layer_poly/crafted.txt`,
  `generate.py crafted`): boards `tests/data/plot_layer_poly/` (hand-written, re-saved by
  `kicad-cli pcb upgrade --force`): `mask_min_width` (pads closer than the 0.3 mm minimum width,
  per-pad margin, PTH oval pad, untented / front-tented vias, F.Mask / B.Mask tracks and arc,
  solid / dashed / dotted mask lines, mask text, mirrored back text, mask text box with dashed
  border, filled poly / rect, F.Mask zone with two fill outlines, rotated back footprint),
  `silk_over_mask` and `silk_over_mask_minw` (silk lines / dashed / dash-dot / circle / text /
  filled poly / text box across pad, via and mask openings, front and back) x 4 sets (all layers;
  `-l F.Cu,Edge.Cuts --cl F.Mask,B.Mask`; `--subtract-soldermask -l F.SilkS,B.SilkS`;
  `--subtract-soldermask -l F.Cu,B.Cu --cl F.SilkS,B.SilkS`): **12/12 runs, 36/36 files
  byte-identical** (after the version / date normalisation).
* `tests/plot_layer_poly.rs` `corpus_replay` (golden `corpus.txt`, `generate.py corpus`, about
  7 minutes): every board under `qa/data` and `demos` below 300 KB with the two
  `--subtract-soldermask` sets: 760 runs, **758 identical** (1422 files), 2 pending (FAB-13 chart
  symbols on `modern_metadata`), 0 failures. This replays `JobExportGerbers`' per-file loop
  (`[layer] + --cl`) on the kicad-fab test loader (parse + project + connectivity + DRC engine).

### Deviations

None known.

### Known gaps

* `table_transform_shape_to_poly_set` (dashed table borders of a board table on a mask / silk
  layer with render settings) is a local port because kicad-pcb lacks the render-settings
  overload; no corpus board exercises it (unverified against the oracle).
* SVG `--subtract-soldermask` reaches the same code; it is verified only through Gerber here
  (SVG plotting is FAB-11).

## REV-FAB (review, 2026-11-10)

Reviewer pass, no source change in this crate. Report: `rust/docs/design/review-fab.md`
(option, stored-parameter and item coverage tables, findings, harness in
`rust/docs/design/review-fab/`). About 2 300 differential runs of `kicad-rs` against the
reference kicad-cli: every `pcb export gerbers` / `pcb export drill` option, 31 variants of the
stored plot parameters, all 256 Excellon option combinations on three boards, nine crafted
boards (996 pads in rotated / flipped footprints, 1008 shapes, 786 text / dimension / table
items, zones, vias, exotic names, extreme coordinates).

### Deviations found (open)

* F-03 (`plot_board_layers.rs` `plot_pad_layer`): the C++ plots the board's pad in place and
  restores it through the board-frame `PAD::SetSize`, which is lossy under a footprint scale
  and persists into the layers plotted later; the port's detached copy did not. Visible as
  1 to 3 nm in the `F.Mask` macro of a chamfered pad in a non-uniformly scaled footprint when
  `F.Cu` or `F.Paste` is plotted first. Fixed by FIX-FAB-1 (`PadPlotState`, below).
* F-01 (accepted, already listed under FAB-03 / FAB-07): `--board-plot-params` with a stored
  non-Gerber `outputformat`: the reference writes that format into the `.gbr` files (HPGL:
  `Failed to plot`, job file, exit code 4); the port stops with an error (exit code 2).

### Known gaps confirmed by the review

`--variant` (model stub), `--ibt` / `plotframeref` (drawing sheet), `--generate-map` (FAB-12),
drill chart symbols (FAB-13): the boards and option sets to rerun are in the report.

## FAB-11: SVG board plots, drill marks, colour themes, the svg job

### C++ to Rust

| C++ | Rust |
|---|---|
| `PCB_PLOTTER::Plot` (`pcb_plotter.cpp` L68-354) | `pcb_plotter::plot -> PlotOutcome { success, error, files, outputs }` (single / multi mode, common layers, skipped copper layers, file names, `Plotted to '%s'.`, `Done.`, Gerber job file branch) |
| `PCBNEW_JOBS_HANDLER::JobExportSvg` from `PCB_PLOT_PARAMS plotOpts` on, static `plotJob` (L1506-1579) | `jobs_handler::job_export_svg`; the rest (`getBoard`, variant, `resolvePlotOutputPath`, `preparePlotLayers`, `RefreshGeneratedTables`) is `kicad_session::jobs_handler::run_export_svg` |
| `JOB_EXPORT_PCB_SVG` (members, defaults, `GEN_MODE`) | `jobs::job_export_pcb_svg::{JobExportPcbSvg, SvgGenMode, SVG_PRECISION_DEFAULT}` (options only) |
| `BRDITEMS_PLOTTER::PlotDrillMarks`, `plotOneDrillMark` (L1409-1477) | `BrditemsPlotter::plot_drill_marks`, private `plot_one_drill_mark`; `ADVANCED_CFG::m_SmallDrillMarkSize` = `SMALL_DRILL_MARK_SIZE` (0.35 mm default) |
| `COLOR_SETTINGS` (`GetColor`, `GetFilename`), `s_defaultTheme` / `s_classicTheme`, `CreateBuiltinColorSettings`, `SETTINGS_MANAGER::GetColorSettings` | `color_settings::{ColorSettings, get_color_settings}` (board layers, `LAYER_DNP_MARKER`, `LAYER_PCB_BACKGROUND`, `LAYER_DRAWINGSHEET`; 98 entries per built-in theme, generated from `builtin_color_themes.h`) |
| `PCB_PLOT_PARAMS::SetColorSettings` / `ColorSettings()` | `plot_job_to_plot_opts` returns the `ColorSettings`; the plot functions (`plot_board_layers`, `plot_one_board_layer`, `plot_standard_layer`, `plot_poly_set`, `plot_solder_mask_layer_min_width`, `plot_clipped_silk_layer`, `BrditemsPlotter::new`) take it as a parameter (the model's `PcbPlotParams` dropped `m_colors`); `BrditemsPlotter::color_settings_color` = `ColorSettings()->GetColor` |
| `PCB_TEXT::TransformTextToPolySet` of a `PCB_DIMENSION_BASE` (knockout / struck-out dimension text in a footprint) | `plot_brditems_text::pcb_text_transform_text_to_poly_set` calls `PcbDimension::transform_text_to_poly_set` |

### Tests and verification (reference kicad-cli)

* kicad-session `tests/svg_job.rs` `corpus_replay` (session path: CLI loader, connectivity, DRC
  engine as rule resolver; golden `kicad-session/tests/golden/svg_job/corpus.txt`,
  `generate.py` there): every corpus board under 300 KB plus the crafted boards of
  `tests/data/{plot_driver,plot_items,plot_graphics,plot_layer_poly,plot_text,svg_plot}` with
  16 layers in multi mode, colour, full drill marks; and 5 boards (ecc83-pp, microwave,
  `plot_items/pads`, `plot_graphics/graphics`, `svg_plot/svg_marks`) x 19 option sets
  (`--mode-single`, single + `--fit-page-to-board`, `--cl`, `--black-and-white`, `--negative`,
  `--mirror`, `--drill-shape-opt 0` / `1`, `--page-size-mode 0` / `1` / `2`,
  `--fit-page-to-board`, `-t _builtin_classic`, `-t no_such_theme` (warning), 
  `--subtract-soldermask`, `--scale 2`, `--cdnp`, b&w + negative + mirror + small marks, and
  the drawing sheet). **492 runs: 490 identical** (exit code, stdout, every file byte for
  byte after normalising the `<title>` date: 6531 files), 1 pending (FAB-13 chart symbols:
  `modern_metadata`), 1 not loadable (`fakeboard`, rejected by kicad-cli too). The drawing
  sheet runs match since the drawing sheet package landed (before: the documented NotPorted
  error, which the test still accepts).
* `cli_test_pcb_export_svg`: `qa/tests/cli/test_pcb.py::test_pcb_export_svg` for
  `ZoneFill-4.0.7` (F.Cu, B.Cu; `--page-size-mode 1 --exclude-drawing-sheet --black-and-white`)
  byte-identical to the reference kicad-cli (the committed `.svg` files are from an older KiCad
  and only pass the C++ test's image diff); `ZoneFill-Legacy.brd` needs the legacy reader.
* `tests/data/svg_plot/svg_marks.kicad_pcb` (hand-written `svg_marks_input.kicad_pcb`, re-saved
  by `kicad-cli pcb upgrade --force`): rotated footprint with round, oval-slot, NPTH (size ==
  drill: black mark), SMD, small-drill and B.Cu-only pads, two vias, a knockout aligned
  dimension on F.SilkS inside a footprint, a DNP footprint whose F.Fab dimension shows
  `${REFERENCE}` (struck out with `--cdnp`). Also checked once by hand through
  `pcb export gerbers --cdnp -l F.SilkS,F.Fab,F.Cu`: all files identical.
* `tests/plot_pads_qa.rs::drill_marks_plotted_on_edge_cuts_svg`: port of
  `DrillMarksPlottedOnEdgeCutsSvg` (`test_plot_edge_cuts_drill_marks.cpp`); the two Gerber cases
  were ported by FAB-04.
* `color_settings.rs` unit test (lookup by key / display name / unknown name, unloaded
  settings); the Gerber corpus (`gerbers_job.rs`) is unchanged by the colour plumbing.

### Deviations

* `PcbPlotParams` has no colour settings: the theme travels as an explicit `ColorSettings`
  argument. The tests that mimic C++ QA code with a default `PCB_PLOT_PARAMS` pass
  `ColorSettings::default()` (never loaded: every colour `UNSPECIFIED`, as in C++).
* Colour theme files (`<config>/colors/*.json`, `--theme <user theme>`) cannot be read (no file
  system): every name that is not `_builtin_default`, `_builtin_classic`, `user` or a built-in
  display name behaves as a missing theme does in C++ (warning, default colours).
* `PCB_RENDER_SETTINGS::LoadColors` is not applied to the plotter's render settings; the board
  plot reads colours from `ColorSettings()` only (the render settings' layer colours are read
  by the drawing sheet, which takes its colour from its own package).
* `PCB_PLOTTER::Plot`: SVG fit-to-board changes the page and aux origin on a copy of the board
  (the C++ changes and restores the board); `RefreshGeneratedTables` runs in the session before
  the call; `SetTitle` gets the unexpanded title (only PDF reads it); an error of
  `StartPlotBoard` (a part outside the port) stops the plot instead of reporting `Failed to
  create file` (the port cannot fail to create a file).

### Known gaps

* `PlotDrillSymbols` / `PlotChartSymbols` (FAB-13): the board `modern_metadata` is pending.
* PDF / DXF / PostScript / PNG branches of `PCB_PLOTTER::Plot` (formats out of scope).

## FAB-12: drill maps

### C++ to Rust

| C++ | Rust |
|---|---|
| `GENDRILL_WRITER_BASE::genDrillMapFile( aFullFileName, aFormat )` | `exporters::gendrill_map::gen_drill_map_file(writer, name, format, env) -> Result<Vec<u8>, FabError>` |
| `GENDRILL_WRITER_BASE::plotDrillMarks( aPlotter )` | `gendrill_map::plot_drill_marks(writer, plotter)` |
| `CreateMapFilesSet( aPlotDirectory, aReporter )` (FAB-01 loop) | `create_map_files_set(dir, env, reporter)`: new `env: &PlotEnv` argument (build version and date of the map plots), passed by `ExcellonWriter` / `GerberWriter::create_drill_and_map_files_set_from` |

Ported in full for `PLOT_FORMAT::GERBER` (X2 header, `TF.FileFunction,Drillmap`, 4.5 mm
format, scale 1 at the drill offset) and `SVG` (A4 landscape `m_pageInfo` default, 10 mm margin,
60/40 split, scale clipped at 3, `bottom_limit` column wrapping of the legend): board and footprint
`Edge.Cuts` shapes as `Dwgs.User` copies without group through `BRDITEMS_PLOTTER::PlotShape`
(default `PCB_PLOT_PARAMS`, unloaded colour settings, colour mode off), one `Marker` per hole
(+ `ThickOval` for slots), the legend with the exact strings (`%3.3fmm / %2.4f"`, castellated /
press-fit / backdrill stub / post-machined / counts / not plated) in the stroke font.

### Tests and verification (reference kicad-cli)

* `tests/drill_map.rs`, golden `tests/golden/drill_map/corpus.txt` (made by `generate.py`
  there on copies; about 25 minutes): every `qa/data` / `demos` board under 300 KB plus the
  crafted boards `tests/data/drill_map/map_columns.kicad_pcb` (250 x 80 mm with 22 via tools
  and slots / castellated / press-fit / NPTH tools: the SVG legend wraps into several columns;
  arc, circle, dashed rect, grouped and footprint `Edge.Cuts` shapes, Edge.Cuts text and a
  Dwgs.User line that must not be plotted), `map_tiny` (scale clipped at 3, aux origin),
  `map_large` (scale < 1) and `tests/data/excellon_corpus/drill_features.kicad_pcb` (backdrill
  stubs, post-machining, blind / buried spans), each with 7 sets: Excellon + gerberx2 map
  (merged, `--excellon-separate-th`, `--drill-origin plot`), Excellon + svg map (merged,
  separate), `--format gerber` + gerberx2 / svg map. Names, order and FNV-1a of the normalised
  maps (`TF.GenerationSoftware`, `TF.CreationDate`, `G04 Created by`, the date of the SVG
  `<title>`); full normalised maps of `map_tiny` for two sets.
  * `corpus_maps_match_cli`: **2681 of 2688 runs identical (4787 maps), 0 pending, 0 differ**;
    7 kicad-cli failures (`plugins/fakeboard.kicad_pcb`, not a board) skipped.
  * `crafted_maps_full_text`: identical. `unsupported_map_formats`: PDF / PS / DXF.
* `KICAD_FAB_DRILL_MAP_DUMP=<dir>` writes differing maps.

### Deviations

* PDF, PostScript and DXF maps (and HPGL, which C++ turns into PDF): `FabError::Unsupported`
  naming the format, after `Failed to create file '...'` is reported (no plot engine).
* The C++ makes `Edge.Cuts` visible around `GetBoardEdgesBoundingBox`; the port does not touch
  the board (`ComputeBoundingBox( true, true )` sets `Edge.Cuts` itself, same result).
* `OpenFile` / `EndPlot` cannot fail (no file system).

### Known gaps

* None for the Gerber X2 and SVG maps.

## FIX-FAB-1: the pad that `plotPadLayer` leaves behind (footprint scale)

Finding F-03 of REV-FAB (`docs/packages/FIX-FAB-1.md`).

### C++ to Rust

| C++ | Rust |
|---|---|
| the board's `PAD` as `plotPadLayer` (L446-681) leaves it for the next layer / file of the job | `plot_board_layers::PadPlotState` (overlay `PadId -> Pad`, `RefCell<BTreeMap>`), passed to `plot_board_layers` / `plot_one_board_layer` / `plot_standard_layer` and installed in `BrditemsPlotter::pad_state` |
| restore `SetSize`, `SetDelta`, `SetShape`, `SetRoundRectRadiusRatio` (L676-680) | end of `plot_pad_layer`, which returns the restored pad (`None` for the zero-size exit L482-486, which modifies nothing); the NPTH skip (L494-500, a `break`) now reaches the restore |
| pad loop body L387-447 reading the (modified) board pad | `plot_footprint_pad`: reads the pad from the state, falls back to the board's pad; the layers of `SeqStackupForPlotting()` chain the restored pad |
| one loaded board per job (`pcbnew_jobs_handler.cpp` L1835-1912, `PCB_PLOTTER::Plot`) | one `PadPlotState` per call of `job_export_gerbers` and `pcb_plotter::plot` (SVG job), shared by all files in plot order; `plot_gerber_layer_file` (one file) and `plot_standard_layer_pads_vias_tracks_zones` without an installed state use a fresh one |

The restore is applied to a clone of the pad the layer started from (it keeps the arena id),
not to the detached `work` copy: the plot code modifies `work` only through the four restored
members of `a_layer`, so the state is the one the C++ leaves.

### Tests and verification (reference kicad-cli)

* `tests/plot_pad_state.rs`: two crafted boards (`tests/data/plot_pad_state`: `one_pad.py ..
  90 2 0.5 15 cham`, and six pad shapes in a footprint with `(scale 0.3 0.3)` and sizes with a
  nm remainder), goldens from `kicad-cli pcb export gerbers -l F.Cu,F.Paste,F.Mask` and
  `-l F.Mask` (the two reference masks differ; both match).
* `review-fab/tools/fabcmp.py`: the three failing `one_pad.py` configurations (`90 2 0.5 15`,
  `33 2 0.5 15`, `90 2 0.5 33` `cham`) with `-l F.Cu,F.Mask`, `-l F.Paste,F.Mask`,
  `-l F.Mask` and the default layers: 12 / 12 identical; 6 shapes x 4 footprint configurations
  (incl. uniform and relative angle 0) x 2 layer orders: 48 / 48 identical; `rev_scaled`
  (`gen_pads.py`, the 12 `(scale 1 1)` replaced by uniform 0.3 .. 3 and non-uniform scales) x
  `tools/gs3.txt`: 5 / 5 sets identical (23 files each, `bpp` 12); the same board and
  `one_pad` with `solder_mask_min_width 0.1`: identical.

### Deviations

* None known.

### Known gaps

* The solder mask minimum width path and the clipped silk (`plot_layer_poly.rs`
  `generate_layer_poly`) read the pads through `FOOTPRINT::TransformPadsToPolySet` of the
  model, i.e. the board's pads, not the `PadPlotState`. The C++ reads the modified pads there
  too. No difference found (`rev_scaled` and `one_pad` with a minimum width), but a footprint
  scale, a minimum mask width and a pad plotted before on another layer of the same job could
  differ by a nanometre. Fixing it needs a model entry point that takes pad overrides.
* DRC rules evaluated for a pad's margins (`ctx.rules`) may look the pad up in the board
  rather than in the state (only conditions on the pad size could notice).

