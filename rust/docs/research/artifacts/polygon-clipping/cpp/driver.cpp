// Differential-test driver for the C++ Clipper2 library (compiled with -DUSINGZ, like KiCad).
// Reads the case file produced by gen_cases.py and prints canonical results.
//
// g++ -O2 -std=c++17 -DUSINGZ -I<Clipper2Lib/include> driver.cpp <Clipper2Lib/src/*.cpp> -o driver
// Optional: -DPIP_THRESHOLD=<n> (KiCad patched copy only) forces/disables the StripeIndex path.

#include <clipper2/clipper.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

using namespace Clipper2Lib;

static double from_bits( uint64_t b )
{
    double d;
    std::memcpy( &d, &b, sizeof( d ) );
    return d;
}

static uint64_t to_bits( double d )
{
    uint64_t b;
    std::memcpy( &b, &d, sizeof( d ) );
    return b;
}

static std::string out;

static void put( const char* s )
{
    out += s;
}

static void put_i( int64_t v )
{
    char buf[32];
    snprintf( buf, sizeof( buf ), " %lld", (long long) v );
    out += buf;
}

static void put_path( const char* tag, const Path64& p )
{
    out += tag;
    put_i( (int64_t) p.size() );

    for( const Point64& pt : p )
    {
        put_i( pt.x );
        put_i( pt.y );
        put_i( pt.z );
    }

    out += "\n";
}

static void put_tree_node( const PolyPath64& node, int depth )
{
    char buf[64];
    snprintf( buf, sizeof( buf ), "N %d %d", depth, node.IsHole() ? 1 : 0 );
    put_path( buf, node.Polygon() );

    for( const std::unique_ptr<PolyPath64>& child : node )
        put_tree_node( *child, depth + 1 );
}

static void put_tree( const char* title, const PolyTree64& tree )
{
    out += title;
    out += "\n";

    for( const std::unique_ptr<PolyPath64>& child : tree )
        put_tree_node( *child, 0 );
}

static void put_paths( const char* title, const Paths64& paths )
{
    out += title;
    put_i( (int64_t) paths.size() );
    out += "\n";

    for( const Path64& p : paths )
        put_path( "Q", p );
}

struct Case
{
    long long                id = 0;
    std::string              cat;
    std::string              op;
    std::vector<std::string> args;
    Paths64                  subj;
    Paths64                  clip;
    std::vector<Paths64>     groups;
    int64_t                  npoints = 0;
};

static uint64_t arg_u64( const Case& c, size_t i )
{
    return strtoull( c.args.at( i ).c_str(), nullptr, 10 );
}

static int64_t arg_i64( const Case& c, size_t i )
{
    return strtoll( c.args.at( i ).c_str(), nullptr, 10 );
}

// KiCad-style Z callback: every call allocates a new z index (zValues.size()).
struct ZLog
{
    int64_t     next_z;
    const char* tag;
    int         calls = 0;

    ZCallback64 make()
    {
        return [this]( const Point64& e1bot, const Point64& e1top, const Point64& e2bot,
                       const Point64& e2top, Point64& pt )
        {
            out += tag;
            put_i( e1bot.x ); put_i( e1bot.y ); put_i( e1bot.z );
            put_i( e1top.x ); put_i( e1top.y ); put_i( e1top.z );
            put_i( e2bot.x ); put_i( e2bot.y ); put_i( e2bot.z );
            put_i( e2top.x ); put_i( e2top.y ); put_i( e2top.z );
            put_i( pt.x ); put_i( pt.y ); put_i( pt.z );
            out += "\n";
            pt.z = next_z++;
            calls++;
        };
    }
};

static void run_bool( const Case& c )
{
    ClipType ct = (ClipType) arg_i64( c, 0 );
    FillRule fr = (FillRule) arg_i64( c, 1 );
    bool     pc = arg_i64( c, 2 ) != 0;

    {
        Clipper64 clipper;
#ifdef PIP_THRESHOLD
        clipper.PipIndexThreshold( PIP_THRESHOLD );
#endif
        clipper.PreserveCollinear( pc );
        clipper.AddSubject( c.subj );
        clipper.AddClip( c.clip );
        ZLog zlog{ c.npoints, "Z" };
        clipper.SetZCallback( zlog.make() );
        PolyTree64 tree;
        bool       ok = clipper.Execute( ct, fr, tree );
        put( ok ? "OK 1\n" : "OK 0\n" );
        put_tree( "TREE", tree );
    }

    {
        Clipper64 clipper;
#ifdef PIP_THRESHOLD
        clipper.PipIndexThreshold( PIP_THRESHOLD );
#endif
        clipper.PreserveCollinear( pc );
        clipper.AddSubject( c.subj );
        clipper.AddClip( c.clip );
        ZLog zlog{ c.npoints, "Y" };
        clipper.SetZCallback( zlog.make() );
        Paths64 closed;
        bool    ok = clipper.Execute( ct, fr, closed );
        put( ok ? "OK 1\n" : "OK 0\n" );
        put_paths( "PATHS", closed );
    }
}

static void setup_offset( ClipperOffset& co, const Case& c )
{
    JoinType jt = (JoinType) arg_i64( c, 0 );
    EndType  et = (EndType) arg_i64( c, 1 );

    for( const Paths64& g : c.groups )
        co.AddPaths( g, jt, et );

    co.ArcTolerance( from_bits( arg_u64( c, 3 ) ) );
    co.MiterLimit( from_bits( arg_u64( c, 2 ) ) );
}

static void run_offset( const Case& c )
{
    double delta = from_bits( arg_u64( c, 4 ) );

    // SHAPE_POLY_SET::inflate2( aSimplify = false ): c.Execute( aAmount, tree )
    {
        ClipperOffset co;
        setup_offset( co, c );
        PolyTree64 tree;
        co.Execute( delta, tree );
        put_tree( "TREE", tree );
    }

    // SHAPE_POLY_SET::inflate2( aSimplify = true ): c.Execute( aAmount, paths ), then
    // SimplifyPaths (result discarded by KiCad), then Union / Positive with
    // PreserveCollinear( false ).
    {
        ClipperOffset co;
        setup_offset( co, c );
        Paths64 paths;
        co.Execute( delta, paths );
        put_paths( "PATHS", paths );

        Clipper64 c2;
        c2.PreserveCollinear( false );
        c2.ReverseSolution( false );
        c2.AddSubject( paths );
        PolyTree64 tree;
        c2.Execute( ClipType::Union, FillRule::Positive, tree );
        put_tree( "TREE2", tree );
    }
}

static void run_case( const Case& c )
{
    if( c.op == "BOOL" )
    {
        run_bool( c );
    }
    else if( c.op == "OFFSET" )
    {
        run_offset( c );
    }
    else if( c.op == "SIMPLIFY" )
    {
        Paths64 res = SimplifyPaths( c.subj, from_bits( arg_u64( c, 0 ) ), arg_i64( c, 1 ) != 0 );
        put_paths( "PATHS", res );
    }
    else if( c.op == "AREA" )
    {
        for( const Path64& p : c.subj )
        {
            char buf[64];
            snprintf( buf, sizeof( buf ), "A %llu\n", (unsigned long long) to_bits( Area( p ) ) );
            out += buf;
        }
    }
    else if( c.op == "RECTCLIP" )
    {
        Rect64 r( arg_i64( c, 0 ), arg_i64( c, 1 ), arg_i64( c, 2 ), arg_i64( c, 3 ) );
        put_paths( "PATHS", RectClip( r, c.subj ) );
    }
    else if( c.op == "MINKSUM" )
    {
        put_paths( "PATHS", MinkowskiSum( c.subj.at( 0 ), c.clip.at( 0 ), arg_i64( c, 0 ) != 0 ) );
    }
    else if( c.op == "MINKDIFF" )
    {
        put_paths( "PATHS", MinkowskiDiff( c.subj.at( 0 ), c.clip.at( 0 ), arg_i64( c, 0 ) != 0 ) );
    }
    else
    {
        put( "UNKNOWN\n" );
    }
}

static Path64 parse_path( std::istringstream& ss, int64_t& npoints )
{
    size_t n = 0;
    ss >> n;
    Path64 p;
    p.reserve( n );

    for( size_t i = 0; i < n; i++ )
    {
        int64_t x, y, z;
        ss >> x >> y >> z;
        p.emplace_back( x, y, z );
    }

    npoints += (int64_t) n;
    return p;
}

int main( int argc, char** argv )
{
    if( argc < 3 )
    {
        fprintf( stderr, "usage: driver <cases> <out>\n" );
        return 2;
    }

    std::ifstream in( argv[1] );
    FILE*         fo = fopen( argv[2], "w" );
    std::string   line;
    Case          cur;

    while( std::getline( in, line ) )
    {
        if( line.empty() )
            continue;

        std::istringstream ss( line );
        std::string        tag;
        ss >> tag;

        if( tag == "CASE" )
        {
            cur = Case();
            ss >> cur.id >> cur.cat >> cur.op;
            std::string a;

            while( ss >> a )
                cur.args.push_back( a );
        }
        else if( tag == "G" )
        {
            cur.groups.emplace_back();
        }
        else if( tag == "S" )
        {
            Path64 p = parse_path( ss, cur.npoints );

            if( !cur.groups.empty() )
                cur.groups.back().push_back( p );

            cur.subj.push_back( std::move( p ) );
        }
        else if( tag == "C" )
        {
            cur.clip.push_back( parse_path( ss, cur.npoints ) );
        }
        else if( tag == "END" )
        {
            out.clear();
            char buf[64];
            snprintf( buf, sizeof( buf ), "CASE %lld\n", cur.id );
            out += buf;

            try
            {
                run_case( cur );
            }
            catch( const std::exception& e )
            {
                out += "ERR ";
                out += e.what();
                out += "\n";
            }
            catch( ... )
            {
                out += "ERR unknown\n";
            }

            out += "END\n";
            fwrite( out.data(), 1, out.size(), fo );
        }
    }

    fclose( fo );
    return 0;
}
