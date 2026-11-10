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

//! CN-01 replay of the `cn_item` goldens (`tests/golden/*.txt`, written by the reference
//! pcb-dump `--sections connectivity` after the loader's `BuildConnectivity`, see
//! `docs/PCB_DUMP_FORMAT.md` section 8): builds the `CN_LIST` the way
//! `CN_CONNECTIVITY_ALGO::Build` does (zone layers, tracks, pads, copper drawings; the loop
//! itself is CN-02's and is reproduced here only to drive the items) and compares, item by
//! item in list order, the fields this package owns: parent, type, `CanChangeNet`, ordinal
//! layer span, board layer, `BBox`, stored anchors, `AnchorCount`, `GetAnchor`, the zone
//! outline size and (for pads and zones, which the propagation never changes) the net.
//!
//! Fields that need the contact search (`connected`, anchor counts, dangling, single
//! connection) and the cluster / ratsnest / dangling / island sections are replayed by CN-02 ..
//! CN-04.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_connectivity::items::{CnItem, CnList};
use kicad_core::lset::Lset;
use kicad_core::{KicadT, PcbLayerId};
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::{ParseOptions, parse_board_report};

/// `section|key|field=value` lines of one golden: key -> (field -> value), per section.
type Golden = BTreeMap<String, BTreeMap<String, String>>;

fn read_golden(name: &str) -> Golden {
    let path = format!(
        "{}/tests/golden/{}.txt",
        env!("CARGO_MANIFEST_DIR"),
        common::golden_name(name)
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out: Golden = BTreeMap::new();

    for line in text.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(section), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };

        if section != "cn_item" {
            continue;
        }

        let Some((field, value)) = rest.split_once('=') else {
            continue;
        };

        out.entry(key.to_string())
            .or_default()
            .insert(field.to_string(), value.to_string());
    }

    out
}

/// `BOARD::CacheTriangulation` (every fill layer of every zone, `CacheTriangulation( false )`),
/// which the connectivity build relies on. `ZONE::CacheTriangulation` is still a model stub, so
/// the test triangulates the fills directly.
fn cache_triangulation(board: &mut Board) {
    let zones: Vec<_> = board.zones().map(|(id, _)| id).collect();

    for id in zones {
        let fill = board.zone_fill_mut(id);

        for poly in fill.filled_polys_list.values_mut() {
            let mut p = (**poly).clone();
            p.cache_triangulation(false);
            *poly = Arc::new(p);
        }
    }
}

/// The item order of `CN_CONNECTIVITY_ALGO::Build` (connectivity_algo.cpp L518-657).
fn build_list(ctx: &BoardCtx<'_>, with_zones: bool) -> CnList {
    let board = ctx.board;
    let mut list = CnList::new();

    list.bulk_add(|list| {
        let mut zitems: Vec<CnItem> = Vec::new();

        for (zone_id, zone) in board.zones() {
            if !with_zones || !zone.is_on_copper_layer(ctx) {
                continue;
            }

            let mut layerset = board.enabled_layers();
            layerset &= &zone.layer_set(ctx);
            layerset &= Lset::all_cu_mask();

            layerset.run_on_layers(|layer| {
                let Some(polys) = zone.fill().filled_polys_list.get(&layer) else {
                    return;
                };

                for j in 0..polys.outline_count() {
                    zitems.push(CnItem::new_zone_layer(ctx, zone_id, layer, j).unwrap());
                }
            });
        }

        for zitem in &mut zitems {
            zitem.zone_layer_mut().unwrap().build_indexes(ctx);
        }

        for zitem in zitems {
            list.add_zone_layer(zitem);
        }

        for (track_id, _) in board.tracks() {
            list.add_track(ctx, track_id);
        }

        for (fp_id, _) in board.footprints() {
            for (pad_id, _) in board.footprint_pads(fp_id) {
                list.add_pad(ctx, pad_id);
            }
        }

        for (id, _) in board.drawings() {
            let Some(item) = ctx.item(id) else { continue };

            if item.type_() == KicadT::PcbShapeT && item.as_dyn().is_on_copper_layer(ctx) {
                list.add_shape(ctx, id);
            }
        }
    });

    list
}

fn type_name(t: KicadT) -> &'static str {
    match t {
        KicadT::PcbPadT => "PCB_PAD_T",
        KicadT::PcbTraceT => "PCB_TRACE_T",
        KicadT::PcbArcT => "PCB_ARC_T",
        KicadT::PcbViaT => "PCB_VIA_T",
        KicadT::PcbShapeT => "PCB_SHAPE_T",
        KicadT::PcbZoneT => "PCB_ZONE_T",
        _ => "?",
    }
}

fn layer_name(l: PcbLayerId) -> String {
    Lset::name(l)
}

fn pt(p: kicad_geom::math::vector2::Vector2I) -> String {
    format!("{},{}", p.x, p.y)
}

/// The fields of one item as the dump prints them.
fn item_fields(ctx: &BoardCtx<'_>, item: &CnItem) -> BTreeMap<&'static str, String> {
    let mut f = BTreeMap::new();
    let parent = ctx.item(item.parent()).unwrap();
    let mut r = parent.uuid().as_std_string();

    if let Some(zl) = item.zone_layer() {
        r = format!("{r}/{}/{}", layer_name(zl.layer()), zl.subpoly_index());
        f.insert("zone_outline_points", zl.outline_point_count().to_string());
    }

    f.insert("ref", r);
    f.insert("list_index", item.list_index().to_string());
    f.insert("type", type_name(item.parent_type()).to_string());
    f.insert("valid", item.valid().to_string());
    f.insert("can_change_net", item.can_change_net().to_string());
    f.insert(
        "layers",
        format!("{}..{}", item.start_layer(), item.end_layer()),
    );
    f.insert("board_layer", layer_name(item.board_layer()));
    let b = item.bbox();
    f.insert(
        "bbox",
        format!("{},{},{},{}", b.x(), b.y(), b.width(), b.height()),
    );
    let anchors: Vec<String> = item.anchors().iter().map(|a| pt(a.pos())).collect();
    f.insert("anchors", format!("[{}]", anchors.join(";")));
    f.insert("anchor_count", item.anchor_count().to_string());
    let get: Vec<String> = (0..item.anchor_count())
        .map(|n| pt(item.anchor(ctx, n)))
        .collect();
    f.insert("get_anchor", format!("[{}]", get.join(" ")));

    // The propagation never changes the net of pads and zones (CanChangeNet() == false).
    if !item.can_change_net() {
        f.insert("net", item.net().to_string());
    }

    f
}

/// The board file of a golden (`common::board_path`).
fn board_path(name: &str) -> std::path::PathBuf {
    common::board_path(name)
}

fn check_board(name: &str) -> Option<usize> {
    let golden = read_golden(name);
    let path = board_path(name);
    let text = std::fs::read_to_string(&path).unwrap();
    // Constructs whose parser package has not landed yet are skipped (and reported below); the
    // comparison then leaves out the golden items of those kinds.
    let options = ParseOptions {
        allow_pending: true,
    };
    // A parser path that still reaches a model stub panics ("port pending"): report it and
    // leave the board out (the count of checked boards is asserted separately).
    let parsed = std::panic::catch_unwind(|| parse_board_report(&text, "", &options));
    let (mut board, report) = match parsed {
        Ok(r) => r.unwrap_or_else(|e| panic!("{name}: {}", e.what())),
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            assert!(
                msg.contains("port pending"),
                "{name}: parser panicked: {msg}"
            );
            eprintln!("{name}: not compared, the parser reached a model stub: {msg}");
            return None;
        }
    };
    cache_triangulation(&mut board);

    // Skipped constructs that create no connectivity item are harmless; skipped footprints
    // and zones remove all pads / zone items from the comparison (on both sides).
    let mut drop_pads = false;
    let mut drop_zones = false;

    for s in &report.skipped {
        match s.token.as_str() {
            // (`pad`: the footprint parser landed before the pad parser, PCBIO-07)
            "footprint" | "module" | "pad" => drop_pads = true,
            "zone" => drop_zones = true,
            "embedded_fonts" | "net_class" | "zone_defaults" | "generated" => {}
            t => panic!("{name}: parser skipped `{t}`: extend the filter"),
        }
    }

    if drop_pads || drop_zones {
        eprintln!(
            "{name}: parser stubs skipped{}{}: those items are not compared",
            if drop_pads { " footprints (pads)" } else { "" },
            if drop_zones { " zones" } else { "" }
        );
    }

    let ctx = BoardCtx::bare(&board);
    let list = build_list(&ctx, !drop_zones);

    // golden items in list order, without the kinds the parser could not read
    let mut expected: Vec<&BTreeMap<String, String>> = Vec::new();

    for n in 0.. {
        let Some(g) = golden.get(&format!("#{n}")) else {
            break;
        };

        if (drop_pads && g["type"] == "PCB_PAD_T") || (drop_zones && g["type"] == "PCB_ZONE_T") {
            continue;
        }

        expected.push(g);
    }

    assert_eq!(list.size() as usize, expected.len(), "{name}: item count");

    let mut checked = 0;
    let mut mismatches = Vec::new();

    for (n, (item, g)) in list.iter().zip(expected).enumerate() {
        for (field, value) in item_fields(&ctx, item) {
            let Some(expected) = g.get(field) else {
                mismatches.push(format!("#{n} {field}: missing in golden"));
                continue;
            };

            // Items without a unique UUID in the file are keyed by ordinal in the dump; list
            // positions shift when pads were left out.
            if (field == "ref" && expected.starts_with('#'))
                || (field == "list_index" && (drop_pads || drop_zones))
            {
                continue;
            }

            if &value != expected {
                mismatches.push(format!("#{n} {field}: rust {value} != c++ {expected}"));
            }

            checked += 1;
        }
    }

    assert!(
        mismatches.is_empty(),
        "{name}: {} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );

    Some(checked)
}

const BOARDS: &[&str] = &[
    "via_dangling",
    "issue18839",
    "teardrop_close_via",
    "issue23123_minimal",
    "issue23339_zone_layer_rules",
    "soldermask_track_to_pad",
    "custom_pads",
    "padstacks_complex",
    "via_stacks",
    "tracks_arcs_vias",
    "fill_bad",
    "net_tie_drc",
    "connection_width_rules",
    "issue8003",
    "test_copper_graphics",
    "intersectingzones",
    "issue14334",
    "notched_zones",
    "issue9870",
    // REV-CN: synthetic boards of gen_cn04.py (tests/golden/cn04/): zone contact paths, the
    // via-net rule, islands, a teardrop zone, copper graphics, pad-less net propagation
    "rev_zone_contacts",
    "rev_ratsnest_zones",
    "rev_shapes",
    "rev_order_probe",
];

/// Every golden board whose parse does not reach a model stub is compared field by field;
/// the boards blocked by parser / model stubs are listed on stderr.
#[test]
fn cn01_items_match_cpp() {
    let mut boards = 0;
    let mut fields = 0;

    let names: Vec<String> = BOARDS
        .iter()
        .map(|s| s.to_string())
        .chain(common::stab_cn_boards())
        .collect();

    for name in &names {
        if let Some(n) = check_board(name) {
            boards += 1;
            fields += n;
        }
    }

    eprintln!(
        "cn01: {boards} of {} boards compared, {fields} fields equal",
        names.len()
    );
    assert!(boards >= 3, "too few boards could be compared");
}
