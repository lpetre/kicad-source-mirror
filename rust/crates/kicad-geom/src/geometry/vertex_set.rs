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
// Based on Uniform Plane Subdivision algorithm from Lamot, Marko, and Borut Zalik.
// "A fast polygon triangulation algorithm based on uniform plane subdivision."
// Computers & graphics 27, no. 2 (2003): 239-253.
//
// Code derived from:
// K-3D which is Copyright (c) 2005-2006, Romain Behar, GPL-2, license above
// earcut which is Copyright (c) 2016, Mapbox, ISC
//
// ISC License:
// Permission to use, copy, modify, and/or distribute this software for any purpose
// with or without fee is hereby granted, provided that the above copyright notice
// and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
// REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
// FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
// INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
// OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
// TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
// THIS SOFTWARE.

//! Port of `libs/kimath/include/geometry/vertex_set.h` and
//! `libs/kimath/src/geometry/vertex_set.cpp`: `VERTEX` (a node of the circular, doubly linked
//! polygon rings and of the z-order list used by the ear clipping triangulation) and
//! `VERTEX_SET` (the owner of the vertices).
//!
//! The C++ vertices live in a `std::deque` and link to each other with raw pointers; here they
//! live in [`VertexSet::vertices`] (same creation order, which `POLYGON_TRIANGULATION` relies
//! on) and link with indices ([`VertexId`], [`NIL`] for a null pointer).  The `VERTEX` methods
//! are therefore methods of [`VertexSet`] taking the vertex id as first argument; dereferencing
//! a null link (which crashes the C++) panics.  The `void* m_userData` is a `usize` (0 for
//! `nullptr`), which is how KiCad uses it (`(void*)( intptr_t )( i )`).

use crate::math::box2::Box2I;
use crate::math::util::{std_clamp, std_max, std_min};
use crate::math::vector2::Vector2I;
use crate::stdsort::std_sort;

/// Index of a vertex in [`VertexSet::vertices`] (C++: `VERTEX*`).
pub type VertexId = usize;

/// The null vertex link (C++: `nullptr`).
pub const NIL: VertexId = usize::MAX;

/// Port of `VERTEX`.
#[derive(Clone, Debug)]
pub struct Vertex {
    /// `i`: index of the vertex in the source outline (or result vertex list)
    pub i: i32,
    /// `x`
    pub x: f64,
    /// `y`
    pub y: f64,
    /// `prev`: previous vertex of the polygon ring
    pub prev: VertexId,
    /// `next`: next vertex of the polygon ring
    pub next: VertexId,
    /// `z`: z-order curve value
    pub z: u32,
    /// `prevZ`: previous node in z-order
    pub prev_z: VertexId,
    /// `nextZ`: next node in z-order
    pub next_z: VertexId,
    /// `m_userData`
    pub user_data: usize,
    /// `m_earBlocker`: last vertex found inside this vertex's ear, rechecked before rescanning
    /// the z-order list
    pub ear_blocker: VertexId,
    /// `m_inReflexIndex`: whether the last `buildReflexIndex()` call recorded this vertex
    pub in_reflex_index: bool,
}

impl Vertex {
    /// Port of `VERTEX( int aIndex, double aX, double aY, VERTEX_SET* aParent, void* aUserData )`.
    pub fn new(a_index: i32, a_x: f64, a_y: f64, a_user_data: usize) -> Self {
        Vertex {
            i: a_index,
            x: a_x,
            y: a_y,
            prev: NIL,
            next: NIL,
            z: 0,
            prev_z: NIL,
            next_z: NIL,
            user_data: a_user_data,
            ear_blocker: NIL,
            in_reflex_index: false,
        }
    }

    /// Port of `VERTEX::operator==`: the coordinates are equal.
    pub fn same_coords(&self, a_rhs: &Vertex) -> bool {
        self.x == a_rhs.x && self.y == a_rhs.y
    }

    /// Port of `VERTEX::inTriangle()`: whether the triangle `a`, `b`, `c` surrounds this
    /// vertex.
    pub fn in_triangle(&self, a: &Vertex, b: &Vertex, c: &Vertex) -> bool {
        in_triangle_xy(self.x, self.y, a, b, c)
    }
}

/// `VERTEX::inTriangle()` for the point (`x`, `y`).
fn in_triangle_xy(x: f64, y: f64, a: &Vertex, b: &Vertex, c: &Vertex) -> bool {
    (c.x - x) * (a.y - y) - (a.x - x) * (c.y - y) >= 0.0
        && (a.x - x) * (b.y - y) - (b.x - x) * (a.y - y) >= 0.0
        && (b.x - x) * (c.y - y) - (c.x - x) * (b.y - y) >= 0.0
}

/// Port of `VERTEX_SET::REFLEX_ENTRY`.
#[derive(Clone, Copy, Debug)]
pub struct ReflexEntry {
    /// `z`
    pub z: u32,
    /// `x`
    pub x: f64,
    /// `y`
    pub y: f64,
    /// `v`
    pub v: VertexId,
}

/// Port of `VERTEX_SET`.
#[derive(Clone, Debug, Default)]
pub struct VertexSet {
    /// `m_bbox`
    pub bbox: Box2I,
    /// `m_vertices` (in creation order)
    pub vertices: Vec<Vertex>,
    /// `m_simplificationLevel`: the squared simplification distance
    pub simplification_level: i64,
    /// `m_reflexIndex`
    pub reflex_index: Vec<ReflexEntry>,
    /// `m_reflexIndexValid`
    pub reflex_index_valid: bool,
}

impl VertexSet {
    /// Port of `VERTEX_SET( int aSimplificationLevel )`.
    pub fn new(a_simplification_level: i32) -> Self {
        VertexSet {
            simplification_level: a_simplification_level as i64 * a_simplification_level as i64,
            ..Default::default()
        }
    }

    /// The vertex `v` (C++ `*v`; panics on `NIL`, where the C++ dereferences a null pointer).
    #[inline]
    pub fn v(&self, v: VertexId) -> &Vertex {
        &self.vertices[v]
    }

    #[inline]
    fn vm(&mut self, v: VertexId) -> &mut Vertex {
        &mut self.vertices[v]
    }

    /// `v->next`
    #[inline]
    pub fn next(&self, v: VertexId) -> VertexId {
        self.vertices[v].next
    }

    /// `v->prev`
    #[inline]
    pub fn prev(&self, v: VertexId) -> VertexId {
        self.vertices[v].prev
    }

    /// `*a == *b` (`VERTEX::operator==`).
    #[inline]
    pub fn eq(&self, a: VertexId, b: VertexId) -> bool {
        self.vertices[a].same_coords(&self.vertices[b])
    }

    /// Port of `VERTEX_SET::SetBoundingBox()`.
    pub fn set_bounding_box(&mut self, a_bbox: &Box2I) {
        self.bbox = *a_bbox;
    }

    /// Port of `VERTEX::remove()`: remove the node from the linked list and z-ordered linked
    /// list.
    pub fn remove(&mut self, v: VertexId) {
        let next = self.vertices[v].next;
        let prev = self.vertices[v].prev;
        self.vm(next).prev = prev;
        self.vm(prev).next = next;

        let prev_z = self.vertices[v].prev_z;
        let next_z = self.vertices[v].next_z;

        if prev_z != NIL {
            self.vm(prev_z).next_z = next_z;
        }

        if next_z != NIL {
            self.vm(next_z).prev_z = prev_z;
        }

        let p = self.vm(v);
        p.next = NIL;
        p.prev = NIL;
        p.next_z = NIL;
        p.prev_z = NIL;
    }

    /// Port of `VERTEX::split()`: split the referenced polygon between the reference point
    /// `a` and vertex `b`, assuming they are in the same polygon.  While we create new vertices
    /// for the linked list, we maintain the same vertex index values from the original
    /// polygon.  In this way, we have two polygons that both share the same vertices.
    ///
    /// Returns the newly created vertex in the polygon that does not include the reference
    /// vertex.
    pub fn split(&mut self, a: VertexId, b: VertexId) -> VertexId {
        let (ai, ax, ay, a_user) = {
            let p = &self.vertices[a];
            (p.i, p.x, p.y, p.user_data)
        };

        // VECTOR2I( x, y ): the coordinates came from integers, the conversion is exact
        let a2 = self.insert_vertex(
            ai,
            Vector2I::new(crate::cmath::f64_to_i32_x86(ax), crate::cmath::f64_to_i32_x86(ay)),
            NIL,
            a_user,
        );
        let (bi, bx, by) = {
            let p = &self.vertices[b];
            (p.i, p.x, p.y)
        };
        self.vertices.push(Vertex::new(bi, bx, by, a_user));
        let b2 = self.vertices.len() - 1;
        let an = self.vertices[a].next;
        let bp = self.vertices[b].prev;

        self.vm(a).next = b;
        self.vm(b).prev = a;

        self.vm(a2).next = an;
        self.vm(an).prev = a2;

        self.vm(b2).next = a2;
        self.vm(a2).prev = b2;

        self.vm(bp).next = b2;
        self.vm(b2).prev = bp;

        b2
    }

    /// Port of `VERTEX::updateOrder()`.
    pub fn update_order(&mut self, v: VertexId) {
        if self.vertices[v].z == 0 {
            let z = self.z_order(self.vertices[v].x, self.vertices[v].y);
            self.vm(v).z = z;
        }
    }

    /// Port of `VERTEX::updateList()`: after inserting or changing nodes, this function should
    /// be called to remove duplicate vertices and ensure z-ordering is correct.
    pub fn update_list(&mut self, this: VertexId) {
        let mut p = self.next(this);

        while p != this {
            // Remove duplicates
            if self.eq(p, self.next(p)) {
                p = self.prev(p);
                let n = self.next(p);
                self.remove(n);

                if p == self.next(p) {
                    break;
                }
            }

            self.update_order(p);
            p = self.next(p);
        }

        self.update_order(this);
        self.z_sort(this);
    }

    /// Port of `VERTEX::zSort()`: sort all vertices in this vertex's list by their Morton
    /// code.
    pub fn z_sort(&mut self, this: VertexId) {
        let mut queue: Vec<VertexId> = vec![this];

        let mut p = self.next(this);

        while p != NIL && p != this {
            queue.push(p);
            p = self.next(p);
        }

        let verts = &self.vertices;
        std_sort(&mut queue, |&a, &b| {
            let a = &verts[a];
            let b = &verts[b];

            if a.z != b.z {
                return a.z < b.z;
            }

            if a.x != b.x {
                return a.x < b.x;
            }

            if a.y != b.y {
                return a.y < b.y;
            }

            a.i < b.i
        });

        let mut prev_elem = NIL;

        for &elem in &queue {
            // A cached ear blocker is only valid while it shares this z-order list
            self.vm(elem).ear_blocker = NIL;

            if prev_elem != NIL {
                self.vm(prev_elem).next_z = elem;
            }

            self.vm(elem).prev_z = prev_elem;
            prev_elem = elem;
        }

        self.vm(prev_elem).next_z = NIL;
    }

    /// Port of `VERTEX::area( const VERTEX* aEnd )`: the signed area of the polygon connected
    /// to `this`, optionally ending at `a_end` (`NIL` for the whole ring).
    pub fn ring_area(&self, this: VertexId, a_end: VertexId) -> f64 {
        let mut p = this;
        let mut a = 0.0;

        loop {
            let pv = &self.vertices[p];
            let nv = &self.vertices[pv.next];
            a += (pv.x + nv.x) * (nv.y - pv.y);
            p = pv.next;

            if p == this || p == a_end {
                break;
            }
        }

        if p != this {
            let pv = &self.vertices[p];
            let t = &self.vertices[this];
            a += (pv.x + t.x) * (t.y - pv.y);
        }

        a / 2.0
    }

    /// Port of `VERTEX::isEar()`: whether `this` is the apex point of an ear in its polygon.
    ///
    /// This works by walking forward and backward in z-order to the limits of the minimal
    /// bounding box formed around the triangle, checking whether any points are located inside
    /// the given triangle.  With `a_match_user_data`, the user data of the vertices must match
    /// (useful when the vertices come from different polygons).  Writes the `mutable`
    /// `m_earBlocker`.
    pub fn is_ear(&mut self, this: VertexId, a_match_user_data: bool) -> bool {
        let mut a = self.prev(this);
        let b = this;
        let mut c = self.next(this);
        let user = self.vertices[this].user_data;

        if a_match_user_data {
            while self.vertices[a].user_data != user {
                a = self.prev(a);
            }

            while self.vertices[c].user_data != user {
                c = self.next(c);
            }
        }

        // If the area >=0, then the three points for a concave sequence
        // with b as the reflex point
        if self.area(a, b, c) >= 0.0 {
            return false;
        }

        let blocks_ear = |s: &VertexSet, p: VertexId| -> bool {
            let pv = &s.vertices[p];
            (!a_match_user_data || pv.user_data == user)
                && p != a
                && p != c
                && pv.in_triangle(&s.vertices[a], &s.vertices[b], &s.vertices[c])
                && s.area(pv.prev, p, pv.next) >= 0.0
        };

        // Earcut revisits a rejected vertex on every lap of the ring, usually for the same
        // blocker
        let blocker = self.vertices[this].ear_blocker;

        if blocker != NIL && self.vertices[blocker].next != NIL && blocks_ear(self, blocker) {
            return false;
        }

        let (av, bv, cv) = (&self.vertices[a], &self.vertices[b], &self.vertices[c]);

        // triangle bbox
        let min_tx = std_min(av.x, std_min(bv.x, cv.x));
        let min_ty = std_min(av.y, std_min(bv.y, cv.y));
        let max_tx = std_max(av.x, std_max(bv.x, cv.x));
        let max_ty = std_max(av.y, std_max(bv.y, cv.y));

        // z-order range for the current triangle bounding box
        let min_z = self.z_order(min_tx, min_ty);
        let max_z = self.z_order(max_tx, max_ty);

        if !a_match_user_data && self.reflex_index_valid {
            let index = &self.reflex_index;
            let mut it = index.partition_point(|e| e.z < min_z);

            while it < index.len() && index[it].z <= max_z {
                let e = &index[it];
                it += 1;

                // Same test as inTriangle(), on the copy held in the index to skip the
                // dereference
                if (cv.x - e.x) * (av.y - e.y) - (av.x - e.x) * (cv.y - e.y) < 0.0
                    || (av.x - e.x) * (bv.y - e.y) - (bv.x - e.x) * (av.y - e.y) < 0.0
                    || (bv.x - e.x) * (cv.y - e.y) - (cv.x - e.x) * (bv.y - e.y) < 0.0
                {
                    continue;
                }

                // Entries of clipped ears stay behind with a null link
                let ev = e.v;

                if self.vertices[ev].next != NIL && blocks_ear(self, ev) {
                    self.vm(this).ear_blocker = ev;
                    return false;
                }
            }

            return true;
        }

        // first look for points inside the triangle in increasing z-order
        let mut p = self.vertices[this].next_z;

        while p != NIL && self.vertices[p].z <= max_z {
            if blocks_ear(self, p) {
                self.vm(this).ear_blocker = p;
                return false;
            }

            p = self.vertices[p].next_z;
        }

        // then look for points in decreasing z-order
        p = self.vertices[this].prev_z;

        while p != NIL && self.vertices[p].z >= min_z {
            if blocks_ear(self, p) {
                self.vm(this).ear_blocker = p;
                return false;
            }

            p = self.vertices[p].prev_z;
        }

        true
    }

    /// Port of `VERTEX_SET::insertVertex()`: create an entry in the vertices lookup and
    /// optionally insert the newly created vertex after `last` into an existing linked list.
    pub fn insert_vertex(
        &mut self,
        a_index: i32,
        pt: Vector2I,
        last: VertexId,
        a_user_data: usize,
    ) -> VertexId {
        self.vertices
            .push(Vertex::new(a_index, pt.x as f64, pt.y as f64, a_user_data));

        let p = self.vertices.len() - 1;

        if last == NIL {
            self.vm(p).prev = p;
            self.vm(p).next = p;
        } else {
            let last_next = self.vertices[last].next;
            self.vm(p).next = last_next;
            self.vm(p).prev = last;
            self.vm(last_next).prev = p;
            self.vm(last).next = p;
        }

        p
    }

    /// Port of `VERTEX_SET::createList()`: link the points of `points` into a circular,
    /// doubly linked list (appended to `a_tail` if given), in counter-clockwise order, dropping
    /// points closer than the simplification level to the previous one.  Returns the last
    /// vertex of the list.
    pub fn create_list(
        &mut self,
        points: &crate::geometry::shape_line_chain::ShapeLineChain,
        a_tail: VertexId,
        a_user_data: usize,
    ) -> VertexId {
        let mut tail = a_tail;
        let mut sum = 0.0;

        // Check for winding order
        for i in 0..points.point_count() {
            let p1 = points.c_point(i);
            let p2 = points.c_point(i + 1);

            sum += (p2.x as f64 - p1.x as f64) * (p2.y as f64 + p1.y as f64);
        }

        let mut last_pt = Vector2I::new(0, 0);
        let mut first = true;

        let mut add_vertex = |s: &mut VertexSet, i: i32| {
            let pt = points.c_point(i);

            if first || pt.squared_distance(last_pt) > s.simplification_level {
                tail = s.insert_vertex(i, pt, tail, a_user_data);
                last_pt = pt;
                first = false;
            }
        };

        if sum > 0.0 {
            for i in (0..points.point_count()).rev() {
                add_vertex(self, i);
            }
        } else {
            for i in 0..points.point_count() {
                add_vertex(self, i);
            }
        }

        if tail != NIL && self.eq(tail, self.next(tail)) {
            let n = self.next(tail);
            self.remove(n);
        }

        tail
    }

    /// Port of `VERTEX_SET::getNextOutlineVertex()`: the next vertex in the outline, avoiding
    /// steiner points and points that overlap with splits.
    pub fn get_next_outline_vertex(&self, a_pt: VertexId) -> VertexId {
        let pt = &self.vertices[a_pt];
        let nz = pt.next_z;
        let pz = pt.prev_z;

        // If we hit a fracture point, we want to continue around the
        // edge we are working on and not switch to the pair edge
        // However, this will depend on which direction the initial
        // fracture hit is.  If we find that we skip directly to
        // a new fracture point, then we know that we are proceeding
        // in the wrong direction from the fracture and should
        // fall through to the next point
        if self.same_point(a_pt, nz)
            && self.same_point(pt.next, self.vertices[nz].prev)
            && pt.y == self.vertices[pt.next].y
        {
            return self.vertices[nz].next;
        }

        if self.same_point(a_pt, pz)
            && self.same_point(pt.next, self.vertices[pz].prev)
            && pt.y == self.vertices[pt.next].y
        {
            return self.vertices[pz].next;
        }

        pt.next
    }

    /// Port of `VERTEX_SET::getPrevOutlineVertex()`: the previous vertex in the outline,
    /// avoiding steiner points and points that overlap with splits.
    pub fn get_prev_outline_vertex(&self, a_pt: VertexId) -> VertexId {
        let pt = &self.vertices[a_pt];
        let nz = pt.next_z;
        let pz = pt.prev_z;

        // See get_next_outline_vertex()
        if self.same_point(a_pt, nz) && pt.y == self.vertices[pt.prev].y {
            return self.vertices[nz].prev;
        }

        if self.same_point(a_pt, pz) && pt.y == self.vertices[pt.prev].y {
            return self.vertices[pz].prev;
        }

        pt.prev
    }

    /// Port of `VERTEX_SET::locallyInside()`: whether the segment from vertex `a` to vertex
    /// `b` is inside the polygon around the immediate area of vertex `a`.
    pub fn locally_inside(&self, a: VertexId, b: VertexId) -> bool {
        let an = self.get_next_outline_vertex(a);
        let ap = self.get_prev_outline_vertex(a);

        if self.area(ap, a, an) < 0.0 {
            self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0
        } else {
            self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0
        }
    }

    /// Port of `VERTEX_SET::middleInside()`: whether the middle of the segment from `a` to `b`
    /// is inside the polygon.
    pub fn middle_inside(&self, a: VertexId, b: VertexId) -> bool {
        let mut p = a;
        let mut inside = false;
        let px = (self.vertices[a].x + self.vertices[b].x) / 2.0;
        let py = (self.vertices[a].y + self.vertices[b].y) / 2.0;

        loop {
            let pv = &self.vertices[p];
            let nv = &self.vertices[pv.next];

            if ((pv.y > py) != (nv.y > py)) && (px < (nv.x - pv.x) * (py - pv.y) / (nv.y - pv.y) + pv.x)
            {
                inside = !inside;
            }

            p = pv.next;

            if p == a {
                break;
            }
        }

        inside
    }

    /// Port of `VERTEX_SET::same_point()`: whether two (non-null) vertices are at the same
    /// point.
    pub fn same_point(&self, a_a: VertexId, a_b: VertexId) -> bool {
        a_a != NIL && a_b != NIL && {
            let a = &self.vertices[a_a];
            let b = &self.vertices[a_b];
            a.x == b.x && a.y == b.y
        }
    }

    /// Port of `VERTEX_SET::zOrder()`: the Morton code of the point (`a_x`, `a_y`), scaled by
    /// the size of the bounding box to fit into 32 bits.
    /// (<http://www.graphics.stanford.edu/~seander/bithacks.html#InterleaveBMN>)
    pub fn z_order(&self, a_x: f64, a_y: f64) -> u32 {
        let limit_x = std_clamp(
            (a_x - self.bbox.x() as f64) / self.bbox.width() as f64,
            0.0,
            1.0,
        );
        let limit_y = std_clamp(
            (a_y - self.bbox.y() as f64) / self.bbox.height() as f64,
            0.0,
            1.0,
        );

        let mut x = f64_to_u32_x86(limit_x * 32767.0);
        let mut y = f64_to_u32_x86(limit_y * 32767.0);

        x = (x | (x << 8)) & 0x00FF00FF;
        x = (x | (x << 4)) & 0x0F0F0F0F;
        x = (x | (x << 2)) & 0x33333333;
        x = (x | (x << 1)) & 0x55555555;

        y = (y | (y << 8)) & 0x00FF00FF;
        y = (y | (y << 4)) & 0x0F0F0F0F;
        y = (y | (y << 2)) & 0x33333333;
        y = (y | (y << 1)) & 0x55555555;

        x | (y << 1)
    }

    /// Port of `VERTEX_SET::area( p, q, r )`: twice the signed area of the triangle formed by
    /// the vertices `p`, `q` and `r`.
    pub fn area(&self, p: VertexId, q: VertexId, r: VertexId) -> f64 {
        let (p, q, r) = (&self.vertices[p], &self.vertices[q], &self.vertices[r]);
        area_xy(p.x, p.y, q.x, q.y, r.x, r.y)
    }

    /// Port of `VERTEX_SET::buildReflexIndex()`: index the reflex vertices of the ring
    /// containing `a_start` in z-order.
    ///
    /// Only a reflex vertex can block an ear, so while the index is valid `is_ear()` scans it
    /// instead of walking every vertex of the z-order list.  The index stays valid only while
    /// the ring changes by removing ears, which never turns a convex vertex reflex.
    pub fn build_reflex_index(&mut self, a_start: VertexId) {
        self.reflex_index.clear();

        let mut p = a_start;

        loop {
            // Collinear vertices count as reflex, matching the blocker test in isEar()
            let (pp, pn) = (self.prev(p), self.next(p));
            let reflex = self.area(pp, p, pn) >= 0.0;
            self.vm(p).in_reflex_index = reflex;

            if reflex {
                self.update_order(p);
                let pv = &self.vertices[p];
                self.reflex_index.push(ReflexEntry {
                    z: pv.z,
                    x: pv.x,
                    y: pv.y,
                    v: p,
                });
            }

            p = self.next(p);

            if p == a_start {
                break;
            }
        }

        std_sort(&mut self.reflex_index, |l, r| l.z < r.z);

        self.reflex_index_valid = true;
    }
}

/// `VERTEX_SET::area()` on raw coordinates (also used for the temporary `VERTEX tmp` of
/// `removeNullTriangles()`).
#[inline]
pub fn area_xy(px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64) -> f64 {
    (qy - py) * (rx - qx) - (qx - px) * (ry - qy)
}

/// `static_cast<uint32_t>( double )` as the reference build (x86-64) executes it: a 64-bit
/// `cvttsd2si` whose low 32 bits are kept, so NaN gives 0.  The values reaching it are in
/// `[0, 32767]` or NaN (a degenerate bounding box).
#[inline]
fn f64_to_u32_x86(x: f64) -> u32 {
    crate::cmath::f64_to_i64_x86(x) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_and_z_order() {
        let mut vs = VertexSet::new(0);
        vs.set_bounding_box(&Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 100)));
        let a = vs.insert_vertex(0, Vector2I::new(0, 0), NIL, 0);
        let b = vs.insert_vertex(1, Vector2I::new(100, 0), a, 0);
        let c = vs.insert_vertex(2, Vector2I::new(100, 100), b, 0);
        assert_eq!(vs.next(c), a);
        assert_eq!(vs.prev(a), c);
        assert_eq!(vs.z_order(100.0, 100.0), 0x3FFF_FFFF);
        assert_eq!(vs.z_order(f64::NAN, 0.0), 0);
        vs.update_list(a);
        assert!(vs.ring_area(a, NIL) > 0.0);
        vs.remove(b);
        assert_eq!(vs.next(a), c);
        assert_eq!(vs.v(b).next, NIL);
    }
}
