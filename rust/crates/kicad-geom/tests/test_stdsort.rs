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

//! Checks that [`kicad_geom::stdsort::std_sort`] and the private libstdc++ `std::sort` copy in
//! the KiCad fidelity patch of the vendored Clipper2 (`gnu_sort.rs` in
//! `docs/research/artifacts/polygon-clipping/clipper2-rs-0.3.0-kicad-fidelity.patch`) produce
//! the same permutation, including the order of equal elements.  The function below is a
//! verbatim copy of that file's code (only `pub(crate)` dropped); keep it in sync if the patch
//! changes.  The comparison against g++ itself is in `tests/oracle_spatial.rs`.

mod gnu_sort {
    const THRESHOLD: isize = 16;

    /// Sorts `v` exactly like libstdc++ `std::sort( first, last, comp )` with `less` as `comp`.
    pub fn gnu_sort<T: Copy>(v: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
        let n = v.len() as isize;
        if n == 0 {
            return;
        }
        // std::__lg( n ) * 2
        let depth_limit = ((usize::BITS - 1 - (n as usize).leading_zeros()) as isize) * 2;
        introsort_loop(v, 0, n, depth_limit, &mut less);
        final_insertion_sort(v, 0, n, &mut less);
    }

    fn introsort_loop<T: Copy>(
        v: &mut [T],
        first: isize,
        mut last: isize,
        mut depth_limit: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        while last - first > THRESHOLD {
            if depth_limit == 0 {
                // std::__partial_sort( first, last, last ): heap select + sort heap
                make_heap(v, first, last, less);
                sort_heap(v, first, last, less);
                return;
            }
            depth_limit -= 1;
            let cut = unguarded_partition_pivot(v, first, last, less);
            introsort_loop(v, cut, last, depth_limit, less);
            last = cut;
        }
    }

    fn unguarded_partition_pivot<T: Copy>(
        v: &mut [T],
        first: isize,
        last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) -> isize {
        let mid = first + (last - first) / 2;
        move_median_to_first(v, first, first + 1, mid, last - 1, less);
        unguarded_partition(v, first + 1, last, first, less)
    }

    fn move_median_to_first<T: Copy>(
        v: &mut [T],
        result: isize,
        a: isize,
        b: isize,
        c: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        let (r, a, b, c) = (result as usize, a as usize, b as usize, c as usize);
        if less(&v[a], &v[b]) {
            if less(&v[b], &v[c]) {
                v.swap(r, b);
            } else if less(&v[a], &v[c]) {
                v.swap(r, c);
            } else {
                v.swap(r, a);
            }
        } else if less(&v[a], &v[c]) {
            v.swap(r, a);
        } else if less(&v[b], &v[c]) {
            v.swap(r, c);
        } else {
            v.swap(r, b);
        }
    }

    fn unguarded_partition<T: Copy>(
        v: &mut [T],
        mut first: isize,
        mut last: isize,
        pivot: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) -> isize {
        loop {
            while less(&v[first as usize], &v[pivot as usize]) {
                first += 1;
            }
            last -= 1;
            while less(&v[pivot as usize], &v[last as usize]) {
                last -= 1;
            }
            if !(first < last) {
                return first;
            }
            v.swap(first as usize, last as usize);
            first += 1;
        }
    }

    fn unguarded_linear_insert<T: Copy>(
        v: &mut [T],
        mut last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        let val = v[last as usize];
        let mut next = last - 1;
        while less(&val, &v[next as usize]) {
            v[last as usize] = v[next as usize];
            last = next;
            next -= 1;
        }
        v[last as usize] = val;
    }

    fn insertion_sort<T: Copy>(
        v: &mut [T],
        first: isize,
        last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        if first == last {
            return;
        }
        let mut i = first + 1;
        while i != last {
            if less(&v[i as usize], &v[first as usize]) {
                let val = v[i as usize];
                v.copy_within(first as usize..i as usize, first as usize + 1);
                v[first as usize] = val;
            } else {
                unguarded_linear_insert(v, i, less);
            }
            i += 1;
        }
    }

    fn final_insertion_sort<T: Copy>(
        v: &mut [T],
        first: isize,
        last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        if last - first > THRESHOLD {
            insertion_sort(v, first, first + THRESHOLD, less);
            // std::__unguarded_insertion_sort
            let mut i = first + THRESHOLD;
            while i != last {
                unguarded_linear_insert(v, i, less);
                i += 1;
            }
        } else {
            insertion_sort(v, first, last, less);
        }
    }

    /// `std::__adjust_heap` followed by its inlined `std::__push_heap`.
    fn adjust_heap<T: Copy>(
        v: &mut [T],
        first: isize,
        mut hole: isize,
        len: isize,
        value: T,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        let top = hole;
        let mut second = hole;
        while second < (len - 1) / 2 {
            second = 2 * (second + 1);
            if less(
                &v[(first + second) as usize],
                &v[(first + second - 1) as usize],
            ) {
                second -= 1;
            }
            v[(first + hole) as usize] = v[(first + second) as usize];
            hole = second;
        }
        if (len & 1) == 0 && second == (len - 2) / 2 {
            second = 2 * (second + 1);
            v[(first + hole) as usize] = v[(first + second - 1) as usize];
            hole = second - 1;
        }
        let mut parent = (hole - 1) / 2;
        while hole > top && less(&v[(first + parent) as usize], &value) {
            v[(first + hole) as usize] = v[(first + parent) as usize];
            hole = parent;
            parent = (hole - 1) / 2;
        }
        v[(first + hole) as usize] = value;
    }

    fn make_heap<T: Copy>(
        v: &mut [T],
        first: isize,
        last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        if last - first < 2 {
            return;
        }
        let len = last - first;
        let mut parent = (len - 2) / 2;
        loop {
            let value = v[(first + parent) as usize];
            adjust_heap(v, first, parent, len, value, less);
            if parent == 0 {
                return;
            }
            parent -= 1;
        }
    }

    fn sort_heap<T: Copy>(
        v: &mut [T],
        first: isize,
        mut last: isize,
        less: &mut impl FnMut(&T, &T) -> bool,
    ) {
        while last - first > 1 {
            last -= 1;
            // std::__pop_heap( first, last, last )
            let value = v[last as usize];
            v[last as usize] = v[first as usize];
            adjust_heap(v, first, 0, last - first, value, less);
        }
    }
}

use kicad_geom::stdsort::std_sort;

/// splitmix64
fn next(s: &mut u64) -> u64 {
    *s = s.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[test]
fn std_sort_agrees_with_clipper_gnu_sort() {
    let mut s = 77u64;

    for round in 0..4000u64 {
        let n = if round % 10 == 9 {
            next(&mut s) % 3000
        } else {
            next(&mut s) % 300
        } as usize;
        let keys = [1u64, 2, 3, 7, 50, 1000, u64::MAX][(round % 7) as usize];
        let v: Vec<(u64, usize)> = (0..n)
            .map(|i| {
                let k = match round % 5 {
                    0 => next(&mut s) % keys,
                    1 => i as u64 / 3,
                    2 => (n - i) as u64 / 2,
                    3 => (i as u64).min((n - i) as u64) % keys.max(1),
                    _ => (i % 17) as u64,
                };
                (k, i)
            })
            .collect();

        let mut a = v.clone();
        let mut b = v;
        std_sort(&mut a, |x, y| x.0 < y.0);
        gnu_sort::gnu_sort(&mut b, |x, y| x.0 < y.0);
        assert_eq!(a, b, "round {round}, n {n}");
    }
}
