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
// This file is a port of delaunator-cpp (https://github.com/abellgithub/delaunator-cpp), as
// bundled with KiCad in thirdparty/delaunator, which is licensed under the MIT licence:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! Statement-by-statement port of `thirdparty/delaunator/delaunator.cpp` and
//! `delaunator.hpp` (delaunator-cpp, namespace `delaunator`), used by the ratsnest
//! (`pcbnew/ratsnest/ratsnest_data.cpp`).
//!
//! The outputs [`Delaunator::triangles`], [`Delaunator::halfedges`] and the hull arrays are
//! bit for bit those of the C++ code, including on collinear, near-collinear and duplicate
//! input (where delaunator-cpp may leave points out of the triangulation).  The point order
//! comes from a `std::sort` by distance whose ties are frequent on symmetric layouts; it uses
//! the libstdc++-exact [`crate::stdsort::std_sort`].
//!
//! Index arithmetic that the C++ does in `std::size_t` (64 bit on the reference platform) is
//! done in `u64` where it can exceed 32 bits (the hash key), so that the results are the same
//! on wasm32.  [`INVALID_INDEX`] is `usize::MAX` (the C++ `SIZE_MAX`); it is only ever used as
//! a sentinel.

use crate::stdsort::std_sort;

/// `delaunator::INVALID_INDEX`
pub const INVALID_INDEX: usize = usize::MAX;

/// Port of `delaunator::Point`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    /// `Point( x, y )`
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    /// `x()`
    pub fn x(&self) -> f64 {
        self.x
    }

    /// `y()`
    pub fn y(&self) -> f64 {
        self.y
    }

    /// `magnitude2()`
    pub fn magnitude2(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// `Point::determinant`
    pub fn determinant(p1: &Point, p2: &Point) -> f64 {
        p1.x * p2.y - p1.y * p2.x
    }

    /// `Point::vector`
    pub fn vector(p1: &Point, p2: &Point) -> Point {
        Point::new(p2.x - p1.x, p2.y - p1.y)
    }

    /// `Point::dist2`
    pub fn dist2(p1: &Point, p2: &Point) -> f64 {
        let vec = Point::vector(p1, p2);
        vec.x * vec.x + vec.y * vec.y
    }

    /// `Point::equal`
    pub fn equal(p1: &Point, p2: &Point, span: f64) -> bool {
        let dist = Point::dist2(p1, p2) / span;

        // ABELL - This number should be examined to figure how how
        // it correlates with the breakdown of calculating determinants.
        dist < 1e-20
    }
}

/// The ways [`Delaunator::new`] can fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelaunatorError {
    /// C++ `throw std::runtime_error( "not triangulation" )`: fewer than three distinct
    /// points, or all points (numerically) collinear.  For fewer than two distinct points the
    /// C++ indexes `m_points[INVALID_INDEX]` before it gets there (undefined behaviour); the
    /// port reports this error instead.
    NotTriangulation,
    /// C++ `throw std::runtime_error( "Cannot link edge" )`.
    CannotLinkEdge,
    /// The convex hull hash held no hull edge (C++: an `assert` and an out-of-bounds read).
    /// Not reachable with finite input.
    HullCorrupt,
}

impl std::fmt::Display for DelaunatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DelaunatorError::NotTriangulation => write!(f, "not triangulation"),
            DelaunatorError::CannotLinkEdge => write!(f, "Cannot link edge"),
            DelaunatorError::HullCorrupt => write!(f, "corrupt convex hull"),
        }
    }
}

impl std::error::Error for DelaunatorError {}

/// `fast_mod`
///
/// See <https://stackoverflow.com/questions/33333363/built-in-mod-vs-custom-mod-function-improve-the-performance-of-modulus-op/33333636#33333636>
fn fast_mod(i: u64, c: u64) -> u64 {
    if i >= c { i % c } else { i }
}

/// `sum`: Kahan and Babuska summation, Neumaier variant; accumulates less FP error.
fn sum(x: &[f64]) -> f64 {
    let mut sum = x[0];
    let mut err = 0.0;

    for &k in &x[1..] {
        let m = sum + k;
        err += if sum.abs() >= k.abs() {
            sum - m + k
        } else {
            k - m + sum
        };
        sum = m;
    }

    sum + err
}

/// `dist`
fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// `circumradius( const Point& p1, const Point& p2, const Point& p3 )`
#[allow(clippy::double_comparisons)]
fn circumradius(p1: &Point, p2: &Point, p3: &Point) -> f64 {
    let d = Point::vector(p1, p2);
    let e = Point::vector(p1, p3);

    let bl = d.magnitude2();
    let cl = e.magnitude2();
    let det = Point::determinant(&d, &e);

    let radius = Point::new(
        (e.y * bl - d.y * cl) * 0.5 / det,
        (d.x * cl - e.x * bl) * 0.5 / det,
    );

    if (bl > 0.0 || bl < 0.0) && (cl > 0.0 || cl < 0.0) && (det > 0.0 || det < 0.0) {
        return radius.magnitude2();
    }

    f64::MAX
}

/// `counterclockwise( const Point& p0, const Point& p1, const Point& p2 )`
fn counterclockwise_pts(p0: &Point, p1: &Point, p2: &Point) -> bool {
    let v0 = Point::vector(p0, p1);
    let v1 = Point::vector(p0, p2);
    let det = Point::determinant(&v0, &v1);
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

/// `counterclockwise( px, py, qx, qy, rx, ry )`
fn counterclockwise(px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64) -> bool {
    counterclockwise_pts(
        &Point::new(px, py),
        &Point::new(qx, qy),
        &Point::new(rx, ry),
    )
}

/// `circumcenter`
fn circumcenter(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> Point {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;

    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;

    // ABELL - This is suspect for div-by-0.
    let d = dx * ey - dy * ex;

    let x = ax + (ey * bl - dy * cl) * 0.5 / d;
    let y = ay + (dx * cl - ex * bl) * 0.5 / d;

    Point::new(x, y)
}

/// `in_circle`
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

/// `EPSILON = std::numeric_limits<double>::epsilon()`
const EPSILON: f64 = f64::EPSILON;

/// `check_pts_equal`
fn check_pts_equal(x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    (x1 - x2).abs() <= EPSILON && (y1 - y2).abs() <= EPSILON
}

/// `pseudo_angle`: monotonically increases with real angle, but doesn't need expensive
/// trigonometry.
fn pseudo_angle(dx: f64, dy: f64) -> f64 {
    let p = dx / (dx.abs() + dy.abs());
    (if dy > 0.0 { 3.0 - p } else { 1.0 + p }) / 4.0 // [0..1)
}

/// `std::llround` of glibc on x86-64 for an integral `double`: out-of-range values and NaN
/// give `LLONG_MIN` (raising `FE_INVALID`).
fn llround_integral(v: f64) -> i64 {
    if (-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&v) {
        v.round() as i64
    } else {
        i64::MIN
    }
}

/// Port of `delaunator::Delaunator`: the Delaunay triangulation of a set of points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delaunator {
    /// The input coordinates `x0, y0, x1, y1, ...` (C++: a reference to the caller's vector).
    pub coords: Vec<f64>,

    /// 'triangles' stores the indices to the 'X's of the input 'coords'.
    pub triangles: Vec<usize>,

    /// 'halfedges' store indices into 'triangles'.  If `halfedges[X] = Y`, It says that there's
    /// an edge from X to Y where a) X and Y are both indices into triangles and b) X and Y are
    /// indices into different triangles in the array.  This allows you to get from a triangle
    /// to its adjacent triangle.  If the a triangle edge has no adjacent triangle, its half
    /// edge will be INVALID_INDEX.
    pub halfedges: Vec<usize>,

    pub hull_prev: Vec<usize>,
    pub hull_next: Vec<usize>,

    /// This contains indexes into the triangles array.
    pub hull_tri: Vec<usize>,

    pub hull_start: usize,

    hash: Vec<usize>,
    center: Point,
    hash_size: usize,
    edge_stack: Vec<usize>,
}

impl Delaunator {
    /// Port of `Delaunator::Delaunator( std::vector<double> const& in_coords )`: triangulates
    /// the points `( coords[2 * i], coords[2 * i + 1] )`.
    pub fn new(coords: &[f64]) -> Result<Delaunator, DelaunatorError> {
        let mut d = Delaunator {
            coords: coords.to_vec(),
            ..Default::default()
        };

        d.triangulate()?;
        Ok(d)
    }

    /// `m_points[i]`
    fn pt(&self, i: usize) -> Point {
        Point::new(self.coords[2 * i], self.coords[2 * i + 1])
    }

    /// The body of the C++ constructor.
    fn triangulate(&mut self) -> Result<(), DelaunatorError> {
        let n = self.coords.len() >> 1;

        let mut ids: Vec<usize> = (0..n).collect();

        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;

        for i in 0..n {
            let p = self.pt(i);

            // std::min( p.x(), min_x ) returns the first argument unless the second is
            // smaller; std::max( p.x(), max_x ) unless the first is smaller
            min_x = if min_x < p.x { min_x } else { p.x };
            min_y = if min_y < p.y { min_y } else { p.y };
            max_x = if p.x < max_x { max_x } else { p.x };
            max_y = if p.y < max_y { max_y } else { p.y };
        }

        let width = max_x - min_x;
        let height = max_y - min_y;
        let span = width * width + height * height; // Everything is square dist.

        let center = Point::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);

        let mut i0 = INVALID_INDEX;
        let mut i1 = INVALID_INDEX;
        let mut i2 = INVALID_INDEX;

        // pick a seed point close to the centroid
        let mut min_dist = f64::MAX;

        for i in 0..n {
            let d = Point::dist2(&center, &self.pt(i));

            if d < min_dist {
                i0 = i;
                min_dist = d;
            }
        }

        if i0 == INVALID_INDEX {
            // C++: m_points[INVALID_INDEX] (undefined behaviour)
            return Err(DelaunatorError::NotTriangulation);
        }

        let p0 = self.pt(i0);

        min_dist = f64::MAX;

        // find the point closest to the seed
        for i in 0..n {
            if i == i0 {
                continue;
            }

            let d = Point::dist2(&p0, &self.pt(i));

            if d < min_dist && d > 0.0 {
                i1 = i;
                min_dist = d;
            }
        }

        if i1 == INVALID_INDEX {
            // C++: m_points[INVALID_INDEX] (undefined behaviour)
            return Err(DelaunatorError::NotTriangulation);
        }

        let p1 = self.pt(i1);

        let mut min_radius = f64::MAX;

        // find the third point which forms the smallest circumcircle
        // with the first two
        for i in 0..n {
            if i == i0 || i == i1 {
                continue;
            }

            let r = circumradius(&p0, &p1, &self.pt(i));

            if r < min_radius {
                i2 = i;
                min_radius = r;
            }
        }

        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(min_radius < f64::MAX) {
            return Err(DelaunatorError::NotTriangulation);
        }

        let p2 = self.pt(i2);

        if counterclockwise_pts(&p0, &p1, &p2) {
            std::mem::swap(&mut i1, &mut i2);
        }

        let i0x = p0.x;
        let i0y = p0.y;
        let i1x = self.pt(i1).x;
        let i1y = self.pt(i1).y;
        let i2x = self.pt(i2).x;
        let i2y = self.pt(i2).y;

        self.center = circumcenter(i0x, i0y, i1x, i1y, i2x, i2y);

        // Calculate the distances from the center once to avoid having to
        // calculate for each compare.  This used to be done in the comparator,
        // but GCC 7.5+ would copy the comparator to iterators used in the
        // sort, and this was excruciatingly slow when there were many points
        // because you had to copy the vector of distances.
        let dists: Vec<f64> = (0..n)
            .map(|i| {
                let p = self.pt(i);
                dist(p.x, p.y, self.center.x, self.center.y)
            })
            .collect();

        // sort the points by distance from the seed triangle circumcenter
        std_sort(&mut ids, |&i, &j| dists[i] < dists[j]);

        // initialize a hash table for storing edges of the advancing convex hull
        self.hash_size = (n as f64).sqrt().ceil() as usize;
        self.hash = vec![INVALID_INDEX; self.hash_size];

        // initialize arrays for tracking the edges of the advancing convex hull
        self.hull_prev = vec![0; n];
        self.hull_next = vec![0; n];
        self.hull_tri = vec![0; n];

        self.hull_start = i0;

        self.hull_next[i0] = i1;
        self.hull_prev[i2] = i1;
        self.hull_next[i1] = i2;
        self.hull_prev[i0] = i2;
        self.hull_next[i2] = i0;
        self.hull_prev[i1] = i0;

        self.hull_tri[i0] = 0;
        self.hull_tri[i1] = 1;
        self.hull_tri[i2] = 2;

        let k = self.hash_key(i0x, i0y);
        self.hash[k] = i0;
        let k = self.hash_key(i1x, i1y);
        self.hash[k] = i1;
        let k = self.hash_key(i2x, i2y);
        self.hash[k] = i2;

        // ABELL - Why are we doing this is n < 3?  There is no triangulation if
        //  there is no triangle.
        let max_triangles = if n < 3 { 1 } else { 2 * n - 5 };
        self.triangles.reserve(max_triangles * 3);
        self.halfedges.reserve(max_triangles * 3);
        self.add_triangle(i0, i1, i2, INVALID_INDEX, INVALID_INDEX, INVALID_INDEX)?;
        let mut xp = f64::NAN;
        let mut yp = f64::NAN;

        // Go through points based on distance from the center.
        for (k, &i) in ids.iter().enumerate() {
            let x = self.coords[2 * i];
            let y = self.coords[2 * i + 1];

            // skip near-duplicate points
            if k > 0 && check_pts_equal(x, y, xp, yp) {
                continue;
            }

            xp = x;
            yp = y;

            // ABELL - This is dumb.  We have the indices.  Use them.
            // skip seed triangle points
            if check_pts_equal(x, y, i0x, i0y)
                || check_pts_equal(x, y, i1x, i1y)
                || check_pts_equal(x, y, i2x, i2y)
            {
                continue;
            }

            // find a visible edge on the convex hull using edge hash
            let mut start: usize = 0;

            let key = self.hash_key(x, y);

            for j in 0..self.hash_size {
                start = self.hash[fast_mod(key as u64 + j as u64, self.hash_size as u64) as usize];

                // ABELL - Not sure how hull_next[start] could ever equal start
                // I *think* hull_next is just a representation of the hull in one
                // direction.
                if start != INVALID_INDEX && start != self.hull_next[start] {
                    break;
                }
            }

            // ABELL
            // Make sure what we found is on the hull.
            if start == INVALID_INDEX {
                // C++: assert( hull_prev[start] != INVALID_INDEX ), out-of-bounds read
                return Err(DelaunatorError::HullCorrupt);
            }

            start = self.hull_prev[start];
            let mut e = start;
            let mut q;

            // Advance until we find a place in the hull where our current point
            // can be added.
            loop {
                q = self.hull_next[e];

                if Point::equal(&self.pt(i), &self.pt(e), span)
                    || Point::equal(&self.pt(i), &self.pt(q), span)
                {
                    e = INVALID_INDEX;
                    break;
                }

                if counterclockwise(
                    x,
                    y,
                    self.coords[2 * e],
                    self.coords[2 * e + 1],
                    self.coords[2 * q],
                    self.coords[2 * q + 1],
                ) {
                    break;
                }

                e = q;

                if e == start {
                    e = INVALID_INDEX;
                    break;
                }
            }

            // ABELL
            // This seems wrong.  Perhaps we should check what's going on?
            if e == INVALID_INDEX {
                // likely a near-duplicate point; skip it
                continue;
            }

            // add the first triangle from the point
            let mut t = self.add_triangle(
                e,
                i,
                self.hull_next[e],
                INVALID_INDEX,
                INVALID_INDEX,
                self.hull_tri[e],
            )?;

            self.hull_tri[i] = self.legalize(t + 2)?; // Legalize the triangle we just added.
            self.hull_tri[e] = t;

            // walk forward through the hull, adding more triangles and
            // flipping recursively
            let mut next = self.hull_next[e];

            loop {
                q = self.hull_next[next];

                if !counterclockwise(
                    x,
                    y,
                    self.coords[2 * next],
                    self.coords[2 * next + 1],
                    self.coords[2 * q],
                    self.coords[2 * q + 1],
                ) {
                    break;
                }

                t = self.add_triangle(
                    next,
                    i,
                    q,
                    self.hull_tri[i],
                    INVALID_INDEX,
                    self.hull_tri[next],
                )?;
                self.hull_tri[i] = self.legalize(t + 2)?;
                self.hull_next[next] = next; // mark as removed
                next = q;
            }

            // walk backward from the other side, adding more triangles and flipping
            if e == start {
                loop {
                    q = self.hull_prev[e];

                    if !counterclockwise(
                        x,
                        y,
                        self.coords[2 * q],
                        self.coords[2 * q + 1],
                        self.coords[2 * e],
                        self.coords[2 * e + 1],
                    ) {
                        break;
                    }

                    t = self.add_triangle(
                        q,
                        i,
                        e,
                        INVALID_INDEX,
                        self.hull_tri[e],
                        self.hull_tri[q],
                    )?;
                    self.legalize(t + 2)?;
                    self.hull_tri[q] = t;
                    self.hull_next[e] = e; // mark as removed
                    e = q;
                }
            }

            // update the hull indices
            self.hull_prev[i] = e;
            self.hull_start = e;
            self.hull_prev[next] = i;
            self.hull_next[e] = i;
            self.hull_next[i] = next;

            let k = self.hash_key(x, y);
            self.hash[k] = i;
            let k = self.hash_key(self.coords[2 * e], self.coords[2 * e + 1]);
            self.hash[k] = e;
        }

        Ok(())
    }

    /// Port of `Delaunator::get_hull_area`.
    pub fn get_hull_area(&self) -> f64 {
        let c = &self.coords;
        let mut hull_area: Vec<f64> = Vec::new();
        let mut e = self.hull_start;

        loop {
            let p = self.hull_prev[e];
            hull_area.push((c[2 * e] - c[2 * p]) * (c[2 * e + 1] + c[2 * p + 1]));
            e = self.hull_next[e];

            if e == self.hull_start {
                break;
            }
        }

        sum(&hull_area)
    }

    /// Port of `Delaunator::get_triangle_area`.
    pub fn get_triangle_area(&self) -> f64 {
        let c = &self.coords;
        let mut vals: Vec<f64> = Vec::new();

        for t in self.triangles.chunks_exact(3) {
            let ax = c[2 * t[0]];
            let ay = c[2 * t[0] + 1];
            let bx = c[2 * t[1]];
            let by = c[2 * t[1] + 1];
            let cx = c[2 * t[2]];
            let cy = c[2 * t[2] + 1];
            let val = ((by - ay) * (cx - bx) - (bx - ax) * (cy - by)).abs();
            vals.push(val);
        }

        // C++: sum() reads x[0] of an empty vector (undefined behaviour)
        if vals.is_empty() { 0.0 } else { sum(&vals) }
    }

    /// Port of `Delaunator::legalize`.
    fn legalize(&mut self, mut a: usize) -> Result<usize, DelaunatorError> {
        let mut i: usize = 0;
        let mut ar: usize;
        self.edge_stack.clear();

        // recursion eliminated with a fixed-size stack
        loop {
            let b = self.halfedges[a];

            // if the pair of triangles doesn't satisfy the Delaunay condition
            // (p1 is inside the circumcircle of [p0, pl, pr]), flip them,
            // then do the same check/flip recursively for the new pair of triangles
            //
            //           pl                    pl
            //          /||\                  /  \
            //       al/ || \bl            al/    \a
            //        /  ||  \              /      \
            //       /  a||b  \    flip    /___ar___\
            //     p0\   ||   /p1   =>   p0\---bl---/p1
            //        \  ||  /              \      /
            //       ar\ || /br             b\    /br
            //          \||/                  \  /
            //           pr                    pr
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

            let c = &self.coords;
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
                self.link(b, self.halfedges[ar])?;
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

    /// Port of `Delaunator::hash_key`.
    fn hash_key(&self, x: f64, y: f64) -> usize {
        let dx = x - self.center.x;
        let dy = y - self.center.y;
        let v = (pseudo_angle(dx, dy) * self.hash_size as f64).floor();

        // static_cast<std::size_t>( std::llround( v ) ): for NaN (dx == dy == 0) glibc returns
        // LLONG_MIN, which becomes 2^63 as size_t
        fast_mod(llround_integral(v) as u64, self.hash_size as u64) as usize
    }

    /// Port of `Delaunator::add_triangle`.
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

    /// Port of `Delaunator::link`.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_centre() {
        let coords = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 5.0, 5.0];
        let d = Delaunator::new(&coords).unwrap();
        assert_eq!(d.triangles.len(), 12);
        assert_eq!(d.halfedges.len(), 12);
        assert_eq!(d.get_triangle_area(), 200.0);
        assert_eq!(d.get_hull_area().abs(), 200.0);

        // every half edge is either on the hull or paired
        for (e, &o) in d.halfedges.iter().enumerate() {
            assert!(o == INVALID_INDEX || d.halfedges[o] == e);
        }
    }

    #[test]
    fn degenerate_inputs() {
        assert_eq!(
            Delaunator::new(&[]).unwrap_err(),
            DelaunatorError::NotTriangulation
        );
        assert_eq!(
            Delaunator::new(&[1.0, 1.0, 1.0, 1.0, 1.0, 1.0]).unwrap_err(),
            DelaunatorError::NotTriangulation
        );
        assert_eq!(
            Delaunator::new(&[0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0]).unwrap_err(),
            DelaunatorError::NotTriangulation
        );
    }
}
