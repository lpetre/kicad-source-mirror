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

//! `ShapeLineChain::insert_points_descending` against the `insert_point` calls it replaces.

use kicad_geom::Vector2I;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;

fn chain(n: i32) -> ShapeLineChain {
    let mut c = ShapeLineChain::new();
    for i in 0..n {
        c.append_point(Vector2I::new(i * 10, i * i), false);
    }
    c
}

fn points(c: &ShapeLineChain) -> Vec<Vector2I> {
    (0..c.point_count()).map(|i| c.c_point(i)).collect()
}

#[test]
fn matches_repeated_insert_point() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut below = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };

    for _ in 0..2000 {
        let n = 2 + below(40) as i32;
        let mut inserts: Vec<(usize, Vector2I)> = (0..below(30))
            .map(|k| {
                let at = below(n as u64) as usize;
                (at, Vector2I::new(-1 - k as i32, at as i32))
            })
            .collect();
        // stable, as `connect_nearby_polys` sorts them: equal positions keep their order
        inserts.sort_by_key(|i| std::cmp::Reverse(i.0));

        let mut one_by_one = chain(n);
        for &(at, p) in &inserts {
            one_by_one.insert_point(at, p);
        }

        let mut batched = chain(n);
        assert!(batched.insert_points_descending(&inserts));
        assert_eq!(points(&batched), points(&one_by_one));
    }
}

#[test]
fn leaves_the_chain_alone_when_it_does_not_apply() {
    let mut c = chain(4);
    let before = points(&c);
    let p = Vector2I::new(-1, -1);

    assert!(!c.insert_points_descending(&[(4, p)])); // insert_point appends at the end
    assert!(!c.insert_points_descending(&[(1, p), (2, p)])); // not descending
    assert_eq!(points(&c), before);
}
