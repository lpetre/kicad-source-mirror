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

// pcb-dump: the "misc" item section and the "defaults" command.

#include "pcb_dump.h"
#include "dump_common.h"

#include <algorithm>

#include <board.h>
#include <board_design_settings.h>
#include <board_stackup_manager/board_stackup.h>
#include <footprint.h>
#include <netclass.h>
#include <netinfo.h>
#include <pad.h>
#include <pcb_barcode.h>
#include <pcb_dimension.h>
#include <pcb_field.h>
#include <pcb_group.h>
#include <pcb_marker.h>
#include <pcb_plot_params.h>
#include <pcb_point.h>
#include <pcb_reference_image.h>
#include <reference_image.h>
#include <pcb_shape.h>
#include <pcb_table.h>
#include <pcb_tablecell.h>
#include <pcb_target.h>
#include <pcb_text.h>
#include <pcb_textbox.h>
#include <pcb_track.h>
#include <generators/pcb_tuning_pattern.h>
#include <teardrop/teardrop_parameters.h>
#include <zone.h>
#include <zone_settings.h>
#include <drc/drc_item.h>


// ---- misc section -------------------------------------------------------------------------

void DumpMisc( BOARD_ITEM* aItem )
{
    switch( aItem->Type() )
    {
    case PCB_TARGET_T:
    {
        PCB_TARGET* t = static_cast<PCB_TARGET*>( aItem );
        EMIT( "target.shape", V( t->GetShape() ) );
        EMIT( "target.size", V( t->GetSize() ) );
        EMIT( "target.width", V( t->GetWidth() ) );
        break;
    }
    case PCB_POINT_T:
    {
        PCB_POINT* p = static_cast<PCB_POINT*>( aItem );
        EMIT( "point.size", V( p->GetSize() ) );
        EMIT( "point.library_position", V( p->GetLibraryPosition() ) );
        break;
    }
    case PCB_BARCODE_T:
    {
        PCB_BARCODE* b = static_cast<PCB_BARCODE*>( aItem );
        EMIT( "barcode.text", V( b->GetText() ) );
        EMIT( "barcode.kind", VEnum( b->GetKind() ) );
        EMIT( "barcode.ecc", VEnum( b->GetErrorCorrection() ) );
        EMIT( "barcode.show_text", V( b->GetShowText() ) );
        EMIT( "barcode.size", V( b->GetWidth() ) + "," + V( b->GetHeight() ) );
        EMIT( "barcode.text_size", V( b->GetTextSize() ) );
        EMIT( "barcode.margin", V( b->GetMargin() ) );
        EMIT( "barcode.angle", V( b->GetAngle() ) );
        EMIT( "barcode.library_pos", V( b->GetLibraryPos() ) );
        EMIT( "barcode.library_angle", V( b->GetLibraryAngle() ) );
        EMIT( "barcode.poly", VPolySet( b->GetPolyShape() ) );
        EMIT( "barcode.symbol_poly", VPolySet( b->GetSymbolPoly() ) );
        EMIT( "barcode.text_poly", VPolySet( b->GetTextPoly() ) );
        EMIT( "barcode.last_error", V( b->GetLastError() ) );
        break;
    }
    case PCB_REFERENCE_IMAGE_T:
    {
        const REFERENCE_IMAGE& r = static_cast<PCB_REFERENCE_IMAGE*>( aItem )->GetReferenceImage();
        EMIT( "image.position", V( r.GetPosition() ) );
        EMIT( "image.size", V( r.GetSize() ) );
        EMIT( "image.transform_origin_offset", V( r.GetTransformOriginOffset() ) );
        EMIT( "image.orientation", V( r.GetOrientation() ) );
        EMIT( "image.opacity", V( r.GetOpacity() ) );
        EMIT( "image.scale", V( r.GetImageScale() ) );
        EMIT( "image.bbox", V( r.GetBoundingBox() ) );
        break;
    }
    case PCB_GROUP_T:
    case PCB_GENERATOR_T:
    {
        PCB_GROUP* g = static_cast<PCB_GROUP*>( aItem );
        EMIT( "group.name", V( g->GetName() ) );

        std::vector<std::string> keys;

        for( BOARD_ITEM* m : g->GetBoardItems() )
            keys.push_back( ItemKey( m ) );

        std::sort( keys.begin(), keys.end() );
        std::string r;

        for( const std::string& k : keys )
            r += ( r.empty() ? "" : " " ) + k;

        EMIT( "group.members", "[" + r + "]" );

        if( aItem->Type() == PCB_GENERATOR_T )
        {
            PCB_GENERATOR* gen = static_cast<PCB_GENERATOR*>( aItem );
            EMIT( "generator.type", V( gen->GetGeneratorType() ) );
        }

        break;
    }
    case PCB_TABLE_T:
    {
        PCB_TABLE* t = static_cast<PCB_TABLE*>( aItem );
        EMIT( "table.cols", V( t->GetColCount() ) );
        EMIT( "table.rows", V( t->GetRowCount() ) );
        EMIT( "table.end", V( t->GetEnd() ) );
        EMIT( "table.border_width", V( t->GetBorderWidth() ) );
        EMIT( "table.border_style", VEnum( t->GetBorderStyle() ) );
        EMIT( "table.separators_width", V( t->GetSeparatorsWidth() ) );
        EMIT( "table.separators_style", VEnum( t->GetSeparatorsStyle() ) );
        EMIT( "table.stroke_external", V( t->StrokeExternal() ) );
        EMIT( "table.stroke_header", V( t->StrokeHeaderSeparator() ) );
        EMIT( "table.stroke_columns", V( t->StrokeColumns() ) );
        EMIT( "table.stroke_rows", V( t->StrokeRows() ) );

        for( int c = 0; c < t->GetColCount(); ++c )
            EMIT( "table.col_width." + std::to_string( c ), V( t->GetColWidth( c ) ) );

        for( int r = 0; r < t->GetRowCount(); ++r )
            EMIT( "table.row_height." + std::to_string( r ), V( t->GetRowHeight( r ) ) );

        break;
    }
    case PCB_MARKER_T:
    {
        PCB_MARKER* m = static_cast<PCB_MARKER*>( aItem );
        EMIT( "marker.severity", VEnum( m->GetSeverity() ) );
        EMIT( "marker.excluded", V( m->IsExcluded() ) );
        EMIT( "marker.error_code", V( m->GetRCItem()->GetErrorCode() ) );
        EMIT( "marker.settings_key", V( m->GetRCItem()->GetSettingsKey() ) );
        break;
    }
    default:
        break;
    }
}


// ---- defaults -----------------------------------------------------------------------------

/// Dump every applicable item section of a default-constructed item under key aClass.
static void dumpDefaultItem( const std::string& aClass, BOARD_ITEM* aItem )
{
    DUMP_OPTIONS all;
    SCOPE        s( "defaults", aClass );

    auto run = [&]( const char* aSection, void ( *aFn )( BOARD_ITEM* ) )
    {
        g_out.m_fieldPrefix = std::string( aSection ) + ".";
        Guard( aSection, [&]() { aFn( aItem ); } );
        g_out.m_fieldPrefix.clear();
    };

    KICAD_T t = aItem->Type();
    run( "item", DumpItemBase );
    run( "bbox", DumpBBox );

    if( t == PCB_TRACE_T || t == PCB_ARC_T || t == PCB_VIA_T )
        run( "track", DumpTrack );

    if( t == PCB_PAD_T )
        run( "pad", DumpPad );

    if( t == PCB_SHAPE_T || t == PCB_TEXTBOX_T || t == PCB_TABLECELL_T )
        run( "shape", DumpShape );

    if( t == PCB_TEXT_T || t == PCB_FIELD_T || t == PCB_TEXTBOX_T || t == PCB_TABLECELL_T
        || BaseType( t ) == PCB_DIMENSION_T )
    {
        run( "text", DumpText );
    }

    if( BaseType( t ) == PCB_DIMENSION_T )
        run( "dim", DumpDimension );

    if( t == PCB_ZONE_T )
        run( "zone", DumpZone );

    if( t == PCB_FOOTPRINT_T )
        run( "footprint", DumpFootprint );

    run( "misc", DumpMisc );
}


void DumpDefaults()
{
    // Items are default-constructed with a default BOARD as parent (not added to it), so that
    // GetBoard() resolves and board-level defaults apply, as for a freshly created item.
    BOARD     board;
    FOOTPRINT fp( &board );

    {
        PCB_TRACK track( &board );
        dumpDefaultItem( "PCB_TRACK", &track );
        PCB_ARC arc( &board );
        dumpDefaultItem( "PCB_ARC", &arc );
        PCB_VIA via( &board );
        dumpDefaultItem( "PCB_VIA", &via );
    }

    {
        PAD pad( &fp );
        dumpDefaultItem( "PAD", &pad );
    }

    {
        FOOTPRINT f( &board );
        dumpDefaultItem( "FOOTPRINT", &f );
    }

    {
        ZONE zone( &board );
        dumpDefaultItem( "ZONE", &zone );
    }

    for( SHAPE_T st : magic_enum::enum_values<SHAPE_T>() )
    {
        PCB_SHAPE shape( &board, st );
        dumpDefaultItem( "PCB_SHAPE." + VEnum( st ), &shape );
    }

    {
        PCB_TEXT text( &board );
        dumpDefaultItem( "PCB_TEXT", &text );
        PCB_TEXT fptext( &fp );
        dumpDefaultItem( "PCB_TEXT.in_footprint", &fptext );
    }

    for( FIELD_T id : magic_enum::enum_values<FIELD_T>() )
    {
        PCB_FIELD field( &fp, id );
        dumpDefaultItem( "PCB_FIELD." + VEnum( id ), &field );
    }

    {
        PCB_TEXTBOX tb( &board );
        dumpDefaultItem( "PCB_TEXTBOX", &tb );
    }

    {
        PCB_TABLE table( &board, pcbIUScale.mmToIU( 0.1 ) );
        dumpDefaultItem( "PCB_TABLE", &table );
        PCB_TABLECELL cell( &table );
        dumpDefaultItem( "PCB_TABLECELL", &cell );
    }

    {
        PCB_DIM_ALIGNED d1( &board );
        dumpDefaultItem( "PCB_DIM_ALIGNED", &d1 );
        PCB_DIM_ORTHOGONAL d2( &board );
        dumpDefaultItem( "PCB_DIM_ORTHOGONAL", &d2 );
        PCB_DIM_RADIAL d3( &board );
        dumpDefaultItem( "PCB_DIM_RADIAL", &d3 );
        PCB_DIM_LEADER d4( &board );
        dumpDefaultItem( "PCB_DIM_LEADER", &d4 );
        PCB_DIM_CENTER d5( &board );
        dumpDefaultItem( "PCB_DIM_CENTER", &d5 );
    }

    {
        PCB_GROUP group( &board );
        dumpDefaultItem( "PCB_GROUP", &group );
        PCB_TUNING_PATTERN gen( &board );
        dumpDefaultItem( "PCB_GENERATOR.PCB_TUNING_PATTERN", &gen );
    }

    {
        PCB_TARGET target( &board );
        dumpDefaultItem( "PCB_TARGET", &target );
        PCB_POINT point( &board );
        dumpDefaultItem( "PCB_POINT", &point );
        PCB_BARCODE barcode( &board );
        dumpDefaultItem( "PCB_BARCODE", &barcode );
        PCB_REFERENCE_IMAGE image( &board );
        dumpDefaultItem( "PCB_REFERENCE_IMAGE", &image );
        PCB_MARKER marker( DRC_ITEM::Create( DRCE_CLEARANCE ), VECTOR2I( 0, 0 ) );
        dumpDefaultItem( "PCB_MARKER", &marker );
    }

    {
        SCOPE        s( "defaults", "NETINFO_ITEM" );
        NETINFO_ITEM net( &board );
        EMIT( "netcode", V( net.GetNetCode() ) );
        EMIT( "netname", V( net.GetNetname() ) );
        EMIT( "short_netname", V( net.GetShortNetname() ) );
        EMIT( "netclass", net.GetNetClass() ? V( net.GetNetClass()->GetName() ) : std::string( "null" ) );
        EMIT( "net_chain", V( net.GetNetChain() ) );
    }

    {
        SCOPE    s( "defaults", "NETCLASS" );
        NETCLASS nc( NETCLASS::Default );
        DumpNetclass( "", nc );
    }

    {
        SCOPE                 s( "defaults", "BOARD_DESIGN_SETTINGS" );
        BOARD_DESIGN_SETTINGS bds( nullptr, "board.design_settings" );
        DumpDesignSettings( "", bds );
    }

    {
        SCOPE         s( "defaults", "BOARD_STACKUP" );
        BOARD_STACKUP stackup;
        DumpStackup( "", stackup );
    }

    {
        SCOPE         s( "defaults", "BOARD_STACKUP.default_list" );
        BOARD_STACKUP stackup;
        stackup.BuildDefaultStackupList( &board.GetDesignSettings(), 2 );
        DumpStackup( "", stackup );
    }

    {
        SCOPE           s( "defaults", "PCB_PLOT_PARAMS" );
        PCB_PLOT_PARAMS pp;
        DumpPlotParams( "", pp );
    }

    {
        SCOPE               s( "defaults", "TEARDROP_PARAMETERS" );
        TEARDROP_PARAMETERS tp;
        DumpTeardropParams( "", tp );
    }

    {
        SCOPE         s( "defaults", "ZONE_SETTINGS" );
        ZONE_SETTINGS zs;
        DumpZoneSettings( "", zs );
    }

    {
        SCOPE s( "defaults", "BOARD" );
        InitItemKeys( &board );
        DumpBoardSection( &board );
    }
}
