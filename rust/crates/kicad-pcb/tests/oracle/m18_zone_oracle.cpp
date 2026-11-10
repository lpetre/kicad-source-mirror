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

// Package M18 (ZONE model) differential oracle. Loads tests/oracle/m18_zones.kicad_pcb with the
// real parser (KI_TEST::ReadBoardFromFileOrStream) and prints, per zone, the members the Rust
// replay rebuilds the zone from and the derived values of pcbnew/zone.cpp and zone_utils.cpp:
// board outline, bounding box, hatch lines, BuildSmoothedPoly (with / without board outline,
// with apron), TransformSmoothedOutlineToPolygon, interacting zones, hit tests, description,
// Move / Rotate / Flip / Mirror results, fill containers and hashes, then
// MergeZonesWithSameOutline and AutoAssignZonePriorities. Polygons other than the stored outline
// are printed as the FNV-1a hash of their pcb-dump text (VPolySet). Built and run by
// m18_zone_golden.sh; the Rust replay is zone::oracle_tests.

#include "pcb_dump.h"

#include <cstdio>
#include <cstdlib>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/wx_utils/wx_assert.h>
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <footprint.h>
#include <zone.h>
#include <zone_utils.h>
#include <units_provider.h>
#include <geometry/shape_line_chain.h>


static std::string H( const SHAPE_POLY_SET& aSet )
{
    char buf[32];
    snprintf( buf, sizeof( buf ), "#%016llx", (unsigned long long) Fnv1a64( VPolySet( aSet ) ) );
    return buf;
}


static std::string Key( const BOARD_ITEM* aItem )
{
    std::string s = aItem->m_Uuid.AsStdString();
    return s.substr( s.size() - 2 );
}


static std::string Segs( const std::vector<SEG>& aSegs )
{
    std::string r;

    for( const SEG& s : aSegs )
        r += ( r.empty() ? "" : " " ) + V( s.A ) + ">" + V( s.B );

    char buf[64];
    snprintf( buf, sizeof( buf ), "n=%zu #%016llx", aSegs.size(),
              (unsigned long long) Fnv1a64( "[" + r + "]" ) );
    return buf;
}


static void P( const std::string& aKey, const std::string& aField, const std::string& aValue )
{
    printf( "zone|%s|%s=%s\n", aKey.c_str(), aField.c_str(), aValue.c_str() );
}


static void dumpZone( BOARD* aBoard, ZONE* z, const std::vector<ZONE*>& aAll )
{
    const std::string k = Key( z );
    FOOTPRINT*        fp = z->GetParentFootprint();

    // ---- members (the Rust replay rebuilds the zone from these) ----
    P( k, "parent", fp ? Key( fp ) : "none" );
    P( k, "net", V( z->GetNetCode() ) );
    P( k, "layers", V( z->GetLayerSet() ) );
    P( k, "priority", V( z->GetAssignedPriority() ) );
    P( k, "rule_area", V( z->GetIsRuleArea() ) );
    P( k, "teardrop", V( (int) z->GetTeardropAreaType() ) );
    P( k, "smoothing", V( (int) z->GetCornerSmoothingType() ) );
    P( k, "radius", V( z->GetCornerRadius() ) );
    P( k, "min_thickness", V( z->GetMinThickness() ) );
    P( k, "border_style", V( (int) z->GetHatchStyle() ) );
    P( k, "border_pitch", V( z->GetBorderHatchPitch() ) );
    P( k, "clearance", V( z->GetLocalClearance().value_or( -1 ) ) );
    P( k, "name", V( z->GetZoneName() ) );
    P( k, "uuid", V( z->m_Uuid ) );
    P( k, "keepout", V( z->GetDoNotAllowZoneFills() ) + "," + V( z->GetDoNotAllowVias() ) + ","
                             + V( z->GetDoNotAllowTracks() ) + "," + V( z->GetDoNotAllowPads() )
                             + "," + V( z->GetDoNotAllowFootprints() ) );
    P( k, "lib_outline", VPolySet( *z->Outline() ) );

    // ---- derived ----
    P( k, "first_layer", V( (int) z->GetFirstLayer() ) );
    P( k, "layer", V( (int) z->GetLayer() ) );
    P( k, "on_copper", V( z->IsOnCopperLayer() ) );
    P( k, "bbox", V( z->GetBoundingBox() ) );
    P( k, "board_outline", VPolySet( z->GetBoardOutline() ) );
    P( k, "hatch", Segs( z->GetHatchLines() ) );
    P( k, "position", V( z->GetPosition() ) );
    P( k, "local_clearance", VOpt( z->GetLocalClearance() ) );
    P( k, "local_clearance_src", VOpt( z->GetLocalClearance( nullptr ) ) );
    P( k, "friendly", V( z->GetFriendlyName() ) );

    UNITS_PROVIDER units( pcbIUScale, EDA_UNITS::MM );
    P( k, "desc", V( z->GetItemDescription( &units, true ) ) );
    P( k, "outline_area", V( z->CalculateOutlineArea() ) );
    P( k, "num_corners", V( z->GetNumCorners() ) );

    SHAPE_POLY_SET boardEdge;
    boardEdge.NewOutline();
    boardEdge.Append( pcbIUScale.mmToIU( 95 ), pcbIUScale.mmToIU( 95 ) );
    boardEdge.Append( pcbIUScale.mmToIU( 129 ), pcbIUScale.mmToIU( 95 ) );
    boardEdge.Append( pcbIUScale.mmToIU( 129 ), pcbIUScale.mmToIU( 150 ) );
    boardEdge.Append( pcbIUScale.mmToIU( 95 ), pcbIUScale.mmToIU( 150 ) );

    for( PCB_LAYER_ID l : z->GetLayerSet().Seq() )
    {
        const std::string ln = VLayer( l );
        SHAPE_POLY_SET    smoothed, apron, smoothedBo;

        bool ok = z->BuildSmoothedPoly( smoothed, l, nullptr, &apron );
        P( k, "smoothed." + ln, V( ok ) + " " + H( smoothed ) );
        P( k, "apron." + ln, H( apron ) );
        z->BuildSmoothedPoly( smoothedBo, l, &boardEdge );
        P( k, "smoothed_bo." + ln, H( smoothedBo ) );

        SHAPE_POLY_SET tso;
        z->TransformSmoothedOutlineToPolygon( tso, l, pcbIUScale.mmToIU( 0.2 ), aBoard->GetDesignSettings().m_MaxError,
                                              ERROR_OUTSIDE, nullptr );
        P( k, "tso." + ln, H( tso ) );
        SHAPE_POLY_SET tso0;
        z->TransformSmoothedOutlineToPolygon( tso0, l, 0, aBoard->GetDesignSettings().m_MaxError, ERROR_INSIDE,
                                              &boardEdge );
        P( k, "tso0." + ln, H( tso0 ) );

        std::vector<ZONE*> same, other;
        z->GetInteractingZones( l, &same, &other );
        std::string s;

        for( ZONE* o : same )
            s += Key( o ) + " ";

        s += "|";

        for( ZONE* o : other )
            s += " " + Key( o );

        P( k, "interacting." + ln, s );
    }

    // ---- hit tests ----
    BOX2I    bb = z->GetBoundingBox();
    VECTOR2I c = bb.Centre();
    SHAPE_POLY_SET bo = z->GetBoardOutline();
    VECTOR2I v0 = bo.CVertex( 0 );
    VECTOR2I v1 = bo.CVertex( 1 );
    VECTOR2I mid = ( v0 + v1 ) / 2;
    std::vector<VECTOR2I> pts = { v0, mid, c, v0 + VECTOR2I( 150000, 0 ), mid + VECTOR2I( 0, 80000 ),
                                  VECTOR2I( 0, 0 ) };
    std::string hits;

    for( const VECTOR2I& p : pts )
    {
        hits += z->HitTest( p, 0 ) ? '1' : '0';
        hits += z->HitTest( p, 200000 ) ? '1' : '0';
        hits += z->HitTestFilledArea( z->GetFirstLayer(), p, 0 ) ? '1' : '0';
        int oi = -1, hi = -1;
        hits += z->HitTestCutout( p, &oi, &hi ) ? '1' : '0';
        hits += ",";
    }

    P( k, "hits", hits );

    SHAPE_POLY_SET::VERTEX_INDEX vi;
    bool cornerHit = z->HitTestForCorner( v1 + VECTOR2I( 1000, 0 ), 5000, &vi );
    P( k, "corner_hit", V( cornerHit ) + " " + V( vi.m_polygon ) + "," + V( vi.m_contour ) + "," + V( vi.m_vertex ) );
    bool edgeHit = z->HitTestForEdge( mid + VECTOR2I( 0, 1000 ), 5000, &vi );
    P( k, "edge_hit", V( edgeHit ) + " " + V( vi.m_polygon ) + "," + V( vi.m_contour ) + "," + V( vi.m_vertex ) );

    BOX2I big = bb;
    big.Inflate( 1000 );
    BOX2I small( c - VECTOR2I( 500000, 500000 ), VECTOR2I( 1000000, 1000000 ) );
    BOX2I corner( v0 - VECTOR2I( 100000, 100000 ), VECTOR2I( 200000, 200000 ) );
    P( k, "hit_rect", std::string( z->HitTest( big, true, 0 ) ? "1" : "0" )
                              + ( z->HitTest( bb, true, 0 ) ? "1" : "0" )
                              + ( z->HitTest( small, false, 0 ) ? "1" : "0" )
                              + ( z->HitTest( corner, false, 0 ) ? "1" : "0" )
                              + ( z->HitTest( small, true, 0 ) ? "1" : "0" ) );

    SHAPE_LINE_CHAIN sel;
    sel.Append( bb.GetOrigin() - VECTOR2I( 1000, 1000 ) );
    sel.Append( VECTOR2I( bb.GetRight() + 1000, bb.GetTop() - 1000 ) );
    sel.Append( VECTOR2I( bb.GetRight() + 1000, bb.GetBottom() + 1000 ) );
    sel.Append( VECTOR2I( bb.GetLeft() - 1000, bb.GetBottom() + 1000 ) );
    sel.SetClosed( true );
    SHAPE_LINE_CHAIN sel2;
    sel2.Append( c );
    sel2.Append( c + VECTOR2I( 300000, 0 ) );
    sel2.Append( c + VECTOR2I( 300000, 300000 ) );
    sel2.SetClosed( true );
    SHAPE_LINE_CHAIN sel3;
    sel3.Append( v0 - VECTOR2I( 50000, 50000 ) );
    sel3.Append( v0 + VECTOR2I( 50000, -50000 ) );
    sel3.Append( v0 + VECTOR2I( 50000, 50000 ) );
    sel3.SetClosed( true );
    P( k, "hit_poly", std::string( z->HitTest( sel, true ) ? "1" : "0" ) + ( z->HitTest( sel, false ) ? "1" : "0" )
                              + ( z->HitTest( sel2, true ) ? "1" : "0" ) + ( z->HitTest( sel2, false ) ? "1" : "0" )
                              + ( z->HitTest( sel3, false ) ? "1" : "0" ) );

    // ---- relations ----
    std::string hp, eq;

    for( ZONE* o : aAll )
    {
        hp += z->HigherPriority( o ) ? '1' : '0';
        eq += ( *z == *static_cast<BOARD_ITEM*>( o ) ) ? '1' : '0';
    }

    P( k, "higher_priority", hp );
    P( k, "equal", eq );

    // ---- transforms (on copies, which keep the parent) ----
    {
        ZONE m( *z );
        m.Move( VECTOR2I( 1000000, -2000000 ) );
        P( k, "move.outline", H( *m.Outline() ) );
        P( k, "move.hatch", Segs( m.GetHatchLines() ) );
        P( k, "move.bbox", V( m.GetBoundingBox() ) );
    }
    {
        ZONE r( *z );
        r.Rotate( c, EDA_ANGLE( 30.0, DEGREES_T ) );
        P( k, "rotate.outline", H( *r.Outline() ) );
        P( k, "rotate.hatch", Segs( r.GetHatchLines() ) );
        P( k, "rotate.bbox", V( r.GetBoundingBox() ) );
    }
    {
        ZONE f( *z );
        f.SetFilledPolysList( z->GetFirstLayer(), z->GetBoardOutline() );
        f.Flip( c, FLIP_DIRECTION::LEFT_RIGHT );
        P( k, "flip.outline", H( *f.Outline() ) );
        P( k, "flip.layers", V( f.GetLayerSet() ) );
        P( k, "flip.hatch", Segs( f.GetHatchLines() ) );
        P( k, "flip.hatch_orientation", V( f.GetHatchOrientation() ) );
        P( k, "flip.bbox", V( f.GetBoundingBox() ) );
        std::string fl;

        for( PCB_LAYER_ID l : f.GetFilledLayers() )
            fl += VLayer( l ) + ":" + H( *f.GetFilledPolysList( l ) ) + " ";

        P( k, "flip.fills", fl );
    }
    {
        ZONE m( *z );
        m.Mirror( c, FLIP_DIRECTION::TOP_BOTTOM );
        P( k, "mirror.outline", H( *m.Outline() ) );
        P( k, "mirror.hatch", Segs( m.GetHatchLines() ) );
        P( k, "mirror.layers", V( m.GetLayerSet() ) );
    }

    // ---- fill containers ----
    {
        ZONE           f( *z );
        PCB_LAYER_ID   l = z->GetFirstLayer();
        SHAPE_POLY_SET fill = z->GetBoardOutline();
        int            maxError = aBoard->GetDesignSettings().m_MaxError;

        P( k, "fill.has_before", V( f.HasFilledPolysForLayer( l ) ) );
        P( k, "fill.hash_before", f.GetHashValue( l ).ToString() );
        f.SetFilledPolysList( l, fill );
        f.SetIsFilled( true );
        f.BuildHashValue( l );
        P( k, "fill.hash", f.GetHashValue( l ).ToString() );
        P( k, "fill.area", V( f.CalculateFilledArea() ) );

        SHAPE_POLY_SET b0, b1, b2, b3;
        f.TransformShapeToPolygon( b0, l, 0, maxError, ERROR_INSIDE );
        f.TransformShapeToPolygon( b1, l, 100000, maxError, ERROR_OUTSIDE );
        f.TransformShapeToPolygon( b2, l, 100000, maxError, ERROR_INSIDE );
        f.TransformShapeToPolygon( b3, l, -100000, maxError, ERROR_OUTSIDE );
        P( k, "fill.tsp", H( b0 ) + " " + H( b1 ) + " " + H( b2 ) + " " + H( b3 ) );

        SHAPE_POLY_SET solid;
        f.TransformSolidAreasShapesToPolygon( l, solid );
        P( k, "fill.solid", H( solid ) );

        std::shared_ptr<SHAPE> es = f.GetEffectiveShape( l );
        P( k, "fill.effective", V( (int) es->Type() ) + " "
                                        + ( es->Type() == SH_POLY_SET
                                                    ? H( *static_cast<SHAPE_POLY_SET*>( es.get() ) )
                                                    : std::string( "-" ) ) );
        std::shared_ptr<SHAPE> es2 = f.GetEffectiveShape( B_Mask );
        P( k, "fill.effective_other", V( (int) es2->Type() ) );
        P( k, "fill.hit_filled", V( f.HitTestFilledArea( l, c, 0 ) ) );

        bool changed = f.UnFill();
        P( k, "fill.unfill", V( changed ) + " " + V( f.IsFilled() ) + " " + V( f.HasFilledPolysForLayer( l ) ) + " "
                                     + V( f.GetFilledPolysList( l )->OutlineCount() ) );
        P( k, "fill.unfill_again", V( f.UnFill() ) );
    }
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    KI_TEST::SetMockConfigDir();
    SetPgm( new MOCK_PGM_BASE() );
    KIPLATFORM::APP::Init();
    wxApp::SetInstance( new wxAppConsole );

    if( !wxInitialize( argc, argv ) )
        return 1;

    wxLog::EnableLogging( false );
    Pgm().InitPgm( true, true );
    Pgm().GetSettingsManager().RegisterSettings( new PCBNEW_SETTINGS, false );
    Pgm().GetSettingsManager().Load();
    Pgm().GetSettingsManager().LoadProject( "" );

    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( argv[1] );

    if( !board )
        return 2;

    std::vector<ZONE*> all;

    for( ZONE* z : board->Zones() )
        all.push_back( z );

    for( FOOTPRINT* fp : board->Footprints() )
    {
        printf( "fp|%s|translate=%s\n", Key( fp ).c_str(), V( fp->GetTransform().GetTranslate() ).c_str() );
        printf( "fp|%s|rotate=%s\n", Key( fp ).c_str(), V( fp->GetTransform().GetRotate() ).c_str() );
        printf( "fp|%s|scale=%s,%s\n", Key( fp ).c_str(), V( fp->GetTransform().GetScaleX() ).c_str(),
                V( fp->GetTransform().GetScaleY() ).c_str() );
        printf( "fp|%s|layer=%s\n", Key( fp ).c_str(), VLayer( fp->GetLayer() ).c_str() );
        printf( "fp|%s|flipped=%s\n", Key( fp ).c_str(), V( fp->IsFlipped() ).c_str() );
        printf( "fp|%s|uuid=%s\n", Key( fp ).c_str(), V( fp->m_Uuid ).c_str() );

        for( ZONE* z : fp->Zones() )
            all.push_back( z );
    }

    for( ZONE* z : all )
        dumpZone( board.get(), z, all );

    // MergeZonesWithSameOutline on copies of the board zones.
    {
        std::vector<std::unique_ptr<ZONE>> copies;

        for( ZONE* z : board->Zones() )
        {
            copies.push_back( std::make_unique<ZONE>( *z ) );
            copies.back()->SetFilledPolysList( z->GetFirstLayer(), z->GetBoardOutline() );
        }

        std::vector<std::unique_ptr<ZONE>> merged = MergeZonesWithSameOutline( std::move( copies ) );
        printf( "merge|count|n=%zu\n", merged.size() );

        for( const std::unique_ptr<ZONE>& z : merged )
        {
            std::string fl;

            for( PCB_LAYER_ID l : z->GetFilledLayers() )
                fl += VLayer( l ) + ":" + H( *z->GetFilledPolysList( l ) ) + " ";

            printf( "merge|%s|layers=%s filled=%s refill=%s fills=%s\n", Key( z.get() ).c_str(),
                    V( z->GetLayerSet() ).c_str(), V( z->IsFilled() ).c_str(), V( z->NeedRefill() ).c_str(),
                    fl.c_str() );
        }
    }

    bool changed = AutoAssignZonePriorities( board.get(), nullptr );
    printf( "auto|result|changed=%s\n", V( changed ).c_str() );

    for( ZONE* z : board->Zones() )
        printf( "auto|%s|priority=%u\n", Key( z ).c_str(), z->GetAssignedPriority() );

    return 0;
}
