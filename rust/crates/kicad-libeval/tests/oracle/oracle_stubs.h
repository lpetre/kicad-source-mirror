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
 * Link-time stubs for symbols the oracle drivers never reach but which the KiCad objects they
 * link (string_utils.cpp, eda_units.cpp) reference.
 */

#ifndef ORACLE_STUBS_H
#define ORACLE_STUBS_H

#include <wx/string.h>

class PROJECT;

// kimath is not linked; eda_units.cpp only needs this symbol on overflow.
void kimathLogOverflow( double, const char* )
{
}

// Only used by LinkifyHTML() and friends in string_utils.cpp.
wxString ResolveUriByEnvVars( const wxString& aUri, const PROJECT* aProject )
{
    return aUri;
}

#endif
