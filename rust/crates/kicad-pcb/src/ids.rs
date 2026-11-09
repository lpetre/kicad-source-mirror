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

//! Item handles (new code, `board-model.md` section 3, `board-model-types.md` section 0):
//! the typed `Id<T>`, the untyped `ItemId` (the port of a stored `BOARD_ITEM*`), `ItemKind`
//! and `NetCode`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Implemented (by `board_items!`) for every arena item type: which `ItemKind` its handles
/// carry.
pub trait ArenaItem {
    /// The arena kind of this type.
    const KIND: ItemKind;
}

/// Typed handle into an `Arena<T>`. 8 bytes, `Copy`, no lifetime. Stays valid until its own
/// item is removed; a stale handle resolves to `None`, never to another item.
pub struct Id<T> {
    index: u32,
    generation: u16,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// Handle from its raw parts (arena and conversions only).
    pub(crate) const fn from_parts(index: u32, generation: u16) -> Self {
        Id {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    /// Slot index in the arena.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when the handle was handed out.
    pub const fn generation(self) -> u16 {
        self.generation
    }
}

impl<T: ArenaItem> Id<T> {
    /// The untyped form.
    pub const fn item_id(self) -> ItemId {
        ItemId {
            kind: T::KIND,
            generation: self.generation,
            index: self.index,
        }
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    /// Same order as the `ItemId` of the same kind: generation, then index.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.generation, self.index).cmp(&(other.generation, other.index))
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({}v{})", self.index, self.generation)
    }
}

/// Untyped handle = stored `BOARD_ITEM*`. 8 bytes, totally ordered: `Ord` compares kind, then
/// generation, then index (field order), which equals the numeric order of `to_bits()` and is
/// the replacement for pointer-address order (`board-model.md` 3.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ItemId {
    kind: ItemKind,
    generation: u16,
    index: u32,
}

impl ItemId {
    /// The handle's kind.
    pub const fn kind(self) -> ItemKind {
        self.kind
    }

    /// The handle's slot index (the net code for `ItemKind::Net`).
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The handle's generation.
    pub const fn generation(self) -> u16 {
        self.generation
    }

    /// The `ItemKind::Net` form for a net code (`ResolveItem` returning a `NETINFO_ITEM`).
    /// `None` for negative codes (`NetCode::ORPHANED` is no net).
    pub fn net(code: NetCode) -> Option<ItemId> {
        u32::try_from(code.0).ok().map(|index| ItemId {
            kind: ItemKind::Net,
            generation: 0,
            index,
        })
    }

    /// `index | generation << 32 | kind << 48`; always < 2^53 (exact as a JS `number`).
    pub const fn to_bits(self) -> u64 {
        self.index as u64 | (self.generation as u64) << 32 | (self.kind as u64) << 48
    }

    /// Inverse of [`ItemId::to_bits`]; `None` for an unknown kind or a value >= 2^53.
    pub fn from_bits(bits: u64) -> Option<ItemId> {
        if bits >= 1 << 53 {
            return None;
        }

        let kind = ItemKind::from_u8((bits >> 48) as u8)?;
        Some(ItemId {
            kind,
            generation: (bits >> 32) as u16,
            index: bits as u32,
        })
    }

    /// The typed handle if the kind matches.
    pub fn typed<T: ArenaItem>(self) -> Option<Id<T>> {
        (self.kind == T::KIND).then_some(Id::from_parts(self.index, self.generation))
    }
}

impl<T: ArenaItem> From<Id<T>> for ItemId {
    fn from(id: Id<T>) -> Self {
        id.item_id()
    }
}

/// Net code as returned by `BOARD_CONNECTED_ITEM::GetNetCode()`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct NetCode(pub i32);

impl NetCode {
    /// `NETINFO_LIST::UNCONNECTED`; also what a new item has (C++ `OrphanedItem()` carries net
    /// code 0).
    pub const UNCONNECTED: NetCode = NetCode(0);
    /// `NETINFO_LIST::ORPHANED`; `GetNetCode()` of a null `m_netinfo` (`SetNetCode` with an
    /// unknown code).
    pub const ORPHANED: NetCode = NetCode(-1);
}

board_items!(gen_ids);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::Pad;
    use crate::pcb_track::Track;

    #[test]
    fn bits_round_trip_and_order() {
        let a: ItemId = Id::<Track>::from_parts(7, 0).into();
        let b: ItemId = Id::<Track>::from_parts(3, 1).into();
        let c: ItemId = Id::<Pad>::from_parts(u32::MAX, u16::MAX).into();

        for id in [a, b, c] {
            assert!(id.to_bits() < 1 << 53);
            assert_eq!(ItemId::from_bits(id.to_bits()), Some(id));
        }

        // Ord = (kind, generation, index) = numeric order of to_bits().
        let mut ids = vec![a, b, c];
        ids.sort();
        assert_eq!(ids, vec![c, a, b]);
        assert!(ids.windows(2).all(|w| w[0].to_bits() < w[1].to_bits()));

        assert_eq!(ItemId::from_bits(1 << 53), None);
        assert_eq!(ItemId::from_bits(0), None);
        assert_eq!(a.typed::<Track>(), Some(Id::from_parts(7, 0)));
        assert_eq!(a.typed::<Pad>(), None);
        assert_eq!(ItemId::net(NetCode::ORPHANED), None);
        assert_eq!(
            ItemId::net(NetCode(5)).map(|n| (n.kind(), n.index())),
            Some((ItemKind::Net, 5))
        );
    }

    #[test]
    fn item_kind_values() {
        assert_eq!(ItemKind::ALL.len(), 23);
        for (n, kind) in ItemKind::ALL.iter().enumerate() {
            assert_eq!(*kind as u8 as usize, n + 1);
            assert_eq!(ItemKind::from_u8(*kind as u8), Some(*kind));
        }
    }
}
