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

//! Port of `PAD_DESC` (`pcbnew/pad.cpp`) and the pad property adapters (`GetSizeXProp`,
//! `GetDrillSizeXProp`, `GetFrontPostMachiningMode`, ...). The pad enum maps are in
//! [`super::enums`].

use kicad_core::PcbLayerId;

use crate::item_ref::ItemRef;
use crate::pad::Pad;
use crate::pad_utils::pad_has_meaningful_rounding_radius;
use crate::padstack::{
    BackdrillMode, PadAttrib, PadDrillPostMachiningMode, PadDrillShape, PadShape, PadstackMode,
};

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
    layer_value,
};

/// `PADSTACK::ALL_LAYERS`.
const ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;

/// `static_cast<PAD*>` of an item registered under `PAD`.
fn pad<'a>(it: &ItemRef<'a>) -> &'a Pad {
    it.as_pad().expect("PAD property on a pad")
}

/// `isCopperPad`.
fn is_copper_pad(it: ItemRef<'_>) -> bool {
    it.as_pad()
        .is_some_and(|p| p.attribute() != PadAttrib::Npth)
}

/// `padCanHaveHole`.
fn pad_can_have_hole(it: ItemRef<'_>) -> bool {
    it.as_pad()
        .is_some_and(|p| p.attribute() == PadAttrib::Pth || p.attribute() == PadAttrib::Npth)
}

/// `hasNormalPadstack`.
fn has_normal_padstack(it: ItemRef<'_>) -> bool {
    match it.as_pad() {
        Some(p) => p.padstack.mode() == PadstackMode::Normal,
        None => true,
    }
}

/// The availability lambda of `Size Y`.
fn size_y_available(it: ItemRef<'_>) -> bool {
    match it.as_pad() {
        Some(p) => {
            // Custom padstacks can't have size modified through panel
            if p.padstack.mode() != PadstackMode::Normal {
                return false;
            }

            // Circle pads have no usable y-size
            p.shape(ALL_LAYERS) != PadShape::Circle
        }
        None => true,
    }
}

/// `hasRoundRadius`.
fn has_round_radius(it: ItemRef<'_>) -> bool {
    match it.as_pad() {
        Some(p) => {
            // Custom padstacks can't have this property modified through panel
            if p.padstack.mode() != PadstackMode::Normal {
                return false;
            }

            pad_has_meaningful_rounding_radius(it.ctx, p, PcbLayerId::FCu)
        }
        None => false,
    }
}

/// The availability lambda of `Hole Size Y`: circle holes have no usable y-size.
fn hole_size_y_available(it: ItemRef<'_>) -> bool {
    it.as_pad()
        .is_none_or(|p| p.drill_shape() != PadDrillShape::Circle)
}

/// `GetDrillShape() == PAD_DRILL_SHAPE::CIRCLE`.
fn has_circle_hole(it: ItemRef<'_>) -> bool {
    it.as_pad()
        .is_some_and(|p| p.drill_shape() == PadDrillShape::Circle)
}

/// The post-machining availability lambdas: a circle hole whose front (`front`) or back
/// post-machining mode is one of `modes`.
fn post_machining_is(it: ItemRef<'_>, front: bool, modes: &[PadDrillPostMachiningMode]) -> bool {
    let Some(p) = it.as_pad() else {
        return false;
    };

    if p.drill_shape() != PadDrillShape::Circle {
        return false;
    }

    let pm = if front {
        p.padstack.front_post_machining()
    } else {
        p.padstack.back_post_machining()
    };

    pm.mode.is_some_and(|m| modes.contains(&m))
}

const CB_OR_CS: &[PadDrillPostMachiningMode] = &[
    PadDrillPostMachiningMode::Counterbore,
    PadDrillPostMachiningMode::Countersink,
];
const CB: &[PadDrillPostMachiningMode] = &[PadDrillPostMachiningMode::Counterbore];
const CS: &[PadDrillPostMachiningMode] = &[PadDrillPostMachiningMode::Countersink];

/// The backdrill availability lambdas: a circle hole with one of the backdrill modes.
fn backdrill_is(it: ItemRef<'_>, modes: &[BackdrillMode]) -> bool {
    let Some(p) = it.as_pad() else {
        return false;
    };

    if p.drill_shape() != PadDrillShape::Circle {
        return false;
    }

    modes.contains(&p.padstack.backdrill_mode())
}

/// `GetFrontPostMachiningMode` / `GetBackPostMachiningMode`: the mode or `NOT_POST_MACHINED`.
fn post_machining_mode(p: &Pad, front: bool) -> PropertyValue {
    let pm = if front {
        p.padstack.front_post_machining()
    } else {
        p.padstack.back_post_machining()
    };
    let mode = pm
        .mode
        .unwrap_or(PadDrillPostMachiningMode::NotPostMachined);
    PropertyValue::Enum(EnumId::PadDrillPostMachiningMode, mode as i32)
}

fn int(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::Pad, ValueType::Int, true, get)
}

fn opt_int(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::Pad, ValueType::OptInt, true, get)
        .with_display(PropertyDisplay::PtSize)
}

fn enum_prop(name: &'static str, e: EnumId, get: super::Getter) -> Property {
    Property::new_enum(name, ClassId::Pad, ValueType::Enum(e), true, get)
}

/// `PAD_DESC::PAD_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    pm.register_type(ClassId::Pad);
    pm.add_type_cast(ClassId::Pad, ClassId::BoardItem);
    pm.add_type_cast(ClassId::Pad, ClassId::BoardConnectedItem);
    pm.inherits_after(ClassId::Pad, ClassId::BoardItem);
    pm.inherits_after(ClassId::Pad, ClassId::BoardConnectedItem);

    pm.mask(ClassId::Pad, ClassId::BoardConnectedItem, "Layer");
    pm.mask(ClassId::Pad, ClassId::BoardItem, "Locked");

    pm.add_property(
        Property::new("Orientation", ClassId::Pad, ValueType::Double, true, |it| {
            PropertyValue::Double(pad(&it).orientation_degrees(it.ctx))
        })
        .with_display(PropertyDisplay::PtDegree),
        "",
    );

    pm.override_availability(
        ClassId::Pad,
        ClassId::BoardConnectedItem,
        "Net",
        is_copper_pad,
    );
    pm.override_availability(
        ClassId::Pad,
        ClassId::BoardConnectedItem,
        "Net Class",
        is_copper_pad,
    );

    let group_pad = "Pad Properties";
    let group_post_machining = "Post-machining Properties";
    let group_backdrill = "Backdrill Properties";

    pm.add_property(
        enum_prop("Pad Type", EnumId::PadAttrib, |it| {
            PropertyValue::Enum(EnumId::PadAttrib, pad(&it).attribute() as i32)
        }),
        group_pad,
    );

    pm.add_property(
        enum_prop("Pad Shape", EnumId::PadShape, |it| {
            PropertyValue::Enum(EnumId::PadShape, pad(&it).shape(PcbLayerId::FCu) as i32)
        }),
        group_pad,
    )
    .set_available_func(has_normal_padstack);

    pm.add_property(
        Property::new("Pad Number", ClassId::Pad, ValueType::String, true, |it| {
            PropertyValue::String(pad(&it).number().to_string())
        }),
        group_pad,
    )
    .set_available_func(is_copper_pad);

    pm.add_property(
        Property::new("Pin Name", ClassId::Pad, ValueType::String, true, |it| {
            PropertyValue::String(pad(&it).pin_function().to_string())
        }),
        group_pad,
    )
    .set_is_hidden_from_library_editors();

    pm.add_property(
        Property::new("Pin Type", ClassId::Pad, ValueType::String, true, |it| {
            PropertyValue::String(pad(&it).pin_type().to_string())
        }),
        group_pad,
    )
    .set_is_hidden_from_library_editors()
    .set_choices_func();

    pm.add_property(
        enum_prop(
            "Simulation Electrical Type",
            EnumId::PadSimElectricalType,
            |it| {
                PropertyValue::Enum(
                    EnumId::PadSimElectricalType,
                    pad(&it).sim_electrical_type() as i32,
                )
            },
        ),
        group_pad,
    );

    pm.add_property(
        int("Size X", |it| {
            PropertyValue::Int(pad(&it).size(it.ctx, ALL_LAYERS).x)
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_available_func(has_normal_padstack);
    pm.add_property(
        int("Size Y", |it| {
            PropertyValue::Int(pad(&it).size(it.ctx, ALL_LAYERS).y)
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_available_func(size_y_available);

    pm.add_property(
        Property::new(
            "Corner Radius Ratio",
            ClassId::Pad,
            ValueType::Double,
            true,
            |it| PropertyValue::Double(pad(&it).padstack.round_rect_radius_ratio(PcbLayerId::FCu)),
        ),
        group_pad,
    )
    .set_available_func(has_round_radius);

    pm.add_property(
        int("Corner Radius Size", |it| {
            PropertyValue::Int(pad(&it).front_round_rect_radius_size(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_available_func(has_round_radius);

    pm.add_property(
        enum_prop("Hole Shape", EnumId::PadDrillShape, |it| {
            PropertyValue::Enum(EnumId::PadDrillShape, pad(&it).drill_shape() as i32)
        }),
        group_pad,
    )
    .set_writeable_func(pad_can_have_hole);

    pm.add_property(
        int("Hole Size X", |it| {
            PropertyValue::Int(pad(&it).drill_size(it.ctx).x)
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_validator();

    pm.add_property(
        int("Hole Size Y", |it| {
            PropertyValue::Int(pad(&it).drill_size(it.ctx).y)
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_validator()
    .set_available_func(hole_size_y_available);

    pm.add_property(
        enum_prop(
            "Top Post-machining",
            EnumId::PadDrillPostMachiningMode,
            |it| post_machining_mode(pad(&it), true),
        ),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(has_circle_hole);

    pm.add_property(
        int("Top Post-machining Size", |it| {
            PropertyValue::Int(pad(&it).padstack.front_post_machining().size)
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, true, CB_OR_CS));

    pm.add_property(
        int("Top Counterbore Depth", |it| {
            PropertyValue::Int(pad(&it).padstack.front_post_machining().depth)
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, true, CB));

    pm.add_property(
        int("Top Countersink Angle", |it| {
            PropertyValue::Int(pad(&it).padstack.front_post_machining().angle)
        })
        .with_display(PropertyDisplay::PtDecidegree),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, true, CS));

    pm.add_property(
        enum_prop(
            "Bottom Post-machining",
            EnumId::PadDrillPostMachiningMode,
            |it| post_machining_mode(pad(&it), false),
        ),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(has_circle_hole);

    pm.add_property(
        int("Bottom Post-machining Size", |it| {
            PropertyValue::Int(pad(&it).padstack.back_post_machining().size)
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, false, CB_OR_CS));

    pm.add_property(
        int("Bottom Counterbore Depth", |it| {
            PropertyValue::Int(pad(&it).padstack.back_post_machining().depth)
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, false, CB));

    pm.add_property(
        int("Bottom Countersink Angle", |it| {
            PropertyValue::Int(pad(&it).padstack.back_post_machining().angle)
        })
        .with_display(PropertyDisplay::PtDecidegree),
        group_post_machining,
    )
    .set_writeable_func(pad_can_have_hole)
    .set_available_func(|it| post_machining_is(it, false, CS));

    pm.add_property(
        enum_prop("Backdrill Mode", EnumId::BackdrillMode, |it| {
            PropertyValue::Enum(
                EnumId::BackdrillMode,
                pad(&it).padstack.backdrill_mode() as i32,
            )
        }),
        group_backdrill,
    );

    const BOTTOM: &[BackdrillMode] =
        &[BackdrillMode::BackdrillBottom, BackdrillMode::BackdrillBoth];
    const TOP: &[BackdrillMode] = &[BackdrillMode::BackdrillTop, BackdrillMode::BackdrillBoth];

    pm.add_property(
        opt_int("Bottom Backdrill Size", |it| {
            PropertyValue::OptInt(pad(&it).padstack.backdrill_size(false))
        }),
        group_backdrill,
    )
    .set_available_func(|it| backdrill_is(it, BOTTOM));

    pm.add_property(
        enum_prop("Bottom Backdrill Must-Cut", EnumId::PcbLayerId, |it| {
            layer_value(pad(&it).padstack.backdrill_end_layer(false))
        }),
        group_backdrill,
    )
    .set_available_func(|it| backdrill_is(it, BOTTOM));

    pm.add_property(
        opt_int("Top Backdrill Size", |it| {
            PropertyValue::OptInt(pad(&it).padstack.backdrill_size(true))
        }),
        group_backdrill,
    )
    .set_available_func(|it| backdrill_is(it, TOP));

    pm.add_property(
        enum_prop("Top Backdrill Must-Cut", EnumId::PcbLayerId, |it| {
            layer_value(pad(&it).padstack.backdrill_end_layer(true))
        }),
        group_backdrill,
    )
    .set_available_func(|it| backdrill_is(it, TOP));

    pm.add_property(
        enum_prop("Fabrication Property", EnumId::PadProp, |it| {
            PropertyValue::Enum(EnumId::PadProp, pad(&it).property() as i32)
        }),
        group_pad,
    );

    pm.add_property(
        enum_prop("Copper Layers", EnumId::UnconnectedLayerMode, |it| {
            PropertyValue::Enum(
                EnumId::UnconnectedLayerMode,
                pad(&it).unconnected_layer_mode() as i32,
            )
        }),
        group_pad,
    );

    pm.add_property(
        int("Pad To Die Length", |it| {
            PropertyValue::Int(pad(&it).pad_to_die_length())
        })
        .with_display(PropertyDisplay::PtSize),
        group_pad,
    )
    .set_available_func(is_copper_pad);

    pm.add_property(
        int("Pad To Die Delay", |it| {
            PropertyValue::Int(pad(&it).pad_to_die_delay())
        })
        .with_display(PropertyDisplay::PtTime),
        group_pad,
    )
    .set_available_func(is_copper_pad);

    let group_overrides = "Overrides";

    pm.add_property(
        opt_int("Clearance Override", |it| {
            PropertyValue::OptInt(pad(&it).local_clearance())
        }),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        opt_int("Soldermask Margin Override", |it| {
            PropertyValue::OptInt(pad(&it).local_solder_mask_margin())
        }),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        opt_int("Solderpaste Margin Override", |it| {
            PropertyValue::OptInt(pad(&it).local_solder_paste_margin())
        }),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new(
            "Solderpaste Margin Ratio Override",
            ClassId::Pad,
            ValueType::OptDouble,
            true,
            |it| PropertyValue::OptDouble(pad(&it).local_solder_paste_margin_ratio()),
        )
        .with_display(PropertyDisplay::PtRatio),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        enum_prop("Zone Connection Style", EnumId::ZoneConnection, |it| {
            PropertyValue::Enum(EnumId::ZoneConnection, pad(&it).local_zone_connection_raw())
        }),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        opt_int("Thermal Relief Spoke Width", |it| {
            PropertyValue::OptInt(pad(&it).local_thermal_spoke_width_override())
        }),
        group_overrides,
    )
    .set_validator()
    .set_is_copyable();

    pm.add_property(
        Property::new(
            "Thermal Relief Spoke Angle",
            ClassId::Pad,
            ValueType::Double,
            true,
            |it| PropertyValue::Double(pad(&it).thermal_spoke_angle_degrees()),
        )
        .with_display(PropertyDisplay::PtDegree),
        group_overrides,
    )
    .set_is_copyable();

    pm.add_property(
        opt_int("Thermal Relief Gap", |it| {
            PropertyValue::OptInt(pad(&it).local_thermal_gap())
        }),
        group_overrides,
    )
    .set_validator()
    .set_is_copyable();

    // TODO delta, drill shape offset, layer set
}
