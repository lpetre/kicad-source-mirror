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

//! CN-02 replay of the reference pcb-dump `connectivity` goldens (`tests/golden/*.txt`,
//! `docs/PCB_DUMP_FORMAT.md` section 8) against `CnConnectivityAlgo`:
//!
//! * the load path (`CONNECTIVITY_DATA::Build`: `Build`, `PropagateNets`, `GetClusters`),
//! * `cn_item`: `connected` (as list positions), `net` (after propagation and the deferred
//!   via-net rule), `zone_single_connection`,
//! * `cn_cluster`: `ratsnest#k` / `propagate#k` (origin net and name, orphaned, conflicting,
//!   members in order),
//! * `cn_island`: `FillIsolatedIslandsMap` for the map `ZONE_FILLER::Fill` builds.
//!
//! Boards whose parse skipped footprints or zones (parser packages in progress) are compared
//! only where the missing items cannot matter: `connected` restricted to the items both sides
//! have, and the propagation clusters whose golden members all exist on the Rust side. Every
//! other field is compared on fully parsed boards only. The anchor fields
//! (`anchor_connected_count`, `anchor_dangling`) need the ratsnest (CN-03).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use kicad_connectivity::algo::{ClusterSearchMode, CnConnectivityAlgo, IsolatedIslandsMap};
use kicad_connectivity::items::CnItem;
use kicad_core::lset::Lset;
use kicad_core::{KicadT, PcbLayerId};
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::{ParseOptions, parse_board_report};
use kicad_pcb::teardrop_parameters::TeardropType;
use kicad_pcb::zone::IsolatedIslands;

/// `section|key|field=value` lines of one golden: (section, key) -> (field -> value), keys in
/// file order per section.
struct Golden {
    map: HashMap<(String, String), BTreeMap<String, String>>,
}

impl Golden {
    fn read(name: &str) -> Golden {
        let path = format!("{}/tests/golden/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
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
}

/// `BOARD::CacheTriangulation` (`ZONE::CacheTriangulation` is still a model stub).
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

/// Splits a dump list `[a;b;c]` (empty: `[]`).
fn split_list(v: &str) -> Vec<String> {
    let inner = v.trim_start_matches('[').trim_end_matches(']');

    if inner.is_empty() {
        Vec::new()
    } else {
        inner.split(';').map(str::to_string).collect()
    }
}

fn layer_name(l: PcbLayerId) -> String {
    Lset::name(l)
}

/// The dump's `ref` of an item.
fn item_ref(ctx: &BoardCtx<'_>, item: &CnItem) -> String {
    let parent = ctx.item(item.parent()).unwrap();
    let r = parent.uuid().as_std_string();

    match item.zone_layer() {
        Some(zl) => format!("{r}/{}/{}", layer_name(zl.layer()), zl.subpoly_index()),
        None => r,
    }
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
            self.mismatches.push(format!("{what}: rust {rust} != c++ {cpp}"));
        }
    }
}

/// Parses a board; `None` (with a message) when the parser reached a model stub. Returns the
/// board and whether footprints / zones were skipped.
fn load(name: &str) -> Option<(Board, bool, bool)> {
    let path = kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).unwrap();
    let options = ParseOptions {
        allow_pending: true,
    };
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

    let mut drop_pads = false;
    let mut drop_zones = false;

    for s in &report.skipped {
        match s.token.as_str() {
            "footprint" | "module" => drop_pads = true,
            "zone" => drop_zones = true,
            "embedded_fonts" | "net_class" | "zone_defaults" | "generated" => {}
            t => panic!("{name}: parser skipped `{t}`: extend the filter"),
        }
    }

    Some((board, drop_pads, drop_zones))
}

/// The map `ZONE_FILLER::Fill` builds (as the dump's `DumpIslands`): no rule areas, more than
/// two corners, every layer of the layer set of zones that are neither thieving nor teardrop
/// areas.
fn islands_map(ctx: &BoardCtx<'_>) -> IsolatedIslandsMap {
    let mut map = IsolatedIslandsMap::new();

    for (zone_id, zone) in ctx.board.zones() {
        if zone.is_rule_area || zone.num_corners() <= 2 {
            continue;
        }

        zone.layer_set(ctx).run_on_layers(|layer| {
            if !zone.is_copper_thieving() && zone.teardrop_type == TeardropType::TdNone {
                map.entry(zone_id)
                    .or_default()
                    .insert(layer, IsolatedIslands::default());
            }
        });
    }

    map
}

fn check_board(name: &str, stats: &mut Stats) -> Option<bool> {
    let golden = Golden::read(name);
    let (board, drop_pads, drop_zones) = load(name)?;
    let full = !drop_pads && !drop_zones;
    let ctx = BoardCtx::bare(&board);

    // CONNECTIVITY_DATA::Build: Build, then internalRecalculateRatsnest (PropagateNets,
    // GetClusters).
    let mut algo = CnConnectivityAlgo::new();
    algo.build(&ctx);
    algo.propagate_nets(&ctx);
    algo.get_clusters(&ctx);

    // golden item positions (list order) without the kinds the parser could not read, aligned
    // with the Rust list
    let mut golden_items: Vec<&BTreeMap<String, String>> = Vec::new();

    for n in 0.. {
        let Some(g) = golden.get("cn_item", &format!("#{n}")) else {
            break;
        };

        if (drop_pads && g["type"] == "PCB_PAD_T") || (drop_zones && g["type"] == "PCB_ZONE_T") {
            continue;
        }

        golden_items.push(g);
    }

    let list = algo.item_list();
    assert_eq!(list.size() as usize, golden_items.len(), "{name}: item count");

    // golden ref -> Rust list position
    let pos_of: HashMap<&str, usize> = golden_items
        .iter()
        .enumerate()
        .map(|(i, g)| (g["ref"].as_str(), i))
        .collect();
    let to_positions = |refs: &[String]| -> (Vec<usize>, bool) {
        let mut out = Vec::new();
        let mut all = true;

        for r in refs {
            match pos_of.get(r.as_str()) {
                Some(&p) => out.push(p),
                None => all = false,
            }
        }

        (out, all)
    };

    for (n, (item, g)) in list.iter().zip(&golden_items).enumerate() {
        // the alignment itself (items without a unique UUID are keyed by ordinal)
        if !g["ref"].starts_with('#') {
            stats.check(
                format!("{name} cn_item #{n} ref"),
                item_ref(&ctx, item),
                &g["ref"],
            );
        }

        // connected, as list positions (restricted to items both sides have)
        let (mut expected, _) = to_positions(&split_list(&g["connected"]));
        expected.sort_unstable();
        let rust: Vec<usize> = item
            .connected_items()
            .iter()
            .map(|&c| list.item(c).list_index() as usize)
            .collect();
        stats.check(
            format!("{name} cn_item #{n} connected"),
            format!("{rust:?}"),
            &format!("{expected:?}"),
        );

        if full {
            stats.check(
                format!("{name} cn_item #{n} net"),
                item.net().to_string(),
                &g["net"],
            );

            if let Some(v) = g.get("zone_single_connection") {
                stats.check(
                    format!("{name} cn_item #{n} zone_single_connection"),
                    item.has_single_connection(list).to_string(),
                    v,
                );
            }
        }
    }

    // clusters, as the dump computes them after the load
    let ratsnest = algo.search_clusters(&ctx, ClusterSearchMode::Ratsnest);
    let propagate = algo.search_clusters(&ctx, ClusterSearchMode::Propagate);
    let list = algo.item_list();

    let cluster_fields = |cl| {
        let c = algo.cluster(cl);
        let items: Vec<usize> = c
            .items()
            .iter()
            .map(|&i| list.item(i).list_index() as usize)
            .collect();
        (c, items)
    };

    for (label, clusters) in [("ratsnest", &ratsnest), ("propagate", &propagate)] {
        if full {
            for (k, &cl) in clusters.iter().enumerate() {
                let key = format!("{label}#{k}");
                let Some(g) = golden.get("cn_cluster", &key) else {
                    stats.check(format!("{name} cn_cluster {key}"), "present".into(), "absent");
                    continue;
                };
                let (c, items) = cluster_fields(cl);
                let what = format!("{name} cn_cluster {key}");
                stats.check(
                    format!("{what} origin_net"),
                    c.origin_net().to_string(),
                    &g["origin_net"],
                );
                stats.check(
                    format!("{what} origin_net_name"),
                    format!("\"{}\"", c.origin_net_name(&ctx, list)),
                    &g["origin_net_name"],
                );
                stats.check(
                    format!("{what} orphaned"),
                    c.is_orphaned().to_string(),
                    &g["orphaned"],
                );
                stats.check(
                    format!("{what} conflicting"),
                    c.is_conflicting().to_string(),
                    &g["conflicting"],
                );
                let (expected, _) = to_positions(&split_list(&g["items"]));
                stats.check(
                    format!("{what} items"),
                    format!("{items:?}"),
                    &format!("{expected:?}"),
                );
            }

            let next = format!("{label}#{}", clusters.len());
            stats.check(
                format!("{name} cn_cluster count {label}"),
                clusters.len().to_string(),
                &if golden.get("cn_cluster", &next).is_some() {
                    "more".to_string()
                } else {
                    clusters.len().to_string()
                },
            );
        } else if label == "propagate" {
            // Propagation clusters never contain zones; a golden cluster whose members all
            // exist here touches no missing pad, so it must exist unchanged.
            for k in 0.. {
                let key = format!("{label}#{k}");
                let Some(g) = golden.get("cn_cluster", &key) else {
                    break;
                };
                let (expected, all) = to_positions(&split_list(&g["items"]));

                if !all || expected.is_empty() {
                    continue;
                }

                let first = list.at(expected[0]);
                let found = clusters
                    .iter()
                    .copied()
                    .find(|&cl| algo.cluster(cl).contains(first));
                let what = format!("{name} cn_cluster {key}");
                let Some(cl) = found else {
                    stats.check(what, "missing".into(), "present");
                    continue;
                };
                let (c, items) = cluster_fields(cl);
                stats.check(
                    format!("{what} items"),
                    format!("{items:?}"),
                    &format!("{expected:?}"),
                );
                stats.check(
                    format!("{what} orphaned"),
                    c.is_orphaned().to_string(),
                    &g["orphaned"],
                );

                // The deferred via-net rule needs the zones.
                if !drop_zones {
                    stats.check(
                        format!("{what} origin_net"),
                        c.origin_net().to_string(),
                        &g["origin_net"],
                    );

                    for &p in &items {
                        stats.check(
                            format!("{what} net of #{p}"),
                            list.item(list.at(p)).net().to_string(),
                            &golden_items[p]["net"],
                        );
                    }
                }
            }
        }
    }

    // FillIsolatedIslandsMap (re-adds the zones), last as in the dump
    if full {
        let mut map = islands_map(&ctx);
        algo.fill_isolated_islands_map(&ctx, &mut map, false);

        for (zone_id, layers) in &map {
            let zone_key = ctx.item((*zone_id).into()).unwrap().uuid().as_std_string();

            for (layer, isl) in layers {
                let key = format!("{zone_key}/{}", layer_name(*layer));
                let Some(g) = golden.get("cn_island", &key) else {
                    stats.check(format!("{name} cn_island {key}"), "present".into(), "absent");
                    continue;
                };
                let mut isolated = isl.isolated_outlines.clone();
                let mut single = isl.single_connection_outlines.clone();
                isolated.sort_unstable_by(|a, b| b.cmp(a));
                single.sort_unstable();
                let fmt = |v: &[i32]| {
                    format!(
                        "[{}]",
                        v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(";")
                    )
                };
                stats.check(
                    format!("{name} cn_island {key} isolated"),
                    fmt(&isolated),
                    &g["isolated"],
                );
                stats.check(
                    format!("{name} cn_island {key} single_connection"),
                    fmt(&single),
                    &g["single_connection"],
                );
            }
        }
    }

    if !full {
        eprintln!(
            "{name}: parser skipped{}{}: partial comparison",
            if drop_pads { " footprints (pads)" } else { "" },
            if drop_zones { " zones" } else { "" }
        );
    }

    // The net changes reach the caller (pads and zones never change net).
    for (id, _) in algo.take_net_changes() {
        let t = ctx.item(id).unwrap().type_();
        assert!(t != KicadT::PcbPadT && t != KicadT::PcbZoneT, "{name}");
    }

    Some(full)
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
    // added by CN-02 (boards without footprints)
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

#[test]
fn cn02_connectivity_matches_cpp() {
    let mut stats = Stats::default();
    let mut boards = 0;
    let mut full = Vec::new();

    for name in BOARDS {
        if let Some(f) = check_board(name, &mut stats) {
            boards += 1;

            if f {
                full.push(*name);
            }
        }
    }

    eprintln!(
        "cn02: {boards} of {} boards compared ({} fully: {}), {} fields, {} mismatches",
        BOARDS.len(),
        full.len(),
        full.join(" "),
        stats.fields,
        stats.mismatches.len()
    );
    assert!(
        stats.mismatches.is_empty(),
        "{} mismatches:\n{}",
        stats.mismatches.len(),
        stats.mismatches.join("\n")
    );
    assert!(full.len() >= 2, "too few boards could be compared fully");
}
