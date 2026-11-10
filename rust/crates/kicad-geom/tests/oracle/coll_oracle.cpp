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
 * Differential-test oracle for the collision matrix and the nearest points (package G5):
 * shape_collisions.cpp (SHAPE::Collide( const SHAPE*, ... ) with and without actual distance,
 * location and MTV) and shape_nearest_points.cpp (SHAPE::NearestPoints) for every ordered
 * pair of shape kinds: rectangle, rounded rectangle, segment, circle, arc, simple polygon,
 * line chain without and with arcs, poly set, compound, poly-set triangle, ellipse and null.
 *
 * Links the static libraries of the reference build (see build_coll_oracle.sh).  Output
 * format, generator and hashing: oracle_common.h; the Rust side is tests/oracle/coll_sections.rs,
 * driven by tests/oracle_coll_diff.rs against tests/golden/coll_oracle.txt.
 *
 * Every case has its own generator (seeded with the section seed and the case index).  Inputs
 * stay clear of the C++ undefined behaviour documented in PORT_STATUS.md: no nearly straight
 * three-point arcs except effectively straight ones (which the matrix routes to segments), and
 * those only where no code path converts their clamped radius.
 */

#include <algorithm>
#include <limits>
#include <memory>
#include <vector>

#include "oracle_common.h"

#include <geometry/shape_arc.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_compound.h>
#include <geometry/shape_ellipse.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_null.h>
#include <geometry/shape_poly_set.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <geometry/shape_simple.h>

#include <wx/debug.h>

namespace
{

enum KIND
{
    K_RECT = 0,
    K_ROUNDRECT,
    K_SEGMENT,
    K_CIRCLE,
    K_ARC,
    K_SIMPLE,
    K_CHAIN,
    K_CHAIN_ARCS,
    K_POLYSET,
    K_COMPOUND,
    K_TRIANGLE,
    K_ELLIPSE,
    K_NULL,
    K_COUNT
};


struct GEN
{
    std::unique_ptr<SHAPE>          shape;
    std::unique_ptr<SHAPE_POLY_SET> triSet; // the parent of a triangle

    const SHAPE* get() const { return shape.get(); }
};


struct OPTS
{
    bool allowStraight = true; // effectively straight arcs
    bool degenerate = false;
};


int scale( RNG& rng )
{
    switch( rng.next() % 4 )
    {
    case 0: return 20;
    case 1: return 2000;
    case 2: return 200000;
    default: return 20000000;
    }
}


int half( int S ) { return std::max( 1, S / 2 ); }


double arcAngle( RNG& rng )
{
    double sign = rng.flag() ? 1.0 : -1.0;

    switch( rng.next() % 4 )
    {
    case 0: return 90.0 * (double) rng.range( 1, 3 ) * sign;
    case 1: return rng.dbl( 0.5, 5.0 ) * sign;
    default: return rng.dbl( 5.0, 355.0 ) * sign;
    }
}


VECTOR2I nonZero( RNG& rng, int aLim )
{
    VECTOR2I d = rng.ptR( -aLim, aLim );

    if( d == VECTOR2I( 0, 0 ) )
        d = VECTOR2I( 1, 0 );

    return d;
}


SHAPE_LINE_CHAIN starRing( RNG& rng, const VECTOR2I& c, int R )
{
    int                  k = (int) rng.range( 3, 10 );
    std::vector<int64_t> ts;

    for( int i = 0; i < k; i++ )
        ts.push_back( rng.range( 0, 8LL * R - 1 ) );

    std::sort( ts.begin(), ts.end() );

    SHAPE_LINE_CHAIN ring;

    for( int64_t t : ts )
    {
        int64_t side = t / ( 2LL * R );
        int64_t off = t % ( 2LL * R ) - R;
        int64_t x, y;

        switch( side )
        {
        case 0: x = off; y = -R; break;
        case 1: x = R; y = off; break;
        case 2: x = -off; y = R; break;
        default: x = -R; y = -off; break;
        }

        int64_t f = rng.range( 30, 100 );
        ring.Append( VECTOR2I( c.x + (int) ( x * f / 100 ), c.y + (int) ( y * f / 100 ) ) );
    }

    ring.SetClosed( true );
    return ring;
}


SHAPE_LINE_CHAIN rectRing( const VECTOR2I& c, int h )
{
    SHAPE_LINE_CHAIN ring;
    ring.Append( VECTOR2I( c.x - h, c.y - h ) );
    ring.Append( VECTOR2I( c.x + h, c.y - h ) );
    ring.Append( VECTOR2I( c.x + h, c.y + h ) );
    ring.Append( VECTOR2I( c.x - h, c.y + h ) );
    ring.SetClosed( true );
    return ring;
}


// An arc from centre, start and angle.  Tiny arcs whose points coincide after rounding get a
// clamped centre (nearly straight three-point arc: the C++ overflows, UB); they are replaced by a
// quarter circle
SHAPE_ARC safeArc( const VECTOR2I& c, const VECTOR2I& s, double a, int w )
{
    SHAPE_ARC arc( c, s, EDA_ANGLE( a, DEGREES_T ), w );

    // (FIX-FAB-3) near the coordinate limits a clamped centre comes with a smaller radius: the
    // chord ends of CIRCLE::IntersectLine() then overflow int64_t (PORT_STATUS.md D11); near
    // the origin the radius test above already catches every clamped centre
    const int64_t kClamped = (int64_t) std::numeric_limits<int>::max() - 100;

    if( arc.GetRadius() >= (double) std::numeric_limits<int>::max() / 2.0
        || std::abs( (int64_t) arc.GetCenter().x ) >= kClamped
        || std::abs( (int64_t) arc.GetCenter().y ) >= kClamped )
        arc = SHAPE_ARC( c, c + VECTOR2I( 10, 0 ), EDA_ANGLE( 90.0, DEGREES_T ), w );

    return arc;
}


SHAPE_ARC genArc( RNG& rng, int S, const VECTOR2I& o, const OPTS& aOpts, bool aInChain )
{
    if( aOpts.degenerate )
    {
        VECTOR2I c = o + rng.ptR( -S, S );
        VECTOR2I d = nonZero( rng, 3 );
        double   a = arcAngle( rng );
        int      w = (int) rng.range( 0, 3 );
        return safeArc( c, c + d, a, aInChain ? 0 : w );
    }

    if( !aInChain && aOpts.allowStraight && rng.next() % 8 == 0 )
    {
        VECTOR2I s = o + rng.ptR( -S, S );
        VECTOR2I d = nonZero( rng, std::max( 1, S / 50 ) );
        int      k1 = (int) rng.range( 1, 20 );
        int      k2 = (int) rng.range( 1, 20 );
        int      w = rng.flag() ? 0 : (int) rng.range( 1, std::max( 1, S / 8 ) );
        return SHAPE_ARC( s, s + d * k1, s + d * ( k1 + k2 ), w );
    }

    VECTOR2I c = o + rng.ptR( -S, S );
    VECTOR2I d = nonZero( rng, half( S ) );
    double   a = arcAngle( rng );
    int      w = 0;

    if( !aInChain && rng.flag() )
        w = (int) rng.range( 1, std::max( 1, S / 8 ) );

    return safeArc( c, c + d, a, w );
}


GEN gen( RNG& rng, int aKind, int S, const VECTOR2I& o, const OPTS& aOpts )
{
    GEN  g;
    bool dg = aOpts.degenerate;

    switch( aKind )
    {
    case K_RECT:
    {
        VECTOR2I p = o + rng.ptR( -S, S );
        int      w, h;

        if( dg )
        {
            w = (int) rng.range( -1, 1 );
            h = (int) rng.range( -1, 1 );
        }
        else
        {
            w = (int) rng.range( -S / 4, S );
            h = (int) rng.range( -S / 4, S );
        }

        g.shape = std::make_unique<SHAPE_RECT>( p, w, h );
        break;
    }
    case K_ROUNDRECT:
    {
        VECTOR2I p = o + rng.ptR( -S, S );
        int      w = dg ? 2 : (int) rng.range( 2, std::max( 2, S ) );
        int      h = dg ? 2 : (int) rng.range( 2, std::max( 2, S ) );
        int      r = (int) rng.range( 1, std::max( 1, std::min( w, h ) / 2 ) );
        auto     rect = std::make_unique<SHAPE_RECT>( p, w, h );
        rect->SetRadius( r );
        g.shape = std::move( rect );
        break;
    }
    case K_SEGMENT:
    {
        VECTOR2I a = o + rng.ptR( -S, S );
        VECTOR2I b = a;

        if( !dg && rng.next() % 8 != 0 )
            b = a + rng.ptR( -S, S );

        int w;

        switch( rng.next() % 3 )
        {
        case 0: w = 0; break;
        case 1: w = (int) rng.range( 1, 10 ); break;
        default: w = (int) rng.range( 0, S / 4 ); break;
        }

        g.shape = std::make_unique<SHAPE_SEGMENT>( a, b, w );
        break;
    }
    case K_CIRCLE:
    {
        VECTOR2I c = o + rng.ptR( -S, S );
        int      r = ( dg || rng.next() % 4 == 0 ) ? (int) rng.range( 0, 3 ) : (int) rng.range( 0, half( S ) );
        g.shape = std::make_unique<SHAPE_CIRCLE>( c, r );
        break;
    }
    case K_ARC:
        g.shape = std::make_unique<SHAPE_ARC>( genArc( rng, S, o, aOpts, false ) );
        break;

    case K_SIMPLE:
    {
        VECTOR2I base = o + rng.ptR( -S, S );
        int      n = (int) rng.range( 3, 8 );
        auto     s = std::make_unique<SHAPE_SIMPLE>();

        for( int i = 0; i < n; i++ )
        {
            VECTOR2I d = dg ? rng.ptR( -1, 1 ) : rng.ptR( -half( S ), half( S ) );
            s->Append( base + d );
        }

        g.shape = std::move( s );
        break;
    }
    case K_CHAIN:
    {
        auto c = std::make_unique<SHAPE_LINE_CHAIN>();

        if( dg )
        {
            int      n = (int) rng.range( 0, 2 );
            VECTOR2I p = o + rng.ptR( -S, S );

            for( int i = 0; i < n; i++ )
                c->Append( p, true );
        }
        else if( rng.next() % 4 == 0 )
        {
            // a long random walk: the SEGMENT_INDEX paths
            int      n = (int) rng.range( 64, 200 );
            int      step = std::max( 1, S / 20 );
            VECTOR2I p = o + rng.ptR( -S, S );

            for( int i = 0; i < n; i++ )
            {
                c->Append( p );
                p += rng.ptR( -step, step );
            }
        }
        else
        {
            VECTOR2I base = o + rng.ptR( -S, S );
            int      n = (int) rng.range( 1, 8 );

            for( int i = 0; i < n; i++ )
            {
                VECTOR2I d = rng.ptR( -S, S );
                bool     dup = rng.flag();
                c->Append( base + d, dup );
            }
        }

        bool closed = rng.next() % 3 == 0;
        c->SetClosed( closed );
        g.shape = std::move( c );
        break;
    }
    case K_CHAIN_ARCS:
    {
        auto     c = std::make_unique<SHAPE_LINE_CHAIN>();
        VECTOR2I p = o + rng.ptR( -S, S );
        c->Append( p );
        int n = (int) rng.range( 1, 4 );

        for( int i = 0; i < n; i++ )
        {
            if( rng.flag() )
            {
                VECTOR2I q = o + rng.ptR( -S, S );
                c->Append( q );
            }
            else
            {
                VECTOR2I last = c->CLastPoint();
                VECTOR2I d = nonZero( rng, dg ? 3 : half( S ) );
                double   a = arcAngle( rng );
                c->Append( safeArc( last + d, last, a, 0 ) );
            }
        }

        bool closed = rng.next() % 3 == 0;
        c->SetClosed( closed );
        g.shape = std::move( c );
        break;
    }
    case K_POLYSET:
    {
        auto ps = std::make_unique<SHAPE_POLY_SET>();
        int  nOut = dg ? (int) rng.range( 0, 1 ) : (int) rng.range( 1, 2 );

        for( int i = 0; i < nOut; i++ )
        {
            VECTOR2I c = o + rng.ptR( -S, S );
            int      R = dg ? 1 : (int) rng.range( 1, half( S ) );

            if( rng.flag() )
            {
                ps->AddOutline( starRing( rng, c, R ) );
            }
            else
            {
                ps->AddOutline( rectRing( c, R ) );

                if( R >= 10 && rng.flag() )
                    ps->AddHole( rectRing( c + rng.ptR( -R / 4, R / 4 ), R / 5 ) );
            }
        }

        switch( rng.next() % 3 )
        {
        case 0: break;
        case 1: ps->CacheTriangulation( false ); break;
        default: ps->CacheTriangulation( false, {}, true ); break;
        }

        g.shape = std::move( ps );
        break;
    }
    case K_COMPOUND:
    {
        auto cmp = std::make_unique<SHAPE_COMPOUND>();
        int  n = dg ? (int) rng.range( 0, 1 ) : (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
        {
            int k = (int) ( rng.next() % 8 );
            GEN sub = gen( rng, k, std::max( 2, S / 2 ), o, aOpts );
            cmp->AddShape( sub.shape.release() );
        }

        g.shape = std::move( cmp );
        break;
    }
    case K_TRIANGLE:
    {
        g.triSet = std::make_unique<SHAPE_POLY_SET>();
        VECTOR2I c = o + rng.ptR( -S, S );
        int      R = dg ? 1 : (int) rng.range( 1, half( S ) );
        g.triSet->AddOutline( starRing( rng, c, R ) );
        g.triSet->CacheTriangulation();
        uint64_t pick = rng.next();

        if( g.triSet->TriangulatedPolyCount() > 0
            && g.triSet->TriangulatedPolygon( 0 )->GetTriangleCount() > 0 )
        {
            const auto* tp = g.triSet->TriangulatedPolygon( 0 );
            size_t      idx = pick % tp->GetTriangleCount();
            g.shape = std::make_unique<SHAPE_POLY_SET::TRIANGULATED_POLYGON::TRI>( tp->Triangles()[idx] );
        }
        else
        {
            g.shape = std::make_unique<SHAPE_NULL>();
        }

        break;
    }
    case K_ELLIPSE:
    {
        VECTOR2I c = o + rng.ptR( -S, S );
        int      maj = dg ? 1 : (int) rng.range( 1, half( S ) );
        int      mn = (int) rng.range( 1, maj );
        double   rot;

        switch( rng.next() % 3 )
        {
        case 0: rot = 0.0; break;
        case 1: rot = 45.0 * (double) rng.range( -8, 8 ); break;
        default: rot = rng.dbl( -360.0, 360.0 ); break;
        }

        if( rng.next() % 3 == 0 )
        {
            double sa = rng.dbl( -360.0, 360.0 );
            double ea = sa + rng.dbl( 10.0, 350.0 );
            g.shape = std::make_unique<SHAPE_ELLIPSE>( c, maj, mn, EDA_ANGLE( rot, DEGREES_T ),
                                                       EDA_ANGLE( sa, DEGREES_T ), EDA_ANGLE( ea, DEGREES_T ) );
        }
        else
        {
            g.shape = std::make_unique<SHAPE_ELLIPSE>( c, maj, mn, EDA_ANGLE( rot, DEGREES_T ) );
        }

        break;
    }
    default:
        g.shape = std::make_unique<SHAPE_NULL>();
        break;
    }

    return g;
}


int clearance( RNG& rng, int S )
{
    switch( rng.next() % 6 )
    {
    case 0:
    case 1: return 0;
    case 2: return (int) rng.range( 1, 10 );
    default: return (int) rng.range( 1, half( S ) );
    }
}


void recCollide( REC& r, const SHAPE* a, const SHAPE* b, int cl )
{
    int      actual = -7;
    VECTOR2I loc( 11, -13 );

    r.b( a->Collide( b, cl ) );
    r.b( a->Collide( b, cl, &actual ) ).i( actual );
    actual = -7;
    r.b( a->Collide( b, cl, &actual, &loc ) ).i( actual ).v( loc );
    loc = VECTOR2I( 11, -13 );
    r.b( a->Collide( b, cl, nullptr, &loc ) ).v( loc );

    VECTOR2I mtv( 7, -3 );
    r.b( a->Collide( b, cl, &mtv ) ).v( mtv );
}


void recNearest( REC& r, const SHAPE* a, const SHAPE* b )
{
    VECTOR2I pa( 5, -9 ), pb( -3, 4 );
    r.b( a->NearestPoints( b, pa, pb ) ).v( pa ).v( pb );
}


// Matrix: random pairs, both orders
void sectionMatrix( const char* aName, uint64_t aSeed, int aCount, bool aDegenerate )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG  rng( aSeed * 1000003 + n );
        int  ka = (int) ( rng.next() % K_COUNT );
        int  kb = (int) ( rng.next() % K_COUNT );
        int  S = aDegenerate ? 20 : scale( rng );
        OPTS opts;
        opts.degenerate = aDegenerate;
        opts.allowStraight = ka != K_ELLIPSE && kb != K_ELLIPSE;

        GEN      a = gen( rng, ka, S, VECTOR2I( 0, 0 ), opts );
        VECTOR2I ob = rng.flag() ? VECTOR2I( 0, 0 ) : rng.ptR( -S, S );
        GEN      b = gen( rng, kb, S, ob, opts );
        int      cl = clearance( rng, S );

        REC r;
        r.i( ka ).i( kb ).i( S ).i( cl ).box( a.get()->BBox() ).box( b.get()->BBox() );
        recCollide( r, a.get(), b.get(), cl );
        recCollide( r, b.get(), a.get(), cl );
        sec.add( r );
    }
}


// B moved so that its bounding box touches (or nearly touches) the one of A
void sectionTouching( const char* aName, uint64_t aSeed, int aCount, bool aNearest )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG  rng( aSeed * 1000003 + n );
        int  ka = (int) ( rng.next() % K_COUNT );
        int  kb = (int) ( rng.next() % K_COUNT );
        int  S = scale( rng );
        OPTS opts;
        opts.allowStraight = !aNearest && ka != K_ELLIPSE && kb != K_ELLIPSE;

        GEN      a = gen( rng, ka, S, VECTOR2I( 0, 0 ), opts );
        uint64_t seedB = rng.next();
        BOX2I    bbA = a.get()->BBox();
        BOX2I    bbB;

        {
            RNG rb( seedB );
            GEN b0 = gen( rb, kb, S, VECTOR2I( 0, 0 ), opts );
            bbB = b0.get()->BBox();
        }

        int64_t j = rng.range( -2, 2 );
        int64_t t = rng.range( -S / 2, S / 2 );
        int64_t dx, dy;

        switch( rng.next() % 5 )
        {
        case 0: dx = (int64_t) bbA.GetRight() - bbB.GetLeft() + j; dy = t; break;
        case 1: dx = (int64_t) bbA.GetLeft() - bbB.GetRight() + j; dy = t; break;
        case 2: dx = t; dy = (int64_t) bbA.GetBottom() - bbB.GetTop() + j; break;
        case 3: dx = t; dy = (int64_t) bbA.GetTop() - bbB.GetBottom() + j; break;
        default:
            dx = (int64_t) bbA.GetRight() - bbB.GetLeft() + j;
            dy = (int64_t) bbA.GetBottom() - bbB.GetTop() + rng.range( -2, 2 );
            break;
        }

        dx = std::clamp<int64_t>( dx, -100000000, 100000000 );
        dy = std::clamp<int64_t>( dy, -100000000, 100000000 );

        RNG rb( seedB );
        GEN b = gen( rb, kb, S, VECTOR2I( (int) dx, (int) dy ), opts );
        int cl = clearance( rng, S );

        REC r;
        r.i( ka ).i( kb ).i( S ).i( cl ).box( a.get()->BBox() ).box( b.get()->BBox() );

        if( aNearest )
        {
            recNearest( r, a.get(), b.get() );
            recNearest( r, b.get(), a.get() );
        }
        else
        {
            recCollide( r, a.get(), b.get(), cl );
            recCollide( r, b.get(), a.get(), cl );
        }

        sec.add( r );
    }
}


// NearestPoints on random pairs (no effectively straight arcs: their radius does not convert)
void sectionNearest( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG  rng( aSeed * 1000003 + n );
        int  ka = (int) ( rng.next() % K_COUNT );
        int  kb = (int) ( rng.next() % K_COUNT );
        int  S = scale( rng );
        OPTS opts;
        opts.allowStraight = false;

        GEN      a = gen( rng, ka, S, VECTOR2I( 0, 0 ), opts );
        VECTOR2I ob = rng.flag() ? VECTOR2I( 0, 0 ) : rng.ptR( -S, S );
        GEN      b = gen( rng, kb, S, ob, opts );

        REC r;
        r.i( ka ).i( kb ).i( S ).box( a.get()->BBox() ).box( b.get()->BBox() );
        recNearest( r, a.get(), b.get() );
        recNearest( r, b.get(), a.get() );
        sec.add( r );
    }
}


SHAPE_LINE_CHAIN walk( RNG& rng, const VECTOR2I& aStart, int aCount, int aStep )
{
    SHAPE_LINE_CHAIN c;
    VECTOR2I         p = aStart;

    for( int i = 0; i < aCount; i++ )
    {
        c.Append( p );
        p += rng.ptR( -aStep, aStep );
    }

    return c;
}


// Long line chains against each other: the SEGMENT_INDEX gates of
// Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )
void sectionChainIndex( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG              rng( aSeed * 1000003 + n );
        int              step = (int) rng.range( 1, 100000 );
        int              na = (int) rng.range( 20, 300 );
        int              nb = (int) rng.range( 20, 300 );
        VECTOR2I         sa = rng.ptR( -10 * step, 10 * step );
        SHAPE_LINE_CHAIN a = walk( rng, sa, na, step );
        SHAPE_LINE_CHAIN b;

        if( rng.next() % 4 == 0 )
        {
            // a shifted copy of a part of A
            int      from = (int) rng.range( 0, na - 1 );
            int      cnt = (int) rng.range( 1, na - from );
            VECTOR2I sh = rng.ptR( -2, 2 );

            for( int i = from; i < from + cnt; i++ )
                b.Append( a.CPoint( i ) + sh );
        }
        else
        {
            VECTOR2I sb = rng.ptR( -10 * step, 10 * step );
            b = walk( rng, sb, nb, step );
        }

        bool ca = rng.next() % 4 == 0;
        bool cb = rng.next() % 4 == 0;
        a.SetClosed( ca );
        b.SetClosed( cb );

        int cl;

        switch( rng.next() % 5 )
        {
        case 0: cl = -1; break;
        case 1: cl = 0; break;
        case 2: cl = (int) rng.range( 0, step ); break;
        case 3: cl = (int) rng.range( 0, 10 * step ); break;
        default: cl = (int) rng.range( 0, 100 * step ); break;
        }

        REC r;
        r.i( na ).i( b.PointCount() ).i( cl ).b( ca ).b( cb );
        recCollide( r, &a, &b, cl );
        recCollide( r, &b, &a, cl );
        recNearest( r, &a, &b );
        sec.add( r );
    }
}

// FIX-FAB-3: touching pairs near the far corners of the coordinate range (centres at
// +/-1.2e9 and +/-2.0e9 nm), where the int sums of two points (( aA.GetCenter() +
// aB.GetCenter() ) / 2, ( ptA + ptB ) / 2) wrap in the reference build.  Every case is
// recorded with and without the out-parameters (recCollide).  Scale and offsets are limited so
// that the generated shapes themselves stay inside the int range.
VECTOR2I farOrigin( RNG& rng )
{
    static const int c[] = { 1200000000, -1200000000, 2000000000, -2000000000 };
    int x = c[rng.next() % 4];
    int y = c[rng.next() % 4];
    return VECTOR2I( x, y );
}


void sectionFar( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );

    for( int n = 0; n < aCount; n++ )
    {
        RNG      rng( aSeed * 1000003 + n );
        int      ka = (int) ( rng.next() % K_COUNT );
        int      kb = (int) ( rng.next() % K_COUNT );
        VECTOR2I o = farOrigin( rng );
        int      S = std::min( scale( rng ), 200000 );
        OPTS     opts;
        // no effectively straight arcs: their clamped centre is far outside the int range here
        // (SEG::LineProject( centre ) overflows, PORT_STATUS.md D11)
        opts.allowStraight = false;

        GEN      a = gen( rng, ka, S, o, opts );
        uint64_t seedB = rng.next();
        BOX2I    bbA = a.get()->BBox();
        BOX2I    bbB;

        {
            RNG rb( seedB );
            GEN b0 = gen( rb, kb, S, o, opts );
            bbB = b0.get()->BBox();
        }

        int64_t j = rng.range( -2, 2 );
        int64_t t = rng.range( -S / 2, S / 2 );
        int64_t dx, dy;

        switch( rng.next() % 5 )
        {
        case 0: dx = (int64_t) bbA.GetRight() - bbB.GetLeft() + j; dy = t; break;
        case 1: dx = (int64_t) bbA.GetLeft() - bbB.GetRight() + j; dy = t; break;
        case 2: dx = t; dy = (int64_t) bbA.GetBottom() - bbB.GetTop() + j; break;
        case 3: dx = t; dy = (int64_t) bbA.GetTop() - bbB.GetBottom() + j; break;
        default:
            dx = (int64_t) bbA.GetRight() - bbB.GetLeft() + j;
            dy = (int64_t) bbA.GetBottom() - bbB.GetTop() + rng.range( -2, 2 );
            break;
        }

        dx = std::clamp<int64_t>( dx, -2000000, 2000000 );
        dy = std::clamp<int64_t>( dy, -2000000, 2000000 );

        RNG rb( seedB );
        GEN b = gen( rb, kb, S, o + VECTOR2I( (int) dx, (int) dy ), opts );
        int cl = clearance( rng, S );

        REC r;
        r.i( ka ).i( kb ).i( S ).i( cl ).v( o ).box( a.get()->BBox() ).box( b.get()->BBox() );
        recCollide( r, a.get(), b.get(), cl );
        recCollide( r, b.get(), a.get(), cl );
        sec.add( r );
    }
}


// FIX-FAB-3: shapes on opposite sides of the coordinate range: the int differences of points
// of the two shapes (delta of two circle centres, pn - c of rectangle and circle, the
// pushout force) wrap in the reference build, and with large radii the wrapped distance
// reports a collision.  (Segment against circle is not included: SHAPE_CIRCLE::Collide( SEG )
// overflows there, PORT_STATUS.md known gaps.)
void sectionFarApart( const char* aName )
{
    SECTION sec( aName );

    const int far[] = { 1200000000, -1200000000, 2000000000, -2000000000, 0 };
    const int radii[] = { 0, 1000, 300000000, 1000000000 };

    for( int xa : far )
    {
        for( int xb : far )
        {
            for( int ra : radii )
            {
                for( int rb : radii )
                {
                    for( int cl : { 0, 100000 } )
                    {
                        VECTOR2I     pa( xa, xa / 2 );
                        VECTOR2I     pb( xb, -xb / 3 );
                        SHAPE_CIRCLE ca( pa, ra );
                        SHAPE_CIRCLE cb( pb, rb );
                        SHAPE_RECT   rect( pa - VECTOR2I( 1000, 1000 ), 2000, 3000 );

                        REC r;
                        r.i( xa ).i( xb ).i( ra ).i( rb ).i( cl );
                        recCollide( r, &ca, &cb, cl );
                        recCollide( r, &cb, &ca, cl );
                        recCollide( r, &rect, &cb, cl );
                        recCollide( r, &cb, &rect, cl );
                        sec.add( r );
                    }
                }
            }
        }
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
    sectionMatrix( "libm:coll_matrix", 600, 100000, false );
    sectionTouching( "libm:coll_touching", 601, 40000, false );
    sectionMatrix( "libm:coll_degenerate", 602, 20000, true );
    sectionNearest( "libm:coll_nearest", 603, 60000 );
    sectionTouching( "libm:coll_nearest_touching", 604, 20000, true );
    sectionChainIndex( "coll_chain_index", 605, 5000 );
    sectionFar( "libm:coll_far", 606, 20000 );
    sectionFarApart( "libm:coll_far_apart" );

    if( g_dump )
        fclose( g_dump );

    return 0;
}
