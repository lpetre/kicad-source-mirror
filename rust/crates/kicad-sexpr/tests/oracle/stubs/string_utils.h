// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port (kicad-sexpr oracle harness).  Copyright The KiCad Developers.
// Stub of <string_utils.h> for the kicad-sexpr oracle harness (see ../oracle.cpp).
#pragma once

#include <string>
#include <wx/string.h>

#define TO_UTF8( wxstring ) ( (const char*) ( wxstring ).utf8_str() )

inline wxString From_UTF8( const char* cstring )
{
    return wxString::FromUTF8( cstring );
}

inline wxString From_UTF8( const std::string& aString )
{
    return wxString::FromUTF8( aString );
}
