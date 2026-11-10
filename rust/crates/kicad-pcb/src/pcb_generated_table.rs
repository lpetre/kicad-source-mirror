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
//! cells (`RefreshGeneratedTables`, templates, row models) is tier 2 (package M26); the column
//! schemas and the constructors' defaults are here (package M11).
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

    /// Port of `PCB_DRILL_CHART( aParent )`: the base values, then
    /// `ApplyTemplate( DRILL_CHART_TEMPLATE::MakeDefault() )` (the schema's default columns,
    /// units and precision, totals shown).
    pub fn new_drill_chart() -> Self {
        let mut data = GeneratedTableData::new(GeneratedTableKind::drill_chart());
        let schema = drill_chart_schema();

        // DRILL_CHART_TEMPLATE::MakeDefault(): the constructor's units / precision / totals
        // and the default columns; PCB_DRILL_CHART::ApplyTemplate copies them.
        data.columns = schema.defaults();
        data.units = schema.default_units;
        data.precision = schema.default_precision;

        if let GeneratedTableKind::DrillChart { show_totals, .. } = &mut data.kind {
            *show_totals = true;
        }

        data
    }

    /// Port of `PCB_STACKUP_TABLE( aParent )`: the schema's default columns, units and
    /// precision.
    pub fn new_stackup_table() -> Self {
        let mut data = GeneratedTableData::new(GeneratedTableKind::StackupTable);
        let schema = stackup_table_schema();

        data.columns = schema.defaults();
        data.units = schema.default_units;
        data.precision = schema.default_precision;

        data
    }

    /// Port of `PCB_GENERATED_TABLE::AllColumns()`: shown and hidden columns in their order.
    pub fn all_columns(&self) -> Vec<GeneratedTableColumn> {
        let mut all = Vec::new();
        let total = self.columns.len() + self.hidden.len();
        let mut shown = 0usize;
        let mut hidden = 0usize;

        for pos in 0..total {
            let hidden_here = hidden < self.hidden.len()
                && (self.hidden[hidden].position == pos as i32 || shown == self.columns.len());

            if hidden_here {
                all.push(self.hidden[hidden].column.clone());
                hidden += 1;
            } else {
                all.push(self.columns[shown].clone());
                shown += 1;
            }
        }

        all
    }

    /// Port of `PCB_GENERATED_TABLE::RestoreColumns( aColumns )`: the visible columns are
    /// shown, the others kept as hidden with their position.
    pub fn restore_columns(&mut self, columns: &[GeneratedTableColumn]) {
        self.columns.clear();
        self.hidden.clear();

        for (pos, col) in columns.iter().enumerate() {
            if col.visible {
                self.columns.push(col.clone());
                continue;
            }

            self.hidden.push(GeneratedTableHiddenColumn {
                column: col.clone(),
                position: pos as i32,
            });
        }
    }

    /// `PCB_GENERATED_TABLE::SetPrecision()`: clamped to `[0, GENERATED_TABLE_MAX_PRECISION]`.
    pub fn set_precision(&mut self, precision: i32) {
        self.precision = precision.clamp(0, GENERATED_TABLE_MAX_PRECISION);
    }
}

// ---- board_tables/generated_table_schema.{h,cpp} (data) ----------------------------------

/// `GENERATED_TABLE_MAX_COLUMN_WIDTH`.
pub const GENERATED_TABLE_MAX_COLUMN_WIDTH: i32 = 1000 * 1_000_000;

/// `GENERATED_TABLE_MAX_PRECISION`: the most digits a generated table prints after the
/// decimal point.
pub const GENERATED_TABLE_MAX_PRECISION: i32 = 6;

/// `GENERATED_TABLE_MAX_TOTAL_WIDTH`.
pub const GENERATED_TABLE_MAX_TOTAL_WIDTH: i64 = 10 * 1000 * 1_000_000;

/// `GENERATED_TABLE_COLUMN_DEF`: one column a schema knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratedTableColumnDef {
    pub id: i32, // m_Id
    /// m_Token: file and JSON token, never translated.
    pub token: &'static str,
    /// m_Heading: the (untranslated) heading.
    pub heading: &'static str,
    pub align: GeneratedTableAlign, // m_Align
    /// m_DefaultShown: part of `Defaults()`.
    pub default_shown: bool,
    /// m_Label: UI name, the heading when `None`.
    pub label: Option<&'static str>,
}

/// `GENERATED_TABLE_SCHEMA`: the columns a generated table kind offers, with its default
/// units and precision.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTableSchema {
    pub defs: Vec<GeneratedTableColumnDef>, // m_defs
    pub default_units: GeneratedTableUnits, // m_defaultUnits
    pub default_precision: i32,             // m_defaultPrecision
}

impl GeneratedTableSchema {
    /// `Find( aId )`.
    pub fn find(&self, id: i32) -> Option<&GeneratedTableColumnDef> {
        self.defs.iter().find(|def| def.id == id)
    }

    /// `FindToken( aToken )` (case-sensitive `IsSameAs`).
    pub fn find_token(&self, token: &str) -> Option<&GeneratedTableColumnDef> {
        self.defs.iter().find(|def| def.token == token)
    }

    /// `Token( aId )`: the column's token, the first column's for an unknown id.
    pub fn token(&self, id: i32) -> &'static str {
        match self.find(id) {
            Some(def) => def.token,
            None => self.defs[0].token,
        }
    }

    /// `DefaultColumn( aId )`: id, heading (`wxGetTranslation`: untranslated here) and
    /// alignment of the definition, the member defaults otherwise.
    pub fn default_column(&self, id: i32) -> GeneratedTableColumn {
        let mut col = GeneratedTableColumn {
            id,
            ..GeneratedTableColumn::default()
        };

        if let Some(def) = self.find(id) {
            col.heading = def.heading.to_string();
            col.align = def.align;
        }

        col
    }

    /// `Defaults()`: the default-shown columns in schema order.
    pub fn defaults(&self) -> Vec<GeneratedTableColumn> {
        self.defs
            .iter()
            .filter(|def| def.default_shown)
            .map(|def| self.default_column(def.id))
            .collect()
    }

    /// `Validate( aColumns )`: drops columns unknown to the schema and repeats, clamps the
    /// widths; false when nothing is left or the widths add up to more than
    /// `GENERATED_TABLE_MAX_TOTAL_WIDTH`.
    pub fn validate(&self, columns: &mut Vec<GeneratedTableColumn>) -> bool {
        let mut kept = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut total: i64 = 0;

        for col in columns.iter_mut() {
            // Unknown to this schema, or a repeat that adds no information and multiplies the
            // generated cell count either way
            if self.find(col.id).is_none() || !seen.insert(col.id) {
                continue;
            }

            col.width = col.width.clamp(0, GENERATED_TABLE_MAX_COLUMN_WIDTH);
            total += col.width as i64;
            kept.push(col.clone());
        }

        *columns = kept;

        !columns.is_empty() && total <= GENERATED_TABLE_MAX_TOTAL_WIDTH
    }
}

/// `DRILL_CHART_COLUMN_ID` (`drill/drill_chart_template.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DrillChartColumnId {
    Symbol = 0,
    DrillDiameter,
    SlotSize,
    Plating,
    OperationCount,
    SiteCount,
    LayerSpan,
    Operation,
    Protection,
    BackdrillStub,
    AspectRatio,
    Description,
}

/// `STACKUP_TABLE_COLUMN_ID` (`board_tables/stackup_table_model.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum StackupTableColumnId {
    LayerName = 0,
    Type,
    Material,
    Thickness,
    Color,
    EpsilonR,
    LossTangent,
}

/// Shorthand for a `GENERATED_TABLE_COLUMN_DEF` initialiser.
const fn def(
    id: i32,
    token: &'static str,
    heading: &'static str,
    align: GeneratedTableAlign,
    default_shown: bool,
    label: &'static str,
) -> GeneratedTableColumnDef {
    GeneratedTableColumnDef {
        id,
        token,
        heading,
        align,
        default_shown,
        label: Some(label),
    }
}

/// Port of `DrillChartSchema()` (`drill/drill_chart_template.cpp`). Headings are the
/// untranslated `_HKI()` literals, as written to a fresh chart.
pub fn drill_chart_schema() -> GeneratedTableSchema {
    use DrillChartColumnId as C;
    use GeneratedTableAlign::{Center, Left, Right};

    GeneratedTableSchema {
        defs: vec![
            def(C::Symbol as i32, "symbol", "SYM", Center, true, "Symbol"),
            def(
                C::DrillDiameter as i32,
                "drill_diameter",
                "DRILL DIA",
                Right,
                true,
                "Drill diameter",
            ),
            def(
                C::SlotSize as i32,
                "slot_size",
                "SLOT W x L",
                Right,
                true,
                "Slot size",
            ),
            def(
                C::Plating as i32,
                "plating",
                "PLATED",
                Center,
                true,
                "Plating",
            ),
            def(
                C::OperationCount as i32,
                "operation_count",
                "OPS",
                Right,
                true,
                "Operation count",
            ),
            def(
                C::SiteCount as i32,
                "site_count",
                "SITE COUNT",
                Right,
                false,
                "Site count",
            ),
            def(
                C::LayerSpan as i32,
                "layer_span",
                "FROM / TO",
                Center,
                true,
                "Layer span",
            ),
            def(
                C::Operation as i32,
                "operation",
                "OPERATION",
                Left,
                false,
                "Operation",
            ),
            def(
                C::Protection as i32,
                "protection",
                "PROTECTION",
                Left,
                false,
                "Protection",
            ),
            def(
                C::BackdrillStub as i32,
                "backdrill_stub",
                "BACKDRILL STUB",
                Right,
                false,
                "Backdrill stub",
            ),
            def(
                C::AspectRatio as i32,
                "aspect_ratio",
                "ASPECT RATIO",
                Right,
                false,
                "Aspect ratio",
            ),
            def(
                C::Description as i32,
                "description",
                "DESCRIPTION",
                Left,
                false,
                "Description",
            ),
        ],
        default_units: GeneratedTableUnits::Mm,
        default_precision: 3,
    }
}

/// Port of `StackupTableSchema()` (`board_tables/stackup_table_model.cpp`). Headings match
/// the one-shot stackup table this generator replaces.
pub fn stackup_table_schema() -> GeneratedTableSchema {
    use GeneratedTableAlign::{Left, Right};
    use StackupTableColumnId as C;

    GeneratedTableSchema {
        defs: vec![
            def(
                C::LayerName as i32,
                "layer",
                "Layer Name",
                Left,
                true,
                "Layer",
            ),
            def(C::Type as i32, "type", "Type", Left, true, "Type"),
            def(
                C::Material as i32,
                "material",
                "Material",
                Left,
                true,
                "Material",
            ),
            def(
                C::Thickness as i32,
                "thickness",
                "Thickness",
                Right,
                true,
                "Thickness",
            ),
            def(C::Color as i32, "color", "Color", Left, true, "Color"),
            def(
                C::EpsilonR as i32,
                "epsilon_r",
                "Epsilon R",
                Right,
                true,
                "Epsilon R",
            ),
            def(
                C::LossTangent as i32,
                "loss_tangent",
                "Loss Tangent",
                Right,
                true,
                "Loss Tan",
            ),
        ],
        default_units: GeneratedTableUnits::Mm,
        default_precision: 3,
    }
}

/// `unitsTokens` / `alignTokens` and their lookups (`generated_table_schema.cpp`).
const UNITS_TOKENS: [(&str, GeneratedTableUnits); 3] = [
    ("mm", GeneratedTableUnits::Mm),
    ("inch", GeneratedTableUnits::Inch),
    ("mils", GeneratedTableUnits::Mils),
];

const ALIGN_TOKENS: [(&str, GeneratedTableAlign); 3] = [
    ("left", GeneratedTableAlign::Left),
    ("center", GeneratedTableAlign::Center),
    ("right", GeneratedTableAlign::Right),
];

/// `GeneratedTableUnitsToken( aUnits )`.
pub fn generated_table_units_token(units: GeneratedTableUnits) -> &'static str {
    UNITS_TOKENS
        .iter()
        .find(|(_, v)| *v == units)
        .map_or(UNITS_TOKENS[0].0, |(t, _)| t)
}

/// `GeneratedTableUnitsFromToken( aToken, aUnits )`.
pub fn generated_table_units_from_token(token: &str) -> Option<GeneratedTableUnits> {
    UNITS_TOKENS
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, v)| *v)
}

/// `GeneratedTableAlignToken( aAlign )`.
pub fn generated_table_align_token(align: GeneratedTableAlign) -> &'static str {
    ALIGN_TOKENS
        .iter()
        .find(|(_, v)| *v == align)
        .map_or(ALIGN_TOKENS[0].0, |(t, _)| t)
}

/// `GeneratedTableAlignFromToken( aToken, aAlign )`.
pub fn generated_table_align_from_token(token: &str) -> Option<GeneratedTableAlign> {
    ALIGN_TOKENS
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, v)| *v)
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
