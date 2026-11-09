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

//! Port of the container part of `pcbnew/board.cpp` (`BOARD::Add`, `Remove`, `RemoveAll`,
//! `RunOnChildren`, `AllConnectedItems`, `collectOwnedItems`, `GetItemSet`, `GetPads`), of
//! `FOOTPRINT::FOOTPRINT` (the four mandatory fields) / `FOOTPRINT::Add` / `FOOTPRINT::Remove` /
//! `FOOTPRINT::RunOnChildren`, `PCB_TABLE::AddCell` / `PCB_TABLE::RunOnChildren`,
//! `PCB_GROUP::RunOnChildren` (by handle) and `common/eda_group.cpp`
//! (`EDA_GROUP::AddItem` / `RemoveItem` / `ContainsItem`) as create / fill / attach
//! (`board-model.md` 8.1). Package M1.

use std::collections::{BTreeSet, HashSet};

use kicad_core::KicadT;
use kicad_core::layer_ids::is_copper_layer;

use crate::board::journal::ChangeKind;
use crate::board::{ArenaOf, Board};
use crate::ctx::MutEnv;
use crate::footprint::Footprint;
use crate::ids::{ArenaItem, FootprintId, Id, ItemId, ItemKind, TableId, TablecellId, TrackId};
use crate::item::{ItemFlags, ParentRef, RecurseMode};
use crate::item_ref::{AnyItem, AnyItemMut, OwnedItem};
use crate::pcb_field::{FieldT, PcbField};
use crate::pcb_table::PcbTable;
use crate::pcb_tablecell::PcbTablecell;
use crate::pcb_track::Track;
use crate::traits::BoardItem;

/// `ADD_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum AddMode {
    /// `ADD_MODE::INSERT` (the C++ default): front of the deques.
    #[default]
    Insert,
    Append,
    BulkAppend,
    BulkInsert,
}

impl AddMode {
    /// `aMode == ADD_MODE::APPEND || aMode == ADD_MODE::BULK_APPEND` (the `BOARD::Add` test;
    /// `FOOTPRINT::Add` appends only for exactly `APPEND`).
    pub fn is_append(self) -> bool {
        matches!(self, AddMode::Append | AddMode::BulkAppend)
    }
}

/// `REMOVE_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum RemoveMode {
    #[default]
    Normal,
    Bulk,
}

/// `Board::arena_remove`: frees one arena slot of any kind (generated from the item table).
macro_rules! gen_arena_remove {
    ( { } $( $kind:ident = $disc:literal, $handle:ident, $arena:ident, $ty:ty, $getter:ident, $modify:ident; )* ) => {
        impl Board {
            /// Free the arena slot of `id` (no order list, index or group bookkeeping).
            fn arena_remove(&mut self, id: ItemId) -> bool {
                match id.kind() {
                    $( ItemKind::$kind => id
                        .typed::<$ty>()
                        .and_then(|typed| self.$arena.remove(typed))
                        .is_some(), )*
                    ItemKind::Net => false,
                }
            }
        }
    };
}

board_items!(gen_arena_remove);

/// Push `id` to the front of `list` unless `append` (`push_back` / `push_front`, or
/// `insert( begin() )` for the footprint vectors: same effect on a `VecDeque`).
fn push_mode<T>(list: &mut std::collections::VecDeque<T>, id: T, append: bool) {
    if append {
        list.push_back(id);
    } else {
        list.push_front(id);
    }
}

impl Board {
    // ---- BOARD::Add for leaf items ---------------------------------------------------------

    /// The common head of `BOARD::Add` for a leaf: `ClearEditFlags()`, store, `SetParent(
    /// this )` (the order list is pushed by the caller).
    fn insert_board_leaf<T: ArenaItem + BoardItem>(&mut self, mut item: T) -> Id<T>
    where
        Board: ArenaOf<T>,
    {
        item.base_mut().flags.clear(ItemFlags::EDIT_FLAGS);
        self.insert_item(item, ParentRef::Board)
    }

    /// The common tail of `BOARD::Add`: `CacheItemById`, `CacheChildrenById` for footprints
    /// and tables, `InvokeListeners( OnBoardItemAdded )` (the journal), time stamp.
    fn after_board_add(&mut self, id: ItemId) {
        self.bind_item(id);

        if matches!(id.kind(), ItemKind::Footprint | ItemKind::Table) {
            self.bind_children(id);
        }

        self.increment_time_stamp();
        self.journal_record(ChangeKind::Added, id);
    }

    /// `BOARD::Add( track, aMode )` for `PCB_TRACE_T`, `PCB_ARC_T` and `PCB_VIA_T`: `push_back`
    /// for `APPEND` / `BULK_APPEND`, else `push_front`; parent = board, edit flags cleared,
    /// `base.id` stamped, UUID bound, time stamp bumped. A segment or arc on a non-copper layer
    /// is refused like C++ ("Cannot place Track on non-copper layer") and handed back.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn add_track(&mut self, track: Track, mode: AddMode) -> Result<TrackId, Track> {
        if !matches!(track, Track::Via(_)) && !is_copper_layer(track.base().layer) {
            return Err(track);
        }

        let id = self.insert_board_leaf(track);
        push_mode(&mut self.track_list, id, mode.is_append());
        self.after_board_add(id.into());
        Ok(id)
    }

    /// `BOARD::Add( Zone, aMode )` (m_zones.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_zone(&mut self, item: crate::zone::Zone, _mode: AddMode) -> crate::ids::ZoneId {
        let id = self.insert_board_leaf(item);
        self.zone_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// One of the `m_drawings` cases of `BOARD::Add` (front unless APPEND / BULK_APPEND).
    fn add_drawing<T: ArenaItem + BoardItem>(&mut self, item: T, mode: AddMode) -> Id<T>
    where
        Board: ArenaOf<T>,
    {
        let id = self.insert_board_leaf(item);
        push_mode(&mut self.drawing_list, id.into(), mode.is_append());
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( PcbShape, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_shape(
        &mut self,
        item: crate::pcb_shape::PcbShape,
        mode: AddMode,
    ) -> crate::ids::ShapeId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbText, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_text(
        &mut self,
        item: crate::pcb_text::PcbText,
        mode: AddMode,
    ) -> crate::ids::TextId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbField, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_field(
        &mut self,
        item: crate::pcb_field::PcbField,
        mode: AddMode,
    ) -> crate::ids::FieldId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbTextbox, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_textbox(
        &mut self,
        item: crate::pcb_textbox::PcbTextbox,
        mode: AddMode,
    ) -> crate::ids::TextboxId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbDimension, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_dimension(
        &mut self,
        item: crate::pcb_dimension::PcbDimension,
        mode: AddMode,
    ) -> crate::ids::DimensionId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbBarcode, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_barcode(
        &mut self,
        item: crate::pcb_barcode::PcbBarcode,
        mode: AddMode,
    ) -> crate::ids::BarcodeId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbReferenceImage, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_image(
        &mut self,
        item: crate::pcb_reference_image::PcbReferenceImage,
        mode: AddMode,
    ) -> crate::ids::ImageId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbTarget, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_target(
        &mut self,
        item: crate::pcb_target::PcbTarget,
        mode: AddMode,
    ) -> crate::ids::TargetId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbGridItem, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_grid_item(
        &mut self,
        item: crate::pcb_grid_item::PcbGridItem,
        mode: AddMode,
    ) -> crate::ids::GridItemId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbDrillMap, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_drill_map(
        &mut self,
        item: crate::pcb_drill_map::PcbDrillMap,
        mode: AddMode,
    ) -> crate::ids::DrillMapId {
        self.add_drawing(item, mode)
    }

    /// `BOARD::Add( PcbPoint, aMode )` (m_points.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_point(
        &mut self,
        item: crate::pcb_point::PcbPoint,
        _mode: AddMode,
    ) -> crate::ids::PointId {
        let id = self.insert_board_leaf(item);
        self.point_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( PcbGroup, aMode )` (m_groups.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped. Members are added afterwards
    /// with `add_to_group`.
    pub fn add_group(
        &mut self,
        item: crate::pcb_group::PcbGroup,
        _mode: AddMode,
    ) -> crate::ids::GroupId {
        let id = self.insert_board_leaf(item);
        self.group_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( PcbGenerator, aMode )` (m_generators.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_generator(
        &mut self,
        item: crate::pcb_generator::PcbGenerator,
        _mode: AddMode,
    ) -> crate::ids::GeneratorId {
        let id = self.insert_board_leaf(item);
        self.generator_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( PcbConstraint, aMode )` (m_constraints.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_constraint(
        &mut self,
        item: crate::pcb_constraint::PcbConstraint,
        _mode: AddMode,
    ) -> crate::ids::ConstraintId {
        let id = self.insert_board_leaf(item);
        self.constraint_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( PCB_MARKER )` (`m_markers.push_back`).
    pub fn add_marker(&mut self, marker: crate::pcb_marker::PcbMarker) -> crate::ids::MarkerId {
        let id = self.insert_board_leaf(marker);
        self.marker_list.push(id);
        self.after_board_add(id.into());
        id
    }

    /// `BOARD::Add( aItem, aMode )`, polymorphic: dispatches on the variant; a kind the board
    /// does not accept (pad, cell, 3D body) or a C++ refusal hands the item back. Footprint and
    /// table trees are deferred (`board-model.md` 17): containers are created and attached
    /// with `create_footprint` / `attach_footprint`, `create_table` / `attach_table`, so the
    /// tree variants are handed back as well.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn add(&mut self, item: OwnedItem, mode: AddMode) -> Result<ItemId, OwnedItem> {
        Ok(match item {
            OwnedItem::Track(t) => self.add_track(t, mode).map_err(OwnedItem::Track)?.into(),
            OwnedItem::Zone(z) => self.add_zone(*z, mode).into(),
            OwnedItem::Shape(s) => self.add_shape(*s, mode).into(),
            OwnedItem::Text(t) => self.add_text(*t, mode).into(),
            OwnedItem::Field(f) => self.add_field(*f, mode).into(),
            OwnedItem::Textbox(t) => self.add_textbox(*t, mode).into(),
            OwnedItem::Dimension(d) => self.add_dimension(*d, mode).into(),
            OwnedItem::Barcode(b) => self.add_barcode(*b, mode).into(),
            OwnedItem::Image(i) => self.add_image(*i, mode).into(),
            OwnedItem::Target(t) => self.add_target(t, mode).into(),
            OwnedItem::Point(p) => self.add_point(p, mode).into(),
            OwnedItem::Group(g) => self.add_group(g, mode).into(),
            OwnedItem::Generator(g) => self.add_generator(*g, mode).into(),
            OwnedItem::Marker(m) => self.add_marker(*m).into(),
            OwnedItem::Constraint(c) => self.add_constraint(c, mode).into(),
            OwnedItem::GridItem(g) => self.add_grid_item(*g, mode).into(),
            OwnedItem::DrillMap(d) => self.add_drill_map(*d, mode).into(),
            // "BOARD::Add() item type %s not handled"; trees: deferred.
            other @ (OwnedItem::Pad(_)
            | OwnedItem::Body3d(_)
            | OwnedItem::Footprint(_)
            | OwnedItem::Table(_)) => return Err(other),
        })
    }

    // ---- BOARD::Remove / FOOTPRINT::Remove + delete -----------------------------------------

    /// `BOARD::Remove` / `FOOTPRINT::Remove` + delete: cascades to children, leaves its
    /// group, unbinds UUIDs, time stamp; also discards an unattached container. `false` for
    /// a stale handle.
    pub fn remove(&mut self, id: ItemId) -> bool {
        self.remove_many(&[id]) == 1
    }

    /// `remove` for several items with one retain pass per list (`REMOVE_MODE::BULK` +
    /// `FinalizeBulkRemove`); returns the number removed. A handle that is stale, or that
    /// names an item already removed as a child of an earlier one, is skipped. A net handle
    /// goes to `remove_net`.
    pub fn remove_many(&mut self, ids: &[ItemId]) -> usize {
        let mut count = 0;
        let mut roots: HashSet<ItemId> = HashSet::new();
        let mut parents: BTreeSet<ItemId> = BTreeSet::new();
        let mut board_level = false;
        let mut doomed: Vec<ItemId> = Vec::new();
        let mut doomed_set: HashSet<ItemId> = HashSet::new();

        for &id in ids {
            if id.kind() == ItemKind::Net {
                if let Some(code) = self.get(id).and_then(|item| match item {
                    AnyItem::Net(net) => Some(net.net_code),
                    _ => None,
                }) {
                    self.remove_net(code);
                    count += 1;
                }

                continue;
            }

            if doomed_set.contains(&id) || !self.contains(id) {
                continue;
            }

            count += 1;
            let parent = self.get(id).map_or(ParentRef::None, |item| item.base().parent);
            self.journal_record(ChangeKind::Removed, id);

            match parent {
                ParentRef::Item(owner) if owner.kind() == ItemKind::Footprint => {
                    self.footprint_remove_steps(owner, id);
                    parents.insert(owner);
                }
                ParentRef::Item(owner) => {
                    // PCB_TABLECELL_T: "Handled by parent table" (m_itemIndex.Unbind only);
                    // the deletion is the table's DeleteMarkedCells.
                    if let Some(uuid) = self.uuid_of(id) {
                        self.unbind_uuid(&uuid);
                    }

                    self.increment_time_stamp();
                    parents.insert(owner);
                }
                ParentRef::Board | ParentRef::None => {
                    if self.is_attached_to_board(id) {
                        // BOARD::Remove: m_itemIndex.Unbind( aBoardItem->m_Uuid ) (keyed by
                        // UUID, so it also drops an entry of another item with the same UUID).
                        if let Some(uuid) = self.uuid_of(id) {
                            self.unbind_uuid(&uuid);
                        }

                        if matches!(id.kind(), ItemKind::Footprint | ItemKind::Table) {
                            self.unbind_children(id);
                        }

                        board_level = true;
                        self.increment_time_stamp();
                    } else {
                        // An unattached container: the C++ unique_ptr going out of scope.
                        self.increment_time_stamp();
                    }
                }
            }

            roots.insert(id);
            let start = doomed.len();
            self.owned_subtree(id, &mut doomed);

            for &x in &doomed[start..] {
                doomed_set.insert(x);
            }
        }

        self.erase_from_lists(&roots, &parents, board_level);
        self.free_items(&doomed, &doomed_set);
        count
    }

    /// The bookkeeping part of `FOOTPRINT::Remove( aBoardItem )` (the list erase is done by
    /// `erase_from_lists`): variant field values of a removed field, `UncacheItemSubtreeById`
    /// when the footprint is indexed, `IncrementTimeStamp`, `InvalidateGeometryCaches`.
    fn footprint_remove_steps(&mut self, owner: ItemId, id: ItemId) {
        let Some(fp_id) = owner.typed::<Footprint>() else {
            return;
        };

        if let Some(field_id) = id.typed::<PcbField>() {
            let field_name = self.fields.get(field_id).map(PcbField::untranslated_name);

            if let (Some(field_name), Some(fp)) = (field_name, self.footprints.get_mut(fp_id)) {
                for variant in fp.variants.values_mut() {
                    variant.remove_field_value(&field_name);
                }
            }
        }

        if self.is_footprint_indexed(fp_id) {
            self.unbind_subtree(id);
        }

        self.increment_time_stamp();

        if let Some(fp) = self.footprints.get_mut(fp_id) {
            fp.invalidate_geometry_caches();
        }
    }

    /// True when `id` (a board-parented item) is in one of the board order lists.
    fn is_attached_to_board(&self, id: ItemId) -> bool {
        match id.kind() {
            ItemKind::Footprint => id
                .typed()
                .is_some_and(|f: FootprintId| self.footprint_list.contains(&f)),
            ItemKind::Track => id
                .typed()
                .is_some_and(|t: TrackId| self.track_list.contains(&t)),
            ItemKind::Zone => id.typed().is_some_and(|z| self.zone_list.contains(&z)),
            ItemKind::Group => id.typed().is_some_and(|g| self.group_list.contains(&g)),
            ItemKind::Generator => id
                .typed()
                .is_some_and(|g| self.generator_list.contains(&g)),
            ItemKind::Constraint => id
                .typed()
                .is_some_and(|c| self.constraint_list.contains(&c)),
            ItemKind::Point => id.typed().is_some_and(|p| self.point_list.contains(&p)),
            ItemKind::Marker => id.typed().is_some_and(|m| self.marker_list.contains(&m)),
            _ => self.drawing_list.contains(&id),
        }
    }

    /// `id` and everything it owns (footprint children, table cells), parents first.
    fn owned_subtree(&self, id: ItemId, out: &mut Vec<ItemId>) {
        out.push(id);

        match id.kind() {
            ItemKind::Footprint => {
                let Some(fp) = id.typed().and_then(|f| self.footprints.get(f)) else {
                    return;
                };
                let children: Vec<ItemId> = fp
                    .fields
                    .iter()
                    .map(|&c| c.into())
                    .chain(fp.pads.iter().map(|&c| c.into()))
                    .chain(fp.zones.iter().map(|&c| c.into()))
                    .chain(fp.groups.iter().map(|&c| c.into()))
                    .chain(fp.bodies.iter().map(|&c| c.into()))
                    .chain(fp.constraints.iter().map(|&c| c.into()))
                    .chain(fp.points.iter().map(|&c| c.into()))
                    .chain(fp.drawings.iter().copied())
                    .collect();

                for child in children {
                    self.owned_subtree(child, out);
                }
            }
            ItemKind::Table => {
                if let Some(table) = id.typed().and_then(|t| self.tables.get(t)) {
                    out.extend(table.cells.iter().map(|&c| ItemId::from(c)));
                }
            }
            _ => {}
        }
    }

    /// The `std::erase` / `erase( it )` steps of `BOARD::Remove`, `FOOTPRINT::Remove` and the
    /// table's `DeleteMarkedCells`, as one retain pass per list.
    fn erase_from_lists(
        &mut self,
        roots: &HashSet<ItemId>,
        parents: &BTreeSet<ItemId>,
        board_level: bool,
    ) {
        let keep = |id: ItemId| !roots.contains(&id);

        if board_level {
            self.marker_list.retain(|&i| keep(i.into()));
            self.drawing_list.retain(|&i| keep(i));
            self.footprint_list.retain(|&i| keep(i.into()));
            self.track_list.retain(|&i| keep(i.into()));
            self.group_list.retain(|&i| keep(i.into()));
            self.constraint_list.retain(|&i| keep(i.into()));
            self.zone_list.retain(|&i| keep(i.into()));
            self.generator_list.retain(|&i| keep(i.into()));
            self.point_list.retain(|&i| keep(i.into()));
        }

        for &parent in parents {
            if let Some(fp) = parent
                .typed::<Footprint>()
                .and_then(|f| self.footprints.get_mut(f))
            {
                fp.fields.retain(|&i| keep(i.into()));
                fp.drawings.retain(|&i| keep(i));
                fp.pads.retain(|&i| keep(i.into()));
                fp.zones.retain(|&i| keep(i.into()));
                fp.groups.retain(|&i| keep(i.into()));
                fp.constraints.retain(|&i| keep(i.into()));
                fp.points.retain(|&i| keep(i.into()));
                fp.bodies.retain(|&i| keep(i.into()));
            } else if let Some(table) = parent
                .typed::<PcbTable>()
                .and_then(|t| self.tables.get_mut(t))
            {
                table.cells.retain(|&i| keep(i.into()));
            }
        }
    }

    /// Delete the doomed items: the `~BOARD_ITEM` index safety net (pointer-keyed unbind),
    /// group membership across the doomed set (a member leaves its surviving group, the
    /// surviving members of a doomed group lose their back reference, `EDA_GROUP::RemoveAll`),
    /// then the arena slots.
    fn free_items(&mut self, doomed: &[ItemId], doomed_set: &HashSet<ItemId>) {
        for &x in doomed {
            self.unbind_item(x);

            let group = self.get(x).and_then(|item| item.base().group);

            if let Some(g) = group.filter(|g| !doomed_set.contains(g)) {
                self.group_members_mut(g, |members| members.retain(|&m| m != x));
            }

            if matches!(x.kind(), ItemKind::Group | ItemKind::Generator) {
                let members: Vec<ItemId> = self.group_members(x).collect();

                for m in members.into_iter().filter(|m| !doomed_set.contains(m)) {
                    if let Some(mut item) = self.get_mut_raw(m) {
                        item.as_dyn_mut().base_mut().group = None;
                    }
                }
            }
        }

        for &x in doomed {
            self.arena_remove(x);
        }
    }

    /// Run `f` on the member list of group or generator `g`.
    fn group_members_mut(&mut self, g: ItemId, f: impl FnOnce(&mut Vec<ItemId>)) {
        match self.get_mut_raw(g) {
            Some(AnyItemMut::Group(group)) => f(&mut group.members),
            Some(AnyItemMut::Generator(generator)) => f(&mut generator.group.members),
            _ => {}
        }
    }

    /// `BOARD::RemoveAll( aTypes )`: `PCB_TRACE_T` removes every track, arc and via,
    /// `PCB_SHAPE_T` every drawing; the other drawing types, `PCB_ARC_T` and `PCB_VIA_T` are
    /// refused (wxFAIL). Clears the whole UUID index (`ClearItemByIdCache`), bumps the time
    /// stamp. The removed items (and their children) are deleted.
    pub fn remove_all(&mut self, types: &[KicadT]) {
        let mut removed: Vec<ItemId> = Vec::new();

        for &kicad_t in types {
            match kicad_t {
                KicadT::PcbNetinfoT => {
                    crate::port_pending!("NETINFO_LIST::detachAll (BOARD::RemoveAll nets, M3)")
                }
                KicadT::PcbMarkerT => {
                    removed.extend(self.marker_list.drain(..).map(ItemId::from));
                }
                KicadT::PcbGroupT => removed.extend(self.group_list.drain(..).map(ItemId::from)),
                KicadT::PcbConstraintT => {
                    removed.extend(self.constraint_list.drain(..).map(ItemId::from));
                }
                KicadT::PcbPointT => removed.extend(self.point_list.drain(..).map(ItemId::from)),
                KicadT::PcbZoneT => removed.extend(self.zone_list.drain(..).map(ItemId::from)),
                KicadT::PcbGeneratorT => {
                    removed.extend(self.generator_list.drain(..).map(ItemId::from));
                }
                KicadT::PcbFootprintT => {
                    removed.extend(self.footprint_list.drain(..).map(ItemId::from));
                }
                KicadT::PcbTraceT => removed.extend(self.track_list.drain(..).map(ItemId::from)),
                KicadT::PcbShapeT => removed.extend(self.drawing_list.drain(..)),
                // "Use PCB_TRACE_T to remove all tracks, arcs, and vias", "Use PCB_SHAPE_T to
                // remove all graphics and text", "needs more ::Type() support".
                _ => {}
            }
        }

        let mut doomed = Vec::new();

        for &id in &removed {
            self.owned_subtree(id, &mut doomed);
        }

        let doomed_set: HashSet<ItemId> = doomed.iter().copied().collect();

        // FinalizeBulkRemove( removed )
        for &id in &removed {
            self.journal_record(ChangeKind::Removed, id);
        }

        self.free_items(&doomed, &doomed_set);
        self.clear_item_by_id_cache();
        self.increment_time_stamp();
    }

    // ---- EDA_GROUP -------------------------------------------------------------------------

    /// `EDA_GROUP::ContainsItem( aItem )`: `item` is a member of `group` or of one of its
    /// member groups (visited once each).
    pub fn group_contains_item(&self, group: ItemId, item: ItemId) -> bool {
        let mut visited: BTreeSet<ItemId> = BTreeSet::new();
        let mut pending = vec![group];

        while let Some(g) = pending.pop() {
            if !visited.insert(g) {
                continue;
            }

            for member in self.group_members(g) {
                if member == item {
                    return true;
                }

                if matches!(member.kind(), ItemKind::Group | ItemKind::Generator) {
                    pending.push(member);
                }
            }
        }

        false
    }

    /// `EDA_GROUP::AddItem`: writes both sides (`member.base.group`, `members`); a member of
    /// another group leaves it first. Refused like the C++ `wxCHECK_RET`s: a stale handle, a
    /// group added to itself, an ancestor group added to its descendant.
    pub fn add_to_group(&mut self, group: ItemId, member: ItemId) {
        if !matches!(group.kind(), ItemKind::Group | ItemKind::Generator)
            || !self.contains(group)
            || !self.contains(member)
            || member.kind() == ItemKind::Net
        {
            return;
        }

        // "Group added to itself."
        if member == group {
            return;
        }

        // "Ancestor group added to group."
        if matches!(member.kind(), ItemKind::Group | ItemKind::Generator)
            && self.group_contains_item(member, group)
        {
            return;
        }

        // Items can only be in one group at a time
        self.remove_from_group(member);

        self.group_members_mut(group, |members| {
            if !members.contains(&member) {
                members.push(member);
            }
        });

        if let Some(mut item) = self.get_mut_raw(member) {
            item.as_dyn_mut().base_mut().group = Some(group);
        }

        self.increment_time_stamp();
    }

    /// `EDA_GROUP::RemoveItem` on the member's group: the back reference is cleared only when
    /// the group really listed the member (`m_items.erase( aItem ) == 1`).
    pub fn remove_from_group(&mut self, member: ItemId) {
        let Some(group) = self.get(member).and_then(|item| item.base().group) else {
            return;
        };

        let mut erased = false;
        self.group_members_mut(group, |members| {
            if let Some(pos) = members.iter().position(|&m| m == member) {
                members.remove(pos);
                erased = true;
            }
        });

        if erased {
            if let Some(mut item) = self.get_mut_raw(member) {
                item.as_dyn_mut().base_mut().group = None;
            }

            self.increment_time_stamp();
        }
    }

    // ---- traversal -------------------------------------------------------------------------

    /// `BOARD::GetPads`: footprint order x pad order.
    pub fn get_pads(&self) -> Vec<crate::ids::PadId> {
        self.footprints()
            .flat_map(|(fp, _)| self.footprint_pads(fp).map(|(pad, _)| pad))
            .collect()
    }

    /// `BOARD::AllConnectedItems` (order: tracks; per footprint pads, zones, connected
    /// graphics; board zones; connected board drawings, `board.cpp:3757`).
    pub fn all_connected_items(&self) -> Vec<ItemId> {
        let is_connected = |id: ItemId| {
            self.get(id)
                .is_some_and(|item| item.as_dyn().connected().is_some())
        };
        let mut items: Vec<ItemId> = Vec::new();

        items.extend(self.tracks().map(|(id, _)| ItemId::from(id)));

        for (fp, footprint) in self.footprints() {
            items.extend(self.footprint_pads(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(self.footprint_zones(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(
                footprint
                    .drawings
                    .iter()
                    .copied()
                    .filter(|&id| is_connected(id)),
            );
        }

        items.extend(self.zones().map(|(id, _)| ItemId::from(id)));
        items.extend(
            self.drawing_list
                .iter()
                .copied()
                .filter(|&id| is_connected(id)),
        );
        items
    }

    /// `BOARD::collectOwnedItems`: tracks, zones, generators, footprints, drawings, markers,
    /// groups, constraints, points (board level only).
    pub fn collect_owned_items(&self) -> Vec<ItemId> {
        let mut items: Vec<ItemId> = Vec::new();
        items.extend(self.track_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.zone_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.generator_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.footprint_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.drawing_list.iter().copied());
        items.extend(self.marker_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.group_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.constraint_list.iter().map(|&i| ItemId::from(i)));
        items.extend(self.point_list.iter().map(|&i| ItemId::from(i)));
        items
    }

    /// `BOARD::GetItemSet`: `std::set<BOARD_ITEM*, CompareByUuid>` of `collectOwnedItems`, i.e.
    /// by UUID, ties by `ItemId` (C++: by address, `board-model.md` 3.1).
    pub fn get_item_set(&self) -> Vec<ItemId> {
        let mut items: Vec<(kicad_core::Kiid, ItemId)> = self
            .collect_owned_items()
            .into_iter()
            .filter_map(|id| self.uuid_of(id).map(|uuid| (uuid, id)))
            .collect();
        items.sort();
        items.dedup();
        items.into_iter().map(|(_, id)| id).collect()
    }

    /// `BOARD::RunOnChildren` (tracks, zones, markers, groups, constraints, points,
    /// footprints (+ children), drawings (+ children), `board.cpp:837`).
    pub fn run_on_children(&self, mode: RecurseMode, f: &mut dyn FnMut(ItemId)) {
        for &id in &self.track_list {
            f(id.into());
        }

        for &id in &self.zone_list {
            f(id.into());
        }

        for &id in &self.marker_list {
            f(id.into());
        }

        for &id in &self.group_list {
            f(id.into());
        }

        for &id in &self.constraint_list {
            f(id.into());
        }

        for &id in &self.point_list {
            f(id.into());
        }

        for &id in &self.footprint_list {
            f(id.into());

            if mode == RecurseMode::Recurse {
                self.item_run_on_children(id.into(), RecurseMode::Recurse, f);
            }
        }

        for &id in &self.drawing_list {
            f(id);

            if mode == RecurseMode::Recurse {
                self.item_run_on_children(id, RecurseMode::Recurse, f);
            }
        }
    }

    /// `BOARD_ITEM::RunOnChildren` by handle, for the overriding classes: `FOOTPRINT` (fields,
    /// pads, zones, groups, bodies, constraints, points, drawings (+ children),
    /// `footprint.cpp:3342`), `PCB_TABLE` (cells), `PCB_GROUP` / `PCB_GENERATOR` (members,
    /// recursing into member groups for `RECURSE`). Every other item has no children.
    pub fn item_run_on_children(&self, id: ItemId, mode: RecurseMode, f: &mut dyn FnMut(ItemId)) {
        match self.get(id) {
            Some(AnyItem::Footprint(fp)) => {
                for &c in &fp.fields {
                    f(c.into());
                }

                for &c in &fp.pads {
                    f(c.into());
                }

                for &c in &fp.zones {
                    f(c.into());
                }

                for &c in &fp.groups {
                    f(c.into());
                }

                for &c in &fp.bodies {
                    f(c.into());
                }

                for &c in &fp.constraints {
                    f(c.into());
                }

                for &c in &fp.points {
                    f(c.into());
                }

                for &c in &fp.drawings {
                    f(c);

                    if mode == RecurseMode::Recurse {
                        self.item_run_on_children(c, RecurseMode::Recurse, f);
                    }
                }
            }
            Some(AnyItem::Table(table)) => {
                // PCB_TABLECELL has no children of its own.
                for &c in &table.cells {
                    f(c.into());
                }
            }
            Some(AnyItem::Group(_)) | Some(AnyItem::Generator(_)) => {
                let members: Vec<ItemId> = self.group_members(id).collect();

                for m in members {
                    f(m);

                    if mode == RecurseMode::Recurse
                        && matches!(m.kind(), ItemKind::Group | ItemKind::Generator)
                    {
                        self.item_run_on_children(m, RecurseMode::Recurse, f);
                    }
                }
            }
            _ => {}
        }
    }

    /// `item_run_on_children( id, NO_RECURSE )` collected.
    pub(crate) fn direct_children(&self, id: ItemId) -> Vec<ItemId> {
        let mut out = Vec::new();
        self.item_run_on_children(id, RecurseMode::NoRecurse, &mut |c| out.push(c));
        out
    }

    // ---- containers: create, fill, attach --------------------------------------------------

    /// `new FOOTPRINT( board )`: arena slot, parent = Board, the four mandatory fields of the
    /// FOOTPRINT constructor. Resolvable through `ctx` and `board[id]`; **not** in
    /// `footprints()` and not UUID-indexed.
    pub fn create_footprint(&mut self) -> FootprintId {
        // m_layer = F_Cu, m_embedFonts = false: the Footprint constructor values.
        let footprint = Footprint::new(&mut self.kiid_generator);
        let fp = self.insert_item(footprint, ParentRef::Board);
        let fp_layer = self.footprints[fp].base.layer;

        let mut add_field = |board: &mut Board, id: FieldT, layer, visible: bool| {
            // new PCB_FIELD( this, id ): PCB_TEXT( FOOTPRINT* ) puts it on F_SilkS (B_SilkS
            // for a back footprint) at the footprint position, which is the origin of the
            // identity transform here, i.e. the default library position.
            let mut field = PcbField::new(&mut board.kiid_generator, Some(fp_layer), id, "");
            field.text.base.layer = layer; // field->SetLayer( layer )
            field.text.text.set_visible(visible); // field->SetVisible( visible )
            let field_id = board.insert_item(field, ParentRef::Item(fp.into()));
            board.footprints[fp].fields.push_back(field_id); // m_fields.push_back( field )
        };

        use kicad_core::PcbLayerId;
        add_field(self, FieldT::Reference, PcbLayerId::FSilkS, true);
        add_field(self, FieldT::Value, PcbLayerId::FFab, true);
        add_field(self, FieldT::Datasheet, PcbLayerId::FFab, false);
        add_field(self, FieldT::Description, PcbLayerId::FFab, false);

        self.increment_time_stamp();
        fp
    }

    /// `IsItemIndexedById( footprint )`: a forward index entry exists for the footprint's UUID
    /// (the test `FOOTPRINT::Add` / `Remove` use to decide whether to (un)index children).
    fn is_footprint_indexed(&self, fp: FootprintId) -> bool {
        self.footprints
            .get(fp)
            .is_some_and(|f| self.is_item_indexed_by_id(&f.base.uuid))
    }

    /// The tail of `FOOTPRINT::Add`: `ClearEditFlags`, `SetParent( this )` were done on
    /// insertion; `CacheItemSubtreeById` when the footprint is indexed,
    /// `InvalidateGeometryCaches`, time stamp, journal.
    fn after_footprint_add(&mut self, fp: FootprintId, id: ItemId) {
        if self.is_footprint_indexed(fp) {
            self.bind_subtree(id);
        }

        if let Some(footprint) = self.footprints.get_mut(fp) {
            footprint.invalidate_geometry_caches();
        }

        self.increment_time_stamp();

        if self.journal.is_enabled() && self.footprint_list.contains(&fp) {
            self.journal_record(ChangeKind::Added, id);
        }
    }

    /// Store a footprint child: `ClearEditFlags()`, `SetParent( footprint )`.
    fn insert_fp_child<T: ArenaItem + BoardItem>(&mut self, mut item: T, fp: FootprintId) -> Id<T>
    where
        Board: ArenaOf<T>,
    {
        item.base_mut().flags.clear(ItemFlags::EDIT_FLAGS);
        self.insert_item(item, ParentRef::Item(fp.into()))
    }

    /// `FOOTPRINT::Add( aItem, aMode )` (front-insert unless mode == Append, fields always
    /// append). Children of an unattached footprint are not UUID-indexed. Refused (handed
    /// back): markers, footprints, tracks and the board-only graphics (the C++ `wxFAIL`
    /// cases), tables (created with `create_table( .., Item( fp ) )` and `attach_table`), a
    /// stale footprint handle.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn footprint_add(
        &mut self,
        fp: FootprintId,
        item: OwnedItem,
        mode: AddMode,
    ) -> Result<ItemId, OwnedItem> {
        if !self.footprints.contains(fp) {
            return Err(item);
        }

        let append = mode == AddMode::Append;

        macro_rules! child {
            ($value:expr, $list:ident) => {{
                let id = self.insert_fp_child($value, fp);
                push_mode(&mut self.footprints[fp].$list, id, append);
                ItemId::from(id)
            }};
        }
        macro_rules! drawing {
            ($value:expr) => {{
                let id: ItemId = self.insert_fp_child($value, fp).into();
                push_mode(&mut self.footprints[fp].drawings, id, append);
                id
            }};
        }

        let id = match item {
            OwnedItem::Field(field) => {
                let id = self.insert_fp_child(*field, fp);
                self.footprints[fp].fields.push_back(id);
                ItemId::from(id)
            }
            OwnedItem::Barcode(b) => drawing!(*b),
            OwnedItem::Text(t) => drawing!(*t),
            OwnedItem::Dimension(d) => drawing!(*d),
            OwnedItem::Shape(s) => drawing!(*s),
            OwnedItem::Textbox(t) => drawing!(*t),
            OwnedItem::Image(i) => drawing!(*i),
            OwnedItem::Pad(p) => child!(*p, pads),
            OwnedItem::Zone(z) => child!(*z, zones),
            OwnedItem::Group(g) => child!(g, groups),
            OwnedItem::Constraint(c) => child!(c, constraints),
            OwnedItem::Point(p) => child!(p, points),
            OwnedItem::Body3d(b) => child!(*b, bodies),
            other => return Err(other),
        };

        self.after_footprint_add(fp, id);
        Ok(id)
    }

    /// `BOARD::Add( footprint, aMode )`: order list per mode, UUID bind of the subtree in the
    /// C++ `CacheItemById` + `CacheChildrenById` order, time stamp. A stale or already
    /// attached footprint is left alone.
    pub fn attach_footprint(&mut self, fp: FootprintId, mode: AddMode) {
        if !self.footprints.contains(fp) || self.footprint_list.contains(&fp) {
            return;
        }

        push_mode(&mut self.footprint_list, fp, mode.is_append());

        let footprint = &mut self.footprints[fp];
        footprint.base.parent = ParentRef::Board;
        footprint.base.flags.clear(ItemFlags::EDIT_FLAGS);

        self.after_board_add(fp.into());
    }

    /// `new PCB_TABLE( parent )`: `parent` is `Board` or `Item(footprint)`; cells added with
    /// `table_add_cell` get `ParentRef::Item(table)`. The value's `cells` must be empty.
    pub fn create_table(&mut self, mut table: PcbTable, parent: ParentRef) -> TableId {
        debug_assert!(table.cells.is_empty(), "create_table: cells are added with table_add_cell");
        table.cells.clear();
        let id = self.insert_item(table, parent);
        self.increment_time_stamp();
        id
    }

    /// `PCB_TABLE::AddCell`: `m_cells.push_back`, `SetLayer( GetLayer() )`, `SetParent( this )`.
    /// Not UUID-indexed (C++ indexes cells when the table is attached).
    pub fn table_add_cell(&mut self, table: TableId, cell: PcbTablecell) -> TablecellId {
        let layer = self[table].base.layer;
        let env = self.child_env(ParentRef::Item(table.into()));
        let id = self.insert_item(cell, ParentRef::Item(table.into()));
        self.tables[table].cells.push(id);
        self.tablecells[id].set_layer(&env, layer);
        self.increment_time_stamp();
        id
    }

    /// `BOARD::Add( table )` / `FOOTPRINT::Add( table )` depending on the table's parent. A
    /// stale or already attached table is left alone; `FOOTPRINT::Add` refuses the generated
    /// tables (`PCB_DRILL_CHART_T`, `PCB_STACKUP_TABLE_T`: wxFAIL).
    pub fn attach_table(&mut self, table: TableId, mode: AddMode) {
        let Some(t) = self.tables.get(table) else {
            return;
        };
        let parent = t.base.parent;
        let kicad_t = BoardItem::type_(t);
        let id: ItemId = table.into();

        match parent.item().and_then(|p| p.typed::<Footprint>()) {
            Some(fp) => {
                if kicad_t != KicadT::PcbTableT
                    || !self.footprints.contains(fp)
                    || self.footprints[fp].drawings.contains(&id)
                {
                    return;
                }

                push_mode(
                    &mut self.footprints[fp].drawings,
                    id,
                    mode == AddMode::Append,
                );
                self.tables[table]
                    .base
                    .flags
                    .clear(ItemFlags::EDIT_FLAGS);
                self.after_footprint_add(fp, id);
            }
            None => {
                if self.drawing_list.contains(&id) {
                    return;
                }

                push_mode(&mut self.drawing_list, id, mode.is_append());
                let t = &mut self.tables[table];
                t.base.parent = ParentRef::Board;
                t.base.flags.clear(ItemFlags::EDIT_FLAGS);
                self.after_board_add(id);
            }
        }
    }

    /// Environment for mutators of a value that is not added yet (the parser calls
    /// `text.rotate( &env, .. )` on a child before `footprint_add`).
    pub fn child_env(&self, parent: ParentRef) -> MutEnv {
        self.env_for_parent(parent, self.current_layer_env())
    }
}

