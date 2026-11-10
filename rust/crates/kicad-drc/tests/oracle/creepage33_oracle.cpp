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

/**
 * creepage33-oracle: the creepage solve of the reference build (work package DRC-33,
 * pcbnew/drc/drc_creepage_utils.cpp part 2, drc_creepage_engine.cpp and
 * drc_test_provider_creepage.cpp).
 *
 *   creepage33-oracle board FILE...    the legacy provider pipeline per net pair and layer
 *   creepage33-oracle engine FILE...   CREEPAGE_ENGINE::SolveNetPairWholeBoard per net pair
 *
 * `board` loads each board like KI_TEST::LoadBoard and runs DRC_TEST_PROVIDER_CREEPAGE's
 * legacy testCreepage (the one graph truncated between pairs) with the provider's own
 * statements, but prints every solve instead of reporting: `S|netA|netB|layer|target|dist|n|
 * item1|item2|start|end` (dist as the 16 hex digits of the double; n the path length; items
 * by UUID, `-` for none), then the path connections `C|k|n1|n2|weight` (node positions).
 * `engine` prints the same for SolveNetPairWholeBoard( netA, netB, F_Cu / B_Cu, 50 mm ) of
 * every net pair (`E|...`, no line when it returns nothing).
 */

#include <cinttypes>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
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
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <core/kicad_algo.h>
#include <netinfo.h>
#include <pcb_shape.h>
#include <pcb_track.h>
#include <drc/drc_engine.h>
#include <drc/drc_rule.h>
#include <drc/drc_creepage_utils.h>
#include <drc/drc_creepage_engine.h>
#include <length_delay_calculation/length_delay_calculation.h>
#include <wildcards_and_files_ext.h>


static void ignoreAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
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


/// KI_TEST::LoadBoard without the Boost.Test macros (see rust/tools/pcb-dump/main.cpp).
static std::unique_ptr<BOARD> loadBoardQa( const wxString& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        fn( aPath );
    fn.MakeAbsolute();
    wxFileName projectFile( fn );
    projectFile.SetExt( FILEEXT::ProjectFileExtension );
    wxFileName rulesFile( fn );
    rulesFile.SetExt( FILEEXT::DesignRulesFileExtension );

    bool hasProject = false;

    if( projectFile.Exists() )
    {
        mgr.LoadProject( projectFile.GetFullPath() );
        hasProject = true;
    }

    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( fn.GetFullPath().ToStdString() );

    if( !board )
        return board;

    if( hasProject )
        board->SetProject( &mgr.Prj() );

    auto drcEngine = std::make_shared<DRC_ENGINE>( board.get(), &board->GetDesignSettings() );

    try
    {
        if( rulesFile.Exists() )
            drcEngine->InitEngine( rulesFile );
        else
            drcEngine->InitEngine( wxFileName() );
    }
    catch( ... )
    {
    }

    board->GetDesignSettings().m_DRCEngine = drcEngine;
    board->BuildListOfNets();
    board->BuildConnectivity();
    board->GetLengthCalculation()->SynchronizeTuningProfileProperties();

    if( board->GetProject() )
    {
        std::unordered_set<wxString> dummy;
        board->SynchronizeComponentClasses( dummy );
    }

    return board;
}




static std::string hx( double aValue )
{
    uint64_t bits;
    memcpy( &bits, &aValue, sizeof( bits ) );
    char buf[32];
    snprintf( buf, sizeof( buf ), "%016" PRIx64, bits );
    return buf;
}


static std::string pt( const VECTOR2I& aP )
{
    return std::to_string( aP.x ) + "," + std::to_string( aP.y );
}


static std::string uuid( const BOARD_ITEM* aItem )
{
    return aItem ? std::string( aItem->m_Uuid.AsString().ToUTF8() ) : std::string( "-" );
}


static void printSolve( const char* aTag, int aNetA, int aNetB, PCB_LAYER_ID aLayer,
                        double aTarget, double aDistance,
                        const std::vector<std::shared_ptr<GRAPH_CONNECTION>>& aPath )
{
    const BOARD_ITEM* item1 = nullptr;
    const BOARD_ITEM* item2 = nullptr;
    std::string       start = "-", end = "-";

    if( aPath.size() >= 4 )
    {
        std::shared_ptr<GRAPH_CONNECTION> gc1 = aPath[1];
        std::shared_ptr<GRAPH_CONNECTION> gc2 = aPath[aPath.size() - 2];
        item1 = gc1->n1 && gc1->n1->m_parent ? gc1->n1->m_parent->GetParent() : nullptr;
        item2 = gc2->n2 && gc2->n2->m_parent ? gc2->n2->m_parent->GetParent() : nullptr;
        start = pt( VECTOR2I( gc1->m_path.a2 ) );
        end = pt( VECTOR2I( gc2->m_path.a2 ) );
    }

    printf( "%s|%d|%d|%d|%s|%s|%zu|%s|%s|%s|%s\n", aTag, aNetA, aNetB, (int) aLayer,
            hx( aTarget ).c_str(), hx( aDistance ).c_str(), aPath.size(), uuid( item1 ).c_str(),
            uuid( item2 ).c_str(), start.c_str(), end.c_str() );

    for( size_t k = 0; k < aPath.size(); ++k )
    {
        const std::shared_ptr<GRAPH_CONNECTION>& gc = aPath[k];
        printf( "C|%zu|%s|%s|%s\n", k, pt( gc->n1->m_pos ).c_str(), pt( gc->n2->m_pos ).c_str(),
                hx( gc->m_path.weight ).c_str() );
    }
}


/// DRC_TEST_PROVIDER_CREEPAGE::placeholderConstraint.
static DRC_CONSTRAINT placeholderConstraint( BOARD& aBoard, int aNetCodeA, int aNetCodeB,
                                             PCB_LAYER_ID aLayer )
{
    PCB_TRACK bci1( &aBoard );
    PCB_TRACK bci2( &aBoard );
    bci1.SetNetCode( aNetCodeA );
    bci2.SetNetCode( aNetCodeB );
    bci1.SetLayer( aLayer );
    bci2.SetLayer( aLayer );

    return aBoard.GetDesignSettings().m_DRCEngine->EvalRules( CREEPAGE_CONSTRAINT, &bci1, &bci2,
                                                             aLayer );
}


/// DRC_TEST_PROVIDER_CREEPAGE::testCreepage( CREEPAGE_GRAPH&, ... ) up to the report.
static int testCreepage( BOARD& aBoard, CREEPAGE_GRAPH& aGraph, int aNetCodeA, int aNetCodeB,
                         PCB_LAYER_ID aLayer, double aMaxCreepage, bool aHasConditional )
{
    DRC_CONSTRAINT netConstraint;
    double         target;

    if( aHasConditional )
    {
        target = aMaxCreepage;
    }
    else
    {
        netConstraint = placeholderConstraint( aBoard, aNetCodeA, aNetCodeB, aLayer );
        target = netConstraint.Value().Min();
    }

    aGraph.SetTarget( target );

    if( target <= 0 )
        return 0;

    NETINFO_ITEM* netA = aBoard.FindNet( aNetCodeA );
    NETINFO_ITEM* netB = aBoard.FindNet( aNetCodeB );

    if( !netA || !netB )
        return 0;

    if( netA->GetBoundingBox().Distance( netB->GetBoundingBox() ) > target )
        return 0;

    std::shared_ptr<GRAPH_NODE> NetA = aGraph.AddNetElements( aNetCodeA, aLayer, target );
    std::shared_ptr<GRAPH_NODE> NetB = aGraph.AddNetElements( aNetCodeB, aLayer, target );

    aGraph.GeneratePaths( target, aLayer );

    std::vector<std::shared_ptr<GRAPH_NODE>> temp_nodes;

    std::copy_if( aGraph.m_nodes.begin(), aGraph.m_nodes.end(), std::back_inserter( temp_nodes ),
                  []( std::shared_ptr<GRAPH_NODE> aNode )
                  {
                      return !!aNode && aNode->m_parent && !aNode->m_parent->IsConductive()
                             && !aNode->m_connectDirectly && aNode->m_type == GRAPH_NODE::POINT;
                  } );

    alg::for_all_pairs( temp_nodes.begin(), temp_nodes.end(),
                        [&]( std::shared_ptr<GRAPH_NODE> aN1, std::shared_ptr<GRAPH_NODE> aN2 )
                        {
                            if( aN1 == aN2 )
                                return;

                            if( !aN1 || !aN2 )
                                return;

                            if( !( aN1->m_parent ) || !( aN2->m_parent ) )
                                return;

                            if( ( aN1->m_parent ) != ( aN2->m_parent ) )
                                return;

                            aN1->m_parent->ConnectChildren( aN1, aN2, aGraph );
                        } );

    std::vector<std::shared_ptr<GRAPH_CONNECTION>> shortestPath;
    double distance = aGraph.Solve( NetA, NetB, shortestPath );

    printSolve( "S", aNetCodeA, aNetCodeB, aLayer, target, distance, shortestPath );
    return 1;
}


/// DRC_TEST_PROVIDER_CREEPAGE::testCreepage() (legacy path, default advanced config).
static void runBoard( BOARD& aBoard )
{
    DRC_ENGINE* engine = aBoard.GetDesignSettings().m_DRCEngine.get();

    if( !engine->HasRulesForConstraintType( CREEPAGE_CONSTRAINT ) )
    {
        printf( "none\n" );
        return;
    }

    std::vector<int> netcodes;

    for( const auto& [code, net] : aBoard.GetNetInfo().NetsByNetcode() )
        netcodes.push_back( code );

    DRC_CONSTRAINT worst;
    double         maxConstraint = 0;

    if( engine->QueryWorstConstraint( CREEPAGE_CONSTRAINT, worst ) )
        maxConstraint = worst.Value().Min();

    printf( "M|%s\n", hx( maxConstraint ).c_str() );

    if( maxConstraint <= 0 )
        return;

    bool hasConditional = engine->HasConditionalConstraint( CREEPAGE_CONSTRAINT );

    SHAPE_POLY_SET outline;
    bool hasValidOutline = aBoard.GetBoardPolygonOutlines( outline, false, nullptr, false, true );

    CREEPAGE_GRAPH graph( aBoard );
    graph.m_minGrooveWidth = 0;
    graph.m_boardOutline = hasValidOutline ? &outline : nullptr;

    BuildCreepageBoardEdges( aBoard, graph.m_boardEdge, graph.m_ownedBoardEdges, nullptr );
    graph.TransformEdgeToCreepShapes();
    graph.RemoveDuplicatedShapes();
    graph.TransformCreepShapesToNodes( graph.m_shapeCollection );

    graph.GeneratePaths( maxConstraint, Edge_Cuts );

    int  beNodeSize = graph.m_nodes.size();
    int  beConnectionsSize = graph.m_connections.size();
    bool prevTestChangedGraph = false;

    LSET layers = aBoard.GetLayerSet();

    alg::for_all_pairs( netcodes.begin(), netcodes.end(),
                        [&]( int aNet1, int aNet2 )
                        {
                            if( aNet1 == aNet2 )
                                return;

                            for( auto it = layers.copper_layers_begin();
                                 it != layers.copper_layers_end(); ++it )
                            {
                                PCB_LAYER_ID layer = *it;

                                if( prevTestChangedGraph )
                                    graph.TruncateToPrefix( beNodeSize, beConnectionsSize );

                                prevTestChangedGraph = testCreepage( aBoard, graph, aNet1, aNet2,
                                                                     layer, maxConstraint,
                                                                     hasConditional );
                            }
                        } );
}


/// SolveNetPairWholeBoard over every net pair (codes > 0) on F_Cu and B_Cu, 50 mm.
static void runEngine( BOARD& aBoard )
{
    std::vector<int> netcodes;

    for( const auto& [code, net] : aBoard.GetNetInfo().NetsByNetcode() )
    {
        if( net && code > 0 )
            netcodes.push_back( code );
    }

    CREEPAGE_ENGINE engine( aBoard );

    for( PCB_LAYER_ID layer : { F_Cu, B_Cu } )
    {
        for( size_t i = 0; i < netcodes.size(); ++i )
        {
            for( size_t j = i + 1; j < netcodes.size(); ++j )
            {
                std::optional<CREEPAGE_RESULT> r = engine.SolveNetPairWholeBoard(
                        netcodes[i], netcodes[j], layer, 50000000 );

                if( !r )
                    continue;

                printf( "E|%d|%d|%d|%s|%s|%s|%s|%s|%zu\n", netcodes[i], netcodes[j], (int) layer,
                        hx( r->m_distance ).c_str(), uuid( r->m_itemA ).c_str(),
                        uuid( r->m_itemB ).c_str(), pt( r->m_start ).c_str(),
                        pt( r->m_end ).c_str(), r->m_path.size() );
            }
        }
    }
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 3 )
    {
        fprintf( stderr, "usage: creepage33-oracle board|engine FILE...\n" );
        return 2;
    }

    if( !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &ignoreAssert );
    KIID::SeedGenerator( 1 );

    bool engineMode = !strcmp( argv[1], "engine" );

    for( int i = 2; i < argc; ++i )
    {
        wxString file = wxString::FromUTF8( argv[i] );
        printf( "file|%s\n", std::string( wxFileName( file ).GetFullName().ToUTF8() ).c_str() );
        fflush( stdout );

        std::unique_ptr<BOARD> board;

        try
        {
            board = loadBoardQa( file );
        }
        catch( ... )
        {
        }

        if( !board )
        {
            printf( "error|load\n" );
            continue;
        }

        if( engineMode )
            runEngine( *board );
        else
            runBoard( *board );

        fflush( stdout );
    }

    return 0;
}
