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

//! The Rust side of `shapes_oracle.cpp`: the same pseudo-random cases, computed with the port.
//! Every function mirrors the C++ function of the same name (random values are drawn in the
//! same order).

use kicad_geom::geometry::arc_chord_params::ArcChordParams;
use kicad_geom::geometry::arc_construction::{
    ArcSolution, arc_from_start_end_center_drag, arc_from_start_end_mid_drag, arc_from_start_tangent_end,
    arc_through_points, project_to_chord_bisector,
};
use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::eda_angle::{DEGREES_T, EdaAngle};
use kicad_geom::geometry::geometry_utils::{
    box_hit_test_box, box_hit_test_point, circle_to_end_segment_delta_radius, clip_line,
    get_arc_to_segment_count, get_circle_to_poly_correction, get_clamped_coords, get_clamped_coords_to_i,
    get_vector_snapped45, get_vector_snapped90, is_vec2_safe_xy,
};
use kicad_geom::geometry::half_line::HalfLine;
use kicad_geom::geometry::line::Line;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeLineChainBase, ShapeRef, ShapeType};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_compound::ShapeCompound;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_null::ShapeNull;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::geometry::shape_simple::ShapeSimple;
use kicad_geom::geometry::shape_utils::{clip_half_line_to_box, clip_line_to_box};
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::box2_minmax::Box2IMinMax;
use kicad_geom::math::vector2::{Vector2D, Vector2I};

use super::sections::{Rec, Results, Rng, Section, arc_points, seg_pair};

const LARGE_RADIUS: f64 = i32::MAX as f64 / 2.0;

fn deg(d: f64) -> EdaAngle {
    EdaAngle::new(d, DEGREES_T)
}

fn width(rng: &mut Rng) -> i32 {
    match rng.next() % 4 {
        0 => 0,
        1 => rng.range(1, 10) as i32,
        2 => rng.range(0, 1000000) as i32,
        _ => rng.range(0, 5000000) as i32,
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

fn rand_arc(rng: &mut Rng) -> ShapeArc {
    match rng.next() % 5 {
        0 | 1 => {
            let (s, m, e) = arc_points(rng);
            let w = width(rng);
            ShapeArc::from_start_mid_end(s, m, e, w)
        }
        2 => {
            let c = rng.pt_mid();
            let s = rng.pt_mid();
            let a = rng.angle_deg();
            let w = width(rng);
            ShapeArc::from_center_start_angle(c, s, deg(a), w)
        }
        3 => {
            let c = rng.pt_r(-1000, 1000);
            let d = rng.pt_r(-50, 50);
            let a = rng.angle_deg();
            let w = rng.range(0, 20) as i32;
            ShapeArc::from_center_start_angle(c, c + d, deg(a), w)
        }
        _ => {
            let s = rng.pt_mid();
            let e = rng.pt_mid();
            let a = rng.angle_deg();
            let w = width(rng);
            let mut arc = ShapeArc::new();
            arc.construct_from_start_end_angle(s, e, deg(a), w as f64);
            arc
        }
    }
}

fn near_arc(rng: &mut Rng, a_arc: &ShapeArc) -> Vector2I {
    match rng.next() % 5 {
        0 => {
            let d = rng.pt_r(-1000, 1000);
            a_arc.get_p0() + d
        }
        1 => {
            let d = rng.pt_r(-1000, 1000);
            a_arc.get_p1() + d
        }
        2 => {
            let d = rng.pt_r(-100000, 100000);
            a_arc.get_arc_mid() + d
        }
        3 => {
            let d = rng.pt_r(-10, 10);
            if a_arc.get_radius() < LARGE_RADIUS {
                a_arc.get_center() + d
            } else {
                a_arc.get_arc_mid() + d
            }
        }
        _ => rng.pt_mid(),
    }
}

fn fmt(a: &dyn Shape, a_cplusplus: bool) -> String {
    if a.type_() == ShapeType::LineChain { String::new() } else { a.format(a_cplusplus) }
}

fn rec_arc<'r>(r: &'r mut Rec, a: &ShapeArc) -> &'r mut Rec {
    r.v(a.get_p0()).v(a.get_arc_mid()).v(a.get_p1()).i(a.get_width()).v(a.get_center());
    r.d(a.get_radius()).bx(&a.bbox(0))
}

fn rec_pts<'r>(r: &'r mut Rec, a_pts: &[Vector2I]) -> &'r mut Rec {
    r.i(a_pts.len() as i64);

    for p in a_pts {
        r.v(*p);
    }

    r
}

fn rec_chain<'r>(r: &'r mut Rec, c: &ShapeLineChain) -> &'r mut Rec {
    r.i(c.point_count()).b(c.is_closed()).i(c.arc_count() as i64);

    for i in 0..c.point_count() {
        r.v(c.c_point(i)).i(c.c_shapes()[i as usize].0 as i64).i(c.c_shapes()[i as usize].1 as i64);
    }

    r
}

fn rand_circle(rng: &mut Rng) -> Circle {
    let c = rng.pt_mid();
    let r = match rng.next() % 3 {
        0 => rng.range(0, 20) as i32,
        1 => rng.range(0, 100000) as i32,
        _ => rng.range(0, 100000000) as i32,
    };

    Circle::new(c, r)
}

fn seg_near(rng: &mut Rng, a_p: Vector2I) -> Seg {
    let a = rng.pt_r(-2000000, 2000000);
    let b = rng.pt_r(-2000000, 2000000);

    if rng.flag() {
        return Seg::new(a_p + a, a_p + b);
    }

    let c = rng.pt_mid();
    Seg::new(a_p + a, c)
}

fn rand_simple(rng: &mut Rng) -> ShapeSimple {
    let base = rng.pt_mid();
    let n = rng.range(3, 8) as i32;
    let spread: i64 = if rng.flag() { 1000 } else { 3000000 };
    let mut s = ShapeSimple::new();

    for _ in 0..n {
        let d = rng.pt_r(-spread, spread);
        s.append(base + d);
    }

    s
}

fn rand_chain(rng: &mut Rng) -> ShapeLineChain {
    let base = rng.pt_mid();
    let n = rng.range(1, 8) as i32;
    let mut c = ShapeLineChain::new();

    for _ in 0..n {
        let d = rng.pt_r(-3000000, 3000000);
        let dup = rng.flag();
        c.append_point(base + d, dup);
    }

    let closed = rng.flag();
    c.set_closed(closed);
    c
}

fn rand_shape(rng: &mut Rng, a_kind: i32) -> Box<dyn Shape> {
    match a_kind {
        0 => {
            let p = rng.pt_mid();
            let s = rng.pt_r(-5000000, 5000000);
            Box::new(ShapeRect::from_pos_size(p, s.x, s.y))
        }
        1 => {
            let a = rng.pt_mid();
            let d = rng.pt_r(-5000000, 5000000);
            let w = width(rng);
            Box::new(ShapeSegment::new(a, a + d, w))
        }
        2 => {
            let c = rng.pt_mid();
            let r = rng.range(0, 5000000) as i32;
            Box::new(ShapeCircle::new(c, r))
        }
        3 => Box::new(rand_simple(rng)),
        4 => Box::new(ShapeNull::new()),
        5 => {
            let mut cmp = ShapeCompound::new();
            let n = rng.range(1, 3);

            for _ in 0..n {
                let k = if rng.flag() { 1 } else { 2 };
                cmp.add_shape(rand_shape(rng, k));
            }

            Box::new(cmp)
        }
        _ => Box::new(rand_chain(rng)),
    }
}

fn rec_collide(r: &mut Rec, a: &dyn Shape, b: &dyn Shape, cl: i32) {
    let mut actual = -7;
    let mut loc = Vector2I::new(11, -13);

    r.b(a.collide(b, cl, None, None));
    r.b(a.collide(b, cl, Some(&mut actual), None)).i(actual);
    actual = -7;
    r.b(a.collide(b, cl, Some(&mut actual), Some(&mut loc))).i(actual).v(loc);
    loc = Vector2I::new(11, -13);
    r.b(a.collide(b, cl, None, Some(&mut loc))).v(loc);
}

fn rec_collide_seg(r: &mut Rec, a: &dyn Shape, s: &Seg, cl: i32) {
    let mut actual = -7;
    let mut loc = Vector2I::new(11, -13);

    r.b(a.collide_seg(s, cl, None, None));
    r.b(a.collide_seg(s, cl, Some(&mut actual), None)).i(actual);
    actual = -7;
    r.b(a.collide_seg(s, cl, Some(&mut actual), Some(&mut loc))).i(actual).v(loc);
    loc = Vector2I::new(11, -13);
    r.b(a.collide_seg(s, cl, None, Some(&mut loc))).v(loc);
}

fn rec_collide_pt(r: &mut Rec, a: &dyn Shape, p: Vector2I, cl: i32) {
    let mut actual = -7;
    let mut loc = Vector2I::new(11, -13);

    r.b(a.collide_point(p, cl, None, None));
    r.b(a.collide_point(p, cl, Some(&mut actual), None)).i(actual);
    actual = -7;
    r.b(a.collide_point(p, cl, Some(&mut actual), Some(&mut loc))).i(actual).v(loc);
    loc = Vector2I::new(11, -13);
    r.b(a.collide_point(p, cl, None, Some(&mut loc))).v(loc);
}

fn section_circle(results: &mut Results) {
    let mut sec = Section::new(results, "libm:circle");
    let mut rng = Rng::new(201);

    for _ in 0..30000 {
        let c = rand_circle(&mut rng);
        let mut c2 = rand_circle(&mut rng);
        let p = rng.pt();
        let pd = rng.pt_d(-2e8, 2e8);
        let s = seg_near(&mut rng, c.center);

        if rng.next() % 4 == 0 {
            // near-tangent and concentric circles
            let d = rng.pt_r(-3, 3);
            let dr = rng.range(-3, 3) as i32;
            c2 = Circle::new(c.center + d, 0.max(c.radius + dr));
        }

        let mut r = Rec::default();
        r.v(c.center).i(c.radius).v(c2.center).i(c2.radius).v(p).vd(pd).seg(&s);

        r.b(c.contains(p)).b(c.contains_inside(p));
        r.v(c.nearest_point(p)).vd(c.nearest_point_d(pd));
        r.v(c.furthest_point(p)).vd(c.furthest_point_d(pd));
        rec_pts(&mut r, &c.intersect(&c2));
        rec_pts(&mut r, &c.intersect_seg(&s));
        rec_pts(&mut r, &c.intersect_line(&s));
        sec.add(&r);
    }
}

fn section_circle_tan_tan_pt(results: &mut Results) {
    let mut sec = Section::new(results, "libm:circle_tan_tan_pt");
    let mut rng = Rng::new(202);

    for _ in 0..5000 {
        let ip = rng.pt_r(-1000000, 1000000);
        let da = rng.pt_r(-1000000, 1000000);
        let db = rng.pt_r(-1000000, 1000000);
        let p = rng.pt_r(-1000000, 1000000);
        let a = Seg::new(ip, ip + da);
        let mut b = Seg::new(ip, ip + db);

        if rng.next() % 4 == 0 {
            b = Seg::new(ip + db, ip + db + da); // parallel
        }

        let mut circle = Circle::default();
        circle.construct_from_tan_tan_pt(&a, &b, p);

        let mut r = Rec::default();
        r.seg(&a).seg(&b).v(p).v(circle.center).i(circle.radius);
        sec.add(&r);
    }
}

fn section_arc_props(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_props");
    let mut rng = Rng::new(203);

    for _ in 0..30000 {
        let a: ShapeArc = match rng.next() % 6 {
            0 => {
                let ip = rng.pt_r(-1000000, 1000000);
                let da = rng.pt_r(-1000000, 1000000);
                let db = rng.pt_r(-1000000, 1000000);
                let rad = rng.range(0, 3000000) as i32;
                let w = width(&mut rng);
                ShapeArc::from_tangent_segments(&Seg::new(ip, ip + da), &Seg::new(ip, ip + db), rad, w)
            }
            1 => {
                let s = rng.pt_mid();
                let e = rng.pt_mid();
                let c = rng.pt_mid();
                let cw = rng.flag();
                let mut a = ShapeArc::new();
                a.construct_from_start_end_center(s, e, c, cw, 0.0);
                a
            }
            _ => rand_arc(&mut rng),
        };

        let cl = clearance(&mut rng);

        let mut r = Rec::default();
        rec_arc(&mut r, &a).bx(&a.bbox(cl));
        r.a(a.get_central_angle()).a(a.get_start_angle()).a(a.get_end_angle()).d(a.get_length());
        r.b(a.is_ccw()).b(a.is_clockwise()).b(a.is_effective_line()).seg(&a.get_chord());
        rec_arc(&mut r, &a.reversed());

        let mut rev = a.clone();
        rev.reverse();
        rec_arc(&mut r, &rev).a(rev.get_central_angle());

        let w2 = ShapeArc::with_width(&a, 1234);
        r.i(w2.get_width()).b(w2 == a);

        let mm = Box2IMinMax::from_arc(&a);
        r.i(mm.left).i(mm.top).i(mm.right).i(mm.bottom);
        sec.add(&r);
    }
}

fn section_arc_polyline(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_polyline");
    let mut rng = Rng::new(204);

    for _ in 0..12000 {
        let a = rand_arc(&mut rng);
        let mut err = match rng.next() % 4 {
            0 => 5000,
            1 => 1000,
            2 => rng.range(100, 100000) as i32,
            _ => rng.range(-10, 10) as i32,
        };

        // Out of range double to int conversions are undefined behaviour in the C++
        if a.get_radius() + a.get_width() as f64 / 2.0 >= LARGE_RADIUS {
            continue;
        }

        // Keep the polylines reasonably short
        if err < 100 && a.get_radius() > 100000.0 {
            err = 100;
        }

        let (pl, actual) = a.convert_to_polyline_with_error(err);

        let mut r = Rec::default();
        rec_arc(&mut r, &a).i(err).i(actual);
        rec_chain(&mut r, &pl);

        let chain = ShapeLineChain::from_arc(&a, false, None);
        rec_chain(&mut r, &chain).bx(&chain.bbox(0));
        sec.add(&r);
    }
}

fn section_arc_collide(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_collide");
    let mut rng = Rng::new(205);

    for _ in 0..40000 {
        let a = rand_arc(&mut rng);
        let p = near_arc(&mut rng, &a);
        let q = near_arc(&mut rng, &a);
        let cl = clearance(&mut rng);

        let mut s = Seg::new(p, q);

        if rng.next() % 8 == 0 {
            s = Seg::new(p, p);
        }

        let mut r = Rec::default();
        rec_arc(&mut r, &a).v(p).seg(&s).i(cl);
        rec_collide_pt(&mut r, &a, p, cl);
        rec_collide_seg(&mut r, &a, &s, cl);
        sec.add(&r);
    }
}

fn section_arc_intersect(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_intersect");
    let mut rng = Rng::new(206);

    for _ in 0..20000 {
        let a = rand_arc(&mut rng);
        let mut b = rand_arc(&mut rng);

        if rng.flag() {
            // move the other arc close
            let d = rng.pt_r(-2000000, 2000000);
            b.move_(a.get_arc_mid() - b.get_arc_mid() + d);
        }

        let p = near_arc(&mut rng, &a);
        let s0 = near_arc(&mut rng, &a);
        let s = Seg::new(s0, p);
        let mut c = rand_circle(&mut rng);

        if rng.flag() {
            c.center = a.get_arc_mid() + rng.pt_r(-1000000, 1000000);
        }

        let mut r = Rec::default();
        rec_arc(&mut r, &a);
        rec_arc(&mut r, &b).v(p).seg(&s).v(c.center).i(c.radius);

        let mut ips = vec![Vector2I::new(1, 2)];
        r.i(a.intersect_line(&s, &mut ips));
        r.i(a.intersect_circle(&c, &mut ips));
        r.i(a.intersect_arc(&b, &mut ips));
        rec_pts(&mut r, &ips);

        // Out of range double to int conversions are undefined behaviour in the C++
        if a.get_radius() < LARGE_RADIUS && b.get_radius() < LARGE_RADIUS {
            r.v(a.nearest_point(p));

            let mut pa = Vector2I::default();
            let mut pb = Vector2I::default();
            let mut d2: i64 = -1;
            r.b(a.nearest_points_seg(&s, &mut pa, &mut pb, &mut d2)).v(pa).v(pb).i(d2);

            let sc = ShapeCircle::from_circle(c);
            pa = Vector2I::default();
            pb = Vector2I::default();
            d2 = -1;
            r.b(a.nearest_points_circle(&sc, &mut pa, &mut pb, &mut d2)).v(pa).v(pb).i(d2);

            pa = Vector2I::default();
            pb = Vector2I::default();
            d2 = -1;
            r.b(a.nearest_points_arc(&b, &mut pa, &mut pb, &mut d2)).v(pa).v(pb).i(d2);
        }

        sec.add(&r);
    }
}

fn section_arc_transform(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_transform");
    let mut rng = Rng::new(207);

    for _ in 0..10000 {
        let a = rand_arc(&mut rng);
        let v = rng.pt_r(-1000000, 1000000);
        let c = rng.pt_mid();
        let ang = rng.angle_deg();
        let rf = rng.pt_mid();
        let lr = rng.flag();
        let axis = seg_near(&mut rng, rf);

        let mut r = Rec::default();
        rec_arc(&mut r, &a);

        let mut m = a.clone();
        m.move_(v);
        rec_arc(&mut r, &m);

        m = a.clone();
        m.rotate(deg(ang), c);
        rec_arc(&mut r, &m);

        m = a.clone();
        m.mirror(rf, if lr { FlipDirection::LeftRight } else { FlipDirection::TopBottom });
        rec_arc(&mut r, &m);

        m = a.clone();
        m.mirror_seg(&axis);
        rec_arc(&mut r, &m);
        sec.add(&r);
    }
}

fn section_shape_collide(results: &mut Results) {
    let mut sec = Section::new(results, "shape_collide");
    let mut rng = Rng::new(208);

    for _ in 0..60000 {
        let ka = (rng.next() % 7) as i32;
        let kb = (rng.next() % 7) as i32;

        let a = rand_shape(&mut rng, ka);
        let mut b = rand_shape(&mut rng, kb);

        if rng.flag() && kb != 4 && ka != 4 {
            // move b next to a
            let d = rng.pt_r(-3000000, 3000000);
            let delta = a.centre() - b.centre() + d;
            b.move_(delta);
        }

        let cl = clearance(&mut rng);

        let lcb_a = ka == 3 || ka == 6;
        let lcb_b = kb == 3 || kb == 6;

        let mut r = Rec::default();
        r.i(ka).i(kb).s(&fmt(a.as_ref(), true)).s(&fmt(b.as_ref(), true)).i(cl);

        if lcb_a && lcb_b {
            // SHAPE_LINE_CHAIN_BASE against SHAPE_LINE_CHAIN_BASE: package G5
        } else if ka == 0 && kb == 0 {
            r.b(a.collide(b.as_ref(), 0, None, None));
        } else {
            rec_collide(&mut r, a.as_ref(), b.as_ref(), cl);

            // the MTV variants that do not assert
            let mtv_ok = (ka == 2 && kb == 2)
                || (ka == 0 && kb == 2)
                || (ka == 2 && kb == 0)
                || (ka == 2 && lcb_b)
                || (ka == 2 && kb == 1)
                || (ka == 1 && kb == 2);

            if mtv_ok {
                let mut mtv = Vector2I::new(7, -3);
                r.b(a.collide_mtv(b.as_ref(), cl, Some(&mut mtv))).v(mtv);
            }

            r.i(a.get_clearance(b.as_ref()));
        }

        sec.add(&r);
    }
}

fn rand_arc_partner(rng: &mut Rng, a_arc: &ShapeArc) -> (Box<dyn Shape>, i32) {
    let kind = (rng.next() % 6) as i32;

    let mut s: Box<dyn Shape> = match kind {
        0 => rand_shape(rng, 2),
        1 => rand_shape(rng, 1),
        2 => rand_shape(rng, 3),
        3 => rand_shape(rng, 6),
        4 => {
            let mut b = rand_arc(rng);

            if b.get_radius() >= LARGE_RADIUS {
                b = ShapeArc::from_start_mid_end(Vector2I::new(0, 0), Vector2I::new(100, 100), Vector2I::new(200, 0), 0);
            }

            let mut chain = ShapeLineChain::from_arc(&b, false, None);
            let d = rng.pt_r(-3000000, 3000000);
            chain.append_point(b.get_p1() + d, false);
            let closed = rng.flag();
            chain.set_closed(closed);
            Box::new(chain)
        }
        _ => Box::new(rand_arc(rng)),
    };

    if rng.flag() {
        let d = rng.pt_r(-3000000, 3000000);
        let delta = a_arc.get_arc_mid() - s.centre() + d;
        s.move_(delta);
    }

    (s, kind)
}

fn section_shape_collide_arc(results: &mut Results) {
    let mut sec = Section::new(results, "libm:shape_collide_arc");
    let mut rng = Rng::new(209);

    for _ in 0..30000 {
        let a = rand_arc(&mut rng);
        let (b, kind) = rand_arc_partner(&mut rng, &a);
        let cl = clearance(&mut rng);

        // The arcs of a chain must have a radius that converts to int (see sectionArcPolyline);
        // NearestPoints() of large arcs converts out of range radii as well
        if a.get_radius() >= LARGE_RADIUS {
            continue;
        }

        if let ShapeRef::Arc(barc) = b.as_shape_ref() {
            if barc.get_radius() >= LARGE_RADIUS {
                continue;
            }
        }

        let mut r = Rec::default();
        rec_arc(&mut r, &a).i(kind).s(&fmt(b.as_ref(), true)).i(cl);
        rec_collide(&mut r, &a, b.as_ref(), cl);
        rec_collide(&mut r, b.as_ref(), &a, cl);

        if kind == 0 {
            let mut mtv = Vector2I::new(7, -3);
            r.b(a.collide_mtv(b.as_ref(), cl, Some(&mut mtv))).v(mtv);
            mtv = Vector2I::new(7, -3);
            r.b(b.collide_mtv(&a, cl, Some(&mut mtv))).v(mtv);
        }

        if kind != 3 && kind != 4 {
            r.i(a.get_clearance(b.as_ref())).i(b.get_clearance(&a));
        }

        sec.add(&r);
    }

    // effectively straight arcs against rectangles
    for _ in 0..5000 {
        let s = rng.pt_mid();
        let d = rng.pt_r(-1000, 1000);
        let k1 = rng.range(1, 1000) as i32;
        let k2 = rng.range(1, 1000) as i32;
        let w = width(&mut rng);

        let a = ShapeArc::from_start_mid_end(s, s + d * k1, s + d * (k1 + k2), w);

        if !a.is_effective_line() {
            continue;
        }

        let mut b = rand_shape(&mut rng, 0);

        if rng.flag() {
            let dd = rng.pt_r(-3000000, 3000000);
            let delta = a.get_arc_mid() - b.centre() + dd;
            b.move_(delta);
        }

        let cl = clearance(&mut rng);

        let mut r = Rec::default();
        rec_arc(&mut r, &a).s(&b.format(true)).i(cl);
        rec_collide(&mut r, &a, b.as_ref(), cl);
        rec_collide(&mut r, b.as_ref(), &a, cl);
        sec.add(&r);
    }
}

fn section_shape_distance(results: &mut Results) {
    let mut sec = Section::new(results, "shape_distance");
    let mut rng = Rng::new(210);

    for _ in 0..30000 {
        let kind = (rng.next() % 7) as i32;
        let a = rand_shape(&mut rng, if kind == 4 { 6 } else { kind });
        let p = a.centre() + rng.pt_r(-3000000, 3000000);
        let s = seg_near(&mut rng, a.centre());
        let cl = clearance(&mut rng);
        let acc_k = rng.range(0, 3) as i32;
        let acc = acc_k * rng.range(0, 1000) as i32;

        let mut r = Rec::default();
        r.i(kind).s(&fmt(a.as_ref(), true)).s(&fmt(a.as_ref(), false)).v(p).seg(&s).i(cl);
        r.bx(&a.bbox(0)).bx(&a.bbox(cl)).v(a.centre()).b(a.is_solid());
        rec_collide_pt(&mut r, a.as_ref(), p, cl);
        rec_collide_seg(&mut r, a.as_ref(), &s, cl);

        let t = a.type_();

        if t == ShapeType::Simple || t == ShapeType::LineChain || t == ShapeType::Rect {
            r.i(a.squared_distance(p, false)).i(a.squared_distance(p, true)).i(a.distance(p));
            r.b(a.point_inside(p, 0, false)).b(a.point_inside(p, acc, false)).b(a.point_inside(p, acc, true));
        }

        if let Some(lcb) = a.as_shape_ref().as_line_chain_base() {
            r.b(lcb.point_on_edge(p, acc)).i(lcb.edge_containing_point(p, acc));

            if lcb.get_point_count() > 0 {
                r.v(lcb.get_point(0)).seg(&lcb.get_segment(-1));
            }
        }

        if let ShapeRef::Rect(rect) = a.as_shape_ref() {
            let mut rect = *rect;
            let off = rng.range(-1000, 1000) as i32;
            let inf = rect.get_inflated(off);
            r.s(&inf.format(true)).i(rect.diagonal()).i(rect.major_dimension()).i(rect.minor_dimension());
            rec_chain(&mut r, &rect.outline());
            rect.normalize();
            r.s(&rect.format(true));
            let quarter = rng.range(-4, 4) as i32;
            rect.rotate(deg(90.0 * quarter as f64), p);
            r.s(&rect.format(true));
        }

        if let ShapeRef::Segment(sg) = a.as_shape_ref() {
            r.i(sg.get_total_length()).v(sg.get_center());
        }

        sec.add(&r);
    }
}

fn section_segment_libm(results: &mut Results) {
    let mut sec = Section::new(results, "libm:shape_segment");
    let mut rng = Rng::new(211);

    for _ in 0..10000 {
        let size = rng.pt_r(0, 5000000);
        let c = rng.pt_mid();
        let ang = rng.angle_deg();
        let tol = rng.dbl(0.0, 5.0);

        let s = ShapeSegment::by_size_and_center(size, c, deg(ang));

        let mut r = Rec::default();
        r.v(size).v(c).d(ang).seg(s.get_seg()).i(s.get_width()).a(s.get_angle());
        r.b(s.is_45_degree()).b(s.is_45_degree_with_tolerance(deg(tol)));

        let mut rot = s;
        rot.rotate(deg(ang), c);
        r.seg(rot.get_seg());

        let mut circ = ShapeCircle::new(c, size.x);
        circ.rotate(deg(ang), size);
        r.s(&circ.format(true)).s(&circ.format(false));

        let mut simple = rand_simple(&mut rng);
        simple.rotate(deg(ang), c);
        r.bx(&simple.bbox(0)).v(simple.c_point(-1));
        sec.add(&r);
    }
}

fn section_geometry_utils(results: &mut Results) {
    let mut sec = Section::new(results, "geometry_utils");
    let mut rng = Rng::new(212);

    for _ in 0..30000 {
        let mut v = rng.pt();
        let vl = rng.pt_l(-5000000000, 5000000000);
        let mut vd = rng.pt_d(-3e9, 3e9);
        let only45 = rng.flag();
        let pad = rng.range(0, 1000) as u32;

        if rng.next() % 4 == 0 {
            v = rng.pt_r(-10, 10);
            vd = Vector2D::new(v.x as f64, v.y as f64);
        }

        let mut r = Rec::default();
        r.v(v).vl(vl).vd(vd).b(only45).i(pad);
        r.v(get_vector_snapped45(v, only45)).vd(get_vector_snapped45(vd, only45));
        r.vl(get_vector_snapped45(vl, only45));
        r.v(get_vector_snapped90(v)).vd(get_vector_snapped90(vd));
        r.v(get_clamped_coords(v, pad)).vl(get_clamped_coords(vl, pad)).vd(get_clamped_coords(vd, pad));
        r.v(get_clamped_coords_to_i(vd, pad));
        r.b(is_vec2_safe_xy(v)).b(is_vec2_safe_xy(vl)).b(is_vec2_safe_xy(vd));

        let bp = rng.pt_mid();
        let bs = rng.pt_r(0, 100000000);
        let bx = Box2I::new(bp, bs);
        let a = rng.pt();
        let b = rng.pt();
        let (mut x1, mut y1, mut x2, mut y2) = (a.x, a.y, b.x, b.y);
        let clipped = clip_line(&bx, &mut x1, &mut y1, &mut x2, &mut y2);
        r.bx(&bx).v(a).v(b).b(clipped).i(x1).i(y1).i(x2).i(y2);

        let acc = rng.range(0, 1000) as i32;
        let b2p = rng.pt_mid();
        let b2s = rng.pt_r(0, 100000000);
        let box2 = Box2I::new(b2p, b2s);
        let contained = rng.flag();
        r.b(box_hit_test_point(a, &bx, acc)).b(box_hit_test_box(&box2, &bx, contained, acc));
        sec.add(&r);
    }
}

fn section_geometry_utils_libm(results: &mut Results) {
    let mut sec = Section::new(results, "libm:geometry_utils");
    let mut rng = Rng::new(213);

    for _ in 0..30000 {
        let mut rad = rng.range(-10, 200000000) as i32;
        let err = rng.range(-10, 100000) as i32;
        let ang = rng.angle_deg();
        let segs = rng.range(-5, 100000) as i32;

        if rng.flag() {
            rad = rng.range(-10, 1000) as i32;
        }

        let mut r = Rec::default();
        r.i(rad).i(err).d(ang).i(segs);
        r.i(get_arc_to_segment_count(rad, err, deg(ang)));
        r.i(circle_to_end_segment_delta_radius(rad, segs));
        r.i(get_circle_to_poly_correction(err));
        sec.add(&r);
    }
}

fn section_lines(results: &mut Results) {
    let mut sec = Section::new(results, "lines");
    let mut rng = Rng::new(214);

    for _ in 0..30000 {
        let (a, b) = seg_pair(&mut rng);
        let mut p = rng.pt();

        if rng.next() % 4 == 0 {
            let d = rng.pt_r(-1, 1);
            p = (if rng.flag() { a.a } else { a.b }) + d;
        }

        let hl = HalfLine::from_seg(a);
        let hl2 = HalfLine::new(b.a, b.b);
        let ln = Line::from_seg(a);
        let ln2 = Line::new(b.a, b.b);

        let mut r = Rec::default();
        r.seg(&a).seg(&b).v(p);
        r.b(hl.contains(p)).opt(hl.intersect_seg(&b)).opt(hl.intersect(&hl2)).v(hl.nearest_point(p));
        r.opt(ln.intersect_seg(&b)).opt(ln.intersect(&ln2)).i(ln.distance(p)).v(ln.nearest_point(p));
        r.b(hl == hl2).b(ln == ln2);

        let bp = rng.pt_mid();
        let bs = rng.pt_r(0, 100000000);
        let bx = Box2I::new(bp, bs);

        let c1 = clip_half_line_to_box(&hl, &bx);
        let c2 = clip_line_to_box(&ln, &bx);
        r.bx(&bx).b(c1.is_some());

        if let Some(c1) = c1 {
            r.seg(&c1);
        }

        r.b(c2.is_some());

        if let Some(c2) = c2 {
            r.seg(&c2);
        }

        sec.add(&r);
    }
}

fn section_arc_chord_params(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_chord_params");
    let mut rng = Rng::new(215);

    for _ in 0..20000 {
        let (s, m, e) = arc_points(&mut rng);

        let mut p = ArcChordParams::new();
        let ok = p.compute(s, m, e);

        let mut r = Rec::default();
        r.v(s).v(m).v(e).b(ok).b(p.is_valid());
        r.d(p.chord_length()).d(p.half_chord()).vd(p.chord_unit_vec()).vd(p.normal_unit_vec());
        r.d(p.sagitta()).d(p.radius()).d(p.center_offset()).vd(p.chord_midpoint());

        if ok {
            r.vd(p.center_point()).a(p.start_angle()).a(p.end_angle()).d(p.arc_angle());
        }

        sec.add(&r);
    }
}

fn rec_sol<'r>(r: &'r mut Rec, s: &ArcSolution) -> &'r mut Rec {
    r.b(s.valid).v(s.start).v(s.mid).v(s.end).vd(s.center).d(s.radius)
}

fn section_arc_construction(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_construction");
    let mut rng = Rng::new(216);

    for _ in 0..20000 {
        let s = rng.pt();
        let mut e = rng.pt();
        let mut c = rng.pt();
        let mut t = rng.pt_d(-10.0, 10.0);
        let major = rng.flag();

        match rng.next() % 4 {
            0 => e = s + rng.pt_r(-10, 10),
            1 => c = (s + e) / 2 + rng.pt_r(-3, 3),
            2 => t = Vector2D::new((e.x - s.x) as f64, (e.y - s.y) as f64),
            _ => {}
        }

        let mut r = Rec::default();
        r.v(s).v(e).v(c).vd(t).b(major);
        r.v(project_to_chord_bisector(s, e, c));
        rec_sol(&mut r, &arc_through_points(s, c, e));
        rec_sol(&mut r, &arc_from_start_end_mid_drag(s, e, c));
        rec_sol(&mut r, &arc_from_start_end_center_drag(s, e, c, major));
        rec_sol(&mut r, &arc_from_start_tangent_end(s, t, e));
        sec.add(&r);
    }
}

/// Runs all sections, in the order of `shapes_oracle.cpp`.
pub fn run_all(results: &mut Results) {
    super::sections::section_libm_probe(results);
    section_circle(results);
    section_circle_tan_tan_pt(results);
    section_arc_props(results);
    section_arc_polyline(results);
    section_arc_collide(results);
    section_arc_intersect(results);
    section_arc_transform(results);
    section_shape_collide(results);
    section_shape_collide_arc(results);
    section_shape_distance(results);
    section_segment_libm(results);
    section_geometry_utils(results);
    section_geometry_utils_libm(results);
    section_lines(results);
    section_arc_chord_params(results);
    section_arc_construction(results);

    if let Some(dump) = results.dump.as_mut() {
        dump.flush().unwrap();
    }
}

use std::io::Write;
