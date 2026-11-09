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
 * Oracle harness for the kicad-sexpr crate.
 *
 * This program links the *original* C++ sources that kicad-sexpr ports (common/dsnlexer.cpp,
 * common/richio.cpp, common/exceptions.cpp, common/io/kicad/kicad_io_utils.cpp, libs/sexpr and
 * the lexers TokenList2DsnLexer.cmake generates from the *.keywords files) and prints what
 * they do for given inputs.  build.sh builds it; generate_goldens.sh runs it to produce the
 * files in ../golden, which the Rust tests compare the port against.  Nothing here is needed
 * for a normal `cargo test`.
 *
 * Text that may contain control or non-ASCII bytes is printed escaped: every byte outside of
 * 0x20..0x7e, and the backslash, as \xHH.
 */

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include <fmt/format.h>

#include <dsnlexer.h>
#include <eda_item.h>
#include <io/kicad/kicad_io_utils.h>
#include <kiid.h>
#include <richio.h>
#include <sexpr/sexpr.h>
#include <sexpr/sexpr_parser.h>

#include <wx/mstream.h>

#include "drawing_sheet_lexer.h"
#include "drc_rules_lexer.h"
#include "embedded_files_lexer.h"
#include "lib_table_lexer.h"
#include "netlist_lexer.h"
#include "pcb_lexer.h"
#include "pcb_plot_params_lexer.h"
#include "specctra_lexer.h"
#include "stroke_params_lexer.h"
#include "template_fieldnames_lexer.h"


static std::string esc( const std::string& aText )
{
    std::string out;

    for( unsigned char c : aText )
    {
        if( c < 0x20 || c > 0x7e || c == '\\' )
        {
            char buf[8];
            snprintf( buf, sizeof( buf ), "\\x%02x", c );
            out += buf;
        }
        else
        {
            out += (char) c;
        }
    }

    return out;
}


static std::string unesc( const std::string& aText )
{
    std::string out;

    for( size_t i = 0; i < aText.size(); ++i )
    {
        if( aText[i] == '\\' && i + 3 < aText.size() && aText[i + 1] == 'x' )
        {
            out += (char) strtoul( aText.substr( i + 2, 2 ).c_str(), nullptr, 16 );
            i += 3;
        }
        else
        {
            out += aText[i];
        }
    }

    return out;
}


static uint64_t fnv1a( const std::string& aData, uint64_t aHash = 0xcbf29ce484222325ull )
{
    for( unsigned char c : aData )
    {
        aHash ^= c;
        aHash *= 0x100000001b3ull;
    }

    return aHash;
}


static std::string readFile( const std::string& aPath )
{
    std::ifstream in( aPath, std::ios::binary );

    if( !in )
    {
        fprintf( stderr, "cannot read %s\n", aPath.c_str() );
        exit( 2 );
    }

    std::stringstream buf;
    buf << in.rdbuf();
    return buf.str();
}


static std::string utf8( const wxString& aText )
{
    // Keep embedded NULs.
    wxScopedCharBuffer buf = aText.utf8_str();
    return std::string( buf.data(), buf.length() );
}


struct KEYWORD_TABLE
{
    const KEYWORD*     keywords;
    unsigned           count;
    const KEYWORD_MAP* map;
};


// Compiled with -fno-access-control, so the private tables of the generated lexers and the
// protected members of DSNLEXER are reachable.
static KEYWORD_TABLE keywordTable( const std::string& aName )
{
    static const KEYWORD empty[1] = {};

#define TABLE( name, cls ) \
    if( aName == name )    \
        return { cls::keywords, cls::keyword_count, &cls::keywords_hash };

    TABLE( "pcb_keys_t", PCB_LEXER )
    TABLE( "pcbplotparams_t", PCB_PLOT_PARAMS_LEXER )
    TABLE( "strokeparams_t", STROKE_PARAMS_LEXER )
    TABLE( "embedded_files_t", EMBEDDED_FILES_LEXER )
    TABLE( "drcrule_t", DRC_RULES_LEXER )
    TABLE( "lib_table_t", LIB_TABLE_LEXER )
    TABLE( "drawingsheet_t", DRAWING_SHEET_LEXER )
    TABLE( "nl_t", NETLIST_LEXER )
    TABLE( "tfield_t", TEMPLATE_FIELDNAMES_LEXER )
    TABLE( "dsn", SPECCTRA_LEXER )

    if( aName == "none" )
        return { empty, 0, nullptr };

    fprintf( stderr, "unknown keyword set %s\n", aName.c_str() );
    exit( 2 );
}


class ORACLE_LEXER : public DSNLEXER
{
public:
    ORACLE_LEXER( const KEYWORD_TABLE& aTable, const std::string& aText, const wxString& aSource ) :
            DSNLEXER( aTable.keywords, aTable.count, aTable.map, aText, aSource )
    {
    }

    ORACLE_LEXER( const KEYWORD_TABLE& aTable, LINE_READER* aReader ) :
            DSNLEXER( aTable.keywords, aTable.count, aTable.map, aReader )
    {
    }
};


static std::string tokenLine( DSNLEXER& aLexer, int aTok )
{
    return fmt::format( "tok\t{}\t{}\t{}\t{}\t{}", aTok, aLexer.CurOffset(), aLexer.CurLineNumber(),
                        esc( aLexer.CurStr() ), esc( aLexer.CurSeparator() ) );
}


static std::string errorLine( const IO_ERROR& aError )
{
    if( const PARSE_ERROR* pe = dynamic_cast<const PARSE_ERROR*>( &aError ) )
    {
        return fmt::format( "parse_error\t{}\t{}\t{}\t{}\t{}", esc( utf8( pe->What() ) ),
                            esc( utf8( const_cast<PARSE_ERROR*>( pe )->ParseProblem() ) ),
                            pe->lineNumber, pe->byteIndex, esc( pe->inputLine ) );
    }

    return fmt::format( "io_error\t{}", esc( utf8( aError.What() ) ) );
}


/// Dump all the tokens of a text; returns the dump.
static std::string lexDump( const std::string& aKeywords, bool aKnowsBar, const std::string& aText,
                            const std::string& aSource, long* aTokenCount = nullptr )
{
    ORACLE_LEXER lexer( keywordTable( aKeywords ), aText, wxString::FromUTF8( aSource ) );
    std::string  out;
    long         count = 0;

    lexer.SetKnowsBar( aKnowsBar );

    try
    {
        for( ;; )
        {
            int tok = lexer.NextTok();

            out += tokenLine( lexer, tok );
            out += '\n';
            count++;

            if( tok == DSN_EOF )
                break;
        }
    }
    catch( const IO_ERROR& e )
    {
        out += errorLine( e );
        out += '\n';
    }

    if( aTokenCount )
        *aTokenCount = count;

    return out;
}


static KICAD_FORMAT::FORMAT_MODE formatMode( const std::string& aName )
{
    if( aName == "normal" )
        return KICAD_FORMAT::FORMAT_MODE::NORMAL;

    if( aName == "compact" )
        return KICAD_FORMAT::FORMAT_MODE::COMPACT_TEXT_PROPERTIES;

    if( aName == "libtable" )
        return KICAD_FORMAT::FORMAT_MODE::LIBRARY_TABLE;

    fprintf( stderr, "unknown format mode %s\n", aName.c_str() );
    exit( 2 );
}


static std::string prettify( std::string aText, const std::string& aMode )
{
    KICAD_FORMAT::Prettify( aText, formatMode( aMode ) );
    return aText;
}


/// One text per (escaped) line: Prettify() in the three modes.
static int modePrettifyLines( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;

    while( std::getline( in, line ) )
    {
        if( !line.empty() && line[0] == '#' )
            continue;

        std::string text = unesc( line );

        std::cout << "> " << line << "\n";

        for( const char* mode : { "normal", "compact", "libtable" } )
            std::cout << mode << "\t" << esc( prettify( text, mode ) ) << "\n";
    }

    return 0;
}


static void dumpTree( const SEXPR::SEXPR* aNode, int aDepth, std::string& aOut )
{
    std::string indent( aDepth * 2, ' ' );

    if( aNode->IsList() )
    {
        aOut += fmt::format( "{}list line={} children={}\n", indent, aNode->GetLineNumber(),
                             aNode->GetNumberOfChildren() );

        for( SEXPR::SEXPR* child : *aNode->GetChildren() )
            dumpTree( child, aDepth + 1, aOut );
    }
    else if( aNode->IsString() )
    {
        aOut += fmt::format( "{}string line={} {}\n", indent, aNode->GetLineNumber(),
                             esc( aNode->GetString() ) );
    }
    else if( aNode->IsSymbol() )
    {
        aOut += fmt::format( "{}symbol line={} {}\n", indent, aNode->GetLineNumber(),
                             esc( aNode->GetSymbol() ) );
    }
    else if( aNode->IsInteger() )
    {
        aOut += fmt::format( "{}integer line={} {}\n", indent, aNode->GetLineNumber(),
                             aNode->GetLongInteger() );
    }
    else if( aNode->IsDouble() )
    {
        double   value = aNode->GetDouble();
        uint64_t bits;
        memcpy( &bits, &value, sizeof( bits ) );
        aOut += fmt::format( "{}double line={} {:016x}\n", indent, aNode->GetLineNumber(), bits );
    }
}


/// One s-expression text per (escaped) line: AsString() and a dump of the tree.
static int modeSexpr( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::cout << "> " << line << "\n";

        SEXPR::PARSER parser;

        try
        {
            std::unique_ptr<SEXPR::SEXPR> tree = parser.Parse( unesc( line ) );

            if( !tree )
            {
                std::cout << "null\n";
                continue;
            }

            std::string dump;
            dumpTree( tree.get(), 0, dump );
            std::cout << "as_string\t" << esc( tree->AsString() ) << "\n" << dump;
        }
        catch( const SEXPR::PARSE_EXCEPTION& e )
        {
            std::cout << "parse_exception\t" << e.what() << "\n";
        }
        catch( const SEXPR::INVALID_TYPE_EXCEPTION& e )
        {
            std::cout << "invalid_type_exception\t" << e.what() << "\n";
        }
    }

    return 0;
}


template <typename F>
static void attempt( const char* aLabel, F&& aFunc )
{
    std::cout << aLabel << "\t";

    try
    {
        aFunc();
    }
    catch( const SEXPR::INVALID_TYPE_EXCEPTION& e )
    {
        std::cout << "invalid_type_exception: " << e.what();
    }
    catch( const SEXPR::PARSE_EXCEPTION& e )
    {
        std::cout << "parse_exception: " << e.what();
    }
    catch( const std::invalid_argument& e )
    {
        std::cout << "invalid_argument: " << e.what();
    }

    std::cout << "\n";
}


/// The programmatic interface of the SEXPR tree on fixed inputs.
static int modeSexprApi()
{
    using namespace SEXPR;

    // The variadic constructor / AddChildren
    {
        std::string sym = "sym";
        std::string str2 = "str2";
        SEXPR_LIST  list( 1, (int64_t) 2, 3.5, "str", AsSymbol( (const std::string&) sym ),
                          AsString( (const std::string&) str2 ), (SEXPR::SEXPR*) new SEXPR_SYMBOL( "node" ) );

        std::cout << "variadic\t" << list.GetNumberOfChildren() << "\t" << list.GetLineNumber() << "\t"
                  << esc( list.AsString() ) << "\n";

        list.AddChildren( -7, 0.25 );
        std::cout << "add_children\t" << esc( list.AsString() ) << "\n";
    }

    // The stream insertion operators
    SEXPR_LIST list;

    {
        std::string name = "name";
        std::string text = "text";

        SEXPR_LIST* sub = new SEXPR_LIST( 7 );
        *sub << 2 << 42.42 << "substring";

        list << AsSymbol( (const std::string&) name ) << AsString( (const std::string&) text ) << 5
             << (int64_t) 1099511627776 << 1.5 << 2.5f << std::string( "plain" ) << sub
             << (SEXPR::SEXPR*) new SEXPR_DOUBLE( 0.125, 3 ) << 9;

        std::cout << "insert\t" << list.GetNumberOfChildren() << "\t" << esc( list.AsString() ) << "\n";
        std::cout << "insert_level\t" << esc( list.AsString( 2 ) ) << "\n";
        std::cout << "sub_line\t" << sub->GetLineNumber() << "\t" << list.GetChild( 8 )->GetLineNumber()
                  << "\n";
    }

    // The stream extraction operators
    {
        std::string symbol, string, plain;
        int32_t     i32 = 0;
        int64_t     i64 = 0;
        float       f32 = 0;
        double      f64 = 0;

        attempt( "extract_string_as_symbol", [&]() { list >> AsString( symbol ); } );
        attempt( "extract_symbol", [&]() { list >> AsSymbol( symbol ); std::cout << symbol; } );
        attempt( "extract_symbol_as_string", [&]() { list >> AsSymbol( string ); } );
        attempt( "extract_int_from_string", [&]() { list >> i32; } );
        attempt( "extract_long_from_string", [&]() { list >> i64; } );
        attempt( "extract_float_from_string", [&]() { list >> f32; } );
        attempt( "extract_double_from_string", [&]() { list >> f64; } );
        attempt( "extract_string", [&]() { list >> AsString( string ); std::cout << string; } );
        attempt( "extract_std_string_from_int", [&]() { list >> plain; } );
        attempt( "extract_double_from_int", [&]() { list >> f64; } );
        attempt( "extract_int", [&]() { list >> i32; std::cout << i32; } );
        attempt( "extract_long", [&]() { list >> i64; std::cout << i64; } );
        attempt( "extract_int_from_double", [&]() { list >> i32; } );
        attempt( "extract_double", [&]() { list >> f64; std::cout << f64; } );
        attempt( "extract_float", [&]() { list >> f32; std::cout << f32; } );
        attempt( "extract_std_string", [&]() { list >> plain; std::cout << plain; } );
    }

    // Scan (without int32_t* arguments: doScan() writes a double through those)
    {
        std::string symbol = "none", string = "none";
        double      number = -1;
        int64_t     big = -1;

        size_t n = list.Scan( "name", "text", &number, &big );
        std::cout << "scan\t" << n << "\t" << number << "\t" << big << "\n";

        n = list.Scan( &symbol, &string, &number );
        std::cout << "scan_strings\t" << n << "\t" << symbol << "\t" << string << "\t" << number << "\n";

        n = list.Scan( "name", "other", &number );
        std::cout << "scan_mismatch\t" << n << "\n";

        n = list.Scan( "name", &big );
        std::cout << "scan_wrong_type\t" << n << "\n";

        // a string comparison against a number is skipped, a string target keeps its value
        string = "kept";
        n = list.Scan( "name", "text", "five", &big, &string );
        std::cout << "scan_skipped\t" << n << "\t" << big << "\t" << string << "\n";

        const std::string expected = "name";
        n = list.Scan( &expected, std::string( "text" ) );
        std::cout << "scan_const_string\t" << n << "\n";
    }

    // The accessors on the wrong kind of node
    {
        SEXPR_INTEGER integer( 7 );
        SEXPR_DOUBLE  dbl( 2.5 );
        SEXPR_STRING  string( "s" );
        SEXPR_SYMBOL  symbol( "sym", 12 );

        std::cout << "values\t" << integer.GetLongInteger() << "\t" << integer.GetInteger() << "\t"
                  << integer.GetDouble() << "\t" << integer.GetFloat() << "\t" << dbl.GetDouble() << "\t"
                  << dbl.GetFloat() << "\t" << string.GetString() << "\t" << symbol.GetSymbol() << "\t"
                  << symbol.GetLineNumber() << "\t" << integer.GetLineNumber() << "\n";

        attempt( "double_as_integer", [&]() { dbl.GetLongInteger(); } );
        attempt( "double_as_int", [&]() { dbl.GetInteger(); } );
        attempt( "string_as_double", [&]() { string.GetDouble(); } );
        attempt( "string_as_float", [&]() { string.GetFloat(); } );
        attempt( "symbol_as_string", [&]() { symbol.GetString(); } );
        attempt( "string_as_symbol", [&]() { string.GetSymbol(); } );
        attempt( "symbol_line_as_symbol", [&]() { SEXPR_INTEGER( 1, 42 ).GetSymbol(); } );
        attempt( "integer_children", [&]() { integer.GetChildren(); } );
        attempt( "integer_child", [&]() { integer.GetChild( 0 ); } );
        attempt( "integer_number_of_children", [&]() { integer.GetNumberOfChildren(); } );
        attempt( "integer_add_child", [&]() { integer.AddChild( nullptr ); } );
        attempt( "integer_list", [&]() { integer.GetList(); } );

        SEXPR_INTEGER wide( 4294967301ll );
        std::cout << "wide\t" << wide.GetLongInteger() << "\t" << wide.GetInteger() << "\t" << wide.AsString()
                  << "\n";
    }

    return 0;
}


/// One token text per (escaped) line: the number classification and conversions.
static int modeNumbers( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;

    ORACLE_LEXER lexer( keywordTable( "none" ), std::string(), wxT( "numbers" ) );

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::string text = unesc( line );
        std::string dbl;

        lexer.curText = text;

        try
        {
            double   value = lexer.parseDouble();
            uint64_t bits;
            memcpy( &bits, &value, sizeof( bits ) );

            // The sign and payload of a NaN are of no interest.
            dbl = value != value ? std::string( "nan" ) : fmt::format( "{:016x}", bits );
        }
        catch( const IO_ERROR& )
        {
            dbl = "error";
        }

        std::cout << line << "\tis_number=" << DSNLEXER::isNumber( text.c_str(), text.c_str() + text.size() )
                  << "\tdouble=" << dbl
                  << "\tstrtol10=" << strtol( text.c_str(), nullptr, 10 )
                  << "\tstrtol16=" << strtol( text.c_str(), nullptr, 16 )
                  << "\tstrtoll0=" << strtoll( text.c_str(), nullptr, 0 ) << "\n";
    }

    return 0;
}


/// Lines "<g|f|e> <precision> <hex bits of the double>": printf and {fmt} conversions.
static int modePrintf( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        char               kind;
        int                precision;
        unsigned long long bits;

        if( sscanf( line.c_str(), "%c %d %llx", &kind, &precision, &bits ) != 3 )
        {
            fprintf( stderr, "bad printf line %s\n", line.c_str() );
            return 2;
        }

        double   value;
        uint64_t bits64 = bits;
        memcpy( &value, &bits64, sizeof( value ) );

        char cfmt[16];
        char buf[512];
        snprintf( cfmt, sizeof( cfmt ), "%%.%d%c", precision, kind );
        snprintf( buf, sizeof( buf ), cfmt, value );

        std::string fmtfmt = fmt::format( "{{:.{}{}}}", precision, kind );
        std::string viaFmt = fmt::format( fmt::runtime( fmtfmt ), value );

        std::cout << line << "\tprintf=" << buf << "\tfmt=" << viaFmt << "\n";
    }

    return 0;
}


/// One text per (escaped) line: OUTPUTFORMATTER::Quotes() and GetQuoteChar().
static int modeQuotes( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;
    STRING_FORMATTER   formatter;

    while( std::getline( in, line ) )
    {
        if( !line.empty() && line[0] == '#' )
            continue;

        std::string text = unesc( line );

        std::cout << line << "\tquotes=" << esc( formatter.Quotes( text ) )
                  << "\tquote_char=" << esc( formatter.GetQuoteChar( text.c_str() ) ) << "\n";
    }

    return 0;
}


/// The KICAD_FORMAT helpers and OUTPUTFORMATTER::Print on fixed inputs.
static int modeFormat()
{
    {
        STRING_FORMATTER out;
        KICAD_FORMAT::FormatBool( &out, wxT( "locked" ), true );
        KICAD_FORMAT::FormatBool( &out, wxT( "hide" ), false );
        KICAD_FORMAT::FormatOptBool( &out, wxT( "a" ), true );
        KICAD_FORMAT::FormatOptBool( &out, wxT( "b" ), false );
        KICAD_FORMAT::FormatOptBool( &out, wxT( "c" ), std::nullopt );
        std::cout << "bool\t" << esc( out.GetString() ) << "\n";
    }

    {
        STRING_FORMATTER out;
        KICAD_FORMAT::FormatUuid( &out, KIID( wxT( "0f1a2b3c-0000-4000-8000-00000000abcd" ) ) );
        std::cout << "uuid\t" << esc( out.GetString() ) << "\n";
    }

    {
        STRING_FORMATTER out;
        EDA_ITEM         item;
        KICAD_FORMAT::FormatCustomProperties( &out, item );
        item.m_props[wxT( "key" )] = wxT( "value" );
        item.m_props[wxT( "a \"b\"" )] = wxT( "line1\nline2" );
        KICAD_FORMAT::FormatCustomProperties( &out, item );
        std::cout << "custom_properties\t" << esc( out.GetString() ) << "\n";
    }

    for( size_t size : { 0, 1, 2, 3, 4, 56, 57, 58, 113, 114, 115, 300 } )
    {
        STRING_FORMATTER      out;
        std::vector<uint8_t>  data( size );

        for( size_t i = 0; i < size; ++i )
            data[i] = (uint8_t) ( i * 37 + 11 );

        wxMemoryOutputStream stream;
        stream.Write( data.data(), data.size() );
        KICAD_FORMAT::FormatStreamData( out, *stream.GetOutputStreamBuffer() );
        std::cout << "stream_data\t" << size << "\t" << esc( out.GetString() ) << "\n";
    }

    {
        STRING_FORMATTER out;
        int              a = out.Print( 3, "(net %d)", 42 );
        int              b = out.Print( "(%s %g %0.4f)", "x", 0.1, 2.5 );
        int              c = out.Print( 0, "%s", "" );
        int              d = out.Indent( 2 );
        std::cout << "print\t" << a << "\t" << b << "\t" << c << "\t" << d << "\t" << esc( out.GetString() )
                  << "\n";
        out.StripUseless();
        std::cout << "strip_useless\t" << esc( out.GetString() ) << "\n";
    }

    // The exception classes
    {
        auto describe = [&]( const char* aLabel, const IO_ERROR& aError )
        {
            std::cout << aLabel << "\t" << errorLine( aError ) << "\n";
        };

        try
        {
            THROW_IO_ERROR( wxT( "Line length exceeded" ) );
        }
        catch( const IO_ERROR& e )
        {
            describe( "io_error", e );
        }

        try
        {
            THROW_IO_CANCELLED();
        }
        catch( const IO_ERROR& e )
        {
            describe( "io_cancelled", e );
        }

        try
        {
            THROW_IO_CANCELLED_MSG( wxT( "stop" ) );
        }
        catch( const IO_ERROR& e )
        {
            describe( "io_cancelled_msg", e );
        }

        FUTURE_FORMAT_ERROR versionOnly( wxT( "20250101" ) );
        describe( "future_format", versionOnly );

        FUTURE_FORMAT_ERROR withGenerator( wxT( "20250101" ), wxT( "10.0" ) );
        describe( "future_format_generator", withGenerator );
        std::cout << "future_format_fields\t" << utf8( withGenerator.requiredVersion ) << "\t"
                  << utf8( withGenerator.requiredGenerator ) << "\n";

        try
        {
            THROW_PARSE_ERROR( wxT( "Unexpected 'foo'" ), wxT( "f.kicad_pcb" ), "the line\n", 3, 9 );
        }
        catch( const PARSE_ERROR& pe )
        {
            describe( "parse_error", pe );

            FUTURE_FORMAT_ERROR fromParse( pe, wxT( "20250101" ) );
            describe( "future_format_from_parse", fromParse );

            FUTURE_FORMAT_ERROR fromParseGenerator( pe, wxT( "20250101" ), wxT( "10.0" ) );
            describe( "future_format_from_parse_generator", fromParseGenerator );

            FUTURE_FORMAT_ERROR again( fromParseGenerator, wxT( "1" ), wxT( "2" ) );
            describe( "future_format_from_future_format", again );
            std::cout << "future_format_from_future_format_fields\t" << utf8( again.requiredVersion ) << "\t"
                      << utf8( again.requiredGenerator ) << "\n";
        }
    }

    return 0;
}


static int modeKeywords( const std::string& aName )
{
    KEYWORD_TABLE table = keywordTable( aName );

    for( unsigned i = 0; i < table.count; ++i )
        std::cout << table.keywords[i].token << "\t" << table.keywords[i].name << "\n";

    return 0;
}


static const char* keywordsForFile( const std::string& aPath )
{
    auto endsWith = [&]( const char* aSuffix )
    {
        size_t len = strlen( aSuffix );
        return aPath.size() >= len && aPath.compare( aPath.size() - len, len, aSuffix ) == 0;
    };

    if( endsWith( ".kicad_pcb" ) || endsWith( ".kicad_mod" ) )
        return "pcb_keys_t";

    if( endsWith( ".kicad_dru" ) )
        return "drcrule_t";

    if( endsWith( "-lib-table" ) )
        return "lib_table_t";

    if( endsWith( ".kicad_wks" ) )
        return "drawingsheet_t";

    return "none";
}


/// For every file named in the list (paths relative to aRoot): hashes of the input, of the
/// token dump and of the prettified text.
static int modeCorpus( const std::string& aListFile, const std::string& aRoot )
{
    std::istringstream in( readFile( aListFile ) );
    std::string        line;

    std::cout << "# path\tinput_fnv1a\ttoken_count\ttokens_fnv1a\tprettify_normal_fnv1a\t"
                 "prettify_compact_fnv1a\tprettify_normal_is_identity\n";

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::string text = readFile( aRoot + "/" + line );
        long        tokens = 0;
        std::string dump = lexDump( keywordsForFile( line ), true, text, line, &tokens );
        std::string normal = prettify( text, "normal" );
        std::string compact = prettify( text, "compact" );

        std::cout << fmt::format( "{}\t{:016x}\t{}\t{:016x}\t{:016x}\t{:016x}\t{}\n", line, fnv1a( text ),
                                  tokens, fnv1a( dump ), fnv1a( normal ), fnv1a( compact ),
                                  normal == text ? 1 : 0 );
    }

    return 0;
}


/// Interpreter for the lexer scripts, see ../golden/lexer_script.txt.
static int modeScript( const std::string& aFile )
{
    std::istringstream in( readFile( aFile ) );
    std::string        line;

    std::unique_ptr<ORACLE_LEXER> mainLexer;
    std::unique_ptr<ORACLE_LEXER> subLexer;
    std::string                   source = "src";

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        std::cout << "> " << line << "\n";

        std::string cmd = line.substr( 0, line.find( ' ' ) );
        std::string arg = line.find( ' ' ) == std::string::npos ? std::string()
                                                                : line.substr( line.find( ' ' ) + 1 );

        if( cmd == "source" )
        {
            source = unesc( arg );
            continue;
        }

        if( cmd == "lexer_repeat" )
        {
            // lexer_repeat <keywords> <count> <text>: the text repeated <count> times
            std::istringstream fields( arg );
            std::string        kw, piece;
            size_t             count = 0;

            fields >> kw >> count >> piece;
            piece = unesc( piece );

            std::string text;
            text.reserve( piece.size() * count );

            for( size_t i = 0; i < count; ++i )
                text += piece;

            subLexer.reset();
            mainLexer = std::make_unique<ORACLE_LEXER>( keywordTable( kw ), text, wxString::FromUTF8( source ) );
            continue;
        }

        if( cmd == "lexer" || cmd == "lexer_no_reader" )
        {
            std::string kw = arg.substr( 0, arg.find( ' ' ) );
            std::string text = arg.find( ' ' ) == std::string::npos ? std::string()
                                                                    : unesc( arg.substr( arg.find( ' ' ) + 1 ) );

            subLexer.reset();
            mainLexer.reset();

            if( cmd == "lexer" )
                mainLexer = std::make_unique<ORACLE_LEXER>( keywordTable( kw ), text, wxString::FromUTF8( source ) );
            else
                mainLexer = std::make_unique<ORACLE_LEXER>( keywordTable( kw ), nullptr );

            continue;
        }

        if( !mainLexer )
        {
            fprintf( stderr, "no lexer for: %s\n", line.c_str() );
            return 2;
        }

        ORACLE_LEXER* lexer = subLexer ? subLexer.get() : mainLexer.get();

        try
        {
            if( cmd == "specctra" )
            {
                lexer->SetSpecctraMode( arg == "1" );
            }
            else if( cmd == "knowsbar" )
            {
                lexer->SetKnowsBar( arg == "1" );
            }
            else if( cmd == "comments" )
            {
                std::cout << "old\t" << lexer->SetCommentsAreTokens( arg == "1" ) << "\n";
            }
            else if( cmd == "delim" )
            {
                std::cout << "old\t" << (int) (unsigned char) lexer->SetStringDelimiter( unesc( arg )[0] ) << "\n";
            }
            else if( cmd == "spaceinquoted" )
            {
                std::cout << "old\t" << lexer->SetSpaceInQuotedTokens( arg == "1" ) << "\n";
            }
            else if( cmd == "next" )
            {
                int tok = lexer->NextTok();
                std::cout << tokenLine( *lexer, tok ) << "\n";
            }
            else if( cmd == "nexttok" )
            {
                // Only the token: usable on a lexer without line reader, whose position
                // accessors dereference a null pointer in C++.
                int tok = lexer->NextTok();
                std::cout << "token\t" << tok << "\n";
            }
            else if( cmd == "all" )
            {
                for( int i = 0; i < 100000; ++i )
                {
                    int tok = lexer->NextTok();
                    std::cout << tokenLine( *lexer, tok ) << "\n";

                    if( tok == DSN_EOF )
                        break;
                }
            }
            else if( cmd == "needleft" )
            {
                lexer->NeedLEFT();
                std::cout << "ok\n";
            }
            else if( cmd == "needright" )
            {
                lexer->NeedRIGHT();
                std::cout << "ok\n";
            }
            else if( cmd == "needbar" )
            {
                lexer->NeedBAR();
                std::cout << "ok\n";
            }
            else if( cmd == "needsymbol" )
            {
                int tok = lexer->NeedSYMBOL();
                std::cout << tokenLine( *lexer, tok ) << "\n";
            }
            else if( cmd == "needsymbolornumber" )
            {
                int tok = lexer->NeedSYMBOLorNUMBER();
                std::cout << tokenLine( *lexer, tok ) << "\n";
            }
            else if( cmd == "neednumber" )
            {
                int tok = lexer->NeedNUMBER( unesc( arg ).c_str() );
                std::cout << tokenLine( *lexer, tok ) << "\n";
            }
            else if( cmd == "expecting" )
            {
                lexer->Expecting( atoi( arg.c_str() ) );
            }
            else if( cmd == "expectingstr" )
            {
                lexer->Expecting( unesc( arg ).c_str() );
            }
            else if( cmd == "unexpected" )
            {
                lexer->Unexpected( atoi( arg.c_str() ) );
            }
            else if( cmd == "unexpectedstr" )
            {
                lexer->Unexpected( unesc( arg ).c_str() );
            }
            else if( cmd == "duplicate" )
            {
                lexer->Duplicate( atoi( arg.c_str() ) );
            }
            else if( cmd == "state" )
            {
                std::cout << fmt::format( "state\tcur={}\tprev={}\ttext={}\toffset={}\tline={}\tsource={}\t"
                                          "curline={}\tsep={}\n",
                                          lexer->CurTok(), lexer->PrevTok(), esc( lexer->CurStr() ),
                                          lexer->CurOffset(), lexer->CurLineNumber(),
                                          esc( utf8( lexer->CurSource() ) ), esc( lexer->CurLine() ),
                                          esc( lexer->CurSeparator() ) );
            }
            else if( cmd == "readcomments" )
            {
                std::unique_ptr<wxArrayString> comments( lexer->ReadCommentLines() );

                if( !comments )
                {
                    std::cout << "comments\tnone\n";
                }
                else
                {
                    std::cout << "comments\t" << comments->GetCount() << "\n";

                    for( const wxString& comment : *comments )
                        std::cout << "\t" << esc( utf8( comment ) ) << "\n";
                }
            }
            else if( cmd == "parsedouble" || cmd == "parsedoubleexp" || cmd == "parsedoubletok" )
            {
                double value;

                if( cmd == "parsedouble" )
                    value = lexer->parseDouble();
                else if( cmd == "parsedoubleexp" )
                    value = lexer->parseDouble( unesc( arg ).c_str() );
                else
                    value = lexer->parseDouble( atoi( arg.c_str() ) );

                uint64_t bits;
                memcpy( &bits, &value, sizeof( bits ) );
                std::cout << fmt::format( "double\t{:016x}\n", bits );
            }
            else if( cmd == "parseint" )
            {
                // PCB_IO_KICAD_SEXPR_PARSER::parseInt()
                std::cout << "int\t" << (int) strtol( lexer->CurText(), nullptr, 10 ) << "\n";
            }
            else if( cmd == "parseintexp" )
            {
                // PCB_IO_KICAD_SEXPR_PARSER::parseInt( const char* aExpected )
                lexer->NeedNUMBER( unesc( arg ).c_str() );
                std::cout << "int\t" << (int) strtol( lexer->CurText(), nullptr, 10 ) << "\n";
            }
            else if( cmd == "parsehex" )
            {
                // PCB_IO_KICAD_SEXPR_PARSER::parseHex()
                lexer->NextTok();
                std::cout << "long\t" << strtol( lexer->CurText(), nullptr, 16 ) << "\n";
            }
            else if( cmd == "tokentext" )
            {
                std::cout << "text\t" << esc( lexer->GetTokenText( atoi( arg.c_str() ) ) ) << "\n";
            }
            else if( cmd == "tokenstring" )
            {
                std::cout << "text\t" << esc( utf8( lexer->GetTokenString( atoi( arg.c_str() ) ) ) ) << "\n";
            }
            else if( cmd == "curstrastoken" )
            {
                std::cout << "token\t" << lexer->GetCurStrAsToken() << "\n";
            }
            else if( cmd == "issymbol" )
            {
                std::cout << "is_symbol\t" << DSNLEXER::IsSymbol( atoi( arg.c_str() ) ) << "\tis_number\t"
                          << DSNLEXER::IsNumber( atoi( arg.c_str() ) ) << "\n";
            }
            else if( cmd == "fromutf8" )
            {
                std::cout << "text\t" << esc( utf8( lexer->FromUTF8() ) ) << "\n";
            }
            else if( cmd == "push" )
            {
                // Owned by the lexer if it owns its readers, leaked otherwise (and when popped).
                lexer->PushReader( new STRING_LINE_READER( unesc( arg ), wxT( "pushed" ) ) );
            }
            else if( cmd == "pop" )
            {
                LINE_READER* popped = lexer->PopReader();
                std::cout << "popped\t" << ( popped ? esc( utf8( popped->GetSource() ) ) : std::string( "null" ) )
                          << "\n";
            }
            else if( cmd == "init" )
            {
                lexer->InitParserState();
            }
            else if( cmd == "sub" )
            {
                // SUB_PARSER sub( reader ); sub.SyncLineReaderWith( *this );
                subLexer = std::make_unique<ORACLE_LEXER>( keywordTable( arg ), mainLexer->reader );
                std::cout << "sync\t" << subLexer->SyncLineReaderWith( *mainLexer ) << "\n";
            }
            else if( cmd == "endsub" )
            {
                // SyncLineReaderWith( sub );
                std::cout << "sync\t" << mainLexer->SyncLineReaderWith( *subLexer ) << "\n";
                subLexer.reset();
            }
            else if( cmd == "dropsub" )
            {
                // The sub-lexer goes away without handing its position back (an exception left
                // the scope in C++).
                subLexer.reset();
            }
            else
            {
                fprintf( stderr, "unknown command: %s\n", line.c_str() );
                return 2;
            }
        }
        catch( const IO_ERROR& e )
        {
            std::cout << errorLine( e ) << "\n";
        }
    }

    return 0;
}


int main( int argc, char** argv )
{
    std::vector<std::string> args( argv + 1, argv + argc );

    if( args.size() == 2 && args[0] == "script" )
        return modeScript( args[1] );

    if( args.size() == 4 && args[0] == "lex" )
    {
        std::cout << lexDump( args[1], args[2] == "1", readFile( args[3] ), "file" );
        return 0;
    }

    if( args.size() == 3 && args[0] == "prettify" )
    {
        std::cout << prettify( readFile( args[2] ), args[1] );
        return 0;
    }

    if( args.size() == 2 && args[0] == "prettifylines" )
        return modePrettifyLines( args[1] );

    if( args.size() == 2 && args[0] == "sexpr" )
        return modeSexpr( args[1] );

    if( args.size() == 1 && args[0] == "sexprapi" )
        return modeSexprApi();

    if( args.size() == 2 && args[0] == "numbers" )
        return modeNumbers( args[1] );

    if( args.size() == 2 && args[0] == "printf" )
        return modePrintf( args[1] );

    if( args.size() == 2 && args[0] == "quotes" )
        return modeQuotes( args[1] );

    if( args.size() == 1 && args[0] == "format" )
        return modeFormat();

    if( args.size() == 2 && args[0] == "keywords" )
        return modeKeywords( args[1] );

    if( args.size() == 3 && args[0] == "corpus" )
        return modeCorpus( args[1], args[2] );

    fprintf( stderr,
             "usage: oracle script <file>\n"
             "       oracle lex <keyword set> <knows bar: 0|1> <file>\n"
             "       oracle prettify <normal|compact|libtable> <file>\n"
             "       oracle prettifylines <file>\n"
             "       oracle sexpr <file>\n"
             "       oracle sexprapi\n"
             "       oracle numbers <file>\n"
             "       oracle printf <file>\n"
             "       oracle quotes <file>\n"
             "       oracle format\n"
             "       oracle keywords <keyword set>\n"
             "       oracle corpus <list file> <root dir>\n" );
    return 2;
}
