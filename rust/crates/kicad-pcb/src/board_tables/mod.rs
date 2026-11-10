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

//! `pcbnew/board_tables/` (package M26): the board-level refresh of generated tables. The
//! schema and table models are [`crate::pcb_generated_table`] (`generated_table_schema`),
//! [`crate::pcb_stackup_table`] (`stackup_table_model`) and [`crate::pcb_drill_chart`].
//! `board_characteristics_table.cpp` (`Build_Board_Characteristics_Table`, a one-shot plain
//! `PCB_TABLE` placed by the editor) is not ported: see PORT_STATUS.md, package M26.

pub mod generated_table_refresh;

pub use generated_table_refresh::refresh_generated_tables;
