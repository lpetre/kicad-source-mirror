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

//! Port of the save-order comparators of `pcbnew/board.cpp` (`BOARD::cmp_items`,
//! `BOARD::cmp_drawings`). Package M22. Free functions next to the C++ comparator's type
//! (`board-model.md` 10).

use std::cmp::Ordering;

use kicad_core::KicadT;

use crate::ctx::BoardCtx;
use crate::eda_text::EdaTextExt;
use crate::ids::ItemId;
use crate::item_ref::ItemRef;

/// `BOARD::cmp_items::operator()` as an `Ordering`: `Type()`, layer, position x then y,
/// `m_Uuid`, then pointer order, which is `ItemId` order (`board-model.md` 3.1). A stale
/// handle sorts by `ItemId` only.
pub fn board_cmp_items(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(a_ref), Some(b_ref)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };

    if a_ref.type_() != b_ref.type_() {
        return (a_ref.type_() as i32).cmp(&(b_ref.type_() as i32));
    }

    if a_ref.layer() != b_ref.layer() {
        return a_ref.layer().0.cmp(&b_ref.layer().0);
    }

    let (a_pos, b_pos) = (a_ref.position(), b_ref.position());

    if a_pos.x != b_pos.x {
        return a_pos.x.cmp(&b_pos.x);
    }

    if a_pos.y != b_pos.y {
        return a_pos.y.cmp(&b_pos.y);
    }

    if a_ref.uuid() != b_ref.uuid() {
        // shopuld be always the case foer valid boards
        return a_ref.uuid().cmp(&b_ref.uuid());
    }

    a.cmp(&b)
}

/// `BOARD::cmp_drawings::operator()` as an `Ordering`: `Type()`, the layer of single-layer
/// types, the type's own `Compare` (shapes, texts and fields, text boxes, tables, barcodes),
/// `m_Uuid`, then `ItemId` order (`board-model.md` 3.1). A stale handle sorts by `ItemId`
/// only.
pub fn board_cmp_drawings(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(first), Some(second)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };

    if first.type_() != second.type_() {
        return (first.type_() as i32).cmp(&(second.type_() as i32));
    }

    // Layer-free drawings (grid items) assert in GetLayer(); they sort by uuid below.
    if first.type_().is_single_layer_type() && first.layer() != second.layer() {
        return first.layer().0.cmp(&second.layer().0);
    }

    // Callers keep these in a std::set, so any branch reporting equality for two distinct items
    // loses one of them. Every branch has to fall through to the uuid
    let mut cmp = 0;

    if first.type_() == KicadT::PcbShapeT {
        cmp = eda_shape_compare(&first, &second);
    } else if first.type_() == KicadT::PcbTextT || first.type_() == KicadT::PcbFieldT {
        cmp = eda_text_compare(ctx, &first, &second);
    } else if first.type_() == KicadT::PcbTextboxT {
        cmp = eda_shape_compare(&first, &second);

        if cmp == 0 {
            cmp = eda_text_compare(ctx, &first, &second);
        }
    } else if first.type_().base_type() == KicadT::PcbTableT {
        cmp = pcb_table_compare(&first, &second);
    } else if first.type_() == KicadT::PcbBarcodeT {
        cmp = pcb_barcode_compare(&first, &second);
    }

    if cmp != 0 {
        return cmp.cmp(&0);
    }

    if first.uuid() != second.uuid() {
        return first.uuid().cmp(&second.uuid());
    }

    a.cmp(&b)
}

/// `EDA_TEXT::Compare` of two text items (`PCB_TEXT` does not override it).
fn eda_text_compare(ctx: &BoardCtx<'_>, a: &ItemRef<'_>, b: &ItemRef<'_>) -> i32 {
    match (a.as_eda_text(), b.as_eda_text()) {
        (Some(ta), Some(tb)) => ta.compare_text(ctx, tb),
        _ => 0,
    }
}

/// `EDA_SHAPE::Compare` (`shape->Compare( other )` / `textbox->PCB_SHAPE::Compare( other )`):
/// a non-virtual member reading only `EDA_SHAPE` data, so the embedded base data of both items
/// is compared.
fn eda_shape_compare(a: &ItemRef<'_>, b: &ItemRef<'_>) -> i32 {
    match (a.as_eda_shape(), b.as_eda_shape()) {
        (Some(sa), Some(sb)) => sa.eda_shape().compare(sb.eda_shape()),
        _ => 0,
    }
}

/// `PCB_TABLE::Compare`: the table package (M11) has no Rust counterpart yet.
fn pcb_table_compare(a: &ItemRef<'_>, b: &ItemRef<'_>) -> i32 {
    crate::port_pending!("PCB_TABLE::Compare", a, b)
}

/// `PCB_BARCODE::Compare`: the barcode package (M25) has no Rust counterpart yet.
fn pcb_barcode_compare(a: &ItemRef<'_>, b: &ItemRef<'_>) -> i32 {
    crate::port_pending!("PCB_BARCODE::Compare", a, b)
}

/// Package M22 tests of the comparators on items whose getters are ported (tracks); the
/// drawing branches need the item `Compare` functions of M7 / M10 / M11 / M25.
#[cfg(test)]
mod m22_tests {
    use kicad_core::{Kiid, PcbLayerId};
    use kicad_geom::Vector2I;

    use super::*;
    use crate::board::{AddMode, Board};
    use crate::pcb_track::{PcbTrack, PcbVia, Track};

    fn segment(board: &mut Board, layer: PcbLayerId, start: (i32, i32), uuid: &str) -> ItemId {
        let mut t = PcbTrack::new(&mut board.kiid_generator);
        t.start = Vector2I::new(start.0, start.1);
        t.end = Vector2I::new(start.0 + 10, start.1);
        t.base.item.layer = layer;
        t.base.item.uuid = Kiid::parse(uuid).unwrap();
        board
            .add_track(Track::Trace(t), AddMode::Append)
            .unwrap()
            .into()
    }

    #[test]
    fn cmp_items_order() {
        let mut board = Board::default();
        let u1 = "00000000-0000-0000-0000-000000000001";
        let u2 = "00000000-0000-0000-0000-000000000002";
        let a = segment(&mut board, PcbLayerId::FCu, (5, 5), u2);
        let b = segment(&mut board, PcbLayerId::BCu, (0, 0), u1);
        let c = segment(&mut board, PcbLayerId::FCu, (5, 1), u2);
        let d = segment(&mut board, PcbLayerId::FCu, (5, 5), u1);
        let e = segment(&mut board, PcbLayerId::FCu, (5, 5), u1);
        let via = PcbVia::new(&mut board.kiid_generator);
        let via: ItemId = board.add_track(via.into(), AddMode::Append).unwrap().into();

        let ctx = BoardCtx::bare(&board);
        // Type first (PCB_TRACE_T < PCB_VIA_T), then layer, x, y, uuid, handle.
        assert_eq!(board_cmp_items(&ctx, a, via), Ordering::Less);
        assert_eq!(board_cmp_items(&ctx, a, b), Ordering::Less); // F_Cu(0) < B_Cu(2)
        assert_eq!(board_cmp_items(&ctx, c, a), Ordering::Less); // y 1 < 5
        assert_eq!(board_cmp_items(&ctx, d, a), Ordering::Less); // uuid
        assert_eq!(board_cmp_items(&ctx, d, e), Ordering::Less); // same uuid: handle order
        assert_eq!(board_cmp_items(&ctx, e, e), Ordering::Equal);
    }
}
