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

// Differential oracle for work package C3 (kicad-core): links libkicommon.so of the reference
// build and prints
//   - every EDA_PATTERN_MATCH class and EDA_COMBINED_MATCHER (all contexts, Find, StartsWith,
//     ScoreTerms) on tests/oracle/c3_patterns.txt plus the netclass patterns of every
//     .kicad_pro file in the list, against tests/oracle/c3_subjects.txt;
//   - ExpandTextVars / ResolveTextVars (without math) / FinalizeTextVarExpansion;
//   - JSON_SETTINGS + PARAM<T> loading of a table of documents.
// Replayed by tests/test_c3_pattern_oracle.rs.  Output format as json_oracle.cpp.
//
//   pattern_oracle <kicad source root> <file list> [full]

#include <base_units.h>
#include <common.h>
#include <eda_pattern_match.h>
#include <settings/json_settings.h>
#include <settings/json_settings_internals.h>
#include <settings/nested_settings.h>
#include <settings/parameters.h>

#include <nlohmann/json.hpp>

#include <wx/init.h>
#include <wx/log.h>

#include <algorithm>
#include <cinttypes>
#include <clocale>
#include <cstring>
#include <fstream>
#include <functional>
#include <map>
#include <optional>
#include <iostream>
#include <set>
#include <sstream>
#include <string>
#include <vector>

static uint64_t fnvBytes( const std::string& s, uint64_t h = 1469598103934665603ULL )
{
    for( unsigned char c : s )
    {
        h ^= c;
        h *= 1099511628211ULL;
    }
    return h;
}

static std::string hex64( uint64_t v )
{
    char buf[32];
    snprintf( buf, sizeof buf, "%016" PRIx64, v );
    return buf;
}

static bool                     g_full = false;
static std::string              g_section;
static std::vector<std::string> g_lines;

static void flush()
{
    if( g_section.empty() )
        return;

    uint64_t h = 1469598103934665603ULL;

    for( size_t i = 0; i < g_lines.size(); ++i )
    {
        h = fnvBytes( g_lines[i] + "\n", h );

        if( g_full || i < 8 || g_section == "deviations" )
            std::cout << "E " << g_section << " " << g_lines[i] << "\n";
    }

    std::cout << "S " << g_section << " " << g_lines.size() << " " << hex64( h ) << "\n";
    g_lines.clear();
}

static void begin( const std::string& name )
{
    flush();
    g_section = name;
}

static void out( const std::string& line ) { g_lines.push_back( line ); }

static std::string esc( const std::string& s )
{
    std::string o;
    for( unsigned char c : s )
    {
        if( c < 0x20 || c >= 0x7f || c == '\\' )
        {
            char buf[8];
            snprintf( buf, sizeof buf, "\\x%02X", c );
            o += buf;
        }
        else
            o += (char) c;
    }
    return o;
}

static std::string esc( const wxString& s ) { return esc( std::string( s.ToUTF8() ) ); }

static std::string res( const EDA_PATTERN_MATCH::FIND_RESULT& r )
{
    if( !r )
        return "-";
    return std::to_string( r.start ) + "," + std::to_string( r.length );
}

template <typename M>
static std::string matcherLine( const char* name, const wxString& pattern,
                                const std::vector<wxString>& subjects )
{
    M    m;
    bool ok = m.SetPattern( pattern );
    std::string line = std::string( name ) + " " + esc( pattern ) + " " + ( ok ? "ok" : "fail" ) + " ["
                       + esc( m.GetPattern() ) + "]";

    for( const wxString& s : subjects )
        line += " " + res( m.Find( s ) );

    return line;
}


// KiROUND overflow logging lives in libkimath.a, which this driver does not link.
void kimathLogOverflow( double, const char* ) {}

// ---- JSON_SETTINGS / PARAM mechanics ----

enum class TEST_ENUM
{
    A = 0,
    B = 1,
    C = 2,
    D = 3
};

static std::string hexd( double v )
{
    uint64_t b;
    memcpy( &b, &v, 8 );
    return hex64( b );
}

static std::string hexf( float v )
{
    uint32_t b;
    memcpy( &b, &v, 4 );
    char buf[16];
    snprintf( buf, sizeof buf, "%08x", b );
    return buf;
}

class TEST_SETTINGS : public JSON_SETTINGS
{
public:
    TEST_SETTINGS( bool aReset ) :
            JSON_SETTINGS( "test", SETTINGS_LOC::NONE, 2, false, false, false )
    {
        m_resetParamsIfMissing = aReset;

        m_params.emplace_back( new PARAM<bool>( "b", &b, false ) );
        m_params.emplace_back( new PARAM<int>( "i", &i, 5 ) );
        m_params.emplace_back( new PARAM<int>( "ir", &ir, 10, 0, 100 ) );
        m_params.emplace_back( new PARAM<double>( "d", &d, 1.5 ) );
        m_params.emplace_back( new PARAM<double>( "dr", &dr, 0.5, 0.0, 1.0 ) );
        m_params.emplace_back( new PARAM<wxString>( "s", &s, "def" ) );
        m_params.emplace_back( new PARAM<unsigned int>( "u", &u, 7 ) );
        m_params.emplace_back( new PARAM<float>( "f", &f, 2.5f ) );
        m_params.emplace_back( new PARAM_ENUM<TEST_ENUM>( "e", &e, TEST_ENUM::B, TEST_ENUM::A, TEST_ENUM::C ) );
        m_params.emplace_back( new PARAM_SCALED<int>( "sc", &sc, 250000, -1000000000, 1000000000,
                                                      pcbIUScale.MM_PER_IU ) );
        m_params.emplace_back( new PARAM_SCALED<int>( "mils", &mils, 100, 0.001 ) );
        m_params.emplace_back( new PARAM_SCALED<long>( "sl", &sl, 5, 1.0 / 1000.0 ) );
        m_params.emplace_back( new PARAM_LIST<int>( "li", &li, { 1, 2 } ) );
        m_params.emplace_back( new PARAM_LIST<wxString>( "ls", &ls, { "x" } ) );
        m_params.emplace_back( new PARAM_LIST<double>( "ld", &ld, { 0.5 }, true ) );
        m_params.emplace_back( new PARAM_PATH_LIST( "lp", &lp, { "a\\b" } ) );
        m_params.emplace_back( new PARAM_SET<wxString>( "set", &set, { "q" } ) );
        m_params.emplace_back( new PARAM_MAP<int>( "mi", &mi, { { "k", 1 } } ) );
        m_params.emplace_back( new PARAM_MAP<double>( "md", &md, { { "k", 1.5 } } ) );
        m_params.emplace_back( new PARAM_WXSTRING_MAP( "ms", &ms, { { "k", "v" } } ) );
        m_params.emplace_back( new PARAM_LAMBDA<int>( "lam",
                [this]() { return lam; },
                [this]( int v ) { lam = v * 2; }, 9 ) );
        m_params.emplace_back( new PARAM_LAMBDA<nlohmann::json>( "lj",
                [this]() { return lj; },
                [this]( nlohmann::json v )
                {
                    if( v.is_object() || v.is_array() )
                        lj = v;
                    else
                        lj = nlohmann::json( "set:" + v.dump() );
                },
                {} ) );
        m_params.emplace_back( new PARAM_PATH( "path", &path, "c:\\x" ) );
        m_params.emplace_back( new PARAM<wxString>( "nested.deep.s", &ns, "n" ) );

        registerMigration( 0, 1,
                [this]() -> bool
                {
                    if( std::optional<int> v = Get<int>( "old" ) )
                        Set<int>( "i", *v );

                    return true;
                } );

        registerMigration( 1, 2,
                [this]() -> bool
                {
                    Set<wxString>( "migrated", "yes" );
                    return !Contains( "fail_migration" );
                } );
    }

    std::string FileContents()
    {
        try
        {
            return esc( formatFileContents() );
        }
        catch( const nlohmann::json::exception& e )
        {
            return "throw " + std::to_string( e.id );
        }
    }
    bool        Future() const { return m_isFutureFormat; }

    std::string Dump() const
    {
        std::string o = std::string( "b=" ) + ( b ? "1" : "0" ) + " i=" + std::to_string( i )
                        + " ir=" + std::to_string( ir ) + " d=" + hexd( d ) + " dr=" + hexd( dr )
                        + " s=" + esc( s ) + " u=" + std::to_string( u ) + " f=" + hexf( f )
                        + " e=" + std::to_string( (int) e ) + " sc=" + std::to_string( sc )
                        + " mils=" + std::to_string( mils ) + " sl=" + std::to_string( sl ) + " li=";
        for( int x : li )
            o += std::to_string( x ) + ";";
        o += " ls=";
        for( const wxString& x : ls )
            o += esc( x ) + ";";
        o += " ld=";
        for( double x : ld )
            o += hexd( x ) + ";";
        o += " lp=";
        for( const wxString& x : lp )
            o += esc( x ) + ";";
        o += " set=";
        for( const wxString& x : set )
            o += esc( x ) + ";";
        o += " mi=";
        for( const auto& [k, v] : mi )
            o += esc( k ) + ":" + std::to_string( v ) + ";";
        o += " md=";
        for( const auto& [k, v] : md )
            o += esc( k ) + ":" + hexd( v ) + ";";
        o += " ms=";
        for( const auto& [k, v] : ms )
            o += esc( k ) + ":" + esc( v ) + ";";
        o += " lam=" + std::to_string( lam ) + " lj=" + esc( lj.dump() ) + " path=" + esc( path )
             + " ns=" + esc( ns );
        return o;
    }

    bool                            b = true;
    int                             i = -77, ir = -77;
    double                          d = -7.25, dr = -7.25;
    wxString                        s = "sentinel";
    unsigned int                    u = 77;
    float                           f = -7.5f;
    TEST_ENUM                       e = TEST_ENUM::D;
    int                             sc = -77, mils = -77;
    long                            sl = -77;
    std::vector<int>                li{ 77 };
    std::vector<wxString>           ls{ "sentinel" };
    std::vector<double>             ld{ 7.0 };
    std::vector<wxString>           lp{ "sentinel" };
    std::set<wxString>              set{ "sentinel" };
    std::map<std::string, int>      mi{ { "sentinel", 77 } };
    std::map<std::string, double>   md{ { "sentinel", 7.0 } };
    std::map<wxString, wxString>    ms{ { "sentinel", "77" } };
    int                             lam = -77;
    nlohmann::json                  lj = "sentinel";
    wxString                        path = "sentinel";
    wxString                        ns = "sentinel";
};

class TEST_NESTED : public NESTED_SETTINGS
{
public:
    TEST_NESTED( JSON_SETTINGS* aParent, bool aReset ) :
            NESTED_SETTINGS( "nested", 1, aParent, "sub", false )
    {
        m_resetParamsIfMissing = aReset;
        m_params.emplace_back( new PARAM<int>( "a", &a, 3 ) );
        m_params.emplace_back( new PARAM_MAP<int>( "m", &m, { { "z", 26 } } ) );

        registerMigration( 0, 1,
                [this]() -> bool
                {
                    Set<int>( "a", 100 );

                    if( Contains( "throw" ) )
                        throw std::runtime_error( "migration" );

                    return true;
                } );

        loaded = LoadFromFile();
    }

    ~TEST_NESTED() override { m_parent = nullptr; }

    int                        a = -77;
    std::map<std::string, int> m{ { "sentinel", 77 } };
    bool                       loaded = false;
};

class TEST_PARENT : public JSON_SETTINGS
{
public:
    TEST_PARENT() : JSON_SETTINGS( "parent", SETTINGS_LOC::NONE, 0, false, false, false ) {}
};

static const char* SETTINGS_DOCS[] = {
    "",
    "{}",
    "not json",
    R"({"meta":{"version":2},"b":true,"i":12,"ir":50,"d":0.25,"dr":0.75,"s":"hello","u":4000000000,"f":0.1,
        "e":2,"sc":1.27,"mils":0.5,"sl":2.5,"li":[3,4,5],"ls":["p","q"],"ld":[1.5,2.5],"lp":["x\\y","z"],
        "set":["b","a","b"],"mi":{"y":2,"x":1},"md":{"m":0.5},"ms":{"k2":"v2"},"lam":4,"lj":{"q":[1]},
        "path":"d:\\p","nested":{"deep":{"s":"deep"}}})",
    R"({"meta":{"version":2},"b":1,"i":"5","ir":true,"d":true,"dr":"x","s":5,"u":-1,"f":true,"e":7,
        "sc":"x","mils":null,"sl":[],"li":[1,"a"],"ls":"notarray","ld":[],"lp":[1],"set":[1],"mi":{"a":"x"},
        "md":[],"ms":{"a":1},"lam":true,"lj":[1],"path":7,"nested":"str"})",
    R"({"meta":{"version":2},"ir":1000,"dr":2.0,"e":-1,"sc":5000,"sl":1e30,"mils":-3000000,"u":1e10,
        "i":3.9,"li":[1.9,-2.9,true],"ld":[true]})",
    R"({"meta":{"version":2},"i":-3.9,"ir":-0.5,"u":3e9,"e":2.7,"sc":0.0000005,"mils":0.0015,"sl":-2.5,
        "d":5,"dr":-0.0,"f":1e40,"lam":2.9,"ir2":4})",
    R"({"meta":{"version":2},"b":null,"s":null,"li":null,"mi":null,"lj":null,"lam":null,"ms":null,
        "set":null,"path":null,"ld":null})",
    R"({"meta":{"version":0},"old":42,"b":false})",
    R"({"meta":{"version":1},"old":42,"i":8})",
    R"({"meta":{"version":0},"old":"x","fail_migration":true})",
    R"({"meta":{"version":3},"i":99})",
    R"({"meta":{"version":"2"},"i":98})",
    R"({"meta":{"version":-1},"i":97})",
    R"({"meta":{"version":1.9},"i":96})",
    R"({"i":95})",
    "\xEF\xBB\xBF// comment\n{\"meta\":{\"version\":2},/* c */\"i\":1,\"i\":2,\"s\":\"a\\u00e9\\n\"}",
    R"({"meta":{"version":2},"mi":{"b":1,"a":2.5,"c":"x","d":4},"ms":{"b":"1","a":2,"c":"3"},"li":[1,2,"x"]})",
    R"({"meta":{"version":2},"nested":{"deep":{"s":["x"]}},"set":["z","é","a"],"lp":["a\\b\\c"]})",
    R"([1,2,3])",
    R"("string")",
    R"({"meta":{"version":2},"sc":-1.0000005,"mils":-0.0005,"sl":9.3e15,"u":-2.5,"f":-1e-46})",
};

static const char* NESTED_DOCS[] = {
    "{}",
    R"({"sub":{"meta":{"version":1},"a":5,"m":{"q":1}}})",
    R"({"sub":{"meta":{"version":0},"a":5}})",
    R"({"sub":{"meta":{"version":0},"a":5,"throw":1}})",
    R"({"sub":{"a":7}})",
    R"({"sub":"notobject"})",
    R"({"sub":null})",
    R"({"sub":{"meta":{"version":2},"a":"x","m":[]}})",
    R"({"sub":{"meta":{"version":1},"a":3,"m":{"z":26}},"other":1})",
};

// ---- ExpandTextVars ----

static bool testResolver( wxString* token )
{
    static const std::map<wxString, wxString> vars = {
        { "VAR", "value" }, { "X", "5" }, { "Y", "2" }, { "NESTED", "${VAR}" }, { "R", "${R}" },
        { "A B", "spaced" }, { "ERC_WARNING x", "assert" }, { "value", "VAL" }, { "5", "five" },
        { "{", "brace" }, { "E", "\\${X}" }
    };

    auto it = vars.find( *token );

    if( it == vars.end() )
        return false;

    *token = it->second;
    return true;
}

static const char* TEXT_FRAGS[] = { "${", "}", "\\", "VAR", "X", "Y", "$", "{", "<<<ESC_DOLLAR:",
                                    "<<<ESC_AT:", "ERC_WARNING x", "abc", " ", "NESTED", "R", "A B",
                                    "@{", "\\${", "\\@{", "DRC_ERROR", "é", "E", "5", "}}" };

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 1;

    // KiCad runs with a UTF-8 locale: towlower / towupper (wxString::Lower, Upper) map
    // non-ASCII letters only then.
    setlocale( LC_ALL, "C.UTF-8" );

    wxInitializer init;
    wxLog::EnableLogging( false );

    std::string root = argv[1];
    g_full = argc > 3 && std::string( argv[3] ) == "full";

    std::vector<std::string> patterns;
    {
        std::ifstream f( root + "/rust/crates/kicad-core/tests/oracle/c3_patterns.txt" );
        std::string   line;
        while( std::getline( f, line ) )
            patterns.push_back( line );
    }

    // Net class patterns of every project file, in file order, first occurrence only.
    {
        std::ifstream         list( argv[2] );
        std::string           rel;
        std::set<std::string> seen( patterns.begin(), patterns.end() );

        while( std::getline( list, rel ) )
        {
            if( rel.size() < 10 || rel.substr( rel.size() - 10 ) != ".kicad_pro" )
                continue;

            std::ifstream     f( root + "/" + rel, std::ios::binary );
            std::stringstream ss;
            ss << f.rdbuf();

            try
            {
                nlohmann::json j = nlohmann::json::parse( ss.str(), nullptr, true, true );
                const nlohmann::json& a = j.at( "net_settings" ).at( "netclass_patterns" );

                for( const nlohmann::json& e : a )
                {
                    std::string p = e.at( "pattern" ).get<std::string>();

                    if( seen.insert( p ).second )
                        patterns.push_back( p );
                }
            }
            catch( ... )
            {
            }
        }
    }

    // Patterns PCRE2 accepts but the port rejects (PORT_STATUS.md): reported separately.
    std::vector<std::string> deviations;
    {
        std::ifstream f( root + "/rust/crates/kicad-core/tests/oracle/c3_pattern_deviations.txt" );
        std::string   line;
        while( std::getline( f, line ) )
            deviations.push_back( line );

        std::vector<std::string> kept;
        for( const std::string& p : patterns )
            if( std::find( deviations.begin(), deviations.end(), p ) == deviations.end() )
                kept.push_back( p );
        patterns = kept;
    }

    std::vector<wxString> subjects;
    {
        std::ifstream f( root + "/rust/crates/kicad-core/tests/oracle/c3_subjects.txt" );
        std::string   line;
        while( std::getline( f, line ) )
            subjects.push_back( wxString::FromUTF8( nlohmann::json::parse( line ).get<std::string>() ) );
    }

    begin( "patterns" );
    for( const std::string& p : patterns )
        out( esc( p ) );

    begin( "matchers" );
    for( const std::string& p8 : patterns )
    {
        wxString p = wxString::FromUTF8( p8 );
        out( matcherLine<EDA_PATTERN_MATCH_SUBSTR>( "SUBSTR", p, subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_REGEX>( "REGEX", p, subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_REGEX>( "REGEX/", "/" + p + "/", subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_REGEX_ANCHORED>( "REGEX_ANCHORED", p, subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_WILDCARD>( "WILDCARD", p, subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_WILDCARD_ANCHORED>( "WILDCARD_ANCHORED", p, subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_RELATIONAL>( "RELATIONAL", p, subjects ) );
    }

    begin( "combined" );
    for( const std::string& p8 : patterns )
    {
        wxString p = wxString::FromUTF8( p8 );

        for( int ctx = 0; ctx < 5; ++ctx )
        {
            EDA_COMBINED_MATCHER m( p, (COMBINED_MATCHER_CONTEXT) ctx );
            std::string line = std::to_string( ctx ) + " " + esc( p8 );

            for( const wxString& s : subjects )
            {
                int  triggered = -7, pos = -7;
                bool found = m.Find( s, triggered, pos );
                line += " " + std::to_string( triggered ) + ":" + std::to_string( pos ) + ":"
                        + ( found ? "1" : "0" ) + ( m.Find( s ) ? "1" : "0" )
                        + ( m.StartsWith( s ) ? "1" : "0" );
            }

            std::vector<SEARCH_TERM> terms;
            for( size_t i = 0; i < subjects.size(); ++i )
                terms.emplace_back( subjects[i], (int) ( i % 7 ) + 1, i % 3 == 0 );

            bool exact = false;
            int  score = m.ScoreTerms( terms, &exact );
            line += " | " + std::to_string( score ) + " " + ( exact ? "1" : "0" );

            // Score of the pattern against itself and its upper-case form.
            std::vector<SEARCH_TERM> self{ SEARCH_TERM( p, 3, true ), SEARCH_TERM( p.Upper() + "  ", 2 ) };
            exact = false;
            score = m.ScoreTerms( self, &exact );
            line += " " + std::to_string( score ) + " " + ( exact ? "1" : "0" ) + " ["
                    + esc( self[1].Text ) + "]";
            out( line );
        }
    }

    begin( "settings" );
    for( const char* doc : SETTINGS_DOCS )
    {
        for( bool reset : { true, false } )
        {
            TEST_SETTINGS t( reset );
            bool          ok = t.LoadFromBuffer( doc );
            std::string   line = std::string( reset ? "R " : "K " ) + ( ok ? "ok " : "fail " )
                               + ( t.Future() ? "future " : "" ) + t.Dump();
            bool modified = t.Store();
            line += std::string( " | store=" ) + ( modified ? "1" : "0" ) + " | " + t.FileContents()
                    + " | " + esc( t.FormatAsString() );
            out( line );
        }
    }

    begin( "nested" );
    for( const char* doc : NESTED_DOCS )
    {
        for( bool reset : { true, false } )
        {
            TEST_PARENT parent;
            ( *parent.Internals() )[nlohmann::json::json_pointer( "" )] = nlohmann::json::parse( doc );
            TEST_NESTED n( &parent, reset );
            std::string line = std::string( reset ? "R " : "K " ) + ( n.loaded ? "ok" : "fail" ) + " a="
                               + std::to_string( n.a ) + " m=";
            for( const auto& [k, v] : n.m )
                line += k + ":" + std::to_string( v ) + ";";
            bool modified = n.SaveToFile();
            line += std::string( " | save=" ) + ( modified ? "1" : "0" ) + " | "
                    + esc( parent.Internals()->dump() );
            n.a = 41;
            modified = n.SaveToFile();
            line += std::string( " | save2=" ) + ( modified ? "1" : "0" ) + " | "
                    + esc( parent.Internals()->dump() );
            out( line );
        }
    }

    begin( "textvars" );
    {
        std::function<bool( wxString* )> resolver = testResolver;
        uint64_t                          state = 0xC3E0;
        auto next = [&]() -> uint32_t
        {
            state = state * 6364136223846793005ULL + 1442695040888963407ULL;
            return (uint32_t) ( state >> 33 );
        };
        const int nFrags = sizeof( TEXT_FRAGS ) / sizeof( TEXT_FRAGS[0] );

        for( int k = 0; k < 4000; ++k )
        {
            std::string src;
            uint32_t    n = next() % 9;
            for( uint32_t j = 0; j < n; ++j )
                src += TEXT_FRAGS[next() % nFrags];

            // EXPRESSION_EVALUATOR is not part of the port: skip texts where it could run
            // (an '@' after a "${").
            size_t dollar = src.find( "${" );
            if( dollar != std::string::npos && src.find( '@', dollar ) != std::string::npos )
                continue;

            wxString    wsrc = wxString::FromUTF8( src );
            std::string line = esc( src );

            for( int ctx = 0; ctx <= (int) FOR_URI; ++ctx )
            {
                wxString r = ExpandTextVars( wsrc, &resolver, (RESOLUTION_CONTEXT) ctx );
                line += " | " + esc( r );
                FinalizeTextVarExpansion( r, (RESOLUTION_CONTEXT) ctx );
                line += " ~ " + esc( r );
            }

            if( src.find( '@' ) == std::string::npos )
            {
                int      depth = 0;
                wxString r = ResolveTextVars( wsrc, &resolver, depth );
                line += " || " + esc( r ) + " " + std::to_string( depth );
            }

            line += " || " + esc( NormalizeFilePathForTextVars( wsrc ) );
            out( line );
        }
    }

    begin( "deviations" );
    for( const std::string& p8 : deviations )
    {
        wxString p = wxString::FromUTF8( p8 );
        out( matcherLine<EDA_PATTERN_MATCH_REGEX>( "REGEX/", "/" + p + "/", subjects ) );
        out( matcherLine<EDA_PATTERN_MATCH_REGEX_ANCHORED>( "REGEX_ANCHORED", p, subjects ) );
    }

    flush();
    return 0;
}
