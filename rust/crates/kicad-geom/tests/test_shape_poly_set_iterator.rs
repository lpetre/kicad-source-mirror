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

//! Port of `qa/tests/libs/kimath/geometry/test_shape_poly_set_iterator.cpp` (suite
//! `PolygonIterator`, fixture `IteratorFixture`).

mod common_poly;

use common_poly::CommonPolyData;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;

/// Port of `IteratorFixture`.
struct IteratorFixture {
    common: CommonPolyData,
    last_null_segment_poly_set: ShapePolySet,
    first_null_segment_poly_set: ShapePolySet,
    inside_null_segment_poly_set: ShapePolySet,
    null_points: Vec<Vector2I>,
}

impl IteratorFixture {
    fn new() -> Self {
        let null_points = vec![
            Vector2I::new(100, 100),
            Vector2I::new(0, 100),
            Vector2I::new(0, 0),
        ];

        // Create a polygon with its last segment null
        let mut poly_line = ShapeLineChain::new();
        poly_line.append_point(null_points[0], false);
        poly_line.append_point(null_points[1], false);
        poly_line.append_point(null_points[2], false);
        poly_line.append_point(null_points[2], true);
        poly_line.set_closed(true);

        let mut last_null_segment_poly_set = ShapePolySet::new();
        last_null_segment_poly_set.add_outline(&poly_line);

        // Create a polygon with its first segment null
        poly_line.clear();
        poly_line.append_point(null_points[0], false);
        poly_line.append_point(null_points[0], true);
        poly_line.append_point(null_points[1], false);
        poly_line.append_point(null_points[2], false);
        poly_line.set_closed(true);

        let mut first_null_segment_poly_set = ShapePolySet::new();
        first_null_segment_poly_set.add_outline(&poly_line);

        // Create a polygon with an inside segment null
        poly_line.clear();
        poly_line.append_point(null_points[0], false);
        poly_line.append_point(null_points[1], false);
        poly_line.append_point(null_points[1], true);
        poly_line.append_point(null_points[2], false);
        poly_line.set_closed(true);

        let mut inside_null_segment_poly_set = ShapePolySet::new();
        inside_null_segment_poly_set.add_outline(&poly_line);

        IteratorFixture {
            common: CommonPolyData::new(),
            last_null_segment_poly_set,
            first_null_segment_poly_set,
            inside_null_segment_poly_set,
            null_points,
        }
    }
}

/// Checks whether the iteration on the vertices of a common polygon is correct.
#[test]
fn vertex_iterator() {
    let f = IteratorFixture::new();
    let mut vertex_index = 0;
    let mut iterator = f.common.holey_poly_set.iterate_with_holes();

    while iterator.is_valid() {
        assert_eq!(f.common.data.holey_points[vertex_index], iterator.get());
        vertex_index += 1;
        iterator.advance();
    }

    assert_eq!(vertex_index, 12);

    // The same traversal through `Iterator`
    let all: Vec<Vector2I> = f.common.holey_poly_set.iterate_with_holes().collect();
    assert_eq!(all, f.common.data.holey_points);
}

/// Checks whether the iteration on the segments of a common polygon is correct.
#[test]
fn segment_iterator() {
    let f = IteratorFixture::new();
    let mut segment_index = 0;

    for segment in f.common.holey_poly_set.iterate_segments_with_holes() {
        assert_eq!(f.common.data.holey_segments[segment_index].a, segment.a);
        assert_eq!(f.common.data.holey_segments[segment_index].b, segment.b);
        segment_index += 1;
    }

    assert_eq!(segment_index, 12);
}

/// Checks whether the iteration on the segments of an empty polygon is correct.
#[test]
fn empty_polygon() {
    let f = IteratorFixture::new();

    if f.common
        .empty_poly_set
        .iterate_segments_with_holes()
        .next()
        .is_some()
    {
        panic!("Empty set is being iterated!");
    }
}

/// Checks whether the iteration on the segments of a polygon with one vertex is correct.
#[test]
fn unique_vertex() {
    let f = IteratorFixture::new();
    let mut iterator = f
        .common
        .unique_vertex_poly_set
        .iterate_segments_with_holes();

    let segment = iterator.get();
    assert_eq!(segment.a, f.common.data.unique_points[0]);
    assert_eq!(segment.b, f.common.data.unique_points[0]);

    iterator.advance();

    assert!(!iterator.is_valid());
}

/// Checks whether the counting of the total number of vertices is correct.
#[test]
fn total_vertices() {
    let f = IteratorFixture::new();

    assert_eq!(f.common.empty_poly_set.total_vertices(), 0);
    assert_eq!(f.common.unique_vertex_poly_set.total_vertices(), 1);
    assert_eq!(f.common.solid_poly_set.total_vertices(), 0);
    assert_eq!(f.common.holey_poly_set.total_vertices(), 12);
}

/// Checks whether the removal of null segments, wherever they are placed, is correct.
#[test]
fn remove_null_segments() {
    let f = IteratorFixture::new();
    let polygon_sets = [
        f.last_null_segment_poly_set.clone(),
        f.first_null_segment_poly_set.clone(),
        f.inside_null_segment_poly_set.clone(),
    ];

    for mut polygon_set in polygon_sets {
        assert_eq!(polygon_set.total_vertices(), 4);
        assert_eq!(polygon_set.remove_null_segments(), 1);
        assert_eq!(polygon_set.total_vertices(), 3);

        assert_eq!(polygon_set.c_vertex_global(0), f.null_points[0]);
        assert_eq!(polygon_set.c_vertex_global(1), f.null_points[1]);
        assert_eq!(polygon_set.c_vertex_global(2), f.null_points[2]);
    }
}
