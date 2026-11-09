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
//! records the changes; M0a wrote the types and the switch.

use kicad_core::{KicadT, Kiid, Lset};

use crate::board::Board;
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
}

impl Clone for ChangeJournal {
    fn clone(&self) -> Self {
        ChangeJournal {
            enabled: self.enabled,
            changes: Vec::new(),
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
        }
    }

    /// Take the recorded changes.
    pub fn drain_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.journal.changes)
    }
}
