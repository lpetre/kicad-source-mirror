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

//! Per-board driver and the `item` / `bbox` sections (`dump_board.cpp`).

use kicad_core::{KicadT, Lset, PcbLayerId};

use super::format::{
    Out, attempt, board_items, copper_layers, emit, init_item_keys, no_api, v_bool, v_box,
    v_enum, v_int, v_kiid, v_layer, v_layer_names, v_lset, v_opt_int, v_polyset, v_str, v_vec,
};
use super::{DumpOptions, defaults, items, settings};
use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::item::ParentRef;
use crate::item_ref::{AnyItem, ItemRef};

/// `DumpBoard`: the whole board (keys must be initialised by the caller's `Out`).
pub(crate) fn dump_board_into(o: &mut Out, board: &Board, opts: &DumpOptions) {
    init_item_keys(o, board);
    let ctx = BoardCtx::bare(board);

    if opts.want("board") {
        o.scope("board", "board", |o| {
            o.guard("board", |o| settings::dump_board_section(o, &ctx));
        });
    }

    for id in board_items(board) {
        if let Some(it) = ctx.item(id) {
            dump_item_sections(o, it, opts);
        }
    }

    if opts.want("order") {
        o.scope("order", "board", |o| {
            o.guard("order", |o| settings::dump_order_section(o, &ctx));
        });
    }
}

pub(crate) fn is_dimension(t: KicadT) -> bool {
    matches!(
        t,
        KicadT::PcbDimAlignedT
            | KicadT::PcbDimLeaderT
            | KicadT::PcbDimCenterT
            | KicadT::PcbDimRadialT
            | KicadT::PcbDimOrthogonalT
    )
}

/// A section dumper.
pub(crate) type SectionFn = fn(&mut Out, ItemRef<'_>);

/// `DumpItemSections`.
fn dump_item_sections(o: &mut Out, it: ItemRef<'_>, opts: &DumpOptions) {
    let key = o.item_key(it.id());
    let t = it.type_();
    let section = |o: &mut Out, name: &str, f: SectionFn| {
        if opts.want(name) {
            o.scope(name, &key, |o| o.guard(name, |o| f(o, it)));
        }
    };

    section(o, "item", dump_item_base);
    section(o, "bbox", dump_bbox);

    if matches!(t, KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT) {
        section(o, "track", items::dump_track);
    }

    if t == KicadT::PcbPadT {
        section(o, "pad", items::dump_pad);
    }

    if matches!(
        t,
        KicadT::PcbShapeT | KicadT::PcbTextboxT | KicadT::PcbTablecellT
    ) {
        section(o, "shape", items::dump_shape);
    }

    if matches!(
        t,
        KicadT::PcbTextT | KicadT::PcbFieldT | KicadT::PcbTextboxT | KicadT::PcbTablecellT
    ) {
        section(o, "text", items::dump_text);
    }

    if is_dimension(t) {
        section(o, "dim", items::dump_dimension);
        section(o, "text", items::dump_text);
    }

    if t == KicadT::PcbZoneT {
        section(o, "zone", items::dump_zone);
    }

    if t == KicadT::PcbFootprintT {
        section(o, "footprint", items::dump_footprint);
    }

    section(o, "misc", defaults::dump_misc);
}

/// `DumpItemBase`.
pub(crate) fn dump_item_base(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let d = it.as_dyn();
    let base = it.base();
    emit!(o, "type", v_enum(it.type_()));
    let in_file = o.uuid_in_file(&base.uuid);
    emit!(
        o,
        "uuid",
        if in_file {
            v_kiid(&base.uuid)
        } else {
            "generated".to_string()
        }
    );
    emit!(o, "uuid_in_file", v_bool(in_file));

    let parent = match base.parent {
        ParentRef::None => "null".to_string(),
        ParentRef::Board => "board".to_string(),
        ParentRef::Item(p) => o.item_key(Some(p)),
    };
    emit!(o, "parent", parent);

    match attempt(|| d.parent_group(ctx)) {
        Ok(g) => {
            let k = o.item_key(g);
            emit!(o, "group", k);
        }
        Err(f) => o.fail("group", f),
    }

    // PCB_GROUP::GetLayer() asserts ("isn't well-defined"); groups have no layer.
    if it.type_() != KicadT::PcbGroupT {
        emit!(o, "layer", v_layer(it.layer()));
    }

    emit!(o, "layerset", v_lset(&it.layer_set()));
    emit!(o, "layers", v_layer_names(&it.layer_set()));
    emit!(o, "position", v_vec(it.position()));
    emit!(o, "fp_relative_position", v_vec(it.fp_relative_position()));
    emit!(o, "locked", v_bool(it.is_locked()));
    emit!(o, "locked_flag", v_bool(base.is_locked));
    emit!(o, "on_copper", v_bool(d.is_on_copper_layer(ctx)));
    emit!(o, "has_hole", v_bool(it.has_hole()));
    emit!(o, "has_drilled_hole", v_bool(d.has_drilled_hole(ctx)));
    emit!(o, "knockout", v_bool(d.is_knockout()));
    emit!(o, "side_specific", v_bool(d.is_side_specific(ctx)));
    emit!(o, "layer_agnostic", v_bool(d.is_layer_agnostic()));

    let connected = attempt(|| it.is_connected());

    match connected {
        Ok(c) => {
            emit!(o, "connected_type", v_bool(c));

            if c {
                let bci = it.as_connected();
                emit!(o, "net", v_int(it.net_code().expect("connected item").0));
                emit!(o, "netname", v_str(it.netname()));
                emit!(
                    o,
                    "local_ratsnest_visible",
                    v_bool(
                        bci.and_then(|b| b.connected())
                            .expect("connected item")
                            .local_ratsnest_visible
                    )
                );
                emit!(
                    o,
                    "local_clearance",
                    v_opt_int(bci.expect("connected item").local_clearance())
                );
            }
        }
        Err(f) => o.fail(
            "connected_type|net|netname|local_ratsnest_visible|local_clearance",
            f,
        ),
    }

    for (k, v) in &base.custom_properties {
        emit!(o, format!("custom_property.{k}"), v_str(v));
    }
}

/// `DumpBBox`.
pub(crate) fn dump_bbox(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    emit!(o, "bbox", v_box(it.bounding_box()));

    match it.any() {
        AnyItem::Footprint(fp) => {
            emit!(o, "bbox_with_text", v_box(fp.bounding_box_with_text(ctx, true)));
            emit!(
                o,
                "bbox_without_text",
                v_box(fp.bounding_box_with_text(ctx, false))
            );
            emit!(
                o,
                "fp_pads_local_bbox",
                no_api("FOOTPRINT::GetFpPadsLocalBbox")
            );
            emit!(o, "bounding_hull", v_polyset(&fp.bounding_hull(ctx)));
            emit!(
                o,
                "layer_bbox.F_Cu",
                v_box(fp.layer_bounding_box(ctx, &Lset::from_layers(&[PcbLayerId::FCu])))
            );
            emit!(
                o,
                "layer_bbox.B_Cu",
                v_box(fp.layer_bounding_box(ctx, &Lset::from_layers(&[PcbLayerId::BCu])))
            );
        }
        AnyItem::Pad(pad) => {
            for l in copper_layers(item_copper_count(it)) {
                if let Some(true) = o.cond(&format!("bbox.{}", v_layer(l)), || pad.is_on_layer(ctx, l)) {
                    emit!(
                        o,
                        format!("bbox.{}", v_layer(l)),
                        no_api("PAD::GetBoundingBox (aLayer)")
                    );
                }
            }

            emit!(o, "bounding_radius", v_int(pad.bounding_radius(ctx)));
        }
        AnyItem::Track(t) if t.as_via().is_some() => {
            for l in copper_layers(item_copper_count(it)) {
                if let Some(true) = o.cond(&format!("bbox.{}", v_layer(l)), || it.is_on_layer(l)) {
                    emit!(
                        o,
                        format!("bbox.{}", v_layer(l)),
                        no_api("PCB_VIA::GetBoundingBox (aLayer)")
                    );
                }
            }
        }
        _ => {}
    }
}

/// `aItem->GetBoard() ? GetCopperLayerCount() : 2` (the argument of `CopperLayers`).
pub(crate) fn item_copper_count(it: ItemRef<'_>) -> Option<i32> {
    it.has_board().then(|| it.ctx.board.copper_layer_count())
}
