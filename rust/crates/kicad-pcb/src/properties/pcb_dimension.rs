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

//! Port of `DIMENSION_DESC`, `ALIGNED_DIMENSION_DESC`, `ORTHOGONAL_DIMENSION_DESC`,
//! `RADIAL_DIMENSION_DESC`, `LEADER_DIMENSION_DESC` and `CENTER_DIMENSION_DESC`
//! (`pcbnew/pcb_dimension.cpp`). The dimension enum maps are in [`super::enums`].

use kicad_core::KicadT;

use crate::item_ref::ItemRef;
use crate::pcb_dimension::{DimensionKind, PcbDimension};

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
    eda_text::host, no_rust_api,
};

/// `static_cast<PCB_DIMENSION_BASE*>`.
fn dim<'a>(it: &ItemRef<'a>) -> &'a PcbDimension {
    it.as_dimension().expect("dimension property on a dimension")
}

/// `isLeader`: `dynamic_cast<PCB_DIM_LEADER*>`.
fn is_leader(it: ItemRef<'_>) -> bool {
    it.as_dimension()
        .is_some_and(|d| matches!(d.kind, DimensionKind::Leader { .. }))
}

/// `isNotLeader`.
fn is_not_leader(it: ItemRef<'_>) -> bool {
    !is_leader(it)
}

/// `isNotCenter`: `dynamic_cast<PCB_DIM_CENTER*> == nullptr`.
fn is_not_center(it: ItemRef<'_>) -> bool {
    !it.as_dimension()
        .is_some_and(|d| matches!(d.kind, DimensionKind::Center))
}

/// `isMultiArrowDirection`: `dynamic_cast<PCB_DIM_ALIGNED*>` (orthogonal dimensions derive
/// from it).
fn is_multi_arrow_direction(it: ItemRef<'_>) -> bool {
    it.as_dimension().is_some_and(|d| {
        matches!(d.kind, DimensionKind::Aligned(_) | DimensionKind::Orthogonal { .. })
    })
}

/// Port of `DimensionHasValueMode` (`pcbnew/constraints/constraint_builder.cpp`).
fn dimension_has_value_mode(it: ItemRef<'_>) -> bool {
    it.as_dimension().is_some()
        && matches!(
            it.type_(),
            KicadT::PcbDimAlignedT | KicadT::PcbDimOrthogonalT | KicadT::PcbDimRadialT
        )
}

/// `usesOverrideText`: value bearing dims use Value row instead while leaders keep Text
/// below; Override Text left for centre mark only.
fn uses_override_text(it: ItemRef<'_>) -> bool {
    !is_leader(it) && !dimension_has_value_mode(it)
}

fn denum(name: &'static str, e: EnumId, get: super::Getter) -> Property {
    Property::new_enum(name, ClassId::PcbDimensionBase, ValueType::Enum(e), true, get)
}

fn dint(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbDimensionBase, ValueType::Int, true, get)
        .with_display(PropertyDisplay::PtSize)
}

fn dstring(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbDimensionBase, ValueType::String, true, get)
}

/// `DIMENSION_DESC::DIMENSION_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimensionBase;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::PcbText);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);

    pm.mask(c, ClassId::EdaText, "Orientation");

    let group_dimension = "Dimension Properties";

    pm.add_property(
        dstring("Prefix", |it| PropertyValue::String(dim(&it).prefix.clone())),
        group_dimension,
    )
    .set_available_func(is_not_leader);
    pm.add_property(
        dstring("Suffix", |it| PropertyValue::String(dim(&it).suffix.clone())),
        group_dimension,
    )
    .set_available_func(is_not_leader);

    pm.add_property(
        dstring("Override Text", |it| {
            PropertyValue::String(dim(&it).override_text().to_string())
        }),
        group_dimension,
    )
    .set_available_func(uses_override_text);

    // GetValueMode: DimensionValueMode( GetBoard(), this ) (the dimension's length constraint).
    pm.add_property(
        denum("Value Mode", EnumId::DimValueMode, |_| {
            no_rust_api("PCB_DIMENSION_BASE::GetValueMode")
        }),
        group_dimension,
    )
    .set_available_func(dimension_has_value_mode)
    .set_validator();

    // Driven mirrors measured geometry so grey it out since edit would not stick.
    pm.add_property(
        dstring("Value", |_| no_rust_api("PCB_DIMENSION_BASE::GetValueFieldText")),
        group_dimension,
    )
    .set_available_func(dimension_has_value_mode)
    .set_writeable_func(|_| no_rust_api("PCB_DIMENSION_BASE::GetValueMode"))
    .set_validator();

    pm.add_property(
        dstring("Text", |it| PropertyValue::String(dim(&it).override_text().to_string())),
        group_dimension,
    )
    .set_available_func(is_leader);

    pm.add_property(
        denum("Units", EnumId::DimUnitsMode, |it| {
            PropertyValue::Enum(EnumId::DimUnitsMode, dim(&it).units_mode() as i32)
        }),
        group_dimension,
    )
    .set_available_func(is_not_leader)
    .set_is_copyable();
    pm.add_property(
        denum("Units Format", EnumId::DimUnitsFormat, |it| {
            PropertyValue::Enum(EnumId::DimUnitsFormat, dim(&it).units_format as i32)
        }),
        group_dimension,
    )
    .set_available_func(is_not_leader)
    .set_is_copyable();
    pm.add_property(
        denum("Precision", EnumId::DimPrecision, |it| {
            PropertyValue::Enum(EnumId::DimPrecision, dim(&it).precision as i32)
        }),
        group_dimension,
    )
    .set_available_func(is_not_leader)
    .set_is_copyable();
    pm.add_property(
        Property::new("Suppress Trailing Zeroes", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(dim(&it).suppress_zeroes)
        }),
        group_dimension,
    )
    .set_available_func(is_not_leader)
    .set_is_copyable();

    pm.add_property(
        denum("Arrow Direction", EnumId::DimArrowDirection, |it| {
            PropertyValue::Enum(EnumId::DimArrowDirection, dim(&it).arrow_direction as i32)
        }),
        group_dimension,
    )
    .set_available_func(is_multi_arrow_direction)
    .set_is_copyable();

    pm.add_property(
        dint("Line Thickness", |it| PropertyValue::Int(dim(&it).line_thickness)),
        group_dimension,
    )
    .set_is_copyable();

    pm.add_property(
        dint("Arrow Length", |it| PropertyValue::Int(dim(&it).arrow_length)),
        group_dimension,
    )
    .set_available_func(is_not_center)
    .set_is_copyable();

    pm.add_property(
        dint("Extension Line Offset", |it| PropertyValue::Int(dim(&it).extension_offset)),
        group_dimension,
    )
    .set_available_func(is_not_center)
    .set_is_copyable();

    let group_text = "Text Properties";

    pm.add_property(
        denum("Text Position", EnumId::DimTextPosition, |it| {
            PropertyValue::Enum(EnumId::DimTextPosition, dim(&it).text_position as i32)
        }),
        group_text,
    )
    .set_available_func(is_not_leader)
    .set_is_copyable();

    pm.add_property(
        Property::new("Keep Aligned with Dimension", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(dim(&it).keep_text_aligned)
        }),
        group_text,
    );

    // GetTextAngleDegreesProp: GetTextAngleDegrees(), m_attributes.m_Angle.
    pm.add_property(
        Property::new("Orientation", c, ValueType::Double, true, |it| {
            PropertyValue::Double(host(&it).eda_text().attributes.angle)
        })
        .with_display(PropertyDisplay::PtDegree),
        group_text,
    )
    .set_writeable_func(|it| !dim(&it).keep_text_aligned);

    register_aligned(pm);
    register_orthogonal(pm);
    register_radial(pm);
    register_leader(pm);
    register_center(pm);
}

/// The four `OverrideAvailability( ..., []( INSPECTABLE* ) { return false; } )` calls every
/// concrete dimension class makes.
fn hide_text_props(pm: &mut PropertyManager, c: ClassId) {
    pm.override_availability(c, ClassId::EdaText, "Text", |_| false);
    pm.override_availability(c, ClassId::EdaText, "Vertical Justification", |_| false);
    pm.override_availability(c, ClassId::EdaText, "Hyperlink", |_| false);
    pm.override_availability(c, ClassId::BoardItem, "Knockout", |_| false);
}

/// The `TYPE_CAST`s and `InheritsAfter`s every concrete dimension class registers.
fn dimension_bases(pm: &mut PropertyManager, c: ClassId) {
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::EdaText);
    pm.add_type_cast(c, ClassId::PcbText);
    pm.add_type_cast(c, ClassId::PcbDimensionBase);
}

/// The `DimAligned` members of an aligned or orthogonal dimension.
fn aligned<'a>(it: &ItemRef<'a>) -> &'a crate::pcb_dimension::DimAligned {
    match &dim(it).kind {
        DimensionKind::Aligned(a) => a,
        DimensionKind::Orthogonal { aligned, .. } => aligned,
        _ => unreachable!("PCB_DIM_ALIGNED property on an aligned dimension"),
    }
}

/// `ALIGNED_DIMENSION_DESC::ALIGNED_DIMENSION_DESC()`.
fn register_aligned(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimAligned;
    pm.register_type(c);
    dimension_bases(pm, c);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::PcbDimensionBase);

    let group_dimension = "Dimension Properties";

    pm.add_property(
        Property::new("Crossbar Height", c, ValueType::Int, true, |it| {
            PropertyValue::Int(aligned(&it).height)
        })
        .with_display(PropertyDisplay::PtSize),
        group_dimension,
    );
    pm.add_property(
        Property::new("Extension Line Overshoot", c, ValueType::Int, true, |it| {
            PropertyValue::Int(aligned(&it).extension_height)
        })
        .with_display(PropertyDisplay::PtSize),
        group_dimension,
    );

    hide_text_props(pm, c);
}

/// `ORTHOGONAL_DIMENSION_DESC::ORTHOGONAL_DIMENSION_DESC()`.
fn register_orthogonal(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimOrthogonal;
    pm.register_type(c);
    dimension_bases(pm, c);
    pm.add_type_cast(c, ClassId::PcbDimAligned);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::PcbDimensionBase);
    pm.inherits_after(c, ClassId::PcbDimAligned);

    hide_text_props(pm, c);
}

/// `RADIAL_DIMENSION_DESC::RADIAL_DIMENSION_DESC()`.
fn register_radial(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimRadial;
    pm.register_type(c);
    dimension_bases(pm, c);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::PcbDimensionBase);

    pm.add_property(
        Property::new("Leader Length", c, ValueType::Int, true, |it| match &dim(&it).kind {
            DimensionKind::Radial { leader_length } => PropertyValue::Int(*leader_length),
            _ => unreachable!("PCB_DIM_RADIAL property on a radial dimension"),
        })
        .with_display(PropertyDisplay::PtSize),
        "Dimension Properties",
    );

    hide_text_props(pm, c);
}

/// `LEADER_DIMENSION_DESC::LEADER_DIMENSION_DESC()` (the `DIM_TEXT_BORDER` map is in
/// [`super::enums`]).
fn register_leader(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimLeader;
    pm.register_type(c);
    dimension_bases(pm, c);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::PcbDimensionBase);

    pm.add_property(
        Property::new_enum(
            "Text Frame",
            c,
            ValueType::Enum(EnumId::DimTextBorder),
            true,
            |it| match &dim(&it).kind {
                DimensionKind::Leader { text_border } => {
                    PropertyValue::Enum(EnumId::DimTextBorder, *text_border as i32)
                }
                _ => unreachable!("PCB_DIM_LEADER property on a leader"),
            },
        ),
        "Dimension Properties",
    );

    hide_text_props(pm, c);
}

/// `CENTER_DIMENSION_DESC::CENTER_DIMENSION_DESC()`.
fn register_center(pm: &mut PropertyManager) {
    let c = ClassId::PcbDimCenter;
    pm.register_type(c);
    dimension_bases(pm, c);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaText);
    pm.inherits_after(c, ClassId::PcbText);
    pm.inherits_after(c, ClassId::PcbDimensionBase);

    hide_text_props(pm, c);
}
