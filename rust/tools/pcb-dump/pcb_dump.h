/*
 * This program source code file is part of KiCad, a free EDA CAD application.
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
 * pcb-dump: shared output and value-formatting helpers.
 *
 * Every output line is "SECTION|KEY|FIELD=VALUE" (see rust/docs/PCB_DUMP_FORMAT.md).  All value
 * formatting goes through the V* functions below so the format stays uniform.
 */

#pragma once

#include <cstdint>
#include <optional>
#include <set>
#include <string>
#include <vector>

#define MAGIC_ENUM_RANGE_MIN -16
#define MAGIC_ENUM_RANGE_MAX 512
#include <magic_enum.hpp>

#include <wx/string.h>
#include <math/vector2d.h>
#include <math/box2.h>
#include <geometry/eda_angle.h>
#include <layer_ids.h>
#include <lset.h>
#include <kiid.h>

class SHAPE;
class SHAPE_POLY_SET;
class SHAPE_LINE_CHAIN;
class SHAPE_ARC;
class BOARD;
class BOARD_ITEM;


/// Output sink.  Writes lines to stdout; in hash mode the value is replaced by its FNV-1a hash.
class DUMP_OUT
{
public:
    bool m_hash = false;

    void Line( const std::string& aSection, const std::string& aKey, const std::string& aField,
               const std::string& aValue );

    /// Current section/key context, used by the short Emit() form.
    std::string m_section;
    std::string m_key;

    /// Prepended to every field name by Emit() (used by the defaults command).
    std::string m_fieldPrefix;

    void Emit( const std::string& aField, const std::string& aValue )
    {
        Line( m_section, m_key, m_fieldPrefix + aField, aValue );
    }

    /// Report an error on an "error|..." line (never hashed).
    void Error( const std::string& aSection, const std::string& aKey, const std::string& aMsg );
};

extern DUMP_OUT g_out;

/// RAII helper setting the section/key context of g_out.
struct SCOPE
{
    std::string m_oldSection, m_oldKey;

    SCOPE( const std::string& aSection, const std::string& aKey )
    {
        m_oldSection = g_out.m_section;
        m_oldKey = g_out.m_key;
        g_out.m_section = aSection;
        g_out.m_key = aKey;
    }

    ~SCOPE()
    {
        g_out.m_section = m_oldSection;
        g_out.m_key = m_oldKey;
    }
};

uint64_t Fnv1a64( const std::string& aData );

// ---- value formatters ---------------------------------------------------------------------

std::string V( int aVal );
std::string V( unsigned aVal );
std::string V( long aVal );
std::string V( long long aVal );
std::string V( unsigned long aVal );
std::string V( unsigned long long aVal );
std::string V( double aVal );
std::string V( bool aVal );
std::string V( const char* aVal );
std::string V( const std::string& aVal );
std::string V( const wxString& aVal );
std::string V( const VECTOR2I& aVal );
std::string V( const VECTOR2D& aVal );
std::string V( const BOX2I& aVal );
std::string V( const EDA_ANGLE& aVal );
std::string V( const LSET& aVal );
std::string V( const KIID& aVal );

/// Layer as its canonical name (LSET::Name), e.g. "F.Cu"; UNDEFINED_LAYER -> "UNDEFINED_LAYER".
std::string VLayer( int aLayer );

/// List of layers: "[F.Cu,B.Cu]".
std::string VLayers( const std::vector<PCB_LAYER_ID>& aLayers );

/// LSET as its layer-name list (complements the hex form of V(LSET)).
std::string VLayerNames( const LSET& aSet );

std::string VShape( const SHAPE* aShape );
std::string VPolySet( const SHAPE_POLY_SET& aSet );
std::string VChain( const SHAPE_LINE_CHAIN& aChain );
std::string VArc( const SHAPE_ARC& aArc );
std::string VPoints( const std::vector<VECTOR2I>& aPts );

template <typename E>
std::string VEnum( E aVal )
{
    auto name = magic_enum::enum_name( aVal );

    if( name.empty() )
        return "?" + std::to_string( static_cast<long long>( aVal ) );

    return std::string( name );
}

template <typename T>
std::string VOpt( const std::optional<T>& aVal )
{
    if( !aVal.has_value() )
        return "none";

    if constexpr( std::is_enum_v<T> )
        return VEnum( *aVal );
    else
        return V( *aVal );
}

/// Item key: UUID when unique and non-nil, otherwise "#<ordinal>".
std::string ItemKey( const BOARD_ITEM* aItem );

/// Run aFn, reporting any exception on an error line and continuing.
template <typename F>
void Guard( const std::string& aWhat, F&& aFn );

/// The copper layers of the board in stack order (F_Cu, In1..., B_Cu).
std::vector<PCB_LAYER_ID> CopperLayers( const BOARD* aBoard );

/// Emit one field; an exception while computing the value (including a wx assertion) is reported
/// on an error line for this field only and the dump continues with the next field.
#define EMIT( aField, aValueExpr ) Guard( aField, [&]() { g_out.Emit( aField, aValueExpr ); } )

// ---- sections (dump_*.cpp) ----------------------------------------------------------------

struct DUMP_OPTIONS
{
    std::set<std::string> sections;   ///< empty = all
    bool                  hash = false;

    bool Want( const std::string& aSection ) const
    {
        return sections.empty() || sections.count( aSection );
    }
};

#include <functional>
void ForEachBoardItem( BOARD* aBoard, const std::function<void( BOARD_ITEM* )>& aFn );
void InitItemKeys( BOARD* aBoard );

/// Collect the UUIDs written in the board file (36-char UUIDs and legacy 8-hex-digit tstamps).
/// Items whose UUID is not in the file were given a generated one by the loader.
void ScanFileUuids( const std::string& aPath );
bool UuidInFile( const KIID& aId );
void DumpBoard( BOARD* aBoard, const DUMP_OPTIONS& aOpts );
void DumpDefaults();
void DumpItemSections( BOARD* aBoard, BOARD_ITEM* aItem, const DUMP_OPTIONS& aOpts );
void DumpBoardSection( BOARD* aBoard );
void DumpOrderSection( BOARD* aBoard );

// Individual item dumpers (key and section are set by the caller).
void DumpItemBase( BOARD_ITEM* aItem );
void DumpBBox( BOARD_ITEM* aItem );
void DumpTrack( BOARD_ITEM* aItem );
void DumpPad( BOARD_ITEM* aItem );
void DumpShape( BOARD_ITEM* aItem );
void DumpText( BOARD_ITEM* aItem );
void DumpDimension( BOARD_ITEM* aItem );
void DumpZone( BOARD_ITEM* aItem );
void DumpFootprint( BOARD_ITEM* aItem );
void DumpMisc( BOARD_ITEM* aItem );


#include <csetjmp>
#include <stdexcept>
#include <ki_exception.h>

/// Stack of recovery points for SIGSEGV/SIGFPE/SIGBUS inside Guard() (see InstallCrashGuard).
extern std::vector<sigjmp_buf*> g_crashJumps;
void InstallCrashGuard();

template <typename F>
void Guard( const std::string& aWhat, F&& aFn )
{
    sigjmp_buf env;

    if( sigsetjmp( env, 1 ) != 0 )
    {
        // A crash inside aFn: report it and carry on.  Destructors of the aborted frames do not
        // run (leaks are acceptable for a dump tool).
        g_crashJumps.pop_back();
        g_out.Error( g_out.m_section, g_out.m_key, aWhat + ": crashed (signal)" );
        return;
    }

    g_crashJumps.push_back( &env );

    try
    {
        aFn();
        g_crashJumps.pop_back();
        return;
    }
    catch( const IO_ERROR& e )
    {
        g_crashJumps.pop_back();
        g_out.Error( g_out.m_section, g_out.m_key, aWhat + ": " + e.What().ToStdString() );
    }
    catch( const std::exception& e )
    {
        g_crashJumps.pop_back();
        g_out.Error( g_out.m_section, g_out.m_key, aWhat + ": " + e.what() );
    }
    catch( ... )
    {
        g_crashJumps.pop_back();
        g_out.Error( g_out.m_section, g_out.m_key, aWhat + ": unknown exception" );
    }
}
