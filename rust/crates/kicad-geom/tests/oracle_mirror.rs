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

//! Differential test of the mirror helpers ([`kicad_geom::mirror_point`],
//! `ShapeLineChain::mirror`, `ShapeArc::mirror`) against MIRROR() of `include/core/mirror.h`
//! and libkimath of the reference build, with coordinates whose mirror image is outside the
//! `int` range (the reference build wraps): replays `oracle/mirror_oracle.cpp` and compares
//! with `golden/mirror_oracle.txt` (regenerate with `oracle/build_mirror_oracle.sh`).

use kicad_geom::{FlipDirection, ShapeArc, ShapeLineChain, Vector2I, mirror_point};

const CASES: i32 = 4000;
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

    /// `coord()` of the driver.
    fn coord(&mut self) -> i32 {
        const TABLE: [i32; 11] = [
            i32::MAX,
            i32::MIN,
            i32::MAX - 7,
            i32::MIN + 7,
            2000000000,
            -2000000000,
            1100000000,
            -1100000000,
            0,
            1,
            -1,
        ];

        match self.below(4) {
            0 => TABLE[self.below(11) as usize],
            1 => (self.below(4294967296) - 2147483648) as i32,
            _ => (self.below(2000001) - 1000000) as i32,
        }
    }
}

#[test]
fn mirror_matches_libkimath() {
    let golden = include_str!("golden/mirror_oracle.txt");
    let mut expected = golden.lines();
    let mut rng = Rng(4242);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for c in 0..CASES {
        let mut p = [Vector2I::new(0, 0); 3];

        for v in &mut p {
            let x = rng.coord();
            let y = rng.coord();
            *v = Vector2I::new(x, y);
        }

        let rx = rng.coord();
        let ry = rng.coord();
        let r = Vector2I::new(rx, ry);
        let dir = if rng.below(2) != 0 {
            FlipDirection::LeftRight
        } else {
            FlipDirection::TopBottom
        };
        let with_arc = rng.below(4) == 0;

        let mut out: Vec<i32> = Vec::new();

        let mut m = p[0];
        mirror_point(&mut m, r, dir);
        out.extend([m.x, m.y]);

        let mut chain = ShapeLineChain::new();

        for v in p {
            chain.append_point(v, true);
        }

        chain.mirror(r, dir);

        for v in chain.c_points() {
            out.extend([v.x, v.y]);
        }

        if with_arc {
            let mut a = [Vector2I::new(0, 0); 3];

            for v in &mut a {
                let x = (rng.below(2000001) - 1000000) as i32;
                let y = (rng.below(2000001) - 1000000) as i32;
                *v = Vector2I::new(x, y);
            }

            let aref = Vector2I::new(rx / 4, ry / 4);
            let mut arc = ShapeArc::from_start_mid_end(a[0], a[1], a[2], 0);
            arc.mirror(aref, dir);

            for v in [arc.get_p0(), arc.get_arc_mid(), arc.get_p1()] {
                out.extend([v.x, v.y]);
            }
        }

        let line: Vec<String> = out.iter().map(i32::to_string).collect();
        let line = line.join(" ");

        if c < VERBOSE_CASES {
            assert_eq!(
                Some(format!("m|{c}|{line}").as_str()),
                expected.next(),
                "case {c}"
            );
        }

        for b in format!(" {line}\n").bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }

        if c % BATCH == BATCH - 1 {
            assert_eq!(
                Some(format!("hash|{}|{hash:016x}", c + 1 - BATCH).as_str()),
                expected.next(),
                "batch starting at case {}",
                c + 1 - BATCH
            );
            hash = 0xcbf2_9ce4_8422_2325;
        }
    }

    assert_eq!(expected.next(), None);
}
