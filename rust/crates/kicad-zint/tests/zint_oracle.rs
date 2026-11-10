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

//! Replay of the zint differential oracle (`tests/oracle/zint_oracle.c`, linked against the
//! reference build's `libzint.a`): the same generated cases are encoded with the port, each
//! result is formatted like the C driver's line and hashed per block of 1000 cases
//! (`tests/golden/zint_hashes.txt`).
//!
//! On a mismatch, set `ZINT_ORACLE_BIN` to the built C driver (`rust/target/zint-oracle/
//! zint_oracle`, see `tests/oracle/build_oracle.sh`) to have the first differing case printed.

use kicad_zint::{
    BARCODE_CODE39, BARCODE_CODE128, BARCODE_DATAMATRIX, BARCODE_MICROQR, BARCODE_QRCODE,
    Options, Stage, encode,
};
use std::fmt::Write as _;

// ---- deterministic generator (mirror of zint_oracle.c) ---------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn rnd(&mut self, n: u32) -> u32 {
        (self.next() % u64::from(n)) as u32
    }
}

const QR_ALNUM: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
const C39_SET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%abcxyz";
const SJIS_SPECIALS: [u32; 20] = [
    0x5C, 0x7E, 0xA5, 0x203E, 0xFF61, 0xFF9F, 0x3000, 0x3001, 0x30A2, 0x30F3, 0x4E00, 0x9FA0,
    0xE000, 0xE757, 0xFF5E, 0x2015, 0x00A7, 0x00B6, 0x0391, 0x0410,
];
const NUM_CLASSES: u32 = 12;

fn gen_cp(rng: &mut Rng, cls: u32) -> u32 {
    match cls {
        0 => u32::from(b'0') + rng.rnd(10),
        1 => u32::from(QR_ALNUM[rng.rnd(45) as usize]),
        2 => u32::from(C39_SET[rng.rnd(49) as usize]),
        3 => 0x20 + rng.rnd(95),
        4 => rng.rnd(128),
        5 => 0x80 + rng.rnd(128),
        6 => 0x100 + rng.rnd(0x700),
        7 => {
            let mut u = 0x800 + rng.rnd(0xF800);
            if (0xD800..=0xDFFF).contains(&u) {
                u -= 0x800;
            }
            u
        }
        8 => 0x4E00 + rng.rnd(0x51B0),
        9 => 0x10000 + rng.rnd(0x100000),
        10 => SJIS_SPECIALS[rng.rnd(SJIS_SPECIALS.len() as u32) as usize],
        _ => {
            if rng.rnd(10) < 8 {
                u32::from(b'0') + rng.rnd(10)
            } else {
                u32::from(b'A') + rng.rnd(26)
            }
        }
    }
}

fn put_utf8(u: u32, d: &mut Vec<u8>) {
    if u < 0x80 {
        d.push(u as u8);
    } else if u < 0x800 {
        d.push((0xC0 | (u >> 6)) as u8);
        d.push((0x80 | (u & 0x3F)) as u8);
    } else if u < 0x10000 {
        d.push((0xE0 | (u >> 12)) as u8);
        d.push((0x80 | ((u >> 6) & 0x3F)) as u8);
        d.push((0x80 | (u & 0x3F)) as u8);
    } else {
        d.push((0xF0 | (u >> 18)) as u8);
        d.push((0x80 | ((u >> 12) & 0x3F)) as u8);
        d.push((0x80 | ((u >> 6) & 0x3F)) as u8);
        d.push((0x80 | (u & 0x3F)) as u8);
    }
}

struct GCase {
    symbology: i32,
    option_1: i32,
    eci_rule: bool,
    data: Vec<u8>,
}

const SYMS: [i32; 5] = [BARCODE_CODE39, BARCODE_CODE128, BARCODE_QRCODE, BARCODE_MICROQR, BARCODE_DATAMATRIX];

fn gen_text(rng: &mut Rng, n: i32, classes: &[u32]) -> Vec<u8> {
    let ncls = classes.len() as u32;
    let mut data = Vec::new();
    let mut i = 0;
    while i < n {
        let cls = classes[rng.rnd(ncls) as usize];
        let mut run = 1 + rng.rnd(if ncls == 1 { 1_000_000 } else { 24 });
        while run > 0 && i < n {
            run -= 1;
            let cp = gen_cp(rng, cls);
            put_utf8(cp, &mut data);
            i += 1;
        }
        // C: `while (run-- && i < n)` decrements `run` once more when `i < n` fails; unused.
    }
    data
}

fn max_chars(sym: i32) -> i32 {
    match sym {
        BARCODE_CODE39 => 92,
        BARCODE_CODE128 => 262,
        BARCODE_MICROQR => 40,
        BARCODE_QRCODE => 7200,
        _ => 3200,
    }
}

const SWEEP_CLASSES: [u32; 6] = [0, 1, 3, 4, 5, 8];

fn sweep_option1(sym: i32, k: i32) -> i32 {
    if sym == BARCODE_QRCODE || sym == BARCODE_MICROQR {
        return if k < 4 { k + 1 } else { -2 };
    }
    if k == 0 { -1 } else { -2 }
}

fn sweep_step(sym: i32, len: i32) -> i32 {
    if sym == BARCODE_QRCODE {
        return if len < 400 { 1 } else { 41 };
    }
    if sym == BARCODE_DATAMATRIX {
        return if len < 400 { 1 } else { 29 };
    }
    1
}

fn sweep_max(sym: i32, cls: u32) -> i32 {
    let mut m = max_chars(sym);
    if sym == BARCODE_QRCODE {
        if cls == 1 {
            m = 4400;
        } else if cls != 0 {
            m = 3000;
        }
        if cls == 8 {
            m = 1900;
        }
    } else if sym == BARCODE_DATAMATRIX && cls != 0 {
        m = 1600;
    }
    m
}

/// Copies of zint's `qr_data_codewords` (qr.h) and `dm_matrixbytes` (dmatrix.h).
const QR_CW: [i32; 160] = [19, 34, 55, 80, 108, 136, 156, 194, 232, 274, 324, 370, 428, 461, 523, 589, 647, 721, 795, 861, 932, 1006, 1094, 1174, 1276, 1370, 1468, 1531, 1631, 1735, 1843, 1955, 2071, 2191, 2306, 2434, 2566, 2702, 2812, 2956, 16, 28, 44, 64, 86, 108, 124, 154, 182, 216, 254, 290, 334, 365, 415, 453, 507, 563, 627, 669, 714, 782, 860, 914, 1000, 1062, 1128, 1193, 1267, 1373, 1455, 1541, 1631, 1725, 1812, 1914, 1992, 2102, 2216, 2334, 13, 22, 34, 48, 62, 76, 88, 110, 132, 154, 180, 206, 244, 261, 295, 325, 367, 397, 445, 485, 512, 568, 614, 664, 718, 754, 808, 871, 911, 985, 1033, 1115, 1171, 1231, 1286, 1354, 1426, 1502, 1582, 1666, 9, 16, 26, 36, 46, 60, 66, 86, 100, 122, 140, 158, 180, 197, 223, 253, 283, 313, 341, 385, 406, 442, 464, 514, 538, 596, 628, 661, 701, 745, 793, 845, 901, 961, 986, 1054, 1096, 1142, 1222, 1276];
const DM_BYTES: [i32; 48] = [3,5,5,8,10,12,16,18,18,22,22,24,30,32,32,36,38,43,44,44,49,49,56,62,62,63,64,70,72,80,84,86,90,108,114,118,144,174,204,280,368,456,576,696,816,1050,1304,1558];

fn qr_capacity(e: usize, v: i32, cls: u32) -> i32 {
    let cci = if cls == 0 {
        if v < 10 { 10 } else if v < 27 { 12 } else { 14 }
    } else if cls == 1 {
        if v < 10 { 9 } else if v < 27 { 11 } else { 13 }
    } else if v < 10 {
        8
    } else {
        16
    };
    let bits = 8 * QR_CW[e * 40 + v as usize - 1] - 4 - cci;
    if cls == 0 {
        let r = bits % 10;
        return (bits / 10) * 3 + if r >= 7 { 2 } else if r >= 4 { 1 } else { 0 };
    }
    if cls == 1 {
        return (bits / 11) * 2 + i32::from(bits % 11 >= 6);
    }
    bits / 8
}

struct SweepEntry {
    symbology: i32,
    option_1: i32,
    cls: u32,
    len: i32,
}

fn sweep_list() -> Vec<SweepEntry> {
    let mut v = Vec::new();
    for &s in &SYMS {
        let mut k = 0;
        while sweep_option1(s, k) != -2 {
            for &cls in &SWEEP_CLASSES {
                let mut len = 1;
                while len <= sweep_max(s, cls) {
                    v.push(SweepEntry { symbology: s, option_1: sweep_option1(s, k), cls, len });
                    len += sweep_step(s, len);
                }
            }
            k += 1;
        }
    }
    // Boundary cases: QR version capacities and Data Matrix size capacities, -1 / 0 / +1
    for e in 0..4 {
        for ver in 1..=40 {
            for cls in [0u32, 1, 3] {
                for d in -1..=1 {
                    let len = qr_capacity(e, ver, cls) + d;
                    v.push(SweepEntry { symbology: BARCODE_QRCODE, option_1: e as i32 + 1, cls, len });
                }
            }
        }
    }
    for &bytes in &DM_BYTES {
        for d in -1..=1 {
            for e in 0..2u32 {
                let len = if e == 0 { 2 * bytes } else { (3 * bytes) / 2 } + d;
                v.push(SweepEntry { symbology: BARCODE_DATAMATRIX, option_1: -1, cls: e, len });
            }
        }
    }
    v
}

const RANDOM_CASES: usize = 300_000;

fn gen_case(sweep: &[SweepEntry], idx: usize) -> GCase {
    if idx < sweep.len() {
        let e = &sweep[idx];
        let mut rng = Rng(0x5EED0000u64 + idx as u64);
        let data = gen_text(&mut rng, e.len, &[e.cls]);
        return GCase { symbology: e.symbology, option_1: e.option_1, eci_rule: true, data };
    }
    let mut rng = Rng(0xC0FFEE0000000000u64.wrapping_add((idx - sweep.len()) as u64));
    let symbology = SYMS[rng.rnd(5) as usize];
    let option_1 = if symbology == BARCODE_QRCODE {
        1 + rng.rnd(4) as i32
    } else if symbology == BARCODE_MICROQR {
        if rng.rnd(10) == 0 { 4 } else { 1 + rng.rnd(3) as i32 }
    } else {
        -1
    };
    let eci_rule = rng.rnd(10) != 0;
    let ncls = 1 + rng.rnd(3);
    let classes: Vec<u32> = (0..ncls).map(|_| rng.rnd(NUM_CLASSES)).collect();
    let r = rng.rnd(100);
    let nchars = if symbology == BARCODE_QRCODE || symbology == BARCODE_DATAMATRIX {
        if r < 70 {
            1 + rng.rnd(100) as i32
        } else if r < 97 {
            1 + rng.rnd(1000) as i32
        } else {
            1 + rng.rnd(max_chars(symbology) as u32) as i32
        }
    } else {
        1 + rng.rnd(max_chars(symbology) as u32) as i32
    };
    let data = gen_text(&mut rng, nchars, &classes);
    GCase { symbology, option_1, eci_rule, data }
}

// ---- result line (mirror of `encode_line()`) -------------------------------------------------

fn encode_line(sym: i32, option_1: i32, eci: i32, data: &[u8]) -> String {
    let options = Options {
        symbology: sym,
        option_1: if sym == BARCODE_QRCODE || sym == BARCODE_MICROQR { option_1 } else { -1 },
        eci,
    };
    let mut s = String::new();
    match encode(&options, data) {
        Err(e) => match e.stage {
            Stage::NotPorted => s.push_str("NP"),
            Stage::Encode => write!(s, "{}|{}", e.code, e.errtxt).unwrap(),
            Stage::Vector => s.push_str("VERR"),
        },
        Ok(sym) => {
            let (ret, errtxt) = sym.warning.clone().unwrap_or((0, String::new()));
            write!(
                s,
                "{ret}|{errtxt}|{},{},{:08x}|{},{},{}|",
                sym.rows,
                sym.width,
                sym.height.to_bits(),
                sym.option_1,
                sym.option_2,
                sym.option_3
            )
            .unwrap();
            for (r, h) in sym.row_heights.iter().enumerate() {
                write!(s, "{}{:08x}", if r > 0 { "," } else { "" }, h.to_bits()).unwrap();
            }
            s.push('|');
            for (r, row) in sym.modules.iter().enumerate() {
                if r > 0 {
                    s.push(',');
                }
                for chunk in row.chunks(4) {
                    let mut nib = 0;
                    for (b, &m) in chunk.iter().enumerate() {
                        if m {
                            nib |= 8 >> b;
                        }
                    }
                    write!(s, "{nib:x}").unwrap();
                }
            }
            write!(
                s,
                "|0|{errtxt}|{:08x},{:08x}|",
                sym.vector.width.to_bits(),
                sym.vector.height.to_bits()
            )
            .unwrap();
            for (k, r) in sym.vector.rectangles.iter().enumerate() {
                write!(
                    s,
                    "{}{:08x},{:08x},{:08x},{:08x}",
                    if k > 0 { ";" } else { "" },
                    r.x.to_bits(),
                    r.y.to_bits(),
                    r.width.to_bits(),
                    r.height.to_bits()
                )
                .unwrap();
            }
            s.push('|');
            for h in &sym.vector.hexagons {
                write!(s, "{:08x},{:08x},{:08x},{};", h.x.to_bits(), h.y.to_bits(), h.diameter.to_bits(), h.rotation)
                    .unwrap();
            }
        }
    }
    s
}

fn case_line(sweep: &[SweepEntry], idx: usize) -> String {
    let c = gen_case(sweep, idx);
    let eci = if c.eci_rule
        && (c.symbology == BARCODE_QRCODE || c.symbology == BARCODE_DATAMATRIX)
        && !c.data.is_ascii()
    {
        26
    } else {
        0
    };
    encode_line(c.symbology, c.option_1, eci, &c.data)
}

fn fnv1a(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// The oracle replay: every block hash of `tests/golden/zint_hashes.txt` must match.
#[test]
fn zint_matches_oracle() {
    let golden = include_str!("golden/zint_hashes.txt");
    let mut lines = golden.lines();
    let n: usize = lines.next().unwrap().strip_prefix("N ").unwrap().parse().unwrap();
    let blocks: Vec<u64> = lines
        .map(|l| u64::from_str_radix(l.rsplit(' ').next().unwrap(), 16).unwrap())
        .collect();

    let sweep = sweep_list();
    assert_eq!(n, sweep.len() + RANDOM_CASES, "case count (generator out of sync)");

    let mut first_bad: Option<usize> = None;
    let mut bad = 0;
    for (b, &expected) in blocks.iter().enumerate() {
        let mut h = 0xcbf29ce484222325u64;
        for idx in b * 1000..((b + 1) * 1000).min(n) {
            let mut line = case_line(&sweep, idx);
            line.push('\n');
            fnv1a(&mut h, line.as_bytes());
        }
        if h != expected {
            bad += 1;
            first_bad.get_or_insert(b);
        }
    }

    if let Some(b) = first_bad {
        if let Ok(bin) = std::env::var("ZINT_ORACLE_BIN") {
            for idx in b * 1000..((b + 1) * 1000).min(n) {
                let out = std::process::Command::new(&bin)
                    .args(["case", &idx.to_string()])
                    .output()
                    .expect("run zint_oracle");
                let c_line = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
                let r_line = case_line(&sweep, idx);
                if c_line != r_line {
                    let c = gen_case(&sweep, idx);
                    panic!(
                        "case {idx} (sym {} option_1 {} data {:02x?}) differs:\n C: {}\n R: {}",
                        c.symbology,
                        c.option_1,
                        &c.data[..c.data.len().min(64)],
                        &c_line[..c_line.len().min(600)],
                        &r_line[..r_line.len().min(600)]
                    );
                }
            }
        }
        panic!("{bad} of {} blocks differ, first block {b}", blocks.len());
    }
}

/// Debug helper: compares the cases `ZINT_ORACLE_FROM .. ZINT_ORACLE_TO` line by line against
/// the C driver given by `ZINT_ORACLE_BIN` (`zint_oracle lines a b`).
#[test]
#[ignore]
fn zint_cases_vs_driver() {
    let bin = std::env::var("ZINT_ORACLE_BIN").expect("ZINT_ORACLE_BIN");
    let from: usize = std::env::var("ZINT_ORACLE_FROM").map_or(0, |v| v.parse().unwrap());
    let to: usize = std::env::var("ZINT_ORACLE_TO").map_or(from + 1000, |v| v.parse().unwrap());
    let out = std::process::Command::new(&bin)
        .args(["lines", &from.to_string(), &to.to_string()])
        .output()
        .expect("run zint_oracle");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let sweep = sweep_list();
    let mut bad = 0;
    for (k, c_line) in text.lines().enumerate() {
        let idx = from + k;
        let r_line = case_line(&sweep, idx);
        if c_line != r_line {
            bad += 1;
            if bad <= 5 {
                let c = gen_case(&sweep, idx);
                eprintln!(
                    "case {idx} (sym {} option_1 {} len {} data {:02x?})\n C: {}\n R: {}",
                    c.symbology,
                    c.option_1,
                    c.data.len(),
                    &c.data[..c.data.len().min(48)],
                    &c_line[..c_line.len().min(400)],
                    &r_line[..r_line.len().min(400)]
                );
            }
        }
    }
    assert_eq!(bad, 0, "{bad} cases differ");
}
