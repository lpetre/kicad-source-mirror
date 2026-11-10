// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `pcbnew/pcb_table.h` and `pcbnew/pcb_table.cpp` (`PCB_TABLE`; with `generated` also
//! `PCB_DRILL_CHART` / `PCB_STACKUP_TABLE`): data, constructors and delegation shims (package
//! M0c), the overrides (package M11). A table is a container: its mutating virtuals
//! cascade to the cells through `Board` methods (`board-model.md` 8.3), so it has no
//! `BoardItemMut` impl. The overrides of the generated-table classes are reached through the
//! same inherent methods, which test `generated` (M26).

use std::collections::BTreeMap;

use kicad_core::color4d::Color4D;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, LineStyle, PCB_IU_SCALE, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, ShapePolySet, Vector2I};

use std::sync::Arc;

use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::ERROR_INSIDE;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::{
    ANGLE_0, ANGLE_180, EdaAngle, FlipDirection, Shape, ShapeCompound, ShapeSegment, ShapeSimple,
};

use crate::board::Board;
use crate::board_item;
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::eda_text::EdaTextExt;
use crate::ids::TableId;
use crate::ids::TablecellId;
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::pcb_generated_table::{GeneratedTableData, GeneratedTableKind};
use crate::pcb_shape::DEFAULT_LINE_WIDTH;
use crate::pcb_tablecell::PcbTablecell;
use crate::traits::EdaTextItem;
use crate::traits::{BoardItem, Flashing, Invalidate};

/// `PCB_TABLE` (and, with `generated`, `PCB_DRILL_CHART` / `PCB_STACKUP_TABLE`).
#[derive(Clone, Debug)]
pub struct PcbTable {
    pub base: BoardItemBase,
    pub stroke_external: bool,           // m_strokeExternal
    pub stroke_header_separator: bool,   // m_StrokeHeaderSeparator
    pub border_stroke: StrokeParams,     // m_borderStroke
    pub stroke_rows: bool,               // m_strokeRows
    pub stroke_columns: bool,            // m_strokeColumns
    pub separators_stroke: StrokeParams, // m_separatorsStroke
    pub col_count: i32,                  // m_colCount
    pub col_widths: BTreeMap<i32, i32>,  // m_colWidths
    pub row_heights: BTreeMap<i32, i32>, // m_rowHeights
    /// m_cells (row-major).
    pub cells: Vec<TablecellId>,
    /// The `PCB_GENERATED_TABLE` members of a drill chart / stack-up table.
    pub generated: Option<Box<GeneratedTableData>>,
}

impl Default for PcbTable {
    /// `PCB_TABLE( aParent )` with a nil UUID: `mmToIU( DEFAULT_LINE_WIDTH )` strokes.
    fn default() -> Self {
        PcbTable::with_line_width(PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH))
    }
}

impl PcbTable {
    /// `PCB_TABLE( aParent, aLineWidth )` with a nil UUID: every stroke flag on, border and
    /// separator strokes `( aLineWidth, DEFAULT, UNSPECIFIED )`, no columns.
    pub fn with_line_width(line_width: i32) -> Self {
        let stroke = StrokeParams::new(line_width, LineStyle::Default, Color4D::UNSPECIFIED);
        PcbTable {
            base: BoardItemBase::default(),
            stroke_external: true,
            stroke_header_separator: true,
            border_stroke: stroke.clone(),
            stroke_rows: true,
            stroke_columns: true,
            separators_stroke: stroke,
            col_count: 0,
            col_widths: BTreeMap::new(),
            row_heights: BTreeMap::new(),
            cells: Vec::new(),
            generated: None,
        }
    }

    /// `new PCB_TABLE( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut t = PcbTable::default();
        t.base.uuid = Kiid::new(kiid_generator);
        t
    }

    /// `PCB_GENERATED_TABLE( aParent, aType, aLineWidth )` with a nil UUID and the given
    /// members (`PCB_DRILL_CHART`: 0.1 mm; `PCB_STACKUP_TABLE`: `DEFAULT_LINE_WIDTH`).
    pub fn with_generated(line_width: i32, generated: GeneratedTableData) -> Self {
        PcbTable {
            generated: Some(Box::new(generated)),
            ..PcbTable::with_line_width(line_width)
        }
    }

    /// `Type()`: `PCB_TABLE_T`, `PCB_DRILL_CHART_T` or `PCB_STACKUP_TABLE_T`.
    pub fn table_type(&self) -> KicadT {
        match self.generated.as_deref().map(|g| &g.kind) {
            None => KicadT::PcbTableT,
            Some(GeneratedTableKind::DrillChart { .. }) => KicadT::PcbDrillChartT,
            Some(GeneratedTableKind::StackupTable) => KicadT::PcbStackupTableT,
        }
    }

    /// `GetClass()` of the concrete class.
    pub fn table_class(&self) -> &'static str {
        match self.generated.as_deref().map(|g| &g.kind) {
            None => "PCB_TABLE",
            Some(GeneratedTableKind::DrillChart { .. }) => "PCB_DRILL_CHART",
            Some(GeneratedTableKind::StackupTable) => "PCB_STACKUP_TABLE",
        }
    }

    /// `GetColCount()`.
    pub fn col_count(&self) -> i32 {
        self.col_count
    }

    /// `SetColCount()`.
    pub fn set_col_count(&mut self, count: i32) {
        self.col_count = count;
    }

    /// `GetRowCount()`: `m_cells.size() / m_colCount` (0 without columns).
    pub fn row_count(&self) -> i32 {
        if self.col_count == 0 {
            0
        } else {
            self.cells.len() as i32 / self.col_count
        }
    }

    /// `GetColWidth( aCol )`: 0 for a column without a stored width.
    pub fn col_width(&self, col: i32) -> i32 {
        self.col_widths.get(&col).copied().unwrap_or(0)
    }

    /// `SetColWidth( aCol, aWidth )`.
    pub fn set_col_width(&mut self, col: i32, width: i32) {
        self.col_widths.insert(col, width);
    }

    /// `GetRowHeight( aRow )`: 0 for a row without a stored height.
    pub fn row_height(&self, row: i32) -> i32 {
        self.row_heights.get(&row).copied().unwrap_or(0)
    }

    /// `SetRowHeight( aRow, aHeight )`.
    pub fn set_row_height(&mut self, row: i32, height: i32) {
        self.row_heights.insert(row, height);
    }

    /// `GetCell( aRow, aCol )` as a handle.
    pub fn cell(&self, row: i32, col: i32) -> Option<TablecellId> {
        let index = row.checked_mul(self.col_count)?.checked_add(col)?;
        usize::try_from(index)
            .ok()
            .and_then(|i| self.cells.get(i).copied())
    }

    /// Port of `setAutosizedRowHeights( aNeeded )` (virtual): `PCB_TABLE` stores each needed
    /// height. The `PCB_GENERATED_TABLE` override (line metrics of data rows, the common
    /// height of the data rows, a `Normalize()` first) belongs to the generated-table refresh
    /// (package M26).
    pub fn set_autosized_row_heights(&mut self, needed: &[i32]) {
        if self.generated.is_some() {
            crate::port_pending!("PCB_GENERATED_TABLE::setAutosizedRowHeights (M26)", needed);
        }

        for (row, &height) in needed.iter().enumerate() {
            self.set_row_height(row as i32, height);
        }
    }

    /// Port of the sizing part of `Autosize()`: every column as wide as its widest text plus
    /// 1.5 times the horizontal margins, the row heights from the tallest text plus the
    /// vertical margins (through `setAutosizedRowHeights`). The text extents are measured in
    /// each cell's own frame. The closing `Normalize()` moves the cells, which are separate
    /// arena items: [`Board::table_autosize`] runs this on a clone and then normalizes.
    pub fn autosize(&mut self, ctx: &BoardCtx<'_>) {
        let mut extents: Vec<Vec<Box2I>> = Vec::new();

        for row in 0..self.row_count() {
            extents.push(Vec::new());

            for col in 0..self.col_count() {
                let Some(cell) = self.cell_ref(ctx, row, col) else {
                    extents[row as usize].push(Box2I::default());
                    continue;
                };
                let mut text_poly = ShapePolySet::new();
                cell.transform_text_to_poly_set(ctx, &mut text_poly, 0, ARC_LOW_DEF, ERROR_INSIDE);

                // Measured in the cell's own frame, or a rotated table swaps its widths and
                // heights
                text_poly.rotate(
                    -EdaTextItem::draw_rotation(cell, ctx),
                    EdaTextItem::draw_pos(cell, ctx),
                );
                extents[row as usize].push(text_poly.bbox(0));
            }
        }

        for col in 0..self.col_count() {
            let mut col_width: i32 = 0;

            for row in 0..self.row_count() {
                let Some(cell) = self.cell_ref(ctx, row, col) else {
                    continue;
                };
                let margins = cell.textbox.margin_left + cell.textbox.margin_right;
                let w = extents[row as usize][col as usize].width() as f64 + margins as f64 * 1.5;
                col_width = col_width.max(f64_to_i32_x86(w));
            }

            self.set_col_width(col, col_width);
        }

        let mut needed: Vec<i32> = Vec::new();

        for row in 0..self.row_count() {
            let mut row_height: i32 = 0;

            for col in 0..self.col_count() {
                let Some(cell) = self.cell_ref(ctx, row, col) else {
                    continue;
                };
                let margins = cell.textbox.margin_top + cell.textbox.margin_bottom;
                row_height =
                    row_height.max(extents[row as usize][col as usize].height() as i32 + margins);
            }

            needed.push(row_height);
        }

        self.set_autosized_row_heights(&needed);
    }

    /// `GetLayer` of the table: `m_layer` (the cells carry the same layer).
    pub fn table_layer(&self) -> PcbLayerId {
        self.base.layer
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for PcbTable {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbTable {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        self.table_type()
    }
    fn class_name(&self) -> &'static str {
        self.table_class()
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbTable::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbTable::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbTable::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbTable::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbTable::item_description(self, ctx, units, full)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTable::position(self, ctx)
    }
    fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        PcbTable::run_on_children(self, ctx, mode, f)
    }
    fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        PcbTable::transform_shape_to_poly_set(self, ctx, buffer, layer, clearance, error, error_loc)
    }
    fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        PcbTable::transform_shape_to_polygon(
            self,
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        )
    }
}

// ---- the PCB_TABLE overrides (package M11) -----------------------------------------------

impl PcbTable {
    /// The cell at `GetCell( aRow, aCol )` as a reference.
    fn cell_ref<'a>(&self, ctx: &BoardCtx<'a>, row: i32, col: i32) -> Option<&'a PcbTablecell> {
        let id = self.cell(row, col)?;
        ctx.board.tablecells.get(id)
    }

    /// The cells (`m_cells`) as references, in order.
    fn cell_refs<'a>(&self, ctx: &BoardCtx<'a>) -> Vec<&'a PcbTablecell> {
        self.cells
            .iter()
            .filter_map(|&id| ctx.board.tablecells.get(id))
            .collect()
    }

    /// Port of `PCB_TABLE::GetBoundingBox`: the cells' boxes, inflated by half the border
    /// width when the external border is stroked.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut bbox = Box2I::default();

        for cell in self.cell_refs(ctx) {
            bbox.merge(&cell.bounding_box(ctx));
        }

        if self.stroke_external {
            bbox.inflate(self.border_stroke.width() / 2);
        }

        bbox
    }

    /// Port of `PCB_TABLE::GetEnd`: the position plus the column widths and row heights.
    pub fn end(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        let mut table_size = Vector2I::default();

        for ii in 0..self.col_count() {
            table_size.x += self.col_width(ii);
        }

        for ii in 0..self.row_count() {
            table_size.y += self.row_height(ii);
        }

        self.position(ctx) + table_size
    }

    /// `GetBorderStyle()`: `SOLID` for `DEFAULT`.
    pub fn border_style(&self) -> LineStyle {
        match self.border_stroke.line_style() {
            LineStyle::Default => LineStyle::Solid,
            style => style,
        }
    }

    /// `GetSeparatorsStyle()`: `SOLID` for `DEFAULT`.
    pub fn separators_style(&self) -> LineStyle {
        match self.separators_stroke.line_style() {
            LineStyle::Default => LineStyle::Solid,
            style => style,
        }
    }

    /// `IsGenerated()`.
    pub fn is_generated(&self) -> bool {
        self.generated.is_some()
    }

    /// Port of `PCB_TABLE::DrawBorders( aCallback )`: the separator and border segments with
    /// their strokes, in the C++ order (column separators, row separators, external border).
    pub fn draw_borders(
        &self,
        ctx: &BoardCtx<'_>,
        callback: &mut dyn FnMut(Vector2I, Vector2I, &StrokeParams),
    ) {
        if self.cells.is_empty() {
            // Shouldn't be possible....
            return;
        }

        let rows = self.row_count();
        let cols = self.col_count();
        let corner_seq = |row: i32, col: i32, angle: EdaAngle| -> Vec<Vector2I> {
            self.cell_ref(ctx, row, col)
                .map(|c| crate::pcb_textbox::corners_in_sequence(c, ctx, angle))
                .unwrap_or_default()
        };

        let Some(cell00) = self.cell_ref(ctx, 0, 0) else {
            return;
        };
        let draw_angle = EdaTextItem::draw_rotation(cell00, ctx);
        let top_left = corner_seq(0, 0, draw_angle);
        let bottom_left = corner_seq(rows - 1, 0, draw_angle);
        let top_right = corner_seq(0, cols - 1, draw_angle);
        let bot_right = corner_seq(rows - 1, cols - 1, draw_angle);
        let mut stroke;

        for col in 0..cols - 1 {
            for row in 0..rows {
                if row == 0 && self.stroke_header_separator {
                    stroke = self.border_stroke.clone();
                } else if self.stroke_columns {
                    stroke = self.separators_stroke.clone();
                } else {
                    continue;
                }

                let Some(cell) = self.cell_ref(ctx, row, col) else {
                    continue;
                };

                if cell.col_span() == 0 {
                    continue;
                }

                if col + cell.col_span() == cols {
                    continue;
                }

                let corners = crate::pcb_textbox::corners_in_sequence(cell, ctx, draw_angle);

                if corners.len() == 4 {
                    callback(corners[1], corners[2], &stroke);
                }
            }
        }

        for row in 0..rows - 1 {
            if row == 0 && self.stroke_header_separator {
                stroke = self.border_stroke.clone();
            } else if self.stroke_rows {
                stroke = self.separators_stroke.clone();
            } else {
                continue;
            }

            for col in 0..cols {
                let Some(cell) = self.cell_ref(ctx, row, col) else {
                    continue;
                };

                if cell.row_span() == 0 {
                    continue;
                }

                if row + cell.row_span() == rows {
                    continue;
                }

                let corners = crate::pcb_textbox::corners_in_sequence(cell, ctx, draw_angle);

                if corners.len() == 4 {
                    callback(corners[2], corners[3], &stroke);
                }
            }
        }

        if self.stroke_external
            && self.border_stroke.width() >= 0
            && [&top_left, &bottom_left, &top_right, &bot_right]
                .iter()
                .all(|c| c.len() == 4)
        {
            callback(top_left[0], top_right[1], &self.border_stroke);
            callback(top_right[1], bot_right[2], &self.border_stroke);
            callback(bot_right[2], bottom_left[3], &self.border_stroke);
            callback(bottom_left[3], top_left[0], &self.border_stroke);
        }
    }

    /// Port of `PCB_TABLE::GetEffectiveShape`: the outline polygon of the grid plus a segment
    /// per drawn border. (A table without cells, a null dereference in C++, gives an empty
    /// compound.)
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        _layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut shape = ShapeCompound::new();

        if let Some(cell00) = self.cell_ref(ctx, 0, 0) {
            let rows = self.row_count();
            let cols = self.col_count();
            let angle = EdaTextItem::draw_rotation(cell00, ctx);
            let corner_seq = |row: i32, col: i32| -> Vec<Vector2I> {
                self.cell_ref(ctx, row, col)
                    .map(|c| crate::pcb_textbox::corners_in_sequence(c, ctx, angle))
                    .unwrap_or_default()
            };
            let top_left = corner_seq(0, 0);
            let bottom_left = corner_seq(rows - 1, 0);
            let top_right = corner_seq(0, cols - 1);
            let bottom_right = corner_seq(rows - 1, cols - 1);

            if [&top_left, &bottom_left, &top_right, &bottom_right]
                .iter()
                .all(|c| c.len() == 4)
            {
                let mut pts = ShapeSimple::new();
                pts.append(top_left[3]);
                pts.append(top_right[2]);
                pts.append(bottom_right[2]);
                pts.append(bottom_left[3]);
                shape.add_shape(Box::new(pts));
            }

            self.draw_borders(ctx, &mut |pt_a, pt_b, stroke| {
                shape.add_shape(Box::new(ShapeSegment::new(pt_a, pt_b, stroke.width())));
            });
        }

        Arc::new(shape)
    }

    /// Port of `PCB_TABLE::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        let mut rect = self.bounding_box(ctx);

        rect.inflate(accuracy);

        rect.contains(pos)
    }

    /// Port of `PCB_TABLE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut rect = *rect;

        rect.inflate(accuracy);

        if contained {
            return rect.contains_box(&self.bounding_box(ctx));
        }

        rect.intersects(&self.bounding_box(ctx))
    }

    /// Port of `PCB_TABLE::GetItemDescription` (untranslated): "%d column table".
    pub fn item_description(
        &self,
        _ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        format!("{} column table", self.col_count)
    }

    /// Port of `PCB_TABLE::GetPosition`: the first cell's position, the origin without cells.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        match self
            .cells
            .first()
            .and_then(|&id| ctx.board.tablecells.get(id))
        {
            // Return origin if table has no cells
            None => Vector2I::new(0, 0),
            Some(cell) => cell.position(ctx),
        }
    }

    /// Port of `PCB_TABLE::RunOnChildren`: every cell (cells have no children of their own).
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        _mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        for &cell in &self.cells {
            if let Some(item) = ctx.item(cell.into()) {
                f(item);
            }
        }
    }

    /// Port of `PCB_TABLE::TransformShapeToPolySet`: `TransformGraphicItemsToPolySet` (no
    /// render settings: every border as a solid oval), inflated (chamfered) by a positive
    /// clearance.
    pub fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        _layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        if clearance <= 0 {
            self.transform_graphic_items_to_poly_set(ctx, buffer, error, error_loc);
        } else {
            let mut tmp = ShapePolySet::new();
            self.transform_graphic_items_to_poly_set(ctx, &mut tmp, error, error_loc);
            tmp.inflate(clearance, CornerStrategy::ChamferAllCorners, error, false);
            buffer.append_set(&tmp);
        }
    }

    /// Port of `PCB_TABLE::TransformGraphicItemsToPolySet( .., aRenderSettings = nullptr )`:
    /// the borders (line styles ignored without render settings) and the cells' text.
    pub fn transform_graphic_items_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        max_error: i32,
        error_loc: ErrorLoc,
    ) {
        // Convert graphic items (segments and texts) to a set of polygonal shapes
        // aRenderSettings is used to draw lines when line style != LINE_STYLE::SOLID, so
        // if nullptr line style will be ignored
        self.draw_borders(ctx, &mut |pt_a, pt_b, stroke| {
            transform_oval_to_polygon(buffer, pt_a, pt_b, stroke.width(), max_error, error_loc, 0);
        });

        for cell in self.cell_refs(ctx) {
            cell.transform_text_to_poly_set(ctx, buffer, 0, max_error, ERROR_INSIDE);
        }
    }

    /// Port of `PCB_TABLE::TransformShapeToPolygon`: every cell's box, grown by half the
    /// widest drawn stroke.
    #[allow(clippy::too_many_arguments)]
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        _ignore_line_width: bool,
    ) {
        let mut gap = clearance;

        if self.stroke_columns || self.stroke_rows {
            gap = gap.max(clearance + self.separators_stroke.width() / 2);
        }

        if self.stroke_external || self.stroke_header_separator {
            gap = gap.max(clearance + self.border_stroke.width() / 2);
        }

        for cell in self.cell_refs(ctx) {
            cell.transform_shape_to_polygon(ctx, buffer, layer, gap, error, error_loc, false);
        }
    }

    /// Port of `PCB_TABLE::Compare( aTable, aOther )` (the save-order comparator): cell
    /// count, columns, widths, rows, heights, then per cell `PCB_SHAPE::Compare` (the
    /// `EDA_SHAPE` body) and `EDA_TEXT::Compare`.
    pub fn compare(ctx: &BoardCtx<'_>, table: &PcbTable, other: &PcbTable) -> i32 {
        let mut diff = table.cells.len() as i32 - other.cells.len() as i32;

        if diff != 0 {
            return diff;
        }

        diff = table.col_count() - other.col_count();

        if diff != 0 {
            return diff;
        }

        for col in 0..table.col_count() {
            diff = table.col_width(col).wrapping_sub(other.col_width(col));

            if diff != 0 {
                return diff;
            }
        }

        diff = table.row_count() - other.row_count();

        if diff != 0 {
            return diff;
        }

        for row in 0..table.row_count() {
            diff = table.row_height(row).wrapping_sub(other.row_height(row));

            if diff != 0 {
                return diff;
            }
        }

        for row in 0..table.row_count() {
            for col in 0..table.col_count() {
                let (Some(cell), Some(other_cell)) =
                    (table.cell_ref(ctx, row, col), other.cell_ref(ctx, row, col))
                else {
                    continue;
                };

                diff = cell
                    .textbox
                    .shape
                    .shape
                    .compare(&other_cell.textbox.shape.shape);

                if diff != 0 {
                    return diff;
                }

                diff = cell.compare_text(ctx, other_cell);

                if diff != 0 {
                    return diff;
                }
            }
        }

        0
    }
}

/// `mirrorCellColumns( aCells, aRowCount, aColCount )` (static, `pcb_table.cpp`): reverse the
/// columns of every row, keeping each column span together.
fn mirror_cell_columns(
    cells: &mut [TablecellId],
    spans: &BTreeMap<TablecellId, i32>,
    row_count: i32,
    col_count: i32,
) {
    let old_cells = cells.to_vec();

    for row in 0..row_count {
        let row_offset = row * col_count;
        let mut col = 0;

        while col < col_count {
            let span = spans
                .get(&old_cells[(row_offset + col) as usize])
                .copied()
                .unwrap_or(1)
                .clamp(1, col_count - col);

            for ii in 0..span {
                cells[(row_offset + col_count - col - span + ii) as usize] =
                    old_cells[(row_offset + col + ii) as usize];
            }

            col += span;
        }
    }
}

/// The `PCB_TABLE` mutators that touch the cells (`board-model.md` 8.3: a table is a
/// container, its cells are separate arena items): `Normalize`, `Autosize`, `Rotate`, `Flip`,
/// `Mirror`.
impl Board {
    /// Port of `PCB_TABLE::Normalize()`: lay the cells out on the grid of column widths and
    /// row heights, keeping the first cell's centre where it is. Like the C++ `operator[]`,
    /// reading a missing width or height stores 0 for it.
    pub fn table_normalize(&mut self, table: TableId) {
        let Some(t) = self.tables.get(table) else {
            return;
        };

        if t.cells.is_empty() {
            return;
        }

        let cells: Vec<TablecellId> = t.cells.clone();
        let cell0 = cells[0];

        let (cell_angle, stable_center) = {
            let ctx = BoardCtx::bare(self);
            let Some(c0) = self.tablecells.get(cell0) else {
                return;
            };
            (c0.text_angle(&ctx), c0.bounding_box(&ctx).center())
        };

        // The C++ reads the widths and heights with operator[], which stores 0 for a missing
        // entry: work on copies and store them at the end.
        let mut col_widths = self[table].col_widths.clone();
        let mut row_heights = self[table].row_heights.clone();
        let (span0_col, span0_row) = {
            let c0 = &self[cell0];
            (c0.col_span(), c0.row_span())
        };

        let mut cell0_width = *col_widths.entry(0).or_insert(0);
        let mut cell0_height = *row_heights.entry(0).or_insert(0);

        if span0_col > 1 {
            for ii in 1..span0_col {
                cell0_width += *col_widths.entry(ii).or_insert(0);
            }
        }

        if span0_row > 1 {
            for ii in 1..span0_row {
                cell0_height += *row_heights.entry(ii).or_insert(0);
            }
        }

        if cell_angle != ANGLE_0 {
            for &cell in &cells {
                self.modify_tablecell(cell, |c, env| c.rotate(env, stable_center, -cell_angle));
            }
        }

        let unrotated_origin = stable_center - Vector2I::new(cell0_width / 2, cell0_height / 2);
        let mut y = unrotated_origin.y;
        let (rows, cols) = (self[table].row_count(), self[table].col_count());

        for row in 0..rows {
            let mut x = unrotated_origin.x;
            let row_height = *row_heights.entry(row).or_insert(0);

            for col in 0..cols {
                let col_width = *col_widths.entry(col).or_insert(0);

                let Some(cell) = self[table].cell(row, col) else {
                    continue;
                };

                let mut cell_width = col_width;
                let mut cell_height = row_height;
                let (col_span, row_span) = {
                    let c = &self[cell];
                    (c.col_span(), c.row_span())
                };

                if col_span > 1 || row_span > 1 {
                    for ii in col + 1..col + col_span {
                        cell_width += *col_widths.entry(ii).or_insert(0);
                    }

                    for ii in row + 1..row + row_span {
                        cell_height += *row_heights.entry(ii).or_insert(0);
                    }
                }

                let pos = Vector2I::new(x, y);
                let end = Vector2I::new(x + cell_width, y + cell_height);

                let (cur_pos, cur_end) = {
                    let c = &self[cell];
                    (
                        crate::eda_shape::shape_position(c),
                        c.textbox.shape.shape.end(),
                    )
                };

                if cur_pos != pos {
                    self.modify_tablecell(cell, |c, env| {
                        c.set_position(env, pos);
                        c.textbox.text.clear_render_cache();
                    });
                }

                let cur_end = if cur_pos != pos {
                    self[cell].textbox.shape.shape.end()
                } else {
                    cur_end
                };

                if cur_end != end {
                    self.modify_tablecell(cell, |c, env| {
                        c.set_end(env, end);
                        c.textbox.text.clear_render_cache();
                    });
                }

                x += col_width;
            }

            y += row_height;
        }

        if col_widths != self[table].col_widths || row_heights != self[table].row_heights {
            self.modify_table(table, |t, _| {
                t.col_widths = col_widths;
                t.row_heights = row_heights;
            });
        }

        if cell_angle != ANGLE_0 {
            for &cell in &cells {
                self.modify_tablecell(cell, |c, env| c.rotate(env, stable_center, cell_angle));
            }
        }

        let new_center = {
            let ctx = BoardCtx::bare(self);
            self[cell0].bounding_box(&ctx).center()
        };

        if new_center != stable_center {
            let correction = stable_center - new_center;

            for &cell in &cells {
                self.modify_tablecell(cell, |c, env| c.move_by(env, correction));
            }
        }
    }

    /// Port of `PCB_TABLE::Autosize()`: column widths and row heights from the cells' text
    /// extents ([`PcbTable::autosize`], clone-edit-store), then `Normalize()`.
    pub fn table_autosize(&mut self, table: TableId) {
        let Some(mut t) = self.tables.get(table).cloned() else {
            return;
        };

        {
            let ctx = BoardCtx::bare(self);
            t.autosize(&ctx);
        }

        let (col_widths, row_heights) = (t.col_widths, t.row_heights);
        self.modify_table(table, |t, _| {
            t.col_widths = col_widths;
            t.row_heights = row_heights;
        });

        self.table_normalize(table);
    }

    /// Port of `PCB_TABLE::Rotate( aRotCentre, aAngle )`: every cell, then `Normalize()`.
    pub(crate) fn table_rotate(&mut self, table: TableId, centre: Vector2I, angle: EdaAngle) {
        let Some(t) = self.tables.get(table) else {
            return;
        };

        if t.cells.is_empty() {
            return;
        }

        for cell in t.cells.clone() {
            self.modify_tablecell(cell, |c, env| c.rotate(env, centre, angle));
        }

        self.table_normalize(table);
    }

    /// `PCB_TABLE::Move( aMoveVector )`: every cell.
    fn table_move(&mut self, table: TableId, delta: Vector2I) {
        let cells = self
            .tables
            .get(table)
            .map(|t| t.cells.clone())
            .unwrap_or_default();

        for cell in cells {
            self.modify_tablecell(cell, |c, env| c.move_by(env, delta));
        }
    }

    /// `PCB_TABLE::SetLayer( aLayer )`: `m_layer`, then every cell's `SetLayer`.
    fn table_set_layer(&mut self, table: TableId, layer: PcbLayerId) {
        self.set_item_layer(table.into(), layer);
    }

    /// The columns of the grid reversed (`mirrorCellColumns`) and the column widths with
    /// them: the shared tail of both `PCB_TABLE::Flip` paths, before the layer change.
    fn table_mirror_columns(&mut self, table: TableId) {
        let Some(t) = self.tables.get(table) else {
            return;
        };

        let spans: BTreeMap<TablecellId, i32> = t
            .cells
            .iter()
            .filter_map(|&c| self.tablecells.get(c).map(|cell| (c, cell.col_span())))
            .collect();
        let (rows, cols) = (t.row_count(), t.col_count());
        let mut cells = t.cells.clone();

        // Flipping a cell turns its text 180 degrees and the grid is laid out in the frame the
        // cells read in, so that turn reverses the rows already. Only the columns are left.
        mirror_cell_columns(&mut cells, &spans, rows, cols);

        self.modify_table(table, |t, _| {
            let mut new_col_widths = BTreeMap::new();

            for col in 0..cols {
                let w = *t.col_widths.entry(cols - 1 - col).or_insert(0);
                new_col_widths.insert(col, w);
            }

            t.col_widths = new_col_widths;
            t.cells = cells;
        });
    }

    /// `GetFlippedLayer( GetLayer() )` of a stored table.
    fn table_flipped_layer(&self, table: TableId) -> Option<PcbLayerId> {
        let ctx = BoardCtx::bare(self);
        let item = ctx.item(table.into())?;
        Some(item.flipped_layer(item.layer()))
    }

    /// Port of `PCB_TABLE::Flip( aCentre, aFlipDirection )`. A footprint child flips each
    /// cell in the library frame; a board table is turned upright, flipped about its origin,
    /// re-laid out and moved to the mirrored place of its box. (A board table without cells,
    /// a null dereference in C++, is left alone.)
    pub(crate) fn table_flip(&mut self, table: TableId, centre: Vector2I, dir: FlipDirection) {
        let Some(t) = self.tables.get(table) else {
            return;
        };

        let has_fp = {
            let ctx = BoardCtx::bare(self);
            board_item::parent_footprint(t, &ctx).is_some()
        };

        // FP-child path keeps cells in lib frame, the standalone path below
        // would corrupt them because FOOTPRINT::Flip zeroes the FP rotation.
        if has_fp {
            for cell in t.cells.clone() {
                self.modify_tablecell(cell, |c, env| c.flip(env, centre, dir));
            }

            // Flipping a cell also turns its text 180 degrees, so in the frame the table reads
            // in it always comes out mirrored left to right.
            self.table_mirror_columns(table);

            if let Some(layer) = self.table_flipped_layer(table) {
                self.table_set_layer(table, layer);
            }

            return;
        }

        if t.cells.is_empty() {
            return;
        }

        let cell0 = t.cells[0];
        let (original_bbox, original_angle) = {
            let ctx = BoardCtx::bare(self);
            (t.bounding_box(&ctx), self[cell0].text_angle(&ctx))
        };

        let target_pos = if dir == FlipDirection::LeftRight {
            Vector2I::new(2 * centre.x - original_bbox.right(), original_bbox.top())
        } else {
            Vector2I::new(original_bbox.left(), 2 * centre.y - original_bbox.bottom())
        };

        if original_angle != ANGLE_0 {
            let pos = self.table_position(table);
            self.table_rotate(table, pos, -original_angle);
        }

        let table_origin = self.table_position(table);

        for cell in self[table].cells.clone() {
            self.modify_tablecell(cell, |c, env| c.flip(env, table_origin, dir));
        }

        self.table_mirror_columns(table);

        if let Some(layer) = self.table_flipped_layer(table) {
            self.table_set_layer(table, layer);
        }

        self.table_normalize(table);

        if original_angle != ANGLE_0 {
            let pos = self.table_position(table);
            self.table_rotate(table, pos, -original_angle);
        }

        let new_bbox = {
            let ctx = BoardCtx::bare(self);
            self[table].bounding_box(&ctx)
        };

        self.table_move(table, target_pos - new_bbox.position());
    }

    /// Port of `PCB_TABLE::Mirror( aCentre, aFlipDirection )`: `Flip` with the layers and the
    /// text mirroring restored. TOP_BOTTOM needs a 180 deg pre-rotation because
    /// rotate-then-LR-flip equals TB-flip.
    pub(crate) fn table_mirror(&mut self, table: TableId, centre: Vector2I, dir: FlipDirection) {
        let Some(t) = self.tables.get(table) else {
            return;
        };

        let orig_layer = t.base.layer;
        let orig_cells: Vec<(TablecellId, PcbLayerId, bool)> = t
            .cells
            .iter()
            .filter_map(|&c| {
                self.tablecells.get(c).map(|cell| {
                    (
                        c,
                        cell.textbox.shape.base.item.layer,
                        cell.textbox.text.is_mirrored(),
                    )
                })
            })
            .collect();

        if dir == FlipDirection::TopBottom {
            self.table_rotate(table, centre, ANGLE_180);
        }

        self.table_flip(table, centre, FlipDirection::LeftRight);
        self.table_set_layer(table, orig_layer);

        for (cell, layer, mirrored) in orig_cells {
            self.set_item_layer(cell.into(), layer);
            self.modify_tablecell(cell, |c, _| c.textbox.text.set_mirrored(mirrored));
        }
    }

    /// `PCB_TABLE::GetPosition()` of a stored table.
    fn table_position(&self, table: TableId) -> Vector2I {
        let ctx = BoardCtx::bare(self);
        self.tables
            .get(table)
            .map_or(Vector2I::new(0, 0), |t| t.position(&ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::MutEnv;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_TABLE::PCB_TABLE( aParent, aType, aLineWidth )` (pcbnew/pcb_table.cpp).
    #[test]
    fn constructor_values() {
        let t = PcbTable::default();
        assert!(
            t.stroke_external && t.stroke_header_separator && t.stroke_rows && t.stroke_columns
        );
        assert_eq!(t.border_stroke.width(), 100_000);
        assert_eq!(t.separators_stroke.line_style(), LineStyle::Default);
        assert_eq!((t.col_count, t.row_count()), (0, 0));
        assert!(t.generated.is_none());
        assert_eq!(t.type_(), KicadT::PcbTableT);
        let chart = PcbTable::with_generated(
            PCB_IU_SCALE.mm_to_iu(0.1),
            GeneratedTableData::new(GeneratedTableKind::drill_chart()),
        );
        assert_eq!(
            (chart.type_(), chart.class_name()),
            (KicadT::PcbDrillChartT, "PCB_DRILL_CHART")
        );

        let mut fx = rotated_flipped_scaled();
        let id = fx.child(ItemKind::Table).typed::<PcbTable>().unwrap();
        let cell = fx.child(ItemKind::Tablecell).typed().unwrap();
        let mut kiids = KiidGenerator::default();
        let mut table = PcbTable::new(&mut kiids);
        table.col_count = 1;
        table.cells = fx.board[id].cells.clone();
        fx.set_child(id, table);
        fx.board.check_invariants().unwrap();
        assert_eq!(fx.board[id].cell(0, 0), Some(cell));
        assert_eq!(fx.board[id].row_count(), 1);
    }

    const MM: i32 = 1_000_000;

    /// A board table of `cols` x `rows` cells laid out on the given widths / heights from the
    /// origin, normalized and attached (as `TableRotates90Cleanly` builds it).
    fn grid_table(widths: &[i32], heights: &[i32]) -> (Board, TableId) {
        use crate::board::AddMode;
        use crate::item::ParentRef;

        let mut board = Board::new();
        let mut table = PcbTable::with_line_width(PCB_IU_SCALE.mm_to_iu(0.1));
        table.set_col_count(widths.len() as i32);

        for (c, &w) in widths.iter().enumerate() {
            table.set_col_width(c as i32, w);
        }

        for (r, &h) in heights.iter().enumerate() {
            table.set_row_height(r as i32, h);
        }

        let tid = board.create_table(table, ParentRef::Board);
        let env = MutEnv::detached();
        let mut y = 0;

        for &h in heights {
            let mut x = 0;

            for &w in widths {
                let mut cell = PcbTablecell::default();
                cell.set_start(&env, Vector2I::new(x, y));
                cell.set_end(&env, Vector2I::new(x + w, y + h));
                board.table_add_cell(tid, cell);
                x += w;
            }

            y += h;
        }

        board.table_normalize(tid);
        board.attach_table(tid, AddMode::Insert);
        (board, tid)
    }

    /// `TableRotates90Cleanly` (qa/tests/pcbnew/test_footprint_transform_sync.cpp).
    #[test]
    fn table_rotates_90_cleanly() {
        let (col_w, row_h) = (20 * MM, 5 * MM);
        let (mut board, tid) = grid_table(&[col_w, col_w], &[row_h, row_h]);

        board.rotate_item(
            tid.into(),
            Vector2I::new(0, 0),
            EdaAngle::from_degrees(90.0),
        );

        let ctx = BoardCtx::bare(&board);
        let mut centers = std::collections::BTreeSet::new();

        for (_, cell) in board.table_cells(tid) {
            let (s, e) = (
                cell.textbox.shape.shape.start(),
                cell.textbox.shape.shape.end(),
            );
            let (w, h) = ((e.x - s.x).abs(), (e.y - s.y).abs());
            assert!((w == col_w && h == row_h) || (w == row_h && h == col_w));

            let corners = cell.corners(&ctx);
            assert_eq!(corners.len(), 4);
            let mut visual = Box2I::default();

            for p in &corners {
                visual.merge_point(*p);
            }

            assert!((visual.width() - row_h as i64).abs() <= (MM / 5) as i64);
            assert!((visual.height() - col_w as i64).abs() <= (MM / 5) as i64);
            let vc = visual.center();
            centers.insert((vc.x, vc.y));
        }

        assert_eq!(centers.len(), 4, "cells overlapped");
    }

    /// `PCB_TABLE::Normalize` on a laid-out grid moves nothing (`checkNormalizeIsStable`),
    /// spans included, and the grid geometry getters (`GetEnd`, `GetBoundingBox`, borders,
    /// row / column lookups) follow the C++ bodies.
    #[test]
    fn grid_getters_and_stable_normalize() {
        let (mut board, tid) = grid_table(&[8 * MM, 4 * MM, 6 * MM], &[2 * MM, 3 * MM]);
        let before: Vec<(Vector2I, Vector2I)> = board
            .table_cells(tid)
            .map(|(_, c)| (c.textbox.shape.shape.start(), c.textbox.shape.shape.end()))
            .collect();
        board.table_normalize(tid);
        let after: Vec<(Vector2I, Vector2I)> = board
            .table_cells(tid)
            .map(|(_, c)| (c.textbox.shape.shape.start(), c.textbox.shape.shape.end()))
            .collect();
        assert_eq!(before, after);

        let ctx = BoardCtx::bare(&board);
        let t = &board[tid];
        assert_eq!(t.position(&ctx), Vector2I::new(0, 0));
        assert_eq!(t.end(&ctx), Vector2I::new(18 * MM, 5 * MM));
        // Cells inflated by half the 0.1 mm cell stroke, then the border half width.
        let bbox = t.bounding_box(&ctx);
        assert_eq!(
            (bbox.left(), bbox.top(), bbox.right(), bbox.bottom()),
            (-100_000, -100_000, 18 * MM + 100_000, 5 * MM + 100_000)
        );

        let mut segments = Vec::new();
        t.draw_borders(&ctx, &mut |a, b, stroke| {
            segments.push((a, b, stroke.width()))
        });
        // 2 column separators x 2 rows, 1 row separator x 3 columns, 4 border sides.
        assert_eq!(segments.len(), 4 + 3 + 4);
        assert_eq!(segments[0].0, Vector2I::new(8 * MM, 0));
        assert_eq!(segments[0].1, Vector2I::new(8 * MM, 2 * MM));

        let cell = t.cell(1, 2).unwrap();
        let c = &board[cell];
        assert_eq!((c.row(&ctx), c.column(&ctx)), (1, 2));
        assert_eq!(c.addr(&ctx), "C2");
        assert_eq!((c.column_width(&ctx), c.row_height(&ctx)), (6 * MM, 3 * MM));
        assert_eq!(
            t.item_description(
                &ctx,
                &UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm),
                false
            ),
            "3 column table"
        );
        assert_eq!(PcbTable::compare(&ctx, t, t), 0);
    }
}

/// Package M11 differential oracle replay: the text boxes and tables of
/// `tests/oracle/m11_oracle.cpp` built the same way, one mutator per case, compared value by
/// value with `tests/golden/m11_boxes_tables.txt` (regenerate with
/// `tests/oracle/m11_golden.sh`).
#[cfg(all(test, feature = "dump"))]
mod oracle_tests {
    use std::collections::BTreeMap;

    use kicad_core::PcbLayerId;
    use kicad_core::text_vars::ResolutionContext;
    use kicad_geom::geometry::approximation::ErrorLoc;
    use kicad_geom::geometry::transform_trs::TransformTrs;
    use kicad_geom::{EdaAngle, FlipDirection, Vector2I};

    use super::*;
    use crate::board::AddMode;
    use crate::dump::fnv1a64;
    use crate::dump::format::{
        v_angle, v_bool, v_box, v_int, v_points, v_polyset, v_shape, v_str, v_vec,
    };
    use crate::ids::ItemId;
    use crate::item::ParentRef;
    use crate::pcb_textbox::{PcbTextbox, TextboxHost};
    use crate::test_support::board_with_footprint;
    use crate::traits::{BoardItem, EdaShapeItem};

    const GOLDEN: &str = include_str!("../tests/golden/m11_boxes_tables.txt");
    const MM: i32 = 1_000_000;
    const WIDTHS: [i32; 3] = [8 * MM, 4 * MM, 6 * MM];
    const HEIGHTS: [i32; 2] = [2 * MM, 3 * MM];
    const TEXTS: [[&str; 3]; 2] = [
        ["Hdr", "Second cell text that wraps", "C"],
        ["row2", "${ROW}:${COL} ${ADDR}", "x"],
    ];
    const PIVOT: Vector2I = Vector2I::new(MM, MM);
    const FLIP_CENTRE: Vector2I = Vector2I::new(10 * MM, 10 * MM);

    fn h(text: &str) -> String {
        format!("#{:016x}", fnv1a64(text.as_bytes()))
    }

    /// `DumpBox` of the C++ driver.
    fn dump_box<H: TextboxHost>(
        out: &mut BTreeMap<String, String>,
        c: &str,
        k: &str,
        ctx: &BoardCtx<'_>,
        tb: &H,
        cell: Option<&PcbTablecell>,
    ) {
        let mut p = |f: &str, v: String| {
            out.insert(format!("{c}|{k}|{f}"), v);
        };
        let b = tb.textbox();
        let es = &b.shape.shape;
        let layer = BoardItem::layer(tb, ctx);

        p("shape", v_int(es.shape() as i32));
        p("start", v_vec(es.start()));
        p("end", v_vec(es.end()));
        p("lib_shape", v_int(b.shape.lib_shape() as i32));
        p("lib_start", v_vec(b.shape.lib_start()));
        p("lib_end", v_vec(b.shape.lib_end()));

        if es.shape() == crate::eda_shape::ShapeT::Poly {
            p("poly", h(&v_polyset(es.poly_shape())));
        }

        p("layer", v_int(layer.0));
        p("mirrored", v_bool(tb.eda_text().is_mirrored()));
        p("text_angle", v_angle(tb.text_angle(ctx)));
        p("lib_text_angle", v_angle(b.lib_text_angle()));
        p(
            "attr_angle",
            v_angle(EdaAngle::from_degrees(tb.eda_text().attributes.angle)),
        );
        p("text_size", v_vec(tb.text_size(ctx)));
        p("text_thickness", v_int(tb.text_thickness(ctx)));
        p("draw_pos", v_vec(tb.draw_pos(ctx)));
        p(
            "draw_pos_flipped",
            v_vec(crate::pcb_textbox::draw_pos_flipped(tb, ctx, true)),
        );
        p("draw_rotation", v_angle(tb.draw_rotation(ctx)));
        p("corners", v_points(&crate::pcb_textbox::corners(tb, ctx)));
        p("top_left", v_vec(EdaShapeItem::top_left(tb, ctx)));
        p("bot_right", v_vec(EdaShapeItem::bot_right(tb, ctx)));
        p("bbox", v_box(BoardItem::bounding_box(tb, ctx)));
        p("center", v_vec(BoardItem::center(tb, ctx)));
        p("position", v_vec(BoardItem::position(tb, ctx)));
        p("legacy_margin", v_int(b.legacy_text_margin(ctx)));
        p("min_size", v_vec(crate::pcb_textbox::min_size(tb, ctx)));
        p(
            "shown_text",
            v_str(&tb.shown_text(ctx, ResolutionContext::Resolved, 0)),
        );
        p(
            "hit_center",
            v_bool(BoardItem::hit_test(tb, ctx, BoardItem::center(tb, ctx), 0)),
        );

        let eff = BoardItem::effective_shape(
            tb,
            ctx,
            layer,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        p("effective_shape", h(&v_shape(Some(eff.as_ref()))));

        let mut text = ShapePolySet::new();
        crate::pcb_textbox::transform_text_to_poly_set(
            tb,
            ctx,
            &mut text,
            0,
            5000,
            ErrorLoc::Inside,
        );
        p("text_polys", h(&v_polyset(&text)));

        let mut bx = ShapePolySet::new();
        BoardItem::transform_shape_to_polygon(
            tb,
            ctx,
            &mut bx,
            layer,
            0,
            5000,
            ErrorLoc::Inside,
            false,
        );
        p("tpoly", h(&v_polyset(&bx)));

        let mut bx_out = ShapePolySet::new();
        BoardItem::transform_shape_to_polygon(
            tb,
            ctx,
            &mut bx_out,
            layer,
            100_000,
            5000,
            ErrorLoc::Outside,
            false,
        );
        p("tpoly_out", h(&v_polyset(&bx_out)));

        if let Some(cell) = cell {
            p("row_col", format!("{},{}", cell.row(ctx), cell.column(ctx)));
            p("addr", v_str(&cell.addr(ctx)));
            p("col_width", v_int(cell.column_width(ctx)));
            p("row_height", v_int(cell.row_height(ctx)));
        }
    }

    /// `DumpTable` of the C++ driver.
    fn dump_table(out: &mut BTreeMap<String, String>, c: &str, board: &Board, tid: TableId) {
        let ctx = BoardCtx::bare(board);
        let t = &board[tid];
        let layer = t.base.layer;
        {
            let mut p = |f: &str, v: String| {
                out.insert(format!("{c}|table|{f}"), v);
            };

            p("layer", v_int(layer.0));
            p("position", v_vec(t.position(&ctx)));
            p("end", v_vec(t.end(&ctx)));
            p("bbox", v_box(t.bounding_box(&ctx)));
            let widths: Vec<String> = (0..t.col_count()).map(|c| v_int(t.col_width(c))).collect();
            p("col_widths", widths.join(" "));
            let heights: Vec<String> = (0..t.row_count()).map(|r| v_int(t.row_height(r))).collect();
            p("row_heights", heights.join(" "));
            let order: Vec<String> = board
                .table_cells(tid)
                .map(|(_, cell)| v_str(cell.textbox.text.text()))
                .collect();
            p("cell_order", order.join(" "));

            let mut borders: Vec<String> = Vec::new();
            t.draw_borders(&ctx, &mut |a, b, stroke| {
                borders.push(format!(
                    "{}>{}/{}",
                    v_vec(a),
                    v_vec(b),
                    v_int(stroke.width())
                ));
            });
            p("borders", h(&borders.join(" ")));

            let eff = t.effective_shape(
                &ctx,
                layer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            p("effective_shape", h(&v_shape(Some(eff.as_ref()))));

            let mut poly = ShapePolySet::new();
            t.transform_shape_to_polygon(&ctx, &mut poly, layer, 0, 5000, ErrorLoc::Inside, false);
            p("tpoly", h(&v_polyset(&poly)));

            let mut set0 = ShapePolySet::new();
            t.transform_shape_to_poly_set(&ctx, &mut set0, layer, 0, 5000, ErrorLoc::Inside);
            p("tpolyset", h(&v_polyset(&set0)));

            let mut set1 = ShapePolySet::new();
            t.transform_shape_to_poly_set(&ctx, &mut set1, layer, 100_000, 5000, ErrorLoc::Outside);
            p("tpolyset_out", h(&v_polyset(&set1)));

            p("hit_pos", v_bool(t.hit_test(&ctx, t.position(&ctx), 0)));
        }

        for (ii, (_, cell)) in board.table_cells(tid).enumerate() {
            dump_box(out, c, &format!("cell{ii}"), &ctx, cell, Some(cell));
        }
    }

    /// `MakeFootprint`: at (10 mm, 20 mm), rotated 30 degrees, scaled (1.5, 0.75).
    fn make_board(in_fp: bool) -> (Board, ParentRef) {
        if in_fp {
            let fx = board_with_footprint(
                TransformTrs::new(
                    Vector2I::new(10 * MM, 20 * MM),
                    EdaAngle::from_degrees(30.0),
                    1.5,
                    0.75,
                ),
                false,
            );
            let parent = ParentRef::Item(fx.footprint.into());
            (fx.board, parent)
        } else {
            (Board::new(), ParentRef::Board)
        }
    }

    /// `MakeTable` of the C++ driver.
    fn make_table(board: &mut Board, parent: ParentRef, origin: Vector2I, spans: bool) -> TableId {
        let mut t = PcbTable::with_line_width(PCB_IU_SCALE.mm_to_iu(0.1));
        t.base.layer = PcbLayerId::FSilkS;
        t.set_col_count(3);

        for (col, &w) in WIDTHS.iter().enumerate() {
            t.set_col_width(col as i32, w);
        }

        for (row, &hh) in HEIGHTS.iter().enumerate() {
            t.set_row_height(row as i32, hh);
        }

        let tid = board.create_table(t, parent);
        let env = board.child_env(ParentRef::Item(tid.into()));
        let mut y = origin.y;

        for row in 0..2 {
            let mut x = origin.x;

            for col in 0..3 {
                let mut cell = PcbTablecell::default();
                cell.textbox.shape.base.item.parent = ParentRef::Item(tid.into());
                let tb = &mut cell.textbox;
                (
                    tb.margin_left,
                    tb.margin_top,
                    tb.margin_right,
                    tb.margin_bottom,
                ) = (MM / 2, MM / 4, MM / 2, MM / 4);
                cell.set_text_size(&env, Vector2I::new(MM, MM), true);
                cell.set_start(&env, Vector2I::new(x, y));
                cell.set_end(&env, Vector2I::new(x + WIDTHS[col], y + HEIGHTS[row]));
                cell.textbox.text.set_text(TEXTS[row][col]);
                board.table_add_cell(tid, cell);
                x += WIDTHS[col];
            }

            y += HEIGHTS[row];
        }

        if spans {
            let c00 = board[tid].cell(0, 0).unwrap();
            let c01 = board[tid].cell(0, 1).unwrap();
            board.modify_tablecell(c00, |c, _| c.set_col_span(2));
            board.modify_tablecell(c01, |c, _| {
                c.set_col_span(0);
                c.set_row_span(0);
            });
        }

        board.table_normalize(tid);
        tid
    }

    /// `MakeTextbox` of the C++ driver.
    fn make_textbox(board: &Board, parent: ParentRef, origin: Vector2I) -> PcbTextbox {
        let env = board.child_env(parent);
        let mut tb = PcbTextbox::default();
        tb.shape.base.item.parent = parent;
        tb.set_layer(&env, PcbLayerId::FSilkS);
        (
            tb.margin_left,
            tb.margin_top,
            tb.margin_right,
            tb.margin_bottom,
        ) = (MM / 2, MM / 3, MM / 4, MM / 5);
        tb.set_text_size(&env, Vector2I::new(MM, 3 * MM / 2), true);
        tb.set_text_thickness(&env, 150_000);
        tb.set_start(&env, origin);
        tb.set_end(&env, origin + Vector2I::new(20 * MM, 5 * MM));
        tb.text.set_text("Hello World wrapping text in a box");
        tb
    }

    /// The common mutators of `Ops()`; `false` for a name that is not one of them.
    fn common_op(board: &mut Board, id: ItemId, name: &str) -> bool {
        match name {
            "none" => {}
            "rot30" => board.rotate_item(id, PIVOT, EdaAngle::from_degrees(30.0)),
            "rot90" => board.rotate_item(id, PIVOT, EdaAngle::from_degrees(90.0)),
            "flipLR" => board.flip_item(id, FLIP_CENTRE, FlipDirection::LeftRight),
            "flipTB" => board.flip_item(id, FLIP_CENTRE, FlipDirection::TopBottom),
            "mirLR" => board.mirror_item(id, FLIP_CENTRE, FlipDirection::LeftRight),
            "mirTB" => board.mirror_item(id, FLIP_CENTRE, FlipDirection::TopBottom),
            "rot30flipLR" => {
                board.rotate_item(id, PIVOT, EdaAngle::from_degrees(30.0));
                board.flip_item(id, FLIP_CENTRE, FlipDirection::LeftRight);
            }
            "move" => board.move_item(id, Vector2I::new(1_234_567, -765_432)),
            _ => return false,
        }

        true
    }

    const COMMON_OPS: [&str; 9] = [
        "none",
        "rot30",
        "rot90",
        "flipLR",
        "flipTB",
        "mirLR",
        "mirTB",
        "rot30flipLR",
        "move",
    ];

    #[test]
    fn m11_matches_oracle() {
        let mut golden: BTreeMap<String, String> = BTreeMap::new();

        for line in GOLDEN.lines() {
            if let Some((k, v)) = line.split_once('=') {
                golden.insert(k.to_string(), v.to_string());
            }
        }

        let mut got: BTreeMap<String, String> = BTreeMap::new();

        for in_fp in [false, true] {
            let origin = if in_fp {
                Vector2I::new(11 * MM, 21 * MM)
            } else {
                Vector2I::new(2 * MM, 3 * MM)
            };

            for spans in [false, true] {
                for name in COMMON_OPS.iter().copied().chain(["autosize"]) {
                    let (mut board, parent) = make_board(in_fp);
                    let tid = make_table(&mut board, parent, origin, spans);
                    board.attach_table(tid, AddMode::Append);

                    if !common_op(&mut board, tid.into(), name) {
                        board.table_autosize(tid);
                    }

                    let c = format!(
                        "table{}{}_{name}",
                        if in_fp { "_fp" } else { "" },
                        if spans { "_span" } else { "" }
                    );
                    dump_table(&mut got, &c, &board, tid);
                }
            }

            for name in COMMON_OPS
                .iter()
                .copied()
                .chain(["angle45", "rot30normalize"])
            {
                let (mut board, parent) = make_board(in_fp);
                let tb = make_textbox(&board, parent, origin);
                let id: ItemId = if in_fp {
                    let fp = parent.item().unwrap().typed().unwrap();
                    board
                        .footprint_add(
                            fp,
                            crate::item_ref::OwnedItem::Textbox(Box::new(tb)),
                            AddMode::Append,
                        )
                        .unwrap_or_else(|_| panic!("footprint_add"))
                } else {
                    board.add_textbox(tb, AddMode::Append).into()
                };
                let tid = id.typed::<PcbTextbox>().unwrap();

                if !common_op(&mut board, id, name) {
                    if name == "angle45" {
                        board.modify_textbox(tid, |t, env| {
                            t.set_text_angle(env, EdaAngle::from_degrees(45.0))
                        });
                    } else {
                        board.rotate_item(id, PIVOT, EdaAngle::from_degrees(30.0));
                        board.modify_textbox(tid, |t, env| t.normalize_env(env));
                    }
                }

                let c = format!("textbox{}_{name}", if in_fp { "_fp" } else { "" });
                let ctx = BoardCtx::bare(&board);
                dump_box(&mut got, &c, "tb", &ctx, &board[tid], None);
            }
        }

        let mut failures = Vec::new();

        for (key, want) in &golden {
            match got.get(key) {
                Some(have) if have == want => {}
                Some(have) => failures.push(format!("{key}: want {want}, have {have}")),
                None => failures.push(format!("{key}: missing (want {want})")),
            }
        }

        for key in got.keys() {
            if !golden.contains_key(key) {
                failures.push(format!("{key}: extra"));
            }
        }

        assert!(
            failures.is_empty(),
            "{} of {} values differ:\n{}",
            failures.len(),
            golden.len(),
            failures[..failures.len().min(60)].join("\n")
        );
    }
}
