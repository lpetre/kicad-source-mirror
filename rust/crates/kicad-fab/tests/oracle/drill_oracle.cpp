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
 * Differential oracle for kicad-fab FAB-01 (drill model, tool list, drill file names).
 *
 * Linked against the real pcbnew objects of the reference build (see build_oracle.sh). For
 * every board (loaded like kicad-cli: BOARD_LOADER::Load) and for synthetic boards built in
 * code it prints, for every drill span of EnumerateDrillSpans plus the NPTH pseudo span and
 * every (merge, npth) combination:
 *
 *   raw|...      the EnumerateDrillOperations result (the query of buildHolesList), unsorted
 *   sorted|...   m_holeListBuffer after buildHolesList, with m_holeToolReferences
 *   tool|...     m_toolListBuffer
 *   ffs|...      BuildFileFunctionAttributeString for every TYPE_FILE, NC and Gerber form
 *   name|...     getDrillFileName with the drl and gbr extensions
 *   prot|...     getProtectionFileName for every IPC-4761 feature (per span)
 *
 * The Rust test tests/drill_oracle.rs replays the raw lists through the port and compares.
 * Line format: see tests/drill_oracle.rs (fields separated by '|').
 */

#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/filename.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <project.h>
#include <pcbnew_settings.h>
#include <qa_utils/mock_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <board_loader.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <pcb_io/pcb_io_mgr.h>
#include <wildcards_and_files_ext.h>

#include <drill/drill_enumerator.h>
#include <exporters/gendrill_writer_base.h>


/// Exposes the protected members of the base writer.
class ORACLE_WRITER : public GENDRILL_WRITER_BASE
{
public:
    ORACLE_WRITER( BOARD* aPcb ) : GENDRILL_WRITER_BASE( aPcb ) {}

    using GENDRILL_WRITER_BASE::buildHolesList;
    using GENDRILL_WRITER_BASE::getDrillFileName;
    using GENDRILL_WRITER_BASE::getProtectionFileName;
    using GENDRILL_WRITER_BASE::BuildFileFunctionAttributeString;
    using GENDRILL_WRITER_BASE::m_holeListBuffer;
    using GENDRILL_WRITER_BASE::m_holeToolReferences;
    using GENDRILL_WRITER_BASE::m_toolListBuffer;
    using GENDRILL_WRITER_BASE::m_drillFileExtension;
    using GENDRILL_WRITER_BASE::m_merge_PTH_NPTH;
};


static std::string fmtOpt( const std::optional<int>& aVal )
{
    return aVal ? std::to_string( *aVal ) : std::string( "-" );
}


static std::string fmtOp( const DRILL_OPERATION& op )
{
    char src = '-';

    if( dynamic_cast<PCB_VIA*>( op.m_SourceItem ) )
        src = 'V';
    else if( dynamic_cast<PAD*>( op.m_SourceItem ) )
        src = 'P';

    char buf[1024];
    snprintf( buf, sizeof( buf ),
              "%d|%c|%s|%d|%d|%d|%d|%.17g|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|"
              "%d|%d|%d|%d|%d|%d|%d|%d|%s",
              (int) op.m_Kind, src, op.m_SourceId.AsStdString().c_str(), op.m_Position.x,
              op.m_Position.y, op.m_SizeXY.x, op.m_SizeXY.y, op.m_Orientation.AsDegrees(),
              op.m_Diameter, (int) op.m_IsSlot, (int) op.m_TopLayer, (int) op.m_BottomLayer,
              (int) op.m_DrillStart, (int) op.m_DrillEnd, (int) op.m_NotPlated,
              (int) op.m_Attribute, (int) op.m_Filled, (int) op.m_Capped, (int) op.m_TopCovered,
              (int) op.m_BottomCovered, (int) op.m_TopPlugged, (int) op.m_BottomPlugged,
              (int) op.m_TopTented, (int) op.m_BottomTented,
              (int) op.m_FrontPostMachining.m_Mode, op.m_FrontPostMachining.m_Size,
              op.m_FrontPostMachining.m_Depth, op.m_FrontPostMachining.m_Angle,
              (int) op.m_BackPostMachining.m_Mode, op.m_BackPostMachining.m_Size,
              op.m_BackPostMachining.m_Depth, op.m_BackPostMachining.m_Angle,
              fmtOpt( op.m_StubLength ).c_str() );
    return buf;
}


static void dumpBoard( BOARD* aBoard, const std::string& aName )
{
    ORACLE_WRITER w( aBoard );

    printf( "board|%s|%d|%s\n", aName.c_str(), aBoard->GetCopperLayerCount(),
            wxFileName( aBoard->GetFileName() ).GetFullName().ToStdString().c_str() );

    std::vector<DRILL_SPAN> spans = EnumerateDrillSpans( *aBoard );
    spans.emplace_back( F_Cu, B_Cu, false, true );

    for( size_t si = 0; si < spans.size(); si++ )
    {
        const DRILL_SPAN& span = spans[si];
        printf( "span|%zu|%d|%d|%d|%d\n", si, (int) span.m_StartLayer, (int) span.m_EndLayer,
                (int) span.m_IsBackdrill, (int) span.m_IsNonPlatedFile );

        for( int f = 0; f < 8; f++ )
        {
            for( const char* ext : { "drl", "gbr" } )
            {
                w.m_drillFileExtension = ext;
                printf( "prot|%d|%s|%s\n", f, ext,
                        w.getProtectionFileName( span, (IPC4761_FEATURES) f ).ToStdString().c_str() );
            }
        }

        for( int merge = 0; merge < 2; merge++ )
        {
            for( int npth = 0; npth < 2; npth++ )
            {
                printf( "case|%zu|%d|%d\n", si, merge, npth );

                DRILL_QUERY query;
                query.m_Span = span;
                query.m_NonPlatedOnly = npth;
                query.m_MergePTHNPTH = merge;

                for( const DRILL_OPERATION& op : EnumerateDrillOperations( *aBoard, query ) )
                    printf( "raw|%s\n", fmtOp( op ).c_str() );

                w.m_merge_PTH_NPTH = merge;
                w.buildHolesList( span, npth );

                for( size_t ii = 0; ii < w.m_holeListBuffer.size(); ii++ )
                {
                    printf( "sorted|%d|%s\n", w.m_holeToolReferences[ii],
                            fmtOp( w.m_holeListBuffer[ii] ).c_str() );
                }

                for( const DRILL_TOOL& t : w.m_toolListBuffer )
                {
                    printf( "tool|%d|%d|%d|%d|%d|%d|%d|%s|%s\n", t.m_Diameter, t.m_TotalCount,
                            t.m_OvalCount, (int) t.m_Hole_NotPlated, (int) t.m_HoleAttribute,
                            (int) t.m_IsBackdrill, (int) t.m_HasPostMachining,
                            fmtOpt( t.m_MinStubLength ).c_str(),
                            fmtOpt( t.m_MaxStubLength ).c_str() );
                }

                for( int type = 0; type < 3; type++ )
                {
                    for( int compat = 0; compat < 2; compat++ )
                    {
                        printf( "ffs|%d|%d|%s\n", type, compat,
                                w.BuildFileFunctionAttributeString(
                                         span, (GENDRILL_WRITER_BASE::TYPE_FILE) type, compat )
                                        .ToStdString().c_str() );
                    }
                }

                for( const char* ext : { "drl", "gbr" } )
                {
                    w.m_drillFileExtension = ext;
                    printf( "name|%s|%s\n", ext,
                            w.getDrillFileName( span, npth, merge ).ToStdString().c_str() );
                }
            }
        }
    }

    printf( "end|%s\n", aName.c_str() );
}


/// Deterministic generator for the synthetic boards (numerical recipes LCG).
struct LCG
{
    uint32_t s;
    uint32_t next() { s = s * 1664525u + 1013904223u; return s >> 8; }
    int      pick( int n ) { return (int) ( next() % (uint32_t) n ); }
};


/**
 * A board with many coincident holes (same plating, diameter, attribute and position but
 * different UUIDs / post-machining) so that the libstdc++ tie order of std::sort shows, plus
 * every via type, backdrills, post-machining, protection modes and pad variants.
 */
static std::unique_ptr<BOARD> makeSynthetic( const std::string& aName, int aCopper, uint32_t aSeed,
                                             int aVias, int aPads )
{
    KIID::SeedGenerator( aSeed );
    LCG rng{ aSeed };

    auto board = std::make_unique<BOARD>();
    board->SetFileName( wxString::FromUTF8( "/synthetic/" + aName + ".kicad_pcb" ) );
    board->SetCopperLayerCount( aCopper );
    BOARD_DESIGN_SETTINGS& bds = board->GetDesignSettings();
    bds.GetStackupDescriptor().BuildDefaultStackupList( &bds, aCopper );

    LSEQ cu = LSET::AllCuMask( aCopper ).UIOrder(); // F_Cu, In1 ... , B_Cu
    const int mm = 1000000;

    auto layerAt = [&]( int i ) { return cu[std::min<int>( i, cu.size() - 1 )]; };

    for( int i = 0; i < aVias; i++ )
    {
        PCB_VIA* via = new PCB_VIA( board.get() );
        via->SetPosition( VECTOR2I( ( 1 + rng.pick( 3 ) ) * mm, ( 1 + rng.pick( 3 ) ) * mm ) );
        via->SetWidth( PADSTACK::ALL_LAYERS, mm );
        int drills[] = { 300000, 400000, 400000 };
        via->SetDrill( drills[rng.pick( 3 )] );

        int type = rng.pick( 4 );

        if( type == 0 || aCopper < 4 )
        {
            via->SetViaType( VIATYPE::THROUGH );
            via->SetLayerPair( F_Cu, B_Cu );
        }
        else if( type == 1 )
        {
            via->SetViaType( VIATYPE::BLIND );
            if( rng.pick( 2 ) )
                via->SetLayerPair( F_Cu, layerAt( 2 ) );
            else
                via->SetLayerPair( layerAt( 2 ), B_Cu );
        }
        else if( type == 2 )
        {
            via->SetViaType( VIATYPE::BURIED );
            via->SetLayerPair( layerAt( 1 ), layerAt( 3 ) );
        }
        else
        {
            via->SetViaType( VIATYPE::MICROVIA );
            via->SetLayerPair( F_Cu, layerAt( 1 ) );
        }

        int pm = rng.pick( 4 );

        if( pm == 1 )
        {
            via->SetFrontPostMachining( PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK );
            via->SetFrontPostMachiningSize( 600000 );
            via->SetFrontPostMachiningDepth( 150000 );
            via->SetFrontPostMachiningAngle( 900 );
        }
        else if( pm == 2 )
        {
            via->Padstack().BackPostMachining().mode = PAD_DRILL_POST_MACHINING_MODE::COUNTERBORE;
            via->Padstack().BackPostMachining().size = 700000;
            via->Padstack().BackPostMachining().depth = 200000;
        }

        if( aCopper >= 4 && via->GetViaType() == VIATYPE::THROUGH )
        {
            int bd = rng.pick( 4 );

            if( bd == 1 || bd == 3 )
            {
                via->SetSecondaryDrillSize( VECTOR2I( rng.pick( 2 ) ? 500000 : 600000, 0 ) );
                via->SetSecondaryDrillStartLayer( B_Cu );
                via->SetSecondaryDrillEndLayer( layerAt( 2 ) );
            }

            if( bd == 2 || bd == 3 )
            {
                via->SetTertiaryDrillSize( VECTOR2I( 500000, 500000 ) );
                via->SetTertiaryDrillStartLayer( F_Cu );
                via->SetTertiaryDrillEndLayer( layerAt( 1 ) );
            }
        }

        const TENTING_MODE  tm[] = { TENTING_MODE::FROM_BOARD, TENTING_MODE::TENTED, TENTING_MODE::NOT_TENTED };
        const COVERING_MODE cm[] = { COVERING_MODE::FROM_BOARD, COVERING_MODE::COVERED, COVERING_MODE::NOT_COVERED };
        const PLUGGING_MODE gm[] = { PLUGGING_MODE::FROM_BOARD, PLUGGING_MODE::PLUGGED, PLUGGING_MODE::NOT_PLUGGED };
        const CAPPING_MODE  am[] = { CAPPING_MODE::FROM_BOARD, CAPPING_MODE::CAPPED, CAPPING_MODE::NOT_CAPPED };
        const FILLING_MODE  fm[] = { FILLING_MODE::FROM_BOARD, FILLING_MODE::FILLED, FILLING_MODE::NOT_FILLED };
        via->SetFrontTentingMode( tm[rng.pick( 3 )] );
        via->SetBackTentingMode( tm[rng.pick( 3 )] );
        via->SetFrontCoveringMode( cm[rng.pick( 3 )] );
        via->SetBackCoveringMode( cm[rng.pick( 3 )] );
        via->SetFrontPluggingMode( gm[rng.pick( 3 )] );
        via->SetBackPluggingMode( gm[rng.pick( 3 )] );
        via->SetCappingMode( am[rng.pick( 3 )] );
        via->SetFillingMode( fm[rng.pick( 3 )] );

        board->Add( via, ADD_MODE::APPEND );
    }

    FOOTPRINT* fp = new FOOTPRINT( board.get() );
    board->Add( fp, ADD_MODE::APPEND );

    for( int i = 0; i < aPads; i++ )
    {
        PAD* pad = new PAD( fp );
        const PAD_ATTRIB attrs[] = { PAD_ATTRIB::PTH, PAD_ATTRIB::PTH, PAD_ATTRIB::NPTH, PAD_ATTRIB::SMD };
        PAD_ATTRIB attr = attrs[rng.pick( 4 )];
        pad->SetAttribute( attr );
        pad->SetSize( PADSTACK::ALL_LAYERS, VECTOR2I( 3 * mm, 3 * mm ) );

        if( attr == PAD_ATTRIB::SMD )
        {
            pad->SetLayerSet( PAD::SMDMask() );
            pad->SetDrillSize( VECTOR2I( 0, 0 ) );
        }
        else
        {
            pad->SetLayerSet( attr == PAD_ATTRIB::NPTH ? PAD::UnplatedHoleMask() : PAD::PTHMask() );
            const VECTOR2I sizes[] = { { mm, mm }, { mm, 2 * mm }, { 2 * mm, mm }, { 800000, 800000 } };
            VECTOR2I sz = sizes[rng.pick( 4 )];
            pad->SetDrillSize( sz );
            pad->SetDrillShape( sz.x != sz.y || rng.pick( 2 ) ? PAD_DRILL_SHAPE::OBLONG
                                                              : PAD_DRILL_SHAPE::CIRCLE );
        }

        const PAD_PROP props[] = { PAD_PROP::NONE, PAD_PROP::CASTELLATED, PAD_PROP::PRESSFIT };
        pad->SetProperty( props[rng.pick( 3 )] );
        pad->SetPosition( VECTOR2I( ( 10 + rng.pick( 3 ) ) * mm, ( 1 + rng.pick( 2 ) ) * mm ) );
        const double angles[] = { 0.0, 90.0, 45.0 };
        pad->SetOrientation( EDA_ANGLE( angles[rng.pick( 3 )], DEGREES_T ) );

        if( rng.pick( 3 ) == 0 )
        {
            pad->SetFrontPostMachining( PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK );
            pad->Padstack().FrontPostMachining().size = 1500000;
        }

        fp->Add( pad, ADD_MODE::APPEND );
    }

    return board;
}


static bool initPgm( int argc, char** argv )
{
    KI_TEST::SetMockConfigDir();
    SetPgm( new MOCK_PGM_BASE() );
    KIPLATFORM::APP::Init();
    wxApp::SetInstance( new wxAppConsole );

    if( !wxInitialize( argc, argv ) )
        return false;

    wxLog::EnableLogging( false );
    Pgm().InitPgm( true, true );
    Pgm().GetSettingsManager().RegisterSettings( new PCBNEW_SETTINGS, false );
    Pgm().GetSettingsManager().Load();
    Pgm().GetSettingsManager().LoadProject( "" );
    return true;
}


/// kicad-cli load path (PCBNEW_JOBS_HANDLER::getBoard).
static std::unique_ptr<BOARD> loadBoardCli( const wxString& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        pro( aPath );
    pro.SetExt( FILEEXT::ProjectFileExtension );
    pro.MakeAbsolute();

    PROJECT* project = mgr.GetProject( pro.GetFullPath() );

    if( !project )
    {
        mgr.LoadProject( pro.GetFullPath(), true );
        project = mgr.GetProject( pro.GetFullPath() );
    }

    PCB_IO_MGR::PCB_FILE_T type = PCB_IO_MGR::FindPluginTypeFromBoardPath( aPath, KICTL_KICAD_ONLY );
    return BOARD_LOADER::Load( aPath, type, project );
}


static void noAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &noAssert );

    // Synthetic boards first (independent of the corpus).
    dumpBoard( makeSynthetic( "synthetic6", 6, 12345, 200, 80 ).get(), "synthetic6" );
    dumpBoard( makeSynthetic( "synthetic4", 4, 777, 60, 40 ).get(), "synthetic4" );
    dumpBoard( makeSynthetic( "synthetic2", 2, 99, 40, 30 ).get(), "synthetic2" );
    dumpBoard( makeSynthetic( "synthetic32", 32, 5, 80, 10 ).get(), "synthetic32" );

    for( int i = 1; i < argc; i++ )
    {
        wxString path = wxString::FromUTF8( argv[i] );
        std::unique_ptr<BOARD> board = loadBoardCli( path );

        if( !board )
        {
            printf( "error|load|%s\n", argv[i] );
            continue;
        }

        dumpBoard( board.get(), wxFileName( path ).GetName().ToStdString() );
        Pgm().GetSettingsManager().LoadProject( "" );
    }

    fflush( stdout );
    // Skip static destructors of the mocked program (as pcb-dump does).
    _exit( 0 );
}
