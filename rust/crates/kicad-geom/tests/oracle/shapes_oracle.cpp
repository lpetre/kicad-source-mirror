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
 * Differential-test oracle for the shapes of the Rust port (crate kicad-geom, package G2):
 * SHAPE_ARC, CIRCLE, SHAPE_RECT / SEGMENT / CIRCLE / SIMPLE / NULL / COMPOUND, the line chain
 * skeleton, geometry_utils, HALF_LINE, LINE, ARC_CHORD_PARAMS and arc_construction.
 *
 * Unlike geom_oracle.cpp this program links the static libraries of the reference build
 * (libkimath.a and friends, see build_shapes_oracle.sh), so the collision code under test is
 * the real one.  Output format, generator and hashing are those of geom_oracle.cpp
 * (oracle_common.h); the Rust side is tests/oracle/shape_sections.rs, driven by
 * tests/oracle_shapes_diff.rs against tests/golden/shapes_oracle.txt.
 *
 * Inputs stay clear of undefined behaviour in the C++ (e.g. double to int conversions of out
 * of range arc radii), see PORT_STATUS.md.
 */

#include <limits>
#include <memory>
#include <vector>

#include "oracle_common.h"

#include <geometry/arc_chord_params.h>
#include <geometry/arc_construction.h>
#include <geometry/circle.h>
#include <geometry/geometry_utils.h>
#include <geometry/half_line.h>
#include <geometry/line.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_compound.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_null.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <geometry/shape_simple.h>
#include <geometry/shape_utils.h>
#include <math/box2_minmax.h>

namespace
{

const double kLargeRadius = (double) std::numeric_limits<int>::max() / 2.0;

int width( RNG& rng )
{
    switch( rng.next() % 4 )
    {
    case 0: return 0;
    case 1: return (int) rng.range( 1, 10 );
    case 2: return (int) rng.range( 0, 1000000 );
    default: return (int) rng.range( 0, 5000000 );
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

// An arc with points within about 1e8 of the origin
SHAPE_ARC randArc( RNG& rng )
{
    switch( rng.next() % 5 )
    {
    case 0:
    case 1:
    {
        VECTOR2I s, m, e;
        arcPoints( rng, s, m, e );
        int w = width( rng );
        return SHAPE_ARC( s, m, e, w );
    }
    case 2:
    {
        VECTOR2I c = rng.ptMid();
        VECTOR2I s = rng.ptMid();
        double   a = rng.angleDeg();
        int      w = width( rng );
        return SHAPE_ARC( c, s, EDA_ANGLE( a, DEGREES_T ), w );
    }
    case 3:
    {
        VECTOR2I c = rng.ptR( -1000, 1000 );
        VECTOR2I d = rng.ptR( -50, 50 );
        double   a = rng.angleDeg();
        int      w = (int) rng.range( 0, 20 );
        return SHAPE_ARC( c, c + d, EDA_ANGLE( a, DEGREES_T ), w );
    }
    default:
    {
        // The angle stays away from multiples of 360 degrees: the center of a nearly straight
        // arc is clamped near the int limits and RotatePoint() would overflow (UB)
        VECTOR2I  s = rng.ptR( -10000000, 10000000 );
        VECTOR2I  e = rng.ptR( -10000000, 10000000 );
        double    a = rng.dbl( 5.0, 355.0 ) * ( rng.flag() ? 1.0 : -1.0 );
        int       w = width( rng );
        SHAPE_ARC arc;
        arc.ConstructFromStartEndAngle( s, e, EDA_ANGLE( a, DEGREES_T ), w );
        return arc;
    }
    }
}

// Whether moving / rotating / mirroring the arc by the amounts used here keeps the quadrant
// points of SHAPE_ARC::update_values() inside the int range (otherwise the C++ overflows: UB)
bool arcMovable( const SHAPE_ARC& a )
{
    return a.GetRadius() >= kLargeRadius
           || std::abs( (double) a.GetCenter().x ) + std::abs( (double) a.GetCenter().y ) + 3.0 * a.GetRadius()
                      < 1.6e9;
}

// A point near the arc (or anywhere)
VECTOR2I nearArc( RNG& rng, const SHAPE_ARC& aArc )
{
    switch( rng.next() % 5 )
    {
    case 0: { VECTOR2I d = rng.ptR( -1000, 1000 ); return aArc.GetP0() + d; }
    case 1: { VECTOR2I d = rng.ptR( -1000, 1000 ); return aArc.GetP1() + d; }
    case 2: { VECTOR2I d = rng.ptR( -100000, 100000 ); return aArc.GetArcMid() + d; }
    case 3:
    {
        VECTOR2I d = rng.ptR( -10, 10 );
        return ( aArc.GetRadius() < kLargeRadius ) ? aArc.GetCenter() + d : aArc.GetArcMid() + d;
    }
    default: return rng.ptMid();
    }
}

// Format() of a shape; SHAPE_LINE_CHAIN::Format() belongs to package G3 and is left out
std::string fmt( const SHAPE* a, bool aCplusPlus = true )
{
    return a->Type() == SH_LINE_CHAIN ? std::string() : a->Format( aCplusPlus );
}

REC& recArc( REC& r, const SHAPE_ARC& a )
{
    r.v( a.GetP0() ).v( a.GetArcMid() ).v( a.GetP1() ).i( a.GetWidth() ).v( a.GetCenter() );
    r.d( a.GetRadius() ).box( a.BBox() );
    return r;
}

REC& recPts( REC& r, const std::vector<VECTOR2I>& aPts )
{
    r.i( (int64_t) aPts.size() );

    for( const VECTOR2I& p : aPts )
        r.v( p );

    return r;
}

REC& recChain( REC& r, const SHAPE_LINE_CHAIN& c )
{
    r.i( c.PointCount() ).b( c.IsClosed() ).i( (int64_t) c.ArcCount() );

    for( int i = 0; i < c.PointCount(); i++ )
        r.v( c.CPoint( i ) ).i( (int64_t) c.CShapes()[i].first ).i( (int64_t) c.CShapes()[i].second );

    return r;
}

CIRCLE randCircle( RNG& rng )
{
    VECTOR2I c = rng.ptMid();
    int      r;

    switch( rng.next() % 3 )
    {
    case 0: r = (int) rng.range( 0, 20 ); break;
    case 1: r = (int) rng.range( 0, 100000 ); break;
    default: r = (int) rng.range( 0, 100000000 ); break;
    }

    return CIRCLE( c, r );
}

SEG segNear( RNG& rng, const VECTOR2I& aP )
{
    VECTOR2I a = rng.ptR( -2000000, 2000000 );
    VECTOR2I b = rng.ptR( -2000000, 2000000 );

    if( rng.flag() )
        return SEG( aP + a, aP + b );

    VECTOR2I c = rng.ptMid();
    return SEG( aP + a, c );
}

SHAPE_SIMPLE randSimple( RNG& rng )
{
    VECTOR2I     base = rng.ptMid();
    int          n = (int) rng.range( 3, 8 );
    int          spread = ( rng.flag() ) ? 1000 : 3000000;
    SHAPE_SIMPLE s;

    for( int i = 0; i < n; i++ )
    {
        VECTOR2I d = rng.ptR( -spread, spread );
        s.Append( base + d );
    }

    return s;
}

SHAPE_LINE_CHAIN randChain( RNG& rng )
{
    VECTOR2I         base = rng.ptMid();
    int              n = (int) rng.range( 1, 8 );
    SHAPE_LINE_CHAIN c;

    for( int i = 0; i < n; i++ )
    {
        VECTOR2I d = rng.ptR( -3000000, 3000000 );
        bool     dup = rng.flag();
        c.Append( base + d, dup );
    }

    bool closed = rng.flag();
    c.SetClosed( closed );
    return c;
}

// A shape for the non-arc collision matrix: 0 rect, 1 segment, 2 circle, 3 simple, 4 null,
// 5 compound of segments and circles, 6 line chain
std::unique_ptr<SHAPE> randShape( RNG& rng, int aKind )
{
    switch( aKind )
    {
    case 0:
    {
        VECTOR2I p = rng.ptMid();
        VECTOR2I s = rng.ptR( -5000000, 5000000 );
        return std::make_unique<SHAPE_RECT>( p, s.x, s.y );
    }
    case 1:
    {
        VECTOR2I a = rng.ptMid();
        VECTOR2I d = rng.ptR( -5000000, 5000000 );
        int      w = width( rng );
        return std::make_unique<SHAPE_SEGMENT>( a, a + d, w );
    }
    case 2:
    {
        VECTOR2I c = rng.ptMid();
        int      r = (int) rng.range( 0, 5000000 );
        return std::make_unique<SHAPE_CIRCLE>( c, r );
    }
    case 3: return std::make_unique<SHAPE_SIMPLE>( randSimple( rng ) );
    case 4: return std::make_unique<SHAPE_NULL>();
    case 5:
    {
        auto cmp = std::make_unique<SHAPE_COMPOUND>();
        int  n = (int) rng.range( 1, 3 );

        for( int i = 0; i < n; i++ )
        {
            int k = rng.flag() ? 1 : 2;
            cmp->AddShape( randShape( rng, k ).release() );
        }

        return cmp;
    }
    default: return std::make_unique<SHAPE_LINE_CHAIN>( randChain( rng ) );
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
}

void recCollideSeg( REC& r, const SHAPE* a, const SEG& s, int cl )
{
    int      actual = -7;
    VECTOR2I loc( 11, -13 );

    r.b( a->Collide( s, cl ) );
    r.b( a->Collide( s, cl, &actual ) ).i( actual );
    actual = -7;
    r.b( a->Collide( s, cl, &actual, &loc ) ).i( actual ).v( loc );
    loc = VECTOR2I( 11, -13 );
    r.b( a->Collide( s, cl, nullptr, &loc ) ).v( loc );
}

void recCollidePt( REC& r, const SHAPE* a, const VECTOR2I& p, int cl )
{
    int      actual = -7;
    VECTOR2I loc( 11, -13 );

    r.b( a->Collide( p, cl ) );
    r.b( a->Collide( p, cl, &actual ) ).i( actual );
    actual = -7;
    r.b( a->Collide( p, cl, &actual, &loc ) ).i( actual ).v( loc );
    loc = VECTOR2I( 11, -13 );
    r.b( a->Collide( p, cl, nullptr, &loc ) ).v( loc );
}


void sectionCircle()
{
    SECTION sec( "libm:circle" );
    RNG     rng( 201 );

    for( int n = 0; n < 30000; n++ )
    {
        CIRCLE   c = randCircle( rng );
        CIRCLE   c2 = randCircle( rng );
        VECTOR2I p = rng.pt();
        VECTOR2D pd = rng.ptD( -2e8, 2e8 );
        SEG      s = segNear( rng, c.Center );

        if( rng.next() % 4 == 0 )
        {
            // near-tangent and concentric circles
            VECTOR2I d = rng.ptR( -3, 3 );
            int      dr = (int) rng.range( -3, 3 );
            c2 = CIRCLE( c.Center + d, std::max( 0, c.Radius + dr ) );
        }

        REC r;
        r.v( c.Center ).i( c.Radius ).v( c2.Center ).i( c2.Radius ).v( p ).vd( pd ).seg( s );

        const CIRCLE& cc = c;
        r.b( cc.Contains( p ) ).b( c.Contains( p ) );
        r.v( c.NearestPoint( p ) ).vd( c.NearestPoint( pd ) );
        r.v( c.FurthestPoint( p ) ).vd( c.FurthestPoint( pd ) );
        recPts( r, c.Intersect( c2 ) );
        recPts( r, c.Intersect( s ) );
        recPts( r, c.IntersectLine( s ) );
        sec.add( r );
    }
}


void sectionCircleTanTanPt()
{
    SECTION sec( "libm:circle_tan_tan_pt" );
    RNG     rng( 202 );

    for( int n = 0; n < 5000; n++ )
    {
        VECTOR2I ip = rng.ptR( -1000000, 1000000 );
        VECTOR2I da = rng.ptR( -1000000, 1000000 );
        VECTOR2I db = rng.ptR( -1000000, 1000000 );
        VECTOR2I p = rng.ptR( -1000000, 1000000 );
        SEG      a( ip, ip + da );
        SEG      b( ip, ip + db );

        if( rng.next() % 4 == 0 )
            b = SEG( ip + db, ip + db + da ); // parallel

        CIRCLE circle;
        circle.ConstructFromTanTanPt( a, b, p );

        REC r;
        r.seg( a ).seg( b ).v( p ).v( circle.Center ).i( circle.Radius );
        sec.add( r );
    }
}


void sectionArcProps()
{
    SECTION sec( "libm:arc_props" );
    RNG     rng( 203 );

    for( int n = 0; n < 30000; n++ )
    {
        SHAPE_ARC a;

        switch( rng.next() % 6 )
        {
        case 0:
        {
            VECTOR2I ip = rng.ptR( -1000000, 1000000 );
            VECTOR2I da = rng.ptR( -1000000, 1000000 );
            VECTOR2I db = rng.ptR( -1000000, 1000000 );
            int      rad = (int) rng.range( 0, 3000000 );
            int      w = width( rng );

            // Nearly parallel segments put the center at infinity and overflow ints (UB)
            double cr = (double) da.x * db.y - (double) da.y * db.x;
            double la = (double) da.x * da.x + (double) da.y * da.y;
            double lb = (double) db.x * db.x + (double) db.y * db.y;

            if( cr * cr < 0.01 * la * lb )
                continue;

            a = SHAPE_ARC( SEG( ip, ip + da ), SEG( ip, ip + db ), rad, w );
            break;
        }
        case 1:
        {
            VECTOR2I s = rng.ptMid();
            VECTOR2I e = rng.ptMid();
            VECTOR2I c = rng.ptMid();
            bool     cw = rng.flag();
            a.ConstructFromStartEndCenter( s, e, c, cw );
            break;
        }
        default: a = randArc( rng ); break;
        }

        int cl = clearance( rng );

        REC r;
        recArc( r, a ).box( a.BBox( cl ) );
        r.a( a.GetCentralAngle() ).a( a.GetStartAngle() ).a( a.GetEndAngle() ).d( a.GetLength() );
        r.b( a.IsCCW() ).b( a.IsClockwise() ).b( a.IsEffectiveLine() ).seg( a.GetChord() );
        recArc( r, a.Reversed() );

        SHAPE_ARC rev( a );
        rev.Reverse();
        recArc( r, rev ).a( rev.GetCentralAngle() );

        SHAPE_ARC w2( a, 1234 );
        r.i( w2.GetWidth() ).b( w2 == a );

        BOX2I_MINMAX mm( a );
        r.i( mm.m_Left ).i( mm.m_Top ).i( mm.m_Right ).i( mm.m_Bottom );
        sec.add( r );
    }
}


void sectionArcPolyline()
{
    SECTION sec( "libm:arc_polyline" );
    RNG     rng( 204 );

    for( int n = 0; n < 12000; n++ )
    {
        SHAPE_ARC a = randArc( rng );
        int       err;

        switch( rng.next() % 4 )
        {
        case 0: err = 5000; break;
        case 1: err = 1000; break;
        case 2: err = (int) rng.range( 100, 100000 ); break;
        default: err = (int) rng.range( -10, 10 ); break;
        }

        // Out of range double to int conversions are undefined behaviour in the C++
        if( a.GetRadius() + a.GetWidth() / 2.0 >= kLargeRadius )
            continue;

        // Keep the polylines reasonably short
        if( err < 100 && a.GetRadius() > 100000 )
            err = 100;

        int              actual = -1;
        SHAPE_LINE_CHAIN pl = a.ConvertToPolyline( err, &actual );

        REC r;
        recArc( r, a ).i( err ).i( actual );
        recChain( r, pl );

        SHAPE_LINE_CHAIN chain( a );
        recChain( r, chain ).box( chain.BBox() );
        sec.add( r );
    }
}


void sectionArcCollide()
{
    SECTION sec( "libm:arc_collide" );
    RNG     rng( 205 );

    for( int n = 0; n < 40000; n++ )
    {
        SHAPE_ARC a = randArc( rng );
        VECTOR2I  p = nearArc( rng, a );
        VECTOR2I  q = nearArc( rng, a );
        int       cl = clearance( rng );

        SEG s( p, q );

        if( rng.next() % 8 == 0 )
            s = SEG( p, p );

        REC r;
        recArc( r, a ).v( p ).seg( s ).i( cl );
        recCollidePt( r, &a, p, cl );
        recCollideSeg( r, &a, s, cl );
        sec.add( r );
    }
}


void sectionArcIntersect()
{
    SECTION sec( "libm:arc_intersect" );
    RNG     rng( 206 );

    for( int n = 0; n < 20000; n++ )
    {
        SHAPE_ARC a = randArc( rng );
        SHAPE_ARC b = randArc( rng );

        if( rng.flag() )
        {
            // move the other arc close
            VECTOR2I d = rng.ptR( -2000000, 2000000 );

            if( arcMovable( b ) )
                b.Move( a.GetArcMid() - b.GetArcMid() + d );
        }

        VECTOR2I p = nearArc( rng, a );
        SEG      s( nearArc( rng, a ), p );
        CIRCLE   c = randCircle( rng );

        if( rng.flag() )
            c.Center = a.GetArcMid() + rng.ptR( -1000000, 1000000 );

        REC r;
        recArc( r, a );
        recArc( r, b ).v( p ).seg( s ).v( c.Center ).i( c.Radius );

        std::vector<VECTOR2I> ips = { VECTOR2I( 1, 2 ) };
        r.i( a.IntersectLine( s, &ips ) );
        r.i( a.Intersect( c, &ips ) );
        r.i( a.Intersect( b, &ips ) );
        recPts( r, ips );

        // Out of range double to int conversions are undefined behaviour in the C++
        if( a.GetRadius() < kLargeRadius && b.GetRadius() < kLargeRadius )
        {
            r.v( a.NearestPoint( p ) );

            VECTOR2I pa, pb;
            int64_t  d2 = -1;
            r.b( a.NearestPoints( s, pa, pb, d2 ) ).v( pa ).v( pb ).i( d2 );

            SHAPE_CIRCLE sc( c );
            pa = pb = VECTOR2I();
            d2 = -1;
            r.b( a.NearestPoints( sc, pa, pb, d2 ) ).v( pa ).v( pb ).i( d2 );

            pa = pb = VECTOR2I();
            d2 = -1;
            r.b( a.NearestPoints( b, pa, pb, d2 ) ).v( pa ).v( pb ).i( d2 );
        }

        sec.add( r );
    }
}


void sectionArcTransform()
{
    SECTION sec( "libm:arc_transform" );
    RNG     rng( 207 );

    for( int n = 0; n < 10000; n++ )
    {
        SHAPE_ARC a = randArc( rng );
        VECTOR2I  v = rng.ptR( -1000000, 1000000 );
        VECTOR2I  c = rng.ptMid();
        double    ang = rng.angleDeg();
        VECTOR2I  ref = rng.ptMid();
        bool      lr = rng.flag();
        SEG       axis = segNear( rng, ref );

        if( !arcMovable( a ) )
            continue;

        REC r;
        recArc( r, a );

        SHAPE_ARC m( a );
        m.Move( v );
        recArc( r, m );

        m = a;
        m.Rotate( EDA_ANGLE( ang, DEGREES_T ), c );
        recArc( r, m );

        m = a;
        m.Mirror( ref, lr ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
        recArc( r, m );

        m = a;
        m.Mirror( axis );
        recArc( r, m );
        sec.add( r );
    }
}


void sectionShapeCollide()
{
    SECTION sec( "shape_collide" );
    RNG     rng( 208 );

    for( int n = 0; n < 60000; n++ )
    {
        int ka = (int) ( rng.next() % 7 );
        int kb = (int) ( rng.next() % 7 );

        std::unique_ptr<SHAPE> a = randShape( rng, ka );
        std::unique_ptr<SHAPE> b = randShape( rng, kb );

        if( rng.flag() && kb != 4 && ka != 4 )
        {
            // move b next to a
            VECTOR2I d = rng.ptR( -3000000, 3000000 );
            b->Move( a->Centre() - b->Centre() + d );
        }

        int cl = clearance( rng );

        bool lcbA = ka == 3 || ka == 6;
        bool lcbB = kb == 3 || kb == 6;

        REC r;
        r.i( ka ).i( kb ).s( fmt( a.get() ) ).s( fmt( b.get() ) ).i( cl );

        if( lcbA && lcbB )
        {
            // SHAPE_LINE_CHAIN_BASE against SHAPE_LINE_CHAIN_BASE: package G5
        }
        else if( ka == 0 && kb == 0 )
        {
            r.b( a->Collide( b.get(), 0 ) );
        }
        else
        {
            recCollide( r, a.get(), b.get(), cl );

            // the MTV variants that do not assert
            bool mtvOk = ( ka == 2 && kb == 2 ) || ( ka == 0 && kb == 2 ) || ( ka == 2 && kb == 0 )
                         || ( ka == 2 && lcbB ) || ( ka == 2 && kb == 1 ) || ( ka == 1 && kb == 2 );

            if( mtvOk )
            {
                VECTOR2I mtv( 7, -3 );
                r.b( a->Collide( b.get(), cl, &mtv ) ).v( mtv );
            }

            r.i( a->GetClearance( b.get() ) );
        }

        sec.add( r );
    }
}


std::unique_ptr<SHAPE> randArcPartner( RNG& rng, const SHAPE_ARC& aArc, int& aKind )
{
    aKind = (int) ( rng.next() % 6 );
    std::unique_ptr<SHAPE> s;
    bool                   movable = true;

    switch( aKind )
    {
    case 0: s = randShape( rng, 2 ); break;
    case 1: s = randShape( rng, 1 ); break;
    case 2: s = randShape( rng, 3 ); break;
    case 3: s = randShape( rng, 6 ); break;
    case 4:
    {
        SHAPE_ARC b = randArc( rng );

        if( b.GetRadius() >= kLargeRadius )
            b = SHAPE_ARC( VECTOR2I( 0, 0 ), VECTOR2I( 100, 100 ), VECTOR2I( 200, 0 ), 0 );

        movable = arcMovable( b );
        auto chain = std::make_unique<SHAPE_LINE_CHAIN>( b );
        VECTOR2I d = rng.ptR( -3000000, 3000000 );
        chain->Append( b.GetP1() + d );
        bool closed = rng.flag();
        chain->SetClosed( closed );
        s = std::move( chain );
        break;
    }
    default:
    {
        SHAPE_ARC b = randArc( rng );
        movable = arcMovable( b );
        s = std::make_unique<SHAPE_ARC>( b );
        break;
    }
    }

    if( rng.flag() )
    {
        VECTOR2I d = rng.ptR( -3000000, 3000000 );

        if( movable )
            s->Move( aArc.GetArcMid() - s->Centre() + d );
    }

    return s;
}


void sectionShapeCollideArc()
{
    SECTION sec( "libm:shape_collide_arc" );
    RNG     rng( 209 );

    for( int n = 0; n < 30000; n++ )
    {
        SHAPE_ARC a = randArc( rng );
        int       kind = 0;

        std::unique_ptr<SHAPE> b = randArcPartner( rng, a, kind );
        int                    cl = clearance( rng );

        // The arcs of a chain must have a radius that converts to int (see sectionArcPolyline);
        // NearestPoints() of large arcs converts out of range radii as well
        if( a.GetRadius() >= kLargeRadius )
            continue;

        if( b->Type() == SH_ARC && static_cast<SHAPE_ARC*>( b.get() )->GetRadius() >= kLargeRadius )
            continue;

        REC r;
        recArc( r, a ).i( kind ).s( fmt( b.get() ) ).i( cl );
        recCollide( r, &a, b.get(), cl );
        recCollide( r, b.get(), &a, cl );

        if( kind == 0 )
        {
            VECTOR2I mtv( 7, -3 );
            r.b( static_cast<const SHAPE&>( a ).Collide( b.get(), cl, &mtv ) ).v( mtv );
            mtv = VECTOR2I( 7, -3 );
            r.b( b->Collide( &a, cl, &mtv ) ).v( mtv );
        }

        if( kind != 3 && kind != 4 )
            r.i( a.GetClearance( b.get() ) ).i( b->GetClearance( &a ) );

        sec.add( r );
    }

    // effectively straight arcs against rectangles
    for( int n = 0; n < 5000; n++ )
    {
        VECTOR2I s = rng.ptMid();
        VECTOR2I d = rng.ptR( -1000, 1000 );
        int      k1 = (int) rng.range( 1, 1000 );
        int      k2 = (int) rng.range( 1, 1000 );
        int      w = width( rng );

        SHAPE_ARC a( s, s + d * k1, s + d * ( k1 + k2 ), w );

        if( !a.IsEffectiveLine() )
            continue;

        std::unique_ptr<SHAPE> b = randShape( rng, 0 );

        if( rng.flag() )
        {
            VECTOR2I dd = rng.ptR( -3000000, 3000000 );
            b->Move( a.GetArcMid() - b->Centre() + dd );
        }

        int cl = clearance( rng );

        REC r;
        recArc( r, a ).s( b->Format() ).i( cl );
        recCollide( r, &a, b.get(), cl );
        recCollide( r, b.get(), &a, cl );
        sec.add( r );
    }
}


void sectionShapeDistance()
{
    SECTION sec( "shape_distance" );
    RNG     rng( 210 );

    for( int n = 0; n < 30000; n++ )
    {
        int                    kind = (int) ( rng.next() % 7 );
        std::unique_ptr<SHAPE> a = randShape( rng, kind == 4 ? 6 : kind );
        VECTOR2I               p = a->Centre() + rng.ptR( -3000000, 3000000 );
        SEG                    s = segNear( rng, a->Centre() );
        int                    cl = clearance( rng );
        int                    accK = (int) rng.range( 0, 3 );
        int                    acc = accK * (int) rng.range( 0, 1000 );

        REC r;
        r.i( kind ).s( fmt( a.get() ) ).s( fmt( a.get(), false ) ).v( p ).seg( s ).i( cl );
        r.box( a->BBox() ).box( a->BBox( cl ) ).v( a->Centre() ).b( a->IsSolid() );
        recCollidePt( r, a.get(), p, cl );
        recCollideSeg( r, a.get(), s, cl );

        if( a->Type() == SH_SIMPLE || a->Type() == SH_LINE_CHAIN || a->Type() == SH_RECT )
        {
            r.i( a->SquaredDistance( p ) ).i( a->SquaredDistance( p, true ) );

            // sqrt( ECOORD_MAX ) does not fit an int: UB in C++ (INT_MIN on x86), the port
            // saturates (deviation D4)
            if( a->SquaredDistance( p ) != VECTOR2I::ECOORD_MAX )
                r.i( a->Distance( p ) );

            r.b( a->PointInside( p ) ).b( a->PointInside( p, acc ) ).b( a->PointInside( p, acc, true ) );
        }

        if( a->Type() == SH_SIMPLE || a->Type() == SH_LINE_CHAIN )
        {
            auto lcb = static_cast<const SHAPE_LINE_CHAIN_BASE*>( a.get() );
            r.b( lcb->PointOnEdge( p, acc ) ).i( lcb->EdgeContainingPoint( p, acc ) );

            if( lcb->GetPointCount() > 0 )
                r.v( lcb->GetPoint( 0 ) ).seg( lcb->GetSegment( -1 ) );
        }

        if( a->Type() == SH_RECT )
        {
            SHAPE_RECT rect = *static_cast<SHAPE_RECT*>( a.get() );
            int        off = (int) rng.range( -1000, 1000 );
            SHAPE_RECT inf = rect.GetInflated( off );
            r.s( inf.Format() ).i( rect.Diagonal() ).i( rect.MajorDimension() ).i( rect.MinorDimension() );
            recChain( r, rect.Outline() );
            rect.Normalize();
            r.s( rect.Format() );
            int quarter = (int) rng.range( -4, 4 );
            rect.Rotate( EDA_ANGLE( 90.0 * quarter, DEGREES_T ), p );
            r.s( rect.Format() );
        }

        if( a->Type() == SH_SEGMENT )
        {
            const SHAPE_SEGMENT* sg = static_cast<SHAPE_SEGMENT*>( a.get() );
            r.i( sg->GetTotalLength() ).v( sg->GetCenter() );
        }

        sec.add( r );
    }
}


void sectionSegmentLibm()
{
    SECTION sec( "libm:shape_segment" );
    RNG     rng( 211 );

    for( int n = 0; n < 10000; n++ )
    {
        VECTOR2I size = rng.ptR( 0, 5000000 );
        VECTOR2I c = rng.ptMid();
        double   ang = rng.angleDeg();
        double   tol = rng.dbl( 0.0, 5.0 );

        SHAPE_SEGMENT s = SHAPE_SEGMENT::BySizeAndCenter( size, c, EDA_ANGLE( ang, DEGREES_T ) );

        REC r;
        r.v( size ).v( c ).d( ang ).seg( s.GetSeg() ).i( s.GetWidth() ).a( s.GetAngle() );
        r.b( s.Is45Degree() ).b( s.Is45Degree( EDA_ANGLE( tol, DEGREES_T ) ) );

        SHAPE_SEGMENT rot( s );
        rot.Rotate( EDA_ANGLE( ang, DEGREES_T ), c );
        r.seg( rot.GetSeg() );

        SHAPE_CIRCLE circ( c, size.x );
        circ.Rotate( EDA_ANGLE( ang, DEGREES_T ), size );
        r.s( circ.Format() ).s( circ.Format( false ) );

        SHAPE_SIMPLE simple = randSimple( rng );
        simple.Rotate( EDA_ANGLE( ang, DEGREES_T ), c );
        r.box( simple.BBox() ).v( simple.CPoint( -1 ) );
        sec.add( r );
    }
}


void sectionGeometryUtils()
{
    SECTION sec( "geometry_utils" );
    RNG     rng( 212 );

    for( int n = 0; n < 30000; n++ )
    {
        VECTOR2I v = rng.pt();
        VECTOR2L vl = rng.ptL( -5000000000LL, 5000000000LL );
        VECTOR2D vd = rng.ptD( -3e9, 3e9 );
        bool     only45 = rng.flag();
        unsigned pad = (unsigned) rng.range( 0, 1000 );

        if( rng.next() % 4 == 0 )
        {
            v = rng.ptR( -10, 10 );
            vd = VECTOR2D( (double) v.x, (double) v.y );
        }

        REC r;
        r.v( v ).vl( vl ).vd( vd ).b( only45 ).i( pad );
        r.v( GetVectorSnapped45( v, only45 ) ).vd( GetVectorSnapped45( vd, only45 ) );
        r.vl( GetVectorSnapped45( vl, only45 ) );
        r.v( GetVectorSnapped90( v ) ).vd( GetVectorSnapped90( vd ) );
        r.v( GetClampedCoords( v, pad ) ).vl( GetClampedCoords( vl, pad ) ).vd( GetClampedCoords( vd, pad ) );
        r.v( GetClampedCoords<double, int>( vd, pad ) );
        r.b( IsVec2SafeXY( v ) ).b( IsVec2SafeXY( vl ) ).b( IsVec2SafeXY( vd ) );

        VECTOR2I bp = rng.ptMid();
        VECTOR2I bs = rng.ptR( 0, 100000000 );
        BOX2I    box( bp, bs );
        VECTOR2I a = rng.pt();
        VECTOR2I b = rng.pt();
        int      x1 = a.x, y1 = a.y, x2 = b.x, y2 = b.y;
        bool     clipped = ClipLine( &box, x1, y1, x2, y2 );
        r.box( box ).v( a ).v( b ).b( clipped ).i( x1 ).i( y1 ).i( x2 ).i( y2 );

        int   acc = (int) rng.range( 0, 1000 );
        VECTOR2I b2p = rng.ptMid();
        VECTOR2I b2s = rng.ptR( 0, 100000000 );
        BOX2I    box2( b2p, b2s );
        bool  contained = rng.flag();
        r.b( KIGEOM::BoxHitTest( a, box, acc ) ).b( KIGEOM::BoxHitTest( box2, box, contained, acc ) );
        sec.add( r );
    }
}


void sectionGeometryUtilsLibm()
{
    SECTION sec( "libm:geometry_utils" );
    RNG     rng( 213 );

    for( int n = 0; n < 30000; n++ )
    {
        int    rad = (int) rng.range( -10, 200000000 );
        int    err = (int) rng.range( -10, 100000 );
        double ang = rng.angleDeg();
        int    segs = (int) rng.range( -5, 100000 );

        if( rng.flag() )
            rad = (int) rng.range( -10, 1000 );

        REC r;
        r.i( rad ).i( err ).d( ang ).i( segs );
        r.i( GetArcToSegmentCount( rad, err, EDA_ANGLE( ang, DEGREES_T ) ) );
        r.i( CircleToEndSegmentDeltaRadius( rad, segs ) );
        r.i( GetCircleToPolyCorrection( err ) );
        sec.add( r );
    }
}


void sectionLines()
{
    SECTION sec( "lines" );
    RNG     rng( 214 );

    for( int n = 0; n < 30000; n++ )
    {
        SEG a, b;
        segPair( rng, a, b );
        VECTOR2I p = rng.pt();

        if( rng.next() % 4 == 0 )
        {
            VECTOR2I d = rng.ptR( -1, 1 );
            p = ( rng.flag() ? a.A : a.B ) + d;
        }

        HALF_LINE hl( a );
        HALF_LINE hl2( b.A, b.B );
        LINE      ln( a );
        LINE      ln2( b.A, b.B );

        REC r;
        r.seg( a ).seg( b ).v( p );
        r.b( hl.Contains( p ) ).opt( hl.Intersect( b ) ).opt( hl.Intersect( hl2 ) ).v( hl.NearestPoint( p ) );
        r.opt( ln.Intersect( b ) ).opt( ln.Intersect( ln2 ) ).i( ln.Distance( p ) ).v( ln.NearestPoint( p ) );
        r.b( hl == hl2 ).b( ln == ln2 );

        VECTOR2I bp = rng.ptMid();
        VECTOR2I bs = rng.ptR( 0, 100000000 );
        BOX2I    box( bp, bs );

        std::optional<SEG> c1 = KIGEOM::ClipHalfLineToBox( hl, box );
        std::optional<SEG> c2 = KIGEOM::ClipLineToBox( ln, box );
        r.box( box ).b( c1.has_value() );

        if( c1 )
            r.seg( *c1 );

        r.b( c2.has_value() );

        if( c2 )
            r.seg( *c2 );

        sec.add( r );
    }
}


void sectionArcChordParams()
{
    SECTION sec( "libm:arc_chord_params" );
    RNG     rng( 215 );

    for( int n = 0; n < 20000; n++ )
    {
        VECTOR2I s, m, e;
        arcPoints( rng, s, m, e );

        ARC_CHORD_PARAMS p;
        bool             ok = p.Compute( s, m, e );

        REC r;
        r.v( s ).v( m ).v( e ).b( ok ).b( p.IsValid() );
        r.d( p.GetChordLength() ).d( p.GetHalfChord() ).vd( p.GetChordUnitVec() ).vd( p.GetNormalUnitVec() );
        r.d( p.GetSagitta() ).d( p.GetRadius() ).d( p.GetCenterOffset() ).vd( p.GetChordMidpoint() );

        if( ok )
            r.vd( p.GetCenterPoint() ).a( p.GetStartAngle() ).a( p.GetEndAngle() ).d( p.GetArcAngle() );

        sec.add( r );
    }
}


REC& recSol( REC& r, const KIGEOM::ARC_SOLUTION& s )
{
    r.b( s.valid ).v( s.start ).v( s.mid ).v( s.end ).vd( s.center ).d( s.radius );
    return r;
}


void sectionArcConstruction()
{
    SECTION sec( "libm:arc_construction" );
    RNG     rng( 216 );

    for( int n = 0; n < 20000; n++ )
    {
        VECTOR2I s = rng.pt();
        VECTOR2I e = rng.pt();
        VECTOR2I c = rng.pt();
        VECTOR2D t = rng.ptD( -10.0, 10.0 );
        bool     major = rng.flag();

        switch( rng.next() % 4 )
        {
        case 0: e = s + rng.ptR( -10, 10 ); break;
        case 1: c = ( s + e ) / 2 + rng.ptR( -3, 3 ); break;
        case 2: t = VECTOR2D( (double) ( e.x - s.x ), (double) ( e.y - s.y ) ); break;
        default: break;
        }

        REC r;
        r.v( s ).v( e ).v( c ).vd( t ).b( major );
        r.v( KIGEOM::ProjectToChordBisector( s, e, c ) );
        recSol( r, KIGEOM::ArcThroughPoints( s, c, e ) );
        recSol( r, KIGEOM::ArcFromStartEndMidDrag( s, e, c ) );
        recSol( r, KIGEOM::ArcFromStartEndCenterDrag( s, e, c, major ) );
        recSol( r, KIGEOM::ArcFromStartTangentEnd( s, t, e ) );
        sec.add( r );
    }
}

} // namespace


int main( int argc, char** argv )
{
    if( argc == 3 && std::string( argv[1] ) == "--dump" )
        g_dump = fopen( argv[2], "w" );

    sectionLibmProbe();
    sectionCircle();
    sectionCircleTanTanPt();
    sectionArcProps();
    sectionArcPolyline();
    sectionArcCollide();
    sectionArcIntersect();
    sectionArcTransform();
    sectionShapeCollide();
    sectionShapeCollideArc();
    sectionShapeDistance();
    sectionSegmentLibm();
    sectionGeometryUtils();
    sectionGeometryUtilsLibm();
    sectionLines();
    sectionArcChordParams();
    sectionArcConstruction();

    if( g_dump )
        fclose( g_dump );

    return 0;
}
