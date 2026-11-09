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

//! The case generators of the spatial differential test: the Rust mirror of
//! `spatial_oracle.cpp` in this directory (libstdc++ `std::sort` and friends, the `KIRTREE`
//! r-trees, delaunator, nanoflann).  See `tests/oracle_spatial.rs`.
//!
//! It must stay in lock-step with the C++ file: every random number is drawn in the same
//! order.  The C++ function names are given in the comments.

use std::io::Write;

use kicad_geom::geometry::rtree::{BulkEntry, DynamicRTree, PackedRTree, PackedRTreeBuilder};
use kicad_geom::stdsort::{
    PriorityQueue, std_make_heap, std_nth_element, std_partial_sort, std_sort, std_sort_heap,
};
use kicad_geom::thirdparty::delaunator::{Delaunator, DelaunatorError, INVALID_INDEX};
use kicad_geom::thirdparty::nanoflann::{KdTreeSingleIndexAdaptor, SearchParameters};

/// `RNG`: splitmix64.
struct Rng {
    s: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Rng { s: seed }
    }

    fn next(&mut self) -> u64 {
        self.s = self.s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// inclusive range
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo.wrapping_add((self.next() % ((hi - lo + 1) as u64)) as i64)
    }
}

/// `HASHER`: FNV-1a over the little-endian bytes of 64-bit words.
#[derive(Clone, Copy)]
struct Hasher {
    h: u64,
}

impl Hasher {
    fn new() -> Self {
        Hasher {
            h: 0xcbf29ce484222325,
        }
    }

    fn u(&mut self, v: u64) {
        for i in 0..8 {
            self.h ^= (v >> (8 * i)) & 0xff;
            self.h = self.h.wrapping_mul(0x100000001b3);
        }
    }

    fn i(&mut self, v: i64) {
        self.u(v as u64);
    }

    fn d(&mut self, x: f64) {
        self.u(x.to_bits());
    }
}

/// The collected results: `( section name, number of cases, hash )` per section.
pub struct Results {
    pub sections: Vec<(String, u64, u64)>,
    pub dump: Option<Box<dyn Write>>,
}

/// `SECTION`
struct Section<'a> {
    name: &'static str,
    count: u64,
    h: Hasher,
    results: &'a mut Results,
}

impl<'a> Section<'a> {
    fn new(name: &'static str, results: &'a mut Results) -> Self {
        Section {
            name,
            count: 0,
            h: Hasher::new(),
            results,
        }
    }

    fn add(&mut self, case: &Hasher) {
        if let Some(dump) = self.results.dump.as_mut() {
            writeln!(dump, "{} {} {:016x}", self.name, self.count, case.h).unwrap();
        }

        self.count += 1;
        self.h.u(case.h);
    }

    fn finish(self) {
        self.results
            .sections
            .push((self.name.to_string(), self.count, self.h.h));
    }
}

// ---------------------------------------------------------------------------------------
// std::sort, std::partial_sort, std::nth_element, heaps
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Tagged {
    key: i64,
    tag: i64,
}

/// `genSortArray`
fn gen_sort_array(rng: &mut Rng, pattern: i32, n: i64) -> Vec<Tagged> {
    let keys: i64 = match pattern {
        0 => 1,
        1 => 2,
        2 => 3 + rng.range(0, 10),
        3 => 1 + n / 4,
        4 => 1 + n,
        _ => 1000000000,
    };

    (0..n)
        .map(|i| {
            let key = match pattern {
                6 => i / 3,
                7 => (n - i) / 2,
                8 => i.min(n - i),
                9 => i % 17,
                10 => {
                    if i % 2 != 0 {
                        i
                    } else {
                        n - i
                    }
                }
                _ => (rng.next() % keys as u64) as i64,
            };

            Tagged { key, tag: i }
        })
        .collect()
}

/// `hashTags`
fn hash_tags(h: &mut Hasher, v: &[Tagged]) {
    h.u(v.len() as u64);

    for t in v {
        h.i(t.key);
        h.i(t.tag);
    }
}

/// `sortLength`
fn sort_length(rng: &mut Rng, case: i64) -> i64 {
    const SPECIAL: [i64; 14] = [0, 1, 2, 3, 15, 16, 17, 18, 31, 32, 33, 34, 64, 65];

    if case < 14 * 4 {
        return SPECIAL[(case % 14) as usize];
    }

    let sel = rng.range(0, 9);

    if sel < 5 {
        rng.range(0, 100)
    } else if sel < 9 {
        rng.range(0, 2000)
    } else {
        rng.range(2000, 6000)
    }
}

fn less(a: &Tagged, b: &Tagged) -> bool {
    a.key < b.key
}

/// `sectionSort`
fn section_sort(results: &mut Results) {
    let mut sec = Section::new("std_sort", results);
    let mut rng = Rng::new(101);

    for c in 0..3000 {
        let n = sort_length(&mut rng, c);
        let pattern = (c % 11) as i32;
        let mut v = gen_sort_array(&mut rng, pattern, n);

        std_sort(&mut v, less);

        let mut h = Hasher::new();
        hash_tags(&mut h, &v);
        sec.add(&h);
    }

    // McIlroy's antiquicksort adversary
    for c in 0..40i64 {
        let n = 100 + c * 250;
        let gas = n;
        let mut val = vec![gas; n as usize];
        let mut nsolid = 0i64;
        let mut candidate = 0i64;
        let mut ids: Vec<i64> = (0..n).collect();

        std_sort(&mut ids, |&x, &y| {
            if val[x as usize] == gas && val[y as usize] == gas {
                if x == candidate {
                    val[x as usize] = nsolid;
                } else {
                    val[y as usize] = nsolid;
                }

                nsolid += 1;
            }

            if val[x as usize] == gas {
                candidate = x;
            } else if val[y as usize] == gas {
                candidate = y;
            }

            val[x as usize] < val[y as usize]
        });

        let mut v: Vec<Tagged> = (0..n)
            .map(|i| Tagged {
                key: val[i as usize] / 2,
                tag: i,
            })
            .collect();

        std_sort(&mut v, less);

        let mut h = Hasher::new();
        h.u(n as u64);

        for &id in &ids {
            h.i(id);
        }

        hash_tags(&mut h, &v);
        sec.add(&h);
    }

    sec.finish();
}

/// `sectionPartialSort`
fn section_partial_sort(results: &mut Results) {
    let mut sec = Section::new("std_partial_sort_nth_element", results);
    let mut rng = Rng::new(202);

    for c in 0..1500 {
        let n = sort_length(&mut rng, c);
        let pattern = (c % 11) as i32;
        let mut v = gen_sort_array(&mut rng, pattern, n);
        let mut w = v.clone();
        let middle = rng.range(0, n);
        let nth = rng.range(0, n);

        std_partial_sort(&mut v, middle as usize, less);

        if nth < n {
            std_nth_element(&mut w, nth as usize, less);
        }

        let mut h = Hasher::new();
        h.i(middle);
        h.i(nth);
        hash_tags(&mut h, &v);
        hash_tags(&mut h, &w);
        sec.add(&h);
    }

    sec.finish();
}

/// `sectionHeap`
fn section_heap(results: &mut Results) {
    let mut sec = Section::new("std_heap_priority_queue", results);
    let mut rng = Rng::new(303);

    for c in 0..1000i32 {
        let n = rng.range(0, 300);
        let pattern = c % 11;
        let v = gen_sort_array(&mut rng, pattern, n);

        let mut h = Hasher::new();

        let mut heap = v.clone();
        std_make_heap(&mut heap, less);
        hash_tags(&mut h, &heap);

        std_sort_heap(&mut heap, less);
        hash_tags(&mut h, &heap);

        let mut pq = PriorityQueue::new(|a: &Tagged, b: &Tagged| a.key > b.key);
        let ops = rng.range(0, 600);
        let mut next = 0;

        for _ in 0..ops {
            let sel = rng.range(0, 2);

            if sel < 2 && next < n {
                pq.push(v[next as usize]);
                next += 1;
            } else if let Some(top) = pq.pop() {
                h.i(top.key);
                h.i(top.tag);
            }
        }

        while let Some(top) = pq.pop() {
            h.i(top.tag);
        }

        sec.add(&h);
    }

    sec.finish();
}

// ---------------------------------------------------------------------------------------
// r-trees
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Bx<const D: usize> {
    min: [i32; D],
    max: [i32; D],
}

/// `genBox`
fn gen_box<const D: usize>(rng: &mut Rng, kind: i32, so_far: &[Bx<D>]) -> Bx<D> {
    let mut b = Bx {
        min: [0; D],
        max: [0; D],
    };
    let mut first = 0;

    if D == 3 {
        let l0 = rng.range(0, 31);
        let sel = rng.range(0, 3);
        let l1 = if sel == 0 { rng.range(l0, 31) } else { l0 };
        b.min[0] = l0 as i32;
        b.max[0] = l1 as i32;
        first = 1;
    }

    for d in first..D {
        let lo: i64;
        let size: i64;

        match kind {
            0 => {
                lo = rng.range(-150000000, 150000000);
                size = rng.range(0, 5000000);
            }
            1 => {
                let centre = rng.range(0, 4) * 25000000;
                let half = rng.range(0, 3) * 100000;
                lo = centre - half;
                size = 2 * half;
            }
            2 => {
                let cell = rng.range(0, 40);
                let half = rng.range(1, 2) * 300000;
                lo = cell * 1270000 - half;
                size = 2 * half;
            }
            3 => {
                lo = rng.range(-1000, 1000);
                size = 0;
            }
            4 => {
                lo = rng.range(i32::MIN as i64, i32::MAX as i64);
                size = rng.range(0, i32::MAX as i64 - lo);
            }
            _ => {
                lo = rng.range(-20, 20);
                size = rng.range(0, 10);
            }
        }

        b.min[d] = lo as i32;
        b.max[d] = (lo + size) as i32;
    }

    let dup = rng.range(0, 9);

    if dup == 0 && !so_far.is_empty() {
        let idx = rng.range(0, so_far.len() as i64 - 1);
        b = so_far[idx as usize];
    }

    b
}

/// `genQuery`
fn gen_query<const D: usize>(rng: &mut Rng, items: &[Bx<D>]) -> Bx<D> {
    let mut q = Bx {
        min: [0; D],
        max: [0; D],
    };
    let sel = rng.range(0, 4);

    if sel == 0 && !items.is_empty() {
        let idx = rng.range(0, items.len() as i64 - 1);
        let grow = rng.range(0, 1000000);
        q = items[idx as usize];

        for d in 0..D {
            q.min[d] = (i32::MIN as i64).max(q.min[d] as i64 - grow) as i32;
            q.max[d] = (i32::MAX as i64).min(q.max[d] as i64 + grow) as i32;
        }
    } else if sel == 1 {
        for d in 0..D {
            q.min[d] = i32::MIN;
            q.max[d] = i32::MAX;
        }
    } else {
        for d in 0..D {
            let a = if sel == 2 {
                rng.range(-20, 20)
            } else {
                rng.range(-160000000, 160000000)
            };
            let s = if sel == 4 {
                rng.range(0, 100000000)
            } else {
                rng.range(0, 30)
            };
            q.min[d] = a as i32;
            q.max[d] = (i32::MAX as i64).min(a + s) as i32;
        }
    }

    q
}

/// `genBoxes`
fn gen_boxes<const D: usize>(rng: &mut Rng, case: i64) -> Vec<Bx<D>> {
    const SPECIAL: [i64; 12] = [0, 1, 2, 15, 16, 17, 31, 32, 33, 255, 256, 257];

    let n = if case < 12 {
        SPECIAL[case as usize]
    } else {
        let sel = rng.range(0, 3);

        if sel == 0 {
            rng.range(0, 40)
        } else {
            rng.range(0, 1500)
        }
    };

    let mixed = rng.range(0, 1);
    let kind = (case % 6) as i32;

    let mut items: Vec<Bx<D>> = Vec::new();

    for _ in 0..n {
        let mut k = kind;

        if mixed != 0 {
            k = rng.range(0, 5) as i32;
        }

        let b = gen_box::<D>(rng, k, &items);
        items.push(b);
    }

    items
}

/// `hashQueries`: `search` is the tree's `Search`.
fn hash_queries<const D: usize>(
    h: &mut Hasher,
    rng: &mut Rng,
    search: &dyn Fn(&[i32; D], &[i32; D], &mut dyn FnMut(&i32) -> bool) -> i32,
    items: &[Bx<D>],
    count: i64,
) {
    for _ in 0..count {
        let q = gen_query::<D>(rng, items);
        let limit = if rng.range(0, 3) == 0 {
            rng.range(1, 20)
        } else {
            -1
        };
        let mut seen = 0i64;

        let found = search(&q.min, &q.max, &mut |&id: &i32| {
            h.i(id as i64);
            seen += 1;
            limit < 0 || seen < limit
        });

        h.i(found as i64);
    }
}

/// `sectionPacked`
fn section_packed<const D: usize>(
    results: &mut Results,
    name: &'static str,
    seed: u64,
    cases: i64,
) {
    let mut sec = Section::new(name, results);
    let mut rng = Rng::new(seed);

    for c in 0..cases {
        let items = gen_boxes::<D>(&mut rng, c);

        let mut builder = PackedRTreeBuilder::<i32, D, 16>::new();
        builder.reserve(items.len());

        for (i, b) in items.iter().enumerate() {
            builder.add(&b.min, &b.max, i as i32);
        }

        let tree: PackedRTree<i32, D, 16> = builder.build();

        let mut h = Hasher::new();
        h.u(tree.len() as u64);
        h.u(tree.is_empty() as u64);

        for &id in &tree {
            h.i(id as i64);
        }

        let search =
            |mn: &[i32; D], mx: &[i32; D], v: &mut dyn FnMut(&i32) -> bool| tree.search(mn, mx, v);
        hash_queries::<D>(&mut h, &mut rng, &search, &items, 25);
        sec.add(&h);
    }

    sec.finish();
}

/// `hashDynamicState`
fn hash_dynamic_state<const D: usize>(
    h: &mut Hasher,
    rng: &mut Rng,
    tree: &DynamicRTree<i32, D, 16>,
    items: &[Bx<D>],
) {
    h.u(tree.len() as u64);

    for &id in tree.iter() {
        h.i(id as i64);
    }

    let search =
        |mn: &[i32; D], mx: &[i32; D], v: &mut dyn FnMut(&i32) -> bool| tree.search(mn, mx, v);
    hash_queries::<D>(h, rng, &search, items, 12);

    let q = gen_query::<D>(rng, items);

    for &id in tree.overlapping(&q.min, &q.max) {
        h.i(id as i64);
    }

    for _ in 0..2 {
        let mut pt = [0i32; D];
        let kk = rng.range(0, 12);

        for p in pt.iter_mut() {
            *p = rng.range(-150000000, 150000000) as i32;
        }

        for (dist, &id) in tree.nearest_neighbors(&pt, kk as i32) {
            h.i(dist);
            h.i(id as i64);
        }
    }
}

/// `sectionDynamic`
fn section_dynamic<const D: usize>(
    results: &mut Results,
    name: &'static str,
    seed: u64,
    cases: i64,
) {
    let mut sec = Section::new(name, results);
    let mut rng = Rng::new(seed);

    for c in 0..cases {
        let items = gen_boxes::<D>(&mut rng, c);
        let mut h = Hasher::new();

        // 1. Insert one by one
        let mut tree = DynamicRTree::<i32, D, 16>::new();

        for (i, b) in items.iter().enumerate() {
            tree.insert(&b.min, &b.max, i as i32);
        }

        hash_dynamic_state::<D>(&mut h, &mut rng, &tree, &items);

        // 2. Remove
        for (i, b) in items.iter().enumerate() {
            if i % 2 == 1 {
                h.u(tree.remove(&b.min, &b.max, &(i as i32)) as u64);
            } else if i % 4 == 0 {
                let wrong = gen_query::<D>(&mut rng, &items);
                h.u(tree.remove(&wrong.min, &wrong.max, &(i as i32)) as u64);
            }
        }

        let zero = [0i32; D];
        h.u(tree.remove(&zero, &zero, &-1) as u64);

        hash_dynamic_state::<D>(&mut h, &mut rng, &tree, &items);

        // 3. Insert again
        for (i, b) in items.iter().enumerate().step_by(2) {
            tree.insert(&b.min, &b.max, (i + 100000) as i32);
        }

        hash_dynamic_state::<D>(&mut h, &mut rng, &tree, &items);

        // 4. Bulk load, then insert and remove on top
        let entries: Vec<BulkEntry<i32, D>> = items
            .iter()
            .enumerate()
            .map(|(i, b)| BulkEntry {
                min: b.min,
                max: b.max,
                data: i as i32,
            })
            .collect();

        tree.bulk_load(entries);
        hash_dynamic_state::<D>(&mut h, &mut rng, &tree, &items);

        for (i, b) in items.iter().enumerate().step_by(3) {
            h.u(tree.remove(&b.min, &b.max, &(i as i32)) as u64);
        }

        for (i, b) in items.iter().enumerate().step_by(5) {
            tree.insert(&b.min, &b.max, (i + 200000) as i32);
        }

        hash_dynamic_state::<D>(&mut h, &mut rng, &tree, &items);

        tree.remove_all();
        h.u(tree.len() as u64);
        h.u(tree.is_empty() as u64);

        sec.add(&h);
    }

    sec.finish();
}

// ---------------------------------------------------------------------------------------
// delaunator
// ---------------------------------------------------------------------------------------

/// `std::llround` for the in-range values used here
fn llround(x: f64) -> i64 {
    x.round() as i64
}

/// `genPoints`
fn gen_points(rng: &mut Rng, case: i64) -> Vec<f64> {
    let mut c: Vec<f64> = Vec::new();
    let kind = case % 9;
    let mut n = rng.range(3, 12);
    let sel = rng.range(0, 3);

    if sel == 1 {
        n = rng.range(3, 300);
    } else if sel == 2 {
        n = rng.range(3, 1500);
    }

    let mut push = |x: i64, y: i64| {
        c.push(x as f64);
        c.push(y as f64);
    };

    match kind {
        0 => {
            for _ in 0..n {
                let x = rng.range(-150000000, 150000000);
                let y = rng.range(-150000000, 150000000);
                push(x, y);
            }
        }
        1 => {
            for _ in 0..n {
                let x = rng.range(0, 6);
                let y = rng.range(0, 6);
                push(x, y);
            }
        }
        2 => {
            for _ in 0..n {
                let x = rng.range(0, 30);
                let y = rng.range(0, 30);
                push(x * 1270000, y * 1270000);
            }
        }
        3 => {
            let w = rng.range(2, 40);
            let hgt = rng.range(2, 40);

            for i in 0..w {
                for j in 0..hgt {
                    push(i * 2540000, j * 1270000);
                }
            }
        }
        4 => {
            for _ in 0..n {
                let x = rng.range(0, 20);
                let y = rng.range(0, 20);
                let jx = rng.range(-2, 2);
                let jy = rng.range(-2, 2);
                push(x * 1000000 + jx, y * 1000000 + jy);
            }
        }
        5 => {
            let r = rng.range(1000, 50000000) as f64;

            for i in 0..n {
                let t = 2.0 * i as f64 / n as f64 - 1.0;
                let x = r * (1.0 - t * t) / (1.0 + t * t);
                let y = r * (2.0 * t) / (1.0 + t * t);
                push(llround(x), llround(y));
                push(llround(-x), llround(-y));
            }
        }
        6 => {
            let dy = rng.range(0, 3);

            for _ in 0..n {
                let x = rng.range(0, 300000000);
                let y = rng.range(0, dy);
                push(x, y);
            }
        }
        7 => {
            for _ in 0..n {
                let row = rng.range(0, 1);
                let pos = rng.range(0, 60);
                push(pos * 500000, row * 7620000);
            }
        }
        _ => {
            for _ in 0..n {
                let t = rng.range(-1000, 1000);
                push(t * 3, t * 7);
            }
        }
    }

    c
}

/// `hashSizeVec`
fn hash_size_vec(h: &mut Hasher, v: &[usize]) {
    h.u(v.len() as u64);

    for &x in v {
        h.i(if x == INVALID_INDEX { -1 } else { x as i64 });
    }
}

/// `sectionDelaunator`
fn section_delaunator(results: &mut Results) {
    let mut sec = Section::new("delaunator", results);
    let mut rng = Rng::new(404);

    for c in 0..2500 {
        let coords = gen_points(&mut rng, c);
        let mut h = Hasher::new();

        h.u(coords.len() as u64);

        let all_same = coords
            .chunks_exact(2)
            .all(|p| p[0] == coords[0] && p[1] == coords[1]);

        if all_same {
            h.i(3);
            sec.add(&h);
            continue;
        }

        match Delaunator::new(&coords) {
            Ok(d) => {
                h.i(0);
                hash_size_vec(&mut h, &d.triangles);
                hash_size_vec(&mut h, &d.halfedges);
                hash_size_vec(&mut h, &d.hull_prev);
                hash_size_vec(&mut h, &d.hull_next);
                hash_size_vec(&mut h, &d.hull_tri);
                h.i(d.hull_start as i64);
                h.d(d.get_hull_area());
                h.d(d.get_triangle_area());
            }
            Err(DelaunatorError::NotTriangulation) => h.i(1),
            Err(_) => h.i(2),
        }

        sec.add(&h);
    }

    sec.finish();
}

// ---------------------------------------------------------------------------------------
// nanoflann
// ---------------------------------------------------------------------------------------

/// `genEndpoints`
fn gen_endpoints(rng: &mut Rng, case: i64) -> Vec<(i32, i32)> {
    let mut pts: Vec<(i32, i32)> = Vec::new();
    let kind = case % 4;
    let shapes = if rng.range(0, 3) == 0 {
        rng.range(0, 8)
    } else {
        rng.range(1, 400)
    };

    let mut x = rng.range(-100000000, 100000000);
    let mut y = rng.range(-100000000, 100000000);

    for _ in 0..shapes {
        if kind == 0 {
            let gap = if rng.range(0, 4) == 0 {
                rng.range(-3, 3)
            } else {
                0
            };
            let nx = x + rng.range(-5000000, 5000000);
            let ny = y + rng.range(-5000000, 5000000);
            pts.push(((x + gap) as i32, y as i32));
            pts.push((nx as i32, ny as i32));
            x = nx;
            y = ny;
        } else if kind == 1 {
            let ax = rng.range(0, 6);
            let ay = rng.range(0, 6);
            let bx = rng.range(0, 6);
            let by = rng.range(0, 6);
            pts.push(((ax * 1000) as i32, (ay * 1000) as i32));
            pts.push(((bx * 1000) as i32, (by * 1000) as i32));
        } else if kind == 2 {
            let a = rng.range(-1000000, 1000000);
            let b = rng.range(-1000000, 1000000);
            pts.push((a as i32, 0));
            pts.push((b as i32, 0));
        } else {
            let ax = rng.range(-100000000, 100000000);
            let ay = rng.range(-100000000, 100000000);
            let bx = rng.range(-100000000, 100000000);
            let by = rng.range(-100000000, 100000000);
            pts.push((ax as i32, ay as i32));
            pts.push((bx as i32, by as i32));
        }
    }

    pts
}

/// `sectionNanoflann`
fn section_nanoflann(results: &mut Results) {
    let mut sec = Section::new("nanoflann", results);
    let mut rng = Rng::new(505);

    for c in 0..1000 {
        let pts = gen_endpoints(&mut rng, c);
        let dataset: Vec<[f64; 2]> = pts.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
        let tree = KdTreeSingleIndexAdaptor::<_, 2>::new(dataset);

        let mut h = Hasher::new();
        h.u(pts.len() as u64);

        for _ in 0..24 {
            let sel = rng.range(0, 2);

            let query: [f64; 2] = if sel == 0 && !pts.is_empty() {
                let idx = rng.range(0, pts.len() as i64 - 1);
                let dx = rng.range(-2, 2);
                [
                    pts[idx as usize].0 as f64 + dx as f64,
                    pts[idx as usize].1 as f64,
                ]
            } else {
                let qx = rng.range(-100000000, 100000000);
                let qy = rng.range(-100000000, 100000000);
                [qx as f64, qy as f64]
            };

            let rsel = rng.range(0, 3);
            let eps = if rsel == 0 {
                1000.0
            } else if rsel == 1 {
                2.0
            } else {
                rng.range(0, 20000000) as f64
            };
            let radius_sq = eps * eps;

            let matches = tree.radius_search(&query, radius_sq);
            h.u(matches.len() as u64);

            for m in &matches {
                h.u(m.first as u64);
                h.d(m.second);
            }

            let unsorted = SearchParameters {
                eps: 0.0,
                sorted: false,
            };

            for m in &tree.radius_search_with_params(&query, radius_sq, &unsorted) {
                h.u(m.first as u64);
            }

            let (indices, dists) = tree.knn_search(&query, 8);
            h.u(indices.len() as u64);

            for (i, d) in indices.iter().zip(dists.iter()) {
                h.u(*i as u64);
                h.d(*d);
            }
        }

        sec.add(&h);
    }

    sec.finish();
}

/// Runs all sections in the order of `spatial_oracle.cpp`'s `main`.
pub fn run_all(results: &mut Results) {
    section_sort(results);
    section_partial_sort(results);
    section_heap(results);
    section_packed::<2>(results, "packed_rtree_2d", 606, 400);
    section_packed::<3>(results, "packed_rtree_3d", 607, 150);
    section_dynamic::<2>(results, "dynamic_rtree_2d", 708, 300);
    section_dynamic::<3>(results, "dynamic_rtree_3d", 709, 150);
    section_delaunator(results);
    section_nanoflann(results);
}
