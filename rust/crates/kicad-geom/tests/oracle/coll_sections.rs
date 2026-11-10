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

//! Rust side of `coll_oracle.cpp` (package G5): the same generators, draw for draw, and the
//! same records.

use kicad_geom::geometry::eda_angle::{DEGREES_T, EdaAngle};
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_null::ShapeNull;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::geometry::shape_simple::ShapeSimple;
use kicad_geom::math::vector2::Vector2I;

use super::sections::{Rec, Results, Rng, Section};

const K_RECT: u64 = 0;
const K_ROUNDRECT: u64 = 1;
const K_SEGMENT: u64 = 2;
const K_CIRCLE: u64 = 3;
const K_ARC: u64 = 4;
const K_SIMPLE: u64 = 5;
const K_CHAIN: u64 = 6;
const K_CHAIN_ARCS: u64 = 7;
const K_POLYSET: u64 = 8;
const K_COMPOUND: u64 = 9;
const K_TRIANGLE: u64 = 10;
const K_ELLIPSE: u64 = 11;
const K_COUNT: u64 = 13;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

#[derive(Clone, Copy, Default)]
struct Opts {
    allow_straight: bool,
    degenerate: bool,
}

fn scale(rng: &mut Rng) -> i32 {
    match rng.next() % 4 {
        0 => 20,
        1 => 2000,
        2 => 200000,
        _ => 20000000,
    }
}

fn half(s: i32) -> i32 {
    1.max(s / 2)
}

fn arc_angle(rng: &mut Rng) -> f64 {
    let sign = if rng.flag() { 1.0 } else { -1.0 };

    match rng.next() % 4 {
        0 => 90.0 * rng.range(1, 3) as f64 * sign,
        1 => rng.dbl(0.5, 5.0) * sign,
        _ => rng.dbl(5.0, 355.0) * sign,
    }
}

fn non_zero(rng: &mut Rng, lim: i32) -> Vector2I {
    let mut d = rng.pt_r(-lim as i64, lim as i64);

    if d == v(0, 0) {
        d = v(1, 0);
    }

    d
}

fn star_ring(rng: &mut Rng, c: Vector2I, r: i32) -> ShapeLineChain {
    let k = rng.range(3, 10);
    let mut ts: Vec<i64> = Vec::new();

    for _ in 0..k {
        ts.push(rng.range(0, 8 * r as i64 - 1));
    }

    ts.sort();

    let mut ring = ShapeLineChain::new();
    let r = r as i64;

    for t in ts {
        let side = t / (2 * r);
        let off = t % (2 * r) - r;
        let (x, y) = match side {
            0 => (off, -r),
            1 => (r, off),
            2 => (-off, r),
            _ => (-r, -off),
        };

        let f = rng.range(30, 100);
        ring.append_point(
            v(c.x + (x * f / 100) as i32, c.y + (y * f / 100) as i32),
            false,
        );
    }

    ring.set_closed(true);
    ring
}

fn rect_ring(c: Vector2I, h: i32) -> ShapeLineChain {
    let mut ring = ShapeLineChain::new();
    ring.append_point(v(c.x - h, c.y - h), false);
    ring.append_point(v(c.x + h, c.y - h), false);
    ring.append_point(v(c.x + h, c.y + h), false);
    ring.append_point(v(c.x - h, c.y + h), false);
    ring.set_closed(true);
    ring
}

fn deg(a: f64) -> EdaAngle {
    EdaAngle::new(a, DEGREES_T)
}

/// `safeArc()`
fn safe_arc(c: Vector2I, st: Vector2I, a: f64, w: i32) -> ShapeArc {
    let mut arc = ShapeArc::from_center_start_angle(c, st, deg(a), w);

    if arc.get_radius() >= i32::MAX as f64 / 2.0 {
        arc = ShapeArc::from_center_start_angle(c, c + v(10, 0), deg(90.0), w);
    }

    arc
}

fn gen_arc(rng: &mut Rng, s: i32, o: Vector2I, opts: Opts, in_chain: bool) -> ShapeArc {
    if opts.degenerate {
        let c = o + rng.pt_r(-s as i64, s as i64);
        let d = non_zero(rng, 3);
        let a = arc_angle(rng);
        let w = rng.range(0, 3) as i32;
        return safe_arc(c, c + d, a, if in_chain { 0 } else { w });
    }

    if !in_chain && opts.allow_straight && rng.next() % 8 == 0 {
        let st = o + rng.pt_r(-s as i64, s as i64);
        let d = non_zero(rng, 1.max(s / 50));
        let k1 = rng.range(1, 20) as i32;
        let k2 = rng.range(1, 20) as i32;
        let w = if rng.flag() {
            0
        } else {
            rng.range(1, 1.max(s / 8) as i64) as i32
        };
        return ShapeArc::from_start_mid_end(
            st,
            st + v(d.x * k1, d.y * k1),
            st + v(d.x * (k1 + k2), d.y * (k1 + k2)),
            w,
        );
    }

    let c = o + rng.pt_r(-s as i64, s as i64);
    let d = non_zero(rng, half(s));
    let a = arc_angle(rng);
    let mut w = 0;

    if !in_chain && rng.flag() {
        w = rng.range(1, 1.max(s / 8) as i64) as i32;
    }

    safe_arc(c, c + d, a, w)
}

/// `GEN`: the shape, or the parent set of a triangle (whose triangle is copied out).
fn gen_shape(rng: &mut Rng, kind: u64, s: i32, o: Vector2I, opts: Opts) -> Box<dyn Shape> {
    let dg = opts.degenerate;
    let sl = s as i64;

    match kind {
        K_RECT => {
            let p = o + rng.pt_r(-sl, sl);
            let (w, h) = if dg {
                let w = rng.range(-1, 1) as i32;
                let h = rng.range(-1, 1) as i32;
                (w, h)
            } else {
                let w = rng.range((-s / 4) as i64, sl) as i32;
                let h = rng.range((-s / 4) as i64, sl) as i32;
                (w, h)
            };

            Box::new(ShapeRect::from_pos_size(p, w, h))
        }
        K_ROUNDRECT => {
            let p = o + rng.pt_r(-sl, sl);
            let w = if dg {
                2
            } else {
                rng.range(2, 2.max(s) as i64) as i32
            };
            let h = if dg {
                2
            } else {
                rng.range(2, 2.max(s) as i64) as i32
            };
            let r = rng.range(1, 1.max(w.min(h) / 2) as i64) as i32;
            let mut rect = ShapeRect::from_pos_size(p, w, h);
            rect.set_radius(r);
            Box::new(rect)
        }
        K_SEGMENT => {
            let a = o + rng.pt_r(-sl, sl);
            let mut b = a;

            if !dg && rng.next() % 8 != 0 {
                b = a + rng.pt_r(-sl, sl);
            }

            let w = match rng.next() % 3 {
                0 => 0,
                1 => rng.range(1, 10) as i32,
                _ => rng.range(0, (s / 4) as i64) as i32,
            };

            Box::new(ShapeSegment::new(a, b, w))
        }
        K_CIRCLE => {
            let c = o + rng.pt_r(-sl, sl);
            let r = if dg || rng.next() % 4 == 0 {
                rng.range(0, 3) as i32
            } else {
                rng.range(0, half(s) as i64) as i32
            };
            Box::new(ShapeCircle::new(c, r))
        }
        K_ARC => Box::new(gen_arc(rng, s, o, opts, false)),
        K_SIMPLE => {
            let base = o + rng.pt_r(-sl, sl);
            let n = rng.range(3, 8);
            let mut sh = ShapeSimple::new();

            for _ in 0..n {
                let d = if dg {
                    rng.pt_r(-1, 1)
                } else {
                    rng.pt_r(-half(s) as i64, half(s) as i64)
                };
                sh.append(base + d);
            }

            Box::new(sh)
        }
        K_CHAIN => {
            let mut c = ShapeLineChain::new();

            if dg {
                let n = rng.range(0, 2);
                let p = o + rng.pt_r(-sl, sl);

                for _ in 0..n {
                    c.append_point(p, true);
                }
            } else if rng.next() % 4 == 0 {
                // a long random walk: the SEGMENT_INDEX paths
                let n = rng.range(64, 200);
                let step = 1.max(s / 20) as i64;
                let mut p = o + rng.pt_r(-sl, sl);

                for _ in 0..n {
                    c.append_point(p, false);
                    p += rng.pt_r(-step, step);
                }
            } else {
                let base = o + rng.pt_r(-sl, sl);
                let n = rng.range(1, 8);

                for _ in 0..n {
                    let d = rng.pt_r(-sl, sl);
                    let dup = rng.flag();
                    c.append_point(base + d, dup);
                }
            }

            let closed = rng.next() % 3 == 0;
            c.set_closed(closed);
            Box::new(c)
        }
        K_CHAIN_ARCS => {
            let mut c = ShapeLineChain::new();
            let p = o + rng.pt_r(-sl, sl);
            c.append_point(p, false);
            let n = rng.range(1, 4);

            for _ in 0..n {
                if rng.flag() {
                    let q = o + rng.pt_r(-sl, sl);
                    c.append_point(q, false);
                } else {
                    let last = c.c_last_point();
                    let d = non_zero(rng, if dg { 3 } else { half(s) });
                    let a = arc_angle(rng);
                    c.append_arc(&safe_arc(last + d, last, a, 0));
                }
            }

            let closed = rng.next() % 3 == 0;
            c.set_closed(closed);
            Box::new(c)
        }
        K_POLYSET => {
            let mut ps = ShapePolySet::new();
            let n_out = if dg { rng.range(0, 1) } else { rng.range(1, 2) };

            for _ in 0..n_out {
                let c = o + rng.pt_r(-sl, sl);
                let r = if dg {
                    1
                } else {
                    rng.range(1, half(s) as i64) as i32
                };

                if rng.flag() {
                    let ring = star_ring(rng, c, r);
                    ps.add_outline(&ring);
                } else {
                    ps.add_outline(&rect_ring(c, r));

                    if r >= 10 && rng.flag() {
                        let hc = c + rng.pt_r((-r / 4) as i64, (r / 4) as i64);
                        ps.add_hole(&rect_ring(hc, r / 5), -1);
                    }
                }
            }

            match rng.next() % 3 {
                0 => {}
                1 => ps.cache_triangulation(false),
                _ => ps.cache_triangulation_with(false, false, true),
            }

            Box::new(ps)
        }
        K_COMPOUND => {
            let mut cmp = ShapeCompound::new();
            let n = if dg { rng.range(0, 1) } else { rng.range(1, 3) };

            for _ in 0..n {
                let k = rng.next() % 8;
                let sub = gen_shape(rng, k, 2.max(s / 2), o, opts);
                cmp.add_shape(sub);
            }

            Box::new(cmp)
        }
        K_TRIANGLE => {
            let mut tri_set = ShapePolySet::new();
            let c = o + rng.pt_r(-sl, sl);
            let r = if dg {
                1
            } else {
                rng.range(1, half(s) as i64) as i32
            };
            let ring = star_ring(rng, c, r);
            tri_set.add_outline(&ring);
            tri_set.cache_triangulation(false);
            let pick = rng.next();

            if tri_set.triangulated_poly_count() > 0
                && tri_set.triangulated_polygon(0).triangle_count() > 0
            {
                let tp = tri_set.triangulated_polygon(0);
                let idx = (pick % tp.triangle_count() as u64) as usize;
                tp.triangles[idx].clone_shape()
            } else {
                Box::new(ShapeNull::new())
            }
        }
        K_ELLIPSE => {
            let c = o + rng.pt_r(-sl, sl);
            let maj = if dg {
                1
            } else {
                rng.range(1, half(s) as i64) as i32
            };
            let mn = rng.range(1, maj as i64) as i32;
            let rot = match rng.next() % 3 {
                0 => 0.0,
                1 => 45.0 * rng.range(-8, 8) as f64,
                _ => rng.dbl(-360.0, 360.0),
            };

            if rng.next() % 3 == 0 {
                let sa = rng.dbl(-360.0, 360.0);
                let ea = sa + rng.dbl(10.0, 350.0);
                Box::new(ShapeEllipse::new_arc(
                    c,
                    maj,
                    mn,
                    deg(rot),
                    deg(sa),
                    deg(ea),
                ))
            } else {
                Box::new(ShapeEllipse::new(c, maj, mn, deg(rot)))
            }
        }
        _ => Box::new(ShapeNull::new()),
    }
}

fn clearance(rng: &mut Rng, s: i32) -> i32 {
    match rng.next() % 6 {
        0 | 1 => 0,
        2 => rng.range(1, 10) as i32,
        _ => rng.range(1, half(s) as i64) as i32,
    }
}

fn rec_collide(r: &mut Rec, a: &dyn Shape, b: &dyn Shape, cl: i32) {
    let mut actual = -7;
    let mut loc = v(11, -13);

    r.b(a.collide(b, cl, None, None));
    r.b(a.collide(b, cl, Some(&mut actual), None)).i(actual);
    actual = -7;
    r.b(a.collide(b, cl, Some(&mut actual), Some(&mut loc)))
        .i(actual)
        .v(loc);
    loc = v(11, -13);
    r.b(a.collide(b, cl, None, Some(&mut loc))).v(loc);

    let mut mtv = v(7, -3);
    r.b(a.collide_mtv(b, cl, Some(&mut mtv))).v(mtv);
}

fn rec_nearest(r: &mut Rec, a: &dyn Shape, b: &dyn Shape) {
    let mut pa = v(5, -9);
    let mut pb = v(-3, 4);
    r.b(a.nearest_points(b, &mut pa, &mut pb)).v(pa).v(pb);
}

fn section_matrix(results: &mut Results, name: &'static str, seed: u64, count: u64, dg: bool) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let ka = rng.next() % K_COUNT;
        let kb = rng.next() % K_COUNT;
        let s = if dg { 20 } else { scale(&mut rng) };
        let opts = Opts {
            degenerate: dg,
            allow_straight: ka != K_ELLIPSE && kb != K_ELLIPSE,
        };

        let a = gen_shape(&mut rng, ka, s, v(0, 0), opts);
        let ob = if rng.flag() {
            v(0, 0)
        } else {
            rng.pt_r(-s as i64, s as i64)
        };
        let b = gen_shape(&mut rng, kb, s, ob, opts);
        let cl = clearance(&mut rng, s);

        let mut r = Rec::default();
        r.i(ka as i64)
            .i(kb as i64)
            .i(s)
            .i(cl)
            .bx(&a.bbox(0))
            .bx(&b.bbox(0));
        rec_collide(&mut r, a.as_ref(), b.as_ref(), cl);
        rec_collide(&mut r, b.as_ref(), a.as_ref(), cl);
        sec.add(&r);
    }
}

fn section_touching(
    results: &mut Results,
    name: &'static str,
    seed: u64,
    count: u64,
    nearest: bool,
) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let ka = rng.next() % K_COUNT;
        let kb = rng.next() % K_COUNT;
        let s = scale(&mut rng);
        let opts = Opts {
            degenerate: false,
            allow_straight: !nearest && ka != K_ELLIPSE && kb != K_ELLIPSE,
        };

        let a = gen_shape(&mut rng, ka, s, v(0, 0), opts);
        let seed_b = rng.next();
        let bb_a = a.bbox(0);
        let bb_b = {
            let mut rb = Rng::new(seed_b);
            gen_shape(&mut rb, kb, s, v(0, 0), opts).bbox(0)
        };

        let j = rng.range(-2, 2);
        let t = rng.range((-s / 2) as i64, (s / 2) as i64);
        let (dx, dy) = match rng.next() % 5 {
            0 => (bb_a.right() as i64 - bb_b.left() as i64 + j, t),
            1 => (bb_a.left() as i64 - bb_b.right() as i64 + j, t),
            2 => (t, bb_a.bottom() as i64 - bb_b.top() as i64 + j),
            3 => (t, bb_a.top() as i64 - bb_b.bottom() as i64 + j),
            _ => {
                let dx = bb_a.right() as i64 - bb_b.left() as i64 + j;
                let dy = bb_a.bottom() as i64 - bb_b.top() as i64 + rng.range(-2, 2);
                (dx, dy)
            }
        };

        let dx = dx.clamp(-100000000, 100000000);
        let dy = dy.clamp(-100000000, 100000000);

        let mut rb = Rng::new(seed_b);
        let b = gen_shape(&mut rb, kb, s, v(dx as i32, dy as i32), opts);
        let cl = clearance(&mut rng, s);

        let mut r = Rec::default();
        r.i(ka as i64)
            .i(kb as i64)
            .i(s)
            .i(cl)
            .bx(&a.bbox(0))
            .bx(&b.bbox(0));

        if nearest {
            rec_nearest(&mut r, a.as_ref(), b.as_ref());
            rec_nearest(&mut r, b.as_ref(), a.as_ref());
        } else {
            rec_collide(&mut r, a.as_ref(), b.as_ref(), cl);
            rec_collide(&mut r, b.as_ref(), a.as_ref(), cl);
        }

        sec.add(&r);
    }
}

fn section_nearest(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let ka = rng.next() % K_COUNT;
        let kb = rng.next() % K_COUNT;
        let s = scale(&mut rng);
        let opts = Opts::default();

        let a = gen_shape(&mut rng, ka, s, v(0, 0), opts);
        let ob = if rng.flag() {
            v(0, 0)
        } else {
            rng.pt_r(-s as i64, s as i64)
        };
        let b = gen_shape(&mut rng, kb, s, ob, opts);

        let mut r = Rec::default();
        r.i(ka as i64)
            .i(kb as i64)
            .i(s)
            .bx(&a.bbox(0))
            .bx(&b.bbox(0));
        rec_nearest(&mut r, a.as_ref(), b.as_ref());
        rec_nearest(&mut r, b.as_ref(), a.as_ref());
        sec.add(&r);
    }
}

fn walk(rng: &mut Rng, start: Vector2I, count: i64, step: i64) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let mut p = start;

    for _ in 0..count {
        c.append_point(p, false);
        p += rng.pt_r(-step, step);
    }

    c
}

fn section_chain_index(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let step = rng.range(1, 100000);
        let na = rng.range(20, 300);
        let nb = rng.range(20, 300);
        let sa = rng.pt_r(-10 * step, 10 * step);
        let mut a = walk(&mut rng, sa, na, step);
        let mut b;

        if rng.next() % 4 == 0 {
            // a shifted copy of a part of A
            let from = rng.range(0, na - 1);
            let cnt = rng.range(1, na - from);
            let sh = rng.pt_r(-2, 2);
            b = ShapeLineChain::new();

            for i in from..from + cnt {
                b.append_point(a.c_point(i as i32) + sh, false);
            }
        } else {
            let sb = rng.pt_r(-10 * step, 10 * step);
            b = walk(&mut rng, sb, nb, step);
        }

        let ca = rng.next() % 4 == 0;
        let cb = rng.next() % 4 == 0;
        a.set_closed(ca);
        b.set_closed(cb);

        let cl = match rng.next() % 5 {
            0 => -1,
            1 => 0,
            2 => rng.range(0, step),
            3 => rng.range(0, 10 * step),
            _ => rng.range(0, 100 * step),
        } as i32;

        let mut r = Rec::default();
        r.i(na).i(b.point_count()).i(cl).b(ca).b(cb);
        rec_collide(&mut r, &a, &b, cl);
        rec_collide(&mut r, &b, &a, cl);
        rec_nearest(&mut r, &a, &b);
        sec.add(&r);
    }
}

/// FIX-FAB-3: `farOrigin()` of the C++ driver.
fn far_origin(rng: &mut Rng) -> Vector2I {
    const C: [i32; 4] = [1200000000, -1200000000, 2000000000, -2000000000];
    let x = C[(rng.next() % 4) as usize];
    let y = C[(rng.next() % 4) as usize];
    v(x, y)
}

/// FIX-FAB-3: `sectionFar()`, touching pairs near the far corners of the coordinate range.
fn section_far(results: &mut Results, name: &'static str, seed: u64, count: u64) {
    let mut sec = Section::new(results, name);

    for n in 0..count {
        let mut rng = Rng::new(seed * 1000003 + n);
        let ka = rng.next() % K_COUNT;
        let kb = rng.next() % K_COUNT;
        let o = far_origin(&mut rng);
        let s = scale(&mut rng).min(200000);
        // no effectively straight arcs (see the C++ driver)
        let opts = Opts {
            degenerate: false,
            allow_straight: false,
        };

        let a = gen_shape(&mut rng, ka, s, o, opts);
        let seed_b = rng.next();
        let bb_a = a.bbox(0);
        let bb_b = {
            let mut rb = Rng::new(seed_b);
            gen_shape(&mut rb, kb, s, o, opts).bbox(0)
        };

        let j = rng.range(-2, 2);
        let t = rng.range((-s / 2) as i64, (s / 2) as i64);
        let (dx, dy) = match rng.next() % 5 {
            0 => (bb_a.right() as i64 - bb_b.left() as i64 + j, t),
            1 => (bb_a.left() as i64 - bb_b.right() as i64 + j, t),
            2 => (t, bb_a.bottom() as i64 - bb_b.top() as i64 + j),
            3 => (t, bb_a.top() as i64 - bb_b.bottom() as i64 + j),
            _ => {
                let dx = bb_a.right() as i64 - bb_b.left() as i64 + j;
                let dy = bb_a.bottom() as i64 - bb_b.top() as i64 + rng.range(-2, 2);
                (dx, dy)
            }
        };

        let dx = dx.clamp(-2000000, 2000000);
        let dy = dy.clamp(-2000000, 2000000);

        let mut rb = Rng::new(seed_b);
        let b = gen_shape(&mut rb, kb, s, o + v(dx as i32, dy as i32), opts);
        let cl = clearance(&mut rng, s);

        let mut r = Rec::default();
        r.i(ka as i64)
            .i(kb as i64)
            .i(s)
            .i(cl)
            .v(o)
            .bx(&a.bbox(0))
            .bx(&b.bbox(0));
        rec_collide(&mut r, a.as_ref(), b.as_ref(), cl);
        rec_collide(&mut r, b.as_ref(), a.as_ref(), cl);
        sec.add(&r);
    }
}

/// FIX-FAB-3: `sectionFarApart()`, shapes on opposite sides of the coordinate range.
fn section_far_apart(results: &mut Results, name: &'static str) {
    let mut sec = Section::new(results, name);

    const FAR: [i32; 5] = [1200000000, -1200000000, 2000000000, -2000000000, 0];
    const RADII: [i32; 4] = [0, 1000, 300000000, 1000000000];

    for xa in FAR {
        for xb in FAR {
            for ra in RADII {
                for rb in RADII {
                    for cl in [0, 100000] {
                        let pa = v(xa, xa / 2);
                        let pb = v(xb, -xb / 3);
                        let ca = ShapeCircle::new(pa, ra);
                        let cb = ShapeCircle::new(pb, rb);
                        let rect = ShapeRect::from_pos_size(pa - v(1000, 1000), 2000, 3000);
                        let seg = ShapeSegment::new(pa, pa + v(5000, -7000), 1000);

                        let mut r = Rec::default();
                        r.i(xa).i(xb).i(ra).i(rb).i(cl);
                        rec_collide(&mut r, &ca, &cb, cl);
                        rec_collide(&mut r, &cb, &ca, cl);
                        rec_collide(&mut r, &rect, &cb, cl);
                        rec_collide(&mut r, &cb, &rect, cl);
                        rec_collide(&mut r, &seg, &cb, cl);
                        rec_collide(&mut r, &cb, &seg, cl);
                        sec.add(&r);
                    }
                }
            }
        }
    }
}

pub fn run_all(results: &mut Results) {
    super::sections::section_libm_probe(results);
    section_matrix(results, "libm:coll_matrix", 600, 100000, false);
    section_touching(results, "libm:coll_touching", 601, 40000, false);
    section_matrix(results, "libm:coll_degenerate", 602, 20000, true);
    section_nearest(results, "libm:coll_nearest", 603, 60000);
    section_touching(results, "libm:coll_nearest_touching", 604, 20000, true);
    section_chain_index(results, "coll_chain_index", 605, 5000);
    section_far(results, "libm:coll_far", 606, 20000);
    section_far_apart(results, "libm:coll_far_apart");
}
