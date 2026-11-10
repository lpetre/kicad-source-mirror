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

/*
 * Differential oracle of work package M6 (board stackup, stackup_predefined_prms, plot params).
 * Linked against the reference pcbnew build by m6_build_oracle.sh. Prints one line per value;
 * doubles as their IEEE bit pattern. The Rust replay is the `m6_oracle` test in
 * src/board_stackup.rs, which builds the same scenarios and must print identical lines.
 */

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <climits>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/wx_utils/wx_assert.h>

#include <board.h>
#include <board_design_settings.h>
#include <board_stackup_manager/board_stackup.h>
#include <board_stackup_manager/stackup_predefined_prms.h>
#include <pcb_plot_params.h>


static std::string bits( double v )
{
    uint64_t u;
    memcpy( &u, &v, 8 );
    char buf[32];
    snprintf( buf, sizeof buf, "%016llx", (unsigned long long) u );
    return buf;
}


static void dumpStackup( const std::string& p, const BOARD_STACKUP& s )
{
    printf( "%s|finish=%s|diel_c=%d|thick_c=%d|edge=%d|plating=%d|count=%d|thickness=%d\n",
            p.c_str(), s.m_FinishType.ToStdString().c_str(), s.m_HasDielectricConstrains,
            s.m_HasThicknessConstrains, (int) s.m_EdgeConnectorConstraints, s.m_EdgePlating,
            s.GetCount(), s.BuildBoardThicknessFromStackup() );

    int i = 0;

    for( const BOARD_STACKUP_ITEM* item : s.GetList() )
    {
        printf( "%s|%d|%d|%d|%s|%s|%d|%d|%d\n", p.c_str(), i, (int) item->GetType(),
                (int) item->GetBrdLayerId(), item->GetLayerName().ToStdString().c_str(),
                item->GetTypeName().ToStdString().c_str(), item->GetDielectricLayerId(),
                item->IsEnabled(), item->GetSublayersCount() );

        for( int sl = 0; sl < item->GetSublayersCount(); ++sl )
        {
            printf( "%s|%d.%d|%d|%d|%s|%s|%s|%s|%s|%d|%d\n", p.c_str(), i, sl,
                    item->GetThickness( sl ), item->IsThicknessLocked( sl ),
                    item->GetMaterial( sl ).ToStdString().c_str(),
                    item->GetColor( sl ).ToStdString().c_str(),
                    bits( item->GetEpsilonR( sl ) ).c_str(),
                    bits( item->GetLossTangent( sl ) ).c_str(),
                    bits( item->GetSpecFreq( sl ) ).c_str(), (int) item->GetDielectricModel( sl ),
                    item->HasMaterialValue( sl ) );
        }

        i++;
    }
}


static void dumpDistances( const std::string& p, const BOARD_STACKUP& s,
                           const std::vector<PCB_LAYER_ID>& aLayers )
{
    for( PCB_LAYER_ID a : aLayers )
    {
        for( PCB_LAYER_ID b : aLayers )
        {
            printf( "%s|dist|%d|%d|%d|%d\n", p.c_str(), (int) a, (int) b,
                    s.GetLayerDistance( a, b ), s.GetDrillDepth( a, b ) );
        }
    }
}


/// Scenario settings: copper count, enabled-layer mask (through SetEnabledLayers), thickness.
struct SCENARIO
{
    const char* name;
    int         cu;
    int         thicknessNm;
    int         techMode;   // 0: AllTechMask, 1: F_SilkS + B_Mask only, 2: none
};


static LSET scenarioMask( const SCENARIO& sc )
{
    LSET mask = LSET::AllCuMask( sc.cu );

    if( sc.techMode == 0 )
        mask |= LSET::AllTechMask();
    else if( sc.techMode == 1 )
        mask |= LSET( { F_SilkS, B_Mask } );

    return mask;
}


static std::vector<PCB_LAYER_ID> distanceLayers( const BOARD_STACKUP& s )
{
    std::vector<PCB_LAYER_ID> layers;

    for( const BOARD_STACKUP_ITEM* item : s.GetList() )
    {
        PCB_LAYER_ID l = item->GetBrdLayerId();

        if( !IsCopperLayer( l ) )
            continue;

        if( s.GetCount() > 40 && !( l == F_Cu || l == B_Cu || l == In1_Cu || l == In2_Cu
                                    || l == In15_Cu || l == In30_Cu ) )
            continue;

        layers.push_back( l );
    }

    return layers;
}


/// Customise a stackup: sublayers, odd thicknesses, a disabled item, names, colours.
static void customise( BOARD_STACKUP& s )
{
    int i = 0;

    for( BOARD_STACKUP_ITEM* item : s.GetList() )
    {
        if( item->GetType() == BS_ITEM_TYPE_DIELECTRIC && item->GetDielectricLayerId() == 2 )
        {
            item->AddDielectricPrms( 1 );
            item->SetThickness( 50001, 1 );
            item->SetMaterial( wxT( "PTFE" ), 1 );
            item->SetEpsilonR( 2.1, 1 );
            item->SetLossTangent( 0.0002, 1 );
            item->SetThicknessLocked( true, 1 );
            item->AddDielectricPrms( 0 );
            item->SetThickness( 33333, 0 );
            item->RemoveDielectricPrms( 5 );     // out of range: ignored
        }

        if( item->GetBrdLayerId() == In1_Cu )
            item->SetThickness( 18001 );

        if( item->GetType() == BS_ITEM_TYPE_SOLDERPASTE )
            item->SetEnabled( false );

        if( item->GetType() == BS_ITEM_TYPE_SOLDERMASK )
        {
            item->SetColor( wxT( "Green" ) );
            item->SetEnabled( i % 2 == 0 );
        }

        item->SetLayerName( wxString::Format( "L%d", i ) );
        i++;
    }
}


static void stackupScenarios()
{
    const SCENARIO scenarios[] = {
        { "cu2", 2, 1600000, 0 },   { "cu4", 4, 1600000, 0 },  { "cu6", 6, 1200000, 0 },
        { "cu32", 32, 1600000, 0 }, { "cu4m", 4, 800000, 1 },  { "cu1", 1, 1600000, 0 },
        { "cu32thin", 32, 100000, 2 }, { "cu8", 8, 1000001, 0 },
    };

    for( const SCENARIO& sc : scenarios )
    {
        BOARD_DESIGN_SETTINGS bds( nullptr, "board.design_settings" );
        bds.SetEnabledLayers( scenarioMask( sc ) );
        bds.SetCopperLayerCount( sc.cu );
        bds.SetBoardThickness( sc.thicknessNm );

        BOARD_STACKUP& src = bds.GetStackupDescriptor();
        src.m_FinishType = wxT( "ENIG" );
        src.m_EdgePlating = true;
        src.m_EdgeConnectorConstraints = BS_EDGE_CONNECTOR_BEVELLED;
        src.m_HasDielectricConstrains = true;
        src.m_HasThicknessConstrains = true;

        BOARD_STACKUP s;
        s.BuildDefaultStackupList( &bds, 2 );
        std::string p = std::string( "default." ) + sc.name;
        dumpStackup( p, s );
        dumpDistances( p, s, distanceLayers( s ) );

        // BuildDefaultStackupList appends to an existing list.
        if( sc.cu == 2 )
        {
            s.BuildDefaultStackupList( &bds, 0 );
            dumpStackup( p + ".again", s );
        }

        BOARD_STACKUP c = s;
        customise( c );
        dumpStackup( p + ".custom", c );
        dumpDistances( p + ".custom", c, distanceLayers( c ) );

        // Synchronise the customised stackup with other copper counts.
        for( int newCu : { sc.cu, 2, 6 } )
        {
            if( newCu == sc.cu && sc.cu >= 32 )
                continue;

            BOARD_DESIGN_SETTINGS bds2( nullptr, "board.design_settings" );
            SCENARIO sc2 = sc;
            sc2.cu = newCu;
            bds2.SetEnabledLayers( scenarioMask( sc2 ) );
            bds2.SetCopperLayerCount( newCu );
            bds2.SetBoardThickness( sc.thicknessNm );
            bds2.GetStackupDescriptor().m_FinishType = wxT( "OSP" );
            bds2.GetStackupDescriptor().m_EdgeConnectorConstraints = BS_EDGE_CONNECTOR_IN_USE;

            BOARD_STACKUP sy = c;
            bool changed = sy.SynchronizeWithBoard( &bds2 );
            std::string sp = p + ".sync" + std::to_string( newCu );
            printf( "%s|changed=%d\n", sp.c_str(), changed );
            dumpStackup( sp, sy );
        }
    }

    for( int n : { 0, 2, 4, 6 } )
    {
        BOARD_STACKUP s;
        s.BuildDefaultStackupList( nullptr, n );
        std::string p = "null." + std::to_string( n );
        dumpStackup( p, s );
    }

    // Allowed layers.
    std::string allowed;

    for( PCB_LAYER_ID l : BOARD_STACKUP::StackupAllowedBrdLayers().Seq() )
        allowed += std::to_string( (int) l ) + ",";

    printf( "allowed|%s\n", allowed.c_str() );
}


static void predefinedScenarios()
{
    for( const wxString& s : GetStandardCopperFinishes( false ) )
        printf( "finish|%s\n", s.ToStdString().c_str() );

    for( BOARD_STACKUP_ITEM_TYPE t : { BS_ITEM_TYPE_UNDEFINED, BS_ITEM_TYPE_COPPER,
                                       BS_ITEM_TYPE_DIELECTRIC, BS_ITEM_TYPE_SOLDERPASTE,
                                       BS_ITEM_TYPE_SOLDERMASK, BS_ITEM_TYPE_SILKSCREEN } )
    {
        const std::vector<FAB_LAYER_COLOR>& list = GetStandardColors( t );
        printf( "colors|%d|n=%d|user=%d\n", (int) t, (int) list.size(),
                GetColorUserDefinedListIdx( t ) );

        for( int i = 0; i < (int) list.size(); i++ )
        {
            KIGFX::COLOR4D c = GetStandardColor( t, i );
            printf( "colors|%d|%d|%s|%s|%s|%s|%s|%s|%s|custom=%d\n", (int) t, i,
                    GetStandardColorName( t, i ).ToStdString().c_str(),
                    list[i].GetColorAsString().ToStdString().c_str(), bits( c.r ).c_str(),
                    bits( c.g ).c_str(), bits( c.b ).c_str(), bits( c.a ).c_str(),
                    bits( GetDefaultUserColor( t ).a ).c_str(), IsCustomColorIdx( t, i ) );
        }
    }

    for( const char* s : { "", "Not specified", "not SPECIFIED", "FR4", "Not specified ",
                           "Green", "green", "GREEN", "Purple", "R1G2B3" } )
    {
        printf( "prm|%s|spec=%d|norm=%d\n", s, IsPrmSpecified( wxString( s ) ),
                IsColorNameNormalized( wxString( s ) ) );
    }

    for( BOARD_STACKUP_ITEM_TYPE t : { BS_ITEM_TYPE_UNDEFINED, BS_ITEM_TYPE_COPPER,
                                       BS_ITEM_TYPE_DIELECTRIC, BS_ITEM_TYPE_SOLDERPASTE,
                                       BS_ITEM_TYPE_SOLDERMASK, BS_ITEM_TYPE_SILKSCREEN } )
    {
        BOARD_STACKUP_ITEM it( t );
        it.SetDielectricLayerId( 7 );
        printf( "flags|%d|eps=%d|lt=%d|sf=%d|mat_ed=%d|col_ed=%d|thk_ed=%d|%s\n", (int) t,
                it.HasEpsilonRValue(), it.HasLossTangentValue(), it.HasSpecFreqValue(),
                it.IsMaterialEditable(), it.IsColorEditable(), it.IsThicknessEditable(),
                it.FormatDielectricLayerName().ToStdString().c_str() );
    }

    // Formatting of epsilon R / loss tangent.
    BOARD_STACKUP_ITEM item( BS_ITEM_TYPE_DIELECTRIC );

    for( double v : { 4.5, 0.02, 3.3, 1e-5, 0.0001, 0.00012, 12.345678901, 1e9, 0.0, -0.5,
                      1.0 / 3.0, 123456789.123 } )
    {
        item.SetEpsilonR( v );
        item.SetLossTangent( v );
        printf( "fmt|%s|%s|%s\n", bits( v ).c_str(), item.FormatEpsilonR( 0 ).ToStdString().c_str(),
                item.FormatLossTangent( 0 ).ToStdString().c_str() );
    }
}


/// One modification of a PCB_PLOT_PARAMS; index i selects it.
static bool modify( PCB_PLOT_PARAMS& p, int i )
{
    switch( i )
    {
    case 0: p.SetLayerSelection( LSET( { F_Cu } ) ); return true;
    case 1: p.SetPlotOnAllLayersSequence( LSEQ( { F_Cu } ) ); return true;
    case 2: p.SetUseGerberProtelExtensions( true ); return true;
    case 3: p.SetDisableGerberMacros( true ); return true;
    case 4: p.SetUseGerberX2format( false ); return true;
    case 5: p.SetIncludeGerberNetlistInfo( false ); return true;
    case 6: p.SetCreateGerberJobFile( false ); return true;
    case 7: p.SetGerberPrecision( 5 ); return true;
    case 8: p.SetDashedLineDashRatio( 5.0 ); return true;
    case 9: p.SetDashedLineGapRatio( 5.0 ); return true;
    case 10: p.SetPlotFrameRef( true ); return true;
    case 11: p.SetDXFPlotMode( SKETCH ); return true;
    case 12: p.SetDXFPlotPolygonMode( false ); return true;
    case 13: p.SetDXFPlotUnits( DXF_UNITS::MM ); return true;
    case 14: p.SetDXFMultiLayeredExportOption( true ); return true;
    case 15: p.SetSvgPrecision( 3 ); return true;
    case 16: p.SetPngDPI( 600 ); return true;
    case 17: p.SetPngAntialias( false ); return true;
    case 18: p.SetUseAuxOrigin( true ); return true;
    case 19: p.SetNegative( true ); return true;
    case 20: p.SetMirror( true ); return true;
    case 21: p.m_PDFFrontFPPropertyPopups = false; return true;
    case 22: p.m_PDFBackFPPropertyPopups = false; return true;
    case 23: p.m_PDFMetadata = false; return true;
    case 24: p.SetPlotReference( false ); return true;
    case 25: p.SetPlotValue( false ); return true;
    case 26: p.SetPlotFPText( false ); return true;
    case 27: p.SetSketchPadsOnFabLayers( true ); return true;
    case 28: p.SetPlotPadNumbers( true ); return true;
    case 29: p.SetHideDNPFPsOnFabLayers( true ); return true;
    case 30: p.SetSketchDNPFPsOnFabLayers( false ); return true;
    case 31: p.SetCrossoutDNPFPsOnFabLayers( false ); return true;
    case 32: p.SetSubtractMaskFromSilk( true ); return true;
    case 33: p.SetDrillMarksType( DRILL_MARKS::NO_DRILL_SHAPE ); return true;
    case 34: p.SetScaleSelection( 3 ); return true;
    case 35: p.SetAutoScale( true ); return true;
    case 36: p.SetScale( 2.0 ); return true;
    case 37: p.SetA4Output( true ); return true;
    case 38: p.SetFineScaleAdjustX( 0.5 ); return true;
    case 39: p.SetFineScaleAdjustY( 0.5 ); return true;
    case 40: p.SetWidthAdjust( 7 ); return true;
    case 41: p.SetTextMode( PLOT_TEXT_MODE::STROKE ); return true;
    case 42: p.SetBlackAndWhite( false ); return true;
    case 43: p.SetOutputDirectory( wxT( "out" ) ); return true;
    case 44: p.SetSkipPlotNPTH_Pads( true ); return true;           // not compared
    case 45: p.SetSketchPadLineWidth( 5 ); return true;             // not compared
    case 46: p.m_PDFSingle = true; return true;                     // not compared
    case 47: p.SetSvgFitPageToBoard( true ); return true;           // not compared
    case 48: p.SetLayer( B_Cu ); return true;                       // not compared
    default: return false;
    }
}


static void plotParamsScenarios()
{
    for( int prec : { -1, 0, 4, 5, 6, 7 } )
    {
        PCB_PLOT_PARAMS p;
        p.SetGerberPrecision( prec );
        printf( "gbrprec|%d|%d\n", prec, p.GetGerberPrecision() );
    }

    for( unsigned prec : { 0u, 2u, 3u, 4u, 6u, 7u, UINT_MAX } )
    {
        PCB_PLOT_PARAMS p;
        p.SetSvgPrecision( prec );
        printf( "svgprec|%u|%u\n", prec, p.GetSvgPrecision() );
    }

    const PLOT_FORMAT formats[] = { PLOT_FORMAT::HPGL, PLOT_FORMAT::GERBER, PLOT_FORMAT::POST,
                                    PLOT_FORMAT::DXF,  PLOT_FORMAT::PDF,    PLOT_FORMAT::SVG };

    for( PLOT_FORMAT f : formats )
    {
        std::string line = "same|" + std::to_string( (int) f ) + "|";

        for( int i = 0;; i++ )
        {
            PCB_PLOT_PARAMS a;
            a.SetFormat( f );
            PCB_PLOT_PARAMS b = a;

            if( !modify( b, i ) )
                break;

            line += ( a.IsSameAs( b ) ? "1" : "0" );
            line += ( b.IsSameAs( a ) ? "1" : "0" );
        }

        PCB_PLOT_PARAMS a;
        a.SetFormat( f );
        PCB_PLOT_PARAMS b;
        printf( "%s|%d|%d\n", line.c_str(), a.IsSameAs( a ), a.IsSameAs( b ) );
    }
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

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
    Pgm().GetSettingsManager().LoadProject( "" );

    stackupScenarios();
    predefinedScenarios();
    plotParamsScenarios();
    return 0;
}
