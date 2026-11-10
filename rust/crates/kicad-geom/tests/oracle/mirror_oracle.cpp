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
 * Differential-test oracle for the mirror helpers of kicad-geom (package CLEAN-1): MIRROR()
 * of include/core/mirror.h on VECTOR2I, SHAPE_LINE_CHAIN::Mirror and SHAPE_ARC::Mirror of
 * libkimath.a, with coordinates whose mirror image is outside the int range (the reference
 * build wraps).
 *
 * Output: "m|<case>|..." for the first VERBOSE_CASES cases, then one "hash|<first>|<fnv>"
 * line per BATCH cases.  Replayed by tests/oracle_mirror.rs; built by
 * build_mirror_oracle.sh.
 */

#include <climits>
#include <cstdint>
#include <cstdio>
#include <string>

#include <core/mirror.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_line_chain.h>

static const int CASES = 4000;
static const int BATCH = 100;
static const int VERBOSE_CASES = 40;

struct RNG // splitmix64
{
    uint64_t s;
    uint64_t next()
    {
        s += 0x9E3779B97F4A7C15ull;
        uint64_t z = s;
        z = ( z ^ ( z >> 30 ) ) * 0xBF58476D1CE4E5B9ull;
        z = ( z ^ ( z >> 27 ) ) * 0x94D049BB133111EBull;
        return z ^ ( z >> 31 );
    }
    int64_t below( int64_t n ) { return (int64_t) ( next() % (uint64_t) n ); }
};

static RNG g_rng{ 4242 };

static int coord()
{
    static const int TABLE[] = { INT_MAX, INT_MIN, INT_MAX - 7, INT_MIN + 7, 2000000000,
                                 -2000000000, 1100000000, -1100000000, 0, 1, -1 };
    int64_t r = g_rng.below( 4 );

    if( r == 0 )
        return TABLE[g_rng.below( 11 )];

    if( r == 1 )
        return (int) ( g_rng.below( 4294967296LL ) - 2147483648LL );

    return (int) ( g_rng.below( 2000001 ) - 1000000 );
}

static std::string g_line;

static void out( int v )
{
    g_line += " " + std::to_string( v );
}

static void outV( const VECTOR2I& v )
{
    out( v.x );
    out( v.y );
}

int main()
{
    uint64_t hash = 0xcbf29ce484222325ull;

    for( int c = 0; c < CASES; c++ )
    {
        VECTOR2I p[3];

        for( VECTOR2I& v : p )
        {
            int x = coord();
            int y = coord();
            v = VECTOR2I( x, y );
        }

        int            rx = coord();
        int            ry = coord();
        VECTOR2I       ref( rx, ry );
        FLIP_DIRECTION dir = g_rng.below( 2 ) ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM;
        bool           withArc = g_rng.below( 4 ) == 0;

        g_line.clear();

        VECTOR2I m = p[0];
        MIRROR( m, ref, dir );
        outV( m );

        SHAPE_LINE_CHAIN chain;

        for( const VECTOR2I& v : p )
            chain.Append( v, true );

        chain.Mirror( ref, dir );

        for( int i = 0; i < chain.PointCount(); i++ )
            outV( chain.CPoint( i ) );

        if( withArc )
        {
            // The arc constructor and update_values() compute the centre from the points:
            // keep them (and their mirror images) inside the int range, only the mirror
            // arithmetic is under test here.
            VECTOR2I a[3];

            for( VECTOR2I& v : a )
            {
                int x = (int) ( g_rng.below( 2000001 ) - 1000000 );
                int y = (int) ( g_rng.below( 2000001 ) - 1000000 );
                v = VECTOR2I( x, y );
            }

            VECTOR2I  aref( rx / 4, ry / 4 );
            SHAPE_ARC arc( a[0], a[1], a[2], 0 );
            arc.Mirror( aref, dir );
            outV( arc.GetP0() );
            outV( arc.GetArcMid() );
            outV( arc.GetP1() );
        }

        if( c < VERBOSE_CASES )
            printf( "m|%d|%s\n", c, g_line.c_str() + 1 );

        for( unsigned char ch : g_line )
        {
            hash ^= ch;
            hash *= 0x100000001b3ull;
        }

        hash ^= '\n';
        hash *= 0x100000001b3ull;

        if( c % BATCH == BATCH - 1 )
        {
            printf( "hash|%d|%016llx\n", c + 1 - BATCH, (unsigned long long) hash );
            hash = 0xcbf29ce484222325ull;
        }
    }

    return 0;
}
