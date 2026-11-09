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

//! Kernel tests of M0a (`board-model.md` 16): `BoardCtx` / `ItemRef` round trip, id
//! stamping, `detached_copy`, `ItemId` order = insertion order, `check_invariants`,
//! `Board: Clone + Send + Sync`. No C++ test corresponds to these (new code).

use kicad_core::{KicadT, Kiid, PcbLayerId};
use kicad_geom::Vector2I;

use crate::board::{AddMode, Board};
use crate::ctx::{BoardCtx, MutEnv};
use crate::ids::{ItemId, ItemKind, TrackId};
use crate::item::{DetachedCopy, ParentRef};
use crate::item_ref::AnyItem;
use crate::pcb_track::{PcbArc, PcbTrack, PcbVia, Track, Viatype, ZoneLayerOverride};
use crate::test_support;
use crate::traits::{BoardConnectedItem, BoardItem};

fn segment(board: &mut Board, start: (i32, i32), end: (i32, i32)) -> Track {
    let mut t = PcbTrack::new(&mut board.kiid_generator);
    t.start = Vector2I::new(start.0, start.1);
    t.end = Vector2I::new(end.0, end.1);
    Track::Trace(t)
}

fn board_with_tracks() -> (Board, Vec<TrackId>) {
    let mut board = Board::new();
    let mut ids = Vec::new();

    for i in 0..3 {
        let t = segment(&mut board, (i, 0), (i, 1_000_000));
        ids.push(board.add_track(t, AddMode::BulkAppend).unwrap());
    }

    let via = PcbVia::new(&mut board.kiid_generator);
    ids.push(board.add_track(via.into(), AddMode::BulkAppend).unwrap());
    let arc = PcbArc::new(&mut board.kiid_generator);
    ids.push(board.add_track(arc.into(), AddMode::BulkAppend).unwrap());
    (board, ids)
}

#[test]
fn constructor_defaults() {
    let t = PcbTrack::default();
    assert_eq!(t.width, 200_000);
    assert!(!t.has_solder_mask);
    assert_eq!(t.solder_mask_margin, None);
    assert_eq!(t.base.item.layer, PcbLayerId::FCu);
    assert_eq!(t.base.item.uuid, Kiid::default());
    assert!(t.base.local_ratsnest_visible);
    assert_eq!(t.net_code(), crate::ids::NetCode::UNCONNECTED);

    let v = PcbVia::default();
    assert_eq!(v.via_type, Viatype::Through);
    assert!(!v.is_free);
    assert!(
        v.zone_layer_overrides
            .iter()
            .all(|z| *z == ZoneLayerOverride::None)
    );
    assert_eq!(BoardItem::type_(&v), KicadT::PcbViaT);
    assert_eq!(Track::from(PcbArc::default()).type_(), KicadT::PcbArcT);
}

#[test]
fn add_track_stamps_ids_and_keeps_order() {
    let (board, ids) = board_with_tracks();

    // Arena indices follow insertion order while nothing was freed, so ItemId order is
    // insertion order (board-model.md 3.1).
    let item_ids: Vec<ItemId> = ids.iter().map(|&id| id.into()).collect();
    let mut sorted = item_ids.clone();
    sorted.sort();
    assert_eq!(sorted, item_ids);
    assert!(item_ids.windows(2).all(|w| w[0].to_bits() < w[1].to_bits()));

    // BulkAppend: file order.
    assert_eq!(board.track_ids(), ids);

    for (id, track) in board.tracks() {
        assert_eq!(track.base().id(), Some(id.into()));
        assert_eq!(track.base().parent, ParentRef::Board);
        assert_eq!(board.resolve_item(&track.base().uuid), Some(id.into()));
    }

    board.check_invariants().unwrap();
}

#[test]
fn insert_mode_prepends_and_non_copper_is_refused() {
    let mut board = Board::new();
    let a = segment(&mut board, (0, 0), (1, 1));
    let b = segment(&mut board, (2, 2), (3, 3));
    let a = board.add_track(a, AddMode::Insert).unwrap();
    let b = board.add_track(b, AddMode::Insert).unwrap();
    assert_eq!(board.track_ids(), vec![b, a]);

    let mut silk = segment(&mut board, (0, 0), (1, 1));
    silk.base_mut().layer = PcbLayerId::FSilkS;
    let stamp = board.time_stamp();
    let refused = board.add_track(silk, AddMode::Append).unwrap_err();
    assert_eq!(refused.base().id(), None);
    assert_eq!(board.time_stamp(), stamp);
    assert_eq!(board.track_ids().len(), 2);

    // Vias are not checked (C++ only tests PCB_TRACE_T / PCB_ARC_T).
    let mut via = PcbVia::default();
    via.track.base.item.layer = PcbLayerId::FSilkS;
    assert!(board.add_track(via.into(), AddMode::Append).is_ok());
    board.check_invariants().unwrap();
}

#[test]
fn modify_track_restamps_and_bumps_time_stamp() {
    let (mut board, ids) = board_with_tracks();
    let id = ids[1];
    let stamp = board.time_stamp();

    let replacement = segment(&mut board, (5, 5), (6, 6));
    let result = board.modify_track(id, |t, env: &MutEnv| {
        assert!(env.parent.is_none());
        t.as_track_mut().width = 1234;
        // f may assign a whole new value: id and parent are re-stamped.
        let width = t.as_track().width;
        *t = replacement;
        t.as_track_mut().width = width;
        7
    });

    assert_eq!(result, Some(7));
    assert!(board.time_stamp() > stamp);
    assert_eq!(board[id].as_track().width, 1234);
    assert_eq!(board[id].base().id(), Some(id.into()));
    assert_eq!(board[id].base().parent, ParentRef::Board);
    board.check_invariants().unwrap();

    // The polymorphic form.
    board
        .modify(id.into(), |item, _| {
            assert_eq!(item.kind(), ItemKind::Track)
        })
        .unwrap();
}

#[test]
fn ctx_item_ref_round_trip() {
    let (board, ids) = board_with_tracks();
    let ctx = BoardCtx::bare(&board);

    for &id in &ids {
        let item = ctx.item(id.into()).expect("live handle");
        assert_eq!(item.id(), Some(id.into()));
        assert_eq!(item.uuid(), board[id].base().uuid);
        assert_eq!(ctx.resolve(&item.uuid()).unwrap(), item);
        assert!(item.as_track().is_some());
        assert!(item.as_connected().is_some());
        assert_eq!(item.net_code(), Some(crate::ids::NetCode::UNCONNECTED));
        assert!(item.parent().is_none());
        assert!(item.parent_footprint().is_none());
        assert!(item.has_board());
        assert!(item.as_pad().is_none());
    }

    let via = ctx.item(ids[3].into()).unwrap();
    assert_eq!(via.type_(), KicadT::PcbViaT);
    assert!(via.as_via().is_some() && via.as_arc().is_none());
    assert!(via.has_hole());
    let arc = ctx.item(ids[4].into()).unwrap();
    assert_eq!(arc.as_dyn().class_name(), "PCB_ARC");
    assert!(arc.as_arc().is_some());

    // Typed dispatch through Track reaches the variant's override.
    let seg = ctx.item(ids[0].into()).unwrap();
    assert_eq!(seg.position(), Vector2I::new(0, 0));
    assert!(seg.as_dyn().is_on_copper_layer(&ctx));

    // A stale handle never resolves to another item.
    let stale: ItemId = crate::ids::Id::<Track>::from_parts(99, 0).into();
    assert!(ctx.item(stale).is_none());
    assert!(board.get(stale).is_none());
    assert!(ctx.resolve(&Kiid::default()).is_none());
}

#[test]
fn detached_copy_has_no_id_but_keeps_parent() {
    let (board, ids) = board_with_tracks();
    let ctx = BoardCtx::bare(&board);
    let original = &board[ids[0]];

    let clone = original.clone();
    assert_eq!(clone.base().id(), Some(ids[0].into()));

    let dummy = original.detached_copy();
    assert_eq!(dummy.base().id(), None);
    assert_eq!(dummy.base().uuid, original.base().uuid);
    assert_eq!(dummy.base().parent, ParentRef::Board);

    let view = ctx.view(AnyItem::Track(&dummy));
    assert_eq!(view.id(), None);
    assert!(view.has_board());
    assert!(view != ctx.item(ids[0].into()).unwrap());
    assert!(view == view);
}

#[test]
fn board_clone_keeps_handles() {
    let (board, ids) = board_with_tracks();
    let copy = board.clone();

    for &id in &ids {
        assert_eq!(copy[id].base().uuid, board[id].base().uuid);
        assert_eq!(copy[id].base().id(), Some(id.into()));
    }

    copy.check_invariants().unwrap();
}

#[test]
fn check_invariants_reports_broken_state() {
    let (mut board, ids) = board_with_tracks();
    board.track_list.pop_back();
    let err = board.check_invariants().unwrap_err();
    assert!(err.contains("in no order list"), "{err}");

    let (mut board, _) = board_with_tracks();
    board.track_list.push_back(ids[0]);
    assert!(
        board
            .check_invariants()
            .unwrap_err()
            .contains("listed twice")
    );

    let (mut board, _) = board_with_tracks();
    board.tracks.get_mut(ids[2]).unwrap().base_mut().id = None;
    assert!(board.check_invariants().unwrap_err().contains("base.id"));

    let (mut board, _) = board_with_tracks();
    board
        .uuid_index
        .insert(Kiid::new(&mut board.kiid_generator), ids[0].into());
    assert!(board.check_invariants().unwrap_err().contains("UUID index"));
}

#[test]
fn frame_fixture_is_consistent() {
    let fx = test_support::rotated_flipped_scaled();
    fx.board.check_invariants().unwrap();
    assert_eq!(fx.children.len(), 15);

    let ctx = BoardCtx::bare(&fx.board);

    for &child in &fx.children {
        let item = ctx.item(child).unwrap();
        assert_eq!(item.parent_footprint_id(), Some(fx.footprint));
        assert!(item.has_board());
    }

    let env = fx.board.mut_env(fx.children[1]);
    let frame = env.parent.expect("footprint child");
    assert_eq!(frame.footprint, fx.footprint);
    assert!(frame.flipped);
    assert_eq!(frame.layer, PcbLayerId::BCu);
    assert_eq!(frame.transform, test_support::rotated_scaled_transform());

    // The cell sees the footprint through its table.
    let cell = *fx
        .children
        .iter()
        .find(|c| c.kind() == ItemKind::Tablecell)
        .unwrap();
    assert_eq!(
        fx.board.mut_env(cell).parent.unwrap().footprint,
        fx.footprint
    );
    assert_eq!(fx.board.footprint_drawings(fx.footprint).count(), 7);
    assert_eq!(fx.board.footprint_pads(fx.footprint).count(), 1);

    // child_env of a value that is not added yet.
    let env = fx.board.child_env(ParentRef::Item(fx.footprint.into()));
    assert!(env.parent.is_some());
    assert!(fx.board.child_env(ParentRef::None).parent.is_none());
}

#[test]
fn board_is_clone_send_sync() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<Board>();
    assert_traits::<Track>();
    assert_traits::<MutEnv>();
    fn assert_sync<T: Sync + ?Sized>() {}
    assert_sync::<BoardCtx<'static>>();
}
