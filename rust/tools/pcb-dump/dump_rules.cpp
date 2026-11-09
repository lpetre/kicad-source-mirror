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

// pcb-dump: the `rules` command. Parses .kicad_dru files with the real DRC_RULES_PARSER, once
// with a collecting REPORTER ("report" mode, the rule editor path) and once without one
// ("throw" mode, the DRC engine / kicad-cli path), and prints every parsed rule and every
// reported message or thrown PARSE_ERROR. See rust/docs/PCB_DUMP_FORMAT.md section 7.

#include "pcb_dump.h"

#include <fstream>
#include <sstream>

#include <wx/filename.h>

#include <reporter.h>
#include <drc/drc_rule.h>
#include <drc/drc_rule_condition.h>
#include <drc/drc_rule_parser.h>
#include <component_classes/component_class_assignment_rule.h>
#include <properties/property_mgr.h>
#include <ki_exception.h>


namespace
{

/// REPORTER that records every message with its severity.
class COLLECTING_REPORTER : public REPORTER
{
public:
    REPORTER& Report( const wxString& aText, SEVERITY aSeverity ) override
    {
        REPORTER::Report( aText, aSeverity );
        m_messages.emplace_back( aText, aSeverity );
        return *this;
    }

    std::vector<std::pair<wxString, SEVERITY>> m_messages;
};


std::string severityName( SEVERITY aSeverity )
{
    switch( aSeverity )
    {
    case RPT_SEVERITY_UNDEFINED: return "RPT_SEVERITY_UNDEFINED";
    case RPT_SEVERITY_INFO:      return "RPT_SEVERITY_INFO";
    case RPT_SEVERITY_EXCLUSION: return "RPT_SEVERITY_EXCLUSION";
    case RPT_SEVERITY_ACTION:    return "RPT_SEVERITY_ACTION";
    case RPT_SEVERITY_WARNING:   return "RPT_SEVERITY_WARNING";
    case RPT_SEVERITY_ERROR:     return "RPT_SEVERITY_ERROR";
    case RPT_SEVERITY_IGNORE:    return "RPT_SEVERITY_IGNORE";
    case RPT_SEVERITY_DEBUG:     return "RPT_SEVERITY_DEBUG";
    }

    return "?" + std::to_string( (int) aSeverity );
}


void dumpCondition( const std::string& aPrefix, const DRC_RULE_CONDITION* aCondition )
{
    if( !aCondition )
    {
        g_out.Emit( aPrefix, "none" );
        return;
    }

    g_out.Emit( aPrefix, V( aCondition->GetExpression() ) );
    g_out.Emit( aPrefix + ".pair", V( aCondition->RequiresPairItems() ) );
    g_out.Emit( aPrefix + ".item_b", V( aCondition->ReferencesItemB() ) );
    g_out.Emit( aPrefix + ".geometry", V( aCondition->HasGeometryDependentFunctions() ) );
}


std::string ruleRef( const DRC_RULE* aRule, const DRC_RULE* aSelf )
{
    if( !aRule )
        return "none";

    return aRule == aSelf ? "self" : "other";
}


void dumpRule( const std::string& aPrefix, const DRC_RULE* aRule )
{
    g_out.Emit( aPrefix + ".name", V( aRule->m_Name ) );
    g_out.Emit( aPrefix + ".implicit", V( aRule->IsImplicit() ) );
    g_out.Emit( aPrefix + ".unary", V( aRule->m_Unary ) );
    g_out.Emit( aPrefix + ".layer_source", V( aRule->m_LayerSource ) );
    g_out.Emit( aPrefix + ".layers", V( aRule->m_LayerCondition ) );
    g_out.Emit( aPrefix + ".severity", severityName( aRule->m_Severity ) );
    dumpCondition( aPrefix + ".condition", aRule->m_Condition );
    g_out.Emit( aPrefix + ".constraints", V( (int) aRule->m_Constraints.size() ) );

    for( size_t ii = 0; ii < aRule->m_Constraints.size(); ++ii )
    {
        const DRC_CONSTRAINT& c = aRule->m_Constraints[ii];
        std::string           p = aPrefix + ".c." + std::to_string( ii );
        const MINOPTMAX<int>& v = c.GetValue();

        g_out.Emit( p + ".type", VEnum( c.m_Type ) );
        g_out.Emit( p + ".min", v.HasMin() ? V( v.Min() ) : std::string( "none" ) );
        g_out.Emit( p + ".opt", v.HasOpt() ? V( v.Opt() ) : std::string( "none" ) );
        g_out.Emit( p + ".max", v.HasMax() ? V( v.Max() ) : std::string( "none" ) );
        g_out.Emit( p + ".disallow", V( c.m_DisallowFlags ) );
        g_out.Emit( p + ".zone_connection", VEnum( c.m_ZoneConnection ) );
        dumpCondition( p + ".test", c.m_Test );
        g_out.Emit( p + ".implicit_min", V( c.m_ImplicitMin ) );
        g_out.Emit( p + ".reference_layer", V( c.m_ReferenceLayer ) );
        g_out.Emit( p + ".reference_net", V( c.m_ReferenceNet ) );
        g_out.Emit( p + ".option.skew_within_diff_pairs",
                    V( c.GetOption( DRC_CONSTRAINT::OPTIONS::SKEW_WITHIN_DIFF_PAIRS ) ) );
        g_out.Emit( p + ".option.space_domain",
                    V( c.GetOption( DRC_CONSTRAINT::OPTIONS::SPACE_DOMAIN ) ) );
        g_out.Emit( p + ".option.time_domain",
                    V( c.GetOption( DRC_CONSTRAINT::OPTIONS::TIME_DOMAIN ) ) );
        g_out.Emit( p + ".name", V( c.GetName() ) );
        g_out.Emit( p + ".severity", severityName( c.GetSeverity() ) );
        g_out.Emit( p + ".parent_rule", ruleRef( c.GetParentRule(), aRule ) );
        g_out.Emit( p + ".min_rule", ruleRef( c.GetMinRule(), aRule ) );
        g_out.Emit( p + ".opt_rule", ruleRef( c.GetOptRule(), aRule ) );
        g_out.Emit( p + ".max_rule", ruleRef( c.GetMaxRule(), aRule ) );
    }
}


void dumpError( PARSE_ERROR& aError )
{
    g_out.Emit( "status", "parse_error" );
    g_out.Emit( "error", V( aError.What() ) );
    g_out.Emit( "error.problem", V( aError.ParseProblem() ) );
    g_out.Emit( "error.line", V( aError.lineNumber ) );
    g_out.Emit( "error.offset", V( aError.byteIndex ) );
    g_out.Emit( "error.input_line", V( aError.inputLine ) );
}


/// One parse of aText in the given mode; prints the section "rules|<mode>|...".
void dumpParse( const wxString& aText, const wxString& aSource, bool aWithReporter,
                bool aComponentClasses )
{
    SCOPE               scope( "rules", aWithReporter ? "report" : "throw" );
    COLLECTING_REPORTER reporter;
    DRC_RULES_PARSER    parser( aText, aSource );

    std::vector<std::shared_ptr<DRC_RULE>>                        rules;
    std::vector<std::shared_ptr<COMPONENT_CLASS_ASSIGNMENT_RULE>> ccRules;
    bool                                                          ok = false;

    try
    {
        if( aComponentClasses )
            parser.ParseComponentClassAssignmentRules( ccRules, aWithReporter ? &reporter : nullptr );
        else
            parser.Parse( rules, aWithReporter ? &reporter : nullptr );

        ok = true;
    }
    catch( PARSE_ERROR& e )
    {
        dumpError( e );
    }
    catch( const IO_ERROR& e )
    {
        g_out.Emit( "status", "io_error" );
        g_out.Emit( "error", V( e.What() ) );
    }

    if( ok )
        g_out.Emit( "status", "ok" );

    for( size_t ii = 0; ii < reporter.m_messages.size(); ++ii )
    {
        g_out.Emit( "msg." + std::to_string( ii ),
                    severityName( reporter.m_messages[ii].second ) + " "
                            + V( reporter.m_messages[ii].first ) );
    }

    if( aComponentClasses )
    {
        g_out.Emit( "count", V( (int) ccRules.size() ) );

        for( size_t ii = 0; ii < ccRules.size(); ++ii )
        {
            std::string p = "ccrule." + std::to_string( ii );

            if( !ccRules[ii] )
            {
                g_out.Emit( p, "null" );
                continue;
            }

            g_out.Emit( p + ".class", V( ccRules[ii]->GetComponentClass() ) );
        }
    }
    else
    {
        g_out.Emit( "count", V( (int) rules.size() ) );

        for( size_t ii = 0; ii < rules.size(); ++ii )
            dumpRule( "rule." + std::to_string( ii ), rules[ii].get() );
    }
}

} // namespace


/// `pcb-dump rules [--component-classes] FILE...`
int DumpRulesFiles( const std::vector<wxString>& aFiles, bool aComponentClasses )
{
    PROPERTY_MANAGER::Instance().Rebuild();

    int failures = 0;

    for( const wxString& file : aFiles )
    {
        std::string   name = std::string( wxFileName( file ).GetFullName().ToUTF8() );
        std::ifstream in( std::string( file.ToUTF8() ), std::ios::binary );

        printf( "file|%s|\n", name.c_str() );

        if( !in )
        {
            g_out.Error( "rules", name, "cannot read file" );
            failures++;
            continue;
        }

        std::stringstream buf;
        buf << in.rdbuf();
        wxString text = wxString::FromUTF8( buf.str() );
        wxString source = wxString::FromUTF8( name );

        Guard( "report", [&]() { dumpParse( text, source, true, aComponentClasses ); } );
        Guard( "throw", [&]() { dumpParse( text, source, false, aComponentClasses ); } );
        fflush( stdout );
    }

    return failures;
}
