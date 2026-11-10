/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * Copyright The KiCad Developers, see AUTHORS.txt for contributors.
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

// Package SET-4 differential oracle: loads each given .kicad_pro with the real SETTINGS_MANAGER /
// PROJECT_FILE, binds a fresh BOARD_DESIGN_SETTINGS to it (SetParent + LoadFromFile, as
// BOARD::SetProject does), prints every member the JSON parameters and migrations touch, then
// saves (NESTED_SETTINGS::SaveToFile, PROJECT_FILE::SerializeToString) and prints the
// board.design_settings subtree of the written project. The Rust test
// project::board_design_settings_json::tests::oracle_* replays the same files.
//
// Output per file: "file|<label>" then, with --full, the lines; without, one
// "hash|<fnv1a64 of the main lines>|<fnv1a64 of the exclusion lines>|<count>" line.
// Build and run: tests/oracle/bds_json_golden.sh

#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/wx_utils/unit_test_utils.h>

#include <board_design_settings.h>
#include <drc/drc_exclusion.h>
#include <project.h>
#include <project/project_file.h>
#include <router/pns_meander.h>
#include <settings/bom_settings.h>
#include <widgets/ui_common.h>
#include <json_common.h>


static std::vector<std::string> g_main;
static std::vector<std::string> g_excl;


static void L( const std::string& k, const std::string& v )
{
    g_main.push_back( k + "=" + v );
}


static void X( const std::string& k, const std::string& v )
{
    g_excl.push_back( k + "=" + v );
}


static std::string I( long long v )
{
    return std::to_string( v );
}


static std::string B( bool v )
{
    return v ? "1" : "0";
}


static std::string D( double v )
{
    return nlohmann::json( v ).dump();
}


static std::string S( const wxString& s )
{
    return nlohmann::json( std::string( s.ToUTF8() ) ).dump();
}


static uint64_t fnv( const std::vector<std::string>& lines )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( const std::string& l : lines )
    {
        for( unsigned char c : l + "\n" )
        {
            h ^= c;
            h *= 0x100000001b3ULL;
        }
    }

    return h;
}


static void meander( const std::string& p, const PNS::MEANDER_SETTINGS& m )
{
    L( p, I( m.m_minAmplitude ) + "|" + I( m.m_maxAmplitude ) + "|" + I( m.m_spacing ) + "|"
                  + B( m.m_cornerStyle == PNS::MEANDER_STYLE_CHAMFER ) + "|"
                  + I( m.m_cornerRadiusPercentage ) + "|" + B( m.m_singleSided ) );
}


static void printBds( const BOARD_DESIGN_SETTINGS& b )
{
    L( "use_height_for_length_calcs", B( b.m_UseHeightForLengthCalcs ) );
    L( "min_clearance", I( b.m_MinClearance ) );
    L( "min_conn", I( b.m_MinConn ) );
    L( "track_min_width", I( b.m_TrackMinWidth ) );
    L( "vias_min_annular_width", I( b.m_ViasMinAnnularWidth ) );
    L( "vias_min_size", I( b.m_ViasMinSize ) );
    L( "min_through_drill", I( b.m_MinThroughDrill ) );
    L( "micro_vias_min_size", I( b.m_MicroViasMinSize ) );
    L( "micro_vias_min_drill", I( b.m_MicroViasMinDrill ) );
    L( "hole_to_hole_min", I( b.m_HoleToHoleMin ) );
    L( "hole_clearance", I( b.m_HoleClearance ) );
    L( "silk_clearance", I( b.m_SilkClearance ) );
    L( "min_groove_width", I( b.m_MinGrooveWidth ) );
    L( "min_resolved_spokes", I( b.m_MinResolvedSpokes ) );
    L( "min_silk_text_height", I( b.m_MinSilkTextHeight ) );
    L( "min_silk_text_thickness", I( b.m_MinSilkTextThickness ) );
    L( "copper_edge_clearance", I( b.m_CopperEdgeClearance ) );

    for( const auto& [code, sev] : b.m_DRCSeverities )
        L( "severity." + I( code ), std::string( SeverityToString( sev ).ToUTF8() ) );

    X( "exclusions", I( b.m_DrcExclusions.size() ) );

    for( const DRC_EXCLUSION& ex : b.m_DrcExclusions )
    {
        nlohmann::json j = ex;
        X( "exclusion", j.dump() + " " + S( ex.GetComment() ) );
    }

    std::string s;

    for( int w : b.m_TrackWidthList )
        s += I( w ) + ",";

    L( "track_widths", s );
    s.clear();

    for( const VIA_DIMENSION& v : b.m_ViasDimensionsList )
        s += I( v.m_Diameter ) + "/" + I( v.m_Drill ) + ",";

    L( "via_dimensions", s );
    s.clear();

    for( const DIFF_PAIR_DIMENSION& d : b.m_DiffPairDimensionsList )
        s += I( d.m_Width ) + "/" + I( d.m_Gap ) + "/" + I( d.m_ViaGap ) + ",";

    L( "diff_pairs", s );

    for( size_t i = 0; i < b.m_ViaStackPresets.size(); i++ )
    {
        const VIA_STACK_PRESET& v = b.m_ViaStackPresets[i];
        L( "via_stack_preset." + I( i ),
           S( v.m_Name ) + "|" + I( v.m_StartLayer ) + "|" + I( v.m_EndLayer ) + "|" + B( v.m_Staggered )
                   + "|" + I( v.m_ViaSize ) + "|" + I( v.m_ViaDrill ) + "|" + B( v.m_UseNetclass ) + "|"
                   + B( v.m_Filled ) + "|" + B( v.m_Capped ) + "|" + I( v.m_Pitch ) );
    }

    const TEARDROP_PARAMETERS_LIST& tdl = b.m_TeardropParamsList;
    L( "teardrop.targets", B( tdl.m_TargetVias ) + B( tdl.m_TargetPTHPads ) + B( tdl.m_TargetSMDPads )
                                   + B( tdl.m_TargetTrack2Track ) + B( tdl.m_UseRoundShapesOnly ) );

    for( int i = 0; i < 3; i++ )
    {
        const TEARDROP_PARAMETERS* t =
                const_cast<TEARDROP_PARAMETERS_LIST&>( tdl ).GetParameters( (TARGET_TD) i );
        L( "teardrop." + I( i ), I( t->m_TdMaxLen ) + "|" + I( t->m_TdMaxWidth ) + "|"
                                         + D( t->m_BestLengthRatio ) + "|" + D( t->m_BestWidthRatio ) + "|"
                                         + D( t->m_WidthtoSizeFilterRatio ) + "|" + B( t->m_CurvedEdges )
                                         + "|" + B( t->m_Enabled ) + "|" + B( t->m_AllowUseTwoTracks ) + "|"
                                         + B( t->m_TdOnPadsInZones ) );
    }

    meander( "meander.0", b.m_SingleTrackMeanderSettings );
    meander( "meander.1", b.m_DiffPairMeanderSettings );
    meander( "meander.2", b.m_SkewMeanderSettings );

    for( int c = 0; c < LAYER_CLASS_COUNT; c++ )
    {
        L( "layer_class." + I( c ),
           I( b.m_LineThickness[c] ) + "|" + I( b.m_TextSize[c].x ) + "|" + I( b.m_TextSize[c].y ) + "|"
                   + I( b.m_TextThickness[c] ) + "|" + B( b.m_TextItalic[c] ) + "|" + B( b.m_TextUpright[c] ) );
    }

    L( "dimension", I( (int) b.m_DimensionUnitsMode ) + "|" + I( (int) b.m_DimensionPrecision ) + "|"
                            + I( (int) b.m_DimensionUnitsFormat ) + "|" + B( b.m_DimensionSuppressZeroes ) + "|"
                            + I( (int) b.m_DimensionTextPosition ) + "|" + B( b.m_DimensionKeepTextAligned )
                            + "|" + I( b.m_DimensionArrowLength ) + "|" + I( b.m_DimensionExtensionOffset ) );
    L( "style_fp", B( b.m_StyleFPFields ) + B( b.m_StyleFPText ) + B( b.m_StyleFPShapes )
                           + B( b.m_StyleFPDimensions ) + B( b.m_StyleFPBarcodes ) );

    const ZONE_SETTINGS& z = const_cast<BOARD_DESIGN_SETTINGS&>( b ).GetDefaultZoneSettings();
    L( "zone.clearance", I( z.m_ZoneClearance ) );
    L( "zone.min_thickness", I( z.m_ZoneMinThickness ) );
    L( "zone.fill_mode", I( (int) z.m_FillMode ) );
    L( "zone.hatch", I( z.m_HatchThickness ) + "|" + I( z.m_HatchGap ) + "|"
                             + D( z.m_HatchOrientation.AsDegrees() ) + "|" + I( z.m_HatchSmoothingLevel ) + "|"
                             + D( z.m_HatchSmoothingValue ) );
    L( "zone.border", I( (int) z.m_ZoneBorderDisplayStyle ) + "|" + I( z.m_BorderHatchPitch ) );
    L( "zone.thermal", I( z.m_ThermalReliefGap ) + "|" + I( z.m_ThermalReliefSpokeWidth ) );
    L( "zone.pad_connection", I( (int) z.GetPadConnection() ) );
    L( "zone.corner", I( (int) z.GetCornerSmoothingType() ) + "|" + I( z.GetCornerRadius() ) );
    L( "zone.islands", I( (int) z.GetIslandRemovalMode() ) + "|" + I( z.GetMinIslandArea() ) );

    L( "max_error", I( b.m_MaxError ) );
    L( "solder_mask_to_copper_clearance", I( b.m_SolderMaskToCopperClearance ) );
    L( "zone_keep_external_fillets", B( b.m_ZoneKeepExternalFillets ) );
    L( "solder_mask_expansion", I( b.m_SolderMaskExpansion ) );
    L( "solder_mask_min_width", I( b.m_SolderMaskMinWidth ) );
    L( "solder_paste_margin", I( b.m_SolderPasteMargin ) );
    L( "solder_paste_margin_ratio", D( b.m_SolderPasteMarginRatio ) );

    L( "bom.export_file_name", S( b.m_BomExportFileName ) );
    L( "bom.settings", nlohmann::json( b.m_BomSettings ).dump() + "|" + B( b.m_BomSettings.readOnly ) );
    L( "bom.presets", nlohmann::json( b.m_BomPresets ).dump() );
    L( "bom.fmt_settings", nlohmann::json( b.m_BomFmtSettings ).dump() + "|" + B( b.m_BomFmtSettings.readOnly ) );
    L( "bom.fmt_presets", nlohmann::json( b.m_BomFmtPresets ).dump() );
}


static void dumpStored( const std::string& aSerialized )
{
    // PROJECT_FILE::SerializeToString returns "" when formatFileContents throws.
    if( aSerialized.empty() )
    {
        L( "stored", "error" );
        return;
    }

    nlohmann::json js = nlohmann::json::parse( aSerialized );
    nlohmann::json::json_pointer ptr( "/board/design_settings" );

    if( !js.contains( ptr ) )
    {
        L( "stored", "none" );
        return;
    }

    nlohmann::json ds = js.at( ptr );

    if( ds.is_object() && ds.contains( "drc_exclusions" ) )
    {
        X( "stored.drc_exclusions", ds["drc_exclusions"].dump( 2 ) );
        ds.erase( "drc_exclusions" );
    }

    L( "stored", ds.dump( 2 ) );
}


static void runFile( const std::string& aPath, const std::string& aLabel, bool aFull )
{
    g_main.clear();
    g_excl.clear();

    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();

    if( !mgr.LoadProject( wxString::FromUTF8( aPath ), true ) )
    {
        printf( "file|%s\nerror|load\n", aLabel.c_str() );
        return;
    }

    PROJECT_FILE& pf = mgr.Prj().GetProjectFile();

    {
        auto bds = std::make_unique<BOARD_DESIGN_SETTINGS>( nullptr, "board.design_settings" );

        // BOARD::SetProject: SetParent( &projectFile, true ) = AddNestedSettings + LoadFromFile
        bds->SetParent( &pf, false );
        // An exception escaping LoadFromFile (the legacy post-step) is reported, the state
        // reached so far is printed.
        try
        {
            L( "load", B( bds->LoadFromFile() ) );
        }
        catch( ... )
        {
            L( "load", "exception" );
        }

        printBds( *bds );

        L( "saved", B( bds->SaveToFile() ) );
        dumpStored( pf.SerializeToString() );

        bds->SetParent( nullptr, false );
        pf.ReleaseNestedSettings( bds.get() );
    }

    mgr.UnloadProject( &mgr.Prj(), false );

    printf( "file|%s\n", aLabel.c_str() );

    if( aFull )
    {
        for( const std::string& l : g_main )
            printf( "%s\n", l.c_str() );

        for( const std::string& l : g_excl )
            printf( "%s\n", l.c_str() );
    }
    else
    {
        printf( "hash|%016llx|%016llx|%zu\n", (unsigned long long) fnv( g_main ),
                (unsigned long long) fnv( g_excl ), g_main.size() + g_excl.size() );
    }
}


int main( int argc, char** argv )
{
    KI_TEST::SetMockConfigDir();
    SetPgm( new MOCK_PGM_BASE() );
    KIPLATFORM::APP::Init();
    wxApp::SetInstance( new wxAppConsole );

    if( !wxInitialize( argc, argv ) )
        return 1;

    wxLog::EnableLogging( false );
    Pgm().InitPgm( true, true );
    Pgm().GetSettingsManager().RegisterSettings( new PCBNEW_SETTINGS, false );
    Pgm().GetSettingsManager().Load();

    bool full = false;

    // Arguments: [--full] then pairs "<path> <label>".
    int i = 1;

    if( i < argc && std::string( argv[i] ) == "--full" )
    {
        full = true;
        i++;
    }

    for( ; i + 1 < argc; i += 2 )
        runFile( argv[i], argv[i + 1], full );

    return 0;
}
