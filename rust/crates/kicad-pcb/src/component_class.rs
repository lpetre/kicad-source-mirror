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
//! (`is_constituent`, name), unique within one manager. The assignment rules need the DRC rule
//! parser and evaluator (kicad-drc, tier 2): matching is passed in by the caller
//! ([`ComponentClassManager::get_dynamic_component_classes_for_footprint`]) and
//! `SyncDynamicComponentClassAssignments` / `CompileAssignmentRule` are stubs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::board::Board;
use crate::ids::FootprintId;

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

/// `COMPONENT_CLASS_ASSIGNMENT_RULE` as data: the class name and the condition text as
/// written; compiled by kicad-drc (custom rule expressions).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ComponentClassAssignmentRule {
    pub component_class: String, // m_componentClass
    pub condition: String,       // m_condition (source text)
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
    /// m_assignmentRules: name + condition text; compiled by kicad-drc.
    pub assignment_rules: Vec<ComponentClassAssignmentRule>,
    pub ticker: i64, // m_ticker
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

    /// `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments`: needs
    /// `CompileAssignmentRule` (the DRC rule parser and `COMPONENT_CLASS_ASSIGNMENT_DATA::
    /// GetAssignmentInDRCLanguage`, kicad-drc / project settings, tier 2).
    pub fn sync_dynamic_component_class_assignments(&mut self) -> bool {
        crate::port_pending!("COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments")
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
                condition: "A".into(),
            },
            ComponentClassAssignmentRule {
                component_class: "S".into(),
                condition: "B".into(),
            },
        ];

        m.init_netlist_update();
        let s = m.get_effective_static_component_class(&names(&["S"]));
        let st = m.get_effective_static_component_class(&names(&["S", "T"]));
        m.finish_netlist_update();
        assert_eq!(m.ticker(), 1);

        // only "DYN" matches
        let dynamic = m.get_dynamic_component_classes_for_footprint(|r| r.condition == "A");
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
        let c = m.get_component_class(&mut cache, Some(&st), |r| r.condition == "A");
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
