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
