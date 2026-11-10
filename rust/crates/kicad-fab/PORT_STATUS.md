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
| `drill/drill_enumerator.rs` | `pcbnew/drill/drill_enumerator.h/.cpp` | FAB-01 | ported; reaches model stubs (see below) |
| `drill/via_access.rs` | placeholders for `PCB_VIA` accessors not yet declared by the model | FAB-01 | stub (panics, to be deleted when M17 lands) |
| `exporters/gendrill_writer_base.rs` | `gendrill_writer_base.h` (all types), `.cpp` L47-300, L303-355, L358-461, L464-501 | FAB-01 | ported |
| `exporters/gendrill_map.rs` | `gendrill_writer_base.cpp` L506-847, L996-1019 | FAB-12 | stub: `gen_drill_map_file` returns `FabError::NotPorted` |
| `exporters/gendrill_excellon_writer.rs` | `gendrill_excellon_writer.*` | FAB-02 | ported |
| `exporters/gendrill_report.rs` (`HoleListSource`, `GenDrillReportFile`, `printToolSummary`) | `gendrill_writer_base.cpp` L850-993, L1022-1070 | FAB-02 | ported |
| `jobs/job_export_pcb_drill.rs`, `jobs_handler.rs` (`job_export_drill`) | `job_export_pcb_drill.*` (options), `pcbnew_jobs_handler.cpp` `JobExportDrill` | FAB-02 | ported (Excellon); Gerber branch waits for FAB-08 |
| `exporters/gendrill_gerber_writer.rs` | `gendrill_gerber_writer.*` | FAB-08 | not started |
| `exporters/gerber_jobfile_writer.rs` | `gerber_jobfile_writer.*` | FAB-09 | not started |
| `pcbplot.rs`, `pcb_plotter.rs` (FAB-03 part), `plot_board_layers.rs` (driver + `PlotStandardLayer` skeleton), `plot_brditems_plotter.rs` (struct, pending entry points) | see "FAB-03" below | FAB-03 | ported (driver); item bodies see FAB-04 and FAB-05/06/10/11/13 |
| `plot_board_layers.rs` (`PlotStandardLayer` L383-930: pads, DNP cross, vias, tracks, zones), `plot_brditems_plotter.rs` (`getColor`, `PlotPadNumber`, `PlotPad`, `PlotZone`) | see "FAB-04" below | FAB-04 | ported |
| `plot_brditems_shapes.rs` (`PlotBoardGraphicItem`, `PlotFootprintGraphicItems`, `PlotShape`, `PlotDimension`, `PlotPcbTarget`, `PlotBarCode`, `PlotTableBorders`), `line_ending_plot.rs` (`LINE_ENDING::Plot`) | see "FAB-05" below | FAB-05 | ported |
| `plot_brditems_text.rs` (`PlotFootprintTextItems`, `PlotText`) | see "FAB-06" below | FAB-06 | ported |
| `plot_layer_poly.rs`, other `jobs/*`, the Gerber / SVG parts of `jobs_handler.rs`, the remaining pending members of `plot_brditems_plotter.rs` | see each module header | FAB-07..13 | not started |

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

### Model accessors used (final API; panics until the model package lands)

* Pad (M14): `Pad::position(ctx)`, `orientation(ctx)`, `drill_size(ctx)`, `has_hole(ctx)`
  (`port_pending!` today); `attribute()`, `property()`, `lib_padstack()` (the C++ `Padstack()`,
  for the drill shape, backdrills and post-machining) are real.
* Via (M17): `drill/via_access.rs` placeholders for `GetDrillValue`, `LayerPair`,
  `ResolveFillingMode`, `ResolveCappingMode`, `Resolve{Front,Back}{Covering,Plugging,Tenting}Mode`;
  the model does not declare these yet (requested from M17; then the calls switch to the model
  methods and the file goes). `PcbVia::padstack` and `track.start` are real.
* Stackup (M6): `BoardStackup::get_drill_depth` (`port_pending!`), reached only for via
  backdrills.
* Board: `tracks()`, `footprints()`, `footprint_pads()`, `copper_layer_count()`,
  `design_settings()`, `file_name` are real.

Consequence: `enumerate_drill_operations` / `enumerate_drill_spans` / `build_holes_list` panic
with a "port pending" message on any board with vias or pads until M14 / M17 land. Everything
downstream of the enumeration (sort, tools, names, file functions) is verified now.

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
* `enumeration_matches_oracle` (ignored, `--features e2e`): enumeration from the parsed corpus
  boards against the same raw lists. Needs `kicad_pcb::io::parse_board`, M14, M17.
* `tests/drill_golden.rs`: kicad-cli goldens under `tests/golden/drill/<board>/<set>/` for the
  10 corpus boards x 5 option sets (`--format excellon`; `-u in`; `--excellon-mirror-y`;
  `--excellon-separate-th`; `--format gerber`), made by `tests/golden/drill/generate.sh`
  (commands recorded there, run on copies). Checks: golden set consistent; `basic_test` goldens
  equal the repository goldens of `qa/data/cli/basic_test/` after normalisation (all 4 equal);
  normaliser. `drill_export_matches_cli_goldens` (ignored) is the end-to-end comparison; its
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
* Enumeration end-to-end verification waits for the parser and M14 / M17.

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
* `tests/drill_golden.rs::drill_export_matches_cli_goldens`: `run_port` wired (parser, CLI
  defaults, job); still ignored: needs the remaining parser packages (PCBIO-04 `setup`), the pad
  / via model accessors (M14, M17) and the Gerber writer (FAB-08).
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

* `--format gerber` in the job: `FabError::NotPorted` until FAB-08 (`GERBER_WRITER`; its report
  must pass its own `getDrillFileName` override to `gen_drill_report_file`).
* `--generate-map`: `FabError::NotPorted` until FAB-12 (as in FAB-01).
* End-to-end from a `.kicad_pcb` file: waits for the parser / M14 / M17 (see above).

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
are both a `detached_copy()` of the pad viewed with `ctx.view` (`board-model.md` 13). Setters
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
* Model differences found by the corpus replay (reported as change requests, listed in
  `KNOWN_DIFFERENCES` of `plot_graphics_corpus.rs` and `plot_items_corpus.rs`): the pad's
  local solder mask margin is lost on B.Mask (`Pad::set_local_solder_mask_margin` sets the
  front mask props only; C++ `PAD::SetLocalSolderMaskMargin` sets F_Mask and B_Mask; same
  for the paste margin and ratio setters): 3 corpus B.Mask files differ. Barcodes plot
  nothing until the model has the zint encoder (M25a).

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

* Knockout or strike-out of a footprint dimension's text reaches a pending panic: the model
  has no public `PcbDimension::transform_text_to_poly_set` (`pcb_text::
  transform_text_to_poly_set` is crate-private).
* The model loader drops the render cache of footprint texts in an outline font (crafted
  `User.3`: C++ plots the cached glyphs, the port gets no cache).
* `PcbTablecell::corners` / `draw_pos` of a table inside a footprint are footprint-local
  (not moved by the footprint position) and degenerate when rotated:
  `qa/data/pcbnew/embedded_table_rotated_legacy_v10.kicad_pcb` F.Cu / B.Cu differ (cell
  text 40 / 28 mm off); the table borders match.

