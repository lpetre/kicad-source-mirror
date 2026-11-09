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
 * Differential-test oracle for SHAPE_LINE_CHAIN (kicad-geom package G3) and for the
 * out-of-range double to int conversions of PORTING.md section 9.
 *
 * Links the static kimath library of the reference build (see build_slc_oracle.sh), builds
 * pseudo-random line chains (plain points, arcs, shared arc points, closed chains), edits
 * them with every mutating method and queries them with every const method, and prints one
 * line per section: name, case count, FNV-1a hash of the records.  The Rust replay is
 * tests/oracle/slc_sections.rs (tests/oracle_slc_diff.rs).  Generators draw their random
 * values in separate statements, so the Rust side can draw them in the same order.
 */

#include <cmath>
#include <limits>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include <clipper2/clipper.h>

// The arc bookkeeping helpers (splitArc, convertArc, convertToClipper2, ...) are protected;
// access specifiers do not change the ABI of the library they are compiled into.
#define protected public
#include <geometry/shape_line_chain.h>
#undef protected

#include <geometry/shape_arc.h>
#include <geometry/shape_poly_set.h>
#include <math/box2.h>
#include <math/vector2d.h>

#include "oracle_common.h"

namespace
{

const int kErr[] = { 5000, 25000, 1000 };


void recArc( REC& r, const SHAPE_ARC& a )
{
    r.v( a.GetP0() ).v( a.GetArcMid() ).v( a.GetP1() ).i( a.GetWidth() );
}


void recChain( REC& r, const SHAPE_LINE_CHAIN& c )
{
    r.b( c.IsClosed() ).i( c.Width() ).i( c.PointCount() );

    for( int i = 0; i < c.PointCount(); i++ )
        r.v( c.CPoint( i ) ).i( c.CShapes()[i].first ).i( c.CShapes()[i].second );

    r.u( c.CShapes().size() ).u( c.ArcCount() );

    for( size_t i = 0; i < c.ArcCount(); i++ )
        recArc( r, c.Arc( i ) );
}


void recIs( REC& r, const SHAPE_LINE_CHAIN::INTERSECTION& is )
{
    r.v( is.p ).i( is.index_our ).i( is.index_their ).b( is.is_corner_our );
    r.b( is.is_corner_their ).b( is.valid );
}


// A coordinate for the chains: several scales, with many collinear / duplicate values
VECTOR2I chainPt( RNG& rng, int aMode )
{
    switch( aMode )
    {
    case 0: { int x = (int) rng.range( -3, 3 ); int y = (int) rng.range( -3, 3 ); return VECTOR2I( x * 1000, y * 1000 ); }
    case 1: return rng.ptR( -20, 20 );
    case 2: return rng.ptR( -1000000, 1000000 );
    default: return rng.ptR( -50000000, 50000000 );
    }
}


SHAPE_ARC chainArc( RNG& rng, bool aConnect, const VECTOR2I& aStart )
{
    VECTOR2I s = aStart;

    if( !aConnect )
        s = rng.ptR( -50000000, 50000000 );

    switch( rng.next() % 4 )
    {
    case 0:
    {
        // small arc from three points (often degenerate: collinear, coincident)
        VECTOR2I m = s + rng.ptR( -3000, 3000 );
        VECTOR2I e = s + rng.ptR( -3000, 3000 );
        return SHAPE_ARC( s, m, e, 0 );
    }
    case 1:
    {
        // full circle
        VECTOR2I d = rng.ptR( -20000000, 20000000 );

        if( d == VECTOR2I( 0, 0 ) )
            d = VECTOR2I( 1000, 0 );

        return SHAPE_ARC( s + d, s, ANGLE_360, 0 );
    }
    default:
    {
        VECTOR2I d = rng.ptR( -20000000, 20000000 );

        if( d == VECTOR2I( 0, 0 ) )
            d = VECTOR2I( 1000, 0 );

        double a = rng.dbl( 5.0, 350.0 );
        bool   neg = rng.flag();
        return SHAPE_ARC( s + d, s, EDA_ANGLE( neg ? -a : a, DEGREES_T ), 0 );
    }
    }
}


SHAPE_LINE_CHAIN plainChain( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    int              n = (int) rng.range( 0, 20 );
    int              mode = (int) ( rng.next() % 4 );

    for( int i = 0; i < n; i++ )
    {
        VECTOR2I p = chainPt( rng, mode );

        if( c.PointCount() > 0 && rng.next() % 8 == 0 )
            p = c.CLastPoint();

        bool dup = rng.next() % 4 == 0;
        c.Append( p, dup );
    }

    bool closed = rng.flag();
    c.SetClosed( closed );

    if( rng.next() % 4 == 0 )
    {
        int w = (int) rng.range( 0, 100000 );
        c.SetWidth( w );
    }

    return c;
}


SHAPE_LINE_CHAIN arcChain( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    int              pieces = (int) rng.range( 1, 5 );

    for( int k = 0; k < pieces; k++ )
    {
        int err = kErr[rng.next() % 3];

        switch( rng.next() % 6 )
        {
        case 0:
        case 1:
        {
            VECTOR2I p = chainPt( rng, 3 );
            c.Append( p );
            break;
        }
        case 2:
        case 3:
        {
            bool      connect = c.PointCount() > 0;
            SHAPE_ARC a = chainArc( rng, connect, connect ? c.CLastPoint() : VECTOR2I() );
            c.Append( a, err );
            break;
        }
        case 4:
        {
            SHAPE_ARC a = chainArc( rng, false, VECTOR2I() );
            c.Append( a, err );
            break;
        }
        default:
        {
            // a chain of its own, appended (with arc index renumbering)
            bool             connect = c.PointCount() > 0;
            SHAPE_ARC        a = chainArc( rng, connect, connect ? c.CLastPoint() : VECTOR2I() );
            SHAPE_LINE_CHAIN sub( a, false, err );
            VECTOR2I         p = chainPt( rng, 3 );
            sub.Append( p );
            c.Append( sub );
            break;
        }
        }
    }

    switch( rng.next() % 4 )
    {
    case 0: c.SetClosed( true ); break;
    case 1:
        // close the chain on its first point (merges shared arc points, fixIndicesRotation)
        if( c.PointCount() > 0 )
            c.Append( c.CPoint( 0 ) );

        c.SetClosed( true );
        break;
    default: break;
    }

    return c;
}


SHAPE_LINE_CHAIN anyChain( RNG& rng )
{
    if( rng.flag() )
        return arcChain( rng );

    return plainChain( rng );
}


VECTOR2I queryPt( RNG& rng, const SHAPE_LINE_CHAIN& c )
{
    switch( rng.next() % 5 )
    {
    case 0:
        if( c.PointCount() > 0 )
            return c.CPoint( (int) rng.range( 0, c.PointCount() - 1 ) );

        return rng.ptMid();
    case 1:
        if( c.PointCount() > 0 )
        {
            VECTOR2I p = c.CPoint( (int) rng.range( 0, c.PointCount() - 1 ) );
            return p + rng.ptR( -3, 3 );
        }

        return rng.ptMid();
    case 2:
        if( c.SegmentCount() > 0 )
        {
            SEG     s = c.CSegment( (int) rng.range( 0, c.SegmentCount() - 1 ) );
            int64_t t = rng.range( 0, 1000 );
            int64_t x = s.A.x + ( (int64_t) s.B.x - s.A.x ) * t / 1000;
            int64_t y = s.A.y + ( (int64_t) s.B.y - s.A.y ) * t / 1000;
            return VECTOR2I( (int) x, (int) y );
        }

        return rng.ptMid();
    case 3: return rng.ptR( -2000, 2000 );
    default: return rng.ptR( -60000000, 60000000 );
    }
}


int clearance( RNG& rng )
{
    switch( rng.next() % 4 )
    {
    case 0: return 0;
    case 1: return (int) rng.range( 0, 10 );
    case 2: return (int) rng.range( 0, 100000 );
    default: return (int) rng.range( 0, 10000000 );
    }
}


// NearestPoint( p, false ) reads m_shapes[PointCount()] (out of bounds) when the nearest
// segment is the closing arc segment of a closed chain and p is nearer to its end
bool nearestPointUB( const SHAPE_LINE_CHAIN& c, const VECTOR2I& p )
{
    int min_d = std::numeric_limits<int>::max();
    int nearest = 0;

    for( int i = 0; i < c.SegmentCount(); i++ )
    {
        int d = c.CSegment( i ).Distance( p );

        if( d < min_d )
        {
            min_d = d;
            nearest = i;
        }
    }

    if( nearest > 0 && nearest < c.PointCount() && c.IsArcSegment( nearest ) )
    {
        VECTOR2I ptToSegStart = c.CSegment( nearest ).A - p;
        VECTOR2I ptToSegEnd = c.CSegment( nearest ).B - p;

        if( ptToSegStart.EuclideanNorm() > ptToSegEnd.EuclideanNorm() )
            nearest++;

        return nearest >= c.PointCount();
    }

    return false;
}


// Consecutive equal points: the Clipper2 constructor would write past the shape array
bool hasCyclicDuplicate( const SHAPE_LINE_CHAIN& c )
{
    for( int i = 0; i < c.PointCount(); i++ )
    {
        if( c.CPoint( i ) == c.CPoint( ( i + 1 ) % c.PointCount() ) )
            return true;
    }

    return false;
}


void recState( REC& r, const SHAPE_LINE_CHAIN& c )
{
    recChain( r, c );
    r.i( c.ShapeCount() );

    for( int i = -2; i <= c.PointCount() + 1; i++ )
        r.i( c.NextShape( i ) );

    for( int i = 0; i < c.PointCount(); i++ )
    {
        r.b( c.IsSharedPt( i ) ).b( c.IsPtOnArc( i ) ).b( c.IsArcSegment( i ) );
        r.b( c.IsArcStart( i ) ).b( c.IsArcEnd( i ) ).i( c.ArcIndex( i ) );
    }

    r.i( c.Length() ).d( c.Area( false ) ).d( c.Area() ).box( c.BBox() ).box( c.BBox( 7 ) );
}


// Chains are built with arcs (libm: ConvertToPolyline) or without
void sectionBuild( const char* aName, bool aArcs, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        SHAPE_LINE_CHAIN c = aArcs ? arcChain( rng ) : plainChain( rng );

        REC r;
        recState( r, c );
        sec.add( r );

        SHAPE_LINE_CHAIN t = c;
        t.SetClosed( !c.IsClosed() );
        REC r2;
        recState( r2, t );
        sec.add( r2 );

        t.SetClosed( c.IsClosed() );
        REC r3;
        recState( r3, t );
        sec.add( r3 );

        // the Clipper2 round trip of closed chains
        if( c.IsClosed() && c.PointCount() >= 3 && !hasCyclicDuplicate( c ) )
        {
            std::vector<CLIPPER_Z_VALUE> z;
            std::vector<SHAPE_ARC>       arcs;
            bool                         orient = rng.flag();

            if( rng.flag() )
                arcs.push_back( SHAPE_ARC( VECTOR2I( 0, 0 ), VECTOR2I( 10, 10 ), VECTOR2I( 20, 0 ), 0 ) );

            Clipper2Lib::Path64 path = c.convertToClipper2( orient, z, arcs );

            REC r4;

            for( const Clipper2Lib::Point64& p : path )
                r4.i( p.x ).i( p.y ).i( p.z );

            for( const CLIPPER_Z_VALUE& v : z )
                r4.i( v.m_FirstArcIdx ).i( v.m_SecondArcIdx );

            r4.u( arcs.size() );

            // Clipper returns paths rotated at random
            size_t rot = path.empty() ? 0 : (size_t) rng.range( 0, (int64_t) path.size() - 1 );
            std::rotate( path.begin(), path.begin() + rot, path.end() );

            SHAPE_LINE_CHAIN back( path, z, arcs );
            recState( r4, back );
            sec.add( r4 );
        }
    }
}


// One random edit of a random chain
void sectionEdit( const char* aName, bool aArcs, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        SHAPE_LINE_CHAIN c = aArcs ? arcChain( rng ) : plainChain( rng );
        int              pc = c.PointCount();
        int              op = (int) ( rng.next() % 20 );
        REC              r;
        r.i( op );

        switch( op )
        {
        case 0:
        {
            size_t   v = (size_t) rng.range( 0, pc );
            VECTOR2I p = queryPt( rng, c );
            c.Insert( v, p );
            break;
        }
        case 1:
        {
            int       err = kErr[rng.next() % 3];
            SHAPE_ARC a = chainArc( rng, false, VECTOR2I() );

            // (Insert( arc ) is only well-defined at vertex 0, see PORT_STATUS.md)
            if( pc > 0 )
                c.Insert( 0, a, err );

            break;
        }
        case 2:
        {
            if( pc == 0 )
                break;

            int      s = (int) rng.range( 0, pc - 1 );
            int      e = (int) rng.range( s, pc - 1 );
            VECTOR2I p = queryPt( rng, c );
            c.Replace( s, e, p );
            break;
        }
        case 3:
        {
            if( pc == 0 )
                break;

            int              s = (int) rng.range( 0, pc - 1 );
            int              e = (int) rng.range( s, pc - 1 );
            SHAPE_LINE_CHAIN other = anyChain( rng );

            if( other.IsClosed() )
                other.SetClosed( false );

            // Replace() of a closed chain can insert past the end of the arrays when the
            // removal merges the first and last points (UB, crashes); see PORT_STATUS.md
            if( c.IsClosed() )
            {
                c.SetClosed( false );
                r.i( c.PointCount() );
            }

            if( rng.next() % 4 == 0 && other.PointCount() > 0 )
                other.SetPoint( 0, c.CPoint( s ) );

            c.Replace( s, e, other );
            break;
        }
        case 4:
        {
            if( pc == 0 )
                break;

            int s = (int) rng.range( -pc, pc - 1 );
            int e = (int) rng.range( -pc, pc - 1 );
            c.Remove( s, e );
            break;
        }
        case 5:
        {
            int i = (int) rng.range( -pc - 1, pc );
            c.RemoveShape( i );
            break;
        }
        case 6:
        {
            if( pc == 0 )
                break;

            int      i = (int) rng.range( -pc, 2 * pc - 1 );
            VECTOR2I p = rng.ptMid();
            c.SetPoint( i, p );
            break;
        }
        case 7:
        {
            VECTOR2I p = queryPt( rng, c );
            bool     exact = rng.flag();
            r.i( c.Split( p, exact ) );
            break;
        }
        case 8: c.ClearArcs(); break;
        case 9: c.RemoveDuplicatePoints(); break;
        case 10:
        {
            const int tols[] = { 0, 1, 10, 1000, 2000000 };
            int       tol = tols[rng.next() % 5];
            c.Simplify( tol );
            break;
        }
        case 11:
        {
            bool col = rng.flag();
            c.Simplify2( col );
            break;
        }
        case 12: c = c.Reverse(); break;
        case 13:
        {
            VECTOR2I ref = rng.ptR( -10000000, 10000000 );
            bool     lr = rng.flag();
            c.Mirror( ref, lr ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
            break;
        }
        case 14:
        {
            VECTOR2I a = rng.ptR( -10000000, 10000000 );
            VECTOR2I b = rng.ptR( -10000000, 10000000 );
            c.Mirror( SEG( a, b ) );
            break;
        }
        case 15:
        {
            double   deg = rng.dbl( -360.0, 360.0 );
            VECTOR2I ctr = rng.ptR( -10000000, 10000000 );

            if( rng.flag() )
                deg = 90.0 * (double) rng.range( -4, 4 );

            c.Rotate( EDA_ANGLE( deg, DEGREES_T ), ctr );
            break;
        }
        case 16:
        {
            VECTOR2I v = rng.ptR( -10000000, 10000000 );
            c.Move( v );
            break;
        }
        case 17:
        {
            if( pc == 0 )
                break;

            ssize_t i = (ssize_t) rng.range( -pc, pc - 1 );
            bool    coincident = rng.flag();
            c.splitArc( i, coincident );
            break;
        }
        case 18:
        {
            // (an index that stays negative erases before begin(): UB)
            ssize_t i = (ssize_t) rng.range( -(int64_t) c.ArcCount(), (int64_t) c.ArcCount() );
            c.convertArc( i );
            break;
        }
        default:
        {
            // a second random edit after a first one (Remove, then Insert)
            if( pc == 0 )
                break;

            int s = (int) rng.range( 0, pc - 1 );
            c.Remove( s );
            VECTOR2I p = queryPt( rng, c );
            c.Insert( (size_t) std::min( s, c.PointCount() ), p );
            break;
        }
        }

        recState( r, c );
        sec.add( r );
    }
}


void recCollide( REC& r, const SHAPE_LINE_CHAIN& c, const VECTOR2I& p, const SEG& s, int cl )
{
    for( int k = 0; k < 4; k++ )
    {
        int      actual = -7;
        VECTOR2I loc( -7, -7 );
        bool     hit = c.Collide( p, cl, ( k & 1 ) ? &actual : nullptr, ( k & 2 ) ? &loc : nullptr );
        r.b( hit ).i( actual ).v( loc );

        actual = -7;
        loc = VECTOR2I( -7, -7 );
        hit = c.Collide( s, cl, ( k & 1 ) ? &actual : nullptr, ( k & 2 ) ? &loc : nullptr );
        r.b( hit ).i( actual ).v( loc );
    }
}


// The const queries (aArcs: chains with arcs, libm)
void sectionQuery( const char* aName, bool aArcs, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        SHAPE_LINE_CHAIN c = aArcs ? arcChain( rng ) : plainChain( rng );
        int              pc = c.PointCount();
        VECTOR2I         p = queryPt( rng, c );
        VECTOR2I         q = queryPt( rng, c );
        SEG              s( p, q );
        int              cl = clearance( rng );
        int              thrA = (int) rng.range( 0, 3 );
        int              thr = thrA * (int) rng.range( 0, 1000 );

        REC r;
        r.v( p ).v( q ).i( cl ).i( thr );
        r.i( c.Find( p ) ).i( c.Find( p, thr ) ).i( c.FindSegment( p ) ).i( c.FindSegment( p, thr ) );
        int idx = (int) rng.range( -1, pc );
        r.i( c.PathLength( p ) ).i( c.PathLength( p, idx ) );

        if( pc > 0 )
        {
            int len = (int) rng.range( 0, 100000000 );
            r.v( c.PointAlong( len ) ).v( c.PointAlong( c.PathLength( p ) ) );
        }

        r.v( c.NearestPoint( p ) );

        if( !nearestPointUB( c, p ) )
            r.v( c.NearestPoint( p, false ) );

        int dist = -9;
        r.v( c.NearestPoint( s, dist ) ).i( dist ).i( c.NearestSegment( p ) );
        r.i( c.Distance( p, false ) ).i( c.Distance( p, true ) ).i( c.SHAPE::Distance( p ) );
        r.i( c.SquaredDistance( p, false ) ).i( c.SquaredDistance( p, true ) );
        r.b( c.PointInside( p ) ).b( c.PointInside( p, thr ) ).b( c.PointOnEdge( p, thr ) );
        r.i( c.EdgeContainingPoint( p, thr ) ).b( c.CheckClearance( p, thr ) );

        SHAPE_LINE_CHAIN cached = c;
        cached.GenerateBBoxCache();
        r.b( cached.PointInside( p, 0, true ) ).b( cached.PointInside( p, thr, true ) );

        recCollide( r, c, p, s, cl );

        std::vector<SHAPE_LINE_CHAIN::INTERSECTION> ips;
        r.i( c.Intersect( s, ips ) );

        for( const auto& is : ips )
            recIs( r, is );

        r.b( c.Intersects( s ) );

        SHAPE_LINE_CHAIN other = aArcs ? arcChain( rng ) : plainChain( rng );

        for( int ex = 0; ex < 2; ex++ )
        {
            std::vector<SHAPE_LINE_CHAIN::INTERSECTION> ips2;
            r.i( c.Intersect( other, ips2, ex == 1 ) );

            for( const auto& is : ips2 )
                recIs( r, is );
        }

        BOX2I bb = other.BBox( (int) rng.range( 0, 1000 ) );
        std::vector<SHAPE_LINE_CHAIN::INTERSECTION> ips3;
        r.i( c.Intersect( other, ips3, false, &bb ) );

        for( const auto& is : ips3 )
            recIs( r, is );

        r.b( c.Intersects( other ) );

        std::optional<SHAPE_LINE_CHAIN::INTERSECTION> si = c.SelfIntersecting();
        r.b( si.has_value() );

        if( si )
            recIs( r, *si );

        if( pc >= 2 )
        {
            std::optional<SHAPE_LINE_CHAIN::INTERSECTION> sia = c.SelfIntersectingWithArcs();
            r.b( sia.has_value() );

            if( sia )
                recIs( r, *sia );
        }

        VECTOR2I a0( -5, -5 ), a1( -5, -5 );
        r.b( c.ClosestPoints( other, a0, a1 ) ).v( a0 ).v( a1 );

        if( pc > 0 && other.PointCount() > 0 )
        {
            VECTOR2I b0( -5, -5 ), b1( -5, -5 );
            r.b( c.ClosestSegmentsFast( other, b0, b1 ) ).v( b0 ).v( b1 );
        }

        r.s( c.Format() ).b( c != other ).b( c != c );
        r.b( c.CompareGeometry( other ) ).b( c.CompareGeometry( c, false, thr ) );

        SHAPE_LINE_CHAIN moved = c;
        moved.Move( rng.ptR( -2, 2 ) );
        r.b( c.CompareGeometry( moved, false, 1 ) ).b( c.CompareGeometry( c.Reverse(), true ) );
        r.b( c.CompareGeometry( other, true, thr ) );

        if( pc > 0 )
        {
            int s0 = (int) rng.range( -pc, pc - 1 );
            int s1 = (int) rng.range( -pc, pc - 1 );

            if( ( s0 < 0 ? s0 + pc : s0 ) <= ( s1 < 0 ? s1 + pc : s1 ) )
            {
                int err = kErr[rng.next() % 3];
                recChain( r, c.Slice( s0, s1 ) );
                recChain( r, c.Slice( s0, s1, err ) );
            }
        }

        if( pc >= 2 && !nearestPointUB( c, p ) && !nearestPointUB( c, q ) )
        {
            SHAPE_LINE_CHAIN pre, mid, post;
            c.Split( p, q, pre, mid, post );
            recChain( r, pre );
            recChain( r, mid );
            recChain( r, post );
        }

        // POINT_INSIDE_TRACKER on the chain split into two polylines
        if( pc >= 2 )
        {
            int                                   k = (int) rng.range( 1, pc - 1 );
            SHAPE_LINE_CHAIN::POINT_INSIDE_TRACKER tracker( p );
            SHAPE_LINE_CHAIN                       part1, part2;

            for( int i = 0; i <= k; i++ )
                part1.Append( c.CPoint( i ), true );

            for( int i = k; i < pc; i++ )
                part2.Append( c.CPoint( i ), true );

            tracker.AddPolyline( part1 );
            tracker.AddPolyline( part2 );
            r.b( tracker.IsInside() );
        }

        // Parse: the text form "n_pts closed n_arcs x y idx ... cx cy sx sy angle ..."
        {
            std::string text = std::to_string( pc ) + " " + ( c.IsClosed() ? "1" : "0" ) + " "
                               + std::to_string( c.ArcCount() );

            for( int i = 0; i < pc; i++ )
            {
                text += " " + std::to_string( c.CPoint( i ).x ) + " " + std::to_string( c.CPoint( i ).y )
                        + " " + std::to_string( c.CShapes()[i].first );
            }

            for( size_t i = 0; i < c.ArcCount(); i++ )
            {
                const SHAPE_ARC& a = c.Arc( i );
                text += " " + std::to_string( a.GetCenter().x ) + " " + std::to_string( a.GetCenter().y )
                        + " " + std::to_string( a.GetP0().x ) + " " + std::to_string( a.GetP0().y )
                        + " " + std::to_string( (int) rng.range( -3600, 3600 ) / 10 ) + ".5";
            }

            // the sanity checks of the counts (reading past the end of the text would leave
            // uninitialised variables in the C++)
            switch( rng.next() % 8 )
            {
            case 0: text = "100000 1 0 1 2 3"; break;
            case 1: text = "1 1 100000 1 2 3"; break;
            default: break;
            }

            std::stringstream ss( text );
            SHAPE_LINE_CHAIN  parsed;
            bool              ok = parsed.Parse( ss );
            r.b( ok );
            recChain( r, parsed );
        }

        sec.add( r );
    }
}


// The double to int conversions of PORTING.md section 9 (x86-64 cvttsd2si)
void sectionX86Casts()
{
    SECTION sec( "x86_casts" );
    RNG     rng( 900 );

    // SHAPE::Distance() / SHAPE_LINE_CHAIN::Distance() of chains without segments
    for( int n = 0; n < 2000; n++ )
    {
        SHAPE_LINE_CHAIN c;

        if( rng.flag() )
            c.Append( rng.ptMid() );

        VECTOR2I p = rng.ptMid();
        REC      r;
        r.i( c.SHAPE::Distance( p ) ).i( c.Distance( p, false ) ).i( c.Distance( p, true ) );
        sec.add( r );
    }

    const double specials[] = { std::nan( "" ), 2147483648.0, -2147483649.0, 2147483647.5, -2147483648.75,
                                9223372036854775808.0, -9223372036854775808.0, 1e300, -1e300,
                                std::numeric_limits<double>::infinity(), 4294967296.0, 0.5 };

    for( int n = 0; n < 4000; n++ )
    {
        double x = specials[rng.next() % 12];
        double y = specials[rng.next() % 12];

        if( rng.flag() )
            y = rng.dbl( -3e9, 3e9 );

        VECTOR2D v( x, y );
        REC      r;
        r.v( VECTOR2I( v ) ).vl( VECTOR2L( v ) ).vu( VECTOR2<unsigned>( v ) );

        VECTOR2<float> f( (float) x, (float) y );
        r.v( VECTOR2I( f ) ).vl( VECTOR2L( f ) );

        BOX2D box( VECTOR2D( 0, 0 ), VECTOR2D( x, y ) );
        r.i( box.GetSizeMax() );

        // SHAPE_ARC::ConstructFromStartEndAngle( ..., double aWidth )
        SHAPE_ARC arc;
        arc.ConstructFromStartEndAngle( VECTOR2I( 0, 0 ), VECTOR2I( 1000, 0 ), ANGLE_90, x );
        r.i( arc.GetWidth() );
        sec.add( r );
    }
}

} // namespace


int main( int argc, char** argv )
{
    if( argc == 3 && std::string( argv[1] ) == "--dump" )
        g_dump = fopen( argv[2], "w" );

    sectionLibmProbe();
    sectionBuild( "slc_build", false, 300, 15000 );
    sectionBuild( "libm:slc_build_arcs", true, 301, 15000 );
    sectionEdit( "libm:slc_edit", false, 302, 30000 );
    sectionEdit( "libm:slc_edit_arcs", true, 303, 40000 );
    sectionQuery( "libm:slc_query", false, 304, 15000 );
    sectionQuery( "libm:slc_query_arcs", true, 305, 15000 );
    sectionX86Casts();

    if( g_dump )
        fclose( g_dump );

    return 0;
}
