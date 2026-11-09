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

//! Port of `pcbnew/board_item_index.cpp` (`BOARD_ITEM_INDEX`: UUID binding) and
//! `BOARD::ResolveItem`. Package M1; M0a wrote `bind_uuid` (what `add_track` needs) and the
//! lookup.

use kicad_core::{Kiid, NILUUID};

use crate::board::Board;
use crate::ids::ItemId;
use crate::item::BoardUse;

impl Board {
    /// `BOARD::ResolveItem( aID )` as a handle; `None` for the nil UUID or an unknown one.
    pub fn resolve_item(&self, uuid: &Kiid) -> Option<ItemId> {
        if *uuid == NILUUID {
            return None;
        }

        self.uuid_index.get(uuid).copied()
    }

    /// `BOARD_ITEM_INDEX::BindItem`: the later bind of a duplicate UUID wins; a footprint
    /// holder board indexes nothing.
    pub(crate) fn bind_uuid(&mut self, uuid: Kiid, id: ItemId) {
        if self.board_use == BoardUse::FpHolder {
            return;
        }

        self.uuid_index.insert(uuid, id);
    }

    /// Keep the UUID index consistent after an item's UUID changed from `old` to `new` (the
    /// `BOARD_ITEM_INDEX::Rebind` effect of `SetUuid`): drop `old` if it named this item, then
    /// bind `new`. Items that were never indexed (unattached children) stay unindexed.
    pub(crate) fn rebind_uuid(&mut self, id: ItemId, old: Kiid, new: Kiid) {
        if self.uuid_index.get(&old) != Some(&id) {
            return;
        }

        self.uuid_index.remove(&old);
        self.bind_uuid(new, id);
    }

    /// `BOARD_ITEM::SetUuid` on a stored item (re-binds the UUID index like
    /// `BOARD_ITEM_INDEX::Rebind`).
    pub fn rebind_item_uuid(&mut self, id: ItemId, new_uuid: Kiid) {
        crate::port_pending!("BOARD_ITEM::SetUuid (rebind_item_uuid)", id, new_uuid)
    }

    /// The UUID of a live item (`None` for a stale handle).
    pub fn uuid_of(&self, id: ItemId) -> Option<Kiid> {
        self.get(id).map(|item| item.base().uuid)
    }

    /// `BOARD::RepairDuplicateItemUuids`: returns the number of UUIDs replaced.
    pub fn repair_duplicate_item_uuids(&mut self) -> usize {
        crate::port_pending!("BOARD::RepairDuplicateItemUuids")
    }
}
