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

//! Port of `pcbnew/pcb_generated_table.h` / `.cpp` (`PCB_GENERATED_TABLE`), the data of
//! `pcb_drill_chart.h` / `pcb_stackup_table.h`, and `board_tables/generated_table_schema.h` /
//! `.cpp` and `drill/drill_chart_template.h` (`DRILL_CHART_FILTER`, `DrillChartSchema()`).
//! The members live in `PcbTable::generated` (types: package M0c); the column schemas and the
//! constructors' defaults are package M11. Package M26 adds the rebuild of the cells
//! (`RebuildCells`, `migrateRows`, `setAutosizedRowHeights`, `IsStale`, `IsDataRow`, the
//! row-height and font property helpers): `impl Board` below, since a table's cells are
//! separate arena items (`board-model.md` 8.3). What each kind reports is in
//! [`crate::pcb_drill_chart`] and [`crate::pcb_stackup_table`]; the board-level refresh in
//! [`crate::board_tables::generated_table_refresh`].
//!
//! Dropped from `PCB_DRILL_CHART`: `m_laidOutShowTotals` (set only between an edit and the next
//! rebuild) and `m_builtGeneration` (refresh bookkeeping); both are transient UI state, see
//! `PcbTable::generated_is_stale` for the consequence.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::EdaUnits;
use kicad_core::printf::format_f;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::{
    GrTextHAlignT, GrTextVAlignT, Kiid, Lset, PCB_IU_SCALE, PcbLayerId, TextAttributes,
};
use kicad_font::FontExt;
use kicad_geom::geometry::approximation::ERROR_INSIDE;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::{Shape, ShapePolySet, Vector2I};

use crate::board::Board;
use crate::board_item;
use crate::ctx::BoardCtx;
use crate::drill::DrillSymbolProfile;
use crate::eda_text;
use crate::ids::{ItemId, TableId, TablecellId};
use crate::item::ParentRef;
use crate::pcb_table::PcbTable;
use crate::pcb_tablecell::PcbTablecell;
use crate::traits::EdaTextItem;

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

// ---- board_tables/generated_table_schema.cpp (free functions) ------------------------------

/// Port of `DocumentationLayers()`: the layers a generated table or a drill map may sit on.
/// Drops mask, paste, adhesive and silk, which print on the finished board, plus the courtyard
/// layers, which DRC reads as constraints.
pub fn documentation_layers() -> Lset {
    let mut layers = Lset::all_non_cu_mask() & !Lset::all_board_tech_mask();

    layers.reset(PcbLayerId::EdgeCuts);
    layers.reset(PcbLayerId::Margin);
    layers.reset(PcbLayerId::FCrtYd);
    layers.reset(PcbLayerId::BCrtYd);
    layers.reset(PcbLayerId::Rescue);

    layers
}

/// Port of `FormatGeneratedTableLength( aValue, aUnits, aPrecision )`: `"%.*f mm"`,
/// `"%.*f″"` or `"%.*f mil"`.
pub fn format_generated_table_length(
    value: i32,
    units: GeneratedTableUnits,
    precision: i32,
) -> String {
    let mm = PCB_IU_SCALE.iu_to_mm(value);
    let precision = precision.max(0) as usize;

    match units {
        GeneratedTableUnits::Inch => format!("{}\u{2033}", format_f(mm / 25.4, precision)),
        GeneratedTableUnits::Mils => {
            format!("{} mil", format_f(mm / 25.4 * 1000.0, precision))
        }
        GeneratedTableUnits::Mm => format!("{} mm", format_f(mm, precision)),
    }
}

/// Port of `ToGeneratedTableUnits( aUnits )`.
pub fn to_generated_table_units(units: EdaUnits) -> GeneratedTableUnits {
    match units {
        EdaUnits::Inch => GeneratedTableUnits::Inch,
        EdaUnits::Mils => GeneratedTableUnits::Mils,
        _ => GeneratedTableUnits::Mm,
    }
}

// ---- pcbnew/pcb_generated_table.h: content of a rebuild --------------------------------------

/// `GENERATED_TABLE_ROW`: one data row of a generated table, and the identity its formatting
/// follows across rebuilds.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GeneratedTableRow {
    pub key: String,        // m_Key
    pub cells: Vec<String>, // m_Cells
}

/// `GENERATED_TABLE_CONTENT` (with the `DRILL_CHART_CONTENT` members, unused by a stackup
/// table): what a generated table reports, one entry per cell below the heading row.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTableContent {
    /// m_Rows: keyed, below the heading row.
    pub rows: Vec<GeneratedTableRow>,
    /// m_Trailer: unkeyed rows below the data, e.g. totals.
    pub trailer: Vec<Vec<String>>,
    /// `DRILL_CHART_CONTENT::m_RowShapes`: per `rows` entry, `None` when the mark is not a
    /// shape.
    pub row_shapes: Vec<Option<i32>>,
    /// `DRILL_CHART_CONTENT::m_SymbolColumn`.
    pub symbol_column: i32,
}

impl Default for GeneratedTableContent {
    fn default() -> Self {
        GeneratedTableContent {
            rows: Vec::new(),
            trailer: Vec::new(),
            row_shapes: Vec::new(),
            symbol_column: -1,
        }
    }
}

/// Port of `GENERATED_TABLE_REFRESH`: board state a batch of rebuilds wants to change, left
/// for the caller to commit once every table has rebuilt. The only pending kind of the C++ is
/// `DRILL_PROFILE_PENDING` (the board's drill symbol profile as a refresh assigns new symbols
/// into it), so `Pending<T>()` is [`drill_profile`](Self::drill_profile).
#[derive(Clone, Debug, Default)]
pub struct GeneratedTableRefresh {
    /// `DRILL_PROFILE_PENDING::m_Profile`, created from the board on first use.
    drill_profile: Option<DrillSymbolProfile>,
}

impl GeneratedTableRefresh {
    /// `GENERATED_TABLE_REFRESH( aBoard )`.
    pub fn new() -> Self {
        GeneratedTableRefresh::default()
    }

    /// `Pending<DRILL_PROFILE_PENDING>().m_Profile`: seeded from the board, not
    /// default-constructed, since assignment builds on whatever it is handed.
    pub fn drill_profile(&mut self, board: &Board) -> &mut DrillSymbolProfile {
        self.drill_profile
            .get_or_insert_with(|| DrillSymbolProfile::from_board(board.design_settings()))
    }

    /// Port of `GENERATED_TABLE_REFRESH::Commit()`: write every pending state back to the
    /// board (`DRILL_PROFILE_PENDING::Commit`).
    pub fn commit(&self, board: &mut Board) {
        if let Some(profile) = &self.drill_profile {
            profile.commit_to_board(board.design_settings_mut());
        }
    }
}

/// `toHorizJustify( aAlign )`.
fn to_horiz_justify(align: GeneratedTableAlign) -> GrTextHAlignT {
    match align {
        GeneratedTableAlign::Left => GrTextHAlignT::Left,
        GeneratedTableAlign::Center => GrTextHAlignT::Center,
        GeneratedTableAlign::Right => GrTextHAlignT::Right,
    }
}

/// `textAt( aCells, aCol )`: a subclass short of cells for a column shows it blank rather than
/// reading past the end.
fn text_at(cells: &[String], col: i32) -> &str {
    usize::try_from(col)
        .ok()
        .and_then(|c| cells.get(c))
        .map_or("", String::as_str)
}

/// The text style `copyCellStyle( aCell, aDonor )` copies: attributes (without the position),
/// knockout and the four margins.
#[derive(Clone, Debug)]
struct CellStyle {
    attributes: TextAttributes,
    knockout: bool,
    margins: [i32; 4],
}

impl CellStyle {
    fn of(cell: &PcbTablecell) -> Self {
        let tb = &cell.textbox;
        CellStyle {
            attributes: tb.text.attributes().clone(),
            knockout: tb.shape.base.item.is_knockout,
            margins: [
                tb.margin_left,
                tb.margin_top,
                tb.margin_right,
                tb.margin_bottom,
            ],
        }
    }

    /// Port of `copyCellStyle( aCell, aDonor )`.
    fn apply(&self, cell: &mut PcbTablecell) {
        // SetAttributes( *aDonor, false )
        cell.textbox.text.set_attributes(&self.attributes);
        cell.textbox.shape.base.item.is_knockout = self.knockout;
        cell.textbox.margin_left = self.margins[0];
        cell.textbox.margin_top = self.margins[1];
        cell.textbox.margin_right = self.margins[2];
        cell.textbox.margin_bottom = self.margins[3];
    }
}

impl PcbTable {
    /// The generated-table members; panics (programmer error) for a plain table.
    fn gen_data(&self) -> &GeneratedTableData {
        self.generated
            .as_deref()
            .expect("PCB_GENERATED_TABLE method on a plain PCB_TABLE")
    }

    /// Port of `PCB_GENERATED_TABLE::IsDataRow( aRow )` and the `PCB_DRILL_CHART` override:
    /// true for a row that reports on the board, false for the heading and trailer rows. A
    /// table loaded without row keys has only position to go on; a drill chart then counts its
    /// last row as the totals row when it shows totals (`m_laidOutShowTotals` is never set in
    /// the port, so `m_showTotals` decides).
    pub fn is_data_row(&self, row: i32) -> bool {
        let data = self.gen_data();

        if let GeneratedTableKind::DrillChart { show_totals, .. } = &data.kind {
            if !data.row_keys.is_empty() {
                return data.row_keys.contains_key(&row);
            }

            // A chart written before the keys were recorded still has to answer this, and its
            // rows are laid out the way RebuildCells lays them out
            let first_data_row = 1;
            let last_data_row = self.row_count() - 1 - i32::from(*show_totals);

            return row >= first_data_row && row <= last_data_row;
        }

        if data.row_keys.is_empty() {
            return row >= 1 && row < self.row_count();
        }

        data.row_keys.contains_key(&row)
    }

    /// Port of `PCB_GENERATED_TABLE::dataRows()`: the rows below the heading that
    /// `IsDataRow` accepts, in order.
    pub fn data_rows(&self) -> Vec<i32> {
        (1..self.row_count())
            .filter(|&row| self.is_data_row(row))
            .collect()
    }

    /// Port of `PCB_GENERATED_TABLE::GetRowHeightProp()`: the first data row's height.
    pub fn row_height_prop(&self) -> i32 {
        self.data_rows()
            .first()
            .map_or(0, |&row| self.row_height(row))
    }

    /// Port of `PCB_GENERATED_TABLE::RowHeightsDiffer()`.
    pub fn row_heights_differ(&self) -> bool {
        let first = self.row_height_prop();

        self.data_rows()
            .iter()
            .any(|&row| self.row_height(row) != first)
    }

    /// Port of `PCB_GENERATED_TABLE::GetFontNameProp()`: the first cell's font
    /// (`PCB_TABLECELL::GetFontProp`, the font name).
    pub fn font_name_prop(&self, ctx: &BoardCtx<'_>) -> String {
        self.cells
            .first()
            .and_then(|&c| ctx.board.tablecells.get(c))
            .map_or_else(String::new, |c| c.textbox.text.font_name().to_string())
    }

    /// Port of `PCB_GENERATED_TABLE::FontNamesDiffer()`.
    pub fn font_names_differ(&self, ctx: &BoardCtx<'_>) -> bool {
        let first = self.font_name_prop(ctx);

        self.cells
            .iter()
            .filter_map(|&c| ctx.board.tablecells.get(c))
            .any(|c| c.textbox.text.font_name() != first)
    }

    /// Port of `PCB_GENERATED_TABLE::GetFontSizeProp()`: the first cell's text height
    /// (`PCB_TABLECELL::GetFontSize`).
    pub fn font_size_prop(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.cells
            .first()
            .and_then(|&c| ctx.board.tablecells.get(c))
            .map_or(0, |c| EdaTextItem::text_size(c, ctx).y)
    }

    /// Port of `PCB_GENERATED_TABLE::FontSizesDiffer()`: some cell has a non-square text size
    /// or another height.
    pub fn font_sizes_differ(&self, ctx: &BoardCtx<'_>) -> bool {
        let first = self.font_size_prop(ctx);

        self.cells
            .iter()
            .filter_map(|&c| ctx.board.tablecells.get(c))
            .any(|c| {
                let size = EdaTextItem::text_size(c, ctx);
                size.x != size.y || size.y != first
            })
    }

    /// Port of `PCB_GENERATED_TABLE::metricRowHeight( aRow )`: the height `row` needs by its
    /// cells' line count, line spacing, pen width and margins.
    fn metric_row_height(&self, ctx: &BoardCtx<'_>, row: i32) -> i32 {
        let mut height = 0;

        for col in 0..self.col_count() {
            let Some(cell) = self
                .cell(row, col)
                .and_then(|c| ctx.board.tablecells.get(c))
            else {
                continue;
            };

            let shown = cell.shown_text(ctx, ResolutionContext::ForCanvas, 0);
            let lines = shown.matches('\n').count() as i32 + 1;

            // The pen strokes half its width past the glyph geometry on each side
            height = height.max(
                lines * eda_text::interline(cell, ctx)
                    + eda_text::effective_text_pen_width(cell, ctx, 0)
                    + cell.textbox.margin_top
                    + cell.textbox.margin_bottom,
            );
        }

        height
    }

    /// Port of `PCB_GENERATED_TABLE::minRowHeight( aRow )`: the smallest height `row` may take
    /// with its text still inside it. Centred text needs no margin, while top or bottom aligned
    /// text keeps the margin it is set in by.
    fn min_row_height(&self, ctx: &BoardCtx<'_>, row: i32) -> i32 {
        let mut height = 0;

        for col in 0..self.col_count() {
            let Some(cell) = self
                .cell(row, col)
                .and_then(|c| ctx.board.tablecells.get(c))
            else {
                continue;
            };

            let mut glyphs = ShapePolySet::new();
            cell.transform_text_to_poly_set(ctx, &mut glyphs, 0, ARC_LOW_DEF, ERROR_INSIDE);

            if glyphs.is_empty() {
                continue;
            }

            let anchor = EdaTextItem::draw_pos(cell, ctx);
            glyphs.rotate(-EdaTextItem::draw_rotation(cell, ctx), anchor);

            let bbox = glyphs.bbox(0);
            let above = anchor.y - bbox.top();
            let below = bbox.bottom() - anchor.y;

            // Centred text ignores the vertical margins, while top and bottom text sits one
            // margin in
            match cell.textbox.text.vert_justify() {
                GrTextVAlignT::Top => height = height.max(cell.textbox.margin_top + below),
                GrTextVAlignT::Bottom => height = height.max(cell.textbox.margin_bottom + above),
                _ => height = height.max(2 * above.max(below)),
            }
        }

        height
    }

    /// Port of `PCB_GENERATED_TABLE::nearestDataCell( aRow, aCol, aStyled )`: the nearest data
    /// cell in `col` among `styled`, or `None` when the column has none.
    fn nearest_data_cell(
        &self,
        ctx: &BoardCtx<'_>,
        row: i32,
        col: i32,
        styled: &BTreeSet<Kiid>,
    ) -> Option<TablecellId> {
        let rows = self.row_count();

        for dist in 1..rows {
            for r in [row - dist, row + dist] {
                if r < 1 || r >= rows || !self.is_data_row(r) {
                    continue;
                }

                if let Some(id) = self.cell(r, col) {
                    if let Some(cell) = ctx.board.tablecells.get(id) {
                        if styled.contains(&cell.textbox.shape.base.item.uuid) {
                            return Some(id);
                        }
                    }
                }
            }
        }

        None
    }

    /// Port of `PCB_GENERATED_TABLE::IsStale( aBoard )` and the `PCB_DRILL_CHART` override.
    ///
    /// The default (stack-up table) compares freshly generated text with the cells. A drill
    /// chart compares `m_builtGeneration` with the board's drill model generation; the port
    /// tracks neither, and treats a drill chart as always stale. In the C++ a chart is stale
    /// after every load (`m_builtGeneration` starts at 0, the board's generation at 1) and
    /// fresh after its rebuild until a pad, via or footprint changes, so the only difference
    /// is a second refresh in the same session without a drill change, which the C++ skips
    /// and the port repeats (a rebuild of a fresh chart reproduces its cells).
    pub fn generated_is_stale(&self, ctx: &BoardCtx<'_>) -> bool {
        let data = self.gen_data();

        if matches!(data.kind, GeneratedTableKind::DrillChart { .. }) {
            return true;
        }

        let content = generate(ctx, data, None);

        let cols = data.columns.len() as i32;
        let data_rows = content.rows.len() as i32;
        let rows = 1 + data_rows + content.trailer.len() as i32;

        if self.col_count() != cols || self.row_count() != rows {
            return true;
        }

        let differs = |row: i32, col: i32, text: &str| -> bool {
            match self
                .cell(row, col)
                .and_then(|c| ctx.board.tablecells.get(c))
            {
                None => true,
                Some(cell) => cell.textbox.text.text() != text,
            }
        };

        for col in 0..cols {
            if differs(0, col, &data.columns[col as usize].heading) {
                return true;
            }
        }

        for (ii, row) in content.rows.iter().enumerate() {
            let ii = ii as i32;

            match data.row_keys.get(&(1 + ii)) {
                Some(key) if *key == row.key => {}
                _ => return true,
            }

            for col in 0..cols {
                if differs(1 + ii, col, text_at(&row.cells, col)) {
                    return true;
                }
            }
        }

        for (ii, trailer) in content.trailer.iter().enumerate() {
            for col in 0..cols {
                if differs(1 + data_rows + ii as i32, col, text_at(trailer, col)) {
                    return true;
                }
            }
        }

        false
    }
}

/// `PCB_GENERATED_TABLE::generate( aBoard, aRefresh )`: the rows this table reports
/// (`PCB_DRILL_CHART::generate` / `PCB_STACKUP_TABLE::generate`). A `None` refresh leaves no
/// pending board state behind.
pub fn generate(
    ctx: &BoardCtx<'_>,
    data: &GeneratedTableData,
    refresh: Option<&mut DrillSymbolProfile>,
) -> GeneratedTableContent {
    match &data.kind {
        GeneratedTableKind::DrillChart { .. } => {
            crate::pcb_drill_chart::generate(ctx, data, refresh)
        }
        GeneratedTableKind::StackupTable => crate::pcb_stackup_table::generate(ctx, data),
    }
}

/// The `PCB_GENERATED_TABLE` methods that move or create cells (separate arena items).
impl Board {
    /// The cells of `table` (`m_cells`), empty for a stale handle.
    fn generated_cell_ids(&self, table: TableId) -> Vec<TablecellId> {
        self.tables
            .get(table)
            .map(|t| t.cells.clone())
            .unwrap_or_default()
    }

    /// `new PCB_TABLECELL( this )` for a board-level table (text mirrored on a back layer,
    /// `PCB_TABLECELL::PCB_TABLECELL`) followed by `SetLayer( GetLayer() )`, stored in the
    /// arena but not in the table's cell list.
    fn new_generated_cell(&mut self, table: TableId) -> TablecellId {
        let layer = self[table].base.layer;
        let mirrored = {
            let ctx = BoardCtx::bare(self);
            match board_item::parent_footprint(&self[table], &ctx) {
                Some(fp) => fp.is_flipped(),
                None => self.is_back_layer(layer),
            }
        };
        let cell = PcbTablecell::new(&mut self.kiid_generator, mirrored);
        let id = self.insert_item(cell, ParentRef::Item(table.into()));
        self.modify_tablecell(id, |c, env| c.set_layer(env, layer));
        id
    }

    /// Port of `PCB_TABLE::ResizeCells( aRows, aCols )`: grow or shrink to `rows` x `cols`,
    /// keeping the cells that already exist (new ones through `AddCell`).
    fn table_resize_cells(&mut self, table: TableId, rows: i32, cols: i32) {
        let wanted = rows as usize * cols as usize;

        self.modify_table(table, |t, _| t.col_count = cols);

        while self[table].cells.len() > wanted {
            let last = *self[table].cells.last().expect("non-empty");
            self.modify_table(table, |t, _| {
                t.cells.pop();
            });
            self.remove(last.into());
        }

        while self[table].cells.len() < wanted {
            let mirrored = self.is_back_layer(self[table].base.layer);
            let cell = PcbTablecell::new(&mut self.kiid_generator, mirrored);
            self.table_add_cell(table, cell);
        }
    }

    /// Port of `PCB_GENERATED_TABLE::migrateRows( aRows, aCols, aNewRowKeys )`: move each
    /// row's cells to the row that reports the same thing.
    ///
    /// Formatting lives on the cells, so without this a rebuild that adds or removes a row
    /// would hand a row's formatting to whichever row happened to land on its index. Rows whose
    /// key has gone are deleted and rows for a new key are created, leaving the table exactly
    /// `rows` by `cols`. A table without row keys has only position to go on for its data
    /// rows, and its trailers still follow their role.
    fn generated_migrate_rows(
        &mut self,
        table: TableId,
        rows: i32,
        cols: i32,
        new_row_keys: &[String],
    ) {
        let t = &self[table];
        let old_cols = t.col_count();

        if old_cols <= 0 || cols <= 0 || t.cells.is_empty() {
            return;
        }

        let old_rows = t.cells.len() as i32 / old_cols;
        let first_data_row = 1;
        let row_keys = t.gen_data().row_keys.clone();

        let mut old_row_by_key: BTreeMap<String, i32> = BTreeMap::new();
        let mut old_first_data = old_rows;
        let mut old_last_data = -1;

        // A legacy table's data rows have no identity beyond their position. Its trailers still
        // go by role, or a data row appearing would take over the totals
        let positional_data = row_keys.is_empty();

        if positional_data {
            old_first_data = first_data_row;
            old_last_data = first_data_row - 1;

            for row in first_data_row..old_rows {
                if t.is_data_row(row) {
                    old_last_data = row;
                }
            }
        } else {
            for (&old_row, key) in &row_keys {
                if old_row < 0 || old_row >= old_rows {
                    continue;
                }

                old_row_by_key.insert(key.clone(), old_row);
                old_first_data = old_first_data.min(old_row);
                old_last_data = old_last_data.max(old_row);
            }

            if old_row_by_key.is_empty() {
                return;
            }
        }

        let new_data_count = new_row_keys.iter().filter(|k| !k.is_empty()).count() as i32;

        let mut source_row = vec![-1i32; rows as usize];

        // The title and heading are matched from the end of their run, so the heading stays
        // the heading when a title is added or removed
        for ii in 0..first_data_row {
            let old_idx = old_first_data - (first_data_row - ii);

            if old_idx >= 0 {
                source_row[ii as usize] = old_idx;
            }
        }

        if positional_data {
            let count = new_data_count.min(old_last_data - old_first_data + 1);

            for ii in 0..count {
                source_row[(first_data_row + ii) as usize] = old_first_data + ii;
            }
        } else {
            for ii in first_data_row..rows {
                let key = &new_row_keys[ii as usize];

                if key.is_empty() {
                    continue;
                }

                if let Some(&old) = old_row_by_key.get(key) {
                    source_row[ii as usize] = old;
                }
            }
        }

        let trailing_new_start = first_data_row + new_data_count;

        for ii in trailing_new_start..rows {
            let old_idx = old_last_data + 1 + (ii - trailing_new_start);

            if old_idx < old_rows {
                source_row[ii as usize] = old_idx;
            }
        }

        let old_cells = t.cells.clone();
        let old_row_heights = t.row_heights.clone();
        let mut new_cells: Vec<Option<TablecellId>> = vec![None; rows as usize * cols as usize];
        let mut carried = vec![false; old_cells.len()];
        let mut new_row_heights: BTreeMap<i32, i32> = BTreeMap::new();

        for ii in 0..rows {
            let src = source_row[ii as usize];

            if src < 0 {
                continue;
            }

            // A column added since the last rebuild has no cell to carry, and one taken away
            // leaves its cells behind to be deleted with the rest of the uncarried ones
            for col in 0..old_cols.min(cols) {
                let from = (src * old_cols + col) as usize;

                new_cells[(ii * cols + col) as usize] = Some(old_cells[from]);
                carried[from] = true;
            }

            if let Some(&height) = old_row_heights.get(&src) {
                new_row_heights.insert(ii, height);
            }
        }

        let mut cells = Vec::with_capacity(new_cells.len());

        for cell in new_cells {
            match cell {
                Some(id) => cells.push(id),
                None => cells.push(self.new_generated_cell(table)),
            }
        }

        self.modify_table(table, |t, _| {
            t.cells = cells;
            t.row_heights = new_row_heights;
        });

        let doomed: Vec<ItemId> = old_cells
            .iter()
            .zip(&carried)
            .filter(|&(_, &kept)| !kept)
            .map(|(&id, _)| id.into())
            .collect();

        if !doomed.is_empty() {
            self.remove_many(&doomed);
        }
    }

    /// Port of `PCB_TABLE::Autosize()` on a generated table, with the `PCB_GENERATED_TABLE`
    /// override of `setAutosizedRowHeights( aNeeded )`.
    ///
    /// The column widths and the needed heights are `PCB_TABLE`'s (`PcbTable::autosize`, run
    /// on a copy without the generated members so that it stores the needed heights). Then the
    /// override: row heights are the user's, carried by key through `migrateRows`. Every row
    /// keeps the height it held while its text still fits inside it, and otherwise takes what
    /// its text needs with margins. A data row that held none takes the height most data rows
    /// held, or when none held one, what the tallest data row's text needs with margins.
    /// Finally `Normalize()`.
    pub fn generated_table_autosize(&mut self, table: TableId) {
        let Some(mut plain) = self.tables.get(table).cloned() else {
            return;
        };

        plain.generated = None;

        {
            let ctx = BoardCtx::bare(self);
            plain.autosize(&ctx);
        }

        let rows = self[table].row_count();
        let needed: Vec<i32> = (0..rows).map(|row| plain.row_height(row)).collect();
        let col_widths = plain.col_widths;
        self.modify_table(table, |t, _| t.col_widths = col_widths);

        // setAutosizedRowHeights( needed ):

        // Cells take Autosize's widths first, or the line metrics below follow the old wrap
        self.table_normalize(table);

        // Normalize() stores a missing row as 0, so 0 is no height rather than the user's
        let mut held = vec![0i32; rows as usize];
        let mut fit = vec![0i32; rows as usize];

        {
            let ctx = BoardCtx::bare(self);
            let t = &self[table];

            for row in 0..rows {
                let r = row as usize;
                held[r] = t.row_height(row).max(0);

                // Glyph outlines differ by character, which splits the Row Height by microns, so
                // data rows size to line metrics unless a glyph reaches past them
                fit[r] = if t.is_data_row(row) {
                    needed[r].max(t.metric_row_height(&ctx, row))
                } else {
                    needed[r]
                };
            }
        }

        let mut max_need = 0;
        let mut counts: BTreeMap<i32, i32> = BTreeMap::new();

        for row in self[table].data_rows() {
            let r = row as usize;
            max_need = max_need.max(fit[r]);

            if held[r] > 0 {
                *counts.entry(held[r]).or_insert(0) += 1;
            }
        }

        // A data row without a height of its own matches most of its siblings, larger on a
        // tie. With none held, as in a fresh table, all share the tallest need, or glyph
        // descents split the value
        let mut common = max_need;
        let mut common_count = 0;

        for (&height, &count) in &counts {
            if count >= common_count {
                common = height;
                common_count = count;
            }
        }

        // Rows default to their text plus margins, but keep any height that still holds the
        // text, down to the text touching its edges. Text that outgrows a height refits it with
        // margins, so one row's font moves no sibling
        for row in 0..rows {
            let r = row as usize;
            let (height, min_height) = {
                let ctx = BoardCtx::bare(self);
                let t = &self[table];
                let height = if t.is_data_row(row) && held[r] == 0 {
                    common
                } else {
                    held[r]
                };
                (height, t.min_row_height(&ctx, row))
            };

            let value = if height > 0 && height >= min_height {
                height
            } else {
                fit[r]
            };
            self.modify_table(table, |t, _| t.set_row_height(row, value));
        }

        // PCB_TABLE::Autosize()
        self.table_normalize(table);
    }

    /// Port of `PCB_GENERATED_TABLE::SetRowHeightProp( aHeight )`: every data row, each no
    /// shorter than its text, then `Normalize()`.
    pub fn generated_set_row_height_prop(&mut self, table: TableId, height: i32) {
        for row in self[table].data_rows() {
            let min = {
                let ctx = BoardCtx::bare(self);
                self[table].min_row_height(&ctx, row)
            };
            self.modify_table(table, |t, _| t.set_row_height(row, height.max(min)));
        }

        self.table_normalize(table);
    }

    /// Port of `PCB_GENERATED_TABLE::SetRowHeightToFit( aRow, aHeight )`.
    pub fn generated_set_row_height_to_fit(&mut self, table: TableId, row: i32, height: i32) {
        let min = {
            let ctx = BoardCtx::bare(self);
            self[table].min_row_height(&ctx, row)
        };
        self.modify_table(table, |t, _| t.set_row_height(row, height.max(min)));
        self.table_normalize(table);
    }

    /// Port of `PCB_GENERATED_TABLE::styleFromColumn( aCell, aRow, aCol, aStyled )`: give a
    /// new cell the text formatting of the nearest data cell in its column among `styled`,
    /// or the kind's defaults (`styleNewCell`) when there is none or the cell is a heading.
    fn generated_style_from_column(
        &mut self,
        table: TableId,
        cell: TablecellId,
        row: i32,
        col: i32,
        styled: &BTreeSet<Kiid>,
    ) {
        let donor = if row > 0 {
            let ctx = BoardCtx::bare(self);
            self[table]
                .nearest_data_cell(&ctx, row, col, styled)
                .map(|d| CellStyle::of(&self[d]))
        } else {
            None
        };

        match donor {
            Some(style) => {
                self.modify_tablecell(cell, |c, _| style.apply(c));
            }
            None => {
                let is_stackup = matches!(
                    self[table].gen_data().kind,
                    GeneratedTableKind::StackupTable
                );

                if is_stackup {
                    self.modify_tablecell(cell, |c, env| {
                        crate::pcb_stackup_table::style_new_cell(c, env, row == 0)
                    });
                }
            }
        }
    }

    /// Port of `PCB_GENERATED_TABLE::RebuildCells( aBoard, aRefresh )`: regenerate the cells
    /// from the board.
    ///
    /// Cells are reused rather than recreated so their UUIDs survive. Board state the rebuild
    /// wants to change is left in `refresh` for the caller to commit; a `None` refresh is a
    /// preview and leaves no pending board state behind.
    pub fn generated_rebuild_cells(
        &mut self,
        table: TableId,
        refresh: Option<&mut GeneratedTableRefresh>,
    ) {
        let content = {
            let ctx = BoardCtx::bare(self);
            let profile = refresh.map(|r| r.drill_profile(self));
            generate(&ctx, self[table].gen_data(), profile)
        };

        let (columns, cols) = {
            let data = self[table].gen_data();
            (data.columns.clone(), data.columns.len() as i32)
        };
        let data_rows = content.rows.len() as i32;

        // The headings are the header row. A table carries no caption of its own
        let rows = 1 + data_rows + content.trailer.len() as i32;

        // A new cell carries a half-INT_MAX rectangle and Normalize() anchors on cell 0's
        // centre, so without holding the old position a rebuild lands the table half a metre
        // off-board
        let anchor = if self[table].cells.is_empty() {
            Vector2I::new(0, 0)
        } else {
            self.generated_table_position(table)
        };

        let mut new_row_keys = vec![String::new(); rows as usize];

        // What a row reports, so a rebuild hands its formatting to the row still reporting the
        // same thing rather than to whatever lands on its index
        for (ii, row) in content.rows.iter().enumerate() {
            new_row_keys[1 + ii] = row.key.clone();
        }

        // By UUID, not pointer. A cell migrateRows deletes can hand its address to a new one
        let existing: BTreeSet<Kiid> = self
            .generated_cell_ids(table)
            .iter()
            .filter_map(|&c| self.tablecells.get(c))
            .map(|c| c.textbox.shape.base.item.uuid)
            .collect();

        self.generated_migrate_rows(table, rows, cols, &new_row_keys);

        // onRowsMigrated(): PCB_DRILL_CHART resets m_laidOutShowTotals (not kept, see the
        // module comment)

        self.modify_table(table, |t, _| t.set_col_count(cols));
        self.table_resize_cells(table, rows, cols);

        self.modify_table(table, |t, _| {
            let data = t.generated.as_deref_mut().expect("generated table");
            data.row_keys.clear();

            for (ii, key) in new_row_keys.iter().enumerate() {
                if !key.is_empty() {
                    data.row_keys.insert(ii as i32, key.clone());
                }
            }
        });

        let set_cell = |board: &mut Board, row: i32, col: i32, text: &str| {
            let Some(cell) = board[table].cell(row, col) else {
                return;
            };

            if !existing.contains(&board[cell].textbox.shape.base.item.uuid) {
                board.generated_style_from_column(table, cell, row, col, &existing);
            }

            // The column's alignment, which was otherwise editable, serialized and ignored
            let justify = to_horiz_justify(columns[col as usize].align);

            board.modify_tablecell(cell, |c, _| {
                c.textbox.text.set_text(text);
                c.textbox.text.set_horiz_justify(justify);
            });
        };

        for col in 0..cols {
            set_cell(self, 0, col, &columns[col as usize].heading);
        }

        for (ii, row) in content.rows.iter().enumerate() {
            for col in 0..cols {
                set_cell(self, 1 + ii as i32, col, text_at(&row.cells, col));
            }
        }

        for (ii, trailer) in content.trailer.iter().enumerate() {
            for col in 0..cols {
                set_cell(self, 1 + data_rows + ii as i32, col, text_at(trailer, col));
            }
        }

        // Autosize measures text as wrapped to its cell, so a reused cell narrower than its new
        // text would keep the wrap. Widen those to their resolved, unwrapped text, which
        // Autosize then tightens
        let mut prewidened = false;

        for col in 0..cols {
            let mut width = 0;

            {
                let ctx = BoardCtx::bare(self);
                let t = &self[table];

                for row in 0..rows {
                    let Some(cell) = t.cell(row, col).and_then(|c| self.tablecells.get(c)) else {
                        continue;
                    };

                    let pen_width = eda_text::effective_text_pen_width(cell, &ctx, 0);
                    let et = &cell.textbox.text;
                    let lookup = et.draw_font_lookup();
                    let extents = lookup.font().string_boundary_limits(
                        &cell.unwrapped_shown_text(&ctx, ResolutionContext::ForCanvas, 0),
                        EdaTextItem::text_size(cell, &ctx),
                        pen_width,
                        et.is_bold(),
                        et.is_italic(),
                        EdaTextItem::font_metrics(cell, &ctx),
                    );

                    // GetShownText breaks at the width less margins, and LinebreakText takes the
                    // pen off too
                    width = width.max(
                        extents.x
                            + pen_width
                            + 2 * (cell.textbox.margin_left + cell.textbox.margin_right),
                    );
                }
            }

            if width > self[table].col_width(col) {
                self.modify_table(table, |t, _| t.set_col_width(col, width));
                prewidened = true;
            }
        }

        if prewidened {
            self.table_normalize(table);
        }

        self.generated_table_autosize(table);

        // After autosizing or the authored width never shows, and as a minimum so a width
        // saved against a narrower board cannot clip its text
        let mut widened = false;

        for col in 0..cols {
            let width = columns[col as usize].width;

            if width > self[table].col_width(col) {
                self.modify_table(table, |t, _| t.set_col_width(col, width));
                widened = true;
            }
        }

        // The widths above are only a map until the cells are placed against them
        if widened {
            self.table_normalize(table);
        }

        let position = self.generated_table_position(table);
        self.move_item(table.into(), anchor - position);

        // Last, so a table is only ever marked fresh once its layout is complete
        // (onRebuilt: PCB_DRILL_CHART keeps the row shapes and the symbol column)
        self.modify_table(table, |t, _| {
            let data = t.generated.as_deref_mut().expect("generated table");

            if let GeneratedTableKind::DrillChart {
                row_shapes,
                symbol_column,
                ..
            } = &mut data.kind
            {
                row_shapes.clear();

                for (ii, shape) in content.row_shapes.iter().enumerate() {
                    if let Some(shape) = shape {
                        row_shapes.insert(1 + ii as i32, *shape);
                    }
                }

                *symbol_column = content.symbol_column;
            }
        });
    }

    /// `PCB_TABLE::GetPosition()` of a stored table.
    fn generated_table_position(&self, table: TableId) -> Vector2I {
        let ctx = BoardCtx::bare(self);
        self[table].position(&ctx)
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
