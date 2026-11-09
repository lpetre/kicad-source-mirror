// Minimal stand-in for <wx/debug.h>, see log.h.
#pragma once
#include <wx/log.h>
#define wxCHECK_MSG( cond, rc, msg ) do { if( !( cond ) ) return rc; } while( 0 )
#define wxASSERT( x ) do {} while( 0 )
