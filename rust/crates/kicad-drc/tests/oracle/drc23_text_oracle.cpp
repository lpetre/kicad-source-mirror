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

// DRC-23 text oracle: the string helpers of DRC_TEST_PROVIDER_MISC::testTextVars against the
// reference build. For every input line (stdin, `\n` escaped as the two characters \n and `\\`
// as \\) it prints
//
//   expand <escaped ExpandEnvVarSubstitutions( text, nullptr )>
//   varref <0|1>                         varRefRegEx.Matches( text )
//   <DRC_WARNING|DRC_ERROR> <start> <len> <message>   each match of the reportEach loop
//
// with an empty environment except KICAD10_FOOTPRINT_DIR=/fp and DRC23_VAR=value (set here).

#include <common.h>
#include <wx/regex.h>
#include <wx/utils.h>
#include <iostream>
#include <string>

static std::string esc( const wxString& s )
{
    std::string out;

    for( wxUniChar c : s )
    {
        if( c == '\\' )
            out += "\\\\";
        else if( c == '\n' )
            out += "\\n";
        else if( c == '\t' )
            out += "\\t";
        else if( c == 0 )
            out += "\\0";
        else
            out += wxString( c ).ToStdString( wxConvUTF8 );
    }

    return out;
}


static wxString unesc( const std::string& s )
{
    std::string out;

    for( size_t i = 0; i < s.size(); ++i )
    {
        if( s[i] == '\\' && i + 1 < s.size() )
        {
            char n = s[++i];
            out += n == 'n' ? '\n' : n == 't' ? '\t' : n;
        }
        else
        {
            out += s[i];
        }
    }

    return wxString::FromUTF8( out );
}


int main()
{
    wxSetEnv( wxS( "KICAD10_FOOTPRINT_DIR" ), wxS( "/fp" ) );
    wxSetEnv( wxS( "DRC23_VAR" ), wxS( "value" ) );

    wxRegEx varRefRegEx( wxT( "(^|[^\\\\])\\$\\{.*\\}.*" ) );
    wxRegEx warningExpr( wxS( "(^|[^\\\\])\\$\\{DRC_WARNING\\s*([^}]*)\\}" ) );
    wxRegEx errorExpr( wxS( "(^|[^\\\\])\\$\\{DRC_ERROR\\s*([^}]*)\\}" ) );

    std::string line;

    while( std::getline( std::cin, line ) )
    {
        wxString text = unesc( line );
        std::cout << "input " << esc( text ) << "\n";
        std::cout << "expand " << esc( ExpandEnvVarSubstitutions( text, nullptr ) ) << "\n";
        std::cout << "varref " << ( varRefRegEx.Matches( text ) ? 1 : 0 ) << "\n";

        for( auto [expr, name] : { std::pair<wxRegEx*, const char*>( &warningExpr, "DRC_WARNING" ),
                                   std::pair<wxRegEx*, const char*>( &errorExpr, "DRC_ERROR" ) } )
        {
            wxString remaining = text;

            while( expr->Matches( remaining ) )
            {
                size_t start = 0;
                size_t len = 0;
                wxString msg = expr->GetMatch( remaining, 2 );

                if( !expr->GetMatch( &start, &len, 0 ) )
                    break;

                std::cout << name << " " << start << " " << len << " " << esc( msg ) << "\n";

                if( len == 0 )
                    break;

                remaining = remaining.Mid( start + len );
            }
        }
    }

    return 0;
}
