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

// Differential oracle for kicad_core::json (work package C3): the bundled nlohmann/json 3.11.3
// used exactly as KiCad uses it.  Replayed by tests/test_c3_json_oracle.rs, which mirrors main()
// section by section, including the pseudo-random generators.
//
//   json_oracle <kicad source root> <file list> [full]
//
// The file list holds paths relative to the source root (see build_c3_oracle.sh).  Output: per
// section the first lines verbatim ("E <section> <line>") and a summary
// ("S <section> <count> <fnv1a-64 of all lines>"); with "full" every line is printed.

#include <nlohmann/json.hpp>

#include <algorithm>
#include <cinttypes>
#include <cstring>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

using json = nlohmann::json;
using ojson = nlohmann::ordered_json;

// ---- shared helpers (mirrored in Rust) ----

struct LCG
{
    uint64_t s;
    uint64_t next()
    {
        s = s * 6364136223846793005ULL + 1442695040888963407ULL;
        return s >> 33;
    }
    uint64_t u64()
    {
        uint64_t a = next(), b = next(), c = next();
        return ( a << 42 ) ^ ( b << 21 ) ^ c;
    }
    uint32_t below( uint32_t n ) { return (uint32_t) ( next() % n ); }
};

static uint64_t fnvBytes( const std::string& s, uint64_t h = 1469598103934665603ULL )
{
    for( unsigned char c : s )
    {
        h ^= c;
        h *= 1099511628211ULL;
    }
    return h;
}

static std::string hex64( uint64_t v )
{
    char buf[32];
    snprintf( buf, sizeof buf, "%016" PRIx64, v );
    return buf;
}

static bool g_full = false;
static std::string g_section;
static std::vector<std::string> g_lines;

static void flush()
{
    if( g_section.empty() )
        return;

    uint64_t h = 1469598103934665603ULL;

    for( size_t i = 0; i < g_lines.size(); ++i )
    {
        h = fnvBytes( g_lines[i] + "\n", h );

        if( g_full || i < 25 )
            std::cout << "E " << g_section << " " << g_lines[i] << "\n";
    }

    std::cout << "S " << g_section << " " << g_lines.size() << " " << hex64( h ) << "\n";
    g_lines.clear();
}

static void begin( const std::string& name )
{
    flush();
    g_section = name;
}

static void out( const std::string& line ) { g_lines.push_back( line ); }

// Escape non-printable bytes so that every output line is one line of ASCII.
static std::string esc( const std::string& s )
{
    std::string o;
    for( unsigned char c : s )
    {
        if( c < 0x20 || c >= 0x7f || c == '\\' )
        {
            char buf[8];
            snprintf( buf, sizeof buf, "\\x%02X", c );
            o += buf;
        }
        else
            o += (char) c;
    }
    return o;
}

static const double P10[23] = { 1e0,  1e1,  1e2,  1e3,  1e4,  1e5,  1e6,  1e7,  1e8,  1e9,  1e10, 1e11,
                                1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22 };

static const double SPECIAL_D[20] = { 0.0,  -0.0,  1e-4, -1e-4, 1.0000000000000002e-4, 9.9999e-5,
                                      0.5,  2.5,   -2.5, 1e16,  123456789.0,           1e-300,
                                      5e-324, 1.7976931348623157e308, 0.1, 0.3, 25.4, 0.0254,
                                      1e10, 99999.99999 };

static double rdouble( LCG& g )
{
    switch( g.below( 6 ) )
    {
    case 0:
    {
        uint64_t bits = g.u64();
        double d;
        memcpy( &d, &bits, 8 );
        return d;
    }
    case 1:
    {
        double v = (double) ( (int64_t) g.next() - 1073741824 );
        return v / P10[g.below( 23 )];
    }
    case 2:
    {
        double v = (double) ( (int64_t) g.next() - 1073741824 );
        return v * P10[g.below( 16 )];
    }
    case 3:
    {
        double v = (double) ( (int64_t) g.below( 2001 ) - 1000 );
        return v / P10[g.below( 12 )];
    }
    case 4: return SPECIAL_D[g.below( 20 )];
    default:
    {
        double v = (double) ( (int64_t) g.next() - 1073741824 ) + 0.5;
        return v / P10[g.below( 8 )];
    }
    }
}

// ---- random JSON text generator (mirrored in Rust) ----

static const char* SPECIAL_NUM[] = { "-0", "0", "-0.0", "0.0", "1e400", "-1e400", "5e-324",
                                     "2.2250738585072014e-308", "18446744073709551615",
                                     "18446744073709551616", "-9223372036854775808",
                                     "-9223372036854775809", "9223372036854775807", "1E5", "1e-7",
                                     "123456789012345678", "0.1", "1.5e300",
                                     "4.9406564584124654e-324", "1e23", "01", "-", "1.", ".5",
                                     "+1", "1e", "-01", "1.0e+", "0e0", "-0e-0" };
static const int N_SPECIAL_NUM = sizeof( SPECIAL_NUM ) / sizeof( SPECIAL_NUM[0] );

static const char* STR_PARTS[] = { "abc", "Net-(R1-Pad1)", "/A/B", " ", "\xC3\xA9", "\xE6\x97\xA5\xE6\x9C\xAC",
                                   "\xF0\x9D\x84\x9E", "\xC2\xB5", "\\n", "\\t", "\\\"", "\\\\", "\\/",
                                   "\\u0041", "\\u00e9", "\\ud834\\udd1e", "\\u0001", "\\u001f",
                                   "\\u007f", "\x7f", "\\b\\f\\r", "\\u20AC", "\\uD83D\\uDE00", "${VAR}",
                                   "{", "}", "[", "]", ":", ",", "//", "/*", "\\uDC00", "\\ud800x",
                                   "\x01", "\xC0\xAF", "\xED\xA0\x80", "\\x", "\\u12G4", "\xF4\x90\x80\x80" };
static const int N_STR_PARTS = sizeof( STR_PARTS ) / sizeof( STR_PARTS[0] );

static const char* KEYS[] = { "a", "b", "meta", "version", "B", "_", "aa", "a b", "\xC3\xA9", "Z",
                              "\\u0000", "1", "10", "2", "", "net_settings", "classes" };
static const int N_KEYS = sizeof( KEYS ) / sizeof( KEYS[0] );

static const char* WS[] = { " ", "\n  ", "\t", "/*c*/", "// x\n", "\r\n", "", "", "", "" };

static std::string digits( LCG& g, int n, bool firstNonZero )
{
    std::string s;
    for( int i = 0; i < n; ++i )
        s += (char) ( '0' + ( ( i == 0 && firstNonZero ) ? 1 + g.below( 9 ) : g.below( 10 ) ) );
    return s;
}

static std::string sign( LCG& g ) { return g.below( 2 ) ? "-" : ""; }

// Every pseudo-random draw is its own statement: the evaluation order of the operands of an
// overloaded operator+ is unspecified in C++, the Rust mirror draws left to right.
static std::string genNumber( LCG& g )
{
    switch( g.below( 8 ) )
    {
    case 0:
    {
        std::string s = sign( g );
        if( g.below( 4 ) == 0 )
            return s + "0";
        int n = 1 + g.below( 20 );
        return s + digits( g, n, true );
    }
    case 1:
    {
        std::string s = sign( g );
        int n = 19 + g.below( 7 );
        return s + digits( g, n, true );
    }
    case 2:
    {
        std::string s = sign( g );
        int n = 1 + g.below( 6 );
        std::string a = digits( g, n, true );
        int m = 1 + g.below( 17 );
        std::string b = digits( g, m, false );
        return s + a + "." + b;
    }
    case 3:
    {
        std::string s0 = sign( g );
        int n = 1 + g.below( 3 );
        std::string s = s0 + digits( g, n, true );
        if( g.below( 2 ) )
        {
            int m = 1 + g.below( 5 );
            s += "." + digits( g, m, false );
        }
        s += g.below( 2 ) ? "e" : "E";
        uint32_t sg = g.below( 3 );
        s += sg == 0 ? "" : sg == 1 ? "+" : "-";
        int m = 1 + g.below( 3 );
        return s + digits( g, m, false );
    }
    case 4: return SPECIAL_NUM[g.below( N_SPECIAL_NUM )];
    case 5:
    {
        std::string s = std::to_string( (int64_t) g.next() - 1073741824 );
        int n = g.below( 3 ) + 1;
        return s + "." + digits( g, n, false );
    }
    default:
    {
        std::string s = sign( g );
        return s + std::to_string( g.below( 100000 ) );
    }
    }
}

static std::string genString( LCG& g )
{
    std::string s = "\"";
    uint32_t n = g.below( 6 );
    for( uint32_t i = 0; i < n; ++i )
    {
        uint32_t k = g.below( N_STR_PARTS + 8 );
        s += k < (uint32_t) N_STR_PARTS ? STR_PARTS[k] : "x";
    }
    return s + "\"";
}

static std::string ws( LCG& g ) { return WS[g.below( 10 )]; }

static std::string genValue( LCG& g, int depth )
{
    uint32_t r = g.below( depth > 3 ? 6 : 10 );

    switch( r )
    {
    case 0:
    {
        uint32_t k = g.below( 3 );
        return k == 0 ? "null" : k == 1 ? "true" : "false";
    }
    case 1:
    case 2:
    case 5: return genNumber( g );
    case 3:
    case 4: return genString( g );
    case 6:
    case 7:
    {
        std::string s = "[";
        s += ws( g );
        uint32_t n = g.below( 5 );
        for( uint32_t i = 0; i < n; ++i )
        {
            if( i )
            {
                s += ",";
                s += ws( g );
            }
            s += genValue( g, depth + 1 );
            s += ws( g );
        }
        return s + "]";
    }
    default:
    {
        std::string s = "{";
        s += ws( g );
        uint32_t n = g.below( 5 );
        for( uint32_t i = 0; i < n; ++i )
        {
            if( i )
            {
                s += ",";
                s += ws( g );
            }
            s += "\"";
            s += KEYS[g.below( N_KEYS )];
            s += "\"";
            s += ws( g );
            s += ":";
            s += ws( g );
            s += genValue( g, depth + 1 );
            s += ws( g );
        }
        return s + "}";
    }
    }
}

static std::string genDocument( LCG& g )
{
    std::string s;
    if( g.below( 16 ) == 0 )
        s += "\xEF\xBB\xBF";
    s += ws( g );
    s += genValue( g, 0 );
    s += ws( g );

    uint32_t c = g.below( 32 );
    if( c == 0 && !s.empty() )
        s = s.substr( 0, g.below( (uint32_t) s.size() ) );
    else if( c == 1 && !s.empty() )
    {
        static const char BAD[] = { '}', ']', ',', ':', '"', '/', 'x', '\x01', '\xff', '\xef' };
        uint32_t i = g.below( (uint32_t) s.size() );
        s[i] = BAD[g.below( 10 )];
    }
    else if( c == 2 )
        s += g.below( 2 ) ? " 1" : "/";

    return s;
}

// ---- dumps ----

template <typename J>
static std::string dumps( const std::string& text, bool withStreams )
{
    J j;
    try
    {
        j = J::parse( text, nullptr, true, true );
    }
    catch( const std::exception& )
    {
        return "ERR";
    }

    std::string d2 = j.dump( 2 ) + '\n';
    std::string dc = j.dump();
    std::string r = hex64( fnvBytes( d2 ) ) + " " + hex64( fnvBytes( dc ) );

    if( withStreams )
    {
        std::stringstream s4, s0;
        s4 << std::setw( 4 ) << j << std::endl;
        s0 << std::setw( 0 ) << j << std::endl;
        r += " " + hex64( fnvBytes( s4.str() ) ) + " " + hex64( fnvBytes( s0.str() ) ) + " "
             + hex64( fnvBytes( j.dump( 0 ) ) );
    }

    return r;
}

// Integer kinds are not told apart: serde_json cannot keep nlohmann's number_integer for a
// non-negative value ("-0" is number_integer 0), see PORT_STATUS.md.
static std::string kind( const json& j )
{
    if( j.is_number_integer() )
        return "n";
    if( j.is_number_float() )
        return "f";
    return j.type_name();
}

static nlohmann::json::json_pointer pointerFromString( std::string aPath )
{
    // JSON_SETTINGS_INTERNALS::PointerFromString
    std::replace( aPath.begin(), aPath.end(), '.', '/' );
    aPath.insert( 0, "/" );
    nlohmann::json::json_pointer p;
    try
    {
        p = nlohmann::json::json_pointer( aPath );
    }
    catch( ... )
    {
    }
    return p;
}

template <typename T>
static std::string conv( const json& j, std::string ( *fmt )( const T& ) )
{
    try
    {
        return fmt( j.get<T>() );
    }
    catch( const json::exception& e )
    {
        return "throw " + std::to_string( e.id );
    }
}

static std::string fBool( const bool& v ) { return v ? "true" : "false"; }
static std::string fInt( const int& v ) { return std::to_string( v ); }
static std::string fUns( const unsigned& v ) { return std::to_string( v ); }
static std::string fI64( const int64_t& v ) { return std::to_string( v ); }
static std::string fU64( const uint64_t& v ) { return std::to_string( v ); }
static std::string fDbl( const double& v )
{
    uint64_t b;
    memcpy( &b, &v, 8 );
    return hex64( b );
}
static std::string fFlt( const float& v )
{
    uint32_t b;
    memcpy( &b, &v, 4 );
    char buf[16];
    snprintf( buf, sizeof buf, "%08x", b );
    return buf;
}
static std::string fStr( const std::string& v ) { return "\"" + esc( v ) + "\""; }
static std::string fVecI( const std::vector<int>& v )
{
    std::string s = "[";
    for( int x : v )
        s += std::to_string( x ) + ";";
    return s + "]";
}
static std::string fVecS( const std::vector<std::string>& v )
{
    std::string s = "[";
    for( const std::string& x : v )
        s += x + ";";
    return s + "]";
}
static std::string fMapI( const std::map<std::string, int>& v )
{
    std::string s = "{";
    for( const auto& [k, x] : v )
        s += k + "=" + std::to_string( x ) + ";";
    return s + "}";
}
static std::string fMapD( const std::map<std::string, double>& v )
{
    std::string s = "{";
    for( const auto& [k, x] : v )
        s += k + "=" + fDbl( x ) + ";";
    return s + "}";
}

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 1;

    std::string root = argv[1];
    g_full = argc > 3 && std::string( argv[3] ) == "full";

    // ---- files: every KiCad JSON file of qa/data and demos ----
    begin( "files" );
    {
        std::ifstream list( argv[2] );
        std::string   rel;

        while( std::getline( list, rel ) )
        {
            std::ifstream     f( root + "/" + rel, std::ios::binary );
            std::stringstream ss;
            ss << f.rdbuf();
            out( rel + " " + dumps<json>( ss.str(), true ) + " " + dumps<ojson>( ss.str(), false ) );
        }
    }

    // ---- doubles: dump of 200000 pseudo-random doubles ----
    begin( "doubles" );
    {
        LCG g{ 0xC3D0 };
        for( int i = 0; i < 200000; ++i )
        {
            double d = rdouble( g );
            out( fDbl( d ) + " " + json( d ).dump() );
        }
    }

    // ---- nums: parse of pseudo-random number texts ----
    begin( "nums" );
    {
        LCG g{ 0xC3D1 };
        for( int i = 0; i < 50000; ++i )
        {
            std::string t = genNumber( g );
            try
            {
                json j = json::parse( t, nullptr, true, true );
                out( t + " " + kind( j ) + " " + j.dump() );
            }
            catch( const json::exception& e )
            {
                out( t + " ERR" );
            }
        }
    }

    // ---- docs: parse and dump of pseudo-random documents ----
    begin( "docs" );
    {
        LCG g{ 0xC3D2 };
        for( int i = 0; i < 20000; ++i )
        {
            std::string t = genDocument( g );
            std::string line = std::to_string( i ) + " " + dumps<json>( t, true ) + " "
                               + dumps<ojson>( t, false );

            if( i < 300 )
            {
                try
                {
                    line += " " + esc( json::parse( t, nullptr, true, true ).dump() );
                }
                catch( ... )
                {
                }
            }

            out( line );
        }
    }

    // ---- get: nlohmann get<T>() conversions ----
    begin( "get" );
    {
        const char* values[] = { "true", "false", "null", "0", "1", "-1", "2147483647", "2147483648",
                                 "-2147483649", "4294967296", "1.5", "-1.5", "2.9999", "-0.0", "-0",
                                 "1e10", "3e9", "-3e9", "1e19", "1e20", "-1e19", "9.3e18",
                                 "1.8446744073709552e19", "\"str\"", "\"\"", "[]", "{}", "[1,2]",
                                 "{\"a\":1}", "[1,\"x\"]", "{\"a\":true}", "18446744073709551615",
                                 "-9223372036854775808", "1e300", "-1e300", "4294967295.5",
                                 "2147483647.9", "-2147483648.9", "-2147483648", "[true,2.5]",
                                 "{\"b\":2.5,\"a\":-3}", "[\"a\",\"b\"]", "{\"a\":\"x\"}", "3.4e38",
                                 "3.5e38", "1e-50", "[[1]]", "-4294967297", "9.223372036854775807e18",
                                 "-9.3e18" };

        for( const char* v : values )
        {
            json j = json::parse( v );
            std::string line = std::string( v ) + " | " + conv<bool>( j, fBool ) + " | "
                               + conv<int>( j, fInt ) + " | " + conv<unsigned>( j, fUns ) + " | "
                               + conv<int64_t>( j, fI64 ) + " | " + conv<uint64_t>( j, fU64 )
                               + " | " + conv<double>( j, fDbl ) + " | " + conv<float>( j, fFlt )
                               + " | " + conv<std::string>( j, fStr ) + " | "
                               + conv<std::vector<int>>( j, fVecI ) + " | "
                               + conv<std::vector<std::string>>( j, fVecS ) + " | "
                               + conv<std::map<std::string, int>>( j, fMapI ) + " | "
                               + conv<std::map<std::string, double>>( j, fMapD );
            out( line );
        }
    }

    // ---- ptr: dotted paths (JSON_SETTINGS::Contains / GetJson / operator[]) ----
    begin( "ptr" );
    {
        json doc = json::parse( R"({"a":{"b":[1,{"c":2}],"x":{"y":7},"~":4,"t/1":5,"t~1":8,"":6},
                                    "n":null,"s":"str","arr":[0,1,2,3,4,5,6,7,8,9,10,11],"e":[]})" );
        const char* paths[] = { "a", "a.b", "a.b.0", "a.b.1.c", "a.b.2", "a.b.01", "a.b.-", "a.b.",
                                "a.b.x", "a.x.y", "a.~", "a.~0", "a.t~1", "a.t~01", "a.", "n",
                                "n.x", "n.0", "s.x", "arr.10", "arr.12", "arr.1a", "", "missing",
                                "a.b.1.c.d", "arr.99999999999999999999", "arr.18446744073709551615",
                                "e.0", "e.-", "e.", "new.path.x", "arr.13", "a.b.1.z", "s",
                                "a.t~", "e.00", "e.1" };

        for( const char* p : paths )
        {
            nlohmann::json::json_pointer ptr = pointerFromString( p );
            std::string line = std::string( "[" ) + p + "] ";

            try
            {
                line += doc.contains( ptr ) ? "1" : "0";
            }
            catch( const json::exception& e )
            {
                line += "throw " + std::to_string( e.id );
            }

            try
            {
                line += " " + doc.at( ptr ).dump();
            }
            catch( const json::exception& e )
            {
                line += " throw " + std::to_string( e.id );
            }

            json copy = doc;

            try
            {
                copy[ptr] = 42;
                line += " " + copy.dump();
            }
            catch( const json::exception& e )
            {
                line += " throw " + std::to_string( e.id );
            }

            out( line );
        }
    }

    // ---- eq / update ----
    begin( "eq" );
    {
        const char* pairs[][2] = { { "1", "1.0" },      { "-1", "-1.0" },     { "0", "-0.0" },
                                   { "1", "true" },     { "{\"a\":1}", "{\"a\":1.0}" },
                                   { "[1,2]", "[1.0,2]" }, { "[1,2]", "[2,1]" },
                                   { "{\"a\":1,\"b\":2}", "{\"b\":2,\"a\":1}" },
                                   { "null", "{}" },    { "\"a\"", "\"a\"" },
                                   { "18446744073709551615", "-1" },
                                   { "9007199254740993", "9007199254740992.0" },
                                   { "{}", "[]" },      { "0.1", "0.1" } };

        for( auto& p : pairs )
            out( std::string( p[0] ) + " == " + p[1] + " : "
                 + ( json::parse( p[0] ) == json::parse( p[1] ) ? "1" : "0" ) );

        const char* upd[][2] = { { "{\"a\":{\"x\":1,\"y\":2},\"b\":1}", "{\"a\":{\"x\":3,\"z\":4},\"c\":5}" },
                                 { "null", "{\"a\":1}" },
                                 { "{\"a\":1}", "{\"a\":{\"b\":2}}" },
                                 { "{\"a\":{\"b\":2}}", "{\"a\":1}" },
                                 { "[1]", "{\"a\":1}" },
                                 { "{\"a\":1}", "[1]" } };

        for( auto& u : upd )
        {
            for( bool merge : { false, true } )
            {
                json t = json::parse( u[0] );
                std::string line = std::string( u[0] ) + " <- " + u[1] + ( merge ? " merge" : "" ) + " : ";
                try
                {
                    t.update( json::parse( u[1] ), merge );
                    line += t.dump();
                }
                catch( const json::exception& e )
                {
                    line += "throw " + std::to_string( e.id );
                }
                out( line );
            }
        }
    }

    flush();
    return 0;
}
