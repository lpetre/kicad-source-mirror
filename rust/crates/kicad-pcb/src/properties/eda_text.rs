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

//! Port of `EDA_TEXT_DESC` (`common/eda_text.cpp`) and the property adapters
//! `EDA_TEXT::GetFontProp`, `GetTextThicknessProperty` and `GetTextAngleDegrees`. The
//! `GR_TEXT_H_ALIGN_T` / `GR_TEXT_V_ALIGN_T` maps are in [`super::enums`].

use crate::eda_text::{EdaText, EdaTextExt};
use crate::item_ref::ItemRef;
use crate::traits::EdaTextItem;

use super::{ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType};

/// `KICAD_FONT_NAME`.
pub(crate) const KICAD_FONT_NAME: &str = "KiCad Font";

/// `dynamic_cast<EDA_TEXT*>( aItem )` of an item registered under a text class.
pub(crate) fn host<'a>(it: &ItemRef<'a>) -> &'a dyn EdaTextItem {
    it.as_eda_text().expect("EDA_TEXT property on a text item")
}

/// The `EDA_TEXT` data.
fn et<'a>(it: &ItemRef<'a>) -> &'a EdaText {
    host(it).eda_text()
}

/// `EDA_TEXT::GetFontProp`: the font's name, else `KICAD_FONT_NAME` on the board side.
pub(crate) fn font_prop(text: &EdaText) -> String {
    match &text.attributes.font {
        Some(name) => name.clone(),
        None => KICAD_FONT_NAME.to_string(),
    }
}

/// `EDA_TEXT::GetTextThicknessProperty`.
fn text_thickness_property(it: ItemRef<'_>) -> i32 {
    let h = host(&it);

    if h.auto_thickness(it.ctx) {
        h.effective_text_pen_width(it.ctx, 0)
    } else {
        h.text_thickness(it.ctx)
    }
}

/// The `isField` lambda.
fn is_field(it: ItemRef<'_>) -> bool {
    it.type_() == kicad_core::KicadT::PcbFieldT
}

/// `EDA_TEXT_DESC::EDA_TEXT_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    pm.register_type(ClassId::EdaText);
    let c = ClassId::EdaText;

    // GetTextAngleDegrees: m_attributes.m_Angle (not the virtual GetTextAngle).
    pm.add_property(
        Property::new("Orientation", c, ValueType::Double, true, |it| {
            PropertyValue::Double(et(&it).attributes.angle)
        })
        .with_display(PropertyDisplay::PtDegree),
        "",
    );

    let text_props = "Text Properties";

    pm.add_property(
        Property::new("Text", c, ValueType::String, true, |it| {
            PropertyValue::String(et(&it).text().to_string())
        }),
        text_props,
    );

    pm.add_property(
        Property::new("Font", c, ValueType::String, true, |it| {
            PropertyValue::String(font_prop(et(&it)))
        }),
        text_props,
    )
    .set_is_hidden_from_rules_editor()
    .set_choices_func()
    .set_is_copyable();

    pm.add_property(
        Property::new("Auto Thickness", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(host(&it).auto_thickness(it.ctx))
        }),
        text_props,
    );
    pm.add_property(
        Property::new("Thickness", c, ValueType::Int, true, |it| {
            PropertyValue::Int(text_thickness_property(it))
        })
        .with_display(PropertyDisplay::PtSize),
        text_props,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new("Italic", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(et(&it).is_italic())
        }),
        text_props,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new("Bold", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(et(&it).is_bold())
        }),
        text_props,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new("Mirrored", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(et(&it).is_mirrored())
        }),
        text_props,
    );

    pm.add_property(
        Property::new("Visible", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(et(&it).is_visible())
        }),
        text_props,
    )
    .set_available_func(is_field)
    .set_is_copyable();

    pm.add_property(
        Property::new("Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(host(&it).text_width(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        text_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Height", c, ValueType::Int, true, |it| {
            PropertyValue::Int(host(&it).text_height(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        text_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new_enum(
            "Horizontal Justification",
            c,
            ValueType::Enum(EnumId::GrTextHAlignT),
            true,
            |it| PropertyValue::Enum(EnumId::GrTextHAlignT, et(&it).horiz_justify() as i32),
        ),
        text_props,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new_enum(
            "Vertical Justification",
            c,
            ValueType::Enum(EnumId::GrTextVAlignT),
            true,
            |it| PropertyValue::Enum(EnumId::GrTextVAlignT, et(&it).vert_justify() as i32),
        ),
        text_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Color", c, ValueType::Color, true, |it| {
            PropertyValue::Color(et(&it).attributes.color)
        }),
        text_props,
    );

    pm.add_property(
        Property::new("Hyperlink", c, ValueType::String, true, |it| {
            PropertyValue::String(et(&it).hyperlink().to_string())
        }),
        text_props,
    );
}
