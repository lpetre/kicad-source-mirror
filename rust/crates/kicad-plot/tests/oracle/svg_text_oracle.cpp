// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// Differential oracle of work packages PLOT-05 (PLOTTER::Text / PlotText, GERBER_PLOTTER text
// overrides) and PLOT-06 (SVG_PLOTTER with the inherited PSLIKE_PLOTTER parts) of the
// kicad-plot crate.  It reads plot scripts (written by tests/test_svg_text_oracle.rs with
// KICAD_PLOT_WRITE_SCRIPTS=1), runs each of them on the real SVG_PLOTTER or GERBER_PLOTTER of
// the reference build (common/libcommon.a, the stroke font of libkigal) and prints, per script,
// the FNV-1a 64 hash of the produced file.
//
//   svg_text_oracle SCRIPTS [full]
//
// Golden output (tests/golden/svg_text.txt):
//   SCRIPTS <count> <fnv64 of the script file>
//   CH <key> <n> <x y>...      one line per distinct TransformRoundChamferedRectToPolygon call
//   F <idx> ... F-END          the complete file of the first FULL_COUNT scripts
//   R <idx> <low 32 bits of the fnv64 of the file>
//
// Normalised before hashing: the SVG "<title>" line (file name and wall clock) becomes
// "<title>SVG Image created as plot.svg date @DATE@ </title>", the Gerber "G04 Created by"
// line becomes "G04 Created by KiCad (<creator> @VERSION@) date @DATE@*".

#include <cinttypes>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include <unistd.h>

#include <convert_basic_shapes_to_polygon.h>
#include <font/font.h>
#include <font/text_attributes.h>
#include <gbr_metadata.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <page_info.h>
#include <plotters/plotter_gerber.h>
#include <plotters/plotters_pslike.h>
#include <render_settings.h>

static const int FULL_COUNT = 4;

class TEST_RENDER_SETTINGS : public KIGFX::RENDER_SETTINGS
{
public:
    COLOR4D GetColor( const KIGFX::VIEW_ITEM*, int ) const override { return m_c; }
    const COLOR4D& GetBackgroundColor() const override { return m_c; }
    void SetBackgroundColor( const COLOR4D& ) override {}
    const COLOR4D& GetGridColor() override { return m_c; }
    const COLOR4D& GetCursorColor() override { return m_c; }
    COLOR4D m_c;
};

class SVG : public SVG_PLOTTER
{
public:
    int HighDef() const { return GetPlotterArcHighDef(); }
};

static uint64_t fnv( const std::string& s, uint64_t h = 1469598103934665603ULL )
{
    for( unsigned char c : s )
    {
        h ^= c;
        h *= 1099511628211ULL;
    }

    return h;
}

static std::string hexbits( double d )
{
    uint64_t u;
    memcpy( &u, &d, 8 );
    char buf[32];
    snprintf( buf, sizeof( buf ), "%016" PRIx64, u );
    return buf;
}

struct TOK
{
    std::vector<std::string> t;
    size_t                   i = 1;

    const std::string& s() { return t.at( i++ ); }
    int                n() { return (int) strtol( s().c_str(), nullptr, 10 ); }
    double             d() { return strtod( s().c_str(), nullptr ); }
    std::string        str()
    {
        std::string v = s();
        return v == "~" ? std::string() : v;
    }
    // hex encoded UTF-8 ("~" = empty)
    wxString hex()
    {
        std::string v = s();
        std::string r;

        if( v != "~" )
        {
            for( size_t k = 0; k + 1 < v.size(); k += 2 )
                r.push_back( (char) strtol( v.substr( k, 2 ).c_str(), nullptr, 16 ) );
        }

        return wxString::FromUTF8( r );
    }
    VECTOR2I  pt() { int x = n(); int y = n(); return VECTOR2I( x, y ); }
    VECTOR2D  ptd() { double x = d(); double y = d(); return VECTOR2D( x, y ); }
    EDA_ANGLE ang() { return EDA_ANGLE( d(), DEGREES_T ); }
    FILL_T    fill() { return s() == "F" ? FILL_T::FILLED_SHAPE : FILL_T::NO_FILL; }
    COLOR4D   color() { double r = d(); double g = d(); double b = d(); double a = d(); return COLOR4D( r, g, b, a ); }

    std::vector<VECTOR2I> pts()
    {
        int                   cnt = n();
        std::vector<VECTOR2I> v;

        for( int k = 0; k < cnt; k++ )
            v.push_back( pt() );

        return v;
    }

    SHAPE_LINE_CHAIN chain()
    {
        bool             closed = n() != 0;
        int              cnt = n();
        SHAPE_LINE_CHAIN c;

        for( int k = 0; k < cnt; k++ )
        {
            std::string kind = s();

            if( kind == "P" )
            {
                VECTOR2I p = pt();
                c.Append( p.x, p.y, true );
            }
            else
            {
                VECTOR2I a = pt();
                VECTOR2I b = pt();
                VECTOR2I e = pt();
                c.Append( SHAPE_ARC( a, b, e, 0 ) );
            }
        }

        c.SetClosed( closed );
        return c;
    }

    SHAPE_POLY_SET polyset()
    {
        int            cnt = n();
        SHAPE_POLY_SET ps;

        for( int k = 0; k < cnt; k++ )
            ps.AddOutline( SHAPE_LINE_CHAIN( pts(), true ) );

        return ps;
    }
};

static const char* g_extra[] = { "", "%TO.CFtp,1*%\n", "G04 extra*\n" };

static std::map<std::string, std::string> g_chamfer; // key -> "n x y ..."

static void recordChamfer( const VECTOR2I& aPos, const VECTOR2I& aSize, const EDA_ANGLE& aRot,
                           int aRadius, double aRatio, int aPositions, int aError )
{
    std::ostringstream key;
    key << aPos.x << ',' << aPos.y << ',' << aSize.x << ',' << aSize.y << ','
        << hexbits( aRot.AsDegrees() ) << ',' << aRadius << ',' << hexbits( aRatio ) << ','
        << aPositions << ',' << aError;

    if( g_chamfer.count( key.str() ) )
        return;

    SHAPE_POLY_SET outline;
    TransformRoundChamferedRectToPolygon( outline, aPos, aSize, aRot, aRadius, aRatio, aPositions,
                                          0, aError, ERROR_INSIDE );
    std::ostringstream val;

    if( outline.OutlineCount() == 0 )
    {
        val << 0;
    }
    else
    {
        const SHAPE_LINE_CHAIN& c = outline.Outline( 0 );
        val << c.PointCount();

        for( int k = 0; k < c.PointCount(); k++ )
            val << ' ' << c.CPoint( k ).x << ' ' << c.CPoint( k ).y;
    }

    g_chamfer[key.str()] = val.str();
}

static std::string runScript( const std::vector<std::vector<std::string>>& aLines,
                              const std::string& aPathBase )
{
    bool                        isSvg = aLines.at( 0 ).at( 1 ) == "SVG";
    std::string                 path = aPathBase + ( isSvg ? ".svg" : ".gbr" );
    std::unique_ptr<SVG>        svg;
    std::unique_ptr<GERBER_PLOTTER> gbr;
    PLOTTER*                    base;
    TEST_RENDER_SETTINGS        rs;
    std::map<int, GBR_METADATA> mds;
    std::string                 creator;
    bool                        started = false;

    if( isSvg )
    {
        svg = std::make_unique<SVG>();
        base = svg.get();
    }
    else
    {
        gbr = std::make_unique<GERBER_PLOTTER>();
        base = gbr.get();
    }

    base->SetRenderSettings( &rs );

    auto md = [&]( TOK& t ) -> GBR_METADATA*
    {
        int id = t.n();

        if( id < 0 )
            return nullptr;

        return &mds[id];
    };

    for( size_t li = 1; li < aLines.size(); li++ )
    {
        TOK t;
        t.t = aLines[li];
        const std::string& op = t.t[0];

        if( op == "PAGE" )
        {
            PAGE_INFO pi;
            double    w = t.d();
            double    h = t.d();

            if( w > 0 )
            {
                pi.SetWidthMils( w );
                pi.SetHeightMils( h );
            }

            base->SetPageSettings( pi );
        }
        else if( op == "VIEW" )
        {
            VECTOR2I off = t.pt();
            double   ipd = t.d();
            double   scale = t.d();
            bool     mirror = t.n();
            base->SetViewport( off, ipd, scale, mirror );
        }
        else if( op == "PREC" )
        {
            base->SetSvgCoordinatesFormat( t.n() );
        }
        else if( op == "BBOX" )
        {
            VECTOR2I p = t.pt();
            VECTOR2I s = t.pt();
            base->SetPlotBBox( BOX2I( p, s ) );
        }
        else if( op == "CMODE" )
        {
            base->SetColorMode( t.n() );
        }
        else if( op == "NEG" )
        {
            base->SetNegative( t.n() );
        }
        else if( op == "OPEN" )
        {
            base->OpenFile( wxString::FromUTF8( path ) );
        }
        else if( op == "CFG" )
        {
            bool x2 = t.n(), net = t.n(), nomacro = t.n();
            int  fmt = t.n();
            bool inch = t.n();
            VECTOR2I off = t.pt();
            double   ipd = t.d();
            gbr->SetViewport( off, ipd, 1.0, false );
            gbr->SetGerberCoordinatesFormat( fmt, inch );
            gbr->UseX2format( x2 );
            gbr->UseX2NetAttributes( net );
            gbr->DisableApertMacros( nomacro );
        }
        else if( op == "CREATOR" )
        {
            creator = t.str();
            base->SetCreator( wxString::FromUTF8( creator ) );
        }
        else if( op == "RS" )
        {
            rs.SetDefaultPenWidth( t.n() );
            rs.SetDashLengthRatio( t.d() );
            rs.SetGapLengthRatio( t.d() );
        }
        else if( op == "MD" )
        {
            int           id = t.n();
            GBR_METADATA& m = mds[id];
            m = GBR_METADATA();
            m.m_ApertureMetadata.m_ApertAttribute =
                    (GBR_APERTURE_METADATA::GBR_APERTURE_ATTRIB) t.n();
            m.m_NetlistMetadata.m_NetAttribType = t.n();
            m.m_NetlistMetadata.m_NotInNet = t.n();
            m.SetCmpReference( wxString::FromUTF8( t.str() ) );
            m.SetNetName( wxString::FromUTF8( t.str() ) );
            m.m_NetlistMetadata.m_ExtraData = wxString::FromUTF8( g_extra[t.n()] );
        }
        else if( op == "START" )
        {
            base->StartPlot( wxT( "1" ) );
            started = true;
        }
        else if( op == "COLOR" )
        {
            base->SetColor( t.color() );
        }
        else if( op == "DASH" )
        {
            int w = t.n();
            base->SetDash( w, (LINE_STYLE) t.n() );
        }
        else if( op == "W" )
        {
            int w = t.n();
            base->SetCurrentLineWidth( w, md( t ) );
        }
        else if( op == "PEN" )
        {
            char c = t.s()[0];
            VECTOR2I p = t.pt();
            base->PenTo( p, c );
        }
        else if( op == "MV" )
        {
            base->MoveTo( t.pt() );
        }
        else if( op == "LN" )
        {
            base->LineTo( t.pt() );
        }
        else if( op == "FIN" )
        {
            base->FinishTo( t.pt() );
        }
        else if( op == "SEG" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            int      w = t.n();
            base->ThickSegment( a, b, w, md( t ) );
        }
        else if( op == "ARC" )
        {
            VECTOR2D  c = t.ptd();
            EDA_ANGLE sa = t.ang();
            EDA_ANGLE a = t.ang();
            double    r = t.d();
            FILL_T    f = t.fill();
            int       w = t.n();
            base->Arc( c, sa, a, r, f, w );
        }
        else if( op == "ARC3" )
        {
            VECTOR2D s = t.ptd();
            VECTOR2D m = t.ptd();
            VECTOR2D e = t.ptd();
            FILL_T   f = t.fill();
            int      w = t.n();
            base->Arc( s, m, e, f, w );
        }
        else if( op == "TARC" )
        {
            VECTOR2D  c = t.ptd();
            EDA_ANGLE sa = t.ang();
            EDA_ANGLE a = t.ang();
            double    r = t.d();
            int       w = t.n();
            base->ThickArc( c, sa, a, r, w, md( t ) );
        }
        else if( op == "RECT" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            FILL_T   f = t.fill();
            int      w = t.n();
            base->Rect( a, b, f, w, 0 );
        }
        else if( op == "RRECT" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            FILL_T   f = t.fill();
            int      w = t.n();
            int      r = t.n();
            base->Rect( a, b, f, w, r );
        }
        else if( op == "TRECT" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            int      w = t.n();
            base->ThickRect( a, b, w, md( t ) );
        }
        else if( op == "CIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            FILL_T   f = t.fill();
            int      w = t.n();
            base->Circle( c, d, f, w );
        }
        else if( op == "TCIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            int      w = t.n();
            base->ThickCircle( c, d, w, md( t ) );
        }
        else if( op == "FCIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            base->FilledCircle( c, d, md( t ) );
        }
        else if( op == "TOVAL" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            EDA_ANGLE o = t.ang();
            int       w = t.n();
            base->ThickOval( p, s, o, w, md( t ) );
        }
        else if( op == "POLY" )
        {
            FILL_T        f = t.fill();
            int           w = t.n();
            GBR_METADATA* m = md( t );
            std::vector<VECTOR2I> v = t.pts();
            base->PlotPoly( v, f, w, m );
        }
        else if( op == "CHAIN" )
        {
            FILL_T           f = t.fill();
            int              w = t.n();
            GBR_METADATA*    m = md( t );
            SHAPE_LINE_CHAIN c = t.chain();
            base->PlotPoly( c, f, w, m );
        }
        else if( op == "TPOLY" )
        {
            int            w = t.n();
            GBR_METADATA*  m = md( t );
            SHAPE_POLY_SET ps = t.polyset();
            base->ThickPoly( ps, w, m );
        }
        else if( op == "BEZ" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            VECTOR2I c = t.pt();
            VECTOR2I d = t.pt();
            int      tol = t.n();
            int      w = t.n();
            base->BezierCurve( a, b, c, d, tol, w );
        }
        else if( op == "FC" )
        {
            VECTOR2I p = t.pt();
            int      d = t.n();
            base->FlashPadCircle( p, d, md( t ) );
        }
        else if( op == "FO" || op == "FR" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            EDA_ANGLE o = t.ang();

            if( op == "FO" )
                base->FlashPadOval( p, s, o, md( t ) );
            else
                base->FlashPadRect( p, s, o, md( t ) );
        }
        else if( op == "FRR" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            int       r = t.n();
            EDA_ANGLE o = t.ang();
            recordChamfer( p, s, o, r, 0.0, 0, svg->HighDef() );
            base->FlashPadRoundRect( p, s, r, o, md( t ) );
        }
        else if( op == "FT" )
        {
            VECTOR2I p = t.pt();
            VECTOR2I c[4];

            for( VECTOR2I& k : c )
                k = t.pt();

            EDA_ANGLE o = t.ang();
            base->FlashPadTrapez( p, c, o, md( t ) );
        }
        else if( op == "FCUS" )
        {
            VECTOR2I       p = t.pt();
            VECTOR2I       s = t.pt();
            EDA_ANGLE      o = t.ang();
            GBR_METADATA*  m = md( t );
            SHAPE_POLY_SET ps = t.polyset();
            base->FlashPadCustom( p, s, o, &ps, m );
        }
        else if( op == "MARK" )
        {
            VECTOR2I p = t.pt();
            int      d = t.n();
            int      id = t.n();
            base->Marker( p, d, id );
        }
        else if( op == "SB" )
        {
            base->StartBlock( nullptr );
        }
        else if( op == "EB" )
        {
            base->EndBlock( nullptr );
        }
        else if( op == "TEXT" )
        {
            VECTOR2I  p = t.pt();
            COLOR4D   c = t.color();
            wxString  txt = t.hex();
            EDA_ANGLE o = t.ang();
            VECTOR2I  s = t.pt();
            int       h = t.n();
            int       v = t.n();
            int       w = t.n();
            bool      it = t.n();
            bool      bo = t.n();
            bool      ml = t.n();
            bool      font = t.n();
            GBR_METADATA* m = md( t );
            base->Text( p, c, txt, o, s, (GR_TEXT_H_ALIGN_T) h, (GR_TEXT_V_ALIGN_T) v, w, it, bo,
                        ml, font ? KIFONT::FONT::GetFont() : nullptr,
                        KIFONT::METRICS::Default(), m );
        }
        else if( op == "PTEXT" )
        {
            VECTOR2I        p = t.pt();
            COLOR4D         c = t.color();
            wxString        txt = t.hex();
            TEXT_ATTRIBUTES a;
            a.m_Angle = t.ang();
            a.m_Size = t.pt();
            a.m_Halign = (GR_TEXT_H_ALIGN_T) t.n();
            a.m_Valign = (GR_TEXT_V_ALIGN_T) t.n();
            a.m_StrokeWidth = t.n();
            a.m_Italic = t.n();
            a.m_Bold = t.n();
            a.m_Mirrored = t.n();
            a.m_Multiline = t.n();
            a.m_LineSpacing = t.d();
            a.m_Underlined = t.n();
            a.m_KeepUpright = t.n();
            bool          font = t.n();
            GBR_METADATA* m = md( t );
            base->PlotText( p, c, txt, a, font ? KIFONT::FONT::GetFont() : nullptr,
                            KIFONT::METRICS::Default(), m );
        }
        else
        {
            fprintf( stderr, "unknown op %s\n", op.c_str() );
            exit( 1 );
        }
    }

    if( started )
        base->EndPlot();

    svg.reset();
    gbr.reset();

    std::ifstream     in( path, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    std::string file = ss.str();
    remove( path.c_str() );

    if( isSvg )
    {
        const std::string marker = "<title>SVG Image created as ";
        size_t            p = file.find( marker );

        if( p != std::string::npos )
        {
            size_t e = file.find( '\n', p );
            file.replace( p, e - p, marker + "plot.svg date @DATE@ </title>" );
        }
    }
    else
    {
        const std::string marker = "G04 Created by KiCad (";
        size_t            p = file.find( marker );

        if( p != std::string::npos && ( p == 0 || file[p - 1] == '\n' ) )
        {
            size_t e = file.find( '\n', p );
            file.replace( p, e - p, marker + creator + " @VERSION@) date @DATE@*" );
        }
    }

    return file;
}


int main( int argc, char** argv )
{
    if( argc < 2 )
    {
        fprintf( stderr, "usage: %s SCRIPTS [full]\n", argv[0] );
        return 2;
    }

    bool full = argc > 2 && strcmp( argv[2], "full" ) == 0;

    std::ifstream     in( argv[1], std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    std::string all = ss.str();

    std::string tmp = std::string( "/tmp/svg_text_oracle_" ) + std::to_string( getpid() );

    std::vector<std::vector<std::vector<std::string>>> scripts;
    std::istringstream lines( all );
    std::string        line;

    while( std::getline( lines, line ) )
    {
        std::vector<std::string> tok;
        std::istringstream       ls( line );
        std::string              w;

        while( ls >> w )
            tok.push_back( w );

        if( tok.empty() )
            continue;

        if( tok[0] == "BEGIN" )
            scripts.emplace_back();
        else if( tok[0] != "END" )
            scripts.back().push_back( tok );
    }

    std::vector<std::string> results;

    for( size_t i = 0; i < scripts.size(); i++ )
        results.push_back( runScript( scripts[i], tmp ) );

    printf( "SCRIPTS %zu %016" PRIx64 "\n", scripts.size(), fnv( all ) );

    for( const auto& [key, val] : g_chamfer )
        printf( "CH %s %s\n", key.c_str(), val.c_str() );

    for( size_t i = 0; i < results.size(); i++ )
    {
        if( full || (int) i < FULL_COUNT )
        {
            printf( "F %zu\n%sF-END\n", i, results[i].c_str() );

            if( full )
                continue;
        }

        printf( "R %zu %08" PRIx64 "\n", i, fnv( results[i] ) & 0xffffffffULL );
    }

    return 0;
}
