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

/**
 * pcb-dump: the opt-in `connectivity` section group (cn_item, cn_cluster, rn_net, cn_dangling,
 * cn_island), printed from the CONNECTIVITY_DATA the loader built (BuildConnectivity).  Format:
 * rust/docs/PCB_DUMP_FORMAT.md, section "Connectivity".  Replayed by the kicad-connectivity
 * tests (crates/kicad-connectivity/tests/golden/).
 */

#include "pcb_dump.h"

#include <board.h>
#include <pcb_track.h>
#include <zone.h>
#include <connectivity/connectivity_data.h>
#include <connectivity/connectivity_algo.h>
#include <connectivity/connectivity_items.h>
#include <ratsnest/ratsnest_data.h>

#include <algorithm>
#include <map>


namespace
{

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


/// Reference to a CN_ITEM independent of list order: the parent's item key, plus
/// "/<layer>/<outline>" for a zone layer item.
std::string CnRef( const CN_ITEM* aItem )
{
    if( !aItem )
        return "null";

    if( !aItem->Valid() )
        return "invalid";

    std::string key = ItemKey( aItem->Parent() );

    if( const CN_ZONE_LAYER* zl = dynamic_cast<const CN_ZONE_LAYER*>( aItem ) )
        key += "/" + VLayer( zl->GetLayer() ) + "/" + std::to_string( zl->SubpolyIndex() );

    return key;
}


std::string AnchorRef( const CN_ANCHOR& aAnchor )
{
    return V( aAnchor.Pos() ) + "@" + CnRef( aAnchor.Item() );
}


template <typename T, typename F>
std::string List( T&& aItems, F&& aFn )
{
    std::string r = "[";
    bool        first = true;

    for( auto&& it : aItems )
    {
        if( !first )
            r += ";";

        first = false;
        r += aFn( it );
    }

    return r + "]";
}


void DumpCnItems( CN_CONNECTIVITY_ALGO* aAlgo )
{
    int idx = 0;

    for( CN_ITEM* item : aAlgo->ItemList() )
    {
        SCOPE s( "cn_item", "#" + std::to_string( idx++ ) );

        EMIT( "ref", CnRef( item ) );
        EMIT( "list_index", V( item->ListIndex() ) );
        EMIT( "type", VEnum( item->Parent()->Type() ) );
        EMIT( "valid", V( item->Valid() ) );
        EMIT( "net", V( item->Net() ) );
        EMIT( "can_change_net", V( item->CanChangeNet() ) );
        EMIT( "layers", V( item->StartLayer() ) + ".." + V( item->EndLayer() ) );
        EMIT( "board_layer", VLayer( item->GetBoardLayer() ) );
        EMIT( "bbox", V( item->BBox() ) );
        EMIT( "anchors", List( item->Anchors(),
                               []( const std::shared_ptr<CN_ANCHOR>& a ) { return V( a->Pos() ); } ) );
        EMIT( "anchor_count", V( item->AnchorCount() ) );

        Guard( "get_anchor",
               [&]()
               {
                   std::vector<VECTOR2I> pts;

                   for( int n = 0; n < item->AnchorCount(); ++n )
                       pts.push_back( item->GetAnchor( n ) );

                   g_out.Emit( "get_anchor", VPoints( pts ) );
               } );

        // m_connected is sorted by heap address, which varies from run to run (items are
        // connected from a thread pool).  Print it in list order, the port's replacement
        // (connectivity-zones.md R-1), so that the output is deterministic.
        std::vector<CN_ITEM*> connected = item->ConnectedItems();
        std::sort( connected.begin(), connected.end(),
                   []( const CN_ITEM* a, const CN_ITEM* b )
                   { return a->ListIndex() < b->ListIndex(); } );
        EMIT( "connected", List( connected, []( const CN_ITEM* c ) { return CnRef( c ); } ) );
        EMIT( "anchor_connected_count",
              List( item->Anchors(),
                    []( const std::shared_ptr<CN_ANCHOR>& a )
                    { return V( a->ConnectedItemsCount() ); } ) );
        EMIT( "anchor_dangling",
              List( item->Anchors(),
                    []( const std::shared_ptr<CN_ANCHOR>& a ) { return V( a->IsDangling() ); } ) );

        if( CN_ZONE_LAYER* zl = dynamic_cast<CN_ZONE_LAYER*>( item ) )
        {
            EMIT( "zone_outline_points", V( zl->OutlinePointCount() ) );
            EMIT( "zone_single_connection", V( zl->HasSingleConnection() ) );
        }
    }
}


void DumpClusters( const std::string& aPrefix, const CN_CONNECTIVITY_ALGO::CLUSTERS& aClusters )
{
    int idx = 0;

    for( const std::shared_ptr<CN_CLUSTER>& cl : aClusters )
    {
        SCOPE s( "cn_cluster", aPrefix + "#" + std::to_string( idx++ ) );

        EMIT( "origin_net", V( cl->OriginNet() ) );
        EMIT( "origin_net_name", V( cl->OriginNetName() ) );
        EMIT( "orphaned", V( cl->IsOrphaned() ) );
        EMIT( "conflicting", V( cl->IsConflicting() ) );
        EMIT( "items", List( *cl, []( const CN_ITEM* c ) { return CnRef( c ); } ) );
    }
}


void DumpRatsnest( BOARD* aBoard, CONNECTIVITY_DATA* aConn )
{
    for( int net = 0; net < aConn->GetNetCount(); ++net )
    {
        RN_NET* rn = aConn->GetRatsnestForNet( net );

        if( !rn || rn->GetNodeCount() == 0 )
            continue;

        SCOPE s( "rn_net", "n" + std::to_string( net ) );

        NETINFO_ITEM* ni = aBoard->FindNet( net );
        EMIT( "name", V( ni ? ni->GetNetname() : wxString( "?" ) ) );
        EMIT( "node_count", V( rn->GetNodeCount() ) );
        EMIT( "nodes", List( RN_NET_PEEK::Nodes( rn ),
                             []( const std::shared_ptr<CN_ANCHOR>& a ) { return AnchorRef( *a ); } ) );
        EMIT( "edges", List( rn->GetEdges(),
                             []( const CN_EDGE& e )
                             {
                                 return AnchorRef( *e.GetSourceNode() ) + ">"
                                        + AnchorRef( *e.GetTargetNode() ) + ":"
                                        + V( e.GetWeight() ) + ":" + V( e.IsVisible() );
                             } ) );
    }

    SCOPE s( "rn_net", "board" );
    EMIT( "unconnected_count", V( aConn->GetUnconnectedCount( false ) ) );
    EMIT( "unconnected_visible", V( aConn->GetUnconnectedCount( true ) ) );
}


void DumpDangling( BOARD* aBoard, CONNECTIVITY_DATA* aConn )
{
    for( PCB_TRACK* track : aBoard->Tracks() )
    {
        SCOPE s( "cn_dangling", ItemKey( track ) );

        for( bool ignore : { false, true } )
        {
            std::string suffix = ignore ? ".ignore_in_pads" : "";

            Guard( "dangling" + suffix,
                   [&]()
                   {
                       VECTOR2I pos( INT_MIN, INT_MIN );
                       bool     dangling = aConn->TestTrackEndpointDangling( track, ignore, &pos );

                       g_out.Emit( "dangling" + suffix, V( dangling ) );
                       g_out.Emit( "pos" + suffix, pos.x == INT_MIN && pos.y == INT_MIN
                                                           ? std::string( "unset" )
                                                           : V( pos ) );
                   } );
        }
    }
}


/// The isolated-island query exactly as ZONE_FILLER::Fill builds its map (zone_filler.cpp:
/// rule areas, degenerate outlines, thieving and teardrop zones skipped; every layer of the
/// layer set), on the fills read from the file.  Mutates the connectivity (zones re-added), so
/// it runs last.
void DumpIslands( BOARD* aBoard, CONNECTIVITY_DATA* aConn )
{
    std::map<ZONE*, std::map<PCB_LAYER_ID, ISOLATED_ISLANDS>> islands;

    for( ZONE* zone : aBoard->Zones() )
    {
        if( zone->GetIsRuleArea() || zone->GetNumCorners() <= 2 )
            continue;

        for( PCB_LAYER_ID layer : zone->GetLayerSet() )
        {
            if( !zone->IsCopperThieving() && !zone->IsTeardropArea() )
                islands[zone][layer] = ISOLATED_ISLANDS();
        }
    }

    aConn->FillIsolatedIslandsMap( islands );

    for( ZONE* zone : aBoard->Zones() )
    {
        auto it = islands.find( zone );

        if( it == islands.end() )
            continue;

        for( auto& [layer, isl] : it->second )
        {
            SCOPE s( "cn_island", ItemKey( zone ) + "/" + VLayer( layer ) );

            std::vector<int> isolated = isl.m_IsolatedOutlines;
            std::vector<int> single = isl.m_SingleConnectionOutlines;
            std::sort( isolated.begin(), isolated.end(), std::greater<int>() );
            std::sort( single.begin(), single.end() );

            EMIT( "isolated", List( isolated, []( int i ) { return V( i ); } ) );
            EMIT( "single_connection", List( single, []( int i ) { return V( i ); } ) );
        }
    }
}

} // namespace


void DumpConnectivity( BOARD* aBoard )
{
    std::shared_ptr<CONNECTIVITY_DATA> conn = aBoard->GetConnectivity();

    if( !conn )
    {
        g_out.Error( "cn_item", "board", "no connectivity" );
        return;
    }

    CN_CONNECTIVITY_ALGO* algo = conn->GetConnectivityAlgo().get();

    Guard( "cn_item", [&]() { DumpCnItems( algo ); } );
    Guard( "cn_cluster", [&]() { DumpClusters( "ratsnest", algo->SearchClusters( CN_CONNECTIVITY_ALGO::CSM_RATSNEST ) ); } );
    Guard( "cn_cluster", [&]() { DumpClusters( "propagate", algo->SearchClusters( CN_CONNECTIVITY_ALGO::CSM_PROPAGATE ) ); } );
    Guard( "rn_net", [&]() { DumpRatsnest( aBoard, conn.get() ); } );
    Guard( "cn_dangling", [&]() { DumpDangling( aBoard, conn.get() ); } );
    Guard( "cn_island", [&]() { DumpIslands( aBoard, conn.get() ); } );
}
