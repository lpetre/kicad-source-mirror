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

// Package SET-3 differential oracle: loads the net_settings object of project documents with the
// real NET_SETTINGS of the reference build (libkicommon.so) under a parameterless JSON_SETTINGS
// parent of schema version 4 (LoadFromBuffer, then NESTED_SETTINGS::LoadFromFile), prints the
// loaded state, then stores it back (flushToStore: NESTED_SETTINGS::SaveToFile + Store) and
// prints the file contents the C++ would write (formatFileContents). Replayed by the Rust test
// `tests/net_settings_json.rs`. Build and run: tests/oracle/net_settings_json_golden.sh
//
// Input: one line per document, "F <path>" (full output) or "C <path>" (compact: hashes for the
// long lists and the output text). Paths are relative to the KiCad root given as argv[2].

#include <project/net_settings.h>
#include <netclass.h>
#include <gal/color4d.h>
#include <eda_pattern_match.h>
#include <settings/json_settings.h>
#include <settings/json_settings_internals.h>

#include <wx/init.h>
#include <wx/log.h>

#include <cstdint>
#include <cstdio>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

using KIGFX::COLOR4D;


class PARENT : public JSON_SETTINGS
{
public:
    PARENT() : JSON_SETTINGS( wxS( "project" ), SETTINGS_LOC::NONE, 4, false, false, false ) {}

    bool Flush() { return flushToStore(); }

    std::string Format() { return formatFileContents(); }
};


static std::string U( const wxString& s )
{
    return std::string( s.ToUTF8().data() );
}


static std::string colorStr( const COLOR4D& c )
{
    nlohmann::json j = c;
    return j.get<std::string>();
}


static std::string opt( bool aHas, int aValue )
{
    return aHas ? std::to_string( aValue ) : std::string( "~" );
}


static uint64_t fnv( const std::string& s )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : s )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


static std::string hex( uint64_t h )
{
    char buf[17];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) h );
    return buf;
}


static void printClass( const NETCLASS& nc )
{
    std::cout << "class\t" << U( nc.GetName() ) << "\t" << nc.IsDefault() << "\t" << nc.GetPriority()
              << "\t" << opt( nc.HasClearance(), nc.GetClearance() )
              << "\t" << opt( nc.HasTrackWidth(), nc.GetTrackWidth() )
              << "\t" << opt( nc.HasViaDiameter(), nc.GetViaDiameter() )
              << "\t" << opt( nc.HasViaDrill(), nc.GetViaDrill() )
              << "\t" << opt( nc.HasuViaDiameter(), nc.GetuViaDiameter() )
              << "\t" << opt( nc.HasuViaDrill(), nc.GetuViaDrill() )
              << "\t" << opt( nc.HasDiffPairWidth(), nc.GetDiffPairWidth() )
              << "\t" << opt( nc.HasDiffPairGap(), nc.GetDiffPairGap() )
              << "\t" << opt( nc.HasDiffPairViaGap(), nc.GetDiffPairViaGap() )
              << "\t" << opt( nc.HasWireWidth(), nc.GetWireWidth() )
              << "\t" << opt( nc.HasBusWidth(), nc.GetBusWidth() )
              << "\t" << opt( nc.HasLineStyle(), nc.GetLineStyle() )
              << "\t" << colorStr( nc.GetPcbColor( true ) )
              << "\t" << colorStr( nc.GetSchematicColor( true ) )
              << "\t" << U( nc.GetTuningProfile() )
              << "\t" << U( nc.GetDescription() ) << "\n";
}


// Prints a list either line by line (full) or as count + hash (compact).
static void printList( const std::string& aTag, const std::vector<std::string>& aLines, bool aFull )
{
    std::string all;

    for( const std::string& l : aLines )
        all += l + "\n";

    std::cout << aTag << "s\t" << aLines.size() << "\t" << hex( fnv( all ) ) << "\n";

    if( aFull )
    {
        for( const std::string& l : aLines )
            std::cout << aTag << "\t" << l << "\n";
    }
}


static void run( const std::string& aRel, const std::string& aPath, bool aFull )
{
    std::ifstream     in( aPath, std::ios::binary );
    std::stringstream buf;
    buf << in.rdbuf();
    std::string contents = buf.str();

    std::cout << "file\t" << aRel << "\n";

    PARENT parent;
    bool   parentOk = parent.LoadFromBuffer( contents );

    NET_SETTINGS ns( &parent, "net_settings" );
    bool         ok = ns.LoadFromFile();

    std::string ver = "~";

    try
    {
        ver = std::to_string( ns.Internals()->Get<int>( "meta.version" ) );
    }
    catch( ... )
    {
    }

    std::cout << "load\t" << parentOk << "\t" << ok << "\t" << ver << "\n";

    printClass( *ns.GetDefaultNetclass() );

    for( const auto& [name, nc] : ns.GetNetclasses() )
    {
        std::cout << "key\t" << U( name ) << "\n";
        printClass( *nc );
    }

    std::vector<std::string> lines;

    for( const auto& [matcher, nc] : ns.GetNetclassPatternAssignments() )
        lines.push_back( U( matcher->GetPattern() ) + "\t" + U( nc ) );

    printList( "pattern", lines, aFull );
    lines.clear();

    for( const auto& [net, classes] : ns.GetNetclassLabelAssignments() )
    {
        std::string l = U( net );

        for( const wxString& c : classes )
            l += "\t" + U( c );

        lines.push_back( l );
    }

    printList( "label", lines, aFull );
    lines.clear();

    for( const auto& [net, color] : ns.GetNetColorAssignments() )
        lines.push_back( U( net ) + "\t" + colorStr( color ) );

    printList( "color", lines, aFull );
    lines.clear();

    for( const auto& [chain, cls] : ns.GetNetChainClasses() )
        lines.push_back( U( chain ) + "\t" + U( cls ) );

    printList( "chainclass", lines, aFull );
    lines.clear();

    for( const auto& [chain, cls] : ns.GetNetChainNetClasses() )
        lines.push_back( U( chain ) + "\t" + U( cls ) );

    printList( "chainnetclass", lines, aFull );

    // Store back as PROJECT_FILE::SaveToFile does (nested settings first, then the parent).
    bool        modified = parent.Flush();
    std::string out;

    try
    {
        out = parent.Format();
    }
    catch( ... )
    {
        out = "<exception>";
    }

    std::cout << "store\t" << modified << "\t" << ( out == contents ) << "\t" << out.size() << "\t"
              << hex( fnv( out ) ) << "\n";

    if( aFull )
    {
        std::istringstream os( out );
        std::string        l;

        while( std::getline( os, l ) )
            std::cout << "o\t" << l << "\n";
    }

    // A second store must report no further change and produce the same text.
    bool        modified2 = parent.Flush();
    std::string out2 = parent.Format();
    std::cout << "store2\t" << modified2 << "\t" << ( out2 == out ) << "\n";
}


int main( int argc, char** argv )
{
    wxInitializer init;

    if( argc < 3 )
    {
        std::cerr << "usage: net_settings_json_oracle LIST ROOT\n";
        return 2;
    }

    wxLog::EnableLogging( false );

    std::ifstream in( argv[1] );
    std::string   root = argv[2];
    std::string   line;

    while( std::getline( in, line ) )
    {
        if( line.size() < 3 || line[0] == '#' )
            continue;

        run( line.substr( 2 ), root + "/" + line.substr( 2 ), line[0] == 'F' );
    }

    return 0;
}
