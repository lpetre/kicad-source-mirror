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

//! Port of `ZONE_DESC` (`pcbnew/zone.cpp`). The zone enum maps are in [`super::enums`].

use kicad_core::layer_ids::is_copper_layer;

use crate::item_ref::ItemRef;
use crate::zone::Zone;
use crate::zone_settings::{IslandRemovalMode, ThievingPattern, ZoneFillMode};

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue,
    ValueType, layer_value,
};

/// `static_cast<ZONE*>`.
fn zone<'a>(it: &ItemRef<'a>) -> &'a Zone {
    it.as_zone().expect("ZONE property on a zone")
}

/// `isCopperZone`.
fn is_copper_zone(it: ItemRef<'_>) -> bool {
    it.as_zone()
        .is_some_and(|z| !z.is_rule_area() && is_copper_layer(z.first_layer()))
}

/// `isNonThievingCopperZone`: hide net-bearing and hatch/island properties for
/// copper-thieving zones. Same predicate gates both kinds of properties.
fn is_non_thieving_copper_zone(it: ItemRef<'_>) -> bool {
    it.as_zone().is_some_and(|z| {
        !z.is_rule_area() && is_copper_layer(z.first_layer()) && !z.is_copper_thieving()
    })
}

/// `isRuleArea`.
fn is_rule_area(it: ItemRef<'_>) -> bool {
    it.as_zone().is_some_and(|z| z.is_rule_area())
}

/// `isHatchedFill`.
fn is_hatched_fill(it: ItemRef<'_>) -> bool {
    it.as_zone()
        .is_some_and(|z| z.fill_mode() == ZoneFillMode::HatchPattern)
}

/// `isThievingFill`.
fn is_thieving_fill(it: ItemRef<'_>) -> bool {
    it.as_zone().is_some_and(|z| z.is_copper_thieving())
}

/// `isThievingHatch`.
fn is_thieving_hatch(it: ItemRef<'_>) -> bool {
    it.as_zone().is_some_and(|z| {
        z.is_copper_thieving() && z.thieving_pattern() == ThievingPattern::Hatch
    })
}

/// `isThievingNonHatch`.
fn is_thieving_non_hatch(it: ItemRef<'_>) -> bool {
    it.as_zone().is_some_and(|z| {
        z.is_copper_thieving() && z.thieving_pattern() != ThievingPattern::Hatch
    })
}

/// `isAreaBasedIslandRemoval`.
fn is_area_based_island_removal(it: ItemRef<'_>) -> bool {
    it.as_zone()
        .is_some_and(|z| z.island_removal_mode == IslandRemovalMode::Area)
}

fn int(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::Zone, ValueType::Int, true, get)
}

fn boolean(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::Zone, ValueType::Bool, true, get)
}

/// `ZONE_DESC::ZONE_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    let c = ClassId::Zone;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardConnectedItem);

    pm.replace_property(
        ClassId::BoardItem,
        "Position X",
        Property::new("Position X", c, ValueType::Int, false, |it| {
            PropertyValue::Int(it.position().x)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsXCoord),
        "",
    )
    .set_is_hidden_from_properties_manager();

    pm.replace_property(
        ClassId::BoardItem,
        "Position Y",
        Property::new("Position Y", c, ValueType::Int, false, |it| {
            PropertyValue::Int(it.position().y)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsYCoord),
        "",
    )
    .set_is_hidden_from_properties_manager();

    // Visible for thieving zones only; ordinary zones use a layer set, not a single layer.
    pm.replace_property(
        ClassId::BoardConnectedItem,
        "Layer",
        Property::new_enum("Layer", c, ValueType::Enum(EnumId::PcbLayerId), true, |it| {
            layer_value(it.layer())
        }),
        "",
    )
    .set_available_func(is_thieving_fill);

    pm.override_availability(c, ClassId::BoardConnectedItem, "Net", is_non_thieving_copper_zone);
    pm.override_availability(
        c,
        ClassId::BoardConnectedItem,
        "Net Class",
        is_non_thieving_copper_zone,
    );

    pm.add_property(
        Property::new("Priority", c, ValueType::Unsigned, true, |it| {
            PropertyValue::Unsigned(zone(&it).assigned_priority())
        }),
        "",
    )
    .set_available_func(is_copper_zone);

    pm.add_property(
        Property::new("Name", c, ValueType::String, true, |it| {
            PropertyValue::String(zone(&it).zone_name().to_string())
        }),
        "",
    );

    let group_keepout = "Keepout";

    pm.add_property(
        boolean("Keep Out Tracks", |it| PropertyValue::Bool(zone(&it).do_not_allow_tracks)),
        group_keepout,
    )
    .set_available_func(is_rule_area)
    .set_is_copyable();

    pm.add_property(
        boolean("Keep Out Vias", |it| PropertyValue::Bool(zone(&it).do_not_allow_vias)),
        group_keepout,
    )
    .set_available_func(is_rule_area)
    .set_is_copyable();

    pm.add_property(
        boolean("Keep Out Pads", |it| PropertyValue::Bool(zone(&it).do_not_allow_pads)),
        group_keepout,
    )
    .set_available_func(is_rule_area)
    .set_is_copyable();

    pm.add_property(
        boolean("Keep Out Zone Fills", |it| {
            PropertyValue::Bool(zone(&it).do_not_allow_zone_fills)
        }),
        group_keepout,
    )
    .set_available_func(is_rule_area)
    .set_is_copyable();

    pm.add_property(
        boolean("Keep Out Footprints", |it| {
            PropertyValue::Bool(zone(&it).do_not_allow_footprints)
        }),
        group_keepout,
    )
    .set_available_func(is_rule_area)
    .set_is_copyable();

    let group_placement = "Placement";

    pm.add_property(
        boolean("Enable", |it| PropertyValue::Bool(zone(&it).placement_area_enabled)),
        group_placement,
    )
    .set_available_func(is_rule_area);

    pm.add_property(
        Property::new_enum(
            "Source Type",
            c,
            ValueType::Enum(EnumId::PlacementSourceT),
            true,
            |it| {
                PropertyValue::Enum(
                    EnumId::PlacementSourceT,
                    zone(&it).placement_area_source_type as i32,
                )
            },
        ),
        group_placement,
    )
    .set_available_func(is_rule_area);

    pm.add_property(
        Property::new("Source Name", c, ValueType::String, true, |it| {
            PropertyValue::String(zone(&it).placement_area_source.clone())
        }),
        group_placement,
    )
    .set_available_func(is_rule_area);

    let group_fill = "Fill Style";

    pm.add_property(
        Property::new_enum("Fill Mode", c, ValueType::Enum(EnumId::ZoneFillMode), true, |it| {
            PropertyValue::Enum(EnumId::ZoneFillMode, zone(&it).fill_mode() as i32)
        }),
        group_fill,
    )
    .set_available_func(is_copper_zone)
    .set_is_copyable();

    pm.add_property(
        Property::new("Hatch Orientation", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(zone(&it).hatch_orientation())
        })
        .with_display(PropertyDisplay::PtDegree),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_is_copyable();

    pm.add_property(
        int("Hatch Width", |it| PropertyValue::Int(zone(&it).hatch_thickness))
            .with_display(PropertyDisplay::PtSize),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_validator()
    .set_is_copyable();

    pm.add_property(
        int("Hatch Gap", |it| PropertyValue::Int(zone(&it).hatch_gap))
            .with_display(PropertyDisplay::PtSize),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_validator()
    .set_is_copyable();

    pm.add_property(
        Property::new("Hatch Minimum Hole Ratio", c, ValueType::Double, true, |it| {
            PropertyValue::Double(zone(&it).hatch_hole_min_area)
        }),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_validator()
    .set_is_copyable();

    // TODO: Smoothing effort needs to change to enum (in dialog too)
    pm.add_property(
        int("Smoothing Effort", |it| PropertyValue::Int(zone(&it).hatch_smoothing_level)),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_is_copyable();

    pm.add_property(
        Property::new("Smoothing Amount", c, ValueType::Double, true, |it| {
            PropertyValue::Double(zone(&it).hatch_smoothing_value)
        }),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_hatched_fill)
    .set_is_copyable();

    pm.add_property(
        Property::new_enum(
            "Thieving Pattern",
            c,
            ValueType::Enum(EnumId::ThievingPattern),
            true,
            |it| PropertyValue::Enum(EnumId::ThievingPattern, zone(&it).thieving_pattern() as i32),
        ),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_is_copyable();

    pm.add_property(
        int("Thieving Element Size", |it| {
            PropertyValue::Int(zone(&it).thieving_settings.element_size)
        })
        .with_display(PropertyDisplay::PtSize),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_writeable_func(is_thieving_non_hatch)
    .set_validator()
    .set_is_copyable();

    // Gap is meaningful for all three patterns: edge-to-edge spacing between
    // adjacent stamps for dots/squares, line-to-line edge spacing for hatch.
    pm.add_property(
        int("Thieving Gap", |it| PropertyValue::Int(zone(&it).thieving_settings.gap))
            .with_display(PropertyDisplay::PtSize),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_validator()
    .set_is_copyable();

    pm.add_property(
        int("Thieving Line Width", |it| {
            PropertyValue::Int(zone(&it).thieving_settings.line_width)
        })
        .with_display(PropertyDisplay::PtSize),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_writeable_func(is_thieving_hatch)
    .set_validator()
    .set_is_copyable();

    pm.add_property(
        boolean("Thieving Stagger", |it| {
            PropertyValue::Bool(zone(&it).thieving_settings.stagger)
        }),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_is_copyable();

    pm.add_property(
        Property::new("Thieving Orientation", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(zone(&it).thieving_orientation())
        })
        .with_display(PropertyDisplay::PtDegree),
        group_fill,
    )
    .set_available_func(is_thieving_fill)
    .set_is_copyable();

    pm.add_property(
        Property::new_enum(
            "Remove Islands",
            c,
            ValueType::Enum(EnumId::IslandRemovalMode),
            true,
            |it| {
                PropertyValue::Enum(EnumId::IslandRemovalMode, zone(&it).island_removal_mode as i32)
            },
        ),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_is_copyable();

    pm.add_property(
        Property::new("Minimum Island Area", c, ValueType::LongLong, true, |it| {
            PropertyValue::LongLong(zone(&it).min_island_area)
        })
        .with_display(PropertyDisplay::PtArea),
        group_fill,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_writeable_func(is_area_based_island_removal)
    .set_is_copyable();

    let group_electrical = "Electrical";

    pm.add_property(
        Property::new("Clearance", c, ValueType::OptInt, true, |it| {
            PropertyValue::OptInt(zone(&it).local_clearance())
        })
        .with_display(PropertyDisplay::PtSize),
        group_electrical,
    )
    .set_available_func(is_copper_zone)
    .set_validator();

    pm.add_property(
        int("Minimum Width", |it| PropertyValue::Int(zone(&it).min_thickness()))
            .with_display(PropertyDisplay::PtSize),
        group_electrical,
    )
    .set_available_func(is_copper_zone)
    .set_validator();

    // Pad connections and thermal-relief controls require a net to act on.
    // Thieving zones are netless and explicitly use ZONE_CONNECTION::NONE,
    // so hide these for them while keeping them visible for solid + hatched zones.

    pm.add_property(
        Property::new_enum(
            "Pad Connections",
            c,
            ValueType::Enum(EnumId::ZoneConnection),
            true,
            |it| PropertyValue::Enum(EnumId::ZoneConnection, zone(&it).pad_connection() as i32),
        ),
        group_electrical,
    )
    .set_available_func(is_non_thieving_copper_zone);

    pm.add_property(
        int("Thermal Relief Gap", |it| PropertyValue::Int(zone(&it).thermal_relief_gap()))
            .with_display(PropertyDisplay::PtSize),
        group_electrical,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_validator();

    pm.add_property(
        int("Thermal Relief Spoke Width", |it| {
            PropertyValue::Int(zone(&it).thermal_relief_spoke_width())
        })
        .with_display(PropertyDisplay::PtSize),
        group_electrical,
    )
    .set_available_func(is_non_thieving_copper_zone)
    .set_validator();
}
