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
// This file ports code of zint 2.15.0 (thirdparty/zint/backend/code.c):
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

//! Port of `code39()` of `thirdparty/zint/backend/code.c` (BARCODE_CODE39 only: the LOGMARS,
//! HIBC_39, check-digit and COMPLIANT_HEIGHT branches are unreachable with `option_2` 0 and the
//! default output options; the human readable text is not ported, `show_hrt` is 0).

use crate::common::{
    ZINT_ERROR_INVALID_DATA, ZINT_ERROR_TOO_LONG, errtxt, expand, not_sane_lookup, set_height,
    to_upper,
};
use crate::library::ZintSymbol;

/// `SILVER` (code.c).
const SILVER: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%abcd";

/// `C39Table` (code.c): widths of each character, the last is the Start (full 10) / Stop (first
/// 9) character.
const C39_TABLE: [&[u8; 10]; 44] = [
    b"1112212111",
    b"2112111121",
    b"1122111121",
    b"2122111111",
    b"1112211121",
    b"2112211111",
    b"1122211111",
    b"1112112121",
    b"2112112111",
    b"1122112111",
    b"2111121121",
    b"1121121121",
    b"2121121111",
    b"1111221121",
    b"2111221111",
    b"1121221111",
    b"1111122121",
    b"2111122111",
    b"1121122111",
    b"1111222111",
    b"2111111221",
    b"1121111221",
    b"2121111211",
    b"1111211221",
    b"2111211211",
    b"1121211211",
    b"1111112221",
    b"2111112211",
    b"1121112211",
    b"1111212211",
    b"2211111121",
    b"1221111121",
    b"2221111111",
    b"1211211121",
    b"2211211111",
    b"1221211111",
    b"1211112121",
    b"2211112111",
    b"1221112111",
    b"1212121111",
    b"1212111211",
    b"1211121211",
    b"1112121211",
    b"1211212111",
];

/// Port of `code39()` for BARCODE_CODE39 with `option_2` 0.
pub(crate) fn code39(symbol: &mut ZintSymbol, source: &mut [u8]) -> i32 {
    let length = source.len();

    if symbol.option_2 < 0 || symbol.option_2 > 2 {
        symbol.option_2 = 0;
    }

    if length > 86 {
        // 13 (Start) + 86*13 + 12 (Stop) = 1143
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            323,
            &format!("Input length {length} too long (maximum 86)"),
        );
    }

    to_upper(source);
    let posns = match not_sane_lookup(&SILVER[..43], source) {
        Ok(p) => p,
        Err(i) => {
            return errtxt(
                ZINT_ERROR_INVALID_DATA,
                symbol,
                324,
                &format!(
                    "Invalid character at position {i} in input (alphanumerics, space and \"-.$/+%\" only)"
                ),
            );
        }
    };

    let mut dest: Vec<u8> = Vec::with_capacity(890);

    // Start character
    dest.extend_from_slice(C39_TABLE[43]);

    let mut counter = 0usize;
    for &p in &posns {
        dest.extend_from_slice(C39_TABLE[p]);
        counter += p;
    }

    if symbol.option_2 == 1 || symbol.option_2 == 2 {
        // Visible or hidden check digit (not reached: KiCad leaves option_2 0)
        counter %= 43;
        dest.extend_from_slice(C39_TABLE[counter]);
    }

    // Stop character
    dest.extend_from_slice(&C39_TABLE[43][..9]);

    expand(symbol, &dest);

    // Not COMPLIANT_HEIGHT
    let _ = set_height(symbol, 0.0, 50.0, 0.0, true);

    0
}
