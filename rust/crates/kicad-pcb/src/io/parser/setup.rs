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

//! Port of the setup part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-04): `parseBoardStackup`, `parseSetup` (incl. the default stackup built
//! when the file has none), `parseDrillSymbolProfile`, `parseDefaults`,
//! `parseDefaultTextDims`. The `(pcbplotparams ...)` block is read by the port of
//! `PCB_PLOT_PARAMS_PARSER` in `setup_plot_params.rs`, the drill symbol profile data by
//! `setup_drill_profile.rs`; `parseZoneDefaults` belongs to PCBIO-08 (`zone.rs`).

use std::collections::BTreeSet;
use std::sync::Arc;

use kicad_core::{Color4D, EdaDataType, Lset, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::keywords::pcbplotparams_t;

use super::PcbParser;
use crate::board_design_settings::{
    DiffPairDimension, LAYER_CLASS_COPPER, LAYER_CLASS_COURTYARD, LAYER_CLASS_EDGES,
    LAYER_CLASS_FAB, LAYER_CLASS_OTHERS, LAYER_CLASS_SILK, ViaDimension,
};
use crate::board_stackup::{
    BoardStackupItem, BoardStackupItemType, BsEdgeConnectorConstraints, DEFAULT_SOLDERMASK_OPACITY,
    DielectricModel,
};
use crate::pcb_dimension::{DimPrecision, DimUnitsMode};
use crate::pcb_plot_params::PcbPlotParams;

#[path = "setup_drill_profile.rs"]
mod drill_profile;
#[path = "setup_plot_params.rs"]
mod plot_params;
#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

use drill_profile::{
    DrillGroupKey, DrillMarkMode, DrillSymbolAssignment, DrillSymbolProfile,
    drill_group_key_from_token, drill_mark_mode_from_token, drill_mark_policy_from_token,
};

/// A 32-copper-layer board has at most 69 stackup items (32 copper + 31 dielectric + 6
/// mask/paste/silk). Anything far beyond that indicates a corrupted file.
const MAX_STACKUP_ITEMS: usize = 128;

impl PcbParser {
    /// Port of `parseBoardStackup`: call with `CurTok() == T_stackup`.
    pub(crate) fn parse_board_stackup(&mut self) -> Result<(), IoError> {
        let mut dielectric_idx = 1; // the index of dielectric layers

        // Remove existing stack or we end up just appending to the existing stackup
        self.board.design_settings_mut().stackup.remove_all();

        // Board appends in older versions could duplicate the whole stackup.  Stop adding items
        // once a board layer id repeats; still parse the duplicates to keep the token stream
        // consistent.
        // https://gitlab.com/kicad/code/kicad/-/issues/24133
        let mut seen_brd_layers: BTreeSet<PcbLayerId> = BTreeSet::new();
        let mut duplicated_stackup = false;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if self.lexer.cur_tok() != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            if token != T::T_layer {
                let stackup = &mut self.board.design_settings_mut().stackup;

                match token {
                    T::T_copper_finish => {
                        self.lexer.need_symbol()?;
                        stackup.finish_type = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    }

                    T::T_edge_plating => {
                        token = self.lexer.next_tok()?;
                        stackup.edge_plating = token == T::T_yes;
                        self.lexer.need_right()?;
                    }

                    T::T_dielectric_constraints => {
                        token = self.lexer.next_tok()?;
                        stackup.has_dielectric_constrains = token == T::T_yes;
                        self.lexer.need_right()?;
                    }

                    T::T_edge_connector => {
                        token = self.lexer.next_tok()?;
                        stackup.edge_connector_constraints = BsEdgeConnectorConstraints::None;

                        if token == T::T_yes {
                            stackup.edge_connector_constraints = BsEdgeConnectorConstraints::InUse;
                        } else if token == T::T_bevelled {
                            stackup.edge_connector_constraints =
                                BsEdgeConnectorConstraints::Bevelled;
                        }

                        self.lexer.need_right()?;
                    }

                    T::T_castellated_pads => {
                        // Legacy compatibility. just skip it
                        self.lexer.next_tok()?;
                        self.lexer.need_right()?;
                    }

                    _ => {
                        // Currently, skip this item if not defined, because the stackup def
                        // is a moving target
                        //Expecting( "copper_finish, edge_plating, dielectric_constrains,
                        // edge_connector, castellated_pads" );
                        self.skip_current()?;
                    }
                }

                token = self.lexer.next_tok()?;
                continue;
            }

            self.lexer.need_symbol()?;
            let name = self.lexer.from_utf8();

            // Match the canonical names that we write, not GetLayerID() because the user-name
            // matching could end up being the same as a canonical name and corrupt the stack.
            let mut layer_id = PcbLayerId::UndefinedLayer;

            // LSET::AllLayersMask() in ascending layer order
            for candidate in 0..PCB_LAYER_ID_COUNT {
                let candidate = PcbLayerId(candidate);

                if Lset::name(candidate) == name {
                    layer_id = candidate;
                    break;
                }
            }

            // Init the type
            let mut type_ = BoardStackupItemType::Undefined;

            if layer_id == PcbLayerId::FSilkS || layer_id == PcbLayerId::BSilkS {
                type_ = BoardStackupItemType::Silkscreen;
            } else if layer_id == PcbLayerId::FMask || layer_id == PcbLayerId::BMask {
                type_ = BoardStackupItemType::Soldermask;
            } else if layer_id == PcbLayerId::FPaste || layer_id == PcbLayerId::BPaste {
                type_ = BoardStackupItemType::Solderpaste;
            } else if layer_id == PcbLayerId::UndefinedLayer {
                type_ = BoardStackupItemType::Dielectric;
            } else if (layer_id.0 & 1) == 0 {
                type_ = BoardStackupItemType::Copper;
            }

            if layer_id != PcbLayerId::UndefinedLayer && !seen_brd_layers.insert(layer_id) {
                duplicated_stackup = true;
            }

            // The C++ adds the item to the stackup before reading its parameters and keeps
            // writing through the pointer; adding it after reading is equivalent (a failure
            // aborts the whole load).
            let mut item;
            let keep;

            if type_ != BoardStackupItemType::Undefined {
                // Parse the item so tokens are consumed correctly, but don't keep it beyond
                // MAX_STACKUP_ITEMS.
                // https://gitlab.com/kicad/code/kicad/-/issues/23625
                item = BoardStackupItem::new(type_);
                item.set_brd_layer_id(layer_id);

                if type_ == BoardStackupItemType::Dielectric {
                    item.set_dielectric_layer_id(dielectric_idx);
                    dielectric_idx += 1;
                }

                keep = !duplicated_stackup
                    && self.board.design_settings().stackup.count() < MAX_STACKUP_ITEMS;
            } else {
                return Err(self.lexer.expecting_str("layer_name"));
            }

            let mut has_next_sublayer = true;
            let mut sublayer_idx: usize = 0; // the index of dielectric sub layers
            // sublayer 0 is always existing (main sublayer)

            while has_next_sublayer {
                has_next_sublayer = false;

                token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    if token == T::T_addsublayer {
                        has_next_sublayer = true;
                        break;
                    }

                    if token == T::T_LEFT {
                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_type => {
                                self.lexer.need_symbol()?;
                                item.set_type_name(&self.lexer.from_utf8());
                                self.lexer.need_right()?;
                            }

                            T::T_thickness => {
                                let thickness = self
                                    .parse_board_units_tok(T::T_thickness, EdaDataType::Distance)?;
                                item.set_thickness(thickness, sublayer_idx);
                                token = self.lexer.next_tok()?;

                                if token == T::T_LEFT {
                                    token = self.lexer.next_tok()?;
                                    continue;
                                }

                                if token == T::T_locked {
                                    // Dielectric thickness can be locked (for impedance
                                    // controlled layers)
                                    if type_ == BoardStackupItemType::Dielectric {
                                        item.set_thickness_locked(true, sublayer_idx);
                                    }

                                    self.lexer.need_right()?;
                                }
                            }

                            T::T_material => {
                                self.lexer.need_symbol()?;
                                item.set_material(self.lexer.from_utf8(), sublayer_idx);
                                self.lexer.need_right()?;
                            }

                            T::T_epsilon_r => {
                                self.lexer.next_tok()?;
                                item.set_epsilon_r(self.parse_double()?, sublayer_idx);
                                self.lexer.need_right()?;
                            }

                            T::T_loss_tangent => {
                                self.lexer.next_tok()?;
                                item.set_loss_tangent(self.parse_double()?, sublayer_idx);
                                self.lexer.need_right()?;
                            }

                            T::T_spec_frequency => {
                                self.lexer.next_tok()?;
                                item.set_spec_freq(self.parse_double()?, sublayer_idx);
                                self.lexer.need_right()?;
                            }

                            T::T_dielectric_model => {
                                token = self.lexer.next_tok()?;

                                match token {
                                    T::T_constant => item.set_dielectric_model(
                                        DielectricModel::Constant,
                                        sublayer_idx,
                                    ),
                                    T::T_djordjevic_sarkar => item.set_dielectric_model(
                                        DielectricModel::DjordjevicSarkar,
                                        sublayer_idx,
                                    ),
                                    _ => {
                                        return Err(self
                                            .lexer
                                            .expecting_str("constant or djordjevic_sarkar"));
                                    }
                                }

                                self.lexer.need_right()?;
                            }

                            T::T_color => {
                                self.lexer.need_symbol()?;
                                let mut color_name = self.lexer.from_utf8();

                                // Older versions didn't store opacity with custom colors
                                if color_name.starts_with('#') && self.required_version < 20210824 {
                                    let mut color = Color4D::from_string(&color_name);

                                    if item.type_() == BoardStackupItemType::Soldermask {
                                        color = color.with_alpha(DEFAULT_SOLDERMASK_OPACITY);
                                    } else {
                                        color = color.with_alpha(1.0);
                                    }

                                    let wx_color = color.to_colour();

                                    // Open-code wxColour::GetAsString() because 3.0 doesn't
                                    // handle rgba
                                    color_name = format!(
                                        "#{:02X}{:02X}{:02X}{:02X}",
                                        wx_color.red, wx_color.green, wx_color.blue, wx_color.alpha
                                    );
                                }

                                item.set_color(color_name, sublayer_idx);
                                self.lexer.need_right()?;
                            }

                            _ => {
                                // Currently, skip this item if not defined, because the stackup
                                // def is a moving target
                                //Expecting( "type, thickness, material, epsilon_r, loss_tangent, color" );
                                self.skip_current()?;
                            }
                        }
                    }

                    token = self.lexer.next_tok()?;
                }

                if has_next_sublayer {
                    // Prepare reading the next sublayer description
                    sublayer_idx += 1;
                    item.add_dielectric_prms(sublayer_idx);
                }
            }

            if keep {
                self.board.design_settings_mut().stackup.add(item);
            }

            token = self.lexer.next_tok()?;
        }

        if token != T::T_RIGHT {
            return Err(self.lexer.expecting_str(")"));
        }

        // Success:
        self.board.design_settings_mut().has_stackup = true;
        Ok(())
    }

    /// Port of `parseDrillSymbolProfile`: call with `CurTok() == T_drill_symbol_profile`.
    /// Unknown entries are skipped. The profile is read into (and stored back to) the
    /// model's canonical form, see `setup_drill_profile.rs`.
    pub(crate) fn parse_drill_symbol_profile(&mut self) -> Result<(), IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_drill_symbol_profile,
            "Cannot parse {} as a drill symbol profile.",
            self.lexer.cur_text()
        );

        let mut profile = DrillSymbolProfile::from_model(
            self.board.design_settings().drill_symbol_profile.as_ref(),
        );

        for key in [
            DrillGroupKey::SizeGrp,
            DrillGroupKey::Slot,
            DrillGroupKey::Plating,
            DrillGroupKey::Span,
            DrillGroupKey::Operation,
            DrillGroupKey::HoleFunction,
            DrillGroupKey::Protection,
            DrillGroupKey::PostMachining,
        ] {
            profile.set_grouped_by(key, false);
        }

        let result = self.parse_drill_symbol_profile_body(&mut profile);

        // The C++ edits the board's profile in place, also when it throws.
        self.board.design_settings_mut().drill_symbol_profile = profile.to_model();
        result
    }

    /// The token loop of `parseDrillSymbolProfile`.
    fn parse_drill_symbol_profile_body(
        &mut self,
        profile: &mut DrillSymbolProfile,
    ) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_name => {
                    self.lexer.need_symbol_or_number()?;
                    profile.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_group_by => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if let Some(key) = drill_group_key_from_token(&self.lexer.from_utf8()) {
                            profile.set_grouped_by(key, true);
                        }

                        token = self.lexer.next_tok()?;
                    }
                }

                T::T_default_marks => {
                    self.lexer.need_symbol_or_number()?;

                    if let Some(policy) = drill_mark_policy_from_token(&self.lexer.from_utf8()) {
                        profile.mark_policy = policy;
                    }

                    self.lexer.need_right()?;
                }

                T::T_size => {
                    profile.symbol_size = self.parse_board_units_str("drill symbol size")?;
                    self.lexer.need_right()?;
                }

                T::T_width => {
                    profile.symbol_width = self.parse_board_units_str("drill symbol line width")?;
                    self.lexer.need_right()?;
                }

                T::T_freeze_assignments => {
                    profile.freeze_assignments = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_assignment => {
                    let mut key = String::new();
                    let mut assignment = DrillSymbolAssignment::default();

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_key => {
                                self.lexer.need_symbol_or_number()?;
                                key = self.lexer.cur_str().to_string();
                                self.lexer.need_right()?;
                            }

                            T::T_mark => {
                                self.lexer.need_symbol_or_number()?;

                                if let Some(mode) =
                                    drill_mark_mode_from_token(&self.lexer.from_utf8())
                                {
                                    assignment.mark_mode = mode;
                                }

                                if assignment.mark_mode == DrillMarkMode::Shape {
                                    assignment.shape_index =
                                        self.parse_int_expected("drill symbol shape")?;
                                } else if assignment.mark_mode == DrillMarkMode::Letter {
                                    self.lexer.need_symbol_or_number()?;
                                    assignment.letter = self.lexer.from_utf8();
                                }

                                self.lexer.need_right()?;
                            }

                            T::T_descr => {
                                self.lexer.need_symbol_or_number()?;
                                assignment.description = self.lexer.from_utf8();
                                self.lexer.need_right()?;
                            }

                            _ => self.skip_current()?,
                        }

                        token = self.lexer.next_tok()?;
                    }

                    if !key.is_empty() {
                        profile.set_assignment(&key, assignment);
                    }
                }

                _ => self.skip_current()?,
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseSetup`: call with `CurTok() == T_setup`.
    pub(crate) fn parse_setup(&mut self) -> Result<(), IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_setup,
            "Cannot parse {} as setup.",
            self.lexer.cur_text()
        );

        // Missing soldermask min width value means that the user has set the value to 0 and
        // not the default value (0.25mm)
        self.board.design_settings_mut().solder_mask_min_width = 0;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_drill_symbol_profile => self.parse_drill_symbol_profile()?,

                T::T_stackup => {
                    if self.preserve_destination_stackup {
                        self.skip_current()?;
                    } else {
                        self.parse_board_stackup()?;
                    }
                }

                T::T_last_trace_width => {
                    // not used now
                    /* lastTraceWidth =*/
                    self.units_tok(T::T_last_trace_width)?;
                    self.lexer.need_right()?;
                }

                T::T_user_trace_width => {
                    let append = self.append_to_existing;

                    // Make room for the netclass value
                    if self.board.design_settings().track_width_list.is_empty() {
                        self.board.design_settings_mut().track_width_list.push(0);
                    }

                    let track_width = self.units_tok(T::T_user_trace_width)?;
                    let bds = self.board.design_settings_mut();

                    if !append || !bds.track_width_list.contains(&track_width) {
                        bds.track_width_list.push(track_width);
                    }

                    self.board.legacy_design_settings_loaded = true;
                    self.lexer.need_right()?;
                }

                T::T_trace_clearance => {
                    let v = self.units_tok(T::T_trace_clearance)?;
                    self.default_netclass_mut().clearance = Some(v);
                    self.legacy_right()?;
                }

                T::T_zone_clearance => {
                    let v = self.units_tok(T::T_zone_clearance)?;
                    self.board
                        .design_settings_mut()
                        .default_zone_settings
                        .zone_clearance = v;
                    self.legacy_right()?;
                }

                T::T_zone_45_only => {
                    // legacy setting
                    /* zoneSettings.m_Zone_45_Only = */
                    self.parse_bool()?;
                    self.legacy_right()?;
                }

                T::T_clearance_min => {
                    self.board.design_settings_mut().min_clearance =
                        self.units_tok(T::T_clearance_min)?;
                    self.legacy_right()?;
                }

                T::T_trace_min => {
                    self.board.design_settings_mut().track_min_width =
                        self.units_tok(T::T_trace_min)?;
                    self.legacy_right()?;
                }

                T::T_via_size => {
                    let v = self.units_tok(T::T_via_size)?;
                    self.default_netclass_mut().via_dia = Some(v);
                    self.legacy_right()?;
                }

                T::T_via_drill => {
                    let v = self.units_tok(T::T_via_drill)?;
                    self.default_netclass_mut().via_drill = Some(v);
                    self.legacy_right()?;
                }

                T::T_via_min_annulus => {
                    self.board.design_settings_mut().vias_min_annular_width =
                        self.units_tok(T::T_via_min_annulus)?;
                    self.legacy_right()?;
                }

                T::T_via_min_size => {
                    self.board.design_settings_mut().vias_min_size =
                        self.units_tok(T::T_via_min_size)?;
                    self.legacy_right()?;
                }

                T::T_through_hole_min => {
                    self.board.design_settings_mut().min_through_drill =
                        self.units_tok(T::T_through_hole_min)?;
                    self.legacy_right()?;
                }

                // Legacy token for T_through_hole_min
                T::T_via_min_drill => {
                    self.board.design_settings_mut().min_through_drill =
                        self.units_tok(T::T_via_min_drill)?;
                    self.legacy_right()?;
                }

                T::T_hole_to_hole_min => {
                    self.board.design_settings_mut().hole_to_hole_min =
                        self.units_tok(T::T_hole_to_hole_min)?;
                    self.legacy_right()?;
                }

                T::T_user_via => {
                    let via_size = self.parse_board_units_str("user via size")?;
                    let via_drill = self.parse_board_units_str("user via drill")?;
                    let via = ViaDimension {
                        diameter: via_size,
                        drill: via_drill,
                    };
                    let append = self.append_to_existing;
                    let bds = self.board.design_settings_mut();

                    // Make room for the netclass value
                    if bds.vias_dimensions_list.is_empty() {
                        bds.vias_dimensions_list.push(ViaDimension {
                            diameter: 0,
                            drill: 0,
                        });
                    }

                    if !append || !bds.vias_dimensions_list.contains(&via) {
                        bds.vias_dimensions_list.push(via);
                    }

                    self.legacy_right()?;
                }

                T::T_uvia_size => {
                    let v = self.units_tok(T::T_uvia_size)?;
                    self.default_netclass_mut().uvia_dia = Some(v);
                    self.legacy_right()?;
                }

                T::T_uvia_drill => {
                    let v = self.units_tok(T::T_uvia_drill)?;
                    self.default_netclass_mut().uvia_drill = Some(v);
                    self.legacy_right()?;
                }

                T::T_uvias_allowed | T::T_blind_buried_vias_allowed => {
                    self.parse_bool()?;
                    self.legacy_right()?;
                }

                T::T_uvia_min_size => {
                    self.board.design_settings_mut().micro_vias_min_size =
                        self.units_tok(T::T_uvia_min_size)?;
                    self.legacy_right()?;
                }

                T::T_uvia_min_drill => {
                    self.board.design_settings_mut().micro_vias_min_drill =
                        self.units_tok(T::T_uvia_min_drill)?;
                    self.legacy_right()?;
                }

                T::T_user_diff_pair => {
                    let width = self.parse_board_units_str("user diff-pair width")?;
                    let gap = self.parse_board_units_str("user diff-pair gap")?;
                    let via_gap = self.parse_board_units_str("user diff-pair via gap")?;
                    let diff_pair = DiffPairDimension {
                        width,
                        gap,
                        via_gap,
                    };
                    let append = self.append_to_existing;
                    let bds = self.board.design_settings_mut();

                    if !append || !bds.diff_pair_dimensions_list.contains(&diff_pair) {
                        bds.diff_pair_dimensions_list.push(diff_pair);
                    }

                    self.legacy_right()?;
                }

                T::T_segment_width => {
                    // note: legacy (pre-6.0) token
                    self.board.design_settings_mut().line_thickness[LAYER_CLASS_COPPER] =
                        self.units_tok(T::T_segment_width)?;
                    self.legacy_right()?;
                }

                T::T_edge_width => {
                    // note: legacy (pre-6.0) token
                    self.board.design_settings_mut().line_thickness[LAYER_CLASS_EDGES] =
                        self.units_tok(T::T_edge_width)?;
                    self.legacy_right()?;
                }

                T::T_mod_edge_width => {
                    // note: legacy (pre-6.0) token
                    self.board.design_settings_mut().line_thickness[LAYER_CLASS_SILK] =
                        self.units_tok(T::T_mod_edge_width)?;
                    self.legacy_right()?;
                }

                T::T_pcb_text_width => {
                    // note: legacy (pre-6.0) token
                    self.board.design_settings_mut().text_thickness[LAYER_CLASS_COPPER] =
                        self.units_tok(T::T_pcb_text_width)?;
                    self.legacy_right()?;
                }

                T::T_mod_text_width => {
                    // note: legacy (pre-6.0) token
                    self.board.design_settings_mut().text_thickness[LAYER_CLASS_SILK] =
                        self.units_tok(T::T_mod_text_width)?;
                    self.legacy_right()?;
                }

                T::T_pcb_text_size => {
                    // note: legacy (pre-6.0) token
                    let x = self.parse_board_units_str("pcb text width")?;
                    self.board.design_settings_mut().text_size[LAYER_CLASS_COPPER].x = x;
                    let y = self.parse_board_units_str("pcb text height")?;
                    self.board.design_settings_mut().text_size[LAYER_CLASS_COPPER].y = y;
                    self.legacy_right()?;
                }

                T::T_mod_text_size => {
                    // note: legacy (pre-6.0) token
                    let x = self.parse_board_units_str("footprint text width")?;
                    self.board.design_settings_mut().text_size[LAYER_CLASS_SILK].x = x;
                    let y = self.parse_board_units_str("footprint text height")?;
                    self.board.design_settings_mut().text_size[LAYER_CLASS_SILK].y = y;
                    self.legacy_right()?;
                }

                T::T_defaults => {
                    self.parse_defaults()?;
                    self.board.legacy_design_settings_loaded = true;
                }

                T::T_pad_size => {
                    // master pad size no longer read
                    self.parse_board_units_str("master pad width")?; // x
                    self.parse_board_units_str("master pad height")?; // y
                    self.legacy_right()?;
                }

                T::T_pad_drill => {
                    // master pad drill size no longer read
                    self.units_tok(T::T_pad_drill)?; // drill size
                    self.legacy_right()?;
                }

                T::T_pad_to_mask_clearance => {
                    self.board.design_settings_mut().solder_mask_expansion =
                        self.units_tok(T::T_pad_to_mask_clearance)?;
                    self.lexer.need_right()?;
                }

                T::T_solder_mask_min_width => {
                    self.board.design_settings_mut().solder_mask_min_width =
                        self.units_tok(T::T_solder_mask_min_width)?;
                    self.lexer.need_right()?;
                }

                T::T_pad_to_paste_clearance => {
                    self.board.design_settings_mut().solder_paste_margin =
                        self.units_tok(T::T_pad_to_paste_clearance)?;
                    self.lexer.need_right()?;
                }

                T::T_pad_to_paste_clearance_ratio => {
                    self.board.design_settings_mut().solder_paste_margin_ratio = self
                        .lexer
                        .parse_double_tok(T::T_pad_to_paste_clearance_ratio)?;
                    self.lexer.need_right()?;
                }

                T::T_allow_soldermask_bridges_in_footprints => {
                    self.board
                        .design_settings_mut()
                        .allow_soldermask_bridges_in_fps = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_tenting => {
                    let (front, back) = self.parse_front_back_opt_bool(true)?;
                    let bds = self.board.design_settings_mut();
                    bds.tent_vias_front = front.unwrap_or(false);
                    bds.tent_vias_back = back.unwrap_or(false);
                }

                T::T_covering => {
                    let (front, back) = self.parse_front_back_opt_bool(false)?;
                    let bds = self.board.design_settings_mut();
                    bds.cover_vias_front = front.unwrap_or(false);
                    bds.cover_vias_back = back.unwrap_or(false);
                }

                T::T_plugging => {
                    let (front, back) = self.parse_front_back_opt_bool(false)?;
                    let bds = self.board.design_settings_mut();
                    bds.plug_vias_front = front.unwrap_or(false);
                    bds.plug_vias_back = back.unwrap_or(false);
                }

                T::T_capping => {
                    self.board.design_settings_mut().cap_vias = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_filling => {
                    self.board.design_settings_mut().fill_vias = self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_aux_axis_origin => {
                    let x = self.parse_board_units_str("auxiliary origin X")?;
                    let y = self.parse_board_units_str("auxiliary origin Y")?;
                    self.board
                        .design_settings_mut()
                        .set_aux_origin(kicad_geom::Vector2I::new(x, y));

                    // Aux origin still stored in board for the moment
                    //m_board->m_LegacyDesignSettingsLoaded = true;
                    self.lexer.need_right()?;
                }

                T::T_grid_origin => {
                    let x = self.parse_board_units_str("grid origin X")?;
                    let y = self.parse_board_units_str("grid origin Y")?;
                    self.board
                        .design_settings_mut()
                        .set_grid_origin(kicad_geom::Vector2I::new(x, y));
                    // Grid origin still stored in board for the moment
                    //m_board->m_LegacyDesignSettingsLoaded = true;
                    self.lexer.need_right()?;
                }

                // Stored in board prior to 6.0
                T::T_visible_elements => {
                    // m_LegacyVisibleItems (GAL_SET::DefaultVisible() with the bits of
                    // `parseHex() | MIN_VISIBILITY_MASK`) is GUI state that is never written
                    // back and has no Rust counterpart: only the token is consumed.
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }

                T::T_max_error => {
                    self.board.design_settings_mut().max_error = self.units_tok(T::T_max_error)?;
                    self.legacy_right()?;
                }

                T::T_filled_areas_thickness => {
                    // Ignore this value, it is not used anymore
                    self.parse_bool()?;
                    self.lexer.need_right()?;
                }

                T::T_pcbplotparams => {
                    let mut plot_params = PcbPlotParams::default();
                    let board_file_version = self.required_version;

                    // parser must share the same current line as our current PCB parser
                    // synchronize it.
                    self.lexer
                        .with_sub_lexer::<pcbplotparams_t::T, _, _>(|sub| {
                            plot_params::PcbPlotParamsParser {
                                lexer: sub,
                                board_file_version,
                            }
                            .parse(&mut plot_params)
                        })?;

                    let legacy_via_on_mask = plot_params.legacy_plot_via_on_mask_layer();
                    self.board.plot_options = plot_params;

                    if let Some(via_on_mask) = legacy_via_on_mask {
                        let tent = !via_on_mask;
                        let bds = self.board.design_settings_mut();
                        bds.tent_vias_front = tent;
                        bds.tent_vias_back = tent;
                    }
                }

                T::T_zone_defaults => {
                    let mut properties =
                        std::mem::take(&mut self.board.design_settings_mut().zone_layer_properties);
                    let result = self.parse_zone_defaults(&mut properties);
                    self.board.design_settings_mut().zone_layer_properties = properties;
                    result?;
                }

                _ => {
                    let text = self.lexer.cur_text().to_string();
                    return Err(self.lexer.unexpected_str(&text));
                }
            }

            token = self.lexer.next_tok()?;
        }

        // Set up a default stackup in case the file doesn't define one, and now we know
        // the enabled layers
        if !self.preserve_destination_stackup && !self.board.design_settings().has_stackup {
            // BuildDefaultStackupList reads the settings that own the stackup (aliasing in
            // C++): pass a copy of the settings.
            let settings = self.board.design_settings().clone();
            let copper_count = self.board.copper_layer_count();
            let stackup = &mut self.board.design_settings_mut().stackup;
            stackup.remove_all();
            stackup.build_default_stackup_list(Some(&settings), copper_count);
        }

        Ok(())
    }

    /// Port of `parseDefaults( BOARD_DESIGN_SETTINGS& )` on the board's design settings.
    pub(crate) fn parse_defaults(&mut self) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_edge_clearance => {
                    self.board.design_settings_mut().copper_edge_clearance =
                        self.units_tok(T::T_edge_clearance)?;
                    self.board.legacy_copper_edge_clearance_loaded = true;
                    self.lexer.need_right()?;
                }

                T::T_copper_line_width => self.default_line_width(token, LAYER_CLASS_COPPER)?,
                T::T_copper_text_dims => self.parse_default_text_dims(LAYER_CLASS_COPPER)?,
                T::T_courtyard_line_width => {
                    self.default_line_width(token, LAYER_CLASS_COURTYARD)?
                }
                T::T_edge_cuts_line_width => self.default_line_width(token, LAYER_CLASS_EDGES)?,
                T::T_silk_line_width => self.default_line_width(token, LAYER_CLASS_SILK)?,
                T::T_silk_text_dims => self.parse_default_text_dims(LAYER_CLASS_SILK)?,
                T::T_fab_layers_line_width => self.default_line_width(token, LAYER_CLASS_FAB)?,
                T::T_fab_layers_text_dims => self.parse_default_text_dims(LAYER_CLASS_FAB)?,
                T::T_other_layers_line_width => {
                    self.default_line_width(token, LAYER_CLASS_OTHERS)?
                }
                T::T_other_layers_text_dims => self.parse_default_text_dims(LAYER_CLASS_OTHERS)?,

                T::T_dimension_units => {
                    let v = self.parse_int_expected("dimension units")?;
                    self.board.design_settings_mut().dimension_units_mode =
                        dim_units_mode_from_int(v);
                    self.lexer.need_right()?;
                }

                T::T_dimension_precision => {
                    let v = self.parse_int_expected("dimension precision")?;
                    self.board.design_settings_mut().dimension_precision =
                        dim_precision_from_int(v);
                    self.lexer.need_right()?;
                }

                _ => {
                    let text = self.lexer.cur_text().to_string();
                    return Err(self.lexer.unexpected_str(&text));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseDefaultTextDims( BOARD_DESIGN_SETTINGS&, int aLayer )` (`aLayer` = layer
    /// class).
    pub(crate) fn parse_default_text_dims(&mut self, layer: usize) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_size => {
                    let x = self.parse_board_units_str("default text size X")?;
                    self.board.design_settings_mut().text_size[layer].x = x;
                    let y = self.parse_board_units_str("default text size Y")?;
                    self.board.design_settings_mut().text_size[layer].y = y;
                    self.lexer.need_right()?;
                }

                T::T_thickness => {
                    self.board.design_settings_mut().text_thickness[layer] =
                        self.parse_board_units_str("default text width")?;
                    self.lexer.need_right()?;
                }

                T::T_italic => self.board.design_settings_mut().text_italic[layer] = true,

                T::T_keep_upright => self.board.design_settings_mut().text_upright[layer] = true,

                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("size, thickness, italic or keep_upright"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    // ---- private helpers ----------------------------------------------------------------

    /// `parseBoardUnits( aToken )` (distance).
    fn units_tok(&mut self, token: T) -> Result<i32, IoError> {
        self.parse_board_units_tok(token, EdaDataType::Distance)
    }

    /// `m_board->m_LegacyDesignSettingsLoaded = true; NeedRIGHT();` of the legacy setup
    /// tokens (their presence stops the project file from overriding the design settings).
    fn legacy_right(&mut self) -> Result<(), IoError> {
        self.board.legacy_design_settings_loaded = true;
        self.lexer.need_right()
    }

    /// `aSettings.m_LineThickness[ aClass ] = parseBoardUnits( token ); NeedRIGHT();` of
    /// `parseDefaults`.
    fn default_line_width(&mut self, token: T, class: usize) -> Result<(), IoError> {
        self.board.design_settings_mut().line_thickness[class] = self.units_tok(token)?;
        self.lexer.need_right()
    }

    /// `bds.m_NetSettings->GetDefaultNetclass()`, edited in place through the `shared_ptr`.
    fn default_netclass_mut(&mut self) -> &mut crate::netclass::Netclass {
        Arc::make_mut(
            &mut self
                .board
                .design_settings_mut()
                .net_settings
                .default_netclass,
        )
    }
}

/// `static_cast<DIM_UNITS_MODE>( parseInt( .. ) )`. Values outside the enum (undefined in the
/// C++ enum class, never written by KiCad) map to the last enumerator.
fn dim_units_mode_from_int(v: i32) -> DimUnitsMode {
    match v {
        0 => DimUnitsMode::Inch,
        1 => DimUnitsMode::Mils,
        2 => DimUnitsMode::Mm,
        _ => DimUnitsMode::Automatic,
    }
}

/// `static_cast<DIM_PRECISION>( parseInt( .. ) )`. Values outside the enum map to the last
/// enumerator (see [`dim_units_mode_from_int`]).
fn dim_precision_from_int(v: i32) -> DimPrecision {
    match v {
        0 => DimPrecision::X,
        1 => DimPrecision::X_X,
        2 => DimPrecision::X_XX,
        3 => DimPrecision::X_XXX,
        4 => DimPrecision::X_XXXX,
        5 => DimPrecision::X_XXXXX,
        6 => DimPrecision::V_VV,
        7 => DimPrecision::V_VVV,
        8 => DimPrecision::V_VVVV,
        _ => DimPrecision::V_VVVVV,
    }
}
