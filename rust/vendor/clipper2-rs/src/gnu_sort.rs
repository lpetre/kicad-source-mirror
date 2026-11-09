//! `std::sort` as implemented by libstdc++ (introsort: median-of-three quicksort down to
//! partitions of 16 elements, heapsort when the recursion gets too deep, then one insertion
//! sort pass).  `std::sort` is not stable, and Clipper2 sorts its intersection list with it, so
//! the order of intersections at the same point - and with it the output vertex order - depends
//! on the standard library.  This reproduces the order a g++/libstdc++ build produces.
//!
//! KiCad port patch (not part of upstream clipper2-rs).

const THRESHOLD: isize = 16;

/// Sorts `v` exactly like libstdc++ `std::sort( first, last, comp )` with `less` as `comp`.
pub(crate) fn gnu_sort<T: Copy>(v: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
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

fn unguarded_linear_insert<T: Copy>(v: &mut [T], mut last: isize, less: &mut impl FnMut(&T, &T) -> bool) {
    let val = v[last as usize];
    let mut next = last - 1;
    while less(&val, &v[next as usize]) {
        v[last as usize] = v[next as usize];
        last = next;
        next -= 1;
    }
    v[last as usize] = val;
}

fn insertion_sort<T: Copy>(v: &mut [T], first: isize, last: isize, less: &mut impl FnMut(&T, &T) -> bool) {
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

fn final_insertion_sort<T: Copy>(v: &mut [T], first: isize, last: isize, less: &mut impl FnMut(&T, &T) -> bool) {
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
        if less(&v[(first + second) as usize], &v[(first + second - 1) as usize]) {
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

fn make_heap<T: Copy>(v: &mut [T], first: isize, last: isize, less: &mut impl FnMut(&T, &T) -> bool) {
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

fn sort_heap<T: Copy>(v: &mut [T], first: isize, mut last: isize, less: &mut impl FnMut(&T, &T) -> bool) {
    while last - first > 1 {
        last -= 1;
        // std::__pop_heap( first, last, last )
        let value = v[last as usize];
        v[last as usize] = v[first as usize];
        adjust_heap(v, first, 0, last - first, value, less);
    }
}
