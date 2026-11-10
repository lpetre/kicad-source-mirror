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

// pcb-dump: the opt-in "engine" section (board command, --sections engine). Prints the DRC
// engine the QA load path created (KI_TEST::LoadBoard: InitEngine with <base>.kicad_dru): the
// provider registry order, every rule in m_rules order, the per-constraint-type compiled lists
// of m_constraintMap and m_explicitConstraints, m_netclassClearances and the engine flags.
// See rust/docs/PCB_DUMP_FORMAT.md section 8.

#include "pcb_dump.h"

#include <map>

#include <board.h>
#include <board_design_settings.h>
#include <reporter.h>
#include <zone.h>
#include <drc/drc_engine.h>
#include <drc/drc_rule.h>
#include <drc/drc_rule_condition.h>
#include <drc/drc_test_provider.h>


namespace
{

/// Read access to the protected members of DRC_ENGINE (the engine object is not modified).
struct ENGINE_PEEK : public DRC_ENGINE
{
    static const ENGINE_PEEK* Of( const DRC_ENGINE* aEngine )
    {
        return static_cast<const ENGINE_PEEK*>( aEngine );
    }

    const std::vector<std::shared_ptr<DRC_RULE>>& Rules() const { return m_rules; }
    const auto& ConstraintMap() const { return m_constraintMap; }
    const auto& ExplicitConstraints() const { return m_explicitConstraints; }
    const auto& NetclassClearances() const { return m_netclassClearances; }
    bool HasExplicitClearanceRules() const { return m_hasExplicitClearanceRules; }
    bool HasDiffPairClearanceOverrides() const { return m_hasDiffPairClearanceOverrides; }
};


std::string severityText( SEVERITY aSeverity )
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


std::string valueText( const MINOPTMAX<int>& aValue )
{
    std::string r;
    r += aValue.HasMin() ? V( aValue.Min() ) : std::string( "none" );
    r += "/";
    r += aValue.HasOpt() ? V( aValue.Opt() ) : std::string( "none" );
    r += "/";
    r += aValue.HasMax() ? V( aValue.Max() ) : std::string( "none" );
    return r;
}


/// Index of a rule in m_rules, or "stale" for a rule that is no longer in the list (left in
/// the constraint map by a failed first compile pass).
std::string ruleIndex( const std::map<const DRC_RULE*, int>& aIndex, const DRC_RULE* aRule )
{
    auto it = aIndex.find( aRule );
    return it == aIndex.end() ? std::string( "stale" ) : std::to_string( it->second );
}


template <typename LIST>
void dumpLists( const std::string& aPrefix, const LIST& aMap,
                const std::map<const DRC_RULE*, int>& aIndex, bool aPointers )
{
    for( const auto& [type, listPtr] : aMap )
    {
        const auto& list = [&]() -> const auto&
        {
            if constexpr( std::is_pointer_v<std::decay_t<decltype( listPtr )>> )
                return *listPtr;
            else
                return listPtr;
        }();

        std::string p = aPrefix + "." + std::string( magic_enum::enum_name( type ) );
        g_out.Emit( p + ".count", V( (int) list.size() ) );

        for( size_t ii = 0; ii < list.size(); ++ii )
        {
            const auto*       c = list[ii];
            const std::string q = p + "." + std::to_string( ii );

            g_out.Emit( q + ".rule", ruleIndex( aIndex, c->parentRule.get() ) );

            if( !aPointers )
                continue;

            g_out.Emit( q + ".layers", V( c->layerTest ) );

            if( c->condition )
            {
                g_out.Emit( q + ".condition", V( c->condition->GetExpression() ) );
                g_out.Emit( q + ".condition.pair", V( c->condition->RequiresPairItems() ) );
                g_out.Emit( q + ".condition.item_b", V( c->condition->ReferencesItemB() ) );
                g_out.Emit( q + ".condition.geometry",
                            V( c->condition->HasGeometryDependentFunctions() ) );
            }
            else
            {
                g_out.Emit( q + ".condition", "none" );
            }

            g_out.Emit( q + ".type", std::string( magic_enum::enum_name( c->constraint.m_Type ) ) );
            g_out.Emit( q + ".value", valueText( c->constraint.m_Value ) );
            g_out.Emit( q + ".disallow", V( c->constraint.m_DisallowFlags ) );
            g_out.Emit( q + ".name", V( c->constraint.GetName() ) );
            g_out.Emit( q + ".severity", severityText( c->constraint.GetSeverity() ) );
            g_out.Emit( q + ".keepout_zone",
                        c->implicitKeepoutZone ? V( c->implicitKeepoutZone->m_Uuid )
                                               : std::string( "none" ) );
        }
    }
}

} // namespace


void DumpEngineSection( BOARD* aBoard )
{
    std::shared_ptr<DRC_ENGINE> engine = aBoard->GetDesignSettings().m_DRCEngine;

    if( !engine )
    {
        g_out.Error( "engine", "engine", "no DRC engine" );
        return;
    }

    {
        SCOPE                           s( "engine", "providers" );
        std::vector<DRC_TEST_PROVIDER*> providers = engine->GetTestProviders();
        g_out.Emit( "count", V( (int) providers.size() ) );

        for( size_t ii = 0; ii < providers.size(); ++ii )
            g_out.Emit( std::to_string( ii ), V( providers[ii]->GetName() ) );
    }

    const ENGINE_PEEK* peek = ENGINE_PEEK::Of( engine.get() );

    std::map<const DRC_RULE*, int> index;

    for( size_t ii = 0; ii < peek->Rules().size(); ++ii )
        index[peek->Rules()[ii].get()] = (int) ii;

    {
        SCOPE s( "engine", "engine" );
        g_out.Emit( "rules_valid", V( engine->RulesValid() ) );
        g_out.Emit( "has_explicit_clearance_rules", V( peek->HasExplicitClearanceRules() ) );
        g_out.Emit( "has_geometry_dependent_rules", V( engine->HasGeometryDependentRules() ) );
        g_out.Emit( "has_dp_clearance_overrides", V( peek->HasDiffPairClearanceOverrides() ) );
        g_out.Emit( "rules", V( (int) peek->Rules().size() ) );
    }

    for( size_t ii = 0; ii < peek->Rules().size(); ++ii )
    {
        const DRC_RULE* rule = peek->Rules()[ii].get();
        SCOPE           s( "engine", "rule." + std::to_string( ii ) );

        g_out.Emit( "name", V( rule->m_Name ) );
        g_out.Emit( "implicit", V( rule->IsImplicit() ) );
        g_out.Emit( "source", std::string( magic_enum::enum_name( rule->GetImplicitSource() ) ) );
        g_out.Emit( "unary", V( rule->m_Unary ) );
        g_out.Emit( "layers", V( rule->m_LayerCondition ) );
        g_out.Emit( "severity", severityText( rule->m_Severity ) );
        g_out.Emit( "netclass", V( rule->m_ImplicitNetclass ) );
        g_out.Emit( "item_id", V( rule->m_ImplicitItemId ) );
        g_out.Emit( "condition", rule->m_Condition ? V( rule->m_Condition->GetExpression() )
                                                   : std::string( "none" ) );
        g_out.Emit( "constraints", V( (int) rule->m_Constraints.size() ) );

        for( size_t jj = 0; jj < rule->m_Constraints.size(); ++jj )
        {
            const DRC_CONSTRAINT& c = rule->m_Constraints[jj];
            const std::string     p = "c" + std::to_string( jj );

            g_out.Emit( p + ".type", std::string( magic_enum::enum_name( c.m_Type ) ) );
            g_out.Emit( p + ".value", valueText( c.m_Value ) );
            g_out.Emit( p + ".disallow", V( c.m_DisallowFlags ) );
            g_out.Emit( p + ".name", V( c.GetName() ) );
            g_out.Emit( p + ".severity", severityText( c.GetSeverity() ) );
        }
    }

    {
        SCOPE s( "engine", "map" );
        dumpLists( "map", peek->ConstraintMap(), index, true );
    }

    {
        SCOPE s( "engine", "explicit" );
        dumpLists( "explicit", peek->ExplicitConstraints(), index, false );
    }

    {
        // unordered_map: printed in name order
        SCOPE                                     s( "engine", "netclass_clearances" );
        std::map<wxString, const DRC_RULE*>        sorted( peek->NetclassClearances().begin(),
                                                           peek->NetclassClearances().end() );

        for( const auto& [name, rule] : sorted )
            g_out.Emit( std::string( name.ToUTF8() ), ruleIndex( index, rule ) );
    }
}
