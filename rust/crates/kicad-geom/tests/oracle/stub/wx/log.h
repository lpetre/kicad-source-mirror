// Minimal stand-in for <wx/log.h>, just enough to compile libs/kimath/src/math/util.cpp
// without wxWidgets for the kicad-geom differential oracle (see ../geom_oracle.cpp).
// Assertions and logging are no-ops, which is the behaviour of a release build.
#pragma once

#include <cstdarg>
#include <string>

struct wxString
{
    wxString() {}
    wxString( const char* ) {}
    static wxString FromAscii( const char* ) { return wxString(); }
    template <class... A>
    static wxString Format( A... ) { return wxString(); }
};

struct wxLog
{
    static bool IsLevelEnabled( int, wxString ) { return false; }
};

#define wxLOG_Debug 0
#define wxLOG_COMPONENT ""
inline void wxVLogWarning( const char*, va_list ) {}
#define wxFAIL_MSG( x ) do {} while( 0 )
#define wxT( x ) x
