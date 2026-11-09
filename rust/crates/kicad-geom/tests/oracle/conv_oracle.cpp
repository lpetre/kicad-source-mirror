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
 * Differential-test oracle for the shape conversion utilities (package G6b):
 * convert_basic_shapes_to_polygon, ROUNDRECT, corner operations, shape_utils, oval,
 * INTERSECTION_VISITOR, nearest and SHAPE_ELLIPSE, against libkimath.a of the reference build.
 * Output format and hashing as in the other oracles (oracle_common.h); the Rust side is
 * conv_sections.rs.  Every random value is drawn in its own statement.
 */

#include <cmath>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <convert_basic_shapes_to_polygon.h>
#include <geometry/circle.h>
#include <geometry/corner_operations.h>
#include <geometry/half_line.h>
#include <geometry/intersection.h>
#include <geometry/line.h>
#include <geometry/nearest.h>
#include <geometry/oval.h>
#include <geometry/point_types.h>
#include <geometry/roundrect.h>
#include <geometry/shape_arc.h>
#include <geometry/shape_circle.h>
#include <geometry/shape_ellipse.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <geometry/shape_utils.h>
#include <math/box2.h>
#include <math/util.h>
#include <math/vector2d.h>
#include <trigo.h>

#include <wx/debug.h>

#include "oracle_common.h"

// File-local in convert_basic_shapes_to_polygon.cpp (external linkage): same layout.
struct ROUNDED_CORNER
{
    VECTOR2I m_position;
    int      m_radius;
    ROUNDED_CORNER( int x, int y ) : m_position( VECTOR2I( x, y ) ), m_radius( 0 ) {}
    ROUNDED_CORNER( int x, int y, int radius ) : m_position( VECTOR2I( x, y ) ), m_radius( radius ) {}
};

void CornerListToPolygon( SHAPE_POLY_SET& outline, std::vector<ROUNDED_CORNER>& aCorners, int aInflate,
                          int aError, ERROR_LOC aErrorLoc );

namespace
{

const int kErr[] = { 1000, 2500, 5000, 10000, 25000, 50000, 1, 100 };


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


void recPts( REC& r, const std::vector<VECTOR2I>& aPts )
{
    r.u( aPts.size() );

    for( const VECTOR2I& p : aPts )
        r.v( p );
}


void recTyped( REC& r, const std::vector<TYPED_POINT2I>& aPts )
{
    r.u( aPts.size() );

    for( const TYPED_POINT2I& p : aPts )
        r.v( p.m_point ).i( p.m_types );
}


void recOptSeg( REC& r, const std::optional<SEG>& aSeg )
{
    r.b( aSeg.has_value() );

    if( aSeg )
        r.seg( *aSeg );
}


int errOf( RNG& rng )
{
    return kErr[rng.next() % 8];
}


ERROR_LOC locOf( RNG& rng )
{
    return rng.flag() ? ERROR_INSIDE : ERROR_OUTSIDE;
}


// A pad / track size: 0.1 .. 10 mm, tiny, whole micrometres or odd
int sizeNm( RNG& rng )
{
    switch( rng.next() % 4 )
    {
    case 0: return (int) rng.range( 100000, 10000000 );
    case 1: return (int) rng.range( 1, 1000 );
    case 2: return 1000 * (int) rng.range( 100, 10000 );
    default: return (int) rng.range( 50000, 5000000 ) * 2 + 1;
    }
}


// A rotation in degrees: cardinal, diagonal, tenths, random, wide
double rotDeg( RNG& rng )
{
    switch( rng.next() % 5 )
    {
    case 0: return 90.0 * (double) rng.range( -4, 4 );
    case 1: return 45.0 * (double) rng.range( -8, 8 );
    case 2: return (double) rng.range( -3600, 3600 ) / 10.0;
    case 3: return rng.dbl( -360.0, 360.0 );
    default: return rng.angleDeg();
    }
}


int minSegOf( RNG& rng )
{
    int k = (int) rng.range( 0, 4 );
    return k == 4 ? 7 : k * 16;
}


void sectionCircle( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I c = rng.ptMid();
        int      r = 0;

        switch( rng.next() % 5 )
        {
        case 0: r = (int) rng.range( 0, 10 ); break;
        case 1: r = (int) rng.range( 50000, 5000000 ); break;
        case 2: r = (int) rng.range( 1, 50000000 ); break;
        case 3: r = -(int) rng.range( 1, 1000 ); break;
        default: r = (int) rng.range( 1000000, 10000000 ); break;
        }

        int       err = errOf( rng );
        ERROR_LOC loc = locOf( rng );
        int       minSeg = minSegOf( rng );

        REC rec;
        rec.v( c ).i( r ).i( err ).i( (int) loc ).i( minSeg );

        SHAPE_LINE_CHAIN chain;

        if( rng.flag() )
            chain.Append( VECTOR2I( 7, 7 ) );

        TransformCircleToPolygon( chain, c, r, err, loc, minSeg );
        recChain( rec, chain );

        SHAPE_POLY_SET ps;

        if( rng.flag() )
        {
            ps.NewOutline();
            ps.Append( 1, 2 );
            ps.Append( 3, 4 );
            ps.Append( 5, 7 );
        }

        TransformCircleToPolygon( ps, c, r, err, loc, minSeg );
        recPoly( rec, ps );

        int w = (int) rng.range( 0, 2 * std::abs( r ) + 10 );

        SHAPE_POLY_SET ring;
        TransformRingToPolygon( ring, c, r, w, err, loc );
        rec.i( w );
        recPoly( rec, ring );

        SHAPE_POLY_SET sc;
        SHAPE_CIRCLE( c, r ).TransformToPolygon( sc, err, loc );
        recPoly( rec, sc );

        sec.add( rec );
    }
}


VECTOR2I otherEnd( RNG& rng, const VECTOR2I& a )
{
    switch( rng.next() % 5 )
    {
    case 0: return a;
    case 1: return a + rng.ptR( -10, 10 );
    case 2:
    {
        int    len = (int) rng.range( 0, 20000000 );
        double ang = rotDeg( rng );
        VECTOR2I d( len, 0 );
        RotatePoint( d, EDA_ANGLE( ang, DEGREES_T ) );
        return a + d;
    }
    case 3:
    {
        int len = (int) rng.range( -10000000, 10000000 );
        return rng.flag() ? a + VECTOR2I( len, 0 ) : a + VECTOR2I( 0, len );
    }
    default: return a + rng.ptR( -10000000, 10000000 );
    }
}


void sectionOval( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I  a = rng.ptMid();
        VECTOR2I  b = otherEnd( rng, a );
        int       w = sizeNm( rng );
        int       err = errOf( rng );
        ERROR_LOC loc = locOf( rng );
        int       minSeg = minSegOf( rng );

        REC rec;
        rec.v( a ).v( b ).i( w ).i( err ).i( (int) loc ).i( minSeg );

        SHAPE_POLY_SET ps;
        TransformOvalToPolygon( ps, a, b, w, err, loc, minSeg );
        recPoly( rec, ps );

        SHAPE_POLY_SET ps2;
        SHAPE_SEGMENT( a, b, w ).TransformToPolygon( ps2, err, loc );
        recPoly( rec, ps2 );

        sec.add( rec );
    }
}


void sectionRect( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I  pos = rng.ptMid();
        int       sx = sizeNm( rng );
        int       sy = sizeNm( rng );
        double    rotD = rotDeg( rng );
        EDA_ANGLE rot( rotD, DEGREES_T );
        int       mode = (int) ( rng.next() % 3 );
        int       minSide = std::min( sx, sy );
        int       inflate = 0;

        switch( rng.next() % 4 )
        {
        case 0: inflate = 0; break;
        case 1: inflate = (int) rng.range( 1, 2000000 ); break;
        case 2: inflate = -(int) rng.range( 1, minSide / 2 + 10 ); break;
        default: inflate = -(int) rng.range( 1, 1000 ); break;
        }

        int       err = errOf( rng );
        ERROR_LOC loc = locOf( rng );

        REC rec;
        rec.v( pos ).i( sx ).i( sy ).d( rotD ).i( mode ).i( inflate ).i( err ).i( (int) loc );

        SHAPE_POLY_SET ps;

        if( mode == 0 )
        {
            int radius = 0;

            switch( rng.next() % 4 )
            {
            case 0: radius = 0; break;
            case 1: radius = (int) rng.range( 0, minSide / 2 ); break;
            case 2: { double f = rng.dbl( 0.0, 0.5 ); radius = (int) ( minSide * f ); break; }
            default: radius = (int) rng.range( 0, minSide ); break;
            }

            double ratio = 0.0;

            switch( rng.next() % 3 )
            {
            case 0: ratio = 0.0; break;
            case 1: ratio = rng.dbl( 0.0, 0.5 ); break;
            default: ratio = rng.flag() ? 0.5 : 0.25; break;
            }

            int corners = (int) rng.range( 0, 15 );

            rec.i( radius ).d( ratio ).i( corners );
            TransformRoundChamferedRectToPolygon( ps, pos, VECTOR2I( sx, sy ), rot, radius, ratio,
                                                  corners, inflate, err, loc );
        }
        else if( mode == 1 )
        {
            int dx = 0;
            int dy = 0;

            switch( rng.next() % 3 )
            {
            case 0: dx = (int) rng.range( -( sy / 2 ), sy / 2 ); break;
            case 1: dy = (int) rng.range( -( sx / 2 ), sx / 2 ); break;
            default: break;
            }

            rec.i( dx ).i( dy );

            // horizontal / vertical trapezoids with a size of 1 divide by zero when shrunk
            if( inflate < 0 && ( ( dx && sx < 2 ) || ( dy && sy < 2 ) ) )
                inflate = 0;

            TransformTrapezoidToPolygon( ps, pos, VECTOR2I( sx, sy ), rot, dx, dy, inflate, err, loc );
        }
        else
        {
            // A convex, clockwise (in KiCad's y-down frame) corner list on a circle
            int    cnt = (int) rng.range( 3, 8 );
            int    rad = sizeNm( rng );
            double a0 = rng.dbl( 0.0, 360.0 );
            int    infl = rng.flag() ? 0 : (int) rng.range( 0, 1000000 );

            std::vector<ROUNDED_CORNER> corners;

            for( int k = 0; k < cnt; k++ )
            {
                double jitter = rng.dbl( -0.3, 0.3 );
                double ang = ( a0 + ( k + jitter ) * 360.0 / cnt ) * M_PI / 180.0;
                int    x = KiROUND( rad * std::cos( ang ) );
                int    y = KiROUND( rad * std::sin( ang ) );
                int    cr = rng.flag() ? 0 : (int) rng.range( 0, rad / 2 );
                corners.emplace_back( x, y, cr );
            }

            bool dup = false;

            for( int k = 0; k < cnt; k++ )
            {
                if( corners[k].m_position == corners[( k + 1 ) % cnt].m_position )
                    dup = true;
            }

            rec.i( cnt ).i( rad ).d( a0 ).i( infl ).b( dup );

            if( !dup )
                CornerListToPolygon( ps, corners, infl, err, loc );
        }

        recPoly( rec, ps );
        sec.add( rec );
    }
}


void sectionArc( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I c = rng.ptR( -100000000, 100000000 );
        int      r = 0;

        switch( rng.next() % 4 )
        {
        case 0: r = (int) rng.range( 1, 1000 ); break;
        case 1: r = (int) rng.range( 100000, 10000000 ); break;
        case 2: r = (int) rng.range( 10000000, 100000000 ); break;
        default: r = (int) rng.range( 0, 10 ); break;
        }

        double sa = rotDeg( rng );
        double aa = 0.0;

        switch( rng.next() % 5 )
        {
        case 0: aa = rng.dbl( -360.0, 360.0 ); break;
        case 1: aa = 90.0 * (double) rng.range( -4, 4 ); break;
        case 2: aa = rng.dbl( -1.0, 1.0 ); break;
        case 3: aa = rng.flag() ? 360.0 : -360.0; break;
        default: aa = (double) rng.range( -3600, 3600 ) / 10.0; break;
        }

        int       accI = errOf( rng );
        double    acc = accI;
        ERROR_LOC loc = locOf( rng );

        REC rec;
        rec.v( c ).i( r ).d( sa ).d( aa ).i( accI ).i( (int) loc );

        SHAPE_LINE_CHAIN chain;
        int cnt = ConvertArcToPolyline( chain, c, r, EDA_ANGLE( sa, DEGREES_T ), EDA_ANGLE( aa, DEGREES_T ), acc,
                                        loc );
        rec.i( cnt );
        recChain( rec, chain );

        VECTOR2I s, m, e;

        if( rng.flag() )
        {
            arcPoints( rng, s, m, e );
        }
        else
        {
            VECTOR2I st = c + VECTOR2I( r, 0 );
            RotatePoint( st, c, EDA_ANGLE( sa, DEGREES_T ) );
            SHAPE_ARC arc( c, st, EDA_ANGLE( aa, DEGREES_T ), 0 );
            s = arc.GetP0();
            m = arc.GetArcMid();
            e = arc.GetP1();
        }

        double radialOffset = 0.0;

        switch( rng.next() % 3 )
        {
        case 0: radialOffset = 0.0; break;
        case 1: radialOffset = rng.dbl( -(double) r, (double) r ); break;
        default: radialOffset = (double) rng.range( 0, 1000000 ); break;
        }

        int width = rng.flag() ? sizeNm( rng ) : (int) rng.range( 0, 2000000 );
        int err = errOf( rng );

        rec.v( s ).v( m ).v( e ).d( radialOffset ).i( width ).i( err );

        SHAPE_LINE_CHAIN chain2;
        int cnt2 = ConvertArcToPolyline( chain2, s, m, e, acc, loc, radialOffset );
        rec.i( cnt2 );
        recChain( rec, chain2 );

        SHAPE_POLY_SET ps;
        TransformArcToPolygon( ps, s, m, e, width, err, loc );
        recPoly( rec, ps );

        SHAPE_POLY_SET ps2;
        SHAPE_ARC( s, m, e, width ).TransformToPolygon( ps2, err, loc );
        recPoly( rec, ps2 );

        sec.add( rec );
    }
}


int signedSize( RNG& rng )
{
    switch( rng.next() % 4 )
    {
    case 0: return sizeNm( rng );
    case 1: return -sizeNm( rng );
    case 2: return (int) rng.range( 0, 3 );
    default: return sizeNm( rng );
    }
}


void sectionRoundrect( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I p0 = rng.ptMid();
        int      w = signedSize( rng );
        int      h = signedSize( rng );
        int      minD = std::min( std::abs( w ), std::abs( h ) );
        int      maxD = std::max( std::abs( w ), std::abs( h ) );
        int      radius = 0;

        switch( rng.next() % 5 )
        {
        case 0: radius = 0; break;
        case 1: radius = (int) rng.range( 0, minD / 2 ); break;
        case 2: radius = minD / 2; break;
        case 3: radius = (int) rng.range( -1000, 2 * maxD ); break;
        default: radius = (int) rng.range( 0, std::abs( w ) ); break;
        }

        bool      norm = rng.flag();
        int       maxErr = errOf( rng );
        ERROR_LOC loc = locOf( rng );
        int       outset = (int) rng.range( -1000000, 1000000 );
        bool      prefill = rng.flag();

        REC rec;
        rec.v( p0 ).i( w ).i( h ).i( radius ).b( norm ).i( maxErr ).i( (int) loc ).i( outset );

        auto recRR = [&]( const ROUNDRECT& aRR )
        {
            rec.i( aRR.GetRoundRadius() ).v( aRR.GetPosition() ).i( aRR.GetWidth() ).i( aRR.GetHeight() );
            rec.i( aRR.GetRect().GetRadius() ).box( aRR.BBox() );

            SHAPE_POLY_SET buf;

            if( prefill )
            {
                buf.NewOutline();
                buf.Append( 1, 1 );
                buf.Append( 2, 1 );
                buf.Append( 2, 2 );
            }

            aRR.TransformToPolygon( buf, maxErr );
            recPoly( rec, buf );
        };

        const SHAPE_RECT rect( p0, w, h );

        recRR( ROUNDRECT( rect, radius, norm ) );
        recRR( ROUNDRECT::OutsetFrom( rect, outset ) );
        recRR( ROUNDRECT( rect, radius, norm ).GetInflated( outset ) );

        SHAPE_RECT rr( p0, w, h );
        rr.SetRadius( radius );

        SHAPE_POLY_SET buf2;
        rr.TransformToPolygon( buf2, maxErr, loc );
        recPoly( rec, buf2 );

        if( buf2.OutlineCount() > 0 )
            recChain( rec, rr.Outline() );

        sec.add( rec );
    }
}


SEG orientedSeg( RNG& rng, const VECTOR2I& aCorner, const VECTOR2I& aOther )
{
    return rng.flag() ? SEG( aCorner, aOther ) : SEG( aOther, aCorner );
}


void sectionCornerOps( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I c = rng.ptR( -10000000, 10000000 );
        VECTOR2I pa;
        VECTOR2I pb;
        int      kind = (int) ( rng.next() % 5 );

        switch( kind )
        {
        case 0:
        {
            int la = (int) rng.range( -1000000, 1000000 );
            int lb = (int) rng.range( -1000000, 1000000 );
            pa = c + VECTOR2I( la, 0 );
            pb = c + VECTOR2I( 0, lb );
            break;
        }
        case 1:
        {
            int k = (int) rng.range( -5, 5 );
            pa = c + rng.ptR( -100000, 100000 );
            pb = c - ( pa - c ) * k;
            break;
        }
        case 2:
        {
            pa = c + rng.ptR( -10, 10 );
            pb = c + rng.ptR( -1000000, 1000000 );
            break;
        }
        default:
        {
            pa = c + rng.ptR( -1000000, 1000000 );
            pb = c + rng.ptR( -1000000, 1000000 );
            break;
        }
        }

        SEG segA = orientedSeg( rng, c, pa );
        SEG segB = orientedSeg( rng, c, pb );

        if( rng.next() % 8 == 0 )
            segB = SEG( pb, pb + rng.ptR( -1000, 1000 ) ); // not coterminous (usually)

        CHAMFER_PARAMS params;
        params.m_chamfer_setback_a = rng.flag() ? 0 : (int) rng.range( 0, 1200000 );
        params.m_chamfer_setback_b = rng.flag() ? 0 : (int) rng.range( 0, 1200000 );

        int  radius = (int) rng.range( 1, 1000000 );
        bool slots = rng.flag();

        REC rec;
        rec.seg( segA ).seg( segB ).i( params.m_chamfer_setback_a ).i( params.m_chamfer_setback_b );
        rec.i( radius ).b( slots );

        std::optional<CHAMFER_RESULT> ch = ComputeChamferPoints( segA, segB, params );
        rec.b( ch.has_value() );

        if( ch )
        {
            rec.seg( ch->m_chamfer );
            recOptSeg( rec, ch->m_updated_seg_a );
            recOptSeg( rec, ch->m_updated_seg_b );
        }

        std::optional<DOGBONE_RESULT> db = ComputeDogbone( segA, segB, radius, slots );
        rec.b( db.has_value() );

        if( db )
        {
            recArc( rec, db->m_arc );
            recOptSeg( rec, db->m_updated_seg_a );
            recOptSeg( rec, db->m_updated_seg_b );
            rec.b( db->m_small_arc_mouth );
        }

        sec.add( rec );
    }
}


SHAPE_LINE_CHAIN randomRing( RNG& rng )
{
    SHAPE_LINE_CHAIN c;
    int              cnt = (int) rng.range( 0, 10 );
    int              scale = rng.flag() ? 1000 : 1000000;

    for( int i = 0; i < cnt; i++ )
    {
        int x = (int) rng.range( -5, 5 );
        int y = (int) rng.range( -5, 5 );
        c.Append( VECTOR2I( x * scale, y * scale ) );
    }

    c.SetClosed( rng.flag() );
    return c;
}


void sectionShapeUtils( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        VECTOR2I pos = rng.ptR( -100000000, 100000000 );
        int      bw = (int) rng.range( 0, 10000000 );
        int      bh = (int) rng.range( 0, 10000000 );
        BOX2I    box( pos, VECTOR2I( bw, bh ) );
        int      dir = (int) rng.range( -1, 8 );
        int      outset = (int) rng.range( -1000, 1000000 );
        int      rad = (int) rng.range( 0, 1000000 );

        REC rec;
        rec.box( box ).i( dir ).i( outset ).i( rad );

        recChain( rec, KIGEOM::BoxToLineChain( box ) );

        std::vector<VECTOR2I> corners;
        KIGEOM::CollectBoxCorners( box, corners );
        recPts( rec, corners );

        std::vector<SEG> segs = KIGEOM::GetSegsInDirection( box, (DIRECTION_45::Directions) dir );
        rec.u( segs.size() );

        for( const SEG& s : segs )
            rec.seg( s );

        int        rw = (int) rng.range( -1000000, 1000000 );
        int        rh = (int) rng.range( -1000000, 1000000 );
        SHAPE_RECT rect( pos, rw, rh );
        rec.v( KIGEOM::GetPoint( rect, (DIRECTION_45::Directions) dir, outset ) );

        recArc( rec, KIGEOM::MakeArcCw90( pos, rad, (DIRECTION_45::Directions) dir ) );
        recArc( rec, KIGEOM::MakeArcCw180( pos, rad, (DIRECTION_45::Directions) dir ) );

        SEG      sg = rng.seg();
        VECTOR2I q = rng.ptMid();

        if( rng.flag() )
            sg.B = sg.A;

        rec.seg( sg ).v( q ).d( KIGEOM::ParameterAlong( SEG( sg.A / 4, sg.B / 4 ), q / 4 ) );
        rec.seg( KIGEOM::NormalisedSeg( sg ) );
        rec.v( KIGEOM::GetOtherEnd( sg, rng.flag() ? sg.A : q ) );

        SEG sg2 = rng.flag() ? SEG( sg.B, q ) : SEG( q, rng.flag() ? sg.A : q );
        rec.opt( KIGEOM::GetSharedEndpoint( sg, sg2 ) );

        recTyped( rec, KIGEOM::GetCircleKeyPoints( CIRCLE( pos, rad ), rng.flag() ) );

        SHAPE_LINE_CHAIN ring = randomRing( rng );
        recChain( rec, KIGEOM::RectifyPolygon( ring ) );

        SHAPE_POLY_SET hp;
        hp.NewOutline();
        hp.Append( -10000000, -10000000 );
        hp.Append( 10000000, -10000000 );
        hp.Append( 10000000, 10000000 );
        hp.Append( -10000000, 10000000 );
        rec.b( KIGEOM::AddHoleIfValid( hp, std::move( ring ) ) );
        recPoly( rec, hp );

        size_t    nSides = (size_t) rng.range( 0, 12 );
        VECTOR2I  pt0 = pos + rng.ptR( -1000000, 1000000 );
        bool      across = rng.flag();
        double    angD = rotDeg( rng );
        size_t    nSides2 = nSides == 0 ? 1 : nSides;

        recPts( rec, KIGEOM::MakeRegularPolygonPoints( pos, nSides, pt0 ) );
        recPts( rec, KIGEOM::MakeRegularPolygonPoints( pos, nSides2, rad, across, EDA_ANGLE( angD, DEGREES_T ) ) );

        VECTOR2I crossSize = rng.ptR( 0, 1000000 );
        std::vector<SEG> cross = KIGEOM::MakeCrossSegments( pos, crossSize, EDA_ANGLE( angD, DEGREES_T ) );
        rec.u( cross.size() );

        for( const SEG& s : cross )
            rec.seg( s );

        // Ovals
        VECTOR2I oa = rng.ptMid();
        VECTOR2I ob = otherEnd( rng, oa );
        int      ow = sizeNm( rng ) + 1;
        unsigned flags = (unsigned) rng.range( 0, 255 );
        SHAPE_SEGMENT oval( oa, ob, ow );

        rec.v( oa ).v( ob ).i( ow ).u( flags );
        recTyped( rec, KIGEOM::GetOvalKeyPoints( oval, flags ) );

        if( oa != ob )
            recChain( rec, KIGEOM::ConvertToChain( oval ) );

        sec.add( rec );
    }
}


// Grid coordinates make collinear / tangent / concentric contacts common
int gcoord( RNG& rng, bool aGrid )
{
    if( aGrid )
        return 100 * (int) rng.range( -10, 10 );

    return (int) rng.range( -10000000, 10000000 );
}


VECTOR2I gpt( RNG& rng, bool aGrid )
{
    int x = gcoord( rng, aGrid );
    int y = gcoord( rng, aGrid );
    return VECTOR2I( x, y );
}


int gradius( RNG& rng, bool aGrid )
{
    if( aGrid )
        return 100 * (int) rng.range( 1, 10 );

    return (int) rng.range( 1, 5000000 );
}


double sweepDeg( RNG& rng )
{
    switch( rng.next() % 3 )
    {
    case 0: return 90.0 * (double) rng.range( 1, 4 ) * ( rng.flag() ? 1.0 : -1.0 );
    case 1: return rng.dbl( 1.0, 359.0 ) * ( rng.flag() ? 1.0 : -1.0 );
    default: return 45.0 * (double) rng.range( 1, 7 );
    }
}


SHAPE_ELLIPSE randomEllipse( RNG& rng, bool aGrid )
{
    VECTOR2I c = gpt( rng, aGrid );
    int      a = gradius( rng, aGrid );
    int      b = gradius( rng, aGrid );
    double   rot = aGrid ? 45.0 * (double) rng.range( -4, 4 ) : rotDeg( rng );

    if( rng.flag() )
        return SHAPE_ELLIPSE( c, a, b, EDA_ANGLE( rot, DEGREES_T ) );

    double sa = (double) rng.range( -720, 720 );
    double ea = sa + sweepDeg( rng );
    return SHAPE_ELLIPSE( c, a, b, EDA_ANGLE( rot, DEGREES_T ), EDA_ANGLE( sa, DEGREES_T ), EDA_ANGLE( ea, DEGREES_T ) );
}


SHAPE_ARC randomArc( RNG& rng, bool aGrid )
{
    VECTOR2I c = gpt( rng, aGrid );
    int      r = gradius( rng, aGrid );
    double   sa = aGrid ? 45.0 * (double) rng.range( -8, 8 ) : rotDeg( rng );
    double   aa = sweepDeg( rng );
    VECTOR2I st = c + VECTOR2I( r, 0 );
    RotatePoint( st, c, EDA_ANGLE( sa, DEGREES_T ) );
    return SHAPE_ARC( c, st, EDA_ANGLE( aa, DEGREES_T ), 0 );
}


INTERSECTABLE_GEOM randomGeom( RNG& rng, bool aGrid, int& aKind )
{
    aKind = (int) ( rng.next() % 7 );

    switch( aKind )
    {
    case 0: { VECTOR2I a = gpt( rng, aGrid ); VECTOR2I b = gpt( rng, aGrid ); if( a == b ) b.x += 100; return LINE( a, b ); }
    case 1: { VECTOR2I a = gpt( rng, aGrid ); VECTOR2I b = gpt( rng, aGrid ); if( a == b ) b.y += 100; return HALF_LINE( a, b ); }
    case 2: { VECTOR2I a = gpt( rng, aGrid ); VECTOR2I b = gpt( rng, aGrid ); return SEG( a, b ); }
    case 3: { VECTOR2I c = gpt( rng, aGrid ); int r = gradius( rng, aGrid ); return CIRCLE( c, r ); }
    case 4: return randomArc( rng, aGrid );
    case 5: return randomEllipse( rng, aGrid );
    default:
    {
        VECTOR2I p = gpt( rng, aGrid );
        int      w = aGrid ? 100 * (int) rng.range( 0, 10 ) : (int) rng.range( 0, 5000000 );
        int      h = aGrid ? 100 * (int) rng.range( 0, 10 ) : (int) rng.range( 0, 5000000 );
        return BOX2I( p, VECTOR2I( w, h ) );
    }
    }
}


NEARABLE_GEOM toNearable( const INTERSECTABLE_GEOM& aGeom )
{
    return std::visit( []( const auto& g ) -> NEARABLE_GEOM { return g; }, aGeom );
}


void sectionIntersection( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        bool grid = rng.flag();
        int  ka = 0;
        int  kb = 0;

        INTERSECTABLE_GEOM ga = randomGeom( rng, grid, ka );
        INTERSECTABLE_GEOM gb = randomGeom( rng, grid, kb );

        REC rec;
        rec.b( grid ).i( ka ).i( kb );

        std::vector<VECTOR2I> pts;
        INTERSECTION_CONTACT  contact;

        if( rng.flag() )
        {
            pts.push_back( VECTOR2I( 3, 3 ) );
            std::visit( INTERSECTION_VISITOR( gb, pts ), ga );
        }
        else
        {
            std::visit( INTERSECTION_VISITOR( gb, pts, contact ), ga );
        }

        recPts( rec, pts );
        rec.b( contact.m_Tangent ).b( contact.m_Overlapping );

        // Both directions, accumulating into the same contact
        std::vector<VECTOR2I> pts2;
        std::visit( INTERSECTION_VISITOR( ga, pts2, contact ), gb );
        recPts( rec, pts2 );
        rec.b( contact.m_Tangent ).b( contact.m_Overlapping );

        // Nearest points
        VECTOR2I q = gpt( rng, grid );
        rec.v( q );
        rec.v( GetNearestPoint( toNearable( ga ), q ) );

        std::vector<NEARABLE_GEOM> list;
        int                        cnt = (int) rng.range( 0, 3 );

        if( cnt >= 1 )
            list.push_back( toNearable( ga ) );

        if( cnt >= 2 )
            list.push_back( toNearable( gb ) );

        if( cnt >= 3 )
            list.push_back( NEARABLE_GEOM( q + VECTOR2I( 100, 0 ) ) );

        rec.opt( GetNearestPoint( list, q ) );

        sec.add( rec );
    }
}


void recEllipse( REC& r, const SHAPE_ELLIPSE& e )
{
    r.v( e.GetCenter() ).i( e.GetMajorRadius() ).i( e.GetMinorRadius() ).a( e.GetRotation() );
    r.a( e.GetStartAngle() ).a( e.GetEndAngle() ).b( e.IsArc() ).b( e.IsSolid() ).box( e.BBox() );
}


void sectionEllipse( const char* aName, uint64_t aSeed, int aCount )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int n = 0; n < aCount; n++ )
    {
        bool     grid = rng.next() % 4 == 0;
        VECTOR2I c = grid ? gpt( rng, true ) : rng.ptR( -100000000, 100000000 );
        int      ctor = (int) ( rng.next() % 4 );
        int      a = 0;
        int      b = 0;

        switch( rng.next() % 4 )
        {
        case 0: a = (int) rng.range( -5, 5 ); b = (int) rng.range( -5, 50 ); break;
        case 1: a = (int) rng.range( 1, 50000000 ); b = (int) rng.range( 1, 50000000 ); break;
        case 2: a = (int) rng.range( 100000, 5000000 ); b = a; break;
        default: a = grid ? gradius( rng, true ) : (int) rng.range( 1000, 5000000 ); b = (int) rng.range( 1, 5000000 ); break;
        }

        double rot = rotDeg( rng );
        double sa = (double) rng.range( -720, 720 ) + ( rng.flag() ? 0.0 : rng.dbl( 0.0, 1.0 ) );
        double ea = 0.0;

        switch( rng.next() % 4 )
        {
        case 0: ea = sa + sweepDeg( rng ); break;
        case 1: ea = sa + 360.0; break;
        case 2: ea = rng.dbl( -720.0, 720.0 ); break;
        default: ea = sa + rng.dbl( -400.0, 400.0 ); break;
        }

        VECTOR2I majorEnd = rng.ptR( -50000000, 50000000 );
        double   ratio = rng.dbl( 0.0, 1.5 );

        SHAPE_ELLIPSE e;

        switch( ctor )
        {
        case 0: e = SHAPE_ELLIPSE( c, a, b, EDA_ANGLE( rot, DEGREES_T ) ); break;
        case 1: e = SHAPE_ELLIPSE( c, a, b, EDA_ANGLE( rot, DEGREES_T ), EDA_ANGLE( sa, DEGREES_T ), EDA_ANGLE( ea, DEGREES_T ) ); break;
        case 2: e = SHAPE_ELLIPSE( c, majorEnd, ratio ); break;
        default: e = SHAPE_ELLIPSE( c, majorEnd, ratio, EDA_ANGLE( sa, DEGREES_T ), EDA_ANGLE( ea, DEGREES_T ) ); break;
        }

        int clearance = (int) rng.range( 0, 1000 );

        REC rec;
        rec.v( c ).i( ctor ).i( a ).i( b ).d( rot ).d( sa ).d( ea ).v( majorEnd ).d( ratio );
        recEllipse( rec, e );
        rec.box( e.BBox( clearance ) );
        rec.s( e.Format( true ) ).s( e.Format( false ) );

        // GetLength() of a large arc runs the adaptive integration to its depth limit (slow)
        if( n % 8 == 0 && e.GetMajorRadius() <= 1000000 )
            rec.d( e.GetLength() );

        int span = std::max( 10, 2 * e.GetMajorRadius() );

        for( int k = 0; k < 3; k++ )
        {
            VECTOR2I p = e.GetCenter() + rng.ptR( -span, span );
            VECTOR2I p2 = rng.flag() ? p : e.GetCenter() + rng.ptR( -span, span );
            int      acc = rng.flag() ? 0 : (int) rng.range( 1, 1000 );
            int      cl = rng.flag() ? 0 : (int) rng.range( 0, 100000 );
            int      outs = (int) rng.range( 0, 3 );

            rec.v( p ).v( p2 ).i( acc ).i( cl ).i( outs );
            rec.b( e.PointInside( p, acc ) ).i( e.SquaredDistance( p, false ) ).i( e.SquaredDistance( p, true ) );
            rec.v( e.NearestPoint( p ) );

            int      actual = -7;
            VECTOR2I loc( -7, -7 );
            bool     hit = e.Collide( SEG( p, p2 ), cl, ( outs & 1 ) ? &actual : nullptr,
                                      ( outs & 2 ) ? &loc : nullptr );
            rec.b( hit ).i( actual ).v( loc );

            bool line = rng.flag();
            recPts( rec, e.Intersect( SEG( p, p2 ), line ) );
        }

        int maxErr = 0;

        switch( rng.next() % 3 )
        {
        case 0: maxErr = (int) rng.range( -5, 5 ); break;
        case 1: maxErr = errOf( rng ); break;
        default: maxErr = std::max( 1, e.GetMajorRadius() / 50 ); break;
        }

        rec.i( maxErr );
        recChain( rec, e.ConvertToPolyline( maxErr ) );

        SHAPE_POLY_SET ps;
        e.TransformToPolygon( ps, maxErr, ERROR_INSIDE );
        recPoly( rec, ps );

        // Intersections with other curves
        CIRCLE        circ( e.GetCenter() + rng.ptR( -span, span ), (int) rng.range( 0, span ) );
        SHAPE_ARC     arc = randomArc( rng, false );
        SHAPE_ELLIPSE other = randomEllipse( rng, false );

        if( rng.flag() )
            other.SetCenter( e.GetCenter() + rng.ptR( -span, span ) );

        rec.v( circ.Center ).i( circ.Radius ).v( arc.GetP0() ).v( arc.GetArcMid() ).v( arc.GetP1() );
        recEllipse( rec, other );
        recPts( rec, e.Intersect( circ ) );
        recPts( rec, e.Intersect( arc ) );
        recPts( rec, e.Intersect( other ) );

        // Transformations
        SHAPE_ELLIPSE t = e;
        double        rotA = rotDeg( rng );
        VECTOR2I      rc = rng.ptR( -10000000, 10000000 );
        t.Rotate( EDA_ANGLE( rotA, DEGREES_T ), rc );
        rec.d( rotA ).v( rc );
        recEllipse( rec, t );

        VECTOR2I mref = rng.ptR( -10000000, 10000000 );
        bool     lr = rng.flag();
        t.Mirror( mref, lr ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
        rec.v( mref ).b( lr );
        recEllipse( rec, t );

        VECTOR2I mv = rng.ptR( -1000000, 1000000 );
        t.Move( mv );
        rec.v( mv );
        recEllipse( rec, t );

        int    nm = (int) rng.range( -10, 5000000 );
        int    nn = (int) rng.range( -10, 5000000 );
        double nr = rotDeg( rng );
        double ns = (double) rng.range( -400, 400 );
        double ne = (double) rng.range( -400, 400 );

        t.SetMajorRadius( nm );
        recEllipse( rec, t );
        t.SetMinorRadius( nn );
        recEllipse( rec, t );
        t.SetRotation( EDA_ANGLE( nr, DEGREES_T ) );
        t.SetStartAngle( EDA_ANGLE( ns, DEGREES_T ) );
        t.SetEndAngle( EDA_ANGLE( ne, DEGREES_T ) );
        rec.i( nm ).i( nn ).d( nr ).d( ns ).d( ne );
        recEllipse( rec, t );
        rec.b( t == e ).b( SHAPE_ELLIPSE( e ) == e );

        sec.add( rec );
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
    sectionCircle( "libm:conv_circle", 600, 20000 );
    sectionOval( "libm:conv_oval", 601, 20000 );
    sectionRect( "libm:conv_rect", 602, 30000 );
    sectionArc( "libm:conv_arc", 603, 20000 );
    sectionRoundrect( "libm:conv_roundrect", 604, 20000 );
    sectionCornerOps( "libm:conv_corner_ops", 605, 20000 );
    sectionShapeUtils( "libm:conv_shape_utils", 606, 20000 );
    sectionIntersection( "libm:intersection", 607, 30000 );
    sectionEllipse( "libm:shape_ellipse", 608, 30000 );

    if( g_dump )
        fclose( g_dump );

    return 0;
}
