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

//! Ports of the unstable sorting algorithms of libstdc++ (GCC 13: `bits/stl_algo.h`,
//! `bits/stl_heap.h`, `bits/stl_queue.h`) and of the red-black tree insertion behind
//! `std::set` (`bits/stl_tree.h`, `src/c++98/tree.cc`), with exactly the same sequence of comparisons and
//! element moves, so that elements which compare equal end up in the same order as in KiCad
//! built with g++/libstdc++ (the reference platform, see `PORTING.md` section 9).
//!
//! `std::sort`, `std::partial_sort`, `std::nth_element` and the heap behind
//! `std::priority_queue` are not stable: where two distinct elements compare equal, their
//! final order is an implementation detail of the standard library.  KiCad results depend on
//! that order in several places (Hilbert-packed r-tree, R*-tree split, delaunator's point
//! order, nearest-neighbour queues, Clipper2's intersection list, ...), so the port must not
//! use `slice::sort_unstable*` there (and `slice::sort*` only for `std::stable_sort`).
//!
//! | C++ | Rust |
//! |---|---|
//! | `std::sort( first, last, comp )` | [`std_sort`]`( &mut v, less )` |
//! | `std::sort( first, last )` | [`std_sort_default`]`( &mut v )` |
//! | `std::partial_sort( first, middle, last, comp )` | [`std_partial_sort`]`( &mut v, middle, less )` |
//! | `std::nth_element( first, nth, last, comp )` | [`std_nth_element`]`( &mut v, nth, less )` |
//! | `std::make_heap` / `push_heap` / `pop_heap` / `sort_heap` | [`std_make_heap`], [`std_push_heap`], [`std_pop_heap`], [`std_sort_heap`] |
//! | `std::priority_queue<T, std::vector<T>, Compare>` | [`PriorityQueue`] |
//! | `std::unique( first, last, pred )` + `erase` | [`std_unique`]`( &mut vec, pred )` |
//! | `std::stable_sort` | `slice::sort_by` (both are stable; no port needed) |
//! | `std::set<T, Compare>( first, last, comp )` (iteration order) | [`std_set_order`]`( items, less )` |
//! | `std::set<T, Compare>` insertions | [`StdSet`] |
//!
//! `less( a, b )` is the C++ comparator `comp( a, b )` ("`a` goes before `b`") and must be a
//! strict weak ordering, as in C++.  With a comparator that is not one (for example one that
//! compares NaN keys, or `<=`), the C++ code has undefined behaviour (it reads outside the
//! range); the ports stay memory safe, never panic and terminate, but the resulting order is
//! then unspecified.
//!
//! The functions work on any `T` (no `Clone` needed): the C++ code moves elements through a
//! temporary, which is reproduced with swaps and rotations that have the same net effect.
//!
//! To sort only a part of a slice, like `std::sort( v.begin() + a, v.begin() + b, comp )`,
//! pass the subslice `&mut v[a..b]`.

/// `enum { _S_threshold = 16 };`
const S_THRESHOLD: usize = 16;

/// Port of `std::sort( first, last, comp )` (`std::__sort`): introsort with a median-of-three
/// pivot, insertion sort for ranges of at most 16 elements and a heap sort fallback when the
/// recursion depth exceeds `2 * floor( log2( n ) )`.
pub fn std_sort<T, F>(v: &mut [T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();

    if n != 0 {
        introsort_loop(v, 0, n, lg(n) * 2, &mut less);
        final_insertion_sort(v, 0, n, &mut less);
    }
}

/// Port of `std::sort( first, last )` (comparison with `operator<`).
pub fn std_sort_default<T: PartialOrd>(v: &mut [T]) {
    std_sort(v, |a, b| a < b);
}

/// Port of `std::partial_sort( first, first + middle, last, comp )`: afterwards the `middle`
/// smallest elements are sorted at the front; the order of the rest is the libstdc++ one.
///
/// `middle` is clamped to the length of the slice (C++: undefined behaviour beyond it).
pub fn std_partial_sort<T, F>(v: &mut [T], middle: usize, mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();
    partial_sort(v, 0, middle.min(n), n, &mut less);
}

/// Port of `std::nth_element( first, first + nth, last, comp )` (`std::__introselect`).
///
/// Does nothing if `nth >= v.len()` (C++: `nth == last` returns, beyond is undefined).
pub fn std_nth_element<T, F>(v: &mut [T], nth: usize, mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();

    if n == 0 || nth >= n {
        return;
    }

    introselect(v, 0, nth, n, lg(n) * 2, &mut less);
}

/// Port of `std::make_heap( first, last, comp )`.
pub fn std_make_heap<T, F>(v: &mut [T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();
    make_heap(v, 0, n, &mut less);
}

/// Port of `std::push_heap( first, last, comp )`: `v[..len - 1]` is a heap, the last element
/// is sifted up into it.
pub fn std_push_heap<T, F>(v: &mut [T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();

    if n != 0 {
        push_heap(v, 0, n - 1, 0, &mut less);
    }
}

/// Port of `std::pop_heap( first, last, comp )`: moves the top of the heap to the last
/// position and restores the heap property on `v[..len - 1]`.
pub fn std_pop_heap<T, F>(v: &mut [T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();

    if n > 1 {
        pop_heap(v, 0, n - 1, n - 1, &mut less);
    }
}

/// Port of `std::sort_heap( first, last, comp )`.
pub fn std_sort_heap<T, F>(v: &mut [T], mut less: F)
where
    F: FnMut(&T, &T) -> bool,
{
    let n = v.len();
    sort_heap(v, 0, n, &mut less);
}

/// Port of the idiom `v.erase( std::unique( v.begin(), v.end(), pred ), v.end() )`: removes
/// all but the first element of every run of consecutive elements for which
/// `pred( kept, candidate )` is true.  Returns the new length.
///
/// The result of `std::unique` is fully specified by the C++ standard; this helper exists
/// because `Vec::dedup_by` passes its arguments in the opposite order (candidate first), which
/// matters for predicates that are not symmetric.
pub fn std_unique<T, F>(v: &mut Vec<T>, mut pred: F) -> usize
where
    F: FnMut(&T, &T) -> bool,
{
    v.dedup_by(|candidate, kept| pred(kept, candidate));
    v.len()
}

/// Port of `std::priority_queue<T, std::vector<T>, Compare>`: a binary max-heap with respect
/// to `less` (the element for which nothing is "greater" is on top) maintained with the
/// libstdc++ `std::push_heap` / `std::pop_heap`, so elements that compare equal are popped in
/// the same order as in the C++ code.
///
/// For a min-queue pass the inverted comparison, exactly like the C++ code does with
/// `std::greater<>` or a custom `cmp`.
pub struct PriorityQueue<T, F>
where
    F: FnMut(&T, &T) -> bool,
{
    c: Vec<T>,
    comp: F,
}

impl<T, F> PriorityQueue<T, F>
where
    F: FnMut(&T, &T) -> bool,
{
    /// `std::priority_queue<...> pq( comp )`
    pub fn new(comp: F) -> Self {
        PriorityQueue {
            c: Vec::new(),
            comp,
        }
    }

    /// `priority_queue::push`
    pub fn push(&mut self, value: T) {
        self.c.push(value);
        let n = self.c.len();
        push_heap(&mut self.c, 0, n - 1, 0, &mut self.comp);
    }

    /// `priority_queue::top` (`None` if the queue is empty; undefined behaviour in C++).
    pub fn top(&self) -> Option<&T> {
        self.c.first()
    }

    /// `priority_queue::pop`, returning the removed top element (`None` if the queue is
    /// empty; undefined behaviour in C++).
    pub fn pop(&mut self) -> Option<T> {
        let n = self.c.len();

        if n > 1 {
            pop_heap(&mut self.c, 0, n - 1, n - 1, &mut self.comp);
        }

        self.c.pop()
    }

    /// `priority_queue::size`
    pub fn len(&self) -> usize {
        self.c.len()
    }

    /// `priority_queue::empty`
    pub fn is_empty(&self) -> bool {
        self.c.is_empty()
    }
}

// ---- std::set ------------------------------------------------------------------------------

/// The iteration order of a C++ `std::set<T, Compare>` built from a range
/// (`std::set( first, last, comp )`, also `set.insert( first, last )` on an empty set): the
/// items in the in-order walk of the libstdc++ red-black tree after inserting them one by one
/// with the hint `end()` (`_M_insert_range_unique`: `_M_insert_unique_( end(), item )`); an
/// item comparing equivalent to one already in the tree is dropped (the earlier one is kept).
///
/// `less( a, b )` is the C++ `comp( a, b )`.  The tree is reproduced exactly (see [`StdSet`])
/// because several KiCad comparators (`EDA_SHAPE::Compare` with its `EPSILON`, ...) are not
/// strict weak orderings: then the result depends on the shape of the tree and on the exact
/// sequence of comparisons, not only on the comparator.
pub fn std_set_order<T, I, F>(items: I, mut less: F) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T, &T) -> bool,
{
    let mut set = StdSet::new();

    for item in items {
        set.insert_hint_end(item, &mut less);
    }

    set.into_vec()
}

/// Null link of [`StdSet`].
const RB_NIL: usize = usize::MAX;

/// The header node index of [`StdSet`] (`_M_impl._M_header`).
const RB_HEADER: usize = 0;

/// A node of [`StdSet`] (`_Rb_tree_node_base` plus the value; the header has none).
struct RbNode<T> {
    value: Option<T>,
    parent: usize,
    left: usize,
    right: usize,
    red: bool,
}

/// Port of the insertion part of libstdc++'s `_Rb_tree` (GCC 13: `bits/stl_tree.h`,
/// `src/c++98/tree.cc`) behind `std::set<T, Compare>`, on an index arena, with exactly the
/// same comparisons and rebalancing.  The header's `parent` is the root, its `left` the
/// leftmost and its `right` the rightmost node, as in libstdc++.
///
/// The comparator is passed to each insertion (`less( a, b )` is the C++ `comp( a, b )`); it
/// must be the same for every call on one set, as it is for a C++ set.  Erasure is not
/// ported (no caller needs it).
pub struct StdSet<T> {
    nodes: Vec<RbNode<T>>,
}

impl<T> Default for StdSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> StdSet<T> {
    /// An empty set (`_M_reset()`: the header is red and links to itself as leftmost and
    /// rightmost).
    pub fn new() -> Self {
        StdSet {
            nodes: vec![RbNode {
                value: None,
                parent: RB_NIL,
                left: RB_HEADER,
                right: RB_HEADER,
                red: true,
            }],
        }
    }

    /// `set::size`
    pub fn len(&self) -> usize {
        self.nodes.len() - 1
    }

    /// `set::empty`
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn key(&self, n: usize) -> &T {
        self.nodes[n].value.as_ref().expect("value node")
    }

    fn root(&self) -> usize {
        self.nodes[RB_HEADER].parent
    }

    /// Port of `set::insert( v )` (`_M_insert_unique`): returns `false` (and drops `v`) when
    /// an equivalent key is present.
    pub fn insert<F>(&mut self, v: T, mut less: F) -> bool
    where
        F: FnMut(&T, &T) -> bool,
    {
        match self.get_insert_unique_pos(&v, &mut less) {
            Some((x, p)) => {
                self.insert_node(x, p, v, &mut less);
                true
            }
            None => false,
        }
    }

    /// Port of `set::insert( end(), v )` (`_M_insert_unique_( end(), v )` with
    /// `_M_get_insert_hint_unique_pos( end(), k )`), the insertion used by the range
    /// constructor: returns `false` (and drops `v`) when an equivalent key is present.
    pub fn insert_hint_end<F>(&mut self, v: T, mut less: F) -> bool
    where
        F: FnMut(&T, &T) -> bool,
    {
        let rightmost = self.nodes[RB_HEADER].right;

        let (x, p) = if self.len() > 0 && less(self.key(rightmost), &v) {
            (RB_NIL, rightmost)
        } else {
            match self.get_insert_unique_pos(&v, &mut less) {
                Some(pos) => pos,
                None => return false,
            }
        };

        self.insert_node(x, p, v, &mut less);
        true
    }

    /// `_M_insert_( x, p, v )`.
    fn insert_node<F>(&mut self, x: usize, p: usize, v: T, less: &mut F)
    where
        F: FnMut(&T, &T) -> bool,
    {
        let insert_left = x != RB_NIL || p == RB_HEADER || less(&v, self.key(p));
        self.nodes.push(RbNode {
            value: Some(v),
            parent: RB_NIL,
            left: RB_NIL,
            right: RB_NIL,
            red: true,
        });
        let z = self.nodes.len() - 1;
        self.insert_and_rebalance(insert_left, z, p);
    }

    /// `_M_get_insert_unique_pos( k )`: `(x, p)` of the insert position, `None` when an
    /// equivalent key is present.
    fn get_insert_unique_pos<F>(&self, k: &T, less: &mut F) -> Option<(usize, usize)>
    where
        F: FnMut(&T, &T) -> bool,
    {
        let mut x = self.root();
        let mut y = RB_HEADER;
        let mut comp = true;

        while x != RB_NIL {
            y = x;
            comp = less(k, self.key(x));
            x = if comp {
                self.nodes[x].left
            } else {
                self.nodes[x].right
            };
        }

        let mut j = y;

        if comp {
            if j == self.nodes[RB_HEADER].left {
                // j == begin()
                return Some((x, y));
            }

            j = self.decrement(j);
        }

        if less(self.key(j), k) {
            return Some((x, y));
        }

        None
    }

    /// `_Rb_tree_decrement( x )`.
    fn decrement(&self, mut x: usize) -> usize {
        if self.nodes[x].red && self.nodes[self.nodes[x].parent].parent == x {
            // x is the header (end()): its predecessor is the rightmost node
            return self.nodes[x].right;
        }

        if self.nodes[x].left != RB_NIL {
            let mut y = self.nodes[x].left;

            while self.nodes[y].right != RB_NIL {
                y = self.nodes[y].right;
            }

            return y;
        }

        let mut y = self.nodes[x].parent;

        while x == self.nodes[y].left {
            x = y;
            y = self.nodes[y].parent;
        }

        y
    }

    /// `local_Rb_tree_rotate_left( x, root )`.
    fn rotate_left(&mut self, x: usize) {
        let y = self.nodes[x].right;
        self.nodes[x].right = self.nodes[y].left;

        if self.nodes[y].left != RB_NIL {
            let yl = self.nodes[y].left;
            self.nodes[yl].parent = x;
        }

        self.nodes[y].parent = self.nodes[x].parent;
        let xp = self.nodes[x].parent;

        if x == self.root() {
            self.nodes[RB_HEADER].parent = y;
        } else if x == self.nodes[xp].left {
            self.nodes[xp].left = y;
        } else {
            self.nodes[xp].right = y;
        }

        self.nodes[y].left = x;
        self.nodes[x].parent = y;
    }

    /// `local_Rb_tree_rotate_right( x, root )`.
    fn rotate_right(&mut self, x: usize) {
        let y = self.nodes[x].left;
        self.nodes[x].left = self.nodes[y].right;

        if self.nodes[y].right != RB_NIL {
            let yr = self.nodes[y].right;
            self.nodes[yr].parent = x;
        }

        self.nodes[y].parent = self.nodes[x].parent;
        let xp = self.nodes[x].parent;

        if x == self.root() {
            self.nodes[RB_HEADER].parent = y;
        } else if x == self.nodes[xp].right {
            self.nodes[xp].right = y;
        } else {
            self.nodes[xp].left = y;
        }

        self.nodes[y].right = x;
        self.nodes[x].parent = y;
    }

    /// `_Rb_tree_insert_and_rebalance( insert_left, x, p, header )`.
    fn insert_and_rebalance(&mut self, insert_left: bool, mut x: usize, p: usize) {
        // Initialize fields in new node to insert.
        self.nodes[x].parent = p;
        self.nodes[x].left = RB_NIL;
        self.nodes[x].right = RB_NIL;
        self.nodes[x].red = true;

        // Insert.
        // Make new node child of parent and maintain root, leftmost and rightmost nodes.
        // N.B. First node is always inserted left.
        if insert_left {
            self.nodes[p].left = x; // also makes leftmost = x when p == &header

            if p == RB_HEADER {
                self.nodes[RB_HEADER].parent = x;
                self.nodes[RB_HEADER].right = x;
            } else if p == self.nodes[RB_HEADER].left {
                self.nodes[RB_HEADER].left = x; // maintain leftmost pointing to min node
            }
        } else {
            self.nodes[p].right = x;

            if p == self.nodes[RB_HEADER].right {
                self.nodes[RB_HEADER].right = x; // maintain rightmost pointing to max node
            }
        }

        // Rebalance.
        while x != self.root() && self.nodes[self.nodes[x].parent].red {
            let xp = self.nodes[x].parent;
            let xpp = self.nodes[xp].parent;

            if xp == self.nodes[xpp].left {
                let y = self.nodes[xpp].right;

                if y != RB_NIL && self.nodes[y].red {
                    self.nodes[xp].red = false;
                    self.nodes[y].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp].right {
                        x = xp;
                        self.rotate_left(x);
                    }

                    let xp = self.nodes[x].parent;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    self.rotate_right(xpp);
                }
            } else {
                let y = self.nodes[xpp].left;

                if y != RB_NIL && self.nodes[y].red {
                    self.nodes[xp].red = false;
                    self.nodes[y].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp].left {
                        x = xp;
                        self.rotate_right(x);
                    }

                    let xp = self.nodes[x].parent;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    self.rotate_left(xpp);
                }
            }
        }

        let root = self.root();
        self.nodes[root].red = false;
    }

    /// The node indices in iteration order (`begin()` .. `end()`).
    fn in_order(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.len());
        let mut stack = Vec::new();
        let mut x = self.root();

        while x != RB_NIL || !stack.is_empty() {
            while x != RB_NIL {
                stack.push(x);
                x = self.nodes[x].left;
            }

            let n = stack.pop().expect("non-empty stack");
            out.push(n);
            x = self.nodes[n].right;
        }

        out
    }

    /// The elements in iteration order (`begin()` .. `end()`).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.in_order().into_iter().map(|n| self.key(n))
    }

    /// The elements in iteration order, consuming the set.
    pub fn into_vec(mut self) -> Vec<T> {
        let order = self.in_order();
        order
            .into_iter()
            .map(|n| self.nodes[n].value.take().expect("value node"))
            .collect()
    }
}

/// Port of `std::__lg` (`floor( log2( n ) )` for `n > 0`).
fn lg(n: usize) -> usize {
    (usize::BITS - 1 - n.leading_zeros()) as usize
}

/// Port of `std::__introsort_loop`.
fn introsort_loop<T, F>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth_limit: usize,
    less: &mut F,
) where
    F: FnMut(&T, &T) -> bool,
{
    while last - first > S_THRESHOLD {
        if depth_limit == 0 {
            partial_sort(v, first, last, last, less);
            return;
        }

        depth_limit -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth_limit, less);
        last = cut;
    }
}

/// Port of `std::__introselect`.
fn introselect<T, F>(
    v: &mut [T],
    mut first: usize,
    nth: usize,
    mut last: usize,
    mut depth_limit: usize,
    less: &mut F,
) where
    F: FnMut(&T, &T) -> bool,
{
    while last - first > 3 {
        if depth_limit == 0 {
            heap_select(v, first, nth + 1, last, less);
            // Place the nth largest element in its final position.
            v.swap(first, nth);
            return;
        }

        depth_limit -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);

        if cut <= nth {
            first = cut;
        } else {
            last = cut;
        }
    }

    insertion_sort(v, first, last, less);
}

/// Port of `std::__unguarded_partition_pivot`.
fn unguarded_partition_pivot<T, F>(v: &mut [T], first: usize, last: usize, less: &mut F) -> usize
where
    F: FnMut(&T, &T) -> bool,
{
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, less);
    unguarded_partition(v, first + 1, last, first, less)
}

/// Port of `std::__move_median_to_first`: swaps the median of `a`, `b`, `c` into `result`.
fn move_median_to_first<T, F>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    less: &mut F,
) where
    F: FnMut(&T, &T) -> bool,
{
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
///
/// "Unguarded": with a strict weak ordering the median-of-three pivot guarantees that both
/// scans stop inside the range.  The two bounds checks below never trigger then; they only
/// keep an invalid comparator (undefined behaviour in C++) from running out of the slice.
fn unguarded_partition<T, F>(
    v: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: usize,
    less: &mut F,
) -> usize
where
    F: FnMut(&T, &T) -> bool,
{
    let end = last;

    loop {
        while first < end && less(&v[first], &v[pivot]) {
            first += 1;
        }

        last -= 1;

        while last > pivot && less(&v[pivot], &v[last]) {
            last -= 1;
        }

        if first >= last {
            return first;
        }

        v.swap(first, last);
        first += 1;
    }
}

/// Port of `std::__final_insertion_sort`.
fn final_insertion_sort<T, F>(v: &mut [T], first: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    if last - first > S_THRESHOLD {
        insertion_sort(v, first, first + S_THRESHOLD, less);
        unguarded_insertion_sort(v, first + S_THRESHOLD, last, less);
    } else {
        insertion_sort(v, first, last, less);
    }
}

/// Port of `std::__insertion_sort`.
fn insertion_sort<T, F>(v: &mut [T], first: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    if first == last {
        return;
    }

    for i in first + 1..last {
        if less(&v[i], &v[first]) {
            // val = move( *i ); move_backward( first, i, i + 1 ); *first = move( val );
            v[first..=i].rotate_right(1);
        } else {
            unguarded_linear_insert(v, i, less);
        }
    }
}

/// Port of `std::__unguarded_insertion_sort`.
fn unguarded_insertion_sort<T, F>(v: &mut [T], first: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    for i in first..last {
        unguarded_linear_insert(v, i, less);
    }
}

/// Port of `std::__unguarded_linear_insert`: moves `v[last]` down past every directly
/// preceding element that is greater.
///
/// "Unguarded": the callers guarantee that an element not greater than `v[last]` exists to
/// the left.  The C++ code shifts the elements one by one while it scans; scanning first and
/// rotating afterwards performs the same comparisons and yields the same arrangement.  The
/// check against index 0 only matters for invalid comparators (C++ would read before the
/// first element).
fn unguarded_linear_insert<T, F>(v: &mut [T], last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    let mut pos = last;

    while pos > 0 && less(&v[last], &v[pos - 1]) {
        pos -= 1;
    }

    v[pos..=last].rotate_right(1);
}

/// Port of `std::__partial_sort` (also the heap sort fallback of introsort).
fn partial_sort<T, F>(v: &mut [T], first: usize, middle: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    heap_select(v, first, middle, last, less);
    sort_heap(v, first, middle, less);
}

/// Port of `std::__heap_select`.
fn heap_select<T, F>(v: &mut [T], first: usize, middle: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    make_heap(v, first, middle, less);

    for i in middle..last {
        if less(&v[i], &v[first]) {
            pop_heap(v, first, middle, i, less);
        }
    }
}

/// Port of `std::__make_heap`.
fn make_heap<T, F>(v: &mut [T], first: usize, last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    if last - first < 2 {
        return;
    }

    let len = last - first;
    let mut parent = (len - 2) / 2;

    loop {
        adjust_heap(v, first, parent, len, less);

        if parent == 0 {
            return;
        }

        parent -= 1;
    }
}

/// Port of `std::__adjust_heap( first, holeIndex, len, value, comp )`.
///
/// In the C++ code `value` has been moved out of the sequence and the hole it left travels
/// down the heap.  Here the value stays in the hole (the caller puts it there) and is swapped
/// along, which leaves every other element exactly where the C++ code moves it; the comparator
/// never looks at the hole while it travels down.
///
/// The index arithmetic is done in the signed `difference_type` like in C++: `len` can be 0
/// (`std::partial_sort( first, first, last )`), where `( len - 1 ) / 2` and `( len - 2 ) / 2`
/// rely on truncating signed division.
fn adjust_heap<T, F>(v: &mut [T], first: usize, hole_index: usize, len: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    let len = len as isize;
    let top_index = hole_index;
    let mut hole_index = hole_index as isize;
    let mut second_child = hole_index;
    let at = |i: isize| first + i as usize;

    while second_child < (len - 1) / 2 {
        second_child = 2 * (second_child + 1);

        if less(&v[at(second_child)], &v[at(second_child - 1)]) {
            second_child -= 1;
        }

        v.swap(at(hole_index), at(second_child));
        hole_index = second_child;
    }

    if (len & 1) == 0 && second_child == (len - 2) / 2 {
        second_child = 2 * (second_child + 1);
        v.swap(at(hole_index), at(second_child - 1));
        hole_index = second_child - 1;
    }

    push_heap(v, first, hole_index as usize, top_index, less);
}

/// Port of `std::__push_heap( first, holeIndex, topIndex, value, comp )`; the value is the
/// element in the hole (see [`adjust_heap`]).
fn push_heap<T, F>(v: &mut [T], first: usize, mut hole_index: usize, top_index: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    // C++ computes `( holeIndex - 1 ) / 2` in a signed type: for holeIndex == 0 that is
    // ( -1 ) / 2 == 0, and the loop condition `holeIndex > topIndex` is false anyway.
    let parent_of = |hole: usize| if hole == 0 { 0 } else { (hole - 1) / 2 };
    let mut parent = parent_of(hole_index);

    while hole_index > top_index && less(&v[first + parent], &v[first + hole_index]) {
        v.swap(first + hole_index, first + parent);
        hole_index = parent;
        parent = parent_of(hole_index);
    }
}

/// Port of `std::__pop_heap( first, last, result, comp )`: `result` is outside of
/// `[first, last)` at every call site.
fn pop_heap<T, F>(v: &mut [T], first: usize, last: usize, result: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    // value = move( *result ); *result = move( *first ); the value goes into the hole at the top
    v.swap(result, first);
    adjust_heap(v, first, 0, last - first, less);
}

/// Port of `std::__sort_heap`.
fn sort_heap<T, F>(v: &mut [T], first: usize, mut last: usize, less: &mut F)
where
    F: FnMut(&T, &T) -> bool,
{
    while last - first > 1 {
        last -= 1;
        pop_heap(v, first, last, last, less);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// splitmix64
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
    }

    fn tagged(rng: &mut Rng, n: usize, keys: u64) -> Vec<(u64, usize)> {
        (0..n).map(|i| (rng.next() % keys, i)).collect()
    }

    fn is_permutation(v: &[(u64, usize)]) -> bool {
        let mut seen = vec![false; v.len()];

        for &(_, i) in v {
            if seen[i] {
                return false;
            }

            seen[i] = true;
        }

        true
    }

    #[test]
    fn sort_orders_and_permutes() {
        let mut rng = Rng(1);

        for round in 0..400 {
            let n = (rng.next() % 600) as usize;
            let keys = [1, 2, 5, 50, 1_000_000][round % 5];
            let mut v = tagged(&mut rng, n, keys);
            std_sort(&mut v, |a, b| a.0 < b.0);
            assert!(v.windows(2).all(|w| w[0].0 <= w[1].0));
            assert!(is_permutation(&v));
        }
    }

    /// Values produced by g++ 13 / libstdc++ for the same input (`tests/oracle` checks far
    /// more; this guards the in-crate basics without the golden file).
    #[test]
    fn known_tie_order() {
        // 20 elements, all keys equal: std::sort leaves a specific permutation
        let mut v: Vec<(u64, usize)> = (0..20).map(|i| (0, i)).collect();
        std_sort(&mut v, |a, b| a.0 < b.0);
        let ids: Vec<usize> = v.iter().map(|e| e.1).collect();
        assert_eq!(
            ids,
            [
                10, 19, 18, 17, 16, 15, 14, 13, 12, 11, 0, 9, 8, 7, 6, 5, 4, 3, 2, 1
            ]
        );
    }

    #[test]
    fn heap_sort_fallback_sorts() {
        // McIlroy's "antiquicksort" adversary drives median-of-three quicksort into its
        // quadratic case, which makes introsort take the heap sort path.
        let n = 3000usize;
        let gas = n as i64;
        let mut val: Vec<i64> = vec![gas; n];
        let mut nsolid = 0i64;
        let mut candidate = 0usize;
        let mut ids: Vec<usize> = (0..n).collect();

        std_sort(&mut ids, |&x, &y| {
            if val[x] == gas && val[y] == gas {
                if x == candidate {
                    val[x] = nsolid;
                } else {
                    val[y] = nsolid;
                }

                nsolid += 1;
            }

            if val[x] == gas {
                candidate = x;
            } else if val[y] == gas {
                candidate = y;
            }

            val[x] < val[y]
        });

        let mut keys: Vec<(i64, usize)> = val.iter().copied().zip(0..n).collect();
        std_sort(&mut keys, |a, b| a.0 < b.0);
        assert!(keys.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn partial_sort_and_nth_element() {
        let mut rng = Rng(7);

        for _ in 0..300 {
            let n = 1 + (rng.next() % 300) as usize;
            let v0 = tagged(&mut rng, n, 40);
            let mut sorted: Vec<u64> = v0.iter().map(|e| e.0).collect();
            sorted.sort();

            let middle = (rng.next() % (n as u64 + 1)) as usize;
            let mut v = v0.clone();
            std_partial_sort(&mut v, middle, |a, b| a.0 < b.0);
            assert!(is_permutation(&v));
            assert!(
                v[..middle]
                    .iter()
                    .map(|e| e.0)
                    .eq(sorted[..middle].iter().copied())
            );

            let nth = (rng.next() % n as u64) as usize;
            let mut v = v0.clone();
            std_nth_element(&mut v, nth, |a, b| a.0 < b.0);
            assert!(is_permutation(&v));
            assert_eq!(v[nth].0, sorted[nth]);
            assert!(v[..nth].iter().all(|e| e.0 <= v[nth].0));
            assert!(v[nth..].iter().all(|e| e.0 >= v[nth].0));
        }
    }

    #[test]
    fn heap_functions_and_priority_queue() {
        let mut rng = Rng(3);
        let mut v: Vec<u64> = (0..200).map(|_| rng.next() % 50).collect();
        std_make_heap(&mut v, |a, b| a < b);
        assert!((1..v.len()).all(|i| v[(i - 1) / 2] >= v[i]));
        std_sort_heap(&mut v, |a, b| a < b);
        assert!(v.windows(2).all(|w| w[0] <= w[1]));

        // min-queue on the key, like the C++ `a.first > b.first`
        let mut pq = PriorityQueue::new(|a: &(u64, usize), b: &(u64, usize)| a.0 > b.0);
        assert!(pq.is_empty());
        assert!(pq.pop().is_none());

        for i in 0..100 {
            pq.push((rng.next() % 10, i));
        }

        assert_eq!(pq.len(), 100);
        let mut last = 0;

        while let Some(&(top_key, top_id)) = pq.top() {
            let (key, id) = pq.pop().unwrap();
            assert_eq!((key, id), (top_key, top_id));
            assert!(key >= last);
            last = key;
        }
    }

    #[test]
    fn unique_argument_order() {
        // Asymmetric predicate: pred( kept, candidate )
        let mut v = vec![1, 2, 3, 10, 11, 20];
        let n = std_unique(&mut v, |kept, cand| cand - kept <= 2);
        assert_eq!(n, 3);
        assert_eq!(v, [1, 10, 20]);
    }

    /// Comparators that are not strict weak orderings must neither panic nor hang.
    #[test]
    fn invalid_comparators_are_safe() {
        let mut rng = Rng(11);

        for n in [0usize, 1, 2, 3, 15, 16, 17, 33, 100, 1000] {
            let mut v: Vec<u64> = (0..n).map(|_| rng.next() % 7).collect();
            std_sort(&mut v, |a, b| a <= b);
            std_sort(&mut v, |_, _| true);
            std_partial_sort(&mut v, n / 2, |_, _| true);
            std_nth_element(&mut v, n / 2, |a, b| a <= b);
            assert_eq!(v.len(), n);

            let mut f: Vec<f64> = (0..n)
                .map(|i| {
                    if i % 3 == 0 {
                        f64::NAN
                    } else {
                        (rng.next() % 100) as f64
                    }
                })
                .collect();
            std_sort(&mut f, |a, b| a < b);
            std_nth_element(&mut f, n / 3, |a, b| a < b);
        }
    }
}
