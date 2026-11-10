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
//! PCBIO-04: `formatSetup` and `formatDrillSymbolProfile` (W:601-745), plus the two formatters
//! it calls on model classes: `BOARD_STACKUP::FormatBoardStackup`
//! (`pcbnew/board_stackup_manager/board_stackup.cpp`) and `PCB_PLOT_PARAMS::Format`
//! (`pcbnew/pcb_plot_params.cpp`).

use kicad_core::plot_enums::DxfOutlineMode;
use kicad_core::plot_enums::DxfUnits;
use kicad_core::plot_enums::PlotTextMode;
use kicad_core::string_utils::format_double2_str;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_sexpr::{IoError, format_bool};

use super::{PcbWriter, format_iu};
use crate::board::Board;
use crate::board_design_settings::BoardDesignSettings;
use crate::board_stackup::{
    BoardStackup, BoardStackupItemType, BsEdgeConnectorConstraints, DielectricModel,
    is_prm_specified,
};
use crate::pcb_plot_params::{GBR_DEFAULT_PRECISION, PcbPlotParams};

#[path = "../parser/setup_drill_profile.rs"]
mod drill_profile;

use drill_profile::{
    DrillMarkMode, DrillSymbolProfile, drill_group_key_token, drill_mark_mode_token,
    drill_mark_policy_token,
};

impl PcbWriter<'_> {
    /// Port of `formatSetup( const BOARD* aBoard )`: the `(setup ..)` section.
    pub(crate) fn format_setup(&mut self, board: &Board) -> Result<(), IoError> {
        // Setup
        self.out.print(format_args!("(setup"))?;

        // Save the board physical stackup structure
        let bds = board.design_settings();
        let stackup = &bds.stackup;

        if bds.has_stackup {
            self.format_board_stackup(stackup)?;
        }

        self.out.print(format_args!(
            "(pad_to_mask_clearance {})",
            format_iu(bds.solder_mask_expansion)
        ))?;

        if bds.solder_mask_min_width != 0 {
            self.out.print(format_args!(
                "(solder_mask_min_width {})",
                format_iu(bds.solder_mask_min_width)
            ))?;
        }

        if bds.solder_paste_margin != 0 {
            self.out.print(format_args!(
                "(pad_to_paste_clearance {})",
                format_iu(bds.solder_paste_margin)
            ))?;
        }

        if bds.solder_paste_margin_ratio != 0.0 {
            self.out.print(format_args!(
                "(pad_to_paste_clearance_ratio {})",
                format_double2_str(bds.solder_paste_margin_ratio)
            ))?;
        }

        format_bool(
            &mut *self.out,
            "allow_soldermask_bridges_in_footprints",
            bds.allow_soldermask_bridges_in_fps,
        )?;

        self.out.print_nest(0, format_args!(" (tenting "))?;
        format_bool(&mut *self.out, "front", bds.tent_vias_front)?;
        format_bool(&mut *self.out, "back", bds.tent_vias_back)?;
        self.out.print_nest(0, format_args!(")"))?;

        self.out.print_nest(0, format_args!(" (covering "))?;
        format_bool(&mut *self.out, "front", bds.cover_vias_front)?;
        format_bool(&mut *self.out, "back", bds.cover_vias_back)?;
        self.out.print_nest(0, format_args!(")"))?;

        self.out.print_nest(0, format_args!(" (plugging "))?;
        format_bool(&mut *self.out, "front", bds.plug_vias_front)?;
        format_bool(&mut *self.out, "back", bds.plug_vias_back)?;
        self.out.print_nest(0, format_args!(")"))?;

        format_bool(&mut *self.out, "capping", bds.cap_vias)?;

        format_bool(&mut *self.out, "filling", bds.fill_vias)?;

        if !bds.zone_layer_properties.is_empty() {
            self.out.print_nest(0, format_args!(" (zone_defaults"))?;

            for (layer, properties) in &bds.zone_layer_properties {
                self.format_zone_layer_properties(properties, 0, *layer)?;
            }

            self.out.print_nest(0, format_args!(")\n"))?;
        }

        let mut origin = bds.aux_origin();

        if origin != Vector2I::new(0, 0) {
            self.out.print(format_args!(
                "(aux_axis_origin {} {})",
                format_iu(origin.x),
                format_iu(origin.y)
            ))?;
        }

        origin = bds.grid_origin();

        if origin != Vector2I::new(0, 0) {
            self.out.print(format_args!(
                "(grid_origin {} {})",
                format_iu(origin.x),
                format_iu(origin.y)
            ))?;
        }

        self.format_drill_symbol_profile(bds)?;

        self.format_plot_params(&board.plot_options)?;

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `formatDrillSymbolProfile( const BOARD_DESIGN_SETTINGS& )`: nothing for the
    /// default profile.
    pub(crate) fn format_drill_symbol_profile(
        &mut self,
        settings: &BoardDesignSettings,
    ) -> Result<(), IoError> {
        let profile = DrillSymbolProfile::from_model(settings.drill_symbol_profile.as_ref());
        let defaults = DrillSymbolProfile::default();

        // A board nobody has configured writes nothing, so existing files keep their bytes
        if profile == defaults {
            return Ok(());
        }

        self.out.print(format_args!("(drill_symbol_profile"))?;

        if !profile.name.is_empty() {
            let name = self.out.quotew(&profile.name);
            self.out.print(format_args!("(name {name})"))?;
        }

        self.out.print(format_args!("(group_by"))?;

        for key in &profile.group_by {
            self.out
                .print(format_args!(" {}", drill_group_key_token(*key)))?;
        }

        self.out.print(format_args!(")"))?;

        self.out.print(format_args!(
            "(default_marks {})",
            drill_mark_policy_token(profile.mark_policy)
        ))?;
        self.out
            .print(format_args!("(size {})", format_iu(profile.symbol_size)))?;
        self.out
            .print(format_args!("(width {})", format_iu(profile.symbol_width)))?;
        format_bool(
            &mut *self.out,
            "freeze_assignments",
            profile.freeze_assignments,
        )?;

        for (key, assignment) in &profile.assignments {
            let quoted_key = self.out.quotew(key);
            self.out.print(format_args!(
                "(assignment (key {quoted_key}) (mark {}",
                drill_mark_mode_token(assignment.mark_mode)
            ))?;

            if assignment.mark_mode == DrillMarkMode::Shape {
                self.out
                    .print(format_args!(" {}", assignment.shape_index))?;
            } else if assignment.mark_mode == DrillMarkMode::Letter {
                let letter = self.out.quotew(&assignment.letter);
                self.out.print(format_args!(" {letter}"))?;
            }

            self.out.print(format_args!(")"))?;

            if !assignment.description.is_empty() {
                let descr = self.out.quotew(&assignment.description);
                self.out.print(format_args!("(descr {descr})"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `BOARD_STACKUP::FormatBoardStackup( OUTPUTFORMATTER*, const BOARD* )`.
    fn format_board_stackup(&mut self, stackup: &BoardStackup) -> Result<(), IoError> {
        // Board stackup is the ordered list from top to bottom of
        // physical layers and substrate used to build the board.
        if stackup.list.is_empty() {
            return Ok(());
        }

        self.out.print(format_args!("(stackup"))?;

        // Note:
        // Unspecified parameters are not stored in file.
        for item in &stackup.list {
            let layer_name = if item.brd_layer_id() == PcbLayerId::UndefinedLayer {
                format!("dielectric {}", item.dielectric_layer_id())
            } else {
                Lset::name(item.brd_layer_id())
            };

            let quoted_name = self.out.quotew(&layer_name);
            let quoted_type = self.out.quotew(item.type_name());
            self.out
                .print(format_args!("(layer {quoted_name} (type {quoted_type})"))?;

            // Output other parameters (in sub layer list there is at least one item)
            for idx in 0..item.sublayers_count() {
                if idx != 0 {
                    // not for the main (first) layer.
                    self.out.print(format_args!(" addsublayer"))?;
                }

                if item.is_color_editable() && is_prm_specified(&item.color(idx)) {
                    let color = self.out.quotew(&item.color(idx));
                    self.out.print(format_args!("(color {color})"))?;
                }

                if item.is_thickness_editable() {
                    self.out
                        .print(format_args!("(thickness {}", format_iu(item.thickness(idx))))?;

                    if item.type_() == BoardStackupItemType::Dielectric
                        && item.is_thickness_locked(idx)
                    {
                        self.out.print(format_args!(" locked"))?;
                    }

                    self.out.print(format_args!(")"))?;
                }

                if item.has_material_value(idx) {
                    let material = self.out.quotew(&item.material(idx));
                    self.out.print(format_args!("(material {material})"))?;
                }

                if item.has_epsilon_r_value() && item.has_material_value(idx) {
                    self.out.print(format_args!(
                        "(epsilon_r {})",
                        format_double2_str(item.epsilon_r(idx))
                    ))?;
                }

                if item.has_loss_tangent_value() && item.has_material_value(idx) {
                    self.out.print(format_args!(
                        "(loss_tangent {})",
                        format_double2_str(item.loss_tangent(idx))
                    ))?;
                }

                if item.has_spec_freq_value() && item.has_material_value(idx) {
                    self.out.print(format_args!(
                        "(spec_frequency {})",
                        format_double2_str(item.spec_freq(idx))
                    ))?;

                    let mut model_token = "constant";

                    if item.dielectric_model(idx) == DielectricModel::DjordjevicSarkar {
                        model_token = "djordjevic_sarkar";
                    }

                    self.out
                        .print(format_args!("(dielectric_model {model_token})"))?;
                }
            }

            self.out.print(format_args!(")"))?;
        }

        // Other infos about board, related to layers and other fabrication specifications
        if is_prm_specified(&stackup.finish_type) {
            let finish = self.out.quotew(&stackup.finish_type);
            self.out.print(format_args!("(copper_finish {finish})"))?;
        }

        format_bool(
            &mut *self.out,
            "dielectric_constraints",
            stackup.has_dielectric_constrains,
        )?;

        if stackup.edge_connector_constraints != BsEdgeConnectorConstraints::None {
            self.out.print(format_args!(
                "(edge_connector {})",
                if stackup.edge_connector_constraints == BsEdgeConnectorConstraints::Bevelled {
                    "bevelled"
                } else {
                    "yes"
                }
            ))?;
        }

        if stackup.edge_plating {
            format_bool(&mut *self.out, "edge_plating", true)?;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `PCB_PLOT_PARAMS::Format( OUTPUTFORMATTER* )`.
    fn format_plot_params(&mut self, params: &PcbPlotParams) -> Result<(), IoError> {
        let out = &mut *self.out;

        out.print(format_args!("(pcbplotparams"))?;

        out.print(format_args!(
            "(layerselection 0x{})",
            params.layer_selection.fmt_hex()
        ))?;

        let mut common_layers = Lset::new();

        for common_layer in &params.plot_on_all_layers_sequence {
            common_layers.set(*common_layer);
        }

        out.print(format_args!(
            "(plot_on_all_layers_selection 0x{})",
            common_layers.fmt_hex()
        ))?;

        format_bool(out, "disableapertmacros", params.gerber_disable_apert_macros)?;
        format_bool(out, "usegerberextensions", params.use_gerber_protel_extensions)?;
        format_bool(out, "usegerberattributes", params.use_gerber_x2_format)?;
        format_bool(
            out,
            "usegerberadvancedattributes",
            params.include_gerber_netlist_info,
        )?;
        format_bool(out, "creategerberjobfile", params.create_gerber_job_file)?;

        // save this option only if it is not the default value,
        // to avoid incompatibility with older Pcbnew version
        if params.gerber_precision != GBR_DEFAULT_PRECISION {
            out.print(format_args!("(gerberprecision {})", params.gerber_precision))?;
        }

        out.print(format_args!(
            "(dashed_line_dash_ratio {})",
            format_double2_str(params.dashed_line_dash_ratio)
        ))?;
        out.print(format_args!(
            "(dashed_line_gap_ratio {})",
            format_double2_str(params.dashed_line_gap_ratio)
        ))?;

        // SVG options
        out.print(format_args!("(svgprecision {})", params.svg_precision))?;

        // PNG options
        out.print(format_args!("(pngdpi {})", params.png_dpi))?;
        format_bool(out, "pngantialias", params.png_antialias)?;

        format_bool(out, "plotframeref", params.plot_drawing_sheet)?;
        out.print(format_args!(
            "(mode {})",
            if params.dxf_plot_mode == DxfOutlineMode::Sketch {
                2
            } else {
                1
            }
        ))?;
        format_bool(out, "useauxorigin", params.use_aux_origin)?;

        // PDF options
        format_bool(
            out,
            "pdf_front_fp_property_popups",
            params.pdf_front_fp_property_popups,
        )?;
        format_bool(
            out,
            "pdf_back_fp_property_popups",
            params.pdf_back_fp_property_popups,
        )?;
        format_bool(out, "pdf_metadata", params.pdf_metadata)?;
        format_bool(out, "pdf_single_document", params.pdf_single)?;

        // DXF options
        format_bool(out, "dxfpolygonmode", params.dxf_polygon_mode)?;
        format_bool(out, "dxfimperialunits", params.dxf_units == DxfUnits::Inch)?;
        format_bool(
            out,
            "dxfusepcbnewfont",
            params.text_mode != PlotTextMode::Native,
        )?;

        format_bool(out, "psnegative", params.negative)?;
        format_bool(out, "psa4output", params.a4_output)?;

        format_bool(out, "plot_black_and_white", params.black_and_white)?;

        format_bool(out, "sketchpadsonfab", params.sketch_pads_on_fab_layers)?;
        format_bool(out, "plotpadnumbers", params.plot_pad_numbers)?;
        format_bool(out, "hidednponfab", params.hide_dnp_fps_on_fab_layers)?;
        format_bool(out, "sketchdnponfab", params.sketch_dnp_fps_on_fab_layers)?;
        format_bool(out, "crossoutdnponfab", params.crossout_dnp_fps_on_fab_layers)?;
        format_bool(out, "subtractmaskfromsilk", params.subtract_mask_from_silk)?;
        out.print(format_args!("(outputformat {})", params.format as i32))?;
        format_bool(out, "mirror", params.mirror)?;
        out.print(format_args!("(drillshape {})", params.drill_marks as i32))?;
        out.print(format_args!("(scaleselection {})", params.scale_selection))?;
        let directory = out.quotew(&params.output_directory);
        out.print(format_args!("(outputdirectory {directory})"))?;
        out.print(format_args!(")"))?;
        Ok(())
    }
}
