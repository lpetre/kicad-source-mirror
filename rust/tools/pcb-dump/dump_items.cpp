/*
 * This program source code file is part of KiCad, a free EDA CAD application.
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

// pcb-dump: the per-item sections track, pad, shape, text, dim, zone, footprint.

#include "pcb_dump.h"
#include "dump_common.h"

#include <board.h>
#include <board_design_settings.h>
#include <footprint.h>
#include <pad.h>
#include <padstack.h>
#include <pcb_dimension.h>
#include <pcb_field.h>
#include <pcb_shape.h>
#include <pcb_text.h>
#include <pcb_textbox.h>
#include <pcb_tablecell.h>
#include <pcb_track.h>
#include <zone.h>
#include <jumper_group.h>
#include <constraints/constraint_builder.h>
#include <geometry/shape_compound.h>
#include <geometry/shape_poly_set.h>
#include <geometry/shape_segment.h>


// ---- shared helpers -----------------------------------------------------------------------

const std::vector<POLY_COMBO>& PolyCombos()
{
    static const std::vector<POLY_COMBO> combos = {
        { 0, 5000, ERROR_INSIDE, "c0_e5000_in" },
        { 0, 1000, ERROR_OUTSIDE, "c0_e1000_out" },
        { 100000, 5000, ERROR_OUTSIDE, "c100000_e5000_out" },
    };

    return combos;
}


void DumpTransformedPolys( BOARD_ITEM* aItem, PCB_LAYER_ID aLayer )
{
    for( const POLY_COMBO& c : PolyCombos() )
    {
        Guard( "tpoly",
               [&]()
               {
                   SHAPE_POLY_SET buf;
                   aItem->TransformShapeToPolygon( buf, aLayer, c.clearance, c.maxError, c.errorLoc );
                   EMIT( "tpoly." + VLayer( aLayer ) + "." + c.name, VPolySet( buf ) );
               } );
    }
}


void DumpEffectiveShape( const std::string& aField, BOARD_ITEM* aItem, PCB_LAYER_ID aLayer,
                         FLASHING aFlash )
{
    Guard( aField,
           [&]()
           {
               std::shared_ptr<SHAPE> s = aItem->GetEffectiveShape( aLayer, aFlash );
               EMIT( aField, VShape( s.get() ) );
           } );
}


void DumpPadstack( const std::string& p, const PADSTACK& ps )
{
    EMIT( p + "mode", VEnum( ps.Mode() ) );
    EMIT( p + "layerset", V( ps.LayerSet() ) );
    EMIT( p + "layers", VLayerNames( ps.LayerSet() ) );
    EMIT( p + "start_layer", VLayer( ps.StartLayer() ) );
    EMIT( p + "end_layer", VLayer( ps.EndLayer() ) );
    EMIT( p + "name", V( ps.Name() ) );
    EMIT( p + "orientation", V( ps.GetOrientation() ) );
    EMIT( p + "unconnected_layer_mode", VEnum( ps.UnconnectedLayerMode() ) );
    EMIT( p + "custom_shape_in_zone_mode", VEnum( ps.CustomShapeInZoneMode() ) );
    EMIT( p + "max_hole_size", V( ps.GetMaxHoleSize() ) );

    auto drill = [&]( const std::string& n, const PADSTACK::DRILL_PROPS& d )
    {
        EMIT( p + n + ".size", V( d.size ) );
        EMIT( p + n + ".shape", VEnum( d.shape ) );
        EMIT( p + n + ".start", VLayer( d.start ) );
        EMIT( p + n + ".end", VLayer( d.end ) );
        EMIT( p + n + ".is_filled", VOpt( d.is_filled ) );
        EMIT( p + n + ".is_capped", VOpt( d.is_capped ) );
    };

    drill( "drill", ps.Drill() );
    drill( "secondary_drill", ps.SecondaryDrill() );
    drill( "tertiary_drill", ps.TertiaryDrill() );

    auto pm = [&]( const std::string& n, const PADSTACK::POST_MACHINING_PROPS& d )
    {
        EMIT( p + n + ".mode", VOpt( d.mode ) );
        EMIT( p + n + ".size", V( d.size ) );
        EMIT( p + n + ".depth", V( d.depth ) );
        EMIT( p + n + ".angle", V( d.angle ) );
    };

    pm( "front_post_machining", ps.FrontPostMachining() );
    pm( "back_post_machining", ps.BackPostMachining() );

    EMIT( p + "backdrill_mode", VEnum( ps.GetBackdrillMode() ) );

    auto mask = [&]( const std::string& n, const PADSTACK::MASK_LAYER_PROPS& m )
    {
        EMIT( p + n + ".solder_mask_margin", VOpt( m.solder_mask_margin ) );
        EMIT( p + n + ".solder_paste_margin", VOpt( m.solder_paste_margin ) );
        EMIT( p + n + ".solder_paste_margin_ratio", VOpt( m.solder_paste_margin_ratio ) );
        EMIT( p + n + ".has_solder_mask", VOpt( m.has_solder_mask ) );
        EMIT( p + n + ".has_solder_paste", VOpt( m.has_solder_paste ) );
        EMIT( p + n + ".has_covering", VOpt( m.has_covering ) );
        EMIT( p + n + ".has_plugging", VOpt( m.has_plugging ) );
    };

    mask( "front_outer", ps.FrontOuterLayers() );
    mask( "back_outer", ps.BackOuterLayers() );

    EMIT( p + "is_tented.F", VOpt( ps.IsTented( F_Cu ) ) );
    EMIT( p + "is_tented.B", VOpt( ps.IsTented( B_Cu ) ) );
    EMIT( p + "is_covered.F", VOpt( ps.IsCovered( F_Cu ) ) );
    EMIT( p + "is_covered.B", VOpt( ps.IsCovered( B_Cu ) ) );
    EMIT( p + "is_plugged.F", VOpt( ps.IsPlugged( F_Cu ) ) );
    EMIT( p + "is_plugged.B", VOpt( ps.IsPlugged( B_Cu ) ) );
    EMIT( p + "is_capped", VOpt( ps.IsCapped() ) );
    EMIT( p + "is_filled", VOpt( ps.IsFilled() ) );
    EMIT( p + "unique_layers", VLayers( ps.UniqueLayers() ) );

    for( PCB_LAYER_ID l : ps.UniqueLayers() )
    {
        const std::string lp = p + "copper." + VLayer( l ) + ".";
        const PADSTACK::COPPER_LAYER_PROPS& c = ps.CopperLayer( l );
        EMIT( lp + "shape", VEnum( c.shape.shape ) );
        EMIT( lp + "anchor_shape", VEnum( c.shape.anchor_shape ) );
        EMIT( lp + "size", V( c.shape.size ) );
        EMIT( lp + "offset", V( c.shape.offset ) );
        EMIT( lp + "round_rect_radius_ratio", V( c.shape.round_rect_radius_ratio ) );
        EMIT( lp + "chamfered_rect_ratio", V( c.shape.chamfered_rect_ratio ) );
        EMIT( lp + "chamfered_rect_positions", V( c.shape.chamfered_rect_positions ) );
        EMIT( lp + "trapezoid_delta_size", V( c.shape.trapezoid_delta_size ) );
        EMIT( lp + "zone_connection", VOpt( c.zone_connection ) );
        EMIT( lp + "thermal_spoke_width", VOpt( c.thermal_spoke_width ) );
        EMIT( lp + "thermal_spoke_angle",
                    c.thermal_spoke_angle ? V( c.thermal_spoke_angle->GetAngle() ) : "none" );
        EMIT( lp + "thermal_gap", VOpt( c.thermal_gap ) );
        EMIT( lp + "clearance", VOpt( c.clearance ) );
        EMIT( lp + "round_rect_radius", V( ps.RoundRectRadius( l ) ) );
        EMIT( lp + "thermal_spoke_angle_effective", V( ps.ThermalSpokeAngle( l ) ) );
        EMIT( lp + "primitives", V( (int) c.custom_shapes.size() ) );

        int i = 0;

        for( const std::shared_ptr<PCB_SHAPE>& prim : c.custom_shapes )
        {
            const std::string pp = lp + "primitive." + std::to_string( i++ ) + ".";
            EMIT( pp + "shape", VEnum( prim->GetShape() ) );
            EMIT( pp + "start", V( prim->GetStart() ) );
            EMIT( pp + "end", V( prim->GetEnd() ) );
            EMIT( pp + "width", V( prim->GetWidth() ) );
            EMIT( pp + "fill", VEnum( prim->GetFillMode() ) );

            if( prim->GetShape() == SHAPE_T::ARC )
                EMIT( pp + "mid", V( prim->GetArcMid() ) );

            if( prim->GetShape() == SHAPE_T::BEZIER )
                EMIT( pp + "bezier", VPoints( { prim->GetBezierC1(), prim->GetBezierC2() } ) );

            if( prim->GetShape() == SHAPE_T::POLY )
                EMIT( pp + "poly", VPolySet( prim->GetPolyShape() ) );

            if( prim->GetShape() == SHAPE_T::RECTANGLE )
                EMIT( pp + "corner_radius", V( prim->GetCornerRadius() ) );
        }
    }
}


// ---- track / arc / via --------------------------------------------------------------------

void DumpTrack( BOARD_ITEM* aItem )
{
    BOARD*     board = aItem->GetBoard();
    PCB_TRACK* t = static_cast<PCB_TRACK*>( aItem );

    EMIT( "start", V( t->GetStart() ) );
    EMIT( "end", V( t->GetEnd() ) );
    if( t->Type() != PCB_VIA_T )
    {
        EMIT( "width", V( t->GetWidth() ) );
        EMIT( "length", V( t->GetLength() ) );
    }

    EMIT( "is_null", V( t->IsNull() ) );
    EMIT( "has_solder_mask", V( t->HasSolderMask() ) );
    EMIT( "local_solder_mask_margin", VOpt( t->GetLocalSolderMaskMargin() ) );
    EMIT( "solder_mask_expansion", V( t->GetSolderMaskExpansion() ) );

    if( t->Type() == PCB_ARC_T )
    {
        PCB_ARC* a = static_cast<PCB_ARC*>( t );
        EMIT( "mid", V( a->GetMid() ) );
        EMIT( "center", V( a->GetCenter() ) );
        EMIT( "radius", V( a->GetRadius() ) );
        EMIT( "angle", V( a->GetAngle() ) );
        EMIT( "angle_start", V( a->GetArcAngleStart() ) );
        EMIT( "angle_end", V( a->GetArcAngleEnd() ) );
        EMIT( "ccw", V( a->IsCCW() ) );
        EMIT( "degenerated", V( a->IsDegenerated() ) );
    }

    if( t->Type() == PCB_VIA_T )
    {
        PCB_VIA* v = static_cast<PCB_VIA*>( t );
        EMIT( "via_type", VEnum( v->GetViaType() ) );
        EMIT( "top_layer", VLayer( v->TopLayer() ) );
        EMIT( "bottom_layer", VLayer( v->BottomLayer() ) );
        EMIT( "drill", V( v->GetDrill() ) );
        EMIT( "drill_value", V( v->GetDrillValue() ) );
        EMIT( "front_width", V( v->GetFrontWidth() ) );
        EMIT( "micro", V( v->IsMicroVia() ) );
        EMIT( "blind", V( v->IsBlindVia() ) );
        EMIT( "buried", V( v->IsBuriedVia() ) );
        EMIT( "is_free", V( v->GetIsFree() ) );
        EMIT( "remove_unconnected", V( v->GetRemoveUnconnected() ) );
        EMIT( "keep_start_end", V( v->GetKeepStartEnd() ) );
        EMIT( "front_tenting", VEnum( v->GetFrontTentingMode() ) );
        EMIT( "back_tenting", VEnum( v->GetBackTentingMode() ) );
        EMIT( "front_covering", VEnum( v->GetFrontCoveringMode() ) );
        EMIT( "back_covering", VEnum( v->GetBackCoveringMode() ) );
        EMIT( "front_plugging", VEnum( v->GetFrontPluggingMode() ) );
        EMIT( "back_plugging", VEnum( v->GetBackPluggingMode() ) );
        EMIT( "capping", VEnum( v->GetCappingMode() ) );
        EMIT( "filling", VEnum( v->GetFillingMode() ) );
        EMIT( "is_tented.F", V( v->IsTented( F_Mask ) ) );
        EMIT( "is_tented.B", V( v->IsTented( B_Mask ) ) );
        EMIT( "primary_drill_filled", VOpt( v->GetPrimaryDrillFilled() ) );
        EMIT( "primary_drill_capped", VOpt( v->GetPrimaryDrillCapped() ) );
        EMIT( "secondary_drill_size", VOpt( v->GetSecondaryDrillSize() ) );
        EMIT( "tertiary_drill_size", VOpt( v->GetTertiaryDrillSize() ) );
        EMIT( "backdrill_mode", VEnum( v->GetBackdrillMode() ) );
        EMIT( "bottom_backdrill_size", VOpt( v->GetBottomBackdrillSize() ) );
        EMIT( "bottom_backdrill_layer", VLayer( v->GetBottomBackdrillLayer() ) );
        EMIT( "top_backdrill_size", VOpt( v->GetTopBackdrillSize() ) );
        EMIT( "top_backdrill_layer", VLayer( v->GetTopBackdrillLayer() ) );
        EMIT( "front_post_machining", VOpt( v->GetFrontPostMachining() ) );
        EMIT( "back_post_machining", VOpt( v->GetBackPostMachining() ) );

        if( board )
            EMIT( "valid_layer_pair", V( v->HasValidLayerPair( board->GetCopperLayerCount() ) ) );

        DumpPadstack( "padstack.", v->Padstack() );

        for( PCB_LAYER_ID l : CopperLayers( board ) )
        {
            const std::string ln = VLayer( l );
            EMIT( "on_layer." + ln, V( v->IsOnLayer( l ) ) );

            if( !v->IsOnLayer( l ) )
                continue;

            EMIT( "width." + ln, V( v->GetWidth( l ) ) );
            EMIT( "flash." + ln, V( v->FlashLayer( l ) ) );
            DumpEffectiveShape( "shape." + ln, v, l, FLASHING::DEFAULT );
            DumpEffectiveShape( "shape_flashed." + ln, v, l, FLASHING::ALWAYS_FLASHED );
        }

        Guard( "hole_shape",
               [&]() { EMIT( "hole_shape", VShape( v->GetEffectiveHoleShape().get() ) ); } );

        DumpTransformedPolys( v, F_Cu );
    }
    else
    {
        DumpEffectiveShape( "shape", t, t->GetLayer(), FLASHING::DEFAULT );
        DumpTransformedPolys( t, t->GetLayer() );
    }
}


// ---- pad ----------------------------------------------------------------------------------

void DumpPad( BOARD_ITEM* aItem )
{
    BOARD* board = aItem->GetBoard();
    PAD*   pad = static_cast<PAD*>( aItem );

    EMIT( "number", V( pad->GetNumber() ) );
    EMIT( "pin_function", V( pad->GetPinFunction() ) );
    EMIT( "pin_type", V( pad->GetPinType() ) );
    EMIT( "attribute", VEnum( pad->GetAttribute() ) );
    EMIT( "property", VEnum( pad->GetProperty() ) );
    EMIT( "padstack_mode", VEnum( pad->GetPadstackMode() ) );
    EMIT( "position", V( pad->GetPosition() ) );
    EMIT( "library_position", V( pad->GetLibraryPosition() ) );
    EMIT( "orientation", V( pad->GetOrientation() ) );
    EMIT( "fp_relative_orientation", V( pad->GetFPRelativeOrientation() ) );

    double sx = 1.0, sy = 1.0;
    pad->GetPrimitiveLibScale( sx, sy );
    EMIT( "primitive_lib_scale", V( sx ) + "," + V( sy ) );

    EMIT( "flipped", V( pad->IsFlipped() ) );
    EMIT( "aperture", V( pad->IsAperturePad() ) );
    EMIT( "npth_no_copper", V( pad->IsNPTHWithNoCopper() ) );
    EMIT( "no_connect", V( pad->IsNoConnectPad() ) );
    EMIT( "free", V( pad->IsFreePad() ) );
    EMIT( "can_have_number", V( pad->CanHaveNumber() ) );
    EMIT( "principal_layer", VLayer( pad->GetPrincipalLayer() ) );
    EMIT( "drill_size", V( pad->GetDrillSize() ) );
    EMIT( "drill_shape", VEnum( pad->GetDrillShape() ) );
    EMIT( "primary_drill_start", VLayer( pad->GetPrimaryDrillStartLayer() ) );
    EMIT( "primary_drill_end", VLayer( pad->GetPrimaryDrillEndLayer() ) );
    EMIT( "secondary_drill_size", V( pad->GetSecondaryDrillSize() ) );
    EMIT( "tertiary_drill_size", V( pad->GetTertiaryDrillSize() ) );
    EMIT( "backdrill_mode", VEnum( pad->GetBackdrillMode() ) );
    EMIT( "front_post_machining", VOpt( pad->GetFrontPostMachining() ) );
    EMIT( "back_post_machining", VOpt( pad->GetBackPostMachining() ) );
    EMIT( "pad_to_die_length", V( pad->GetPadToDieLength() ) );
    EMIT( "pad_to_die_delay", V( pad->GetPadToDieDelay() ) );
    EMIT( "local_clearance", VOpt( pad->GetLocalClearance() ) );
    EMIT( "local_solder_mask_margin", VOpt( pad->GetLocalSolderMaskMargin() ) );
    EMIT( "local_solder_paste_margin", VOpt( pad->GetLocalSolderPasteMargin() ) );
    EMIT( "local_solder_paste_margin_ratio", VOpt( pad->GetLocalSolderPasteMarginRatio() ) );
    EMIT( "local_zone_connection", VEnum( pad->GetLocalZoneConnection() ) );
    EMIT( "zone_connection_overrides", VEnum( pad->GetZoneConnectionOverrides() ) );
    EMIT( "clearance_overrides", VOpt( pad->GetClearanceOverrides( nullptr ) ) );
    EMIT( "local_thermal_spoke_width", VOpt( pad->GetLocalThermalSpokeWidthOverride() ) );
    EMIT( "local_spoke_width_override", V( pad->GetLocalSpokeWidthOverride() ) );
    EMIT( "thermal_spoke_angle", V( pad->GetThermalSpokeAngle() ) );
    EMIT( "thermal_gap", V( pad->GetThermalGap() ) );
    EMIT( "local_thermal_gap", VOpt( pad->GetLocalThermalGapOverride() ) );
    EMIT( "unconnected_layer_mode", VEnum( pad->GetUnconnectedLayerMode() ) );
    EMIT( "custom_shape_in_zone", VEnum( pad->GetCustomShapeInZoneOpt() ) );

    for( PCB_LAYER_ID l : { F_Mask, B_Mask } )
        Guard( "mask", [&]() {
            EMIT( "solder_mask_expansion." + VLayer( l ), V( pad->GetSolderMaskExpansion( l ) ) );
        } );

    for( PCB_LAYER_ID l : { F_Paste, B_Paste } )
        Guard( "paste", [&]() {
            EMIT( "solder_paste_margin." + VLayer( l ), V( pad->GetSolderPasteMargin( l ) ) );
        } );

    DumpPadstack( "padstack.", pad->Padstack() );

    // Board-frame (scaled) per-layer values, for every unique padstack layer.
    for( PCB_LAYER_ID l : pad->Padstack().UniqueLayers() )
    {
        const std::string lp = "layer." + VLayer( l ) + ".";
        EMIT( lp + "shape", VEnum( pad->GetShape( l ) ) );
        EMIT( lp + "anchor_shape", VEnum( pad->GetAnchorPadShape( l ) ) );
        EMIT( lp + "size", V( pad->GetSize( l ) ) );
        EMIT( lp + "offset", V( pad->GetOffset( l ) ) );
        EMIT( lp + "delta", V( pad->GetDelta( l ) ) );
        EMIT( lp + "round_rect_radius_ratio", V( pad->GetRoundRectRadiusRatio( l ) ) );
        EMIT( lp + "round_rect_corner_radius", V( pad->GetRoundRectCornerRadius( l ) ) );
        EMIT( lp + "chamfer_ratio", V( pad->GetChamferRectRatio( l ) ) );
        EMIT( lp + "chamfer_positions", V( pad->GetChamferPositions( l ) ) );
    }

    for( PCB_LAYER_ID l : CopperLayers( board ) )
    {
        const std::string ln = VLayer( l );
        EMIT( "on_layer." + ln, V( pad->IsOnLayer( l ) ) );
        EMIT( "flash." + ln, V( pad->FlashLayer( l ) ) );
        EMIT( "flash_permitted." + ln, V( pad->FlashLayer( l, true ) ) );

        if( !pad->IsOnLayer( l ) )
            continue;

        Guard( "own_clearance", [&]() {
            EMIT( "own_clearance." + ln, V( pad->GetOwnClearance( l ) ) );
        } );
        DumpEffectiveShape( "shape." + ln, pad, l, FLASHING::DEFAULT );
        DumpEffectiveShape( "shape_flashed." + ln, pad, l, FLASHING::ALWAYS_FLASHED );

        Guard( "effective_polygon", [&]() {
            EMIT( "effective_polygon." + ln,
                        VPolySet( *pad->GetEffectivePolygon( l, ERROR_INSIDE ) ) );
            EMIT( "effective_polygon_out." + ln,
                        VPolySet( *pad->GetEffectivePolygon( l, ERROR_OUTSIDE ) ) );
        } );
    }

    // Non-copper layers the pad is on (mask, paste, ...).
    for( PCB_LAYER_ID l : pad->GetLayerSet().Seq() )
    {
        if( IsCopperLayer( l ) )
            continue;

        DumpEffectiveShape( "shape." + VLayer( l ), pad, l, FLASHING::DEFAULT );
    }

    Guard( "hole_shape", [&]() {
        EMIT( "hole_shape", VShape( pad->GetEffectiveHoleShape().get() ) );
    } );

    for( const POLY_COMBO& c : PolyCombos() )
    {
        Guard( "hole_poly", [&]() {
            SHAPE_POLY_SET buf;
            bool ok = pad->TransformHoleToPolygon( buf, c.clearance, c.maxError, c.errorLoc );
            EMIT( "hole_poly." + c.name, ok ? VPolySet( buf ) : std::string( "none" ) );
        } );
    }

    PCB_LAYER_ID tl = pad->IsOnLayer( F_Cu ) ? F_Cu : pad->GetPrincipalLayer();
    DumpTransformedPolys( pad, tl );
}


// ---- shape --------------------------------------------------------------------------------

void DumpEdaShape( const std::string& p, EDA_SHAPE* s )
{
    SHAPE_T st = s->GetShape();
    EMIT( p + "shape", VEnum( st ) );
    EMIT( p + "start", V( s->GetStart() ) );
    EMIT( p + "end", V( s->GetEnd() ) );
    EMIT( p + "width", V( s->GetWidth() ) );
    EMIT( p + "effective_width", V( s->GetEffectiveWidth() ) );
    EMIT( p + "line_style", VEnum( s->GetLineStyle() ) );
    EMIT( p + "fill", VEnum( s->GetFillMode() ) );
    EMIT( p + "closed", V( s->IsClosed() ) );
    EMIT( p + "hatch_line_width", V( s->GetHatchLineWidth() ) );
    EMIT( p + "hatch_line_spacing", V( s->GetHatchLineSpacing() ) );

    switch( st )
    {
    case SHAPE_T::ARC:
    {
        EMIT( p + "center", V( s->getCenter() ) );
        EMIT( p + "mid", V( s->GetArcMid() ) );
        EMIT( p + "radius", V( s->GetRadius() ) );
        EMIT( p + "arc_angle", V( s->GetArcAngle() ) );
        EDA_ANGLE a0, a1;
        s->CalcArcAngles( a0, a1 );
        EMIT( p + "arc_angles", V( a0 ) + "," + V( a1 ) );
        EMIT( p + "clockwise", V( s->IsClockwiseArc() ) );
        EMIT( p + "length", V( s->GetLength() ) );
        break;
    }
    case SHAPE_T::CIRCLE:
        EMIT( p + "center", V( s->getCenter() ) );
        EMIT( p + "radius", V( s->GetRadius() ) );
        break;
    case SHAPE_T::RECTANGLE:
        EMIT( p + "corner_radius", V( s->GetCornerRadius() ) );
        EMIT( p + "rect_corners", VPoints( s->GetRectCorners() ) );
        EMIT( p + "rect_size", V( s->GetRectangleWidth() ) + "," + V( s->GetRectangleHeight() ) );
        break;
    case SHAPE_T::SEGMENT:
        EMIT( p + "length", V( s->GetLength() ) );
        EMIT( p + "segment_angle", V( s->GetSegmentAngle() ) );
        break;
    case SHAPE_T::BEZIER:
        EMIT( p + "bezier_c1", V( s->GetBezierC1() ) );
        EMIT( p + "bezier_c2", V( s->GetBezierC2() ) );
        EMIT( p + "bezier_points", VPoints( s->GetBezierPoints() ) );
        break;
    case SHAPE_T::POLY:
        EMIT( p + "poly", VPolySet( s->GetPolyShape() ) );
        EMIT( p + "point_count", V( s->GetPointCount() ) );
        break;
    case SHAPE_T::ELLIPSE:
    case SHAPE_T::ELLIPSE_ARC:
        EMIT( p + "ellipse_center", V( s->GetEllipseCenter() ) );
        EMIT( p + "ellipse_major", V( s->GetEllipseMajorRadius() ) );
        EMIT( p + "ellipse_minor", V( s->GetEllipseMinorRadius() ) );
        EMIT( p + "ellipse_rotation", V( s->GetEllipseRotation() ) );
        EMIT( p + "ellipse_start", V( s->GetEllipseStartAngle() ) );
        EMIT( p + "ellipse_end", V( s->GetEllipseEndAngle() ) );
        break;
    default:
        break;
    }

    if( s->IsHatchedFill() )
    {
        Guard( "hatching", [&]() {
            EMIT( p + "hatching", VPolySet( s->GetHatching() ) );
        } );
    }
}


void DumpShape( BOARD_ITEM* aItem )
{
    PCB_SHAPE* s = static_cast<PCB_SHAPE*>( aItem );

    DumpEdaShape( "", s );
    EMIT( "has_solder_mask", V( s->HasSolderMask() ) );
    EMIT( "local_solder_mask_margin", VOpt( s->GetLocalSolderMaskMargin() ) );
    EMIT( "solder_mask_expansion", V( s->GetSolderMaskExpansion() ) );
    EMIT( "proxy", V( s->IsProxyItem() ) );
    EMIT( "lib.shape", VEnum( s->GetLibraryShape() ) );
    EMIT( "lib.start", V( s->GetLibraryStart() ) );
    EMIT( "lib.end", V( s->GetLibraryEnd() ) );
    EMIT( "lib.corner_radius", V( s->GetLibraryCornerRadius() ) );

    if( s->GetShape() == SHAPE_T::ARC )
        EMIT( "lib.arc_mid", V( s->GetLibraryArcMid() ) );

    if( s->GetShape() == SHAPE_T::BEZIER )
        EMIT( "lib.bezier", VPoints( { s->GetLibraryBezierC1(), s->GetLibraryBezierC2() } ) );

    if( s->GetShape() == SHAPE_T::POLY )
        EMIT( "lib.poly", VPolySet( s->GetLibraryPolyShape() ) );

    if( s->GetShape() == SHAPE_T::ELLIPSE || s->GetShape() == SHAPE_T::ELLIPSE_ARC )
    {
        EMIT( "lib.ellipse", V( s->GetLibraryEllipseCenter() ) + " "
                                           + V( s->GetLibraryEllipseMajorRadius() ) + " "
                                           + V( s->GetLibraryEllipseMinorRadius() ) + " "
                                           + V( s->GetLibraryEllipseRotation() ) + " "
                                           + V( s->GetLibraryEllipseStartAngle() ) + " "
                                           + V( s->GetLibraryEllipseEndAngle() ) );
    }

    if( s->GetShape() == SHAPE_T::RECTANGLE
        || ( s->GetShape() == SHAPE_T::POLY && s->GetPolyShape().TotalVertices() > 0 ) )
        EMIT( "corners", VPoints( s->GetCorners() ) );

    for( PCB_LAYER_ID l : s->GetLayerSet().Seq() )
    {
        DumpEffectiveShape( "shape." + VLayer( l ), s, l, FLASHING::DEFAULT );
        DumpTransformedPolys( s, l );
    }
}


// ---- text ---------------------------------------------------------------------------------

void DumpText( BOARD_ITEM* aItem )
{
    EDA_TEXT* t = dynamic_cast<EDA_TEXT*>( aItem );

    if( !t )
        return;

    EMIT( "text", V( t->GetText() ) );
    EMIT( "shown_text", V( t->GetShownText( RESOLVED ) ) );
    EMIT( "text_pos", V( t->GetTextPos() ) );
    EMIT( "text_size", V( t->GetTextSize() ) );
    EMIT( "text_thickness", V( t->GetTextThickness() ) );
    EMIT( "auto_thickness", V( t->GetAutoThickness() ) );
    EMIT( "effective_pen_width", V( t->GetEffectiveTextPenWidth() ) );
    EMIT( "text_angle", V( t->GetTextAngle() ) );
    EMIT( "italic", V( t->IsItalic() ) );
    EMIT( "bold", V( t->IsBold() ) );
    EMIT( "visible", V( t->IsVisible() ) );
    EMIT( "mirrored", V( t->IsMirrored() ) );
    EMIT( "multiline", V( t->IsMultilineAllowed() ) );
    EMIT( "h_justify", VEnum( t->GetHorizJustify() ) );
    EMIT( "v_justify", VEnum( t->GetVertJustify() ) );
    EMIT( "keep_upright", V( t->IsKeepUpright() ) );
    EMIT( "line_spacing", V( t->GetLineSpacing() ) );
    EMIT( "font", V( t->GetFontName() ) );
    EMIT( "default_formatting", V( t->IsDefaultFormatting() ) );
    EMIT( "hyperlink", V( t->GetHyperlink() ) );
    EMIT( "draw_pos", V( t->GetDrawPos() ) );
    EMIT( "draw_rotation", V( t->GetDrawRotation() ) );

    Guard( "text_box", [&]() { EMIT( "text_box", V( t->GetTextBox( nullptr ) ) ); } );
    Guard( "interline", [&]() { EMIT( "interline", V( t->GetInterline( nullptr ) ) ); } );

    if( aItem->Type() == PCB_FIELD_T )
    {
        PCB_FIELD* f = static_cast<PCB_FIELD*>( aItem );
        EMIT( "field.id", VEnum( f->GetId() ) );
        EMIT( "field.name", V( f->GetName() ) );
        EMIT( "field.default_name", V( f->GetName( true ) ) );
        EMIT( "field.raw_name", V( f->GetName( false ) ) );
        EMIT( "field.ordinal", V( f->GetOrdinal() ) );
        EMIT( "field.mandatory", V( f->IsMandatory() ) );
    }

    if( aItem->Type() == PCB_TEXTBOX_T || aItem->Type() == PCB_TABLECELL_T )
    {
        PCB_TEXTBOX* tb = static_cast<PCB_TEXTBOX*>( aItem );
        EMIT( "textbox.margins", V( tb->GetMarginLeft() ) + "," + V( tb->GetMarginTop() ) + ","
                                               + V( tb->GetMarginRight() ) + ","
                                               + V( tb->GetMarginBottom() ) );
        EMIT( "textbox.border_enabled", V( tb->IsBorderEnabled() ) );
        EMIT( "textbox.border_width", V( tb->GetBorderWidth() ) );
        EMIT( "textbox.corners", VPoints( tb->GetCorners() ) );
    }

    if( aItem->Type() == PCB_TABLECELL_T )
    {
        PCB_TABLECELL* c = static_cast<PCB_TABLECELL*>( aItem );
        EMIT( "cell.row_col", V( c->GetRow() ) + "," + V( c->GetColumn() ) );
        EMIT( "cell.span", V( c->GetRowSpan() ) + "," + V( c->GetColSpan() ) );
    }

    Guard( "text_shape", [&]() {
        std::shared_ptr<SHAPE_COMPOUND> shape = t->GetEffectiveTextShape( false );
        EMIT( "text_shape", VShape( shape.get() ) );
    } );

    Guard( "text_polys", [&]() {
        SHAPE_POLY_SET buf;

        if( aItem->Type() == PCB_TEXTBOX_T || aItem->Type() == PCB_TABLECELL_T )
            static_cast<PCB_TEXTBOX*>( aItem )->TransformTextToPolySet( buf, 0, 5000, ERROR_INSIDE );
        else
            static_cast<PCB_TEXT*>( aItem )->TransformTextToPolySet( buf, 0, 5000, ERROR_INSIDE );

        EMIT( "text_polys.c0_e5000_in", VPolySet( buf ) );
    } );

    if( aItem->IsKnockout() )
        DumpTransformedPolys( aItem, aItem->GetLayer() );

    DumpEffectiveShape( "effective_shape", aItem, aItem->GetLayer(), FLASHING::DEFAULT );
}


// ---- dimension ----------------------------------------------------------------------------

void DumpDimension( BOARD_ITEM* aItem )
{
    PCB_DIMENSION_BASE* d = static_cast<PCB_DIMENSION_BASE*>( aItem );

    EMIT( "start", V( d->GetStart() ) );
    EMIT( "end", V( d->GetEnd() ) );
    EMIT( "lib_start", V( d->GetLibraryStart() ) );
    EMIT( "lib_end", V( d->GetLibraryEnd() ) );
    EMIT( "override_text_enabled", V( d->GetOverrideTextEnabled() ) );
    EMIT( "override_text", V( d->GetOverrideText() ) );
    EMIT( "value_mode", VEnum( d->GetValueMode() ) );
    EMIT( "measured_value", V( d->GetMeasuredValue() ) );
    EMIT( "value_text", V( d->GetValueText() ) );
    EMIT( "prefix", V( d->GetPrefix() ) );
    EMIT( "suffix", V( d->GetSuffix() ) );
    EMIT( "arrow_direction", VEnum( d->GetArrowDirection() ) );
    EMIT( "units", VEnum( d->GetUnits() ) );
    EMIT( "units_mode", VEnum( d->GetUnitsMode() ) );
    EMIT( "units_format", VEnum( d->GetUnitsFormat() ) );
    EMIT( "precision", VEnum( d->GetPrecision() ) );
    EMIT( "suppress_zeroes", V( d->GetSuppressZeroes() ) );
    EMIT( "keep_text_aligned", V( d->GetKeepTextAligned() ) );
    EMIT( "text_position_mode", VEnum( d->GetTextPositionMode() ) );
    EMIT( "arrow_length", V( d->GetArrowLength() ) );
    EMIT( "extension_offset", V( d->GetExtensionOffset() ) );
    EMIT( "line_thickness", V( d->GetLineThickness() ) );

    const auto& shapes = d->GetShapes();
    EMIT( "shape_count", V( (int) shapes.size() ) );

    for( size_t i = 0; i < shapes.size(); ++i )
        EMIT( "shapes." + std::to_string( i ), VShape( shapes[i].get() ) );

    switch( aItem->Type() )
    {
    case PCB_DIM_ALIGNED_T:
    case PCB_DIM_ORTHOGONAL_T:
    {
        PCB_DIM_ALIGNED* a = static_cast<PCB_DIM_ALIGNED*>( aItem );
        EMIT( "crossbar", V( a->GetCrossbarStart() ) + " " + V( a->GetCrossbarEnd() ) );
        EMIT( "height", V( a->GetHeight() ) );
        EMIT( "extension_height", V( a->GetExtensionHeight() ) );
        EMIT( "angle", V( a->GetAngle() ) );

        if( aItem->Type() == PCB_DIM_ORTHOGONAL_T )
            EMIT( "orientation", VEnum( static_cast<PCB_DIM_ORTHOGONAL*>( aItem )->GetOrientation() ) );

        break;
    }
    case PCB_DIM_RADIAL_T:
    {
        PCB_DIM_RADIAL* r = static_cast<PCB_DIM_RADIAL*>( aItem );
        EMIT( "leader_length", V( r->GetLeaderLength() ) );
        EMIT( "knee", V( r->GetKnee() ) );
        break;
    }
    case PCB_DIM_LEADER_T:
        EMIT( "text_border", VEnum( static_cast<PCB_DIM_LEADER*>( aItem )->GetTextBorder() ) );
        break;
    default:
        break;
    }

    DumpTransformedPolys( aItem, aItem->GetLayer() );
}


// ---- zone ---------------------------------------------------------------------------------

void DumpZone( BOARD_ITEM* aItem )
{
    ZONE* z = static_cast<ZONE*>( aItem );

    EMIT( "name", V( z->GetZoneName() ) );
    EMIT( "priority", V( z->GetAssignedPriority() ) );
    EMIT( "rule_area", V( z->GetIsRuleArea() ) );
    EMIT( "placement_area_enabled", V( z->GetPlacementAreaEnabled() ) );
    EMIT( "placement_area_source", V( z->GetPlacementAreaSource() ) );
    EMIT( "placement_area_source_type", VEnum( z->GetPlacementAreaSourceType() ) );
    EMIT( "keepout.zone_fills", V( z->GetDoNotAllowZoneFills() ) );
    EMIT( "keepout.vias", V( z->GetDoNotAllowVias() ) );
    EMIT( "keepout.tracks", V( z->GetDoNotAllowTracks() ) );
    EMIT( "keepout.pads", V( z->GetDoNotAllowPads() ) );
    EMIT( "keepout.footprints", V( z->GetDoNotAllowFootprints() ) );
    EMIT( "teardrop_area", V( z->IsTeardropArea() ) );
    EMIT( "teardrop_type", VEnum( z->GetTeardropAreaType() ) );
    EMIT( "fill_mode", VEnum( z->GetFillMode() ) );
    EMIT( "local_clearance", VOpt( z->GetLocalClearance() ) );
    EMIT( "min_thickness", V( z->GetMinThickness() ) );
    EMIT( "pad_connection", VEnum( z->GetPadConnection() ) );
    EMIT( "thermal_relief_gap", V( z->GetThermalReliefGap() ) );
    EMIT( "thermal_relief_spoke_width", V( z->GetThermalReliefSpokeWidth() ) );
    EMIT( "hatch_thickness", V( z->GetHatchThickness() ) );
    EMIT( "hatch_gap", V( z->GetHatchGap() ) );
    EMIT( "hatch_orientation", V( z->GetHatchOrientation() ) );
    EMIT( "hatch_smoothing_level", V( z->GetHatchSmoothingLevel() ) );
    EMIT( "hatch_smoothing_value", V( z->GetHatchSmoothingValue() ) );
    EMIT( "hatch_hole_min_area", V( z->GetHatchHoleMinArea() ) );
    EMIT( "hatch_border_algorithm", V( z->GetHatchBorderAlgorithm() ) );
    EMIT( "copper_thieving", V( z->IsCopperThieving() ) );
    EMIT( "thieving.pattern", VEnum( z->GetThievingPattern() ) );
    EMIT( "thieving.element_size", V( z->GetThievingElementSize() ) );
    EMIT( "thieving.gap", V( z->GetThievingGap() ) );
    EMIT( "thieving.line_width", V( z->GetThievingLineWidth() ) );
    EMIT( "thieving.stagger", V( z->GetThievingStagger() ) );
    EMIT( "thieving.orientation", V( z->GetThievingOrientation() ) );
    EMIT( "island_removal_mode", VEnum( z->GetIslandRemovalMode() ) );
    EMIT( "min_island_area", V( z->GetMinIslandArea() ) );
    EMIT( "corner_smoothing", VEnum( z->GetCornerSmoothingType() ) );
    EMIT( "corner_radius", V( z->GetCornerRadius() ) );
    EMIT( "border_style", VEnum( z->GetHatchStyle() ) );
    EMIT( "border_hatch_pitch", V( z->GetBorderHatchPitch() ) );
    EMIT( "is_filled", V( z->IsFilled() ) );
    EMIT( "need_refill", V( z->NeedRefill() ) );
    EMIT( "num_corners", V( z->GetNumCorners() ) );
    EMIT( "first_layer", VLayer( z->GetFirstLayer() ) );
    EMIT( "outline", VPolySet( *z->Outline() ) );

    Guard( "library_outline", [&]() {
        EMIT( "library_outline", VPolySet( z->GetLibraryOutline() ) );
    } );

    Guard( "hatch_lines", [&]() {
        std::string r;

        for( const SEG& s : z->GetHatchLines() )
            r += ( r.empty() ? "" : " " ) + V( s.A ) + ">" + V( s.B );

        EMIT( "hatch_lines", "[" + r + "]" );
    } );

    for( PCB_LAYER_ID l : z->GetLayerSet().Seq() )
    {
        const std::string ln = VLayer( l );

        if( !z->HasFilledPolysForLayer( l ) )
        {
            EMIT( "filled." + ln, "none" );
            continue;
        }

        std::shared_ptr<SHAPE_POLY_SET> fill = z->GetFilledPolysList( l );
        EMIT( "filled." + ln, VPolySet( *fill ) );

        std::string islands;

        for( int i = 0; i < fill->OutlineCount(); ++i )
            islands += z->IsIsland( l, i ) ? '1' : '0';

        EMIT( "islands." + ln, "[" + islands + "]" );
    }

    if( !z->GetIsRuleArea() && z->IsOnCopperLayer() )
        DumpEffectiveShape( "shape." + VLayer( z->GetFirstLayer() ), z, z->GetFirstLayer(),
                            FLASHING::DEFAULT );
}


// ---- footprint ----------------------------------------------------------------------------

void DumpFootprint( BOARD_ITEM* aItem )
{
    FOOTPRINT* fp = static_cast<FOOTPRINT*>( aItem );

    EMIT( "fpid", V( fp->GetFPIDAsString() ) );
    EMIT( "reference", V( fp->GetReference() ) );
    EMIT( "value", V( fp->GetValue() ) );
    EMIT( "position", V( fp->GetPosition() ) );
    EMIT( "orientation", V( fp->GetOrientation() ) );
    EMIT( "layer", VLayer( fp->GetLayer() ) );
    EMIT( "side", VLayer( fp->GetSide() ) );
    EMIT( "flipped", V( fp->IsFlipped() ) );

    const TRANSFORM_TRS& tr = fp->GetTransform();
    EMIT( "transform.translate", V( tr.GetTranslate() ) );
    EMIT( "transform.rotate", V( tr.GetRotate() ) );
    EMIT( "transform.scale", V( tr.GetScaleX() ) + "," + V( tr.GetScaleY() ) );
    EMIT( "transform.identity", V( tr.IsIdentity() ) );

    EMIT( "attributes", V( fp->GetAttributes() ) );
    EMIT( "footprint_type", VEnum( fp->GetFootprintType() ) );
    EMIT( "likely_attribute", V( fp->GetLikelyAttribute() ) );
    EMIT( "board_only", V( fp->IsBoardOnly() ) );
    EMIT( "exclude_from_pos_files", V( fp->IsExcludedFromPosFiles() ) );
    EMIT( "exclude_from_bom", V( fp->IsExcludedFromBOM() ) );
    EMIT( "exclude_from_sim", V( fp->IsExcludedFromSim() ) );
    EMIT( "dnp", V( fp->IsDNP() ) );
    EMIT( "allow_missing_courtyard", V( fp->AllowMissingCourtyard() ) );
    EMIT( "allow_solder_mask_bridges", V( fp->AllowSolderMaskBridges() ) );
    EMIT( "net_tie", V( fp->IsNetTie() ) );
    EMIT( "placed", V( fp->IsPlaced() ) );
    EMIT( "local_clearance", VOpt( fp->GetLocalClearance() ) );
    EMIT( "local_solder_mask_margin", VOpt( fp->GetLocalSolderMaskMargin() ) );
    EMIT( "local_solder_paste_margin", VOpt( fp->GetLocalSolderPasteMargin() ) );
    EMIT( "local_solder_paste_margin_ratio", VOpt( fp->GetLocalSolderPasteMarginRatio() ) );
    EMIT( "local_zone_connection", VEnum( fp->GetLocalZoneConnection() ) );
    EMIT( "stackup_mode", VEnum( fp->GetStackupMode() ) );
    EMIT( "stackup_layers", V( fp->GetStackupLayers() ) );
    EMIT( "private_layers", V( fp->GetPrivateLayers() ) );
    EMIT( "path", V( fp->GetPath().AsString() ) );
    EMIT( "sheetname", V( fp->GetSheetname() ) );
    EMIT( "sheetfile", V( fp->GetSheetfile() ) );
    EMIT( "filters", V( fp->GetFilters() ) );
    EMIT( "description", V( fp->GetLibDescription() ) );
    EMIT( "keywords", V( fp->GetKeywords() ) );
    EMIT( "duplicate_pad_numbers_are_jumpers", V( fp->GetDuplicatePadNumbersAreJumpers() ) );
    EMIT( "file_format_version_at_load", V( fp->GetFileFormatVersionAtLoad() ) );
    EMIT( "pad_count", V( fp->GetPadCount() ) );
    EMIT( "has_through_hole_pads", V( fp->HasThroughHolePads() ) );
    EMIT( "component_class", V( fp->GetComponentClassAsString() ) );

    {
        std::string r;

        for( const wxString& g : fp->GetNetTiePadGroups() )
            r += ( r.empty() ? "" : " " ) + V( g );

        EMIT( "net_tie_pad_groups", "[" + r + "]" );
    }

    {
        std::string r;

        for( const JUMPER_GROUP& jg : fp->JumperPadGroups().GetAll() )
        {
            std::string grp;

            for( const wxString& n : jg.GetNames() )
                grp += ( grp.empty() ? "" : " " ) + V( n );

            r += ( r.empty() ? "" : " " ) + std::string( "{" ) + grp + "}";
        }

        EMIT( "jumper_pad_groups", "[" + r + "]" );
    }

    int i = 0;

    for( PCB_FIELD* f : fp->GetFields() )
    {
        const std::string p = "field." + std::to_string( i++ ) + ".";
        EMIT( p + "key", ItemKey( f ) );
        EMIT( p + "name", V( f->GetName() ) );
        EMIT( p + "text", V( f->GetText() ) );
    }

    i = 0;

    for( PAD* pad : fp->Pads() )
        EMIT( "pad." + std::to_string( i++ ), ItemKey( pad ) + " " + V( pad->GetNumber() ) );

    i = 0;

    for( BOARD_ITEM* d : fp->GraphicalItems() )
        EMIT( "drawing." + std::to_string( i++ ), ItemKey( d ) );

    i = 0;

    for( ZONE* z : fp->Zones() )
        EMIT( "zone." + std::to_string( i++ ), ItemKey( z ) );

    i = 0;

    for( const FP_3DMODEL& m : fp->Models() )
    {
        const std::string p = "model." + std::to_string( i++ ) + ".";
        EMIT( p + "filename", V( m.m_Filename ) );
        EMIT( p + "offset", V( m.m_Offset.x ) + "," + V( m.m_Offset.y ) + "," + V( m.m_Offset.z ) );
        EMIT( p + "rotation",
                    V( m.m_Rotation.x ) + "," + V( m.m_Rotation.y ) + "," + V( m.m_Rotation.z ) );
        EMIT( p + "scale", V( m.m_Scale.x ) + "," + V( m.m_Scale.y ) + "," + V( m.m_Scale.z ) );
        EMIT( p + "opacity", V( m.m_Opacity ) );
        EMIT( p + "show", V( m.m_Show ) );
    }

    for( const auto& [name, var] : fp->GetVariants() )
        EMIT( "variant." + std::string( name.ToUTF8() ) + ".dnp",
                    V( fp->GetDNPForVariant( name ) ) );

    Guard( "courtyard", [&]() {
        EMIT( "courtyard.F", VPolySet( fp->GetCourtyard( F_CrtYd ) ) );
        EMIT( "courtyard.B", VPolySet( fp->GetCourtyard( B_CrtYd ) ) );
    } );

    Guard( "hull", [&]() {
        EMIT( "hull.F_Cu", VPolySet( fp->GetBoundingHull( F_Cu ) ) );
        EMIT( "hull.B_Cu", VPolySet( fp->GetBoundingHull( B_Cu ) ) );
    } );

    Guard( "area", [&]() { EMIT( "area", V( fp->GetArea() ) ); } );
}
