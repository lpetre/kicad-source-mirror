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

/*
 * Package CC-1 oracle (rust/crates/kicad-pcb/src/component_class.rs): against libkicommon.so,
 *
 *   - COMPONENT_CLASS_ASSIGNMENT_DATA::GetAssignmentInDRCLanguage for hand-made assignments
 *     ("assign" lines of the case file),
 *   - COMPONENT_CLASS_SETTINGS loaded by PROJECT_FILE::LoadFromFile from a one-line project
 *     text ("project" lines, written to a scratch file) or from a corpus file ("file" lines).
 *
 * Case file: one case per line, fields separated by TAB:
 *   assign  <class>  <ALL|ANY>  { <TYPE>  <primary>  <secondary> }...
 *   project <json text>
 *   file    <path relative to the KiCad root>
 * Output: "<case>|<key>|<value>" lines, replayed by tests/cc1_oracle.rs.
 */

#include <project/component_class_settings.h>
#include <project/project_file.h>

#include <wx/init.h>
#include <wx/log.h>

#include <clocale>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

static std::vector<std::string> split( const std::string& aLine )
{
    std::vector<std::string> out;
    std::string              cur;

    for( char c : aLine )
    {
        if( c == '\t' )
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

static std::string u8( const wxString& s )
{
    return std::string( s.ToUTF8().data() );
}

static void dumpSettings( int aCase, COMPONENT_CLASS_SETTINGS& aSettings )
{
    std::cout << aCase << "|enabled|" << ( aSettings.GetEnableSheetComponentClasses() ? 1 : 0 )
              << "\n";

    const auto& list = aSettings.GetComponentClassAssignments();
    std::cout << aCase << "|count|" << list.size() << "\n";

    for( size_t i = 0; i < list.size(); ++i )
    {
        const COMPONENT_CLASS_ASSIGNMENT_DATA& a = list[i];
        std::cout << aCase << "|a" << i << ".class|" << u8( a.GetComponentClass() ) << "\n";
        std::cout << aCase << "|a" << i << ".op|"
                  << ( a.GetConditionsOperator()
                                       == COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITIONS_OPERATOR::ALL
                               ? "ALL"
                               : "ANY" )
                  << "\n";

        int j = 0;

        for( const auto& [type, primary, secondary] : a.GetConditions() )
        {
            std::cout << aCase << "|a" << i << ".c" << j++ << "|"
                      << u8( COMPONENT_CLASS_ASSIGNMENT_DATA::GetConditionName( type ) ) << "\t"
                      << u8( primary ) << "\t" << u8( secondary ) << "\n";
        }

        std::cout << aCase << "|a" << i << ".drc|" << u8( a.GetAssignmentInDRCLanguage() )
                  << "\n";
    }
}

static void loadProject( int aCase, const std::string& aPath )
{
    PROJECT_FILE file( wxString::FromUTF8( aPath ) );
    bool         ok = file.LoadFromFile();
    std::cout << aCase << "|loaded|" << ( ok ? 1 : 0 ) << "\n";
    dumpSettings( aCase, *file.ComponentClassSettings() );
}

int main( int argc, char** argv )
{
    if( argc < 4 )
    {
        std::cerr << "usage: cc1_oracle <kicad root> <cases> <scratch dir>\n";
        return 1;
    }

    setlocale( LC_ALL, "C.UTF-8" );
    wxInitializer init;
    wxLog::EnableLogging( false );

    std::string   root = argv[1];
    std::ifstream in( argv[2] );
    std::string   scratch = argv[3];
    std::string   line;
    int           index = 0;

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::vector<std::string> f = split( line );
        int                      c = index++;

        if( f[0] == "assign" )
        {
            COMPONENT_CLASS_ASSIGNMENT_DATA a;
            a.SetComponentClass( wxString::FromUTF8( f[1] ) );
            a.SetConditionsOperation( f[2] == "ANY"
                                              ? COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITIONS_OPERATOR::ANY
                                              : COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITIONS_OPERATOR::ALL );

            for( size_t k = 3; k + 2 < f.size(); k += 3 )
            {
                a.AddCondition( COMPONENT_CLASS_ASSIGNMENT_DATA::GetConditionType(
                                        wxString::FromUTF8( f[k] ) ),
                                wxString::FromUTF8( f[k + 1] ), wxString::FromUTF8( f[k + 2] ) );
            }

            std::cout << c << "|drc|" << u8( a.GetAssignmentInDRCLanguage() ) << "\n";
        }
        else if( f[0] == "project" )
        {
            std::string path = scratch + "/cc1_case.kicad_pro";
            {
                std::ofstream out( path );
                out << f[1];
            }
            loadProject( c, path );
        }
        else if( f[0] == "file" )
        {
            loadProject( c, root + "/" + f[1] );
        }
    }

    return 0;
}
