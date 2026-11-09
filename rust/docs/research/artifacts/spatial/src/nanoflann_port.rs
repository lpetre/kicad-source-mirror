//! Research prototype: port of the part of the bundled `thirdparty/nanoflann/nanoflann.hpp`
//! (v1.7.1, BSD) that KiCad uses in `pcbnew/convert_shape_list_to_polygon.cpp`:
//! `KDTreeSingleIndexAdaptor< L2_Simple_Adaptor<double, ...>, ..., 2 >` with default parameters
//! (`leaf_max_size = 10`, single-threaded build), `radiusSearch` and `knnSearch`.
//!
//! The tree build (`divideTree` / `middleSplit_` / `planeSplit`) and the traversal
//! (`searchLevel`) are ported statement by statement, because the order in which equidistant
//! points are reported by `knnSearch` depends on them.

const DIM: usize = 2;

#[derive(Clone, Copy, Debug)]
struct Interval {
    low: f64,
    high: f64,
}

type BoundingBox = [Interval; DIM];

#[derive(Debug)]
enum Node {
    /// Indices of points in leaf node: `[left, right)` into `v_acc`
    Leaf { left: usize, right: usize },
    Sub { divfeat: usize, divlow: f64, divhigh: f64, child1: usize, child2: usize },
}

#[derive(Debug)]
pub struct KdTree {
    pts: Vec<[f64; DIM]>,
    v_acc: Vec<u32>,
    nodes: Vec<Node>,
    root: Option<usize>,
    root_bbox: BoundingBox,
    leaf_max_size: usize,
}

/// `nanoflann::ResultItem<uint32_t, double>`
pub type ResultItem = (u32, f64);

trait ResultSet {
    fn add_point(&mut self, dist: f64, index: u32) -> bool;
    fn worst_dist(&self) -> f64;
}

/// `nanoflann::KNNResultSet` (without `NANOFLANN_FIRST_MATCH`)
struct KnnResultSet {
    indices: Vec<u32>,
    dists: Vec<f64>,
    capacity: usize,
    count: usize,
}

impl ResultSet for KnnResultSet {
    fn add_point(&mut self, dist: f64, index: u32) -> bool {
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
        true
    }

    fn worst_dist(&self) -> f64 {
        if self.count < self.capacity || self.count == 0 { f64::MAX } else { self.dists[self.count - 1] }
    }
}

/// `nanoflann::RadiusResultSet`
struct RadiusResultSet {
    radius: f64,
    items: Vec<ResultItem>,
}

impl ResultSet for RadiusResultSet {
    fn add_point(&mut self, dist: f64, index: u32) -> bool {
        if dist < self.radius {
            self.items.push((index, dist));
        }
        true
    }

    fn worst_dist(&self) -> f64 {
        self.radius
    }
}

impl KdTree {
    /// `KDTreeSingleIndexAdaptor( 2, adaptor )` + `buildIndex()`
    pub fn new(pts: Vec<[f64; DIM]>) -> KdTree {
        let n = pts.len();
        let mut tree = KdTree {
            pts,
            v_acc: (0..n as u32).collect(),
            nodes: Vec::new(),
            root: None,
            root_bbox: [Interval { low: 0.0, high: 0.0 }; DIM],
            leaf_max_size: 10,
        };
        if n == 0 {
            return tree;
        }
        // computeBoundingBox()
        let mut bbox = [Interval { low: 0.0, high: 0.0 }; DIM];
        for i in 0..DIM {
            let v = tree.dataset_get(tree.v_acc[0], i);
            bbox[i] = Interval { low: v, high: v };
        }
        for k in 1..n {
            for i in 0..DIM {
                let val = tree.dataset_get(tree.v_acc[k], i);
                if val < bbox[i].low {
                    bbox[i].low = val;
                }
                if val > bbox[i].high {
                    bbox[i].high = val;
                }
            }
        }
        let root = tree.divide_tree(0, n, &mut bbox);
        tree.root_bbox = bbox;
        tree.root = Some(root);
        tree
    }

    fn dataset_get(&self, element: u32, component: usize) -> f64 {
        self.pts[element as usize][component]
    }

    /// `KDTreeBaseClass::divideTree`
    fn divide_tree(&mut self, left: usize, right: usize, bbox: &mut BoundingBox) -> usize {
        // allocate the node first, like the pool allocator does (keeps parent-before-child ids)
        let id = self.nodes.len();
        self.nodes.push(Node::Leaf { left: 0, right: 0 });

        /* If too few exemplars remain, then make this a leaf node. */
        if (right - left) <= self.leaf_max_size {
            // compute bounding-box of leaf points
            for i in 0..DIM {
                let v = self.dataset_get(self.v_acc[left], i);
                bbox[i].low = v;
                bbox[i].high = v;
            }
            for k in left + 1..right {
                for i in 0..DIM {
                    let val = self.dataset_get(self.v_acc[k], i);
                    if bbox[i].low > val {
                        bbox[i].low = val;
                    }
                    if bbox[i].high < val {
                        bbox[i].high = val;
                    }
                }
            }
            self.nodes[id] = Node::Leaf { left, right };
        } else {
            /* Determine the index, dimension and value for split plane */
            let (idx, cutfeat, cutval) = self.middle_split(left, right - left, bbox);

            /* Recurse on left */
            let mut left_bbox = *bbox;
            left_bbox[cutfeat].high = cutval;
            let child1 = self.divide_tree(left, left + idx, &mut left_bbox);

            /* Recurse on right */
            let mut right_bbox = *bbox;
            right_bbox[cutfeat].low = cutval;
            let child2 = self.divide_tree(left + idx, right, &mut right_bbox);

            let divlow = left_bbox[cutfeat].high;
            let divhigh = right_bbox[cutfeat].low;

            for i in 0..DIM {
                // std::min( a, b ) / std::max( a, b )
                bbox[i].low = if right_bbox[i].low < left_bbox[i].low { right_bbox[i].low } else { left_bbox[i].low };
                bbox[i].high =
                    if left_bbox[i].high < right_bbox[i].high { right_bbox[i].high } else { left_bbox[i].high };
            }
            self.nodes[id] = Node::Sub { divfeat: cutfeat, divlow, divhigh, child1, child2 };
        }
        id
    }

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

    /// `KDTreeBaseClass::middleSplit_`; returns (index, cutfeat, cutval)
    fn middle_split(&mut self, ind: usize, count: usize, bbox: &BoundingBox) -> (usize, usize, f64) {
        const EPS: f64 = 0.00001;
        let mut max_span = bbox[0].high - bbox[0].low;
        for i in 1..DIM {
            let span = bbox[i].high - bbox[i].low;
            if span > max_span {
                max_span = span;
            }
        }
        let mut max_spread = -1.0;
        let mut cutfeat = 0;
        let mut min_elem = 0.0;
        let mut max_elem = 0.0;
        for i in 0..DIM {
            let span = bbox[i].high - bbox[i].low;
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

    /// `KDTreeBaseClass::planeSplit` (`Offset` is unsigned: the `!right` guards are kept)
    fn plane_split(&mut self, ind: usize, count: usize, cutfeat: usize, cutval: f64) -> (usize, usize) {
        let mut left: usize = 0;
        let mut right: usize = count - 1;
        loop {
            while left <= right && self.dataset_get(self.v_acc[ind + left], cutfeat) < cutval {
                left += 1;
            }
            while right != 0 && left <= right && self.dataset_get(self.v_acc[ind + right], cutfeat) >= cutval {
                right -= 1;
            }
            if left > right || right == 0 {
                break;
            }
            self.v_acc.swap(ind + left, ind + right);
            left += 1;
            right -= 1;
        }
        let lim1 = left;
        right = count - 1;
        loop {
            while left <= right && self.dataset_get(self.v_acc[ind + left], cutfeat) <= cutval {
                left += 1;
            }
            while right != 0 && left <= right && self.dataset_get(self.v_acc[ind + right], cutfeat) > cutval {
                right -= 1;
            }
            if left > right || right == 0 {
                break;
            }
            self.v_acc.swap(ind + left, ind + right);
            left += 1;
            right -= 1;
        }
        (lim1, left)
    }

    /// `L2_Simple_Adaptor::evalMetric`
    fn eval_metric(&self, a: &[f64; DIM], b_idx: u32) -> f64 {
        let mut result = 0.0;
        for i in 0..DIM {
            let diff = a[i] - self.dataset_get(b_idx, i);
            result += diff * diff;
        }
        result
    }

    /// `L2_Simple_Adaptor::accum_dist`
    fn accum_dist(a: f64, b: f64) -> f64 {
        (a - b) * (a - b)
    }

    fn compute_initial_distances(&self, vec: &[f64; DIM], dists: &mut [f64; DIM]) -> f64 {
        let mut dist = 0.0;
        for i in 0..DIM {
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

    /// `KDTreeSingleIndexAdaptor::searchLevel` (epsError == 1)
    fn search_level<R: ResultSet>(
        &self,
        result_set: &mut R,
        vec: &[f64; DIM],
        node: usize,
        mut mindist: f64,
        dists: &mut [f64; DIM],
    ) -> bool {
        match self.nodes[node] {
            Node::Leaf { left, right } => {
                let worst_dist = result_set.worst_dist();
                for i in left..right {
                    let accessor = self.v_acc[i];
                    let dist = self.eval_metric(vec, accessor);
                    if dist < worst_dist && !result_set.add_point(dist, self.v_acc[i]) {
                        return false;
                    }
                }
                true
            }
            Node::Sub { divfeat, divlow, divhigh, child1, child2 } => {
                /* Which child branch should be taken first? */
                let idx = divfeat;
                let val = vec[idx];
                let diff1 = val - divlow;
                let diff2 = val - divhigh;

                let (best_child, other_child, cut_dist) = if (diff1 + diff2) < 0.0 {
                    (child1, child2, Self::accum_dist(val, divhigh))
                } else {
                    (child2, child1, Self::accum_dist(val, divlow))
                };

                /* Call recursively to search next level down. */
                if !self.search_level(result_set, vec, best_child, mindist, dists) {
                    return false;
                }

                let dst = dists[idx];
                mindist = mindist + cut_dist - dst;
                dists[idx] = cut_dist;
                // `mindist * epsError <= result_set.worstDist()` with float epsError == 1.0f
                if mindist * (1.0f32 as f64) <= result_set.worst_dist()
                    && !self.search_level(result_set, vec, other_child, mindist, dists)
                {
                    return false;
                }
                dists[idx] = dst;
                true
            }
        }
    }

    fn find_neighbors<R: ResultSet>(&self, result: &mut R, vec: &[f64; DIM]) {
        let Some(root) = self.root else { return };
        let mut dists = [0.0; DIM];
        let dist = self.compute_initial_distances(vec, &mut dists);
        self.search_level(result, vec, root, dist, &mut dists);
    }

    /// `knnSearch( query, num_closest, indices, dists )`: (index, squared distance), nearest first.
    pub fn knn_search(&self, query: &[f64; DIM], num_closest: usize) -> Vec<ResultItem> {
        let mut rs = KnnResultSet {
            indices: vec![0; num_closest],
            dists: vec![0.0; num_closest],
            capacity: num_closest,
            count: 0,
        };
        self.find_neighbors(&mut rs, query);
        (0..rs.count).map(|i| (rs.indices[i], rs.dists[i])).collect()
    }

    /// `radiusSearch( query, radius_sq, matches )` *before* nanoflann's final `std::sort` by
    /// distance (KiCad re-sorts the matches by (distance, index) anyway).
    pub fn radius_search_unsorted(&self, query: &[f64; DIM], radius_sq: f64) -> Vec<ResultItem> {
        let mut rs = RadiusResultSet { radius: radius_sq, items: Vec::new() };
        self.find_neighbors(&mut rs, query);
        rs.items
    }
}
