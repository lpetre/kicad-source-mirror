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

//! Port of `pcbnew/component_classes/component_class.h`, `component_class_cache_proxy.h` and
//! `component_class_manager.cpp`.
//!
//! Placeholder written by M0a; the type package M0d fills in the definitions and `Default`
//! (`board-model.md` 16). `ComponentClass` and `ComponentClassCache` exist already because
//! `Footprint` (package M0b) holds them: M0b uses them as they are, M0d completes them here.

use std::sync::Arc;

/// `COMPONENT_CLASS`.
///
/// PLACEHOLDER (M0a review): M0d replaces the body with all fields of `board-model-types.md`
/// (and adds `ComponentClassUsage`).
#[derive(Clone, Debug, Default)]
pub struct ComponentClass {}

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

/// `COMPONENT_CLASS_MANAGER`.
///
/// PLACEHOLDER (M0a): M0d replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct ComponentClassManager {}
