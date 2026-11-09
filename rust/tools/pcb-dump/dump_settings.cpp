/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the
 * Free Software Foundation, either version 3 of the License, or (at your
 * option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

// pcb-dump: settings classes, the "board" section and the "order" section.

#include "pcb_dump.h"
#include "dump_common.h"

#include <board.h>
#include <board_design_settings.h>
#include <board_stackup_manager/board_stackup.h>
#include <footprint.h>
#include <netclass.h>
#include <netinfo.h>
#include <pad.h>
#include <pcb_group.h>
#include <pcb_plot_params.h>
#include <pcb_point.h>
#include <pcb_track.h>
#include <project/net_settings.h>
#include <router/pns_meander.h>
#include <teardrop/teardrop_parameters.h>
#include <title_block.h>
#include <zone.h>
#include <zone_settings.h>
#include <pcb_dimension.h>
#include <pcb_generator.h>
#include <drc/drc_item.h>
#include <widgets/report_severity.h>
#include <geometry/shape_poly_set.h>


template <typename T>
static std::string VMinOptMax( const MINOPTMAX<T>& m )
{
    return V( m.Min() ) + "," + V( m.Opt() ) + "," + V( m.Max() );
}


static void dumpMeander( const std::string& p, const PNS::MEANDER_SETTINGS& m )
{
    EMIT( p + "min_amplitude", V( m.m_minAmplitude ) );
    EMIT( p + "max_amplitude", V( m.m_maxAmplitude ) );
    EMIT( p + "spacing", V( m.m_spacing ) );
    EMIT( p + "step", V( m.m_step ) );
    EMIT( p + "len_pad_to_die", V( m.m_lenPadToDie ) );
    EMIT( p + "signal_extra_length", V( m.m_signalExtraLength ) );
    EMIT( p + "signal_extra_delay", V( m.m_signalExtraDelay ) );
    EMIT( p + "target_length", VMinOptMax( m.m_targetLength ) );
    EMIT( p + "target_length_delay", VMinOptMax( m.m_targetLengthDelay ) );
    EMIT( p + "target_signal_length", VMinOptMax( m.m_targetSignalLength ) );
    EMIT( p + "target_signal_length_delay", VMinOptMax( m.m_targetSignalLengthDelay ) );
    EMIT( p + "target_skew", VMinOptMax( m.m_targetSkew ) );
    EMIT( p + "target_skew_delay", VMinOptMax( m.m_targetSkewDelay ) );
    EMIT( p + "override_custom_rules", V( m.m_overrideCustomRules ) );
    EMIT( p + "corner_style", VEnum( m.m_cornerStyle ) );
    EMIT( p + "corner_radius_percentage", V( m.m_cornerRadiusPercentage ) );
    EMIT( p + "single_sided", V( m.m_singleSided ) );
    EMIT( p + "initial_side", VEnum( m.m_initialSide ) );
    EMIT( p + "length_tolerance", V( m.m_lengthTolerance ) );
    EMIT( p + "keep_endpoints", V( m.m_keepEndpoints ) );
    EMIT( p + "is_time_domain", V( m.m_isTimeDomain ) );
}


void DumpTeardropParams( const std::string& p, const TEARDROP_PARAMETERS& t )
{
    EMIT( p + "max_len", V( t.m_TdMaxLen ) );
    EMIT( p + "max_width", V( t.m_TdMaxWidth ) );
    EMIT( p + "best_length_ratio", V( t.m_BestLengthRatio ) );
    EMIT( p + "best_width_ratio", V( t.m_BestWidthRatio ) );
    EMIT( p + "width_to_size_filter_ratio", V( t.m_WidthtoSizeFilterRatio ) );
    EMIT( p + "curved_edges", V( t.m_CurvedEdges ) );
    EMIT( p + "enabled", V( t.m_Enabled ) );
    EMIT( p + "allow_use_two_tracks", V( t.m_AllowUseTwoTracks ) );
    EMIT( p + "td_on_pads_in_zones", V( t.m_TdOnPadsInZones ) );
}


void DumpZoneSettings( const std::string& p, const ZONE_SETTINGS& z )
{
    EMIT( p + "priority", V( z.m_ZonePriority ) );
    EMIT( p + "fill_mode", VEnum( z.m_FillMode ) );
    EMIT( p + "clearance", V( z.m_ZoneClearance ) );
    EMIT( p + "min_thickness", V( z.m_ZoneMinThickness ) );
    EMIT( p + "hatch_thickness", V( z.m_HatchThickness ) );
    EMIT( p + "hatch_gap", V( z.m_HatchGap ) );
    EMIT( p + "hatch_orientation", V( z.m_HatchOrientation ) );
    EMIT( p + "hatch_smoothing_level", V( z.m_HatchSmoothingLevel ) );
    EMIT( p + "hatch_smoothing_value", V( z.m_HatchSmoothingValue ) );
    EMIT( p + "hatch_hole_min_area", V( z.m_HatchHoleMinArea ) );
    EMIT( p + "hatch_border_algorithm", V( z.m_HatchBorderAlgorithm ) );
    EMIT( p + "thieving.pattern", VEnum( z.m_ThievingSettings.pattern ) );
    EMIT( p + "thieving.element_size", V( z.m_ThievingSettings.element_size ) );
    EMIT( p + "thieving.gap", V( z.m_ThievingSettings.gap ) );
    EMIT( p + "thieving.line_width", V( z.m_ThievingSettings.line_width ) );
    EMIT( p + "thieving.stagger", V( z.m_ThievingSettings.stagger ) );
    EMIT( p + "thieving.orientation", V( z.m_ThievingSettings.orientation.GetAngle() ) );
    EMIT( p + "netcode", V( z.m_Netcode ) );
    EMIT( p + "name", V( z.m_Name ) );
    EMIT( p + "layers", V( z.m_Layers ) );
    EMIT( p + "border_display_style", VEnum( z.m_ZoneBorderDisplayStyle ) );
    EMIT( p + "border_hatch_pitch", V( z.m_BorderHatchPitch ) );
    EMIT( p + "thermal_relief_gap", V( z.m_ThermalReliefGap ) );
    EMIT( p + "thermal_relief_spoke_width", V( z.m_ThermalReliefSpokeWidth ) );
    EMIT( p + "locked", V( z.m_Locked ) );
    EMIT( p + "teardrop_type", VEnum( z.m_TeardropType ) );
    EMIT( p + "corner_smoothing", VEnum( z.GetCornerSmoothingType() ) );
    EMIT( p + "corner_radius", V( z.GetCornerRadius() ) );
    EMIT( p + "pad_connection", VEnum( z.GetPadConnection() ) );
    EMIT( p + "rule_area", V( z.GetIsRuleArea() ) );
    EMIT( p + "placement_area_enabled", V( z.GetPlacementAreaEnabled() ) );
    EMIT( p + "placement_area_source_type", VEnum( z.GetPlacementAreaSourceType() ) );
    EMIT( p + "placement_area_source", V( z.GetPlacementAreaSource() ) );
    EMIT( p + "keepout.zone_fills", V( z.GetDoNotAllowZoneFills() ) );
    EMIT( p + "keepout.vias", V( z.GetDoNotAllowVias() ) );
    EMIT( p + "keepout.tracks", V( z.GetDoNotAllowTracks() ) );
    EMIT( p + "keepout.pads", V( z.GetDoNotAllowPads() ) );
    EMIT( p + "keepout.footprints", V( z.GetDoNotAllowFootprints() ) );
    EMIT( p + "island_removal_mode", VEnum( z.GetIslandRemovalMode() ) );
    EMIT( p + "min_island_area", V( z.GetMinIslandArea() ) );

    for( const auto& [layer, props] : z.m_LayerProperties )
    {
        EMIT( p + "layer_properties." + VLayer( layer ) + ".hatching_offset",
              props.hatching_offset ? V( *props.hatching_offset ) : std::string( "none" ) );
    }
}


static std::string optParent( const NETCLASS* aParent )
{
    return aParent ? V( aParent->GetName() ) : std::string( "null" );
}


void DumpNetclass( const std::string& p, const NETCLASS& nc )
{
    EMIT( p + "name", V( nc.GetName() ) );
    EMIT( p + "is_default", V( nc.IsDefault() ) );
    EMIT( p + "description", V( nc.GetDescription() ) );
    EMIT( p + "priority", V( nc.GetPriority() ) );
    EMIT( p + "clearance", nc.HasClearance() ? V( nc.GetClearance() ) : std::string( "none" ) );
    EMIT( p + "track_width", VOpt( nc.GetTrackWidthOpt() ) );
    EMIT( p + "via_diameter", VOpt( nc.GetViaDiameterOpt() ) );
    EMIT( p + "via_drill", VOpt( nc.GetViaDrillOpt() ) );
    EMIT( p + "uvia_diameter", VOpt( nc.GetuViaDiameterOpt() ) );
    EMIT( p + "uvia_drill", VOpt( nc.GetuViaDrillOpt() ) );
    EMIT( p + "diff_pair_width", VOpt( nc.GetDiffPairWidthOpt() ) );
    EMIT( p + "diff_pair_gap", VOpt( nc.GetDiffPairGapOpt() ) );
    EMIT( p + "diff_pair_via_gap", VOpt( nc.GetDiffPairViaGapOpt() ) );
    EMIT( p + "wire_width", VOpt( nc.GetWireWidthOpt() ) );
    EMIT( p + "bus_width", VOpt( nc.GetBusWidthOpt() ) );
    EMIT( p + "line_style", VOpt( nc.GetLineStyleOpt() ) );
    EMIT( p + "pcb_color", V( nc.GetPcbColor( true ).ToCSSString() ) );
    EMIT( p + "schematic_color", V( nc.GetSchematicColor( true ).ToCSSString() ) );
    EMIT( p + "tuning_profile", V( nc.GetTuningProfile() ) );
    EMIT( p + "track_width_parent", optParent( nc.GetTrackWidthParent() ) );
    EMIT( p + "via_diameter_parent", optParent( nc.GetViaDiameterParent() ) );

    std::string c;

    for( const NETCLASS* n : nc.GetConstituentNetclasses() )
        c += ( c.empty() ? "" : " " ) + V( n->GetName() );

    EMIT( p + "constituents", "[" + c + "]" );
}


void DumpStackup( const std::string& p, const BOARD_STACKUP& s )
{
    EMIT( p + "finish_type", V( s.m_FinishType ) );
    EMIT( p + "has_dielectric_constraints", V( s.m_HasDielectricConstrains ) );
    EMIT( p + "has_thickness_constraints", V( s.m_HasThicknessConstrains ) );
    EMIT( p + "edge_connector_constraints", VEnum( s.m_EdgeConnectorConstraints ) );
    EMIT( p + "edge_plating", V( s.m_EdgePlating ) );
    EMIT( p + "count", V( s.GetCount() ) );
    EMIT( p + "board_thickness", V( s.BuildBoardThicknessFromStackup() ) );

    int i = 0;

    for( BOARD_STACKUP_ITEM* item : s.GetList() )
    {
        const std::string ip = p + "item." + std::to_string( i++ ) + ".";
        EMIT( ip + "type", VEnum( item->GetType() ) );
        EMIT( ip + "layer", VLayer( item->GetBrdLayerId() ) );
        EMIT( ip + "layer_name", V( item->GetLayerName() ) );
        EMIT( ip + "type_name", V( item->GetTypeName() ) );
        EMIT( ip + "dielectric_layer_id", V( item->GetDielectricLayerId() ) );
        EMIT( ip + "enabled", V( item->IsEnabled() ) );
        EMIT( ip + "sublayers", V( item->GetSublayersCount() ) );

        for( int sl = 0; sl < item->GetSublayersCount(); ++sl )
        {
            const std::string sp = ip + "sub." + std::to_string( sl ) + ".";
            EMIT( sp + "thickness", V( item->GetThickness( sl ) ) );
            EMIT( sp + "thickness_locked", V( item->IsThicknessLocked( sl ) ) );
            EMIT( sp + "material", V( item->GetMaterial( sl ) ) );
            EMIT( sp + "color", V( item->GetColor( sl ) ) );
            EMIT( sp + "epsilon_r", V( item->GetEpsilonR( sl ) ) );
            EMIT( sp + "loss_tangent", V( item->GetLossTangent( sl ) ) );
            EMIT( sp + "spec_freq", V( item->GetSpecFreq( sl ) ) );
            EMIT( sp + "dielectric_model", VEnum( item->GetDielectricModel( sl ) ) );
        }
    }
}


void DumpPlotParams( const std::string& p, const PCB_PLOT_PARAMS& pp )
{
    EMIT( p + "text_mode", VEnum( pp.GetTextMode() ) );
    EMIT( p + "dxf_plot_mode", VEnum( pp.GetDXFPlotMode() ) );
    EMIT( p + "plot_pad_numbers", V( pp.GetPlotPadNumbers() ) );
    EMIT( p + "dxf_polygon_mode", V( pp.GetDXFPlotPolygonMode() ) );
    EMIT( p + "dxf_units", VEnum( pp.GetDXFPlotUnits() ) );
    EMIT( p + "drill_marks", VEnum( pp.GetDrillMarksType() ) );
    EMIT( p + "scale", V( pp.GetScale() ) );
    EMIT( p + "fine_scale_adjust", V( pp.GetFineScaleAdjustX() ) + "," + V( pp.GetFineScaleAdjustY() ) );
    EMIT( p + "width_adjust", V( pp.GetWidthAdjust() ) );
    EMIT( p + "auto_scale", V( pp.GetAutoScale() ) );
    EMIT( p + "mirror", V( pp.GetMirror() ) );
    EMIT( p + "sketch_pads_on_fab", V( pp.GetSketchPadsOnFabLayers() ) );
    EMIT( p + "sketch_pad_line_width", V( pp.GetSketchPadLineWidth() ) );
    EMIT( p + "hide_dnp_on_fab", V( pp.GetHideDNPFPsOnFabLayers() ) );
    EMIT( p + "sketch_dnp_on_fab", V( pp.GetSketchDNPFPsOnFabLayers() ) );
    EMIT( p + "crossout_dnp_on_fab", V( pp.GetCrossoutDNPFPsOnFabLayers() ) );
    EMIT( p + "plot_value", V( pp.GetPlotValue() ) );
    EMIT( p + "plot_reference", V( pp.GetPlotReference() ) );
    EMIT( p + "plot_fp_text", V( pp.GetPlotFPText() ) );
    EMIT( p + "negative", V( pp.GetNegative() ) );
    EMIT( p + "legacy_plot_via_on_mask", VOpt( pp.GetLegacyPlotViaOnMaskLayer() ) );
    EMIT( p + "plot_frame_ref", V( pp.GetPlotFrameRef() ) );
    EMIT( p + "format", VEnum( pp.GetFormat() ) );
    EMIT( p + "output_directory", V( pp.GetOutputDirectory() ) );
    EMIT( p + "disable_gerber_macros", V( pp.GetDisableGerberMacros() ) );
    EMIT( p + "use_gerber_x2", V( pp.GetUseGerberX2format() ) );
    EMIT( p + "gerber_netlist_info", V( pp.GetIncludeGerberNetlistInfo() ) );
    EMIT( p + "gerber_job_file", V( pp.GetCreateGerberJobFile() ) );
    EMIT( p + "gerber_protel_ext", V( pp.GetUseGerberProtelExtensions() ) );
    EMIT( p + "gerber_precision", V( pp.GetGerberPrecision() ) );
    EMIT( p + "svg_precision", V( pp.GetSvgPrecision() ) );
    EMIT( p + "svg_fit_page", V( pp.GetSvgFitPagetoBoard() ) );
    EMIT( p + "png_dpi", V( pp.GetPngDPI() ) );
    EMIT( p + "png_antialias", V( pp.GetPngAntialias() ) );
    EMIT( p + "black_and_white", V( pp.GetBlackAndWhite() ) );
    EMIT( p + "subtract_mask_from_silk", V( pp.GetSubtractMaskFromSilk() ) );
    EMIT( p + "layer_selection", V( pp.GetLayerSelection() ) );
    EMIT( p + "plot_on_all_layers", VLayers( pp.GetPlotOnAllLayersSequence() ) );
    EMIT( p + "use_aux_origin", V( pp.GetUseAuxOrigin() ) );
    EMIT( p + "scale_selection", V( pp.GetScaleSelection() ) );
    EMIT( p + "a4_output", V( pp.GetA4Output() ) );
    EMIT( p + "dashed_line_dash_ratio", V( pp.GetDashedLineDashRatio() ) );
    EMIT( p + "dashed_line_gap_ratio", V( pp.GetDashedLineGapRatio() ) );
    EMIT( p + "dxf_multilayered", V( pp.GetDXFMultiLayeredExportOption() ) );
    EMIT( p + "layer", VLayer( pp.GetLayer() ) );
}


void DumpDesignSettings( const std::string& p, const BOARD_DESIGN_SETTINGS& b )
{
    {
        std::string r;

        for( int w : b.m_TrackWidthList )
            r += ( r.empty() ? "" : " " ) + V( w );

        EMIT( p + "track_width_list", "[" + r + "]" );
    }
    {
        std::string r;

        for( const VIA_DIMENSION& v : b.m_ViasDimensionsList )
            r += ( r.empty() ? "" : " " ) + V( v.m_Diameter ) + "/" + V( v.m_Drill );

        EMIT( p + "vias_dimensions_list", "[" + r + "]" );
    }
    {
        std::string r;

        for( const DIFF_PAIR_DIMENSION& d : b.m_DiffPairDimensionsList )
            r += ( r.empty() ? "" : " " ) + V( d.m_Width ) + "/" + V( d.m_Gap ) + "/" + V( d.m_ViaGap );

        EMIT( p + "diff_pair_dimensions_list", "[" + r + "]" );
    }

    int i = 0;

    for( const VIA_STACK_PRESET& v : b.m_ViaStackPresets )
    {
        const std::string vp = p + "via_stack_preset." + std::to_string( i++ ) + ".";
        EMIT( vp + "name", V( v.m_Name ) );
        EMIT( vp + "layers", VLayer( v.m_StartLayer ) + "," + VLayer( v.m_EndLayer ) );
        EMIT( vp + "staggered", V( v.m_Staggered ) );
        EMIT( vp + "size", V( v.m_ViaSize ) + "/" + V( v.m_ViaDrill ) );
        EMIT( vp + "use_netclass", V( v.m_UseNetclass ) );
        EMIT( vp + "filled", V( v.m_Filled ) );
        EMIT( vp + "capped", V( v.m_Capped ) );
        EMIT( vp + "pitch", V( v.m_Pitch ) );
    }

    const TEARDROP_PARAMETERS_LIST& tdl = b.m_TeardropParamsList;
    EMIT( p + "teardrops.target_vias", V( tdl.m_TargetVias ) );
    EMIT( p + "teardrops.target_pth_pads", V( tdl.m_TargetPTHPads ) );
    EMIT( p + "teardrops.target_smd_pads", V( tdl.m_TargetSMDPads ) );
    EMIT( p + "teardrops.target_track_to_track", V( tdl.m_TargetTrack2Track ) );
    EMIT( p + "teardrops.use_round_shapes_only", V( tdl.m_UseRoundShapesOnly ) );

    for( TARGET_TD t : magic_enum::enum_values<TARGET_TD>() )
    {
        if( t == TARGET_COUNT || t == TARGET_UNKNOWN )
            continue;

        DumpTeardropParams( p + "teardrops." + VEnum( t ) + ".",
                            *const_cast<TEARDROP_PARAMETERS_LIST&>( tdl ).GetParameters( t ) );
    }

    dumpMeander( p + "meander.single.", b.m_SingleTrackMeanderSettings );
    dumpMeander( p + "meander.diff_pair.", b.m_DiffPairMeanderSettings );
    dumpMeander( p + "meander.skew.", b.m_SkewMeanderSettings );

    EMIT( p + "current_via_type", VEnum( b.m_CurrentViaType ) );
    EMIT( p + "use_connected_track_width", V( b.m_UseConnectedTrackWidth ) );
    EMIT( p + "temp_override_track_width", V( b.m_TempOverrideTrackWidth ) );
    EMIT( p + "min_clearance", V( b.m_MinClearance ) );
    EMIT( p + "min_groove_width", V( b.m_MinGrooveWidth ) );
    EMIT( p + "min_conn", V( b.m_MinConn ) );
    EMIT( p + "track_min_width", V( b.m_TrackMinWidth ) );
    EMIT( p + "vias_min_annular_width", V( b.m_ViasMinAnnularWidth ) );
    EMIT( p + "vias_min_size", V( b.m_ViasMinSize ) );
    EMIT( p + "min_through_drill", V( b.m_MinThroughDrill ) );
    EMIT( p + "micro_vias_min_size", V( b.m_MicroViasMinSize ) );
    EMIT( p + "micro_vias_min_drill", V( b.m_MicroViasMinDrill ) );
    EMIT( p + "copper_edge_clearance", V( b.m_CopperEdgeClearance ) );
    EMIT( p + "hole_clearance", V( b.m_HoleClearance ) );
    EMIT( p + "hole_to_hole_min", V( b.m_HoleToHoleMin ) );
    EMIT( p + "silk_clearance", V( b.m_SilkClearance ) );
    EMIT( p + "min_resolved_spokes", V( b.m_MinResolvedSpokes ) );
    EMIT( p + "min_silk_text_height", V( b.m_MinSilkTextHeight ) );
    EMIT( p + "min_silk_text_thickness", V( b.m_MinSilkTextThickness ) );

    for( const auto& [code, sev] : b.m_DRCSeverities )
        EMIT( p + "drc_severity." + VEnum( (PCB_DRC_CODE) code ), VEnum( sev ) );

    EMIT( p + "drc_exclusions", V( (int) b.m_DrcExclusions.size() ) );
    EMIT( p + "zone_keep_external_fillets", V( b.m_ZoneKeepExternalFillets ) );
    EMIT( p + "max_error", V( b.m_MaxError ) );
    EMIT( p + "solder_mask_expansion", V( b.m_SolderMaskExpansion ) );
    EMIT( p + "solder_mask_min_width", V( b.m_SolderMaskMinWidth ) );
    EMIT( p + "solder_mask_to_copper_clearance", V( b.m_SolderMaskToCopperClearance ) );
    EMIT( p + "solder_paste_margin", V( b.m_SolderPasteMargin ) );
    EMIT( p + "solder_paste_margin_ratio", V( b.m_SolderPasteMarginRatio ) );
    EMIT( p + "allow_soldermask_bridges_in_fps", V( b.m_AllowSoldermaskBridgesInFPs ) );
    EMIT( p + "tent_vias_front", V( b.m_TentViasFront ) );
    EMIT( p + "tent_vias_back", V( b.m_TentViasBack ) );
    EMIT( p + "cover_vias_front", V( b.m_CoverViasFront ) );
    EMIT( p + "cover_vias_back", V( b.m_CoverViasBack ) );
    EMIT( p + "plug_vias_front", V( b.m_PlugViasFront ) );
    EMIT( p + "plug_vias_back", V( b.m_PlugViasBack ) );
    EMIT( p + "cap_vias", V( b.m_CapVias ) );
    EMIT( p + "fill_vias", V( b.m_FillVias ) );

    i = 0;

    for( const TEXT_ITEM_INFO& t : b.m_DefaultFPTextItems )
    {
        EMIT( p + "default_fp_text." + std::to_string( i++ ),
              V( t.m_Text ) + " " + V( t.m_Visible ) + " " + VLayer( t.m_Layer ) );
    }

    for( const auto& [k, v] : b.m_UserLayerNames )
        EMIT( p + "user_layer_name." + k, V( v ) );

    for( const auto& [layer, props] : b.m_ZoneLayerProperties )
    {
        EMIT( p + "zone_layer_properties." + VLayer( layer ) + ".hatching_offset",
              props.hatching_offset ? V( *props.hatching_offset ) : std::string( "none" ) );
    }

    static const char* classNames[] = { "silk", "copper", "edges", "courtyard", "fab", "other" };

    for( int c = 0; c < LAYER_CLASS_COUNT; ++c )
    {
        const std::string cp = p + "layer_class." + ( c < 6 ? classNames[c] : std::to_string( c ).c_str() ) + ".";
        EMIT( cp + "line_thickness", V( b.m_LineThickness[c] ) );
        EMIT( cp + "text_size", V( b.m_TextSize[c] ) );
        EMIT( cp + "text_thickness", V( b.m_TextThickness[c] ) );
        EMIT( cp + "text_italic", V( b.m_TextItalic[c] ) );
        EMIT( cp + "text_upright", V( b.m_TextUpright[c] ) );
    }

    EMIT( p + "dimension.units_mode", VEnum( b.m_DimensionUnitsMode ) );
    EMIT( p + "dimension.precision", VEnum( b.m_DimensionPrecision ) );
    EMIT( p + "dimension.units_format", VEnum( b.m_DimensionUnitsFormat ) );
    EMIT( p + "dimension.suppress_zeroes", V( b.m_DimensionSuppressZeroes ) );
    EMIT( p + "dimension.text_position", VEnum( b.m_DimensionTextPosition ) );
    EMIT( p + "dimension.keep_text_aligned", V( b.m_DimensionKeepTextAligned ) );
    EMIT( p + "dimension.arrow_length", V( b.m_DimensionArrowLength ) );
    EMIT( p + "dimension.extension_offset", V( b.m_DimensionExtensionOffset ) );
    EMIT( p + "style_fp_fields", V( b.m_StyleFPFields ) );
    EMIT( p + "style_fp_text", V( b.m_StyleFPText ) );
    EMIT( p + "style_fp_shapes", V( b.m_StyleFPShapes ) );
    EMIT( p + "style_fp_dimensions", V( b.m_StyleFPDimensions ) );
    EMIT( p + "style_fp_barcodes", V( b.m_StyleFPBarcodes ) );
    EMIT( p + "has_stackup", V( b.m_HasStackup ) );
    EMIT( p + "use_height_for_length_calcs", V( b.m_UseHeightForLengthCalcs ) );
    EMIT( p + "aux_origin", V( b.GetAuxOrigin() ) );
    EMIT( p + "grid_origin", V( b.GetGridOrigin() ) );
    EMIT( p + "track_width_index", V( b.GetTrackWidthIndex() ) );
    EMIT( p + "via_size_index", V( b.GetViaSizeIndex() ) );
    EMIT( p + "diff_pair_index", V( b.GetDiffPairIndex() ) );
    EMIT( p + "via_stack_index", V( b.GetViaStackIndex() ) );
    EMIT( p + "custom_track_width", V( b.GetCustomTrackWidth() ) );
    EMIT( p + "custom_via_size", V( b.GetCustomViaSize() ) + "/" + V( b.GetCustomViaDrill() ) );
    EMIT( p + "copper_layer_count", V( b.GetCopperLayerCount() ) );
    EMIT( p + "user_defined_layer_count", V( b.GetUserDefinedLayerCount() ) );
    EMIT( p + "enabled_layers", V( b.GetEnabledLayers() ) );
    EMIT( p + "board_thickness", V( b.GetBoardThickness() ) );
    EMIT( p + "current_netclass_name", V( b.GetCurrentNetClassName() ) );
    EMIT( p + "biggest_clearance", V( b.GetBiggestClearanceValue() ) );
    EMIT( p + "smallest_clearance", V( b.GetSmallestClearanceValue() ) );
    EMIT( p + "drc_epsilon", V( b.GetDRCEpsilon() ) );
    EMIT( p + "hole_plating_thickness", V( b.GetHolePlatingThickness() ) );

    DumpZoneSettings( p + "default_zone_settings.", const_cast<BOARD_DESIGN_SETTINGS&>( b ).GetDefaultZoneSettings() );
    DumpStackup( p + "stackup.", b.GetStackupDescriptor() );

    if( b.m_NetSettings )
    {
        DumpNetclass( p + "netclass.Default.", *b.m_NetSettings->GetDefaultNetclass() );

        for( const auto& [name, nc] : b.m_NetSettings->GetNetclasses() )
            DumpNetclass( p + "netclass." + std::string( name.ToUTF8() ) + ".", *nc );

        i = 0;

        for( const auto& [matcher, ncName] : b.m_NetSettings->GetNetclassPatternAssignments() )
        {
            EMIT( p + "netclass_pattern." + std::to_string( i++ ),
                  V( matcher->GetPattern() ) + " " + V( ncName ) );
        }

        for( const auto& [net, classes] : b.m_NetSettings->GetNetclassLabelAssignments() )
        {
            std::string r;

            for( const wxString& c : classes )
                r += ( r.empty() ? "" : " " ) + V( c );

            EMIT( p + "netclass_label." + std::string( net.ToUTF8() ), "[" + r + "]" );
        }
    }
}


// ---- board section ------------------------------------------------------------------------

void DumpBoardSection( BOARD* aBoard )
{
    EMIT( "file_format_version", V( aBoard->GetFileFormatVersionAtLoad() ) );
    EMIT( "generator", V( aBoard->GetGenerator() ) );
    EMIT( "board_use", VEnum( aBoard->GetBoardUse() ) );
    EMIT( "copper_layer_count", V( aBoard->GetCopperLayerCount() ) );
    EMIT( "enabled_layers", V( aBoard->GetEnabledLayers() ) );
    EMIT( "enabled_layer_names", VLayerNames( aBoard->GetEnabledLayers() ) );
    EMIT( "visible_layers", V( aBoard->GetVisibleLayers() ) );
    EMIT( "has_project", V( aBoard->GetProject() != nullptr ) );

    for( PCB_LAYER_ID l : aBoard->GetEnabledLayers().Seq() )
    {
        EMIT( "layer." + VLayer( l ) + ".name", V( aBoard->GetLayerName( l ) ) );
        EMIT( "layer." + VLayer( l ) + ".type", VEnum( aBoard->GetLayerType( l ) ) );
    }

    const PAGE_INFO& page = aBoard->GetPageSettings();
    EMIT( "page.type", V( page.GetTypeAsString() ) );
    EMIT( "page.size_mils", V( page.GetWidthMils() ) + "," + V( page.GetHeightMils() ) );
    EMIT( "page.portrait", V( page.IsPortrait() ) );

    const TITLE_BLOCK& tb = aBoard->GetTitleBlock();
    EMIT( "title_block.title", V( tb.GetTitle() ) );
    EMIT( "title_block.date", V( tb.GetDate() ) );
    EMIT( "title_block.revision", V( tb.GetRevision() ) );
    EMIT( "title_block.company", V( tb.GetCompany() ) );

    for( int i = 0; i < 9; ++i )
        EMIT( "title_block.comment." + std::to_string( i ), V( tb.GetComment( i ) ) );

    for( const auto& [k, v] : aBoard->GetProperties() )
        EMIT( "property." + std::string( k.ToUTF8() ), V( v ) );

    DumpDesignSettings( "ds.", aBoard->GetDesignSettings() );
    DumpPlotParams( "plot.", aBoard->GetPlotOptions() );

    for( NETINFO_ITEM* net : aBoard->GetNetInfo() )
    {
        const std::string np = "net." + std::to_string( net->GetNetCode() ) + ".";
        EMIT( np + "name", V( net->GetNetname() ) );
        EMIT( np + "short_name", V( net->GetShortNetname() ) );
        EMIT( np + "netclass", net->GetNetClass() ? V( net->GetNetClass()->GetName() ) : std::string( "null" ) );
        EMIT( np + "net_chain", V( net->GetNetChain() ) );
    }

    Guard( "effective_netclass", [&]() {
        std::shared_ptr<NET_SETTINGS>& ns = aBoard->GetDesignSettings().m_NetSettings;

        if( !ns )
            return;

        for( NETINFO_ITEM* net : aBoard->GetNetInfo() )
        {
            std::shared_ptr<NETCLASS> nc = ns->GetEffectiveNetClass( net->GetNetname() );
            EMIT( "effective_netclass." + std::to_string( net->GetNetCode() ),
                  nc ? V( nc->GetName() ) : std::string( "null" ) );
        }

        for( const auto& [name, nc] : ns->GetCompositeNetclasses() )
            DumpNetclass( "composite_netclass." + std::string( name.ToUTF8() ) + ".", *nc );
    } );

    DumpStackup( "stackup_or_default.", aBoard->GetStackupOrDefault() );

    EMIT( "bbox", V( aBoard->ComputeBoundingBox( false, false ) ) );
    EMIT( "bbox_edges", V( aBoard->ComputeBoundingBox( true, false ) ) );
    EMIT( "bbox_edges_physical", V( aBoard->ComputeBoundingBox( true, true ) ) );
    EMIT( "bbox_physical", V( aBoard->ComputeBoundingBox( false, true ) ) );

    Guard( "outline", [&]() {
        SHAPE_POLY_SET outline;
        bool ok = aBoard->GetBoardPolygonOutlines( outline, false );
        EMIT( "outline.ok", V( ok ) );
        EMIT( "outline", VPolySet( outline ) );
    } );

    Guard( "outline_inferred", [&]() {
        SHAPE_POLY_SET outline;
        bool ok = aBoard->GetBoardPolygonOutlines( outline, true );
        EMIT( "outline_inferred.ok", V( ok ) );
        EMIT( "outline_inferred", VPolySet( outline ) );
    } );

    Guard( "outline_arcs", [&]() {
        SHAPE_POLY_SET outline;
        bool ok = aBoard->GetBoardPolygonOutlines( outline, false, nullptr, true );
        EMIT( "outline_arcs.ok", V( ok ) );
        EMIT( "outline_arcs", VPolySet( outline ) );
    } );

    int count = 0;
    ForEachBoardItem( aBoard, [&]( BOARD_ITEM* ) { count++; } );
    EMIT( "item_count", V( count ) );
}


// ---- order section ------------------------------------------------------------------------

template <typename SET>
static void emitOrder( const std::string& aField, const SET& aSet, size_t aInputCount )
{
    std::string r;

    for( auto* item : aSet )
        r += ( r.empty() ? "" : " " ) + ItemKey( item );

    EMIT( aField, "[" + r + "]" );

    if( aSet.size() != aInputCount )
        EMIT( aField + ".dropped", V( (int) ( aInputCount - aSet.size() ) ) );
}


/// The sorted containers of PCB_IO_KICAD_SEXPR::format( const BOARD* ) and
/// format( const FOOTPRINT* ), with the same comparators (std::set: equal items are dropped).
void DumpOrderSection( BOARD* b )
{
    emitOrder( "footprints",
               std::set<BOARD_ITEM*, BOARD_ITEM::ptr_cmp>( b->Footprints().begin(), b->Footprints().end() ),
               b->Footprints().size() );
    emitOrder( "drawings",
               std::set<BOARD_ITEM*, BOARD::cmp_drawings>( b->Drawings().begin(), b->Drawings().end() ),
               b->Drawings().size() );
    emitOrder( "tracks",
               std::set<PCB_TRACK*, PCB_TRACK::cmp_tracks>( b->Tracks().begin(), b->Tracks().end() ),
               b->Tracks().size() );
    emitOrder( "points",
               std::set<PCB_POINT*, PCB_POINT::cmp_points>( b->Points().begin(), b->Points().end() ),
               b->Points().size() );
    emitOrder( "zones", std::set<BOARD_ITEM*, BOARD_ITEM::ptr_cmp>( b->Zones().begin(), b->Zones().end() ),
               b->Zones().size() );
    emitOrder( "groups",
               std::set<BOARD_ITEM*, BOARD_ITEM::ptr_cmp>( b->Groups().begin(), b->Groups().end() ),
               b->Groups().size() );
    emitOrder( "generators",
               std::set<BOARD_ITEM*, BOARD_ITEM::ptr_cmp>( b->Generators().begin(), b->Generators().end() ),
               b->Generators().size() );

    for( FOOTPRINT* fp : b->Footprints() )
    {
        const std::string p = "fp." + ItemKey( fp ) + ".";
        emitOrder( p + "pads",
                   std::set<PAD*, FOOTPRINT::cmp_pads>( fp->Pads().begin(), fp->Pads().end() ),
                   fp->Pads().size() );
        emitOrder( p + "drawings",
                   std::set<BOARD_ITEM*, FOOTPRINT::cmp_drawings>( fp->GraphicalItems().begin(),
                                                                   fp->GraphicalItems().end() ),
                   fp->GraphicalItems().size() );
        emitOrder( p + "points",
                   std::set<PCB_POINT*, PCB_POINT::cmp_points>( fp->Points().begin(), fp->Points().end() ),
                   fp->Points().size() );
        emitOrder( p + "zones",
                   std::set<ZONE*, FOOTPRINT::cmp_zones>( fp->Zones().begin(), fp->Zones().end() ),
                   fp->Zones().size() );
        emitOrder( p + "groups",
                   std::set<BOARD_ITEM*, PCB_GROUP::ptr_cmp>( fp->Groups().begin(), fp->Groups().end() ),
                   fp->Groups().size() );
    }
}
