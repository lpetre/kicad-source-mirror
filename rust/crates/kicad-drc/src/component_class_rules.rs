// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! The DRC half of `COMPONENT_CLASS_MANAGER::CompileAssignmentRule`
//! (`pcbnew/component_classes/component_class_manager.cpp`) and of
//! `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches` (`component_class_assignment_rule.cpp`): the
//! rule compiler and condition hooks of `kicad_pcb::component_class`, implemented on
//! [`DrcRulesParser::parse_component_class_assignment_rules`] and
//! [`DrcRuleCondition::evaluate_for`].

use std::sync::Arc;

use kicad_core::{PcbLayerId, Severity};
use kicad_pcb::component_class::{
    ComponentClassAssignmentRule, ComponentClassRuleCompiler, ComponentClassRuleCompilerHook,
    ComponentClassRuleCondition,
};
use kicad_pcb::{Board, ItemRef};

use crate::reporter::{Reporter, WxStringReporter};
use crate::rule_condition::DrcRuleCondition;
use crate::rule_parser::DrcRulesParser;

impl ComponentClassRuleCondition for DrcRuleCondition {
    /// `m_condition->EvaluateFor( aFootprint, nullptr, 0, aFootprint->GetSide(), nullptr )`
    /// (`COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`).
    fn evaluate_for_footprint(&self, footprint: ItemRef<'_>, layer: PcbLayerId) -> bool {
        self.evaluate_for(Some(footprint), None, 0, layer, None)
    }
}

/// The DRC rule compiler of `COMPONENT_CLASS_MANAGER::CompileAssignmentRule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrcComponentClassRuleCompiler;

impl ComponentClassRuleCompiler for DrcComponentClassRuleCompiler {
    /// The `DRC_RULES_PARSER` part of `COMPONENT_CLASS_MANAGER::CompileAssignmentRule`.
    fn compile(&self, rule_source: &str) -> Option<ComponentClassAssignmentRule> {
        compile_assignment_rule_source(rule_source)
    }
}

/// Port of the parser part of `COMPONENT_CLASS_MANAGER::CompileAssignmentRule` on a non-empty
/// rule source: `ParseComponentClassAssignmentRules` with a `WX_STRING_REPORTER`; `None` on a
/// `PARSE_ERROR`, a reported error, or anything but exactly one parsed rule (a null rule,
/// from a statement cut short, is `None` too).
pub fn compile_assignment_rule_source(rule_source: &str) -> Option<ComponentClassAssignmentRule> {
    let mut parsed = Vec::new();
    let mut reporter = WxStringReporter::new();

    {
        let mut parser: DrcRulesParser<'_> =
            DrcRulesParser::new(rule_source, "Component class assignment rule");

        if parser
            .parse_component_class_assignment_rules(&mut parsed, Some(&mut reporter))
            .is_err()
        {
            return None;
        }
    }

    if reporter.has_message_of_severity(Severity::Error as i32) {
        return None;
    }

    if parsed.len() != 1 {
        return None;
    }

    let rule = parsed.pop()??;

    Some(ComponentClassAssignmentRule {
        component_class: rule.component_class,
        condition: rule
            .condition
            .map(|c| c as Arc<dyn ComponentClassRuleCondition>),
    })
}

/// Installs the DRC rule compiler on the board's component class manager, so that
/// `Board::synchronize_component_classes` can compile the project's assignment rules (the
/// load sequence calls this before the loader steps).
pub fn install(board: &mut Board) {
    board.component_class_manager.rule_compiler = Some(ComponentClassRuleCompilerHook(Arc::new(
        DrcComponentClassRuleCompiler,
    )));
}
