//! Research prototype: statement-by-statement port of KiCad's bundled
//! `thirdparty/delaunator/delaunator.cpp` / `delaunator.hpp`
//! (abellgithub/delaunator-cpp, MIT) to safe Rust.
//!
//! All index arithmetic that C++ does in `std::size_t` (64 bit on the reference platform) is done
//! in `u64` here so that the result is the same on wasm32 (32-bit `usize`).

use crate::stdsort::std_sort;

pub const INVALID_INDEX: usize = usize::MAX;

#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn magnitude2(self) -> f64 {
        self.x * self.x + self.y * self.y
    }
    fn determinant(p1: Point, p2: Point) -> f64 {
        p1.x * p2.y - p1.y * p2.x
    }
    fn vector(p1: Point, p2: Point) -> Point {
        Point { x: p2.x - p1.x, y: p2.y - p1.y }
    }
    fn dist2(p1: Point, p2: Point) -> f64 {
        let vec = Point::vector(p1, p2);
        vec.x * vec.x + vec.y * vec.y
    }
    fn equal(p1: Point, p2: Point, span: f64) -> bool {
        let dist = Point::dist2(p1, p2) / span;
        dist < 1e-20
    }
}

#[derive(Debug)]
pub enum DelaunatorError {
    /// C++ `throw std::runtime_error("not triangulation")`
    NotTriangulation,
    /// C++ `throw std::runtime_error("Cannot link edge")`
    CannotLinkEdge,
    /// C++ would index out of bounds / hit an `assert` (undefined behaviour in release builds).
    HullCorrupt,
}

#[derive(Debug, Default)]
pub struct Delaunator {
    pub triangles: Vec<usize>,
    pub halfedges: Vec<usize>,
    pub hull_prev: Vec<usize>,
    pub hull_next: Vec<usize>,
    pub hull_tri: Vec<usize>,
    pub hull_start: usize,
}

struct State<'a> {
    coords: &'a [f64],
    triangles: Vec<usize>,
    halfedges: Vec<usize>,
    hull_prev: Vec<usize>,
    hull_next: Vec<usize>,
    hull_tri: Vec<usize>,
    hull_start: usize,
    hash: Vec<usize>,
    center: Point,
    hash_size: usize,
    edge_stack: Vec<usize>,
}

/// `inline size_t fast_mod(const size_t i, const size_t c)`
fn fast_mod(i: u64, c: u64) -> u64 {
    if i >= c { i % c } else { i }
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// `inline double circumradius(const Point& p1, const Point& p2, const Point& p3)`
fn circumradius(p1: Point, p2: Point, p3: Point) -> f64 {
    let d = Point::vector(p1, p2);
    let e = Point::vector(p1, p3);

    let bl = d.magnitude2();
    let cl = e.magnitude2();
    let det = Point::determinant(d, e);

    let radius = Point {
        x: (e.y * bl - d.y * cl) * 0.5 / det,
        y: (d.x * cl - e.x * bl) * 0.5 / det,
    };

    if (bl > 0.0 || bl < 0.0) && (cl > 0.0 || cl < 0.0) && (det > 0.0 || det < 0.0) {
        return radius.magnitude2();
    }
    f64::MAX
}

/// `inline bool counterclockwise(const Point& p0, const Point& p1, const Point& p2)`
fn counterclockwise_pts(p0: Point, p1: Point, p2: Point) -> bool {
    let v0 = Point::vector(p0, p1);
    let v1 = Point::vector(p0, p2);
    let det = Point::determinant(v0, v1);
    let dist = v0.magnitude2() + v1.magnitude2();
    if det == 0.0 {
        return false;
    }
    let reldet = (dist / det).abs();
    if reldet > 1e14 {
        return false;
    }
    det > 0.0
}

fn counterclockwise(px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64) -> bool {
    counterclockwise_pts(Point { x: px, y: py }, Point { x: qx, y: qy }, Point { x: rx, y: ry })
}

/// `inline Point circumcenter(ax, ay, bx, by, cx, cy)`
fn circumcenter(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> Point {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;

    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;
    let d = dx * ey - dy * ex;

    let x = ax + (ey * bl - dy * cl) * 0.5 / d;
    let y = ay + (dx * cl - ex * bl) * 0.5 / d;

    Point { x, y }
}

#[allow(clippy::too_many_arguments)]
fn in_circle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    let dx = ax - px;
    let dy = ay - py;
    let ex = bx - px;
    let ey = by - py;
    let fx = cx - px;
    let fy = cy - py;

    let ap = dx * dx + dy * dy;
    let bp = ex * ex + ey * ey;
    let cp = fx * fx + fy * fy;

    (dx * (ey * cp - bp * fy) - dy * (ex * cp - bp * fx) + ap * (ex * fy - ey * fx)) < 0.0
}

const EPSILON: f64 = f64::EPSILON;

fn check_pts_equal(x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    (x1 - x2).abs() <= EPSILON && (y1 - y2).abs() <= EPSILON
}

/// monotonically increases with real angle, but doesn't need expensive trigonometry
fn pseudo_angle(dx: f64, dy: f64) -> f64 {
    let p = dx / (dx.abs() + dy.abs());
    (if dy > 0.0 { 3.0 - p } else { 1.0 + p }) / 4.0 // [0..1)
}

impl<'a> State<'a> {
    fn pt(&self, i: usize) -> Point {
        Point { x: self.coords[2 * i], y: self.coords[2 * i + 1] }
    }

    /// `std::size_t Delaunator::hash_key(const double x, const double y) const`
    fn hash_key(&self, x: f64, y: f64) -> usize {
        let dx = x - self.center.x;
        let dy = y - self.center.y;
        let v = (pseudo_angle(dx, dy) * self.hash_size as f64).floor();
        // `static_cast<std::size_t>( std::llround( v ) )`: glibc/x86-64 returns LLONG_MIN for NaN
        // (dx == dy == 0), which becomes 2^63 as size_t.
        let ll: i64 = if v.is_nan() { i64::MIN } else { v as i64 };
        fast_mod(ll as u64, self.hash_size as u64) as usize
    }

    /// `void Delaunator::link(const std::size_t a, const std::size_t b)`
    fn link(&mut self, a: usize, b: usize) -> Result<(), DelaunatorError> {
        let s = self.halfedges.len();
        if a == s {
            self.halfedges.push(b);
        } else if a < s {
            self.halfedges[a] = b;
        } else {
            return Err(DelaunatorError::CannotLinkEdge);
        }
        if b != INVALID_INDEX {
            let s2 = self.halfedges.len();
            if b == s2 {
                self.halfedges.push(a);
            } else if b < s2 {
                self.halfedges[b] = a;
            } else {
                return Err(DelaunatorError::CannotLinkEdge);
            }
        }
        Ok(())
    }

    /// `std::size_t Delaunator::add_triangle(i0, i1, i2, a, b, c)`
    fn add_triangle(
        &mut self,
        i0: usize,
        i1: usize,
        i2: usize,
        a: usize,
        b: usize,
        c: usize,
    ) -> Result<usize, DelaunatorError> {
        let t = self.triangles.len();
        self.triangles.push(i0);
        self.triangles.push(i1);
        self.triangles.push(i2);
        self.link(t, a)?;
        self.link(t + 1, b)?;
        self.link(t + 2, c)?;
        Ok(t)
    }

    /// `std::size_t Delaunator::legalize(std::size_t a)`
    fn legalize(&mut self, mut a: usize) -> Result<usize, DelaunatorError> {
        let mut i: usize = 0;
        #[allow(unused_assignments)]
        let mut ar: usize = 0;
        self.edge_stack.clear();

        // recursion eliminated with a fixed-size stack
        loop {
            let b = self.halfedges[a];

            let a0 = 3 * (a / 3);
            ar = a0 + (a + 2) % 3;

            if b == INVALID_INDEX {
                if i > 0 {
                    i -= 1;
                    a = self.edge_stack[i];
                    continue;
                } else {
                    break;
                }
            }

            let b0 = 3 * (b / 3);
            let al = a0 + (a + 1) % 3;
            let bl = b0 + (b + 2) % 3;

            let p0 = self.triangles[ar];
            let pr = self.triangles[a];
            let pl = self.triangles[al];
            let p1 = self.triangles[bl];

            let c = self.coords;
            let illegal = in_circle(
                c[2 * p0],
                c[2 * p0 + 1],
                c[2 * pr],
                c[2 * pr + 1],
                c[2 * pl],
                c[2 * pl + 1],
                c[2 * p1],
                c[2 * p1 + 1],
            );

            if illegal {
                self.triangles[a] = p1;
                self.triangles[b] = p0;

                let hbl = self.halfedges[bl];

                // Edge swapped on the other side of the hull (rare).
                // Fix the halfedge reference
                if hbl == INVALID_INDEX {
                    let mut e = self.hull_start;
                    loop {
                        if self.hull_tri[e] == bl {
                            self.hull_tri[e] = a;
                            break;
                        }
                        e = self.hull_prev[e];
                        if e == self.hull_start {
                            break;
                        }
                    }
                }
                self.link(a, hbl)?;
                let har = self.halfedges[ar];
                self.link(b, har)?;
                self.link(ar, bl)?;
                let br = b0 + (b + 1) % 3;

                if i < self.edge_stack.len() {
                    self.edge_stack[i] = br;
                } else {
                    self.edge_stack.push(br);
                }
                i += 1;
            } else if i > 0 {
                i -= 1;
                a = self.edge_stack[i];
                continue;
            } else {
                break;
            }
        }
        Ok(ar)
    }
}

impl Delaunator {
    /// `Delaunator::Delaunator(std::vector<double> const& in_coords)`
    pub fn new(coords: &[f64]) -> Result<Delaunator, DelaunatorError> {
        let n = coords.len() >> 1;

        let mut ids: Vec<usize> = (0..n).collect();

        let pt = |i: usize| Point { x: coords[2 * i], y: coords[2 * i + 1] };

        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        for i in 0..n {
            let p = pt(i);
            // std::min( p.x(), min_x ): returns the first argument unless the second is smaller
            min_x = if min_x < p.x { min_x } else { p.x };
            min_y = if min_y < p.y { min_y } else { p.y };
            max_x = if p.x < max_x { max_x } else { p.x };
            max_y = if p.y < max_y { max_y } else { p.y };
        }
        let width = max_x - min_x;
        let height = max_y - min_y;
        let span = width * width + height * height; // Everything is square dist.

        let center = Point { x: (min_x + max_x) / 2.0, y: (min_y + max_y) / 2.0 };

        let mut i0 = INVALID_INDEX;
        let mut i1 = INVALID_INDEX;
        let mut i2 = INVALID_INDEX;

        // pick a seed point close to the centroid
        let mut min_dist = f64::MAX;
        for i in 0..n {
            let d = Point::dist2(center, pt(i));
            if d < min_dist {
                i0 = i;
                min_dist = d;
            }
        }

        if i0 == INVALID_INDEX {
            // C++: m_points[INVALID_INDEX] (undefined behaviour); only reachable for n == 0
            return Err(DelaunatorError::NotTriangulation);
        }
        let p0 = pt(i0);

        min_dist = f64::MAX;

        // find the point closest to the seed
        for i in 0..n {
            if i == i0 {
                continue;
            }
            let d = Point::dist2(p0, pt(i));
            if d < min_dist && d > 0.0 {
                i1 = i;
                min_dist = d;
            }
        }

        if i1 == INVALID_INDEX {
            // C++: m_points[INVALID_INDEX] (undefined behaviour); all points identical
            return Err(DelaunatorError::NotTriangulation);
        }
        let p1 = pt(i1);

        let mut min_radius = f64::MAX;

        // find the third point which forms the smallest circumcircle
        // with the first two
        for i in 0..n {
            if i == i0 || i == i1 {
                continue;
            }
            let r = circumradius(p0, p1, pt(i));
            if r < min_radius {
                i2 = i;
                min_radius = r;
            }
        }

        if !(min_radius < f64::MAX) {
            return Err(DelaunatorError::NotTriangulation);
        }

        let p2 = pt(i2);

        if counterclockwise_pts(p0, p1, p2) {
            std::mem::swap(&mut i1, &mut i2);
        }

        let i0x = p0.x;
        let i0y = p0.y;
        let i1x = pt(i1).x;
        let i1y = pt(i1).y;
        let i2x = pt(i2).x;
        let i2y = pt(i2).y;

        let m_center = circumcenter(i0x, i0y, i1x, i1y, i2x, i2y);

        // Calculate the distances from the center once to avoid having to
        // calculate for each compare.
        let mut dists: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            let p = pt(i);
            dists.push(dist(p.x, p.y, m_center.x, m_center.y));
        }

        // sort the points by distance from the seed triangle circumcenter
        std_sort(&mut ids, |&i, &j| dists[i] < dists[j]);

        // initialize a hash table for storing edges of the advancing convex hull
        let hash_size = (n as f64).sqrt().ceil() as usize;

        let mut st = State {
            coords,
            triangles: Vec::new(),
            halfedges: Vec::new(),
            hull_prev: vec![0; n],
            hull_next: vec![0; n],
            hull_tri: vec![0; n],
            hull_start: i0,
            hash: vec![INVALID_INDEX; hash_size],
            center: m_center,
            hash_size,
            edge_stack: Vec::new(),
        };

        st.hull_next[i0] = i1;
        st.hull_prev[i2] = i1;
        st.hull_next[i1] = i2;
        st.hull_prev[i0] = i2;
        st.hull_next[i2] = i0;
        st.hull_prev[i1] = i0;

        st.hull_tri[i0] = 0;
        st.hull_tri[i1] = 1;
        st.hull_tri[i2] = 2;

        let k0 = st.hash_key(i0x, i0y);
        st.hash[k0] = i0;
        let k1 = st.hash_key(i1x, i1y);
        st.hash[k1] = i1;
        let k2 = st.hash_key(i2x, i2y);
        st.hash[k2] = i2;

        let max_triangles = if n < 3 { 1 } else { 2 * n - 5 };
        st.triangles.reserve(max_triangles * 3);
        st.halfedges.reserve(max_triangles * 3);
        st.add_triangle(i0, i1, i2, INVALID_INDEX, INVALID_INDEX, INVALID_INDEX)?;
        let mut xp = f64::NAN;
        let mut yp = f64::NAN;

        // Go through points based on distance from the center.
        for k in 0..n {
            let i = ids[k];
            let x = coords[2 * i];
            let y = coords[2 * i + 1];

            // skip near-duplicate points
            if k > 0 && check_pts_equal(x, y, xp, yp) {
                continue;
            }
            xp = x;
            yp = y;

            // skip seed triangle points
            if check_pts_equal(x, y, i0x, i0y)
                || check_pts_equal(x, y, i1x, i1y)
                || check_pts_equal(x, y, i2x, i2y)
            {
                continue;
            }

            // find a visible edge on the convex hull using edge hash
            let mut start: usize = 0;

            let key = st.hash_key(x, y);
            for j in 0..st.hash_size {
                start = st.hash[fast_mod((key + j) as u64, st.hash_size as u64) as usize];

                if start != INVALID_INDEX && start != st.hull_next[start] {
                    break;
                }
            }

            if start == INVALID_INDEX {
                // C++: assert / out-of-bounds read
                return Err(DelaunatorError::HullCorrupt);
            }

            start = st.hull_prev[start];
            let mut e = start;
            let mut q;

            // Advance until we find a place in the hull where our current point
            // can be added.
            loop {
                q = st.hull_next[e];
                if Point::equal(st.pt(i), st.pt(e), span) || Point::equal(st.pt(i), st.pt(q), span)
                {
                    e = INVALID_INDEX;
                    break;
                }
                if counterclockwise(
                    x,
                    y,
                    coords[2 * e],
                    coords[2 * e + 1],
                    coords[2 * q],
                    coords[2 * q + 1],
                ) {
                    break;
                }
                e = q;
                if e == start {
                    e = INVALID_INDEX;
                    break;
                }
            }

            if e == INVALID_INDEX {
                // likely a near-duplicate point; skip it
                continue;
            }

            // add the first triangle from the point
            let he = st.hull_next[e];
            let hte = st.hull_tri[e];
            let mut t = st.add_triangle(e, i, he, INVALID_INDEX, INVALID_INDEX, hte)?;

            st.hull_tri[i] = st.legalize(t + 2)?; // Legalize the triangle we just added.
            st.hull_tri[e] = t;

            // walk forward through the hull, adding more triangles and
            // flipping recursively
            let mut next = st.hull_next[e];
            loop {
                q = st.hull_next[next];
                if !counterclockwise(
                    x,
                    y,
                    coords[2 * next],
                    coords[2 * next + 1],
                    coords[2 * q],
                    coords[2 * q + 1],
                ) {
                    break;
                }
                let hti = st.hull_tri[i];
                let htn = st.hull_tri[next];
                t = st.add_triangle(next, i, q, hti, INVALID_INDEX, htn)?;
                st.hull_tri[i] = st.legalize(t + 2)?;
                st.hull_next[next] = next; // mark as removed
                next = q;
            }

            // walk backward from the other side, adding more triangles and flipping
            if e == start {
                loop {
                    q = st.hull_prev[e];
                    if !counterclockwise(
                        x,
                        y,
                        coords[2 * q],
                        coords[2 * q + 1],
                        coords[2 * e],
                        coords[2 * e + 1],
                    ) {
                        break;
                    }
                    let hte = st.hull_tri[e];
                    let htq = st.hull_tri[q];
                    t = st.add_triangle(q, i, e, INVALID_INDEX, hte, htq)?;
                    st.legalize(t + 2)?;
                    st.hull_tri[q] = t;
                    st.hull_next[e] = e; // mark as removed
                    e = q;
                }
            }

            // update the hull indices
            st.hull_prev[i] = e;
            st.hull_start = e;
            st.hull_prev[next] = i;
            st.hull_next[e] = i;
            st.hull_next[i] = next;

            let ki = st.hash_key(x, y);
            st.hash[ki] = i;
            let ke = st.hash_key(coords[2 * e], coords[2 * e + 1]);
            st.hash[ke] = e;
        }

        Ok(Delaunator {
            triangles: st.triangles,
            halfedges: st.halfedges,
            hull_prev: st.hull_prev,
            hull_next: st.hull_next,
            hull_tri: st.hull_tri,
            hull_start: st.hull_start,
        })
    }
}
