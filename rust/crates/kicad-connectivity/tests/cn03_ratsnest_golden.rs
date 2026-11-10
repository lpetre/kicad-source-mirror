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

//! CN-03 replay of the `rn_net` lines of the reference pcb-dump `connectivity` goldens
//! (`tests/golden/*.txt`, `docs/PCB_DUMP_FORMAT.md` section 8) against `RnNet`: per net the
//! node list (`m_nodes` multiset order), the node count and the ratsnest edges in `GetEdges()`
//! order (source, target, weight, visible), plus the board's unconnected counts.
//!
//! The ratsnest is built as `CONNECTIVITY_DATA::Build` does it (`internalRecalculateRatsnest`
//! + `updateRatsnest`, sequentially); that facade is CN-04's, so this test drives `RnNet`
//! directly. Only boards the parser loads completely (no skipped footprints or zones) are
//! compared: a missing pad or zone changes every net it is on.
//!
//! Also ports the ratsnest half of `qa/tests/pcbnew/test_connectivity_clusters.cpp`
//! (`checkRatsnestSpansClusters`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use kicad_connectivity::algo::{ClusterSearchMode, CnConnectivityAlgo};
use kicad_connectivity::edge::CnEdge;
use kicad_connectivity::items::{CnAnchor, CnList};
use kicad_connectivity::ratsnest::RnNet;
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::NetCode;
use kicad_pcb::io::{ParseOptions, parse_board_report};

/// `section|key|field=value` lines of one golden: (section, key) -> (field -> value).
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

    fn keys(&self, section: &str) -> BTreeSet<String> {
        self.map
            .keys()
            .filter(|(s, _)| s == section)
            .map(|(_, k)| k.clone())
            .collect()
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

/// Parses a board completely; `None` (with a message) when the parser reached a model stub or
/// skipped footprints / zones.
fn load(name: &str) -> Option<Board> {
    load_path(
        name,
        &kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb")),
    )
}

fn load_path(name: &str, path: &std::path::Path) -> Option<Board> {
    let text = std::fs::read_to_string(path).unwrap();
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

    cache_triangulation(&mut board);
    Some(board)
}

/// `CONNECTIVITY_DATA::Build` up to the ratsnest: `CN_CONNECTIVITY_ALGO::Build`, then
/// `internalRecalculateRatsnest` (`PropagateNets`, `GetClusters`, `addRatsnestCluster` for
/// the dirty nets except kept single zone islands, `ClearDirtyFlags`) and `updateRatsnest`
/// (`UpdateNet` then `OptimizeRNEdges` for nets 1.. that are dirty and have nodes).
fn build_ratsnest(ctx: &BoardCtx<'_>) -> (CnConnectivityAlgo, Vec<RnNet>) {
    let mut algo = CnConnectivityAlgo::new();
    algo.build(ctx);
    algo.propagate_nets(ctx);

    let last_net = algo.net_count();
    let mut nets: Vec<RnNet> = (0..=last_net).map(|_| RnNet::new()).collect();

    let clusters = algo.get_clusters(ctx).to_vec();

    for net in 0..last_net {
        if algo.is_net_dirty(net) {
            nets[net as usize].clear();
        }
    }

    for c in clusters {
        let cluster = algo.cluster(c);
        let net = cluster.origin_net();

        // Don't add intentionally-kept zone islands to the ratsnest
        if cluster.is_orphaned()
            && cluster.size() == 1
            && algo
                .item_list()
                .item(cluster.items()[0])
                .zone_layer()
                .is_some()
        {
            continue;
        }

        if algo.is_net_dirty(net) {
            let items = cluster.items().to_vec();
            nets[net as usize].add_cluster(algo.item_list_mut(), c, &items);
        }
    }

    algo.clear_dirty_flags();

    let dirty: Vec<usize> = (1..nets.len())
        .filter(|&i| nets[i].is_dirty() && nets[i].node_count() > 0)
        .collect();

    for &i in &dirty {
        // A delaunator error leaves the net as it is (see RnNet::update_net).
        let _ = nets[i].update_net(ctx, algo.item_list_mut());
    }

    for &i in &dirty {
        nets[i].optimize_rn_edges(ctx, algo.item_list());
    }

    (algo, nets)
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

/// Returns whether the board was compared.
fn check_board(name: &str, stats: &mut Stats) -> bool {
    let golden = Golden::read(name);
    let Some(board) = load(name) else {
        return false;
    };

    // the dump's item refs, by list position
    let mut golden_refs: Vec<String> = Vec::new();

    for n in 0.. {
        let Some(g) = golden.get("cn_item", &format!("#{n}")) else {
            break;
        };

        golden_refs.push(g["ref"].clone());
    }

    check_ratsnest(name, &golden, &board, Some(golden_refs), stats);
    true
}

/// Compares the `rn_net` lines. `golden_refs` are the dump's item refs by list position; `None`
/// uses the parent UUIDs (boards whose items all have one, no zones).
fn check_ratsnest(
    name: &str,
    golden: &Golden,
    board: &Board,
    golden_refs: Option<Vec<String>>,
    stats: &mut Stats,
) {
    let ctx = BoardCtx::bare(board);
    let (algo, mut nets) = build_ratsnest(&ctx);
    let list: &CnList = algo.item_list();

    let golden_refs = golden_refs.unwrap_or_else(|| {
        list.iter()
            .map(|item| {
                assert!(item.zone_layer().is_none());
                ctx.item(item.parent()).unwrap().uuid().as_std_string()
            })
            .collect()
    });

    assert_eq!(
        list.size() as usize,
        golden_refs.len(),
        "{name}: item count"
    );

    // AnchorRef(): "<x>,<y>@<CnRef>"
    let anchor_ref = |a: &CnAnchor| {
        let r = match a.item() {
            None => "null".to_string(),
            Some(id) if !list.item(id).valid() => "invalid".to_string(),
            Some(id) => golden_refs[list.item(id).list_index() as usize].clone(),
        };
        format!("{},{}@{r}", a.pos().x, a.pos().y)
    };
    let edge_ref = |e: &CnEdge| {
        format!(
            "{}>{}:{}:{}",
            anchor_ref(e.source_anchor(list).unwrap()),
            anchor_ref(e.target_anchor(list).unwrap()),
            e.weight(),
            e.is_visible()
        )
    };

    let mut rust_keys = BTreeSet::new();
    let mut unconnected = 0;

    for (code, net) in nets.iter_mut().enumerate() {
        // GetUnconnectedCount( false ) counts every net
        unconnected += net.edges(list).len();

        if net.node_count() == 0 {
            continue;
        }

        let key = format!("n{code}");
        rust_keys.insert(key.clone());

        let Some(g) = golden.get("rn_net", &key) else {
            continue;
        };
        let what = format!("{name} rn_net {key}");
        let net_name = board
            .find_net(NetCode(code as i32))
            .map_or("?".to_string(), |n| n.netname().to_string());

        stats.check(
            format!("{what} name"),
            format!("\"{net_name}\""),
            &g["name"],
        );
        stats.check(
            format!("{what} node_count"),
            net.node_count().to_string(),
            &g["node_count"],
        );

        let nodes: Vec<String> = net
            .nodes()
            .map(|r| anchor_ref(&list.item(r.item).anchors()[r.index as usize]))
            .collect();
        stats.check(
            format!("{what} nodes"),
            format!("[{}]", nodes.join(";")),
            &g["nodes"],
        );

        let edges: Vec<String> = net.edges(list).iter().map(edge_ref).collect();
        stats.check(
            format!("{what} edges"),
            format!("[{}]", edges.join(";")),
            &g["edges"],
        );
    }

    stats.check(
        format!("{name} rn_net keys"),
        format!("{rust_keys:?}"),
        &format!(
            "{:?}",
            golden
                .keys("rn_net")
                .into_iter()
                .filter(|k| k != "board")
                .collect::<BTreeSet<_>>()
        ),
    );

    let g = golden.get("rn_net", "board").expect("rn_net|board");
    stats.check(
        format!("{name} unconnected_count"),
        unconnected.to_string(),
        &g["unconnected_count"],
    );
    // UpdateRatsnestExclusions (CN-04) hides excluded edges; none of these boards has one
    stats.check(
        format!("{name} unconnected_visible"),
        unconnected.to_string(),
        &g["unconnected_visible"],
    );
}

/// The `connectivity` goldens of `gen_goldens.sh` (hashed goldens are left out).
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

#[test]
fn cn03_ratsnest_matches_cpp() {
    let mut stats = Stats::default();
    let mut compared = Vec::new();

    for name in BOARDS {
        if check_board(name, &mut stats) {
            compared.push(*name);
        }
    }

    eprintln!(
        "cn03: {} of {} boards compared ({}), {} fields, {} mismatches",
        compared.len(),
        BOARDS.len(),
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
    assert!(compared.len() >= 2, "too few boards could be compared");
}

/// The synthetic boards of `tests/golden/rn_random/` (`tests/oracle/gen_rn_random.sh`): random
/// via / track point sets for the triangulation, the Filter-Kruskal recursion, collinear and
/// near-collinear sets, coincident anchors of different clusters and the small-net cases.
const RANDOM_BOARDS: &[&str] = &[
    "rn_dense",
    "rn_collinear",
    "rn_near_collinear",
    "rn_coincident",
    "rn_mixed1",
    "rn_mixed2",
];

#[test]
fn cn03_ratsnest_random_boards_match_cpp() {
    let mut stats = Stats::default();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/rn_random");

    for name in RANDOM_BOARDS {
        let golden = Golden::read(&format!("rn_random/{name}"));
        let board = load_path(name, &dir.join(format!("{name}.kicad_pcb")))
            .unwrap_or_else(|| panic!("{name}: synthetic board does not load"));
        check_ratsnest(name, &golden, &board, None, &mut stats);
    }

    eprintln!(
        "cn03 random: {} boards, {} fields, {} mismatches",
        RANDOM_BOARDS.len(),
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

/// Port of `checkRatsnestSpansClusters` (`qa/tests/pcbnew/test_connectivity_clusters.cpp`):
/// per net, the ratsnest joins the net's clusters with exactly `clusters - 1` edges, each with
/// both end points. Returns the number of nets checked.
fn check_ratsnest_spans_clusters(name: &str, ctx: &BoardCtx<'_>) -> usize {
    let (mut algo, mut nets) = build_ratsnest(ctx);
    let clusters = algo.search_clusters(ctx, ClusterSearchMode::Ratsnest);
    let list = algo.item_list();

    let mut clusters_per_net: BTreeMap<i32, BTreeSet<_>> = BTreeMap::new();

    for cl in clusters {
        let cluster = algo.cluster(cl);

        // A kept island contributes no anchor, matching what AddCluster() skips.
        if cluster.is_orphaned()
            && cluster.size() == 1
            && list.item(cluster.items()[0]).zone_layer().is_some()
        {
            continue;
        }

        if cluster.origin_net() > 0 {
            clusters_per_net
                .entry(cluster.origin_net())
                .or_default()
                .insert(cl);
        }
    }

    let mut nets_checked = 0;

    for (&net, net_clusters) in &clusters_per_net {
        let Some(rn) = nets.get_mut(net as usize) else {
            continue;
        };

        if rn.node_count() == 0 {
            continue;
        }

        let edges = rn.edges(list);

        assert_eq!(
            edges.len(),
            net_clusters.len() - 1,
            "{name}: net {net} has {} clusters but {} ratsnest edges",
            net_clusters.len(),
            edges.len()
        );

        for edge in edges {
            assert!(edge.source_node().is_some() && edge.target_node().is_some());
        }

        nets_checked += 1;
    }

    nets_checked
}

/// `ClustersAndRatsnestMatchTheBoard`, ratsnest part, on the C++ test's boards plus the golden
/// boards (whatever the parser loads completely today; fills are read from the files,
/// `FillZones` is ZF-*). `BOOST_REQUIRE( netsChecked > 0 )` is applied to the total because
/// boards without footprints have few netted items.
#[test]
fn cn03_ratsnest_spans_clusters() {
    let cpp_boards = [
        "issue22267",
        "issue17429",
        "issue16182",
        "issue14559",
        "issue8909",
    ];
    let mut boards = 0;
    let mut nets_checked = 0;

    for name in cpp_boards.iter().chain(BOARDS) {
        let Some(board) = load(name) else {
            continue;
        };
        let ctx = BoardCtx::bare(&board);
        nets_checked += check_ratsnest_spans_clusters(name, &ctx);
        boards += 1;
    }

    eprintln!("cn03 ratsnest spans clusters: {boards} boards, {nets_checked} nets checked");
    assert!(nets_checked > 0, "no net exercised the ratsnest");
}
