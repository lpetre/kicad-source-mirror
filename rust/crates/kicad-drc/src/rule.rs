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

//! Port of `pcbnew/drc/drc_rule.h` and `pcbnew/drc/drc_rule.cpp` (without the IPC-API
//! functions `FormatRuleFromProto` / `ToProto` and without the board helper `GetConstraint`).
//!
//! `DRC_CONSTRAINT` is `kicad_pcb::drc_api::DrcConstraint` (the board model returns it from
//! `RuleResolver::eval_rules`); the methods of the C++ class that need [`DrcRule`] are the
//! extension trait [`DrcConstraintExt`]. Two representation changes follow from that type:
//!
//! * the rule back pointers (`m_parentRule`, `m_minRule`, `m_optRule`, `m_maxRule`) are
//!   [`DrcRuleInfo`] copies (name, severity, implicit flag, and as the pointer's identity
//!   the rule's index in the engine's rule list) taken when the pointer would be set. A C++
//!   pointer sees later changes of the rule (e.g. a `(severity ...)` clause after the
//!   `(constraint ...)` clause), so whoever finishes modifying a rule calls
//!   [`DrcRule::sync_constraint_rule_info`] (the parser does it at the end of every rule);
//! * `m_Test` (the assertion condition) is not a member of `DrcConstraint`; it is kept next to
//!   the constraint in [`DrcRule::constraint_tests`] (same index).

use std::sync::Arc;

use kicad_core::{Kiid, Lset, NILUUID, Severity};
use kicad_pcb::ItemId;
pub use kicad_pcb::drc_api::{DrcConstraint, DrcRuleInfo};

use crate::constraint_types::{DrcConstraintT, DrcImplicitSource};
use crate::rule_condition::DrcRuleCondition;

/// Port of `IsComponentClassSelector`: `${Class:X}` inside a DRC rule is a component-class
/// selector consumed by `testFootprintSelector()`, not a text variable. Text-variable
/// expansion and unresolved-variable checks must leave it intact. `token` is the contents
/// between `${` and `}`.
pub fn is_component_class_selector(token: &str) -> bool {
    // aToken.Upper().StartsWith( wxT( "CLASS:" ) )
    token.to_uppercase().starts_with("CLASS:")
}

/// Port of `DRC_RULE`.
#[derive(Clone)]
pub struct DrcRule {
    /// `m_Unary`.
    pub unary: bool,
    /// `m_ImplicitItemId`.
    pub implicit_item_id: Kiid,
    /// `m_ImplicitItem` (the keepout zone of an implicit keepout rule).
    pub implicit_item: Option<ItemId>,
    /// `m_Name`.
    pub name: String,
    /// `m_ImplicitNetclass`: netclass named by an implicit netclass rule's condition.
    pub implicit_netclass: String,
    /// `m_LayerSource`: the text of the `(layer ...)` clause.
    pub layer_source: String,
    /// `m_LayerCondition`.
    pub layer_condition: Lset,
    /// `m_Condition` (shared, as the engine keeps the pointer in its constraint map).
    pub condition: Option<Arc<DrcRuleCondition>>,
    /// `m_Constraints`.
    pub constraints: Vec<DrcConstraint>,
    /// `m_Constraints[i].m_Test`: the assertion condition of constraint `i`.
    pub constraint_tests: Vec<Option<Arc<DrcRuleCondition>>>,
    /// `m_Severity`.
    pub severity: Severity,
    /// `m_implicitSource`.
    implicit_source: DrcImplicitSource,
}

impl Default for DrcRule {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DrcRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrcRule")
            .field("name", &self.name)
            .field("unary", &self.unary)
            .field("layer_source", &self.layer_source)
            .field("severity", &self.severity)
            .field(
                "condition",
                &self.condition.as_ref().map(|c| c.expression().to_string()),
            )
            .field("constraints", &self.constraints)
            .field("implicit_source", &self.implicit_source)
            .finish()
    }
}

impl DrcRule {
    /// Port of `DRC_RULE::DRC_RULE()`.
    pub fn new() -> Self {
        DrcRule {
            unary: false,
            implicit_item_id: NILUUID,
            implicit_item: None,
            name: String::new(),
            implicit_netclass: String::new(),
            layer_source: String::new(),
            layer_condition: Lset::all_layers_mask().clone(),
            condition: None,
            constraints: Vec::new(),
            constraint_tests: Vec::new(),
            severity: Severity::Undefined,
            implicit_source: DrcImplicitSource::None,
        }
    }

    /// Port of `DRC_RULE::DRC_RULE( const wxString& aName )`.
    pub fn with_name(name: &str) -> Self {
        DrcRule {
            name: name.to_string(),
            ..Self::new()
        }
    }

    /// Port of `DRC_RULE::AppliesTo`: the base class applies to everything.
    pub fn applies_to(&self) -> bool {
        true
    }

    /// Port of `DRC_RULE::AddConstraint`: attribute `constraint` to this rule and append it.
    /// `test` is the constraint's `m_Test` (the assertion condition, if any).
    pub fn add_constraint(
        &mut self,
        constraint: &mut DrcConstraint,
        test: Option<Arc<DrcRuleCondition>>,
    ) {
        let info = self.info();
        constraint.set_parent_rule(Some(&info));
        self.constraints.push(constraint.clone());
        self.constraint_tests.push(test);
    }

    /// Port of `DRC_RULE::FindConstraint`: a copy of the first constraint of type `type_`.
    pub fn find_constraint(&self, type_: DrcConstraintT) -> Option<DrcConstraint> {
        self.constraints.iter().find(|c| c.type_ == type_).cloned()
    }

    /// Port of `DRC_RULE::IsImplicit`.
    pub fn is_implicit(&self) -> bool {
        self.implicit_source != DrcImplicitSource::None
    }

    /// Port of `DRC_RULE::SetImplicitSource`.
    pub fn set_implicit_source(&mut self, implicit_source: DrcImplicitSource) {
        self.implicit_source = implicit_source;
    }

    /// Port of `DRC_RULE::GetImplicitSource`.
    pub fn implicit_source(&self) -> DrcImplicitSource {
        self.implicit_source
    }

    /// What a `DRC_RULE*` held by a constraint gives access to (see the module comment), for
    /// a rule that is not (yet) in an engine: `rule_index` is `None`.
    pub fn info(&self) -> DrcRuleInfo {
        DrcRuleInfo {
            name: self.name.clone(),
            severity: self.severity,
            implicit: self.is_implicit(),
            rule_index: None,
        }
    }

    /// [`DrcRule::info`] of this rule as `DrcEngine::rules()[ rule_index ]`: the pointer
    /// identity of the C++ is the index of the rule in the engine's rule list.
    pub fn info_in_engine(&self, rule_index: u32) -> DrcRuleInfo {
        DrcRuleInfo {
            rule_index: Some(rule_index),
            ..self.info()
        }
    }

    /// Refresh the rule copies held by this rule's constraints after the rule changed, which
    /// is what the C++ pointers would observe. Valid for a rule whose constraints were all
    /// added with [`DrcRule::add_constraint`] (every rule copy they hold is this rule), which
    /// is the case for rules being built by the parser.
    pub fn sync_constraint_rule_info(&mut self) {
        let info = self.info();

        for c in &mut self.constraints {
            for slot in [
                &mut c.parent_rule,
                &mut c.min_rule,
                &mut c.opt_rule,
                &mut c.max_rule,
            ] {
                if slot.is_some() {
                    *slot = Some(info.clone());
                }
            }
        }
    }
}

/// The methods of `DRC_CONSTRAINT` that refer to rules (see the module comment).
pub trait DrcConstraintExt {
    /// Port of `DRC_CONSTRAINT::IsUnary`.
    fn is_unary(&self) -> bool;

    /// Port of `DRC_CONSTRAINT::SetParentRule`: attribute the constraint, including each min,
    /// opt and max value it has, to `parent_rule`.
    fn set_parent_rule(&mut self, parent_rule: Option<&DrcRuleInfo>);

    /// Port of `DRC_CONSTRAINT::SetValueRules`.
    fn set_value_rules(
        &mut self,
        min_rule: Option<&DrcRuleInfo>,
        opt_rule: Option<&DrcRuleInfo>,
        max_rule: Option<&DrcRuleInfo>,
    );

    /// Port of `DRC_CONSTRAINT::GetName`: `GetRuleName( m_parentRule )`.
    fn name(&self) -> String;

    /// Port of `DRC_CONSTRAINT::GetRuleName`: a user-facing name for `rule`, or this
    /// constraint's own name when `rule` is `None` (for constraints from board setup or local
    /// overrides rather than rules).
    fn rule_name(&self, rule: Option<&DrcRuleInfo>) -> String;

    /// Port of `DRC_CONSTRAINT::SetOptionsFromOther`.
    fn set_options_from_other(&mut self, other: &DrcConstraint);

    /// Port of `DRC_CONSTRAINT::Merge`.
    fn merge(&mut self, other: &DrcConstraint);
}

impl DrcConstraintExt for DrcConstraint {
    fn is_unary(&self) -> bool {
        use DrcConstraintT::*;

        // Classify by the items supplied to rule evaluation, not the geometry measured by the
        // test.
        matches!(
            self.type_,
            HoleSizeConstraint
                | TextHeightConstraint
                | TextThicknessConstraint
                | TrackSegmentLengthConstraint
                | AnnularWidthConstraint
                | SolderMaskExpansionConstraint
                | SolderPasteAbsMarginConstraint
                | SolderPasteRelMarginConstraint
                | DisallowConstraint
                | ViaDiameterConstraint
                | LengthConstraint
                | NetChainLengthConstraint
                | NetChainStubLengthConstraint
                | NetChainReturnPathConstraint
                | SkewConstraint
                | ViaCountConstraint
                | AssertionConstraint
                | ViaDanglingConstraint
                | BridgedMaskConstraint
                | MicroviaStackDepthConstraint
                | HoleAspectRatioConstraint
        )
    }

    fn set_parent_rule(&mut self, parent_rule: Option<&DrcRuleInfo>) {
        self.parent_rule = parent_rule.cloned();
        self.min_rule = if self.value.has_min() {
            parent_rule.cloned()
        } else {
            None
        };
        self.opt_rule = if self.value.has_opt() {
            parent_rule.cloned()
        } else {
            None
        };
        self.max_rule = if self.value.has_max() {
            parent_rule.cloned()
        } else {
            None
        };
    }

    fn set_value_rules(
        &mut self,
        min_rule: Option<&DrcRuleInfo>,
        opt_rule: Option<&DrcRuleInfo>,
        max_rule: Option<&DrcRuleInfo>,
    ) {
        self.min_rule = min_rule.cloned();
        self.opt_rule = opt_rule.cloned();
        self.max_rule = max_rule.cloned();
    }

    // GetName, GetRuleName and Merge are inherent methods of `DrcConstraint`
    // (`kicad_pcb::drc_api`, integration step 2); the trait methods forward to them.
    fn name(&self) -> String {
        DrcConstraint::name(self)
    }

    fn rule_name(&self, rule: Option<&DrcRuleInfo>) -> String {
        DrcConstraint::rule_name(self, rule)
    }

    fn set_options_from_other(&mut self, other: &DrcConstraint) {
        self.options = other.options;
    }

    fn merge(&mut self, other: &DrcConstraint) {
        DrcConstraint::merge(self, other)
    }
}
