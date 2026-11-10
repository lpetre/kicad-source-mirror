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


// Integration step 2 (INT-2a) differential oracle for the kernel changes that alter behaviour:
//  * the SetNet virtual (ZONE copper-thieving override) and BOARD::Remove( net );
//  * the per-layer LAYER_T in mutators: BOARD_ITEM::IsSideSpecific() in Flip / Mirror and
//    BOARD::IsBackLayer() in StyleFromSettings, on user layers typed LT_FRONT / LT_BACK;
//  * StyleFromSettings with the parent frame (footprint at (10 mm, 20 mm), rotated 30 degrees,
//    scaled (1.5, 0.75)): text, text box, field;
//  * PCB_DRILL_MAP::GetSymbolExtent and BOARD_DESIGN_SETTINGS::operator==.
// One "case|field=value" line per value. Built and run by int2a_golden.sh; the Rust replay is
// integration2_tests::int2a_matches_oracle (feature `dump`).

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
#include <board_design_settings.h>
#include <footprint.h>
#include <netinfo.h>
#include <pcb_dimension.h>
#include <pcb_drill_map.h>
#include <pcb_field.h>
#include <pcb_text.h>
#include <pcb_textbox.h>
#include <pcb_track.h>
#include <zone.h>


static const int MM = 1000000;


static void P( const std::string& aCase, const std::string& aField, const std::string& aValue )
{
    printf( "%s|%s=%s\n", aCase.c_str(), aField.c_str(), aValue.c_str() );
}


static void Setup( BOARD& aBoard )
{
    P( "setup", "set_user1_front", V( aBoard.SetLayerType( User_1, LT_FRONT ) ) );
    P( "setup", "set_user2_back", V( aBoard.SetLayerType( User_2, LT_BACK ) ) );

    BOARD_DESIGN_SETTINGS& bds = aBoard.GetDesignSettings();
    bds.m_TextSize[LAYER_CLASS_SILK] = VECTOR2I( 1300000, 1100000 );
    bds.m_TextThickness[LAYER_CLASS_SILK] = 170000;
    bds.m_TextItalic[LAYER_CLASS_SILK] = true;
    bds.m_TextUpright[LAYER_CLASS_SILK] = true;
    bds.m_LineThickness[LAYER_CLASS_SILK] = 230000;
    bds.m_TextSize[LAYER_CLASS_OTHERS] = VECTOR2I( 900000, 1700000 );
    bds.m_TextThickness[LAYER_CLASS_OTHERS] = 210000;
    bds.m_TextItalic[LAYER_CLASS_OTHERS] = false;
    bds.m_TextUpright[LAYER_CLASS_OTHERS] = true;
    bds.m_LineThickness[LAYER_CLASS_OTHERS] = 110000;
    bds.m_TextSize[LAYER_CLASS_COPPER] = VECTOR2I( 1500000, 1400000 );
    bds.m_TextThickness[LAYER_CLASS_COPPER] = 250000;
    bds.m_LineThickness[LAYER_CLASS_COPPER] = 190000;
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


static const VECTOR2I CENTRE( 5 * MM, 5 * MM );
static const PCB_LAYER_ID LAYERS[] = { F_SilkS, B_SilkS, User_1, User_2, User_3, Dwgs_User, F_Cu, B_Cu };


static void InitText( PCB_TEXT& aText, PCB_LAYER_ID aLayer )
{
    aText.SetLayer( aLayer );
    aText.SetText( wxT( "abc" ) );
    aText.SetTextPos( VECTOR2I( 3 * MM, 4 * MM ) );
    aText.SetTextAngle( EDA_ANGLE( 30.0, DEGREES_T ) );
}


static void InitTextbox( PCB_TEXTBOX& aBox, PCB_LAYER_ID aLayer, const VECTOR2I& aOrigin )
{
    aBox.SetLayer( aLayer );
    aBox.SetStart( aOrigin );
    aBox.SetEnd( aOrigin + VECTOR2I( 10 * MM, 5 * MM ) );
    aBox.SetText( wxT( "abc" ) );
}


static void TextStyle( const std::string& aCase, const std::string& aPrefix, const PCB_TEXT& aText )
{
    P( aCase, aPrefix + "mirrored", V( aText.IsMirrored() ) );
    P( aCase, aPrefix + "size", V( aText.GetTextSize() ) );
    P( aCase, aPrefix + "lib_size", V( aText.EDA_TEXT::GetTextSize() ) );
    P( aCase, aPrefix + "thickness", V( aText.GetTextThickness() ) );
    P( aCase, aPrefix + "lib_thickness", V( aText.EDA_TEXT::GetTextThickness() ) );
    P( aCase, aPrefix + "italic", V( aText.IsItalic() ) );
    P( aCase, aPrefix + "keep_upright", V( aText.IsKeepUpright() ) );
}


static void BoxStyle( const std::string& aCase, const std::string& aPrefix, const PCB_TEXTBOX& aBox )
{
    P( aCase, aPrefix + "mirrored", V( aBox.IsMirrored() ) );
    P( aCase, aPrefix + "size", V( aBox.GetTextSize() ) );
    P( aCase, aPrefix + "lib_size", V( aBox.EDA_TEXT::GetTextSize() ) );
    P( aCase, aPrefix + "thickness", V( aBox.GetTextThickness() ) );
    P( aCase, aPrefix + "lib_thickness", V( aBox.EDA_TEXT::GetTextThickness() ) );
    P( aCase, aPrefix + "italic", V( aBox.IsItalic() ) );
    P( aCase, aPrefix + "keep_upright", V( aBox.IsKeepUpright() ) );
    P( aCase, aPrefix + "stroke_width", V( aBox.GetStroke().GetWidth() ) );
    P( aCase, aPrefix + "width", V( aBox.GetWidth() ) );
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

    // ---- layer types in mutators, board level -------------------------------------------
    {
        BOARD board;
        Setup( board );
        const BOARD_DESIGN_SETTINGS& bds = board.GetDesignSettings();

        for( PCB_LAYER_ID layer : LAYERS )
        {
            std::string n = std::to_string( (int) layer );

            P( "layer_" + n, "type", V( (int) board.GetLayerType( layer ) ) );
            P( "layer_" + n, "is_back", V( board.IsBackLayer( layer ) ) );
            P( "layer_" + n, "flipped", V( (int) board.FlipLayer( layer ) ) );

            {
                std::string c = "text_" + n;
                PCB_TEXT    t( &board );
                InitText( t, layer );
                P( c, "side_specific", V( t.IsSideSpecific() ) );
                t.Flip( CENTRE, FLIP_DIRECTION::LEFT_RIGHT );
                P( c, "flip.mirrored", V( t.IsMirrored() ) );
                P( c, "flip.layer", V( (int) t.GetLayer() ) );
                P( c, "flip.pos", V( t.GetTextPos() ) );
                P( c, "flip.angle", V( t.GetTextAngle() ) );

                PCB_TEXT s( &board );
                InitText( s, layer );
                s.StyleFromSettings( bds, true );
                TextStyle( c, "style.", s );

                PCB_TEXT u( &board );
                InitText( u, layer );
                u.SetMirrored( true );
                u.StyleFromSettings( bds, false );
                P( c, "style_nocheck.mirrored", V( u.IsMirrored() ) );
            }

            {
                std::string c = "textbox_" + n;
                PCB_TEXTBOX t( &board );
                InitTextbox( t, layer, VECTOR2I( 2 * MM, 3 * MM ) );
                P( c, "side_specific", V( t.IsSideSpecific() ) );
                t.Flip( CENTRE, FLIP_DIRECTION::LEFT_RIGHT );
                P( c, "flip.mirrored", V( t.IsMirrored() ) );
                P( c, "flip.layer", V( (int) t.GetLayer() ) );
                P( c, "flip.start", V( t.GetStart() ) );
                P( c, "flip.end", V( t.GetEnd() ) );

                PCB_TEXTBOX s( &board );
                InitTextbox( s, layer, VECTOR2I( 2 * MM, 3 * MM ) );
                s.StyleFromSettings( bds, true );
                BoxStyle( c, "style.", s );
            }

            {
                std::string     c = "dim_" + n;
                PCB_DIM_ALIGNED d( &board );
                d.SetLayer( layer );
                P( c, "side_specific", V( d.IsSideSpecific() ) );
                d.Mirror( CENTRE, FLIP_DIRECTION::LEFT_RIGHT );
                P( c, "mirror.mirrored", V( d.IsMirrored() ) );

                PCB_DIM_ALIGNED s( &board );
                s.SetLayer( layer );
                s.StyleFromSettings( bds, true );
                P( c, "style.mirrored", V( s.IsMirrored() ) );
                P( c, "style.size", V( s.GetTextSize() ) );
                P( c, "style.thickness", V( s.GetTextThickness() ) );
                P( c, "style.line_thickness", V( s.GetLineThickness() ) );
            }
        }
    }

    // ---- StyleFromSettings in a scaled, rotated footprint --------------------------------
    {
        BOARD board;
        Setup( board );
        const BOARD_DESIGN_SETTINGS& bds = board.GetDesignSettings();
        FOOTPRINT*                   fp = MakeFootprint( board );

        for( PCB_LAYER_ID layer : { F_SilkS, User_2 } )
        {
            std::string n = std::to_string( (int) layer );

            PCB_TEXT t( fp );
            InitText( t, layer );
            t.StyleFromSettings( bds, true );
            TextStyle( "fp_text_" + n, "style.", t );

            PCB_TEXTBOX b( fp );
            InitTextbox( b, layer, VECTOR2I( 11 * MM, 21 * MM ) );
            b.StyleFromSettings( bds, true );
            BoxStyle( "fp_textbox_" + n, "style.", b );
        }

        PCB_FIELD& ref = fp->Reference();
        ref.StyleFromSettings( bds, true );
        TextStyle( "fp_reference", "style.", ref );
        P( "fp_reference", "layer", V( (int) ref.GetLayer() ) );
    }

    // ---- SetNet / SetNetCode virtuals, BOARD::Remove( net ) -------------------------------
    {
        BOARD         board;
        NETINFO_ITEM* net = new NETINFO_ITEM( &board, wxT( "TestNet" ), 1 );
        board.Add( net );

        ZONE* thieving = new ZONE( &board );
        thieving->SetLayerSet( LSET( { F_Cu } ) );
        thieving->SetFillMode( ZONE_FILL_MODE::COPPER_THIEVING );
        board.Add( thieving );

        ZONE* solid = new ZONE( &board );
        solid->SetLayerSet( LSET( { F_Cu } ) );
        board.Add( solid );

        PCB_TRACK* track = new PCB_TRACK( &board );
        track->SetLayer( F_Cu );
        board.Add( track );

        BOARD_CONNECTED_ITEM* base = thieving;
        P( "nets", "thieving.initial", V( thieving->GetNetCode() ) );
        P( "nets", "thieving.set_net_code.ok", V( base->SetNetCode( 1, true ) ) );
        P( "nets", "thieving.set_net_code", V( thieving->GetNetCode() ) );
        base->SetNet( net );
        P( "nets", "thieving.set_net", V( thieving->GetNetCode() ) );

        base = solid;
        base->SetNet( net );
        P( "nets", "solid.set_net", V( solid->GetNetCode() ) );
        P( "nets", "track.set_net_code.ok", V( track->SetNetCode( 1, true ) ) );
        P( "nets", "track.set_net_code", V( track->GetNetCode() ) );
        P( "nets", "track.set_unknown.ok", V( track->SetNetCode( 7, true ) ) );
        P( "nets", "track.set_unknown", V( track->GetNetCode() ) );
        track->SetNetCode( 1, true );

        board.Remove( net );
        P( "nets", "removed.thieving", V( thieving->GetNetCode() ) );
        P( "nets", "removed.solid", V( solid->GetNetCode() ) );
        P( "nets", "removed.track", V( track->GetNetCode() ) );
        delete net;
    }

    // ---- PCB_DRILL_MAP::GetSymbolExtent ---------------------------------------------------
    {
        BOARD         board;
        PCB_DRILL_MAP map( &board );
        P( "drill_map", "extent", V( map.GetSymbolExtent() ) );
        map.SetSymbolSize( 700000 );
        P( "drill_map", "extent_700", V( map.GetSymbolExtent() ) );

        PCB_DRILL_MAP orphan( nullptr );
        P( "drill_map", "extent_no_board", V( orphan.GetSymbolExtent() ) );
    }

    // ---- BOARD_DESIGN_SETTINGS::operator== ------------------------------------------------
    {
        BOARD a;
        P( "bds_eq", "defaults", V( a.GetDesignSettings() == BOARD().GetDesignSettings() ) );

        auto differs = [&]( const char* aName, std::function<void( BOARD_DESIGN_SETTINGS& )> aEdit )
        {
            BOARD b;
            aEdit( b.GetDesignSettings() );
            P( "bds_eq", aName, V( a.GetDesignSettings() == b.GetDesignSettings() ) );
        };

        differs( "track_width_list", []( BOARD_DESIGN_SETTINGS& s ) { s.m_TrackWidthList.push_back( 250000 ); } );
        differs( "text_size", []( BOARD_DESIGN_SETTINGS& s ) { s.m_TextSize[LAYER_CLASS_FAB].x += 1; } );
        differs( "edge_plating",
                 []( BOARD_DESIGN_SETTINGS& s ) { s.GetStackupDescriptor().m_EdgePlating = true; } );
        differs( "paste_ratio", []( BOARD_DESIGN_SETTINGS& s ) { s.m_SolderPasteMarginRatio = -0.1; } );
        differs( "max_error", []( BOARD_DESIGN_SETTINGS& s ) { s.m_MaxError += 1; } );
        differs( "style_fp_text", []( BOARD_DESIGN_SETTINGS& s ) { s.m_StyleFPText = !s.m_StyleFPText; } );
        differs( "copper_layer_count", []( BOARD_DESIGN_SETTINGS& s ) { s.SetCopperLayerCount( 4 ); } );
        differs( "teardrops",
                 []( BOARD_DESIGN_SETTINGS& s )
                 {
                     TEARDROP_PARAMETERS* p = s.m_TeardropParamsList.GetParameters( TARGET_ROUND );
                     p->m_Enabled = !p->m_Enabled;
                 } );
    }

    return 0;
}
