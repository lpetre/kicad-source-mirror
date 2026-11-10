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
 * Differential oracle of work package M24 (reference images, embedded files). Built by
 * m24_build_oracle.sh against the reference build (common/reference_image.cpp and
 * common/bitmap_base.cpp compiled unchanged, EMBEDDED_FILES from libkicommon.so, wxWidgets
 * image handlers). Prints one line per value, doubles as IEEE bit patterns. The Rust replay
 * is the `m24_oracle_*` tests in src/pcb_reference_image.rs and src/embedded_files.rs.
 *
 *   m24_oracle images <kicad root> <board>...   reference image probe + maths
 *   m24_oracle embed <kicad root> <file>...     (embedded_files ...) sections of files
 *   m24_oracle decode                           stdin "name base64 hash" -> DecompressAndDecode
 */

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/base64.h>
#include <wx/image.h>
#include <wx/init.h>
#include <wx/log.h>
#include <wx/mstream.h>

#include <zlib.h>

#include <base_units.h>
#include <bitmap_base.h>
#include <embedded_files.h>
#include <embedded_files_lexer.h>
#include <embedded_files_parser.h>
#include <geometry/seg.h>
#include <reference_image.h>
#include <richio.h>

using BYTES = std::vector<unsigned char>;

// Link stubs for BITMAP_BASE members the oracle never calls (DrawBitmap, ConvertColourToAlpha);
// the real ones live in objects of libcommon.a that pull in the whole GUI.
bool GetGRForceBlackPenState()
{
    return false;
}

void ConvertColourToAlphaInPlace( wxImage&, const wxColour& )
{
}

static std::string bits( double v )
{
    uint64_t u;
    std::memcpy( &u, &v, 8 );
    char buf[32];
    snprintf( buf, sizeof buf, "%016llx", (unsigned long long) u );
    return buf;
}

static void put32( BYTES& b, uint32_t v )
{
    b.push_back( v >> 24 ); b.push_back( v >> 16 ); b.push_back( v >> 8 ); b.push_back( v );
}

static BYTES chunk( const char* type, const BYTES& body, bool goodCrc = true )
{
    BYTES c;
    put32( c, body.size() );
    BYTES td( type, type + 4 );
    td.insert( td.end(), body.begin(), body.end() );
    uint32_t crc = crc32( 0, td.data(), td.size() );
    c.insert( c.end(), td.begin(), td.end() );
    put32( c, goodCrc ? crc : crc ^ 1 );
    return c;
}

struct PNG_CHUNK { std::string type; BYTES body; };

static std::vector<PNG_CHUNK> splitPng( const BYTES& png )
{
    std::vector<PNG_CHUNK> out;
    size_t pos = 8;

    while( pos + 8 <= png.size() )
    {
        uint32_t len = ( png[pos] << 24 ) | ( png[pos + 1] << 16 ) | ( png[pos + 2] << 8 ) | png[pos + 3];
        PNG_CHUNK c{ std::string( (const char*) &png[pos + 4], 4 ),
                     BYTES( png.begin() + pos + 8, png.begin() + pos + 8 + len ) };
        out.push_back( c );
        pos += 12 + len;
    }

    return out;
}

static BYTES phys( uint32_t x, uint32_t y, unsigned char unit )
{
    BYTES b;
    put32( b, x );
    put32( b, y );
    b.push_back( unit );
    return b;
}

// Rebuild a PNG from the chunks of `base` (pHYs dropped) with `before` inserted after IHDR and
// `after` after the first IDAT.
static BYTES rebuildPng( const BYTES& base, const std::vector<BYTES>& before,
                         const std::vector<BYTES>& after, const BYTES* ihdrOverride = nullptr )
{
    BYTES out( base.begin(), base.begin() + 8 );

    for( const PNG_CHUNK& c : splitPng( base ) )
    {
        if( c.type == "pHYs" )
            continue;

        if( c.type == "IHDR" && ihdrOverride )
            for( unsigned char v : *ihdrOverride ) out.push_back( v );
        else
            for( unsigned char v : chunk( c.type.c_str(), c.body ) ) out.push_back( v );

        if( c.type == "IHDR" )
            for( const BYTES& b : before ) out.insert( out.end(), b.begin(), b.end() );

        if( c.type == "IDAT" )
            for( const BYTES& b : after ) out.insert( out.end(), b.begin(), b.end() );
    }

    return out;
}

static BYTES saveImage( wxImage& img, wxBitmapType type )
{
    wxMemoryOutputStream os;
    img.SaveFile( os, type );
    BYTES b( os.GetSize() );
    os.CopyTo( b.data(), b.size() );
    return b;
}

static std::string b64( const BYTES& b )
{
    return std::string( wxBase64Encode( b.data(), b.size() ).ToStdString() );
}

static void pv( const std::string& l, const char* k, const VECTOR2I& v )
{
    printf( "img|%s|%s|%d,%d\n", l.c_str(), k, v.x, v.y );
}

static void state( const std::string& l, const char* k, const REFERENCE_IMAGE& r )
{
    BOX2I bb = r.GetBoundingBox();
    printf( "img|%s|%s|pos=%d,%d off=%d,%d scale=%s orient=%s size=%d,%d bbox=%d,%d,%lld,%lld\n",
            l.c_str(), k, r.GetPosition().x, r.GetPosition().y, r.GetTransformOriginOffset().x,
            r.GetTransformOriginOffset().y, bits( r.GetImageScale() ).c_str(),
            bits( r.GetOrientation().AsDegrees() ).c_str(), r.GetSize().x, r.GetSize().y,
            bb.GetX(), bb.GetY(), (long long) bb.GetWidth(), (long long) bb.GetHeight() );
}

static bool load( REFERENCE_IMAGE& r, const BYTES& data )
{
    wxMemoryBuffer buf;
    buf.AppendData( data.data(), data.size() );
    return r.ReadImageFile( buf );
}

static void runImage( const std::string& l, const BYTES& data )
{
    {
        REFERENCE_IMAGE r( pcbIUScale );
        bool ok = load( r, data );
        const BITMAP_BASE& bm = r.GetImage();
        printf( "img|%s|read|ok=%d px=%d,%d ppi=%d legacy=%d pxiu=%s\n", l.c_str(), ok ? 1 : 0,
                bm.GetSizePixels().x, bm.GetSizePixels().y, bm.GetPPI(), bm.GetLegacyPPI(),
                bits( bm.GetPixelSizeIu() ).c_str() );
        state( l, "loaded", r );

        if( !ok )
            return;
    }

    // S1: scales about a transform origin
    {
        REFERENCE_IMAGE r( pcbIUScale );
        load( r, data );
        r.SetPosition( VECTOR2I( 1000, 2000 ) );
        r.SetTransformOriginOffset( VECTOR2I( 300, -100 ) );
        const double scales[] = { 0.159863, 2.0, 0.5, 1e-9, 1e9, -1.0, 0.0, 3.25, 1e4 };
        int i = 0;

        for( double s : scales )
        {
            r.SetImageScale( s );
            state( l, ( "scale" + std::to_string( i++ ) ).c_str(), r );
        }

        r.SetWidth( 123456 );
        state( l, "width", r );
        r.SetHeight( 654321 );
        state( l, "height", r );
        r.SetWidth( 0 );
        state( l, "width0", r );
    }

    // S2: rotations
    {
        REFERENCE_IMAGE r( pcbIUScale );
        load( r, data );
        r.SetImageScale( 2.5 );
        r.SetPosition( VECTOR2I( -3000, 1500 ) );
        r.SetTransformOriginOffset( VECTOR2I( 250, 75 ) );
        const double angles[] = { 30.0, 45.0, 90.0, 137.5, -10.0, 270.0, 0.1 };
        int i = 0;

        for( double a : angles )
        {
            r.Rotate( VECTOR2I( 500, 700 ), EDA_ANGLE( a, DEGREES_T ) );
            state( l, ( "rot" + std::to_string( i++ ) ).c_str(), r );
        }
    }

    // S3: flips and mirrors
    {
        REFERENCE_IMAGE r( pcbIUScale );
        load( r, data );
        r.SetPosition( VECTOR2I( 1234, -5678 ) );
        r.SetTransformOriginOffset( VECTOR2I( 11, 22 ) );
        r.SetOrientation( EDA_ANGLE( 30.0, DEGREES_T ) );
        printf( "img|%s|flip0|%d\n", l.c_str(), r.Flip( VECTOR2I( 10, 20 ), FLIP_DIRECTION::LEFT_RIGHT ) );
        state( l, "flip0s", r );
        printf( "img|%s|flip1|%d\n", l.c_str(), r.Flip( VECTOR2I( 10, 20 ), FLIP_DIRECTION::TOP_BOTTOM ) );
        state( l, "flip1s", r );
        printf( "img|%s|mirror0|%d\n", l.c_str(), r.Mirror( SEG( VECTOR2I( 0, 0 ), VECTOR2I( 100, 37 ) ) ) );
        state( l, "mirror0s", r );
        printf( "img|%s|mirror1|%d\n", l.c_str(), r.Mirror( SEG( VECTOR2I( 5, 5 ), VECTOR2I( 5, 105 ) ) ) );
        state( l, "mirror1s", r );
    }

    // S4: overflow checks
    {
        REFERENCE_IMAGE r( pcbIUScale );
        load( r, data );
        r.SetImageScale( 40.0 );
        r.SetPosition( VECTOR2I( 2147483000, 0 ) );
        state( l, "edge0", r );
        r.SetPosition( VECTOR2I( 2140000000, -2140000000 ) );
        state( l, "edge1", r );
        printf( "img|%s|edgeflip|%d\n", l.c_str(), r.Flip( VECTOR2I( -2140000000, 0 ), FLIP_DIRECTION::LEFT_RIGHT ) );
        state( l, "edge2", r );
        r.SetImageScale( 1e6 );
        state( l, "edge3", r );
    }
}

static std::string readFile( const std::string& path )
{
    std::ifstream in( path, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

// Crude scan shared with the Rust replay: every "(image" followed by white space, then the
// first "(data" after it up to the next ')', white space and quotes removed.
static std::vector<std::string> corpusImages( const std::string& text )
{
    std::vector<std::string> out;
    size_t pos = 0;

    while( ( pos = text.find( "(image", pos ) ) != std::string::npos )
    {
        pos += 6;

        if( pos >= text.size() || !isspace( (unsigned char) text[pos] ) )
            continue;

        size_t d = text.find( "(data", pos );
        size_t e = text.find( ')', d );
        std::string b;

        for( size_t i = d + 5; i < e; ++i )
            if( !isspace( (unsigned char) text[i] ) && text[i] != '"' )
                b += text[i];

        out.push_back( b );
    }

    return out;
}

static void images( const std::string& root, int argc, char** argv )
{
    // Synthetic PNGs: a 3x2 RGB image without pHYs, then pHYs variants.
    wxImage png( 3, 2 );
    png.SetRGB( wxRect( 0, 0, 3, 2 ), 10, 20, 30 );
    BYTES base = saveImage( png, wxBITMAP_TYPE_PNG );
    BYTES none = rebuildPng( base, {}, {} );

    std::vector<std::pair<std::string, BYTES>> payloads;
    payloads.push_back( { "png_nophys", none } );

    struct PH { const char* n; uint32_t x; unsigned char u; };
    const PH ph[] = { { "png_3780m", 3780, 1 },   { "png_3779m", 3779, 1 },
                      { "png_11811m", 11811, 1 }, { "png_2834m", 2834, 1 },
                      { "png_100m", 100, 1 },     { "png_101m", 101, 1 },
                      { "png_150m", 150, 1 },     { "png_72u", 72, 0 },
                      { "png_1u", 1, 0 },         { "png_2u", 2, 0 },
                      { "png_neg_u", 0xFFFFFFF0u, 0 }, { "png_huge_m", 0xFFFFFFF0u, 1 },
                      { "png_3780_unit2", 3780, 2 } };

    for( const PH& p : ph )
        payloads.push_back( { p.n, rebuildPng( base, { chunk( "pHYs", phys( p.x, p.x * 2, p.u ) ) }, {} ) } );

    payloads.push_back( { "png_badcrc", rebuildPng( base, { chunk( "pHYs", phys( 3780, 3780, 1 ), false ) }, {} ) } );
    payloads.push_back( { "png_late", rebuildPng( base, {}, { chunk( "pHYs", phys( 3780, 3780, 1 ) ) } ) } );
    payloads.push_back( { "png_dup", rebuildPng( base, { chunk( "pHYs", phys( 3780, 3780, 1 ) ),
                                                         chunk( "pHYs", phys( 11811, 11811, 1 ) ) }, {} ) } );
    BYTES short8 = phys( 3780, 3780, 1 );
    short8.pop_back();
    payloads.push_back( { "png_len8", rebuildPng( base, { chunk( "pHYs", short8 ) }, {} ) } );
    payloads.push_back( { "png_ancillary", rebuildPng( base, { chunk( "tEXt", BYTES{ 'a', 0, 'b' } ),
                                                              chunk( "pHYs", phys( 5000, 5000, 1 ) ) }, {} ) } );
    payloads.push_back( { "png_critical", rebuildPng( base, { chunk( "ABCD", BYTES{ 1, 2 } ) }, {} ) } );

    {
        std::vector<PNG_CHUNK> cs = splitPng( base );
        BYTES ihdr = cs[0].body;
        BYTES zero = ihdr;
        zero[0] = zero[1] = zero[2] = zero[3] = 0;
        BYTES z = chunk( "IHDR", zero );
        payloads.push_back( { "png_width0", rebuildPng( base, {}, {}, &z ) } );
        BYTES bad = chunk( "IHDR", ihdr, false );
        payloads.push_back( { "png_ihdrcrc", rebuildPng( base, {}, {}, &bad ) } );
        BYTES trunc( none.begin(), none.begin() + 8 + 25 );
        payloads.push_back( { "png_trunc", trunc } );
    }

    // Synthetic JPEGs: 4x3, JFIF density from wx, then patched density fields.
    wxImage jpg( 4, 3 );
    jpg.SetRGB( wxRect( 0, 0, 4, 3 ), 200, 100, 50 );
    jpg.SetOption( wxIMAGE_OPTION_RESOLUTIONUNIT, wxIMAGE_RESOLUTION_INCHES );
    jpg.SetOption( wxIMAGE_OPTION_RESOLUTIONX, 150 );
    jpg.SetOption( wxIMAGE_OPTION_RESOLUTIONY, 150 );
    BYTES jbase = saveImage( jpg, wxBITMAP_TYPE_JPEG );
    payloads.push_back( { "jpg_150in", jbase } );

    // APP0 at offset 2: FF E0 len(2) "JFIF\0" ver(2) units(1) Xd(2) Yd(2)
    auto patch = []( BYTES b, unsigned char unit, uint16_t d )
    {
        if( b.size() > 20 && b[2] == 0xFF && b[3] == 0xE0 )
        {
            b[13] = unit;
            b[14] = d >> 8; b[15] = d & 0xFF;
            b[16] = d >> 8; b[17] = d & 0xFF;
        }
        return b;
    };

    payloads.push_back( { "jpg_28cm", patch( jbase, 2, 28 ) } );
    payloads.push_back( { "jpg_72none", patch( jbase, 0, 72 ) } );
    payloads.push_back( { "jpg_1none", patch( jbase, 0, 1 ) } );
    payloads.push_back( { "jpg_1in", patch( jbase, 1, 1 ) } );
    payloads.push_back( { "jpg_unit3", patch( jbase, 3, 200 ) } );

    if( jbase.size() > 20 && jbase[2] == 0xFF && jbase[3] == 0xE0 )
    {
        size_t len = ( jbase[4] << 8 ) | jbase[5];
        BYTES nojfif( jbase.begin(), jbase.begin() + 2 );
        nojfif.insert( nojfif.end(), jbase.begin() + 4 + len, jbase.end() );
        payloads.push_back( { "jpg_nojfif", nojfif } );
    }

    for( const auto& [label, data] : payloads )
        printf( "payload|%s|%s\n", label.c_str(), b64( data ).c_str() );

    for( const auto& [label, data] : payloads )
        runImage( label, data );

    // Corpus boards
    for( int i = 0; i < argc; ++i )
    {
        std::string text = readFile( root + "/" + argv[i] );
        int n = 0;

        for( const std::string& data64 : corpusImages( text ) )
        {
            wxMemoryBuffer buf = wxBase64Decode( data64 );
            BYTES data( (unsigned char*) buf.GetData(), (unsigned char*) buf.GetData() + buf.GetDataLen() );
            runImage( std::string( argv[i] ) + "#" + std::to_string( n++ ), data );
        }
    }
}

static void embed( const std::string& root, int argc, char** argv )
{
    for( int i = 0; i < argc; ++i )
    {
        std::string text = readFile( root + "/" + argv[i] );
        STRING_LINE_READER reader( text, wxString::FromUTF8( argv[i] ) );
        EMBEDDED_FILES_PARSER parser( &reader );
        int section = 0;

        try
        {
            for( int tok = parser.NextTok(); tok != DSN_EOF; tok = parser.NextTok() )
            {
                if( tok != EMBEDDED_FILES_T::T_embedded_files || parser.PrevTok() != DSN_LEFT )
                    continue;

                EMBEDDED_FILES files;

                try
                {
                    parser.ParseEmbedded( &files );
                }
                catch( const PARSE_ERROR& )
                {
                    printf( "embed|%s|%d|error\n", argv[i], section );
                }

                for( const auto& [name, file] : files.EmbeddedFileMap() )
                {
                    printf( "embed|%s|%d|%s|type=%d|size=%zu|hash=%s|valid=%d|data=%zu\n", argv[i],
                            section, name.ToStdString().c_str(), (int) file->type,
                            file->decompressedData.size(), file->data_hash.c_str(),
                            file->Validate() ? 1 : 0, file->compressedEncodedData.size() );
                }

                ++section;
            }
        }
        catch( const IO_ERROR& )
        {
            printf( "embed|%s|lexer-error\n", argv[i] );
        }
    }
}

static void decode()
{
    std::string name, data, hash;

    while( std::cin >> name >> data >> hash )
    {
        EMBEDDED_FILES::EMBEDDED_FILE f;
        f.name = name;
        f.compressedEncodedData = data;
        f.data_hash = hash;
        int rc = (int) EMBEDDED_FILES::DecompressAndDecode( f );
        printf( "decode|%s|rc=%d|size=%zu|hash=%s\n", name.c_str(), rc, f.decompressedData.size(),
                f.data_hash.c_str() );
    }
}

int main( int argc, char** argv )
{
    wxInitializer init( argc, argv );
    wxLog::EnableLogging( false );
    wxInitAllImageHandlers();

    if( argc < 2 )
        return 1;

    std::string mode = argv[1];

    if( mode == "images" && argc >= 3 )
        images( argv[2], argc - 3, argv + 3 );
    else if( mode == "embed" && argc >= 3 )
        embed( argv[2], argc - 3, argv + 3 );
    else if( mode == "decode" )
        decode();
    else
        return 1;

    return 0;
}
