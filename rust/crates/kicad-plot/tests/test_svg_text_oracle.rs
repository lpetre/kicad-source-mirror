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

//! Differential oracle of PLOT-05 (`PLOTTER::Text` / `PlotText` and the `GERBER_PLOTTER`
//! overrides) and PLOT-06 (`SVG_PLOTTER` with the inherited `PSLIKE_PLOTTER` parts).
//!
//! [`generate`] writes [`SCRIPT_COUNT`] seeded plot scripts: SVG scripts (page sizes incl.
//! fractional mils, offsets, scales, mirroring, plot bounding boxes, precision 3..6, colour /
//! black-and-white / negative modes, dashes, every primitive and pad flash, texts) and Gerber
//! text scripts (`Text` / `PlotText` with metadata, X2 and net attributes). The C++ driver
//! `tests/oracle/svg_text_oracle.cpp` runs each one on the real `SVG_PLOTTER` /
//! `GERBER_PLOTTER` and records the FNV-1a hash of the produced file (the first files in
//! full) in `tests/golden/svg_text.txt`. [`run_script`] is the twin of `runScript()` in the
//! driver. `KICAD_PLOT_WRITE_SCRIPTS=1` writes the scripts to
//! `target/plot-oracle/svg_text_scripts.txt` (see `tests/oracle/build_svg_text_oracle.sh`); on
//! a mismatch the Rust files are written next to it.

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::color4d::Color4D;
use kicad_core::page_info::PageInfo;
use kicad_core::stroke_params::LineStyle;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::{Font, Metrics, default_font};
use kicad_geom::{
    Box2I, DEGREES_T, EdaAngle, ShapeArc, ShapeLineChain, ShapePolySet, Vector2D, Vector2I,
};
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};
use kicad_plot::{FillT, GerberPlotter, Plotter, RenderSettings, SvgPlotter};

const SCRIPT_COUNT: usize = 1500;
const FULL_COUNT: usize = 4;
const GOLDEN: &str = include_str!("golden/svg_text.txt");

const EXTRA: [&str; 3] = ["", "%TO.CFtp,1*%\n", "G04 extra*\n"];

// ---- generator ----

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi as i64 - lo as i64 + 1) as u64) as i32
    }

    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[self.below(v.len() as u32) as usize]
    }

    fn coord(&mut self) -> i32 {
        match self.below(6) {
            0 => self.range(-2000, 2000),
            1 => self.range(-20000, 20000) * 10 + 5,
            2 => self.range(-200_000, 200_000) * 1000,
            3 => self.range(-1000, 1000) * 100_005,
            _ => self.range(-200_000_000, 200_000_000),
        }
    }

    fn size(&mut self) -> i32 {
        match self.below(4) {
            0 => self.range(1, 2000),
            1 => self.range(1, 400) * 5000 + 5,
            _ => self.range(10_000, 5_000_000),
        }
    }

    fn angle(&mut self) -> String {
        match self.below(4) {
            0 => self
                .pick(&["0", "90", "180", "270", "-90", "360", "450", "-180", "-270", "540"])
                .to_string(),
            1 => self
                .pick(&["45", "30.5", "-12.3", "135", "0.05", "89.99", "200", "359.9", "-0"])
                .to_string(),
            _ => fmt_f(self.range(-7200, 7200) as f64 / 10.0),
        }
    }

    fn width(&mut self) -> i32 {
        match self.below(10) {
            0 => -1,
            1 => -2,
            2 => 0,
            3 => self.range(1, 3000),
            _ => self.range(1, 100) * 10_000,
        }
    }

    fn fill(&mut self) -> &'static str {
        self.pick(&["N", "F"])
    }
}

/// Rust's shortest round-trip text of a double (parsed identically by `strtod`).
fn fmt_f(v: f64) -> String {
    format!("{v}")
}

const TEXTS: [&str; 22] = [
    "R1",
    "Hello World",
    "~{RESET}",
    "V_{CC}",
    "x^{2}+y",
    "Line1\nLine2",
    "a\tb\tc",
    "<&>\"'",
    "\u{3a9} \u{b5} \u{fc}",
    "\u{65e5}\u{672c}",
    "",
    " ",
    "ABC\n\nDEF\n",
    "{}",
    "~~",
    "back\\slash",
    "100%",
    "~{A}_{B}^{C} D",
    "MMMMWWW",
    "il1|.,;:",
    "\r\n x",
    "${REFERENCE}",
];

const COLORS: [&str; 7] = [
    "0 0 0 1",
    "1 1 1 1",
    "1 0 0 0.5",
    "0.2 0.4 0.6 0.75",
    "0.5 0.5 0.5 1",
    "0.13 0.77 0.31 0.25",
    "0 0 1 0",
];

fn hex(s: &str) -> String {
    if s.is_empty() {
        return "~".to_string();
    }

    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn pt(g: &mut Lcg, out: &mut String) {
    out.push_str(&format!(" {} {}", g.coord(), g.coord()));
}

fn md_ref(g: &mut Lcg, mds: i32, out: &mut String) {
    if mds == 0 || g.below(4) == 0 {
        out.push_str(" -1");
    } else {
        out.push_str(&format!(" {}", g.below(mds as u32)));
    }
}

fn local_pts(g: &mut Lcg, out: &mut String, n: usize) {
    out.push_str(&format!(" {n}"));
    let c = (
        g.range(-50_000_000, 50_000_000),
        g.range(-50_000_000, 50_000_000),
    );

    for _ in 0..n {
        out.push_str(&format!(
            " {} {}",
            c.0 + g.range(-3_000_000, 3_000_000),
            c.1 + g.range(-3_000_000, 3_000_000)
        ));
    }
}

fn chain(g: &mut Lcg, out: &mut String) {
    let closed = g.below(2);
    let n = g.range(2, 7);
    out.push_str(&format!(" {closed} {n}"));
    let base = (
        g.range(-50_000_000, 50_000_000),
        g.range(-50_000_000, 50_000_000),
    );
    let mut p = || {
        (
            base.0 + g.range(-3_000_000, 3_000_000),
            base.1 + g.range(-3_000_000, 3_000_000),
        )
    };

    for _ in 0..n {
        let arc = p().0 % 3 == 0;

        if arc {
            let (s, m, e) = (p(), p(), p());
            out.push_str(&format!(
                " A {} {} {} {} {} {}",
                s.0, s.1, m.0, m.1, e.0, e.1
            ));
        } else {
            let q = p();
            out.push_str(&format!(" P {} {}", q.0, q.1));
        }
    }
}

fn polyset(g: &mut Lcg, out: &mut String) {
    let n = g.range(1, 3);
    out.push_str(&format!(" {n}"));

    for _ in 0..n {
        let cnt = g.range(3, 10) as usize;
        local_pts(g, out, cnt);
    }
}

fn text_size(g: &mut Lcg) -> (i32, i32) {
    match g.below(8) {
        0 => (0, 0),
        1 => (g.range(1, 999), g.range(1, 999)),
        2 => (123_457, 98_765),
        _ => {
            let s = g.range(1, 30) * 100_000;
            if g.below(3) == 0 {
                (s, g.range(1, 30) * 100_000 + g.range(0, 9))
            } else {
                (s, s)
            }
        }
    }
}

fn text_width(g: &mut Lcg) -> i32 {
    match g.below(9) {
        0 => 0,
        1 => -1,
        2 => -2,
        3 => 1,
        4 => -120_000,
        5 => 300_000,
        6 => 0,
        _ => g.range(1, 30) * 10_000,
    }
}

fn gen_text(g: &mut Lcg, mds: i32, plot_text: bool, gerber: bool, out: &mut String) {
    let mut l = if plot_text { "PTEXT" } else { "TEXT" }.to_string();
    pt(g, &mut l);
    l += &format!(" {}", g.pick(&COLORS));
    l += &format!(" {}", hex(TEXTS[g.below(TEXTS.len() as u32) as usize]));
    l += &format!(" {}", g.angle());
    let (mut sx, sy) = text_size(g);

    if !plot_text && g.below(3) == 0 {
        sx = -sx; // mirrored
    }

    l += &format!(" {sx} {sy}");
    l += &format!(" {} {}", g.range(-1, 1), g.range(-1, 1));
    let mut w = text_width(g);

    // GERBER_PLOTTER::SetCurrentLineWidth asserts on a negative width (debug_assert in the
    // port), PlotText selects the aperture before taking the absolute value.
    if gerber && plot_text && w < -2 {
        w = -w;
    }

    l += &format!(" {w}");

    if plot_text {
        // italic bold mirrored multiline linespacing underlined keepupright
        l += &format!(
            " {} {} {} {} {} {} {}",
            (g.below(4) == 0) as i32,
            (g.below(3) == 0) as i32,
            (g.below(3) == 0) as i32,
            (g.below(5) != 0) as i32,
            g.pick(&["1", "1.5", "0.8", "1.25"]),
            (g.below(5) == 0) as i32,
            (g.below(5) == 0) as i32
        );
    } else {
        // italic bold multiline
        l += &format!(
            " {} {} {}",
            (g.below(4) == 0) as i32,
            (g.below(3) == 0) as i32,
            g.below(2)
        );
    }

    l += &format!(" {}", g.below(2)); // explicit stroke font
    md_ref(g, mds, &mut l);
    out.push_str(&l);
    out.push('\n');
}

const SVG_OPS: [&str; 34] = [
    "W", "COLOR", "DASH", "PEN", "MV", "LN", "FIN", "SEG", "ARC", "ARC3", "TARC", "RECT", "TRECT",
    "CIRC", "TCIRC", "FCIRC", "TOVAL", "POLY", "CHAIN", "TPOLY", "BEZ", "FC", "FO", "FR", "FRR",
    "FT", "FCUS", "MARK", "SB", "EB", "TEXT", "PTEXT", "CMODE", "NEG",
];

fn gen_svg_op(g: &mut Lcg, op: &str, out: &mut String) {
    if op == "TEXT" || op == "PTEXT" {
        gen_text(g, 0, op == "PTEXT", false, out);
        return;
    }

    let mut l = op.to_string();

    match op {
        "W" => l += &format!(" {} -1", g.width()),
        "COLOR" => l += &format!(" {}", g.pick(&COLORS)),
        "DASH" => l += &format!(" {} {}", g.width(), g.range(-1, 4)),
        "PEN" => {
            l += &format!(" {}", g.pick(&["U", "D", "Z"]));
            pt(g, &mut l);
        }
        "MV" | "LN" | "FIN" => pt(g, &mut l),
        "SEG" => {
            pt(g, &mut l);
            pt(g, &mut l);
            l += &format!(" {} -1", g.width());
        }
        "ARC" | "TARC" => {
            l += &format!(
                " {} {} {} {} {}",
                fmt_f(g.coord() as f64 + g.below(4) as f64 * 0.25),
                fmt_f(g.coord() as f64 - g.below(2) as f64 * 0.5),
                g.angle(),
                g.angle(),
                fmt_f(match g.below(4) {
                    0 => g.range(-10, 200) as f64,
                    1 => g.range(1, 2000) as f64 + 0.5,
                    _ => g.range(1000, 20_000_000) as f64,
                })
            );

            if op == "ARC" {
                l += &format!(" {}", g.fill());
            }

            l += &format!(" {}", g.width());

            if op == "TARC" {
                l += " -1";
            }
        }
        "ARC3" => {
            for _ in 0..3 {
                l += &format!(" {} {}", g.coord(), g.coord());
            }
            l += &format!(" {} {}", g.fill(), g.width());
        }
        "RECT" => {
            pt(g, &mut l);
            if g.below(5) == 0 {
                let p: Vec<String> = l.split(' ').map(str::to_string).collect();
                l += &format!(" {} {}", p[1], g.coord());
            } else {
                pt(g, &mut l);
            }
            l += &format!(" {} {}", g.fill(), g.width());
        }
        "TRECT" => {
            pt(g, &mut l);
            pt(g, &mut l);
            l += &format!(" {} -1", g.width());
        }
        "CIRC" => {
            pt(g, &mut l);
            l += &format!(" {} {} {}", g.size(), g.fill(), g.width());
        }
        "TCIRC" => {
            pt(g, &mut l);
            l += &format!(" {} {} -1", g.size(), g.width());
        }
        "FCIRC" | "FC" => {
            pt(g, &mut l);
            l += &format!(" {} -1", g.size());
        }
        "TOVAL" => {
            pt(g, &mut l);
            l += &format!(" {} {} {} {}", g.size(), g.size(), g.angle(), g.width());
            l += " -1";
        }
        "POLY" => {
            l += &format!(" {} {} -1", g.fill(), g.width());
            let n = g.range(0, 7) as usize;
            local_pts(g, &mut l, n);
            if g.below(2) == 0 && n > 0 {
                // closed list: repeat the first point
                let p: Vec<String> = l.split(' ').map(str::to_string).collect();
                let cnt_idx = 4;
                let first = format!(" {} {}", p[cnt_idx + 1], p[cnt_idx + 2]);
                let mut q = p.clone();
                q[cnt_idx] = (n + 1).to_string();
                l = q.join(" ") + &first;
            }
        }
        "CHAIN" => {
            l += &format!(" {} {} -1", g.fill(), g.width());
            chain(g, &mut l);
        }
        "TPOLY" => {
            l += &format!(" {} -1", g.width());
            polyset(g, &mut l);
        }
        "BEZ" => {
            for _ in 0..4 {
                pt(g, &mut l);
            }
            l += &format!(" {} {}", g.range(0, 10000), g.width());
        }
        "FO" | "FR" => {
            pt(g, &mut l);
            l += &format!(" {} {} {} -1", g.size(), g.size(), g.angle());
        }
        "FRR" => {
            // a small table so that the polygons in the golden file stay few
            let sizes = [(1_000_000, 800_000), (2_000_000, 500_000), (600_005, 600_005)];
            let s = sizes[g.below(3) as usize];
            let r = [0, 100_000, 250_000][g.below(3) as usize];
            let pos = [(0, 0), (1_000_005, -2_000_003)][g.below(2) as usize];
            let a = g.pick(&["0", "90", "30", "-45.5"]);
            l += &format!(" {} {} {} {} {r} {a} -1", pos.0, pos.1, s.0, s.1);
        }
        "FT" => {
            pt(g, &mut l);
            let a = g.range(10_000, 2_000_000);
            let b = g.range(10_000, 2_000_000);
            let d = g.range(-500_000, 500_000);
            l += &format!(
                " {} {} {} {} {} {} {} {}",
                -a - d,
                b,
                -a + d,
                -b,
                a - d,
                -b,
                a + d,
                b
            );
            l += &format!(" {} -1", g.angle());
        }
        "FCUS" => {
            pt(g, &mut l);
            l += &format!(" {} {} {} -1", g.size(), g.size(), g.angle());
            polyset(g, &mut l);
        }
        "MARK" => {
            pt(g, &mut l);
            l += &format!(" {} {}", g.size(), g.range(0, 61));
        }
        "SB" | "EB" => {}
        "CMODE" | "NEG" => l += &format!(" {}", g.below(2)),
        _ => unreachable!("{op}"),
    }

    out.push_str(&l);
    out.push('\n');
}

fn gen_svg_script(g: &mut Lcg, idx: usize, out: &mut String) {
    out.push_str("KIND SVG\n");

    match g.below(4) {
        0 => {}
        1 => out.push_str("PAGE 0 0\n"),
        2 => out.push_str(&format!(
            "PAGE {} {}\n",
            g.range(1000, 50000),
            g.range(1000, 50000)
        )),
        _ => out.push_str(&format!(
            "PAGE {} {}\n",
            fmt_f(g.range(10000, 500000) as f64 / 10.0 + 0.05),
            fmt_f(g.range(10000, 500000) as f64 / 7.0)
        )),
    }

    let off = if g.below(3) == 0 {
        (0, 0)
    } else {
        (
            g.range(-10_000_000, 10_000_000),
            g.range(-10_000_000, 10_000_000),
        )
    };
    let ipd = g.pick(&["2540", "2540", "1", "254", "10"]);
    let scale = g.pick(&["1", "1", "0.5", "2", "1.3"]);
    let mirror = (idx % 4 == 1) as i32;
    out.push_str(&format!("VIEW {} {} {ipd} {scale} {mirror}\n", off.0, off.1));

    if g.below(2) == 0 {
        out.push_str(&format!("PREC {}\n", g.range(3, 6)));
    }

    if g.below(3) == 0 {
        out.push_str(&format!(
            "BBOX {} {} {} {}\n",
            g.coord(),
            g.coord(),
            g.range(-1000, 300_000_000),
            g.range(-1000, 300_000_000)
        ));
    }

    out.push_str(&format!("CMODE {}\n", (idx / 2 % 3 != 0) as i32));
    out.push_str(&format!("NEG {}\n", (idx % 7 == 3) as i32));
    out.push_str(&format!(
        "CREATOR {}\n",
        g.pick(&["PCBNEW", "~", "Eeschema<&>"])
    ));
    out.push_str(&format!(
        "RS {} {} {}\n",
        g.pick(&["0", "50000", "150000"]),
        g.pick(&["12", "5", "0.5"]),
        g.pick(&["3", "1", "0"])
    ));
    out.push_str("OPEN\nSTART\n");

    let op_count = if idx < FULL_COUNT { 20 } else { g.range(1, 45) };

    for k in 0..op_count {
        let op = if idx < FULL_COUNT {
            SVG_OPS[(idx * 20 + k as usize) % SVG_OPS.len()]
        } else if g.below(3) == 0 {
            g.pick(&["TEXT", "PTEXT"])
        } else {
            SVG_OPS[g.below(SVG_OPS.len() as u32) as usize]
        };
        // round rect polygons come from the golden file: keep their number (one per arc
        // error, i.e. per viewport) small
        let op = if op == "FRR" && !(ipd == "2540" && scale == "1") {
            "FR"
        } else {
            op
        };
        gen_svg_op(g, op, out);
    }
}

const NAMES: [&str; 8] = ["~", "R1", "U12", "GND", "Net-(U1-Pad3)", "A,B", "x*y", "\"q\""];

fn gen_gbr_script(g: &mut Lcg, idx: usize, out: &mut String) {
    out.push_str("KIND GBR\n");
    out.push_str(&format!(
        "RS {} 12 3\n",
        g.pick(&["0", "50000", "150000"])
    ));
    out.push_str("OPEN\n");
    let x2 = g.below(2);
    let net = g.below(3) != 0;
    let fmt = 5 + g.below(2);
    let ipd = if idx % 8 == 7 { "254" } else { "2540" };
    out.push_str(&format!(
        "CFG {x2} {} {} {fmt} 0 {} {} {ipd}\n",
        net as i32,
        g.below(2),
        g.range(-1_000_000, 1_000_000),
        g.range(-1_000_000, 1_000_000)
    ));
    out.push_str(&format!("CREATOR {}\n", g.pick(&["PCBNEW", "~"])));

    let mds = g.range(0, 4);

    for id in 0..mds {
        out.push_str(&format!(
            "MD {id} {} {} {} {} {} {}\n",
            g.pick(&["0", "1", "26", "27", "30"]),
            g.range(0, 7),
            g.below(2),
            g.pick(&NAMES),
            g.pick(&NAMES),
            if g.below(6) == 0 { g.range(1, 2) } else { 0 }
        ));
    }

    out.push_str("START\n");

    for _ in 0..g.range(1, 12) {
        match g.below(6) {
            0 => {
                let mut l = "SEG".to_string();
                pt(g, &mut l);
                pt(g, &mut l);
                l += &format!(" {}", g.width());
                md_ref(g, mds, &mut l);
                out.push_str(&l);
                out.push('\n');
            }
            1 => {
                let mut l = format!("W {}", text_width(g).max(-2));
                md_ref(g, mds, &mut l);
                out.push_str(&l);
                out.push('\n');
            }
            k => gen_text(g, mds, k % 2 == 0, true, out),
        }
    }
}

fn generate() -> String {
    let mut out = String::new();

    for idx in 0..SCRIPT_COUNT {
        let mut g = Lcg(0x7e47_0000 + idx as u64 * 104_729);
        out.push_str(&format!("BEGIN {idx}\n"));

        if idx % 5 == 4 {
            gen_gbr_script(&mut g, idx, &mut out);
        } else {
            gen_svg_script(&mut g, idx, &mut out);
        }

        out.push_str("END\n");
    }

    out
}

// ---- interpreter (twin of runScript() in svg_text_oracle.cpp) ----

struct Tok<'a> {
    t: Vec<&'a str>,
    i: usize,
}

impl<'a> Tok<'a> {
    fn s(&mut self) -> &'a str {
        let v = self.t[self.i];
        self.i += 1;
        v
    }
    fn n(&mut self) -> i32 {
        self.s().parse::<i64>().unwrap() as i32
    }
    fn b(&mut self) -> bool {
        self.n() != 0
    }
    fn d(&mut self) -> f64 {
        self.s().parse::<f64>().unwrap()
    }
    fn str(&mut self) -> String {
        let v = self.s();
        if v == "~" {
            String::new()
        } else {
            v.to_string()
        }
    }
    fn hex(&mut self) -> String {
        let v = self.s();
        if v == "~" {
            return String::new();
        }
        let bytes: Vec<u8> = (0..v.len() / 2)
            .map(|k| u8::from_str_radix(&v[2 * k..2 * k + 2], 16).unwrap())
            .collect();
        String::from_utf8(bytes).unwrap()
    }
    fn pt(&mut self) -> Vector2I {
        let x = self.n();
        let y = self.n();
        Vector2I::new(x, y)
    }
    fn ptd(&mut self) -> Vector2D {
        let x = self.d();
        let y = self.d();
        Vector2D::new(x, y)
    }
    fn ang(&mut self) -> EdaAngle {
        EdaAngle::new(self.d(), DEGREES_T)
    }
    fn fill(&mut self) -> FillT {
        if self.s() == "F" {
            FillT::FilledShape
        } else {
            FillT::NoFill
        }
    }
    fn color(&mut self) -> Color4D {
        let r = self.d();
        let g = self.d();
        let b = self.d();
        let a = self.d();
        Color4D::new(r, g, b, a)
    }
    fn halign(&mut self) -> GrTextHAlignT {
        match self.n() {
            -1 => GrTextHAlignT::Left,
            0 => GrTextHAlignT::Center,
            1 => GrTextHAlignT::Right,
            _ => GrTextHAlignT::Indeterminate,
        }
    }
    fn valign(&mut self) -> GrTextVAlignT {
        match self.n() {
            -1 => GrTextVAlignT::Top,
            0 => GrTextVAlignT::Center,
            1 => GrTextVAlignT::Bottom,
            _ => GrTextVAlignT::Indeterminate,
        }
    }
    fn pts(&mut self) -> Vec<Vector2I> {
        let cnt = self.n();
        (0..cnt).map(|_| self.pt()).collect()
    }
    fn chain(&mut self) -> ShapeLineChain {
        let closed = self.n() != 0;
        let cnt = self.n();
        let mut c = ShapeLineChain::new();

        for _ in 0..cnt {
            if self.s() == "P" {
                let p = self.pt();
                c.append_xy(p.x, p.y, true);
            } else {
                let a = self.pt();
                let b = self.pt();
                let e = self.pt();
                c.append_arc(&ShapeArc::from_start_mid_end(a, b, e, 0));
            }
        }

        c.set_closed(closed);
        c
    }
    fn polyset(&mut self) -> ShapePolySet {
        let cnt = self.n();
        let mut ps = ShapePolySet::new();

        for _ in 0..cnt {
            let pts = self.pts();
            ps.add_outline(&ShapeLineChain::from_points(&pts, true));
        }

        ps
    }
}

fn line_style(v: i32) -> LineStyle {
    match v {
        0 => LineStyle::Solid,
        1 => LineStyle::Dash,
        2 => LineStyle::Dot,
        3 => LineStyle::DashDot,
        4 => LineStyle::DashDotDot,
        _ => LineStyle::Default,
    }
}

fn chamfer_key(
    pos: Vector2I,
    size: Vector2I,
    rot: EdaAngle,
    radius: i32,
    ratio: f64,
    positions: i32,
    err: i32,
) -> String {
    format!(
        "{},{},{},{},{:016x},{},{:016x},{},{}",
        pos.x,
        pos.y,
        size.x,
        size.y,
        rot.as_degrees().to_bits(),
        radius,
        ratio.to_bits(),
        positions,
        err
    )
}

enum Engine {
    Svg(Box<SvgPlotter>),
    Gbr(Box<GerberPlotter>),
}

impl Engine {
    fn p(&mut self) -> &mut dyn Plotter {
        match self {
            Engine::Svg(p) => p.as_mut(),
            Engine::Gbr(p) => p.as_mut(),
        }
    }
}

fn run_script(lines: &[&str], chamfer: &Arc<BTreeMap<String, Vec<Vector2I>>>) -> String {
    let is_svg = lines[0] == "KIND SVG";
    let mut eng = if is_svg {
        let mut p = SvgPlotter::new();
        p.set_creation_date("@DATE@");
        let map = Arc::clone(chamfer);
        p.set_round_rect_polygon_fn(Some(Arc::new(
            move |pos: Vector2I,
                  size: Vector2I,
                  rot: EdaAngle,
                  radius: i32,
                  ratio: f64,
                  positions: i32,
                  err: i32|
                  -> ShapePolySet {
                let key = chamfer_key(pos, size, rot, radius, ratio, positions, err);
                let pts = map
                    .get(&key)
                    .unwrap_or_else(|| panic!("round rect polygon {key} missing in the golden"));
                let mut ps = ShapePolySet::new();

                // the C++ outline keeps a closing point equal to the first one (CornerList-
                // ToPolygon appends without merging): no SetClosed( true ) merge here
                if !pts.is_empty() {
                    ps.new_outline();

                    for q in pts {
                        ps.append(q.x, q.y, -1, -1, true);
                    }
                }

                ps
            },
        )));
        Engine::Svg(Box::new(p))
    } else {
        let mut p = GerberPlotter::new();
        p.set_build_version("@VERSION@");
        p.set_creation_date("@DATE@");
        Engine::Gbr(Box::new(p))
    };
    let mut mds: BTreeMap<i32, GbrMetadata> = BTreeMap::new();
    let mut started = false;
    let stroke_font: &dyn Font = default_font();
    let metrics = Metrics::default();

    eng.p().set_render_settings(Some(RenderSettings::default()));

    for line in &lines[1..] {
        let mut t = Tok {
            t: line.split(' ').filter(|s| !s.is_empty()).collect(),
            i: 1,
        };
        let op = t.t[0];

        macro_rules! md {
            () => {{
                let id = t.n();
                if id < 0 {
                    None
                } else {
                    Some(mds.entry(id).or_default().clone())
                }
            }};
        }

        let p = eng.p();

        match op {
            "PAGE" => {
                let mut pi = PageInfo::default();
                let w = t.d();
                let h = t.d();

                if w > 0.0 {
                    pi.set_width_mils(w);
                    pi.set_height_mils(h);
                }

                p.set_page_settings(&pi);
            }
            "VIEW" => {
                let off = t.pt();
                let ipd = t.d();
                let scale = t.d();
                let mirror = t.b();
                p.set_viewport(off, ipd, scale, mirror);
            }
            "PREC" => {
                let n = t.n();
                p.set_svg_coordinates_format(n as u32);
            }
            "BBOX" => {
                let a = t.pt();
                let s = t.pt();
                p.set_plot_bbox(&Box2I::new(a, s));
            }
            "CMODE" => {
                let v = t.b();
                p.set_color_mode(v);
            }
            "NEG" => {
                let v = t.b();
                p.set_negative(v);
            }
            "OPEN" => {
                p.open_file(if is_svg { "/x/plot.svg" } else { "plot.gbr" });
            }
            "CFG" => {
                let x2 = t.b();
                let net = t.b();
                let nomacro = t.b();
                let fmt = t.n();
                let inch = t.b();
                let off = t.pt();
                let ipd = t.d();
                let Engine::Gbr(gp) = &mut eng else {
                    unreachable!()
                };
                gp.set_viewport(off, ipd, 1.0, false);
                gp.set_gerber_coordinates_format(fmt, inch);
                gp.use_x2format(x2);
                gp.use_x2_net_attributes(net);
                gp.disable_apert_macros(nomacro);
            }
            "CREATOR" => {
                let c = t.str();
                p.set_creator(&c);
            }
            "RS" => {
                let w = t.n();
                let dash = t.d();
                let gap = t.d();
                let rs = p.state_mut().render_settings.as_mut().unwrap();
                rs.set_default_pen_width(w);
                rs.set_dash_length_ratio(dash);
                rs.set_gap_length_ratio(gap);
            }
            "MD" => {
                let id = t.n();
                let mut m = GbrMetadata::new();
                m.aperture_metadata.apert_attribute = GbrApertureAttrib::from_i32(t.n()).unwrap();
                m.netlist_metadata.net_attrib_type = t.n();
                m.netlist_metadata.not_in_net = t.b();
                m.set_cmp_reference(&t.str());
                m.set_net_name(&t.str());
                m.netlist_metadata.extra_data = EXTRA[t.n() as usize].to_string();
                mds.insert(id, m);
            }
            "START" => {
                p.start_plot("1");
                started = true;
            }
            "COLOR" => {
                let c = t.color();
                p.set_color(&c);
            }
            "DASH" => {
                let w = t.n();
                let s = line_style(t.n());
                p.set_dash(w, s);
            }
            "W" => {
                let w = t.n();
                let m = md!();
                eng.p().set_current_line_width(w, m.as_ref());
            }
            "PEN" => {
                let c = t.s().chars().next().unwrap();
                let q = t.pt();
                p.pen_to(q, c);
            }
            "MV" => {
                let q = t.pt();
                p.move_to(q);
            }
            "LN" => {
                let q = t.pt();
                p.line_to(q);
            }
            "FIN" => {
                let q = t.pt();
                p.finish_to(q);
            }
            "SEG" => {
                let a = t.pt();
                let b = t.pt();
                let w = t.n();
                let m = md!();
                eng.p().thick_segment(a, b, w, m.as_ref());
            }
            "ARC" => {
                let c = t.ptd();
                let sa = t.ang();
                let a = t.ang();
                let r = t.d();
                let f = t.fill();
                let w = t.n();
                p.arc(c, sa, a, r, f, w);
            }
            "ARC3" => {
                let s = t.ptd();
                let m = t.ptd();
                let e = t.ptd();
                let f = t.fill();
                let w = t.n();
                p.arc_3pt(s, m, e, f, w);
            }
            "TARC" => {
                let c = t.ptd();
                let sa = t.ang();
                let a = t.ang();
                let r = t.d();
                let w = t.n();
                let m = md!();
                eng.p().thick_arc(c, sa, a, r, w, m.as_ref());
            }
            "RECT" => {
                let a = t.pt();
                let b = t.pt();
                let f = t.fill();
                let w = t.n();
                p.rect(a, b, f, w, 0);
            }
            "TRECT" => {
                let a = t.pt();
                let b = t.pt();
                let w = t.n();
                let m = md!();
                eng.p().thick_rect(a, b, w, m.as_ref());
            }
            "CIRC" => {
                let c = t.pt();
                let d = t.n();
                let f = t.fill();
                let w = t.n();
                p.circle(c, d, f, w);
            }
            "TCIRC" => {
                let c = t.pt();
                let d = t.n();
                let w = t.n();
                let m = md!();
                eng.p().thick_circle(c, d, w, m.as_ref());
            }
            "FCIRC" => {
                let c = t.pt();
                let d = t.n();
                let m = md!();
                eng.p().filled_circle(c, d, m.as_ref());
            }
            "TOVAL" => {
                let q = t.pt();
                let s = t.pt();
                let o = t.ang();
                let w = t.n();
                let m = md!();
                eng.p().thick_oval(q, s, o, w, m.as_ref());
            }
            "POLY" => {
                let f = t.fill();
                let w = t.n();
                let m = md!();
                let v = t.pts();
                eng.p().plot_poly(&v, f, w, m.as_ref());
            }
            "CHAIN" => {
                let f = t.fill();
                let w = t.n();
                let m = md!();
                let c = t.chain();
                eng.p().plot_poly_chain(&c, f, w, m.as_ref());
            }
            "TPOLY" => {
                let w = t.n();
                let m = md!();
                let ps = t.polyset();
                eng.p().thick_poly(&ps, w, m.as_ref());
            }
            "BEZ" => {
                let a = t.pt();
                let b = t.pt();
                let c = t.pt();
                let d = t.pt();
                let tol = t.n();
                let w = t.n();
                p.bezier_curve(a, b, c, d, tol, w);
            }
            "FC" => {
                let q = t.pt();
                let d = t.n();
                let m = md!();
                eng.p().flash_pad_circle(q, d, m.as_ref());
            }
            "FO" | "FR" => {
                let q = t.pt();
                let s = t.pt();
                let o = t.ang();
                let m = md!();

                if op == "FO" {
                    eng.p().flash_pad_oval(q, s, o, m.as_ref());
                } else {
                    eng.p().flash_pad_rect(q, s, o, m.as_ref());
                }
            }
            "FRR" => {
                let q = t.pt();
                let s = t.pt();
                let r = t.n();
                let o = t.ang();
                let m = md!();
                eng.p().flash_pad_round_rect(q, s, r, o, m.as_ref());
            }
            "FT" => {
                let q = t.pt();
                let c = [t.pt(), t.pt(), t.pt(), t.pt()];
                let o = t.ang();
                let m = md!();
                eng.p().flash_pad_trapez(q, &c, o, m.as_ref());
            }
            "FCUS" => {
                let q = t.pt();
                let s = t.pt();
                let o = t.ang();
                let m = md!();
                let ps = t.polyset();
                eng.p().flash_pad_custom(q, s, o, &ps, m.as_ref());
            }
            "MARK" => {
                let q = t.pt();
                let d = t.n();
                let id = t.n();
                p.marker(q, d, id as u32);
            }
            "SB" => p.start_block(None),
            "EB" => p.end_block(None),
            "TEXT" => {
                let q = t.pt();
                let c = t.color();
                let txt = t.hex();
                let o = t.ang();
                let s = t.pt();
                let h = t.halign();
                let v = t.valign();
                let w = t.n();
                let it = t.b();
                let bo = t.b();
                let ml = t.b();
                let font = t.b().then_some(stroke_font);
                let m = md!();
                eng.p().text(
                    q,
                    &c,
                    &txt,
                    o,
                    s,
                    h,
                    v,
                    w,
                    it,
                    bo,
                    ml,
                    font,
                    &metrics,
                    m.as_ref(),
                );
            }
            "PTEXT" => {
                let q = t.pt();
                let c = t.color();
                let txt = t.hex();
                let mut a = TextAttributes::default();
                a.set_angle(t.ang().as_degrees());
                let s = t.pt();
                a.size = (s.x, s.y);
                a.halign = t.halign();
                a.valign = t.valign();
                a.stroke_width = t.n();
                a.italic = t.b();
                a.bold = t.b();
                a.mirrored = t.b();
                a.multiline = t.b();
                a.line_spacing = t.d();
                a.underlined = t.b();
                a.keep_upright = t.b();
                let font = t.b().then_some(stroke_font);
                let m = md!();
                eng.p()
                    .plot_text(q, &c, &txt, &a, font, &metrics, m.as_ref());
            }
            _ => panic!("unknown op {op}"),
        }
    }

    if started {
        eng.p().end_plot();
    }

    eng.p().output().to_string()
}

fn fnv(s: &[u8]) -> u64 {
    let mut h: u64 = 1469598103934665603;

    for &c in s {
        h ^= c as u64;
        h = h.wrapping_mul(1099511628211);
    }

    h
}

fn split_scripts(all: &str) -> Vec<Vec<&str>> {
    let mut scripts: Vec<Vec<&str>> = Vec::new();

    for line in all.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if line.starts_with("BEGIN") {
            scripts.push(Vec::new());
        } else if line != "END" {
            scripts.last_mut().unwrap().push(line);
        }
    }

    scripts
}

struct Golden {
    scripts: (usize, u64),
    chamfer: BTreeMap<String, Vec<Vector2I>>,
    full: BTreeMap<usize, String>,
    hashes: BTreeMap<usize, u64>,
}

fn parse_golden() -> Golden {
    let mut g = Golden {
        scripts: (0, 0),
        chamfer: BTreeMap::new(),
        full: BTreeMap::new(),
        hashes: BTreeMap::new(),
    };
    let mut lines = GOLDEN.split_inclusive('\n');

    while let Some(line) = lines.next() {
        let l = line.trim_end_matches('\n');
        let f: Vec<&str> = l.split(' ').collect();

        match f[0] {
            "SCRIPTS" => {
                g.scripts = (
                    f[1].parse().unwrap(),
                    u64::from_str_radix(f[2], 16).unwrap(),
                );
            }
            "CH" => {
                let n: usize = f[2].parse().unwrap();
                let pts = (0..n)
                    .map(|k| {
                        Vector2I::new(f[3 + 2 * k].parse().unwrap(), f[4 + 2 * k].parse().unwrap())
                    })
                    .collect();
                g.chamfer.insert(f[1].to_string(), pts);
            }
            "F" => {
                let idx: usize = f[1].parse().unwrap();
                let mut text = String::new();

                for body in lines.by_ref() {
                    if body == "F-END\n" {
                        break;
                    }

                    // the file may not end with a new line (SVG text groups end with "</g>")
                    if let Some(rest) = body.strip_suffix("F-END\n") {
                        text.push_str(rest);
                        break;
                    }

                    text.push_str(body);
                }

                g.full.insert(idx, text);
            }
            "R" => {
                g.hashes.insert(
                    f[1].parse().unwrap(),
                    u64::from_str_radix(f[2], 16).unwrap(),
                );
            }
            _ => panic!("bad golden line {l}"),
        }
    }

    g
}

#[test]
fn svg_and_text_match_oracle() {
    let all = generate();
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plot-oracle");

    if std::env::var_os("KICAD_PLOT_WRITE_SCRIPTS").is_some() {
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("svg_text_scripts.txt"), &all).unwrap();
    }

    let golden = parse_golden();
    let scripts = split_scripts(&all);

    assert_eq!(
        golden.scripts,
        (scripts.len(), fnv(all.as_bytes())),
        "the script generator changed: regenerate the golden file \
         (tests/oracle/build_svg_text_oracle.sh \"\" regen)"
    );

    let chamfer = Arc::new(golden.chamfer);
    let mut mismatches = Vec::new();

    for (idx, script) in scripts.iter().enumerate() {
        if std::env::var_os("KICAD_PLOT_TRACE").is_some() {
            eprintln!("script {idx}");
        }

        let out = run_script(script, &chamfer);

        let ok = match golden.full.get(&idx) {
            Some(text) => *text == out,
            None => golden.hashes.get(&idx) == Some(&(fnv(out.as_bytes()) & 0xffff_ffff)),
        };

        if !ok {
            if mismatches.len() < 20 {
                let _ = std::fs::create_dir_all(&out_dir);
                let _ = std::fs::write(out_dir.join(format!("rust_svgtext_{idx}.txt")), &out);
            }

            mismatches.push(idx);
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} scripts differ from the oracle, first: {:?} (Rust files in target/plot-oracle)",
        mismatches.len(),
        scripts.len(),
        &mismatches[..mismatches.len().min(20)]
    );
}
