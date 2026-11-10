# FAB-12: Drill maps

Crate: kicad-fab

C++ (FAB-12 row of rust/docs/analysis/plot-fab.md section 10): pcbnew/exporters/
gendrill_writer_base.cpp L303-355, L464-847, L996-1019 and gen_drill_report_files.cpp as far
as the map needs it: GenDrillMapFile / genDrillMapFile (the plotter set-up per map format,
the board outline plot, the drill symbols per tool (plotter Marker shapes), the legend table
with its text layout and the exact strings, the scale and page fitting logic), for the map
formats whose plot engines exist in kicad-plot: gerberx2 and svg (pdf / ps / dxf: return the
documented Unsupported error naming the format). Wire it into the drill job
(`--generate-map --map-format gerberx2|svg`; FAB-02's job_export_drill has the hook) and
into the Gerber drill writer of FAB-08 if it has landed.

Fabrication package rules and verification: the paragraphs "Fabrication package rules" and
"Verification is against the reference kicad-cli" of rust/docs/packages/FAB-07.md apply
verbatim (read them there). For this package: `kicad-cli pcb export drill --generate-map
--map-format gerberx2` and `--map-format svg` (with excellon and with gerber drill files,
merged and --excellon-separate-th) on every loadable corpus board under 300 KB that has
holes: map files byte-identical after normalising date / version lines.
