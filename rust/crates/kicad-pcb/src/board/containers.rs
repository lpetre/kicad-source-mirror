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
//! `FinalizeBulk*`) and of `FOOTPRINT::FOOTPRINT` / `FOOTPRINT::Add` / `PCB_TABLE::AddCell` as
//! create / fill / attach (`board-model.md` 8.1). Package M1; M0a wrote `add_track` (the
//! pattern for every `add_*`), `child_env` and the signatures of the container family.

use kicad_core::layer_ids::is_copper_layer;

use crate::board::Board;
use crate::ctx::MutEnv;
use crate::ids::{FootprintId, ItemId, TableId, TablecellId, TrackId};
use crate::item::{ItemFlags, ParentRef};
use crate::item_ref::OwnedItem;
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

impl Board {
    /// `BOARD::Add( track, aMode )` for `PCB_TRACE_T`, `PCB_ARC_T` and `PCB_VIA_T`: `push_back`
    /// for `APPEND` / `BULK_APPEND`, else `push_front`; parent = board, edit flags cleared,
    /// `base.id` stamped, UUID bound, time stamp bumped. A segment or arc on a non-copper layer
    /// is refused like C++ ("Cannot place Track on non-copper layer") and handed back.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn add_track(&mut self, mut track: Track, mode: AddMode) -> Result<TrackId, Track> {
        if !matches!(track, Track::Via(_)) && !is_copper_layer(track.base().layer) {
            return Err(track);
        }

        track.base_mut().flags.clear(ItemFlags::EDIT_FLAGS);
        let uuid = track.base().uuid;
        let id = self.insert_item(track, ParentRef::Board);

        if mode.is_append() {
            self.track_list.push_back(id);
        } else {
            self.track_list.push_front(id);
        }

        self.bind_uuid(uuid, id.into());
        self.increment_time_stamp();

        if self.journal.is_enabled() {
            crate::port_pending!("BOARD_LISTENER::OnBoardItemAdded (journal Added)")
        }

        Ok(id)
    }

    /// `BOARD::Add( Zone, aMode )` (m_zones.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_zone(&mut self, item: crate::zone::Zone, mode: AddMode) -> crate::ids::ZoneId {
        crate::port_pending!("BOARD::Add (zone)", item, mode)
    }

    /// `BOARD::Add( PcbShape, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_shape(
        &mut self,
        item: crate::pcb_shape::PcbShape,
        mode: AddMode,
    ) -> crate::ids::ShapeId {
        crate::port_pending!("BOARD::Add (shape)", item, mode)
    }

    /// `BOARD::Add( PcbText, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_text(
        &mut self,
        item: crate::pcb_text::PcbText,
        mode: AddMode,
    ) -> crate::ids::TextId {
        crate::port_pending!("BOARD::Add (text)", item, mode)
    }

    /// `BOARD::Add( PcbField, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_field(
        &mut self,
        item: crate::pcb_field::PcbField,
        mode: AddMode,
    ) -> crate::ids::FieldId {
        crate::port_pending!("BOARD::Add (field)", item, mode)
    }

    /// `BOARD::Add( PcbTextbox, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_textbox(
        &mut self,
        item: crate::pcb_textbox::PcbTextbox,
        mode: AddMode,
    ) -> crate::ids::TextboxId {
        crate::port_pending!("BOARD::Add (textbox)", item, mode)
    }

    /// `BOARD::Add( PcbDimension, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_dimension(
        &mut self,
        item: crate::pcb_dimension::PcbDimension,
        mode: AddMode,
    ) -> crate::ids::DimensionId {
        crate::port_pending!("BOARD::Add (dimension)", item, mode)
    }

    /// `BOARD::Add( PcbBarcode, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_barcode(
        &mut self,
        item: crate::pcb_barcode::PcbBarcode,
        mode: AddMode,
    ) -> crate::ids::BarcodeId {
        crate::port_pending!("BOARD::Add (barcode)", item, mode)
    }

    /// `BOARD::Add( PcbReferenceImage, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_image(
        &mut self,
        item: crate::pcb_reference_image::PcbReferenceImage,
        mode: AddMode,
    ) -> crate::ids::ImageId {
        crate::port_pending!("BOARD::Add (image)", item, mode)
    }

    /// `BOARD::Add( PcbTarget, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_target(
        &mut self,
        item: crate::pcb_target::PcbTarget,
        mode: AddMode,
    ) -> crate::ids::TargetId {
        crate::port_pending!("BOARD::Add (target)", item, mode)
    }

    /// `BOARD::Add( PcbGridItem, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_grid_item(
        &mut self,
        item: crate::pcb_grid_item::PcbGridItem,
        mode: AddMode,
    ) -> crate::ids::GridItemId {
        crate::port_pending!("BOARD::Add (grid_item)", item, mode)
    }

    /// `BOARD::Add( PcbDrillMap, aMode )` (m_drawings, front unless APPEND / BULK_APPEND): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_drill_map(
        &mut self,
        item: crate::pcb_drill_map::PcbDrillMap,
        mode: AddMode,
    ) -> crate::ids::DrillMapId {
        crate::port_pending!("BOARD::Add (drill_map)", item, mode)
    }

    /// `BOARD::Add( PcbPoint, aMode )` (m_points.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_point(
        &mut self,
        item: crate::pcb_point::PcbPoint,
        mode: AddMode,
    ) -> crate::ids::PointId {
        crate::port_pending!("BOARD::Add (point)", item, mode)
    }

    /// `BOARD::Add( PcbGroup, aMode )` (m_groups.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_group(
        &mut self,
        item: crate::pcb_group::PcbGroup,
        mode: AddMode,
    ) -> crate::ids::GroupId {
        crate::port_pending!("BOARD::Add (group)", item, mode)
    }

    /// `BOARD::Add( PcbGenerator, aMode )` (m_generators.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_generator(
        &mut self,
        item: crate::pcb_generator::PcbGenerator,
        mode: AddMode,
    ) -> crate::ids::GeneratorId {
        crate::port_pending!("BOARD::Add (generator)", item, mode)
    }

    /// `BOARD::Add( PcbConstraint, aMode )` (m_constraints.push_back): parent = board, edit flags
    /// cleared, `base.id` stamped, UUID bound, time stamp bumped.
    pub fn add_constraint(
        &mut self,
        item: crate::pcb_constraint::PcbConstraint,
        mode: AddMode,
    ) -> crate::ids::ConstraintId {
        crate::port_pending!("BOARD::Add (constraint)", item, mode)
    }

    /// `BOARD::Add( PCB_MARKER )` (`m_markers.push_back`).
    pub fn add_marker(&mut self, marker: crate::pcb_marker::PcbMarker) -> crate::ids::MarkerId {
        crate::port_pending!("BOARD::Add (marker)", marker)
    }

    /// `BOARD::Add( aItem, aMode )`, polymorphic: dispatches on the variant; a kind the board
    /// does not accept (pad, cell, 3D body) or a C++ refusal hands the item back.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn add(&mut self, item: OwnedItem, mode: AddMode) -> Result<ItemId, OwnedItem> {
        crate::port_pending!("BOARD::Add", item, mode)
    }

    /// `BOARD::Remove` / `FOOTPRINT::Remove` + delete: cascades to children, leaves its
    /// group, unbinds UUIDs, time stamp; also discards an unattached container. `false` for
    /// a stale handle.
    pub fn remove(&mut self, id: ItemId) -> bool {
        crate::port_pending!("BOARD::Remove", id)
    }

    /// `remove` for several items with one retain pass per list (`REMOVE_MODE::BULK` +
    /// `FinalizeBulkRemove`); returns the number removed.
    pub fn remove_many(&mut self, ids: &[ItemId]) -> usize {
        crate::port_pending!("BOARD::Remove (bulk)", ids)
    }

    /// `BOARD::RemoveAll( aTypes )`.
    pub fn remove_all(&mut self, types: &[kicad_core::KicadT]) {
        crate::port_pending!("BOARD::RemoveAll", types)
    }

    /// `EDA_GROUP::AddItem`: writes both sides (`member.base.group`, `members`); a member of
    /// another group leaves it first.
    pub fn add_to_group(&mut self, group: ItemId, member: ItemId) {
        crate::port_pending!("EDA_GROUP::AddItem", group, member)
    }

    /// `EDA_GROUP::RemoveItem` on the member's group.
    pub fn remove_from_group(&mut self, member: ItemId) {
        crate::port_pending!("EDA_GROUP::RemoveItem", member)
    }

    /// `BOARD::GetPads`: footprint order x pad order.
    pub fn get_pads(&self) -> Vec<crate::ids::PadId> {
        crate::port_pending!("BOARD::GetPads")
    }

    /// `BOARD::AllConnectedItems` (order: tracks; per footprint pads, zones, connected
    /// graphics; board zones; connected board drawings, `board.cpp:3757`).
    pub fn all_connected_items(&self) -> Vec<ItemId> {
        crate::port_pending!("BOARD::AllConnectedItems")
    }

    /// `BOARD::collectOwnedItems`.
    pub fn collect_owned_items(&self) -> Vec<ItemId> {
        crate::port_pending!("BOARD::collectOwnedItems")
    }

    /// `BOARD::GetItemSet`: by UUID, ties by `ItemId`.
    pub fn get_item_set(&self) -> Vec<ItemId> {
        crate::port_pending!("BOARD::GetItemSet")
    }

    /// `BOARD::RunOnChildren` (tracks, zones, markers, groups, constraints, points,
    /// footprints (+ children), drawings (+ children), `board.cpp:837`).
    pub fn run_on_children(&self, mode: crate::item::RecurseMode, f: &mut dyn FnMut(ItemId)) {
        crate::port_pending!("BOARD::RunOnChildren", mode, f)
    }

    /// `new FOOTPRINT( board )`: arena slot, parent = Board, the four mandatory fields of the
    /// FOOTPRINT constructor. Resolvable through `ctx` and `board[id]`; **not** in
    /// `footprints()` and not UUID-indexed.
    pub fn create_footprint(&mut self) -> FootprintId {
        crate::port_pending!("FOOTPRINT::FOOTPRINT (create_footprint)")
    }

    /// `FOOTPRINT::Add( aItem, aMode )` (front-insert unless mode == Append, fields always
    /// append). Children of an unattached footprint are not UUID-indexed.
    #[allow(clippy::result_large_err)] // the C++ refusal hands the item back (board-model.md 8.1)
    pub fn footprint_add(
        &mut self,
        fp: FootprintId,
        item: OwnedItem,
        mode: AddMode,
    ) -> Result<ItemId, OwnedItem> {
        crate::port_pending!("FOOTPRINT::Add", fp, item, mode)
    }

    /// `BOARD::Add( footprint, aMode )`: order list per mode, UUID bind of the subtree in the
    /// C++ `CacheItemById` + `CacheChildrenById` order, time stamp.
    pub fn attach_footprint(&mut self, fp: FootprintId, mode: AddMode) {
        crate::port_pending!("BOARD::Add (footprint)", fp, mode)
    }

    /// `new PCB_TABLE( parent )`: `parent` is `Board` or `Item(footprint)`; cells added with
    /// `table_add_cell` get `ParentRef::Item(table)`.
    pub fn create_table(&mut self, table: PcbTable, parent: ParentRef) -> TableId {
        crate::port_pending!("PCB_TABLE::PCB_TABLE (create_table)", table, parent)
    }

    /// `PCB_TABLE::AddCell`.
    pub fn table_add_cell(&mut self, table: TableId, cell: PcbTablecell) -> TablecellId {
        crate::port_pending!("PCB_TABLE::AddCell", table, cell)
    }

    /// `BOARD::Add( table )` / `FOOTPRINT::Add( table )` depending on the table's parent.
    pub fn attach_table(&mut self, table: TableId, mode: AddMode) {
        crate::port_pending!("BOARD::Add (table)", table, mode)
    }

    /// Environment for mutators of a value that is not added yet (the parser calls
    /// `text.rotate( &env, .. )` on a child before `footprint_add`).
    pub fn child_env(&self, parent: ParentRef) -> MutEnv {
        self.env_for_parent(parent, self.current_layer_env())
    }
}
