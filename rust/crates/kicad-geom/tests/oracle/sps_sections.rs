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

//! The Rust side of `tests/oracle/sps_oracle.cpp` (package G4a): the same generators and
//! records, statement by statement, running the port.

// `x % n == 0` mirrors the C++ statements
#![allow(clippy::manual_is_multiple_of)]

use kicad_geom::cmath;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::eda_angle::{DEGREES_T, EdaAngle};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_line_chain::{ShapeLineChain, StringStream};
use kicad_geom::geometry::shape_poly_set::{ShapePolySet, VertexIndex};
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::hash_128::Hash128;
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;

use std::f64::consts::PI;

use super::sections::{Rec, Results, Rng, Section};

const K_MAX_ERR: [i32; 6] = [5000, 25000, 1000, 2500, 10000, 500];

const K_STRATEGIES: [CornerStrategy; 5] = [
    CornerStrategy::AllowAcuteCorners,
    CornerStrategy::ChamferAcuteCorners,
    CornerStrategy::RoundAcuteCorners,
    CornerStrategy::ChamferAllCorners,
    CornerStrategy::RoundAllCorners,
];

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn rec_arc(r: &mut Rec, a: &ShapeArc) {
    r.v(a.get_p0())
        .v(a.get_arc_mid())
        .v(a.get_p1())
        .i(a.get_width());
}

fn rec_chain(r: &mut Rec, c: &ShapeLineChain) {
    r.b(c.is_closed()).i(c.point_count());

    for i in 0..c.point_count() {
        let sh = c.c_shapes()[i as usize];
        r.v(c.c_point(i)).i(sh.0 as i64).i(sh.1 as i64);
    }

    r.u(c.arc_count() as u64);

    for i in 0..c.arc_count() {
        rec_arc(r, c.arc(i));
    }
}

fn rec_poly(r: &mut Rec, s: &ShapePolySet) {
    r.i(s.outline_count());

    for i in 0..s.outline_count() {
        r.u(s.c_polygon(i).len() as u64);

        for c in s.c_polygon(i) {
            rec_chain(r, c);
        }
    }
}

fn rec_hash(r: &mut Rec, h: &Hash128) {
    r.u(h.value64[0]).u(h.value64[1]);
}

fn grid_pt(rng: &mut Rng, mode: i32) -> Vector2I {
    match mode {
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

fn random_ring(rng: &mut Rng, mode: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let n = rng.range(3, 12) as i32;

    for _ in 0..n {
        let p = grid_pt(rng, mode);
        c.append_point(p, false);
    }

    c.set_closed(true);
    c
}

fn rect_chain(c0: Vector2I, w: i32, h: i32, cw: bool) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    c.append_point(v(c0.x - w / 2, c0.y - h / 2), false);

    if cw {
        c.append_point(v(c0.x - w / 2, c0.y + h / 2), false);
        c.append_point(v(c0.x + w / 2, c0.y + h / 2), false);
        c.append_point(v(c0.x + w / 2, c0.y - h / 2), false);
    } else {
        c.append_point(v(c0.x + w / 2, c0.y - h / 2), false);
        c.append_point(v(c0.x + w / 2, c0.y + h / 2), false);
        c.append_point(v(c0.x - w / 2, c0.y + h / 2), false);
    }

    c.set_closed(true);
    c
}

fn circle_chain(c0: Vector2I, r: i32, n: i32, rot: f64) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();

    for i in 0..n {
        let a = rot + 2.0 * PI * i as f64 / n as f64;
        let x: i32 = ki_round(r as f64 * cmath::cos(a));
        let y: i32 = ki_round(r as f64 * cmath::sin(a));
        c.append_point(v(c0.x + x, c0.y + y), false);
    }

    c.set_closed(true);
    c
}

fn oval_chain(a0: Vector2I, b0: Vector2I, r: i32, n: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let dir = cmath::atan2(b0.y as f64 - a0.y as f64, b0.x as f64 - a0.x as f64);

    for i in 0..=n {
        let a = dir + PI / 2.0 + PI * i as f64 / n as f64;
        let x: i32 = ki_round(r as f64 * cmath::cos(a));
        let y: i32 = ki_round(r as f64 * cmath::sin(a));
        c.append_point(v(a0.x + x, a0.y + y), false);
    }

    for i in 0..=n {
        let a = dir - PI / 2.0 + PI * i as f64 / n as f64;
        let x: i32 = ki_round(r as f64 * cmath::cos(a));
        let y: i32 = ki_round(r as f64 * cmath::sin(a));
        c.append_point(v(b0.x + x, b0.y + y), false);
    }

    c.set_closed(true);
    c
}

fn arc_chain(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let ctr = rng.pt_r(-50000000, 50000000);
    let r = rng.range(100000, 5000000) as i32;
    let n = rng.range(2, 5) as i32;
    let a0 = rng.dbl(0.0, 2.0 * PI);

    for i in 0..n {
        let s = a0 + 2.0 * PI * i as f64 / n as f64;
        let e = s + rng.dbl(0.3, 2.0 * PI / n as f64);
        let m = (s + e) / 2.0;
        let sx: i32 = ki_round(r as f64 * cmath::cos(s));
        let sy: i32 = ki_round(r as f64 * cmath::sin(s));
        let mx: i32 = ki_round(r as f64 * cmath::cos(m));
        let my: i32 = ki_round(r as f64 * cmath::sin(m));
        let ex: i32 = ki_round(r as f64 * cmath::cos(e));
        let ey: i32 = ki_round(r as f64 * cmath::sin(e));

        if rng.flag() {
            c.append_arc(&ShapeArc::from_start_mid_end(
                v(ctr.x + sx, ctr.y + sy),
                v(ctr.x + mx, ctr.y + my),
                v(ctr.x + ex, ctr.y + ey),
                0,
            ));
        } else {
            c.append_point(v(ctr.x + sx, ctr.y + sy), false);
            c.append_point(v(ctr.x + ex, ctr.y + ey), false);
        }
    }

    c.set_closed(true);
    c
}

fn random_set(rng: &mut Rng, libm: bool) -> ShapePolySet {
    let mut s = ShapePolySet::new();
    let kind = (rng.next() % if libm { 8 } else { 5 }) as i32;

    match kind {
        0 => {
            let mode = (rng.next() % 4) as i32;
            let n = rng.range(1, 3) as i32;

            for _ in 0..n {
                let ring = random_ring(rng, mode);
                s.add_outline(&ring);

                if rng.next() % 3 == 0 {
                    let hole = random_ring(rng, mode);
                    s.add_hole(&hole, -1);
                }
            }
        }
        1 => {
            let n = rng.range(1, 6) as i32;
            let pitch = rng.range(1, 4) as i32 * 250000;

            for _ in 0..n {
                let cx = rng.range(-4, 4) as i32 * pitch / 2;
                let cy = rng.range(-4, 4) as i32 * pitch / 2;
                let w = rng.range(1, 4) as i32 * pitch / 2;
                let h = rng.range(1, 4) as i32 * pitch / 2;
                let cw = rng.flag();
                s.add_outline(&rect_chain(v(cx, cy), w, h, cw));
            }
        }
        2 => {
            let size = rng.range(2, 40) as i32 * 1000000;
            s.add_outline(&rect_chain(v(0, 0), size, size, false));
            let n = rng.range(0, 10) as i32;

            for _ in 0..n {
                let cx = rng.range((-size / 2) as i64, (size / 2) as i64) as i32 / 100000 * 100000;
                let cy = rng.range((-size / 2) as i64, (size / 2) as i64) as i32 / 100000 * 100000;
                let w = rng.range(1, (size / 200000) as i64) as i32 * 100000;
                let h = rng.range(1, (size / 200000) as i64) as i32 * 100000;
                let cw = rng.flag();
                s.add_hole(&rect_chain(v(cx, cy), w, h, cw), -1);
            }
        }
        3 => {
            let teeth = rng.range(2, 12) as i32;
            let pitch = rng.range(1, 10) as i32 * 100000;
            let depth = rng.range(1, 20) as i32 * 100000;
            let mut c = ShapeLineChain::new();
            c.append_point(v(0, 0), false);

            for i in 0..teeth {
                c.append_point(v(i * pitch * 2, depth), false);
                c.append_point(v(i * pitch * 2 + pitch, depth), false);
                c.append_point(v(i * pitch * 2 + pitch, 0), false);
            }

            c.append_point(v(teeth * pitch * 2, 0), false);
            c.append_point(v(teeth * pitch * 2, -depth), false);
            c.append_point(v(0, -depth), false);
            c.set_closed(true);
            s.add_outline(&c);
        }
        4 => {
            let sub = (rng.next() % 4) as i32;

            if sub >= 1 {
                s.new_outline();
            }

            if sub >= 2 {
                let p = grid_pt(rng, 1);
                s.append(p.x, p.y, -1, -1, true);
                s.append(p.x, p.y, -1, -1, true);
            }

            if sub >= 3 {
                let p = grid_pt(rng, 1);
                s.append_point(p, -1, -1);
            }
        }
        5 => {
            let n = rng.range(1, 4) as i32;

            for _ in 0..n {
                let c = rng.pt_r(-2000000, 2000000);
                let r = rng.range(100000, 1500000) as i32;
                let segs = 8 << (rng.next() % 4);
                let rot = if rng.next() % 2 != 0 {
                    0.0
                } else {
                    PI / segs as f64
                };
                s.add_outline(&circle_chain(c, r, segs, rot));
            }
        }
        6 => {
            let n = rng.range(1, 4) as i32;
            let r = rng.range(50000, 500000) as i32;
            let segs = rng.range(4, 16) as i32;
            let mut a = rng.pt_r(-2000000, 2000000);

            for _ in 0..n {
                let b = rng.pt_r(-2000000, 2000000);
                s.add_outline(&oval_chain(a, b, r, segs));
                a = b;
            }
        }
        _ => {
            let c = arc_chain(rng);
            s.add_outline(&c);

            if rng.flag() {
                let c = arc_chain(rng);
                s.add_outline(&c);
            }
        }
    }

    s
}

fn has_empty_contour(a_set: &ShapePolySet) -> bool {
    (0..a_set.outline_count()).any(|i| a_set.c_polygon(i).iter().any(|c| c.point_count() == 0))
}

fn query_pt(rng: &mut Rng, a_set: &ShapePolySet) -> Vector2I {
    if a_set.total_vertices() > 0 && rng.next() % 3 == 0 {
        let idx = rng.range(0, (a_set.total_vertices() - 1) as i64) as i32;
        let p = a_set.c_vertex_global(idx);
        let dx = rng.range(-2, 2) as i32;
        let dy = rng.range(-2, 2) as i32;
        return v(p.x + dx, p.y + dy);
    }

    let bb = a_set.bbox(0);
    let x0 = bb.x() as i64 - 1000;
    let y0 = bb.y() as i64 - 1000;
    let x = rng.range(x0, x0 + bb.width() + 2000);
    let y = rng.range(y0, y0 + bb.height() + 2000);
    v(x as i32, y as i32)
}

fn section_boolean(results: &mut Results, name: &'static str, libm: bool, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let a = random_set(&mut rng, libm);
        let mut b = random_set(&mut rng, libm);
        let op = (rng.next() % 9) as i32;
        let mut res = a.clone();

        if rng.next() % 4 == 0 {
            let dx = rng.range(-1000, 1000) as i32;
            b.move_(v(dx, 0));
        }

        match op {
            0 => res.boolean_add(&b),
            1 => res.boolean_subtract(&b),
            2 => res.boolean_intersection(&b),
            3 => res.boolean_xor(&b),
            4 => res.boolean_add_sets(&a, &b),
            5 => res.boolean_subtract_sets(&b, &a),
            6 => res.boolean_intersection_sets(&a, &b),
            7 => res.boolean_xor_sets(&b, &a),
            _ => res.simplify(),
        }

        let mut r = Rec::default();
        r.i(op);
        rec_poly(&mut r, &res);
        r.d(res.area()).i(res.arc_count());
        sec.add(&r);
    }
}

fn section_inflate(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let mut s = random_set(&mut rng, true);
        let cs = K_STRATEGIES[(rng.next() % 5) as usize];
        let err = K_MAX_ERR[(rng.next() % 6) as usize];
        let amount: i32 = match rng.next() % 4 {
            0 => rng.range(-10, 10) as i32,
            1 => rng.range(-2000000, 2000000) as i32,
            2 => 100000 * rng.range(-10, 10) as i32,
            _ => rng.range(-300000, 300000) as i32,
        };

        let mode = (rng.next() % 5) as i32;
        let simplify = rng.flag();

        let mut r = Rec::default();
        r.i(mode).i(amount).i(err).b(simplify);

        if mode == 0 {
            s.inflate(amount, cs, err, simplify);
            rec_poly(&mut r, &s);
        } else if mode == 1 {
            s.deflate(amount, cs, err);
            rec_poly(&mut r, &s);
        } else if mode == 2 {
            s.clear_arcs();
            s.fracture(true);
            s.inflate_with_linked_holes(amount, cs, err);
            rec_poly(&mut r, &s);
        } else {
            let mut line = ShapeLineChain::new();

            if s.outline_count() > 0 {
                line = s.c_outline(0).clone();
            }

            if mode == 4 && line.point_count() > 0 {
                line.remove(line.point_count() - 1);
            }

            line.set_closed(false);

            let distinct = (1..line.point_count()).any(|i| line.c_point(i) != line.c_point(0));

            r.b(distinct);

            if !distinct {
                sec.add(&r);
                continue;
            }

            let mut out = ShapePolySet::new();
            out.offset_line_chain(&line, amount, cs, err, simplify);
            rec_poly(&mut r, &out);

            let res = line.offset_line(amount, cs, err, simplify);
            r.b(res.is_some());

            if let Some((left, right)) = res {
                rec_chain(&mut r, &left);
                rec_chain(&mut r, &right);
            }
        }

        sec.add(&r);
    }
}

fn zone_with_holes(rng: &mut Rng, big: bool) -> ShapePolySet {
    let mut s = ShapePolySet::new();
    let r = rng.range(5000000, 50000000) as i32;
    let segs = if big {
        rng.range(900, 2500) as i32
    } else {
        rng.range(8, 64) as i32
    };
    s.add_outline(&circle_chain(v(0, 0), r, segs, 0.0));
    let nh = if big {
        rng.range(8, 40) as i32
    } else {
        rng.range(0, 8) as i32
    };

    for _ in 0..nh {
        let hr = rng.range((r / 100) as i64, (r / 10) as i64) as i32;
        let c = rng.pt_r((-r / 2) as i64, (r / 2) as i64);
        let hs = rng.range(3, 40) as i32;

        if rng.flag() {
            let rot = rng.dbl(0.0, 1.0);
            s.add_hole(&circle_chain(c, hr, hs, rot), -1);
        } else {
            let cw = rng.flag();
            s.add_hole(&rect_chain(c, hr, hr * 2 / 3, cw), -1);
        }
    }

    s
}

fn section_fracture(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let big = n % 25 == 0;
        let src = (rng.next() % 3) as i32;
        let mut s = if src == 0 {
            random_set(&mut rng, true)
        } else {
            zone_with_holes(&mut rng, big)
        };
        let mode = (rng.next() % 4) as i32;

        s.clear_arcs();

        let mut r = Rec::default();
        r.i(src).i(mode);

        if mode == 0 {
            s.fracture(true);
            rec_poly(&mut r, &s);
        } else if mode == 1 {
            s.fracture(false);
            rec_poly(&mut r, &s);
            s.unfracture();
            rec_poly(&mut r, &s);
        } else if mode == 2 {
            s.fracture(true);
            r.b(s.has_holes()).b(s.has_touching_holes());
            let mut b = random_set(&mut rng, true);
            b.clear_arcs();
            let mut res = s.clone();

            if rng.flag() {
                res.boolean_subtract(&b);
            } else {
                res.boolean_add(&b);
            }

            rec_poly(&mut r, &res);
            res = s.clone();
            res.simplify();
            rec_poly(&mut r, &res);
        } else {
            s.fracture(true);
            let mut copy = s.clone();
            copy.unfracture();
            rec_poly(&mut r, &copy);
            copy.fracture(true);
            rec_poly(&mut r, &copy);
        }

        sec.add(&r);
    }
}

fn section_simplify(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let mut s = random_set(&mut rng, false);
        let mode = (rng.next() % 8) as i32;

        let mut r = Rec::default();
        r.i(mode);

        match mode {
            0 => s.simplify(),
            1 => {
                let tol = rng.range(0, 3) as i32 * 1000;
                s.simplify_outlines(tol);
            }
            2 => {
                if s.outline_count() > 0 {
                    r.i(s.normalize_area_outlines());
                }
            }
            3 => s.rebuild_holes_from_contours(),
            4 => {
                let mut paths: Vec<ShapeLineChain> = Vec::new();

                for i in 0..s.outline_count() {
                    for c in s.c_polygon(i) {
                        paths.push(c.clone());
                    }
                }

                let even_odd = rng.flag();
                s.build_polyset_from_oriented_paths(&paths, even_odd);
            }
            5 => {
                if s.outline_count() > 0 && !has_empty_contour(&s) {
                    r.i(s.remove_null_segments());
                }
            }
            6 => s.split_self_touching_outlines(),
            _ => {
                let w = rng.range(1, 50) as i32 * 100000;
                let h = rng.range(1, 50) as i32 * 100000;
                let g = rng.range(0, 3) as i32 * 10;
                let mut c = ShapeLineChain::new();
                c.append_point(v(0, 0), false);
                c.append_point(v(w, 0), false);
                c.append_point(v(w, h), false);
                c.append_point(v(w + w, h + g), false);
                c.append_point(v(w + w, h + h), false);
                c.append_point(v(w, h + h), false);
                c.append_point(v(w, h + g), false);
                c.append_point(v(0, h), false);
                c.set_closed(true);
                s.add_outline(&c);
                s.simplify();
            }
        }

        rec_poly(&mut r, &s);
        sec.add(&r);
    }
}

fn section_chamfer_fillet(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let mut s = random_set(&mut rng, true);
        let dist = if rng.next() % 3 == 0 {
            rng.range(0, 100) as i32
        } else {
            rng.range(0, 2000000) as i32
        };
        let err = K_MAX_ERR[(rng.next() % 6) as usize];
        let mode = (rng.next() % 4) as i32;

        s.clear_arcs();

        // Leave out contours with fewer than 3 distinct points (division by zero in C++)
        let mut ok = s.outline_count() > 0 && !has_empty_contour(&s);

        if !ok {
            continue;
        }

        let mut probe = s.clone();
        probe.remove_null_segments();

        for i in 0..probe.outline_count() {
            for c in probe.c_polygon(i) {
                if c.point_count() < 3 {
                    ok = false;
                }
            }
        }

        if !ok {
            continue;
        }

        let mut r = Rec::default();
        r.i(mode).i(dist).i(err);

        if mode == 0 {
            let res = s.chamfer(dist);
            rec_poly(&mut r, &res);
        } else if mode == 1 {
            let res = s.fillet(dist, err);
            rec_poly(&mut r, &res);
        } else {
            let idx = rng.range(0, (s.outline_count() - 1) as i64) as i32;
            let p = if mode == 2 {
                s.chamfer_polygon(dist as u32, idx)
            } else {
                s.fillet_polygon(dist as u32, err, idx)
            };
            r.u(p.len() as u64);

            for c in &p {
                rec_chain(&mut r, c);
            }
        }

        rec_poly(&mut r, &s);
        sec.add(&r);
    }
}

fn section_query(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let mut s = random_set(&mut rng, true);

        let mut r = Rec::default();
        r.d(s.area())
            .i(s.arc_count())
            .i(s.total_vertices())
            .i(s.full_point_count());
        r.i(s.vertex_count(-1, -1))
            .i(s.vertex_count(0, 0))
            .i(s.hole_count(0));
        r.b(s.has_holes())
            .b(s.has_touching_holes())
            .b(s.is_self_intersecting())
            .b(s.is_empty());
        r.bx(&s.bbox(0)).bx(&s.bbox(1000));
        rec_hash(&mut r, &s.get_hash());
        r.s(&s.format(true));

        for i in 0..s.outline_count() {
            r.b(s.is_polygon_self_intersecting(i));
        }

        // vertex indices
        for g in -1..=s.total_vertices() {
            let rel = s.get_relative_indices(g);
            let idx = rel.unwrap_or_default();
            r.b(rel.is_some())
                .i(idx.polygon)
                .i(idx.contour)
                .i(idx.vertex);

            if rel.is_some() {
                let back = s.get_global_index(idx);
                r.b(back.is_some())
                    .i(back.unwrap_or(-7))
                    .v(s.c_vertex_index(idx))
                    .v(s.c_vertex_global(g));
                r.b(s.is_vertex_in_hole(g));
                let nb = s.get_neighbour_indexes(g);
                r.b(nb.is_some());

                if s.c_polygon(idx.polygon)[idx.contour as usize].point_count() >= 3 {
                    let (prev, next) = nb.unwrap_or((-7, -7));
                    r.i(prev).i(next);
                }
            }
        }

        // iterators
        let mut it = s.c_iterate_with_holes();

        while it.is_valid() {
            r.v(it.get())
                .i(it.get_index().contour)
                .b(it.is_end_contour())
                .b(it.is_last_polygon());
            it.advance();
        }

        for i in 0..s.outline_count() {
            for p in s.c_iterate_outline(i) {
                r.v(p);
            }

            let mut it = s.c_iterate_segments_with_holes_outline(i);

            while it.is_valid() {
                r.seg(&it.get())
                    .i(it.get_index().contour)
                    .i(it.get_index().vertex);
                it.advance();
            }
        }

        for seg in s.c_iterate_segments() {
            r.seg(&seg);
        }

        if n % 2 == 0 {
            s.build_bbox_caches();
        }

        if s.outline_count() > 0 && n % 2 == 0 {
            r.bx(&s.bbox_from_caches());
        }

        if s.total_vertices() == 0 {
            sec.add(&r);
            continue;
        }

        for _ in 0..12 {
            let p = query_pt(&mut rng, &s);
            let cl = if rng.next() % 3 == 0 {
                0
            } else {
                rng.range(0, 200000) as i32
            };
            let acc = rng.range(0, 3) as i32;
            let sub = rng.range(-1, (s.outline_count() - 1) as i64) as i32;

            r.v(p)
                .b(s.contains(p, -1, 0, false))
                .b(s.contains(p, sub, acc, n % 2 == 0));
            r.b(s.point_inside(p, 0, false))
                .b(s.point_on_edge(p, 0))
                .b(s.point_on_edge(p, acc));

            let mut nearest = v(-1, -1);
            r.i(s.squared_distance(p, false))
                .i(s.squared_distance_nearest(p, false, Some(&mut nearest)))
                .v(nearest);

            let mut actual = -1;
            let mut loc = v(-1, -1);
            r.b(s.collide_point(p, cl, None, None))
                .b(s.collide_point(p, cl, Some(&mut actual), Some(&mut loc)))
                .i(actual)
                .v(loc);

            let mut vi = VertexIndex::default();
            r.b(s.collide_vertex(p, None, cl))
                .b(s.collide_vertex(p, Some(&mut vi), cl));
            r.i(vi.polygon).i(vi.contour).i(vi.vertex);
            let mut ei = VertexIndex::default();
            r.b(s.collide_edge(p, None, cl))
                .b(s.collide_edge(p, Some(&mut ei), cl));
            r.i(ei.polygon).i(ei.contour).i(ei.vertex);

            let p2 = query_pt(&mut rng, &s);
            let seg = Seg::new(p, p2);
            let mut near2 = v(-1, -1);
            r.i(s.squared_distance_to_seg(&seg, None))
                .i(s.squared_distance_to_seg(&seg, Some(&mut near2)))
                .v(near2);

            let mut actual2 = -1;
            let mut loc2 = v(-1, -1);
            r.b(s.collide_seg(&seg, cl, None, None))
                .b(s.collide_seg(&seg, cl, Some(&mut actual2), Some(&mut loc2)))
                .i(actual2)
                .v(loc2);

            if s.outline_count() > 0 {
                let pi = rng.range(0, (s.outline_count() - 1) as i64) as i32;
                let mut n3 = v(-1, -1);
                let mut n4 = v(-1, -1);
                r.i(s.squared_distance_to_polygon(p, pi, Some(&mut n3)))
                    .v(n3);
                r.i(s.squared_distance_to_polygon_seg(&seg, pi, Some(&mut n4)))
                    .v(n4);
            }

            let w = rng.range(0, 300000) as i32;
            let ss = ShapeSegment::from_seg(seg, w);
            let sc = ShapeCircle::new(p2, w / 2 + 1);
            let mut actual3 = -1;
            let mut actual4 = -1;
            let mut loc3 = v(-1, -1);
            let mut loc4 = v(-1, -1);
            r.b(s.collide(&ss, cl, None, None))
                .b(s.collide(&ss, cl, Some(&mut actual3), Some(&mut loc3)))
                .i(actual3)
                .v(loc3);
            r.b(s.collide(&sc, cl, Some(&mut actual4), None)).i(actual4);
            r.b(s.collide(&sc, cl, None, Some(&mut loc4))).v(loc4);
        }

        // hatch lines
        if n % 3 == 0 && s.outline_count() > 0 {
            let bb = s.bbox(0);
            let div = rng.range(5, 40);
            let spacing = 1.max((bb.width().max(bb.height()) / div) as i32);
            let len = if rng.flag() {
                -1
            } else {
                rng.range(1, spacing as i64 * 4) as i32
            };
            let mut slopes: Vec<f64> = vec![1.0, -1.0];

            if rng.flag() {
                slopes.push(rng.dbl(-3.0, 3.0));
            }

            let lines = s.generate_hatch_lines(&slopes, spacing, len);
            r.u(lines.len() as u64);

            for l in &lines {
                r.seg(l);
            }
        }

        sec.add(&r);
    }
}

fn section_edit(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n as u64);
        let mut s = random_set(&mut rng, true);
        let mode = (rng.next() % 12) as i32;
        let tv = s.total_vertices();

        let mut r = Rec::default();
        r.i(mode);

        match mode {
            0 => {
                if s.outline_count() != 0 {
                    let g = rng.range(-3, tv as i64 + 3) as i32;
                    let p = rng.pt_r(-1000000, 1000000);
                    s.insert_vertex(g, p);
                }
            }
            1 => {
                if tv > 0 {
                    let g = rng.range(0, tv as i64 - 1) as i32;
                    s.remove_vertex(g);
                }
            }
            2 => {
                if tv > 0 {
                    let g = rng.range(0, tv as i64 - 1) as i32;
                    let p = rng.pt_r(-1000000, 1000000);
                    s.set_vertex(g, p);
                }
            }
            3 => {
                if s.outline_count() > 0 {
                    let o = rng.range(0, (s.outline_count() - 1) as i64) as i32;

                    if rng.flag() {
                        s.remove_outline(o);
                    } else if s.hole_count(o) > 0 {
                        let c = rng.range(1, s.hole_count(o) as i64) as i32;
                        s.remove_contour(c, o);
                    } else {
                        s.delete_polygon(o);
                    }
                }
            }
            4 => {
                let t = random_set(&mut rng, true);
                s.append_set(&t);
                let a = rng.range(0, s.outline_count() as i64) as i32;
                let b = rng.range(a as i64, s.outline_count() as i64) as i32;
                rec_poly(&mut r, &s.subset(a, b));

                if a < s.outline_count() {
                    rec_poly(&mut r, &s.unit_set(a));
                }
            }
            5 => {
                let rf = rng.pt_r(-1000000, 1000000);
                let dir = if rng.flag() {
                    FlipDirection::LeftRight
                } else {
                    FlipDirection::TopBottom
                };
                s.mirror(rf, dir);
            }
            6 => {
                let a = rng.pt_r(-1000000, 1000000);
                let b = rng.pt_r(-1000000, 1000000);
                s.mirror_seg(&Seg::new(a, b));
            }
            7 => {
                let mv = rng.pt_r(-1000000, 1000000);
                s.move_(mv);
                rec_hash(&mut r, &s.get_hash());

                if tv > 0 {
                    s.set_vertex(0, v(1, 2));
                }

                // a stale hash survives in this set, a copy recomputes it
                rec_hash(&mut r, &s.get_hash());
                let copy = s.clone();
                rec_hash(&mut r, &copy.get_hash());
                rec_hash(&mut r, &s.clone_drop_triangulation().get_hash());
                r.b(s.is_triangulation_up_to_date());
            }
            8 => {
                let fx = rng.dbl(-3.0, 3.0);
                let fy = if rng.flag() { fx } else { rng.dbl(-3.0, 3.0) };
                let c = rng.pt_r(-1000000, 1000000);
                s.scale(fx, fy, c);
            }
            9 => {
                let deg = rng.angle_deg();
                let c = rng.pt_r(-1000000, 1000000);
                s.rotate(EdaAngle::new(deg, DEGREES_T), c);
            }
            10 => {
                if s.outline_count() == 0 {
                    s.new_outline();
                }

                let h = s.new_hole(-1);
                r.i(h);
                let a = rng.pt_r(-1000000, 1000000);
                let m = rng.pt_r(-1000000, 1000000);
                let e = rng.pt_r(-1000000, 1000000);
                let arc = ShapeArc::from_start_mid_end(a, m, e, 0);

                if rng.flag() {
                    r.i(s.append_arc(&arc, -1, h, None));
                } else {
                    let err = K_MAX_ERR[(rng.next() % 6) as usize];
                    r.i(s.append_arc(&arc, -1, h, Some(err)));
                }

                let dup = rng.flag();
                r.i(s.append(5, 6, -1, -1, dup));
            }
            _ => {
                // Format / Parse round trip and corrupted texts
                let mut text = format!("polyset {}", s.outline_count());

                for i in 0..s.outline_count() {
                    text += &format!(" poly {}", s.c_polygon(i).len());

                    for c in s.c_polygon(i) {
                        text += &format!(" {}", c.point_count());

                        for p in c.c_points() {
                            text += &format!(" {} {}", p.x, p.y);
                        }
                    }
                }

                let corrupt = (rng.next() % 4) as i32;

                if corrupt == 1 && !text.is_empty() {
                    let len = rng.range(0, text.len() as i64 - 1) as usize;
                    text.truncate(len);
                } else if corrupt == 2 {
                    text += "abc 12x -3";
                }

                let mut stream = StringStream::new(&text);
                let mut parsed = ShapePolySet::new();
                r.b(parsed.parse(&mut stream));
                rec_poly(&mut r, &parsed);
            }
        }

        rec_poly(&mut r, &s);
        rec_hash(&mut r, &s.get_hash());
        sec.add(&r);
    }
}

pub fn run_all(results: &mut Results) {
    super::sections::section_libm_probe(results);
    section_boolean(results, "sps_boolean", false, 400, 20000);
    section_boolean(results, "libm:sps_boolean_kicad", true, 401, 20000);
    section_inflate(results, "libm:sps_inflate", 402, 20000);
    section_fracture(results, "libm:sps_fracture", 403, 5000);
    section_simplify(results, "sps_simplify", 404, 20000);
    section_chamfer_fillet(results, "libm:sps_chamfer_fillet", 405, 15000);
    section_query(results, "libm:sps_query", 406, 10000);
    section_edit(results, "libm:sps_edit", 407, 20000);

    if let Some(dump) = results.dump.as_mut() {
        dump.flush().unwrap();
    }
}

use std::io::Write;
