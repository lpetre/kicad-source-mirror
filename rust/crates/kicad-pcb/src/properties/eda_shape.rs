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

//! Port of `EDA_SHAPE_DESC` (`common/eda_shape.cpp`) and the property adapter
//! `EDA_SHAPE::GetFillModeProp`. The `SHAPE_T`, `LINE_STYLE`, `UI_FILL_MODE` and
//! `LINE_ENDING_STYLE` maps are in [`super::enums`].

use crate::eda_shape::{EdaShape, FillT, ShapeT};
use crate::item_ref::ItemRef;
use crate::traits::EdaShapeItem;

use super::{
    ClassId, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue, ValueType,
};
use super::CoordType;

/// `dynamic_cast<EDA_SHAPE*>( aItem )`.
pub(crate) fn shape_of<'a>(it: &ItemRef<'a>) -> Option<&'a dyn EdaShapeItem> {
    it.as_eda_shape()
}

/// The `EDA_SHAPE` data of an item registered under a shape class.
fn es<'a>(it: &ItemRef<'a>) -> &'a EdaShape {
    shape_of(it)
        .expect("EDA_SHAPE property on a shape item")
        .eda_shape()
}

/// `EDA_SHAPE::GetFillModeProp`.
pub(crate) fn fill_mode_prop(shape: &EdaShape) -> i32 {
    // UI_FILL_MODE: NONE = 0, SOLID, HATCH, REVERSE_HATCH, CROSS_HATCH
    match shape.fill {
        FillT::NoFill => 0,
        FillT::Hatch => 2,
        FillT::ReverseHatch => 3,
        FillT::CrossHatch => 4,
        _ => 1,
    }
}

fn shape_is(it: ItemRef<'_>, f: impl Fn(ShapeT) -> bool) -> bool {
    shape_of(&it).is_some_and(|s| f(s.eda_shape().shape))
}

/// `isNotPolygonOrCircle`: polygons, unlike other shapes, have no meaningful start or end
/// coordinates.
fn is_not_polygon_or_circle(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s != ShapeT::Poly && s != ShapeT::Circle)
}

/// `isCircle`.
fn is_circle(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s == ShapeT::Circle)
}

/// `isRectangle`.
fn is_rectangle(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s == ShapeT::Rectangle)
}

/// `isEllipseOrEllipseArc`.
fn is_ellipse_or_ellipse_arc(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s == ShapeT::Ellipse || s == ShapeT::EllipseArc)
}

/// `isEllipseArc`.
fn is_ellipse_arc(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s == ShapeT::EllipseArc)
}

/// The availability lambda of `Angle`.
fn is_arc(it: ItemRef<'_>) -> bool {
    shape_is(it, |s| s == ShapeT::Arc)
}

/// `fillAvailable`.
fn fill_available(it: ItemRef<'_>) -> bool {
    // For some reason masking "Filled" and "Fill Color" at the
    // PCB_TABLECELL level doesn't work.
    if matches!(
        it.type_(),
        kicad_core::KicadT::PcbTablecellT | kicad_core::KicadT::PcbTextboxT
    ) {
        return false;
    }

    if let Some(s) = shape_of(&it) {
        return matches!(
            s.eda_shape().shape,
            ShapeT::Poly | ShapeT::Rectangle | ShapeT::Circle | ShapeT::Bezier | ShapeT::Ellipse
        );
    }

    false
}

/// `isOpenShape`.
fn is_open_shape(it: ItemRef<'_>) -> bool {
    shape_of(&it).is_some_and(|s| !s.eda_shape().is_closed())
}

fn int(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::EdaShape, ValueType::Int, true, get)
}

/// `EDA_SHAPE_DESC::EDA_SHAPE_DESC()`.
pub(crate) fn register(pm: &mut PropertyManager) {
    pm.register_type(ClassId::EdaShape);

    let shape_props = "Shape Properties";
    let c = ClassId::EdaShape;

    pm.add_property(
        Property::new_enum("Shape", c, ValueType::Enum(EnumId::ShapeT), false, |it| {
            PropertyValue::Enum(EnumId::ShapeT, es(&it).shape as i32)
        }),
        shape_props,
    );

    pm.add_property(
        int("Start X", |it| PropertyValue::Int(es(&it).start().x))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsXCoord),
        shape_props,
    )
    .set_available_func(is_not_polygon_or_circle);
    pm.add_property(
        int("Start Y", |it| PropertyValue::Int(es(&it).start().y))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsYCoord),
        shape_props,
    )
    .set_available_func(is_not_polygon_or_circle);

    pm.add_property(
        int("Center X", |it| PropertyValue::Int(es(&it).start().x))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsXCoord),
        shape_props,
    )
    .set_available_func(is_circle);

    pm.add_property(
        int("Center Y", |it| PropertyValue::Int(es(&it).start().y))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsYCoord),
        shape_props,
    )
    .set_available_func(is_circle);

    pm.add_property(
        int("Radius", |it| PropertyValue::Int(es(&it).radius()))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_circle);

    pm.add_property(
        int("End X", |it| PropertyValue::Int(es(&it).end().x))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsXCoord),
        shape_props,
    )
    .set_available_func(is_not_polygon_or_circle);

    pm.add_property(
        int("End Y", |it| PropertyValue::Int(es(&it).end().y))
            .with_display(PropertyDisplay::PtCoord)
            .with_coord(CoordType::AbsYCoord),
        shape_props,
    )
    .set_available_func(is_not_polygon_or_circle);

    pm.add_property(
        int("Width", |it| PropertyValue::Int(es(&it).rectangle_width()))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_rectangle);

    pm.add_property(
        int("Height", |it| PropertyValue::Int(es(&it).rectangle_height()))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_rectangle);

    pm.add_property(
        int("Corner Radius", |it| PropertyValue::Int(es(&it).corner_radius()))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_rectangle)
    .set_validator();

    pm.add_property(
        int("Major Radius", |it| PropertyValue::Int(es(&it).ellipse.major_radius))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_ellipse_or_ellipse_arc);

    pm.add_property(
        int("Minor Radius", |it| PropertyValue::Int(es(&it).ellipse.minor_radius))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_ellipse_or_ellipse_arc);

    pm.add_property(
        Property::new("Ellipse Rotation", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(es(&it).ellipse.rotation)
        })
        .with_display(PropertyDisplay::PtDecidegree),
        shape_props,
    )
    .set_available_func(is_ellipse_or_ellipse_arc);

    pm.add_property(
        Property::new("Arc Start Angle", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(es(&it).ellipse.start_angle)
        })
        .with_display(PropertyDisplay::PtDecidegree),
        shape_props,
    )
    .set_available_func(is_ellipse_arc);

    pm.add_property(
        Property::new("Arc End Angle", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(es(&it).ellipse.end_angle)
        })
        .with_display(PropertyDisplay::PtDecidegree),
        shape_props,
    )
    .set_available_func(is_ellipse_arc);

    // GetWidth() is virtual (PCB_SHAPE scales it by the parent footprint).
    pm.add_property(
        int("Line Width", |it| {
            PropertyValue::Int(shape_of(&it).expect("shape").width(it.ctx))
        })
        .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new_enum("Line Style", c, ValueType::Enum(EnumId::LineStyle), true, |it| {
            PropertyValue::Enum(EnumId::LineStyle, es(&it).line_style() as i32)
        }),
        shape_props,
    )
    .set_is_copyable();

    pm.add_property(
        Property::new("Line Color", c, ValueType::Color, true, |it| {
            PropertyValue::Color(*es(&it).stroke.color())
        }),
        shape_props,
    )
    .set_is_hidden_from_rules_editor();

    pm.add_property(
        Property::new("Angle", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(es(&it).arc_angle())
        })
        .with_display(PropertyDisplay::PtDecidegree),
        shape_props,
    )
    .set_available_func(is_arc)
    .set_validator();

    pm.add_property(
        Property::new_enum("Fill", c, ValueType::Enum(EnumId::UiFillMode), true, |it| {
            PropertyValue::Enum(EnumId::UiFillMode, fill_mode_prop(es(&it)))
        }),
        shape_props,
    )
    .set_available_func(fill_available)
    .set_is_copyable();

    pm.add_property(
        Property::new("Fill Color", c, ValueType::Color, true, |it| {
            PropertyValue::Color(*es(&it).fill_color())
        }),
        shape_props,
    )
    .set_available_func(fill_available)
    .set_is_hidden_from_rules_editor();

    pm.add_property(
        Property::new_enum(
            "Start Shape",
            c,
            ValueType::Enum(EnumId::LineEndingStyle),
            true,
            |it| PropertyValue::Enum(EnumId::LineEndingStyle, es(&it).start_ending.style as i32),
        ),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("Start Length", |it| PropertyValue::Int(es(&it).start_ending.length))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("Start Width", |it| PropertyValue::Int(es(&it).start_ending.width))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("Start Stroke Width", |it| {
            PropertyValue::Int(es(&it).start_ending.stroke.width())
        })
        .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        Property::new_enum(
            "End Shape",
            c,
            ValueType::Enum(EnumId::LineEndingStyle),
            true,
            |it| PropertyValue::Enum(EnumId::LineEndingStyle, es(&it).end_ending.style as i32),
        ),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("End Length", |it| PropertyValue::Int(es(&it).end_ending.length))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("End Width", |it| PropertyValue::Int(es(&it).end_ending.width))
            .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);

    pm.add_property(
        int("End Stroke Width", |it| {
            PropertyValue::Int(es(&it).end_ending.stroke.width())
        })
        .with_display(PropertyDisplay::PtSize),
        shape_props,
    )
    .set_available_func(is_open_shape);
}
