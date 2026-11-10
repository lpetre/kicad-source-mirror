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
//! PCBIO-08: `format( const ZONE* )` and `format( const ZONE_LAYER_PROPERTIES&, int,
//! PCB_LAYER_ID )` (W:3499-3790).

use kicad_core::string_utils::format_double2_str;
use kicad_core::{Kiid, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_sexpr::IoError;
use kicad_sexpr::{format_bool, format_custom_properties, format_uuid};

use super::{CTL_OMIT_PAD_NETS, PcbWriter, format_iu, format_iu_pt};
use crate::board_connected_item::netname;
use crate::ids::ZoneId;
use crate::teardrop_parameters::TeardropType;
use crate::zone_settings::{
    CornerSmoothing, IslandRemovalMode, PlacementSourceT, ThievingPattern, ZoneBorderDisplayStyle,
    ZoneConnection, ZoneFillMode, ZoneLayerProperties,
};

impl PcbWriter<'_> {
    /// Port of `format( const ZONE* aZone )`: `(zone ..)`.
    ///
    /// # Errors
    ///
    /// Write errors; "map::at" for a layer of the zone without a fill entry (the C++
    /// `std::map::at` throws).
    pub(crate) fn format_zone(&mut self, zone: ZoneId) -> Result<(), IoError> {
        let Some(zone) = self.src.zone(zone) else {
            return Ok(());
        };

        let ctx = self.ctx;

        self.out.print(format_args!("(zone"))?;

        if (self.ctl & CTL_OMIT_PAD_NETS) == 0
            && zone.is_on_copper_layer(&ctx)
            && !zone.is_rule_area()
            && zone.base.net.0 > 0
        {
            let name = self.out.quotew(netname(zone, &ctx));
            self.out.print(format_args!("(net {name})"))?;
        }

        if zone.base.item.is_locked {
            format_bool(self.out, "locked", true)?;
        }

        // If a zone exists on multiple layers, format accordingly
        let mut layers = zone.layer_set(&ctx);

        if crate::board_item::has_board(zone, &ctx) {
            layers &= &ctx.board.enabled_layers();
        }

        // Always enumerate every layer for a zone on a copper layer
        if layers.count() > 1 {
            self.format_layers(layers, zone.is_on_copper_layer(&ctx), true)?;
        } else {
            self.format_layer(zone.first_layer(), false)?;
        }

        if !zone.is_teardrop_area() {
            format_uuid(self.out, &zone.base.item.uuid.as_string())?;
        }

        if !zone.zone_name().is_empty() && !zone.is_teardrop_area() {
            let name = self.out.quotew(zone.zone_name());
            self.out.print(format_args!("(name {name})"))?;
        }

        // Save the outline aux info
        let hatch = match zone.border_style {
            ZoneBorderDisplayStyle::DiagonalEdge => "edge",
            ZoneBorderDisplayStyle::DiagonalFull => "full",
            _ => "none",
        };

        self.out.print(format_args!(
            "(hatch {} {})",
            hatch,
            format_iu(zone.border_hatch_pitch)
        ))?;

        if zone.assigned_priority() > 0 {
            // "%d" of an unsigned int.
            self.out.print(format_args!(
                "(priority {})",
                zone.assigned_priority() as i32
            ))?;
        }

        // Add teardrop keywords in file: (attr (teardrop (type xxx))) where xxx is the teardrop
        // type
        if zone.is_teardrop_area() {
            self.out.print(format_args!(
                "(attr (teardrop (type {})))",
                if zone.teardrop_type == TeardropType::TdViapad {
                    "padvia"
                } else {
                    "track_end"
                }
            ))?;
        }

        self.out.print(format_args!("(connect_pads"))?;

        match zone.pad_connection() {
            ZoneConnection::ThtThermal => self.out.print(format_args!(" thru_hole_only"))?,
            ZoneConnection::Full => self.out.print(format_args!(" yes"))?,
            ZoneConnection::None => self.out.print(format_args!(" no"))?,
            // Default option (THERMAL) not saved or loaded.
            _ => 0,
        };

        // GetLocalClearance() always has a value (0 for a rule area).
        self.out.print(format_args!(
            "(clearance {})",
            format_iu(zone.local_clearance().unwrap_or(0))
        ))?;

        self.out.print(format_args!(")"))?;

        self.out.print(format_args!(
            "(min_thickness {})",
            format_iu(zone.min_thickness())
        ))?;

        if zone.is_rule_area() {
            let allowed = |not_allowed: bool| {
                if not_allowed {
                    "not_allowed"
                } else {
                    "allowed"
                }
            };

            // Keepout settings
            self.out.print(format_args!(
                "(keepout (tracks {}) (vias {}) (pads {}) (copperpour {}) (footprints {}))",
                allowed(zone.do_not_allow_tracks),
                allowed(zone.do_not_allow_vias),
                allowed(zone.do_not_allow_pads),
                allowed(zone.do_not_allow_zone_fills),
                allowed(zone.do_not_allow_footprints)
            ))?;

            // Multichannel settings
            self.out.print(format_args!("(placement"))?;
            format_bool(self.out, "enabled", zone.placement_area_enabled)?;

            match zone.placement_area_source_type {
                PlacementSourceT::Sheetname => {
                    let source = self.out.quotew(&zone.placement_area_source);
                    self.out.print(format_args!("(sheetname {source})"))?;
                }
                PlacementSourceT::ComponentClass => {
                    let source = self.out.quotew(&zone.placement_area_source);
                    self.out.print(format_args!("(component_class {source})"))?;
                }
                PlacementSourceT::GroupPlacement => {
                    let source = self.out.quotew(&zone.placement_area_source);
                    self.out.print(format_args!("(group {source})"))?;
                }
                // These are transitory and should not be saved
                PlacementSourceT::DesignBlock => {}
            }

            if zone.link_set != Kiid::nil() {
                let opts = &zone.repeat_options;

                let link = self.out.quotew(&zone.link_set.as_string());
                self.out.print(format_args!("(link {link}"))?;
                format_bool(self.out, "placement", opts.copy_placement)?;
                format_bool(self.out, "routing", opts.copy_routing)?;
                format_bool(self.out, "connected_routing", opts.connected_routing_only)?;
                format_bool(self.out, "other_items", opts.copy_other_items)?;
                format_bool(self.out, "group_items", opts.group_items)?;
                format_bool(self.out, "locked_items", opts.include_locked_items)?;

                if opts.anchor != Kiid::nil() {
                    let anchor = self.out.quotew(&opts.anchor.as_string());
                    self.out.print(format_args!("(anchor {anchor})"))?;
                }

                self.out.print(format_args!(")"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.out.print(format_args!("(fill"))?;

        // Default is not filled.
        if zone.is_filled() {
            self.out.print(format_args!(" yes"))?;
        }

        // Default is polygon filled.
        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            self.out.print(format_args!("(mode hatch)"))?;
        } else if zone.fill_mode() == ZoneFillMode::CopperThieving {
            self.out.print(format_args!("(mode thieving)"))?;
        }

        if !zone.is_teardrop_area() {
            self.out.print(format_args!(
                "(thermal_gap {}) (thermal_bridge_width {})",
                format_iu(zone.thermal_relief_gap()),
                format_iu(zone.thermal_relief_spoke_width())
            ))?;
        }

        // CORNER_SMOOTHING has no other value, so the C++ "unknown zone corner smoothing
        // type" error is unreachable.
        if zone.corner_smoothing_type != CornerSmoothing::NoSmoothing {
            match zone.corner_smoothing_type {
                CornerSmoothing::Chamfer => {
                    self.out.print(format_args!("(smoothing chamfer)"))?;
                }
                _ => {
                    self.out.print(format_args!("(smoothing fillet)"))?;
                }
            }

            if zone.corner_radius != 0 {
                // GetCornerRadius() is unsigned; formatInternalUnits takes an int.
                self.out.print(format_args!(
                    "(radius {})",
                    format_iu(zone.corner_radius as i32)
                ))?;
            }
        }

        self.out.print(format_args!(
            "(island_removal_mode {})",
            zone.island_removal_mode as i32
        ))?;

        if zone.island_removal_mode == IslandRemovalMode::Area {
            // long long / double, implicitly truncated to the int of formatInternalUnits.
            let area = zone.min_island_area as f64 / PCB_IU_SCALE.iu_per_mm;
            self.out.print(format_args!(
                "(island_area_min {})",
                format_iu(f64_to_i32_x86(area))
            ))?;
        }

        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            self.out.print(format_args!(
                "(hatch_thickness {}) (hatch_gap {}) (hatch_orientation {})",
                format_iu(zone.hatch_thickness),
                format_iu(zone.hatch_gap),
                format_double2_str(zone.hatch_orientation().as_degrees())
            ))?;

            if zone.hatch_smoothing_level > 0 {
                self.out.print(format_args!(
                    "(hatch_smoothing_level {}) (hatch_smoothing_value {})",
                    zone.hatch_smoothing_level,
                    format_double2_str(zone.hatch_smoothing_value)
                ))?;
            }

            self.out.print(format_args!(
                "(hatch_border_algorithm {}) (hatch_min_hole_area {})",
                if zone.hatch_border_algorithm != 0 {
                    "hatch_thickness"
                } else {
                    "min_thickness"
                },
                format_double2_str(zone.hatch_hole_min_area)
            ))?;
        } else if zone.fill_mode() == ZoneFillMode::CopperThieving {
            let thieving = &zone.thieving_settings;

            let pattern_str = match thieving.pattern {
                ThievingPattern::Squares => "squares",
                ThievingPattern::Hatch => "hatch",
                ThievingPattern::Dots => "dots",
            };

            self.out.print(format_args!(
                "(thieving (type {}) (size {}) (gap {}) (width {}) (stagger {}) (orientation {}))",
                pattern_str,
                format_iu(thieving.element_size),
                format_iu(thieving.gap),
                format_iu(thieving.line_width),
                if thieving.stagger { "yes" } else { "no" },
                format_double2_str(thieving.orientation.angle().as_degrees())
            ))?;
        }

        self.out.print(format_args!(")"))?;

        for (layer, properties) in &zone.layer_properties {
            self.format_zone_layer_properties(properties, 0, *layer)?;
        }

        if zone.num_corners() != 0 {
            // Outline() of a footprint zone is its library outline: no un-baking.
            let poly = zone.library_outline().c_polygon(0);

            for chain in poly {
                if chain.point_count() == 0 {
                    continue;
                }

                self.out.print(format_args!("(polygon"))?;
                self.format_poly_pts(chain, None)?;
                self.out.print(format_args!(")"))?;
            }
        }

        // Save the PolysList (filled areas)
        for layer in zone.layer_set(&ctx).seq() {
            let Some(fv) = zone.get_fill(layer) else {
                // m_FilledPolysList.at( aLayer ) throws std::out_of_range.
                return Err(IoError::new("map::at"));
            };

            for ii in 0..fv.outline_count() {
                self.out.print(format_args!("(filled_polygon"))?;
                let name = self.out.quotew(&Lset::name(layer));
                self.out.print(format_args!("(layer {name})"))?;

                if zone.is_island(layer, ii) {
                    format_bool(self.out, "island", true)?;
                }

                let chain = fv.c_outline(ii);

                self.format_poly_pts(chain, None)?;
                self.out.print(format_args!(")"))?;
            }
        }

        format_custom_properties(self.out, &zone.base.item.custom_properties)?;
        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `format( const ZONE_LAYER_PROPERTIES& aZoneLayerProperties, int aNestLevel,
    /// PCB_LAYER_ID aLayer )`: `(property (layer ..) (hatch_position (xy ..)))` of a zone or
    /// of the board zone defaults; nothing when no value is set.
    pub(crate) fn format_zone_layer_properties(
        &mut self,
        properties: &ZoneLayerProperties,
        nest_level: i32,
        layer: PcbLayerId,
    ) -> Result<(), IoError> {
        // Do not store the layer properties if no value is actually set.
        let Some(hatching_offset) = properties.hatching_offset else {
            return Ok(());
        };

        self.out
            .print_nest(nest_level, format_args!("(property\n"))?;
        let name = self.out.quotew(&Lset::name(layer));
        self.out
            .print_nest(nest_level, format_args!("(layer {name})\n"))?;

        self.out.print_nest(
            nest_level,
            format_args!("(hatch_position (xy {}))", format_iu_pt(hatching_offset)),
        )?;

        self.out.print_nest(nest_level, format_args!(")\n"))?;
        Ok(())
    }
}
