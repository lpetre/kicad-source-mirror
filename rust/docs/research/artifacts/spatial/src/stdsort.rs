//! Research prototype: a statement-by-statement port of libstdc++'s `std::sort`
//! (GCC 13, `bits/stl_algo.h` + `bits/stl_heap.h`): introsort with median-of-3 pivot,
//! `_S_threshold = 16`, heapsort fallback at depth `2 * floor(log2(n))`, and a final
//! insertion sort.  `std::sort` is not stable, so the order of equivalent elements is
//! implementation defined; KiCad code whose result depends on that order (Hilbert-packed
//! R-tree, delaunator point order, R*-tree split) can only be reproduced by using the very
//! same algorithm.
//!
//! `less(a, b)` must be the C++ comparator (`comp(a, b)`).

/// Port of `std::__sort` (what `std::sort(first, last, comp)` calls).
pub fn std_sort<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    let n = v.len();
    if n != 0 {
        introsort_loop(v, 0, n, lg(n) * 2, &mut less);
        final_insertion_sort(v, 0, n, &mut less);
    }
}

const S_THRESHOLD: usize = 16;

/// Test instrumentation: how often the heapsort fallback of introsort was taken.
pub static HEAPSORT_FALLBACKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Port of `std::partial_sort(first, first + middle, last, comp)`.
pub fn std_partial_sort<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], middle: usize, mut less: F) {
    let n = v.len();
    partial_sort(v, 0, middle, n, &mut less);
}

/// Port of `std::__lg` (floor(log2(n)) for n > 0).
fn lg(n: usize) -> usize {
    (usize::BITS - 1 - n.leading_zeros()) as usize
}

/// Port of `std::__introsort_loop`.
fn introsort_loop<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth_limit: usize,
    less: &mut F,
) {
    while last - first > S_THRESHOLD {
        if depth_limit == 0 {
            HEAPSORT_FALLBACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            partial_sort(v, first, last, last, less);
            return;
        }
        depth_limit -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth_limit, less);
        last = cut;
    }
}

/// Port of `std::__unguarded_partition_pivot`.
fn unguarded_partition_pivot<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut F,
) -> usize {
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, less);
    unguarded_partition(v, first + 1, last, first, less)
}

/// Port of `std::__move_median_to_first`.
fn move_median_to_first<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    less: &mut F,
) {
    if less(&v[a], &v[b]) {
        if less(&v[b], &v[c]) {
            v.swap(result, b);
        } else if less(&v[a], &v[c]) {
            v.swap(result, c);
        } else {
            v.swap(result, a);
        }
    } else if less(&v[a], &v[c]) {
        v.swap(result, a);
    } else if less(&v[b], &v[c]) {
        v.swap(result, c);
    } else {
        v.swap(result, b);
    }
}

/// Port of `std::__unguarded_partition`.
fn unguarded_partition<T, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: usize,
    less: &mut F,
) -> usize {
    loop {
        while less(&v[first], &v[pivot]) {
            first += 1;
        }
        last -= 1;
        while less(&v[pivot], &v[last]) {
            last -= 1;
        }
        if !(first < last) {
            return first;
        }
        v.swap(first, last);
        first += 1;
    }
}

/// Port of `std::__final_insertion_sort`.
fn final_insertion_sort<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut F,
) {
    if last - first > S_THRESHOLD {
        insertion_sort(v, first, first + S_THRESHOLD, less);
        unguarded_insertion_sort(v, first + S_THRESHOLD, last, less);
    } else {
        insertion_sort(v, first, last, less);
    }
}

/// Port of `std::__insertion_sort`.
fn insertion_sort<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut F,
) {
    if first == last {
        return;
    }
    let mut i = first + 1;
    while i != last {
        if less(&v[i], &v[first]) {
            let val = v[i].clone();
            // _GLIBCXX_MOVE_BACKWARD3( first, i, i + 1 )
            let mut k = i;
            while k > first {
                v[k] = v[k - 1].clone();
                k -= 1;
            }
            v[first] = val;
        } else {
            unguarded_linear_insert(v, i, less);
        }
        i += 1;
    }
}

/// Port of `std::__unguarded_insertion_sort`.
fn unguarded_insertion_sort<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut F,
) {
    let mut i = first;
    while i != last {
        unguarded_linear_insert(v, i, less);
        i += 1;
    }
}

/// Port of `std::__unguarded_linear_insert`.  "Unguarded": relies on an element that is
/// not greater than `val` existing to the left (guaranteed by the callers).
fn unguarded_linear_insert<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    mut last: usize,
    less: &mut F,
) {
    let val = v[last].clone();
    let mut next = last - 1;
    while less(&val, &v[next]) {
        v[last] = v[next].clone();
        last = next;
        if next == 0 {
            // C++ would read before `begin()` here (undefined behaviour); this can only happen
            // when the comparator is not a strict weak ordering (e.g. NaN keys).
            break;
        }
        next -= 1;
    }
    v[last] = val;
}

/// Port of `std::__partial_sort` (heapsort fallback of introsort; also `std::partial_sort`).
fn partial_sort<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &mut F,
) {
    heap_select(v, first, middle, last, less);
    sort_heap(v, first, middle, less);
}

/// Port of `std::__heap_select`.
fn heap_select<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &mut F,
) {
    make_heap(v, first, middle, less);
    let mut i = middle;
    while i < last {
        if less(&v[i], &v[first]) {
            pop_heap(v, first, middle, i, less);
        }
        i += 1;
    }
}

/// Port of `std::__make_heap`.
fn make_heap<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut F,
) {
    if last - first < 2 {
        return;
    }
    let len = last - first;
    let mut parent = (len - 2) / 2;
    loop {
        let value = v[first + parent].clone();
        adjust_heap(v, first, parent, len, value, less);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

/// Port of `std::__adjust_heap`.
///
/// C++ does this arithmetic in the signed `difference_type`; `len` can be 0 or 1 (e.g.
/// `std::partial_sort( first, first, last )`), where `(len - 1) / 2` and `(len - 2) / 2` rely on
/// signed truncating division, so the port uses `isize` as well.
fn adjust_heap<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    hole_index: usize,
    len: usize,
    value: T,
    less: &mut F,
) {
    let len = len as isize;
    let top_index = hole_index as isize;
    let mut hole_index = hole_index as isize;
    let mut second_child = hole_index;
    let at = |i: isize| first + i as usize;
    while second_child < (len - 1) / 2 {
        second_child = 2 * (second_child + 1);
        if less(&v[at(second_child)], &v[at(second_child - 1)]) {
            second_child -= 1;
        }
        v[at(hole_index)] = v[at(second_child)].clone();
        hole_index = second_child;
    }
    if (len & 1) == 0 && second_child == (len - 2) / 2 {
        second_child = 2 * (second_child + 1);
        v[at(hole_index)] = v[at(second_child - 1)].clone();
        hole_index = second_child - 1;
    }
    push_heap(v, first, hole_index as usize, top_index as usize, value, less);
}

/// Port of `std::__push_heap`.
fn push_heap<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut hole_index: usize,
    top_index: usize,
    value: T,
    less: &mut F,
) {
    // C++ computes `(holeIndex - 1) / 2` on a signed distance type; for holeIndex == 0 that is
    // (-1) / 2 == 0 and the loop condition `holeIndex > topIndex` is false anyway.
    let mut parent = if hole_index == 0 { 0 } else { (hole_index - 1) / 2 };
    while hole_index > top_index && less(&v[first + parent], &value) {
        v[first + hole_index] = v[first + parent].clone();
        hole_index = parent;
        parent = if hole_index == 0 { 0 } else { (hole_index - 1) / 2 };
    }
    v[first + hole_index] = value;
}

/// Port of `std::__pop_heap`.
fn pop_heap<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    last: usize,
    result: usize,
    less: &mut F,
) {
    let value = v[result].clone();
    v[result] = v[first].clone();
    adjust_heap(v, first, 0, last - first, value, less);
}

/// Port of `std::__sort_heap`.
fn sort_heap<T: Clone, F: FnMut(&T, &T) -> bool>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    less: &mut F,
) {
    while last - first > 1 {
        last -= 1;
        pop_heap(v, first, last, last, less);
    }
}
