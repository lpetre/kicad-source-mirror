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
 * drc34-report-oracle: the REPORTER texts of DRC_ENGINE::EvalRules, EvalZoneConnection and
 * ProcessAssertions (package DRC-34), each call with its own WX_STRING_REPORTER.
 *
 *   drc34-report-oracle FILE...
 *
 * Each board is loaded like KI_TEST::LoadBoard (the loader of drc29_oracle.cpp). The items
 * and pairs are those of rust/tools/pcb-dump/dump_evalrules.cpp, capped at the first K_ITEMS
 * items of each list:
 *
 *   track / arc / via / pad / zone / fpzone / drawing  CLEARANCE per enabled copper layer of
 *       the item, then every constraint type but DISALLOW and HOLE_TO_HOLE (which
 *       dereferences the missing second item) on the item's layer;
 *   disallow  DISALLOW on UNDEFINED_LAYER;
 *   assert    ProcessAssertions( item );
 *   inch      the first item of each list again (CLEARANCE on its layer and every unary type)
 *             after SetUserUnits( INCH ) (the engine is reset to MM afterwards);
 *   pair_tt / pair_pp / pair_tp / pair_zt / pair_zp / pair_zz  (dump_evalrules.cpp pairs of
 *       the first K_ITEMS items): CLEARANCE per shared enabled copper layer (pair_zp also
 *       EvalZoneConnection, THERMAL_RELIEF_GAP and THERMAL_SPOKE_WIDTH), then PAIR_TYPES on
 *       UNDEFINED_LAYER.
 *
 * Output: `file|<name>`, then per call `r|<category>|<item index>[:<partner index>]|<field>|
 * <messages>` (field `c<type>@<layer>`, `zc@<layer>` or `assert`; the messages of
 * WX_STRING_REPORTER::GetMessages() with "\" as "\\" and newlines as "\n").
 */

#include <algorithm>
#include <cstdio>
#include <cstdlib>
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
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <zone.h>
#include <reporter.h>
#include <drc/drc_engine.h>
#include <drc/drc_rule.h>
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



static const size_t K_ITEMS = 6;
static const int    K_PAIRS = 3;

static const DRC_CONSTRAINT_T PAIR_TYPES[] = {
    HOLE_CLEARANCE_CONSTRAINT,      HOLE_TO_HOLE_CONSTRAINT,       DIFF_PAIR_GAP_CONSTRAINT,
    PHYSICAL_CLEARANCE_CONSTRAINT,  PHYSICAL_HOLE_CLEARANCE_CONSTRAINT,
    EDGE_CLEARANCE_CONSTRAINT,      SILK_CLEARANCE_CONSTRAINT,     COURTYARD_CLEARANCE_CONSTRAINT,
    CREEPAGE_CONSTRAINT,            SKEW_CONSTRAINT,               MAX_UNCOUPLED_CONSTRAINT
};


static std::string escape( const wxString& aText )
{
    std::string in( aText.ToUTF8() );
    std::string out;

    for( char c : in )
    {
        if( c == '\\' )
            out += "\\\\";
        else if( c == '\n' )
            out += "\\n";
        else
            out += c;
    }

    return out;
}


static std::string field( DRC_CONSTRAINT_T aType, PCB_LAYER_ID aLayer )
{
    return "c" + std::to_string( (int) aType ) + "@" + std::to_string( (int) aLayer );
}


static void emit( const std::string& aCat, const std::string& aKey, const std::string& aField,
                  const WX_STRING_REPORTER& aReporter )
{
    printf( "r|%s|%s|%s|%s\n", aCat.c_str(), aKey.c_str(), aField.c_str(),
            escape( aReporter.GetMessages() ).c_str() );
}


static LSEQ copperSeq( BOARD* aBoard, const LSET& aSet )
{
    return LSET( aSet & aBoard->GetEnabledLayers() & LSET::AllCuMask() ).Seq();
}


static void evalOne( DRC_ENGINE* aEngine, const std::string& aCat, const std::string& aKey,
                     DRC_CONSTRAINT_T aType, BOARD_ITEM* a, BOARD_ITEM* b, PCB_LAYER_ID aLayer )
{
    WX_STRING_REPORTER r;

    try
    {
        aEngine->EvalRules( aType, a, b, aLayer, &r );
    }
    catch( ... )
    {
        r.Report( wxS( "<exception>" ) );
    }

    emit( aCat, aKey, field( aType, aLayer ), r );
}


static void unary( BOARD* aBoard, DRC_ENGINE* aEngine, const std::string& aCat, size_t aIndex,
                   BOARD_ITEM* aItem, bool aAllCopperLayers )
{
    std::string key = std::to_string( aIndex );

    if( aAllCopperLayers )
    {
        for( PCB_LAYER_ID layer : copperSeq( aBoard, aItem->GetLayerSet() ) )
            evalOne( aEngine, aCat, key, CLEARANCE_CONSTRAINT, aItem, nullptr, layer );
    }
    else
    {
        evalOne( aEngine, aCat, key, CLEARANCE_CONSTRAINT, aItem, nullptr, aItem->GetLayer() );
    }

    for( int t = CLEARANCE_CONSTRAINT + 1; t <= HOLE_ASPECT_RATIO_CONSTRAINT; ++t )
    {
        // DISALLOW has its own category; HOLE_TO_HOLE dereferences the missing b for an item
        // with a hole
        if( t == DISALLOW_CONSTRAINT || t == HOLE_TO_HOLE_CONSTRAINT )
            continue;

        evalOne( aEngine, aCat, key, (DRC_CONSTRAINT_T) t, aItem, nullptr, aItem->GetLayer() );
    }
}


static void pair( BOARD* aBoard, DRC_ENGINE* aEngine, const std::string& aCat, size_t i, size_t j,
                  BOARD_ITEM* a, BOARD_ITEM* b, bool aZoneConnection )
{
    std::string key = std::to_string( i ) + ":" + std::to_string( j );

    for( PCB_LAYER_ID layer : copperSeq( aBoard, a->GetLayerSet() & b->GetLayerSet() ) )
    {
        evalOne( aEngine, aCat, key, CLEARANCE_CONSTRAINT, a, b, layer );

        if( aZoneConnection )
        {
            WX_STRING_REPORTER r;

            try
            {
                aEngine->EvalZoneConnection( a, b, layer, &r );
            }
            catch( ... )
            {
                r.Report( wxS( "<exception>" ) );
            }

            emit( aCat, key, "zc@" + std::to_string( (int) layer ), r );
            evalOne( aEngine, aCat, key, THERMAL_RELIEF_GAP_CONSTRAINT, a, b, layer );
            evalOne( aEngine, aCat, key, THERMAL_SPOKE_WIDTH_CONSTRAINT, a, b, layer );
        }
    }

    for( DRC_CONSTRAINT_T type : PAIR_TYPES )
        evalOne( aEngine, aCat, key, type, a, b, UNDEFINED_LAYER );
}


static int netOf( BOARD_ITEM* aItem )
{
    return static_cast<BOARD_CONNECTED_ITEM*>( aItem )->GetNetCode();
}


static void dumpBoard( BOARD* aBoard, DRC_ENGINE* aEngine )
{
    std::vector<std::pair<std::string, BOARD_ITEM*>> tracks;
    std::vector<BOARD_ITEM*> trackItems, pads, zones, fpZones, drawings;

    for( PCB_TRACK* track : aBoard->Tracks() )
    {
        trackItems.push_back( track );
        tracks.emplace_back( track->Type() == PCB_VIA_T   ? "via"
                             : track->Type() == PCB_ARC_T ? "arc"
                                                          : "track",
                             track );
    }

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
            pads.push_back( pad );
    }

    for( ZONE* zone : aBoard->Zones() )
        zones.push_back( zone );

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( ZONE* zone : fp->Zones() )
            fpZones.push_back( zone );
    }

    for( BOARD_ITEM* item : aBoard->Drawings() )
    {
        if( item->Type() == PCB_SHAPE_T || item->Type() == PCB_TEXT_T
            || item->Type() == PCB_TEXTBOX_T )
        {
            drawings.push_back( item );
        }
    }

    // The unary categories: the first K_ITEMS items of each kind (tracks by their own kind).
    std::vector<std::pair<std::string, std::vector<BOARD_ITEM*>>> lists;
    lists.push_back( { "track", {} } );
    lists.push_back( { "arc", {} } );
    lists.push_back( { "via", {} } );

    for( auto& [cat, item] : tracks )
    {
        for( auto& [name, list] : lists )
        {
            if( name == cat && list.size() < K_ITEMS )
                list.push_back( item );
        }
    }

    auto firstK = []( const std::vector<BOARD_ITEM*>& aList )
    {
        return std::vector<BOARD_ITEM*>( aList.begin(),
                                         aList.begin() + std::min( K_ITEMS, aList.size() ) );
    };

    lists.push_back( { "pad", firstK( pads ) } );
    lists.push_back( { "zone", firstK( zones ) } );
    lists.push_back( { "fpzone", firstK( fpZones ) } );
    lists.push_back( { "drawing", firstK( drawings ) } );

    for( auto& [cat, list] : lists )
    {
        for( size_t i = 0; i < list.size(); ++i )
        {
            unary( aBoard, aEngine, cat, i, list[i], true );
            evalOne( aEngine, "disallow", cat + std::to_string( i ), DISALLOW_CONSTRAINT, list[i],
                     nullptr, UNDEFINED_LAYER );

            WX_STRING_REPORTER r;

            try
            {
                aEngine->ProcessAssertions( list[i], []( const DRC_CONSTRAINT* ) {}, &r );
            }
            catch( ... )
            {
                r.Report( wxS( "<exception>" ) );
            }

            emit( "assert", cat + std::to_string( i ), "assert", r );
        }
    }

    aEngine->SetUserUnits( EDA_UNITS::INCH );

    for( auto& [cat, list] : lists )
    {
        if( !list.empty() )
            unary( aBoard, aEngine, "inch", 0, list[0], false );
    }

    aEngine->SetUserUnits( EDA_UNITS::MM );

    // Pairs (dump_evalrules.cpp dumpPairs, outer index below K_ITEMS)
    auto nextOfOtherNets =
            [&]( const std::string& aCategory, const std::vector<BOARD_ITEM*>& aList )
            {
                for( size_t i = 0; i < aList.size() && i < K_ITEMS; ++i )
                {
                    int found = 0;

                    for( size_t j = i + 1; j < aList.size() && found < K_PAIRS; ++j )
                    {
                        if( netOf( aList[j] ) == netOf( aList[i] ) )
                            continue;

                        pair( aBoard, aEngine, aCategory, i, j, aList[i], aList[j], false );
                        found++;
                    }
                }
            };

    nextOfOtherNets( "pair_tt", trackItems );
    nextOfOtherNets( "pair_pp", pads );

    if( !pads.empty() )
    {
        for( size_t i = 0; i < trackItems.size() && i < K_ITEMS; ++i )
        {
            int found = 0;

            for( size_t m = 0; m < 3 * K_PAIRS && found < K_PAIRS; ++m )
            {
                size_t p = ( 7 * i + m ) % pads.size();

                if( netOf( pads[p] ) == netOf( trackItems[i] ) )
                    continue;

                pair( aBoard, aEngine, "pair_tp", i, p, trackItems[i], pads[p], false );
                found++;
            }
        }
    }

    for( size_t z = 0; z < zones.size() && z < K_ITEMS; ++z )
    {
        int found = 0;

        for( size_t j = 0; j < trackItems.size() && found < K_PAIRS; ++j )
        {
            if( netOf( trackItems[j] ) == netOf( zones[z] ) )
                continue;

            pair( aBoard, aEngine, "pair_zt", z, j, zones[z], trackItems[j], false );
            found++;
        }

        for( size_t j = 0; j < pads.size() && (int) j < K_PAIRS; ++j )
            pair( aBoard, aEngine, "pair_zp", z, j, pads[j], zones[z], true );

        if( z + 1 < zones.size() )
            pair( aBoard, aEngine, "pair_zz", z, z + 1, zones[z], zones[z + 1], false );
    }
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
        fprintf( stderr, "usage: drc34-report-oracle FILE...\n" );
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

        if( !board || !board->GetDesignSettings().m_DRCEngine )
        {
            printf( "error|load\n" );
            continue;
        }

        dumpBoard( board.get(), board->GetDesignSettings().m_DRCEngine.get() );
        fflush( stdout );

        Pgm().GetSettingsManager().UnloadProject( &Pgm().GetSettingsManager().Prj(), false );
    }

    return 0;
}
