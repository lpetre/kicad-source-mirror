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

// Differential oracle for the kicad-font crate (see rust/docs/ORACLE.md and
// crates/kicad-font/tests/test_font_oracle.rs, which mirrors the case generator and the
// output format of this file exactly).
//
//   font_oracle golden          -> tests/golden/font_oracle.txt (hashes per case)
//   font_oracle full [KIND IDX] -> the full output text of every case (or of one case)
//   font_oracle strings         -> the raw glyph table strings, one per line

#include <algorithm>
#include <cinttypes>
#include <climits>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include <callback_gal.h>
#include <font/font.h>
#include <font/stroke_font.h>
#include <font/glyph_types.h>
#include <font/text_attributes.h>
#include <gr_text.h>
#include <wx/debug.h>
#include <markup_parser.h>
#include <newstroke_font.h>

using namespace KIFONT;

// Exposes the protected layout members of FONT.
struct FONT_PEEK : public STROKE_FONT
{
    using FONT::getLinePositions;
};

static uint64_t g_state;

static uint64_t next64()
{
    g_state += 0x9E3779B97F4A7C15ULL;
    uint64_t z = g_state;
    z = ( z ^ ( z >> 30 ) ) * 0xBF58476D1CE4E5B9ULL;
    z = ( z ^ ( z >> 27 ) ) * 0x94D049BB133111EBULL;
    return z ^ ( z >> 31 );
}

static int64_t rnd( int64_t n )
{
    return (int64_t) ( next64() % (uint64_t) n );
}

static const char* TOKENS[] = { "~{", "_{", "^{", "}", "http://", "https://", "{slash}", "{dblquote}",
                                "{ab_1}", "{a b}", "  ", "~{}", "~{ }", "x}" };
static const int TOKEN_COUNT = sizeof( TOKENS ) / sizeof( TOKENS[0] );

static uint32_t genChar()
{
    int64_t r = rnd( 100 );

    if( r < 45 ) return 0x21 + rnd( 94 );
    if( r < 57 ) return ' ';
    if( r < 67 ) return "~_^{}"[rnd( 5 )];
    if( r < 70 ) return '\t';
    if( r < 72 ) return '\n';
    if( r < 74 ) return 1 + rnd( 31 );
    if( r < 80 ) return 0xA0 + rnd( 96 );
    if( r < 85 ) return 0x391 + rnd( 57 );
    if( r < 90 ) return 0x410 + rnd( 64 );
    if( r < 94 ) return 0x4E00 + rnd( 0x5200 );
    if( r < 96 ) return 0x1F600 + rnd( 80 );
    if( r < 97 ) return 0xFFEF + rnd( 16 );
    if( r < 98 ) return 0;
    return 0x100 + rnd( 0xD800 - 0x100 );
}

static std::vector<uint32_t> genText( int aMaxLen )
{
    std::vector<uint32_t> cps;
    int64_t len = rnd( aMaxLen + 1 );

    for( int64_t i = 0; i < len; i++ )
    {
        if( rnd( 6 ) == 0 )
        {
            for( const char* p = TOKENS[rnd( TOKEN_COUNT )]; *p; p++ )
                cps.push_back( (unsigned char) *p );
        }
        else
        {
            cps.push_back( genChar() );
        }
    }

    return cps;
}

static wxString toWx( const std::vector<uint32_t>& aCps )
{
    wxString s;

    for( uint32_t c : aCps )
        s += wxUniChar( c );

    return s;
}

// Kinds 'O' and 'E' (package CLEAN-1) rerun the glyph and draw cases with out-of-range sizes,
// coordinates, stroke widths and line spacings, so that the double -> int conversions of
// font.cpp / stroke_font.cpp see values outside the int range (x86: INT_MIN).
static bool g_extreme = false;

static int genExtremeInt()
{
    static const int TABLE[] = { INT_MAX,     INT_MIN,     INT_MAX - 1000, INT_MIN + 1000,
                                 2000000000,  -2000000000, 1900000000,     -1900000000,
                                 1500000000,  -1500000000, 1000000000 };
    int64_t r = rnd( 16 );

    if( r < 11 )
        return TABLE[r];

    return (int) ( rnd( 4294967296LL ) - 2147483648LL );
}

// A tab advances the cursor in steps of 4 * size.x until it passes the cursor: near INT_MAX
// that loop never ends (also in the C++), so the out-of-range kinds replace tabs by spaces.
static void noTabsIfExtreme( std::vector<uint32_t>& aCps )
{
    if( g_extreme )
        std::replace( aCps.begin(), aCps.end(), (uint32_t) '\t', (uint32_t) ' ' );
}

static int genSize()
{
    if( g_extreme && rnd( 2 ) == 0 )
        return genExtremeInt();

    int64_t r = rnd( 10 );

    if( r == 0 ) return 0;
    if( r == 1 ) return (int) rnd( 1000 );
    if( r == 2 ) return -(int) rnd( 2000000 );
    return (int) ( 500000 + rnd( 4500001 ) );
}

static int genCoord()
{
    if( g_extreme && rnd( 2 ) == 0 )
        return genExtremeInt();

    return (int) ( rnd( 200000001 ) - 100000000 );
}

static double genAngle()
{
    static const int TABLE[] = { 0, 0, 0, 900, 1800, 2700, 450, 300, 333, 3599, 1, 1350, 2250, 3150 };
    int64_t r = rnd( 20 );

    if( r < 14 )
        return TABLE[r] / 10.0;

    return rnd( 3600 ) / 10.0;
}

// ---- output helpers ----

static std::string g_out;

static void outS( const char* s ) { g_out += s; }

static void outI( int64_t v )
{
    char buf[32];
    snprintf( buf, sizeof( buf ), " %" PRId64, v );
    g_out += buf;
}

static void outD( double v )
{
    uint64_t bits;
    memcpy( &bits, &v, 8 );
    char buf[32];
    snprintf( buf, sizeof( buf ), " %016" PRIx64, bits );
    g_out += buf;
}

static void outF( float v )
{
    uint32_t bits;
    memcpy( &bits, &v, 4 );
    char buf[32];
    snprintf( buf, sizeof( buf ), " %08" PRIx32, bits );
    g_out += buf;
}

static void outHex( const std::string& s )
{
    g_out += " ";

    if( s.empty() )
        g_out += "-";

    for( unsigned char c : s )
    {
        char buf[4];
        snprintf( buf, sizeof( buf ), "%02x", c );
        g_out += buf;
    }
}

static void outWx( const wxString& s )
{
    outHex( std::string( s.utf8_str().data(), s.utf8_str().length() ) );
}

static void outBox( const BOX2I& b )
{
    outI( b.GetX() );
    outI( b.GetY() );
    outI( b.GetWidth() );
    outI( b.GetHeight() );
}

static void outGlyphs( const std::vector<std::unique_ptr<GLYPH>>& aGlyphs )
{
    for( const std::unique_ptr<GLYPH>& g : aGlyphs )
    {
        const STROKE_GLYPH* sg = static_cast<const STROKE_GLYPH*>( g.get() );
        BOX2D bb = const_cast<STROKE_GLYPH*>( sg )->BoundingBox();
        outS( "\ng" );
        outD( bb.GetX() );
        outD( bb.GetY() );
        outD( bb.GetWidth() );
        outD( bb.GetHeight() );
        outI( g->IsHover() );

        for( const std::vector<VECTOR2D>& pl : *sg )
        {
            outS( "\ns" );

            for( const VECTOR2D& p : pl )
            {
                outD( p.x );
                outD( p.y );
            }
        }
    }
}

static uint64_t fnv( const std::string& s )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : s )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}

static void dumpNode( const MARKUP::NODE* aNode )
{
    outS( "(" );
    outS( aNode->is_root() ? "root" : aNode->typeString().c_str() );

    if( aNode->has_content() )
        outHex( aNode->string() );

    for( const auto& c : aNode->children )
        dumpNode( c.get() );

    outS( ")" );
}

// ---- recording GAL ----

class RECORDING_GAL : public CALLBACK_GAL
{
public:
    RECORDING_GAL( KIGFX::GAL_DISPLAY_OPTIONS& aOpts ) :
            CALLBACK_GAL( aOpts,
                          [&]( const VECTOR2I& a, const VECTOR2I& b )
                          {
                              outS( "\nL" );
                              outI( a.x );
                              outI( a.y );
                              outI( b.x );
                              outI( b.y );
                          },
                          []( const VECTOR2I&, const VECTOR2I&, const VECTOR2I& ) {} )
    {
    }

    void DrawGlyph( const KIFONT::GLYPH& aGlyph, int aNth, int aTotal ) override
    {
        outS( "\nG" );
        outI( aNth );
        outI( aTotal );
        std::vector<std::unique_ptr<GLYPH>> one;
        one.push_back( std::make_unique<STROKE_GLYPH>( static_cast<const STROKE_GLYPH&>( aGlyph ) ) );
        outGlyphs( one );
        CALLBACK_GAL::DrawGlyph( aGlyph, aNth, aTotal );
    }
};

// ---- cases ----

static STROKE_FONT* g_font;

static void caseMarkup()
{
    std::vector<uint32_t> cps = genText( 30 );
    wxString              text = toWx( cps );
    std::string           utf8( text.utf8_str().data(), text.utf8_str().length() );

    outS( "M" );
    outHex( utf8 );
    outS( "\n" );

    MARKUP::MARKUP_PARSER         parser( utf8 );
    std::unique_ptr<MARKUP::NODE> root = parser.Parse();

    if( root )
        dumpNode( root.get() );
    else
        outS( "null" );
}

static void caseGlyphs()
{
    std::vector<uint32_t> cps = genText( 12 );
    noTabsIfExtreme( cps );
    wxString text = toWx( cps );
    int      sx = genSize();
    int      sy = rnd( 4 ) == 0 ? genSize() : sx;
    // (every random draw is its own statement: the evaluation order of function arguments
    // is unspecified)
    int      px = genCoord();
    int      py = genCoord();
    VECTOR2I pos( px, py );
    double   angle = genAngle();
    bool     mirror = rnd( 2 );
    VECTOR2I origin = pos;

    if( rnd( 2 ) == 0 )
    {
        int ox = genCoord();
        int oy = genCoord();
        origin = VECTOR2I( ox, oy );
    }
    unsigned style = (unsigned) rnd( 64 );

    outS( "T" );
    outWx( text );
    outI( sx );
    outI( sy );
    outI( pos.x );
    outI( pos.y );
    outD( angle );
    outI( mirror );
    outI( origin.x );
    outI( origin.y );
    outI( style );

    std::vector<std::unique_ptr<GLYPH>> glyphs;
    BOX2I                               bbox;
    VECTOR2I end = g_font->GetTextAsGlyphs( &bbox, &glyphs, text, VECTOR2I( sx, sy ), pos,
                                            EDA_ANGLE( angle, DEGREES_T ), mirror, origin, style );
    outS( "\nR" );
    outI( end.x );
    outI( end.y );
    outBox( bbox );
    outGlyphs( glyphs );
}

static void caseDraw()
{
    // multi-line text with markup
    std::vector<uint32_t> cps;
    int64_t               lines = 1 + rnd( 3 );

    for( int64_t l = 0; l < lines; l++ )
    {
        if( l )
            cps.push_back( '\n' );

        std::vector<uint32_t> part = genText( 10 );
        cps.insert( cps.end(), part.begin(), part.end() );
    }

    noTabsIfExtreme( cps );
    wxString        text = toWx( cps );
    TEXT_ATTRIBUTES attrs;
    int             sx = rnd( 8 ) == 0 ? genSize() : (int) ( 500000 + rnd( 4500001 ) );
    int             sy = rnd( 4 ) == 0 ? (int) ( 500000 + rnd( 4500001 ) ) : sx;
    static const double SPACING[] = { 1.0, 1.0, 1.5, 0.8 };
    int64_t         sw = rnd( 4 );

    attrs.m_Size = VECTOR2I( sx, sy );
    attrs.m_Angle = EDA_ANGLE( genAngle(), DEGREES_T );
    attrs.m_Halign = (GR_TEXT_H_ALIGN_T) ( rnd( 3 ) - 1 );
    attrs.m_Valign = (GR_TEXT_V_ALIGN_T) ( rnd( 3 ) - 1 );
    attrs.m_Mirrored = rnd( 2 );
    attrs.m_Italic = rnd( 2 );
    attrs.m_Bold = rnd( 2 );
    attrs.m_Underlined = rnd( 4 ) == 0;
    attrs.m_StrokeWidth = sw == 0 ? 0 : sw == 1 ? 1 : (int) ( 50000 + rnd( 550001 ) );
    attrs.m_LineSpacing = rnd( 3 ) ? SPACING[rnd( 4 )] : rnd( 300 ) / 100.0;

    if( g_extreme && rnd( 2 ) == 0 )
        attrs.m_StrokeWidth = genExtremeInt();

    if( g_extreme && rnd( 2 ) == 0 )
    {
        static const double LS[] = { 1e9, 3e9, -3e9, 1e300, -1e300, INFINITY, NAN, 20000.0 };
        attrs.m_LineSpacing = LS[rnd( 8 )];
    }

    int      px = genCoord();
    int      py = genCoord();
    VECTOR2I pos( px, py );
    VECTOR2I cursor( 0, 0 );

    if( rnd( 4 ) == 0 )
    {
        int cx = (int) rnd( 2000001 ) - 1000000;
        int cy = (int) rnd( 2000001 ) - 1000000;
        cursor = VECTOR2I( cx, cy );
    }
    int      column = (int) rnd( 50000001 );

    outS( "D" );
    outWx( text );
    outI( sx );
    outI( sy );
    outD( attrs.m_Angle.GetAngle().AsDegrees() );
    outI( attrs.m_Halign );
    outI( attrs.m_Valign );
    outI( attrs.m_Mirrored );
    outI( attrs.m_Italic );
    outI( attrs.m_Bold );
    outI( attrs.m_Underlined );
    outI( attrs.m_StrokeWidth );
    outD( attrs.m_LineSpacing );
    outI( pos.x );
    outI( pos.y );
    outI( cursor.x );
    outI( cursor.y );
    outI( column );

    // line positions as Draw() computes them
    wxArrayString         lineStrs;
    std::vector<VECTOR2I> positions;
    std::vector<VECTOR2I> extents;
    static_cast<FONT_PEEK*>( g_font )->getLinePositions( text, pos - cursor, lineStrs, positions, extents,
                                                         attrs, METRICS::Default() );

    for( size_t i = 0; i < lineStrs.size(); i++ )
    {
        outS( "\nP" );
        outWx( lineStrs[i] );
        outI( positions[i].x );
        outI( positions[i].y );
        outI( extents[i].x );
        outI( extents[i].y );
    }

    KIGFX::GAL_DISPLAY_OPTIONS opts;
    RECORDING_GAL              gal( opts );

    g_font->Draw( &gal, text, pos, cursor, attrs, METRICS::Default() );
    outS( "\nW" );
    outF( gal.GetLineWidth() );

    VECTOR2I limits = g_font->StringBoundaryLimits( text, attrs.m_Size, attrs.m_StrokeWidth, attrs.m_Bold,
                                                    attrs.m_Italic, METRICS::Default() );
    outS( "\nS" );
    outI( limits.x );
    outI( limits.y );

    VECTOR2I aligned = g_font->GetAlignedDrawPosition( text, pos, attrs, METRICS::Default() );
    outS( "\nA" );
    outI( aligned.x );
    outI( aligned.y );
    outD( g_font->GetInterline( sy, METRICS::Default() ) );

    if( !text.Contains( wxS( "@{" ) ) )
    {
        outS( "\nX" );
        // GRTextWidth() without the text_eval part (texts without "@{")
        outI( KiROUND( g_font->StringBoundaryLimits( text, attrs.m_Size, attrs.m_StrokeWidth, attrs.m_Bold,
                                                     attrs.m_Italic, METRICS::Default() ).x ) );
    }

    wxString broken = text;
    g_font->LinebreakText( broken, column, attrs.m_Size, attrs.m_StrokeWidth, attrs.m_Bold, attrs.m_Italic );
    outS( "\nK" );
    outWx( broken );
}

static void caseHelpers()
{
    int      a = (int) ( rnd( 20000001 ) - 1000000 );
    int      b = (int) ( rnd( 20000001 ) - 1000000 );
    int      c = (int) ( rnd( 20000001 ) - 1000000 );
    bool     strict = rnd( 2 );
    float    f = (float) ( rnd( 20000001 ) - 1000000 ) / 7.0f;

    outS( "H" );
    outI( a );
    outI( b );
    outI( c );
    outI( strict );
    outF( f );
    outS( "\n" );
    outI( GetPenSizeForBold( a ) );
    outI( GetPenSizeForBold( wxSize( a, b ) ) );
    outI( GetPenSizeForDemiBold( a ) );
    outI( GetPenSizeForDemiBold( wxSize( a, b ) ) );
    outI( GetPenSizeForNormal( a ) );
    outI( GetPenSizeForNormal( wxSize( a, b ) ) );
    outI( ClampTextPenSize( a, b, strict ) );
    outF( ClampTextPenSize( f, b, strict ) );
    outI( ClampTextPenSize( a, VECTOR2I( b, c ), strict ) );
    outI( GetKnockoutTextMargin( VECTOR2I( b, c ), a ) );
}

struct KIND
{
    char        tag;
    int         count;
    void ( *fn )();
};

static void caseGlyphsExtreme()
{
    g_extreme = true;
    caseGlyphs();
    g_extreme = false;
}

static void caseDrawExtreme()
{
    g_extreme = true;
    caseDraw();
    g_extreme = false;
}

static const KIND KINDS[] = {
    { 'M', 3000, caseMarkup },
    { 'T', 2000, caseGlyphs },
    { 'D', 3000, caseDraw },
    { 'H', 500, caseHelpers },
    { 'O', 600, caseGlyphsExtreme },
    { 'E', 600, caseDrawExtreme },
};

static void dumpTable( bool aFull )
{
    // the raw strings
    std::string all;

    for( int i = 0; i < newstroke_font_bufsize; i++ )
    {
        all += newstroke_font[i];
        all += "\n";
    }

    if( aFull )
    {
        fputs( all.c_str(), stdout );
        return;
    }

    printf( "COUNT %d %u\n", newstroke_font_bufsize, g_font->GetGlyphCount() );
    printf( "STRINGS %016" PRIx64 "\n", fnv( all ) );

    // decoded glyphs, hashed per block of 1024
    for( unsigned block = 0; block * 1024 < g_font->GetGlyphCount(); block++ )
    {
        g_out.clear();

        for( unsigned i = block * 1024; i < std::min( g_font->GetGlyphCount(), ( block + 1 ) * 1024 ); i++ )
        {
            const BOX2D& bb = g_font->GetGlyphBoundingBox( i );
            outS( "b" );
            outD( bb.GetX() );
            outD( bb.GetY() );
            outD( bb.GetWidth() );
            outD( bb.GetHeight() );

            for( const std::vector<VECTOR2D>& pl : *g_font->GetGlyph( i ) )
            {
                outS( "\ns" );

                for( const VECTOR2D& p : pl )
                {
                    outD( p.x );
                    outD( p.y );
                }
            }

            outS( "\n" );
        }

        printf( "BLOCK %u %016" PRIx64 "\n", block, fnv( g_out ) );
    }
}

int main( int argc, char** argv )
{
    std::string mode = argc > 1 ? argv[1] : "golden";

    // KiROUND's overflow report (kimathLogOverflow -> wxFAIL_MSG) only logs; the default
    // handler is very slow, and the out-of-range kinds 'O' / 'E' trigger it a lot.
    wxSetAssertHandler( nullptr );

    g_font = STROKE_FONT::LoadFont( wxEmptyString );

    if( mode == "strings" )
    {
        dumpTable( true );
        return 0;
    }

    if( mode == "golden" )
        dumpTable( false );

    for( const KIND& kind : KINDS )
    {
        for( int idx = 0; idx < kind.count; idx++ )
        {
            if( argc > 3 && ( argv[2][0] != kind.tag || atoi( argv[3] ) != idx ) )
                continue;

            g_state = (uint64_t) idx * 1000003ULL + (uint64_t) kind.tag;
            g_out.clear();
            kind.fn();

            if( mode == "full" )
                printf( "%c %d\n%s\n", kind.tag, idx, g_out.c_str() );
            else
                printf( "%c %d %016" PRIx64 "\n", kind.tag, idx, fnv( g_out ) );
        }
    }

    return 0;
}
