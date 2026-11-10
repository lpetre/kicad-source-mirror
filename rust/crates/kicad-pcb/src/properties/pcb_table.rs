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

//! Port of `PCB_TABLE_DESC` (`pcbnew/pcb_table.cpp`), `PCB_TABLECELL_DESC`
//! (`pcbnew/pcb_tablecell.cpp`), `PCB_GENERATED_TABLE_DESC` (`pcbnew/pcb_generated_table.cpp`),
//! `PCB_DRILL_CHART_DESC` (`pcbnew/pcb_drill_chart.cpp`) and `PCB_STACKUP_TABLE_DESC`
//! (`pcbnew/pcb_stackup_table.cpp`).

use crate::eda_text::EdaTextExt;
use crate::item_ref::{AnyItem, ItemRef};
use crate::pcb_generated_table::{GeneratedTableData, GeneratedTableKind};
use crate::pcb_table::PcbTable;
use crate::pcb_tablecell::PcbTablecell;

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue,
    ValueType, eda_text::font_prop, no_rust_api,
};

/// `static_cast<PCB_TABLE*>`.
fn table<'a>(it: &ItemRef<'a>) -> &'a PcbTable {
    it.as_table().expect("PCB_TABLE property on a table")
}

fn tbool(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbTable, ValueType::Bool, true, get)
}

/// `PCB_TABLE_DESC::PCB_TABLE_DESC()` (the `LINE_STYLE` map is in [`super::enums`]).
pub(crate) fn register_pcb_table(pm: &mut PropertyManager) {
    let c = ClassId::PcbTable;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardItemContainer);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::BoardItemContainer);

    pm.add_property(
        Property::new("Start X", c, ValueType::Int, true, |it| {
            PropertyValue::Int(it.position().x)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsXCoord),
        "",
    );
    pm.add_property(
        Property::new("Start Y", c, ValueType::Int, true, |it| {
            PropertyValue::Int(it.position().y)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsYCoord),
        "",
    );

    // _( "Table Properties" )
    let table_props = "Table Properties";

    pm.add_property(
        tbool("External Border", |it| {
            PropertyValue::Bool(table(&it).stroke_external)
        }),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        tbool("Header Border", |it| {
            PropertyValue::Bool(table(&it).stroke_header_separator)
        }),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Border Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(table(&it).border_stroke.width())
        })
        .with_display(PropertyDisplay::PtSize),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new_enum(
            "Border Style",
            c,
            ValueType::Enum(EnumId::LineStyle),
            true,
            |it| PropertyValue::Enum(EnumId::LineStyle, table(&it).border_style() as i32),
        ),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Border Color", c, ValueType::Color, true, |it| {
            PropertyValue::Color(table(&it).border_stroke.color().clone())
        }),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        tbool("Row Separators", |it| {
            PropertyValue::Bool(table(&it).stroke_rows)
        }),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        tbool("Cell Separators", |it| {
            PropertyValue::Bool(table(&it).stroke_columns)
        }),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Separators Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(table(&it).separators_stroke.width())
        })
        .with_display(PropertyDisplay::PtSize),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new_enum(
            "Separators Style",
            c,
            ValueType::Enum(EnumId::LineStyle),
            true,
            |it| PropertyValue::Enum(EnumId::LineStyle, table(&it).separators_style() as i32),
        ),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Separators Color", c, ValueType::Color, true, |it| {
            PropertyValue::Color(table(&it).separators_stroke.color().clone())
        }),
        table_props,
    )
    .set_is_copyable();
}

/// `static_cast<PCB_TABLECELL*>`.
fn cell<'a>(it: &ItemRef<'a>) -> &'a PcbTablecell {
    match it.any() {
        AnyItem::Tablecell(c) => c,
        _ => unreachable!("PCB_TABLECELL property on a table cell"),
    }
}

/// `IsGeneratedTableCell` (`pcbnew/pcb_table.h`): a cell whose parent table is generated.
fn is_generated_table_cell(it: ItemRef<'_>) -> bool {
    if it.type_() != kicad_core::KicadT::PcbTablecellT {
        return false;
    }

    it.parent()
        .and_then(|p| p.as_table())
        .is_some_and(|t| t.is_generated())
}

/// `isPlainCell`.
fn is_plain_cell(it: ItemRef<'_>) -> bool {
    !is_generated_table_cell(it)
}

/// `PCB_TABLECELL_DESC::PCB_TABLECELL_DESC()`.
pub(crate) fn register_pcb_tablecell(pm: &mut PropertyManager) {
    let c = ClassId::PcbTablecell;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardConnectedItem);
    pm.add_type_cast(c, ClassId::PcbTextbox);
    pm.add_type_cast(c, ClassId::PcbShape);
    pm.add_type_cast(c, ClassId::EdaShape);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::BoardConnectedItem);
    pm.inherits_after(c, ClassId::PcbTextbox);
    pm.inherits_after(c, ClassId::PcbShape);
    pm.inherits_after(c, ClassId::EdaShape);
    pm.inherits_after(c, ClassId::EdaText);

    pm.mask(c, ClassId::BoardItem, "Position X");
    pm.mask(c, ClassId::BoardItem, "Position Y");
    pm.mask(c, ClassId::PcbShape, "Layer");
    pm.mask(c, ClassId::PcbShape, "Soldermask");
    pm.mask(c, ClassId::PcbShape, "Soldermask Margin Override");
    pm.mask(c, ClassId::EdaShape, "Corner Radius");

    pm.mask(c, ClassId::BoardConnectedItem, "Net");

    pm.mask(c, ClassId::PcbTextbox, "Border");
    pm.mask(c, ClassId::PcbTextbox, "Border Style");
    pm.mask(c, ClassId::PcbTextbox, "Border Width");

    for name in [
        "Start X",
        "Start Y",
        "End X",
        "End Y",
        "Shape",
        "Width",
        "Height",
        "Line Width",
        "Line Style",
        "Line Color",
    ] {
        pm.mask(c, ClassId::EdaShape, name);
    }

    pm.mask(c, ClassId::EdaText, "Orientation");
    pm.mask(c, ClassId::EdaText, "Hyperlink");
    pm.mask(c, ClassId::EdaText, "Color");

    // A generated table reports the board, so cell contents are not the user's to edit but
    // their formatting is. Scoped here, not on EDA_TEXT's descriptor, which eeschema shares
    pm.override_writeability(c, ClassId::EdaText, "Text", |it| {
        !is_generated_table_cell(it)
    });

    // A generated cell takes its font and size through the Font group, the same as its table
    for name in ["Font", "Width", "Height"] {
        pm.override_availability(c, ClassId::EdaText, name, is_plain_cell);
    }

    let font_props = "Font";

    pm.add_property(
        Property::new("Font Name", c, ValueType::String, true, |it| {
            PropertyValue::String(font_prop(&cell(&it).textbox.text))
        })
        .with_base(ClassId::EdaText),
        font_props,
    )
    .set_available_func(is_generated_table_cell)
    .set_is_hidden_from_rules_editor()
    .set_is_copyable()
    .set_choices_func();

    // GetFontSize: GetTextHeight() (virtual).
    pm.add_property(
        Property::new("Font Size", c, ValueType::Int, true, |it| {
            PropertyValue::Int(cell(&it).text_height(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        font_props,
    )
    .set_available_func(is_generated_table_cell)
    .set_is_copyable()
    .set_indeterminate_func();

    // _( "Table" )
    let table_props = "Table";

    pm.add_property(
        Property::new("Column Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(cell(&it).column_width(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        table_props,
    );

    pm.add_property(
        Property::new("Row Height", c, ValueType::Int, true, |it| {
            PropertyValue::Int(cell(&it).row_height(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        table_props,
    );
}

/// The generated-table data of a `PCB_GENERATED_TABLE` item.
fn generated<'a>(it: &ItemRef<'a>) -> &'a GeneratedTableData {
    table(it)
        .generated
        .as_deref()
        .expect("PCB_GENERATED_TABLE property on a generated table")
}

/// `PCB_GENERATED_TABLE_DESC::PCB_GENERATED_TABLE_DESC()` (the `GENERATED_TABLE_UNITS` map is
/// in [`super::enums`]).
pub(crate) fn register_pcb_generated_table(pm: &mut PropertyManager) {
    let c = ClassId::PcbGeneratedTable;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardItemContainer);
    pm.add_type_cast(c, ClassId::PcbTable);
    pm.inherits_after(c, ClassId::PcbTable);

    // _( "Table Generator" )
    let generator_props = "Table Generator";

    pm.add_property(
        Property::new_enum(
            "Units",
            c,
            ValueType::Enum(EnumId::GeneratedTableUnits),
            true,
            |it| PropertyValue::Enum(EnumId::GeneratedTableUnits, generated(&it).units as i32),
        ),
        generator_props,
    );

    pm.add_property(
        Property::new("Decimal Places", c, ValueType::Int, true, |it| {
            PropertyValue::Int(generated(&it).precision)
        }),
        generator_props,
    );

    // Scoped here rather than removed from PCB_TABLE so colors saved on plain tables stay
    // editable
    pm.mask(c, ClassId::PcbTable, "Border Color");
    pm.mask(c, ClassId::PcbTable, "Separators Color");
    pm.mask(c, ClassId::PcbTable, "Cell Separators");

    // Registered again below so it sits beside Column Separators
    pm.mask(c, ClassId::PcbTable, "Row Separators");

    // _( "Table Properties" )
    let table_props = "Table Properties";

    pm.add_property(
        Property::new("Row Separators", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(table(&it).stroke_rows)
        })
        .with_base(ClassId::PcbTable),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Column Separators", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(table(&it).stroke_columns)
        })
        .with_base(ClassId::PcbTable),
        table_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Row Height", c, ValueType::Int, true, |_| {
            no_rust_api("PCB_GENERATED_TABLE::GetRowHeightProp")
        })
        .with_display(PropertyDisplay::PtSize),
        table_props,
    )
    .set_is_copyable()
    .set_indeterminate_func();

    let font_props = "Font";

    pm.add_property(
        Property::new("Font Name", c, ValueType::String, true, |_| {
            no_rust_api("PCB_GENERATED_TABLE::GetFontNameProp")
        }),
        font_props,
    )
    .set_is_hidden_from_rules_editor()
    .set_is_copyable()
    .set_indeterminate_func()
    .set_choices_func();

    pm.add_property(
        Property::new("Font Size", c, ValueType::Int, true, |_| {
            no_rust_api("PCB_GENERATED_TABLE::GetFontSizeProp")
        })
        .with_display(PropertyDisplay::PtSize),
        font_props,
    )
    .set_is_copyable()
    .set_indeterminate_func();
}

/// The drill-chart members (`show_totals`, `filter`).
fn chart<'a>(it: &ItemRef<'a>) -> (&'a bool, &'a crate::pcb_generated_table::DrillChartFilter) {
    match &generated(it).kind {
        GeneratedTableKind::DrillChart {
            show_totals,
            filter,
            ..
        } => (show_totals, filter),
        _ => unreachable!("PCB_DRILL_CHART property on a drill chart"),
    }
}

fn cbool(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbDrillChart, ValueType::Bool, true, get)
}

/// `PCB_DRILL_CHART_DESC::PCB_DRILL_CHART_DESC()`.
pub(crate) fn register_pcb_drill_chart(pm: &mut PropertyManager) {
    let c = ClassId::PcbDrillChart;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardItemContainer);
    pm.add_type_cast(c, ClassId::PcbTable);
    pm.add_type_cast(c, ClassId::PcbGeneratedTable);
    pm.inherits_after(c, ClassId::PcbGeneratedTable);

    // _( "Drill Chart Properties" )
    let chart_props = "Drill Chart Properties";

    pm.add_property(
        cbool("Show Totals", |it| PropertyValue::Bool(*chart(&it).0)),
        chart_props,
    );

    let include_props = "Include Holes";

    pm.add_property(
        cbool("Plated Holes", |it| {
            PropertyValue::Bool(chart(&it).1.plated)
        }),
        include_props,
    );
    pm.add_property(
        cbool("Non-plated Holes", |it| {
            PropertyValue::Bool(chart(&it).1.non_plated)
        }),
        include_props,
    );
    pm.add_property(
        cbool("Vias", |it| PropertyValue::Bool(chart(&it).1.vias)),
        include_props,
    );
    pm.add_property(
        cbool("Slots", |it| PropertyValue::Bool(chart(&it).1.slots)),
        include_props,
    );
    pm.add_property(
        cbool("Backdrills", |it| {
            PropertyValue::Bool(chart(&it).1.backdrills)
        }),
        include_props,
    );
    pm.add_property(
        cbool("Castellated", |it| {
            PropertyValue::Bool(chart(&it).1.castellated)
        }),
        include_props,
    );
}

/// `PCB_STACKUP_TABLE_DESC::PCB_STACKUP_TABLE_DESC()`.
pub(crate) fn register_pcb_stackup_table(pm: &mut PropertyManager) {
    let c = ClassId::PcbStackupTable;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardItemContainer);
    pm.add_type_cast(c, ClassId::PcbTable);
    pm.add_type_cast(c, ClassId::PcbGeneratedTable);
    pm.inherits_after(c, ClassId::PcbGeneratedTable);
}
