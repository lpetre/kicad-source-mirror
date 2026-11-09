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
// The generator follows boost/random/mersenne_twister.hpp:
//   Copyright Jens Maurer 2000-2001
//   Copyright Steven Watanabe 2010
//   Distributed under the Boost Software License, Version 1.0. (See
//   accompanying file LICENSE_1_0.txt or copy at
//   http://www.boost.org/LICENSE_1_0.txt)

//! Port of `boost::mt19937` (`boost::random::mersenne_twister_engine` with the MT19937
//! parameters), the pseudo-random number generator behind `KIID` generation in
//! `common/kiid.cpp`.
//!
//! The algorithm is described in "Mersenne Twister: A 623-dimensionally equidistributed
//! uniform pseudo-random number generator", Makoto Matsumoto and Takuji Nishimura, ACM
//! Transactions on Modeling and Computer Simulation: Special Issue on Uniform Random Number
//! Generation, Vol. 8, No. 1, January 1998, pp. 3-30.

/// Word size `w`.
const W: u32 = 32;
/// State size `n`.
const N: usize = 624;
/// Shift size `m`.
const M: usize = 397;
/// Mask bits `r`.
const R: u32 = 31;
/// Xor mask `a`.
const A: u32 = 0x9908_b0df;
/// Tempering parameters `u`, `d`, `s`, `b`, `t`, `c`, `l`.
const U: u32 = 11;
const D: u32 = 0xffff_ffff;
const S: u32 = 7;
const B: u32 = 0x9d2c_5680;
const T: u32 = 15;
const C: u32 = 0xefc6_0000;
const L: u32 = 18;
/// Initialization multiplier `f`.
const F: u32 = 1_812_433_253;

const UPPER_MASK: u32 = u32::MAX << R;
const LOWER_MASK: u32 = !UPPER_MASK;

/// Port of `boost::mt19937`.
#[derive(Clone)]
pub struct Mt19937 {
    /// State representation: next output is o(x(i)).
    x: [u32; N],
    i: usize,
}

impl Mt19937 {
    /// Number of 32 bit words of the generator state (`state_size`).
    pub const STATE_SIZE: usize = N;

    /// The seed used by the default constructor (`default_seed`).
    pub const DEFAULT_SEED: u32 = 5489;

    /// Port of the default constructor: calls `seed( default_seed )`.
    pub fn new() -> Self {
        Self::from_seed(Self::DEFAULT_SEED)
    }

    /// Port of the constructor taking a seed value: calls `seed( value )`.
    pub fn from_seed(value: u32) -> Self {
        let mut rng = Mt19937 { x: [0; N], i: N };
        rng.seed(value);
        rng
    }

    /// Port of the constructor taking a seed sequence, with the 624 words the sequence would
    /// generate given directly (see [`Mt19937::seed_words`]).
    pub fn from_seed_words(words: &[u32; N]) -> Self {
        let mut rng = Mt19937 { x: [0; N], i: N };
        rng.seed_words(words);
        rng
    }

    /// Port of `seed( value )`.
    ///
    /// Sets the state x(0) to v mod 2^w.  Then, iteratively, sets x(i) to
    /// (i + f * (x(i-1) xor (x(i-1) rshift w-2))) mod 2^w for i = 1 .. n-1.  x(n) is the first
    /// value to be returned by `next_u32`.
    pub fn seed(&mut self, value: u32) {
        // New seeding algorithm from
        // http://www.math.sci.hiroshima-u.ac.jp/~m-mat/MT/MT2002/emt19937ar.html
        // In the previous versions, MSBs of the seed affected only MSBs of the
        // state x[].
        self.x[0] = value;

        for i in 1..N {
            // See Knuth "The Art of Computer Programming"
            // Vol. 2, 3rd ed., page 106
            self.x[i] = F
                .wrapping_mul(self.x[i - 1] ^ (self.x[i - 1] >> (W - 2)))
                .wrapping_add(i as u32);
        }

        self.i = N;

        self.normalize_state();
    }

    /// Port of `seed( SeedSeq& )`: seeds the generator using the 624 values a seed sequence's
    /// `generate()` would produce.
    pub fn seed_words(&mut self, words: &[u32; N]) {
        self.x = *words;
        self.i = N;

        self.normalize_state();
    }

    /// Port of `normalize_state`.
    ///
    /// Converts an arbitrary array into a valid generator state.  First we normalize x[0], so
    /// that it contains the same value we would get by running the generator forwards and then
    /// in reverse.  (The low order r bits are redundant).  Then, if the state consists of all
    /// zeros, we set the high order bit of x[0] to 1.  This function only needs to be called
    /// by seed, since the state transform preserves this relationship.
    fn normalize_state(&mut self) {
        let mut y0 = self.x[M - 1] ^ self.x[N - 1];

        if (y0 & (1u32 << (W - 1))) != 0 {
            y0 = ((y0 ^ A) << 1) | 1;
        } else {
            y0 <<= 1;
        }

        self.x[0] = (self.x[0] & UPPER_MASK) | (y0 & LOWER_MASK);

        // fix up the state if it's all zeroes.
        if self.x.iter().any(|&word| word != 0) {
            return;
        }

        self.x[0] = 1u32 << (W - 1);
    }

    /// Port of `twist`.
    fn twist(&mut self) {
        let x = &mut self.x;

        for j in 0..N - M {
            let y = (x[j] & UPPER_MASK) | (x[j + 1] & LOWER_MASK);
            x[j] = x[j + M] ^ (y >> 1) ^ ((x[j + 1] & 1).wrapping_mul(A));
        }

        for j in N - M..N - 1 {
            let y = (x[j] & UPPER_MASK) | (x[j + 1] & LOWER_MASK);
            x[j] = x[j - (N - M)] ^ (y >> 1) ^ ((x[j + 1] & 1).wrapping_mul(A));
        }

        // last iteration
        let y = (x[N - 1] & UPPER_MASK) | (x[0] & LOWER_MASK);
        x[N - 1] = x[M - 1] ^ (y >> 1) ^ ((x[0] & 1).wrapping_mul(A));

        self.i = 0;
    }

    /// Port of `operator()`: produces the next value of the generator.
    pub fn next_u32(&mut self) -> u32 {
        if self.i == N {
            self.twist();
        }

        // Step 4
        let mut z = self.x[self.i];
        self.i += 1;
        z ^= (z >> U) & D;
        z ^= (z << S) & B;
        z ^= (z << T) & C;
        z ^= z >> L;
        z
    }
}

impl Default for Mt19937 {
    fn default() -> Self {
        Mt19937::new()
    }
}

impl std::fmt::Debug for Mt19937 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt19937")
            .field("i", &self.i)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seed_reference_values() {
        // The C++ standard requires the 10000th consecutive invocation of a
        // default-constructed std::mt19937 to produce 4123659995.
        let mut rng = Mt19937::new();
        let mut last = 0;

        for _ in 0..10000 {
            last = rng.next_u32();
        }

        assert_eq!(last, 4_123_659_995);
    }

    #[test]
    fn seeded_outputs_match_boost() {
        // First four outputs of boost::mt19937 for a few seeds (boost 1.83).
        let cases: [(u32, [u32; 4]); 5] = [
            (0, [2357136044, 2546248239, 3071714933, 3626093760]),
            (1, [1791095845, 4282876139, 3093770124, 4005303368]),
            (5489, [3499211612, 581869302, 3890346734, 3586334585]),
            (12345, [3992670690, 3823185381, 1358822685, 561383553]),
            (0xFFFF_FFFF, [419326371, 479346978, 3918654476, 2416749639]),
        ];

        for (seed, expected) in cases {
            let mut rng = Mt19937::from_seed(seed);

            for value in expected {
                assert_eq!(rng.next_u32(), value, "seed {seed}");
            }
        }
    }

    #[test]
    fn all_zero_seed_words_are_fixed_up() {
        let mut rng = Mt19937::from_seed_words(&[0; Mt19937::STATE_SIZE]);

        // An all-zero state would only ever produce zeros.
        assert!((0..2000).any(|_| rng.next_u32() != 0));
    }

    #[test]
    fn seed_words_equal_to_seed_state() {
        // Seeding from the words seed( value ) computes gives the same sequence.
        let mut words = [0u32; Mt19937::STATE_SIZE];
        words[0] = 4242;

        for i in 1..Mt19937::STATE_SIZE {
            words[i] = F
                .wrapping_mul(words[i - 1] ^ (words[i - 1] >> 30))
                .wrapping_add(i as u32);
        }

        let mut a = Mt19937::from_seed(4242);
        let mut b = Mt19937::from_seed_words(&words);

        for _ in 0..1500 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
