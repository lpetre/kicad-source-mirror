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
 * ft-cache-oracle: FROM_TO_CACHE (pcbnew/connectivity/from_to_cache.cpp) of the reference
 * build on corpus boards, for the Rust port in kicad_connectivity::from_to (CN-05).
 *
 *   ft-cache-oracle FILE...
 *
 * Per board (QA KI_TEST::LoadBoard path, as cn-order-oracle): FROM_TO_CACHE::Rebuild, then
 * for generated (from, to) pairs of every net with at least two pads (all ordered pairs of
 * nets with up to four pads, a seeded sample of six for larger nets, plus per net the
 * wildcard pairs "REF-*" -> pad and pad -> "REF", and once per board pad -> "*"), one line
 *
 *   ft|<from>|<to>|real=[..]|paths=N|unique=[..][|list=[..]]
 *
 * real   the items (pads, tracks, zones; cn-order-oracle names: p<f>.<k>, t<i>, z<i>) for which
 *        IsOnFromToPath( item, from, to ) is true, in the port's order;
 * list   the union of the path items of the cached paths of the pair when the C++
 *        uniquePathBetweenNodes (copied below unchanged) runs with ConnectedItems() in CN list
 *        order, i.e. the port's order; printed only where it differs from real, which happens
 *        only where the heap order of CN_ITEM::m_connected decides between paths;
 * paths  the number of cached FT_PATHs of the pair, unique their isUnique flags (list order).
 */

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <list>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <unordered_map>
#include <unordered_set>
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
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <zone.h>
#include <connectivity/connectivity_data.h>
#include <connectivity/connectivity_algo.h>
#include <connectivity/connectivity_items.h>
#include <connectivity/from_to_cache.h>
#include <drc/drc_engine.h>
#include <length_delay_calculation/length_delay_calculation.h>
#include <wildcards_and_files_ext.h>


/// Legal access to the private FROM_TO_CACHE::m_ftPaths (explicit instantiation may name
/// private members).
struct FT_PATHS_TAG
{
    typedef std::list<FROM_TO_CACHE::FT_PATH> FROM_TO_CACHE::*type;
    friend type get( FT_PATHS_TAG );
};

template <typename TAG, typename TAG::type M>
struct ROB
{
    friend typename TAG::type get( TAG ) { return M; }
};

template struct ROB<FT_PATHS_TAG, &FROM_TO_CACHE::m_ftPaths>;


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


/// KI_TEST::LoadBoard without the Boost.Test macros (as cn_order_oracle.cpp).
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


static std::unordered_map<const BOARD_ITEM*, std::string>         s_refs;
static std::unordered_map<const BOARD_ITEM*, std::pair<int, int>> s_rank;
static std::vector<BOARD_CONNECTED_ITEM*>                         s_items;   // port order


static void buildRefs( BOARD* aBoard )
{
    s_refs.clear();
    s_rank.clear();
    s_items.clear();

    int i = 0;
    int f = 0;
    int padSeq = 0;

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        int k = 0;

        for( PAD* pad : fp->Pads() )
        {
            s_refs[pad] = "p" + std::to_string( f ) + "." + std::to_string( k++ );
            s_rank[pad] = { 1, padSeq++ };
            s_items.push_back( pad );
        }

        f++;
    }

    for( PCB_TRACK* t : aBoard->Tracks() )
    {
        s_refs[t] = "t" + std::to_string( i );
        s_rank[t] = { 2, i++ };
        s_items.push_back( t );
    }

    i = 0;

    for( ZONE* z : aBoard->Zones() )
    {
        s_refs[z] = "z" + std::to_string( i );
        s_rank[z] = { 3, i++ };
        s_items.push_back( z );
    }
}


static std::string ref( const BOARD_ITEM* aItem )
{
    auto it = s_refs.find( aItem );
    return it == s_refs.end() ? std::string( "?" ) : it->second;
}


static std::string list( const std::vector<const BOARD_ITEM*>& aItems )
{
    std::string r = "[";

    for( size_t i = 0; i < aItems.size(); ++i )
        r += ( i ? "," : "" ) + ref( aItems[i] );

    return r + "]";
}


// ---- from_to_cache.cpp, copied unchanged except for the neighbour order ----

enum PATH_STATUS {
    PS_OK = 0,
    PS_MULTIPLE_PATHS = -1,
    PS_NO_PATH = -2
};


static bool isVertexVisited( CN_ITEM* v, const std::vector<CN_ITEM*>& path )
{
    for( CN_ITEM* u : path )
    {
        if ( u == v )
            return true;
    }

    return false;
}


static std::unordered_map<const CN_ITEM*, int> s_listIndex;


/// ConnectedItems() sorted by CN list index (the port's order of CN_ITEM::m_connected).
static std::vector<CN_ITEM*> connectedInListOrder( CN_ITEM* aItem )
{
    std::vector<CN_ITEM*> r( aItem->ConnectedItems().begin(), aItem->ConnectedItems().end() );
    std::sort( r.begin(), r.end(),
               []( CN_ITEM* a, CN_ITEM* b ) { return s_listIndex.at( a ) < s_listIndex.at( b ); } );
    return r;
}


static PATH_STATUS uniquePathBetweenNodes( CN_ITEM* u, CN_ITEM* v, std::vector<CN_ITEM*>& outPath )
{
    using Path = std::vector<CN_ITEM*>;
    std::deque<Path> Q;

    Path pInit;
    bool pathFound = false;
    pInit.push_back( u );
    Q.push_back( std::move( pInit ) );

    while( Q.size() )
    {
        Path path = Q.front();
        Q.pop_front();
        CN_ITEM* last = path.back();

        if( last == v )
        {
            outPath = path;

            if( pathFound )
                return PS_MULTIPLE_PATHS;

            pathFound = true;
        }

        for( CN_ITEM* ci : connectedInListOrder( last ) )
        {
            bool vertexVisited = isVertexVisited( ci, path );

            for( std::vector<CN_ITEM*>& p : Q )
            {
                if( isVertexVisited( ci, p ) )
                {
                    vertexVisited = true;
                    break;
                }
            }

            if( !vertexVisited )
            {
                Path newpath( path );
                newpath.push_back( ci );
                Q.push_back( std::move( newpath ) );
            }
        }
    }

    return pathFound ? PS_OK : PS_NO_PATH;
};

// ---- end of the copy ----


static std::string padName( const PAD* aPad )
{
    return std::string( ( aPad->GetParentFootprint()->GetReference() + wxT( "-" )
                          + aPad->GetNumber() ).ToUTF8() );
}


static void queryPair( BOARD* aBoard, FROM_TO_CACHE* aCache, const std::string& aFrom,
                       const std::string& aTo )
{
    wxString from = wxString::FromUTF8( aFrom );
    wxString to = wxString::FromUTF8( aTo );

    std::vector<const BOARD_ITEM*> real;

    for( BOARD_CONNECTED_ITEM* item : s_items )
    {
        if( aCache->IsOnFromToPath( item, from, to ) )
            real.push_back( item );
    }

    std::shared_ptr<CN_CONNECTIVITY_ALGO> cnAlgo = aBoard->GetConnectivity()->GetConnectivityAlgo();
    std::list<FROM_TO_CACHE::FT_PATH>&    paths = aCache->*get( FT_PATHS_TAG() );
    std::set<const BOARD_ITEM*>           replica;
    int                                   nPaths = 0;
    std::string                           unique = "[";

    for( FROM_TO_CACHE::FT_PATH& path : paths )
    {
        if( path.fromWildcard != from || path.toWildcard != to )
            continue;

        CN_ITEM*              cnFrom = cnAlgo->ItemEntry( path.from ).GetItems().front();
        CN_ITEM*              cnTo = cnAlgo->ItemEntry( path.to ).GetItems().front();
        std::vector<CN_ITEM*> upath;

        PATH_STATUS result = uniquePathBetweenNodes( cnFrom, cnTo, upath );

        for( const CN_ITEM* item : upath )
            replica.insert( item->Parent() );

        unique += ( nPaths ? "," : "" ) + std::string( result == PS_OK ? "1" : "0" );
        nPaths++;
    }

    std::vector<const BOARD_ITEM*> rep( replica.begin(), replica.end() );
    std::sort( rep.begin(), rep.end(),
               []( const BOARD_ITEM* a, const BOARD_ITEM* b )
               {
                   auto ra = s_rank.find( a );
                   auto rb = s_rank.find( b );
                   std::pair<int, int> ka = ra == s_rank.end() ? std::make_pair( 9, 0 ) : ra->second;
                   std::pair<int, int> kb = rb == s_rank.end() ? std::make_pair( 9, 0 ) : rb->second;
                   return ka < kb;
               } );

    std::string realList = list( real );
    std::string repList = list( rep );

    printf( "ft|%s|%s|real=%s|paths=%d|unique=%s]%s\n", aFrom.c_str(), aTo.c_str(),
            realList.c_str(), nPaths, unique.c_str(),
            repList == realList ? "" : ( "|list=" + repList ).c_str() );
}


static void dumpBoard( BOARD* aBoard )
{
    std::shared_ptr<CONNECTIVITY_DATA> conn = aBoard->GetConnectivity();
    std::shared_ptr<FROM_TO_CACHE>     cache = conn->GetFromToCache();

    // DRC_ENGINE::RunTests
    cache->Rebuild( aBoard );

    s_listIndex.clear();
    int idx = 0;

    for( CN_ITEM* item : conn->GetConnectivityAlgo()->ItemList() )
        s_listIndex[item] = idx++;

    // Pads per net, footprint order
    std::map<int, std::vector<PAD*>> netPads;

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
        {
            if( pad->GetNetCode() > 0 )
                netPads[pad->GetNetCode()].push_back( pad );
        }
    }

    bool starDone = false;

    for( auto& [net, pads] : netPads )
    {
        if( pads.size() < 2 )
            continue;

        std::vector<std::pair<std::string, std::string>> pairs;
        size_t n = pads.size();

        if( n <= 4 )
        {
            for( size_t a = 0; a < n; ++a )
                for( size_t b = 0; b < n; ++b )
                    if( a != b )
                        pairs.emplace_back( padName( pads[a] ), padName( pads[b] ) );
        }
        else
        {
            uint64_t x = 0x9E3779B97F4A7C15ULL ^ (uint64_t) net;

            for( int k = 0; k < 6; ++k )
            {
                x = x * 6364136223846793005ULL + 1442695040888963407ULL;
                size_t a = ( x >> 33 ) % n;
                x = x * 6364136223846793005ULL + 1442695040888963407ULL;
                size_t b = ( x >> 33 ) % ( n - 1 );

                if( b >= a )
                    b++;

                pairs.emplace_back( padName( pads[a] ), padName( pads[b] ) );
            }
        }

        std::string refA( pads[0]->GetParentFootprint()->GetReference().ToUTF8() );
        std::string refB( pads[n - 1]->GetParentFootprint()->GetReference().ToUTF8() );

        pairs.emplace_back( refA + "-*", padName( pads[n - 1] ) );
        pairs.emplace_back( padName( pads[0] ), refB );

        if( !starDone )
        {
            pairs.emplace_back( padName( pads[0] ), "*" );
            starDone = true;
        }

        for( const auto& [from, to] : pairs )
            queryPair( aBoard, cache.get(), from, to );
    }
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    std::vector<wxString> files;

    for( int i = 1; i < argc; ++i )
        files.push_back( wxString::FromUTF8( argv[i] ) );

    if( files.empty() )
    {
        fprintf( stderr, "usage: ft-cache-oracle FILE...\n" );
        return 2;
    }

    if( !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &ignoreAssert );
    KIID::SeedGenerator( 1 );

    for( const wxString& file : files )
    {
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

        buildRefs( board.get() );
        dumpBoard( board.get() );
        fflush( stdout );

        Pgm().GetSettingsManager().UnloadProject( &Pgm().GetSettingsManager().Prj(), false );
    }

    return 0;
}
