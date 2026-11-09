// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// Differential oracle of work packages PLOT-03 and PLOT-04 (GERBER_PLOTTER) of the kicad-plot
// crate.  It reads plot scripts (written by tests/test_gerber_oracle.rs with
// KICAD_PLOT_WRITE_SCRIPTS=1), runs each of them on the real GERBER_PLOTTER of the reference
// build (common/libcommon.a) and prints, per script, the FNV-1a 64 hash of the produced file.
//
//   gerber_plotter_oracle SCRIPTS [full]
//
// Golden output (tests/golden/gerber_plotter.txt):
//   SCRIPTS <count> <fnv64 of the script file>
//   CH <key> <n> <x y>...      one line per distinct TransformRoundChamferedRectToPolygon call
//   F <idx> ... F-END          the complete file of the first FULL_COUNT scripts
//   R <idx> <fnv64 of the file>
// "full" prints the complete file of every script instead of its hash.
//
// The "G04 Created by KiCad (...)" line holds the build version and the wall clock; it is
// replaced by "G04 Created by KiCad (<creator> @VERSION@) date @DATE@*" before hashing.

#include <cinttypes>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include <unistd.h>

#include <convert_basic_shapes_to_polygon.h>
#include <gbr_metadata.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <plotters/plotter_gerber.h>
#include <render_settings.h>

static const int FULL_COUNT = 6;

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

// Expose the protected FlashPadChamferRoundRect polygon parameters (public in fact) and the
// arc high definition.
class GBR : public GERBER_PLOTTER
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

// ---- tokenized script reader ----

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
    VECTOR2I  pt() { int x = n(); int y = n(); return VECTOR2I( x, y ); }
    VECTOR2D  ptd() { double x = d(); double y = d(); return VECTOR2D( x, y ); }
    EDA_ANGLE ang() { return EDA_ANGLE( d(), DEGREES_T ); }
    FILL_T    fill() { return s() == "F" ? FILL_T::FILLED_SHAPE : FILL_T::NO_FILL; }

    std::vector<VECTOR2I> pts()
    {
        int                   cnt = n();
        std::vector<VECTOR2I> v;

        for( int k = 0; k < cnt; k++ )
            v.push_back( pt() );

        return v;
    }

    // closed n items: P x y | A sx sy mx my ex ey
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
                              const std::string& aPath )
{
    GBR                  plotter;
    PLOTTER&             base = plotter;
    TEST_RENDER_SETTINGS rs;
    std::map<int, GBR_METADATA> mds;
    std::string          creator;
    bool                 started = false;
    bool                 nomacros = false;

    plotter.SetRenderSettings( &rs );
    plotter.OpenFile( wxString::FromUTF8( aPath ) );

    auto md = [&]( TOK& t ) -> GBR_METADATA*
    {
        int id = t.n();

        if( id < 0 )
            return nullptr;

        return &mds[id];
    };

    for( const std::vector<std::string>& line : aLines )
    {
        TOK t;
        t.t = line;
        const std::string& op = line[0];

        if( op == "CFG" )
        {
            bool x2 = t.n(), net = t.n(), nomacro = t.n();
            int  fmt = t.n();
            bool inch = t.n();
            VECTOR2I off = t.pt();
            double   ipd = t.d();
            plotter.SetViewport( off, ipd, 1.0, false );
            plotter.SetGerberCoordinatesFormat( fmt, inch );
            plotter.UseX2format( x2 );
            plotter.UseX2NetAttributes( net );
            plotter.DisableApertMacros( nomacro );
            nomacros = nomacro;
        }
        else if( op == "CREATOR" )
        {
            creator = t.str();
            plotter.SetCreator( wxString::FromUTF8( creator ) );
        }
        else if( op == "HDR" )
        {
            plotter.AddLineToHeader( wxString::FromUTF8( t.str() ) );
        }
        else if( op == "RS" )
        {
            rs.SetDefaultPenWidth( t.n() );
        }
        else if( op == "MD" )
        {
            int           id = t.n();
            GBR_METADATA& m = mds[id];
            m = GBR_METADATA();
            m.m_ApertureMetadata.m_ApertAttribute =
                    (GBR_APERTURE_METADATA::GBR_APERTURE_ATTRIB) t.n();
            m.m_ApertureMetadata.m_CustomAttribute = t.str();
            m.m_NetlistMetadata.m_NetAttribType = t.n();
            m.m_NetlistMetadata.m_NotInNet = t.n();
            std::string pad = t.str();
            int         padflags = t.n();
            m.SetPadName( wxString::FromUTF8( pad ), padflags & 1, padflags & 2 );
            std::string pin = t.str();
            int         pinflags = t.n();
            m.SetPadPinFunction( wxString::FromUTF8( pin ), pinflags & 1, pinflags & 2 );
            m.SetCmpReference( wxString::FromUTF8( t.str() ) );
            m.SetNetName( wxString::FromUTF8( t.str() ) );
            m.m_NetlistMetadata.m_ExtraData = wxString::FromUTF8( g_extra[t.n()] );
        }
        else if( op == "START" )
        {
            plotter.StartPlot( wxT( "1" ) );
            started = true;
        }
        else if( op == "W" )
        {
            int w = t.n();
            base.SetCurrentLineWidth( w, md( t ) );
        }
        else if( op == "PEN" )
        {
            char c = t.s()[0];
            VECTOR2I p = t.pt();
            base.PenTo( p, c );
        }
        else if( op == "MV" )
        {
            base.MoveTo( t.pt() );
        }
        else if( op == "LN" )
        {
            base.LineTo( t.pt() );
        }
        else if( op == "FIN" )
        {
            base.FinishTo( t.pt() );
        }
        else if( op == "SEG" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            int      w = t.n();
            base.ThickSegment( a, b, w, md( t ) );
        }
        else if( op == "ARC" )
        {
            VECTOR2D  c = t.ptd();
            EDA_ANGLE sa = t.ang();
            EDA_ANGLE a = t.ang();
            double    r = t.d();
            FILL_T    f = t.fill();
            int       w = t.n();
            base.Arc( c, sa, a, r, f, w );
        }
        else if( op == "ARC3" )
        {
            VECTOR2D s = t.ptd();
            VECTOR2D m = t.ptd();
            VECTOR2D e = t.ptd();
            FILL_T   f = t.fill();
            int      w = t.n();
            base.Arc( s, m, e, f, w );
        }
        else if( op == "TARC" )
        {
            VECTOR2D  c = t.ptd();
            EDA_ANGLE sa = t.ang();
            EDA_ANGLE a = t.ang();
            double    r = t.d();
            int       w = t.n();
            base.ThickArc( c, sa, a, r, w, md( t ) );
        }
        else if( op == "RECT" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            FILL_T   f = t.fill();
            int      w = t.n();
            base.Rect( a, b, f, w, 0 );
        }
        else if( op == "TRECT" )
        {
            VECTOR2I a = t.pt();
            VECTOR2I b = t.pt();
            int      w = t.n();
            base.ThickRect( a, b, w, md( t ) );
        }
        else if( op == "CIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            FILL_T   f = t.fill();
            int      w = t.n();
            base.Circle( c, d, f, w );
        }
        else if( op == "TCIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            int      w = t.n();
            base.ThickCircle( c, d, w, md( t ) );
        }
        else if( op == "FCIRC" )
        {
            VECTOR2I c = t.pt();
            int      d = t.n();
            base.FilledCircle( c, d, md( t ) );
        }
        else if( op == "TOVAL" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            EDA_ANGLE o = t.ang();
            int       w = t.n();
            base.ThickOval( p, s, o, w, md( t ) );
        }
        else if( op == "POLY" )
        {
            FILL_T        f = t.fill();
            int           w = t.n();
            GBR_METADATA* m = md( t );
            std::vector<VECTOR2I> v = t.pts();
            base.PlotPoly( v, f, w, m );
        }
        else if( op == "CHAIN" )
        {
            FILL_T           f = t.fill();
            int              w = t.n();
            GBR_METADATA*    m = md( t );
            SHAPE_LINE_CHAIN c = t.chain();
            base.PlotPoly( c, f, w, m );
        }
        else if( op == "REGV" )
        {
            GBR_METADATA*         m = md( t );
            std::vector<VECTOR2I> v = t.pts();
            plotter.PlotGerberRegion( v, m );
        }
        else if( op == "REGC" )
        {
            GBR_METADATA*    m = md( t );
            SHAPE_LINE_CHAIN c = t.chain();
            plotter.PlotGerberRegion( c, m );
        }
        else if( op == "ASREG" )
        {
            FILL_T           f = t.fill();
            int              w = t.n();
            GBR_METADATA*    m = md( t );
            SHAPE_LINE_CHAIN c = t.chain();
            plotter.PlotPolyAsRegion( c, f, w, m );
        }
        else if( op == "TPOLY" )
        {
            int            w = t.n();
            GBR_METADATA*  m = md( t );
            SHAPE_POLY_SET ps = t.polyset();
            base.ThickPoly( ps, w, m );
        }
        else if( op == "FC" )
        {
            VECTOR2I p = t.pt();
            int      d = t.n();
            base.FlashPadCircle( p, d, md( t ) );
        }
        else if( op == "FO" || op == "FR" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            EDA_ANGLE o = t.ang();

            if( op == "FO" )
                base.FlashPadOval( p, s, o, md( t ) );
            else
                base.FlashPadRect( p, s, o, md( t ) );
        }
        else if( op == "FRR" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            int       r = t.n();
            EDA_ANGLE o = t.ang();
            base.FlashPadRoundRect( p, s, r, o, md( t ) );
        }
        else if( op == "FT" )
        {
            VECTOR2I p = t.pt();
            VECTOR2I c[4];

            for( VECTOR2I& k : c )
                k = t.pt();

            EDA_ANGLE o = t.ang();
            base.FlashPadTrapez( p, c, o, md( t ) );
        }
        else if( op == "FCUS" )
        {
            VECTOR2I       p = t.pt();
            VECTOR2I       s = t.pt();
            EDA_ANGLE      o = t.ang();
            GBR_METADATA*  m = md( t );
            SHAPE_POLY_SET ps = t.polyset();
            base.FlashPadCustom( p, s, o, &ps, m );
        }
        else if( op == "FCH" )
        {
            VECTOR2I  p = t.pt();
            VECTOR2I  s = t.pt();
            int       r = t.n();
            double    ratio = t.d();
            int       pos = t.n();
            EDA_ANGLE o = t.ang();

            if( ( r != 0 && pos != 15 ) || nomacros )
                recordChamfer( p, s, o, r, ratio, pos, plotter.HighDef() );
            else
                recordChamfer( VECTOR2I( 0, 0 ), s, ANGLE_0, 0, ratio, pos, plotter.HighDef() );
            plotter.FlashPadChamferRoundRect( p, s, r, ratio, pos, o, md( t ) );
        }
        else if( op == "FPOLY" )
        {
            VECTOR2I  p = t.pt();
            int       d = t.n();
            int       n = t.n();
            EDA_ANGLE o = t.ang();
            base.FlashRegularPolygon( p, d, n, o, md( t ) );
        }
        else if( op == "POL" )
        {
            base.SetLayerPolarity( t.n() != 0 );
        }
        else if( op == "SB" )
        {
            base.StartBlock( md( t ) );
        }
        else if( op == "EB" )
        {
            base.EndBlock( md( t ) );
        }
        else if( op == "CLR" )
        {
            plotter.ClearAllAttributes();
        }
        else if( op == "MARK" )
        {
            VECTOR2I p = t.pt();
            int      d = t.n();
            int      id = t.n();
            base.Marker( p, d, id );
        }
        else
        {
            fprintf( stderr, "unknown op %s\n", op.c_str() );
            exit( 1 );
        }
    }

    if( started )
        plotter.EndPlot();

    std::ifstream     in( aPath, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    std::string file = ss.str();

    // Normalise the creation line.
    const std::string marker = "G04 Created by KiCad (";
    size_t            p = file.find( marker );

    if( p != std::string::npos && ( p == 0 || file[p - 1] == '\n' ) )
    {
        size_t e = file.find( '\n', p );
        file.replace( p, e - p, marker + creator + " @VERSION@) date @DATE@*" );
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

    std::ifstream in( argv[1], std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    std::string all = ss.str();

    std::string tmp = std::string( "/tmp/gerber_oracle_" ) + std::to_string( getpid() ) + ".gbr";

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

    remove( tmp.c_str() );

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

        printf( "R %zu %016" PRIx64 "\n", i, fnv( results[i] ) );
    }

    return 0;
}
