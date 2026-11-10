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

//! Port of `TRACK_VIA_DESC` (`pcbnew/pcb_track.cpp`): `PCB_TRACK`, `PCB_ARC` and `PCB_VIA`.
//! The via enum maps are in [`super::enums`].

use kicad_core::layer_ids::is_external_copper_layer;

use crate::item_ref::ItemRef;
use crate::padstack::{BackdrillMode, PadDrillPostMachiningMode};
use crate::pcb_track::{PcbTrack, PcbVia, Viatype};

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue,
    ValueType, layer_value,
};

/// `static_cast<PCB_TRACK*>`: the track part of a track, arc or via.
fn track<'a>(it: &ItemRef<'a>) -> &'a PcbTrack {
    it.as_track().expect("PCB_TRACK property on a track").as_track()
}

/// `static_cast<PCB_VIA*>`.
fn via<'a>(it: &ItemRef<'a>) -> &'a PcbVia {
    it.as_via().expect("PCB_VIA property on a via")
}

/// `isExternalLayerTrack`.
fn is_external_layer_track(it: ItemRef<'_>) -> bool {
    it.as_track().is_some() && is_external_copper_layer(it.layer())
}

/// The `Backdrill Mode` availability lambda.
fn via_backdrill_available(it: ItemRef<'_>) -> bool {
    if let Some(v) = it.as_via() {
        if v.via_type() == Viatype::Through {
            return true;
        }

        if v.padstack().backdrill_mode() != BackdrillMode::NoBackdrill {
            return true;
        }
    }

    false
}

fn via_backdrill_is(it: ItemRef<'_>, modes: &[BackdrillMode]) -> bool {
    it.as_via().is_some_and(|v| modes.contains(&v.backdrill_mode()))
}

const BOTTOM: &[BackdrillMode] = &[BackdrillMode::BackdrillBottom, BackdrillMode::BackdrillBoth];
const TOP: &[BackdrillMode] = &[BackdrillMode::BackdrillTop, BackdrillMode::BackdrillBoth];

fn via_post_machining_is(it: ItemRef<'_>, front: bool, modes: &[PadDrillPostMachiningMode]) -> bool {
    it.as_via().is_some_and(|v| {
        let mode = if front {
            v.front_post_machining()
        } else {
            v.back_post_machining()
        };

        mode.is_some_and(|m| modes.contains(&m))
    })
}

const CB_OR_CS: &[PadDrillPostMachiningMode] = &[
    PadDrillPostMachiningMode::Counterbore,
    PadDrillPostMachiningMode::Countersink,
];
const CB: &[PadDrillPostMachiningMode] = &[PadDrillPostMachiningMode::Counterbore];
const CS: &[PadDrillPostMachiningMode] = &[PadDrillPostMachiningMode::Countersink];

fn tint(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbTrack, ValueType::Int, true, get)
}

fn vint(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbVia, ValueType::Int, true, get)
}

fn venum(name: &'static str, e: EnumId, get: super::Getter) -> Property {
    Property::new_enum(name, ClassId::PcbVia, ValueType::Enum(e), true, get)
}

/// `TRACK_VIA_DESC::TRACK_VIA_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    // Track
    pm.register_type(ClassId::PcbTrack);
    pm.inherits_after(ClassId::PcbTrack, ClassId::BoardConnectedItem);

    pm.add_property(
        tint("Width", |it| PropertyValue::Int(track(&it).width()))
            .with_display(PropertyDisplay::PtSize),
        "",
    )
    .set_is_copyable();
    pm.replace_property(
        ClassId::BoardItem,
        "Position X",
        tint("Start X", |it| PropertyValue::Int(track(&it).start().x))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsXCoord),
        "",
    );
    pm.replace_property(
        ClassId::BoardItem,
        "Position Y",
        tint("Start Y", |it| PropertyValue::Int(track(&it).start().y))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsYCoord),
        "",
    );
    pm.add_property(
        tint("End X", |it| PropertyValue::Int(track(&it).end().x))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsXCoord),
        "",
    );
    pm.add_property(
        tint("End Y", |it| PropertyValue::Int(track(&it).end().y))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsYCoord),
        "",
    );

    let group_tech_layers = "Technical Layers";

    pm.add_property(
        Property::new("Soldermask", ClassId::PcbTrack, ValueType::Bool, true, |it| {
            PropertyValue::Bool(track(&it).has_solder_mask())
        }),
        group_tech_layers,
    )
    .set_available_func(is_external_layer_track)
    .set_is_copyable();
    pm.add_property(
        Property::new(
            "Soldermask Margin Override",
            ClassId::PcbTrack,
            ValueType::OptInt,
            true,
            |it| PropertyValue::OptInt(track(&it).local_solder_mask_margin()),
        )
        .with_display(PropertyDisplay::PtSize),
        group_tech_layers,
    )
    .set_available_func(is_external_layer_track)
    .set_is_copyable();

    // Arc
    pm.register_type(ClassId::PcbArc);
    pm.inherits_after(ClassId::PcbArc, ClassId::PcbTrack);

    // Via
    pm.register_type(ClassId::PcbVia);
    pm.inherits_after(ClassId::PcbVia, ClassId::BoardConnectedItem);

    let group_via = "Via Properties";
    let group_backdrill = "Backdrill";
    let group_post_machining = "Post-machining";

    pm.mask(ClassId::PcbVia, ClassId::BoardConnectedItem, "Layer");

    pm.add_property(
        Property::new(
            "Automatically Update Net",
            ClassId::PcbVia,
            ValueType::Bool,
            true,
            |it| PropertyValue::Bool(!via(&it).is_free()),
        ),
        "",
    );

    pm.add_property(
        vint("Diameter", |it| PropertyValue::Int(via(&it).front_width()))
            .with_display(PropertyDisplay::PtSize),
        group_via,
    )
    .set_validator()
    .set_is_copyable();
    pm.add_property(
        vint("Hole", |it| PropertyValue::Int(via(&it).drill_value(it.ctx)))
            .with_display(PropertyDisplay::PtSize),
        group_via,
    )
    .set_validator()
    .set_is_copyable();
    pm.add_property(
        venum("Layer Top", EnumId::PcbLayerId, |it| layer_value(it.layer())),
        group_via,
    )
    .set_validator();
    pm.add_property(
        venum("Layer Bottom", EnumId::PcbLayerId, |it| {
            layer_value(via(&it).bottom_layer())
        }),
        group_via,
    )
    .set_validator();
    pm.add_property(
        venum("Via Type", EnumId::Viatype, |it| {
            PropertyValue::Enum(EnumId::Viatype, via(&it).via_type() as i32)
        }),
        group_via,
    );
    pm.add_property(
        venum("Front Tenting", EnumId::TentingMode, |it| {
            PropertyValue::Enum(EnumId::TentingMode, via(&it).front_tenting_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Back Tenting", EnumId::TentingMode, |it| {
            PropertyValue::Enum(EnumId::TentingMode, via(&it).back_tenting_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Front Covering", EnumId::CoveringMode, |it| {
            PropertyValue::Enum(EnumId::CoveringMode, via(&it).front_covering_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Back Covering", EnumId::CoveringMode, |it| {
            PropertyValue::Enum(EnumId::CoveringMode, via(&it).back_covering_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Front Plugging", EnumId::PluggingMode, |it| {
            PropertyValue::Enum(EnumId::PluggingMode, via(&it).front_plugging_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Back Plugging", EnumId::PluggingMode, |it| {
            PropertyValue::Enum(EnumId::PluggingMode, via(&it).back_plugging_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Capping", EnumId::CappingMode, |it| {
            PropertyValue::Enum(EnumId::CappingMode, via(&it).capping_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();
    pm.add_property(
        venum("Filling", EnumId::FillingMode, |it| {
            PropertyValue::Enum(EnumId::FillingMode, via(&it).filling_mode() as i32)
        }),
        group_via,
    )
    .set_is_copyable();

    pm.add_property(
        venum("Backdrill Mode", EnumId::BackdrillMode, |it| {
            PropertyValue::Enum(EnumId::BackdrillMode, via(&it).backdrill_mode() as i32)
        }),
        group_backdrill,
    )
    .set_available_func(via_backdrill_available);

    pm.add_property(
        Property::new(
            "Bottom Backdrill Size",
            ClassId::PcbVia,
            ValueType::OptInt,
            true,
            |it| PropertyValue::OptInt(via(&it).bottom_backdrill_size()),
        )
        .with_display(PropertyDisplay::PtSize),
        group_backdrill,
    )
    .set_available_func(|it| via_backdrill_is(it, BOTTOM));

    pm.add_property(
        venum("Bottom Backdrill Must-Cut", EnumId::PcbLayerId, |it| {
            layer_value(via(&it).bottom_backdrill_layer())
        }),
        group_backdrill,
    )
    .set_available_func(|it| via_backdrill_is(it, BOTTOM));

    pm.add_property(
        Property::new(
            "Top Backdrill Size",
            ClassId::PcbVia,
            ValueType::OptInt,
            true,
            |it| PropertyValue::OptInt(via(&it).top_backdrill_size()),
        )
        .with_display(PropertyDisplay::PtSize),
        group_backdrill,
    )
    .set_available_func(|it| via_backdrill_is(it, TOP));

    pm.add_property(
        venum("Top Backdrill Must-Cut", EnumId::PcbLayerId, |it| {
            layer_value(via(&it).top_backdrill_layer())
        }),
        group_backdrill,
    )
    .set_available_func(|it| via_backdrill_is(it, TOP));

    pm.add_property(
        venum("Front Post-machining", EnumId::PadDrillPostMachiningMode, |it| {
            PropertyValue::Enum(
                EnumId::PadDrillPostMachiningMode,
                via(&it).front_post_machining_mode() as i32,
            )
        }),
        group_post_machining,
    );

    pm.add_property(
        vint("Front Post-machining Size", |it| {
            PropertyValue::Int(via(&it).front_post_machining_size())
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, true, CB_OR_CS));

    pm.add_property(
        vint("Front Post-machining Depth", |it| {
            PropertyValue::Int(via(&it).front_post_machining_depth())
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, true, CB));

    pm.add_property(
        vint("Front Post-machining Angle", |it| {
            PropertyValue::Int(via(&it).front_post_machining_angle())
        })
        .with_display(PropertyDisplay::PtDecidegree),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, true, CS));

    pm.add_property(
        venum("Back Post-machining", EnumId::PadDrillPostMachiningMode, |it| {
            PropertyValue::Enum(
                EnumId::PadDrillPostMachiningMode,
                via(&it).back_post_machining_mode() as i32,
            )
        }),
        group_post_machining,
    );

    pm.add_property(
        vint("Back Post-machining Size", |it| {
            PropertyValue::Int(via(&it).back_post_machining_size())
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, false, CB_OR_CS));

    pm.add_property(
        vint("Back Post-machining Depth", |it| {
            PropertyValue::Int(via(&it).back_post_machining_depth())
        })
        .with_display(PropertyDisplay::PtSize),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, false, CB));

    pm.add_property(
        vint("Back Post-machining Angle", |it| {
            PropertyValue::Int(via(&it).back_post_machining_angle())
        })
        .with_display(PropertyDisplay::PtDecidegree),
        group_post_machining,
    )
    .set_available_func(|it| via_post_machining_is(it, false, CS));
}
