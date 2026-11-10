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

//! Port of the zone part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-08): `parseZONE` (P:9610-10480), `parseZoneDefaults` and
//! `parseZoneLayerProperty` (P:3450-3501).
//!
//! The zone is built as a local value (`std::make_unique<ZONE>( aParent )`) and returned to
//! the caller, which adds it (`parseBOARD`) or drops it when it has no corners. Mutators that
//! need the board state (`SetLayer`, `SetLayerSet`, `SetBorderDisplayStyle`) get the
//! environment of a child of `aParent` (`Board::child_env`). Fill results are written into
//! the zone's fill state directly (the zone is not on the board yet).

use std::collections::BTreeMap;

use kicad_core::{EdaDataType, Kiid, PcbLayerId};
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::{EdaAngle, Seg, ShapeLineChain, ShapePolySet};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use crate::ids::{ItemKind, NetCode};
use crate::item::ParentRef;
use crate::teardrop_parameters::TeardropType;
use crate::zone::Zone;
use crate::zone_settings::{
    CornerSmoothing, IslandRemovalMode, PlacementSourceT, RepeatLayoutOptions, ThievingPattern,
    ZoneBorderDisplayStyle, ZoneConnection, ZoneFillMode, ZoneLayerProperties, ZoneSettings,
};

impl PcbParser {
    /// Port of `parseZONE( BOARD_ITEM_CONTAINER* aParent )`: call with `CurTok() == T_zone`;
    /// reads up to and including the closing `)`. Always returns `Some` (the C++ returns
    /// null only on the `wxCHECK` of a wrong current token); `parseBOARD` drops a zone without
    /// corners.
    ///
    /// # Errors
    ///
    /// `PARSE_ERROR`s of the C++ (`Expecting` / `Unexpected` with the same texts).
    pub(crate) fn parse_zone(&mut self, parent: ParentRef) -> Result<Option<Zone>, IoError> {
        if self.lexer.cur_tok() != T::T_zone {
            debug_assert!(
                false,
                "Cannot parse {} as ZONE.",
                self.lexer.token_string(self.lexer.cur_tok())
            );
            return Ok(None);
        }

        let mut hatch_style = ZoneBorderDisplayStyle::NoHatch;

        let mut hatch_pitch = Zone::default_hatch_pitch();
        // the (non-authoratative) zone net name found in a legacy file
        let mut legacy_netname_from_file = String::new();

        // bigger scope since each filled_polygon is concatenated in here
        let mut pts: BTreeMap<PcbLayerId, ShapePolySet> = BTreeMap::new();
        let mut legacy_segs: BTreeMap<PcbLayerId, Vec<Seg>> = BTreeMap::new();
        let mut added_filled_polygons = false;

        // This hasn't been supported since V6 or so, but we only stopped writing out the token
        // in V10.
        let mut is_stroked_fill = self.required_version < 20250210;

        let mut zone = self.new_zone(parent);
        let env = self.board.child_env(parent);

        zone.set_assigned_priority(0);

        // This is the default for board files:
        zone.island_removal_mode = IslandRemovalMode::Always;

        // The ZONE ctor copies the board's default zone settings, but these tokens are
        // omitted from the file when at their defaults. Reset them so appending into a
        // live board (design block, paste) doesn't pick up its session defaults.
        zone.pad_connection = ZoneConnection::Thermal;
        zone.set_fill_mode(ZoneFillMode::Polygons);
        zone.corner_smoothing_type = CornerSmoothing::NoSmoothing;
        zone.set_corner_radius(0);
        zone.hatch_smoothing_level = 0;
        zone.base.item.is_locked = false;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            // legacy locked
            if token == T::T_locked {
                zone.base.item.is_locked = true;
                token = self.lexer.next_tok()?;
            }

            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_net => self.parse_zone_net(&mut zone)?,

                T::T_net_name => {
                    self.lexer.need_symbol_or_number()?;
                    legacy_netname_from_file = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                // keyword for zones that are on only one layer
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    zone.set_layer(&env, layer);
                    self.lexer.need_right()?;
                }

                // keyword for zones that can live on a set of layers
                T::T_layers => {
                    let layers = self.parse_board_item_layers_as_mask()?;
                    zone.set_layer_set(&env, layers);
                }

                T::T_property => {
                    self.parse_zone_layer_property(&mut zone.layer_properties)?;
                }

                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    zone.base.item.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }

                T::T_hatch => {
                    let token = self.lexer.next_tok()?;

                    if token != T::T_none && token != T::T_edge && token != T::T_full {
                        return Err(self.lexer.expecting_str("none, edge, or full"));
                    }

                    hatch_style = match token {
                        T::T_edge => ZoneBorderDisplayStyle::DiagonalEdge,
                        T::T_full => ZoneBorderDisplayStyle::DiagonalFull,
                        _ => ZoneBorderDisplayStyle::NoHatch,
                    };

                    hatch_pitch = self.parse_board_units_str("hatch pitch")?;
                    self.lexer.need_right()?;
                }

                T::T_priority => {
                    // SetAssignedPriority( unsigned ): int -> unsigned conversion.
                    let priority = self.parse_int_expected("zone priority")?;
                    zone.set_assigned_priority(priority as u32);
                    self.lexer.need_right()?;
                }

                T::T_connect_pads => self.parse_zone_connect_pads(&mut zone)?,

                T::T_min_thickness => {
                    let value =
                        self.parse_board_units_tok(T::T_min_thickness, EdaDataType::Distance)?;
                    zone.set_min_thickness(value);
                    self.lexer.need_right()?;
                }

                T::T_filled_areas_thickness => {
                    // A new zone fill strategy was added in v6, so we need to know if we're
                    // parsing a zone that was filled before that. Note that the change was
                    // implemented as a new parameter, so we need to check for the  presence
                    // of filled_areas_thickness instead of just its value.

                    if !self.parse_bool()? {
                        is_stroked_fill = false;
                    }

                    self.lexer.need_right()?;
                }

                T::T_fill => self.parse_zone_fill(&mut zone)?,

                T::T_placement => self.parse_zone_placement(&mut zone)?,

                T::T_keepout => self.parse_zone_keepout(&mut zone)?,

                T::T_polygon => {
                    let mut outline = ShapeLineChain::new();

                    self.lexer.need_left()?;
                    let token = self.lexer.next_tok()?;

                    if token != T::T_pts {
                        return Err(self.lexer.expecting(T::T_pts));
                    }

                    let mut token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        self.parse_outline_points(&mut outline)?;
                        token = self.lexer.next_tok()?;
                    }

                    self.lexer.need_right()?;

                    outline.set_closed(true);

                    // Remark: The first polygon is the main outline.
                    // Others are holes inside the main outline.
                    if outline.point_count() != 0 {
                        zone.add_polygon(&outline);
                    }
                }

                T::T_filled_polygon => {
                    // "(filled_polygon (pts"
                    self.lexer.need_left()?;
                    let mut token = self.lexer.next_tok()?;

                    let filled_layer;

                    if token == T::T_layer {
                        filled_layer = self.parse_board_item_layer()?;
                        self.lexer.need_right()?;
                        token = self.lexer.next_tok()?;

                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;
                    } else {
                        // for legacy, single-layer zones
                        filled_layer = zone.first_layer();
                    }

                    let mut island = false;

                    if token == T::T_island {
                        island = self.parse_maybe_absent_bool(true)?;
                        self.lexer.need_left()?;
                        token = self.lexer.next_tok()?;
                    }

                    if token != T::T_pts {
                        return Err(self.lexer.expecting(T::T_pts));
                    }

                    let poly = pts.entry(filled_layer).or_default();

                    let idx = poly.new_outline();

                    if island {
                        zone.fill.set_is_island(filled_layer, idx);
                    }

                    let mut token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        self.parse_outline_points(poly.outline(idx))?;
                        token = self.lexer.next_tok()?;
                    }

                    self.lexer.need_right()?;

                    added_filled_polygons |= !poly.is_empty();
                }

                T::T_fill_segments => {
                    // Legacy segment fill

                    let mut token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        if token != T::T_pts {
                            return Err(self.lexer.expecting(T::T_pts));
                        }

                        // Legacy zones only had one layer
                        let filled_layer = zone.first_layer();

                        let a = self.parse_xy()?;
                        let b = self.parse_xy()?;

                        legacy_segs
                            .entry(filled_layer)
                            .or_default()
                            .push(Seg::new(a, b));

                        self.lexer.need_right()?;

                        token = self.lexer.next_tok()?;
                    }
                }

                T::T_name => {
                    self.lexer.next_tok()?;
                    zone.zone_name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_attr => self.parse_zone_attr(&mut zone)?,

                T::T_locked => {
                    zone.base.item.is_locked = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_custom_property => self.parse_custom_property_item(&mut zone)?,

                _ => {
                    return Err(self.lexer.expecting_str(
                        "net, layer/layers, tstamp, hatch, priority, connect_pads, \
                         min_thickness, fill, polygon, filled_polygon, fill_segments, attr, \
                         locked, uuid, or name",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if zone.num_corners() > 2 {
            if !self.zone_is_on_copper_layer(&zone) {
                //zone->SetFillMode( ZONE_FILL_MODE::POLYGONS );
                self.set_zone_net_code(&mut zone, NetCode::UNCONNECTED);
            }

            // Set hatch here, after outlines corners are read
            zone.set_border_display_style(&env, hatch_style, hatch_pitch, true);
        }

        if added_filled_polygons {
            if is_stroked_fill && !zone.is_rule_area() {
                if self.show_legacy5_zone_warning {
                    self.push_warning(
                        "Legacy zone fill strategy is not supported anymore.\n\
                         Zone fills will be converted on best-effort basis.",
                    );

                    self.show_legacy5_zone_warning = false;
                }

                if zone.min_thickness() > 0 {
                    for polyset in pts.values_mut() {
                        polyset.inflate_with_linked_holes(
                            zone.min_thickness() / 2,
                            CornerStrategy::RoundAllCorners,
                            ARC_HIGH_DEF / 2,
                        );
                    }
                }
            }

            for (layer, polyset) in pts {
                zone.fill.move_filled_polys_list(layer, polyset);
            }

            zone.calculate_filled_area();
        } else if !legacy_segs.is_empty() {
            // No polygons, just segment fill?
            // Note RFB: This code might be removed if turns out this never existed for sexpr
            // file format or otherwise we should add a test case to the qa folder

            if self.show_legacy_segment_zone_warning {
                self.push_warning(
                    "The legacy segment zone fill mode is no longer supported.\n\
                     Zone fills will be converted on a best-effort basis.",
                );

                self.show_legacy_segment_zone_warning = false;
            }

            for (layer, segments) in &legacy_segs {
                let mut layer_fill = ShapePolySet::new();

                if zone.has_filled_polys_for_layer(*layer) {
                    if let Some(fill) = zone.get_fill(*layer) {
                        layer_fill = fill.clone();
                    }
                }

                for seg in segments {
                    let mut seg_polygon = ShapePolySet::new();

                    transform_oval_to_polygon(
                        &mut seg_polygon,
                        seg.a,
                        seg.b,
                        zone.min_thickness(),
                        ARC_HIGH_DEF,
                        ErrorLoc::Outside,
                        0,
                    );

                    layer_fill.boolean_add(&seg_polygon);
                }

                zone.fill.move_filled_polys_list(*layer, layer_fill);
                zone.calculate_filled_area();
            }
        }

        // Ensure keepout and non copper zones do not have a net
        // (which have no sense for these zones)
        // the netcode 0 is used for these zones
        let zone_has_net = self.zone_is_on_copper_layer(&zone) && !zone.is_rule_area();

        if !zone_has_net {
            self.set_zone_net_code(&mut zone, NetCode::UNCONNECTED);
        }

        // In legacy files, ensure the zone net name is valid, and matches the net code
        if self.board_present
            && !legacy_netname_from_file.is_empty()
            && self.zone_netname(&zone) != legacy_netname_from_file
        {
            // Can happens which old boards, with nonexistent nets ...
            // or after being edited by hand
            // We try to fix the mismatch.
            let existing = self
                .board
                .find_net_by_name(&legacy_netname_from_file)
                .map(|net| net.net_code);

            if let Some(code) = existing {
                // An existing net has the same net name. use it for the zone
                self.set_zone_net_code(&mut zone, code);
            } else {
                // Not existing net: add a new net to keep track of the zone netname
                let newnetcode = self.board.net_count() as i32;
                let code = self
                    .board
                    .find_or_add_net(&legacy_netname_from_file, Some(NetCode(newnetcode)));

                // Store the new code mapping
                self.push_value_into_map(newnetcode, code.0);

                // and update the zone netcode
                self.set_zone_net_code(&mut zone, code);
            }
        }

        if self.board_present && zone.is_teardrop_area() && self.required_version < 20230517 {
            self.board.legacy_teardrops = true;
        }

        // Clear flags used in zone edition:
        zone.fill.need_refill = false;

        // By UUID, since the caller may still discard the zone
        if zone.link_set != Kiid::nil() || zone.repeat_options.anchor != Kiid::nil() {
            self.linked_zones.insert(zone.base.item.uuid);
        }

        Ok(Some(zone))
    }

    /// `std::make_unique<ZONE>( aParent )`: a zone in a footprint starts as a rule area (then
    /// overwritten by `ExportSetting`, as in C++); the board's default zone settings are
    /// exported when the parent reaches a board, `ZONE_SETTINGS()` otherwise.
    fn new_zone(&mut self, parent: ParentRef) -> Zone {
        let in_footprint =
            matches!(parent, ParentRef::Item(id) if id.kind() == ItemKind::Footprint);

        // aParent->GetBoard(): a board-level parent or a footprint created with m_board.
        let has_board = self.board_present && parent != ParentRef::None;

        let settings = if has_board {
            self.board.design_settings().default_zone_settings().clone()
        } else {
            ZoneSettings::default()
        };

        let mut zone =
            Zone::new_with_settings(&mut self.board.kiid_generator, &settings, in_footprint);
        zone.base.item.parent = parent;
        zone
    }

    /// `ZONE::IsOnCopperLayer()` of the zone being parsed.
    fn zone_is_on_copper_layer(&self, zone: &Zone) -> bool {
        let ctx = crate::ctx::BoardCtx::bare(&self.board);
        zone.is_on_copper_layer(&ctx)
    }

    /// `ZONE::GetNetname()` of the zone being parsed (empty for no net).
    fn zone_netname(&self, zone: &Zone) -> String {
        self.board
            .find_net(zone.base.net)
            .map(|net| net.netname().to_string())
            .unwrap_or_default()
    }

    /// The virtual `ZONE::SetNetCode( aNetCode )` (`aNoAssert = false`): copper-thieving zones
    /// are forced to net 0, then `BOARD_CONNECTED_ITEM::SetNetCode`.
    fn set_zone_net_code(&self, zone: &mut Zone, code: NetCode) {
        let code = zone.filter_net_code(code);
        self.set_net_code(zone, code);
    }

    /// Port of `parseNet( BOARD_CONNECTED_ITEM* aItem )` for a zone: the same as
    /// [`PcbParser::parse_net_with`] with the virtual `ZONE::SetNetCode` / `ZONE::SetNet`
    /// overrides (a copper-thieving zone gets net 0 from a legacy net code and no net,
    /// `SetNet( nullptr )`, from a net name).
    fn parse_zone_net(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        let token = self.lexer.next_tok()?;

        if token == T::T_NUMBER {
            let code = 0.max(self.get_net_code(self.parse_int()));
            // The C++ logs "Invalid net ID" to a trace channel when SetNetCode fails.
            self.set_zone_net_code(zone, NetCode(code));
            self.lexer.need_right()?;
            return Ok(());
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

            let code = self.board.find_or_add_net(&net_name, None);

            // ZONE::SetNet: a thieving zone drops the net (m_netinfo = nullptr).
            zone.base.net = if zone.is_copper_thieving() {
                NetCode::ORPHANED
            } else {
                code
            };
        }

        self.lexer.need_right()
    }

    /// The `T_connect_pads` case of `parseZONE`: `[yes|no|thru_hole_only] (clearance mm))`.
    fn parse_zone_connect_pads(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_yes => zone.pad_connection = ZoneConnection::Full,
                T::T_no => zone.pad_connection = ZoneConnection::None,
                T::T_thru_hole_only => zone.pad_connection = ZoneConnection::ThtThermal,
                T::T_clearance => {
                    // SetLocalClearance( std::optional<int> ): value_or( 0 ).
                    zone.zone_clearance = self.parse_board_units_str("zone clearance")?;
                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("yes, no, or clearance")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_fill` case of `parseZONE`: every fill mode and option, up to and including the
    /// closing `)`.
    fn parse_zone_fill(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_yes => zone.fill.set_is_filled(true),

                T::T_mode => {
                    let token = self.lexer.next_tok()?;

                    if token != T::T_segment
                        && token != T::T_hatch
                        && token != T::T_polygon
                        && token != T::T_thieving
                    {
                        return Err(self
                            .lexer
                            .expecting_str("segment, hatch, polygon or thieving"));
                    }

                    match token {
                        T::T_hatch => zone.set_fill_mode(ZoneFillMode::HatchPattern),

                        T::T_thieving => {
                            if self.required_version < 20260513 {
                                return Err(self.lexer.expecting_str(
                                    "segment, hatch or polygon \
                                     (thieving requires file version >= 20260513)",
                                ));
                            }

                            zone.set_fill_mode(ZoneFillMode::CopperThieving);
                        }

                        // T_segment: deprecated, convert to polygons
                        _ => zone.set_fill_mode(ZoneFillMode::Polygons),
                    }

                    self.lexer.need_right()?;
                }

                T::T_hatch_thickness => {
                    zone.hatch_thickness =
                        self.parse_board_units_tok(T::T_hatch_thickness, EdaDataType::Distance)?;
                    self.lexer.need_right()?;
                }

                T::T_hatch_gap => {
                    zone.hatch_gap =
                        self.parse_board_units_tok(T::T_hatch_gap, EdaDataType::Distance)?;
                    self.lexer.need_right()?;
                }

                T::T_hatch_orientation => {
                    let expected = self.lexer.token_text(T::T_hatch_orientation);
                    let orientation = EdaAngle::from_degrees(self.parse_double_expected(expected)?);
                    zone.set_hatch_orientation(orientation);
                    self.lexer.need_right()?;
                }

                T::T_hatch_smoothing_level => {
                    // SetHatchSmoothingLevel( int ) from a double: truncation.
                    let expected = self.lexer.token_text(T::T_hatch_smoothing_level);
                    zone.hatch_smoothing_level =
                        f64_to_i32_x86(self.parse_double_expected(expected)?);
                    self.lexer.need_right()?;
                }

                T::T_hatch_smoothing_value => {
                    let expected = self.lexer.token_text(T::T_hatch_smoothing_value);
                    zone.hatch_smoothing_value = self.parse_double_expected(expected)?;
                    self.lexer.need_right()?;
                }

                T::T_hatch_border_algorithm => {
                    let token = self.lexer.next_tok()?;

                    if token != T::T_hatch_thickness && token != T::T_min_thickness {
                        return Err(self.lexer.expecting_str("hatch_thickness or min_thickness"));
                    }

                    zone.hatch_border_algorithm = if token == T::T_hatch_thickness { 1 } else { 0 };
                    self.lexer.need_right()?;
                }

                T::T_hatch_min_hole_area => {
                    let expected = self.lexer.token_text(T::T_hatch_min_hole_area);
                    zone.hatch_hole_min_area = self.parse_double_expected(expected)?;
                    self.lexer.need_right()?;
                }

                T::T_thieving => self.parse_zone_thieving(zone)?,

                T::T_arc_segments => {
                    let _ = self.parse_int_expected("arc segment count")?;
                    self.lexer.need_right()?;
                }

                T::T_thermal_gap => {
                    let gap =
                        self.parse_board_units_tok(T::T_thermal_gap, EdaDataType::Distance)?;

                    if zone.thermal_relief_gap != gap {
                        zone.fill.need_refill = true;
                    }

                    zone.thermal_relief_gap = gap;
                    self.lexer.need_right()?;
                }

                T::T_thermal_bridge_width => {
                    let width = self
                        .parse_board_units_tok(T::T_thermal_bridge_width, EdaDataType::Distance)?;

                    if zone.thermal_relief_spoke_width != width {
                        zone.fill.need_refill = true;
                    }

                    zone.thermal_relief_spoke_width = width;
                    self.lexer.need_right()?;
                }

                T::T_smoothing => {
                    match self.lexer.next_tok()? {
                        T::T_none => zone.corner_smoothing_type = CornerSmoothing::NoSmoothing,

                        T::T_chamfer => {
                            // smoothing has meaning only for filled zones
                            if !zone.is_rule_area() {
                                zone.corner_smoothing_type = CornerSmoothing::Chamfer;
                            }
                        }

                        T::T_fillet => {
                            // smoothing has meaning only for filled zones
                            if !zone.is_rule_area() {
                                zone.corner_smoothing_type = CornerSmoothing::Fillet;
                            }
                        }

                        _ => return Err(self.lexer.expecting_str("none, chamfer, or fillet")),
                    }

                    self.lexer.need_right()?;
                }

                T::T_radius => {
                    let tmp = self.parse_board_units_str("corner radius")?;

                    // smoothing has meaning only for filled zones
                    if !zone.is_rule_area() {
                        // SetCornerRadius( unsigned int ): int -> unsigned conversion.
                        zone.set_corner_radius(tmp as u32);
                    }

                    self.lexer.need_right()?;
                }

                T::T_island_removal_mode => {
                    let tmp = self.parse_int_expected("island_removal_mode")?;

                    match tmp {
                        0 => zone.island_removal_mode = IslandRemovalMode::Always,
                        1 => zone.island_removal_mode = IslandRemovalMode::Never,
                        2 => zone.island_removal_mode = IslandRemovalMode::Area,
                        _ => {}
                    }

                    self.lexer.need_right()?;
                }

                T::T_island_area_min => {
                    let area =
                        self.parse_board_units_tok(T::T_island_area_min, EdaDataType::Distance)?;
                    // SetMinIslandArea( long long ) of int * double: truncation (always in
                    // range: |area| < 2^31, times 1e6).
                    zone.min_island_area =
                        (area as f64 * kicad_core::PCB_IU_SCALE.iu_per_mm) as i64;
                    self.lexer.need_right()?;
                }

                _ => {
                    return Err(self.lexer.expecting_str(
                        "mode, arc_segments, thermal_gap, thermal_bridge_width, \
                         hatch_thickness, hatch_gap, hatch_orientation, \
                         hatch_smoothing_level, hatch_smoothing_value, \
                         hatch_border_algorithm, hatch_min_hole_area, thieving, \
                         smoothing, radius, island_removal_mode, or island_area_min",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_thieving` case of the zone `fill` list: `(type ..) (size ..) (gap ..) (width ..)
    /// (stagger ..) (orientation ..)`, up to and including the closing `)`.
    fn parse_zone_thieving(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        if self.required_version < 20260513 {
            return Err(self
                .lexer
                .expecting_str("thieving requires file version >= 20260513"));
        }

        let mut thieving = zone.thieving_settings.clone();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_type => {
                    match self.lexer.next_tok()? {
                        T::T_dots => thieving.pattern = ThievingPattern::Dots,
                        T::T_squares => thieving.pattern = ThievingPattern::Squares,
                        T::T_hatch => thieving.pattern = ThievingPattern::Hatch,
                        _ => return Err(self.lexer.expecting_str("dots, squares or hatch")),
                    }

                    self.lexer.need_right()?;
                }

                // Reject non-positive geometry inline.  Zero size would emit
                // zero-area stamps and zero gap would deadlock the filler grid
                // loop.  The zone's existing setting (constructor defaults)
                // stays in place for any malformed field.
                T::T_size => {
                    let val = self.parse_board_units_tok(T::T_size, EdaDataType::Distance)?;

                    if val > 0 {
                        thieving.element_size = val;
                    }

                    self.lexer.need_right()?;
                }

                T::T_gap => {
                    let val = self.parse_board_units_tok(T::T_gap, EdaDataType::Distance)?;

                    if val > 0 {
                        thieving.gap = val;
                    }

                    self.lexer.need_right()?;
                }

                T::T_width => {
                    let val = self.parse_board_units_tok(T::T_width, EdaDataType::Distance)?;

                    if val > 0 {
                        thieving.line_width = val;
                    }

                    self.lexer.need_right()?;
                }

                T::T_stagger => {
                    thieving.stagger = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_orientation => {
                    let expected = self.lexer.token_text(T::T_orientation);
                    let angle = EdaAngle::from_degrees(self.parse_double_expected(expected)?);
                    thieving.orientation = kicad_geom::EdaOrientation::new(angle);
                    self.lexer.need_right()?;
                }

                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("type, size, gap, width, stagger or orientation"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        zone.set_thieving_settings(&thieving);
        Ok(())
    }

    /// The `T_placement` case of `parseZONE`: marks a rule area and reads `(enabled ..)`,
    /// `(sheetname ..)`, `(component_class ..)`, `(group ..)` and `(link ..)`.
    fn parse_zone_placement(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        zone.is_rule_area = true;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_sheetname => {
                    zone.placement_area_source_type = PlacementSourceT::Sheetname;
                    self.lexer.need_symbol()?;
                    zone.placement_area_source = self.lexer.from_utf8();
                }
                T::T_component_class => {
                    zone.placement_area_source_type = PlacementSourceT::ComponentClass;
                    self.lexer.need_symbol()?;
                    zone.placement_area_source = self.lexer.from_utf8();
                }
                T::T_group => {
                    zone.placement_area_source_type = PlacementSourceT::GroupPlacement;
                    self.lexer.need_symbol()?;
                    zone.placement_area_source = self.lexer.from_utf8();
                }
                T::T_enabled => {
                    let token = self.lexer.next_tok()?;

                    if token == T::T_yes {
                        zone.placement_area_enabled = true;
                    } else if token == T::T_no {
                        zone.placement_area_enabled = false;
                    } else {
                        return Err(self.lexer.expecting_str("yes or no"));
                    }
                }
                T::T_link => {
                    let mut opts = RepeatLayoutOptions::default();

                    self.lexer.need_symbol()?;
                    zone.link_set = Kiid::from_string_or_random(
                        self.lexer.cur_str(),
                        &mut self.board.kiid_generator,
                    );

                    let mut link_token = self.lexer.next_tok()?;

                    while link_token != T::T_RIGHT {
                        if link_token == T::T_LEFT {
                            link_token = self.lexer.next_tok()?;
                        }

                        match link_token {
                            T::T_placement => opts.copy_placement = self.parse_bool()?,
                            T::T_routing => opts.copy_routing = self.parse_bool()?,
                            T::T_connected_routing => {
                                opts.connected_routing_only = self.parse_bool()?;
                            }
                            T::T_other_items => opts.copy_other_items = self.parse_bool()?,
                            T::T_group_items => opts.group_items = self.parse_bool()?,
                            T::T_locked_items => opts.include_locked_items = self.parse_bool()?,
                            T::T_anchor => {
                                self.lexer.need_symbol()?;
                                opts.anchor = Kiid::from_string_or_random(
                                    self.lexer.cur_str(),
                                    &mut self.board.kiid_generator,
                                );
                            }
                            _ => {
                                return Err(self.lexer.expecting_str(
                                    "placement, routing, connected_routing, other_items, \
                                     group_items, locked_items or anchor",
                                ));
                            }
                        }

                        self.lexer.need_right()?;
                        link_token = self.lexer.next_tok()?;
                    }

                    zone.repeat_options = opts;

                    // The loop already consumed the closing paren
                    token = self.lexer.next_tok()?;
                    continue;
                }
                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("enabled, sheetname, component_class, group or link"));
                }
            }

            self.lexer.need_right()?;
            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_keepout` case of `parseZONE`: "keepout" now means rule area, but the file token
    /// stays the same.
    fn parse_zone_keepout(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        zone.is_rule_area = true;

        // Initialize these two because their tokens won't appear in older files:
        zone.do_not_allow_pads = false;
        zone.do_not_allow_footprints = false;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_tracks | T::T_vias | T::T_copperpour | T::T_pads | T::T_footprints => {
                    let value = self.lexer.next_tok()?;

                    if value != T::T_allowed && value != T::T_not_allowed {
                        return Err(self.lexer.expecting_str("allowed or not_allowed"));
                    }

                    let not_allowed = value == T::T_not_allowed;

                    match token {
                        T::T_tracks => zone.do_not_allow_tracks = not_allowed,
                        T::T_vias => zone.do_not_allow_vias = not_allowed,
                        T::T_copperpour => zone.do_not_allow_zone_fills = not_allowed,
                        T::T_pads => zone.do_not_allow_pads = not_allowed,
                        _ => zone.do_not_allow_footprints = not_allowed,
                    }
                }

                _ => return Err(self.lexer.expecting_str("tracks, vias or copperpour")),
            }

            self.lexer.need_right()?;
            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_attr` case of `parseZONE`: `(teardrop (type padvia|track_end))`.
    fn parse_zone_attr(&mut self, zone: &mut Zone) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_teardrop => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token == T::T_LEFT {
                            token = self.lexer.next_tok()?;
                        }

                        match token {
                            T::T_type => {
                                let token = self.lexer.next_tok()?;

                                if token == T::T_padvia {
                                    zone.teardrop_type = TeardropType::TdViapad;
                                } else if token == T::T_track_end {
                                    zone.teardrop_type = TeardropType::TdTrackend;
                                } else {
                                    return Err(self.lexer.expecting_str("padvia or track_end"));
                                }

                                self.lexer.need_right()?;
                            }

                            _ => return Err(self.lexer.expecting_str("type")),
                        }

                        token = self.lexer.next_tok()?;
                    }
                }

                _ => return Err(self.lexer.expecting_str("teardrop")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseZoneDefaults( std::map<PCB_LAYER_ID, ZONE_LAYER_PROPERTIES>& )`:
    /// `{(property ..)}` up to and including the closing `)`.
    pub(crate) fn parse_zone_defaults(
        &mut self,
        properties: &mut BTreeMap<PcbLayerId, ZoneLayerProperties>,
    ) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_property => self.parse_zone_layer_property(properties)?,
                _ => {
                    let text = self.lexer.cur_text().to_string();
                    return Err(self.lexer.unexpected_str(&text));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseZoneLayerProperty( std::map<PCB_LAYER_ID, ZONE_LAYER_PROPERTIES>& )`:
    /// `(layer "L") (hatch_position (xy x y)))`; inserted with `emplace` (an existing entry
    /// of that layer is kept).
    pub(crate) fn parse_zone_layer_property(
        &mut self,
        properties: &mut BTreeMap<PcbLayerId, ZoneLayerProperties>,
    ) -> Result<(), IoError> {
        let mut layer = PcbLayerId::UndefinedLayer;
        let mut layer_properties = ZoneLayerProperties::default();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_layer => {
                    layer = self.parse_board_item_layer()?;
                    self.lexer.need_right()?;
                }
                T::T_hatch_position => {
                    layer_properties.hatching_offset = Some(self.parse_xy()?);
                    self.lexer.need_right()?;
                }
                _ => {
                    let text = self.lexer.cur_text().to_string();
                    return Err(self.lexer.unexpected_str(&text));
                }
            }

            token = self.lexer.next_tok()?;
        }

        properties.entry(layer).or_insert(layer_properties);
        Ok(())
    }
}
