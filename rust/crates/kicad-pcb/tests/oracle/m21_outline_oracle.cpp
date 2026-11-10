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

// Package M21 (board outline) differential oracle. Loads every board given on the command line
// with the real parser only (KI_TEST::ReadBoardFromFileOrStream, no project, as
// `pcb-dump board --no-init`) and runs the functions of pcbnew/convert_shape_list_to_polygon.cpp
// and BOARD::GetBoardPolygonOutlines / UpdateBoardOutline with a recording OUTLINE_ERROR_HANDLER.
// Output, one line each:
//
//   board|<file name>
//   <fn>|err|<message>|<uuid of item A or ->|<uuid of item B or ->|<x>,<y>     (handler calls)
//   <fn>|ok=<true|false>|n=<outlines>/<vertices>|poly=<FNV-1a 64 of the polygon text>
//
// The polygon text (hashed) is, per polygon "[" then per contour "C" or "O" (closed), "a" and
// the arc count, ":", then "x,y;" per vertex, then "]". Built and run by m21_outline_golden.sh;
// the Rust replay is convert_shape_list_to_polygon::tests::oracle_corpus.

#include "pcb_dump.h"

#include <cstdio>
#include <cstdlib>
#include <string>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/wx_utils/wx_assert.h>
#include <pcbnew_utils/board_file_utils.h>

#include <base_units.h>
#include <board.h>
#include <board_design_settings.h>
#include <convert_shape_list_to_polygon.h>
#include <footprint.h>
#include <pcb_board_outline.h>
#include <pcb_shape.h>
#include <geometry/shape_poly_set.h>


static std::string PolyText( const SHAPE_POLY_SET& aSet )
{
    std::string s;

    for( int ii = 0; ii < aSet.OutlineCount(); ++ii )
    {
        s += "[";

        for( const SHAPE_LINE_CHAIN& chain : aSet.CPolygon( ii ) )
        {
            s += chain.IsClosed() ? "C" : "O";
            s += "a" + std::to_string( chain.ArcCount() ) + ":";

            for( int jj = 0; jj < chain.PointCount(); ++jj )
            {
                s += std::to_string( chain.CPoint( jj ).x ) + "," + std::to_string( chain.CPoint( jj ).y )
                     + ";";
            }
        }

        s += "]";
    }

    return s;
}


static void Result( const char* aFn, bool aOk, const SHAPE_POLY_SET& aSet )
{
    int vertices = 0;

    for( int ii = 0; ii < aSet.OutlineCount(); ++ii )
    {
        for( const SHAPE_LINE_CHAIN& chain : aSet.CPolygon( ii ) )
            vertices += chain.PointCount();
    }

    printf( "%s|ok=%s|n=%d/%d|poly=#%016llx\n", aFn, aOk ? "true" : "false", aSet.OutlineCount(), vertices,
            (unsigned long long) Fnv1a64( PolyText( aSet ) ) );
}


static OUTLINE_ERROR_HANDLER Recorder( const char* aFn )
{
    return [aFn]( const wxString& msg, BOARD_ITEM* itemA, BOARD_ITEM* itemB, const VECTOR2I& pt )
    {
        printf( "%s|err|%s|%s|%s|%d,%d\n", aFn, msg.ToStdString().c_str(),
                itemA ? itemA->m_Uuid.AsString().ToStdString().c_str() : "-",
                itemB ? itemB->m_Uuid.AsString().ToStdString().c_str() : "-", pt.x, pt.y );
    };
}


template <typename FN>
static void Guarded( const char* aFn, FN aBody )
{
    try
    {
        aBody();
    }
    catch( const std::exception& e )
    {
        printf( "%s|exception\n", aFn );
    }
    catch( ... )
    {
        printf( "%s|exception\n", aFn );
    }

    fflush( stdout );
}


static void DumpBoard( BOARD* aBoard )
{
    BOARD_DESIGN_SETTINGS& bds = aBoard->GetDesignSettings();
    const int              eps = aBoard->GetOutlinesChainingEpsilon();

    Guarded( "test_items",
             [&]()
             {
                 OUTLINE_ERROR_HANDLER h = Recorder( "test_items" );
                 bool                  ok = TestBoardOutlinesGraphicItems( aBoard, pcbIUScale.mmToIU( 0.001 ), &h );
                 printf( "test_items|ok=%s\n", ok ? "true" : "false" );
             } );

    // DRC_TEST_PROVIDER_MISC::testOutline parameters
    Guarded( "drc_build",
             [&]()
             {
                 OUTLINE_ERROR_HANDLER h = Recorder( "drc_build" );
                 SHAPE_POLY_SET        poly;
                 bool ok = BuildBoardPolygonOutlines( aBoard, poly, pcbIUScale.mmToIU( 0.05 ), eps, true, &h );
                 Result( "drc_build", ok, poly );
             } );

    struct VARIANT
    {
        const char* name;
        bool        infer;
        bool        arcs;
        bool        npth;
    };

    for( const VARIANT& v : { VARIANT{ "outline", false, false, false }, VARIANT{ "outline_inferred", true, false, false },
                              VARIANT{ "outline_arcs", false, true, false },
                              VARIANT{ "outline_npth", false, false, true } } )
    {
        Guarded( v.name,
                 [&]()
                 {
                     OUTLINE_ERROR_HANDLER h = Recorder( v.name );
                     SHAPE_POLY_SET        poly;
                     bool ok = aBoard->GetBoardPolygonOutlines( poly, v.infer, &h, v.arcs, v.npth );
                     Result( v.name, ok, poly );
                 } );
    }

    Guarded( "fp_outline",
             [&]()
             {
                 OUTLINE_ERROR_HANDLER h = Recorder( "fp_outline" );
                 SHAPE_POLY_SET        poly;
                 bool ok = BuildFootprintPolygonOutlines( aBoard, poly, bds.m_MaxError, eps, &h );
                 Result( "fp_outline", ok, poly );
             } );

    // ConvertOutlineToPolygon without disjoint outlines, on the F.CrtYd shapes of each footprint
    // (the parameters of FOOTPRINT::BuildCourtyardCaches).
    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        std::vector<PCB_SHAPE*> list;

        for( BOARD_ITEM* item : fp->GraphicalItems() )
        {
            if( item->Type() == PCB_SHAPE_T && item->GetLayer() == F_CrtYd )
                list.push_back( static_cast<PCB_SHAPE*>( item ) );
        }

        if( list.empty() )
            continue;

        Guarded( "crtyd",
                 [&]()
                 {
                     OUTLINE_ERROR_HANDLER h = Recorder( "crtyd" );
                     SHAPE_POLY_SET        poly;
                     bool ok = ConvertOutlineToPolygon( list, poly, bds.m_MaxError, pcbIUScale.mmToIU( 0.02 ), false,
                                                        &h );
                     Result( "crtyd", ok, poly );
                 } );
    }

    Guarded( "board_outline",
             [&]()
             {
                 aBoard->UpdateBoardOutline();
                 Result( "board_outline", true, aBoard->BoardOutline()->GetOutline() );
             } );
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

    for( int ii = 1; ii < argc; ++ii )
    {
        std::string path = argv[ii];
        std::string name = path.substr( path.find_last_of( '/' ) + 1 );

        printf( "board|%s\n", name.c_str() );
        fflush( stdout );

        std::unique_ptr<BOARD> board;

        try
        {
            board = KI_TEST::ReadBoardFromFileOrStream( path );
        }
        catch( ... )
        {
        }

        if( !board )
        {
            printf( "load|error\n" );
            continue;
        }

        DumpBoard( board.get() );
    }

    return 0;
}
