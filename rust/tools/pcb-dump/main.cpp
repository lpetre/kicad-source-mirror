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

/**
 * pcb-dump: load boards with the real pcbnew code and print the data model in a stable
 * line-oriented format (rust/docs/PCB_DUMP_FORMAT.md).
 *
 * Initialisation is the one of qa/tests/pcbnew/test_module.cpp (init_unit_test) and loading the
 * one of KI_TEST::LoadBoard (qa/pcbnew_utils/board_test_utils.cpp), minus the Boost.Test
 * checkpoints.  --loader uses BOARD_LOADER::Load like kicad-cli (PCBNEW_JOBS_HANDLER::getBoard).
 */

#include "pcb_dump.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <unordered_set>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/filename.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <project.h>
#include <pcbnew_settings.h>
#include <qa_utils/mock_utils.h>
#include <qa_utils/wx_utils/wx_assert.h>
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <board_loader.h>
#include <drc/drc_engine.h>
#include <length_delay_calculation/length_delay_calculation.h>
#include <pcb_io/pcb_io_mgr.h>
#include <wildcards_and_files_ext.h>
#include <kiway_player.h>


/// wx assertions during loading are counted and otherwise ignored (as in kicad-cli); during
/// dumping they throw KI_TEST::WX_ASSERT_ERROR (as in the QA tests) and are reported per section.
static int s_loadAsserts = 0;

static void countingAssertHandler( const wxString&, int, const wxString&, const wxString&,
                                   const wxString& )
{
    s_loadAsserts++;
}


static void usage()
{
    fprintf( stderr,
             "usage: pcb-dump defaults [--hash]\n"
             "       pcb-dump board [--sections LIST] [--hash] [--loader] [--no-init] FILE...\n"
             "sections: item,bbox,track,pad,shape,text,dim,zone,footprint,board,order\n" );
}


/// Headless initialisation, as qa/tests/pcbnew/test_module.cpp::init_unit_test().
static bool initPgm( int argc, char** argv )
{
    KI_TEST::SetMockConfigDir();
    SetPgm( new MOCK_PGM_BASE() );
    KIPLATFORM::APP::Init();

    wxApp::SetInstance( new wxAppConsole );

    if( !wxInitialize( argc, argv ) )
        return false;

    wxLog::EnableLogging( false );

    Pgm().InitPgm( true, true );
    Pgm().GetSettingsManager().RegisterSettings( new PCBNEW_SETTINGS, false );
    Pgm().GetSettingsManager().Load();
    Pgm().GetSettingsManager().LoadProject( "" );
    return true;
}


/**
 * KI_TEST::LoadBoard without the Boost.Test macros: bind <base>.kicad_pro (or <base>.pro) if it
 * exists, parse with PCB_IO_KICAD_SEXPR_PARSER (ReadBoardFromFileOrStream), set the project,
 * init a DRC engine with <base>.kicad_dru, BuildListOfNets, BuildConnectivity,
 * SynchronizeTuningProfileProperties and (with a project) SynchronizeComponentClasses.
 *
 * With aInit == false only the parser runs (no project, no post-processing).
 */
static std::unique_ptr<BOARD> loadBoardQa( const wxString& aPath, bool aInit )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        fn( aPath );
    fn.MakeAbsolute();
    wxFileName projectFile( fn );
    projectFile.SetExt( FILEEXT::ProjectFileExtension );
    wxFileName legacyProject( fn );
    legacyProject.SetExt( FILEEXT::LegacyProjectFileExtension );
    wxFileName rulesFile( fn );
    rulesFile.SetExt( FILEEXT::DesignRulesFileExtension );

    bool hasProject = false;

    if( aInit )
    {
        if( projectFile.Exists() )
        {
            mgr.LoadProject( projectFile.GetFullPath() );
            hasProject = true;
        }
        else if( legacyProject.Exists() )
        {
            mgr.LoadProject( legacyProject.GetFullPath() );
            hasProject = true;
        }
    }

    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( fn.GetFullPath().ToStdString() );

    if( !board )
        THROW_IO_ERROR( wxT( "file could not be read or is not a board" ) );

    if( !aInit )
        return board;

    if( hasProject )
        board->SetProject( &mgr.Prj() );

    auto drcEngine = std::make_shared<DRC_ENGINE>( board.get(), &board->GetDesignSettings() );

    try
    {
        if( rulesFile.Exists() )
            drcEngine->InitEngine( rulesFile );
        else
            drcEngine->InitEngine( wxFileName() );
    }
    catch( ... )
    {
        // Rule errors are not load errors; KI_TEST::LoadBoard does not catch them either but the
        // corpus contains broken rule files.
        g_out.Error( "load", "drc", "DRC rules failed to initialise" );
    }

    board->GetDesignSettings().m_DRCEngine = drcEngine;
    board->BuildListOfNets();
    board->BuildConnectivity();
    board->GetLengthCalculation()->SynchronizeTuningProfileProperties();

    if( board->GetProject() )
    {
        std::unordered_set<wxString> dummy;
        board->SynchronizeComponentClasses( dummy );
    }

    return board;
}


/// kicad-cli load path (PCBNEW_JOBS_HANDLER::getBoard): project from <base>.kicad_pro (created
/// in memory when missing), then BOARD_LOADER::Load (which also builds connectivity).
static std::unique_ptr<BOARD> loadBoardCli( const wxString& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        pro( aPath );
    pro.SetExt( FILEEXT::ProjectFileExtension );
    pro.MakeAbsolute();

    PROJECT* project = mgr.GetProject( pro.GetFullPath() );

    if( !project )
    {
        mgr.LoadProject( pro.GetFullPath(), true );
        project = mgr.GetProject( pro.GetFullPath() );
    }

    PCB_IO_MGR::PCB_FILE_T type = PCB_IO_MGR::FindPluginTypeFromBoardPath( aPath, KICTL_KICAD_ONLY );

    if( !project || type == PCB_IO_MGR::FILE_TYPE_NONE )
        THROW_IO_ERROR( wxT( "no project or unknown board type" ) );

    return BOARD_LOADER::Load( aPath, type, project );
}


int main( int argc, char** argv )
{
    // Deterministic, headless environment (same as the kicad-cli oracle wrapper).
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 2 )
    {
        usage();
        return 2;
    }

    std::string           cmd = argv[1];
    DUMP_OPTIONS          opts;
    bool                  useLoader = false;
    bool                  init = true;
    std::vector<wxString> files;

    for( int i = 2; i < argc; ++i )
    {
        std::string a = argv[i];

        if( a == "--hash" )
        {
            opts.hash = true;
        }
        else if( a == "--loader" )
        {
            useLoader = true;
        }
        else if( a == "--no-init" )
        {
            init = false;
        }
        else if( a == "--sections" && i + 1 < argc )
        {
            std::string list = argv[++i];
            size_t      pos = 0;

            while( pos <= list.size() )
            {
                size_t comma = list.find( ',', pos );

                if( comma == std::string::npos )
                    comma = list.size();

                if( comma > pos )
                    opts.sections.insert( list.substr( pos, comma - pos ) );

                pos = comma + 1;
            }
        }
        else if( !a.empty() && a[0] == '-' )
        {
            usage();
            return 2;
        }
        else
        {
            files.push_back( wxString::FromUTF8( a ) );
        }
    }

    if( cmd != "defaults" && cmd != "board" )
    {
        usage();
        return 2;
    }

    if( !initPgm( 1, argv ) )
    {
        fprintf( stderr, "pcb-dump: wx initialisation failed\n" );
        return 1;
    }

    wxSetAssertHandler( &KI_TEST::wxAssertThrower );
    KIID::SeedGenerator( 1 );   // generated UUIDs are reproducible from run to run
    g_out.m_hash = opts.hash;
    int failures = 0;

    if( cmd == "defaults" )
    {
        Guard( "defaults", [&]() { DumpDefaults(); } );
    }
    else
    {
        for( const wxString& file : files )
        {
            std::string name = std::string( wxFileName( file ).GetFullName().ToUTF8() );
            printf( "file|%s|\n", name.c_str() );
            fflush( stdout );

            std::unique_ptr<BOARD> board;

            s_loadAsserts = 0;
            wxSetAssertHandler( &countingAssertHandler );

            try
            {
                board = useLoader ? loadBoardCli( file ) : loadBoardQa( file, init );
            }
            catch( const IO_ERROR& e )
            {
                g_out.Error( "load", name, e.What().ToStdString() );
            }
            catch( const std::exception& e )
            {
                g_out.Error( "load", name, e.what() );
            }
            catch( ... )
            {
                g_out.Error( "load", name, "unknown exception" );
            }

            wxSetAssertHandler( &KI_TEST::wxAssertThrower );

            if( s_loadAsserts )
                printf( "warn|load|%s|wx assertions during load: %d\n", name.c_str(), s_loadAsserts );

            if( !board )
            {
                failures++;
                continue;
            }

            ScanFileUuids( std::string( file.ToUTF8() ) );
            Guard( "dump", [&]() { DumpBoard( board.get(), opts ); } );
            fflush( stdout );

            board->SetProject( nullptr );
            board.reset();
            // Back to the empty project so that the next file starts from the same state.
            Pgm().GetSettingsManager().LoadProject( "" );
        }
    }

    fflush( stdout );

    // Skip global destructors / wxUninitialize: pcbnew singletons are not safe to tear down in
    // this headless setup and the output is complete at this point.
    _exit( failures ? 1 : 0 );
}
