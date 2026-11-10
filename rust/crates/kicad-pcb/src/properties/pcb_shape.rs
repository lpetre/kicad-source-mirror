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

//! Port of `PCB_SHAPE_DESC` (`pcbnew/pcb_shape.cpp`).

use kicad_core::layer_ids::is_external_copper_layer;

use crate::eda_shape::ShapeT;
use crate::item::BoardUse;
use crate::item_ref::ItemRef;
use crate::pcb_shape::PcbShape;

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
    layer_value, no_rust_api,
};

/// `static_cast<PCB_SHAPE*>`: the shape part of a shape, text box or table cell.
fn pcb_shape<'a>(it: &ItemRef<'a>) -> Option<&'a PcbShape> {
    it.as_shape()
}

fn shape_t(it: ItemRef<'_>) -> Option<ShapeT> {
    pcb_shape(&it).map(|s| s.shape.shape)
}

/// `isPolygonOrEllipse`.
fn is_polygon_or_ellipse(it: ItemRef<'_>) -> bool {
    shape_t(it).is_some_and(|t| matches!(t, ShapeT::Poly | ShapeT::Ellipse | ShapeT::EllipseArc))
}

/// `isNotBezierOrEllipseArc`.
fn is_not_bezier_or_ellipse_arc(it: ItemRef<'_>) -> bool {
    shape_t(it).is_none_or(|t| t != ShapeT::Bezier && t != ShapeT::EllipseArc)
}

/// `isCircle`.
fn is_circle(it: ItemRef<'_>) -> bool {
    shape_t(it).is_some_and(|t| t == ShapeT::Circle)
}

/// `isNotCircleOrEllipse`.
fn is_not_circle_or_ellipse(it: ItemRef<'_>) -> bool {
    shape_t(it)
        .is_none_or(|t| t != ShapeT::Circle && t != ShapeT::Ellipse && t != ShapeT::EllipseArc)
}

/// `isCopper`.
fn is_copper(it: ItemRef<'_>) -> bool {
    pcb_shape(&it).is_some() && it.as_dyn().is_on_copper_layer(it.ctx)
}

/// `isPadEditMode`: a footprint-holder board with an entered pad (editor state, not in the
/// Rust model: only reachable in the footprint editor).
fn is_pad_edit_mode(it: ItemRef<'_>) -> bool {
    if it.has_board() && it.ctx.board.board_use == BoardUse::FpHolder {
        no_rust_api("PAD::IsEntered");
    }

    false
}

/// `showNumberBoxProperty`.
fn show_number_box_property(it: ItemRef<'_>) -> bool {
    shape_t(it) == Some(ShapeT::Rectangle) && is_pad_edit_mode(it)
}

/// `showSpokeTemplateProperty`.
fn show_spoke_template_property(it: ItemRef<'_>) -> bool {
    shape_t(it) == Some(ShapeT::Segment) && is_pad_edit_mode(it)
}

/// `isExternalCuLayer`.
fn is_external_cu_layer(it: ItemRef<'_>) -> bool {
    pcb_shape(&it).is_some() && is_external_copper_layer(it.layer())
}

/// `PCB_SHAPE_DESC::PCB_SHAPE_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    let c = ClassId::PcbShape;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardConnectedItem);
    pm.add_type_cast(c, ClassId::EdaShape);
    pm.inherits_after(c, ClassId::BoardConnectedItem);
    pm.inherits_after(c, ClassId::EdaShape);

    pm.replace_property(
        ClassId::BoardConnectedItem,
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
    .set_is_copyable();

    pm.override_availability(c, ClassId::BoardItem, "Position X", is_polygon_or_ellipse);
    pm.override_availability(c, ClassId::BoardItem, "Position Y", is_polygon_or_ellipse);

    pm.mask(c, ClassId::EdaShape, "Line Color");
    pm.mask(c, ClassId::EdaShape, "Fill Color");

    pm.override_availability(c, ClassId::EdaShape, "Fill", is_not_bezier_or_ellipse_arc);

    pm.override_availability(c, ClassId::EdaShape, "Start X", is_not_circle_or_ellipse);
    pm.override_availability(c, ClassId::EdaShape, "Start Y", is_not_circle_or_ellipse);
    pm.override_availability(c, ClassId::EdaShape, "End X", is_not_circle_or_ellipse);
    pm.override_availability(c, ClassId::EdaShape, "End Y", is_not_circle_or_ellipse);
    pm.override_availability(c, ClassId::EdaShape, "Center X", is_circle);
    pm.override_availability(c, ClassId::EdaShape, "Center Y", is_circle);
    pm.override_availability(c, ClassId::EdaShape, "Radius", is_circle);

    pm.override_availability(c, ClassId::BoardConnectedItem, "Net", is_copper);

    let group_pad_primitives = "Pad Primitives";

    pm.add_property(
        Property::new("Number Box", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(pcb_shape(&it).expect("shape").shape.proxy_item)
        }),
        group_pad_primitives,
    )
    .set_available_func(show_number_box_property)
    .set_is_hidden_from_rules_editor();

    pm.add_property(
        Property::new("Thermal Spoke Template", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(pcb_shape(&it).expect("shape").shape.proxy_item)
        }),
        group_pad_primitives,
    )
    .set_available_func(show_spoke_template_property)
    .set_is_hidden_from_rules_editor();

    let group_tech_layers = "Technical Layers";

    pm.add_property(
        Property::new("Soldermask", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(pcb_shape(&it).expect("shape").has_solder_mask())
        }),
        group_tech_layers,
    )
    .set_available_func(is_external_cu_layer)
    .set_is_copyable();

    pm.add_property(
        Property::new(
            "Soldermask Margin Override",
            c,
            ValueType::OptInt,
            true,
            |it| PropertyValue::OptInt(pcb_shape(&it).expect("shape").local_solder_mask_margin()),
        )
        .with_display(PropertyDisplay::PtSize),
        group_tech_layers,
    )
    .set_available_func(is_external_cu_layer)
    .set_is_copyable();
}
