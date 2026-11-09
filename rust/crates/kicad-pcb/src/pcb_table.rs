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
//! M0c), the overrides (stubs for package M11). A table is a container: its mutating virtuals
//! cascade to the cells through `Board` methods (`board-model.md` 8.3), so it has no
//! `BoardItemMut` impl. The overrides of the generated-table classes are reached through the
//! same inherent methods, which test `generated` (M26).

use std::collections::BTreeMap;

use kicad_core::color4d::Color4D;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, LineStyle, PCB_IU_SCALE, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, ShapePolySet, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::ids::TablecellId;
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::pcb_generated_table::{GeneratedTableData, GeneratedTableKind};
use crate::pcb_shape::DEFAULT_LINE_WIDTH;
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

    /// `setAutosizedRowHeights( aNeeded )` (virtual, `PCB_GENERATED_TABLE` overrides).
    pub fn set_autosized_row_heights(&mut self, needed: &[i32]) {
        crate::port_pending!("PCB_TABLE::setAutosizedRowHeights", needed)
    }

    /// `Autosize()`.
    pub fn autosize(&mut self, ctx: &BoardCtx<'_>) {
        crate::port_pending!("PCB_TABLE::Autosize", ctx)
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
    /// `PCB_TABLE::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TABLE::GetBoundingBox", ctx)
    }

    /// `PCB_TABLE::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_TABLE::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TABLE::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_TABLE::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_TABLE::HitTest`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_TABLE::HitTest", ctx, rect, contained, accuracy)
    }

    /// `PCB_TABLE::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TABLE::GetItemDescription", ctx, units, full)
    }

    /// `PCB_TABLE::GetPosition`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_TABLE::GetPosition", ctx)
    }

    /// `PCB_TABLE::RunOnChildren`.
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        crate::port_pending!("PCB_TABLE::RunOnChildren", ctx, mode, f)
    }

    /// `PCB_TABLE::TransformShapeToPolySet`.
    pub fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        crate::port_pending!(
            "PCB_TABLE::TransformShapeToPolySet",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc
        )
    }

    /// `PCB_TABLE::TransformShapeToPolygon`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PCB_TABLE::TransformShapeToPolygon",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
