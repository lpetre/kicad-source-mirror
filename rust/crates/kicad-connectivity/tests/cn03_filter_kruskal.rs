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

//! Port of `qa/tests/common/test_filter_kruskal.cpp` (suite `FilterKruskal`).
//!
//! `std::mt19937` is `kicad_core::mt19937::Mt19937` (same sequence as `boost::mt19937`), so the
//! random graphs are the C++ ones. `std::shuffle` in `SelectionIgnoresInputOrderAndOrientation`
//! is a plain Fisher-Yates over the same generator: the case checks independence of the input
//! order, which any permutation exercises.

use kicad_connectivity::algo::KiUnionFind;
use kicad_connectivity::ratsnest::filter_kruskal::filter_kruskal;
use kicad_core::mt19937::Mt19937;
use kicad_geom::stdsort::std_sort;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TestEdge {
    u: usize,
    v: usize,
    weight: i64,
}

fn edge_order(lhs: &TestEdge, rhs: &TestEdge) -> bool {
    (lhs.weight, lhs.u, lhs.v) < (rhs.weight, rhs.u, rhs.v)
}

fn edge_ends(edge: &TestEdge) -> (usize, usize) {
    (edge.u, edge.v)
}

fn rng(r: &mut Mt19937) -> usize {
    r.next_u32() as usize
}

/// Plain sort-then-scan Kruskal.  Filter-Kruskal is only worth having if it agrees with this
/// on every input, so every case below is checked against it rather than against a
/// hand-written expectation.
fn reference_kruskal(
    mut edges: Vec<TestEdge>,
    nodes: usize,
    seeds: &[(usize, usize)],
) -> Vec<TestEdge> {
    let mut forest = KiUnionFind::new(nodes);

    for &(a, b) in seeds {
        forest.unite(a, b);
    }

    std_sort(&mut edges, edge_order);

    let mut tree = Vec::new();

    for edge in &edges {
        if forest.unite(edge.u, edge.v) {
            tree.push(*edge);
        }
    }

    tree
}

fn run_filter_kruskal(
    mut edges: Vec<TestEdge>,
    nodes: usize,
    seeds: &[(usize, usize)],
    returned_count: &mut usize,
) -> Vec<TestEdge> {
    let mut forest = KiUnionFind::new(nodes);

    for &(a, b) in seeds {
        forest.unite(a, b);
    }

    let mut tree = Vec::new();

    *returned_count = filter_kruskal(&mut edges, &mut forest, edge_order, edge_ends, |e| {
        tree.push(*e)
    });

    tree
}

fn total_weight(tree: &[TestEdge]) -> i64 {
    tree.iter().map(|e| e.weight).sum()
}

/// Compare against the reference on total weight, on the edge set itself, on the emission
/// order, and on the count the algorithm reports.  Weight alone would not catch an
/// implementation that picked a different tree of equal cost.
fn check_against_reference(edges: &[TestEdge], nodes: usize, seeds: &[(usize, usize)]) {
    let mut reported_count = 0;
    let got = run_filter_kruskal(edges.to_vec(), nodes, seeds, &mut reported_count);
    let want = reference_kruskal(edges.to_vec(), nodes, seeds);

    assert_eq!(reported_count, got.len());
    assert_eq!(got.len(), want.len());
    assert_eq!(total_weight(&got), total_weight(&want));

    // Emission must be in non-decreasing order; a caller relying on Kruskal semantics may
    // depend on that even though the set is what matters.
    for ii in 1..got.len() {
        assert!(!edge_order(&got[ii], &got[ii - 1]));
    }

    let mut sorted_got = got.clone();
    let mut sorted_want = want.clone();
    std_sort(&mut sorted_got, edge_order);
    std_sort(&mut sorted_want, edge_order);

    assert_eq!(sorted_got, sorted_want);
}

fn random_edges(r: &mut Mt19937, nodes: usize, count: usize, weight_range: i64) -> Vec<TestEdge> {
    let mut edges = Vec::with_capacity(count);

    for _ in 0..count {
        let u = rng(r) % nodes;
        let v = rng(r) % nodes;
        let weight = (r.next_u32() % weight_range as u32) as i64;
        edges.push(TestEdge { u, v, weight });
    }

    edges
}

/// Sizes are chosen to straddle KRUSKAL_THRESHOLD so both the recursive partitioning path and
/// the direct sort path are exercised.
#[test]
fn matches_plain_kruskal() {
    let mut r = Mt19937::from_seed(555);

    for _trial in 0..60 {
        let nodes = 2 + rng(&mut r) % 2000;
        let count = rng(&mut r) % (5 * nodes);

        let edges = random_edges(&mut r, nodes, count, 100000);
        check_against_reference(&edges, nodes, &[]);
    }
}

/// Every weight identical is the case that breaks a naive pivot: the light partition can come
/// back empty and the recursion never shrinks.
#[test]
fn all_weights_equal() {
    let mut r = Mt19937::from_seed(12);

    for _trial in 0..8 {
        let nodes = 500 + rng(&mut r) % 2000;
        let mut edges = random_edges(&mut r, nodes, 5 * nodes, 100000);

        for edge in &mut edges {
            edge.weight = 0;
        }

        check_against_reference(&edges, nodes, &[]);
    }
}

#[test]
fn few_distinct_weights() {
    let mut r = Mt19937::from_seed(606);

    for _trial in 0..8 {
        let nodes = 1000 + rng(&mut r) % 1500;

        let edges = random_edges(&mut r, nodes, 6 * nodes, 2);
        check_against_reference(&edges, nodes, &[]);
    }
}

/// A disconnected graph yields a spanning forest, not a tree, so the ComponentCount()-based
/// early exit never fires and the recursion has to bottom out on its own.
#[test]
fn disconnected_graph_yields_forest() {
    let mut r = Mt19937::from_seed(808);

    let nodes = 4000;
    let islands = 8;
    let island_size = nodes / islands;

    let mut edges = Vec::new();

    for _ in 0..20000 {
        let island = rng(&mut r) % islands;
        let u = island * island_size + rng(&mut r) % island_size;
        let v = island * island_size + rng(&mut r) % island_size;
        let weight = (rng(&mut r) % 1000) as i64;
        edges.push(TestEdge { u, v, weight });
    }

    check_against_reference(&edges, nodes, &[]);
}

/// RN_NET seeds the forest with the connections the board already makes before handing over
/// the candidate edges, so pre-united endpoints have to be filtered rather than selected.
#[test]
fn honours_pre_seeded_forest() {
    let mut r = Mt19937::from_seed(909);

    let nodes = 3000;

    let mut seeds = Vec::new();

    for _ in 0..2000 {
        let a = rng(&mut r) % nodes;
        let b = rng(&mut r) % nodes;
        seeds.push((a, b));
    }

    let mut edges = random_edges(&mut r, nodes, 15000, 5000);

    for edge in &mut edges {
        edge.weight += 1;
    }

    check_against_reference(&edges, nodes, &seeds);
}

/// Running the same input twice must give the same tree.  A randomised pivot would satisfy
/// the weight check above but not this, and the ratsnest is drawn from the result.
#[test]
fn selection_is_reproducible() {
    let mut r = Mt19937::from_seed(246);

    let nodes = 5000;
    let edges = random_edges(&mut r, nodes, 25000, 50);

    let mut count = 0;
    let first = run_filter_kruskal(edges.clone(), nodes, &[], &mut count);

    for _repeat in 0..4 {
        let again = run_filter_kruskal(edges.clone(), nodes, &[], &mut count);
        assert_eq!(again, first);
    }
}

/// Reproducibility only follows if the comparator is a total order on *undirected* edges,
/// which takes a canonical endpoint key.  Feed the same graph with the edge list shuffled and
/// individual edges flipped end for end; the selected tree must not move.  Rerunning an
/// identical input, as the case above does, cannot detect the difference.
#[test]
fn selection_ignores_input_order_and_orientation() {
    let canonical = |edge: &TestEdge| {
        let low = edge.u.min(edge.v);
        let high = edge.u.max(edge.v);
        (edge.weight, low, high)
    };

    let canonical_order = |lhs: &TestEdge, rhs: &TestEdge| canonical(lhs) < canonical(rhs);

    let mut r = Mt19937::from_seed(1357);

    let nodes = 4000;
    let edges = random_edges(&mut r, nodes, 20000, 40);

    let select = |mut input: Vec<TestEdge>| {
        let mut forest = KiUnionFind::new(nodes);
        let mut tree = Vec::new();

        filter_kruskal(&mut input, &mut forest, canonical_order, edge_ends, |e| {
            tree.push(*e)
        });

        let mut keys: Vec<(i64, usize, usize)> = tree.iter().map(canonical).collect();
        keys.sort();
        keys
    };

    let expected = select(edges.clone());

    for _trial in 0..8 {
        let mut permuted = edges.clone();

        // std::shuffle stand-in (Fisher-Yates)
        for i in (1..permuted.len()).rev() {
            let j = rng(&mut r) % (i + 1);
            permuted.swap(i, j);
        }

        for edge in &mut permuted {
            if r.next_u32() & 1 != 0 {
                std::mem::swap(&mut edge.u, &mut edge.v);
            }
        }

        assert_eq!(select(permuted), expected);
    }
}

#[test]
fn degenerate_inputs() {
    let e = |u, v, weight| TestEdge { u, v, weight };

    check_against_reference(&[], 1, &[]);
    check_against_reference(&[e(0, 1, 5)], 2, &[]);

    // Self loops can never be selected.
    check_against_reference(&[e(0, 0, 1), e(1, 1, 2), e(0, 1, 3)], 2, &[]);

    // Parallel edges: only the lightest of each pair may be taken.
    let mut count = 0;
    let tree = run_filter_kruskal(vec![e(0, 1, 9), e(0, 1, 2), e(0, 1, 7)], 2, &[], &mut count);

    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].weight, 2);
}
