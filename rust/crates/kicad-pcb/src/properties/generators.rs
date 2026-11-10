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

//! Port of `PCB_TUNING_PATTERN_DESC` (`pcbnew/generators/pcb_tuning_pattern.cpp`),
//! `PCB_VIA_STACK_DESC` (`pcbnew/generators/pcb_via_stack.cpp`) and `PCB_VIA_STITCH_DESC`
//! (`pcbnew/generators/pcb_via_stitch.cpp`). The generator subclasses keep their typed members
//! in the property map of `PcbGenerator` (`board-model-types.md`); their getters
//! (`PCB_TUNING_PATTERN::GetWidth` ...) are not in the Rust model yet, so these properties are
//! pending (`no_rust_api`) apart from the `BOARD_ITEM` layer.

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, ValueType,
    layer_value, no_rust_api,
};

fn tp(name: &'static str, t: ValueType, getter: super::Getter) -> Property {
    Property::new(name, ClassId::PcbTuningPattern, t, true, getter)
}

/// `isSkew` / `isTimeDomain` and their combinations: `GetTuningMode()` and
/// `GetSettings().m_isTimeDomain` are not in the Rust model.
fn tuning_mode_pending(_: crate::item_ref::ItemRef<'_>) -> bool {
    no_rust_api("PCB_TUNING_PATTERN::GetTuningMode")
}

/// `PCB_TUNING_PATTERN_DESC::PCB_TUNING_PATTERN_DESC()`.
pub(crate) fn register_tuning_pattern(pm: &mut PropertyManager) {
    let c = ClassId::PcbTuningPattern;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbGenerator);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::PcbGenerator);
    pm.inherits_after(c, ClassId::BoardItem);

    let layer_choices = EnumId::PcbLayerId.map().choices_owned();
    pm.replace_property(
        ClassId::BoardItem,
        "Layer",
        Property::new_enum("Layer", c, ValueType::Enum(EnumId::PcbLayerId), true, |it| {
            layer_value(it.layer())
        }),
        "",
    )
    .set_choices(layer_choices);

    pm.add_property(
        tp("Width", ValueType::Int, |_| no_rust_api("PCB_TUNING_PATTERN::GetWidth"))
            .with_display(PropertyDisplay::PtSize),
        "",
    );

    pm.add_property(
        Property::new_enum("Net", c, ValueType::Int, true, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetNetCode")
        })
        .with_display(PropertyDisplay::PtNet),
        "",
    );

    let group_tech_layers = "Technical Layers";

    pm.add_property(
        tp("Soldermask", ValueType::Bool, |_| no_rust_api("PCB_TUNING_PATTERN::HasSolderMask")),
        group_tech_layers,
    );

    pm.add_property(
        tp("Soldermask Margin Override", ValueType::OptInt, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetLocalSolderMaskMargin")
        })
        .with_display(PropertyDisplay::PtSize),
        group_tech_layers,
    );

    let group_tab = "Pattern Properties";

    pm.add_property(
        tp("End X", ValueType::Int, |_| no_rust_api("PCB_TUNING_PATTERN::GetEndX"))
            .with_display(PropertyDisplay::PtSize)
            .with_coord(CoordType::AbsXCoord),
        group_tab,
    );

    pm.add_property(
        tp("End Y", ValueType::Int, |_| no_rust_api("PCB_TUNING_PATTERN::GetEndY"))
            .with_display(PropertyDisplay::PtSize)
            .with_coord(CoordType::AbsYCoord),
        group_tab,
    );

    pm.add_property(
        Property::new_enum(
            "Tuning Mode",
            c,
            ValueType::Enum(EnumId::LengthTuningMode),
            false,
            |_| no_rust_api("PCB_TUNING_PATTERN::GetTuningMode"),
        ),
        group_tab,
    );

    pm.add_property(
        tp("Min Amplitude", ValueType::Int, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetMinAmplitude")
        })
        .with_display(PropertyDisplay::PtSize)
        .with_coord(CoordType::AbsXCoord),
        group_tab,
    );

    pm.add_property(
        tp("Max Amplitude", ValueType::Int, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetMaxAmplitude")
        })
        .with_display(PropertyDisplay::PtSize)
        .with_coord(CoordType::AbsXCoord),
        group_tab,
    );

    pm.add_property(
        Property::new_enum(
            "Initial Side",
            c,
            ValueType::Enum(EnumId::MeanderSide),
            true,
            |_| no_rust_api("PCB_TUNING_PATTERN::GetInitialSide"),
        ),
        group_tab,
    );

    pm.add_property(
        tp("Min Spacing", ValueType::Int, |_| no_rust_api("PCB_TUNING_PATTERN::GetSpacing"))
            .with_display(PropertyDisplay::PtSize)
            .with_coord(CoordType::AbsXCoord),
        group_tab,
    );

    pm.add_property(
        tp("Corner Radius %", ValueType::Int, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetCornerRadiusPercentage")
        }),
        group_tab,
    );

    pm.add_property(
        tp("Target Length", ValueType::OptInt, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetTargetLength")
        })
        .with_display(PropertyDisplay::PtSize)
        .with_coord(CoordType::AbsXCoord),
        group_tab,
    )
    .set_available_func(tuning_mode_pending);

    pm.add_property(
        tp("Target Delay", ValueType::OptInt, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetTargetDelay")
        })
        .with_display(PropertyDisplay::PtTime),
        group_tab,
    )
    .set_available_func(tuning_mode_pending);

    pm.add_property(
        tp("Target Skew", ValueType::Int, |_| no_rust_api("PCB_TUNING_PATTERN::GetTargetSkew"))
            .with_display(PropertyDisplay::PtSize)
            .with_coord(CoordType::AbsXCoord),
        group_tab,
    )
    .set_available_func(tuning_mode_pending);

    pm.add_property(
        tp("Target Skew Delay", ValueType::Int, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetTargetSkewDelay")
        })
        .with_display(PropertyDisplay::PtTime),
        group_tab,
    )
    .set_available_func(tuning_mode_pending);

    pm.add_property(
        tp("Override Custom Rules", ValueType::Bool, |_| {
            no_rust_api("PCB_TUNING_PATTERN::GetOverrideCustomRules")
        }),
        group_tab,
    );

    pm.add_property(
        tp("Single-sided", ValueType::Bool, |_| no_rust_api("PCB_TUNING_PATTERN::IsSingleSided")),
        group_tab,
    );

    pm.add_property(
        tp("Rounded", ValueType::Bool, |_| no_rust_api("PCB_TUNING_PATTERN::IsRounded")),
        group_tab,
    );
}

fn vs(name: &'static str, t: ValueType, getter: super::Getter) -> Property {
    Property::new(name, ClassId::PcbViaStack, t, false, getter)
}

/// `PCB_VIA_STACK_DESC::PCB_VIA_STACK_DESC()`.
pub(crate) fn register_via_stack(pm: &mut PropertyManager) {
    let c = ClassId::PcbViaStack;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbGenerator);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::PcbGenerator);
    pm.inherits_after(c, ClassId::BoardItem);

    // A stack spans layers, so the single inherited layer is meaningless, as for a via.
    pm.mask(c, ClassId::BoardItem, "Layer");

    // Read-only: a stack's geometry is rebuilt from these, and the Properties panel has no
    // way to trigger that rebuild. Editing goes through the stack's own dialog.
    let group_stack = "Microvia Stack Properties";

    let layer_choices = EnumId::PcbLayerId.map().choices_owned();
    pm.add_property(
        Property::new_enum("Start Layer", c, ValueType::Enum(EnumId::PcbLayerId), false, |_| {
            no_rust_api("PCB_VIA_STACK::GetStartLayer")
        }),
        group_stack,
    )
    .set_choices(layer_choices.clone());

    pm.add_property(
        Property::new_enum("End Layer", c, ValueType::Enum(EnumId::PcbLayerId), false, |_| {
            no_rust_api("PCB_VIA_STACK::GetEndLayer")
        }),
        group_stack,
    )
    .set_choices(layer_choices);

    pm.add_property(
        Property::new_enum("Style", c, ValueType::Enum(EnumId::ViaStackStyle), false, |_| {
            no_rust_api("PCB_VIA_STACK::GetStyle")
        }),
        group_stack,
    );

    pm.add_property(
        Property::new_enum("Net", c, ValueType::Int, false, |_| {
            no_rust_api("PCB_VIA_STACK::GetNetCode")
        })
        .with_display(PropertyDisplay::PtNet),
        "",
    );

    pm.add_property(
        vs("Use Netclass Values", ValueType::Bool, |_| {
            no_rust_api("PCB_VIA_STACK::GetUseNetclass")
        }),
        group_stack,
    );

    pm.add_property(
        vs("Via Diameter", ValueType::Int, |_| no_rust_api("PCB_VIA_STACK::GetViaSize"))
            .with_display(PropertyDisplay::PtSize),
        group_stack,
    );

    pm.add_property(
        vs("Via Hole", ValueType::Int, |_| no_rust_api("PCB_VIA_STACK::GetViaDrill"))
            .with_display(PropertyDisplay::PtSize),
        group_stack,
    );

    pm.add_property(
        vs("Pitch", ValueType::Int, |_| no_rust_api("PCB_VIA_STACK::GetPitch"))
            .with_display(PropertyDisplay::PtSize),
        group_stack,
    )
    .set_available_func(|_| no_rust_api("PCB_VIA_STACK::GetStyle"));

    pm.add_property(
        vs("Copper-filled", ValueType::Bool, |_| no_rust_api("PCB_VIA_STACK::IsFilled")),
        group_stack,
    );

    pm.add_property(
        vs("Capped", ValueType::Bool, |_| no_rust_api("PCB_VIA_STACK::IsCapped")),
        group_stack,
    );
}

fn st(name: &'static str, t: ValueType, getter: super::Getter) -> Property {
    Property::new(name, ClassId::PcbViaStitch, t, true, getter)
}

/// `PCB_VIA_STITCH_DESC::PCB_VIA_STITCH_DESC()`.
pub(crate) fn register_via_stitch(pm: &mut PropertyManager) {
    let c = ClassId::PcbViaStitch;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbGenerator);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::PcbGenerator);
    pm.inherits_after(c, ClassId::BoardItem);

    pm.add_property(
        st("Size", ValueType::Int, |_| no_rust_api("PCB_VIA_STITCH::GetViaSize"))
            .with_display(PropertyDisplay::PtSize),
        "",
    );

    pm.add_property(
        st("Drill", ValueType::Int, |_| no_rust_api("PCB_VIA_STITCH::GetViaDrill"))
            .with_display(PropertyDisplay::PtSize),
        "",
    );

    pm.add_property(
        st("Pitch", ValueType::Int, |_| no_rust_api("PCB_VIA_STITCH::GetPitch"))
            .with_display(PropertyDisplay::PtSize),
        "",
    );

    pm.add_property(
        Property::new_enum(
            "Layout",
            c,
            ValueType::Enum(EnumId::PcbViaStitchLayout),
            true,
            |_| no_rust_api("PCB_VIA_STITCH::GetLayout"),
        ),
        "",
    )
    .set_available_func(|_| no_rust_api("PCB_VIA_STITCH::GetMode"));

    pm.add_property(
        Property::new_enum("Mode", c, ValueType::Enum(EnumId::PcbViaStitchMode), true, |_| {
            no_rust_api("PCB_VIA_STITCH::GetMode")
        }),
        "",
    );

    pm.add_property(
        st("Seed", ValueType::Unsigned, |_| no_rust_api("PCB_VIA_STITCH::GetSeed")),
        "",
    )
    .set_available_func(|_| no_rust_api("PCB_VIA_STITCH::GetMode"));

    pm.add_property(
        Property::new_enum("Net", c, ValueType::Int, true, |_| {
            no_rust_api("PCB_VIA_STITCH::GetNetCode")
        })
        .with_display(PropertyDisplay::PtNet),
        "",
    );

    pm.add_property(
        Property::new_enum("Guarded Net", c, ValueType::Int, true, |_| {
            no_rust_api("PCB_VIA_STITCH::GetGuardedNetCode")
        })
        .with_display(PropertyDisplay::PtNet),
        "",
    )
    .set_available_func(|_| no_rust_api("PCB_VIA_STITCH::GetMode"));

    // The stitch lives on its own GAL layer and the vias are multi-layer configured separately
    pm.mask(c, ClassId::BoardItem, "Layer");
}
