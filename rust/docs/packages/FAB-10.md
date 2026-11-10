# FAB-10: Solder mask minimum width and clipped silk layers

Crate: kicad-fab

C++: pcbnew/plot_board_layers.cpp L49-125 (PlotClippedSilkLayer / the `--subtract-soldermask`
path) and L1020-1275 (PlotSolderMaskLayer with `solder_mask_min_width > 0`: the polygon
build per item kind in the C++ order, the inflate / deflate pair with its exact corner
strategy and max error, the zone handling, the final Fracture and plot), per the FAB-10 row
of rust/docs/analysis/plot-fab.md section 10. FAB-03 left `PlotSolderMaskLayer` (min width 0
-> PlotLayer) and the pending entries in plot_board_layers.rs: fill them in (move them to
plot_layer_poly.rs if that reads better; say so).

Fabrication package rules and verification: the paragraphs "Fabrication package rules" and
"Verification is against the reference kicad-cli" of rust/docs/packages/FAB-07.md apply
verbatim (read them there). Additionally for this package: Gerber F.Mask / B.Mask of boards
whose setup has `solder_mask_min_width > 0` (find them in the corpus with grep; craft two
small boards with pads closer than the minimum width, a via with tenting off, a zone on the
mask layer and text on the mask layer), and `pcb export gerbers --subtract-soldermask` with
silk over mask openings (--cl and without): byte-identical files. The polygons come from
Clipper operations whose operand order matters: when a file differs, find the first
differing operation by dumping intermediate polygons on both sides.
