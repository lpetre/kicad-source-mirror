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

//! Differential test of work package C2 against the C++ implementation.
//!
//! `tests/oracle/units_strings_oracle.cpp` calls the original functions of `libkicommon.so`
//! (`eda_units.cpp`, `string_utils.cpp`, `color4d.cpp`, `page_info.cpp`, `title_block.cpp`)
//! on about 150 000 deterministic edge-case and pseudo-random inputs.  The golden file
//! `tests/golden/c2_units_strings.txt` keeps, per section, the first lines verbatim and the
//! line count and FNV-1a hash of all lines.  This test produces the same lines with the port;
//! [`dump`] mirrors `main()` of the C++ program line by line, including the order of the
//! pseudo-random draws.  On a mismatch the Rust lines are written to
//! `target/core-oracle/rust_full.txt` for a diff against `units_strings_oracle full`.

use std::collections::BTreeMap;

use kicad_core::base_units::*;
use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::eda_units::{self, EdaDataType, EdaUnits, ui};
use kicad_core::minoptmax::MinOptMax;
use kicad_core::page_info::{CustomPageSize, PageInfo, PageSizeType};
use kicad_core::string_utils::*;
use kicad_core::title_block::TitleBlock;
use kicad_sexpr::{OutputFormatter, StringFormatter};

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

    fn u64(&mut self) -> u64 {
        let a = self.next();
        let b = self.next();
        let c = self.next();
        (a << 42) ^ (b << 21) ^ c
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
}

const P10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

#[allow(clippy::excessive_precision)]
const SPECIAL_D: [f64; 20] = [
    0.0,
    -0.0,
    1e-4,
    -1e-4,
    1.0000000000000002e-4,
    9.9999e-5,
    0.5,
    2.5,
    -2.5,
    1e16,
    123456789.0,
    1e-300,
    5e-324,
    1.7976931348623157e308,
    0.1,
    0.3,
    25.4,
    0.0254,
    1e10,
    99999.99999,
];

fn rdouble(g: &mut Lcg) -> f64 {
    match g.below(6) {
        0 => f64::from_bits(g.u64()),
        1 => {
            let v = (g.next() as i64 - 1073741824) as f64;
            v / P10[g.below(23) as usize]
        }
        2 => {
            let v = (g.next() as i64 - 1073741824) as f64;
            v * P10[g.below(16) as usize]
        }
        3 => {
            let v = (g.below(2001) as i64 - 1000) as f64;
            v / P10[g.below(12) as usize]
        }
        4 => SPECIAL_D[g.below(SPECIAL_D.len() as u32) as usize],
        _ => {
            let v = (g.next() as i64 - 1073741824) as f64 + 0.5;
            v / P10[g.below(8) as usize]
        }
    }
}

const SPECIAL_I: [i32; 12] = [
    0,
    1,
    -1,
    i32::MAX,
    i32::MIN,
    100,
    50,
    -50,
    1000000,
    25400,
    127,
    5,
];
const MULT_I: [i32; 8] = [1, 10, 100, 1000, 25400, 127, 254000, 2540];

fn rint(g: &mut Lcg) -> i32 {
    match g.below(4) {
        0 => g.u64() as u32 as i32,
        1 => g.below(2001) as i32 - 1000,
        2 => {
            let v = g.below(20001) as i32 - 10000;
            v.wrapping_mul(MULT_I[g.below(8) as usize])
        }
        _ => SPECIAL_I[g.below(SPECIAL_I.len() as u32) as usize],
    }
}

fn rstring(g: &mut Lcg, frags: &[&str], max_parts: u32) -> String {
    let n = g.below(max_parts + 1);
    let mut s = String::new();

    for _ in 0..n {
        s.push_str(frags[g.below(frags.len() as u32) as usize]);
    }

    s
}

// ---- output ----

fn esc_bytes(s: &[u8]) -> String {
    let mut out = String::new();

    for &c in s {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            c if c < 0x20 || c >= 0x7F => out.push_str(&format!("\\x{c:02X}")),
            c => out.push(c as char),
        }
    }

    out
}

fn u(s: &str) -> String {
    esc_bytes(s.as_bytes())
}

fn h(d: f64) -> String {
    format!("{:016x}", d.to_bits())
}

fn b(v: bool) -> String {
    (v as i32).to_string()
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

const SCALES: [&EdaIuScale; 5] = [
    &PCB_IU_SCALE,
    &SCH_IU_SCALE,
    &GERB_IU_SCALE,
    &DRAW_SHEET_IU_SCALE,
    &UNITY_SCALE,
];

fn units(i: i32) -> EdaUnits {
    EdaUnits::ALL[i as usize]
}

fn dtype(i: i32) -> EdaDataType {
    EdaDataType::ALL[i as usize]
}

fn dump() -> Out {
    let mut o = Out::default();
    let mut g = Lcg(0x5eed1234abcd);

    // 1. FormatInternalUnits / FormatAngle / FormatDouble2Str / UIDouble2Str
    o.begin("fiu");

    for (si, scale) in SCALES.iter().enumerate() {
        for t in 0..7 {
            for _ in 0..400 {
                let v = rint(&mut g);
                o.out(format!(
                    "{si} {t} {v} {}",
                    eda_units::format_internal_units(scale, v, dtype(t))
                ));
            }
        }

        for _ in 0..200 {
            let x = rint(&mut g);
            let y = rint(&mut g);
            o.out(format!(
                "{si} P {}",
                eda_units::format_internal_units_point(scale, x, y)
            ));
        }
    }

    o.begin("angle");

    for _ in 0..4000 {
        let d = rdouble(&mut g);
        o.out(format!(
            "{} {} {} {}",
            h(d),
            eda_units::format_angle(d),
            u(&ui::message_text_from_angle(d, true)),
            u(&ui::message_text_from_angle(d, false))
        ));
    }

    o.begin("double2str");

    for _ in 0..12000 {
        let d = rdouble(&mut g);
        o.out(format!(
            "{} {} {}",
            h(d),
            format_double2_str(d),
            ui_double2_str(d)
        ));
    }

    o.begin("trimmed");

    for _ in 0..4000 {
        let d = rdouble(&mut g);
        let p = g.below(12) as i32 - 1;
        o.out(format!("{} {p} {}", h(d), u(&format_trimmed_decimal(d, p))));
    }

    // 2. StringFromValue / MessageTextFromValue
    o.begin("sfv");

    for (si, scale) in SCALES.iter().enumerate() {
        for un in 0..=12 {
            for t in 0..7 {
                for k in 0..24 {
                    let d = if k & 1 == 1 {
                        rint(&mut g) as f64
                    } else {
                        rdouble(&mut g)
                    };
                    let add = g.below(2) == 1;
                    o.out(format!(
                        "{si} {un} {t} {} {} {} {}",
                        b(add),
                        h(d),
                        u(&ui::string_from_value(scale, units(un), d, add, dtype(t))),
                        u(&ui::message_text_from_value(
                            scale,
                            units(un),
                            d,
                            add,
                            dtype(t)
                        ))
                    ));
                }
            }
        }
    }

    o.begin("mtfv_int");

    for (si, scale) in SCALES.iter().enumerate() {
        for un in 0..=12 {
            for _ in 0..40 {
                let v = rint(&mut g);
                let l1 = rint(&mut g) as i64;
                let l2 = rint(&mut g) as i64;
                let l = l1 * l2;
                o.out(format!(
                    "{si} {un} {v} {l} {} {}",
                    u(&ui::message_text_from_value_int(
                        scale,
                        units(un),
                        v,
                        true,
                        EdaDataType::Distance
                    )),
                    u(&ui::message_text_from_value_i64(
                        scale,
                        units(un),
                        l,
                        true,
                        EdaDataType::Distance
                    ))
                ));
            }

            for _ in 0..8 {
                let mut m: MinOptMax<i32> = MinOptMax::new();
                let mask = g.below(8);
                let a = rint(&mut g);
                let bb = rint(&mut g);
                let c = rint(&mut g);

                if mask & 1 != 0 {
                    m.set_min(a);
                }

                if mask & 2 != 0 {
                    m.set_opt(bb);
                }

                if mask & 4 != 0 {
                    m.set_max(c);
                }

                o.out(format!(
                    "{si} {un} M {mask} {a} {bb} {c} {}",
                    u(&ui::message_text_from_min_opt_max(scale, units(un), &m))
                ));
            }
        }
    }

    o.begin("userunit");

    for (si, scale) in SCALES.iter().enumerate() {
        for un in 0..=12 {
            for _ in 0..30 {
                let d = rdouble(&mut g);
                o.out(format!(
                    "{si} {un} {} {} {}",
                    h(d),
                    h(ui::to_user_unit(scale, units(un), d)),
                    h(ui::from_user_unit(scale, units(un), d))
                ));
            }
        }
    }

    for _ in 0..400 {
        let d = rdouble(&mut g);
        o.out(format!(
            "mm {} {} {}",
            h(d),
            eda_units::mm2mils(d),
            eda_units::mils2mm(d)
        ));
    }

    for un in 0..=12 {
        for t in 0..7 {
            o.out(format!(
                "txt {un} {t} {}|{}|{}{}",
                u(&eda_units::get_text(units(un), dtype(t))),
                u(&eda_units::get_label(units(un), dtype(t))),
                b(eda_units::is_imperial_unit(units(un))),
                b(eda_units::is_metric_unit(units(un)))
            ));
        }
    }

    // 3. Parsing of user values
    let num_frags = [
        "0", "1", "2", "5", "9", "12", ".", ",", "-", "+", " ", "  ", "mm", "um", "\u{b5}m",
        "\u{3bc}m", "cm", "mil", "mils", "thou", "th", "mi", "in", "\"", "oz", "deg", "rad", "ra",
        "de", "fs", "ps", "ps/in", "ps/cm", "ps/mm", "PS/MM", "Mm", "IN", "x", "e5", "\t", "1e3",
    ];

    o.begin("parse");

    for _ in 0..6000 {
        let s = rstring(&mut g, &num_frags, 6);
        let si = g.below(5) as usize;
        let un = g.below(13) as i32;
        let t = g.below(7) as i32;
        let scale = SCALES[si];

        let fu = eda_units::fetch_units_from_string(&s);
        let dv = ui::double_value_from_string_with_units(scale, &s);

        o.out(format!(
            "{}|{si} {un} {t} {} {} {} {} {} {} {} {}",
            u(&s),
            b(fu.is_some()),
            fu.unwrap_or(EdaUnits::Unscaled) as i32,
            h(ui::double_value_from_string(scale, units(un), &s, dtype(t))),
            h(ui::double_value_from_string_unscaled(&s)),
            b(dv.is_some()),
            h(dv.unwrap_or(12345.0)),
            ui::value_from_string(scale, units(un), &s, dtype(t)),
            ui::value_from_string_unscaled(&s)
        ));
    }

    // 4. EscapeString / UnescapeString
    let esc_frags = [
        "a",
        "b",
        "c",
        "Z",
        "0",
        "9",
        " ",
        "/",
        "\\",
        "<",
        ">",
        ":",
        "\"",
        "'",
        ",",
        "|",
        "{",
        "}",
        "$",
        "~",
        "^",
        "_",
        "\n",
        "\r",
        "\t",
        ".",
        "(",
        ")",
        "-",
        "[",
        "]",
        "+",
        "*",
        "?",
        "\x7F",
        "\u{b5}",
        "\u{3bc}",
        "\u{20ac}",
        "\u{1f600}",
        "\u{e9}",
        "\u{df}",
        "\u{3a9}",
        "\u{2018}",
        "\u{201d}",
        "\u{2013}",
        "\u{2014}",
        "{slash}",
        "{dblquote}",
    ];

    o.begin("escape");

    for _ in 0..20000 {
        let s = rstring(&mut g, &esc_frags, 12);
        let ctx = g.below(11) as i32;
        let e = escape_string(&s, EscapeContext::ALL[ctx as usize]);
        o.out(format!(
            "{ctx} {}|{}|{}",
            u(&s),
            u(&e),
            u(&unescape_string(&e))
        ));
    }

    let unesc_frags = [
        "{slash}",
        "{dblquote}",
        "{quote}",
        "{lt}",
        "{gt}",
        "{backslash}",
        "{bar}",
        "{comma}",
        "{colon}",
        "{space}",
        "{dollar}",
        "{tab}",
        "{return}",
        "{brace}",
        "{unknown}",
        "{",
        "}",
        "{{",
        "}}",
        "$",
        "~",
        "^",
        "_",
        "a",
        "x",
        " ",
        "{slash",
        "\\",
        "\u{b5}",
        "${VAR}",
        "~{over}",
        "{}",
    ];

    o.begin("unescape");

    for _ in 0..20000 {
        let s = rstring(&mut g, &unesc_frags, 10);
        o.out(format!("{}|{}", u(&s), u(&unescape_string(&s))));
    }

    // 5. Other string functions
    o.begin("strmisc");

    let cmp_frags = [
        "a",
        "A",
        "b",
        "B",
        "0",
        "1",
        "01",
        "9",
        "10",
        "100",
        "2147483648",
        "99999999999",
        " ",
        ".",
        "-",
        ",",
        "U",
        "u",
        "\u{e9}",
        "\u{c9}",
        "\u{df}",
        "_",
        "z",
    ];

    for _ in 0..6000 {
        let a = rstring(&mut g, &cmp_frags, 5);
        let bb = rstring(&mut g, &cmp_frags, 5);
        o.out(format!(
            "cmp {}|{}|{} {} {} {}",
            u(&a),
            u(&bb),
            str_num_cmp(&a, &bb, CmpT::CaseSensitive),
            str_num_cmp(&a, &bb, CmpT::IgnoreCase),
            sort_variant_names(&a, &bb),
            get_trailing_int(&a)
        ));
    }

    let val_frags = [
        "1", "0", "2", "5", "9", "47", ".", ",", "K", "k", "R", "u", "\u{b5}", "\u{3bc}", "n", "p",
        "F", "M", "G", "T", "m", "L", "\u{3a9}", "V", "A", "H", "s", "Hz", "hz", " ", "{slash}",
        "a", "X", "W", "mils", "in", "E", "P", "f", "c", "000", "1,000",
    ];

    for _ in 0..8000 {
        let a = rstring(&mut g, &val_frags, 5);
        let bb = rstring(&mut g, &val_frags, 5);
        let (beg, dig, end) = split_string(&a);
        let q = quacks_like_a_duck(&a);
        o.out(format!(
            "val {}|{}|{} {}|{}|{}|{} {}",
            u(&a),
            u(&bb),
            value_string_compare(&a, &bb),
            u(&beg),
            u(&dig),
            u(&end),
            b(q.is_some()),
            q.map_or("-".to_string(), h)
        ));
    }

    let wild_p = ["a", "b", "*", "?", "A", "\u{e9}"];
    let wild_s = ["a", "b", "A", "B", "\u{c9}"];

    for _ in 0..4000 {
        let p = rstring(&mut g, &wild_p, 6);
        let s = rstring(&mut g, &wild_s, 6);
        o.out(format!(
            "wild {}|{}|{}{}",
            u(&p),
            u(&s),
            b(wild_compare_string(&p, &s, true)),
            b(wild_compare_string(&p, &s, false))
        ));
    }

    let text_frags = [
        "~",
        "~~",
        "{",
        "}",
        " ",
        ")",
        "a",
        "B",
        "^",
        "_",
        "\t",
        "x y",
        "\u{e9}",
        "\u{df}",
        "hELLO",
        "\u{2018}",
        "\u{201c}",
        "\u{2014}",
        "\"",
        "\\",
        "\n",
        "*",
        "?",
        "/",
        ":",
        "<",
        "&",
        ";",
        "#",
        "&amp;",
        "&lt;",
        "&#65;",
        "&#x20AC;",
        "&#x0x12;",
        "&#;",
        "&quot;",
        "&apos;",
        "&gt;",
        "&#xzz12;",
        "&#99999999999999999999;",
        ">",
    ];

    for _ in 0..8000 {
        let s = rstring(&mut g, &text_frags, 8);
        let mut sq = s.clone();
        let sqc = convert_smart_quotes_and_dashes(&mut sq);
        let mut ill = s.clone();
        let illc = replace_illegal_file_name_chars(&mut ill, '\0');
        let mut ill2 = s.clone();
        replace_illegal_file_name_chars(&mut ill2, '_');
        let mut ills = s.clone();
        let illsc = replace_illegal_file_name_chars_std(&mut ills, 0);
        o.out(format!(
            "txt {}|{}|{}|{}|{}|{}|{}|{}|{}|{}{}{}{}{}|{}|{}|{}|{}|{}",
            u(&s),
            u(&convert_to_new_overbar_notation(&s)),
            u(&title_caps(&s)),
            u(&initial_caps(&s)),
            u(&escaped_utf8(&s)),
            u(&escape_html(&s)),
            u(&unescape_html(&s)),
            u(&remove_html_tags(&s)),
            printable_char_count(&s),
            b(no_printable_chars(&s)),
            b(is_full_file_name_valid(&s)),
            b(sqc),
            b(illc),
            b(illsc),
            u(&sq),
            u(&ill),
            u(&ill2),
            u(&ills),
            u(&get_variant_file_token(&s))
        ));
    }

    let delim_frags = ["\"", "\\", "a", "b", " ", "\u{e9}", "\\\"", "\\\\", "n"];

    for _ in 0..3000 {
        let src = rstring(&mut g, &delim_frags, 8);
        let size = g.below(12) as i32 - 1;
        let (dest, n) = read_delimited_text(src.as_bytes());
        let (buf, n2) = read_delimited_text_buf(src.as_bytes(), size);
        o.out(format!(
            "delim {}|{}|{n}|{size} {}|{n2}",
            u(&src),
            u(&dest),
            esc_bytes(&buf)
        ));
    }

    let zero_frags = ["0", "1", ".", "00", "5", "-"];

    for _ in 0..2000 {
        let s = rstring(&mut g, &zero_frags, 6);
        let allowed = g.below(4);
        let mut z = s.clone();
        strip_trailing_zeros(&mut z, allowed);
        o.out(format!("zeros {} {allowed} {}", u(&s), u(&z)));
    }

    let pin_frags = [
        "[",
        "]",
        ",",
        "-",
        "\\",
        "A",
        "B",
        "1",
        "2",
        "10",
        " ",
        "a",
        "99999999999999999999",
    ];

    for _ in 0..6000 {
        let s = rstring(&mut g, &pin_frags, 8);
        let (pins, valid) = expand_stacked_pin_notation(&s);
        let (count, cvalid) = count_stacked_pin_notation(&s);
        let mut line = format!("pins {}|{} {count} {}", u(&s), b(valid), b(cvalid));

        for p in pins.iter().take(20) {
            line += &format!("|{}", u(p));
        }

        line += &format!("|{}|{}", pins.len(), u(&escape_stacked_pin_item(&s)));

        for item in split_stacked_pin_display_items(&s) {
            line += &format!("|{}", u(&item));
        }

        o.out(line);
    }

    // 6. COLOR4D
    o.begin("color");

    let col_frags = [
        "rgb", "RGBA", "rgba", "Rgb", "(", ")", ",", " ", "0", "255", "300", "-5", "0.5", "1",
        "128", "#", "ff", "FF", "00", "12", "abc", "g", "1e3", ".", "0x", "-", "  ", "\t",
    ];

    for k in 0..8000 {
        let s = if k % 3 == 0 {
            "#".to_string() + &rstring(&mut g, &col_frags, 4)
        } else {
            rstring(&mut g, &col_frags, 9)
        };
        let mut a = Color4D::default();
        let wok = a.set_from_wx_string(&s);
        let mut bb = Color4D::default();
        let hok = bb.set_from_hex_string(&s);
        let c = Color4D::from_string(&s);
        o.out(format!(
            "{}|{} {}{}{}{} {} {}{}{}{} {}{}{}",
            u(&s),
            b(wok),
            h(a.r),
            h(a.g),
            h(a.b),
            h(a.a),
            b(hok),
            h(bb.r),
            h(bb.g),
            h(bb.b),
            h(bb.a),
            b(c.text.is_some()),
            h(c.r),
            h(c.a)
        ));
    }

    for _ in 0..6000 {
        let r = g.below(1001) as f64 / 1000.0;
        let gg = g.below(1001) as f64 / 1000.0;
        let bl = g.below(1001) as f64 / 1000.0;
        let a = g.below(1001) as f64 / 1000.0;
        let f = g.below(101) as f64 / 100.0;
        let c = Color4D::new(r, gg, bl, a);
        let (hh, s, v) = c.to_hsv(false);
        let (h2, s2, l) = c.to_hsl();
        let mut from_hsv = Color4D::default();
        from_hsv.from_hsv(h2, s2, l);
        let mut from_hsl = Color4D::default();
        from_hsl.from_hsl(h2, s2, l);
        let mut sat = c.clone();
        sat.saturate(f);
        let mut desat = c.clone();
        desat.desaturate();
        let other = Color4D::new(a, r, f, gg);
        let mix = c.legacy_mix(&other);
        let wc = c.to_colour();
        o.out(format!(
            "{}{}{}{} {} {}{}{} {}{}{} {}{}{} {}{}{} {}{}{} {}{}{} {}{}{}{} {} {} {}{} {} {}{}{} {} {} {} {}{}{}",
            h(r),
            h(gg),
            h(bl),
            h(a),
            h(f),
            h(hh),
            h(s),
            h(v),
            h(h2),
            h(s2),
            h(l),
            h(from_hsv.r),
            h(from_hsv.g),
            h(from_hsv.b),
            h(from_hsl.r),
            h(from_hsl.g),
            h(from_hsl.b),
            h(sat.r),
            h(sat.g),
            h(sat.b),
            h(desat.r),
            h(desat.g),
            h(desat.b),
            h(mix.r),
            h(mix.g),
            h(mix.b),
            h(mix.a),
            u(&c.to_css_string()),
            u(&c.to_hex_string()),
            wc.red,
            wc.alpha,
            h(c.distance(&other)),
            c.compare(&other),
            b(c.less_than(&other)),
            b(c == other),
            h(c.relative_luminance()),
            h(Color4D::contrast_ratio(&c, &other)),
            h(c.brightness()),
            h(c.mix(&other, f).r),
            h(c.brightened(f).g),
            h(c.darkened(f).b)
        ));
    }

    for _ in 0..3000 {
        let r = g.below(300) as i32 - 20;
        let gg = g.below(300) as i32 - 20;
        let bl = g.below(300) as i32 - 20;
        o.out(format!(
            "near {r} {gg} {bl} {}",
            Color4D::find_nearest_legacy_color(r, gg, bl).0
        ));
    }

    for i in -2..=36 {
        let c = Color4D::from_eda_color(EdaColorT(i));
        o.out(format!("eda {i} {}{}{}{}", h(c.r), h(c.g), h(c.b), h(c.a)));
    }

    // 7. PAGE_INFO / TITLE_BLOCK
    o.begin("page");

    let custom = CustomPageSize::default();

    for t in 0..=15 {
        for portrait in 0..2 {
            let p = PageInfo::new(PageSizeType::ALL[t], portrait == 1, &custom);
            let mut f = StringFormatter::new();
            p.format(&mut f).unwrap();
            o.out(format!(
                "{t} {portrait} {} {} {} {}{}{} {} {} {} {} {}",
                p.type_as_string(),
                h(p.width_mils()),
                h(p.height_mils()),
                b(p.is_portrait()),
                b(p.is_custom()),
                b(p.is_default()),
                p.paper_id() as i32,
                u(p.page_format_description()),
                p.width_iu(1000.0),
                p.height_iu(25.4),
                f.string()
            ));
        }
    }

    let names = [
        "A4", "a4", "USLETTER", "user", "User", "GERBER", "A6", "", "a",
    ];

    for _ in 0..400 {
        let mut p = PageInfo::default();
        let name = names[g.below(names.len() as u32) as usize];
        let portrait = g.below(2) == 1;
        let ok = p.set_type_by_name(name, portrait, &custom);
        let w = rdouble(&mut g);
        let hh = rint(&mut g) as f64;

        if w.is_finite() && w.abs() < 1e9 && g.below(2) == 1 {
            p.set_width_mils(w);
        }

        if g.below(2) == 1 {
            p.set_height_mils(hh);
        }

        if g.below(3) == 0 {
            let flipped = !p.is_portrait();
            p.set_portrait(flipped);
        }

        let mut f = StringFormatter::new();
        p.format(&mut f).unwrap();
        o.out(format!(
            "{} {} {} {} {} {} {} {} {} {}",
            u(name),
            b(portrait),
            b(ok),
            h(w),
            h(hh),
            p.type_as_string(),
            h(p.width_mils()),
            h(p.height_mils()),
            b(p.is_portrait()),
            f.string()
        ));
    }

    let tb_frags = ["a", "\"", "\\", "\n", "\u{e9}", " ", "x"];

    for _ in 0..300 {
        let mut tb = TitleBlock::new();
        let mask = g.below(1 << 13);
        let texts: Vec<String> = (0..13).map(|_| rstring(&mut g, &tb_frags, 3)).collect();

        if mask & 1 != 0 {
            tb.set_title(&texts[0]);
        }

        if mask & 2 != 0 {
            tb.set_date(&texts[1]);
        }

        if mask & 4 != 0 {
            tb.set_revision(&texts[2]);
        }

        if mask & 8 != 0 {
            tb.set_company(&texts[3]);
        }

        for i in 0..9 {
            if mask & (16 << i) != 0 {
                tb.set_comment(i, &texts[4 + i]);
            }
        }

        let mut f = StringFormatter::new();
        tb.format(&mut f).unwrap();
        o.out(format!("tb {mask} {}", u(f.string())));
    }

    o
}

#[test]
fn c2_oracle_golden() {
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/c2_units_strings.txt"
    );
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

        for (i, exp) in expected_lines.get(name).into_iter().flatten().enumerate() {
            if lines.get(i) != Some(exp) {
                failures.push(format!(
                    "{name} line {i}:\n  C++ : {exp}\n  Rust: {:?}",
                    lines.get(i)
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
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/core-oracle");
        let _ = std::fs::create_dir_all(dir);
        let full: String = o
            .sections
            .iter()
            .flat_map(|(name, lines)| lines.iter().map(move |l| format!("{name} {l}\n")))
            .collect();
        let _ = std::fs::write(format!("{dir}/rust_full.txt"), full);

        panic!(
            "{} mismatches (diff target/core-oracle/rust_full.txt against `units_strings_oracle full`):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

#[test]
fn quotew_is_used_for_title_block() {
    // Guards the OutputFormatter import used above.
    let f = StringFormatter::new();
    assert_eq!(f.quotew("a b"), "\"a b\"");
}
