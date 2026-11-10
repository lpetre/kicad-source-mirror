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

//! Port of `PCB_TEXT_DESC` (`pcbnew/pcb_text.cpp`), `PCB_TEXTBOX_DESC`
//! (`pcbnew/pcb_textbox.cpp`) and `PCB_FIELD_DESC` (`pcbnew/pcb_field.cpp`).

use crate::item_ref::ItemRef;
use crate::pcb_textbox::PcbTextbox;

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
    eda_text::host,
};

/// `BOARD_ITEM::IsKnockout` (virtual).
fn knockout(it: ItemRef<'_>) -> PropertyValue {
    PropertyValue::Bool(it.as_dyn().is_knockout())
}

/// `isFootprintText`.
fn is_footprint_text(it: ItemRef<'_>) -> bool {
    it.type_() == kicad_core::KicadT::PcbTextT && it.parent_footprint().is_some()
}

/// `PCB_TEXT_DESC::PCB_TEXT_DESC()`.
pub(crate) fn register_pcb_text(pm: &mut PropertyManager) {
    let c = ClassId::PcbText;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);

    pm.mask(c, ClassId::EdaText, "Color");

    pm.add_property(
        Property::new("Knockout", c, ValueType::Bool, true, knockout).with_base(ClassId::BoardItem),
        "Text Properties",
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Keep Upright", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(host(&it).eda_text().is_keep_upright())
        })
        .with_base(ClassId::EdaText),
        "Text Properties",
    )
    .set_is_copyable();

    pm.override_availability(c, ClassId::EdaText, "Keep Upright", is_footprint_text);

    pm.mask(c, ClassId::EdaText, "Hyperlink");
}

/// `static_cast<PCB_TEXTBOX*>` (also the text box part of a table cell).
fn textbox<'a>(it: &ItemRef<'a>) -> &'a PcbTextbox {
    it.as_textbox().expect("PCB_TEXTBOX property on a text box")
}

fn tb_int(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbTextbox, ValueType::Int, true, get)
        .with_display(PropertyDisplay::PtSize)
}

/// `PCB_TEXTBOX_DESC::PCB_TEXTBOX_DESC()`.
pub(crate) fn register_pcb_textbox(pm: &mut PropertyManager) {
    let c = ClassId::PcbTextbox;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbShape);
    pm.add_type_cast(c, ClassId::EdaShape);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbShape);
    pm.inherits_after(c, ClassId::EdaShape);
    pm.inherits_after(c, ClassId::EdaText);

    for name in [
        "Shape",
        "Start X",
        "Start Y",
        "End X",
        "End Y",
        "Width",
        "Height",
        "Line Width",
        "Line Style",
        "Filled",
        "Line Color",
        "Corner Radius",
    ] {
        pm.mask(c, ClassId::EdaShape, name);
    }

    pm.mask(c, ClassId::EdaText, "Color");

    pm.mask(c, ClassId::PcbShape, "Soldermask");
    pm.mask(c, ClassId::PcbShape, "Soldermask Margin Override");

    pm.add_property(
        Property::new("Knockout", c, ValueType::Bool, true, knockout).with_base(ClassId::BoardItem),
        "Text Properties",
    )
    .set_is_copyable();

    // _( "Border Properties" ): untranslated in the reference environment.
    let border_props = "Border Properties";

    pm.add_property(
        Property::new("Border", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(textbox(&it).is_border_enabled())
        }),
        border_props,
    )
    .set_is_copyable();

    // lineStyleGetter: the EDA_SHAPE::GetLineStyle PCB_TEXTBOX inherits.
    pm.add_property(
        Property::new_enum("Border Style", c, ValueType::Enum(EnumId::LineStyle), true, |it| {
            PropertyValue::Enum(EnumId::LineStyle, textbox(&it).shape.shape.line_style() as i32)
        }),
        border_props,
    )
    .set_is_copyable();

    pm.add_property(
        tb_int("Border Width", |it| PropertyValue::Int(textbox(&it).border_width())),
        border_props,
    )
    .set_is_copyable();

    // _( "Margins" )
    let margin_props = "Margins";

    pm.add_property(
        tb_int("Margin Left", |it| PropertyValue::Int(textbox(&it).margin_left)),
        margin_props,
    )
    .set_is_copyable();
    pm.add_property(
        tb_int("Margin Top", |it| PropertyValue::Int(textbox(&it).margin_top)),
        margin_props,
    )
    .set_is_copyable();
    pm.add_property(
        tb_int("Margin Right", |it| PropertyValue::Int(textbox(&it).margin_right)),
        margin_props,
    )
    .set_is_copyable();
    pm.add_property(
        tb_int("Margin Bottom", |it| PropertyValue::Int(textbox(&it).margin_bottom)),
        margin_props,
    )
    .set_is_copyable();

    pm.mask(c, ClassId::EdaText, "Hyperlink");
}

/// `PCB_FIELD_DESC::PCB_FIELD_DESC()`.
pub(crate) fn register_pcb_field(pm: &mut PropertyManager) {
    let c = ClassId::PcbField;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbText);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::EdaText);

    pm.add_property(
        Property::new("Name", c, ValueType::String, false, |it| match it.any() {
            crate::item_ref::AnyItem::Field(f) => PropertyValue::String(f.untranslated_name()),
            _ => unreachable!("PCB_FIELD property on a field"),
        }),
        "",
    )
    .set_is_hidden_from_library_editors()
    .set_is_hidden_from_properties_manager();

    // These properties, inherited from EDA_TEXT, have no sense for the board editor
    pm.mask(c, ClassId::EdaText, "Hyperlink");
    pm.mask(c, ClassId::EdaText, "Color");
}
