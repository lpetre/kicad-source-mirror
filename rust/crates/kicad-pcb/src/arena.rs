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

//! Generational slot vector holding one kind of board item (new code, `board-model.md`
//! section 2 and `board-model-types.md` section 0).
//!
//! C++ owns board items through raw pointers in `std::deque` / `std::vector` containers; the
//! port stores every item in a board-level `Arena<T>` and keeps the C++ containers as order
//! lists of handles. A slot is `Live`, `Free` (on the free list) or `Retired` (its 16-bit
//! generation is exhausted; never handed out again). While no slot has been freed, `insert`
//! hands out indices in insertion order (the parser relies on it: `board-model.md` 3.1).
//! Slot order is meaningless for semantics: iterate the order lists, never an arena.

use std::fmt;

use crate::ids::Id;

/// Sentinel for "no free slot" in the free list.
const NO_FREE: u32 = u32::MAX;

/// State of one slot.
#[derive(Clone)]
enum SlotState<T> {
    Live(T),
    Free {
        next_free: u32,
    },
    /// Generation exhausted; the slot is never reused.
    Retired,
}

#[derive(Clone)]
struct Slot<T> {
    generation: u16,
    state: SlotState<T>,
}

/// Generational slot vector (plain `Vec` + enum, no unsafe). `Clone` clones the items, so every
/// handle (and every item's `base.id`) stays valid in the copy.
#[derive(Clone)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: u32,
    live: u32,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            free_head: NO_FREE,
            live: 0,
        }
    }
}

impl<T> Arena<T> {
    /// An empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live items.
    pub fn len(&self) -> usize {
        self.live as usize
    }

    /// True when no item is live.
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Store `value` and return its handle. Reuses the most recently freed slot (with a new
    /// generation); while nothing was freed the index is the insertion count.
    pub fn insert(&mut self, value: T) -> Id<T> {
        self.live += 1;

        if self.free_head != NO_FREE {
            let index = self.free_head;
            let slot = &mut self.slots[index as usize];

            match slot.state {
                SlotState::Free { next_free } => self.free_head = next_free,
                _ => unreachable!("arena free list points at a slot that is not free"),
            }

            slot.state = SlotState::Live(value);
            return Id::from_parts(index, slot.generation);
        }

        let index = u32::try_from(self.slots.len()).expect("arena index overflow");
        assert!(index != NO_FREE, "arena index overflow");
        self.slots.push(Slot {
            generation: 0,
            state: SlotState::Live(value),
        });
        Id::from_parts(index, 0)
    }

    /// Remove the item of `id` and return it; `None` for a stale handle. The slot's generation
    /// moves on, so `id` (and every copy of it) resolves to `None` from now on.
    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let slot = self.slots.get_mut(id.index() as usize)?;

        if slot.generation != id.generation() || !matches!(slot.state, SlotState::Live(_)) {
            return None;
        }

        let old = if slot.generation == u16::MAX {
            std::mem::replace(&mut slot.state, SlotState::Retired)
        } else {
            slot.generation += 1;
            let next_free = self.free_head;
            self.free_head = id.index();
            std::mem::replace(&mut slot.state, SlotState::Free { next_free })
        };

        self.live -= 1;

        match old {
            SlotState::Live(value) => Some(value),
            _ => None,
        }
    }

    /// True when `id` names a live item of this arena.
    pub fn contains(&self, id: Id<T>) -> bool {
        self.get(id).is_some()
    }

    /// The item of `id`; `None` for a stale handle (generation mismatch, freed or retired slot).
    pub fn get(&self, id: Id<T>) -> Option<&T> {
        let slot = self.slots.get(id.index() as usize)?;

        match &slot.state {
            SlotState::Live(value) if slot.generation == id.generation() => Some(value),
            _ => None,
        }
    }

    /// Mutable access to the item of `id`; `None` for a stale handle.
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(id.index() as usize)?;

        match &mut slot.state {
            SlotState::Live(value) if slot.generation == id.generation() => Some(value),
            _ => None,
        }
    }

    /// Live items in **slot order** (bookkeeping only: invariant checks, bulk passes whose
    /// result does not depend on order). Semantic iteration uses the board's order lists.
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match &slot.state {
                SlotState::Live(value) => {
                    Some((Id::from_parts(index as u32, slot.generation), value))
                }
                _ => None,
            })
    }

    /// Mutable form of [`Arena::iter`], same order and caveat.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id<T>, &mut T)> + '_ {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| {
                let generation = slot.generation;
                match &mut slot.state {
                    SlotState::Live(value) => {
                        Some((Id::from_parts(index as u32, generation), value))
                    }
                    _ => None,
                }
            })
    }

    /// Two distinct live items at once (footprint cascades, group edits); `None` if either
    /// handle is stale or both are the same.
    pub fn get2_mut(&mut self, a: Id<T>, b: Id<T>) -> Option<(&mut T, &mut T)> {
        if a.index() == b.index() || !self.contains(a) || !self.contains(b) {
            return None;
        }

        let (lo, hi, swapped) = if a.index() < b.index() {
            (a, b, false)
        } else {
            (b, a, true)
        };
        let (head, tail) = self.slots.split_at_mut(hi.index() as usize);

        let first = match &mut head[lo.index() as usize].state {
            SlotState::Live(value) => value,
            _ => return None,
        };
        let second = match &mut tail[0].state {
            SlotState::Live(value) => value,
            _ => return None,
        };

        Some(if swapped {
            (second, first)
        } else {
            (first, second)
        })
    }
}

impl<T: fmt::Debug> fmt::Debug for Arena<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_order_while_nothing_freed() {
        let mut arena = Arena::new();
        let ids: Vec<_> = (0..5).map(|i| arena.insert(i)).collect();

        for (n, id) in ids.iter().enumerate() {
            assert_eq!(id.index() as usize, n);
            assert_eq!(id.generation(), 0);
        }

        assert_eq!(arena.len(), 5);
    }

    #[test]
    fn stale_handle_resolves_to_none_and_slot_is_reused() {
        let mut arena = Arena::new();
        let a = arena.insert("a");
        let b = arena.insert("b");

        assert_eq!(arena.remove(a), Some("a"));
        assert_eq!(arena.get(a), None);
        assert_eq!(arena.remove(a), None);

        let c = arena.insert("c");
        assert_eq!(c.index(), a.index());
        assert_eq!(c.generation(), 1);
        assert_eq!(arena.get(a), None);
        assert_eq!(arena.get(c), Some(&"c"));
        assert_eq!(arena.get(b), Some(&"b"));
    }

    #[test]
    fn exhausted_generation_retires_the_slot() {
        let mut arena = Arena::new();
        let mut id = arena.insert(0u8);

        for _ in 0..u16::MAX {
            arena.remove(id).unwrap();
            id = arena.insert(0u8);
            assert_eq!(id.index(), 0);
        }

        assert_eq!(id.generation(), u16::MAX);
        arena.remove(id).unwrap();

        let next = arena.insert(1u8);
        assert_eq!(next.index(), 1);
        assert_eq!(arena.get(id), None);
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn get2_mut_and_clone() {
        let mut arena = Arena::new();
        let a = arena.insert(1);
        let b = arena.insert(2);

        let (x, y) = arena.get2_mut(b, a).unwrap();
        assert_eq!((*x, *y), (2, 1));
        *x = 20;
        assert!(arena.get2_mut(a, a).is_none());

        let copy = arena.clone();
        assert_eq!(copy.get(b), Some(&20));
        assert_eq!(copy.iter().count(), 2);
    }
}
