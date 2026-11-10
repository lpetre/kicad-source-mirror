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

// Package M11 (PCB_TEXTBOX / PCB_TABLE / PCB_TABLECELL) differential oracle. Builds text boxes
// and tables in code (board level, and inside a footprint placed at (10 mm, 20 mm), rotated
// 30 degrees, scaled (1.5, 0.75)), applies one mutator per case (Rotate, Flip, Mirror,
// SetTextAngle, Move, Normalize, Autosize) and prints the resulting geometry and the derived
// values (corners, draw position, bounding box, shown text with line breaks, effective shape,
// text and box polygons, borders). Polygons and shapes are printed as the FNV-1a hash of their
// pcb-dump text. Built and run by m11_golden.sh; the Rust replay is
// pcb_table::oracle_tests::m11_matches_oracle (feature `dump`).

#include "pcb_dump.h"

#include <cstdio>
#include <cstdlib>
#include <functional>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/wx_utils/wx_assert.h>

#include <board.h>
#include <footprint.h>
#include <pcb_table.h>
#include <pcb_tablecell.h>
#include <pcb_textbox.h>
#include <geometry/shape_compound.h>


static const int MM = 1000000;


static std::string H( const std::string& aText )
{
    char buf[32];
    snprintf( buf, sizeof( buf ), "#%016llx", (unsigned long long) Fnv1a64( aText ) );
    return buf;
}


static void P( const std::string& aCase, const std::string& aKey, const std::string& aField,
               const std::string& aValue )
{
    printf( "%s|%s|%s=%s\n", aCase.c_str(), aKey.c_str(), aField.c_str(), aValue.c_str() );
}


static void DumpBox( const std::string& c, const std::string& k, PCB_TEXTBOX* tb )
{
    PCB_LAYER_ID layer = tb->GetLayer();

    P( c, k, "shape", V( (int) tb->GetShape() ) );
    P( c, k, "start", V( tb->GetStart() ) );
    P( c, k, "end", V( tb->GetEnd() ) );
    P( c, k, "lib_shape", V( (int) tb->GetLibraryShape() ) );
    P( c, k, "lib_start", V( tb->GetLibraryStart() ) );
    P( c, k, "lib_end", V( tb->GetLibraryEnd() ) );

    if( tb->GetShape() == SHAPE_T::POLY )
        P( c, k, "poly", H( VPolySet( tb->GetPolyShape() ) ) );

    P( c, k, "layer", V( (int) layer ) );
    P( c, k, "mirrored", V( tb->IsMirrored() ) );
    P( c, k, "text_angle", V( tb->GetTextAngle() ) );
    P( c, k, "lib_text_angle", V( tb->GetLibTextAngle() ) );
    P( c, k, "attr_angle", V( tb->GetAttributes().m_Angle.GetAngle() ) );
    P( c, k, "text_size", V( tb->GetTextSize() ) );
    P( c, k, "text_thickness", V( tb->GetTextThickness() ) );
    P( c, k, "draw_pos", V( tb->GetDrawPos() ) );
    P( c, k, "draw_pos_flipped", V( tb->GetDrawPos( true ) ) );
    P( c, k, "draw_rotation", V( tb->GetDrawRotation() ) );
    P( c, k, "corners", VPoints( tb->GetCorners() ) );
    P( c, k, "top_left", V( tb->GetTopLeft() ) );
    P( c, k, "bot_right", V( tb->GetBotRight() ) );
    P( c, k, "bbox", V( tb->GetBoundingBox() ) );
    P( c, k, "center", V( tb->GetCenter() ) );
    P( c, k, "position", V( tb->GetPosition() ) );
    P( c, k, "legacy_margin", V( tb->GetLegacyTextMargin() ) );
    P( c, k, "min_size", V( tb->GetMinSize() ) );
    P( c, k, "shown_text", V( tb->GetShownText( RESOLVED ) ) );
    P( c, k, "hit_center", V( tb->HitTest( tb->GetCenter(), 0 ) ) );

    std::shared_ptr<SHAPE> eff = tb->GetEffectiveShape( layer );
    P( c, k, "effective_shape", H( VShape( eff.get() ) ) );

    SHAPE_POLY_SET text;
    tb->TransformTextToPolySet( text, 0, 5000, ERROR_INSIDE );
    P( c, k, "text_polys", H( VPolySet( text ) ) );

    SHAPE_POLY_SET box;
    tb->TransformShapeToPolygon( box, layer, 0, 5000, ERROR_INSIDE );
    P( c, k, "tpoly", H( VPolySet( box ) ) );

    SHAPE_POLY_SET boxOut;
    tb->TransformShapeToPolygon( boxOut, layer, 100000, 5000, ERROR_OUTSIDE );
    P( c, k, "tpoly_out", H( VPolySet( boxOut ) ) );

    if( tb->Type() == PCB_TABLECELL_T )
    {
        PCB_TABLECELL* cell = static_cast<PCB_TABLECELL*>( tb );
        P( c, k, "row_col", V( cell->GetRow() ) + "," + V( cell->GetColumn() ) );
        P( c, k, "addr", V( cell->GetAddr() ) );
        P( c, k, "col_width", V( cell->GetColumnWidth() ) );
        P( c, k, "row_height", V( cell->GetRowHeight() ) );
    }
}


static void DumpTable( const std::string& c, PCB_TABLE* t )
{
    PCB_LAYER_ID layer = t->GetLayer();

    P( c, "table", "layer", V( (int) layer ) );
    P( c, "table", "position", V( t->GetPosition() ) );
    P( c, "table", "end", V( t->GetEnd() ) );
    P( c, "table", "bbox", V( t->GetBoundingBox() ) );

    std::string widths;

    for( int col = 0; col < t->GetColCount(); ++col )
        widths += ( col ? " " : "" ) + V( t->GetColWidth( col ) );

    P( c, "table", "col_widths", widths );

    std::string heights;

    for( int row = 0; row < t->GetRowCount(); ++row )
        heights += ( row ? " " : "" ) + V( t->GetRowHeight( row ) );

    P( c, "table", "row_heights", heights );

    std::string order;

    for( PCB_TABLECELL* cell : t->GetCells() )
        order += ( order.empty() ? "" : " " ) + V( cell->GetText() );

    P( c, "table", "cell_order", order );

    std::string borders;

    t->DrawBorders(
            [&]( const VECTOR2I& aPt1, const VECTOR2I& aPt2, const STROKE_PARAMS& aStroke )
            {
                borders += ( borders.empty() ? "" : " " ) + V( aPt1 ) + ">" + V( aPt2 ) + "/"
                           + V( aStroke.GetWidth() );
            } );

    P( c, "table", "borders", H( borders ) );

    std::shared_ptr<SHAPE> eff = t->GetEffectiveShape( layer );
    P( c, "table", "effective_shape", H( VShape( eff.get() ) ) );

    SHAPE_POLY_SET poly;
    t->TransformShapeToPolygon( poly, layer, 0, 5000, ERROR_INSIDE );
    P( c, "table", "tpoly", H( VPolySet( poly ) ) );

    SHAPE_POLY_SET set0;
    t->TransformShapeToPolySet( set0, layer, 0, 5000, ERROR_INSIDE );
    P( c, "table", "tpolyset", H( VPolySet( set0 ) ) );

    SHAPE_POLY_SET set1;
    t->TransformShapeToPolySet( set1, layer, 100000, 5000, ERROR_OUTSIDE );
    P( c, "table", "tpolyset_out", H( VPolySet( set1 ) ) );

    P( c, "table", "hit_pos", V( t->HitTest( t->GetPosition(), 0 ) ) );

    int ii = 0;

    for( PCB_TABLECELL* cell : t->GetCells() )
        DumpBox( c, "cell" + std::to_string( ii++ ), cell );
}


static const std::vector<int> WIDTHS = { 8 * MM, 4 * MM, 6 * MM };
static const std::vector<int> HEIGHTS = { 2 * MM, 3 * MM };
static const char* TEXTS[2][3] = { { "Hdr", "Second cell text that wraps", "C" },
                                   { "row2", "${ROW}:${COL} ${ADDR}", "x" } };


// A 3 x 2 table with its cells laid out from aOrigin; aSpans makes the first header cell span
// two columns.
static PCB_TABLE* MakeTable( BOARD_ITEM* aParent, const VECTOR2I& aOrigin, bool aSpans )
{
    PCB_TABLE* t = new PCB_TABLE( aParent, pcbIUScale.mmToIU( 0.1 ) );
    t->SetLayer( F_SilkS );
    t->SetColCount( 3 );

    for( int col = 0; col < 3; ++col )
        t->SetColWidth( col, WIDTHS[col] );

    for( int row = 0; row < 2; ++row )
        t->SetRowHeight( row, HEIGHTS[row] );

    int y = aOrigin.y;

    for( int row = 0; row < 2; ++row )
    {
        int x = aOrigin.x;

        for( int col = 0; col < 3; ++col )
        {
            PCB_TABLECELL* cell = new PCB_TABLECELL( t );
            cell->SetMarginLeft( MM / 2 );
            cell->SetMarginTop( MM / 4 );
            cell->SetMarginRight( MM / 2 );
            cell->SetMarginBottom( MM / 4 );
            cell->SetTextSize( VECTOR2I( MM, MM ) );
            cell->SetStart( VECTOR2I( x, y ) );
            cell->SetEnd( VECTOR2I( x + WIDTHS[col], y + HEIGHTS[row] ) );
            cell->SetText( wxString::FromUTF8( TEXTS[row][col] ) );
            t->AddCell( cell );
            x += WIDTHS[col];
        }

        y += HEIGHTS[row];
    }

    if( aSpans )
    {
        t->GetCell( 0, 0 )->SetColSpan( 2 );
        t->GetCell( 0, 1 )->SetColSpan( 0 );
        t->GetCell( 0, 1 )->SetRowSpan( 0 );
    }

    t->Normalize();
    return t;
}


static PCB_TEXTBOX* MakeTextbox( BOARD_ITEM* aParent, const VECTOR2I& aOrigin )
{
    PCB_TEXTBOX* tb = new PCB_TEXTBOX( aParent );
    tb->SetLayer( F_SilkS );
    tb->SetMarginLeft( MM / 2 );
    tb->SetMarginTop( MM / 3 );
    tb->SetMarginRight( MM / 4 );
    tb->SetMarginBottom( MM / 5 );
    tb->SetTextSize( VECTOR2I( MM, 3 * MM / 2 ) );
    tb->SetTextThickness( 150000 );
    tb->SetStart( aOrigin );
    tb->SetEnd( aOrigin + VECTOR2I( 20 * MM, 5 * MM ) );
    tb->SetText( wxT( "Hello World wrapping text in a box" ) );
    return tb;
}


static FOOTPRINT* MakeFootprint( BOARD& aBoard )
{
    FOOTPRINT* fp = new FOOTPRINT( &aBoard );
    fp->SetPosition( VECTOR2I( 10 * MM, 20 * MM ) );
    fp->SetOrientation( EDA_ANGLE( 30.0, DEGREES_T ) );
    fp->SetTransformScale( 1.5, 0.75 );
    aBoard.Add( fp );
    return fp;
}


typedef std::function<void( BOARD_ITEM* )> OP;

static const VECTOR2I PIVOT( 1 * MM, 1 * MM );
static const VECTOR2I FLIP_CENTRE( 10 * MM, 10 * MM );

static std::vector<std::pair<std::string, OP>> Ops( bool aTable )
{
    std::vector<std::pair<std::string, OP>> ops = {
        { "none", []( BOARD_ITEM* ) {} },
        { "rot30", []( BOARD_ITEM* i ) { i->Rotate( PIVOT, EDA_ANGLE( 30.0, DEGREES_T ) ); } },
        { "rot90", []( BOARD_ITEM* i ) { i->Rotate( PIVOT, EDA_ANGLE( 90.0, DEGREES_T ) ); } },
        { "flipLR", []( BOARD_ITEM* i ) { i->Flip( FLIP_CENTRE, FLIP_DIRECTION::LEFT_RIGHT ); } },
        { "flipTB", []( BOARD_ITEM* i ) { i->Flip( FLIP_CENTRE, FLIP_DIRECTION::TOP_BOTTOM ); } },
        { "mirLR", []( BOARD_ITEM* i ) { i->Mirror( FLIP_CENTRE, FLIP_DIRECTION::LEFT_RIGHT ); } },
        { "mirTB", []( BOARD_ITEM* i ) { i->Mirror( FLIP_CENTRE, FLIP_DIRECTION::TOP_BOTTOM ); } },
        { "rot30flipLR",
          []( BOARD_ITEM* i )
          {
              i->Rotate( PIVOT, EDA_ANGLE( 30.0, DEGREES_T ) );
              i->Flip( FLIP_CENTRE, FLIP_DIRECTION::LEFT_RIGHT );
          } },
        { "move", []( BOARD_ITEM* i ) { i->Move( VECTOR2I( 1234567, -765432 ) ); } },
    };

    if( aTable )
    {
        ops.push_back( { "autosize", []( BOARD_ITEM* i ) { static_cast<PCB_TABLE*>( i )->Autosize(); } } );
    }
    else
    {
        ops.push_back( { "angle45",
                         []( BOARD_ITEM* i )
                         { static_cast<PCB_TEXTBOX*>( i )->SetTextAngle( EDA_ANGLE( 45.0, DEGREES_T ) ); } } );
        ops.push_back( { "rot30normalize",
                         []( BOARD_ITEM* i )
                         {
                             i->Rotate( PIVOT, EDA_ANGLE( 30.0, DEGREES_T ) );
                             i->Normalize();
                         } } );
    }

    return ops;
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

    for( bool inFp : { false, true } )
    {
        for( bool spans : { false, true } )
        {
            for( const auto& [name, op] : Ops( true ) )
            {
                BOARD      board;
                FOOTPRINT* fp = inFp ? MakeFootprint( board ) : nullptr;
                VECTOR2I   origin = inFp ? VECTOR2I( 11 * MM, 21 * MM ) : VECTOR2I( 2 * MM, 3 * MM );
                PCB_TABLE* t = MakeTable( inFp ? (BOARD_ITEM*) fp : (BOARD_ITEM*) &board, origin, spans );

                if( fp )
                    fp->Add( t, ADD_MODE::APPEND );
                else
                    board.Add( t, ADD_MODE::APPEND );

                op( t );

                std::string c = std::string( "table" ) + ( inFp ? "_fp" : "" ) + ( spans ? "_span" : "" ) + "_"
                                + name;
                DumpTable( c, t );
            }
        }

        for( const auto& [name, op] : Ops( false ) )
        {
            BOARD        board;
            FOOTPRINT*   fp = inFp ? MakeFootprint( board ) : nullptr;
            VECTOR2I     origin = inFp ? VECTOR2I( 11 * MM, 21 * MM ) : VECTOR2I( 2 * MM, 3 * MM );
            PCB_TEXTBOX* tb = MakeTextbox( inFp ? (BOARD_ITEM*) fp : (BOARD_ITEM*) &board, origin );

            if( fp )
                fp->Add( tb, ADD_MODE::APPEND );
            else
                board.Add( tb, ADD_MODE::APPEND );

            op( tb );

            std::string c = std::string( "textbox" ) + ( inFp ? "_fp" : "" ) + "_" + name;
            DumpBox( c, "tb", tb );
        }
    }

    return 0;
}
