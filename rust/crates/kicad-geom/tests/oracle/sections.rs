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

//! The case generators of the differential test against the original C++ kimath code: the
//! Rust mirror of `geom_oracle.cpp` in this directory.  See `tests/oracle_diff.rs`.
//!
//! This file is a module shared by the integration test `tests/oracle_diff.rs` and by the
//! WebAssembly check `run_wasm_check.sh`, which builds it for `wasm32-unknown-unknown` and
//! runs it under node.js.  It must stay in lock-step with the C++ file; the C++ function names
//! are given in the comments.

use std::io::Write;

use kicad_geom::geometry::eda_angle::{
    DEGREES_T, EdaAngle, EdaOrientation, RADIANS_T, TENTHS_OF_A_DEGREE_T,
};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::vector_utils::{
    get_length_ratio_from_start, get_nearest_endpoint, get_projected_point_length_ratio,
    point_is_in_direction, point_projects_onto_segment, points_are_in_same_direction, round_grid,
    round_nw, round_se, seg_is_in_direction,
};
use kicad_geom::math::box2::{Box2D, Box2I, Box2L, box2i_safe, box2i_safe_from_pos_size};
use kicad_geom::math::util::{
    isqrt, ki_round, ki_round_to, rescale, rounded_hypot, sign, unsigned_abs,
};
use kicad_geom::math::vector2::{
    Vector2, Vector2D, Vector2I, Vector2L, equals_vec, equals_vec_eps, ki_round_xy,
    lexicographical_compare, lexicographical_max, lexicographical_min,
};
use kicad_geom::math::wide_int::{cross_wide, dot_wide, to_double};
use kicad_geom::trigo::{
    calc_arc_center, calc_arc_center_d, calc_arc_center_from_angle, calc_arc_mid, deg2rad,
    get_rotated, get_rotated_around, intercepts_negative_x, intercepts_positive_x,
    is_point_on_segment, is_straight_arc, normalize_angle_pos, normalize_angle180, rad2decideg,
    rad2deg, rotate_point, rotate_point_around, segment_intersects_segment,
    segment_intersects_segment_with_point, test_segment_hit,
};

/// `RNG` of the oracle: splitmix64 and the derived generators.
pub(crate) struct Rng {
    s: u64,
}

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng { s: seed }
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.s = self.s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// inclusive range
    pub(crate) fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }

    /// [0, 1)
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }

    pub(crate) fn dbl(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    pub(crate) fn flag(&mut self) -> bool {
        (self.next() & 1) != 0
    }

    pub(crate) fn coord(&mut self) -> i32 {
        match self.next() % 4 {
            0 => self.range(-10, 10) as i32,
            1 => self.range(-1000, 1000) as i32,
            2 => self.range(-1000000, 1000000) as i32,
            _ => self.range(-500000000, 500000000) as i32,
        }
    }

    pub(crate) fn coord_mid(&mut self) -> i32 {
        match self.next() % 3 {
            0 => self.range(-20, 20) as i32,
            1 => self.range(-100000, 100000) as i32,
            _ => self.range(-100000000, 100000000) as i32,
        }
    }

    pub(crate) fn pt(&mut self) -> Vector2I {
        let x = self.coord();
        let y = self.coord();
        Vector2I::new(x, y)
    }

    pub(crate) fn pt_mid(&mut self) -> Vector2I {
        let x = self.coord_mid();
        let y = self.coord_mid();
        Vector2I::new(x, y)
    }

    pub(crate) fn pt_r(&mut self, lo: i64, hi: i64) -> Vector2I {
        let x = self.range(lo, hi) as i32;
        let y = self.range(lo, hi) as i32;
        Vector2I::new(x, y)
    }

    pub(crate) fn pt_l(&mut self, lo: i64, hi: i64) -> Vector2L {
        let x = self.range(lo, hi);
        let y = self.range(lo, hi);
        Vector2L::new(x, y)
    }

    pub(crate) fn pt_d(&mut self, lo: f64, hi: f64) -> Vector2D {
        let x = self.dbl(lo, hi);
        let y = self.dbl(lo, hi);
        Vector2D::new(x, y)
    }

    pub(crate) fn pt_d_scaled(&mut self, min_exp: i32, max_exp: i32) -> Vector2D {
        let x = self.dbl_scaled(min_exp, max_exp);
        let y = self.dbl_scaled(min_exp, max_exp);
        Vector2D::new(x, y)
    }

    pub(crate) fn seg(&mut self) -> Seg {
        let a = self.pt();
        let b = self.pt();
        Seg::new(a, b)
    }

    pub(crate) fn angle_deg(&mut self) -> f64 {
        match self.next() % 6 {
            0 => 45.0 * self.range(-16, 16) as f64,
            1 => self.range(-7200, 7200) as f64 / 10.0,
            2 => self.dbl(-1000.0, 1000.0),
            3 => {
                let b = 90.0 * self.range(-8, 8) as f64;
                b + self.dbl(-1e-9, 1e-9)
            }
            4 => self.dbl(-1e6, 1e6),
            _ => self.range(-360, 360) as f64,
        }
    }

    /// unit() scaled by an exact power of two (std::ldexp)
    pub(crate) fn dbl_scaled(&mut self, min_exp: i32, max_exp: i32) -> f64 {
        let u = self.unit();
        let e = self.range(min_exp as i64, max_exp as i64);
        let v = u * f64::from_bits(((1023 + e) as u64) << 52);
        let neg = self.flag();
        if neg { -v } else { v }
    }
}

/// `REC` of the oracle: the textual record of one case.
#[derive(Default)]
pub(crate) struct Rec {
    line: String,
}

impl Rec {
    pub(crate) fn sep(&mut self) {
        if !self.line.is_empty() {
            self.line.push(' ');
        }
    }

    pub(crate) fn i(&mut self, v: impl Into<i64>) -> &mut Self {
        self.sep();
        self.line.push_str(&v.into().to_string());
        self
    }

    pub(crate) fn u(&mut self, v: u64) -> &mut Self {
        self.sep();
        self.line.push_str(&v.to_string());
        self
    }

    pub(crate) fn b(&mut self, v: bool) -> &mut Self {
        self.sep();
        self.line.push(if v { '1' } else { '0' });
        self
    }

    pub(crate) fn d(&mut self, v: f64) -> &mut Self {
        self.sep();

        if v.is_nan() {
            self.line.push_str("nan");
        } else {
            self.line.push_str(&format!("{:016x}", v.to_bits()));
        }

        self
    }

    pub(crate) fn s(&mut self, v: &str) -> &mut Self {
        self.sep();
        self.line.push('"');
        self.line.push_str(v);
        self.line.push('"');
        self
    }

    pub(crate) fn v(&mut self, p: Vector2I) -> &mut Self {
        self.i(p.x).i(p.y)
    }

    pub(crate) fn vl(&mut self, p: Vector2L) -> &mut Self {
        self.i(p.x).i(p.y)
    }

    pub(crate) fn vd(&mut self, p: Vector2D) -> &mut Self {
        self.d(p.x).d(p.y)
    }

    pub(crate) fn a(&mut self, p: EdaAngle) -> &mut Self {
        self.d(p.as_degrees())
    }

    pub(crate) fn seg(&mut self, p: &Seg) -> &mut Self {
        self.v(p.a).v(p.b)
    }

    pub(crate) fn bx(&mut self, p: &Box2I) -> &mut Self {
        self.v(p.position()).vl(p.size()).b(p.is_valid())
    }

    pub(crate) fn bxd(&mut self, p: &Box2D) -> &mut Self {
        self.vd(p.position()).vd(p.size()).b(p.is_valid())
    }

    pub(crate) fn bxl(&mut self, p: &Box2L) -> &mut Self {
        self.vl(p.position()).vl(p.size()).b(p.is_valid())
    }

    pub(crate) fn vf(&mut self, p: Vector2<f32>) -> &mut Self {
        self.d(p.x as f64).d(p.y as f64)
    }

    pub(crate) fn vu(&mut self, p: Vector2<u32>) -> &mut Self {
        self.u(p.x as u64).u(p.y as u64)
    }

    pub(crate) fn opt(&mut self, p: Option<Vector2I>) -> &mut Self {
        match p {
            Some(p) => self.b(true).v(p),
            None => self.b(false),
        }
    }
}

/// The results of all sections: name, case count, hash.
pub struct Results {
    /// name, case count, hash - in the order of the oracle's output
    pub sections: Vec<(String, u64, u64)>,
    /// if set, every record is written here as `section|record`
    pub dump: Option<Box<dyn Write>>,
}

/// `SECTION` of the oracle.
pub(crate) struct Section<'a> {
    name: &'static str,
    hash: u64,
    count: u64,
    results: &'a mut Results,
}

impl<'a> Section<'a> {
    pub(crate) fn new(results: &'a mut Results, name: &'static str) -> Self {
        Section {
            name,
            hash: 1469598103934665603,
            count: 0,
            results,
        }
    }

    pub(crate) fn add(&mut self, rec: &Rec) {
        for c in rec.line.bytes() {
            self.hash ^= c as u64;
            self.hash = self.hash.wrapping_mul(1099511628211);
        }

        self.hash ^= b'\n' as u64;
        self.hash = self.hash.wrapping_mul(1099511628211);
        self.count += 1;

        if let Some(dump) = self.results.dump.as_mut() {
            writeln!(dump, "{}|{}", self.name, rec.line).unwrap();
        }
    }
}

impl Drop for Section<'_> {
    fn drop(&mut self) {
        self.results
            .sections
            .push((self.name.to_string(), self.count, self.hash));
    }
}

/// `sectionLibmProbe`
pub(crate) fn section_libm_probe(results: &mut Results) {
    let mut sec = Section::new(results, "libm_probe");
    let mut rng = Rng::new(100);

    for _ in 0..200000 {
        let deg = rng.angle_deg();
        let rad = deg * (std::f64::consts::PI / 180.0);
        let x = rng.dbl(-1e9, 1e9);
        let y = rng.dbl(-1e9, 1e9);
        let u = rng.dbl(-1.0, 1.0);
        let digits = rng.range(0, 6) as i32;

        let mut r = Rec::default();
        r.d(rad.sin()).d(rad.cos()).d(rad.tan()).d(y.atan2(x));
        r.d(x.hypot(y))
            .d(u.acos())
            .d(u.asin())
            .d((u * 100.0).atan());
        r.d(10.0f64.powf(digits as f64))
            .d((x as f32).hypot(y as f32) as f64);
        sec.add(&r);
    }
}

const K_ROUND_SPECIALS: [f64; 26] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    2147483647.5,
    -2147483648.5,
    2147483647.4999995,
    -2147483648.4999995,
    2147483646.5,
    -2147483647.5,
    9223372036854775808.0,
    -9223372036854775808.0,
    9223372036854774784.0,
    -9223372036854777856.0,
    0.49999999999999994,
    -0.49999999999999994,
    4503599627370497.0,
    -4503599627370497.0,
    4294967295.5,
    4294967294.5,
    -0.5,
    0.5,
    -0.0,
    0.0,
    1e300,
    -1e300,
    5e-324,
];

/// `sectionKiRound`
fn section_ki_round(results: &mut Results) {
    let mut sec = Section::new(results, "ki_round");
    let mut rng = Rng::new(101);

    for _ in 0..40000 {
        let v: f64 = match rng.next() % 6 {
            0 => rng.dbl(-10.0, 10.0),
            1 => rng.range(-1000000, 1000000) as f64 + 0.5,
            2 => rng.dbl(-3e9, 3e9),
            3 => rng.dbl(-1e19, 1e19),
            4 => K_ROUND_SPECIALS[(rng.next() % K_ROUND_SPECIALS.len() as u64) as usize],
            _ => {
                let base = rng.range(-2147483658, 2147483657) as f64;
                let offs = [-0.5, 0.0, 0.5, 0.25];
                base + offs[(rng.next() % 4) as usize]
            }
        };

        let f = v as f32;

        let mut r = Rec::default();
        r.d(v).i(ki_round(v)).i(ki_round_to::<i64, f64>(v));
        r.u(ki_round_to::<u32, f64>(v) as u64);
        r.i(ki_round(f)).i(ki_round_to::<i64, f32>(f));
        sec.add(&r);
    }

    for _ in 0..5000 {
        let bits = rng.next();
        let shift = rng.next() % 64;
        let mut v = (bits >> shift) as i64;

        if rng.flag() {
            v = -v;
        }

        let mut r = Rec::default();
        r.i(v).i(ki_round(v)).i(ki_round_to::<i64, i64>(v));
        r.u(ki_round_to::<u32, i64>(v) as u64);
        sec.add(&r);
    }
}

/// `anyI64`
fn any_i64(rng: &mut Rng) -> i64 {
    let bits = rng.next();
    let shift = rng.next() % 64;
    let v = (bits >> shift) as i64;
    let neg = rng.flag();
    if neg { -v } else { v }
}

/// `anyVecL`
fn any_vec_l(rng: &mut Rng) -> Vector2L {
    let x = any_i64(rng);
    let y = any_i64(rng);
    Vector2L::new(x, y)
}

/// `anyI32` (never INT_MIN)
fn any_i32(rng: &mut Rng) -> i32 {
    let bits = rng.next();
    let shift = 33 + rng.next() % 31;
    let v = (bits >> shift) as i64;
    let neg = rng.flag();
    (if neg { -v } else { v }) as i32
}

/// `anyVecI`
fn any_vec_i(rng: &mut Rng) -> Vector2I {
    let x = any_i32(rng);
    let y = any_i32(rng);
    Vector2I::new(x, y)
}

/// `sectionRescale`
fn section_rescale(results: &mut Results) {
    let mut sec = Section::new(results, "rescale");
    let mut rng = Rng::new(102);

    for _ in 0..30000 {
        let num = any_i64(&mut rng);
        let val = any_i64(&mut rng);
        let mut den = any_i64(&mut rng);

        if den == 0 {
            den = 1;
        }

        let mut r = Rec::default();
        r.i(num).i(val).i(den).i(rescale(num, val, den));

        let inum = any_i32(&mut rng);
        let ival = any_i32(&mut rng);
        let mut iden = any_i32(&mut rng);

        if iden == 0 {
            iden = 1;
        }

        r.i(inum).i(ival).i(iden).i(rescale(inum, ival, iden));

        let dn = rng.dbl_scaled(-10, 40);
        let dv = rng.dbl_scaled(-10, 40);
        let dd = rng.dbl_scaled(-10, 40);
        r.d(rescale(dn, dv, dd));
        r.i(sign(num)).i(sign(dn));
        sec.add(&r);
    }
}

/// `sectionIsqrt`
fn section_isqrt(results: &mut Results) {
    let mut sec = Section::new(results, "isqrt_hypot");
    let mut rng = Rng::new(103);

    for n in 0..30000 {
        let mut x = any_i64(&mut rng);
        let ubits = rng.next();
        let ushift = rng.next() % 64;
        let mut ux = ubits >> ushift;
        let hx = any_i64(&mut rng);
        let hy = any_i64(&mut rng);

        if n % 7 == 0 {
            // perfect squares and their neighbours
            let rbits = rng.next();
            let rshift = 32 + rng.next() % 32;
            let root = rbits >> rshift;
            ux = (root * root).wrapping_add(rng.range(-1, 1) as u64);
            x = ((root >> 1) * (root >> 1)) as i64 + rng.range(-1, 1);
        }

        let mut r = Rec::default();
        r.i(x)
            .i(isqrt(x))
            .u(ux)
            .u(isqrt(ux))
            .i(hx)
            .i(hy)
            .u(rounded_hypot(hx, hy));
        r.u(unsigned_abs(x));
        sec.add(&r);
    }
}

/// `sectionVector2I`
fn section_vector2i(results: &mut Results) {
    let mut sec = Section::new(results, "vector2i");
    let mut rng = Rng::new(104);

    for _ in 0..30000 {
        let full = any_vec_i(&mut rng);
        let a = rng.pt();
        let b = rng.pt();

        let mut r = Rec::default();
        r.v(full)
            .i(full.euclidean_norm())
            .i(full.squared_euclidean_norm());
        r.v(a)
            .v(b)
            .i(a.cross(b))
            .i(a.dot(b))
            .i(a.squared_distance(b))
            .i(a * b);
        r.v(a.perpendicular()).v(a + b).v(a - b).v(-a);
        r.b(a < b)
            .b(a <= b)
            .b(a > b)
            .b(a >= b)
            .b(a == b)
            .i(lexicographical_compare(&a, &b));
        r.v(*lexicographical_max(&a, &b))
            .v(*lexicographical_min(&a, &b));

        let len = if rng.flag() {
            rng.range(-1000000000, 1000000000) as i32
        } else {
            rng.range(-100, 100) as i32
        };
        r.i(len).v(a.resize(len));

        let divs = [2.0, -3.0, 0.5, 1.0, 7.0];
        let f = if rng.flag() {
            divs[(rng.next() % 5) as usize]
        } else {
            rng.dbl(0.1, 10.0)
        };
        r.d(f).v(a / f);

        let sc = rng.dbl(-5.0, 5.0);
        r.d(sc).v(a + sc).v(a - sc).vd(a * sc);
        r.s(&a.format()).s(&a.to_stream_string());
        sec.add(&r);
    }
}

/// `sectionVector2L`
fn section_vector2l(results: &mut Results) {
    let mut sec = Section::new(results, "vector2l");
    let mut rng = Rng::new(105);

    for n in 0..20000 {
        let full = any_vec_l(&mut rng);
        let mut a = rng.pt_l(-1000000000, 1000000000);
        let b = rng.pt_l(-1000000000, 1000000000);

        if n % 5 == 0 {
            a = rng.pt_l(-50, 50);
        }

        let mut r = Rec::default();
        r.vl(full)
            .i(full.euclidean_norm())
            .i(full.squared_euclidean_norm());
        r.v(full.cast::<i32>()).vd(full.cast::<f64>());
        r.vl(a)
            .vl(b)
            .i(a.cross(b))
            .i(a.dot(b))
            .i(a.squared_distance(b));

        let len = if rng.flag() {
            rng.range(-3000000000, 3000000000)
        } else {
            rng.range(-100, 100)
        };
        r.i(len).vl(a.resize(len));

        let f = rng.dbl(0.1, 10.0);
        r.d(f).vl(a / f).vl(full / f);

        // 128-bit helpers
        r.d(to_double(cross_wide(full, b)))
            .d(to_double(dot_wide(a, b)));
        sec.add(&r);
    }
}

/// `sectionVector2D`
fn section_vector2d(results: &mut Results) {
    let mut sec = Section::new(results, "vector2d");
    let mut rng = Rng::new(106);

    for _ in 0..30000 {
        let a: Vector2D = match rng.next() % 4 {
            0 => rng.pt_d(-1e9, 1e9),
            1 => rng.pt().cast::<f64>(),
            2 => {
                let m = rng.dbl(-1e6, 1e6);
                Vector2D::new(m, if rng.flag() { m } else { -m })
            }
            _ => rng.pt_d_scaled(-30, 62),
        };

        let b = rng.pt_d(-1e6, 1e6);
        let len = if rng.flag() {
            rng.dbl(-1e6, 1e6)
        } else {
            rng.range(-10, 10) as f64
        };

        let mut r = Rec::default();
        r.vd(a)
            .d(a.squared_euclidean_norm())
            .vd(a.resize(len))
            .d(a.cross(b))
            .d(a.dot(b));
        r.d(a.squared_distance(b))
            .vd(a.perpendicular())
            .vd(a + b)
            .vd(a - b)
            .vd(a / len);
        r.v(a.cast::<i32>()).v(ki_round(a));
        r.b(a < b).b(a >= b);

        if a.x.abs() < 9e18 && a.y.abs() < 9e18 {
            r.vl(a.cast::<i64>());
        }

        r.s(&a.format()).s(&a.to_stream_string());
        r.b(equals_vec(a, b))
            .b(equals_vec(a, a + 1e-12))
            .b(equals_vec_eps(a, b, 1e6));

        // KiROUND( x, y ) and the mixed-type operators
        let ia = rng.pt();
        r.v(ki_round_xy(a.x / 16.0, a.y / 16.0))
            .vd(ia + b)
            .vd(b - ia)
            .d(ia * b);
        sec.add(&r);
    }
}

/// `sectionVector2DNorm`
fn section_vector2d_norm(results: &mut Results) {
    let mut sec = Section::new(results, "libm:vector2d_norm");
    let mut rng = Rng::new(107);

    for n in 0..20000 {
        let mut a = rng.pt_d(-1e9, 1e9);

        if n % 4 == 0 {
            a = rng.pt_d_scaled(-30, 62);
        }

        let p = rng.pt();
        let q = rng.pt();

        let mut r = Rec::default();
        r.vd(a).d(a.euclidean_norm()).v(p).v(q).d(p.distance(q));
        sec.add(&r);
    }
}

/// `sectionEdaAngle`
fn section_eda_angle(results: &mut Results) {
    let mut sec = Section::new(results, "eda_angle");
    let mut rng = Rng::new(108);

    let steps = [90.0, 45.0, 15.0, 0.0, -5.0, 0.1, 360.0];

    for _ in 0..40000 {
        let deg = rng.angle_deg();
        let deg2 = rng.angle_deg();
        let a = EdaAngle::new(deg, DEGREES_T);
        let b = EdaAngle::new(deg2, DEGREES_T);

        let mut r = Rec::default();
        r.d(deg);
        r.a({ a }.normalize()).a({ a }.normalize_negative());
        r.a({ a }.normalize90()).a({ a }.normalize180());
        r.a({ a }.normalize720()).a(a.normalized());
        r.b(a.is_cardinal())
            .b(a.is_cardinal90())
            .b(a.is_zero())
            .b(a.is_horizontal())
            .b(a.is_vertical());
        r.a(a.keep_upright())
            .i(a.as_tenths_of_a_degree())
            .d(a.as_radians());
        r.d(deg2)
            .b(a.is_parallel_to(b))
            .a(a.reflect(b))
            .a(a + b)
            .a(a - b)
            .a(-a);
        r.a(a * 0.5)
            .a(a / 3.0)
            .d(a / b)
            .b(a < b)
            .b(a == b)
            .a(a.abs());
        r.a(a.snapped(EdaAngle::new(steps[(rng.next() % 7) as usize], DEGREES_T)));
        r.a(EdaAngle::new(deg, TENTHS_OF_A_DEGREE_T))
            .a(EdaAngle::new(deg / 64.0, RADIANS_T));
        r.s(&a.to_string());

        // The exact cases of the vector constructor (axes and diagonals)
        let m = rng.range(-1000, 1000) as i32;
        let dirs = [
            Vector2I::new(m, 0),
            Vector2I::new(0, m),
            Vector2I::new(m, m),
            Vector2I::new(m, -m),
            Vector2I::new(0, 0),
        ];
        r.a(EdaAngle::from_vector(dirs[(rng.next() % 5) as usize]));

        let o = EdaOrientation::new(a);
        r.a(o.angle())
            .a((o + b).angle())
            .a((o - b).angle())
            .a((-o).angle());
        r.a(o.reflect(b).angle())
            .a((b - o).angle())
            .a(o - EdaOrientation::new(b));

        // Sin/Cos are exact (no libm) for multiples of 45 degrees
        let c = EdaAngle::new(45.0 * rng.range(-24, 24) as f64, DEGREES_T);
        r.a(c).d(c.sin()).d(c.cos());
        sec.add(&r);
    }
}

/// `sectionEdaAngleTrig`
fn section_eda_angle_trig(results: &mut Results) {
    let mut sec = Section::new(results, "libm:eda_angle_trig");
    let mut rng = Rng::new(109);

    for _ in 0..20000 {
        let deg = rng.angle_deg();
        let a = EdaAngle::new(deg, DEGREES_T);
        let vd = rng.pt_d(-1e6, 1e6);
        let vi = rng.pt();
        let x = rng.dbl(-1.0, 1.0);

        let mut r = Rec::default();
        r.d(deg).d(a.sin()).d(a.cos()).d(a.tan());
        r.vd(vd)
            .a(EdaAngle::from_vector(vd))
            .v(vi)
            .a(EdaAngle::from_vector(vi));
        r.d(x)
            .a(EdaAngle::arccos(x))
            .a(EdaAngle::arcsin(x))
            .a(EdaAngle::arctan(x * 100.0));
        r.a(EdaAngle::arctan2(vd.y, vd.x));
        r.a(a.round(rng.range(0, 6) as i32));
        sec.add(&r);
    }
}

/// The common body of `sectionRotateExact` and `sectionRotateLibm`.
fn rotate_record(rng: &mut Rng, angle: EdaAngle, with_get_rotated: bool) -> Rec {
    let p = rng.pt();
    let c = rng.pt();
    let pd = rng.pt_d(-1e9, 1e9);
    let cd = rng.pt_d(-1e9, 1e9);

    let mut p1 = p;
    rotate_point(&mut p1, angle);
    let mut p2 = p;
    rotate_point_around(&mut p2, c, angle);
    let mut d1 = pd;
    rotate_point(&mut d1, angle);
    let mut d2 = pd;
    rotate_point_around(&mut d2, cd, angle);

    let mut r = Rec::default();
    r.a(angle).v(p).v(c).v(p1).v(p2).vd(pd).vd(cd).vd(d1).vd(d2);

    if with_get_rotated {
        r.v(get_rotated(p, angle))
            .v(get_rotated_around(p, c, angle))
            .vd(get_rotated(pd, angle));
    }

    r
}

/// `sectionRotateExact`
fn section_rotate_exact(results: &mut Results) {
    let mut sec = Section::new(results, "rotate_point_exact");
    let mut rng = Rng::new(110);

    for _ in 0..30000 {
        // Multiples of 45 degrees never reach libm
        let angle = EdaAngle::new(45.0 * rng.range(-24, 24) as f64, DEGREES_T);
        let r = rotate_record(&mut rng, angle, true);
        sec.add(&r);
    }
}

/// `sectionRotateLibm`
fn section_rotate_libm(results: &mut Results) {
    let mut sec = Section::new(results, "libm:rotate_point");
    let mut rng = Rng::new(111);

    for _ in 0..30000 {
        let angle = EdaAngle::new(rng.angle_deg(), DEGREES_T);
        let r = rotate_record(&mut rng, angle, false);
        sec.add(&r);
    }
}

/// `arcPoints`
pub(crate) fn arc_points(rng: &mut Rng) -> (Vector2I, Vector2I, Vector2I) {
    match rng.next() % 8 {
        0 => {
            // axis-aligned points of a circle
            let c = rng.pt_mid();
            let rad = rng.range(1, 100000000) as i32;
            let q = [
                c + Vector2I::new(rad, 0),
                c + Vector2I::new(0, rad),
                c + Vector2I::new(-rad, 0),
                c + Vector2I::new(0, -rad),
            ];
            let k = (rng.next() % 4) as usize;
            let s = q[k];
            let m = q[(k + 1) % 4];
            let e = q[(k + 2 + (rng.next() % 2) as usize) % 4];
            (s, m, e)
        }
        1 => {
            // collinear
            let a = rng.pt_mid();
            let d = rng.pt_r(-1000, 1000);
            let s = a;
            let m = a + d * rng.range(-1000, 1000) as i32;
            let e = a + d * rng.range(-1000, 1000) as i32;
            (s, m, e)
        }
        2 => {
            // nearly coincident
            let a = rng.pt_mid();
            let s = a;
            let m = a + rng.pt_r(-6, 6);
            let e = if rng.flag() {
                a + rng.pt_r(-6, 6)
            } else {
                rng.pt_mid()
            };
            (s, m, e)
        }
        3 => {
            // full circle
            let s = rng.pt_mid();
            let m = rng.pt_mid();
            (s, m, s)
        }
        _ => {
            let s = rng.pt();
            let m = rng.pt();
            let e = rng.pt();
            (s, m, e)
        }
    }
}

/// `sectionArcCenter`
fn section_arc_center(results: &mut Results) {
    let mut sec = Section::new(results, "libm:arc_center");
    let mut rng = Rng::new(112);

    for _ in 0..30000 {
        let (s, m, e) = arc_points(&mut rng);

        let mut ds: Vector2D = s.cast();
        let mut dm: Vector2D = m.cast();
        let de: Vector2D = e.cast();

        if rng.flag() {
            ds += rng.pt_d(-0.5, 0.5);
            dm += rng.pt_d(-0.5, 0.5);
        }

        let angle = EdaAngle::new(rng.angle_deg(), DEGREES_T);

        let mut r = Rec::default();
        r.v(s)
            .v(m)
            .v(e)
            .v(calc_arc_center(s, m, e))
            .b(is_straight_arc(s, m, e));
        r.vd(ds).vd(dm).vd(de).vd(calc_arc_center_d(ds, dm, de));
        r.a(angle).vd(calc_arc_center_from_angle(ds, de, angle));

        let c = rng.pt_mid();
        let arc_s = rng.pt_mid();
        let arc_e = rng.pt_mid();
        r.v(c)
            .v(arc_s)
            .v(arc_e)
            .v(calc_arc_mid(arc_s, arc_e, c, true))
            .v(calc_arc_mid(arc_s, arc_e, c, false));
        sec.add(&r);
    }
}

/// `segPair`
pub(crate) fn seg_pair(rng: &mut Rng) -> (Seg, Seg) {
    let a = rng.seg();

    let in_range = |v: i64| (-500000000..=500000000).contains(&v);

    match rng.next() % 8 {
        4 => {
            let shared = if rng.flag() { a.a } else { a.b };
            let other = rng.pt();
            return (a, Seg::new(shared, other));
        }
        5 => {
            // collinear
            let dx = a.b.x as i64 - a.a.x as i64;
            let dy = a.b.y as i64 - a.a.y as i64;
            let k1 = rng.range(-3, 3);
            let k2 = rng.range(-3, 3);
            let (x1, y1) = (a.a.x as i64 + dx * k1, a.a.y as i64 + dy * k1);
            let (x2, y2) = (a.a.x as i64 + dx * k2, a.a.y as i64 + dy * k2);

            if in_range(x1) && in_range(y1) && in_range(x2) && in_range(y2) {
                return (
                    a,
                    Seg::from_coords(x1 as i32, y1 as i32, x2 as i32, y2 as i32),
                );
            }
        }
        6 => {
            // parallel, slightly offset
            let ox = rng.range(-5, 5);
            let oy = rng.range(-5, 5);
            let (x1, y1) = (a.a.x as i64 + ox, a.a.y as i64 + oy);
            let (x2, y2) = (a.b.x as i64 + ox, a.b.y as i64 + oy);

            if in_range(x1) && in_range(y1) && in_range(x2) && in_range(y2) {
                return (
                    a,
                    Seg::from_coords(x1 as i32, y1 as i32, x2 as i32, y2 as i32),
                );
            }
        }
        7 => {
            // zero length
            let p = match rng.next() % 3 {
                0 => a.a,
                1 => Vector2I::new(
                    ((a.a.x as i64 + a.b.x as i64) / 2) as i32,
                    ((a.a.y as i64 + a.b.y as i64) / 2) as i32,
                ),
                _ => rng.pt(),
            };

            return (a, Seg::new(p, p));
        }
        _ => {}
    }

    let b = rng.seg();
    (a, b)
}

/// `sectionTrigoMisc`
fn section_trigo_misc(results: &mut Results) {
    let mut sec = Section::new(results, "trigo_misc");
    let mut rng = Rng::new(113);

    for _ in 0..40000 {
        let (a, b) = seg_pair(&mut rng);

        let p = if rng.flag() {
            rng.pt()
        } else {
            let near = a.nearest_point(rng.pt());
            near + rng.pt_r(-2, 2)
        };

        let dist = if rng.flag() {
            rng.range(0, 20) as i32
        } else {
            rng.range(0, 1000000) as i32
        };

        let mut r = Rec::default();
        r.seg(&a).seg(&b).v(p).i(dist);
        r.b(is_point_on_segment(a.a, a.b, p))
            .b(test_segment_hit(p, a.a, a.b, dist));

        let hit = segment_intersects_segment(a.a, a.b, b.a, b.b);
        r.b(hit);

        if hit {
            let (hit2, ip) = segment_intersects_segment_with_point(a.a, a.b, b.a, b.b);
            r.b(hit2).v(ip.unwrap());
        }

        r.i(normalize_angle_pos(rng.range(-20000, 20000) as i32));
        r.i(normalize_angle180(rng.range(-20000, 20000) as i32));
        r.d(normalize_angle_pos(rng.dbl(-20000.0, 20000.0)));
        r.d(normalize_angle180(rng.dbl(-20000.0, 20000.0)));

        let a1 = rng.dbl(0.0, 360.0);
        let a2 = rng.dbl(0.0, 360.0);
        r.b(intercepts_positive_x(a1, a2))
            .b(intercepts_negative_x(a1, a2));
        r.d(deg2rad(a1)).d(rad2deg(a1)).d(rad2decideg(a1));
        sec.add(&r);
    }
}

/// `sectionSegPoint`
fn section_seg_point(results: &mut Results) {
    let mut sec = Section::new(results, "seg_point");
    let mut rng = Rng::new(114);

    for n in 0..60000 {
        let mut a = rng.seg();

        if n % 16 == 0 {
            a.b = a.a;
        }

        let p = match rng.next() % 4 {
            0 => {
                let near = a.nearest_point(rng.pt());
                near + rng.pt_r(-3, 3)
            }
            1 => {
                if rng.flag() {
                    a.a
                } else {
                    a.b
                }
            }
            _ => rng.pt(),
        };

        let mut r = Rec::default();
        r.seg(&a).v(p);
        r.i(a.squared_distance(p))
            .i(a.distance(p))
            .v(a.nearest_point(p))
            .v(a.line_project(p));
        r.v(a.reflect_point(p))
            .i(a.line_distance(p))
            .i(a.line_distance_sided(p, true))
            .i(a.side(p));
        r.b(a.contains(p))
            .b(a.contains_with_threshold(p, 0))
            .b(a.contains_with_threshold(p, 100))
            .i(a.t_coef(p));
        r.i(a.length()).i(a.squared_length()).v(a.center());
        r.seg(&a.perpendicular_seg(p))
            .seg(&a.parallel_seg(p))
            .seg(&a.reversed());

        let (qa, qb, qc) = a.canonical_coefs();
        r.i(qa).i(qb).i(qc).i(Seg::square(p.x)).s(&a.to_string());
        sec.add(&r);
    }
}

/// `sectionSegSeg`
fn section_seg_seg(results: &mut Results) {
    let mut sec = Section::new(results, "seg_seg");
    let mut rng = Rng::new(115);

    for n in 0..80000 {
        let (mut a, b) = seg_pair(&mut rng);

        if n % 32 == 0 {
            a.b = a.a;
        }

        let clearance = if rng.flag() {
            rng.range(-2, 50) as i32
        } else {
            rng.range(0, 100000000) as i32
        };
        let thr = rng.range(0, 10) as i32;

        let mut r = Rec::default();
        r.seg(&a).seg(&b).i(clearance).i(thr);

        let (coll, actual) = a.collide_actual(&b, clearance);
        r.b(coll).i(actual).b(a.collide(&b, clearance));
        r.i(a.squared_distance_seg(&b))
            .i(a.distance_seg(&b))
            .v(a.nearest_point_seg(&b));

        let (pa, pb, dsq) = a.nearest_points(&b);
        r.b(true).v(pa).v(pb).i(dsq);

        r.opt(a.intersect(&b))
            .opt(a.intersect_with(&b, true, false))
            .opt(a.intersect_with(&b, false, true));
        r.opt(a.intersect_with(&b, true, true))
            .opt(a.intersect_lines(&b))
            .b(a.intersects(&b));
        r.b(a.collinear(&b))
            .b(a.approx_collinear(&b))
            .b(a.approx_collinear_with_threshold(&b, thr));
        r.b(a.approx_parallel(&b))
            .b(a.approx_parallel_with_threshold(&b, thr))
            .b(a.approx_perpendicular(&b));
        r.b(a.overlaps(&b))
            .b(a.approx_overlaps(&b))
            .b(a.approx_overlaps_with_threshold(&b, thr));
        r.b(a.contains_seg(&b));
        r.b(a == b).b(a != b).b(a < b);
        sec.add(&r);
    }
}

/// `sectionSegAngle`
fn section_seg_angle(results: &mut Results) {
    let mut sec = Section::new(results, "libm:seg_angle");
    let mut rng = Rng::new(116);

    for _ in 0..20000 {
        let (a, b) = seg_pair(&mut rng);

        let mut r = Rec::default();
        r.seg(&a).seg(&b).a(a.angle(&b));
        sec.add(&r);
    }
}

/// `sectionSegLine`
fn section_seg_line(results: &mut Results) {
    let mut sec = Section::new(results, "seg_line");
    let mut rng = Rng::new(117);

    let slopes = [0.0, 1.0, -1.0, 0.5, 2.0, 1000.0, -0.001];

    for n in 0..40000 {
        let mut a = rng.seg();

        if n % 5 == 0 {
            a.b.x = a.a.x;
        }

        if n % 11 == 0 {
            a.b = a.a;
        }

        let slope = if rng.flag() {
            slopes[(rng.next() % 7) as usize]
        } else {
            rng.dbl(-5.0, 5.0)
        };

        let offset = match rng.next() % 3 {
            0 => rng.dbl(-1e9, 1e9),
            1 => a.a.y as f64 - slope * a.a.x as f64 + rng.range(-2, 2) as f64,
            _ => rng.range(-1000, 1000) as f64,
        };

        let result = a.intersects_line(slope, offset);

        let mut r = Rec::default();
        r.seg(&a)
            .d(slope)
            .d(offset)
            .b(result.is_some())
            .v(result.unwrap_or(Vector2I::new(-777, -777)));
        sec.add(&r);
    }
}

/// `genBox`
fn gen_box(rng: &mut Rng) -> Box2I {
    let pos = rng.pt_mid();

    let size = match rng.next() % 4 {
        0 => rng.pt_l(-50, 50),
        1 => rng.pt_l(0, 200000),
        2 => rng.pt_l(-200000000, 200000000),
        _ => rng.pt_l(0, 200000000),
    };

    Box2I::new(pos, size)
}

/// `sectionBox2I`
fn section_box2i(results: &mut Results) {
    let mut sec = Section::new(results, "box2i");
    let mut rng = Rng::new(118);

    for n in 0..60000 {
        let a = gen_box(&mut rng);
        let mut b = gen_box(&mut rng);

        if n % 6 == 0 {
            // b near a
            b = a;
            b.move_(rng.pt_r(-100, 100));
            let infl = rng.pt_r(-60, 60);
            b.inflate_xy(infl.x, infl.y);
        }

        if n % 9 == 0 {
            // raw (non-normalized) size
            b.set_size(rng.pt_l(-1000, 1000));
        }

        let p = if rng.flag() {
            rng.pt_mid()
        } else {
            a.origin() + rng.pt_r(-10, 200010)
        };

        let q = rng.pt_mid();

        let mut r = Rec::default();
        r.bx(&a).bx(&b).v(p).v(q);
        r.v(a.centre())
            .v(a.end())
            .i(a.right())
            .i(a.bottom())
            .i(a.size_max());
        r.i(a.area()).i(a.diagonal()).i(a.squared_diagonal());
        r.b(a.contains(p))
            .b(a.contains_box(&b))
            .b(b.contains(p))
            .b(a.intersects(&b))
            .b(b.intersects(&a));
        r.bx(&a.intersect(&b))
            .b(a.intersects_segment(p, q))
            .b(b.intersects_segment(p, q));
        r.i(a.squared_distance_point(p)).i(a.distance_point(p));
        r.i(a.squared_distance_box(&b)).i(a.distance_box(&b));
        r.i(b.squared_distance_point(p))
            .i(b.squared_distance_box(&a));
        r.v(a.nearest_point(p))
            .v(a.farthest_point_to(p))
            .v(b.nearest_point(p))
            .v(b.farthest_point_to(p));

        let radius = rng.range(0, 300000) as i32;
        let width = rng.range(0, 1000) as i32;
        r.i(radius)
            .i(width)
            .b(a.intersects_circle(p, radius))
            .b(a.intersects_circle_edge(p, radius, width));

        let dx;
        let dy;

        match rng.next() % 3 {
            0 => {
                dx = rng.range(-100, 100) as i32;
                dy = rng.range(-100, 100) as i32;
            }
            1 => {
                dx = rng.range(-300000000, 300000000) as i32;
                dy = rng.range(-300000000, 300000000) as i32;
            }
            _ => {
                dx = any_i32(&mut rng);
                dy = any_i32(&mut rng);
            }
        }

        r.i(dx).i(dy).bx(&a.inflated_xy(dx, dy)).bx(&b.inflated(dx));

        let mut m1 = a;
        m1.merge(&b);
        let mut m2 = b;
        m2.merge_point(p);
        let mut m3 = Box2I::default();
        m3.merge(&b);
        let mut m4 = Box2I::default();
        m4.merge_point(p);
        r.bx(&m1).bx(&m2).bx(&m3).bx(&m4);

        r.b(a == b)
            .b(a != b)
            .bx(&Box2I::by_corners(p, q))
            .bx(&Box2I::by_center(p, a.size()));
        r.bx(&a.with_offset(q)).b(a.is_box2_safe()).s(&a.format());

        // exact rotations (multiples of 45 degrees stay clear of libm)
        let angle = EdaAngle::new(45.0 * rng.range(-8, 8) as f64, DEGREES_T);
        r.a(angle)
            .b(a.intersects_rotated(&b, angle))
            .bx(&a.bounding_box_rotated(q, angle));

        let pts = [p, q, a.origin(), b.end()];
        let mut c = Box2I::default();
        c.compute(&pts);
        r.bx(&c);
        sec.add(&r);
    }
}

/// `sectionBox2IRotated`
fn section_box2i_rotated(results: &mut Results) {
    let mut sec = Section::new(results, "libm:box2i_rotated");
    let mut rng = Rng::new(119);

    for n in 0..30000 {
        let a = gen_box(&mut rng);
        let mut b = gen_box(&mut rng);

        if n % 3 == 0 {
            b = a;
            b.move_(rng.pt_r(-100000, 100000));
        }

        let angle = EdaAngle::new(rng.angle_deg(), DEGREES_T);
        let c = rng.pt_mid();

        let mut r = Rec::default();
        r.bx(&a).bx(&b).a(angle).v(c);
        r.b(a.intersects_rotated(&b, angle))
            .bx(&a.bounding_box_rotated(c, angle));
        sec.add(&r);
    }
}

/// `sectionBox2D`
fn section_box2d(results: &mut Results) {
    let mut sec = Section::new(results, "box2d");
    let mut rng = Rng::new(120);

    for _ in 0..30000 {
        let apos = rng.pt_d(-1e6, 1e6);
        let asize = rng.pt_d(-1e6, 1e6);
        let bpos = rng.pt_d(-1e6, 1e6);
        let bsize = rng.pt_d(0.0, 1e6);
        let a = Box2D::new(apos, asize);
        let b = Box2D::new(bpos, bsize);
        let p = rng.pt_d(-2e6, 2e6);
        let d = rng.dbl(-1e5, 1e5);

        let mut r = Rec::default();
        r.bxd(&a).bxd(&b).vd(p).d(d);
        r.vd(a.centre())
            .vd(a.end())
            .d(a.area())
            .d(a.squared_diagonal());
        r.b(a.contains(p))
            .b(a.contains_box(&b))
            .b(a.intersects(&b))
            .bxd(&a.intersect(&b));
        r.d(a.squared_distance_point(p)).d(a.distance_point(p));
        r.d(a.squared_distance_box(&b)).d(a.distance_box(&b));
        r.vd(a.nearest_point(p))
            .vd(a.farthest_point_to(p))
            .bxd(&a.inflated(d));

        let mut m1 = a;
        m1.merge(&b);
        let mut m2 = b;
        m2.merge_point(p);
        r.bxd(&m1).bxd(&m2).b(a == b).i(a.size_max());

        let radius = rng.range(0, 1000000) as i32;
        let width = rng.range(0, 1000) as i32;
        r.b(a.intersects_circle(p, radius))
            .b(a.intersects_circle_edge(p, radius, width));
        r.bx(&box2i_safe(&a))
            .bx(&ki_round(a))
            .bx(&box2i_safe_from_pos_size(p * 4096.0, a.size() * 4096.0));
        r.b(a.is_box2_safe()).s(&a.format());
        sec.add(&r);
    }
}

/// `sectionVectorUtils`
fn section_vector_utils(results: &mut Results) {
    let mut sec = Section::new(results, "vector_utils");
    let mut rng = Rng::new(121);

    for _ in 0..30000 {
        let a = rng.seg();
        let p = if rng.flag() {
            rng.pt()
        } else {
            a.nearest_point(rng.pt())
        };
        let dir = rng.pt();
        let grid = rng.range(1, 1000) as i32;
        let g = rng.pt_r(-1000000, 1000000);

        let mut r = Rec::default();
        r.seg(&a).v(p).v(dir).i(grid).v(g);
        r.b(point_is_in_direction(p, dir, a.a))
            .b(points_are_in_same_direction(p, a.b, a.a));
        r.b(seg_is_in_direction(&a, dir))
            .b(point_projects_onto_segment(p, &a));
        r.d(get_length_ratio_from_start(p, &a))
            .d(get_projected_point_length_ratio(p, &a));
        r.v(round_grid(g, grid))
            .v(round_nw(g, grid))
            .v(round_se(g, grid));
        sec.add(&r);
    }
}

/// `sectionNearestEndpoint`
fn section_nearest_endpoint(results: &mut Results) {
    let mut sec = Section::new(results, "libm:nearest_endpoint");
    let mut rng = Rng::new(122);

    for _ in 0..20000 {
        let a = rng.seg();
        let p = rng.pt();

        let mut r = Rec::default();
        r.seg(&a).v(p).v(get_nearest_endpoint(&a, p));
        sec.add(&r);
    }
}

/// `sectionVector2FloatUnsigned`
///
/// VECTOR2<float> and VECTOR2<unsigned> are only instantiated by the C++ unit tests, but the
/// port supports them, so check them as well.
fn section_vector2_float_unsigned(results: &mut Results) {
    let mut sec = Section::new(results, "vector2_float_unsigned");
    let mut rng = Rng::new(123);

    for n in 0..20000 {
        let mut fx = rng.dbl(-1e6, 1e6) as f32;
        let mut fy = rng.dbl(-1e6, 1e6) as f32;

        if n % 4 == 0 {
            fx = rng.range(-100, 100) as f32;
            fy = if rng.flag() { fx } else { -fx };
        }

        let f = Vector2::<f32>::new(fx, fy);
        let flen = rng.dbl(-1e3, 1e3) as f32;

        let mut r = Rec::default();
        r.vf(f)
            .d(flen as f64)
            .d(f.squared_euclidean_norm() as f64)
            .vf(f.resize(flen));
        r.d(f.cross(f.perpendicular()) as f64);
        r.d(f.dot(f) as f64)
            .v(f.cast::<i32>())
            .vl(f.cast::<i64>())
            .vd(f.cast::<f64>())
            .vu(f.cast::<u32>());
        r.vf(f - 1)
            .vf(f - 5.0)
            .vf(f + 1u32)
            .vf(f / 3.0)
            .vf(-f)
            .s(&f.format());

        let mut ux = rng.next() as u32;
        let mut uy = rng.next() as u32;

        if n % 3 == 0 {
            ux = rng.range(0, 10) as u32;
            uy = rng.range(0, 10) as u32;
        }

        let u = Vector2::<u32>::new(ux, uy);
        let u2 = u - 1;
        let anyi = any_vec_i(&mut rng);

        r.vu(u)
            .vu(u2)
            .vu(u + 7u32)
            .vu(u - 5.0)
            .vu(u * 3u32)
            .vu(-u)
            .vu(u.perpendicular())
            .vu(u / 2.0);
        r.u(u.squared_euclidean_norm() as u64)
            .u(u.cross(u2) as u64)
            .u(u.dot(u2) as u64);
        r.u(u.squared_distance(u2) as u64);
        r.v(u.cast::<i32>())
            .vl(u.cast::<i64>())
            .vd(u.cast::<f64>())
            .vf(u.cast::<f32>());
        r.v(anyi)
            .vu(anyi.cast::<u32>())
            .vf(anyi.cast::<f32>())
            .s(&u.format());
        r.b(u < u2).b(u == u2);
        sec.add(&r);
    }
}

/// `sectionVector2FloatNorm`
fn section_vector2_float_norm(results: &mut Results) {
    let mut sec = Section::new(results, "libm:vector2f_norm");
    let mut rng = Rng::new(124);

    for n in 0..10000 {
        let mut fx = rng.dbl(-1e6, 1e6) as f32;
        let mut fy = rng.dbl(-1e6, 1e6) as f32;

        if n % 4 == 0 {
            fx = rng.range(-100, 100) as f32;
            fy = if rng.flag() {
                fx
            } else if rng.flag() {
                -fx
            } else {
                0.0
            };
        }

        let f = Vector2::<f32>::new(fx, fy);

        let mut r = Rec::default();
        r.vf(f).d(f.euclidean_norm() as f64);
        sec.add(&r);
    }
}

/// `sectionBox2L`
fn section_box2l(results: &mut Results) {
    let mut sec = Section::new(results, "box2l");
    let mut rng = Rng::new(125);

    for _ in 0..20000 {
        // A large common offset makes every coordinate need more than 32 bits
        let k = rng.range(-1000000000000, 1000000000000);
        let koff = Vector2L::new(k, k);
        let apos = rng.pt_l(-500000000, 500000000) + koff;
        let asize = rng.pt_l(-500000000, 500000000);
        let bpos = rng.pt_l(-500000000, 500000000) + koff;
        let bsize = rng.pt_l(0, 500000000);
        let a = Box2L::new(apos, asize);
        let b = Box2L::new(bpos, bsize);
        let p = rng.pt_l(-1000000000, 1000000000) + koff;
        let d = rng.range(-3000000000, 3000000000);

        let mut r = Rec::default();
        r.bxl(&a).bxl(&b).vl(p).i(d);
        r.vl(a.centre())
            .vl(a.end())
            .i(a.area())
            .i(a.diagonal())
            .i(a.squared_diagonal());
        r.i(a.size_max())
            .b(a.contains(p))
            .b(a.contains_box(&b))
            .b(a.intersects(&b))
            .bxl(&a.intersect(&b));
        r.i(a.squared_distance_point(p)).i(a.distance_point(p));
        r.i(a.squared_distance_box(&b)).i(a.distance_box(&b));
        r.vl(a.nearest_point(p))
            .vl(a.farthest_point_to(p))
            .bxl(&a.inflated(d))
            .bxl(&b.inflated_xy(d, -d));

        let mut m1 = a;
        m1.merge(&b);
        let mut m2 = b;
        m2.merge_point(p);
        r.bxl(&m1)
            .bxl(&m2)
            .b(a == b)
            .b(a.is_box2_safe())
            .s(&a.format());
        r.bxl(&Box2L::by_corners(apos, p))
            .bxl(&Box2L::by_center(p, bsize));
        sec.add(&r);
    }

    let mut max_i = Box2I::default();
    max_i.set_maximum();
    let mut max_l = Box2L::default();
    max_l.set_maximum();
    let mut max_d = Box2D::default();
    max_d.set_maximum();

    let mut r = Rec::default();
    r.bx(&max_i).bxl(&max_l).bxd(&max_d);
    r.v(max_i.end())
        .v(max_i.centre())
        .b(max_i.contains(Vector2I::new(123456789, -987654321)));
    sec.add(&r);
}

/// Run all sections, in the same order as `main()` of the oracle.
pub fn run_all(results: &mut Results) {
    section_libm_probe(results);
    section_ki_round(results);
    section_rescale(results);
    section_isqrt(results);
    section_vector2i(results);
    section_vector2l(results);
    section_vector2d(results);
    section_vector2d_norm(results);
    section_eda_angle(results);
    section_eda_angle_trig(results);
    section_rotate_exact(results);
    section_rotate_libm(results);
    section_arc_center(results);
    section_trigo_misc(results);
    section_seg_point(results);
    section_seg_seg(results);
    section_seg_angle(results);
    section_seg_line(results);
    section_box2i(results);
    section_box2i_rotated(results);
    section_box2d(results);
    section_vector_utils(results);
    section_nearest_endpoint(results);
    section_vector2_float_unsigned(results);
    section_vector2_float_norm(results);
    section_box2l(results);

    if let Some(dump) = results.dump.as_mut() {
        dump.flush().unwrap();
    }
}
