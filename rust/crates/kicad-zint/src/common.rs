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
//
// This file ports code of zint 2.15.0 (thirdparty/zint/backend/common.c):
//
//     libzint - the open source barcode library
//     Copyright (C) 2008-2025 Robin Stuart <rstuart114@gmail.com>
//
//     Redistribution and use in source and binary forms, with or without
//     modification, are permitted provided that the following conditions
//     are met:
//
//     1. Redistributions of source code must retain the above copyright
//        notice, this list of conditions and the following disclaimer.
//     2. Redistributions in binary form must reproduce the above copyright
//        notice, this list of conditions and the following disclaimer in the
//        documentation and/or other materials provided with the distribution.
//     3. Neither the name of the project nor the names of its contributors
//        may be used to endorse or promote products derived from this software
//        without specific prior written permission.
//
//     THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
//     ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//     IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
//     ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE
//     FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//     DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
//     OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
//     HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
//     LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
//     OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
//     SUCH DAMAGE.
//
//     SPDX-License-Identifier: BSD-3-Clause

//! Port of the parts of `thirdparty/zint/backend/common.c` / `common.h` reached by the five
//! symbologies KiCad uses: character classification, `errtxt()`, the module bitmap helpers,
//! `expand()`, `set_height()`, `stripf()` and the UTF-8 decoder.

use crate::library::ZintSymbol;

/// `ZINT_WARN_HRT_TRUNCATED`.
pub const ZINT_WARN_HRT_TRUNCATED: i32 = 1;
/// `ZINT_WARN_INVALID_OPTION`.
pub const ZINT_WARN_INVALID_OPTION: i32 = 2;
/// `ZINT_WARN_USES_ECI`.
pub const ZINT_WARN_USES_ECI: i32 = 3;
/// `ZINT_WARN_NONCOMPLIANT`.
pub const ZINT_WARN_NONCOMPLIANT: i32 = 4;
/// `ZINT_ERROR`: warn/error marker, not returned.
pub const ZINT_ERROR: i32 = 5;
/// `ZINT_ERROR_TOO_LONG`.
pub const ZINT_ERROR_TOO_LONG: i32 = 5;
/// `ZINT_ERROR_INVALID_DATA`.
pub const ZINT_ERROR_INVALID_DATA: i32 = 6;
/// `ZINT_ERROR_INVALID_OPTION`.
pub const ZINT_ERROR_INVALID_OPTION: i32 = 8;
/// `ZINT_ERROR_ENCODING_PROBLEM`.
pub const ZINT_ERROR_ENCODING_PROBLEM: i32 = 9;

/// `ZINT_ROWS_MAX` (zint.h).
pub(crate) const ZINT_ROWS_MAX: usize = 200;
/// `ZINT_COLS_MAX` (zint.h): bytes per row of `encoded_data`.
pub(crate) const ZINT_COLS_MAX: usize = 144;

/// `sizeof(symbol->errtxt)` (zint.h).
const ERRTXT_SIZE: usize = 160;

/// `z_isdigit()`.
#[inline]
pub(crate) fn z_isdigit(c: u32) -> bool {
    c <= u32::from(b'9') && c >= u32::from(b'0')
}

/// `z_isupper()`.
#[inline]
pub(crate) fn z_isupper(c: u32) -> bool {
    c >= u32::from(b'A') && c <= u32::from(b'Z')
}

/// `z_islower()`.
#[inline]
pub(crate) fn z_islower(c: u32) -> bool {
    c >= u32::from(b'a') && c <= u32::from(b'z')
}

/// `z_isascii()`.
#[inline]
pub(crate) fn z_isascii(c: u32) -> bool {
    (c & 0x7F) == c
}

pub(crate) const IS_SPC_F: u16 = 0x0001;
pub(crate) const IS_HSH_F: u16 = 0x0002;
pub(crate) const IS_AST_F: u16 = 0x0004;
pub(crate) const IS_PLS_F: u16 = 0x0008;
pub(crate) const IS_MNS_F: u16 = 0x0010;
pub(crate) const IS_NUM_F: u16 = 0x0020;
pub(crate) const IS_UPO_F: u16 = 0x0040;
pub(crate) const IS_UHX_F: u16 = 0x0080;
pub(crate) const IS_UX__F: u16 = 0x0100;
pub(crate) const IS_LWO_F: u16 = 0x0200;
pub(crate) const IS_LHX_F: u16 = 0x0400;
pub(crate) const IS_LX__F: u16 = 0x0800;
pub(crate) const IS_C82_F: u16 = 0x1000;
pub(crate) const IS_SIL_F: u16 = 0x2000;
pub(crate) const IS_CLI_F: u16 = 0x4000;
pub(crate) const IS_ARS_F: u16 = 0x8000;
/// `IS_UPR_F`: uppercase letters.
pub(crate) const IS_UPR_F: u16 = IS_UPO_F | IS_UHX_F | IS_UX__F;
const IS_CLS_F: u16 = IS_CLI_F | IS_SIL_F;

/// `flgs[256]`: flag table for `is_chr()` and `not_sane()` (0x00-0x1F and 0x80-0xFF are 0).
const FLGS: [u16; 256] = {
    let mut t = [0u16; 256];
    let row: [u16; 96] = [
        IS_SPC_F,
        IS_C82_F,
        IS_C82_F,
        IS_HSH_F, // 20-23  !"#
        IS_CLS_F,
        IS_SIL_F | IS_C82_F,
        IS_C82_F,
        IS_C82_F, // 24-27 $%&'
        IS_C82_F,
        IS_C82_F,
        IS_AST_F,
        IS_PLS_F, // 28-2B ()*+
        IS_C82_F,
        IS_MNS_F,
        IS_CLS_F | IS_C82_F,
        IS_CLS_F | IS_C82_F, // 2C-2F ,-./
        IS_NUM_F,
        IS_NUM_F,
        IS_NUM_F,
        IS_NUM_F, // 30-33
        IS_NUM_F,
        IS_NUM_F,
        IS_NUM_F,
        IS_NUM_F, // 34-37
        IS_NUM_F,
        IS_NUM_F,
        IS_CLI_F | IS_C82_F,
        IS_C82_F, // 38-3B 89:;
        IS_C82_F,
        IS_C82_F,
        IS_C82_F,
        IS_C82_F, // 3C-3F <=>?
        0,
        IS_UHX_F | IS_ARS_F,
        IS_UHX_F | IS_ARS_F,
        IS_UHX_F | IS_ARS_F, // 40-43 @ABC
        IS_UHX_F | IS_ARS_F,
        IS_UHX_F | IS_ARS_F,
        IS_UHX_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F, // DEFG
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F, // HIJK
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F, // LMNO
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F, // PQRS
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F, // TUVW
        IS_UX__F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        IS_UPO_F | IS_ARS_F,
        0, // XYZ[
        0,
        0,
        0,
        IS_C82_F, // \]^_
        0,
        IS_LHX_F,
        IS_LHX_F,
        IS_LHX_F, // `abc
        IS_LHX_F,
        IS_LHX_F,
        IS_LHX_F,
        IS_LWO_F, // defg
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F, // hijk
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F, // lmno
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F, // pqrs
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F,
        IS_LWO_F, // tuvw
        IS_LX__F,
        IS_LWO_F,
        IS_LWO_F,
        0, // xyz{
        0,
        0,
        0,
        0, // |}~ DEL
    ];
    let mut i = 0;
    while i < 96 {
        t[0x20 + i] = row[i];
        i += 1;
    }
    t
};

/// Port of `is_chr()`: whether a character matches `flg`.
pub(crate) fn is_chr(flg: u16, c: u32) -> bool {
    z_isascii(c) && (FLGS[c as usize] & flg) != 0
}

/// Port of `not_sane()`: 1-based position of the first character not matching `flg`, 0 if all do.
pub(crate) fn not_sane(flg: u16, source: &[u8]) -> usize {
    for (i, &c) in source.iter().enumerate() {
        if FLGS[c as usize] & flg == 0 {
            return i + 1;
        }
    }
    0
}

/// Port of `not_sane_lookup()`: positions of each character of `source` in `test_string`;
/// `Err(1-based position)` of the first one not found.
pub(crate) fn not_sane_lookup(test_string: &[u8], source: &[u8]) -> Result<Vec<usize>, usize> {
    let mut posns = Vec::with_capacity(source.len());
    for (i, c) in source.iter().enumerate() {
        match test_string.iter().position(|t| t == c) {
            Some(p) => posns.push(p),
            None => return Err(i + 1),
        }
    }
    Ok(posns)
}

/// Port of `ctoi()`: converts a character 0-9, A-F to its equivalent integer value.
pub(crate) fn ctoi(source: u8) -> i32 {
    if z_isdigit(u32::from(source)) {
        return i32::from(source) - i32::from(b'0');
    }
    if source.is_ascii_uppercase() && source <= b'F' {
        return i32::from(source) - i32::from(b'A') + 10;
    }
    if source.is_ascii_lowercase() && source <= b'f' {
        return i32::from(source) - i32::from(b'a') + 10;
    }
    -1
}

/// Port of `to_upper()`.
pub(crate) fn to_upper(source: &mut [u8]) {
    for c in source.iter_mut() {
        *c &= if z_islower(u32::from(*c)) { 0x5F } else { 0xFF };
    }
}

/// Port of `posn()`: position of `data` in `set_string`, or -1.
pub(crate) fn posn(set_string: &[u8], data: u8) -> i32 {
    match set_string.iter().position(|&c| c == data) {
        Some(p) => p as i32,
        None => -1,
    }
}

/// Port of `bin_append_posn()`: appends `arg` as `length` bits ('0' / '1') to `binary`.
pub(crate) fn bin_append(arg: i32, length: i32, binary: &mut Vec<u8>) {
    let end = length - 1;
    for i in 0..length {
        binary.push(b'0' + ((arg >> (end - i)) & 1) as u8);
    }
}

/// Port of `is_twodigits()`.
pub(crate) fn is_twodigits(source: &[u8], position: usize) -> bool {
    position + 1 < source.len()
        && z_isdigit(u32::from(source[position]))
        && z_isdigit(u32::from(source[position + 1]))
}

/// Port of `module_is_set()`.
pub(crate) fn module_is_set(symbol: &ZintSymbol, y_coord: i32, x_coord: i32) -> bool {
    (symbol.encoded_data[y_coord as usize][(x_coord >> 3) as usize] >> (x_coord & 0x07)) & 1 != 0
}

/// Port of `set_module()`.
pub(crate) fn set_module(symbol: &mut ZintSymbol, y_coord: i32, x_coord: i32) {
    symbol.encoded_data[y_coord as usize][(x_coord >> 3) as usize] |= 1 << (x_coord & 0x07);
}

/// Port of `expand()`: expands a width pattern (`'1'`..`'9'` digits) to a bit pattern row.
pub(crate) fn expand(symbol: &mut ZintSymbol, data: &[u8]) {
    let mut writer = 0;
    let mut latch = true;
    let row = symbol.rows;

    symbol.rows += 1;

    for &d in data {
        let num = i32::from(d) - i32::from(b'0');
        debug_assert!(num >= 0);
        for _ in 0..num {
            if latch {
                set_module(symbol, row, writer);
            }
            writer += 1;
        }

        latch = !latch;
    }

    if writer > symbol.width {
        symbol.width = writer;
    }
}

/// Port of `errtxt_id_str()`: the "err_id: " prefix.
fn errtxt_id_str(num: i32) -> String {
    if num == -1 {
        return String::new();
    }
    let mut num = if !(0..=9999).contains(&num) {
        9999
    } else {
        num
    };
    let mut s = String::new();
    if num >= 1000 {
        s.push((b'0' + (num / 1000) as u8) as char);
        num %= 1000;
    }
    s.push((b'0' + (num / 100) as u8) as char);
    num %= 100;
    s.push((b'0' + (num / 10) as u8) as char);
    num %= 10;
    s.push((b'0' + num as u8) as char);
    s.push_str(": ");
    s
}

/// Truncates `s` to the `errtxt[160]` buffer (159 bytes plus NUL) as `errtxt()` / `errtxtf()` do.
fn errtxt_truncate(mut s: String) -> String {
    let max_len = ERRTXT_SIZE - 1;
    if s.len() > max_len {
        let mut cut = max_len;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Port of `errtxt()` / `errtxtf()`: sets `symbol->errtxt` to "err_id: msg" (no prefix for
/// `err_id` -1; `errtxtf`'s formatting is done by the caller with `format!`, whose `%d` / `%s` /
/// `%c` output is the same), returning `error_number`.
pub(crate) fn errtxt(error_number: i32, symbol: &mut ZintSymbol, err_id: i32, msg: &str) -> i32 {
    let mut s = errtxt_id_str(err_id);
    s.push_str(msg);
    symbol.errtxt = errtxt_truncate(s);
    error_number
}

/// Port of `errtxt_adj()` with the formats `ZBarcode_*` use ("Error %s" / "Warning %s").
pub(crate) fn errtxt_adj_prefix(error_number: i32, symbol: &mut ZintSymbol, prefix: &str) -> i32 {
    let s = format!("{prefix}{}", symbol.errtxt);
    symbol.errtxt = errtxt_truncate(s);
    error_number
}

/// Port of `stripf()`: removes excess precision from floats. Rust `f32` arithmetic has no excess
/// precision (SSE on x86-64, as the reference build), so this is the identity.
#[inline]
pub(crate) fn stripf(arg: f32) -> f32 {
    arg
}

/// Port of `set_height()`: sets the symbol height, returning a warning if not compliant.
pub(crate) fn set_height(
    symbol: &mut ZintSymbol,
    min_row_height: f32,
    default_height: f32,
    max_height: f32,
    no_errtxt: bool,
) -> i32 {
    let mut error_number = 0;
    let mut fixed_height = 0.0f32;
    let mut zero_count = 0;
    let row_height_v: f32;
    let rows = if symbol.rows != 0 { symbol.rows } else { 1 }; // Sometimes called before expand()
    let epsilon = 0.000_000_953_674_316_406_25f32; // Allow some leeway in non-compliance checks

    for i in 0..rows as usize {
        if symbol.row_height[i] != 0.0 {
            fixed_height += symbol.row_height[i];
        } else {
            zero_count += 1;
        }
    }

    if zero_count != 0 {
        let mut row_height = if symbol.height != 0.0 {
            if symbol.input_mode & crate::library::HEIGHTPERROW_MODE != 0 {
                stripf(symbol.height)
            } else {
                stripf((symbol.height - fixed_height) / zero_count as f32)
            }
        } else if default_height != 0.0 {
            stripf(default_height / zero_count as f32)
        } else {
            stripf(min_row_height)
        };
        if row_height < 0.5 {
            // Absolute minimum
            row_height = 0.5;
        }
        if min_row_height != 0.0 && stripf(row_height + epsilon) < stripf(min_row_height) {
            error_number = ZINT_WARN_NONCOMPLIANT;
            if !no_errtxt {
                errtxt(
                    0,
                    symbol,
                    247,
                    "Height not compliant with standards (too small)",
                );
            }
        }
        row_height_v = row_height;
        symbol.height = stripf(row_height_v * zero_count as f32 + fixed_height);
    } else {
        symbol.height = stripf(fixed_height); // Ignore any given height
    }
    if max_height != 0.0 && stripf(symbol.height) > stripf(max_height + epsilon) {
        // Not reached: every caller in the ported subset passes `max_height` 0.
        error_number = ZINT_WARN_NONCOMPLIANT;
    }

    error_number
}

/// The UTF-8 decoding DFA table of `decode_utf8()`.
///
/// Copyright (c) 2008-2009 Bjoern Hoehrmann <bjoern@hoehrmann.de>
///
/// Permission is hereby granted, free of charge, to any person obtaining a copy of this software
/// and associated documentation files (the "Software"), to deal in the Software without
/// restriction, including without limitation the rights to use, copy, modify, merge, publish,
/// distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
/// Software is furnished to do so, subject to the following conditions:
///
/// The above copyright notice and this permission notice shall be included in all copies or
/// substantial portions of the Software.
///
/// See <https://bjoern.hoehrmann.de/utf-8/decoder/dfa/> for details.
const UTF8D: [u8; 364] = [
    // The first part of the table maps bytes to character classes that
    // reduce the size of the transition table and create bitmasks.
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    8, 8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    10, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 3, 3, 11, 6, 6, 6, 5, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, // The second part is a transition table that maps a combination
    // of a state of the automaton and a character class to a state.
    0, 12, 24, 36, 60, 96, 84, 12, 12, 12, 48, 72, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
    12, 0, 12, 12, 12, 12, 12, 0, 12, 0, 12, 12, 12, 24, 12, 12, 12, 12, 12, 24, 12, 24, 12, 12,
    12, 12, 12, 12, 12, 12, 12, 24, 12, 12, 12, 12, 12, 24, 12, 12, 12, 12, 12, 12, 12, 24, 12, 12,
    12, 12, 12, 12, 12, 12, 12, 36, 12, 36, 12, 12, 12, 36, 12, 12, 12, 12, 12, 36, 12, 36, 12, 12,
    12, 36, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
];

/// Port of `decode_utf8()`: state machine to decode UTF-8 to Unicode codepoints (state 0 means
/// done, state 12 means error).
pub(crate) fn decode_utf8(state: &mut u32, codep: &mut u32, byte: u8) -> u32 {
    let type_ = u32::from(UTF8D[byte as usize]);

    *codep = if *state != 0 {
        (u32::from(byte) & 0x3f) | (*codep << 6)
    } else {
        (0xff >> type_) & u32::from(byte)
    };

    *state = u32::from(UTF8D[(256 + *state + type_) as usize]);

    *state
}

/// Port of `is_valid_utf8()`.
pub(crate) fn is_valid_utf8(source: &[u8]) -> bool {
    let mut codepoint = 0u32;
    let mut state = 0u32;

    for &b in source {
        if decode_utf8(&mut state, &mut codepoint, b) == 12 {
            return false;
        }
    }

    state == 0
}

/// Port of `utf8_to_unicode()`: converts UTF-8 to Unicode code points; `disallow_4byte` only
/// allows code points <= U+FFFF.
pub(crate) fn utf8_to_unicode(
    symbol: &mut ZintSymbol,
    source: &[u8],
    vals: &mut Vec<u32>,
    disallow_4byte: bool,
) -> i32 {
    let mut bpos = 0;
    let mut codepoint = 0u32;
    let mut state = 0u32;
    let length = source.len();

    vals.clear();
    while bpos < length {
        loop {
            decode_utf8(&mut state, &mut codepoint, source[bpos]);
            bpos += 1;
            if !(bpos < length && state != 0 && state != 12) {
                break;
            }
        }

        if state != 0 {
            return errtxt(ZINT_ERROR_INVALID_DATA, symbol, 240, "Corrupt Unicode data");
        }
        if disallow_4byte && codepoint > 0xffff {
            return errtxt(
                ZINT_ERROR_INVALID_DATA,
                symbol,
                242,
                "Unicode sequences of more than 3 bytes not supported",
            );
        }

        vals.push(codepoint);
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `decode_utf8()`'s DFA table: `is_valid_utf8()` accepts exactly what the Rust standard
    /// library accepts as UTF-8 (overlongs, surrogates and > U+10FFFF rejected).
    #[test]
    fn utf8_dfa_matches_std() {
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..200_000 {
            let len = (next() % 6) as usize;
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    let r = next();
                    // Bias towards lead / continuation bytes
                    match r % 4 {
                        0 => (r >> 8) as u8 & 0x7F,
                        1 => 0x80 | ((r >> 8) as u8 & 0x3F),
                        _ => 0xC0 | ((r >> 8) as u8 & 0x3F),
                    }
                })
                .collect();
            assert_eq!(
                is_valid_utf8(&bytes),
                std::str::from_utf8(&bytes).is_ok(),
                "{bytes:02x?}"
            );
        }
        for cp in (0..0x11_0000u32).filter_map(char::from_u32) {
            let mut buf = [0u8; 4];
            assert!(is_valid_utf8(cp.encode_utf8(&mut buf).as_bytes()));
        }
    }
}
