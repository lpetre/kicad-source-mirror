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
//! PCBIO-03: `format( const PCB_TRACK* )` (segments, arcs, vias, W:3241-3496),
//! `formatTeardropParameters` and `isDefaultTeardropParameters` (W:871-905).

use kicad_core::layer_ids::is_external_copper_layer;
use kicad_core::layer_range::LayerRange;
use kicad_core::string_utils::format_double2_str;
use kicad_core::{Lset, PcbLayerId};
use kicad_sexpr::IoError;
use kicad_sexpr::{format_bool, format_custom_properties, format_opt_bool, format_uuid};

use super::{CTL_OMIT_PAD_NETS, PcbWriter, format_iu, format_iu_pt};
use crate::board_connected_item::netname;
use crate::ids::TrackId;
use crate::padstack::{
    PadDrillPostMachiningMode, Padstack, PadstackMode, PostMachiningProps, UnconnectedLayerMode,
};
use crate::pcb_track::{PcbVia, Track, UNDEFINED_DRILL_DIAMETER, Viatype, ZoneLayerOverride};
use crate::teardrop_parameters::TeardropParameters;
use crate::traits::BoardItem;

/// Port of the static `isDefaultTeardropParameters( const TEARDROP_PARAMETERS& )`: every
/// member equals the value of a default-constructed `TEARDROP_PARAMETERS`.
pub(crate) fn is_default_teardrop_parameters(td_params: &TeardropParameters) -> bool {
    let defaults = TeardropParameters::default();

    td_params.enabled == defaults.enabled
        && td_params.best_length_ratio == defaults.best_length_ratio
        && td_params.td_max_len == defaults.td_max_len
        && td_params.best_width_ratio == defaults.best_width_ratio
        && td_params.td_max_width == defaults.td_max_width
        && td_params.curved_edges == defaults.curved_edges
        && td_params.width_to_size_filter_ratio == defaults.width_to_size_filter_ratio
        && td_params.allow_use_two_tracks == defaults.allow_use_two_tracks
        && td_params.td_on_pads_in_zones == defaults.td_on_pads_in_zones
}

impl PcbWriter<'_> {
    /// Port of `format( const PCB_TRACK* aTrack )`: `(segment ..)`, `(arc ..)` or `(via ..)`.
    ///
    /// # Errors
    ///
    /// "unknown via type %d" for a via of type `NOT_DEFINED`; write errors.
    pub(crate) fn format_track(&mut self, track: TrackId) -> Result<(), IoError> {
        let Some(item) = self.src.track(track) else {
            return Ok(());
        };

        let ctx = self.ctx;

        if let Track::Via(via) = item {
            let via: &PcbVia = via;

            if !crate::board_item::has_board(via, &ctx) {
                // wxCHECK_RET( board != nullptr, wxT( "Via has no parent." ) )
                debug_assert!(false, "Via has no parent.");
                return Ok(());
            }

            let board = ctx.board;

            self.out.print(format_args!("(via"))?;

            let (layer1, layer2) = via.layer_pair();

            match via.via_type() {
                //  Default shape not saved.
                Viatype::Through => {}
                Viatype::Blind => {
                    self.out.print(format_args!(" blind "))?;
                }
                Viatype::Buried => {
                    self.out.print(format_args!(" buried "))?;
                }
                Viatype::Microvia => {
                    self.out.print(format_args!(" micro "))?;
                }
                other => {
                    return Err(IoError::new(format!("unknown via type {}", other as i32)));
                }
            }

            self.out.print(format_args!(
                "(at {}) (size {})",
                format_iu_pt(via.track.start()),
                format_iu(via.width(PcbLayerId::FCu))
            ))?;

            // Old boards were using UNDEFINED_DRILL_DIAMETER value in file for via drill when
            // via drill was the netclass value.
            // recent boards always set the via drill to the actual value, but now we need to
            // always store the drill value, because netclass value is not stored in the board
            // file. Otherwise the drill value of some (old) vias can be unknown
            if via.drill() != UNDEFINED_DRILL_DIAMETER {
                self.out
                    .print(format_args!("(drill {})", format_iu(via.drill())))?;
            } else {
                self.out
                    .print(format_args!("(drill {})", format_iu(via.drill_value(&ctx))))?;
            }

            let padstack: &Padstack = via.padstack();

            for (token, drill) in [
                ("backdrill", padstack.secondary_drill()),
                ("tertiary_drill", padstack.tertiary_drill()),
            ] {
                if drill.size.x > 0 && drill.start != PcbLayerId::UndefinedLayer {
                    let start = self.out.quotew(&Lset::name(drill.start));
                    let end = self.out.quotew(&Lset::name(drill.end));
                    self.out.print(format_args!(
                        "({token} (size {}) (layers {start} {end}))",
                        format_iu(drill.size.x)
                    ))?;
                }
            }

            self.format_post_machining("front_post_machining", padstack.front_post_machining())?;
            self.format_post_machining("back_post_machining", padstack.back_post_machining())?;

            let name1 = self.out.quotew(&Lset::name(layer1));
            let name2 = self.out.quotew(&Lset::name(layer2));
            self.out.print(format_args!("(layers {name1} {name2})"))?;

            match padstack.unconnected_layer_mode() {
                UnconnectedLayerMode::RemoveAll => {
                    format_bool(self.out, "remove_unused_layers", true)?;
                    format_bool(self.out, "keep_end_layers", false)?;
                }
                UnconnectedLayerMode::RemoveExceptStartAndEnd => {
                    format_bool(self.out, "remove_unused_layers", true)?;
                    format_bool(self.out, "keep_end_layers", true)?;
                }
                UnconnectedLayerMode::StartEndOnly => {
                    format_bool(self.out, "start_end_only", true)?;
                }
                UnconnectedLayerMode::KeepAll => {}
            }

            if BoardItem::is_locked(via, &ctx) {
                format_bool(self.out, "locked", true)?;
            }

            if via.is_free() {
                format_bool(self.out, "free", true)?;
            }

            if via.remove_unconnected() {
                self.out.print(format_args!("(zone_layer_connections"))?;

                for layer in board.enabled_layers().cu_stack().iter() {
                    if via.zone_layer_override(*layer) == ZoneLayerOverride::ForceFlashed {
                        let name = self.out.quotew(&Lset::name(*layer));
                        self.out.print(format_args!(" {name}"))?;
                    }
                }

                self.out.print(format_args!(")"))?;
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

            if padstack.drill().is_capped.is_some() {
                format_opt_bool(self.out, "capping", padstack.drill().is_capped)?;
            }

            if padstack.front_outer_layers().has_covering.is_some()
                || padstack.back_outer_layers().has_covering.is_some()
            {
                self.out.print_nest(0, format_args!(" (covering "))?;
                format_opt_bool(
                    self.out,
                    "front",
                    padstack.front_outer_layers().has_covering,
                )?;
                format_opt_bool(self.out, "back", padstack.back_outer_layers().has_covering)?;
                self.out.print_nest(0, format_args!(")"))?;
            }

            if padstack.front_outer_layers().has_plugging.is_some()
                || padstack.back_outer_layers().has_plugging.is_some()
            {
                self.out.print_nest(0, format_args!(" (plugging "))?;
                format_opt_bool(
                    self.out,
                    "front",
                    padstack.front_outer_layers().has_plugging,
                )?;
                format_opt_bool(self.out, "back", padstack.back_outer_layers().has_plugging)?;
                self.out.print_nest(0, format_args!(")"))?;
            }

            if padstack.drill().is_filled.is_some() {
                format_opt_bool(self.out, "filling", padstack.drill().is_filled)?;
            }

            if padstack.mode() != PadstackMode::Normal {
                self.out.print(format_args!("(padstack"))?;

                if padstack.mode() == PadstackMode::FrontInnerBack {
                    self.out.print(format_args!("(mode front_inner_back)"))?;

                    self.out.print(format_args!("(layer \"Inner\""))?;
                    self.out.print(format_args!(
                        "(size {})",
                        format_iu(padstack.size(Padstack::INNER_LAYERS).x)
                    ))?;
                    self.out.print(format_args!(")"))?;
                    self.out.print(format_args!("(layer \"B.Cu\""))?;
                    self.out.print(format_args!(
                        "(size {})",
                        format_iu(padstack.size(PcbLayerId::BCu).x)
                    ))?;
                    self.out.print(format_args!(")"))?;
                } else {
                    self.out.print(format_args!("(mode custom)"))?;

                    // LAYER_RANGE( F_Cu, B_Cu, n ) never fails: both ends are copper layers.
                    let range = LayerRange::new(
                        PcbLayerId::FCu,
                        PcbLayerId::BCu,
                        board.copper_layer_count(),
                    )
                    .map_err(|_| IoError::new("invalid layer range"))?;

                    for layer in range.iter() {
                        if layer == PcbLayerId::FCu {
                            continue;
                        }

                        let name = self.out.quotew(&Lset::name(layer));
                        self.out.print(format_args!("(layer {name}"))?;
                        self.out
                            .print(format_args!("(size {})", format_iu(padstack.size(layer).x)))?;
                        self.out.print(format_args!(")"))?;
                    }
                }

                self.out.print(format_args!(")"))?;
            }

            let td_params = &via.track.base.teardrop_params;

            if !is_default_teardrop_parameters(td_params) {
                self.format_teardrop_parameters(td_params)?;
            }
        } else {
            let t = item.as_track();

            if let Track::Arc(arc) = item {
                self.out.print(format_args!(
                    "(arc (start {}) (mid {}) (end {}) (width {})",
                    format_iu_pt(arc.track.start()),
                    format_iu_pt(arc.mid()),
                    format_iu_pt(arc.track.end()),
                    format_iu(arc.track.width())
                ))?;
            } else {
                self.out.print(format_args!(
                    "(segment (start {}) (end {}) (width {})",
                    format_iu_pt(t.start()),
                    format_iu_pt(t.end()),
                    format_iu(t.width())
                ))?;
            }

            if BoardItem::is_locked(item, &ctx) {
                format_bool(self.out, "locked", true)?;
            }

            let layer_set = BoardItem::layer_set(item, &ctx);
            let layer = BoardItem::layer(item, &ctx);

            if layer_set.count() > 1 {
                self.format_layers(layer_set, false, false)?;
            } else {
                self.format_layer(layer, false)?;
            }

            if t.has_solder_mask() && is_external_copper_layer(layer) {
                if let Some(margin) = t.local_solder_mask_margin() {
                    self.out
                        .print(format_args!("(solder_mask_margin {})", format_iu(margin)))?;
                }
            }
        }

        if (self.ctl & CTL_OMIT_PAD_NETS) == 0 {
            let name = self.out.quotew(netname(item, &ctx));
            self.out.print(format_args!("(net {name})"))?;
        }

        let base = &item.as_track().base.item;
        format_uuid(self.out, &base.uuid.as_string())?;
        format_custom_properties(self.out, &base.custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }

    /// The `formatPostMachining` lambda of `format( const PCB_TRACK* )`:
    /// `(<name> counterbore|countersink [(size ..)] [(depth ..)] [(angle ..)])`, nothing when
    /// not post-machined.
    fn format_post_machining(
        &mut self,
        name: &str,
        props: &PostMachiningProps,
    ) -> Result<(), IoError> {
        let mode = match props.mode {
            None | Some(PadDrillPostMachiningMode::NotPostMachined) => return Ok(()),
            Some(mode) => mode,
        };

        self.out.print(format_args!(
            "({name} {}",
            if mode == PadDrillPostMachiningMode::Counterbore {
                "counterbore"
            } else {
                "countersink"
            }
        ))?;

        if props.size > 0 {
            self.out
                .print(format_args!(" (size {})", format_iu(props.size)))?;
        }

        if props.depth > 0 {
            self.out
                .print(format_args!(" (depth {})", format_iu(props.depth)))?;
        }

        if props.angle > 0 {
            self.out.print(format_args!(
                " (angle {})",
                format_double2_str(f64::from(props.angle) / 10.0)
            ))?;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `formatTeardropParameters( const TEARDROP_PARAMETERS& )`: `(teardrops ..)`.
    pub(crate) fn format_teardrop_parameters(
        &mut self,
        td_params: &TeardropParameters,
    ) -> Result<(), IoError> {
        self.out.print(format_args!(
            "(teardrops (best_length_ratio {}) (max_length {}) (best_width_ratio {}) \
             (max_width {})",
            format_double2_str(td_params.best_length_ratio),
            format_iu(td_params.td_max_len),
            format_double2_str(td_params.best_width_ratio),
            format_iu(td_params.td_max_width)
        ))?;

        format_bool(self.out, "curved_edges", td_params.curved_edges)?;

        self.out.print(format_args!(
            "(filter_ratio {})",
            format_double2_str(td_params.width_to_size_filter_ratio)
        ))?;

        format_bool(self.out, "enabled", td_params.enabled)?;
        format_bool(
            self.out,
            "allow_two_segments",
            td_params.allow_use_two_tracks,
        )?;
        format_bool(
            self.out,
            "prefer_zone_connections",
            !td_params.td_on_pads_in_zones,
        )?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }
}
