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

//! Port of `include/case_insensitive_map.h`: `CASE_INSENSITIVE_MAP<T>`, the
//! `std::map<wxString, T, CASE_INSENSITIVE_COMPARER>` KiCad uses for footprint and component
//! variants (`FOOTPRINT::m_variants`, `COMPONENT::m_variants`) and Altium library caches,
//! ordered by `wxString::CmpNoCase`.
//!
//! As with `std::map`, keys that compare equal ignoring case are the same entry: the key text
//! first inserted is kept. `BTreeMap::insert` replaces the value (like `map[key] = value`);
//! use `entry( key ).or_insert( .. )` for `std::map::insert` / `emplace`, which keep the old
//! value.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::wxstr::cmp_no_case;

/// Port of `DETAIL::CASE_INSENSITIVE_COMPARER`: `aLhs.CmpNoCase( aRhs ) < 0`.
pub fn case_insensitive_less(lhs: &str, rhs: &str) -> bool {
    cmp_no_case(lhs, rhs) < 0
}

/// Three-way form of [`case_insensitive_less`] (`wxString::CmpNoCase` sign).
pub fn case_insensitive_cmp(lhs: &str, rhs: &str) -> Ordering {
    cmp_no_case(lhs, rhs).cmp(&0)
}

/// A map key ordered with `wxString::CmpNoCase`. The original spelling is kept.
#[derive(Clone, Debug, Default)]
pub struct CaseInsensitiveKey(pub String);

impl CaseInsensitiveKey {
    /// The key as stored.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CaseInsensitiveKey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for CaseInsensitiveKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl PartialEq for CaseInsensitiveKey {
    fn eq(&self, other: &Self) -> bool {
        cmp_no_case(&self.0, &other.0) == 0
    }
}

impl Eq for CaseInsensitiveKey {}

impl PartialOrd for CaseInsensitiveKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CaseInsensitiveKey {
    fn cmp(&self, other: &Self) -> Ordering {
        case_insensitive_cmp(&self.0, &other.0)
    }
}

/// Port of `CASE_INSENSITIVE_MAP<ValueType>`.
pub type CaseInsensitiveMap<V> = BTreeMap<CaseInsensitiveKey, V>;

/// Looks `key` up ignoring case (`map.find( key )`).
pub fn ci_get<'m, V>(map: &'m CaseInsensitiveMap<V>, key: &str) -> Option<&'m V> {
    map.get(&CaseInsensitiveKey(key.to_string()))
}

/// Mutable [`ci_get`].
pub fn ci_get_mut<'m, V>(map: &'m mut CaseInsensitiveMap<V>, key: &str) -> Option<&'m mut V> {
    map.get_mut(&CaseInsensitiveKey(key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_collapse_ignoring_case() {
        let mut m: CaseInsensitiveMap<i32> = CaseInsensitiveMap::new();
        m.insert("Variant".into(), 1);
        m.insert("VARIANT".into(), 2);
        m.insert("alpha".into(), 3);
        m.entry("ALPHA".into()).or_insert(4);

        assert_eq!(m.len(), 2);
        assert_eq!(ci_get(&m, "variant"), Some(&2));
        assert_eq!(ci_get(&m, "Alpha"), Some(&3));
        // std::map keeps the first spelling of the key.
        let keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["alpha", "Variant"]);
        assert!(case_insensitive_less("abc", "ABD"));
        assert!(!case_insensitive_less("ABC", "abc"));
    }
}
