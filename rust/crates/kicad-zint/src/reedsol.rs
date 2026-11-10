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
// This file ports code of zint 2.15.0 (thirdparty/zint/backend/reedsol.c):
//
//     This is a simple Reed-Solomon encoder
//     (C) Cliff Hones 2004
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

//! Port of `thirdparty/zint/backend/reedsol.c` / `reedsol.h` (`rs_init_gf`, `rs_init_code`,
//! `rs_encode`) for the two 8-bit Galois fields used here: 0x11d (QR Code) and 0x12d
//! (Data Matrix ECC 200). The `rs_uint_*` variants (> 8-bit fields) are not used by the five
//! symbologies.

use crate::reedsol_logs::{ALOG_0X11D, ALOG_0X12D, LOGT_0X11D, LOGT_0X12D};

/// Port of `rs_t`.
pub(crate) struct Rs {
    logt: &'static [u8],
    alog: &'static [u8],
    rspoly: [u8; 256],
    log_rspoly: [u8; 256],
    nsym: usize,
    zero: bool,
}

impl Rs {
    /// Port of `rs_init_gf()`: initialises the Galois field parameters for `prime_poly`
    /// (0x11d or 0x12d).
    pub(crate) fn init_gf(prime_poly: u32) -> Rs {
        let (logt, alog): (&'static [u8], &'static [u8]) = match prime_poly {
            0x11d => (&LOGT_0X11D, &ALOG_0X11D),
            0x12d => (&LOGT_0X12D, &ALOG_0X12D),
            _ => unreachable!("prime poly {prime_poly:#x} not used by the ported symbologies"),
        };
        Rs {
            logt,
            alog,
            rspoly: [0; 256],
            log_rspoly: [0; 256],
            nsym: 0,
            zero: false,
        }
    }

    /// Port of `rs_init_code()`: initialises the encoder for `nsym` check symbols with generator
    /// polynomial `(x + 2**index) * (x + 2**(index+1)) * ...`.
    pub(crate) fn init_code(&mut self, nsym: usize, mut index: usize) {
        let logt = self.logt;
        let alog = self.alog;
        let rspoly = &mut self.rspoly;

        self.nsym = nsym;

        rspoly[0] = 1;
        for i in 1..=nsym {
            rspoly[i] = 1;
            for k in (1..i).rev() {
                if rspoly[k] != 0 {
                    rspoly[k] = alog[logt[rspoly[k] as usize] as usize + index]; // Multiply coeff by 2**index
                }
                rspoly[k] ^= rspoly[k - 1]; // Add coeff of x**(k-1) * x
            }
            rspoly[0] = alog[logt[rspoly[0] as usize] as usize + index]; // 2**(i + (i+1) + ... + index)
            index += 1;
        }

        // Set logs of poly and check if have zero coeffs
        self.zero = false;
        for i in 0..=nsym {
            self.log_rspoly[i] = logt[self.rspoly[i] as usize]; // For simplicity allow log of 0
            self.zero |= self.rspoly[i] == 0;
        }
    }

    /// Port of `rs_encode()`: the `nsym` Reed-Solomon check codes of `data`.
    pub(crate) fn encode(&self, data: &[u8], res: &mut [u8]) {
        let logt = self.logt;
        let alog = self.alog;
        let nsym = self.nsym;
        let nsym_halved = nsym >> 1;

        res[..nsym].fill(0);
        for &d in data {
            let m = u32::from(res[nsym - 1] ^ d);
            if m != 0 {
                let log_m = logt[m as usize] as usize;
                for k in (1..nsym).rev() {
                    if !self.zero || self.rspoly[k] != 0 {
                        res[k] = res[k - 1] ^ alog[log_m + self.log_rspoly[k] as usize];
                    } else {
                        res[k] = res[k - 1];
                    }
                }
                res[0] = alog[log_m + self.log_rspoly[0] as usize];
            } else {
                res.copy_within(0..nsym - 1, 1);
                res[0] = 0;
            }
        }
        // Reverse the result
        for i in 0..nsym_halved {
            res.swap(i, nsym - 1 - i);
        }
    }
}
