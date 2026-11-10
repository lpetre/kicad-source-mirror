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

//! Replays the differential oracle `tests/oracle/font_oracle.cpp` (golden output in
//! `tests/golden/font_oracle.txt`): the same seeded case generator and the same output text
//! per case, compared through FNV-1a hashes.
//!
//! On a mismatch the Rust output of the case is printed; compare it with
//! `font_oracle full KIND IDX` of the C++ driver.

use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::callback_gal::{TextGal, to_vector2i};
use kicad_font::font::{Font, FontExt, default_font};
use kicad_font::font_metrics::Metrics;
use kicad_font::glyph::Glyph;
use kicad_font::gr_text::*;
use kicad_font::markup_parser::{MarkupNode, MarkupParser};
use kicad_font::newstroke_font::NEWSTROKE_FONT;
use kicad_geom::{Box2I, EdaAngle, Vector2I, ki_round};

struct Gen {
    state: u64,
    /// `g_extreme` of the driver: kinds 'O' and 'E' (out-of-range values)
    extreme: bool,
}

/// `genExtremeInt` table of the driver.
const EXTREME: [i32; 11] = [
    i32::MAX,
    i32::MIN,
    i32::MAX - 1000,
    i32::MIN + 1000,
    2000000000,
    -2000000000,
    1900000000,
    -1900000000,
    1500000000,
    -1500000000,
    1000000000,
];

impl Gen {
    fn next64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn rnd(&mut self, n: i64) -> i64 {
        (self.next64() % n as u64) as i64
    }

    fn gen_char(&mut self) -> u32 {
        let r = self.rnd(100);

        if r < 45 {
            return 0x21 + self.rnd(94) as u32;
        }
        if r < 57 {
            return ' ' as u32;
        }
        if r < 67 {
            return b"~_^{}"[self.rnd(5) as usize] as u32;
        }
        if r < 70 {
            return '\t' as u32;
        }
        if r < 72 {
            return '\n' as u32;
        }
        if r < 74 {
            return 1 + self.rnd(31) as u32;
        }
        if r < 80 {
            return 0xA0 + self.rnd(96) as u32;
        }
        if r < 85 {
            return 0x391 + self.rnd(57) as u32;
        }
        if r < 90 {
            return 0x410 + self.rnd(64) as u32;
        }
        if r < 94 {
            return 0x4E00 + self.rnd(0x5200) as u32;
        }
        if r < 96 {
            return 0x1F600 + self.rnd(80) as u32;
        }
        if r < 97 {
            return 0xFFEF + self.rnd(16) as u32;
        }
        if r < 98 {
            return 0;
        }
        0x100 + self.rnd(0xD800 - 0x100) as u32
    }

    fn gen_text(&mut self, max_len: i64) -> String {
        const TOKENS: [&str; 14] = [
            "~{",
            "_{",
            "^{",
            "}",
            "http://",
            "https://",
            "{slash}",
            "{dblquote}",
            "{ab_1}",
            "{a b}",
            "  ",
            "~{}",
            "~{ }",
            "x}",
        ];
        let mut s = String::new();
        let len = self.rnd(max_len + 1);

        for _ in 0..len {
            if self.rnd(6) == 0 {
                s.push_str(TOKENS[self.rnd(TOKENS.len() as i64) as usize]);
            } else {
                s.push(char::from_u32(self.gen_char()).unwrap());
            }
        }

        s
    }

    /// `noTabsIfExtreme` of the driver.
    fn no_tabs_if_extreme(&self, text: String) -> String {
        if self.extreme {
            text.replace('\t', " ")
        } else {
            text
        }
    }

    fn gen_extreme_int(&mut self) -> i32 {
        let r = self.rnd(16);

        if r < 11 {
            return EXTREME[r as usize];
        }

        (self.rnd(4294967296) - 2147483648) as i32
    }

    fn gen_size(&mut self) -> i32 {
        if self.extreme && self.rnd(2) == 0 {
            return self.gen_extreme_int();
        }

        let r = self.rnd(10);

        if r == 0 {
            return 0;
        }
        if r == 1 {
            return self.rnd(1000) as i32;
        }
        if r == 2 {
            return -(self.rnd(2000000) as i32);
        }
        (500000 + self.rnd(4500001)) as i32
    }

    fn gen_coord(&mut self) -> i32 {
        if self.extreme && self.rnd(2) == 0 {
            return self.gen_extreme_int();
        }

        (self.rnd(200000001) - 100000000) as i32
    }

    fn gen_angle(&mut self) -> f64 {
        const TABLE: [i32; 14] = [
            0, 0, 0, 900, 1800, 2700, 450, 300, 333, 3599, 1, 1350, 2250, 3150,
        ];
        let r = self.rnd(20);

        if r < 14 {
            return TABLE[r as usize] as f64 / 10.0;
        }

        self.rnd(3600) as f64 / 10.0
    }
}

#[derive(Default)]
struct Out {
    s: String,
}

impl Out {
    fn s(&mut self, t: &str) {
        self.s.push_str(t);
    }
    fn i(&mut self, v: i64) {
        self.s.push_str(&format!(" {v}"));
    }
    fn d(&mut self, v: f64) {
        self.s.push_str(&format!(" {:016x}", v.to_bits()));
    }
    fn f(&mut self, v: f32) {
        self.s.push_str(&format!(" {:08x}", v.to_bits()));
    }
    fn hex(&mut self, t: &str) {
        self.s.push(' ');

        if t.is_empty() {
            self.s.push('-');
        }

        for b in t.as_bytes() {
            self.s.push_str(&format!("{b:02x}"));
        }
    }
    fn bbox(&mut self, b: &Box2I) {
        self.i(b.x() as i64);
        self.i(b.y() as i64);
        self.i(b.width());
        self.i(b.height());
    }
    fn glyphs(&mut self, glyphs: &[Glyph]) {
        for g in glyphs {
            let sg = g.as_stroke().unwrap();
            let bb = sg.bounding_box();
            self.s("\ng");
            self.d(bb.x());
            self.d(bb.y());
            self.d(bb.width());
            self.d(bb.height());
            self.i(g.is_hover() as i64);

            for pl in &sg.strokes {
                self.s("\ns");

                for p in pl {
                    self.d(p.x);
                    self.d(p.y);
                }
            }
        }
    }
    fn node(&mut self, n: &MarkupNode) {
        self.s("(");
        self.s(if n.is_root() { "root" } else { n.type_string() });

        if let Some(c) = n.content() {
            self.hex(c);
        }

        for c in &n.children {
            self.node(c);
        }

        self.s(")");
    }
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;

    for &c in s.as_bytes() {
        h ^= c as u64;
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

/// The `RECORDING_GAL` of the C++ driver.
struct RecordingGal<'a> {
    out: &'a mut Out,
    line_width: f32,
}

impl TextGal for RecordingGal<'_> {
    fn set_line_width(&mut self, w: f32) {
        self.line_width = w;
    }

    fn line_width(&self) -> f32 {
        self.line_width
    }

    fn draw_glyph(&mut self, glyph: &Glyph, nth: i32, total: i32) {
        self.out.s("\nG");
        self.out.i(nth as i64);
        self.out.i(total as i64);
        // the C++ prints a copy (copy constructor, which resets the hover flag)
        #[allow(clippy::cloned_ref_to_slice_refs)]
        self.out.glyphs(&[glyph.clone()]);

        if let Glyph::Stroke(g) = glyph {
            for pl in &g.strokes {
                for ii in 1..pl.len() {
                    let a = to_vector2i(pl[ii - 1]);
                    let b = to_vector2i(pl[ii]);
                    self.out.s("\nL");
                    self.out.i(a.x as i64);
                    self.out.i(a.y as i64);
                    self.out.i(b.x as i64);
                    self.out.i(b.y as i64);
                }
            }
        }
    }
}

fn case_markup(g: &mut Gen, o: &mut Out) {
    let text = g.gen_text(30);

    o.s("M");
    o.hex(&text);
    o.s("\n");

    match MarkupParser::new(&text).parse() {
        Some(root) => o.node(&root),
        None => o.s("null"),
    }
}

fn case_glyphs(g: &mut Gen, o: &mut Out) {
    let font = default_font();
    let text = g.gen_text(12);
    let text = g.no_tabs_if_extreme(text);
    let sx = g.gen_size();
    let sy = if g.rnd(4) == 0 { g.gen_size() } else { sx };
    let px = g.gen_coord();
    let py = g.gen_coord();
    let pos = Vector2I::new(px, py);
    let angle = g.gen_angle();
    let mirror = g.rnd(2) != 0;
    let mut origin = pos;

    if g.rnd(2) == 0 {
        let ox = g.gen_coord();
        let oy = g.gen_coord();
        origin = Vector2I::new(ox, oy);
    }

    let style = g.rnd(64) as u32;

    o.s("T");
    o.hex(&text);
    o.i(sx as i64);
    o.i(sy as i64);
    o.i(pos.x as i64);
    o.i(pos.y as i64);
    o.d(angle);
    o.i(mirror as i64);
    o.i(origin.x as i64);
    o.i(origin.y as i64);
    o.i(style as i64);

    let mut glyphs = Vec::new();
    let mut bbox = Box2I::default();
    let end = font.text_as_glyphs(
        Some(&mut bbox),
        Some(&mut glyphs),
        &text,
        Vector2I::new(sx, sy),
        pos,
        EdaAngle::from_degrees(angle),
        mirror,
        origin,
        style,
    );

    o.s("\nR");
    o.i(end.x as i64);
    o.i(end.y as i64);
    o.bbox(&bbox);
    o.glyphs(&glyphs);
}

fn case_draw(g: &mut Gen, o: &mut Out) {
    let font = default_font();
    let metrics = Metrics::default();
    let mut text = String::new();
    let lines = 1 + g.rnd(3);

    for l in 0..lines {
        if l != 0 {
            text.push('\n');
        }

        text.push_str(&g.gen_text(10));
    }

    let text = g.no_tabs_if_extreme(text);
    let mut attrs = TextAttributes::default();
    let sx = if g.rnd(8) == 0 {
        g.gen_size()
    } else {
        (500000 + g.rnd(4500001)) as i32
    };
    let sy = if g.rnd(4) == 0 {
        (500000 + g.rnd(4500001)) as i32
    } else {
        sx
    };
    const SPACING: [f64; 4] = [1.0, 1.0, 1.5, 0.8];
    let sw = g.rnd(4);

    attrs.size = (sx, sy);
    let angle = g.gen_angle();
    attrs.set_angle(angle);
    attrs.halign = match g.rnd(3) - 1 {
        -1 => GrTextHAlignT::Left,
        0 => GrTextHAlignT::Center,
        _ => GrTextHAlignT::Right,
    };
    attrs.valign = match g.rnd(3) - 1 {
        -1 => GrTextVAlignT::Top,
        0 => GrTextVAlignT::Center,
        _ => GrTextVAlignT::Bottom,
    };
    attrs.mirrored = g.rnd(2) != 0;
    attrs.italic = g.rnd(2) != 0;
    attrs.bold = g.rnd(2) != 0;
    attrs.underlined = g.rnd(4) == 0;
    attrs.stroke_width = if sw == 0 {
        0
    } else if sw == 1 {
        1
    } else {
        (50000 + g.rnd(550001)) as i32
    };
    attrs.line_spacing = if g.rnd(3) != 0 {
        SPACING[g.rnd(4) as usize]
    } else {
        g.rnd(300) as f64 / 100.0
    };

    if g.extreme && g.rnd(2) == 0 {
        attrs.stroke_width = g.gen_extreme_int();
    }

    if g.extreme && g.rnd(2) == 0 {
        const LS: [f64; 8] = [
            1e9,
            3e9,
            -3e9,
            1e300,
            -1e300,
            f64::INFINITY,
            f64::NAN,
            20000.0,
        ];
        attrs.line_spacing = LS[g.rnd(8) as usize];
    }

    let px = g.gen_coord();
    let py = g.gen_coord();
    let pos = Vector2I::new(px, py);
    let mut cursor = Vector2I::new(0, 0);

    if g.rnd(4) == 0 {
        let cx = (g.rnd(2000001) - 1000000) as i32;
        let cy = (g.rnd(2000001) - 1000000) as i32;
        cursor = Vector2I::new(cx, cy);
    }

    let column = g.rnd(50000001) as i32;
    let size = Vector2I::new(sx, sy);

    o.s("D");
    o.hex(&text);
    o.i(sx as i64);
    o.i(sy as i64);
    o.d(attrs.angle);
    o.i(attrs.halign as i64);
    o.i(attrs.valign as i64);
    o.i(attrs.mirrored as i64);
    o.i(attrs.italic as i64);
    o.i(attrs.bold as i64);
    o.i(attrs.underlined as i64);
    o.i(attrs.stroke_width as i64);
    o.d(attrs.line_spacing);
    o.i(pos.x as i64);
    o.i(pos.y as i64);
    o.i(cursor.x as i64);
    o.i(cursor.y as i64);
    o.i(column as i64);

    let (line_strs, positions, extents) = font.line_positions(
        &text,
        Vector2I::new(pos.x.wrapping_sub(cursor.x), pos.y.wrapping_sub(cursor.y)),
        &attrs,
        &metrics,
    );

    for i in 0..line_strs.len() {
        o.s("\nP");
        o.hex(&line_strs[i]);
        o.i(positions[i].x as i64);
        o.i(positions[i].y as i64);
        o.i(extents[i].x as i64);
        o.i(extents[i].y as i64);
    }

    let line_width = {
        let mut gal = RecordingGal {
            out: o,
            line_width: 1.0,
        };
        font.draw_with_cursor(&mut gal, &text, pos, cursor, &attrs, &metrics, None, None);
        gal.line_width
    };
    o.s("\nW");
    o.f(line_width);

    let limits = font.string_boundary_limits(
        &text,
        size,
        attrs.stroke_width,
        attrs.bold,
        attrs.italic,
        &metrics,
    );
    o.s("\nS");
    o.i(limits.x as i64);
    o.i(limits.y as i64);

    let aligned = font.aligned_draw_position(&text, pos, &attrs, &metrics);
    o.s("\nA");
    o.i(aligned.x as i64);
    o.i(aligned.y as i64);
    o.d(font.interline(sy as f64, &metrics));

    if !text.contains("@{") {
        o.s("\nX");
        o.i(ki_round(gr_text_width(
            &text,
            Some(font),
            size,
            attrs.stroke_width,
            attrs.bold,
            attrs.italic,
            &metrics,
        ) as f64) as i64);
    }

    let mut broken = text.clone();
    font.linebreak_text(
        &mut broken,
        column,
        size,
        attrs.stroke_width,
        attrs.bold,
        attrs.italic,
    );
    o.s("\nK");
    o.hex(&broken);
}

fn case_helpers(g: &mut Gen, o: &mut Out) {
    let a = (g.rnd(20000001) - 1000000) as i32;
    let b = (g.rnd(20000001) - 1000000) as i32;
    let c = (g.rnd(20000001) - 1000000) as i32;
    let strict = g.rnd(2) != 0;
    let f = (g.rnd(20000001) - 1000000) as f32 / 7.0f32;

    o.s("H");
    o.i(a as i64);
    o.i(b as i64);
    o.i(c as i64);
    o.i(strict as i64);
    o.f(f);
    o.s("\n");
    o.i(get_pen_size_for_bold(a) as i64);
    o.i(get_pen_size_for_bold_size(Vector2I::new(a, b)) as i64);
    o.i(get_pen_size_for_demi_bold(a) as i64);
    o.i(get_pen_size_for_demi_bold_size(Vector2I::new(a, b)) as i64);
    o.i(get_pen_size_for_normal(a) as i64);
    o.i(get_pen_size_for_normal_size(Vector2I::new(a, b)) as i64);
    o.i(clamp_text_pen_size(a, b, strict) as i64);
    o.f(clamp_text_pen_size_f32(f, b, strict));
    o.i(clamp_text_pen_size_vec(a, Vector2I::new(b, c), strict) as i64);
    o.i(get_knockout_text_margin(Vector2I::new(b, c), a) as i64);
}

fn table_lines() -> Vec<String> {
    let font = default_font();
    let mut lines = Vec::new();
    let mut all = String::new();

    for s in NEWSTROKE_FONT.iter() {
        all.push_str(s);
        all.push('\n');
    }

    lines.push(format!(
        "COUNT {} {}",
        NEWSTROKE_FONT.len(),
        font.glyph_count()
    ));
    lines.push(format!("STRINGS {:016x}", fnv(&all)));

    let count = font.glyph_count();
    let mut block = 0;

    while block * 1024 < count {
        let mut o = Out::default();

        for i in block * 1024..std::cmp::min(count, (block + 1) * 1024) {
            let bb = font.glyph_bounding_box(i);
            o.s("b");
            o.d(bb.x());
            o.d(bb.y());
            o.d(bb.width());
            o.d(bb.height());

            for pl in &font.glyph(i).unwrap().strokes {
                o.s("\ns");

                for p in pl {
                    o.d(p.x);
                    o.d(p.y);
                }
            }

            o.s("\n");
        }

        lines.push(format!("BLOCK {} {:016x}", block, fnv(&o.s)));
        block += 1;
    }

    lines
}

#[test]
fn font_oracle_golden() {
    let golden = include_str!("golden/font_oracle.txt");
    let mut expected = golden.lines();
    let mut mismatches = 0;
    let mut cases = 0;

    for line in table_lines() {
        let exp = expected.next().unwrap();
        assert_eq!(line, exp, "glyph table mismatch");
    }

    type CaseFn = fn(&mut Gen, &mut Out);
    let kinds: [(char, CaseFn); 6] = [
        ('M', case_markup),
        ('T', case_glyphs),
        ('D', case_draw),
        ('H', case_helpers),
        ('O', case_glyphs),
        ('E', case_draw),
    ];

    for exp in expected {
        let mut parts = exp.split(' ');
        let tag = parts.next().unwrap().chars().next().unwrap();
        let idx: u64 = parts.next().unwrap().parse().unwrap();
        let hash = parts.next().unwrap();
        let f = kinds.iter().find(|k| k.0 == tag).unwrap().1;
        let mut g = Gen {
            state: idx.wrapping_mul(1000003).wrapping_add(tag as u64),
            extreme: tag == 'O' || tag == 'E',
        };
        let mut o = Out::default();

        f(&mut g, &mut o);
        cases += 1;

        if format!("{:016x}", fnv(&o.s)) != hash {
            mismatches += 1;

            if mismatches <= 3 {
                eprintln!("MISMATCH {tag} {idx}\n{}\n", o.s);
            }
        }
    }

    assert_eq!(mismatches, 0, "{mismatches} of {cases} oracle cases differ");
    assert!(cases >= 8000);
}
