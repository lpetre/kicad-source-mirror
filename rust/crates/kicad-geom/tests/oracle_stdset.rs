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

//! Differential test of [`kicad_geom::stdsort::StdSet`] / [`std_set_order`] against libstdc++
//! `std::set`: replays the cases of `oracle/stdset_oracle.cpp` (6,000 random sequences, with
//! comparators that are and deliberately are not strict weak orderings, all irreflexive and
//! asymmetric) and compares the
//! iteration order with `golden/stdset_oracle.txt` (regenerate with
//! `oracle/build_stdset_oracle.sh`).

use kicad_geom::stdsort::{StdSet, std_set_order};

const CASES: i32 = 6000;
const BATCH: i32 = 100;
const VERBOSE_CASES: i32 = 40;

/// splitmix64, as `RNG` in the driver.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

/// FNV-1a 64 over the little-endian bytes of each value.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn add(&mut self, v: i64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
}

/// `CMP::operator()` of the driver.
fn less(kind: i64, eps: i64, table: &[u8], size: i64, a: i64, b: i64) -> bool {
    match kind {
        0 => a < b,
        1 => a < b - eps,
        2 => table[(a * size + b) as usize] != 0,
        3 => (b - a).rem_euclid(3) == 1,
        4 => a < b && b - a <= eps + 1,
        _ => a.abs() < b.abs() - eps,
    }
}

#[test]
fn std_set_matches_libstdcxx() {
    let golden = include_str!("golden/stdset_oracle.txt");
    let mut expected = golden.lines();
    let mut rng = Rng(20261110);
    let mut batch = Fnv::new();
    let mut checked = 0;

    for c in 0..CASES {
        let kind = rng.below(6);
        let n = if rng.below(4) == 0 {
            rng.below(400)
        } else {
            rng.below(40)
        };
        let range = 1 + rng.below(if kind == 2 { 30 } else { 200 });
        let eps = rng.below(6);
        let hinted = rng.below(2) == 0;

        let mut table = Vec::new();

        if kind == 2 {
            table = vec![0u8; (range * range) as usize];

            for a in 0..range {
                for b in a + 1..range {
                    match rng.below(3) {
                        1 => table[(a * range + b) as usize] = 1,
                        2 => table[(b * range + a) as usize] = 1,
                        _ => {}
                    }
                }
            }
        }

        let items: Vec<i64> = (0..n)
            .map(|_| {
                if kind == 5 {
                    rng.below(2 * range) - range
                } else {
                    rng.below(range)
                }
            })
            .collect();

        let cmp = |a: &i64, b: &i64| less(kind, eps, &table, range, *a, *b);

        let out: Vec<i64> = if hinted {
            std_set_order(items, cmp)
        } else {
            let mut set = StdSet::new();

            for v in items {
                set.insert(v, cmp);
            }

            assert_eq!(set.iter().count(), set.len());
            set.into_vec()
        };

        if c < VERBOSE_CASES {
            let text: Vec<String> = out.iter().map(i64::to_string).collect();
            assert_eq!(
                Some(format!("set|{c}|{}", text.join(" ")).as_str()),
                expected.next(),
                "case {c} (kind {kind}, hinted {hinted})"
            );
        }

        batch.add(out.len() as i64);

        for v in &out {
            batch.add(*v);
        }

        if c % BATCH == BATCH - 1 {
            assert_eq!(
                Some(format!("hash|{}|{:016x}", c + 1 - BATCH, batch.0).as_str()),
                expected.next(),
                "batch starting at case {}",
                c + 1 - BATCH
            );
            batch = Fnv::new();
            checked += BATCH;
        }
    }

    assert_eq!(expected.next(), None);
    assert_eq!(checked, CASES);
}
