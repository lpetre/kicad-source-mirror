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

//! Port of `BOARD_ITEM_DESC` (`pcbnew/board_item.cpp`) and `BOARD_CONNECTED_ITEM_DESC`
//! (`pcbnew/board_connected_item.cpp`).

use crate::board_connected_item::netname;
use crate::item::BoardUse;
use crate::item_ref::ItemRef;
use crate::traits::BoardConnectedItem;

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue,
    ValueType, layer_value, no_rust_api,
};

/// `BOARD_ITEM::GetParentAsString`: the parent footprint's reference, else the parent's UUID.
fn parent_as_string(it: ItemRef<'_>) -> String {
    match it.parent() {
        Some(p) => match p.as_footprint() {
            Some(fp) => fp.reference(it.ctx),
            None => p.uuid().as_string(),
        },
        // m_parent is the BOARD (whose UUID is generated at construction) or null (crash).
        None => no_rust_api("BOARD::m_Uuid"),
    }
}

/// The `isNotFootprintHolder` lambda of `BOARD_ITEM_DESC`.
pub(crate) fn board_item_is_not_footprint_holder(it: ItemRef<'_>) -> bool {
    it.has_board() && it.ctx.board.board_use != BoardUse::FpHolder
}

/// `BOARD_ITEM_DESC::BOARD_ITEM_DESC()` (the `PCB_LAYER_ID` map is in [`super::enums`]).
pub(crate) fn register_board_item(pm: &mut PropertyManager) {
    pm.register_type(ClassId::BoardItem);
    pm.inherits_after(ClassId::BoardItem, ClassId::EdaItem);

    pm.add_property(
        Property::new(
            "Parent",
            ClassId::BoardItem,
            ValueType::String,
            false,
            |it| PropertyValue::String(parent_as_string(it)),
        ),
        "",
    )
    .set_is_hidden_from_library_editors()
    .set_is_hidden_from_properties_manager();

    pm.add_property(
        Property::new(
            "Position X",
            ClassId::BoardItem,
            ValueType::Int,
            true,
            |it| PropertyValue::Int(it.position().x),
        )
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsXCoord),
        "",
    );
    pm.add_property(
        Property::new(
            "Position Y",
            ClassId::BoardItem,
            ValueType::Int,
            true,
            |it| PropertyValue::Int(it.position().y),
        )
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsYCoord),
        "",
    );
    pm.add_property(
        Property::new_enum(
            "Layer",
            ClassId::BoardItem,
            ValueType::Enum(EnumId::PcbLayerId),
            true,
            |it| layer_value(it.layer()),
        ),
        "",
    )
    .set_is_copyable();
    pm.add_property(
        Property::new("Locked", ClassId::BoardItem, ValueType::Bool, true, |it| {
            PropertyValue::Bool(it.is_locked())
        }),
        "",
    )
    .set_available_func(board_item_is_not_footprint_holder);
}

/// `static_cast<BOARD_CONNECTED_ITEM*>` of an item registered under a connected class.
fn bci(it: ItemRef<'_>) -> &dyn BoardConnectedItem {
    it.as_connected()
        .expect("BOARD_CONNECTED_ITEM property on a connected item")
}

/// The `supportsTeardrops` lambda.
fn supports_teardrops(it: ItemRef<'_>) -> bool {
    if it.as_connected().is_some() {
        if !it.has_board() || it.ctx.board.legacy_teardrops {
            return false;
        }

        return matches!(
            it.type_(),
            kicad_core::KicadT::PcbPadT | kicad_core::KicadT::PcbViaT
        );
    }

    false
}

/// The `supportsTeardropPreferZoneSetting` lambda.
fn supports_teardrop_prefer_zone_setting(it: ItemRef<'_>) -> bool {
    if it.as_connected().is_some() {
        if !it.has_board() || it.ctx.board.legacy_teardrops {
            return false;
        }

        return it.type_() == kicad_core::KicadT::PcbPadT;
    }

    false
}

/// `m_teardropParams` of a connected item.
fn td(it: ItemRef<'_>) -> &crate::teardrop_parameters::TeardropParameters {
    &bci(it)
        .connected()
        .expect("connected item base")
        .teardrop_params
}

/// `BOARD_CONNECTED_ITEM_DESC::BOARD_CONNECTED_ITEM_DESC()`.
pub(crate) fn register_board_connected_item(pm: &mut PropertyManager) {
    pm.register_type(ClassId::BoardConnectedItem);
    pm.inherits_after(ClassId::BoardConnectedItem, ClassId::BoardItem);

    // Replace layer property as the properties panel will set a restriction for copper layers
    // only for BOARD_CONNECTED_ITEM that we don't want to apply to BOARD_ITEM
    let layer_choices = EnumId::PcbLayerId.map().choices_owned();
    pm.replace_property(
        ClassId::BoardItem,
        "Layer",
        Property::new_enum(
            "Layer",
            ClassId::BoardConnectedItem,
            ValueType::Enum(EnumId::PcbLayerId),
            true,
            |it| layer_value(it.layer()),
        ),
        "",
    )
    .set_is_copyable()
    .set_choices(layer_choices);

    pm.add_property(
        Property::new_enum(
            "Net",
            ClassId::BoardConnectedItem,
            ValueType::Int,
            true,
            |it| PropertyValue::Int(bci(it).net_code().0),
        )
        .with_display(PropertyDisplay::PtNet),
        "",
    )
    .set_is_hidden_from_rules_editor()
    .set_is_hidden_from_library_editors();

    // GetNetClassName: GetEffectiveNetClass()->GetName() (the composite name of a multi-class
    // net, e.g. "L,Default").

    // This property should just be an alias for the one below, it only exists so that we
    // maintain compatibility with both `NetClass` and `Net_Class` in custom rules.
    // It has the name we would show in the GUI if we wanted to show this in the GUI, but we
    // don't at the moment because there is no way to edit the netclass of a net from a selected
    // connected item, and showing it makes users think they can change it.
    pm.add_property(
        Property::new(
            "Net Class",
            ClassId::BoardConnectedItem,
            ValueType::String,
            false,
            |it| PropertyValue::String(bci(it).effective_netclass(it.ctx).get_name()),
        ),
        "",
    )
    .set_is_hidden_from_rules_editor()
    .set_is_hidden_from_properties_manager()
    .set_is_hidden_from_library_editors();

    // Compatibility alias for DRC engine
    pm.add_property(
        Property::new(
            "NetClass",
            ClassId::BoardConnectedItem,
            ValueType::String,
            false,
            |it| PropertyValue::String(bci(it).effective_netclass(it.ctx).get_name()),
        ),
        "",
    )
    .set_is_hidden_from_properties_manager()
    .set_is_hidden_from_library_editors();

    // Used only in DRC engine
    pm.add_property(
        Property::new(
            "NetName",
            ClassId::BoardConnectedItem,
            ValueType::String,
            false,
            |it| PropertyValue::String(netname(bci(it), it.ctx).to_string()),
        ),
        "",
    )
    .set_is_hidden_from_properties_manager()
    .set_is_hidden_from_library_editors();

    let group_teardrops = "Teardrops";
    let bc = ClassId::BoardConnectedItem;

    pm.add_property(
        Property::new("Enable Teardrops", bc, ValueType::Bool, true, |it| {
            PropertyValue::Bool(td(it).enabled)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);

    pm.add_property(
        Property::new("Best Length Ratio", bc, ValueType::Double, true, |it| {
            PropertyValue::Double(td(it).best_length_ratio)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrops)
    .set_validator();

    pm.add_property(
        Property::new("Max Length", bc, ValueType::Int, true, |it| {
            PropertyValue::Int(td(it).td_max_len)
        })
        .with_display(PropertyDisplay::PtSize),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);

    pm.add_property(
        Property::new("Best Width Ratio", bc, ValueType::Double, true, |it| {
            PropertyValue::Double(td(it).best_width_ratio)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrops)
    .set_validator();

    pm.add_property(
        Property::new("Max Width", bc, ValueType::Int, true, |it| {
            PropertyValue::Int(td(it).td_max_width)
        })
        .with_display(PropertyDisplay::PtSize),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);

    pm.add_property(
        Property::new("Curved Teardrops", bc, ValueType::Bool, true, |it| {
            PropertyValue::Bool(td(it).curved_edges)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);

    pm.add_property(
        Property::new("Prefer Zone Connections", bc, ValueType::Bool, true, |it| {
            PropertyValue::Bool(!td(it).td_on_pads_in_zones)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrop_prefer_zone_setting);

    pm.add_property(
        Property::new(
            "Allow Teardrops To Span Two Tracks",
            bc,
            ValueType::Bool,
            true,
            |it| PropertyValue::Bool(td(it).allow_use_two_tracks),
        ),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);

    pm.add_property(
        Property::new("Max Width Ratio", bc, ValueType::Double, true, |it| {
            PropertyValue::Double(td(it).width_to_size_filter_ratio)
        }),
        group_teardrops,
    )
    .set_available_func(supports_teardrops);
}
