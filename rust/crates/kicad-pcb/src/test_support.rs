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

//! Fixture builders for frame tests (`board-model.md` 16): a board with one rotated, flipped
//! and non-uniformly scaled footprint and one child of each kind, built by setting the
//! `pub(crate)` frame fields and order lists directly, so that packages of waves 1-3 can test
//! frame-dependent code before `create_footprint` (M1) and the footprint cascades (M19) exist.
//!
//! **This file is frozen (M0a review): no package edits it.** The children are `T::default()`
//! values. A test that needs meaningful child data, in its own module, either
//!
//! * edits the fixture child field by field through the `pub(crate)` arena
//!   (`fx.board.pads.get_mut(fx.child(ItemKind::Pad).typed().unwrap())`), or
//! * replaces its value with `fx.set_child(id, my_pad)` (identity and parent are kept), or
//! * adds its own child with `fx.add_child(value, |fp| &mut fp.pads)` /
//!   `fx.add_cell(table, cell)`.
//!
//! Run `fx.board.check_invariants()` after building.

use std::collections::VecDeque;

use kicad_core::{Kiid, PcbLayerId};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{EdaAngle, Vector2I};

use crate::board::{ArenaOf, Board};
use crate::footprint::Footprint;
use crate::ids::{ArenaItem, FootprintId, Id, ItemId, ItemKind, TableId, TablecellId};
use crate::item::ParentRef;
use crate::traits::BoardItem;

/// A board with one attached footprint and its children.
pub(crate) struct FrameFixture {
    pub board: Board,
    pub footprint: FootprintId,
    /// Every child created by the builders, in creation order (the table's cell included).
    pub children: Vec<ItemId>,
}

/// The transform of `rotated_flipped_scaled()`: translate (10 mm, 20 mm), rotate 30 degrees,
/// scale (1.5, 0.75).
pub(crate) fn rotated_scaled_transform() -> TransformTrs {
    TransformTrs::new(
        Vector2I::new(10_000_000, 20_000_000),
        EdaAngle::from_degrees(30.0),
        1.5,
        0.75,
    )
}

/// The standard frame fixture: rotated, flipped, non-uniformly scaled footprint.
pub(crate) fn rotated_flipped_scaled() -> FrameFixture {
    board_with_footprint(rotated_scaled_transform(), true)
}

/// A board with one footprint placed with `transform` / `flipped` (side B_Cu when flipped)
/// and one default-constructed child of each footprint child kind: field, pad, shape, text,
/// textbox, table (with one cell), dimension, barcode, image, zone, group, point, constraint,
/// 3D body.
pub(crate) fn board_with_footprint(transform: TransformTrs, flipped: bool) -> FrameFixture {
    let mut board = Board::new();
    let mut footprint = Footprint::default();
    footprint.base.uuid = Kiid::new(&mut board.kiid_generator);
    footprint.base.layer = if flipped {
        PcbLayerId::BCu
    } else {
        PcbLayerId::FCu
    };
    footprint.transform = transform;
    footprint.flipped = flipped;

    let fp = board.insert_item(footprint, ParentRef::Board);
    board.footprint_list.push_back(fp);
    let uuid = board[fp].base.uuid;
    board.bind_uuid(uuid, fp.into());

    let mut fx = FrameFixture {
        board,
        footprint: fp,
        children: Vec::new(),
    };
    fx.add_child(crate::pcb_field::PcbField::default(), |fp| &mut fp.fields);
    fx.add_child(crate::pad::Pad::default(), |fp| &mut fp.pads);
    fx.add_child(crate::pcb_shape::PcbShape::default(), |fp| &mut fp.drawings);
    fx.add_child(crate::pcb_text::PcbText::default(), |fp| &mut fp.drawings);
    fx.add_child(crate::pcb_textbox::PcbTextbox::default(), |fp| {
        &mut fp.drawings
    });
    let table = fx.add_child(crate::pcb_table::PcbTable::default(), |fp| &mut fp.drawings);
    fx.add_cell(table, crate::pcb_tablecell::PcbTablecell::default());
    fx.add_child(crate::pcb_dimension::PcbDimension::default(), |fp| {
        &mut fp.drawings
    });
    fx.add_child(crate::pcb_barcode::PcbBarcode::default(), |fp| {
        &mut fp.drawings
    });
    fx.add_child(
        crate::pcb_reference_image::PcbReferenceImage::default(),
        |fp| &mut fp.drawings,
    );
    fx.add_child(crate::zone::Zone::default(), |fp| &mut fp.zones);
    fx.add_child(crate::pcb_group::PcbGroup::default(), |fp| &mut fp.groups);
    fx.add_child(crate::pcb_point::PcbPoint::default(), |fp| &mut fp.points);
    fx.add_child(crate::pcb_constraint::PcbConstraint::default(), |fp| {
        &mut fp.constraints
    });
    fx.add_child(crate::pcb_3d_body::Pcb3dBody::default(), |fp| {
        &mut fp.bodies
    });
    fx
}

impl FrameFixture {
    /// The first fixture child of `kind` (panics if there is none).
    pub(crate) fn child(&self, kind: ItemKind) -> ItemId {
        *self
            .children
            .iter()
            .find(|id| id.kind() == kind)
            .unwrap_or_else(|| panic!("the fixture has no {} child", kind.name()))
    }

    /// Add `item` as a child of the fixture footprint: `parent` and `base.id` stamped, a fresh
    /// UUID drawn and bound, the handle appended to the child list `list` selects
    /// (`|fp| &mut fp.pads`, `|fp| &mut fp.drawings`, ...).
    pub(crate) fn add_child<T, L>(
        &mut self,
        mut item: T,
        list: impl FnOnce(&mut Footprint) -> &mut VecDeque<L>,
    ) -> Id<T>
    where
        T: ArenaItem + BoardItem,
        Board: ArenaOf<T>,
        Id<T>: Into<L>,
    {
        item.base_mut().uuid = Kiid::new(&mut self.board.kiid_generator);
        let uuid = item.base().uuid;
        let id = self
            .board
            .insert_item(item, ParentRef::Item(self.footprint.into()));
        self.board.bind_uuid(uuid, id.item_id());
        let fp = self
            .board
            .footprints
            .get_mut(self.footprint)
            .expect("fixture footprint");
        list(fp).push_back(id.into());
        self.children.push(id.item_id());
        id
    }

    /// Add `cell` to `table` (`ParentRef::Item(table)`, appended to `PcbTable::cells`).
    pub(crate) fn add_cell(
        &mut self,
        table: TableId,
        mut cell: crate::pcb_tablecell::PcbTablecell,
    ) -> TablecellId {
        cell.base_mut().uuid = Kiid::new(&mut self.board.kiid_generator);
        let uuid = cell.base().uuid;
        let id = self.board.insert_item(cell, ParentRef::Item(table.into()));
        self.board.bind_uuid(uuid, id.into());
        self.board
            .tables
            .get_mut(table)
            .expect("fixture table")
            .cells
            .push(id);
        self.children.push(id.into());
        id
    }

    /// Replace the value of a fixture child, keeping what makes it that child (`base.id`,
    /// `parent`, `group`, UUID): the bookkeeping of `Board::modify_*`, usable before the
    /// type's mutators exist.
    pub(crate) fn set_child<T>(&mut self, id: Id<T>, mut value: T)
    where
        T: ArenaItem + BoardItem,
        Board: ArenaOf<T>,
    {
        let slot = self
            .board
            .arena_mut()
            .get_mut(id)
            .expect("live fixture child");
        let (own_id, parent, group, uuid) = {
            let old = slot.base();
            (old.id, old.parent, old.group, old.uuid)
        };
        let base = value.base_mut();
        base.id = own_id;
        base.parent = parent;
        base.group = group;
        base.uuid = uuid;
        *slot = value;
    }
}
