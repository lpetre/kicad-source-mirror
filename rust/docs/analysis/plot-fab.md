# Plotters and fabrication outputs - porting analysis

Analyst document for the port of the plot engines (`common/plotters/`), the board plot code
(`pcbnew/plot_*.cpp`, `pcbnew/pcbplot.cpp`, `pcbnew/pcb_plotter.cpp`), the drill file writers
(`pcbnew/exporters/gendrill_*`, `pcbnew/drill/`), the Gerber job file writer and the
`kicad-cli pcb export gerbers | drill | svg` flows to the Rust crates `kicad-plot` and `kicad-fab`.
The C++ tree is the specification. All line numbers refer to the files in `<kicad>` as checked in
(KiCad master, board file format 20261031, `KICAD_SEMANTIC_VERSION` `10.99.0-unknown`).

Read `rust/PORTING.md` first. This document gives the map and the work packages; the implementer
of a package reads this document plus the C++ files of that package.

File abbreviations used in line references:

| Abbr. | File | Lines |
|---|---|---|
| `PH` | `include/plotters/plotter.h` | 789 |
| `P` | `common/plotters/plotter.cpp` | 667 |
| `GH` | `include/plotters/plotter_gerber.h` | 398 |
| `G` | `common/plotters/GERBER_plotter.cpp` | 1919 |
| `M` | `common/gbr_metadata.cpp` | 803 |
| `S` | `common/plotters/SVG_plotter.cpp` | 1019 |
| `PS` | `common/plotters/PS_plotter.cpp` (only L45-226 are relevant) | 1117 |
| `PP` | `pcbnew/pcbplot.cpp` | 372 |
| `PL` | `pcbnew/pcb_plotter.cpp` | 606 |
| `L` | `pcbnew/plot_board_layers.cpp` | 1641 |
| `B` | `pcbnew/plot_brditems_plotter.cpp` | 1656 |
| `DE` | `pcbnew/drill/drill_enumerator.cpp` | 352 |
| `DB` | `pcbnew/exporters/gendrill_writer_base.cpp` | 1070 |
| `DX` | `pcbnew/exporters/gendrill_excellon_writer.cpp` | 789 |
| `DG` | `pcbnew/exporters/gendrill_gerber_writer.cpp` | 633 |
| `J` | `pcbnew/exporters/gerber_jobfile_writer.cpp` | 772 |
| `H` | `pcbnew/pcbnew_jobs_handler.cpp` | 5279 |

Contents:

1. Purpose and entry points
2. File inventory
3. Data structures
4. Control flow and algorithms (with every order-sensitive place)
5. What this subsystem needs from other subsystems
6. Output formats
7. Backward compatibility
8. Multi-threading
9. Existing tests and data, differential tests
10. Rust module layout and work packages
11. Risks, traps, open questions

---

## 1. Purpose and entry points

The subsystem turns a loaded board into manufacturing files:

* **Gerber layer files** (RS-274X with X2 attributes, or X1 with attributes as structured
  comments), one file per board layer, optionally with extra "common" layers plotted into each
  file, plus the **Gerber job file** (`.gbrjob`, JSON).
* **Drill files**: Excellon (`.drl`) or Gerber X2 (`-drl.gbr`), split by plated/non-plated,
  layer pair (blind/buried vias) and backdrill span; via-protection files (IPC-4761); a text
  **drill report**; **drill map** drawings.
* **SVG** plots of board layers (wanted here for a browser preview).

It has two halves:

* *Plot engines* (`PLOTTER` and subclasses): a small imperative drawing API in board internal
  units (`MoveTo/LineTo`, `Circle`, `Arc`, `PlotPoly`, `ThickSegment`, `FlashPad*`, `Text`) that
  each engine translates into its file syntax. They know nothing about boards.
* *Board plotting* (`BRDITEMS_PLOTTER`, `PlotOneBoardLayer`, `PlotStandardLayer`, the drill
  writers): walks the board in a fixed order and calls the plot engine.

### 1.1 Callers

| Caller | File | In port scope |
|---|---|---|
| `kicad-cli pcb export gerbers` | `kicad/cli/command_pcb_export_gerbers.cpp` + `command_pcb_export_gerber.cpp` (shared options) -> `JOB_EXPORT_PCB_GERBERS` -> `PCBNEW_JOBS_HANDLER::JobExportGerbers` (`H:L1750-1923`) | yes, tier 1 |
| `kicad-cli pcb export drill` | `kicad/cli/command_pcb_export_drill.cpp` -> `JOB_EXPORT_PCB_DRILL` -> `JobExportDrill` (`H:L2320-2472`) | yes, tier 1 |
| `kicad-cli pcb export svg` | `kicad/cli/command_pcb_export_svg.cpp` -> `JOB_EXPORT_PCB_SVG` -> `JobExportSvg` (`H:L1542-1579`) -> `PCB_PLOTTER::Plot` (`PL:L68-354`) | yes, tier 2 |
| Job type `"gerber"` (single file, several layers in one file) | `JobExportGerber` (`H:L2214-2314`). **There is no CLI sub-command for it in this tree** (`kicad/kicad_cli.cpp:L192,291` registers only `gerbers`); it is reachable from jobsets only | optional (tier 3): it is 60 lines once the rest exists |
| `kicad-cli pcb export pdf/ps/dxf/png/hpgl`, 3D, pos, IPC-2581, ODB++ ... | other jobs | no |
| GUI: `DIALOG_PLOT`, `DIALOG_GENDRILL`, printing, the IPC API, Python (`PLOT_CONTROLLER`) | `pcbnew/dialogs/*`, `pcbnew/plotcontroller.cpp` | no |
| Zone filler, DRC | do **not** call into this subsystem | - |

There is no single-layer `pcb export gerber` command any more. `PCB_EXPORT_GERBER_COMMAND` only
exists as the base class that owns the option definitions and `populateJob()`.

### 1.2 `kicad-cli pcb export gerbers`, in execution order

`command_pcb_export_gerber.cpp:L29-108` defines the options, `L111-154` (`populateJob`) copies them
into the job, `command_pcb_export_gerbers.cpp:L28-57` adds one flag and dispatches.

| Flag | Default | Job member | Effect |
|---|---|---|---|
| positional input | required | `m_filename` | board file; "Board file does not exist or is not accessible" -> exit `ERR_INVALID_INPUT_FILE` |
| `-o, --output` | `""` | configured output path | output **directory** |
| `-l, --layers LIST` | `""` | `m_argLayers = Some("")` | comma separated layer names. Empty string means "all layers" (see below) |
| `--cl, --common-layers LIST` | `""` | `m_argCommonLayers` | layers plotted into every file after the file's own layer |
| `--drawing-sheet PATH` | `""` | `m_drawingSheet` | override drawing sheet (only matters with `--ibt`) |
| `-D, --define-var K=V` | none | var overrides | project text variables; `LAYER`, `SHEETNAME`, `SHEETPATH` are special (`H:L1880-1887`) |
| `--erd, --exclude-refdes` | off | `m_plotRefDes = !flag` | do not plot reference designators |
| `--ev, --exclude-value` | off | `m_plotFootprintValues = !flag` | do not plot footprint values |
| `--ibt, --include-border-title` | off | `m_plotDrawingSheet` | plot the drawing sheet into the Gerber (tier 3 in the port) |
| `--sp, --sketch-pads-on-fab-layers` | off | `m_sketchPadsOnFabLayers` | pad outlines on F.Fab/B.Fab |
| `--spn, --sketch-pad-numbers` | off | `m_plotPadNumbers = sp && flag` | pad numbers inside sketched pads |
| `--hdnp, --hide-DNP-footprints-on-fab-layers` | off | `m_hideDNPFPsOnFabLayers` | |
| `--sdnp, --sketch-DNP-footprints-on-fab-layers` | off | `m_sketchDNPFPsOnFabLayers` | note: the *job* default is `true`, the CLI always overwrites it with the flag, so the CLI default is `false` |
| `--cdnp, --crossout-DNP-footprints-on-fab-layers` | off | `m_crossoutDNPFPsOnFabLayers` | same remark (job default `true`, CLI default `false`) |
| `--no-x2` | off | `m_useX2Format = !flag` | X1: attributes as `G04 #@! ...` comments |
| `--no-netlist` | off | `m_includeNetlistAttributes = !flag` | no `TO.N/TO.P/TO.C` object attributes |
| `--subtract-soldermask` | off | `m_subtractSolderMaskFromSilk` | 4.6 |
| `--disable-aperture-macros` | off | `m_disableApertureMacros` | regions instead of `%AM` macros |
| `--use-drill-file-origin` | off | `m_useDrillOrigin` | plot offset = aux origin |
| `--precision N` | 6 | `m_precision` | must be 5 or 6, otherwise "Gerber coordinate precision should be either 5 or 6" -> `ERR_ARGS`. (Job default is 5; the CLI always sets it) |
| `--no-protel-ext` | off | `m_useProtelFileExtension = !flag` | default: Protel extensions (`.gtl`, `.gbl`, ...); with the flag every file is `.gbr` |
| `--check-zones` | off | `m_checkZonesBeforePlot` | refill all zones before plotting (needs `kicad-zone`, tier 2) |
| `--variant NAME` | none | `m_variant` (first only) | `BOARD::SetCurrentVariant` -> DNP state per variant |
| `--board-plot-params` | off | `m_useBoardPlotParams` | take everything from the board's `(pcbplotparams ...)` |

`JobExportGerbers` then does, in this order:

1. Load the board (`getBoard`, `H:L540-629`: project + `BOARD_LOADER::Load`; see `pcb-io.md` 4.3 for
   what post-load initialisation does). Set the variant.
2. `resolveJobOutputPath` (`H:L3698-3713`): copy the title block into the job, load the override
   drawing sheet, apply `-D` variables to the project, `SynchronizeProperties`. Create the
   output directory.
3. If `--check-zones`: `ZONE_FILLER_TOOL::FillAllZones`. **Without the flag nothing checks the
   zone fill state**; the fills stored in the file are plotted as they are (an unfilled zone
   plots nothing).
4. Layer selection (`H:L1784-1830`):
   * `m_argLayers` is always set by the CLI. Non-empty: `convertLayerArg` (below), and
     `hasLayerListSpecified = true`. Empty: `LSET::AllLayersMask().SeqStackupForPlotting()`.
   * `--board-plot-params`: the layer list is **replaced** by
     `(boardOpts.GetLayerSelection() & AllNonCuMask) | (enabledLayers & AllCuMask)` (all enabled
     copper layers are always plotted, whatever the stored selection says), and the common layers
     by `boardOpts.GetPlotOnAllLayersSequence()`.
   * Finally `layersToPlot = LSET( ResolvePlotLayers( enabled ) ) & enabled`
     (`job_export_pcb_plot.cpp:L122-131`; `m_plotAllCopperLayers` is false from the CLI).
5. `RefreshGeneratedTables( *brd )` (drill charts / stackup tables are rebuilt; tier 3, see 11).
6. **For each `layer` in `layersToPlot.UIOrder()`** (copper front to back, then
   `TechAndUserUIOrder`): build `plotSequence = [layer] + common layers not already present`
   (`H:L1837-1850`); build `PCB_PLOT_PARAMS` (board's, or `PlotJobToPlotOpts`, 1.5); choose the
   extension (`GetGerberProtelExtension( layer )` or `gbr`); build the file name with
   `BuildPlotFileName( fn, outDir, brd->GetLayerName( layer ), ext )` (6.1); register the file
   with the job-file writer; `StartPlotBoard( brd, &plotOpts, layer, layerName, path, sheetName,
   sheetPath )` (4.5); `PlotBoardLayers( brd, plotter, plotSequence, plotOpts )` (4.6);
   `plotter->EndPlot()`. A failing file reports "Failed to plot to '...'" and sets the exit code
   to `ERR_INVALID_OUTPUT_CONFLICT` but the loop continues.
7. If `m_createJobsFile` (job default `true`, no CLI flag; **not** taken from the board's
   `creategerberjobfile` even with `--board-plot-params`): write `<board>-job.gbrjob` (4.16).

`convertLayerArg` (`H:L632-696`): split at `,`, trim both ends, then look the token up in three
maps in this order: (1) the board's user layer names (`BOARD::GetLayerName`), (2) canonical file
names `LSET::Name` plus the groups `*`, `*.Cu`, `*In.Cu`, `F&B.Cu`, `*.Adhes`, `*.Paste`, `*.Mask`,
`*.SilkS`, `*.Fab`, `*.CrtYd`, (3) GUI names `LayerName()` plus `*.Adhesive`, `*.Silkscreen`,
`*.Courtyard`. A hit appends `mask.Seq()` (ascending layer id) to the result; a miss reports
`Invalid layer name '%s'` and continues. Duplicates are kept in the list (the later `LSET`
conversion removes them for `gerbers`; for SVG single mode they survive).

### 1.3 `kicad-cli pcb export drill`

Options (`command_pcb_export_drill.cpp:L48-119`), validation and mapping (`L122-283`):

| Flag | Default | Job member | Values / effect |
|---|---|---|---|
| positional input | required | `m_filename` | |
| `-o` | `""` | output dir | must be a directory ("Output must be a directory") |
| `--format` | `excellon` | `m_format` | `excellon`, `gerber`; else "Invalid drill format" |
| `--drill-origin` | `absolute` | `m_drillOrigin` | `absolute` -> offset (0,0); `plot` -> aux origin |
| `--excellon-zeros-format` | `decimal` | `m_zeroFormat` | `decimal`, `suppressleading`, `suppresstrailing`, `keep` |
| `--excellon-oval-format` | `alternate` | `m_excellonOvalDrillRoute` | `route` -> true (G00/M15/G01/M16 routing), `alternate` -> false (**G85 canned slot is the CLI default**) |
| `-u, --excellon-units` | `mm` | `m_drillUnits` | `mm`, `in` |
| `--excellon-mirror-y` | off | `m_excellonMirrorY` | Y not negated |
| `--excellon-min-header` | off | `m_excellonMinimalHeader` | no comment lines in header, no hole attribute comments |
| `--excellon-separate-th` | off | `m_excellonCombinePTHNPTH = !flag` | default: **one merged file** for PTH + NPTH |
| `--generate-map` | off | `m_generateMap` | drill map drawing per file |
| `--map-format` | `pdf` | `m_mapFormat` | `pdf`, `gerberx2`, `ps`, `dxf`, `svg` (port: `gerberx2`, `svg`) |
| `--generate-report` | off | `m_generateReport` | text report |
| `--report-path` | `""` | `m_reportPath` | default `<outdir>/<board>-drill.rpt` (`H:L2370-2378`) |
| `--generate-tenting` | off | `m_generateTenting` | Gerber format only: also write tenting protection files |
| `--gerber-precision` | 6 | `m_gerberPrecision` | 5 or 6 |

`JobExportDrill`: load board, resolve output dir, offset, map format, then

* Excellon: `SetFormat( metric, zeroFmt, lhs, rhs )` with precision 2:4 (inch) or 3:3 (mm)
  (`H:L2316-2317`), `SetOptions( mirror, minimalHeader, offset, mergePTHNPTH )`,
  `SetRouteModeForOvalHoles`, `SetMapFileFormat`, `CreateDrillandMapFilesSet( dir, true, genMap )`,
  then `GenDrillReportFile` if requested.
* Gerber: `SetFormat( precision )`, `SetOptions( offset )`, `SetMapFileFormat`,
  `CreateDrillandMapFilesSet( dir, true, genMap, genTenting )`, then the report.

No zone filling, no variant handling, no drawing sheet.

### 1.4 `kicad-cli pcb export svg`

Options (`command_pcb_export_svg.cpp:L37-135`), mapping (`L138-209`):

| Flag | Default | Effect |
|---|---|---|
| `-l/--layers`, `--cl/--common-layers`, `--drawing-sheet`, `-D`, `--variant`, `--check-zones` | as for gerbers | via `preparePlotLayers` (`H:L1464-1490`); an empty resulting layer list is an error "At least one layer must be specified" |
| `--subtract-soldermask` | off | `PlotClippedSilkLayer` (4.11) |
| `-m, --mirror` | off | mirror horizontally |
| `-t, --theme NAME` | `""` | colour theme (PCB editor settings if empty) |
| `-n, --negative` | off | 4.5 |
| `--black-and-white` | off | colour mode off |
| `--sp`, `--spn`, `--hdnp`, `--sdnp`, `--cdnp` | off | as for gerbers |
| `--page-size-mode N` | 0 | legacy: 2 sets `m_fitPageToBoard = true`. Its effect on `m_plotDrawingSheet` is **overwritten** a few lines later by `--exclude-drawing-sheet` (`L187`) |
| `--fit-page-to-board` | off | page = board bounding box |
| `--exclude-drawing-sheet` | off | `m_plotDrawingSheet = !flag`: **the drawing sheet is plotted by default** |
| `--drill-shape-opt N` | 2 | 0 none, 1 small, 2 actual size drill marks |
| `--mode-single` / `--mode-multi` | neither | neither: job default `SINGLE` plus a deprecation warning on stdout. Single: `-o` is the file name, the first layer is the file's layer and all other `--layers` become common layers (`PL:L123-129`). Multi: `-o` is a directory, one file per layer named `<board>-<layer>.svg` |
| `--scale S` | 1.0 | 0 = autoscale (`SetAutoScale( !scale )`) |

`JobExportSvg` -> `PlotJobToPlotOpts` -> `PCB_PLOTTER::Plot( outPath, layers, common, false,
isSingle, layerName?, sheetName?, sheetPath?, &outputs )`. For the browser preview the port calls
the equivalent of `--mode-single --exclude-drawing-sheet [--fit-page-to-board]`.

### 1.5 `PCB_PLOTTER::PlotJobToPlotOpts` (`PL:L426-555`): job -> `PCB_PLOT_PARAMS`

Starts from a default-constructed `PCB_PLOT_PARAMS` (3.5) and sets:

* Gerber: `DisableGerberMacros`, `UseGerberProtelExtensions`, `UseGerberX2format`,
  `IncludeGerberNetlistInfo`, `CreateGerberJobFile`, `SetGerberPrecision( n )` (5 stays 5, anything
  else becomes 6), `DrillMarksType = NO_DRILL_SHAPE`.
* Non-Gerber: `Scale`, `AutoScale = !scale`, `ScaleSelection`, `DrillMarksType` from the job.
* SVG: `SvgPrecision` (clamped 3..6; job default 4), `SvgFitPageToBoard`.
* All: `UseAuxOrigin = m_useDrillOrigin`, `PlotFrameRef`, `SubtractMaskFromSilk`, `PlotReference`,
  `PlotValue`, the four fab-layer flags, `PlotPadNumbers`, `BlackAndWhite`, `Mirror`, `Negative`,
  `LayerSelection`, `PlotOnAllLayersSequence`, `Format`, `ColorSettings`, `OutputDirectory`.
* Not touched, hence defaults: `m_plotFPText = true`, `m_sketchPadLineWidth = 0.1 mm`,
  `m_skipNPTH_Pads = false`, `m_textMode = DEFAULT`, `m_A4Output = false`, dash ratios 12 / 3.

The colour theme lookup (`GetColorSettings( theme )`) reads user configuration; in the port the
colours are an explicit parameter (5.6).

### 1.6 Entry points the port should expose (proposal)

```rust
// kicad-fab
pub struct OutputFile { pub name: String, pub contents: Vec<u8> }   // no filesystem access
pub struct PlotEnv   { pub build_version: String,       // GetBuildVersion(), e.g. "10.99.0-unknown"
                       pub now: PlotTimestamp }         // wall clock, passed in by the caller
pub fn export_gerbers(cx: &BoardCtx, job: &GerbersJob, env: &PlotEnv, rep: &mut dyn Reporter)
        -> Result<Vec<OutputFile>, FabError>;           // JobExportGerbers
pub fn export_drill(cx: &BoardCtx, job: &DrillJob, env: &PlotEnv, rep: &mut dyn Reporter)
        -> Result<Vec<OutputFile>, FabError>;           // JobExportDrill
pub fn export_svg(cx: &BoardCtx, job: &SvgJob, colors: &dyn LayerColors, env: &PlotEnv,
                  rep: &mut dyn Reporter) -> Result<Vec<OutputFile>, FabError>; // JobExportSvg
```

`BoardCtx` must carry connectivity (pad/via flashing on copper layers depends on it, 5.4). The job
structs mirror the C++ job members one to one, with the C++ *job* defaults; `kicad-cli` applies the
*CLI* defaults of 1.2-1.4.

---

## 2. File inventory

### 2.1 In scope

| File | Lines | Role |
|---|---|---|
| `include/plotters/plotter.h` | 789 | `PLOTTER` abstract base, `PLOT_FORMAT`, `PLOT_TEXT_MODE`, `PLOT_PARAMS` (DXF hook, drop), pen-width sentinels |
| `common/plotters/plotter.cpp` | 667 | base implementations: user->device transform, generic arcs (`polyArc`), Bezier, markers, `ThickSegment/Arc/Rect/Circle/Oval/Poly`, `PlotPoly( SHAPE_LINE_CHAIN )`, `Text`, `PlotText` |
| `include/plotters/plotter_gerber.h` | 398 | `GERBER_PLOTTER` |
| `common/plotters/GERBER_plotter.cpp` | 1919 | whole Gerber engine incl. `APER_MACRO_FREEPOLY(_LIST)` at the end |
| `include/plotters/gbr_plotter_apertures.h` | 216 | `APERTURE` (types, D-code), `APER_MACRO_FREEPOLY`, `APER_MACRO_FREEPOLY_LIST`, `FIRST_DCODE_VALUE = 10` |
| `include/plotters/gbr_plotter_aperture_macros.h` | 157 | the fixed `%AM...` macro texts (`RoundRect`, `RotRect`, `HorizOval`, `Outline4P`..`Outline8P`) - copy verbatim |
| `include/gbr_metadata.h`, `include/gbr_netlist_metadata.h` | 350, 261 | `GBR_METADATA`, `GBR_APERTURE_METADATA` (attribute enum), `GBR_NETLIST_METADATA`, `GBR_DATA_FIELD`, `GBR_CMP_PNP_METADATA` (placement files, not needed) |
| `common/gbr_metadata.cpp` | 803 | creation-date string, project GUID, `TA.AperFunction` strings, Gerber string escaping, `FormatNetAttribute` |
| `common/plotters/SVG_plotter.cpp` | 1019 | `SVG_PLOTTER` |
| `include/plotters/plotters_pslike.h` | 867 | only `PSLIKE_PLOTTER` (L41-143) and `SVG_PLOTTER` (L712-867) |
| `common/plotters/PS_plotter.cpp` | 1117 | only `PSLIKE_PLOTTER::SetColor` and `FlashPad*` (L45-226), inherited by SVG |
| `common/plotters/drill_markers.cpp`, `include/plotters/drill_markers.h` | 219, 88 | marker pattern table and `BuildMarker` (drill maps) |
| `common/plotters/common_plot_functions.cpp` | 207 | `GetDefaultPlotExtension`; `PlotDrawingSheet` (tier 3) |
| `common/line_ending.cpp` (L62-190, L240-265), `include/line_ending.h` | 336, 175 | `LINE_ENDING::GetEffectiveSize/GetShortenDepth/GetShapes/Plot` (arrow heads etc. on open shapes; new in this format). Data + parser are in `pcb-io.md` |
| `pcbnew/pcbplot.h` | 296 | `BRDITEMS_PLOTTER` declaration and the free plot functions |
| `pcbnew/pcbplot.cpp` | 372 | Protel extensions, `TF.FileFunction`, `TF.FilePolarity`, `AddGerberX2Header`, `AddGerberX2Attribute` |
| `pcbnew/pcb_plot_params.h/.cpp` | 316, 899 | options class. Struct, defaults, `Format`, parser are ported in `kicad-pcb` (`pcb-io.md` PCBIO-04); this document specifies what each option *does* (3.5) |
| `pcbnew/plotprint_opts.h` | 35 | `DRILL_MARKS { NO_DRILL_SHAPE, SMALL_DRILL_SHAPE, FULL_DRILL_SHAPE }` |
| `pcbnew/plot_board_layers.cpp` | 1641 | `PlotBoardLayers`, `PlotOneBoardLayer`, `PlotStandardLayer`, `PlotSolderMaskLayer`, `PlotClippedSilkLayer`, `GenerateLayerPoly`, `PlotPolySet`, `initializePlotter`, `StartPlotBoard`, negative knockout |
| `pcbnew/plot_brditems_plotter.cpp` | 1656 | `BRDITEMS_PLOTTER`: pads, pad numbers, text, shapes, zones, dimensions, tables, barcodes, targets, drill marks, drill symbols |
| `pcbnew/pcb_plotter.h/.cpp` | 111, 606 | `PCB_PLOTTER`: `Plot` (multi-file driver used by SVG), `BuildPlotFileName`, `getPlotSequence`, `PlotJobToPlotOpts`, `copperLayerShouldBeSkipped` |
| `pcbnew/drill/drill_span.h`, `drill_operation.h`, `drill_enumerator.h/.cpp` | 134, 149, 58, 352 | the drill model: which holes exist, per span |
| `pcbnew/exporters/gendrill_writer_base.h/.cpp` | 395, 1070 | tool list, file names, `TF.FileFunction` for drill files, drill map, drill report |
| `pcbnew/exporters/gendrill_excellon_writer.h/.cpp` | 172, 789 | Excellon |
| `pcbnew/exporters/gendrill_gerber_writer.h/.cpp` | 128, 633 | Gerber X2 drill and IPC-4761 protection files |
| `pcbnew/exporters/gerber_jobfile_writer.h/.cpp` | 173, 772 | `.gbrjob` |
| `common/jobs/job_export_pcb_plot.h/.cpp`, `job_export_pcb_gerber(s).h/.cpp`, `job_export_pcb_drill.h/.cpp`, `job_export_pcb_svg.h/.cpp` | ~600 | job members and defaults (the JSON `m_params` are jobset persistence: out of scope) |
| `kicad/cli/command_pcb_export_base.h/.cpp`, `command_pcb_export_gerber(s).cpp`, `command_pcb_export_drill.cpp`, `command_pcb_export_svg.cpp` | ~900 | CLI option tables (1.2-1.4) |
| `pcbnew/pcbnew_jobs_handler.cpp` | L632-696, L1464-1579, L1750-1923, L2214-2472, L3698-3713 | job handlers |

### 2.2 Out of scope (record in `PORT_STATUS.md`)

| File / class | Why |
|---|---|
| `DXF_plotter.cpp`, `PDF_plotter.cpp`, `pdf_*.cpp`, `PNG_plotter.cpp`, `PS_plotter.cpp` except L45-226, `plotter_dxf.h`, `plotter_png.h` | other output formats. Consequence: every `GetPlotterType() == PLOT_FORMAT::DXF / PDF / POST / PNG` branch in the board plot code is dead in the port; keep the `match` arms with a comment so the mapping stays visible |
| `PlotLayerOutlines` (`L:L937-1017`), `BOARD::ConvertBrdLayerToPolygonalContours`, `PlotLayer`'s DXF branch (`L:L53-61`) | "designed only for DXF plotters" |
| `PlotInteractiveLayer` (`L:L174-244`), `PLOTTER::HyperlinkBox/HyperlinkMenu/Bookmark`, `MarkFootprintPlotted/WasFootprintPlotted` | PDF popups and bookmarks; no-ops for Gerber and SVG |
| `setupPlotterNewPDFPage`, `plotBackgroundColor`, PDF/PNG parts of `StartPlotBoard` and `PCB_PLOTTER::Plot` | PDF / PNG |
| `PLOTTER::PlotImage`, `SVG_PLOTTER::PlotImage` | bitmaps: only reached from the drawing sheet (reference images on a board are never plotted, `B:L758-760`) |
| `PCB_PLOT_PARAMS::IsSameAs`, `m_scaleSelection`, `PCB_PLOTTER::DialogLayerSelection` | GUI dialog support |
| `pcbnew/dialogs/dialog_plot*.cpp`, `dialog_gendrill.cpp`, `pcbnew/plotcontroller.cpp` | GUI / Python |
| `gerber_placefile_writer.cpp`, `place_file_exporter.cpp`, `export_d356.cpp`, `fab_model/`, IPC-2581, ODB++ | other exporters |
| `GBR_CMP_PNP_METADATA`, `FormatStringFromGerber` | placement files / Gerber reader |
| `EXPRESSION_EVALUATOR` (`@{...}` in text, `P:L566-570`, `P:L629-633`, `S:L904-908`) | text expression evaluation; out of tier 1-2 (see 11) |
| `PlotDrawingSheet` + `common/drawing_sheet/*` (3959 lines) | tier 3 stretch (PLOT-07, PLOT-08) |
| `PlotDrillSymbols`, `PlotChartSymbols` (`B:L1515-1656`), `drill_symbol_*`, `drill_chart_*`, `pcb_drill_map.cpp`, `pcb_drill_chart.cpp`, `board_tables/*`, `RefreshGeneratedTables` | tier 3 stretch (FAB-13); the model design keeps these items as data only |
| Job JSON persistence (`JOB_PARAM*`, `FromJson`), jobsets, `REGISTER_JOB` | not needed for a CLI |

---

## 3. Data structures

### 3.1 `PLOTTER` (`PH:L137-771`)

State shared by all engines (`PH:L724-770`):

| Member | Init (`P:L46-64`) | Meaning |
|---|---|---|
| `m_plotScale` | 1 | user scale (Gerber: always 1) |
| `m_IUsPerDecimil` | 1 | caller scale; pcbnew passes `pcbIUScale.IU_PER_MILS / 10 = 2540` |
| `m_iuPerDeviceUnit` | 1 | IU -> device unit factor, set by `SetViewport` |
| `m_plotOffset` | (0,0) | IU, subtracted from every coordinate |
| `m_plotMirror`, `m_mirrorIsHorizontal` (true), `m_yaxisReversed` (false; SVG sets true) | | |
| `m_paperSize` | | paper size **in IU**; Gerber sets (0,0) |
| `m_outputFile` | | C `FILE*`; the port writes into an in-memory buffer |
| `m_colorMode` (false), `m_negativeMode` (false) | | |
| `m_currentPenWidth` | -1 | last width given to `SetCurrentLineWidth` |
| `m_penState` ('Z'), `m_penLastpos` | | used by SVG `PenTo` |
| `m_creator`, `m_filename`, `m_title`, `m_author`, `m_subject` | | header strings |
| `m_pageInfo` | | `PAGE_INFO` |
| `m_headerExtraLines` | | lines printed first by `GERBER_PLOTTER::StartPlot` |
| `m_renderSettings` | null | `RENDER_SETTINGS*`: default pen width, dash/gap ratios, default font, layer name |
| `m_project`, `m_layersToExport`, `m_layer`, `m_plottedFootprints` | | PDF/DXF only |

Sentinels: `DO_NOT_SET_LINE_WIDTH = -2`, `USE_DEFAULT_LINE_WIDTH = -1` (`PH:L141-142`).
`GetPlotterArcLowDef() = m_IUsPerDecimil * 8` (20320 IU), `GetPlotterArcHighDef() = m_IUsPerDecimil
* 2` (5080 IU) (`PH:L290-291`, result truncated to `int`).

The `void* aData` parameter threaded through most calls is a `GBR_METADATA*` for Gerber, a
`PLOT_PARAMS*` for DXF and null otherwise. In the port make it `Option<&GbrMetadata>`.

Virtual interface to keep (trait `Plotter`): `GetPlotterType`, `StartPlot`, `EndPlot`,
`SetNegative`, `SetColorMode`, `SetCurrentLineWidth`, `SetColor`, `SetDash`, `SetViewport`, `Rect`,
`Circle`, `Arc` (two overloads), `BezierCurve`, `PenTo` (+ `MoveTo/LineTo/FinishTo/PenFinish`),
`PlotPoly` (two overloads), `ThickSegment`, `ThickArc`, `ThickRect`, `ThickCircle`, `FilledCircle`,
`ThickOval`, `ThickPoly`, `FlashPadCircle/Oval/Rect/RoundRect/Custom/Trapez`,
`FlashRegularPolygon`, `Text`, `PlotText`, `Marker`, `SetLayerPolarity`, `SetTextMode`,
`SetGerberCoordinatesFormat`, `SetSvgCoordinatesFormat`, `SetPlotBBox`, `StartBlock`, `EndBlock`.
The board code also down-casts to `GERBER_PLOTTER` for `PlotGerberRegion`, `PlotPolyAsRegion`,
`FlashPadChamferRoundRect`, `UseX2format`, `UseX2NetAttributes`, `DisableApertMacros`; give the
trait an `as_gerber_mut() -> Option<&mut GerberPlotter>` or make the board plotter generic with an
enum of engines.

`PLOTTER::ThickArc( const EDA_SHAPE&, ... )` (`P:L484-506`) has no caller in the board plot code;
skip it (it would create a dependency from `kicad-plot` on the board model).

### 3.2 `GERBER_PLOTTER` (`GH:L29-398`)

| Member | Init (`G:L94-121`) | Meaning |
|---|---|---|
| `m_objectAttributesDictionary` | "" | the text of the `TO.*` attributes currently in force (exactly the lines last "printed", joined), used to print only changes |
| `m_apertureAttribute` | 0 | last aperture attribute written by `writeApertureList` |
| `workFile`, `finalFile`, `m_workFilename` | | body is written to a temp file, copied to the real file by `EndPlot` with the aperture list spliced in. Port: body buffer + splice |
| `m_apertures` | empty | `std::vector<APERTURE>` in creation order; D-codes are assigned `10, 11, ...` in creation order |
| `m_currentApertureIdx` | -1 | index of the selected aperture |
| `m_hasApertureRoundRect`, `m_hasApertureRotOval`, `m_hasApertureRotRect`, `m_hasApertureOutline4P`, `m_hasApertureChamferedRect` | false | which fixed macros must be defined in the header (reset by `StartPlot`) |
| `m_gerberUnitInch` | false | always false from pcbnew |
| `m_gerberUnitFmt` | 6 | digits after the decimal point (5 or 6) |
| `m_gerberDisableApertMacros` | false | |
| `m_useX2format` | true | false: attributes as `G04 #@! ` comments |
| `m_useNetAttributes` | true | false: `formatNetAttribute` does nothing |
| `m_am_freepoly_list` | empty | `APER_MACRO_FREEPOLY_LIST`: polygons of on-the-fly macros `FreePoly0, FreePoly1, ...` in creation order |

`APERTURE` (`gbr_plotter_apertures.h:L44-144`): `m_Type` (enum values matter only for
`AT_REGULAR_POLY3 + n - 3` arithmetic), `m_Size`, `m_Corners`, `m_Radius`, `m_Rotation`
(`EDA_ANGLE`, compared with `==` on the stored double), `m_DCode`, `m_ApertureAttribute` (int value
of `GBR_APERTURE_ATTRIB`), `m_CustomAttribute`. `GetDiameter()`: `m_Size.x` for `AT_CIRCLE` and
`AT_PLOTTING`, else `m_Radius * 2`.

Note there are two distinct circle types: `AT_PLOTTING` (line drawing) and `AT_CIRCLE` (flash).
A 0.2 mm trace aperture and a 0.2 mm round pad are two different D-codes even with identical
attributes.

### 3.3 Gerber metadata

`GBR_METADATA` (`gbr_metadata.h:L218-293`) = `GBR_APERTURE_METADATA` (attribute id + custom string)
+ `GBR_NETLIST_METADATA` + `m_isCopper` (set by callers, never read in scope).

`GBR_NETLIST_METADATA` (`gbr_netlist_metadata.h:L147-253`): `m_NetAttribType` (bit set: `PAD = 1`,
`NET = 2`, `CMP = 4`, 0 = unspecified), `m_NotInNet`, `m_Padname`, `m_PadPinFunction`
(`GBR_DATA_FIELD` = string + `useUTF8` + `escapeString` flags), `m_Cmpref`, `m_Netname`,
`m_ExtraData` (never set in scope), `m_TryKeepPreviousAttributes` (always false in scope).

`GBR_APERTURE_ATTRIB` enum and its strings: `M:L153-358`. Port the full table.

### 3.4 `SVG_PLOTTER` (`plotters_pslike.h:L712-867`)

Adds: `m_fillMode` (`FILL_T`), `m_pen_rgb_color`, `m_brush_rgb_color` (24-bit), `m_brush_alpha`,
`m_graphics_changed` (lazy style emission), `m_dashed` (`LINE_STYLE`), `m_precision` (digits, 4 by
default, set from `svgprecision`), `m_plotBBoxIU` (`optional<BOX2I>`); from `PSLIKE_PLOTTER`:
`plotScaleAdjX/Y` (unused by SVG), `m_textMode` (SVG forces `STROKE`).

### 3.5 `PCB_PLOT_PARAMS`: every option

Defaults from the constructor (`pcb_plot_params.cpp:L47-112`). "Token" is the `(pcbplotparams)`
keyword (`Format`, `L133-203`); options without a token are not persisted.

| Member | Default | Token | Used by (in scope) |
|---|---|---|---|
| `m_format` | `GERBER` | `outputformat` (int: HPGL 0, GERBER 1, POST 2, DXF 3, PDF 4, SVG 5, PNG 6) | `StartPlotBoard` engine choice; many branches |
| `m_layerSelection` | F/B SilkS, F/B Mask, F/B Paste, Edge.Cuts + all copper | `layerselection 0x<hex>` | `--board-plot-params` (non-copper part only) |
| `m_plotOnAllLayersSequence` | empty | `plot_on_all_layers_selection 0x<hex>` (stored as a set, read back through `SeqStackupForPlotting()`) | common layers with `--board-plot-params` |
| `m_gerberDisableApertMacros` | false | `disableapertmacros` | `GERBER_PLOTTER::DisableApertMacros` |
| `m_useGerberProtelExtensions` | false | `usegerberextensions` | file extension |
| `m_useGerberX2format` | true | `usegerberattributes` | `UseX2format`, header syntax |
| `m_includeGerberNetlistInfo` | true | `usegerberadvancedattributes` | `UseX2NetAttributes` |
| `m_createGerberJobFile` | true | `creategerberjobfile` | only `PCB_PLOTTER::Plot`; **not** `JobExportGerbers` |
| `m_gerberPrecision` | 6 | `gerberprecision` (written only when != 6) | `SetGerberCoordinatesFormat` |
| `m_dashedLineDashRatio`, `m_dashedLineGapRatio` | 12.0, 3.0 | `dashed_line_dash_ratio`, `dashed_line_gap_ratio` | render settings -> `STROKE_PARAMS::Stroke`, SVG dash arrays |
| `m_svgPrecision` | 4 | `svgprecision` | `SetSvgCoordinatesFormat` |
| `m_svgFitPageToBoard` | false | - | `PCB_PLOTTER::Plot`, `initializePlotter` |
| `m_pngDPI`, `m_pngAntialias` | 300, true | `pngdpi`, `pngantialias` | no |
| `m_plotDrawingSheet` | false | `plotframeref` | `StartPlotBoard` |
| `m_DXFPlotMode`, `m_DXFPolygonMode`, `m_DXFUnits`, `m_DXFExportAsMultiLayeredFile`, `m_layersToExport` | FILLED, true, INCH, false | `mode` (written `1`/`2`, ignored on read), `dxfpolygonmode`, `dxfimperialunits` | no (DXF) |
| `m_textMode` | `DEFAULT` | `dxfusepcbnewfont` (false -> `NATIVE`) | `SetTextMode`; no effect for Gerber/SVG |
| `m_useAuxOrigin` | false | `useauxorigin` | plot offset; **also read from the board's stored params** for `TF.SameCoordinates` (11) |
| `m_negative` | false | `psnegative` | `FillNegativeKnockout` |
| `m_A4Output` | false | `psa4output` | `initializePlotter` |
| `m_blackAndWhite` | true | `plot_black_and_white` | `SetColorMode( !bw )` |
| `m_sketchPadsOnFabLayers` | false | `sketchpadsonfab` | `PlotStandardLayer` |
| `m_plotPadNumbers` | false | `plotpadnumbers` | `PlotStandardLayer` |
| `m_hideDNPFPsOnFabLayers` | false | `hidednponfab` | text/graphics of DNP footprints on F.Fab/B.Fab |
| `m_sketchDNPFPsOnFabLayers` | true | `sketchdnponfab` | `PlotShape` |
| `m_crossoutDNPFPsOnFabLayers` | true | `crossoutdnponfab` | DNP cross, reference strike-out |
| `m_subtractMaskFromSilk` | false | `subtractmaskfromsilk` | `PlotOneBoardLayer` |
| `m_mirror` | false | `mirror` | `SetViewport` |
| `m_drillMarks` | `SMALL_DRILL_SHAPE` | `drillshape` (0..2) | `PlotBoardLayers` forces `NO_DRILL_SHAPE` for Gerber |
| `m_scaleSelection` | 1 | `scaleselection` | GUI only |
| `m_outputDirectory` | "" | `outputdirectory` | GUI only |
| `m_plotReference`, `m_plotValue`, `m_plotFPText` | true, true, true | - | footprint text |
| `m_sketchPadLineWidth` | 0.1 mm (100000 IU) | - | sketch pads, DNP sketch |
| `m_skipNPTH_Pads` | false | - | set per layer by `PlotOneBoardLayer` |
| `m_autoScale`, `m_scale` | false, 1.0 | - | `initializePlotter` |
| `m_fineScaleAdjustX/Y`, `m_widthAdjust` | 1.0, 1.0, 0 | - | PostScript only; `getFineWidthAdj()` is 0 for every other format (`pcbplot.h:L75-81`) |
| `m_plotViaOnMaskLayer` | none | `viasonmask` (read only) | consumed by the board loader (7) |
| `m_PDF*`, `m_backgroundColor` | | `pdf_*` | no |
| `m_colors` | default `COLOR_SETTINGS` | - | `SetColor` calls: no-ops for Gerber |
| `m_layer` | | - | no |

### 3.6 `BRDITEMS_PLOTTER` (`pcbplot.h:L61-162`)

*Is-a* `PCB_PLOT_PARAMS` (copy of the options) + `m_plotter` + `m_board` + `m_layerMask` (the
layers being plotted, set with `SetLayerSet`). One instance is created per call of
`PlotStandardLayer`, `PlotPolySet`, `PlotBoardLayers` (drill marks). No caches.

### 3.7 Drill model

`DRILL_SPAN` (`drill_span.h:L48-131`): `m_StartLayer`, `m_EndLayer`, `m_IsBackdrill`,
`m_IsNonPlatedFile`. `TopLayer()/BottomLayer()` use `IsCopperLayerLowerThan` (B_Cu is lowest
although its id is 2), `Pair() = (Top, Bottom)`. `operator<` (used by the `std::set` in
`EnumerateDrillSpans`): by `TopLayer()` id, then `BottomLayer()` id (numeric ids, so B_Cu = 2 sorts
before inner layers), then backdrill first, then non-plated first, then stored start, stored end.
`operator==` compares the stored layers (a span and its reverse differ).

`DRILL_OPERATION` (`drill_operation.h:L79-125`): one hole. `m_Kind` (primary / secondary /
tertiary), `m_SourceItem` (+ `m_SourceId`), `m_Position`, `m_SizeXY` (source axes, not
normalised), `m_Orientation`, `m_Diameter` (the tool: smaller of X/Y), `m_IsSlot`, top/bottom
layer, drill start/end, `m_NotPlated`, `m_Attribute` (`HOLE_ATTRIBUTE`, enum order is a sort key:
`UNKNOWN < VIA_THROUGH < VIA_BURIED < VIA_BACKDRILL < PAD < PAD_CASTELLATED < PAD_PRESSFIT <
MECHANICAL < PAD_BACKDRILL`), eight IPC-4761 flags, front/back `DRILL_POST_MACHINING`,
`m_StubLength`. The writers test `dyn_cast<PCB_VIA*>( m_SourceItem )`: in the port store an enum
`DrillSource::{Via(handle), Pad(handle)}`.

`DRILL_QUERY`: `m_Span`, `m_NonPlatedOnly`, `m_MergePTHNPTH`, `m_PadMachining` (false for the NC
writers).

`DRILL_TOOL` (`gendrill_writer_base.h:L65-90`): `m_Diameter`, `m_TotalCount`, `m_OvalCount`,
`m_Hole_NotPlated`, `m_HoleAttribute`, `m_IsBackdrill`, `m_HasPostMachining`,
`m_MinStubLength/m_MaxStubLength`.

`GENDRILL_WRITER_BASE` state (`gendrill_writer_base.h:L371-392`): `m_pcb`, `m_drillFileExtension`
(`drl` / `gbr`), `m_unitsMetric`, `m_zeroFormat`, `m_precision` (`m_Lhs:m_Rhs`),
`m_conversionUnits`, `m_offset`, `m_merge_PTH_NPTH`, `m_holeListBuffer` (sorted operations),
`m_holeToolReferences` (1-based tool number per hole), `m_toolListBuffer`, `m_mapFileFmt`,
`m_pageInfo`, `m_createdFiles`.

### 3.8 Job file

`JOBFILE_PARAMS`: two parallel vectors, file name (no path) and layer id, in the order the files
were plotted. `GERBER_JOBFILE_WRITER::m_json` is an `nlohmann::ordered_json` (insertion order is
the output order).

---

## 4. Control flow and algorithms

### 4.1 Coordinates, scaling and number conversion

All plot calls take board coordinates in IU (nm, Y down). `PLOTTER::userToDeviceCoordinates`
(`P:L91-113`), in this exact expression order:

```
pos = aCoordinate - m_plotOffset                       (integer)
x = pos.x * m_plotScale
y = m_paperSize.y - pos.y * m_plotScale
if m_plotMirror:  horizontal: x = m_paperSize.x - pos.x * m_plotScale   else: y = pos.y * m_plotScale
if m_yaxisReversed: y = m_paperSize.y - y
x *= m_iuPerDeviceUnit;  y *= m_iuPerDeviceUnit
```

`userToDeviceSize( v ) = v * m_plotScale * m_iuPerDeviceUnit` (`P:L116-126`).

**Gerber** (`G:L124-155`): `SetViewport` ignores scale and mirror (scale forced to 1, paper size
(0,0)), so `x_dev = (x - off.x) * k`, `y_dev = -(y - off.y) * k`: the Y axis is flipped, there is
no page. `k = m_iuPerDeviceUnit = pow( 10.0, fmt ) / ( m_IUsPerDecimil * 10000.0 )`, then
`k *= 25.4` in `SetGerberCoordinatesFormat` (always mm from pcbnew). With `m_IUsPerDecimil = 2540`
these doubles are **not** exact: `fmt = 6` gives `k = 0.9999999999999999`, `fmt = 5` gives
`k = 0.09999999999999999`. Coordinates are emitted as `KiROUND( x_dev )` (`G:L158-161`; `KiROUND`
is `std::llround`, half away from zero on the exact double). With 4.6 this still yields the
integer nm value for every `int`. With 4.5 a coordinate ending in 5 nm becomes either exactly
`n.5` or the double just below it, depending on its magnitude (15 -> `1.4999999999999998` -> 1,
but 100005 -> `10000.5` -> 10001): the rounding direction of half cases is data dependent, and
only the identical arithmetic reproduces it. The port must compute `k` with the same two steps
and multiply; never divide by 10 or use integer arithmetic. `KiROUND( -0.0 )` is 0 and prints
`0`, never `-0`.

Aperture dimensions use a different factor, `fscale = 0.0001 * m_plotScale / m_IUsPerDecimil`
then `*= 25.4` (`G:L620-623`), which evaluates to exactly `1e-06`; values are printed with fmt's
`{:#f}`, i.e. always 6 decimals with a decimal point (6.2).

**SVG** (`S:L178-227`): device unit is the millimetre. `m_paperSize = pageInfo.GetSizeMils()`
(a `VECTOR2D` in mils assigned to a `VECTOR2I`: truncation; standard sizes are whole mils, e.g. A4
= 11693 x 8268, which is why an A4 SVG is `297.0022mm` wide), then each component
`*= 10.0 * aIusPerDecimil` (int *= double, truncating);
`iusPerMM = m_IUsPerDecimil / 2.54 * 1000` (= 1000000.0), `m_iuPerDeviceUnit = 1 / iusPerMM`.
`m_yaxisReversed = true`, so `y_dev = (y - off.y) * scale / 1e6` (no flip). Mirroring:
`SVG_PLOTTER::userToDeviceCoordinates` additionally subtracts the paper width (so mirrored content
has negative X) and `StartPlot` mirrors the viewBox to match (`S:L829-835`).

**Implicit float -> int vector conversions.** `VECTOR2I( VECTOR2D )` *truncates* (with clamping,
`vector2d.h:L84-99`); it does not round. This happens silently in several places and must be
reproduced: `polyArc` `MoveTo( aCenter )` for filled arcs (`P:L200,221`), `GERBER_PLOTTER::Arc` ->
`plotArc( aCenter, ... )` (`G:L901`: the `VECTOR2D` centre becomes a truncated `VECTOR2I`),
`SVG_PLOTTER::Arc` -> `Circle( aCenter, ...)` and `userToDeviceCoordinates( aCenter )`
(`S:L492,503`), the stroke-font callback in `CALLBACK_GAL::DrawGlyph`
(`common/callback_gal.cpp:L36-44`: glyph points are `VECTOR2D`, the stroke callback takes
`VECTOR2I`), `B:L1106-1114` Bezier points. Everything else uses explicit `KiROUND`.

### 4.2 Gerber engine

#### 4.2.1 File assembly (`StartPlot` `G:L240-317`, `EndPlot` `G:L320-397`)

`StartPlot` resets the macro flags and the free-polygon list (but **not** `m_apertures`,
`m_currentApertureIdx`, `m_objectAttributesDictionary`: one plotter instance = one file), then
writes to the body buffer:

1. every non-empty `m_headerExtraLines` entry (the `TF.*` lines, 4.5) followed by `\n`;
2. `%FSLAX{i}{f}Y{i}{f}*%` with `i = 4` (mm) or 3 (inch), `f = m_gerberUnitFmt`;
3. `G04 Gerber Fmt {i}.{f}, Leading zero omitted, Abs format (unit mm)*`;
4. `G04 Created by KiCad ({creator} {version}) date {YYYY-MM-DD HH:MM:SS}*` (creator is `PCBNEW`);
5. `%MOMM*%` (or `%MOIN*%`), `%LPD*%`, `G01*`, `G04 APERTURE LIST*`.

It also raises the render settings' default pen width to at least 0.1 mm
(`pen_min = 0.1 * m_IUsPerDecimil * 10000 / 25.4` = 100000 IU, `G:L309-314`); this is the pen
used for `USE_DEFAULT_LINE_WIDTH`.

`EndPlot` appends `M02*` and copies the body to the output; directly after the line that equals
`G04 APERTURE LIST*` it inserts:

* if any macro is used: `G04 Aperture macros list*`, then the fixed macro texts in the order
  `RoundRect`, `HorizOval`, `RotRect`, `Outline4P`, (`Outline5P`, `Outline6P`, `Outline7P`,
  `Outline8P` together when `m_hasApertureChamferedRect`), then the free-polygon macros in list
  order, then `G04 Aperture macros list end*`;
* the aperture list (`writeApertureList`, 6.2);
* `G04 APERTURE END LIST*`.

Port: keep header and body in one `String`; remember the byte offset after the marker line instead
of re-scanning. (The C++ re-reads with `fgets( line, 1024 )`; lines longer than 1023 bytes are
copied in pieces, which is harmless.)

#### 4.2.2 Aperture selection and D-codes (`G:L400-603`)

Every drawing call first selects an aperture. `selectAperture( size, radius, rotation, type,
attribute, customAttribute )` compares the request with the **current** aperture (type, size,
radius, rotation, then attribute and custom attribute); only if something differs it calls
`GetOrCreateAperture` and prints `D{dcode}*`. `GetOrCreateAperture` is a linear search over
`m_apertures` for the first exact match of all six properties; otherwise it appends a new
aperture with `m_DCode = (D-code of the last aperture in the list, or 9) + 1`.

Consequences that shape the output:

* D-codes are numbered in order of first use; the aperture list is in creation order. The
  plot order of 4.7 therefore determines every D-code.
* A `D{n}*` line is printed even when the newly selected aperture is never used to draw (e.g.
  `BRDITEMS_PLOTTER::PlotText` selects a plain pen, then `PLOTTER::PlotText` selects the same width
  with the `NonConductor` attribute on copper: both lines appear). Do not "optimise" this away.
* `SetCurrentLineWidth( w, data )` (`G:L400-421`): `DO_NOT_SET_LINE_WIDTH` returns immediately,
  `USE_DEFAULT_LINE_WIDTH` takes the render settings' default pen; the aperture is
  `( (w,w), 0, ANGLE_0, AT_PLOTTING, attr-from-metadata-or-0, custom )`. Calls *without* metadata
  (e.g. from the base class `polyArc`, `P:L193`, or `GERBER_PLOTTER::Arc`, `G:L886`, when a real
  width is passed) select the attribute-less aperture and can thereby switch away from an
  attributed one of the same size.
* Corner-list apertures (`Outline4P`..`8P`, `FreePoly`): `selectAperture` (`G:L544-578`) compares
  the corners with the current aperture *exactly*; `GetOrCreateAperture` (`G:L462-515`) compares
  with `polyCompare` (`G:L74-91`): same count and every coordinate within **+-2 IU**. The first
  aperture in list order within tolerance wins and keeps *its* corner values. For
  `AM_FREE_POLYGON` the macro list is searched the same way (`FindAm`) and a macro is appended on
  a miss; `writeApertureList` later looks the macro index up again with `FindAm( tool.m_Corners )`.
* `selectApertureWithAttributes` (`G:L581-603`), used by the simple flashes: select aperture, then
  `formatNetAttribute`, then `X..Y..D03*`.

#### 4.2.3 Object (net) attributes: a small state machine

`formatNetAttribute( &metadata.m_NetlistMetadata )` (`G:L206-237`) is called before every object
that carries metadata. It does nothing if `m_useNetAttributes` is false or the type is
`UNSPECIFIED`. Otherwise `FormatNetAttribute` (`M:L551-752`) builds the full attribute text, in
this order: `TO.P,<cmpref>,<padname>[,<pinfunction>]` (if `PAD`), `TO.N,<netname>` (if `NET`),
`TO.C,<cmpref>` (if `CMP` and not `PAD`); each wrapped as `%...*%\n` (X2) or `G04 #@! ...*\n` (X1).
Special values: empty pad name -> empty field; empty net name -> `N/C` unless `m_NotInNet`
(then empty: `%TO.N,*%`). Strings go through `FormatStringToGerber` (6.2).

If the full text equals `m_objectAttributesDictionary` nothing is printed. Otherwise, per
attribute kind (P, N, C): if the dictionary has that kind and the new set does not ->
`clearDict = true`; if it has it with a different value, or does not have it -> the attribute is
added to the "short" list. The dictionary is set to the new full text. When `clearDict`:
`clearNetAttribute()` prints `%TD*%` (`G04 #@! TD*` in X1) **and empties the dictionary**, then
the *full* text is printed. Because the dictionary is now empty although the file's dictionary is
not, the next object with the same attributes prints them all again. This double emission is
visible in real files; reproduce it by porting the three functions literally.

`StartBlock( nullptr )` and `EndBlock( nullptr )` both call `clearNetAttribute()` (prints the
`TD` only when the dictionary is non-empty). `ClearAllAttributes()` is unconditional (not used
in scope).

Aperture attributes (`TA.AperFunction`) are written in two different ways: for apertures, in the
aperture list in front of the `%ADD` line and followed by `%TD*%` (6.2); for regions (which have
no aperture), immediately before the `G36` and removed after the `G37` with
`%TD.AperFunction*%` (`PlotGerberRegion`, `G:L978-1038`).

#### 4.2.4 Lines, arcs, polygons

* `PenTo` (`G:L828-847`): `'U'` -> `X..Y..D02*`, `'D'` -> `X..Y..D01*`, `'Z'` nothing. No
  suppression of repeated coordinates: every `ThickSegment` is a `D02` + `D01` pair.
* `ThickSegment` (`G:L1190-1208`): `formatNetAttribute`, `SetCurrentLineWidth( width, data )`;
  zero-length -> `MoveTo` + `FinishTo` at the same point (`D02`, `D01`); else base class
  (`MoveTo`, `FinishTo`). `ThickArc`, `ThickRect`, `ThickCircle`, `ThickPoly` follow the same
  "attribute, pen, base class with `DO_NOT_SET_LINE_WIDTH`" pattern (`G:L1211-1263`).
* `FilledCircle( pos, d )` (`G:L1242-1253`): a circle of diameter `d/2` drawn with a pen of width
  `d/2` (integer divisions), not a flash and not a region.
* `Circle` (`G:L875-879`) = two `Arc` calls of 180 deg (`ANGLE_0..180`, `180..360`), radius
  `diameter / 2` (int).
* `Arc( centre, start, angle, radius, fill, width )` (`G:L883-903`): `SetCurrentLineWidth( width )`;
  `arcLength = |radius * angle_rad|`; if `arcLength < 100` (IU) or `|angle_deg| < 0.1` -> base
  `polyArc` with the current pen width (5 degree steps, `P:L181-227`); else
  `plotArc( centre, start, start + angle, radius, false )`.
* `plotArc` by angles (`G:L944-975`): `start = KiROUND( c + r * (cos a0, sin a0) )`, `MoveTo` (or
  `LineTo` inside a region); `end` likewise; prints `G75*`, then `G03*` if `a0 > a1` else `G02*`,
  then `X{end}Y{end}I{dx}J{dy}D01*` with `(dx,dy) = KiROUND( dev(centre) - dev(start) )`, then
  `G01*`. `plotArc( SHAPE_ARC )` (`G:L906-941`) is the variant for arcs inside a
  `SHAPE_LINE_CHAIN`; its direction test is `SHAPE_ARC( dev(start), dev(mid), dev(end), 0 )
  .IsClockwise()` -> `G02*`.
* `PlotPoly( vector<VECTOR2I> )` (`G:L1140-1187`): fewer than 2 points -> nothing.
  `formatNetAttribute`. If filled: `G36*`, `MoveTo( p0 )`, `G01*`, `LineTo` the rest, close with
  `FinishTo( p0 )` if needed, `G37*`. Then, **if `width != 0` or not filled**, the outline with
  `SetCurrentLineWidth( width, metadata )`.
* `PlotPoly( SHAPE_LINE_CHAIN )` (`G:L1054-1137`): same but with arcs (`ArcIndex( i ) >= 0` ->
  `plotArc( arc, true )` and skip the arc's remaining points), and the outline is drawn **only
  when not filled** (`else if( aWidth != 0 )`). A non-filled chain with width 0 draws nothing.
  The outline is closed when the chain `IsClosed()`.
* `PlotPolyAsRegion( chain, fill, width, md )` (`G:L1041-1051`): outline first (if `width != 0` or
  not filled), then `PlotGerberRegion` (if filled).
* `Rect` (`G:L850-872`): 5-point polygon `p1, (p1.x,p2.y), p2, (p2.x,p1.y), p1` -> `PlotPoly`.
* `SetLayerPolarity( positive )`: `%LPD*%` / `%LPC*%`.

#### 4.2.5 Pad flashes and aperture macros (`G:L1266-1805`)

| Call | Aperture macros enabled (default) | `--disable-aperture-macros` |
|---|---|---|
| `FlashPadCircle( pos, d )` | `AT_CIRCLE`, size `(d,d)` | same |
| `FlashPadOval( pos, size, orient )` | `orient.Normalize()`; cardinal: `AT_OVAL` (X/Y swapped for 90/270). Otherwise macro `HorizOval`: make `size.x >= size.y` by swapping and `orient += 90`, then `if orient > 180: orient -= 180`; aperture `(size, 0, orient, AM_ROTATED_OVAL)` | non-cardinal: `FlashPadRoundRect( pos, size, min(size)/2, orient )` -> region |
| `FlashPadRect( pos, size, orient )` | `orient.IsCardinal()` (orient **not** normalised): `AT_RECT` (swapped for 90/270). Otherwise `RotRect` aperture `(size, 0, orient, AM_ROT_RECT)` | non-cardinal: `FlashPadTrapez` with the 4 rectangle corners -> region |
| `FlashPadRoundRect( pos, size, r, orient )` | always macro `RoundRect`: aperture `(size, r, orient, AM_ROUND_RECT)`, even for orientation 0 | region built from 4 arcs by `plotRoundRectAsRegion` (`G:L1426-1552`), with `TA` printed before and `TD.AperFunction` after |
| `FlashPadTrapez( pos, corners[4], orient )` | `Outline4P` aperture with the *unrotated* corners and `orient` | corners rotated + translated -> `PlotGerberRegion` |
| `FlashPadChamferRoundRect( pos, size, r, ratio, positions, orient )` | if `r != 0 && positions != 15` (some corner is rounded): polygon from `TransformRoundChamferedRectToPolygon( ..., GetPlotterArcHighDef(), ERROR_INSIDE )`, moved to origin / rotation 0 -> `FreePoly` macro. Otherwise polygon built at origin with radius 0: 4..8 corners -> `Outline4P`..`Outline8P` | same polygon -> `PlotGerberRegion` |
| `FlashPadCustom( pos, size, orient, polys )` | per outline: corners + closing point; more than 4990 corners -> region; else moved by `-pos`, rotated by `-orient` -> `FreePoly` macro aperture with rotation `orient`, `formatNetAttribute`, `D03` | `PlotGerberRegion` per outline |
| `FlashRegularPolygon` | `AT_REGULAR_POLY3 + n - 3`, radius `d/2` | same (not reached from board plotting) |

The corner order of every polygon handed to a macro is the order produced by the geometry
library (`TransformRoundChamferedRectToPolygon`, `PAD::GetEffectivePolygon`); it is written
verbatim into the macro. The start vertex and winding must therefore match the C++.

### 4.3 SVG engine (`S`)

* `StartPlot` (`S:L792-870`): fixed XML header, `width`/`height`/`viewBox` in mm with
  `m_precision` decimals (page size, or the plot bounding box set with `SetPlotBBox`), `<title>`
  with the output file's base name and the date, `<desc>`, then an opening `<g style=...
  transform="translate(0 0) scale(1 1)">`.
* Style handling is lazy: setters only set `m_graphics_changed`; the next primitive calls
  `setSVGPlotStyle` (`S:L240-335`), which prints `</g>\n<g style="...">\n` - it closes the
  previous style group and opens a new one. `EndPlot` prints the final `</g> \n</svg>\n`.
  `SVG_PLOTTER::Text` opens extra `<g>` elements of its own (`S:L953,992`), so the nesting in the
  file is not what a pretty-printer would produce; port the print statements literally.
* `SetColor` is `PSLIKE_PLOTTER::SetColor` (`PS:L62-87`): colour mode -> RGB(A) (inverted when
  `m_negativeMode`); black-and-white mode -> black unless the colour is exactly `WHITE`.
  `emitSetRGBColor` (`S:L369-385`) truncates `255.0 * c`.
* Primitives: `Rect` (`S:L398-449`; `<rect>` with `{:f}`, a `<line>` when degenerate, a polygon
  when it has a corner radius), `Circle` (`S:L452-478`), `Arc` (`S:L481-595`, SVG `A` path
  command, start/end swapped so that the sweep flag is always 0), `BezierCurve` (`S:L598-628`,
  native `C` command), `PlotPoly` (`S:L631-679`, one `<path>`; `fill-rule:evenodd` for filled),
  `PenTo` (`S:L744-789`, builds `<path d="M.. L.. " />` incrementally; consecutive `D` moves to
  the same point are dropped).
* Pads: inherited `PSLIKE_PLOTTER::FlashPad*` (`PS:L90-225`): circle -> filled `Circle`; oval ->
  `ThickSegment` between the two centres with width = small size; rect / trapezoid / custom ->
  filled `PlotPoly`; round rect -> `TransformRoundChamferedRectToPolygon(...,
  GetPlotterArcHighDef(), ERROR_INSIDE )` -> filled `PlotPoly`.
* Number formatting: most coordinates `{:.{}f}` with `m_precision`; a few with plain `{:f}`
  (`<rect>` attributes, `rotate(...)`, dash arrays for dotted styles). Keep exactly which is which.

### 4.4 Text

There is one generic path for both engines: `PLOTTER::PlotText` (`P:L618-667`) /
`PLOTTER::Text` (`P:L548-615`):

1. (`@{...}` expression evaluation: out of scope, 11.)
2. Pen. The two entry points differ:
   * `PlotText` (attribute version; the one the board plotter uses): `SetColor`, then
     `SetCurrentLineWidth( attributes.m_StrokeWidth, data )` **before** any adjustment; only
     afterwards `width == 0 && bold` -> `GetPenSizeForBold( min( size.x, size.y ) )`
     (= `KiROUND( size / 5.0 )`) and negative -> absolute, stored back into the attributes
     (this value reaches the font, not the aperture).
   * `Text` (parameter version; used by the drawing sheet and, through `SVG_PLOTTER::PlotText`,
     by every SVG text): `SetColor`, the same bold / absolute adjustment first, and the stroke
     callback itself calls `SetCurrentLineWidth( penWidth )` - without metadata - before every
     segment. A negative `size.x` means mirrored text.
3. Create a `CALLBACK_GAL` with a stroke callback (`MoveTo( a )`, `LineTo( b )`, `PenFinish()`
   per glyph segment) and an outline callback (`PlotPoly( chain, FILLED_SHAPE, 0, data )`), take
   the default font if none was given, and call `font->Draw( &gal, text, pos, attributes,
   fontMetrics )`.

For the stroke font this yields, per glyph, per polyline, per segment, one `D02` + `D01` pair in
Gerber (segments of a polyline are not chained). The glyph geometry, kerning, overbar, sub- and
superscript, italic shear, mirroring, justification and rotation all come from the font module
(`kicad-font`); nothing in the plotter adjusts them.

`GERBER_PLOTTER::Text/PlotText` (`G:L1807-1846`) only add `formatNetAttribute` in front.
`SVG_PLOTTER::Text` (`S:L883-1000`) first writes a hidden `<text ... opacity="0">` element for
search (needs `GRTextWidth`), then `<g class="stroked-text"><desc>...</desc>`, the stroked
glyphs via `PLOTTER::Text`, and `</g>`; `SVG_PLOTTER::PlotText` forwards to it with the mirror
flag encoded as a negative X size.

What the board plotter adds on top (multi-line splitting, knockout, strike-out) is in 4.10.

### 4.5 `StartPlotBoard` (`L:L1420-1595`) and `initializePlotter` (`L:L1284-1370`)

1. Create the engine for `aPlotOpts->GetFormat()`. For Gerber an out-of-range layer only logs an
   error.
2. New `PCB_RENDER_SETTINGS`: `LoadColors( opts.ColorSettings() )`, default pen width
   `mmToIU( 0.0212 )` (21200 IU), layer name, dash and gap ratios from the options.
3. If the drawing sheet is to be plotted, work on a copy of the options with mirror off, scale
   1.0 and autoscale off.
4. `initializePlotter`:
   * page: A4 if `GetA4Output()` (then `paperscale = A4.x / page.x`, `autocenter = true`), else
     the board's page (`paperscale = 1`, `autocenter = scale != 1.0 || autoscale`);
   * `bbox = board.ComputeBoundingBox( false, false )`;
   * `compound_scale`: autoscale and non-empty board -> `min( 0.8 * paper / board ) * paperscale`,
     else `scale * paperscale`;
   * offset: autocenter -> `KiROUND( boardCentre - ( paperSize / 2.0 ) / compound_scale )` per
     axis; else `GetUseAuxOrigin() ? designSettings.GetAuxOrigin() : (0,0)`;
   * `SetPageSettings`, `SetViewport( offset, IU_PER_MILS / 10, compound_scale, mirror )`,
     SVG + fit-to-board: `SetPlotBBox( bbox )`, `SetGerberCoordinatesFormat( precision )`,
     `SetSvgCoordinatesFormat( precision )`, `SetCreator( "PCBNEW" )`,
     `SetColorMode( !blackAndWhite )`, `SetTextMode`.

   For Gerber the scale is ignored by the engine, but the *offset* is not: stored options with
   `psa4output yes` shift a Gerber plotted with `--board-plot-params` (11).
5. `OpenFile`; `ClearHeaderLinesList`; Gerber: `DisableApertMacros`, `UseX2format`,
   `UseX2NetAttributes`, then `AddGerberX2Attribute( plotter, board, layer, !useX2 )` (6.3).
6. `StartPlot( pageName )`.
7. If `GetPlotFrameRef()`: `PlotDrawingSheet(...)` (tier 3), then re-run `initializePlotter` with
   the real options if mirror/scale/autoscale were changed in step 3.
8. If `GetNegative()`: `FillNegativeKnockout` (`L:L1376-1386`): `SetNegative( true )`,
   `SetColor( WHITE )`, a filled `Rect` of the board bounding box inflated by 5 mm, `SetColor(
   BLACK )`. (For SVG the driver inverts colours; for Gerber this would plot a solid region.)

`PCB_PLOTTER::Plot` (`PL:L68-354`), the multi-file driver used by SVG, additionally: for SVG
fit-to-board it temporarily replaces the board's page size by the board outline bounding box
(`GetBoardPolygonOutlines`, falling back to `ComputeBoundingBox`) in mils (integer division of IU
by `IU_PER_MILS`), forces `UseAuxOrigin` with aux origin (0,0), and restores both afterwards
(`PL:L92-115`, `L346-351`); skips copper layers that are not enabled
(`copperLayerShouldBeSkipped`); sets the plotter title from the title block. In the port do not
mutate the board: pass page and origin overrides down.

### 4.6 `PlotBoardLayers` (`L:L128-171`) and `PlotOneBoardLayer` (`L:L247-351`)

`PlotBoardLayers( board, plotter, layerSequence, options )`:

1. copy the options; **for Gerber force `DrillMarksType = NO_DRILL_SHAPE`** (issue 25532);
2. `for layer in sequence: PlotOneBoardLayer( board, plotter, layer, options, layer == sequence[0] )`;
3. drill symbols if the board has drill-map items on a plotted layer (`BOARD::DrillSymbolLayers()`,
   tier 3);
4. if drill marks are enabled and there are no map layers: one `PlotDrillMarks()` pass over all
   plotted layers (4.12).

`PlotOneBoardLayer` sets `SetColor( BLACK )`, `SetTextMode`, and dispatches on the layer:

| Layer | Options changed | Function |
|---|---|---|
| any copper layer | `SkipPlotNPTH_Pads = true` | `PlotStandardLayer` |
| `F_Mask`, `B_Mask` | - | `PlotSolderMaskLayer`: `PlotStandardLayer` when `m_SolderMaskMinWidth == 0`, else the polygon path (4.11) |
| `F/B_Adhes`, `F/B_Paste` | `DrillMarksType = NO_DRILL_SHAPE` | `PlotStandardLayer` |
| `F_SilkS`, `B_SilkS` with `SubtractMaskFromSilk` | | Gerber **and** primary layer: `PlotStandardLayer( silk )`, `SetLayerPolarity( false )`, drill marks off, `PlotSolderMaskLayer( matching mask )`, `SetLayerPolarity( true )`. Otherwise `PlotClippedSilkLayer` (4.11) |
| everything else (silk without subtraction, `Edge_Cuts`, `Margin`, courtyards, fab, user layers) | - | `PlotStandardLayer` |

`layer_mask` always contains exactly one layer in these flows, so all `aLayerMask.count() == 1`
branches are taken and the multi-layer colour mixing is dead code.

### 4.7 `PlotStandardLayer` (`L:L357-931`): the plot order

This order defines the object order in the file and, through 4.2.2, all D-codes:

1. `for item in board.Drawings()`: `PlotBoardGraphicItem( item )` (4.10);
2. `for fp in board.Footprints()`: `PlotFootprintTextItems( fp )` (4.10);
3. `for fp in board.Footprints()`: `PlotFootprintGraphicItems( fp )` (4.10);
4. `for fp in board.Footprints()`: `StartBlock`; `for pad in fp.Pads()`: pad (4.8); DNP cross;
   `EndBlock`;
5. `StartBlock`; vias, in `board.Tracks()` order; `EndBlock`;
6. `StartBlock`; track segments and arcs, in `board.Tracks()` order; `EndBlock`;
7. `StartBlock`; `for zone in board.Zones()`: `for layer in zone.GetLayerSet().Seq()`: fill of
   that layer if it is in the mask; `EndBlock`.

All containers are iterated in their in-memory order, which is the order of the board file as
loaded (see the model design, "Iteration orders the C++ code relies on"). Nothing is sorted here.

### 4.8 Pads

Loop body (`L:L392-685`), with `sketchPads = (F_Fab or B_Fab plotted) && GetSketchPadsOnFabLayers()`:

1. Pad not on the plotted layer: skip, unless `sketchPads` and the pad is on the matching outer
   copper layer (F.Fab with F_Cu, B.Fab with B_Cu) -> `doSketchPads = true`.
2. On a copper layer: skip if `!pad->IsOnCopperLayer()`; skip if `!pad->FlashLayer( layerMask )`.
   The latter implements "remove unused pad layers" and NPTH pads without copper, and needs
   connectivity (5.4).
3. Colour (irrelevant for Gerber). If sketching on fab and `GetPlotPadNumbers()`:
   `PlotPadNumber( pad, color )` **before** the pad outline (`B:L84-152`: text sized to the pad
   bounding box, `tsize = min( KiROUND( w / max( chars, 3 ) ), h, 5 mm )`, pen `KiROUND( tsize /
   12.0 )`, rotated 90 deg when the box is taller than wide, custom pads use the number-box
   primitive).
4. `for layer in layerMask.SeqStackupForPlotting()`: `plotPadLayer( layer )` (`L:L446-681`).

`plotPadLayer` computes the plotted size and shape. In C++ it **mutates the pad** (size, shape,
radius ratio), plots, and restores; for trapezoid/chamfered/custom it plots a modified copy. The
port must work on a clone of the pad in all cases (the board is borrowed immutably).

* `margin` (`VECTOR2I`): mask layer -> `x = y = pad->GetSolderMaskExpansion( layer )`; paste layer
  -> `pad->GetSolderPasteMargin( layer )` (X and Y can differ); else 0. `mask_clearance =
  margin.x`. `padPlotsSize = size + 2 * margin` (the PostScript width adjust is 0 here).
* Not custom and `padPlotsSize.x <= 0 || padPlotsSize.y <= 0`: nothing is plotted.

| Shape | Treatment |
|---|---|
| `CIRCLE`, `OVAL` | size = `padPlotsSize`. Not plotted when `GetSkipPlotNPTH_Pads()` (copper layers) and drill marks are off and the new size equals the drill size and the pad is NPTH |
| `RECTANGLE` | size = `padPlotsSize`; if `mask_clearance > 0` the shape becomes `ROUNDRECT` with corner radius `mask_clearance` |
| `TRAPEZOID` | `mask_clearance == 0`: as is. Otherwise a custom-shape copy: polygon `(-dx-ddy, dy+ddx), (dx+ddy, dy-ddx), (dx-ddy, -dy+ddx), (-dx+ddy, -dy-ddx)` with `dx = size.x/2`, `ddx = delta.x/2` etc., `InflateWithLinkedHoles( mask_clearance, ROUND_ALL_CORNERS, maxError )`, anchor = circle of size 0 |
| `ROUNDRECT` | equal X/Y margin: size = `padPlotsSize`, radius = `max( 0, oldRadius + mask_clearance )` (issue 24327). Unequal: size = `padPlotsSize`, radius *ratio* kept |
| `CHAMFERED_RECT` | `mask_clearance <= 0`: size = `padPlotsSize`. Otherwise custom-shape copy: `TransformShapeToPolygon( outline, layer, 0, maxError, ERROR_INSIDE )` of the pad placed at (0,0), offset 0, orientation 0 (with size `padPlotsSize` when the margins differ), inflated with `InflateWithLinkedHoles` when the margins are equal; anchor circle of size 0; position, offset and orientation restored |
| `CUSTOM` | **always** a copy: `MergePrimitivesAsPolygon( layer, &shape )`, `InflateWithLinkedHoles( mask_clearance, ROUND_ALL_CORNERS, maxError )` (also with clearance 0, i.e. an Unfracture / Inflate( 0 ) / Fracture round trip through the polygon clipper), primitives replaced by the result; anchor size clamped to `max( 0, padPlotsSize )` when the clearance is negative |

The corner radius is stored as a *ratio* (`PADSTACK::SetRoundRectRadius`: `ratio = r / min( size )`,
`RoundRectRadius = KiROUND( min( size ) * ratio )`), so "set radius r, read radius back" can
differ from `r` by rounding. Go through the same accessors, in the same order (size first, then
radius).

`BRDITEMS_PLOTTER::PlotPad( pad, layer, color, sketch )` (`B:L155-398`):

* Position: `pad->ShapePos( layer )` (position + rotated offset).
* Metadata, when a copper layer is plotted: component reference; net attribute type
  `PAD | NET | CMP` on outer copper, `NET | CMP` on inner copper; pad name
  (`pad->GetNumber()`), pin function (only when the number is non-empty), net name;
  `m_NotInNet = NPTH || number empty`. Aperture attribute: default `Conductor`; then by
  attribute: `NPTH` -> `WasherPad`, `PTH` -> `ComponentPad`, `CONN` -> `ConnectorPad` (outer
  only), `SMD` -> `SMDPad,CuDef` (outer only); then by property (overrides): `BGA` ->
  `BGAPad,CuDef` (outer only), `FIDUCIAL_GLBL` -> `FiducialPad,Global`, `FIDUCIAL_LOCAL` ->
  `FiducialPad,Local`, `TESTPOINT` -> `TestPad` (outer only), `HEATSINK` -> `HeatsinkPad`,
  `CASTELLATED` -> `CastellatedPad`; finally NPTH is forced back to `WasherPad`.
  When no copper layer is plotted (mask, paste, fab ...): only `TO.C` (`CMP`), no aperture
  attribute.
* Sketch mode: `ThickCircle` / `ThickOval` / `ThickPoly( TransformShapeToPolygon( ...,
  GetPlotterArcHighDef(), ERROR_INSIDE ) )` with `GetSketchPadLineWidth()` and **no** metadata.
* Normal mode: `CIRCLE` -> `FlashPadCircle( pos, size.x )`; `OVAL` -> `FlashPadOval`; `RECTANGLE`
  -> `FlashPadRect`; `ROUNDRECT` -> `FlashPadRoundRect( pos, size, GetRoundRectCornerRadius )`;
  `TRAPEZOID` -> corners `(-hx-dy, hy+dx), (hx+dy, hy-dx), (hx-dy, -hy+dx), (-hx+dy, -hy-dx)` with
  `h = size / 2`, `d = delta / 2` (integer division of the vectors) -> `FlashPadTrapez`;
  `CHAMFERED_RECT` -> Gerber: `FlashPadChamferRoundRect`, others: as custom; `CUSTOM` ->
  `pad->GetEffectivePolygon( layer, ERROR_INSIDE )` -> `FlashPadCustom` if it has outlines.
  All with the pad orientation `pad->GetOrientation()`.

DNP cross (`L:L687-739`): for a DNP footprint on the matching side when F.Fab/B.Fab is plotted
and "cross out" is on and "hide" is off: bounding box of the courtyard (or of the footprint's
effective shape if there is no courtyard) in the footprint's own frame (rotate by `-orientation`
about the position, take `BBox()`, rotate the 4 corners back), two `ThickSegment`s corner1-corner3
and corner2-corner4 with width `m_LineThickness[ LAYER_CLASS_FAB ]`, no metadata.

### 4.9 Vias, tracks, zones

Vias (`L:L744-828`). One `GBR_METADATA` reused for all: on copper `ViaPad` + `NET`. Per via: skip
unless `( via->GetLayerSet() & layerMask ).any()` (`GetLayerSet` contains `F_Mask`/`B_Mask` when
the via is not tented on that side); on copper skip unless `via->FlashLayer( layerMask )`;
`diameter = max over layers in mask of via->GetWidth( layer )`, plus `2 *
via->GetSolderMaskExpansion()` on mask layers; skip if `<= 0`; `m_NotInNet = netname empty`; net
name; `FlashPadCircle( via->GetStart(), diameter, metadata )`.

Tracks (`L:L831-890`). Metadata on copper: `Conductor` + `NET`; on other layers reset to none.
Per segment/arc: skip unless `( layerMask & track->GetLayerSet() ).any()` (`GetLayerSet` adds the
mask layer for tracks with `m_hasSolderMask` on outer copper); `width = GetWidth() + 2 * margin`
with `margin = track->GetSolderMaskExpansion()` on mask layers; arcs:
`!arc->IsDegenerated( 10 )` -> `ThickArc( arc->GetCenter(), arc->GetArcAngleStart(),
arc->GetAngle(), arc->GetRadius(), width, md )`, else a `ThickSegment` start-end.

Zones (`L:L893-930`). Rule areas are skipped. `mainArea = zone->GetFilledPolysList( layer )`
(copy); polygons flagged `zone->IsIsland( layer, i )` are moved to `islands`, iterating `i` from
the last index down (so `islands` is in reverse order). `PlotZone( zone, layer, mainArea )`, then,
if any, `PlotZone( copy of the zone with the "no net" net, layer, islands )`.

`PlotZone` (`B:L902-957`): nothing for an empty set. Metadata for zones on copper: net name; no
net name -> `NonConductor`, no net attribute; else `Conductor` + `NET`. `StartBlock`; per outline:
Gerber `PlotGerberRegion( outline, &md )` (so each fill polygon is `TA` line, `TO.N` line when
changed, `G36`..`G37`, `TD.AperFunction`); other engines `PlotPoly( outline, FILLED_SHAPE, 0 )`;
`EndBlock`. Only outlines are plotted, holes are ignored: this relies on stored zone fills being
fractured (they are).

### 4.10 Graphics, text, dimensions, tables, barcodes, targets

`PlotBoardGraphicItem` (`B:L489-566`) dispatches on the item type; the callee checks the layer:

| Type | Action |
|---|---|
| `PCB_SHAPE_T` | `PlotShape` |
| `PCB_TEXT_T` | `PlotText( text, layer, IsKnockout(), fontMetrics )` |
| `PCB_TEXTBOX_T` | `SetTextMode( STROKE )`; `PlotText`; if `IsBorderEnabled()`: `PlotShape( GetPolygonalBoxShape() )`; restore text mode |
| `PCB_BARCODE_T` | `PlotBarCode`: a filled, zero-width polygon `PCB_SHAPE` from `TransformShapeToPolySet( ..., 0, 0, ERROR_INSIDE )` -> `PlotShape` |
| `PCB_TABLE_T`, `PCB_DRILL_CHART_T`, `PCB_STACKUP_TABLE_T` | `PlotText` for every cell (`GetCells()` order), `PlotTableBorders` (`DrawBorders` callback -> `ThickSegment` or dashed strokes, metadata `TO.C` for footprint tables), chart symbols (tier 3) |
| `PCB_DIM_*_T` | `PlotDimension` (`B:L569-621`): layer check, `PlotText( dim, layer, false, ...)`, then every `SH_SEGMENT` / `SH_CIRCLE` of `GetShapes()` as a temporary parentless `PCB_SHAPE` (stroke = `GetLineThickness()`, solid) through `PlotShape` |
| `PCB_TARGET_T` | `PlotPcbTarget` (`B:L624-675`): circle of radius `size/3` (`size/2` for the X shape) plus two segments, via `PlotShape` |
| anything else (reference image, group, generator, point, marker ...) | not plotted |

Footprint text, `PlotFootprintTextItems` (`B:L401-486`). Nothing if `!GetPlotFPText()`. With
`dnp = footprint->GetDNPForVariant( board->GetCurrentVariant() )`, `hide( l ) = GetHideDNPFPs...
&& l is F_Fab/B_Fab`, `cross( l ) = GetCrossoutDNPFPs... && l is F_Fab/B_Fab`:

1. Reference: plotted if `GetPlotReference()`, its layer is in the mask, it `IsVisible()`, and not
   `( dnp && hide( layer ) )`; struck out if `dnp && cross( layer )`.
2. Value: same conditions with `GetPlotValue()`; never struck out.
3. Then, in this order: every other **visible** field (`GetFields()` order), then every
   `GraphicalItems()` entry that *is-a* `PCB_TEXT`. For each: skip if the layer is `Edge_Cuts`,
   out of range, hidden-DNP, not in the mask, or a private layer of the footprint; text exactly
   `${REFERENCE}` obeys `GetPlotReference()` (and is struck out for DNP), text exactly `${VALUE}`
   obeys `GetPlotValue()`.

   Two quirks to keep: graphical text items are **not** tested for visibility here; and
   `PCB_DIMENSION_BASE` derives from `PCB_TEXT`, so a dimension inside a footprint matches the
   `dynamic_cast<PCB_TEXT*>` and its text is plotted here *and again* by `PlotDimension` in the
   graphics pass.

Footprint graphics, `PlotFootprintGraphicItems` (`B:L678-766`): per `GraphicalItems()` entry skip
private layers, hidden-DNP layers and items not on the mask; then shapes, text boxes, barcodes,
tables, dimensions as above; `PCB_TEXT_T` and `PCB_REFERENCE_IMAGE_T` do nothing here.

`PlotText` (`B:L786-899`):

1. `font = text->GetDrawFont( renderSettings )`, `shown = text->GetShownText(...)`. Return if
   `shown` is empty (unless it is a knockout text box) or the layer is not in the mask.
2. Metadata: on copper layers aperture attribute `NonConductor`; **no** net or component
   attribute, also for footprint text.
3. `attrs = text->GetAttributes()` with `m_Size = GetTextSize()`, `m_StrokeWidth =
   GetEffectiveTextPenWidth()`, `m_Angle = GetDrawRotation()`, `m_Multiline = false`;
   `plotter->SetCurrentLineWidth( attrs.m_StrokeWidth )` (no metadata).
4. Knockout: `TransformTextToPolySet( poly, 0, maxError, ERROR_INSIDE )`, `Fracture()`, every
   outline `PlotPoly( outline, FILLED_SHAPE, 0, md )`.
5. Else outline font without embedded fonts: glyphs from the render cache through a
   `CALLBACK_GAL` (tier 2, outline fonts).
6. Else if `IsMultilineAllowed()`: split `shown` at `\n`, `GetLinePositions`, one
   `plotter->PlotText( pos_i, color, line_i, attrs, font, metrics, md )` per line; strike-out only
   when there is exactly one line.
7. Else one `plotter->PlotText( GetTextPos(), ... )`, then the strike-out.

Strike-out: bounding box of `TransformTextToPolySet( ..., 0, ARC_LOW_DEF, ERROR_INSIDE )` rotated
by `-GetDrawRotation()` about `GetDrawPos()`; a `ThickSegment` at mid height from `left -
strokeWidth` to `right + strokeWidth`, rotated back, width = stroke width.

`PlotShape` (`B:L960-1350`):

1. Skip unless `( m_layerMask & shape->GetLayerSet() ).any()`.
2. `thickness = shape->GetWidth()`, `margin = thickness`. When a mask layer is plotted and the
   shape `HasSolderMask()` and lies on outer copper: `margin += 2 * GetSolderMaskExpansion()`,
   `thickness = max( margin, 0 )`, hatched fill becomes solid.
3. Metadata: parent footprint -> `TO.C`. DNP footprint + "sketch DNP" + shape on F_Fab/B_Fab ->
   `thickness = GetSketchPadLineWidth()`, no fill. Aperture attribute: `Edge_Cuts` -> `Profile`;
   else when a copper layer is plotted: in a footprint -> `EtchedComponent`; net code > 0 ->
   `Conductor` + `TO.N`; else `NonConductor`.
4. Solid line style (`<= LINE_STYLE::FIRST_TYPE`):
   * `SEGMENT`: `ShortenSegmentForEndings`, `ThickSegment`.
   * `CIRCLE`: solid fill -> `FilledCircle( centre, 2 * radius + thickness )` (reduced by a
     negative margin, floor 0); else `ThickCircle( centre, 2 * radius, thickness )`.
   * `ARC`: `|GetArcAngle()| == 360.0` -> `ThickCircle`; else `CalcArcAngles`, `ShortenArcForEndings`,
     `ThickArc( VECTOR2D( centre ), startAngle, endAngle - startAngle, radius, thickness )`.
   * `BEZIER`: SVG -> native `BezierCurve`; else `ShortenedBezierPolyline( thickness )` as
     `ThickSegment`s.
   * `POLY` (if `IsPolyShapeValid()`): `SetCurrentLineWidth( thickness, &md )`; if `thickness > 0`:
     stroke every outline of the **unfractured** polygon with `PlotPoly( outline, NO_FILL,
     thickness, md )` (issue 24143; with line-ending shortening where applicable) and set
     `thickness = 0`; if solid fill: copy, `Fracture()`, `Inflate( margin / 2, ROUND_ALL_CORNERS,
     maxError )` when `margin < 0`, then per outline `SetClosed( true )` and Gerber
     `PlotPolyAsRegion( outline, FILLED_SHAPE, 0, &md )` / others `PlotPoly`.
   * `RECTANGLE`: normalised box, shrunk and radius reduced by a negative margin; `SHAPE_RECT`
     with corner radius -> `Outline()` (a chain with real arcs when the radius is non-zero) ->
     Gerber `PlotPolyAsRegion( outline, fill, thickness, &md )` / others `PlotPoly`.
   * `ELLIPSE`, `ELLIPSE_ARC`: `SHAPE_ELLIPSE::ConvertToPolyline( maxError )`; closed ellipses on
     Gerber via `PlotPolyAsRegion`, everything else `PlotPoly`.
5. Dashed styles: `MakeEffectiveShapesForStroking( thickness )`, then `STROKE_PARAMS::Stroke(
   shape, style, GetWidth(), renderSettings, cb )` with `cb = ThickSegment( a, b, thickness, md )`.
6. Line endings (arrow, open arrow, circle, square; `common/line_ending.cpp:L122-190,240-265`):
   polygon from `GetShapes` -> `PlotPoly` (open arrow: polyline with the stroke width; others:
   filled, plus outline when the ending has its own stroke width).
7. Hatched fill: every outline of `GetHatching()` as Gerber `PlotPolyAsRegion( outline,
   FILLED_SHAPE, 0, &md )` / others filled `PlotPoly`.

### 4.11 Solder mask with minimum width, and silk clipped by mask

`PlotSolderMaskLayer` (`L:L92-108`): if `m_SolderMaskMinWidth == 0` (the default) the mask layer
is a standard layer (4.7-4.10 with the margins of 4.8/4.9). Otherwise `GenerateLayerPoly` builds
one polygon set for the layer and `PlotPolySet` plots it.

`GenerateLayerPoly( result, board, plotter, layer, plotFPText, plotRefs, plotValues )`
(`L:L1023-1275`), with `inflate = m_SolderMaskMinWidth / 2 - 1` for mask layers and 0 for silk,
`maxError = m_MaxError`, all conversions with `ERROR_OUTSIDE`, and the arc radius correction
disabled for the duration (`DISABLE_ARC_RADIUS_CORRECTION`, a process-global flag in the geometry
library, see 5.2). Everything is added twice when `inflate != 0`: exact shape to `exactPolys`,
inflated shape to `result`. Order of accumulation:

1. per footprint: `TransformPadsToPolySet`; fields (visible, on layer, reference/value options);
   graphical items on the layer: `PCB_TEXT_T` -> `TransformTextToPolySet`; text boxes (text + box
   outline when bordered and not knockout); `PCB_SHAPE` -> solid style `TransformShapeToPolySet`,
   dashed style `STROKE_PARAMS::Stroke` + `TransformOvalToPolygon( ..., thickness [+ inflate],
   maxError, ERROR_OUTSIDE, 8 )`; anything else `TransformShapeToPolySet`;
2. tracks and vias with `track->IsOnLayer( layer )`: `TransformShapeToPolygon( ..., clearance [+
   inflate], ...)` with `clearance = GetSolderMaskExpansion()`;
3. board drawings on the layer (same rules as footprint graphics; board text is not subject to
   the footprint text options);
4. zones (not rule areas) on the layer: the fill, `Inflate( inflate, CHAMFER_ALL_CORNERS,
   maxError )`.

Then `result.Simplify()`; if `inflate != 0`: `result.Deflate( inflate, CHAMFER_ALL_CORNERS,
maxError )` and `result.BooleanAdd( exactPolys )`.

`PlotPolySet` (`L:L64-83`): `Fracture()` the set and plot it through `PlotZone` with a temporary
net-less zone on that layer (non-copper -> no attributes at all: plain `G36`/`G37` regions).

`PlotClippedSilkLayer` (`L:L111-125`): `GenerateLayerPoly` for the silk layer and for the matching
mask layer, `silk.BooleanSubtract( mask )`, `PlotPolySet`. Used for SVG and for Gerber when the
silk layer is not the file's primary layer. Note the consequence: in this mode silk text and
lines become filled polygons instead of stroked lines.

All of 4.11 depends on the polygon clipper producing the same contours in the same order with
the same start vertices as Clipper2 does in C++.

### 4.12 Drill marks and drill symbols

`PlotDrillMarks` (`B:L1437-1512`) runs only for non-Gerber engines (forced off in
`PlotBoardLayers`; the unit tests of 9.1 check exactly that). `smallDrill = mmToIU( 0.35 )`
(`ADVANCED_CFG::m_SmallDrillMarkSize`) in `SMALL_DRILL_SHAPE` mode, 0 otherwise. Colour `WHITE`
(knock-out). Vias on a plotted layer: `plotOneDrillMark( CIRCLE, via->GetStart(), (drill, 0),
(via width, 0), ANGLE_0, smallDrill )`. Pads with a hole: colour `BLACK` unless some plotted layer
of the pad has a pad size larger than the drill (then `WHITE`); `plotOneDrillMark( drill shape,
pad->GetPosition(), drill size, pad size, orientation, smallDrill )`. `plotOneDrillMark`
(`B:L1409-1434`): small-mark clamp for round holes, `clamp( size, 1, padSize - 1 )`, then
`FlashPadOval` or `FlashPadCircle` without metadata.

`PlotDrillSymbols` / `PlotChartSymbols` (`B:L1515-1656`): only for boards containing
`PCB_DRILL_MAP` / `PCB_DRILL_CHART` items (new in this file format). Tier 3 (FAB-13); until then
such boards are a documented gap: C++ draws the hole symbols on the map's layer and *suppresses
the global drill-mark pass* when a plotted layer carries a map.

### 4.13 The drill model (`pcbnew/drill/`, `DB:L54-162`)

`EnumerateDrillSpans( board )` (`DE:L302-352`): collect into a `std::set<DRILL_SPAN>` (ordering of
3.7): for every via whose `LayerPair( top, bottom )` is not `(F_Cu, B_Cu)` the span `(top, bottom,
false, false)`; for every via's secondary and tertiary drill that `isBackdrillSet` (start and end
layer defined and a size > 0) the span `(start, end, true, false)`. Pad backdrills are not
collected by the drill writers (`aPadBackdrills = false`). Result: the through span `(F_Cu, B_Cu,
false, false)` first, then the set in order.

`EnumerateDrillOperations( board, query )` (`DE:L270-299`): vias in `board.Tracks()` order (none
when `m_NonPlatedOnly`), then - only for the non-backdrill through span - pads in
`Footprints()` x `Pads()` order.

* Via, normal span (`DE:L124-177`): skip `GetDrillValue() == 0`; the via's layer pair must equal
  the span's pair (either orientation). Attribute `HOLE_VIA_THROUGH` for the through span, else
  `HOLE_VIA_BURIED`; diameter = drill; position `GetStart()`; `m_Filled`, `m_Capped` from
  `ResolveFillingMode()` / `ResolveCappingMode()`; covered / plugged / tented flags per side only
  when the via reaches that outer layer; post-machining from the padstack.
* Via, backdrill span (`DE:L99-122`, `backdrillOperation` `DE:L55-81`): for the secondary and the
  tertiary drill whose start/end equal the span's stored start/end: not plated, kind
  secondary/tertiary, `m_Diameter = size.x` (or the smaller of X/Y when Y > 0), attribute
  `HOLE_VIA_BACKDRILL`, `m_StubLength = stackup.GetDrillDepth( start, end )`, filled/capped from
  the drill properties only (surface protection is not inherited).
* Pad (`DE:L181-257`): unless `m_MergePTHNPTH`, the plated query skips NPTH pads and the
  non-plated query skips everything else; skip `GetDrillSize().x == 0`. `m_NotPlated = NPTH`;
  attribute `HOLE_MECHANICAL` (NPTH), else `HOLE_PAD_CASTELLATED` / `HOLE_PAD_PRESSFIT` by pad
  property, else `HOLE_PAD`; `m_Diameter = min( drill.x, drill.y )`; `m_IsSlot = IsDrillSlot( pad )`
  (`HasHole() && drill shape != CIRCLE && drill.x != drill.y`); `m_SizeXY = GetDrillSize()`;
  position `pad->GetPosition()` (the hole is at the pad position, not at `ShapePos`); orientation
  `pad->GetOrientation()`. Post-machining is not copied (`m_PadMachining` is false).

`GENDRILL_WRITER_BASE::buildHolesList( span, npthOnly )` (`DB:L77-162`): enumerate with
`m_MergePTHNPTH = m_merge_PTH_NPTH`, then `std::sort` with `cmpHoleSorting` (`DB:L54-74`): plated
before non-plated; diameter ascending; `HOLE_ATTRIBUTE` ascending (so vias before pads before
mechanical for one diameter); X ascending; Y ascending. Then one pass assigns tools: a new
`DRILL_TOOL` starts whenever diameter, plating or attribute differs from the previous hole;
`m_holeToolReferences[i]` is the 1-based tool index; counts (`m_TotalCount`, `m_OvalCount` for
slots), backdrill flag, min/max stub length and the post-machining flag are accumulated per tool.

### 4.14 Which drill files are written

Excellon, `EXCELLON_WRITER::CreateDrillandMapFilesSet` (`DX:L75-182`). Spans = `EnumerateDrillSpans`
plus, when PTH and NPTH are separate, the NPTH pseudo-span `(F_Cu, B_Cu, false, true)` at the end.
Per span: `doing_npth = merge ? false : span.m_IsNonPlatedFile`; `buildHolesList`; the file is
written if it has holes **or** it is the NPTH file **or** the span's pair is `(F_Cu, B_Cu)` (so
the main file and, in separate mode, the NPTH file always exist, possibly empty). File type for
the header: through span -> `MIXED_FILE` (merged) / `NPTH_FILE` / `PTH_FILE`; backdrill span ->
`NPTH_FILE`; blind/buried -> `PTH_FILE`. A backdrill span with holes is written a second time
(`writeBackdrillLayerPairFile`, `DX:L654-718`) under the layer-pair name with a `; backdrill`
comment before each hit. Reporter text: `Created file '<path>'` (the CLI test greps stdout for
`Created file '(.+)'`).

Gerber, `GERBER_WRITER::CreateDrillandMapFilesSet` (`DG:L57-201`). PTH and NPTH are always
separate. Spans = `EnumerateDrillSpans` + the NPTH pseudo-span. Per span: `buildHolesList`;
**no file when there are no holes**; `createDrillFile( name, nonPlated = npth || backdrill, span )`;
backdrill spans are written a second time under the pair name. For every span with holes except
the NPTH pseudo-span (backdrill spans included): the `FILLED` and `CAPPED` protection files, each
only if some via in the span's list has the property. After the
loop, over the via holes of all plated non-backdrill spans together: `COVERED_BACK`,
`COVERED_FRONT`, `PLUGGED_BACK`, `PLUGGED_FRONT`, and with `--generate-tenting` `TENTED_BACK`,
`TENTED_FRONT`, each only if some via has the property. Reporter text: `Created file '<path>'.`

Then, for both formats, `CreateMapFilesSet` (`DB:L303-355`) if maps are requested, and `Done.`

File names: 6.1. File contents: 6.4-6.6.

### 4.15 Drill map and drill report

`genDrillMapFile( name, format )` (`DB:L506-847`), per span after `buildHolesList`:

1. `bbox = board.GetBoardEdgesBoundingBox()` (computed with `Edge_Cuts` forced visible).
2. Gerber map: `SetViewport( offset, 2540, 1.0, false )`, `SetGerberCoordinatesFormat( 5 )`.
   SVG (and PDF/PS, not ported): page = the writer's page info, or A4 landscape when none was
   set (the job handler sets none); 10 mm margin; `Xscale = ( pageW - 2 * margin ) / bboxW`,
   `Yscale = ( KiROUND( pageH * 0.6 ) - margin ) / bboxH`, `scale = min( Xscale, Yscale, 3.0 )`;
   `offset = KiROUND( bboxCentre - ( pageW / 2.0, ypagesize_for_board / 2.0 ) / scale )`;
   `bottom_limit = ( pageH - margin ) / min( scale, 1.0 )`.
3. Creator `PCBNEW`, colour mode off, render settings with default pen 0.2 mm. Gerber header:
   `AddGerberX2Header( plotter, board, false )`, `%TF.FileFunction,Drillmap*%`,
   `%TF.FilePolarity,Positive*%`; net attributes off.
4. Board outline: every `PCB_SHAPE` on `Edge_Cuts` from `board.Drawings()` and then from every
   footprint's graphics, copied, moved to `Dwgs_User`, plotted with a `BRDITEMS_PLOTTER` with
   default options and layer set `{ Dwgs_User }` (-> `PlotShape`, no `Profile` attribute).
5. `plotDrillMarks` (`DB:L996-1019`): per hole `SetCurrentLineWidth( max( diameter / 10, 0.1 mm ) )`,
   `Marker( position, diameter, toolIndex0 )` (`PLOTTER::Marker`, `P:L364-386`, pattern table in
   `drill_markers.cpp`; more than 58 tools share the circle), slots additionally `ThickOval(
   position, sizeXY, orientation, 0.1 mm )`.
6. Legend below the board: `Drill Map:` and one line per tool (marker + text
   `"%3.3fmm / %2.4f\" "` + count text + ` (not plated)`), 2 mm characters scaled by
   `min( 1, 1 / scale )`, wrapping into a new column when `bottom_limit` is reached. Formulas:
   `DB:L696-841`; port them literally (many `KiROUND`s and int/double mixes).

`GenDrillReportFile` (`DB:L850-993`): 6.6.

### 4.16 Gerber job file (`J`)

`WriteJSONJobFile` (`J:L164-189`) builds an ordered JSON object with the sections `Header`,
`GeneralSpecs`, `DesignRules`, `FilesAttributes`, `MaterialStackup` in that order and dumps it
with 2-space indentation. Data sources:

* `GeneralSpecs` (`J:L209-315`): project name, GUID and revision (as in 6.3), board size from
  `GetBoardEdgesBoundingBox()`, copper layer count, board thickness, and optional flags from the
  stackup descriptor and pad counts.
* `DesignRules` (`J:L437-553`): minima over the default netclass and all netclasses with a
  clearance; over every pad's `GetOwnClearance( layer )` for each layer of its layer set (outer vs
  inner); over track widths (outer vs inner, vias excluded); over copper zones'
  `GetOwnClearance( layer )`. An `Inner` entry only when the board has more than 2 copper layers.
* `FilesAttributes` (`J:L318-434`): one entry per plotted file in plot order; courtyard and margin
  layers are skipped silently, other unknown layers with an error report.
* `MaterialStackup` (`J:L556-772`): one entry per stackup item (per sub-layer for dielectrics),
  top to bottom, from a copy of the stackup descriptor that is first re-synchronised with the
  board (`SynchronizeWithBoard`; a change means "not up to date": colours and thicknesses are then
  omitted and an error is reported when the board has a stackup).

All lengths go through `mapValue` (`J:L192-206`): `"%.4f"` of the mm value, parsed back to a
number (6.7).

### 4.17 Summary: every place where order or tie-breaking reaches the output

| Place | Order in C++ | Port |
|---|---|---|
| Files of a `gerbers` run, and `FilesAttributes` in the job file | `LSET::UIOrder()` of the selected layers | same function from `kicad-core` |
| Layers inside one file | the file's layer, then common layers in the given sequence (duplicates removed) | `Vec`, first occurrence wins |
| `--layers` / `--cl` tokens | token order; a group token expands in `LSET::Seq()` order | same |
| Items of a layer | 4.7: drawings, footprint texts, footprint graphics, pads per footprint, vias, tracks, zones; each in board container order | iterate the model's containers in file order; never sort |
| Pad layers | `layerMask.SeqStackupForPlotting()` (one layer in practice) | same |
| Zone layers / fill polygons | `GetLayerSet().Seq()`; polygon index order; islands reversed | same |
| Aperture numbering | first use (linear list, first match; corner lists with +-2 IU tolerance) | `Vec` + linear search; no hashing |
| Free-polygon macro numbering | first use, linear search with tolerance | same |
| Net attribute emission | depends on the previous object's attributes (4.2.3) | port the state machine literally |
| Polygon contours and vertex order (fracture, simplify, boolean, inflate; pad effective polygons; round-rect/chamfer polygonisation) | Clipper2 / KiCad geometry | `kicad-geom` must reproduce them |
| Drill spans | `std::set<DRILL_SPAN>` | `BTreeSet` with the same `Ord` |
| Holes and tool numbers | `std::sort` (not stable) with `cmpHoleSorting` | stable sort; differs from libstdc++ only for coincident holes that compare equal (same plating, diameter, attribute, position), where the emitted lines are identical unless their post-machining differs - record in `PORT_STATUS.md` |
| Protection files | fixed feature order (4.14) | same |
| Job-file keys | insertion order (`ordered_json`) | emit in the same order by hand |
| Netclass scan in `DesignRules` | `std::map` iteration, but only a minimum is taken | order-independent |
| `convertLayerArg` maps | lookup only | any map |

There are no unordered containers, no hashing and no threads on any of these paths.

---

## 5. What this subsystem needs from other subsystems

### 5.1 `kicad-core`

* Layers: `PcbLayerId` constants incl. `UNDEFINED_LAYER`, `PCBNEW_LAYER_ID_START`,
  `PCB_LAYER_ID_COUNT`; `IsCopperLayer`, `IsExternalCopperLayer`, `IsUserLayer`, `IsFrontLayer`,
  `IsBackLayer`, `IsCopperLayerLowerThan`, `CopperLayerToOrdinal`, `ToLAYER_ID`; `LayerName()`
  (GUI names).
* `LSET`: construction from lists and from `LSEQ`, `&`, `|`, `~`, `any/none/count/test/set`,
  iteration in ascending id order, `Seq()`, `Seq( order )`, `SeqStackupForPlotting()`,
  `UIOrder()`, `CuStack()`, `TechAndUserUIOrder()`, `AllCuMask()`, `AllCuMask( n )`,
  `AllNonCuMask()`, `ExternalCuMask()`, `InternalCuMask()`, `AllLayersMask()`,
  `FrontBoardTechMask()`, `BackBoardTechMask()`, `Contains`, `ContainsAll`, `Name( layer )`,
  `FmtHex()`. `LSEQ`.
* GAL layer ids used as colour keys: `LAYER_VIAS` (+ via type), `LAYER_DNP_MARKER`,
  `LAYER_DRAWINGSHEET`, `LAYER_PCB_BACKGROUND`.
* `KIID` (drill operation ids).
* Units: `pcbIUScale` (`IU_PER_MM = 1e6`, `IU_PER_MILS = 25400`, `mmToIU`, `IUTomm`),
  `ARC_LOW_DEF`, `ARC_HIGH_DEF`.
* Strings: `UnescapeString`, `PrintableCharCount`, UTF-8 <-> code point iteration (Gerber
  escaping works on UTF-16-ish `wxChar` code units: use `char` code points and `& 0xFFFF`).
* Enums shared with the model: `FILL_T`, `LINE_STYLE`, `PLOT_FORMAT`, `DRILL_MARKS`.
* A `Reporter` trait (severity + text) for the "Created file ..." / "Plotted to ..." messages.
* Number formatting helpers with C `printf` semantics: fixed notation with N decimals
  (`{:.Nf}`, `{:#f}` = 6 decimals), zero-padded integers (`%0*d`), left/right padded fields
  (`{:<25}`, `{:<2}`, `{:2.3f}`), `%x` of a (possibly negative) `int` as two's complement. One
  small module with tests; do not scatter `format!` calls.

### 5.2 `kicad-geom`

* `Vector2I`, `Vector2D` (incl. the truncating `Vector2D -> Vector2I` conversion as a *named*
  function), `KiROUND` (scalar and vector), `EuclideanNorm`, `Cross`, `Distance`.
* `Box2I` / `Box2D`: construction from origin + size, `Normalize`, `Inflate( n )`,
  `Inflate( dx, dy )`, `Move`, `GetOrigin`, `GetEnd`, `GetPosition`, `GetSize`, `Centre`,
  `GetCenter`, `GetX`, `GetLeft/Right/Top/Bottom`, `GetWidth/GetHeight`.
* `EdaAngle`: from degrees, from a vector (`atan2`), `ANGLE_0/90/180/HORIZONTAL`, `+ - unary-`,
  comparisons, `==` on the raw value, `Normalize`, `NormalizeNegative`, `Normalize180`,
  `IsCardinal`, `IsCardinal90`, `IsZero`, `Sin`, `Cos`, `AsDegrees`, `AsRadians`.
* `RotatePoint` (integer point; point about a centre; `Vector2D` variant; the `int* x, int* y,
  cx, cy` form used by Excellon), `CalcArcCenter( start, mid, end )` (double variant).
* `Seg`; `ShapeLineChain`: `PointCount`, `CPoint`, `CPoints`, `CLastPoint`, `IsClosed`,
  `SetClosed`, `Append`, `ArcIndex`, `Arc( i )`, `ArcCount`, `SegmentCount`, `CSegment`,
  `GetSegment`, `IsArcSegment`.
* `ShapeArc`: three-point constructor with width, `GetP0`, `GetP1`, `GetArcMid`, `GetCenter`,
  `IsClockwise`.
* `ShapePolySet`: `NewOutline`, `Append`, `AddOutline`, `OutlineCount`, `Outline( i )`,
  `COutline( i )`, `HoleCount`, `CHole`, `CPolygon( i )`, `DeletePolygon`, `RemoveAllContours`,
  `IsEmpty`, `BBox`, `Rotate( angle, centre )`, `CloneDropTriangulation`, `Fracture`,
  `Unfracture`, `Simplify`, `BooleanAdd`, `BooleanSubtract`, `Inflate( amount, CORNER_STRATEGY,
  maxError )`, `Deflate`, `InflateWithLinkedHoles`, `Area` (tests). The clipper-backed ones must
  match Clipper2's contour order and start vertices.
* `ShapeRect` (from box, `SetRadius`, `Outline()` with arcs), `ShapeSegment` (`GetSeg`),
  `ShapeCircle` (`GetRadius`), `ShapeSimple`, `ShapeEllipse` (`ConvertToPolyline( maxError )`),
  `Shape::Type()`, `Shape::Centre()`, `Shape::BBox()`, `Shape::Rotate`.
* `BezierPoly::GetPoly( points, tolerance )`.
* `convert_basic_shapes_to_polygon`: `TransformRoundChamferedRectToPolygon`,
  `TransformOvalToPolygon` (incl. the `aMinSegCount` argument), `TransformCircleToPolygon`,
  `TransformTrapezoidToPolygon`; `GetArcToSegmentCount`; `ERROR_INSIDE` / `ERROR_OUTSIDE`;
  `CORNER_STRATEGY`.
* `GetCircleToPolyCorrection` with a way to disable it for a scope
  (`DISABLE_ARC_RADIUS_CORRECTION`, a static flag in `geometry_utils.cpp:L90-105`). Without
  `unsafe` this needs a thread-local guard or an explicit parameter; whichever is chosen,
  `GenerateLayerPoly` must be able to switch it off for all conversions it triggers, including
  the ones inside model methods.

### 5.3 `kicad-font`

* `FONT::GetFont( name )` (default = stroke font), `FONT::IsOutline()`.
* `FONT::Draw( gal, text, position, attributes, metrics )` driving a sink equivalent to
  `CALLBACK_GAL` (`include/callback_gal.h`, `common/callback_gal.cpp`): stroke glyph segments
  delivered as integer points (**truncated**, not rounded), outline glyphs delivered as fractured
  `ShapeLineChain` outlines. `CALLBACK_GAL` itself (77 lines) belongs in `kicad-font`.
* `TEXT_ATTRIBUTES` (`m_Angle`, `m_StrokeWidth`, `m_Italic`, `m_Bold`, `m_Underlined`, `m_Halign`,
  `m_Valign`, `m_Size`, `m_Mirrored`, `m_Multiline`, `m_LineSpacing`, `m_KeepUpright`),
  `GR_TEXT_H_ALIGN_T`, `GR_TEXT_V_ALIGN_T`, `KIFONT::METRICS::Default()`.
* `GetPenSizeForBold( int )`, `GRTextWidth( text, font, size, thickness, bold, italic, metrics )`
  (`common/gr_text.cpp`; SVG hidden text only).

### 5.4 Board model (`kicad-pcb`)

Board: `Drawings()`, `Footprints()`, `Tracks()`, `Zones()` in file order; `GetDesignSettings()`
(`m_MaxError`, `m_SolderMaskMinWidth`, `m_SolderMaskExpansion`, `m_LineThickness[ LAYER_CLASS_FAB ]`,
`GetAuxOrigin()`, `GetBoardThickness()`, `GetStackupDescriptor()`, `m_HasStackup`, `m_NetSettings`,
`m_TentViasFront/Back`, `GetDrillSymbolProfile()` (tier 3)); `GetPlotOptions()`;
`GetPageSettings()`; `GetTitleBlock()` (`GetRevision()`, `GetTitle()`); `GetProject()` /
text-variable expansion (`ExpandTextVars`, `ResolveTextVar`); `GetProperties()`; `GetFileName()`;
`GetLayerName( layer )`; `GetEnabledLayers()`; `GetCopperLayerCount()`; `GetCurrentVariant()`,
`SetCurrentVariant`, `GetVariantDescription`; `ComputeBoundingBox( false, false )`;
`GetBoardEdgesBoundingBox()`; `GetBoardPolygonOutlines( polys, false )`;
`GetPadWithCastellatedAttrCount()`, `GetPadWithPressFitAttrCount()`; `GetEmbeddedFiles()->
GetAreFontsEmbedded()`; a "no net" `NETINFO_ITEM`.

`PAGE_INFO`: `GetSizeIU( iuPerMils )`, `GetSizeMils()`, `GetWidthIU/GetHeightIU`, `A4` (portrait
flag), `SetWidthMils/SetHeightMils`.

Footprint: `Pads()`, `GraphicalItems()`, `GetFields()`, `Reference()`, `Value()`,
`GetReference()`, `GetLayer()`, `GetPosition()`, `GetOrientation()`, `GetDNPForVariant( name )`,
`GetPrivateLayers()`, `GetCourtyard( layer )`, `GetEffectiveShape()`,
`TransformPadsToPolySet( out, layer, clearance, maxError, errorLoc )`,
`GetLocalSolderMaskMargin()` & co. (through the pad accessors).

Pad: `GetLayerSet()`, `IsOnLayer`, `IsOnCopperLayer()`, `FlashLayer( LSET )`,
`FlashLayer( layer )`, `GetAttribute()`, `GetProperty()`, `GetNumber()`, `GetPinFunction()`,
`GetNetname()`, `GetParentFootprint()`, `GetPosition()`, `GetOrientation()`,
`ShapePos( layer )`, `GetShape( layer )`, `GetSize( layer )`, `GetDelta( layer )`,
`GetOffset( layer )`, `GetRoundRectCornerRadius( layer )`, `GetRoundRectRadiusRatio( layer )`,
`GetChamferRectRatio( layer )`, `GetChamferPositions( layer )`, `GetDrillSize()`,
`GetDrillShape()`, `HasHole()`, `GetBoundingBox()`, `GetPrimitives( layer )` (+ `IsProxyItem`,
`GetShape`, `GetCenter`, `GetTopLeft`, `GetBotRight` of a primitive),
`GetSolderMaskExpansion( layer )`, `GetSolderPasteMargin( layer )`, `GetOwnClearance( layer )`,
`GetEffectivePolygon( layer, ERROR_INSIDE )`, `TransformShapeToPolygon( out, layer, clearance,
maxError, errorLoc [, ignoreLineWidth] )`, `MergePrimitivesAsPolygon( layer, out )`,
`Padstack()` (`Drill()`, `SecondaryDrill()`, `TertiaryDrill()`, `FrontPostMachining()`,
`BackPostMachining()`, `UnconnectedLayerMode()`), and on a **clone**: `SetSize`, `SetShape`,
`SetAnchorPadShape`, `SetRoundRectCornerRadius`, `SetRoundRectRadiusRatio`, `SetPosition`,
`SetOffset`, `SetOrientation`, `DeletePrimitivesList`, `AddPrimitivePoly( layer, polys, 0, true )`.
A clone must still resolve its parent footprint (reference, margins) and the board.

Track / arc / via: `Type`, `GetLayer()`, `GetLayerSet()`, `IsOnLayer`, `GetStart()`, `GetEnd()`,
`GetWidth()`, `GetNetname()`, `GetSolderMaskExpansion()`, `TransformShapeToPolygon`;
arc: `GetCenter()`, `GetArcAngleStart()`, `GetAngle()`, `GetRadius()`, `IsDegenerated( 10 )`;
via: `GetWidth( layer )`, `GetDrillValue()` (netclass default when the drill is 0),
`GetViaType()`, `LayerPair()`, `TopLayer()`, `BottomLayer()`, `FlashLayer( LSET )`, `IsTented`,
`ResolveFillingMode/CappingMode/Front|BackCoveringMode/PluggingMode/TentingMode`, `Padstack()`.

Zone: `GetIsRuleArea()`, `GetLayerSet()`, `IsOnLayer`, `IsOnCopperLayer()`, `GetNetname()`,
`GetFilledPolysList( layer )`, `GetFill( layer )`, `IsIsland( layer, index )`,
`GetOwnClearance( layer )`.

Shapes (`PCB_SHAPE` / `EDA_SHAPE`): `GetLayer()`, `GetLayerSet()`, `GetWidth()`, `GetStroke()`
(`GetLineStyle`, `GetWidth`), `GetShape()`, `GetStart/GetEnd/GetCenter/GetRadius`, `GetArcAngle`,
`CalcArcAngles`, `GetPolyShape`, `IsPolyShapeValid`, `GetCornerRadius`, `GetRectCorners`,
ellipse accessors, `IsSolidFill`, `IsHatchedFill`, `GetHatching()`, `HasSolderMask`,
`GetSolderMaskExpansion`, `GetNetCode`, `GetNetname`, `GetParentFootprint`, `GetMaxError`,
`MakeEffectiveShapesForStroking( width )`, `TransformShapeToPolySet`, line endings:
`GetStartEnding/GetEndEnding` (`GetStyle`, `GetShortenDepth`, `GetEffectiveSize`, stroke),
`ShortenSegmentForEndings`, `ShortenArcForEndings`, `ShortenedBezierCurve`,
`ShortenedBezierPolyline`, `GetShortenedBodyPolyPoints`, `GetEndingTangents`,
`GetLineEndingEndpoints`; a way to build temporary shapes (segment, circle, polygon) on a layer.

Text family (`EDA_TEXT`, `PCB_TEXT`, `PCB_FIELD`, `PCB_TEXTBOX`, `PCB_TABLECELL`,
`PCB_DIMENSION_BASE`): `GetLayer`, `IsVisible`, `IsKnockout`, `GetText`, `GetShownText`,
`GetTextPos`, `GetDrawPos`, `GetTextSize`, `GetEffectiveTextPenWidth`, `GetDrawRotation`,
`GetAttributes`, `GetFontMetrics`, `GetDrawFont( renderSettings )`, `IsMultilineAllowed`,
`GetLinePositions( settings, out, count )`, `GetRenderCache` (outline fonts),
`TransformTextToPolySet( out, clearance, maxError, errorLoc )`, field `IsReference/IsValue`,
text box `IsBorderEnabled`, `GetPolygonalBoxShape`; dimension `GetShapes()`,
`GetLineThickness()`; table `GetCells()`, `DrawBorders( callback )`, `GetParentFootprint`;
barcode `TransformShapeToPolySet`; target `GetShape/GetSize/GetWidth/GetPosition`.

Other: `STROKE_PARAMS::Stroke( shape, style, width, renderSettings, callback )`
(`common/stroke_params.cpp:L51-250`) with the dash/dot/gap lengths of `RENDER_SETTINGS`
(`common/render_settings.cpp:L63-80`); `BOARD_STACKUP` (items, types, sub-layers, thickness,
colour, material, epsilon/loss tangent formatting, `GetDrillDepth`, `SynchronizeWithBoard`,
finish, edge options); standard colour list `GetStandardColors( type )` and `IsPrmSpecified`
(`board_stackup_manager/stackup_predefined_prms.*`); `NET_SETTINGS` (default netclass clearance,
all netclasses with `HasClearance`); `PCB_PLOT_PARAMS` struct.

### 5.5 Connectivity, DRC, zones

* `PAD::FlashLayer` / `PCB_VIA::FlashLayer` for items with "remove unused layers": zone layer
  overrides stored on the item (`ZLO_FORCE_FLASHED`) and
  `CONNECTIVITY_DATA::IsConnectedOnLayer( item, layer, { trace, arc, via, pad } )`
  (`pad.cpp:L756-778`, `pcb_track.cpp:L2356-2404`). A board without such items needs no
  connectivity.
* `GetSolderMaskExpansion` (pads, tracks, shapes) and `GetSolderPasteMargin` consult the DRC
  engine when the board has custom rules for `SOLDER_MASK_EXPANSION`, `SOLDER_PASTE_ABS_MARGIN`,
  `SOLDER_PASTE_REL_MARGIN` (`pad.cpp:L1960-2110`, `pcb_track.cpp:L1797-1825`); `GetOwnClearance`
  (job file) uses the clearance resolution. Without custom rules these are plain look-ups
  (pad -> footprint -> board). `kicad-fab` therefore only depends on `kicad-drc` through the
  model's "upward hook"; tier 1 can run with the local-override chain alone.
* `--check-zones`: `kicad-zone` (tier 2).

### 5.6 Things read from the environment in C++ that become parameters

Build version string (`GetBuildVersion()`); current local time and UTC offset
(`wxDateTime::Now()`, `Format( "%z" )`); colour theme (`COLOR_SETTINGS`; default theme
`common/settings/builtin_color_themes.h`, only needed for SVG colour mode);
`ADVANCED_CFG::m_SmallDrillMarkSize` (0.35 mm); the drawing-sheet file; project text variables.

---

## 6. Output formats, exactly

All files use `\n` line ends (the oracle runs on Linux). `<ver>` is `GetBuildVersion()`. Dates:
`<iso>` = local time `YYYY-MM-DDTHH:MM:SS`, `<tz>` = UTC offset `+hh:mm` (from `%z` with a colon
inserted after the third character when it is longer than 3 characters, `M:L43-51`).

### 6.1 File names

`PCB_PLOTTER::BuildPlotFileName( fn, outDir, suffix, ext )` (`PL:L374-403`): start from the board
file name; directory = `outDir`; extension = `ext`; `suffix` trimmed, then each of the characters
`* ? \ / : " < > | % .` replaced by `_`; if the suffix is not empty the base name becomes
`<board>-<suffix>`.

| Output | Name |
|---|---|
| Gerber layer | suffix = `board.GetLayerName( layer )` (the user's layer name if renamed), e.g. `demo-F_Cu.gtl`. Extension `gbr`, or with Protel extensions (`PP:L39-75`): `gtl` F.Cu, `gbl` B.Cu, `g<n>` inner copper with `n = CopperLayerToOrdinal` = `( id - B_Cu ) / 2` (In1.Cu -> `g1`), `gta`/`gba` adhesive, `gtp`/`gbp` paste, `gto`/`gbo` silk, `gts`/`gbs` mask, `gm1` Edge.Cuts, everything else `gbr` |
| Gerber job file | `<board>-job.gbrjob` |
| SVG (multi mode) | `<board>-<layer name>.svg`; single mode: the given path, or `<board>.svg` |
| Drill, main (`DB:L194-246`) | `<board><ext1>.drl` with `ext1` = `` (merged PTH+NPTH), `-PTH`, `-NPTH`, `-<pair>` for blind/buried (`front-in1`, `in1-in2`, `in2-back`; `layerName`: `front`, `back`, `in<(id - B_Cu)/2>`), `_Backdrills_Drill_<a>_<b>` for backdrill spans (`a`, `b` = 1-based positions of the stored start and end layer in `LSET::AllCuMask( copperCount ).UIOrder()`) |
| Drill, Gerber format (`DG:L567-576`) | the same with `-drl` appended to the base name and extension `gbr`: `<board>-PTH-drl.gbr`, `<board>-NPTH-drl.gbr`, ... |
| Backdrill pair copy | Excellon `<board>-<pair>-backdrill.drl`; Gerber `<board>-<pair>-backdrill-drl.gbr` |
| Via protection (`DB:L249-300`) | `<board>-filling-<pair>.gbr`, `<board>-capping-<pair>.gbr`, and with `<side>` = `front` or `back`: `<board>-covering-<side>.gbr`, `<board>-plugging-<side>.gbr`, `<board>-tenting-<side>.gbr` (no `-drl`) |
| Drill map | `<board><ext1>-drl_map.gbr` or `.svg` (base class name, i.e. without the `-drl` of the Gerber writer) |
| Drill report | `--report-path`, default `<outdir>/<board>-drill.rpt` |

### 6.2 Gerber layer file

```
<TF header lines, 6.3>
%FSLAX46Y46*%                                         (45 for precision 5)
G04 Gerber Fmt 4.6, Leading zero omitted, Abs format (unit mm)*
G04 Created by KiCad (PCBNEW <ver>) date <YYYY-MM-DD HH:MM:SS>*
%MOMM*%
%LPD*%
G01*
G04 APERTURE LIST*
[G04 Aperture macros list*
 <macro definitions>
 G04 Aperture macros list end*]
<aperture definitions>
G04 APERTURE END LIST*
<body>
M02*
```

Body records (all from `fmt::println`):

| Record | Syntax |
|---|---|
| select aperture | `D<n>*` (n >= 10, no zero padding) |
| move / draw / flash | `X<int>Y<int>D02*`, `...D01*`, `...D03*` (D-code two digits; integers without leading zeros, `-` sign when negative) |
| arc | `G75*` / `G02*` or `G03*` / `X<int>Y<int>I<int>J<int>D01*` / `G01*` (four lines per arc) |
| region | `G36*`, first point with `D02`, `G01*`, further points with `D01` (arcs as above), `G37*` |
| polarity | `%LPD*%`, `%LPC*%` |
| object attribute | X2 `%TO.P,<ref>,<pad>[,<function>]*%`, `%TO.N,<net>*%`, `%TO.C,<ref>*%`; X1 `G04 #@! TO.N,<net>*` etc. |
| delete all object attributes | `%TD*%` / `G04 #@! TD*` |
| region aperture function | `%TA.AperFunction,<f>*%` before, `%TD.AperFunction*%` after (X1: `G04 #@! TA.AperFunction,<f>*`, `G04 #@! TD.AperFunction*`) |

Aperture definitions (`writeApertureList`, `G:L606-825`), one per aperture in creation order:

```
[<TA.AperFunction line>]              only when the aperture has an attribute
%ADD<dcode><definition>*%
[%TD*%]                                 only when the aperture has an attribute (X1: G04 #@! TD*)
```

with `s = fscale` (mm per IU) and every number printed as `{:#f}` (6 decimals):

| Type | `<definition>` |
|---|---|
| `AT_CIRCLE`, `AT_PLOTTING` | `C,<size.x*s>` |
| `AT_RECT` | `R,<x*s>X<y*s>` |
| `AT_OVAL` | `O,<x*s>X<y*s>` |
| `AT_REGULAR_POLYn` | `P,<2*radius*s>X<n>X<rotation deg>` |
| `AM_ROUND_RECT` | `RoundRect,<r*s>X` + 4 x `<cx*s>X<cy*s>X` + `0`. The corners are the centres of the corner circles: `h = ( size.x/2 - r, size.y/2 - r )`, each component at least 10 IU; `(-hx,-hy), (hx,-hy), (hx,hy), (-hx,hy)`, each `RotatePoint( c, -rotation )`. Y is **not** negated here |
| `AM_ROT_RECT` | `RotRect,<x*s>X<y*s>X<rotation deg>` |
| `APER_MACRO_OUTLINE4P`..`8P` | `Outline<n>P,` + per corner `<x*s>X<-y*s>X` + `<rotation deg>` (Y negated as integer before scaling) |
| `AM_ROTATED_OVAL` | `HorizOval,<size.y*s>X<sx*s>X<-sy*s>X<ex*s>X<-ey*s>X0` with `len = size.x - size.y`, `start = RotatePoint( ( len/2, 0 ), rotation )`, `end = RotatePoint( ( -len/2, 0 ), rotation )` |
| `AM_FREE_POLYGON` | `FreePoly<index>,<rotation deg>` |

Aperture attribute lines (`M:L153-358`): X2 `%TA.AperFunction,<f>*%\n`, X1 `G04 #@!
TA.AperFunction,<f>*\n`; the slot-hole attribute is preceded by `G04 aperture for slot hole*\n`.
`<f>` values used on layers: `EtchedComponent`, `Conductor`, `Profile`, `ViaPad`, `NonConductor`,
`ComponentPad`, `SMDPad,CuDef`, `BGAPad,CuDef`, `ConnectorPad`, `WasherPad`, `HeatsinkPad`,
`TestPad`, `FiducialPad,Global`, `FiducialPad,Local`, `CastellatedPad`; on drill files:
`ViaDrill`, `BackDrill`, `ComponentDrill`, `ComponentDrill,PressFit`, `CastellatedDrill`;
custom: `Other,<text>`.

Macro definitions: the fixed ones are the string literals of `gbr_plotter_aperture_macros.h`
(multi-line, with `0 ...*` comment primitives; copy byte for byte). Free polygons
(`APER_MACRO_FREEPOLY::Format`, `G:L1864-1894`):

```
%AMFreePoly<id>*
4,1,<corner count>,<x0>,<-y0>,<x1>,<-y1>, ... ,<x0>,<-y0>,          newline after every 20 pairs
$1*%
```

The loop prints `corner count + 1` pairs (all corners, then the first again), each as
`{:#f},{:#f},`; the corner list already contains the closing point when it comes from
`FlashPadCustom`. Port the loop literally.

Strings in attributes (`FormatStringToGerber`, `M:L521-542`; `ConvertNotAllowedCharsInGerber`,
`M:L444-503`): unless the string starts and ends with `"`, every `\`, `%`, `*`, `,` and every code
unit above 0x7F becomes `\uXXXX` (4 upper-case hex digits of `code & 0xFFFF`). Pad names and pin
functions use the `GBR_DATA_FIELD` path with `useUTF8 = false`, `escape = false`: same conversion
(a `"` is only escaped in quoting mode, which is not used).

### 6.3 Gerber header attributes (`PP:L290-372`)

X2 form; in X1 mode every `%` is removed and `G04 #@! ` is prepended (`makeStringCompatX1`), which
gives lines ending in `*`:

```
%TF.GenerationSoftware,KiCad,Pcbnew,<ver>*%
%TF.CreationDate,<iso><tz>*%
%TF.ProjectId,<name>,<guid>,<rev>*%
%TF.SameCoordinates,<Original | PX<hex>PY<hex>>*%
%TF.FileFunction,<function>*%
[%TF.FilePolarity,<Positive|Negative>*%]
```

* `<name>`: board file base name; `,` -> `_`, `*` -> `*`, `%` -> `%`; then
  `wxString::ToAscii()` (non-ASCII characters become `_`).
* `<guid>` (`GbrMakeProjectGUIDfromString`, `M:L78-150`): from the board file name *with*
  extension, padded with `X` to 16 characters; 15 characters are consumed as code point `& 0xFF`:
  `c0c1c2c3-c4c5-4<3 hex>-9<3 hex>-c9..c14` where the two 3-digit groups are
  `( c6 << 4 & 0xFF0 ) + ( c7 >> 4 & 0x0F )` and `( ( c7 & 0x0F ) << 8 ) + c8`, lower-case hex.
  Check values: `basic_test.kicad_pcb` -> `62617369-635f-4746-9573-742e6b696361`,
  `Issue24143.kicad_pcb` -> `49737375-6532-4343-9134-332e6b696361` (from the golden files).
* `<rev>`: title block revision with text variables expanded, same character replacement, ASCII;
  `rev?` when empty.
* `SameCoordinates`: `Original`, unless **the board's stored plot options** have `useauxorigin`
  and both aux origin coordinates are non-zero: then `PX%xPY%x` of the aux origin in IU. The
  `--use-drill-file-origin` flag does not influence this line.
* `<function>` (`GetGerberFileFunctionAttribute`, `PP:L78-203`): `Copper,L1,Top`;
  `Copper,L<copper count>,Bot`; inner `Copper,L<(id - B_Cu)/2 + 1>,Inr`; `Glue,Top|Bot`;
  `Legend,Top|Bot`; `Soldermask,Top|Bot`; `Paste,Top|Bot`; `Profile,NP` (Edge.Cuts);
  `OtherDrawing,Comment` (Dwgs.User); `Other,Comment` (Cmts.User); `Other,ECO1`, `Other,ECO2`;
  `AssemblyDrawing,Top|Bot` (Fab); everything else `Other,User`.
* Polarity (`PP:L206-258`): `Positive` for copper, adhesive, silk, paste; `Negative` for mask;
  no line for other layers.

### 6.4 Excellon (`DX:L232-430`, `L458-643`)

```
M48
; DRILL file KiCad <ver> date <iso>                              \
; FORMAT={<L>:<R> or -:-/ absolute / <metric|inch> / <zero text>}  |
; #@! TF.CreationDate,<iso><tz>                                    |  omitted with
; #@! TF.GenerationSoftware,Kicad,Pcbnew,<ver>                     |  --excellon-min-header
; #@! TF.FileFunction,<plating>,<l1>,<l2>[,<kind>]                 |
FMAT,2                                                           /
<METRIC|INCH>[,TZ|,LZ]
per tool:   [; #@! TA.AperFunction,...]      (not with minimal header)
            T<n>C<diameter>
            [; Backdrill[ stub <a>mm (<b>")[ to <c>mm (<d>")]][, post-machining]]
            [; Post-machining]
%
G90
G05
round holes, in sorted order:   [T<n>] when the tool changes, [comments], X<x>Y<y>
slots, in sorted order:         [T<n>] when the tool changes, [comments], then
     route mode:  G00X<x0>Y<y0> / M15 / G01X<x1>Y<y1> / M16 / G05        (5 lines)
     G85 mode:    X<x0>Y<y0>G85X<x1>Y<y1> / G05                          (2 lines)
M30
```

* `<zero text>`: `decimal`, `suppress leading zeros`, `suppress trailing zeros`, `keep zeros`;
  the precision is `-:-` for decimal, else `m_Lhs:m_Rhs` (3:3 mm, 2:4 inch). Unit line suffix:
  `,TZ` for suppress-leading, `,LZ` for suppress-trailing, nothing otherwise.
* `<plating>` etc. (`BuildFileFunctionAttributeString( span, type, true )`, `DB:L358-461`):
  `NonPlated` for NPTH files and backdrills, `MixedPlating` for the merged file, else `Plated`;
  `<l1>,<l2>` = 1-based copper layer numbers of the span's pair, smaller first (`F_Cu` = 1,
  `B_Cu` = copper count, inner = `(id - B_Cu)/2 + 1`); `<kind>`: `Blind` for backdrills, `NPTH`,
  nothing for the merged file, else `PTH` (1..n), `Blind` (touches one outer layer), `Buried`.
* Hole attribute comments (`writeHoleAttribute`, `DX:L185-229`): `Plated,PTH,ViaDrill`;
  `Plated,Buried,ViaDrill`; `NonPlated,BackDrill`; `Plated,PTH,ComponentDrill`;
  `Plated,PTH,CastelletedDrill` (sic); `Plated,PTH,ComponentDrill,PressFit`;
  `NonPlated,NPTH,ComponentDrill`; unknown: `; #@! TD`.
* Tool diameter: `{:.Nf}` of `diameter * conv` with N = 3 (mm, `conv = 1.0 / IU_PER_MM`) or 4
  (inch, `conv = 0.001 / IU_PER_MILS`).
* Coordinates: `x = ( pos.x - offset.x ) * conv`; `y = ( pos.y - offset.y )`, negated unless
  `--excellon-mirror-y`, `* conv`. Slot end points: `delta = |size.x - size.y| / 2` along the
  longer of the *source* axes (Y when `size.x < size.y`, else X), both points rotated about the
  hole centre with `RotatePoint( &x, &y, cx, cy, orientation )` before the Y negation.
* Coordinate text (`writeCoordinates`, `DX:L458-556`): decimal: `{:.Nf}` (N = 3 or 4), trailing
  zeros removed but one digit kept after the point (`200.0`, `-148.25`, `0.0157`).
  Suppress-leading: value multiplied by 10 `m_Rhs` times (a loop of `*= 10`, not `pow`), `KiROUND`,
  `%d`. Keep zeros: `%0*d` with width `m_Lhs + m_Rhs` (+1 when negative). Suppress-trailing: as
  keep-zeros, then trailing `0` characters removed while more than one character remains.
* Post-machining comments per hit (`DX:L735-775`): `; Post-machining <front|back>
  <countersink|counterbore>[ dia <v><mm|in>][ depth <v><mm|in>][ angle <a>deg]`; the angle is
  stored in tenths of a degree and printed with 0 or 1 decimals. Only vias carry post-machining
  into the NC files (4.13).

### 6.5 Gerber drill and protection files (`DG:L209-482`)

A normal X2 Gerber (6.2) produced with net attributes on, macros enabled, viewport offset = the
drill origin, precision 5 or 6, and this header: the four lines of `AddGerberX2Header`, then
`%TF.FileFunction,<plating>,<l1>,<l2>,<kind>,<Drill|Rout|Mixed>*%` (as in 6.4 plus the last field:
`Drill` when the list has only round holes, `Rout` only slots, `Mixed` both, nothing when empty),
then `%TF.FilePolarity,Positive*%`.

Body, in sorted hole order: round hole -> `FlashPadCircle( pos, min( size ) )`; slot ->
`ThickSegment` between the two centres (`convertOblong2Segment`, `DG:L531-552`) with width
`min( size )`. Aperture attribute: via -> `ViaDrill` (`BackDrill` for backdrills); pad ->
`CastellatedDrill` / `ComponentDrill,PressFit` / `ComponentDrill` (slots: the oblong variant with
its comment line). Pads add `TO.C,<reference>`; when a via follows a pad the attributes are
cleared first (`EndBlock` -> `%TD*%`). The golden file `basic_test-PTH-drl.gbr` shows the result.

Protection files: header `AddGerberX2Header` lines, then the literal line
`%TF,FileFunction,Other,<Capping|Filling|Covering-Back|Covering-Front|Plugging-Back|Plugging-Front|Tenting-Back|Tenting-Front>*%`
(with a **comma** after `TF`; a unit test asserts this text), then the polarity line. Body: one
circle flash per via having the property, aperture attribute `Other,<Capping|Filling|Covering|
Plugging|Tenting>`; diameter = drill, except covering and tenting: the via pad diameter on that
side.

### 6.6 Drill report (`DB:L850-993`, `L1022-1070`)

```
Drill report for <board file name with extension>
Created on <iso>

Copper Layer Stackup:
    =============================================================
    L<n, left aligned width 2>:  <layer name, left aligned width 25> <front|inN|back>      per copper layer, UIOrder


<section per span, in EnumerateDrillSpans order>
<NPTH section>
```

Section: `Drill file '<name>' contains\n`, then `    plated through holes:\n` /
`    backdrill span: '<start layer name>' to '<end layer name>':\n` /
`    holes connecting layer pair: '<top> and <bottom>' (<blind|buried> vias):\n`, the separator
line (4 spaces + 61 `=`), the tool lines, an empty line, `    Total plated holes count <n>\n`
(`backdrilled` for backdrill spans), two empty lines. The NPTH section starts with
`Not plated through holes are merged with plated holes\n` (merged mode) or
`Drill file '<npth name>' contains\n`, then `    unplated through holes:\n`, separator, tools,
empty line, `    Total unplated holes count <n>\n` (no trailing empty lines).

Tool line: `    T<n>  {:2.3f}mm  {:2.4f}"  ` followed by exactly one of `(1 hole`,
`(1 hole)  (with 1 slot`, `(<n> holes)`, `(<n> holes)  (with 1 slot`,
`(<n> holes)  (with <k> slots`, then optionally `, castellated`, `, press-fit`, then `)\n`. Note
the resulting **double closing parenthesis** in the common case `(<n> holes))`; port as is. Only
tools of the section's class are listed (plated / unplated-non-backdrill / backdrill), but the
tool number is the index in the whole tool list.

### 6.7 Gerber job file (`J`)

`std::setw( 2 ) << ordered_json << std::endl`: pretty-printed with 2-space indentation, one
member or array element per line, `"key": value`, no trailing spaces, final newline; strings in
UTF-8 with standard JSON escapes; integers as such; doubles in shortest round-trip form, always
with a decimal point or exponent (`1.6`, `0.2`, `50.0`). All doubles here are `mapValue` results
(multiples of 0.0001 between 0 and a few thousand), which stay in plain decimal notation.
Because the key order and number format are part of the output, write the JSON with a small
dedicated emitter (or `serde_json` with `preserve_order` only after checking the float format on
the test corpus).

```
Header:          GenerationSoftware { Vendor "KiCad", Application "Pcbnew", Version <ver> },
                 CreationDate "<iso><tz>"
GeneralSpecs:    ProjectId { Name <board base name, UTF-8>, GUID <guid>, Revision <rev or "rev?"> },
                 Size { X, Y }, LayerNumber <int>, BoardThickness,
                 [Finish <text>], [ImpedanceControlled true], [Castellated true], [Press-fit true],
                 [EdgePlating true], [EdgeConnector true, EdgeConnectorBevelled <bool>]
DesignRules:     [ { Layers "Outer", PadToPad, PadToTrack, TrackToTrack,
                     [MinLineWidth], [TrackToRegion], [RegionToRegion] }
                   [, { Layers "Inner", ... same keys ... }] ]
FilesAttributes: [ { Path <file name>, FileFunction <f>, FilePolarity <Positive|Negative> }, ... ]
MaterialStackup: [ { Type <Copper|Legend|SolderMask|SolderPaste|Dielectric>, [Color], [Thickness],
                     [Material], [DielectricConstant], [LossTangent], Name, [Notes] }, ... ]
```

`FileFunction` here is **not** the Gerber header string: `Copper,L<n>,<Top|Inr|Bot>`,
`Other,User` (user layers), `Glue,Top|Bot`, `SolderPaste,Top|Bot`, `Legend,Top|Bot`,
`SolderMask,Top|Bot` (polarity `Negative`), `Profile`, `AssemblyDrawing,Top|Bot`. Name and
revision are not character-replaced here (only UTF-8 encoded). Stackup details (dielectric
`Name` = `<copper above>/<copper below>[ (i/n)]`, `Notes` = `Type: <text> (from <a> to <b>)`,
colour mapping to `R<r>G<g>B<b>` for user colours and through the standard colour table
otherwise): `J:L574-771`.

### 6.8 SVG

Header (`S:L792-870`), with `P = m_precision` decimals:

```
<?xml version="1.0" standalone="no"?>
 <!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" 
 "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"> 
<svg
  xmlns:svg="http://www.w3.org/2000/svg"
  xmlns="http://www.w3.org/2000/svg"
  xmlns:xlink="http://www.w3.org/1999/xlink"
  xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
  version="1.1"
  width="<w>mm" height="<h>mm" viewBox="<x0> <y0> <w> <h>">
<title>SVG Image created as <file name> date <iso> </title>
  <desc>Image generated by <creator> </desc>
<g style="fill:#RRGGBB; fill-opacity:<a>;stroke:#RRGGBB; stroke-opacity:<1>;
stroke-linecap:round; stroke-linejoin:round;"
 transform="translate(0 0) scale(1 1)">
```

(mind the trailing blanks on lines 2 and 3). Style group, per change (`S:L240-335`):
`</g>\n<g style="` + `fill:none; ` or `fill:#RRGGBB; fill-opacity:<a>; ` + `stroke:none;` or
`\nstroke:#RRGGBB; stroke-width:<w>; stroke-opacity:<a>; \nstroke-linecap:round;
stroke-linejoin:round;` + dash array (`stroke-dasharray:none;` for solid) + `">\n`. Element
syntax per primitive: read it off the `fmt::print` calls; the file ends with `</g> \n</svg>\n`.

---

## 7. Backward compatibility to keep

* `(pcbplotparams)` legacy tokens and the pre-20240819 layer-mask remapping are handled by the
  parser (`pcb-io.md` 7). Semantics that matter here: `excludeedgelayer no` adds `Edge_Cuts` to
  the common layers (visible with `--board-plot-params`); `viasonmask` is only kept as
  `GetLegacyPlotViaOnMaskLayer()` for the loader to derive via tenting; `mode`, `hpgl*`,
  `svguseinch`, `pscolor`, `plotinvisibletext` are read and ignored; `outputformat 0` (HPGL) is a
  valid stored value for which `StartPlotBoard` fails.
* X1 output (`--no-x2`): attributes as `G04 #@! ` comments. Protel extensions. Aperture macros
  can be disabled. All three are compatibility features for old CAM software and must work.
* Excellon `G85` slots (`--excellon-oval-format alternate`, the CLI default) and the three
  zero-suppression formats.
* `pcb export svg`: `--page-size-mode` (legacy) and the deprecated default single mode with its
  warning text on stdout (`command_pcb_export_svg.cpp:L195-206`).
* Boards from old versions (the artwork regression data are a 4.0.7 `.kicad_pcb` and a legacy
  `.brd`): everything version-specific is resolved at load time (legacy zone fills, old pad
  shapes); the plot code has no version checks. Legacy `.brd` input is outside the port.
* Typo-level oddities that existing consumers and tests see and that must not be "fixed":
  `TF,FileFunction` in protection files, `CastelletedDrill` and `Kicad` (lower-case c) in Excellon
  comments, `(<n> holes))` in the report, `Soldermask` (Gerber) vs `SolderMask` (job file).

---

## 8. Multi-threading and global state

The subsystem is single-threaded. There is no thread pool, no `std::async`, no parallel loop in
any file of section 2.1. What exists and how to port it:

| C++ | Port |
|---|---|
| Gerber body written to a temp file (`wxFileName::CreateTempFileName`), re-read in `EndPlot` | in-memory buffer |
| `LOCALE_IO` in `GERBER_WRITER::createDrillFile` (forces the "C" locale for `printf`) | nothing: Rust formatting is locale-independent |
| `static std::vector<VECTOR2I> cornerList` in `PSLIKE_PLOTTER::FlashPadTrapez` (`PS:L203`) | local variable |
| `DISABLE_ARC_RADIUS_CORRECTION` (process-global flag) | scoped guard or parameter (5.2) |
| `wxDateTime::Now()`, `GetBuildVersion()`, `ADVANCED_CFG::GetCfg()`, `Pgm()`/settings for colours | explicit parameters (5.6) |
| `wxSafeYield()` in `PCB_PLOTTER::Plot`, progress reporter calls | drop |
| Mutation of the board during plotting: pad size/shape (`L:L492-680`), page settings and aux origin (`PL:L92-115`), visible layers (`DB:L524-527`), current variant | clones / overrides; the board stays immutable |
| Indirect: `--check-zones` (zone filler thread pool), connectivity built at load, the font's markup cache mutex | owned by those subsystems; each is ported sequentially |

A sequential port is therefore the same algorithm; determinism only requires the ordering rules
of 4.17.

---

## 9. Validation

### 9.1 Existing C++ tests to port

| Test file | Cases | What they assert | Package |
|---|---|---|---|
| `qa/tests/common/test_gerber_plotter.cpp` | `GerberPlotter/ZeroLengthSegmentEmitsSingleFlash` | plotter with viewport `( (0,0), 2540.0, 1.0, false )`; `ThickSegment( (0,0), (0,0), 2 mm, null )`: output contains `C,2.000000`, no `C,0.000000`, no `G02`/`G03`, exactly one `D01*`, and both `X0Y0D02*` and `X0Y0D01*` (issue 24131) | PLOT-03 |
| `qa/tests/common/test_svg_plotter.cpp` | `SvgPlotter/` `FillOpacityMatchesColorAlpha`, `FillOpacityFullyOpaque`, `FillOpacityPreservedThroughSetColor`, `StrokeOpacityMatchesColorAlpha`, `DefaultPlotViewBoxIsOrigin`, `MirroredSvgPreservesPlotOrigin`, `FitToBoardUsesViewBox`, `MirroredFitToBoardUsesMirroredViewBox` | page 1000 x 1000 mils, viewport `( (0,0), 1, 1.0, mirror )`, colour mode, one filled `Rect` of 4000 IU with width 10. Opacity strings `0.5000`, `1.0000`, `0.2500` ...; viewBox `0 0 25.4 25.4` / `-25.4 0 ...` / `50.8 50.8 ...` / `-76.2 50.8 ...`; rect `x` attribute `0.254000`, `-10.414000`, `50.800000`, `-60.960000` | PLOT-06 |
| `qa/tests/pcbnew/test_plot_edge_cuts_drill_marks.cpp` | `NoDrillFlashesOnEdgeCuts`, `DrillMarksPlottedOnEdgeCutsSvg`, `NoDrillMarksOnGerberFromStoredOptions` | `PlotBoardLayers` on a one-pad board: Gerber Edge.Cuts and F.Cu contain no `D03` even with drill marks requested (issues 24416, 25532); SVG Edge.Cuts with full drill marks contains a `<circle` (issue 24867) | FAB-04, FAB-11 |
| `qa/tests/pcbnew/test_plot_pad_mask.cpp` | `RoundRectMaskMatchesInflatedOutline`, `RoundRectGerberMaskApertureHasCorrectRadius`, `PthPadKeepsMaskOpeningWhenCopperRemoved` | round-rect pad 2 x 2 mm, ratio 0.125, mask margin 1 mm: plotted with macros disabled the F.Mask region has exactly 4 arcs whose radius / bounding box ratio is 0.3125 (issue 24327); a PTH pad with "remove unused layers" still flashes on both mask layers | FAB-04 (first and third are model tests) |
| `qa/tests/pcbnew/test_plot_footprint_scaled_text.cpp` | `ScaledFootprintReferencePlotsScaled`, `ScaledFootprintGraphicTextPlotsScaled` | bounding box of all `D01/D02` coordinates of the F.SilkS Gerber doubles (within 12 %) when the footprint text size doubles | FAB-06 |
| `qa/tests/pcbnew/test_pcb_textbox.cpp` | `EmptyKnockoutPlotsTheBox` | a knockout text box without text still plots its box polygon on F.SilkS | FAB-06 |
| `qa/tests/pcbnew/test_cam_backdrill.cpp` | `GerberDrillBoardDefaultsCreateProtectionFiles`, `GerberBackdrillDoesNotInheritViaProtection`, `BackdrillCamOutputs` (Excellon and Gerber parts), `FrontAndBackBackdrillCamOutputs`, `DualBackdrillSameViaCamOutputs`, `GerberDrillPrecision`, `DrillFileLayerOrderInFilename`, `DrillReportNullReporter`, `DrillReportWithTools`, `DrillReportBackdrillHoleCount` | created file names (`_Backdrills_Drill_1_2.drl`, `_Backdrills_Drill_6_4.drl`, `...-in2-back...`, `-tenting-front.gbr`, counts of `-filling-` / `-tenting-` files); texts `TF.FileFunction,NonPlated,1,4,Blind`, `; Backdrill`, `post-machining`, `; backdrill`, `; Post-machining front countersink dia 0.600mm depth 0.150mm angle 90deg`, `%TA.AperFunction,BackDrill*%`, `%TF.FileFunction,NonPlated,1,4,Blind,Drill*%`, `TF,FileFunction,Other,Tenting-Front`, `Fmt 4.5` / `Fmt 4.6`; report contains `T1`, `0.300mm`, `Total plated holes count 4`. Boards: `qa/data/pcbnew/issue25021/backdrill.kicad_pcb`, `drc_via_stack/FillFromBoardHonoursTheBoardDefault.kicad_pcb`, and boards built in code (the port builds the same boards through the model API). The `OdbPp*` cases are out of scope | FAB-01, FAB-02, FAB-08 |
| `qa/tests/pcbnew/test_plot_job_layers.cpp` | `AllCopperLayersSurviveSmallerBoard` | only the `ResolvePlotLayers` behaviour is in scope (job JSON is not) | FAB-07 |
| `qa/tests/pcbnew/test_fab_pad_geometry.cpp` | - | out of scope (IPC-2581 / ODB++ fab model), but its `PAD_CAPTURE_PLOTTER` (a `PLOTTER` that records flashes) is a good pattern for port unit tests | - |
| `qa/tests/cli/test_pcb.py` | `test_pcb_export_drill` (4 parameter sets), `test_pcb_export_gerber` (3), `test_pcb_export_svg` (2) | see 9.2 | FAB-02, FAB-08, FAB-07, FAB-11 |

### 9.2 Data and golden files

* `qa/data/cli/basic_test/basic_test.kicad_pcb` with `basic_test_excellon_default.drl`
  (`--format excellon`), `basic_test_excellon_inches.drl` (`-u in`),
  `basic_test_excellon_mirror.drl` (`--excellon-mirror-y`), `basic_test-PTH-drl.gbr`
  (`--format gerber`). The Python test (`utils.textdiff_files`) compares text line by line after
  **skipping the first 5 lines** (Excellon) or **9 lines** (Gerber), i.e. the header lines that
  contain version and date. Everything after must be byte-identical. The port should use the
  stricter normalisation of 9.3 instead of skipping (the goldens were written by an older build
  whose `; DRILL file ... date` line still carried a UTC offset; the current code prints none).
* `qa/data/cli/artwork_generation_regressions/`: `ZoneFill-4.0.7.kicad_pcb`, `ZoneFill-Legacy.brd`
  with `-F_Cu` / `-B_Cu` `.gbr` and `.svg` goldens made with KiCad 4.0.7 / 2023 builds, and
  `Issue24143.kicad_pcb` + `Issue24143-F_Cu.gbr` (made with a 10.99 build, X1 mode, 89 lines).
  The Python tests compare these **as images** (`kicad-cli gerber diff --format json --no-align`,
  maximum 0.5 % / 0.0 % difference; SVG rendered with cairosvg at 850 dpi and compared after a
  1-pixel erosion). Only `Issue24143-F_Cu.gbr` is recent enough to be a byte-level reference
  (command: `pcb export gerbers --no-x2 --use-drill-file-origin --layers F.Cu`); it exercises net
  attributes, a stroked polygon outline and fractured fill regions and is small enough to commit
  as a golden for FAB-05.
* `qa/data/cli/basic_test/basic_test.svg` and `basic_test_nobg_bnw_nods.svg` are *schematic*
  exports, but they come from the same `SVG_PLOTTER` and show the header and the lazy `<g>`
  style groups. They were written by an older build (e.g. `stroke-opacity:1;` and no
  `stroke-dasharray:none;`), so use them to understand the structure, not as byte references.
  In general the oracle is the byte reference; of the goldens only the drill files (asserted
  textually after the skipped header lines) and `Issue24143-F_Cu.gbr` (checked against the
  current code while writing this document) are known to match today's output.
* Boards with interesting pads, vias and drills in `qa/data/pcbnew/`: `custom_pads.kicad_pcb`,
  `padstacks*.kicad_pcb`, `odbpp/pad_margin_shapes.kicad_pcb`, `odbpp/custom_pad_mask_split.kicad_pcb`,
  `odbpp/pad_roles.kicad_pcb`, `odbpp/npth_pressfit(_slot).kicad_pcb`, `odbpp/slot_machining.kicad_pcb`,
  `odbpp/pad_backdrill.kicad_pcb`, `odbpp/reverse_backdrill.kicad_pcb`, `odbpp/via_in_pad.kicad_pcb`,
  `odbpp/knockout_textbox_unicode.kicad_pcb`, `issue25021/backdrill.kicad_pcb`, `drc_via_stack/*`.
* Corpus for differential runs: 355 boards under `qa/data/pcbnew`, 20 under `demos`.

### 9.3 Differential tests against the oracle (`/home/user/kicad-oracle/bin/kicad-cli`)

Run the oracle and the port with the same arguments into two directories and compare (1) the set
of file names and (2) every file byte for byte after normalisation. Run the oracle with
`KICAD_CONFIG_HOME` pointing at a scratch directory (as `qa/tests/cli/utils.py` does) so that it
neither reads nor writes user settings. None of these outputs contains a random UUID: the only
GUID-like value (`TF.ProjectId`) is a pure function of the board file name.

Lines to normalise (replace the whole line by a fixed token on both sides):

| Output | Pattern |
|---|---|
| Gerber (layers, drill, protection, maps) | `^%TF\.GenerationSoftware,KiCad,Pcbnew,.*\*%$`, `^%TF\.CreationDate,.*\*%$`, `^G04 Created by KiCad \(PCBNEW .*\) date .*\*$`, and their X1 forms `^G04 #@! TF\.GenerationSoftware,.*\*$`, `^G04 #@! TF\.CreationDate,.*\*$` |
| Excellon | `^; DRILL file KiCad .* date .*$`, `^; #@! TF\.CreationDate,.*$`, `^; #@! TF\.GenerationSoftware,Kicad,Pcbnew,.*$` |
| Job file | JSON members `Header.GenerationSoftware.Version` and `Header.CreationDate` (compare the rest as text) |
| SVG | `^<title>SVG Image created as .* date .* </title>$` (keep the file-name part if both sides use the same output name) |
| Drill report | `^Created on .*$` |

When the port is given the same version string as the oracle (`10.99.0-unknown`) only the date
lines differ.

Suites (each over the corpus; start with `qa/data/cli`, then the pad/drill boards of 9.2, then
everything):

* **D-drill**: `pcb export drill` with: defaults; `-u in`; each of
  `--excellon-zeros-format decimal|suppressleading|suppresstrailing|keep` in both units;
  `--excellon-oval-format route`; `--excellon-separate-th`; `--excellon-min-header`;
  `--excellon-mirror-y`; `--drill-origin plot`; `--format gerber` with `--gerber-precision 5` and
  6 and with `--generate-tenting`; `--generate-report`; later `--generate-map --map-format
  gerberx2|svg`.
* **D-gerbers**: `pcb export gerbers` with: defaults (all enabled layers, Protel extensions, X2,
  job file); `--no-protel-ext`; `--no-x2`; `--no-netlist`; `--disable-aperture-macros`;
  `--precision 5`; `--use-drill-file-origin`; `--layers F.Cu,B.Cu,Edge.Cuts --cl Edge.Cuts`;
  `--erd --ev`; `--sp --spn --sdnp --cdnp`; `--subtract-soldermask`; `--board-plot-params`.
  Compare per layer file so that a failure names the layer; report the first differing line
  with the preceding `D<n>*` and attribute lines for context.
* **D-svg** (tier 2): `pcb export svg --mode-single --exclude-drawing-sheet --black-and-white`
  per layer, plus `--fit-page-to-board`, `--mirror`, `--negative`, `--drill-shape-opt 0|1|2`.

Keep three result classes per board: identical; differs only in files that involve a documented
gap (drill maps, barcodes, outline fonts, `@{}` text); real difference. Boards that the port's
loader rejects are skipped and listed. For triage of geometry-only differences
`kicad-cli gerber diff --format json --no-align` (if the oracle was built with gerbview) gives a
quick "same picture or not" answer, but the pass criterion is byte equality.

Small normalised outputs of the oracle for `basic_test` (drill variants, F.Cu, F.Mask, F.SilkS,
Edge.Cuts, job file) and `Issue24143` are committed under `crates/kicad-fab/tests/golden/` so the
default test run needs no oracle.

---

## 10. Rust module layout and work packages

### 10.1 Layout

```
crates/kicad-plot/src/
  lib.rs
  plotter.rs                      plotter.h, plotter.cpp (trait Plotter, PlotterState, defaults)
  plot_text.rs                    PLOTTER::Text / PlotText (plotter.cpp L548-667)
  drill_markers.rs                drill_markers.h/.cpp
  common_plot_functions.rs        GetDefaultPlotExtension [PlotDrawingSheet: tier 3]
  gbr_metadata.rs                 gbr_metadata.h/.cpp
  gbr_netlist_metadata.rs         gbr_netlist_metadata.h
  gbr_plotter_apertures.rs        gbr_plotter_apertures.h + APER_MACRO_FREEPOLY* (GERBER_plotter.cpp L1858-1919)
  gbr_plotter_aperture_macros.rs  gbr_plotter_aperture_macros.h (string constants)
  plotter_gerber.rs               plotter_gerber.h, GERBER_plotter.cpp L1-1264, L1807-1856
  plotter_gerber_flash.rs         GERBER_plotter.cpp L1266-1805
  plotters_pslike.rs              PSLIKE_PLOTTER parts (plotters_pslike.h L41-143, PS_plotter.cpp L45-226)
  plotter_svg.rs                  SVG_plotter.cpp, plotters_pslike.h L712-867
  PORT_STATUS.md
crates/kicad-fab/src/
  lib.rs                          OutputFile, PlotEnv, FabError, public export_* functions
  pcbplot.rs                      pcbplot.h/.cpp
  pcb_plotter.rs                  pcb_plotter.h/.cpp
  plot_board_layers.rs            plot_board_layers.cpp L128-171, L247-931, L1278-1595
  plot_layer_poly.rs              plot_board_layers.cpp L49-125, L1020-1275
  plot_brditems_plotter.rs        plot_brditems_plotter.cpp: struct, getColor, pads, zones, drill marks
  plot_brditems_shapes.rs         ... PlotBoardGraphicItem, PlotFootprintGraphicItems, PlotShape,
                                  PlotDimension, PlotPcbTarget, PlotBarCode, PlotTableBorders
  plot_brditems_text.rs           ... PlotFootprintTextItems, PlotText
  line_ending_plot.rs             common/line_ending.cpp (GetShapes, Plot) unless the model owns it
  drill/{drill_span,drill_operation,drill_enumerator}.rs
  exporters/gendrill_writer_base.rs      tool list, names, file function, report
  exporters/gendrill_map.rs              gendrill_writer_base.cpp L303-355, L464-847, L996-1019
  exporters/gendrill_excellon_writer.rs
  exporters/gendrill_gerber_writer.rs
  exporters/gerber_jobfile_writer.rs
  jobs/{job_export_pcb_plot,job_export_pcb_gerbers,job_export_pcb_drill,job_export_pcb_svg}.rs
  jobs_handler.rs                 pcbnew_jobs_handler.cpp: convertLayerArg, JobExportGerbers,
                                  JobExportDrill, JobExportSvg (pure: bytes in, OutputFile out)
  PORT_STATUS.md
```

The plot engines write into `String` buffers and return them from `end_plot`; nothing in either
crate touches the file system, the clock or the environment (wasm). `kicad-cli` parses the
options of 1.2-1.4, reads the board, calls `export_*`, writes the files and prints the reporter
messages.

Polymorphism: a `Plotter` trait with provided methods for everything `PLOTTER` implements in the
base class; `GerberPlotter` and `SvgPlotter` implement it. Where C++ calls an overridden method
from a base method (e.g. `PLOTTER::ThickSegment` -> virtual `Circle`, `SetCurrentLineWidth`,
`PenTo`), trait default methods calling required methods give the same dispatch. Where C++ calls
`PLOTTER::X` explicitly from an override (`GERBER_PLOTTER::ThickSegment` -> `PLOTTER::ThickSegment`),
expose the base implementation as a free function `plotter::thick_segment_base( &mut dyn Plotter,
... )`.

### 10.2 Work packages

Lines are C++ lines to read and port (comments included). Tier 1 = first end-to-end result,
2 = completeness, 3 = stretch.

| Id | Title | C++ files (line ranges) | Lines | Depends on | Tier | Validating tests |
|---|---|---|---|---|---|---|
| PLOT-01 | Plotter trait, base primitives, drill markers | `plotter.h`; `plotter.cpp` L1-546; `drill_markers.h/.cpp`; `common_plot_functions.cpp` L34-46 | 1250 | geom (angles, `RotatePoint`, `CalcArcCenter`, line chain, Bezier), core | 1 | new unit tests with a recording plotter: transform table, `polyArc` point lists, `ThickOval`, all 58 markers |
| PLOT-02 | Gerber metadata and attribute text | `gbr_metadata.h/.cpp`; `gbr_netlist_metadata.h` | 1100 | core | 1 | GUID check values of 6.3; `FormatNetAttribute` sequences (incl. the clear-and-repeat case of 4.2.3); escaping table; all `TA.AperFunction` strings |
| PLOT-03 | Gerber plotter core | `plotter_gerber.h`; `gbr_plotter_apertures.h`; `GERBER_plotter.cpp` L1-1264, L1849-1856 | 1900 | PLOT-01, PLOT-02 | 1 | `ZeroLengthSegmentEmitsSingleFlash`; new tests: D-code numbering, `EndPlot` splice, arc threshold, 4.5 rounding |
| PLOT-04 | Gerber pad flashes and aperture macros | `GERBER_plotter.cpp` L1266-1805, L1858-1919; `gbr_plotter_aperture_macros.h` | 900 | PLOT-03, geom `TransformRoundChamferedRectToPolygon` | 1 | one test per row of the table in 4.2.5 (both macro modes); aperture definition strings of 6.2; `RoundRectGerberMaskApertureHasCorrectRadius` (with FAB-04) |
| PLOT-05 | Text through the font | `plotter.cpp` L548-667; `GERBER_plotter.cpp` L1807-1846; `callback_gal.h/.cpp` if not already in `kicad-font` | 300 | PLOT-01, PLOT-03, kicad-font | 1 | `ScaledFootprint*` (with FAB-06); D-gerbers silk layers |
| PLOT-06 | SVG plotter | `SVG_plotter.cpp`; `PS_plotter.cpp` L45-226; `plotters_pslike.h` L41-143, L712-867 | 1400 | PLOT-01 (PLOT-05 for text) | 1 | the 8 `SvgPlotter` cases |
| FAB-01 | Drill model, tool list, names | `drill/drill_span.h`, `drill_operation.h`, `drill_enumerator.h/.cpp`; `gendrill_writer_base.h`; `gendrill_writer_base.cpp` L47-300, L358-461 | 1300 | model: pads, vias, padstack, stackup depth | 1 | `GerberBackdrillDoesNotInheritViaProtection` (enumeration part), `DrillFileLayerOrderInFilename`; name table of 6.1 |
| FAB-02 | Excellon writer, drill report, drill job | `gendrill_excellon_writer.h/.cpp`; `gendrill_writer_base.cpp` L850-993, L1022-1070; `job_export_pcb_drill.*`; `JobExportDrill` (`H:L2316-2472`); CLI table 1.3 | 1400 | FAB-01 | 1 | three `basic_test_excellon_*.drl` goldens; Excellon parts of `BackdrillCamOutputs`, `FrontAndBack...`, `DualBackdrill...`, `DrillReport*`; D-drill |
| FAB-03 | Board plot driver, Gerber header, file names | `pcbplot.h/.cpp`; `plot_board_layers.cpp` L128-171, L247-351, L1278-1595; `pcb_plotter.cpp` L357-423 | 1100 | PLOT-03; model: board, page, title block, plot params | 1 | header of `Issue24143-F_Cu.gbr` and `basic_test-PTH-drl.gbr`; extension / file-function / polarity tables; `initializePlotter` offsets |
| FAB-04 | Standard layer: pads, vias, tracks, zones | `plot_board_layers.cpp` L354-931; `plot_brditems_plotter.cpp` L72-398, L902-957 | 1100 | FAB-03, PLOT-04; model pad shapes; connectivity (flashing) | 1 | `NoDrillFlashesOnEdgeCuts`, `NoDrillMarksOnGerberFromStoredOptions`, `RoundRectGerberMaskApertureHasCorrectRadius`; D-gerbers copper / mask / paste |
| FAB-05 | Graphic items | `plot_brditems_plotter.cpp` L489-783, L960-1406; `line_ending.cpp` L62-190, L240-265 | 1100 | FAB-03; model shapes, dimensions, tables; `STROKE_PARAMS::Stroke` | 1 | `Issue24143-F_Cu.gbr` byte-exact after normalisation; D-gerbers Edge.Cuts, Fab, CrtYd, user layers |
| FAB-06 | Text items | `plot_brditems_plotter.cpp` L401-486, L786-899 | 300 | FAB-03, PLOT-05; model text | 1 | `ScaledFootprintReferencePlotsScaled`, `ScaledFootprintGraphicTextPlotsScaled`, `EmptyKnockoutPlotsTheBox`; D-gerbers silk |
| FAB-07 | `gerbers` job | `job_export_pcb_plot.*`, `job_export_pcb_gerber(s).*`; `H:L632-696`, `L1750-1923`; `pcb_plotter.cpp` L426-555; CLI table 1.2 | 800 | FAB-03 (useful as soon as FAB-04 exists) | 1 | `AllCopperLayersSurviveSmallerBoard` (layer resolution); layer-argument table; D-gerbers file sets |
| FAB-08 | Gerber drill and via-protection files | `gendrill_gerber_writer.h/.cpp` | 760 | FAB-01, PLOT-03, FAB-03 (X2 header) | 1 | `basic_test-PTH-drl.gbr`; `GerberDrillBoardDefaultsCreateProtectionFiles`, `GerberDrillPrecision`, Gerber parts of `BackdrillCamOutputs`; D-drill `--format gerber` |
| FAB-09 | Gerber job file | `gerber_jobfile_writer.h/.cpp` | 950 | FAB-07; model: stackup, net settings, own clearance | 1 | D-gerbers `.gbrjob`; unit tests for `mapValue` and the JSON emitter |
| FAB-T1 | Differential harness and goldens | (test code only: runner, normaliser, corpus lists; 9.3) | - | FAB-02 first, then each package | 1 | itself |
| FAB-10 | Solder mask minimum width, clipped silk | `plot_board_layers.cpp` L49-125, L1020-1275 | 400 | FAB-04..06; geom clipper operations; model `Transform*ToPolySet` | 2 | D-gerbers on boards with `solder_mask_min_width > 0`; `--subtract-soldermask --cl ...` |
| FAB-11 | SVG board plots and drill marks | `pcb_plotter.cpp` L68-354; `H:L1464-1579`; `plot_brditems_plotter.cpp` L1409-1512; `job_export_pcb_svg.*`; default colour theme subset | 900 | PLOT-06, FAB-03..06 | 2 | `DrillMarksPlottedOnEdgeCutsSvg`; D-svg; wasm preview smoke test |
| FAB-12 | Drill maps | `gendrill_writer_base.cpp` L303-355, L464-847, L996-1019 | 500 | FAB-01, FAB-05, PLOT-05 | 2 | D-drill `--generate-map` (gerberx2, svg) |
| PLOT-07 | Drawing sheet model and parser | `common/drawing_sheet/ds_data_item.cpp`, `ds_data_model.cpp`, `drawing_sheet_parser.cpp`, `drawing_sheet_default_description.cpp` | 2150 | sexpr, font | 3 | D-svg without `--exclude-drawing-sheet` |
| PLOT-08 | Drawing sheet plotting | `ds_draw_item.cpp`; `common_plot_functions.cpp` L49-207 | 800 | PLOT-07, PLOT-05 | 3 | same; D-gerbers `--ibt` |
| FAB-13 | Drill symbols, drill charts, generated tables | `plot_brditems_plotter.cpp` L1515-1656; `drill/drill_symbol_*`, `drill_chart_*`; `pcb_drill_map.cpp`; `pcb_drill_chart.cpp`; `board_tables/*` | 2900 | FAB-01, FAB-05; model | 3 | `test_drill_chart.cpp` |
| FAB-14 | Remaining job glue | `JobExportGerber` (`H:L2214-2314`); `--check-zones` wiring; variants | 250 | FAB-07, kicad-zone | 3 | D-gerbers `--check-zones` |

Suggested order for an early end-to-end result:

1. **Drill slice** (needs no plot engine): FAB-01 -> FAB-02 -> FAB-T1. Result: `pcb export drill`
   byte-identical for `basic_test` and then the corpus.
2. **Gerber engine**: PLOT-01, PLOT-02 in parallel -> PLOT-03 -> PLOT-04.
3. **Copper slice**: FAB-03 -> FAB-04 -> FAB-07. Result: copper, mask and paste layers of boards
   without custom pads are byte-identical (no clipper and no font needed: tracks, vias, stored
   zone fills, circle / oval / rect / round-rect pads).
4. FAB-05 (Edge.Cuts and graphics), PLOT-05 + FAB-06 (silk text, needs `kicad-font`), FAB-08,
   FAB-09 complete tier 1. PLOT-06 can run in parallel at any time after PLOT-01.

---

## 11. Risks, traps and open questions

### 11.1 Traps (things that silently change the output)

1. **Scale factor and truncation** (4.1): compute the Gerber factor in two steps; keep the
   truncating `VECTOR2D -> VECTOR2I` conversions; never replace `KiROUND` by `round()` on an
   already scaled value or vice versa.
2. **Aperture and attribute state** (4.2.2, 4.2.3): unused `D<n>*` lines, re-emitted attributes
   after a `TD`, the attribute-less re-selection inside `polyArc` are all part of the reference
   output.
3. **Two `PlotPoly` overloads behave differently** on Gerber (4.2.4): the corner-list version
   strokes a filled polygon when `width != 0`, the line-chain version does not.
4. **Polygon clipper parity**: custom pads (always, even with zero margin), chamfered pads and
   trapezoids with a mask margin, filled graphic polygons (`Fracture`), knockout text, the
   solder-mask minimum-width path and clipped silk all print clipper output vertex by vertex.
   Byte equality for these needs Clipper2-identical results from `kicad-geom`.
5. **Polygonisation parity**: `TransformRoundChamferedRectToPolygon` with
   `GetPlotterArcHighDef()` (5080 IU) feeds `FreePoly` macros and SVG round-rect pads;
   `SHAPE_ELLIPSE::ConvertToPolyline`, Bezier subdivision and text outlines likewise.
6. **Stroke font parity**: every silkscreen byte comes from glyph coordinates (doubles, truncated
   to integers at the callback). The font port decides whether silk layers match.
7. **Pad mutation** (4.8): port with clones; the clone must answer `GetParentFootprint()`,
   margins and `GetEffectivePolygon` like a real pad, and the corner radius must take the
   ratio round trip.
8. **Footprint text quirks** (4.10): dimension text in footprints is plotted twice; graphic text
   visibility is not checked; `${REFERENCE}` / `${VALUE}` are matched as literal strings before
   expansion.
9. **Round-rect pads always use the `RoundRect` macro**, also unrotated ones; rectangles use
   `R,` only at cardinal angles and the test is done on the un-normalised angle, ovals on the
   normalised one.
10. **`TF.SameCoordinates` reads the board's stored plot options**, not the options in use (6.3).
11. **CLI defaults differ from job and from board defaults** (1.2): `--sdnp` / `--cdnp` off,
    precision 6, Protel extensions on, G85 slots, merged PTH/NPTH, drawing sheet on for SVG.
12. **`--board-plot-params`** uses the stored options wholesale: all enabled copper layers are
    plotted regardless of the stored selection; `psnegative yes` puts a filled rectangle into
    every Gerber; `plotframeref yes` plots the drawing sheet; `psa4output yes` shifts the plot
    offset; a stored non-Gerber `outputformat` makes `StartPlotBoard` create that engine while
    the files keep Gerber names. The port can only reproduce the last case for SVG; for the other
    formats return a clear error (see Q3).
13. **File names use the board's (user-renamable) layer names** with character replacement, the
    Gerber header uses fixed function strings; do not derive one from the other.
14. **Zone fills are plotted as stored.** Nothing refills or warns; an unfilled or stale zone
    gives the same (wrong) artwork as in C++. `--check-zones` needs `kicad-zone`.
15. **Empty-file rules differ per drill format** (4.14): Excellon writes empty main and NPTH
    files, Gerber writes none.
16. **Report and comment oddities** (7) are literal.
17. **`std::sort` instability** for coincident, otherwise equal holes (4.17).
18. **Job-file numbers**: `mapValue` prints with `%.4f` and re-parses through `long double`
    (`sscanf "%Lg"`) before narrowing to `double`; parse with a correctly rounded decimal parser
    and accept the theoretical double-rounding difference. The JSON float text must follow
    nlohmann's shortest-round-trip format.
19. **Dates**: three different formats are in use (`YYYY-MM-DD HH:MM:SS` in the Gerber comment,
    `...T...` without offset in Excellon / report / SVG, `...T...+hh:mm` in `TF.CreationDate`,
    the Excellon `TF.CreationDate` comment and the job file). All derive from one timestamp
    parameter.
20. **Gerber drill maps and protection files create their own `GERBER_PLOTTER`** with different
    settings from layer plots (X2 always on, net attributes on or off, precision 5 for maps).

### 11.2 Known gaps to record in `PORT_STATUS.md` until the stretch packages exist

* Drawing sheet / title block plotting (`--ibt` for Gerber; the default of `pcb export svg`).
  Until PLOT-07/08 the port's CLI must reject or ignore the request explicitly (Q4).
* Drill maps in PDF / PostScript / DXF (the CLI default map format is `pdf`): only `gerberx2` and
  `svg` are ported; other formats return an error.
* `PCB_DRILL_MAP` symbols, drill chart symbols, regeneration of generated tables before plotting
  (table text is plotted as stored in the file).
* Barcodes (need a zint-equivalent encoder in the model), outline (TrueType) fonts, `@{...}` text
  expressions, reference images in the drawing sheet.
* Colour themes other than the built-in default (SVG colour mode).
* The DRC-rule-driven mask/paste margins when custom rules for those constraints exist (falls
  back to the local-override chain until `kicad-drc` provides the hook).

### 11.3 Open questions for the orchestrator

* **Q1** Where do the small shared types live so that neither `kicad-plot` nor `kicad-pcb` has to
  depend on the other: `RenderSettings` (default pen width, dash / gap / dot lengths, default
  font, layer name) used by the plot engines *and* by `STROKE_PARAMS::Stroke`; `Color4D` with the
  named colours `BLACK`, `WHITE`, `LIGHTGRAY`; `FILL_T`; `LINE_STYLE`; `PlotFormat`;
  `DrillMarks`. Proposal: `kicad-core`.
* **Q2** `kicad-fab` needs `kicad-connectivity` (pad/via flashing) and, for full fidelity of
  mask/paste margins and the job file's clearances, the DRC engine's rule evaluation. The crate
  graph in `PLAN.md` has no `kicad-fab -> kicad-drc` edge. Proposal: the model exposes these as
  context hooks (as the model design already plans for `GetOwnClearance`), `kicad-fab` stays
  independent of `kicad-drc`, and `kicad-cli` wires a DRC-backed context when it has one.
* **Q3** `--board-plot-params` with a stored non-Gerber `outputformat`: reproduce (SVG content in
  Gerber-named files), force Gerber, or error? C++ behaviour is accidental; proposal: error with
  a clear message, noted as a deviation.
* **Q4** `pcb export svg` plots the drawing sheet by default. Before PLOT-07/08: should the
  port's CLI default to `--exclude-drawing-sheet` (deviation) or fail unless the flag is given?
* **Q5** Should the drill model (`pcbnew/drill/drill_enumerator.*`) live in `kicad-fab` (proposed
  here) or in `kicad-pcb` next to the drill chart / drill map items that also use it (tier 3)?
* **Q6** Reporter text ("Created file '...'", "Plotted to '...'.", "Done.") is observable on
  stdout and one CLI test greps it. Proposal: `kicad-fab` reports the same English strings
  through the `Reporter`, the CLI prints them.
* **Q7** The oracle wrapper exists but its binary was not built yet when this analysis was
  written, so no statement in this document was checked against oracle output; the Gerber
  examples come from the golden files in `qa/data/cli`. The first differential run (FAB-T1 on
  `basic_test`) should be used to confirm the header lines of section 6 before the bulk of the
  work starts.





