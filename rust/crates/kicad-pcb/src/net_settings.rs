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

//! Port of `include/project/net_settings.h` and the non-JSON part of
//! `common/project/net_settings.cpp` (`NET_SETTINGS`): data and constructor defaults
//! (package M0d); the effective-netclass logic is package M4, the JSON loader belongs to the
//! project-settings packages.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::Color4D;
use kicad_core::eda_pattern_match::{CombinedMatcherContext, EdaCombinedMatcher};

use crate::netclass::{NETCLASS_DEFAULT_NAME, Netclass};

/// `NET_CHAIN_SOURCE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NetChainSource {
    Schematic,
    Board,
}

/// A netclass pattern of `m_netClassPatternAssignments`: C++ stores a
/// `unique_ptr<EDA_COMBINED_MATCHER>`. `kicad_core::eda_pattern_match::EdaCombinedMatcher` is
/// neither `Clone` nor `Send + Sync` (its matchers are `Box<dyn EdaPatternMatch>`), which
/// `Board: Clone + Sync` needs; so the pattern text is stored and the matcher
/// (`EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`) is built when it is used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetclassPattern {
    pattern: String,
}

impl NetclassPattern {
    /// `std::make_unique<EDA_COMBINED_MATCHER>( aPattern, CTX_NETCLASS )`.
    pub fn new(pattern: &str) -> Self {
        NetclassPattern {
            pattern: pattern.to_string(),
        }
    }

    /// `EDA_COMBINED_MATCHER::GetPattern`.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The matcher C++ keeps alive in the assignment.
    pub fn matcher(&self) -> EdaCombinedMatcher {
        EdaCombinedMatcher::new(&self.pattern, CombinedMatcherContext::Netclass)
    }
}

/// `NET_SETTINGS` (`include/project/net_settings.h`). Loaded from the `.kicad_pro` JSON.
#[derive(Clone, Debug)]
#[allow(dead_code)] // the derived maps are read by package M4
pub struct NetSettings {
    pub default_netclass: Arc<Netclass>, // m_defaultNetClass
    pub netclasses: BTreeMap<String, Arc<Netclass>>, // m_netClasses
    pub netclass_label_assignments: BTreeMap<String, BTreeSet<String>>, // m_netClassLabelAssignments
    pub netclass_pattern_assignments: Vec<(NetclassPattern, String)>, // m_netClassPatternAssignments
    pub netclass_chain_pattern_assignments:
        BTreeMap<NetChainSource, Vec<(NetclassPattern, String)>>, // m_netClassChainPatternAssignments
    pub net_color_assignments: BTreeMap<String, Color4D>, // m_netColorAssignments
    pub net_chain_classes: BTreeMap<String, String>,      // m_netChainClasses
    pub net_chain_net_classes: BTreeMap<String, String>,  // m_netChainNetClasses
    pub netclass_bus_members: BTreeMap<String, BTreeSet<String>>, // m_netclassBusMembers
    // derived; plain maps because GetEffectiveNetClass is non-const in C++ too (`&mut self`):
    pub(crate) composite_netclasses: BTreeMap<String, Arc<Netclass>>, // m_compositeNetClasses
    pub(crate) implicit_netclasses: BTreeMap<String, Arc<Netclass>>,  // m_impicitNetClasses
    pub(crate) effective_netclass_cache: BTreeMap<String, Arc<Netclass>>, // m_effectiveNetclassCache
}

impl Default for NetSettings {
    /// `NET_SETTINGS::NET_SETTINGS( nullptr, "" )`: a `Default` class with the netclass
    /// defaults, its description and priority `INT_MAX`; everything else empty.
    fn default() -> Self {
        let mut default_netclass = Netclass::new(NETCLASS_DEFAULT_NAME, true);
        default_netclass.description = "This is the default net class.".to_string();
        default_netclass.priority = i32::MAX;

        NetSettings {
            default_netclass: Arc::new(default_netclass),
            netclasses: BTreeMap::new(),
            netclass_label_assignments: BTreeMap::new(),
            netclass_pattern_assignments: Vec::new(),
            netclass_chain_pattern_assignments: BTreeMap::new(),
            net_color_assignments: BTreeMap::new(),
            net_chain_classes: BTreeMap::new(),
            net_chain_net_classes: BTreeMap::new(),
            netclass_bus_members: BTreeMap::new(),
            composite_netclasses: BTreeMap::new(),
            implicit_netclasses: BTreeMap::new(),
            effective_netclass_cache: BTreeMap::new(),
        }
    }
}

impl NetSettings {
    /// `NET_SETTINGS::GetDefaultNetclass`.
    pub fn default_netclass(&self) -> &Arc<Netclass> {
        &self.default_netclass
    }

    /// `NET_SETTINGS::SetDefaultNetclass`.
    pub fn set_default_netclass(&mut self, netclass: Arc<Netclass>) {
        self.default_netclass = netclass;
    }

    /// `NET_SETTINGS::GetEffectiveNetClass`: explicit labels, then patterns in file order,
    /// then chain classes; several matches build a cached composite class. Non-const in C++
    /// too (fills the composite / implicit / effective caches).
    pub fn get_effective_netclass(&mut self, net_name: &str) -> Arc<Netclass> {
        crate::port_pending!("NET_SETTINGS::GetEffectiveNetClass", net_name)
    }

    /// `NET_SETTINGS::GetCachedEffectiveNetClass`.
    pub fn get_cached_effective_netclass(&self, net_name: &str) -> Option<Arc<Netclass>> {
        crate::port_pending!("NET_SETTINGS::GetCachedEffectiveNetClass", net_name)
    }

    /// `NET_SETTINGS::HasEffectiveNetClass`.
    pub fn has_effective_netclass(&self, net_name: &str) -> bool {
        crate::port_pending!("NET_SETTINGS::HasEffectiveNetClass", net_name)
    }

    /// `NET_SETTINGS::GetNetClassByName`.
    pub fn get_net_class_by_name(&self, name: &str) -> Option<Arc<Netclass>> {
        crate::port_pending!("NET_SETTINGS::GetNetClassByName", name)
    }

    /// `NET_SETTINGS::ClearAllCaches`: the effective cache, the composites and the bus
    /// members (not the implicit classes).
    pub fn clear_all_caches(&mut self) {
        crate::port_pending!("NET_SETTINGS::ClearAllCaches")
    }

    /// `NET_SETTINGS::ClearCacheForNet`.
    pub fn clear_cache_for_net(&mut self, net_name: &str) {
        crate::port_pending!("NET_SETTINGS::ClearCacheForNet", net_name)
    }

    /// `NET_SETTINGS::SetNetclassPatternAssignment`.
    pub fn set_netclass_pattern_assignment(&mut self, pattern: &str, netclass: &str) {
        crate::port_pending!(
            "NET_SETTINGS::SetNetclassPatternAssignment",
            pattern,
            netclass
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NET_SETTINGS::NET_SETTINGS` (`common/project/net_settings.cpp`).
    #[test]
    fn constructor_defaults() {
        let ns = NetSettings::default();
        assert_eq!(ns.default_netclass.name, "Default");
        assert!(ns.default_netclass.is_default);
        assert_eq!(ns.default_netclass.priority, i32::MAX);
        assert_eq!(ns.default_netclass.clearance, Some(200_000));
        assert_eq!(
            ns.default_netclass.description,
            "This is the default net class."
        );
        assert!(ns.netclasses.is_empty());
        assert!(ns.netclass_pattern_assignments.is_empty());
        assert_eq!(NetclassPattern::new("HV*").pattern(), "HV*");
    }
}
