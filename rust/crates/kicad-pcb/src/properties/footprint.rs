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

//! Port of `FOOTPRINT_DESC` (`pcbnew/footprint.cpp`). The `FOOTPRINT_TYPE` map is in
//! [`super::enums`].

use kicad_core::PcbLayerId;

use crate::footprint::{Footprint, FootprintType};
use crate::item::BoardUse;
use crate::item_ref::ItemRef;

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
    layer_value,
};

/// `static_cast<FOOTPRINT*>`.
fn fp<'a>(it: &ItemRef<'a>) -> &'a Footprint {
    it.as_footprint()
        .expect("FOOTPRINT property on a footprint")
}

/// `isNotFootprintHolder` (the `FOOTPRINT_DESC` version: true without a board).
fn is_not_footprint_holder(it: ItemRef<'_>) -> bool {
    if it.as_footprint().is_some() && it.has_board() {
        return it.ctx.board.board_use != BoardUse::FpHolder;
    }

    true
}

/// `FOOTPRINT_TYPE` as its C++ value.
fn footprint_type_value(t: FootprintType) -> i32 {
    match t {
        FootprintType::Unspecified => 0,
        FootprintType::ThroughHole => 1,
        FootprintType::Smd => 2,
    }
}

fn boolean(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::Footprint, ValueType::Bool, true, get)
}

fn string(name: &'static str, has_setter: bool, get: super::Getter) -> Property {
    Property::new(name, ClassId::Footprint, ValueType::String, has_setter, get)
}

/// `FOOTPRINT_DESC::FOOTPRINT_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    let c = ClassId::Footprint;

    // footprints might be placed only on F.Cu & B.Cu
    let fp_layers = vec![
        (
            i32::from(PcbLayerId::FCu),
            kicad_core::Lset::name(PcbLayerId::FCu),
        ),
        (
            i32::from(PcbLayerId::BCu),
            kicad_core::Lset::name(PcbLayerId::BCu),
        ),
    ];

    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::BoardItemContainer);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::BoardItemContainer);

    pm.replace_property(
        ClassId::BoardItem,
        "Layer",
        Property::new_enum(
            "Layer",
            c,
            ValueType::Enum(EnumId::PcbLayerId),
            true,
            |it| layer_value(it.layer()),
        ),
        "",
    )
    .set_available_func(is_not_footprint_holder)
    .set_choices(fp_layers);

    pm.add_property(
        Property::new("Orientation", c, ValueType::Double, true, |it| {
            PropertyValue::Double(fp(&it).transform().rotate().as_degrees())
        })
        .with_display(PropertyDisplay::PtDegree),
        "",
    )
    .set_available_func(is_not_footprint_holder);

    pm.add_property(
        Property::new("Scale X", c, ValueType::Double, true, |it| {
            PropertyValue::Double(fp(&it).transform().scale_x())
        }),
        "",
    )
    .set_available_func(is_not_footprint_holder);

    pm.add_property(
        Property::new("Scale Y", c, ValueType::Double, true, |it| {
            PropertyValue::Double(fp(&it).transform().scale_y())
        }),
        "",
    )
    .set_available_func(is_not_footprint_holder);

    let group_fields = "Fields";

    pm.add_property(
        string("Reference", true, |it| {
            PropertyValue::String(fp(&it).reference(it.ctx))
        }),
        group_fields,
    );

    let property_fields = "Footprint Properties";

    pm.add_property(
        string("Library Link", false, |it| {
            PropertyValue::String(fp(&it).fpid_as_string())
        }),
        property_fields,
    );
    pm.add_property(
        string("Library Description", false, |it| {
            PropertyValue::String(fp(&it).lib_description.clone())
        }),
        property_fields,
    );
    pm.add_property(
        string("Keywords", false, |it| {
            PropertyValue::String(fp(&it).keywords.clone())
        }),
        property_fields,
    );

    pm.add_property(
        Property::new("Height", c, ValueType::Int, true, |it| {
            PropertyValue::Int(fp(&it).height)
        })
        .with_display(PropertyDisplay::PtSize),
        property_fields,
    )
    .set_is_copyable();

    // Note: Also used by DRC engine
    pm.add_property(
        string("Component Class", false, |it| {
            PropertyValue::String(fp(&it).component_class_as_string(it.ctx))
        }),
        property_fields,
    )
    .set_is_hidden_from_library_editors();

    let group_attributes = "Attributes";

    pm.add_property(
        Property::new_enum(
            "Footprint Type",
            c,
            ValueType::Enum(EnumId::FootprintType),
            true,
            |it| {
                PropertyValue::Enum(
                    EnumId::FootprintType,
                    footprint_type_value(fp(&it).footprint_type()),
                )
            },
        ),
        group_attributes,
    );
    pm.add_property(
        boolean("Not in Schematic", |it| {
            PropertyValue::Bool(fp(&it).is_board_only())
        }),
        group_attributes,
    );
    pm.add_property(
        boolean("Exclude From Position Files", |it| {
            PropertyValue::Bool(fp(&it).is_excluded_from_pos_files())
        }),
        group_attributes,
    );
    pm.add_property(
        boolean("Exclude From Bill of Materials", |it| {
            PropertyValue::Bool(fp(&it).is_excluded_from_bom())
        }),
        group_attributes,
    );
    pm.add_property(
        boolean("Exclude From Simulation", |it| {
            PropertyValue::Bool(fp(&it).is_excluded_from_sim())
        }),
        group_attributes,
    );
    pm.add_property(
        boolean("Do not Populate", |it| {
            PropertyValue::Bool(fp(&it).is_dnp())
        }),
        group_attributes,
    );

    let group_overrides = "Overrides";

    pm.add_property(
        boolean("Exempt From Courtyard Requirement", |it| {
            PropertyValue::Bool(fp(&it).allow_missing_courtyard())
        }),
        group_overrides,
    );
    pm.add_property(
        Property::new("Clearance Override", c, ValueType::OptInt, true, |it| {
            PropertyValue::OptInt(fp(&it).local_clearance())
        })
        .with_display(PropertyDisplay::PtSize),
        group_overrides,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new(
            "Solderpaste Margin Override",
            c,
            ValueType::OptInt,
            true,
            |it| PropertyValue::OptInt(fp(&it).solder_paste_margin),
        )
        .with_display(PropertyDisplay::PtSize),
        group_overrides,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new(
            "Solderpaste Margin Ratio Override",
            c,
            ValueType::OptDouble,
            true,
            |it| PropertyValue::OptDouble(fp(&it).solder_paste_margin_ratio),
        )
        .with_display(PropertyDisplay::PtRatio),
        group_overrides,
    )
    .set_is_copyable();
    pm.add_property(
        Property::new_enum(
            "Zone Connection Style",
            c,
            ValueType::Enum(EnumId::ZoneConnection),
            true,
            |it| {
                PropertyValue::Enum(
                    EnumId::ZoneConnection,
                    fp(&it).local_zone_connection_raw(),
                )
            },
        ),
        group_overrides,
    )
    .set_is_copyable();
}
