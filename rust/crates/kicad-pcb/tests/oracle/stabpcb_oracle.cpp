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

// Package STAB-PCB: reference values of the pad primitives of a footprint flip.
//
//   stabpcb_oracle FILE...
//
// Each board is parsed (PCB_IO_KICAD_SEXPR_PARSER, as pcb-dump --no-init), every footprint is
// flipped left/right about its position (FOOTPRINT::Flip -> PAD::Flip -> PAD::FlipPrimitives
// -> PCB_SHAPE::Flip, which reads PAD::GetPrimitiveLibScale()), then the primitives' board
// and library coordinates are printed, followed by the pcb-dump sections pad and shape of
// the flipped board. The Rust replay is tests/stab_pcb.rs.

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
#include <footprint.h>
#include <pad.h>
#include <pcb_shape.h>


static void P( const std::string& aKey, const std::string& aField, const std::string& aValue )
{
    printf( "prim|%s|%s=%s\n", aKey.c_str(), aField.c_str(), aValue.c_str() );
}


static void dumpPrimitives( BOARD* aBoard )
{
    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
        {
            double sx = 1.0, sy = 1.0;
            pad->GetPrimitiveLibScale( sx, sy );
            P( pad->m_Uuid.AsStdString(), "primitive_lib_scale", V( sx ) + "," + V( sy ) );

            pad->Padstack().ForEachUniqueLayer(
                    [&]( PCB_LAYER_ID aLayer )
                    {
                        int i = 0;

                        for( const std::shared_ptr<PCB_SHAPE>& prim : pad->Padstack().Primitives( aLayer ) )
                        {
                            std::string k = pad->m_Uuid.AsStdString() + "." + VLayer( aLayer ) + "."
                                            + std::to_string( i++ );
                            P( k, "shape", VEnum( prim->GetShape() ) );
                            P( k, "start", V( prim->GetStart() ) );
                            P( k, "end", V( prim->GetEnd() ) );

                            if( prim->GetShape() == SHAPE_T::ARC )
                                P( k, "mid", V( prim->GetArcMid() ) );

                            if( prim->GetShape() == SHAPE_T::BEZIER )
                                P( k, "bezier", VPoints( { prim->GetBezierC1(), prim->GetBezierC2() } ) );

                            if( prim->GetShape() == SHAPE_T::POLY )
                                P( k, "poly", VPolySet( prim->GetPolyShape() ) );

                            P( k, "lib.shape", VEnum( prim->GetLibraryShape() ) );
                            P( k, "lib.start", V( prim->GetLibraryStart() ) );
                            P( k, "lib.end", V( prim->GetLibraryEnd() ) );

                            if( prim->GetLibraryShape() == SHAPE_T::ARC )
                                P( k, "lib.arc_mid", V( prim->GetLibraryArcMid() ) );

                            if( prim->GetLibraryShape() == SHAPE_T::BEZIER )
                            {
                                P( k, "lib.bezier", VPoints( { prim->GetLibraryBezierC1(),
                                                               prim->GetLibraryBezierC2() } ) );
                            }

                            if( prim->GetLibraryShape() == SHAPE_T::POLY )
                                P( k, "lib.poly", VPolySet( prim->GetLibraryPolyShape() ) );
                        }
                    } );
        }
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
    wxSetAssertHandler( &KI_TEST::wxAssertThrower );
    KIID::SeedGenerator( 1 );

    for( int a = 1; a < argc; a++ )
    {
        std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( argv[a] );

        if( !board )
            return 1;

        for( FOOTPRINT* fp : board->Footprints() )
            fp->Flip( fp->GetPosition(), FLIP_DIRECTION::LEFT_RIGHT );

        dumpPrimitives( board.get() );

        ScanFileUuids( argv[a] );
        DUMP_OPTIONS opts;
        opts.sections = { "pad", "shape" };
        DumpBoard( board.get(), opts );
    }

    return 0;
}
