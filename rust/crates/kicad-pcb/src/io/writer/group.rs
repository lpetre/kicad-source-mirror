// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-09: `format( const PCB_GROUP* )` (W:3002-3050).

use kicad_sexpr::IoError;
use kicad_sexpr::{format_bool, format_custom_properties, format_uuid};

use super::PcbWriter;
use crate::ids::{GroupId, ItemId};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_GROUP* aGroup )`: `(group "name" (uuid ..) [(locked yes)]
    /// [(lib_id "..")] (members "uuid" ...) (custom_property ..)*)`; a group without (valid)
    /// members is not written.
    pub(crate) fn format_group(&mut self, group: GroupId) -> Result<(), IoError> {
        let Some(g) = self.src.group(group) else {
            return Ok(());
        };
        let group_item: ItemId = group.into();

        // Validate member pointers against the board cache to avoid use-after-free on dangling
        // pointers (e.g. when a group held a reference to a deleted item).  This validation
        // only applies when the group itself is part of m_board; for groups created off-board
        // (e.g. a DeepClone() used by the clipboard) the cache contains the originals, not our
        // clones, so skip the validation in that case and trust the member pointers.
        //
        // IsItemIndexedById( const BOARD_ITEM* ): the index entry of the item's UUID names
        // this very item (handles cannot dangle here).
        let indexed = |board: &crate::board::Board, id: ItemId| -> bool {
            board
                .uuid_of(id)
                .is_some_and(|uuid| board.uuid_index.get(&uuid) == Some(&id))
        };

        let validate_against_board = self.board.is_some_and(|b| indexed(b, group_item));

        let mut member_ids: Vec<String> = Vec::with_capacity(g.members.len());

        for &member in &g.members {
            let keep = match self.board {
                Some(board) if validate_against_board => indexed(board, member),
                _ => true,
            };

            if keep {
                if let Some(uuid) = self.src.uuid_of(member) {
                    member_ids.push(uuid.as_string());
                }
            }
        }

        if member_ids.is_empty() {
            return Ok(());
        }

        let name = self.out.quotew(g.name());
        self.out.print(format_args!("(group {name}"))?;

        format_uuid(self.out, &g.base.uuid.as_string())?;

        if self
            .ctx
            .item(group_item)
            .is_some_and(|item| item.is_locked())
        {
            format_bool(self.out, "locked", true)?;
        }

        if g.has_design_block_link() {
            // Manual quotes, no escaping (W:3037).
            let lib_id = g.design_block_lib_id().format();
            self.out.print(format_args!("(lib_id \"{lib_id}\")"))?;
        }

        // wxArrayString::Sort(): wxString::Cmp, i.e. code point (= UTF-8 byte) order.
        member_ids.sort();

        self.out.print(format_args!("(members"))?;

        for member_id in &member_ids {
            let quoted = self.out.quotew(member_id);
            self.out.print(format_args!(" {quoted}"))?;
        }

        self.out.print(format_args!(")"))?; // Close `members` token.

        format_custom_properties(self.out, &g.base.custom_properties)?;
        self.out.print(format_args!(")"))?; // Close `group` token.

        Ok(())
    }
}
