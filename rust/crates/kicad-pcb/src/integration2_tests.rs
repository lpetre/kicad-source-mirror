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

//! Integration step 2 (INT-2a): replay of `tests/oracle/int2a_oracle.cpp`, the reference
//! values of the kernel changes that alter behaviour. No C++ file is ported here; the cases
//! exercise `BOARD_CONNECTED_ITEM::SetNet` / `ZONE::SetNet` / `ZONE::SetNetCode` /
//! `BOARD::Remove( net )` (`pcbnew/board_connected_item.h`, `zone.cpp`, `board.cpp`),
//! `BOARD_ITEM::IsSideSpecific` in `Flip` / `Mirror` and `BOARD::IsBackLayer` in
//! `StyleFromSettings` (`pcb_text.cpp`, `pcb_textbox.cpp`, `pcb_dimension.cpp`) on user layers
//! typed `LT_FRONT` / `LT_BACK`, `StyleFromSettings` inside a rotated, scaled footprint,
//! `PCB_DRILL_MAP::GetSymbolExtent` and `BOARD_DESIGN_SETTINGS::operator==`.
//!
//! The golden `tests/golden/int2a_mutators.txt` is regenerated with
//! `tests/oracle/int2a_golden.sh`. Two ported cases of
//! `qa/tests/pcbnew/test_board_connected_items.cpp` are included.

use std::collections::BTreeMap;

use kicad_core::{Lset, PcbLayerId};
use kicad_geom::{EdaAngle, FlipDirection, Vector2I};

use crate::board::{AddMode, Board, LayerT};
use crate::board_design_settings::{
    BoardDesignSettings, LAYER_CLASS_COPPER, LAYER_CLASS_FAB, LAYER_CLASS_OTHERS, LAYER_CLASS_SILK,
};
use crate::ctx::BoardCtx;
use crate::dump::format::{v_angle, v_bool, v_int, v_vec};
use crate::ids::{ItemId, NetCode};
use crate::item::ParentRef;
use crate::netinfo::NetinfoItem;
use crate::pcb_dimension::PcbDimension;
use crate::pcb_drill_map::PcbDrillMap;
use crate::pcb_text::PcbText;
use crate::pcb_textbox::PcbTextbox;
use crate::pcb_track::{PcbTrack, Track};
use crate::teardrop_parameters::TargetTd;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, EdaShapeItem, EdaTextItem};
use crate::zone::Zone;
use crate::zone_settings::ZoneFillMode;

const GOLDEN: &str = include_str!("../tests/golden/int2a_mutators.txt");
const MM: i32 = 1_000_000;
const CENTRE: Vector2I = Vector2I::new(5 * MM, 5 * MM);
const LAYERS: [PcbLayerId; 8] = [
    PcbLayerId::FSilkS,
    PcbLayerId::BSilkS,
    PcbLayerId::User1,
    PcbLayerId::User2,
    PcbLayerId::User3,
    PcbLayerId::DwgsUser,
    PcbLayerId::FCu,
    PcbLayerId::BCu,
];

type Out = BTreeMap<String, String>;

fn p(out: &mut Out, case: &str, field: &str, value: String) {
    out.insert(format!("{case}|{field}"), value);
}

/// `Setup` of the C++ driver.
fn setup(out: &mut Out, board: &mut Board) {
    let front = board.set_layer_type(PcbLayerId::User1, LayerT::Front);
    p(out, "setup", "set_user1_front", v_bool(front));
    let back = board.set_layer_type(PcbLayerId::User2, LayerT::Back);
    p(out, "setup", "set_user2_back", v_bool(back));

    let bds = board.design_settings_mut();
    bds.text_size[LAYER_CLASS_SILK] = Vector2I::new(1_300_000, 1_100_000);
    bds.text_thickness[LAYER_CLASS_SILK] = 170_000;
    bds.text_italic[LAYER_CLASS_SILK] = true;
    bds.text_upright[LAYER_CLASS_SILK] = true;
    bds.line_thickness[LAYER_CLASS_SILK] = 230_000;
    bds.text_size[LAYER_CLASS_OTHERS] = Vector2I::new(900_000, 1_700_000);
    bds.text_thickness[LAYER_CLASS_OTHERS] = 210_000;
    bds.text_italic[LAYER_CLASS_OTHERS] = false;
    bds.text_upright[LAYER_CLASS_OTHERS] = true;
    bds.line_thickness[LAYER_CLASS_OTHERS] = 110_000;
    bds.text_size[LAYER_CLASS_COPPER] = Vector2I::new(1_500_000, 1_400_000);
    bds.text_thickness[LAYER_CLASS_COPPER] = 250_000;
    bds.line_thickness[LAYER_CLASS_COPPER] = 190_000;
}

/// `InitText`.
fn make_text(board: &Board, parent: ParentRef, layer: PcbLayerId) -> PcbText {
    let env = board.child_env(parent);
    let mut t = PcbText::default();
    t.base.parent = parent;
    t.set_layer(&env, layer);
    t.text.set_text("abc");
    t.set_text_pos(&env, Vector2I::new(3 * MM, 4 * MM));
    t.set_text_angle(&env, EdaAngle::from_degrees(30.0));
    t
}

/// `InitTextbox`.
fn make_textbox(
    board: &Board,
    parent: ParentRef,
    layer: PcbLayerId,
    origin: Vector2I,
) -> PcbTextbox {
    let env = board.child_env(parent);
    let mut tb = PcbTextbox::default();
    tb.shape.base.item.parent = parent;
    tb.set_layer(&env, layer);
    tb.set_start(&env, origin);
    tb.set_end(&env, origin + Vector2I::new(10 * MM, 5 * MM));
    tb.text.set_text("abc");
    tb
}

/// `TextStyle`.
fn text_style<H: EdaTextItem>(out: &mut Out, case: &str, ctx: &BoardCtx<'_>, t: &H) {
    let text = t.eda_text();
    p(out, case, "style.mirrored", v_bool(text.is_mirrored()));
    p(out, case, "style.size", v_vec(t.text_size(ctx)));
    p(
        out,
        case,
        "style.lib_size",
        v_vec(crate::eda_text::text_size(t, ctx)),
    );
    p(out, case, "style.thickness", v_int(t.text_thickness(ctx)));
    p(
        out,
        case,
        "style.lib_thickness",
        v_int(crate::eda_text::text_thickness(t, ctx)),
    );
    p(out, case, "style.italic", v_bool(text.is_italic()));
    p(
        out,
        case,
        "style.keep_upright",
        v_bool(text.is_keep_upright()),
    );
}

/// `BoxStyle`.
fn box_style(out: &mut Out, case: &str, ctx: &BoardCtx<'_>, tb: &PcbTextbox) {
    text_style(out, case, ctx, tb);
    p(
        out,
        case,
        "style.stroke_width",
        v_int(BoardItem::stroke(tb, ctx).width()),
    );
    p(
        out,
        case,
        "style.width",
        v_int(EdaShapeItem::width(tb, ctx)),
    );
}

fn board_level_cases(out: &mut Out) {
    let mut board = Board::new();
    setup(out, &mut board);
    let bds: BoardDesignSettings = board.design_settings().clone();
    let ctx = BoardCtx::bare(&board);
    let env = board.child_env(ParentRef::Board);

    for layer in LAYERS {
        let n = layer.0.to_string();

        let c = format!("layer_{n}");
        p(out, &c, "type", v_int(board.layer_type(layer) as i32));
        p(out, &c, "is_back", v_bool(board.is_back_layer(layer)));
        p(out, &c, "flipped", v_int(board.flip_layer(layer).0));

        {
            let c = format!("text_{n}");
            let mut t = make_text(&board, ParentRef::Board, layer);
            p(
                out,
                &c,
                "side_specific",
                v_bool(BoardItem::is_side_specific(&t, &ctx)),
            );
            t.flip(&env, CENTRE, FlipDirection::LeftRight);
            p(out, &c, "flip.mirrored", v_bool(t.text.is_mirrored()));
            p(out, &c, "flip.layer", v_int(BoardItem::layer(&t, &ctx).0));
            p(out, &c, "flip.pos", v_vec(t.text_pos(&ctx)));
            p(out, &c, "flip.angle", v_angle(t.text_angle(&ctx)));

            let mut s = make_text(&board, ParentRef::Board, layer);
            s.style_from_settings(&env, &bds, true);
            text_style(out, &c, &ctx, &s);

            let mut u = make_text(&board, ParentRef::Board, layer);
            u.text.set_mirrored(true);
            u.style_from_settings(&env, &bds, false);
            p(
                out,
                &c,
                "style_nocheck.mirrored",
                v_bool(u.text.is_mirrored()),
            );
        }

        {
            let c = format!("textbox_{n}");
            let origin = Vector2I::new(2 * MM, 3 * MM);
            let mut t = make_textbox(&board, ParentRef::Board, layer, origin);
            p(
                out,
                &c,
                "side_specific",
                v_bool(BoardItem::is_side_specific(&t, &ctx)),
            );
            t.flip(&env, CENTRE, FlipDirection::LeftRight);
            p(out, &c, "flip.mirrored", v_bool(t.text.is_mirrored()));
            p(out, &c, "flip.layer", v_int(BoardItem::layer(&t, &ctx).0));
            p(out, &c, "flip.start", v_vec(t.shape.shape.start()));
            p(out, &c, "flip.end", v_vec(t.shape.shape.end()));

            let mut s = make_textbox(&board, ParentRef::Board, layer, origin);
            s.style_from_settings(&env, &bds, true);
            box_style(out, &c, &ctx, &s);
        }

        {
            let c = format!("dim_{n}");
            let mut d = PcbDimension::default();
            d.text.base.parent = ParentRef::Board;
            BoardItemMut::set_layer(&mut d, &env, layer);
            p(
                out,
                &c,
                "side_specific",
                v_bool(BoardItem::is_side_specific(&d, &ctx)),
            );
            d.mirror(&env, CENTRE, FlipDirection::LeftRight);
            p(
                out,
                &c,
                "mirror.mirrored",
                v_bool(d.text.text.is_mirrored()),
            );

            let mut s = PcbDimension::default();
            s.text.base.parent = ParentRef::Board;
            BoardItemMut::set_layer(&mut s, &env, layer);
            s.style_from_settings(&env, &bds, true);
            p(out, &c, "style.mirrored", v_bool(s.text.text.is_mirrored()));
            p(out, &c, "style.size", v_vec(s.text_size(&ctx)));
            p(out, &c, "style.thickness", v_int(s.text_thickness(&ctx)));
            p(out, &c, "style.line_thickness", v_int(s.line_thickness));
        }
    }
}

fn footprint_cases(out: &mut Out) {
    let mut board = Board::new();
    setup(out, &mut board);

    // MakeFootprint
    let fp = board.create_footprint();
    board.footprint_set_position(fp, Vector2I::new(10 * MM, 20 * MM));
    board.footprint_set_orientation(fp, EdaAngle::from_degrees(30.0));
    board.footprint_set_transform_scale(fp, 1.5, 0.75);
    board.attach_footprint(fp, AddMode::Insert);

    let bds: BoardDesignSettings = board.design_settings().clone();
    let parent = ParentRef::Item(fp.into());

    {
        let ctx = BoardCtx::bare(&board);
        let env = board.child_env(parent);

        for layer in [PcbLayerId::FSilkS, PcbLayerId::User2] {
            let n = layer.0.to_string();

            let mut t = make_text(&board, parent, layer);
            t.style_from_settings(&env, &bds, true);
            text_style(out, &format!("fp_text_{n}"), &ctx, &t);

            let mut b = make_textbox(&board, parent, layer, Vector2I::new(11 * MM, 21 * MM));
            b.style_from_settings(&env, &bds, true);
            box_style(out, &format!("fp_textbox_{n}"), &ctx, &b);
        }
    }

    // fp->Reference().StyleFromSettings( bds, true )
    let reference = board[fp].fields[0];
    board.modify_field(reference, |f, env| f.style_from_settings(env, &bds, true));

    let ctx = BoardCtx::bare(&board);
    let field = &board[reference];
    text_style(out, "fp_reference", &ctx, field);
    p(
        out,
        "fp_reference",
        "layer",
        v_int(BoardItem::layer(field, &ctx).0),
    );
}

fn net_cases(out: &mut Out) {
    let mut board = Board::new();
    let net = NetinfoItem::new(&mut board.kiid_generator, "TestNet", NetCode(1));
    board.add_net(net);

    let env = board.child_env(ParentRef::Board);
    let f_cu = Lset::from_layers(&[PcbLayerId::FCu]);

    let mut thieving = Zone::default();
    thieving.set_layer_set(&env, f_cu.clone());
    thieving.set_fill_mode(ZoneFillMode::CopperThieving);
    let thieving: ItemId = board.add_zone(thieving, AddMode::Insert).into();

    let mut solid = Zone::default();
    solid.set_layer_set(&env, f_cu);
    let solid: ItemId = board.add_zone(solid, AddMode::Insert).into();

    let mut track = PcbTrack::default();
    BoardItemMut::set_layer(&mut track, &env, PcbLayerId::FCu);
    let track: ItemId = board
        .add_track(Track::Trace(track), AddMode::Insert)
        .expect("track on copper")
        .into();

    let code = |board: &Board, id: ItemId| -> String {
        let ctx = BoardCtx::bare(board);
        v_int(
            ctx.item(id)
                .and_then(|i| i.net_code())
                .map_or(i32::MIN, |c| c.0),
        )
    };

    p(out, "nets", "thieving.initial", code(&board, thieving));
    let ok = board.set_item_net(thieving, NetCode(1), true);
    p(out, "nets", "thieving.set_net_code.ok", v_bool(ok));
    p(out, "nets", "thieving.set_net_code", code(&board, thieving));
    board.set_item_netinfo(thieving, NetCode(1));
    p(out, "nets", "thieving.set_net", code(&board, thieving));

    board.set_item_netinfo(solid, NetCode(1));
    p(out, "nets", "solid.set_net", code(&board, solid));
    let ok = board.set_item_net(track, NetCode(1), true);
    p(out, "nets", "track.set_net_code.ok", v_bool(ok));
    p(out, "nets", "track.set_net_code", code(&board, track));
    let ok = board.set_item_net(track, NetCode(7), true);
    p(out, "nets", "track.set_unknown.ok", v_bool(ok));
    p(out, "nets", "track.set_unknown", code(&board, track));
    board.set_item_net(track, NetCode(1), true);

    board.remove_net(NetCode(1));
    p(out, "nets", "removed.thieving", code(&board, thieving));
    p(out, "nets", "removed.solid", code(&board, solid));
    p(out, "nets", "removed.track", code(&board, track));
}

fn drill_map_cases(out: &mut Out) {
    let board = Board::new();
    let ctx = BoardCtx::bare(&board);

    let mut map = PcbDrillMap::default();
    map.base.parent = ParentRef::Board;
    p(out, "drill_map", "extent", v_int(map.symbol_extent(&ctx)));
    map.set_symbol_size(700_000);
    p(
        out,
        "drill_map",
        "extent_700",
        v_int(map.symbol_extent(&ctx)),
    );

    let orphan = PcbDrillMap::default();
    p(
        out,
        "drill_map",
        "extent_no_board",
        v_int(orphan.symbol_extent(&ctx)),
    );
}

fn bds_eq_cases(out: &mut Out) {
    let a = Board::new();
    p(
        out,
        "bds_eq",
        "defaults",
        v_bool(a.design_settings() == Board::new().design_settings()),
    );

    let mut differs = |name: &str, edit: &dyn Fn(&mut Board)| {
        let mut b = Board::new();
        edit(&mut b);
        p(
            out,
            "bds_eq",
            name,
            v_bool(a.design_settings() == b.design_settings()),
        );
    };

    differs("track_width_list", &|b| {
        b.design_settings_mut().track_width_list.push(250_000)
    });
    differs("text_size", &|b| {
        b.design_settings_mut().text_size[LAYER_CLASS_FAB].x += 1
    });
    differs("edge_plating", &|b| {
        b.design_settings_mut().stackup.edge_plating = true
    });
    differs("paste_ratio", &|b| {
        b.design_settings_mut().solder_paste_margin_ratio = -0.1
    });
    differs("max_error", &|b| b.design_settings_mut().max_error += 1);
    differs("style_fp_text", &|b| {
        let s = b.design_settings_mut();
        s.style_fp_text = !s.style_fp_text;
    });
    differs("copper_layer_count", &|b| {
        b.design_settings_mut().set_copper_layer_count(4)
    });
    differs("teardrops", &|b| {
        let params = b
            .design_settings_mut()
            .teardrop_params_list
            .parameters_mut(TargetTd::Round);
        params.enabled = !params.enabled;
    });
}

#[test]
fn int2a_matches_oracle() {
    let mut golden: Out = BTreeMap::new();

    for line in GOLDEN.lines() {
        if let Some((k, v)) = line.split_once('=') {
            golden.insert(k.to_string(), v.to_string());
        }
    }

    let mut got: Out = BTreeMap::new();
    board_level_cases(&mut got);
    footprint_cases(&mut got);
    net_cases(&mut got);
    drill_map_cases(&mut got);
    bds_eq_cases(&mut got);

    let mut diffs = Vec::new();

    for (key, expected) in &golden {
        match got.get(key) {
            Some(value) if value == expected => {}
            Some(value) => diffs.push(format!("{key}: rust {value}, oracle {expected}")),
            None => diffs.push(format!("{key}: missing (oracle {expected})")),
        }
    }

    for key in got.keys() {
        if !golden.contains_key(key) {
            diffs.push(format!("{key}: not in the golden"));
        }
    }

    assert!(
        diffs.is_empty(),
        "{} of {} values differ:\n{}",
        diffs.len(),
        golden.len(),
        diffs.join("\n")
    );
    assert_eq!(golden.len(), 354);
}

/// `ThievingZone_RejectsNetCodeViaBasePointer` and `ThievingZone_RejectsSetNetViaBasePointer`
/// (`qa/tests/pcbnew/test_board_connected_items.cpp`): a copper-thieving zone stays net-less
/// through the base-class `SetNetCode` and `SetNet`.
#[test]
fn thieving_zone_rejects_nets_via_base_pointer() {
    for set_net in [false, true] {
        let mut board = Board::new();
        let net = NetinfoItem::new(&mut board.kiid_generator, "TestNet", NetCode(1));
        board.add_net(net);

        let env = board.child_env(ParentRef::Board);
        let mut zone = Zone::default();
        zone.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::FCu]));
        zone.set_fill_mode(ZoneFillMode::CopperThieving);
        let zone = board.add_zone(zone, AddMode::Insert);

        if set_net {
            // base->SetNet( net ); BOOST_CHECK( zone->GetNet() == nullptr );
            assert!(board.set_item_netinfo(zone.into(), NetCode(1)));
            assert_eq!(board[zone].net_code(), NetCode::ORPHANED);
        } else {
            // base->SetNetCode( 1 ); BOOST_CHECK_EQUAL( zone->GetNetCode(), 0 );
            board.set_item_net(zone.into(), NetCode(1), false);
            assert_eq!(board[zone].net_code(), NetCode(0));
        }

        assert!(board.check_invariants().is_ok());
    }
}
