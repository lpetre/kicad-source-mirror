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

//! Port of the descriptors of the smaller item classes: `PCB_GROUP_DESC`
//! (`pcbnew/pcb_group.cpp`), `PCB_BARCODE_DESC` (`pcbnew/pcb_barcode.cpp`),
//! `PCB_REFERENCE_IMAGE_DESC` (`pcbnew/pcb_reference_image.cpp`), `PCB_3D_BODY_DESC`
//! (`pcbnew/pcb_3d_body.cpp`), `PCB_DRILL_MAP_DESC` (`pcbnew/pcb_drill_map.cpp`),
//! `PCB_GRIDITEM_DESC` (`pcbnew/pcb_grid_item.cpp`), `PCB_MARKER_DESC` (`pcbnew/pcb_marker.cpp`),
//! `PCB_POINT_DESC` (`pcbnew/pcb_point.cpp`), `PCB_TARGET_DESC` (`pcbnew/pcb_target.cpp`) and
//! `PCB_CONSTRAINT_DESC` (`pcbnew/constraints/pcb_constraint.cpp`).

use crate::eda_text::EdaTextExt;
use crate::item_ref::{AnyItem, ItemRef};
use crate::pcb_barcode::{BarcodeT, PcbBarcode};
use crate::pcb_drill_map::PcbDrillMap;
use crate::pcb_grid_item::{PcbGridItem, PcbGridType};
use crate::pcb_reference_image::PcbReferenceImage;

use super::{
    ClassId, CoordType, EnumId, Property, PropertyDisplay, PropertyManager, PropertyValue,
    ValueType, layer_value, no_rust_api,
};

/// `PCB_GROUP_DESC::PCB_GROUP_DESC()`.
pub(crate) fn register_pcb_group(pm: &mut PropertyManager) {
    let c = ClassId::PcbGroup;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::EdaGroup);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::EdaGroup);

    pm.mask(c, ClassId::BoardItem, "Position X");
    pm.mask(c, ClassId::BoardItem, "Position Y");
    pm.mask(c, ClassId::BoardItem, "Layer");

    let group_tab = "Group Properties";

    // PROPERTY<EDA_GROUP, wxString>: registered for EDA_GROUP.
    pm.add_property(
        Property::new("Name", ClassId::EdaGroup, ValueType::String, true, |it| {
            match it.any() {
                AnyItem::Group(g) => PropertyValue::String(g.name().to_string()),
                AnyItem::Generator(g) => PropertyValue::String(g.group.name().to_string()),
                _ => unreachable!("EDA_GROUP property on a group"),
            }
        }),
        group_tab,
    );
}

/// `static_cast<PCB_BARCODE*>`.
fn barcode<'a>(it: &ItemRef<'a>) -> &'a PcbBarcode {
    match it.any() {
        AnyItem::Barcode(b) => b,
        _ => unreachable!("PCB_BARCODE property on a barcode"),
    }
}

/// `hasKnockout`.
fn has_knockout(it: ItemRef<'_>) -> bool {
    matches!(it.any(), AnyItem::Barcode(b) if b.is_knockout())
}

fn bint(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbBarcode, ValueType::Int, true, get)
        .with_display(PropertyDisplay::PtCoord)
}

/// `PCB_BARCODE_DESC::PCB_BARCODE_DESC()` (the `BARCODE_T` / `BARCODE_ECC_T` maps are in
/// [`super::enums`]).
pub(crate) fn register_pcb_barcode(pm: &mut PropertyManager) {
    let c = ClassId::PcbBarcode;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardItem);

    let group_barcode = "Barcode Properties";

    pm.add_property(
        Property::new("Text", c, ValueType::String, true, |it| {
            PropertyValue::String(barcode(&it).text.text.text().to_string())
        }),
        group_barcode,
    );

    pm.add_property(
        Property::new("Show Text", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(barcode(&it).text.text.is_visible())
        }),
        group_barcode,
    )
    .set_is_copyable();

    // GetTextSize: m_text.GetTextHeight().
    pm.add_property(
        bint("Text Size", |it| {
            PropertyValue::Int(barcode(&it).text.text_height(it.ctx))
        }),
        group_barcode,
    );

    pm.add_property(bint("Width", |it| PropertyValue::Int(barcode(&it).width)), group_barcode)
        .set_is_copyable();

    pm.add_property(bint("Height", |it| PropertyValue::Int(barcode(&it).height)), group_barcode)
        .set_is_copyable();

    // GetOrientation: GetAngle().AsDegrees().
    pm.add_property(
        Property::new("Orientation", c, ValueType::Double, true, |it| {
            PropertyValue::Double(barcode(&it).angle(it.ctx).as_degrees())
        }),
        group_barcode,
    );

    pm.add_property(
        Property::new_enum("Barcode Type", c, ValueType::Enum(EnumId::BarcodeT), true, |it| {
            PropertyValue::Enum(EnumId::BarcodeT, barcode(&it).kind as i32)
        }),
        group_barcode,
    );

    pm.add_property(
        Property::new_enum(
            "Error Correction",
            c,
            ValueType::Enum(EnumId::BarcodeEccT),
            true,
            |it| PropertyValue::Enum(EnumId::BarcodeEccT, barcode(&it).error_correction as i32),
        ),
        group_barcode,
    )
    .set_available_func(|it| match it.any() {
        AnyItem::Barcode(b) => b.kind == BarcodeT::QrCode || b.kind == BarcodeT::MicroQrCode,
        _ => false,
    })
    .set_choices_func();

    pm.add_property(
        Property::new("Knockout", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(barcode(&it).is_knockout())
        }),
        group_barcode,
    )
    .set_is_copyable();

    pm.add_property(bint("Margin X", |it| PropertyValue::Int(barcode(&it).margin.x)), group_barcode)
        .set_available_func(has_knockout);

    pm.add_property(bint("Margin Y", |it| PropertyValue::Int(barcode(&it).margin.y)), group_barcode)
        .set_available_func(has_knockout);
}

/// `static_cast<PCB_REFERENCE_IMAGE*>`.
fn image<'a>(it: &ItemRef<'a>) -> &'a PcbReferenceImage {
    match it.any() {
        AnyItem::Image(i) => i,
        _ => unreachable!("PCB_REFERENCE_IMAGE property on an image"),
    }
}

/// `PCB_REFERENCE_IMAGE_DESC::PCB_REFERENCE_IMAGE_DESC()`.
pub(crate) fn register_pcb_reference_image(pm: &mut PropertyManager) {
    let c = ClassId::PcbReferenceImage;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardItem);

    pm.replace_property(
        ClassId::BoardItem,
        "Layer",
        Property::new_enum(
            "Associated Layer",
            c,
            ValueType::Enum(EnumId::PcbLayerId),
            true,
            |it| layer_value(it.layer()),
        )
        .with_base(ClassId::BoardItem),
        "",
    )
    .set_is_copyable();

    let group_image = "Image Properties";

    pm.add_property(
        Property::new("Scale", c, ValueType::Double, true, |it| {
            PropertyValue::Double(image(&it).image_scale())
        }),
        group_image,
    );

    pm.add_property(
        Property::new("Transform Offset X", c, ValueType::Int, true, |it| {
            PropertyValue::Int(image(&it).transform_origin_offset().x)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsXCoord),
        group_image,
    );

    pm.add_property(
        Property::new("Transform Offset Y", c, ValueType::Int, true, |it| {
            PropertyValue::Int(image(&it).transform_origin_offset().y)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::AbsYCoord),
        group_image,
    );

    pm.add_property(
        Property::new("Orientation", c, ValueType::Angle, true, |it| {
            PropertyValue::Angle(image(&it).orientation())
        })
        .with_display(PropertyDisplay::PtDecidegree),
        group_image,
    );

    pm.add_property(
        Property::new("Opacity", c, ValueType::Double, true, |it| {
            PropertyValue::Double(image(&it).opacity())
        }),
        group_image,
    );

    // GetWidth / GetHeight: m_referenceImage.GetImage().GetSize().
    pm.add_property(
        Property::new("Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(image(&it).size().x)
        })
        .with_display(PropertyDisplay::PtCoord),
        group_image,
    );

    pm.add_property(
        Property::new("Height", c, ValueType::Int, true, |it| {
            PropertyValue::Int(image(&it).size().y)
        })
        .with_display(PropertyDisplay::PtCoord),
        group_image,
    );

    // For future use
    let _greyscale = "Greyscale";
}

/// `PCB_3D_BODY_DESC::PCB_3D_BODY_DESC()`.
pub(crate) fn register_pcb_3d_body(pm: &mut PropertyManager) {
    pm.register_type(ClassId::Pcb3dBody);
    pm.inherits_after(ClassId::Pcb3dBody, ClassId::BoardItem);
}

/// `static_cast<PCB_DRILL_MAP*>`.
fn drill_map<'a>(it: &ItemRef<'a>) -> &'a PcbDrillMap {
    match it.any() {
        AnyItem::DrillMap(m) => m,
        _ => unreachable!("PCB_DRILL_MAP property on a drill map"),
    }
}

/// `PCB_DRILL_MAP_DESC::PCB_DRILL_MAP_DESC()`.
pub(crate) fn register_pcb_drill_map(pm: &mut PropertyManager) {
    let c = ClassId::PcbDrillMap;
    pm.register_type(c);

    pm.add_type_cast(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::BoardItem);

    // _( "Drill Map Properties" )
    let map_props = "Drill Map Properties";

    // Relative, not absolute. This is how far the marks are slid from their holes, so the
    // drawing origin has nothing to say about it
    pm.add_property(
        Property::new("Offset X", c, ValueType::Int, true, |it| {
            PropertyValue::Int(drill_map(&it).offset().x)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::RelXCoord),
        map_props,
    );

    pm.add_property(
        Property::new("Offset Y", c, ValueType::Int, true, |it| {
            PropertyValue::Int(drill_map(&it).offset().y)
        })
        .with_display(PropertyDisplay::PtCoord)
        .with_coord(CoordType::RelYCoord),
        map_props,
    );

    pm.add_property(
        Property::new("Symbol Size", c, ValueType::Int, true, |it| {
            PropertyValue::Int(drill_map(&it).symbol_size())
        })
        .with_display(PropertyDisplay::PtSize),
        map_props,
    );

    pm.mask(c, ClassId::BoardItem, "Position X");
    pm.mask(c, ClassId::BoardItem, "Position Y");

    // Choices come from the board's stackup, so they are filled in by the properties panel
    pm.add_property(
        Property::new_enum("Hole Span", c, ValueType::Int, true, |it| {
            if drill_map(&it).all_spans() || !it.has_board() {
                return PropertyValue::Int(-1);
            }

            no_rust_api("EnumerateDrillSpans")
        }),
        map_props,
    );

    pm.add_property(
        Property::new("Outline Slots", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(drill_map(&it).outline_slots())
        }),
        map_props,
    );

    pm.add_property(
        Property::new("Guide Cross", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(drill_map(&it).guide_cross())
        }),
        map_props,
    );
}

/// `static_cast<PCB_GRID_ITEM*>`.
fn grid<'a>(it: &ItemRef<'a>) -> &'a PcbGridItem {
    match it.any() {
        AnyItem::GridItem(g) => g,
        _ => unreachable!("PCB_GRID_ITEM property on a grid item"),
    }
}

/// `isCartesian`.
fn is_cartesian(it: ItemRef<'_>) -> bool {
    matches!(it.any(), AnyItem::GridItem(g) if g.grid_item_type() == PcbGridType::Cartesian)
}

/// `isPolar`.
fn is_polar(it: ItemRef<'_>) -> bool {
    matches!(it.any(), AnyItem::GridItem(g) if g.grid_item_type() == PcbGridType::Polar)
}

fn gint(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbGridItem, ValueType::Int, true, get)
        .with_display(PropertyDisplay::PtSize)
}

fn gdeg(name: &'static str, get: super::Getter) -> Property {
    Property::new(name, ClassId::PcbGridItem, ValueType::Double, true, get)
        .with_display(PropertyDisplay::PtDegree)
}

/// `PCB_GRIDITEM_DESC::PCB_GRIDITEM_DESC()` (the `PCB_GRID_TYPE` map is in [`super::enums`]).
pub(crate) fn register_pcb_grid_item(pm: &mut PropertyManager) {
    let c = ClassId::PcbGridItem;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardItem);

    // Grid items live on every layer; the inherited Layer property is meaningless.
    pm.override_availability(c, ClassId::BoardItem, "Layer", |_| false);

    pm.add_property(
        Property::new_enum("Grid Type", c, ValueType::Enum(EnumId::PcbGridType), true, |it| {
            PropertyValue::Enum(EnumId::PcbGridType, grid(&it).grid_item_type() as i32)
        }),
        "",
    );

    pm.add_property(
        gdeg("Orientation", |it| PropertyValue::Double(grid(&it).orientation().as_degrees())),
        "",
    );

    pm.add_property(gint("Extent X", |it| PropertyValue::Int(grid(&it).extent().x * 2)), "")
        .set_available_func(is_cartesian);
    pm.add_property(gint("Extent Y", |it| PropertyValue::Int(grid(&it).extent().y * 2)), "")
        .set_available_func(is_cartesian);
    pm.add_property(gint("Spacing X", |it| PropertyValue::Int(grid(&it).spacing().x)), "")
        .set_available_func(is_cartesian);
    pm.add_property(gint("Spacing Y", |it| PropertyValue::Int(grid(&it).spacing().y)), "")
        .set_available_func(is_cartesian);
    pm.add_property(gint("Radius Extent", |it| PropertyValue::Int(grid(&it).extent().x)), "")
        .set_available_func(is_polar);
    pm.add_property(gint("Radius Spacing", |it| PropertyValue::Int(grid(&it).spacing().x)), "")
        .set_available_func(is_polar);
    pm.add_property(
        gdeg("Phi Extent", |it| PropertyValue::Double(grid(&it).phi_extent().as_degrees())),
        "",
    )
    .set_available_func(is_polar);
    pm.add_property(
        gdeg("Phi Spacing", |it| PropertyValue::Double(grid(&it).phi_spacing().as_degrees())),
        "",
    )
    .set_available_func(is_polar);

    pm.add_property(
        Property::new("Priority", c, ValueType::Unsigned, true, |it| {
            PropertyValue::Unsigned(grid(&it).assigned_priority())
        }),
        "",
    );
    pm.add_property(
        Property::new("Tick Interval", c, ValueType::Unsigned, true, |it| {
            PropertyValue::Unsigned(grid(&it).tick_interval())
        }),
        "",
    );
    pm.add_property(
        Property::new("Affects Cursor Snap", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(grid(&it).affects().cursor)
        }),
        "",
    );
    pm.add_property(
        Property::new("Affects Routing", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(grid(&it).affects().routing)
        }),
        "",
    );
    pm.add_property(
        Property::new("Affects Placement", c, ValueType::Bool, true, |it| {
            PropertyValue::Bool(grid(&it).affects().placement)
        }),
        "",
    );
}

/// `PCB_MARKER_DESC::PCB_MARKER_DESC()`.
pub(crate) fn register_pcb_marker(pm: &mut PropertyManager) {
    let c = ClassId::PcbMarker;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.add_type_cast(c, ClassId::MarkerBase);
    pm.inherits_after(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::MarkerBase);

    // Markers cannot be locked and have no user-accessible layer control
    pm.mask(c, ClassId::BoardItem, "Layer");
    pm.mask(c, ClassId::BoardItem, "Locked");
}

/// `PCB_POINT_DESC::PCB_POINT_DESC()`.
pub(crate) fn register_pcb_point(pm: &mut PropertyManager) {
    let c = ClassId::PcbPoint;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardItem);

    pm.add_property(
        Property::new("Size", c, ValueType::Int, true, |it| match it.any() {
            AnyItem::Point(p) => PropertyValue::Int(p.size()),
            _ => unreachable!("PCB_POINT property on a point"),
        })
        .with_display(PropertyDisplay::PtSize),
        "",
    );
}

/// `PCB_TARGET_DESC::PCB_TARGET_DESC()`.
pub(crate) fn register_pcb_target(pm: &mut PropertyManager) {
    let c = ClassId::PcbTarget;
    pm.register_type(c);
    pm.inherits_after(c, ClassId::BoardItem);

    fn target(it: ItemRef<'_>) -> &crate::pcb_target::PcbTarget {
        match it.any() {
            AnyItem::Target(t) => t,
            _ => unreachable!("PCB_TARGET property on a target"),
        }
    }

    pm.add_property(
        Property::new("Size", c, ValueType::Int, true, |it| {
            PropertyValue::Int(target(it).size())
        })
        .with_display(PropertyDisplay::PtSize),
        "",
    )
    .set_is_copyable();
    pm.add_property(
        Property::new("Width", c, ValueType::Int, true, |it| {
            PropertyValue::Int(target(it).width())
        })
        .with_display(PropertyDisplay::PtSize),
        "",
    )
    .set_is_copyable();

    // TODO change the integer to an enum?
    pm.add_property(
        Property::new("Shape", c, ValueType::Int, true, |it| {
            PropertyValue::Int(target(it).shape())
        }),
        "",
    )
    .set_is_copyable();
}

/// `PCB_CONSTRAINT_DESC::PCB_CONSTRAINT_DESC()`.
pub(crate) fn register_pcb_constraint(pm: &mut PropertyManager) {
    let c = ClassId::PcbConstraint;
    pm.register_type(c);
    pm.add_type_cast(c, ClassId::BoardItem);
    pm.inherits_after(c, ClassId::BoardItem);

    // A constraint has no geometry, so hide the inherited positional/layer properties.
    pm.mask(c, ClassId::BoardItem, "Position X");
    pm.mask(c, ClassId::BoardItem, "Position Y");
    pm.mask(c, ClassId::BoardItem, "Layer");
    pm.mask(c, ClassId::BoardItem, "Locked");

    let constraint_tab = "Constraint Properties";

    // Driving vs reference is the one freely-editable property (the "convert to reference"
    // toggle); type and members are intrinsic and set at creation.
    pm.add_property(
        Property::new("Driving", c, ValueType::Bool, true, |it| match it.any() {
            AnyItem::Constraint(k) => PropertyValue::Bool(k.is_driving()),
            _ => unreachable!("PCB_CONSTRAINT property on a constraint"),
        }),
        constraint_tab,
    );
}
