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

// Differential-test oracle for the kicad-core Rust port.
//
// This program is built against the real KiCad sources and prints a deterministic text dump
// (tests/golden/core_oracle_dump.txt) that rust/crates/kicad-core/tests/test_cpp_oracle.rs
// regenerates with the Rust port and compares line by line.
//
// Build and regenerate with tests/oracle/build_oracle.sh (needs wxWidgets and Boost
// development files; the dump in the repository was made with wxWidgets 3.2.4 and Boost 1.83
// on x86-64 Linux).
//
// Inputs that trigger undefined behaviour in the C++ code (out of range bit tests, the
// copper layer iteration of odd sized sets, LAYER_RANGEs whose stop layer is not part of the
// stack, KIID( timestamp_t ) with Boost < 1.86) are deliberately left out.
#include <cstdio>
#include <cstdint>
#include <string>
#include <vector>
#include <algorithm>
#include <climits>
#include <clocale>
#include <cstdarg>

#include <wx/string.h>
#include <wx/debug.h>

#include <layer_ids.h>
#include <layer_range.h>
#include <lseq.h>
#include <lset.h>
#include <base_set.h>
#include <kiid.h>
#include <lib_id.h>
#include <name_validation.h>
#include <ki_exception.h>

static std::string U( const wxString& s ) { return std::string( s.utf8_str().data() ); }

// ---- shared helpers (mirrored in Rust) ----
struct LCG
{
    uint64_t s;
    explicit LCG( uint64_t seed ) : s( seed ) {}
    uint64_t next()
    {
        s = s * 6364136223846793005ULL + 1442695040888963407ULL;
        return s >> 33;
    }
};

static uint64_t fnv1a( uint64_t h, const std::string& s )
{
    for( unsigned char c : s )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }
    return h;
}


static bool     g_folding = false;
static uint64_t g_foldHash = 0;

// Print one line of the dump.  Lines longer than 120 bytes are shortened to their first 32
// bytes followed by "...#<length>:<fnv1a hash of the whole line>" to keep the dump small.
static void out( const char* fmt, ... ) __attribute__( ( format( printf, 1, 2 ) ) );

static void out( const char* fmt, ... )
{
    va_list args;
    va_start( args, fmt );
    va_list copy;
    va_copy( copy, args );
    int len = vsnprintf( nullptr, 0, fmt, copy );
    va_end( copy );
    std::string line( len + 1, '\0' );
    vsnprintf( line.data(), len + 1, fmt, args );
    va_end( args );
    line.resize( len );

    if( !line.empty() && line.back() == '\n' )
        line.pop_back();

    // escape control characters so that every dump line is exactly one text line
    std::string escaped;

    for( unsigned char c : line )
    {
        if( c < 0x20 )
        {
            char buf[8];
            snprintf( buf, sizeof( buf ), "\\x%02x", c );
            escaped += buf;
        }
        else
        {
            escaped += (char) c;
        }
    }

    line = escaped;

    if( line.size() > 120 )
    {
        char tail[64];
        snprintf( tail, sizeof( tail ), "...#%zu:%016llx", line.size(),
                  (unsigned long long) fnv1a( 0xcbf29ce484222325ULL, line ) );

        // keep at most 32 leading bytes, stopping before the first non-ASCII byte
        size_t keep = 0;

        while( keep < 32 && (unsigned char) line[keep] < 0x80 )
            ++keep;

        line = line.substr( 0, keep ) + tail;
    }

    if( g_folding )
    {
        // fold the line into a running hash instead of printing it
        g_foldHash = fnv1a( g_foldHash, line + "\n" );
        return;
    }

    fputs( line.c_str(), stdout );
    fputc( '\n', stdout );
}

// While folding, out() hashes its lines instead of printing them; endFold() prints the hash.
static void beginFold()
{
    g_folding = true;
    g_foldHash = 0xcbf29ce484222325ULL;
}

static void endFold( const std::string& tag )
{
    g_folding = false;
    out( "%s folded=%016llx\n", tag.c_str(), (unsigned long long) g_foldHash );
}

static std::string seqStr( const std::vector<PCB_LAYER_ID>& seq )
{
    std::string r;
    for( size_t i = 0; i < seq.size(); ++i )
    {
        if( i ) r += ',';
        r += std::to_string( (int) seq[i] );
    }
    return r.empty() ? "-" : r;
}

static std::string bits( std::initializer_list<bool> b )
{
    std::string r;
    for( bool v : b ) r += v ? '1' : '0';
    return r;
}

static std::string layerLine( int id )
{
    std::string line = std::to_string( id ) + " ";
    line += bits( { IsValidLayer( id ), IsPcbLayer( id ), IsCopperLayer( id ), IsExternalCopperLayer( id ),
                    IsInnerCopperLayer( id ), IsNonCopperLayer( id ), IsCopperLayer( id, true ),
                    IsCopperLayer( id, false ), IsSolderMaskLayer( id ), IsViaPadLayer( id ), IsHoleLayer( id ),
                    IsNetnameLayer( id ), IsZoneFillLayer( id ), IsPadCopperLayer( id ), IsViaCopperLayer( id ),
                    IsClearanceLayer( id ), IsDrillSymbolLayer( id ), IsPointsLayer( id ), IsDCodeLayer( id ) } );
    line += " " + std::to_string( GetNetnameLayer( id ) );
    line += " " + U( LayerName( id ) );
    return line;
}

static LSET randomLset( LCG& rng, int density )
{
    LSET set;
    for( int i = 0; i < PCB_LAYER_ID_COUNT; ++i )
    {
        if( (int) ( rng.next() % 100 ) < density )
            set.set( i );
    }
    return set;
}

static void dumpKiid( const char* tag, const KIID& id )
{
    out( "%s %s hash=%llu legacy=%d ts=%u %s\n", tag, id.AsStdString().c_str(),
            (unsigned long long) id.Hash(), (int) id.IsLegacyTimestamp(), (unsigned) id.AsLegacyTimestamp(),
            U( id.AsLegacyTimestampString() ).c_str() );
}

static void dumpLibId( const char* tag, int ret, const LIB_ID& id )
{
    out( "%s ret=%d nick=[%s] item=[%s] fmt=[%s] valid=%d legacy=%d empty=%d full=[%s]\n", tag, ret,
            id.GetLibNickname().c_str(), id.GetLibItemName().c_str(), id.Format().c_str(), (int) id.IsValid(),
            (int) id.IsLegacy(), (int) id.empty(), U( id.GetFullLibraryName() ).c_str() );
}

int main()
{
    // KiCad runs in the user's (UTF-8) locale; wxString::Format( "%s", const char* ) converts
    // through the C library locale (LIB_ID::GetFullLibraryName).
    setlocale( LC_ALL, "C.UTF-8" );

    wxSetAssertHandler( nullptr );
    setvbuf( stdout, nullptr, _IOLBF, 0 );

    // ================= A. layer ids =================
    for( int id = -3; id <= 140; ++id )
        out( "LN %s\n", layerLine( id ).c_str() );

    for( int start = 141; start <= LAYER_ID_COUNT + 2; start += 64 )
    {
        uint64_t h = 0xcbf29ce484222325ULL;
        int      end = std::min( start + 63, (int) LAYER_ID_COUNT + 2 );
        for( int id = start; id <= end; ++id )
            h = fnv1a( h, layerLine( id ) + "\n" );
        out( "LNH %d-%d %016llx\n", start, end, (unsigned long long) h );
    }

    static const int flipCounts[] = { 0, 2, 3, 4, 5, 6, 8, 16, 32 };

    for( int id = -3; id <= 140; ++id )
    {
        PCB_LAYER_ID layer = static_cast<PCB_LAYER_ID>( id );
        std::string  flips;
        for( int n : flipCounts )
            flips += " " + std::to_string( (int) FlipLayer( layer, n ) );
        out( "LP %d %s %s ord=%zu name=%s flip=%s\n", id,
                bits( { IsUserLayer( layer ), IsFrontLayer( layer ), IsBackLayer( layer ) } ).c_str(),
                bits( { IsCopperLayerLowerThan( layer, F_Cu ), IsCopperLayerLowerThan( layer, B_Cu ),
                        IsCopperLayerLowerThan( layer, In2_Cu ), IsCopperLayerLowerThan( F_Cu, layer ),
                        IsCopperLayerLowerThan( B_Cu, layer ), IsCopperLayerLowerThan( In2_Cu, layer ) } ).c_str(),
                IsCopperLayer( id ) ? CopperLayerToOrdinal( layer ) : (size_t) 0, U( LSET::Name( layer ) ).c_str(),
                flips.c_str() );
    }

    for( int id = -5; id <= 70; ++id )
        out( "LEG %d %d\n", id, (int) BoardLayerFromLegacyId( id ) );

    out( "TOLAYER %d %d %d\n", (int) ToLAYER_ID( INT_MAX ), (int) ToLAYER_ID( 5 ), (int) ToLAYER_ID( -1 ) );

    // ================= B. NameToLayer =================
    {
        std::vector<std::string> names;
        for( int id = -2; id < PCB_LAYER_ID_COUNT + 4; ++id )
        {
            names.push_back( U( LSET::Name( static_cast<PCB_LAYER_ID>( id ) ) ) );
            names.push_back( U( LayerName( id ) ) );
        }
        for( const char* s : { "", "F.Cu ", " F.Cu", "f.cu", "User.", "User.0", "User.1", "User.01", "User. 7",
                               "User.+3", "User.-3", "User.46", "User.1000", "User.99999999999999999999",
                               "User.9223372036854775807", "User.4611686018427387905", "User.1x", "User.1 ",
                               "In", "In1", "In12", "In123", "In1.Cu", "In01.Cu", "In30.Cu", "In31.Cu", "In0.Cu",
                               "In-1.Cu", "In1xyz", "In1.Cuu", "In 5.Cu", "Inner1.Cu", "In2147483648.Cu",
                               "In99999999999999999999.Cu", "Internal", "Rescue", "rescue", "Margin", "Edge.Cuts",
                               "B.CrtYd", "F.Courtyard", "User.Drawings", "Dwgs.User", "Cmts.User", "Eco1.User",
                               "Eco2.User", "UNDEFINED", "undefined", "User.\xc3\xa9", "In\xc3\xa9\xc3\xa9\xc3\xa9",
                               "In5\xc3\xa9\xc3\xa9\xc3\xa9", "In7ab" } )
            names.push_back( s );

        for( const std::string& n : names )
        {
            wxString w = wxString::FromUTF8( n.c_str() );
            out( "N2L [%s] %d\n", n.c_str(), LSET::NameToLayer( w ) );
        }
    }

    // ================= C. LSET masks =================
    out( "M InternalCuMask %s\n", LSET::InternalCuMask().FmtHex().c_str() );
    out( "M FrontAssembly %s\n", LSET::FrontAssembly().FmtHex().c_str() );
    out( "M BackAssembly %s\n", LSET::BackAssembly().FmtHex().c_str() );
    out( "M AllCuMask %s\n", LSET::AllCuMask().FmtHex().c_str() );
    out( "M ExternalCuMask %s\n", LSET::ExternalCuMask().FmtHex().c_str() );
    out( "M AllNonCuMask %s\n", LSET::AllNonCuMask().FmtHex().c_str() );
    out( "M AllLayersMask %s\n", LSET::AllLayersMask().FmtHex().c_str() );
    out( "M FrontTechMask %s\n", LSET::FrontTechMask().FmtHex().c_str() );
    out( "M FrontBoardTechMask %s\n", LSET::FrontBoardTechMask().FmtHex().c_str() );
    out( "M BackTechMask %s\n", LSET::BackTechMask().FmtHex().c_str() );
    out( "M BackBoardTechMask %s\n", LSET::BackBoardTechMask().FmtHex().c_str() );
    out( "M AllTechMask %s\n", LSET::AllTechMask().FmtHex().c_str() );
    out( "M AllBoardTechMask %s\n", LSET::AllBoardTechMask().FmtHex().c_str() );
    out( "M FrontMask %s\n", LSET::FrontMask().FmtHex().c_str() );
    out( "M BackMask %s\n", LSET::BackMask().FmtHex().c_str() );
    out( "M SideSpecificMask %s\n", LSET::SideSpecificMask().FmtHex().c_str() );
    out( "M UserMask %s\n", LSET::UserMask().FmtHex().c_str() );
    out( "M PhysicalLayersMask %s\n", LSET::PhysicalLayersMask().FmtHex().c_str() );

    for( int n = -1; n <= 64; ++n )
    {
        LSET m = LSET::AllCuMask( n );
        out( "MCU %d %zu %s\n", n, m.size(), m.FmtHex().c_str() );
    }

    for( int n = -1; n <= 50; ++n )
    {
        LSET m = LSET::UserDefinedLayersMask( n );
        out( "MUD %d %zu %s\n", n, m.size(), m.FmtHex().c_str() );
    }

    // ================= D. LSET operations =================
    {
        LCG               rng( 20240819 );
        std::vector<LSET> sets;
        sets.push_back( LSET() );
        sets.push_back( LSET::AllLayersMask() );
        sets.push_back( LSET::AllCuMask() );
        sets.push_back( LSET::AllNonCuMask() );
        sets.push_back( LSET( { F_Cu } ) );
        sets.push_back( LSET( { B_Cu } ) );
        sets.push_back( LSET( { In7_Cu } ) );
        sets.push_back( LSET( { User_45 } ) );
        sets.push_back( LSET( { F_Cu, B_Cu, F_Mask, B_Mask, F_SilkS, B_SilkS, Edge_Cuts } ) );
        static const int densities[] = { 3, 10, 25, 50, 75, 95 };
        for( int k = 0; k < 24; ++k )
            sets.push_back( randomLset( rng, densities[k % 6] ) );

        static const PCB_LAYER_ID selected[] = { UNDEFINED_LAYER, F_Cu, In2_Cu, F_SilkS, User_3, Edge_Cuts, B_Fab };
        static const int          flipN[] = { 0, 2, 3, 4, 6, 8, 16, 32 };

        for( size_t k = 0; k < sets.size(); ++k )
        {
            const LSET& s = sets[k];
            const LSET& prev = sets[( k + sets.size() - 1 ) % sets.size()];

            // Only the first sets are dumped in full, the others as one hash each.
            if( k >= 20 )
                beginFold();

            out( "S%zu hex %s count=%zu extract=%d any=%d all=%d hash=%llu\n", k, s.FmtHex().c_str(), s.count(),
                    (int) s.ExtractLayer(), (int) s.any(), (int) s.all(),
                    (unsigned long long) std::hash<BASE_SET>()( s ) );
            if( k % 8 == 0 )
                out( "S%zu bin %s\n", k, s.FmtBin().c_str() );
            out( "S%zu seq %s\n", k, seqStr( s.Seq() ).c_str() );
            out( "S%zu custack %s\n", k, seqStr( s.CuStack() ).c_str() );
            out( "S%zu techuser %s\n", k, seqStr( s.TechAndUserUIOrder() ).c_str() );
            out( "S%zu uiorder %s\n", k, seqStr( s.UIOrder() ).c_str() );
            out( "S%zu top2bottom %s\n", k, seqStr( s.SeqStackupTop2Bottom( selected[k % 7] ) ).c_str() );
            out( "S%zu plotting %s\n", k, seqStr( s.SeqStackupForPlotting() ).c_str() );
            {
                std::vector<PCB_LAYER_ID> it;
                for( PCB_LAYER_ID l : s ) it.push_back( l );
                std::vector<PCB_LAYER_ID> run;
                s.RunOnLayers( [&]( PCB_LAYER_ID l ) { run.push_back( l ); } );
                std::vector<PCB_LAYER_ID> nc;
                for( auto i = s.non_copper_layers_begin(); i != s.non_copper_layers_end(); ++i ) nc.push_back( *i );
                out( "S%zu iter %d %d noncu %s\n", k, (int) ( it == s.Seq() ), (int) ( run == s.Seq() ),
                        seqStr( nc ).c_str() );
            }
            for( int n : flipN )
            {
                LSET f = s;
                f.FlipStandardLayers( n );
                out( "S%zu flip%d %zu %s\n", k, n, f.size(), f.FmtHex().c_str() );
            }
            {
                LSET a = s, b = s, c = s;
                a.ClearCopperLayers();
                b.ClearNonCopperLayers();
                c.ClearUserDefinedLayers();
                out( "S%zu clear %s %s %s\n", k, a.FmtHex().c_str(), b.FmtHex().c_str(), c.FmtHex().c_str() );
            }
            {
                LSET a = s & prev, o = s | prev, x = s ^ prev, n = ~s;
                out( "S%zu ops %s %s %s %s cmp=%d lt=%d eq=%d containsall=%d%d\n", k, a.FmtHex().c_str(),
                        o.FmtHex().c_str(), x.FmtHex().c_str(), n.FmtHex().c_str(), s.compare( prev ),
                        (int) ( s < prev ), (int) ( s == prev ), (int) s.ContainsAll( prev ),
                        (int) prev.ContainsAll( s ) );
            }
            {
                std::string c;
                for( PCB_LAYER_ID l : { F_Cu, B_Cu, In1_Cu, In30_Cu, F_Mask, Edge_Cuts, User_1, User_45,
                                        UNDEFINED_LAYER, UNSELECTED_LAYER } )
                    c += s.Contains( l ) ? '1' : '0';
                out( "S%zu contains %s\n", k, c.c_str() );
            }
            {
                // FmtHex / ParseHex round trip
                LSET p;
                int  consumed = p.ParseHex( s.FmtHex() );
                out( "S%zu parsehex %d %d\n", k, consumed, (int) ( p == s ) );
            }

            if( k >= 20 )
                endFold( "S" + std::to_string( k ) );
        }

        // Sets of other sizes
        static const size_t sizes[] = { 1, 7, 10, 60, 64, 65, 100, 128, 129, 200 };
        std::vector<BASE_SET> bsets;
        for( size_t sz : sizes )
        {
            for( int rep = 0; rep < 3; ++rep )
            {
                BASE_SET bs( sz );
                for( size_t i = 0; i < sz; ++i )
                {
                    if( rng.next() % 3 == 0 )
                        bs.set( i );
                }
                bsets.push_back( bs );
            }
        }

        for( size_t k = 0; k < bsets.size(); ++k )
        {
            const BASE_SET& s = bsets[k];
            const BASE_SET& prev = bsets[( k + bsets.size() - 1 ) % bsets.size()];
            const BASE_SET& far = bsets[( k + 7 ) % bsets.size()];
            BASE_SET        a = s & far, o = s | far, x = s ^ far, n = ~s;
            out( "B%zu size=%zu hex %s bin %s count=%zu hash=%llu cmp=%d,%d lt=%d,%d eq=%d\n", k, s.size(),
                    s.FmtHex().c_str(), s.FmtBin().c_str(), s.count(),
                    (unsigned long long) std::hash<BASE_SET>()( s ), s.compare( prev ), s.compare( far ),
                    (int) ( s < prev ), (int) ( s < far ), (int) ( s == prev ) );
            out( "B%zu ops %zu %s %zu %s %zu %s %zu %s\n", k, a.size(), a.FmtHex().c_str(), o.size(),
                    o.FmtHex().c_str(), x.size(), x.FmtHex().c_str(), n.size(), n.FmtHex().c_str() );
            LSET asLset( s );
            // The copper layer iterator of LSET never terminates for a set of odd size.
            bool even = s.size() % 2 == 0;
            out( "B%zu lset custack %s noncu-ui %s top2bottom %s\n", k,
                    even ? seqStr( asLset.CuStack() ).c_str() : "skipped",
                    seqStr( asLset.TechAndUserUIOrder() ).c_str(),
                    even ? seqStr( asLset.SeqStackupTop2Bottom() ).c_str() : "skipped" );
        }

        // ParseHex of assorted strings into sets of several sizes
        for( const char* str : { "", "0", "1", "f", "ff", "FF", "aBcDeF", "0x1f", "1_f", "_", "__1", "zz", "1z2",
                                 "00010fc_ffffffff", "ffffffff_ffffffff_ffffffff_ffffffff",
                                 "1_00000000_00000000_00000000_00000000", "fffffffffffffffffffffffffffffffffff",
                                 "12 34", "-1", "0000000_00000000_55555555_5755f5ff", "8", "g", "1g" } )
        {
            for( size_t sz : { (size_t) 4, (size_t) 10, (size_t) 60, (size_t) 64, (size_t) 128 } )
            {
                BASE_SET bs( sz );
                bs.set( 1 );
                int consumed = bs.ParseHex( std::string( str ) );
                out( "PH [%s] %zu %d %s\n", str, sz, consumed, bs.FmtHex().c_str() );
            }
        }
    }

    // ================= E. LayerCount / IsBetween =================
    {
        static const PCB_LAYER_ID cu[] = { F_Cu, B_Cu, In1_Cu, In2_Cu, In3_Cu, In5_Cu, In30_Cu };
        static const PCB_LAYER_ID probe[] = { F_Cu, B_Cu, In1_Cu, In2_Cu, In3_Cu, In4_Cu, In5_Cu, In29_Cu, In30_Cu,
                                              F_Mask, B_Mask, User_1, UNDEFINED_LAYER };
        for( PCB_LAYER_ID a : cu )
        {
            for( PCB_LAYER_ID b : cu )
            {
                std::string counts;
                for( int n : { 2, 4, 6, 8, 32 } )
                    counts += " " + std::to_string( LSET::LayerCount( a, b, n ) );
                std::string between;
                for( PCB_LAYER_ID p : probe )
                    between += LSET::IsBetween( a, b, p ) ? '1' : '0';
                out( "LC %d %d%s between=%s\n", (int) a, (int) b, counts.c_str(), between.c_str() );
            }
        }
        out( "LC noncu %d %d %d\n", LSET::LayerCount( F_Mask, B_Cu, 6 ), LSET::LayerCount( F_Cu, B_Mask, 4 ),
                LSET::LayerCount( UNDEFINED_LAYER, F_Cu, 2 ) );
        out( "IB noncu %d %d %d %d\n", (int) LSET::IsBetween( F_Mask, B_Cu, F_Cu ),
                (int) LSET::IsBetween( F_Mask, B_Mask, B_Cu ), (int) LSET::IsBetween( F_Cu, User_1, In5_Cu ),
                (int) LSET::IsBetween( F_Mask, User_1, F_SilkS ) );
    }

    // ================= F. LAYER_RANGE =================
    for( int n : { 2, 3, 4, 5, 6, 8, 32 } )
    {
        std::vector<PCB_LAYER_ID> stack;
        stack.push_back( F_Cu );
        for( int i = 1; i <= n - 2; ++i )
        {
            if( i <= 3 || i >= n - 3 )
                stack.push_back( static_cast<PCB_LAYER_ID>( In1_Cu + ( i - 1 ) * 2 ) );
        }
        stack.push_back( B_Cu );

        for( PCB_LAYER_ID a : stack )
        {
            for( PCB_LAYER_ID b : stack )
            {
                LAYER_RANGE               range( a, b, n );
                std::vector<PCB_LAYER_ID> layers;
                for( PCB_LAYER_ID l : range ) layers.push_back( l );
                std::string contains;
                for( PCB_LAYER_ID p : stack ) contains += range.Contains( p ) ? '1' : '0';
                LSET fromRange( range );
                out( "LR %d %d %d size=%zu [%s] contains=%s lset=%zu:%s\n", n, (int) a, (int) b, range.size(),
                        seqStr( layers ).c_str(), contains.c_str(), fromRange.size(), fromRange.FmtHex().c_str() );
            }
        }
    }

    for( int n : { 0, 1, -7, 64, 65, 128 } )
    {
        LAYER_RANGE               range( F_Cu, B_Cu, n );
        std::vector<PCB_LAYER_ID> layers;
        for( PCB_LAYER_ID l : range ) layers.push_back( l );
        out( "LRN %d size=%zu count=%zu last=%d\n", n, range.size(), layers.size(),
                (int) layers[layers.size() - 2] );
    }

    for( int a : { 0, 2, 4, 6, 60 } )
        for( int b : { 0, 2, 4, 6, 60 } )
        {
            std::string c;
            for( int t : { 0, 2, 4, 6, 8, 60, 62, 1, -1 } ) c += LAYER_RANGE::Contains( a, b, t ) ? '1' : '0';
            out( "LRC %d %d %s\n", a, b, c.c_str() );
        }

    {
        LSEQ seq = LSET::AllLayersMask().SeqStackupTop2Bottom();
        out( "TL %d %d %d %d %d\n", seq.TestLayers( F_Cu, F_Cu ), seq.TestLayers( F_Cu, In1_Cu ),
                seq.TestLayers( In1_Cu, F_Cu ), seq.TestLayers( B_Fab, Edge_Cuts ),
                seq.TestLayers( UNDEFINED_LAYER, F_Cu ) );
    }

    // ================= G. KIID =================
    for( unsigned seed : { 0u, 1u, 42u, 123456789u, 4294967295u } )
    {
        KIID::SeedGenerator( seed );
        for( int i = 0; i < 6; ++i )
        {
            KIID id;
            dumpKiid( "KS", id );
        }
    }

    KIID::SeedGenerator( 7 );
    std::vector<KIID> parsed;
    for( const char* s : { "0", "1", "a", "F", "12", "123", "1234", "12345", "123456", "1234567", "12345678",
                           "AABBCCDD", "aabbccdd", "00000012", "123456789", "1234567g", "g", "", " ", "0x12",
                           "01234567-89ab-cdef-0123-456789abcdef", "0123456789abcdef0123456789abcdef",
                           "{01234567-89ab-cdef-0123-456789abcdef}", "{0123456789abcdef0123456789abcdef}",
                           "01234567-89abcdef0123456789abcdef", "0123456789ab-cdef-0123-456789abcdef",
                           "01234567-89ab-cdef-0123-456789abcdef0", "01234567-89ab-cdef-0123-456789abcde",
                           "{01234567-89ab-cdef-0123-456789abcdef", "01234567-89ab-cdef-0123-456789abcdef}",
                           "01234567-89AB-CDEF-0123-456789ABCDEF", "g1234567-89ab-cdef-0123-456789abcdef",
                           "01234567-89ab-cdef-0123456789abcdef", " 01234567-89ab-cdef-0123-456789abcdef",
                           "00000000-0000-0000-0000-000000000000", "ffffffff-ffff-ffff-ffff-ffffffffffff",
                           "00000000-0000-0000-0000-00005f0b8a2c", "5f0b8a2c-0000-0000-0000-000000000000",
                           "r\xc3\xa9sistor", "\xc3\xa9", "deadbeef", "DEADBEEF1" } )
    {
        KIID id( s );
        parsed.push_back( id );
        out( "KP [%s] sniff=%d\n", s, (int) KIID::SniffTest( wxString::FromUTF8( s ) ) );
        dumpKiid( "KP  ", id );
    }

    {
        KIID::CreateNilUuids( true );
        KIID nil1;
        KIID bad( "not-a-uuid" );
        // NB: KIID( timestamp_t ) leaves the first 12 bytes uninitialised with Boost < 1.86, so the
        // legacy timestamp is built from its string form here.
        KIID ts( "5F0B8A2C" );
        dumpKiid( "KN nil", nil1 );
        dumpKiid( "KN bad", bad );
        dumpKiid( "KN ts", ts );
        ts.ConvertTimestampToUuid();
        dumpKiid( "KN converted", ts );
        KIID keep = ts;
        ts.ConvertTimestampToUuid();
        out( "KN unchanged %d\n", (int) ( keep == ts ) );
        KIID::CreateNilUuids( false );
        KIID after;
        dumpKiid( "KN after", after );
        {
            KIID_NIL_SET_RESET guard;
            KIID               guarded;
            dumpKiid( "KN guarded", guarded );
        }
        KIID after2;
        dumpKiid( "KN after2", after2 );
        out( "KN niluuid %s %d\n", niluuid.AsStdString().c_str(), (int) ( NilUuid() == niluuid ) );
    }

    for( size_t i = 0; i + 1 < parsed.size(); i += 2 )
    {
        KIID c = KIID::Combine( parsed[i], parsed[i + 1] );
        KIID inc = parsed[i];
        inc.Increment();
        out( "KC %s %s lt=%d gt=%d eq=%d\n", c.AsStdString().c_str(), inc.AsStdString().c_str(),
                (int) ( parsed[i] < parsed[i + 1] ), (int) ( parsed[i] > parsed[i + 1] ),
                (int) ( parsed[i] == parsed[i + 1] ) );
    }

    {
        std::vector<KIID> sorted = parsed;
        std::sort( sorted.begin(), sorted.end() );
        uint64_t h = 0xcbf29ce484222325ULL;
        for( const KIID& id : sorted ) h = fnv1a( h, id.AsStdString() + "\n" );
        out( "KSORT %s %s %016llx\n", sorted.front().AsStdString().c_str(), sorted.back().AsStdString().c_str(),
                (unsigned long long) h );
    }

    for( const char* s : { "", "a", "Device:R", "R_0402_1005Metric", "some known string", "R\xc3\xa9sistance",
                           "\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e", "\xf0\x9f\x98\x80 smile",
                           "a longer name that spans more than one sha1 block: 0123456789012345678901234567890123456789",
                           "exactly-fifty-five-bytes-of-payload-with-namespace-uuid" } )
    {
        out( "KD [%s] %s %s\n", s, KIID::FromDeterministicString( wxString::FromUTF8( s ) ).AsStdString().c_str(),
                KIID::FromName( s ).AsStdString().c_str() );
    }

    {
        std::array<uint8_t, 16> bytes;
        for( int i = 0; i < 16; ++i ) bytes[i] = (uint8_t) ( i * 17 + 3 );
        KIID fb = KIID::FromBytes( bytes );
        KIID inc = fb;
        for( int i = 0; i < 300; ++i ) inc.Increment();
        KIID wrap( "ffffffff-ffff-ffff-ffff-ffffffffffff" );
        wrap.Increment();
        out( "KB %s %s %s %d\n", fb.AsStdString().c_str(), inc.AsStdString().c_str(), wrap.AsStdString().c_str(),
                (int) ( fb.AsBytes() == bytes ) );
    }

    // KIID_PATH
    KIID::SeedGenerator( 99 );
    {
        std::vector<KIID_PATH> paths;
        for( const char* s : { "", "/", "//", "/00000000-0000-0000-0000-00005f0b8a2c",
                               "/00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d",
                               "00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d/",
                               "/00000000-0000-0000-0000-00005f0b8a2c/00000000-0000-0000-0000-00005f0b8a2d/"
                               "e63e39d7-6ac0-4ffd-8aa3-1841a4541b55",
                               "/5F0B8A2C/5F0B8A2D", "/garbage/5F0B8A2D", "/a\\/b/c", "/00000000-0000-0000-0000-00005f0b8a2d",
                               "/e63e39d7-6ac0-4ffd-8aa3-1841a4541b55",
                               "/00000000-0000-0000-0000-00005f0b8a2d/e63e39d7-6ac0-4ffd-8aa3-1841a4541b55" } )
        {
            KIID_PATH p( wxString::FromUTF8( s ) );
            paths.push_back( p );
            out( "PP [%s] n=%zu [%s] hash=%llu\n", s, p.size(), U( p.AsString() ).c_str(),
                    (unsigned long long) std::hash<KIID_PATH>()( p ) );
        }

        for( size_t i = 0; i < paths.size(); ++i )
        {
            std::string row;
            for( size_t j = 0; j < paths.size(); ++j )
            {
                KIID_PATH rel = paths[i];
                bool      ok = rel.MakeRelativeTo( paths[j] );
                row += " ";
                row += bits( { paths[i] == paths[j], paths[i] < paths[j], paths[i] > paths[j],
                               paths[i].EndsWith( paths[j] ), paths[i].IsContainedWithin( paths[j] ), ok } );
                row += ":" + std::to_string( rel.size() );
            }
            KIID_PATH sum = paths[i] + paths[( i + 1 ) % paths.size()];
            out( "PR %zu%s sum=%zu\n", i, row.c_str(), sum.size() );
        }
    }

    // ================= H. LIB_ID =================
    {
        std::vector<LIB_ID> ids;
        for( const char* s : { "", ":", "a", "a:", ":a", "Package_SO:DGG56", "DGG56", "lib:item:extra", "lib::item",
                               "lib:it/em", "lib:it em", "lib name:item", "lib:item<", "lib:>item", "lib:it\"em",
                               "lib:it\\em", "lib:it\tem", "lib:it\nem", "lib:it\rem", "li\\b:item", "li\tb:item",
                               "lib:%$/", "Biblioth\xc3\xa8que:R\xc3\xa9sistance", "\xe6\x97\xa5:\xe6\x9c\xac<\xe8\xaa\x9e",
                               "lib:item ", " lib : item ", "LIB:ITEM", "lib:Item", "liB:item", "::", "a:b:c:d<e",
                               "<>\"\\:\t\n\r", "x<>\"\\\t\n\r" } )
        {
            LIB_ID id;
            int    ret = id.Parse( s );
            out( "LI [%s]\n", s );
            dumpLibId( "LI   parse", ret, id );
            ids.push_back( id );
            LIB_ID fixed;
            ret = fixed.Parse( s, true );
            dumpLibId( "LI   fixed", ret, fixed );
            out( "LI   illegal=%d fixlib=[%s] fixitem=[%s] badlibchar=%u\n", LIB_ID::HasIllegalChars( s ),
                    LIB_ID::FixIllegalChars( s, true ).c_str(), LIB_ID::FixIllegalChars( s, false ).c_str(),
                    LIB_ID::FindIllegalLibraryNameChar( s ) );
            try
            {
                UTF8 f = LIB_ID::Format( s, "item" );
                out( "LI   format=[%s]\n", f.c_str() );
            }
            catch( const PARSE_ERROR& e )
            {
                out( "LI   format threw [%s]\n", U( e.Problem() ).c_str() );
            }
            LIB_ID direct( wxString::FromUTF8( s ), wxString::FromUTF8( "it:em" ) );
            direct.SetSubLibraryName( "sub" );
            int nickRet = direct.SetLibNickname( "ni:ck" );
            dumpLibId( "LI   direct", nickRet, direct );
        }

        for( size_t i = 0; i < ids.size(); ++i )
        {
            std::string row;
            for( size_t j = 0; j < ids.size(); ++j )
            {
                int c = ids[i].compare( ids[j] );
                row += c < 0 ? '<' : ( c > 0 ? '>' : '=' );
            }
            out( "LIC %zu %s\n", i, row.c_str() );
        }

        out( "LIF [%s] [%s]\n", U( GetLibIdForbiddenChars() ).c_str(), U( GetLibFilenameForbiddenChars() ).c_str() );
    }

    return 0;
}
