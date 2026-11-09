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

//! SHA-1 (FIPS 180-4), as needed by `KIID::FromName`, which uses
//! `boost::uuids::name_generator_sha1` (`boost/uuid/detail/sha1.hpp`).
//!
//! This is only used to derive name based (version 5) UUIDs; it is not meant as a general
//! purpose or security relevant hash.

/// Compute the SHA-1 digest of the concatenation of `parts`.
pub(crate) fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    let mut block = [0u8; 64];
    let mut block_len = 0usize;
    let mut total_len: u64 = 0;

    for part in parts {
        for &byte in *part {
            block[block_len] = byte;
            block_len += 1;
            total_len += 1;

            if block_len == 64 {
                process_block(&mut h, &block);
                block_len = 0;
            }
        }
    }

    // Padding: a single 1 bit, zeros, then the message length in bits as a 64 bit big-endian
    // number.
    let bit_len = total_len.wrapping_mul(8);

    block[block_len] = 0x80;
    block_len += 1;

    if block_len > 56 {
        block[block_len..].fill(0);
        process_block(&mut h, &block);
        block_len = 0;
    }

    block[block_len..56].fill(0);
    block[56..].copy_from_slice(&bit_len.to_be_bytes());
    process_block(&mut h, &block);

    let mut digest = [0u8; 20];

    for (i, word) in h.iter().enumerate() {
        digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }

    digest
}

fn process_block(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];

    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let [mut a, mut b, mut c, mut d, mut e] = *h;

    for (i, &word) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };

        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(word);

        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(digest: [u8; 20]) -> String {
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn fips_180_test_vectors() {
        assert_eq!(
            hex(sha1(&[b""])),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex(sha1(&[b"abc"])),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(sha1(&[
                b"abcdbcdecdefdefgefghfghighijhijk",
                b"ijkljklmklmnlmnomnopnopq"
            ])),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );

        let million_a = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(sha1(&[&million_a])),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn padding_boundaries() {
        // 55, 56 and 64 byte messages exercise the one/two block padding paths.
        assert_eq!(
            hex(sha1(&[&[b'a'; 55]])),
            "c1c8bbdc22796e28c0e15163d20899b65621d65a"
        );
        assert_eq!(
            hex(sha1(&[&[b'a'; 56]])),
            "c2db330f6083854c99d4b5bfb6e8f29f201be699"
        );
        assert_eq!(
            hex(sha1(&[&[b'a'; 64]])),
            "0098ba824b5c16427bd7a1122a5a442a25ec644d"
        );
    }
}
