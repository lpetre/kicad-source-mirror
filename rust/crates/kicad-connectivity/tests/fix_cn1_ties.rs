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

//! FIX-CN-1: ratsnest ends that follow the heap order in the reference
//! (`rust/docs/design/review-connectivity.md` F-1, sites P7 .. P9 of
//! `pcbnew/ratsnest/ratsnest_data.cpp`).
//!
//! - the tie flags of the edges (`CnEdgeTie` / `UnconnectedEdge::tie`) on the probe boards;
//! - the tie-aware comparison rule (`UnconnectedEdge::accepts`) against the `unconnected_items`
//!   of the reference `kicad-cli pcb drc --format json` (`tests/golden/fix_cn1_unconnected.txt`,
//!   `tests/oracle/gen_fix_cn1.sh`): the reference DRC rebuilds the connectivity and then
//!   answers differently from the port on `rev_order_probe` (and differently from run to run
//!   on `rn_coincident`); the exact comparison fails there, the tie-aware one passes.
//!
//! An unconnected-items violation is `( source item, target item )` of one edge
//! (`drc_test_provider_connectivity.cpp`, `RunOnUnconnectedEdges`), so comparing the edges'
//! items is comparing the violations' items.

use std::collections::BTreeMap;

use kicad_connectivity::data::{ConnectivityData, UnconnectedEdge};
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::ItemId;
use kicad_pcb::io::{ParseOptions, parse_board_report};

fn board_path(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");

    if name.starts_with("rn_") {
        dir.join("rn_random").join(format!("{name}.kicad_pcb"))
    } else {
        dir.join("cn04").join(format!("{name}.kicad_pcb"))
    }
}

/// Parses a board; `None` (with a message) when the parser reached a model stub or skipped
/// items the connectivity needs.
fn load(name: &str) -> Option<Board> {
    let text = std::fs::read_to_string(board_path(name)).unwrap();
    let options = ParseOptions {
        allow_pending: true,
    };
    let parsed = std::panic::catch_unwind(|| parse_board_report(&text, "", &options));
    let (board, report) = match parsed {
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
            "footprint" | "module" | "pad" | "zone" => {
                eprintln!("{name}: not compared, the parser skipped `{}`", s.token);
                return None;
            }
            "embedded_fonts" | "net_class" | "zone_defaults" | "generated" => {}
            t => panic!("{name}: parser skipped `{t}`: extend the filter"),
        }
    }

    Some(board)
}

/// The port's unconnected edges and the UUID of every item.
fn edges(name: &str) -> Option<(Vec<UnconnectedEdge>, BTreeMap<String, ItemId>)> {
    let mut board = load(name)?;
    let mut data = ConnectivityData::new();
    assert!(data.build_connectivity(&mut board), "{name}: build");
    let edges = data.unconnected_edges();

    let ctx = BoardCtx::bare(&board);
    let mut ids = BTreeMap::new();

    for e in &edges {
        let t = e.tie.iter();
        let cands = t.flat_map(|t| t.source_candidates.iter().chain(&t.target_candidates));

        for id in [e.source_item, e.target_item]
            .into_iter()
            .chain(cands.map(|c| c.0))
        {
            ids.insert(ctx.item(id).unwrap().uuid().as_string(), id);
        }
    }

    Some((edges, ids))
}

/// The reference violations per `( board, run )`: `( uuid 0, uuid 1 )` in report order.
fn reference() -> BTreeMap<(String, u32), Vec<(String, String)>> {
    let mut out: BTreeMap<(String, u32), Vec<(String, String)>> = BTreeMap::new();

    for line in include_str!("golden/fix_cn1_unconnected.txt").lines() {
        let f: Vec<&str> = line.split('|').collect();
        assert_eq!(f.len(), 4, "{line}");
        out.entry((f[0].to_string(), f[1].parse().unwrap()))
            .or_default()
            .push((f[2].to_string(), f[3].to_string()));
    }

    out
}

/// Exact comparison: the reference violations are the port's, in order.
fn exact_match(
    edges: &[UnconnectedEdge],
    ids: &BTreeMap<String, ItemId>,
    r: &[(String, String)],
) -> bool {
    let id = |u: &str| ids.get(u).copied();

    edges.len() == r.len()
        && edges
            .iter()
            .zip(r)
            .all(|(e, (a, b))| id(a) == Some(e.source_item) && id(b) == Some(e.target_item))
}

/// Tie-aware comparison: a one-to-one assignment of reference violations to port edges such
/// that every edge accepts its violation (`UnconnectedEdge::accepts`; the order of the
/// violations is not compared, a tied end moves its edge in the `GetEdges()` order). Kuhn's
/// augmenting paths.
fn tie_aware_match(
    edges: &[UnconnectedEdge],
    ids: &BTreeMap<String, ItemId>,
    r: &[(String, String)],
) -> bool {
    if edges.len() != r.len() {
        return false;
    }

    let accepts = |e: usize, v: usize| match (ids.get(&r[v].0), ids.get(&r[v].1)) {
        (Some(&a), Some(&b)) => edges[e].accepts(a, b),
        _ => false,
    };

    fn augment(
        v: usize,
        n: usize,
        accepts: &dyn Fn(usize, usize) -> bool,
        seen: &mut [bool],
        owner: &mut [Option<usize>],
    ) -> bool {
        for e in 0..n {
            if !seen[e] && accepts(e, v) {
                seen[e] = true;

                if owner[e].is_none_or(|w| augment(w, n, accepts, seen, owner)) {
                    owner[e] = Some(v);
                    return true;
                }
            }
        }

        false
    }

    let n = edges.len();
    let mut owner = vec![None; n];

    (0..n).all(|v| augment(v, n, &accepts, &mut vec![false; n], &mut owner))
}

#[test]
fn rev_order_probe_both_nets_flagged() {
    let Some((edges, _)) = edges("rev_order_probe") else {
        return;
    };

    assert_eq!(edges.len(), 2);

    for e in &edges {
        let tie = e
            .tie
            .as_ref()
            .unwrap_or_else(|| panic!("net {} not flagged", e.net));
        assert!(!tie.coincident_clusters && !tie.unordered, "{e:?}");
    }

    // N2: via to two zones sharing the nearest outline vertex: one end tied (P8)
    let n2 = edges
        .iter()
        .find(|e| e.tie.as_ref().unwrap().zone_anchor)
        .expect("N2");
    let t = n2.tie.as_ref().unwrap();
    assert_eq!(
        t.source_candidates.len() + t.target_candidates.len(),
        2,
        "{n2:?}"
    );
    assert!(t.source_candidates.is_empty() || t.target_candidates.is_empty());

    // N3: zone islands with equal bounding boxes: both ends tied (P9)
    let n3 = edges
        .iter()
        .find(|e| e.tie.as_ref().unwrap().zone_to_zone)
        .expect("N3");
    let t = n3.tie.as_ref().unwrap();
    // (four pairs at one centre distance, three tested: any pair can be last, and the
    // islands of one zone are distinct anchors)
    assert!(t.source_candidates.len() >= 2, "{n3:?}");
    assert!(t.target_candidates.len() >= 2, "{n3:?}");
}

#[test]
fn rev_ratsnest_zones_none_flagged() {
    let Some((edges, _)) = edges("rev_ratsnest_zones") else {
        return;
    };

    assert!(!edges.is_empty());

    for e in &edges {
        assert!(e.tie.is_none(), "{e:?}");
    }
}

#[test]
fn rn_coincident_chain_edges_flagged() {
    let Some((edges, _)) = edges("rn_coincident") else {
        return;
    };

    let flagged: Vec<&UnconnectedEdge> = edges.iter().filter(|e| e.tie.is_some()).collect();
    assert!(!flagged.is_empty());

    for e in flagged {
        let t = e.tie.as_ref().unwrap();
        assert!(t.coincident_clusters && t.unordered, "{e:?}");
        assert_eq!(e.weight, 1, "{e:?}");
        assert_eq!(e.source, e.target, "{e:?}");
    }
}

/// The DRC comparison of FIX-CN-1: the reference `unconnected_items` match the port's edges
/// with the tie rule on every board and run, and without it they do not on the boards whose
/// ties the reference resolves differently.
#[test]
fn drc_unconnected_items_tie_aware() {
    let reference = reference();
    let mut exact_fail = Vec::new();
    let mut compared = Vec::new();

    for ((name, run), r) in &reference {
        let Some((edges, ids)) = edges(name) else {
            continue;
        };

        compared.push(name.clone());

        assert!(
            tie_aware_match(&edges, &ids, r),
            "{name} run {run}: the reference violations are not among the tie candidates"
        );

        // Untied edges must match exactly: the rule may only relax flagged edges.
        let untied: Vec<UnconnectedEdge> =
            edges.iter().filter(|e| e.tie.is_none()).cloned().collect();
        let mut stripped = edges.clone();
        stripped.iter_mut().for_each(|e| e.tie = None);

        if !exact_match(&edges, &ids, r) {
            exact_fail.push(format!("{name}/{run}"));
            assert!(!tie_aware_match(&stripped, &ids, r), "{name} run {run}");
        }

        for e in &untied {
            let found = r.iter().any(|(a, b)| {
                ids.get(a) == Some(&e.source_item) && ids.get(b) == Some(&e.target_item)
            });
            assert!(
                found,
                "{name} run {run}: untied edge not in the reference: {e:?}"
            );
        }
    }

    // Without the rule the comparison fails on rev_order_probe (N2: "Zone [N2] on B.Cu",
    // N3: the other zone pair) and on rn_coincident (chain pairing differs between runs).
    if compared.contains(&"rev_order_probe".to_string()) {
        assert!(
            exact_fail.contains(&"rev_order_probe/1".to_string()),
            "{exact_fail:?}"
        );
    }

    assert!(
        !exact_fail.contains(&"rev_ratsnest_zones/1".to_string()),
        "{exact_fail:?}"
    );
    eprintln!("exact comparison fails on {exact_fail:?}");
}
