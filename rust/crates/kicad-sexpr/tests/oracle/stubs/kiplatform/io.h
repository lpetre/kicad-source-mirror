// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port (kicad-sexpr oracle harness).  Copyright The KiCad Developers.
// Stub of <kiplatform/io.h> for the kicad-sexpr oracle harness (see ../../oracle.cpp).
// common/richio.cpp needs these for the file based formatters, which the harness never uses.
#pragma once

#include <cstdio>
#include <wx/string.h>

namespace KIPLATFORM
{
namespace IO
{
    inline FILE* SeqFOpen( const wxString& aPath, const wxString& aMode )
    {
        return fopen( aPath.fn_str(), aMode.utf8_str() );
    }

    inline FILE* OpenUniqueSiblingTempFile( const wxString&, const wxChar*, wxString*, wxString* )
    {
        return nullptr;
    }

    inline bool FlushToDisk( FILE* ) { return false; }

    inline bool CommitTempFile( const wxString&, const wxString&, wxString* ) { return false; }

    inline wxString ResolveSymlinkTarget( const wxString& aPath ) { return aPath; }
}
}
