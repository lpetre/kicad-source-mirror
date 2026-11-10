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

//! Port of `qa/tests/common/test_union_find.cpp` (suite `UnionFind`) for
//! [`kicad_connectivity::algo::KiUnionFind`].
//!
//! The port is single-threaded: `ConcurrentUniteMatchesSerial` and
//! `ReverseChainUnderContention` interleave the edges the way their threads would (edge `ii`
//! goes to thread `ii % threadCount`, thread by thread), with `threadCount = 4`; the
//! expectations are unchanged. `rng() % n` on `std::mt19937` (32-bit results) is reproduced
//! with `kicad_core::mt19937`.

use std::collections::BTreeMap;

use kicad_connectivity::algo::KiUnionFind;
use kicad_core::mt19937::Mt19937;

/// Textbook union-by-size with full path compression. Deliberately a different algorithm from
/// the one under test so that agreement means something.
struct ReferenceSet {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl ReferenceSet {
    fn new(count: usize) -> Self {
        ReferenceSet {
            parent: (0..count).collect(),
            size: vec![1; count],
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }

        x
    }

    fn unite(&mut self, a: usize, b: usize) -> bool {
        let mut a = self.find(a);
        let mut b = self.find(b);

        if a == b {
            return false;
        }

        if self.size[a] < self.size[b] {
            std::mem::swap(&mut a, &mut b);
        }

        self.parent[b] = a;
        self.size[a] += self.size[b];
        true
    }
}

fn rnd(rng: &mut Mt19937) -> usize {
    rng.next_u32() as usize
}

fn random_edges(rng: &mut Mt19937, nodes: usize, count: usize) -> Vec<(usize, usize)> {
    let mut edges = Vec::with_capacity(count);

    for _ in 0..count {
        // emplace_back( aRng() % aNodes, aRng() % aNodes ): the C++ argument order is
        // unspecified; the checks compare against the reference on the same edges, so the
        // exact edge list does not matter.
        let a = rnd(rng) % nodes;
        let b = rnd(rng) % nodes;
        edges.push((a, b));
    }

    edges
}

/// The partition, the merge count and the component count must all track a reference
/// implementation exactly, across a spread of graph densities.
#[test]
fn matches_reference_implementation() {
    let mut rng = Mt19937::from_seed(4242);

    for _trial in 0..100 {
        let nodes = 1 + rnd(&mut rng) % 300;
        let count = rnd(&mut rng) % (3 * nodes + 1);
        let edges = random_edges(&mut rng, nodes, count);

        let mut under = KiUnionFind::new(nodes);
        let mut reference = ReferenceSet::new(nodes);
        let mut merges = 0;
        let mut reference_merges = 0;

        for &(a, b) in &edges {
            merges += usize::from(under.unite(a, b));
            reference_merges += usize::from(reference.unite(a, b));
        }

        assert_eq!(merges, reference_merges);
        assert_eq!(under.component_count(), nodes - reference_merges);

        for i in 0..nodes {
            for j in 0..nodes {
                let expected = reference.find(i) == reference.find(j);
                assert_eq!(under.connected(i, j), expected);
            }
        }
    }
}

/// Rem's algorithm roots every tree at the smallest index it holds.
#[test]
fn root_is_minimum_of_component() {
    let mut rng = Mt19937::from_seed(99);
    let nodes = 2000;
    let mut under = KiUnionFind::new(nodes);
    let mut reference = ReferenceSet::new(nodes);

    for (a, b) in random_edges(&mut rng, nodes, 5000) {
        under.unite(a, b);
        reference.unite(a, b);
    }

    let mut minimum_of: BTreeMap<usize, usize> = BTreeMap::new();

    for ii in 0..nodes {
        let root = reference.find(ii);
        minimum_of.entry(root).or_insert(ii);
    }

    for ii in 0..nodes {
        let root = reference.find(ii);
        assert_eq!(under.find(ii), minimum_of[&root]);
    }
}

/// FindCompress() rewrites the tree as it walks. It must return what Find() would have, and
/// must not disturb the partition.
#[test]
fn compression_preserves_partition() {
    let mut rng = Mt19937::from_seed(7);
    let nodes = 4000;
    let mut under = KiUnionFind::new(nodes);
    let mut reference = ReferenceSet::new(nodes);

    for (a, b) in random_edges(&mut rng, nodes, 9000) {
        under.unite(a, b);
        reference.unite(a, b);
    }

    let before: Vec<usize> = (0..nodes).map(|ii| under.find(ii)).collect();

    for (ii, &b) in before.iter().enumerate() {
        assert_eq!(under.find_compress(ii), b);
    }

    for (ii, &b) in before.iter().enumerate() {
        assert_eq!(under.find(ii), b);
    }
}

const THREAD_COUNT: usize = 4;

/// The final partition must not depend on how the edges were divided up, and exactly one
/// caller may see true per merge.
#[test]
fn concurrent_unite_matches_serial() {
    let mut rng = Mt19937::from_seed(31337);
    let nodes = 20000;
    let edges = random_edges(&mut rng, nodes, 50000);
    let mut under = KiUnionFind::new(nodes);
    let mut merges = 0;

    for t in 0..THREAD_COUNT {
        for ii in (t..edges.len()).step_by(THREAD_COUNT) {
            merges += usize::from(under.unite(edges[ii].0, edges[ii].1));
        }
    }

    let mut reference = ReferenceSet::new(nodes);
    let mut reference_merges = 0;

    for &(a, b) in &edges {
        reference_merges += usize::from(reference.unite(a, b));
    }

    assert_eq!(merges, reference_merges);
    assert_eq!(under.component_count(), nodes - reference_merges);

    for ii in 0..nodes {
        let other = (ii * 7919) % nodes;
        let expected = reference.find(ii) == reference.find(other);
        assert_eq!(under.connected(ii, other), expected);
    }
}

/// A chain united back to front is the worst case for Rem's interleaved climb: it must still
/// terminate and collapse to one component.
#[test]
fn reverse_chain_under_contention() {
    let nodes: usize = 50000;
    let mut under = KiUnionFind::new(nodes);
    let mut merges = 0;

    for t in 0..THREAD_COUNT as i64 {
        let mut ii = nodes as i64 - 2 - t;

        while ii >= 0 {
            merges += usize::from(under.unite(ii as usize, ii as usize + 1));
            ii -= THREAD_COUNT as i64;
        }
    }

    assert_eq!(merges, nodes - 1);
    assert_eq!(under.component_count(), 1);

    for ii in (0..nodes).step_by(997) {
        assert_eq!(under.find(ii), 0);
    }
}

#[test]
fn degenerate_sizes() {
    let empty = KiUnionFind::new(0);
    assert_eq!(empty.size(), 0);
    assert_eq!(empty.component_count(), 0);

    let mut singleton = KiUnionFind::new(1);
    assert!(!singleton.unite(0, 0));
    assert_eq!(singleton.component_count(), 1);
    assert!(singleton.connected(0, 0));

    let mut pair = KiUnionFind::new(2);
    assert!(pair.unite(1, 0));
    assert!(!pair.unite(0, 1));
    assert_eq!(pair.component_count(), 1);
    assert_eq!(pair.find(1), 0);
}

#[test]
fn reset_clears_state() {
    let mut under = KiUnionFind::new(10);

    for ii in 1..10 {
        under.unite(0, ii);
    }

    assert_eq!(under.component_count(), 1);

    under.reset(5);
    assert_eq!(under.size(), 5);
    assert_eq!(under.component_count(), 5);

    for ii in 0..5 {
        assert_eq!(under.find(ii), ii);
    }
}
