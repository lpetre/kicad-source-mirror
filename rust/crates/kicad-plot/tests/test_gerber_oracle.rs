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

//! Differential test of work packages PLOT-03 and PLOT-04 (`GERBER_PLOTTER`) against the C++
//! implementation.
//!
//! [`generate`] writes [`SCRIPT_COUNT`] pseudo-random plot scripts (every primitive, every pad
//! flash with cardinal and non-cardinal orientations, with and without aperture macros, X2
//! on/off, net attributes on/off, formats 4.5/4.6, mm/inch, offsets, attribute changes). The
//! C++ driver `tests/oracle/gerber_plotter_oracle.cpp` runs each script on the real
//! `GERBER_PLOTTER` and records the FNV-1a hash of the produced file (and the full text of
//! the first scripts) in `tests/golden/gerber_plotter.txt`. [`run_script`] is the line by line
//! twin of `runScript()` in the C++ driver. `KICAD_PLOT_WRITE_SCRIPTS=1` writes the scripts
//! to `target/plot-oracle/gerber_scripts.txt` for the driver (see
//! `tests/oracle/build_gerber_plotter_oracle.sh`); on a mismatch the Rust files are written
//! next to it.

use std::collections::BTreeMap;

use kicad_geom::{DEGREES_T, EdaAngle, ShapeArc, ShapeLineChain, ShapePolySet, Vector2D, Vector2I};
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};
use kicad_plot::{FillT, GerberPlotter, Plotter, RenderSettings};

const SCRIPT_COUNT: usize = 2400;
const FULL_COUNT: usize = 6;
const GOLDEN: &str = include_str!("golden/gerber_plotter.txt");

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

    /// A coordinate: small, large, or ending in 5 nm (4.5 format half cases).
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
                .pick(&[
                    "0", "90", "180", "270", "-90", "360", "450", "-180", "-270", "540",
                ])
                .to_string(),
            1 => self
                .pick(&[
                    "45", "30.5", "-12.3", "135", "0.05", "89.99", "200", "359.9", "-0",
                ])
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
}

/// Rust's shortest round-trip text of a double (parsed identically by `strtod`).
fn fmt_f(v: f64) -> String {
    format!("{v}")
}

const STRINGS: [&str; 12] = [
    "~",
    "R1",
    "U12",
    "GND",
    "Net-(U1-Pad3)",
    "A,B",
    "x*y",
    "50%",
    "back\\slash",
    "\"quoted\"",
    "\u{3a9}1",
    "N/C",
];

fn gen_pt(g: &mut Lcg, out: &mut String) {
    out.push_str(&format!(" {} {}", g.coord(), g.coord()));
}

fn gen_pts(g: &mut Lcg, out: &mut String, n: usize) {
    out.push_str(&format!(" {n}"));

    for _ in 0..n {
        gen_pt(g, out);
    }
}

fn gen_local_pts(g: &mut Lcg, out: &mut String, n: usize, c: (i32, i32)) {
    out.push_str(&format!(" {n}"));
    let r = g.range(10_000, 3_000_000);

    for k in 0..n {
        // a star-ish polygon around c
        let a = k as f64 / n as f64 * std::f64::consts::TAU;
        let rr = r / 2 + g.range(0, r / 2);
        let x = c.0 as i64 + (rr as f64 * a.cos()) as i64;
        let y = c.1 as i64 + (rr as f64 * a.sin()) as i64;
        out.push_str(&format!(" {} {}", x, y));
    }
}

fn gen_chain(g: &mut Lcg, out: &mut String) {
    let closed = g.below(2);
    let n = g.range(2, 7);
    out.push_str(&format!(" {closed} {n}"));
    let base = (
        g.range(-50_000_000, 50_000_000),
        g.range(-50_000_000, 50_000_000),
    );

    for k in 0..n {
        if g.below(3) == 0 {
            // arc start, mid, end around a centre
            let r = g.range(200, 5_000_000) as f64;
            let a0 = g.range(0, 359) as f64;
            let sweep = g.range(-300, 300) as f64;
            let cx = base.0 as f64 + k as f64 * 1_000_000.0;
            let cy = base.1 as f64;
            let p = |a: f64| {
                let ar = a.to_radians();
                ((cx + r * ar.cos()) as i64, (cy + r * ar.sin()) as i64)
            };
            let s = p(a0);
            let m = p(a0 + sweep / 2.0);
            let e = p(a0 + sweep);
            out.push_str(&format!(
                " A {} {} {} {} {} {}",
                s.0, s.1, m.0, m.1, e.0, e.1
            ));
        } else {
            out.push_str(&format!(
                " P {} {}",
                base.0 + g.range(-3_000_000, 3_000_000),
                base.1 + g.range(-3_000_000, 3_000_000)
            ));
        }
    }
}

fn gen_polyset(g: &mut Lcg, out: &mut String, c: (i32, i32)) {
    let n = g.range(1, 3);
    out.push_str(&format!(" {n}"));

    for _ in 0..n {
        let cnt = g.range(3, 12) as usize;
        gen_local_pts(g, out, cnt, c);
    }
}

/// The fixed table of chamfered pads (their polygons come from the golden file).
fn chamfer_entry(k: u32) -> String {
    let sizes = [
        (1_000_000, 800_000),
        (2_000_000, 500_000),
        (600_000, 600_000),
        (1_500_005, 1_200_005),
    ];
    let radii = [0, 100_000, 50_000];
    let ratios = ["0.25", "0.1", "0.5", "0.333"];
    let positions = [1, 5, 15, 0, 6, 9, 3, 7, 14];
    let angles = ["0", "90", "30", "-45.5", "180"];
    let s = sizes[(k % 4) as usize];
    let pos = [(0, 0), (1_000_005, -2_000_003), (-30_000_000, 40_000_000)][(k % 3) as usize];

    format!(
        "{} {} {} {} {} {} {} {}",
        pos.0,
        pos.1,
        s.0,
        s.1,
        radii[(k / 3 % 3) as usize],
        ratios[(k / 2 % 4) as usize],
        positions[(k / 5 % 9) as usize],
        angles[(k / 7 % 5) as usize]
    )
}

const OPS: [&str; 33] = [
    "W", "PEN", "MV", "LN", "FIN", "SEG", "ARC", "ARC3", "TARC", "RECT", "TRECT", "CIRC", "TCIRC",
    "FCIRC", "TOVAL", "POLY", "CHAIN", "REGV", "REGC", "ASREG", "TPOLY", "FC", "FO", "FR", "FRR",
    "FT", "FCUS", "FCH", "FPOLY", "POL", "SB", "EB", "CLR",
];

fn gen_md_ref(g: &mut Lcg, mds: i32, out: &mut String) {
    if mds == 0 || g.below(4) == 0 {
        out.push_str(" -1");
    } else {
        out.push_str(&format!(" {}", g.below(mds as u32)));
    }
}

fn gen_script(idx: usize, out: &mut String) {
    let mut g = Lcg(0x5eed_0000 + idx as u64 * 7919);
    let i = idx;
    let x2 = i.is_multiple_of(2) as i32;
    let net = !(i / 2).is_multiple_of(3) as i32;
    let nomacro = ((i / 6) % 2) as i32;
    let inch = ((i / 24) % 4 == 3) as i32;
    let fmt = if inch != 0 {
        6 + (i / 12) % 2
    } else {
        5 + (i / 12) % 2
    };
    let ipd = if i % 8 == 7 { "254" } else { "2540" };
    let off = if g.below(3) == 0 {
        (0, 0)
    } else {
        (
            g.range(-10_000_000, 10_000_000),
            g.range(-10_000_000, 10_000_000),
        )
    };

    out.push_str(&format!("BEGIN {idx}\n"));
    out.push_str(&format!(
        "CFG {x2} {net} {nomacro} {fmt} {inch} {} {} {ipd}\n",
        off.0, off.1
    ));
    out.push_str(&format!(
        "CREATOR {}\n",
        g.pick(&["PCBNEW", "~", "GERBVIEW"])
    ));

    for _ in 0..g.below(3) {
        let h = g.pick(&[
            "%TF.FileFunction,Copper,L1,Top*%",
            "%TF.FilePolarity,Positive*%",
            "~",
            "G04 APERTURE LIST*",
            "G04 #@! TF.SameCoordinates,Original*",
        ]);
        out.push_str(&format!("HDR {h}\n"));
    }

    out.push_str(&format!("RS {}\n", g.pick(&["0", "50000", "150000"])));

    let mds = g.range(0, 5);

    for id in 0..mds {
        let attrib = if g.below(3) == 0 { 0 } else { g.range(0, 32) };
        out.push_str(&format!(
            "MD {id} {attrib} {} {} {} {} {} {} {} {} {} {}\n",
            // ASCII only: see PORT_STATUS.md (C++ drops non-ASCII custom attributes in a C
            // locale)
            if g.below(4) == 0 {
                g.pick(&STRINGS[..10])
            } else {
                "~"
            },
            g.range(0, 7),
            g.below(2),
            g.pick(&STRINGS),
            g.below(4),
            g.pick(&STRINGS),
            g.below(4),
            g.pick(&STRINGS),
            g.pick(&STRINGS),
            if g.below(6) == 0 { g.range(1, 2) } else { 0 }
        ));
    }

    out.push_str("START\n");

    let op_count = if idx < FULL_COUNT { 12 } else { g.range(1, 40) };

    for k in 0..op_count {
        // the first scripts walk through every op kind once
        let op = if idx < FULL_COUNT {
            OPS[(idx * 12 + k as usize) % OPS.len()]
        } else {
            OPS[g.below(OPS.len() as u32) as usize]
        };
        let mut l = op.to_string();

        match op {
            "W" => {
                l += &format!(" {}", g.width());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "PEN" => {
                l += &format!(" {}", g.pick(&["U", "D", "Z"]));
                gen_pt(&mut g, &mut l);
            }
            "MV" | "LN" | "FIN" => gen_pt(&mut g, &mut l),
            "SEG" => {
                gen_pt(&mut g, &mut l);

                if g.below(4) == 0 {
                    let p = l.clone();
                    let pts: Vec<&str> = p.split(' ').collect();
                    l += &format!(" {} {}", pts[1], pts[2]);
                } else {
                    gen_pt(&mut g, &mut l);
                }

                l += &format!(" {}", g.width());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "ARC" | "TARC" => {
                l += &format!(
                    " {} {} {} {} {}",
                    fmt_f(g.coord() as f64 + g.below(4) as f64 * 0.25),
                    fmt_f(g.coord() as f64 - g.below(2) as f64 * 0.5),
                    g.angle(),
                    g.angle(),
                    fmt_f(match g.below(4) {
                        0 => g.range(0, 200) as f64,
                        1 => g.range(1, 2000) as f64 + 0.5,
                        _ => g.range(1000, 20_000_000) as f64,
                    })
                );

                if op == "ARC" {
                    l += &format!(" {}", g.pick(&["N", "F"]));
                }

                l += &format!(" {}", g.width());

                if op == "TARC" {
                    gen_md_ref(&mut g, mds, &mut l);
                }
            }
            "ARC3" => {
                let c = (g.coord() as f64, g.coord() as f64);
                let r = g.range(50, 10_000_000) as f64;
                let a0 = g.range(0, 3599) as f64 / 10.0;
                let sw = g.range(-3590, 3590) as f64 / 10.0;
                for a in [a0, a0 + sw / 2.0, a0 + sw] {
                    let ar = a.to_radians();
                    l += &format!(
                        " {} {}",
                        fmt_f((c.0 + r * ar.cos()).round()),
                        fmt_f((c.1 + r * ar.sin()).round())
                    );
                }
                l += &format!(" {} {}", g.pick(&["N", "F"]), g.width());
            }
            "RECT" => {
                gen_pt(&mut g, &mut l);
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {}", g.pick(&["N", "F"]), g.width());
            }
            "TRECT" => {
                gen_pt(&mut g, &mut l);
                gen_pt(&mut g, &mut l);
                l += &format!(" {}", g.width());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "CIRC" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {} {}", g.size(), g.pick(&["N", "F"]), g.width());
            }
            "TCIRC" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {}", g.size(), g.width());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FCIRC" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {}", g.size());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "TOVAL" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {} {} {}", g.size(), g.size(), g.angle(), g.width());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "POLY" => {
                l += &format!(" {} {}", g.pick(&["N", "F"]), g.width());
                gen_md_ref(&mut g, mds, &mut l);
                let n = g.range(0, 8) as usize;
                gen_pts(&mut g, &mut l, n);
            }
            "CHAIN" | "ASREG" => {
                l += &format!(" {} {}", g.pick(&["N", "F"]), g.width());
                gen_md_ref(&mut g, mds, &mut l);
                gen_chain(&mut g, &mut l);
            }
            "REGV" => {
                gen_md_ref(&mut g, mds, &mut l);
                let n = g.range(0, 8) as usize;
                gen_pts(&mut g, &mut l, n);
            }
            "REGC" => {
                gen_md_ref(&mut g, mds, &mut l);
                gen_chain(&mut g, &mut l);
            }
            "TPOLY" => {
                l += &format!(" {}", g.width());
                gen_md_ref(&mut g, mds, &mut l);
                let c = (g.coord(), g.coord());
                gen_polyset(&mut g, &mut l, c);
            }
            "FC" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {}", g.size());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FO" | "FR" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {} {}", g.size(), g.size(), g.angle());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FRR" => {
                gen_pt(&mut g, &mut l);
                let (sx, sy) = (g.size(), g.size());
                let r = match g.below(4) {
                    0 => 0,
                    1 => sx.min(sy) / 2,
                    _ => g.range(0, sx.min(sy) / 2),
                };
                l += &format!(" {sx} {sy} {r} {}", g.angle());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FT" => {
                gen_pt(&mut g, &mut l);
                let (sx, sy) = (g.size() / 2, g.size() / 2);
                let d = g.range(0, sx / 2);
                l += &format!(
                    " {} {} {} {} {} {} {} {}",
                    -sx + d,
                    sy,
                    -sx - d,
                    -sy,
                    sx + d,
                    -sy,
                    sx - d,
                    sy
                );
                l += &format!(" {}", g.angle());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FCUS" => {
                let c = (g.coord(), g.coord());
                l += &format!(" {} {} {} {} {}", c.0, c.1, g.size(), g.size(), g.angle());
                gen_md_ref(&mut g, mds, &mut l);
                // reuse a few shapes so that FreePoly macros are shared
                if g.below(2) == 0 {
                    let mut g2 = Lcg(g.below(3) as u64);
                    gen_polyset(&mut g2, &mut l, c);
                } else {
                    gen_polyset(&mut g, &mut l, c);
                }
            }
            "FCH" => {
                l += &format!(" {}", chamfer_entry(g.below(42)));
                gen_md_ref(&mut g, mds, &mut l);
            }
            "FPOLY" => {
                gen_pt(&mut g, &mut l);
                l += &format!(" {} {} {}", g.size(), g.range(3, 12), g.angle());
                gen_md_ref(&mut g, mds, &mut l);
            }
            "POL" => l += &format!(" {}", g.below(2)),
            "SB" | "EB" => gen_md_ref(&mut g, mds, &mut l),
            "CLR" => {}
            _ => unreachable!(),
        }

        out.push_str(&l);
        out.push('\n');
    }

    if g.below(10) == 0 {
        out.push_str(&format!(
            "MARK {} {} {} {}\n",
            g.coord(),
            g.coord(),
            g.size(),
            g.below(58)
        ));
    }

    out.push_str("END\n");
}

fn generate() -> String {
    let mut out = String::new();

    for idx in 0..SCRIPT_COUNT {
        gen_script(idx, &mut out);
    }

    out
}

// ---- interpreter (twin of runScript() in gerber_plotter_oracle.cpp) ----

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

fn run_script(lines: &[&str]) -> String {
    let mut plotter = GerberPlotter::new();
    let mut mds: BTreeMap<i32, GbrMetadata> = BTreeMap::new();
    let mut creator = String::new();
    let mut started = false;

    plotter.set_render_settings(Some(RenderSettings::default()));
    plotter.open_file("plot.gbr");
    plotter.set_build_version("@VERSION@");
    plotter.set_creation_date("@DATE@");

    for line in lines {
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

        match op {
            "CFG" => {
                let x2 = t.n() != 0;
                let net = t.n() != 0;
                let nomacro = t.n() != 0;
                let fmt = t.n();
                let inch = t.n() != 0;
                let off = t.pt();
                let ipd = t.d();
                plotter.set_viewport(off, ipd, 1.0, false);
                plotter.set_gerber_coordinates_format(fmt, inch);
                plotter.use_x2format(x2);
                plotter.use_x2_net_attributes(net);
                plotter.disable_apert_macros(nomacro);
            }
            "CREATOR" => {
                creator = t.str();
                plotter.set_creator(&creator);
            }
            "HDR" => {
                let h = t.str();
                plotter.add_line_to_header(&h);
            }
            "RS" => {
                let w = t.n();
                plotter
                    .state_mut()
                    .render_settings
                    .as_mut()
                    .unwrap()
                    .set_default_pen_width(w);
            }
            "MD" => {
                let id = t.n();
                let mut m = GbrMetadata::new();
                m.aperture_metadata.apert_attribute = GbrApertureAttrib::from_i32(t.n()).unwrap();
                m.aperture_metadata.custom_attribute = t.str();
                m.netlist_metadata.net_attrib_type = t.n();
                m.netlist_metadata.not_in_net = t.n() != 0;
                let pad = t.str();
                let padflags = t.n();
                m.set_pad_name(&pad, padflags & 1 != 0, padflags & 2 != 0);
                let pin = t.str();
                let pinflags = t.n();
                m.set_pad_pin_function(&pin, pinflags & 1 != 0, pinflags & 2 != 0);
                m.set_cmp_reference(&t.str());
                m.set_net_name(&t.str());
                m.netlist_metadata.extra_data = EXTRA[t.n() as usize].to_string();
                mds.insert(id, m);
            }
            "START" => {
                plotter.start_plot("1");
                started = true;
            }
            "W" => {
                let w = t.n();
                let m = md!();
                plotter.set_current_line_width(w, m.as_ref());
            }
            "PEN" => {
                let c = t.s().chars().next().unwrap();
                let p = t.pt();
                plotter.pen_to(p, c);
            }
            "MV" => {
                let p = t.pt();
                plotter.move_to(p);
            }
            "LN" => {
                let p = t.pt();
                plotter.line_to(p);
            }
            "FIN" => {
                let p = t.pt();
                plotter.finish_to(p);
            }
            "SEG" => {
                let a = t.pt();
                let b = t.pt();
                let w = t.n();
                let m = md!();
                plotter.thick_segment(a, b, w, m.as_ref());
            }
            "ARC" => {
                let c = t.ptd();
                let sa = t.ang();
                let a = t.ang();
                let r = t.d();
                let f = t.fill();
                let w = t.n();
                plotter.arc(c, sa, a, r, f, w);
            }
            "ARC3" => {
                let s = t.ptd();
                let m = t.ptd();
                let e = t.ptd();
                let f = t.fill();
                let w = t.n();
                plotter.arc_3pt(s, m, e, f, w);
            }
            "TARC" => {
                let c = t.ptd();
                let sa = t.ang();
                let a = t.ang();
                let r = t.d();
                let w = t.n();
                let m = md!();
                plotter.thick_arc(c, sa, a, r, w, m.as_ref());
            }
            "RECT" => {
                let a = t.pt();
                let b = t.pt();
                let f = t.fill();
                let w = t.n();
                plotter.rect(a, b, f, w, 0);
            }
            "TRECT" => {
                let a = t.pt();
                let b = t.pt();
                let w = t.n();
                let m = md!();
                plotter.thick_rect(a, b, w, m.as_ref());
            }
            "CIRC" => {
                let c = t.pt();
                let d = t.n();
                let f = t.fill();
                let w = t.n();
                plotter.circle(c, d, f, w);
            }
            "TCIRC" => {
                let c = t.pt();
                let d = t.n();
                let w = t.n();
                let m = md!();
                plotter.thick_circle(c, d, w, m.as_ref());
            }
            "FCIRC" => {
                let c = t.pt();
                let d = t.n();
                let m = md!();
                plotter.filled_circle(c, d, m.as_ref());
            }
            "TOVAL" => {
                let p = t.pt();
                let s = t.pt();
                let o = t.ang();
                let w = t.n();
                let m = md!();
                plotter.thick_oval(p, s, o, w, m.as_ref());
            }
            "POLY" => {
                let f = t.fill();
                let w = t.n();
                let m = md!();
                let v = t.pts();
                plotter.plot_poly(&v, f, w, m.as_ref());
            }
            "CHAIN" => {
                let f = t.fill();
                let w = t.n();
                let m = md!();
                let c = t.chain();
                plotter.plot_poly_chain(&c, f, w, m.as_ref());
            }
            "REGV" => {
                let m = md!();
                let v = t.pts();
                plotter.plot_gerber_region(&v, m.as_ref());
            }
            "REGC" => {
                let m = md!();
                let c = t.chain();
                plotter.plot_gerber_region_chain(&c, m.as_ref());
            }
            "ASREG" => {
                let f = t.fill();
                let w = t.n();
                let m = md!();
                let c = t.chain();
                plotter.plot_poly_as_region(&c, f, w, m.as_ref());
            }
            "TPOLY" => {
                let w = t.n();
                let m = md!();
                let ps = t.polyset();
                plotter.thick_poly(&ps, w, m.as_ref());
            }
            "FC" => {
                let p = t.pt();
                let d = t.n();
                let m = md!();
                plotter.flash_pad_circle(p, d, m.as_ref());
            }
            "FO" | "FR" => {
                let p = t.pt();
                let s = t.pt();
                let o = t.ang();
                let m = md!();

                if op == "FO" {
                    plotter.flash_pad_oval(p, s, o, m.as_ref());
                } else {
                    plotter.flash_pad_rect(p, s, o, m.as_ref());
                }
            }
            "FRR" => {
                let p = t.pt();
                let s = t.pt();
                let r = t.n();
                let o = t.ang();
                let m = md!();
                plotter.flash_pad_round_rect(p, s, r, o, m.as_ref());
            }
            "FT" => {
                let p = t.pt();
                let c = [t.pt(), t.pt(), t.pt(), t.pt()];
                let o = t.ang();
                let m = md!();
                plotter.flash_pad_trapez(p, &c, o, m.as_ref());
            }
            "FCUS" => {
                let p = t.pt();
                let s = t.pt();
                let o = t.ang();
                let m = md!();
                let ps = t.polyset();
                plotter.flash_pad_custom(p, s, o, &ps, m.as_ref());
            }
            "FCH" => {
                let p = t.pt();
                let s = t.pt();
                let r = t.n();
                let ratio = t.d();
                let pos = t.n();
                let o = t.ang();
                let m = md!();
                // the default TransformRoundChamferedRectToPolygon (kicad-geom); the C++
                // polygons of the golden file are checked by round_rect_polygons_match_cpp
                plotter.flash_pad_chamfer_round_rect_default(p, s, r, ratio, pos, o, m.as_ref());
            }
            "FPOLY" => {
                let p = t.pt();
                let d = t.n();
                let n = t.n();
                let o = t.ang();
                let m = md!();
                plotter.flash_regular_polygon(p, d, n, o, m.as_ref());
            }
            "POL" => {
                let v = t.n() != 0;
                plotter.set_layer_polarity(v);
            }
            "SB" => {
                let m = md!();
                plotter.start_block(m.as_ref());
            }
            "EB" => {
                let m = md!();
                plotter.end_block(m.as_ref());
            }
            "CLR" => plotter.clear_all_attributes(),
            "MARK" => {
                let p = t.pt();
                let d = t.n();
                let id = t.n();
                plotter.marker(p, d, id as u32);
            }
            _ => panic!("unknown op {op}"),
        }
    }

    if started {
        plotter.end_plot();
    }

    let _ = &creator;
    plotter.output().to_string()
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
fn gerber_plotter_matches_oracle() {
    let all = generate();
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plot-oracle");

    if std::env::var_os("KICAD_PLOT_WRITE_SCRIPTS").is_some() {
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join("gerber_scripts.txt"), &all).unwrap();
    }

    let golden = parse_golden();
    let scripts = split_scripts(&all);

    assert_eq!(
        golden.scripts,
        (scripts.len(), fnv(all.as_bytes())),
        "the script generator changed: regenerate the golden file \
         (tests/oracle/build_gerber_plotter_oracle.sh \"\" regen)"
    );

    let mut mismatches = Vec::new();

    for (idx, script) in scripts.iter().enumerate() {
        let out = run_script(script);

        let ok = match golden.full.get(&idx) {
            Some(text) => *text == out,
            None => golden.hashes.get(&idx) == Some(&(fnv(out.as_bytes()) & 0xffff_ffff)),
        };

        if !ok {
            if mismatches.len() < 20 {
                let _ = std::fs::create_dir_all(&out_dir);
                let _ = std::fs::write(out_dir.join(format!("rust_{idx}.gbr")), &out);
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

/// Every `TransformRoundChamferedRectToPolygon` result recorded by the C++ driver (the `CH`
/// lines of the golden file, keyed by [`chamfer_key`]) is reproduced by the default generator
/// of the plotters, [`kicad_plot::plotter_gerber::round_chamfered_rect_to_polygon`], point for
/// point.
#[test]
fn round_rect_polygons_match_cpp() {
    let golden = parse_golden();
    assert!(!golden.chamfer.is_empty());

    for (key, pts) in &golden.chamfer {
        let f: Vec<&str> = key.split(',').collect();
        let n = |i: usize| f[i].parse::<i32>().unwrap();
        let hex = |i: usize| f64::from_bits(u64::from_str_radix(f[i], 16).unwrap());
        let (pos, size, rot) = (
            Vector2I::new(n(0), n(1)),
            Vector2I::new(n(2), n(3)),
            EdaAngle::new(hex(4), DEGREES_T),
        );
        assert_eq!(chamfer_key(pos, size, rot, n(5), hex(6), n(7), n(8)), *key);
        let outline = kicad_plot::plotter_gerber::round_chamfered_rect_to_polygon(
            pos,
            size,
            rot,
            n(5),
            hex(6),
            n(7),
            n(8),
        );
        let got: Vec<Vector2I> = if outline.outline_count() == 0 {
            Vec::new()
        } else {
            outline.c_outline(0).c_points().to_vec()
        };

        assert_eq!(&got, pts, "polygon {key}");
    }
}
