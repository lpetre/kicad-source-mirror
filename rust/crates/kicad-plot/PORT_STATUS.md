# kicad-plot port status

Design: `rust/docs/design/plot-engines.md`. Analysis: `rust/docs/analysis/plot-fab.md`.

## Mapping

| C++ | Rust | State |
|---|---|---|
| `include/plotters/plotter.h`, `common/plotters/plotter.cpp` L1-546 (`PLOTTER`) | `plotter.rs` (`Plotter` trait, `PlotterState`, `*_base` fns) | ported (except below) |
| `PLOTTER::BezierCurve` | `plotter::bezier_curve_base` (uses `kicad_geom::bezier_curves::BezierPoly`) | ported |
| `PLOTTER::PlotImage` | `Plotter::plot_image` (pixel size instead of `wxImage`) | ported (placeholder rectangle) |
| `PLOTTER::ThickArc( const EDA_SHAPE& )` | - | skipped: no caller in board plotting; would make kicad-plot depend on the board model |
| `PLOTTER::Text`, `PlotText` (`plotter.cpp` L548-667) | - | PLOT-05 |
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
| `GERBER_PLOTTER`, `SVG_PLOTTER`, `PSLIKE_PLOTTER` | - | PLOT-03, PLOT-04, PLOT-06 |

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
* wasm32 only: libm differences of one ulp in `sin/cos/atan2` (PORTING.md 9) can reach
  `polyArc` / `Arc` point rounding.

## Known gaps

* `Text` / `PlotText` (PLOT-05) and the engines (PLOT-03/04/06) are not in this crate yet.
* `RenderSettings` carries only the members the plotters read; default font and layer name
  arrive with PLOT-05/PLOT-06. If `kicad-pcb`'s `STROKE_PARAMS::Stroke` needs the same dash
  settings, the struct may move to `kicad-core`.
* `FillT` is defined in `kicad_core::plot_enums`; the board model (`kicad-pcb`) should use it
  instead of defining its own copy.
