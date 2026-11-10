# FIX-BEZ-1: Bezier points re-approximated by the outline build (C++ side effect)

Crate: kicad-pcb (`src/convert_shape_list_to_polygon.rs`, `src/board/outline.rs`, a
derived-state writer next to `Board::zone_fill_mut`), call sites in kicad-drc (the outline
steps of the DRC run) and kicad-fab (every plot / drill path that builds the board outline)

Origin: review INT-2b, finding R2-07 (`rust/docs/design/review-2.md`); package M21 recorded
the deviation ("Effects across calls ... are not reproduced").

## The problem

`processShapeSegment` (`pcbnew/convert_shape_list_to_polygon.cpp:365`) calls
`PCB_SHAPE::RebuildBezierToSegmentsPointsList( aErrorMax )` on the board item: after any
outline build every Bezier on Edge.Cuts (board level and in footprints) holds
`m_bezierPoints` approximated with the error of that build (`BOARD::GetBoardPolygonOutlines`
and the DRC outline tests pass `GetDesignSettings().m_MaxError`), not the error it was parsed
with (`pcb_io_kicad_sexpr_parser.cpp:4105`: the board's `m_MaxError` at parse time, which is
the constructor default 0.005 mm because the project settings are bound after the parse).

Readers of `m_bezierPoints` that do not rebuild it themselves:

* `EDA_SHAPE::getBoundingBox` (`common/eda_shape.cpp:1741`: the box of the curve points),
  so `PCB_SHAPE::GetBoundingBox`, `BOARD::ComputeBoundingBox` / `GetBoardEdgesBoundingBox`,
  footprint boxes;
* `EDA_SHAPE::GetLength` (`:551`), `IsClosed` (`:698`), `hitTest` (`:1838`, `:2128`),
  `Compare` (`:3677`), `operator==` / `Similarity`;
* the line-ending tangents (`:4199`, `:4346`).

`makeEffectiveShapes` and `TransformShapeToPolygon` rebuild the list from `getMaxError()` and
are not affected.

The port keeps the board unchanged (the build uses a local point list), so after a DRC run or
a plot the Rust bezier still has the parse-time points. The results differ when the project's
`rules.max_error` is not 0.005 mm and the board has a Bezier on Edge.Cuts: the bounding box of
that Bezier (and what is derived from it: board edges box in the drill map, the Gerber job
file size, page-fit plots) moves by up to the difference of the two approximations. 10 of the
270 corpus projects have `max_error` 0.01; none of their boards has an Edge.Cuts Bezier, so
no corpus output differs today.

## Work

1. kicad-pcb: a derived-state writer `Board::rebuild_bezier_points( ShapeId, max_error )`
   (no time stamp bump, no cache invalidation, like the C++ call; `board-model.md` 8.4), and a
   function listing the Beziers an outline build touches, in the order
   `doConvertOutlineToPolygon` reaches them (it only touches the shapes it consumes).
2. Hosts: after each ported call of `BuildBoardPolygonOutlines` /
   `TestBoardOutlinesGraphicItems` / `GetBoardPolygonOutlines` / `ConvertOutlineToPolygon`
   that runs on the real board in C++ (DRC outline test, plot of negative layers / mask
   subtraction, drill map, courtyard build of footprints), apply the writer with the same
   error. List the call sites in the PORT_STATUS.md of each crate.
3. Remove the M21 deviation entry.

## Verification

A fixture with Beziers on Edge.Cuts (board level and in a footprint) and a `.kicad_pro` with
`"max_error": 0.02`. Extend `tests/oracle/m21_outline_oracle.cpp` (it links the pcb-dump
objects; build under the C++ lock): print `GetBezierPoints()`, `GetBoundingBox()` and
`GetLength()` of every Bezier before and after `BuildBoardPolygonOutlines` and after a
`DRC_ENGINE::RunTests`, and the board edges box. The Rust replay must match bit for bit;
without the writer the "after" lines differ.
