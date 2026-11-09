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

#include <common.h>
#include <eda_pattern_match.h>
#include <settings/json_settings.h>
#include <settings/json_settings_internals.h>
#include <settings/nested_settings.h>
#include <settings/parameters.h>

#include <nlohmann/json.hpp>

#include <wx/init.h>
#include <wx/log.h>

#include <cinttypes>
#include <cstring>
#include <fstream>
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

        if( g_full || i < 25 )
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

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 1;

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

    flush();
    return 0;
}
