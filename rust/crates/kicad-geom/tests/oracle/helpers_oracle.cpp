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
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

/**
 * Differential-test oracle of kicad-geom work package G6a (polygon-independent kimath helpers):
 * bezier_curves, convex_hull, transform_trs, transform, vector3, ellipse, geometry_predicates,
 * distribute, grid_geometry and the hashes (hash.h, hash_128.h, mmh3_hash.h, md5_hash).
 *
 * Links libkimath.a of the reference build (see build_helpers_oracle.sh).  Output format and
 * generator are shared with the other kicad-geom oracles (oracle_common.h); the Rust replay is
 * tests/oracle/helper_sections.rs driven by tests/oracle_helpers_diff.rs.
 */

#include <algorithm>
#include <climits>
#include <sstream>
#include <limits>
#include <string>
#include <vector>

#include "oracle_common.h"

#include <bezier_curves.h>
#include <hash.h>
#include <hash_128.h>
#include <md5_hash.h>
#include <mmh3_hash.h>
#include <transform.h>
#include <geometry/convex_hull.h>
#include <geometry/distribute.h>
#include <geometry/ellipse.h>
#include <geometry/geometry_predicates.h>
#include <geometry/grid_geometry.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_poly_set.h>
#include <geometry/transform_trs.h>
#include <math/vector3.h>

#include <wx/debug.h>

namespace
{

const int MAX_ERRORS[] = { 1, 2, 5, 10, 100, 1000, 2500, 5000, 10000, 20000, 0, -5 };
const int64_t P10[] = { 1, 10, 100, 1000, 10000, 100000, 1000000 };


int maxErrorI( RNG& rng )
{
    return MAX_ERRORS[rng.next() % 12];
}


/// Control points: aCount = 3 (quadratic) or 4 (cubic), spanning about aErr * 10^k.
void bezierCtrl( RNG& rng, int aCount, int aErr, std::vector<VECTOR2I>& aPts )
{
    int64_t span = std::max<int64_t>( 1, std::max( aErr, 1 ) * P10[rng.range( 0, 6 )] );
    span = std::min<int64_t>( span, 300000000 );
    VECTOR2I c = rng.ptR( -200000000, 200000000 );
    int      kind = (int) ( rng.next() % 10 );

    aPts.clear();

    for( int i = 0; i < aCount; i++ )
    {
        VECTOR2I d = rng.ptR( -span, span );
        aPts.push_back( c + d );
    }

    switch( kind )
    {
    case 1:
    {
        // collinear (integer multiples of one direction)
        VECTOR2I d = rng.ptR( -span / 4 - 1, span / 4 + 1 );

        for( int i = 0; i < aCount; i++ )
        {
            int k = (int) rng.range( -3, 3 );
            aPts[i] = c + d * k;
        }

        break;
    }
    case 2:
        for( int i = 0; i < aCount; i++ )
            aPts[i] = c;

        break;
    case 3: aPts[aCount - 1] = aPts[0]; break;               // closed loop
    case 4:
        aPts[1] = aPts[0];                                     // control point on the endpoint
        if( aCount == 4 && rng.flag() )
            aPts[2] = aPts[3];
        break;
    case 5:
    {
        // S curve (inflection points)
        int s = (int) span;
        int h = (int) rng.range( 1, s );
        aPts[0] = c + VECTOR2I( -s, 0 );
        aPts[1] = c + VECTOR2I( (int) rng.range( -s, s ), h );
        aPts[aCount - 2] = c + VECTOR2I( (int) rng.range( -s, s ), -h );
        aPts[aCount - 1] = c + VECTOR2I( s, 0 );
        break;
    }
    case 6:
    {
        // nearly collinear
        VECTOR2I d = rng.ptR( -span / 4 - 1, span / 4 + 1 );

        for( int i = 0; i < aCount; i++ )
        {
            int k = (int) rng.range( -3, 3 );
            VECTOR2I e = rng.ptR( -3, 3 );
            aPts[i] = c + d * k + e;
        }

        break;
    }
    case 7:
        if( aCount == 4 )
            aPts[2] = aPts[1];
        else
            aPts[1] = aPts[2];
        break;
    default: break;
    }
}


void recPolyI( REC& r, const std::vector<VECTOR2I>& aPts )
{
    r.u( aPts.size() );

    for( const VECTOR2I& p : aPts )
        r.v( p );
}


void recPolyD( REC& r, const std::vector<VECTOR2D>& aPts )
{
    r.u( aPts.size() );

    for( const VECTOR2D& p : aPts )
        r.vd( p );
}


void sectionBezierPolyI()
{
    SECTION sec( "libm:bezier_poly_i" );
    RNG     rng( 6001 );

    for( int n = 0; n < 30000; n++ )
    {
        int                   err = maxErrorI( rng );
        int                   count = rng.flag() ? 4 : 3;
        std::vector<VECTOR2I> ctrl;
        bezierCtrl( rng, count, err, ctrl );
        bool                  fourArg = count == 4 && rng.flag();

        std::vector<VECTOR2I> out{ VECTOR2I( 7, 7 ) };  // GetPoly( VECTOR2I ) clears

        if( fourArg )
            BEZIER_POLY( ctrl[0], ctrl[1], ctrl[2], ctrl[3] ).GetPoly( out, err );
        else
            BEZIER_POLY( ctrl ).GetPoly( out, err );

        REC r;
        r.i( err ).b( fourArg );
        recPolyI( r, ctrl );
        recPolyI( r, out );
        sec.add( r );
    }
}


void sectionBezierPolyD()
{
    SECTION sec( "libm:bezier_poly_d" );
    RNG     rng( 6002 );
    const double errs[] = { 0.01, 0.5, 1.0, 10.0, 2500.0, 5000.0, 0.0, -1.0 };

    for( int n = 0; n < 20000; n++ )
    {
        double                err = errs[rng.next() % 8];
        int                   count = rng.flag() ? 4 : 3;
        std::vector<VECTOR2I> ctrlI;
        bezierCtrl( rng, count, (int) std::max( 1.0, err ), ctrlI );
        std::vector<VECTOR2D> ctrl;

        for( const VECTOR2I& p : ctrlI )
        {
            double fx = rng.flag() ? rng.dbl( -1.0, 1.0 ) : 0.0;
            double fy = rng.flag() ? rng.dbl( -1.0, 1.0 ) : 0.0;

            if( err < 1.0 )
            {
                // small curves at a fine tolerance
                ctrl.emplace_back( ( p.x % 1000 ) + fx, ( p.y % 1000 ) + fy );
            }
            else
            {
                ctrl.emplace_back( p.x + fx, p.y + fy );
            }
        }

        std::vector<VECTOR2D> out;

        if( rng.flag() )
            out.emplace_back( 1.5, -2.5 );  // GetPoly( VECTOR2D ) appends

        BEZIER_POLY( ctrl ).GetPoly( out, err );

        REC r;
        r.d( err );
        recPolyD( r, ctrl );
        recPolyD( r, out );
        sec.add( r );
    }
}


double tValue( RNG& rng )
{
    switch( rng.next() % 6 )
    {
    case 0: return 0.0;
    case 1: return 1.0;
    case 2: return 0.5;
    case 3: return rng.dbl( -0.5, 1.5 );
    default: return rng.unit();
    }
}


template <typename T>
void recBez( REC& r, const BEZIER<T>& b );

template <>
void recBez( REC& r, const BEZIER<int>& b )
{
    r.v( b.Start ).v( b.C1 ).v( b.C2 ).v( b.End );
}

template <>
void recBez( REC& r, const BEZIER<double>& b )
{
    r.vd( b.Start ).vd( b.C1 ).vd( b.C2 ).vd( b.End );
}


template <typename T>
void bezTemplateCase( RNG& rng, REC& r, const BEZIER<T>& b )
{
    double t = tValue( rng );
    double t0 = tValue( rng );
    double t1 = tValue( rng );

    if( rng.flag() && t1 < t0 )
        std::swap( t0, t1 );

    BEZIER<T> left, right;
    b.Split( t, left, right );

    r.d( t ).d( t0 ).d( t1 );
    recBez( r, b );
    VECTOR2<T> p = b.PointAt( t );
    recBez( r, BEZIER<T>( p, p, p, p ) );
    recBez( r, left );
    recBez( r, right );
    recBez( r, b.SubCurve( t0, t1 ) );
}


void sectionBezierTemplate()
{
    SECTION sec( "bezier_template" );
    RNG     rng( 6003 );

    for( int n = 0; n < 20000; n++ )
    {
        std::vector<VECTOR2I> ctrl;
        bezierCtrl( rng, 4, (int) rng.range( 1, 1000 ), ctrl );
        REC r;

        if( rng.flag() )
        {
            BEZIER<int> b( ctrl[0], ctrl[1], ctrl[2], ctrl[3] );
            r.b( true );
            bezTemplateCase( rng, r, b );
        }
        else
        {
            double fx = rng.dbl( -1.0, 1.0 );
            BEZIER<double> b( VECTOR2D( ctrl[0] ) + VECTOR2D( fx, 0.25 ), VECTOR2D( ctrl[1] ),
                              VECTOR2D( ctrl[2] ) - VECTOR2D( 0.5, fx ), VECTOR2D( ctrl[3] ) );
            r.b( false );
            bezTemplateCase( rng, r, b );
        }

        sec.add( r );
    }
}


EDA_ANGLE ellipseAngle( RNG& rng )
{
    switch( rng.next() % 5 )
    {
    case 0: return ANGLE_0;
    case 1: return FULL_CIRCLE;
    case 2: return EDA_ANGLE( rng.dbl( -7.0, 7.0 ), RADIANS_T );
    default: return EDA_ANGLE( rng.angleDeg(), DEGREES_T );
    }
}


template <typename T>
void recEllipse( REC& r, const ELLIPSE<T>& e );

template <>
void recEllipse( REC& r, const ELLIPSE<int>& e )
{
    r.v( e.Center ).i( e.MajorRadius ).i( e.MinorRadius ).a( e.Rotation ).a( e.StartAngle ).a( e.EndAngle );
}

template <>
void recEllipse( REC& r, const ELLIPSE<double>& e )
{
    r.vd( e.Center ).d( e.MajorRadius ).d( e.MinorRadius ).a( e.Rotation ).a( e.StartAngle ).a( e.EndAngle );
}


template <typename T>
void recVec( REC& r, const VECTOR2<T>& v );

template <>
void recVec( REC& r, const VECTOR2<int>& v )
{
    r.v( v );
}

template <>
void recVec( REC& r, const VECTOR2<double>& v )
{
    r.vd( v );
}


template <typename T>
void ellipseCase( RNG& rng, REC& r, ELLIPSE<T> e )
{
    recEllipse( r, e );
    r.a( e.GetSubtendedAngle() );
    recVec( r, e.GetArcStartPoint() );
    recVec( r, e.GetArcEndPoint() );

    EDA_ANGLE a = ellipseAngle( rng );
    recVec( r, e.GetPointAtAngle( a ) );

    VECTOR2<T> probe;

    if constexpr( std::is_integral_v<T> )
        probe = rng.flag() ? e.GetPointAtAngle( a ) : rng.ptMid();
    else
        probe = rng.flag() ? e.GetPointAtAngle( a ) : rng.ptD( -1e8, 1e8 );

    r.a( a );
    recVec( r, probe );
    r.a( e.GetAngleAtPoint( probe ) );

    std::vector<BEZIER<T>> beziers;
    TransformEllipseToBeziers( e, beziers );
    r.u( beziers.size() );

    for( const BEZIER<T>& b : beziers )
        recBez( r, b );

    VECTOR2<T> ref;

    if constexpr( std::is_integral_v<T> )
        ref = rng.ptMid();
    else
        ref = rng.ptD( -1e8, 1e8 );

    bool lr = rng.flag();
    e.Mirror( ref, lr ? FLIP_DIRECTION::LEFT_RIGHT : FLIP_DIRECTION::TOP_BOTTOM );
    recVec( r, ref );
    r.b( lr );
    recEllipse( r, e );
}


void sectionEllipse()
{
    SECTION sec( "libm:ellipse" );
    RNG     rng( 6004 );

    for( int n = 0; n < 30000; n++ )
    {
        REC r;
        int kind = (int) ( rng.next() % 4 );
        r.i( kind );

        EDA_ANGLE s = ellipseAngle( rng );
        EDA_ANGLE e = ellipseAngle( rng );

        if( rng.next() % 8 == 0 )
            e = s;

        switch( kind )
        {
        case 0:
        {
            VECTOR2I  c = rng.ptMid();
            int       major = (int) rng.range( 0, 100000000 );
            int       minor = (int) rng.range( 0, 100000000 );
            EDA_ANGLE rot = ellipseAngle( rng );
            ellipseCase( rng, r, ELLIPSE<int>( c, major, minor, rot, s, e ) );
            break;
        }
        case 1:
        {
            VECTOR2I c = rng.ptMid();
            VECTOR2I m = rng.ptMid();
            double   ratio = rng.flag() ? rng.dbl( 0.0, 1.0 ) : rng.dbl( 0.0, 10.0 );
            ellipseCase( rng, r, ELLIPSE<int>( c, m, ratio, s, e ) );
            break;
        }
        case 2:
        {
            VECTOR2D  c = rng.ptD( -1e8, 1e8 );
            double    major = rng.dbl( 0.0, 1e8 );
            double    minor = rng.dbl( 0.0, 1e8 );
            EDA_ANGLE rot = ellipseAngle( rng );
            ellipseCase( rng, r, ELLIPSE<double>( c, major, minor, rot, s, e ) );
            break;
        }
        default:
        {
            VECTOR2D c = rng.ptD( -1e8, 1e8 );
            VECTOR2D m = rng.ptD( -1e8, 1e8 );
            double   ratio = rng.dbl( 0.0, 10.0 );
            ellipseCase( rng, r, ELLIPSE<double>( c, m, ratio, s, e ) );
            break;
        }
        }

        sec.add( r );
    }
}


void hullPoints( RNG& rng, std::vector<VECTOR2I>& aPts )
{
    int count = (int) rng.range( 0, 60 );
    int kind = (int) ( rng.next() % 6 );
    aPts.clear();

    for( int i = 0; i < count; i++ )
    {
        switch( kind )
        {
        case 0: aPts.push_back( rng.ptR( -5, 5 ) ); break;           // many duplicates
        case 1:
        {
            // collinear
            int k = (int) rng.range( -1000, 1000 );
            aPts.push_back( VECTOR2I( 3 * k + 7, -2 * k + 1 ) );
            break;
        }
        case 2: aPts.push_back( rng.pt() ); break;
        case 3:
        {
            // on a circle-ish ring plus interior points
            double a = rng.dbl( 0.0, 6.283185307179586 );
            int    rad = (int) rng.range( 1, 100000000 );
            aPts.push_back( VECTOR2I( (int) ( rad * std::cos( a ) ), (int) ( rad * std::sin( a ) ) ) );
            break;
        }
        case 4: aPts.push_back( rng.ptR( -500000000, 500000000 ) ); break;
        default: aPts.push_back( rng.ptMid() ); break;
        }

        if( !aPts.empty() && rng.next() % 5 == 0 )
            aPts.push_back( aPts[rng.next() % aPts.size()] );
    }
}


void sectionConvexHull()
{
    SECTION sec( "libm:convex_hull" );
    RNG     rng( 6005 );

    for( int n = 0; n < 30000; n++ )
    {
        std::vector<VECTOR2I> pts;
        hullPoints( rng, pts );

        // the previous content survives fewer than 2 points
        std::vector<VECTOR2I> hull{ VECTOR2I( 1, 2 ), VECTOR2I( 3, 4 ) };
        BuildConvexHull( hull, pts );

        REC r;
        recPolyI( r, pts );
        recPolyI( r, hull );

        if( n % 4 == 0 )
        {
            SHAPE_POLY_SET set;
            int            outlines = (int) rng.range( 0, 3 );

            for( int o = 0; o < outlines; o++ )
            {
                std::vector<VECTOR2I> opts;
                hullPoints( rng, opts );
                set.AddOutline( SHAPE_LINE_CHAIN( opts, true ) );
            }

            VECTOR2I  pos = rng.ptMid();
            EDA_ANGLE rot( rng.angleDeg(), DEGREES_T );
            std::vector<VECTOR2I> h1, h2;
            BuildConvexHull( h1, set );
            BuildConvexHull( h2, set, pos, rot );
            r.v( pos ).a( rot );
            recPolyI( r, h1 );
            recPolyI( r, h2 );
        }

        sec.add( r );
    }
}


double scaleValue( RNG& rng )
{
    switch( rng.next() % 10 )
    {
    case 0: return 1.0;
    case 1: return -1.0;
    case 2: return 2.0;
    case 3: return 0.5;
    case 4: return std::nextafter( 1.0, 2.0 );
    case 5: return 1.0 + 1e-6;
    case 6: return -rng.dbl( 0.01, 10.0 );
    case 7: return rng.next() % 50 == 0 ? 0.0 : rng.dbl( 0.001, 0.1 );
    default: return rng.dbl( 0.01, 10.0 );
    }
}


void recTrs( REC& r, const TRANSFORM_TRS& t )
{
    r.v( t.GetTranslate() ).a( t.GetRotate() ).d( t.GetScaleX() ).d( t.GetScaleY() );
}


void sectionTransformTrs()
{
    SECTION sec( "libm:transform_trs" );
    RNG     rng( 6006 );

    for( int n = 0; n < 40000; n++ )
    {
        VECTOR2I  tr = rng.flag() ? rng.ptMid() : VECTOR2I( 0, 0 );
        EDA_ANGLE rot = rng.flag() ? EDA_ANGLE( rng.angleDeg(), DEGREES_T ) : ANGLE_0;
        double    sx = scaleValue( rng );
        double    sy = rng.flag() ? sx : scaleValue( rng );

        TRANSFORM_TRS t( tr, rot, sx, sy );

        VECTOR2I p = rng.ptMid();
        VECTOR2D pd = rng.ptD( -1e8, 1e8 );
        VECTOR2I fixed = rng.ptMid();
        double   rsx = scaleValue( rng );
        double   rsy = scaleValue( rng );
        double   len = rng.dbl( -1e7, 1e7 );

        REC r;
        recTrs( r, t );
        r.v( p ).vd( pd ).v( fixed ).d( rsx ).d( rsy ).d( len );
        r.v( t.Apply( p ) ).v( t.InverseApply( p ) ).v( t.InverseApply( t.Apply( p ) ) );
        r.vd( t.Apply( pd ) ).vd( t.InverseApply( pd ) );
        r.b( t.IsIdentity() ).b( t.IsUniformScale() ).b( t.HasUnitScale() );
        r.d( t.ApplyLinearScale( len ) ).d( t.InverseApplyLinearScale( len ) );

        TRANSFORM_TRS rs = t.RescaleAround( fixed, rsx, rsy );
        recTrs( r, rs );
        r.b( rs == t ).b( rs != t ).b( t == TRANSFORM_TRS( tr, rot, sx, sy ) );

        TRANSFORM_TRS s;
        s.SetTranslate( fixed );
        s.SetRotate( rot );
        s.SetScale( rsx, rsy );
        recTrs( r, s );
        r.v( s.Apply( t.Apply( p ) ) );
        sec.add( r );
    }
}


void sectionTransform()
{
    SECTION sec( "libm:transform" );
    RNG     rng( 6007 );

    for( int n = 0; n < 30000; n++ )
    {
        TRANSFORM t;

        do
        {
            int lim = rng.flag() ? 1 : 3;
            int x1 = (int) rng.range( -lim, lim );
            int y1 = (int) rng.range( -lim, lim );
            int x2 = (int) rng.range( -lim, lim );
            int y2 = (int) rng.range( -lim, lim );
            t = TRANSFORM( x1, y1, x2, y2 );
        } while( t.x1 * t.y2 - t.x2 * t.y1 == 0 );

        VECTOR2I p = rng.ptMid();
        VECTOR2I bp = rng.ptMid();
        VECTOR2L bs = rng.ptL( -100000000, 100000000 );
        BOX2I    box( bp, bs );
        EDA_ANGLE a1( rng.angleDeg(), DEGREES_T );
        EDA_ANGLE a2( rng.angleDeg(), DEGREES_T );

        if( rng.next() % 6 == 0 )
            a2 = a1;

        REC r;
        r.i( t.x1 ).i( t.y1 ).i( t.x2 ).i( t.y2 ).v( p ).box( box ).a( a1 ).a( a2 );
        r.v( t.TransformCoordinate( p ) ).box( t.TransformCoordinate( box ) );
        TRANSFORM inv = t.InverseTransform();
        r.i( inv.x1 ).i( inv.y1 ).i( inv.x2 ).i( inv.y2 );
        r.b( t == inv ).b( t != inv ).b( t == TRANSFORM() );
        bool swapped = t.MapAngles( &a1, &a2 );
        r.b( swapped ).a( a1 ).a( a2 );
        r.u( std::hash<TRANSFORM>{}( t ) );
        sec.add( r );
    }
}


void sectionDistribute()
{
    SECTION sec( "distribute" );
    RNG     rng( 6008 );

    for( int n = 0; n < 20000; n++ )
    {
        int count = (int) rng.range( 0, 12 );
        int lim = rng.flag() ? 1000 : 100000000;
        std::vector<std::pair<int, int>> extents;
        std::vector<int>                 positions;

        for( int i = 0; i < count; i++ )
        {
            int a = (int) rng.range( -lim, lim );
            int w = (int) rng.range( 0, lim / 10 );
            extents.emplace_back( a, a + w );
            positions.push_back( (int) rng.range( -lim, lim ) );
        }

        if( rng.flag() )
            std::sort( extents.begin(), extents.end() );

        if( rng.flag() )
            std::sort( positions.begin(), positions.end() );

        REC r;
        r.u( extents.size() );

        for( const auto& [s, e] : extents )
            r.i( s ).i( e );

        for( int p : positions )
            r.i( p );

        for( int d : GetDeltasForDistributeByGaps( extents ) )
            r.i( d );

        for( int d : GetDeltasForDistributeByPoints( positions ) )
            r.i( d );

        sec.add( r );
    }
}


void sectionGridGeometry()
{
    SECTION sec( "libm:grid_geometry" );
    RNG     rng( 6009 );

    for( int n = 0; n < 30000; n++ )
    {
        GRID_GEOMETRY g;
        g.kind = rng.flag() ? GRID_GEOMETRY::KIND::CARTESIAN : GRID_GEOMETRY::KIND::POLAR;
        g.origin = rng.flag() ? VECTOR2D( 0, 0 ) : rng.ptD( -1e8, 1e8 );

        bool polar = g.kind == GRID_GEOMETRY::KIND::POLAR;

        switch( rng.next() % 6 )
        {
        case 0: g.pitch = VECTOR2D( 0, rng.dbl( -1, 1 ) ); break;
        case 1: g.pitch = VECTOR2D( rng.dbl( -1e6, 0 ), 1e5 ); break;
        default:
        {
            double px = rng.dbl( 1e3, 1e6 );
            double py = polar ? rng.dbl( 0.01, 1.0 ) : rng.dbl( 1e3, 1e6 );
            g.pitch = VECTOR2D( px, py );
            break;
        }
        }

        g.orientation = rng.flag() ? 0.0 : rng.dbl( -7.0, 7.0 );

        double ex = rng.next() % 10 == 0 ? -rng.dbl( 0, 1e7 ) : rng.dbl( 0, 5e7 );
        double ey = polar ? rng.dbl( -7.0, 7.0 ) : rng.dbl( 0, 5e7 );
        g.extent = VECTOR2D( ex, ey );
        g.priority = (unsigned) rng.range( 0, 3 );

        GRID_GEOMETRY o = g;
        o.priority = (unsigned) rng.range( 0, 3 );
        double oex = rng.dbl( 0, 5e7 );
        double oey = polar ? rng.dbl( -7.0, 7.0 ) : rng.dbl( 0, 5e7 );
        o.extent = VECTOR2D( oex, oey );

        VECTOR2D p = rng.flag() ? rng.ptD( -1e8, 1e8 ) : g.origin + rng.ptD( -1e5, 1e5 );
        double   tol = rng.flag() ? 0.0 : rng.dbl( 0, 1e5 );

        REC r;
        r.b( polar ).vd( g.origin ).vd( g.pitch ).d( g.orientation ).vd( g.extent ).u( g.priority );
        r.u( o.priority ).vd( o.extent ).vd( p ).d( tol );
        r.vd( g.Snap( p ) ).b( g.Contains( p, tol ) ).d( g.Area() );
        r.b( g.TakesPrecedenceOver( o ) ).b( o.TakesPrecedenceOver( g ) );
        sec.add( r );
    }
}


void sectionPredicates()
{
    SECTION sec( "geometry_predicates" );
    RNG     rng( 6010 );

    for( int n = 0; n < 40000; n++ )
    {
        VECTOR2I a, b, c, p;

        switch( rng.next() % 5 )
        {
        case 0:
            a = rng.pt(); b = rng.pt(); c = rng.pt(); p = rng.pt();
            break;
        case 1:
        {
            // full int range (exercises the double fallback of InCircleDelaunayLegal)
            a = rng.ptR( INT_MIN, INT_MAX );
            b = rng.ptR( INT_MIN, INT_MAX );
            c = rng.ptR( INT_MIN, INT_MAX );
            p = rng.ptR( INT_MIN, INT_MAX );
            break;
        }
        case 2:
        {
            // cocircular-ish: axis-aligned points of a circle and a probe near it
            VECTOR2I ctr = rng.ptR( -1000000, 1000000 );
            int      rad = (int) rng.range( 1, 100000000 );
            a = ctr + VECTOR2I( rad, 0 );
            b = ctr + VECTOR2I( 0, rad );
            c = ctr + VECTOR2I( -rad, 0 );
            p = ctr + VECTOR2I( 0, -rad ) + rng.ptR( -2, 2 );
            break;
        }
        case 3:
        {
            // collinear / degenerate
            VECTOR2I o = rng.ptMid();
            VECTOR2I d = rng.ptR( -1000, 1000 );
            a = o;
            b = o + d * (int) rng.range( -100, 100 );
            int      kc = (int) rng.range( -100, 100 );
            VECTOR2I e = rng.ptR( -1, 1 );
            c = o + d * kc + e;
            p = rng.flag() ? a : rng.ptMid();
            break;
        }
        default:
            a = rng.ptR( -10, 10 ); b = rng.ptR( -10, 10 ); c = rng.ptR( -10, 10 ); p = rng.ptR( -10, 10 );
            break;
        }

        REC r;
        r.v( a ).v( b ).v( c ).v( p );
        r.i( KIGEOM::OrientationSign( a, b, c ) ).i( KIGEOM::OrientationSign( a, c, b ) );
        r.b( KIGEOM::InCircleDelaunayLegal( a, b, c, p ) );
        r.b( KIGEOM::detail::inCircleLegalDouble( a, b, c, p ) );
        r.b( KIGEOM::IsSliverTriangle( a, b, c ) );
        sec.add( r );
    }
}


void sectionVector3()
{
    SECTION sec( "vector3" );
    RNG     rng( 6011 );

    for( int n = 0; n < 20000; n++ )
    {
        int lim = rng.flag() ? 1000 : 30000;
        VECTOR3I a, b;
        a.x = (int) rng.range( -lim, lim );
        a.y = (int) rng.range( -lim, lim );
        a.z = (int) rng.range( -lim, lim );
        b.x = (int) rng.range( -lim, lim );
        b.y = (int) rng.range( -lim, lim );
        b.z = (int) rng.range( -lim, lim );
        int      s = (int) rng.range( -100, 100 );

        if( s == 0 )
            s = 1;

        VECTOR3D ad, bd;
        ad.x = rng.dbl( -1e6, 1e6 );
        ad.y = rng.dbl( -1e6, 1e6 );
        ad.z = rng.dbl( -1e6, 1e6 );
        bd.x = rng.dbl( -1e6, 1e6 );
        bd.y = rng.dbl( -1e6, 1e6 );
        bd.z = rng.dbl( -1e6, 1e6 );
        double   sd = rng.dbl( -10, 10 );

        REC r;
        r.i( a.x ).i( a.y ).i( a.z ).i( b.x ).i( b.y ).i( b.z ).i( s );
        VECTOR3I cr = a.Cross( b );
        r.i( cr.x ).i( cr.y ).i( cr.z ).i( a.Dot( b ) ).i( a.EuclideanNorm() );
        VECTOR3I m = a;
        m *= s;
        VECTOR3I dv = a;
        dv /= s;
        VECTOR3I nn = a;
        VECTOR3I nr = nn.Normalize();
        r.i( m.x ).i( m.y ).i( m.z ).i( dv.x ).i( dv.y ).i( dv.z ).i( nr.x ).i( nr.y ).i( nr.z );
        r.b( a == b ).b( a != b );

        r.d( ad.x ).d( ad.y ).d( ad.z ).d( bd.x ).d( bd.y ).d( bd.z ).d( sd );
        VECTOR3D crd = ad.Cross( bd );
        r.d( crd.x ).d( crd.y ).d( crd.z ).d( ad.Dot( bd ) ).d( ad.EuclideanNorm() );
        VECTOR3D md = ad;
        md *= sd;
        VECTOR3D dd = ad;
        dd /= sd;
        VECTOR3D nd = ad;
        nd.Normalize();
        r.d( md.x ).d( md.y ).d( md.z ).d( dd.x ).d( dd.y ).d( dd.z ).d( nd.x ).d( nd.y ).d( nd.z );

        std::ostringstream ss;
        ss << a << ad;
        r.s( ss.str() );
        sec.add( r );
    }
}


std::vector<uint8_t> randomBytes( RNG& rng )
{
    size_t len;

    switch( rng.next() % 4 )
    {
    case 0: len = (size_t) rng.range( 0, 20 ); break;
    case 1: len = (size_t) rng.range( 0, 80 ); break;
    case 2: len = (size_t) rng.range( 55, 70 ); break;
    default: len = (size_t) rng.range( 0, 1000 ); break;
    }

    std::vector<uint8_t> v( len );

    for( uint8_t& b : v )
        b = (uint8_t) rng.next();

    return v;
}


void recBytes( REC& r, const std::vector<uint8_t>& aBytes )
{
    std::string hex;
    char        buf[3];

    for( uint8_t b : aBytes )
    {
        snprintf( buf, sizeof( buf ), "%02x", b );
        hex += buf;
    }

    r.s( hex );
}


void sectionHashes()
{
    SECTION sec( "hashes" );
    RNG     rng( 6012 );

    for( int n = 0; n < 30000; n++ )
    {
        std::vector<uint8_t> bytes = randomBytes( rng );
        uint32_t             seed = rng.flag() ? 0 : (uint32_t) rng.next();
        REC                  r;
        recBytes( r, bytes );
        r.u( seed );

        // MMH3_HASH: addData in random pieces, interleaved with add( int32_t )
        {
            MMH3_HASH h( seed );
            size_t    pos = 0;

            while( pos < bytes.size() )
            {
                size_t piece = std::min<size_t>( bytes.size() - pos, (size_t) rng.range( 0, 40 ) );

                // add( int32_t ) after a misaligned addData() hashes a block byte that was
                // never written (uninitialised in C++), so mix them only at aligned lengths
                if( rng.next() % 4 == 0 && piece >= 4 && pos % 4 == 0 )
                {
                    int32_t v;
                    memcpy( &v, bytes.data() + pos, 4 );
                    h.add( v );
                    pos += 4;
                    r.i( -4 );
                }
                else
                {
                    h.addData( bytes.data() + pos, piece );
                    pos += piece;
                    r.u( piece );
                }
            }

            r.s( h.digest().ToString() );
        }

        // addDataV1, std::string and std::vector<char> overloads, reset
        {
            MMH3_HASH h( seed );
            h.addDataV1( bytes.data(), bytes.size() );
            r.s( h.digest().ToString() );

            MMH3_HASH h2;
            h2.add( std::string( bytes.begin(), bytes.end() ) );
            h2.add( std::vector<char>( bytes.begin(), bytes.end() ) );
            h2.add( (int32_t) seed );
            r.s( h2.digest().ToString() );

            h2.reset( seed );
            size_t half = bytes.size() / 2;
            h2.addDataV1( bytes.data(), half );
            h2.addDataV1( bytes.data() + half, bytes.size() - half );
            r.s( h2.digest().ToString() );
        }

        // MD5_HASH
        {
            MD5_HASH m;
            size_t   half = bytes.size() / 3;
            m.Hash( bytes.data(), (uint32_t) half );
            m.Hash( (int) seed );
            m.Hash( bytes.data() + half, (uint32_t) ( bytes.size() - half ) );
            r.b( m.IsValid() );
            m.Finalize();
            r.b( m.IsValid() ).s( m.Format( false ) ).s( m.Format( true ) );

            MD5_HASH m2( m );
            r.b( m2 == m ).b( m2 != m );
            m2.Finalize();  // finalizing twice keeps hashing the padded context
            r.s( m2.Format( true ) ).b( m2 == m );

            MD5_HASH m3;
            m3.Hash( bytes.data(), (uint32_t) bytes.size() );
            m3.Finalize();
            r.s( m3.Format( true ) );
            m3.Init();
            r.b( m3.IsValid() ).s( m3.Format( true ) );
        }

        // hash_combine / hash_val / std::hash
        {
            int         i1 = (int) rng.next();
            int64_t     i2 = (int64_t) rng.next();
            double      d1 = rng.next() % 8 == 0 ? 0.0 : rng.dbl( -1e9, 1e9 );
            double      d2 = rng.next() % 8 == 0 ? -0.0 : rng.dblScaled( -60, 60 );
            bool        b1 = rng.flag();
            unsigned    u1 = (unsigned) rng.next();
            std::string s1( bytes.begin(), bytes.begin() + std::min<size_t>( bytes.size(), 50 ) );

            r.i( i1 ).i( i2 ).d( d1 ).d( d2 ).b( b1 ).u( u1 );
            r.u( std::hash<double>{}( d1 ) ).u( std::hash<double>{}( d2 ) );
            r.u( std::hash<std::string>{}( s1 ) ).u( std::hash<float>{}( (float) d1 ) );
            r.u( hash_val( i1 ) ).u( hash_val( i1, i2, d1, d2, b1, u1, s1 ) );

            std::size_t seedc = (std::size_t) rng.next();
            r.u( seedc );
            hash_combine( seedc, s1, i1, d2 );
            r.u( seedc );
        }

        // HASH_128
        {
            HASH_128 h;
            h.Value64[0] = rng.next();
            h.Value64[1] = rng.next();
            HASH_128 z{};
            r.s( h.ToString() ).b( h == z ).u( h.Value32[1] ).u( h.Value8[9] );
            h.Clear();
            r.b( h == z );
        }

        sec.add( r );
    }
}

} // namespace


int main( int argc, char** argv )
{
    // The overflow logging of KiROUND and the wxCHECKs call the wx assert handler; they do
    // not change any result
    wxSetAssertHandler( nullptr );

    if( argc == 3 && std::string( argv[1] ) == "--dump" )
        g_dump = fopen( argv[2], "w" );

    sectionLibmProbe();
    sectionBezierPolyI();
    sectionBezierPolyD();
    sectionBezierTemplate();
    sectionEllipse();
    sectionConvexHull();
    sectionTransformTrs();
    sectionTransform();
    sectionDistribute();
    sectionGridGeometry();
    sectionPredicates();
    sectionVector3();
    sectionHashes();

    if( g_dump )
        fclose( g_dump );

    return 0;
}
