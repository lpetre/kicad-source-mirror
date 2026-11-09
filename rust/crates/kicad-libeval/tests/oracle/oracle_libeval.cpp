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
 * Reference ("oracle") driver for the Rust port of LIBEVAL (crate kicad-libeval).
 *
 * It links the *unmodified* C++ sources common/libeval_compiler/libeval_compiler.cpp (with the
 * Lemon parser generated from grammar.lemon by thirdparty/lemon), common/string_utils.cpp and
 * common/eda_units.cpp and runs expressions through LIBEVAL::COMPILER / LIBEVAL::UCODE with a
 * small, deterministic test binding (TEST_UCODE below).  The Rust integration test
 * tests/oracle_golden.rs implements the very same binding on top of the Rust port and
 * compares every observable (return values, error callbacks and offsets, microcode dump,
 * results, runtime errors) against the golden file this tool writes.
 *
 * Usage: oracle_libeval < cases.txt > golden.txt        (see build_oracle.sh)
 *
 * This file is not part of the cargo build.
 */

#include <clocale>
#include <cstdio>
#include <memory>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

#include <wx/string.h>
#include <wx/tokenzr.h>
#include <wx/crt.h>

#include <base_units.h>
#include <eda_units.h>
#include <libeval_compiler/libeval_compiler.h>

using namespace LIBEVAL;


#include "oracle_stubs.h"


/* --------------------------------------------------------------------------------------------
 * Unit resolvers: verbatim copies of PCBEXPR_UNIT_RESOLVER from pcbnew/pcbexpr_evaluator.cpp
 */

class TEST_PCB_UNIT_RESOLVER : public UNIT_RESOLVER
{
public:
    TEST_PCB_UNIT_RESOLVER( bool aImpliedDegrees = false ) :
            m_impliedDegrees( aImpliedDegrees )
    { }

    const std::vector<wxString>& GetSupportedUnits() const override
    {
        static const std::vector<wxString> pcbUnits = { wxT( "mil" ), wxT( "mm" ), wxT( "in" ),
                                                        wxT( "deg" ), wxT( "fs" ), wxT( "ps" ) };
        return pcbUnits;
    }

    const std::vector<EDA_UNITS>& GetSupportedUnitsTypes() const override
    {
        static const std::vector<EDA_UNITS> pcbUnits = { EDA_UNITS::MILS,    EDA_UNITS::MM, EDA_UNITS::INCH,
                                                         EDA_UNITS::DEGREES, EDA_UNITS::FS, EDA_UNITS::PS };
        return pcbUnits;
    }

    wxString GetSupportedUnitsMessage() const override
    {
        if( m_impliedDegrees )
            return wxEmptyString;

        return _( "must be mm, in, mil, deg, fs, or ps" );
    }

    double Convert( const wxString& aString, int unitId ) const override
    {
        double v = wxAtof( aString );

        switch( unitId )
        {
        case 0: return EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, EDA_UNITS::MILS, aString );
        case 1: return EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, EDA_UNITS::MM, aString );
        case 2: return EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, EDA_UNITS::INCH, aString );
        case 3: return v;
        case 4: return EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, EDA_UNITS::FS, aString );
        case 5: return EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, EDA_UNITS::PS, aString );
        default: return v;
        }
    }

private:
    bool m_impliedDegrees;
};


class TEST_COMPILER : public COMPILER
{
public:
    TEST_COMPILER( UNIT_RESOLVER* aUnitResolver )
    {
        if( aUnitResolver )
            m_unitResolver.reset( aUnitResolver );
    }
};


/* --------------------------------------------------------------------------------------------
 * Test binding
 */

class TEST_CONTEXT : public CONTEXT
{
public:
    std::vector<wxString> m_log;        // side effects of deferred evaluations
};


// A value comparing by code like PCBEXPR_NET_VALUE, with a lazily computed string like
// PCBEXPR_NETCLASS_VALUE.
class TEST_CODE_VALUE : public VALUE
{
public:
    TEST_CODE_VALUE( int aCode, const wxString& aName ) :
            VALUE( wxEmptyString ),
            m_code( aCode ),
            m_name( aName )
    {}

    const wxString& AsString() const override
    {
        const_cast<TEST_CODE_VALUE*>( this )->Set( m_name );
        return VALUE::AsString();
    }

    bool EqualTo( CONTEXT* aCtx, const VALUE* b ) const override
    {
        if( const TEST_CODE_VALUE* bValue = dynamic_cast<const TEST_CODE_VALUE*>( b ) )
            return m_code == bValue->m_code;
        else
            return VALUE::EqualTo( aCtx, b );
    }

    bool NotEqualTo( CONTEXT* aCtx, const VALUE* b ) const override
    {
        if( const TEST_CODE_VALUE* bValue = dynamic_cast<const TEST_CODE_VALUE*>( b ) )
            return m_code != bValue->m_code;
        else
            return VALUE::NotEqualTo( aCtx, b );
    }

private:
    int      m_code;
    wxString m_name;
};


// A value with only EqualTo overridden, like PCBEXPR_PINTYPE_VALUE.
class TEST_ALIAS_VALUE : public VALUE
{
public:
    TEST_ALIAS_VALUE( const wxString& aName, const wxString& aAlias ) :
            VALUE( aName ),
            m_alias( aAlias )
    {}

    bool EqualTo( CONTEXT* aCtx, const VALUE* b ) const override
    {
        const wxString& thisStr = AsString();
        const wxString& otherStr = b->AsString();

        if( thisStr.IsSameAs( otherStr, false ) )
            return true;

        return m_alias.IsSameAs( otherStr, false );
    }

private:
    wxString m_alias;
};


enum TEST_FIELD
{
    F_OBJECT,
    F_NULLVAR,
    F_NUM,
    F_FRAC,
    F_STR,
    F_NUMSTR,
    F_EMPTY,
    F_UNDEF,
    F_NULL,
    F_MM,
    F_WILD,
    F_CODE,
    F_ALIAS,
    F_BAD
};


class TEST_VAR_REF : public VAR_REF
{
public:
    TEST_VAR_REF( int aItem, int aNav, TEST_FIELD aField, VAR_TYPE_T aType ) :
            m_item( aItem ),
            m_nav( aNav ),
            m_field( aField ),
            m_type( aType )
    {}

    VAR_TYPE_T GetType() const override { return m_type; }

    VALUE* GetValue( CONTEXT* aCtx ) override
    {
        static const char* strs[] = { "hello", "World", "F.Cu" };
        static const char* numstrs[] = { "42", "4k7", "1.5mm" };
        static const double fracs[] = { 2.5, 0.1, -0.75 };

        switch( m_field )
        {
        case F_NULLVAR: return VALUE::MakeNullValue();
        case F_OBJECT:  return new VALUE();
        case F_NUM:     return new VALUE( 10.0 * ( m_item + 1 ) + 100.0 * m_nav );
        case F_FRAC:    return new VALUE( fracs[m_item] );
        case F_STR:     return new VALUE( wxString( strs[m_item] ) );
        case F_NUMSTR:  return new VALUE( wxString( numstrs[m_item] ) );
        case F_EMPTY:   return new VALUE( wxString() );
        case F_UNDEF:   return new VALUE();
        case F_NULL:    return VALUE::MakeNullValue();

        case F_MM:
        {
            VALUE* v = new VALUE( 1e6 * ( m_item + 1 ) );
            v->SetUnits( EDA_UNITS::MM );
            return v;
        }

        case F_WILD:    return new VALUE( wxString( "h*" ), true );
        case F_CODE:    return new TEST_CODE_VALUE( m_item == 2 ? 9 : 7, m_item == 0 ? "seven" : m_item == 1 ? "SEVEN-B" : "nine" );
        case F_ALIAS:   return new TEST_ALIAS_VALUE( m_item == 0 ? "power_in" : "passive", m_item == 0 ? "Power input" : "Passive" );
        case F_BAD:     return new VALUE();
        }

        return new VALUE();
    }

    int m_item;
    int m_nav;
    TEST_FIELD m_field;
    VAR_TYPE_T m_type;
};


static void fnOne( CONTEXT* aCtx, void* self )
{
    VALUE* result = aCtx->AllocValue();
    result->Set( 1.0 );
    aCtx->Push( result );
}


static void fnIsFoo( CONTEXT* aCtx, void* self )
{
    VALUE* arg = aCtx->Pop();
    VALUE* result = aCtx->AllocValue();

    result->Set( 0.0 );
    aCtx->Push( result );

    if( !arg || arg->AsString().IsEmpty() )
    {
        if( aCtx->HasErrorCallback() )
            aCtx->ReportError( wxString::Format( _( "Missing argument to %s." ), wxT( "isFoo()" ) ) );

        return;
    }

    if( arg->AsString() == wxT( "foo" ) )
        result->Set( 1.0 );
}


static void fnAdd( CONTEXT* aCtx, void* self )
{
    VALUE* b = aCtx->Pop();
    VALUE* a = aCtx->Pop();
    VALUE* result = aCtx->AllocValue();

    result->Set( a->AsDouble() + b->AsDouble() );
    aCtx->Push( result );
}


static void fnCat( CONTEXT* aCtx, void* self )
{
    VALUE* b = aCtx->Pop();
    VALUE* a = aCtx->Pop();
    VALUE* result = aCtx->AllocValue();

    result->Set( a->AsString() + b->AsString() );
    aCtx->Push( result );
}


// Returns the navigation depth of the receiver plus 1, or 0 without receiver.
static void fnSelf( CONTEXT* aCtx, void* self )
{
    TEST_VAR_REF* vref = static_cast<TEST_VAR_REF*>( self );
    VALUE*        result = aCtx->AllocValue();

    result->Set( vref ? 1.0 + vref->m_nav + 10.0 * vref->m_item : 0.0 );
    aCtx->Push( result );
}


static void fnLazy( CONTEXT* aCtx, void* self )
{
    VALUE* result = aCtx->AllocValue();

    result->Set( 0.0 );
    aCtx->Push( result );

    result->SetDeferredEval(
            [aCtx]() -> double
            {
                static_cast<TEST_CONTEXT*>( aCtx )->m_log.push_back( wxT( "lazy evaluated" ) );

                if( aCtx->HasErrorCallback() )
                    aCtx->ReportError( _( "lazy() evaluated with error callback" ) );

                return 5.0;
            } );
}


static void fnLazyStr( CONTEXT* aCtx, void* self )
{
    VALUE* arg = aCtx->Pop();
    VALUE* result = aCtx->AllocValue();

    result->Set( "" );
    aCtx->Push( result );

    result->SetDeferredEval(
            [aCtx, arg]() -> wxString
            {
                static_cast<TEST_CONTEXT*>( aCtx )->m_log.push_back( wxT( "lazyStr evaluated" ) );
                return wxT( "<" ) + arg->AsString() + wxT( ">" );
            } );
}


static void fnNoPush( CONTEXT* aCtx, void* self )
{
    aCtx->Pop();
}


static void fnPush2( CONTEXT* aCtx, void* self )
{
    VALUE* a = aCtx->AllocValue();
    a->Set( 1.0 );
    aCtx->Push( a );

    VALUE* b = aCtx->AllocValue();
    b->Set( 2.0 );
    aCtx->Push( b );
}


class TEST_UCODE : public UCODE
{
public:
    std::unique_ptr<VAR_REF> CreateVarRef( const wxString& aVar, const wxString& aField ) override
    {
        m_log.push_back( wxT( "var:" ) + aVar + wxT( ":" ) + aField );

        if( aVar.IsSameAs( wxT( "null" ), false ) )
            return std::make_unique<TEST_VAR_REF>( 0, 0, F_NULLVAR, VT_NULL );

        wxString baseVar = aVar;
        int      nav = 0;

        if( aVar.Contains( wxT( "." ) ) )
        {
            wxArrayString tokens = wxSplit( aVar, '.' );
            baseVar = tokens.IsEmpty() ? wxString() : tokens[0];

            for( size_t i = 1; i < tokens.GetCount(); ++i )
            {
                if( tokens[i].CmpNoCase( wxT( "Parent" ) ) == 0 )
                    nav++;
                else
                    return nullptr;
            }

            if( baseVar != wxT( "A" ) && baseVar != wxT( "B" ) )
                return nullptr;
        }

        int item;

        if( baseVar == wxT( "A" ) )
            item = 0;
        else if( baseVar == wxT( "B" ) )
            item = 1;
        else if( baseVar == wxT( "L" ) )
            item = 2;
        else
            return nullptr;

        if( aField.length() == 0 )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_OBJECT, VT_UNDEFINED );

        wxString field = aField.Lower();

        if( field == wxT( "num" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_NUM, VT_NUMERIC );
        else if( field == wxT( "frac" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_FRAC, VT_NUMERIC_DOUBLE );
        else if( field == wxT( "str" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_STR, VT_STRING );
        else if( field == wxT( "numstr" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_NUMSTR, VT_STRING );
        else if( field == wxT( "empty" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_EMPTY, VT_STRING );
        else if( field == wxT( "undef" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_UNDEF, VT_NUMERIC );
        else if( field == wxT( "null" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_NULL, VT_NUMERIC );
        else if( field == wxT( "mm" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_MM, VT_NUMERIC );
        else if( field == wxT( "wild" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_WILD, VT_STRING );
        else if( field == wxT( "code" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_CODE, VT_STRING );
        else if( field == wxT( "alias" ) )
            return std::make_unique<TEST_VAR_REF>( item, nav, F_ALIAS, VT_STRING );

        return std::make_unique<TEST_VAR_REF>( item, nav, F_BAD, VT_PARSE_ERROR );
    }

    FUNC_CALL_REF CreateFuncCall( const wxString& aName ) override
    {
        m_log.push_back( wxT( "func:" ) + aName );

        wxString name = aName.Lower();

        if( name == wxT( "one" ) )
            return fnOne;
        else if( name == wxT( "isfoo" ) )
            return fnIsFoo;
        else if( name == wxT( "add" ) )
            return fnAdd;
        else if( name == wxT( "cat" ) )
            return fnCat;
        else if( name == wxT( "self" ) )
            return fnSelf;
        else if( name == wxT( "lazy" ) )
            return fnLazy;
        else if( name == wxT( "lazystr" ) )
            return fnLazyStr;
        else if( name == wxT( "nopush" ) )
            return fnNoPush;
        else if( name == wxT( "push2" ) )
            return fnPush2;

        return nullptr;
    }

    std::vector<wxString> m_log;
};


/* --------------------------------------------------------------------------------------------
 * Output helpers
 */

static std::string escape( const wxString& aStr )
{
    std::string in( aStr.ToUTF8().data() );
    std::string out;

    for( char c : in )
    {
        switch( c )
        {
        case '\\': out += "\\\\"; break;
        case '\n': out += "\\n";  break;
        case '\t': out += "\\t";  break;
        case '\r': out += "\\r";  break;
        default:   out += c;      break;
        }
    }

    return out;
}


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


static const char* typeName( VAR_TYPE_T aType )
{
    switch( aType )
    {
    case VT_STRING:         return "STRING";
    case VT_NUMERIC:        return "NUMERIC";
    case VT_NUMERIC_DOUBLE: return "NUMERIC_DOUBLE";
    case VT_UNDEFINED:      return "UNDEFINED";
    case VT_PARSE_ERROR:    return "PARSE_ERROR";
    case VT_NULL:           return "NULL";
    }

    return "?";
}


static std::string bits( double aValue )
{
    uint64_t u;
    memcpy( &u, &aValue, sizeof( u ) );

    // All NaNs are equal for the purpose of the comparison.
    if( aValue != aValue )
        return "nan";

    char buf[32];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) u );
    return buf;
}


// Replace the addresses printed by UOP::Format() ("PUSH VAR [0x55...]") by a constant.
static wxString normalizeDump( const wxString& aDump )
{
    wxString out;
    size_t   i = 0;

    while( i < aDump.length() )
    {
        if( aDump.Mid( i, 10 ) == wxT( "PUSH VAR [" ) )
        {
            size_t end = aDump.find( ']', i );
            wxString ptr = aDump.Mid( i + 10, end - ( i + 10 ) );

            out += wxT( "PUSH VAR [" );
            out += ( ptr == wxT( "(nil)" ) || ptr == wxT( "0" ) || ptr == wxT( "0x0" ) ) ? wxT( "null" ) : wxT( "ref" );
            out += wxT( "]" );
            i = end + 1;
        }
        else
        {
            out += aDump[i];
            i++;
        }
    }

    return out;
}


static UNIT_RESOLVER* makeResolver( const std::string& aName )
{
    if( aName == "pcb" )
        return new TEST_PCB_UNIT_RESOLVER( false );
    else if( aName == "pcbdeg" )
        return new TEST_PCB_UNIT_RESOLVER( true );

    return nullptr;     // keep the LIBEVAL::UNIT_RESOLVER default
}


static void runOnce( TEST_UCODE& aCode, bool aWithCallback, const char* aTag )
{
    TEST_CONTEXT          ctx;
    std::vector<wxString> errors;

    if( aWithCallback )
    {
        ctx.SetErrorCallback(
                [&]( const wxString& aMessage, int aOffset )
                {
                    errors.push_back( wxString::Format( wxT( "%d\t" ), aOffset ) + aMessage );
                } );
    }

    VALUE*     result = aCode.Run( &ctx );
    VAR_TYPE_T type = result->GetType();
    double     dbl = result->AsDouble();
    wxString   str = result->AsString();
    EDA_UNITS  units = result->GetUnits();

    printf( "%s\t%s\t%d\t%s\t%s\n", aTag, typeName( type ), (int) units, bits( dbl ).c_str(),
            escape( str ).c_str() );

    for( const wxString& err : errors )
        printf( "X\t%s\n", escape( err ).c_str() );

    for( const wxString& entry : ctx.m_log )
        printf( "L\t%s\n", escape( entry ).c_str() );
}


static void compileOne( TEST_COMPILER& aCompiler, const wxString& aExpr, bool aRun )
{
    TEST_UCODE            ucode;
    TEST_CONTEXT          preflight;
    std::vector<wxString> errors;

    aCompiler.SetErrorCallback(
            [&]( const wxString& aMessage, int aOffset )
            {
                errors.push_back( wxString::Format( wxT( "%d\t" ), aOffset ) + aMessage );
            } );

    bool ok = aCompiler.Compile( aExpr, &ucode, &preflight );

    const ERROR_STATUS& status = aCompiler.GetError();

    if( aCompiler.IsErrorPending() )
    {
        printf( "C\t%d\t1\t%d\t%d\t%s\n", ok ? 1 : 0, (int) status.stage, status.srcPos,
                escape( status.message ).c_str() );
    }
    else
    {
        printf( "C\t%d\t0\n", ok ? 1 : 0 );
    }

    for( const wxString& err : errors )
        printf( "E\t%s\n", escape( err ).c_str() );

    for( const wxString& entry : ucode.m_log )
        printf( "B\t%s\n", escape( entry ).c_str() );

    for( const wxString& entry : preflight.m_log )
        printf( "P\t%s\n", escape( entry ).c_str() );

    wxString dump = normalizeDump( ucode.Dump() );
    printf( "U\t%s\n", escape( dump ).c_str() );

    if( ok && aRun )
    {
        runOnce( ucode, true, "R" );
        runOnce( ucode, false, "N" );
    }

    // The callback captures locals of this function.
    aCompiler.SetErrorCallback( nullptr );
}


int main( int argc, char* argv[] )
{
    // Case-insensitive comparisons (wxString::IsSameAs / MakeUpper) go through towlower() /
    // towupper(), which only know ASCII in the "C" locale.  KiCad runs with the user's locale,
    // so take the character classification from the environment (run with LC_ALL=C.UTF-8);
    // everything else, in particular LC_NUMERIC, stays "C".
    setlocale( LC_CTYPE, "" );

    std::string line;

    while( std::getline( std::cin, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        // <resolver> TAB <expr> [TAB <expr> ...]; more than one expression = same COMPILER reused
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

        TEST_COMPILER compiler( makeResolver( parts[0] ) );

        for( size_t i = 1; i < parts.size(); ++i )
            compileOne( compiler, wxString::FromUTF8( unescape( parts[i] ).c_str() ), parts.size() == 2 );

        printf( "END\n" );
        fflush( stdout );
    }

    return 0;
}
