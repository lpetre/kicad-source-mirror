// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-07: `format( const PAD* )` (W:1972-2549) with its lambdas `shapeName`,
//! `formatPostMachining` (shared with the via writer: `PcbWriter::format_post_machining` in
//! `tracks.rs`), `formatCornerProperties`, `anchorShape`, `formatPrimitives` and
//! `formatPadLayer`.
//!
//! Pad primitives are written from their library members (`GetLibraryStart()` ... of a
//! primitive whose parent is a pad are the `m_lib*` members).

use kicad_core::eda_units::format_angle;
use kicad_core::layer_range::LayerRange;
use kicad_core::string_utils::format_double2_str;
use kicad_core::{EdaDataType, Lset, MAX_CU_LAYERS, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::{
    RECT_CHAMFER_BOTTOM_LEFT, RECT_CHAMFER_BOTTOM_RIGHT, RECT_CHAMFER_TOP_LEFT,
    RECT_CHAMFER_TOP_RIGHT,
};
use kicad_geom::{ANGLE_45, ANGLE_90, Vector2I};
use kicad_sexpr::IoError;
use kicad_sexpr::{format_bool, format_custom_properties, format_opt_bool, format_uuid};

use super::tracks::is_default_teardrop_parameters;
use super::{
    CTL_OMIT_PAD_NETS, PcbWriter, format_internal_units_dt, format_iu, format_iu_pt, unbake_size,
    unbake_size_uniform,
};
use crate::board_connected_item::netname;
use crate::eda_shape::ShapeT;
use crate::ids::PadId;
use crate::pad::Pad;
use crate::padstack::{
    CustomShapeZoneMode, PadAttrib, PadDrillShape, PadProp, PadShape, PadSimElectricalType,
    Padstack, PadstackMode,
};
use crate::pcb_track::ZoneLayerOverride;
use crate::traits::EdaShapeItem;
use crate::zone_settings::ZoneConnection;

/// The `shapeName` lambda of `format( const PAD* )`.
fn shape_name(pad: &Pad, layer: PcbLayerId) -> &'static str {
    match pad.shape(layer) {
        PadShape::Circle => "circle",
        PadShape::Rectangle => "rect",
        PadShape::Oval => "oval",
        PadShape::Trapezoid => "trapezoid",
        PadShape::ChamferedRect | PadShape::Roundrect => "roundrect",
        PadShape::Custom => "custom",
    }
}

/// The `anchorShape` lambda of `format( const PAD* )`.
fn anchor_shape(pad: &Pad, layer: PcbLayerId) -> &'static str {
    match pad.anchor_pad_shape(layer) {
        PadShape::Rectangle => "rect",
        _ => "circle",
    }
}

impl PcbWriter<'_> {
    /// Port of `format( const PAD* aPad )`: `(pad ..)`.
    ///
    /// # Errors
    ///
    /// An [`IoError`] on write error.
    pub(crate) fn format_pad(&mut self, pad_id: PadId) -> Result<(), IoError> {
        let Some(pad) = self.src.pad(pad_id) else {
            return Ok(());
        };

        let ctx = self.ctx;
        let board = if crate::board_item::has_board(pad, &ctx) {
            Some(ctx.board)
        } else {
            None
        };
        let parent_fp = pad.parent_footprint(&ctx);
        let padstack = pad.lib_padstack();

        let type_ = match pad.attribute() {
            PadAttrib::Pth => "thru_hole",
            PadAttrib::Smd => "smd",
            PadAttrib::Conn => "connect",
            PadAttrib::NonComponent => "non_component",
            PadAttrib::Npth => "np_thru_hole",
        };

        let property = match pad.property() {
            PadProp::None => None, // could be "none"
            PadProp::Bga => Some("pad_prop_bga"),
            PadProp::FiducialGlbl => Some("pad_prop_fiducial_glob"),
            PadProp::FiducialLocal => Some("pad_prop_fiducial_loc"),
            PadProp::Testpoint => Some("pad_prop_testpoint"),
            PadProp::Heatsink => Some("pad_prop_heatsink"),
            PadProp::Castellated => Some("pad_prop_castellated"),
            PadProp::Mechanical => Some("pad_prop_mechanical"),
            PadProp::Pressfit => Some("pad_prop_pressfit"),
        };

        let sim_electrical_type = match pad.sim_electrical_type() {
            PadSimElectricalType::Source => Some("source"),
            PadSimElectricalType::Sink => Some("sink"),
            PadSimElectricalType::None => None,
        };

        let number = self.out.quotew(pad.number());
        self.out.print(format_args!(
            "(pad {} {} {}",
            number,
            type_,
            shape_name(pad, PcbLayerId::FCu)
        ))?;

        let orientation = pad.orientation(&ctx);
        self.out.print(format_args!(
            "(at {} {})",
            format_iu_pt(pad.fp_relative_position(&ctx)),
            if orientation.is_zero() {
                String::new()
            } else {
                format_angle(orientation.as_degrees())
            }
        ))?;

        // Write the stored library size directly: it is the footprint-frame value the parser
        // reads back, and avoids a bake/unbake that is not the inverse of GetSize() for a
        // pad rotated within the footprint.
        self.out.print(format_args!(
            "(size {})",
            format_iu_pt(padstack.size(PcbLayerId::FCu))
        ))?;

        let delta = pad.delta(PcbLayerId::FCu);

        if delta.x != 0 || delta.y != 0 {
            self.out
                .print(format_args!("(rect_delta {})", format_iu_pt(delta)))?;
        }

        let drill = if pad.drill_shape() == PadDrillShape::Circle {
            unbake_size_uniform(pad.drill_size(&ctx), parent_fp)
        } else {
            unbake_size(pad.drill_size(&ctx), parent_fp)
        };
        let shapeoffset = pad.offset(&ctx, PcbLayerId::FCu);
        let mut force_shape_offset_output = false;

        padstack.for_each_unique_layer(|layer| {
            if pad.offset(&ctx, layer) != shapeoffset {
                force_shape_offset_output = true;
            }
        });

        if drill.x > 0
            || drill.y > 0
            || shapeoffset.x != 0
            || shapeoffset.y != 0
            || force_shape_offset_output
        {
            self.out.print(format_args!("(drill"))?;

            if pad.drill_shape() == PadDrillShape::Oblong {
                self.out.print(format_args!(" oval"))?;
            }

            if drill.x > 0 {
                self.out.print(format_args!(" {}", format_iu(drill.x)))?;
            }

            if drill.y > 0 && drill.x != drill.y {
                self.out.print(format_args!(" {}", format_iu(drill.y)))?;
            }

            // NOTE: Shape offest is a property of the copper shape, not of the drill, but this
            // was put in the file format under the drill section.  So, it is left here to
            // minimize file format changes, but note that the other padstack layers (if
            // present) will have an offset stored separately.
            if shapeoffset.x != 0 || shapeoffset.y != 0 || force_shape_offset_output {
                self.out.print(format_args!(
                    "(offset {})",
                    format_iu_pt(padstack.offset(PcbLayerId::FCu))
                ))?;
            }

            self.out.print(format_args!(")"))?;
        }

        for (name, drill) in [
            ("backdrill", padstack.secondary_drill()),
            ("tertiary_drill", padstack.tertiary_drill()),
        ] {
            if drill.size.x > 0 && drill.start != PcbLayerId::UndefinedLayer {
                let start = self.out.quotew(&Lset::name(drill.start));
                let end = self.out.quotew(&Lset::name(drill.end));
                self.out.print(format_args!(
                    "({name} (size {}) (layers {start} {end}))",
                    format_iu(drill.size.x)
                ))?;
            }
        }

        self.format_post_machining("front_post_machining", padstack.front_post_machining())?;
        self.format_post_machining("back_post_machining", padstack.back_post_machining())?;

        // Add pad property, if exists.
        if let Some(property) = property {
            self.out.print(format_args!("(property {property})"))?;
        }

        if let Some(sim_electrical_type) = sim_electrical_type {
            self.out
                .print(format_args!("(sim_electrical_type {sim_electrical_type})"))?;
        }

        self.format_layers(
            pad.layer_set(&ctx),
            false, /* enumerate layers */
            false,
        )?;

        if pad.attribute() == PadAttrib::Pth {
            format_bool(self.out, "remove_unused_layers", pad.remove_unconnected())?;

            if pad.remove_unconnected() {
                format_bool(self.out, "keep_end_layers", pad.keep_top_bottom())?;

                // Will be nullptr in footprint library
                if let Some(board) = board {
                    self.out.print(format_args!("(zone_layer_connections"))?;

                    for layer in board.enabled_layers().cu_stack().0 {
                        if pad.zone_layer_override(layer) == ZoneLayerOverride::ForceFlashed {
                            let name = self.out.quotew(&Lset::name(layer));
                            self.out.print(format_args!(" {name}"))?;
                        }
                    }

                    self.out.print(format_args!(")"))?;
                }
            }
        }

        // For normal padstacks, this is the one and only set of properties.  For complex
        // ones, this will represent the front layer properties, and other layers will be
        // formatted below
        self.format_corner_properties(pad, PcbLayerId::FCu)?;

        // Unconnected pad is default net so don't save it.
        if (self.ctl & CTL_OMIT_PAD_NETS) == 0 && pad.base.net.0 > 0 {
            let name = self.out.quotew(netname(pad, &ctx));
            self.out.print(format_args!("(net {name})"))?;
        }

        // Pin functions and types are closely related to nets, so if CTL_OMIT_NETS is set,
        // omit them as well (for instance when saved from library editor).
        if (self.ctl & CTL_OMIT_PAD_NETS) == 0 {
            if !pad.pin_function().is_empty() {
                let name = self.out.quotew(pad.pin_function());
                self.out.print(format_args!("(pinfunction {name})"))?;
            }

            if !pad.pin_type().is_empty() {
                let name = self.out.quotew(pad.pin_type());
                self.out.print(format_args!("(pintype {name})"))?;
            }
        }

        if pad.pad_to_die_length() != 0 {
            self.out.print(format_args!(
                "(die_length {})",
                format_iu(pad.pad_to_die_length())
            ))?;
        }

        if pad.pad_to_die_delay() != 0 {
            self.out.print(format_args!(
                "(die_delay {})",
                format_internal_units_dt(pad.pad_to_die_delay(), EdaDataType::Time)
            ))?;
        }

        if let Some(margin) = pad.local_solder_mask_margin() {
            self.out
                .print(format_args!("(solder_mask_margin {})", format_iu(margin)))?;
        }

        if let Some(margin) = pad.local_solder_paste_margin() {
            self.out
                .print(format_args!("(solder_paste_margin {})", format_iu(margin)))?;
        }

        if let Some(ratio) = pad.local_solder_paste_margin_ratio() {
            self.out.print(format_args!(
                "(solder_paste_margin_ratio {})",
                format_double2_str(ratio)
            ))?;
        }

        if let Some(clearance) = pad.local_clearance() {
            self.out
                .print(format_args!("(clearance {})", format_iu(clearance)))?;
        }

        if pad.local_zone_connection_raw() != ZoneConnection::Inherited as i32 {
            self.out.print(format_args!(
                "(zone_connect {})",
                pad.local_zone_connection_raw()
            ))?;
        }

        if let Some(width) = pad.local_thermal_spoke_width_override() {
            self.out
                .print(format_args!("(thermal_bridge_width {})", format_iu(width)))?;
        }

        if pad.thermal_spoke_angle()
            != padstack.default_thermal_spoke_angle_for_shape(PcbLayerId::FCu)
        {
            self.out.print(format_args!(
                "(thermal_bridge_angle {})",
                format_angle(pad.thermal_spoke_angle().as_degrees())
            ))?;
        }

        if let Some(gap) = pad.local_thermal_gap() {
            self.out
                .print(format_args!("(thermal_gap {})", format_iu(gap)))?;
        }

        if pad.shape(PcbLayerId::FCu) == PadShape::Custom {
            self.format_custom_options(pad, PcbLayerId::FCu)?;

            // Output graphic primitive of the pad shape
            self.format_primitives(pad, PcbLayerId::FCu)?;
        }

        let td_params = &pad.base.teardrop_params;

        if !is_default_teardrop_parameters(td_params) {
            self.format_teardrop_parameters(td_params)?;
        }

        if padstack.front_outer_layers().has_solder_mask.is_some()
            || padstack.back_outer_layers().has_solder_mask.is_some()
        {
            self.out.print_nest(0, format_args!(" (tenting "))?;
            format_opt_bool(
                self.out,
                "front",
                padstack.front_outer_layers().has_solder_mask,
            )?;
            format_opt_bool(
                self.out,
                "back",
                padstack.back_outer_layers().has_solder_mask,
            )?;
            self.out.print_nest(0, format_args!(")"))?;
        }

        format_uuid(self.out, &pad.base.item.uuid.as_string())?;

        if padstack.mode() != PadstackMode::Normal {
            if padstack.mode() == PadstackMode::FrontInnerBack {
                self.out
                    .print(format_args!("(padstack (mode front_inner_back)"))?;

                self.out.print(format_args!("(layer \"Inner\""))?;
                self.format_pad_layer(pad, Padstack::INNER_LAYERS)?;
                self.out.print(format_args!(")"))?;
                self.out.print(format_args!("(layer \"B.Cu\""))?;
                self.format_pad_layer(pad, PcbLayerId::BCu)?;
                self.out.print(format_args!(")"))?;
            } else {
                self.out.print(format_args!("(padstack (mode custom)"))?;

                let layer_count = match board {
                    Some(board) => board.copper_layer_count(),
                    None => MAX_CU_LAYERS,
                };

                // LAYER_RANGE( F_Cu, B_Cu, n ) never fails: both ends are copper layers.
                let range = LayerRange::new(PcbLayerId::FCu, PcbLayerId::BCu, layer_count)
                    .map_err(|_| IoError::new("invalid layer range"))?;

                for layer in range.iter() {
                    if layer == PcbLayerId::FCu {
                        continue;
                    }

                    // Without a board there is no layer count, so list only the layers the
                    // pad defines
                    if board.is_none() && !padstack.has_explicit_definition_for_layer(layer) {
                        continue;
                    }

                    let name = self.out.quotew(&Lset::name(layer));
                    self.out.print(format_args!("(layer {name}"))?;
                    self.format_pad_layer(pad, layer)?;
                    self.out.print(format_args!(")"))?;
                }
            }

            self.out.print(format_args!(")"))?;
        }

        format_custom_properties(self.out, &pad.base.item.custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }

    /// The `formatCornerProperties` lambda of `format( const PAD* )`.
    fn format_corner_properties(&mut self, pad: &Pad, layer: PcbLayerId) -> Result<(), IoError> {
        let shape = pad.shape(layer);

        // Output the radius ratio for rounded and chamfered rect pads
        if shape == PadShape::Roundrect || shape == PadShape::ChamferedRect {
            self.out.print(format_args!(
                "(roundrect_rratio {})",
                format_double2_str(pad.round_rect_radius_ratio(layer))
            ))?;
        }

        // Output the chamfer corners for chamfered rect pads
        if shape == PadShape::ChamferedRect {
            self.out.print(format_args!(
                "(chamfer_ratio {})",
                format_double2_str(pad.chamfer_rect_ratio(layer))
            ))?;

            self.out.print(format_args!("(chamfer"))?;

            let positions = pad.chamfer_positions(layer);

            if (positions & RECT_CHAMFER_TOP_LEFT) != 0 {
                self.out.print(format_args!(" top_left"))?;
            }

            if (positions & RECT_CHAMFER_TOP_RIGHT) != 0 {
                self.out.print(format_args!(" top_right"))?;
            }

            if (positions & RECT_CHAMFER_BOTTOM_LEFT) != 0 {
                self.out.print(format_args!(" bottom_left"))?;
            }

            if (positions & RECT_CHAMFER_BOTTOM_RIGHT) != 0 {
                self.out.print(format_args!(" bottom_right"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        Ok(())
    }

    /// The `(options (clearance ..) (anchor ..))` block written for a custom shape on `layer`
    /// (inline twice in `format( const PAD* )`).
    fn format_custom_options(&mut self, pad: &Pad, layer: PcbLayerId) -> Result<(), IoError> {
        self.out.print(format_args!("(options"))?;

        if pad.custom_shape_in_zone_opt() == CustomShapeZoneMode::Convexhull {
            self.out.print(format_args!("(clearance convexhull)"))?;
        } else {
            self.out.print(format_args!("(clearance outline)"))?;
        }

        // Output the anchor pad shape (circle/rect)
        self.out
            .print(format_args!("(anchor {})", anchor_shape(pad, layer)))?;

        self.out.print(format_args!(")"))?; // end of (options ...
        Ok(())
    }

    /// The `formatPrimitives` lambda of `format( const PAD* )`: `(primitives ..)` of `layer`,
    /// in library coordinates.
    fn format_primitives(&mut self, pad: &Pad, layer: PcbLayerId) -> Result<(), IoError> {
        let ctx = self.ctx;

        self.out.print(format_args!("(primitives"))?;

        // Output all basic shapes
        for primitive in pad.lib_padstack().primitives(layer) {
            let lib = &primitive.lib;
            let lib_shape = lib.shape;

            match lib_shape {
                ShapeT::Segment => {
                    let keyword = if primitive.is_proxy_item() {
                        "gr_vector"
                    } else {
                        "gr_line"
                    };
                    self.out.print(format_args!(
                        "({keyword} (start {}) (end {})",
                        format_iu_pt(lib.start),
                        format_iu_pt(lib.end)
                    ))?;
                }
                ShapeT::Rectangle => {
                    if primitive.is_proxy_item() {
                        self.out.print(format_args!(
                            "(gr_bbox (start {}) (end {})",
                            format_iu_pt(lib.start),
                            format_iu_pt(lib.end)
                        ))?;
                    } else {
                        self.out.print(format_args!(
                            "(gr_rect (start {}) (end {})",
                            format_iu_pt(lib.start),
                            format_iu_pt(lib.end)
                        ))?;

                        if primitive.shape.corner_radius() > 0 {
                            self.out.print(format_args!(
                                " (radius {})",
                                format_iu(lib.corner_radius)
                            ))?;
                        }
                    }
                }
                ShapeT::Arc => {
                    self.out.print(format_args!(
                        "(gr_arc (start {}) (mid {}) (end {})",
                        format_iu_pt(lib.start),
                        format_iu_pt(lib.arc_mid),
                        format_iu_pt(lib.end)
                    ))?;
                }
                ShapeT::Circle => {
                    self.out.print(format_args!(
                        "(gr_circle (center {}) (end {})",
                        format_iu_pt(lib.start),
                        format_iu_pt(lib.end)
                    ))?;
                }
                ShapeT::Bezier => {
                    // Pad primitives are stored in raw library coordinates and read back
                    // raw, so emit library coordinates like the other primitive types above.
                    self.out.print(format_args!(
                        "(gr_curve (pts (xy {}) (xy {}) (xy {}) (xy {}))",
                        format_iu_pt(lib.start),
                        format_iu_pt(lib.bezier_c1),
                        format_iu_pt(lib.bezier_c2),
                        format_iu_pt(lib.end)
                    ))?;
                }
                ShapeT::Poly if primitive.shape.is_poly_shape_valid() => {
                    let outline = lib.poly.c_outline(0);

                    self.out.print(format_args!("(gr_poly"))?;
                    self.format_poly_pts(outline, None)?;
                }
                _ => {}
            }

            if !primitive.is_proxy_item() {
                self.out.print(format_args!(
                    "(width {})",
                    format_iu(EdaShapeItem::width(primitive, &ctx))
                ))?;
            }

            // The filled flag represents if a solid fill is present on circles, rectangles
            // and polygons
            if matches!(lib_shape, ShapeT::Poly | ShapeT::Rectangle | ShapeT::Circle) {
                format_bool(self.out, "fill", primitive.shape.is_solid_fill())?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.out.print(format_args!(")"))?; // end of (primitives
        Ok(())
    }

    /// The `formatPadLayer` lambda of `format( const PAD* )`: the properties of one layer of a
    /// complex padstack.
    fn format_pad_layer(&mut self, pad: &Pad, layer: PcbLayerId) -> Result<(), IoError> {
        let ctx = self.ctx;
        let padstack = pad.lib_padstack();

        self.out
            .print(format_args!("(shape {})", shape_name(pad, layer)))?;

        self.out.print(format_args!(
            "(size {})",
            format_iu_pt(padstack.size(layer))
        ))?;

        let delta: Vector2I = pad.delta(layer);

        if delta.x != 0 || delta.y != 0 {
            self.out
                .print(format_args!("(rect_delta {})", format_iu_pt(delta)))?;
        }

        let shapeoffset = pad.offset(&ctx, layer);

        if shapeoffset.x != 0 || shapeoffset.y != 0 {
            self.out.print(format_args!(
                "(offset {})",
                format_iu_pt(padstack.offset(layer))
            ))?;
        }

        self.format_corner_properties(pad, layer)?;

        if pad.shape(layer) == PadShape::Custom {
            self.format_custom_options(pad, layer)?;

            // Output graphic primitive of the pad shape
            self.format_primitives(pad, layer)?;
        }

        let mut default_layer_angle = ANGLE_90;

        if pad.shape(layer) == PadShape::Circle
            || (pad.shape(layer) == PadShape::Custom
                && pad.anchor_pad_shape(layer) == PadShape::Circle)
        {
            default_layer_angle = ANGLE_45;
        }

        let layer_spoke_angle = padstack.thermal_spoke_angle(layer);

        if layer_spoke_angle != default_layer_angle {
            self.out.print(format_args!(
                "(thermal_bridge_angle {})",
                format_angle(layer_spoke_angle.as_degrees())
            ))?;
        }

        if let Some(gap) = padstack.thermal_gap(layer) {
            self.out
                .print(format_args!("(thermal_gap {})", format_iu(gap)))?;
        }

        if let Some(width) = padstack.thermal_spoke_width(layer) {
            self.out
                .print(format_args!("(thermal_bridge_width {})", format_iu(width)))?;
        }

        if let Some(clearance) = padstack.clearance(layer) {
            self.out
                .print(format_args!("(clearance {})", format_iu(clearance)))?;
        }

        if let Some(connection) = padstack.zone_connection_raw(layer) {
            self.out
                .print(format_args!("(zone_connect {})", connection))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Ports of the C++ pad save / reload tests. The C++ fixtures save to a file and read it
    //! back (`KI_TEST::DumpBoardToFile` / `ReadBoardFromFile`, or a `.kicad_mod` for
    //! `test_pad_roundtrip.cpp`); here the board is formatted to a string and parsed again.
    //! Footprint-library files are package PCBIO-14, so `test_pad_roundtrip.cpp` round-trips
    //! through a board file. `PAD::AddPrimitive` has no model port yet: the custom-shape cases
    //! of `test_custom_pad_primitive_roundtrip.cpp` build their pad by parsing a board text
    //! (the parser runs the same `AddPrimitive` statements) instead of with the setters.

    use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
    use kicad_geom::{EdaAngle, Vector2I};

    use crate::AddMode;
    use crate::board::Board;
    use crate::ctx::{BoardCtx, MutEnv};
    use crate::eda_shape::ShapeT;
    use crate::footprint::FootprintStackup;
    use crate::ids::{FootprintId, PadId};
    use crate::io::parser::ParseOptions;
    use crate::io::writer::FormatOptions;
    use crate::io::{format_board, parse_board};
    use crate::item::{BoardUse, ParentRef};
    use crate::item_ref::OwnedItem;
    use crate::pad::Pad;
    use crate::padstack::{PadAttrib, PadShape, Padstack};
    use crate::pcb_shape::PcbShape;

    fn mm(value: f64) -> i32 {
        PCB_IU_SCALE.mm_to_iu(value)
    }

    /// `new PAD( fp )` and the mutator environment of a child of `fp`.
    fn new_pad(board: &mut Board, fp: FootprintId) -> (Pad, MutEnv) {
        let parent = ParentRef::Item(fp.into());
        let mut pad = Pad::new(&mut board.kiid_generator);
        pad.base.item.parent = parent;
        (pad, board.child_env(parent))
    }

    /// `fp->Add( pad, ADD_MODE::APPEND, true )`.
    fn add_pad(board: &mut Board, fp: FootprintId, pad: Pad) -> PadId {
        let id = board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
            .unwrap_or_else(|_| panic!("FOOTPRINT::Add refused a pad"));
        id.typed().expect("a pad")
    }

    /// `DumpBoardToFile` + `ReadBoardFromFile`.
    fn round_trip(board: &Board) -> (String, Board) {
        let text = format_board(board, &FormatOptions::default()).expect("save");
        let reloaded = parse_board(&text, "board.kicad_pcb", &ParseOptions::default())
            .unwrap_or_else(|e| panic!("reload: {e}\n{text}"));
        (text, reloaded)
    }

    fn first_pad(board: &Board) -> &Pad {
        let (fp, _) = board
            .footprints()
            .next()
            .expect("Footprint not found in reloaded board");
        let (_, pad) = board
            .footprint_pads(fp)
            .next()
            .expect("Pad not found in reloaded footprint");
        pad
    }

    /// A footprint at (50, 50) mm with the given orientation and scale, attached to `board`.
    fn transformed_footprint(board: &mut Board, angle: f64, sx: f64, sy: f64) -> FootprintId {
        let fp = board.create_footprint();
        board.footprint_set_position(fp, Vector2I::new(mm(50.0), mm(50.0)));

        if angle != 0.0 {
            board.footprint_set_orientation(fp, EdaAngle::from_degrees(angle));
        }

        board.footprint_set_transform_scale(fp, sx, sy);
        fp
    }

    /// An SMD pad on F.Cu with a library-frame size.
    fn smd_pad(board: &mut Board, fp: FootprintId, shape: PadShape, lib_size: Vector2I) -> Pad {
        let (mut pad, env) = new_pad(board, fp);
        pad.set_number("1");
        pad.set_attribute(PadAttrib::Smd);
        pad.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::FCu]));
        pad.set_shape(Padstack::ALL_LAYERS, shape);
        pad.set_lib_size(Padstack::ALL_LAYERS, lib_size);
        pad
    }

    // ---- qa/tests/pcbnew/test_pad_roundtrip.cpp ---------------------------------------------

    /// `PadRoundTrip/CopperOnlyPad`: a copper-only pad must survive a save/reload intact
    /// (board file instead of the C++ `.kicad_mod`, see the module comment).
    #[test]
    fn copper_only_pad() {
        let mut board = Board::new();
        board.set_copper_layer_count(4);
        let fp = board.create_footprint();

        let (mut pad, env) = new_pad(&mut board, fp);
        pad.set_number("1");
        pad.set_attribute(PadAttrib::Smd);
        pad.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::FCu]));
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Rectangle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(2.0)));

        pad.set_attribute(PadAttrib::NonComponent);
        pad.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::In1Cu]));
        add_pad(&mut board, fp, pad);

        // Inner-layer pads need a custom stackup to be properly represented.
        {
            let footprint = board.footprints.get_mut(fp).unwrap();
            footprint.set_stackup_mode(FootprintStackup::CustomLayers);
            footprint.set_stackup_layers(Lset::all_cu_mask_n(4));
        }

        board.attach_footprint(fp, AddMode::Append);

        let (_, loaded) = round_trip(&board);
        let ctx = BoardCtx::bare(&loaded);
        let (lfp, _) = loaded.footprints().next().unwrap();
        assert_eq!(loaded.footprint_pads(lfp).count(), 1);

        let loaded_pad = first_pad(&loaded);
        assert_eq!(loaded_pad.attribute(), PadAttrib::NonComponent);
        assert_eq!(
            loaded_pad.layer_set(&ctx),
            Lset::from_layers(&[PcbLayerId::In1Cu])
        );
        assert_eq!(loaded_pad.drill_size(&ctx).x, 0);
        assert_eq!(loaded_pad.drill_size(&ctx).y, 0);
    }

    // ---- qa/tests/pcbnew/test_pad_tenting.cpp -----------------------------------------------

    /// The fixture of the pad cases: an FP-holder board with one footprint at the origin.
    fn tenting_board() -> (Board, FootprintId) {
        let mut board = Board::new();
        board.board_use = BoardUse::FpHolder;
        let fp = board.create_footprint();
        board.footprint_set_position(fp, Vector2I::new(0, 0));
        (board, fp)
    }

    fn pth_pad(board: &mut Board, fp: FootprintId) -> Pad {
        let (mut pad, env) = new_pad(board, fp);
        pad.set_attribute(PadAttrib::Pth);
        pad.set_layer_set(&env, Pad::pth_mask());
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Circle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.5), mm(1.5)));
        pad.set_drill_size(&env, Vector2I::new(mm(0.8), mm(0.8)));
        pad.set_number("1");
        pad.set_position(&env, Vector2I::new(0, 0));
        pad
    }

    fn save(board: &Board) -> String {
        format_board(board, &FormatOptions::default()).expect("save")
    }

    /// `PadTenting/DefaultTentingNotSerialized`.
    #[test]
    fn default_tenting_not_serialized() {
        let (mut board, fp) = tenting_board();
        let pad = pth_pad(&mut board, fp);
        assert!(pad.padstack.front_outer_layers().has_solder_mask.is_none());
        assert!(pad.padstack.back_outer_layers().has_solder_mask.is_none());
        add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let output = save(&board);
        let tenting = output.find("(tenting");
        assert!(
            tenting.is_none() || tenting < output.find("(footprint"),
            "PTH pad with default tenting should not have a (tenting) block. Board-level \
             tenting in setup section is expected."
        );
    }

    /// `PadTenting/ExplicitTentingSerialized`.
    #[test]
    fn explicit_tenting_serialized() {
        let (mut board, fp) = tenting_board();
        let mut pad = pth_pad(&mut board, fp);
        pad.padstack.front_outer_layers_mut().has_solder_mask = Some(true);
        assert!(pad.padstack.front_outer_layers().has_solder_mask.is_some());
        add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let output = save(&board);
        let fp_start = output.find("(footprint").expect("(footprint");
        assert!(
            output[fp_start..].contains("(tenting"),
            "PTH pad with explicit tenting should have a (tenting) block"
        );
    }

    /// `PadTenting/NpthDefaultTentingNotSerialized`.
    #[test]
    fn npth_default_tenting_not_serialized() {
        let (mut board, fp) = tenting_board();
        let (mut pad, env) = new_pad(&mut board, fp);
        pad.set_attribute(PadAttrib::Npth);
        pad.set_layer_set(&env, Pad::unplated_hole_mask());
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Circle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(1.0)));
        pad.set_drill_size(&env, Vector2I::new(mm(1.0), mm(1.0)));
        pad.set_number("");
        pad.set_position(&env, Vector2I::new(PCB_IU_SCALE.mm_to_iu(5.0), 0));
        assert!(pad.padstack.front_outer_layers().has_solder_mask.is_none());
        assert!(pad.padstack.back_outer_layers().has_solder_mask.is_none());
        add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let output = save(&board);
        let fp_start = output.find("(footprint").expect("(footprint");
        assert!(
            !output[fp_start..].contains("(tenting"),
            "NPTH pad with default tenting should not have a (tenting) block"
        );
    }

    // ---- qa/tests/pcbnew/test_custom_pad_primitive_roundtrip.cpp ----------------------------

    /// A board with one footprint at (50, 25) mm, rotated 30 degrees and scaled (2, 1.5),
    /// holding a custom pad with the given primitives (library coordinates in mm).
    fn custom_pad_board(primitives: &str) -> Board {
        let text = format!(
            "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")
              (general (thickness 1.6) (legacy_teardrops no))
              (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (1 \"F.Mask\" user) (3 \"B.Mask\" user))
              (footprint \"U1\" (layer \"F.Cu\")
                (transform (translate 50 25) (rotate 30) (scale 2 1.5))
                (pad \"1\" smd custom (at 0 0) (size 1 1) (layers \"F.Cu\")
                  (options (clearance outline) (anchor circle))
                  (primitives {primitives}))))"
        );
        parse_board(&text, "board.kicad_pcb", &ParseOptions::default()).expect("fixture")
    }

    fn find_primitive(pad: &Pad, shape: ShapeT) -> Option<&PcbShape> {
        pad.lib_padstack()
            .primitives(Padstack::ALL_LAYERS)
            .iter()
            .find(|prim| prim.shape.shape() == shape)
    }

    /// `CustomPadPrimitiveRoundTrip/BezierAndPolyOnTransformedFootprint`: a custom pad's
    /// bezier and polygon primitives must survive a save/reload unchanged, even when the
    /// parent footprint carries a non-trivial transform (rotated + non-uniform scale).
    #[test]
    fn bezier_and_poly_on_transformed_footprint() {
        let board = custom_pad_board(
            "(gr_curve (pts (xy 0 0) (xy 1 2) (xy 3 -1) (xy 4 0.5)) (width 0.1))
             (gr_poly (pts (xy 1 0) (xy 3 1) (xy 2 4) (xy -1 2)) (width 0.1) (fill no))",
        );
        let ctx = BoardCtx::bare(&board);
        let pad = first_pad(&board);
        let bezier = find_primitive(pad, ShapeT::Bezier).expect("bezier");
        let poly = find_primitive(pad, ShapeT::Poly).expect("poly");

        // The values the C++ fixture sets.
        assert_eq!(bezier.lib_start(), Vector2I::new(0, 0));
        assert_eq!(bezier.lib.bezier_c1, Vector2I::new(mm(1.0), mm(2.0)));
        assert_eq!(bezier.lib.bezier_c2, Vector2I::new(mm(3.0), mm(-1.0)));
        assert_eq!(bezier.lib_end(), Vector2I::new(mm(4.0), mm(0.5)));

        let exp_bezier = (
            bezier.lib_start(),
            bezier.lib.bezier_c1,
            bezier.lib.bezier_c2,
            bezier.lib_end(),
            bezier.width(&ctx),
        );
        let exp_poly = poly.lib.poly.clone();
        let exp_poly_width = poly.width(&ctx);

        let (_, reloaded) = round_trip(&board);
        let rctx = BoardCtx::bare(&reloaded);
        let pad_found = first_pad(&reloaded);

        let bezier_found =
            find_primitive(pad_found, ShapeT::Bezier).expect("Bezier primitive not found");
        assert_eq!(
            (
                bezier_found.lib_start(),
                bezier_found.lib.bezier_c1,
                bezier_found.lib.bezier_c2,
                bezier_found.lib_end(),
                bezier_found.width(&rctx),
            ),
            exp_bezier
        );

        let poly_found =
            find_primitive(pad_found, ShapeT::Poly).expect("Polygon primitive not found");
        assert!(poly_found.shape.is_poly_shape_valid());

        let got = &poly_found.lib.poly;
        assert_eq!(got.outline_count(), exp_poly.outline_count());
        let (exp_outline, got_outline) = (exp_poly.c_outline(0), got.c_outline(0));
        assert_eq!(got_outline.point_count(), exp_outline.point_count());

        for ii in 0..exp_outline.point_count() {
            assert_eq!(got_outline.c_point(ii), exp_outline.c_point(ii));
        }

        assert_eq!(poly_found.width(&rctx), exp_poly_width);
    }

    /// `CustomPadPrimitiveRoundTrip/SimplePadPrimitivesOnTransformedFootprint`: a segment and
    /// a circle (which the non-uniform scale bakes into an ellipse) keep their library
    /// geometry over a save/reload.
    #[test]
    fn simple_pad_primitives_on_transformed_footprint() {
        let board = custom_pad_board(
            "(gr_line (start 1 1) (end 6 2) (width 0.1))
             (gr_circle (center 3 3) (end 4 3) (width 0.01))",
        );
        let ctx = BoardCtx::bare(&board);
        let pad = first_pad(&board);
        let seg = find_primitive(pad, ShapeT::Segment).expect("segment");
        let circle = find_primitive(pad, ShapeT::Ellipse).expect("circle baked to an ellipse");

        let exp_seg = (seg.lib_start(), seg.lib_end(), seg.width(&ctx));
        let exp_circle_center = circle.lib_start();
        let exp_circle_radius =
            kicad_geom::ki_round(circle.shape.start().distance(circle.shape.end()));

        let (_, reloaded) = round_trip(&board);
        let rctx = BoardCtx::bare(&reloaded);
        let pad_found = first_pad(&reloaded);

        let seg_found =
            find_primitive(pad_found, ShapeT::Segment).expect("Segment primitive not found");
        assert_eq!(
            (
                seg_found.lib_start(),
                seg_found.lib_end(),
                seg_found.width(&rctx)
            ),
            exp_seg
        );

        let circle_found =
            find_primitive(pad_found, ShapeT::Ellipse).expect("Elipse primitive not found");
        assert_eq!(circle_found.lib_start(), exp_circle_center);
        assert_eq!(
            kicad_geom::ki_round(
                circle_found
                    .shape
                    .start()
                    .distance(circle_found.shape.end())
            ),
            exp_circle_radius
        );
    }

    /// `CustomPadPrimitiveRoundTrip/ScaledFootprintRoundTripStable`.
    #[test]
    fn scaled_footprint_round_trip_stable() {
        let mut board = Board::new();
        let fp = transformed_footprint(&mut board, 0.0, 2.0, 1.5);
        let pad = smd_pad(
            &mut board,
            fp,
            PadShape::Circle,
            Vector2I::new(mm(1.0), mm(1.0)),
        );
        add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let (_, reloaded) = round_trip(&board);
        let (_, r) = reloaded.footprints().next().unwrap();
        assert!((r.transform().scale_x() - 2.0).abs() <= 2.0 * 1e-8);
        assert!((r.transform().scale_y() - 1.5).abs() <= 1.5 * 1e-8);
    }

    /// `CustomPadPrimitiveRoundTrip/PadPositionAndOrientationRoundTripStable`.
    #[test]
    fn pad_position_and_orientation_round_trip_stable() {
        let mut board = Board::new();
        let fp = transformed_footprint(&mut board, 30.0, 2.0, 1.5);
        let mut pad = smd_pad(
            &mut board,
            fp,
            PadShape::Rectangle,
            Vector2I::new(mm(1.0), mm(0.5)),
        );
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.set_position(&env, Vector2I::new(mm(53.0), mm(51.0)));
        pad.set_orientation(&env, EdaAngle::from_degrees(75.0));
        let id = add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let ctx = BoardCtx::bare(&board);
        let pos_before = board.pads.get(id).unwrap().position(&ctx);
        let orient_before = board.pads.get(id).unwrap().orientation(&ctx);

        let (_, reloaded) = round_trip(&board);
        let rctx = BoardCtx::bare(&reloaded);
        let r = first_pad(&reloaded);
        assert_eq!(r.position(&rctx), pos_before);
        let after = r.orientation(&rctx).as_degrees();
        assert!((after - orient_before.as_degrees()).abs() <= orient_before.as_degrees() * 1e-8);
    }

    /// `CustomPadPrimitiveRoundTrip/PadDeltaRoundTripStable`.
    #[test]
    fn pad_delta_round_trip_stable() {
        let mut board = Board::new();
        let fp = transformed_footprint(&mut board, 0.0, 2.0, 1.5);
        let mut pad = smd_pad(
            &mut board,
            fp,
            PadShape::Trapezoid,
            Vector2I::new(mm(1.0), mm(1.0)),
        );
        pad.set_delta(Padstack::ALL_LAYERS, Vector2I::new(mm(0.4), mm(0.0)));
        let before = pad.delta(Padstack::ALL_LAYERS);
        add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let (_, reloaded) = round_trip(&board);
        assert_eq!(first_pad(&reloaded).delta(Padstack::ALL_LAYERS), before);
    }

    /// `CustomPadPrimitiveRoundTrip/PadOffsetRoundTripStable`.
    #[test]
    fn pad_offset_round_trip_stable() {
        let mut board = Board::new();
        let fp = transformed_footprint(&mut board, 0.0, 2.0, 1.5);
        let mut pad = smd_pad(
            &mut board,
            fp,
            PadShape::Rectangle,
            Vector2I::new(mm(1.0), mm(1.0)),
        );
        pad.set_lib_offset(Padstack::ALL_LAYERS, Vector2I::new(mm(0.3), mm(0.2)));
        let id = add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let before = board
            .pads
            .get(id)
            .unwrap()
            .offset(&BoardCtx::bare(&board), Padstack::ALL_LAYERS);

        let (_, reloaded) = round_trip(&board);
        let rctx = BoardCtx::bare(&reloaded);
        assert_eq!(
            first_pad(&reloaded).offset(&rctx, Padstack::ALL_LAYERS),
            before
        );
    }

    /// `CustomPadPrimitiveRoundTrip/RotatedPadSizeRoundTripStable`.
    #[test]
    fn rotated_pad_size_round_trip_stable() {
        let mut board = Board::new();
        let fp = transformed_footprint(&mut board, 0.0, 2.0, 1.5);
        let mut pad = smd_pad(
            &mut board,
            fp,
            PadShape::Rectangle,
            Vector2I::new(mm(1.0), mm(0.5)),
        );
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.set_orientation(&env, EdaAngle::from_degrees(90.0));
        let id = add_pad(&mut board, fp, pad);
        board.attach_footprint(fp, AddMode::Append);

        let before = board
            .pads
            .get(id)
            .unwrap()
            .size(&BoardCtx::bare(&board), Padstack::ALL_LAYERS);

        let (_, reloaded) = round_trip(&board);
        let rctx = BoardCtx::bare(&reloaded);
        assert_eq!(
            first_pad(&reloaded).size(&rctx, Padstack::ALL_LAYERS),
            before
        );
    }
}
