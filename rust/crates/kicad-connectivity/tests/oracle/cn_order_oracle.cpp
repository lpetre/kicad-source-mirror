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
 * cn-order-oracle: measures what the reference build does at every place where the C++
 * connectivity orders by heap address (rust/docs/design/review-connectivity.md).
 *
 *   cn-order-oracle [--loader] [--rebuild] [--drc] [--queries] FILE...
 *
 * Per board and per phase (`load` = after the load path built the connectivity, `rebuild` =
 * after CONNECTIVITY_DATA::Build was run again as DRC_CACHE_GENERATOR does, `drc` = after a real
 * DRC_ENGINE::RunTests) it prints one line per measurement:
 *
 *   <phase>|cn_addr|items=N|adjacent_inversions=K|at=[..]     CN_ITEM address vs list order
 *   <phase>|c_raw|<item>|[..]                                 m_connected where it is not in list order
 *   <phase>|c_summary|ge2=M|differs=K
 *   <phase>|bi_addr|tracks=K/N|pads=K/N|zones=K/N|drawings=K/N|kinds=..   BOARD_ITEM address vs board order
 *   <phase>|cl_addr|clusters=N|adjacent_inversions=K          ratsnest cluster address vs creation order
 *   <phase>|chains|multi_cluster=N|differs=K                  coincident-anchor chains (rn.cpp:253)
 *   <phase>|q_summary|ct=K/N|cp=K/N|cpv=K/N|net=K/N|zic=K/N   pointer-ordered query results that are
 *                                                             not in the port's order / results with >= 2 entries
 *   <phase>|rn_hash|<fnv64>                                   every ratsnest edge in GetEdges() order
 *   <phase>|dangling_hash|<fnv64>                             TestTrackEndpointDangling of every track
 *   <phase>|q|<kind>|<key>|[..]                               (--queries) the query results themselves,
 *                                                             pointer-ordered ones sorted into the
 *                                                             port's order (the goldens must not
 *                                                             depend on the heap); kinds: ct =
 *                                                             GetConnectedTracks, cp = GetConnectedPads,
 *                                                             cpv = GetConnectedPadsAndVias, ci<flags> =
 *                                                             GetConnectedItems, icl = IsConnectedOnLayer
 *                                                             per copper layer (no type filter / the
 *                                                             FlashLayer types), net = GetNetItems,
 *                                                             zic = GetZoneIslandConnections
 *   <phase>|rn|n<net>|[..]                                    (--queries) the ratsnest edges
 *   <phase>|dangling|<track>|<0/1>:<pos>|<0/1>:<pos>          (--queries) aIgnoreTracksInPads false / true
 *   <phase>|island|<zone>/<layer>|isolated=[..]|single=[..]   (--queries) FillIsolatedIslandsMap( map,
 *                                                             true ) for the map DRC builds, push order
 *   drc|<error type>|<main>|<aux>|<x>,<y>                     (--drc) violations whose items depend on
 *                                                             the connected order, sorted
 *
 * Items are named by board position: t<i> = Tracks()[i], p<f>.<k> = pad k of footprint f, z<i> =
 * Zones()[i], d<i> = Drawings()[i]; a zone layer item is z<i>/<layer id>/<outline>.
 *
 * The port's order (board-model.md 3.1): CN_ITEMs by list index; board items by kind
 * (pads, tracks, zones, graphics) and then board order.
 *
 * Loading is the one of rust/tools/pcb-dump/main.cpp (QA KI_TEST::LoadBoard path, or with
 * --loader BOARD_LOADER::Load like kicad-cli).
 */

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <map>
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
#include <board_loader.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_shape.h>
#include <pcb_track.h>
#include <zone.h>
#include <connectivity/connectivity_data.h>
#include <connectivity/connectivity_algo.h>
#include <connectivity/connectivity_items.h>
#include <ratsnest/ratsnest_data.h>
#include <drc/drc_engine.h>
#include <drc/drc_item.h>
#include <length_delay_calculation/length_delay_calculation.h>
#include <pcb_io/pcb_io_mgr.h>
#include <wildcards_and_files_ext.h>
#include <kiway_player.h>


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

    if( !project || type == PCB_IO_MGR::FILE_TYPE_NONE )
        return nullptr;

    return BOARD_LOADER::Load( aPath, type, project );
}


/// Legal access to the protected RN_NET::m_nodes through a member pointer of a derived class.
struct RN_NET_PEEK : public RN_NET
{
    using NODES = decltype( RN_NET_PEEK::m_nodes );

    static const NODES& Nodes( const RN_NET* aNet )
    {
        static constexpr NODES RN_NET::*nodes = &RN_NET_PEEK::m_nodes;
        return aNet->*nodes;
    }
};


static std::unordered_map<const BOARD_ITEM*, std::string>         s_refs;
static std::unordered_map<const BOARD_ITEM*, std::pair<int, int>> s_rank;


static void buildRefs( BOARD* aBoard )
{
    s_refs.clear();
    s_rank.clear();

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
        }

        f++;
    }

    for( PCB_TRACK* t : aBoard->Tracks() )
    {
        s_refs[t] = "t" + std::to_string( i );
        s_rank[t] = { 2, i++ };
    }

    i = 0;

    for( ZONE* z : aBoard->Zones() )
    {
        s_refs[z] = "z" + std::to_string( i );
        s_rank[z] = { 3, i++ };
    }

    i = 0;

    for( BOARD_ITEM* d : aBoard->Drawings() )
    {
        s_refs[d] = "d" + std::to_string( i );
        s_rank[d] = { 4, i++ };
    }
}


static std::string ref( const BOARD_ITEM* aItem )
{
    auto it = s_refs.find( aItem );
    return it == s_refs.end() ? std::string( "?" ) : it->second;
}


static std::string cnRef( const CN_ITEM* aItem )
{
    if( !aItem || !aItem->Valid() )
        return "invalid";

    std::string r = ref( aItem->Parent() );

    if( const CN_ZONE_LAYER* zl = dynamic_cast<const CN_ZONE_LAYER*>( aItem ) )
        r += "/" + std::to_string( (int) zl->GetLayer() ) + "/" + std::to_string( zl->SubpolyIndex() );

    return r;
}


template <typename T, typename F>
static std::string list( const T& aItems, F&& aFn )
{
    std::string r = "[";
    bool        first = true;

    for( const auto& it : aItems )
    {
        if( !first )
            r += ",";

        first = false;
        r += aFn( it );
    }

    return r + "]";
}


static bool rankLess( const BOARD_ITEM* a, const BOARD_ITEM* b )
{
    return s_rank[a] < s_rank[b];
}


template <typename T>
static bool inPortOrder( const std::vector<T>& aItems )
{
    for( size_t i = 1; i < aItems.size(); ++i )
    {
        if( rankLess( aItems[i], aItems[i - 1] ) )
            return false;
    }

    return true;
}


static uint64_t fnv( uint64_t h, const std::string& aBytes )
{
    for( unsigned char c : aBytes )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


static size_t adjacentInversions( const std::vector<const void*>& aPtrs )
{
    size_t n = 0;

    for( size_t i = 1; i < aPtrs.size(); ++i )
    {
        if( aPtrs[i] < aPtrs[i - 1] )
            n++;
    }

    return n;
}


static void dumpOrders( BOARD* aBoard, const char* aPhase, bool aQueries )
{
    std::shared_ptr<CONNECTIVITY_DATA> conn = aBoard->GetConnectivity();
    CN_CONNECTIVITY_ALGO*              algo = conn->GetConnectivityAlgo().get();

    // ---- CN_ITEM addresses vs list order ------------------------------------------------------
    std::vector<CN_ITEM*> items( algo->ItemList().begin(), algo->ItemList().end() );

    {
        size_t      inv = 0;
        std::string at;

        for( size_t i = 1; i < items.size(); ++i )
        {
            if( items[i] < items[i - 1] )
            {
                if( inv < 12 )
                    at += ( at.empty() ? "" : "," ) + std::to_string( i );

                inv++;
            }
        }

        printf( "%s|cn_addr|items=%zu|adjacent_inversions=%zu|at=[%s]\n", aPhase, items.size(), inv,
                at.c_str() );
    }

    // ---- m_connected in stored (address) order -----------------------------------------------
    {
        size_t ge2 = 0;
        size_t differs = 0;

        for( CN_ITEM* item : items )
        {
            const std::vector<CN_ITEM*>& c = item->ConnectedItems();

            if( c.size() < 2 )
                continue;

            ge2++;

            bool ascending = true;

            for( size_t j = 1; j < c.size(); ++j )
            {
                if( c[j]->ListIndex() < c[j - 1]->ListIndex() )
                    ascending = false;
            }

            if( !ascending )
            {
                if( differs < 40 )
                {
                    printf( "%s|c_raw|%s|%s\n", aPhase, cnRef( item ).c_str(),
                            list( c, []( const CN_ITEM* i ) { return cnRef( i ); } ).c_str() );
                }

                differs++;
            }
        }

        printf( "%s|c_summary|ge2=%zu|differs=%zu\n", aPhase, ge2, differs );
    }

    // ---- BOARD_ITEM addresses vs board order --------------------------------------------------
    {
        std::vector<const void*> tracks, pads, zones, drawings;

        for( PCB_TRACK* t : aBoard->Tracks() )
            tracks.push_back( t );

        for( FOOTPRINT* fp : aBoard->Footprints() )
        {
            for( PAD* pad : fp->Pads() )
                pads.push_back( pad );
        }

        for( ZONE* z : aBoard->Zones() )
            zones.push_back( z );

        for( BOARD_ITEM* d : aBoard->Drawings() )
            drawings.push_back( d );

        // Address ranges per kind: "a<b" when every a lies below every b.
        std::vector<std::pair<std::string, const std::vector<const void*>*>> kinds = {
            { "pads", &pads }, { "tracks", &tracks }, { "zones", &zones }, { "drawings", &drawings }
        };
        std::string rel;

        for( size_t a = 0; a < kinds.size(); ++a )
        {
            for( size_t b = a + 1; b < kinds.size(); ++b )
            {
                const auto& va = *kinds[a].second;
                const auto& vb = *kinds[b].second;

                if( va.empty() || vb.empty() )
                    continue;

                const void* maxA = *std::max_element( va.begin(), va.end() );
                const void* minA = *std::min_element( va.begin(), va.end() );
                const void* maxB = *std::max_element( vb.begin(), vb.end() );
                const void* minB = *std::min_element( vb.begin(), vb.end() );

                rel += rel.empty() ? "" : ",";
                rel += kinds[a].first + ( maxA < minB ? "<" : maxB < minA ? ">" : "~" ) + kinds[b].first;
            }
        }

        printf( "%s|bi_addr|tracks=%zu/%zu|pads=%zu/%zu|zones=%zu/%zu|drawings=%zu/%zu|kinds=%s\n",
                aPhase, adjacentInversions( tracks ), tracks.size(), adjacentInversions( pads ),
                pads.size(), adjacentInversions( zones ), zones.size(),
                adjacentInversions( drawings ), drawings.size(), rel.c_str() );
    }

    // ---- ratsnest clusters: address vs creation order -----------------------------------------
    {
        // A cluster is created when its first (lowest list index) member is reached.
        std::map<int, const CN_CLUSTER*> byCreation;

        for( CN_ITEM* item : items )
        {
            for( const std::shared_ptr<CN_ANCHOR>& a : item->Anchors() )
            {
                if( const std::shared_ptr<CN_CLUSTER>& cl = a->GetCluster() )
                    byCreation[( *cl->begin() )->ListIndex()] = cl.get();
            }
        }

        std::vector<const void*> addrs;

        for( const auto& [key, cl] : byCreation )
            addrs.push_back( cl );

        printf( "%s|cl_addr|clusters=%zu|adjacent_inversions=%zu\n", aPhase, addrs.size(),
                adjacentInversions( addrs ) );

        // The chains the triangulator sorts by cluster address (rn.cpp:253).
        size_t multi = 0;
        size_t differs = 0;

        for( int net = 0; net < conn->GetNetCount(); ++net )
        {
            RN_NET* rn = conn->GetRatsnestForNet( net );

            if( !rn || rn->GetNodeCount() < 3 )
                continue;

            std::vector<std::shared_ptr<CN_ANCHOR>> chain;

            auto flush =
                    [&]()
                    {
                        std::set<const CN_CLUSTER*> distinct;

                        for( const auto& a : chain )
                            distinct.insert( a->GetCluster().get() );

                        if( distinct.size() >= 2 )
                        {
                            multi++;

                            std::vector<const CN_CLUSTER*> byAddr( distinct.begin(), distinct.end() );
                            std::vector<const CN_CLUSTER*> byKey = byAddr;

                            std::sort( byKey.begin(), byKey.end(),
                                       []( const CN_CLUSTER* a, const CN_CLUSTER* b )
                                       {
                                           CN_CLUSTER* ma = const_cast<CN_CLUSTER*>( a );
                                           CN_CLUSTER* mb = const_cast<CN_CLUSTER*>( b );
                                           return ( *ma->begin() )->ListIndex() < ( *mb->begin() )->ListIndex();
                                       } );

                            if( byKey != byAddr )
                                differs++;
                        }

                        chain.clear();
                    };

            for( const std::shared_ptr<CN_ANCHOR>& a : RN_NET_PEEK::Nodes( rn ) )
            {
                if( !chain.empty() && chain.back()->Pos() != a->Pos() )
                    flush();

                chain.push_back( a );
            }

            flush();
        }

        printf( "%s|chains|multi_cluster=%zu|differs=%zu\n", aPhase, multi, differs );
    }

    // ---- pointer-ordered query results ---------------------------------------------------------
    {
        size_t ctN = 0, ctD = 0, cpN = 0, cpD = 0, cpvN = 0, cpvD = 0, netN = 0, netD = 0, zicN = 0,
               zicD = 0;

        std::vector<BOARD_CONNECTED_ITEM*> all;

        for( FOOTPRINT* fp : aBoard->Footprints() )
        {
            for( PAD* pad : fp->Pads() )
                all.push_back( pad );
        }

        for( PCB_TRACK* t : aBoard->Tracks() )
            all.push_back( t );

        for( ZONE* z : aBoard->Zones() )
            all.push_back( z );

        for( BOARD_ITEM* d : aBoard->Drawings() )
        {
            if( d->Type() == PCB_SHAPE_T )
                all.push_back( static_cast<PCB_SHAPE*>( d ) );
        }

        auto listIndex =
                [&]( const BOARD_CONNECTED_ITEM* aItem )
                {
                    const auto& e = algo->ItemEntry( aItem ).GetItems();
                    return e.empty() ? -1 : e.front()->ListIndex();
                };

        auto refOf = []( const BOARD_ITEM* i ) { return ref( i ); };

        for( BOARD_CONNECTED_ITEM* item : all )
        {
            if( !algo->ItemExists( item ) )
                continue;

            std::vector<PCB_TRACK*> ct = conn->GetConnectedTracks( item );

            if( ct.size() >= 2 )
            {
                ctN++;

                if( !inPortOrder( ct ) )
                    ctD++;
            }

            if( aQueries && !ct.empty() )
            {
                std::sort( ct.begin(), ct.end(), rankLess );
                printf( "%s|q|ct|%s|%s\n", aPhase, ref( item ).c_str(), list( ct, refOf ).c_str() );
            }

            std::vector<PAD*> cp = conn->GetConnectedPads( item );

            if( cp.size() >= 2 )
            {
                cpN++;

                if( !inPortOrder( cp ) )
                    cpD++;
            }

            if( aQueries && !cp.empty() )
            {
                std::sort( cp.begin(), cp.end(), rankLess );
                printf( "%s|q|cp|%s|%s\n", aPhase, ref( item ).c_str(), list( cp, refOf ).c_str() );
            }

            std::vector<PAD*>     pads;
            std::vector<PCB_VIA*> vias;
            conn->GetConnectedPadsAndVias( item, &pads, &vias );

            if( pads.size() >= 2 || vias.size() >= 2 )
            {
                cpvN++;

                // The port's order: the list index of the connected CN_ITEMs.  A zone has several
                // CN_ITEMs, so only the order inside one CN_ITEM's connections is defined; for
                // other parents there is one CN_ITEM.
                bool ok = true;

                if( item->Type() != PCB_ZONE_T )
                {
                    for( size_t i = 1; i < pads.size(); ++i )
                        ok = ok && listIndex( pads[i - 1] ) <= listIndex( pads[i] );

                    for( size_t i = 1; i < vias.size(); ++i )
                        ok = ok && listIndex( vias[i - 1] ) <= listIndex( vias[i] );
                }

                if( !ok )
                    cpvD++;
            }

            if( aQueries && item->Type() != PCB_ZONE_T && ( !pads.empty() || !vias.empty() ) )
            {
                auto byListIndex =
                        [&]( const BOARD_CONNECTED_ITEM* a, const BOARD_CONNECTED_ITEM* b )
                        {
                            return listIndex( a ) < listIndex( b );
                        };

                std::sort( pads.begin(), pads.end(), byListIndex );
                std::sort( vias.begin(), vias.end(), byListIndex );
                printf( "%s|q|cpv|%s|%s|%s\n", aPhase, ref( item ).c_str(),
                        list( pads, refOf ).c_str(), list( vias, refOf ).c_str() );
            }

            if( aQueries )
            {
                // GetConnectedItems: cluster order (deterministic), for the three flag sets
                // the callers use.
                for( int flags : { 0, (int) EXCLUDE_ZONES, (int) ( EXCLUDE_ZONES | IGNORE_NETS ) } )
                {
                    std::vector<BOARD_CONNECTED_ITEM*> ci = conn->GetConnectedItems( item, flags );

                    printf( "%s|q|ci%d|%s|%s\n", aPhase, flags, ref( item ).c_str(),
                            list( ci, refOf ).c_str() );
                }

                // IsConnectedOnLayer with an empty type list (zones count: the island branches)
                // and with the types PAD / PCB_VIA::FlashLayer pass.
                if( item->Type() == PCB_VIA_T || item->Type() == PCB_PAD_T )
                {
                    std::string any, flash;

                    for( PCB_LAYER_ID layer : LSET::AllCuMask( aBoard->GetCopperLayerCount() ) )
                    {
                        any += conn->IsConnectedOnLayer( item, layer, {} ) ? "1" : "0";
                        flash += conn->IsConnectedOnLayer( item, layer,
                                                           { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_PAD_T } )
                                         ? "1" : "0";
                    }

                    printf( "%s|q|icl|%s|any=%s|flash=%s\n", aPhase, ref( item ).c_str(), any.c_str(),
                            flash.c_str() );
                }
            }
        }

        static const std::vector<KICAD_T> types = { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T,
                                                    PCB_ZONE_T,  PCB_PAD_T, PCB_SHAPE_T };

        for( int net = 0; net < conn->GetNetCount(); ++net )
        {
            std::vector<BOARD_CONNECTED_ITEM*> ni = conn->GetNetItems( net, types );

            if( ni.size() >= 2 )
            {
                netN++;

                if( !inPortOrder( ni ) )
                    netD++;
            }

            if( aQueries && !ni.empty() )
            {
                std::sort( ni.begin(), ni.end(), rankLess );
                printf( "%s|q|net|%d|%s\n", aPhase, net, list( ni, refOf ).c_str() );
            }
        }

        for( ZONE* zone : aBoard->Zones() )
        {
            if( !algo->ItemExists( zone ) )
                continue;

            for( PCB_LAYER_ID layer : zone->GetLayerSet() )
            {
                std::vector<std::set<const BOARD_ITEM*>> islands;
                conn->GetZoneIslandConnections( zone, layer, &islands );

                std::string text;
                bool        any = false;
                bool        ok = true;

                for( const std::set<const BOARD_ITEM*>& island : islands )
                {
                    std::vector<const BOARD_ITEM*> v( island.begin(), island.end() );

                    if( v.size() >= 2 )
                        any = true;

                    ok = ok && inPortOrder( v );
                    std::sort( v.begin(), v.end(), rankLess );
                    text += list( v, refOf );
                }

                if( any )
                {
                    zicN++;

                    if( !ok )
                        zicD++;
                }

                if( aQueries && !islands.empty() )
                {
                    printf( "%s|q|zic|%s/%d|%s\n", aPhase, ref( zone ).c_str(), (int) layer,
                            text.c_str() );
                }
            }
        }

        printf( "%s|q_summary|ct=%zu/%zu|cp=%zu/%zu|cpv=%zu/%zu|net=%zu/%zu|zic=%zu/%zu\n", aPhase,
                ctD, ctN, cpD, cpN, cpvD, cpvN, netD, netN, zicD, zicN );
    }

    // ---- the ratsnest -------------------------------------------------------------------------
    {
        uint64_t h = 0xcbf29ce484222325ULL;

        auto anchor =
                []( const CN_ANCHOR& a )
                {
                    return std::to_string( a.Pos().x ) + "," + std::to_string( a.Pos().y ) + "@"
                           + cnRef( a.Item() );
                };

        for( int net = 0; net < conn->GetNetCount(); ++net )
        {
            RN_NET* rn = conn->GetRatsnestForNet( net );

            if( !rn || rn->GetNodeCount() == 0 )
                continue;

            std::string text = list( rn->GetEdges(),
                                     [&]( const CN_EDGE& e )
                                     {
                                         return anchor( *e.GetSourceNode() ) + ">"
                                                + anchor( *e.GetTargetNode() ) + ":"
                                                + std::to_string( e.GetWeight() );
                                     } );

            h = fnv( h, std::to_string( net ) + text );

            if( aQueries && text != "[]" )
                printf( "%s|rn|n%d|%s\n", aPhase, net, text.c_str() );
        }

        printf( "%s|rn_hash|%016llx\n", aPhase, (unsigned long long) h );
    }

    // ---- TestTrackEndpointDangling (in the drc phase with the copper zone r-trees) ------------
    {
        uint64_t h = 0xcbf29ce484222325ULL;

        for( PCB_TRACK* track : aBoard->Tracks() )
        {
            std::string text = ref( track );

            for( bool ignore : { false, true } )
            {
                VECTOR2I pos( INT_MIN, INT_MIN );
                bool     dangling = conn->TestTrackEndpointDangling( track, ignore, &pos );

                text += dangling ? "|1" : "|0";
                text += pos.x == INT_MIN ? std::string( ":unset" )
                                         : ":" + std::to_string( pos.x ) + "," + std::to_string( pos.y );
            }

            h = fnv( h, text );

            if( aQueries )
                printf( "%s|dangling|%s\n", aPhase, text.c_str() );
        }

        printf( "%s|dangling_hash|%016llx\n", aPhase, (unsigned long long) h );
    }

    // ---- FillIsolatedIslandsMap( map, true ) as DRC_CACHE_GENERATOR::Run calls it ---------------
    {
        std::map<ZONE*, std::map<PCB_LAYER_ID, ISOLATED_ISLANDS>> islands;

        for( ZONE* zone : aBoard->Zones() )
        {
            if( !zone->GetIsRuleArea() && !zone->IsTeardropArea() && !zone->IsCopperThieving() )
            {
                zone->GetLayerSet().RunOnLayers(
                        [&]( PCB_LAYER_ID layer )
                        {
                            islands[zone][layer] = ISOLATED_ISLANDS();
                        } );
            }
        }

        conn->FillIsolatedIslandsMap( islands, true );

        // In board order (the map itself iterates by zone address); the outline lists in the
        // order the algorithm pushed them.
        for( ZONE* zone : aBoard->Zones() )
        {
            auto it = islands.find( zone );

            if( it == islands.end() || !aQueries )
                continue;

            for( auto& [layer, isl] : it->second )
            {
                printf( "%s|island|%s/%d|isolated=%s|single=%s\n", aPhase, ref( zone ).c_str(),
                        (int) layer,
                        list( isl.m_IsolatedOutlines, []( int i ) { return std::to_string( i ); } ).c_str(),
                        list( isl.m_SingleConnectionOutlines,
                              []( int i ) { return std::to_string( i ); } ).c_str() );
            }
        }
    }

    fflush( stdout );
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    bool                  useLoader = false;
    bool                  rebuild = false;
    bool                  drc = false;
    bool                  queries = false;
    std::vector<wxString> files;

    for( int i = 1; i < argc; ++i )
    {
        std::string a = argv[i];

        if( a == "--loader" )
            useLoader = true;
        else if( a == "--rebuild" )
            rebuild = true;
        else if( a == "--drc" )
            drc = true;
        else if( a == "--queries" )
            queries = true;
        else
            files.push_back( wxString::FromUTF8( a ) );
    }

    if( files.empty() )
    {
        fprintf( stderr, "usage: cn-order-oracle [--loader] [--rebuild] [--drc] [--queries] FILE...\n" );
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
            board = useLoader ? loadBoardCli( file ) : loadBoardQa( file );
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
        dumpOrders( board.get(), "load", queries );

        if( rebuild )
        {
            // DRC_CACHE_GENERATOR::Run
            std::shared_ptr<CONNECTIVITY_DATA> conn = board->GetConnectivity();
            conn->ClearRatsnest();
            conn->Build( board.get() );
            dumpOrders( board.get(), "rebuild", queries );
        }

        if( drc )
        {
            std::shared_ptr<DRC_ENGINE> engine = board->GetDesignSettings().m_DRCEngine;
            std::vector<std::string>    lines;
            std::mutex                  mutex;

            engine->SetViolationHandler(
                    [&]( const std::shared_ptr<DRC_ITEM>& aItem, const VECTOR2I& aPos, int aLayer,
                         const std::function<void( PCB_MARKER* )>& aPathGenerator )
                    {
                        int code = aItem->GetErrorCode();

                        if( code != DRCE_TRACK_NOT_CENTERED_ON_VIA
                            && code != DRCE_TRACK_ON_POST_MACHINED_LAYER
                            && code != DRCE_UNCONNECTED_ITEMS )
                        {
                            return;
                        }

                        std::string line = "drc|" + std::string( aItem->GetErrorText( false ).ToUTF8() );

                        for( const KIID& id : aItem->GetIDs() )
                            line += "|" + ref( board->ResolveItem( id, true ) );

                        line += "|" + std::to_string( aPos.x ) + "," + std::to_string( aPos.y );

                        std::lock_guard<std::mutex> lock( mutex );
                        lines.push_back( line );
                    } );

            try
            {
                engine->RunTests( EDA_UNITS::MM, true, false );
            }
            catch( ... )
            {
                printf( "error|drc\n" );
            }

            engine->ClearViolationHandler();

            std::sort( lines.begin(), lines.end() );

            for( const std::string& l : lines )
                printf( "%s\n", l.c_str() );

            dumpOrders( board.get(), "drc", queries );
        }

        Pgm().GetSettingsManager().UnloadProject( &Pgm().GetSettingsManager().Prj(), false );
    }

    return 0;
}
