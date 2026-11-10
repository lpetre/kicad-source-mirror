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
 * Package LIVE-1, verification b: incremental connectivity of the reference under scripted
 * edit batches, applied the way BOARD_COMMIT::Push does it for a headless board editor
 * commit (pcbnew/board_commit.cpp):
 *
 *   CHT_ADD     board->Add( item, ADD_MODE::BULK_INSERT )   (calls CONNECTIVITY_DATA::Add)
 *   CHT_REMOVE  board->Remove( item, REMOVE_MODE::BULK )    (calls CONNECTIVITY_DATA::Remove)
 *   CHT_MODIFY  copy = Clone(); edit; MarkItemNetAsDirty( copy ); Update( item )
 *   end         RecalculateRatsnest(); UpdateRatsnestExclusions()
 *
 * The board is read as KI_TEST::ReadBoardFromFileOrStream does and built with
 * BOARD::BuildConnectivity().  After the load and after every commit ("C" line of the script)
 * the connectivity facts are printed as one FNV-1a hash per section ("<commit>|<section>|<hash>",
 * the rn_edges lines as they are), or all lines as they are with LIVE1_LINES=1.  The facts and the script format are those
 * of tests/live1_common/mod.rs (fact_lines, Op::line), the Rust replay is
 * tests/live1_oracle.rs.  Built and run by gen_live1_golden.sh.
 *
 *   live1_oracle BOARD SCRIPT
 */

#include "pcb_dump.h"

#include <algorithm>
#include <climits>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <sstream>
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
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <zone.h>
#include <connectivity/connectivity_data.h>
#include <connectivity/connectivity_algo.h>
#include <connectivity/connectivity_items.h>
#include <ratsnest/ratsnest_data.h>


namespace
{

struct RN_NET_PEEK : public RN_NET
{
    using NODES = decltype( RN_NET_PEEK::m_nodes );

    static const NODES& Nodes( const RN_NET* aNet )
    {
        static constexpr NODES RN_NET::*nodes = &RN_NET_PEEK::m_nodes;
        return aNet->*nodes;
    }
};


std::string Uuid( const BOARD_ITEM* aItem )
{
    return aItem ? aItem->m_Uuid.AsString().ToStdString() : std::string( "null" );
}


std::string CnRef( const CN_ITEM* aItem )
{
    if( !aItem )
        return "null";

    if( !aItem->Valid() )
        return "invalid";

    std::string r = Uuid( aItem->Parent() );

    if( const CN_ZONE_LAYER* zl = dynamic_cast<const CN_ZONE_LAYER*>( aItem ) )
        r += "/" + std::to_string( (int) aItem->GetBoardLayer() ) + "/"
             + std::to_string( zl->SubpolyIndex() );

    return r;
}


std::string Pos( const VECTOR2I& aPos )
{
    return std::to_string( aPos.x ) + "," + std::to_string( aPos.y );
}


std::string AnchorText( const CN_ANCHOR& aAnchor )
{
    return Pos( aAnchor.Pos() ) + "@" + CnRef( aAnchor.Item() );
}


/// An edge end: the position, "/z" when the anchor belongs to a zone layer item.
std::string EndText( const std::shared_ptr<const CN_ANCHOR>& aNode )
{
    if( !aNode )
        return "none";

    bool zone = dynamic_cast<const CN_ZONE_LAYER*>( aNode->Item() ) != nullptr;
    return Pos( aNode->Pos() ) + ( zone ? "/z" : "" );
}


std::string Join( std::vector<std::string> aParts )
{
    std::sort( aParts.begin(), aParts.end() );
    std::string r;

    for( size_t i = 0; i < aParts.size(); i++ )
        r += ( i ? " " : "" ) + aParts[i];

    return r;
}


/// The facts of tests/live1_common/mod.rs fact_lines(), sorted.
std::vector<std::string> FactLines( BOARD* aBoard )
{
    std::shared_ptr<CONNECTIVITY_DATA> conn = aBoard->GetConnectivity();
    std::vector<std::string> out;
    std::vector<PAD*> pads;

    for( PCB_TRACK* t : aBoard->Tracks() )
        out.push_back( "net|" + Uuid( t ) + "|" + std::to_string( t->GetNetCode() ) );

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
            pads.push_back( pad );
    }

    for( PAD* pad : pads )
        out.push_back( "net|" + Uuid( pad ) + "|" + std::to_string( pad->GetNetCode() ) );

    for( const std::shared_ptr<CN_CLUSTER>& cl : conn->GetConnectivityAlgo()->GetClusters() )
    {
        std::vector<std::string> members;

        for( CN_ITEM* it : *cl )
            members.push_back( CnRef( it ) );

        out.push_back( "cluster|" + std::to_string( cl->OriginNet() ) + "|" + Join( members ) );
    }

    for( int net = 1; net < conn->GetNetCount(); net++ )
    {
        RN_NET* rn = conn->GetRatsnestForNet( net );

        if( !rn || rn->GetNodeCount() == 0 )
            continue;

        std::vector<std::string> nodes;

        for( const std::shared_ptr<CN_ANCHOR>& a : RN_NET_PEEK::Nodes( rn ) )
            nodes.push_back( AnchorText( *a ) );

        out.push_back( "rn_nodes|" + std::to_string( net ) + "|" + Join( nodes ) );

        std::vector<std::string> edges;

        for( const CN_EDGE& e : rn->GetEdges() )
        {
            std::string a = EndText( e.GetSourceNode() );
            std::string b = EndText( e.GetTargetNode() );

            if( b < a )
                std::swap( a, b );

            edges.push_back( a + ">" + b + ":" + std::to_string( e.GetWeight() ) );
        }

        out.push_back( "rn_edges|" + std::to_string( net ) + "|" + Join( edges ) );
    }

    out.push_back( "unconnected|" + std::to_string( conn->GetUnconnectedCount( false ) ) + "|"
                   + std::to_string( conn->GetUnconnectedCount( true ) ) );

    LSEQ cu = LSET::AllCuMask( aBoard->GetCopperLayerCount() ).CuStack();
    std::initializer_list<KICAD_T> nonZone = { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_PAD_T };

    for( PCB_TRACK* t : aBoard->Tracks() )
    {
        VECTOR2I pos( INT_MIN, INT_MIN );
        bool     dangling = conn->TestTrackEndpointDangling( t, false, &pos );

        out.push_back( "dangling|" + Uuid( t ) + "|" + std::to_string( (int) dangling ) + "|"
                       + ( pos.x == INT_MIN && pos.y == INT_MIN ? std::string( "unset" )
                                                                : Pos( pos ) ) );

        if( t->Type() == PCB_VIA_T )
        {
            for( PCB_LAYER_ID l : cu )
            {
                if( t->IsOnLayer( l ) )
                {
                    bool on = conn->IsConnectedOnLayer( t, l, nonZone );
                    out.push_back( "conn_on_layer|" + Uuid( t ) + "|" + std::to_string( (int) l )
                                   + "|" + std::to_string( (int) on ) );
                }
            }
        }
    }

    for( PAD* pad : pads )
    {
        for( PCB_LAYER_ID l : cu )
        {
            if( pad->IsOnLayer( l ) )
            {
                bool on = conn->IsConnectedOnLayer( pad, l, nonZone );
                out.push_back( "conn_on_layer|" + Uuid( pad ) + "|" + std::to_string( (int) l )
                               + "|" + std::to_string( (int) on ) );
            }
        }
    }

    std::sort( out.begin(), out.end() );
    return out;
}


uint64_t Fnv( const std::string& aText )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aText )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


void Print( int aCommit, BOARD* aBoard, bool aLines )
{
    std::vector<std::string> lines = FactLines( aBoard );

    if( aLines )
    {
        for( const std::string& l : lines )
            printf( "%d|%s\n", aCommit, l.c_str() );

        return;
    }

    std::map<std::string, std::string> sections;

    for( const std::string& l : lines )
    {
        // The ratsnest edges are printed as they are: their ends follow heap order under ties
        // (review-connectivity.md F-1), the replay compares them against the tie candidates.
        if( l.rfind( "rn_edges|", 0 ) == 0 )
            printf( "%d|%s\n", aCommit, l.c_str() );
        else
            sections[l.substr( 0, l.find( '|' ) )] += l + "\n";
    }

    for( const auto& [name, text] : sections )
        printf( "%d|%s|%016llx\n", aCommit, name.c_str(), (unsigned long long) Fnv( text ) );
}


int g_asserts = 0;

void countAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
    g_asserts++;
}

} // namespace


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 3 )
    {
        fprintf( stderr, "usage: live1_oracle BOARD SCRIPT\n" );
        return 2;
    }

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
    wxSetAssertHandler( &countAssert );

    bool lines = getenv( "LIVE1_LINES" ) != nullptr;

    KIID::SeedGenerator( 5489 );
    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( argv[1] );

    if( !board )
        return 1;

    board->BuildConnectivity();
    std::shared_ptr<CONNECTIVITY_DATA> conn = board->GetConnectivity();

    std::map<std::string, PCB_TRACK*> byUuid;

    for( PCB_TRACK* t : board->Tracks() )
        byUuid[Uuid( t )] = t;

    // Removed items stay allocated (the undo list owns them in pcbnew).
    std::vector<std::unique_ptr<BOARD_ITEM>> removed;

    int commit = 0;
    Print( commit, board.get(), lines );

    std::ifstream script( argv[2] );
    std::string   line;

    while( std::getline( script, line ) )
    {
        std::istringstream in( line );
        std::string        op, uuid;
        in >> op;

        if( op == "F" )
        {
            // a full rebuild, printed as the next commit
            board->BuildConnectivity();
            conn = board->GetConnectivity();
            Print( ++commit, board.get(), lines );
            continue;
        }

        if( op == "C" )
        {
            conn->RecalculateRatsnest( nullptr );
            board->UpdateRatsnestExclusions();
            Print( ++commit, board.get(), lines );
            continue;
        }

        in >> uuid;

        if( op == "R" )
        {
            PCB_TRACK* t = byUuid.at( uuid );
            board->Remove( t, REMOVE_MODE::BULK );
            byUuid.erase( uuid );
            removed.emplace_back( t );
        }
        else if( op == "T" )
        {
            int x1, y1, x2, y2, w, layer, net;
            in >> x1 >> y1 >> x2 >> y2 >> w >> layer >> net;

            PCB_TRACK* t = new PCB_TRACK( board.get() );
            const_cast<KIID&>( t->m_Uuid ) = KIID( uuid );
            t->SetStart( VECTOR2I( x1, y1 ) );
            t->SetEnd( VECTOR2I( x2, y2 ) );
            t->SetWidth( w );
            t->SetLayer( (PCB_LAYER_ID) layer );
            t->SetNetCode( net );
            board->Add( t, ADD_MODE::BULK_INSERT );
            byUuid[uuid] = t;
        }
        else if( op == "V" )
        {
            int x, y, d, drill, net, free;
            in >> x >> y >> d >> drill >> net >> free;

            PCB_VIA* v = new PCB_VIA( board.get() );
            const_cast<KIID&>( v->m_Uuid ) = KIID( uuid );
            v->SetPosition( VECTOR2I( x, y ) );
            v->SetViaType( VIATYPE::THROUGH );
            v->SetLayerPair( F_Cu, B_Cu );
            v->SetWidth( PADSTACK::ALL_LAYERS, d );
            v->SetDrill( drill );
            v->SetNetCode( net );
            v->SetIsFree( free != 0 );
            board->Add( v, ADD_MODE::BULK_INSERT );
            byUuid[uuid] = v;
        }
        else if( op == "M" || op == "N" )
        {
            PCB_TRACK* t = byUuid.at( uuid );
            std::unique_ptr<BOARD_ITEM> copy( static_cast<BOARD_ITEM*>( t->Clone() ) );

            if( op == "M" )
            {
                int x, y;
                in >> x >> y;

                if( t->Type() == PCB_VIA_T )
                    t->SetPosition( VECTOR2I( x, y ) );
                else
                    t->SetStart( VECTOR2I( x, y ) );
            }
            else
            {
                int net;
                in >> net;
                t->SetNetCode( net, false );
            }

            conn->MarkItemNetAsDirty( copy.get() );
            conn->Update( t );
        }
        else if( !op.empty() )
        {
            fprintf( stderr, "bad script line: %s\n", line.c_str() );
            return 1;
        }
    }

    // The removed items must outlive the board's connectivity (CN_ITEM parents)
    board.reset();
    return 0;
}
