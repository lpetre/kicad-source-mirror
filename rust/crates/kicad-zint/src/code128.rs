//! Port of `code128()` of `thirdparty/zint/backend/code128.c` (BARCODE_CODE128): the minimal
//! Code Set selection (`c128_cost`, `c128_set_values`), `c128_expand` and `c128_set_priority`.
//! Not reached with KiCad's settings (and not ported): EXTRA_ESCAPE_MODE manual Code Set / FNC1
//! escapes (so `manuals` / `fncs` stay all zero), READER_INIT, BARCODE_CODE128AB, GS1-128 and
//! the human readable text.

use crate::common::{
    ZINT_ERROR_TOO_LONG, ZINT_WARN_HRT_TRUNCATED, errtxt, expand, z_isascii, z_isdigit,
};
use crate::library::ZintSymbol;

/// `C128_SYMBOL_MAX`: 102 * 10 + 10 (check digit) + 13 (Stop) = 1043.
const C128_SYMBOL_MAX: i32 = 102;
/// `C128_MAX` (code128.h).
const C128_MAX: usize = 256;

/// `C128Table` (code128.c, Table 1 of ISO/IEC 15417:2007, with the final CODE16K-only character).
const C128_TABLE: [&[u8; 6]; 107] = [
    b"212222", b"222122", b"222221", b"121223", b"121322", b"131222", b"122213", b"122312",
    b"132212", b"221213", b"221312", b"231212", b"112232", b"122132", b"122231", b"113222",
    b"123122", b"123221", b"223211", b"221132", b"221231", b"213212", b"223112", b"312131",
    b"311222", b"321122", b"321221", b"312212", b"322112", b"322211", b"212123", b"212321",
    b"232121", b"111323", b"131123", b"131321", b"112313", b"132113", b"132311", b"211313",
    b"231113", b"231311", b"112133", b"112331", b"132131", b"113123", b"113321", b"133121",
    b"313121", b"211331", b"231131", b"213113", b"213311", b"213131", b"311123", b"311321",
    b"331121", b"312113", b"312311", b"332111", b"314111", b"221411", b"431111", b"111224",
    b"111422", b"121124", b"121421", b"141122", b"141221", b"112214", b"112412", b"122114",
    b"122411", b"142112", b"142211", b"241211", b"221114", b"413111", b"241112", b"134111",
    b"111242", b"121142", b"121241", b"114212", b"124112", b"124211", b"411212", b"421112",
    b"421211", b"212141", b"214121", b"412121", b"111143", b"111341", b"131141", b"114113",
    b"114311", b"411113", b"411311", b"113141", b"114131", b"311141", b"411131", b"211412",
    b"211214", b"211232", b"211133",
];

// Code Set states
const C128_A0: usize = 1;
const C128_B0: usize = 2;
const C128_A1: usize = 3;
const C128_B1: usize = 4;
const C128_C0: usize = 5;
const C128_C1: usize = 6;
const C128_STATES: usize = 7;

/// `C128_A0B0(cset)`.
#[inline]
fn c128_a0b0(cset: usize) -> bool {
    cset <= C128_B0
}
/// `C128_C0C1(cset)`.
#[inline]
fn c128_c0c1(cset: usize) -> bool {
    cset >= C128_C0
}
/// `C128_A0A1(cset)` (assuming !C).
#[inline]
fn c128_a0a1(cset: usize) -> usize {
    cset & 1
}
/// `C128_AB(cset)` (assuming !C).
#[inline]
fn c128_ab(cset: usize) -> usize {
    cset >> usize::from(cset > C128_B0)
}

/// `c128_latch_seq`: Code Set states are named A0, B0, A1, B1, C0 and C1. A, B, and C are the
/// Code 128 Code Sets. 0 is for ASCII and 1 is for extended ASCII range. Indexed [prior][current].
const C128_LATCH_SEQ: [[[i32; 3]; C128_STATES]; C128_STATES] = [
    [[0; 3]; 7],
    // A0
    [[0; 3], [0, 0, 0], [100, 0, 0], [101, 101, 0], [100, 100, 100], [99, 0, 0], [101, 101, 99]],
    // B0
    [[0; 3], [101, 0, 0], [0, 0, 0], [101, 101, 101], [100, 100, 0], [99, 0, 0], [100, 100, 99]],
    // A1
    [[0; 3], [101, 101, 0], [100, 100, 100], [0, 0, 0], [100, 0, 0], [101, 101, 99], [99, 0, 0]],
    // B1
    [[0; 3], [101, 101, 101], [100, 100, 0], [101, 0, 0], [0, 0, 0], [100, 100, 99], [99, 0, 0]],
    // C0
    [[0; 3], [101, 0, 0], [100, 0, 0], [101, 101, 101], [100, 100, 100], [0, 0, 0], [0, 0, 0]],
    // C1
    [[0; 3], [101, 101, 101], [100, 100, 100], [101, 0, 0], [100, 0, 0], [0, 0, 0], [0, 0, 0]],
];
/// `c128_latch_len`: lengths of the above.
const C128_LATCH_LEN: [[i32; C128_STATES]; C128_STATES] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 1, 2, 3, 1, 3],  // A0
    [0, 1, 0, 3, 2, 1, 3],  // B0
    [0, 2, 3, 0, 1, 3, 1],  // A1
    [0, 3, 2, 1, 0, 3, 1],  // B1
    [0, 1, 1, 3, 3, 0, 64], // C0
    [0, 3, 3, 1, 1, 64, 0], // C1
];

/// `c128_start_latch_seq[0]`: normal start sequences (GS1_MODE and READER_INIT not reached).
const C128_START_LATCH_SEQ: [[i32; 4]; C128_STATES] = [
    [0; 4],
    [103, 0, 0, 0],
    [104, 0, 0, 0],
    [103, 101, 101, 0],
    [104, 100, 100, 0],
    [105, 0, 0, 0],
    [0; 4],
];
/// `c128_start_latch_len[0]`.
const C128_START_LATCH_LEN: [i32; C128_STATES] = [0, 1, 1, 3, 3, 1, 64];

/// Port of `c128_cost_ab()`: output cost (length) for Code Sets A/B.
fn c128_cost_ab(cset: usize, ch: u8, p_mode: &mut usize) -> i32 {
    let mask_0x60 = ch & 0x60; // 0 for (ch & 0x7F) < 32, 0x60 for (ch & 0x7F) >= 96
    let ga = c128_a0a1(cset) != 0;
    let mut cost = 1;

    debug_assert!(!c128_c0c1(cset));

    // SHIFT
    if (ga && mask_0x60 == 0x60) || (!ga && mask_0x60 == 0) {
        // A and (ch & 0x7F) >= 96, or B and (ch & 0x7F) < 32
        cost += 1;
        *p_mode |= 0x10;
    }

    // FNC4
    if c128_a0b0(cset) == !z_isascii(u32::from(ch)) {
        // If A0/B0 and extended ASCII, or A1/B1 and ASCII
        cost += 1;
        *p_mode |= 0x20;
    }

    cost
}

/// State of the memoized cost search of `c128_cost()` / `c128_set_values()`.
struct C128Search<'a> {
    source: &'a [u8],
    priority: &'a [usize],
    costs: Vec<[i16; C128_STATES]>,
    modes: Vec<[u8; C128_STATES]>,
}

impl C128Search<'_> {
    /// `source[i]` of the NUL-terminated C buffer.
    fn at(&self, i: usize) -> u8 {
        self.source.get(i).copied().unwrap_or(0)
    }

    /// Port of `c128_cost()`: the cost of encoding from `i` starting in Code Set `prior_cset`
    /// (no manual FNC1s or Code Sets, normal start).
    fn cost(&mut self, i: usize, prior_cset: usize) -> i32 {
        let length = self.source.len();
        let ch = self.at(i);
        let latch_len: &[i32; C128_STATES] = if prior_cset == 0 {
            &C128_START_LATCH_LEN
        } else {
            &C128_LATCH_LEN[prior_cset]
        };
        let is_fnc1 = false; // `ch == '\x1D' && fncs[i]`, no manual FNC1s
        let can_c = is_fnc1 || (z_isdigit(u32::from(ch)) && z_isdigit(u32::from(self.at(i + 1))));
        let manual_c_fail = false; // `!can_c && manuals[i] == C128_C0`, no manuals
        let mut min_cost = 999999; // Max possible cost less than 2 * 256
        let mut min_mode = 0usize;

        for p in 0..self.priority.len() {
            let cset = self.priority[p];
            if c128_c0c1(cset) {
                if can_c {
                    let incr = 2 - usize::from(is_fnc1);
                    let mut mode = prior_cset;
                    let mut cost = 1;
                    if prior_cset != cset {
                        cost += latch_len[cset];
                        mode = cset;
                    }
                    if i + incr < length {
                        // Check if memoized
                        if self.costs[i + incr][cset] != 0 {
                            cost += i32::from(self.costs[i + incr][cset]);
                        } else {
                            cost += self.cost(i + incr, cset);
                        }
                    }
                    if cost < min_cost {
                        min_cost = cost;
                        min_mode = mode;
                    }
                }
            } else {
                let _ = (manual_c_fail, c128_ab(cset)); // `!manuals[i] || ...` always true
                let mut mode = cset;
                let mut cost = if is_fnc1 { 1 } else { c128_cost_ab(cset, ch, &mut mode) };
                if prior_cset != cset {
                    cost += latch_len[cset];
                }
                if i + 1 < length {
                    // Check if memoized
                    if self.costs[i + 1][cset] != 0 {
                        cost += i32::from(self.costs[i + 1][cset]);
                    } else {
                        cost += self.cost(i + 1, cset);
                    }
                }
                if cost < min_cost {
                    min_cost = cost;
                    min_mode = mode;
                }
            }
        }
        debug_assert!(min_cost != 999999);

        self.costs[i][prior_cset] = min_cost as i16;
        self.modes[i][prior_cset] = min_mode as u8;

        min_cost
    }
}

/// Port of `c128_set_values()`: minimal encoding using Divide-And-Conquer with Memoization by
/// Alex Geller - see <https://github.com/zxing/zxing/commit/94fb277607003c070ffd1413754a782f3f87cbcd>.
/// Many ideas, especially enabling extended ASCII, taken from BWIPP minimal encoding by Bue
/// Jensen - see <https://github.com/bwipp/postscriptbarcode/pull/278>. Returns the glyph count
/// (the minimal cost if it exceeds `C128_SYMBOL_MAX`, without filling `values`).
fn c128_set_values(source: &[u8], priority: &[usize], values: &mut Vec<i32>) -> i32 {
    let length = source.len();
    let mut search = C128Search {
        source,
        priority,
        costs: vec![[0i16; C128_STATES]; length],
        modes: vec![[0u8; C128_STATES]; length],
    };
    let mut cset = 0usize;

    search.cost(0, 0);

    if i32::from(search.costs[0][0]) > C128_SYMBOL_MAX {
        // Total minimal cost (glyph count)
        return i32::from(search.costs[0][0]);
    }

    // Output codewords into `values`
    let mut i = 0;
    while i < length {
        let ch = source[i];
        let mode = usize::from(search.modes[i][cset]);
        let prior_cset = cset;

        cset = mode & 0x0F;
        debug_assert!(cset != 0);
        if cset != prior_cset {
            if prior_cset == 0 {
                debug_assert!(cset != C128_C1);
                for j in 0..C128_START_LATCH_LEN[cset] as usize {
                    values.push(C128_START_LATCH_SEQ[cset][j]);
                }
            } else {
                for j in 0..C128_LATCH_LEN[prior_cset][cset] as usize {
                    values.push(C128_LATCH_SEQ[prior_cset][cset][j]);
                }
            }
        }
        if mode >= 0x30 {
            // Extended Shift A/B
            values.push(100 + c128_a0a1(cset) as i32); // FNC4
            values.push(98); // SHIFT
        } else if mode >= 0x20 {
            // Extended A/B
            values.push(100 + c128_a0a1(cset) as i32); // FNC4
        } else if mode >= 0x10 {
            // Shift A/B
            values.push(98); // SHIFT
        }
        if c128_c0c1(cset) {
            debug_assert!(i + 1 < length); // Guaranteed by algorithm
            i += 1;
            values.push((i32::from(ch) - i32::from(b'0')) * 10 + i32::from(source[i]) - i32::from(b'0'));
        } else {
            // (ch & 0x7F) < 32 ? (ch & 0x7F) + 64 : (ch & 0x7F) - 32
            values.push(i32::from(ch & 0x7F) + 96 * i32::from(ch & 0x60 == 0) - 32);
        }
        i += 1;
    }
    debug_assert_eq!(values.len() as i32, i32::from(search.costs[0][0]));

    values.len() as i32
}

/// Port of `c128_expand()`: writes out the symbol, calculating the check digit.
fn c128_expand(symbol: &mut ZintSymbol, values: &[i32]) {
    let mut dest: Vec<u8> = Vec::with_capacity(640);

    // Destination setting and check digit calculation
    dest.extend_from_slice(C128_TABLE[values[0] as usize]);
    let mut total_sum = values[0];

    for (i, &v) in values.iter().enumerate().skip(1) {
        dest.extend_from_slice(C128_TABLE[v as usize]);
        total_sum += v * i as i32; // Note can't overflow as 106 * C128_SYMBOL_MAX * C128_SYMBOL_MAX = 1102824
    }
    total_sum %= 103;
    dest.extend_from_slice(C128_TABLE[total_sum as usize]);

    // Stop character
    dest.extend_from_slice(b"2331112");

    expand(symbol, &dest);
}

/// Port of `c128_set_priority()`: sets the `priority` array based on flags.
fn c128_set_priority(have_a: bool, have_b: bool, have_c: bool, have_extended: bool) -> Vec<usize> {
    let mut priority = Vec::with_capacity(C128_STATES);
    if have_c {
        priority.push(C128_C0);
    }
    if have_b || !have_a {
        priority.push(C128_B0);
    }
    if have_a {
        priority.push(C128_A0);
    }
    if have_extended {
        if have_c {
            priority.push(C128_C1);
        }
        if have_b || !have_a {
            priority.push(C128_B1);
        }
        if have_a {
            priority.push(C128_A1);
        }
    }
    priority
}

/// Port of `code128()` for BARCODE_CODE128 without EXTRA_ESCAPE_MODE / READER_INIT.
pub(crate) fn code128(symbol: &mut ZintSymbol, source: &[u8]) -> i32 {
    let length = source.len();
    let mut have_a = false;
    let mut have_b = false;
    let mut have_c = false;
    let mut have_extended = false;

    debug_assert!(length != 0);

    if length > C128_MAX {
        // This only blocks ridiculously long input - the actual length of the
        // resulting barcode depends on the type of data, so this is trapped later
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            340,
            &format!("Input length {length} too long (maximum 256)"),
        );
    }

    // Classify data to detect which Code Set states are needed
    let mut digit = false;
    for &ch in source {
        let mask_0x60 = ch & 0x60; // 0 for (ch & 0x7F) < 32, 0x60 for (ch & 0x7F) >= 96
        have_extended |= ch & 0x80 != 0;
        have_a |= mask_0x60 == 0;
        have_b |= mask_0x60 == 0x60;
        let prev_digit = digit;
        digit = z_isdigit(u32::from(ch));
        have_c |= prev_digit && digit;
    }
    let priority = c128_set_priority(have_a, have_b, have_c, have_extended);

    let mut values = Vec::with_capacity(C128_SYMBOL_MAX as usize + 2);
    let glyph_count = c128_set_values(source, &priority, &mut values);

    // Now we know how long the barcode is - stop it from being too long
    if glyph_count > C128_SYMBOL_MAX {
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            341,
            &format!(
                "Input too long, requires {glyph_count} symbol characters (maximum 102)"
            ),
        );
    }

    // Now we can get on with it!
    c128_expand(symbol, &values);

    // ISO/IEC 15417:2007 leaves dimensions/height as application specification

    // HRT
    hrt_cpy_iso8859_1(symbol, source) // Returns warning only
}

/// Port of `hrt_cpy_iso8859_1()` (common.c) as far as it affects the result: the HRT itself is
/// not kept (`show_hrt` 0), only the truncation warning for a text exceeding `text[256]`.
fn hrt_cpy_iso8859_1(symbol: &mut ZintSymbol, source: &[u8]) -> i32 {
    const TEXT_SIZE: usize = 256; // ARRAY_SIZE(symbol->text)
    let mut warn_number = 0;
    let mut j = 0usize;

    for &c in source {
        if j >= TEXT_SIZE {
            break;
        }
        if z_isascii(u32::from(c)) || c < 0xA0 {
            j += 1;
        } else {
            if j + 2 >= TEXT_SIZE {
                warn_number = ZINT_WARN_HRT_TRUNCATED;
                break;
            }
            j += 2;
        }
    }
    if j == TEXT_SIZE {
        warn_number = ZINT_WARN_HRT_TRUNCATED;
        j -= 1;
    }
    symbol.text_length = j as i32;

    if warn_number != 0 {
        errtxt(0, symbol, 249, "Human Readable Text truncated");
    }
    warn_number
}
