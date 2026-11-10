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

/// `static_cast<int>( ZONE_CONNECTION )`.
fn zone_connection_int(connection: ZoneConnection) -> i32 {
    connection as i32
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

        self.format_layers(pad.layer_set(&ctx), false /* enumerate layers */, false)?;

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

        if pad.local_zone_connection() != ZoneConnection::Inherited {
            self.out.print(format_args!(
                "(zone_connect {})",
                zone_connection_int(pad.local_zone_connection())
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
                ShapeT::Poly => {
                    if primitive.shape.is_poly_shape_valid() {
                        let outline = lib.poly.c_outline(0);

                        self.out.print(format_args!("(gr_poly"))?;
                        self.format_poly_pts(outline, None)?;
                    }
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

        self.out
            .print(format_args!("(size {})", format_iu_pt(padstack.size(layer))))?;

        let delta: Vector2I = pad.delta(layer);

        if delta.x != 0 || delta.y != 0 {
            self.out
                .print(format_args!("(rect_delta {})", format_iu_pt(delta)))?;
        }

        let shapeoffset = pad.offset(&ctx, layer);

        if shapeoffset.x != 0 || shapeoffset.y != 0 {
            self.out
                .print(format_args!("(offset {})", format_iu_pt(padstack.offset(layer))))?;
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

        if let Some(connection) = padstack.zone_connection(layer) {
            self.out.print(format_args!(
                "(zone_connect {})",
                zone_connection_int(connection)
            ))?;
        }

        Ok(())
    }
}
