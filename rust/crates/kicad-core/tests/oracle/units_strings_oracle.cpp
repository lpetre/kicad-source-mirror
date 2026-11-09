// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// Differential oracle of work package C2 (units, number formatting, strings, colours, pages)
// of the kicad-core crate.  It calls the original functions in libkicommon.so of the reference
// build on deterministic edge-case and pseudo-random inputs and prints one line per result.
//
//   units_strings_oracle          golden mode: per section the first lines verbatim
//                                 ("E <section> <line>") and "S <section> <count> <fnv64>"
//   units_strings_oracle full     every line, prefixed with the section name
//
// Lines are ASCII: control characters and every non-ASCII byte of the UTF-8 text are written
// as \xHH.
//
// tests/test_c2_oracle.rs produces the same lines with the Rust port.  Keep the two in sync:
// the order of the pseudo-random draws matters.

#include <clocale>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include <base_units.h>
#include <eda_units.h>
#include <gal/color4d.h>
#include <page_info.h>
#include <richio.h>
#include <string_utils.h>
#include <title_block.h>

using namespace KIGFX;

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
        uint64_t a = next();
        uint64_t b = next();
        uint64_t c = next();
        return ( a << 42 ) ^ ( b << 21 ) ^ c;
    }
    uint32_t below( uint32_t n ) { return (uint32_t) ( next() % n ); }
};

static const double P10[23] = { 1e0,  1e1,  1e2,  1e3,  1e4,  1e5,  1e6,  1e7,
                                1e8,  1e9,  1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
                                1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22 };

static const double SPECIAL_D[] = { 0.0,     -0.0,   1e-4,    -1e-4,  1.0000000000000002e-4,
                                    9.9999e-5, 0.5,  2.5,     -2.5,   1e16,
                                    123456789.0, 1e-300, 5e-324, 1.7976931348623157e308,
                                    0.1,     0.3,    25.4,    0.0254, 1e10, 99999.99999 };

static double rdouble( LCG& g )
{
    switch( g.below( 6 ) )
    {
    case 0:
    {
        uint64_t bits = g.u64();
        double   d;
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
    case 4: return SPECIAL_D[g.below( sizeof( SPECIAL_D ) / sizeof( double ) )];
    default:
    {
        double v = (double) ( (int64_t) g.next() - 1073741824 ) + 0.5;
        return v / P10[g.below( 8 )];
    }
    }
}

static const int SPECIAL_I[] = { 0, 1, -1, 2147483647, -2147483647 - 1, 100, 50, -50, 1000000, 25400, 127, 5 };
static const int MULT_I[] = { 1, 10, 100, 1000, 25400, 127, 254000, 2540 };

static int rint( LCG& g )
{
    switch( g.below( 4 ) )
    {
    case 0: return (int) (uint32_t) g.u64();
    case 1: return (int) g.below( 2001 ) - 1000;
    case 2:
    {
        int v = (int) g.below( 20001 ) - 10000;
        return v * MULT_I[g.below( 8 )];
    }
    default: return SPECIAL_I[g.below( sizeof( SPECIAL_I ) / sizeof( int ) )];
    }
}

static wxString rstring( LCG& g, const std::vector<wxString>& frags, uint32_t maxParts )
{
    wxString s;
    uint32_t n = g.below( maxParts + 1 );

    for( uint32_t i = 0; i < n; ++i )
        s += frags[g.below( (uint32_t) frags.size() )];

    return s;
}

// ---- output ----

static bool        g_full = false;
static std::string g_section;
static uint64_t    g_hash;
static int         g_count;

static std::string esc( const std::string& s )
{
    std::string out;

    for( unsigned char c : s )
    {
        if( c == '\\' )
            out += "\\\\";
        else if( c == '\n' )
            out += "\\n";
        else if( c == '\r' )
            out += "\\r";
        else if( c == '\t' )
            out += "\\t";
        else if( c < 0x20 || c >= 0x7F )
        {
            char buf[8];
            snprintf( buf, sizeof( buf ), "\\x%02X", c );
            out += buf;
        }
        else
            out += (char) c;
    }

    return out;
}

static std::string U( const wxString& s ) { return esc( s.utf8_string() ); }

static std::string H( double d )
{
    uint64_t bits;
    memcpy( &bits, &d, 8 );
    char buf[24];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) bits );
    return buf;
}

static void begin( const char* name )
{
    g_section = name;
    g_hash = 1469598103934665603ULL;
    g_count = 0;
}

static void out( const std::string& line )
{
    for( unsigned char c : line )
    {
        g_hash ^= c;
        g_hash *= 1099511628211ULL;
    }

    g_hash ^= '\n';
    g_hash *= 1099511628211ULL;

    if( g_full )
        printf( "%s %s\n", g_section.c_str(), line.c_str() );
    else if( g_count < 12 )
        printf( "E %s %s\n", g_section.c_str(), line.c_str() );

    g_count++;
}

static void end()
{
    if( !g_full )
        printf( "S %s %d %016llx\n", g_section.c_str(), g_count, (unsigned long long) g_hash );
}

static std::string I( long long v ) { return std::to_string( v ); }

// ---- data ----

static const EDA_IU_SCALE* SCALES[] = { &pcbIUScale, &schIUScale, &gerbIUScale, &drawSheetIUScale,
                                        &unityScale };

static wxString W( const char* utf8 ) { return wxString::FromUTF8( utf8 ); }

static std::vector<wxString> frags( std::initializer_list<const char*> l )
{
    std::vector<wxString> v;

    for( const char* s : l )
        v.push_back( W( s ) );

    return v;
}

int main( int argc, char** argv )
{
    // KiCad runs in a UTF-8 locale with the "C" numeric locale.
    setlocale( LC_ALL, "C.utf8" );
    setlocale( LC_NUMERIC, "C" );

    g_full = argc > 1 && std::string( argv[1] ) == "full";

    LCG g{ 0x5eed1234abcdULL };

    // 1. FormatInternalUnits / FormatAngle / FormatDouble2Str / UIDouble2Str
    begin( "fiu" );

    for( int si = 0; si < 5; ++si )
    {
        for( int t = 0; t < 7; ++t )
        {
            for( int k = 0; k < 400; ++k )
            {
                int v = rint( g );
                out( I( si ) + " " + I( t ) + " " + I( v ) + " "
                     + EDA_UNIT_UTILS::FormatInternalUnits( *SCALES[si], v, (EDA_DATA_TYPE) t ) );
            }
        }

        for( int k = 0; k < 200; ++k )
        {
            int x = rint( g );
            int y = rint( g );
            out( I( si ) + " P " + EDA_UNIT_UTILS::FormatInternalUnits( *SCALES[si], VECTOR2I( x, y ) ) );
        }
    }

    end();

    begin( "angle" );

    for( int k = 0; k < 4000; ++k )
    {
        double d = rdouble( g );
        out( H( d ) + " " + EDA_UNIT_UTILS::FormatAngle( EDA_ANGLE( d, DEGREES_T ) ) + " "
             + U( EDA_UNIT_UTILS::UI::MessageTextFromValue( EDA_ANGLE( d, DEGREES_T ), true ) ) + " "
             + U( EDA_UNIT_UTILS::UI::MessageTextFromValue( EDA_ANGLE( d, DEGREES_T ), false ) ) );
    }

    end();

    begin( "double2str" );

    for( int k = 0; k < 12000; ++k )
    {
        double d = rdouble( g );
        out( H( d ) + " " + FormatDouble2Str( d ) + " " + UIDouble2Str( d ) );
    }

    end();

    begin( "trimmed" );

    for( int k = 0; k < 4000; ++k )
    {
        double d = rdouble( g );
        int    p = (int) g.below( 12 ) - 1;
        out( H( d ) + " " + I( p ) + " " + U( FormatTrimmedDecimal( d, p ) ) );
    }

    end();

    // 2. StringFromValue / MessageTextFromValue
    begin( "sfv" );

    for( int si = 0; si < 5; ++si )
    {
        for( int u = 0; u <= 12; ++u )
        {
            for( int t = 0; t < 7; ++t )
            {
                for( int k = 0; k < 24; ++k )
                {
                    double d = ( k & 1 ) ? (double) rint( g ) : rdouble( g );
                    bool   add = g.below( 2 );
                    out( I( si ) + " " + I( u ) + " " + I( t ) + " " + I( add ) + " " + H( d ) + " "
                         + U( EDA_UNIT_UTILS::UI::StringFromValue( *SCALES[si], (EDA_UNITS) u, d, add,
                                                                   (EDA_DATA_TYPE) t ) )
                         + " "
                         + U( EDA_UNIT_UTILS::UI::MessageTextFromValue( *SCALES[si], (EDA_UNITS) u, d, add,
                                                                        (EDA_DATA_TYPE) t ) ) );
                }
            }
        }
    }

    end();

    begin( "mtfv_int" );

    for( int si = 0; si < 5; ++si )
    {
        for( int u = 0; u <= 12; ++u )
        {
            for( int k = 0; k < 40; ++k )
            {
                int       v = rint( g );
                long long l1 = rint( g );
                long long l2 = rint( g );
                long long l = l1 * l2;
                out( I( si ) + " " + I( u ) + " " + I( v ) + " " + I( l ) + " "
                     + U( EDA_UNIT_UTILS::UI::MessageTextFromValue( *SCALES[si], (EDA_UNITS) u, v ) ) + " "
                     + U( EDA_UNIT_UTILS::UI::MessageTextFromValue( *SCALES[si], (EDA_UNITS) u, l ) ) );
            }

            for( int k = 0; k < 8; ++k )
            {
                MINOPTMAX<int> m;
                uint32_t       mask = g.below( 8 );
                int            a = rint( g );
                int            b = rint( g );
                int            c = rint( g );

                if( mask & 1 )
                    m.SetMin( a );

                if( mask & 2 )
                    m.SetOpt( b );

                if( mask & 4 )
                    m.SetMax( c );

                out( I( si ) + " " + I( u ) + " M " + I( mask ) + " " + I( a ) + " " + I( b ) + " " + I( c ) + " "
                     + U( EDA_UNIT_UTILS::UI::MessageTextFromMinOptMax( *SCALES[si], (EDA_UNITS) u, m ) ) );
            }
        }
    }

    end();

    begin( "userunit" );

    for( int si = 0; si < 5; ++si )
    {
        for( int u = 0; u <= 12; ++u )
        {
            for( int k = 0; k < 30; ++k )
            {
                double d = rdouble( g );
                out( I( si ) + " " + I( u ) + " " + H( d ) + " "
                     + H( EDA_UNIT_UTILS::UI::ToUserUnit( *SCALES[si], (EDA_UNITS) u, d ) ) + " "
                     + H( EDA_UNIT_UTILS::UI::FromUserUnit( *SCALES[si], (EDA_UNITS) u, d ) ) );
            }
        }
    }

    for( int k = 0; k < 400; ++k )
    {
        double d = rdouble( g );
        out( "mm " + H( d ) + " " + I( EDA_UNIT_UTILS::Mm2mils( d ) ) + " " + I( EDA_UNIT_UTILS::Mils2mm( d ) ) );
    }

    for( int u = 0; u <= 12; ++u )
        for( int t = 0; t < 7; ++t )
            out( "txt " + I( u ) + " " + I( t ) + " " + U( EDA_UNIT_UTILS::GetText( (EDA_UNITS) u, (EDA_DATA_TYPE) t ) )
                 + "|" + U( EDA_UNIT_UTILS::GetLabel( (EDA_UNITS) u, (EDA_DATA_TYPE) t ) ) + "|"
                 + I( EDA_UNIT_UTILS::IsImperialUnit( (EDA_UNITS) u ) )
                 + I( EDA_UNIT_UTILS::IsMetricUnit( (EDA_UNITS) u ) ) );

    end();

    // 3. Parsing of user values
    const std::vector<wxString> numFrags =
            frags( { "0", "1", "2", "5", "9", "12", ".", ",", "-", "+", " ", "  ", "mm", "um", "\xC2\xB5m",
                     "\xCE\xBCm", "cm", "mil", "mils", "thou", "th", "mi", "in", "\"", "oz", "deg", "rad", "ra",
                     "de", "fs", "ps", "ps/in", "ps/cm", "ps/mm", "PS/MM", "Mm", "IN", "x", "e5", "\t", "1e3" } );

    begin( "parse" );

    for( int k = 0; k < 6000; ++k )
    {
        wxString s = rstring( g, numFrags, 6 );
        int      si = (int) g.below( 5 );
        int      u = (int) g.below( 13 );
        int      t = (int) g.below( 7 );

        EDA_UNITS fu = EDA_UNITS::UNSCALED;
        bool      fok = EDA_UNIT_UTILS::FetchUnitsFromString( s, fu );
        double    dv = 12345.0;
        bool      dok = EDA_UNIT_UTILS::UI::DoubleValueFromString( *SCALES[si], s, dv );

        out( U( s ) + "|" + I( si ) + " " + I( u ) + " " + I( t ) + " " + I( fok ) + " " + I( (int) fu ) + " "
             + H( EDA_UNIT_UTILS::UI::DoubleValueFromString( *SCALES[si], (EDA_UNITS) u, s, (EDA_DATA_TYPE) t ) )
             + " " + H( EDA_UNIT_UTILS::UI::DoubleValueFromString( s ) ) + " " + I( dok ) + " " + H( dv ) + " "
             + I( EDA_UNIT_UTILS::UI::ValueFromString( *SCALES[si], (EDA_UNITS) u, s, (EDA_DATA_TYPE) t ) ) + " "
             + I( EDA_UNIT_UTILS::UI::ValueFromString( s ) ) );
    }

    end();

    // 4. EscapeString / UnescapeString
    const std::vector<wxString> escFrags = frags(
            { "a", "b", "c", "Z", "0", "9", " ", "/", "\\", "<", ">", ":", "\"", "'", ",", "|", "{", "}", "$", "~",
              "^", "_", "\n", "\r", "\t", ".", "(", ")", "-", "[", "]", "+", "*", "?", "\x7F", "\xC2\xB5",
              "\xCE\xBC", "\xE2\x82\xAC", "\xF0\x9F\x98\x80", "\xC3\xA9", "\xC3\x9F", "\xCE\xA9", "\xE2\x80\x98",
              "\xE2\x80\x9D", "\xE2\x80\x93", "\xE2\x80\x94", "{slash}", "{dblquote}" } );

    begin( "escape" );

    for( int k = 0; k < 20000; ++k )
    {
        wxString s = rstring( g, escFrags, 12 );
        int      ctx = (int) g.below( 11 );
        wxString e = EscapeString( s, (ESCAPE_CONTEXT) ctx );
        out( I( ctx ) + " " + U( s ) + "|" + U( e ) + "|" + U( UnescapeString( e ) ) );
    }

    end();

    const std::vector<wxString> unescFrags =
            frags( { "{slash}", "{dblquote}", "{quote}", "{lt}", "{gt}", "{backslash}", "{bar}", "{comma}", "{colon}",
                     "{space}", "{dollar}", "{tab}", "{return}", "{brace}", "{unknown}", "{", "}", "{{", "}}", "$",
                     "~", "^", "_", "a", "x", " ", "{slash", "\\", "\xC2\xB5", "${VAR}", "~{over}", "{}" } );

    begin( "unescape" );

    for( int k = 0; k < 20000; ++k )
    {
        wxString s = rstring( g, unescFrags, 10 );
        out( U( s ) + "|" + U( UnescapeString( s ) ) );
    }

    end();

    // 5. Other string functions
    begin( "strmisc" );

    const std::vector<wxString> cmpFrags =
            frags( { "a", "A", "b", "B", "0", "1", "01", "9", "10", "100", "2147483648", "99999999999", " ", ".",
                     "-", ",", "U", "u", "\xC3\xA9", "\xC3\x89", "\xC3\x9F", "_", "z" } );

    for( int k = 0; k < 6000; ++k )
    {
        wxString a = rstring( g, cmpFrags, 5 );
        wxString b = rstring( g, cmpFrags, 5 );
        out( "cmp " + U( a ) + "|" + U( b ) + "|" + I( StrNumCmp( a, b, CMP_T::CASE_SENSITIVE ) ) + " "
             + I( StrNumCmp( a, b, CMP_T::IGNORE_CASE ) ) + " " + I( SortVariantNames( a, b ) ) + " "
             + I( GetTrailingInt( a ) ) );
    }

    const std::vector<wxString> valFrags =
            frags( { "1", "0", "2", "5", "9", "47", ".", ",", "K", "k", "R", "u", "\xC2\xB5", "\xCE\xBC", "n", "p",
                     "F", "M", "G", "T", "m", "L", "\xCE\xA9", "V", "A", "H", "s", "Hz", "hz", " ", "{slash}", "a",
                     "X", "W", "mils", "in", "E", "P", "f", "c", "000", "1,000" } );

    for( int k = 0; k < 8000; ++k )
    {
        wxString a = rstring( g, valFrags, 5 );
        wxString b = rstring( g, valFrags, 5 );
        wxString beg, dig, end_;
        SplitString( a, &beg, &dig, &end_ );
        double qv = 0;
        bool   q = QuacksLikeADuck( a, &qv );
        out( "val " + U( a ) + "|" + U( b ) + "|" + I( ValueStringCompare( a, b ) ) + " " + U( beg ) + "|"
             + U( dig ) + "|" + U( end_ ) + "|" + I( q ) + " " + ( q ? H( qv ) : std::string( "-" ) ) );
    }

    const std::vector<wxString> wildP = frags( { "a", "b", "*", "?", "A", "\xC3\xA9" } );
    const std::vector<wxString> wildS = frags( { "a", "b", "A", "B", "\xC3\x89" } );

    for( int k = 0; k < 4000; ++k )
    {
        wxString p = rstring( g, wildP, 6 );
        wxString s = rstring( g, wildS, 6 );
        out( "wild " + U( p ) + "|" + U( s ) + "|" + I( WildCompareString( p, s, true ) )
             + I( WildCompareString( p, s, false ) ) );
    }

    const std::vector<wxString> textFrags =
            frags( { "~", "~~", "{", "}", " ", ")", "a", "B", "^", "_", "\t", "x y", "\xC3\xA9", "\xC3\x9F", "hELLO",
                     "\xE2\x80\x98", "\xE2\x80\x9C", "\xE2\x80\x94", "\"", "\\", "\n", "*", "?", "/", ":", "<",
                     "&", ";", "#", "&amp;", "&lt;", "&#65;", "&#x20AC;", "&#x0x12;", "&#;", "&quot;", "&apos;",
                     "&gt;", "&#xzz12;", "&#99999999999999999999;", ">" } );

    for( int k = 0; k < 8000; ++k )
    {
        wxString s = rstring( g, textFrags, 8 );
        wxString sq = s;
        bool     sqc = ConvertSmartQuotesAndDashes( &sq );
        wxString ill = s;
        bool     illc = ReplaceIllegalFileNameChars( ill, 0 );
        wxString ill2 = s;
        ReplaceIllegalFileNameChars( ill2, '_' );
        std::string ills = s.utf8_string();
        bool        illsc = ReplaceIllegalFileNameChars( ills, 0 );
        out( "txt " + U( s ) + "|" + U( ConvertToNewOverbarNotation( s ) ) + "|" + U( TitleCaps( s ) ) + "|"
             + U( InitialCaps( s ) ) + "|" + esc( EscapedUTF8( s ) ) + "|" + U( EscapeHTML( s ) ) + "|"
             + U( UnescapeHTML( s ) ) + "|" + U( RemoveHTMLTags( s ) ) + "|" + I( PrintableCharCount( s ) ) + "|"
             + I( NoPrintableChars( s ) ) + I( IsFullFileNameValid( s ) ) + I( sqc ) + I( illc ) + I( illsc ) + "|"
             + U( sq ) + "|" + U( ill ) + "|" + U( ill2 ) + "|" + esc( ills ) + "|"
             + U( GetVariantFileToken( s ) ) );
    }

    const std::vector<wxString> delimFrags = frags( { "\"", "\\", "a", "b", " ", "\xC3\xA9", "\\\"", "\\\\", "n" } );

    for( int k = 0; k < 3000; ++k )
    {
        std::string src = rstring( g, delimFrags, 8 ).utf8_string();
        int         size = (int) g.below( 12 ) - 1;
        wxString    dest;
        int         n = ReadDelimitedText( &dest, src.c_str() );
        char        buf[16];
        memset( buf, 0, sizeof( buf ) );
        int n2 = ReadDelimitedText( buf, src.c_str(), size );
        out( "delim " + esc( src ) + "|" + U( dest ) + "|" + I( n ) + "|" + I( size ) + " " + esc( buf ) + "|"
             + I( n2 ) );
    }

    const std::vector<wxString> zeroFrags = frags( { "0", "1", ".", "00", "5", "-" } );

    for( int k = 0; k < 2000; ++k )
    {
        wxString s = rstring( g, zeroFrags, 6 );
        unsigned allowed = g.below( 4 );
        wxString z = s;
        StripTrailingZeros( z, allowed );
        out( "zeros " + U( s ) + " " + I( allowed ) + " " + U( z ) );
    }

    const std::vector<wxString> pinFrags = frags( { "[", "]", ",", "-", "\\", "A", "B", "1", "2", "10", " ", "a",
                                                    "99999999999999999999" } );

    for( int k = 0; k < 6000; ++k )
    {
        wxString s = rstring( g, pinFrags, 8 );
        bool     valid = false;
        bool     cvalid = false;
        std::vector<wxString> pins = ExpandStackedPinNotation( s, &valid );
        int                   count = CountStackedPinNotation( s, &cvalid );
        std::string           line = "pins " + U( s ) + "|" + I( valid ) + " " + I( count ) + " " + I( cvalid );

        for( size_t i = 0; i < pins.size() && i < 20; ++i )
            line += "|" + U( pins[i] );

        line += "|" + I( (long long) pins.size() ) + "|" + U( EscapeStackedPinItem( s ) );

        for( const wxString& item : SplitStackedPinDisplayItems( s ) )
            line += "|" + U( item );

        out( line );
    }

    end();

    // 6. COLOR4D
    begin( "color" );

    const std::vector<wxString> colFrags =
            frags( { "rgb", "RGBA", "rgba", "Rgb", "(", ")", ",", " ", "0", "255", "300", "-5", "0.5", "1", "128",
                     "#", "ff", "FF", "00", "12", "abc", "g", "1e3", ".", "0x", "-", "  ", "\t" } );

    for( int k = 0; k < 8000; ++k )
    {
        wxString s = ( k % 3 == 0 ) ? wxString( "#" ) + rstring( g, colFrags, 4 ) : rstring( g, colFrags, 9 );
        COLOR4D  a;
        bool     wok = a.SetFromWxString( s );
        COLOR4D  b;
        bool     hok = b.SetFromHexString( s );
        COLOR4D  c( s );
        out( U( s ) + "|" + I( wok ) + " " + H( a.r ) + H( a.g ) + H( a.b ) + H( a.a ) + " " + I( hok ) + " "
             + H( b.r ) + H( b.g ) + H( b.b ) + H( b.a ) + " " + I( c.m_text != nullptr ) + H( c.r ) + H( c.a ) );
    }

    for( int k = 0; k < 6000; ++k )
    {
        double r = (double) g.below( 1001 ) / 1000.0;
        double gg = (double) g.below( 1001 ) / 1000.0;
        double b = (double) g.below( 1001 ) / 1000.0;
        double a = (double) g.below( 1001 ) / 1000.0;
        double f = (double) g.below( 101 ) / 100.0;
        COLOR4D c( r, gg, b, a );
        double  h, s, v, h2, s2, l;
        c.ToHSV( h, s, v, false );
        c.ToHSL( h2, s2, l );
        COLOR4D fromHsv;
        fromHsv.FromHSV( h2, s2, l );
        COLOR4D fromHsl;
        fromHsl.FromHSL( h2, s2, l );
        COLOR4D sat = c;
        sat.Saturate( f );
        COLOR4D desat = c;
        desat.Desaturate();
        COLOR4D other( a, r, f, gg );
        COLOR4D mix = c.LegacyMix( other );
        wxColour wc = c.ToColour();
        out( H( r ) + H( gg ) + H( b ) + H( a ) + " " + H( f ) + " " + H( h ) + H( s ) + H( v ) + " " + H( h2 )
             + H( s2 ) + H( l ) + " " + H( fromHsv.r ) + H( fromHsv.g ) + H( fromHsv.b ) + " " + H( fromHsl.r )
             + H( fromHsl.g ) + H( fromHsl.b ) + " " + H( sat.r ) + H( sat.g ) + H( sat.b ) + " " + H( desat.r )
             + H( desat.g ) + H( desat.b ) + " " + H( mix.r ) + H( mix.g ) + H( mix.b ) + H( mix.a ) + " "
             + U( c.ToCSSString() ) + " " + U( c.ToHexString() ) + " " + I( wc.Red() ) + I( wc.Alpha() ) + " "
             + H( c.Distance( other ) ) + " " + I( c.Compare( other ) ) + I( c < other ) + I( c == other ) + " "
             + H( c.RelativeLuminance() ) + " " + H( COLOR4D::ContrastRatio( c, other ) ) + " "
             + H( c.GetBrightness() ) + " " + H( c.Mix( other, f ).r ) + H( c.Brightened( f ).g )
             + H( c.Darkened( f ).b ) );
    }

    for( int k = 0; k < 3000; ++k )
    {
        int r = (int) g.below( 300 ) - 20;
        int gg = (int) g.below( 300 ) - 20;
        int b = (int) g.below( 300 ) - 20;
        out( "near " + I( r ) + " " + I( gg ) + " " + I( b ) + " "
             + I( COLOR4D::FindNearestLegacyColor( r, gg, b ) ) );
    }

    for( int i = -2; i <= 36; ++i )
    {
        COLOR4D c( (EDA_COLOR_T) i );
        out( "eda " + I( i ) + " " + H( c.r ) + H( c.g ) + H( c.b ) + H( c.a ) );
    }

    end();

    // 7. PAGE_INFO / TITLE_BLOCK
    begin( "page" );

    for( int t = 0; t <= 15; ++t )
    {
        for( int portrait = 0; portrait < 2; ++portrait )
        {
            PAGE_INFO p( (PAGE_SIZE_TYPE) t, portrait );
            STRING_FORMATTER f;
            p.Format( &f );
            out( I( t ) + " " + I( portrait ) + " " + U( p.GetTypeAsString() ) + " " + H( p.GetWidthMils() ) + " "
                 + H( p.GetHeightMils() ) + " " + I( p.IsPortrait() ) + I( p.IsCustom() ) + I( p.IsDefault() )
                 + " " + I( (int) p.GetPaperId() ) + " " + U( p.GetPageFormatDescription() ) + " "
                 + I( p.GetWidthIU( 1000.0 ) ) + " " + I( p.GetHeightIU( 25.4 ) ) + " " + f.GetString() );
        }
    }

    const std::vector<wxString> names = frags( { "A4", "a4", "USLETTER", "user", "User", "GERBER", "A6", "", "a" } );

    for( int k = 0; k < 400; ++k )
    {
        PAGE_INFO p;
        wxString  name = names[g.below( (uint32_t) names.size() )];
        bool      portrait = g.below( 2 );
        bool      ok = p.SetType( name, portrait );
        double    w = rdouble( g );
        double    h = (double) rint( g );

        if( std::isfinite( w ) && std::fabs( w ) < 1e9 && g.below( 2 ) )
            p.SetWidthMils( w );

        if( g.below( 2 ) )
            p.SetHeightMils( h );

        if( g.below( 3 ) == 0 )
            p.SetPortrait( !p.IsPortrait() );

        STRING_FORMATTER f;
        p.Format( &f );
        out( U( name ) + " " + I( portrait ) + " " + I( ok ) + " " + H( w ) + " " + H( h ) + " "
             + U( p.GetTypeAsString() ) + " " + H( p.GetWidthMils() ) + " " + H( p.GetHeightMils() ) + " "
             + I( p.IsPortrait() ) + " " + f.GetString() );
    }

    for( int k = 0; k < 300; ++k )
    {
        TITLE_BLOCK tb;
        uint32_t    mask = g.below( 1 << 13 );
        const std::vector<wxString> tbFrags = frags( { "a", "\"", "\\", "\n", "\xC3\xA9", " ", "x" } );
        std::vector<wxString>       texts;

        for( int i = 0; i < 13; ++i )
            texts.push_back( rstring( g, tbFrags, 3 ) );

        if( mask & 1 ) tb.SetTitle( texts[0] );
        if( mask & 2 ) tb.SetDate( texts[1] );
        if( mask & 4 ) tb.SetRevision( texts[2] );
        if( mask & 8 ) tb.SetCompany( texts[3] );

        for( int i = 0; i < 9; ++i )
            if( mask & ( 16 << i ) )
                tb.SetComment( i, texts[4 + i] );

        STRING_FORMATTER f;
        tb.Format( &f );
        out( "tb " + I( mask ) + " " + esc( f.GetString() ) );
    }

    end();

    return 0;
}
