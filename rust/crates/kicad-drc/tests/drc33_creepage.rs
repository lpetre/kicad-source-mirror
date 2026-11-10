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

//! Work package DRC-33: the creepage provider (`drc_test_provider_creepage.cpp`), the
//! creepage engine (`drc_creepage_engine.cpp`) and part 2 of `drc_creepage_utils.cpp`,
//! verified against the reference `kicad-cli` reports of the corpus, plus the ported QA tests
//! (`qa/tests/pcbnew/drc/test_drc_creepage*.cpp`, `test_creepage_engine_*.cpp`,
//! `qa/tests/pcbnew/test_creepage_groove_order.cpp`).

mod drc_harness;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::layer_ids::PcbLayerId;
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::creepage_engine::CreepageEngine;
use kicad_drc::creepage_utils::{ConnId, CreepageGraph, build_creepage_board_edges};
use kicad_drc::pcbexpr::functions::PcbExprCacheScope;
use kicad_drc::providers::creepage::DrcTestProviderCreepage;
use kicad_geom::Vector2I;
use kicad_pcb::ids::NetCode;
use kicad_pcb::BoardCtx;

fn hx(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn pt(p: Vector2I) -> String {
    format!("{},{}", p.x, p.y)
}

/// The `S|...` line and the `C|...` lines of one solve (creepage33_oracle.cpp `printSolve`).
#[allow(clippy::too_many_arguments)]
fn print_solve(
    out: &mut Vec<String>,
    tag: &str,
    graph: &CreepageGraph<'_>,
    net_a: i32,
    net_b: i32,
    layer: PcbLayerId,
    target: f64,
    distance: f64,
    path: &[ConnId],
) {
    let ctx = graph.ctx;
    let uuid = |item: Option<kicad_drc::creepage_utils::CreepItem>| -> String {
        match item {
            Some(kicad_drc::creepage_utils::CreepItem::Item(id)) => {
                ctx.item(id).map_or("-".to_string(), |r| r.uuid().as_string())
            }
            Some(kicad_drc::creepage_utils::CreepItem::OwnedEdge(_)) => "owned".to_string(),
            None => "-".to_string(),
        }
    };
    let (mut item1, mut item2) = (None, None);
    let (mut start, mut end) = ("-".to_string(), "-".to_string());

    if path.len() >= 4 {
        let gc1 = &graph.connections[path[1].0];
        let gc2 = &graph.connections[path[path.len() - 2].0];
        item1 = graph.nodes[gc1.n1.0].parent.and_then(|p| graph.shape(p).parent());
        item2 = graph.nodes[gc2.n2.0].parent.and_then(|p| graph.shape(p).parent());
        start = pt(Vector2I::from_vec(gc1.path.a2));
        end = pt(Vector2I::from_vec(gc2.path.a2));
    }

    out.push(format!(
        "{tag}|{net_a}|{net_b}|{}|{}|{}|{}|{}|{}|{start}|{end}",
        layer.0,
        hx(target),
        hx(distance),
        path.len(),
        uuid(item1),
        uuid(item2)
    ));

    for (k, &gc) in path.iter().enumerate() {
        let c = &graph.connections[gc.0];
        out.push(format!(
            "C|{k}|{}|{}|{}",
            pt(graph.nodes[c.n1.0].pos),
            pt(graph.nodes[c.n2.0].pos),
            hx(c.path.weight)
        ));
    }
}

/// The Rust counterpart of creepage33_oracle.cpp `runBoard`: the legacy provider pipeline,
/// every solve printed.
fn run_board(loaded: &mut drc_harness::Loaded) -> Vec<String> {
    let mut out = Vec::new();
    let mut conn = ConnectivityData::new();
    conn.build_connectivity(&mut loaded.board);
    let board = &loaded.board;
    let engine = &loaded.engine;
    let ctx = BoardCtx::bare(board)
        .with_rules(engine)
        .with_connectivity(&conn);
    let _scope = PcbExprCacheScope::enter(board);

    if !engine.has_rules_for_constraint_type(DrcConstraintT::CreepageConstraint) {
        out.push("none".to_string());
        return out;
    }

    let netcodes: Vec<i32> = board.nets_by_netcode().map(|n| n.net_code.0).collect();
    let max_constraint = DrcTestProviderCreepage::max_constraint(engine);
    out.push(format!("M|{}", hx(max_constraint)));

    if max_constraint <= 0.0 {
        return out;
    }

    let has_conditional = engine.has_conditional_constraint(DrcConstraintT::CreepageConstraint);
    let (outline, has_valid_outline) =
        board.get_board_polygon_outlines(&ctx, false, None, false, true);

    let mut graph = CreepageGraph::new(&ctx);
    graph.min_groove_width = 0;
    graph.board_outline = has_valid_outline.then_some(outline);
    build_creepage_board_edges(&ctx, &mut graph.board_edge, &mut graph.owned_board_edges, None);
    graph.transform_edge_to_creep_shapes();
    graph.remove_duplicated_shapes();
    graph.transform_creep_shapes_to_nodes();
    graph.generate_paths(max_constraint, PcbLayerId::EdgeCuts, None);

    let be_node_size = graph.nodes.len();
    let be_connections_size = graph.connections.len();
    let mut prev_test_changed_graph = false;
    let layers = board.enabled_layers();

    for i in 0..netcodes.len() {
        for j in i + 1..netcodes.len() {
            let (net_a, net_b) = (netcodes[i], netcodes[j]);

            for layer in layers.copper_layers() {
                if prev_test_changed_graph {
                    graph.truncate_to_prefix(be_node_size, be_connections_size);
                }

                // testCreepage( CREEPAGE_GRAPH&, ... ) up to the report
                prev_test_changed_graph = (|| {
                    let target = if has_conditional {
                        max_constraint
                    } else {
                        DrcTestProviderCreepage::placeholder_constraint(
                            engine, &ctx, net_a, net_b, layer,
                        )
                        .value()
                        .min() as f64
                    };

                    graph.set_target(target);

                    if target <= 0.0 {
                        return false;
                    }

                    let (Some(na), Some(nb)) = (
                        board.find_net(NetCode(net_a)),
                        board.find_net(NetCode(net_b)),
                    ) else {
                        return false;
                    };

                    if na.bounding_box(&ctx).distance_box(&nb.bounding_box(&ctx)) as f64 > target {
                        return false;
                    }

                    let node_a = graph.add_net_elements(net_a, layer, target as i32);
                    let node_b = graph.add_net_elements(net_b, layer, target as i32);
                    graph.generate_paths(target, layer, None);
                    CreepageEngine::connect_children(&mut graph);

                    let mut path = Vec::new();
                    let distance = graph.solve(Some(node_a), Some(node_b), &mut path);
                    print_solve(
                        &mut out, "S", &graph, net_a, net_b, layer, target, distance, &path,
                    );
                    true
                })();
            }
        }
    }

    out
}

/// Splits an oracle dump into its boards (`file|NAME` headers).
fn split_boards(text: &str) -> Vec<(String, Vec<String>)> {
    let mut boards: Vec<(String, Vec<String>)> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("file|") {
            boards.push((name.to_string(), Vec::new()));
        } else if let Some(b) = boards.last_mut() {
            b.1.push(line.to_string());
        }
    }

    boards
}

/// The corpus boards with a `creepage` rule (`grep -l creepage qa/data/pcbnew/*/*.kicad_dru`).
const CREEPAGE_BOARDS: &[&str] = &[
    "qa/data/pcbnew/creepage/creepage.kicad_pcb",
    "qa/data/pcbnew/creepage/creepage_malformed_edge.kicad_pcb",
    "qa/data/pcbnew/creepage_slots/creepage_slots.kicad_pcb",
    "qa/data/pcbnew/issue20480/issue20480.kicad_pcb",
    "qa/data/pcbnew/issue21482/issue21482.kicad_pcb",
    "qa/data/pcbnew/issue23389/issue23389.kicad_pcb",
    "qa/data/pcbnew/issue23578/issue23578.kicad_pcb",
    "qa/data/pcbnew/issue23658/issue23658.kicad_pcb",
    "qa/data/pcbnew/issue24078/issue24078.kicad_pcb",
    "qa/data/pcbnew/issue24286/issue24286.kicad_pcb",
    "qa/data/pcbnew/issue24523/issue24523.kicad_pcb",
    "qa/data/pcbnew/issue24543/issue24543.kicad_pcb",
    "qa/data/pcbnew/issue24544/issue24544.kicad_pcb",
    "qa/data/pcbnew/issue24597/issue24597.kicad_pcb",
    "qa/data/pcbnew/issue24853/issue24853.kicad_pcb",
    "qa/data/pcbnew/issue24875/issue24875.kicad_pcb",
    "qa/data/pcbnew/issue24875_capstraight/issue24875_capstraight.kicad_pcb",
    "qa/data/pcbnew/issue24875_fullround/issue24875_fullround.kicad_pcb",
    "qa/data/pcbnew/issue24885/issue24885.kicad_pcb",
    "qa/data/pcbnew/issue25439/issue25439.kicad_pcb",
];

/// Corpus replay of the `creepage` error code against the reference reports
/// (`tests/golden/drc_corpus.txt`), through the real run path with the creepage provider only.
#[test]
fn drc33_creepage_corpus() {
    let mut total = drc_harness::Scoreboard::default();

    for board in CREEPAGE_BOARDS {
        let score = drc_harness::compare_corpus_flow(&["creepage"], board, &["creepage"]);

        for (t, s) in score.per_type {
            let e = total.per_type.entry(t).or_default();
            e.matched += s.matched;
            e.missing += s.missing;
            e.extra += s.extra;
        }

        total.compared.extend(score.compared);
        total.pending.extend(score.pending);
        total.differences.extend(score.differences);
        total.partial_differences.extend(score.partial_differences);
    }

    total.print("DRC-33 creepage");
    total.assert_no_mismatch();
}

#[test]
#[ignore = "diagnostic"]
fn drc33_debug_one() {
    let board = std::env::var("DRC33_BOARD").unwrap_or_default();
    let path = kicad_qa::kicad_source_root().join(&board);
    let mut loaded = drc_harness::load_board(&path).unwrap();
    let found = drc_harness::run_flow_providers(
        &mut loaded,
        kicad_core::eda_units::EdaUnits::Mm,
        false,
        &["creepage"],
    );
    eprintln!("{} violations", found.len());
}

/// Replays the reference solves of `creepage33-oracle board` (the full dump in
/// `DRC33_ORACLE_DUMP`, a diagnostic run) against the Rust pipeline.
#[test]
#[ignore = "diagnostic: needs a local creepage33-oracle dump"]
fn drc33_oracle_full_dump() {
    let Ok(path) = std::env::var("DRC33_ORACLE_DUMP") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap();
    let root = kicad_qa::kicad_source_root();

    let only = std::env::var("DRC33_ONLY").unwrap_or_default();

    for (name, reference) in split_boards(&text) {
        if !name.contains(&only) {
            continue;
        }

        let rel = CREEPAGE_BOARDS
            .iter()
            .find(|b| b.ends_with(&format!("/{name}")))
            .unwrap();
        let mut loaded = drc_harness::load_board(&root.join(rel)).unwrap();
        let mine = run_board(&mut loaded);
        let (mut same_s, mut diff_s, mut diff_c) = (0, 0, 0);
        let ref_s: Vec<&String> = reference.iter().filter(|l| !l.starts_with("C|")).collect();
        let mine_s: Vec<&String> = mine.iter().filter(|l| !l.starts_with("C|")).collect();

        for (k, (a, b)) in ref_s.iter().zip(mine_s.iter()).enumerate() {
            if a == b {
                same_s += 1;
            } else {
                diff_s += 1;
                if diff_s <= 3 {
                    eprintln!("{name} #{k}\n  ref  {a}\n  rust {b}");
                }
            }
        }

        for (a, b) in reference.iter().zip(mine.iter()) {
            if a != b && a.starts_with("C|") {
                diff_c += 1;
                if diff_c <= 3 {
                    eprintln!("  C ref  {a}\n  C rust {b}");
                }
            }
        }

        eprintln!(
            "{name}: {} / {} solve lines, same {same_s} differ {diff_s}, paths differ {diff_c}",
            ref_s.len(),
            mine_s.len()
        );
    }
}
