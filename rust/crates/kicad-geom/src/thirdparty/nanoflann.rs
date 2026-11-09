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
//
// This file is a port of parts of nanoflann (thirdparty/nanoflann/nanoflann.hpp, v1.7.1),
// which carries the following licence:
//
// Software License Agreement (BSD License)
//
// Copyright 2008-2009  Marius Muja (mariusm@cs.ubc.ca). All rights reserved.
// Copyright 2008-2009  David G. Lowe (lowe@cs.ubc.ca). All rights reserved.
// Copyright 2011-2025  Jose Luis Blanco (joseluisblancoc@gmail.com).
//   All rights reserved.
//
// THE BSD LICENSE
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
// IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
// OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
// IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
// NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
// THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Port of the subset of `thirdparty/nanoflann/nanoflann.hpp` (v1.7.1) that KiCad uses.
//!
//! The only production user in scope is `pcbnew/convert_shape_list_to_polygon.cpp`:
//! `KDTreeSingleIndexAdaptor< L2_Simple_Adaptor<double, PCB_SHAPE_ENDPOINTS_ADAPTOR>,
//! PCB_SHAPE_ENDPOINTS_ADAPTOR, 2 >` (index type `uint32_t`, default parameters:
//! `leaf_max_size = 10`, no `kdtree_get_bbox`), queried with `radiusSearch` (default
//! `SearchParameters`: `eps = 0`, `sorted = true`) and `knnSearch`.
//! (`pcbnew/fix_board_shape.cpp` uses the same types from a GUI tool.)
//!
//! Ported are: [`KdTreeSingleIndexAdaptor`] (`buildIndex` with `computeBoundingBox`,
//! `divideTree`, `middleSplit_`, `planeSplit`, `computeMinMax`; `findNeighbors`,
//! `searchLevel`, `computeInitialDistances`, `knnSearch`, `radiusSearch`), the
//! `L2_Simple_Adaptor` metric, [`KnnResultSet`] (without `NANOFLANN_FIRST_MATCH`) and
//! [`RadiusResultSet`].  The tree build and traversal are ported statement by statement
//! because the order in which equidistant points are reported depends on them; the final
//! sort of `radiusSearch` uses the libstdc++-exact [`crate::stdsort::std_sort`].
//!
//! The pool allocator and the multi-threaded build (`divideTreeConcurrent`, which builds the
//! same tree) are not needed: nodes live in a `Vec`.

use crate::stdsort::std_sort;

/// The index type of the KiCad instantiation (`uint32_t`).
pub type IndexType = u32;

/// Port of `nanoflann::ResultItem<uint32_t, double>`: `first` is the index of the sample in
/// the dataset, `second` the (squared) distance from the sample to the query point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResultItem {
    pub first: IndexType,
    pub second: f64,
}

/// The dataset interface nanoflann expects (`kdtree_get_point_count`, `kdtree_get_pt`; the
/// optional `kdtree_get_bbox` returns `false` in KiCad and is not part of the port).
pub trait DatasetAdaptor {
    /// `kdtree_get_point_count()`
    fn kdtree_get_point_count(&self) -> usize;

    /// `kdtree_get_pt( idx, dim )`: the `dim`'th component of the `idx`'th point.
    fn kdtree_get_pt(&self, idx: usize, dim: usize) -> f64;
}

impl<const D: usize> DatasetAdaptor for Vec<[f64; D]> {
    fn kdtree_get_point_count(&self) -> usize {
        self.len()
    }

    fn kdtree_get_pt(&self, idx: usize, dim: usize) -> f64 {
        self[idx][dim]
    }
}

impl<const D: usize> DatasetAdaptor for &[[f64; D]] {
    fn kdtree_get_point_count(&self) -> usize {
        self.len()
    }

    fn kdtree_get_pt(&self, idx: usize, dim: usize) -> f64 {
        self[idx][dim]
    }
}

/// Port of `nanoflann::KDTreeSingleIndexAdaptorParams` (the build is always single-threaded;
/// `n_thread_build` and `flags` do not influence the resulting tree and are not ported).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdTreeSingleIndexAdaptorParams {
    pub leaf_max_size: usize,
}

impl Default for KdTreeSingleIndexAdaptorParams {
    fn default() -> Self {
        KdTreeSingleIndexAdaptorParams { leaf_max_size: 10 }
    }
}

/// Port of `nanoflann::SearchParameters`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchParameters {
    /// search for eps-approximate neighbours (default: 0)
    pub eps: f32,
    /// only for radius search, require neighbours sorted by distance (default: true)
    pub sorted: bool,
}

impl Default for SearchParameters {
    fn default() -> Self {
        SearchParameters {
            eps: 0.0,
            sorted: true,
        }
    }
}

/// The result set interface of `findNeighbors` (`addPoint`, `worstDist`, `sort`, `full`).
pub trait ResultSet {
    /// `addPoint`: called during search to add an element matching the criteria.  Returns
    /// `true` if the search should be continued, `false` if the results are sufficient.
    fn add_point(&mut self, dist: f64, index: IndexType) -> bool;

    /// `worstDist`
    fn worst_dist(&self) -> f64;

    /// `sort`
    fn sort(&mut self);

    /// `full`
    fn full(&self) -> bool;
}

/// Port of `nanoflann::KNNResultSet<double, uint32_t>`: result set for KNN searches
/// (N-closest neighbors), without `NANOFLANN_FIRST_MATCH`.
#[derive(Clone, Debug)]
pub struct KnnResultSet {
    indices: Vec<IndexType>,
    dists: Vec<f64>,
    capacity: usize,
    count: usize,
}

impl KnnResultSet {
    /// `KNNResultSet( capacity )` + `init()` with output arrays of `capacity` elements.
    pub fn new(capacity: usize) -> Self {
        KnnResultSet {
            indices: vec![0; capacity],
            dists: vec![0.0; capacity],
            capacity,
            count: 0,
        }
    }

    /// `size()`
    pub fn len(&self) -> usize {
        self.count
    }

    /// `empty()`
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The found indices, nearest first.
    pub fn indices(&self) -> &[IndexType] {
        &self.indices[..self.count]
    }

    /// The found squared distances, nearest first.
    pub fn dists(&self) -> &[f64] {
        &self.dists[..self.count]
    }
}

impl ResultSet for KnnResultSet {
    fn add_point(&mut self, dist: f64, index: IndexType) -> bool {
        let mut i = self.count;

        while i > 0 {
            if self.dists[i - 1] > dist {
                if i < self.capacity {
                    self.dists[i] = self.dists[i - 1];
                    self.indices[i] = self.indices[i - 1];
                }
            } else {
                break;
            }

            i -= 1;
        }

        if i < self.capacity {
            self.dists[i] = dist;
            self.indices[i] = index;
        }

        if self.count < self.capacity {
            self.count += 1;
        }

        // tell caller that the search shall continue
        true
    }

    /// Returns the worst distance among found solutions if the search result is full, or the
    /// maximum possible distance, if not full yet.
    fn worst_dist(&self) -> f64 {
        if self.count < self.capacity || self.count == 0 {
            f64::MAX
        } else {
            self.dists[self.count - 1]
        }
    }

    fn sort(&mut self) {
        // already sorted
    }

    fn full(&self) -> bool {
        self.count == self.capacity
    }
}

/// Port of `nanoflann::RadiusResultSet<double, uint32_t>`: all points with a distance
/// strictly below `radius`.
#[derive(Clone, Debug)]
pub struct RadiusResultSet {
    pub radius: f64,
    pub indices_dists: Vec<ResultItem>,
}

impl RadiusResultSet {
    /// `RadiusResultSet( radius, indices_dists )` (which clears the output vector).
    pub fn new(radius: f64) -> Self {
        RadiusResultSet {
            radius,
            indices_dists: Vec::new(),
        }
    }
}

impl ResultSet for RadiusResultSet {
    fn add_point(&mut self, dist: f64, index: IndexType) -> bool {
        if dist < self.radius {
            self.indices_dists.push(ResultItem {
                first: index,
                second: dist,
            });
        }

        true
    }

    fn worst_dist(&self) -> f64 {
        self.radius
    }

    /// `std::sort( ..., IndexDist_Sorter() )`: by distance only, so equal distances are left
    /// in the libstdc++ order.
    fn sort(&mut self) {
        std_sort(&mut self.indices_dists, |p1, p2| p1.second < p2.second);
    }

    fn full(&self) -> bool {
        true
    }
}

/// `Interval { low, high }` of the bounding box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Interval {
    low: f64,
    high: f64,
}

/// `Node`: a leaf (`child1 == child2 == nullptr`, `lr.left/right`) or an inner node
/// (`sub.divfeat/divlow/divhigh`).
#[derive(Clone, Copy, Debug)]
enum Node {
    /// Indices of points in leaf node: `[left, right)` into `vAcc_`
    Leaf { left: usize, right: usize },
    Sub {
        /// Dimension used for subdivision.
        divfeat: usize,
        /// The values used for subdivision.
        divlow: f64,
        divhigh: f64,
        child1: usize,
        child2: usize,
    },
}

/// Port of `nanoflann::KDTreeSingleIndexAdaptor< L2_Simple_Adaptor<double, DS>, DS, D,
/// uint32_t >`: a static kd-tree over the points of a dataset with the (squared) Euclidean
/// distance.
///
/// ```
/// use kicad_geom::thirdparty::nanoflann::KdTreeSingleIndexAdaptor;
///
/// let points: Vec<[f64; 2]> = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
/// let tree = KdTreeSingleIndexAdaptor::<_, 2>::new(points);
///
/// let near = tree.radius_search(&[9.0, 0.0], 4.0);
/// assert_eq!(near.len(), 1);
/// assert_eq!(near[0].first, 1);
///
/// let (indices, dists) = tree.knn_search(&[9.0, 9.0], 2);
/// assert_eq!(indices, [2, 1]);
/// assert_eq!(dists, [2.0, 82.0]);
/// ```
#[derive(Clone, Debug)]
pub struct KdTreeSingleIndexAdaptor<DS, const D: usize> {
    dataset: DS,
    v_acc: Vec<IndexType>,
    nodes: Vec<Node>,
    root: Option<usize>,
    root_bbox: [Interval; D],
    leaf_max_size: usize,
}

impl<DS: DatasetAdaptor, const D: usize> KdTreeSingleIndexAdaptor<DS, D> {
    /// `KDTreeSingleIndexAdaptor( D, dataset )`: builds the index with the default
    /// parameters.
    pub fn new(dataset: DS) -> Self {
        Self::with_params(dataset, KdTreeSingleIndexAdaptorParams::default())
    }

    /// `KDTreeSingleIndexAdaptor( D, dataset, params )`: builds the index.
    pub fn with_params(dataset: DS, params: KdTreeSingleIndexAdaptorParams) -> Self {
        let mut tree = KdTreeSingleIndexAdaptor {
            dataset,
            v_acc: Vec::new(),
            nodes: Vec::new(),
            root: None,
            root_bbox: [Interval::default(); D],
            leaf_max_size: params.leaf_max_size,
        };

        tree.build_index();
        tree
    }

    /// The dataset the tree was built from.
    pub fn dataset(&self) -> &DS {
        &self.dataset
    }

    /// `size()`: number of points in the index.
    pub fn len(&self) -> usize {
        self.v_acc.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.v_acc.is_empty()
    }

    /// Port of `buildIndex`: (re)builds the index from the dataset.
    pub fn build_index(&mut self) {
        let size = self.dataset.kdtree_get_point_count();
        self.init_vind(size);
        self.nodes.clear();
        self.root = None;

        if size == 0 {
            return;
        }

        let mut bbox = self.compute_bounding_box();

        // construct the tree
        let root = self.divide_tree(0, size, &mut bbox);
        self.root_bbox = bbox;
        self.root = Some(root);
    }

    /// Port of `init_vind`: create a permutable array of indices to the input vectors.
    fn init_vind(&mut self, size: usize) {
        self.v_acc = (0..size as IndexType).collect();
    }

    /// `dataset_get`
    fn dataset_get(&self, element: IndexType, component: usize) -> f64 {
        self.dataset.kdtree_get_pt(element as usize, component)
    }

    /// Port of `computeBoundingBox` (the dataset has no `kdtree_get_bbox`).
    fn compute_bounding_box(&self) -> [Interval; D] {
        let mut bbox = [Interval::default(); D];

        for (i, b) in bbox.iter_mut().enumerate() {
            let v = self.dataset_get(self.v_acc[0], i);
            b.low = v;
            b.high = v;
        }

        for k in 1..self.v_acc.len() {
            for (i, b) in bbox.iter_mut().enumerate() {
                let val = self.dataset_get(self.v_acc[k], i);

                if val < b.low {
                    b.low = val;
                }

                if val > b.high {
                    b.high = val;
                }
            }
        }

        bbox
    }

    /// Port of `divideTree`: create a tree node that subdivides the list of vecs from
    /// `vind[left]` to `vind[right]`.  The routine is called recursively on each sublist.
    /// `bbox` is the bounding box used as input for splitting and output for the parent
    /// node.
    fn divide_tree(&mut self, left: usize, right: usize, bbox: &mut [Interval; D]) -> usize {
        // allocate memory (parent before children, like the pool allocator)
        let id = self.nodes.len();
        self.nodes.push(Node::Leaf { left, right });

        // If too few exemplars remain, then make this a leaf node.
        if (right - left) <= self.leaf_max_size {
            // compute bounding-box of leaf points (C++: vAcc_[left] is read even for an
            // empty leaf; past the end it is undefined behaviour, here the box is kept)
            if let Some(&first) = self.v_acc.get(left) {
                for (i, b) in bbox.iter_mut().enumerate() {
                    let v = self.dataset_get(first, i);
                    b.low = v;
                    b.high = v;
                }
            }

            for k in left + 1..right {
                for (i, b) in bbox.iter_mut().enumerate() {
                    let val = self.dataset_get(self.v_acc[k], i);

                    if b.low > val {
                        b.low = val;
                    }

                    if b.high < val {
                        b.high = val;
                    }
                }
            }
        } else {
            // Determine the index, dimension and value for split plane
            let (idx, cutfeat, cutval) = self.middle_split(left, right - left, bbox);

            // Recurse on left
            let mut left_bbox = *bbox;
            left_bbox[cutfeat].high = cutval;
            let child1 = self.divide_tree(left, left + idx, &mut left_bbox);

            // Recurse on right
            let mut right_bbox = *bbox;
            right_bbox[cutfeat].low = cutval;
            let child2 = self.divide_tree(left + idx, right, &mut right_bbox);

            let divlow = left_bbox[cutfeat].high;
            let divhigh = right_bbox[cutfeat].low;

            for i in 0..D {
                // std::min( left, right ): right if right < left; std::max( left, right ):
                // right if left < right
                bbox[i].low = if right_bbox[i].low < left_bbox[i].low {
                    right_bbox[i].low
                } else {
                    left_bbox[i].low
                };
                bbox[i].high = if left_bbox[i].high < right_bbox[i].high {
                    right_bbox[i].high
                } else {
                    left_bbox[i].high
                };
            }

            self.nodes[id] = Node::Sub {
                divfeat: cutfeat,
                divlow,
                divhigh,
                child1,
                child2,
            };
        }

        id
    }

    /// Port of `computeMinMax`.
    fn compute_min_max(&self, ind: usize, count: usize, element: usize) -> (f64, f64) {
        let mut min_elem = self.dataset_get(self.v_acc[ind], element);
        let mut max_elem = min_elem;

        for i in 1..count {
            let val = self.dataset_get(self.v_acc[ind + i], element);

            if val < min_elem {
                min_elem = val;
            }

            if val > max_elem {
                max_elem = val;
            }
        }

        (min_elem, max_elem)
    }

    /// Port of `middleSplit_`; returns `( index, cutfeat, cutval )`.
    fn middle_split(
        &mut self,
        ind: usize,
        count: usize,
        bbox: &[Interval; D],
    ) -> (usize, usize, f64) {
        const EPS: f64 = 0.00001;

        let mut max_span = bbox[0].high - bbox[0].low;

        for b in &bbox[1..] {
            let span = b.high - b.low;

            if span > max_span {
                max_span = span;
            }
        }

        let mut max_spread = -1.0;
        let mut cutfeat = 0;
        let mut min_elem = 0.0;
        let mut max_elem = 0.0;

        for (i, b) in bbox.iter().enumerate() {
            let span = b.high - b.low;

            if span >= (1.0 - EPS) * max_span {
                let (min_elem_, max_elem_) = self.compute_min_max(ind, count, i);
                let spread = max_elem_ - min_elem_;

                if spread > max_spread {
                    cutfeat = i;
                    max_spread = spread;
                    min_elem = min_elem_;
                    max_elem = max_elem_;
                }
            }
        }

        // split in the middle
        let split_val = (bbox[cutfeat].low + bbox[cutfeat].high) / 2.0;

        let cutval = if split_val < min_elem {
            min_elem
        } else if split_val > max_elem {
            max_elem
        } else {
            split_val
        };

        let (lim1, lim2) = self.plane_split(ind, count, cutfeat, cutval);

        let index = if lim1 > count / 2 {
            lim1
        } else if lim2 < count / 2 {
            lim2
        } else {
            count / 2
        };

        (index, cutfeat, cutval)
    }

    /// Port of `planeSplit`: subdivide the list of points by a plane perpendicular on the
    /// axis corresponding to the `cutfeat` dimension at `cutval` position.
    ///
    /// On return:
    /// `dataset[ind[0..lim1-1]][cutfeat] < cutval`,
    /// `dataset[ind[lim1..lim2-1]][cutfeat] == cutval`,
    /// `dataset[ind[lim2..count]][cutfeat] > cutval`.
    fn plane_split(
        &mut self,
        ind: usize,
        count: usize,
        cutfeat: usize,
        cutval: f64,
    ) -> (usize, usize) {
        // First pass.
        // Determine lim1 with all values less than cutval to the left.
        let mut left: usize = 0;
        let mut right: usize = count - 1;

        loop {
            while left <= right && self.dataset_get(self.v_acc[ind + left], cutfeat) < cutval {
                left += 1;
            }

            while right != 0
                && left <= right
                && self.dataset_get(self.v_acc[ind + right], cutfeat) >= cutval
            {
                right -= 1;
            }

            if left > right || right == 0 {
                break; // "!right" was added to support unsigned Index types
            }

            self.v_acc.swap(ind + left, ind + right);
            left += 1;
            right -= 1;
        }

        // Second pass
        // Determine lim2 with all values greater than cutval to the right
        // The middle is used for balancing the tree
        let lim1 = left;
        right = count - 1;

        loop {
            while left <= right && self.dataset_get(self.v_acc[ind + left], cutfeat) <= cutval {
                left += 1;
            }

            while right != 0
                && left <= right
                && self.dataset_get(self.v_acc[ind + right], cutfeat) > cutval
            {
                right -= 1;
            }

            if left > right || right == 0 {
                break; // "!right" was added to support unsigned Index types
            }

            self.v_acc.swap(ind + left, ind + right);
            left += 1;
            right -= 1;
        }

        (lim1, left)
    }

    /// Port of `L2_Simple_Adaptor::evalMetric`.
    fn eval_metric(&self, a: &[f64; D], b_idx: IndexType) -> f64 {
        let mut result = 0.0;

        for (i, &ai) in a.iter().enumerate() {
            let diff = ai - self.dataset_get(b_idx, i);
            result += diff * diff;
        }

        result
    }

    /// Port of `L2_Simple_Adaptor::accum_dist`.
    fn accum_dist(a: f64, b: f64) -> f64 {
        (a - b) * (a - b)
    }

    /// Port of `computeInitialDistances`.
    fn compute_initial_distances(&self, vec: &[f64; D], dists: &mut [f64; D]) -> f64 {
        let mut dist = 0.0;

        for i in 0..D {
            if vec[i] < self.root_bbox[i].low {
                dists[i] = Self::accum_dist(vec[i], self.root_bbox[i].low);
                dist += dists[i];
            }

            if vec[i] > self.root_bbox[i].high {
                dists[i] = Self::accum_dist(vec[i], self.root_bbox[i].high);
                dist += dists[i];
            }
        }

        dist
    }

    /// Port of `searchLevel`.  Returns `false` if the result set does not want any more
    /// points.
    fn search_level<R: ResultSet>(
        &self,
        result_set: &mut R,
        vec: &[f64; D],
        node: usize,
        mut mindist: f64,
        dists: &mut [f64; D],
        eps_error: f32,
    ) -> bool {
        match self.nodes[node] {
            // If this is a leaf node, then do check and return.
            Node::Leaf { left, right } => {
                let worst_dist = result_set.worst_dist();

                for i in left..right {
                    let accessor = self.v_acc[i];
                    let dist = self.eval_metric(vec, accessor);

                    if dist < worst_dist && !result_set.add_point(dist, self.v_acc[i]) {
                        // the resultset doesn't want to receive any more points, we're done
                        // searching!
                        return false;
                    }
                }

                true
            }
            Node::Sub {
                divfeat,
                divlow,
                divhigh,
                child1,
                child2,
            } => {
                // Which child branch should be taken first?
                let idx = divfeat;
                let val = vec[idx];
                let diff1 = val - divlow;
                let diff2 = val - divhigh;

                let (best_child, other_child, cut_dist) = if (diff1 + diff2) < 0.0 {
                    (child1, child2, Self::accum_dist(val, divhigh))
                } else {
                    (child2, child1, Self::accum_dist(val, divlow))
                };

                // Call recursively to search next level down.
                if !self.search_level(result_set, vec, best_child, mindist, dists, eps_error) {
                    // the resultset doesn't want to receive any more points, we're done
                    // searching!
                    return false;
                }

                let dst = dists[idx];
                mindist = mindist + cut_dist - dst;
                dists[idx] = cut_dist;

                // `mindist * epsError` with a float epsError promoted to double
                if mindist * (eps_error as f64) <= result_set.worst_dist()
                    && !self.search_level(result_set, vec, other_child, mindist, dists, eps_error)
                {
                    // the resultset doesn't want to receive any more points, we're done
                    // searching!
                    return false;
                }

                dists[idx] = dst;
                true
            }
        }
    }

    /// Port of `findNeighbors`: find set of nearest neighbors to `vec`.  Their indices are
    /// stored inside the result object.  Returns `result.full()`, or `false` for an empty
    /// index.
    pub fn find_neighbors<R: ResultSet>(
        &self,
        result: &mut R,
        vec: &[f64; D],
        search_params: &SearchParameters,
    ) -> bool {
        let Some(root) = self.root else {
            return false;
        };

        let eps_error: f32 = 1.0 + search_params.eps;

        // Fill it with zeros.
        let mut dists = [0.0f64; D];
        let dist = self.compute_initial_distances(vec, &mut dists);
        self.search_level(result, vec, root, dist, &mut dists, eps_error);

        if search_params.sorted {
            result.sort();
        }

        result.full()
    }

    /// Port of `knnSearch( query_point, num_closest, out_indices, out_distances )`: the
    /// `num_closest` nearest neighbours, nearest first (equal distances in the order the
    /// traversal found them).  Returns `( indices, squared distances )`; their length is the
    /// C++ return value.
    pub fn knn_search(
        &self,
        query_point: &[f64; D],
        num_closest: usize,
    ) -> (Vec<IndexType>, Vec<f64>) {
        let mut result_set = KnnResultSet::new(num_closest);
        self.find_neighbors(&mut result_set, query_point, &SearchParameters::default());
        (result_set.indices().to_vec(), result_set.dists().to_vec())
    }

    /// Port of `radiusSearch( query_point, radius, IndicesDists )` with the default
    /// `SearchParameters` (sorted by distance): all points whose squared distance is strictly
    /// below `radius` (a squared radius, as in nanoflann).
    pub fn radius_search(&self, query_point: &[f64; D], radius: f64) -> Vec<ResultItem> {
        self.radius_search_with_params(query_point, radius, &SearchParameters::default())
    }

    /// Port of `radiusSearch( query_point, radius, IndicesDists, searchParams )`.
    pub fn radius_search_with_params(
        &self,
        query_point: &[f64; D],
        radius: f64,
        search_params: &SearchParameters,
    ) -> Vec<ResultItem> {
        let mut result_set = RadiusResultSet::new(radius);
        self.find_neighbors(&mut result_set, query_point, search_params);
        result_set.indices_dists
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_small() {
        let tree = KdTreeSingleIndexAdaptor::<_, 2>::new(Vec::<[f64; 2]>::new());
        assert!(tree.is_empty());
        assert_eq!(tree.knn_search(&[0.0, 0.0], 3), (vec![], vec![]));
        assert!(tree.radius_search(&[0.0, 0.0], 10.0).is_empty());

        // more points than a leaf holds, many duplicates
        let pts: Vec<[f64; 2]> = (0..100).map(|i| [(i % 7) as f64, (i % 3) as f64]).collect();
        let tree = KdTreeSingleIndexAdaptor::<_, 2>::new(pts.clone());
        assert_eq!(tree.len(), 100);

        for q in [[0.0, 0.0], [3.5, 1.0], [10.0, -4.0]] {
            let r = tree.radius_search(&q, 2.0);
            let brute = pts
                .iter()
                .filter(|p| (p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]) < 2.0)
                .count();
            assert_eq!(r.len(), brute);
            assert!(r.windows(2).all(|w| w[0].second <= w[1].second));

            let (idx, d) = tree.knn_search(&q, 8);
            assert_eq!(idx.len(), 8);
            assert!(d.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
