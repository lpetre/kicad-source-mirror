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

//! Work package DRC-32 (creepage utilities, part 1): replay of the goldens of
//! `tests/oracle/creepage32_oracle.cpp` (the reference build's shape-pair paths and board-edge
//! graphs, `tests/golden/creepage32_*.txt`) and the ports of the C++ unit tests
//! `qa/tests/pcbnew/drc/test_drc_creepage_circle_prune.cpp` and
//! `qa/tests/pcbnew/drc/test_drc_creepage_issue24524.cpp`.

mod drc_harness;

use std::collections::BTreeMap;
use std::path::PathBuf;

use kicad_core::PCB_IU_SCALE;
use kicad_drc::creepage_utils::{
    BeShapeArc, BeShapeCircle, BeShapePoint, CreepItem, CreepShape, CreepShapeType, CreepageGraph,
    CuShapeArc, CuShapeCircle, CuShapeSegment, GraphNodeType, NodeId, PathConnection, ShapeId,
    build_creepage_board_edges,
};
use kicad_geom::{EdaAngle, Vector2D, Vector2I};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{Board, BoardCtx, MutEnv};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn hx(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn unhx(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex double"))
}

fn pt(p: Vector2I) -> String {
    format!("{},{}", p.x, p.y)
}

fn fnv1a(text: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;

    for &c in text.as_bytes() {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

fn paths_text(paths: &[PathConnection]) -> String {
    paths
        .iter()
        .map(|pc| {
            format!(
                "{},{},{},{},{}",
                hx(pc.a1.x),
                hx(pc.a1.y),
                hx(pc.a2.x),
                hx(pc.a2.y),
                hx(pc.weight)
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse the comma-separated fields of a golden shape line.
struct Fields<'a>(std::str::Split<'a, char>);

impl Fields<'_> {
    fn int(&mut self) -> i32 {
        self.0.next().unwrap().parse().unwrap()
    }

    fn pt(&mut self) -> Vector2I {
        let x = self.int();
        let y = self.int();
        Vector2I::new(x, y)
    }

    fn dbl(&mut self) -> f64 {
        unhx(self.0.next().unwrap())
    }
}

fn shape_from_golden(kind: &str, text: &str) -> CreepShape {
    let mut f = Fields(text.split(','));
    let vd = |p: Vector2I| Vector2D::new(p.x as f64, p.y as f64);

    match kind {
        "bp" => CreepShape::BePoint(BeShapePoint::new(f.pt())),
        "bc" => {
            let p = f.pt();
            CreepShape::BeCircle(BeShapeCircle::new(p, f.int()))
        }
        "ba" => {
            let p = f.pt();
            let r = f.int();
            let sa = EdaAngle::from_degrees(f.dbl());
            let ea = EdaAngle::from_degrees(f.dbl());
            let sp = f.pt();
            let ep = f.pt();
            CreepShape::BeArc(BeShapeArc::new(p, r, sa, ea, vd(sp), vd(ep)))
        }
        "cs" => {
            let s = f.pt();
            let e = f.pt();
            CreepShape::CuSegment(CuShapeSegment::new(s, e, f.dbl()))
        }
        "cc" => {
            let p = f.pt();
            CreepShape::CuCircle(CuShapeCircle::new(p, f.dbl()))
        }
        "ca" => {
            let p = f.pt();
            let r = f.dbl();
            let sa = EdaAngle::from_degrees(f.dbl());
            let ea = EdaAngle::from_degrees(f.dbl());
            let sp = f.pt();
            let ep = f.pt();
            let mut arc = CuShapeArc::new(p, r, sa, ea, vd(sp), vd(ep));
            arc.set_width(f.int() as f64);
            CreepShape::CuArc(arc)
        }
        _ => panic!("unknown shape kind {kind}"),
    }
}

/// Every `<shape>::Paths( <shape> )` overload against the reference build: 42 generated shapes
/// (7 per class, with coincident centres, zero radii, a degenerate segment and arcs across
/// +-180 degrees / over 180 degrees), every ordered pair, two max weights.
#[test]
fn creepage32_paths_match_reference() {
    let text = std::fs::read_to_string(golden_dir().join("creepage32_paths.txt")).unwrap();
    let mut shapes: Vec<CreepShape> = Vec::new();
    let mut compared = 0;
    let mut mismatches: Vec<String> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
    let mut dangling_differences = 0;

    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').collect();

        match fields[0] {
            "S" => {
                shapes.push(shape_from_golden(fields[2], fields[3]));
                kinds.push(fields[2].to_string());
            }
            "P" => {
                let i: usize = fields[1].parse().unwrap();
                let j: usize = fields[2].parse().unwrap();
                let max_weight = unhx(fields[3]);
                let paths = shapes[i].paths(&shapes[j], max_weight, max_weight * max_weight);
                let got = format!("{}|{:016x}", paths.len(), fnv1a(&paths_text(&paths)));
                let want = format!("{}|{}", fields[4], fields[5]);
                compared += 1;

                // CU_SHAPE_SEGMENT::Paths( const CU_SHAPE_ARC& ) answers its fallback best path
                // through a pointer into destroyed temporaries: the reference prints freed heap
                // memory (tcache pointers in a1, a later vector's path), see PORT_STATUS.md.
                // Only the number of paths is comparable there.
                let dangling = matches!(
                    (kinds[i].as_str(), kinds[j].as_str()),
                    ("cs", "ca") | ("ca", "cs")
                );

                if dangling && paths.len().to_string() == fields[4] {
                    if got != want {
                        dangling_differences += 1;
                    }

                    continue;
                }

                if got != want {
                    mismatches.push(format!(
                        "{}x{} ({}x{}) maxW {}: rust {} [{}], reference {}",
                        i,
                        j,
                        kinds[i],
                        kinds[j],
                        max_weight,
                        got,
                        paths_text(&paths),
                        want
                    ));
                }
            }
            _ => {}
        }
    }

    eprintln!(
        "creepage32 paths: {compared} pairs compared, {} mismatches, {dangling_differences} \
         segment x arc results differ where the reference reads freed memory",
        mismatches.len()
    );

    for m in mismatches.iter().take(20) {
        eprintln!("  {m}");
    }

    assert!(compared > 3000);
    assert!(
        mismatches.is_empty(),
        "{} path mismatches",
        mismatches.len()
    );
}

/// The node / connection / drawn-shape dump of a graph, in the format of the oracle's
/// `dumpGraphTail`.
fn dump_graph_tail(
    graph: &CreepageGraph<'_>,
    shape_idx: &BTreeMap<ShapeId, usize>,
    cfg: &str,
    node_start: usize,
    conn_start: usize,
    out: &mut Vec<String>,
) {
    for k in node_start..graph.nodes.len() {
        let n = &graph.nodes[k];
        let parent = n.parent.map_or(-1, |p| shape_idx[&p] as i64);
        out.push(format!(
            "N|{cfg}|{k}|{}|{parent}|{}",
            n.type_ as i32,
            pt(n.pos)
        ));
    }

    for k in conn_start..graph.connections.len() {
        let gc = &graph.connections[k];
        out.push(format!(
            "K|{cfg}|{k}|{}|{}|{}|{}",
            gc.n1.0,
            gc.n2.0,
            paths_text(std::slice::from_ref(&gc.path)),
            gc.force_straight_line as i32
        ));

        let mut drawn: Vec<PcbShape> = Vec::new();
        graph.connection_shapes(kicad_drc::creepage_utils::ConnId(k), &mut drawn);

        for s in drawn {
            let name = match s.shape.shape() {
                ShapeT::Segment => "segment",
                ShapeT::Arc => "arc",
                _ => "other",
            };
            let center = if s.shape.shape() == ShapeT::Arc {
                pt(s.shape.center())
            } else {
                "-".to_string()
            };
            out.push(format!(
                "G|{cfg}|{k}|{name}|{}|{}|{center}",
                pt(s.shape.start()),
                pt(s.shape.end())
            ));
        }
    }
}

fn shape_t_name(shape: ShapeT) -> &'static str {
    match shape {
        ShapeT::Segment => "segment",
        ShapeT::Rectangle => "rect",
        ShapeT::Arc => "arc",
        ShapeT::Circle => "circle",
        ShapeT::Poly => "poly",
        ShapeT::Bezier => "bezier",
        _ => "other",
    }
}

/// The Rust counterpart of the oracle's `runBoard`, fed with the child points (`X` lines) of
/// the reference dump.
fn run_board(
    board: &Board,
    child_points: &BTreeMap<String, Vec<(usize, Vector2I)>>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let ctx = BoardCtx::bare(board);
    let (outline, has_outline) = board.get_board_polygon_outlines(&ctx, false, None, false, true);

    let mut graph = CreepageGraph::new(&ctx);
    build_creepage_board_edges(
        &ctx,
        &mut graph.board_edge,
        &mut graph.owned_board_edges,
        None,
    );
    out.push(format!(
        "O|{}|{}",
        has_outline as i32,
        outline.outline_count()
    ));
    graph.board_outline = has_outline.then_some(outline);

    for (k, &item) in graph.board_edge.iter().enumerate() {
        match item {
            CreepItem::OwnedEdge(index) => {
                let s = &graph.owned_board_edges[index];
                let center = if s.shape.shape() == ShapeT::Arc {
                    pt(s.shape.center())
                } else {
                    "-".to_string()
                };
                out.push(format!(
                    "E|{k}|owned|{}|{}|{}|{center}",
                    shape_t_name(s.shape.shape()),
                    pt(s.shape.start()),
                    pt(s.shape.end())
                ));
            }
            CreepItem::Item(id) => {
                let uuid = board.get(id).unwrap().base().uuid.as_std_string();
                out.push(format!("E|{k}|item|{uuid}"));
            }
        }
    }

    graph.transform_edge_to_creep_shapes();
    graph.remove_duplicated_shapes();
    graph.transform_creep_shapes_to_nodes();

    out.push(format!("H|{}", graph.has_overlapping_cutouts as i32));

    let mut shape_idx: BTreeMap<ShapeId, usize> = BTreeMap::new();

    for (k, &id) in graph.shape_collection.iter().enumerate() {
        shape_idx.insert(id, k);
        let s = graph.shape(id);
        let parent = s.parent().map_or(-1, |p| {
            graph.board_edge.iter().position(|&e| e == p).unwrap() as i64
        });
        out.push(format!(
            "C|{k}|{}|{parent}|{}|{}|{}|{}|{}|{}",
            s.type_() as i32,
            pt(s.pos()),
            s.radius(),
            hx(s.start_angle().as_degrees()),
            hx(s.end_angle().as_degrees()),
            pt(s.start_point()),
            pt(s.end_point())
        ));
    }

    dump_graph_tail(&graph, &shape_idx, "base", 0, 0, &mut out);

    let prefix_nodes = graph.nodes.len();
    let prefix_conns = graph.connections.len();

    for (cfg, target, groove) in [("t20", 20000000.0, 0), ("t8g", 8000000.0, 500000)] {
        graph.set_target(target);
        graph.min_groove_width = groove;

        for &(k, p) in child_points.get(cfg).map(Vec::as_slice).unwrap_or(&[]) {
            out.push(format!("X|{cfg}|{k}|{}", pt(p)));
            let shape = graph.shape_collection[k];
            let gn = graph.add_node(GraphNodeType::Point, Some(shape), p);
            graph.nodes[gn.0].connect_directly = false;
        }

        // CREEPAGE_ENGINE::connectChildren
        let temp_nodes: Vec<NodeId> = (0..graph.nodes.len())
            .map(NodeId)
            .filter(|&n| {
                let node = &graph.nodes[n.0];
                node.parent.is_some_and(|p| !graph.shape(p).is_conductive())
                    && !node.connect_directly
                    && node.type_ == GraphNodeType::Point
            })
            .collect();

        for i in 0..temp_nodes.len() {
            for j in (i + 1)..temp_nodes.len() {
                let (n1, n2) = (temp_nodes[i], temp_nodes[j]);

                if n1 == n2 {
                    continue;
                }

                let (p1, p2) = (graph.nodes[n1.0].parent, graph.nodes[n2.0].parent);

                if p1.is_none() || p2.is_none() || p1 != p2 {
                    continue;
                }

                graph.connect_children(p1.unwrap(), n1, n2);
            }
        }

        dump_graph_tail(
            &graph,
            &shape_idx,
            cfg,
            prefix_nodes,
            prefix_conns,
            &mut out,
        );

        graph.truncate_to_prefix(prefix_nodes, prefix_conns);
        out.push(format!(
            "T|{cfg}|{}|{}",
            graph.nodes.len(),
            graph.connections.len()
        ));
    }

    out
}

/// The board-edge graph of the 19 QA creepage boards against the reference build.
#[test]
fn creepage32_board_graphs_match_reference() {
    let text = std::fs::read_to_string(golden_dir().join("creepage32_boards.txt")).unwrap();
    let mut boards: Vec<(String, Vec<String>)> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("file|") {
            boards.push((name.to_string(), Vec::new()));
        } else {
            boards.last_mut().unwrap().1.push(line.to_string());
        }
    }

    let data = kicad_qa::pcbnew_data_dir();
    let mut failures: Vec<String> = Vec::new();
    let mut lines_compared = 0;

    for (name, reference) in &boards {
        let stem = name.trim_end_matches(".kicad_pcb");
        let dir = match stem {
            "creepage" | "creepage_malformed_edge" => "creepage",
            other => other,
        };
        let path = data.join(dir).join(name);
        let loaded = drc_harness::load_board(&path)
            .unwrap_or_else(|e| panic!("{name}: the Rust loader failed: {e}"));

        let mut child_points: BTreeMap<String, Vec<(usize, Vector2I)>> = BTreeMap::new();

        for line in reference.iter().filter(|l| l.starts_with("X|")) {
            let f: Vec<&str> = line.split('|').collect();
            let mut xy = f[3].split(',').map(|v| v.parse::<i32>().unwrap());
            let p = Vector2I::new(xy.next().unwrap(), xy.next().unwrap());
            child_points
                .entry(f[1].to_string())
                .or_default()
                .push((f[2].parse().unwrap(), p));
        }

        let got = run_board(&loaded.board, &child_points);
        lines_compared += reference.len();

        if &got != reference {
            let first = got
                .iter()
                .zip(reference.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(reference.len()));
            failures.push(format!(
                "{name}: {} rust lines, {} reference lines; first difference at {first}:\n  rust {:?}\n  ref  {:?}",
                got.len(),
                reference.len(),
                got.get(first),
                reference.get(first)
            ));
        }
    }

    eprintln!(
        "creepage32 boards: {} boards, {lines_compared} reference lines, {} boards differ",
        boards.len(),
        failures.len()
    );

    for f in &failures {
        eprintln!("  {f}");
    }

    assert_eq!(boards.len(), 19);
    assert!(failures.is_empty());
}

// ---- qa/tests/pcbnew/drc/test_drc_creepage_circle_prune.cpp ---------------------------------

/// Callers select a side by index, so a reachable pair always yields both tangents
const TANGENTS_PER_PAIR: usize = 2;

/// `BOOST_CHECK_CLOSE( a, b, tolerancePercent )`.
fn check_close(a: f64, b: f64, tolerance_percent: f64) {
    let diff = (a - b).abs();
    let tol = tolerance_percent / 100.0;
    assert!(
        diff <= tol * a.abs() && diff <= tol * b.abs(),
        "{a} and {b} differ by more than {tolerance_percent}%"
    );
}

/// Port of `CreepageCircleToHoleKeepsReachableTangent`.
#[test]
fn creepage_circle_to_hole_keeps_reachable_tangent() {
    let r1: f64 = PCB_IU_SCALE.mm_to_iu(0.1) as f64;
    let r2: i32 = PCB_IU_SCALE.mm_to_iu(10.0);
    let dist: i32 = PCB_IU_SCALE.mm_to_iu(10.2);
    let max_weight: f64 = PCB_IU_SCALE.mm_to_iu(9.0) as f64;
    let expected_weight = (dist as f64 * dist as f64 - r2 as f64 * r2 as f64).sqrt() - r1;

    // Outside the hole, centres beyond the target, tangent inside it
    assert!(dist > r2);
    assert!(dist as f64 > max_weight);
    assert!(expected_weight < max_weight);

    let copper = CreepShape::CuCircle(CuShapeCircle::new(Vector2I::new(0, 0), r1));
    let hole = CreepShape::BeCircle(BeShapeCircle::new(Vector2I::new(dist, 0), r2));

    let paths = copper.paths(&hole, max_weight, max_weight * max_weight);
    assert_eq!(paths.len(), TANGENTS_PER_PAIR);

    for pc in &paths {
        check_close(pc.weight, expected_weight, 0.1);
    }
}

/// Port of `CreepageCircleInsideHoleKeepsRadialGap`.
#[test]
fn creepage_circle_inside_hole_keeps_radial_gap() {
    let r1: f64 = PCB_IU_SCALE.mm_to_iu(0.1) as f64;
    let r2: i32 = PCB_IU_SCALE.mm_to_iu(10.0);
    let dist: i32 = PCB_IU_SCALE.mm_to_iu(9.0);
    let max_weight: f64 = PCB_IU_SCALE.mm_to_iu(6.0) as f64;
    let expected_weight = (r2 - dist) as f64 - r1;

    // Inside the hole, centres beyond the target, radial gap inside it
    assert!(dist <= r2);
    assert!(dist as f64 > max_weight);
    assert!(expected_weight < max_weight);

    let copper = CreepShape::CuCircle(CuShapeCircle::new(Vector2I::new(0, 0), r1));
    let hole = CreepShape::BeCircle(BeShapeCircle::new(Vector2I::new(dist, 0), r2));

    let paths = copper.paths(&hole, max_weight, max_weight * max_weight);
    assert_eq!(paths.len(), TANGENTS_PER_PAIR);

    for pc in &paths {
        check_close(pc.weight, expected_weight, 0.1);
    }
}

// ---- qa/tests/pcbnew/drc/test_drc_creepage_issue24524.cpp -----------------------------------

/// Build a horizontal stadium slot: 20 mm wide, 2 mm tall, 1 mm corner radius.
/// The left cap is centered at (1, 1) mm and the right cap at (19, 1) mm.
fn make_horizontal_stadium() -> PcbShape {
    let env = MutEnv::detached();
    let mut slot = PcbShape::with_shape(ShapeT::Rectangle);
    slot.set_start(&env, Vector2I::new(0, 0));
    slot.set_end(
        &env,
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(20.0), PCB_IU_SCALE.mm_to_iu(2.0)),
    );
    slot.set_corner_radius(&env, PCB_IU_SCALE.mm_to_iu(1.0));
    slot
}

/// Port of `CreepageStadiumCapsBlockOuterSide`: needs `SegmentIntersectsBoard` (DRC-33).
#[test]
#[ignore = "SegmentIntersectsBoard is ported by work package DRC-33"]
fn creepage_stadium_caps_block_outer_side() {
    let board = Board::new();
    let ctx = BoardCtx::bare(&board);
    let slot = make_horizontal_stadium();
    let owned = vec![slot];
    let edges = vec![CreepItem::OwnedEdge(0)];
    let dont_test: Vec<CreepItem> = Vec::new();

    let one_mm = PCB_IU_SCALE.mm_to_iu(1.0);
    let half_mm = PCB_IU_SCALE.mm_to_iu(0.5);
    let sib = |a: Vector2I, b: Vector2I| {
        kicad_drc::creepage_utils::pending::segment_intersects_board(
            &ctx, a, b, &edges, &owned, &dont_test, 0,
        )
    };

    // Left cap apex sits at (0, 1) mm. A horizontal probe through it must be blocked.
    assert!(!sib(
        Vector2I::new(-half_mm, one_mm),
        Vector2I::new(half_mm, one_mm)
    ));

    // Right cap apex sits at (20, 1). A horizontal probe through it must be blocked.
    assert!(!sib(
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(19.5), one_mm),
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(20.5), one_mm)
    ));

    // A probe across the inner half of the left cap's circle (around (2, 1)) lies
    // inside the slot, away from any real boundary, so it must read as clear. A cap
    // mistakenly swept over the inner half would model a boundary here and block it.
    assert!(sib(
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(1.5), one_mm),
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(2.5), one_mm)
    ));
}

/// Port of `CreepageStadiumCapsArcOutward`: the decomposition feeding the routing graph must
/// emit two cap arcs whose swept midpoints fall on the outer side of each cap center.
#[test]
fn creepage_stadium_caps_arc_outward() {
    let board = Board::new();
    let ctx = BoardCtx::bare(&board);
    let mut graph = CreepageGraph::new(&ctx);
    graph.owned_board_edges.push(make_horizontal_stadium());
    graph.board_edge.push(CreepItem::OwnedEdge(0));
    graph.transform_edge_to_creep_shapes();

    let arcs: Vec<&CreepShape> = graph
        .shape_collection
        .iter()
        .map(|&id| graph.shape(id))
        .filter(|s| s.type_() == CreepShapeType::Arc)
        .collect();

    assert_eq!(arcs.len(), 2);

    let one_mm = PCB_IU_SCALE.mm_to_iu(1.0);

    for arc in arcs {
        let center = arc.pos();
        let mid_angle = (arc.start_angle() + arc.end_angle()) / 2.0;
        let apex_x = center.x as f64 + arc.radius() as f64 * mid_angle.cos();

        assert_eq!(arc.radius(), one_mm);

        if center.x < PCB_IU_SCALE.mm_to_iu(10.0) {
            assert!(apex_x < center.x as f64); // left cap bulges left
        } else {
            assert!(apex_x > center.x as f64); // right cap bulges right
        }
    }
}
