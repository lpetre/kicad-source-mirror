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

/**
 * creepage32-oracle: the creepage graph model of the reference build (work package DRC-32,
 * pcbnew/drc/drc_creepage_utils.h / .cpp part 1).
 *
 *   creepage32-oracle paths [--verbose]   shape-pair paths of generated shapes
 *   creepage32-oracle board FILE...       board-edge graph of boards
 *
 * Doubles are printed as the 16 hex digits of their bit pattern. `paths` prints every
 * generated shape (`S|...`, the inputs) and, per ordered pair and max weight, the number of
 * paths and the FNV-1a 64 hash of their text (`P|i|j|maxW|n|hash`; `--verbose` prints the
 * text instead of the hash). `board` loads each board like KI_TEST::LoadBoard and runs the
 * board-edge part of CREEPAGE_ENGINE::populateBoardEdgeGraph (GetBoardPolygonOutlines,
 * BuildCreepageBoardEdges, TransformEdgeToCreepShapes, RemoveDuplicatedShapes,
 * TransformCreepShapesToNodes), then for two (target, groove width) configurations adds
 * child points on every board-edge circle / arc (`X|...`, inputs), runs the connectChildren
 * pair loop of the engine, prints the connections and their GetShapes, and truncates the
 * graph back to its prefix.
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
#include <pcb_shape.h>
#include <drc/drc_engine.h>
#include <drc/drc_creepage_utils.h>
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
    std::memcpy( &bits, &aValue, sizeof( bits ) );
    char buf[32];
    snprintf( buf, sizeof( buf ), "%016" PRIx64, bits );
    return buf;
}


static std::string pt( const VECTOR2I& aP )
{
    return std::to_string( aP.x ) + "," + std::to_string( aP.y );
}


static uint64_t fnv1a( const std::string& aText )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aText )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


static std::string pathsText( const std::vector<PATH_CONNECTION>& aPaths )
{
    std::string s;

    for( const PATH_CONNECTION& pc : aPaths )
    {
        if( !s.empty() )
            s += ";";

        s += hx( pc.a1.x ) + "," + hx( pc.a1.y ) + "," + hx( pc.a2.x ) + "," + hx( pc.a2.y ) + ","
             + hx( pc.weight );
    }

    return s;
}


// ---- paths ----------------------------------------------------------------------------------

enum KIND { BP = 0, BC, BA, CS, CC, CA };
static const char* KIND_NAMES[] = { "bp", "bc", "ba", "cs", "cc", "ca" };


struct GEN_SHAPE
{
    KIND                         kind;
    std::unique_ptr<CREEP_SHAPE> shape;
    std::string                  text;
};


struct LCG
{
    uint64_t state = 0x2545F4914F6CDD1DULL;

    uint32_t next()
    {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return (uint32_t) ( state >> 33 );
    }

    /// Uniform integer in [lo, hi].
    int range( int lo, int hi ) { return lo + (int) ( next() % (uint32_t) ( hi - lo + 1 ) ); }
};


template <class F>
static auto withShape( GEN_SHAPE& aS, F aF )
{
    switch( aS.kind )
    {
    case BP: return aF( *static_cast<BE_SHAPE_POINT*>( aS.shape.get() ) );
    case BC: return aF( *static_cast<BE_SHAPE_CIRCLE*>( aS.shape.get() ) );
    case BA: return aF( *static_cast<BE_SHAPE_ARC*>( aS.shape.get() ) );
    case CS: return aF( *static_cast<CU_SHAPE_SEGMENT*>( aS.shape.get() ) );
    case CC: return aF( *static_cast<CU_SHAPE_CIRCLE*>( aS.shape.get() ) );
    default: return aF( *static_cast<CU_SHAPE_ARC*>( aS.shape.get() ) );
    }
}


static void addArcAngles( LCG& aRng, int aRadius, const VECTOR2I& aPos, EDA_ANGLE& aStart,
                          EDA_ANGLE& aEnd, VECTOR2I& aStartPt, VECTOR2I& aEndPt, int aSpecial )
{
    int startDeg = aRng.range( -180, 179 );
    int sweep = aRng.range( 10, 350 );

    if( aSpecial == 1 )
    {
        startDeg = 170;     // crossing the +-180 boundary
        sweep = 40;
    }
    else if( aSpecial == 2 )
    {
        startDeg = -30;     // more than 180
        sweep = 250;
    }

    aStart = EDA_ANGLE( (double) startDeg, DEGREES_T );
    aEnd = EDA_ANGLE( (double) ( startDeg + sweep ), DEGREES_T );
    aStartPt = aPos + VECTOR2I( KiROUND( aRadius * aStart.Cos() ), KiROUND( aRadius * aStart.Sin() ) );
    aEndPt = aPos + VECTOR2I( KiROUND( aRadius * aEnd.Cos() ), KiROUND( aRadius * aEnd.Sin() ) );
}


static std::vector<GEN_SHAPE> generateShapes()
{
    LCG                    rng;
    std::vector<GEN_SHAPE> shapes;
    const int              box = 8000000;

    auto pos = [&]()
    {
        return VECTOR2I( rng.range( -box, box ), rng.range( -box, box ) );
    };

    for( int kind = BP; kind <= CA; ++kind )
    {
        for( int i = 0; i < 7; ++i )
        {
            GEN_SHAPE g;
            g.kind = (KIND) kind;
            VECTOR2I p = pos();
            int      special = i >= 5 ? i - 4 : 0;

            // Concentric / coincident specials: reuse the centre of the first shape
            if( i == 4 && !shapes.empty() )
                p = shapes.front().shape->GetPos();

            switch( kind )
            {
            case BP:
                g.shape = std::make_unique<BE_SHAPE_POINT>( p );
                g.text = pt( p );
                break;

            case BC:
            {
                int r = special == 2 ? 0 : rng.range( 50000, 5000000 );
                g.shape = std::make_unique<BE_SHAPE_CIRCLE>( p, r );
                g.text = pt( p ) + "," + std::to_string( r );
                break;
            }

            case BA:
            {
                int       r = rng.range( 50000, 5000000 );
                EDA_ANGLE sa, ea;
                VECTOR2I  sp, ep;
                addArcAngles( rng, r, p, sa, ea, sp, ep, special );
                g.shape = std::make_unique<BE_SHAPE_ARC>( p, r, sa, ea, sp, ep );
                g.text = pt( p ) + "," + std::to_string( r ) + "," + hx( sa.AsDegrees() ) + ","
                         + hx( ea.AsDegrees() ) + "," + pt( sp ) + "," + pt( ep );
                break;
            }

            case CS:
            {
                VECTOR2I e = special == 2 ? p : pos();
                double   w = rng.range( 100000, 2000000 ) + 0.5 * ( i % 2 );
                g.shape = std::make_unique<CU_SHAPE_SEGMENT>( p, e, w );
                g.text = pt( p ) + "," + pt( e ) + "," + hx( w );
                break;
            }

            case CC:
            {
                double r = rng.range( 50000, 3000000 ) + 0.25 * i;
                g.shape = std::make_unique<CU_SHAPE_CIRCLE>( p, r );
                g.text = pt( p ) + "," + hx( r );
                break;
            }

            default:
            {
                double    r = rng.range( 100000, 5000000 ) + 0.5 * ( i % 2 );
                EDA_ANGLE sa, ea;
                VECTOR2I  sp, ep;
                addArcAngles( rng, (int) r, p, sa, ea, sp, ep, special );
                auto arc = std::make_unique<CU_SHAPE_ARC>( p, r, sa, ea, sp, ep );
                int  w = rng.range( 100000, 1500000 );
                arc->SetWidth( w );
                g.text = pt( p ) + "," + hx( r ) + "," + hx( sa.AsDegrees() ) + ","
                         + hx( ea.AsDegrees() ) + "," + pt( sp ) + "," + pt( ep ) + ","
                         + std::to_string( w );
                g.shape = std::move( arc );
                break;
            }
            }

            shapes.push_back( std::move( g ) );
        }
    }

    return shapes;
}


static int runPaths( bool aVerbose )
{
    std::vector<GEN_SHAPE> shapes = generateShapes();

    for( size_t i = 0; i < shapes.size(); ++i )
        printf( "S|%zu|%s|%s\n", i, KIND_NAMES[shapes[i].kind], shapes[i].text.c_str() );

    const double maxWeights[] = { 2000000.0, 25000000.5 };

    for( size_t i = 0; i < shapes.size(); ++i )
    {
        for( size_t j = 0; j < shapes.size(); ++j )
        {
            for( double maxWeight : maxWeights )
            {
                std::vector<PATH_CONNECTION> paths = withShape( shapes[i],
                        [&]( auto& a )
                        {
                            return withShape( shapes[j],
                                    [&]( auto& b )
                                    {
                                        return a.Paths( b, maxWeight, maxWeight * maxWeight );
                                    } );
                        } );

                std::string text = pathsText( paths );

                if( aVerbose )
                {
                    printf( "P|%zu|%zu|%s|%zu|%s\n", i, j, hx( maxWeight ).c_str(), paths.size(),
                            text.c_str() );
                }
                else
                {
                    printf( "P|%zu|%zu|%s|%zu|%016" PRIx64 "\n", i, j, hx( maxWeight ).c_str(),
                            paths.size(), fnv1a( text ) );
                }
            }
        }
    }

    return 0;
}


// ---- board ----------------------------------------------------------------------------------

static const char* shapeTName( SHAPE_T aShape )
{
    switch( aShape )
    {
    case SHAPE_T::SEGMENT:   return "segment";
    case SHAPE_T::RECTANGLE: return "rect";
    case SHAPE_T::ARC:       return "arc";
    case SHAPE_T::CIRCLE:    return "circle";
    case SHAPE_T::POLY:      return "poly";
    case SHAPE_T::BEZIER:    return "bezier";
    default:                 return "other";
    }
}


static void dumpGraphTail( CREEPAGE_GRAPH& aGraph, std::map<CREEP_SHAPE*, size_t>& aShapeIdx,
                           const char* aCfg, size_t aNodeStart, size_t aConnStart )
{
    for( size_t k = aNodeStart; k < aGraph.m_nodes.size(); ++k )
    {
        const std::shared_ptr<GRAPH_NODE>& n = aGraph.m_nodes[k];
        long parent = n->m_parent ? (long) aShapeIdx[n->m_parent] : -1;
        printf( "N|%s|%zu|%d|%ld|%s\n", aCfg, k, (int) n->m_type, parent, pt( n->m_pos ).c_str() );
    }

    std::map<GRAPH_NODE*, size_t> nodeIdx;

    for( size_t k = 0; k < aGraph.m_nodes.size(); ++k )
        nodeIdx[aGraph.m_nodes[k].get()] = k;

    for( size_t k = aConnStart; k < aGraph.m_connections.size(); ++k )
    {
        std::shared_ptr<GRAPH_CONNECTION>& gc = aGraph.m_connections[k];
        printf( "K|%s|%zu|%zu|%zu|%s|%d\n", aCfg, k, nodeIdx[gc->n1.get()], nodeIdx[gc->n2.get()],
                pathsText( { gc->m_path } ).c_str(), gc->m_forceStraightLine ? 1 : 0 );

        std::vector<PCB_SHAPE> drawn;
        gc->GetShapes( drawn );

        for( PCB_SHAPE& s : drawn )
        {
            printf( "G|%s|%zu|%s|%s|%s|%s\n", aCfg, k, shapeTName( s.GetShape() ),
                    pt( s.GetStart() ).c_str(), pt( s.GetEnd() ).c_str(),
                    s.GetShape() == SHAPE_T::ARC ? pt( s.GetCenter() ).c_str() : "-" );
        }
    }
}


static void runBoard( BOARD& aBoard )
{
    SHAPE_POLY_SET outline;
    bool           hasOutline = aBoard.GetBoardPolygonOutlines( outline, false, nullptr, false, true );

    CREEPAGE_GRAPH graph( aBoard );
    BuildCreepageBoardEdges( aBoard, graph.m_boardEdge, graph.m_ownedBoardEdges, nullptr );
    graph.m_boardOutline = hasOutline ? &outline : nullptr;

    printf( "O|%d|%d\n", hasOutline ? 1 : 0, outline.OutlineCount() );

    std::map<const BOARD_ITEM*, size_t> edgeIdx;

    for( size_t k = 0; k < graph.m_boardEdge.size(); ++k )
    {
        BOARD_ITEM* item = graph.m_boardEdge[k];
        PCB_SHAPE*  s = static_cast<PCB_SHAPE*>( item );
        edgeIdx[item] = k;
        bool owned = false;

        for( const std::unique_ptr<PCB_SHAPE>& o : graph.m_ownedBoardEdges )
            owned |= o.get() == item;

        if( owned )
        {
            printf( "E|%zu|owned|%s|%s|%s|%s\n", k, shapeTName( s->GetShape() ),
                    pt( s->GetStart() ).c_str(), pt( s->GetEnd() ).c_str(),
                    s->GetShape() == SHAPE_T::ARC ? pt( s->GetCenter() ).c_str() : "-" );
        }
        else
        {
            printf( "E|%zu|item|%s\n", k, item->m_Uuid.AsStdString().c_str() );
        }
    }

    graph.TransformEdgeToCreepShapes();
    graph.RemoveDuplicatedShapes();
    graph.TransformCreepShapesToNodes( graph.m_shapeCollection );

    printf( "H|%d\n", graph.m_hasOverlappingCutouts ? 1 : 0 );

    std::map<CREEP_SHAPE*, size_t> shapeIdx;

    for( size_t k = 0; k < graph.m_shapeCollection.size(); ++k )
    {
        CREEP_SHAPE* s = graph.m_shapeCollection[k].get();
        shapeIdx[s] = k;
        long parent = s->GetParent() ? (long) edgeIdx[s->GetParent()] : -1;
        printf( "C|%zu|%d|%ld|%s|%d|%s|%s|%s|%s\n", k, (int) s->GetType(), parent,
                pt( s->GetPos() ).c_str(), s->GetRadius(), hx( s->GetStartAngle().AsDegrees() ).c_str(),
                hx( s->GetEndAngle().AsDegrees() ).c_str(), pt( s->GetStartPoint() ).c_str(),
                pt( s->GetEndPoint() ).c_str() );
    }

    dumpGraphTail( graph, shapeIdx, "base", 0, 0 );

    size_t prefixNodes = graph.m_nodes.size();
    size_t prefixConns = graph.m_connections.size();

    struct CFG
    {
        const char* name;
        double      target;
        int         groove;
    };

    const CFG cfgs[] = { { "t20", 20000000.0, 0 }, { "t8g", 8000000.0, 500000 } };

    for( const CFG& cfg : cfgs )
    {
        graph.SetTarget( cfg.target );
        graph.m_minGrooveWidth = cfg.groove;

        // Child points on every board-edge circle and arc, as GeneratePaths would add them
        for( size_t k = 0; k < graph.m_shapeCollection.size(); ++k )
        {
            CREEP_SHAPE* s = graph.m_shapeCollection[k].get();
            std::vector<VECTOR2I> points;

            if( s->GetType() == CREEP_SHAPE::TYPE::CIRCLE )
            {
                for( int deg : { 0, 100, 200, 300 } )
                {
                    EDA_ANGLE a( (double) deg, DEGREES_T );
                    points.push_back( s->GetPos()
                                      + VECTOR2I( KiROUND( s->GetRadius() * a.Cos() ),
                                                  KiROUND( s->GetRadius() * a.Sin() ) ) );
                }
            }
            else if( s->GetType() == CREEP_SHAPE::TYPE::ARC )
            {
                EDA_ANGLE mid = ( s->GetStartAngle() + s->GetEndAngle() ) / 2.0;
                points.push_back( s->GetStartPoint() );
                points.push_back( s->GetPos()
                                  + VECTOR2I( KiROUND( s->GetRadius() * mid.Cos() ),
                                              KiROUND( s->GetRadius() * mid.Sin() ) ) );
                points.push_back( s->GetEndPoint() );
            }

            for( const VECTOR2I& p : points )
            {
                printf( "X|%s|%zu|%s\n", cfg.name, k, pt( p ).c_str() );
                std::shared_ptr<GRAPH_NODE> gn = graph.AddNode( GRAPH_NODE::POINT, s, p );
                gn->m_connectDirectly = false;
            }
        }

        // CREEPAGE_ENGINE::connectChildren
        std::vector<std::shared_ptr<GRAPH_NODE>> temp_nodes;

        std::copy_if( graph.m_nodes.begin(), graph.m_nodes.end(), std::back_inserter( temp_nodes ),
                      []( const std::shared_ptr<GRAPH_NODE>& aNode )
                      {
                          return !!aNode && aNode->m_parent && !aNode->m_parent->IsConductive()
                                 && !aNode->m_connectDirectly && aNode->m_type == GRAPH_NODE::POINT;
                      } );

        alg::for_all_pairs( temp_nodes.begin(), temp_nodes.end(),
                            [&]( const std::shared_ptr<GRAPH_NODE>& aN1,
                                 const std::shared_ptr<GRAPH_NODE>& aN2 )
                            {
                                if( aN1 == aN2 || !aN1 || !aN2 )
                                    return;

                                if( !aN1->m_parent || !aN2->m_parent )
                                    return;

                                if( aN1->m_parent != aN2->m_parent )
                                    return;

                                aN1->m_parent->ConnectChildren(
                                        const_cast<std::shared_ptr<GRAPH_NODE>&>( aN1 ),
                                        const_cast<std::shared_ptr<GRAPH_NODE>&>( aN2 ), graph );
                            } );

        dumpGraphTail( graph, shapeIdx, cfg.name, prefixNodes, prefixConns );

        graph.TruncateToPrefix( prefixNodes, prefixConns );
        printf( "T|%s|%zu|%zu\n", cfg.name, graph.m_nodes.size(), graph.m_connections.size() );
    }
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 2 )
    {
        fprintf( stderr, "usage: creepage32-oracle paths [--verbose] | board FILE...\n" );
        return 2;
    }

    if( !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &ignoreAssert );
    KIID::SeedGenerator( 1 );

    if( !strcmp( argv[1], "paths" ) )
        return runPaths( argc > 2 && !strcmp( argv[2], "--verbose" ) );

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

        runBoard( *board );
    }

    return 0;
}
