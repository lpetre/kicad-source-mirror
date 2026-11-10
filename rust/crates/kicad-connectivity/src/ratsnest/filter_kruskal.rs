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

//! Port of `libs/core/include/core/filter_kruskal.h` (`KI_MST::FilterKruskal`).
//!
//! Minimum spanning forest by Filter-Kruskal.
//!
//! Plain Kruskal sorts every edge, although most edges of a dense graph close a cycle and are
//! discarded as soon as the scan reaches them.  Filter-Kruskal does not sort those.  It splits
//! the edges at a pivot, recurses into the lighter half, then discards each edge of the heavier
//! half whose endpoints the lighter half already joined.  The cycle property keeps the result
//! the same as plain Kruskal.
//!
//! Osipov, V., Sanders, P., & Singler, J. (2009). The Filter-Kruskal minimum spanning tree
//! algorithm. Proceedings of the Meeting on Algorithm Engineering & Experiments (ALENEX),
//! 52-61. SIAM.
//!
//! It lives next to its only user because `KI_UNION_FIND` was ported into this crate
//! ([`KiUnionFind`]). `std::sort` and `std::nth_element` are the libstdc++ ports of
//! `kicad_geom::stdsort`; `std::partition` is ported below.

use kicad_geom::stdsort::{std_nth_element, std_sort};

use crate::algo::KiUnionFind;

/// Sort and scan a range of this size or smaller instead of splitting it again.
pub const KRUSKAL_THRESHOLD: usize = 1024;

/// Port of `KI_MST::FilterKruskal`: builds a minimum spanning forest over `edges`.
///
/// * `edges` is the candidate edge set.  This function reorders it.
/// * `forest` is the union-find state.  A caller can unite known-connected pairs first,
///   because this function discards the edges those pairs imply.
/// * `less` must be a strict total order on edges and not on their weights alone.  A tie
///   that the order leaves open makes the chosen tree depend on the platform.
/// * `endpoints` maps an edge to its two vertex indices, both below `forest.size()`.
/// * `emit` receives each selected edge in order of increasing weight.
///
/// Returns the number of edges emitted.
pub fn filter_kruskal<E, L, P, M>(
    edges: &mut [E],
    forest: &mut KiUnionFind,
    mut less: L,
    endpoints: P,
    mut emit: M,
) -> usize
where
    L: FnMut(&E, &E) -> bool,
    P: Fn(&E) -> (usize, usize),
    M: FnMut(&E),
{
    let mut emitted = 0;
    let mut state = State {
        edge: std::marker::PhantomData,
        forest,
        less: &mut less,
        endpoints: &endpoints,
        emit: &mut emit,
        emitted: &mut emitted,
    };

    state.recurse(edges);

    emitted
}

/// The captures of the C++ lambdas `kruskal`, `filter` and `recurse`.
struct State<'a, E, L, P, M> {
    edge: std::marker::PhantomData<fn(&E)>,
    forest: &'a mut KiUnionFind,
    less: &'a mut L,
    endpoints: &'a P,
    emit: &'a mut M,
    emitted: &'a mut usize,
}

impl<E, L, P, M> State<'_, E, L, P, M>
where
    L: FnMut(&E, &E) -> bool,
    P: Fn(&E) -> (usize, usize),
    M: FnMut(&E),
{
    /// Lambda `kruskal`: sort the range and run plain Kruskal over it.
    fn kruskal(&mut self, range: &mut [E]) {
        std_sort(range, |a, b| (self.less)(a, b));

        for edge in range.iter() {
            let (u, v) = (self.endpoints)(edge);

            if self.forest.unite(u, v) {
                (self.emit)(edge);
                *self.emitted += 1;
            }
        }
    }

    /// Lambda `filter`: an edge whose endpoints are already joined closes a cycle, so no
    /// minimum spanning forest can hold it and nothing needs to sort it. Returns the length of
    /// the kept prefix.
    fn filter(&mut self, range: &mut [E]) -> usize {
        let forest = &*self.forest;
        let endpoints = self.endpoints;

        std_partition(range, |edge| {
            let (u, v) = endpoints(edge);
            !forest.connected(u, v)
        })
    }

    /// Lambda `recurse`.
    fn recurse(&mut self, range: &mut [E]) {
        // Nothing later can be selected once everything is joined
        if range.is_empty() || self.forest.component_count() == 1 {
            return;
        }

        if range.len() <= KRUSKAL_THRESHOLD {
            self.kruskal(range);
            return;
        }

        // nth_element places the median with the lighter edges ahead of it, so both
        // halves shrink.  It is also deterministic, unlike a random pivot
        let mid = range.len() / 2;
        std_nth_element(range, mid, |a, b| (self.less)(a, b));

        self.recurse(&mut range[..mid]);

        if self.forest.component_count() == 1 {
            return;
        }

        let upper = &mut range[mid..];
        let kept = self.filter(upper);
        self.recurse(&mut upper[..kept]);
    }
}

/// Port of libstdc++ `std::partition` (`std::__partition`, bidirectional iterator version,
/// which libstdc++ also uses for random access iterators): moves the elements satisfying
/// `pred` to the front and returns their count. The order matches libstdc++.
pub fn std_partition<T>(v: &mut [T], mut pred: impl FnMut(&T) -> bool) -> usize {
    let mut first = 0;
    let mut last = v.len();

    loop {
        loop {
            if first == last {
                return first;
            } else if pred(&v[first]) {
                first += 1;
            } else {
                break;
            }
        }

        last -= 1;

        loop {
            if first == last {
                return first;
            } else if !pred(&v[last]) {
                last -= 1;
            } else {
                break;
            }
        }

        v.swap(first, last);
        first += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::std_partition;

    #[test]
    fn partition_matches_libstdcxx_order() {
        // libstdc++: swaps the first failing element from the front with the last passing one
        // from the back.
        let mut v = [1, 2, 3, 4, 5, 6, 7, 8];
        let n = std_partition(&mut v, |x| x % 2 == 0);
        assert_eq!(n, 4);
        assert_eq!(v, [8, 2, 6, 4, 5, 3, 7, 1]);

        let mut e: [i32; 0] = [];
        assert_eq!(std_partition(&mut e, |_| true), 0);
    }
}
