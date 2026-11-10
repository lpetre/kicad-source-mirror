# REV-FAB: Review of the fabrication output port (reviewer role)

Crate: kicad-fab and kicad-plot (review of the plot engines PLOT-*, the drill model and
writers FAB-01 / 02 / 08, the board plot driver and item plotting FAB-03 .. 06, the gerbers
job FAB-07, the job file FAB-09)

Gerber files are byte-identical to the reference on 15,506 files of 690 corpus runs, the
Gerber job file on 704 runs, drill files on every board without vias (the enumeration on
the real via model is being finished right now by package FAB-GLUE, which is followed in
this crate by FAB-10 mask minimum width, FAB-11 SVG, FAB-12 drill maps: those agents are
EDITING kicad-fab while you review - you edit nothing in kicad-fab/src; findings there go
into your document and, where a fix is needed, into `rust/docs/packages/FIX-FAB-<n>.md`;
in kicad-plot (finished, quiet) you may fix what you can prove, with a regression case
against the reference plot oracle of that crate).

The byte comparisons prove the paths the corpus and the default options exercise. Find
what they do not show:
1. Option coverage: for every option of `kicad-cli pcb export gerbers` and `pcb export
   drill` (rust/docs/ORACLE.md lists them) and every stored plot parameter of a board
   (PCB_PLOT_PARAMS: read pcb_plot_params.cpp for all of them) say which value combinations
   a test compares against the reference. Run the missing ones through `kicad-rs` (the
   binary, built with `cargo build -p kicad-cli`; reference on COPIES) on ten varied boards
   and report every difference with the smallest case: e.g. --subtract-soldermask,
   --disable-aperture-macros, --no-netlist, --ev / --erd / --hdnp / --sdnp / --cdnp / --sp
   / --spn / --ibt, --precision 5, --no-protel-ext, --check-zones, --variant, -D, layer
   lists with common layers, stored negative / mirror / A4 / scale / drill marks /
   plot-pads-on-silk style options, Excellon zeros formats x units x origin x mirror x
   minimal header x separate TH x oval format, Gerber drill precision.
2. Item coverage: which item kinds / shapes / states never occur on a compared layer
   (dashed strokes on copper, hatched fills, thermal-relief custom pads, pads with
   offsets and rotations in rotated flipped scaled footprints, arcs near 0 / 360 degrees,
   zero-width shapes, knockout text, multi-line and mirrored text, table cells with
   borders, dimensions of every kind, teardrops, zones with islands and hatching, vias
   with tenting / covering variants on mask layers, NPTH on copper, castellated pads,
   edge-plated boards, aperture macro pads of every kind and rotation). Craft boards for
   the gaps (reference = kicad-cli) and report.
3. Statement-level reading of the parts where a difference would hide: aperture list
   management and the order apertures are first used (it decides D-code numbers), aperture
   attribute emission and its clearing rules (TA / TO / TD and the "same attribute" cache),
   coordinate formatting and rounding at both precisions, arc output (G75, I / J, full
   circles, tiny arcs), region output, the Excellon tool table ordering and slot routing
   commands, number formats per unit and zeros mode.
4. Numeric rules of PORTING.md (KiROUND, x86 casts, float expression order) in kicad-plot
   and kicad-fab; wasm vs native equality of the produced bytes (run the wasm module on
   five boards through rust/web/test or a small node script and compare with native).

Write rust/docs/design/review-fab.md: the option and item coverage tables, findings (id,
severity, file:line, C++ reference, what differs for which input, status), and the FIX-FAB
package files you wrote. Verification of your own fixes: kicad-plot tests and oracles
green.
