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
 * PCBIO-12 oracle driver: pure parse + write of a board file with the reference build, i.e.
 * KI_TEST::ReadBoardFromFileOrStream (PCB_IO_KICAD_SEXPR_PARSER only, no project, no
 * BOARD_LOADER post-processing, in particular no RefreshGeneratedTables) followed by
 * PCB_IO_KICAD_SEXPR::SaveBoard. `kicad-cli pcb upgrade` cannot be used for generated tables
 * (drill charts, stackup tables) because its loader rebuilds their cells from the board.
 *
 * usage: pcbio12-resave IN OUT [IN OUT ...]
 */

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
#include <qa_utils/wx_utils/unit_test_utils.h>
#include <pcbnew_utils/board_file_utils.h>
#include <board.h>
#include <pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h>


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 3 || ( argc - 1 ) % 2 != 0 )
    {
        fprintf( stderr, "usage: pcbio12-resave IN OUT [IN OUT ...]\n" );
        return 2;
    }

    // Headless initialisation, as qa/tests/pcbnew/test_module.cpp::init_unit_test()
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

    int failures = 0;

    for( int i = 1; i + 1 < argc; i += 2 )
    {
        try
        {
            std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( argv[i] );

            if( !board )
            {
                fprintf( stderr, "%s: not a board\n", argv[i] );
                failures++;
                continue;
            }

            PCB_IO_KICAD_SEXPR io;
            io.SaveBoard( wxString::FromUTF8( argv[i + 1] ), board.get() );
            printf( "saved %s\n", argv[i + 1] );
        }
        catch( const IO_ERROR& e )
        {
            fprintf( stderr, "%s: %s\n", argv[i], e.What().ToStdString().c_str() );
            failures++;
        }
        catch( const std::exception& e )
        {
            fprintf( stderr, "%s: %s\n", argv[i], e.what() );
            failures++;
        }
    }

    fflush( stdout );
    _exit( failures ? 1 : 0 );
}
