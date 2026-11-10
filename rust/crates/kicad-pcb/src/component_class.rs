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

//! Port of `pcbnew/component_classes/component_class.h`, `component_class_cache_proxy.h`,
//! `component_class_assignment_rule.h`, `component_class_manager.h` and their `.cpp` files
//! (`COMPONENT_CLASS`, `COMPONENT_CLASS_MANAGER`, `COMPONENT_CLASS_CACHE_PROXY`).
//!
//! C++ shares mutable `COMPONENT_CLASS` objects by pointer; here they are `Arc` values. The
//! manager's maps are the owners: a usage context is changed on the map entry
//! (`Arc::make_mut`) and always read from there (C++ only ever reads it through the map or
//! through the constituents of an effective class, which are the map entries of the same
//! names). Pointer identity (`FinishNetlistUpdate` clearing footprint pointers) is the pair
//! (`is_constituent`, name), unique within one manager.
//!
//! Also ports `common/project/component_class_settings.cpp` / `.h` (`COMPONENT_CLASS_SETTINGS`
//! load side and `COMPONENT_CLASS_ASSIGNMENT_DATA`, including `GetAssignmentInDRCLanguage`)
//! and `component_class_assignment_rule.cpp` (`COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`).
//!
//! kicad-drc depends on kicad-pcb, so the DRC rule parser and the condition evaluator reach the
//! manager through two hooks defined here: [`ComponentClassRuleCompiler`] (the
//! `DRC_RULES_PARSER::ParseComponentClassAssignmentRules` part of `CompileAssignmentRule`,
//! installed on the manager by the session's load sequence, see
//! `kicad_drc::component_class_rules::install`) and [`ComponentClassRuleCondition`] (the
//! compiled `DRC_RULE_CONDITION` of a rule). With no assignments and sheet classes off,
//! `SyncDynamicComponentClassAssignments` needs no hook.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_core::json::Value;
use kicad_core::json_settings::{JsonSettingsInternals, NestedSettings};
use kicad_core::settings_parameters::{JsonValue, ParamLambda};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::footprint::Footprint;
use crate::ids::FootprintId;
use crate::item_ref::{AnyItem, ItemRef};

/// `COMPONENT_CLASS::USAGE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum ComponentClassUsage {
    #[default]
    Static,
    Dynamic,
    StaticAndDynamic,
    Effective,
}

/// `COMPONENT_CLASS`. The C++ `m_constituentClasses` is a `COMPONENT_CLASS*` vector with three
/// shapes: empty (the "none" class and a freshly constructed class), `{ this }` (a constituent
/// class, `getOrCreateConstituentClass`) and the constituents of an effective class. An owned
/// value cannot list itself, so the self entry is the flag `is_constituent`; the name plays no
/// part in telling the shapes apart (a constituent class may have an empty name).
#[derive(Clone, Debug, Default)]
pub struct ComponentClass {
    pub name: String, // m_name
    /// m_constituentClasses of an effective class, in C++ order. Empty for the "none" class
    /// and for a constituent class.
    pub constituent_classes: Vec<Arc<ComponentClass>>,
    /// m_constituentClasses is `{ this }`.
    pub is_constituent: bool,
    pub usage_context: ComponentClassUsage, // m_usageContext
}

impl ComponentClass {
    /// `COMPONENT_CLASS::COMPONENT_CLASS( name, aUsageContext )`: no constituents.
    pub fn new(name: &str, usage_context: ComponentClassUsage) -> Self {
        ComponentClass {
            name: name.to_string(),
            constituent_classes: Vec::new(),
            is_constituent: false,
            usage_context,
        }
    }

    /// The constructor followed by `AddConstituentClass( this )`: a constituent class, as
    /// `COMPONENT_CLASS_MANAGER::getOrCreateConstituentClass` builds it.
    pub fn new_constituent(name: &str, usage_context: ComponentClassUsage) -> Self {
        ComponentClass {
            is_constituent: true,
            ..ComponentClass::new(name, usage_context)
        }
    }

    /// `m_constituentClasses.size()`.
    pub fn constituent_count(&self) -> usize {
        if self.is_constituent {
            1
        } else {
            self.constituent_classes.len()
        }
    }

    /// `COMPONENT_CLASS::GetName`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `COMPONENT_CLASS::GetUsageContext`.
    pub fn usage_context(&self) -> ComponentClassUsage {
        self.usage_context
    }

    /// The names of `m_constituentClasses` in order: the class's own name for a constituent
    /// class (`{ this }`), the constituents' names for an effective class, none for the "none"
    /// class.
    pub fn constituent_names(&self) -> Vec<&str> {
        if self.is_constituent {
            vec![self.name.as_str()]
        } else {
            self.constituent_classes
                .iter()
                .map(|c| c.name.as_str())
                .collect()
        }
    }

    /// `COMPONENT_CLASS::GetHumanReadableName`.
    pub fn get_human_readable_name(&self) -> String {
        let names = self.constituent_names();

        if names.is_empty() {
            return "<None>".to_string();
        }

        if names.len() == 1 {
            return self.name.clone();
        }

        debug_assert!(names.len() >= 2);

        if names.len() == 2 {
            format!("{} and {}", names[0], names[1])
        } else if names.len() == 3 {
            format!("{}, {} and {}", names[0], names[1], names[2])
        } else {
            format!(
                "{}, {} and {} more",
                names[0],
                names[1],
                (names.len() - 2) as i32
            )
        }
    }

    /// `COMPONENT_CLASS::GetConstituentClass( className ) != nullptr`.
    pub fn has_constituent_class(&self, name: &str) -> bool {
        self.constituent_names().contains(&name)
    }

    /// `COMPONENT_CLASS::ContainsClassName`.
    pub fn contains_class_name(&self, name: &str) -> bool {
        self.has_constituent_class(name)
    }

    /// `COMPONENT_CLASS::operator==`: equal names.
    pub fn eq_class(&self, other: &ComponentClass) -> bool {
        self.name == other.name
    }

    /// The identity of the C++ object within one manager (see the module comment).
    fn identity(&self) -> (bool, &str) {
        (self.is_constituent, self.name.as_str())
    }

    /// `COMPONENT_CLASS::IsEmpty`: `m_constituentClasses.empty()`.
    pub fn is_empty(&self) -> bool {
        self.constituent_count() == 0
    }
}

/// The mutable part of `COMPONENT_CLASS_CACHE_PROXY` (held by a footprint in a `CacheCell`).
/// Complete; `Default` = the C++ member initialisers (`m_lastTickerValue{ -1 }`: invalid).
#[derive(Clone, Debug)]
pub struct ComponentClassCache {
    pub last_ticker_value: i64, // m_lastTickerValue
    pub dynamic_component_class: Option<Arc<ComponentClass>>, // m_dynamicComponentClass
    pub final_component_class: Option<Arc<ComponentClass>>, // m_finalComponentClass
}

impl Default for ComponentClassCache {
    fn default() -> Self {
        ComponentClassCache {
            last_ticker_value: -1,
            dynamic_component_class: None,
            final_component_class: None,
        }
    }
}

/// The compiled condition of a component class assignment rule (`DRC_RULE_CONDITION`,
/// implemented by kicad-drc).
pub trait ComponentClassRuleCondition: Send + Sync + std::fmt::Debug {
    /// `m_condition->EvaluateFor( aFootprint, nullptr, 0, aLayer, nullptr )`.
    fn evaluate_for_footprint(&self, footprint: ItemRef<'_>, layer: PcbLayerId) -> bool;
}

/// The DRC rule compiler used by `COMPONENT_CLASS_MANAGER::CompileAssignmentRule`
/// (implemented by kicad-drc on its rule parser).
pub trait ComponentClassRuleCompiler: Send + Sync {
    /// The `DRC_RULES_PARSER` part of `CompileAssignmentRule` on a non-empty rule source:
    /// `ParseComponentClassAssignmentRules` with a `WX_STRING_REPORTER`; `None` when the
    /// reporter got an error, a `PARSE_ERROR` was thrown, or the source did not give exactly
    /// one (non-null) rule.
    fn compile(&self, rule_source: &str) -> Option<ComponentClassAssignmentRule>;
}

/// The installed [`ComponentClassRuleCompiler`] (a shared hook; `Debug` prints a marker).
#[derive(Clone)]
pub struct ComponentClassRuleCompilerHook(pub Arc<dyn ComponentClassRuleCompiler>);

impl std::fmt::Debug for ComponentClassRuleCompilerHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ComponentClassRuleCompilerHook")
    }
}

/// Port of `COMPONENT_CLASS_ASSIGNMENT_RULE`.
#[derive(Clone, Debug, Default)]
pub struct ComponentClassAssignmentRule {
    /// `m_componentClass`: the component class to assign to matching footprints.
    pub component_class: String,
    /// `m_condition`: the DRC condition which specifies footprint matches for this component
    /// class (`None`: the rule has no condition and matches every footprint).
    pub condition: Option<Arc<dyn ComponentClassRuleCondition>>,
}

impl ComponentClassAssignmentRule {
    /// `COMPONENT_CLASS_ASSIGNMENT_RULE::GetComponentClass`.
    pub fn component_class(&self) -> &str {
        &self.component_class
    }

    /// Port of `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`: tests whether this rule matches the
    /// given footprint.
    pub fn matches(&self, footprint: &Footprint, ctx: &BoardCtx<'_>) -> bool {
        let Some(condition) = &self.condition else {
            return true;
        };

        let side = footprint.side(ctx);
        condition.evaluate_for_footprint(ctx.view(AnyItem::Footprint(footprint)), side)
    }
}

/// `COMPONENT_CLASS_MANAGER`. Dropped: `m_board` (passed as context). The C++
/// `unordered_map` / `unordered_set` members are only looked up and erased, never iterated for
/// output; they are `BTreeMap` / `BTreeSet` here.
#[derive(Clone, Debug)]
pub struct ComponentClassManager {
    pub none_class: Arc<ComponentClass>, // m_noneClass
    pub constituent_classes: BTreeMap<String, Arc<ComponentClass>>, // m_constituentClasses
    pub effective_classes: BTreeMap<String, Arc<ComponentClass>>, // m_effectiveClasses
    pub static_class_names_cache: BTreeSet<String>, // m_staticClassNamesCache
    /// m_assignmentRules (compiled by the installed [`ComponentClassRuleCompiler`]).
    pub assignment_rules: Vec<ComponentClassAssignmentRule>,
    pub ticker: i64, // m_ticker
    /// The DRC rule compiler behind `CompileAssignmentRule` (no C++ member: the C++ calls
    /// `DRC_RULES_PARSER` directly). Installed by the load sequence
    /// (`kicad_drc::component_class_rules::install`).
    pub rule_compiler: Option<ComponentClassRuleCompilerHook>,
}

impl Default for ComponentClassManager {
    /// `COMPONENT_CLASS_MANAGER::COMPONENT_CLASS_MANAGER( board )`: the "none" class (empty
    /// name, `STATIC`), ticker 0 (`m_ticker{ 0 }`).
    fn default() -> Self {
        ComponentClassManager {
            none_class: Arc::new(ComponentClass::new("", ComponentClassUsage::Static)),
            constituent_classes: BTreeMap::new(),
            effective_classes: BTreeMap::new(),
            static_class_names_cache: BTreeSet::new(),
            assignment_rules: Vec::new(),
            ticker: 0,
            rule_compiler: None,
        }
    }
}

impl ComponentClassManager {
    /// `COMPONENT_CLASS_MANAGER::GetTicker`.
    pub fn ticker(&self) -> i64 {
        self.ticker
    }

    /// `COMPONENT_CLASS_MANAGER::GetNoneComponentClass`.
    pub fn none_component_class(&self) -> &Arc<ComponentClass> {
        &self.none_class
    }

    /// `COMPONENT_CLASS_MANAGER::GetEffectiveStaticComponentClass`: the class a netlist update
    /// assigns to a footprint with the static class names `class_names` (the "none" class, a
    /// constituent class or an effective class), marking the names as used.
    pub fn get_effective_static_component_class(
        &mut self,
        class_names: &BTreeSet<String>,
    ) -> Arc<ComponentClass> {
        // Handle no component class condition
        if class_names.is_empty() {
            return self.none_class.clone();
        }

        // Handle single-assignment component classes
        if class_names.len() == 1 {
            let class_name = class_names.iter().next().cloned().unwrap_or_default();
            self.static_class_names_cache.remove(&class_name);
            return self.get_or_create_constituent_class(&class_name, ComponentClassUsage::Static);
        }

        // Handle composite component classes
        let sorted_class_names = Self::sort_class_names(class_names);

        let effective_class =
            self.get_or_create_effective_class(&sorted_class_names, ComponentClassUsage::Static);

        for constituent_name in effective_class.constituent_names() {
            self.static_class_names_cache.remove(constituent_name);
        }

        effective_class
    }

    /// `COMPONENT_CLASS_MANAGER::InitNetlistUpdate`: remember every class used statically, so
    /// that `FinishNetlistUpdate` can remove the ones the update did not assign.
    pub fn init_netlist_update(&mut self) {
        for (class_name, class_ptr) in &self.constituent_classes {
            if class_ptr.usage_context == ComponentClassUsage::Static
                || class_ptr.usage_context == ComponentClassUsage::StaticAndDynamic
            {
                self.static_class_names_cache.insert(class_name.clone());
            }
        }

        self.ticker += 1;
    }

    /// The first loop of `COMPONENT_CLASS_MANAGER::FinishNetlistUpdate`: the identities of the
    /// classes it will delete (static-only classes not used by the update and the effective
    /// classes containing them).
    fn classes_to_delete(&self) -> BTreeSet<(bool, String)> {
        let mut classes_to_delete = BTreeSet::new();

        for class_name in &self.static_class_names_cache {
            let Some(static_class) = self.constituent_classes.get(class_name) else {
                continue;
            };

            if static_class.usage_context == ComponentClassUsage::Static {
                classes_to_delete.insert((true, class_name.clone()));

                for combined_comp_class in self.effective_classes.values() {
                    if combined_comp_class.contains_class_name(class_name) {
                        classes_to_delete.insert((false, combined_comp_class.name.clone()));
                    }
                }
            }
        }

        classes_to_delete
    }

    /// `COMPONENT_CLASS_MANAGER::FinishNetlistUpdate`, the part on the manager: delete any
    /// effective component classes which refer to static classes unused by the netlist update
    /// in a static-only context, or update their usage context.
    ///
    /// The C++ also clears the static class of every footprint that refers to a deleted class
    /// (issue 22623); that needs the board: use [`Board::finish_component_class_netlist_update`].
    pub fn finish_netlist_update(&mut self) {
        let cache: Vec<String> = self.static_class_names_cache.iter().cloned().collect();

        // Now perform the actual deletions
        for class_name in &cache {
            let Some(static_class) = self.constituent_classes.get(class_name) else {
                // C++ dereferences a null pointer created by operator[] here.
                continue;
            };

            if static_class.usage_context == ComponentClassUsage::Static {
                // Any static-only classes can be deleted, along with effective classes which
                // refer to them
                self.effective_classes
                    .retain(|_, combined| !combined.contains_class_name(class_name));

                self.constituent_classes.remove(class_name);
            } else {
                // Set the component class to dynamic-only scope
                debug_assert!(static_class.usage_context == ComponentClassUsage::StaticAndDynamic);
                self.set_usage_context(class_name, ComponentClassUsage::Dynamic);
            }
        }

        // Clear the caches
        self.static_class_names_cache.clear();
    }

    /// `COMPONENT_CLASS_MANAGER::GetFullClassNameForConstituents( const std::vector<wxString>& )`:
    /// the names joined with commas.
    pub fn get_full_class_name_for_constituents(class_names: &[String]) -> String {
        class_names.join(",")
    }

    /// `COMPONENT_CLASS_MANAGER::GetFullClassNameForConstituents( const std::unordered_set& )`.
    pub fn get_full_class_name_for_constituent_set(class_names: &BTreeSet<String>) -> String {
        Self::get_full_class_name_for_constituents(&Self::sort_class_names(class_names))
    }

    /// `COMPONENT_CLASS_MANAGER::sortClassNames`: `wxString::Cmp` order, which is code point
    /// order, the order of a `BTreeSet<String>` (the names are distinct, so ties cannot occur).
    pub fn sort_class_names(class_names: &BTreeSet<String>) -> Vec<String> {
        class_names.iter().cloned().collect()
    }

    /// Port of `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments`: compiles the
    /// assignments (and, with `generate_sheet_classes`, one rule per sheet of `new_sheet_paths`
    /// and of `footprint_sheet_names`, the `GetSheetname()` of every board footprint) and
    /// installs them if all compiled. False when a rule failed to compile.
    ///
    /// The C++ iterates `std::unordered_set`s here; the order only decides the order of
    /// `m_assignmentRules`, which no result depends on (matching collects into a set and
    /// sorts), so `BTreeSet`s are used.
    pub fn sync_dynamic_component_class_assignments<'s>(
        &mut self,
        assignments: &[ComponentClassAssignmentData],
        generate_sheet_classes: bool,
        new_sheet_paths: &BTreeSet<String>,
        footprint_sheet_names: impl IntoIterator<Item = &'s str>,
    ) -> bool {
        let mut success = true;

        // Invalidate component class cache entries
        self.ticker += 1;

        // Save previous dynamically assigned component class names
        let mut prev_class_names: BTreeSet<String> = BTreeSet::new();

        for rule in &self.assignment_rules {
            prev_class_names.insert(rule.component_class.clone());
        }

        self.assignment_rules.clear();

        // Parse all assignment rules
        let mut rules: Vec<ComponentClassAssignmentRule> = Vec::new();

        for assignment in assignments {
            match self.compile_assignment_rule(assignment) {
                Some(rule) => rules.push(rule),
                None => success = false,
            }
        }

        // Generate sheet classes if required
        if generate_sheet_classes {
            let mut sheet_names: BTreeSet<String> = new_sheet_paths.clone();

            for sheet_name in footprint_sheet_names {
                sheet_names.insert(sheet_name.to_string());
            }

            for sheet_name in sheet_names {
                // Don't generate a class for empty sheets (e.g. manually placed footprints) or
                // the root sheet
                if sheet_name.is_empty() || sheet_name == "/" {
                    continue;
                }

                let sheet_name = sheet_name.replace(['"', '\''], "");

                let mut assignment = ComponentClassAssignmentData::default();
                assignment.set_component_class(&sheet_name);
                assignment.add_condition(ConditionType::SheetName, &sheet_name, "");

                match self.compile_assignment_rule(&assignment) {
                    Some(rule) => rules.push(rule),
                    None => success = false,
                }
            }
        }

        // Set the assignment rules
        if success {
            self.assignment_rules = rules;
        }

        // Re-use or create component classes which may be output by assignment rules
        let class_names: Vec<String> = self
            .assignment_rules
            .iter()
            .map(|rule| rule.component_class.clone())
            .collect();

        for class_name in &class_names {
            prev_class_names.remove(class_name);
            self.get_or_create_constituent_class(class_name, ComponentClassUsage::Dynamic);
        }

        // prevClassNames now contains all dynamic component class names no longer in use.
        // Remove any effective component classes no longer in use.
        for class_name in &prev_class_names {
            // C++ creates a null entry with operator[] (and crashes) for an unknown name;
            // every previous rule class was created above in the previous run.
            let Some(dynamic_class) = self.constituent_classes.get(class_name) else {
                continue;
            };

            if dynamic_class.usage_context == ComponentClassUsage::Dynamic {
                // Any dynamic-only classes can be deleted, along with effective classes which
                // refer to them
                self.effective_classes
                    .retain(|_, combined| !combined.contains_class_name(class_name));

                self.constituent_classes.remove(class_name);
            } else {
                // Set the component class to dynamic-only scope
                debug_assert!(dynamic_class.usage_context == ComponentClassUsage::StaticAndDynamic);
                self.set_usage_context(class_name, ComponentClassUsage::Static);
            }
        }

        success
    }

    /// Port of `COMPONENT_CLASS_MANAGER::CompileAssignmentRule`: the assignment in the DRC
    /// language compiled by the installed [`ComponentClassRuleCompiler`]; `None` for an
    /// incomplete assignment or a rule that does not compile.
    ///
    /// # Panics
    ///
    /// When a rule has to be compiled and no compiler is installed (a programming error: the
    /// load sequence installs `kicad_drc::component_class_rules`).
    pub fn compile_assignment_rule(
        &self,
        assignment: &ComponentClassAssignmentData,
    ) -> Option<ComponentClassAssignmentRule> {
        let rule_source = assignment.get_assignment_in_drc_language();

        // Ignore incomplete rules (e.g. no component class name specified)
        if rule_source.is_empty() {
            return None;
        }

        let Some(compiler) = &self.rule_compiler else {
            panic!(
                "COMPONENT_CLASS_MANAGER::CompileAssignmentRule: no component class rule \
                 compiler installed (call kicad_drc::component_class_rules::install on the board)"
            );
        };

        compiler.0.compile(&rule_source)
    }

    /// `COMPONENT_CLASS_MANAGER::GetCombinedComponentClass`: the class combining the
    /// constituents of a footprint's static and dynamic classes.
    pub fn get_combined_component_class(
        &mut self,
        static_class: Option<&ComponentClass>,
        dynamic_class: Option<&ComponentClass>,
    ) -> Arc<ComponentClass> {
        let mut class_names = BTreeSet::new();

        if let Some(static_class) = static_class {
            for name in static_class.constituent_names() {
                class_names.insert(name.to_string());
            }
        }

        if let Some(dynamic_class) = dynamic_class {
            for name in dynamic_class.constituent_names() {
                class_names.insert(name.to_string());
            }
        }

        if class_names.is_empty() {
            return self.none_class.clone();
        }

        if class_names.len() == 1 {
            let name = class_names.iter().next().cloned().unwrap_or_default();
            debug_assert!(self.constituent_classes.contains_key(&name));

            // operator[] creates a null entry for an unknown name (a C++ crash later on)
            return match self.constituent_classes.get(&name) {
                Some(class) => class.clone(),
                None => self.none_class.clone(),
            };
        }

        let sorted_class_names = Self::sort_class_names(&class_names);
        let full_combined_name = Self::get_full_class_name_for_constituents(&sorted_class_names);

        if !self.effective_classes.contains_key(&full_combined_name) {
            let mut combined_class =
                ComponentClass::new(&full_combined_name, ComponentClassUsage::Effective);

            for class_name in &sorted_class_names {
                debug_assert!(self.constituent_classes.contains_key(class_name));

                if let Some(constituent) = self.constituent_classes.get(class_name) {
                    combined_class.constituent_classes.push(constituent.clone());
                }
            }

            self.effective_classes
                .insert(full_combined_name.clone(), Arc::new(combined_class));
        }

        self.effective_classes[&full_combined_name].clone()
    }

    /// `COMPONENT_CLASS_MANAGER::GetDynamicComponentClassesForFootprint`: the class made of the
    /// assignment rules that match the footprint. `rule_matches` is
    /// `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches( footprint )` (the compiled DRC condition,
    /// kicad-drc), called once per rule in rule order.
    pub fn get_dynamic_component_classes_for_footprint(
        &mut self,
        mut rule_matches: impl FnMut(&ComponentClassAssignmentRule) -> bool,
    ) -> Option<Arc<ComponentClass>> {
        let mut class_names = BTreeSet::new();

        // Assemble matching component class names
        for rule in &self.assignment_rules {
            if rule_matches(rule) {
                class_names.insert(rule.component_class.clone());
            }
        }

        // Handle composite component classes
        let sorted_class_names = Self::sort_class_names(&class_names);

        // No matching component classes
        if class_names.is_empty() {
            return None;
        }

        // One matching component class
        if class_names.len() == 1 {
            return Some(self.get_or_create_constituent_class(
                &sorted_class_names[0],
                ComponentClassUsage::Dynamic,
            ));
        }

        // Multiple matching component classes
        Some(self.get_or_create_effective_class(&sorted_class_names, ComponentClassUsage::Dynamic))
    }

    /// `COMPONENT_CLASS_CACHE_PROXY::RecomputeComponentClass`: recompute the dynamic and the
    /// final class of a footprint into `cache` (`rule_matches` as in
    /// [`get_dynamic_component_classes_for_footprint`](Self::get_dynamic_component_classes_for_footprint)).
    pub fn recompute_component_class(
        &mut self,
        cache: &mut ComponentClassCache,
        static_class: Option<&ComponentClass>,
        rule_matches: impl FnMut(&ComponentClassAssignmentRule) -> bool,
    ) {
        cache.dynamic_component_class =
            self.get_dynamic_component_classes_for_footprint(rule_matches);
        let dynamic = cache.dynamic_component_class.clone();
        cache.final_component_class =
            Some(self.get_combined_component_class(static_class, dynamic.as_deref()));
        cache.last_ticker_value = self.ticker;
    }

    /// `COMPONENT_CLASS_CACHE_PROXY::GetComponentClass`: recompute the cache if the manager's
    /// ticker moved on, then the final class.
    pub fn get_component_class(
        &mut self,
        cache: &mut ComponentClassCache,
        static_class: Option<&ComponentClass>,
        rule_matches: impl FnMut(&ComponentClassAssignmentRule) -> bool,
    ) -> Option<Arc<ComponentClass>> {
        if self.ticker > cache.last_ticker_value {
            self.recompute_component_class(cache, static_class, rule_matches);
        }

        cache.final_component_class.clone()
    }

    /// `COMPONENT_CLASS_MANAGER::GetClassNames`: the names of all constituent classes.
    pub fn get_class_names(&self) -> BTreeSet<String> {
        self.constituent_classes.keys().cloned().collect()
    }

    /// Set the usage context of the constituent class `name` (the map entry).
    fn set_usage_context(&mut self, name: &str, usage: ComponentClassUsage) {
        if let Some(class) = self.constituent_classes.get_mut(name) {
            Arc::make_mut(class).usage_context = usage;
        }
    }

    /// `COMPONENT_CLASS_MANAGER::getOrCreateConstituentClass`.
    pub fn get_or_create_constituent_class(
        &mut self,
        class_name: &str,
        context: ComponentClassUsage,
    ) -> Arc<ComponentClass> {
        if context == ComponentClassUsage::StaticAndDynamic
            || context == ComponentClassUsage::Effective
        {
            // wxFAIL_MSG( "Can't create a STATIC_AND_DYNAMIC or EFFECTIVE constituent
            // component class" )
            return self.none_class.clone();
        }

        if let Some(comp_class) = self.constituent_classes.get(class_name) {
            if context != comp_class.usage_context {
                self.set_usage_context(class_name, ComponentClassUsage::StaticAndDynamic);
            }

            return self.constituent_classes[class_name].clone();
        }

        let new_class = Arc::new(ComponentClass::new_constituent(class_name, context));
        self.constituent_classes
            .insert(class_name.to_string(), new_class.clone());

        new_class
    }

    /// `COMPONENT_CLASS_MANAGER::getOrCreateEffectiveClass`.
    pub fn get_or_create_effective_class(
        &mut self,
        class_names: &[String],
        context: ComponentClassUsage,
    ) -> Arc<ComponentClass> {
        let full_class_name = Self::get_full_class_name_for_constituents(class_names);

        if let Some(comp_class) = self.effective_classes.get(&full_class_name).cloned() {
            for constituent_name in comp_class.constituent_names() {
                let usage = self
                    .constituent_classes
                    .get(constituent_name)
                    .map(|c| c.usage_context);

                if usage.is_some_and(|u| u != context) {
                    self.set_usage_context(constituent_name, ComponentClassUsage::StaticAndDynamic);
                }
            }
        } else {
            let mut effective_class =
                ComponentClass::new(&full_class_name, ComponentClassUsage::Effective);

            for class_name in class_names {
                let constituent_class = self.get_or_create_constituent_class(class_name, context);
                effective_class.constituent_classes.push(constituent_class);
            }

            self.effective_classes
                .insert(full_class_name.clone(), Arc::new(effective_class));
        }

        self.effective_classes[&full_class_name].clone()
    }

    /// `COMPONENT_CLASS_MANAGER::RebuildRequiredCaches`: C++ rebuilds the courtyard caches of
    /// the footprints eagerly (`BuildCourtyardCaches`). The port's courtyard cache is a lazy
    /// keyed cache rebuilt on demand from its inputs (board-model.md 8.4), so there is nothing
    /// to do.
    pub fn rebuild_required_caches(&mut self) {}

    /// `COMPONENT_CLASS_MANAGER::InvalidateComponentClasses` (`++m_ticker`).
    pub fn invalidate_component_classes(&mut self) {
        self.ticker += 1;
    }
}

// ---- COMPONENT_CLASS_ASSIGNMENT_DATA / COMPONENT_CLASS_SETTINGS ------------------------------

/// `COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITION_TYPE`: a condition match type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConditionType {
    Reference,
    Footprint,
    Side,
    Rotation,
    FootprintField,
    Custom,
    SheetName,
}

/// `COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITIONS_OPERATOR`: whether conditions are applied with
/// OR or AND logic.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ConditionsOperator {
    #[default]
    All,
    Any,
}

/// Port of `COMPONENT_CLASS_ASSIGNMENT_DATA` (common/project/component_class_settings.h).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentClassAssignmentData {
    /// `m_componentClass`: the name of the component class for this assignment rule.
    pub component_class: String,
    /// `m_conditions`: type, primary and secondary data fields of each condition.
    pub conditions: Vec<(ConditionType, String, String)>,
    /// `m_conditionsOperator`: whether conditions are applied with AND or OR logic (default
    /// ALL).
    pub conditions_operator: ConditionsOperator,
}

/// `wxString::Trim( true ).Trim( false )`: strips `wxSafeIsspace` characters (ASCII white
/// space) from both ends. Also `wxString::Strip( wxString::both )`.
fn wx_trim_both(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

/// `wxSplit( aStr, aSep )` with the default escape character `\`: an escaped separator is kept
/// (the escape removed), an escaped escape character stands for one escape character, the
/// empty string gives no token, otherwise there is always a last token. The handling of
/// escape characters is the measured behaviour of the reference wxWidgets 3.2 (every string
/// of up to five characters over `a`, `\` and `,`, golden `cc1_component_class.txt`).
fn wx_split(s: &str, sep: char) -> Vec<String> {
    let escape = '\\';
    let mut ret = Vec::new();
    let mut curr = String::new();
    let mut prev = '\0';

    for ch in s.chars() {
        if ch == sep {
            if prev == escape {
                // remove the escape character and don't consider this occurrence of 'sep' as a
                // real separator
                curr.pop();
                curr.push(sep);
            } else {
                // real separator
                ret.push(std::mem::take(&mut curr));
            }

            prev = ch;
        } else if ch == escape && prev == escape {
            // an escaped escape character: the one already added stands for both
            prev = '\0';
        } else {
            // normal character
            curr.push(ch);
            prev = ch;
        }
    }

    // add the last token, which we always have unless the string is empty
    if !s.is_empty() {
        ret.push(curr);
    }

    ret
}

/// `wxString::ToInt( &val )` (base 10): `wcstol` must consume the whole, non-empty string
/// without `ERANGE` and the value must fit an `int`. The string reaching it is already
/// trimmed, so the white space `wcstol` skips is not considered.
fn wx_to_int(s: &str) -> bool {
    let digits = s
        .strip_prefix('+')
        .or_else(|| s.strip_prefix('-'))
        .unwrap_or(s);

    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }

    // a value out of the range of `long` sets ERANGE; out of `int` fails the range check
    s.parse::<i64>()
        .is_ok_and(|v| v >= i32::MIN as i64 && v <= i32::MAX as i64)
}

impl ComponentClassAssignmentData {
    /// `AddCondition`: sets the given condition type with the associated match data.
    pub fn add_condition(&mut self, condition: ConditionType, primary: &str, secondary: &str) {
        self.conditions
            .push((condition, primary.to_string(), secondary.to_string()));
    }

    /// `SetComponentClass`.
    pub fn set_component_class(&mut self, component_class: &str) {
        self.component_class = component_class.to_string();
    }

    /// `SetConditionsOperation`.
    pub fn set_conditions_operation(&mut self, operator: ConditionsOperator) {
        self.conditions_operator = operator;
    }

    /// Port of `COMPONENT_CLASS_ASSIGNMENT_DATA::GetConditionName`.
    pub fn get_condition_name(condition: ConditionType) -> &'static str {
        match condition {
            ConditionType::Reference => "REFERENCE",
            ConditionType::Footprint => "FOOTPRINT",
            ConditionType::Side => "SIDE",
            ConditionType::Rotation => "ROTATION",
            ConditionType::FootprintField => "FOOTPRINT_FIELD",
            ConditionType::Custom => "CUSTOM",
            ConditionType::SheetName => "SHEET_NAME",
        }
    }

    /// Port of `COMPONENT_CLASS_ASSIGNMENT_DATA::GetConditionType`: an unknown name is
    /// `REFERENCE` (after a `wxASSERT_MSG`, a no-op in the reference build).
    pub fn get_condition_type(condition: &str) -> ConditionType {
        match condition {
            "REFERENCE" => ConditionType::Reference,
            "FOOTPRINT" => ConditionType::Footprint,
            "SIDE" => ConditionType::Side,
            "ROTATION" => ConditionType::Rotation,
            "FOOTPRINT_FIELD" => ConditionType::FootprintField,
            "CUSTOM" => ConditionType::Custom,
            "SHEET_NAME" => ConditionType::SheetName,
            // wxASSERT_MSG( false, "Invalid condition type" )
            _ => ConditionType::Reference,
        }
    }

    /// Port of `COMPONENT_CLASS_ASSIGNMENT_DATA::GetAssignmentInDRCLanguage`: the DRC rules
    /// language for this component class assignment (empty without a class name).
    pub fn get_assignment_in_drc_language(&self) -> String {
        if self.component_class.is_empty() {
            return String::new();
        }

        if self.conditions.is_empty() {
            // A condition which always applies the netclass
            return format!(
                "(version 2) (assign_component_class \"{}\")",
                self.component_class
            );
        }

        // Lambda to format a comma-separated list of references in to a DRC expression
        let get_ref_expr = |refs: &str| -> String {
            let refs = wx_split(wx_trim_both(refs), ',');

            if refs.is_empty() {
                return String::new();
            }

            let refs: Vec<String> = refs
                .iter()
                .map(|r| format!("A.Reference == '{}'", wx_trim_both(r)))
                .collect();

            let mut refs_expr = refs[0].clone();

            for r in &refs[1..] {
                refs_expr = refs_expr + " || " + r;
            }

            format!("( {refs_expr} )")
        };

        // Lambda to format a footprint match DRC expression
        let get_footprint_expr = |footprint: &str| -> String {
            let footprint = wx_trim_both(footprint);

            if footprint.is_empty() {
                return String::new();
            }

            format!("( A.Library_Link == '{footprint}' )")
        };

        // Lambda to format a layer side DRC expression
        let get_side_expr = |side: &str| -> String {
            if side == "Any" {
                return String::new();
            }

            format!(
                "( A.Layer == '{}' )",
                if side == "Front" { "F.Cu" } else { "B.Cu" }
            )
        };

        // Lambda to format a rotation DRC expression
        let get_rotation_expr = |rotation: &str| -> String {
            let rotation = wx_trim_both(rotation);

            if rotation.is_empty() || rotation == "Any" || !wx_to_int(rotation) {
                return String::new();
            }

            format!("( A.Orientation == {rotation} deg )")
        };

        // Lambda to format a footprint field DRC expression
        let get_footprint_field_expr = |field_name: &str, field_match: &str| -> String {
            let field_name = wx_trim_both(field_name);
            let field_match = wx_trim_both(field_match);

            if field_name.is_empty() || field_match.is_empty() {
                return String::new();
            }

            format!("( A.getField('{field_name}') == '{field_match}' )")
        };

        // Lambda to format a custom DRC expression
        let get_custom_field_expr = |expr: &str| -> String {
            let expr = wx_trim_both(expr);

            if expr.is_empty() {
                return String::new();
            }

            format!("( {expr} )")
        };

        // Lambda to format a sheet name expression
        let get_sheet_name_expr = |sheet_name: &str| -> String {
            let sheet_name = wx_trim_both(sheet_name);

            if sheet_name.is_empty() {
                return String::new();
            }

            format!("( A.memberOfSheet('{sheet_name}') )")
        };

        let mut conditions_exprs: Vec<String> = Vec::new();

        for (condition_type, primary_data, secondary_data) in &self.conditions {
            let condition_expr = match condition_type {
                ConditionType::Reference => get_ref_expr(primary_data),
                ConditionType::Footprint => get_footprint_expr(primary_data),
                ConditionType::Side => get_side_expr(primary_data),
                ConditionType::Rotation => get_rotation_expr(primary_data),
                ConditionType::FootprintField => {
                    get_footprint_field_expr(primary_data, secondary_data)
                }
                ConditionType::Custom => get_custom_field_expr(primary_data),
                ConditionType::SheetName => get_sheet_name_expr(primary_data),
            };

            if !condition_expr.is_empty() {
                conditions_exprs.push(condition_expr);
            }
        }

        if conditions_exprs.is_empty() {
            return format!(
                "(version 2) (assign_component_class \"{}\")",
                self.component_class
            );
        }

        let mut all_conditions_expr = conditions_exprs[0].clone();

        let operator_expr = if self.conditions_operator == ConditionsOperator::All {
            " && "
        } else {
            " || "
        };

        for expr in &conditions_exprs[1..] {
            all_conditions_expr = all_conditions_expr + operator_expr + expr;
        }

        format!(
            "(version 2) (assign_component_class \"{}\" (condition \"{}\" ) )",
            self.component_class, all_conditions_expr
        )
    }
}

/// `componentClassSettingsSchemaVersion`.
pub const COMPONENT_CLASS_SETTINGS_SCHEMA_VERSION: i32 = 0;

/// Port of `COMPONENT_CLASS_SETTINGS` (the data; nested in the project file at
/// `component_class_settings`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentClassSettings {
    /// `m_enableSheetComponentClasses`: toggle generation of component classes for
    /// hierarchical sheets.
    pub enable_sheet_component_classes: bool,
    /// `m_componentClassAssignments`: all dynamic component class assignment rules.
    pub component_class_assignments: Vec<ComponentClassAssignmentData>,
}

/// `wxString( s.c_str(), wxConvUTF8 )`: the C string conversion stops at the first NUL.
fn wx_from_c_str(s: &str) -> String {
    match s.find('\0') {
        Some(end) => s[..end].to_string(),
        None => s.to_string(),
    }
}

/// `aJson[key].get<std::string>()` on a `const nlohmann::json&`: `None` where nlohmann throws
/// (not an object, not a string) or where the C++ is undefined (missing key, `JSON_ASSERT`).
fn json_get_string(json: &Value, key: &str) -> Option<String> {
    match json {
        Value::Object(map) => match map.get(key)? {
            Value::String(s) => Some(wx_from_c_str(s)),
            _ => None,
        },
        _ => None,
    }
}

/// `nlohmann::json::items()` of `json`: an object's entries in key byte order (`std::map`),
/// an array's elements keyed by their index, nothing for `null`, and a primitive value as one
/// entry with an empty key.
fn json_items(json: &Value) -> Vec<(String, &Value)> {
    match json {
        Value::Object(map) => {
            let mut entries: Vec<(String, &Value)> =
                map.iter().map(|(k, v)| (k.clone(), v)).collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            entries
        }
        Value::Array(list) => list
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        Value::Null => Vec::new(),
        other => vec![(String::new(), other)],
    }
}

/// `conditionData.contains( key )`: only objects contain keys.
fn json_contains(json: &Value, key: &str) -> bool {
    matches!(json, Value::Object(map) if map.contains_key(key))
}

impl ComponentClassSettings {
    /// Port of `COMPONENT_CLASS_SETTINGS::loadAssignment`; `None` where the C++ throws (the
    /// exception ends the setter of `assignments`).
    fn load_assignment(json: &Value) -> Option<ComponentClassAssignmentData> {
        let mut assignment = ComponentClassAssignmentData::default();

        assignment.set_component_class(&json_get_string(json, "component_class")?);

        let match_operator = json_get_string(json, "conditions_operator")?;

        if match_operator == "ALL" {
            assignment.set_conditions_operation(ConditionsOperator::All);
        } else {
            assignment.set_conditions_operation(ConditionsOperator::Any);
        }

        // const operator[]: throws on a non-object, undefined for a missing key
        let Value::Object(map) = json else {
            return None;
        };
        let conditions = map.get("conditions")?;

        for (condition_type_str, condition_data) in json_items(conditions) {
            let mut primary = String::new();
            let mut secondary = String::new();

            // typeStr.BeforeFirst( '-' )
            let type_str = condition_type_str
                .split('-')
                .next()
                .unwrap_or_default()
                .to_string();

            let condition_type = ComponentClassAssignmentData::get_condition_type(&type_str);

            if json_contains(condition_data, "primary") {
                primary = json_get_string(condition_data, "primary")?;
            }

            if json_contains(condition_data, "secondary") {
                secondary = json_get_string(condition_data, "secondary")?;
            }

            assignment.add_condition(condition_type, &primary, &secondary);
        }

        Some(assignment)
    }

    /// The setter of the `sheet_component_classes` parameter.
    fn sheet_component_classes_setter(&mut self, json: JsonValue) {
        let Value::Object(map) = &json.0 else {
            return;
        };

        let Some(enabled) = map.get("enabled") else {
            return;
        };

        // get<bool>() throws for anything but a boolean
        if let Value::Bool(enabled) = enabled {
            self.enable_sheet_component_classes = *enabled;
        }
    }

    /// The setter of the `assignments` parameter. An exception thrown by `loadAssignment`
    /// leaves the assignments loaded so far.
    fn assignments_setter(&mut self, json: JsonValue) {
        let Value::Array(list) = &json.0 else {
            return;
        };

        self.component_class_assignments.clear();

        for assignment_json in list {
            let Some(assignment) = Self::load_assignment(assignment_json) else {
                return;
            };

            self.component_class_assignments.push(assignment);
        }
    }

    /// The getter of the `sheet_component_classes` parameter.
    fn sheet_component_classes_getter(&self) -> JsonValue {
        let mut ret = kicad_core::json::Map::new();
        ret.insert(
            "enabled".to_string(),
            Value::Bool(self.enable_sheet_component_classes),
        );
        JsonValue(Value::Object(ret))
    }

    /// Port of the `assignments` getter (`saveAssignment` per assignment).
    fn assignments_getter(&self) -> JsonValue {
        JsonValue(Value::Array(
            self.component_class_assignments
                .iter()
                .map(Self::save_assignment)
                .collect(),
        ))
    }

    /// Port of `COMPONENT_CLASS_SETTINGS::saveAssignment`.
    fn save_assignment(assignment: &ComponentClassAssignmentData) -> Value {
        let mut ret = kicad_core::json::Map::new();

        let match_operator = if assignment.conditions_operator == ConditionsOperator::All {
            "ALL"
        } else {
            "ANY"
        };

        ret.insert(
            "component_class".to_string(),
            Value::String(assignment.component_class.clone()),
        );
        ret.insert(
            "conditions_operator".to_string(),
            Value::String(match_operator.to_string()),
        );

        let mut conditions_json = kicad_core::json::Map::new();

        for (condition_type, primary_data, secondary_data) in &assignment.conditions {
            let mut condition_json = kicad_core::json::Map::new();

            if !primary_data.is_empty() {
                condition_json.insert("primary".to_string(), Value::String(primary_data.clone()));
            }

            if !secondary_data.is_empty() {
                condition_json.insert(
                    "secondary".to_string(),
                    Value::String(secondary_data.clone()),
                );
            }

            let condition_name = ComponentClassAssignmentData::get_condition_name(*condition_type);
            let mut suffix = 1;
            let mut unique_name = condition_name.to_string();

            while conditions_json.contains_key(&unique_name) {
                unique_name = format!("{condition_name}-{suffix}");
                suffix += 1;
            }

            // an empty condition object is null in nlohmann (`conditionJson` never assigned)
            let value = if condition_json.is_empty() {
                Value::Null
            } else {
                Value::Object(condition_json)
            };
            conditions_json.insert(unique_name, value);
        }

        // an assignment without conditions stores null (`conditionsJson` never assigned)
        let conditions = if conditions_json.is_empty() {
            Value::Null
        } else {
            Value::Object(conditions_json)
        };
        ret.insert("conditions".to_string(), conditions);

        Value::Object(ret)
    }

    /// Port of the `COMPONENT_CLASS_SETTINGS( aParent, aPath )` constructor's parameters and
    /// `NESTED_SETTINGS::LoadFromFile` from the project file document `parent` (the nested
    /// object at `component_class_settings`).
    pub fn load_from_parent(parent: Option<&JsonSettingsInternals>) -> ComponentClassSettings {
        let mut nested: NestedSettings<ComponentClassSettings> = NestedSettings::new(
            COMPONENT_CLASS_SETTINGS_SCHEMA_VERSION,
            "component_class_settings",
        );

        nested.settings.add_param(Box::new(ParamLambda::new(
            "sheet_component_classes",
            Self::sheet_component_classes_getter,
            Self::sheet_component_classes_setter,
            JsonValue(Value::Null),
        )));

        nested.settings.add_param(Box::new(ParamLambda::new(
            "assignments",
            Self::assignments_getter,
            Self::assignments_setter,
            JsonValue(Value::Null),
        )));

        let mut settings = ComponentClassSettings::default();
        nested.load_from_parent(parent, &mut settings);
        settings
    }
}

impl Board {
    /// `COMPONENT_CLASS_MANAGER::FinishNetlistUpdate` with its board part: clear the static
    /// component class of every footprint that refers to a class being deleted (prevents a
    /// use-after-free in C++, issue 22623), then
    /// [`ComponentClassManager::finish_netlist_update`].
    pub fn finish_component_class_netlist_update(&mut self) {
        // First, collect all component classes that will be deleted so we can clear footprint
        // pointers before deletion.
        let classes_to_delete = self.component_class_manager.classes_to_delete();

        // Clear footprint static component class pointers that reference classes being deleted
        if !classes_to_delete.is_empty() {
            let doomed: Vec<FootprintId> = self
                .footprints()
                .filter(|(_, fp)| {
                    fp.static_component_class.as_ref().is_some_and(|c| {
                        let (constituent, name) = c.identity();
                        classes_to_delete.contains(&(constituent, name.to_string()))
                    })
                })
                .map(|(id, _)| id)
                .collect();

            for id in doomed {
                self.modify_footprint(id, |fp, _| fp.static_component_class = None);
            }
        }

        self.component_class_manager.finish_netlist_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `COMPONENT_CLASS_MANAGER::COMPONENT_CLASS_MANAGER` (`component_class_manager.cpp:33`)
    /// and the `COMPONENT_CLASS_CACHE_PROXY` initialisers.
    #[test]
    fn constructor_defaults() {
        let m = ComponentClassManager::default();
        assert_eq!(m.none_class.name, "");
        assert_eq!(m.none_class.usage_context, ComponentClassUsage::Static);
        assert_eq!(m.ticker, 0);
        assert!(m.constituent_classes.is_empty() && m.assignment_rules.is_empty());
        assert_eq!(ComponentClassCache::default().last_ticker_value, -1);
    }

    /// `COMPONENT_CLASS::IsEmpty` (`component_class.cpp`) and the three shapes of
    /// `m_constituentClasses`: the "none" class is empty, a constituent class lists itself
    /// (also with an empty name), an effective class lists its constituents.
    #[test]
    fn constituent_shapes() {
        let none = ComponentClassManager::default().none_class;
        assert!(none.is_empty());
        assert_eq!(none.constituent_count(), 0);

        let unnamed = ComponentClass::new_constituent("", ComponentClassUsage::Static);
        assert!(!unnamed.is_empty());
        assert_eq!(unnamed.constituent_count(), 1);

        let a = Arc::new(ComponentClass::new_constituent(
            "A",
            ComponentClassUsage::Static,
        ));
        let b = Arc::new(ComponentClass::new_constituent(
            "B",
            ComponentClassUsage::Static,
        ));
        let mut effective = ComponentClass::new("A,B", ComponentClassUsage::Effective);
        assert!(effective.is_empty());
        effective.constituent_classes = vec![a, b];
        assert_eq!(effective.constituent_count(), 2);
        assert!(!effective.is_empty());
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// `COMPONENT_CLASS::GetHumanReadableName`, `ContainsClassName`, and the manager's
    /// static / effective classes (`GetEffectiveStaticComponentClass`,
    /// `getOrCreateEffectiveClass`, `GetFullClassNameForConstituents`).
    #[test]
    fn names_and_effective_classes() {
        let mut m = ComponentClassManager::default();
        assert_eq!(m.none_class.get_human_readable_name(), "<None>");
        assert!(Arc::ptr_eq(
            &m.get_effective_static_component_class(&BTreeSet::new()),
            &m.none_class
        ));

        let a = m.get_effective_static_component_class(&names(&["A"]));
        assert!(a.is_constituent && a.name == "A");
        assert_eq!(a.get_human_readable_name(), "A");
        assert!(a.contains_class_name("A") && !a.contains_class_name("B"));

        let ab = m.get_effective_static_component_class(&names(&["B", "A"]));
        assert_eq!(ab.name, "A,B");
        assert_eq!(ab.usage_context, ComponentClassUsage::Effective);
        assert_eq!(ab.get_human_readable_name(), "A and B");
        assert!(ab.contains_class_name("A") && ab.contains_class_name("B"));
        assert!(!ab.contains_class_name("A,B"));

        let abc = m.get_effective_static_component_class(&names(&["C", "B", "A"]));
        assert_eq!(abc.get_human_readable_name(), "A, B and C");
        let abcde = m.get_effective_static_component_class(&names(&["E", "D", "C", "B", "A"]));
        assert_eq!(abcde.get_human_readable_name(), "A, B and 3 more");

        // the same effective class is returned again
        let ab2 = m.get_effective_static_component_class(&names(&["A", "B"]));
        assert!(Arc::ptr_eq(&ab, &ab2));
        assert_eq!(m.get_class_names(), names(&["A", "B", "C", "D", "E"]));
        assert_eq!(
            ComponentClassManager::get_full_class_name_for_constituent_set(&names(&[
                "b", "B", "a"
            ])),
            "B,a,b"
        );
    }

    /// Usage contexts: a class used statically and dynamically becomes `STATIC_AND_DYNAMIC`,
    /// an unused static class is deleted by the netlist update, a shared one goes back to
    /// `DYNAMIC` (`InitNetlistUpdate` / `FinishNetlistUpdate`), and the dynamic / combined
    /// classes of a footprint (`GetDynamicComponentClassesForFootprint`,
    /// `GetCombinedComponentClass`, the cache proxy and its ticker).
    #[test]
    fn usage_contexts_and_dynamic_classes() {
        let mut m = ComponentClassManager::default();
        m.assignment_rules = vec![
            ComponentClassAssignmentRule {
                component_class: "DYN".into(),
                condition: None,
            },
            ComponentClassAssignmentRule {
                component_class: "S".into(),
                condition: None,
            },
        ];

        m.init_netlist_update();
        let s = m.get_effective_static_component_class(&names(&["S"]));
        let st = m.get_effective_static_component_class(&names(&["S", "T"]));
        m.finish_netlist_update();
        assert_eq!(m.ticker(), 1);

        // only "DYN" matches
        let dynamic = m.get_dynamic_component_classes_for_footprint(|r| r.component_class == "DYN");
        let dynamic = dynamic.unwrap();
        assert_eq!(dynamic.name, "DYN");
        assert_eq!(
            m.constituent_classes["DYN"].usage_context,
            ComponentClassUsage::Dynamic
        );

        // both match: S becomes static and dynamic
        let both = m
            .get_dynamic_component_classes_for_footprint(|_| true)
            .unwrap();
        assert_eq!(both.name, "DYN,S");
        assert_eq!(
            m.constituent_classes["S"].usage_context,
            ComponentClassUsage::StaticAndDynamic
        );
        assert!(
            m.get_dynamic_component_classes_for_footprint(|_| false)
                .is_none()
        );

        // combined class of a footprint with static "S,T" and dynamic "DYN"
        let combined = m.get_combined_component_class(Some(&st), Some(&dynamic));
        assert_eq!(combined.name, "DYN,S,T");
        assert_eq!(m.get_combined_component_class(Some(&s), None).name, "S");
        assert!(Arc::ptr_eq(
            &m.get_combined_component_class(None, None),
            &m.none_class
        ));

        // cache proxy: recomputed only when the ticker moved on
        let mut cache = ComponentClassCache::default();
        let c = m.get_component_class(&mut cache, Some(&st), |r| r.component_class == "DYN");
        assert_eq!(c.unwrap().name, "DYN,S,T");
        assert_eq!(cache.last_ticker_value, m.ticker());
        let c = m.get_component_class(&mut cache, Some(&st), |_| false);
        assert_eq!(c.unwrap().name, "DYN,S,T");
        m.invalidate_component_classes();
        let c = m.get_component_class(&mut cache, Some(&st), |_| false);
        assert_eq!(c.unwrap().name, "S,T");

        // a netlist update that no longer uses S or T: T (static only) is deleted with the
        // effective classes containing it, S (also dynamic) becomes dynamic only
        m.init_netlist_update();
        m.finish_netlist_update();
        assert!(!m.constituent_classes.contains_key("T"));
        assert_eq!(
            m.constituent_classes["S"].usage_context,
            ComponentClassUsage::Dynamic
        );
        assert!(
            m.effective_classes
                .keys()
                .all(|k| !k.split(',').any(|n| n == "T"))
        );
        assert!(m.effective_classes.contains_key("DYN,S"));
        assert!(m.static_class_names_cache.is_empty());

        // invalid contexts give the none class
        assert!(Arc::ptr_eq(
            &m.get_or_create_constituent_class("X", ComponentClassUsage::Effective),
            &m.none_class
        ));
    }

    /// A stand-in for the kicad-drc compiler: the class name between the first pair of
    /// quotes, no condition; "BAD" does not compile. Records the rule sources.
    struct FakeCompiler(std::sync::Mutex<Vec<String>>);

    impl ComponentClassRuleCompiler for FakeCompiler {
        fn compile(&self, rule_source: &str) -> Option<ComponentClassAssignmentRule> {
            self.0.lock().unwrap().push(rule_source.to_string());
            let class = rule_source.split('"').nth(1)?.to_string();
            (class != "BAD").then_some(ComponentClassAssignmentRule {
                component_class: class,
                condition: None,
            })
        }
    }

    fn assignment(class: &str, reference: &str) -> ComponentClassAssignmentData {
        let mut a = ComponentClassAssignmentData::default();
        a.set_component_class(class);
        a.add_condition(ConditionType::Reference, reference, "");
        a
    }

    /// `SyncDynamicComponentClassAssignments` without assignments and sheet classes needs no
    /// compiler (the loaders' common case).
    #[test]
    fn sync_without_assignments_needs_no_compiler() {
        let mut m = ComponentClassManager::default();
        assert!(m.sync_dynamic_component_class_assignments(&[], false, &BTreeSet::new(), []));
        assert_eq!(m.ticker(), 1);
        assert!(m.assignment_rules.is_empty() && m.constituent_classes.is_empty());

        // an incomplete assignment (no class name) is not compiled and fails
        let incomplete = ComponentClassAssignmentData::default();
        assert!(!m.sync_dynamic_component_class_assignments(
            &[incomplete],
            false,
            &BTreeSet::new(),
            []
        ));
    }

    /// `SyncDynamicComponentClassAssignments`: rules installed only when all compile, sheet
    /// classes (empty and root sheets skipped, quotes removed), classes of rules no longer in
    /// use deleted (dynamic only) or set back to `STATIC` (also static).
    #[test]
    fn sync_dynamic_assignments() {
        let compiler = Arc::new(FakeCompiler(Default::default()));
        let mut m = ComponentClassManager {
            rule_compiler: Some(ComponentClassRuleCompilerHook(compiler.clone())),
            ..Default::default()
        };

        let first = [assignment("D1", "R1"), assignment("S", "R2")];
        assert!(m.sync_dynamic_component_class_assignments(&first, false, &BTreeSet::new(), []));
        assert_eq!(
            compiler.0.lock().unwrap()[0],
            "(version 2) (assign_component_class \"D1\" (condition \"( A.Reference == 'R1' )\" ) )"
        );
        assert_eq!(m.assignment_rules.len(), 2);
        assert_eq!(
            m.constituent_classes["D1"].usage_context,
            ComponentClassUsage::Dynamic
        );

        // S is also used statically, D1 is part of an effective class
        m.get_effective_static_component_class(&names(&["S"]));
        assert_eq!(
            m.constituent_classes["S"].usage_context,
            ComponentClassUsage::StaticAndDynamic
        );
        m.get_dynamic_component_classes_for_footprint(|_| true);
        assert!(m.effective_classes.contains_key("D1,S"));

        // sheet classes only
        let sheets: BTreeSet<String> = ["/New/".to_string()].into_iter().collect();
        assert!(m.sync_dynamic_component_class_assignments(
            &[],
            true,
            &sheets,
            ["", "/", "/A'\"x/", "/New/"]
        ));
        let mut classes: Vec<&str> = m
            .assignment_rules
            .iter()
            .map(|r| r.component_class())
            .collect();
        classes.sort();
        assert_eq!(classes, ["/Ax/", "/New/"]);
        assert!(
            compiler
                .0
                .lock()
                .unwrap()
                .contains(&"(version 2) (assign_component_class \"/Ax/\" (condition \"( A.memberOfSheet('/Ax/') )\" ) )".to_string())
        );
        assert!(!m.constituent_classes.contains_key("D1"));
        assert!(!m.effective_classes.contains_key("D1,S"));
        assert_eq!(
            m.constituent_classes["S"].usage_context,
            ComponentClassUsage::Static
        );

        // one rule fails: nothing installed, the previous classes go
        let ticker = m.ticker();
        assert!(!m.sync_dynamic_component_class_assignments(
            &[assignment("BAD", "R1"), assignment("OK", "R2")],
            false,
            &BTreeSet::new(),
            []
        ));
        assert_eq!(m.ticker(), ticker + 1);
        assert!(m.assignment_rules.is_empty());
        assert!(!m.constituent_classes.contains_key("/Ax/"));
        assert!(!m.constituent_classes.contains_key("OK"));
    }

    fn board_with_footprint() -> (Board, FootprintId) {
        let mut board = Board::default();
        let fp = board.create_footprint();
        board.attach_footprint(fp, crate::board::containers::AddMode::Append);
        (board, fp)
    }

    /// qa/tests/pcbnew/test_component_classes.cpp `FinishNetlistUpdateClearsDeletedClassPointers`
    /// (issue 22623).
    #[test]
    fn finish_netlist_update_clears_deleted_class_pointers() {
        let (mut board, fp) = board_with_footprint();

        // Simulate a previous netlist update that assigned a component class
        board.component_class_manager.init_netlist_update();
        let test_class = board
            .component_class_manager
            .get_effective_static_component_class(&names(&["TEST_CLASS"]));
        board.modify_footprint(fp, |f, _| {
            f.static_component_class = Some(test_class.clone())
        });
        board.finish_component_class_netlist_update();

        // Verify the class exists and footprint has it
        assert!(Arc::ptr_eq(
            board[fp].static_component_class.as_ref().unwrap(),
            &test_class
        ));
        assert!(!test_class.is_empty());

        // Now simulate a NEW netlist update where the class is no longer used
        board.component_class_manager.init_netlist_update();
        board.finish_component_class_netlist_update();

        // Verify the footprint's pointer was cleared since the class was deleted
        assert!(board[fp].static_component_class.is_none());
        board.check_invariants().unwrap();
    }

    /// qa/tests/pcbnew/test_component_classes.cpp
    /// `FinishNetlistUpdatePreservesActiveClassPointers`.
    #[test]
    fn finish_netlist_update_preserves_active_class_pointers() {
        let (mut board, fp) = board_with_footprint();
        let class_names = names(&["TEST_CLASS"]);

        // Begin netlist update and assign a static component class to the footprint
        board.component_class_manager.init_netlist_update();
        let test_class = board
            .component_class_manager
            .get_effective_static_component_class(&class_names);
        board.modify_footprint(fp, |f, _| {
            f.static_component_class = Some(test_class.clone())
        });

        // Simulate the class being used in this netlist update
        let test_class = board
            .component_class_manager
            .get_effective_static_component_class(&class_names);
        board.modify_footprint(fp, |f, _| {
            f.static_component_class = Some(test_class.clone())
        });

        board.finish_component_class_netlist_update();

        // The footprint's pointer should still be valid since the class is still in use
        assert!(Arc::ptr_eq(
            board[fp].static_component_class.as_ref().unwrap(),
            &test_class
        ));
        board.check_invariants().unwrap();
    }
}
