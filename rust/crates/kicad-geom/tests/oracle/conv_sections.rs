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

//! Replay of `conv_oracle.cpp` (package G6b): the same inputs through the port, recorded and
//! hashed identically.  Every function mirrors its C++ counterpart draw for draw.

#![allow(clippy::too_many_lines)]

use kicad_geom::cmath;
use kicad_geom::convert_basic_shapes_to_polygon::{
    RoundedCorner, convert_arc_to_polyline, convert_arc_to_polyline_3pt, corner_list_to_polygon,
    transform_arc_to_polygon, transform_circle_to_polygon, transform_circle_to_polygon_chain,
    transform_oval_to_polygon, transform_ring_to_polygon,
    transform_round_chamfered_rect_to_polygon, transform_trapezoid_to_polygon,
};
use kicad_geom::geometry::approximation::{ERROR_INSIDE, ERROR_OUTSIDE, ErrorLoc};
use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::corner_operations::{
    ChamferParams, compute_chamfer_points, compute_dogbone,
};
use kicad_geom::geometry::direction45::Directions;
use kicad_geom::geometry::eda_angle::{DEGREES_T, EdaAngle};
use kicad_geom::geometry::half_line::HalfLine;
use kicad_geom::geometry::intersection::{
    IntersectableGeom, IntersectionContact, IntersectionVisitor,
};
use kicad_geom::geometry::line::Line;
use kicad_geom::geometry::nearest::{NearableGeom, get_nearest_point, get_nearest_point_any};
use kicad_geom::geometry::oval::{convert_to_chain, get_oval_key_points};
use kicad_geom::geometry::point_types::TypedPoint2I;
use kicad_geom::geometry::roundrect::Roundrect;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::geometry::shape_utils as kigeom;
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::trigo::{rotate_point, rotate_point_around};

use super::sections::{Rec, Results, Rng, Section, arc_points};

const K_ERR: [i32; 8] = [1000, 2500, 5000, 10000, 25000, 50000, 1, 100];

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
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

fn rec_pts(r: &mut Rec, pts: &[Vector2I]) {
    r.u(pts.len() as u64);

    for p in pts {
        r.v(*p);
    }
}

fn rec_typed(r: &mut Rec, pts: &[TypedPoint2I]) {
    r.u(pts.len() as u64);

    for p in pts {
        r.v(p.point).i(p.types);
    }
}

fn rec_opt_seg(r: &mut Rec, s: &Option<Seg>) {
    r.b(s.is_some());

    if let Some(s) = s {
        r.seg(s);
    }
}

fn err_of(rng: &mut Rng) -> i32 {
    K_ERR[(rng.next() % 8) as usize]
}

fn loc_of(rng: &mut Rng) -> ErrorLoc {
    if rng.flag() {
        ERROR_INSIDE
    } else {
        ERROR_OUTSIDE
    }
}

fn size_nm(rng: &mut Rng) -> i32 {
    match rng.next() % 4 {
        0 => rng.range(100000, 10000000) as i32,
        1 => rng.range(1, 1000) as i32,
        2 => 1000 * rng.range(100, 10000) as i32,
        _ => rng.range(50000, 5000000) as i32 * 2 + 1,
    }
}

fn rot_deg(rng: &mut Rng) -> f64 {
    match rng.next() % 5 {
        0 => 90.0 * rng.range(-4, 4) as f64,
        1 => 45.0 * rng.range(-8, 8) as f64,
        2 => rng.range(-3600, 3600) as f64 / 10.0,
        3 => rng.dbl(-360.0, 360.0),
        _ => rng.angle_deg(),
    }
}

fn min_seg_of(rng: &mut Rng) -> i32 {
    let k = rng.range(0, 4) as i32;
    if k == 4 { 7 } else { k * 16 }
}

fn section_circle(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let c = rng.pt_mid();
        let r = match rng.next() % 5 {
            0 => rng.range(0, 10) as i32,
            1 => rng.range(50000, 5000000) as i32,
            2 => rng.range(1, 50000000) as i32,
            3 => -(rng.range(1, 1000) as i32),
            _ => rng.range(1000000, 10000000) as i32,
        };

        let err = err_of(&mut rng);
        let loc = loc_of(&mut rng);
        let min_seg = min_seg_of(&mut rng);

        let mut rec = Rec::default();
        rec.v(c).i(r).i(err).i(loc as i32).i(min_seg);

        let mut chain = ShapeLineChain::new();

        if rng.flag() {
            chain.append_point(v(7, 7), false);
        }

        transform_circle_to_polygon_chain(&mut chain, c, r, err, loc, min_seg);
        rec_chain(&mut rec, &chain);

        let mut ps = ShapePolySet::new();

        if rng.flag() {
            ps.new_outline();
            ps.append(1, 2, -1, -1, false);
            ps.append(3, 4, -1, -1, false);
            ps.append(5, 7, -1, -1, false);
        }

        transform_circle_to_polygon(&mut ps, c, r, err, loc, min_seg);
        rec_poly(&mut rec, &ps);

        let w = rng.range(0, 2 * r.abs() as i64 + 10) as i32;

        let mut ring = ShapePolySet::new();
        transform_ring_to_polygon(&mut ring, c, r, w, err, loc);
        rec.i(w);
        rec_poly(&mut rec, &ring);

        let mut sc = ShapePolySet::new();
        ShapeCircle::new(c, r).transform_to_polygon(&mut sc, err, loc);
        rec_poly(&mut rec, &sc);

        sec.add(&rec);
    }
}

fn other_end(rng: &mut Rng, a: Vector2I) -> Vector2I {
    match rng.next() % 5 {
        0 => a,
        1 => a + rng.pt_r(-10, 10),
        2 => {
            let len = rng.range(0, 20000000) as i32;
            let ang = rot_deg(rng);
            let mut d = v(len, 0);
            rotate_point(&mut d, deg(ang));
            a + d
        }
        3 => {
            let len = rng.range(-10000000, 10000000) as i32;
            if rng.flag() {
                a + v(len, 0)
            } else {
                a + v(0, len)
            }
        }
        _ => a + rng.pt_r(-10000000, 10000000),
    }
}

fn section_oval(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let a = rng.pt_mid();
        let b = other_end(&mut rng, a);
        let w = size_nm(&mut rng);
        let err = err_of(&mut rng);
        let loc = loc_of(&mut rng);
        let min_seg = min_seg_of(&mut rng);

        let mut rec = Rec::default();
        rec.v(a).v(b).i(w).i(err).i(loc as i32).i(min_seg);

        let mut ps = ShapePolySet::new();
        transform_oval_to_polygon(&mut ps, a, b, w, err, loc, min_seg);
        rec_poly(&mut rec, &ps);

        let mut ps2 = ShapePolySet::new();
        ShapeSegment::new(a, b, w).transform_to_polygon(&mut ps2, err, loc);
        rec_poly(&mut rec, &ps2);

        sec.add(&rec);
    }
}

fn section_rect(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let pos = rng.pt_mid();
        let sx = size_nm(&mut rng);
        let sy = size_nm(&mut rng);
        let rot_d = rot_deg(&mut rng);
        let rot = deg(rot_d);
        let mode = (rng.next() % 3) as i32;
        let min_side = sx.min(sy);

        let mut inflate = match rng.next() % 4 {
            0 => 0,
            1 => rng.range(1, 2000000) as i32,
            2 => -(rng.range(1, (min_side / 2 + 10) as i64) as i32),
            _ => -(rng.range(1, 1000) as i32),
        };

        let err = err_of(&mut rng);
        let loc = loc_of(&mut rng);

        let mut rec = Rec::default();
        rec.v(pos)
            .i(sx)
            .i(sy)
            .d(rot_d)
            .i(mode)
            .i(inflate)
            .i(err)
            .i(loc as i32);

        let mut ps = ShapePolySet::new();

        if mode == 0 {
            let radius = match rng.next() % 4 {
                0 => 0,
                1 => rng.range(0, (min_side / 2) as i64) as i32,
                2 => {
                    let f = rng.dbl(0.0, 0.5);
                    (min_side as f64 * f) as i32
                }
                _ => rng.range(0, min_side as i64) as i32,
            };

            let ratio = match rng.next() % 3 {
                0 => 0.0,
                1 => rng.dbl(0.0, 0.5),
                _ => {
                    if rng.flag() {
                        0.5
                    } else {
                        0.25
                    }
                }
            };

            let corners = rng.range(0, 15) as i32;

            rec.i(radius).d(ratio).i(corners);
            transform_round_chamfered_rect_to_polygon(
                &mut ps,
                pos,
                v(sx, sy),
                rot,
                radius,
                ratio,
                corners,
                inflate,
                err,
                loc,
            );
        } else if mode == 1 {
            let mut dx = 0;
            let mut dy = 0;

            match rng.next() % 3 {
                0 => dx = rng.range(-((sy / 2) as i64), (sy / 2) as i64) as i32,
                1 => dy = rng.range(-((sx / 2) as i64), (sx / 2) as i64) as i32,
                _ => {}
            }

            rec.i(dx).i(dy);

            // horizontal / vertical trapezoids with a size of 1 divide by zero when shrunk
            if inflate < 0 && ((dx != 0 && sx < 2) || (dy != 0 && sy < 2)) {
                inflate = 0;
            }

            // Inflating a needle-like trapezoid overflows int at its acute corners (undefined
            // behaviour in C++: `( int )( aInflate / tanAngle2 )` out of range)
            if inflate > 0 && sx.min(sy) * 50 < sx.max(sy) {
                inflate = 0;
            }

            transform_trapezoid_to_polygon(&mut ps, pos, v(sx, sy), rot, dx, dy, inflate, err, loc);
        } else {
            // A convex, clockwise (in KiCad's y-down frame) corner list on a circle
            let cnt = rng.range(3, 8) as i32;
            let rad = size_nm(&mut rng);
            let a0 = rng.dbl(0.0, 360.0);
            let infl = if rng.flag() {
                0
            } else {
                rng.range(0, 1000000) as i32
            };

            let mut corners: Vec<RoundedCorner> = Vec::new();

            for k in 0..cnt {
                let jitter = rng.dbl(-0.3, 0.3);
                let ang =
                    (a0 + (k as f64 + jitter) * 360.0 / cnt as f64) * std::f64::consts::PI / 180.0;
                let x: i32 = ki_round(rad as f64 * cmath::cos(ang));
                let y: i32 = ki_round(rad as f64 * cmath::sin(ang));
                let cr = if rng.flag() {
                    0
                } else {
                    rng.range(0, (rad / 2) as i64) as i32
                };
                corners.push(RoundedCorner::with_radius(x, y, cr));
            }

            let mut dup = false;

            for k in 0..cnt as usize {
                if corners[k].position == corners[(k + 1) % cnt as usize].position {
                    dup = true;
                }
            }

            rec.i(cnt).i(rad).d(a0).i(infl).b(dup);

            if !dup {
                corner_list_to_polygon(&mut ps, &corners, infl, err, loc);
            }
        }

        rec_poly(&mut rec, &ps);
        sec.add(&rec);
    }
}

fn section_arc(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let c = rng.pt_r(-100000000, 100000000);
        let r = match rng.next() % 4 {
            0 => rng.range(1, 1000) as i32,
            1 => rng.range(100000, 10000000) as i32,
            2 => rng.range(10000000, 100000000) as i32,
            _ => rng.range(0, 10) as i32,
        };

        let sa = rot_deg(&mut rng);
        let aa = match rng.next() % 5 {
            0 => rng.dbl(-360.0, 360.0),
            1 => 90.0 * rng.range(-4, 4) as f64,
            2 => rng.dbl(-1.0, 1.0),
            3 => {
                if rng.flag() {
                    360.0
                } else {
                    -360.0
                }
            }
            _ => rng.range(-3600, 3600) as f64 / 10.0,
        };

        let acc_i = err_of(&mut rng);
        let acc = acc_i as f64;
        let loc = loc_of(&mut rng);

        let mut rec = Rec::default();
        rec.v(c).i(r).d(sa).d(aa).i(acc_i).i(loc as i32);

        let mut chain = ShapeLineChain::new();
        let cnt = convert_arc_to_polyline(&mut chain, c, r, deg(sa), deg(aa), acc, loc);
        rec.i(cnt);
        rec_chain(&mut rec, &chain);

        let (s, m, e) = if rng.flag() {
            arc_points(&mut rng)
        } else {
            let mut st = c + v(r, 0);
            rotate_point_around(&mut st, c, deg(sa));
            let arc = ShapeArc::from_center_start_angle(c, st, deg(aa), 0);
            (arc.get_p0(), arc.get_arc_mid(), arc.get_p1())
        };

        let radial_offset = match rng.next() % 3 {
            0 => 0.0,
            1 => rng.dbl(-(r as f64), r as f64),
            _ => rng.range(0, 1000000) as f64,
        };

        let width = if rng.flag() {
            size_nm(&mut rng)
        } else {
            rng.range(0, 2000000) as i32
        };
        let err = err_of(&mut rng);

        rec.v(s).v(m).v(e).d(radial_offset).i(width).i(err);

        let mut chain2 = ShapeLineChain::new();
        let cnt2 = convert_arc_to_polyline_3pt(&mut chain2, s, m, e, acc, loc, radial_offset);
        rec.i(cnt2);
        rec_chain(&mut rec, &chain2);

        let mut ps = ShapePolySet::new();
        transform_arc_to_polygon(&mut ps, s, m, e, width, err, loc);
        rec_poly(&mut rec, &ps);

        let mut ps2 = ShapePolySet::new();
        ShapeArc::from_start_mid_end(s, m, e, width).transform_to_polygon(&mut ps2, err, loc);
        rec_poly(&mut rec, &ps2);

        sec.add(&rec);
    }
}

fn signed_size(rng: &mut Rng) -> i32 {
    match rng.next() % 4 {
        0 => size_nm(rng),
        1 => -size_nm(rng),
        2 => rng.range(0, 3) as i32,
        _ => size_nm(rng),
    }
}

fn section_roundrect(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let p0 = rng.pt_mid();
        let w = signed_size(&mut rng);
        let h = signed_size(&mut rng);
        let min_d = w.abs().min(h.abs());
        let max_d = w.abs().max(h.abs());

        let radius = match rng.next() % 5 {
            0 => 0,
            1 => rng.range(0, (min_d / 2) as i64) as i32,
            2 => min_d / 2,
            3 => rng.range(-1000, 2 * max_d as i64) as i32,
            _ => rng.range(0, w.abs() as i64) as i32,
        };

        let norm = rng.flag();
        let max_err = err_of(&mut rng);
        let loc = loc_of(&mut rng);
        let outset = rng.range(-1000000, 1000000) as i32;
        let prefill = rng.flag();

        let mut rec = Rec::default();
        rec.v(p0)
            .i(w)
            .i(h)
            .i(radius)
            .b(norm)
            .i(max_err)
            .i(loc as i32)
            .i(outset);

        let rec_rr = |rec: &mut Rec, rr: &Roundrect| {
            rec.i(rr.get_round_radius())
                .v(rr.get_position())
                .i(rr.get_width())
                .i(rr.get_height());
            rec.i(rr.get_rect().get_radius()).bx(&rr.bbox());

            let mut buf = ShapePolySet::new();

            if prefill {
                buf.new_outline();
                buf.append(1, 1, -1, -1, false);
                buf.append(2, 1, -1, -1, false);
                buf.append(2, 2, -1, -1, false);
            }

            rr.transform_to_polygon(&mut buf, max_err);
            rec_poly(rec, &buf);
        };

        let rect = ShapeRect::from_pos_size(p0, w, h);

        rec_rr(&mut rec, &Roundrect::new(rect.clone(), radius, norm));
        rec_rr(&mut rec, &Roundrect::outset_from(&rect, outset));
        rec_rr(
            &mut rec,
            &Roundrect::new(rect.clone(), radius, norm).get_inflated(outset),
        );

        let mut rr = ShapeRect::from_pos_size(p0, w, h);
        rr.set_radius(radius);

        let mut buf2 = ShapePolySet::new();
        rr.transform_to_polygon(&mut buf2, max_err, loc);
        rec_poly(&mut rec, &buf2);

        if buf2.outline_count() > 0 {
            rec_chain(&mut rec, &rr.outline());
        }

        sec.add(&rec);
    }
}

fn oriented_seg(rng: &mut Rng, corner: Vector2I, other: Vector2I) -> Seg {
    if rng.flag() {
        Seg::new(corner, other)
    } else {
        Seg::new(other, corner)
    }
}

fn section_corner_ops(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let c = rng.pt_r(-10000000, 10000000);
        let kind = (rng.next() % 5) as i32;

        let (pa, pb) = match kind {
            0 => {
                let la = rng.range(-1000000, 1000000) as i32;
                let lb = rng.range(-1000000, 1000000) as i32;
                (c + v(la, 0), c + v(0, lb))
            }
            1 => {
                let k = rng.range(-5, 5) as i32;
                let pa = c + rng.pt_r(-100000, 100000);
                (pa, c - (pa - c) * k)
            }
            2 => {
                let pa = c + rng.pt_r(-10, 10);
                let pb = c + rng.pt_r(-1000000, 1000000);
                (pa, pb)
            }
            _ => {
                let pa = c + rng.pt_r(-1000000, 1000000);
                let pb = c + rng.pt_r(-1000000, 1000000);
                (pa, pb)
            }
        };

        let seg_a = oriented_seg(&mut rng, c, pa);
        let mut seg_b = oriented_seg(&mut rng, c, pb);

        if rng.next() % 8 == 0 {
            seg_b = Seg::new(pb, pb + rng.pt_r(-1000, 1000)); // not coterminous (usually)
        }

        let setback_a = if rng.flag() {
            0
        } else {
            rng.range(0, 1200000) as i32
        };
        let setback_b = if rng.flag() {
            0
        } else {
            rng.range(0, 1200000) as i32
        };
        let params = ChamferParams {
            chamfer_setback_a: setback_a,
            chamfer_setback_b: setback_b,
        };

        let radius = rng.range(1, 1000000) as i32;
        let slots = rng.flag();

        let mut rec = Rec::default();
        rec.seg(&seg_a)
            .seg(&seg_b)
            .i(params.chamfer_setback_a)
            .i(params.chamfer_setback_b);
        rec.i(radius).b(slots);

        let ch = compute_chamfer_points(&seg_a, &seg_b, &params);
        rec.b(ch.is_some());

        if let Some(ch) = &ch {
            rec.seg(&ch.chamfer);
            rec_opt_seg(&mut rec, &ch.updated_seg_a);
            rec_opt_seg(&mut rec, &ch.updated_seg_b);
        }

        let db = compute_dogbone(&seg_a, &seg_b, radius, slots);
        rec.b(db.is_some());

        if let Some(db) = &db {
            rec_arc(&mut rec, &db.arc);
            rec_opt_seg(&mut rec, &db.updated_seg_a);
            rec_opt_seg(&mut rec, &db.updated_seg_b);
            rec.b(db.small_arc_mouth);
        }

        sec.add(&rec);
    }
}

fn random_ring(rng: &mut Rng) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    let cnt = rng.range(0, 10) as i32;
    let scale = if rng.flag() { 1000 } else { 1000000 };

    for _ in 0..cnt {
        let x = rng.range(-5, 5) as i32;
        let y = rng.range(-5, 5) as i32;
        c.append_point(v(x * scale, y * scale), false);
    }

    c.set_closed(rng.flag());
    c
}

fn direction_of(d: i32) -> Directions {
    match d {
        0 => Directions::N,
        1 => Directions::NE,
        2 => Directions::E,
        3 => Directions::SE,
        4 => Directions::S,
        5 => Directions::SW,
        6 => Directions::W,
        7 => Directions::NW,
        8 => Directions::Last,
        _ => Directions::Undefined,
    }
}

fn section_shape_utils(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let pos = rng.pt_r(-100000000, 100000000);
        let bw = rng.range(0, 10000000) as i32;
        let bh = rng.range(0, 10000000) as i32;
        let bx = Box2I::new(pos, v(bw, bh));
        let dir = rng.range(-1, 8) as i32;
        let outset = rng.range(-1000, 1000000) as i32;
        let rad = rng.range(0, 1000000) as i32;
        let d = direction_of(dir);

        let mut rec = Rec::default();
        rec.bx(&bx).i(dir).i(outset).i(rad);

        rec_chain(&mut rec, &kigeom::box_to_line_chain(&bx));

        let mut corners = Vec::new();
        kigeom::collect_box_corners(&bx, &mut corners);
        rec_pts(&mut rec, &corners);

        let segs = kigeom::get_segs_in_direction(&bx, d);
        rec.u(segs.len() as u64);

        for s in &segs {
            rec.seg(s);
        }

        let rw = rng.range(-1000000, 1000000) as i32;
        let rh = rng.range(-1000000, 1000000) as i32;
        let rect = ShapeRect::from_pos_size(pos, rw, rh);
        rec.v(kigeom::get_point(&rect, d, outset));

        rec_arc(&mut rec, &kigeom::make_arc_cw90(pos, rad, d));
        rec_arc(&mut rec, &kigeom::make_arc_cw180(pos, rad, d));

        let mut sg = rng.seg();
        let q = rng.pt_mid();

        if rng.flag() {
            sg.b = sg.a;
        }

        rec.seg(&sg).v(q).d(kigeom::parameter_along(
            &Seg::new(sg.a / 4, sg.b / 4),
            q / 4,
        ));
        rec.seg(&kigeom::normalised_seg(&sg));
        let which = if rng.flag() { sg.a } else { q };
        rec.v(kigeom::get_other_end(&sg, which));

        let sg2 = if rng.flag() {
            Seg::new(sg.b, q)
        } else {
            let o = if rng.flag() { sg.a } else { q };
            Seg::new(q, o)
        };
        rec.opt(kigeom::get_shared_endpoint(&sg, &sg2));

        let inc = rng.flag();
        rec_typed(
            &mut rec,
            &kigeom::get_circle_key_points(&Circle::new(pos, rad), inc),
        );

        let ring = random_ring(&mut rng);
        rec_chain(&mut rec, &kigeom::rectify_polygon(&ring));

        let mut hp = ShapePolySet::new();
        hp.new_outline();
        hp.append(-10000000, -10000000, -1, -1, false);
        hp.append(10000000, -10000000, -1, -1, false);
        hp.append(10000000, 10000000, -1, -1, false);
        hp.append(-10000000, 10000000, -1, -1, false);
        rec.b(kigeom::add_hole_if_valid(&mut hp, ring));
        rec_poly(&mut rec, &hp);

        let n_sides = rng.range(0, 12) as usize;
        let pt0 = pos + rng.pt_r(-1000000, 1000000);
        let across = rng.flag();
        let ang_d = rot_deg(&mut rng);
        let n_sides2 = if n_sides < 3 { 3 } else { n_sides }; // across flats of 1 or 2 sides divides by cos( 90 deg )

        rec_pts(
            &mut rec,
            &kigeom::make_regular_polygon_points(pos, n_sides, pt0),
        );
        rec_pts(
            &mut rec,
            &kigeom::make_regular_polygon_points_radius(pos, n_sides2, rad, across, deg(ang_d)),
        );

        let cross_size = rng.pt_r(0, 1000000);
        let cross = kigeom::make_cross_segments(pos, cross_size, deg(ang_d));
        rec.u(cross.len() as u64);

        for s in &cross {
            rec.seg(s);
        }

        // Ovals
        let oa = rng.pt_mid();
        let ob = other_end(&mut rng, oa);
        let ow = size_nm(&mut rng) + 1;
        let flags = rng.range(0, 255) as u32;
        let oval = ShapeSegment::new(oa, ob, ow);

        rec.v(oa).v(ob).i(ow).u(flags as u64);
        rec_typed(&mut rec, &get_oval_key_points(&oval, flags));

        if oa != ob {
            rec_chain(&mut rec, &convert_to_chain(&oval));
        }

        sec.add(&rec);
    }
}

fn gcoord(rng: &mut Rng, grid: bool) -> i32 {
    if grid {
        return 100 * rng.range(-10, 10) as i32;
    }

    rng.range(-10000000, 10000000) as i32
}

fn gpt(rng: &mut Rng, grid: bool) -> Vector2I {
    let x = gcoord(rng, grid);
    let y = gcoord(rng, grid);
    v(x, y)
}

fn gradius(rng: &mut Rng, grid: bool) -> i32 {
    if grid {
        return 100 * rng.range(1, 10) as i32;
    }

    rng.range(1, 5000000) as i32
}

fn sweep_deg(rng: &mut Rng) -> f64 {
    match rng.next() % 3 {
        0 => {
            let k = rng.range(1, 4) as f64;
            90.0 * k * if rng.flag() { 1.0 } else { -1.0 }
        }
        1 => {
            let a = rng.dbl(1.0, 359.0);
            a * if rng.flag() { 1.0 } else { -1.0 }
        }
        _ => 45.0 * rng.range(1, 7) as f64,
    }
}

fn random_ellipse(rng: &mut Rng, grid: bool) -> ShapeEllipse {
    let c = gpt(rng, grid);
    let a = gradius(rng, grid);
    let b = gradius(rng, grid);
    let rot = if grid {
        45.0 * rng.range(-4, 4) as f64
    } else {
        rot_deg(rng)
    };

    if rng.flag() {
        return ShapeEllipse::new(c, a, b, deg(rot));
    }

    let sa = rng.range(-720, 720) as f64;
    let ea = sa + sweep_deg(rng);
    ShapeEllipse::new_arc(c, a, b, deg(rot), deg(sa), deg(ea))
}

fn random_arc(rng: &mut Rng, grid: bool) -> ShapeArc {
    let c = gpt(rng, grid);
    let r = gradius(rng, grid);
    let sa = if grid {
        45.0 * rng.range(-8, 8) as f64
    } else {
        rot_deg(rng)
    };
    let aa = sweep_deg(rng);
    let mut st = c + v(r, 0);
    rotate_point_around(&mut st, c, deg(sa));
    let arc = ShapeArc::from_center_start_angle(c, st, deg(aa), 0);

    // Arcs whose rounded points are collinear get a clamped centre near the int limits, which
    // overflows int in the intersection code (undefined behaviour in C++)
    if arc.get_radius() > 2.0 * r as f64 + 1000.0 {
        return ShapeArc::from_center_start_angle(c, c + v(r, 0), EdaAngle::new(90.0, DEGREES_T), 0);
    }

    arc
}

fn random_geom(rng: &mut Rng, grid: bool) -> (IntersectableGeom, i32) {
    let kind = (rng.next() % 7) as i32;

    let g = match kind {
        0 => {
            let a = gpt(rng, grid);
            let mut b = gpt(rng, grid);
            if a == b {
                b.x += 100;
            }
            IntersectableGeom::Line(Line::new(a, b))
        }
        1 => {
            let a = gpt(rng, grid);
            let mut b = gpt(rng, grid);
            if a == b {
                b.y += 100;
            }
            IntersectableGeom::HalfLine(HalfLine::new(a, b))
        }
        2 => {
            let a = gpt(rng, grid);
            let b = gpt(rng, grid);
            IntersectableGeom::Seg(Seg::new(a, b))
        }
        3 => {
            let c = gpt(rng, grid);
            let r = gradius(rng, grid);
            IntersectableGeom::Circle(Circle::new(c, r))
        }
        4 => IntersectableGeom::Arc(random_arc(rng, grid)),
        5 => IntersectableGeom::Ellipse(random_ellipse(rng, grid)),
        _ => {
            let p = gpt(rng, grid);
            let w = if grid {
                100 * rng.range(0, 10) as i32
            } else {
                rng.range(0, 5000000) as i32
            };
            let h = if grid {
                100 * rng.range(0, 10) as i32
            } else {
                rng.range(0, 5000000) as i32
            };
            IntersectableGeom::Box(Box2I::new(p, v(w, h)))
        }
    };

    (g, kind)
}

fn to_nearable(g: &IntersectableGeom) -> NearableGeom {
    match g {
        IntersectableGeom::Line(x) => NearableGeom::Line(*x),
        IntersectableGeom::HalfLine(x) => NearableGeom::HalfLine(*x),
        IntersectableGeom::Seg(x) => NearableGeom::Seg(*x),
        IntersectableGeom::Circle(x) => NearableGeom::Circle(*x),
        IntersectableGeom::Arc(x) => NearableGeom::Arc(x.clone()),
        IntersectableGeom::Ellipse(x) => NearableGeom::Ellipse(*x),
        IntersectableGeom::Box(x) => NearableGeom::Box(*x),
    }
}

fn section_intersection(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for _ in 0..count {
        let grid = rng.flag();
        let (ga, ka) = random_geom(&mut rng, grid);
        let (gb, kb) = random_geom(&mut rng, grid);

        let mut rec = Rec::default();
        rec.b(grid).i(ka).i(kb);

        let mut pts = Vec::new();
        let mut contact = IntersectionContact::default();

        if rng.flag() {
            pts.push(v(3, 3));
            IntersectionVisitor::new(&gb, &mut pts).visit(&ga);
        } else {
            IntersectionVisitor::with_contact(&gb, &mut pts, &mut contact).visit(&ga);
        }

        rec_pts(&mut rec, &pts);
        rec.b(contact.tangent).b(contact.overlapping);

        // Both directions, accumulating into the same contact
        let mut pts2 = Vec::new();
        IntersectionVisitor::with_contact(&ga, &mut pts2, &mut contact).visit(&gb);
        rec_pts(&mut rec, &pts2);
        rec.b(contact.tangent).b(contact.overlapping);

        // Nearest points
        let q = gpt(&mut rng, grid);
        rec.v(q);
        rec.v(get_nearest_point(&to_nearable(&ga), q));

        let mut list = Vec::new();
        let cnt = rng.range(0, 3) as i32;

        if cnt >= 1 {
            list.push(to_nearable(&ga));
        }

        if cnt >= 2 {
            list.push(to_nearable(&gb));
        }

        if cnt >= 3 {
            list.push(NearableGeom::Point(q + v(100, 0)));
        }

        rec.opt(get_nearest_point_any(&list, q));

        sec.add(&rec);
    }
}

fn rec_ellipse(r: &mut Rec, e: &ShapeEllipse) {
    r.v(e.get_center())
        .i(e.get_major_radius())
        .i(e.get_minor_radius())
        .a(e.get_rotation());
    r.a(e.get_start_angle())
        .a(e.get_end_angle())
        .b(e.is_arc())
        .b(e.is_solid())
        .bx(&e.bbox(0));
}

fn section_ellipse(results: &mut Results, name: &'static str, seed: u64, count: i32) {
    let mut sec = Section::new(results, name);
    let mut rng = Rng::new(seed);

    for n in 0..count {
        let grid = rng.next() % 4 == 0;
        let c = if grid {
            gpt(&mut rng, true)
        } else {
            rng.pt_r(-100000000, 100000000)
        };
        let ctor = (rng.next() % 4) as i32;

        let (a, b) = match rng.next() % 4 {
            0 => {
                let a = rng.range(-5, 5) as i32;
                let b = rng.range(-5, 50) as i32;
                (a, b)
            }
            1 => {
                let a = rng.range(1, 50000000) as i32;
                let b = rng.range(1, 50000000) as i32;
                (a, b)
            }
            2 => {
                let a = rng.range(100000, 5000000) as i32;
                (a, a)
            }
            _ => {
                let a = if grid {
                    gradius(&mut rng, true)
                } else {
                    rng.range(1000, 5000000) as i32
                };
                let b = rng.range(1, 5000000) as i32;
                (a, b)
            }
        };

        let rot = rot_deg(&mut rng);
        let sa = rng.range(-720, 720) as f64 + if rng.flag() { 0.0 } else { rng.dbl(0.0, 1.0) };
        let ea = match rng.next() % 4 {
            0 => sa + sweep_deg(&mut rng),
            1 => sa + 360.0,
            2 => rng.dbl(-720.0, 720.0),
            _ => sa + rng.dbl(-400.0, 400.0),
        };

        let major_end = rng.pt_r(-50000000, 50000000);
        let ratio = rng.dbl(0.0, 1.5);

        let e = match ctor {
            0 => ShapeEllipse::new(c, a, b, deg(rot)),
            1 => ShapeEllipse::new_arc(c, a, b, deg(rot), deg(sa), deg(ea)),
            2 => ShapeEllipse::from_major_endpoint(c, major_end, ratio),
            _ => ShapeEllipse::from_major_endpoint_arc(c, major_end, ratio, deg(sa), deg(ea)),
        };

        let clearance = rng.range(0, 1000) as i32;

        let mut rec = Rec::default();
        rec.v(c)
            .i(ctor)
            .i(a)
            .i(b)
            .d(rot)
            .d(sa)
            .d(ea)
            .v(major_end)
            .d(ratio);
        rec_ellipse(&mut rec, &e);
        rec.bx(&e.bbox(clearance));
        rec.s(&e.format(true)).s(&e.format(false));

        // GetLength() of a large arc runs the adaptive integration to its depth limit (slow)
        if n % 8 == 0 && e.get_major_radius() <= 1000000 {
            rec.d(e.get_length());
        }

        let span = 10.max(2 * e.get_major_radius());

        for _ in 0..3 {
            let p = e.get_center() + rng.pt_r(-(span as i64), span as i64);
            let p2 = if rng.flag() {
                p
            } else {
                e.get_center() + rng.pt_r(-(span as i64), span as i64)
            };
            let acc = if rng.flag() {
                0
            } else {
                rng.range(1, 1000) as i32
            };
            let cl = if rng.flag() {
                0
            } else {
                rng.range(0, 100000) as i32
            };
            let outs = rng.range(0, 3) as i32;

            rec.v(p).v(p2).i(acc).i(cl).i(outs);
            rec.b(e.point_inside(p, acc, false))
                .i(e.squared_distance(p, false))
                .i(e.squared_distance(p, true));
            rec.v(e.nearest_point(p));

            let mut actual = -7;
            let mut loc = v(-7, -7);
            let hit = e.collide_seg(
                &Seg::new(p, p2),
                cl,
                if outs & 1 != 0 {
                    Some(&mut actual)
                } else {
                    None
                },
                if outs & 2 != 0 { Some(&mut loc) } else { None },
            );
            rec.b(hit).i(actual).v(loc);

            let line = rng.flag();
            rec_pts(&mut rec, &e.intersect_seg(&Seg::new(p, p2), line));
        }

        let max_err = match rng.next() % 3 {
            0 => rng.range(-5, 5) as i32,
            1 => err_of(&mut rng),
            _ => 1.max(e.get_major_radius() / 50),
        };

        rec.i(max_err);
        rec_chain(&mut rec, &e.convert_to_polyline(max_err));

        let mut ps = ShapePolySet::new();
        e.transform_to_polygon(&mut ps, max_err, ERROR_INSIDE);
        rec_poly(&mut rec, &ps);

        // Intersections with other curves
        let cc = e.get_center() + rng.pt_r(-(span as i64), span as i64);
        let cr = rng.range(0, span as i64) as i32;
        let circ = Circle::new(cc, cr);
        let arc = random_arc(&mut rng, false);
        let mut other = random_ellipse(&mut rng, false);

        if rng.flag() {
            other.set_center(e.get_center() + rng.pt_r(-(span as i64), span as i64));
        }

        rec.v(circ.center)
            .i(circ.radius)
            .v(arc.get_p0())
            .v(arc.get_arc_mid())
            .v(arc.get_p1());
        rec_ellipse(&mut rec, &other);
        rec_pts(&mut rec, &e.intersect_circle(&circ));
        rec_pts(&mut rec, &e.intersect_arc(&arc));
        rec_pts(&mut rec, &e.intersect_ellipse(&other));

        // Transformations
        let mut t = e;
        let rot_a = rot_deg(&mut rng);
        let rc = rng.pt_r(-10000000, 10000000);
        t.rotate(deg(rot_a), rc);
        rec.d(rot_a).v(rc);
        rec_ellipse(&mut rec, &t);

        let mref = rng.pt_r(-10000000, 10000000);
        let lr = rng.flag();
        t.mirror(
            mref,
            if lr {
                FlipDirection::LeftRight
            } else {
                FlipDirection::TopBottom
            },
        );
        rec.v(mref).b(lr);
        rec_ellipse(&mut rec, &t);

        let mv = rng.pt_r(-1000000, 1000000);
        t.move_(mv);
        rec.v(mv);
        rec_ellipse(&mut rec, &t);

        let nm = rng.range(-10, 5000000) as i32;
        let nn = rng.range(-10, 5000000) as i32;
        let nr = rot_deg(&mut rng);
        let ns = rng.range(-400, 400) as f64;
        let ne = rng.range(-400, 400) as f64;

        t.set_major_radius(nm);
        rec_ellipse(&mut rec, &t);
        t.set_minor_radius(nn);
        rec_ellipse(&mut rec, &t);
        t.set_rotation(deg(nr));
        t.set_start_angle(deg(ns));
        t.set_end_angle(deg(ne));
        rec.i(nm).i(nn).d(nr).d(ns).d(ne);
        rec_ellipse(&mut rec, &t);
        rec.b(t.equals(&e)).b(e.equals(&e));

        sec.add(&rec);
    }
}

/// `KICAD_CONV_ORACLE_ONLY=NAME` runs just that section (to dump a mismatch quickly, like
/// `conv_oracle --only NAME`).
pub fn run_all(results: &mut Results) {
    let only = std::env::var("KICAD_CONV_ORACLE_ONLY").ok();
    let run = |name: &str| only.as_deref().is_none_or(|o| o == name);

    if run("libm_probe") {
        super::sections::section_libm_probe(results);
    }

    type SectionFn = fn(&mut Results, &'static str, u64, i32);
    let sections: [(SectionFn, &'static str, u64, i32); 9] = [
        (section_circle, "libm:conv_circle", 600, 20000),
        (section_oval, "libm:conv_oval", 601, 20000),
        (section_rect, "libm:conv_rect", 602, 30000),
        (section_arc, "libm:conv_arc", 603, 20000),
        (section_roundrect, "libm:conv_roundrect", 604, 20000),
        (section_corner_ops, "libm:conv_corner_ops", 605, 20000),
        (section_shape_utils, "libm:conv_shape_utils", 606, 20000),
        (section_intersection, "libm:intersection", 607, 30000),
        (section_ellipse, "libm:shape_ellipse", 608, 30000),
    ];

    for (f, name, seed, count) in sections {
        if run(name) {
            f(results, name, seed, count);
        }
    }
}
