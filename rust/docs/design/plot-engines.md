# Plot engines: `PLOTTER` as a Rust trait

Crate `kicad-plot`, module `plotter` (port of `include/plotters/plotter.h`,
`common/plotters/plotter.cpp`).

## Split

* `PlotterState` (plain struct, `pub` fields) = the protected members of `PLOTTER`
  (`m_plotScale`, `m_IUsPerDecimil`, `m_iuPerDeviceUnit`, `m_plotOffset`, mirror flags,
  `m_paperSize`, pen state, header strings, `m_pageInfo`, `m_renderSettings`, ...) plus the
  output sink `output: String` that replaces `m_outputFile`. `PlotterState::new()` is the C++
  constructor.
* `trait Plotter` = the class interface. Every engine (`GerberPlotter`, `SvgPlotter`) holds a
  `PlotterState` and implements `state()` / `state_mut()`.
  * C++ **pure virtuals** are required methods: `plotter_type`, `start_plot`, `end_plot`,
    `set_current_line_width`, `set_color`, `set_dash`, `set_viewport`, `rect`, `circle`,
    `pen_to`, `plot_poly` (corner list), `flash_pad_circle/oval/rect/round_rect/custom/trapez`,
    `flash_regular_polygon`.
  * C++ **virtuals with a base implementation** are provided methods whose body is a call to
    a free function `plotter::<name>_base(self, ...)` (`arc_3pt_base`, `arc_base`,
    `poly_arc_base`, `bezier_curve_base`, `plot_poly_chain_base`, `plot_image_base`,
    `thick_segment_base`, `thick_arc_base`, `thick_rect_base`, `thick_circle_base`,
    `filled_circle_base`, `thick_oval_base`, `thick_poly_base`,
    `user_to_device_coordinates_base`, `user_to_device_size_base`). An engine overrides the
    trait method one-to-one with its C++ override; where the C++ override calls `PLOTTER::X`
    explicitly, the Rust override calls `plotter::x_base(self, ...)`. Base code calling a
    virtual (`ThickSegment` -> `Circle`, `polyArc` -> `SetCurrentLineWidth`/`PenTo`) calls the
    trait method, so dynamic dispatch is the same as in C++.
  * C++ **non-virtual** members (`MoveTo`, `LineTo`, `FinishTo`, `PenFinish`, `Marker`, the
    `marker*` helpers, `GetDash*LenIU`, `GetPlotterArcLowDef`, getters/setters) are provided
    methods documented "non-virtual"; engines must not override them.
  * No-op hooks (`set_layer_polarity`, `set_text_mode`, `set_gerber_coordinates_format`,
    `set_svg_coordinates_format`, `set_plot_bbox`, `start_block`, `end_block`) are provided
    no-ops, overridden by the engine that needs them.
* The trait is object safe: board plot code can hold `&mut dyn Plotter`. The `*_base`
  functions are generic over `P: Plotter + ?Sized`.

## Parameter mapping

* `void* aData` -> `PlotData<'_> = Option<&GbrMetadata>` (DXF's `PLOT_PARAMS*` is out of scope).
* C++ overloads get suffixes: `Arc(start, mid, end)` -> `arc_3pt`, `Arc(centre, angles)` ->
  `arc`; `PlotPoly(SHAPE_LINE_CHAIN)` -> `plot_poly_chain`; `userToDeviceSize(double)` ->
  `user_to_device_size_f`.
* `PlotImage( wxImage, ... )` -> `plot_image( width_px, height_px, ... )` (only the size is
  used by the base placeholder; bitmap embedding is out of scope).
* `RENDER_SETTINGS*` -> `Option<RenderSettings>` owned copy (`render_settings.rs`, the subset
  used by plotters: pen widths, dash/gap ratios). A null pointer dereference in C++ becomes
  "use the defaults".
* `OpenFile` only records the file name and clears the buffer; the caller writes
  `state().output` (wasm: no file system). Engines that build the file in two parts
  (Gerber's work file + aperture list splice) keep their own buffers and put the final text
  into `output` in `end_plot`.

## Downcasts and later packages

The board plotter down-casts to `GERBER_PLOTTER` (`PlotGerberRegion`, `UseX2format`, ...).
PLOT-03 adds a provided `fn as_gerber_mut(&mut self) -> Option<&mut GerberPlotter> { None }`
overridden by `GerberPlotter`. PLOT-05 adds `text` / `plot_text` as provided methods (base
implementation through `kicad-font`'s callback GAL). Shared enums (`PlotFormat`, `FillT`,
`PlotTextMode`, `DrillMarks`, DXF enums) live in `kicad_core::plot_enums`; dash styles are
`kicad_core::stroke_params::LineStyle`.
