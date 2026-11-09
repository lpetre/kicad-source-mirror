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

//! Differential test of work packages PLOT-01 and PLOT-02 against the C++ implementation.
//!
//! `tests/oracle/plot_base_oracle.cpp` calls the original `PLOTTER` base class (through a
//! recording subclass), `DRILL_MARKERS` and the Gerber metadata functions of `libcommon.a` on
//! about 20 000 deterministic edge-case and pseudo-random inputs. The golden file
//! `tests/golden/plot_base.txt` keeps, per section, the first lines verbatim and the line
//! count and FNV-1a hash of all lines. [`dump`] mirrors `main()` of the C++ program line by
//! line, including the order of the pseudo-random draws. On a mismatch the Rust lines are
//! written to `target/plot-oracle/rust_full.txt` for a diff against `plot_base_oracle full`.

use std::collections::BTreeMap;

use kicad_core::color4d::Color4D;
use kicad_core::stroke_params::LineStyle;
use kicad_geom::cmath;
use kicad_geom::{DEGREES_T, EdaAngle, ShapeLineChain, ShapePolySet, Vector2D, Vector2I};
use kicad_plot::drill_markers::{curated_shape, curated_shape_count};
use kicad_plot::gbr_metadata::*;
use kicad_plot::gbr_netlist_metadata::{GbrDataField, GbrNetlistMetadata};
use kicad_plot::plotter::*;
use kicad_plot::{FillT, PlotFormat, RenderSettings};

// ---- shared helpers (mirrored in C++) ----

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
        lo + self.below((hi - lo + 1) as u32) as i32
    }
}

fn esc(s: &[u8]) -> String {
    let mut out = String::new();

    for &c in s {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            c if c < 0x20 || c >= 0x7F => out.push_str(&format!("\\x{c:02X}")),
            c => out.push(c as char),
        }
    }

    out
}

fn u(s: &str) -> String {
    esc(s.as_bytes())
}

fn h(d: f64) -> String {
    format!("{:016x}", d.to_bits())
}

fn i<T: ToString>(v: T) -> String {
    v.to_string()
}

fn b(v: bool) -> String {
    (v as i32).to_string()
}

fn pt(p: Vector2I) -> String {
    format!("{},{}", p.x, p.y)
}

#[derive(Default)]
struct Out {
    sections: Vec<(String, Vec<String>)>,
}

impl Out {
    fn begin(&mut self, name: &str) {
        self.sections.push((name.to_string(), Vec::new()));
    }

    fn out(&mut self, line: String) {
        self.sections.last_mut().unwrap().1.push(line);
    }
}

fn fnv(lines: &[String]) -> u64 {
    let mut hash: u64 = 1469598103934665603;

    for line in lines {
        for &c in line.as_bytes().iter().chain(b"\n") {
            hash ^= c as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
    }

    hash
}

fn rcoord(g: &mut Lcg) -> i32 {
    match g.below(4) {
        0 => g.range(-50, 50),
        1 => g.range(-1000000, 1000000),
        2 => g.range(-100000000, 100000000),
        _ => g.range(-5000, 5000) * 1000,
    }
}

fn rpt(g: &mut Lcg) -> Vector2I {
    let x = rcoord(g);
    let y = rcoord(g);
    Vector2I::new(x, y)
}

fn rdouble(g: &mut Lcg) -> f64 {
    let a = rcoord(g) as f64;

    match g.below(3) {
        0 => a,
        1 => a + g.below(1000) as f64 / 64.0,
        _ => a / 7.0,
    }
}

fn rdeg(g: &mut Lcg) -> f64 {
    const SPECIAL: [f64; 12] = [
        0.0, 90.0, 180.0, -180.0, 360.0, 45.0, 0.05, -0.05, 270.0, -90.0, 720.0, 359.99,
    ];

    match g.below(3) {
        0 => SPECIAL[g.below(12) as usize],
        1 => g.range(-144, 144) as f64 * 5.0,
        _ => g.range(-400000, 400000) as f64 / 1000.0,
    }
}

fn rwidth(g: &mut Lcg) -> i32 {
    match g.below(5) {
        0 => DO_NOT_SET_LINE_WIDTH,
        1 => USE_DEFAULT_LINE_WIDTH,
        2 => 0,
        _ => g.range(1, 3000000),
    }
}

fn rfill(g: &mut Lcg) -> FillT {
    if g.below(2) != 0 {
        FillT::NoFill
    } else {
        FillT::FilledShape
    }
}

const CODEPOINTS: [u32; 24] = [
    '\\' as u32,
    '%' as u32,
    '*' as u32,
    ',' as u32,
    '"' as u32,
    0,
    1,
    '\n' as u32,
    0x7F,
    0x80,
    0xA0,
    0xE9,
    0xFF,
    0x100,
    0x3A9,
    0x4E2D,
    0xFEFF,
    0xFFFD,
    0xFFFF,
    0x10000,
    0x1F600,
    0x10FFFF,
    'X' as u32,
    ' ' as u32,
];

fn rstring(g: &mut Lcg, max_len: u32) -> String {
    let mut s = String::new();
    let len = g.below(max_len + 1);

    for _ in 0..len {
        let cp = if g.below(3) == 0 {
            CODEPOINTS[g.below(CODEPOINTS.len() as u32) as usize]
        } else {
            0x21 + g.below(0x5E)
        };

        s.push(char::from_u32(cp).unwrap());
    }

    // Sometimes a quoted string
    if g.below(8) == 0 {
        s = format!("\"{s}\"");
    }

    s
}

// ---- recording plotter ----

struct Rec {
    state: PlotterState,
    log: String,
}

impl Rec {
    fn new() -> Self {
        Rec {
            state: PlotterState::new(),
            log: String::new(),
        }
    }

    fn ev(&mut self, e: String) {
        self.log.push(' ');
        self.log.push_str(&e);
    }

    fn take(&mut self) -> String {
        std::mem::take(&mut self.log)
    }
}

impl Plotter for Rec {
    fn state(&self) -> &PlotterState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut PlotterState {
        &mut self.state
    }

    fn plotter_type(&self) -> PlotFormat {
        PlotFormat::Undefined
    }

    fn start_plot(&mut self, _a_page_number: &str) -> bool {
        true
    }

    fn end_plot(&mut self) -> bool {
        true
    }

    fn set_current_line_width(&mut self, width: i32, _a_data: PlotData) {
        self.ev(format!("W{width}"));

        if width == USE_DEFAULT_LINE_WIDTH {
            self.state.current_pen_width = 77;
        } else if width != DO_NOT_SET_LINE_WIDTH {
            self.state.current_pen_width = width;
        }
    }

    fn set_color(&mut self, _color: &Color4D) {}

    fn set_dash(&mut self, _a_line_width: i32, _a_line_style: LineStyle) {}

    fn set_viewport(&mut self, _o: Vector2I, _i: f64, _s: f64, _m: bool) {}

    fn rect(&mut self, p1: Vector2I, p2: Vector2I, fill: FillT, width: i32, r: i32) {
        self.ev(format!(
            "R{};{};{};{};{}",
            pt(p1),
            pt(p2),
            fill as i32,
            width,
            r
        ));
    }

    fn circle(&mut self, pos: Vector2I, diametre: i32, fill: FillT, width: i32) {
        self.ev(format!(
            "C{};{};{};{}",
            pt(pos),
            diametre,
            fill as i32,
            width
        ));
    }

    fn pen_to(&mut self, pos: Vector2I, plume: char) {
        self.ev(format!("P{plume}{}", pt(pos)));
    }

    fn plot_poly(&mut self, a_corner_list: &[Vector2I], a_fill: FillT, a_width: i32, _d: PlotData) {
        let mut e = format!("Y{};{};{}", a_fill as i32, a_width, a_corner_list.len());

        for p in a_corner_list {
            e += &format!(";{}", pt(*p));
        }

        self.ev(e);
    }

    fn flash_pad_circle(&mut self, _p: Vector2I, _d: i32, _a: PlotData) {}
    fn flash_pad_oval(&mut self, _p: Vector2I, _s: Vector2I, _o: EdaAngle, _a: PlotData) {}
    fn flash_pad_rect(&mut self, _p: Vector2I, _s: Vector2I, _o: EdaAngle, _a: PlotData) {}
    fn flash_pad_round_rect(
        &mut self,
        _p: Vector2I,
        _s: Vector2I,
        _r: i32,
        _o: EdaAngle,
        _a: PlotData,
    ) {
    }
    fn flash_pad_custom(
        &mut self,
        _p: Vector2I,
        _s: Vector2I,
        _o: EdaAngle,
        _ps: &ShapePolySet,
        _a: PlotData,
    ) {
    }
    fn flash_pad_trapez(&mut self, _p: Vector2I, _c: &[Vector2I; 4], _o: EdaAngle, _a: PlotData) {}
    fn flash_regular_polygon(
        &mut self,
        _p: Vector2I,
        _d: i32,
        _c: i32,
        _o: EdaAngle,
        _a: PlotData,
    ) {
    }
}

// ---- sections ----

fn section_transform(o: &mut Out) {
    let mut g = Lcg(101);
    let mut rec = Rec::new();
    let mut rs = RenderSettings::default();

    o.begin("transform");

    let scales = [1.0, 0.5, 1.7, 2.0, 0.123456789];
    let iu_per_dev = [
        1.0,
        cmath::pow(10.0, 6.0) / (2540.0 * 10000.0) * 25.4,
        cmath::pow(10.0, 5.0) / (2540.0 * 10000.0) * 25.4,
        1.0 / (2540.0 / 2.54 * 1000.0),
        0.0001 * 3.0 / 7.0,
    ];

    for _ in 0..3000 {
        let off = rpt(&mut g);
        let scale = scales[g.below(5) as usize];
        let iu = iu_per_dev[g.below(5) as usize];
        let px = g.range(0, 500000000);
        let py = g.range(0, 500000000);
        let paper = Vector2I::new(px, py);
        let mirror = g.below(2) != 0;
        let horiz = g.below(2) != 0;
        let yrev = g.below(2) != 0;
        let p = rpt(&mut g);
        let w = g.range(0, 1000000);
        let v = rdouble(&mut g);

        rs.set_dash_length_ratio(g.range(0, 30) as f64 / 2.0);
        rs.set_gap_length_ratio(g.range(-4, 20) as f64 / 2.0);
        rec.set_render_settings(Some(rs.clone()));

        let s = rec.state_mut();
        s.plot_offset = off;
        s.plot_scale = scale;
        s.ius_per_decimil = 2540.0;
        s.iu_per_device_unit = iu;
        s.paper_size = paper;
        s.plot_mirror = mirror;
        s.mirror_is_horizontal = horiz;
        s.yaxis_reversed = yrev;

        let c = rec.user_to_device_coordinates(p);
        let sz = rec.user_to_device_size(p);

        o.out(format!(
            "{} {} {}{}{} {} {} {} {} {} {} {} {} {}",
            pt(off),
            pt(paper),
            b(mirror),
            b(horiz),
            b(yrev),
            pt(p),
            h(c.x),
            h(c.y),
            h(sz.x),
            h(sz.y),
            h(rec.user_to_device_size_f(v)),
            h(rec.dot_mark_len_iu(w)),
            h(rec.dash_mark_len_iu(w)),
            h(rec.dash_gap_len_iu(w))
        ));
    }
}

fn section_arcs(o: &mut Out) {
    let mut g = Lcg(202);
    let mut rec = Rec::new();

    o.begin("polyarc");

    for _ in 0..2000 {
        let cx = rdouble(&mut g);
        let cy = rdouble(&mut g);
        let c = Vector2D::new(cx, cy);
        let st = EdaAngle::new(rdeg(&mut g), DEGREES_T);
        let an = EdaAngle::new(rdeg(&mut g), DEGREES_T);
        let r = rdouble(&mut g).abs();
        let fill = rfill(&mut g);
        let w = rwidth(&mut g);

        rec.arc(c, st, an, r, fill, w);
        let log = rec.take();
        o.out(format!(
            "{} {} {} {} {}{}",
            h(c.x),
            h(c.y),
            h(st.as_degrees()),
            h(an.as_degrees()),
            h(r),
            log
        ));
    }

    o.begin("arc3pt");

    for _ in 0..2000 {
        let mut v = [0.0; 6];

        for x in v.iter_mut() {
            *x = rdouble(&mut g);
        }

        let a = Vector2D::new(v[0], v[1]);
        let m = Vector2D::new(v[2], v[3]);
        let mut e = Vector2D::new(v[4], v[5]);

        if g.below(10) == 0 {
            e = a;
        }

        let fill = rfill(&mut g);
        let w = rwidth(&mut g);

        rec.arc_3pt(a, m, e, fill, w);
        let log = rec.take();
        o.out(format!(
            "{}{}{}{}{}{}{}",
            h(a.x),
            h(a.y),
            h(m.x),
            h(m.y),
            h(e.x),
            h(e.y),
            log
        ));
    }
}

fn section_thick(o: &mut Out) {
    let mut g = Lcg(303);
    let mut rec = Rec::new();

    o.begin("thick");

    for _ in 0..3000 {
        let kind = g.below(7);
        let a = rpt(&mut g);
        let bb = if g.below(4) == 0 { a } else { rpt(&mut g) };
        let w = rwidth(&mut g);
        let d = g.range(0, 4000000);

        rec.state.current_pen_width = g.range(-3, 5000);

        let mut head = format!("{} {} {} {} {}", kind, pt(a), pt(bb), w, d);

        match kind {
            0 => rec.thick_segment(a, bb, w, None),
            1 => rec.thick_rect(a, bb, w, None),
            2 => rec.thick_circle(a, d, w, None),
            3 => rec.filled_circle(a, d, None),
            4 => {
                let sx = g.range(0, 3000000);
                let sy = g.range(0, 3000000);
                let size = Vector2I::new(sx, sy);
                let orient = EdaAngle::new(rdeg(&mut g), DEGREES_T);
                head += &format!(" {} {}", pt(size), h(orient.as_degrees()));
                rec.thick_oval(a, size, orient, w, None);
            }
            5 => {
                let st = EdaAngle::new(rdeg(&mut g), DEGREES_T);
                let an = EdaAngle::new(rdeg(&mut g), DEGREES_T);
                let r = d as f64 / 3.0;
                head += &format!(" {} {}", h(st.as_degrees()), h(an.as_degrees()));
                rec.thick_arc(a.into(), st, an, r, w, None);
            }
            _ => {
                let sx = g.range(1, 3000);
                let sy = g.range(1, 3000);
                head += &format!(" {sx} {sy}");
                let scale = g.range(1, 4000) as f64 / 7.0;
                rec.plot_image(sx, sy, a, scale, 1.0);
            }
        }

        let log = rec.take();
        o.out(head + &log);
    }

    o.begin("polys");

    for _ in 0..1500 {
        let n = g.below(7);
        let mut pts = Vec::new();

        for _ in 0..n {
            pts.push(rpt(&mut g));
        }

        if n > 2 && g.below(3) == 0 {
            pts.push(pts[0]);
        }

        let closed = g.below(2) != 0;
        let fill = rfill(&mut g);
        let w = rwidth(&mut g);

        let head = format!("{} {} {}", n, b(closed), pts.len());

        let chain = ShapeLineChain::from_points(&pts, closed);

        // An empty closed chain is undefined behaviour in C++ (front() of an empty vector)
        if !(closed && pts.is_empty()) {
            rec.plot_poly_chain(&chain, fill, w, None);
        }

        if n > 0 {
            let closed_chain = ShapeLineChain::from_points(&pts, true);
            let mut set = ShapePolySet::new();
            set.add_outline(&closed_chain);
            set.add_outline(&ShapeLineChain::from_points(
                &[
                    Vector2I::new(1, 2),
                    Vector2I::new(3, 4),
                    Vector2I::new(5, 0),
                ],
                true,
            ));
            rec.thick_poly(&set, w, None);
        }

        let log = rec.take();
        o.out(head + &log);
    }
}

fn section_bezier(o: &mut Out) {
    let mut g = Lcg(808);
    let mut rec = Rec::new();

    o.begin("bezier");

    for _ in 0..1000 {
        let a = rpt(&mut g);
        let c1 = if g.below(6) == 0 { a } else { rpt(&mut g) };
        let c2 = rpt(&mut g);
        let e = if g.below(6) == 0 { c2 } else { rpt(&mut g) };
        let tol = g.range(1, 20000);
        let w = rwidth(&mut g);

        rec.bezier_curve(a, c1, c2, e, tol, w);
        let log = rec.take();
        o.out(format!(
            "{} {} {} {} {}{}",
            pt(a),
            pt(c1),
            pt(c2),
            pt(e),
            tol,
            log
        ));
    }
}

fn section_markers(o: &mut Out) {
    let mut g = Lcg(404);
    let mut rec = Rec::new();

    o.begin("markers");

    for id in 0..62u32 {
        for _ in 0..6 {
            let p = rpt(&mut g);
            let d = g.range(0, 3000000);
            rec.state.current_pen_width = g.range(0, 5000);
            rec.marker(p, d, id);
            let log = rec.take();
            o.out(format!("{} {} {}{}", id, pt(p), d, log));
        }
    }

    for _ in 0..300 {
        let p = rpt(&mut g);
        let r = g.range(0, 3000000);
        rec.state.current_pen_width = g.range(0, 5000);
        rec.marker_square(p, r);
        rec.marker_lozenge(p, r);
        rec.marker_circle(p, r);
        rec.marker_h_bar(p, r);
        rec.marker_slash(p, r);
        rec.marker_back_slash(p, r);
        rec.marker_v_bar(p, r);
        let log = rec.take();
        o.out(format!("{} {}{}", pt(p), r, log));
    }

    o.begin("curated");

    let mut line = i(curated_shape_count());

    for k in -2..15 {
        line += &format!(" {}", curated_shape(k));
    }

    o.out(line);
}

fn section_guid(o: &mut Out) {
    let mut g = Lcg(505);

    o.begin("guid");

    let fixed = [
        "basic_test.kicad_pcb",
        "Issue24143.kicad_pcb",
        "",
        "a",
        "0123456789abcdef",
        "0123456789abcdefghij",
    ];

    for f in fixed {
        o.out(format!(
            "{} {}",
            u(f),
            u(&gbr_make_project_guid_from_string(f))
        ));
    }

    for _ in 0..3000 {
        let s = rstring(&mut g, 24);
        o.out(format!(
            "{} {}",
            u(&s),
            u(&gbr_make_project_guid_from_string(&s))
        ));
    }
}

fn section_escape(o: &mut Out) {
    let mut g = Lcg(606);

    o.begin("escape");

    for _ in 0..5000 {
        let s = rstring(&mut g, 12);

        let mut line = u(&s);

        for k in 0..4 {
            line += " ";
            line += &u(&convert_not_allowed_chars_in_gerber(
                &s,
                k & 1 != 0,
                k & 2 != 0,
            ));
        }

        line += " ";
        line += &u(&format_string_to_gerber(&s));

        let mut field = GbrDataField::new();
        let utf8 = g.below(2) != 0;
        let quote = g.below(2) != 0;
        field.set_field(&s, utf8, quote);
        line += &format!(" {}{} {}", b(utf8), b(quote), u(&field.gerber_string()));

        o.out(line);
    }
}

fn section_aper_attr(o: &mut Out) {
    o.begin("aperattr");

    let customs = ["", "Custom,text", "x*%"];

    for a in GbrApertureAttrib::ALL {
        for x1 in 0..2 {
            for c in customs {
                o.out(format!(
                    "{} {} {} {}",
                    a as i32,
                    x1,
                    u(c),
                    u(&GbrApertureMetadata::format_attribute_static(a, x1 != 0, c))
                ));
            }
        }
    }
}

fn section_net_attr(o: &mut Out) {
    let mut g = Lcg(707);

    o.begin("netattr");

    let pool = [
        "",
        "R1",
        "U2",
        "GND",
        "Net-(R1-Pad1)",
        "a,b*c%d\\e",
        "\u{e9}t\u{e9}",
        "\"quoted\"",
    ];

    for seq in 0..300 {
        let mut last = String::new();
        let x1 = g.below(2) != 0;

        for step in 0..12 {
            let mut md = GbrNetlistMetadata::new();
            md.net_attrib_type = g.below(8) as i32;
            md.not_in_net = g.below(4) == 0;
            md.try_keep_previous_attributes = g.below(3) == 0;
            md.cmpref = pool[g.below(8) as usize].to_string();
            md.netname = pool[g.below(8) as usize].to_string();

            let u1 = g.below(2) != 0;
            let q1 = g.below(2) != 0;
            md.padname.set_field(pool[g.below(8) as usize], u1, q1);
            let u2 = g.below(2) != 0;
            let q2 = g.below(2) != 0;
            md.pad_pin_function
                .set_field(pool[g.below(8) as usize], u2, q2);
            let use_null = g.below(20) == 0;

            let res = format_net_attribute(&mut last, if use_null { None } else { Some(&md) }, x1);
            let (ret, clear, printed) = match res {
                None => (false, false, "<untouched>".to_string()),
                Some(r) => (
                    true,
                    r.clear_previous_attributes,
                    r.printed_text.unwrap_or_else(|| "<untouched>".to_string()),
                ),
            };

            o.out(format!(
                "{} {} {} {}{}{}{} {}|{}|{}{}{}|{}{}{} {}{} {} {}",
                seq,
                step,
                b(x1),
                b(use_null),
                md.net_attrib_type,
                b(md.not_in_net),
                b(md.try_keep_previous_attributes),
                u(&md.cmpref),
                u(&md.netname),
                u(md.padname.value()),
                b(u1),
                b(q1),
                u(md.pad_pin_function.value()),
                b(u2),
                b(q2),
                b(ret),
                b(clear),
                u(&printed),
                u(&last)
            ));
        }
    }
}

fn dump() -> Out {
    let mut o = Out::default();
    section_transform(&mut o);
    section_arcs(&mut o);
    section_thick(&mut o);
    section_bezier(&mut o);
    section_markers(&mut o);
    section_guid(&mut o);
    section_escape(&mut o);
    section_aper_attr(&mut o);
    section_net_attr(&mut o);
    o
}

#[test]
fn plot_base_oracle_golden() {
    let golden_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/plot_base.txt");
    let golden = std::fs::read_to_string(golden_path).expect("golden file");

    let mut expected_lines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut expected_summary: BTreeMap<String, (usize, u64)> = BTreeMap::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("E ") {
            let (section, text) = rest.split_once(' ').unwrap();
            expected_lines
                .entry(section.to_string())
                .or_default()
                .push(text.to_string());
        } else if let Some(rest) = line.strip_prefix("S ") {
            let parts: Vec<&str> = rest.split(' ').collect();
            expected_summary.insert(
                parts[0].to_string(),
                (
                    parts[1].parse().unwrap(),
                    u64::from_str_radix(parts[2], 16).unwrap(),
                ),
            );
        }
    }

    let o = dump();
    let mut failures = Vec::new();

    assert_eq!(o.sections.len(), expected_summary.len(), "section count");

    for (name, lines) in &o.sections {
        let (count, hash) = expected_summary[name];

        for (k, exp) in expected_lines.get(name).into_iter().flatten().enumerate() {
            if lines.get(k) != Some(exp) {
                failures.push(format!(
                    "{name} line {k}:\n  C++ : {exp}\n  Rust: {:?}",
                    lines.get(k)
                ));
            }
        }

        if lines.len() != count || fnv(lines) != hash {
            failures.push(format!(
                "{name}: {} lines (C++ {count}), hash mismatch",
                lines.len()
            ));
        }
    }

    if !failures.is_empty() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/plot-oracle");
        let _ = std::fs::create_dir_all(dir);
        let full: String = o
            .sections
            .iter()
            .flat_map(|(name, lines)| lines.iter().map(move |l| format!("{name} {l}\n")))
            .collect();
        let _ = std::fs::write(format!("{dir}/rust_full.txt"), full);

        panic!(
            "{} mismatches (diff target/plot-oracle/rust_full.txt against `plot_base_oracle full`):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
