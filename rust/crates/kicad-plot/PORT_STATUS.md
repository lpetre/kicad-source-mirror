# kicad-plot port status

Design: `rust/docs/design/plot-engines.md`. Analysis: `rust/docs/analysis/plot-fab.md`.

## Mapping

| C++ | Rust | State |
|---|---|---|
| `include/plotters/plotter.h`, `common/plotters/plotter.cpp` L1-546 (`PLOTTER`) | `plotter.rs` (`Plotter` trait, `PlotterState`, `*_base` fns) | ported (except below) |
| `PLOTTER::BezierCurve` | `plotter::bezier_curve_base` (uses `kicad_geom::bezier_curves::BezierPoly`) | ported |
| `PLOTTER::PlotImage` | `Plotter::plot_image` (pixel size instead of `wxImage`) | ported (placeholder rectangle) |
| `PLOTTER::ThickArc( const EDA_SHAPE& )` | - | skipped: no caller in board plotting; would make kicad-plot depend on the board model |
| `PLOTTER::Text`, `PlotText` (`plotter.cpp` L548-667) | `Plotter::text` / `plot_text` -> `plotter_text::text_base` / `plot_text_base` | ported (PLOT-05) except `@{...}` evaluation and outline fonts (below) |
| `CALLBACK_GAL` + the two lambdas of `Text` / `PlotText` (`callback_gal.cpp`) | `plotter_text::PlotterGal` (a `kicad_font::TextGal` holding the plotter) | ported: stroke points truncated (`to_vector2i`), `SetCurrentLineWidth( penWidth )` without metadata before every segment in `Text`, outline glyphs copied, fractured when they have holes and plotted as filled `PlotPoly( chain )` with `aData` |
| `common/gr_text.cpp` (`GetPenSizeForBold`, `GRTextWidth`) | `kicad_font::gr_text` (used as is) | (kicad-font) |
| `RENDER_SETTINGS::GetDefaultFont/SetDefaultFont` | `RenderSettings::default_font` | ported |
| `HyperlinkBox/HyperlinkMenu/Bookmark`, `m_project`, `m_layersToExport`, `m_plottedFootprints`, `SetLayersToExport`, `MarkFootprintPlotted` | - | skipped: PDF/DXF only |
| `PLOT_PARAMS` (DXF hook) | - | skipped: DXF |
| `PLOT_FORMAT`, `PLOT_TEXT_MODE`, `DXF_OUTLINE_MODE`, `DXF_UNITS`, `DXF_LAYER_OUTPUT_MODE`, `DRILL_MARKS`, `FILL_T` | `kicad_core::plot_enums` (re-exported) | ported |
| `KIGFX::RENDER_SETTINGS` (pen widths, `GetDashLength/GetDotLength/GetGapLength`) | `render_settings.rs` | partial: plotter subset only |
| `include/plotters/drill_markers.h`, `common/plotters/drill_markers.cpp` | `drill_markers.rs` | ported |
| `GetDefaultPlotExtension` (`common_plot_functions.cpp` L34-46) | `common_plot_functions.rs` | ported |
| `PlotDrawingSheet` | - | PLOT-08 (tier 3) |
| `include/gbr_metadata.h`, `common/gbr_metadata.cpp` | `gbr_metadata.rs` | ported (except below) |
| `GbrMakeCreationDateAttributeString` | `gbr_make_creation_date_attribute_string( format, &GbrCreationDate )` | ported; date/time zone supplied by the caller |
| `FormatStringFromGerber`, `GBR_CMP_PNP_METADATA` | - | skipped: Gerber reader / placement files |
| `GBR_APERTURE_METADATA::GetAttributeName` | - | skipped: declared but never defined in C++ |
| `include/gbr_netlist_metadata.h` (`GBR_DATA_FIELD`, `GBR_NETLIST_METADATA`, `GBR_NETINFO_ALL`) | `gbr_netlist_metadata.rs` | ported |
| `include/plotters/gbr_plotter_apertures.h` (`APERTURE`, `APER_MACRO_FREEPOLY(_LIST)`), `GERBER_plotter.cpp` L74-91, L1858-1919 | `gbr_plotter_apertures.rs` | ported |
| `include/plotters/plotter_gerber.h`, `common/plotters/GERBER_plotter.cpp` L1-1805, L1849-1856 (`GERBER_PLOTTER`) | `plotter_gerber.rs` (`GerberPlotter`, `Plotter::as_gerber_mut`) | ported (PLOT-03, PLOT-04) except below |
| `GERBER_PLOTTER::FlashPadChamferRoundRect` | `GerberPlotter::flash_pad_chamfer_round_rect( ..., &RoundChamferedRectToPolygonFn )` | ported; `TransformRoundChamferedRectToPolygon` is a parameter (not in `kicad-geom` yet) |
| `GERBER_PLOTTER::Text`, `PlotText` (`GERBER_plotter.cpp` L1807-1846) | `impl Plotter for GerberPlotter` `text` / `plot_text` | ported |
| `include/plotters/gbr_plotter_aperture_macros.h` | `gbr_plotter_aperture_macros.rs` | ported (byte-identical literals) |
| `include/plotters/plotters_pslike.h` L41-143, `PS_plotter.cpp` L45-226 (`PSLIKE_PLOTTER`: `SetTextMode`, `SetColor`, `FlashPadOval/Circle/Rect/RoundRect/Custom/Trapez`, `FlashRegularPolygon`) | `plotters_pslike.rs` (trait `PsLikePlotter` = `emitSetRGBColor`, `pslike_*` free functions) | ported; `FlashPadRoundRect` takes the polygon (below) |
| `PSLIKE_PLOTTER::SetScaleAdjust`, `computeTextParameters`, `encodeStringForPlotter`, `returnPostscriptTextWidth`, Helvetica tables | - | skipped: PostScript / PDF only, never reached from SVG |
| `plotters_pslike.h` L712-867, `common/plotters/SVG_plotter.cpp` (`SVG_PLOTTER`, `XmlEsc`) | `plotter_svg.rs` (`SvgPlotter`, `xml_esc`) | ported (PLOT-06) except below |
| `SVG_PLOTTER::PlotImage` (PNG + base64 `<image>`) | `SvgPlotter::plot_image` | partial: the empty-size branch (base placeholder) only; a non-empty bitmap also plots the placeholder |

## Tests

* `tests/test_plot_base_oracle.rs`: differential oracle (`tests/oracle/plot_base_oracle.cpp`,
  built by `tests/oracle/build_plot_base_oracle.sh` against `common/libcommon.a` of the
  reference build; golden `tests/golden/plot_base.txt`, 29 KB). 12 sections, about 21 000
  cases, all byte-identical: `userToDeviceCoordinates/Size` and `GetDot/Dash/GapLenIU` for
  random viewports (mirror, Y reversal, paper size, Gerber/SVG device factors); `polyArc`
  through `Arc(centre, angles)` and `Arc(start, mid, end)`; `ThickSegment` (incl. sentinels),
  `ThickRect/Circle/Oval/Arc`, `FilledCircle`, `PlotImage`; `PlotPoly(SHAPE_LINE_CHAIN)` and
  `ThickPoly`; `BezierCurve`; `Marker` for ids 0..61 and the `marker*` helpers; curated
  shapes; `GbrMakeProjectGUIDfromString` (3006 strings); `ConvertNotAllowedCharsInGerber`
  (all 4 flag combinations), `FormatStringToGerber`, `GBR_DATA_FIELD::GetGerberString`
  (5000 random strings with NUL, controls, Latin-1, BMP, BOM, U+FFFF, astral code points,
  quoted strings); `FormatAttribute` for all 33 attribute values x X1/X2 x custom texts;
  `FormatNetAttribute` over 300 sequences of 12 random objects (state carried between calls,
  keep/clear logic, null metadata).
* `tests/test_gerber_oracle.rs`: differential oracle of `GERBER_PLOTTER`
  (`tests/oracle/gerber_plotter_oracle.cpp`, built by
  `tests/oracle/build_gerber_plotter_oracle.sh [outdir] regen`; golden
  `tests/golden/gerber_plotter.txt`, 80 KB). The Rust test generates 2400 seeded plot scripts
  (every primitive: pen moves, `ThickSegment` incl. zero length and sentinels, `Arc` by angles
  and by 3 points incl. the short-arc fallback, `ThickArc/Rect/Circle/Oval`, `FilledCircle`,
  `Rect`, `Circle`, `PlotPoly` of corner lists and of line chains with arcs, `PlotGerberRegion`
  (both overloads), `PlotPolyAsRegion`, `ThickPoly`, `Marker`; every pad flash: circle, oval,
  rect, round rect, trapezoid, custom (shared FreePoly macros), chamfered (Outline4P..8P and
  FreePoly), regular polygons 3..12, cardinal / non-cardinal / negative / > 360 degree
  orientations; aperture macros on/off, X2 on/off, net attributes on/off, 4.5/4.6 mm, 3.6/3.7
  inch, offsets, 2540 and 254 IU per decimil, header lines incl. a second `G04 APERTURE LIST*`,
  33 aperture attributes, custom attributes, TO.P/N/C with escaped names, extra data, layer
  polarity, StartBlock/EndBlock, ClearAllAttributes). The C++ driver runs each script on the real
  `GERBER_PLOTTER` (libcommon.a); per script the golden keeps the file hash (the first 6 files
  verbatim), plus the `TransformRoundChamferedRectToPolygon` results the scripts need. All 2400
  files are byte-identical. Regeneration: the build script first runs the Rust test with
  `KICAD_PLOT_WRITE_SCRIPTS=1` (writes `target/plot-oracle/gerber_scripts.txt`).
* `tests/test_gerber_plotter.rs`: `qa/tests/common/test_gerber_plotter.cpp`
  (`ZeroLengthSegmentEmitsSingleFlash`, issue 24131) plus header/splice, D-code numbering and
  re-selection, arc threshold, 4.5 rounding of 5 nm half cases, the net attribute state machine
  (TD + full re-emission), the pad flash table of the analysis 4.2.5 in both macro modes and
  the aperture definition strings, Outline5P from a chamfered pad, X1 comments and polarity.
* `tests/test_svg_text_oracle.rs`: differential oracle of PLOT-05 and PLOT-06
  (`tests/oracle/svg_text_oracle.cpp`, built by `tests/oracle/build_svg_text_oracle.sh [outdir]
  regen` against the kicad-cli link libraries; golden `tests/golden/svg_text.txt`, 92 KB). 1500
  seeded scripts: 1200 SVG scripts (A3 default / custom / fractional-mil pages, offsets, IU per
  decimil 2540 / 254 / 10 / 1, scales 0.5..2, horizontal mirroring, plot bounding boxes incl.
  negative sizes, precision 3..6, colour / black-and-white / negative modes, dash styles with
  render-settings ratios, every primitive: pen moves, `ThickSegment`, `Arc` by angles (incl.
  radius <= 0) and by 3 points (incl. collinear points, far centres), `ThickArc`, `Rect` (incl.
  degenerate -> `<line>`), `ThickRect`, `Circle` (incl. diameter < width), `ThickCircle`,
  `FilledCircle`, `ThickOval`, `PlotPoly` (open / closed lists, 0 and 1 points), line chains with
  arcs, `ThickPoly`, `BezierCurve`, all PSLIKE pad flashes incl. round rect (polygons replayed
  from the golden) and the no-op regular polygon, `Marker` 0..61, blocks, `Text` and `PlotText`
  with 22 strings (markup overbar / sub / superscript, multi-line, tabs, CR, XML specials,
  Latin-1, CJK, empty), random angles, sizes (0, tiny, non-square, mirrored), all 3x3
  justifications, pen widths 0 / -1 / -2 / negative / large, bold, italic, underline, line
  spacing, explicit or default stroke font) and 300 Gerber text scripts (`Text` / `PlotText`
  with net metadata, X2 on/off, net attributes on/off, 4.5 / 4.6). All 1500 files are
  byte-identical.
* `tests/test_svg_plotter.rs`: `qa/tests/common/test_svg_plotter.cpp` (all 8 `SvgPlotter`
  cases: `FillOpacityMatchesColorAlpha`, `FillOpacityFullyOpaque`,
  `FillOpacityPreservedThroughSetColor`, `StrokeOpacityMatchesColorAlpha`,
  `DefaultPlotViewBoxIsOrigin`, `MirroredSvgPreservesPlotOrigin`, `FitToBoardUsesViewBox`,
  `MirroredFitToBoardUsesMirroredViewBox`; the XML parsing helpers of `svg_test_utils` are
  replaced by string scanning) plus header literal, style group / pen path, dash arrays,
  colour modes, degenerate rect, arc / Bezier syntax, text elements, outline-font fallback,
  `XmlEsc`.
* `tests/test_gbr_metadata.rs`: GUID check values of `basic_test`/`Issue24143`, creation
  date formats, aperture function strings, escaping, the clear-and-repeat net attribute
  sequence of the analysis (4.2.3), plot extensions.
* `tests/test_plotter_base.rs`: base half of `qa/tests/common/test_gerber_plotter.cpp`
  `ZeroLengthSegmentEmitsSingleFlash` (issue 24131: sentinel resolution of a zero-length
  `ThickSegment`); the Gerber half follows with PLOT-03. polyArc step, arc definitions, image
  placeholder.
* There is no C++ test of `gbr_metadata.cpp`.

## Deviations

* `OpenFile` does not open a file: it records the name and clears `PlotterState::output`; the
  caller writes the bytes.
* `GbrMakeCreationDateAttributeString` takes the date and UTC offset as a parameter
  (`GbrCreationDate`) instead of reading the clock and local time zone. It is not covered by
  the oracle (the C++ reads the wall clock); the format is unit-tested.
* Undefined behaviour in C++ turned into defined behaviour: `PlotPoly( SHAPE_LINE_CHAIN )` of
  an empty closed chain (C++ reads `front()` of an empty vector and crashes) passes the empty
  list on; `ThickPoly` of an empty set (C++ `COutline( 0 )` out of range) plots nothing;
  `GetDash*LenIU` without render settings (C++ null dereference) uses the default settings.
* `PlotImage` takes the image pixel size instead of a `wxImage`.
* `GerberPlotter` takes the build version and the creation date (`G04 Created by KiCad (...)
  date ...*`) from `set_build_version` / `set_creation_date` instead of `GetBuildVersion()` and
  `wxDateTime::Now()`; the work file is an in-memory buffer (same `fgets( line, 1024 )`
  chunking and NUL truncation in the splice).
* Custom aperture attributes (`GBR_APERTURE_ATTRIB_OTHER`) with non-ASCII text: C++
  `GBR_APERTURE_METADATA::FormatAttribute` builds `attribute_string + eol_string` through a
  locale-dependent `std::string` -> `wxString` conversion; in a C locale (the oracle driver) a
  non-ASCII text becomes empty (`%*%`), the port keeps the UTF-8 text (as a UTF-8 locale does).
  Excluded from the oracle; no board-plot caller passes non-ASCII custom attributes.
* `FlashRegularPolygon` with a corner count whose `AT_REGULAR_POLY3 + n - 3` is no
  `APERTURE_TYPE` at all (C++ asserts; the aperture definition would be left unterminated)
  plots nothing. Undefined behaviour avoided: `FlashPadCustom` skips empty outlines,
  `FlashPadChamferRoundRect` returns when the polygon generator gives no outline.
* `@{...}` text expressions (`EXPRESSION_EVALUATOR`) are not evaluated by `Text`, `PlotText`,
  `SVG_PLOTTER::Text` (and `GRTextWidth`, kicad-font GAP-3): such text is plotted literally.
* Outline (TrueType) fonts: no outline font engine (kicad-font D1). When `a_font` is `None`
  and the render settings' default font names an outline font, the stroke font is used (what
  C++ does when the font cannot be loaded) and the name is pushed to
  `PlotterState::outline_font_fallbacks` so callers can report the substitution. The outline
  glyph branch of the callback (fractured filled polygons) is ported for when outline glyphs
  exist. The oracle uses the stroke font only.
* `SVG_PLOTTER::StartPlot` takes the `<title>` date from `SvgPlotter::set_creation_date`
  instead of `GetISO8601CurrentDateTime()`. `EndPlot` does not close a file (`output_open`
  becomes false).
* `SVG_PLOTTER::SetCurrentLineWidth` does not `debug_assert` on negative widths (C++
  `wxASSERT_MSG`): `SVG_PLOTTER::Text` passes the caller's pen width (negative allowed by the
  `Text` API) before the absolute value is taken. `GerberPlotter` keeps its `debug_assert`
  (so a negative `PlotText` stroke width panics in debug builds; the oracle avoids it).
* `PSLIKE_PLOTTER::FlashPadRoundRect` needs `TransformRoundChamferedRectToPolygon`, not in
  `kicad-geom` yet: `SvgPlotter::set_round_rect_polygon_fn` installs it (the oracle replays
  the C++ polygons); without it the pad is plotted as `FlashPadRect` (sharp corners).
  Undefined behaviour avoided: an empty generator result / empty custom outline plots nothing.
* `emitSetRGBColor` converts `255.0 * c` like g++ on x86-64 (`cvttsd2si` to 64 bits, then the
  low 32 bits) for out-of-range components; `int` overflows in `userToDeviceCoordinates`
  (clamped far arc centres), the text v-justify offset and `aSize.x * 4 / 3` wrap as on x86.
* wasm32 only: libm differences of one ulp in `sin/cos/atan2` (PORTING.md 9) can reach
  `polyArc` / `Arc` point rounding.

## Known gaps

* `SVG_PLOTTER::PlotImage` bitmap embedding (wxImage PNG encoding, greyscale conversion).
* `SVG_PLOTTER::Rect` with a corner radius: `kicad-geom`'s `SHAPE_RECT::Outline()` returns an
  empty chain for rounded corners (`ROUNDRECT` not ported), so nothing is plotted (C++: a
  rounded polygon). Not reached by board SVG plots of pads (they use `FlashPadRoundRect`).
* `SvgPlotter` round-rect pads depend on `TransformRoundChamferedRectToPolygon` (see
  Deviations), like Gerber chamfered pads.
* `rust/docs/design/plot-engines.md` still describes PLOT-05 as "to do"; the design is as
  announced there (`text` / `plot_text` provided trait methods through `kicad-font`).
* `TransformRoundChamferedRectToPolygon` is not in `kicad-geom` yet: callers of
  `flash_pad_chamfer_round_rect` pass it (the oracle replays the C++ polygons). The FreePoly
  macros of chamfered / custom pads therefore depend on that port and on `kicad-geom`'s
  polygon code (analysis 11.1 items 4 and 5).
* Line chains with arcs are verified through `kicad-geom`'s `ShapeLineChain::append_arc` /
  `ShapeArc` (all oracle scripts match today); they follow that crate's state.
* `RenderSettings` carries only the members the plotters read; the default font is
  there since PLOT-05. If `kicad-pcb`'s `STROKE_PARAMS::Stroke` needs the same dash settings, the struct may move to `kicad-core`.
* `FillT` is defined in `kicad_core::plot_enums`; the board model (`kicad-pcb`) should use it
  instead of defining its own copy.
