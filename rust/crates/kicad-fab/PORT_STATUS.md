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
| `exporters/gendrill_excellon_writer.rs` | `gendrill_excellon_writer.*` | FAB-02 | not started |
| `exporters/gendrill_gerber_writer.rs` | `gendrill_gerber_writer.*` | FAB-08 | not started |
| `exporters/gerber_jobfile_writer.rs` | `gerber_jobfile_writer.*` | FAB-09 | not started |
| `pcbplot.rs`, `pcb_plotter.rs`, `plot_board_layers.rs`, `plot_layer_poly.rs`, `plot_brditems_*.rs`, `line_ending_plot.rs`, `jobs/*`, `jobs_handler.rs` | see each module header | FAB-02..11 | not started |

Drill report (`GenDrillReportFile`, `printToolSummary`, `gendrill_writer_base.cpp` L850-993,
L1022-1070) is FAB-02; the `ToolSummary` enum of the header is already here.

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
