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

//! The two cache cells items may hold (new code, `board-model.md` section 9).
//!
//! Both are `Sync` (a `std::sync::Mutex` inside, which also works on wasm32), both `Clone` to
//! **empty** (which is what lets item structs derive `Clone`), both `Debug`, and both are
//! ignored by item equality (`eq_item` / `compare` are written by hand). A bare `Mutex` must
//! never be a field of an item struct.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// Lock a mutex, ignoring poisoning (a panic inside a cache build leaves at worst an empty or
/// stale cell, which the `valid` predicates handle).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Re-validated from `&self` (C++ `mutable` + mutex + time stamp / dirty check). `T` stores the
/// inputs it was built from (time stamp, parent transform, text ...); `valid` compares them, so
/// a forgotten invalidation cannot produce a stale read. A cache whose only input is the item
/// itself uses `|_| true` and relies on `Invalidate`.
pub struct KeyedCache<T>(Mutex<Option<Arc<T>>>);

impl<T> KeyedCache<T> {
    /// An empty cache.
    pub const fn new() -> Self {
        KeyedCache(Mutex::new(None))
    }

    /// The cached value if `valid` accepts it, else `build()` (run without holding the lock,
    /// so that `build` may read other caches of the same item) stored and returned. When two
    /// threads build concurrently the last store wins; both results are equal by construction.
    pub fn get_or_build(&self, valid: impl Fn(&T) -> bool, build: impl FnOnce() -> T) -> Arc<T> {
        if let Some(value) = self.peek() {
            if valid(&value) {
                return value;
            }
        }

        let value = Arc::new(build());
        *lock(&self.0) = Some(Arc::clone(&value));
        value
    }

    /// The cached value, valid or not.
    pub fn peek(&self) -> Option<Arc<T>> {
        lock(&self.0).clone()
    }

    /// Replace the cached value.
    pub fn store(&self, value: T) {
        *lock(&self.0) = Some(Arc::new(value));
    }

    /// Drop the cached value from `&self` (C++ `SetDirty()` on a `const` path).
    pub fn invalidate(&self) {
        *lock(&self.0) = None;
    }

    /// Drop the cached value (`&mut self`: no locking).
    pub fn clear(&mut self) {
        *self
            .0
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// Patch the cached value in place, if there is one (C++ `FOOTPRINT::Flip` / `Move`
    /// updating its bounding-box caches instead of dropping them).
    pub fn update(&mut self, f: impl FnOnce(&mut T))
    where
        T: Clone,
    {
        let slot = self
            .0
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(value) = slot.as_mut() {
            f(Arc::make_mut(value));
        }
    }
}

impl<T> Default for KeyedCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for KeyedCache<T> {
    /// A copy starts empty (the C++ copy constructors do not copy these caches either, or
    /// rebuild them on first use).
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for KeyedCache<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let filled = lock(&self.0).is_some();
        write!(f, "KeyedCache({})", if filled { "filled" } else { "empty" })
    }
}

/// Plain interior-mutable scratch for C++ `mutable` members that are not "one value built
/// from inputs" (per-line text bbox map, the three time-stamped footprint boxes).
pub struct CacheCell<T: Default>(Mutex<T>);

impl<T: Default> CacheCell<T> {
    /// A cell holding `T::default()`.
    pub fn new() -> Self {
        CacheCell(Mutex::new(T::default()))
    }

    /// Run `f` on the contents (the lock is held while `f` runs: `f` must not touch this
    /// cell again).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut lock(&self.0))
    }

    /// Reset the contents to `T::default()`.
    pub fn clear(&mut self) {
        *self
            .0
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = T::default();
    }
}

impl<T: Default> Default for CacheCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default> Clone for CacheCell<T> {
    /// A copy starts with `T::default()`.
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<T: Default> fmt::Debug for CacheCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CacheCell")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_cache_rebuilds_on_key_change_and_clones_empty() {
        let cache: KeyedCache<(i32, String)> = KeyedCache::new();
        let built = cache.get_or_build(|v| v.0 == 1, || (1, "one".into()));
        assert_eq!(built.1, "one");

        let again = cache.get_or_build(|v| v.0 == 1, || unreachable!());
        assert!(Arc::ptr_eq(&built, &again));

        let rebuilt = cache.get_or_build(|v| v.0 == 2, || (2, "two".into()));
        assert_eq!(rebuilt.1, "two");

        assert!(cache.clone().peek().is_none());
        cache.invalidate();
        assert!(cache.peek().is_none());

        let mut cache = cache;
        cache.store((3, "three".into()));
        cache.update(|v| v.0 = 4);
        assert_eq!(cache.peek().unwrap().0, 4);
        cache.clear();
        assert!(cache.peek().is_none());
    }

    #[test]
    fn cache_cell_clones_to_default() {
        let mut cell: CacheCell<Vec<i32>> = CacheCell::new();
        cell.with(|v| v.push(1));
        assert_eq!(cell.with(|v| v.len()), 1);
        assert_eq!(cell.clone().with(|v| v.len()), 0);
        cell.clear();
        assert_eq!(cell.with(|v| v.len()), 0);
    }

    fn assert_sync<T: Sync + Send>() {}

    #[test]
    fn cells_are_sync() {
        assert_sync::<KeyedCache<Vec<u8>>>();
        assert_sync::<CacheCell<Vec<u8>>>();
    }
}
