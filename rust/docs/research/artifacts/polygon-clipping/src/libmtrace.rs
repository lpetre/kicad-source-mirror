// Deterministic trace of libm results (bit patterns) to compare native (glibc) with
// wasm32-unknown-unknown (Rust's built-in libm port).  Scratch research code.
use std::f64::consts::PI;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Section names and the values of each section.
pub fn trace() -> Vec<(&'static str, Vec<u64>)> {
    let mut out: Vec<(&'static str, Vec<u64>)> = Vec::new();
    let mut r = Rng(12345);

    // (a) the exact chain KiCad + ClipperOffset use for round joins:
    //     coeff = 1 - cos(pi/n); arc_tol = |amount| * coeff;
    //     steps_per_360 = min(pi / acos(1 - arc_tol/abs_delta), abs_delta*pi); step_sin/cos;
    //     steps = ceil(steps_per_rad * |angle|) for the usual corner angles.
    let mut coeff = Vec::new();
    let mut acosv = Vec::new();
    let mut sinv = Vec::new();
    let mut cosv = Vec::new();
    let mut steps = Vec::new();
    let amounts = [1.0, 10.0, 100.0, 2000.0, 25_000.0, 50_000.0, 100_000.0, 127_000.0, 200_000.0, 254_000.0,
        500_000.0, 1_000_000.0, 2_540_000.0, 12_700_000.0, 333_333.0, 99_999.0, 1_234_567.0];
    for n in 6..=1024u32 {
        let c = 1.0 - (PI / n as f64).cos();
        coeff.push(c.to_bits());
        for &a in &amounts {
            let arc_tol: f64 = a * c;
            let abs_delta: f64 = a;
            let ac = (1.0 - arc_tol / abs_delta).acos();
            acosv.push(ac.to_bits());
            let steps_per_360 = (PI / ac).min(abs_delta * PI);
            let s = (2.0 * PI / steps_per_360).sin();
            let co = (2.0 * PI / steps_per_360).cos();
            sinv.push(s.to_bits());
            cosv.push(co.to_bits());
            let steps_per_rad = steps_per_360 / (2.0 * PI);
            for (sa, ca) in [(1.0f64, 0.0f64), (-1.0, 0.0), (0.0, -1.0), (0.5f64.sqrt(), 0.5f64.sqrt()),
                (0.5f64.sqrt(), -(0.5f64.sqrt())), (-(0.5f64.sqrt()), 0.5f64.sqrt())]
            {
                let angle = sa.atan2(ca);
                steps.push(((steps_per_rad * angle.abs()).ceil() as i64) as u64);
            }
            steps.push(((steps_per_rad * 2.0 * PI).ceil() as i64) as u64);
        }
    }
    out.push(("kicad_coeff_1-cos(pi/n)", coeff));
    out.push(("clipper_acos(1-arcTol/delta)", acosv));
    out.push(("clipper_step_sin", sinv));
    out.push(("clipper_step_cos", cosv));
    out.push(("clipper_round_join_step_count", steps));

    // (b) atan2 of cross/dot of unit normals of random integer edges (DoRound argument)
    let mut at = Vec::new();
    for _ in 0..400_000 {
        let v = |r: &mut Rng| -> (f64, f64) {
            let dx = (r.next() % 4_000_001) as f64 - 2_000_000.0;
            let dy = (r.next() % 4_000_001) as f64 - 2_000_000.0;
            let h = (dx * dx + dy * dy).sqrt();
            if h == 0.0 { (0.0, 0.0) } else { (dy / h, -dx / h) }
        };
        let (n1, n2) = (v(&mut r), v(&mut r));
        let sin_a = n1.0 * n2.1 - n2.0 * n1.1;
        let cos_a = n1.0 * n2.0 + n1.1 * n2.1;
        at.push(sin_a.atan2(cos_a).to_bits());
    }
    out.push(("atan2(cross,dot) of unit normals", at));

    // (c) generic arguments
    macro_rules! gen1 {
        ($name:expr, $n:expr, $lo:expr, $hi:expr, $f:expr) => {{
            let mut v = Vec::with_capacity($n);
            for _ in 0..$n {
                let x: f64 = r.range($lo, $hi);
                let y: f64 = $f(x);
                v.push(y.to_bits());
            }
            out.push(($name, v));
        }};
    }
    gen1!("sin[-2pi,2pi]", 300_000, -2.0 * PI, 2.0 * PI, |x: f64| x.sin());
    gen1!("cos[-2pi,2pi]", 300_000, -2.0 * PI, 2.0 * PI, |x: f64| x.cos());
    gen1!("sin[0,1e6]", 100_000, 0.0, 1.0e6, |x: f64| x.sin());
    gen1!("cos[0,1e6]", 100_000, 0.0, 1.0e6, |x: f64| x.cos());
    gen1!("tan[-1.5,1.5]", 200_000, -1.5, 1.5, |x: f64| x.tan());
    gen1!("acos[-1,1]", 300_000, -1.0, 1.0, |x: f64| x.acos());
    gen1!("acos[0.999,1]", 300_000, 0.999, 1.0, |x: f64| x.acos());
    gen1!("asin[-1,1]", 200_000, -1.0, 1.0, |x: f64| x.asin());
    gen1!("atan[-1e3,1e3]", 200_000, -1.0e3, 1.0e3, |x: f64| x.atan());
    gen1!("exp[-20,20]", 200_000, -20.0, 20.0, |x: f64| x.exp());
    gen1!("ln[1e-9,1e9]", 200_000, 1.0e-9, 1.0e9, |x: f64| x.ln());
    gen1!("log10[1e-9,1e9]", 200_000, 1.0e-9, 1.0e9, |x: f64| x.log10());
    gen1!("sqrt[0,1e18]", 200_000, 0.0, 1.0e18, |x: f64| x.sqrt());
    gen1!("cbrt[0,1e18]", 100_000, 0.0, 1.0e18, |x: f64| x.cbrt());
    gen1!("round[-1e6,1e6]", 100_000, -1.0e6, 1.0e6, |x: f64| x.round());
    let mut v = Vec::new();
    for _ in 0..300_000 {
        let (y, x) = (r.range(-1.0e7, 1.0e7), r.range(-1.0e7, 1.0e7));
        v.push(y.atan2(x).to_bits());
    }
    out.push(("atan2 random", v));
    let mut v = Vec::new();
    for _ in 0..200_000 {
        let (y, x) = (r.range(-2.0e9, 2.0e9), r.range(-2.0e9, 2.0e9));
        v.push(y.hypot(x).to_bits());
    }
    out.push(("hypot random", v));
    let mut v = Vec::new();
    for _ in 0..200_000 {
        let (y, x) = (r.range(0.0, 100.0), r.range(-5.0, 5.0));
        v.push(y.powf(x).to_bits());
    }
    out.push(("powf random", v));
    let mut v = Vec::new();
    for _ in 0..100_000 {
        let (y, x) = (r.range(-1.0e7, 1.0e7), r.range(0.001, 360.0));
        v.push((y % x).to_bits());
    }
    out.push(("fmod random", v));
    out
}

pub fn trace_text() -> String {
    let mut s = String::new();
    for (name, vals) in trace() {
        s.push_str(&format!("# {}\n", name));
        for v in vals {
            s.push_str(&format!("{:016x}\n", v));
        }
    }
    s
}
