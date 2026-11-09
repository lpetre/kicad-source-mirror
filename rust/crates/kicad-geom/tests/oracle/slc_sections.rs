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

//! The Rust side of `tests/oracle/slc_oracle.cpp` (package G3): the same generators and
//! records, statement by statement, running the port.

use kicad_geom::geometry::eda_angle::{ANGLE_90, ANGLE_360, DEGREES_T, EdaAngle};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_line_chain::{
    ClipperZValue, Intersection, PointInsideTracker, ShapeLineChain, StringStream,
};
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::box2::Box2D;
use kicad_geom::math::vector2::{Vector2, Vector2D, Vector2I, Vector2L};

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;

use super::sections::{Rec, Results, Rng, Section};

const K_ERR: [i32; 3] = [5000, 25000, 1000];

fn rec_arc(r: &mut Rec, a: &ShapeArc) {
    r.v(a.get_p0())
        .v(a.get_arc_mid())
        .v(a.get_p1())
        .i(a.get_width());
}

fn rec_chain(r: &mut Rec, c: &ShapeLineChain) {
    r.b(c.is_closed()).i(c.width()).i(c.point_count());

    for i in 0..c.point_count() {
        let sh = c.c_shapes()[i as usize];
        r.v(c.c_point(i)).i(sh.0 as i64).i(sh.1 as i64);
    }

    r.u(c.c_shapes().len() as u64).u(c.arc_count() as u64);

    for i in 0..c.arc_count() {
        rec_arc(r, c.arc(i));
    }
}

fn rec_is(r: &mut Rec, is: &Intersection) {
    r.v(is.p)
        .i(is.index_our)
        .i(is.index_their)
        .b(is.is_corner_our);
    r.b(is.is_corner_their).b(is.valid);
}

fn chain_pt(rng: &mut Rng, mode: i32) -> Vector2I {
    match mode {
        0 => {
            let x = rng.range(-3, 3) as i32;
            let y = rng.range(-3, 3) as i32;
            Vector2I::new(x * 1000, y * 1000)
        }
        1 => rng.pt_r(-20, 20),
        2 => rng.pt_r(-1000000, 1000000),
        _ => rng.pt_r(-50000000, 50000000),
    }
}

fn chain_arc(rng: &mut Rng, connect: bool, start: Vector2I) -> ShapeArc {
    let mut s = start;

    if !connect {
        s = rng.pt_r(-50000000, 50000000);
    }

    match rng.next() % 4 {
        0 => {
            let m = s + rng.pt_r(-3000, 3000);
            let e = s + rng.pt_r(-3000, 3000);
            let mut a = ShapeArc::from_start_mid_end(s, m, e, 0);

            if a.get_radius() >= i32::MAX as f64 / 2.0 {
                a = ShapeArc::from_start_mid_end(
                    s,
                    s + Vector2I::new(1000, 1000),
                    s + Vector2I::new(2000, 0),
                    0,
                );
            }

            a
        }
        1 => {
            let mut d = rng.pt_r(-20000000, 20000000);

            if d == Vector2I::new(0, 0) {
                d = Vector2I::new(1000, 0);
            }

            ShapeArc::from_center_start_angle(s + d, s, ANGLE_360, 0)
        }
        _ => {
            let mut d = rng.pt_r(-20000000, 20000000);

            if d == Vector2I::new(0, 0) {
                d = Vector2I::new(1000, 0);
            }

            let a = rng.dbl(5.0, 350.0);
            let neg = rng.flag();
            ShapeArc::from_center_start_angle(
                s + d,
                s,
                EdaAngle::new(if neg { -a } else { a }, DEGREES_T),
                0,
            )
        }
    }
}

fn plain_chain(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let n = rng.range(0, 20) as i32;
    let mode = (rng.next() % 4) as i32;

    for _ in 0..n {
        let mut p = chain_pt(rng, mode);

        if c.point_count() > 0 && rng.next() % 8 == 0 {
            p = c.c_last_point();
        }

        let dup = rng.next() % 4 == 0;
        c.append_point(p, dup);
    }

    let closed = rng.flag();
    c.set_closed(closed);

    if rng.next() % 4 == 0 {
        let w = rng.range(0, 100000) as i32;
        c.set_width(w);
    }

    c
}

fn arc_chain(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let pieces = rng.range(1, 5) as i32;

    for _ in 0..pieces {
        let err = K_ERR[(rng.next() % 3) as usize];

        match rng.next() % 6 {
            0 | 1 => {
                let p = chain_pt(rng, 3);
                c.append_point(p, false);
            }
            2 | 3 => {
                let connect = c.point_count() > 0;
                let start = if connect {
                    c.c_last_point()
                } else {
                    Vector2I::default()
                };
                let a = chain_arc(rng, connect, start);
                c.append_arc_with_error(&a, err);
            }
            4 => {
                let a = chain_arc(rng, false, Vector2I::default());
                c.append_arc_with_error(&a, err);
            }
            _ => {
                let connect = c.point_count() > 0;
                let start = if connect {
                    c.c_last_point()
                } else {
                    Vector2I::default()
                };
                let a = chain_arc(rng, connect, start);
                let mut sub = ShapeLineChain::from_arc(&a, false, Some(err));
                let p = chain_pt(rng, 3);
                sub.append_point(p, false);
                c.append_chain(&sub);
            }
        }
    }

    match rng.next() % 4 {
        0 => c.set_closed(true),
        1 => {
            if c.point_count() > 0 {
                let p = c.c_point(0);
                c.append_point(p, false);
            }

            c.set_closed(true);
        }
        _ => {}
    }

    c
}

fn any_chain(rng: &mut Rng) -> ShapeLineChain {
    if rng.flag() {
        return arc_chain(rng);
    }

    plain_chain(rng)
}

fn query_pt(rng: &mut Rng, c: &ShapeLineChain) -> Vector2I {
    match rng.next() % 5 {
        0 => {
            if c.point_count() > 0 {
                return c.c_point(rng.range(0, c.point_count() as i64 - 1) as i32);
            }

            rng.pt_mid()
        }
        1 => {
            if c.point_count() > 0 {
                let p = c.c_point(rng.range(0, c.point_count() as i64 - 1) as i32);
                return p + rng.pt_r(-3, 3);
            }

            rng.pt_mid()
        }
        2 => {
            if c.segment_count() > 0 {
                let s = c.c_segment(rng.range(0, c.segment_count() as i64 - 1) as i32);
                let t = rng.range(0, 1000);
                let x = s.a.x as i64 + (s.b.x as i64 - s.a.x as i64) * t / 1000;
                let y = s.a.y as i64 + (s.b.y as i64 - s.a.y as i64) * t / 1000;
                return Vector2I::new(x as i32, y as i32);
            }

            rng.pt_mid()
        }
        3 => rng.pt_r(-2000, 2000),
        _ => rng.pt_r(-60000000, 60000000),
    }
}

fn clearance(rng: &mut Rng) -> i32 {
    match rng.next() % 4 {
        0 => 0,
        1 => rng.range(0, 10) as i32,
        2 => rng.range(0, 100000) as i32,
        _ => rng.range(0, 10000000) as i32,
    }
}

fn nearest_point_ub(c: &ShapeLineChain, p: Vector2I) -> bool {
    let mut min_d = i32::MAX;
    let mut nearest = 0;

    for i in 0..c.segment_count() {
        let d = c.c_segment(i).distance(p);

        if d < min_d {
            min_d = d;
            nearest = i;
        }
    }

    if nearest > 0 && nearest < c.point_count() && c.is_arc_segment(nearest as usize) {
        let to_start = c.c_segment(nearest).a - p;
        let to_end = c.c_segment(nearest).b - p;

        if to_start.euclidean_norm() > to_end.euclidean_norm() {
            nearest += 1;
        }

        return nearest >= c.point_count();
    }

    false
}

fn has_cyclic_duplicate(c: &ShapeLineChain) -> bool {
    (0..c.point_count()).any(|i| c.c_point(i) == c.c_point((i + 1) % c.point_count()))
}

fn rec_state(r: &mut Rec, c: &ShapeLineChain) {
    rec_chain(r, c);
    r.i(c.shape_count());

    for i in -2..=c.point_count() + 1 {
        r.i(c.next_shape(i));
    }

    for i in 0..c.point_count() as usize {
        r.b(c.is_shared_pt(i))
            .b(c.is_pt_on_arc(i))
            .b(c.is_arc_segment(i));
        r.b(c.is_arc_start(i))
            .b(c.is_arc_end(i))
            .i(c.arc_index(i) as i64);
    }

    r.i(c.length())
        .d(c.area(false))
        .d(c.area(true))
        .bx(&Shape::bbox(c, 0))
        .bx(&Shape::bbox(c, 7));
}

fn section_build(
    results: &mut Results,
    skips: &mut Skips,
    name: &'static str,
    arcs: bool,
    seed: u64,
    count: i32,
) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        if let Some(recs) = run_case(
            skips,
            name,
            n,
            seed,
            |rng: &mut Rng, recs: &mut Vec<Rec>| {
                let c = if arcs {
                    arc_chain(rng)
                } else {
                    plain_chain(rng)
                };

                let mut r = Rec::default();
                rec_state(&mut r, &c);
                recs.push(r);

                let mut t = c.clone();
                t.set_closed(!c.is_closed());
                let mut r2 = Rec::default();
                rec_state(&mut r2, &t);
                recs.push(r2);

                t.set_closed(c.is_closed());
                let mut r3 = Rec::default();
                rec_state(&mut r3, &t);
                recs.push(r3);

                if c.is_closed() && c.point_count() >= 3 && !has_cyclic_duplicate(&c) {
                    let mut z: Vec<ClipperZValue> = Vec::new();
                    let mut arcs_buf: Vec<ShapeArc> = Vec::new();
                    let orient = rng.flag();

                    if rng.flag() {
                        arcs_buf.push(ShapeArc::from_start_mid_end(
                            Vector2I::new(0, 0),
                            Vector2I::new(10, 10),
                            Vector2I::new(20, 0),
                            0,
                        ));
                    }

                    let mut path = c.convert_to_clipper2(orient, &mut z, &mut arcs_buf);

                    let mut r4 = Rec::default();

                    for p in &path {
                        r4.i(p.0).i(p.1).i(p.2);
                    }

                    for v in &z {
                        r4.i(v.first_arc_idx as i64).i(v.second_arc_idx as i64);
                    }

                    r4.u(arcs_buf.len() as u64);

                    let rot = if path.is_empty() {
                        0
                    } else {
                        rng.range(0, path.len() as i64 - 1) as usize
                    };
                    path.rotate_left(rot);

                    let back = ShapeLineChain::from_clipper2(&path, &z, &arcs_buf);
                    rec_state(&mut r4, &back);
                    recs.push(r4);
                }
            },
        ) {
            for r in &recs {
                sec.add(r);
            }
        }
    }
}

fn section_edit(
    results: &mut Results,
    skips: &mut Skips,
    name: &'static str,
    arcs: bool,
    seed: u64,
    count: i32,
) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        if let Some(recs) = run_case(
            skips,
            name,
            n,
            seed,
            |rng: &mut Rng, recs: &mut Vec<Rec>| {
                let mut c = if arcs {
                    arc_chain(rng)
                } else {
                    plain_chain(rng)
                };
                let pc = c.point_count() as i64;
                let op = (rng.next() % 20) as i32;
                let mut r = Rec::default();
                r.i(op);

                match op {
                    0 => {
                        let v = rng.range(0, pc) as usize;
                        let p = query_pt(rng, &c);
                        c.insert_point(v, p);
                    }
                    1 => {
                        let err = K_ERR[(rng.next() % 3) as usize];
                        let a = chain_arc(rng, false, Vector2I::default());

                        if pc > 0 {
                            c.insert_arc_with_error(0, &a, err);
                        }
                    }
                    2 => {
                        if pc != 0 {
                            let s = rng.range(0, pc - 1) as i32;
                            let e = rng.range(s as i64, pc - 1) as i32;
                            let p = query_pt(rng, &c);
                            c.replace_point(s, e, p);
                        }
                    }
                    3 => {
                        if pc != 0 {
                            let s = rng.range(0, pc - 1) as i32;
                            let e = rng.range(s as i64, pc - 1) as i32;
                            let mut other = any_chain(rng);

                            if other.is_closed() {
                                other.set_closed(false);
                            }

                            if c.is_closed() {
                                c.set_closed(false);
                                r.i(c.point_count());
                            }

                            if rng.next() % 4 == 0 && other.point_count() > 0 {
                                other.set_point(0, c.c_point(s));
                            }

                            c.replace_chain(s, e, &other);
                        }
                    }
                    4 => {
                        if pc != 0 {
                            let s = rng.range(-pc, pc - 1) as i32;
                            let e = rng.range(-pc, pc - 1) as i32;
                            c.remove_range(s, e);
                        }
                    }
                    5 => {
                        let i = rng.range(-pc - 1, pc) as i32;
                        c.remove_shape(i);
                    }
                    6 => {
                        if pc != 0 {
                            let i = rng.range(-pc, 2 * pc - 1) as i32;
                            let p = rng.pt_mid();
                            c.set_point(i, p);
                        }
                    }
                    7 => {
                        let p = query_pt(rng, &c);
                        let exact = rng.flag();
                        r.i(c.split(p, exact));
                    }
                    8 => c.clear_arcs(),
                    9 => c.remove_duplicate_points(),
                    10 => {
                        const TOLS: [i32; 5] = [0, 1, 10, 1000, 2000000];
                        let tol = TOLS[(rng.next() % 5) as usize];
                        c.simplify(tol);
                    }
                    11 => {
                        let col = rng.flag();
                        c.simplify2(col);
                    }
                    12 => c = c.reverse(),
                    13 => {
                        let rf = rng.pt_r(-10000000, 10000000);
                        let lr = rng.flag();
                        c.mirror(
                            rf,
                            if lr {
                                FlipDirection::LeftRight
                            } else {
                                FlipDirection::TopBottom
                            },
                        );
                    }
                    14 => {
                        let a = rng.pt_r(-10000000, 10000000);
                        let b = rng.pt_r(-10000000, 10000000);
                        c.mirror_seg(&Seg::new(a, b));
                    }
                    15 => {
                        let mut deg = rng.dbl(-360.0, 360.0);
                        let ctr = rng.pt_r(-10000000, 10000000);

                        if rng.flag() {
                            deg = 90.0 * rng.range(-4, 4) as f64;
                        }

                        c.rotate(EdaAngle::new(deg, DEGREES_T), ctr);
                    }
                    16 => {
                        let v = rng.pt_r(-10000000, 10000000);
                        c.move_(v);
                    }
                    17 => {
                        if pc != 0 {
                            let i = rng.range(-pc, pc - 1) as isize;
                            let coincident = rng.flag();
                            c.split_arc(i, coincident);
                        }
                    }
                    18 => {
                        let ac = c.arc_count() as i64;
                        let i = rng.range(-ac, ac) as isize;
                        c.convert_arc(i);
                    }
                    _ => {
                        if pc != 0 {
                            let s = rng.range(0, pc - 1) as i32;
                            c.remove(s);
                            let p = query_pt(rng, &c);
                            c.insert_point(s.min(c.point_count()) as usize, p);
                        }
                    }
                }

                rec_state(&mut r, &c);
                recs.push(r);
            },
        ) {
            for r in &recs {
                sec.add(r);
            }
        }
    }
}

fn rec_collide(r: &mut Rec, c: &ShapeLineChain, p: Vector2I, s: &Seg, cl: i32) {
    for k in 0..4 {
        let mut actual = -7;
        let mut loc = Vector2I::new(-7, -7);
        let hit = c.collide_point(
            p,
            cl,
            if k & 1 != 0 { Some(&mut actual) } else { None },
            if k & 2 != 0 { Some(&mut loc) } else { None },
        );
        r.b(hit).i(actual).v(loc);

        actual = -7;
        loc = Vector2I::new(-7, -7);
        let hit = c.collide_seg(
            s,
            cl,
            if k & 1 != 0 { Some(&mut actual) } else { None },
            if k & 2 != 0 { Some(&mut loc) } else { None },
        );
        r.b(hit).i(actual).v(loc);
    }
}

fn section_query(
    results: &mut Results,
    skips: &mut Skips,
    name: &'static str,
    arcs: bool,
    seed: u64,
    count: i32,
) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        if let Some(recs) = run_case(
            skips,
            name,
            n,
            seed,
            |rng: &mut Rng, recs: &mut Vec<Rec>| {
                let c = if arcs {
                    arc_chain(rng)
                } else {
                    plain_chain(rng)
                };
                let pc = c.point_count();
                let p = query_pt(rng, &c);
                let q = query_pt(rng, &c);
                let s = Seg::new(p, q);
                let cl = clearance(rng);
                let thr_a = rng.range(0, 3) as i32;
                let thr = thr_a * rng.range(0, 1000) as i32;

                let mut r = Rec::default();
                r.v(p).v(q).i(cl).i(thr);
                r.i(c.find(p, 0))
                    .i(c.find(p, thr))
                    .i(c.find_segment(p, 1))
                    .i(c.find_segment(p, thr));
                let idx = rng.range(-1, pc as i64) as i32;
                r.i(c.path_length(p, -1)).i(c.path_length(p, idx));

                if pc > 0 {
                    let len = rng.range(0, 100000000) as i32;
                    r.v(c.point_along(len))
                        .v(c.point_along(c.path_length(p, -1)));
                }

                r.v(c.nearest_point(p, true));

                if !nearest_point_ub(&c, p) {
                    r.v(c.nearest_point(p, false));
                }

                let mut dist = -9;
                r.v(c.nearest_point_seg(&s, &mut dist))
                    .i(dist)
                    .i(c.nearest_segment(p));
                r.i(c.distance_outline(p, false))
                    .i(c.distance_outline(p, true))
                    .i(Shape::distance(&c, p));
                r.i(c.squared_distance(p, false))
                    .i(c.squared_distance(p, true));
                r.b(c.point_inside(p, 0, false))
                    .b(c.point_inside(p, thr, false))
                    .b(c.point_on_edge(p, thr));
                r.i(c.edge_containing_point(p, thr))
                    .b(c.check_clearance(p, thr));

                let mut cached = c.clone();
                cached.generate_bbox_cache();
                r.b(cached.point_inside(p, 0, true))
                    .b(cached.point_inside(p, thr, true));

                rec_collide(&mut r, &c, p, &s, cl);

                let mut ips: Vec<Intersection> = Vec::new();
                r.i(c.intersect_seg(&s, &mut ips));

                for is in &ips {
                    rec_is(&mut r, is);
                }

                r.b(c.intersects_seg(&s));

                let other = if arcs {
                    arc_chain(rng)
                } else {
                    plain_chain(rng)
                };

                for ex in 0..2 {
                    let mut ips2: Vec<Intersection> = Vec::new();
                    r.i(c.intersect_chain(&other, &mut ips2, ex == 1, None));

                    for is in &ips2 {
                        rec_is(&mut r, is);
                    }
                }

                let bbcl = rng.range(0, 1000) as i32;
                let bb = Shape::bbox(&other, bbcl);
                let mut ips3: Vec<Intersection> = Vec::new();
                r.i(c.intersect_chain(&other, &mut ips3, false, Some(&bb)));

                for is in &ips3 {
                    rec_is(&mut r, is);
                }

                r.b(c.intersects_chain(&other));

                let si = c.self_intersecting();
                r.b(si.is_some());

                if let Some(si) = si {
                    rec_is(&mut r, &si);
                }

                if pc >= 2 {
                    let sia = c.self_intersecting_with_arcs();
                    r.b(sia.is_some());

                    if let Some(sia) = sia {
                        rec_is(&mut r, &sia);
                    }
                }

                let (found, a0, a1) = match c.closest_points(&other) {
                    Some((a, b)) => (true, a, b),
                    None => (false, Vector2I::new(-5, -5), Vector2I::new(-5, -5)),
                };
                r.b(found).v(a0).v(a1);

                if pc > 0 && other.point_count() > 0 {
                    let (found, b0, b1) = match c.closest_segments_fast(&other) {
                        Some((a, b)) => (true, a, b),
                        None => (false, Vector2I::new(-5, -5), Vector2I::new(-5, -5)),
                    };
                    r.b(found).v(b0).v(b1);
                }

                r.s(&Shape::format(&c, true)).b(c != other).b(c != c);
                r.b(c.compare_geometry(&other, false, 0))
                    .b(c.compare_geometry(&c, false, thr));

                let mut moved = c.clone();
                moved.move_(rng.pt_r(-2, 2));
                r.b(c.compare_geometry(&moved, false, 1))
                    .b(c.compare_geometry(&c.reverse(), true, 0));
                r.b(c.compare_geometry(&other, true, thr));

                if pc > 0 {
                    let s0 = rng.range(-pc as i64, pc as i64 - 1) as i32;
                    let s1 = rng.range(-pc as i64, pc as i64 - 1) as i32;

                    if (if s0 < 0 { s0 + pc } else { s0 }) <= (if s1 < 0 { s1 + pc } else { s1 }) {
                        let err = K_ERR[(rng.next() % 3) as usize];
                        rec_chain(&mut r, &c.slice(s0, s1));
                        rec_chain(&mut r, &c.slice_with_error(s0, s1, err));
                    }
                }

                if pc >= 2 && !nearest_point_ub(&c, p) && !nearest_point_ub(&c, q) {
                    let (pre, mid, post) = c.split_range(p, q);
                    rec_chain(&mut r, &pre);
                    rec_chain(&mut r, &mid);
                    rec_chain(&mut r, &post);
                }

                if pc >= 2 {
                    let k = rng.range(1, pc as i64 - 1) as i32;
                    let mut tracker = PointInsideTracker::new(p);
                    let mut part1 = ShapeLineChain::new();
                    let mut part2 = ShapeLineChain::new();

                    for i in 0..=k {
                        part1.append_point(c.c_point(i), true);
                    }

                    for i in k..pc {
                        part2.append_point(c.c_point(i), true);
                    }

                    tracker.add_polyline(&part1);
                    tracker.add_polyline(&part2);
                    r.b(tracker.is_inside());
                }

                {
                    let mut text = format!(
                        "{} {} {}",
                        pc,
                        if c.is_closed() { "1" } else { "0" },
                        c.arc_count()
                    );

                    for i in 0..pc {
                        text += &format!(
                            " {} {} {}",
                            c.c_point(i).x,
                            c.c_point(i).y,
                            c.c_shapes()[i as usize].0
                        );
                    }

                    for i in 0..c.arc_count() {
                        let a = c.arc(i);
                        let ang = rng.range(-3600, 3600) as i32 / 10;
                        text += &format!(
                            " {} {} {} {} {}.5",
                            a.get_center().x,
                            a.get_center().y,
                            a.get_p0().x,
                            a.get_p0().y,
                            ang
                        );
                    }

                    match rng.next() % 8 {
                        0 => text = "100000 1 0 1 2 3".to_string(),
                        1 => text = "1 1 100000 1 2 3".to_string(),
                        _ => {}
                    }

                    let mut ss = StringStream::new(&text);
                    let mut parsed = ShapeLineChain::new();
                    let ok = parsed.parse(&mut ss);
                    r.b(ok);
                    rec_chain(&mut r, &parsed);
                }

                recs.push(r);
            },
        ) {
            for r in &recs {
                sec.add(r);
            }
        }
    }
}

fn section_x86_casts(results: &mut Results) {
    let mut sec = Section::new(results, "x86_casts");
    let mut rng = Rng::new(900);

    for _ in 0..2000 {
        let mut c = ShapeLineChain::new();

        if rng.flag() {
            let p = rng.pt_mid();
            c.append_point(p, false);
        }

        let p = rng.pt_mid();
        let mut r = Rec::default();
        r.i(Shape::distance(&c, p))
            .i(c.distance_outline(p, false))
            .i(c.distance_outline(p, true));
        sec.add(&r);
    }

    let specials: [f64; 12] = [
        f64::NAN,
        2147483648.0,
        -2147483649.0,
        2147483647.5,
        -2147483648.75,
        9223372036854775808.0,
        -9223372036854775808.0,
        1e300,
        -1e300,
        f64::INFINITY,
        4294967296.0,
        0.5,
    ];

    for _ in 0..4000 {
        let x = specials[(rng.next() % 12) as usize];
        let mut y = specials[(rng.next() % 12) as usize];

        if rng.flag() {
            y = rng.dbl(-3e9, 3e9);
        }

        let v = Vector2D::new(x, y);
        let mut r = Rec::default();
        r.v(Vector2I::from_vec(v))
            .vl(Vector2L::from_vec(v))
            .vu(Vector2::<u32>::from_vec(v));

        let f = Vector2::<f32>::new(x as f32, y as f32);
        r.v(Vector2I::from_vec(f)).vl(Vector2L::from_vec(f));

        let bx = Box2D::new(Vector2D::new(0.0, 0.0), Vector2D::new(x, y));
        r.i(bx.size_max());

        let mut arc = ShapeArc::new();
        arc.construct_from_start_end_angle(
            Vector2I::new(0, 0),
            Vector2I::new(1000, 0),
            ANGLE_90,
            x,
        );
        r.i(arc.get_width());
        sec.add(&r);
    }
}

/// The cases of `slc_oracle.cpp` that are not replayed (`golden/slc_oracle_skip.txt`: the
/// C++ overflows a signed integer on them, see `PORT_STATUS.md`), and the cases that panicked.
#[derive(Default)]
pub struct Skips {
    pub listed: BTreeSet<(String, i32)>,
    pub found: Vec<(String, i32)>,
}

/// Run one case with a generator of its own (seeded like the C++ one); `None` if the case is
/// listed in the skip list or panics (then it is added to `skips.found`).
fn run_case(
    skips: &mut Skips,
    name: &str,
    n: i32,
    seed: u64,
    f: impl FnOnce(&mut Rng, &mut Vec<Rec>),
) -> Option<Vec<Rec>> {
    let mut rng = Rng::new(seed.wrapping_mul(1000003).wrapping_add(n as u64));

    if skips.listed.contains(&(name.to_string(), n)) {
        return None;
    }

    let mut recs = Vec::new();

    match std::panic::catch_unwind(AssertUnwindSafe(|| f(&mut rng, &mut recs))) {
        Ok(()) => Some(recs),
        Err(_) => {
            skips.found.push((name.to_string(), n));
            None
        }
    }
}

pub fn run_all(results: &mut Results, skips: &mut Skips) {
    super::sections::section_libm_probe(results);
    section_build(results, skips, "slc_build", false, 300, 15000);
    section_build(results, skips, "libm:slc_build_arcs", true, 301, 15000);
    section_edit(results, skips, "libm:slc_edit", false, 302, 30000);
    section_edit(results, skips, "libm:slc_edit_arcs", true, 303, 40000);
    section_query(results, skips, "libm:slc_query", false, 304, 15000);
    section_query(results, skips, "libm:slc_query_arcs", true, 305, 15000);
    section_x86_casts(results);

    if let Some(dump) = results.dump.as_mut() {
        dump.flush().unwrap();
    }
}

use std::io::Write;
