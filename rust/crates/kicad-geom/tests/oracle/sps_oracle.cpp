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
 * Differential-test oracle for SHAPE_POLY_SET (package G4a): booleans, offsets, fracturing,
 * simplification, chamfer / fillet, queries, editing, hashes, Format / Parse, against
 * libkimath.a of the reference build (which contains the bundled Clipper2).  Output format
 * and hashing as in the other oracles (oracle_common.h); the Rust side is sps_sections.rs.
 */

#include <cmath>
#include <fstream>
#include <limits>
#include <memory>
#include <mutex>
#include <atomic>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <vector>
#include <deque>
#include <functional>

#include <clipper2/clipper.h>
#include <geometry/shape_line_chain.h>

#define private public
#include <geometry/shape_poly_set.h>
#undef private

#include <geometry/shape_arc.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_segment.h>
#include <geometry/corner_strategy.h>
#include <math/box2.h>
#include <math/util.h>
#include <math/vector2d.h>

#include <wx/debug.h>

#include "oracle_common.h"

namespace
{

const int kMaxErr[] = { 5000, 25000, 1000, 2500, 10000, 500 };

const CORNER_STRATEGY kStrategies[] = { CORNER_STRATEGY::ALLOW_ACUTE_CORNERS,
                                        CORNER_STRATEGY::CHAMFER_ACUTE_CORNERS,
                                        CORNER_STRATEGY::ROUND_ACUTE_CORNERS,
                                        CORNER_STRATEGY::CHAMFER_ALL_CORNERS,
                                        CORNER_STRATEGY::ROUND_ALL_CORNERS };


void recArc( REC& r, const SHAPE_ARC& a )
{
    r.v( a.GetP0() ).v( a.GetArcMid() ).v( a.GetP1() ).i( a.GetWidth() );
}


void recChain( REC& r, const SHAPE_LINE_CHAIN& c )
{
    r.b( c.IsClosed() ).i( c.PointCount() );

    for( int i = 0; i < c.PointCount(); i++ )
        r.v( c.CPoint( i ) ).i( c.CShapes()[i].first ).i( c.CShapes()[i].second );

    r.u( c.ArcCount() );

    for( size_t i = 0; i < c.ArcCount(); i++ )
        recArc( r, c.Arc( i ) );
}


void recPoly( REC& r, const SHAPE_POLY_SET& s )
{
    r.i( s.OutlineCount() );

    for( int i = 0; i < s.OutlineCount(); i++ )
    {
        r.u( s.CPolygon( i ).size() );

        for( const SHAPE_LINE_CHAIN& c : s.CPolygon( i ) )
            recChain( r, c );
    }
}


void recHash( REC& r, const HASH_128& h )
{
    r.u( h.Value64[0] ).u( h.Value64[1] );
}


// A coordinate: several scales, with many collinear / duplicate values
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


SHAPE_LINE_CHAIN randomRing( RNG& rng, int aMode )
{
    SHAPE_LINE_CHAIN c;
    int n = (int) rng.range( 3, 12 );

    for( int i = 0; i < n; i++ )
    {
        VECTOR2I p = gridPt( rng, aMode );
        c.Append( p );
    }

    c.SetClosed( true );
    return c;
}


SHAPE_LINE_CHAIN rectChain( const VECTOR2I& aC, int aW, int aH, bool aCw )
{
    SHAPE_LINE_CHAIN c;
    c.Append( VECTOR2I( aC.x - aW / 2, aC.y - aH / 2 ) );

    if( aCw )
    {
        c.Append( VECTOR2I( aC.x - aW / 2, aC.y + aH / 2 ) );
        c.Append( VECTOR2I( aC.x + aW / 2, aC.y + aH / 2 ) );
        c.Append( VECTOR2I( aC.x + aW / 2, aC.y - aH / 2 ) );
    }
    else
    {
        c.Append( VECTOR2I( aC.x + aW / 2, aC.y - aH / 2 ) );
        c.Append( VECTOR2I( aC.x + aW / 2, aC.y + aH / 2 ) );
        c.Append( VECTOR2I( aC.x - aW / 2, aC.y + aH / 2 ) );
    }

    c.SetClosed( true );
    return c;
}


// A regular polygon approximating a circle (libm)
SHAPE_LINE_CHAIN circleChain( const VECTOR2I& aC, int aR, int aN, double aRot )
{
    SHAPE_LINE_CHAIN c;

    for( int i = 0; i < aN; i++ )
    {
        double a = aRot + 2.0 * M_PI * i / aN;
        int    x = KiROUND( aR * std::cos( a ) );
        int    y = KiROUND( aR * std::sin( a ) );
        c.Append( VECTOR2I( aC.x + x, aC.y + y ) );
    }

    c.SetClosed( true );
    return c;
}


// A track oval (two half circles of aN points each, libm)
SHAPE_LINE_CHAIN ovalChain( const VECTOR2I& aA, const VECTOR2I& aB, int aR, int aN )
{
    SHAPE_LINE_CHAIN c;
    double dir = std::atan2( (double) aB.y - aA.y, (double) aB.x - aA.x );

    for( int i = 0; i <= aN; i++ )
    {
        double a = dir + M_PI / 2 + M_PI * i / aN;
        int    x = KiROUND( aR * std::cos( a ) );
        int    y = KiROUND( aR * std::sin( a ) );
        c.Append( VECTOR2I( aA.x + x, aA.y + y ) );
    }

    for( int i = 0; i <= aN; i++ )
    {
        double a = dir - M_PI / 2 + M_PI * i / aN;
        int    x = KiROUND( aR * std::cos( a ) );
        int    y = KiROUND( aR * std::sin( a ) );
        c.Append( VECTOR2I( aB.x + x, aB.y + y ) );
    }

    c.SetClosed( true );
    return c;
}


// A closed chain made of arcs and straight segments (libm)
SHAPE_LINE_CHAIN arcChain( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    VECTOR2I         ctr = rng.ptR( -50000000, 50000000 );
    int              r = (int) rng.range( 100000, 5000000 );
    int              n = (int) rng.range( 2, 5 );
    double           a0 = rng.dbl( 0, 2 * M_PI );

    for( int i = 0; i < n; i++ )
    {
        double s = a0 + 2 * M_PI * i / n;
        double e = s + rng.dbl( 0.3, 2 * M_PI / n );
        double m = ( s + e ) / 2;
        int    sx = KiROUND( r * std::cos( s ) );
        int    sy = KiROUND( r * std::sin( s ) );
        int    mx = KiROUND( r * std::cos( m ) );
        int    my = KiROUND( r * std::sin( m ) );
        int    ex = KiROUND( r * std::cos( e ) );
        int    ey = KiROUND( r * std::sin( e ) );

        if( rng.flag() )
        {
            c.Append( SHAPE_ARC( VECTOR2I( ctr.x + sx, ctr.y + sy ), VECTOR2I( ctr.x + mx, ctr.y + my ),
                                 VECTOR2I( ctr.x + ex, ctr.y + ey ), 0 ) );
        }
        else
        {
            c.Append( VECTOR2I( ctr.x + sx, ctr.y + sy ) );
            c.Append( VECTOR2I( ctr.x + ex, ctr.y + ey ) );
        }
    }

    c.SetClosed( true );
    return c;
}


// KiCad-like polygon sets; aLibm adds the shapes that need sin / cos (circles, ovals, arcs)
SHAPE_POLY_SET randomSet( RNG& rng, bool aLibm )
{
    SHAPE_POLY_SET s;
    int            kind = (int) ( rng.next() % ( aLibm ? 8 : 5 ) );

    switch( kind )
    {
    case 0: // random rings on several grids (degenerate, self-intersecting)
    {
        int mode = (int) ( rng.next() % 4 );
        int n = (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
        {
            s.AddOutline( randomRing( rng, mode ) );

            if( rng.next() % 3 == 0 )
                s.AddHole( randomRing( rng, mode ) );
        }

        break;
    }
    case 1: // pads: grid aligned rectangles, often touching or overlapping
    {
        int n = (int) rng.range( 1, 6 );
        int pitch = (int) rng.range( 1, 4 ) * 250000;

        for( int i = 0; i < n; i++ )
        {
            int cx = (int) rng.range( -4, 4 ) * pitch / 2;
            int cy = (int) rng.range( -4, 4 ) * pitch / 2;
            int w = (int) rng.range( 1, 4 ) * pitch / 2;
            int h = (int) rng.range( 1, 4 ) * pitch / 2;
            bool cw = rng.flag();
            s.AddOutline( rectChain( VECTOR2I( cx, cy ), w, h, cw ) );
        }

        break;
    }
    case 2: // a zone outline with rectangular holes (some touching each other / the outline)
    {
        int size = (int) rng.range( 2, 40 ) * 1000000;
        s.AddOutline( rectChain( VECTOR2I( 0, 0 ), size, size, false ) );
        int n = (int) rng.range( 0, 10 );

        for( int i = 0; i < n; i++ )
        {
            int cx = (int) rng.range( -size / 2, size / 2 ) / 100000 * 100000;
            int cy = (int) rng.range( -size / 2, size / 2 ) / 100000 * 100000;
            int w = (int) rng.range( 1, size / 200000 ) * 100000;
            int h = (int) rng.range( 1, size / 200000 ) * 100000;
            bool cw = rng.flag();
            s.AddHole( rectChain( VECTOR2I( cx, cy ), w, h, cw ) );
        }

        break;
    }
    case 3: // a comb / star outline (many collinear and acute vertices)
    {
        int teeth = (int) rng.range( 2, 12 );
        int pitch = (int) rng.range( 1, 10 ) * 100000;
        int depth = (int) rng.range( 1, 20 ) * 100000;
        SHAPE_LINE_CHAIN c;
        c.Append( VECTOR2I( 0, 0 ) );

        for( int i = 0; i < teeth; i++ )
        {
            c.Append( VECTOR2I( i * pitch * 2, depth ) );
            c.Append( VECTOR2I( i * pitch * 2 + pitch, depth ) );
            c.Append( VECTOR2I( i * pitch * 2 + pitch, 0 ) );
        }

        c.Append( VECTOR2I( teeth * pitch * 2, 0 ) );
        c.Append( VECTOR2I( teeth * pitch * 2, -depth ) );
        c.Append( VECTOR2I( 0, -depth ) );
        c.SetClosed( true );
        s.AddOutline( c );
        break;
    }
    case 4: // degenerate sets: empty, empty outline, one or two points, duplicate points
    {
        int sub = (int) ( rng.next() % 4 );

        if( sub >= 1 )
            s.NewOutline();

        if( sub >= 2 )
        {
            VECTOR2I p = gridPt( rng, 1 );
            s.Append( p.x, p.y, -1, -1, true );
            s.Append( p.x, p.y, -1, -1, true );
        }

        if( sub >= 3 )
        {
            VECTOR2I p = gridPt( rng, 1 );
            s.Append( p );
        }

        break;
    }
    case 5: // round pads
    {
        int n = (int) rng.range( 1, 4 );

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I c = rng.ptR( -2000000, 2000000 );
            int      r = (int) rng.range( 100000, 1500000 );
            int      segs = 8 << ( rng.next() % 4 );
            double   rot = ( rng.next() % 2 ) ? 0.0 : M_PI / segs;
            s.AddOutline( circleChain( c, r, segs, rot ) );
        }

        break;
    }
    case 6: // chained track ovals
    {
        int      n = (int) rng.range( 1, 4 );
        int      r = (int) rng.range( 50000, 500000 );
        int      segs = (int) rng.range( 4, 16 );
        VECTOR2I a = rng.ptR( -2000000, 2000000 );

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I b = rng.ptR( -2000000, 2000000 );
            s.AddOutline( ovalChain( a, b, r, segs ) );
            a = b;
        }

        break;
    }
    default: // outlines with arcs
    {
        s.AddOutline( arcChain( rng ) );

        if( rng.flag() )
            s.AddOutline( arcChain( rng ) );

        break;
    }
    }

    return s;
}


// RemoveNullSegments() dereferences the first vertex of every contour it reaches: undefined
// behaviour for empty contours
bool hasEmptyContour( const SHAPE_POLY_SET& aSet )
{
    for( int i = 0; i < aSet.OutlineCount(); i++ )
    {
        for( const SHAPE_LINE_CHAIN& c : aSet.CPolygon( i ) )
        {
            if( c.PointCount() == 0 )
                return true;
        }
    }

    return false;
}


// A random query point near the set
VECTOR2I queryPt( RNG& rng, const SHAPE_POLY_SET& aSet )
{
    if( aSet.TotalVertices() > 0 && rng.next() % 3 == 0 )
    {
        int      idx = (int) rng.range( 0, aSet.TotalVertices() - 1 );
        VECTOR2I p = aSet.CVertex( idx );
        int      dx = (int) rng.range( -2, 2 );
        int      dy = (int) rng.range( -2, 2 );
        return VECTOR2I( p.x + dx, p.y + dy );
    }

    BOX2I bb = aSet.BBox();
    int64_t x0 = bb.GetX() - 1000;
    int64_t y0 = bb.GetY() - 1000;
    int64_t x = rng.range( x0, x0 + bb.GetWidth() + 2000 );
    int64_t y = rng.range( y0, y0 + bb.GetHeight() + 2000 );
    return VECTOR2I( (int) x, (int) y );
}


void sectionBoolean( const char* aName, bool aLibm, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET a = randomSet( rng, aLibm );
        SHAPE_POLY_SET b = randomSet( rng, aLibm );
        int            op = (int) ( rng.next() % 9 );
        SHAPE_POLY_SET res = a;

        if( rng.next() % 4 == 0 )
            b.Move( VECTOR2I( (int) rng.range( -1000, 1000 ), 0 ) );

        switch( op )
        {
        case 0: res.BooleanAdd( b ); break;
        case 1: res.BooleanSubtract( b ); break;
        case 2: res.BooleanIntersection( b ); break;
        case 3: res.BooleanXor( b ); break;
        case 4: res.BooleanAdd( a, b ); break;
        case 5: res.BooleanSubtract( b, a ); break;
        case 6: res.BooleanIntersection( a, b ); break;
        case 7: res.BooleanXor( b, a ); break;
        default: res.Simplify(); break;
        }

        REC r;
        r.i( op );
        recPoly( r, res );
        r.d( res.Area() ).i( res.ArcCount() );
        sec.add( r );
    }
}


void sectionInflate( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG             rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET  s = randomSet( rng, true );
        CORNER_STRATEGY cs = kStrategies[rng.next() % 5];
        int             err = kMaxErr[rng.next() % 6];
        int             amount;

        switch( rng.next() % 4 )
        {
        case 0: amount = (int) rng.range( -10, 10 ); break;
        case 1: amount = (int) rng.range( -2000000, 2000000 ); break;
        case 2: amount = 100000 * (int) rng.range( -10, 10 ); break;
        default: amount = (int) rng.range( -300000, 300000 ); break;
        }

        int  mode = (int) ( rng.next() % 5 );
        bool simplify = rng.flag();

        REC r;
        r.i( mode ).i( amount ).i( err ).b( simplify );

        if( mode == 0 )
        {
            s.Inflate( amount, cs, err, simplify );
            recPoly( r, s );
        }
        else if( mode == 1 )
        {
            s.Deflate( amount, cs, err );
            recPoly( r, s );
        }
        else if( mode == 2 )
        {
            s.ClearArcs();
            s.Fracture();
            s.InflateWithLinkedHoles( amount, cs, err );
            recPoly( r, s );
        }
        else
        {
            SHAPE_LINE_CHAIN line;

            if( s.OutlineCount() > 0 )
                line = s.COutline( 0 );

            if( mode == 4 && line.PointCount() > 0 )
                line.Remove( line.PointCount() - 1 );

            line.SetClosed( false );

            // Clipper2 crashes on open paths without two distinct points (empty group)
            bool distinct = false;

            for( int i = 1; i < line.PointCount(); i++ )
            {
                if( line.CPoint( i ) != line.CPoint( 0 ) )
                    distinct = true;
            }

            r.b( distinct );

            if( !distinct )
            {
                sec.add( r );
                continue;
            }

            SHAPE_POLY_SET out;
            out.OffsetLineChain( line, amount, cs, err, simplify );
            recPoly( r, out );

            SHAPE_LINE_CHAIN left, right;
            bool ok = line.OffsetLine( amount, cs, err, left, right, simplify );
            r.b( ok );

            if( ok )
            {
                recChain( r, left );
                recChain( r, right );
            }
        }

        sec.add( r );
    }
}


// A zone-like set with many holes, fractured; big ones exercise the fracture edge index and
// the bridge removal of booleanOp() (single outlines of 1024+ points)
SHAPE_POLY_SET zoneWithHoles( RNG& rng, bool aBig )
{
    SHAPE_POLY_SET s;
    int            r = (int) rng.range( 5000000, 50000000 );
    int            segs = aBig ? (int) rng.range( 900, 2500 ) : (int) rng.range( 8, 64 );
    s.AddOutline( circleChain( VECTOR2I( 0, 0 ), r, segs, 0.0 ) );
    int nh = aBig ? (int) rng.range( 8, 40 ) : (int) rng.range( 0, 8 );

    for( int i = 0; i < nh; i++ )
    {
        int      hr = (int) rng.range( r / 100, r / 10 );
        VECTOR2I c = rng.ptR( -r / 2, r / 2 );
        int      hs = (int) rng.range( 3, 40 );

        if( rng.flag() )
            s.AddHole( circleChain( c, hr, hs, rng.dbl( 0, 1 ) ) );
        else
            s.AddHole( rectChain( c, hr, hr * 2 / 3, rng.flag() ) );
    }

    return s;
}


void sectionFracture( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        bool           big = n % 25 == 0;
        int            src = (int) ( rng.next() % 3 );
        SHAPE_POLY_SET s = src == 0 ? randomSet( rng, true ) : zoneWithHoles( rng, big );
        int            mode = (int) ( rng.next() % 4 );

        s.ClearArcs();

        REC r;
        r.i( src ).i( mode );

        if( mode == 0 )
        {
            s.Fracture( true );
            recPoly( r, s );
        }
        else if( mode == 1 )
        {
            s.Fracture( false );
            recPoly( r, s );
            s.Unfracture();
            recPoly( r, s );
        }
        else if( mode == 2 )
        {
            s.Fracture();
            r.b( s.HasHoles() ).b( s.HasTouchingHoles() );
            SHAPE_POLY_SET b = randomSet( rng, true );
            b.ClearArcs();
            SHAPE_POLY_SET res = s;

            if( rng.flag() )
                res.BooleanSubtract( b );
            else
                res.BooleanAdd( b );

            recPoly( r, res );
            res = s;
            res.Simplify();
            recPoly( r, res );
        }
        else
        {
            s.Fracture();
            SHAPE_POLY_SET copy = s;
            copy.Unfracture();
            recPoly( r, copy );
            copy.Fracture();
            recPoly( r, copy );
        }

        sec.add( r );
    }
}


void sectionSimplify( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET s = randomSet( rng, false );
        int            mode = (int) ( rng.next() % 8 );

        REC r;
        r.i( mode );

        switch( mode )
        {
        case 0: s.Simplify(); break;
        case 1: s.SimplifyOutlines( (int) rng.range( 0, 3 ) * 1000 ); break;
        case 2: // Polygon( 0 ) of an empty set is undefined behaviour
            if( s.OutlineCount() > 0 )
                r.i( s.NormalizeAreaOutlines() );

            break;
        case 3: s.RebuildHolesFromContours(); break;
        case 4:
        {
            std::vector<SHAPE_LINE_CHAIN> paths;

            for( int i = 0; i < s.OutlineCount(); i++ )
            {
                for( const SHAPE_LINE_CHAIN& c : s.CPolygon( i ) )
                    paths.push_back( c );
            }

            s.BuildPolysetFromOrientedPaths( paths, rng.flag() );
            break;
        }
        case 5: // the first vertex of an empty set is undefined behaviour
            if( s.OutlineCount() > 0 && !hasEmptyContour( s ) )
                r.i( s.RemoveNullSegments() );

            break;
        case 6: s.splitSelfTouchingOutlines(); break;
        default:
        {
            // An outline with an exterior waist (two collinear, touching runs)
            int w = (int) rng.range( 1, 50 ) * 100000;
            int h = (int) rng.range( 1, 50 ) * 100000;
            int g = (int) rng.range( 0, 3 ) * 10;
            SHAPE_LINE_CHAIN c;
            c.Append( VECTOR2I( 0, 0 ) );
            c.Append( VECTOR2I( w, 0 ) );
            c.Append( VECTOR2I( w, h ) );
            c.Append( VECTOR2I( w + w, h + g ) );
            c.Append( VECTOR2I( w + w, h + h ) );
            c.Append( VECTOR2I( w, h + h ) );
            c.Append( VECTOR2I( w, h + g ) );
            c.Append( VECTOR2I( 0, h ) );
            c.SetClosed( true );
            s.AddOutline( c );
            s.Simplify();
            break;
        }
        }

        recPoly( r, s );
        sec.add( r );
    }
}


void sectionChamferFillet( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET s = randomSet( rng, true );
        int            dist = (int) ( rng.next() % 3 == 0 ? rng.range( 0, 100 ) : rng.range( 0, 2000000 ) );
        int            err = kMaxErr[rng.next() % 6];
        int            mode = (int) ( rng.next() % 4 );

        s.ClearArcs();

        // Leave out contours with fewer than 3 distinct points (division by zero in C++)
        bool ok = s.OutlineCount() > 0 && !hasEmptyContour( s );

        if( !ok )
            continue;

        SHAPE_POLY_SET probe = s;
        probe.RemoveNullSegments();

        for( int i = 0; i < probe.OutlineCount(); i++ )
        {
            for( const SHAPE_LINE_CHAIN& c : probe.CPolygon( i ) )
            {
                if( c.PointCount() < 3 )
                    ok = false;
            }
        }

        if( !ok )
            continue;

        REC r;
        r.i( mode ).i( dist ).i( err );

        if( mode == 0 )
        {
            recPoly( r, s.Chamfer( dist ) );
        }
        else if( mode == 1 )
        {
            recPoly( r, s.Fillet( dist, err ) );
        }
        else
        {
            int idx = (int) rng.range( 0, s.OutlineCount() - 1 );
            SHAPE_POLY_SET::POLYGON p = mode == 2 ? s.ChamferPolygon( dist, idx ) : s.FilletPolygon( dist, err, idx );
            r.u( p.size() );

            for( const SHAPE_LINE_CHAIN& c : p )
                recChain( r, c );
        }

        recPoly( r, s );
        sec.add( r );
    }
}


void sectionQuery( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET s = randomSet( rng, true );

        REC r;
        r.d( s.Area() ).i( s.ArcCount() ).i( s.TotalVertices() ).i( s.FullPointCount() );
        r.i( s.VertexCount() ).i( s.VertexCount( 0, 0 ) ).i( s.HoleCount( 0 ) );
        r.b( s.HasHoles() ).b( s.HasTouchingHoles() ).b( s.IsSelfIntersecting() ).b( s.IsEmpty() );
        r.box( s.BBox() ).box( s.BBox( 1000 ) );
        recHash( r, s.GetHash() );
        r.s( s.Format() );

        for( int i = 0; i < s.OutlineCount(); i++ )
            r.b( s.IsPolygonSelfIntersecting( i ) );

        // vertex indices
        for( int g = -1; g <= s.TotalVertices(); g++ )
        {
            SHAPE_POLY_SET::VERTEX_INDEX idx;
            bool ok = s.GetRelativeIndices( g, &idx );
            r.b( ok ).i( idx.m_polygon ).i( idx.m_contour ).i( idx.m_vertex );

            if( ok )
            {
                int back = -7;
                r.b( s.GetGlobalIndex( idx, back ) ).i( back ).v( s.CVertex( idx ) ).v( s.CVertex( g ) );
                r.b( s.IsVertexInHole( g ) );
                int prev = -7, next = -7;
                r.b( s.GetNeighbourIndexes( g, &prev, &next ) );

                // the C++ leaves an output uninitialised for contours with fewer than 3 points
                if( s.CPolygon( idx.m_polygon )[idx.m_contour].PointCount() >= 3 )
                    r.i( prev ).i( next );
            }
        }

        // iterators
        for( SHAPE_POLY_SET::CONST_ITERATOR it = s.CIterateWithHoles(); it; it++ )
            r.v( *it ).i( it.GetIndex().m_contour ).b( it.IsEndContour() ).b( it.IsLastPolygon() );

        for( int i = 0; i < s.OutlineCount(); i++ )
        {
            for( SHAPE_POLY_SET::CONST_ITERATOR it = s.CIterate( i ); it; it++ )
                r.v( *it );

            for( SHAPE_POLY_SET::CONST_SEGMENT_ITERATOR it = s.CIterateSegmentsWithHoles( i ); it; it++ )
                r.seg( *it ).i( it.GetIndex().m_contour ).i( it.GetIndex().m_vertex );
        }

        for( SHAPE_POLY_SET::CONST_SEGMENT_ITERATOR it = s.CIterateSegments(); it; it++ )
            r.seg( *it );

        if( n % 2 == 0 )
            s.BuildBBoxCaches();

        if( s.OutlineCount() > 0 && n % 2 == 0 )
            r.box( s.BBoxFromCaches() );

        if( s.TotalVertices() == 0 )
        {
            sec.add( r );
            continue;
        }

        for( int q = 0; q < 12; q++ )
        {
            VECTOR2I p = queryPt( rng, s );
            int      cl = (int) ( rng.next() % 3 == 0 ? 0 : rng.range( 0, 200000 ) );
            int      acc = (int) rng.range( 0, 3 );
            int      sub = (int) rng.range( -1, s.OutlineCount() - 1 );

            r.v( p ).b( s.Contains( p ) ).b( s.Contains( p, sub, acc, n % 2 == 0 ) );
            r.b( s.PointInside( p ) ).b( s.PointOnEdge( p ) ).b( s.PointOnEdge( p, acc ) );

            VECTOR2I nearest( -1, -1 );
            r.i( s.SquaredDistance( p ) ).i( s.SquaredDistance( p, false, &nearest ) ).v( nearest );

            int      actual = -1;
            VECTOR2I loc( -1, -1 );
            r.b( s.Collide( p, cl ) ).b( s.Collide( p, cl, &actual, &loc ) ).i( actual ).v( loc );

            SHAPE_POLY_SET::VERTEX_INDEX vi;
            r.b( s.CollideVertex( p, nullptr, cl ) ).b( s.CollideVertex( p, &vi, cl ) );
            r.i( vi.m_polygon ).i( vi.m_contour ).i( vi.m_vertex );
            SHAPE_POLY_SET::VERTEX_INDEX ei;
            r.b( s.CollideEdge( p, nullptr, cl ) ).b( s.CollideEdge( p, &ei, cl ) );
            r.i( ei.m_polygon ).i( ei.m_contour ).i( ei.m_vertex );

            VECTOR2I p2 = queryPt( rng, s );
            SEG      seg( p, p2 );
            VECTOR2I near2( -1, -1 );
            r.i( s.SquaredDistanceToSeg( seg ) ).i( s.SquaredDistanceToSeg( seg, &near2 ) ).v( near2 );

            int      actual2 = -1;
            VECTOR2I loc2( -1, -1 );
            r.b( s.Collide( seg, cl ) ).b( s.Collide( seg, cl, &actual2, &loc2 ) ).i( actual2 ).v( loc2 );

            if( s.OutlineCount() > 0 )
            {
                int      pi = (int) rng.range( 0, s.OutlineCount() - 1 );
                VECTOR2I n3( -1, -1 ), n4( -1, -1 );
                r.i( s.SquaredDistanceToPolygon( p, pi, &n3 ) ).v( n3 );
                r.i( s.SquaredDistanceToPolygon( seg, pi, &n4 ) ).v( n4 );
            }

            int           w = (int) rng.range( 0, 300000 );
            SHAPE_SEGMENT ss( seg, w );
            SHAPE_CIRCLE  sc( p2, w / 2 + 1 );
            int           actual3 = -1, actual4 = -1;
            VECTOR2I      loc3( -1, -1 ), loc4( -1, -1 );
            r.b( s.Collide( &ss, cl ) ).b( s.Collide( &ss, cl, &actual3, &loc3 ) ).i( actual3 ).v( loc3 );
            r.b( s.Collide( &sc, cl, &actual4, nullptr ) ).i( actual4 );
            r.b( s.Collide( &sc, cl, nullptr, &loc4 ) ).v( loc4 );
        }

        // hatch lines
        if( n % 3 == 0 && s.OutlineCount() > 0 )
        {
            BOX2I               bb = s.BBox();
            int                 spacing = std::max( 1, (int) ( std::max( bb.GetWidth(), bb.GetHeight() ) / rng.range( 5, 40 ) ) );
            int                 len = rng.flag() ? -1 : (int) rng.range( 1, spacing * 4 );
            std::vector<double> slopes = { 1.0, -1.0 };

            if( rng.flag() )
                slopes.push_back( rng.dbl( -3, 3 ) );

            std::vector<SEG> lines = s.GenerateHatchLines( slopes, spacing, len );
            r.u( lines.size() );

            for( const SEG& l : lines )
                r.seg( l );
        }

        sec.add( r );
    }
}


void sectionEdit( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG            rng( aSeed * 1000003 + n );
        SHAPE_POLY_SET s = randomSet( rng, true );
        int            mode = (int) ( rng.next() % 12 );
        int            tv = s.TotalVertices();

        REC r;
        r.i( mode );

        switch( mode )
        {
        case 0:
        {
            if( s.OutlineCount() == 0 )
                break;

            int      g = (int) rng.range( -3, tv + 3 );
            VECTOR2I p = rng.ptR( -1000000, 1000000 );
            s.InsertVertex( g, p );
            break;
        }
        case 1:
            if( tv > 0 )
                s.RemoveVertex( (int) rng.range( 0, tv - 1 ) );

            break;
        case 2:
            if( tv > 0 )
            {
                int      g = (int) rng.range( 0, tv - 1 );
                VECTOR2I p = rng.ptR( -1000000, 1000000 );
                s.SetVertex( g, p );
            }

            break;
        case 3:
            if( s.OutlineCount() > 0 )
            {
                int o = (int) rng.range( 0, s.OutlineCount() - 1 );

                if( rng.flag() )
                    s.RemoveOutline( o );
                else if( s.HoleCount( o ) > 0 )
                    s.RemoveContour( (int) rng.range( 1, s.HoleCount( o ) ), o );
                else
                    s.DeletePolygon( o );
            }

            break;
        case 4:
        {
            SHAPE_POLY_SET t = randomSet( rng, true );
            s.Append( t );
            int a = (int) rng.range( 0, s.OutlineCount() );
            int b = (int) rng.range( a, s.OutlineCount() );
            recPoly( r, s.Subset( a, b ) );

            if( a < s.OutlineCount() )
                recPoly( r, s.UnitSet( a ) );

            break;
        }
        case 5:
        {
            VECTOR2I ref = rng.ptR( -1000000, 1000000 );
            s.Mirror( ref, rng.flag() ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
            break;
        }
        case 6:
        {
            VECTOR2I a = rng.ptR( -1000000, 1000000 );
            VECTOR2I b = rng.ptR( -1000000, 1000000 );
            s.Mirror( SEG( a, b ) );
            break;
        }
        case 7:
        {
            VECTOR2I v = rng.ptR( -1000000, 1000000 );
            s.Move( v );
            recHash( r, s.GetHash() );

            if( tv > 0 )
                s.SetVertex( 0, VECTOR2I( 1, 2 ) );

            // a stale hash survives in this set, a copy recomputes it
            recHash( r, s.GetHash() );
            SHAPE_POLY_SET copy = s;
            recHash( r, copy.GetHash() );
            recHash( r, s.CloneDropTriangulation().GetHash() );
            r.b( s.IsTriangulationUpToDate() );
            break;
        }
        case 8:
        {
            double   fx = rng.dbl( -3, 3 );
            double   fy = rng.flag() ? fx : rng.dbl( -3, 3 );
            VECTOR2I c = rng.ptR( -1000000, 1000000 );
            s.Scale( fx, fy, c );
            break;
        }
        case 9:
        {
            double   deg = rng.angleDeg();
            VECTOR2I c = rng.ptR( -1000000, 1000000 );
            s.Rotate( EDA_ANGLE( deg, DEGREES_T ), c );
            break;
        }
        case 10:
        {
            // Append( SHAPE_ARC ) and holes
            if( s.OutlineCount() == 0 )
                s.NewOutline();

            int h = s.NewHole();
            r.i( h );
            VECTOR2I a = rng.ptR( -1000000, 1000000 );
            VECTOR2I m = rng.ptR( -1000000, 1000000 );
            VECTOR2I e = rng.ptR( -1000000, 1000000 );
            SHAPE_ARC arc( a, m, e, 0 );

            if( rng.flag() )
                r.i( s.Append( arc, -1, h ) );
            else
                r.i( s.Append( arc, -1, h, kMaxErr[rng.next() % 6] ) );

            r.i( s.Append( 5, 6, -1, -1, rng.flag() ) );
            break;
        }
        default:
        {
            // Format / Parse round trip and corrupted texts
            std::stringstream ss;
            ss << "polyset " << s.OutlineCount();

            for( int i = 0; i < s.OutlineCount(); i++ )
            {
                ss << " poly " << s.CPolygon( i ).size();

                for( const SHAPE_LINE_CHAIN& c : s.CPolygon( i ) )
                {
                    ss << " " << c.PointCount();

                    for( const VECTOR2I& p : c.CPoints() )
                        ss << " " << p.x << " " << p.y;
                }
            }

            std::string text = ss.str();
            int         corrupt = (int) ( rng.next() % 4 );

            if( corrupt == 1 && !text.empty() )
                text = text.substr( 0, (size_t) rng.range( 0, (int64_t) text.size() - 1 ) );
            else if( corrupt == 2 )
                text += "abc 12x -3";

            std::stringstream in( text );
            SHAPE_POLY_SET    parsed;
            r.b( parsed.Parse( in ) );
            recPoly( r, parsed );
            break;
        }
        }

        recPoly( r, s );
        recHash( r, s.GetHash() );
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
    sectionBoolean( "sps_boolean", false, 400, 20000 );
    sectionBoolean( "libm:sps_boolean_kicad", true, 401, 20000 );
    sectionInflate( "libm:sps_inflate", 402, 20000 );
    sectionFracture( "libm:sps_fracture", 403, 5000 );
    sectionSimplify( "sps_simplify", 404, 20000 );
    sectionChamferFillet( "libm:sps_chamfer_fillet", 405, 15000 );
    sectionQuery( "libm:sps_query", 406, 10000 );
    sectionEdit( "libm:sps_edit", 407, 20000 );

    if( g_dump )
        fclose( g_dump );

    return 0;
}
