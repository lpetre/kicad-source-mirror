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
// This file ports code of zint 2.15.0 (thirdparty/zint/backend/library.c (API)):
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

//! Port of the subset of zint 2.15.0 (`thirdparty/zint/backend`) used by KiCad's `PCB_BARCODE`.
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and `PORT_STATUS.md`.
//! zint is BSD-3-Clause; every ported file carries its licence text.
//!
//! The single entry point is [`encode`], which does what `PCB_BARCODE::ComputeBarcode`
//! (pcbnew/pcb_barcode.cpp) does with zint: `ZBarcode_Create()`, the settings of [`Options`]
//! (`input_mode = UNICODE_MODE`, `show_hrt = 0`), `ZBarcode_Encode()` and
//! `ZBarcode_Buffer_Vector( symbol, 0 )`.

#![forbid(unsafe_code)]

mod code;
mod code128;
mod common;
mod dmatrix;
mod dmatrix_tables;
mod eci;
mod library;
mod qr;
mod qr_tables;
mod reedsol;
mod reedsol_logs;
mod sjis_tables;
mod vector;

pub use common::{
    ZINT_ERROR, ZINT_ERROR_ENCODING_PROBLEM, ZINT_ERROR_INVALID_DATA, ZINT_ERROR_INVALID_OPTION,
    ZINT_ERROR_TOO_LONG, ZINT_WARN_HRT_TRUNCATED, ZINT_WARN_INVALID_OPTION, ZINT_WARN_NONCOMPLIANT,
    ZINT_WARN_USES_ECI,
};
pub use library::{
    BARCODE_CODE39, BARCODE_CODE128, BARCODE_DATAMATRIX, BARCODE_MICROQR, BARCODE_QRCODE,
};
pub use vector::{Vector, VectorHexagon, VectorRect};

use library::{NOT_PORTED, UNICODE_MODE, ZintSymbol, zbarcode_buffer_vector, zbarcode_encode};

/// The `zint_symbol` settings a caller makes after `ZBarcode_Create()` (everything else stays at
/// the defaults; `input_mode` is UNICODE_MODE and `show_hrt` 0, as `PCB_BARCODE` sets them).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    /// `symbol->symbology`: one of the `BARCODE_*` constants of this crate.
    pub symbology: i32,
    /// `symbol->option_1` (zint default -1): the ECC level 1..4 for QR / Micro QR.
    pub option_1: i32,
    /// `symbol->eci` (0, or 26 = UTF-8 for non-ASCII QR / Data Matrix content in KiCad).
    pub eci: i32,
}

/// Where an [`EncodeError`] was raised.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// `ZBarcode_Encode()` returned `>= ZINT_ERROR`.
    Encode,
    /// `ZBarcode_Buffer_Vector()` returned `>= ZINT_ERROR`.
    Vector,
    /// The input needs a zint code path this port does not include (unreachable with the
    /// settings `PCB_BARCODE` uses; see PORT_STATUS.md). `code` is 0 and `errtxt` explains.
    NotPorted,
}

/// A zint error: the return code (`>= ZINT_ERROR`) and `symbol->errtxt`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EncodeError {
    pub stage: Stage,
    pub code: i32,
    pub errtxt: String,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.errtxt)
    }
}

impl std::error::Error for EncodeError {}

/// The encoded symbol: what `zint_symbol` holds after `ZBarcode_Encode()` and
/// `ZBarcode_Buffer_Vector( symbol, 0 )`.
#[derive(Clone, PartialEq, Debug)]
pub struct Symbol {
    /// `symbol->rows`.
    pub rows: i32,
    /// `symbol->width`.
    pub width: i32,
    /// `symbol->height` (after `ZBarcode_Buffer_Vector`).
    pub height: f32,
    /// `symbol->row_height[0 .. rows]`.
    pub row_heights: Vec<f32>,
    /// The module bitmap (`module_is_set( symbol, row, col )`), `rows` x `width`.
    pub modules: Vec<Vec<bool>>,
    /// `symbol->vector`.
    pub vector: Vector,
    /// `symbol->scale`.
    pub scale: f32,
    /// `symbol->option_1/2/3` as fed back by the encoder (e.g. QR ECC level, version, mask).
    pub option_1: i32,
    pub option_2: i32,
    pub option_3: i32,
    /// The warning code and `errtxt` if `ZBarcode_Encode()` (or `ZBarcode_Buffer_Vector()`)
    /// returned a warning (`0 < code < ZINT_ERROR`).
    pub warning: Option<(i32, String)>,
}

/// Encodes `data` (UTF-8 bytes, as `wxString::ToUTF8()` gives them) like `PCB_BARCODE::
/// ComputeBarcode` does with zint: `ZBarcode_Create()`, the [`Options`], `ZBarcode_Encode(
/// symbol, data, length )` and `ZBarcode_Buffer_Vector( symbol, 0 )`.
pub fn encode(options: &Options, data: &[u8]) -> Result<Symbol, EncodeError> {
    let mut symbol = ZintSymbol::create();
    symbol.input_mode = UNICODE_MODE;
    symbol.show_hrt = 0; // do not show HRT
    symbol.symbology = options.symbology;
    symbol.option_1 = options.option_1;
    symbol.eci = options.eci;

    let not_ported = |what: &str| EncodeError {
        stage: Stage::NotPorted,
        code: 0,
        errtxt: format!("zint code path not ported: {what}"),
    };

    let mut warning = None;
    let ret = zbarcode_encode(&mut symbol, data);
    if ret == NOT_PORTED {
        return Err(not_ported(
            "input conversion or option outside KiCad's settings",
        ));
    }
    if ret >= ZINT_ERROR {
        return Err(EncodeError {
            stage: Stage::Encode,
            code: ret,
            errtxt: symbol.errtxt,
        });
    }
    if ret != 0 {
        warning = Some((ret, symbol.errtxt.clone()));
    }

    let ret = zbarcode_buffer_vector(&mut symbol);
    if ret >= ZINT_ERROR {
        return Err(EncodeError {
            stage: Stage::Vector,
            code: ret,
            errtxt: symbol.errtxt,
        });
    }
    if ret != 0 {
        warning = Some((ret, symbol.errtxt.clone()));
    }

    let rows = symbol.rows;
    let width = symbol.width;
    let modules = (0..rows)
        .map(|r| {
            (0..width)
                .map(|c| common::module_is_set(&symbol, r, c))
                .collect()
        })
        .collect();

    Ok(Symbol {
        rows,
        width,
        height: symbol.height,
        row_heights: symbol.row_height[..rows as usize].to_vec(),
        modules,
        vector: symbol.vector.take().unwrap_or_default(),
        scale: symbol.scale,
        option_1: symbol.option_1,
        option_2: symbol.option_2,
        option_3: symbol.option_3,
        warning,
    })
}
