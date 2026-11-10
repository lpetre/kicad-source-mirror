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
//! (package M0d), the netclass containers, label / pattern / chain-pattern / colour
//! assignments, the effective-netclass resolution with its caches and the bus-notation
//! helpers (package M4). The JSON parameters, migrations and `CopyFrom` (which round-trips
//! through the JSON) belong to the project-settings packages. Pattern assignments hold compiled
//! matchers (`kicad_core::eda_pattern_match::EdaCombinedMatcher` is `Clone + Send + Sync`), as
//! in C++.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::Color4D;
use kicad_core::eda_pattern_match::{CombinedMatcherContext, EdaCombinedMatcher};
use kicad_core::string_utils::{EscapeContext, escape_string};
use kicad_geom::stdsort::std_sort;

use crate::netclass::{NETCLASS_DEFAULT_NAME, Netclass};

/// `NET_CHAIN_SOURCE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NetChainSource {
    Schematic,
    Board,
}

/// One entry of `m_netClassPatternAssignments` / `m_netClassChainPatternAssignments`:
/// `std::pair<std::unique_ptr<EDA_COMBINED_MATCHER>, wxString>`, the matcher compiled once
/// with `CTX_NETCLASS` when the assignment is added, and the netclass name.
pub type NetclassPatternAssignment = (EdaCombinedMatcher, String);

/// `std::make_unique<EDA_COMBINED_MATCHER>( aPattern, CTX_NETCLASS )`: the compiled matcher
/// of a netclass pattern (anchored regular expression and anchored wildcard).
pub fn netclass_matcher(pattern: &str) -> EdaCombinedMatcher {
    EdaCombinedMatcher::new(pattern, CombinedMatcherContext::Netclass)
}

/// `NET_SETTINGS` (`include/project/net_settings.h`). Loaded from the `.kicad_pro` JSON.
#[derive(Clone, Debug)]
pub struct NetSettings {
    pub default_netclass: Arc<Netclass>, // m_defaultNetClass
    pub netclasses: BTreeMap<String, Arc<Netclass>>, // m_netClasses
    pub netclass_label_assignments: BTreeMap<String, BTreeSet<String>>, // m_netClassLabelAssignments
    /// m_netClassPatternAssignments: compiled matchers, in file order.
    pub netclass_pattern_assignments: Vec<NetclassPatternAssignment>,
    /// m_netClassChainPatternAssignments (derived from `net_chain_net_classes`, not
    /// serialised).
    pub netclass_chain_pattern_assignments:
        BTreeMap<NetChainSource, Vec<NetclassPatternAssignment>>,
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

impl PartialEq for NetSettings {
    /// Port of `NET_SETTINGS::operator==`: compares the persisted state by content.
    fn eq(&self, other: &Self) -> bool {
        // m_netClasses maps name -> shared_ptr<NETCLASS>.  The default pair operator==
        // would compare the shared_ptrs by pointer identity, missing in-place edits
        // to the underlying NETCLASS (the UI's normal edit path mutates through the
        // shared_ptr rather than swapping it).  Compare names and contents instead.
        let netclass_entry_equal =
            |lhs: (&String, &Arc<Netclass>), rhs: (&String, &Arc<Netclass>)| {
                if lhs.0 != rhs.0 {
                    return false;
                }

                if Arc::ptr_eq(lhs.1, rhs.1) {
                    return true;
                }

                lhs.1.equals_by_persisted_fields(rhs.1)
            };

        if self.netclasses.len() != other.netclasses.len()
            || !self
                .netclasses
                .iter()
                .zip(&other.netclasses)
                .all(|(l, r)| netclass_entry_equal(l, r))
        {
            return false;
        }

        // m_defaultNetClass is held by shared_ptr and (per board.cpp / kicad_sexpr_parser.cpp
        // setup paths) is mutated in place via GetDefaultNetclass()->SetClearance( ... ).
        // shared_ptr identity is preserved across those edits, so a pointer-only check
        // would silently drop the most common project edit.  Compare contents.
        if !Arc::ptr_eq(&self.default_netclass, &other.default_netclass)
            && !self
                .default_netclass
                .equals_by_persisted_fields(&other.default_netclass)
        {
            return false;
        }

        // m_netClassPatternAssignments stores std::unique_ptr<EDA_COMBINED_MATCHER>, so a naive
        // std::equal would compare matcher pointer identity and report two settings instances
        // built from identical input as unequal.  Compare pattern text plus the assigned
        // netclass name.
        if self.netclass_pattern_assignments.len() != other.netclass_pattern_assignments.len()
            || !self
                .netclass_pattern_assignments
                .iter()
                .zip(&other.netclass_pattern_assignments)
                .all(|(l, r)| l.0.pattern() == r.0.pattern() && l.1 == r.1)
        {
            return false;
        }

        // m_netClassChainPatternAssignments is derived state, rebuilt from m_netChainNetClasses
        // and the current chain membership.  Equality is defined by persisted inputs only;
        // including the derived list here would mark the project dirty whenever a rebuild
        // produced a transient ordering difference.

        self.netclass_label_assignments == other.netclass_label_assignments
            && self.net_color_assignments == other.net_color_assignments
            && self.net_chain_classes == other.net_chain_classes
            && self.net_chain_net_classes == other.net_chain_net_classes
    }
}

impl NetSettings {
    /// `NET_SETTINGS::GetDefaultNetclass`.
    pub fn default_netclass(&self) -> &Arc<Netclass> {
        &self.default_netclass
    }

    /// `NET_SETTINGS::SetDefaultNetclass`. The caller is responsible for resetting the
    /// effective netclass caches.
    pub fn set_default_netclass(&mut self, netclass: Arc<Netclass>) {
        self.default_netclass = netclass;
    }

    /// `NET_SETTINGS::HasNetclass`.
    pub fn has_netclass(&self, netclass_name: &str) -> bool {
        self.netclasses.contains_key(netclass_name)
    }

    /// `NET_SETTINGS::SetNetclass`. The caller is responsible for resetting the effective
    /// netclass caches.
    pub fn set_netclass(&mut self, netclass_name: &str, netclass: Arc<Netclass>) {
        self.netclasses.insert(netclass_name.to_string(), netclass);
    }

    /// `NET_SETTINGS::SetNetclasses`: replaces all netclasses, then `ClearAllCaches`.
    pub fn set_netclasses(&mut self, netclasses: BTreeMap<String, Arc<Netclass>>) {
        self.netclasses = netclasses;
        self.clear_all_caches();
    }

    /// `NET_SETTINGS::GetNetclasses`.
    pub fn netclasses(&self) -> &BTreeMap<String, Arc<Netclass>> {
        &self.netclasses
    }

    /// `NET_SETTINGS::GetCompositeNetclasses` (keyed by the composite's `GetName()` at
    /// creation).
    pub fn composite_netclasses(&self) -> &BTreeMap<String, Arc<Netclass>> {
        &self.composite_netclasses
    }

    /// `NET_SETTINGS::ClearNetclasses`: the netclasses and the implicit classes, then
    /// `ClearAllCaches`.
    pub fn clear_netclasses(&mut self) {
        self.netclasses.clear();
        self.implicit_netclasses.clear();
        self.clear_all_caches();
    }

    /// `NET_SETTINGS::GetNetclassLabelAssignments`.
    pub fn netclass_label_assignments(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.netclass_label_assignments
    }

    /// `NET_SETTINGS::ClearNetclassLabelAssignments` (does not clear the caches, as in C++).
    pub fn clear_netclass_label_assignments(&mut self) {
        self.netclass_label_assignments.clear();
    }

    /// `NET_SETTINGS::ClearNetclassLabelAssignment` (does not clear the caches).
    pub fn clear_netclass_label_assignment(&mut self, net_name: &str) {
        self.netclass_label_assignments.remove(net_name);
    }

    /// `NET_SETTINGS::SetNetclassLabelAssignment` (does not clear the caches).
    pub fn set_netclass_label_assignment(&mut self, net_name: &str, netclasses: BTreeSet<String>) {
        self.netclass_label_assignments
            .insert(net_name.to_string(), netclasses);
    }

    /// `NET_SETTINGS::AppendNetclassLabelAssignment` (does not clear the caches).
    pub fn append_netclass_label_assignment(
        &mut self,
        net_name: &str,
        netclasses: &BTreeSet<String>,
    ) {
        self.netclass_label_assignments
            .entry(net_name.to_string())
            .or_default()
            .extend(netclasses.iter().cloned());
    }

    /// `NET_SETTINGS::HasNetclassLabelAssignment`.
    pub fn has_netclass_label_assignment(&self, net_name: &str) -> bool {
        self.netclass_label_assignments.contains_key(net_name)
    }

    /// `NET_SETTINGS::HasEffectiveNetClass`.
    pub fn has_effective_netclass(&self, net_name: &str) -> bool {
        self.effective_netclass_cache.contains_key(net_name)
    }

    /// `NET_SETTINGS::GetCachedEffectiveNetClass`. C++ uses `std::map::at`, which throws
    /// for a net that is not cached; that is `None` here.
    pub fn get_cached_effective_netclass(&self, net_name: &str) -> Option<Arc<Netclass>> {
        self.effective_netclass_cache.get(net_name).cloned()
    }

    /// `NET_SETTINGS::GetNetClassByName`: the named class, else the default class (never
    /// `None`; the `Option` is the frozen signature).
    pub fn get_net_class_by_name(&self, name: &str) -> Option<Arc<Netclass>> {
        match self.netclasses.get(name) {
            None => Some(self.default_netclass.clone()),
            Some(netclass) => Some(netclass.clone()),
        }
    }

    /// `NET_SETTINGS::ClearAllCaches`: the effective cache, the composites and the bus
    /// members (not the implicit classes).
    pub fn clear_all_caches(&mut self) {
        self.effective_netclass_cache.clear();
        self.composite_netclasses.clear();
        self.netclass_bus_members.clear();
    }

    /// Port of `NET_SETTINGS::ClearCacheForNet`: drops the net's cache entry (and the
    /// composite of that name), then, transitively, the bus nets whose cached class was
    /// derived from it.
    pub fn clear_cache_for_net(&mut self, net_name: &str) {
        let mut pending: BTreeSet<String> = BTreeSet::from([net_name.to_string()]);

        while let Some(name) = pending.pop_first() {
            if let Some(cached) = self.effective_netclass_cache.remove(&name) {
                self.composite_netclasses.remove(&cached.get_name());
            }

            self.netclass_bus_members.remove(&name);

            for (bus, members) in &self.netclass_bus_members {
                if members.contains(&name) {
                    pending.insert(bus.clone());
                }
            }
        }
    }

    /// Port of `NET_SETTINGS::SetNetclassPatternAssignment`: expands bus patterns into
    /// `add_single_pattern_assignment` calls, then `ClearAllCaches`.
    pub fn set_netclass_pattern_assignment(&mut self, pattern: &str, netclass: &str) {
        // Expand bus patterns (vector buses and bus groups) to individual member patterns.
        // This is necessary because the regex/wildcard matchers interpret brackets and braces
        // as special characters, not as bus notation.
        Self::for_each_bus_member(pattern, &mut |member_pattern| {
            self.add_single_pattern_assignment(member_pattern, netclass);
        });

        self.clear_all_caches();
    }

    /// `NET_SETTINGS::addSinglePatternAssignment`: appends the compiled matcher unless the
    /// same pattern is already assigned to the same class.
    pub fn add_single_pattern_assignment(&mut self, pattern: &str, netclass: &str) {
        // Avoid exact duplicates - these shouldn't cause problems, due to later de-duplication
        // but they are unnecessary.
        for assignment in &self.netclass_pattern_assignments {
            if assignment.0.pattern() == pattern && assignment.1 == netclass {
                return;
            }
        }

        // No assignment, add a new one
        self.netclass_pattern_assignments
            .push((netclass_matcher(pattern), netclass.to_string()));
    }

    /// `NET_SETTINGS::SetNetclassPatternAssignments( && )`.
    pub fn set_netclass_pattern_assignments(&mut self, patterns: Vec<NetclassPatternAssignment>) {
        self.netclass_pattern_assignments = patterns;
        self.clear_all_caches();
    }

    /// `NET_SETTINGS::GetNetclassPatternAssignments`.
    pub fn netclass_pattern_assignments(&self) -> &[NetclassPatternAssignment] {
        &self.netclass_pattern_assignments
    }

    /// `NET_SETTINGS::ClearNetclassPatternAssignments` (does not clear the caches, as in
    /// C++).
    pub fn clear_netclass_pattern_assignments(&mut self) {
        self.netclass_pattern_assignments.clear();
    }

    /// Port of `NET_SETTINGS::SetChainPatternAssignment`: a chain-derived pattern owned by
    /// `source`, bus-expanded like user patterns, then `ClearAllCaches`.
    pub fn set_chain_pattern_assignment(
        &mut self,
        source: NetChainSource,
        pattern: &str,
        netclass: &str,
    ) {
        Self::for_each_bus_member(pattern, &mut |member_pattern| {
            self.add_single_chain_pattern_assignment(source, member_pattern, netclass);
        });

        self.clear_all_caches();
    }

    /// `NET_SETTINGS::addSingleChainPatternAssignment`.
    pub fn add_single_chain_pattern_assignment(
        &mut self,
        source: NetChainSource,
        pattern: &str,
        netclass: &str,
    ) {
        let assignments = self
            .netclass_chain_pattern_assignments
            .entry(source)
            .or_default();

        for assignment in assignments.iter() {
            if assignment.0.pattern() == pattern && assignment.1 == netclass {
                return;
            }
        }

        assignments.push((netclass_matcher(pattern), netclass.to_string()));
    }

    /// `NET_SETTINGS::ClearChainPatternAssignments`: clears `source`'s entries only (the map
    /// entry is created, as `operator[]` does), then `ClearAllCaches`.
    pub fn clear_chain_pattern_assignments(&mut self, source: NetChainSource) {
        self.netclass_chain_pattern_assignments
            .entry(source)
            .or_default()
            .clear();
        self.clear_all_caches();
    }

    /// `NET_SETTINGS::HasChainPatternAssignments` (inline in `net_settings.h`).
    pub fn has_chain_pattern_assignments(&self, source: NetChainSource) -> bool {
        self.netclass_chain_pattern_assignments
            .get(&source)
            .is_some_and(|v| !v.is_empty())
    }

    /// `NET_SETTINGS::GetChainPatternAssignments` (inline in `net_settings.h`).
    pub fn chain_pattern_assignments(
        &self,
    ) -> &BTreeMap<NetChainSource, Vec<NetclassPatternAssignment>> {
        &self.netclass_chain_pattern_assignments
    }

    /// `NET_SETTINGS::SetNetColorAssignment`.
    pub fn set_net_color_assignment(&mut self, net_name: &str, color: Color4D) {
        self.net_color_assignments
            .insert(net_name.to_string(), color);
    }

    /// `NET_SETTINGS::GetNetColorAssignments`.
    pub fn net_color_assignments(&self) -> &BTreeMap<String, Color4D> {
        &self.net_color_assignments
    }

    /// `NET_SETTINGS::ClearNetColorAssignments`.
    pub fn clear_net_color_assignments(&mut self) {
        self.net_color_assignments.clear();
    }

    /// Port of `NET_SETTINGS::RenameNetPathPrefix`: retargets netclass patterns and net colours
    /// after a path prefix changes (sheet rename). Returns true and clears the caches if
    /// anything changed.
    pub fn rename_net_path_prefix(&mut self, old_prefix: &str, new_prefix: &str) -> bool {
        if old_prefix.is_empty() || old_prefix == new_prefix {
            return false;
        }

        let mut changed = false;

        // Patterns hold the path as plain text, so swap the leading prefix and rebuild the
        // matcher.
        for (matcher, _netclass) in &mut self.netclass_pattern_assignments {
            if let Some(rest) = matcher.pattern().strip_prefix(old_prefix) {
                let updated = format!("{new_prefix}{rest}");
                *matcher = netclass_matcher(&updated);
                changed = true;
            }
        }

        // Net color keys are full net names, which carry the path too.
        let mut updated_colors: BTreeMap<String, Color4D> = BTreeMap::new();

        for (net_name, color) in &self.net_color_assignments {
            if let Some(rest) = net_name.strip_prefix(old_prefix) {
                updated_colors.insert(format!("{new_prefix}{rest}"), color.clone());
                changed = true;
            } else {
                updated_colors.insert(net_name.clone(), color.clone());
            }
        }

        if changed {
            self.net_color_assignments = updated_colors;
            self.clear_all_caches();
        }

        changed
    }

    /// Port of `NET_SETTINGS::RenameNets`: retargets exact-net netclass patterns and net
    /// colours after nets are renamed. Returns true and clears the caches if anything changed.
    pub fn rename_nets(&mut self, new_names: &BTreeMap<String, String>) -> bool {
        if new_names.is_empty() {
            return false;
        }

        let mut changed = false;

        // Only an exact-net pattern names one net; a wildcard may still match after the rename.
        for (matcher, _netclass) in &mut self.netclass_pattern_assignments {
            if let Some((from, to)) = new_names.get_key_value(matcher.pattern())
                && to != from
            {
                *matcher = netclass_matcher(to);
                changed = true;
            }
        }

        let mut updated_colors: BTreeMap<String, Color4D> = BTreeMap::new();

        for (net_name, color) in &self.net_color_assignments {
            match new_names.get(net_name) {
                Some(renamed) if renamed != net_name => {
                    updated_colors.insert(renamed.clone(), color.clone());
                    changed = true;
                }
                _ => {
                    updated_colors.insert(net_name.clone(), color.clone());
                }
            }
        }

        if changed {
            self.net_color_assignments = updated_colors;
            self.clear_all_caches();
        }

        changed
    }

    /// `NET_SETTINGS::SetNetChainClass` (inline): an empty class removes the entry.
    pub fn set_net_chain_class(&mut self, chain: &str, class: &str) {
        if class.is_empty() {
            self.net_chain_classes.remove(chain);
        } else {
            self.net_chain_classes
                .insert(chain.to_string(), class.to_string());
        }
    }

    /// `NET_SETTINGS::GetNetChainClass` (inline): empty means "no class".
    pub fn get_net_chain_class(&self, chain: &str) -> String {
        self.net_chain_classes
            .get(chain)
            .cloned()
            .unwrap_or_default()
    }

    /// `NET_SETTINGS::GetNetChainClasses` (inline).
    pub fn net_chain_classes(&self) -> &BTreeMap<String, String> {
        &self.net_chain_classes
    }

    /// `NET_SETTINGS::ClearNetChainClasses` (inline).
    pub fn clear_net_chain_classes(&mut self) {
        self.net_chain_classes.clear();
    }

    /// `NET_SETTINGS::SetNetChainNetClass` (inline): an empty netclass removes the entry.
    pub fn set_net_chain_net_class(&mut self, chain: &str, netclass: &str) {
        if netclass.is_empty() {
            self.net_chain_net_classes.remove(chain);
        } else {
            self.net_chain_net_classes
                .insert(chain.to_string(), netclass.to_string());
        }
    }

    /// `NET_SETTINGS::GetNetChainNetClass` (inline): empty means "none".
    pub fn get_net_chain_net_class(&self, chain: &str) -> String {
        self.net_chain_net_classes
            .get(chain)
            .cloned()
            .unwrap_or_default()
    }

    /// `NET_SETTINGS::GetNetChainNetClasses` (inline).
    pub fn net_chain_net_classes(&self) -> &BTreeMap<String, String> {
        &self.net_chain_net_classes
    }

    /// `NET_SETTINGS::ClearNetChainNetClasses` (inline).
    pub fn clear_net_chain_net_classes(&mut self) {
        self.net_chain_net_classes.clear();
    }

    /// Port of `NET_SETTINGS::GetEffectiveNetClass`: explicit labels, then patterns in file
    /// order, then chain classes; several matches (or missing fields) build a cached composite
    /// class. Non-const in C++ too (fills the composite / implicit / effective caches).
    pub fn get_effective_netclass(&mut self, net_name: &str) -> Arc<Netclass> {
        // <no net> is forced to be part of the default netclass.
        if net_name.is_empty() {
            return self.default_netclass.clone();
        }

        // First check if we have a cached resolved netclass
        if let Some(cached) = self.effective_netclass_cache.get(net_name) {
            return cached.clone();
        }

        // No cache found - build a vector of all netclasses assigned to or matching this net.
        // C++ collects them in a std::unordered_set of shared_ptrs (de-duplicated by
        // identity, iterated in hash order); they are sorted by priority and name below, so
        // the first-insertion order kept here only matters for exact ties (PORT_STATUS.md).
        let mut resolved_netclasses: Vec<Arc<Netclass>> = Vec::new();

        let insert = |set: &mut Vec<Arc<Netclass>>, netclass: Arc<Netclass>| {
            if !set.iter().any(|nc| Arc::ptr_eq(nc, &netclass)) {
                set.push(netclass);
            }
        };

        // First find explicit netclass assignments
        let label_classes: Vec<String> = self
            .netclass_label_assignments
            .get(net_name)
            .map(|classes| classes.iter().cloned().collect())
            .unwrap_or_default();

        for netclass_name in &label_classes {
            let netclass = match self.get_explicit_netclass(netclass_name) {
                Some(netclass) => netclass,
                None => self.get_or_add_implicit_netclass(netclass_name),
            };

            insert(&mut resolved_netclasses, netclass);
        }

        // Now find any pattern-matched netclass assignments (user + chain-derived)
        let mut matched_names: Vec<String> = Vec::new();

        for (matcher, netclass_name) in &self.netclass_pattern_assignments {
            if matcher.starts_with(net_name) {
                matched_names.push(netclass_name.clone());
            }
        }

        for chain_patterns in self.netclass_chain_pattern_assignments.values() {
            for (matcher, netclass_name) in chain_patterns {
                if matcher.starts_with(net_name) {
                    matched_names.push(netclass_name.clone());
                }
            }
        }

        for netclass_name in &matched_names {
            let netclass = match self.get_explicit_netclass(netclass_name) {
                Some(netclass) => netclass,
                None => self.get_or_add_implicit_netclass(netclass_name),
            };

            insert(&mut resolved_netclasses, netclass);
        }

        // Handle zero resolved netclasses
        if resolved_netclasses.is_empty() {
            // For bus patterns, check if all members share the same netclass.
            // If they do, the bus inherits that netclass for coloring purposes.
            let mut shared_netclass: Option<Arc<Netclass>> = None;
            let mut all_same_netclass = true;
            let mut is_bus_pattern = false;

            Self::for_each_bus_member(net_name, &mut |member| {
                // If ForEachBusMember gives us back the same name, it's not a bus.
                // Skip to avoid infinite recursion.
                if member == net_name {
                    return;
                }

                is_bus_pattern = true;

                if !all_same_netclass {
                    return;
                }

                // The first disagreeing pair suffices: later members cannot change
                // the result while that pair remains unequal.
                self.netclass_bus_members
                    .entry(net_name.to_string())
                    .or_default()
                    .insert(member.to_string());
                let member_nc = self.get_effective_netclass(member);

                match &shared_netclass {
                    None => shared_netclass = Some(member_nc),
                    Some(shared) => {
                        if member_nc.get_name() != shared.get_name() {
                            all_same_netclass = false;
                        }
                    }
                }
            });

            if let Some(shared) = shared_netclass
                && is_bus_pattern
                && all_same_netclass
                && shared.get_name() != NETCLASS_DEFAULT_NAME
            {
                self.effective_netclass_cache
                    .insert(net_name.to_string(), shared.clone());
                return shared;
            }

            self.effective_netclass_cache
                .insert(net_name.to_string(), self.default_netclass.clone());

            return self.default_netclass.clone();
        }

        // Make and cache the effective netclass. Note that makeEffectiveNetclass will add the
        // default netclass to resolvedNetclasses if it is needed to complete the netclass
        // paramters set. It will also sort resolvedNetclasses by priority order.
        let mut netclass_ptrs: Vec<Arc<Netclass>> = resolved_netclasses.clone();

        let name = format!("Effective for net: {net_name}");
        let mut effective_netclass = Netclass::new(&name, false);
        self.make_effective_netclass(&mut effective_netclass, &mut netclass_ptrs);

        if netclass_ptrs.len() == 1 {
            // No defaults were added - just return the primary netclass
            let primary = resolved_netclasses[0].clone();
            self.effective_netclass_cache
                .insert(net_name.to_string(), primary.clone());
            primary
        } else {
            effective_netclass.set_constituent_netclasses(netclass_ptrs);
            let effective_netclass = Arc::new(effective_netclass);

            self.composite_netclasses
                .insert(effective_netclass.get_name(), effective_netclass.clone());
            self.effective_netclass_cache
                .insert(net_name.to_string(), effective_netclass.clone());

            effective_netclass
        }
    }

    /// The `getExplicitNetclass` lambda of `GetEffectiveNetClass`: the default class for the
    /// name `Default`, else the configured class, else `None`.
    fn get_explicit_netclass(&self, netclass: &str) -> Option<Arc<Netclass>> {
        if netclass == NETCLASS_DEFAULT_NAME {
            return Some(self.default_netclass.clone());
        }

        self.netclasses.get(netclass).cloned()
    }

    /// The `getOrAddImplicitNetcless` lambda of `GetEffectiveNetClass`: fetches or creates
    /// an implicit netclass (defined with a label, but not configured). These are needed as
    /// while they do not provide any netclass parameters, they do now appear in DRC matching
    /// strings as an assigned netclass.
    fn get_or_add_implicit_netclass(&mut self, netclass: &str) -> Arc<Netclass> {
        if let Some(existing) = self.implicit_netclasses.get(netclass) {
            return existing.clone();
        }

        let mut nc = Netclass::new(netclass, false);
        nc.priority = i32::MAX - 1; // Priority > default netclass
        let nc = Arc::new(nc);
        self.implicit_netclasses
            .insert(netclass.to_string(), nc.clone());
        nc
    }

    /// Port of `NET_SETTINGS::RecomputeEffectiveNetclasses`: rebuilds every composite from its
    /// constituents (dropping a trailing `Default`, which is re-added if still needed).
    ///
    /// C++ rebuilds each composite in place, so every holder of the `shared_ptr` sees the new
    /// values. A shared `Arc<Netclass>` cannot be mutated, so the rebuilt class is a new `Arc`
    /// that replaces the old one in the composite map and in every effective-cache entry that
    /// held it; other holders (`NetinfoItem::netclass`) keep the old values until the next
    /// `synchronize_nets_and_netclasses` (PORT_STATUS.md).
    pub fn recompute_effective_netclasses(&mut self) {
        let names: Vec<String> = self.composite_netclasses.keys().cloned().collect();

        for nc_name in names {
            let old = self.composite_netclasses[&nc_name].clone();

            // Note this needs to be a copy in case we now need to add the default netclass
            let mut constituents: Vec<Arc<Netclass>> = old.constituents.clone();

            debug_assert!(!constituents.is_empty());

            // If the last netclass is Default, remove it (it will be re-added if still needed)
            if constituents
                .last()
                .is_some_and(|last| last.get_name() == NETCLASS_DEFAULT_NAME)
            {
                constituents.pop();
            }

            // Remake the netclass from original constituents
            let mut nc = (*old).clone();
            nc.reset_parameters();
            self.make_effective_netclass(&mut nc, &mut constituents);
            nc.set_constituent_netclasses(constituents);
            let nc = Arc::new(nc);

            for cached in self.effective_netclass_cache.values_mut() {
                if Arc::ptr_eq(cached, &old) {
                    *cached = nc.clone();
                }
            }

            self.composite_netclasses.insert(nc_name, nc);
        }
    }

    /// Port of `NET_SETTINGS::makeEffectiveNetclass`: sorts the constituents by priority
    /// (then name) and composes the parameters from the lowest priority up; missing fields
    /// are filled from the default netclass, which is then appended to the constituents.
    fn make_effective_netclass(
        &self,
        effective_netclass: &mut Netclass,
        constituent_netclasses: &mut Vec<Arc<Netclass>>,
    ) {
        // Sort the resolved netclasses by priority (highest first), with same-priority
        // netclasses ordered alphabetically
        std_sort(constituent_netclasses, |nc1, nc2| {
            let p1 = nc1.priority();
            let p2 = nc2.priority();

            if p1 < p2 {
                return true;
            }

            if p1 == p2 {
                return nc1.get_name() < nc2.get_name();
            }

            false
        });

        // Iterate from lowest priority netclass and fill effective netclass parameters
        for itr in constituent_netclasses.iter().rev() {
            let nc: &Netclass = itr;
            let parent = || Some(itr.clone());
            let e = &mut *effective_netclass;

            if nc.has_clearance() {
                e.clearance = Some(nc.clearance());
                e.parents.clearance = parent();
            }

            if nc.has_track_width() {
                e.track_width = Some(nc.track_width());
                e.parents.track_width = parent();
            }

            if nc.has_via_diameter() {
                e.via_dia = Some(nc.via_diameter());
                e.parents.via_diameter = parent();
            }

            if nc.has_via_drill() {
                e.via_drill = Some(nc.via_drill());
                e.parents.via_drill = parent();
            }

            if nc.has_uvia_diameter() {
                e.uvia_dia = Some(nc.uvia_diameter());
                e.parents.uvia_dia = parent();
            }

            if nc.has_uvia_drill() {
                e.uvia_drill = Some(nc.uvia_drill());
                e.parents.uvia_drill = parent();
            }

            if nc.has_diff_pair_width() {
                e.diff_pair_width = Some(nc.diff_pair_width());
                e.parents.diff_pair_width = parent();
            }

            if nc.has_diff_pair_gap() {
                e.diff_pair_gap = Some(nc.diff_pair_gap());
                e.parents.diff_pair_gap = parent();
            }

            if nc.has_diff_pair_via_gap() {
                e.diff_pair_via_gap = Some(nc.diff_pair_via_gap());
                e.parents.diff_pair_via_gap = parent();
            }

            if nc.has_wire_width() {
                e.wire_width = Some(nc.wire_width());
                e.parents.wire_width = parent();
            }

            if nc.has_bus_width() {
                e.bus_width = Some(nc.bus_width());
                e.parents.bus_width = parent();
            }

            if nc.has_line_style() {
                e.line_style = Some(nc.line_style());
                e.parents.line_style = parent();
            }

            // GetPcbColor() / GetSchematicColor(): UNSPECIFIED for the default class.
            let pcb_color = if nc.is_default() {
                Color4D::UNSPECIFIED
            } else {
                nc.pcb_color.clone()
            };

            if pcb_color != Color4D::UNSPECIFIED {
                e.pcb_color = pcb_color;
                e.parents.pcb_color = parent();
            }

            let sch_color = if nc.is_default() {
                Color4D::UNSPECIFIED
            } else {
                nc.schematic_color.clone()
            };

            if sch_color != Color4D::UNSPECIFIED {
                e.schematic_color = sch_color;
                e.parents.schematic_color = parent();
            }

            if nc.has_tuning_profile() {
                e.tuning_profile = nc.tuning_profile().to_string();
                e.parents.tuning_profile = parent();
            }
        }

        // Fill in any required defaults
        if self.add_missing_defaults(effective_netclass) {
            constituent_netclasses.push(self.default_netclass.clone());
        }
    }

    /// Port of `NET_SETTINGS::addMissingDefaults`: fills the missing fields of `nc` from the
    /// default netclass; true if any field was filled.
    fn add_missing_defaults(&self, nc: &mut Netclass) -> bool {
        let mut added_default = false;
        let d = &self.default_netclass;
        let parent = || Some(d.clone());

        if !nc.has_clearance() {
            added_default = true;
            nc.clearance = Some(d.clearance());
            nc.parents.clearance = parent();
        }

        if !nc.has_track_width() {
            added_default = true;
            nc.track_width = Some(d.track_width());
            nc.parents.track_width = parent();
        }

        if !nc.has_via_diameter() {
            added_default = true;
            nc.via_dia = Some(d.via_diameter());
            nc.parents.via_diameter = parent();
        }

        if !nc.has_via_drill() {
            added_default = true;
            nc.via_drill = Some(d.via_drill());
            nc.parents.via_drill = parent();
        }

        if !nc.has_uvia_diameter() {
            added_default = true;
            nc.uvia_dia = Some(d.uvia_diameter());
            nc.parents.uvia_dia = parent();
        }

        if !nc.has_uvia_drill() {
            added_default = true;
            nc.uvia_drill = Some(d.uvia_drill());
            nc.parents.uvia_drill = parent();
        }

        if !nc.has_diff_pair_width() {
            added_default = true;
            nc.diff_pair_width = Some(d.diff_pair_width());
            nc.parents.diff_pair_width = parent();
        }

        if !nc.has_diff_pair_gap() {
            added_default = true;
            nc.diff_pair_gap = Some(d.diff_pair_gap());
            nc.parents.diff_pair_gap = parent();
        }

        // Currently this is only on the default netclass, and not editable in the setup panel
        // if( !nc->HasDiffPairViaGap() ) { ... }

        if !nc.has_wire_width() {
            added_default = true;
            nc.wire_width = Some(d.wire_width());
            nc.parents.wire_width = parent();
        }

        if !nc.has_bus_width() {
            added_default = true;
            nc.bus_width = Some(d.bus_width());
            nc.parents.bus_width = parent();
        }

        // The tuning profile can be empty - only fill if a default tuning profile is set
        if !nc.has_tuning_profile() && d.has_tuning_profile() {
            added_default = true;
            nc.tuning_profile = d.tuning_profile().to_string();
            nc.parents.tuning_profile = parent();
        }

        added_default
    }

    /// Port of `NET_SETTINGS::ParseBusVector`: parses a bus vector (e.g. `A[7..0]`) into the
    /// name (`A`) and the member list (`A0` ... `A7`). The out-parameters are written exactly
    /// when C++ writes them (both only on success).
    pub fn parse_bus_vector(
        bus: &str,
        name: Option<&mut String>,
        member_list: Option<&mut Vec<String>>,
    ) -> bool {
        let is_digit = |c: char| c.is_ascii_digit();

        let b: Vec<char> = bus.chars().collect();
        let bus_len = b.len();
        let mut i = 0usize;
        let mut prefix: Vec<char> = Vec::with_capacity(bus_len);
        let mut suffix = String::new();
        let mut tmp = String::new();
        let mut begin: i64 = 0;
        let mut end: i64 = 0;
        let mut brace_nesting: i32 = 0;
        let mut fmt_wraps_name = false;
        let mut in_quotes = false;
        let mut parsed_end = false;
        let mut padded = false;
        let mut width = 0usize;

        // Parse prefix
        //
        // Formatting markers (^{}, _{}, ~{}) can appear either as part of the prefix name
        // (e.g. I^{2}C[0..7]) or wrapping the range specifier (e.g. D_{[1..2]}).
        // We preserve formatting in the prefix and only strip it when the range bracket
        // appears inside formatting braces, indicating the formatting wraps the range.
        while i < bus_len {
            // Handle quoted strings (allows spaces inside)
            if b[i] == '"' && !is_escaped(&b, i) {
                in_quotes = !in_quotes;
                i += 1;
                continue;
            }

            if in_quotes {
                // Inside quotes, add characters directly (including spaces)
                if b[i] == '\\' && i + 1 < bus_len {
                    // Handle escaped characters inside quotes
                    i += 1;
                    prefix.push(b[i]);
                } else {
                    prefix.push(b[i]);
                }

                i += 1;
                continue;
            }

            if b[i] == '{' {
                if i > 0 && is_super_sub_overbar(b[i - 1]) {
                    brace_nesting += 1;
                    prefix.push('{');
                    i += 1;
                    continue;
                } else {
                    return false;
                }
            } else if b[i] == '}' {
                brace_nesting -= 1;
                prefix.push('}');
                i += 1;
                continue;
            }

            // Handle backslash-escaped spaces
            if b[i] == '\\' && i + 1 < bus_len && b[i + 1] == ' ' {
                i += 1;
                prefix.push(b[i]);
                i += 1;
                continue;
            }

            // Unescaped space or ] in bus vector prefix is not allowed
            if b[i] == ' ' || b[i] == ']' {
                return false;
            }

            if b[i] == '[' {
                if brace_nesting > 0 {
                    let fmt_start = prefix.iter().rposition(|&c| c == '{');

                    if let Some(fmt_start) = fmt_start
                        && fmt_start > 0
                        && is_super_sub_overbar(prefix[fmt_start - 1])
                    {
                        if fmt_start == prefix.len() - 1 {
                            // '{' immediately precedes '[' (e.g. D_{[1..2]}).
                            // The formatting decorates the range indices, not the
                            // name itself.
                            prefix.truncate(fmt_start - 1);
                        } else {
                            // Name characters exist between '{' and '[' (e.g.
                            // ~{BE[0..3]}).  The formatting wraps the signal name,
                            // not the range.
                            fmt_wraps_name = true;
                        }
                    }
                }

                break;
            }

            prefix.push(b[i]);
            i += 1;
        }

        // Parse start number
        //
        i += 1; // '[' character

        if i >= bus_len {
            return false;
        }

        while i < bus_len {
            if b[i] == '.' && i + 1 < bus_len && b[i + 1] == '.' {
                match to_long(&tmp) {
                    Some(value) => begin = value,
                    None => return false,
                }

                width = tmp.chars().count();
                padded = width > 1 && tmp.starts_with('0');
                i += 2;
                break;
            }

            if !is_digit(b[i]) {
                return false;
            }

            tmp.push(b[i]);
            i += 1;
        }

        // Parse end number
        //
        tmp.clear();

        if i >= bus_len {
            return false;
        }

        while i < bus_len {
            if b[i] == ']' {
                match to_long(&tmp) {
                    Some(value) => end = value,
                    None => return false,
                }

                let tmp_len = tmp.chars().count();
                padded |= tmp_len > 1 && tmp.starts_with('0');
                width = width.max(tmp_len);
                parsed_end = true;
                i += 1;
                break;
            }

            if !is_digit(b[i]) {
                return false;
            }

            tmp.push(b[i]);
            i += 1;
        }

        if !parsed_end {
            return false;
        }

        // Parse suffix
        //
        while i < bus_len {
            if b[i] == '}' {
                brace_nesting -= 1;

                if fmt_wraps_name {
                    suffix.push(b[i]);
                }
            } else if b[i] == '+' || b[i] == '-' || b[i] == 'P' || b[i] == 'N' {
                suffix.push(b[i]);
            } else {
                return false;
            }

            i += 1;
        }

        if brace_nesting != 0 {
            return false;
        }

        if begin == end {
            return false;
        } else if begin > end {
            std::mem::swap(&mut begin, &mut end);
        }

        let prefix: String = prefix.into_iter().collect();

        if let Some(name) = name {
            *name = prefix.clone();
        }

        if let Some(member_list) = member_list {
            // We can overflow the counter with the increment, so idx <= end is not safe here.
            let mut idx = begin;

            loop {
                let number = idx.to_string();
                let mut s = prefix.clone();

                if padded && number.len() < width {
                    s.extend(std::iter::repeat_n('0', width - number.len()));
                }

                s.push_str(&number);
                s.push_str(&suffix);
                member_list.push(s);

                if idx == end {
                    break;
                }

                idx += 1;
            }
        }

        true
    }

    /// Port of `NET_SETTINGS::ParseBusGroup`: parses a bus group label (e.g. `USB{DP DM}`)
    /// into the name (`USB`) and the members (`DP`, `DM`; spaces re-escaped and
    /// `EscapeString( CTX_NETNAME )`-encoded). The out-parameters are written exactly when C++
    /// writes them: `name` once the prefix is parsed (also when the member list then fails),
    /// members as they are found, `prefix_end` (the opening member-list brace position, in
    /// characters as C++ `size_t` string positions) only on success.
    pub fn parse_bus_group(
        group: &str,
        name: Option<&mut String>,
        mut member_list: Option<&mut Vec<String>>,
        prefix_end_out: Option<&mut usize>,
    ) -> bool {
        let g: Vec<char> = group.chars().collect();
        let group_len = g.len();
        let mut i = 0usize;
        let mut prefix = String::with_capacity(group.len());
        let mut tmp = String::new();
        let mut brace_nesting: i32 = 0;
        let mut in_quotes = false;

        // Escape spaces in member names so recursive parsing by ForEachBusMember works
        // correctly. Both quoted strings and backslash-escaped spaces collapse to bare spaces
        // during parsing, so we must re-escape them for subsequent ParseBusVector/ParseBusGroup
        // calls.
        let escape_spaces_for_bus = |member: &str| -> String {
            let mut escaped = String::with_capacity(member.len() * 2);

            for c in member.chars() {
                if c == ' ' {
                    escaped.push_str("\\ ");
                } else {
                    escaped.push(c);
                }
            }

            escaped
        };

        let push_member = |member_list: &mut Option<&mut Vec<String>>, tmp: &str| {
            if let Some(list) = member_list
                && !tmp.is_empty()
            {
                list.push(escape_string(
                    &escape_spaces_for_bus(tmp),
                    EscapeContext::Netname,
                ));
            }
        };

        // Parse prefix
        //
        // Formatting markers (^{}, _{}, ~{}) in the prefix are part of the group name
        // and must be preserved.  The member-list opening brace is distinguished by NOT
        // being preceded by a formatting character.
        while i < group_len {
            // Handle quoted strings (allows spaces inside)
            if g[i] == '"' && !is_escaped(&g, i) {
                in_quotes = !in_quotes;
                i += 1;
                continue;
            }

            if in_quotes {
                // Inside quotes, add characters directly (including spaces)
                if g[i] == '\\' && i + 1 < group_len {
                    // Handle escaped characters inside quotes
                    i += 1;
                    prefix.push(g[i]);
                } else {
                    prefix.push(g[i]);
                }

                i += 1;
                continue;
            }

            if g[i] == '{' {
                if i > 0 && is_super_sub_overbar(g[i - 1]) {
                    brace_nesting += 1;
                    prefix.push('{');
                    i += 1;
                    continue;
                } else {
                    break;
                }
            } else if g[i] == '}' {
                brace_nesting -= 1;
                prefix.push('}');
                i += 1;
                continue;
            }

            // Handle backslash-escaped spaces
            if g[i] == '\\' && i + 1 < group_len && g[i + 1] == ' ' {
                i += 1;
                prefix.push(g[i]);
                i += 1;
                continue;
            }

            // Unescaped space, [, or ] in bus group prefix is not allowed
            if g[i] == ' ' || g[i] == '[' || g[i] == ']' {
                return false;
            }

            prefix.push(g[i]);
            i += 1;
        }

        if brace_nesting != 0 {
            return false;
        }

        if let Some(name) = name {
            *name = prefix;
        }

        let prefix_end = i;

        // Parse members
        //
        i += 1; // '{' character

        if i >= group_len {
            return false;
        }

        in_quotes = false;

        while i < group_len {
            // Handle quoted strings (allows spaces inside member names)
            if g[i] == '"' && !is_escaped(&g, i) {
                in_quotes = !in_quotes;
                i += 1;
                continue;
            }

            if in_quotes {
                // Inside quotes, add characters directly (including spaces)
                if g[i] == '\\' && i + 1 < group_len {
                    // Handle escaped characters inside quotes
                    i += 1;
                    tmp.push(g[i]);
                } else {
                    tmp.push(g[i]);
                }

                i += 1;
                continue;
            }

            if g[i] == '{' {
                if i > 0 && is_super_sub_overbar(g[i - 1]) {
                    brace_nesting += 1;

                    // Keep the full formatting notation (e.g. ~{CAS}) in the member name.
                    // A net named ~{CAS} is distinct from CAS, and stripping the marker
                    // would lose that identity.  Vector bus members like D_{[1..2]} also
                    // preserve their subscript so recursive ForEachBusMember can parse them.
                    tmp.push('{');
                    i += 1;
                    continue;
                } else {
                    return false;
                }
            } else if g[i] == '}' {
                if brace_nesting != 0 {
                    brace_nesting -= 1;
                    tmp.push('}');
                    i += 1;
                    continue;
                } else {
                    push_member(&mut member_list, &tmp);

                    if let Some(out) = prefix_end_out {
                        *out = prefix_end;
                    }

                    return true;
                }
            }

            // Handle backslash-escaped spaces in member names
            if g[i] == '\\' && i + 1 < group_len && g[i + 1] == ' ' {
                i += 1;
                tmp.push(g[i]);
                i += 1;
                continue;
            }

            // Unescaped space or comma separates members
            if g[i] == ' ' || g[i] == ',' {
                push_member(&mut member_list, &tmp);
                tmp.clear();
                i += 1;
                continue;
            }

            tmp.push(g[i]);
            i += 1;
        }

        false
    }

    /// Port of `NET_SETTINGS::ForEachBusMember`: calls `function` for each member of an
    /// expanded bus pattern (vector buses and, recursively, bus groups), or once with the
    /// pattern itself when it is not a bus.
    pub fn for_each_bus_member(bus_pattern: &str, function: &mut dyn FnMut(&str)) {
        let mut members: Vec<String> = Vec::new();

        if Self::parse_bus_vector(bus_pattern, None, Some(&mut members)) {
            // Vector bus: call function for each expanded member
            for member in &members {
                function(member);
            }
        } else if Self::parse_bus_group(bus_pattern, None, Some(&mut members), None) {
            // Bus group: recursively expand each member (which may itself be a vector or
            // group)
            for member in &members {
                Self::for_each_bus_member(member, function);
            }
        } else {
            // Not a bus pattern: call function with the original pattern
            function(bus_pattern);
        }
    }
}

/// `isSuperSubOverbar` (`net_settings.cpp`).
fn is_super_sub_overbar(c: char) -> bool {
    c == '_' || c == '^' || c == '~'
}

/// Port of `isEscaped` (`net_settings.cpp`): the character at `pos` is preceded by an odd
/// number of backslashes.
fn is_escaped(s: &[char], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }

    // Count consecutive backslashes before this position
    let mut backslash_count = 0;
    let mut p = pos;

    while p > 0 && s[p - 1] == '\\' {
        backslash_count += 1;
        p -= 1;
    }

    // If odd number of backslashes, the character is escaped
    backslash_count % 2 == 1
}

/// `tmp.IsEmpty() || !tmp.ToLong( &value )` of `ParseBusVector` (the text holds only ASCII
/// digits there): `wxString::ToLong` is `strtol` base 10 into a 64-bit `long`, failing on
/// overflow (`ERANGE`).
fn to_long(tmp: &str) -> Option<i64> {
    if tmp.is_empty() {
        return None;
    }

    tmp.parse::<i64>().ok()
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
    }

    /// `NET_SETTINGS::addSinglePatternAssignment` / `addSingleChainPatternAssignment` /
    /// `ClearNetclassPatternAssignments` / `ClearChainPatternAssignments`
    /// (`common/project/net_settings.cpp`): the matcher is compiled once (`CTX_NETCLASS`:
    /// anchored), exact duplicates are skipped, and a clone of the settings keeps the
    /// compiled matchers.
    #[test]
    fn pattern_assignments_hold_compiled_matchers() {
        let mut ns = NetSettings::default();
        ns.add_single_pattern_assignment("HV*", "HighVoltage");
        ns.add_single_pattern_assignment("HV*", "HighVoltage");
        ns.add_single_pattern_assignment("HV*", "Other");
        assert_eq!(ns.netclass_pattern_assignments().len(), 2);

        let (matcher, class) = &ns.netclass_pattern_assignments()[0];
        assert_eq!((matcher.pattern(), class.as_str()), ("HV*", "HighVoltage"));
        assert!(matcher.find("HV_BUS"));
        assert!(!matcher.find("xHV_BUS"));

        ns.add_single_chain_pattern_assignment(NetChainSource::Board, "/CHAIN?", "Fast");
        ns.add_single_chain_pattern_assignment(NetChainSource::Board, "/CHAIN?", "Fast");
        let copy = ns.clone();
        let chain = &copy.netclass_chain_pattern_assignments[&NetChainSource::Board];
        assert_eq!(chain.len(), 1);
        assert!(chain[0].0.find("/CHAIN1"));
        assert!(copy.netclass_pattern_assignments()[1].0.find("HV1"));

        ns.clear_chain_pattern_assignments(NetChainSource::Board);
        assert!(ns.netclass_chain_pattern_assignments[&NetChainSource::Board].is_empty());
        ns.clear_netclass_pattern_assignments();
        assert!(ns.netclass_pattern_assignments().is_empty());
        ns.set_netclass_pattern_assignments(vec![(netclass_matcher("A?"), "X".to_string())]);
        assert_eq!(ns.netclass_pattern_assignments().len(), 1);
    }

    // ---- qa/tests/common/test_net_settings.cpp (the cases without the JSON layer) ----

    fn class(name: &str) -> Arc<Netclass> {
        Arc::new(Netclass::new(name, false))
    }

    fn classes(names: &[&str]) -> BTreeMap<String, Arc<Netclass>> {
        names.iter().map(|n| (n.to_string(), class(n))).collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// `resolvesToNetclass`: the effective class is `expected`, or has it as a constituent.
    fn resolves_to_netclass(settings: &mut NetSettings, net_name: &str, expected: &str) -> bool {
        let resolved = settings.get_effective_netclass(net_name);

        if resolved.get_name() == expected {
            return true;
        }

        resolved
            .constituent_netclasses()
            .iter()
            .any(|c| c.get_name() == expected)
    }

    #[test]
    fn chain_class_assignment_affects_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        assert!(a == b);
        a.set_net_chain_class("CHAIN_A", "Default");
        assert!(a != b);
        b.set_net_chain_class("CHAIN_A", "Default");
        assert!(a == b);
    }

    #[test]
    fn chain_class_value_and_clear_affect_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        a.set_net_chain_class("CHAIN_A", "Default");
        b.set_net_chain_class("CHAIN_A", "HighSpeed");
        assert!(a != b);
        b.set_net_chain_class("CHAIN_A", "Default");
        assert!(a == b);
        a.set_net_chain_class("CHAIN_A", "");
        assert!(a != b);
        b.set_net_chain_class("CHAIN_A", "");
        assert!(a == b);
    }

    #[test]
    fn assignment_size_differences_affect_equality() {
        let a = NetSettings::default();
        let mut b = NetSettings::default();
        b.set_netclass_label_assignment("NET_A", set(&["Default"]));
        assert!(a != b);
        assert!(b != a);
    }

    #[test]
    fn chain_pattern_assignment_resolves_and_clears() {
        let mut settings = NetSettings::default();
        settings.set_netclasses(classes(&["HighSpeed"]));
        settings.set_chain_pattern_assignment(NetChainSource::Schematic, "DDR_DQ0", "HighSpeed");
        assert!(resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
        settings.clear_chain_pattern_assignments(NetChainSource::Schematic);
        assert!(!resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
    }

    #[test]
    fn clear_chain_pattern_assignments_leaves_user_patterns() {
        let mut settings = NetSettings::default();
        settings.set_netclasses(classes(&["HighSpeed", "Power"]));
        settings.set_netclass_pattern_assignment("VCC_*", "Power");
        settings.set_chain_pattern_assignment(NetChainSource::Schematic, "DDR_DQ0", "HighSpeed");
        assert!(resolves_to_netclass(&mut settings, "VCC_3V3", "Power"));
        assert!(resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
        settings.clear_chain_pattern_assignments(NetChainSource::Schematic);
        assert!(resolves_to_netclass(&mut settings, "VCC_3V3", "Power"));
        assert!(!resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
    }

    #[test]
    fn chain_pattern_assignment_sources_are_independent() {
        let mut settings = NetSettings::default();
        settings.set_netclasses(classes(&["HighSpeed", "Power"]));
        settings.set_chain_pattern_assignment(NetChainSource::Schematic, "DDR_DQ0", "HighSpeed");
        settings.set_chain_pattern_assignment(NetChainSource::Board, "VCC_3V3", "Power");
        assert!(resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
        assert!(resolves_to_netclass(&mut settings, "VCC_3V3", "Power"));
        settings.clear_chain_pattern_assignments(NetChainSource::Board);
        assert!(resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
        assert!(!resolves_to_netclass(&mut settings, "VCC_3V3", "Power"));
        assert!(settings.has_chain_pattern_assignments(NetChainSource::Schematic));
        assert!(!settings.has_chain_pattern_assignments(NetChainSource::Board));
        settings.clear_chain_pattern_assignments(NetChainSource::Schematic);
        assert!(!resolves_to_netclass(&mut settings, "DDR_DQ0", "HighSpeed"));
    }

    #[test]
    fn clear_net_chain_classes_removes_all_entries() {
        let mut settings = NetSettings::default();
        settings.set_net_chain_class("CHAIN_A", "Default");
        settings.set_net_chain_class("CHAIN_B", "HighSpeed");
        assert_eq!(settings.net_chain_classes().len(), 2);
        settings.clear_net_chain_classes();
        assert!(settings.net_chain_classes().is_empty());
        assert!(settings.get_net_chain_class("CHAIN_A").is_empty());
    }

    #[test]
    fn chain_pattern_assignment_excluded_from_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        let shared = classes(&["HighSpeed", "Power"]);
        a.set_netclasses(shared.clone());
        b.set_netclasses(shared);
        assert!(a == b);
        a.set_chain_pattern_assignment(NetChainSource::Schematic, "DDR_DQ0", "HighSpeed");
        assert!(a == b);
        a.set_chain_pattern_assignment(NetChainSource::Schematic, "VCC_3V3", "Power");
        b.set_chain_pattern_assignment(NetChainSource::Board, "OTHER", "HighSpeed");
        assert!(a == b);
        a.clear_chain_pattern_assignments(NetChainSource::Schematic);
        assert!(a == b);
    }

    #[test]
    fn chain_netclass_assignment_affects_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        a.set_net_chain_net_class("CHAIN_A", "HighSpeed");
        assert!(a != b);
        b.set_net_chain_net_class("CHAIN_A", "Power");
        assert!(a != b);
        b.set_net_chain_net_class("CHAIN_A", "HighSpeed");
        assert!(a == b);
        a.set_net_chain_net_class("CHAIN_A", "");
        assert!(a != b);
        b.clear_net_chain_net_classes();
        assert!(a == b);
    }

    /// In-place edits through the `shared_ptr` are `Arc::make_mut` on the settings' handle.
    #[test]
    fn default_netclass_in_place_edit_affects_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        assert!(a == b);
        Arc::make_mut(&mut a.default_netclass).clearance = Some(350_000);
        assert!(a != b);
        Arc::make_mut(&mut b.default_netclass).clearance = Some(350_000);
        assert!(a == b);
        Arc::make_mut(&mut a.default_netclass).track_width = Some(254_000);
        assert!(a != b);
    }

    #[test]
    fn default_netclass_replacement_affects_equality() {
        let mut a = NetSettings::default();
        let b = NetSettings::default();
        let mut replacement = Netclass::new(NETCLASS_DEFAULT_NAME, true);
        replacement.track_width = Some(300_000);
        a.set_default_netclass(Arc::new(replacement));
        assert!(a != b);
    }

    #[test]
    fn named_netclass_parameter_edit_affects_equality() {
        let mut a = NetSettings::default();
        let mut b = NetSettings::default();
        a.set_netclass("HighSpeed", class("HighSpeed"));
        b.set_netclass("HighSpeed", class("HighSpeed"));
        assert!(a == b);
        Arc::make_mut(a.netclasses.get_mut("HighSpeed").unwrap()).clearance = Some(100_000);
        assert!(a != b);
        Arc::make_mut(b.netclasses.get_mut("HighSpeed").unwrap()).clearance = Some(100_000);
        assert!(a == b);
    }

    #[test]
    fn rename_net_path_prefix_retargets_assignments() {
        let mut settings = NetSettings::default();
        settings.set_netclasses(classes(&["HighSpeed"]));
        settings.set_netclass_pattern_assignment("/Sheet1/CLK", "HighSpeed");
        settings.set_netclass_pattern_assignment("/Sheet1/DATA*", "HighSpeed");
        settings.set_netclass_pattern_assignment("/Sheet1/Sub/EN", "HighSpeed");
        settings.set_netclass_pattern_assignment("/Other/RST", "HighSpeed");
        settings.set_netclass_pattern_assignment("/Sheet10/BUS", "HighSpeed");
        settings.set_net_color_assignment("/Sheet1/CLK", Color4D::new(1.0, 0.0, 0.0, 1.0));

        assert!(resolves_to_netclass(
            &mut settings,
            "/Sheet1/CLK",
            "HighSpeed"
        ));
        assert!(!resolves_to_netclass(
            &mut settings,
            "/Sheet2/CLK",
            "HighSpeed"
        ));

        assert!(settings.rename_net_path_prefix("/Sheet1/", "/Sheet2/"));

        assert!(resolves_to_netclass(
            &mut settings,
            "/Sheet2/CLK",
            "HighSpeed"
        ));
        assert!(resolves_to_netclass(
            &mut settings,
            "/Sheet2/DATA0",
            "HighSpeed"
        ));
        assert!(resolves_to_netclass(
            &mut settings,
            "/Sheet2/Sub/EN",
            "HighSpeed"
        ));
        assert!(!resolves_to_netclass(
            &mut settings,
            "/Sheet1/CLK",
            "HighSpeed"
        ));
        assert!(resolves_to_netclass(
            &mut settings,
            "/Other/RST",
            "HighSpeed"
        ));
        assert!(resolves_to_netclass(
            &mut settings,
            "/Sheet10/BUS",
            "HighSpeed"
        ));

        let colors = settings.net_color_assignments();
        assert!(colors.contains_key("/Sheet2/CLK"));
        assert!(!colors.contains_key("/Sheet1/CLK"));

        assert!(!settings.rename_net_path_prefix("/Nope/", "/Nada/"));
    }

    #[test]
    fn shared_parser_rejects_malformed_bounds_without_expansion() {
        assert!(!NetSettings::parse_bus_vector("D[..2]", None, None));
        assert!(!NetSettings::parse_bus_vector("D[1..]", None, None));
        assert!(!NetSettings::parse_bus_vector("D[1..2", None, None));
        assert!(!NetSettings::parse_bus_vector(
            "D[1..999999999999999999999999999999]",
            None,
            None
        ));
    }

    #[test]
    fn shared_parser_expands_largest_vector_index() {
        let last = i64::MAX;
        let text = format!("D[{}..{}]", last - 1, last);
        let mut members = Vec::new();
        assert!(NetSettings::parse_bus_vector(
            &text,
            None,
            Some(&mut members)
        ));
        assert_eq!(members.len(), 2);
        assert_eq!(members[1], format!("D{last}"));
    }

    #[test]
    fn shared_parser_preserves_padding() {
        let mut members = Vec::new();
        assert!(NetSettings::parse_bus_vector(
            "D[01..03]",
            None,
            Some(&mut members)
        ));
        assert_eq!(members.len(), 3);
        assert_eq!(members[0], "D01");
        assert_eq!(members[2], "D03");
    }

    #[test]
    fn shared_parser_group_prefix_boundary_uses_source_spelling() {
        let mut boundary = 99usize;
        assert!(NetSettings::parse_bus_group(
            "I^{2}C\\ BUS{A}",
            None,
            None,
            Some(&mut boundary)
        ));
        assert_eq!(boundary, "I^{2}C\\ BUS".chars().count());
        assert!(NetSettings::parse_bus_group(
            "{A}",
            None,
            None,
            Some(&mut boundary)
        ));
        assert_eq!(boundary, 0);
        boundary = 99;
        assert!(!NetSettings::parse_bus_group(
            "G{A",
            None,
            None,
            Some(&mut boundary)
        ));
        assert_eq!(boundary, 99);
    }

    #[test]
    fn composite_owns_constituents() {
        let mut settings = NetSettings::default();
        let weak_signal;

        {
            let signal = class("Signal");
            weak_signal = Arc::downgrade(&signal);
            settings.set_netclass("Signal", signal);
        }

        settings.set_netclass_label_assignment("N1", set(&["Signal"]));
        let composite = settings.get_effective_netclass("N1");
        assert_eq!(composite.constituent_netclasses().len(), 2);

        settings.clear_netclasses();
        assert!(weak_signal.upgrade().is_some());
        assert_eq!(composite.get_name(), "Signal,Default");

        drop(composite);
        assert!(weak_signal.upgrade().is_none());
    }

    // ---- differential oracle: tests/oracle/net_settings_oracle.cpp ----

    fn parse_color(s: &str) -> Color4D {
        if s == "~" {
            return Color4D::UNSPECIFIED;
        }

        let v: Vec<f64> = s.split(',').map(|x| x.parse().unwrap()).collect();
        Color4D::new(v[0], v[1], v[2], v[3])
    }

    fn color_str(c: &Color4D) -> String {
        if *c == Color4D::UNSPECIFIED {
            return "~".to_string();
        }

        let q = |x: f64| (x * 1000.0).round() as i64;
        format!("{},{},{},{}", q(c.r), q(c.g), q(c.b), q(c.a))
    }

    /// `applyFields` of the driver: `f[first]` is the priority.
    fn apply_fields(nc: &mut Netclass, f: &[&str], first: usize) {
        let opt = |i: usize, field: &mut Option<i32>| match f[first + i] {
            "=" => {}
            "~" => *field = None,
            v => *field = Some(v.parse().unwrap()),
        };

        if f[first] != "=" {
            nc.priority = f[first].parse().unwrap();
        }

        opt(1, &mut nc.clearance);
        opt(2, &mut nc.track_width);
        opt(3, &mut nc.via_dia);
        opt(4, &mut nc.via_drill);
        opt(5, &mut nc.uvia_dia);
        opt(6, &mut nc.uvia_drill);
        opt(7, &mut nc.diff_pair_width);
        opt(8, &mut nc.diff_pair_gap);
        opt(9, &mut nc.diff_pair_via_gap);
        opt(10, &mut nc.wire_width);
        opt(11, &mut nc.bus_width);
        opt(12, &mut nc.line_style);

        if f[first + 13] != "=" {
            nc.pcb_color = parse_color(f[first + 13]);
        }

        if f[first + 14] != "=" {
            nc.schematic_color = parse_color(f[first + 14]);
        }

        if f[first + 15] != "=" {
            nc.tuning_profile = f[first + 15].to_string();
        }
    }

    fn opt_str(v: Option<i32>) -> String {
        v.map_or_else(|| "~".to_string(), |v| v.to_string())
    }

    fn kind_of(ns: &NetSettings, nc: &Arc<Netclass>) -> &'static str {
        if Arc::ptr_eq(nc, &ns.default_netclass) {
            return "default";
        }

        if ns
            .netclasses
            .get(&nc.get_name())
            .is_some_and(|c| Arc::ptr_eq(c, nc))
        {
            return "class";
        }

        if ns.composite_netclasses.values().any(|c| Arc::ptr_eq(c, nc)) {
            return "composite";
        }

        "other"
    }

    fn detail(nc: &Netclass) -> String {
        let p = &nc.parents;
        let parent =
            |x: &Option<Arc<Netclass>>| x.as_ref().map_or_else(|| nc.get_name(), |p| p.get_name());
        let parents = [
            &p.clearance,
            &p.track_width,
            &p.via_diameter,
            &p.via_drill,
            &p.uvia_dia,
            &p.uvia_drill,
            &p.diff_pair_width,
            &p.diff_pair_gap,
            &p.diff_pair_via_gap,
            &p.wire_width,
            &p.bus_width,
            &p.line_style,
            &p.pcb_color,
            &p.schematic_color,
            &p.tuning_profile,
        ]
        .iter()
        .map(|x| parent(x))
        .collect::<Vec<_>>()
        .join(",");
        let cons = nc
            .constituent_netclasses()
            .iter()
            .map(|c| c.get_name())
            .collect::<Vec<_>>()
            .join(";");

        format!(
            "name={}|default={}|prio={}|cl={}|tw={}|vd={}|vdr={}|uvd={}|uvdr={}|dpw={}|dpg={}|\
             dpvg={}|ww={}|bw={}|ls={}|pcb={}|sch={}|tp={}|par={}|cons={}",
            nc.get_name(),
            nc.is_default() as i32,
            nc.priority,
            opt_str(nc.clearance),
            opt_str(nc.track_width),
            opt_str(nc.via_dia),
            opt_str(nc.via_drill),
            opt_str(nc.uvia_dia),
            opt_str(nc.uvia_drill),
            opt_str(nc.diff_pair_width),
            opt_str(nc.diff_pair_gap),
            opt_str(nc.diff_pair_via_gap),
            opt_str(nc.wire_width),
            opt_str(nc.bus_width),
            opt_str(nc.line_style),
            color_str(&nc.pcb_color),
            color_str(&nc.schematic_color),
            nc.tuning_profile,
            parents,
            cons
        )
    }

    fn source(s: &str) -> NetChainSource {
        if s == "S" {
            NetChainSource::Schematic
        } else {
            NetChainSource::Board
        }
    }

    /// Runs the scenario script like the C++ driver and returns its output lines.
    fn run_scenarios(script: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut ns = NetSettings::default();

        for line in script.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Corpus scenarios separate fields by tabs (patterns may contain '|').
            let f: Vec<&str> = line
                .split(if line.contains('\t') { '\t' } else { '|' })
                .collect();

            match f[0] {
                "reset" => {
                    ns = NetSettings::default();
                    out.push(match f.get(1) {
                        Some(label) => format!("reset|{label}"),
                        None => "reset".to_string(),
                    });
                }
                "class" => {
                    let mut nc = Netclass::new(f[1], false);
                    apply_fields(&mut nc, &f, 2);
                    ns.set_netclass(f[1], Arc::new(nc));
                }
                "default" => apply_fields(Arc::make_mut(&mut ns.default_netclass), &f, 1),
                "setclasses" => {
                    let classes = ns.netclasses().clone();
                    ns.set_netclasses(classes);
                }
                "clearclasses" => ns.clear_netclasses(),
                "clearcaches" | "clearall" => ns.clear_all_caches(),
                "recompute" => ns.recompute_effective_netclasses(),
                "composites" => {
                    for (key, nc) in ns.composite_netclasses() {
                        out.push(format!("comp|{key}|{}", detail(nc)));
                    }

                    out.push(format!("composites|{}", ns.composite_netclasses().len()));
                }
                "patterns" => {
                    for (matcher, nc) in ns.netclass_pattern_assignments() {
                        out.push(format!("pat|{}|{nc}", matcher.pattern()));
                    }

                    for (src, list) in ns.chain_pattern_assignments() {
                        let s = if *src == NetChainSource::Schematic {
                            "S"
                        } else {
                            "B"
                        };

                        for (matcher, nc) in list {
                            out.push(format!("chainpat|{s}|{}|{nc}", matcher.pattern()));
                        }
                    }

                    out.push(format!(
                        "haschain|{}|{}",
                        ns.has_chain_pattern_assignments(NetChainSource::Schematic) as i32,
                        ns.has_chain_pattern_assignments(NetChainSource::Board) as i32
                    ));
                }
                "colors" => {
                    for (net, color) in ns.net_color_assignments() {
                        out.push(format!("color|{net}|{}", color_str(color)));
                    }
                }
                "color" => ns.set_net_color_assignment(f[1], parse_color(f[2])),
                "label" => {
                    let classes: BTreeSet<String> = f[2].split(',').map(String::from).collect();
                    ns.set_netclass_label_assignment(f[1], classes);
                }
                "appendlabel" => {
                    let classes: BTreeSet<String> = f[2].split(',').map(String::from).collect();
                    ns.append_netclass_label_assignment(f[1], &classes);
                }
                "clearlabel" => ns.clear_netclass_label_assignment(f[1]),
                "clearlabels" => ns.clear_netclass_label_assignments(),
                "pattern" => ns.set_netclass_pattern_assignment(f[1], f[2]),
                "clearpatterns" => ns.clear_netclass_pattern_assignments(),
                "chain" => ns.set_chain_pattern_assignment(source(f[1]), f[2], f[3]),
                "clearchain" => ns.clear_chain_pattern_assignments(source(f[1])),
                "q" => {
                    let nc = ns.get_effective_netclass(f[1]);
                    out.push(format!(
                        "q|{}|kind={}|{}",
                        f[1],
                        kind_of(&ns, &nc),
                        detail(&nc)
                    ));
                }
                "qn" => {
                    let nc = ns.get_effective_netclass(f[1]);
                    out.push(format!("qn|{}|{}", f[1], nc.get_name()));
                }
                "rx" => {
                    use kicad_core::eda_pattern_match::{
                        EdaPatternMatch, EdaPatternMatchRegexAnchored,
                    };

                    let mut regex = EdaPatternMatchRegexAnchored::default();
                    out.push(format!("rx|{}|{}", f[1], regex.set_pattern(f[1]) as i32));
                }
                "has" => out.push(format!(
                    "has|{}|{}",
                    f[1],
                    ns.has_effective_netclass(f[1]) as i32
                )),
                "clearnet" => ns.clear_cache_for_net(f[1]),
                "byname" => {
                    let nc = ns.get_net_class_by_name(f[1]).unwrap();
                    out.push(format!(
                        "byname|{}|kind={}|{}",
                        f[1],
                        kind_of(&ns, &nc),
                        nc.get_name()
                    ));
                }
                "renameprefix" => {
                    let changed = ns.rename_net_path_prefix(f[1], f[2]);
                    out.push(format!("renameprefix|{}|{}|{}", f[1], f[2], changed as i32));
                }
                "renamenets" => {
                    let mut names = BTreeMap::new();

                    if !f[1].is_empty() {
                        for pair in f[1].split(',') {
                            let (k, v) = pair.split_once('=').unwrap();
                            names.insert(k.to_string(), v.to_string());
                        }
                    }

                    out.push(format!(
                        "renamenets|{}|{}",
                        f[1],
                        ns.rename_nets(&names) as i32
                    ));
                }
                "bv" => {
                    let mut name = "?".to_string();
                    let mut members = Vec::new();
                    let ok =
                        NetSettings::parse_bus_vector(f[1], Some(&mut name), Some(&mut members));
                    out.push(format!(
                        "bv|{}|ok={}|name={name}|members={}",
                        f[1],
                        ok as i32,
                        members.join(";")
                    ));
                }
                "bg" => {
                    let mut name = "?".to_string();
                    let mut members = Vec::new();
                    let mut prefix_end = 99usize;
                    let ok = NetSettings::parse_bus_group(
                        f[1],
                        Some(&mut name),
                        Some(&mut members),
                        Some(&mut prefix_end),
                    );
                    out.push(format!(
                        "bg|{}|ok={}|name={name}|members={}|prefix_end={prefix_end}",
                        f[1],
                        ok as i32,
                        members.join(";")
                    ));
                }
                "fe" => {
                    let mut members: Vec<String> = Vec::new();
                    NetSettings::for_each_bus_member(f[1], &mut |m| members.push(m.to_string()));
                    out.push(format!("fe|{}|{}", f[1], members.join(";")));
                }
                other => panic!("unknown scenario command {other}"),
            }
        }

        out
    }

    /// Replays `tests/data/net_settings/scenarios.txt` and compares every line with the
    /// output of the C++ driver on the reference build (`tests/golden/net_settings.txt`,
    /// written by `tests/oracle/net_settings_golden.sh`).
    #[test]
    fn oracle_scenarios() {
        let script = include_str!("../tests/data/net_settings/scenarios.txt");
        let golden = include_str!("../tests/golden/net_settings.txt");
        let actual = run_scenarios(script);
        let expected: Vec<&str> = golden.lines().collect();

        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "line {} differs", i + 1);
        }

        assert_eq!(actual.len(), expected.len());
    }

    /// Replays `tests/data/net_settings/corpus_scenarios.txt` (generated from every corpus
    /// `.kicad_pro` with netclass patterns by `tests/oracle/net_settings_corpus.py`: regex
    /// compilability of each pattern and the effective netclass name of the board's nets)
    /// against `tests/golden/net_settings_corpus.txt`.
    #[test]
    fn oracle_corpus_scenarios() {
        let script = include_str!("../tests/data/net_settings/corpus_scenarios.txt");
        let golden = include_str!("../tests/golden/net_settings_corpus.txt");
        let actual = run_scenarios(script);
        let expected: Vec<&str> = golden.lines().collect();
        let mismatches: Vec<String> = actual
            .iter()
            .zip(&expected)
            .filter(|(a, e)| a != *e)
            .map(|(a, e)| format!("rust {a}\n  c++ {e}"))
            .collect();

        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert_eq!(actual.len(), expected.len());
    }
}
