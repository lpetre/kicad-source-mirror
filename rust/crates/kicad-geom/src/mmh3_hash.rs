// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2024 Alex Shvartzkop <dudesuchamazing@gmail.com>
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
// Based on https://github.com/aappleby/smhasher/blob/master/src/MurmurHash3.cpp
// MurmurHash3 was written by Austin Appleby, and is placed in the public
// domain. The author hereby disclaims copyright to this source code.

//! Port of `libs/kimath/include/mmh3_hash.h` (`MMH3_HASH`, a streaming MurmurHash3_x64_128).
//!
//! The C++ block buffer is a union of `uint32_t[4]` and `uint64_t[2]`; the port keeps it as
//! 16 bytes and reads / writes the words little endian, as the reference platform does.  The
//! block counter `len` is a `uint32_t` and wraps like it.

use crate::hash_128::Hash128;

const C1: u64 = 0x87c3_7b91_1142_53d5;
const C2: u64 = 0x4cf5_ad43_2745_937f;

/// Port of `MMH3_HASH`: a streaming C++ equivalent for MurmurHash3_x64_128.
#[derive(Clone, Debug)]
pub struct Mmh3Hash {
    h1: u64,
    h2: u64,
    /// the `blocks` / `blocks64` union
    blocks: [u8; 16],
    len: u32,
}

impl Default for Mmh3Hash {
    /// `MMH3_HASH()`: seed 0.
    fn default() -> Self {
        Self::new(0)
    }
}

impl Mmh3Hash {
    /// `MMH3_HASH( uint32_t aSeed )`.
    ///
    /// The C++ leaves the block buffer uninitialised; it is zeroed here.  No sequence of public
    /// calls reads a buffer byte that was not written before.
    pub fn new(a_seed: u32) -> Self {
        let mut h = Mmh3Hash {
            h1: 0,
            h2: 0,
            blocks: [0; 16],
            len: 0,
        };
        h.reset(a_seed);
        h
    }

    /// Port of `MMH3_HASH::reset`.
    pub fn reset(&mut self, a_seed: u32) {
        self.h1 = a_seed as u64;
        self.h2 = a_seed as u64;
        self.len = 0;
    }

    /// Port of `MMH3_HASH::addData`.
    pub fn add_data(&mut self, a_data: &[u8]) {
        let mut data = a_data;
        let mut remaining = data.len();
        let pending = (self.len % 16) as usize;

        // Complete a block an earlier call left partial, so data may arrive in pieces of any size
        if pending > 0 && remaining > 0 {
            let fill = remaining.min(16 - pending);
            self.blocks[pending..pending + fill].copy_from_slice(&data[..fill]);
            data = &data[fill..];
            remaining -= fill;
            self.len = self.len.wrapping_add(fill as u32);

            if self.len % 16 != 0 {
                return;
            }

            self.hash_block();
        }

        while remaining >= 16 {
            self.blocks.copy_from_slice(&data[..16]);
            self.hash_block();
            data = &data[16..];
            remaining -= 16;
            self.len = self.len.wrapping_add(16);
        }

        if remaining > 0 {
            self.blocks[..remaining].copy_from_slice(&data[..remaining]);
            self.blocks[remaining..].fill(0);
            self.len = self.len.wrapping_add(remaining as u32);
        }
    }

    /// Port of `MMH3_HASH::addDataV1`.
    ///
    /// Reproduces the pre-fix addData tail handling for reading files saved
    /// before the correction. The old code rounded remaining bytes to 4-byte
    /// alignment, counted padding in len, and only zeroed the padding bytes
    /// (not the full block remainder). This produced wrong hashes but files
    /// were saved with them, so we need to verify against them on load.
    pub fn add_data_v1(&mut self, a_data: &[u8]) {
        let mut data = a_data;
        let mut remaining = data.len();

        while remaining >= 16 {
            self.blocks.copy_from_slice(&data[..16]);
            self.hash_block();
            data = &data[16..];
            remaining -= 16;
            self.len = self.len.wrapping_add(16);
        }

        if remaining > 0 {
            self.blocks[..remaining].copy_from_slice(&data[..remaining]);
            let padding = 4 - (remaining + 4) % 4;
            self.blocks[remaining..remaining + padding].fill(0);
            self.len = self.len.wrapping_add((remaining + padding) as u32);
        }
    }

    /// Port of `MMH3_HASH::add( const std::string& )` and `add( const std::vector<char>& )`.
    pub fn add_bytes(&mut self, a_input: &[u8]) {
        self.add_data(a_input);
    }

    /// Port of `MMH3_HASH::add( const std::string& )` for a Rust string (its UTF-8 bytes).
    pub fn add_str(&mut self, a_input: &str) {
        self.add_data(a_input.as_bytes());
    }

    /// Port of `MMH3_HASH::add( int32_t )`.
    pub fn add_i32(&mut self, a_input: i32) {
        let word = ((self.len % 16) / 4) as usize * 4;
        self.blocks[word..word + 4].copy_from_slice(&a_input.to_le_bytes());
        self.len = self.len.wrapping_add(4);

        if self.len % 16 == 0 {
            self.hash_block();
        }
    }

    /// Port of `MMH3_HASH::digest`.
    pub fn digest(&mut self) -> Hash128 {
        self.hash_tail();

        let mut h128 = Hash128::new();
        self.hash_final(&mut h128);

        h128
    }

    fn get_block64(&self, i: usize) -> u64 {
        let mut word = [0u8; 8];
        word.copy_from_slice(&self.blocks[i * 8..i * 8 + 8]);
        u64::from_le_bytes(word)
    }

    /// Port of `MMH3_HASH::hashBlock`.
    fn hash_block(&mut self) {
        let mut k1 = self.get_block64(0);
        let mut k2 = self.get_block64(1);

        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(31);
        k1 = k1.wrapping_mul(C2);
        self.h1 ^= k1;

        self.h1 = self.h1.rotate_left(27);
        self.h1 = self.h1.wrapping_add(self.h2);
        self.h1 = self.h1.wrapping_mul(5).wrapping_add(0x52dc_e729);

        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(33);
        k2 = k2.wrapping_mul(C1);
        self.h2 ^= k2;

        self.h2 = self.h2.rotate_left(31);
        self.h2 = self.h2.wrapping_add(self.h1);
        self.h2 = self.h2.wrapping_mul(5).wrapping_add(0x3849_5ab5);
    }

    /// Port of `MMH3_HASH::hashTail` (the fall-through `switch( len & 15 )`).
    fn hash_tail(&mut self) {
        let tail = &self.blocks;
        let n = (self.len & 15) as usize;

        let mut k1: u64 = 0;
        let mut k2: u64 = 0;

        if n >= 9 {
            for i in (8..n).rev() {
                k2 ^= (tail[i] as u64) << ((i - 8) * 8);
            }

            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            self.h2 ^= k2;
        }

        if n >= 1 {
            for i in (0..n.min(8)).rev() {
                k1 ^= (tail[i] as u64) << (i * 8);
            }

            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            self.h1 ^= k1;
        }
    }

    /// Port of `MMH3_HASH::fmix64`: finalization mix - force all bits of a hash block to
    /// avalanche.
    fn fmix64(mut k: u64) -> u64 {
        k ^= k >> 33;
        k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
        k ^= k >> 33;
        k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        k ^= k >> 33;

        k
    }

    /// Port of `MMH3_HASH::hashFinal`.
    fn hash_final(&mut self, out: &mut Hash128) {
        self.h1 ^= self.len as u64;
        self.h2 ^= self.len as u64;

        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);

        self.h1 = Self::fmix64(self.h1);
        self.h2 = Self::fmix64(self.h2);

        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);

        out.value64[0] = self.h1;
        out.value64[1] = self.h2;
    }
}

/// The original one-shot `MurmurHash3_x64_128` (as copied into
/// `qa/tests/libs/kimath/test_kimath.cpp`), the reference `MMH3_HASH` is tested against.
pub fn murmur_hash3_x64_128(a_key: &[u8], a_seed: u32) -> Hash128 {
    let mut h = Mmh3Hash::new(a_seed);
    let nblocks = a_key.len() / 16;

    for i in 0..nblocks {
        h.blocks.copy_from_slice(&a_key[i * 16..i * 16 + 16]);
        h.hash_block();
    }

    let tail = &a_key[nblocks * 16..];
    h.blocks = [0; 16];
    h.blocks[..tail.len()].copy_from_slice(tail);
    // `len` is an `int` in the original; only its low 32 bits enter `h1 ^= len`, sign extended
    h.len = a_key.len() as u32;
    h.hash_tail();

    let mut out = Hash128::new();
    let len = a_key.len() as i32 as i64 as u64;
    h.h1 ^= len;
    h.h2 ^= len;
    h.h1 = h.h1.wrapping_add(h.h2);
    h.h2 = h.h2.wrapping_add(h.h1);
    h.h1 = Mmh3Hash::fmix64(h.h1);
    h.h2 = Mmh3Hash::fmix64(h.h2);
    h.h1 = h.h1.wrapping_add(h.h2);
    h.h2 = h.h2.wrapping_add(h.h1);
    out.value64 = [h.h1, h.h2];
    out
}
