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

//! Port of the data flow of `BOARD_COMMIT::Push` (`pcbnew/board_commit.cpp`) without the GUI
//! (package LIVE-1): transactions with incremental connectivity, checkpoints and the final
//! full rebuild.
//!
//! A [`Transaction`] stages removals, changes and additions (`COMMIT::Remove / Modify / Add`,
//! the `m_entries` order); [`Session::commit`] applies them in that order the way `Push` does
//! for a board editor commit with a null frame (the QA / headless case: `SKIP_UNDO`, no view,
//! no auto zone refill):
//!
//! * `CHT_ADD`: `board->Add( item, ADD_MODE::BULK_INSERT )` (front of `m_tracks`; with
//!   [`Transaction::append`] `ADD_MODE::APPEND`, the end), which calls
//!   `CONNECTIVITY_DATA::Add( item )`;
//! * `CHT_REMOVE`: `board->Remove( item, REMOVE_MODE::BULK )`, which calls
//!   `CONNECTIVITY_DATA::Remove( item )`;
//! * `CHT_MODIFY`: the item is edited, then `MarkItemNetAsDirty( copy )` (the pre-edit
//!   net) and `Update( item )` unless `SKIP_CONNECTIVITY`;
//! * then `RecalculateRatsnest( commit )` (net propagation, ratsnest of the dirty nets
//!   only), `UpdateRatsnestExclusions()`, or with `SKIP_CONNECTIVITY` `ClearRatsnest()`.
//!
//! Teardrops are not touched (`SKIP_TEARDROPS`: the router calls
//! [`Session::update_teardrops`] when it wants them), see `PORT_STATUS.md`.
//!
//! [`Session::checkpoint`] / [`Session::rollback`] snapshot and restore board and
//! connectivity (the model is plain data: `Board: Clone`, `ConnectivityData: Clone`), the C++
//! counterpart being `BOARD_COMMIT::Revert` / undo. [`Session::finalize`] is
//! `BOARD::BuildConnectivity()`, the state a reload of the saved board has.

use kicad_connectivity::data::ConnectivityData;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::{AddMode, Board, ItemId};
use serde::{Deserialize, Serialize};

use super::edit::{EditBatch, EditResult, ItemChange, NewItem, RemoveRouting};
use super::refill::ChangeDamage;
use super::{handle, item_id};
use crate::error::SessionError;
use crate::session::Session;

/// One staged change (`COMMIT_LINE`), in staging order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Staged {
    /// `COMMIT::Remove( item )` by handle.
    Remove {
        /// The item handle.
        id: u64,
    },
    /// `COMMIT::Remove` of every item [`RemoveRouting`] selects when the entry is reached.
    RemoveRouting(RemoveRouting),
    /// `COMMIT::Modify( item )` and the edit of [`ItemChange`].
    Change(ItemChange),
    /// `COMMIT::Add( new item )`.
    Add(NewItem),
}

/// A staged commit (`BOARD_COMMIT` before `Push`). Build it with [`Transaction::begin`] and
/// the `stage_*` methods, apply it with [`Session::commit`]. Nothing touches the board before
/// the commit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Transaction {
    /// The entries in `m_entries` order.
    pub entries: Vec<Staged>,
    /// `SKIP_CONNECTIVITY`: the connectivity items are still added / removed (by
    /// `BOARD::Add / Remove`), but changes are not re-indexed and the ratsnest is cleared
    /// instead of recalculated. A later [`Session::finalize`] or full rebuild repairs it.
    pub skip_connectivity: bool,
    /// Add new items with `ADD_MODE::APPEND` (end of `BOARD::Tracks()`) instead of `Push`'s
    /// `ADD_MODE::BULK_INSERT` (front). The board order is what a full connectivity build
    /// iterates (and what `apply_edits` keeps for re-added routing).
    pub append: bool,
}

impl Transaction {
    /// An empty transaction (`BOARD_COMMIT( ... )`).
    pub fn begin() -> Transaction {
        Transaction::default()
    }

    /// Whether nothing is staged (`COMMIT::Empty`).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stages the removal of an item.
    pub fn stage_remove(&mut self, id: u64) -> &mut Self {
        self.entries.push(Staged::Remove { id });
        self
    }

    /// Stages the removal of the routing [`RemoveRouting`] selects.
    pub fn stage_remove_routing(&mut self, r: RemoveRouting) -> &mut Self {
        self.entries.push(Staged::RemoveRouting(r));
        self
    }

    /// Stages a change of an existing track / arc / via.
    pub fn stage_change(&mut self, c: ItemChange) -> &mut Self {
        self.entries.push(Staged::Change(c));
        self
    }

    /// Stages a new item.
    pub fn stage_add(&mut self, item: NewItem) -> &mut Self {
        self.entries.push(Staged::Add(item));
        self
    }

    /// Stages an [`EditBatch`] in its documented order: removals by handle, the routing
    /// removal, changes, additions.
    pub fn stage_batch(&mut self, batch: &EditBatch) -> &mut Self {
        for &id in &batch.remove {
            self.stage_remove(id);
        }

        if let Some(r) = &batch.remove_routing {
            self.stage_remove_routing(r.clone());
        }

        for c in &batch.change {
            self.stage_change(c.clone());
        }

        for a in &batch.add {
            self.stage_add(a.clone());
        }

        self
    }
}

/// A net whose item was given another net by the net propagation of the commit
/// (`propagateConnections`: `aCommit->Modify( item ); item->SetNetCode( net )`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetChange {
    /// The item handle.
    pub id: u64,
    /// The net before the propagation.
    pub from: i32,
    /// The net after it.
    pub to: i32,
}

/// What [`Session::commit`] returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReport {
    /// New / removed / changed handles (as [`Session::apply_edits`] returns them).
    #[serde(flatten)]
    pub edit: EditResult,
    /// The nets whose ratsnest was rebuilt (dirty after the net propagation), ascending.
    pub dirty_nets: Vec<i32>,
    /// `[net, unconnected edges]` for every dirty net (`RN_NET::GetEdges().size()`).
    pub net_unconnected: Vec<[i32; 2]>,
    /// `GetUnconnectedCount( false )` of the whole board after the commit.
    pub unconnected: u32,
    /// `GetUnconnectedCount( true )` (edges not excluded by markers).
    pub unconnected_visible: u32,
    /// Items whose net the propagation changed, in call order.
    pub net_changes: Vec<NetChange>,
    /// The areas, layers and nets the commit changed, for
    /// [`Session::refill_zones_affected`] (package LIVE-2).
    #[serde(default)]
    pub damage: super::refill::ChangeDamage,
}

/// A saved state of board and connectivity ([`Session::checkpoint`]).
#[derive(Clone)]
pub struct Checkpoint {
    board: Board,
    connectivity: ConnectivityData,
    /// The damage the session recorded since the last refill (package LIVE-2).
    fill_tracking: super::refill::FillTracking,
}

impl Session {
    /// Checks the removals and changes of a transaction before anything is applied: live
    /// handles, trackable items, valid layers and nets.
    fn validate(&self, txn: &Transaction) -> Result<(), SessionError> {
        for entry in &txn.entries {
            match entry {
                Staged::Remove { id } => {
                    if !self.board.contains(item_id(*id)?) {
                        return Err(SessionError::StaleId { id: *id });
                    }
                }
                Staged::Change(c) => {
                    let id = item_id(c.id)?;
                    let Some(track_id) = id.typed::<kicad_pcb::pcb_track::Track>() else {
                        return Err(SessionError::invalid_argument(
                            "only tracks, arcs and vias can be changed",
                        ));
                    };

                    if self.board.track(track_id).is_none() {
                        return Err(SessionError::StaleId { id: c.id });
                    }

                    for l in [&c.layer, &c.bottom].into_iter().flatten() {
                        super::edit::copper_layer(l)?;
                    }

                    if let Some(net) = c.net {
                        self.net_code(net)?;
                    }
                }
                Staged::RemoveRouting(_) | Staged::Add(_) => {}
            }
        }

        Ok(())
    }

    /// `CHT_REMOVE` of one item: `board->Remove( item, BULK )` with its
    /// `m_connectivity->Remove( item )`. The connectivity is told first because the port's
    /// `Remove` reads the item (type, net, footprint pads) through the board, which the C++
    /// reads through the still-allocated item after `BOARD::Remove`; the result is the same.
    fn commit_remove(&mut self, item: ItemId, result: &mut EditResult, damage: &mut ChangeDamage) {
        if !self.board.contains(item) {
            return; // already removed by an earlier entry
        }

        damage.record_removed(&BoardCtx::bare(&self.board), item);

        self.connectivity.remove(&BoardCtx::bare(&self.board), item);

        if self.board.remove(item) {
            result.removed.push(item.to_bits());
        }
    }

    /// Port of `BOARD_COMMIT::Push` for the staged entries (see the module comment): the
    /// board edits in staging order with the incremental connectivity calls, then
    /// `RecalculateRatsnest` of the dirty nets, the propagated nets applied to the board and
    /// `UpdateRatsnestExclusions`. Validation errors (stale handles, bad layers / nets in
    /// removals and changes) leave everything unchanged; an addition that fails stops the
    /// commit after the entries before it, the connectivity is still brought up to date for
    /// them and the error is returned.
    pub fn commit(&mut self, txn: &Transaction) -> Result<CommitReport, SessionError> {
        self.validate(txn)?;

        let tracking_valid = self.fill_tracking_valid();
        let mut report = CommitReport::default();

        // if( Empty() ) return;
        if txn.is_empty() {
            report.unconnected = self.connectivity.unconnected_count(false);
            report.unconnected_visible = self.connectivity.unconnected_count(true);
            return Ok(report);
        }

        let outcome = self.commit_entries(txn, &mut report.edit, &mut report.damage);

        if txn.skip_connectivity {
            // connectivity->ClearRatsnest(); ClearLocalRatsnest() (no local ratsnest)
            self.connectivity.clear_ratsnest();
        } else {
            // connectivity->RecalculateRatsnest( this )
            self.connectivity
                .recalculate_ratsnest(&BoardCtx::bare(&self.board));

            // propagateConnections: aCommit->Modify( item ); item->SetNetCode( net )
            for (id, net) in self.connectivity.take_net_changes() {
                let from = BoardCtx::bare(&self.board)
                    .item(id)
                    .and_then(|it| it.net_code())
                    .map_or(0, |n| n.0);
                report.damage.record_net(from);
                self.board.set_item_net(id, net, false);
                report.damage.record_item(&BoardCtx::bare(&self.board), id);
                report.net_changes.push(NetChange {
                    id: handle(id),
                    from,
                    to: net.0,
                });
            }

            // board->UpdateRatsnestExclusions()
            self.connectivity.update_ratsnest_exclusions(&self.board);

            report.dirty_nets = self.connectivity.last_dirty_nets().to_vec();
            report.net_unconnected = report
                .dirty_nets
                .iter()
                .map(|&n| {
                    let edges = self
                        .connectivity
                        .ratsnest_for_net(n)
                        .map_or(0, |rn| rn.unsorted_edges().len());
                    [n, edges as i32]
                })
                .collect();
        }

        // The connectivity is up to date with the board again (the tier 1/2 staleness check
        // would otherwise rebuild it in full).
        self.connectivity.mark_up_to_date(&self.board);

        // The own-clearance memo of the DRC engine is invalidated per item by the C++ setters
        // (BOARD::InvalidateClearanceCache); clearing it all never changes a value.
        if let Some(engine) = &self.drc_engine {
            engine.clear_clearance_cache();
        }

        report.unconnected = self.connectivity.unconnected_count(false);
        report.unconnected_visible = self.connectivity.unconnected_count(true);

        // A failed entry may have left an item half edited: the next refill is a full one.
        if outcome.is_err() {
            report.damage = ChangeDamage::full();
        }

        self.fill_tracking_advance(tracking_valid, &report.damage);
        outcome?;

        let ctx = BoardCtx::bare(&self.board);
        report.edit.added_uuids = report
            .edit
            .added
            .iter()
            .filter_map(|&id| {
                ctx.item(ItemId::from_bits(id)?)
                    .map(|it| it.uuid().as_string())
            })
            .collect();
        Ok(report)
    }

    /// The board half of `Push`: every entry in staging order with its connectivity call.
    fn commit_entries(
        &mut self,
        txn: &Transaction,
        result: &mut EditResult,
        damage: &mut ChangeDamage,
    ) -> Result<(), SessionError> {
        for entry in &txn.entries {
            match entry {
                Staged::Remove { id } => self.commit_remove(item_id(*id)?, result, damage),
                Staged::RemoveRouting(r) => {
                    for item in self.routing_to_remove(r) {
                        self.commit_remove(item, result, damage);
                    }
                }
                Staged::Change(c) => {
                    let id = item_id(c.id)?;

                    // connectivity->MarkItemNetAsDirty( boardItemCopy ): the item before the
                    // edit (C++ edits before Push and keeps the copy).
                    if !txn.skip_connectivity {
                        self.connectivity
                            .mark_item_net_as_dirty(&BoardCtx::bare(&self.board), id);
                    }

                    damage.record_item(&BoardCtx::bare(&self.board), id);
                    self.change_item(c)?;
                    damage.record_item(&BoardCtx::bare(&self.board), id);

                    // connectivity->Update( boardItem )
                    if !txn.skip_connectivity {
                        self.connectivity.update(&BoardCtx::bare(&self.board), id);
                    }

                    result.changed.push(id.to_bits());
                }
                Staged::Add(item) => {
                    let mode = if txn.append {
                        AddMode::Append
                    } else {
                        AddMode::BulkInsert
                    };
                    let id = match item {
                        NewItem::Track(t) => self.insert_track(t, mode)?,
                        NewItem::Arc(a) => self.insert_arc(a, mode)?,
                        NewItem::Via(v) => self.insert_via(v, mode)?,
                    };

                    // board->Add( boardItem, ADD_MODE::BULK_INSERT ): m_connectivity->Add
                    self.connectivity.add(&BoardCtx::bare(&self.board), id);
                    damage.record_item(&BoardCtx::bare(&self.board), id);
                    result.added.push(id.to_bits());
                }
            }
        }

        Ok(())
    }

    /// Saves board and connectivity (items, handles, UUIDs, nets, zone fills, UUID generator
    /// state, connectivity graph and ratsnest) for [`Session::rollback`]. Cost: one deep copy
    /// of the board and of the connectivity (see `docs/PERF.md`, pipeline `edit`).
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            board: self.board.clone(),
            connectivity: self.connectivity.clone(),
            fill_tracking: self.fill_tracking_state(),
        }
    }

    /// Puts board and connectivity back to a [`Checkpoint`] exactly (the counterpart of
    /// `BOARD_COMMIT::Revert` / undo, without reparsing). The checkpoint stays usable.
    pub fn rollback(&mut self, checkpoint: &Checkpoint) {
        self.restore(checkpoint.clone());
    }

    /// [`Session::rollback`] consuming the checkpoint (no copy).
    pub fn restore(&mut self, checkpoint: Checkpoint) {
        self.board = checkpoint.board;
        self.connectivity = checkpoint.connectivity;
        self.set_fill_tracking_state(checkpoint.fill_tracking);

        if let Some(engine) = &self.drc_engine {
            engine.clear_clearance_cache();
        }
    }

    /// The full rebuild `BOARD::BuildConnectivity()` that a reload of the saved board runs:
    /// after it the connectivity (and what follows: full refill, full DRC, save) is the one a
    /// fresh load computes, independent of the order of the incremental edits. The items the
    /// rebuild re-nets are recorded for the next [`Session::refill_zones_affected`]
    /// ([`Session::finalize_tracked`] returns them).
    pub fn finalize(&mut self) {
        let _ = self.finalize_tracked();
    }
}
