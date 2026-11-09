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
 * Shared infrastructure of the kicad-geom differential oracles: the deterministic random
 * generator, the record / section hashing and a few input generators.  Copied verbatim from
 * geom_oracle.cpp (which keeps its own copy so that the package G1 golden file stays stable);
 * the Rust side of both is tests/oracle/sections.rs.
 */

#pragma once

#include <cinttypes>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

#include <geometry/eda_angle.h>
#include <geometry/seg.h>
#include <math/box2.h>
#include <math/vector2d.h>

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

} // namespace
