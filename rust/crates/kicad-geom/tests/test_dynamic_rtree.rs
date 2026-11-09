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

//! Port of `qa/tests/libs/kimath/geometry/test_dynamic_rtree.cpp` (suite `DynamicRTree`).
//! The suite `CowRTree` of the same file is not ported because `COW_RTREE` is not (see
//! `PORT_STATUS.md`).

use std::collections::BTreeSet;

use kicad_geom::geometry::rtree::rtree_node::hilbert_nd2d;
use kicad_geom::geometry::rtree::{BulkEntry, DynamicRTree};
use kicad_geom::test_utils::{Mt19937, uniform_int_libstdcxx};

type Tree2 = DynamicRTree<isize, 2>;

/// `std::uniform_int_distribution<int>( a, b )( rng )`
fn dist(rng: &mut Mt19937, a: i32, b: i32) -> i32 {
    uniform_int_libstdcxx(rng, a as i64, b as i64) as i32
}

fn overlaps<const D: usize>(
    mn: &[i32; D],
    mx: &[i32; D],
    q_min: &[i32; D],
    q_max: &[i32; D],
) -> bool {
    (0..D).all(|d| mn[d] <= q_max[d] && mx[d] >= q_min[d])
}

fn collect_set<T: Copy + Ord, const D: usize>(
    tree: &DynamicRTree<T, D>,
    mn: &[i32; D],
    mx: &[i32; D],
) -> BTreeSet<T> {
    let mut out = BTreeSet::new();
    tree.search(mn, mx, |&v| {
        out.insert(v);
        true
    });
    out
}

#[test]
fn empty_tree() {
    let tree = Tree2::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    assert_eq!(tree.search(&[0, 0], &[100, 100], |_| true), 0);
    assert!(tree.iter().next().is_none());
}

#[test]
fn single_insert() {
    let mut tree = Tree2::new();
    tree.insert(&[10, 20], &[30, 40], 42);
    assert_eq!(tree.len(), 1);
    assert!(!tree.is_empty());

    let mut results = Vec::new();
    let found = tree.search(&[0, 0], &[15, 25], |&v| {
        results.push(v);
        true
    });
    assert_eq!(found, 1);
    assert_eq!(results[0], 42);

    assert_eq!(tree.search(&[100, 100], &[200, 200], |_| true), 0);
}

#[test]
fn bulk_insert_1k() {
    let mut tree = Tree2::new();

    for i in 0..1000 {
        tree.insert(&[i * 10, i * 10], &[i * 10 + 9, i * 10 + 9], i as isize);
    }

    assert_eq!(tree.len(), 1000);

    let results = collect_set(&tree, &[500 * 10, 500 * 10], &[500 * 10 + 9, 500 * 10 + 9]);
    assert!(results.contains(&500));
}

#[test]
fn remove_existing() {
    let mut tree = Tree2::new();
    tree.insert(&[0, 0], &[10, 10], 1);
    tree.insert(&[20, 20], &[30, 30], 2);
    assert_eq!(tree.len(), 2);

    assert!(tree.remove(&[0, 0], &[10, 10], &1));
    assert_eq!(tree.len(), 1);

    let mut results = Vec::new();
    tree.search(&[-100, -100], &[100, 100], |&v| {
        results.push(v);
        true
    });
    assert_eq!(results, [2]);
}

#[test]
fn remove_non_existent() {
    let mut tree = Tree2::new();
    tree.insert(&[0, 0], &[10, 10], 1);
    assert!(!tree.remove(&[100, 100], &[200, 200], &999));
    assert_eq!(tree.len(), 1);
}

#[test]
fn search_overlap_disjoint_all() {
    let mut tree = Tree2::new();
    tree.insert(&[0, 0], &[10, 10], 1);
    tree.insert(&[100, 0], &[110, 10], 2);
    tree.insert(&[0, 100], &[10, 110], 3);
    tree.insert(&[100, 100], &[110, 110], 4);

    // Overlapping query (left half)
    let results = collect_set(&tree, &[-10, -10], &[50, 200]);
    assert_eq!(results.len(), 2);
    assert!(results.contains(&1));
    assert!(results.contains(&3));

    // Disjoint query (middle gap)
    assert!(collect_set(&tree, &[50, 50], &[90, 90]).is_empty());

    // All-encompassing query
    assert_eq!(collect_set(&tree, &[-1000, -1000], &[1000, 1000]).len(), 4);
}

#[test]
fn bulk_insert_and_brute_force() {
    const N: usize = 5000;
    let mut rng = Mt19937::new(67890);
    let mut items: Vec<([i32; 2], [i32; 2], isize)> = Vec::with_capacity(N);
    let mut tree = Tree2::new();

    for i in 0..N {
        let x = dist(&mut rng, -500000, 500000);
        let y = dist(&mut rng, -500000, 500000);
        let w = dist(&mut rng, 1, 5000);
        let h = dist(&mut rng, 1, 5000);
        let item = ([x, y], [x + w, y + h], i as isize);
        tree.insert(&item.0, &item.1, item.2);
        items.push(item);
    }

    assert_eq!(tree.len(), N);

    for _ in 0..50 {
        let qx = dist(&mut rng, -500000, 500000);
        let qy = dist(&mut rng, -500000, 500000);
        let qw = dist(&mut rng, 1, 5000) * 10;
        let qh = dist(&mut rng, 1, 5000) * 10;
        let q_min = [qx, qy];
        let q_max = [qx + qw, qy + qh];

        let expected: BTreeSet<isize> = items
            .iter()
            .filter(|(mn, mx, _)| overlaps(mn, mx, &q_min, &q_max))
            .map(|it| it.2)
            .collect();

        let actual = collect_set(&tree, &q_min, &q_max);
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }
}

#[test]
fn remove_and_verify() {
    const N: i32 = 200;
    let mut tree = Tree2::new();
    let items: Vec<([i32; 2], [i32; 2], isize)> = (0..N)
        .map(|i| ([i * 5, i * 5], [i * 5 + 4, i * 5 + 4], i as isize))
        .collect();

    for item in &items {
        tree.insert(&item.0, &item.1, item.2);
    }

    // Remove every even item
    for item in items.iter().step_by(2) {
        assert!(tree.remove(&item.0, &item.1, &item.2));
    }

    assert_eq!(tree.len(), (N / 2) as usize);

    let remaining = collect_set(&tree, &[-10000, -10000], &[10000, 10000]);
    assert_eq!(remaining.len(), (N / 2) as usize);

    for i in (1..N).step_by(2) {
        assert!(remaining.contains(&(i as isize)));
    }
}

#[test]
fn move_semantics() {
    let mut tree = Tree2::new();

    for i in 0..50 {
        tree.insert(&[i, i], &[i + 5, i + 5], i as isize);
    }

    assert_eq!(tree.len(), 50);

    let mut tree2 = std::mem::take(&mut tree);
    assert_eq!(tree2.len(), 50);
    assert_eq!(tree.len(), 0);

    let mut tree3 = Tree2::new();
    assert!(tree3.is_empty());
    tree3 = std::mem::take(&mut tree2);
    assert_eq!(tree3.len(), 50);
    assert_eq!(tree2.len(), 0);

    let mut count = 0;
    tree3.search(&[0, 0], &[100, 100], |_| {
        count += 1;
        true
    });
    assert_eq!(count, 50);
}

#[test]
fn remove_all() {
    let mut tree = Tree2::new();

    for i in 0..100 {
        tree.insert(&[i, i], &[i + 1, i + 1], i as isize);
    }

    assert_eq!(tree.len(), 100);
    tree.remove_all();
    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());

    // Can still insert after RemoveAll
    tree.insert(&[0, 0], &[10, 10], 999);
    assert_eq!(tree.len(), 1);
}

#[test]
fn three_dimensional() {
    let mut tree = DynamicRTree::<isize, 3>::new();

    for i in 0..100 {
        tree.insert(&[i, i, i], &[i + 5, i + 5, i + 5], i as isize);
    }

    assert_eq!(tree.len(), 100);

    let mut count = 0;
    tree.search(&[10, 10, 10], &[20, 20, 20], |_| {
        count += 1;
        true
    });
    assert!(count > 0);
}

#[test]
fn nearest_neighbors() {
    let mut tree = Tree2::new();

    for i in 0..20 {
        tree.insert(&[i * 100, 0], &[i * 100 + 10, 10], i as isize);
    }

    let results = tree.nearest_neighbors(&[0, 0], 3);
    assert_eq!(results.len(), 3);
    assert_eq!(*results[0].1, 0);

    for i in 1..results.len() {
        assert!(results[i].0 >= results[i - 1].0);
    }
}

#[test]
fn iterator_full_coverage() {
    let mut tree = Tree2::new();

    for i in 0..200 {
        tree.insert(&[i * 3, i * 7], &[i * 3 + 2, i * 7 + 6], i as isize);
    }

    let iterated: BTreeSet<isize> = tree.iter().copied().collect();
    assert_eq!(iterated.len(), 200);

    for i in 0..200 {
        assert!(iterated.contains(&i));
    }
}

#[test]
fn early_termination() {
    let mut tree = Tree2::new();

    for i in 0..100 {
        tree.insert(&[0, 0], &[100, 100], i as isize);
    }

    let mut count = 0;
    tree.search(&[0, 0], &[100, 100], |_| {
        count += 1;
        count < 5
    });
    assert_eq!(count, 5);
}

#[test]
fn remove_moved_item() {
    let mut tree = Tree2::new();
    tree.insert(&[100, 100], &[200, 200], 42);
    tree.insert(&[500, 500], &[600, 600], 99);
    assert_eq!(tree.len(), 2);

    // Remove using a bbox that doesn't match the insertion bbox: the stored insertion bbox
    // fallback finds it
    assert!(tree.remove(&[300, 300], &[400, 400], &42));
    assert_eq!(tree.len(), 1);

    let mut results = Vec::new();
    tree.search(&[-1000, -1000], &[1000, 1000], |&v| {
        results.push(v);
        true
    });
    assert_eq!(results, [99]);
}

#[test]
fn stress_interleaved_insert_remove_query() {
    const N: i32 = 1000;
    let mut rng = Mt19937::new(11111);
    let mut tree = Tree2::new();
    let mut live_items: Vec<([i32; 2], [i32; 2], isize)> = Vec::new();

    for op in 0..N {
        let which = dist(&mut rng, 0, 9);

        if which < 5 || live_items.is_empty() {
            let x = dist(&mut rng, 0, 100000);
            let y = dist(&mut rng, 0, 100000);
            let w = dist(&mut rng, 100, 5000);
            let h = dist(&mut rng, 100, 5000);
            let item = ([x, y], [x + w, y + h], op as isize);
            tree.insert(&item.0, &item.1, item.2);
            live_items.push(item);
        } else if which < 7 {
            let idx = dist(&mut rng, 0, live_items.len() as i32 - 1) as usize;
            let item = live_items[idx];
            assert!(tree.remove(&item.0, &item.1, &item.2));
            live_items.remove(idx);
        } else {
            let q0 = dist(&mut rng, 0, 100000);
            let q1 = dist(&mut rng, 0, 100000);
            let q_min = [q0, q1];
            let s0 = dist(&mut rng, 100, 5000);
            let s1 = dist(&mut rng, 100, 5000);
            let q_max = [q_min[0] + s0 * 5, q_min[1] + s1 * 5];

            let expected = live_items
                .iter()
                .filter(|(mn, mx, _)| overlaps(mn, mx, &q_min, &q_max))
                .count();

            let actual = collect_set(&tree, &q_min, &q_max);
            assert_eq!(actual.len(), expected);
        }
    }

    assert_eq!(tree.len(), live_items.len());
}

#[test]
fn bulk_load_empty() {
    let mut tree = DynamicRTree::<i32, 2>::new();
    tree.bulk_load(Vec::new());
    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());
}

#[test]
fn bulk_load_single_item() {
    let mut tree = DynamicRTree::<i32, 2>::new();
    tree.bulk_load(vec![BulkEntry {
        min: [10, 20],
        max: [30, 40],
        data: 42,
    }]);
    assert_eq!(tree.len(), 1);

    let mut results = Vec::new();
    tree.search(&[0, 0], &[100, 100], |&v| {
        results.push(v);
        true
    });
    assert_eq!(results, [42]);
}

#[test]
fn bulk_load_correctness_vs_brute_force() {
    const N: usize = 5000;
    let mut rng = Mt19937::new(99999);
    let mut items: Vec<([i32; 2], [i32; 2], i32)> = Vec::with_capacity(N);

    for i in 0..N {
        let x = dist(&mut rng, 0, 100000);
        let y = dist(&mut rng, 0, 100000);
        let w = dist(&mut rng, 100, 5000);
        let h = dist(&mut rng, 100, 5000);
        items.push(([x, y], [x + w, y + h], i as i32));
    }

    let mut tree = DynamicRTree::<i32, 2>::new();
    tree.bulk_load(
        items
            .iter()
            .map(|it| BulkEntry {
                min: it.0,
                max: it.1,
                data: it.2,
            })
            .collect(),
    );
    assert_eq!(tree.len(), N);

    for _ in 0..200 {
        let q0 = dist(&mut rng, 0, 100000);
        let q1 = dist(&mut rng, 0, 100000);
        let q_min = [q0, q1];
        let s0 = dist(&mut rng, 100, 5000);
        let s1 = dist(&mut rng, 100, 5000);
        let q_max = [q_min[0] + s0 * 5, q_min[1] + s1 * 5];

        let expected: BTreeSet<i32> = items
            .iter()
            .filter(|(mn, mx, _)| overlaps(mn, mx, &q_min, &q_max))
            .map(|it| it.2)
            .collect();

        let actual = collect_set(&tree, &q_min, &q_max);
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }

    // Verify iteration covers all items
    let all_via_iter: BTreeSet<i32> = tree.iter().copied().collect();
    assert_eq!(all_via_iter.len(), N);
}

#[test]
fn morton_index_orders_on_high_bits() {
    // A 64-bit Morton index keeps 21 bits per axis in 3D; it must take them from the top of
    // each coordinate
    let origin = [0u32, 0, 0];

    for dim in 0..3 {
        let mut top_bit_set = [0u32, 0, 0];
        top_bit_set[dim] = 0x80000000;
        assert_ne!(
            hilbert_nd2d::<3>(32, &origin),
            hilbert_nd2d::<3>(32, &top_bit_set)
        );
    }

    let near_half = [0x10000000u32, 0x20000000, 0x30000000];
    let far_half = [0x90000000u32, 0xA0000000, 0xB0000000];
    assert!(hilbert_nd2d::<3>(32, &near_half) < hilbert_nd2d::<3>(32, &far_half));
}

#[test]
fn bulk_load_3d_correctness_vs_brute_force() {
    const N: usize = 4000;
    let mut rng = Mt19937::new(12345);
    let mut items: Vec<([i32; 3], [i32; 3], i32)> = Vec::with_capacity(N);

    for i in 0..N {
        let l0 = dist(&mut rng, 0, 31);
        let l1 = l0 + dist(&mut rng, 0, 31) / 8;
        let x = dist(&mut rng, 0, 100000);
        let y = dist(&mut rng, 0, 100000);
        let w = dist(&mut rng, 100, 5000);
        let h = dist(&mut rng, 100, 5000);
        items.push(([l0, x, y], [l1, x + w, y + h], i as i32));
    }

    let mut tree = DynamicRTree::<i32, 3>::new();
    tree.bulk_load(
        items
            .iter()
            .map(|it| BulkEntry {
                min: it.0,
                max: it.1,
                data: it.2,
            })
            .collect(),
    );
    assert_eq!(tree.len(), N);

    let all_via_iter: BTreeSet<i32> = tree.iter().copied().collect();
    assert_eq!(all_via_iter.len(), N);

    for _ in 0..200 {
        let l = dist(&mut rng, 0, 31);
        let x = dist(&mut rng, 0, 100000);
        let y = dist(&mut rng, 0, 100000);
        let q_min = [l, x, y];
        let s0 = dist(&mut rng, 100, 5000);
        let s1 = dist(&mut rng, 100, 5000);
        let q_max = [q_min[0] + 2, q_min[1] + s0 * 5, q_min[2] + s1 * 5];

        let expected: BTreeSet<i32> = items
            .iter()
            .filter(|(mn, mx, _)| overlaps(mn, mx, &q_min, &q_max))
            .map(|it| it.2)
            .collect();

        assert_eq!(collect_set(&tree, &q_min, &q_max), expected);
    }
}

#[test]
fn bulk_load_then_insert_and_remove() {
    const N: i32 = 500;
    let mut rng = Mt19937::new(77777);
    let mut entries: Vec<BulkEntry<i32, 2>> = Vec::new();
    let mut live_items: Vec<([i32; 2], [i32; 2], i32)> = Vec::new();

    let gen_item = |rng: &mut Mt19937, id: i32| {
        let x = dist(rng, 0, 100000);
        let y = dist(rng, 0, 100000);
        let w = dist(rng, 100, 5000);
        let h = dist(rng, 100, 5000);
        ([x, y], [x + w, y + h], id)
    };

    for i in 0..N {
        let item = gen_item(&mut rng, i);
        entries.push(BulkEntry {
            min: item.0,
            max: item.1,
            data: item.2,
        });
        live_items.push(item);
    }

    let mut tree = DynamicRTree::<i32, 2>::new();
    tree.bulk_load(entries);
    assert_eq!(tree.len(), N as usize);

    // Insert additional items after bulk load
    for i in N..N + 100 {
        let item = gen_item(&mut rng, i);
        tree.insert(&item.0, &item.1, item.2);
        live_items.push(item);
    }

    assert_eq!(tree.len(), (N + 100) as usize);

    // Remove some items
    for _ in 0..50 {
        let idx = dist(&mut rng, 0, live_items.len() as i32 - 1) as usize;
        let item = live_items[idx];
        assert!(tree.remove(&item.0, &item.1, &item.2));
        live_items.remove(idx);
    }

    assert_eq!(tree.len(), live_items.len());

    let q_min = [0, 0];
    let q_max = [200000, 200000];
    let expected = live_items
        .iter()
        .filter(|(mn, mx, _)| overlaps(mn, mx, &q_min, &q_max))
        .count();
    assert_eq!(collect_set(&tree, &q_min, &q_max).len(), expected);
}

/// Not in the C++ test: the lazy `Overlapping` range visits the same items in the same order
/// as `Search`.
#[test]
fn overlapping_matches_search_order() {
    let mut rng = Mt19937::new(4242);
    let mut tree = Tree2::new();

    for i in 0..3000 {
        let x = dist(&mut rng, 0, 100000);
        let y = dist(&mut rng, 0, 100000);
        let w = dist(&mut rng, 0, 3000);
        let h = dist(&mut rng, 0, 3000);
        tree.insert(&[x, y], &[x + w, y + h], i as isize);
    }

    for _ in 0..100 {
        let x = dist(&mut rng, 0, 100000);
        let y = dist(&mut rng, 0, 100000);
        let s = dist(&mut rng, 0, 20000);
        let mut by_search = Vec::new();
        tree.search(&[x, y], &[x + s, y + s], |&v| {
            by_search.push(v);
            true
        });
        let by_range: Vec<isize> = tree
            .overlapping(&[x, y], &[x + s, y + s])
            .copied()
            .collect();
        assert_eq!(by_search, by_range);
    }
}
