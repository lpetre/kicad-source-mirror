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

// pcb-dump value formatting (the canonical forms of rust/docs/PCB_DUMP_FORMAT.md).

#include "pcb_dump.h"

#include <csignal>
#include <cstdio>
#include <map>

#include <board.h>
#include <board_item.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <pcb_table.h>
#include <pcb_tablecell.h>
#include <pcb_group.h>
#include <pcb_generator.h>
#include <pcb_marker.h>
#include <pcb_point.h>
#include <zone.h>
#include <geometry/shape.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_compound.h>
#include <geometry/shape_ellipse.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <geometry/shape_simple.h>


DUMP_OUT g_out;

std::vector<sigjmp_buf*> g_crashJumps;


static void crashHandler( int aSig )
{
    if( !g_crashJumps.empty() )
        siglongjmp( *g_crashJumps.back(), 1 );

    signal( aSig, SIG_DFL );
    raise( aSig );
}


void InstallCrashGuard()
{
    struct sigaction sa = {};
    sa.sa_handler = crashHandler;
    sigemptyset( &sa.sa_mask );
    sa.sa_flags = SA_NODEFER;
    sigaction( SIGSEGV, &sa, nullptr );
    sigaction( SIGBUS, &sa, nullptr );
    sigaction( SIGFPE, &sa, nullptr );
}


uint64_t Fnv1a64( const std::string& aData )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aData )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


void DUMP_OUT::Line( const std::string& aSection, const std::string& aKey,
                     const std::string& aField, const std::string& aValue )
{
    std::string line;
    line.reserve( aSection.size() + aKey.size() + aField.size() + aValue.size() + 4 );
    line += aSection;
    line += '|';
    line += aKey;
    line += '|';
    line += aField;
    line += '=';

    if( m_hash )
    {
        char buf[24];
        snprintf( buf, sizeof( buf ), "#%016llx", (unsigned long long) Fnv1a64( aValue ) );
        line += buf;
    }
    else
    {
        line += aValue;
    }

    line += '\n';
    fwrite( line.data(), 1, line.size(), stdout );
}


void DUMP_OUT::Error( const std::string& aSection, const std::string& aKey, const std::string& aMsg )
{
    std::string msg;

    for( char c : aMsg )
        msg += ( c == '\n' || c == '\r' ) ? ' ' : c;

    std::string line = "error|" + aSection + "|" + aKey + "|" + msg + "\n";
    fwrite( line.data(), 1, line.size(), stdout );
}


std::string V( int aVal ) { return std::to_string( aVal ); }
std::string V( unsigned aVal ) { return std::to_string( aVal ); }
std::string V( long aVal ) { return std::to_string( aVal ); }
std::string V( long long aVal ) { return std::to_string( aVal ); }
std::string V( unsigned long aVal ) { return std::to_string( aVal ); }
std::string V( unsigned long long aVal ) { return std::to_string( aVal ); }
std::string V( bool aVal ) { return aVal ? "true" : "false"; }


std::string V( double aVal )
{
    if( aVal == 0.0 )
        return "0";     // also folds -0

    char buf[40];
    snprintf( buf, sizeof( buf ), "%.17g", aVal );
    return buf;
}


std::string V( const std::string& aVal )
{
    std::string r = "\"";

    for( unsigned char c : aVal )
    {
        switch( c )
        {
        case '\\': r += "\\\\"; break;
        case '"':  r += "\\\""; break;
        case '\n': r += "\\n"; break;
        case '\r': r += "\\r"; break;
        case '\t': r += "\\t"; break;
        default:
            if( c < 0x20 || c == 0x7f )
            {
                char buf[8];
                snprintf( buf, sizeof( buf ), "\\x%02x", c );
                r += buf;
            }
            else
            {
                r += (char) c;
            }
        }
    }

    r += "\"";
    return r;
}


std::string V( const char* aVal ) { return V( std::string( aVal ) ); }
std::string V( const wxString& aVal ) { return V( std::string( aVal.ToUTF8() ) ); }

std::string V( const VECTOR2I& aVal )
{
    return std::to_string( aVal.x ) + "," + std::to_string( aVal.y );
}

std::string V( const VECTOR2D& aVal ) { return V( aVal.x ) + "," + V( aVal.y ); }

std::string V( const BOX2I& aVal )
{
    return V( aVal.GetPosition() ) + "," + V( VECTOR2I( aVal.GetSize().x, aVal.GetSize().y ) );
}

std::string V( const EDA_ANGLE& aVal ) { return V( aVal.AsDegrees() ); }

std::string V( const LSET& aVal )
{
    size_t nbits = std::max<size_t>( PCB_LAYER_ID_COUNT, aVal.size() );
    nbits = ( nbits + 3 ) / 4 * 4;
    std::string r;

    for( size_t nib = nbits / 4; nib-- > 0; )
    {
        int v = 0;

        for( int b = 3; b >= 0; --b )
        {
            size_t bit = nib * 4 + b;
            v = v * 2 + ( ( bit < aVal.size() && aVal.test( bit ) ) ? 1 : 0 );
        }

        r += "0123456789abcdef"[v];
    }

    return r;
}

std::string V( const KIID& aVal ) { return aVal.AsStdString(); }


std::string VLayer( int aLayer )
{
    if( aLayer == UNDEFINED_LAYER )
        return "UNDEFINED_LAYER";

    if( aLayer == UNSELECTED_LAYER )
        return "UNSELECTED_LAYER";

    if( aLayer < 0 || aLayer >= PCB_LAYER_ID_COUNT )
        return "?" + std::to_string( aLayer );

    wxString n = LSET::Name( ToLAYER_ID( aLayer ) );

    if( n.IsEmpty() )
        return "?" + std::to_string( aLayer );

    return std::string( n.ToUTF8() );
}


std::string VLayers( const std::vector<PCB_LAYER_ID>& aLayers )
{
    std::string r = "[";

    for( size_t i = 0; i < aLayers.size(); ++i )
    {
        if( i )
            r += ",";

        r += VLayer( aLayers[i] );
    }

    return r + "]";
}


std::string VLayerNames( const LSET& aSet )
{
    std::vector<PCB_LAYER_ID> layers;

    for( size_t i = 0; i < aSet.size() && i < PCB_LAYER_ID_COUNT; ++i )
    {
        if( aSet.test( i ) )
            layers.push_back( ToLAYER_ID( (int) i ) );
    }

    return VLayers( layers );
}


std::string VPoints( const std::vector<VECTOR2I>& aPts )
{
    std::string r = "[";

    for( size_t i = 0; i < aPts.size(); ++i )
    {
        if( i )
            r += " ";

        r += V( aPts[i] );
    }

    return r + "]";
}


std::string VArc( const SHAPE_ARC& aArc )
{
    return "A(" + V( aArc.GetP0() ) + " " + V( aArc.GetArcMid() ) + " " + V( aArc.GetP1() )
           + " w=" + V( aArc.GetWidth() ) + ")";
}


std::string VChain( const SHAPE_LINE_CHAIN& aChain )
{
    std::string r = "C(";
    r += aChain.IsClosed() ? "closed" : "open";
    r += " w=" + V( aChain.Width() );
    r += " n=" + V( aChain.PointCount() ) + " ";
    r += VPoints( aChain.CPoints() );

    if( aChain.ArcCount() > 0 )
    {
        r += " arcs=[";

        for( size_t i = 0; i < aChain.ArcCount(); ++i )
        {
            if( i )
                r += " ";

            r += VArc( aChain.Arc( i ) );
        }

        r += "] shapes=[";

        const auto& shapes = aChain.CShapes();

        for( size_t i = 0; i < shapes.size(); ++i )
        {
            if( i )
                r += " ";

            r += std::to_string( shapes[i].first );

            if( shapes[i].second >= 0 )
                r += "/" + std::to_string( shapes[i].second );
        }

        r += "]";
    }

    return r + ")";
}


std::string VPolySet( const SHAPE_POLY_SET& aSet )
{
    std::string r = "P(" + V( aSet.OutlineCount() );

    for( int i = 0; i < aSet.OutlineCount(); ++i )
    {
        r += " {";
        r += VChain( aSet.COutline( i ) );

        for( int h = 0; h < aSet.HoleCount( i ); ++h )
            r += " hole " + VChain( aSet.CHole( i, h ) );

        r += "}";
    }

    return r + ")";
}


std::string VShape( const SHAPE* aShape )
{
    if( !aShape )
        return "null";

    switch( aShape->Type() )
    {
    case SH_RECT:
    {
        auto s = static_cast<const SHAPE_RECT*>( aShape );
        return "SH_RECT(p=" + V( s->GetPosition() ) + " size=" + V( s->GetSize() )
               + " r=" + V( s->GetRadius() ) + " w=" + V( s->GetWidth() ) + ")";
    }
    case SH_SEGMENT:
    {
        auto s = static_cast<const SHAPE_SEGMENT*>( aShape );
        return "SH_SEGMENT(a=" + V( s->GetSeg().A ) + " b=" + V( s->GetSeg().B )
               + " w=" + V( s->GetWidth() ) + ")";
    }
    case SH_LINE_CHAIN:
        return "SH_LINE_CHAIN(" + VChain( *static_cast<const SHAPE_LINE_CHAIN*>( aShape ) ) + ")";
    case SH_CIRCLE:
    {
        auto s = static_cast<const SHAPE_CIRCLE*>( aShape );
        return "SH_CIRCLE(c=" + V( s->GetCenter() ) + " r=" + V( s->GetRadius() ) + ")";
    }
    case SH_SIMPLE:
    {
        auto s = static_cast<const SHAPE_SIMPLE*>( aShape );
        return "SH_SIMPLE(" + VChain( s->Vertices() ) + ")";
    }
    case SH_POLY_SET:
        return "SH_POLY_SET(" + VPolySet( *static_cast<const SHAPE_POLY_SET*>( aShape ) ) + ")";
    case SH_COMPOUND:
    {
        auto        s = static_cast<const SHAPE_COMPOUND*>( aShape );
        std::string r = "SH_COMPOUND(" + V( (int) s->Shapes().size() );

        for( const SHAPE* sub : s->Shapes() )
            r += " " + VShape( sub );

        return r + ")";
    }
    case SH_ARC:
        return "SH_ARC(" + VArc( *static_cast<const SHAPE_ARC*>( aShape ) ) + ")";
    case SH_ELLIPSE:
    {
        auto s = static_cast<const SHAPE_ELLIPSE*>( aShape );
        return "SH_ELLIPSE(c=" + V( s->GetCenter() ) + " major=" + V( s->GetMajorRadius() )
               + " minor=" + V( s->GetMinorRadius() ) + " rot=" + V( s->GetRotation() )
               + " start=" + V( s->GetStartAngle() ) + " end=" + V( s->GetEndAngle() )
               + " arc=" + V( s->IsArc() ) + ")";
    }
    case SH_NULL:
        return "SH_NULL()";
    default:
        return std::string( SHAPE_TYPE_asString( aShape->Type() ).ToUTF8() ) + "(?)";
    }
}


// ---- item keys ----------------------------------------------------------------------------

static std::map<const BOARD_ITEM*, std::string> s_keys;
static std::set<std::string>                    s_fileUuids;


void ScanFileUuids( const std::string& aPath )
{
    s_fileUuids.clear();
    FILE* f = fopen( aPath.c_str(), "rb" );

    if( !f )
        return;

    std::string text;
    char        buf[65536];
    size_t      n;

    while( ( n = fread( buf, 1, sizeof( buf ), f ) ) > 0 )
        text.append( buf, n );

    fclose( f );

    auto isHex = []( char c ) { return isxdigit( (unsigned char) c ) != 0; };
    size_t i = 0;

    while( i < text.size() )
    {
        if( !isHex( text[i] ) && text[i] != '-' )
        {
            i++;
            continue;
        }

        size_t j = i;

        while( j < text.size() && ( isHex( text[j] ) || text[j] == '-' ) )
            j++;

        std::string tok = text.substr( i, j - i );

        for( char& c : tok )
            c = tolower( (unsigned char) c );

        if( tok.size() == 36 && tok[8] == '-' && tok[13] == '-' && tok[18] == '-' && tok[23] == '-' )
            s_fileUuids.insert( tok );
        else if( tok.size() <= 8 && tok.find( '-' ) == std::string::npos )
            s_fileUuids.insert( "00000000-0000-0000-0000-" + std::string( 12 - tok.size(), '0' ) + tok );

        i = j;
    }
}


bool UuidInFile( const KIID& aId )
{
    return s_fileUuids.count( aId.AsStdString() ) > 0;
}


std::string ItemKey( const BOARD_ITEM* aItem )
{
    if( !aItem )
        return "null";

    auto it = s_keys.find( aItem );

    if( it != s_keys.end() )
        return it->second;

    return "unkeyed";
}


/// Visit every item of the board in dump order (see PCB_DUMP_FORMAT.md, "Item order").
void ForEachBoardItem( BOARD* aBoard, const std::function<void( BOARD_ITEM* )>& aFn )
{
    std::function<void( BOARD_ITEM* )> visit = [&]( BOARD_ITEM* aItem )
    {
        aFn( aItem );

        if( aItem->Type() == PCB_FOOTPRINT_T )
        {
            FOOTPRINT* fp = static_cast<FOOTPRINT*>( aItem );

            for( PCB_FIELD* f : fp->GetFields() )
                visit( f );

            for( PAD* p : fp->Pads() )
                visit( p );

            for( BOARD_ITEM* d : fp->GraphicalItems() )
                visit( d );

            for( ZONE* z : fp->Zones() )
                visit( z );

            for( PCB_GROUP* g : fp->Groups() )
                visit( g );

            for( PCB_POINT* p : fp->Points() )
                visit( p );
        }
        else if( aItem->Type() == PCB_TABLE_T )
        {
            for( PCB_TABLECELL* c : static_cast<PCB_TABLE*>( aItem )->GetCells() )
                visit( c );
        }
    };

    for( FOOTPRINT* fp : aBoard->Footprints() )
        visit( fp );

    for( BOARD_ITEM* d : aBoard->Drawings() )
        visit( d );

    for( PCB_TRACK* t : aBoard->Tracks() )
        visit( t );

    for( ZONE* z : aBoard->Zones() )
        visit( z );

    for( PCB_GENERATOR* g : aBoard->Generators() )
        visit( g );

    for( PCB_GROUP* g : aBoard->Groups() )
        visit( g );

    for( PCB_POINT* p : aBoard->Points() )
        visit( p );

    for( PCB_MARKER* m : aBoard->Markers() )
        visit( m );
}


void InitItemKeys( BOARD* aBoard )
{
    s_keys.clear();
    std::map<KIID, int> counts;

    ForEachBoardItem( aBoard, [&]( BOARD_ITEM* aItem ) { counts[aItem->m_Uuid]++; } );

    int ordinal = 0;

    ForEachBoardItem( aBoard,
                      [&]( BOARD_ITEM* aItem )
                      {
                          if( aItem->m_Uuid == niluuid || counts[aItem->m_Uuid] > 1
                              || !UuidInFile( aItem->m_Uuid ) )
                              s_keys[aItem] = "#" + std::to_string( ordinal );
                          else
                              s_keys[aItem] = V( aItem->m_Uuid );

                          ordinal++;
                      } );
}


std::vector<PCB_LAYER_ID> CopperLayers( const BOARD* aBoard )
{
    std::vector<PCB_LAYER_ID> r;

    for( PCB_LAYER_ID l : LSET::AllCuMask( aBoard ? aBoard->GetCopperLayerCount() : 2 ).CuStack() )
        r.push_back( l );

    return r;
}
