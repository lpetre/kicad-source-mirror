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

//! The Rust side of `tri_oracle.cpp` (package G4b): the same generators (drawing the random
//! values in the same order) and the same records.

use kicad_geom::geometry::eda_angle::ANGLE_90;
use kicad_geom::geometry::poly_ystripes_index::PolyYStripesIndex;
use kicad_geom::geometry::polygon_triangulation::PolygonTriangulation;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::{Polygon, ShapePolySet, TriangulatedPolygon};
use kicad_geom::geometry::shape_poly_tri_index::any_triangle_hit_point;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::vector2::Vector2I;

use super::sections::{Rec, Results, Rng, Section};

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn rec_tri(r: &mut Rec, t: &TriangulatedPolygon) {
    r.i(t.source_outline_index()).u(t.vertex_count() as u64);

    for &p in &t.vertices {
        r.v(p);
    }

    r.u(t.triangle_count() as u64);

    for tri in &t.triangles {
        r.i(tri.a).i(tri.b).i(tri.c);
    }
}

fn rec_cache(r: &mut Rec, s: &ShapePolySet) {
    r.b(s.is_triangulation_up_to_date())
        .u(s.triangulated_poly_count() as u64);

    for i in 0..s.triangulated_poly_count() {
        rec_tri(r, s.triangulated_polygon(i as i32));
    }

    r.b(s.triangulation_index().is_some());
}

fn scale_of(rng: &mut Rng) -> i32 {
    const K: [i32; 5] = [60, 1000, 100000, 10000000, 400000000];
    K[(rng.next() % 5) as usize]
}

fn grid_pt(rng: &mut Rng, a_mode: i32) -> Vector2I {
    match a_mode {
        0 => {
            let x = rng.range(-3, 3) as i32;
            let y = rng.range(-3, 3) as i32;
            v(x * 1000, y * 1000)
        }
        1 => rng.pt_r(-20, 20),
        2 => rng.pt_r(-1000000, 1000000),
        _ => rng.pt_r(-50000000, 50000000),
    }
}

fn ring(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let n = rng.range(3, 12);
    let mode = rng.range(0, 3) as i32;

    for _ in 0..n {
        let p = grid_pt(rng, mode);
        c.append_point(p, false);
    }

    c.set_closed(true);
    c
}

fn monotone(rng: &mut Rng, k: i32, s: i32, a_dup: bool) -> ShapeLineChain {
    let s = s as i64;
    let mut xs: Vec<i32> = (0..k).map(|_| rng.range(-s, s) as i32).collect();
    xs.sort_unstable();
    let up: Vec<i32> = (0..k).map(|_| rng.range(0, s) as i32).collect();
    let lo: Vec<i32> = (0..k).map(|_| rng.range(-s, 0) as i32).collect();

    let mut c = ShapeLineChain::new();

    for i in 0..k as usize {
        c.append_xy(xs[i], up[i], a_dup);
    }

    for i in (0..k as usize).rev() {
        c.append_xy(xs[i], lo[i], a_dup);
    }

    c.set_closed(true);
    c
}

fn serpentine(rng: &mut Rng, a_step: i32, a_teeth: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    c.append_xy(0, 0, false);

    let mut x = 0;

    for _ in 0..a_teeth {
        let h = rng.range(a_step as i64, a_step as i64 * 3) as i32;
        x += a_step;
        c.append_xy(x, 0, false);
        c.append_xy(x, h, false);
        x += a_step;
        c.append_xy(x, h, false);
        c.append_xy(x, a_step * 4, false);
    }

    c.append_xy(0, a_step * 4, false);
    c.set_closed(true);
    c
}

const K_DIRS: [[i32; 2]; 16] = [
    [1, 0],
    [2, 1],
    [1, 1],
    [1, 2],
    [0, 1],
    [-1, 2],
    [-1, 1],
    [-2, 1],
    [-1, 0],
    [-2, -1],
    [-1, -1],
    [-1, -2],
    [0, -1],
    [1, -2],
    [1, -1],
    [2, -1],
];

fn star(rng: &mut Rng, s: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let ctr = rng.pt_r((-s / 2) as i64, (s / 2) as i64);

    for d in K_DIRS {
        if rng.range(0, 3) == 0 {
            continue;
        }

        let r = rng.range(1, 1.max(s / 2) as i64) as i32;
        c.append_xy(ctr.x + d[0] * r, ctr.y + d[1] * r, false);
    }

    c.set_closed(true);
    c
}

fn subdivided_square(a_size: i32, a_steps: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let f = |i: i32| (i as i64 * a_size as i64 / a_steps as i64) as i32;

    for i in 0..a_steps {
        c.append_xy(f(i), 0, false);
    }

    for i in 0..a_steps {
        c.append_xy(a_size, f(i), false);
    }

    for i in 0..a_steps {
        c.append_xy(a_size - f(i), a_size, false);
    }

    for i in 0..a_steps {
        c.append_xy(0, a_size - f(i), false);
    }

    c.set_closed(true);
    c
}

fn tiny(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();

    match rng.next() % 4 {
        0 => {
            let n = rng.range(0, 8);

            for _ in 0..n {
                let p = rng.pt_r(0, 60);
                c.append_point(p, true);
            }
        }
        1 => {
            let n = rng.range(1, 8);

            for _ in 0..n {
                let x = rng.range(-100000, 100000) as i32;
                c.append_xy(x, 0, true);
            }
        }
        2 => {
            let n = rng.range(1, 6);
            let p = rng.pt_r(-1000, 1000);

            for _ in 0..n {
                c.append_point(p, true);
            }
        }
        _ => {
            let s = rng.range(1, 200) as i32;
            c.append_xy(0, 0, false);
            c.append_xy(s, 0, false);
            c.append_xy(s, s, false);
            c.append_xy(0, s, false);
        }
    }

    c.set_closed(true);
    c
}

fn noisy(rng: &mut Rng, a_base: &ShapeLineChain) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();

    for i in 0..a_base.point_count() {
        let p = a_base.c_point(i);
        c.append_point(p, true);

        if rng.range(0, 3) == 0 {
            let dx = rng.range(-2, 2) as i32;
            let dy = rng.range(-2, 2) as i32;
            c.append_point(p + v(dx, dy), true);
        }

        if rng.range(0, 7) == 0 {
            c.append_point(p, true);
        }
    }

    c.set_closed(true);
    c
}

fn gen_chain(rng: &mut Rng) -> ShapeLineChain {
    let s = scale_of(rng);

    let mut c = match rng.next() % 8 {
        0 => ring(rng),
        1 => {
            let k = rng.range(2, 40) as i32;
            let dup = rng.flag();
            monotone(rng, k, s, dup)
        }
        2 => {
            let step = rng.range(1, 1.max(s / 50) as i64) as i32;
            let teeth = rng.range(1, 24) as i32;
            serpentine(rng, step, teeth)
        }
        3 => {
            let size = rng.range(1, s as i64) as i32;
            let steps = rng.range(1, 30) as i32;
            subdivided_square(size, steps)
        }
        4 => star(rng, s),
        5 => tiny(rng),
        6 => {
            let k = rng.range(2, 30) as i32;
            let base = monotone(rng, k, s, false);
            noisy(rng, &base)
        }
        _ => {
            let k = rng.range(40, 400) as i32;
            monotone(rng, k, s, false)
        }
    };

    if rng.flag() {
        c = c.reverse();
    }

    c
}

fn rect_chain(x0: i32, y0: i32, x1: i32, y1: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    c.append_xy(x0, y0, false);
    c.append_xy(x1, y0, false);
    c.append_xy(x1, y1, false);
    c.append_xy(x0, y1, false);
    c.set_closed(true);
    c
}

fn translated(a_chain: &ShapeLineChain, a_delta: Vector2I) -> ShapeLineChain {
    let mut c = a_chain.clone();
    c.move_(a_delta);
    c
}

fn gen_polygon(rng: &mut Rng) -> Polygon {
    let mut poly: Polygon = Vec::new();
    let s = 1000.max(scale_of(rng));

    match rng.next() % 4 {
        0 => {
            let w = rng.range(10, s as i64) as i32;
            let h = rng.range(10, s as i64) as i32;
            let g = rng.range(1, 4) as i32;
            let cw = w / g;
            let ch = h / g;
            poly.push(rect_chain(0, 0, w, h));

            for i in 0..g {
                for j in 0..g {
                    if !rng.flag() {
                        continue;
                    }

                    let x0 = i * cw + rng.range(1, (cw / 3 + 1) as i64) as i32;
                    let x1 = (i + 1) * cw - rng.range(1, (cw / 3 + 1) as i64) as i32;
                    let y0 = j * ch + rng.range(1, (ch / 3 + 1) as i64) as i32;
                    let y1 = (j + 1) * ch - rng.range(1, (ch / 3 + 1) as i64) as i32;
                    let mid = v((x0 + x1) / 2, (y0 + y1) / 2);

                    match rng.next() % 3 {
                        0 => poly.push(rect_chain(x0, y0, x1, y1)),
                        1 => {
                            let sz = (x1 - x0).min(y1 - y0) / 2 - 1;

                            if sz >= 1 {
                                let k = rng.range(2, 12) as i32;
                                let m = monotone(rng, k, sz, false);
                                poly.push(translated(&m, mid));
                            }
                        }
                        _ => {
                            let mut d = ShapeLineChain::new();
                            let n = rng.range(1, 4);

                            for _ in 0..n {
                                d.append_point(mid, true);
                            }

                            d.set_closed(true);
                            poly.push(d);
                        }
                    }
                }
            }
        }
        1 => {
            let k = rng.range(3, 30) as i32;
            poly.push(monotone(rng, k, s, false));
            let n = rng.range(1, 4);

            for _ in 0..n {
                let a = rng.pt_r((-s / 2) as i64, (s / 2) as i64);
                let w = rng.range(1, (s / 4) as i64) as i32;
                let h = rng.range(1, (s / 4) as i64) as i32;
                poly.push(rect_chain(a.x, a.y, a.x + w, a.y + h));
            }
        }
        2 => {
            // A hole sharing a vertex with the outline (issue 24059)
            let w = rng.range(100, s as i64) as i32;
            let d = rng.range(1, (w / 4) as i64) as i32;
            let y0 = rng.range(0, (w / 2) as i64) as i32;
            poly.push(rect_chain(0, 0, w, w));
            let mut h = ShapeLineChain::new();
            h.append_xy(0, y0, false);
            h.append_xy(d, y0 + d, false);
            h.append_xy(0, y0 + 2 * d, false);
            h.set_closed(true);
            poly.push(h);
        }
        _ => {
            poly.push(star(rng, s));
            let n = rng.range(1, 3);

            for _ in 0..n {
                let a = rng.pt_r((-s / 8) as i64, (s / 8) as i64);
                let w = rng.range(1, (s / 16 + 1) as i64) as i32;
                poly.push(rect_chain(a.x, a.y, a.x + w, a.y + w));
            }
        }
    }

    for c in poly.iter_mut() {
        if rng.range(0, 3) == 0 {
            *c = c.reverse();
        }
    }

    poly
}

fn gen_zone(rng: &mut Rng) -> ShapePolySet {
    let mut set = ShapePolySet::new();

    match rng.next() % 4 {
        0 | 2 => {
            let n = rng.range(1, 3);

            for _ in 0..n {
                let p = gen_polygon(rng);
                set.add_outline(&p[0]);

                for hole in p.iter().skip(1) {
                    set.add_hole(hole, -1);
                }
            }
        }
        1 => {
            let s = 1000.max(scale_of(rng));
            let n = rng.range(1, 4);

            for _ in 0..n {
                let a = rng.pt_r(-s as i64, (s / 2) as i64);
                let w = rng.range((s / 10) as i64, s as i64) as i32;
                let h = rng.range((s / 10) as i64, s as i64) as i32;
                let mut r = ShapePolySet::new();
                r.add_outline(&rect_chain(a.x, a.y, a.x + w, a.y + h));
                set.boolean_add(&r);
            }

            let m = rng.range(0, 12);

            for _ in 0..m {
                let mut r = ShapePolySet::new();

                if rng.flag() {
                    let a = rng.pt_r(-s as i64, s as i64);
                    let w = rng.range(1, (s / 5) as i64) as i32;
                    let h = rng.range(1, (s / 5) as i64) as i32;
                    r.add_outline(&rect_chain(a.x, a.y, a.x + w, a.y + h));
                } else {
                    r.add_outline(&star(rng, s / 4));
                }

                set.boolean_subtract(&r);
            }
        }
        _ => {
            let n = rng.range(1, 3);

            for _ in 0..n {
                let c = gen_chain(rng);
                set.add_outline(&c);
            }
        }
    }

    if rng.range(0, 2) == 0 {
        set.fracture(true);
    }

    set
}

fn section_chain(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let c = gen_chain(&mut rng);
        let mut r = Rec::default();

        let mut result = TriangulatedPolygon::new(rng.range(-1, 3) as i32);
        let ok = PolygonTriangulation::new(&mut result).tesselate_chain(&c, None);
        r.b(ok);
        rec_tri(&mut r, &result);

        if rng.range(0, 7) == 0 {
            // Hint data: from the same chain, or from another chain
            let other = if rng.flag() {
                c.clone()
            } else {
                gen_chain(&mut rng)
            };
            let mut hinted = TriangulatedPolygon::new(0);
            let ok = PolygonTriangulation::new(&mut hinted).tesselate_chain(&other, Some(&result));
            r.b(ok);
            rec_tri(&mut r, &hinted);
        }

        sec.add(&r);
    }
}

fn section_polygon(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let p = gen_polygon(&mut rng);
        let mut r = Rec::default();

        let mut result = TriangulatedPolygon::new(0);
        let ok = PolygonTriangulation::new(&mut result).tesselate_polygon(&p, None);
        r.b(ok);
        rec_tri(&mut r, &result);
        sec.add(&r);
    }
}

fn section_cache(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let mut set = gen_zone(&mut rng);
        let mut r = Rec::default();
        let simplify = rng.range(0, 3) == 0;
        let index = rng.flag();

        set.cache_triangulation_with(simplify, false, index);
        rec_cache(&mut r, &set);

        match rng.next() % 5 {
            0 => {}
            1 => {
                let d = rng.pt_r(-100000, 100000);
                set.move_(d);
                rec_cache(&mut r, &set);
                set.cache_triangulation(false);
            }
            2 => {
                let k = rng.range(1, 3) as i32;
                let c = rng.pt_r(-100000, 100000);
                set.rotate(ANGLE_90 * k, c);
            }
            3 => {
                let c = rng.pt_r(-100000, 100000);
                let dir = if rng.flag() {
                    FlipDirection::LeftRight
                } else {
                    FlipDirection::TopBottom
                };
                set.mirror(c, dir);
            }
            _ => {
                let copy = set.clone();
                rec_cache(&mut r, &copy);

                if set.outline_count() > 0 && set.c_outline(0).point_count() > 0 {
                    let p = set.c_outline(0).c_point(0);
                    set.set_vertex(0, p + v(1, 0));
                }

                set.cache_triangulation_with(false, false, true);
            }
        }

        rec_cache(&mut r, &set);
        sec.add(&r);
    }
}

fn section_large(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);

        let c = if rng.flag() {
            let k = rng.range(4096, 14000) as i32;
            monotone(&mut rng, k, 400000000, false)
        } else {
            let teeth = rng.range(2000, 4000) as i32;
            serpentine(&mut rng, 20000, teeth)
        };

        let mut r = Rec::default();
        let mut result = TriangulatedPolygon::new(0);
        let ok = PolygonTriangulation::new(&mut result).tesselate_chain(&c, None);
        r.b(ok);
        rec_tri(&mut r, &result);
        sec.add(&r);
    }
}

fn section_partition(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let s = if rng.flag() { 400000000 } else { 30000000 };
        let k = rng.range(26000, 60000) as i32;
        let c = monotone(&mut rng, k, s, false);
        let mut set = ShapePolySet::new();
        set.add_outline(&c);

        let submitter = rng.flag();
        set.cache_triangulation_with(false, submitter, false);

        let mut r = Rec::default();
        rec_cache(&mut r, &set);
        sec.add(&r);
    }
}

fn section_fractions(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let c = gen_chain(&mut rng);
        let leaves = rng.range(0, 8) as usize;

        let mut scratch = TriangulatedPolygon::new(0);
        let tess = PolygonTriangulation::new(&mut scratch);
        let f = tess.partition_area_fractions_for_testing(&c, leaves);
        let mut r = Rec::default();
        r.u(f.len() as u64);

        for d in f {
            r.d(d);
        }

        sec.add(&r);
    }
}

fn section_refine(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let gx = rng.range(1, 12) as i32;
        let gy = rng.range(1, 12) as i32;
        let s_scale = 1000.max(scale_of(&mut rng));
        let s = rng.range(1, (s_scale / 12) as i64) as i32;
        let j = rng.range(0, s as i64) as i64;
        let flip_orientation = rng.flag();
        let mut tp = TriangulatedPolygon::new(0);

        for y in 0..=gy {
            for x in 0..=gx {
                let dx = rng.range(-j, j) as i32;
                let dy = rng.range(-j, j) as i32;
                tp.add_vertex(v(x * s + dx, y * s + dy));
            }
        }

        for y in 0..gy {
            for x in 0..gx {
                let a = y * (gx + 1) + x;
                let b = a + 1;
                let c = a + gx + 2;
                let d = a + gx + 1;

                let t = if rng.flag() {
                    [[a, b, c], [a, c, d]]
                } else {
                    [[a, b, d], [b, c, d]]
                };

                for m in t {
                    if flip_orientation {
                        tp.add_triangle(m[2], m[1], m[0]);
                    } else {
                        tp.add_triangle(m[0], m[1], m[2]);
                    }
                }
            }
        }

        tp.refine();
        let mut r = Rec::default();
        rec_tri(&mut r, &tp);
        sec.add(&r);
    }
}

fn section_ystripes(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);
    const K_ACC: [i32; 6] = [0, 1, 2, 5, 100, 10000];

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let set = gen_zone(&mut rng);
        let mut r = Rec::default();

        let mut index = PolyYStripesIndex::new();
        index.build(&set);

        let bb = set.bbox(0);
        let mx = bb.width() / 10 + 2;
        let my = bb.height() / 10 + 2;

        for _ in 0..40 {
            let mut pt = Vector2I::default();

            if set.outline_count() > 0 && rng.range(0, 3) == 0 {
                let o = rng.range(0, (set.outline_count() - 1) as i64) as i32;
                let c = set.c_outline(o);

                if c.point_count() > 0 {
                    pt = c.c_point(rng.range(0, (c.point_count() - 1) as i64) as i32);
                }
            } else {
                let x = rng.range(bb.x() as i64 - mx, bb.right() as i64 + mx) as i32;
                let y = rng.range(bb.y() as i64 - my, bb.bottom() as i64 + my) as i32;
                pt = v(x, y);
            }

            let acc = K_ACC[(rng.next() % 6) as usize];
            r.b(index.contains(pt, acc));
        }

        sec.add(&r);
    }
}

fn section_collide(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let mut set = gen_zone(&mut rng);
        let mut r = Rec::default();

        match rng.next() % 3 {
            0 => {}                              // cold: Collide() triangulates
            1 => set.cache_triangulation(false), // cached, no index yet
            _ => set.cache_triangulation_with(false, false, true),
        }

        let bb = set.bbox(0);
        let s = 10i64.max(bb.width().max(bb.height())) as i32;

        for _ in 0..8 {
            let x = rng.range(
                bb.x() as i64 - (s / 4) as i64,
                bb.right() as i64 + (s / 4) as i64,
            );
            let y = rng.range(
                bb.y() as i64 - (s / 4) as i64,
                bb.bottom() as i64 + (s / 4) as i64,
            );
            let p = v(x as i32, y as i32);

            let shape: Box<dyn Shape> = match rng.next() % 5 {
                0 => {
                    let w = rng.range(0, (s / 4) as i64) as i32;
                    let h = rng.range(0, (s / 4) as i64) as i32;
                    Box::new(ShapeRect::from_pos_size(p, w, h))
                }
                1 => Box::new(ShapeRect::from_pos_size(p, 1, 1)),
                2 => {
                    let w = rng.range(0, (s / 20) as i64) as i32;
                    let d = rng.pt_r((-s / 4) as i64, (s / 4) as i64);
                    Box::new(ShapeSegment::new(p, p + d, w))
                }
                3 => {
                    let rad = rng.range(0, (s / 10) as i64) as i32;
                    Box::new(ShapeCircle::new(p, rad))
                }
                _ => {
                    let mut compound = ShapeCompound::new();
                    let m = rng.range(1, 3);

                    for _ in 0..m {
                        let d = rng.pt_r((-s / 4) as i64, (s / 4) as i64);

                        match rng.next() % 3 {
                            0 => {
                                let w = rng.range(0, (s / 20) as i64) as i32;
                                compound.add_shape(Box::new(ShapeSegment::new(p, p + d, w)));
                            }
                            1 => {
                                let rad = rng.range(0, (s / 10) as i64) as i32;
                                compound.add_shape(Box::new(ShapeCircle::new(p + d, rad)));
                            }
                            _ => {
                                let w = rng.range(0, (s / 8) as i64) as i32;
                                let h = rng.range(0, (s / 8) as i64) as i32;
                                compound.add_shape(Box::new(ShapeRect::from_pos_size(p + d, w, h)));
                            }
                        }
                    }

                    Box::new(compound)
                }
            };

            let clearance = if rng.range(0, 2) == 0 {
                0
            } else {
                rng.range(-5, (s / 10) as i64) as i32
            };
            let mode = rng.next() % 4;

            // The `&self` variant (triangulating a copy when stale) must agree with the
            // caching one, which is what the C++ const_cast does
            let mut actual_c = -7;
            let mut loc_c = v(-7, -7);
            let hit_c = set.collide_shape(
                shape.as_ref(),
                clearance,
                if mode & 1 != 0 {
                    Some(&mut actual_c)
                } else {
                    None
                },
                if mode & 2 != 0 {
                    Some(&mut loc_c)
                } else {
                    None
                },
            );

            let mut actual = -7;
            let mut loc = v(-7, -7);
            let hit = set.collide_shape_mut(
                shape.as_ref(),
                clearance,
                if mode & 1 != 0 {
                    Some(&mut actual)
                } else {
                    None
                },
                if mode & 2 != 0 { Some(&mut loc) } else { None },
            );

            assert_eq!(
                (hit_c, actual_c, loc_c),
                (hit, actual, loc),
                "collide_shape() and collide_shape_mut() differ"
            );

            r.b(hit).i(actual).v(loc);
            r.b(any_triangle_hit_point(
                set.triangulation_index(),
                set.triangulated_polys(),
                p,
            ));
        }

        rec_cache(&mut r, &set);
        sec.add(&r);
    }
}

fn section_intersects_line(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);
    let inf = f64::INFINITY;
    let nan = f64::NAN;
    let k_slopes: [f64; 18] = [
        inf,
        -inf,
        nan,
        1e300,
        -1e300,
        1e16,
        -1e16,
        9.3e15,
        9.2233720368547758e15,
        9.2233720368547e15,
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        1e-300,
        1e10,
        4.6e15,
    ];

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let a = rng.pt();

        let s = if rng.range(0, 3) == 0 {
            let b = v(a.x, rng.coord());
            Seg::new(a, b)
        } else {
            let b = rng.pt();
            Seg::new(a, b)
        };

        let slope = match rng.next() % 3 {
            0 => k_slopes[(rng.next() % 18) as usize],
            1 => rng.dbl(-10.0, 10.0),
            _ => rng.dbl_scaled(-20, 60),
        };

        let offset = match rng.next() % 6 {
            0 => inf,
            1 => nan,
            2 => 0.0,
            _ => rng.dbl(-1e9, 1e9),
        };

        let res = s.intersects_line(slope, offset);
        let mut r = Rec::default();
        r.seg(&s)
            .d(slope)
            .d(offset)
            .b(res.is_some())
            .v(res.unwrap_or(v(-7, -7)));
        sec.add(&r);
    }
}

pub fn run_all(results: &mut Results) {
    super::sections::section_libm_probe(results);
    section_chain(results, "tri_chain", 500, 40000);
    section_polygon(results, "tri_polygon", 501, 20000);
    section_cache(results, "libm:tri_cache", 502, 10000);
    section_large(results, "tri_large", 503, 24);
    section_partition(results, "tri_partition", 504, 12);
    section_fractions(results, "tri_fractions", 505, 10000);
    section_refine(results, "tri_refine", 506, 10000);
    section_ystripes(results, "ystripes", 507, 5000);
    section_collide(results, "tri_collide", 508, 5000);
    section_intersects_line(results, "seg_intersects_line", 509, 20000);
}
