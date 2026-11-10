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
 * drc-report-oracle: differential oracle of kicad-drc work package DRC-11 (markers, severities,
 * exclusions, report writers), linked against the real pcbnew code of the reference build.
 *
 *     drc-report-oracle SCENARIO...
 *
 * A scenario is a text file of tab-separated commands, executed in order (paths relative to the
 * KiCad source root, which is the first argument after --root, default: the current directory):
 *
 *     board      PATH                                    parse a board (parser only)
 *     severity   KEY  error|warning|ignore               bds.m_DRCSeverities[ code of KEY ]
 *     exclusion  json  JSON                              from_json -> inserted if the key is set
 *     exclusion  legacy  DATA  COMMENT                   DRC_EXCLUSION::FromLegacyStrings
 *     marker     KEY  X  Y  LAYER  IDS  MSG  RULE  RSEV  EXCL  COMMENT
 *                LAYER: a PCB_LAYER_ID number or DS (LAYER_DRAWINGSHEET);
 *                IDS: comma separated KIID strings or "-"; MSG: "-", "D:detail" (SetErrorDetail)
 *                or "M:message" (SetErrorMessage); RULE: "-" or a rule name; RSEV: rule severity
 *                (undefined|error|warning|ignore|exclusion); EXCL: 0/1 (SetExcluded with COMMENT)
 *     resolve    0|1                                     BOARD::ResolveDRCExclusions( flag ),
 *                                                        created markers are added to the board
 *     record                                             BOARD::RecordDRCExclusions()
 *     bds                                                the design settings' exclusions
 *     report     mm|in|mils  MASK                        both report writers, three providers
 *     legacy     DATA                                    PCB_MARKER::FromLegacyString
 *     strip      TEXT                                    HYPERLINK_DV_RENDERER::StripMarkup
 *
 * Output: one block per scenario, see dumpMarkers() / the command implementations. Dates in the
 * reports are replaced by <DATE>. The Rust twin is tests/report_oracle.rs.
 */

#include <cstdio>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/filename.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <pcbnew_settings.h>
#include <qa_utils/mock_utils.h>
#include <pcbnew_utils/board_file_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <build_version.h>
#include <pcb_marker.h>
#include <drc/drc_item.h>
#include <drc/drc_rule.h>
#include <drc/drc_report.h>
#include <drc/drc_exclusion.h>
#include <widgets/wx_data_view_hyperlink_renderer.h>
#include <widgets/ui_common.h>
#include <api/board/board_rules.pb.h>
#include <json_common.h>


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


static std::vector<std::string> split( const std::string& aLine, char aSep )
{
    std::vector<std::string> out;
    std::string              cur;

    for( char c : aLine )
    {
        if( c == aSep )
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


static std::string hex( const std::string& aBytes )
{
    static const char* digits = "0123456789abcdef";
    std::string        out;

    for( unsigned char c : aBytes )
    {
        out += digits[c >> 4];
        out += digits[c & 15];
    }

    return out.empty() ? "-" : out;
}


/// FNV-1a 64 of a sort key (marker and settings lines; exclusion lines print the full bytes).
static std::string fnv( const std::string& aBytes )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aBytes )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    char buf[20];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) h );
    return buf;
}


static std::string jsonOf( const DRC_EXCLUSION& aEx )
{
    nlohmann::json j;
    to_json( j, aEx );
    return j.dump();
}


static std::string severityName( SEVERITY aSev )
{
    switch( aSev )
    {
    case RPT_SEVERITY_UNDEFINED: return "undefined";
    case RPT_SEVERITY_INFO:      return "info";
    case RPT_SEVERITY_EXCLUSION: return "exclusion";
    case RPT_SEVERITY_ACTION:    return "action";
    case RPT_SEVERITY_WARNING:   return "warning";
    case RPT_SEVERITY_ERROR:     return "error";
    case RPT_SEVERITY_IGNORE:    return "ignore";
    case RPT_SEVERITY_DEBUG:     return "debug";
    }

    return "?";
}


static SEVERITY severityFromName( const std::string& aName )
{
    if( aName == "error" )     return RPT_SEVERITY_ERROR;
    if( aName == "warning" )   return RPT_SEVERITY_WARNING;
    if( aName == "ignore" )    return RPT_SEVERITY_IGNORE;
    if( aName == "exclusion" ) return RPT_SEVERITY_EXCLUSION;
    if( aName == "info" )      return RPT_SEVERITY_INFO;
    return RPT_SEVERITY_UNDEFINED;
}


static std::string idsOf( const std::shared_ptr<RC_ITEM>& aItem )
{
    std::string out;

    for( const KIID& id : aItem->GetIDs() )
    {
        if( !out.empty() )
            out += ",";

        out += id.AsStdString();
    }

    return out.empty() ? "-" : out;
}


/// One line per marker: index, code, key, position, layer, type, ids, excluded, comment,
/// severity, treated-as-excluded and the exclusion identity (sort key, JSON).
static void dumpMarker( const char* aTag, int aIndex, PCB_MARKER* aMarker, bool aOnBoard )
{
    std::shared_ptr<RC_ITEM> item = aMarker->GetRCItem();
    DRC_EXCLUSION            ex = DRC_EXCLUSION::FromMarker( *aMarker );

    printf( "%s|%d|code=%d|key=%s|pos=%d,%d|layer=%d|type=%d|ids=%s|excluded=%d|comment=%s",
            aTag, aIndex, item->GetErrorCode(), item->GetSettingsKey().ToStdString().c_str(),
            aMarker->GetPos().x, aMarker->GetPos().y, (int) aMarker->GetLayer(),
            (int) aMarker->GetMarkerType(), idsOf( item ).c_str(), aMarker->IsExcluded() ? 1 : 0,
            aMarker->GetComment().ToStdString().c_str() );

    if( aOnBoard )
    {
        printf( "|severity=%s|treated=%d", severityName( aMarker->GetSeverity() ).c_str(),
                aMarker->IsTreatedAsExcluded() ? 1 : 0 );
    }

    printf( "|key64=%s|json=%s\n", fnv( ex.GetSortKey() ).c_str(), jsonOf( ex ).c_str() );
}


static std::string readFile( const std::string& aPath )
{
    std::ifstream     in( aPath, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    return ss.str();
}


static void printBlock( const std::string& aTag, std::string aText, bool aJson )
{
    // Replace the creation date.
    if( aJson )
    {
        size_t p = aText.find( "\"date\": \"" );

        if( p != std::string::npos )
        {
            size_t s = p + 9;
            size_t e = aText.find( '"', s );
            aText.replace( s, e - s, "<DATE>" );
        }
    }
    else
    {
        size_t p = aText.find( "** Created on " );

        if( p != std::string::npos )
        {
            size_t s = p + 14;
            size_t e = aText.find( " **", s );
            aText.replace( s, e - s, "<DATE>" );
        }
    }

    std::vector<std::string> lines = split( aText, '\n' );

    // split() yields a last empty element after the final newline.
    if( !lines.empty() && lines.back().empty() )
        lines.pop_back();

    printf( "%s|lines=%zu\n", aTag.c_str(), lines.size() );

    for( const std::string& l : lines )
        printf( "> %s\n", l.c_str() );
}


static int runScenario( const std::string& aRoot, const std::string& aPath, const std::string& aTmp )
{
    std::ifstream                in( aPath );
    std::string                  line;
    std::unique_ptr<BOARD>       board;
    std::vector<DRC_RULE*>       rules;
    int                          lineNo = 0;

    printf( "scenario|%s\n", wxFileName( aPath ).GetFullName().ToStdString().c_str() );

    while( std::getline( in, line ) )
    {
        lineNo++;

        if( line.empty() || line[0] == '#' )
            continue;

        std::vector<std::string> f = split( line, '\t' );
        const std::string&       cmd = f[0];

        if( cmd == "board" )
        {
            std::string path = aRoot + "/" + f[1];
            board = KI_TEST::ReadBoardFromFileOrStream( path );

            if( !board )
            {
                printf( "error|%d|cannot load %s\n", lineNo, f[1].c_str() );
                return 1;
            }

            board->SetFileName( path );
            printf( "board|%s\n", wxFileName( path ).GetFullName().ToStdString().c_str() );
        }
        else if( cmd == "severity" )
        {
            std::shared_ptr<DRC_ITEM> item = DRC_ITEM::Create( wxString( f[1] ) );
            board->GetDesignSettings().m_DRCSeverities[item->GetErrorCode()] = severityFromName( f[2] );
        }
        else if( cmd == "exclusion" )
        {
            DRC_EXCLUSION ex;

            if( f[1] == "json" )
            {
                nlohmann::json j;

                try
                {
                    j = nlohmann::json::parse( f[2] );
                }
                catch( ... )
                {
                    printf( "error|%d|bad json\n", lineNo );
                    continue;
                }

                from_json( j, ex );
            }
            else
            {
                ex = DRC_EXCLUSION::FromLegacyStrings( wxString::FromUTF8( f[2] ),
                                                       wxString::FromUTF8( f.size() > 3 ? f[3] : "" ) );
            }

            printf( "exclusion|%d|sortkey=%s|comment=%s|json=%s\n", lineNo, hex( ex.GetSortKey() ).c_str(),
                    ex.GetComment().ToStdString().c_str(), jsonOf( ex ).c_str() );

            // BOARD_DESIGN_SETTINGS reader: only entries with a sort key
            if( !ex.GetSortKey().empty() )
                board->GetDesignSettings().m_DrcExclusions.insert( ex );
        }
        else if( cmd == "marker" )
        {
            std::shared_ptr<DRC_ITEM> item = DRC_ITEM::Create( wxString( f[1] ) );

            if( !item )
            {
                printf( "error|%d|unknown key %s\n", lineNo, f[1].c_str() );
                continue;
            }

            std::vector<KIID> ids;

            if( f[5] != "-" )
            {
                for( const std::string& s : split( f[5], ',' ) )
                    ids.emplace_back( s );
            }

            item->SetItems( ids );

            if( f[6].rfind( "D:", 0 ) == 0 )
                item->SetErrorDetail( wxString::FromUTF8( f[6].substr( 2 ) ) );
            else if( f[6].rfind( "M:", 0 ) == 0 )
                item->SetErrorMessage( wxString::FromUTF8( f[6].substr( 2 ) ) );

            if( f[7] != "-" )
            {
                DRC_RULE* rule = new DRC_RULE( wxString::FromUTF8( f[7] ) );
                rule->m_Severity = severityFromName( f[8] );
                rules.push_back( rule );
                item->SetViolatingRule( rule );
            }

            int         layer = f[4] == "DS" ? (int) LAYER_DRAWINGSHEET : std::stoi( f[4] );
            PCB_MARKER* marker = new PCB_MARKER( item, VECTOR2I( std::stoi( f[2] ), std::stoi( f[3] ) ),
                                                 layer );

            if( f[9] == "1" )
                marker->SetExcluded( true, wxString::FromUTF8( f.size() > 10 ? f[10] : "" ) );

            board->Add( marker );
        }
        else if( cmd == "resolve" )
        {
            std::vector<PCB_MARKER*> created = board->ResolveDRCExclusions( f[1] == "1" );
            int                      i = 0;

            for( PCB_MARKER* marker : created )
                dumpMarker( "created", i++, marker, false );

            for( PCB_MARKER* marker : created )
                board->Add( marker );
        }
        else if( cmd == "record" )
        {
            board->RecordDRCExclusions();
        }
        else if( cmd == "bds" )
        {
            int i = 0;

            for( const DRC_EXCLUSION& ex : board->GetDesignSettings().m_DrcExclusions )
            {
                printf( "bds|%d|key64=%s|comment=%s|json=%s\n", i++, fnv( ex.GetSortKey() ).c_str(),
                        ex.GetComment().ToStdString().c_str(), jsonOf( ex ).c_str() );
            }
        }
        else if( cmd == "report" )
        {
            EDA_UNITS units = f[1] == "in" ? EDA_UNITS::INCH : f[1] == "mils" ? EDA_UNITS::MILS : EDA_UNITS::MM;
            int       mask = std::stoi( f[2] );

            int i = 0;

            for( PCB_MARKER* marker : board->Markers() )
                dumpMarker( "marker", i++, marker, true );

            i = 0;

            for( const DRC_EXCLUSION& ex : board->GetDesignSettings().m_DrcExclusions )
            {
                printf( "bds|%d|key64=%s|comment=%s|json=%s\n", i++, fnv( ex.GetSortKey() ).c_str(),
                        ex.GetComment().ToStdString().c_str(), jsonOf( ex ).c_str() );
            }

            auto markersProvider = std::make_shared<DRC_ITEMS_PROVIDER>( board.get(), MARKER_BASE::MARKER_DRC,
                                                                         MARKER_BASE::MARKER_DRAWING_SHEET );
            auto ratsnestProvider = std::make_shared<DRC_ITEMS_PROVIDER>( board.get(), MARKER_BASE::MARKER_RATSNEST );
            auto fpWarningsProvider = std::make_shared<DRC_ITEMS_PROVIDER>( board.get(), MARKER_BASE::MARKER_PARITY );

            markersProvider->SetSeverities( mask );
            ratsnestProvider->SetSeverities( mask );
            fpWarningsProvider->SetSeverities( mask );

            printf( "counts|%d|%d|%d|errors=%d|warnings=%d\n", markersProvider->GetCount(),
                    ratsnestProvider->GetCount(), fpWarningsProvider->GetCount(),
                    markersProvider->GetCount( RPT_SEVERITY_ERROR ),
                    markersProvider->GetCount( RPT_SEVERITY_WARNING ) );

            DRC_REPORT  writer( board.get(), units, markersProvider, ratsnestProvider, fpWarningsProvider );
            std::string txt = aTmp + "/report.rpt";
            std::string json = aTmp + "/report.json";

            if( !writer.WriteTextReport( txt ) || !writer.WriteJsonReport( json ) )
            {
                printf( "error|%d|cannot write report\n", lineNo );
                return 1;
            }

            printBlock( "text", readFile( txt ), false );
            printBlock( "json", readFile( json ), true );
        }
        else if( cmd == "legacy" )
        {
            PCB_MARKER* marker = PCB_MARKER::FromLegacyString( wxString::FromUTF8( f[1] ) );

            if( marker )
                dumpMarker( "legacy", lineNo, marker, false );
            else
                printf( "legacy|%d|null\n", lineNo );

            delete marker;
        }
        else if( cmd == "strip" )
        {
            printf( "strip|%d|%s\n", lineNo,
                    HYPERLINK_DV_RENDERER::StripMarkup( wxString::FromUTF8( f[1] ) ).ToStdString( wxConvUTF8 ).c_str() );
        }
        else
        {
            printf( "error|%d|unknown command %s\n", lineNo, cmd.c_str() );
        }
    }

    // Markers own their items; the rules are leaked on purpose (the items point at them).
    return 0;
}


int main( int argc, char** argv )
{
    // Deterministic, headless environment (same as the kicad-cli oracle wrapper).
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "LANG", "C.UTF-8", 1 );
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    std::string              root = ".";
    std::vector<std::string> files;

    for( int i = 1; i < argc; ++i )
    {
        std::string a = argv[i];

        if( a == "--root" && i + 1 < argc )
            root = argv[++i];
        else
            files.push_back( a );
    }

    if( !initPgm( argc, argv ) )
        return 2;

    char        tmpl[] = "/tmp/drc-report-oracle-XXXXXX";
    std::string tmp = mkdtemp( tmpl );

    printf( "version|%s\n", GetMajorMinorPatchVersion().ToStdString().c_str() );

    int rc = 0;

    for( const std::string& f : files )
        rc |= runScenario( root, f, tmp );

    std::remove( ( tmp + "/report.rpt" ).c_str() );
    std::remove( ( tmp + "/report.json" ).c_str() );
    rmdir( tmp.c_str() );
    fflush( stdout );
    _exit( rc );
}
