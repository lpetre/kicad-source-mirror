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

//! Differential test of `kicad_core::json` against the bundled nlohmann/json 3.11.3.
//!
//! `tests/oracle/json_oracle.cpp` (built by `tests/oracle/build_c3_oracle.sh`) parses and
//! re-dumps every `.kicad_pro`, `.kicad_prl`, `.json` and `.gbrjob` file under `qa/data` and
//! `demos` (sorted `nlohmann::json` with `dump(2)`, `dump()`, `setw(4)`, `setw(0)`, `dump(0)`,
//! and `ordered_json`), 200 000 pseudo-random doubles, 50 000 number texts, 20 000 random
//! documents (comments, BOMs, escapes, invalid UTF-8, truncations) and tables of `get<T>()`,
//! JSON pointer, `operator==` and `update()` cases. The golden file
//! `tests/golden/c3_json.txt` keeps per section the first lines and the count and FNV-1a hash
//! of all lines; [`dump_all`] mirrors `main()` of the C++ program line by line.

use std::collections::BTreeMap;

use kicad_core::json::{
    self, FromJson, JsonError, JsonPointer, ObjectOrder, Value, dump, float_value, json_eq,
};

mod c3_common;
use c3_common::{Lcg, Out, check_golden, hex64, json_files};

const P10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

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
        4 => SPECIAL_D[g.below(20) as usize],
        _ => {
            let v = (g.next() as i64 - 1073741824) as f64 + 0.5;
            v / P10[g.below(8) as usize]
        }
    }
}

const SPECIAL_NUM: [&str; 30] = [
    "-0",
    "0",
    "-0.0",
    "0.0",
    "1e400",
    "-1e400",
    "5e-324",
    "2.2250738585072014e-308",
    "18446744073709551615",
    "18446744073709551616",
    "-9223372036854775808",
    "-9223372036854775809",
    "9223372036854775807",
    "1E5",
    "1e-7",
    "123456789012345678",
    "0.1",
    "1.5e300",
    "4.9406564584124654e-324",
    "1e23",
    "01",
    "-",
    "1.",
    ".5",
    "+1",
    "1e",
    "-01",
    "1.0e+",
    "0e0",
    "-0e-0",
];

const STR_PARTS: [&[u8]; 40] = [
    b"abc",
    b"Net-(R1-Pad1)",
    b"/A/B",
    b" ",
    b"\xC3\xA9",
    b"\xE6\x97\xA5\xE6\x9C\xAC",
    b"\xF0\x9D\x84\x9E",
    b"\xC2\xB5",
    b"\\n",
    b"\\t",
    b"\\\"",
    b"\\\\",
    b"\\/",
    b"\\u0041",
    b"\\u00e9",
    b"\\ud834\\udd1e",
    b"\\u0001",
    b"\\u001f",
    b"\\u007f",
    b"\x7f",
    b"\\b\\f\\r",
    b"\\u20AC",
    b"\\uD83D\\uDE00",
    b"${VAR}",
    b"{",
    b"}",
    b"[",
    b"]",
    b":",
    b",",
    b"//",
    b"/*",
    b"\\uDC00",
    b"\\ud800x",
    b"\x01",
    b"\xC0\xAF",
    b"\xED\xA0\x80",
    b"\\x",
    b"\\u12G4",
    b"\xF4\x90\x80\x80",
];

const KEYS: [&[u8]; 17] = [
    b"a",
    b"b",
    b"meta",
    b"version",
    b"B",
    b"_",
    b"aa",
    b"a b",
    b"\xC3\xA9",
    b"Z",
    b"\\u0000",
    b"1",
    b"10",
    b"2",
    b"",
    b"net_settings",
    b"classes",
];

const WS: [&[u8]; 10] = [b" ", b"\n  ", b"\t", b"/*c*/", b"// x\n", b"\r\n", b"", b"", b"", b""];

fn digits(g: &mut Lcg, n: u32, first_non_zero: bool) -> String {
    let mut s = String::new();

    for i in 0..n {
        let d = if i == 0 && first_non_zero { 1 + g.below(9) } else { g.below(10) };
        s.push((b'0' + d as u8) as char);
    }

    s
}

fn sign(g: &mut Lcg) -> &'static str {
    if g.below(2) != 0 { "-" } else { "" }
}

fn gen_number(g: &mut Lcg) -> String {
    match g.below(8) {
        0 => {
            let s = sign(g);

            if g.below(4) == 0 {
                return format!("{s}0");
            }

            let n = 1 + g.below(20);
            format!("{s}{}", digits(g, n, true))
        }
        1 => {
            let s = sign(g);
            let n = 19 + g.below(7);
            format!("{s}{}", digits(g, n, true))
        }
        2 => {
            let s = sign(g);
            let n = 1 + g.below(6);
            let a = digits(g, n, true);
            let m = 1 + g.below(17);
            let b = digits(g, m, false);
            format!("{s}{a}.{b}")
        }
        3 => {
            let s0 = sign(g);
            let n = 1 + g.below(3);
            let mut s = format!("{s0}{}", digits(g, n, true));

            if g.below(2) != 0 {
                let m = 1 + g.below(5);
                s += &format!(".{}", digits(g, m, false));
            }

            s += if g.below(2) != 0 { "e" } else { "E" };
            let sg = g.below(3);
            s += if sg == 0 {
                ""
            } else if sg == 1 {
                "+"
            } else {
                "-"
            };
            let m = 1 + g.below(3);
            s + &digits(g, m, false)
        }
        4 => SPECIAL_NUM[g.below(SPECIAL_NUM.len() as u32) as usize].to_string(),
        5 => {
            let s = (g.next() as i64 - 1073741824).to_string();
            let n = g.below(3) + 1;
            format!("{s}.{}", digits(g, n, false))
        }
        _ => {
            let s = sign(g);
            format!("{s}{}", g.below(100000))
        }
    }
}

fn gen_string(g: &mut Lcg) -> Vec<u8> {
    let mut s = b"\"".to_vec();
    let n = g.below(6);

    for _ in 0..n {
        let k = g.below(STR_PARTS.len() as u32 + 8) as usize;
        s.extend_from_slice(if k < STR_PARTS.len() { STR_PARTS[k] } else { b"x" });
    }

    s.push(b'"');
    s
}

fn ws(g: &mut Lcg) -> &'static [u8] {
    WS[g.below(10) as usize]
}

fn gen_value(g: &mut Lcg, depth: i32) -> Vec<u8> {
    let r = g.below(if depth > 3 { 6 } else { 10 });

    match r {
        0 => {
            let k = g.below(3);
            (if k == 0 {
                "null"
            } else if k == 1 {
                "true"
            } else {
                "false"
            })
            .as_bytes()
            .to_vec()
        }
        1 | 2 | 5 => gen_number(g).into_bytes(),
        3 | 4 => gen_string(g),
        6 | 7 => {
            let mut s = b"[".to_vec();
            s.extend_from_slice(ws(g));
            let n = g.below(5);

            for i in 0..n {
                if i != 0 {
                    s.push(b',');
                    s.extend_from_slice(ws(g));
                }

                s.extend(gen_value(g, depth + 1));
                s.extend_from_slice(ws(g));
            }

            s.push(b']');
            s
        }
        _ => {
            let mut s = b"{".to_vec();
            s.extend_from_slice(ws(g));
            let n = g.below(5);

            for i in 0..n {
                if i != 0 {
                    s.push(b',');
                    s.extend_from_slice(ws(g));
                }

                s.push(b'"');
                s.extend_from_slice(KEYS[g.below(KEYS.len() as u32) as usize]);
                s.push(b'"');
                s.extend_from_slice(ws(g));
                s.push(b':');
                s.extend_from_slice(ws(g));
                s.extend(gen_value(g, depth + 1));
                s.extend_from_slice(ws(g));
            }

            s.push(b'}');
            s
        }
    }
}

fn gen_document(g: &mut Lcg) -> Vec<u8> {
    let mut s = Vec::new();

    if g.below(16) == 0 {
        s.extend_from_slice(b"\xEF\xBB\xBF");
    }

    s.extend_from_slice(ws(g));
    s.extend(gen_value(g, 0));
    s.extend_from_slice(ws(g));

    let c = g.below(32);

    if c == 0 && !s.is_empty() {
        let n = g.below(s.len() as u32) as usize;
        s.truncate(n);
    } else if c == 1 && !s.is_empty() {
        const BAD: [u8; 10] = [b'}', b']', b',', b':', b'"', b'/', b'x', 0x01, 0xff, 0xef];
        let i = g.below(s.len() as u32) as usize;
        s[i] = BAD[g.below(10) as usize];
    } else if c == 2 {
        s.extend_from_slice(if g.below(2) != 0 { b" 1" } else { b"/" });
    }

    s
}

fn fnv_str(s: &str) -> String {
    hex64(c3_common::fnv_bytes(s.as_bytes(), c3_common::FNV_OFFSET))
}

fn dumps(text: &[u8], order: ObjectOrder, with_streams: bool) -> String {
    let Ok(j) = json::parse_bytes(text, order, true) else {
        return "ERR".to_string();
    };

    let d2 = dump(&j, Some(2), order) + "\n";
    let dc = dump(&j, None, order);
    let mut r = format!("{} {}", fnv_str(&d2), fnv_str(&dc));

    if with_streams {
        let s4 = json::dump_stream(&j, 4, order);
        let s0 = json::dump_stream(&j, 0, order);
        r += &format!(" {} {} {}", fnv_str(&s4), fnv_str(&s0), fnv_str(&dump(&j, Some(0), order)));
    }

    r
}

fn kind(j: &Value) -> String {
    // Integer kinds are not told apart (see PORT_STATUS.md, "-0").
    if json::is_number_integer(j) {
        "n".into()
    } else if json::is_number_float(j) {
        "f".into()
    } else {
        json::type_name(j).into()
    }
}

fn conv<T: FromJson>(j: &Value, f: impl Fn(&T) -> String) -> String {
    match T::from_json(j) {
        Ok(v) => f(&v),
        Err(e) => format!("throw {}", e.id),
    }
}

fn f_dbl(v: &f64) -> String {
    hex64(v.to_bits())
}

fn sorted(text: &str) -> Value {
    json::parse(text, ObjectOrder::Sorted).expect("valid JSON")
}

fn throw(e: &JsonError) -> String {
    format!("throw {}", e.id)
}

fn dump_all() -> Out {
    let mut o = Out::default();
    let root = kicad_qa::kicad_source_root();

    o.begin("files");

    for rel in json_files() {
        let text = std::fs::read(root.join(&rel)).expect("readable file");
        o.out(format!(
            "{rel} {} {}",
            dumps(&text, ObjectOrder::Sorted, true),
            dumps(&text, ObjectOrder::Insertion, false)
        ));
    }

    o.begin("doubles");
    {
        let mut g = Lcg(0xC3D0);

        for _ in 0..200000 {
            let d = rdouble(&mut g);
            o.out(format!("{} {}", f_dbl(&d), dump(&float_value(d), None, ObjectOrder::Sorted)));
        }
    }

    o.begin("nums");
    {
        let mut g = Lcg(0xC3D1);

        for _ in 0..50000 {
            let t = gen_number(&mut g);

            match json::parse(&t, ObjectOrder::Sorted) {
                Ok(j) => o.out(format!("{t} {} {}", kind(&j), dump(&j, None, ObjectOrder::Sorted))),
                Err(_) => o.out(format!("{t} ERR")),
            }
        }
    }

    o.begin("docs");
    {
        let mut g = Lcg(0xC3D2);

        for i in 0..20000 {
            let t = gen_document(&mut g);
            let mut line = format!(
                "{i} {} {}",
                dumps(&t, ObjectOrder::Sorted, true),
                dumps(&t, ObjectOrder::Insertion, false)
            );

            if i < 300 {
                if let Ok(j) = json::parse_bytes(&t, ObjectOrder::Sorted, true) {
                    line += &format!(" {}", c3_common::esc(dump(&j, None, ObjectOrder::Sorted).as_bytes()));
                }
            }

            o.out(line);
        }
    }

    o.begin("get");
    {
        let values = [
            "true",
            "false",
            "null",
            "0",
            "1",
            "-1",
            "2147483647",
            "2147483648",
            "-2147483649",
            "4294967296",
            "1.5",
            "-1.5",
            "2.9999",
            "-0.0",
            "-0",
            "1e10",
            "3e9",
            "-3e9",
            "1e19",
            "1e20",
            "-1e19",
            "9.3e18",
            "1.8446744073709552e19",
            "\"str\"",
            "\"\"",
            "[]",
            "{}",
            "[1,2]",
            "{\"a\":1}",
            "[1,\"x\"]",
            "{\"a\":true}",
            "18446744073709551615",
            "-9223372036854775808",
            "1e300",
            "-1e300",
            "4294967295.5",
            "2147483647.9",
            "-2147483648.9",
            "-2147483648",
            "[true,2.5]",
            "{\"b\":2.5,\"a\":-3}",
            "[\"a\",\"b\"]",
            "{\"a\":\"x\"}",
            "3.4e38",
            "3.5e38",
            "1e-50",
            "[[1]]",
            "-4294967297",
            "9.223372036854775807e18",
            "-9.3e18",
        ];

        for v in values {
            let j = sorted(v);
            let line = [
                v.to_string(),
                conv::<bool>(&j, |b| if *b { "true".into() } else { "false".into() }),
                conv::<i32>(&j, |x| x.to_string()),
                conv::<u32>(&j, |x| x.to_string()),
                conv::<i64>(&j, |x| x.to_string()),
                conv::<u64>(&j, |x| x.to_string()),
                conv::<f64>(&j, f_dbl),
                conv::<f32>(&j, |x| format!("{:08x}", x.to_bits())),
                conv::<String>(&j, |s| format!("\"{}\"", c3_common::esc(s.as_bytes()))),
                conv::<Vec<i32>>(&j, |v| format!("[{}]", v.iter().map(|x| format!("{x};")).collect::<String>())),
                conv::<Vec<String>>(&j, |v| format!("[{}]", v.iter().map(|x| format!("{x};")).collect::<String>())),
                conv::<BTreeMap<String, i32>>(&j, |m| {
                    format!("{{{}}}", m.iter().map(|(k, x)| format!("{k}={x};")).collect::<String>())
                }),
                conv::<BTreeMap<String, f64>>(&j, |m| {
                    format!("{{{}}}", m.iter().map(|(k, x)| format!("{k}={};", f_dbl(x))).collect::<String>())
                }),
            ]
            .join(" | ");
            o.out(line);
        }
    }

    o.begin("ptr");
    {
        let doc = sorted(
            r#"{"a":{"b":[1,{"c":2}],"x":{"y":7},"~":4,"t/1":5,"t~1":8,"":6},
                                    "n":null,"s":"str","arr":[0,1,2,3,4,5,6,7,8,9,10,11],"e":[]}"#,
        );
        let paths = [
            "a",
            "a.b",
            "a.b.0",
            "a.b.1.c",
            "a.b.2",
            "a.b.01",
            "a.b.-",
            "a.b.",
            "a.b.x",
            "a.x.y",
            "a.~",
            "a.~0",
            "a.t~1",
            "a.t~01",
            "a.",
            "n",
            "n.x",
            "n.0",
            "s.x",
            "arr.10",
            "arr.12",
            "arr.1a",
            "",
            "missing",
            "a.b.1.c.d",
            "arr.99999999999999999999",
            "arr.18446744073709551615",
            "e.0",
            "e.-",
            "e.",
            "new.path.x",
            "arr.13",
            "a.b.1.z",
            "s",
            "a.t~",
            "e.00",
            "e.1",
        ];

        for p in paths {
            let ptr = JsonPointer::from_dotted(p);
            let mut line = format!("[{p}] ");

            match ptr.contains(&doc) {
                Ok(c) => line += if c { "1" } else { "0" },
                Err(e) => line += &throw(&e),
            }

            match ptr.at(&doc) {
                Ok(v) => line += &format!(" {}", dump(v, None, ObjectOrder::Sorted)),
                Err(e) => line += &format!(" {}", throw(&e)),
            }

            let mut copy = doc.clone();

            match ptr.get_unchecked(&mut copy) {
                Ok(slot) => {
                    *slot = Value::from(42);
                    line += &format!(" {}", dump(&copy, None, ObjectOrder::Sorted));
                }
                Err(e) => line += &format!(" {}", throw(&e)),
            }

            o.out(line);
        }
    }

    o.begin("eq");
    {
        let pairs = [
            ("1", "1.0"),
            ("-1", "-1.0"),
            ("0", "-0.0"),
            ("1", "true"),
            ("{\"a\":1}", "{\"a\":1.0}"),
            ("[1,2]", "[1.0,2]"),
            ("[1,2]", "[2,1]"),
            ("{\"a\":1,\"b\":2}", "{\"b\":2,\"a\":1}"),
            ("null", "{}"),
            ("\"a\"", "\"a\""),
            ("18446744073709551615", "-1"),
            ("9007199254740993", "9007199254740992.0"),
            ("{}", "[]"),
            ("0.1", "0.1"),
        ];

        for (a, b) in pairs {
            let eq = json_eq(&sorted(a), &sorted(b));
            o.out(format!("{a} == {b} : {}", if eq { "1" } else { "0" }));
        }

        let upd = [
            ("{\"a\":{\"x\":1,\"y\":2},\"b\":1}", "{\"a\":{\"x\":3,\"z\":4},\"c\":5}"),
            ("null", "{\"a\":1}"),
            ("{\"a\":1}", "{\"a\":{\"b\":2}}"),
            ("{\"a\":{\"b\":2}}", "{\"a\":1}"),
            ("[1]", "{\"a\":1}"),
            ("{\"a\":1}", "[1]"),
        ];

        for (a, b) in upd {
            for merge in [false, true] {
                let mut t = sorted(a);
                let mut line = format!("{a} <- {b}{} : ", if merge { " merge" } else { "" });

                match json::update(&mut t, &sorted(b), merge) {
                    Ok(()) => line += &dump(&t, None, ObjectOrder::Sorted),
                    Err(e) => line += &throw(&e),
                }

                o.out(line);
            }
        }
    }

    o
}

#[test]
fn c3_json_oracle_golden() {
    let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/c3_json.txt");
    check_golden(golden, &dump_all(), "c3_json_rust_full.txt");
}
