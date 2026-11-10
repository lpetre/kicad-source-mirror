# FIX-FAB-1: the pad that `plotPadLayer` leaves behind (footprint scale)

Crate: kicad-fab (`src/plot_board_layers.rs` `plot_pad_layer`, and whatever carries state from
one plotted layer to the next: `src/pcbplot.rs` / `src/jobs_handler.rs` / `BrditemsPlotter`).
Start after FAB-12 (one agent at a time in kicad-fab).

Origin: finding F-03 of `rust/docs/design/review-fab.md` (package REV-FAB).

## Problem

`PlotStandardLayer` (`pcbnew/plot_board_layers.cpp` L446-681, the `plotPadLayer` lambda) plots
the board's own pad: it calls `pad->SetSize( aLayer, padPlotsSize )` (and shape / radius
setters), plots, and restores with

```cpp
pad->SetSize( aLayer, padSize );
pad->SetDelta( aLayer, padDelta );
pad->SetShape( aLayer, padShape );
pad->SetRoundRectRadiusRatio( aLayer, padCornerRadiusRatio );
```

Shape, delta and radius ratio are restored exactly. `PAD::SetSize` (`pad.cpp` L264-273) is a
board-frame setter: for a pad of a footprint it stores
`derivePadLibSize( aSize, shape, fp->GetTransform(), GetFPRelativeOrientation() )`, i.e.
`KiROUND( board / localScale )`, and `GetSize` returns `KiROUND( lib * localScale )` with
`localSx = sx * c * c + sy * s * s`, `localSy = sx * s * s + sy * c * c` (L128-180). With a
footprint scale other than (1, 1) the round trip `lib -> board -> lib` is not exact: after the
first plotted layer that contains the pad, its library size is
`L' = KiROUND( KiROUND( L * s ) / s )`, which can differ from `L` by 1 nm, and the board keeps
`L'` for every layer plotted afterwards in the same run (the gerbers job plots all layers of
one loaded board, `pcbnew_jobs_handler.cpp` L1835-1912).

The port plots a detached copy (`let mut work = pad.detached_copy()`, comment "Restore the pad
parameters modified by the plot code: `work` is a copy") and therefore starts every layer from
the size in the file. Without a footprint scale both are the same (`scaleLength( x, 1.0 ) == x`).

Where it becomes visible: any later read of the pad's size whose result changes with the 1 nm.
Found: the chamfered-rectangle branch with a positive mask clearance (L600-640) reads the
library size at another orientation (`dummy.SetOrientation( ANGLE_0 )`), which turns the 1 nm
into 2 nm of the inflated outline. In general also `KiROUND( L' * s ) != KiROUND( L * s )` for
some sizes, which would change a plain aperture on the second layer.

## Reproduce

```
python3 rust/docs/design/review-fab/gen/one_pad.py /tmp/x/one.kicad_pcb 90 2 0.5 15 cham
python3 rust/docs/design/review-fab/tools/fabcmp.py --show 20 gerbers t /tmp/x/one.kicad_pcb -- -l F.Cu,F.Mask
```

(one footprint at 90 degrees, `(scale 2 0.5)`, one chamfered pad 2.2 x 1.4 mm at 15 degrees
relative, board mask clearance 0.05 mm.) Reference `F.Mask`:
`4,1,15,0.292678,2.870708,0.300000,2.799998,0.300000,-2.359998,...`; port: `...2.870710,
0.300000,2.800000,0.300000,-2.360000,...`. With `-l F.Mask` alone (nothing plotted before) the
two are identical; with `-l F.Paste,F.Mask` they differ as well. `one_pad.py` variants that
differ today: `90 2 0.5 15 cham`, `33 2 0.5 15 cham`, `90 2 0.5 33 cham`; twelve others
(uniform scale, relative angle 0, other shapes) are identical and must stay so. Board with all
pad kinds: `gen_pads.py` + the scale edit described in `review-fab.md` section 4
(`rev_scaled`): 2 of 996 pads differ.

## Work

1. Give the pad plotting a state that outlives one layer and one output file of a job: the
   pads as the C++ leaves them. Either
   * (a) let the job plot from a mutable board and write the restored size back exactly as the
     C++ does (`set_size( env, layer, pad_size )` on the board's pad after each
     `plot_pad_layer`), or
   * (b) keep an overlay `PadId -> Pad` (the restored copy) in the plot context owned by the
     job loop (`job_export_gerbers`, the SVG job, the drill map plots that go through
     `PlotStandardLayer`), consulted at the top of `plot_pad_layer` instead of the board's pad.

   (b) keeps the board immutable (board-model.md 13) and is enough, because nothing else reads
   the pad between two layers of a plot job. The overlay must be shared by all layers of one
   file (`--cl` common layers) and by all files of one job, in plot order.
2. The restore sequence must be the C++ one (L676-680), applied to the pad the layer started
   from: `set_size`, `set_delta`, `set_shape`, `set_round_rect_radius_ratio`; only the first is
   lossy, but keep all four so that a later change of the setters is covered.
3. Mind the two exits of the lambda. The zero-size check (L482-486) returns before anything is
   modified: no restore, the pad stays as it was. The NPTH skip (L494-500) is a `break` after
   `pad->SetSize( aLayer, padPlotsSize )`: the restore runs. In the port the skip is a
   `return`, which is equivalent only while nothing is written back; it must reach the restore.
4. Record the mechanism in PORT_STATUS (FAB-04 deviation list: remove "which is the same pad
   as far as `PlotPad` can tell").

## Verification

* `tools/fabcmp.py` on the three failing `one_pad.py` configurations with `-l F.Cu,F.Mask`,
  `-l F.Paste,F.Mask`, `-l F.Mask` and the default layer set: identical.
* `rev_scaled` (section 4 of the review) x `tools/gs3.txt`: 5 of 5 sets identical.
* A committed crafted test: the `one_pad` board (a few KB) and a second one with a uniform
  scale below 1 and sizes that are not multiples of the scale's denominator, golden from the
  reference, in `tests/plot_items_corpus.rs` style.
* The existing kicad-fab and kicad-session corpus tests unchanged (no corpus board has a
  scaled footprint).
