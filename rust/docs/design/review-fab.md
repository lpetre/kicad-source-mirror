# Review of the fabrication output port (package REV-FAB)

Reviewer pass over `kicad-plot` (plot engines, Gerber metadata) and `kicad-fab` (drill model and
writers FAB-01 / 02 / 08, board plot driver and item plotting FAB-03 .. 06, gerbers job FAB-07,
job file FAB-09), judged by one thing: can `kicad-rs pcb export gerbers` / `pcb export drill`
write a byte that the reference `kicad-cli` (docs/ORACLE.md) does not write, on a path the
existing corpus comparisons do not exercise.

State of the tree: 2026-11-10. Two builds of the binary were used: one from 05:16 UTC (drill
export and the mask minimum width not wired yet) and one from 05:50 UTC (FAB-GLUE drill export,
FAB-10 mask minimum width and the `-D` handling of the session had landed in between). FAB-11
(SVG, drill marks), FAB-12 (drill maps), FAB-13 (drill chart symbols), the drawing sheet
(PLOT-07 / 08) and `BOARD::SetCurrentVariant` were still pending; everything that stops in one
of those stubs is listed as pending, not as a difference.

## 1. Result

About 2 300 differential runs of the two binaries on copies of 33 boards (17 corpus boards, the
7 crafted boards of the kicad-fab tests, 9 boards generated for this review) and of mutated
copies of them, every output file compared after normalising only the version / date lines,
plus exit code and stdout.

* Every option of both commands and every stored plot parameter that reaches the Gerber path
  behaves like the reference on every board that does not stop in a pending stub.
* The item kinds named in the assignment (dashed strokes on copper, hatched fills, arcs near
  0 / 360 degrees, degenerate shapes, knockout / multi-line / mirrored text, text boxes, tables
  with borders and merged cells, all five dimension kinds, every pad shape / property / margin /
  offset in rotated and flipped footprints, custom pads with thermal reliefs, zones with islands
  and hatching, blind / buried / micro vias, NPTH on copper, castellated pads, exotic net / pad /
  reference names) plot byte-identically: 6 crafted boards, 5 to 6 option sets each.
* One real difference was found, in a place no corpus board reaches: pads in a footprint with a
  non-uniform scale (F-03). Cause: the C++ modifies the board's pad in place while plotting and
  the restore is lossy under a footprint scale, so the pad carries a 1 nm perturbation into the
  layers plotted afterwards; the port plots a detached copy.
* The statement-level reading of the Gerber engine and of the Excellon number formatting found
  no deviation.
* wasm == native on 8 boards (178 Gerber / job files, 11 drill files).

| Severity | Count | Ids |
|---|---|---|
| blocker | 0 | |
| major | 0 | |
| minor | 4 | F-03 (open, FIX-FAB-1), F-04 (open, FIX-FAB-2), X-1 (open, FIX-FAB-3), F-01 (accepted) |
| note | 6 | F-02 (fixed meanwhile, no regression test), F-05 .. F-07 (pending packages), F-08, X-2 |

## 2. Method and tools

Everything is in `docs/design/review-fab/` (scratch output under `rust/target/rev-fab/work`,
the reference always runs on copies with a private `KICAD_ORACLE_HOME`):

| File | Purpose |
|---|---|
| `tools/fabcmp.py KIND TAG BOARD -- OPTS` | one board, one option set: both binaries, all files, exit code, stdout |
| `tools/matrix.py KIND SETS BOARD...` | option sets (`tag|options` lines) x boards |
| `tools/drill_matrix.py full|pair|other BOARD...` | the 256 Excellon combinations / 24 sampled ones / Gerber drill, tenting, report, map sets |
| `tools/effect.py`, `effect_bp.py` | reference only: how many files an option (a stored parameter) changes. An option that changes nothing on a board proves nothing there |
| `tools/mutate.py`, `strip.py` | rewrite the `pcbplotparams` block / mark footprints DNP / drop top-level items of a copy |
| `tools/outcmp.py` | output path forms (`-o` missing, without slash, nested, with text variables) with cwd = board directory |
| `tools/wasm_export.mjs`, `wasm_cmp.py` | exports through `web/pkg-node` and compares them with the native files |
| `gen/gen_pads.py`, `gen_shapes.py`, `gen_text.py`, `gen_zones.py`, `gen_extreme.py`, `one_pad.py` | board generators (KiCad 8 syntax, canonicalised with `kicad-cli pcb upgrade --force`; zones filled with `pcb drc --refill-zones --save-board`) |
| `tools/*.txt` | the option sets and the board list used below |

Boards ("the ten boards", `tools/boards10.txt`, in fact eleven): `pic_programmer`, `interf_u`,
`issue5830`, `issue12609`, `basic_test`, `ecc83-pp`, `variants`, `issue6945`, `microwave`,
`RoyalBlue54L-NFC-Antenna`, `issue22267`. Drill additionally: `slot_machining`, `issue5990`,
`issue24951`, `drill_features`, `ipc4761_vias`.

## 3. Option coverage

"Before" is what a committed test compared against the reference before this review
(`kicad-fab/tests/golden/*/generate.py`, `kicad-session/tests/golden/gerbers_job`); "effect" is
the number of the eleven boards on which the option changes at least one reference file.

### 3.1 `pcb export gerbers`

| Option | Before | Effect | REV-FAB runs | Result |
|---|---|---|---|---|
| defaults | corpus (760 runs), crafted | - | 11 boards | identical |
| `--no-x2` | crafted (4 boards), corpus with `--use-drill-file-origin` | 11 | 11 boards, also with `--no-netlist` | identical |
| `--no-netlist` | none | 11 | 11 boards | identical |
| `--precision 5` | crafted (4 boards without items) | 11 | 11 boards + every crafted board | identical |
| `--precision 4`, `7` | none | error | 2 boards | same message, same exit code |
| `--no-protel-ext` | crafted | 11 | 11 boards | identical |
| `--use-drill-file-origin` | crafted, corpus | 5 (boards with an aux origin) | 11 boards | identical |
| `--subtract-soldermask` | none | 11 (2 files each) | 11 boards + crafted | identical |
| `--disable-aperture-macros` | stored option only, boards without pads | 8 | 11 boards + `rev_pads` (996 pads) | identical |
| `--ev`, `--erd` | `plot_text` crafted board | 11 | 11 boards, both together | identical |
| `--sp`, `--spn` | stored option on one crafted board | 11 (`--spn` alone: 0, as in C++) | 11 boards | identical |
| `--hdnp`, `--sdnp`, `--cdnp` | `plot_text` (`--hdnp`, `--cdnp`) | 0 on the corpus boards (no DNP footprint); 4 of 4 copies with 8 footprints marked DNP (`--sdnp` alone changes nothing in the reference) | 4 DNP boards (and one without) x 8 sets | identical |
| `--check-zones` | none | 9 | 11 boards + `rev_zones` | identical |
| `-l`, `--cl` | crafted (`--cl Edge.Cuts,F.Fab`), `gerbers_layer_args` | 11 | 3 common-layer sets x 11 boards; 17 edge cases x 2 boards (duplicates, canonical and user names, unknown names, empty list, lower case, blanks, layers the board does not have, order, `--cl` equal to `-l`) | identical, including messages and exit codes |
| `-D` | none | 1 (`issue22267`) | 2 sets x 2 boards | 05:16 build: the override was not applied to the board texts (F-02); 05:50 build: identical |
| `-o` forms | none | - | 8 forms (missing, no slash, nested, `./`, `${REVISION}`, unknown variable, `-D` variable, a file name) | identical |
| `--board-plot-params` | 4 crafted boards, 3 stored-option boards | 11 | section 3.2 | identical |
| `--variant` | none | 1 crafted board (footprint variants with `dnp` and a field override) | 6 sets | pending: `BOARD::SetCurrentVariant` (F-05); several variants without `${VARIANT}`: same error |
| `--ibt`, `--drawing-sheet` | none | 11 | 11 boards | not ported: drawing sheet (F-06) |
| combinations | none | 11 | `--subtract-soldermask --disable-aperture-macros --precision 5 --no-x2`; `--sdnp --cdnp --sp --spn --ev --no-netlist --no-protel-ext` | identical |

### 3.2 Stored plot parameters (`PCB_PLOT_PARAMS`, `--board-plot-params`)

Variants of the `pcbplotparams` block (`tools/stored_variants.txt`) on copies of 5 boards,
compared with `--board-plot-params`: 29 variants with every layer selected (145 runs) and 31
with the boards' own layer selection (155 runs); "effect" from
`effect_bp.py` on `pic_programmer` / `issue5830` (files changed of 21 / 23).

| Parameter | Values | Effect | Result |
|---|---|---|---|
| `layerselection`, `plot_on_all_layers_selection` | all layers; two partial masks with common layers | all | identical (the `-l` argument is ignored, as in C++) |
| `disableapertmacros` | yes | 4 / 4 | identical |
| `usegerberextensions` | yes, no | names | identical |
| `usegerberattributes`, `usegerberadvancedattributes` | no; no + no; yes + no | 20 / 22; 10 / 14 | identical |
| `creategerberjobfile` | no | 0 (the job file is written by the job) | identical |
| `gerberprecision` | 5 | 20 / 22 | identical |
| `useauxorigin` | yes | 20 / 22 | identical |
| `psnegative` | yes | 20 / 22 | identical |
| `psa4output` | yes | 12 / 16 | identical |
| `mirror` | yes | 0 (Gerber ignores it) | identical |
| `scaleselection` | 0, 2, 3, 4 | 0 (Gerber is 1:1) | identical |
| `drillshape` | 0, 2 | 0 (no drill marks on Gerber) | identical |
| `sketchpadsonfab`, `plotpadnumbers` | yes + yes; no + yes | 2 / 2; 0 | identical |
| `hidednponfab`, `sketchdnponfab`, `crossoutdnponfab` | the four combinations, 8 DNP footprints | 1 / 1, 0, 1 / 2, 0 | identical |
| `subtractmaskfromsilk` | yes | 2 / 0 | identical |
| `plotframeref` | yes | - | not ported (F-06) |
| `outputformat` | 0 (HPGL), 2 (PostScript), 3 (DXF), 4 (PDF), 5 (SVG) | the reference writes that format into the `.gbr` files | F-01 |
| `outputdirectory` | `"gerb/"` | 0 (`-o` wins) | identical |
| `dashed_line_dash_ratio`, `dashed_line_gap_ratio` | stored 12 / 3 only | - | not varied (the dashed shapes of `rev_shapes` use the defaults) |
| legacy tokens `excludeedgelayer`, `viasonmask`, `plotreference`, `plotvalue`, `plotfptext`, `padsonsilk`, `plotinvisibletext`, `linewidth`, `svguseinch` | two sets | 19 / 21 (`excludeedgelayer no`) | identical |
| combination | negative + macros off + precision 5 + X1 + subtract mask + sketch pads + aux origin; negative + A4 + scale 3 + drill marks 2 | all | identical |

PDF / DXF / PNG / SVG-only parameters (`pdf_*`, `dxf*`, `svgprecision`, `pngdpi`, `mode`,
`plot_black_and_white`) do not reach the Gerber path.

### 3.3 `pcb export drill`

| Option | Before | REV-FAB runs | Result |
|---|---|---|---|
| Excellon zeros format x units x origin x mirror x minimal header x separate TH x oval format | 13 single-option sets on the corpus (`excellon_corpus`) | all 256 combinations on `slot_machining`, `drill_features`, `issue24951` (768 runs); 24 sampled combinations on 16 more boards (384 runs) | identical |
| `--format gerber`, `--gerber-precision 5 / 6`, `--drill-origin` | 3 sets (`gerber_drill`) | precision x origin, with and without `--generate-tenting`, on 16 boards | identical |
| `--generate-tenting` (Excellon and Gerber) | Gerber only | 16 boards | identical |
| `--generate-report`, with `--excellon-separate-th -u in`, with Gerber | 2 sets | 16 boards | identical |
| `--report-path` | none | existing directory: identical; missing directory: F-04 | F-04 |
| `--generate-map`, `--map-format` | none | 6 sets | pending FAB-12 (F-07) |
| invalid values (zeros format, units, precision 7) | none | 16 boards | same message and exit code |
| `-o` forms | none | 5 forms | identical |
| unknown argument (`-D`) | none | 1 | exit code 1 in both; the usage text differs (F-08) |

## 4. Item coverage

| Item kind / state | In a compared corpus file | Crafted for REV-FAB | Result |
|---|---|---|---|
| Dashed / dotted strokes on copper, silk, mask, Edge.Cuts, user layers (line, rect, circle, arc, polygon, bezier; widths 0.1, 0.5, 0) | silk only | `rev_shapes` (1008 shapes) | identical |
| Fills solid / hatch / reverse hatch / cross hatch on copper and both silk layers, with and without outline | one hatch on a crafted board | `rev_shapes` | identical |
| Arcs of 0.02 .. 359.99 degrees, both directions, radius 0.05 .. 40 mm, as graphics on copper / silk / Edge.Cuts and as tracks | common angles | `rev_shapes` (17 sweeps x 4 radii) | identical |
| Degenerate shapes: zero-length lines, zero-radius circles, empty and inverted rectangles, 2- and collinear 3-point polygons, a bezier in one point, 2 nm dashed line | no | `rev_shapes` | identical |
| Copper shapes with a solder mask margin (positive, negative), tracks with mask, zero-width and zero-length tracks | few | `rev_shapes` | identical |
| Text: knockout, multi-line, mirrored, rotated, bold / italic, every justification, overbar / sub / superscript, tabs, non-ASCII, unresolved variables, empty and blank text, 0.02 mm thickness; on copper, silk, mask, Edge.Cuts | `plot_text` crafted board, corpus | `rev_text` (396 texts) | identical |
| Text boxes with and without border, dashed border, rotated 30 / 90 / 180 / 270 / -60, margins | corpus (few) | `rev_text` (54) | identical |
| Tables: external border on / off, header separator, row / column separators, dashed strokes, column and row spans, mirrored on the back | `api_kitchen_sink` (stopped in FAB-13) | `rev_text` (12 tables), `api_kitchen_sink` without its drill chart | identical |
| Dimensions: aligned, orthogonal, leader (three frames), center, radial; units, formats, precision, suppressed zeros, override, prefix / suffix, arrow direction, text position modes, keep aligned; back side | 5 corpus boards | `rev_text` (120) | identical |
| Barcodes, images, groups | `api_kitchen_sink` | stripped copy | identical |
| Pads: rect, oval, circle, round rect (ratio 0, 0.25, 0.5), trapezoid (both deltas), chamfered (8 corner / ratio combinations), custom (polygon, line, arc, circle, rect primitives; both anchors; outline and convex hull) at 0 / 20 / 90 / 133 degrees in footprints at 0, 0.7, 33, 90, 180, 179.9, 210, 270, -45.5 degrees, front and back | orthogonal footprints mostly | `rev_pads` (996 pads, 12 footprints) | identical |
| Pad offsets, oval and offset drills, NPTH with and without copper, NPTH slots, remove-unused-layers, pads on the opposite side, paste-only and mask-only apertures, `*.Paste` on through-hole pads | partly | `rev_pads` | identical |
| Fabrication properties (BGA, fiducials, test point, heat sink, castellated), pin function with reserved characters, local mask / paste margins and ratios on every shape | `pads.kicad_pcb` crafted board | `rev_pads` | identical |
| Aperture macros of every kind and rotation; the same with `--disable-aperture-macros` | corpus | `rev_pads` | identical |
| Custom pads with thermal reliefs, solid / hatched / multi-layer / prioritised zones, islands, legacy outline thickness, keepouts, non-copper zones (silk, mask, user) | corpus (hatch, teardrop, thermal boards) | `rev_zones` (9 zones, 13 filled polygons, filled by the reference; also `--check-zones`) | identical |
| Vias: through, blind, buried, micro; remove unused layers; free; locked; three nets; on copper and mask layers | corpus, `ipc4761_vias` | `rev_zones` (75 vias), `ipc4761_vias` through the gerbers job | identical |
| Mask minimum width (FAB-10, landed during the review) | 31 corpus boards | the four crafted boards with `solder_mask_min_width 0.15`, 3 option sets; `issue24312`, `severities` x 24 sets | identical |
| Net, reference and pad names with `,` `*` `%` `\`, Greek, emoji (non-BMP), tab | one crafted file name | `rev_names` (gerbers, Excellon, Gerber drill, report) | identical |
| Coordinates from -500 mm to 1065 mm, sizes with a nm remainder, 1 nm and 100 mm vias, slots at odd angles and with offsets | no | `rev_extreme` (gerbers 5 sets, drill 24 + 16 sets) | identical |
| Coordinates beyond 1073.7 mm | no | `rev_far` | dev build panics in kicad-geom (X-1); release build and wasm identical |
| Footprints with a scale (`(transform (scale ..))`): uniform 0.3 .. 3, non-uniform | no | `rev_scaled` (the 12 footprints of `rev_pads`) | 22 of 23 files identical; `F.Mask` differs for two chamfered pads (F-03) |
| Teardrops, edge plating / castellation in the stack-up | corpus (`teardrop_*`), `jobfile_stackup` | not crafted | covered by the existing comparisons at the defaults only |
| Drill chart symbols, drill marks, drawing sheet | - | - | pending FAB-13, FAB-11, PLOT-07 / 08 |

## 5. Statement-level reading

Read side by side with the C++ (`common/plotters/GERBER_plotter.cpp`, `plotter.cpp`,
`common/gbr_metadata.cpp`, `pcbnew/exporters/gendrill_excellon_writer.cpp`,
`pcbnew/plot_board_layers.cpp` L446-681, `pcbnew/pcbnew_jobs_handler.cpp` L1750-1930):

* Aperture list: `get_or_create_aperture` / `_corners` (search order, the six compared members,
  `polyCompare` for corner lists, D-code = last + 1, free-polygon macro registered before the
  search), `select_aperture` (exact comparison with the current aperture, then the attribute
  pair), `select_aperture_with_attributes` (aperture, net attribute, flash): equivalent. The
  order of first use is the order of the plot calls; no container reorders it.
* `write_aperture_list`: one `fscale` expression, `{:#f}` as `{:.6}` (Rust prints `-0.000000`
  like fmt for a negative value rounding to zero), integer negation of the Y corner before the
  multiplication, minimum 10 nm half size of round-rect macros, attribute printed when it
  differs from the previous tool and cleared after every tool with an attribute: equivalent.
* Object attributes: `format_net_attribute` (pad / net / component strings, `find` of the
  previous dictionary, clear-one versus clear-all with `m_TryKeepPreviousAttributes`, the
  "pad attribute exists" refinement for `TO.C`), `clear_net_attribute` only when the dictionary
  is not empty, `clear_all_attributes` unconditional, the X1 structured comments: equivalent.
* Coordinates: `user_to_device_coordinates` (wrapping int subtraction with the reason recorded,
  same expression order), `emit_dcode` (`KiROUND` of both, `D%02d`), format 4.5 through
  `m_iuPerDeviceUnit` only: equivalent.
* Arcs: `arc` (the 100 device unit / 0.1 degree threshold to a polyline), both `plotArc`
  overloads (truncating `VECTOR2D -> VECTOR2I` for the direction test and the centre, `I` / `J`
  from two separate device conversions, `G75`, `G02` / `G03` by angle order), `circle` as two
  half arcs with the integer radius: equivalent.
* Regions and polygons: `plot_poly` (vector overload: region, then outline when a width is
  given or nothing is filled; closing rules), `plot_poly_chain` (arcs of the chain, `else if`
  for the outline), `plot_gerber_region` (`TA.AperFunction` printed before and deleted after),
  `plot_poly_as_region`: equivalent.
* Excellon numbers: `write_coordinates` in the four zero formats (sign-aware `%0*d`, trailing
  zero removal keeping the first character, decimal format with one kept zero), hole list
  sorted with the libstdc++-exact `std_sort`: equivalent; confirmed by the full factorial of
  section 3.3 on boards with slots, NPTH, micro vias and an aux origin.
* `plot_pad_layer` (kicad-fab): line by line the C++ lambda, except that it never writes the
  modified pad back: F-03.

## 6. Numeric rules and WebAssembly

* No float-to-int `as` cast on a value that can be out of range in either crate (`f64_to_i64_x86`
  in the SVG colour conversion, explicit clamps elsewhere); no `mul_add`; trigonometry only
  through `EdaAngle` / `cmath`; no `HashMap` in either crate, `BTreeSet` / `BTreeMap` where the
  C++ has ordered containers.
* `plotter_gerber.rs` `set_viewport` / `set_gerber_coordinates_format` use `10.0_f64.powi(n)`
  where the C++ calls `pow( 10.0, n )`. Both are exact for the two formats in use (n = 5, 6);
  noted only because it bypasses `kicad_geom::cmath`.
* wasm (`web/pkg-node` of 05:43 UTC) against the native 05:50 build, `exportGerbers({})` and
  `exportDrill({})`: `pic_programmer`, `issue5830`, `rev_pads`, `rev_shapes`, `rev_text`,
  `rev_zones`, `rev_scaled`, `rev_far`: 178 Gerber / job files and 11 drill files, all
  identical after the date / version lines. No one-ulp libm difference became visible, not
  even on the 996 rotated pads.

## 7. Findings

Line numbers are those of 2026-11-10 06:00 UTC; the function name is the stable reference.

| Id | Sev. | Where (Rust) | C++ reference | What differs, for which input | Status |
|---|---|---|---|---|---|
| F-03 | minor | `kicad-fab/src/plot_board_layers.rs` `plot_pad_layer` (L574-864: `let mut work = pad.detached_copy()` L629, "Restore ..: `work` is a copy" L863) | `plot_board_layers.cpp:446-681` (`pad->SetSize( aLayer, padPlotsSize )` L492 .. L597, restore `pad->SetSize( aLayer, padSize ); pad->SetDelta( aLayer, padDelta )` L677-678); `pad.cpp:264-273` `PAD::SetSize`, `145-180` `derivePadBoardSize` / `derivePadLibSize` | The C++ plots the board's own pad and restores it with the board-frame setters. For a pad in a scaled footprint these convert to the library frame with rounding, so after the first layer the pad's stored size is `round( round( L * s ) / s )`, which can be 1 nm off, and every layer plotted later in the same run starts from that value. The port plots a copy and always starts from the file's value. Smallest case (`gen/one_pad.py one.kicad_pcb 90 2 0.5 15 cham`: footprint at 90 degrees with `(scale 2 0.5)`, one chamfered pad 2.2 x 1.4 mm at 15 degrees, mask clearance 0.05): `-l F.Cu,F.Mask` (or `F.Paste,F.Mask`, or the default layer set) gives an `F.Mask` aperture macro whose corners are 1 to 3 nm smaller in the reference (`2.799998` against `2.800000`); `-l F.Mask` alone is identical. Seen on 2 of 996 pads of `rev_scaled`; needs a non-uniform (or below 1) scale, a pad rotated in its footprint and a path that reads the stored size at another orientation (the inflated copy of a chamfered pad). Unscaled footprints cannot be affected (the conversion is the identity) | open: `docs/packages/FIX-FAB-1.md` |
| F-04 | minor | host side of `kicad-rs pcb export drill` (file writing of the report) | `pcbnew_jobs_handler.cpp:2425-2430` (`GenDrillReportFile` fails, `ERR_INVALID_OUTPUT_CONFLICT`) | `--generate-report --report-path DIR/file` with a directory that does not exist: the reference logs `can't open file '...' (error 2: No such file or directory)` on stderr, writes no report and exits with 4; kicad-rs creates the directory, writes the report and exits with 0. Same stdout | open: `docs/packages/FIX-FAB-2.md` |
| F-01 | minor | `kicad-fab/src/pcbplot.rs` `start_plot_board` (format switch), `jobs_handler.rs` gerbers loop | `pcbnew_jobs_handler.cpp:1859-1915` (`plotOpts = boardPlotOptions`, `StartPlotBoard` with the stored format) | `--board-plot-params` on a board whose stored `outputformat` is not Gerber (corpus: `issue12609`, DXF): the reference writes DXF / PostScript / PDF / SVG content into the `.gbr` files and a job file, exit 0; for HPGL it reports `Failed to plot to ...` per layer, still writes the job file, exit 4. kicad-rs stops at the first layer with `plot format .. is not part of the port`, exit 2, no file (SVG: stops in FAB-11 today) | accepted: formats out of scope, recorded in PORT_STATUS FAB-03 / FAB-07; the exit code and the missing job file of the HPGL case go with FIX-FAB-2 |
| F-02 | note | `kicad-session/src/jobs_handler.rs` `resolve_job_output_path` | `pcbnew_jobs_handler.cpp:3697-3713` (`ApplyTextVars`, `SynchronizeProperties`) | 05:16 build: `-D KEY=VALUE` changed the output path only, board texts kept the project's value (`issue22267 -D DWG_NUMBER=XYZ-123`: User.Drawings, F.Silkscreen). Fixed in the tree before the 05:50 build; identical now | fixed by another package; no committed test compares a `-D` run: add `tools/d_sets.txt` to the end-to-end scoreboard (FIX-FAB-2) |
| F-05 | note | `kicad-pcb` `Board::set_current_variant` (pending stub) | `pcbnew_jobs_handler.cpp:1763` | `--variant NAME` stops with "not yet ported". A board for it: `pic_programmer` with a `(variants ..)` block and `(variant (name "V1") (dnp yes) (field ..))` in every fourth footprint; in the reference `--variant V1` changes 2 files, with `--cdnp --sdnp --sp` 3 | pending (model); rerun the six sets of `tools/var_sets.txt` when it lands |
| F-06 | note | `kicad-fab` `PlotDrawingSheet` (not ported) | `pcbplot.cpp` `StartPlotBoard`, `PlotDrawingSheet` | `--ibt`, stored `plotframeref yes` (corpus: `issue22267`) stop with "not ported yet" | pending PLOT-07 / 08 (in progress in kicad-plot during this review) |
| F-07 | note | `kicad-fab/src/exporters/gendrill_map.rs` | `gendrill_writer_base.cpp` `genDrillMapFile` | `--generate-map` in any format: pending FAB-12. Side effect while the stub exists: stdout says `Created file '...drl'` but no drill file is written, because the host drops the files of a run that ends in a not-ported error | pending FAB-12 |
| F-08 | note | `kicad-cli` (clap) | argparse usage text | After `Unknown argument: X` the usage text has clap's layout, not argparse's (`pcb export drill -D a=b`); exit code 1 in both | accepted (cosmetic) unless the CLI package wants byte-equal help |
| X-1 | minor | `kicad-geom/src/geometry/shape_collisions.rs:81` `collide_circle_circle` (`a_a.get_center() + a_b.get_center()`) | `shape_collisions.cpp:57` (`( aA.GetCenter() + aB.GetCenter() ) / 2`, int addition, wraps in the reference build) | The C++ computes the location only when the caller asks for it (`if( aLocation )`); the port's `set( &mut out, value )` helper evaluates `value` first. Two round shapes that touch at a coordinate above 2^30 nm (1073.7 mm, reachable on an A0 sheet) therefore overflow an addition the reference never executes in that call: the connectivity build of the board loader panics with "attempt to add with overflow" in dev / test builds and nothing can be exported. The release build and the wasm module (no overflow checks) are byte-identical to the reference on that board (`rev_far`) | open: `docs/packages/FIX-FAB-3.md` (kicad-geom) |
| X-2 | note | `kicad-pcb/src/pcb_shape.rs` `PcbShape::normalize_env` | `PCB_SHAPE::Normalize` (`end - start`) | A rectangle wider than 2^31 nm panics in the parser in dev builds ("attempt to subtract with overflow"); the reference wraps. Not a valid board | with FIX-FAB-3 |

Checked and found equivalent (no finding): everything in sections 3 to 5 not named above.

Observed outside the scope, for the owners: `pcb-dump` against `pcb-dump-rs` on `rev_pads`
(160 981 values match) differs in `own_clearance` of the NPTH pads on every copper layer (reference
250000, Rust 0; 1620 lines) and in `ds.biggest_clearance` (1000000 against 500000). Not read by
the plot or drill code; reported to the model / DRC packages.

## 8. FIX packages written

* `docs/packages/FIX-FAB-1.md`: F-03, the lossy pad restore of `plotPadLayer` under a footprint
  scale (kicad-fab, after FAB-12).
* `docs/packages/FIX-FAB-2.md`: F-04, the HPGL / exit code part of F-01, regression coverage
  for `-D`, `--variant`, the option matrix as a committed scoreboard (kicad-cli / kicad-session).
* `docs/packages/FIX-FAB-3.md`: X-1, X-2 (kicad-geom, kicad-pcb).

No source file of kicad-plot was changed: the review found nothing to fix there. (While the
review ran, another package added `kicad-plot/src/drawing_sheet/` and
`tests/test_drawing_sheet_oracle.rs`; their clippy warnings and the failing oracle test belong
to that work in progress.)

## 9. Not verified

* Everything behind a pending stub: SVG plots and drill marks (FAB-11), drill maps (FAB-12),
  drill chart symbols (FAB-13), drawing sheet, variants. The option sets and boards to rerun are
  named in the findings.
* `dashed_line_dash_ratio` / `dashed_line_gap_ratio` other than the stored defaults.
* Outline-font text (stand-in glyphs, FONT-RC-1) was not varied.
* Teardrops and stack-up options beyond the corpus boards at the CLI defaults.
* The large demo boards (machine limits); the option matrix ran on boards below 900 KB.
