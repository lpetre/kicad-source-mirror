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

//! Port of `pcbnew/board_item_index.cpp` (`BOARD_ITEM_INDEX`: UUID binding),
//! `BOARD::ResolveItem`, `BOARD::RepairDuplicateItemUuids`, `BOARD::RebindItemUuid` and
//! `BOARD_ITEM::SetUuid`. Package M1.
//!
//! The C++ index keeps a forward map (UUID -> item) and a reverse map (item -> UUID) so that
//! a pointer-keyed unbind never drops another item's entry. Here every stored item carries its
//! current UUID and `modify_*` re-binds a changed one, so the forward map alone answers both:
//! "the reverse entry of `id`" is "the forward entry of `id`'s UUID, if it names `id`".

use std::collections::BTreeSet;

use kicad_core::{Kiid, NILUUID};

use crate::board::Board;
use crate::footprint::Footprint;
use crate::ids::{ItemId, ItemKind};
use crate::item::BoardUse;
use crate::item_ref::AnyItem;
use crate::pcb_field::FieldT;

impl Board {
    /// `BOARD::ResolveItem( aID, aAllowNullptrReturn = true )` as a handle; `None` for the nil
    /// UUID or an unknown one. The index is tried first (an entry whose item no longer
    /// carries the UUID is ignored, `BOARD_ITEM_INDEX::Find`), then the C++ linear scan in its
    /// order. C++ caches a scan hit; this `&self` lookup does not, which changes only the cost
    /// (a later bind still wins over a scan result, as in C++). The board's own UUID and
    /// `DELETED_BOARD_ITEM` have no handle.
    pub fn resolve_item(&self, uuid: &Kiid) -> Option<ItemId> {
        if *uuid == NILUUID {
            return None;
        }

        if let Some(&id) = self.uuid_index.get(uuid) {
            if self.uuid_of(id) == Some(*uuid) {
                return Some(id);
            }
        }

        self.resolve_item_scan(uuid)
    }

    /// The linear-scan fallback of `BOARD::ResolveItem` (groups, constraints, generators,
    /// tracks, footprints with their children, zones, drawings, markers, points, nets).
    fn resolve_item_scan(&self, uuid: &Kiid) -> Option<ItemId> {
        let hit = |id: ItemId| self.uuid_of(id) == Some(*uuid);

        // findTableCell( drawing, aID ), then the drawing itself.
        let drawing_hit = |id: ItemId| -> Option<ItemId> {
            if let Some(AnyItem::Table(table)) = self.get(id) {
                if let Some(&cell) = table.cells.iter().find(|&&c| hit(c.into())) {
                    return Some(cell.into());
                }
            }

            hit(id).then_some(id)
        };

        macro_rules! scan {
            ($iter:expr) => {
                for id in $iter {
                    let id: ItemId = id.into();

                    if hit(id) {
                        return Some(id);
                    }
                }
            };
        }

        scan!(self.group_list.iter().copied());
        scan!(self.constraint_list.iter().copied());
        scan!(self.generator_list.iter().copied());
        scan!(self.track_list.iter().copied());

        for &fp_id in &self.footprint_list {
            let Some(fp) = self.footprints.get(fp_id) else {
                continue;
            };

            scan!(std::iter::once(fp_id));
            scan!(fp.pads.iter().copied());
            scan!(fp.fields.iter().copied());

            for &drawing in &fp.drawings {
                if let Some(found) = drawing_hit(drawing) {
                    return Some(found);
                }
            }

            scan!(fp.zones.iter().copied());
            scan!(fp.groups.iter().copied());
            scan!(fp.constraints.iter().copied());
            scan!(fp.points.iter().copied());
            scan!(fp.bodies.iter().copied());
        }

        scan!(self.zone_list.iter().copied());

        for &drawing in &self.drawing_list {
            if let Some(found) = drawing_hit(drawing) {
                return Some(found);
            }
        }

        scan!(self.marker_list.iter().copied());
        scan!(self.point_list.iter().copied());

        self.net_info
            .nets
            .values()
            .find(|net| net.base.uuid == *uuid)
            .and_then(|net| ItemId::net(net.net_code))
    }

    /// `BOARD::IsItemIndexedById( aId )`: a forward index entry exists for `uuid` (it may name
    /// another item carrying that UUID).
    pub fn is_item_indexed_by_id(&self, uuid: &Kiid) -> bool {
        self.uuid_index.contains_key(uuid)
    }

    /// `BOARD::ClearItemByIdCache` (`BOARD_ITEM_INDEX::Clear`).
    pub(crate) fn clear_item_by_id_cache(&mut self) {
        self.uuid_index.clear();
    }

    /// `BOARD_ITEM_INDEX::BindItem`: the later bind of a duplicate UUID wins; a footprint
    /// holder board indexes nothing.
    pub(crate) fn bind_uuid(&mut self, uuid: Kiid, id: ItemId) {
        if self.board_use == BoardUse::FpHolder {
            return;
        }

        self.uuid_index.insert(uuid, id);
    }

    /// `BOARD_ITEM_INDEX::BindItem( item )` by handle (`CacheItemById`).
    pub(crate) fn bind_item(&mut self, id: ItemId) {
        if let Some(uuid) = self.uuid_of(id) {
            self.bind_uuid(uuid, id);
        }
    }

    /// `BOARD_ITEM_INDEX::Unbind( aId )`: drops the entry of `uuid` whichever item it names.
    pub(crate) fn unbind_uuid(&mut self, uuid: &Kiid) {
        self.uuid_index.remove(uuid);
    }

    /// `BOARD_ITEM_INDEX::UnbindItem( item )`: pointer-keyed, an entry of another item with the
    /// same UUID is left alone.
    pub(crate) fn unbind_item(&mut self, id: ItemId) {
        if let Some(uuid) = self.uuid_of(id) {
            if self.uuid_index.get(&uuid) == Some(&id) {
                self.uuid_index.remove(&uuid);
            }
        }
    }

    /// `BOARD_ITEM_INDEX::BindSubtree` (`CacheItemSubtreeById`): the item, then each child of
    /// `RunOnChildren( NO_RECURSE )` with its own subtree (group members included, as C++).
    pub(crate) fn bind_subtree(&mut self, id: ItemId) {
        self.bind_item(id);
        self.bind_children(id);
    }

    /// `BOARD_ITEM_INDEX::BindChildren` (`CacheChildrenById`).
    pub(crate) fn bind_children(&mut self, id: ItemId) {
        for child in self.direct_children(id) {
            self.bind_subtree(child);
        }
    }

    /// `BOARD_ITEM_INDEX::UnbindSubtree` (`UncacheItemSubtreeById`), pointer-keyed.
    pub(crate) fn unbind_subtree(&mut self, id: ItemId) {
        self.unbind_item(id);
        self.unbind_children(id);
    }

    /// `BOARD_ITEM_INDEX::UnbindChildren` (`UncacheChildrenById`), pointer-keyed.
    pub(crate) fn unbind_children(&mut self, id: ItemId) {
        for child in self.direct_children(id) {
            self.unbind_subtree(child);
        }
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

    /// `SetUuidDirect` on a stored item.
    fn set_uuid_direct(&mut self, id: ItemId, uuid: Kiid) {
        if let Some(mut item) = self.get_mut_raw(id) {
            item.as_dyn_mut().base_mut().uuid = uuid;
        }
    }

    /// `BOARD::RebindItemUuid` = `BOARD_ITEM_INDEX::Rebind( aItem, aNewId )`: nothing on a
    /// footprint holder; the same UUID re-binds; a UUID another indexed item holds is refused
    /// (wxFAIL); otherwise unbind (pointer-keyed), set, bind.
    pub(crate) fn rebind_item_uuid_index(&mut self, id: ItemId, new_uuid: Kiid) {
        if self.board_use == BoardUse::FpHolder {
            return;
        }

        let Some(current) = self.uuid_of(id) else {
            return;
        };

        if current == new_uuid {
            self.bind_item(id);
            return;
        }

        if let Some(&existing) = self.uuid_index.get(&new_uuid) {
            // Find() verifies the entry; a stale alias is dropped, a live one refuses.
            if self.uuid_of(existing) == Some(new_uuid) {
                if existing != id {
                    return;
                }
            } else {
                self.uuid_index.remove(&new_uuid);
            }
        }

        self.unbind_item(id);
        self.set_uuid_direct(id, new_uuid);
        self.bind_item(id);
    }

    /// `BOARD_ITEM::SetUuid` on a stored item: an indexed item is moved with
    /// `BOARD_ITEM_INDEX::Rebind`, any other gets the UUID directly. Bumps the time stamp.
    pub fn rebind_item_uuid(&mut self, id: ItemId, new_uuid: Kiid) {
        let Some(current) = self.uuid_of(id) else {
            return;
        };

        if current == new_uuid {
            return;
        }

        if self.uuid_index.get(&current) == Some(&id) {
            self.rebind_item_uuid_index(id, new_uuid);
        } else {
            self.set_uuid_direct(id, new_uuid);
        }

        self.increment_time_stamp();
    }

    /// The UUID of a live item (`None` for a stale handle).
    pub fn uuid_of(&self, id: ItemId) -> Option<Kiid> {
        self.get(id).map(|item| item.base().uuid)
    }

    /// `BOARD::RepairDuplicateItemUuids`: returns the number of UUIDs replaced. Footprints
    /// claim first, then their pads, then tracks, then per footprint the reference and value
    /// fields, graphics, bodies, zones and groups, then the rest of `GetItemSet()`. A
    /// duplicate gets a new UUID from `kiid_generator` (C++ `KIID()`); on a footprint holder
    /// the UUID is counted but not changed, as `Rebind` does nothing there.
    pub fn repair_duplicate_item_uuids(&mut self) -> usize {
        let mut ids: BTreeSet<Kiid> = BTreeSet::new();
        let mut duplicates = 0;

        let mut process = |board: &mut Board, id: ItemId| {
            let Some(uuid) = board.uuid_of(id) else {
                return;
            };

            if ids.contains(&uuid) {
                duplicates += 1;
                let new_uuid = Kiid::new(&mut board.kiid_generator);
                board.rebind_item_uuid_index(id, new_uuid);
            }

            if let Some(current) = board.uuid_of(id) {
                ids.insert(current);
            }
        };

        // Footprint IDs are the most important, so give them the first crack at "claiming" a
        // particular KIID.
        let footprints: Vec<_> = self.footprint_list.iter().copied().collect();

        for &fp in &footprints {
            process(self, fp.into());
        }

        // After that the principal use is for DRC marker pointers, which are most likely to
        // pads or tracks.
        for &fp in &footprints {
            let pads: Vec<ItemId> = self[fp].pads.iter().map(|&p| p.into()).collect();

            for pad in pads {
                process(self, pad);
            }
        }

        let tracks: Vec<ItemId> = self.track_list.iter().map(|&t| t.into()).collect();

        for track in tracks {
            process(self, track);
        }

        // From here out I don't think order matters much.
        for &fp in &footprints {
            let footprint: &Footprint = &self[fp];
            let field = |id: FieldT| {
                footprint
                    .fields
                    .iter()
                    .copied()
                    .find(|&f| self.fields.get(f).is_some_and(|x| x.id == id))
                    .map(ItemId::from)
            };
            let mut items: Vec<ItemId> = Vec::new();
            items.extend(field(FieldT::Reference));
            items.extend(field(FieldT::Value));
            items.extend(footprint.drawings.iter().copied());
            items.extend(footprint.bodies.iter().map(|&b| ItemId::from(b)));
            items.extend(footprint.zones.iter().map(|&z| ItemId::from(z)));
            items.extend(footprint.groups.iter().map(|&g| ItemId::from(g)));

            for item in items {
                process(self, item);
            }
        }

        // Everything owned by the board not handled above.
        for item in self.get_item_set() {
            // Top-level footprints and tracks were handled above.
            match item.kind() {
                ItemKind::Footprint | ItemKind::Track => {}
                _ => process(self, item),
            }
        }

        if duplicates > 0 {
            self.increment_time_stamp();
        }

        duplicates
    }
}
