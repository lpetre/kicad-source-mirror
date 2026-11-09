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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.
//
// Code by: B-Con (http://b-con.us)
// Released under the GNU GPL
// MD5 Hash Digest implementation (little endian byte order)

//! Port of `libs/kimath/include/md5_hash.h` and `libs/kimath/src/md5_hash.cpp` (`MD5_HASH`).

/// `MD5_HASH::MD5_CTX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Md5Ctx {
    data: [u8; 64],
    datalen: u32,
    bitlen: [u32; 2],
    state: [u32; 4],
}

/// Port of `MD5_HASH`.
#[derive(Clone, Copy, Debug)]
pub struct Md5Hash {
    valid: bool,
    ctx: Md5Ctx,
    hash: [u8; 16],
}

impl Default for Md5Hash {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Md5Hash {
    /// Port of `MD5_HASH::operator==`: compares the finalized hash bytes only.
    fn eq(&self, a_other: &Self) -> bool {
        self.hash == a_other.hash
    }
}

impl Eq for Md5Hash {}

/// `DBL_INT_ADD( a, b, c )`: treats two unsigned ints a and b as one 64-bit integer and adds c
/// to it.
#[inline]
fn dbl_int_add(a: &mut u32, b: &mut u32, c: u32) {
    if *a > 0xffff_ffff - c {
        *b = b.wrapping_add(1);
    }

    *a = a.wrapping_add(c);
}

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}

#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}

#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline]
fn i(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

/// `FF/GG/HH/II( a, b, c, d, m, s, t )`: `a += fn( b, c, d ) + m + t; a = b + ROTLEFT( a, s );`
#[inline]
fn step(
    func: fn(u32, u32, u32) -> u32,
    a: &mut u32,
    b: u32,
    c: u32,
    d: u32,
    m: u32,
    s: u32,
    t: u32,
) {
    *a = a.wrapping_add(func(b, c, d).wrapping_add(m).wrapping_add(t));
    *a = b.wrapping_add(a.rotate_left(s));
}

impl Md5Hash {
    /// Port of `MD5_HASH::MD5_HASH()` (calls `Init()`).
    pub fn new() -> Self {
        let mut h = Md5Hash {
            valid: false,
            ctx: Md5Ctx {
                data: [0; 64],
                datalen: 0,
                bitlen: [0; 2],
                state: [0; 4],
            },
            hash: [0; 16],
        };
        h.init();
        h
    }

    /// Port of `MD5_HASH::Init`.
    pub fn init(&mut self) {
        self.valid = false;
        Self::md5_init(&mut self.ctx);
        self.hash = [0; 16];
    }

    /// Port of `MD5_HASH::Hash( uint8_t* data, uint32_t length )`.
    pub fn hash_bytes(&mut self, a_data: &[u8]) {
        Self::md5_update(&mut self.ctx, a_data);
    }

    /// Port of `MD5_HASH::Hash( int value )`: the 4 bytes of the `int` in memory order
    /// (little endian).
    pub fn hash_int(&mut self, a_value: i32) {
        Self::md5_update(&mut self.ctx, &a_value.to_le_bytes());
    }

    /// Port of `MD5_HASH::Finalize`.
    pub fn finalize(&mut self) {
        Self::md5_final(&mut self.ctx, &mut self.hash);
        self.valid = true;
    }

    /// Port of `MD5_HASH::IsValid`.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Port of `MD5_HASH::SetValid`.
    pub fn set_valid(&mut self, a_valid: bool) {
        self.valid = a_valid;
    }

    /// The 16 bytes of the finalized hash (`m_hash`).
    pub fn digest(&self) -> [u8; 16] {
        self.hash
    }

    /// Port of `MD5_HASH::Format`.
    ///
    /// Builds a "hexadecimal" string from the 16 bytes; with `a_compact_form == false` each
    /// byte is followed by a space.  Note that the C++ maps the nibbles 10..15 to `'B'..'G'`
    /// (it adds `'A' - '9'` to `'0' + nibble`), which is reproduced.
    pub fn format(&self, a_compact_form: bool) -> String {
        let mut data = String::new();

        let digit = |nibble: u8| -> char {
            let mut c = nibble + b'0';

            if c > b'9' {
                c += b'A' - b'9';
            }

            c as char
        };

        // Build a hexadecimal string from the 16 bytes of MD5_HASH:
        for &b in self.hash.iter() {
            let lsb = digit(b & 0x0F);
            let msb = digit((b >> 4) & 0x0F);

            data.push(msb);
            data.push(lsb);

            if !a_compact_form {
                data.push(' ');
            }
        }

        data
    }

    /// Port of `MD5_HASH::md5_transform`.
    fn md5_transform(ctx: &mut Md5Ctx) {
        let data = &ctx.data;
        let mut m = [0u32; 16];

        // MD5 specifies big endian byte order, but this implementation assumes a little
        // endian byte order CPU. Reverse all the bytes upon input, and re-reverse them
        // on output (in md5_final()).
        for (idx, word) in m.iter_mut().enumerate() {
            let j = idx * 4;
            *word = (data[j] as u32)
                .wrapping_add((data[j + 1] as u32) << 8)
                .wrapping_add((data[j + 2] as u32) << 16)
                .wrapping_add((data[j + 3] as u32) << 24);
        }

        let mut a = ctx.state[0];
        let mut b = ctx.state[1];
        let mut c = ctx.state[2];
        let mut d = ctx.state[3];

        macro_rules! r {
            ($f:expr, $a:ident, $b:ident, $c:ident, $d:ident, $m:expr, $s:expr, $t:expr) => {
                step($f, &mut $a, $b, $c, $d, $m, $s, $t)
            };
        }

        r!(f, a, b, c, d, m[0], 7, 0xd76aa478);
        r!(f, d, a, b, c, m[1], 12, 0xe8c7b756);
        r!(f, c, d, a, b, m[2], 17, 0x242070db);
        r!(f, b, c, d, a, m[3], 22, 0xc1bdceee);
        r!(f, a, b, c, d, m[4], 7, 0xf57c0faf);
        r!(f, d, a, b, c, m[5], 12, 0x4787c62a);
        r!(f, c, d, a, b, m[6], 17, 0xa8304613);
        r!(f, b, c, d, a, m[7], 22, 0xfd469501);
        r!(f, a, b, c, d, m[8], 7, 0x698098d8);
        r!(f, d, a, b, c, m[9], 12, 0x8b44f7af);
        r!(f, c, d, a, b, m[10], 17, 0xffff5bb1);
        r!(f, b, c, d, a, m[11], 22, 0x895cd7be);
        r!(f, a, b, c, d, m[12], 7, 0x6b901122);
        r!(f, d, a, b, c, m[13], 12, 0xfd987193);
        r!(f, c, d, a, b, m[14], 17, 0xa679438e);
        r!(f, b, c, d, a, m[15], 22, 0x49b40821);

        r!(g, a, b, c, d, m[1], 5, 0xf61e2562);
        r!(g, d, a, b, c, m[6], 9, 0xc040b340);
        r!(g, c, d, a, b, m[11], 14, 0x265e5a51);
        r!(g, b, c, d, a, m[0], 20, 0xe9b6c7aa);
        r!(g, a, b, c, d, m[5], 5, 0xd62f105d);
        r!(g, d, a, b, c, m[10], 9, 0x02441453);
        r!(g, c, d, a, b, m[15], 14, 0xd8a1e681);
        r!(g, b, c, d, a, m[4], 20, 0xe7d3fbc8);
        r!(g, a, b, c, d, m[9], 5, 0x21e1cde6);
        r!(g, d, a, b, c, m[14], 9, 0xc33707d6);
        r!(g, c, d, a, b, m[3], 14, 0xf4d50d87);
        r!(g, b, c, d, a, m[8], 20, 0x455a14ed);
        r!(g, a, b, c, d, m[13], 5, 0xa9e3e905);
        r!(g, d, a, b, c, m[2], 9, 0xfcefa3f8);
        r!(g, c, d, a, b, m[7], 14, 0x676f02d9);
        r!(g, b, c, d, a, m[12], 20, 0x8d2a4c8a);

        r!(h, a, b, c, d, m[5], 4, 0xfffa3942);
        r!(h, d, a, b, c, m[8], 11, 0x8771f681);
        r!(h, c, d, a, b, m[11], 16, 0x6d9d6122);
        r!(h, b, c, d, a, m[14], 23, 0xfde5380c);
        r!(h, a, b, c, d, m[1], 4, 0xa4beea44);
        r!(h, d, a, b, c, m[4], 11, 0x4bdecfa9);
        r!(h, c, d, a, b, m[7], 16, 0xf6bb4b60);
        r!(h, b, c, d, a, m[10], 23, 0xbebfbc70);
        r!(h, a, b, c, d, m[13], 4, 0x289b7ec6);
        r!(h, d, a, b, c, m[0], 11, 0xeaa127fa);
        r!(h, c, d, a, b, m[3], 16, 0xd4ef3085);
        r!(h, b, c, d, a, m[6], 23, 0x04881d05);
        r!(h, a, b, c, d, m[9], 4, 0xd9d4d039);
        r!(h, d, a, b, c, m[12], 11, 0xe6db99e5);
        r!(h, c, d, a, b, m[15], 16, 0x1fa27cf8);
        r!(h, b, c, d, a, m[2], 23, 0xc4ac5665);

        r!(i, a, b, c, d, m[0], 6, 0xf4292244);
        r!(i, d, a, b, c, m[7], 10, 0x432aff97);
        r!(i, c, d, a, b, m[14], 15, 0xab9423a7);
        r!(i, b, c, d, a, m[5], 21, 0xfc93a039);
        r!(i, a, b, c, d, m[12], 6, 0x655b59c3);
        r!(i, d, a, b, c, m[3], 10, 0x8f0ccc92);
        r!(i, c, d, a, b, m[10], 15, 0xffeff47d);
        r!(i, b, c, d, a, m[1], 21, 0x85845dd1);
        r!(i, a, b, c, d, m[8], 6, 0x6fa87e4f);
        r!(i, d, a, b, c, m[15], 10, 0xfe2ce6e0);
        r!(i, c, d, a, b, m[6], 15, 0xa3014314);
        r!(i, b, c, d, a, m[13], 21, 0x4e0811a1);
        r!(i, a, b, c, d, m[4], 6, 0xf7537e82);
        r!(i, d, a, b, c, m[11], 10, 0xbd3af235);
        r!(i, c, d, a, b, m[2], 15, 0x2ad7d2bb);
        r!(i, b, c, d, a, m[9], 21, 0xeb86d391);

        ctx.state[0] = ctx.state[0].wrapping_add(a);
        ctx.state[1] = ctx.state[1].wrapping_add(b);
        ctx.state[2] = ctx.state[2].wrapping_add(c);
        ctx.state[3] = ctx.state[3].wrapping_add(d);
    }

    /// Port of `MD5_HASH::md5_init`.
    fn md5_init(ctx: &mut Md5Ctx) {
        ctx.data = [0; 64];
        ctx.datalen = 0;
        ctx.bitlen = [0, 0];
        ctx.state = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    }

    /// Port of `MD5_HASH::md5_update`.
    fn md5_update(ctx: &mut Md5Ctx, a_data: &[u8]) {
        for &byte in a_data {
            ctx.data[ctx.datalen as usize] = byte;
            ctx.datalen += 1;

            if ctx.datalen == 64 {
                Self::md5_transform(ctx);
                let (lo, hi) = ctx.bitlen.split_at_mut(1);
                dbl_int_add(&mut lo[0], &mut hi[0], 512);
                ctx.datalen = 0;
            }
        }
    }

    /// Port of `MD5_HASH::md5_final`.
    fn md5_final(ctx: &mut Md5Ctx, hash: &mut [u8; 16]) {
        let mut i = ctx.datalen as usize;

        // Pad whatever data is left in the buffer.
        if ctx.datalen < 56 {
            ctx.data[i] = 0x80;
            i += 1;

            while i < 56 {
                ctx.data[i] = 0x00;
                i += 1;
            }
        } else {
            ctx.data[i] = 0x80;
            i += 1;

            while i < 64 {
                ctx.data[i] = 0x00;
                i += 1;
            }

            Self::md5_transform(ctx);
            ctx.data[..56].fill(0);
        }

        // Append to the padding the total message's length in bits and transform.
        let add = ctx.datalen.wrapping_mul(8);
        let (lo, hi) = ctx.bitlen.split_at_mut(1);
        dbl_int_add(&mut lo[0], &mut hi[0], add);
        ctx.data[56..60].copy_from_slice(&ctx.bitlen[0].to_le_bytes());
        ctx.data[60..64].copy_from_slice(&ctx.bitlen[1].to_le_bytes());
        Self::md5_transform(ctx);

        // Since this implementation uses little endian byte ordering and MD uses big endian,
        // reverse all the bytes when copying the final state to the output hash.
        for k in 0..4 {
            hash[k] = (ctx.state[0] >> (k * 8)) as u8;
            hash[k + 4] = (ctx.state[1] >> (k * 8)) as u8;
            hash[k + 8] = (ctx.state[2] >> (k * 8)) as u8;
            hash[k + 12] = (ctx.state[3] >> (k * 8)) as u8;
        }
    }
}
