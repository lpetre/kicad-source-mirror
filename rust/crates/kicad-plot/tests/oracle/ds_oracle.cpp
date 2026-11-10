// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port.  Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// Differential oracle of work package DS-1 (drawing sheet: DS_DATA_MODEL, the .kicad_wks
// parser, the default / empty sheets, DS_DRAW_ITEM_LIST::BuildDrawItemsList with the text
// expansion, PlotDrawingSheet) of the kicad-plot crate.  It reads tests/oracle/ds_cases.txt,
// runs every case on the real code of the reference build (common/libcommon.a) and prints
// the data model after each load, every draw item of each built list and the FNV-1a 64 hash
// of each plotted file (Gerber, SVG black and white, SVG colour).
//
//   ds_oracle <kicad root> <cases> [full]
//
// Without "full" the draw item dump of each RUN is printed as "B <lines> <fnv64>" and the
// plotted files as "H <mode> <size> <fnv64>" only.
//
// Golden output: tests/golden/drawing_sheet.txt (replayed by tests/test_drawing_sheet_oracle.rs).
// Doubles are printed as their IEEE bit patterns, texts escaped (\\, \n, \t, \r).

#include <cinttypes>
#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include <unistd.h>

#include <wx/image.h>

#include <base_units.h>
#include <bitmap_base.h>
#include <drawing_sheet/ds_data_item.h>
#include <drawing_sheet/ds_data_model.h>
#include <drawing_sheet/ds_draw_item.h>
#include <page_info.h>
#include <plotters/plotter_gerber.h>
#include <plotters/plotters_pslike.h>
#include <render_settings.h>
#include <string_utils.h>
#include <title_block.h>

class TEST_RENDER_SETTINGS : public KIGFX::RENDER_SETTINGS
{
public:
    COLOR4D GetColor( const KIGFX::VIEW_ITEM*, int ) const override { return m_c; }
    const COLOR4D& GetBackgroundColor() const override { return m_c; }
    void SetBackgroundColor( const COLOR4D& ) override {}
    const COLOR4D& GetGridColor() override { return m_c; }
    const COLOR4D& GetCursorColor() override { return m_c; }
    COLOR4D m_c;
};

static uint64_t fnv( const std::string& s, uint64_t h = 1469598103934665603ULL )
{
    for( unsigned char c : s )
    {
        h ^= c;
        h *= 1099511628211ULL;
    }

    return h;
}

static std::string hx( double d )
{
    uint64_t u;
    memcpy( &u, &d, 8 );
    char buf[32];
    snprintf( buf, sizeof buf, "%016" PRIx64, u );
    return buf;
}

static std::string esc( const wxString& w )
{
    std::string s = w.ToStdString( wxConvUTF8 );
    std::string o;

    for( char c : s )
    {
        if( c == '\\' )
            o += "\\\\";
        else if( c == '\n' )
            o += "\\n";
        else if( c == '\t' )
            o += "\\t";
        else if( c == '\r' )
            o += "\\r";
        else
            o += c;
    }

    return o;
}

static std::string pt( const VECTOR2I& p )
{
    return std::to_string( p.x ) + "," + std::to_string( p.y );
}

static std::string box( const BOX2I& b )
{
    return pt( b.GetOrigin() ) + "," + std::to_string( b.GetWidth() ) + ","
           + std::to_string( b.GetHeight() );
}

static std::string col( const COLOR4D& c )
{
    return hx( c.r ) + "," + hx( c.g ) + "," + hx( c.b ) + "," + hx( c.a );
}

static std::string g_block;

// printf into the block of the current RUN
static void outf( const char* fmt, ... ) __attribute__( ( format( printf, 1, 2 ) ) );

static void outf( const char* fmt, ... )
{
    char    buf[65536];
    va_list ap;
    va_start( ap, fmt );
    vsnprintf( buf, sizeof buf, fmt, ap );
    va_end( ap );
    g_block += buf;
}

static void flushBlock( bool full )
{
    if( full )
    {
        printf( "%s", g_block.c_str() );
    }
    else
    {
        size_t n = 0;

        for( char c : g_block )
            n += c == '\n';

        printf( "B %zu %016" PRIx64 "\n", n, fnv( g_block ) );
    }

    g_block.clear();
}

static std::vector<std::string> split( const std::string& s, char sep )
{
    std::vector<std::string> out;
    std::string              cur;

    for( char c : s )
    {
        if( c == sep )
        {
            out.push_back( cur );
            cur.clear();
        }
        else
        {
            cur += c;
        }
    }

    out.push_back( cur );
    return out;
}

static void dumpModel( DS_DATA_MODEL& model )
{
    printf( "MODEL n=%u ver=%d gen=%s void=%d m=%s,%s,%s,%s lw=%s ts=%s,%s tt=%s\n", model.GetCount(),
            model.GetFileFormatVersionAtLoad(), esc( model.GetGenerator() ).c_str(),
            (int) model.VoidListAllowed(), hx( model.GetLeftMargin() ).c_str(),
            hx( model.GetRightMargin() ).c_str(), hx( model.GetTopMargin() ).c_str(),
            hx( model.GetBottomMargin() ).c_str(), hx( model.m_DefaultLineWidth ).c_str(),
            hx( model.m_DefaultTextSize.x ).c_str(), hx( model.m_DefaultTextSize.y ).c_str(),
            hx( model.m_DefaultTextThickness ).c_str() );

    for( unsigned ii = 0; ii < model.GetCount(); ii++ )
    {
        DS_DATA_ITEM* it = model.GetItem( ii );
        printf( "D %u t=%d name=%s info=%s pos=%s,%s,%d end=%s,%s,%d lw=%s rep=%d inc=%s,%s il=%d opt=%d",
                ii, (int) it->GetType(), esc( it->m_Name ).c_str(), esc( it->m_Info ).c_str(),
                hx( it->m_Pos.m_Pos.x ).c_str(), hx( it->m_Pos.m_Pos.y ).c_str(),
                it->m_Pos.m_Anchor, hx( it->m_End.m_Pos.x ).c_str(),
                hx( it->m_End.m_Pos.y ).c_str(), it->m_End.m_Anchor, hx( it->m_LineWidth ).c_str(),
                it->m_RepeatCount, hx( it->m_IncrementVector.x ).c_str(),
                hx( it->m_IncrementVector.y ).c_str(), it->m_IncrementLabel,
                (int) it->GetPage1Option() );

        if( it->GetType() == DS_DATA_ITEM::DS_TEXT )
        {
            DS_DATA_ITEM_TEXT* t = static_cast<DS_DATA_ITEM_TEXT*>( it );
            printf( " tb=%s or=%s hj=%d vj=%d it=%d bd=%d face=%s sz=%s,%s c=%s bb=%s,%s",
                    esc( t->m_TextBase ).c_str(), hx( t->m_Orient ).c_str(), (int) t->m_Hjustify,
                    (int) t->m_Vjustify, (int) t->m_Italic, (int) t->m_Bold,
                    t->m_Font ? esc( t->m_Font->GetName() ).c_str() : "-",
                    hx( t->m_TextSize.x ).c_str(), hx( t->m_TextSize.y ).c_str(),
                    col( t->m_TextColor ).c_str(), hx( t->m_BoundingBoxSize.x ).c_str(),
                    hx( t->m_BoundingBoxSize.y ).c_str() );
        }
        else if( it->GetType() == DS_DATA_ITEM::DS_POLYPOLYGON )
        {
            DS_DATA_ITEM_POLYGONS* p = static_cast<DS_DATA_ITEM_POLYGONS*>( it );
            std::string            corners;

            for( const VECTOR2D& c : p->m_Corners )
                corners += hx( c.x ) + hx( c.y );

            printf( " or=%s nc=%zu pc=%d ch=%016" PRIx64, hx( p->m_Orient.AsDegrees() ).c_str(),
                    p->m_Corners.size(), p->GetPolyCount(), fnv( corners ) );

            for( int kk = 0; kk < p->GetPolyCount(); kk++ )
                printf( " %u-%u", p->GetPolyIndexStart( kk ), p->GetPolyIndexEnd( kk ) );
        }
        else if( it->GetType() == DS_DATA_ITEM::DS_BITMAP )
        {
            DS_DATA_ITEM_BITMAP* b = static_cast<DS_DATA_ITEM_BITMAP*>( it );
            printf( " sc=%s ppi=%d px=%s", hx( b->m_ImageBitmap->GetScale() ).c_str(),
                    b->m_ImageBitmap->GetPPI(), pt( b->m_ImageBitmap->GetSizePixels() ).c_str() );
        }

        printf( "\n" );
    }
}

static int itemIndex( DS_DATA_MODEL& model, DS_DATA_ITEM* peer )
{
    for( unsigned ii = 0; ii < model.GetCount(); ii++ )
    {
        if( model.GetItem( ii ) == peer )
            return ii;
    }

    return -1;
}

static void dumpList( DS_DATA_MODEL& model, DS_DRAW_ITEM_LIST& list )
{
    int k = 0;

    for( DS_DRAW_ITEM_BASE* item = list.GetFirst(); item; item = list.GetNext(), k++ )
    {
        outf( "I %d p=%d i=%d", k, itemIndex( model, item->GetPeer() ), item->GetIndexInPeer() );

        switch( item->Type() )
        {
        case WSG_LINE_T:
        case WSG_RECT_T:
        {
            DS_DRAW_ITEM_BASE* s = item;
            VECTOR2I           a, b;

            if( item->Type() == WSG_LINE_T )
            {
                a = static_cast<DS_DRAW_ITEM_LINE*>( s )->GetStart();
                b = static_cast<DS_DRAW_ITEM_LINE*>( s )->GetEnd();
            }
            else
            {
                a = static_cast<DS_DRAW_ITEM_RECT*>( s )->GetStart();
                b = static_cast<DS_DRAW_ITEM_RECT*>( s )->GetEnd();
            }

            outf( " %s %s %s w=%d bb=%s", item->Type() == WSG_LINE_T ? "L" : "R", pt( a ).c_str(),
                    pt( b ).c_str(), item->GetPenWidth(), box( item->GetBoundingBox() ).c_str() );
            break;
        }

        case WSG_POLY_T:
        {
            DS_DRAW_ITEM_POLYPOLYGONS* p = static_cast<DS_DRAW_ITEM_POLYPOLYGONS*>( item );
            outf( " P %s w=%d bb=%s", pt( p->GetPosition() ).c_str(), p->GetPenWidth(),
                    box( p->GetBoundingBox() ).c_str() );

            for( int idx = 0; idx < p->GetPolygons().OutlineCount(); ++idx )
            {
                const SHAPE_LINE_CHAIN& o = p->GetPolygons().COutline( idx );
                outf( " |" );

                for( int ii = 0; ii < o.PointCount(); ii++ )
                    outf( " %s", pt( o.CPoint( ii ) ).c_str() );
            }

            break;
        }

        case WSG_TEXT_T:
        {
            DS_DRAW_ITEM_TEXT* t = static_cast<DS_DRAW_ITEM_TEXT*>( item );
            outf( " T txt=%s shown=%s pos=%s sz=%s th=%d an=%s hj=%d vj=%d bd=%d it=%d ml=%d "
                    "c=%s ep=%d bb=%s",
                    esc( t->GetText() ).c_str(), esc( t->GetShownText( FOR_ERC_DRC ) ).c_str(),
                    pt( t->GetTextPos() ).c_str(), pt( t->GetTextSize() ).c_str(),
                    t->GetTextThickness(), hx( t->GetTextAngle().AsDegrees() ).c_str(),
                    (int) t->GetHorizJustify(), (int) t->GetVertJustify(), (int) t->IsBold(),
                    (int) t->IsItalic(), (int) t->IsMultilineAllowed(),
                    col( t->GetTextColor() ).c_str(), t->GetEffectiveTextPenWidth(),
                    box( t->GetBoundingBox() ).c_str() );

            if( t->IsMultilineAllowed() )
            {
                wxArrayString lines;
                wxStringSplit( t->GetShownText( FOR_CANVAS ), lines, '\n' );
                std::vector<VECTOR2I> positions;
                t->GetLinePositions( nullptr, positions, (int) lines.Count() );

                outf( " lp=" );

                for( const VECTOR2I& p : positions )
                    outf( "%s;", pt( p ).c_str() );
            }

            break;
        }

        case WSG_BITMAP_T:
            outf( " B %s bb=%s", pt( item->GetPosition() ).c_str(),
                    box( item->GetBoundingBox() ).c_str() );
            break;

        default:
            outf( " ?" );
        }

        outf( "\n" );
    }

    // The data items' state after the build
    for( unsigned ii = 0; ii < model.GetCount(); ii++ )
    {
        DS_DATA_ITEM* it = model.GetItem( ii );

        if( it->GetType() == DS_DATA_ITEM::DS_TEXT )
        {
            DS_DATA_ITEM_TEXT* t = static_cast<DS_DATA_ITEM_TEXT*>( it );
            outf( "X %u full=%s cts=%s,%s\n", ii, esc( t->m_FullText ).c_str(),
                    hx( t->m_ConstrainedTextSize.x ).c_str(),
                    hx( t->m_ConstrainedTextSize.y ).c_str() );
        }
    }
}

static std::string readAndNormalise( const std::string& path, bool isSvg )
{
    std::ifstream     in( path, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    std::string file = ss.str();
    remove( path.c_str() );

    if( isSvg )
    {
        const std::string marker = "<title>SVG Image created as ";
        size_t            p = file.find( marker );

        if( p != std::string::npos )
        {
            size_t e = file.find( '\n', p );
            file.replace( p, e - p, marker + "plot.svg date @DATE@ </title>" );
        }
    }
    else
    {
        const std::string marker = "G04 Created by KiCad (";
        size_t            p = file.find( marker );

        if( p != std::string::npos )
        {
            size_t e = file.find( '\n', p );
            file.replace( p, e - p, marker + "x @VERSION@) date @DATE@*" );
        }

        // %TF.CreationDate,...*%
        const std::string cd = "%TF.CreationDate,";
        p = file.find( cd );

        if( p != std::string::npos )
        {
            size_t e = file.find( '\n', p );
            file.replace( p, e - p, cd + "@DATE@*%" );
        }
    }

    return file;
}

int main( int argc, char** argv )
{
    if( argc < 3 )
    {
        fprintf( stderr, "usage: ds_oracle <kicad root> <cases> [full]\n" );
        return 2;
    }

    bool full = argc > 3 && std::string( argv[3] ) == "full";

    wxInitAllImageHandlers();

    std::string root = argv[1];
    std::ifstream in( argv[2] );
    std::string   line;

    // A fresh model per sheet (kicad-cli loads one sheet into its singleton per run)
    std::unique_ptr<DS_DATA_MODEL> modelPtr = std::make_unique<DS_DATA_MODEL>();
    DS_DATA_MODEL::SetAltInstance( modelPtr.get() );

    PAGE_INFO   page;
    TITLE_BLOCK tb;
    std::string tmp = "/tmp/ds_oracle_" + std::to_string( getpid() );

    while( std::getline( in, line ) )
    {
        if( line.empty() )
            continue;

        std::vector<std::string> f = split( line, '\t' );
        DS_DATA_MODEL&           model = *modelPtr;

        if( f[0] == "SHEET" )
        {
            printf( "SHEET %s\n", f[1].c_str() );
            modelPtr = std::make_unique<DS_DATA_MODEL>();
            DS_DATA_MODEL::SetAltInstance( modelPtr.get() );

            if( f[1] == "default" )
            {
                modelPtr->SetDefaultLayout();
                printf( "LOAD ok\n" );
            }
            else if( f[1] == "empty" )
            {
                modelPtr->SetEmptyLayout();
                printf( "LOAD ok\n" );
            }
            else
            {
                wxString msg;
                bool     ok = modelPtr->LoadDrawingSheet( wxString::FromUTF8( root + "/" + f[1] ), &msg );
                printf( "LOAD %s %s\n", ok ? "ok" : "err", esc( msg ).c_str() );
            }

            dumpModel( *modelPtr );
        }
        else if( f[0] == "PAGE" )
        {
            PAGE_INFO::SetCustomWidthMils( atof( f[3].c_str() ) );
            PAGE_INFO::SetCustomHeightMils( atof( f[4].c_str() ) );
            page = PAGE_INFO();
            page.SetType( wxString::FromUTF8( f[1] ), atoi( f[2].c_str() ) != 0 );
            printf( "PAGE %s %s %s,%s\n", f[1].c_str(), f[2].c_str(), hx( page.GetWidthMils() ).c_str(),
                    hx( page.GetHeightMils() ).c_str() );
        }
        else if( f[0] == "TB" )
        {
            tb = TITLE_BLOCK();
            tb.SetTitle( wxString::FromUTF8( f[1] ) );
            tb.SetDate( wxString::FromUTF8( f[2] ) );
            tb.SetRevision( wxString::FromUTF8( f[3] ) );
            tb.SetCompany( wxString::FromUTF8( f[4] ) );

            for( int c = 0; c < 9; c++ )
                tb.SetComment( c, wxString::FromUTF8( f[5 + c] ) );
        }
        else if( f[0] == "RUN" )
        {
            const std::string& mode = f[1];
            printf( "RUN %s\n", line.substr( 4 ).c_str() );

            if( mode == "drc" )
            {
                DS_DRAW_ITEM_LIST drawItems( pcbIUScale, FOR_ERC_DRC );
                drawItems.SetPageNumber( wxT( "1" ) );
                drawItems.SetSheetCount( 1 );
                drawItems.SetFileName( wxT( "dummyFilename" ) );
                drawItems.SetSheetName( wxT( "dummySheet" ) );
                drawItems.SetSheetLayer( wxT( "dummyLayer" ) );
                drawItems.SetProject( nullptr );
                drawItems.BuildDrawItemsList( page, tb );
                dumpList( model, drawItems );
                flushBlock( full );
                continue;
            }

            bool     first = atoi( f[2].c_str() ) != 0;
            wxString sheetNum = wxString::FromUTF8( f[3] );
            int      count = atoi( f[4].c_str() );
            wxString layer = wxString::FromUTF8( f[5] );
            wxString sheetName = wxString::FromUTF8( f[6] );
            wxString sheetPath = wxString::FromUTF8( f[7] );
            wxString fileName = wxString::FromUTF8( f[8] );

            if( mode == "list" )
            {
                // As PlotDrawingSheet builds it for a plotter with 2540 IU per decimil
                DS_DRAW_ITEM_LIST drawList( unityScale, FOR_CANVAS );
                drawList.SetDefaultPenSize( pcbIUScale.mmToIU( 0.0212 ) );
                drawList.SetPlotterMilsToIUfactor( 2540.0 * 10.0 );
                drawList.SetPageNumber( sheetNum );
                drawList.SetSheetCount( count );
                drawList.SetFileName( fileName );
                drawList.SetSheetName( sheetName );
                drawList.SetSheetPath( sheetPath );
                drawList.SetSheetLayer( layer );
                drawList.SetProject( nullptr );
                drawList.SetIsFirstPage( first );
                drawList.BuildDrawItemsList( page, tb );
                dumpList( model, drawList );
                flushBlock( full );
                continue;
            }

            bool isSvg = mode != "gbr";
            TEST_RENDER_SETTINGS rs;
            rs.SetDefaultPenWidth( pcbIUScale.mmToIU( 0.0212 ) );
            rs.SetLayerName( layer );

            std::unique_ptr<PLOTTER> plotter;

            if( isSvg )
                plotter = std::make_unique<SVG_PLOTTER>();
            else
                plotter = std::make_unique<GERBER_PLOTTER>();

            plotter->SetRenderSettings( &rs );
            plotter->SetPageSettings( page );
            plotter->SetColorMode( mode == "svgc" );
            plotter->SetViewport( VECTOR2I( 0, 0 ), pcbIUScale.IU_PER_MILS / 10, 1.0, false );

            if( !isSvg )
            {
                GERBER_PLOTTER* gbr = static_cast<GERBER_PLOTTER*>( plotter.get() );
                gbr->SetGerberCoordinatesFormat( 6 );
                gbr->UseX2format( true );
                gbr->SetCreator( wxT( "x" ) );
            }

            std::string path = tmp + ( isSvg ? ".svg" : ".gbr" );
            plotter->OpenFile( wxString::FromUTF8( path ) );
            plotter->StartPlot( wxT( "1" ) );
            PlotDrawingSheet( plotter.get(), nullptr, tb, page, nullptr, sheetNum, count, sheetName,
                              sheetPath, fileName, COLOR4D( 0.2, 0.4, 0.6, 1.0 ), first, wxEmptyString,
                              wxEmptyString );
            plotter->EndPlot();
            plotter.reset();

            std::string file = readAndNormalise( path, isSvg );
            printf( "H %s %zu %016" PRIx64 "\n", mode.c_str(), file.size(), fnv( file ) );

            if( full )
                printf( "%s\nF-END\n", file.c_str() );
        }
    }

    DS_DATA_MODEL::SetAltInstance( nullptr );
    return 0;
}
