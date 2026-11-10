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
 * drc-rtree-oracle: differential oracle of work package DRC-02 (DRC_RTREE, DRC_ZONE_INDEX,
 * CollectOverlappingPairs, DRC_TEST_PROVIDER base). Runs the real header templates of the
 * reference build on boards loaded with KI_TEST::ReadBoardFromFileOrStream and prints, line by
 * line, every visit sequence. The Rust twin is tests/rtree_oracle.rs; the line format is
 * documented there. Items are named by their ordinal ("#n") in the walk
 * forEachGeometryItem( {}, AllLayersMask ).
 *
 *   drc-rtree-oracle [--global] FILE...
 *   drc-rtree-oracle --cachegen FILE...
 *
 * --cachegen (work package DRC-05) loads each board like KI_TEST::LoadBoard (sibling project,
 * InitEngine with the sibling rule file, nets, connectivity), runs the real
 * DRC_CACHE_GENERATOR::Run() and prints the BOARD caches it filled (runCacheGen below).
 * The Rust twin is tests/cache_generator_oracle.rs.
 */

#include <cstdio>
#include <map>
#include <set>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/mock_utils.h>
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <footprint.h>
#include <pad.h>
#include <zone.h>
#include <drc/drc_engine.h>
#include <drc/drc_rtree.h>
#include <drc/drc_zone_index.h>
#include <drc/drc_test_provider.h>
#include <drc/drc_cache_generator.h>
#include <wx/filename.h>


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


class ORACLE_PROVIDER : public DRC_TEST_PROVIDER
{
public:
    bool Run() override { return true; }

    using DRC_TEST_PROVIDER::forEachGeometryItem;
    using DRC_TEST_PROVIDER::formatMsg;

    static const std::vector<KICAD_T>& Basic() { return s_allBasicItems; }
    static const std::vector<KICAD_T>& BasicButZones() { return s_allBasicItemsButZones; }
};


static const int WORST = 200000;

static std::map<const BOARD_ITEM*, int> g_key;


static std::string key( const BOARD_ITEM* aItem )
{
    auto it = g_key.find( aItem );
    return it == g_key.end() ? std::string( "?" ) : "#" + std::to_string( it->second );
}


static std::string sub( const DRC_RTREE::ITEM_WITH_SHAPE* aEntry )
{
    if( aEntry->shapeStorage )
        return "h";

    if( aEntry->shape == aEntry->parentShape.get() )
        return "-";

    std::vector<const SHAPE*> subshapes;
    aEntry->parentShape->GetIndexableSubshapes( subshapes );

    for( size_t i = 0; i < subshapes.size(); ++i )
    {
        if( subshapes[i] == aEntry->shape )
            return std::to_string( i );
    }

    return "?";
}


static std::string entry( const DRC_RTREE::ITEM_WITH_SHAPE* aEntry )
{
    return key( aEntry->parent ) + ":" + sub( aEntry );
}


static std::string join( const std::vector<std::string>& aList )
{
    std::string out;

    for( size_t i = 0; i < aList.size(); ++i )
        out += ( i ? "," : "" ) + aList[i];

    return out;
}


static std::string boxStr( const BOX2I& aBox )
{
    return std::to_string( aBox.GetX() ) + " " + std::to_string( aBox.GetY() ) + " "
           + std::to_string( aBox.GetRight() ) + " " + std::to_string( aBox.GetBottom() );
}


/// Layers of an item in the copper tree, as DRC_CACHE_GENERATOR::Run computes them.
static LSET copperLayersOf( BOARD_ITEM* aItem, const LSET& aBoardCopper )
{
    LSET copperLayers = aItem->GetLayerSet() & aBoardCopper;

    if( aItem->Type() == PCB_PAD_T && static_cast<PAD*>( aItem )->HasHole() )
        copperLayers = aBoardCopper;

    return copperLayers;
}


static void runBoard( const std::string& aPath, bool aGlobal )
{
    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( aPath );

    std::string name = aPath.substr( aPath.find_last_of( '/' ) + 1 );
    printf( "board|%s\n", name.c_str() );

    DRC_ENGINE      engine( board.get(), &board->GetDesignSettings() );
    ORACLE_PROVIDER prov;
    prov.SetDRCEngine( &engine );

    g_key.clear();

    std::vector<std::string> types;

    prov.forEachGeometryItem( {}, LSET::AllLayersMask(),
            [&]( BOARD_ITEM* aItem ) -> bool
            {
                int ord = (int) g_key.size();
                g_key[aItem] = ord;
                types.push_back( std::to_string( (int) aItem->Type() ) );
                return true;
            } );

    printf( "walk|keys|%s\n", join( types ).c_str() );

    LSET boardCopper = LSET::AllCuMask( board->GetCopperLayerCount() );

    static const std::vector<KICAD_T> copperTypes = {
        PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_PAD_T, PCB_SHAPE_T, PCB_FIELD_T, PCB_TEXT_T,
        PCB_TEXTBOX_T, PCB_TABLE_T, PCB_TABLECELL_T, PCB_DIMENSION_T, PCB_BARCODE_T
    };

    auto walk =
            [&]( const char* aName, const std::vector<KICAD_T>& aTypes, const LSET& aLayers,
                 int aStopAt )
            {
                std::vector<std::string> seen;
                int                      calls = 0;
                int n = prov.forEachGeometryItem( aTypes, aLayers,
                        [&]( BOARD_ITEM* aItem ) -> bool
                        {
                            seen.push_back( key( aItem ) );
                            return ++calls != aStopAt;
                        } );

                printf( "walk|%s|%d|%s\n", aName, n, join( seen ).c_str() );
            };

    walk( "basic", ORACLE_PROVIDER::Basic(), LSET::AllLayersMask(), -1 );
    walk( "basicbz_cu", ORACLE_PROVIDER::BasicButZones(), LSET::AllCuMask(), -1 );
    walk( "copper", copperTypes, boardCopper, -1 );
    walk( "zones_fcu", { PCB_ZONE_T }, LSET( { F_Cu } ), -1 );
    walk( "trace_via_bcu", { PCB_TRACE_T, PCB_VIA_T }, LSET( { B_Cu } ), -1 );
    walk( "fp_pad", { PCB_FOOTPRINT_T, PCB_PAD_T }, LSET::AllLayersMask(), -1 );
    walk( "stop3", ORACLE_PROVIDER::Basic(), LSET::AllLayersMask(), 3 );

    // The copper tree, as DRC_CACHE_GENERATOR::Run builds it
    std::vector<BOARD_ITEM*> copperItems;

    prov.forEachGeometryItem( copperTypes, boardCopper,
            [&]( BOARD_ITEM* aItem ) -> bool
            {
                copperItems.push_back( aItem );
                return true;
            } );

    DRC_RTREE tree;

    for( BOARD_ITEM* item : copperItems )
    {
        copperLayersOf( item, boardCopper ).RunOnLayers(
                [&]( PCB_LAYER_ID layer )
                {
                    tree.Insert( item, layer, CLEARANCE_CONSTRAINT, WORST );
                } );
    }

    tree.Build();
    printf( "tree|size|%zu|%d\n", tree.size(), tree.empty() ? 1 : 0 );

    boardCopper.RunOnLayers(
            [&]( PCB_LAYER_ID layer )
            {
                std::vector<std::string> list;

                for( DRC_RTREE::ITEM_WITH_SHAPE* e : tree.OnLayer( layer ) )
                    list.push_back( entry( e ) );

                printf( "onlayer|%d|%s\n", (int) layer, join( list ).c_str() );
            } );

    for( BOARD_ITEM* item : copperItems )
    {
        copperLayersOf( item, boardCopper ).RunOnLayers(
                [&]( PCB_LAYER_ID layer )
                {
                    std::string k = key( item );

                    for( int clr : { 0, 150000 } )
                    {
                        std::vector<std::string> hits;
                        int count = tree.QueryColliding( item, layer, layer, nullptr,
                                [&]( BOARD_ITEM* aOther ) -> bool
                                {
                                    hits.push_back( key( aOther ) );
                                    return true;
                                },
                                clr );

                        printf( "qc|%s|%d|%d|%d|%s\n", k.c_str(), (int) layer, clr, count,
                                join( hits ).c_str() );

                        hits.clear();
                        int filterCalls = 0;
                        count = tree.QueryColliding( item, layer, layer,
                                [&]( BOARD_ITEM* aOther ) -> bool
                                {
                                    filterCalls++;
                                    return g_key[aOther] % 3 != 0;
                                },
                                [&]( BOARD_ITEM* aOther ) -> bool
                                {
                                    hits.push_back( key( aOther ) );
                                    return hits.size() < 2;
                                },
                                clr );

                        printf( "qcf|%s|%d|%d|%d|%d|%s\n", k.c_str(), (int) layer, clr, count,
                                filterCalls, join( hits ).c_str() );

                        std::shared_ptr<SHAPE> shape = item->GetEffectiveShape( layer );
                        BOX2I                  box = item->GetBoundingBox();
                        box.Merge( shape->BBox() );
                        box.Inflate( clr );

                        std::vector<std::string> ovl;

                        for( DRC_RTREE::ITEM_WITH_SHAPE* e : tree.Overlapping( layer, box ) )
                            ovl.push_back( entry( e ) );

                        printf( "ovl|%s|%d|%d|%s\n", k.c_str(), (int) layer, clr,
                                join( ovl ).c_str() );

                        bool contained = item->GetBoundingBox().Contains( shape->BBox() );

                        for( bool credit : { false, true } )
                        {
                            if( credit && !contained )
                            {
                                printf( "qp|%s|%d|%d|1|skip\n", k.c_str(), (int) layer, clr );
                                continue;
                            }

                            hits.clear();
                            count = tree.QueryCollidingPreparedCopper( item, shape, layer, nullptr,
                                    [&]( BOARD_ITEM* aOther, const std::shared_ptr<SHAPE>& aShape )
                                    {
                                        hits.push_back( key( aOther ) + "/"
                                                        + std::to_string( (int) aShape->Type() ) );
                                        return true;
                                    },
                                    clr, credit );

                            printf( "qp|%s|%d|%d|%d|%d|%s\n", k.c_str(), (int) layer, clr,
                                    credit ? 1 : 0, count, join( hits ).c_str() );
                        }

                        bool cc = tree.CheckColliding( shape.get(), layer, clr,
                                [&]( BOARD_ITEM* aOther ) -> bool
                                {
                                    return aOther != item;
                                } );

                        printf( "cc|%s|%d|%d|%d\n", k.c_str(), (int) layer, clr, cc ? 1 : 0 );
                    }

                    if( item->Type() == PCB_VIA_T || item->Type() == PCB_PAD_T )
                    {
                        for( int clr : { 0, 100000 } )
                        {
                            std::set<int> at;

                            for( BOARD_ITEM* o : tree.GetObjectsAt( item->GetPosition(), layer, clr ) )
                                at.insert( g_key[o] );

                            std::vector<std::string> list;

                            for( int o : at )
                                list.push_back( "#" + std::to_string( o ) );

                            printf( "at|%s|%d|%d|%s\n", k.c_str(), (int) layer, clr,
                                    join( list ).c_str() );
                        }
                    }
                } );
    }

    std::vector<DRC_RTREE::LAYER_PAIR> layerPairs;

    boardCopper.RunOnLayers(
            [&]( PCB_LAYER_ID layer )
            {
                layerPairs.emplace_back( layer, layer );
            } );

    layerPairs.emplace_back( F_Cu, B_Cu );

    int pairNo = 0;

    tree.QueryCollidingPairs( &tree, layerPairs,
            [&]( const DRC_RTREE::LAYER_PAIR& aPair, DRC_RTREE::ITEM_WITH_SHAPE* aRef,
                 DRC_RTREE::ITEM_WITH_SHAPE* aTest, bool* aCollision ) -> bool
            {
                *aCollision = aRef->shape->Collide( aTest->shape, 0 );
                printf( "pair|%d|%d,%d|%s|%s|%d\n", pairNo++, (int) aPair.first,
                        (int) aPair.second, entry( aRef ).c_str(), entry( aTest ).c_str(),
                        *aCollision ? 1 : 0 );
                return true;
            },
            150000,
            []( int, int ) -> bool
            {
                return true;
            } );

    int progressCalls = 0;
    int visits = 0;

    tree.QueryCollidingPairs( &tree, layerPairs,
            [&]( const DRC_RTREE::LAYER_PAIR&, DRC_RTREE::ITEM_WITH_SHAPE*,
                 DRC_RTREE::ITEM_WITH_SHAPE*, bool* ) -> bool
            {
                return ++visits < 7;
            },
            0,
            [&]( int aDone, int aCount ) -> bool
            {
                progressCalls++;
                return aDone < 9;
            } );

    printf( "pairstop|%d|%d\n", visits, progressCalls );

    // Zone trees (DRC_CACHE_GENERATOR: one per copper zone, triangulated fills)
    std::vector<ZONE*> zones;

    for( ZONE* zone : board->Zones() )
        zones.push_back( zone );

    for( FOOTPRINT* fp : board->Footprints() )
    {
        for( ZONE* zone : fp->Zones() )
            zones.push_back( zone );
    }

    std::map<PCB_LAYER_ID, std::vector<ZONE*>> zonesByLayer;

    for( ZONE* zone : zones )
    {
        if( zone->GetIsRuleArea() || !zone->IsOnCopperLayer() )
            continue;

        zone->CacheBoundingBox();
        zone->CacheTriangulation();

        DRC_RTREE ztree;
        LSET      zoneCu = zone->GetLayerSet() & boardCopper;

        zone->GetLayerSet().RunOnLayers(
                [&]( PCB_LAYER_ID layer )
                {
                    if( IsCopperLayer( layer ) )
                        ztree.Insert( zone, layer, CLEARANCE_CONSTRAINT );
                } );

        ztree.Build();

        for( PCB_LAYER_ID layer : zoneCu )
            zonesByLayer[layer].push_back( zone );

        std::string zk = key( zone );

        zone->GetLayerSet().RunOnLayers(
                [&]( PCB_LAYER_ID layer )
                {
                    if( !IsCopperLayer( layer ) )
                        return;

                    std::vector<std::string> list;

                    for( DRC_RTREE::ITEM_WITH_SHAPE* e : ztree.OnLayer( layer ) )
                        list.push_back( sub( e ) );

                    printf( "ztree|%s|%d|%s\n", zk.c_str(), (int) layer, join( list ).c_str() );

                    for( BOARD_ITEM* item : copperItems )
                    {
                        if( !copperLayersOf( item, boardCopper ).Contains( layer ) )
                            continue;

                        std::shared_ptr<SHAPE> shape = item->GetEffectiveShape( layer );
                        BOX2I                  box = shape->BBox();

                        for( int clr : { 0, 200000 } )
                        {
                            int      actual = -1;
                            VECTOR2I pos;
                            bool     hit = ztree.QueryColliding( box, shape.get(), layer, clr,
                                                                 &actual, &pos );

                            printf( "zq|%s|%s|%d|%d|%d|%d|%d %d\n", zk.c_str(), key( item ).c_str(),
                                    (int) layer, clr, hit ? 1 : 0, hit ? actual : -1,
                                    hit ? pos.x : 0, hit ? pos.y : 0 );
                        }

                        printf( "zqb|%s|%s|%d|%d\n", zk.c_str(), key( item ).c_str(), (int) layer,
                                ztree.QueryColliding( box, shape.get(), layer ) ? 1 : 0 );

                        SHAPE_POLY_SET poly;
                        poly.NewOutline();
                        poly.Append( box.GetX(), box.GetY() );
                        poly.Append( box.GetRight(), box.GetY() );
                        poly.Append( box.GetRight(), box.GetBottom() );
                        poly.Append( box.GetX(), box.GetBottom() );

                        printf( "zqp|%s|%s|%d|%d\n", zk.c_str(), key( item ).c_str(), (int) layer,
                                ztree.QueryColliding( box, &poly, layer ) ? 1 : 0 );
                    }
                } );
    }

    DRC_ZONE_INDEX zoneIndex;
    zoneIndex.Build( zonesByLayer );

    for( const auto& [layer, list] : zonesByLayer )
    {
        printf( "zidx|has|%d|%d\n", (int) layer, zoneIndex.HasLayer( layer ) ? 1 : 0 );

        for( BOARD_ITEM* item : copperItems )
        {
            std::vector<ZONE*>       out;
            std::vector<std::string> keys;
            zoneIndex.Query( layer, item->GetBoundingBox(), out );

            for( ZONE* z : out )
                keys.push_back( key( z ) );

            printf( "zidx|%s|%d|%s\n", key( item ).c_str(), (int) layer, join( keys ).c_str() );
        }
    }

    (void) aGlobal;
}


/// Board independent sections: provider registry, item type lists, formatMsg,
/// CollectOverlappingPairs.
static void runGlobal()
{
    int i = 0;

    for( DRC_TEST_PROVIDER* p : DRC_TEST_PROVIDER_REGISTRY::Instance().GetTestProviders() )
        printf( "registry|%d|%s\n", i++, p->GetName().ToStdString().c_str() );

    DRC_TEST_PROVIDER::Init();

    std::vector<std::string> list;

    for( KICAD_T t : ORACLE_PROVIDER::Basic() )
        list.push_back( std::to_string( (int) t ) );

    printf( "basic|%s\n", join( list ).c_str() );
    list.clear();

    for( KICAD_T t : ORACLE_PROVIDER::BasicButZones() )
        list.push_back( std::to_string( (int) t ) );

    printf( "basicbz|%s\n", join( list ).c_str() );

    ORACLE_PROVIDER prov;
    printf( "name|%s\n", prov.GetName().ToStdString().c_str() );

    const std::vector<std::pair<double, double>> values = {
        { 200000, 150000 }, { 200000, 199999.6 }, { 100000, 100000.4 }, { 0, 0 },
        { 1000000, 2540000 }, { -5, 3 }, { 127000, 127001 }, { 1, 2 }, { 254, 254.4 },
        { 1e9, 1e9 + 1 }, { 3.5e6, 3.5e6 }
    };

    const std::vector<std::pair<EDA_DATA_TYPE, const char*>> dataTypes = {
        { EDA_DATA_TYPE::DISTANCE, "distance" }, { EDA_DATA_TYPE::AREA, "area" },
        { EDA_DATA_TYPE::VOLUME, "volume" }, { EDA_DATA_TYPE::UNITLESS, "unitless" },
        { EDA_DATA_TYPE::TIME, "time" }, { EDA_DATA_TYPE::LENGTH_DELAY, "length_delay" }
    };

    for( EDA_UNITS units : { EDA_UNITS::MM, EDA_UNITS::MILS, EDA_UNITS::INCH, EDA_UNITS::UM,
                             EDA_UNITS::CM, EDA_UNITS::PS, EDA_UNITS::FS,
                             EDA_UNITS::PS_PER_INCH } )
    {
        prov.SetUserUnits( units );

        for( const auto& [type, typeName] : dataTypes )
        {
            for( const auto& [c, a] : values )
            {
                wxString msg = prov.formatMsg( _( "(%s clearance %s; actual %s)" ),
                                               wxT( "netclass 'Default'" ), c, a, type );
                printf( "fmt|%d|%s|%.17g|%.17g|%s\n", (int) units, typeName, c, a,
                        msg.ToStdString( wxConvUTF8 ).c_str() );

                msg = prov.formatMsg( _( "(%s min diameter %s; actual %s on %s)" ),
                                      wxT( "board setup" ), c, a, wxT( "F.Cu" ), type );
                printf( "fmtx|%d|%s|%.17g|%.17g|%s\n", (int) units, typeName, c, a,
                        msg.ToStdString( wxConvUTF8 ).c_str() );
            }
        }

        for( const auto& [c, a] : std::vector<std::pair<double, double>>{
                     { 45, 44.99999 }, { 90, 30 }, { 0, 0 }, { 10.0004, 10.0001 }, { 180, -180 } } )
        {
            wxString msg = prov.formatMsg( _( "(%s max angle %s; actual %s)" ), wxT( "rule 'a'" ),
                                           EDA_ANGLE( c, DEGREES_T ), EDA_ANGLE( a, DEGREES_T ) );
            printf( "fmta|%d|%.17g|%.17g|%s\n", (int) units, c, a,
                    msg.ToStdString( wxConvUTF8 ).c_str() );
        }
    }

    // CollectOverlappingPairs on deterministic pseudo-random boxes (LCG)
    for( int seed : { 1, 2, 3 } )
    {
        uint32_t state = (uint32_t) seed * 2654435761u;

        auto next =
                [&]( int aMod ) -> int
                {
                    state = state * 1664525u + 1013904223u;
                    return (int) ( ( state >> 8 ) % (uint32_t) aMod );
                };

        int                        n = 20 + 25 * seed;
        std::vector<BOX2I>         boxes;
        std::vector<bool>          has;
        std::vector<size_t>        order;

        for( int b = 0; b < n; ++b )
        {
            int x = next( 1000 ) - 200;
            int y = next( 1000 ) - 200;
            int w = next( 120 ) - 20;
            int h = next( 120 ) - 20;
            boxes.emplace_back( VECTOR2I( x, y ), VECTOR2L( w, h ) );
            has.push_back( b % 7 != 3 );
        }

        // a degenerate origin box, twice
        boxes.emplace_back( VECTOR2I( 0, 0 ), VECTOR2L( 0, 0 ) );
        has.push_back( true );
        boxes.emplace_back( VECTOR2I( 0, 0 ), VECTOR2L( 0, 0 ) );
        has.push_back( true );

        for( size_t b = 0; b < boxes.size(); ++b )
            order.push_back( ( b * 7 + (size_t) seed ) % boxes.size() );

        std::vector<std::pair<size_t, size_t>> pairs = CollectOverlappingPairs( order,
                [&]( size_t aIdx ) -> std::optional<BOX2I>
                {
                    if( !has[aIdx] )
                        return std::nullopt;

                    return boxes[aIdx];
                } );

        std::vector<std::string> out;

        for( const auto& [a, b] : pairs )
            out.push_back( std::to_string( a ) + "-" + std::to_string( b ) );

        printf( "cop|%d|%zu|%s\n", seed, boxes.size(), join( out ).c_str() );
    }
}


/// FNV-1a 64 of a string (keeps the zone tree entry lists of large fills small).
static std::string fnv( const std::string& aText )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aText )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    char buf[20];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) h );
    return buf;
}


static std::string zoneKeys( const std::vector<ZONE*>& aZones )
{
    std::vector<std::string> list;

    for( ZONE* zone : aZones )
        list.push_back( key( zone ) );

    return join( list );
}


/// DRC-05: the BOARD caches after DRC_CACHE_GENERATOR::Run() on a board loaded like
/// KI_TEST::LoadBoard.
static void runCacheGen( const std::string& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    std::string       base = aPath.substr( 0, aPath.rfind( '.' ) );
    wxFileName        projectFile( base + ".kicad_pro" );
    wxFileName        rulesFile( base + ".kicad_dru" );

    if( projectFile.Exists() )
        mgr.LoadProject( projectFile.GetFullPath() );

    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( aPath );

    std::string name = aPath.substr( aPath.find_last_of( '/' ) + 1 );
    printf( "board|%s\n", name.c_str() );

    if( projectFile.Exists() )
        board->SetProject( &mgr.Prj() );

    auto engine = std::make_shared<DRC_ENGINE>( board.get(), &board->GetDesignSettings() );

    try
    {
        engine->InitEngine( rulesFile.Exists() ? rulesFile : wxFileName() );
    }
    catch( ... )
    {
        printf( "cg|init_error\n" );
    }

    board->GetDesignSettings().m_DRCEngine = engine;
    board->BuildListOfNets();
    board->BuildConnectivity();

    ORACLE_PROVIDER prov;
    prov.SetDRCEngine( engine.get() );

    g_key.clear();

    std::vector<std::string> types;

    prov.forEachGeometryItem( {}, LSET::AllLayersMask(),
            [&]( BOARD_ITEM* aItem ) -> bool
            {
                int ord = (int) g_key.size();
                g_key[aItem] = ord;
                types.push_back( std::to_string( (int) aItem->Type() ) );
                return true;
            } );

    printf( "cg|keys|%s\n", join( types ).c_str() );

    DRC_CACHE_GENERATOR gen;
    gen.SetDRCEngine( engine.get() );
    bool ok = gen.Run();

    printf( "cg|run|%d\n", ok ? 1 : 0 );
    printf( "cg|bds_biggest|%d\n", board->GetDesignSettings().GetBiggestClearanceValue() );
    printf( "cg|board_max|%d\n", board->GetMaxClearanceValue() );
    printf( "cg|max_clearance|%d\n", board->m_DRCMaxClearance );
    printf( "cg|max_physical|%d\n", board->m_DRCMaxPhysicalClearance );
    printf( "cg|zones|%s\n", zoneKeys( board->m_DRCZones ).c_str() );
    printf( "cg|copper_zones|%s\n", zoneKeys( board->m_DRCCopperZones ).c_str() );

    for( const auto& [layer, zones] : board->m_DRCCopperZonesByLayer )
        printf( "cg|zones_by_layer|%d|%s\n", (int) layer, zoneKeys( zones ).c_str() );

    std::shared_ptr<DRC_RTREE> tree = board->m_CopperItemRTreeCache;
    LSET boardCopper = LSET::AllCuMask( board->GetCopperLayerCount() );

    printf( "cg|tree|%zu|%d\n", tree->size(), tree->empty() ? 1 : 0 );

    boardCopper.RunOnLayers(
            [&]( PCB_LAYER_ID layer )
            {
                std::vector<std::string> list;

                for( DRC_RTREE::ITEM_WITH_SHAPE* e : tree->OnLayer( layer ) )
                    list.push_back( entry( e ) );

                printf( "cg|onlayer|%d|%s\n", (int) layer, join( list ).c_str() );
            } );

    std::vector<ZONE*> allZones( board->Zones().begin(), board->Zones().end() );

    for( FOOTPRINT* fp : board->Footprints() )
        allZones.insert( allZones.end(), fp->Zones().begin(), fp->Zones().end() );

    for( ZONE* zone : allZones )
    {
        printf( "cg|zbbox|%s|%s\n", key( zone ).c_str(), boxStr( zone->GetBoundingBox() ).c_str() );

        DRC_RTREE* ztree = board->GetCopperZoneRTree( zone );

        if( !ztree )
            continue;

        printf( "cg|ztree|%s|%zu\n", key( zone ).c_str(), ztree->size() );

        for( int l = 0; l < PCB_LAYER_ID_COUNT; ++l )
        {
            std::vector<std::string> list;

            for( DRC_RTREE::ITEM_WITH_SHAPE* e : ztree->OnLayer( (PCB_LAYER_ID) l ) )
                list.push_back( entry( e ) );

            if( !list.empty() )
            {
                printf( "cg|ztree_layer|%s|%d|%zu|%s\n", key( zone ).c_str(), l, list.size(),
                        fnv( join( list ) ).c_str() );
            }
        }
    }

    for( ZONE* zone : board->Zones() )
    {
        auto it = board->m_ZoneIsolatedIslandsMap.find( zone );

        if( it == board->m_ZoneIsolatedIslandsMap.end() )
            continue;

        std::vector<std::string> layers;

        for( const auto& [layer, islands] : it->second )
            layers.push_back( std::to_string( (int) layer ) );

        printf( "cg|islands|%s|%s\n", key( zone ).c_str(), join( layers ).c_str() );
    }

    board->GetDesignSettings().m_DRCEngine = nullptr;
    board->SetProject( nullptr );
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    std::vector<std::string> files;
    bool                     global = false;
    bool                     cachegen = false;

    for( int i = 1; i < argc; ++i )
    {
        std::string a = argv[i];

        if( a == "--global" )
            global = true;
        else if( a == "--cachegen" )
            cachegen = true;
        else
            files.push_back( a );
    }

    if( !initPgm( argc, argv ) )
        return 2;

    if( global )
        runGlobal();

    for( const std::string& f : files )
    {
        if( cachegen )
            runCacheGen( f );
        else
            runBoard( f, global );
    }

    fflush( stdout );
    _exit( 0 );
}
