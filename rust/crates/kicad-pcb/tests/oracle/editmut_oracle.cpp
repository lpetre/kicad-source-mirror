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

// Package FIX-EDIT-1: reference values of the editing paths (mutators no load path runs).
//
//   editmut_oracle OUTDIR FILE...
//
// For every board and every scripted edit (EDITS below) the board is parsed afresh
// (KI_TEST::ReadBoardFromFileOrStream, the parser only, as pcb-dump --no-init), the edit is
// applied to every item of the walk (top level: footprints, drawings, tracks, zones, points,
// generators, groups; "c_" edits: every child of every footprint), then
//   "@@|<edit>|<board>" and the full pcb-dump of the edited board are printed on stdout and
//   the board is saved (PCB_IO_KICAD_SEXPR::SaveBoard) as OUTDIR/<edit>__<board>.
// editmut_golden.sh hashes both into tests/golden/editmut/; the Rust replay is
// tests/fix_edit1.rs (it applies the same edits through Board::move_item etc.).

#include "pcb_dump.h"

#include <cstdio>
#include <cstdlib>
#include <functional>
#include <vector>

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
#include <board_connected_item.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_generator.h>
#include <pcb_group.h>
#include <pcb_point.h>
#include <pcb_shape.h>
#include <pcb_text.h>
#include <pcb_track.h>
#include <zone.h>
#include <pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h>


static const VECTOR2I DELTA( 1234567, -2500000 );
static const VECTOR2I CENTRE( 12345600, 7890000 );
static const VECTOR2I NEWPOS( 3300000, 4400000 );
static const int      WIDTH = 333000;


/// Items without a Mirror override (the BOARD_ITEM base body is a message box).
static bool hasMirror( BOARD_ITEM* aItem )
{
    switch( aItem->Type() )
    {
    case PCB_FOOTPRINT_T:
    case PCB_BARCODE_T:
    case PCB_TARGET_T:
    case PCB_MARKER_T:
        return false;
    default:
        return true;
    }
}


static void setWidth( BOARD_ITEM* aItem )
{
    switch( aItem->Type() )
    {
    case PCB_TRACE_T:
    case PCB_ARC_T:
        static_cast<PCB_TRACK*>( aItem )->SetWidth( WIDTH );
        break;
    case PCB_VIA_T:
        static_cast<PCB_VIA*>( aItem )->SetWidth( F_Cu, WIDTH );
        break;
    case PCB_SHAPE_T:
        static_cast<PCB_SHAPE*>( aItem )->SetWidth( WIDTH );
        break;
    case PCB_PAD_T:
        static_cast<PAD*>( aItem )->SetSize( PADSTACK::ALL_LAYERS, VECTOR2I( 1100000, 700000 ) );
        break;
    default:
        break;
    }
}


static void setNet( BOARD_ITEM* aItem )
{
    if( BOARD_CONNECTED_ITEM* c = dynamic_cast<BOARD_CONNECTED_ITEM*>( aItem ) )
        c->SetNetCode( 1, true );
}


static void fpFlipLayer( FOOTPRINT* aFp )
{
    aFp->SetLayerAndFlip( aFp->GetLayer() == F_Cu ? B_Cu : F_Cu );
}


/// wxASSERT / wxFAIL do not throw (a throwing handler would abort an edit half-way): they are
/// counted and the edit continues, as in a release build without dialogs.
static int g_asserts = 0;


static void countAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
    g_asserts++;
}


typedef std::function<void( BOARD_ITEM* )> ITEM_OP;


struct EDIT
{
    const char* name;
    bool        children;   ///< apply to the children of the footprints instead
    ITEM_OP     op;
};


static std::vector<EDIT> edits()
{
    auto rot = []( double aDeg )
    {
        return [aDeg]( BOARD_ITEM* i ) { i->Rotate( CENTRE, EDA_ANGLE( aDeg, DEGREES_T ) ); };
    };
    auto flip = []( FLIP_DIRECTION aDir )
    {
        return [aDir]( BOARD_ITEM* i ) { i->Flip( CENTRE, aDir ); };
    };
    auto mirror = []( FLIP_DIRECTION aDir )
    {
        return [aDir]( BOARD_ITEM* i )
        {
            if( hasMirror( i ) )
                i->Mirror( CENTRE, aDir );
        };
    };
    auto layer = []( PCB_LAYER_ID aLayer )
    {
        // A via on a non-copper layer is not a board state: LAYER_RANGE throws on it.
        return [aLayer]( BOARD_ITEM* i )
        {
            if( i->Type() != PCB_VIA_T || IsCopperLayer( aLayer ) )
                i->SetLayer( aLayer );
        };
    };
    auto fp = []( std::function<void( FOOTPRINT* )> aFn )
    {
        return [aFn]( BOARD_ITEM* i )
        {
            if( i->Type() == PCB_FOOTPRINT_T )
                aFn( static_cast<FOOTPRINT*>( i ) );
        };
    };

    return {
        { "none", false, []( BOARD_ITEM* ) {} },
        { "move", false, []( BOARD_ITEM* i ) { i->Move( DELTA ); } },
        { "rot90", false, rot( 90.0 ) },
        { "rot180", false, rot( 180.0 ) },
        { "rot37_5", false, rot( 37.5 ) },
        { "flip_lr", false, flip( FLIP_DIRECTION::LEFT_RIGHT ) },
        { "flip_tb", false, flip( FLIP_DIRECTION::TOP_BOTTOM ) },
        { "mirror_lr", false, mirror( FLIP_DIRECTION::LEFT_RIGHT ) },
        { "mirror_tb", false, mirror( FLIP_DIRECTION::TOP_BOTTOM ) },
        { "layer_bcu", false, layer( B_Cu ) },
        { "layer_bsilk", false, layer( B_SilkS ) },
        { "setpos", false, []( BOARD_ITEM* i ) { i->SetPosition( NEWPOS ); } },
        { "width", false, setWidth },
        { "net1", false, setNet },
        { "fp_orient", false,
          fp( []( FOOTPRINT* f ) { f->SetOrientation( EDA_ANGLE( 123.4, DEGREES_T ) ); } ) },
        { "fp_pos", false, fp( []( FOOTPRINT* f ) { f->SetPosition( NEWPOS ); } ) },
        { "fp_layerflip", false, fp( fpFlipLayer ) },
        { "fp_scale", false, fp( []( FOOTPRINT* f ) { f->SetTransformScale( 1.25, 0.8 ); } ) },
        { "c_move", true, []( BOARD_ITEM* i ) { i->Move( DELTA ); } },
        { "c_rot37_5", true, rot( 37.5 ) },
        { "c_flip_lr", true, flip( FLIP_DIRECTION::LEFT_RIGHT ) },
        { "c_mirror_tb", true, mirror( FLIP_DIRECTION::TOP_BOTTOM ) },
        { "c_layer_bsilk", true, layer( B_SilkS ) },
        { "c_setpos", true, []( BOARD_ITEM* i ) { i->SetPosition( NEWPOS ); } },
        { "c_width", true, setWidth },
        { "c_net1", true, setNet },
    };
}


/// The walk the edits run over (the Rust replay walks the same way).
static std::vector<BOARD_ITEM*> walk( BOARD* aBoard, bool aChildren )
{
    std::vector<BOARD_ITEM*> items;

    if( aChildren )
    {
        for( FOOTPRINT* fp : aBoard->Footprints() )
        {
            for( PCB_FIELD* f : fp->GetFields() )
                items.push_back( f );

            for( PAD* p : fp->Pads() )
                items.push_back( p );

            for( BOARD_ITEM* d : fp->GraphicalItems() )
                items.push_back( d );

            for( ZONE* z : fp->Zones() )
                items.push_back( z );

            for( PCB_GROUP* g : fp->Groups() )
                items.push_back( g );

            for( PCB_POINT* p : fp->Points() )
                items.push_back( p );
        }

        return items;
    }

    for( FOOTPRINT* fp : aBoard->Footprints() )
        items.push_back( fp );

    for( BOARD_ITEM* d : aBoard->Drawings() )
        items.push_back( d );

    for( PCB_TRACK* t : aBoard->Tracks() )
        items.push_back( t );

    for( ZONE* z : aBoard->Zones() )
        items.push_back( z );

    for( PCB_POINT* p : aBoard->Points() )
        items.push_back( p );

    for( PCB_GENERATOR* g : aBoard->Generators() )
        items.push_back( g );

    for( PCB_GROUP* g : aBoard->Groups() )
        items.push_back( g );

    return items;
}


int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 3 )
    {
        fprintf( stderr, "usage: editmut_oracle OUTDIR FILE...\n" );
        return 2;
    }

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
    wxSetAssertHandler( &countAssert );
    KIID::SeedGenerator( 1 );

    std::string outDir = argv[1];

    for( int a = 2; a < argc; a++ )
    {
        std::string path = argv[a];
        std::string base = wxFileName( wxString::FromUTF8( path ) ).GetFullName().ToStdString();

        for( const EDIT& edit : edits() )
        {
            // Every load draws the UUIDs of the items without one from the same sequence: the
            // Rust board's generator starts from the mt19937 default seed on every parse.
            KIID::SeedGenerator( 5489 );
            std::unique_ptr<BOARD> board = KI_TEST::ReadBoardFromFileOrStream( path );

            if( !board )
                return 1;

            g_asserts = 0;

            int throws = 0;

            for( BOARD_ITEM* item : walk( board.get(), edit.children ) )
            {
                try
                {
                    edit.op( item );
                }
                catch( const std::exception& e )
                {
                    fprintf( stderr, "%s %s %s: %s\n", edit.name, base.c_str(),
                             item->GetClass().ToStdString().c_str(), e.what() );
                    throws++;
                }
            }

            fflush( stdout );
            printf( "@@|%s|%s|asserts=%d,throws=%d\n", edit.name, base.c_str(), g_asserts,
                    throws );
            ScanFileUuids( path );
            DUMP_OPTIONS opts;
            DumpBoard( board.get(), opts );
            fflush( stdout );

            PCB_IO_KICAD_SEXPR io;
            io.SaveBoard( wxString::FromUTF8( outDir + "/" + edit.name + "__" + base ), *board );
        }
    }

    fflush( stdout );
    _exit( 0 );
}
