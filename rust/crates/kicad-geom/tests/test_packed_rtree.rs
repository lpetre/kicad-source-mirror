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

//! Port of `qa/tests/libs/kimath/geometry/test_packed_rtree.cpp` (suite `PackedRTree`).

use std::collections::BTreeSet;

use kicad_geom::geometry::rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_geom::test_utils::{Mt19937, uniform_int_libstdcxx};

type Builder2 = PackedRTreeBuilder<isize, 2>;

#[test]
fn empty_tree() {
    let tree = PackedRTree::<isize, 2>::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);

    assert_eq!(tree.search(&[0, 0], &[100, 100], |_| true), 0);
    assert!(tree.iter().next().is_none());
}

#[test]
fn single_item() {
    let mut builder = Builder2::new();
    builder.add(&[10, 20], &[30, 40], 42);
    let tree = builder.build();

    assert!(!tree.is_empty());
    assert_eq!(tree.len(), 1);

    // Search that overlaps
    let mut results = Vec::new();
    let found = tree.search(&[0, 0], &[15, 25], |&v| {
        results.push(v);
        true
    });
    assert_eq!(found, 1);
    assert_eq!(results, [42]);

    // Search that doesn't overlap
    results.clear();
    let found = tree.search(&[100, 100], &[200, 200], |&v| {
        results.push(v);
        true
    });
    assert_eq!(found, 0);
    assert!(results.is_empty());
}

#[test]
fn known_grid() {
    // Create a 10x10 grid of non-overlapping 10x10 items
    let mut builder = Builder2::new();
    builder.reserve(100);

    for y in 0..10 {
        for x in 0..10 {
            builder.add(
                &[x * 10, y * 10],
                &[x * 10 + 10, y * 10 + 10],
                (y * 10 + x) as isize,
            );
        }
    }

    let tree = builder.build();
    assert_eq!(tree.len(), 100);

    // Search a region that should contain exactly 4 items (2x2 in top-left)
    let mut results = BTreeSet::new();
    tree.search(&[0, 0], &[19, 19], |&v| {
        results.insert(v);
        true
    });
    assert_eq!(results.len(), 4);
    assert!(results.contains(&0)); // (0,0)
    assert!(results.contains(&1)); // (1,0)
    assert!(results.contains(&10)); // (0,1)
    assert!(results.contains(&11)); // (1,1)

    // Search entire region should return all 100
    results.clear();
    tree.search(&[0, 0], &[100, 100], |&v| {
        results.insert(v);
        true
    });
    assert_eq!(results.len(), 100);
}

#[test]
fn point_items() {
    // Zero-area bounding boxes (points)
    let mut builder = Builder2::new();

    for i in 0..50 {
        builder.add(&[i * 10, i * 10], &[i * 10, i * 10], i as isize);
    }

    let tree = builder.build();
    assert_eq!(tree.len(), 50);

    // Search a point
    let mut results = Vec::new();
    tree.search(&[100, 100], &[100, 100], |&v| {
        results.push(v);
        true
    });
    assert_eq!(results, [10]);
}

#[test]
fn overlapping_items() {
    let mut builder = Builder2::new();

    // All items overlap at the origin region
    for i in 0..20 {
        builder.add(&[-i, -i], &[i + 10, i + 10], i as isize);
    }

    let tree = builder.build();

    // Search at origin should find all 20
    let mut count = 0;
    tree.search(&[0, 0], &[0, 0], |_| {
        count += 1;
        true
    });
    assert_eq!(count, 20);
}

#[test]
fn early_termination() {
    let mut builder = Builder2::new();

    for i in 0..100 {
        builder.add(&[0, 0], &[100, 100], i as isize);
    }

    let tree = builder.build();

    // Visitor that stops after 3 items
    let mut count = 0;
    tree.search(&[0, 0], &[100, 100], |_| {
        count += 1;
        count < 3
    });
    assert_eq!(count, 3);
}

#[test]
fn full_iteration() {
    let mut builder = Builder2::new();

    for i in 0..100 {
        builder.add(&[i, i], &[i + 1, i + 1], i as isize);
    }

    let tree = builder.build();
    let iterated: BTreeSet<isize> = tree.iter().copied().collect();
    assert_eq!(iterated.len(), 100);

    for i in 0..100 {
        assert!(iterated.contains(&i));
    }
}

#[test]
fn large_dataset_brute_force() {
    // Verify correctness against brute-force for 10K items
    const N: usize = 10000;
    let mut rng = Mt19937::new(12345);
    let coord = |rng: &mut Mt19937| uniform_int_libstdcxx(rng, -1000000, 1000000) as i32;
    let size = |rng: &mut Mt19937| uniform_int_libstdcxx(rng, 1, 10000) as i32;

    let mut items: Vec<([i32; 2], [i32; 2], isize)> = Vec::with_capacity(N);
    let mut builder = Builder2::new();
    builder.reserve(N);

    for i in 0..N {
        let x = coord(&mut rng);
        let y = coord(&mut rng);
        let w = size(&mut rng);
        let h = size(&mut rng);
        let item = ([x, y], [x + w, y + h], i as isize);
        builder.add(&item.0, &item.1, item.2);
        items.push(item);
    }

    let tree = builder.build();

    // Run 100 random queries, verify against brute-force
    for _ in 0..100 {
        let qx = coord(&mut rng);
        let qy = coord(&mut rng);
        let qw = size(&mut rng) * 10;
        let qh = size(&mut rng) * 10;
        let q_min = [qx, qy];
        let q_max = [qx + qw, qy + qh];

        let expected: BTreeSet<isize> = items
            .iter()
            .filter(|(mn, mx, _)| {
                mn[0] <= q_max[0] && mx[0] >= q_min[0] && mn[1] <= q_max[1] && mx[1] >= q_min[1]
            })
            .map(|it| it.2)
            .collect();

        let mut actual = BTreeSet::new();
        tree.search(&q_min, &q_max, |&v| {
            actual.insert(v);
            true
        });

        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }
}

#[test]
fn memory_usage() {
    let mut builder = Builder2::new();
    builder.reserve(1000);

    for i in 0..1000 {
        builder.add(&[i, i], &[i + 1, i + 1], i as isize);
    }

    let tree = builder.build();
    let mem_usage = tree.memory_usage();

    // Should be reasonable: data (8KB for pointers) + node overhead
    // With fanout=16, 1000 items need ~63 leaf nodes + ~5 internal nodes
    // Memory overhead should be modest relative to item storage
    let ptr = std::mem::size_of::<isize>();
    assert!(mem_usage > 1000 * ptr);
    assert!(mem_usage < 1000 * ptr * 5); // < 5x overhead
}

#[test]
fn move_semantics() {
    let mut builder = Builder2::new();

    for i in 0..50 {
        builder.add(&[i, i], &[i + 10, i + 10], i as isize);
    }

    let mut tree1 = builder.build();
    assert_eq!(tree1.len(), 50);

    // Move construct
    let mut tree2 = std::mem::take(&mut tree1);
    assert_eq!(tree2.len(), 50);
    assert_eq!(tree1.len(), 0);
    assert!(tree1.is_empty());

    // Move assign
    let mut tree3 = PackedRTree::<isize, 2>::new();
    assert!(tree3.is_empty());
    tree3 = std::mem::take(&mut tree2);
    assert_eq!(tree3.len(), 50);
    assert_eq!(tree2.len(), 0);

    // Verify the moved-to tree still works
    let mut count = 0;
    tree3.search(&[0, 0], &[100, 100], |_| {
        count += 1;
        true
    });
    assert_eq!(count, 50);
}

#[test]
fn three_dimensional() {
    let mut builder = PackedRTreeBuilder::<isize, 3>::new();

    for i in 0..100 {
        builder.add(&[i, i, i], &[i + 5, i + 5, i + 5], i as isize);
    }

    let tree = builder.build();
    assert_eq!(tree.len(), 100);

    // Search a region
    let mut count = 0;
    tree.search(&[10, 10, 10], &[20, 20, 20], |_| {
        count += 1;
        true
    });
    assert!(count > 0);
}
