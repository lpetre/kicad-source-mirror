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

//! Port of the pad part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-07): `parsePAD` (P:7320-8030), `parsePAD_primitives` (P:8033-8072),
//! `parsePAD_option` (P:8075-8119), `parsePostMachining` (P:8122-8168), `parsePadstack`
//! (P:8171-8428), plus `PAD::AddPrimitive` (`pcbnew/pad.cpp`), which the board model does not
//! provide yet.
//!
//! The pad is built as a local value whose parent is the footprint being parsed
//! (`std::make_unique<PAD>( aParent )`); mutators that depend on the parent frame
//! (`SetFPRelativePosition`, `SetOrientation`, `SetDrillSize`, `SetSize`) get the environment
//! of a child of that footprint (`Board::child_env`). The caller (`parseFOOTPRINT_unchecked`)
//! adds the returned pad to the footprint.

use kicad_core::layer_ids::is_copper_layer;
use kicad_core::layer_range::LayerRange;
use kicad_core::{EdaDataType, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::{
    RECT_CHAMFER_BOTTOM_LEFT, RECT_CHAMFER_BOTTOM_RIGHT, RECT_CHAMFER_TOP_LEFT,
    RECT_CHAMFER_TOP_RIGHT, RECT_NO_CHAMFER,
};
use kicad_geom::{ANGLE_0, ANGLE_45, ANGLE_90, EdaAngle, Vector2I, ki_round};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use crate::ctx::{BoardCtx, MutEnv};
use crate::eda_shape::ShapeT;
use crate::ids::{FootprintId, NetCode};
use crate::item::ParentRef;
use crate::pad::Pad;
use crate::padstack::{
    CustomShapeZoneMode, PadAttrib, PadDrillPostMachiningMode, PadDrillShape, PadProp, PadShape,
    PadSimElectricalType, Padstack, PadstackMode, PostMachiningProps, UnconnectedLayerMode,
};
use crate::pcb_shape::PcbShape;
use crate::pcb_track::ZoneLayerOverride;
use crate::zone_settings::ZoneConnection;

/// `(ZONE_CONNECTION) aValue` (top-level `zone_connect` of a pad) for the values the enum has.
/// The C++ cast keeps any other integer, which the Rust enum cannot hold: such a value reads
/// as `INHERITED` (`PORT_STATUS.md`, PCBIO-07 deviations; same rule as PCBIO-06).
fn zone_connection_cast(value: i32) -> ZoneConnection {
    zone_connection_enum_cast(value).unwrap_or(ZoneConnection::Inherited)
}

/// `magic_enum::enum_cast<ZONE_CONNECTION>( aValue )`: the enumerator with that value, if any.
fn zone_connection_enum_cast(value: i32) -> Option<ZoneConnection> {
    match value {
        -1 => Some(ZoneConnection::Inherited),
        0 => Some(ZoneConnection::None),
        1 => Some(ZoneConnection::Thermal),
        2 => Some(ZoneConnection::Full),
        3 => Some(ZoneConnection::ThtThermal),
        _ => None,
    }
}

/// `PAD::SetRemoveUnconnected( aSet )` (inline in `pad.h`).
fn set_remove_unconnected(pad: &mut Pad, set: bool) {
    pad.set_unconnected_layer_mode(if set {
        UnconnectedLayerMode::RemoveAll
    } else {
        UnconnectedLayerMode::KeepAll
    });
}

/// `PAD::SetKeepTopBottom( aSet )` (inline in `pad.h`).
fn set_keep_top_bottom(pad: &mut Pad, set: bool) {
    pad.set_unconnected_layer_mode(if set {
        UnconnectedLayerMode::RemoveExceptStartAndEnd
    } else {
        UnconnectedLayerMode::RemoveAll
    });
}

impl PcbParser {
    /// Port of `parsePAD( FOOTPRINT* aParent )`: call with `CurTok() == T_pad`. Returns the
    /// pad as a local value with `parent = Item( aParent )` (detached without a parent); the
    /// caller adds it. `Ok(None)` only when the C++ `wxCHECK` fails.
    pub(crate) fn parse_pad(
        &mut self,
        parent: Option<FootprintId>,
    ) -> Result<Option<Pad>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_pad,
            "Cannot parse {} as PAD.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_pad {
            return Ok(None);
        }

        let mut sz = Vector2I::default();
        let mut pt = Vector2I::default();
        let mut found_net = false;

        let parent_ref = match parent {
            Some(fp) => ParentRef::Item(fp.into()),
            None => ParentRef::None,
        };

        let mut pad = Pad::new(&mut self.board.kiid_generator);
        pad.base.item.parent = parent_ref;

        // The constructor's `m_zoneLayerOverrides` loop runs over
        // `LAYER_RANGE( F_Cu, B_Cu, BoardCopperLayerCount() )`: the board's copper layers when
        // the parent chain reaches a board, else 32 (what `Pad::new` assumes).
        if self.board_present && self.parent_reaches_board(parent_ref) {
            let count = self.board.copper_layer_count();
            pad.zone_layer_overrides.clear();

            if let Ok(range) = LayerRange::new(PcbLayerId::FCu, PcbLayerId::BCu, count) {
                for layer in range.iter() {
                    pad.zone_layer_overrides
                        .insert(layer, ZoneLayerOverride::None);
                }
            }
        }

        // The mutator environment of a child of `aParent` (its frame is fixed while the pad
        // is read: the footprint placement precedes the pads in the file).
        let env: MutEnv = self.board.child_env(parent_ref);

        self.lexer.need_symbol_or_number()?;
        pad.set_number(&self.lexer.from_utf8());

        let mut token = self.lexer.next_tok()?;

        // NB: all pre-padstack tokens are stored into the F_Cu layer.  For PADSTACK::MODE::NORMAL
        // this will be all there is.  For complex padstacks, the other values will get loaded
        // from the T_padstack record.

        match token {
            T::T_thru_hole => {
                pad.set_attribute(PadAttrib::Pth);

                // The drill token is usually missing if 0 drill size is specified.
                // Emulate it using 1 nm drill size to avoid errors.
                // Drill size cannot be set to 0 in newer versions.
                pad.set_drill_size(&env, Vector2I::new(1, 1));
            }
            T::T_smd => pad.set_attribute(PadAttrib::Smd),
            T::T_connect => pad.set_attribute(PadAttrib::Conn),
            T::T_non_component => pad.set_attribute(PadAttrib::NonComponent),
            T::T_np_thru_hole => pad.set_attribute(PadAttrib::Npth),
            _ => {
                return Err(self
                    .lexer
                    .expecting_str("thru_hole, smd, connect, non_component, or np_thru_hole"));
            }
        }

        token = self.lexer.next_tok()?;

        match token {
            T::T_circle => pad.set_shape(PcbLayerId::FCu, PadShape::Circle),
            T::T_rect => pad.set_shape(PcbLayerId::FCu, PadShape::Rectangle),
            T::T_oval => pad.set_shape(PcbLayerId::FCu, PadShape::Oval),
            T::T_trapezoid => pad.set_shape(PcbLayerId::FCu, PadShape::Trapezoid),
            // Note: the shape can be PAD_SHAPE::ROUNDRECT or PAD_SHAPE::CHAMFERED_RECT
            // (if chamfer parameters are found later in pad descr.)
            T::T_roundrect => pad.set_shape(PcbLayerId::FCu, PadShape::Roundrect),
            T::T_custom => pad.set_shape(PcbLayerId::FCu, PadShape::Custom),
            _ => {
                return Err(self
                    .lexer
                    .expecting_str("circle, rectangle, roundrect, oval, trapezoid or custom"));
            }
        }

        let mut thermal_br_angle_override: Option<EdaAngle> = None;

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_locked {
                // Pad locking is now a session preference
                token = self.lexer.next_tok()?;
            }

            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_size => {
                    sz.x = self.parse_board_units_str("width value")?;
                    sz.y = self.parse_board_units_str("height value")?;
                    pad.set_lib_size(PcbLayerId::FCu, sz);
                    self.lexer.need_right()?;
                }
                T::T_at => {
                    pt.x = self.parse_board_units_str("X coordinate")?;
                    pt.y = self.parse_board_units_str("Y coordinate")?;
                    pad.set_fp_relative_position(&env, pt);
                    token = self.lexer.next_tok()?;

                    // The pad angle in the file is a board frame absolute value. If it is
                    // missing, the pad is axis aligned regardless of the parent footprint
                    // orientation, matching the pre affine transform behavior.
                    if token == T::T_NUMBER {
                        let angle = EdaAngle::from_degrees(self.parse_double()?);
                        pad.set_orientation(&env, angle);
                        self.lexer.need_right()?;
                    } else if token == T::T_RIGHT {
                        pad.set_orientation(&env, ANGLE_0);
                    } else {
                        return Err(self.lexer.expecting_str(") or angle value"));
                    }
                }
                T::T_rect_delta => {
                    let delta = Vector2I::new(
                        self.parse_board_units_str("rectangle delta width")?,
                        self.parse_board_units_str("rectangle delta height")?,
                    );
                    pad.set_delta(PcbLayerId::FCu, delta);
                    self.lexer.need_right()?;
                }
                T::T_drill => {
                    let mut have_width = false;
                    let mut drill_size = pad.drill_size(&BoardCtx::bare(&self.board));

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token == T::T_LEFT {
                            token = self.lexer.next_tok()?;
                        }

                        match token {
                            T::T_oval => pad.set_drill_shape(PadDrillShape::Oblong),
                            T::T_NUMBER => {
                                if !have_width {
                                    drill_size.x = self.parse_board_units()?;

                                    // If height is not defined the width and height are the same.
                                    drill_size.y = drill_size.x;
                                    have_width = true;
                                } else {
                                    drill_size.y = self.parse_board_units()?;
                                }
                            }
                            T::T_offset => {
                                pt.x = self.parse_board_units_str("drill offset x")?;
                                pt.y = self.parse_board_units_str("drill offset y")?;
                                pad.set_lib_offset(PcbLayerId::FCu, pt);
                                self.lexer.need_right()?;
                            }
                            _ => return Err(self.lexer.expecting_str("oval, size, or offset")),
                        }

                        token = self.lexer.next_tok()?;
                    }

                    // This fixes a bug caused by setting the default PAD drill size to a value
                    // other than 0 used to fix a bunch of debug assertions even though it is
                    // defined as a through hole pad.  Wouldn't a though hole pad with no drill
                    // be a surface mount pad (or a conn pad which is a smd pad with no solder
                    // paste)?
                    if matches!(
                        pad.attribute(),
                        PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent
                    ) {
                        pad.set_lib_drill_size(Vector2I::new(0, 0));
                    } else {
                        pad.set_lib_drill_size(drill_size);
                    }
                }
                T::T_backdrill => {
                    // Parse: (backdrill (size ...) (layers start end))
                    self.parse_pad_extra_drill(&mut pad, false)?;
                }
                T::T_tertiary_drill => {
                    // Parse: (tertiary_drill (size ...) (layers start end))
                    self.parse_pad_extra_drill(&mut pad, true)?;
                }
                T::T_layers => {
                    let layer_mask = self.parse_board_item_layers_as_mask()?;
                    pad.set_layer_set(&env, layer_mask);
                }
                T::T_net => {
                    found_net = true;
                    self.parse_pad_net(&mut pad)?;
                }
                T::T_pinfunction => {
                    self.lexer.need_symbol_or_number()?;
                    pad.set_pin_function(&self.lexer.from_utf8());
                    self.lexer.need_right()?;
                }
                T::T_pintype => {
                    self.lexer.need_symbol_or_number()?;
                    pad.set_pin_type(&self.lexer.from_utf8());
                    self.lexer.need_right()?;
                }
                T::T_sim_electrical_type => {
                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_source => pad.set_sim_electrical_type(PadSimElectricalType::Source),
                        T::T_sink => pad.set_sim_electrical_type(PadSimElectricalType::Sink),
                        _ => return Err(self.lexer.expecting_str("sink or source")),
                    }

                    self.lexer.need_right()?;
                }
                T::T_die_length => {
                    let length =
                        self.parse_board_units_tok(T::T_die_length, EdaDataType::Distance)?;
                    pad.set_pad_to_die_length(length);
                    self.lexer.need_right()?;
                }
                T::T_die_delay => {
                    let delay = if self.required_version <= 20250926 {
                        self.parse_board_units_tok(T::T_die_delay, EdaDataType::Distance)?
                    } else {
                        self.parse_board_units_tok(T::T_die_delay, EdaDataType::Time)?
                    };

                    pad.set_pad_to_die_delay(delay);
                    self.lexer.need_right()?;
                }
                T::T_solder_mask_margin => {
                    let margin = self.parse_board_units_str("local solder mask margin value")?;
                    pad.set_local_solder_mask_margin(Some(margin));
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && pad.local_solder_mask_margin() == Some(0)
                    {
                        pad.set_local_solder_mask_margin(None);
                    }
                }
                T::T_solder_paste_margin => {
                    let margin = self.parse_board_units_str("local solder paste margin value")?;
                    pad.set_local_solder_paste_margin(Some(margin));
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && pad.local_solder_paste_margin() == Some(0)
                    {
                        pad.set_local_solder_paste_margin(None);
                    }
                }
                T::T_solder_paste_margin_ratio => {
                    let ratio =
                        self.parse_double_expected("local solder paste margin ratio value")?;
                    pad.set_local_solder_paste_margin_ratio(Some(ratio));
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && pad.local_solder_paste_margin_ratio() == Some(0.0)
                    {
                        pad.set_local_solder_paste_margin_ratio(None);
                    }
                }
                T::T_clearance => {
                    let clearance = self.parse_board_units_str("local clearance value")?;
                    pad.set_local_clearance(Some(clearance));
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201 && pad.local_clearance() == Some(0) {
                        pad.set_local_clearance(None);
                    }
                }
                T::T_teardrops => {
                    self.parse_teardrop_parameters(&mut pad.base.teardrop_params)?;
                }
                T::T_zone_connect => {
                    let value = self.parse_int_expected("zone connection value")?;
                    pad.set_local_zone_connection(zone_connection_cast(value));
                    self.lexer.need_right()?;
                }
                // legacy token thermal_width
                T::T_thermal_width | T::T_thermal_bridge_width => {
                    let width = self.parse_board_units_tok(token, EdaDataType::Distance)?;
                    pad.set_local_thermal_spoke_width_override(Some(width));
                    self.lexer.need_right()?;
                }
                T::T_thermal_bridge_angle => {
                    thermal_br_angle_override = Some(EdaAngle::from_degrees(
                        self.parse_double_expected("thermal spoke angle")?,
                    ));
                    self.lexer.need_right()?;
                }
                T::T_thermal_gap => {
                    let gap = self.parse_board_units_str("thermal relief gap value")?;
                    pad.set_thermal_gap(gap);
                    self.lexer.need_right()?;
                }
                T::T_roundrect_rratio => {
                    let ratio = self.parse_double_expected("roundrect radius ratio")?;
                    pad.set_round_rect_radius_ratio(PcbLayerId::FCu, ratio);
                    self.lexer.need_right()?;
                }
                T::T_chamfer_ratio => {
                    let ratio = self.parse_double_expected("chamfer ratio")?;
                    pad.set_chamfer_rect_ratio(PcbLayerId::FCu, ratio);

                    if pad.chamfer_rect_ratio(PcbLayerId::FCu) > 0.0 {
                        pad.set_shape(PcbLayerId::FCu, PadShape::ChamferedRect);
                    }

                    self.lexer.need_right()?;
                }
                T::T_chamfer => {
                    let chamfers = self.parse_chamfer_list(
                        "chamfer_top_left chamfer_top_right chamfer_bottom_left or \
                         chamfer_bottom_right",
                    )?;
                    pad.set_chamfer_positions(PcbLayerId::FCu, chamfers);

                    if pad.chamfer_positions(PcbLayerId::FCu) != RECT_NO_CHAMFER {
                        pad.set_shape(PcbLayerId::FCu, PadShape::ChamferedRect);
                    }
                }
                T::T_property => {
                    while token != T::T_RIGHT {
                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_pad_prop_bga => pad.set_property(PadProp::Bga),
                            T::T_pad_prop_fiducial_glob => pad.set_property(PadProp::FiducialGlbl),
                            T::T_pad_prop_fiducial_loc => pad.set_property(PadProp::FiducialLocal),
                            T::T_pad_prop_testpoint => pad.set_property(PadProp::Testpoint),
                            T::T_pad_prop_castellated => pad.set_property(PadProp::Castellated),
                            T::T_pad_prop_heatsink => pad.set_property(PadProp::Heatsink),
                            T::T_pad_prop_mechanical => pad.set_property(PadProp::Mechanical),
                            T::T_pad_prop_pressfit => pad.set_property(PadProp::Pressfit),
                            T::T_none => pad.set_property(PadProp::None),
                            // Currently: skip unknown property
                            _ => {}
                        }
                    }
                }
                T::T_options => self.parse_pad_option(&mut pad, PcbLayerId::FCu)?,
                T::T_padstack => self.parse_padstack(&mut pad)?,
                // Primitives at the top level are put in the padstack's F_Cu; other layers will
                // be parsed in parsePadstack().
                T::T_primitives => self.parse_pad_primitives(&mut pad, PcbLayerId::FCu)?,
                T::T_remove_unused_layers => {
                    let remove = self.parse_maybe_absent_bool(true)?;
                    set_remove_unconnected(&mut pad, remove);
                }
                T::T_keep_end_layers => {
                    let keep = self.parse_maybe_absent_bool(true)?;
                    set_keep_top_bottom(&mut pad, keep);
                }
                T::T_tenting => {
                    let (front, back) = self.parse_front_back_opt_bool(true)?;
                    pad.padstack.front_outer_layers_mut().has_solder_mask = front;
                    pad.padstack.back_outer_layers_mut().has_solder_mask = back;
                }
                T::T_zone_layer_connections => {
                    let cu_layers = {
                        let ctx = BoardCtx::bare(&self.board);
                        &pad.layer_set(&ctx) & Lset::all_cu_mask()
                    };

                    for layer in cu_layers.seq() {
                        pad.zone_layer_overrides
                            .insert(layer, ZoneLayerOverride::ForceNoZoneConnection);
                    }

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        let layer = self.look_up_layer(false);

                        if !is_copper_layer(layer) {
                            return Err(self.lexer.expecting_str("copper layer name"));
                        }

                        pad.zone_layer_overrides
                            .insert(layer, ZoneLayerOverride::ForceFlashed);
                        token = self.lexer.next_tok()?;
                    }
                }
                // Continue to process "(locked)" format which was output during 5.99 development
                T::T_locked => {
                    // Pad locking is now a session preference
                    self.parse_maybe_absent_bool(true)?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    pad.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_front_post_machining => {
                    self.parse_post_machining(pad.padstack.front_post_machining_mut())?;
                }
                T::T_back_post_machining => {
                    self.parse_post_machining(pad.padstack.back_post_machining_mut())?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut pad)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "at, locked, drill, layers, net, die_length, roundrect_rratio, \
                         solder_mask_margin, solder_paste_margin, solder_paste_margin_ratio, uuid, \
                         clearance, tstamp, primitives, remove_unused_layers, keep_end_layers, \
                         pinfunction, pintype, zone_connect, thermal_width, thermal_gap, padstack, \
                         teardrops, front_post_machining, back_post_machining, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if !found_net {
            // Make sure default netclass is correctly assigned to pads that don't define a net.
            self.set_net_code(&mut pad, NetCode(0));
        }

        if let Some(angle) = thermal_br_angle_override {
            pad.set_thermal_spoke_angle(angle);
        } else {
            // This is here because custom pad anchor shape isn't known before reading (options
            if pad.shape(PcbLayerId::FCu) == PadShape::Circle {
                pad.set_thermal_spoke_angle(ANGLE_45);
            } else if pad.shape(PcbLayerId::FCu) == PadShape::Custom
                && pad.anchor_pad_shape(PcbLayerId::FCu) == PadShape::Circle
            {
                if self.required_version <= 20211014 {
                    // 6.0
                    pad.set_thermal_spoke_angle(ANGLE_90);
                } else {
                    pad.set_thermal_spoke_angle(ANGLE_45);
                }
            } else {
                pad.set_thermal_spoke_angle(ANGLE_90);
            }
        }

        if !pad.can_have_number() {
            // At some point it was possible to assign a number to aperture pads so we need to
            // clean those out here.
            pad.set_number("");
        }

        // Zero-sized pads are likely algorithmically unsafe.
        for layer in pad.padstack.unique_layers() {
            let pad_size = pad.size(&BoardCtx::bare(&self.board), layer);

            if pad_size.x <= 0 || pad_size.y <= 0 {
                let min = PCB_IU_SCALE.mm_to_iu(0.001);
                pad.set_size(&env, layer, Vector2I::new(min, min));

                let warning = format!(
                    "Invalid zero-sized pad pinned to {} in\nfile: {}\nline: {}\noffset: {}",
                    "1µm",
                    self.lexer.cur_source(),
                    self.lexer.cur_line_number(),
                    self.lexer.cur_offset()
                );
                self.push_warning(warning);
            }
        }

        Ok(Some(pad))
    }

    /// The `T_net` case of `parsePAD`; call after the `net` keyword.
    ///
    /// Legacy files (pre-10.0) will have a netcode written before the netname. This netcode is
    /// authoritative (though may be mapped by `getNetCode()` to prevent collisions).
    fn parse_pad_net(&mut self, pad: &mut Pad) -> Result<(), IoError> {
        let mut found_netcode = false;
        let mut token = self.lexer.next_tok()?;

        if kicad_sexpr::DsnLexer::<T>::is_number(token) {
            let code = self.get_net_code(self.parse_int());

            // The C++ logs "Invalid net ID" to a trace channel when SetNetCode fails.
            if self.set_net_code(pad, NetCode(code)) {
                found_netcode = true;
            }

            token = self.lexer.next_tok()?;
        }

        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) {
            return Err(self.lexer.expecting_str("net name"));
        }

        if self.board_present {
            let mut net_name = self.lexer.from_utf8();

            // Convert overbar syntax from `~...~` to `~{...}`.  These were left out of the
            // first merge so the version is a bit later.
            if self.required_version < 20210606 {
                net_name = kicad_core::string_utils::convert_to_new_overbar_notation(&net_name);
            }

            if found_netcode {
                // `m_board->FindNet( pad->GetNetCode() )` cannot be null here: SetNetCode
                // succeeded.
                let matches = self
                    .board
                    .find_net(pad.base.net)
                    .is_some_and(|net| net.netname() == net_name);

                if !matches {
                    // The C++ logs "Net name doesn't match ID" to a trace channel.
                    self.set_net_code(pad, NetCode::ORPHANED);
                }
            } else {
                // FindNet( netName ), else new NETINFO_ITEM + Add( INSERT ); then the
                // non-virtual BOARD_CONNECTED_ITEM::SetNet.
                let code = self.board.find_or_add_net(&net_name, None);
                pad.base.net = code;
            }
        }

        self.lexer.need_right()
    }

    /// The `T_backdrill` / `T_tertiary_drill` cases of `parsePAD`: `(size ..)` and
    /// `(layers start end)` into `SecondaryDrill()` (`tertiary == false`) or
    /// `TertiaryDrill()`; a drill without a start layer gets size 0.
    fn parse_pad_extra_drill(&mut self, pad: &mut Pad, tertiary: bool) -> Result<(), IoError> {
        let size_text = if tertiary {
            "tertiary drill size"
        } else {
            "backdrill size"
        };

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_size => {
                    let size = self.parse_board_units_str(size_text)?;
                    extra_drill(&mut pad.padstack, tertiary).size = Vector2I::new(size, size);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    self.lexer.next_tok()?;
                    let start = self.look_up_layer(true);
                    self.lexer.next_tok()?;
                    let end = self.look_up_layer(true);
                    let drill = extra_drill(&mut pad.padstack, tertiary);
                    drill.start = start;
                    drill.end = end;
                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("size or layers")),
            }

            token = self.lexer.next_tok()?;
        }

        let drill = extra_drill(&mut pad.padstack, tertiary);

        if drill.start == PcbLayerId::UndefinedLayer {
            drill.size = Vector2I::new(0, 0);
        }

        Ok(())
    }

    /// The `T_chamfer` loop of `parsePAD` / `parsePadstack`: corner keywords up to and
    /// including the closing `)`; returns the `RECT_CHAMFER_*` bits.
    fn parse_chamfer_list(&mut self, expected: &str) -> Result<i32, IoError> {
        let mut chamfers = 0;

        loop {
            let token = self.lexer.next_tok()?;

            match token {
                T::T_top_left => chamfers |= RECT_CHAMFER_TOP_LEFT,
                T::T_top_right => chamfers |= RECT_CHAMFER_TOP_RIGHT,
                T::T_bottom_left => chamfers |= RECT_CHAMFER_BOTTOM_LEFT,
                T::T_bottom_right => chamfers |= RECT_CHAMFER_BOTTOM_RIGHT,
                T::T_RIGHT => return Ok(chamfers),
                _ => return Err(self.lexer.expecting_str(expected)),
            }
        }
    }

    /// Port of `parsePAD_primitives( PAD* aPad, PCB_LAYER_ID aLayer )`: call with
    /// `CurTok() == T_primitives`; reads up to and including the closing `)`.
    pub(crate) fn parse_pad_primitives(
        &mut self,
        pad: &mut Pad,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_gr_arc
                | T::T_gr_line
                | T::T_gr_circle
                | T::T_gr_rect
                | T::T_gr_poly
                | T::T_gr_curve => {
                    if let Some(shape) = self.parse_pcb_shape(ParentRef::None)? {
                        self.pad_add_primitive(pad, layer, shape);
                    }
                }
                // number box / spoke template
                T::T_gr_bbox | T::T_gr_vector => {
                    if let Some(mut shape) = self.parse_pcb_shape(ParentRef::None)? {
                        shape.set_is_proxy_item(true);
                        self.pad_add_primitive(pad, layer, shape);
                    }
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "gr_line, gr_arc, gr_circle, gr_curve, gr_rect, gr_bbox or gr_poly",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `PAD::AddPrimitive( aLayer, aPrimitive )` (`pcbnew/pad.cpp`) for a pad being
    /// parsed: seed the lib state from the primitive's current values, then re-bake it with
    /// `GetPrimitiveLibScale()` (`rebakePrimitiveToFootprint`: covers load, where the
    /// footprint scale is set before the primitives are parsed), and append it to the layer.
    ///
    /// The C++ `SetParent( this )` cannot be reproduced on a local value (the pad has no
    /// handle until the footprint adds it): the primitive keeps `ParentRef::None`
    /// (`PORT_STATUS.md`, PCBIO-07 known gaps).
    fn pad_add_primitive(&self, pad: &mut Pad, layer: PcbLayerId, mut primitive: PcbShape) {
        let start = primitive.shape.start();
        let end = primitive.shape.end();
        let arc_mid = if primitive.shape.shape() == ShapeT::Arc {
            primitive.shape.arc_mid()
        } else {
            Vector2I::new(0, 0)
        };

        primitive.override_lib_coords(start, end, arc_mid);

        if primitive.shape.shape() == ShapeT::Bezier {
            let (c1, c2) = (primitive.shape.bezier_c1(), primitive.shape.bezier_c2());
            primitive.override_lib_bezier(c1, c2);
        }

        if primitive.shape.shape() == ShapeT::Poly {
            let poly = primitive.shape.poly_shape().clone();
            primitive.override_lib_poly(poly);
        }

        // rebakePrimitiveToFootprint( aPrimitive )
        let ctx = BoardCtx::bare(&self.board);

        if pad.parent_footprint(&ctx).is_some() {
            let (local_sx, local_sy) = pad.primitive_lib_scale(&ctx);

            // The primitive's environment: no parent frame (pad frame), the board's max error.
            let mut prim_env = self.board.child_env(pad.base.item.parent);
            prim_env.parent = None;
            primitive.rebake_with_scale(&prim_env, local_sx, local_sy);
        }

        pad.padstack.add_primitive(primitive, layer);
        pad.set_dirty();
    }

    /// Port of `parsePAD_option( PAD* aPad, PCB_LAYER_ID aLayer )`: parse only the
    /// `(options ...)` inside a pad description; reads up to and including the closing `)`.
    pub(crate) fn parse_pad_option(
        &mut self,
        pad: &mut Pad,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_anchor => {
                    token = self.lexer.next_tok()?;

                    // Custom shaped pads have a "anchor pad", which is the reference for
                    // connection calculations. Because this is an anchor, only the 2 very basic
                    // shapes are managed: circle and rect.
                    match token {
                        T::T_circle => pad.set_anchor_pad_shape(layer, PadShape::Circle),
                        T::T_rect => pad.set_anchor_pad_shape(layer, PadShape::Rectangle),
                        _ => return Err(self.lexer.expecting_str("circle or rect")),
                    }

                    self.lexer.need_right()?;
                }
                T::T_clearance => {
                    token = self.lexer.next_tok()?;

                    // TODO: m_customShapeInZoneMode is not per-layer at the moment
                    if layer == PcbLayerId::FCu {
                        // Custom shaped pads have a clearance area that is the pad shape (like
                        // usual pads) or the convex hull of the pad shape.
                        match token {
                            T::T_outline => {
                                pad.set_custom_shape_in_zone_opt(CustomShapeZoneMode::Outline);
                            }
                            T::T_convexhull => {
                                pad.set_custom_shape_in_zone_opt(CustomShapeZoneMode::Convexhull);
                            }
                            _ => return Err(self.lexer.expecting_str("outline or convexhull")),
                        }
                    }

                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("anchor or clearance")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parsePostMachining( PADSTACK::POST_MACHINING_PROPS& aProps )`:
    /// `(front_post_machining counterbore (size ...) (depth ...) (angle ...))` or
    /// `(back_post_machining countersink ...)`; call after the keyword, reads up to and
    /// including the closing `)`.
    pub(crate) fn parse_post_machining(
        &mut self,
        props: &mut PostMachiningProps,
    ) -> Result<(), IoError> {
        // The mode token (counterbore/countersink) comes first
        let mut token = self.lexer.next_tok()?;

        match token {
            T::T_counterbore => props.mode = Some(PadDrillPostMachiningMode::Counterbore),
            T::T_countersink => props.mode = Some(PadDrillPostMachiningMode::Countersink),
            _ => return Err(self.lexer.expecting_str("counterbore or countersink")),
        }

        // Parse optional properties
        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_size => {
                    props.size = self.parse_board_units_str("post machining size")?;
                    self.lexer.need_right()?;
                }
                T::T_depth => {
                    props.depth = self.parse_board_units_str("post machining depth")?;
                    self.lexer.need_right()?;
                }
                T::T_angle => {
                    props.angle =
                        ki_round(self.parse_double_expected("post machining angle")? * 10.0);
                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("size, depth, or angle")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parsePadstack( PAD* aPad )`: call with `CurTok() == T_padstack`; reads up to
    /// and including the closing `)`.
    pub(crate) fn parse_padstack(&mut self, pad: &mut Pad) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_mode => {
                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_front_inner_back => {
                            pad.padstack.set_mode(PadstackMode::FrontInnerBack);
                        }
                        T::T_custom => pad.padstack.set_mode(PadstackMode::Custom),
                        _ => return Err(self.lexer.expecting_str("front_inner_back or custom")),
                    }

                    self.lexer.need_right()?;
                }
                T::T_layer => self.parse_padstack_layer(pad)?,
                _ => return Err(self.lexer.expecting_str("mode or layer")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_layer` case of `parsePadstack`: `(layer "Inner"|"L" ...)`, reads up to and
    /// including the closing `)`.
    fn parse_padstack_layer(&mut self, pad: &mut Pad) -> Result<(), IoError> {
        self.lexer.next_tok()?;

        let cur_layer = if self.lexer.cur_text() == "Inner" {
            if pad.padstack.mode() != PadstackMode::FrontInnerBack {
                return Err(IoError::new(format!(
                    "Invalid padstack layer in\nfile: {}\nline: {}\noffset: {}.",
                    self.lexer.cur_source(),
                    self.lexer.cur_line_number(),
                    self.lexer.cur_offset()
                )));
            }

            Padstack::INNER_LAYERS
        } else {
            self.look_up_layer(false)
        };

        if !is_copper_layer(cur_layer) {
            return Err(IoError::new(format!(
                "Invalid padstack layer '{}' in file '{}' at line {}, offset {}.",
                self.lexer.cur_text(),
                self.lexer.cur_source(),
                self.lexer.cur_line_number(),
                self.lexer.cur_offset()
            )));
        }

        // Reset layer properties to default that are omitted when default in the formatter
        pad.set_lib_offset(cur_layer, Vector2I::new(0, 0));
        pad.set_delta(cur_layer, Vector2I::new(0, 0));

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_shape => {
                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_circle => pad.set_shape(cur_layer, PadShape::Circle),
                        T::T_rect => pad.set_shape(cur_layer, PadShape::Rectangle),
                        T::T_oval => pad.set_shape(cur_layer, PadShape::Oval),
                        T::T_trapezoid => pad.set_shape(cur_layer, PadShape::Trapezoid),
                        // Note: the shape can be PAD_SHAPE::ROUNDRECT or
                        // PAD_SHAPE::CHAMFERED_RECT (if chamfer parameters are found later in
                        // pad descr.)
                        T::T_roundrect => pad.set_shape(cur_layer, PadShape::Roundrect),
                        T::T_custom => pad.set_shape(cur_layer, PadShape::Custom),
                        _ => {
                            return Err(self.lexer.expecting_str(
                                "circle, rectangle, roundrect, oval, trapezoid or custom",
                            ));
                        }
                    }

                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let sz = Vector2I::new(
                        self.parse_board_units_str("width value")?,
                        self.parse_board_units_str("height value")?,
                    );
                    pad.set_lib_size(cur_layer, sz);
                    self.lexer.need_right()?;
                }
                T::T_offset => {
                    let pt = Vector2I::new(
                        self.parse_board_units_str("drill offset x")?,
                        self.parse_board_units_str("drill offset y")?,
                    );
                    pad.set_lib_offset(cur_layer, pt);
                    self.lexer.need_right()?;
                }
                T::T_rect_delta => {
                    let delta = Vector2I::new(
                        self.parse_board_units_str("rectangle delta width")?,
                        self.parse_board_units_str("rectangle delta height")?,
                    );
                    pad.set_delta(cur_layer, delta);
                    self.lexer.need_right()?;
                }
                T::T_roundrect_rratio => {
                    let ratio = self.parse_double_expected("roundrect radius ratio")?;
                    pad.set_round_rect_radius_ratio(cur_layer, ratio);
                    self.lexer.need_right()?;
                }
                T::T_chamfer_ratio => {
                    let ratio = self.parse_double_expected("chamfer ratio")?;
                    pad.set_chamfer_rect_ratio(cur_layer, ratio);

                    if ratio > 0.0 {
                        pad.set_shape(cur_layer, PadShape::ChamferedRect);
                    }

                    self.lexer.need_right()?;
                }
                T::T_chamfer => {
                    let chamfers = self.parse_chamfer_list(
                        "chamfer_top_left chamfer_top_right chamfer_bottom_left or \
                         chamfer_bottom_right",
                    )?;
                    pad.set_chamfer_positions(cur_layer, chamfers);

                    if chamfers != RECT_NO_CHAMFER {
                        pad.set_shape(cur_layer, PadShape::ChamferedRect);
                    }
                }
                T::T_thermal_bridge_width => {
                    *pad.padstack.thermal_spoke_width_mut(cur_layer) =
                        Some(self.parse_board_units_str("thermal relief spoke width")?);
                    self.lexer.need_right()?;
                }
                T::T_thermal_gap => {
                    *pad.padstack.thermal_gap_mut(cur_layer) =
                        Some(self.parse_board_units_str("thermal relief gap value")?);
                    self.lexer.need_right()?;
                }
                T::T_thermal_bridge_angle => {
                    // `padstack.SetThermalSpokeAngle( angle )`: the default layer argument
                    // (F_Cu), as in the C++.
                    let angle =
                        EdaAngle::from_degrees(self.parse_double_expected("thermal spoke angle")?);
                    pad.padstack.set_thermal_spoke_angle(angle, PcbLayerId::FCu);
                    self.lexer.need_right()?;
                }
                T::T_zone_connect => {
                    *pad.padstack.zone_connection_mut(cur_layer) = zone_connection_enum_cast(
                        self.parse_int_expected("zone connection value")?,
                    );
                    self.lexer.need_right()?;
                }
                T::T_clearance => {
                    *pad.padstack.clearance_mut(cur_layer) =
                        Some(self.parse_board_units_str("local clearance value")?);
                    self.lexer.need_right()?;
                }
                T::T_tenting => {
                    let (front, back) = self.parse_front_back_opt_bool(true)?;
                    pad.padstack.front_outer_layers_mut().has_solder_mask = front;
                    pad.padstack.back_outer_layers_mut().has_solder_mask = back;
                }
                T::T_options => self.parse_pad_option(pad, cur_layer)?,
                T::T_primitives => {
                    // the file lists every primitive of the layer, so none inherited from
                    // another layer stay (`DeletePrimitivesList( curLayer )`)
                    pad.padstack.clear_primitives(cur_layer);
                    pad.set_dirty();
                    self.parse_pad_primitives(pad, cur_layer)?;
                }
                // Not strict-parsing padstack layers yet: the C++ `continue` reads the next
                // token without skipping the unknown list.
                _ => {}
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }
}

/// `pad->Padstack().SecondaryDrill()` or `TertiaryDrill()`.
fn extra_drill(padstack: &mut Padstack, tertiary: bool) -> &mut crate::padstack::DrillProps {
    if tertiary {
        padstack.tertiary_drill_mut()
    } else {
        padstack.secondary_drill_mut()
    }
}
