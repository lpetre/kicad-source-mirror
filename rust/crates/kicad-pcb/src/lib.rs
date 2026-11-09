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

//! Port of KiCad's board data model (`pcbnew/board*.{h,cpp}`, the `BOARD_ITEM` hierarchy),
//! the `.kicad_pcb` reader / writer (`io`) and project settings (`project`).
//!
//! Part of the Rust port of KiCad core features; see `rust/PORTING.md`. The architecture is
//! `rust/docs/design/board-model.md`, the data model `rust/docs/design/board-model-types.md`.
//! The module tree below is complete and frozen after work package M0 (`board-model.md` 16):
//! packages add code to their modules, not modules to this file.

// A trait shim whose inherent method is missing recurses into itself; make that a build error
// (board-model.md 6.2).
#![deny(unconditional_recursion)]

// ---- kernel (new code) -------------------------------------------------------------------
#[macro_use]
pub mod pending;
#[macro_use]
mod items_macro;
pub mod arena;
pub mod cache;
pub mod ctx;
pub mod drc_api;
pub mod foundation_stub;
pub mod ids;
pub mod item;
pub mod item_ref;
#[cfg(test)]
pub(crate) mod test_support;
pub mod traits;

// ---- board and item base -----------------------------------------------------------------
pub mod board;
pub mod board_connected_item;
pub mod board_item;
pub mod eda_item;

// ---- nets, settings, project data ---------------------------------------------------------
pub mod board_design_settings;
pub mod board_stackup;
pub mod component_class;
pub mod embedded_files;
pub mod net_settings;
pub mod netclass;
pub mod netinfo;
pub mod pcb_plot_params;
pub mod teardrop_parameters;
pub mod zone_settings;

// ---- items ---------------------------------------------------------------------------------
pub mod convert_shape_list_to_polygon;
pub mod eda_shape;
pub mod eda_text;
pub mod footprint;
pub mod footprint_courtyard_index;
pub mod line_ending;
pub mod pad;
pub mod pad_utils;
pub mod padstack;
pub mod pcb_3d_body;
pub mod pcb_barcode;
pub mod pcb_constraint;
pub mod pcb_dimension;
pub mod pcb_drill_map;
pub mod pcb_field;
pub mod pcb_generated_table;
pub mod pcb_generator;
pub mod pcb_grid_item;
pub mod pcb_group;
pub mod pcb_marker;
pub mod pcb_point;
pub mod pcb_reference_image;
pub mod pcb_shape;
pub mod pcb_table;
pub mod pcb_tablecell;
pub mod pcb_target;
pub mod pcb_text;
pub mod pcb_textbox;
pub mod pcb_track;
pub mod stroke_params;
pub mod zone;
pub mod zone_utils;

// ---- file formats ----------------------------------------------------------------------------
pub mod io;
pub mod project;

pub use board::{AddMode, Board, RemoveMode};
pub use ctx::{BoardCtx, ConnectivityQuery, LayerEnv, MutEnv, ParentFrame, RuleResolver};
pub use ids::{ArenaItem, Id, ItemId, ItemKind, NetCode};
pub use item::{BoardItemBase, ConnectedItemBase, DetachedCopy, ParentRef};
pub use item_ref::{AnyItem, AnyItemMut, ItemRef, OwnedItem};
pub use traits::{
    BoardConnectedItem, BoardItem, BoardItemMut, EdaShapeItem, EdaTextItem, Invalidate,
};
