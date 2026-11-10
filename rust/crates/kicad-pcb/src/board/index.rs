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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;

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
        let found = self.visit_scan_items(|id| {
            if self.uuid_of(id) == Some(*uuid) {
                ControlFlow::Break(id)
            } else {
                ControlFlow::Continue(())
            }
        });

        if let ControlFlow::Break(id) = found {
            return Some(id);
        }

        self.net_info
            .nets
            .values()
            .find(|net| net.base.uuid == *uuid)
            .and_then(|net| ItemId::net(net.net_code))
    }

    /// The item walk of the `BOARD::ResolveItem` linear scan, in its order: groups,
    /// constraints, generators, tracks; per footprint the footprint, its pads, fields,
    /// drawings (`findTableCell`: a table's cells before the table), zones, groups,
    /// constraints, points and bodies; board zones; board drawings (cells first); markers;
    /// points. The nets are not part of it (see [`Board::resolve_item_scan`]). `visit` may
    /// stop the walk with `Break`. [`UuidPresence`] relies on this being the one walk the
    /// scan does.
    fn visit_scan_items<B>(
        &self,
        mut visit: impl FnMut(ItemId) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        macro_rules! scan {
            ($iter:expr) => {
                for id in $iter {
                    visit(id.into())?;
                }
            };
        }

        // findTableCell( drawing, aID ), then the drawing itself.
        macro_rules! scan_drawings {
            ($iter:expr) => {
                for &drawing in $iter {
                    if let Some(AnyItem::Table(table)) = self.get(drawing) {
                        scan!(table.cells.iter().copied());
                    }

                    visit(drawing)?;
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
            scan_drawings!(&fp.drawings);
            scan!(fp.zones.iter().copied());
            scan!(fp.groups.iter().copied());
            scan!(fp.constraints.iter().copied());
            scan!(fp.points.iter().copied());
            scan!(fp.bodies.iter().copied());
        }

        scan!(self.zone_list.iter().copied());
        scan_drawings!(&self.drawing_list);
        scan!(self.marker_list.iter().copied());
        scan!(self.point_list.iter().copied());

        ControlFlow::Continue(())
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

/// A snapshot of which UUIDs [`Board::resolve_item`] can resolve, for a caller that asks
/// "does this UUID resolve?" many times while only adding items whose UUIDs it reports with
/// [`UuidPresence::note_added`] (the DRC job drawing marker UUIDs). It answers exactly
/// `resolve_item(uuid).is_some()` without one linear scan per question.
///
/// Invariant: between [`UuidPresence::new`] and a [`UuidPresence::resolves`] call the board
/// must not change except by adding items whose UUIDs were passed to `note_added`, and the
/// UUIDs of existing items must not change. The index branch is evaluated live, as
/// `resolve_item` does. The containers are only used for membership, never iterated.
#[derive(Debug, Clone, Default)]
pub struct UuidPresence {
    /// `uuid_of( id )` of every item of the scan walk ([`Board::visit_scan_items`]) and of
    /// every item noted since.
    scanned: HashSet<Kiid>,
    /// The scan's net tail: for the first net with a UUID (in `net_info.nets` order), whether
    /// it has a handle (`ItemId::net`).
    nets: HashMap<Kiid, bool>,
}

impl UuidPresence {
    /// One walk over the items and nets the `BOARD::ResolveItem` scan visits.
    pub fn new(board: &Board) -> Self {
        let mut scanned = HashSet::new();
        let _ = board.visit_scan_items(|id| {
            if let Some(uuid) = board.uuid_of(id) {
                scanned.insert(uuid);
            }

            ControlFlow::<()>::Continue(())
        });

        let mut nets = HashMap::new();

        for net in board.net_info.nets.values() {
            // The scan stops at the first net carrying the UUID.
            nets.entry(net.base.uuid)
                .or_insert_with(|| ItemId::net(net.net_code).is_some());
        }

        UuidPresence { scanned, nets }
    }

    /// `board.resolve_item( uuid ).is_some()` for the board this snapshot was taken of (see
    /// the type's invariant).
    pub fn resolves(&self, board: &Board, uuid: &Kiid) -> bool {
        if *uuid == NILUUID {
            return false;
        }

        // The index branch of `resolve_item`.
        if let Some(&id) = board.uuid_index.get(uuid) {
            if board.uuid_of(id) == Some(*uuid) {
                return true;
            }
        }

        self.scanned.contains(uuid) || self.nets.get(uuid).copied().unwrap_or(false)
    }

    /// Record an item added to the board after the snapshot (for example a marker appended
    /// to `marker_list`, which the scan visits).
    pub fn note_added(&mut self, uuid: Kiid) {
        self.scanned.insert(uuid);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kicad_core::{Kiid, NILUUID};
    use kicad_geom::Vector2I;

    use super::UuidPresence;
    use crate::board::Board;
    use crate::ids::{ItemId, ItemKind, NetCode};
    use crate::netinfo::NetinfoItem;
    use crate::pcb_marker::{PcbMarker, RcItem};
    use crate::test_support;

    /// `resolves` agrees with `resolve_item( .. ).is_some()` for every UUID of `uuids`.
    fn assert_agrees(presence: &UuidPresence, board: &Board, uuids: &[Kiid]) {
        for uuid in uuids {
            assert_eq!(
                presence.resolves(board, uuid),
                board.resolve_item(uuid).is_some(),
                "uuid {uuid:?}"
            );
        }
    }

    #[test]
    fn uuid_presence_matches_resolve_item() {
        let mut fx = test_support::rotated_flipped_scaled();
        let pad = fx.child(ItemKind::Pad);
        let shape = fx.child(ItemKind::Shape);
        let text = fx.child(ItemKind::Text);
        let cell = fx.child(ItemKind::Tablecell);
        let zone = fx.child(ItemKind::Zone);
        let board = &mut fx.board;
        let uuid = |board: &Board, id: ItemId| board.uuid_of(id).unwrap();

        // An indexed item: the pad keeps its entry.
        let pad_uuid = uuid(board, pad);

        // A footprint child only the scan finds.
        let zone_uuid = uuid(board, zone);
        board.unbind_uuid(&zone_uuid);

        // A table cell only the scan finds (findTableCell).
        let cell_uuid = uuid(board, cell);
        board.unbind_uuid(&cell_uuid);

        // A duplicate UUID whose index entry was dropped.
        let dup_uuid = uuid(board, shape);
        board.set_uuid_direct(text, dup_uuid);
        board.unbind_uuid(&dup_uuid);

        // A stale index entry: the entry names an item that does not carry the UUID.
        let stale_uuid = Kiid::new(&mut board.kiid_generator);
        board.bind_uuid(stale_uuid, pad);

        // Net UUIDs: the first net carrying a UUID decides, even when it has no handle.
        let net_uuid = Kiid::new(&mut board.kiid_generator);
        let shadowed_net_uuid = Kiid::new(&mut board.kiid_generator);
        let mut net = |code: i32, uuid: Kiid| {
            let mut net = NetinfoItem {
                net_code: NetCode(code),
                ..NetinfoItem::default()
            };
            net.base.uuid = uuid;
            board.net_info.nets.insert(NetCode(code), net);
        };
        net(-1, shadowed_net_uuid);
        net(3, net_uuid);
        net(7, shadowed_net_uuid);

        let unknown = Kiid::new(&mut board.kiid_generator);
        let mut uuids = vec![
            pad_uuid,
            zone_uuid,
            cell_uuid,
            dup_uuid,
            stale_uuid,
            net_uuid,
            shadowed_net_uuid,
            NILUUID,
            unknown,
            board.uuid(),
        ];
        uuids.extend(fx.children.iter().map(|&id| uuid(&fx.board, id)));
        uuids.push(uuid(&fx.board, fx.footprint.into()));

        let board = &mut fx.board;
        assert!(board.resolve_item(&zone_uuid).is_some());
        assert!(board.resolve_item(&cell_uuid).is_some());
        assert!(board.resolve_item(&dup_uuid).is_some());
        assert!(board.resolve_item(&stale_uuid).is_none());
        assert!(board.resolve_item(&net_uuid).is_some());
        assert!(board.resolve_item(&shadowed_net_uuid).is_none());

        let mut presence = UuidPresence::new(board);
        assert_agrees(&presence, board, &uuids);

        // A marker added after the snapshot and noted.
        let marker = PcbMarker::new(
            &mut board.kiid_generator,
            Arc::new(RcItem::default()),
            Vector2I::new(0, 0),
            0,
        );
        let marker_uuid = marker.base.uuid;
        assert!(!presence.resolves(board, &marker_uuid));
        board.add_marker(marker);
        presence.note_added(marker_uuid);
        uuids.push(marker_uuid);
        assert!(board.resolve_item(&marker_uuid).is_some());
        assert_agrees(&presence, board, &uuids);

        // The same marker once the index forgot it: only the scan (and the note) find it.
        board.unbind_uuid(&marker_uuid);
        assert_agrees(&presence, board, &uuids);
    }
}
