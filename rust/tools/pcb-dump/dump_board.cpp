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

// pcb-dump: per-board driver and the "item" / "bbox" sections.

#include "pcb_dump.h"

#include <board.h>
#include <board_connected_item.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_dimension.h>
#include <pcb_field.h>
#include <pcb_group.h>
#include <pcb_shape.h>
#include <pcb_text.h>
#include <pcb_textbox.h>
#include <pcb_tablecell.h>
#include <pcb_track.h>
#include <zone.h>
#include <netinfo.h>
#include <geometry/shape_poly_set.h>


void DumpBoard( BOARD* aBoard, const DUMP_OPTIONS& aOpts )
{
    InitItemKeys( aBoard );

    if( aOpts.Want( "board" ) )
    {
        SCOPE s( "board", "board" );
        Guard( "board", [&]() { DumpBoardSection( aBoard ); } );
    }

    ForEachBoardItem( aBoard,
                      [&]( BOARD_ITEM* aItem )
                      {
                          DumpItemSections( aBoard, aItem, aOpts );
                      } );

    if( aOpts.Want( "order" ) )
    {
        SCOPE s( "order", "board" );
        Guard( "order", [&]() { DumpOrderSection( aBoard ); } );
    }
}


static bool isTextItem( KICAD_T aType )
{
    switch( aType )
    {
    case PCB_TEXT_T:
    case PCB_FIELD_T:
    case PCB_TEXTBOX_T:
    case PCB_TABLECELL_T:
        return true;
    default:
        return false;
    }
}


static bool isShapeItem( KICAD_T aType )
{
    switch( aType )
    {
    case PCB_SHAPE_T:
    case PCB_TEXTBOX_T:
    case PCB_TABLECELL_T:
        return true;
    default:
        return false;
    }
}


static bool isDimension( KICAD_T aType )
{
    switch( aType )
    {
    case PCB_DIM_ALIGNED_T:
    case PCB_DIM_LEADER_T:
    case PCB_DIM_CENTER_T:
    case PCB_DIM_RADIAL_T:
    case PCB_DIM_ORTHOGONAL_T:
        return true;
    default:
        return false;
    }
}


void DumpProps( BOARD_ITEM* aItem ); // dump_props.cpp


void DumpItemSections( BOARD* aBoard, BOARD_ITEM* aItem, const DUMP_OPTIONS& aOpts )
{
    const std::string key = ItemKey( aItem );
    const KICAD_T     type = aItem->Type();

    auto section = [&]( const char* aName, void ( *aFn )( BOARD_ITEM* ) )
    {
        if( !aOpts.Want( aName ) )
            return;

        SCOPE s( aName, key );
        Guard( aName, [&]() { aFn( aItem ); } );
    };

    section( "item", DumpItemBase );
    section( "bbox", DumpBBox );

    if( type == PCB_TRACE_T || type == PCB_ARC_T || type == PCB_VIA_T )
        section( "track", DumpTrack );

    if( type == PCB_PAD_T )
        section( "pad", DumpPad );

    if( isShapeItem( type ) )
        section( "shape", DumpShape );

    if( isTextItem( type ) )
        section( "text", DumpText );

    if( isDimension( type ) )
    {
        section( "dim", DumpDimension );
        section( "text", DumpText );
    }

    if( type == PCB_ZONE_T )
        section( "zone", DumpZone );

    if( type == PCB_FOOTPRINT_T )
        section( "footprint", DumpFootprint );

    section( "misc", DumpMisc );

    // Opt-in (not part of "all sections"): the property values (dump_props.cpp).
    if( aOpts.sections.count( "props" ) )
        section( "props", DumpProps );
}


void DumpItemBase( BOARD_ITEM* aItem )
{
    EMIT( "type", VEnum( aItem->Type() ) );
    EMIT( "uuid", UuidInFile( aItem->m_Uuid ) ? V( aItem->m_Uuid ) : std::string( "generated" ) );
    EMIT( "uuid_in_file", V( UuidInFile( aItem->m_Uuid ) ) );

    BOARD_ITEM_CONTAINER* parent = aItem->GetParent();

    if( !parent )
        EMIT( "parent", "null" );
    else if( parent->Type() == PCB_T )
        EMIT( "parent", "board" );
    else
        EMIT( "parent", ItemKey( parent ) );

    EDA_GROUP* group = aItem->GetParentGroup();
    EMIT( "group", group ? ItemKey( static_cast<PCB_GROUP*>( group->AsEdaItem() ) )
                               : std::string( "null" ) );

    // PCB_GROUP::GetLayer() asserts ("isn't well-defined"); groups have no layer.
    if( aItem->Type() != PCB_GROUP_T )
        EMIT( "layer", VLayer( aItem->GetLayer() ) );
    LSET ls = aItem->GetLayerSet();
    EMIT( "layerset", V( ls ) );
    EMIT( "layers", VLayerNames( ls ) );
    EMIT( "position", V( aItem->GetPosition() ) );
    EMIT( "fp_relative_position", V( aItem->GetFPRelativePosition() ) );
    EMIT( "locked", V( aItem->IsLocked() ) );
    EMIT( "locked_flag", V( aItem->BOARD_ITEM::IsLocked() ) );
    EMIT( "on_copper", V( aItem->IsOnCopperLayer() ) );
    EMIT( "has_hole", V( aItem->HasHole() ) );
    EMIT( "has_drilled_hole", V( aItem->HasDrilledHole() ) );
    EMIT( "knockout", V( aItem->IsKnockout() ) );
    EMIT( "side_specific", V( aItem->IsSideSpecific() ) );
    EMIT( "layer_agnostic", V( aItem->IsLayerAgnostic() ) );
    EMIT( "connected_type", V( aItem->IsConnected() ) );

    if( aItem->IsConnected() )
    {
        BOARD_CONNECTED_ITEM* bci = static_cast<BOARD_CONNECTED_ITEM*>( aItem );
        EMIT( "net", V( bci->GetNetCode() ) );
        EMIT( "netname", V( bci->GetNetname() ) );
        EMIT( "local_ratsnest_visible", V( bci->GetLocalRatsnestVisible() ) );
        EMIT( "local_clearance", VOpt( bci->GetLocalClearance() ) );
    }

    for( const auto& [k, v] : aItem->GetCustomProperties() )
        EMIT( "custom_property." + std::string( k.ToUTF8() ), V( v ) );
}


void DumpBBox( BOARD_ITEM* aItem )
{
    EMIT( "bbox", V( aItem->GetBoundingBox() ) );

    if( aItem->Type() == PCB_FOOTPRINT_T )
    {
        FOOTPRINT* fp = static_cast<FOOTPRINT*>( aItem );
        EMIT( "bbox_with_text", V( fp->GetBoundingBox( true ) ) );
        EMIT( "bbox_without_text", V( fp->GetBoundingBox( false ) ) );
        EMIT( "fp_pads_local_bbox", V( fp->GetFpPadsLocalBbox() ) );
        EMIT( "bounding_hull", VPolySet( fp->GetBoundingHull() ) );
        EMIT( "layer_bbox.F_Cu", V( fp->GetLayerBoundingBox( LSET( { F_Cu } ) ) ) );
        EMIT( "layer_bbox.B_Cu", V( fp->GetLayerBoundingBox( LSET( { B_Cu } ) ) ) );
    }
    else if( aItem->Type() == PCB_PAD_T )
    {
        PAD* pad = static_cast<PAD*>( aItem );

        for( PCB_LAYER_ID l : CopperLayers( aItem->GetBoard() ) )
        {
            if( pad->IsOnLayer( l ) )
                EMIT( "bbox." + VLayer( l ), V( pad->GetBoundingBox( l ) ) );
        }

        EMIT( "bounding_radius", V( pad->GetBoundingRadius() ) );
    }
    else if( aItem->Type() == PCB_VIA_T )
    {
        PCB_VIA* via = static_cast<PCB_VIA*>( aItem );

        for( PCB_LAYER_ID l : CopperLayers( aItem->GetBoard() ) )
        {
            if( via->IsOnLayer( l ) )
                EMIT( "bbox." + VLayer( l ), V( via->GetBoundingBox( l ) ) );
        }
    }
}
