// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// Differential oracle of work packages PLOT-01 (PLOTTER base class, drill markers) and PLOT-02
// (Gerber metadata) of the kicad-plot crate.  It calls the original functions of
// common/libcommon.a of the reference build on deterministic edge-case and pseudo-random inputs
// and prints one line per result.  The PLOTTER base class is exercised through REC, a tiny
// subclass that records every call of a pure virtual method.
//
//   plot_base_oracle          golden mode: per section the first lines verbatim
//                             ("E <section> <line>") and "S <section> <count> <fnv64>"
//   plot_base_oracle full     every line, prefixed with the section name
//
// Lines are ASCII: control characters and every non-ASCII byte of UTF-8 text are written as
// \xHH.  tests/test_plot_base_oracle.rs produces the same lines with the Rust port.  Keep the
// two in sync: the order of the pseudo-random draws matters.

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include <wx/image.h>

#include <gbr_metadata.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <plotters/drill_markers.h>
#include <plotters/gbr_plotter_apertures.h>
#include <plotters/plotter.h>
#include <render_settings.h>

// ---- shared helpers (mirrored in Rust) ----

struct LCG
{
    uint64_t s;
    uint64_t next()
    {
        s = s * 6364136223846793005ULL + 1442695040888963407ULL;
        return s >> 33;
    }
    uint32_t below( uint32_t n ) { return (uint32_t) ( next() % n ); }
    int      range( int lo, int hi ) { return lo + (int) below( (uint32_t) ( hi - lo + 1 ) ); }
};

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

static std::string I( long long v ) { return std::to_string( v ); }

static std::string PT( const VECTOR2I& p ) { return I( p.x ) + "," + I( p.y ); }

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
    else if( g_count < 8 )
        printf( "E %s %s\n", g_section.c_str(), line.c_str() );

    g_count++;
}

static void end()
{
    if( !g_full )
        printf( "S %s %d %016llx\n", g_section.c_str(), g_count, (unsigned long long) g_hash );
}

// Random inputs

static int rcoord( LCG& g )
{
    switch( g.below( 4 ) )
    {
    case 0: return g.range( -50, 50 );
    case 1: return g.range( -1000000, 1000000 );
    case 2: return g.range( -100000000, 100000000 );
    default: return g.range( -5000, 5000 ) * 1000;
    }
}

static VECTOR2I rpt( LCG& g )
{
    int x = rcoord( g );
    int y = rcoord( g );
    return VECTOR2I( x, y );
}

static double rdouble( LCG& g )
{
    double a = (double) rcoord( g );

    switch( g.below( 3 ) )
    {
    case 0: return a;
    case 1: return a + (double) g.below( 1000 ) / 64.0;
    default: return a / 7.0;
    }
}

static double rdeg( LCG& g )
{
    static const double special[] = { 0.0, 90.0, 180.0, -180.0, 360.0, 45.0, 0.05, -0.05,
                                       270.0, -90.0, 720.0, 359.99 };

    switch( g.below( 3 ) )
    {
    case 0: return special[g.below( 12 )];
    case 1: return (double) g.range( -144, 144 ) * 5.0;
    default: return (double) g.range( -400000, 400000 ) / 1000.0;
    }
}

static int rwidth( LCG& g )
{
    switch( g.below( 5 ) )
    {
    case 0: return PLOTTER::DO_NOT_SET_LINE_WIDTH;
    case 1: return PLOTTER::USE_DEFAULT_LINE_WIDTH;
    case 2: return 0;
    default: return g.range( 1, 3000000 );
    }
}

static FILL_T rfill( LCG& g )
{
    return g.below( 2 ) ? FILL_T::NO_FILL : FILL_T::FILLED_SHAPE;
}

static const uint32_t CODEPOINTS[] = { '\\', '%', '*', ',', '"', 0, 1, '\n', 0x7F, 0x80, 0xA0,
                                       0xE9, 0xFF, 0x100, 0x3A9, 0x4E2D, 0xFEFF, 0xFFFD, 0xFFFF,
                                       0x10000, 0x1F600, 0x10FFFF, 'X', ' ' };

static wxString rstring( LCG& g, int maxLen )
{
    wxString s;
    int      len = (int) g.below( (uint32_t) maxLen + 1 );

    for( int i = 0; i < len; i++ )
    {
        uint32_t cp;

        if( g.below( 3 ) == 0 )
            cp = CODEPOINTS[g.below( sizeof( CODEPOINTS ) / sizeof( CODEPOINTS[0] ) )];
        else
            cp = 0x21 + g.below( 0x5E );

        s.Append( wxUniChar( cp ) );
    }

    // Sometimes a quoted string
    if( g.below( 8 ) == 0 )
        s = wxS( "\"" ) + s + wxS( "\"" );

    return s;
}

// ---- recording plotter ----

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

class REC : public PLOTTER
{
public:
    using PLOTTER::PlotPoly;

    std::string m_log;

    void ev( const std::string& e )
    {
        m_log += ' ';
        m_log += e;
    }

    PLOT_FORMAT GetPlotterType() const override { return PLOT_FORMAT::UNDEFINED; }
    bool StartPlot( const wxString& ) override { return true; }
    bool EndPlot() override { return true; }

    void SetCurrentLineWidth( int aWidth, void* aData ) override
    {
        ev( "W" + I( aWidth ) );

        if( aWidth == USE_DEFAULT_LINE_WIDTH )
            m_currentPenWidth = 77;
        else if( aWidth != DO_NOT_SET_LINE_WIDTH )
            m_currentPenWidth = aWidth;
    }

    void SetColor( const COLOR4D& ) override {}
    void SetDash( int, LINE_STYLE ) override {}
    void SetViewport( const VECTOR2I&, double, double, bool ) override {}

    void Rect( const VECTOR2I& p1, const VECTOR2I& p2, FILL_T fill, int width,
               int aCornerRadius ) override
    {
        ev( "R" + PT( p1 ) + ";" + PT( p2 ) + ";" + I( (int) fill ) + ";" + I( width ) + ";"
            + I( aCornerRadius ) );
    }

    void Circle( const VECTOR2I& pos, int diametre, FILL_T fill, int width ) override
    {
        ev( "C" + PT( pos ) + ";" + I( diametre ) + ";" + I( (int) fill ) + ";" + I( width ) );
    }

    void PenTo( const VECTOR2I& pos, char plume ) override
    {
        ev( std::string( "P" ) + plume + PT( pos ) );
    }

    void PlotPoly( const std::vector<VECTOR2I>& aCornerList, FILL_T aFill, int aWidth,
                   void* aData ) override
    {
        std::string e = "Y" + I( (int) aFill ) + ";" + I( aWidth ) + ";" + I( aCornerList.size() );

        for( const VECTOR2I& p : aCornerList )
            e += ";" + PT( p );

        ev( e );
    }

    void FlashPadCircle( const VECTOR2I&, int, void* ) override {}
    void FlashPadOval( const VECTOR2I&, const VECTOR2I&, const EDA_ANGLE&, void* ) override {}
    void FlashPadRect( const VECTOR2I&, const VECTOR2I&, const EDA_ANGLE&, void* ) override {}
    void FlashPadRoundRect( const VECTOR2I&, const VECTOR2I&, int, const EDA_ANGLE&,
                            void* ) override {}
    void FlashPadCustom( const VECTOR2I&, const VECTOR2I&, const EDA_ANGLE&, SHAPE_POLY_SET*,
                         void* ) override {}
    void FlashPadTrapez( const VECTOR2I&, const VECTOR2I*, const EDA_ANGLE&, void* ) override {}
    void FlashRegularPolygon( const VECTOR2I&, int, int, const EDA_ANGLE&, void* ) override {}

    std::string take()
    {
        std::string r = m_log;
        m_log.clear();
        return r;
    }

    // access to protected members
    void setTransform( VECTOR2I aOffset, double aScale, double aIus, double aIuPerDev,
                       VECTOR2I aPaper, bool aMirror, bool aHoriz, bool aYrev )
    {
        m_plotOffset = aOffset;
        m_plotScale = aScale;
        m_IUsPerDecimil = aIus;
        m_iuPerDeviceUnit = aIuPerDev;
        m_paperSize = aPaper;
        m_plotMirror = aMirror;
        m_mirrorIsHorizontal = aHoriz;
        m_yaxisReversed = aYrev;
    }

    VECTOR2D u2dc( const VECTOR2I& p ) { return userToDeviceCoordinates( p ); }
    VECTOR2D u2ds( const VECTOR2I& p ) { return userToDeviceSize( p ); }
    double   u2dsf( double v ) { return userToDeviceSize( v ); }
    double   dot( int w ) { return GetDotMarkLenIU( w ); }
    double   dash( int w ) { return GetDashMarkLenIU( w ); }
    double   gap( int w ) { return GetDashGapLenIU( w ); }
    void     mSquare( const VECTOR2I& p, int r ) { markerSquare( p, r ); }
    void     mLozenge( const VECTOR2I& p, int r ) { markerLozenge( p, r ); }
    void     mCircle( const VECTOR2I& p, int r ) { markerCircle( p, r ); }
    void     mBars( const VECTOR2I& p, int r )
    {
        markerHBar( p, r );
        markerSlash( p, r );
        markerBackSlash( p, r );
        markerVBar( p, r );
    }
    void setPen( int w ) { m_currentPenWidth = w; }
};

// ---- sections ----

static void sectionTransform()
{
    LCG                  g{ 101 };
    REC                  rec;
    TEST_RENDER_SETTINGS rs;
    rec.SetRenderSettings( &rs );

    begin( "transform" );

    const double scales[] = { 1.0, 0.5, 1.7, 2.0, 0.123456789 };
    const double iuPerDev[] = { 1.0, std::pow( 10.0, 6 ) / ( 2540 * 10000.0 ) * 25.4,
                                std::pow( 10.0, 5 ) / ( 2540 * 10000.0 ) * 25.4,
                                1.0 / ( 2540 / 2.54 * 1000 ), 0.0001 * 3 / 7 };

    for( int i = 0; i < 3000; i++ )
    {
        VECTOR2I off = rpt( g );
        double   scale = scales[g.below( 5 )];
        double   iu = iuPerDev[g.below( 5 )];
        VECTOR2I paper{ g.range( 0, 500000000 ), g.range( 0, 500000000 ) };
        bool     mirror = g.below( 2 );
        bool     horiz = g.below( 2 );
        bool     yrev = g.below( 2 );
        VECTOR2I p = rpt( g );
        int      w = g.range( 0, 1000000 );
        double   v = rdouble( g );

        rs.SetDashLengthRatio( (double) g.range( 0, 30 ) / 2.0 );
        rs.SetGapLengthRatio( (double) g.range( -4, 20 ) / 2.0 );

        rec.setTransform( off, scale, 2540.0, iu, paper, mirror, horiz, yrev );

        VECTOR2D c = rec.u2dc( p );
        VECTOR2D s = rec.u2ds( p );

        out( PT( off ) + " " + PT( paper ) + " " + I( mirror ) + I( horiz ) + I( yrev ) + " "
             + PT( p ) + " " + H( c.x ) + " " + H( c.y ) + " " + H( s.x ) + " " + H( s.y ) + " "
             + H( rec.u2dsf( v ) ) + " " + H( rec.dot( w ) ) + " " + H( rec.dash( w ) ) + " "
             + H( rec.gap( w ) ) );
    }

    end();
}

static void sectionArcs()
{
    LCG g{ 202 };
    REC rec;

    begin( "polyarc" );

    for( int i = 0; i < 2000; i++ )
    {
        VECTOR2D  c{ rdouble( g ), rdouble( g ) };
        EDA_ANGLE st( rdeg( g ), DEGREES_T );
        EDA_ANGLE an( rdeg( g ), DEGREES_T );
        double    r = std::fabs( rdouble( g ) );
        FILL_T    fill = rfill( g );
        int       w = rwidth( g );

        rec.Arc( c, st, an, r, fill, w );
        out( H( c.x ) + " " + H( c.y ) + " " + H( st.AsDegrees() ) + " " + H( an.AsDegrees() )
             + " " + H( r ) + rec.take() );
    }

    end();

    begin( "arc3pt" );

    for( int i = 0; i < 2000; i++ )
    {
        VECTOR2D a{ rdouble( g ), rdouble( g ) };
        VECTOR2D m{ rdouble( g ), rdouble( g ) };
        VECTOR2D e{ rdouble( g ), rdouble( g ) };

        if( g.below( 10 ) == 0 )
            e = a;

        FILL_T fill = rfill( g );
        int    w = rwidth( g );

        rec.Arc( a, m, e, fill, w );
        out( H( a.x ) + H( a.y ) + H( m.x ) + H( m.y ) + H( e.x ) + H( e.y ) + rec.take() );
    }

    end();
}

static void sectionThick()
{
    LCG g{ 303 };
    REC rec;

    begin( "thick" );

    for( int i = 0; i < 3000; i++ )
    {
        int      kind = g.below( 7 );
        VECTOR2I a = rpt( g );
        VECTOR2I b = g.below( 4 ) == 0 ? a : rpt( g );
        int      w = rwidth( g );
        int      d = g.range( 0, 4000000 );

        rec.setPen( g.range( -3, 5000 ) );

        std::string head = I( kind ) + " " + PT( a ) + " " + PT( b ) + " " + I( w ) + " " + I( d );

        switch( kind )
        {
        case 0: rec.ThickSegment( a, b, w, nullptr ); break;
        case 1: rec.ThickRect( a, b, w, nullptr ); break;
        case 2: rec.ThickCircle( a, d, w, nullptr ); break;
        case 3: rec.FilledCircle( a, d, nullptr ); break;
        case 4:
        {
            VECTOR2I  size{ g.range( 0, 3000000 ), g.range( 0, 3000000 ) };
            EDA_ANGLE o( rdeg( g ), DEGREES_T );
            head += " " + PT( size ) + " " + H( o.AsDegrees() );
            rec.ThickOval( a, size, o, w, nullptr );
            break;
        }
        case 5:
        {
            EDA_ANGLE st( rdeg( g ), DEGREES_T );
            EDA_ANGLE an( rdeg( g ), DEGREES_T );
            double    r = (double) d / 3.0;
            head += " " + H( st.AsDegrees() ) + " " + H( an.AsDegrees() );
            rec.ThickArc( VECTOR2D( a ), st, an, r, w, nullptr );
            break;
        }
        default:
        {
            int  sx = g.range( 1, 3000 );
            int  sy = g.range( 1, 3000 );
            head += " " + I( sx ) + " " + I( sy );
            rec.PlotImage( wxImage( sx, sy ), a, (double) g.range( 1, 4000 ) / 7.0, 1.0 );
            break;
        }
        }

        out( head + rec.take() );
    }

    end();

    begin( "polys" );

    for( int i = 0; i < 1500; i++ )
    {
        int                   n = g.below( 7 );
        std::vector<VECTOR2I> pts;

        for( int k = 0; k < n; k++ )
            pts.push_back( rpt( g ) );

        if( n > 2 && g.below( 3 ) == 0 )
            pts.push_back( pts.front() );

        bool   closed = g.below( 2 );
        FILL_T fill = rfill( g );
        int    w = rwidth( g );

        std::string head = I( n ) + " " + I( closed ) + " " + I( pts.size() );

        SHAPE_LINE_CHAIN chain( pts, closed );
        // An empty closed chain is undefined behaviour in C++ (front() of an empty vector)
        if( !( closed && pts.empty() ) )
            rec.PlotPoly( chain, fill, w, nullptr );

        if( n > 0 )
        {
            SHAPE_LINE_CHAIN closedChain( pts, true );
            SHAPE_POLY_SET   set;
            set.AddOutline( closedChain );
            set.AddOutline( SHAPE_LINE_CHAIN( { VECTOR2I( 1, 2 ), VECTOR2I( 3, 4 ),
                                                VECTOR2I( 5, 0 ) },
                                              true ) );
            rec.ThickPoly( set, w, nullptr );
        }

        out( head + rec.take() );
    }

    end();
}

static void sectionBezier()
{
    LCG g{ 808 };
    REC rec;

    begin( "bezier" );

    for( int i = 0; i < 1000; i++ )
    {
        VECTOR2I a = rpt( g );
        VECTOR2I c1 = g.below( 6 ) == 0 ? a : rpt( g );
        VECTOR2I c2 = rpt( g );
        VECTOR2I e = g.below( 6 ) == 0 ? c2 : rpt( g );
        int      tol = g.range( 1, 20000 );
        int      w = rwidth( g );

        rec.BezierCurve( a, c1, c2, e, tol, w );
        out( PT( a ) + " " + PT( c1 ) + " " + PT( c2 ) + " " + PT( e ) + " " + I( tol )
             + rec.take() );
    }

    end();
}


static void sectionMarkers()
{
    LCG g{ 404 };
    REC rec;

    begin( "markers" );

    for( unsigned id = 0; id < 62; id++ )
    {
        for( int k = 0; k < 6; k++ )
        {
            VECTOR2I p = rpt( g );
            int      d = g.range( 0, 3000000 );
            rec.setPen( g.range( 0, 5000 ) );
            rec.Marker( p, d, id );
            out( I( id ) + " " + PT( p ) + " " + I( d ) + rec.take() );
        }
    }

    for( int k = 0; k < 300; k++ )
    {
        VECTOR2I p = rpt( g );
        int      r = g.range( 0, 3000000 );
        rec.setPen( g.range( 0, 5000 ) );
        rec.mSquare( p, r );
        rec.mLozenge( p, r );
        rec.mCircle( p, r );
        rec.mBars( p, r );
        out( PT( p ) + " " + I( r ) + rec.take() );
    }

    end();

    begin( "curated" );

    std::string line = I( DRILL_MARKERS::CuratedShapeCount() );

    for( int i = -2; i < 15; i++ )
        line += " " + I( DRILL_MARKERS::CuratedShape( i ) );

    out( line );
    end();
}

static void sectionGuid()
{
    LCG g{ 505 };

    begin( "guid" );

    const char* fixed[] = { "basic_test.kicad_pcb", "Issue24143.kicad_pcb", "", "a",
                            "0123456789abcdef", "0123456789abcdefghij" };

    for( const char* f : fixed )
        out( esc( f ) + " " + U( GbrMakeProjectGUIDfromString( wxString::FromUTF8( f ) ) ) );

    for( int i = 0; i < 3000; i++ )
    {
        wxString s = rstring( g, 24 );
        out( U( s ) + " " + U( GbrMakeProjectGUIDfromString( s ) ) );
    }

    end();
}

static void sectionEscape()
{
    LCG g{ 606 };

    begin( "escape" );

    for( int i = 0; i < 5000; i++ )
    {
        wxString s = rstring( g, 12 );

        std::string line = U( s );

        for( int k = 0; k < 4; k++ )
            line += " " + U( ConvertNotAllowedCharsInGerber( s, k & 1, k & 2 ) );

        line += " " + esc( FormatStringToGerber( s ) );

        GBR_DATA_FIELD field;
        bool           utf8 = g.below( 2 );
        bool           quote = g.below( 2 );
        field.SetField( s, utf8, quote );
        line += " " + I( utf8 ) + I( quote ) + " " + esc( field.GetGerberString() );

        out( line );
    }

    end();
}

static void sectionAperAttr()
{
    begin( "aperattr" );

    const char* customs[] = { "", "Custom,text", "x*%" };

    for( int a = 0; a <= GBR_APERTURE_METADATA::GBR_APERTURE_ATTRIB_END; a++ )
    {
        for( int x1 = 0; x1 < 2; x1++ )
        {
            for( const char* c : customs )
            {
                out( I( a ) + " " + I( x1 ) + " " + esc( c ) + " "
                     + esc( GBR_APERTURE_METADATA::FormatAttribute(
                             (GBR_APERTURE_METADATA::GBR_APERTURE_ATTRIB) a, x1, c ) ) );
            }
        }
    }

    end();
}

static void sectionNetAttr()
{
    LCG g{ 707 };

    begin( "netattr" );

    const wxString pool[] = { wxS( "" ), wxS( "R1" ), wxS( "U2" ), wxS( "GND" ),
                              wxS( "Net-(R1-Pad1)" ), wxS( "a,b*c%d\\e" ),
                              wxString::FromUTF8( "\xC3\xA9t\xC3\xA9" ), wxS( "\"quoted\"" ) };

    for( int seq = 0; seq < 300; seq++ )
    {
        std::string last;
        bool        x1 = g.below( 2 );

        for( int step = 0; step < 12; step++ )
        {
            GBR_NETLIST_METADATA md;
            md.m_NetAttribType = g.below( 8 );
            md.m_NotInNet = g.below( 4 ) == 0;
            md.m_TryKeepPreviousAttributes = g.below( 3 ) == 0;
            md.m_Cmpref = pool[g.below( 8 )];
            md.m_Netname = pool[g.below( 8 )];

            bool u1 = g.below( 2 );
            bool q1 = g.below( 2 );
            md.m_Padname.SetField( pool[g.below( 8 )], u1, q1 );
            bool u2 = g.below( 2 );
            bool q2 = g.below( 2 );
            md.m_PadPinFunction.SetField( pool[g.below( 8 )], u2, q2 );
            bool useNull = g.below( 20 ) == 0;

            std::string printed = "<untouched>";
            bool        clear = true;
            bool        ret = FormatNetAttribute( printed, last, useNull ? nullptr : &md, clear,
                                                  x1 );

            out( I( seq ) + " " + I( step ) + " " + I( x1 ) + " " + I( useNull )
                 + I( md.m_NetAttribType ) + I( md.m_NotInNet )
                 + I( md.m_TryKeepPreviousAttributes ) + " " + U( md.m_Cmpref ) + "|"
                 + U( md.m_Netname ) + "|" + U( md.m_Padname.GetValue() ) + I( u1 ) + I( q1 )
                 + "|" + U( md.m_PadPinFunction.GetValue() ) + I( u2 ) + I( q2 ) + " " + I( ret )
                 + I( clear ) + " " + esc( printed ) + " " + esc( last ) );
        }
    }

    end();
}

int main( int argc, char** argv )
{
    g_full = argc > 1 && std::string( argv[1] ) == "full";

    sectionTransform();
    sectionArcs();
    sectionThick();
    sectionBezier();
    sectionMarkers();
    sectionGuid();
    sectionEscape();
    sectionAperAttr();
    sectionNetAttr();

    return 0;
}
