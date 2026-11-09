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
 * Reference ("oracle") driver for the Rust port of NUMERIC_EVALUATOR (crate kicad-libeval,
 * module numeric_evaluator).  Links the unmodified common/libeval/numeric_evaluator.cpp.
 *
 * Input lines:  <units> TAB <expr> [TAB <expr> ...]
 *   units is the numeric value of EDA_UNITS given to the constructor; all expressions of a line
 *   are processed in sequence by the same evaluator (Clear() is NOT called in between, variables
 *   persist), with the variables MoL=42 and pi=3.14 predefined.
 * Output: for each expression one line "R <valid> <result>", framed by "EXPR <line>" / "END".
 *
 * This file is not part of the cargo build.
 */

#include <cstdio>
#include <iostream>
#include <string>
#include <vector>

#include <wx/string.h>

#include <eda_units.h>
#include <libeval/numeric_evaluator.h>


#include "oracle_stubs.h"


static std::string unescape( const std::string& in )
{
    std::string out;

    for( size_t i = 0; i < in.size(); ++i )
    {
        if( in[i] == '\\' && i + 1 < in.size() )
        {
            ++i;

            switch( in[i] )
            {
            case 'n':  out += '\n'; break;
            case 't':  out += '\t'; break;
            case 'r':  out += '\r'; break;
            default:   out += in[i]; break;
            }
        }
        else
        {
            out += in[i];
        }
    }

    return out;
}


int main( int argc, char* argv[] )
{
    std::string line;

    while( std::getline( std::cin, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::vector<std::string> parts;
        size_t                   start = 0;

        for( ;; )
        {
            size_t tab = line.find( '\t', start );

            if( tab == std::string::npos )
            {
                parts.push_back( line.substr( start ) );
                break;
            }

            parts.push_back( line.substr( start, tab - start ) );
            start = tab + 1;
        }

        if( parts.size() < 2 )
            continue;

        printf( "EXPR\t%s\n", line.c_str() );

        NUMERIC_EVALUATOR eval( static_cast<EDA_UNITS>( atoi( parts[0].c_str() ) ) );

        eval.SetVar( "MoL", 42 );
        eval.SetVar( "pi", 3.14 );

        for( size_t i = 1; i < parts.size(); ++i )
        {
            bool ok = eval.Process( wxString::FromUTF8( unescape( parts[i] ).c_str() ) );

            printf( "R\t%d\t%d\t%s\n", ok ? 1 : 0, eval.IsValid() ? 1 : 0,
                    (const char*) eval.Result().ToUTF8() );
        }

        printf( "END\n" );
    }

    return 0;
}
