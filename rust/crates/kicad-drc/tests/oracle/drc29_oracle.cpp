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
 * drc29-oracle: the copper sliver and connection width violations of the reference build.
 *
 *   drc29-oracle FILE...
 *
 * Each board is loaded like KI_TEST::LoadBoard (project, rule file, InitEngine, nets,
 * connectivity; the loader of rust/tools/pcb-dump/main.cpp), every DRC code is set to ignore
 * except DRCE_COPPER_SLIVER and DRCE_CONNECTION_WIDTH (error), and the real
 * DRC_ENGINE::RunTests( MM, true, false ) runs. Every violation the handler receives is
 * printed as
 *
 *   v|<settings key>|<error message>|<marker x>,<marker y>|<layer>|<item uuids>|<marker path>
 *
 * with the path as the start/end of each marker shape (the path generator applied to a
 * PCB_MARKER). copper_sliver lines come in handler order (the provider reports from one
 * sequential loop); connection_width lines are sorted, with their item uuids sorted (thread
 * pool and hash / address order, drc.md 4.12). `n|<count sliver>|<count width>` ends a board.
 */

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <string>
#include <unordered_set>
#include <vector>

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
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <pcb_marker.h>
#include <pcb_shape.h>
#include <drc/drc_engine.h>
#include <drc/drc_item.h>
#include <length_delay_calculation/length_delay_calculation.h>
#include <wildcards_and_files_ext.h>


static void ignoreAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
}



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


/// KI_TEST::LoadBoard without the Boost.Test macros (see rust/tools/pcb-dump/main.cpp).
static std::unique_ptr<BOARD> loadBoardQa( const wxString& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        fn( aPath );
    fn.MakeAbsolute();
    wxFileName projectFile( fn );
    projectFile.SetExt( FILEEXT::ProjectFileExtension );
    wxFileName rulesFile( fn );
    rulesFile.SetExt( FILEEXT::DesignRulesFileExtension );

    bool hasProject = false;

    if( projectFile.Exists() )
    {
        mgr.LoadProject( projectFile.GetFullPath() );
        hasProject = true;
    }

    std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( fn.GetFullPath().ToStdString() );

    if( !board )
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


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    std::vector<wxString> files;

    for( int i = 1; i < argc; ++i )
        files.push_back( wxString::FromUTF8( argv[i] ) );

    if( files.empty() )
    {
        fprintf( stderr, "usage: drc29-oracle FILE...\n" );
        return 2;
    }

    if( !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &ignoreAssert );
    KIID::SeedGenerator( 1 );

    for( const wxString& file : files )
    {
        printf( "file|%s\n", std::string( wxFileName( file ).GetFullName().ToUTF8() ).c_str() );
        fflush( stdout );

        std::unique_ptr<BOARD> board;

        try
        {
            board = loadBoardQa( file );
        }
        catch( ... )
        {
        }

        if( !board )
        {
            printf( "error|load\n" );
            continue;
        }

        BOARD_DESIGN_SETTINGS&     bds = board->GetDesignSettings();
        std::shared_ptr<DRC_ENGINE> engine = bds.m_DRCEngine;
        std::vector<std::string>   slivers;
        std::vector<std::string>   widths;
        std::mutex                 mutex;

        for( int ii = DRCE_FIRST; ii <= DRCE_LAST; ++ii )
            bds.m_DRCSeverities[ii] = SEVERITY::RPT_SEVERITY_IGNORE;

        bds.m_DRCSeverities[DRCE_COPPER_SLIVER] = SEVERITY::RPT_SEVERITY_ERROR;
        bds.m_DRCSeverities[DRCE_CONNECTION_WIDTH] = SEVERITY::RPT_SEVERITY_ERROR;

        engine->SetViolationHandler(
                [&]( const std::shared_ptr<DRC_ITEM>& aItem, const VECTOR2I& aPos, int aLayer,
                     const std::function<void( PCB_MARKER* )>& aPathGenerator )
                {
                    int code = aItem->GetErrorCode();

                    if( code != DRCE_COPPER_SLIVER && code != DRCE_CONNECTION_WIDTH )
                        return;

                    std::vector<std::string> ids;

                    for( const KIID& id : aItem->GetIDs() )
                        ids.push_back( std::string( id.AsString().ToUTF8() ) );

                    if( code == DRCE_CONNECTION_WIDTH )
                        std::sort( ids.begin(), ids.end() );

                    PCB_MARKER marker( aItem, aPos, aLayer );

                    if( aPathGenerator )
                        aPathGenerator( &marker );

                    std::string line = "v|" + std::string( aItem->GetSettingsKey().ToUTF8() ) + "|"
                                       + std::string( aItem->GetErrorMessage( false ).ToUTF8() ) + "|"
                                       + std::to_string( aPos.x ) + "," + std::to_string( aPos.y )
                                       + "|" + std::to_string( aLayer ) + "|";

                    for( size_t i = 0; i < ids.size(); ++i )
                        line += ( i ? "," : "" ) + ids[i];

                    line += "|";

                    for( const PCB_SHAPE& s : marker.GetPath() )
                    {
                        line += std::to_string( s.GetStart().x ) + "," + std::to_string( s.GetStart().y )
                                + ";" + std::to_string( s.GetEnd().x ) + ","
                                + std::to_string( s.GetEnd().y ) + " ";
                    }

                    std::lock_guard<std::mutex> lock( mutex );

                    if( code == DRCE_COPPER_SLIVER )
                        slivers.push_back( line );
                    else
                        widths.push_back( line );
                } );

        try
        {
            engine->RunTests( EDA_UNITS::MM, true, false );
        }
        catch( ... )
        {
            printf( "error|drc\n" );
        }

        engine->ClearViolationHandler();

        std::sort( widths.begin(), widths.end() );

        for( const std::string& l : slivers )
            printf( "%s\n", l.c_str() );

        for( const std::string& l : widths )
            printf( "%s\n", l.c_str() );

        printf( "n|%zu|%zu\n", slivers.size(), widths.size() );
        fflush( stdout );

        Pgm().GetSettingsManager().UnloadProject( &Pgm().GetSettingsManager().Prj(), false );
    }

    return 0;
}
