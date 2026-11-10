# FAB-11: SVG board plots, drill marks, the svg export job

Crate: kicad-fab

C++ (FAB-11 row of rust/docs/analysis/plot-fab.md section 10): pcbnew/pcb_plotter.cpp
L68-354 (PCB_PLOTTER::Plot: single / multi page modes, the layer sequence with common
layers, fit-to-board and page size handling, the output naming), pcbnew_jobs_handler.cpp
JobExportSvg (H:L1464-1579) with resolvePlotOutputPath, plot_brditems_plotter.cpp
L1409-1512 (PlotDrillMarks, plotOneDrillMark: the small / full drill mark shapes, the
white-on-copper knockout order), the drill-mark related parts FAB-04 left pending, the
colour handling for SVG output (getColor / PCB_RENDER_SETTINGS::LoadColors for the default
theme subset the svg export uses: black-and-white and the builtin default colours;
`--theme` with a user theme file is out of scope: report it as unsupported), and
common/jobs/job_export_pcb_svg.* as far as it is not in kicad-session yet. Then wire it:
kicad-session run_export_svg (replace its NotYetPorted error; minimal edits in kicad-session
are sanctioned), so that `kicad-rs pcb export svg` and the wasm exportSvg call work.

Fabrication package rules and verification: the paragraphs "Fabrication package rules" and
"Verification is against the reference kicad-cli" of rust/docs/packages/FAB-07.md apply
verbatim (read them there), with SVG in place of Gerber: `kicad-cli pcb export svg` output
must be reproduced byte for byte after normalising the date in `<title>` (plot-fab.md
section 9.3). Cover: --mode-single and --mode-multi, --black-and-white, --negative,
--mirror, --exclude-drawing-sheet (the drawing sheet itself is out of scope: with the sheet
requested return the documented NotPorted error), --drill-shape-opt 0 / 1 / 2, --page-size-mode
0 / 1 / 2, --fit-page-to-board, common layers, over every loadable corpus board under
300 KB for the default option set and over five boards for each other option. Port
qa/tests/pcbnew/test_plot_edge_cuts_drill_marks.cpp (three cases) and the two
test_pcb_export_svg cases of qa/tests/cli/test_pcb.py.

Orchestrator note: small leftover in this crate to do first - src/plot_brditems_text.rs
`pcb_text_transform_text_to_poly_set`: replace the `AnyItem::Dimension(_)`
`fab_port_pending!` arm with a call to the model method that now exists
(`dim.transform_text_to_poly_set(ctx, buffer, clearance, max_error, error_loc)`), and cover
knockout / struck-out dimension text inside a footprint with a crafted board against the
reference. Generated tables: `kicad_pcb::board_tables::refresh_generated_tables` must run
before plotting in the SVG job as PCB_PLOTTER::Plot does (pcb_plotter.cpp L90).
