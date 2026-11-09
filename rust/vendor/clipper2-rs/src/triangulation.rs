//! Constrained (optionally Delaunay) triangulation of non-intersecting polygons
//! (`clipper.triangulation.cpp`). Upstream has withdrawn this code; it is maintained here.

use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

use crate::core::{
    Path64, Paths64, PathsD, Point64, check_range, cross_product_sign_t, distance_sqr, scale_paths,
    sqr,
};

type Idx = u32;
const NONE: Idx = u32::MAX;

/// Why [`triangulate`] produced no triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TriangulateError {
    /// Internal failure (upstream returns `fail` or throws), or a result that would not
    /// cover the input exactly.
    Fail,
    /// Fewer than three usable vertices.
    NoPolygons,
    /// The paths intersect each other or themselves.
    PathsIntersect,
    /// A floating-point coordinate is out of range after scaling.
    CoordinateOutOfRange,
    /// A path's outline walk never returns to its start (typically a self-intersecting path).
    /// Upstream loops forever, allocating without bound, on such input.
    Degenerate,
}

impl fmt::Display for TriangulateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fail => "triangulation failed",
            Self::NoPolygons => "no polygons to triangulate",
            Self::PathsIntersect => "paths intersect",
            Self::CoordinateOutOfRange => "coordinate out of range",
            Self::Degenerate => "path outline walk does not terminate",
        })
    }
}

impl std::error::Error for TriangulateError {}

/// Upstream's `DoError(undefined_error_i)`: an inconsistent internal state.
#[derive(Debug)]
struct Oops;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeKind {
    Loose,
    Ascend,
    Descend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntersectKind {
    None,
    Collinear,
    Intersect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeContains {
    Neither,
    Left,
    Right,
}

struct Vertex2 {
    pt: Point64,
    edges: Vec<Idx>,
    inner_lm: bool,
}

#[derive(Clone, Copy)]
struct Edge {
    v_l: Idx,
    v_r: Idx,
    v_b: Idx,
    v_t: Idx,
    kind: EdgeKind,
    tri_a: Idx,
    tri_b: Idx,
    is_active: bool,
    next_e: Idx,
    prev_e: Idx,
}

#[inline]
fn left_turning(p1: Point64, p2: Point64, p3: Point64) -> bool {
    cross_product_sign_t(p1, p2, p3) < 0
}

#[inline]
fn right_turning(p1: Point64, p2: Point64, p3: Point64) -> bool {
    cross_product_sign_t(p1, p2, p3) > 0
}

fn in_circle_test(pt_a: Point64, pt_b: Point64, pt_c: Point64, pt_d: Point64) -> f64 {
    let m00 = pt_a.x.wrapping_sub(pt_d.x) as f64;
    let m01 = pt_a.y.wrapping_sub(pt_d.y) as f64;
    let m02 = sqr(m00) + sqr(m01);
    let m10 = pt_b.x.wrapping_sub(pt_d.x) as f64;
    let m11 = pt_b.y.wrapping_sub(pt_d.y) as f64;
    let m12 = sqr(m10) + sqr(m11);
    let m20 = pt_c.x.wrapping_sub(pt_d.x) as f64;
    let m21 = pt_c.y.wrapping_sub(pt_d.y) as f64;
    let m22 = sqr(m20) + sqr(m21);
    m00 * (m11 * m22 - m21 * m12) - m10 * (m01 * m22 - m21 * m02) + m20 * (m01 * m12 - m11 * m02)
}

fn shortest_dist_from_segment(pt: Point64, seg1: Point64, seg2: Point64) -> f64 {
    let dx = seg2.x.wrapping_sub(seg1.x) as f64;
    let dy = seg2.y.wrapping_sub(seg1.y) as f64;
    let ax = pt.x.wrapping_sub(seg1.x) as f64;
    let ay = pt.y.wrapping_sub(seg1.y) as f64;
    let q_num = ax * dx + ay * dy;
    if q_num < 0.0 {
        distance_sqr(pt, seg1)
    } else if q_num > sqr(dx) + sqr(dy) {
        distance_sqr(pt, seg2)
    } else {
        sqr(ax * dy - dx * ay) / (dx * dx + dy * dy)
    }
}

fn segs_intersect(s1a: Point64, s1b: Point64, s2a: Point64, s2b: Point64) -> IntersectKind {
    if s1a == s2a || s1b == s2a || s1b == s2b {
        return IntersectKind::None;
    }
    let dy1 = s1b.y.wrapping_sub(s1a.y) as f64;
    let dx1 = s1b.x.wrapping_sub(s1a.x) as f64;
    let dy2 = s2b.y.wrapping_sub(s2a.y) as f64;
    let dx2 = s2b.x.wrapping_sub(s2a.x) as f64;
    let cp = dy1 * dx2 - dy2 * dx1;
    if cp == 0.0 {
        return IntersectKind::Collinear;
    }
    let ax = s1a.x.wrapping_sub(s2a.x) as f64;
    let ay = s1a.y.wrapping_sub(s2a.y) as f64;
    let mut t = ax * dy2 - ay * dx2;
    if t >= 0.0 {
        if cp < 0.0 || t >= cp {
            return IntersectKind::None;
        }
    } else if cp > 0.0 || t <= cp {
        return IntersectKind::None;
    }
    t = ax * dy1 - ay * dx1;
    if t >= 0.0 {
        if cp > 0.0 && t < cp {
            return IntersectKind::Intersect;
        }
    } else if cp < 0.0 && t > cp {
        return IntersectKind::Intersect;
    }
    IntersectKind::None
}

/// For directions pointing up (y <= 0): `> 0` when `b` points further left than `a`.
fn upward_turn(a: Point64, b: Point64) -> i128 {
    // with y flipped, both lie in the upper half-plane and "further left" is a larger angle
    a.x as i128 * -(b.y as i128) - -(a.y as i128) * b.x as i128
}

fn distance_sqr_exact(a: Point64, b: Point64) -> i128 {
    let (dx, dy) = ((a.x - b.x) as i128, (a.y - b.y) as i128);
    dx * dx + dy * dy
}

/// Whether segment `c`-`d` crosses segment `a`-`b`, or has an end point strictly inside it
/// (exact).
fn segment_blocked(a: Point64, b: Point64, c: Point64, d: Point64) -> bool {
    strictly_inside(c, a, b) || strictly_inside(d, a, b) || segments_cross(a, b, c, d)
}

/// Whether `p` lies on segment `a`-`b` without being one of its ends (exact).
fn strictly_inside(p: Point64, a: Point64, b: Point64) -> bool {
    p != a
        && p != b
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
        && cross_product_sign_t(a, p, b) == 0
}

/// Whether segments `a`-`b` and `c`-`d` cross at a point inside both (exact).
fn segments_cross(a: Point64, b: Point64, c: Point64, d: Point64) -> bool {
    let (s1, s2) = (cross_product_sign_t(a, b, c), cross_product_sign_t(a, b, d));
    let (s3, s4) = (cross_product_sign_t(c, d, a), cross_product_sign_t(c, d, b));
    s1 * s2 < 0 && s3 * s4 < 0
}

fn find_loc_min_idx(path: &Path64, len: usize, idx: &mut usize) -> bool {
    if len < 3 {
        return false;
    }
    let i0 = *idx;
    let mut n = (*idx + 1) % len;
    while path[n].y <= path[*idx].y {
        *idx = n;
        n = (n + 1) % len;
        if *idx == i0 {
            return false; // completely horizontal
        }
    }
    while path[n].y >= path[*idx].y {
        *idx = n;
        n = (n + 1) % len;
    }
    true
}

#[inline]
fn prev_i(idx: usize, len: usize) -> usize {
    if idx == 0 { len - 1 } else { idx - 1 }
}

#[inline]
fn next_i(idx: usize, len: usize) -> usize {
    (idx + 1) % len
}

struct Delaunay {
    verts: Vec<Vertex2>,
    /// `allVertices`: creation order, later sorted.
    all_vertices: Vec<Idx>,
    edges: Vec<Edge>,
    tris: Vec<[Idx; 3]>,
    pending_delaunay: Vec<Idx>,
    horz_edges: Vec<Idx>,
    loc_mins: Vec<Idx>,
    use_delaunay: bool,
    first_active: Idx,
}

impl Delaunay {
    fn new(use_delaunay: bool) -> Self {
        Delaunay {
            verts: Vec::new(),
            all_vertices: Vec::new(),
            edges: Vec::new(),
            tris: Vec::new(),
            pending_delaunay: Vec::new(),
            horz_edges: Vec::new(),
            loc_mins: Vec::new(),
            use_delaunay,
            first_active: NONE,
        }
    }

    #[inline]
    fn e(&self, i: Idx) -> &Edge {
        &self.edges[i as usize]
    }

    #[inline]
    fn em(&mut self, i: Idx) -> &mut Edge {
        &mut self.edges[i as usize]
    }

    #[inline]
    fn v(&self, i: Idx) -> &Vertex2 {
        &self.verts[i as usize]
    }

    #[inline]
    fn vm(&mut self, i: Idx) -> &mut Vertex2 {
        &mut self.verts[i as usize]
    }

    #[inline]
    fn pt(&self, v: Idx) -> Point64 {
        self.verts[v as usize].pt
    }

    fn new_vertex(&mut self, pt: Point64) -> Idx {
        let i = self.verts.len() as Idx;
        self.verts.push(Vertex2 {
            pt,
            edges: Vec::with_capacity(2),
            inner_lm: false,
        });
        self.all_vertices.push(i);
        i
    }

    #[inline]
    fn is_horizontal(&self, e: Idx) -> bool {
        self.pt(self.e(e).v_b).y == self.pt(self.e(e).v_t).y
    }

    fn edge_completed(&self, e: Idx) -> bool {
        let ed = self.e(e);
        if ed.tri_a == NONE {
            return false;
        }
        if ed.tri_b != NONE {
            return true;
        }
        ed.kind != EdgeKind::Loose
    }

    fn edge_contains(&self, e: Idx, v: Idx) -> EdgeContains {
        if self.e(e).v_l == v {
            EdgeContains::Left
        } else if self.e(e).v_r == v {
            EdgeContains::Right
        } else {
            EdgeContains::Neither
        }
    }

    fn remove_edge_from_vertex(&mut self, vert: Idx, edge: Idx) -> Result<(), Oops> {
        let edges = &mut self.vm(vert).edges;
        let pos = edges.iter().position(|&e| e == edge).ok_or(Oops)?;
        edges.remove(pos);
        Ok(())
    }

    fn find_linking_edge(&self, vert1: Idx, vert2: Idx, prefer_ascending: bool) -> Idx {
        let mut res = NONE;
        for &e in &self.v(vert1).edges {
            let ed = self.e(e);
            if ed.v_l == vert2 || ed.v_r == vert2 {
                if ed.kind == EdgeKind::Loose || ((ed.kind == EdgeKind::Ascend) == prefer_ascending)
                {
                    return e;
                }
                res = e;
            }
        }
        res
    }

    fn path_from_triangle(&self, tri: [Idx; 3]) -> Path64 {
        let e0 = self.e(tri[0]);
        let mut res = vec![self.pt(e0.v_l), self.pt(e0.v_r)];
        let e1 = self.e(tri[1]);
        if self.pt(e1.v_l) == res[0] || self.pt(e1.v_l) == res[1] {
            res.push(self.pt(e1.v_r));
        } else {
            res.push(self.pt(e1.v_l));
        }
        res
    }

    #[allow(clippy::needless_range_loop)] // indexes two parallel arrays, as upstream does
    fn force_legal(&mut self, edge: Idx) -> Result<(), Oops> {
        let ed = *self.e(edge);
        if ed.tri_a == NONE || ed.tri_b == NONE {
            return Ok(());
        }
        let mut vert_a = NONE;
        let mut vert_b = NONE;
        let mut edges_a = [NONE; 3];
        let mut edges_b = [NONE; 3];
        for i in 0..3 {
            let te = self.tris[ed.tri_a as usize][i];
            if te == edge {
                continue;
            }
            match self.edge_contains(te, ed.v_l) {
                EdgeContains::Left => {
                    edges_a[1] = te;
                    vert_a = self.e(te).v_r;
                }
                EdgeContains::Right => {
                    edges_a[1] = te;
                    vert_a = self.e(te).v_l;
                }
                EdgeContains::Neither => edges_b[1] = te,
            }
        }
        for i in 0..3 {
            let te = self.tris[ed.tri_b as usize][i];
            if te == edge {
                continue;
            }
            match self.edge_contains(te, ed.v_l) {
                EdgeContains::Left => {
                    edges_a[2] = te;
                    vert_b = self.e(te).v_r;
                }
                EdgeContains::Right => {
                    edges_a[2] = te;
                    vert_b = self.e(te).v_l;
                }
                EdgeContains::Neither => edges_b[2] = te,
            }
        }
        if vert_a == NONE || vert_b == NONE {
            return Err(Oops);
        }
        let (pa, pl, pr, pb) = (
            self.pt(vert_a),
            self.pt(ed.v_l),
            self.pt(ed.v_r),
            self.pt(vert_b),
        );
        if cross_product_sign_t(pa, pl, pr) == 0 {
            return Ok(());
        }
        let ict = in_circle_test(pa, pl, pr, pb);
        if ict == 0.0 || right_turning(pa, pl, pr) == (ict < 0.0) {
            return Ok(()); // on or outside the circle
        }
        self.em(edge).v_l = vert_a;
        self.em(edge).v_r = vert_b;
        let (tri_a, tri_b) = (ed.tri_a, ed.tri_b);
        self.tris[tri_a as usize][0] = edge;
        for i in 1..3 {
            let ea = edges_a[i];
            self.tris[tri_a as usize][i] = ea;
            if ea == NONE {
                return Err(Oops);
            }
            if self.e(ea).kind == EdgeKind::Loose {
                self.pending_delaunay.push(ea);
            }
            if self.e(ea).tri_a == tri_a || self.e(ea).tri_b == tri_a {
                continue;
            }
            if self.e(ea).tri_a == tri_b {
                self.em(ea).tri_a = tri_a;
            } else if self.e(ea).tri_b == tri_b {
                self.em(ea).tri_b = tri_a;
            } else {
                return Err(Oops);
            }
        }
        self.tris[tri_b as usize][0] = edge;
        for i in 1..3 {
            let eb = edges_b[i];
            self.tris[tri_b as usize][i] = eb;
            if eb == NONE {
                return Err(Oops);
            }
            if self.e(eb).kind == EdgeKind::Loose {
                self.pending_delaunay.push(eb);
            }
            if self.e(eb).tri_a == tri_b || self.e(eb).tri_b == tri_b {
                continue;
            }
            if self.e(eb).tri_a == tri_a {
                self.em(eb).tri_a = tri_b;
            } else if self.e(eb).tri_b == tri_a {
                self.em(eb).tri_b = tri_b;
            } else {
                return Err(Oops);
            }
        }
        Ok(())
    }

    fn create_edge(&mut self, v1: Idx, v2: Idx, k: EdgeKind) -> Idx {
        let (p1, p2) = (self.pt(v1), self.pt(v2));
        let (v_b, v_t) = if p1.y == p2.y {
            (v1, v2)
        } else if p1.y < p2.y {
            (v2, v1)
        } else {
            (v1, v2)
        };
        let (v_l, v_r) = if p1.x <= p2.x { (v1, v2) } else { (v2, v1) };
        let res = self.edges.len() as Idx;
        self.edges.push(Edge {
            v_l,
            v_r,
            v_b,
            v_t,
            kind: k,
            tri_a: NONE,
            tri_b: NONE,
            is_active: false,
            next_e: NONE,
            prev_e: NONE,
        });
        self.vm(v1).edges.push(res);
        self.vm(v2).edges.push(res);
        if k == EdgeKind::Loose {
            self.pending_delaunay.push(res);
            self.add_edge_to_actives(res);
        }
        res
    }

    fn create_triangle(&mut self, e1: Idx, e2: Idx, e3: Idx) -> Result<Idx, Oops> {
        let res = self.tris.len() as Idx;
        self.tris.push([e1, e2, e3]);
        for e in [e1, e2, e3] {
            if self.e(e).tri_a != NONE {
                self.em(e).tri_b = res;
                self.remove_edge_from_actives(e)?;
            } else {
                self.em(e).tri_a = res;
                if self.e(e).kind != EdgeKind::Loose {
                    self.remove_edge_from_actives(e)?;
                }
            }
        }
        Ok(res)
    }

    /// Checks every pair of edges that can meet: `None` if two of them cross (upstream instead
    /// moves an end point of one when they cross within a unit of it, which can leave the
    /// sweep inconsistent), otherwise the vertices that lie inside an edge, as (edge, vertex).
    fn scan_edges(&self) -> Option<Vec<(Idx, Idx)>> {
        // edges of ignored paths are no longer attached to a vertex
        let mut boxes: Vec<(i64, i64, i64, i64, Idx)> = (0..self.edges.len() as Idx)
            .filter(|&e| self.v(self.e(e).v_b).edges.contains(&e))
            .map(|e| {
                let ed = self.e(e);
                let (l, r) = (self.pt(ed.v_l).x, self.pt(ed.v_r).x);
                (l, r, self.pt(ed.v_t).y, self.pt(ed.v_b).y, e)
            })
            .collect();
        boxes.sort_unstable();
        let mut touches = Vec::new();
        for (i, &(_, right, top, bottom, e1)) in boxes.iter().enumerate() {
            let (v1, w1) = (self.e(e1).v_b, self.e(e1).v_t);
            let (p1, q1) = (self.pt(v1), self.pt(w1));
            for &(left2, _, top2, bottom2, e2) in &boxes[i + 1..] {
                if left2 > right {
                    break;
                }
                if bottom2 < top || top2 > bottom {
                    continue;
                }
                let (v2, w2) = (self.e(e2).v_b, self.e(e2).v_t);
                let (p2, q2) = (self.pt(v2), self.pt(w2));
                if segments_cross(p1, q1, p2, q2) {
                    return None;
                }
                for (e, (s, t), v) in [
                    (e1, (p1, q1), v2),
                    (e1, (p1, q1), w2),
                    (e2, (p2, q2), v1),
                    (e2, (p2, q2), w1),
                ] {
                    if strictly_inside(self.pt(v), s, t) {
                        touches.push((e, v));
                    }
                }
            }
        }
        Some(touches)
    }

    fn split_edge(&mut self, long_e: Idx, short_e: Idx) -> Result<(), Oops> {
        let old_t = self.e(long_e).v_t;
        let new_t = self.e(short_e).v_t;
        self.remove_edge_from_vertex(old_t, long_e)?;
        self.em(long_e).v_t = new_t;
        if self.e(long_e).v_l == old_t {
            self.em(long_e).v_l = new_t;
        } else {
            self.em(long_e).v_r = new_t;
        }
        self.vm(new_t).edges.push(long_e);
        let kind = self.e(long_e).kind;
        self.create_edge(new_t, old_t, kind);
        Ok(())
    }

    /// Splits each edge at the vertices inside it. `add_path` drops collinear vertices, so a
    /// vertex that another path shares can end up in the middle of an edge, where the sweep
    /// would never connect to it.
    fn split_edges_at(&mut self, mut touches: Vec<(Idx, Idx)>) -> Result<(), Oops> {
        touches.sort_unstable_by_key(|&(e, v)| {
            (e, distance_sqr_exact(self.pt(self.e(e).v_b), self.pt(v)))
        });
        touches.dedup();
        // from the bottom up, each split leaves the rest of the edge as a new edge
        for same_edge in touches.chunk_by(|a, b| a.0 == b.0) {
            let mut rest = same_edge[0].0;
            for &(_, v) in same_edge {
                self.split_edge_at(rest, v)?;
                rest = (self.edges.len() - 1) as Idx;
            }
        }
        Ok(())
    }

    /// Removes pairs of edges between the same two vertices: a path that runs back along
    /// itself, or two paths that share an edge, once collinear vertices are split. Such a
    /// zero-width slit bounds no area, and the sweep cannot handle coincident edges.
    fn remove_slits(&mut self) -> Result<(), Oops> {
        for vi in 0..self.all_vertices.len() {
            let v = self.all_vertices[vi];
            let (mut i, mut lost_edges) = (0, false);
            while i < self.v(v).edges.len() {
                let e1 = self.v(v).edges[i];
                let other = |s: &Self, e: Idx| {
                    if s.e(e).v_b == v {
                        s.e(e).v_t
                    } else {
                        s.e(e).v_b
                    }
                };
                let w = other(self, e1);
                let twin = self.v(v).edges[i + 1..]
                    .iter()
                    .copied()
                    .find(|&e2| other(self, e2) == w);
                match twin {
                    Some(e2) => {
                        for e in [e1, e2] {
                            self.remove_edge_from_vertex(v, e)?;
                            self.remove_edge_from_vertex(w, e)?;
                        }
                        self.reclassify_minimum(w);
                        lost_edges = true;
                    }
                    None => i += 1,
                }
            }
            if lost_edges {
                self.reclassify_minimum(v);
            }
        }
        Ok(())
    }

    /// Recomputes whether `v` is an inner local minimum after it lost edges.
    fn reclassify_minimum(&mut self, v: Idx) {
        let p = self.pt(v);
        let edges = &self.v(v).edges;
        let top = |e: Idx| {
            if self.e(e).v_b == v {
                self.e(e).v_t
            } else {
                self.e(e).v_b
            }
        };
        if edges.iter().any(|&e| self.pt(top(e)).y > p.y) {
            self.vm(v).inner_lm = false; // an edge goes down
        } else if let [e1, e2] = edges[..] {
            // as in add_path: the path arrives on the descending edge and leaves on the
            // ascending one
            let (descend, ascend) = match (self.e(e1).kind, self.e(e2).kind) {
                (EdgeKind::Descend, EdgeKind::Ascend) => (e1, e2),
                (EdgeKind::Ascend, EdgeKind::Descend) => (e2, e1),
                _ => return,
            };
            let inner = left_turning(self.pt(top(descend)), p, self.pt(top(ascend)));
            self.vm(v).inner_lm = inner;
        }
    }

    /// Shortens `e` to end at `v` and adds an edge of the same kind from `v` to its old top.
    fn split_edge_at(&mut self, e: Idx, v: Idx) -> Result<(), Oops> {
        let old_t = self.e(e).v_t;
        self.remove_edge_from_vertex(old_t, e)?;
        self.em(e).v_t = v;
        if self.e(e).v_l == old_t {
            self.em(e).v_l = v;
        } else {
            self.em(e).v_r = v;
        }
        self.vm(v).edges.push(e);
        // v now has an edge going down, so it is no local minimum (as upstream's
        // RemoveIntersection, #47)
        self.vm(v).inner_lm = false;
        let kind = self.e(e).kind;
        self.create_edge(v, old_t, kind);
        Ok(())
    }

    /// Whether the region reaches below `v1` once `v2`, at the same point, is merged into it.
    /// Upstream says so only if both are inner local minima. When both are local minima (no
    /// edge leaves downwards) and the edges of one enclose the other's, the enclosing one
    /// decides instead: a path in the notch of another, or a hole whose bottom is the outer
    /// path's bottom.
    fn merged_inner_lm(&self, v1: Idx, v2: Idx) -> bool {
        let (f1, f2) = (self.v(v1).inner_lm, self.v(v2).inner_lm);
        match (self.upward_span(v1), self.upward_span(v2)) {
            (Some((l1, r1)), Some((l2, r2))) => {
                let left_of = |a: Point64, b: Point64| upward_turn(b, a) >= 0;
                if left_of(l1, l2) && left_of(r2, r1) {
                    f1
                } else if left_of(l2, l1) && left_of(r1, r2) {
                    f2
                } else {
                    f1 && f2
                }
            }
            _ => f1 && f2,
        }
    }

    /// The leftmost and rightmost directions of `v`'s edges, if none of them leaves `v`
    /// downwards (towards larger y).
    fn upward_span(&self, v: Idx) -> Option<(Point64, Point64)> {
        let p = self.pt(v);
        let mut span: Option<(Point64, Point64)> = None;
        for &e in &self.v(v).edges {
            let other = if self.e(e).v_b == v {
                self.e(e).v_t
            } else {
                self.e(e).v_b
            };
            let q = self.pt(other);
            if q.y > p.y {
                return None;
            }
            let d = Point64::new(q.x - p.x, q.y - p.y);
            span = Some(match span {
                None => (d, d),
                Some((l, r)) => (
                    if upward_turn(l, d) > 0 { d } else { l },
                    if upward_turn(d, r) > 0 { d } else { r },
                ),
            });
        }
        span
    }

    fn merge_dup_or_collinear_vertices(&mut self) -> Result<(), Oops> {
        if self.all_vertices.is_empty() {
            return Ok(());
        }
        let mut i1 = 0;
        for i2 in 1..self.all_vertices.len() {
            let (v1, v2) = (self.all_vertices[i1], self.all_vertices[i2]);
            if self.pt(v1) != self.pt(v2) {
                i1 = i2;
                continue;
            }
            let inner = self.merged_inner_lm(v1, v2);
            let moved = std::mem::take(&mut self.vm(v2).edges);
            for &e in &moved {
                let ed = self.em(e);
                if ed.v_b == v2 {
                    ed.v_b = v1;
                } else {
                    ed.v_t = v1;
                }
                if ed.v_l == v2 {
                    ed.v_l = v1;
                } else {
                    ed.v_r = v1;
                }
            }
            self.vm(v1).edges.extend(moved);
            self.vm(v1).inner_lm = inner;
            let mut ie = 0;
            while ie < self.v(v1).edges.len() {
                let e1 = self.v(v1).edges[ie];
                if self.is_horizontal(e1) || self.e(e1).v_b != v1 {
                    ie += 1;
                    continue;
                }
                let mut ie2 = ie + 1;
                while ie2 < self.v(v1).edges.len() {
                    let e2 = self.v(v1).edges[ie2];
                    let (a, b) = (*self.e(e1), *self.e(e2));
                    if b.v_b != v1
                        || self.pt(a.v_t).y == self.pt(b.v_t).y
                        || cross_product_sign_t(self.pt(a.v_t), self.pt(v1), self.pt(b.v_t)) != 0
                    {
                        ie2 += 1;
                        continue;
                    }
                    if self.pt(a.v_t).y < self.pt(b.v_t).y {
                        self.split_edge(e1, e2)?;
                    } else {
                        self.split_edge(e2, e1)?;
                    }
                    break; // only two edges can be collinear
                }
                ie += 1;
            }
        }
        Ok(())
    }

    fn create_inner_loc_min_loose_edge(&mut self, v_above: Idx) -> Idx {
        if self.first_active == NONE {
            return NONE;
        }
        let pa = self.pt(v_above);
        let (x_above, y_above) = (pa.x, pa.y);
        let mut e = self.first_active;
        let mut e_below = NONE;
        let mut best_d = -1.0;
        while e != NONE {
            let ed = *self.e(e);
            let (pl, pr) = (self.pt(ed.v_l), self.pt(ed.v_r));
            if pl.x <= x_above
                && pr.x >= x_above
                && self.pt(ed.v_b).y >= y_above
                && ed.v_b != v_above
                && ed.v_t != v_above
                && !left_turning(pl, pa, pr)
            {
                let d = shortest_dist_from_segment(pa, pl, pr);
                if e_below == NONE || d < best_d {
                    e_below = e;
                    best_d = d;
                }
            }
            e = ed.next_e;
        }
        if e_below == NONE {
            return NONE;
        }
        let eb = *self.e(e_below);
        let mut v_best = if self.pt(eb.v_t).y <= y_above {
            eb.v_b
        } else {
            eb.v_t
        };
        let mut x_best = self.pt(v_best).x;
        let mut y_best = self.pt(v_best).y;
        e = self.first_active;
        if x_best < x_above {
            while e != NONE {
                let ed = *self.e(e);
                if self.pt(ed.v_r).x > x_best
                    && self.pt(ed.v_l).x < x_above
                    && self.pt(ed.v_b).y > y_above
                    && self.pt(ed.v_t).y < y_best
                    && segs_intersect(self.pt(ed.v_b), self.pt(ed.v_t), self.pt(v_best), pa)
                        == IntersectKind::Intersect
                {
                    v_best = if self.pt(ed.v_t).y > y_above {
                        ed.v_t
                    } else {
                        ed.v_b
                    };
                    x_best = self.pt(v_best).x;
                    y_best = self.pt(v_best).y;
                }
                e = ed.next_e;
            }
        } else {
            while e != NONE {
                let ed = *self.e(e);
                if self.pt(ed.v_r).x < x_best
                    && self.pt(ed.v_l).x > x_above
                    && self.pt(ed.v_b).y > y_above
                    && self.pt(ed.v_t).y < y_best
                    && segs_intersect(self.pt(ed.v_b), self.pt(ed.v_t), self.pt(v_best), pa)
                        == IntersectKind::Intersect
                {
                    v_best = if self.pt(ed.v_t).y > y_above {
                        ed.v_t
                    } else {
                        ed.v_b
                    };
                    x_best = self.pt(v_best).x;
                    y_best = self.pt(v_best).y;
                }
                e = ed.next_e;
            }
        }
        // Upstream's search skips edges by their bounding boxes and can return a vertex hidden
        // behind an edge; the loose edge then crosses it.
        if !self.is_visible(v_best, v_above) {
            v_best = self.nearest_visible_below(v_above);
            if v_best == NONE {
                return NONE;
            }
        }
        self.create_edge(v_best, v_above, EdgeKind::Loose)
    }

    /// Whether `v1` and `v2` share a row with a vertex strictly between them.
    fn row_vertex_between(&self, v1: Idx, v2: Idx) -> bool {
        let (p1, p2) = (self.pt(v1), self.pt(v2));
        if p1.y != p2.y {
            return false;
        }
        let (l, r) = (p1.x.min(p2.x), p1.x.max(p2.x));
        // all_vertices is sorted by descending y, then ascending x
        let start = self.all_vertices.partition_point(|&v| {
            let p = self.pt(v);
            p.y > p1.y || (p.y == p1.y && p.x <= l)
        });
        self.all_vertices[start..]
            .iter()
            .take_while(|&&v| self.pt(v).y == p1.y && self.pt(v).x < r)
            .any(|&v| !self.v(v).edges.is_empty())
    }

    /// Whether segment `v1`-`v2` crosses no active edge and passes through none of their
    /// vertices.
    fn is_visible(&self, v1: Idx, v2: Idx) -> bool {
        let (a, b) = (self.pt(v1), self.pt(v2));
        let mut e = self.first_active;
        while e != NONE {
            let ed = self.e(e);
            if segment_blocked(a, b, self.pt(ed.v_b), self.pt(ed.v_t)) {
                return false;
            }
            e = ed.next_e;
        }
        true
    }

    /// The nearest swept vertex of an active edge that `v` can see.
    fn nearest_visible_below(&self, v: Idx) -> Idx {
        let p = self.pt(v);
        let mut candidates = Vec::new();
        let mut e = self.first_active;
        while e != NONE {
            let ed = self.e(e);
            for c in [ed.v_b, ed.v_t] {
                if c != v && self.pt(c).y >= p.y {
                    candidates.push((distance_sqr_exact(p, self.pt(c)), c));
                }
            }
            e = ed.next_e;
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates
            .into_iter()
            .map(|(_, c)| c)
            .find(|&c| self.is_visible(c, v))
            .unwrap_or(NONE)
    }

    fn horizontal_between(&self, v1: Idx, v2: Idx) -> Idx {
        let (p1, p2) = (self.pt(v1), self.pt(v2));
        let y = p1.y;
        let (l, r) = if p1.x > p2.x {
            (p2.x, p1.x)
        } else {
            (p1.x, p2.x)
        };
        let mut res = self.first_active;
        while res != NONE {
            let ed = self.e(res);
            let (pl, pr) = (self.pt(ed.v_l), self.pt(ed.v_r));
            if pl.y == y && pr.y == y && pl.x >= l && pr.x <= r && (pl.x != l || pl.x != r) {
                break;
            }
            res = ed.next_e;
        }
        res
    }

    /// `DoTriangulateLeft`; upstream's tail recursion is a loop here.
    fn do_triangulate_left(
        &mut self,
        mut edge: Idx,
        mut pivot: Idx,
        min_y: i64,
    ) -> Result<(), Oops> {
        loop {
            let mut v_alt = NONE;
            let mut e_alt = NONE;
            let v = if self.e(edge).v_b == pivot {
                self.e(edge).v_t
            } else {
                self.e(edge).v_b
            };
            for k in 0..self.v(pivot).edges.len() {
                let e = self.v(pivot).edges[k];
                if e == edge || !self.e(e).is_active {
                    continue;
                }
                let vx = if self.e(e).v_t == pivot {
                    self.e(e).v_b
                } else {
                    self.e(e).v_t
                };
                if vx == v {
                    continue;
                }
                let (pv, pp, px) = (self.pt(v), self.pt(pivot), self.pt(vx));
                let cps = cross_product_sign_t(pv, pp, px);
                if cps == 0 {
                    if (pv.x > pp.x) == (pp.x > px.x) {
                        continue;
                    }
                } else if cps > 0 || (v_alt != NONE && !left_turning(px, pp, self.pt(v_alt))) {
                    continue;
                }
                v_alt = vx;
                e_alt = e;
            }
            if v_alt == NONE || self.pt(v_alt).y < min_y {
                return Ok(());
            }
            let (pa, pp) = (self.pt(v_alt), self.pt(pivot));
            if pa.y < pp.y {
                if self.e(e_alt).kind == EdgeKind::Ascend {
                    return Ok(());
                }
            } else if pa.y > pp.y && self.e(e_alt).kind == EdgeKind::Descend {
                return Ok(());
            }
            if self.row_vertex_between(v_alt, v) {
                return Ok(()); // the edge would pass through it
            }
            let mut e_x = self.find_linking_edge(v_alt, v, pa.y < self.pt(v).y);
            if e_x == NONE {
                if pa.y == self.pt(v).y
                    && self.pt(v).y == min_y
                    && self.horizontal_between(v_alt, v) != NONE
                {
                    return Ok(());
                }
                e_x = self.create_edge(v_alt, v, EdgeKind::Loose);
            }
            self.create_triangle(edge, e_alt, e_x)?;
            if self.edge_completed(e_x) {
                return Ok(());
            }
            edge = e_x;
            pivot = v_alt;
        }
    }

    /// `DoTriangulateRight`; upstream's tail recursion is a loop here.
    fn do_triangulate_right(
        &mut self,
        mut edge: Idx,
        mut pivot: Idx,
        min_y: i64,
    ) -> Result<(), Oops> {
        loop {
            let mut v_alt = NONE;
            let mut e_alt = NONE;
            let v = if self.e(edge).v_b == pivot {
                self.e(edge).v_t
            } else {
                self.e(edge).v_b
            };
            for k in 0..self.v(pivot).edges.len() {
                let e = self.v(pivot).edges[k];
                if e == edge || !self.e(e).is_active {
                    continue;
                }
                let vx = if self.e(e).v_t == pivot {
                    self.e(e).v_b
                } else {
                    self.e(e).v_t
                };
                if vx == v {
                    continue;
                }
                let (pv, pp, px) = (self.pt(v), self.pt(pivot), self.pt(vx));
                let cps = cross_product_sign_t(pv, pp, px);
                if cps == 0 {
                    if (pv.x > pp.x) == (pp.x > px.x) {
                        continue;
                    }
                } else if cps < 0 || (v_alt != NONE && !right_turning(px, pp, self.pt(v_alt))) {
                    continue;
                }
                v_alt = vx;
                e_alt = e;
            }
            if v_alt == NONE || self.pt(v_alt).y < min_y {
                return Ok(());
            }
            let (pa, pp) = (self.pt(v_alt), self.pt(pivot));
            if pa.y < pp.y {
                if self.e(e_alt).kind == EdgeKind::Descend {
                    return Ok(());
                }
            } else if pa.y > pp.y && self.e(e_alt).kind == EdgeKind::Ascend {
                return Ok(());
            }
            if self.row_vertex_between(v_alt, v) {
                return Ok(()); // the edge would pass through it
            }
            let mut e_x = self.find_linking_edge(v_alt, v, pa.y > self.pt(v).y);
            if e_x == NONE {
                if pa.y == self.pt(v).y
                    && self.pt(v).y == min_y
                    && self.horizontal_between(v_alt, v) != NONE
                {
                    return Ok(());
                }
                e_x = self.create_edge(v_alt, v, EdgeKind::Loose);
            }
            self.create_triangle(edge, e_x, e_alt)?;
            if self.edge_completed(e_x) {
                return Ok(());
            }
            edge = e_x;
            pivot = v_alt;
        }
    }

    fn add_edge_to_actives(&mut self, edge: Idx) {
        if self.e(edge).is_active {
            return;
        }
        let first = self.first_active;
        let ed = self.em(edge);
        ed.prev_e = NONE;
        ed.next_e = first;
        ed.is_active = true;
        if first != NONE {
            self.em(first).prev_e = edge;
        }
        self.first_active = edge;
    }

    fn remove_edge_from_actives(&mut self, edge: Idx) -> Result<(), Oops> {
        let (vb, vt) = (self.e(edge).v_b, self.e(edge).v_t);
        self.remove_edge_from_vertex(vb, edge)?;
        self.remove_edge_from_vertex(vt, edge)?;
        let (prev, next) = (self.e(edge).prev_e, self.e(edge).next_e);
        if next != NONE {
            self.em(next).prev_e = prev;
        }
        if prev != NONE {
            self.em(prev).next_e = next;
        }
        self.em(edge).is_active = false;
        if self.first_active == edge {
            self.first_active = next;
        }
        Ok(())
    }

    fn add_path(&mut self, path: &Path64) -> Result<(), TriangulateError> {
        let len = path.len();
        let mut i0 = 0;
        if !find_loc_min_idx(path, len, &mut i0) {
            return Ok(());
        }
        let mut i_prev = prev_i(i0, len);
        while path[i_prev] == path[i0] {
            i_prev = prev_i(i_prev, len);
        }
        let mut i_next = next_i(i0, len);
        let mut i = i0;
        while cross_product_sign_t(path[i_prev], path[i], path[i_next]) == 0 {
            find_loc_min_idx(path, len, &mut i);
            if i == i0 {
                return Ok(()); // entirely collinear
            }
            i_prev = prev_i(i, len);
            while path[i_prev] == path[i] {
                i_prev = prev_i(i_prev, len);
            }
            i_next = next_i(i, len);
        }
        let vert_cnt = self.all_vertices.len();
        let v0 = self.new_vertex(path[i]);
        if left_turning(path[i_prev], path[i], path[i_next]) {
            self.vm(v0).inner_lm = true;
        }
        let mut v_prev = v0;
        // A valid walk is one lap: `i` advances `len` times, from i0 back to i0. Upstream never
        // checks i0 inside the collinear skips, so on some self-intersecting paths it overshoots
        // and walks (adding vertices) forever.
        let mut steps = 0;
        macro_rules! advance {
            () => {
                i = i_next;
                steps += 1;
                if steps > len {
                    return Err(TriangulateError::Degenerate);
                }
            };
        }
        advance!();
        loop {
            self.loc_mins.push(v_prev);
            i_next = next_i(i, len);
            if cross_product_sign_t(self.pt(v_prev), path[i], path[i_next]) == 0 {
                advance!();
                continue;
            }
            while path[i].y <= self.pt(v_prev).y {
                let v = self.new_vertex(path[i]);
                self.create_edge(v_prev, v, EdgeKind::Ascend);
                v_prev = v;
                advance!();
                i_next = next_i(i, len);
                while cross_product_sign_t(self.pt(v_prev), path[i], path[i_next]) == 0 {
                    advance!();
                    i_next = next_i(i, len);
                }
            }
            let mut v_prev_prev = v_prev;
            while i != i0 && path[i].y >= self.pt(v_prev).y {
                let v = self.new_vertex(path[i]);
                self.create_edge(v, v_prev, EdgeKind::Descend);
                v_prev_prev = v_prev;
                v_prev = v;
                advance!();
                i_next = next_i(i, len);
                while cross_product_sign_t(self.pt(v_prev), path[i], path[i_next]) == 0 {
                    advance!();
                    i_next = next_i(i, len);
                }
            }
            if i == i0 {
                break;
            }
            if left_turning(self.pt(v_prev_prev), self.pt(v_prev), path[i]) {
                self.vm(v_prev).inner_lm = true;
            }
        }
        self.create_edge(v0, v_prev, EdgeKind::Descend);
        // Upstream also drops triangles with a side of length 1 or less, although their area
        // is real.
        if self.all_vertices.len() - vert_cnt < 3 {
            for j in vert_cnt..self.all_vertices.len() {
                let v = self.all_vertices[j];
                self.vm(v).edges.clear(); // flag to ignore
            }
        }
        Ok(())
    }

    fn add_paths(&mut self, paths: &Paths64) -> Result<bool, TriangulateError> {
        let total: usize = paths.iter().map(|p| p.len()).sum();
        if total == 0 {
            return Ok(false);
        }
        for path in paths {
            self.add_path(path)?;
        }
        Ok(self.all_vertices.len() > 2)
    }

    fn execute(&mut self, paths: &Paths64, clockwise: bool) -> Result<Paths64, TriangulateError> {
        if !self.add_paths(paths)? {
            return Err(TriangulateError::NoPolygons);
        }
        self.run(clockwise).map_err(|_| TriangulateError::Fail)?
    }

    /// `clockwise`: the input's total area is negative. Upstream decides this from the lowest
    /// local minimum, which can belong to a hole, and flips a minimum that add_path queued
    /// twice (after a collinear vertex) back again.
    fn run(&mut self, clockwise: bool) -> Result<Result<Paths64, TriangulateError>, Oops> {
        if clockwise {
            self.loc_mins.sort_unstable();
            self.loc_mins.dedup();
            while let Some(lm) = self.loc_mins.pop() {
                let v = self.vm(lm);
                v.inner_lm = !v.inner_lm;
            }
            for e in &mut self.edges {
                e.kind = if e.kind == EdgeKind::Ascend {
                    EdgeKind::Descend
                } else {
                    EdgeKind::Ascend
                };
            }
        } else {
            self.loc_mins.clear();
        }

        let verts = &self.verts;
        self.all_vertices.sort_by(|&a, &b| {
            let (pa, pb) = (verts[a as usize].pt, verts[b as usize].pt);
            if pa.y == pb.y {
                pa.x.cmp(&pb.x)
            } else {
                pb.y.cmp(&pa.y)
            }
        });
        self.merge_dup_or_collinear_vertices()?;
        let Some(touches) = self.scan_edges() else {
            return Ok(Err(TriangulateError::PathsIntersect));
        };
        self.split_edges_at(touches)?;
        self.remove_slits()?;

        let mut curr_y = self.pt(self.all_vertices[0]).y;
        for vi in 0..self.all_vertices.len() {
            let v = self.all_vertices[vi];
            if self.v(v).edges.is_empty() {
                continue;
            }
            if self.pt(v).y != curr_y {
                while let Some(lm) = self.loc_mins.pop() {
                    let e = self.create_inner_loc_min_loose_edge(lm);
                    if e == NONE {
                        return Ok(Err(TriangulateError::Fail));
                    }
                    let vb = self.e(e).v_b;
                    if self.is_horizontal(e) {
                        if self.e(e).v_l == vb {
                            self.do_triangulate_left(e, vb, curr_y)?;
                        } else {
                            self.do_triangulate_right(e, vb, curr_y)?;
                        }
                    } else {
                        self.do_triangulate_left(e, vb, curr_y)?;
                        if !self.edge_completed(e) {
                            self.do_triangulate_right(e, vb, curr_y)?;
                        }
                    }
                    // a merged vertex can start more than two edges
                    for k in 0..self.v(lm).edges.len() {
                        let e = self.v(lm).edges[k];
                        if self.e(e).kind != EdgeKind::Loose && self.e(e).v_b == lm {
                            self.add_edge_to_actives(e);
                        }
                    }
                }
                while let Some(e) = self.horz_edges.pop() {
                    if self.edge_completed(e) {
                        continue;
                    }
                    let vb = self.e(e).v_b;
                    if vb == self.e(e).v_l {
                        // #45
                        if self.e(e).kind == EdgeKind::Ascend {
                            self.do_triangulate_left(e, vb, curr_y)?;
                        }
                    } else if self.e(e).kind == EdgeKind::Descend {
                        self.do_triangulate_right(e, vb, curr_y)?;
                    }
                }
                curr_y = self.pt(v).y;
            }

            let mut i = self.v(v).edges.len() as isize - 1;
            while i >= 0 {
                if i as usize >= self.v(v).edges.len() {
                    i -= 1;
                    continue;
                }
                let e = self.v(v).edges[i as usize];
                i -= 1;
                if self.edge_completed(e) || self.e(e).kind == EdgeKind::Loose {
                    continue;
                }
                if v == self.e(e).v_b {
                    if self.is_horizontal(e) {
                        self.horz_edges.push(e);
                    }
                    if !self.v(v).inner_lm {
                        self.add_edge_to_actives(e);
                    }
                } else if self.is_horizontal(e) {
                    self.horz_edges.push(e);
                } else {
                    let vb = self.e(e).v_b;
                    let y = self.pt(v).y;
                    if self.e(e).kind == EdgeKind::Ascend {
                        self.do_triangulate_left(e, vb, y)?;
                    } else {
                        self.do_triangulate_right(e, vb, y)?;
                    }
                }
            }
            if self.v(v).inner_lm {
                self.loc_mins.push(v);
            }
        }
        while let Some(e) = self.horz_edges.pop() {
            if !self.edge_completed(e) && self.e(e).v_b == self.e(e).v_l {
                let vb = self.e(e).v_b;
                self.do_triangulate_left(e, vb, curr_y)?;
            }
        }

        if self.use_delaunay {
            while let Some(e) = self.pending_delaunay.pop() {
                self.force_legal(e)?;
            }
        }

        let mut res = Vec::with_capacity(self.tris.len());
        for t in 0..self.tris.len() {
            let mut p = self.path_from_triangle(self.tris[t]);
            let cps = cross_product_sign_t(p[0], p[1], p[2]);
            if cps == 0 {
                continue; // empty triangle
            }
            if cps < 0 {
                p.reverse();
            }
            res.push(p);
        }
        Ok(Ok(res))
    }
}

/// Triangulates non-intersecting polygons (holes allowed). With `use_delaunay` the result
/// is a constrained Delaunay triangulation.
///
/// The result always covers the input exactly; where the sweep cannot produce such a result
/// (on some degenerate input), the error is [`TriangulateError::Fail`]. Crossing edges are
/// [`TriangulateError::PathsIntersect`].
pub fn triangulate(paths: &Paths64, use_delaunay: bool) -> Result<Paths64, TriangulateError> {
    check_range(paths).map_err(|_| TriangulateError::CoordinateOutOfRange)?;
    let clockwise = total_area_sign(paths) < 0;
    let tris = Delaunay::new(use_delaunay).execute(paths, clockwise)?;
    // The sweep is not proven correct on every degenerate input (touching or overlapping
    // edges, pinched paths); never return a result that does not cover the input exactly.
    if !covers_exactly(paths, &tris, clockwise) {
        return Err(TriangulateError::Fail);
    }
    Ok(tris)
}

/// Whether the counter-clockwise `tris` cover the region of `paths` exactly: their boundary,
/// with shared edges cancelled, equals the paths' boundary (oriented like the triangles), so
/// their coverage equals the paths' winding number everywhere (exact).
fn covers_exactly(paths: &Paths64, tris: &Paths64, clockwise: bool) -> bool {
    let input_weight = if clockwise { -1 } else { 1 };
    // Most edges cancel exactly, against their twin or the input edge they lie on, keyed by
    // their ends in lexicographic order.
    let edge_count = paths.iter().map(Vec::len).sum::<usize>() + 3 * tris.len();
    type Ends = ((i64, i64), (i64, i64));
    let mut segments: FxMap<Ends, i32> =
        FxMap::with_capacity_and_hasher(edge_count, Default::default());
    let mut add = |a: Point64, b: Point64, weight: i32| {
        let (ka, kb) = ((a.x, a.y), (b.x, b.y));
        if ka < kb {
            *segments.entry((ka, kb)).or_default() += weight;
        } else if kb < ka {
            *segments.entry((kb, ka)).or_default() -= weight;
        }
    };
    for p in paths {
        for i in 0..p.len() {
            add(p[i], p[(i + 1) % p.len()], input_weight);
        }
    }
    for t in tris {
        for i in 0..3 {
            add(t[i], t[(i + 1) % 3], -1);
        }
    }
    // The rest runs along input edges that the triangles split or join: per supporting line
    // (reduced direction, offset), the signed coverage along it must cancel.
    let mut lines: FxMap<(i64, i64, i128), Vec<(i128, i32)>> = FxMap::default();
    for (&((ax, ay), (bx, by)), &weight) in &segments {
        if weight == 0 {
            continue;
        }
        // a precedes b, so the reduced direction already points the canonical way
        let (dx, dy) = (bx - ax, by - ay);
        let g = gcd(dx.unsigned_abs(), dy.unsigned_abs()) as i64;
        let (ux, uy) = (dx / g, dy / g);
        let offset = ux as i128 * ay as i128 - uy as i128 * ax as i128;
        let along = |x: i64, y: i64| ux as i128 * x as i128 + uy as i128 * y as i128;
        let events = lines.entry((ux, uy, offset)).or_default();
        events.push((along(ax, ay), weight));
        events.push((along(bx, by), -weight));
    }
    lines.into_values().all(|mut events| {
        events.sort_unstable();
        let mut sum = 0;
        events.chunk_by(|a, b| a.0 == b.0).all(|same_t| {
            sum += same_t.iter().map(|e| e.1).sum::<i32>();
            sum == 0
        })
    })
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// rustc's FxHash: much faster than the default hasher for integer keys. It is not
/// collision-resistant, so crafted coordinates can make the check slow, never wrong.
#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }

    fn write_i128(&mut self, n: i128) {
        self.write_u64(n as u64);
        self.write_u64((n >> 64) as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// The sign of the paths' total area: exact while the sum fits in an i128 (one edge's term
/// always does for coordinates in range), otherwise in floating point, which only has to get
/// the sign of a huge total right.
fn total_area_sign(paths: &Paths64) -> i32 {
    let terms = || {
        paths.iter().flat_map(|p| {
            (0..p.len()).map(move |i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a.x as i128 * b.y as i128 - b.x as i128 * a.y as i128
            })
        })
    };
    match terms().try_fold(0i128, i128::checked_add) {
        Some(sum) => sum.signum() as i32,
        None if terms().map(|term| term as f64).sum::<f64>() < 0.0 => -1,
        None => 1,
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Floating-point [`triangulate`]; coordinates are scaled by `10^decimal_places` (clamped to `0..=8`).
pub fn triangulate_d(
    paths: &PathsD,
    decimal_places: i32,
    use_delaunay: bool,
) -> Result<PathsD, TriangulateError> {
    let scale = if decimal_places <= 0 {
        1.0
    } else if decimal_places > 8 {
        10f64.powi(8)
    } else {
        10f64.powi(decimal_places)
    };
    let pp64: Paths64 =
        scale_paths(paths, scale, scale).map_err(|_| TriangulateError::CoordinateOutOfRange)?;
    let sol = triangulate(&pp64, use_delaunay)?;
    scale_paths(&sol, 1.0 / scale, 1.0 / scale).map_err(|_| TriangulateError::CoordinateOutOfRange)
}
