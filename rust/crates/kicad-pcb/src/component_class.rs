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
//! `component_class_assignment_rule.h` and `component_class_manager.h`: data and constructor
//! defaults (package M0d). The behaviour of the `.cpp` files is package M24.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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

    /// `COMPONENT_CLASS::GetHumanReadableName`.
    pub fn get_human_readable_name(&self) -> String {
        crate::port_pending!("COMPONENT_CLASS::GetHumanReadableName")
    }

    /// `COMPONENT_CLASS::ContainsClassName`.
    pub fn contains_class_name(&self, name: &str) -> bool {
        crate::port_pending!("COMPONENT_CLASS::ContainsClassName", name)
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

    /// `COMPONENT_CLASS_MANAGER::GetEffectiveStaticComponentClass`.
    pub fn get_effective_static_component_class(
        &mut self,
        class_names: &BTreeSet<String>,
    ) -> Arc<ComponentClass> {
        crate::port_pending!(
            "COMPONENT_CLASS_MANAGER::GetEffectiveStaticComponentClass",
            class_names
        )
    }

    /// `COMPONENT_CLASS_MANAGER::InitNetlistUpdate`.
    pub fn init_netlist_update(&mut self) {
        crate::port_pending!("COMPONENT_CLASS_MANAGER::InitNetlistUpdate")
    }

    /// `COMPONENT_CLASS_MANAGER::FinishNetlistUpdate`.
    pub fn finish_netlist_update(&mut self) {
        crate::port_pending!("COMPONENT_CLASS_MANAGER::FinishNetlistUpdate")
    }

    /// `COMPONENT_CLASS_MANAGER::RebuildRequiredCaches`.
    pub fn rebuild_required_caches(&mut self) {
        crate::port_pending!("COMPONENT_CLASS_MANAGER::RebuildRequiredCaches")
    }

    /// `COMPONENT_CLASS_MANAGER::InvalidateComponentClasses` (`++m_ticker`).
    pub fn invalidate_component_classes(&mut self) {
        self.ticker += 1;
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
}
