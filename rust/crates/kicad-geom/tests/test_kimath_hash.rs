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

//! Port of `TestHash128` and `TestMMH3Hash` from `qa/tests/libs/kimath/test_kimath.cpp` (suite
//! `KiMath`; the other cases are in `test_kimath.rs`).  `TestMMH3Hash` draws its data with
//! glibc `rand()` in its default state (seed 1), replicated here, so the inputs are the same
//! as in the reference build.

use kicad_geom::hash_128::Hash128;
use kicad_geom::mmh3_hash::{Mmh3Hash, murmur_hash3_x64_128};

/// glibc `rand()` (`random_r`, `TYPE_3`, default seed 1).
struct GlibcRand {
    r: Vec<i32>,
    i: usize,
}

impl GlibcRand {
    fn new() -> Self {
        let mut r: Vec<i32> = vec![0; 34];
        r[0] = 1;

        for i in 1..31 {
            r[i] = ((16807 * r[i - 1] as i64) % 2147483647) as i32;

            if r[i] < 0 {
                r[i] += 2147483647;
            }
        }

        for i in 31..34 {
            r[i] = r[i - 31];
        }

        let mut g = GlibcRand { r, i: 34 };

        for _ in 34..344 {
            g.step();
        }

        g
    }

    fn step(&mut self) -> u32 {
        let v = (self.r[self.i - 31] as u32).wrapping_add(self.r[self.i - 3] as u32);
        self.r.push(v as i32);
        self.i += 1;
        v
    }

    fn rand(&mut self) -> i32 {
        (self.step() >> 1) as i32
    }
}

#[test]
fn glibc_rand_replica() {
    // The first values of glibc rand() after srand( 1 )
    let mut g = GlibcRand::new();
    assert_eq!(g.rand(), 1804289383);
    assert_eq!(g.rand(), 846930886);
    assert_eq!(g.rand(), 1681692777);
}

#[test]
fn test_hash128() {
    let zero = Hash128::default();

    for b in zero.value8() {
        assert_eq!(b, 0);
    }

    let zero_h = Box::new(Hash128::new());

    for b in zero_h.value8() {
        assert_eq!(b, 0);
    }

    let mut h = Hash128::default();
    h.value64[0] = 0x00CDEF0123456789;
    h.value64[1] = 0x56789ABCDEF01234;

    assert!(h != zero);

    let b = h;
    assert!(b != zero);

    assert!(b == h);
    assert!(h == b);
    assert_eq!(h.to_string_hex(), "00CDEF012345678956789ABCDEF01234");
}

/// Test MMH3_HASH against the original MurmurHash3_x64_128 implementation
#[test]
fn test_mmh3_hash() {
    let mut rng = GlibcRand::new();
    let mut data: Vec<Vec<i32>> = Vec::new();

    for _ in 0..10 {
        let vec_size = rng.rand() % 128;
        let vec: Vec<i32> = (0..vec_size).map(|_| rng.rand()).collect();
        data.push(vec);
    }

    let mut seed: i64 = 0; // Test 0 seed first

    for vec in &data {
        let mut mmh3 = Mmh3Hash::new(seed as u32);

        for &val in vec {
            mmh3.add_i32(val);
        }

        let h128 = mmh3.digest();

        let bytes: Vec<u8> = vec.iter().flat_map(|v| v.to_le_bytes()).collect();
        let orig128 = murmur_hash3_x64_128(&bytes, seed as u32);

        assert!(h128 == orig128);

        // Generate new random seed
        seed = rng.rand() as i64;
    }
}

/// Additional: the streaming `addData` gives the one-shot hash for any split of the input.
#[test]
fn mmh3_add_data_in_pieces() {
    let bytes: Vec<u8> = (0..200u32).map(|i| (i * 37 + 11) as u8).collect();

    for len in [0usize, 1, 7, 15, 16, 17, 31, 32, 33, 100, 200] {
        let reference = murmur_hash3_x64_128(&bytes[..len], 42);

        for piece in [1usize, 3, 5, 16, 50] {
            let mut h = Mmh3Hash::new(42);

            for chunk in bytes[..len].chunks(piece) {
                h.add_data(chunk);
            }

            assert_eq!(h.digest(), reference, "len {len} piece {piece}");
        }
    }
}
