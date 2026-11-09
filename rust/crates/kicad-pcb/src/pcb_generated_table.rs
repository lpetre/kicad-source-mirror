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

//! Data of `pcbnew/pcb_generated_table.h`, `pcb_drill_chart.h`, `pcb_stackup_table.h`,
//! `board_tables/generated_table_schema.h` and `drill/drill_chart_template.h`
//! (`DRILL_CHART_FILTER`), held by `PcbTable::generated` (types: package M0c). Refreshing the
//! cells (`RefreshGeneratedTables`, templates, schemas) is tier 2 (packages M11 / M26).
//!
//! Dropped from `PCB_DRILL_CHART`: `m_laidOutShowTotals` (set only between an edit and the next
//! rebuild) and `m_builtGeneration` (refresh bookkeeping); both are transient UI state.

use std::collections::BTreeMap;

/// `GENERATED_TABLE_ALIGN`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum GeneratedTableAlign {
    #[default]
    Left = 0,
    Center,
    Right,
}

/// `GENERATED_TABLE_UNITS`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum GeneratedTableUnits {
    #[default]
    Mm = 0,
    Inch,
    Mils,
}

/// `GENERATED_TABLE_COLUMN` (`board_tables/generated_table_schema.h`).
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTableColumn {
    pub id: i32,                    // m_Id
    pub heading: String,            // m_Heading
    pub align: GeneratedTableAlign, // m_Align
    /// m_Width: minimum width, 0 = autosize.
    pub width: i32,
    pub visible: bool, // m_Visible
}

impl Default for GeneratedTableColumn {
    /// The in-class initialisers: id 0, empty heading, left, autosize, visible.
    fn default() -> Self {
        GeneratedTableColumn {
            id: 0,
            heading: String::new(),
            align: GeneratedTableAlign::Left,
            width: 0,
            visible: true,
        }
    }
}

/// `PCB_GENERATED_TABLE::HIDDEN_COLUMN`: a hidden column and its place among all columns.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GeneratedTableHiddenColumn {
    pub column: GeneratedTableColumn, // m_Column
    /// m_Position: index in `AllColumns()`.
    pub position: i32,
}

/// `DRILL_CHART_FILTER`; all default true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillChartFilter {
    pub plated: bool,      // m_Plated
    pub non_plated: bool,  // m_NonPlated
    pub vias: bool,        // m_Vias
    pub slots: bool,       // m_Slots
    pub backdrills: bool,  // m_Backdrills
    pub castellated: bool, // m_Castellated
}

impl Default for DrillChartFilter {
    fn default() -> Self {
        DrillChartFilter {
            plated: true,
            non_plated: true,
            vias: true,
            slots: true,
            backdrills: true,
            castellated: true,
        }
    }
}

/// The `PCB_GENERATED_TABLE` members (`pcbnew/pcb_generated_table.h`). The cells are ordinary
/// `PcbTablecell`s read from the file.
#[derive(Clone, Debug)]
pub struct GeneratedTableData {
    /// m_columns: the shown columns.
    pub columns: Vec<GeneratedTableColumn>,
    pub units: GeneratedTableUnits,      // m_units
    pub precision: i32,                  // m_precision
    pub row_keys: BTreeMap<i32, String>, // m_rowKeys
    /// m_hidden: hidden columns by position (written with the shown ones, `AllColumns()`).
    pub hidden: Vec<GeneratedTableHiddenColumn>,
    pub kind: GeneratedTableKind,
}

/// The concrete generated-table class.
#[derive(Clone, Debug)]
pub enum GeneratedTableKind {
    /// `PCB_DRILL_CHART_T` (`pcbnew/pcb_drill_chart.h`).
    DrillChart {
        filter: DrillChartFilter,       // m_filter
        show_totals: bool,              // m_showTotals
        row_shapes: BTreeMap<i32, i32>, // m_rowShapes
        symbol_column: i32,             // m_symbolColumn
    },
    /// `PCB_STACKUP_TABLE_T` (no own members).
    StackupTable,
}

impl GeneratedTableKind {
    /// The `PCB_DRILL_CHART` member initialisers: default filter, totals shown, no row shapes,
    /// no symbol column.
    pub fn drill_chart() -> Self {
        GeneratedTableKind::DrillChart {
            filter: DrillChartFilter::default(),
            show_totals: true,
            row_shapes: BTreeMap::new(),
            symbol_column: -1,
        }
    }
}

impl GeneratedTableData {
    /// `PCB_GENERATED_TABLE( aParent, aType, aLineWidth )`: no columns, mm, precision 3.
    pub fn new(kind: GeneratedTableKind) -> Self {
        GeneratedTableData {
            columns: Vec::new(),
            units: GeneratedTableUnits::Mm,
            precision: 3,
            row_keys: BTreeMap::new(),
            hidden: Vec::new(),
            kind,
        }
    }

    /// `PCB_DRILL_CHART( aParent )`: the base values, then
    /// `ApplyTemplate( DRILL_CHART_TEMPLATE::MakeDefault() )`.
    pub fn new_drill_chart() -> Self {
        crate::port_pending!("PCB_DRILL_CHART::PCB_DRILL_CHART")
    }

    /// `PCB_STACKUP_TABLE( aParent )`: the schema's default columns, units and precision.
    pub fn new_stackup_table() -> Self {
        crate::port_pending!("PCB_STACKUP_TABLE::PCB_STACKUP_TABLE")
    }

    /// `PCB_GENERATED_TABLE::AllColumns()`: shown and hidden columns in their order.
    pub fn all_columns(&self) -> Vec<GeneratedTableColumn> {
        crate::port_pending!("PCB_GENERATED_TABLE::AllColumns")
    }

    /// `PCB_GENERATED_TABLE::RestoreColumns( aColumns )`.
    pub fn restore_columns(&mut self, columns: &[GeneratedTableColumn]) {
        crate::port_pending!("PCB_GENERATED_TABLE::RestoreColumns", columns)
    }

    /// `PCB_GENERATED_TABLE::SetPrecision()`: clamped to `[0, GENERATED_TABLE_MAX_PRECISION]`.
    pub fn set_precision(&mut self, precision: i32) {
        crate::port_pending!("PCB_GENERATED_TABLE::SetPrecision", precision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PCB_GENERATED_TABLE( .. )`, `PCB_DRILL_CHART` initialisers, `DRILL_CHART_FILTER`,
    /// `GENERATED_TABLE_COLUMN` (pcb_generated_table.cpp, pcb_drill_chart.cpp, headers).
    #[test]
    fn constructor_values() {
        let d = GeneratedTableData::new(GeneratedTableKind::drill_chart());
        assert_eq!((d.units, d.precision), (GeneratedTableUnits::Mm, 3));
        assert!(d.columns.is_empty() && d.hidden.is_empty() && d.row_keys.is_empty());
        let GeneratedTableKind::DrillChart {
            filter,
            show_totals,
            symbol_column,
            ..
        } = &d.kind
        else {
            panic!()
        };
        assert!(*show_totals && *symbol_column == -1);
        assert!(filter.plated && filter.non_plated && filter.vias && filter.slots);
        assert!(filter.backdrills && filter.castellated);
        let c = GeneratedTableColumn::default();
        assert_eq!(
            (c.id, c.width, c.visible, c.align),
            (0, 0, true, GeneratedTableAlign::Left)
        );
    }
}
