//! `PolyTree64` / `PolyTreeD`: closed solution paths nested by containment.

use crate::core::{Coord, Path, area};

/// One polygon of a [`PolyTree`] with links to its parent and children.
#[derive(Clone, Debug)]
pub struct PolyNode<T> {
    /// The polygon's vertices.
    pub polygon: Path<T>,
    /// `None` for top-level polygons.
    pub parent: Option<usize>,
    /// Indices of the polygons directly inside this one.
    pub children: Vec<usize>,
}

/// Polygons nested by containment: outers at even depth, holes at odd depth. Nodes live in
/// one arena and are addressed by index; [`PolyTree::roots`] lists the top level.
#[derive(Clone, Debug)]
pub struct PolyTree<T> {
    nodes: Vec<PolyNode<T>>,
    roots: Vec<usize>,
}

/// Integer node (`PolyPath64`).
pub type PolyNode64 = PolyNode<i64>;
/// Floating-point node (`PolyPathD`).
pub type PolyNodeD = PolyNode<f64>;
/// Integer polygon tree (`PolyTree64`).
pub type PolyTree64 = PolyTree<i64>;
/// Floating-point polygon tree (`PolyTreeD`).
pub type PolyTreeD = PolyTree<f64>;

impl<T> Default for PolyTree<T> {
    fn default() -> Self {
        PolyTree {
            nodes: Vec::new(),
            roots: Vec::new(),
        }
    }
}

impl<T: Coord> PolyTree<T> {
    /// Indices of the top-level polygons, in order.
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// The node at index `id`; panics if out of range.
    pub fn node(&self, id: usize) -> &PolyNode<T> {
        &self.nodes[id]
    }

    /// Number of polygons in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True if the tree holds no polygons.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Removes every polygon.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.roots.clear();
    }

    /// Depth of a node; top-level polygons are at level 1.
    pub fn level(&self, id: usize) -> usize {
        let mut level = 1;
        let mut p = self.nodes[id].parent;
        while let Some(i) = p {
            level += 1;
            p = self.nodes[i].parent;
        }
        level
    }

    /// True for polygons at even levels.
    pub fn is_hole(&self, id: usize) -> bool {
        self.level(id) % 2 == 0
    }

    /// The signed area of a node plus all of its descendants.
    pub fn node_area(&self, id: usize) -> f64 {
        self.nodes[id]
            .children
            .iter()
            .fold(area(&self.nodes[id].polygon), |a, &c| a + self.node_area(c))
    }

    /// The signed area of every polygon in the tree.
    pub fn area(&self) -> f64 {
        self.roots.iter().fold(0.0, |a, &r| a + self.node_area(r))
    }

    /// Appends a polygon under `parent` (`None` for the top level) and returns its index.
    pub fn add_child(&mut self, parent: Option<usize>, polygon: Path<T>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(PolyNode {
            polygon,
            parent,
            children: Vec::new(),
        });
        match parent {
            Some(p) => self.nodes[p].children.push(id),
            None => self.roots.push(id),
        }
        id
    }

    /// Every node in depth-first pre-order (the order the C++ tree is walked).
    pub fn dfs(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(i) = stack.pop() {
            out.push(i);
            stack.extend(self.nodes[i].children.iter().rev().copied());
        }
        out
    }

    /// Converts every polygon with `f`, keeping the structure.
    pub(crate) fn map<U: Coord>(self, mut f: impl FnMut(Path<T>) -> Path<U>) -> PolyTree<U> {
        PolyTree {
            nodes: self
                .nodes
                .into_iter()
                .map(|n| PolyNode {
                    polygon: f(n.polygon),
                    parent: n.parent,
                    children: n.children,
                })
                .collect(),
            roots: self.roots,
        }
    }
}
