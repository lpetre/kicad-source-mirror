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

//! Port of the track part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-03): `parseTEARDROP_PARAMETERS` (P:707-772), `parseARC` (P:8951-9062),
//! `parsePCB_TRACK` (P:9065-9159), `parsePCB_VIA` (P:9162-9455), `parseViastack`
//! (P:9524-9607).
//!
//! Items are built as local values with their parent set (`std::make_unique<PCB_xxx>(
//! m_board )`) and returned to `parseBOARD`, which adds them. Mutators that need the board
//! state (`SetLayer`, `SetLayerSet`, `SetViaType`, `SetLayerPair`) get the environment of a
//! board-level child (`Board::child_env`).

use kicad_core::layer_ids::is_copper_layer;
use kicad_core::{EdaDataType, Lset, PcbLayerId};
use kicad_geom::{Vector2I, is_straight_arc};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use crate::ctx::{BoardCtx, MutEnv};
use crate::item::ParentRef;
use crate::padstack::{Padstack, PadstackMode, UnconnectedLayerMode};
use crate::pcb_track::{PcbArc, PcbTrack, PcbVia, Track, Viatype, ZoneLayerOverride};
use crate::teardrop_parameters::TeardropParameters;
use crate::traits::{BoardItem, BoardItemMut};

impl PcbParser {
    /// The parent `m_board` gives to a new board-level item (null while parsing a footprint
    /// file).
    fn board_parent(&self) -> ParentRef {
        if self.board_present {
            ParentRef::Board
        } else {
            ParentRef::None
        }
    }

    /// The mutator environment of a board-level item being parsed.
    fn board_child_env(&self) -> MutEnv {
        self.board.child_env(self.board_parent())
    }

    /// Port of `parseTEARDROP_PARAMETERS( TEARDROP_PARAMETERS* tdParams )`: call with
    /// `CurTok() == T_teardrops`; reads up to and including the closing `)`.
    pub(crate) fn parse_teardrop_parameters(
        &mut self,
        td_params: &mut TeardropParameters,
    ) -> Result<(), IoError> {
        td_params.enabled = false;
        td_params.allow_use_two_tracks = false;
        td_params.td_on_pads_in_zones = true;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_enabled => {
                    td_params.enabled = self.parse_maybe_absent_bool(true)?;
                }
                T::T_allow_two_segments => {
                    td_params.allow_use_two_tracks = self.parse_maybe_absent_bool(true)?;
                }
                T::T_prefer_zone_connections => {
                    td_params.td_on_pads_in_zones = !self.parse_maybe_absent_bool(false)?;
                }
                T::T_best_length_ratio => {
                    td_params.best_length_ratio =
                        self.parse_double_expected("teardrop best length ratio")?;
                    self.lexer.need_right()?;
                }
                T::T_max_length => {
                    td_params.td_max_len = self.parse_board_units_str("teardrop max length")?;
                    self.lexer.need_right()?;
                }
                T::T_best_width_ratio => {
                    td_params.best_width_ratio =
                        self.parse_double_expected("teardrop best width ratio")?;
                    self.lexer.need_right()?;
                }
                T::T_max_width => {
                    td_params.td_max_width = self.parse_board_units_str("teardrop max width")?;
                    self.lexer.need_right()?;
                }
                // Legacy token
                T::T_curve_points => {
                    td_params.curved_edges =
                        self.parse_int_expected("teardrop curve points count")? > 0;
                    self.lexer.need_right()?;
                }
                T::T_curved_edges => {
                    td_params.curved_edges = self.parse_maybe_absent_bool(true)?;
                }
                T::T_filter_ratio => {
                    td_params.width_to_size_filter_ratio =
                        self.parse_double_expected("teardrop filter ratio")?;
                    self.lexer.need_right()?;
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "enabled, allow_two_segments, prefer_zone_connections, \
                         best_length_ratio, max_length, best_width_ratio, max_width, \
                         curve_points or filter_ratio",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseARC`: a track arc (`Track::Arc`); `None` on a non-copper layer (C++
    /// null). Collinear points have no arc centre, so they load as the straight track they
    /// describe (`Track::Trace`, same UUID).
    pub(crate) fn parse_arc(&mut self) -> Result<Option<Track>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_arc,
            "Cannot parse {} as ARC.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_arc {
            return Ok(None);
        }

        let mut pt = Vector2I::default();
        let mut arc = PcbArc::new(&mut self.board.kiid_generator);
        arc.track.base.item.parent = self.board_parent();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            // Legacy locked
            if token == T::T_locked {
                arc.track.base.item.is_locked = true;
                token = self.lexer.next_tok()?;
            }

            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_start => {
                    pt.x = self.parse_board_units_str("start x")?;
                    pt.y = self.parse_board_units_str("start y")?;
                    arc.track.set_start(pt);
                    self.lexer.need_right()?;
                }
                T::T_mid => {
                    pt.x = self.parse_board_units_str("mid x")?;
                    pt.y = self.parse_board_units_str("mid y")?;
                    arc.set_mid(pt);
                    self.lexer.need_right()?;
                }
                T::T_end => {
                    pt.x = self.parse_board_units_str("end x")?;
                    pt.y = self.parse_board_units_str("end y")?;
                    arc.track.set_end(pt);
                    self.lexer.need_right()?;
                }
                T::T_width => {
                    arc.track.set_width(self.parse_board_units_str("width")?);
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    let env = self.board_child_env();
                    BoardItemMut::set_layer(&mut arc, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    let layers = self.parse_layers_for_cu_item_with_soldermask()?;
                    let env = self.board_child_env();
                    arc.set_layer_set(&env, layers);
                }
                T::T_solder_mask_margin => {
                    let margin = self.parse_board_units_str("local solder mask margin value")?;
                    arc.track.set_local_solder_mask_margin(Some(margin));
                    self.lexer.need_right()?;
                }
                T::T_net => self.parse_net(&mut arc)?,
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    arc.track.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                // We continue to parse the status field but it is no longer written
                T::T_status => {
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }
                T::T_locked => {
                    arc.track.base.item.is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut arc)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "start, mid, end, width, layer, solder_mask_margin, net, tstamp, uuid \
                         or status",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if !is_copper_layer(arc.track.base.item.layer) {
            // No point in asserting; these usually come from hand-edited boards
            return Ok(None);
        }

        // Collinear points have no arc center, so load them as the straight track they describe
        if is_straight_arc(arc.track.start(), arc.mid(), arc.track.end()) {
            // Base assignment copies every track property except the item type and UUID; the
            // UUID is then set to the arc's (`SetUuidDirect( arc->m_Uuid )`), so the result is
            // the arc's PCB_TRACK part.
            return Ok(Some(Track::Trace(arc.track)));
        }

        Ok(Some(Track::Arc(arc)))
    }

    /// Port of `parsePCB_TRACK`: a segment (`Track::Trace`); `None` on a non-copper layer (C++
    /// null).
    pub(crate) fn parse_pcb_track(&mut self) -> Result<Option<Track>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_segment,
            "Cannot parse {} as PCB_TRACK.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_segment {
            return Ok(None);
        }

        let mut pt = Vector2I::default();
        let mut track = PcbTrack::new(&mut self.board.kiid_generator);
        track.base.item.parent = self.board_parent();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            // Legacy locked flag
            if token == T::T_locked {
                track.base.item.is_locked = true;
                token = self.lexer.next_tok()?;
            }

            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_start => {
                    pt.x = self.parse_board_units_str("start x")?;
                    pt.y = self.parse_board_units_str("start y")?;
                    track.set_start(pt);
                    self.lexer.need_right()?;
                }
                T::T_end => {
                    pt.x = self.parse_board_units_str("end x")?;
                    pt.y = self.parse_board_units_str("end y")?;
                    track.set_end(pt);
                    self.lexer.need_right()?;
                }
                T::T_width => {
                    track.set_width(self.parse_board_units_str("width")?);
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    let env = self.board_child_env();
                    BoardItemMut::set_layer(&mut track, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    let layers = self.parse_layers_for_cu_item_with_soldermask()?;
                    let env = self.board_child_env();
                    track.set_layer_set(&env, layers);
                }
                T::T_solder_mask_margin => {
                    let margin = self.parse_board_units_str("local solder mask margin value")?;
                    track.set_local_solder_mask_margin(Some(margin));
                    self.lexer.need_right()?;
                }
                T::T_net => self.parse_net(&mut track)?,
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    track.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                // We continue to parse the status field but it is no longer written
                T::T_status => {
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }
                T::T_locked => {
                    track.base.item.is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut track)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "start, end, width, layer, solder_mask_margin, net, tstamp, uuid or \
                         locked",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if !is_copper_layer(track.base.item.layer) {
            // No point in asserting; these usually come from hand-edited boards
            return Ok(None);
        }

        Ok(Some(Track::Trace(track)))
    }

    /// Port of `parsePCB_VIA`: a via (`Track::Via`).
    pub(crate) fn parse_pcb_via(&mut self) -> Result<Option<Track>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_via,
            "Cannot parse {} as PCB_VIA.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_via {
            return Ok(None);
        }

        let mut pt = Vector2I::default();
        let mut via = PcbVia::new(&mut self.board.kiid_generator);
        via.track.base.item.parent = self.board_parent();

        // NB: all pre-padstack tokens are stored into the F_Cu layer.  For PADSTACK::MODE::NORMAL
        // this will be all there is.  For complex padstacks, the other values will get loaded
        // from the T_padstack record.

        // File format default is no-token == no-feature.
        via.padstack_mut()
            .set_unconnected_layer_mode(UnconnectedLayerMode::KeepAll);

        // Versions before 10.0 had no protection features other than tenting, so those features
        // must be interpreted as OFF in legacy boards, not as unspecified (aka: inherit from board
        // stackup)
        if self.required_version < 20250228 {
            let padstack = via.padstack_mut();
            padstack.front_outer_layers_mut().has_covering = Some(false);
            padstack.back_outer_layers_mut().has_covering = Some(false);
            padstack.front_outer_layers_mut().has_plugging = Some(false);
            padstack.back_outer_layers_mut().has_plugging = Some(false);
            padstack.drill_mut().is_filled = Some(false);
            padstack.drill_mut().is_capped = Some(false);
        }

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            // Legacy locked
            if token == T::T_locked {
                via.track.base.item.is_locked = true;
                token = self.lexer.next_tok()?;
            }

            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_blind => {
                    let env = self.board_child_env();
                    via.set_via_type(&env, Viatype::Blind);
                }
                T::T_buried => {
                    let env = self.board_child_env();
                    via.set_via_type(&env, Viatype::Buried);
                }
                T::T_micro => {
                    let env = self.board_child_env();
                    via.set_via_type(&env, Viatype::Microvia);
                }
                T::T_at => {
                    pt.x = self.parse_board_units_str("start x")?;
                    pt.y = self.parse_board_units_str("start y")?;
                    via.track.set_start(pt);
                    via.track.set_end(pt);
                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let width = self.parse_board_units_str("via width")?;
                    via.set_width(PcbLayerId::FCu, width);
                    self.lexer.need_right()?;
                }
                T::T_drill => {
                    via.set_drill(self.parse_board_units_str("drill diameter")?);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    self.lexer.next_tok()?;
                    let layer1 = self.look_up_layer(false);
                    self.lexer.next_tok()?;
                    let layer2 = self.look_up_layer(false);
                    let env = self.board_child_env();
                    via.set_layer_pair(&env, layer1, layer2);

                    if layer1 == PcbLayerId::UndefinedLayer || layer2 == PcbLayerId::UndefinedLayer
                    {
                        return Err(self.lexer.expecting_str("layer name"));
                    }

                    self.lexer.need_right()?;
                }
                T::T_net => self.parse_net(&mut via)?,
                T::T_remove_unused_layers => {
                    if self.parse_maybe_absent_bool(true)? {
                        via.set_remove_unconnected(true);
                    }
                }
                T::T_keep_end_layers => {
                    if self.parse_maybe_absent_bool(true)? {
                        via.set_keep_start_end(true);
                    }
                }
                T::T_start_end_only => {
                    if self.parse_maybe_absent_bool(true)? {
                        via.padstack_mut()
                            .set_unconnected_layer_mode(UnconnectedLayerMode::StartEndOnly);
                    }
                }
                T::T_zone_layer_connections => {
                    // Ensure only copper layers are stored int ZoneLayerOverride array
                    let cu_layers = {
                        let ctx = BoardCtx::bare(&self.board);
                        &BoardItem::layer_set(&via, &ctx) & Lset::all_cu_mask()
                    };

                    for layer in cu_layers.seq() {
                        via.set_zone_layer_override(
                            layer,
                            ZoneLayerOverride::ForceNoZoneConnection,
                        );
                    }

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        let layer = self.look_up_layer(false);

                        if !is_copper_layer(layer) {
                            return Err(self.lexer.expecting_str("copper layer name"));
                        }

                        via.set_zone_layer_override(layer, ZoneLayerOverride::ForceFlashed);
                        token = self.lexer.next_tok()?;
                    }
                }
                T::T_padstack => self.parse_viastack(&mut via)?,
                T::T_teardrops => {
                    let mut td_params = via.track.base.teardrop_params.clone();
                    self.parse_teardrop_parameters(&mut td_params)?;
                    via.track.base.teardrop_params = td_params;
                }
                T::T_tenting => {
                    let (front, back) = self.parse_front_back_opt_bool(true)?;
                    via.padstack_mut().front_outer_layers_mut().has_solder_mask = front;
                    via.padstack_mut().back_outer_layers_mut().has_solder_mask = back;
                }
                T::T_covering => {
                    let (front, back) = self.parse_front_back_opt_bool(false)?;
                    via.padstack_mut().front_outer_layers_mut().has_covering = front;
                    via.padstack_mut().back_outer_layers_mut().has_covering = back;
                }
                T::T_plugging => {
                    let (front, back) = self.parse_front_back_opt_bool(false)?;
                    via.padstack_mut().front_outer_layers_mut().has_plugging = front;
                    via.padstack_mut().back_outer_layers_mut().has_plugging = back;
                }
                T::T_filling => {
                    via.padstack_mut().drill_mut().is_filled = self.parse_opt_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_capping => {
                    via.padstack_mut().drill_mut().is_capped = self.parse_opt_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    via.track.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                // We continue to parse the status field but it is no longer written
                T::T_status => {
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }
                T::T_locked => {
                    via.track.base.item.is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_free => {
                    let free = self.parse_maybe_absent_bool(true)?;
                    via.set_is_free(free);
                }
                T::T_backdrill => {
                    // Parse: (backdrill (size ...) (layers start end))
                    self.parse_via_extra_drill(&mut via, false)?;
                }
                T::T_tertiary_drill => {
                    // Parse: (tertiary_drill (size ...) (layers start end))
                    self.parse_via_extra_drill(&mut via, true)?;
                }
                T::T_front_post_machining => {
                    let mut props = via.padstack().front_post_machining().clone();
                    self.parse_post_machining(&mut props)?;
                    *via.padstack_mut().front_post_machining_mut() = props;
                }
                T::T_back_post_machining => {
                    let mut props = via.padstack().back_post_machining().clone();
                    self.parse_post_machining(&mut props)?;
                    *via.padstack_mut().back_post_machining_mut() = props;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut via)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "blind, micro, at, size, drill, layers, net, free, tstamp, uuid, status, \
                         teardrops, backdrill, tertiary_drill, front_post_machining, or \
                         back_post_machining",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(Track::Via(Box::new(via))))
    }

    /// The `T_backdrill` / `T_tertiary_drill` cases of `parsePCB_VIA`: `(size ..)` and
    /// `(layers start end)` into `SecondaryDrill()` (`tertiary == false`) or
    /// `TertiaryDrill()`; a drill without a start layer gets size 0.
    fn parse_via_extra_drill(&mut self, via: &mut PcbVia, tertiary: bool) -> Result<(), IoError> {
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
                    let size = self.parse_board_units_expected(size_text, EdaDataType::Distance)?;
                    drill_of(via.padstack_mut(), tertiary).size = Vector2I::new(size, size);
                    self.lexer.need_right()?;
                }
                T::T_layers => {
                    self.lexer.next_tok()?;
                    let start = self.look_up_layer(true);
                    self.lexer.next_tok()?;
                    let end = self.look_up_layer(true);
                    let drill = drill_of(via.padstack_mut(), tertiary);
                    drill.start = start;
                    drill.end = end;
                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("size or layers")),
            }

            token = self.lexer.next_tok()?;
        }

        let drill = drill_of(via.padstack_mut(), tertiary);

        if drill.start == PcbLayerId::UndefinedLayer {
            drill.size = Vector2I::new(0, 0);
        }

        Ok(())
    }

    /// Port of `parseViastack( PCB_VIA* aVia )`: call with `CurTok() == T_padstack`.
    pub(crate) fn parse_viastack(&mut self, via: &mut PcbVia) -> Result<(), IoError> {
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
                            via.padstack_mut().set_mode(PadstackMode::FrontInnerBack);
                        }
                        T::T_custom => via.padstack_mut().set_mode(PadstackMode::Custom),
                        _ => return Err(self.lexer.expecting_str("front_inner_back or custom")),
                    }

                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    self.lexer.next_tok()?;

                    let cur_layer = if self.lexer.cur_text() == "Inner" {
                        if via.padstack().mode() != PadstackMode::FrontInnerBack {
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

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_size => {
                                let diameter = self.parse_board_units_str("via width")?;
                                via.padstack_mut()
                                    .set_size(Vector2I::new(diameter, diameter), cur_layer);
                                self.lexer.need_right()?;
                            }
                            // Currently only supporting custom via diameter per layer, not
                            // other properties
                            _ => return Err(self.lexer.expecting_str("size")),
                        }

                        token = self.lexer.next_tok()?;
                    }
                }
                _ => return Err(self.lexer.expecting_str("mode or layer")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }
}

/// `via->Padstack().SecondaryDrill()` or `TertiaryDrill()`.
fn drill_of(padstack: &mut Padstack, tertiary: bool) -> &mut crate::padstack::DrillProps {
    if tertiary {
        padstack.tertiary_drill_mut()
    } else {
        padstack.secondary_drill_mut()
    }
}
