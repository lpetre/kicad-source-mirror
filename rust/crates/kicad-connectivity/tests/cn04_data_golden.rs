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

//! CN-04 replay of the reference pcb-dump goldens through the `CONNECTIVITY_DATA` facade
//! (`ConnectivityData::build_connectivity` = `BOARD::BuildConnectivity`):
//!
//! - `rn_net` (ratsnest per net, unconnected counts) and `cn_dangling`
//!   (`TestTrackEndpointDangling` with and without `aIgnoreTracksInPads`) of the
//!   `connectivity` section;
//! - `flash.<layer>` of the `track` / `pad` sections: `PCB_VIA::FlashLayer` /
//!   `PAD::FlashLayer`, which ask `CONNECTIVITY_DATA::IsConnectedOnLayer` for conditionally
//!   flashed layers.
//!
//! Boards: the connectivity goldens of `gen_goldens.sh` (flash lines in
//! `tests/golden/cn04_flash/`) and the synthetic boards of `tests/oracle/gen_cn04.py`
//! (`tests/golden/cn04/`, generator `gen_cn04.sh`).
//!
//! Also ports `qa/tests/pcbnew/test_issue25621_via_flashing_contact_layer.cpp`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kicad_connectivity::data::{ConnectivityData, EXCLUDE_ZONES, IGNORE_NETS};
use kicad_connectivity::edge::CnEdge;
use kicad_connectivity::items::CnAnchor;
use kicad_core::PCB_IU_SCALE;
use kicad_core::lset::Lset;
use kicad_core::{KicadT, Kiid, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_pcb::Board;
use kicad_pcb::board::AddMode;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::{ItemId, NetCode};
use kicad_pcb::io::{ParseOptions, parse_board_report};

/// `section|key|field=value` lines of one golden: (section, key) -> (field -> value).
struct Golden {
    map: HashMap<(String, String), BTreeMap<String, String>>,
}

impl Golden {
    fn read(name: &str) -> Golden {
        let path = format!("{}/tests/golden/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        Golden::parse(&text)
    }

    fn parse(text: &str) -> Golden {
        let mut map: HashMap<(String, String), BTreeMap<String, String>> = HashMap::new();

        for line in text.lines() {
            let mut parts = line.splitn(3, '|');
            let (Some(section), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };

            let Some((field, value)) = rest.split_once('=') else {
                continue;
            };

            map.entry((section.to_string(), key.to_string()))
                .or_default()
                .insert(field.to_string(), value.to_string());
        }

        Golden { map }
    }

    fn get(&self, section: &str, key: &str) -> Option<&BTreeMap<String, String>> {
        self.map.get(&(section.to_string(), key.to_string()))
    }

    fn section(&self, section: &str) -> Vec<(&String, &BTreeMap<String, String>)> {
        let mut v: Vec<_> = self
            .map
            .iter()
            .filter(|((s, _), _)| s == section)
            .map(|((_, k), f)| (k, f))
            .collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    }
}

/// Parses a board completely; `None` (with a message) when the parser reached a model stub or
/// skipped footprints / zones.
fn load_path(name: &str, path: &std::path::Path) -> Option<Board> {
    let text = std::fs::read_to_string(path).unwrap();
    let options = ParseOptions {
        allow_pending: true,
    };
    let parsed = std::panic::catch_unwind(|| parse_board_report(&text, "", &options));
    let (board, report) = match parsed {
        Ok(r) => r.unwrap_or_else(|e| panic!("{name}: {}", e.what())),
        Err(e) => {
            let msg = panic_message(&e);
            assert!(
                msg.contains("port pending"),
                "{name}: parser panicked: {msg}"
            );
            eprintln!("{name}: not compared, the parser reached a model stub: {msg}");
            return None;
        }
    };

    for s in &report.skipped {
        match s.token.as_str() {
            "footprint" | "module" | "zone" => {
                eprintln!("{name}: not compared, the parser skipped `{}`", s.token);
                return None;
            }
            "embedded_fonts" | "net_class" | "zone_defaults" | "generated" => {}
            t => panic!("{name}: parser skipped `{t}`: extend the filter"),
        }
    }

    Some(board)
}

fn panic_message(e: &Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[derive(Default)]
struct Stats {
    fields: usize,
    mismatches: Vec<String>,
}

impl Stats {
    fn check(&mut self, what: String, rust: String, cpp: &str) {
        self.fields += 1;

        if rust != cpp {
            self.mismatches
                .push(format!("{what}:\n  rust {rust}\n  c++  {cpp}"));
        }
    }
}

/// The dump's item key of a board item (its UUID; the boards compared here have unique ones).
fn item_key(board: &Board, id: ItemId) -> String {
    BoardCtx::bare(board)
        .item(id)
        .unwrap()
        .uuid()
        .as_std_string()
}

fn layer_by_name(name: &str) -> PcbLayerId {
    PcbLayerId(Lset::name_to_layer(name))
}

/// Compares everything the golden holds for a board loaded and built by the facade.
fn check_board(name: &str, golden: &Golden, mut board: Board, stats: &mut Stats) {
    let mut data = ConnectivityData::new();
    assert!(data.build_connectivity(&mut board), "{name}: build");

    check_ratsnest(name, golden, &board, &mut data, stats);
    check_dangling(name, golden, &board, &data, stats);
    check_flash(name, golden, &board, &data, stats);
}

/// `rn_net` lines through the facade (`GetRatsnestForNet`, `GetEdges`,
/// `GetUnconnectedCount`).
fn check_ratsnest(
    name: &str,
    golden: &Golden,
    board: &Board,
    data: &mut ConnectivityData,
    stats: &mut Stats,
) {
    if golden.get("rn_net", "board").is_none() {
        return;
    }

    let ctx = BoardCtx::bare(board);

    // the dump's item refs, by list position (cn_item lines when present, else UUIDs)
    let mut golden_refs: Vec<String> = Vec::new();

    for n in 0.. {
        let Some(g) = golden.get("cn_item", &format!("#{n}")) else {
            break;
        };

        golden_refs.push(g["ref"].clone());
    }

    if golden_refs.is_empty() {
        golden_refs = data
            .item_list()
            .iter()
            .map(|item| {
                let r = ctx.item(item.parent()).unwrap().uuid().as_std_string();

                match item.zone_layer() {
                    Some(zl) => format!("{r}/{}/{}", Lset::name(zl.layer()), zl.subpoly_index()),
                    None => r,
                }
            })
            .collect();
    }

    let net_count = data.net_count();
    let mut rust_keys = BTreeSet::new();

    for code in 0..=net_count {
        let Some(node_count) = data.ratsnest_for_net(code).map(|n| n.node_count()) else {
            continue;
        };

        if node_count == 0 {
            continue;
        }

        let key = format!("n{code}");
        rust_keys.insert(key.clone());

        let Some(g) = golden.get("rn_net", &key) else {
            continue;
        };

        let what = format!("{name} rn_net {key}");
        let list = data.item_list();

        let anchor_ref = |a: &CnAnchor| {
            let r = match a.item() {
                None => "null".to_string(),
                Some(id) if !list.item(id).valid() => "invalid".to_string(),
                Some(id) => golden_refs[list.item(id).list_index() as usize].clone(),
            };
            format!("{},{}@{r}", a.pos().x, a.pos().y)
        };

        let nodes: Vec<String> = data
            .ratsnest_for_net(code)
            .unwrap()
            .nodes()
            .map(|r| anchor_ref(&list.item(r.item).anchors()[r.index as usize]))
            .collect();

        stats.check(
            format!("{what} node_count"),
            node_count.to_string(),
            &g["node_count"],
        );
        stats.check(
            format!("{what} nodes"),
            format!("[{}]", nodes.join(";")),
            &g["nodes"],
        );

        let edges: Vec<CnEdge> = data.net_edges(code).to_vec();
        let list = data.item_list();
        let anchor_ref = |a: &CnAnchor| {
            let r = match a.item() {
                None => "null".to_string(),
                Some(id) if !list.item(id).valid() => "invalid".to_string(),
                Some(id) => golden_refs[list.item(id).list_index() as usize].clone(),
            };
            format!("{},{}@{r}", a.pos().x, a.pos().y)
        };
        let edges: Vec<String> = edges
            .iter()
            .map(|e| {
                format!(
                    "{}>{}:{}:{}",
                    anchor_ref(e.source_anchor(list).unwrap()),
                    anchor_ref(e.target_anchor(list).unwrap()),
                    e.weight(),
                    e.is_visible()
                )
            })
            .collect();
        stats.check(
            format!("{what} edges"),
            format!("[{}]", edges.join(";")),
            &g["edges"],
        );
    }

    let golden_keys: BTreeSet<String> = golden
        .section("rn_net")
        .into_iter()
        .map(|(k, _)| k.clone())
        .filter(|k| k != "board")
        .collect();
    stats.check(
        format!("{name} rn_net keys"),
        format!("{rust_keys:?}"),
        &format!("{golden_keys:?}"),
    );

    let g = golden.get("rn_net", "board").unwrap();
    stats.check(
        format!("{name} unconnected_count"),
        data.unconnected_count(false).to_string(),
        &g["unconnected_count"],
    );
    stats.check(
        format!("{name} unconnected_visible"),
        data.unconnected_count(true).to_string(),
        &g["unconnected_visible"],
    );
}

/// `cn_dangling` lines: `TestTrackEndpointDangling( track, false / true, &pos )` for every
/// track, arc and via.
fn check_dangling(
    name: &str,
    golden: &Golden,
    board: &Board,
    data: &ConnectivityData,
    stats: &mut Stats,
) {
    let ctx = BoardCtx::bare(board);

    for (track_id, _) in board.tracks() {
        let key = item_key(board, track_id.into());

        let Some(g) = golden.get("cn_dangling", &key) else {
            continue;
        };

        for (suffix, ignore) in [("", false), (".ignore_in_pads", true)] {
            let (dangling, pos) =
                data.test_track_endpoint_dangling(&ctx, track_id.into(), ignore, None);
            let pos = pos.map_or("unset".to_string(), |p| format!("{},{}", p.x, p.y));

            stats.check(
                format!("{name} cn_dangling {key} dangling{suffix}"),
                dangling.to_string(),
                &g[&format!("dangling{suffix}")],
            );
            stats.check(
                format!("{name} cn_dangling {key} pos{suffix}"),
                pos,
                &g[&format!("pos{suffix}")],
            );
        }
    }
}

/// `flash.<layer>` lines of vias and pads: `FlashLayer( layer )` with this connectivity.
fn check_flash(
    name: &str,
    golden: &Golden,
    board: &Board,
    data: &ConnectivityData,
    stats: &mut Stats,
) {
    let ctx = BoardCtx::bare(board).with_connectivity(data);

    for section in ["track", "pad"] {
        for (key, fields) in golden.section(section) {
            let Some(id) = Kiid::parse(key).and_then(|k| board.resolve_item(&k)) else {
                stats.check(format!("{name} {section} {key}"), "missing".into(), "");
                continue;
            };

            let item = ctx.item(id).unwrap();

            for (field, value) in fields {
                let Some(layer) = field.strip_prefix("flash.") else {
                    continue;
                };

                let layer = layer_by_name(layer);
                let rust = if let Some(via) = item.as_via() {
                    via.flash_layer(&ctx, layer)
                } else if let Some(pad) = item.as_pad() {
                    pad.flash_layer(&ctx, layer)
                } else {
                    continue;
                };

                stats.check(
                    format!("{name} {section} {key} {field}"),
                    rust.to_string(),
                    value,
                );
            }
        }
    }
}

/// The connectivity goldens of `gen_goldens.sh` (hashed goldens are left out).
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
    "issue10697",
    "issue18142",
    "issue19574",
    "issue2512",
    "teardrop_multi_inside_via",
    "teardrop_offcenter_two_segment",
    "tuning_generators_load_save",
    "two_segment_teardrop",
    "via_stack_track_width",
    "zone_thieving_track_bisection",
    "bad_triangulation_case",
];

/// The synthetic boards of `gen_cn04.py`.
const SYNTHETIC: &[&str] = &[
    "cn04_flash",
    "cn04_dangling",
    "cn04_random1",
    "cn04_random2",
];

fn report(what: &str, compared: &[&str], stats: &Stats) {
    eprintln!(
        "{what}: {} boards compared ({}), {} fields, {} mismatches",
        compared.len(),
        compared.join(" "),
        stats.fields,
        stats.mismatches.len()
    );
    assert!(
        stats.mismatches.is_empty(),
        "{} mismatches:\n{}",
        stats.mismatches.len(),
        stats.mismatches.join("\n")
    );
}

#[test]
fn cn04_golden_boards_match_cpp() {
    let mut stats = Stats::default();
    let mut compared = Vec::new();

    for &name in BOARDS {
        let mut golden = Golden::read(name);
        let flash = Golden::read(&format!("cn04_flash/{name}"));
        golden.map.extend(flash.map);

        let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));
        let Some(board) = load_path(name, &path) else {
            continue;
        };

        check_board(name, &golden, board, &mut stats);
        compared.push(name);
    }

    report("cn04 golden boards", &compared, &stats);
}

#[test]
fn cn04_synthetic_boards_match_cpp() {
    let mut stats = Stats::default();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cn04");

    for &name in SYNTHETIC {
        let golden = Golden::read(&format!("cn04/{name}"));
        let board = load_path(name, &dir.join(format!("{name}.kicad_pcb")))
            .unwrap_or_else(|| panic!("{name}: synthetic board does not load"));
        check_board(name, &golden, board, &mut stats);
    }

    report("cn04 synthetic boards", SYNTHETIC, &stats);
}

/// Self-consistency of the queries that have no dump line (no C++ comparison): the item
/// lists agree with the connectivity graph and with each other.
#[test]
fn cn04_queries_are_consistent() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cn04");
    let mut checked = 0;

    for &name in SYNTHETIC {
        let mut board = load_path(name, &dir.join(format!("{name}.kicad_pcb"))).unwrap();
        let mut data = ConnectivityData::new();
        data.build_connectivity(&mut board);
        assert!(!data.is_stale(&board), "{name}: stamp recorded");

        let ctx = BoardCtx::bare(&board);

        // GetUnconnectedCount, RunOnUnconnectedEdges, the client edge list, GetNodeCount
        let total = data.unconnected_count(false);
        let mut visited = 0;
        data.run_on_unconnected_edges(|_, _| {
            visited += 1;
            true
        });
        assert_eq!(visited, total);
        assert_eq!(data.unconnected_edges().len() as u32, total, "{name}");

        let all_nodes: u32 = (0..=data.net_count())
            .filter_map(|n| data.ratsnest_for_net(n).map(|r| r.node_count()))
            .sum();
        assert_eq!(data.node_count_of(-1), all_nodes, "{name}");

        for edge in data.unconnected_edges() {
            // GetRatsnestForPad only compares the edge ends' parents with the item
            assert!(
                !data.ratsnest_for_pad(&ctx, edge.source_item).is_empty(),
                "{name}: the edge ends on its source item"
            );
            let both = data.ratsnest_for_items(&ctx, &[edge.source_item, edge.target_item]);
            assert!(!both.is_empty(), "{name}: the edge joins its two items");
        }

        let tracks: Vec<ItemId> = board.tracks().map(|(id, _)| id.into()).collect();

        for &id in &tracks {
            let net = ctx.item(id).unwrap().net_code().unwrap().0;
            let connected_tracks = data.connected_tracks(id);
            assert!(connected_tracks.windows(2).all(|w| w[0] < w[1]));

            // every connected track of the same net is in the item's cluster
            let cluster = data.connected_items(&ctx, id, 0);

            if net > 0 {
                assert!(
                    cluster.contains(&id),
                    "{name}: the item is in its own cluster"
                );

                for t in &connected_tracks {
                    if ctx.item(*t).unwrap().net_code().unwrap().0 == net {
                        assert!(cluster.contains(t), "{name}: connected track in cluster");
                    }
                }
            }

            let all = data.connected_items(&ctx, id, IGNORE_NETS | EXCLUDE_ZONES);
            assert!(all.contains(&id) || all.is_empty());

            let (mut pads, mut vias) = (Vec::new(), Vec::new());
            data.connected_pads_and_vias(id, &mut pads, &mut vias);
            assert!(pads.is_empty());

            for v in &vias {
                assert!(connected_tracks.contains(v));
            }

            checked += 1;
        }

        for net in 0..data.net_count() {
            let items = data.net_items(net, &[KicadT::PcbTraceT, KicadT::PcbViaT, KicadT::PcbArcT]);
            assert!(items.windows(2).all(|w| w[0] < w[1]));

            let expected: BTreeSet<ItemId> = tracks
                .iter()
                .copied()
                .filter(|&id| ctx.item(id).unwrap().net_code() == Some(NetCode(net)))
                .collect();
            assert_eq!(items.into_iter().collect::<BTreeSet<_>>(), expected);
        }

        for (zone, _) in board.zones() {
            for layer in [PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::BCu] {
                for island in data.zone_island_connections(zone.into(), layer) {
                    for item in island {
                        assert!(ctx.item(item).is_some());
                    }
                }
            }
        }
    }

    assert!(checked > 500, "{checked}");
}

/// Port of `qa/tests/pcbnew/test_issue25621_via_flashing_contact_layer.cpp`.
mod issue25621 {
    use super::*;
    use kicad_pcb::OwnedItem;
    use kicad_pcb::ctx::MutEnv;
    use kicad_pcb::pad::Pad;
    use kicad_pcb::padstack::{PadAttrib, Padstack, PadstackMode, UnconnectedLayerMode};
    use kicad_pcb::pcb_track::{PcbTrack, PcbVia, Track, Viatype};
    #[allow(unused_imports)]
    use kicad_pcb::traits::{BoardItem, BoardItemMut};

    fn setup_four_layer_board(board: &mut Board) {
        board.set_copper_layer_count(4);
        let layers = board.enabled_layers() | Lset::all_cu_mask_n(4);
        board.set_enabled_layers(layers);
    }

    fn add_through_via(board: &mut Board, net: NetCode, pos: Vector2I) -> ItemId {
        let env = MutEnv::detached();
        let mut via = PcbVia::new(&mut board.kiid_generator);

        via.set_via_type(&env, Viatype::Through);
        via.set_padstack_mode(PadstackMode::Normal);
        via.set_position(&env, pos);
        via.set_layer_pair(&env, PcbLayerId::FCu, PcbLayerId::BCu);
        via.set_width(Padstack::ALL_LAYERS, PCB_IU_SCALE.mm_to_iu(0.6));
        via.set_drill(PCB_IU_SCALE.mm_to_iu(0.3));
        via.padstack_mut()
            .set_unconnected_layer_mode(UnconnectedLayerMode::RemoveExceptStartAndEnd);

        let id: ItemId = board
            .add_track(Track::Via(Box::new(via)), AddMode::Append)
            .unwrap()
            .into();
        board.set_item_net(id, net, false);
        id
    }

    #[test]
    fn via_touching_pad_on_front_only_has_no_inner_rings() {
        let mut board = Board::new();
        setup_four_layer_board(&mut board);
        let net = board.find_or_add_net("GND", Some(NetCode(1)));

        let fp = board.create_footprint();
        let env = MutEnv::detached();
        let mut pad = Pad::new(&mut board.kiid_generator);

        let outer_diameter = PCB_IU_SCALE.mm_to_iu(5.0);
        let inner_diameter = PCB_IU_SCALE.mm_to_iu(3.0);
        let hole_diameter = PCB_IU_SCALE.mm_to_iu(2.5);

        pad.set_attribute(PadAttrib::Pth);
        pad.set_layer_set(&env, Lset::all_cu_mask().clone());
        let mut padstack = pad.lib_padstack().clone();
        padstack.set_mode(PadstackMode::FrontInnerBack);
        pad.set_padstack(&padstack);
        pad.set_size(
            &env,
            PcbLayerId::FCu,
            Vector2I::new(outer_diameter, outer_diameter),
        );
        pad.set_size(
            &env,
            Padstack::INNER_LAYERS,
            Vector2I::new(inner_diameter, inner_diameter),
        );
        pad.set_size(
            &env,
            PcbLayerId::BCu,
            Vector2I::new(outer_diameter, outer_diameter),
        );
        pad.set_drill_size(&env, Vector2I::new(hole_diameter, hole_diameter));
        pad.set_position(&env, Vector2I::new(0, 0));
        pad.set_unconnected_layer_mode(UnconnectedLayerMode::RemoveExceptStartAndEnd);
        pad.base_mut().parent = kicad_pcb::item::ParentRef::Item(fp.into());

        let pad_id = board
            .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
            .ok()
            .unwrap();
        board.attach_footprint(fp, AddMode::Append);
        board.set_item_net(pad_id, net, false);

        // 2.2 mm puts the via inside the 5 mm front copper and 0.4 mm clear of the 3 mm inner
        // copper
        let via = add_through_via(
            &mut board,
            net,
            Vector2I::new(PCB_IU_SCALE.mm_to_iu(2.2), 0),
        );

        let mut data = ConnectivityData::new();
        data.build_connectivity(&mut board);

        let ctx = BoardCtx::bare(&board).with_connectivity(&data);
        let via_ref = ctx.item(via).unwrap();
        assert!(
            data.is_connected_on_layer(&ctx, via_ref, PcbLayerId::FCu.0, &[KicadT::PcbPadT]),
            "the via and the pad do not touch on F.Cu, so the rest of this test proves nothing"
        );

        let v = via_ref.as_via().unwrap();
        let p = ctx.item(pad_id).unwrap().as_pad().unwrap();

        assert!(
            v.flash_layer(&ctx, PcbLayerId::FCu),
            "the via lost its ring on its start layer"
        );
        assert!(
            v.flash_layer(&ctx, PcbLayerId::BCu),
            "the via lost its ring on its end layer"
        );
        assert!(
            p.flash_layer(&ctx, PcbLayerId::FCu),
            "the pad lost its front copper"
        );

        assert!(
            !v.flash_layer(&ctx, PcbLayerId::In1Cu),
            "the via has a ring on In1.Cu where it touches nothing"
        );
        assert!(
            !v.flash_layer(&ctx, PcbLayerId::In2Cu),
            "the via has a ring on In2.Cu where it touches nothing"
        );
        assert!(
            !p.flash_layer(&ctx, PcbLayerId::In1Cu),
            "the pad has copper on In1.Cu where it touches nothing"
        );
        assert!(
            !p.flash_layer(&ctx, PcbLayerId::In2Cu),
            "the pad has copper on In2.Cu where it touches nothing"
        );
    }

    #[test]
    fn via_keeps_the_ring_on_the_layer_its_track_is_on() {
        let mut board = Board::new();
        setup_four_layer_board(&mut board);
        let net = board.find_or_add_net("GND", Some(NetCode(1)));

        let via = add_through_via(&mut board, net, Vector2I::new(0, 0));

        let env = MutEnv::detached();
        let mut track = PcbTrack::new(&mut board.kiid_generator);
        track.base.item.layer = PcbLayerId::In1Cu;
        track.set_start(Vector2I::new(0, 0));
        track.set_end(Vector2I::new(PCB_IU_SCALE.mm_to_iu(5.0), 0));
        track.set_width(PCB_IU_SCALE.mm_to_iu(0.25));
        let _ = &env;

        let track_id: ItemId = board
            .add_track(Track::Trace(track), AddMode::Append)
            .unwrap()
            .into();
        board.set_item_net(track_id, net, false);

        let mut data = ConnectivityData::new();
        data.build_connectivity(&mut board);

        let ctx = BoardCtx::bare(&board).with_connectivity(&data);
        let via_ref = ctx.item(via).unwrap();
        assert!(
            data.is_connected_on_layer(&ctx, via_ref, PcbLayerId::In1Cu.0, &[KicadT::PcbTraceT]),
            "the via and the track do not touch on In1.Cu, so this test proves nothing"
        );

        let v = via_ref.as_via().unwrap();
        assert!(
            v.flash_layer(&ctx, PcbLayerId::In1Cu),
            "the via lost the ring on the layer its track is on"
        );
        assert!(
            !v.flash_layer(&ctx, PcbLayerId::In2Cu),
            "the via has a ring on In2.Cu where it touches nothing"
        );
    }
}

/// Builds the connectivity of every corpus board the parser loads (qa/data/pcbnew and the
/// demos) and reports the time per board and any panic. Slow: run with `--ignored`.
#[test]
#[ignore]
fn cn04_corpus_build() {
    let mut files: Vec<std::path::PathBuf> = Vec::new();

    for dir in [kicad_qa::pcbnew_data_dir(), kicad_qa::demos_dir()] {
        let mut stack = vec![dir];

        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };

            for e in rd.flatten() {
                let p = e.path();

                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
                    files.push(p);
                }
            }
        }
    }

    files.sort();

    let (mut built, mut skipped, mut panics) = (0, 0, Vec::new());
    let mut total = std::time::Duration::ZERO;

    for path in &files {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };

        let options = ParseOptions {
            allow_pending: true,
        };

        let parsed = std::panic::catch_unwind(|| parse_board_report(&text, "", &options));
        let Ok(Ok((mut board, _))) = parsed else {
            skipped += 1;
            continue;
        };

        let start = std::time::Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut data = ConnectivityData::new();
            data.build_connectivity(&mut board);
            let ctx = BoardCtx::bare(&board);
            let mut dangling = 0;

            for (id, _) in board.tracks() {
                if data
                    .test_track_endpoint_dangling(&ctx, id.into(), true, None)
                    .0
                {
                    dangling += 1;
                }
            }

            (data.unconnected_count(false), dangling)
        }));
        let elapsed = start.elapsed();

        match result {
            Ok((unconnected, dangling)) => {
                built += 1;
                total += elapsed;
                eprintln!(
                    "{name}: {:.1} ms, {} tracks, {unconnected} unconnected, {dangling} dangling",
                    elapsed.as_secs_f64() * 1000.0,
                    board.tracks().count()
                );
            }
            Err(e) => {
                let msg = panic_message(&e);
                eprintln!("{name}: PANIC {msg}");
                panics.push(format!("{name}: {msg}"));
            }
        }
    }

    eprintln!(
        "cn04 corpus: {} files, {built} built in {:.1} ms, {skipped} not parsed, {} panics",
        files.len(),
        total.as_secs_f64() * 1000.0,
        panics.len()
    );

    for p in &panics {
        eprintln!("  {p}");
    }
}
