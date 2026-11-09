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
