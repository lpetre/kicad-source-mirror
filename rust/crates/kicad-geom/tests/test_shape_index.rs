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

//! Tests of `SHAPE_INDEX` (`shape_index.h`) and `SHAPE_INDEX_LIST` (`shape_index_list.h`).
//! The C++ QA has no tests for them; these check the documented behaviour.

use std::sync::Arc;

use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_circle::ShapeCircle;
use kicad_geom::geometry::shape_index::{ShapeIndex, SharedShape};
use kicad_geom::geometry::shape_index_list::ShapeIndexList;
use kicad_geom::geometry::shape_rect::ShapeRect;
use kicad_geom::math::vector2::Vector2I;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

fn circles() -> Vec<SharedShape> {
    (0..50)
        .map(|i| SharedShape(Arc::new(ShapeCircle::new(v(i * 100, 0), 10)) as Arc<dyn Shape>))
        .collect()
}

#[test]
fn shape_index_query_add_remove() {
    let items = circles();
    let mut index = ShapeIndex::new(-1);

    for it in &items {
        index.add(it.clone());
    }

    assert_eq!(index.size(), 50);

    let probe = ShapeRect::from_corners(v(450, -5), v(560, 5));
    let mut found = Vec::new();
    let n = index.query(&probe, 0, |s: &SharedShape| {
        found.push(s.0.bbox(0).centre().x);
        true
    });

    found.sort();
    assert_eq!(n, 1);
    assert_eq!(found, vec![500]);

    // inflated by the minimum distance
    let n = index.query(&probe, 60, |_| true);
    assert_eq!(n, 3);

    // early stop
    let n = index.query(&probe, 60, |_| false);
    assert_eq!(n, 1);

    index.remove(&items[5]);
    assert_eq!(index.size(), 49);
    assert_eq!(index.query(&probe, 0, |_| true), 0);

    let clone = index.clone_index();
    assert_eq!(clone.size(), 49);

    let mut visited = 0;
    index.accept(|_| visited += 1);
    assert_eq!(visited, 49);

    index.reindex();
    assert_eq!(index.size(), 49);

    let bulk: Vec<_> = items.iter().map(|s| (s.clone(), s.0.bbox(0))).collect();
    index.bulk_load(bulk);
    assert_eq!(index.size(), 50);
    assert_eq!(index.iter().count(), 50);

    index.remove_all();
    assert_eq!(index.size(), 0);
}

#[test]
fn shape_index_list_query() {
    let items = circles();
    let mut list = ShapeIndexList::new();

    for it in &items {
        list.add(it.clone());
    }

    assert_eq!(list.size(), 50);

    let probe = ShapeRect::from_corners(v(450, -5), v(560, 5));

    // bounding boxes only
    let mut order = Vec::new();
    let n = list.query(
        &probe,
        50,
        |s: &SharedShape| {
            order.push(s.0.bbox(0).centre().x);
            true
        },
        false,
    );
    assert_eq!(n, 3);
    assert_eq!(order, vec![400, 500, 600]);

    // exact: the circles at 400 and 600 are 40 and 30 away from the probe
    assert_eq!(list.query(&probe, 30, |_| true, true), 1);
    assert_eq!(list.qbegin(&probe, 30, true).count(), 1);
    assert_eq!(list.qbegin(&probe, 31, true).count(), 2);
    assert_eq!(list.qbegin(&probe, 50, false).count(), 3);

    list.remove(&items[4]);
    assert_eq!(list.size(), 49);
    assert_eq!(list.iter().count(), 49);

    list.clear();
    assert_eq!(list.size(), 0);
}
