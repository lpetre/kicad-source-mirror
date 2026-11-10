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
// This file ports code of zint 2.15.0 (thirdparty/zint/backend/eci.c):
//
//     libzint - the open source barcode library
//     Copyright (C) 2009-2025 Robin Stuart <rstuart114@gmail.com>
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

//! Port of the parts of `thirdparty/zint/backend/eci.c` reached by KiCad's settings: the
//! ISO/IEC 8859-1 conversion of `utf8_to_eci()` (ECI 0 / 1 / 3), `is_eci_convertible()`,
//! `get_eci_length()`, and the QR Code Shift JIS helpers (`u_sjis_int()`, `sjis_utf8()`,
//! `sjis_cpy()`, `sjis_utf8_to_eci()`).
//!
//! Not ported (unreachable from KiCad, see PORT_STATUS.md): the other single- and multi-byte
//! ECI converters (`u_iso8859_*`, `u_cp*`, Big5, GB 2312, GBK, GB 18030, KS X 1001, UTF-16/32),
//! which `utf8_to_eci()` only uses for an explicit ECI other than 0 / 3 / 20 / 26, and
//! `get_best_eci()`, which only runs when a QR / Data Matrix input with ECI 0 fails to convert
//! (KiCad sets ECI 26 for every non-ASCII QR / Data Matrix text). Asking for them returns
//! [`EciUnsupported`].

use crate::common::{ZINT_ERROR_INVALID_DATA, decode_utf8, errtxt, utf8_to_unicode};
use crate::library::ZintSymbol;
use crate::sjis_tables::{SJIS_MB, SJIS_U, SJIS_URO_MB_IND, SJIS_URO_U};

/// An ECI conversion the port does not include (unreachable with KiCad's settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EciUnsupported(pub i32);

/// Port of `is_eci_convertible()`: is the ECI convertible from UTF-8?
pub(crate) fn is_eci_convertible(eci: i32) -> bool {
    // UTF-8 (26) or 8-bit binary data (899) or undefined (> 35 and < 899) or not character set
    // (> 899); excludes ECI 170 - ASCII Invariant
    !(eci == 26 || (eci > 35 && eci != 170))
}

/// Port of `get_eci_length()`: length required to convert UTF-8 to the ECI's encoding (only the
/// Shift JIS case matters here; Latin-1 fits in the UTF-8 length).
pub(crate) fn get_eci_length(eci: i32, source: &[u8]) -> usize {
    let mut length = source.len();
    if eci == 20 {
        // Only ASCII backslash (reverse solidus) exceeds UTF-8 length
        length += source.iter().filter(|&&c| c == b'\\').count();
    }
    length
}

/// Port of `utf8_to_eci()` for ECI 0 / 1 / 3 (ISO/IEC 8859-1) and 20 (Shift JIS): `Ok(Err(code))`
/// is zint's `ZINT_ERROR_INVALID_DATA` return (not convertible), `Err` an ECI the port omits.
pub(crate) fn utf8_to_eci(eci: i32, source: &[u8]) -> Result<Result<Vec<u8>, i32>, EciUnsupported> {
    let mut codepoint = 0u32;
    let mut state = 0u32;
    let mut in_posn = 0;
    let length = source.len();
    let mut dest = Vec::with_capacity(length);

    // Special case ISO/IEC 8859-1
    // Default ECI 0 to ISO/IEC 8859-1 (and ECI 1 for libzueci compatibility)
    if eci == 0 || eci == 3 || eci == 1 {
        while in_posn < length {
            loop {
                decode_utf8(&mut state, &mut codepoint, source[in_posn]);
                in_posn += 1;
                if !(in_posn < length && state != 0 && state != 12) {
                    break;
                }
            }
            if state != 0 {
                return Ok(Err(ZINT_ERROR_INVALID_DATA));
            }
            if codepoint >= 0x80 && (codepoint < 0xA0 || codepoint >= 0x100) {
                return Ok(Err(ZINT_ERROR_INVALID_DATA));
            }
            dest.push(codepoint as u8);
        }
        return Ok(Ok(dest));
    }

    if eci != 20 {
        return Err(EciUnsupported(eci));
    }

    while in_posn < length {
        loop {
            decode_utf8(&mut state, &mut codepoint, source[in_posn]);
            in_posn += 1;
            if !(in_posn < length && state != 0 && state != 12) {
                break;
            }
        }
        if state != 0 {
            return Ok(Err(ZINT_ERROR_INVALID_DATA));
        }
        // `u_sjis()`
        match u_sjis_int(codepoint) {
            Some((1, d)) => dest.push(d as u8),
            Some((_, d)) => {
                dest.push((d >> 8) as u8);
                dest.push(d as u8);
            }
            None => return Ok(Err(ZINT_ERROR_INVALID_DATA)),
        }
    }

    Ok(Ok(dest))
}

/// Port of `eci_u_lookup_uro_int()`: looks up Unicode code point `u` in the URO (Unified
/// Repertoire and Ordering) block (U+4E00-9FFF).
fn eci_u_lookup_uro_int(u: u32, tab_u: &[u16], tab_mb_ind: &[u16], tab_mb: &[u16]) -> Option<u32> {
    let u2 = ((u - 0x4E00) >> 4) as usize; // Blocks of 16
    let mut v: u32 = 1u32 << (u & 0xF);
    if (u32::from(tab_u[u2]) & v) == 0 {
        return None;
    }
    v = u32::from(tab_u[u2]) & (v - 1); // Mask to bits prior to this one
    // Count bits set (http://graphics.stanford.edu/~seander/bithacks.html#CountBitsSetParallel)
    v = v - ((v >> 1) & 0x55555555);
    v = (v & 0x33333333) + ((v >> 2) & 0x33333333);
    v = ((v + (v >> 4)) & 0xF0F0F0F).wrapping_mul(0x1010101) >> 24;
    Some(u32::from(tab_mb[tab_mb_ind[u2] as usize + v as usize]))
}

/// Port of `u_sjis_int()` (ECI 20 Shift JIS): `Some((bytes, value))`, or `None` if `u` has no
/// Shift JIS mapping.
pub(crate) fn u_sjis_int(u: u32) -> Option<(i32, u32)> {
    if u < 0x80 && u != 0x5C && u != 0x7E {
        // Backslash & tilde re-mapped according to JIS X 0201 Roman
        return Some((1, u));
    }
    // Special case URO block sequential mappings (considerably lessens size of `sjis_u[]` array)
    if (0x4E00..=0xDFFF).contains(&u) {
        // 0xE000 next used value >= 0x4E00
        if u >= 0x9FB0 {
            return None;
        }
        return eci_u_lookup_uro_int(u, &SJIS_URO_U, &SJIS_URO_MB_IND, &SJIS_MB).map(|d| (2, d));
    }
    // PUA to user-defined (Table 4-86, Lunde, 2nd ed.)
    if (0xE000..=0xE757).contains(&u) {
        let u2 = u - 0xE000;
        let dv = u2 / 188;
        let md = u2 - dv * 188;
        return Some((2, ((dv + 0xF0) << 8) | (md + 0x40 + u32::from(md >= 0x3F))));
    }
    if u >= u32::from(SJIS_U[0]) && u <= u32::from(SJIS_U[SJIS_U.len() - 1]) {
        let mut s: i32 = 0;
        let mut e: i32 = SJIS_U.len() as i32 - 1;
        while s <= e {
            let m = (s + e) >> 1;
            let um = u32::from(SJIS_U[m as usize]);
            if um < u {
                s = m + 1;
            } else if um > u {
                e = m - 1;
            } else {
                // Adjust for URO block
                let d = u32::from(
                    SJIS_MB[if u >= 0x4E00 {
                        m as usize + 6356
                    } else {
                        m as usize
                    }],
                );
                return Some((1 + i32::from(d > 0xFF), d));
            }
        }
    }
    None
}

/// Port of `sjis_utf8()`: converts UTF-8 to Shift JIS code values (one per character).
pub(crate) fn sjis_utf8(symbol: &mut ZintSymbol, source: &[u8], ddata: &mut Vec<u32>) -> i32 {
    let mut utfdata = Vec::new();

    let error_number = utf8_to_unicode(symbol, source, &mut utfdata, true);
    if error_number != 0 {
        return error_number;
    }

    ddata.clear();
    for &u in &utfdata {
        match u_sjis_int(u) {
            Some((_, d)) => ddata.push(d),
            None => {
                return errtxt(
                    ZINT_ERROR_INVALID_DATA,
                    symbol,
                    800,
                    "Invalid character in input",
                );
            }
        }
    }

    0
}

/// Port of `sjis_cpy()`: copies bytes to code values; with `full_multibyte` double-bytes that
/// match QR Kanji mode go into a single entry.
pub(crate) fn sjis_cpy(source: &[u8], ddata: &mut Vec<u32>, full_multibyte: bool) {
    ddata.clear();
    let length = source.len();
    if full_multibyte {
        let mut i = 0;
        while i < length {
            let c1 = source[i];
            // Now using stricter interpretation of standard, and excluding certain trailing bytes
            if ((0x81..=0x9F).contains(&c1) || (0xE0..=0xEB).contains(&c1)) && length - i >= 2 {
                let c2 = source[i + 1];
                if (0x40..=0xFC).contains(&c2) && c2 != 0x7F && (c1 != 0xEB || c2 <= 0xBF) {
                    // This may or may not be valid Shift JIS, but don't care as long as it can be
                    // encoded in QR Kanji mode
                    ddata.push((u32::from(c1) << 8) | u32::from(c2));
                    i += 1;
                } else {
                    ddata.push(u32::from(c1));
                }
            } else {
                ddata.push(u32::from(c1));
            }
            i += 1;
        }
    } else {
        // Straight copy
        ddata.extend(source.iter().map(|&c| u32::from(c)));
    }
}

/// Port of `sjis_utf8_to_eci()`: converts UTF-8 to the ECI's encoding and copies it with
/// `sjis_cpy()`. Returns zint's error number (0 on success); the caller sets `errtxt`.
pub(crate) fn sjis_utf8_to_eci(
    eci: i32,
    source: &[u8],
    ddata: &mut Vec<u32>,
    full_multibyte: bool,
) -> Result<i32, EciUnsupported> {
    if is_eci_convertible(eci) {
        match utf8_to_eci(eci, source)? {
            Ok(converted) => sjis_cpy(&converted, ddata, full_multibyte || eci == 20),
            // Note not setting `symbol->errtxt`, up to caller
            Err(error_number) => return Ok(error_number),
        }
    } else {
        sjis_cpy(source, ddata, full_multibyte);
    }

    Ok(0)
}
