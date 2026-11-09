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
//! The children are the placeholder `Default` values until the type packages land; packages
//! that need meaningful child data set it on the fixture (`board.<arena>.get_mut(id)`) or
//! extend these builders with their own kind (each kind has its own builder function below,
//! so packages do not collide).

use kicad_core::{Kiid, PcbLayerId};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{EdaAngle, Vector2I};

use crate::board::Board;
use crate::ids::{FootprintId, ItemId, TableId};
use crate::item::ParentRef;
use crate::traits::BoardItem;

/// A board with one attached footprint and its children.
pub(crate) struct FrameFixture {
    pub board: Board,
    pub footprint: FootprintId,
    /// Every child created by the builder, in creation order.
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
/// and one child of each footprint child kind.
pub(crate) fn board_with_footprint(transform: TransformTrs, flipped: bool) -> FrameFixture {
    let mut board = Board::new();
    let mut footprint = crate::footprint::Footprint::default();
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

    let mut fixture = FrameFixture {
        board,
        footprint: fp,
        children: Vec::new(),
    };
    add_field(&mut fixture);
    add_pad(&mut fixture);
    add_shape(&mut fixture);
    add_text(&mut fixture);
    add_textbox(&mut fixture);
    add_table(&mut fixture);
    add_dimension(&mut fixture);
    add_barcode(&mut fixture);
    add_image(&mut fixture);
    add_zone(&mut fixture);
    add_group(&mut fixture);
    add_point(&mut fixture);
    add_constraint(&mut fixture);
    add_body(&mut fixture);
    fixture
}

/// Insert `item` as a child of the fixture footprint, draw its UUID, bind it.
fn child<T>(fx: &mut FrameFixture, mut item: T) -> crate::ids::Id<T>
where
    T: crate::ids::ArenaItem + BoardItem,
    Board: crate::board::ArenaOf<T>,
{
    item.base_mut().uuid = Kiid::new(&mut fx.board.kiid_generator);
    let uuid = item.base().uuid;
    let id = fx
        .board
        .insert_item(item, ParentRef::Item(fx.footprint.into()));
    fx.board.bind_uuid(uuid, id.into());
    fx.children.push(id.into());
    id
}

macro_rules! fp_child_builder {
    ($(#[$doc:meta])* $name:ident, $ty:ty, $list:ident) => {
        $(#[$doc])*
        pub(crate) fn $name(fx: &mut FrameFixture) -> ItemId {
            let id = child(fx, <$ty>::default());
            let fp = fx.footprint;
            fx.board.footprints.get_mut(fp).expect("fixture footprint").$list.push_back(id.into());
            id.into()
        }
    };
}

fp_child_builder!(/// A field (`fields`).
    add_field, crate::pcb_field::PcbField, fields);
fp_child_builder!(/// A pad (`pads`).
    add_pad, crate::pad::Pad, pads);
fp_child_builder!(/// A graphic shape (`drawings`).
    add_shape, crate::pcb_shape::PcbShape, drawings);
fp_child_builder!(/// A text (`drawings`).
    add_text, crate::pcb_text::PcbText, drawings);
fp_child_builder!(/// A text box (`drawings`).
    add_textbox, crate::pcb_textbox::PcbTextbox, drawings);
fp_child_builder!(/// A dimension (`drawings`).
    add_dimension, crate::pcb_dimension::PcbDimension, drawings);
fp_child_builder!(/// A barcode (`drawings`).
    add_barcode, crate::pcb_barcode::PcbBarcode, drawings);
fp_child_builder!(/// A reference image (`drawings`).
    add_image, crate::pcb_reference_image::PcbReferenceImage, drawings);
fp_child_builder!(/// A zone (`zones`).
    add_zone, crate::zone::Zone, zones);
fp_child_builder!(/// A group (`groups`, no members).
    add_group, crate::pcb_group::PcbGroup, groups);
fp_child_builder!(/// A point (`points`).
    add_point, crate::pcb_point::PcbPoint, points);
fp_child_builder!(/// A constraint (`constraints`).
    add_constraint, crate::pcb_constraint::PcbConstraint, constraints);
fp_child_builder!(/// A 3D body (`bodies`).
    add_body, crate::pcb_3d_body::Pcb3dBody, bodies);

/// A table (`drawings`) with one cell (`ParentRef::Item(table)`).
pub(crate) fn add_table(fx: &mut FrameFixture) -> TableId {
    let table = child(fx, crate::pcb_table::PcbTable::default());
    let fp = fx.footprint;
    fx.board
        .footprints
        .get_mut(fp)
        .expect("fixture footprint")
        .drawings
        .push_back(table.into());

    let mut cell = crate::pcb_tablecell::PcbTablecell::default();
    cell.base_mut().uuid = Kiid::new(&mut fx.board.kiid_generator);
    let uuid = cell.base().uuid;
    let cell = fx.board.insert_item(cell, ParentRef::Item(table.into()));
    fx.board.bind_uuid(uuid, cell.into());
    fx.board
        .tables
        .get_mut(table)
        .expect("fixture table")
        .cells
        .push(cell);
    fx.children.push(cell.into());
    table
}
