# DS-1: Drawing sheet (model, parser, default sheet, draw items, plotting, DRC hook)

Crate: kicad-plot (new module tree src/drawing_sheet/ - the crate already depends on
kicad-sexpr, kicad-geom, kicad-core and kicad-font and holds PLOTTER; if a dependency is
missing say so before adding one), then the call sites in kicad-fab (StartPlotBoard /
PlotBoardLayers `plotframeref` path), kicad-drc (the misc provider's drawing-sheet loop:
`DrcTestProviderMisc::drawing_sheet`) and kicad-session (loading the sheet the way
kicad-cli does) - minimal edits there

Why: `kicad-cli pcb export svg` plots the drawing sheet by default and `pcb export gerbers
--ibt` plots it on request (the port returns NotPorted for both), and `kicad-cli pcb drc`
reports `unresolved_variable` for drawing-sheet texts (8 entries on 3 corpus boards are
missing in the port: kicad-drc/tests/oracle/drc12_cli_compare.py and
kicad-session/tests/drc12_corpus.rs list them as known differences).

C++ to port statement by statement (rows PLOT-07 and PLOT-08 of
rust/docs/analysis/plot-fab.md section 10; about 3,000 lines):
- common/drawing_sheet/ds_data_model.cpp, ds_data_item.cpp (DS_DATA_MODEL, DS_DATA_ITEM
  and subclasses: coordinates relative to the page corners, repeats, constraints, the
  first-page-only / subsequent options, text size and constraint fitting, polygons,
  bitmaps as data only), drawing_sheet_default_description.cpp (the built-in default
  sheet and the empty sheet), drawing_sheet_parser.cpp with drawing_sheet.keywords (the
  .kicad_wks reader incl. legacy `page_layout` forms; errors with their texts),
  ds_data_model_io.cpp only if a caller in scope writes a sheet (probably not: say so);
- ds_draw_item.cpp (DS_DRAW_ITEM_LIST::BuildDrawItemsList: the expansion of data items into
  draw items for a page, text variable expansion - title block fields, ${...} with the
  project / sheet context, the legacy %-format codes -, page / sheet numbering, the
  bounding boxes and GetShownText used by DRC);
- common/plotters/common_plot_functions.cpp L49-207 (PlotDrawingSheet: item dispatch,
  pen widths, colours, text plotting through PLOTTER::PlotText with the stroke font,
  bitmaps skipped as the C++ does for Gerber / SVG - check);
- the callers: plot_board_layers.cpp L1560-1640 (StartPlotBoard / PlotBoardLayers with
  plotframeref, sheet name / path / page arguments, the offsets and scaling applied), the
  kicad-cli side that picks the sheet (pcbnew_jobs_handler.cpp loadOverrideDrawingSheet,
  the project's drawing sheet file, `--drawing-sheet`, the default when none is set) and
  DRC_ENGINE::SetDrawingSheet / the misc provider loop (drc_test_provider_misc.cpp
  testTextVars: which items, which resolver, positions).

Verification (reference kicad-cli; COPIES; ORACLE.md):
1. `pcb export svg` WITHOUT --exclude-drawing-sheet (after the SVG job of kicad-fab exists:
   package FAB-11 is in the fabrication chain right now - if it has not landed, verify
   through the plot driver of kicad-fab directly and leave the CLI comparison as a ready
   test) and `pcb export gerbers --ibt` on twenty corpus boards with different page sizes,
   title blocks (all fields, comments 1-9, text variables), projects with a custom
   .kicad_wks (qa data and demos have several: find them), and crafted sheets covering
   every item kind, repeat, corner anchor and option: files byte-identical after the date /
   version normalisation.
2. DRC: the three boards with drawing-sheet unresolved_variable entries become identical;
   remove them from the known-difference lists.
3. Unit level: a C++ driver (the kicad-plot plot oracle pattern: scripted, built under the
   machine-wide flock) that loads a .kicad_wks / the default sheet, builds the draw item
   list for a page and prints every item (type, coordinates, pen, shown text, bbox) for
   several page formats and title blocks; Rust replay identical.
4. wasm32 build of the touched crates; the stroke-font-only rule stands (a sheet text in an
   outline font falls back like board text does: report how the reference treats it).

Other agents: the fabrication chain (FAB-10 -> FAB-11 -> FAB-12) is editing kicad-fab, a
stabilisation pass is editing kicad-drc providers, WIRE-1 is editing kicad-session / cli /
wasm: your edits there are the minimal call-site wiring, made at the end, after re-reading
the file; everything else lives in the new kicad-plot module.
