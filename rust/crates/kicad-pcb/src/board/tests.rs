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
use crate::ids::{ItemId, ItemKind, NetCode, PadId, TrackId};
use crate::item::{DetachedCopy, ParentRef};
use crate::item_ref::AnyItem;
use crate::netinfo::NetinfoItem;
use crate::pad::Pad;
use crate::pcb_textbox::PcbTextbox;
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

    // The polymorphic form, with the mutating virtuals of a leaf item.
    board
        .modify(id.into(), |mut item, env| {
            assert_eq!(item.kind(), ItemKind::Track);
            item.as_leaf_mut()
                .expect("a track is a leaf item")
                .move_by(env, Vector2I::new(10, 20));
        })
        .unwrap();
    assert_eq!(board[id].as_track().start, Vector2I::new(15, 25));
    assert_eq!(board[id].as_track().end, Vector2I::new(16, 26));

    // Containers have no leaf mutators: they cascade through Board methods.
    let mut fx = test_support::rotated_flipped_scaled();
    let fp = fx.footprint;
    fx.board
        .modify(fp.into(), |mut item, _| {
            assert!(item.as_leaf_mut().is_none())
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
    let cell = fx.child(ItemKind::Tablecell);
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

/// A type that implements `BoardItem`, `BoardConnectedItem`, `BoardItemMut`, `EdaTextItem` and
/// `EdaShapeItem` (PCB_TEXTBOX : PCB_SHAPE, EDA_TEXT) must be usable with method syntax while
/// all five traits and both extension traits are in scope: no method name may exist in two of
/// them. Never run (the bodies are stubs); it only has to compile.
#[allow(dead_code)]
fn textbox_method_syntax_is_unambiguous(tb: &mut PcbTextbox, ctx: &BoardCtx<'_>, env: &MutEnv) {
    use crate::drc_api::DrcConstraintT;
    use crate::eda_shape::EdaShapeExt;
    use crate::eda_text::EdaTextExt;
    use crate::item::RecurseMode;
    use crate::traits::{BoardItemMut, EdaShapeItem, EdaTextItem, Flashing};
    use kicad_core::text_vars::ResolutionContext;
    use kicad_core::{Lset, PCB_IU_SCALE, units_provider::UnitsProvider};
    use kicad_geom::geometry::approximation::ErrorLoc;
    use kicad_geom::{ANGLE_0, Box2I, FlipDirection, ShapePolySet};

    fn ext_only<T: EdaShapeExt + EdaTextExt + ?Sized>(_: &T) {}
    // The textbox has an inherent method for every mutating virtual, so method syntax below
    // never reaches `BoardItemMut`; the bound keeps the trait part of the check.
    fn mutators<T: BoardItemMut + EdaShapeItem + EdaTextItem + ?Sized>(_: &T) {}

    let layer = PcbLayerId::FCu;
    let p = Vector2I::new(0, 0);
    let units = UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
    let mut buffer = ShapePolySet::default();

    // BoardItem
    let _ = (tb.base().id(), tb.type_(), tb.class_name(), tb.uuid());
    let _ = (tb.connected().is_some(), tb.parent_group(ctx));
    let _ = (tb.layer(ctx), tb.layer_set(ctx), tb.is_on_layer(ctx, layer));
    let _ = (tb.is_on_copper_layer(ctx), tb.is_side_specific(ctx));
    let _ = (
        tb.is_layer_agnostic(),
        tb.fits_enabled_layers(ctx, Lset::new(), 2),
    );
    let _ = (tb.position(ctx), tb.center(ctx), tb.bounding_box(ctx));
    let _ = tb.effective_shape(
        ctx,
        layer,
        Flashing::Default,
        DrcConstraintT::NullConstraint,
    );
    let _ = tb.effective_hole_shape(ctx, layer, DrcConstraintT::NullConstraint);
    tb.transform_shape_to_polygon(ctx, &mut buffer, layer, 0, 1, ErrorLoc::Inside, false);
    tb.transform_shape_to_poly_set(ctx, &mut buffer, layer, 0, 1, ErrorLoc::Inside);
    let _ = (tb.has_hole(ctx), tb.has_drilled_hole(ctx));
    let _ = (
        tb.is_tented(ctx, layer),
        tb.has_line_stroke(),
        tb.stroke(ctx),
    );
    let _ = (
        tb.hit_test(ctx, p, 0),
        tb.hit_test_rect(ctx, &Box2I::new(p, p), true, 0),
    );
    let _ = (tb.is_locked(ctx), tb.is_knockout());
    tb.run_on_children(ctx, RecurseMode::Recurse, &mut |_| {});
    let _ = (
        tb.item_description(ctx, &units, true),
        tb.friendly_name(ctx),
    );
    let _ = tb.layer_mask_describe(ctx);

    // BoardConnectedItem
    let _ = (tb.net_code(), tb.is_connected(ctx), tb.local_clearance());
    let _ = (
        tb.effective_netclass(ctx),
        tb.own_clearance(ctx, layer, false),
    );
    let _ = tb.clearance_overrides(ctx, false);

    // EdaTextItem + EdaTextExt
    let _ = (tb.text_pos(ctx), tb.text_size(ctx), tb.text_thickness(ctx));
    let _ = (tb.text_angle(ctx), tb.draw_pos(ctx), tb.draw_rotation(ctx));
    let _ = tb.shown_text(ctx, ResolutionContext::ForErcDrc, 0);
    let _ = (tb.font_metrics(ctx), tb.text_box(ctx, None));

    // EdaShapeItem + EdaShapeExt
    let _ = (
        tb.width(ctx),
        tb.effective_width(ctx),
        tb.hatch_line_width(ctx),
    );
    let _ = (tb.hatch_line_spacing(ctx), tb.is_proxy_item());
    let _ = (
        tb.is_filled_for_hit_testing(),
        tb.top_left(ctx),
        tb.bot_right(ctx),
    );
    let _ = (tb.corners_in_sequence(ctx, ANGLE_0), tb.shape_draw_rotation(ctx));
    let _ = (tb.shape_max_error(ctx), tb.hatching_knockouts(ctx));
    ext_only(tb);
    mutators(tb);

    // mutators of all three
    tb.set_position(env, p);
    tb.move_by(env, p);
    tb.rotate(env, p, ANGLE_0);
    tb.flip(env, p, FlipDirection::LeftRight);
    tb.mirror(env, p, FlipDirection::LeftRight);
    tb.set_layer(env, layer);
    tb.set_layer_set(env, Lset::new());
    tb.normalize();
    tb.on_footprint_transformed(env);
    tb.on_footprint_rescaled(env, 1.0, 1.0, 1.0, p, ANGLE_0);
    tb.style_from_settings(ctx.board.design_settings(), false);
    tb.set_text_pos(env, p);
    tb.set_text_size(env, p, true);
    tb.set_text_thickness(env, 0);
    tb.set_text_angle(env, ANGLE_0);
    tb.offset(env, p);
    let _ = tb.eda_text_mut();
    let _ = tb.connected_mut();
    let _ = tb.eda_shape_mut();
    let _ = tb.base_mut();
}

#[test]
fn derived_items_are_their_own_mixin_hosts() {
    // PCB_FIELD, PCB_DIMENSION_BASE and PCB_TABLECELL override EDA_TEXT / EDA_SHAPE virtuals of
    // their bases: the trait object must be the item itself, not its embedded base struct.
    fn text_host<T: crate::traits::EdaTextItem>() {}
    fn shape_host<T: crate::traits::EdaShapeItem>() {}
    text_host::<crate::pcb_text::PcbText>();
    text_host::<crate::pcb_field::PcbField>();
    text_host::<crate::pcb_dimension::PcbDimension>();
    text_host::<PcbTextbox>();
    text_host::<crate::pcb_tablecell::PcbTablecell>();
    shape_host::<crate::pcb_shape::PcbShape>();
    shape_host::<PcbTextbox>();
    shape_host::<crate::pcb_tablecell::PcbTablecell>();

    let fx = test_support::rotated_flipped_scaled();
    let ctx = BoardCtx::bare(&fx.board);

    for &child in &fx.children {
        let item = ctx.item(child).unwrap();
        let is_text = matches!(
            child.kind(),
            ItemKind::Text
                | ItemKind::Field
                | ItemKind::Dimension
                | ItemKind::Textbox
                | ItemKind::Tablecell
                | ItemKind::Barcode
        );
        let is_shape = matches!(
            child.kind(),
            ItemKind::Shape | ItemKind::Textbox | ItemKind::Tablecell
        );
        assert_eq!(item.as_eda_text().is_some(), is_text, "{child:?}");
        assert_eq!(item.as_eda_shape().is_some(), is_shape, "{child:?}");
        assert_eq!(
            item.as_connected().is_some(),
            is_shape || matches!(child.kind(), ItemKind::Pad | ItemKind::Zone),
            "{child:?}"
        );
    }
}

#[test]
fn table_cell_answers_with_the_group_of_its_table() {
    let mut fx = test_support::rotated_flipped_scaled();
    let group = fx.child(ItemKind::Group);
    let table = fx.child(ItemKind::Table);
    let cell = fx.child(ItemKind::Tablecell);

    // Both sides of the relation, written directly (add_to_group is M1).
    fx.board
        .groups
        .get_mut(group.typed().unwrap())
        .unwrap()
        .members
        .push(table);
    fx.board
        .tables
        .get_mut(table.typed().unwrap())
        .unwrap()
        .base
        .group = Some(group);
    fx.board.check_invariants().unwrap();

    let ctx = BoardCtx::bare(&fx.board);
    let cell = ctx.item(cell).unwrap();
    assert_eq!(cell.base().group, None);
    assert_eq!(cell.parent_group().and_then(|g| g.id()), Some(group));
    assert_eq!(
        ctx.item(table).unwrap().parent_group().unwrap().id(),
        Some(group)
    );
    assert!(ctx.item(group).unwrap().parent_group().is_none());
    assert_eq!(
        ctx.item(group).unwrap().as_group().unwrap().members,
        vec![table]
    );
}

#[test]
fn modify_keeps_the_relations_only_board_writes() {
    let mut fx = test_support::rotated_flipped_scaled();
    let group = fx.child(ItemKind::Group);
    let pad: PadId = fx.child(ItemKind::Pad).typed().unwrap();
    fx.board
        .groups
        .get_mut(group.typed().unwrap())
        .unwrap()
        .members
        .push(pad.into());
    fx.board.pads.get_mut(pad).unwrap().base.item.group = Some(group);
    fx.board.check_invariants().unwrap();
    let uuid = fx.board[pad].base.item.uuid;
    let stamp = fx.board.time_stamp();

    // A footprint child gets its parent's frame, and may be replaced by a whole new value.
    let footprint = fx.footprint;
    fx.board
        .modify_pad(pad, |p, env| {
            let frame = env.parent.expect("footprint child");
            assert_eq!(frame.footprint, footprint);
            assert!(frame.flipped);
            *p = Pad::default();
            p.base.item.uuid = uuid;
        })
        .unwrap();

    let base = &fx.board[pad].base.item;
    assert_eq!(base.id(), Some(pad.into()));
    assert_eq!(base.parent, ParentRef::Item(footprint.into()));
    assert_eq!(base.group, Some(group));
    assert!(fx.board.time_stamp() != stamp);
    fx.board.check_invariants().unwrap();

    // A changed UUID is re-bound.
    let new_uuid = Kiid::new(&mut fx.board.kiid_generator);
    fx.board
        .modify_pad(pad, |p, _| p.base.item.uuid = new_uuid)
        .unwrap();
    assert_eq!(fx.board.resolve_item(&new_uuid), Some(pad.into()));
    assert_eq!(fx.board.resolve_item(&uuid), None);
    fx.board.check_invariants().unwrap();

    // The fixture's own replacement helper keeps identity too.
    fx.set_child(pad, Pad::default());
    assert_eq!(fx.board[pad].base.item.uuid, new_uuid);
    fx.board.check_invariants().unwrap();

    // A stale handle is refused.
    let stale = crate::ids::Id::<Pad>::from_parts(77, 3);
    assert!(fx.board.modify_pad(stale, |_, _| ()).is_none());
}

#[test]
fn net_handles_resolve_through_the_net_list() {
    let (mut board, ids) = board_with_tracks();
    let net = NetinfoItem {
        net_code: NetCode(5),
        ..NetinfoItem::default()
    };
    board.net_info.nets.insert(NetCode(5), net);

    let handle = ItemId::net(NetCode(5)).unwrap();
    assert!(matches!(board.get(handle), Some(AnyItem::Net(n)) if n.net_code == NetCode(5)));
    assert!(board.contains(handle));
    assert!(!board.contains(ItemId::net(NetCode(6)).unwrap()));
    assert!(board.modify(handle, |_, _| ()).is_none());

    let ctx = BoardCtx::bare(&board);
    let item = ctx.item(handle).unwrap();
    assert_eq!(item.id(), Some(handle));
    assert_eq!(item.as_net().map(|n| n.net_code), Some(NetCode(5)));
    assert!(item.as_connected().is_none());

    // check_invariants knows the net list.
    board
        .tracks
        .get_mut(ids[0])
        .unwrap()
        .as_track_mut()
        .base
        .net = NetCode(5);
    board.check_invariants().unwrap();
    board
        .tracks
        .get_mut(ids[0])
        .unwrap()
        .as_track_mut()
        .base
        .net = NetCode(6);
    assert!(
        board
            .check_invariants()
            .unwrap_err()
            .contains("unknown net")
    );
}

#[test]
fn track_dispatches_the_virtuals_pcb_track_introduces() {
    let seg = PcbTrack {
        end: Vector2I::new(3_000_000, 4_000_000),
        ..PcbTrack::default()
    };
    let mut track = Track::from(seg);
    assert_eq!(track.width(), 200_000);
    track.set_width(150_000);
    assert_eq!(track.width(), 150_000);
    assert_eq!(track.as_track().width, 150_000);
    assert_eq!(track.length(), 5_000_000.0);

    let mut arc = Track::from(PcbArc::default());
    arc.set_width(1);
    assert_eq!(arc.width(), 1);

    // A via inherits PCB_TRACK::GetLength (start == end).
    assert_eq!(Track::from(PcbVia::default()).length(), 0.0);
}

#[test]
fn time_stamp_wraps() {
    let mut board = Board::new();
    assert_eq!(board.time_stamp(), 1);
    board.time_stamp = i32::MAX;
    board.increment_time_stamp();
    assert_eq!(board.time_stamp(), i32::MIN);
}

#[test]
fn design_settings_mut_bumps_the_time_stamp() {
    let mut board = Board::new();
    let stamp = board.time_stamp();
    let _ = board.design_settings_mut();
    assert!(board.time_stamp() != stamp);
    assert!(board.layer_env_stale);
}
