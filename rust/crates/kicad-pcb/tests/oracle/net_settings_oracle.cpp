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

// Package M4 differential oracle: runs tests/data/net_settings/scenarios.txt against the real
// NET_SETTINGS / NETCLASS of the reference build (libkicommon.so) and prints one line per
// observation. The Rust test `net_settings::tests::oracle_scenarios` replays the same script.
// Build and run: tests/oracle/net_settings_golden.sh

#include <project/net_settings.h>
#include <netclass.h>
#include <gal/color4d.h>
#include <eda_pattern_match.h>

#include <wx/init.h>
#include <wx/log.h>

#include <cmath>
#include <cstdio>
#include <fstream>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

using KIGFX::COLOR4D;

static std::vector<std::string> split( const std::string& aLine, char aSep )
{
    std::vector<std::string> out;
    std::string              cur;

    for( char c : aLine )
    {
        if( c == aSep )
        {
            out.push_back( cur );
            cur.clear();
        }
        else
        {
            cur += c;
        }
    }

    out.push_back( cur );
    return out;
}


static wxString W( const std::string& s )
{
    return wxString::FromUTF8( s.c_str() );
}


static std::string U( const wxString& s )
{
    return std::string( s.ToUTF8().data() );
}


static std::string colorStr( const COLOR4D& c )
{
    if( c == COLOR4D::UNSPECIFIED )
        return "~";

    std::ostringstream o;
    o << std::lround( c.r * 1000 ) << "," << std::lround( c.g * 1000 ) << ","
      << std::lround( c.b * 1000 ) << "," << std::lround( c.a * 1000 );
    return o.str();
}


static COLOR4D parseColor( const std::string& s )
{
    if( s == "~" )
        return COLOR4D::UNSPECIFIED;

    std::vector<std::string> v = split( s, ',' );
    return COLOR4D( std::stod( v[0] ), std::stod( v[1] ), std::stod( v[2] ), std::stod( v[3] ) );
}


// Applies the class|NAME|PRIO|... field list (from index aFirst = PRIO) to aNc.
static void applyFields( NETCLASS* aNc, const std::vector<std::string>& f, size_t aFirst )
{
    auto opt = [&]( size_t i, auto setter )
    {
        const std::string& s = f[aFirst + i];

        if( s == "=" )
            return;

        if( s == "~" )
            setter( std::optional<int>() );
        else
            setter( std::optional<int>( std::stoi( s ) ) );
    };

    if( f[aFirst] != "=" )
        aNc->SetPriority( std::stoi( f[aFirst] ) );

    opt( 1, [&]( std::optional<int> v ) { aNc->SetClearance( v ); } );
    opt( 2, [&]( std::optional<int> v ) { aNc->SetTrackWidth( v ); } );
    opt( 3, [&]( std::optional<int> v ) { aNc->SetViaDiameter( v ); } );
    opt( 4, [&]( std::optional<int> v ) { aNc->SetViaDrill( v ); } );
    opt( 5, [&]( std::optional<int> v ) { aNc->SetuViaDiameter( v ); } );
    opt( 6, [&]( std::optional<int> v ) { aNc->SetuViaDrill( v ); } );
    opt( 7, [&]( std::optional<int> v ) { aNc->SetDiffPairWidth( v ); } );
    opt( 8, [&]( std::optional<int> v ) { aNc->SetDiffPairGap( v ); } );
    opt( 9, [&]( std::optional<int> v ) { aNc->SetDiffPairViaGap( v ); } );
    opt( 10, [&]( std::optional<int> v ) { aNc->SetWireWidth( v ); } );
    opt( 11, [&]( std::optional<int> v ) { aNc->SetBusWidth( v ); } );
    opt( 12, [&]( std::optional<int> v ) { aNc->SetLineStyle( v ); } );

    if( f[aFirst + 13] != "=" )
        aNc->SetPcbColor( parseColor( f[aFirst + 13] ) );

    if( f[aFirst + 14] != "=" )
        aNc->SetSchematicColor( parseColor( f[aFirst + 14] ) );

    if( f[aFirst + 15] != "=" )
        aNc->SetTuningProfile( W( f[aFirst + 15] ) );
}


static std::string optStr( bool aHas, int aValue )
{
    return aHas ? std::to_string( aValue ) : std::string( "~" );
}


static std::string kindOf( NET_SETTINGS& ns, const std::shared_ptr<NETCLASS>& nc )
{
    if( nc == ns.GetDefaultNetclass() )
        return "default";

    auto it = ns.GetNetclasses().find( nc->GetName() );

    if( it != ns.GetNetclasses().end() && it->second == nc )
        return "class";

    for( const auto& [name, comp] : ns.GetCompositeNetclasses() )
    {
        if( comp == nc )
            return "composite";
    }

    return "other";
}


static std::string detail( NETCLASS* nc )
{
    std::ostringstream o;
    o << "name=" << U( nc->GetName() ) << "|default=" << nc->IsDefault() << "|prio=" << nc->GetPriority()
      << "|cl=" << optStr( nc->HasClearance(), nc->GetClearance() )
      << "|tw=" << optStr( nc->HasTrackWidth(), nc->GetTrackWidth() )
      << "|vd=" << optStr( nc->HasViaDiameter(), nc->GetViaDiameter() )
      << "|vdr=" << optStr( nc->HasViaDrill(), nc->GetViaDrill() )
      << "|uvd=" << optStr( nc->HasuViaDiameter(), nc->GetuViaDiameter() )
      << "|uvdr=" << optStr( nc->HasuViaDrill(), nc->GetuViaDrill() )
      << "|dpw=" << optStr( nc->HasDiffPairWidth(), nc->GetDiffPairWidth() )
      << "|dpg=" << optStr( nc->HasDiffPairGap(), nc->GetDiffPairGap() )
      << "|dpvg=" << optStr( nc->HasDiffPairViaGap(), nc->GetDiffPairViaGap() )
      << "|ww=" << optStr( nc->HasWireWidth(), nc->GetWireWidth() )
      << "|bw=" << optStr( nc->HasBusWidth(), nc->GetBusWidth() )
      << "|ls=" << optStr( nc->HasLineStyle(), nc->GetLineStyle() )
      << "|pcb=" << colorStr( nc->GetPcbColor( true ) )
      << "|sch=" << colorStr( nc->GetSchematicColor( true ) )
      << "|tp=" << U( nc->GetTuningProfile() );

    o << "|par=" << U( nc->GetClearanceParent()->GetName() ) << ","
      << U( nc->GetTrackWidthParent()->GetName() ) << ","
      << U( nc->GetViaDiameterParent()->GetName() ) << ","
      << U( nc->GetViaDrillParent()->GetName() ) << ","
      << U( nc->GetuViaDiameterParent()->GetName() ) << ","
      << U( nc->GetuViaDrillParent()->GetName() ) << ","
      << U( nc->GetDiffPairWidthParent()->GetName() ) << ","
      << U( nc->GetDiffPairGapParent()->GetName() ) << ","
      << U( nc->GetDiffPairViaGapParent()->GetName() ) << ","
      << U( nc->GetWireWidthParent()->GetName() ) << ","
      << U( nc->GetBusWidthParent()->GetName() ) << ","
      << U( nc->GetLineStyleParent()->GetName() ) << ","
      << U( nc->GetPcbColorParent()->GetName() ) << ","
      << U( nc->GetSchematicColorParent()->GetName() ) << ","
      << U( nc->GetTuningProfileParent()->GetName() );

    o << "|cons=";
    bool first = true;

    for( NETCLASS* c : nc->GetConstituentNetclasses() )
    {
        o << ( first ? "" : ";" ) << U( c->GetName() );
        first = false;
    }

    return o.str();
}


static std::string joinMembers( const std::vector<wxString>& aMembers )
{
    std::string out;

    for( size_t i = 0; i < aMembers.size(); ++i )
        out += ( i ? ";" : "" ) + U( aMembers[i] );

    return out;
}


int main( int argc, char** argv )
{
    wxInitializer init;

    if( argc < 2 )
    {
        std::cerr << "usage: net_settings_oracle SCENARIOS\n";
        return 2;
    }

    // wxRegEx reports compile errors through wxLogError; keep them off stderr.
    wxLog::EnableLogging( false );

    std::ifstream                 in( argv[1] );
    std::string                   line;
    std::unique_ptr<NET_SETTINGS> ns = std::make_unique<NET_SETTINGS>( nullptr, "" );

    while( std::getline( in, line ) )
    {
        if( line.empty() || line[0] == '#' )
            continue;

        // Corpus scenarios separate fields by tabs (patterns may contain '|').
        std::vector<std::string> f = split( line, line.find( '\t' ) != std::string::npos ? '\t'
                                                                                         : '|' );
        const std::string&       cmd = f[0];

        if( cmd == "reset" )
        {
            ns = std::make_unique<NET_SETTINGS>( nullptr, "" );
            std::cout << "reset" << ( f.size() > 1 ? "|" + f[1] : std::string() ) << "\n";
        }
        else if( cmd == "class" )
        {
            std::shared_ptr<NETCLASS> nc = std::make_shared<NETCLASS>( W( f[1] ), false );
            applyFields( nc.get(), f, 2 );
            ns->SetNetclass( W( f[1] ), nc );
        }
        else if( cmd == "default" )
        {
            applyFields( ns->GetDefaultNetclass().get(), f, 1 );
        }
        else if( cmd == "setclasses" )
        {
            std::map<wxString, std::shared_ptr<NETCLASS>> classes = ns->GetNetclasses();
            ns->SetNetclasses( classes );
        }
        else if( cmd == "clearclasses" )
        {
            ns->ClearNetclasses();
        }
        else if( cmd == "clearcaches" || cmd == "clearall" )
        {
            ns->ClearAllCaches();
        }
        else if( cmd == "recompute" )
        {
            ns->RecomputeEffectiveNetclasses();
        }
        else if( cmd == "composites" )
        {
            for( const auto& [key, nc] : ns->GetCompositeNetclasses() )
                std::cout << "comp|" << U( key ) << "|" << detail( nc.get() ) << "\n";

            std::cout << "composites|" << ns->GetCompositeNetclasses().size() << "\n";
        }
        else if( cmd == "patterns" )
        {
            for( const auto& [matcher, nc] : ns->GetNetclassPatternAssignments() )
                std::cout << "pat|" << U( matcher->GetPattern() ) << "|" << U( nc ) << "\n";

            for( const auto& [source, list] : ns->GetChainPatternAssignments() )
            {
                for( const auto& [matcher, nc] : list )
                {
                    std::cout << "chainpat|" << ( source == NET_CHAIN_SOURCE::SCHEMATIC ? "S" : "B" )
                              << "|" << U( matcher->GetPattern() ) << "|" << U( nc ) << "\n";
                }
            }

            std::cout << "haschain|" << ns->HasChainPatternAssignments( NET_CHAIN_SOURCE::SCHEMATIC )
                      << "|" << ns->HasChainPatternAssignments( NET_CHAIN_SOURCE::BOARD ) << "\n";
        }
        else if( cmd == "colors" )
        {
            for( const auto& [net, color] : ns->GetNetColorAssignments() )
                std::cout << "color|" << U( net ) << "|" << colorStr( color ) << "\n";
        }
        else if( cmd == "color" )
        {
            ns->SetNetColorAssignment( W( f[1] ), parseColor( f[2] ) );
        }
        else if( cmd == "label" || cmd == "appendlabel" )
        {
            std::set<wxString> classes;

            for( const std::string& c : split( f[2], ',' ) )
                classes.insert( W( c ) );

            if( cmd == "label" )
                ns->SetNetclassLabelAssignment( W( f[1] ), classes );
            else
                ns->AppendNetclassLabelAssignment( W( f[1] ), classes );
        }
        else if( cmd == "clearlabel" )
        {
            ns->ClearNetclassLabelAssignment( W( f[1] ) );
        }
        else if( cmd == "clearlabels" )
        {
            ns->ClearNetclassLabelAssignments();
        }
        else if( cmd == "pattern" )
        {
            ns->SetNetclassPatternAssignment( W( f[1] ), W( f[2] ) );
        }
        else if( cmd == "clearpatterns" )
        {
            ns->ClearNetclassPatternAssignments();
        }
        else if( cmd == "chain" || cmd == "clearchain" )
        {
            NET_CHAIN_SOURCE src = f[1] == "S" ? NET_CHAIN_SOURCE::SCHEMATIC
                                               : NET_CHAIN_SOURCE::BOARD;

            if( cmd == "chain" )
                ns->SetChainPatternAssignment( src, W( f[2] ), W( f[3] ) );
            else
                ns->ClearChainPatternAssignments( src );
        }
        else if( cmd == "q" )
        {
            std::shared_ptr<NETCLASS> nc = ns->GetEffectiveNetClass( W( f[1] ) );
            std::cout << "q|" << f[1] << "|kind=" << kindOf( *ns, nc ) << "|" << detail( nc.get() )
                      << "\n";
        }
        else if( cmd == "qn" )
        {
            std::cout << "qn|" << f[1] << "|" << U( ns->GetEffectiveNetClass( W( f[1] ) )->GetName() )
                      << "\n";
        }
        else if( cmd == "rx" )
        {
            EDA_PATTERN_MATCH_REGEX_ANCHORED regex;
            std::cout << "rx|" << f[1] << "|" << regex.SetPattern( W( f[1] ) ) << "\n";
        }
        else if( cmd == "has" )
        {
            std::cout << "has|" << f[1] << "|" << ns->HasEffectiveNetClass( W( f[1] ) ) << "\n";
        }
        else if( cmd == "clearnet" )
        {
            ns->ClearCacheForNet( W( f[1] ) );
        }
        else if( cmd == "byname" )
        {
            std::shared_ptr<NETCLASS> nc = ns->GetNetClassByName( W( f[1] ) );
            std::cout << "byname|" << f[1] << "|kind=" << kindOf( *ns, nc ) << "|"
                      << U( nc->GetName() ) << "\n";
        }
        else if( cmd == "renameprefix" )
        {
            bool changed = ns->RenameNetPathPrefix( W( f[1] ), W( f[2] ) );
            std::cout << "renameprefix|" << f[1] << "|" << f[2] << "|" << changed << "\n";
        }
        else if( cmd == "renamenets" )
        {
            std::map<wxString, wxString> names;

            if( !f[1].empty() )
            {
                for( const std::string& pair : split( f[1], ',' ) )
                {
                    std::vector<std::string> kv = split( pair, '=' );
                    names[W( kv[0] )] = W( kv[1] );
                }
            }

            std::cout << "renamenets|" << f[1] << "|" << ns->RenameNets( names ) << "\n";
        }
        else if( cmd == "bv" )
        {
            wxString              name = wxS( "?" );
            std::vector<wxString> members;
            bool                  ok = NET_SETTINGS::ParseBusVector( W( f[1] ), &name, &members );
            std::cout << "bv|" << f[1] << "|ok=" << ok << "|name=" << U( name )
                      << "|members=" << joinMembers( members ) << "\n";
        }
        else if( cmd == "bg" )
        {
            wxString              name = wxS( "?" );
            std::vector<wxString> members;
            size_t                prefixEnd = 99;
            bool ok = NET_SETTINGS::ParseBusGroup( W( f[1] ), &name, &members, &prefixEnd );
            std::cout << "bg|" << f[1] << "|ok=" << ok << "|name=" << U( name )
                      << "|members=" << joinMembers( members ) << "|prefix_end=" << prefixEnd
                      << "\n";
        }
        else if( cmd == "fe" )
        {
            std::vector<wxString> members;
            NET_SETTINGS::ForEachBusMember( W( f[1] ),
                                            [&]( const wxString& m ) { members.push_back( m ); } );
            std::cout << "fe|" << f[1] << "|" << joinMembers( members ) << "\n";
        }
        else
        {
            std::cerr << "unknown command: " << line << "\n";
            return 1;
        }
    }

    return 0;
}
