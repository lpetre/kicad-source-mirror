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

//! Settings classes, the `board` section and the `order` section (`dump_settings.cpp`).

use std::cmp::Ordering;
use std::sync::Arc;

use kicad_core::PcbLayerId;

use super::format::{
    Out, board_items, emit, no_api, v_angle, v_bool, v_box, v_enum, v_f64, v_int, v_layer,
    v_layer_names, v_layers, v_lset, v_opt_bool, v_polyset, v_str, v_vec,
};
use crate::board_design_settings::{BoardDesignSettings, LAYER_CLASS_COUNT};
use crate::board_stackup::BoardStackup;
use crate::ctx::BoardCtx;
use crate::ids::ItemId;
use crate::netclass::Netclass;
use crate::pcb_plot_params::PcbPlotParams;
use crate::teardrop_parameters::{TargetTd, TeardropParameters};
use crate::zone_settings::ZoneSettings;

/// `dumpMeander`: `PNS::MEANDER_SETTINGS` is not modelled (the Rust settings keep the JSON of
/// `tuning_pattern_settings`), so every field is pending.
fn dump_meander(o: &mut Out, p: &str) {
    for f in [
        "min_amplitude",
        "max_amplitude",
        "spacing",
        "step",
        "len_pad_to_die",
        "signal_extra_length",
        "signal_extra_delay",
        "target_length",
        "target_length_delay",
        "target_signal_length",
        "target_signal_length_delay",
        "target_skew",
        "target_skew_delay",
        "override_custom_rules",
        "corner_style",
        "corner_radius_percentage",
        "single_sided",
        "initial_side",
        "length_tolerance",
        "keep_endpoints",
        "is_time_domain",
    ] {
        emit!(o, format!("{p}{f}"), no_api("PNS::MEANDER_SETTINGS"));
    }
}

/// `DumpTeardropParams`.
pub(crate) fn dump_teardrop_params(o: &mut Out, p: &str, t: &TeardropParameters) {
    emit!(o, format!("{p}max_len"), v_int(t.td_max_len));
    emit!(o, format!("{p}max_width"), v_int(t.td_max_width));
    emit!(
        o,
        format!("{p}best_length_ratio"),
        v_f64(t.best_length_ratio)
    );
    emit!(o, format!("{p}best_width_ratio"), v_f64(t.best_width_ratio));
    emit!(
        o,
        format!("{p}width_to_size_filter_ratio"),
        v_f64(t.width_to_size_filter_ratio)
    );
    emit!(o, format!("{p}curved_edges"), v_bool(t.curved_edges));
    emit!(o, format!("{p}enabled"), v_bool(t.enabled));
    emit!(
        o,
        format!("{p}allow_use_two_tracks"),
        v_bool(t.allow_use_two_tracks)
    );
    emit!(
        o,
        format!("{p}td_on_pads_in_zones"),
        v_bool(t.td_on_pads_in_zones)
    );
}

/// `DumpZoneSettings`.
pub(crate) fn dump_zone_settings(o: &mut Out, p: &str, z: &ZoneSettings) {
    emit!(o, format!("{p}priority"), v_int(z.zone_priority));
    emit!(o, format!("{p}fill_mode"), v_enum(z.fill_mode));
    emit!(o, format!("{p}clearance"), v_int(z.zone_clearance));
    emit!(o, format!("{p}min_thickness"), v_int(z.zone_min_thickness));
    emit!(o, format!("{p}hatch_thickness"), v_int(z.hatch_thickness));
    emit!(o, format!("{p}hatch_gap"), v_int(z.hatch_gap));
    emit!(
        o,
        format!("{p}hatch_orientation"),
        v_angle(z.hatch_orientation)
    );
    emit!(
        o,
        format!("{p}hatch_smoothing_level"),
        v_int(z.hatch_smoothing_level)
    );
    emit!(
        o,
        format!("{p}hatch_smoothing_value"),
        v_f64(z.hatch_smoothing_value)
    );
    emit!(
        o,
        format!("{p}hatch_hole_min_area"),
        v_f64(z.hatch_hole_min_area)
    );
    emit!(
        o,
        format!("{p}hatch_border_algorithm"),
        v_int(z.hatch_border_algorithm)
    );
    let t = &z.thieving_settings;
    emit!(o, format!("{p}thieving.pattern"), v_enum(t.pattern));
    emit!(
        o,
        format!("{p}thieving.element_size"),
        v_int(t.element_size)
    );
    emit!(o, format!("{p}thieving.gap"), v_int(t.gap));
    emit!(o, format!("{p}thieving.line_width"), v_int(t.line_width));
    emit!(o, format!("{p}thieving.stagger"), v_bool(t.stagger));
    emit!(
        o,
        format!("{p}thieving.orientation"),
        v_angle(t.orientation.angle())
    );
    emit!(o, format!("{p}netcode"), v_int(z.netcode));
    emit!(o, format!("{p}name"), v_str(&z.name));
    emit!(o, format!("{p}layers"), v_lset(&z.layers));
    emit!(
        o,
        format!("{p}border_display_style"),
        v_enum(z.zone_border_display_style)
    );
    emit!(
        o,
        format!("{p}border_hatch_pitch"),
        v_int(z.border_hatch_pitch)
    );
    emit!(
        o,
        format!("{p}thermal_relief_gap"),
        v_int(z.thermal_relief_gap)
    );
    emit!(
        o,
        format!("{p}thermal_relief_spoke_width"),
        v_int(z.thermal_relief_spoke_width)
    );
    emit!(o, format!("{p}locked"), v_bool(z.locked));
    emit!(o, format!("{p}teardrop_type"), v_enum(z.teardrop_type));
    emit!(
        o,
        format!("{p}corner_smoothing"),
        v_enum(z.corner_smoothing_type)
    );
    emit!(o, format!("{p}corner_radius"), v_int(z.corner_radius));
    emit!(o, format!("{p}pad_connection"), v_enum(z.pad_connection));
    emit!(o, format!("{p}rule_area"), v_bool(z.is_rule_area));
    emit!(
        o,
        format!("{p}placement_area_enabled"),
        v_bool(z.placement_area_enabled)
    );
    emit!(
        o,
        format!("{p}placement_area_source_type"),
        v_enum(z.placement_area_source_type)
    );
    emit!(
        o,
        format!("{p}placement_area_source"),
        v_str(&z.placement_area_source)
    );
    emit!(
        o,
        format!("{p}keepout.zone_fills"),
        v_bool(z.keepout_do_not_allow_zone_fills)
    );
    emit!(
        o,
        format!("{p}keepout.vias"),
        v_bool(z.keepout_do_not_allow_vias)
    );
    emit!(
        o,
        format!("{p}keepout.tracks"),
        v_bool(z.keepout_do_not_allow_tracks)
    );
    emit!(
        o,
        format!("{p}keepout.pads"),
        v_bool(z.keepout_do_not_allow_pads)
    );
    emit!(
        o,
        format!("{p}keepout.footprints"),
        v_bool(z.keepout_do_not_allow_footprints)
    );
    emit!(
        o,
        format!("{p}island_removal_mode"),
        v_enum(z.remove_islands)
    );
    emit!(o, format!("{p}min_island_area"), v_int(z.min_island_area));

    for (layer, props) in &z.layer_properties {
        emit!(
            o,
            format!("{p}layer_properties.{}.hatching_offset", v_layer(*layer)),
            props
                .hatching_offset
                .map_or_else(|| "none".to_string(), v_vec)
        );
    }
}

/// `optParent( nc.GetXxxParent() )`: in the Rust model a `None` parent is the class itself
/// (`NETCLASS::ResetParents` sets `this`), so it prints the class's own name.
fn opt_parent(nc: &Netclass, p: &Option<Arc<Netclass>>) -> String {
    v_str(&p.as_ref().map_or_else(|| nc.get_name(), |n| n.get_name()))
}

fn v_opt_i32(v: Option<i32>) -> String {
    v.map_or_else(|| "none".to_string(), v_int)
}

/// `DumpNetclass`.
pub(crate) fn dump_netclass(o: &mut Out, p: &str, nc: &Netclass) {
    emit!(o, format!("{p}name"), v_str(&nc.get_name()));
    emit!(o, format!("{p}is_default"), v_bool(nc.is_default()));
    emit!(o, format!("{p}description"), v_str(nc.description()));
    emit!(o, format!("{p}priority"), v_int(nc.priority()));
    emit!(
        o,
        format!("{p}clearance"),
        if nc.has_clearance() {
            v_int(nc.clearance())
        } else {
            "none".to_string()
        }
    );
    emit!(o, format!("{p}track_width"), v_opt_i32(nc.track_width));
    emit!(o, format!("{p}via_diameter"), v_opt_i32(nc.via_dia));
    emit!(o, format!("{p}via_drill"), v_opt_i32(nc.via_drill));
    emit!(o, format!("{p}uvia_diameter"), v_opt_i32(nc.uvia_dia));
    emit!(o, format!("{p}uvia_drill"), v_opt_i32(nc.uvia_drill));
    emit!(
        o,
        format!("{p}diff_pair_width"),
        v_opt_i32(nc.diff_pair_width)
    );
    emit!(o, format!("{p}diff_pair_gap"), v_opt_i32(nc.diff_pair_gap));
    emit!(
        o,
        format!("{p}diff_pair_via_gap"),
        v_opt_i32(nc.diff_pair_via_gap)
    );
    emit!(o, format!("{p}wire_width"), v_opt_i32(nc.wire_width));
    emit!(o, format!("{p}bus_width"), v_opt_i32(nc.bus_width));
    emit!(o, format!("{p}line_style"), v_opt_i32(nc.line_style));
    emit!(
        o,
        format!("{p}pcb_color"),
        v_str(&nc.pcb_color.to_css_string())
    );
    emit!(
        o,
        format!("{p}schematic_color"),
        v_str(&nc.schematic_color.to_css_string())
    );
    emit!(o, format!("{p}tuning_profile"), v_str(nc.tuning_profile()));
    emit!(
        o,
        format!("{p}track_width_parent"),
        opt_parent(nc, &nc.parents.track_width)
    );
    emit!(
        o,
        format!("{p}via_diameter_parent"),
        opt_parent(nc, &nc.parents.via_diameter)
    );
    emit!(o, format!("{p}constituents"), {
        let v: Vec<String> = nc
            .constituent_netclasses()
            .iter()
            .map(|n| v_str(&n.get_name()))
            .collect();
        format!("[{}]", v.join(" "))
    });
}

/// `DumpStackup`.
pub(crate) fn dump_stackup(o: &mut Out, p: &str, s: &BoardStackup) {
    emit!(o, format!("{p}finish_type"), v_str(&s.finish_type));
    emit!(
        o,
        format!("{p}has_dielectric_constraints"),
        v_bool(s.has_dielectric_constrains)
    );
    emit!(
        o,
        format!("{p}has_thickness_constraints"),
        v_bool(s.has_thickness_constrains)
    );
    emit!(
        o,
        format!("{p}edge_connector_constraints"),
        format!("BS_EDGE_CONNECTOR_{}", v_enum(s.edge_connector_constraints))
    );
    emit!(o, format!("{p}edge_plating"), v_bool(s.edge_plating));
    emit!(o, format!("{p}count"), v_int(s.count() as i64));
    emit!(
        o,
        format!("{p}board_thickness"),
        v_int(s.build_board_thickness_from_stackup())
    );

    for (i, item) in s.list().iter().enumerate() {
        let ip = format!("{p}item.{i}.");
        emit!(
            o,
            format!("{ip}type"),
            format!("BS_ITEM_TYPE_{}", v_enum(item.type_))
        );
        emit!(o, format!("{ip}layer"), v_layer(item.layer_id));
        emit!(o, format!("{ip}layer_name"), v_str(&item.layer_name));
        emit!(o, format!("{ip}type_name"), v_str(&item.type_name));
        emit!(
            o,
            format!("{ip}dielectric_layer_id"),
            v_int(item.dielectric_layer_id)
        );
        emit!(o, format!("{ip}enabled"), v_bool(item.enabled));
        emit!(
            o,
            format!("{ip}sublayers"),
            v_int(item.sublayers_count() as i64)
        );

        for sl in 0..item.sublayers_count() {
            let sp = format!("{ip}sub.{sl}.");
            emit!(o, format!("{sp}thickness"), v_int(item.thickness(sl)));
            emit!(
                o,
                format!("{sp}thickness_locked"),
                v_bool(item.is_thickness_locked(sl))
            );
            emit!(o, format!("{sp}material"), v_str(&item.material(sl)));
            emit!(o, format!("{sp}color"), v_str(&item.color(sl)));
            emit!(o, format!("{sp}epsilon_r"), v_f64(item.epsilon_r(sl)));
            emit!(o, format!("{sp}loss_tangent"), v_f64(item.loss_tangent(sl)));
            emit!(o, format!("{sp}spec_freq"), v_f64(item.spec_freq(sl)));
            emit!(
                o,
                format!("{sp}dielectric_model"),
                v_enum(item.dielectric_model(sl))
            );
        }
    }
}

/// `DumpPlotParams`.
pub(crate) fn dump_plot_params(o: &mut Out, p: &str, pp: &PcbPlotParams) {
    emit!(o, format!("{p}text_mode"), v_enum(pp.text_mode));
    emit!(o, format!("{p}dxf_plot_mode"), v_enum(pp.dxf_plot_mode));
    emit!(
        o,
        format!("{p}plot_pad_numbers"),
        v_bool(pp.plot_pad_numbers)
    );
    emit!(
        o,
        format!("{p}dxf_polygon_mode"),
        v_bool(pp.dxf_polygon_mode)
    );
    emit!(o, format!("{p}dxf_units"), v_enum(pp.dxf_units));
    emit!(o, format!("{p}drill_marks"), v_enum(pp.drill_marks));
    emit!(o, format!("{p}scale"), v_f64(pp.scale));
    emit!(
        o,
        format!("{p}fine_scale_adjust"),
        format!(
            "{},{}",
            v_f64(pp.fine_scale_adjust_x),
            v_f64(pp.fine_scale_adjust_y)
        )
    );
    emit!(o, format!("{p}width_adjust"), v_int(pp.width_adjust));
    emit!(o, format!("{p}auto_scale"), v_bool(pp.auto_scale));
    emit!(o, format!("{p}mirror"), v_bool(pp.mirror));
    emit!(
        o,
        format!("{p}sketch_pads_on_fab"),
        v_bool(pp.sketch_pads_on_fab_layers)
    );
    emit!(
        o,
        format!("{p}sketch_pad_line_width"),
        v_int(pp.sketch_pad_line_width)
    );
    emit!(
        o,
        format!("{p}hide_dnp_on_fab"),
        v_bool(pp.hide_dnp_fps_on_fab_layers)
    );
    emit!(
        o,
        format!("{p}sketch_dnp_on_fab"),
        v_bool(pp.sketch_dnp_fps_on_fab_layers)
    );
    emit!(
        o,
        format!("{p}crossout_dnp_on_fab"),
        v_bool(pp.crossout_dnp_fps_on_fab_layers)
    );
    emit!(o, format!("{p}plot_value"), v_bool(pp.plot_value));
    emit!(o, format!("{p}plot_reference"), v_bool(pp.plot_reference));
    emit!(o, format!("{p}plot_fp_text"), v_bool(pp.plot_fp_text));
    emit!(o, format!("{p}negative"), v_bool(pp.negative));
    emit!(
        o,
        format!("{p}legacy_plot_via_on_mask"),
        v_opt_bool(pp.plot_via_on_mask_layer)
    );
    emit!(
        o,
        format!("{p}plot_frame_ref"),
        v_bool(pp.plot_drawing_sheet)
    );
    emit!(o, format!("{p}format"), v_enum(pp.format));
    emit!(
        o,
        format!("{p}output_directory"),
        v_str(&pp.output_directory)
    );
    emit!(
        o,
        format!("{p}disable_gerber_macros"),
        v_bool(pp.gerber_disable_apert_macros)
    );
    emit!(
        o,
        format!("{p}use_gerber_x2"),
        v_bool(pp.use_gerber_x2_format)
    );
    emit!(
        o,
        format!("{p}gerber_netlist_info"),
        v_bool(pp.include_gerber_netlist_info)
    );
    emit!(
        o,
        format!("{p}gerber_job_file"),
        v_bool(pp.create_gerber_job_file)
    );
    emit!(
        o,
        format!("{p}gerber_protel_ext"),
        v_bool(pp.use_gerber_protel_extensions)
    );
    emit!(
        o,
        format!("{p}gerber_precision"),
        v_int(pp.gerber_precision)
    );
    emit!(o, format!("{p}svg_precision"), v_int(pp.svg_precision));
    emit!(
        o,
        format!("{p}svg_fit_page"),
        v_bool(pp.svg_fit_page_to_board)
    );
    emit!(o, format!("{p}png_dpi"), v_int(pp.png_dpi));
    emit!(o, format!("{p}png_antialias"), v_bool(pp.png_antialias));
    emit!(o, format!("{p}black_and_white"), v_bool(pp.black_and_white));
    emit!(
        o,
        format!("{p}subtract_mask_from_silk"),
        v_bool(pp.subtract_mask_from_silk)
    );
    emit!(
        o,
        format!("{p}layer_selection"),
        v_lset(&pp.layer_selection)
    );
    emit!(
        o,
        format!("{p}plot_on_all_layers"),
        v_layers(pp.plot_on_all_layers_sequence.iter().copied())
    );
    emit!(o, format!("{p}use_aux_origin"), v_bool(pp.use_aux_origin));
    emit!(o, format!("{p}scale_selection"), v_int(pp.scale_selection));
    emit!(o, format!("{p}a4_output"), v_bool(pp.a4_output));
    emit!(
        o,
        format!("{p}dashed_line_dash_ratio"),
        v_f64(pp.dashed_line_dash_ratio)
    );
    emit!(
        o,
        format!("{p}dashed_line_gap_ratio"),
        v_f64(pp.dashed_line_gap_ratio)
    );
    emit!(
        o,
        format!("{p}dxf_multilayered"),
        v_bool(pp.dxf_export_as_multi_layered_file)
    );
    emit!(o, format!("{p}layer"), v_layer(pp.layer));
}

fn join_ints(v: impl IntoIterator<Item = String>) -> String {
    let v: Vec<String> = v.into_iter().collect();
    format!("[{}]", v.join(" "))
}

/// The C++ magic_enum names of `TARGET_TD` without `TARGET_COUNT` / `TARGET_UNKNOWN`, in
/// enumerator order.
const TARGETS: [(TargetTd, &str); 4] = [
    (TargetTd::Round, "TARGET_ROUND"),
    (TargetTd::Rect, "TARGET_RECT"),
    (TargetTd::Track, "TARGET_TRACK"),
    (TargetTd::Unknown, "TARGET_UNKNOWN"),
];

/// `DumpDesignSettings`.
pub(crate) fn dump_design_settings(o: &mut Out, p: &str, b: &BoardDesignSettings) {
    emit!(
        o,
        format!("{p}track_width_list"),
        join_ints(b.track_width_list.iter().map(|w| v_int(*w)))
    );
    emit!(
        o,
        format!("{p}vias_dimensions_list"),
        join_ints(
            b.vias_dimensions_list
                .iter()
                .map(|v| format!("{}/{}", v.diameter, v.drill))
        )
    );
    emit!(
        o,
        format!("{p}diff_pair_dimensions_list"),
        join_ints(
            b.diff_pair_dimensions_list
                .iter()
                .map(|d| format!("{}/{}/{}", d.width, d.gap, d.via_gap))
        )
    );

    for (i, v) in b.via_stack_presets.iter().enumerate() {
        let vp = format!("{p}via_stack_preset.{i}.");
        emit!(o, format!("{vp}name"), v_str(&v.name));
        emit!(
            o,
            format!("{vp}layers"),
            format!("{},{}", v_layer(v.start_layer), v_layer(v.end_layer))
        );
        emit!(o, format!("{vp}staggered"), v_bool(v.staggered));
        emit!(
            o,
            format!("{vp}size"),
            format!("{}/{}", v.via_size, v.via_drill)
        );
        emit!(o, format!("{vp}use_netclass"), v_bool(v.use_netclass));
        emit!(o, format!("{vp}filled"), v_bool(v.filled));
        emit!(o, format!("{vp}capped"), v_bool(v.capped));
        emit!(o, format!("{vp}pitch"), v_int(v.pitch));
    }

    let tdl = b.teardrop_params_list();
    emit!(
        o,
        format!("{p}teardrops.target_vias"),
        v_bool(tdl.target_vias)
    );
    emit!(
        o,
        format!("{p}teardrops.target_pth_pads"),
        v_bool(tdl.target_pth_pads)
    );
    emit!(
        o,
        format!("{p}teardrops.target_smd_pads"),
        v_bool(tdl.target_smd_pads)
    );
    emit!(
        o,
        format!("{p}teardrops.target_track_to_track"),
        v_bool(tdl.target_track2track)
    );
    emit!(
        o,
        format!("{p}teardrops.use_round_shapes_only"),
        v_bool(tdl.use_round_shapes_only)
    );

    for (t, name) in TARGETS {
        if t == TargetTd::Unknown {
            continue;
        }

        let tp = format!("{p}teardrops.{name}.");

        if let Some(params) = o.cond(&format!("{tp}*"), || tdl.parameters(t)) {
            dump_teardrop_params(o, &tp, params);
        }
    }

    dump_meander(o, &format!("{p}meander.single."));
    dump_meander(o, &format!("{p}meander.diff_pair."));
    dump_meander(o, &format!("{p}meander.skew."));

    emit!(
        o,
        format!("{p}current_via_type"),
        v_enum(b.current_via_type)
    );
    emit!(
        o,
        format!("{p}use_connected_track_width"),
        v_bool(b.use_connected_track_width)
    );
    emit!(
        o,
        format!("{p}temp_override_track_width"),
        v_bool(b.temp_override_track_width)
    );

    for (f, v) in [
        ("min_clearance", b.min_clearance),
        ("min_groove_width", b.min_groove_width),
        ("min_conn", b.min_conn),
        ("track_min_width", b.track_min_width),
        ("vias_min_annular_width", b.vias_min_annular_width),
        ("vias_min_size", b.vias_min_size),
        ("min_through_drill", b.min_through_drill),
        ("micro_vias_min_size", b.micro_vias_min_size),
        ("micro_vias_min_drill", b.micro_vias_min_drill),
        ("copper_edge_clearance", b.copper_edge_clearance),
        ("hole_clearance", b.hole_clearance),
        ("hole_to_hole_min", b.hole_to_hole_min),
        ("silk_clearance", b.silk_clearance),
        ("min_resolved_spokes", b.min_resolved_spokes),
        ("min_silk_text_height", b.min_silk_text_height),
        ("min_silk_text_thickness", b.min_silk_text_thickness),
    ] {
        emit!(o, format!("{p}{f}"), v_int(v));
    }

    for (code, sev) in &b.drc_severities {
        emit!(
            o,
            format!("{p}drc_severity.{}", super::defaults::drc_code_name(*code)),
            super::defaults::severity_name(*sev)
        );
    }

    emit!(
        o,
        format!("{p}drc_exclusions"),
        v_int(b.drc_exclusions.len() as i64)
    );
    emit!(
        o,
        format!("{p}zone_keep_external_fillets"),
        v_bool(b.zone_keep_external_fillets)
    );
    emit!(o, format!("{p}max_error"), v_int(b.max_error));
    emit!(
        o,
        format!("{p}solder_mask_expansion"),
        v_int(b.solder_mask_expansion)
    );
    emit!(
        o,
        format!("{p}solder_mask_min_width"),
        v_int(b.solder_mask_min_width)
    );
    emit!(
        o,
        format!("{p}solder_mask_to_copper_clearance"),
        v_int(b.solder_mask_to_copper_clearance)
    );
    emit!(
        o,
        format!("{p}solder_paste_margin"),
        v_int(b.solder_paste_margin)
    );
    emit!(
        o,
        format!("{p}solder_paste_margin_ratio"),
        v_f64(b.solder_paste_margin_ratio)
    );

    for (f, v) in [
        (
            "allow_soldermask_bridges_in_fps",
            b.allow_soldermask_bridges_in_fps,
        ),
        ("tent_vias_front", b.tent_vias_front),
        ("tent_vias_back", b.tent_vias_back),
        ("cover_vias_front", b.cover_vias_front),
        ("cover_vias_back", b.cover_vias_back),
        ("plug_vias_front", b.plug_vias_front),
        ("plug_vias_back", b.plug_vias_back),
        ("cap_vias", b.cap_vias),
        ("fill_vias", b.fill_vias),
    ] {
        emit!(o, format!("{p}{f}"), v_bool(v));
    }

    for (i, t) in b.default_fp_text_items.iter().enumerate() {
        emit!(
            o,
            format!("{p}default_fp_text.{}", 2 * i + 1),
            format!(
                "{} {} {}",
                v_str(&t.text),
                v_bool(t.visible),
                v_layer(t.layer)
            )
        );
    }

    for (k, v) in &b.user_layer_names {
        emit!(o, format!("{p}user_layer_name.{k}"), v_str(v));
    }

    for (layer, props) in &b.zone_layer_properties {
        emit!(
            o,
            format!(
                "{p}zone_layer_properties.{}.hatching_offset",
                v_layer(*layer)
            ),
            props
                .hatching_offset
                .map_or_else(|| "none".to_string(), v_vec)
        );
    }

    const CLASS_NAMES: [&str; 6] = ["silk", "copper", "edges", "courtyard", "fab", "other"];

    for c in 0..LAYER_CLASS_COUNT {
        let cp = format!("{p}layer_class.{}.", CLASS_NAMES[c]);
        emit!(o, format!("{cp}line_thickness"), v_int(b.line_thickness[c]));
        emit!(o, format!("{cp}text_size"), v_vec(b.text_size[c]));
        emit!(o, format!("{cp}text_thickness"), v_int(b.text_thickness[c]));
        emit!(o, format!("{cp}text_italic"), v_bool(b.text_italic[c]));
        emit!(o, format!("{cp}text_upright"), v_bool(b.text_upright[c]));
    }

    emit!(
        o,
        format!("{p}dimension.units_mode"),
        v_enum(b.dimension_units_mode)
    );
    emit!(
        o,
        format!("{p}dimension.precision"),
        v_enum(b.dimension_precision)
    );
    emit!(
        o,
        format!("{p}dimension.units_format"),
        v_enum(b.dimension_units_format)
    );
    emit!(
        o,
        format!("{p}dimension.suppress_zeroes"),
        v_bool(b.dimension_suppress_zeroes)
    );
    emit!(
        o,
        format!("{p}dimension.text_position"),
        v_enum(b.dimension_text_position)
    );
    emit!(
        o,
        format!("{p}dimension.keep_text_aligned"),
        v_bool(b.dimension_keep_text_aligned)
    );
    emit!(
        o,
        format!("{p}dimension.arrow_length"),
        v_int(b.dimension_arrow_length)
    );
    emit!(
        o,
        format!("{p}dimension.extension_offset"),
        v_int(b.dimension_extension_offset)
    );

    for (f, v) in [
        ("style_fp_fields", b.style_fp_fields),
        ("style_fp_text", b.style_fp_text),
        ("style_fp_shapes", b.style_fp_shapes),
        ("style_fp_dimensions", b.style_fp_dimensions),
        ("style_fp_barcodes", b.style_fp_barcodes),
        ("has_stackup", b.has_stackup),
        ("use_height_for_length_calcs", b.use_height_for_length_calcs),
    ] {
        emit!(o, format!("{p}{f}"), v_bool(v));
    }

    emit!(o, format!("{p}aux_origin"), v_vec(b.aux_origin()));
    emit!(o, format!("{p}grid_origin"), v_vec(b.grid_origin()));
    emit!(
        o,
        format!("{p}track_width_index"),
        v_int(b.track_width_index)
    );
    emit!(o, format!("{p}via_size_index"), v_int(b.via_size_index));
    emit!(o, format!("{p}diff_pair_index"), v_int(b.diff_pair_index));
    emit!(o, format!("{p}via_stack_index"), v_int(b.via_stack_index));
    emit!(
        o,
        format!("{p}custom_track_width"),
        v_int(b.custom_track_width)
    );
    emit!(
        o,
        format!("{p}custom_via_size"),
        format!("{}/{}", b.custom_via_size.diameter, b.custom_via_size.drill)
    );
    emit!(
        o,
        format!("{p}copper_layer_count"),
        v_int(b.copper_layer_count())
    );
    emit!(
        o,
        format!("{p}user_defined_layer_count"),
        v_int(b.user_defined_layer_count())
    );
    emit!(o, format!("{p}enabled_layers"), v_lset(b.enabled_layers()));
    emit!(o, format!("{p}board_thickness"), v_int(b.board_thickness()));
    emit!(
        o,
        format!("{p}current_netclass_name"),
        v_str(&b.current_net_class_name)
    );
    emit!(
        o,
        format!("{p}biggest_clearance"),
        v_int(b.get_biggest_clearance_value(None))
    );
    emit!(
        o,
        format!("{p}smallest_clearance"),
        v_int(b.get_smallest_clearance_value())
    );
    emit!(o, format!("{p}drc_epsilon"), v_int(b.get_drc_epsilon()));
    emit!(
        o,
        format!("{p}hole_plating_thickness"),
        v_int(b.get_hole_plating_thickness())
    );

    dump_zone_settings(
        o,
        &format!("{p}default_zone_settings."),
        b.default_zone_settings(),
    );
    dump_stackup(o, &format!("{p}stackup."), b.stackup_descriptor());

    // m_NetSettings is always set in the Rust model (a C++ BDS built with a null parent has
    // none: that case is handled by the caller).
    if let Some(ns) = super::defaults::net_settings_of(b) {
        dump_netclass(o, &format!("{p}netclass.Default."), ns.default_netclass());

        for (name, nc) in &ns.netclasses {
            dump_netclass(o, &format!("{p}netclass.{name}."), nc);
        }

        for (i, (matcher, nc_name)) in ns.netclass_pattern_assignments.iter().enumerate() {
            emit!(
                o,
                format!("{p}netclass_pattern.{}", 2 * i + 1),
                format!("{} {}", v_str(matcher.pattern()), v_str(nc_name))
            );
        }

        for (net, classes) in &ns.netclass_label_assignments {
            emit!(
                o,
                format!("{p}netclass_label.{net}"),
                join_ints(classes.iter().map(|c| v_str(c)))
            );
        }
    }
}

// ---- board section ------------------------------------------------------------------------

/// `DumpBoardSection`.
pub(crate) fn dump_board_section(o: &mut Out, ctx: &BoardCtx<'_>) {
    let b = ctx.board;
    emit!(
        o,
        "file_format_version",
        v_int(b.file_format_version_at_load)
    );
    emit!(o, "generator", v_str(&b.generator));
    emit!(o, "board_use", v_enum(b.board_use));
    emit!(o, "copper_layer_count", v_int(b.copper_layer_count()));
    emit!(o, "enabled_layers", v_lset(&b.enabled_layers()));
    emit!(o, "enabled_layer_names", v_layer_names(&b.enabled_layers()));
    emit!(o, "visible_layers", no_api("BOARD::GetVisibleLayers"));
    emit!(o, "has_project", v_bool(b.project.is_some()));

    if let Some(ls) = o.cond("layer.*", || b.enabled_layers()) {
        for l in ls.seq().iter().copied() {
            emit!(
                o,
                format!("layer.{}.name", v_layer(l)),
                v_str(&b.layer_name(l))
            );
            emit!(
                o,
                format!("layer.{}.type", v_layer(l)),
                format!("LT_{}", v_enum(b.layer_type(l)))
            );
        }
    }

    let page = &b.paper;
    emit!(o, "page.type", v_str(page.type_as_string()));
    emit!(
        o,
        "page.size_mils",
        format!("{},{}", v_f64(page.width_mils()), v_f64(page.height_mils()))
    );
    emit!(o, "page.portrait", v_bool(page.is_portrait()));

    let tb = &b.titles;
    emit!(o, "title_block.title", v_str(tb.title()));
    emit!(o, "title_block.date", v_str(tb.date()));
    emit!(o, "title_block.revision", v_str(tb.revision()));
    emit!(o, "title_block.company", v_str(tb.company()));

    for i in 0..9 {
        emit!(o, format!("title_block.comment.{i}"), v_str(tb.comment(i)));
    }

    for (k, v) in &b.properties {
        emit!(o, format!("property.{k}"), v_str(v));
    }

    dump_design_settings(o, "ds.", b.design_settings());
    dump_plot_params(o, "plot.", &b.plot_options);

    for net in b.nets() {
        let np = format!("net.{}.", net.net_code().0);
        emit!(o, format!("{np}name"), v_str(net.netname()));
        emit!(o, format!("{np}short_name"), v_str(net.short_netname()));
        emit!(
            o,
            format!("{np}netclass"),
            net.netclass()
                .map_or_else(|| "null".to_string(), |n| v_str(&n.get_name()))
        );
        emit!(o, format!("{np}net_chain"), v_str(net.net_chain()));
    }

    o.guard("effective_netclass", |o| {
        let Some(ns) = super::defaults::net_settings_of(b.design_settings()) else {
            return;
        };
        // GetEffectiveNetClass caches (non-const in C++): run it on a copy.
        let mut ns = ns.clone();

        for net in b.nets() {
            emit!(
                o,
                format!("effective_netclass.{}", net.net_code().0),
                v_str(&ns.get_effective_netclass(net.netname()).get_name())
            );
        }

        for (name, nc) in &ns.composite_netclasses {
            dump_netclass(o, &format!("composite_netclass.{name}."), nc);
        }
    });

    if let Some(s) = o.cond("stackup_or_default.*", || b.get_stackup_or_default()) {
        dump_stackup(o, "stackup_or_default.", &s);
    }

    emit!(o, "bbox", v_box(b.compute_bounding_box(ctx, false, false)));
    emit!(
        o,
        "bbox_edges",
        v_box(b.compute_bounding_box(ctx, true, false))
    );
    emit!(
        o,
        "bbox_edges_physical",
        v_box(b.compute_bounding_box(ctx, true, true))
    );
    emit!(
        o,
        "bbox_physical",
        v_box(b.compute_bounding_box(ctx, false, true))
    );

    for (name, infer, arcs) in [
        ("outline", false, false),
        ("outline_inferred", true, false),
        ("outline_arcs", false, true),
    ] {
        o.guard(&format!("{name}.ok|{name}"), |o| {
            let (outline, ok) = b.get_board_polygon_outlines(ctx, infer, None, arcs, false);
            emit!(o, format!("{name}.ok"), v_bool(ok));
            emit!(o, name, v_polyset(&outline));
        });
    }

    emit!(o, "item_count", v_int(board_items(b).len() as i64));
}

// ---- order section ------------------------------------------------------------------------

type Cmp = fn(&BoardCtx<'_>, ItemId, ItemId) -> Ordering;

/// `emitOrder`: the items sorted into a `std::set` with `cmp` (equal items dropped, the first
/// inserted one kept).
fn emit_order(o: &mut Out, ctx: &BoardCtx<'_>, field: &str, items: Vec<ItemId>, cmp: Option<Cmp>) {
    let n = items.len();
    let set = super::format::attempt(|| {
        let Some(cmp) = cmp else {
            no_api(&format!("set comparator of {field}"))
        };
        let mut v = items;
        v.sort_by(|a, b| cmp(ctx, *a, *b));
        v.dedup_by(|a, b| cmp(ctx, *b, *a) == Ordering::Equal);
        v
    });

    match set {
        Ok(v) => {
            let keys: Vec<String> = v.iter().map(|id| o.item_key(Some(*id))).collect();
            emit!(o, field, format!("[{}]", keys.join(" ")));

            if v.len() != n {
                emit!(o, format!("{field}.dropped"), v_int((n - v.len()) as i64));
            }
        }
        Err(f) => o.fail(field, f),
    }
}

/// `PCB_TRACK::cmp_tracks` on item handles (both are tracks here).
fn track_cmp(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    match (a.typed::<crate::pcb_track::Track>(), b.typed::<crate::pcb_track::Track>()) {
        (Some(ta), Some(tb)) => crate::pcb_track::pcb_track_cmp_tracks(ctx, ta, tb),
        _ => a.cmp(&b),
    }
}

/// `DumpOrderSection`: the sorted containers of `PCB_IO_KICAD_SEXPR::format`.
pub(crate) fn dump_order_section(o: &mut Out, ctx: &BoardCtx<'_>) {
    use crate::board::compare::board_cmp_drawings;
    use crate::board_item::board_item_ptr_cmp;
    use crate::footprint::compare::{
        footprint_cmp_drawings, footprint_cmp_pads, footprint_cmp_zones,
    };
    use crate::pcb_point::pcb_point_cmp_points;

    let b = ctx.board;
    let ids = |v: Vec<ItemId>| v;
    emit_order(
        o,
        ctx,
        "footprints",
        ids(b.footprints().map(|(i, _)| i.into()).collect()),
        Some(board_item_ptr_cmp),
    );
    emit_order(
        o,
        ctx,
        "drawings",
        b.drawings().map(|(i, _)| i).collect(),
        Some(board_cmp_drawings),
    );
    emit_order(
        o,
        ctx,
        "tracks",
        b.tracks().map(|(i, _)| i.into()).collect(),
        Some(track_cmp),
    );
    emit_order(
        o,
        ctx,
        "points",
        b.board_points().map(|(i, _)| i.into()).collect(),
        Some(pcb_point_cmp_points),
    );
    emit_order(
        o,
        ctx,
        "zones",
        b.zones().map(|(i, _)| i.into()).collect(),
        Some(board_item_ptr_cmp),
    );
    emit_order(
        o,
        ctx,
        "groups",
        b.groups().map(|(i, _)| i.into()).collect(),
        Some(board_item_ptr_cmp),
    );
    emit_order(
        o,
        ctx,
        "generators",
        b.generators().map(|(i, _)| i.into()).collect(),
        Some(board_item_ptr_cmp),
    );

    for (fp, _) in b.footprints() {
        let p = format!("fp.{}.", o.item_key(Some(fp.into())));
        emit_order(
            o,
            ctx,
            &format!("{p}pads"),
            b.footprint_pads(fp).map(|(i, _)| i.into()).collect(),
            Some(footprint_cmp_pads),
        );
        emit_order(
            o,
            ctx,
            &format!("{p}drawings"),
            b.footprint_drawings(fp).map(|(i, _)| i).collect(),
            Some(footprint_cmp_drawings),
        );
        emit_order(
            o,
            ctx,
            &format!("{p}points"),
            b.footprint_points(fp).map(|(i, _)| i.into()).collect(),
            Some(pcb_point_cmp_points),
        );
        emit_order(
            o,
            ctx,
            &format!("{p}zones"),
            b.footprint_zones(fp).map(|(i, _)| i.into()).collect(),
            Some(footprint_cmp_zones),
        );
        // PCB_GROUP::ptr_cmp has no Rust port yet.
        emit_order(
            o,
            ctx,
            &format!("{p}groups"),
            b.footprint_groups(fp).map(|(i, _)| i.into()).collect(),
            None,
        );
    }
}

#[allow(dead_code)]
fn _unused(_: PcbLayerId) {}
