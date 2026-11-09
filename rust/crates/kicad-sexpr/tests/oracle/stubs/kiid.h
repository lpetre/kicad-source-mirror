// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port (kicad-sexpr oracle harness).  Copyright The KiCad Developers.
// Stub of <kiid.h> for the kicad-sexpr oracle harness (see ../oracle.cpp): just what
// KICAD_FORMAT::FormatUuid uses.
#pragma once

#include <wx/string.h>

class KIID
{
public:
    KIID( const wxString& aText ) : m_text( aText ) {}
    wxString AsString() const { return m_text; }

private:
    wxString m_text;
};
