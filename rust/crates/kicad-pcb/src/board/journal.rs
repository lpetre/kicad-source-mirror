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

//! The optional pull-based change journal that replaces `BOARD_LISTENER`
//! (`board-model.md` 8.5). Off by default; no tier 1/2 package may depend on it. Package M1
//! records the changes: `Added` where `BOARD::Add` invokes `OnBoardItemAdded` (every mode:
//! the bulk modes are reported at once instead of by `FinalizeBulkAdd`) and for children added
//! to an attached footprint, `Removed` for every removed root item (`OnBoardItemRemoved`,
//! `FinalizeBulkRemove`), `Modified` after every `modify_*` (`OnBoardItemChanged`). Recording
//! asks the item for its layer set, so it reaches the type packages' `GetLayerSet`.

use kicad_core::{KicadT, Kiid, Lset};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::{ItemId, NetCode};

/// What happened to an item.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// One journal entry (`BOARD_LISTENER::OnBoardItem{Added,Removed,Changed}`).
#[derive(Clone, Debug)]
pub struct Change {
    pub kind: ChangeKind,
    pub id: ItemId,
    pub uuid: Kiid,
    pub kicad_t: KicadT,
    pub net_before: Option<NetCode>,
    pub layers_before: Lset,
}

/// The journal: a switch and the recorded changes. A clone starts empty (switch kept).
#[derive(Debug, Default)]
pub struct ChangeJournal {
    enabled: bool,
    changes: Vec<Change>,
    /// Snapshot of the item a `modify_*` is about to edit (taken when its `MutEnv` is built).
    before: Option<Change>,
}

impl Clone for ChangeJournal {
    fn clone(&self) -> Self {
        ChangeJournal {
            enabled: self.enabled,
            changes: Vec::new(),
            before: None,
        }
    }
}

impl ChangeJournal {
    /// True when changes are recorded.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Board {
    /// Switch the journal on or off (default: off). Switching it off drops recorded changes.
    pub fn set_journal_enabled(&mut self, on: bool) {
        self.journal.enabled = on;

        if !on {
            self.journal.changes.clear();
            self.journal.before = None;
        }
    }

    /// The journal entry of `id` as it is now (`None` for a stale handle).
    fn journal_snapshot(&self, kind: ChangeKind, id: ItemId) -> Option<Change> {
        let item = self.get(id)?;
        let ctx = BoardCtx::bare(self);
        let item = item.as_dyn();

        Some(Change {
            kind,
            id,
            uuid: item.base().uuid,
            kicad_t: item.type_(),
            net_before: item.connected().map(|c| c.net),
            layers_before: item.layer_set(&ctx),
        })
    }

    /// Record `kind` for `id` when the journal is on.
    pub(crate) fn journal_record(&mut self, kind: ChangeKind, id: ItemId) {
        if !self.journal.enabled {
            return;
        }

        if let Some(change) = self.journal_snapshot(kind, id) {
            self.journal.changes.push(change);
        }
    }

    /// Remember net and layers of `id` before a `modify_*` edits it.
    pub(crate) fn journal_note_before(&mut self, id: ItemId) {
        if !self.journal.enabled {
            return;
        }

        self.journal.before = self.journal_snapshot(ChangeKind::Modified, id);
    }

    /// Record `Modified` for `id` with the values noted before the edit (the current ones if
    /// no snapshot of `id` was taken).
    pub(crate) fn journal_record_modified(&mut self, id: ItemId) {
        if !self.journal.enabled {
            return;
        }

        let before = self.journal.before.take().filter(|c| c.id == id);
        let Some(mut change) = self.journal_snapshot(ChangeKind::Modified, id) else {
            return;
        };

        if let Some(before) = before {
            change.net_before = before.net_before;
            change.layers_before = before.layers_before;
        }

        self.journal.changes.push(change);
    }

    /// Take the recorded changes.
    pub fn drain_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.journal.changes)
    }
}
