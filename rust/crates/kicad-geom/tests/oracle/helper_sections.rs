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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Rust side of `helpers_oracle.cpp` (work package G6a): replays every section with the same
//! inputs through the port.  Keep the two files in sync statement by statement (the order of
//! the random draws matters).

use std::io::Write;

use kicad_geom::bezier_curves::{Bezier, BezierPoly, NumericType, transform_ellipse_to_beziers};
use kicad_geom::geometry::convex_hull::{
    build_convex_hull, build_convex_hull_poly_set, build_convex_hull_poly_set_moved,
};
use kicad_geom::geometry::distribute::{
    get_deltas_for_distribute_by_gaps, get_deltas_for_distribute_by_points,
};
use kicad_geom::geometry::eda_angle::{ANGLE_0, DEGREES_T, EdaAngle, FULL_CIRCLE, RADIANS_T};
use kicad_geom::geometry::ellipse::Ellipse;
use kicad_geom::geometry::geometry_predicates::{
    in_circle_delaunay_legal, in_circle_legal_double, is_sliver_triangle, orientation_sign,
};
use kicad_geom::geometry::grid_geometry::{GridGeometry, GridKind};
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::hash::StdHash;
use kicad_geom::hash_128::Hash128;
use kicad_geom::kicore::mirror::FlipDirection;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::{Vector2, Vector2D, Vector2I};
use kicad_geom::math::vector3::{Vector3D, Vector3I};
use kicad_geom::md5_hash::Md5Hash;
use kicad_geom::mmh3_hash::Mmh3Hash;
use kicad_geom::transform::Transform;
use kicad_geom::{hash_combine, hash_val};

use super::sections::{Rec, Results, Rng, Section};

const MAX_ERRORS: [i32; 12] = [1, 2, 5, 10, 100, 1000, 2500, 5000, 10000, 20000, 0, -5];
const P10: [i64; 7] = [1, 10, 100, 1000, 10000, 100000, 1000000];

fn max_error_i(rng: &mut Rng) -> i32 {
    MAX_ERRORS[(rng.next() % 12) as usize]
}

fn add(a: Vector2I, b: Vector2I) -> Vector2I {
    Vector2I::new(a.x + b.x, a.y + b.y)
}

fn mul(a: Vector2I, k: i32) -> Vector2I {
    Vector2I::new(a.x * k, a.y * k)
}

/// `bezierCtrl`
fn bezier_ctrl(rng: &mut Rng, a_count: usize, a_err: i32, a_pts: &mut Vec<Vector2I>) {
    let mut span: i64 = std::cmp::max(1, a_err.max(1) as i64 * P10[rng.range(0, 6) as usize]);
    span = span.min(300000000);
    let c = rng.pt_r(-200000000, 200000000);
    let kind = rng.next() % 10;

    a_pts.clear();

    for _ in 0..a_count {
        let d = rng.pt_r(-span, span);
        a_pts.push(add(c, d));
    }

    match kind {
        1 => {
            let d = rng.pt_r(-span / 4 - 1, span / 4 + 1);

            for i in 0..a_count {
                let k = rng.range(-3, 3) as i32;
                a_pts[i] = add(c, mul(d, k));
            }
        }
        2 => {
            for p in a_pts.iter_mut() {
                *p = c;
            }
        }
        3 => a_pts[a_count - 1] = a_pts[0],
        4 => {
            a_pts[1] = a_pts[0];

            if a_count == 4 && rng.flag() {
                a_pts[2] = a_pts[3];
            }
        }
        5 => {
            let s = span as i32;
            let h = rng.range(1, s as i64) as i32;
            a_pts[0] = add(c, Vector2I::new(-s, 0));
            let x1 = rng.range(-s as i64, s as i64) as i32;
            a_pts[1] = add(c, Vector2I::new(x1, h));
            let x2 = rng.range(-s as i64, s as i64) as i32;
            a_pts[a_count - 2] = add(c, Vector2I::new(x2, -h));
            a_pts[a_count - 1] = add(c, Vector2I::new(s, 0));
        }
        6 => {
            let d = rng.pt_r(-span / 4 - 1, span / 4 + 1);

            for i in 0..a_count {
                let k = rng.range(-3, 3) as i32;
                let e = rng.pt_r(-3, 3);
                a_pts[i] = add(add(c, mul(d, k)), e);
            }
        }
        7 => {
            if a_count == 4 {
                a_pts[2] = a_pts[1];
            } else {
                a_pts[1] = a_pts[2];
            }
        }
        _ => {}
    }
}

fn rec_poly_i(r: &mut Rec, a_pts: &[Vector2I]) {
    r.u(a_pts.len() as u64);

    for &p in a_pts {
        r.v(p);
    }
}

fn rec_poly_d(r: &mut Rec, a_pts: &[Vector2D]) {
    r.u(a_pts.len() as u64);

    for &p in a_pts {
        r.vd(p);
    }
}

fn section_bezier_poly_i(results: &mut Results) {
    let mut sec = Section::new(results, "libm:bezier_poly_i");
    let mut rng = Rng::new(6001);

    for _ in 0..30000 {
        let err = max_error_i(&mut rng);
        let count = if rng.flag() { 4 } else { 3 };
        let mut ctrl = Vec::new();
        bezier_ctrl(&mut rng, count, err, &mut ctrl);
        let four_arg = count == 4 && rng.flag();

        let mut out = vec![Vector2I::new(7, 7)];

        if four_arg {
            BezierPoly::new(ctrl[0], ctrl[1], ctrl[2], ctrl[3]).get_poly_i(&mut out, err);
        } else {
            BezierPoly::from_points_i(&ctrl).get_poly_i(&mut out, err);
        }

        let mut r = Rec::default();
        r.i(err).b(four_arg);
        rec_poly_i(&mut r, &ctrl);
        rec_poly_i(&mut r, &out);
        sec.add(&r);
    }
}

fn section_bezier_poly_d(results: &mut Results) {
    let mut sec = Section::new(results, "libm:bezier_poly_d");
    let mut rng = Rng::new(6002);
    let errs = [0.01, 0.5, 1.0, 10.0, 2500.0, 5000.0, 0.0, -1.0];

    for _ in 0..20000 {
        let err: f64 = errs[(rng.next() % 8) as usize];
        let count = if rng.flag() { 4 } else { 3 };
        let mut ctrl_i = Vec::new();
        bezier_ctrl(&mut rng, count, err.max(1.0) as i32, &mut ctrl_i);
        let mut ctrl: Vec<Vector2D> = Vec::new();

        for &p in &ctrl_i {
            let fx = if rng.flag() { rng.dbl(-1.0, 1.0) } else { 0.0 };
            let fy = if rng.flag() { rng.dbl(-1.0, 1.0) } else { 0.0 };

            if err < 1.0 {
                ctrl.push(Vector2D::new(
                    (p.x % 1000) as f64 + fx,
                    (p.y % 1000) as f64 + fy,
                ));
            } else {
                ctrl.push(Vector2D::new(p.x as f64 + fx, p.y as f64 + fy));
            }
        }

        let mut out: Vec<Vector2D> = Vec::new();

        if rng.flag() {
            out.push(Vector2D::new(1.5, -2.5));
        }

        BezierPoly::from_points_d(ctrl.clone()).get_poly_d(&mut out, err);

        let mut r = Rec::default();
        r.d(err);
        rec_poly_d(&mut r, &ctrl);
        rec_poly_d(&mut r, &out);
        sec.add(&r);
    }
}

fn t_value(rng: &mut Rng) -> f64 {
    match rng.next() % 6 {
        0 => 0.0,
        1 => 1.0,
        2 => 0.5,
        3 => rng.dbl(-0.5, 1.5),
        _ => rng.unit(),
    }
}

/// Records of the `int` / `double` instantiations.
trait RecT: NumericType {
    fn rec_vec(r: &mut Rec, v: Vector2<Self>);
    fn rec_num(r: &mut Rec, v: Self);
}

impl RecT for i32 {
    fn rec_vec(r: &mut Rec, v: Vector2I) {
        r.v(v);
    }

    fn rec_num(r: &mut Rec, v: i32) {
        r.i(v);
    }
}

impl RecT for f64 {
    fn rec_vec(r: &mut Rec, v: Vector2D) {
        r.vd(v);
    }

    fn rec_num(r: &mut Rec, v: f64) {
        r.d(v);
    }
}

fn rec_bez<T: RecT>(r: &mut Rec, b: &Bezier<T>) {
    T::rec_vec(r, b.start);
    T::rec_vec(r, b.c1);
    T::rec_vec(r, b.c2);
    T::rec_vec(r, b.end);
}

fn bez_template_case<T: RecT>(rng: &mut Rng, r: &mut Rec, b: &Bezier<T>) {
    let t = t_value(rng);
    let mut t0 = t_value(rng);
    let mut t1 = t_value(rng);

    if rng.flag() && t1 < t0 {
        std::mem::swap(&mut t0, &mut t1);
    }

    let (left, right) = b.split(t);

    r.d(t).d(t0).d(t1);
    rec_bez(r, b);
    let p = b.point_at(t);
    rec_bez(r, &Bezier::new(p, p, p, p));
    rec_bez(r, &left);
    rec_bez(r, &right);
    rec_bez(r, &b.sub_curve(t0, t1));
}

fn section_bezier_template(results: &mut Results) {
    let mut sec = Section::new(results, "bezier_template");
    let mut rng = Rng::new(6003);

    for _ in 0..20000 {
        let mut ctrl = Vec::new();
        let err = rng.range(1, 1000) as i32;
        bezier_ctrl(&mut rng, 4, err, &mut ctrl);
        let mut r = Rec::default();

        if rng.flag() {
            let b: Bezier<i32> = Bezier::new(ctrl[0], ctrl[1], ctrl[2], ctrl[3]);
            r.b(true);
            bez_template_case(&mut rng, &mut r, &b);
        } else {
            let fx = rng.dbl(-1.0, 1.0);
            let b: Bezier<f64> = Bezier::new(
                Vector2D::from(ctrl[0]) + Vector2D::new(fx, 0.25),
                Vector2D::from(ctrl[1]),
                Vector2D::from(ctrl[2]) - Vector2D::new(0.5, fx),
                Vector2D::from(ctrl[3]),
            );
            r.b(false);
            bez_template_case(&mut rng, &mut r, &b);
        }

        sec.add(&r);
    }
}

fn ellipse_angle(rng: &mut Rng) -> EdaAngle {
    match rng.next() % 5 {
        0 => ANGLE_0,
        1 => FULL_CIRCLE,
        2 => EdaAngle::new(rng.dbl(-7.0, 7.0), RADIANS_T),
        _ => EdaAngle::new(rng.angle_deg(), DEGREES_T),
    }
}

fn rec_ellipse<T: RecT>(r: &mut Rec, e: &Ellipse<T>) {
    T::rec_vec(r, e.center);
    T::rec_num(r, e.major_radius);
    T::rec_num(r, e.minor_radius);
    r.a(e.rotation).a(e.start_angle).a(e.end_angle);
}

/// Draws a random point of the instantiation's type, like the `if constexpr` branches.
trait RandPoint: RecT {
    fn rand_point(rng: &mut Rng) -> Vector2<Self>;
}

impl RandPoint for i32 {
    fn rand_point(rng: &mut Rng) -> Vector2I {
        rng.pt_mid()
    }
}

impl RandPoint for f64 {
    fn rand_point(rng: &mut Rng) -> Vector2D {
        rng.pt_d(-1e8, 1e8)
    }
}

fn ellipse_case<T: RandPoint>(rng: &mut Rng, r: &mut Rec, mut e: Ellipse<T>) {
    rec_ellipse(r, &e);
    r.a(e.subtended_angle());
    T::rec_vec(r, e.arc_start_point());
    T::rec_vec(r, e.arc_end_point());

    let a = ellipse_angle(rng);
    T::rec_vec(r, e.point_at_angle(a));

    let probe = if rng.flag() {
        e.point_at_angle(a)
    } else {
        T::rand_point(rng)
    };

    r.a(a);
    T::rec_vec(r, probe);
    r.a(e.angle_at_point(probe));

    let mut beziers: Vec<Bezier<T>> = Vec::new();
    transform_ellipse_to_beziers(&e, &mut beziers);
    r.u(beziers.len() as u64);

    for b in &beziers {
        rec_bez(r, b);
    }

    let reference = T::rand_point(rng);
    let lr = rng.flag();
    e.mirror(
        reference,
        if lr {
            FlipDirection::LeftRight
        } else {
            FlipDirection::TopBottom
        },
    );
    T::rec_vec(r, reference);
    r.b(lr);
    rec_ellipse(r, &e);
}

fn section_ellipse(results: &mut Results) {
    let mut sec = Section::new(results, "libm:ellipse");
    let mut rng = Rng::new(6004);

    for _ in 0..30000 {
        let mut r = Rec::default();
        let kind = rng.next() % 4;
        r.i(kind as i64);

        let s = ellipse_angle(&mut rng);
        let mut e = ellipse_angle(&mut rng);

        if rng.next().is_multiple_of(8) {
            e = s;
        }

        match kind {
            0 => {
                let c = rng.pt_mid();
                let major = rng.range(0, 100000000) as i32;
                let minor = rng.range(0, 100000000) as i32;
                let rot = ellipse_angle(&mut rng);
                ellipse_case(
                    &mut rng,
                    &mut r,
                    Ellipse::<i32>::new(c, major, minor, rot, s, e),
                );
            }
            1 => {
                let c = rng.pt_mid();
                let m = rng.pt_mid();
                let ratio = if rng.flag() {
                    rng.dbl(0.0, 1.0)
                } else {
                    rng.dbl(0.0, 10.0)
                };
                ellipse_case(
                    &mut rng,
                    &mut r,
                    Ellipse::<i32>::from_major_axis(c, m, ratio, s, e),
                );
            }
            2 => {
                let c = rng.pt_d(-1e8, 1e8);
                let major = rng.dbl(0.0, 1e8);
                let minor = rng.dbl(0.0, 1e8);
                let rot = ellipse_angle(&mut rng);
                ellipse_case(
                    &mut rng,
                    &mut r,
                    Ellipse::<f64>::new(c, major, minor, rot, s, e),
                );
            }
            _ => {
                let c = rng.pt_d(-1e8, 1e8);
                let m = rng.pt_d(-1e8, 1e8);
                let ratio = rng.dbl(0.0, 10.0);
                ellipse_case(
                    &mut rng,
                    &mut r,
                    Ellipse::<f64>::from_major_axis(c, m, ratio, s, e),
                );
            }
        }

        sec.add(&r);
    }
}

fn hull_points(rng: &mut Rng, a_pts: &mut Vec<Vector2I>) {
    let count = rng.range(0, 60);
    let kind = rng.next() % 6;
    a_pts.clear();

    for _ in 0..count {
        match kind {
            0 => a_pts.push(rng.pt_r(-5, 5)),
            1 => {
                let k = rng.range(-1000, 1000) as i32;
                a_pts.push(Vector2I::new(3 * k + 7, -2 * k + 1));
            }
            2 => a_pts.push(rng.pt()),
            3 => {
                let a = rng.dbl(0.0, std::f64::consts::TAU);
                let rad = rng.range(1, 100000000) as i32;
                a_pts.push(Vector2I::new(
                    (rad as f64 * kicad_geom::cmath::cos(a)) as i32,
                    (rad as f64 * kicad_geom::cmath::sin(a)) as i32,
                ));
            }
            4 => a_pts.push(rng.pt_r(-500000000, 500000000)),
            _ => a_pts.push(rng.pt_mid()),
        }

        if !a_pts.is_empty() && rng.next().is_multiple_of(5) {
            let idx = (rng.next() % a_pts.len() as u64) as usize;
            a_pts.push(a_pts[idx]);
        }
    }
}

fn section_convex_hull(results: &mut Results) {
    let mut sec = Section::new(results, "libm:convex_hull");
    let mut rng = Rng::new(6005);

    for n in 0..30000 {
        let mut pts = Vec::new();
        hull_points(&mut rng, &mut pts);

        let mut hull = vec![Vector2I::new(1, 2), Vector2I::new(3, 4)];
        build_convex_hull(&mut hull, &pts);

        let mut r = Rec::default();
        rec_poly_i(&mut r, &pts);
        rec_poly_i(&mut r, &hull);

        if n % 4 == 0 {
            let mut set = ShapePolySet::new();
            let outlines = rng.range(0, 3);

            for _ in 0..outlines {
                let mut opts = Vec::new();
                hull_points(&mut rng, &mut opts);
                set.add_outline(&ShapeLineChain::from_points(&opts, true));
            }

            let pos = rng.pt_mid();
            let rot = EdaAngle::new(rng.angle_deg(), DEGREES_T);
            let mut h1 = Vec::new();
            let mut h2 = Vec::new();
            build_convex_hull_poly_set(&mut h1, &set);
            build_convex_hull_poly_set_moved(&mut h2, &set, pos, rot);
            r.v(pos).a(rot);
            rec_poly_i(&mut r, &h1);
            rec_poly_i(&mut r, &h2);
        }

        sec.add(&r);
    }
}

fn scale_value(rng: &mut Rng) -> f64 {
    match rng.next() % 10 {
        0 => 1.0,
        1 => -1.0,
        2 => 2.0,
        3 => 0.5,
        4 => f64::from_bits(1.0f64.to_bits() + 1),
        5 => 1.0 + 1e-6,
        6 => -rng.dbl(0.01, 10.0),
        7 => {
            if rng.next().is_multiple_of(50) {
                0.0
            } else {
                rng.dbl(0.001, 0.1)
            }
        }
        _ => rng.dbl(0.01, 10.0),
    }
}

fn rec_trs(r: &mut Rec, t: &TransformTrs) {
    r.v(t.translate())
        .a(t.rotate())
        .d(t.scale_x())
        .d(t.scale_y());
}

fn section_transform_trs(results: &mut Results) {
    let mut sec = Section::new(results, "libm:transform_trs");
    let mut rng = Rng::new(6006);

    for _ in 0..40000 {
        let tr = if rng.flag() {
            rng.pt_mid()
        } else {
            Vector2I::new(0, 0)
        };
        let rot = if rng.flag() {
            EdaAngle::new(rng.angle_deg(), DEGREES_T)
        } else {
            ANGLE_0
        };
        let sx = scale_value(&mut rng);
        let sy = if rng.flag() {
            sx
        } else {
            scale_value(&mut rng)
        };

        let t = TransformTrs::new(tr, rot, sx, sy);

        let p = rng.pt_mid();
        let pd = rng.pt_d(-1e8, 1e8);
        let fixed = rng.pt_mid();
        let rsx = scale_value(&mut rng);
        let rsy = scale_value(&mut rng);
        let len = rng.dbl(-1e7, 1e7);

        let mut r = Rec::default();
        rec_trs(&mut r, &t);
        r.v(p).vd(pd).v(fixed).d(rsx).d(rsy).d(len);
        r.v(t.apply(p))
            .v(t.inverse_apply(p))
            .v(t.inverse_apply(t.apply(p)));
        r.vd(t.apply_d(pd)).vd(t.inverse_apply_d(pd));
        r.b(t.is_identity())
            .b(t.is_uniform_scale())
            .b(t.has_unit_scale());
        r.d(t.apply_linear_scale(len))
            .d(t.inverse_apply_linear_scale(len));

        let rs = t.rescale_around(fixed, rsx, rsy);
        rec_trs(&mut r, &rs);
        r.b(rs == t)
            .b(rs != t)
            .b(t == TransformTrs::new(tr, rot, sx, sy));

        let mut s = TransformTrs::default();
        s.set_translate(fixed);
        s.set_rotate(rot);
        s.set_scale(rsx, rsy);
        rec_trs(&mut r, &s);
        r.v(s.apply(t.apply(p)));
        sec.add(&r);
    }
}

fn section_transform(results: &mut Results) {
    let mut sec = Section::new(results, "libm:transform");
    let mut rng = Rng::new(6007);

    for _ in 0..30000 {
        let mut t;

        loop {
            let lim = if rng.flag() { 1 } else { 3 };
            let x1 = rng.range(-lim, lim) as i32;
            let y1 = rng.range(-lim, lim) as i32;
            let x2 = rng.range(-lim, lim) as i32;
            let y2 = rng.range(-lim, lim) as i32;
            t = Transform::new(x1, y1, x2, y2);

            if t.x1 * t.y2 - t.x2 * t.y1 != 0 {
                break;
            }
        }

        let p = rng.pt_mid();
        let bp = rng.pt_mid();
        let bs = rng.pt_l(-100000000, 100000000);
        let bx = Box2I::new(bp, bs);
        let mut a1 = EdaAngle::new(rng.angle_deg(), DEGREES_T);
        let mut a2 = EdaAngle::new(rng.angle_deg(), DEGREES_T);

        if rng.next().is_multiple_of(6) {
            a2 = a1;
        }

        let mut r = Rec::default();
        r.i(t.x1).i(t.y1).i(t.x2).i(t.y2).v(p).bx(&bx).a(a1).a(a2);
        r.v(t.transform_coordinate(p))
            .bx(&t.transform_coordinate_box(&bx));
        let inv = t.inverse_transform();
        r.i(inv.x1).i(inv.y1).i(inv.x2).i(inv.y2);
        r.b(t == inv).b(t != inv).b(t == Transform::default());
        let swapped = t.map_angles(&mut a1, &mut a2);
        r.b(swapped).a(a1).a(a2);
        r.u(t.std_hash());
        sec.add(&r);
    }
}

fn section_distribute(results: &mut Results) {
    let mut sec = Section::new(results, "distribute");
    let mut rng = Rng::new(6008);

    for _ in 0..20000 {
        let count = rng.range(0, 12);
        let lim: i64 = if rng.flag() { 1000 } else { 100000000 };
        let mut extents: Vec<(i32, i32)> = Vec::new();
        let mut positions: Vec<i32> = Vec::new();

        for _ in 0..count {
            let a = rng.range(-lim, lim) as i32;
            let w = rng.range(0, lim / 10) as i32;
            extents.push((a, a + w));
            positions.push(rng.range(-lim, lim) as i32);
        }

        if rng.flag() {
            extents.sort();
        }

        if rng.flag() {
            positions.sort();
        }

        let mut r = Rec::default();
        r.u(extents.len() as u64);

        for &(s, e) in &extents {
            r.i(s).i(e);
        }

        for &p in &positions {
            r.i(p);
        }

        for d in get_deltas_for_distribute_by_gaps(&extents) {
            r.i(d);
        }

        for d in get_deltas_for_distribute_by_points(&positions) {
            r.i(d);
        }

        sec.add(&r);
    }
}

fn section_grid_geometry(results: &mut Results) {
    let mut sec = Section::new(results, "libm:grid_geometry");
    let mut rng = Rng::new(6009);

    for _ in 0..30000 {
        let mut g = GridGeometry {
            kind: if rng.flag() {
                GridKind::Cartesian
            } else {
                GridKind::Polar
            },
            ..Default::default()
        };
        g.origin = if rng.flag() {
            Vector2D::new(0.0, 0.0)
        } else {
            rng.pt_d(-1e8, 1e8)
        };

        let polar = g.kind == GridKind::Polar;

        match rng.next() % 6 {
            0 => g.pitch = Vector2D::new(0.0, rng.dbl(-1.0, 1.0)),
            1 => g.pitch = Vector2D::new(rng.dbl(-1e6, 0.0), 1e5),
            _ => {
                let px = rng.dbl(1e3, 1e6);
                let py = if polar {
                    rng.dbl(0.01, 1.0)
                } else {
                    rng.dbl(1e3, 1e6)
                };
                g.pitch = Vector2D::new(px, py);
            }
        }

        g.orientation = if rng.flag() { 0.0 } else { rng.dbl(-7.0, 7.0) };

        let ex = if rng.next().is_multiple_of(10) {
            -rng.dbl(0.0, 1e7)
        } else {
            rng.dbl(0.0, 5e7)
        };
        let ey = if polar {
            rng.dbl(-7.0, 7.0)
        } else {
            rng.dbl(0.0, 5e7)
        };
        g.extent = Vector2D::new(ex, ey);
        g.priority = rng.range(0, 3) as u32;

        let mut o = g;
        o.priority = rng.range(0, 3) as u32;
        let oex = rng.dbl(0.0, 5e7);
        let oey = if polar {
            rng.dbl(-7.0, 7.0)
        } else {
            rng.dbl(0.0, 5e7)
        };
        o.extent = Vector2D::new(oex, oey);

        let p = if rng.flag() {
            rng.pt_d(-1e8, 1e8)
        } else {
            g.origin + rng.pt_d(-1e5, 1e5)
        };
        let tol = if rng.flag() { 0.0 } else { rng.dbl(0.0, 1e5) };

        let mut r = Rec::default();
        r.b(polar)
            .vd(g.origin)
            .vd(g.pitch)
            .d(g.orientation)
            .vd(g.extent)
            .u(g.priority as u64);
        r.u(o.priority as u64).vd(o.extent).vd(p).d(tol);
        r.vd(g.snap(p)).b(g.contains(p, tol)).d(g.area());
        r.b(g.takes_precedence_over(&o))
            .b(o.takes_precedence_over(&g));
        sec.add(&r);
    }
}

fn section_predicates(results: &mut Results) {
    let mut sec = Section::new(results, "geometry_predicates");
    let mut rng = Rng::new(6010);

    for _ in 0..40000 {
        let (a, b, c, p);

        match rng.next() % 5 {
            0 => {
                a = rng.pt();
                b = rng.pt();
                c = rng.pt();
                p = rng.pt();
            }
            1 => {
                let lo = i32::MIN as i64;
                let hi = i32::MAX as i64;
                a = rng.pt_r(lo, hi);
                b = rng.pt_r(lo, hi);
                c = rng.pt_r(lo, hi);
                p = rng.pt_r(lo, hi);
            }
            2 => {
                let ctr = rng.pt_r(-1000000, 1000000);
                let rad = rng.range(1, 100000000) as i32;
                a = add(ctr, Vector2I::new(rad, 0));
                b = add(ctr, Vector2I::new(0, rad));
                c = add(ctr, Vector2I::new(-rad, 0));
                p = add(add(ctr, Vector2I::new(0, -rad)), rng.pt_r(-2, 2));
            }
            3 => {
                let o = rng.pt_mid();
                let d = rng.pt_r(-1000, 1000);
                a = o;
                let kb = rng.range(-100, 100) as i32;
                b = add(o, mul(d, kb));
                let kc = rng.range(-100, 100) as i32;
                let e = rng.pt_r(-1, 1);
                c = add(add(o, mul(d, kc)), e);
                p = if rng.flag() { a } else { rng.pt_mid() };
            }
            _ => {
                a = rng.pt_r(-10, 10);
                b = rng.pt_r(-10, 10);
                c = rng.pt_r(-10, 10);
                p = rng.pt_r(-10, 10);
            }
        }

        let mut r = Rec::default();
        r.v(a).v(b).v(c).v(p);
        r.i(orientation_sign(a, b, c)).i(orientation_sign(a, c, b));
        r.b(in_circle_delaunay_legal(a, b, c, p));
        r.b(in_circle_legal_double(a, b, c, p));
        r.b(is_sliver_triangle(a, b, c));
        sec.add(&r);
    }
}

fn section_vector3(results: &mut Results) {
    let mut sec = Section::new(results, "vector3");
    let mut rng = Rng::new(6011);

    for _ in 0..20000 {
        let lim: i64 = if rng.flag() { 1000 } else { 30000 };
        let mut a = Vector3I::default();
        let mut b = Vector3I::default();
        a.x = rng.range(-lim, lim) as i32;
        a.y = rng.range(-lim, lim) as i32;
        a.z = rng.range(-lim, lim) as i32;
        b.x = rng.range(-lim, lim) as i32;
        b.y = rng.range(-lim, lim) as i32;
        b.z = rng.range(-lim, lim) as i32;
        let mut s = rng.range(-100, 100) as i32;

        if s == 0 {
            s = 1;
        }

        let mut ad = Vector3D::default();
        let mut bd = Vector3D::default();
        ad.x = rng.dbl(-1e6, 1e6);
        ad.y = rng.dbl(-1e6, 1e6);
        ad.z = rng.dbl(-1e6, 1e6);
        bd.x = rng.dbl(-1e6, 1e6);
        bd.y = rng.dbl(-1e6, 1e6);
        bd.z = rng.dbl(-1e6, 1e6);
        let sd = rng.dbl(-10.0, 10.0);

        let mut r = Rec::default();
        r.i(a.x).i(a.y).i(a.z).i(b.x).i(b.y).i(b.z).i(s);
        let cr = a.cross(&b);
        r.i(cr.x).i(cr.y).i(cr.z).i(a.dot(&b)).i(a.euclidean_norm());
        let mut m = a;
        m *= s;
        let mut dv = a;
        dv /= s;
        let mut nn = a;
        let nr = nn.normalize();
        r.i(m.x)
            .i(m.y)
            .i(m.z)
            .i(dv.x)
            .i(dv.y)
            .i(dv.z)
            .i(nr.x)
            .i(nr.y)
            .i(nr.z);
        r.b(a == b).b(a != b);

        r.d(ad.x).d(ad.y).d(ad.z).d(bd.x).d(bd.y).d(bd.z).d(sd);
        let crd = ad.cross(&bd);
        r.d(crd.x)
            .d(crd.y)
            .d(crd.z)
            .d(ad.dot(&bd))
            .d(ad.euclidean_norm());
        let mut md = ad;
        md *= sd;
        let mut dd = ad;
        dd /= sd;
        let mut nd = ad;
        nd.normalize();
        r.d(md.x)
            .d(md.y)
            .d(md.z)
            .d(dd.x)
            .d(dd.y)
            .d(dd.z)
            .d(nd.x)
            .d(nd.y)
            .d(nd.z);

        r.s(&format!("{a}{ad}"));
        sec.add(&r);
    }
}

fn random_bytes(rng: &mut Rng) -> Vec<u8> {
    let len = match rng.next() % 4 {
        0 => rng.range(0, 20),
        1 => rng.range(0, 80),
        2 => rng.range(55, 70),
        _ => rng.range(0, 1000),
    } as usize;

    (0..len).map(|_| rng.next() as u8).collect()
}

fn rec_bytes(r: &mut Rec, a_bytes: &[u8]) {
    let hex: String = a_bytes.iter().map(|b| format!("{b:02x}")).collect();
    r.s(&hex);
}

fn section_hashes(results: &mut Results) {
    let mut sec = Section::new(results, "hashes");
    let mut rng = Rng::new(6012);

    for _ in 0..30000 {
        let bytes = random_bytes(&mut rng);
        let seed: u32 = if rng.flag() { 0 } else { rng.next() as u32 };
        let mut r = Rec::default();
        rec_bytes(&mut r, &bytes);
        r.u(seed as u64);

        // MMH3_HASH: addData in random pieces, interleaved with add( int32_t ) when aligned
        {
            let mut h = Mmh3Hash::new(seed);
            let mut pos = 0usize;

            while pos < bytes.len() {
                let piece = (bytes.len() - pos).min(rng.range(0, 40) as usize);

                if rng.next().is_multiple_of(4) && piece >= 4 && pos.is_multiple_of(4) {
                    let v = i32::from_le_bytes([
                        bytes[pos],
                        bytes[pos + 1],
                        bytes[pos + 2],
                        bytes[pos + 3],
                    ]);
                    h.add_i32(v);
                    pos += 4;
                    r.i(-4);
                } else {
                    h.add_data(&bytes[pos..pos + piece]);
                    pos += piece;
                    r.u(piece as u64);
                }
            }

            r.s(&h.digest().to_string_hex());
        }

        // addDataV1, std::string and std::vector<char> overloads, reset
        {
            let mut h = Mmh3Hash::new(seed);
            h.add_data_v1(&bytes);
            r.s(&h.digest().to_string_hex());

            let mut h2 = Mmh3Hash::default();
            h2.add_bytes(&bytes);
            h2.add_bytes(&bytes);
            h2.add_i32(seed as i32);
            r.s(&h2.digest().to_string_hex());

            h2.reset(seed);
            let half = bytes.len() / 2;
            h2.add_data_v1(&bytes[..half]);
            h2.add_data_v1(&bytes[half..]);
            r.s(&h2.digest().to_string_hex());
        }

        // MD5_HASH
        {
            let mut m = Md5Hash::new();
            let third = bytes.len() / 3;
            m.hash_bytes(&bytes[..third]);
            m.hash_int(seed as i32);
            m.hash_bytes(&bytes[third..]);
            r.b(m.is_valid());
            m.finalize();
            r.b(m.is_valid()).s(&m.format(false)).s(&m.format(true));

            let mut m2 = m;
            r.b(m2 == m).b(m2 != m);
            m2.finalize(); // finalizing twice keeps hashing the padded context
            r.s(&m2.format(true)).b(m2 == m);

            let mut m3 = Md5Hash::new();
            m3.hash_bytes(&bytes);
            m3.finalize();
            r.s(&m3.format(true));
            m3.init();
            r.b(m3.is_valid()).s(&m3.format(true));
        }

        // hash_combine / hash_val / std::hash
        {
            let i1: i32 = rng.next() as i32;
            let i2: i64 = rng.next() as i64;
            let d1: f64 = if rng.next().is_multiple_of(8) {
                0.0
            } else {
                rng.dbl(-1e9, 1e9)
            };
            let d2: f64 = if rng.next().is_multiple_of(8) {
                -0.0
            } else {
                rng.dbl_scaled(-60, 60)
            };
            let b1: bool = rng.flag();
            let u1: u32 = rng.next() as u32;
            let s1: Vec<u8> = bytes[..bytes.len().min(50)].to_vec();

            r.i(i1).i(i2).d(d1).d(d2).b(b1).u(u1 as u64);
            r.u(d1.std_hash()).u(d2.std_hash());
            r.u(s1.std_hash()).u((d1 as f32).std_hash());
            r.u(hash_val!(i1)).u(hash_val!(i1, i2, d1, d2, b1, u1, s1));

            let mut seedc: u64 = rng.next();
            r.u(seedc);
            hash_combine!(seedc, s1, i1, d2);
            r.u(seedc);
        }

        // HASH_128
        {
            let mut h = Hash128::default();
            h.value64[0] = rng.next();
            h.value64[1] = rng.next();
            let z = Hash128::default();
            r.s(&h.to_string_hex())
                .b(h == z)
                .u(h.value32()[1] as u64)
                .u(h.value8()[9] as u64);
            h.clear();
            r.b(h == z);
        }

        sec.add(&r);
    }
}

/// All sections, in the order of `helpers_oracle.cpp`.
pub fn run_all(results: &mut Results) {
    super::sections::section_libm_probe(results);
    section_bezier_poly_i(results);
    section_bezier_poly_d(results);
    section_bezier_template(results);
    section_ellipse(results);
    section_convex_hull(results);
    section_transform_trs(results);
    section_transform(results);
    section_distribute(results);
    section_grid_geometry(results);
    section_predicates(results);
    section_vector3(results);
    section_hashes(results);

    if let Some(dump) = results.dump.as_mut() {
        dump.flush().unwrap();
    }
}
