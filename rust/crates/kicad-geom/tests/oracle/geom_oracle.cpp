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

/**
 * Differential-test oracle for the Rust port of the kimath foundation (crate kicad-geom).
 *
 * This program links the *original* C++ kimath sources, feeds them pseudo-random input from a
 * small deterministic generator and prints, per section, the number of cases and an FNV-1a
 * hash over a textual record of every input and result.  The Rust test
 * `tests/oracle_diff.rs` reproduces the same inputs with the ported code and compares the
 * hashes with the committed output of this program (`tests/golden/geom_oracle.txt`).
 *
 * Build and run (from <kicad>/rust/crates/kicad-geom/tests/oracle):
 *
 *     ./build_oracle.sh              # writes ../golden/geom_oracle.txt
 *     ./build_oracle.sh --dump FILE  # additionally dumps every record to FILE
 *
 * Sections whose name starts with "libm:" depend on the C library's transcendental functions
 * (sin, cos, tan, atan2, hypot, pow, ...); their hashes are only reproducible with the same
 * libm (glibc on x86-64 for the committed golden file).
 */

#include <cinttypes>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <functional>
#include <limits>
#include <string>
#include <vector>

#include <geometry/eda_angle.h>
#include <geometry/seg.h>
#include <geometry/vector_utils.h>
#include <math/box2.h>
#include <math/util.h>
#include <math/vector2d.h>
#include <math/wide_int.h>
#include <trigo.h>

namespace
{
FILE* g_dump = nullptr;

struct RNG
{
    uint64_t s;

    explicit RNG( uint64_t aSeed ) : s( aSeed ) {}

    // splitmix64
    uint64_t next()
    {
        s += 0x9E3779B97F4A7C15ULL;
        uint64_t z = s;
        z = ( z ^ ( z >> 30 ) ) * 0xBF58476D1CE4E5B9ULL;
        z = ( z ^ ( z >> 27 ) ) * 0x94D049BB133111EBULL;
        return z ^ ( z >> 31 );
    }

    // inclusive range
    int64_t range( int64_t aLo, int64_t aHi )
    {
        return aLo + (int64_t) ( next() % (uint64_t) ( aHi - aLo + 1 ) );
    }

    // [0, 1)
    double unit() { return (double) ( next() >> 11 ) * ( 1.0 / 9007199254740992.0 ); }

    double dbl( double aLo, double aHi ) { return aLo + ( aHi - aLo ) * unit(); }

    bool flag() { return ( next() & 1 ) != 0; }

    // A board coordinate at one of several scales, at most 5e8 in magnitude
    int coord()
    {
        switch( next() % 4 )
        {
        case 0: return (int) range( -10, 10 );
        case 1: return (int) range( -1000, 1000 );
        case 2: return (int) range( -1000000, 1000000 );
        default: return (int) range( -500000000, 500000000 );
        }
    }

    // A coordinate at a single moderate scale, at most 1e8 in magnitude
    int coordMid()
    {
        switch( next() % 3 )
        {
        case 0: return (int) range( -20, 20 );
        case 1: return (int) range( -100000, 100000 );
        default: return (int) range( -100000000, 100000000 );
        }
    }

    // NOTE: the order in which C++ evaluates function arguments is unspecified, so every
    // helper below (and every caller) draws its random values in separate statements.
    VECTOR2I pt() { int x = coord(); int y = coord(); return VECTOR2I( x, y ); }
    VECTOR2I ptMid() { int x = coordMid(); int y = coordMid(); return VECTOR2I( x, y ); }
    VECTOR2I ptR( int64_t aLo, int64_t aHi ) { int x = (int) range( aLo, aHi ); int y = (int) range( aLo, aHi ); return VECTOR2I( x, y ); }
    VECTOR2L ptL( int64_t aLo, int64_t aHi ) { int64_t x = range( aLo, aHi ); int64_t y = range( aLo, aHi ); return VECTOR2L( x, y ); }
    VECTOR2D ptD( double aLo, double aHi ) { double x = dbl( aLo, aHi ); double y = dbl( aLo, aHi ); return VECTOR2D( x, y ); }
    VECTOR2D ptDScaled( int aMinExp, int aMaxExp ) { double x = dblScaled( aMinExp, aMaxExp ); double y = dblScaled( aMinExp, aMaxExp ); return VECTOR2D( x, y ); }
    SEG      seg() { VECTOR2I a = pt(); VECTOR2I b = pt(); return SEG( a, b ); }

    // An angle in degrees with many exact cardinal / diagonal values
    double angleDeg()
    {
        switch( next() % 6 )
        {
        case 0: return 45.0 * (double) range( -16, 16 );
        case 1: return (double) range( -7200, 7200 ) / 10.0;
        case 2: return dbl( -1000.0, 1000.0 );
        case 3: { double b = 90.0 * (double) range( -8, 8 ); return b + dbl( -1e-9, 1e-9 ); }
        case 4: return dbl( -1e6, 1e6 );
        default: return (double) range( -360, 360 );
        }
    }

    // unit() scaled by an exact power of two
    double dblScaled( int aMinExp, int aMaxExp )
    {
        double u = unit();
        int    e = (int) range( aMinExp, aMaxExp );
        double v = std::ldexp( u, e );
        bool   neg = flag();
        return neg ? -v : v;
    }
};


struct REC
{
    std::string line;

    void sep() { if( !line.empty() ) line += ' '; }
    REC& i( int64_t v ) { sep(); line += std::to_string( v ); return *this; }
    REC& u( uint64_t v ) { sep(); line += std::to_string( v ); return *this; }
    REC& b( bool v ) { sep(); line += v ? '1' : '0'; return *this; }
    REC& d( double v )
    {
        sep();

        if( std::isnan( v ) )
        {
            line += "nan";
        }
        else
        {
            uint64_t bits;
            std::memcpy( &bits, &v, sizeof( bits ) );
            char buf[32];
            snprintf( buf, sizeof( buf ), "%016" PRIx64, bits );
            line += buf;
        }

        return *this;
    }
    REC& s( const std::string& v ) { sep(); line += '"'; line += v; line += '"'; return *this; }
    REC& v( const VECTOR2I& p ) { return i( p.x ).i( p.y ); }
    REC& vl( const VECTOR2L& p ) { return i( p.x ).i( p.y ); }
    REC& vd( const VECTOR2D& p ) { return d( p.x ).d( p.y ); }
    REC& a( const EDA_ANGLE& p ) { return d( p.AsDegrees() ); }
    REC& seg( const SEG& p ) { return v( p.A ).v( p.B ); }
    REC& box( const BOX2I& p ) { return v( p.GetPosition() ).vl( p.GetSize() ).b( p.IsValid() ); }
    REC& boxd( const BOX2D& p ) { return vd( p.GetPosition() ).vd( p.GetSize() ).b( p.IsValid() ); }
    REC& boxl( const BOX2L& p ) { return vl( p.GetPosition() ).vl( p.GetSize() ).b( p.IsValid() ); }
    REC& vf( const VECTOR2<float>& p ) { return d( p.x ).d( p.y ); }
    REC& vu( const VECTOR2<unsigned>& p ) { return u( p.x ).u( p.y ); }
    REC& opt( const OPT_VECTOR2I& p )
    {
        if( p )
            return b( true ).v( *p );

        return b( false );
    }
};


struct SECTION
{
    const char* name;
    uint64_t    hash = 1469598103934665603ULL;
    uint64_t    count = 0;

    explicit SECTION( const char* aName ) : name( aName ) {}

    void add( const REC& aRec )
    {
        for( unsigned char c : aRec.line )
        {
            hash ^= c;
            hash *= 1099511628211ULL;
        }

        hash ^= (unsigned char) '\n';
        hash *= 1099511628211ULL;
        count++;

        if( g_dump )
            fprintf( g_dump, "%s|%s\n", name, aRec.line.c_str() );
    }

    ~SECTION() { printf( "%s %" PRIu64 " %016" PRIx64 "\n", name, count, hash ); }
};


std::string streamStr( const VECTOR2I& v )
{
    std::stringstream ss;
    ss << v;
    return ss.str();
}

std::string streamStr( const VECTOR2D& v )
{
    std::stringstream ss;
    ss << v;
    return ss.str();
}

std::string streamStr( const SEG& v )
{
    std::stringstream ss;
    ss << v;
    return ss.str();
}

std::string streamStr( const EDA_ANGLE& v )
{
    std::stringstream ss;
    ss << v;
    return ss.str();
}


const double kRoundSpecials[] = { std::numeric_limits<double>::quiet_NaN(),
                                  std::numeric_limits<double>::infinity(),
                                  -std::numeric_limits<double>::infinity(),
                                  2147483647.5,
                                  -2147483648.5,
                                  2147483647.4999995,
                                  -2147483648.4999995,
                                  2147483646.5,
                                  -2147483647.5,
                                  9223372036854775808.0,
                                  -9223372036854775808.0,
                                  9223372036854774784.0,
                                  -9223372036854777856.0,
                                  0.49999999999999994,
                                  -0.49999999999999994,
                                  4503599627370497.0,
                                  -4503599627370497.0,
                                  4294967295.5,
                                  4294967294.5,
                                  -0.5,
                                  0.5,
                                  -0.0,
                                  0.0,
                                  1e300,
                                  -1e300,
                                  5e-324 };


// Raw libm calls on the same kind of arguments the kimath code passes.  The Rust test uses this
// section to find out whether it runs against the same libm as this oracle did: only then are
// the "libm:" sections comparable.
void sectionLibmProbe()
{
    SECTION sec( "libm_probe" );
    RNG     rng( 100 );

    for( int n = 0; n < 200000; n++ )
    {
        double deg = rng.angleDeg();
        double rad = deg * ( M_PI / 180.0 );
        double x = rng.dbl( -1e9, 1e9 );
        double y = rng.dbl( -1e9, 1e9 );
        double u = rng.dbl( -1.0, 1.0 );
        int    digits = (int) rng.range( 0, 6 );

        REC r;
        r.d( std::sin( rad ) ).d( std::cos( rad ) ).d( std::tan( rad ) ).d( std::atan2( y, x ) );
        r.d( std::hypot( x, y ) ).d( std::acos( u ) ).d( std::asin( u ) ).d( std::atan( u * 100.0 ) );
        r.d( std::pow( 10.0, (double) digits ) ).d( std::hypot( (float) x, (float) y ) );
        sec.add( r );
    }
}


void sectionKiRound()
{
    SECTION sec( "ki_round" );
    RNG     rng( 101 );

    for( int n = 0; n < 40000; n++ )
    {
        double v;

        switch( rng.next() % 6 )
        {
        case 0: v = rng.dbl( -10.0, 10.0 ); break;
        case 1: v = (double) rng.range( -1000000, 1000000 ) + 0.5; break;
        case 2: v = rng.dbl( -3e9, 3e9 ); break;
        case 3: v = rng.dbl( -1e19, 1e19 ); break;
        case 4: v = kRoundSpecials[rng.next() % ( sizeof( kRoundSpecials ) / sizeof( double ) )]; break;
        default:
        {
            double base = (double) rng.range( -2147483658LL, 2147483657LL );
            const double offs[] = { -0.5, 0.0, 0.5, 0.25 };
            v = base + offs[rng.next() % 4];
            break;
        }
        }

        float f = (float) v;

        REC r;
        r.d( v ).i( KiROUND( v, true ) ).i( KiROUND<double, int64_t>( v, true ) );
        r.u( KiROUND<double, unsigned>( v, true ) );
        r.i( KiROUND( f, true ) ).i( KiROUND<float, int64_t>( f, true ) );
        sec.add( r );
    }

    for( int n = 0; n < 5000; n++ )
    {
        uint64_t bits = rng.next();
        uint64_t shift = rng.next() % 64;
        int64_t  v = (int64_t) ( bits >> shift );

        if( rng.flag() )
            v = -v;

        REC r;
        r.i( v ).i( KiROUND( v, true ) ).i( KiROUND<int64_t, int64_t>( v, true ) );
        r.u( KiROUND<int64_t, unsigned>( v, true ) );
        sec.add( r );
    }
}


int64_t anyI64( RNG& rng )
{
    uint64_t bits = rng.next();
    uint64_t shift = rng.next() % 64;
    int64_t  v = (int64_t) ( bits >> shift );
    bool     neg = rng.flag();
    return neg ? -v : v;
}


VECTOR2L anyVecL( RNG& rng )
{
    int64_t x = anyI64( rng );
    int64_t y = anyI64( rng );
    return VECTOR2L( x, y );
}


int anyI32( RNG& rng )
{
    // never INT_MIN
    uint64_t bits = rng.next();
    uint64_t shift = 33 + rng.next() % 31;
    int64_t  v = (int64_t) ( bits >> shift );
    bool     neg = rng.flag();
    return (int) ( neg ? -v : v );
}


VECTOR2I anyVecI( RNG& rng )
{
    int x = anyI32( rng );
    int y = anyI32( rng );
    return VECTOR2I( x, y );
}


void sectionRescale()
{
    SECTION sec( "rescale" );
    RNG     rng( 102 );

    for( int n = 0; n < 30000; n++ )
    {
        int64_t num = anyI64( rng );
        int64_t val = anyI64( rng );
        int64_t den = anyI64( rng );

        if( den == 0 )
            den = 1;

        REC r;
        r.i( num ).i( val ).i( den ).i( rescale( num, val, den ) );

        int inum = anyI32( rng );
        int ival = anyI32( rng );
        int iden = anyI32( rng );

        if( iden == 0 )
            iden = 1;

        r.i( inum ).i( ival ).i( iden ).i( rescale( inum, ival, iden ) );

        double dn = rng.dblScaled( -10, 40 );
        double dv = rng.dblScaled( -10, 40 );
        double dd = rng.dblScaled( -10, 40 );
        r.d( rescale( dn, dv, dd ) );
        r.i( sign( num ) ).i( sign( dn ) );
        sec.add( r );
    }
}


void sectionIsqrt()
{
    SECTION sec( "isqrt_hypot" );
    RNG     rng( 103 );

    for( int n = 0; n < 30000; n++ )
    {
        int64_t  x = anyI64( rng );
        uint64_t ubits = rng.next();
        uint64_t ushift = rng.next() % 64;
        uint64_t ux = ubits >> ushift;
        int64_t  hx = anyI64( rng );
        int64_t  hy = anyI64( rng );

        if( n % 7 == 0 )
        {
            // perfect squares and their neighbours
            uint64_t rbits = rng.next();
            uint64_t rshift = 32 + rng.next() % 32;
            uint64_t root = rbits >> rshift;
            ux = root * root + (uint64_t) rng.range( -1, 1 );
            x = (int64_t) ( ( root >> 1 ) * ( root >> 1 ) ) + rng.range( -1, 1 );
        }

        REC r;
        r.i( x ).i( isqrt( x ) ).u( ux ).u( isqrt( ux ) ).i( hx ).i( hy ).u( RoundedHypot( hx, hy ) );
        r.u( UnsignedAbs( x ) );
        sec.add( r );
    }
}


void sectionVector2I()
{
    SECTION sec( "vector2i" );
    RNG     rng( 104 );

    for( int n = 0; n < 30000; n++ )
    {
        VECTOR2I full = anyVecI( rng );
        VECTOR2I a = rng.pt();
        VECTOR2I b = rng.pt();

        REC r;
        r.v( full ).i( full.EuclideanNorm() ).i( full.SquaredEuclideanNorm() );
        r.v( a ).v( b ).i( a.Cross( b ) ).i( a.Dot( b ) ).i( a.SquaredDistance( b ) ).i( a * b );
        r.v( a.Perpendicular() ).v( a + b ).v( a - b ).v( -a );
        r.b( a < b ).b( a <= b ).b( a > b ).b( a >= b ).b( a == b ).i( LexicographicalCompare( a, b ) );
        r.v( LexicographicalMax( a, b ) ).v( LexicographicalMin( a, b ) );

        int len = rng.flag() ? (int) rng.range( -1000000000, 1000000000 ) : (int) rng.range( -100, 100 );
        r.i( len ).v( a.Resize( len ) );

        const double divs[] = { 2.0, -3.0, 0.5, 1.0, 7.0 };
        double       f = rng.flag() ? divs[rng.next() % 5] : rng.dbl( 0.1, 10.0 );
        r.d( f ).v( a / f );

        double sc = rng.dbl( -5.0, 5.0 );
        r.d( sc ).v( a + sc ).v( a - sc ).vd( a * sc );
        r.s( a.Format() ).s( streamStr( a ) );
        sec.add( r );
    }
}


void sectionVector2L()
{
    SECTION sec( "vector2l" );
    RNG     rng( 105 );

    for( int n = 0; n < 20000; n++ )
    {
        VECTOR2L full = anyVecL( rng );
        VECTOR2L a = rng.ptL( -1000000000LL, 1000000000LL );
        VECTOR2L b = rng.ptL( -1000000000LL, 1000000000LL );

        if( n % 5 == 0 )
            a = rng.ptL( -50, 50 );

        REC r;
        r.vl( full ).i( full.EuclideanNorm() ).i( full.SquaredEuclideanNorm() );
        r.v( VECTOR2I( full ) ).vd( VECTOR2D( full ) );
        r.vl( a ).vl( b ).i( a.Cross( b ) ).i( a.Dot( b ) ).i( a.SquaredDistance( b ) );

        int64_t len = rng.flag() ? rng.range( -3000000000LL, 3000000000LL ) : rng.range( -100, 100 );
        r.i( len ).vl( a.Resize( len ) );

        double f = rng.dbl( 0.1, 10.0 );
        r.d( f ).vl( a / f ).vl( full / f );

        // 128-bit helpers
        r.d( ToDouble( CrossWide( full, b ) ) ).d( ToDouble( DotWide( a, b ) ) );
        sec.add( r );
    }
}


void sectionVector2D()
{
    SECTION sec( "vector2d" );
    RNG     rng( 106 );

    for( int n = 0; n < 30000; n++ )
    {
        VECTOR2D a;

        switch( rng.next() % 4 )
        {
        case 0: a = rng.ptD( -1e9, 1e9 ); break;
        case 1: a = VECTOR2D( rng.pt() ); break;
        case 2: { double m = rng.dbl( -1e6, 1e6 ); a = VECTOR2D( m, rng.flag() ? m : -m ); break; }
        default: a = rng.ptDScaled( -30, 62 ); break;
        }

        VECTOR2D b = rng.ptD( -1e6, 1e6 );
        double   len = rng.flag() ? rng.dbl( -1e6, 1e6 ) : (double) rng.range( -10, 10 );

        REC r;
        r.vd( a ).d( a.SquaredEuclideanNorm() ).vd( a.Resize( len ) ).d( a.Cross( b ) ).d( a.Dot( b ) );
        r.d( a.SquaredDistance( b ) ).vd( a.Perpendicular() ).vd( a + b ).vd( a - b ).vd( a / len );
        r.v( VECTOR2I( a ) ).v( KiROUND( a ) );
        r.b( a < b ).b( a >= b );

        if( std::abs( a.x ) < 9e18 && std::abs( a.y ) < 9e18 )
            r.vl( VECTOR2L( a ) );

        r.s( a.Format() ).s( streamStr( a ) );
        r.b( equals( a, b ) ).b( equals( a, a + 1e-12 ) ).b( equals( a, b, 1e6 ) );

        // KiROUND( x, y ) and the mixed-type operators
        VECTOR2I ia = rng.pt();
        r.v( KiROUND( a.x / 16.0, a.y / 16.0 ) ).vd( ia + b ).vd( b - ia ).d( ia * b );
        sec.add( r );
    }
}


void sectionVector2DNorm()
{
    SECTION sec( "libm:vector2d_norm" );
    RNG     rng( 107 );

    for( int n = 0; n < 20000; n++ )
    {
        VECTOR2D a = rng.ptD( -1e9, 1e9 );

        if( n % 4 == 0 )
            a = rng.ptDScaled( -30, 62 );

        VECTOR2I p = rng.pt();
        VECTOR2I q = rng.pt();

        REC r;
        r.vd( a ).d( a.EuclideanNorm() ).v( p ).v( q ).d( p.Distance( q ) );
        sec.add( r );
    }
}


void sectionEdaAngle()
{
    SECTION sec( "eda_angle" );
    RNG     rng( 108 );

    const double steps[] = { 90.0, 45.0, 15.0, 0.0, -5.0, 0.1, 360.0 };

    for( int n = 0; n < 40000; n++ )
    {
        double    deg = rng.angleDeg();
        double    deg2 = rng.angleDeg();
        EDA_ANGLE a( deg, DEGREES_T );
        EDA_ANGLE b( deg2, DEGREES_T );

        REC r;
        r.d( deg );
        r.a( EDA_ANGLE( a ).Normalize() ).a( EDA_ANGLE( a ).NormalizeNegative() );
        r.a( EDA_ANGLE( a ).Normalize90() ).a( EDA_ANGLE( a ).Normalize180() );
        r.a( EDA_ANGLE( a ).Normalize720() ).a( a.Normalized() );
        r.b( a.IsCardinal() ).b( a.IsCardinal90() ).b( a.IsZero() ).b( a.IsHorizontal() ).b( a.IsVertical() );
        r.a( a.KeepUpright() ).i( a.AsTenthsOfADegree() ).d( a.AsRadians() );
        r.d( deg2 ).b( a.IsParallelTo( b ) ).a( a.Reflect( b ) ).a( a + b ).a( a - b ).a( -a );
        r.a( a * 0.5 ).a( a / 3.0 ).d( a / b ).b( a < b ).b( a == b ).a( std::abs( a ) );
        r.a( a.Snapped( EDA_ANGLE( steps[rng.next() % 7], DEGREES_T ) ) );
        r.a( EDA_ANGLE( deg, TENTHS_OF_A_DEGREE_T ) ).a( EDA_ANGLE( deg / 64.0, RADIANS_T ) );
        r.s( streamStr( a ) );

        // The exact cases of the vector constructor (axes and diagonals)
        int       m = (int) rng.range( -1000, 1000 );
        VECTOR2I  dirs[] = { { m, 0 }, { 0, m }, { m, m }, { m, -m }, { 0, 0 } };
        r.a( EDA_ANGLE( VECTOR2D( dirs[rng.next() % 5] ) ) );

        EDA_ORIENTATION o( a );
        r.a( o.GetAngle() ).a( ( o + b ).GetAngle() ).a( ( o - b ).GetAngle() ).a( ( -o ).GetAngle() );
        r.a( o.Reflect( b ).GetAngle() ).a( ( b - o ).GetAngle() ).a( o - EDA_ORIENTATION( b ) );

        // Sin/Cos are exact (no libm) for multiples of 45 degrees
        EDA_ANGLE c( 45.0 * (double) rng.range( -24, 24 ), DEGREES_T );
        r.a( c ).d( c.Sin() ).d( c.Cos() );
        sec.add( r );
    }
}


void sectionEdaAngleTrig()
{
    SECTION sec( "libm:eda_angle_trig" );
    RNG     rng( 109 );

    for( int n = 0; n < 20000; n++ )
    {
        double    deg = rng.angleDeg();
        EDA_ANGLE a( deg, DEGREES_T );
        VECTOR2D  vd = rng.ptD( -1e6, 1e6 );
        VECTOR2I  vi = rng.pt();
        double    x = rng.dbl( -1.0, 1.0 );

        REC r;
        r.d( deg ).d( a.Sin() ).d( a.Cos() ).d( a.Tan() );
        r.vd( vd ).a( EDA_ANGLE( vd ) ).v( vi ).a( EDA_ANGLE( VECTOR2D( vi ) ) );
        r.d( x ).a( EDA_ANGLE::Arccos( x ) ).a( EDA_ANGLE::Arcsin( x ) ).a( EDA_ANGLE::Arctan( x * 100.0 ) );
        r.a( EDA_ANGLE::Arctan2( vd.y, vd.x ) );
        r.a( a.Round( (int) rng.range( 0, 6 ) ) );
        sec.add( r );
    }
}


void sectionRotateExact()
{
    SECTION sec( "rotate_point_exact" );
    RNG     rng( 110 );

    for( int n = 0; n < 30000; n++ )
    {
        // Multiples of 45 degrees never reach libm
        EDA_ANGLE angle( 45.0 * (double) rng.range( -24, 24 ), DEGREES_T );
        VECTOR2I  p = rng.pt();
        VECTOR2I  c = rng.pt();
        VECTOR2D  pd = rng.ptD( -1e9, 1e9 );
        VECTOR2D  cd = rng.ptD( -1e9, 1e9 );

        VECTOR2I p1 = p;
        RotatePoint( p1, angle );
        VECTOR2I p2 = p;
        RotatePoint( p2, c, angle );
        VECTOR2D d1 = pd;
        RotatePoint( d1, angle );
        VECTOR2D d2 = pd;
        RotatePoint( d2, cd, angle );

        REC r;
        r.a( angle ).v( p ).v( c ).v( p1 ).v( p2 ).vd( pd ).vd( cd ).vd( d1 ).vd( d2 );
        r.v( GetRotated( p, angle ) ).v( GetRotated( p, c, angle ) ).vd( GetRotated( pd, angle ) );
        sec.add( r );
    }
}


void sectionRotateLibm()
{
    SECTION sec( "libm:rotate_point" );
    RNG     rng( 111 );

    for( int n = 0; n < 30000; n++ )
    {
        EDA_ANGLE angle( rng.angleDeg(), DEGREES_T );
        VECTOR2I  p = rng.pt();
        VECTOR2I  c = rng.pt();
        VECTOR2D  pd = rng.ptD( -1e9, 1e9 );
        VECTOR2D  cd = rng.ptD( -1e9, 1e9 );

        VECTOR2I p1 = p;
        RotatePoint( p1, angle );
        VECTOR2I p2 = p;
        RotatePoint( p2, c, angle );
        VECTOR2D d1 = pd;
        RotatePoint( d1, angle );
        VECTOR2D d2 = pd;
        RotatePoint( d2, cd, angle );

        REC r;
        r.a( angle ).v( p ).v( c ).v( p1 ).v( p2 ).vd( pd ).vd( cd ).vd( d1 ).vd( d2 );
        sec.add( r );
    }
}


void arcPoints( RNG& rng, VECTOR2I& s, VECTOR2I& m, VECTOR2I& e )
{
    switch( rng.next() % 8 )
    {
    case 0:
    {
        // axis-aligned points of a circle
        VECTOR2I c = rng.ptMid();
        int      rad = (int) rng.range( 1, 100000000 );
        VECTOR2I q[] = { c + VECTOR2I( rad, 0 ), c + VECTOR2I( 0, rad ), c + VECTOR2I( -rad, 0 ),
                         c + VECTOR2I( 0, -rad ) };
        int      k = (int) ( rng.next() % 4 );
        s = q[k];
        m = q[( k + 1 ) % 4];
        e = q[( k + 2 + rng.next() % 2 ) % 4];
        break;
    }
    case 1:
    {
        // collinear
        VECTOR2I a = rng.ptMid();
        VECTOR2I d = rng.ptR( -1000, 1000 );
        s = a;
        m = a + d * (int) rng.range( -1000, 1000 );
        e = a + d * (int) rng.range( -1000, 1000 );
        break;
    }
    case 2:
    {
        // nearly coincident
        VECTOR2I a = rng.ptMid();
        s = a;
        m = a + rng.ptR( -6, 6 );

        if( rng.flag() )
            e = a + rng.ptR( -6, 6 );
        else
            e = rng.ptMid();

        break;
    }
    case 3:
    {
        // full circle
        s = rng.ptMid();
        m = rng.ptMid();
        e = s;
        break;
    }
    default:
        s = rng.pt();
        m = rng.pt();
        e = rng.pt();
        break;
    }
}


void sectionArcCenter()
{
    SECTION sec( "libm:arc_center" );
    RNG     rng( 112 );

    for( int n = 0; n < 30000; n++ )
    {
        VECTOR2I s, m, e;
        arcPoints( rng, s, m, e );

        VECTOR2D ds( s ), dm( m ), de( e );

        if( rng.flag() )
        {
            ds += rng.ptD( -0.5, 0.5 );
            dm += rng.ptD( -0.5, 0.5 );
        }

        EDA_ANGLE angle( rng.angleDeg(), DEGREES_T );

        REC r;
        r.v( s ).v( m ).v( e ).v( CalcArcCenter( s, m, e ) ).b( IsStraightArc( s, m, e ) );
        r.vd( ds ).vd( dm ).vd( de ).vd( CalcArcCenter( ds, dm, de ) );
        r.a( angle ).vd( CalcArcCenter( ds, de, angle ) );

        VECTOR2I c = rng.ptMid();
        VECTOR2I as = rng.ptMid();
        VECTOR2I ae = rng.ptMid();
        r.v( c ).v( as ).v( ae ).v( CalcArcMid( as, ae, c, true ) ).v( CalcArcMid( as, ae, c, false ) );
        sec.add( r );
    }
}


void segPair( RNG& rng, SEG& a, SEG& b )
{
    a = rng.seg();

    auto inRange = []( int64_t v ) { return v >= -500000000 && v <= 500000000; };

    switch( rng.next() % 8 )
    {
    case 4:
    {
        VECTOR2I shared = rng.flag() ? a.A : a.B;
        VECTOR2I other = rng.pt();
        b = SEG( shared, other );
        return;
    }

    case 5:
    {
        // collinear
        int64_t dx = (int64_t) a.B.x - a.A.x;
        int64_t dy = (int64_t) a.B.y - a.A.y;
        int64_t k1 = rng.range( -3, 3 );
        int64_t k2 = rng.range( -3, 3 );
        int64_t x1 = a.A.x + dx * k1, y1 = a.A.y + dy * k1;
        int64_t x2 = a.A.x + dx * k2, y2 = a.A.y + dy * k2;

        if( inRange( x1 ) && inRange( y1 ) && inRange( x2 ) && inRange( y2 ) )
        {
            b = SEG( VECTOR2I( (int) x1, (int) y1 ), VECTOR2I( (int) x2, (int) y2 ) );
            return;
        }

        break;
    }

    case 6:
    {
        // parallel, slightly offset
        int64_t ox = rng.range( -5, 5 );
        int64_t oy = rng.range( -5, 5 );
        int64_t x1 = a.A.x + ox, y1 = a.A.y + oy;
        int64_t x2 = a.B.x + ox, y2 = a.B.y + oy;

        if( inRange( x1 ) && inRange( y1 ) && inRange( x2 ) && inRange( y2 ) )
        {
            b = SEG( VECTOR2I( (int) x1, (int) y1 ), VECTOR2I( (int) x2, (int) y2 ) );
            return;
        }

        break;
    }

    case 7:
    {
        // zero length
        VECTOR2I p;

        switch( rng.next() % 3 )
        {
        case 0: p = a.A; break;
        case 1: p = VECTOR2I( (int) ( ( (int64_t) a.A.x + a.B.x ) / 2 ), (int) ( ( (int64_t) a.A.y + a.B.y ) / 2 ) ); break;
        default: p = rng.pt(); break;
        }

        b = SEG( p, p );
        return;
    }

    default:
        break;
    }

    b = rng.seg();
}


void sectionTrigoMisc()
{
    SECTION sec( "trigo_misc" );
    RNG     rng( 113 );

    for( int n = 0; n < 40000; n++ )
    {
        SEG a, b;
        segPair( rng, a, b );
        VECTOR2I p;

        if( rng.flag() )
        {
            p = rng.pt();
        }
        else
        {
            VECTOR2I near = a.NearestPoint( rng.pt() );
            p = near + rng.ptR( -2, 2 );
        }

        int      dist = rng.flag() ? (int) rng.range( 0, 20 ) : (int) rng.range( 0, 1000000 );

        REC r;
        r.seg( a ).seg( b ).v( p ).i( dist );
        r.b( IsPointOnSegment( a.A, a.B, p ) ).b( TestSegmentHit( p, a.A, a.B, dist ) );

        bool hit = SegmentIntersectsSegment( a.A, a.B, b.A, b.B );
        r.b( hit );

        if( hit )
        {
            VECTOR2I ip;
            bool     hit2 = SegmentIntersectsSegment( a.A, a.B, b.A, b.B, &ip );
            r.b( hit2 ).v( ip );
        }

        r.i( NormalizeAnglePos( (int) rng.range( -20000, 20000 ) ) );
        r.i( NormalizeAngle180( (int) rng.range( -20000, 20000 ) ) );
        r.d( NormalizeAnglePos( rng.dbl( -20000.0, 20000.0 ) ) );
        r.d( NormalizeAngle180( rng.dbl( -20000.0, 20000.0 ) ) );

        double a1 = rng.dbl( 0.0, 360.0 );
        double a2 = rng.dbl( 0.0, 360.0 );
        r.b( InterceptsPositiveX( a1, a2 ) ).b( InterceptsNegativeX( a1, a2 ) );
        r.d( DEG2RAD( a1 ) ).d( RAD2DEG( a1 ) ).d( RAD2DECIDEG( a1 ) );
        sec.add( r );
    }
}


void sectionSegPoint()
{
    SECTION sec( "seg_point" );
    RNG     rng( 114 );

    for( int n = 0; n < 60000; n++ )
    {
        SEG a = rng.seg();

        if( n % 16 == 0 )
            a.B = a.A;

        VECTOR2I p;

        switch( rng.next() % 4 )
        {
        case 0:
        {
            VECTOR2I near = a.NearestPoint( rng.pt() );
            p = near + rng.ptR( -3, 3 );
            break;
        }
        case 1: p = rng.flag() ? a.A : a.B; break;
        default: p = rng.pt(); break;
        }

        REC r;
        r.seg( a ).v( p );
        r.i( a.SquaredDistance( p ) ).i( a.Distance( p ) ).v( a.NearestPoint( p ) ).v( a.LineProject( p ) );
        r.v( a.ReflectPoint( p ) ).i( a.LineDistance( p ) ).i( a.LineDistance( p, true ) ).i( a.Side( p ) );
        r.b( a.Contains( p ) ).b( a.Contains( p, 0 ) ).b( a.Contains( p, 100 ) ).i( a.TCoef( p ) );
        r.i( a.Length() ).i( a.SquaredLength() ).v( a.Center() );
        r.seg( a.PerpendicularSeg( p ) ).seg( a.ParallelSeg( p ) ).seg( a.Reversed() );

        SEG::ecoord qa, qb, qc;
        a.CanonicalCoefs( qa, qb, qc );
        r.i( qa ).i( qb ).i( qc ).i( SEG::Square( p.x ) ).s( streamStr( a ) );
        sec.add( r );
    }
}


void sectionSegSeg()
{
    SECTION sec( "seg_seg" );
    RNG     rng( 115 );

    for( int n = 0; n < 80000; n++ )
    {
        SEG a, b;
        segPair( rng, a, b );

        if( n % 32 == 0 )
            a.B = a.A;

        int clearance = rng.flag() ? (int) rng.range( -2, 50 ) : (int) rng.range( 0, 100000000 );
        int thr = (int) rng.range( 0, 10 );

        REC r;
        r.seg( a ).seg( b ).i( clearance ).i( thr );

        int  actual = -12345;
        bool coll = a.Collide( b, clearance, &actual );
        r.b( coll ).i( actual ).b( a.Collide( b, clearance ) );
        r.i( a.SquaredDistance( b ) ).i( a.Distance( b ) ).v( a.NearestPoint( b ) );

        VECTOR2I pa, pb;
        int64_t  dsq = 0;
        r.b( a.NearestPoints( b, pa, pb, dsq ) ).v( pa ).v( pb ).i( dsq );

        r.opt( a.Intersect( b ) ).opt( a.Intersect( b, true, false ) ).opt( a.Intersect( b, false, true ) );
        r.opt( a.Intersect( b, true, true ) ).opt( a.IntersectLines( b ) ).b( a.Intersects( b ) );
        r.b( a.Collinear( b ) ).b( a.ApproxCollinear( b ) ).b( a.ApproxCollinear( b, thr ) );
        r.b( a.ApproxParallel( b ) ).b( a.ApproxParallel( b, thr ) ).b( a.ApproxPerpendicular( b ) );
        r.b( a.Overlaps( b ) ).b( a.ApproxOverlaps( b ) ).b( a.ApproxOverlaps( b, thr ) ).b( a.Contains( b ) );
        r.b( a == b ).b( a != b ).b( a < b );
        sec.add( r );
    }
}


void sectionSegAngle()
{
    SECTION sec( "libm:seg_angle" );
    RNG     rng( 116 );

    for( int n = 0; n < 20000; n++ )
    {
        SEG a, b;
        segPair( rng, a, b );

        REC r;
        r.seg( a ).seg( b ).a( a.Angle( b ) );
        sec.add( r );
    }
}


void sectionSegLine()
{
    SECTION sec( "seg_line" );
    RNG     rng( 117 );

    const double slopes[] = { 0.0, 1.0, -1.0, 0.5, 2.0, 1000.0, -0.001 };

    for( int n = 0; n < 40000; n++ )
    {
        SEG a = rng.seg();

        if( n % 5 == 0 )
            a.B.x = a.A.x;

        if( n % 11 == 0 )
            a.B = a.A;

        double slope = rng.flag() ? slopes[rng.next() % 7] : rng.dbl( -5.0, 5.0 );
        double offset;

        switch( rng.next() % 3 )
        {
        case 0: offset = rng.dbl( -1e9, 1e9 ); break;
        case 1: offset = (double) a.A.y - slope * (double) a.A.x + (double) rng.range( -2, 2 ); break;
        default: offset = (double) rng.range( -1000, 1000 ); break;
        }

        VECTOR2I ip( -777, -777 );
        bool     hit = a.IntersectsLine( slope, offset, ip );

        REC r;
        r.seg( a ).d( slope ).d( offset ).b( hit ).v( ip );
        sec.add( r );
    }
}


BOX2I genBox( RNG& rng )
{
    VECTOR2I pos = rng.ptMid();
    VECTOR2L size;

    switch( rng.next() % 4 )
    {
    case 0: size = rng.ptL( -50, 50 ); break;
    case 1: size = rng.ptL( 0, 200000 ); break;
    case 2: size = rng.ptL( -200000000, 200000000 ); break;
    default: size = rng.ptL( 0, 200000000 ); break;
    }

    return BOX2I( pos, size );
}


void sectionBox2I()
{
    SECTION sec( "box2i" );
    RNG     rng( 118 );

    for( int n = 0; n < 60000; n++ )
    {
        BOX2I a = genBox( rng );
        BOX2I b = genBox( rng );

        if( n % 6 == 0 )
        {
            // b near a
            b = a;
            b.Move( rng.ptR( -100, 100 ) );
            VECTOR2I infl = rng.ptR( -60, 60 );
            b.Inflate( infl.x, infl.y );
        }

        if( n % 9 == 0 )
        {
            // raw (non-normalized) size
            b.SetSize( rng.ptL( -1000, 1000 ) );
        }

        VECTOR2I p;

        if( rng.flag() )
            p = rng.ptMid();
        else
            p = a.GetOrigin() + rng.ptR( -10, 200010 );

        VECTOR2I q = rng.ptMid();

        REC r;
        r.box( a ).box( b ).v( p ).v( q );
        r.v( a.Centre() ).v( a.GetEnd() ).i( a.GetRight() ).i( a.GetBottom() ).i( a.GetSizeMax() );
        r.i( a.GetArea() ).i( a.Diagonal() ).i( a.SquaredDiagonal() );
        r.b( a.Contains( p ) ).b( a.Contains( b ) ).b( b.Contains( p ) ).b( a.Intersects( b ) ).b( b.Intersects( a ) );
        r.box( a.Intersect( b ) ).b( a.Intersects( p, q ) ).b( b.Intersects( p, q ) );
        r.i( a.SquaredDistance( p ) ).i( a.Distance( p ) ).i( a.SquaredDistance( b ) ).i( a.Distance( b ) );
        r.i( b.SquaredDistance( p ) ).i( b.SquaredDistance( a ) );
        r.v( a.NearestPoint( p ) ).v( a.FarthestPointTo( p ) ).v( b.NearestPoint( p ) ).v( b.FarthestPointTo( p ) );

        int radius = (int) rng.range( 0, 300000 );
        int width = (int) rng.range( 0, 1000 );
        r.i( radius ).i( width ).b( a.IntersectsCircle( p, radius ) ).b( a.IntersectsCircleEdge( p, radius, width ) );

        int dx = 0, dy = 0;

        switch( rng.next() % 3 )
        {
        case 0: dx = (int) rng.range( -100, 100 ); dy = (int) rng.range( -100, 100 ); break;
        case 1: dx = (int) rng.range( -300000000, 300000000 ); dy = (int) rng.range( -300000000, 300000000 ); break;
        default: dx = anyI32( rng ); dy = anyI32( rng ); break;
        }

        r.i( dx ).i( dy ).box( a.GetInflated( dx, dy ) ).box( b.GetInflated( dx ) );

        BOX2I m1 = a;
        m1.Merge( b );
        BOX2I m2 = b;
        m2.Merge( p );
        BOX2I m3;
        m3.Merge( b );
        BOX2I m4;
        m4.Merge( p );
        r.box( m1 ).box( m2 ).box( m3 ).box( m4 );

        r.b( a == b ).b( a != b ).box( BOX2I::ByCorners( p, q ) ).box( BOX2I::ByCenter( p, a.GetSize() ) );
        r.box( a.GetWithOffset( q ) ).b( IsBOX2Safe( a ) ).s( a.Format() );

        // exact rotations (multiples of 45 degrees stay clear of libm)
        EDA_ANGLE angle( 45.0 * (double) rng.range( -8, 8 ), DEGREES_T );
        r.a( angle ).b( a.Intersects( b, angle ) ).box( a.GetBoundingBoxRotated( q, angle ) );

        std::vector<VECTOR2I> pts = { p, q, a.GetOrigin(), b.GetEnd() };
        BOX2I                 c;
        c.Compute( pts );
        r.box( c );
        sec.add( r );
    }
}


void sectionBox2IRotated()
{
    SECTION sec( "libm:box2i_rotated" );
    RNG     rng( 119 );

    for( int n = 0; n < 30000; n++ )
    {
        BOX2I a = genBox( rng );
        BOX2I b = genBox( rng );

        if( n % 3 == 0 )
        {
            b = a;
            b.Move( rng.ptR( -100000, 100000 ) );
        }

        EDA_ANGLE angle( rng.angleDeg(), DEGREES_T );
        VECTOR2I  c = rng.ptMid();

        REC r;
        r.box( a ).box( b ).a( angle ).v( c );
        r.b( a.Intersects( b, angle ) ).box( a.GetBoundingBoxRotated( c, angle ) );
        sec.add( r );
    }
}


void sectionBox2D()
{
    SECTION sec( "box2d" );
    RNG     rng( 120 );

    for( int n = 0; n < 30000; n++ )
    {
        VECTOR2D apos = rng.ptD( -1e6, 1e6 );
        VECTOR2D asize = rng.ptD( -1e6, 1e6 );
        VECTOR2D bpos = rng.ptD( -1e6, 1e6 );
        VECTOR2D bsize = rng.ptD( 0.0, 1e6 );
        BOX2D    a( apos, asize );
        BOX2D    b( bpos, bsize );
        VECTOR2D p = rng.ptD( -2e6, 2e6 );
        double   d = rng.dbl( -1e5, 1e5 );

        REC r;
        r.boxd( a ).boxd( b ).vd( p ).d( d );
        r.vd( a.Centre() ).vd( a.GetEnd() ).d( a.GetArea() ).d( a.SquaredDiagonal() );
        r.b( a.Contains( p ) ).b( a.Contains( b ) ).b( a.Intersects( b ) ).boxd( a.Intersect( b ) );
        r.d( a.SquaredDistance( p ) ).d( a.Distance( p ) ).d( a.SquaredDistance( b ) ).d( a.Distance( b ) );
        r.vd( a.NearestPoint( p ) ).vd( a.FarthestPointTo( p ) ).boxd( a.GetInflated( d ) );

        BOX2D m1 = a;
        m1.Merge( b );
        BOX2D m2 = b;
        m2.Merge( p );
        r.boxd( m1 ).boxd( m2 ).b( a == b ).i( a.GetSizeMax() );
        int radius = (int) rng.range( 0, 1000000 );
        int width = (int) rng.range( 0, 1000 );
        r.b( a.IntersectsCircle( p, radius ) ).b( a.IntersectsCircleEdge( p, radius, width ) );
        r.box( BOX2ISafe( a ) ).box( KiROUND( a ) ).box( BOX2ISafe( p * 4096.0, a.GetSize() * 4096.0 ) );
        r.b( IsBOX2Safe( a ) ).s( a.Format() );
        sec.add( r );
    }
}


void sectionVectorUtils()
{
    SECTION sec( "vector_utils" );
    RNG     rng( 121 );

    for( int n = 0; n < 30000; n++ )
    {
        SEG      a = rng.seg();
        VECTOR2I p = rng.flag() ? rng.pt() : a.NearestPoint( rng.pt() );
        VECTOR2I dir = rng.pt();
        int      grid = (int) rng.range( 1, 1000 );
        VECTOR2I g = rng.ptR( -1000000, 1000000 );

        REC r;
        r.seg( a ).v( p ).v( dir ).i( grid ).v( g );
        r.b( KIGEOM::PointIsInDirection( p, dir, a.A ) ).b( KIGEOM::PointsAreInSameDirection( p, a.B, a.A ) );
        r.b( KIGEOM::SegIsInDirection( a, dir ) ).b( KIGEOM::PointProjectsOntoSegment( p, a ) );
        r.d( KIGEOM::GetLengthRatioFromStart( p, a ) ).d( KIGEOM::GetProjectedPointLengthRatio( p, a ) );
        r.v( KIGEOM::RoundGrid( g, grid ) ).v( KIGEOM::RoundNW( g, grid ) ).v( KIGEOM::RoundSE( g, grid ) );
        sec.add( r );
    }
}


void sectionNearestEndpoint()
{
    SECTION sec( "libm:nearest_endpoint" );
    RNG     rng( 122 );

    for( int n = 0; n < 20000; n++ )
    {
        SEG      a = rng.seg();
        VECTOR2I p = rng.pt();

        REC r;
        r.seg( a ).v( p ).v( KIGEOM::GetNearestEndpoint( a, p ) );
        sec.add( r );
    }
}


// VECTOR2<float> and VECTOR2<unsigned> are only instantiated by the C++ unit tests, but the
// port supports them, so check them as well.
void sectionVector2FloatUnsigned()
{
    SECTION sec( "vector2_float_unsigned" );
    RNG     rng( 123 );

    for( int n = 0; n < 20000; n++ )
    {
        float fx = (float) rng.dbl( -1e6, 1e6 );
        float fy = (float) rng.dbl( -1e6, 1e6 );

        if( n % 4 == 0 )
        {
            fx = (float) rng.range( -100, 100 );
            fy = rng.flag() ? fx : -fx;
        }

        VECTOR2<float> f( fx, fy );
        float          flen = (float) rng.dbl( -1e3, 1e3 );

        REC r;
        r.vf( f ).d( flen ).d( f.SquaredEuclideanNorm() ).vf( f.Resize( flen ) ).d( f.Cross( f.Perpendicular() ) );
        r.d( f.Dot( f ) ).v( VECTOR2I( f ) ).vl( VECTOR2L( f ) ).vd( VECTOR2D( f ) ).vu( VECTOR2<unsigned>( f ) );
        r.vf( f - 1 ).vf( f - 5.0 ).vf( f + 1u ).vf( f / 3.0 ).vf( -f ).s( f.Format() );

        unsigned ux = (unsigned) rng.next();
        unsigned uy = (unsigned) rng.next();

        if( n % 3 == 0 )
        {
            ux = (unsigned) rng.range( 0, 10 );
            uy = (unsigned) rng.range( 0, 10 );
        }

        VECTOR2<unsigned> u( ux, uy );
        VECTOR2<unsigned> u2 = u - 1;
        VECTOR2I          anyi = anyVecI( rng );

        r.vu( u ).vu( u2 ).vu( u + 7u ).vu( u - 5.0 ).vu( u * 3u ).vu( -u ).vu( u.Perpendicular() ).vu( u / 2.0 );
        r.u( u.SquaredEuclideanNorm() ).u( u.Cross( u2 ) ).u( u.Dot( u2 ) ).u( u.SquaredDistance( u2 ) );
        r.v( VECTOR2I( u ) ).vl( VECTOR2L( u ) ).vd( VECTOR2D( u ) ).vf( VECTOR2<float>( u ) );
        r.v( anyi ).vu( VECTOR2<unsigned>( anyi ) ).vf( VECTOR2<float>( anyi ) ).s( u.Format() );
        r.b( u < u2 ).b( u == u2 );
        sec.add( r );
    }
}


void sectionVector2FloatNorm()
{
    SECTION sec( "libm:vector2f_norm" );
    RNG     rng( 124 );

    for( int n = 0; n < 10000; n++ )
    {
        float fx = (float) rng.dbl( -1e6, 1e6 );
        float fy = (float) rng.dbl( -1e6, 1e6 );

        if( n % 4 == 0 )
        {
            fx = (float) rng.range( -100, 100 );
            fy = rng.flag() ? fx : ( rng.flag() ? -fx : 0.0f );
        }

        VECTOR2<float> f( fx, fy );

        REC r;
        r.vf( f ).d( f.EuclideanNorm() );
        sec.add( r );
    }
}


void sectionBox2L()
{
    SECTION sec( "box2l" );
    RNG     rng( 125 );

    for( int n = 0; n < 20000; n++ )
    {
        // A large common offset makes every coordinate need more than 32 bits
        int64_t  k = rng.range( -1000000000000LL, 1000000000000LL );
        VECTOR2L koff( k, k );
        VECTOR2L apos = rng.ptL( -500000000, 500000000 ) + koff;
        VECTOR2L asize = rng.ptL( -500000000, 500000000 );
        VECTOR2L bpos = rng.ptL( -500000000, 500000000 ) + koff;
        VECTOR2L bsize = rng.ptL( 0, 500000000 );
        BOX2L    a( apos, asize );
        BOX2L    b( bpos, bsize );
        VECTOR2L p = rng.ptL( -1000000000, 1000000000 ) + koff;
        int64_t  d = rng.range( -3000000000LL, 3000000000LL );

        REC r;
        r.boxl( a ).boxl( b ).vl( p ).i( d );
        r.vl( a.Centre() ).vl( a.GetEnd() ).i( a.GetArea() ).i( a.Diagonal() ).i( a.SquaredDiagonal() );
        r.i( a.GetSizeMax() ).b( a.Contains( p ) ).b( a.Contains( b ) ).b( a.Intersects( b ) ).boxl( a.Intersect( b ) );
        r.i( a.SquaredDistance( p ) ).i( a.Distance( p ) ).i( a.SquaredDistance( b ) ).i( a.Distance( b ) );
        r.vl( a.NearestPoint( p ) ).vl( a.FarthestPointTo( p ) ).boxl( a.GetInflated( d ) ).boxl( b.GetInflated( d, -d ) );

        BOX2L m1 = a;
        m1.Merge( b );
        BOX2L m2 = b;
        m2.Merge( p );
        r.boxl( m1 ).boxl( m2 ).b( a == b ).b( IsBOX2Safe( a ) ).s( a.Format() );
        r.boxl( BOX2L::ByCorners( apos, p ) ).boxl( BOX2L::ByCenter( p, bsize ) );
        sec.add( r );
    }

    BOX2I maxI;
    maxI.SetMaximum();
    BOX2L maxL;
    maxL.SetMaximum();
    BOX2D maxD;
    maxD.SetMaximum();

    REC r;
    r.box( maxI ).boxl( maxL ).boxd( maxD );
    r.v( maxI.GetEnd() ).v( maxI.Centre() ).b( maxI.Contains( VECTOR2I( 123456789, -987654321 ) ) );
    sec.add( r );
}

} // namespace


int main( int argc, char** argv )
{
    if( argc == 3 && std::string( argv[1] ) == "--dump" )
        g_dump = fopen( argv[2], "w" );

    sectionLibmProbe();
    sectionKiRound();
    sectionRescale();
    sectionIsqrt();
    sectionVector2I();
    sectionVector2L();
    sectionVector2D();
    sectionVector2DNorm();
    sectionEdaAngle();
    sectionEdaAngleTrig();
    sectionRotateExact();
    sectionRotateLibm();
    sectionArcCenter();
    sectionTrigoMisc();
    sectionSegPoint();
    sectionSegSeg();
    sectionSegAngle();
    sectionSegLine();
    sectionBox2I();
    sectionBox2IRotated();
    sectionBox2D();
    sectionVectorUtils();
    sectionNearestEndpoint();
    sectionVector2FloatUnsigned();
    sectionVector2FloatNorm();
    sectionBox2L();

    if( g_dump )
        fclose( g_dump );

    return 0;
}
