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

//! Port of `pcbnew/drill/drill_chart_template.h` / `.cpp`: `DRILL_CHART_TEMPLATE`, the column
//! set and formatting a new chart starts from. `DRILL_CHART_COLUMN_ID`, `DrillChartSchema()`
//! and `DRILL_CHART_FILTER` are in [`crate::pcb_generated_table`] (package M11).
//!
//! `SaveToFile` / `LoadFromFile` (a standalone JSON template file shared between projects, an
//! editor dialog feature) are not ported: see PORT_STATUS.md, package M26.

pub use crate::pcb_generated_table::{DrillChartColumnId, DrillChartFilter, drill_chart_schema};
use crate::pcb_generated_table::{GeneratedTableColumn, GeneratedTableUnits};

/// `DRILL_CHART_TEMPLATE`. A chart copies a template rather than linking to it, so editing or
/// importing a template cannot silently rewrite a fabrication drawing that has already been
/// approved.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillChartTemplate {
    /// m_name: identifies the template file to whoever opens it.
    pub name: String,
    pub version: i32,                       // m_version
    pub columns: Vec<GeneratedTableColumn>, // m_columns
    pub units: GeneratedTableUnits,         // m_units
    pub precision: i32,                     // m_precision
    pub show_totals: bool,                  // m_showTotals
}

impl Default for DrillChartTemplate {
    /// Port of `DRILL_CHART_TEMPLATE()`: version 1, the schema's units and precision, totals
    /// shown, no columns.
    fn default() -> Self {
        let schema = drill_chart_schema();
        DrillChartTemplate {
            name: String::new(),
            version: 1,
            columns: Vec::new(),
            units: schema.default_units,
            precision: schema.default_precision,
            show_totals: true,
        }
    }
}

impl DrillChartTemplate {
    /// Port of `DRILL_CHART_TEMPLATE::MakeDefault()`: "Default" with the schema's default
    /// columns.
    pub fn make_default() -> Self {
        DrillChartTemplate {
            name: "Default".to_string(),
            columns: drill_chart_schema().defaults(),
            ..DrillChartTemplate::default()
        }
    }
}
