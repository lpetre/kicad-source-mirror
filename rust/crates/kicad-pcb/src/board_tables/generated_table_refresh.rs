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

//! Port of `pcbnew/board_tables/generated_table_refresh.h` / `.cpp`:
//! `RefreshGeneratedTables( BOARD& )`. `GENERATED_TABLE_REFRESH` / `GENERATED_TABLE_PENDING`
//! are [`GeneratedTableRefresh`].

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::TableId;
pub use crate::pcb_generated_table::GeneratedTableRefresh;

/// Port of `RefreshGeneratedTables( aBoard )`: rebuild every stale generated table among the
/// board's drawings (`Drawings()` order) and return how many were rebuilt.
///
/// Pending state is committed only once every table has rebuilt, so a table that throws cannot
/// leave half a profile behind. `aBoard.OnItemsChanged( rebuilt )` (the editor's view and
/// selection listeners) has no counterpart in the port.
pub fn refresh_generated_tables(board: &mut Board) -> i32 {
    let mut refresh = GeneratedTableRefresh::new();
    let mut rebuilt: Vec<TableId> = Vec::new();

    let tables: Vec<TableId> = board
        .drawings()
        .filter_map(|(id, _)| id.typed::<crate::pcb_table::PcbTable>())
        .filter(|&t| board[t].generated.is_some())
        .collect();

    for table in tables {
        let stale = {
            let ctx = BoardCtx::bare(board);
            board[table].generated_is_stale(&ctx)
        };

        if !stale {
            continue;
        }

        board.generated_rebuild_cells(table, Some(&mut refresh));
        rebuilt.push(table);
    }

    if rebuilt.is_empty() {
        return 0;
    }

    refresh.commit(board);

    rebuilt.len() as i32
}
