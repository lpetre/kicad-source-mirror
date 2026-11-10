# FAB-13: Drill symbols and chart symbols in board plots

Crate: kicad-fab

C++ (FAB-13 row of rust/docs/analysis/plot-fab.md section 10, the plotting part - the
models exist in kicad-pcb since package M26: `kicad_pcb::drill` (drill symbol profile,
symbol assigner, the drill symbol cache `drill::drill_symbol_cache(ctx)`), PcbDrillMap,
the generated drill chart): pcbnew/plot_brditems_plotter.cpp L1515-1656 (PlotDrillSymbols
and PlotChartSymbols: which holes get a symbol on which layer, the symbol shapes through
PLOTTER::Marker and their sizes, the chart's symbol column), the call sites in
plot_board_layers.cpp (PlotStandardLayer / PlotOneBoardLayer: where drill-map items and
generated tables are plotted, the `BOARD::DrillSymbolLayers` logic), and whatever of
pcb_drill_map.cpp / drill_symbol_*.cpp the plot path needs that M26 left out
(`RefreshDrillSymbolLayers`, `ExpandBoundingBoxForDrillSymbols` if a plot reads them).
The stubs are the `fab_port_pending!` entries naming FAB-13 (rg FAB-13 crates/kicad-fab/src).

Fabrication package rules and verification: the paragraphs "Fabrication package rules" and
"Verification is against the reference kicad-cli" of rust/docs/packages/FAB-07.md apply
verbatim (read them there). For this package: the corpus runs that are "pending (FAB-13)"
in kicad-session tests/gerbers_job.rs (2), tests/svg_job.rs (1) and the kicad-fab plot
corpora (the drill-map layers: 26 files, and `--subtract-soldermask`: 2 runs) must become
identical; crafted boards with drill maps on user layers and drill charts (M26's crafted
boards under kicad-pcb tests/data/m26 have charts: reuse them) over through / blind /
micro vias, slots, NPTH, several tools and both symbol profiles (letters, size text),
plotted as Gerber and SVG: byte-identical. Port the plotting cases of
qa/tests/pcbnew/test_drill_chart.cpp / test_drill_map*.cpp if they exist.
