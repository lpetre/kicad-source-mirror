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
 * Differential-test oracle for the polygon triangulation (package G4b): POLYGON_TRIANGULATION
 * (single chains, polygons with holes, partition fractions), SHAPE_POLY_SET::CacheTriangulation
 * (incl. the fallbacks, the partition path with and without a task submitter, re-caching after
 * Move / Rotate / Mirror), TRIANGULATED_POLYGON::Refine, POLY_YSTRIPES_INDEX, the triangle-based
 * SHAPE_POLY_SET::Collide( SHAPE* ) and AnyTriangleHit, and SEG::IntersectsLine with out-of-range
 * slopes (GAP-P4).  Links libkimath.a of the reference build.  Output format and hashing as in
 * the other oracles (oracle_common.h); the Rust side is tri_sections.rs.
 */

#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>
#include <memory>
#include <string>
#include <vector>

#include <geometry/shape_line_chain.h>

#define private public
#define protected public
#include <geometry/shape_poly_set.h>
#include <geometry/polygon_triangulation.h>
#include <geometry/shape_poly_tri_index.h>
#undef protected
#undef private

#include <geometry/poly_ystripes_index.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_compound.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <math/box2.h>
#include <math/util.h>
#include <math/vector2d.h>

#include <wx/debug.h>

#include "oracle_common.h"

namespace
{

typedef SHAPE_POLY_SET::TRIANGULATED_POLYGON TPOLY;

void recTri( REC& r, const TPOLY& t )
{
    r.i( t.GetSourceOutlineIndex() ).u( t.GetVertexCount() );

    for( const VECTOR2I& v : t.Vertices() )
        r.v( v );

    r.u( t.GetTriangleCount() );

    for( const TPOLY::TRI& tri : t.Triangles() )
        r.i( tri.a ).i( tri.b ).i( tri.c );
}


void recCache( REC& r, const SHAPE_POLY_SET& s )
{
    r.b( s.IsTriangulationUpToDate() ).u( s.TriangulatedPolyCount() );

    for( unsigned i = 0; i < s.TriangulatedPolyCount(); i++ )
        recTri( r, *s.TriangulatedPolygon( i ) );

    r.b( s.TriangulationIndex() != nullptr );
}


int scaleOf( RNG& rng )
{
    static const int k[] = { 60, 1000, 100000, 10000000, 400000000 };
    return k[rng.next() % 5];
}


VECTOR2I gridPt( RNG& rng, int aMode )
{
    switch( aMode )
    {
    case 0: { int x = (int) rng.range( -3, 3 ); int y = (int) rng.range( -3, 3 ); return VECTOR2I( x * 1000, y * 1000 ); }
    case 1: return rng.ptR( -20, 20 );
    case 2: return rng.ptR( -1000000, 1000000 );
    default: return rng.ptR( -50000000, 50000000 );
    }
}


SHAPE_LINE_CHAIN ring( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    int n = (int) rng.range( 3, 12 );
    int mode = (int) rng.range( 0, 3 );

    for( int i = 0; i < n; i++ )
        c.Append( gridPt( rng, mode ) );

    c.SetClosed( true );
    return c;
}


// An x-monotone polygon (simple unless xs repeat)
SHAPE_LINE_CHAIN monotone( RNG& rng, int k, int S, bool aDup )
{
    std::vector<int> xs, up, lo;

    for( int i = 0; i < k; i++ )
        xs.push_back( (int) rng.range( -S, S ) );

    std::sort( xs.begin(), xs.end() );

    for( int i = 0; i < k; i++ )
        up.push_back( (int) rng.range( 0, S ) );

    for( int i = 0; i < k; i++ )
        lo.push_back( (int) rng.range( -S, 0 ) );

    SHAPE_LINE_CHAIN c;

    for( int i = 0; i < k; i++ )
        c.Append( xs[i], up[i], aDup );

    for( int i = k - 1; i >= 0; i-- )
        c.Append( xs[i], lo[i], aDup );

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN serpentine( RNG& rng, int aStep, int aTeeth )
{
    SHAPE_LINE_CHAIN c;
    c.Append( 0, 0 );

    int x = 0;

    for( int ii = 0; ii < aTeeth; ++ii )
    {
        int h = (int) rng.range( aStep, aStep * 3 );
        x += aStep;
        c.Append( x, 0 );
        c.Append( x, h );
        x += aStep;
        c.Append( x, h );
        c.Append( x, aStep * 4 );
    }

    c.Append( 0, aStep * 4 );
    c.SetClosed( true );
    return c;
}


const int kDirs[16][2] = { { 1, 0 },   { 2, 1 },   { 1, 1 },   { 1, 2 },  { 0, 1 },  { -1, 2 },
                           { -1, 1 },  { -2, 1 },  { -1, 0 },  { -2, -1 }, { -1, -1 }, { -1, -2 },
                           { 0, -1 },  { 1, -2 },  { 1, -1 },  { 2, -1 } };


SHAPE_LINE_CHAIN star( RNG& rng, int S )
{
    SHAPE_LINE_CHAIN c;
    VECTOR2I         ctr = rng.ptR( -S / 2, S / 2 );

    for( int d = 0; d < 16; d++ )
    {
        if( rng.range( 0, 3 ) == 0 )
            continue;

        int r = (int) rng.range( 1, std::max( 1, S / 2 ) );
        c.Append( ctr.x + kDirs[d][0] * r, ctr.y + kDirs[d][1] * r );
    }

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN subdividedSquare( int aSize, int aSteps )
{
    SHAPE_LINE_CHAIN c;

    for( int i = 0; i < aSteps; i++ )
        c.Append( (int) ( (int64_t) i * aSize / aSteps ), 0 );

    for( int i = 0; i < aSteps; i++ )
        c.Append( aSize, (int) ( (int64_t) i * aSize / aSteps ) );

    for( int i = 0; i < aSteps; i++ )
        c.Append( aSize - (int) ( (int64_t) i * aSize / aSteps ), aSize );

    for( int i = 0; i < aSteps; i++ )
        c.Append( 0, aSize - (int) ( (int64_t) i * aSize / aSteps ) );

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN tiny( RNG& rng )
{
    SHAPE_LINE_CHAIN c;

    switch( rng.next() % 4 )
    {
    case 0:
    {
        int n = (int) rng.range( 0, 8 );

        for( int i = 0; i < n; i++ )
            c.Append( rng.ptR( 0, 60 ), true );

        break;
    }
    case 1:
    {
        int n = (int) rng.range( 1, 8 );

        for( int i = 0; i < n; i++ )
        {
            int x = (int) rng.range( -100000, 100000 );
            c.Append( x, 0, true );
        }

        break;
    }
    case 2:
    {
        int      n = (int) rng.range( 1, 6 );
        VECTOR2I p = rng.ptR( -1000, 1000 );

        for( int i = 0; i < n; i++ )
            c.Append( p, true );

        break;
    }
    default:
    {
        int s = (int) rng.range( 1, 200 );
        c.Append( 0, 0 );
        c.Append( s, 0 );
        c.Append( s, s );
        c.Append( 0, s );
        break;
    }
    }

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN noisy( RNG& rng, const SHAPE_LINE_CHAIN& aBase )
{
    SHAPE_LINE_CHAIN c;

    for( int i = 0; i < aBase.PointCount(); i++ )
    {
        VECTOR2I p = aBase.CPoint( i );
        c.Append( p, true );

        if( rng.range( 0, 3 ) == 0 )
        {
            int dx = (int) rng.range( -2, 2 );
            int dy = (int) rng.range( -2, 2 );
            c.Append( p + VECTOR2I( dx, dy ), true );
        }

        if( rng.range( 0, 7 ) == 0 )
            c.Append( p, true );
    }

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN genChain( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    int              S = scaleOf( rng );

    switch( rng.next() % 8 )
    {
    case 0: c = ring( rng ); break;
    case 1:
    {
        int  k = (int) rng.range( 2, 40 );
        bool dup = rng.flag();
        c = monotone( rng, k, S, dup );
        break;
    }
    case 2:
    {
        int step = (int) rng.range( 1, std::max( 1, S / 50 ) );
        int teeth = (int) rng.range( 1, 24 );
        c = serpentine( rng, step, teeth );
        break;
    }
    case 3:
    {
        int size = (int) rng.range( 1, S );
        int steps = (int) rng.range( 1, 30 );
        c = subdividedSquare( size, steps );
        break;
    }
    case 4: c = star( rng, S ); break;
    case 5: c = tiny( rng ); break;
    case 6:
    {
        int              k = (int) rng.range( 2, 30 );
        SHAPE_LINE_CHAIN base = monotone( rng, k, S, false );
        c = noisy( rng, base );
        break;
    }
    default:
    {
        int k = (int) rng.range( 40, 400 );
        c = monotone( rng, k, S, false );
        break;
    }
    }

    if( rng.flag() )
        c = c.Reverse();

    return c;
}


SHAPE_LINE_CHAIN rectChain( int x0, int y0, int x1, int y1 )
{
    SHAPE_LINE_CHAIN c;
    c.Append( x0, y0 );
    c.Append( x1, y0 );
    c.Append( x1, y1 );
    c.Append( x0, y1 );
    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN translated( const SHAPE_LINE_CHAIN& aChain, const VECTOR2I& aDelta )
{
    SHAPE_LINE_CHAIN c( aChain );
    c.Move( aDelta );
    return c;
}


SHAPE_POLY_SET::POLYGON genPolygon( RNG& rng )
{
    SHAPE_POLY_SET::POLYGON poly;
    int                     S = std::max( 1000, scaleOf( rng ) );

    switch( rng.next() % 4 )
    {
    case 0:
    {
        int W = (int) rng.range( 10, S );
        int H = (int) rng.range( 10, S );
        int g = (int) rng.range( 1, 4 );
        int cw = W / g;
        int ch = H / g;
        poly.push_back( rectChain( 0, 0, W, H ) );

        for( int i = 0; i < g; i++ )
        {
            for( int j = 0; j < g; j++ )
            {
                if( !rng.flag() )
                    continue;

                int x0 = i * cw + (int) rng.range( 1, cw / 3 + 1 );
                int x1 = ( i + 1 ) * cw - (int) rng.range( 1, cw / 3 + 1 );
                int y0 = j * ch + (int) rng.range( 1, ch / 3 + 1 );
                int y1 = ( j + 1 ) * ch - (int) rng.range( 1, ch / 3 + 1 );
                VECTOR2I mid( ( x0 + x1 ) / 2, ( y0 + y1 ) / 2 );

                switch( rng.next() % 3 )
                {
                case 0: poly.push_back( rectChain( x0, y0, x1, y1 ) ); break;
                case 1:
                {
                    int s = std::min( x1 - x0, y1 - y0 ) / 2 - 1;

                    if( s < 1 )
                        break;

                    int k = (int) rng.range( 2, 12 );
                    poly.push_back( translated( monotone( rng, k, s, false ), mid ) );
                    break;
                }
                default:
                {
                    SHAPE_LINE_CHAIN d;
                    int              n = (int) rng.range( 1, 4 );

                    for( int m = 0; m < n; m++ )
                        d.Append( mid, true );

                    d.SetClosed( true );
                    poly.push_back( d );
                    break;
                }
                }
            }
        }

        break;
    }
    case 1:
    {
        int k = (int) rng.range( 3, 30 );
        poly.push_back( monotone( rng, k, S, false ) );
        int n = (int) rng.range( 1, 4 );

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I a = rng.ptR( -S / 2, S / 2 );
            int      w = (int) rng.range( 1, S / 4 );
            int      h = (int) rng.range( 1, S / 4 );
            poly.push_back( rectChain( a.x, a.y, a.x + w, a.y + h ) );
        }

        break;
    }
    case 2:
    {
        // A hole sharing a vertex with the outline (issue 24059)
        int W = (int) rng.range( 100, S );
        int d = (int) rng.range( 1, W / 4 );
        int y0 = (int) rng.range( 0, W / 2 );
        poly.push_back( rectChain( 0, 0, W, W ) );
        SHAPE_LINE_CHAIN h;
        h.Append( 0, y0 );
        h.Append( d, y0 + d );
        h.Append( 0, y0 + 2 * d );
        h.SetClosed( true );
        poly.push_back( h );
        break;
    }
    default:
    {
        poly.push_back( star( rng, S ) );
        int n = (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I a = rng.ptR( -S / 8, S / 8 );
            int      w = (int) rng.range( 1, S / 16 + 1 );
            poly.push_back( rectChain( a.x, a.y, a.x + w, a.y + w ) );
        }

        break;
    }
    }

    for( size_t i = 0; i < poly.size(); i++ )
    {
        if( rng.range( 0, 3 ) == 0 )
            poly[i] = poly[i].Reverse();
    }

    return poly;
}


SHAPE_POLY_SET genZone( RNG& rng )
{
    SHAPE_POLY_SET set;

    switch( rng.next() % 4 )
    {
    case 0:
    case 2:
    {
        int n = (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
        {
            SHAPE_POLY_SET::POLYGON p = genPolygon( rng );
            set.AddOutline( p[0] );

            for( size_t j = 1; j < p.size(); j++ )
                set.AddHole( p[j] );
        }

        break;
    }
    case 1:
    {
        int S = std::max( 1000, scaleOf( rng ) );
        int n = (int) rng.range( 1, 4 );

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I a = rng.ptR( -S, S / 2 );
            int      w = (int) rng.range( S / 10, S );
            int      h = (int) rng.range( S / 10, S );
            SHAPE_POLY_SET r;
            r.AddOutline( rectChain( a.x, a.y, a.x + w, a.y + h ) );
            set.BooleanAdd( r );
        }

        int m = (int) rng.range( 0, 12 );

        for( int i = 0; i < m; i++ )
        {
            SHAPE_POLY_SET r;

            if( rng.flag() )
            {
                VECTOR2I a = rng.ptR( -S, S );
                int      w = (int) rng.range( 1, S / 5 );
                int      h = (int) rng.range( 1, S / 5 );
                r.AddOutline( rectChain( a.x, a.y, a.x + w, a.y + h ) );
            }
            else
            {
                r.AddOutline( star( rng, S / 4 ) );
            }

            set.BooleanSubtract( r );
        }

        break;
    }
    default:
    {
        int n = (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
            set.AddOutline( genChain( rng ) );

        break;
    }
    }

    if( rng.range( 0, 2 ) == 0 )
        set.Fracture();

    return set;
}


// ---------------------------------------------------------------------------------------------

void sectionChain( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG              rng( aSeed * 1000003 + n );
        SHAPE_LINE_CHAIN c = genChain( rng );
        REC              r;

        TPOLY                 result( (int) rng.range( -1, 3 ) );
        POLYGON_TRIANGULATION tess( result );
        r.b( tess.TesselatePolygon( c, nullptr ) );
        recTri( r, result );

        if( rng.range( 0, 7 ) == 0 )
        {
            // Hint data: from the same chain, or from another chain
            SHAPE_LINE_CHAIN other = rng.flag() ? c : genChain( rng );
            TPOLY            hinted( 0 );
            POLYGON_TRIANGULATION tess2( hinted );
            r.b( tess2.TesselatePolygon( other, &result ) );
            recTri( r, hinted );
        }

        sec.add( r );
    }
}


void sectionPolygon( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG                     rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET::POLYGON p = genPolygon( rng );
        REC                     r;

        TPOLY                 result( 0 );
        POLYGON_TRIANGULATION tess( result );
        r.b( tess.TesselatePolygon( p, nullptr ) );
        recTri( r, result );
        sec.add( r );
    }
}


void sectionCache( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET set = genZone( rng );
        REC            r;
        bool           simplify = rng.range( 0, 3 ) == 0;
        bool           index = rng.flag();

        set.CacheTriangulation( simplify, {}, index );
        recCache( r, set );

        switch( rng.next() % 5 )
        {
        case 0: break;
        case 1:
        {
            VECTOR2I d = rng.ptR( -100000, 100000 );
            set.Move( d );
            recCache( r, set );
            set.CacheTriangulation( false );
            break;
        }
        case 2:
        {
            int      k = (int) rng.range( 1, 3 );
            VECTOR2I c = rng.ptR( -100000, 100000 );
            set.Rotate( ANGLE_90 * k, c );
            break;
        }
        case 3:
        {
            VECTOR2I c = rng.ptR( -100000, 100000 );
            set.Mirror( c, rng.flag() ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
            break;
        }
        default:
        {
            // A copy keeps the triangulation only while it is up to date; then re-cache an
            // edited set
            SHAPE_POLY_SET copy( set );
            recCache( r, copy );

            if( set.OutlineCount() > 0 && set.COutline( 0 ).PointCount() > 0 )
            {
                VECTOR2I p = set.COutline( 0 ).CPoint( 0 );
                set.SetVertex( 0, p + VECTOR2I( 1, 0 ) );
            }

            set.CacheTriangulation( false, {}, true );
            break;
        }
        }

        recCache( r, set );
        sec.add( r );
    }
}


void sectionLarge( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG              rng( aSeed * 1000003 + n );
        SHAPE_LINE_CHAIN c;

        if( rng.flag() )
        {
            int k = (int) rng.range( 4096, 14000 );
            c = monotone( rng, k, 400000000, false );
        }
        else
        {
            int teeth = (int) rng.range( 2000, 4000 );
            c = serpentine( rng, 20000, teeth );
        }

        REC                   r;
        TPOLY                 result( 0 );
        POLYGON_TRIANGULATION tess( result );
        r.b( tess.TesselatePolygon( c, nullptr ) );
        recTri( r, result );
        sec.add( r );
    }
}


void sectionPartition( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    SHAPE_POLY_SET::TASK_SUBMITTER inlineSubmitter =
            []( std::function<void()> aTask )
            {
                aTask();
            };

    for( int n = 0; n < aCount; n++ )
    {
        RNG              rng( aSeed * 1000003 + n );
        int              S = rng.flag() ? 400000000 : 30000000;
        int              k = (int) rng.range( 26000, 60000 );
        SHAPE_LINE_CHAIN c = monotone( rng, k, S, false );
        SHAPE_POLY_SET   set;
        set.AddOutline( c );

        bool submitter = rng.flag();
        set.CacheTriangulation( false, submitter ? inlineSubmitter : SHAPE_POLY_SET::TASK_SUBMITTER(),
                                false );

        REC r;
        recCache( r, set );
        sec.add( r );
    }
}


void sectionFractions( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG              rng( aSeed * 1000003 + n );
        SHAPE_LINE_CHAIN c = genChain( rng );
        size_t           leaves = (size_t) rng.range( 0, 8 );

        TPOLY                 scratch( 0 );
        POLYGON_TRIANGULATION tess( scratch );
        std::vector<double>   f = tess.PartitionAreaFractionsForTesting( c, leaves );
        REC                   r;
        r.u( f.size() );

        for( double d : f )
            r.d( d );

        sec.add( r );
    }
}


void sectionRefine( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG   rng( aSeed * 1000003 + n );
        int   gx = (int) rng.range( 1, 12 );
        int   gy = (int) rng.range( 1, 12 );
        int   S = std::max( 1000, scaleOf( rng ) );
        int   s = (int) rng.range( 1, S / 12 );
        int   j = (int) rng.range( 0, s );
        bool  flipOrientation = rng.flag();
        TPOLY tp( 0 );

        for( int y = 0; y <= gy; y++ )
        {
            for( int x = 0; x <= gx; x++ )
            {
                int dx = (int) rng.range( -j, j );
                int dy = (int) rng.range( -j, j );
                tp.AddVertex( VECTOR2I( x * s + dx, y * s + dy ) );
            }
        }

        for( int y = 0; y < gy; y++ )
        {
            for( int x = 0; x < gx; x++ )
            {
                int a = y * ( gx + 1 ) + x;
                int b = a + 1;
                int c = a + gx + 2;
                int d = a + gx + 1;
                int t[2][3];

                if( rng.flag() )
                {
                    t[0][0] = a; t[0][1] = b; t[0][2] = c;
                    t[1][0] = a; t[1][1] = c; t[1][2] = d;
                }
                else
                {
                    t[0][0] = a; t[0][1] = b; t[0][2] = d;
                    t[1][0] = b; t[1][1] = c; t[1][2] = d;
                }

                for( int m = 0; m < 2; m++ )
                {
                    if( flipOrientation )
                        tp.AddTriangle( t[m][2], t[m][1], t[m][0] );
                    else
                        tp.AddTriangle( t[m][0], t[m][1], t[m][2] );
                }
            }
        }

        tp.Refine();
        REC r;
        recTri( r, tp );
        sec.add( r );
    }
}


void sectionYStripes( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    static const int kAcc[] = { 0, 1, 2, 5, 100, 10000 };

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET set = genZone( rng );
        REC            r;

        POLY_YSTRIPES_INDEX index;
        index.Build( set );

        BOX2I   bb = set.BBox();
        int64_t mx = bb.GetWidth() / 10 + 2;
        int64_t my = bb.GetHeight() / 10 + 2;

        for( int q = 0; q < 40; q++ )
        {
            VECTOR2I pt;

            if( set.OutlineCount() > 0 && rng.range( 0, 3 ) == 0 )
            {
                int o = (int) rng.range( 0, set.OutlineCount() - 1 );
                const SHAPE_LINE_CHAIN& c = set.COutline( o );

                if( c.PointCount() > 0 )
                    pt = c.CPoint( (int) rng.range( 0, c.PointCount() - 1 ) );
            }
            else
            {
                int x = (int) rng.range( (int64_t) bb.GetX() - mx, (int64_t) bb.GetRight() + mx );
                int y = (int) rng.range( (int64_t) bb.GetY() - my, (int64_t) bb.GetBottom() + my );
                pt = VECTOR2I( x, y );
            }

            int acc = kAcc[rng.next() % 6];
            r.b( index.Contains( pt, acc ) );
        }

        sec.add( r );
    }
}


void sectionCollide( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET set = genZone( rng );
        REC            r;

        switch( rng.next() % 3 )
        {
        case 0: break;                                    // cold: Collide() triangulates
        case 1: set.CacheTriangulation( false ); break;   // cached, no index yet
        default: set.CacheTriangulation( false, {}, true ); break;
        }

        BOX2I bb = set.BBox();
        int   S = (int) std::max<int64_t>( 10, std::max( bb.GetWidth(), bb.GetHeight() ) );

        for( int q = 0; q < 8; q++ )
        {
            int64_t  x = rng.range( (int64_t) bb.GetX() - S / 4, (int64_t) bb.GetRight() + S / 4 );
            int64_t  y = rng.range( (int64_t) bb.GetY() - S / 4, (int64_t) bb.GetBottom() + S / 4 );
            VECTOR2I p( (int) x, (int) y );
            std::unique_ptr<SHAPE> shape;

            switch( rng.next() % 5 )
            {
            case 0:
            {
                int w = (int) rng.range( 0, S / 4 );
                int h = (int) rng.range( 0, S / 4 );
                shape = std::make_unique<SHAPE_RECT>( p, w, h );
                break;
            }
            case 1: shape = std::make_unique<SHAPE_RECT>( p, 1, 1 ); break;
            case 2:
            {
                int w = (int) rng.range( 0, S / 20 );
                VECTOR2I d = rng.ptR( -S / 4, S / 4 );
                shape = std::make_unique<SHAPE_SEGMENT>( p, p + d, w );
                break;
            }
            case 3:
            {
                int rad = (int) rng.range( 0, S / 10 );
                shape = std::make_unique<SHAPE_CIRCLE>( p, rad );
                break;
            }
            default:
            {
                auto compound = std::make_unique<SHAPE_COMPOUND>();
                int  m = (int) rng.range( 1, 3 );

                for( int i = 0; i < m; i++ )
                {
                    VECTOR2I d = rng.ptR( -S / 4, S / 4 );

                    switch( rng.next() % 3 )
                    {
                    case 0:
                    {
                        int w = (int) rng.range( 0, S / 20 );
                        compound->AddShape( new SHAPE_SEGMENT( p, p + d, w ) );
                        break;
                    }
                    case 1:
                    {
                        int rad = (int) rng.range( 0, S / 10 );
                        compound->AddShape( new SHAPE_CIRCLE( p + d, rad ) );
                        break;
                    }
                    default:
                    {
                        int w = (int) rng.range( 0, S / 8 );
                        int h = (int) rng.range( 0, S / 8 );
                        compound->AddShape( new SHAPE_RECT( p + d, w, h ) );
                        break;
                    }
                    }
                }

                shape = std::move( compound );
                break;
            }
            }

            int clearance = rng.range( 0, 2 ) == 0 ? 0 : (int) rng.range( -5, S / 10 );
            int mode = (int) ( rng.next() % 4 );
            int actual = -7;
            VECTOR2I loc( -7, -7 );

            bool hit = set.Collide( shape.get(), clearance, ( mode & 1 ) ? &actual : nullptr,
                                    ( mode & 2 ) ? &loc : nullptr );
            r.b( hit ).i( actual ).v( loc );
            r.b( AnyTriangleHit( set.TriangulationIndex(), set.m_triangulatedPolys, p ) );
        }

        recCache( r, set );
        sec.add( r );
    }
}


void sectionIntersectsLine( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    const double inf = std::numeric_limits<double>::infinity();
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double kSlopes[] = { inf, -inf, nan, 1e300, -1e300, 1e16, -1e16, 9.3e15, 9.2233720368547758e15,
                               9.2233720368547e15, 0.0, -0.0, 1.0, -1.0, 0.5, 1e-300, 1e10, 4.6e15 };

    for( int n = 0; n < aCount; n++ )
    {
        RNG    rng( aSeed * 1000003 + n );
        SEG    s;
        VECTOR2I a = rng.pt();

        if( rng.range( 0, 3 ) == 0 )
        {
            VECTOR2I b( a.x, rng.coord() );
            s = SEG( a, b );
        }
        else
        {
            VECTOR2I b = rng.pt();
            s = SEG( a, b );
        }

        double slope;

        switch( rng.next() % 3 )
        {
        case 0: slope = kSlopes[rng.next() % 18]; break;
        case 1: slope = rng.dbl( -10.0, 10.0 ); break;
        default: slope = rng.dblScaled( -20, 60 ); break;
        }

        double offset;

        switch( rng.next() % 6 )
        {
        case 0: offset = inf; break;
        case 1: offset = nan; break;
        case 2: offset = 0.0; break;
        default: offset = rng.dbl( -1e9, 1e9 ); break;
        }

        VECTOR2I pt( -7, -7 );
        bool     hit = s.IntersectsLine( slope, offset, pt );
        REC      r;
        r.seg( s ).d( slope ).d( offset ).b( hit ).v( pt );
        sec.add( r );
    }
}

} // namespace


int main( int argc, char** argv )
{
    wxSetAssertHandler( nullptr );

    for( int i = 1; i + 1 < argc; i += 2 )
    {
        if( std::string( argv[i] ) == "--dump" )
            g_dump = fopen( argv[i + 1], "w" );
    }

    sectionLibmProbe();
    sectionChain( "tri_chain", 500, 40000 );
    sectionPolygon( "tri_polygon", 501, 20000 );
    sectionCache( "libm:tri_cache", 502, 10000 );
    sectionLarge( "tri_large", 503, 24 );
    sectionPartition( "tri_partition", 504, 12 );
    sectionFractions( "tri_fractions", 505, 10000 );
    sectionRefine( "tri_refine", 506, 10000 );
    sectionYStripes( "ystripes", 507, 5000 );
    sectionCollide( "tri_collide", 508, 5000 );
    sectionIntersectsLine( "seg_intersects_line", 509, 20000 );

    if( g_dump )
        fclose( g_dump );

    return 0;
}
