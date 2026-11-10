# kicad-geom port status

Port of KiCad's `libs/kimath` (integer 2D math, shapes, polygon sets, collisions).

State after **work package G2 - basic shapes (kimath part 2)**, on top of G1 (math foundation):
utilities and rounding, vectors, boxes, angles, trigonometry helpers, segments and vector
utilities (G1); the `SHAPE` hierarchy, `SHAPE_ARC`, `CIRCLE`, the simple shapes, compound shapes,
geometry_utils, half lines / lines, arc chord parameters and the arc construction helpers (G2),
plus skeletons of `SHAPE_LINE_CHAIN` (completed in G3, below) and `SHAPE_POLY_SET` (G4) and the part of
the collision matrix that needs nothing else (full port: G5).  Everything is tested with the
ported C++ unit tests and verified bit for bit against the original C++ code with two
differential oracles (about 1.6 million pseudo-random cases, see "Verification").  The design of
the shape hierarchy is in `rust/docs/design/geom-shapes.md`.

**Work package G6a** (polygon-independent kimath helpers) added `bezier_curves`, `convex_hull`,
`transform_trs`, `transform`, `vector3`, `ellipse`, `geometry_predicates`, `point_types`,
`distribute`, `grid_geometry` and the hashes (`hash`, `hash_128`, `mmh3_hash`, `md5_hash`),
verified by the `helpers_oracle` (540,000 cases, bit for bit).

**Work package G3** (`SHAPE_LINE_CHAIN`) completed the line chain: every method of
`shape_line_chain.h/.cpp` (arc bookkeeping, editing, slicing, splitting, simplification,
intersections, distances, Clipper2 conversions, `Format` / `Parse`, `POINT_INSIDE_TRACKER`)
except the polygon-set part of `OffsetLine` (G4), verified by the `slc_oracle` (about 230,000
line chain cases and 6,000 conversion cases, bit for bit).  It also introduced the x86-64
conversions of out-of-range doubles (`cmath::f64_to_i32_x86` / `f64_to_i64_x86`, PORTING.md
section 9) at the G1/G2 sites (D4, D12).

**Work package G4a** (`SHAPE_POLY_SET` without the triangulation) vendored the Clipper2 port
(`rust/vendor/clipper2-rs`, see below) and ported `shape_poly_set.h/.cpp`: the Clipper2 bridge
(booleans, offsets, `PolyTree` import), fracturing (with the Y-stripe edge index), unfracturing,
simplification, chamfer / fillet, the vertex index machinery and iterators, distances, the
non-triangulated collisions, hashing, `Format` / `Parse`; verified by the `sps_oracle` (328,146
cases, bit for bit).  The triangulation is package G4b (GAP-P1).

**Work package G4b** (triangulation) ported `polygon_triangulation.h` (`POLYGON_TRIANGULATION`),
`vertex_set.h/.cpp`, `shape_poly_tri_index.h`, `poly_ystripes_index.h`, `TRIANGULATED_POLYGON::Refine`
and the triangulation parts of `SHAPE_POLY_SET` (`cacheTriangulation` with its partition /
fracture / self-touching fallbacks and hash check, the triangle index, the triangle-based
`Collide( SHAPE* )`), closing GAP-P1, GAP-5 and GAP-P4; verified by the `tri_oracle` (140,000
cases, bit for bit, triangle lists and their order included).

**Work package G5** (collision matrix) completed `shape_collisions.cpp` (every `Collide( A, B )`
pair incl. `SHAPE_LINE_CHAIN_BASE` against `SHAPE_LINE_CHAIN_BASE` with its `SEGMENT_INDEX`
gates, rounded rectangles, the ellipse pairs, the poly-set dispatch through
`SHAPE_POLY_SET::Collide( SHAPE* )`, compounds), ported `shape_nearest_points.cpp`
(`SHAPE::NearestPoints`, which also completes `SHAPE_ARC::NearestPoints( SHAPE_RECT )`),
`shape_index.h` and `shape_index_list.h`, added `Shape::indexable_subshape( i )`, and closed
GAP-4, GAP-P2 and GAP-P3; verified by the `coll_oracle` (245,000 cases, every ordered pair of 13
shape kinds, bit for bit).

**Work package G6b** (shape conversion utilities) ported `convert_basic_shapes_to_polygon`
(circles, rings, ovals, trapezoids, round / chamfered rectangles, arcs, `CornerListToPolygon`),
`ROUNDRECT`, the corner operations (`ComputeChamferPoints`, `ComputeDogbone`), the rest of
`shape_utils`, `oval`, `INTERSECTION_VISITOR` (`intersection.h`), `nearest` and `SHAPE_ELLIPSE`
(with `ShapeRef::Ellipse`), and wired the `TransformToPolygon` of circles, segments, arcs and
rounded rectangles (closing GAP-5 and GAP-H1); verified by the `conv_oracle` (250,000 cases,
bit for bit).  It also replaced `cmath::cbrt` by a port of glibc's `cbrt` (D-G1).

Everything in this crate builds for `wasm32-unknown-unknown` and has no `unsafe`.  Its only
dependency is the vendored, fidelity-patched `clipper2-rs` 0.3.0 (path dependency, feature
`usingz`; pure Rust, BSL-1.0; PORTING.md section 9).

## C++ to Rust map

| C++ file / entity | Rust | State |
|---|---|---|
| `libs/kimath/include/math/util.h`, `src/math/util.cpp` | `src/math/util.rs` | ported |
| - `KiROUND<fp_type, ret_type>` | `ki_round` (ret `int`), `ki_round_to::<R, _>`, `ki_round_i64`; traits `KiRound`, `KiRoundArg`, `KiRoundRet` | ported (args `f64`, `f32`, `i32`, `i64`, `u32`, `u64`; ret `i32`, `i64`, `u32`) |
| - `KiCheckedCast<in, ret>` | `ki_checked_cast`, trait `KiCheckedCast` | ported, see deviation D1 |
| - `rescale<T>` (generic, `int`, `int64_t`) | `rescale`, trait `Rescale` (`i32`, `i64`, `u32`, `f32`, `f64`) | ported (`int64_t`: the `__int128` code path) |
| - `sign`, `ct_sqrt`, `ct_sqrt_helper`, `isqrt`, `UnsignedAbs`, `RoundedHypot`, `RoundedHypotWide`, `equals` | `sign`, `ct_sqrt_{u64,i64,u32,i32}`, `isqrt`, `unsigned_abs`, `rounded_hypot`, `rounded_hypot_wide`, `equals` / `equals_eps` | ported |
| - `kimathLogDebug`, `kimathLogOverflow` | `kimath_log_debug`, `kimath_log_overflow` | ported as no-ops (release behaviour), see D2 |
| - (uses of `std::clamp/min/max` on doubles) | `std_clamp`, `std_min`, `std_max` | support code with the exact C++ NaN semantics |
| `libs/kimath/include/math/wide_int.h` | `src/math/wide_int.rs` (`KiInt128 = i128`, `cross_wide`, `dot_wide`, `to_double`, `words_to_double`) | ported |
| `libs/kimath/include/math/vector2d.h` | `src/math/vector2.rs` | ported |
| - `VECTOR2<T>`, `VECTOR2I/D/L`, `VECTOR2_TRAITS` | `Vector2<T>`, `Vector2I/D/L`, traits `Coord` (`i32`, `i64`, `u32`, `f32`, `f64`) and `SignedCoord` | ported; all methods and operators |
| - converting constructor / `operator()<U>` | `Vector2::cast::<U>()`, `Vector2::<T>::from_vec( v )` | ported, see D4 |
| - `LexicographicalMax/Min/Compare`, `equals( VECTOR2 )`, `KiROUND( VECTOR2D )`, `KiROUND( x, y )` | `lexicographical_max/min/compare`, `equals_vec` / `equals_vec_eps`, `ki_round( v )`, `ki_round_xy` | ported |
| - `Format()`, `operator<<` | `format()` and `Display`; `to_stream_string()` | ported (doubles formatted like `std::ostream`, i.e. `%g`) |
| `libs/kimath/include/math/box2.h` | `src/math/box2.rs` | ported |
| - `BOX2<Vec>`, `BOX2I/D/L`, `OPT_BOX2I` | `Box2<T>`, `Box2I/D/L`, `OptBox2I`; methods generated per instantiation | ported; all methods |
| - `BOX2ISafe` (3 overloads), `IsBOX2Safe`, `KiROUND( BOX2D )` | `box2i_safe`, `box2i_safe_from_pos_size`, `box2i_safe_from_int`, method `is_box2_safe`, `ki_round( box )` | ported |
| `libs/kimath/include/math/box2_minmax.h` | `src/math/box2_minmax.rs` (`Box2IMinMax`); the `SHAPE_ARC` constructor is `Box2IMinMax::from_arc` in `src/geometry/shape_arc.rs` | ported |
| `libs/kimath/include/geometry/eda_angle.h`, `src/geometry/eda_angle.cpp` | `src/geometry/eda_angle.rs` | ported |
| - `EDA_ANGLE`, `EDA_ANGLE_T`, `ANGLE_*`, `FULL_CIRCLE` | `EdaAngle`, `EdaAngleT` (+ consts `DEGREES_T`, `RADIANS_T`, `TENTHS_OF_A_DEGREE_T`), `ANGLE_*`, `FULL_CIRCLE` | ported; all methods and operators, `std::abs` is `EdaAngle::abs` |
| - `EDA_ORIENTATION` | `EdaOrientation` | ported |
| `libs/kimath/include/trigo.h`, `src/trigo.cpp` | `src/trigo.rs` | ported (everything in the files; see the overload table in the module docs) |
| `libs/kimath/include/geometry/seg.h`, `src/geometry/seg.cpp` | `src/geometry/seg.rs` (`Seg`, `OptVector2I`, `Ecoord`) | ported; all methods (this version of `SEG` has no methods taking shapes) |
| `libs/kimath/include/geometry/vector_utils.h`, `src/geometry/vector_utils.cpp` | `src/geometry/vector_utils.rs` | ported |
| `include/core/mirror.h` (`FLIP_DIRECTION`, `MIRRORVAL`, `MIRROR`, `MIRROR_AXIS`) | `src/kicore/mirror.rs` (`FlipDirection`, `mirror_val`, `mirror`, `mirror_point`, `mirror_axis`) | ported |
| `libs/core/include/core/kicad_algo.h` | - | skipped: nothing in it is used by the files of this package (`alg::pair_contains` / `alg::run_on_pair` are used by shape code) |
| `libs/kimath/include/geometry/rtree/rtree_node.h` | `src/geometry/rtree/rtree_node.rs` | ported: `HilbertXY2D` (`hilbert_xy2d`), `HilbertND2D` (`hilbert_nd2d`), `OverlapMask2D` (`overlap_mask`, all `D`), `RTREE_NODE` (`RtreeNode`, all methods), `SLAB_ALLOCATOR` (`NodeArena`: `allocate`, `free`, `memory_usage`); see D12 |
| `libs/kimath/include/geometry/rtree/packed_rtree.h` | `src/geometry/rtree/packed_rtree.rs` (`PackedRTree`, `PackedRTreeBuilder`) | ported (`Builder::Reserve/Add/Build`, `Search`, `begin/end`, `size`, `empty`, `MemoryUsage`; move = `std::mem::take`) |
| `libs/kimath/include/geometry/rtree/dynamic_rtree.h` | `src/geometry/rtree/dynamic_rtree.rs` (`DynamicRTree`, `BulkEntry`, `Iter`) | ported: `Insert`, `Remove` (with the insertion-bbox fallback), `Search`, `RemoveAll`, `BulkLoad`, `size`, `empty`, `MemoryUsage`, `Iterator` (`iter`), `Overlapping` / `SearchRange` / `SearchIterator` (`overlapping`), `NearestNeighbors` (`nearest_neighbors`), and every private R*-tree helper; see D11, D12 |
| `libs/kimath/include/geometry/rtree/dynamic_rtree_cow.h` (`COW_RTREE`) | - | skipped: only used by the interactive router through `SHAPE_INDEX`, which the port builds on `DynamicRTree` (D-C2), see GAP-4b |
| `thirdparty/delaunator/delaunator.cpp`, `.hpp` | `src/thirdparty/delaunator.rs` (`Delaunator`, `Point`, `INVALID_INDEX`, `DelaunatorError`) | ported statement by statement (incl. `get_hull_area`, `get_triangle_area`; the unused `clockwise` helpers are omitted); MIT notice kept; see D13 |
| `thirdparty/nanoflann/nanoflann.hpp` | `src/thirdparty/nanoflann.rs` (`KdTreeSingleIndexAdaptor`, `DatasetAdaptor`, `KnnResultSet`, `RadiusResultSet`, `ResultItem`, `SearchParameters`, `KdTreeSingleIndexAdaptorParams`) | partial: exactly the subset KiCad uses (`convert_shape_list_to_polygon.cpp`, `fix_board_shape.cpp`): static kd-tree, `L2_Simple_Adaptor`, `knnSearch`, `radiusSearch`, `findNeighbors`; BSD notice kept; see GAP-6 |
| `libs/kimath/include/geometry/polygon_triangulation.h` (`POLYGON_TRIANGULATION`) | `src/geometry/polygon_triangulation.rs` (`PolygonTriangulation`; consts `TRIANGULATE_SIMPLIFICATION_LEVEL`, `TRIANGULATE_MINIMUM_AREA`, `TRIANGULATE_DELAUNAY_REFINE`) | ported (G4b): both `TesselatePolygon`s (`tesselate_chain`, `tesselate_polygon`), `PartitionAreaFractionsForTesting`, and every private helper (scanline splitting, balanced partition, `simplifyList`, `decimateList`, `removeNullTriangles`, `earcutList` / `earcutRing` with the ear lookahead and reflex index, `createRing`, `eliminateHoles`, `filterPoints`, `findHoleBridge`, `subdividePolygon`, `splitPolygon`, `clipRemainingEars`, `goodSplit`, `intersects`, `intersectsPolygon`); `logRemaining` / `logVertices` (trace output only) dropped; see D-T1..D-T3 |
| `geometry/vertex_set.h`, `src/geometry/vertex_set.cpp` (`VERTEX`, `VERTEX_SET`) | `src/geometry/vertex_set.rs` (`Vertex`, `VertexSet`, `VertexId`, `NIL`, `ReflexEntry`, `area_xy`) | ported (G4b); the `VERTEX` methods are `VertexSet` methods taking the vertex id (`remove`, `split`, `update_order`, `update_list`, `z_sort`, `ring_area` = `VERTEX::area`, `is_ear`), the protected `VERTEX_SET` helpers are `pub` for the zone filler / connection-width users; D-T1 |
| `geometry/shape_poly_tri_index.h` (`TRIANGULATION_INDEX`, `VisitTriangleCandidates`, both `AnyTriangleHit`s) | `src/geometry/shape_poly_tri_index.rs` (`TriangulationIndex`, `visit_triangle_candidates`, `any_triangle_hit`, `any_triangle_hit_point`) | ported (G4b); the index stores ( polygon, triangle ) indices and queries take the triangulation (C++: `const TRI*`) |
| `geometry/poly_ystripes_index.h` (`POLY_YSTRIPES_INDEX`) | `src/geometry/poly_ystripes_index.rs` (`PolyYStripesIndex`: `build`, `contains`) | ported (G4b) |
| libstdc++ `std::sort`, `std::partial_sort`, `std::nth_element`, `std::make_heap` / `push_heap` / `pop_heap` / `sort_heap`, `std::priority_queue`, `erase( std::unique( ... ) )` | `src/stdsort.rs` (`std_sort`, `std_sort_default`, `std_partial_sort`, `std_nth_element`, `std_make_heap`, `std_push_heap`, `std_pop_heap`, `std_sort_heap`, `PriorityQueue`, `std_unique`) | ported from GCC 13 `bits/stl_algo.h` / `stl_heap.h` (PORTING.md section 9); see D14 |
| `libs/kimath/include/math/vector2wx.h` | - | skipped: wxWidgets glue |
| `libs/kimath/include/math/vector3.h` | `src/math/vector3.rs` (`Vector3<T>`, `Vector3I/D/U`, trait `Vector3Coord`) | ported (G6a): all methods, `operator<<` (`Display`) |
| `libs/kimath/include/math/matrix3x3.h` | - | skipped (G6a): neither `TRANSFORM_TRS` nor the ellipse code uses it; its users are GAL / painter / DXF import (out of scope); `test_matrix3x3.cpp` not ported |
| `geometry/approximation.h` (`ERROR_LOC`) | `src/geometry/approximation.rs` (`ErrorLoc`, `ERROR_INSIDE`, `ERROR_OUTSIDE`) | ported |
| `geometry/shape.h`, `src/geometry/shape.cpp` | `src/geometry/shape.rs` | ported (design: `docs/design/geom-shapes.md`) |
| - `SHAPE_TYPE`, `SHAPE_TYPE_asString` | `ShapeType`, `shape_type_as_string` | ported |
| - `SHAPE_BASE`, `SHAPE` (virtual interface) | trait `Shape` (+ enum `ShapeRef` for dispatch) | ported; `GetClearance` (also free fn `get_clearance`), `Distance`, `SquaredDistance`, `PointInside`, `Format`, `Centre` defaults; `Parse` is not part of the trait (the base `SHAPE::Parse` is `assert( false )`; `SHAPE_LINE_CHAIN` / `SHAPE_POLY_SET` implement it: `ShapeLineChain::parse`, `ShapePolySet::parse`); `NearestPoints( SHAPE* )` is `Shape::nearest_points` (G5); `indexable_subshape( i )` (no C++ counterpart, G5) returns the `i`-th of `indexable_subshapes()` without allocating (compounds, triangulated poly sets) |
| - `SHAPE_LINE_CHAIN_BASE` + its methods in `shape_line_chain.cpp` | trait `ShapeLineChainBase` (`lcb_collide_point`, `lcb_collide_seg`, `lcb_squared_distance`, `lcb_point_inside`, `point_on_edge`, `edge_containing_point`) | ported |
| `geometry/shape_arc.h`, `src/geometry/shape_arc.cpp` | `src/geometry/shape_arc.rs` (`ShapeArc`) | ported (`NearestPoints( SHAPE_RECT )` completed in G5) |
| `geometry/shape_circle.h` (+ `Format`, `TransformToPolygon` from `shape_segment.cpp`) | `src/geometry/shape_circle.rs` | ported (`TransformToPolygon` since G6b) |
| `geometry/shape_rect.h`, `src/geometry/shape_rect.cpp` | `src/geometry/shape_rect.rs` | ported (rounded corners through `Roundrect` since G6b) |
| `geometry/shape_segment.h`, `src/geometry/shape_segment.cpp` | `src/geometry/shape_segment.rs` | ported (`TransformToPolygon` since G6b) |
| `geometry/shape_simple.h` (+ `TransformToPolygon` from `shape.cpp`) | `src/geometry/shape_simple.rs` | ported |
| `geometry/shape_null.h` | `src/geometry/shape_null.rs` | ported |
| `geometry/shape_compound.h`, `src/geometry/shape_compound.cpp` | `src/geometry/shape_compound.rs` | ported; `Distance( const SEG& )` (`assert( false )` in C++) skipped |
| `geometry/shape_line_chain.h/.cpp` (`SHAPE_LINE_CHAIN`) | `src/geometry/shape_line_chain.rs` (`ShapeLineChain`) | ported (G3): every member of the header and the file, overloads as in the module docs' table; `OffsetLine` completed in G4a (`offset_line`); see D-S1..D-S8 |
| - `CLIPPER_Z_VALUE`, `INTERSECTION`, `POINT_INSIDE_TRACKER`, `SHAPE_LINE_CHAIN::compareOriginDistance` | `ClipperZValue`, `Intersection`, `PointInsideTracker`, `CompareOriginDistance` | ported.  `Intersect( SEG )` sorts with the nested functor (rounded `EuclideanNorm()`), which C++ name lookup picks over the file-scope `compareOriginDistance` (squared norms); that file-scope struct is therefore dead code and not ported |
| - `Clipper2Lib::Path64` / `Point64` (constructor, `convertToClipper2`) | `ShapeLineChain::from_clipper2( &[P: ClipperPoint64], .. )`, `convert_to_clipper2() -> Vec<(i64, i64, i64)>` | ported; trait `ClipperPoint64` (implemented for `(x, y, z)` triples) so that the module needs no clipper dependency |
| - protected helpers (`convertArc`, `splitArc`, `amendArc*`, `fixIndicesRotation`, `mergeFirstLastPointIfNeeded`, `reversedArcIndex`, `convertToClipper2`) | same names, `pub` (the C++ `friend SHAPE_POLY_SET` becomes visibility) | ported |
| - `std::stringstream` of `Parse()` | `StringStream` | the `>>` extractions `Parse` uses (D-S6) |
| `geometry/shape_poly_set.h/.cpp` (`SHAPE_POLY_SET`) | `src/geometry/shape_poly_set.rs` (`ShapePolySet`, `Polygon`, `VertexIndex`, `PolySetIterator`, `SegmentIterator`, `Tri`, `TriangulatedPolygon`) and `src/geometry/shape_poly_set_fracture.rs` (file-local fracture helpers) | ported (G4a, triangulation G4b): every member of the header (overload table in the module docs) and every function of the `.cpp`; `CacheTriangulation( aSimplify, aSubmitter, aBuildIndexes )` is `cache_triangulation` / `cache_triangulation_with( simplify, has_submitter, build_indexes )`, the protected `cacheTriangulation( ..., aHintData, ... )` is `cache_triangulation_hint`, `TriangulationIndex()` is `triangulation_index()`, `Collide( SHAPE* )` is `collide_shape` (`&self`) / `collide_shape_mut` (caching); see D-P1..D-P9, D-T2, D-T3 |
| - `ITERATOR_TEMPLATE` (`ITERATOR`, `CONST_ITERATOR`), `SEGMENT_ITERATOR_TEMPLATE` | `PolySetIterator`, `SegmentIterator`: cursors with the C++ methods (`is_valid` = `operator bool`, `advance` = `operator++`, `get` = `operator*`, `get_index`, `is_end_contour`, `is_last_polygon`, `is_adjacent`) that also implement `Iterator` with the same traversal and end condition (D-P3) | ported |
| - `TRIANGULATED_POLYGON` / `TRI` | `TriangulatedPolygon` (`new`, `clear`, `get_triangle`, `add_triangle`, `add_vertex`, `triangle_count`, `source_outline_index`, `set_source_outline_index`, `vertex_count`, `move_`; `Triangles()` / `Vertices()` / `SetTriangles()` / `SetVertices()` are the public fields), `Tri` (incl. `Area()`) | ported (G4b: `refine`, `set_triangles`, `set_vertices`; the triangles' vertex copies are kept in sync, D15) |
| - file-local helpers of `shape_poly_set.cpp`: `splitAtBridges`, `FractureEdge`, `FRACTURE_EDGE_INDEX`, `fractureIntersectX`, `processHole`, `fractureSingleCacheFriendly` | `shape_poly_set_fracture.rs` | ported; `FractureEdgeSlow` / `processEdge` / `fractureSingleSlow` (only selected by `ADVANCED_CFG::m_EnableCacheFriendlyFracture = false`) skipped, D-P7 |
| - `SHAPE_LINE_CHAIN::OffsetLine()` | `ShapeLineChain::offset_line` (in `shape_poly_set.rs`) | ported (closes GAP-S1) |
| `geometry/corner_operations.cpp`: `SHAPE_POLY_SET::chamferFilletPolygon` | `shape_poly_set.rs` | ported |
| `geometry/corner_operations.h`, the free functions of `corner_operations.cpp` (`CHAMFER_PARAMS`, `CHAMFER_RESULT`, `DOGBONE_RESULT`, `ComputeChamferPoints`, `ComputeDogbone`, file-local `GetBisectorOfCornerSegments`) | `src/geometry/corner_operations.rs` (`ChamferParams`, `ChamferResult`, `DogboneResult`, `compute_chamfer_points`, `compute_dogbone`) | ported (G6b); `std::optional` results are `Option` |
| `geometry/corner_strategy.h` (`CORNER_STRATEGY`) | `src/geometry/corner_strategy.rs` (`CornerStrategy`) | ported |
| `geometry/segment_index.h` (`SEGMENT_INDEX`) | `src/geometry/segment_index.rs` (`SegmentIndex`) | ported (over the ported `PACKED_RTREE`, same visit order) |
| `geometry/fracture_edge_index_utils.h` (`KIGEOM::FRACTURE_INDEX`) | `src/geometry/fracture_edge_index_utils.rs` | ported (`size_t` = `u64`) |
| `thirdparty/clipper2` (Clipper2, built with `USINGZ`) | `rust/vendor/clipper2-rs` (crate `clipper2_rs` 0.3.0 + `clipper2-rs-0.3.0-kicad-fidelity.patch`, see its `README-KICAD.md`) | vendored, re-validated (see "Clipper2 re-validation") |
| `src/geometry/shape_collisions.cpp` | `src/geometry/shape_collisions.rs` | ported (G5): every static `Collide( A, B )` (`collide_<a>_<b>`; `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )` is the public `collide_lcb_lcb`), `collideEllipseVsSegments`, `pushoutForce`, `CollCase` / `CollCaseReversed` (spelled out per pair in `collide_single_shapes`, with the C++ overload resolution), `collideSingleShapes`, `collideShapes` (`collide_shapes`), `SHAPE::Collide` (trait `collide` / `collide_mtv`); `unsupported_g5` remains as the "Unsupported collision" fallback (nested compounds); D-C1 |
| `src/geometry/shape_nearest_points.cpp` | `src/geometry/shape_nearest_points.rs` | ported (G5): every static `NearestPoints( A, B )` (`np_<a>_<b>`), `NearestPointsCase(Reversed)`, `nearestPointsSingleShapes` (`nearest_points_single_shapes`), `nearestPoints` / `SHAPE::NearestPoints` (`nearest_points`, also `shape_collisions::nearest_points` and `Shape::nearest_points`) |
| `geometry/shape_index.h` (`SHAPE_INDEX`, `shapeFunctor`, `boundingBox`, `acceptVisitor`, `collide`, `queryCallback`) | `src/geometry/shape_index.rs` (`ShapeIndex<T>`, trait `ShapeIndexItem` = `T::Shape( aLayer )`, `SharedShape` = `SHAPE*` item compared by identity) | ported (G5) over `DynamicRTree` instead of `COW_RTREE` (D-C2): `Add` (both), `Remove`, `RemoveAll`, `Accept`, `BulkLoad`, `Reindex`, `Query`, `Clone` (`clone_index`), iteration, `Size` |
| `geometry/shape_index_list.h` (`SHAPE_INDEX_LIST`) | `src/geometry/shape_index_list.rs` (`ShapeIndexList<T>`, `ShapeEntry`, `QueryIterator`) | ported (G5): `Add`, `Remove`, `Size`, `Query` (exact / bbox only), `Clear`, `qbegin` / `qend` (an `Iterator`), `begin` / `end` |
| `geometry/circle.h`, `src/geometry/circle.cpp` | `src/geometry/circle.rs` (`Circle`) | ported; the non-const `Contains()` overload is `contains_inside()` |
| `geometry/arc_chord_params.h/.cpp` | `src/geometry/arc_chord_params.rs` (`ArcChordParams`) | ported |
| `geometry/arc_construction.h/.cpp` | `src/geometry/arc_construction.rs` | ported |
| `geometry/half_line.h/.cpp` | `src/geometry/half_line.rs` (`HalfLine`) | ported |
| `geometry/line.h/.cpp` | `src/geometry/line.rs` (`Line`) | ported |
| `geometry/geometry_utils.h/.cpp` | `src/geometry/geometry_utils.rs` | ported (`GetVectorSnapped45/90`, `GetClampedCoords`, `IsVec2SafeXY` for `int`, `int64_t`, `double`; `GetClampedCoords<double, int>` / `<int64_t, int>` as `get_clamped_coords_to_i` / `_l_to_i`) |
| `include/base_units.h`: `ARC_HIGH_DEF`, `ARC_LOW_DEF` (+ `_MM`) | `src/geometry/geometry_utils.rs` | ported (constants only) |
| `geometry/shape_utils.h`, `src/geometry/shape_utils.cpp` (`KIGEOM`) | `src/geometry/shape_utils.rs` | ported (G6b completed it): `ParameterAlong`, `NormalisedSeg`, `GetOtherEnd`, `GetSharedEndpoint`, `BoxToSegs`, `CollectBoxCorners`, `BoxToLineChain`, `GetSegsInDirection`, `ClipHalfLineToBox`, `ClipLineToBox`, `MakeArcCw90`, `MakeArcCw180`, `GetPoint`, `GetCircleKeyPoints`, `RectifyPolygon`, `AddHoleIfValid`, both `MakeRegularPolygonPoints` (`make_regular_polygon_points`, `..._radius`), `MakeCrossSegments`; invalid directions return the C++ fallback values (`wxFAIL_MSG` in release) |
| `geometry/direction45.h` | `src/geometry/direction45.rs` (`Directions`) | partial (G6b): only the `DIRECTION_45::Directions` enum the shape utilities take; the `DIRECTION_45` class is router-only, skipped |
| `include/bezier_curves.h`, `src/bezier_curves.cpp` | `src/bezier_curves.rs` | ported (G6a): `BEZIER_POLY` (`BezierPoly`: all three constructors, both `GetPoly` overloads as `get_poly_i` / `get_poly_d`, and every private helper with the exact tolerances), `BEZIER<T>` (`Bezier<T>`: `PointAt`, `Split`, `SubCurve`), `TransformEllipseToBeziers<T>`; `int` / `double` instantiations via trait `NumericType`; `wxLogTrace` dropped |
| `geometry/ellipse.h`, `src/geometry/ellipse.cpp` | `src/geometry/ellipse.rs` (`Ellipse<T>`) | ported (G6a): both constructors (`new`, `from_major_axis`), `Mirror`, `GetSubtendedAngle`, `GetPointAtAngle`, `GetArcStart/EndPoint`, `GetAngleAtPoint` |
| `geometry/convex_hull.h`, `src/geometry/convex_hull.cpp` | `src/geometry/convex_hull.rs` | ported (G6a): `BuildConvexHull` (points: `build_convex_hull`, plus `convex_hull`; poly set: `build_convex_hull_poly_set`, `..._moved`, over the skeleton's `outline_count` / `c_outline` / `point_count` / `c_point`) |
| `geometry/transform_trs.h`, `src/geometry/transform_trs.cpp` | `src/geometry/transform_trs.rs` (`TransformTrs`) | ported (G6a): all methods (the C++ class has no compose / matrix API) |
| `include/transform.h`, `src/transform.cpp` | `src/transform.rs` (`Transform`, `DEFAULT_TRANSFORM`) | ported (G6a): all methods, `std::hash<TRANSFORM>` (`StdHash`) |
| `include/hash.h` | `src/hash.rs` (`hash_combine`, macros `hash_combine!` / `hash_val!`, trait `StdHash` = libstdc++ `std::hash`, `std_hash_bytes` = `std::_Hash_bytes`) | ported (G6a) |
| `include/hash_128.h` | `src/hash_128.rs` (`Hash128`) | ported (G6a) |
| `include/mmh3_hash.h` | `src/mmh3_hash.rs` (`Mmh3Hash`, incl. `addDataV1`; `murmur_hash3_x64_128` = the reference copy in `test_kimath.cpp`) | ported (G6a), see D-H2 |
| `include/md5_hash.h`, `src/md5_hash.cpp` | `src/md5_hash.rs` (`Md5Hash`) | ported (G6a), incl. the `Format` digit quirk (D-H3) |
| `geometry/geometry_predicates.h` | `src/geometry/geometry_predicates.rs` | ported (G6a): `OrientationSign`, `detail::inCircleLegalDouble`, `InCircleDelaunayLegal`, `IsSliverTriangle` |
| `geometry/point_types.h` | `src/geometry/point_types.rs` (`PointType` flags `PT_*`, `TypedPoint2I`) | ported (G6a) |
| `geometry/distribute.h`, `src/geometry/distribute.cpp` | `src/geometry/distribute.rs` | ported (G6a) |
| `geometry/grid_geometry.h`, `src/geometry/grid_geometry.cpp` | `src/geometry/grid_geometry.rs` (`GridGeometry`, `GridKind`) | ported (G6a) |
| `geometry/nearest.h`, `src/geometry/nearest.cpp` | `src/geometry/nearest.rs` (`NearableGeom`, `get_nearest_point`, `get_nearest_point_any`) | ported (G6b) |
| `geometry/shape_ellipse.h/.cpp` (`SHAPE_ELLIPSE`, incl. the file-private `adaptiveSimpson`, `subdivideEllipseArc`, `quadraticRoots` / `cubicRoots` / `quarticRoots`, `dedupePoints`, the private `CONIC`, `conicOf`, `conicRoots`, `closestLocalPoint`, ...) | `src/geometry/shape_ellipse.rs` (`ShapeEllipse`; constructors `new`, `new_arc`, `from_major_endpoint`, `from_major_endpoint_arc`; `Intersect` overloads `intersect_ellipse` / `intersect_circle` / `intersect_arc` / `intersect_seg( seg, treat_as_line )`; `operator==` is `equals`; the root finders are `pub`) | ported (G6b); `ShapeRef::Ellipse` added in `shape.rs` |
| `geometry/intersection.h`, `src/geometry/intersection.cpp` (`INTERSECTABLE_GEOM`, `INTERSECTION_CONTACT`, `INTERSECTION_VISITOR`, every `findIntersections` pair, the contact helpers) | `src/geometry/intersection.rs` (`IntersectableGeom`, `IntersectionContact`, `IntersectionVisitor::new` / `with_contact` / `visit`) | ported (G6b): the C++ argument order of every pair is kept (it decides the point order and the overlap reference segment); the `LINE_LIKE` / `CIRCULAR` concepts are private traits |
| `include/convert_basic_shapes_to_polygon.h`, `src/convert_basic_shapes_to_polygon.cpp` | `src/convert_basic_shapes_to_polygon.rs` | ported (G6b): `TransformCircleToPolygon` (`transform_circle_to_polygon_chain` / `transform_circle_to_polygon`), `TransformOvalToPolygon`, `TransformTrapezoidToPolygon`, `TransformRoundChamferedRectToPolygon`, both `ConvertArcToPolyline` (`convert_arc_to_polyline` centre / radius, `convert_arc_to_polyline_3pt`), `TransformArcToPolygon`, `TransformRingToPolygon`, `ROUNDED_CORNER` (`RoundedCorner`), `CornerListToPolygon`, `CornerListRemoveDuplicates`, `RECT_CHAMFER_*`; defaulted arguments are explicit |
| `geometry/roundrect.h`, `src/geometry/roundrect.cpp` (`ROUNDRECT`) | `src/geometry/roundrect.rs` (`Roundrect`) | ported (G6b) |
| `geometry/oval.h`, `src/geometry/oval.cpp` | `src/geometry/oval.rs` (`convert_to_chain`, `get_oval_key_points`, `OVAL_*`) | ported (G6b) |
| `geometry/shape_file_io.h/.cpp` | - | skipped: writes debug files (`FILE*`), no filesystem in the library (PORTING.md section 5), no user in scope |
| `geometry/poisson_disk.h/.cpp` | - | skipped: only used by the via-stitching generator / dialog (out of scope) |

Support modules without a C++ counterpart:

| Rust | Purpose |
|---|---|
| `src/cmath.rs` | The transcendental libm functions the C++ calls (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `hypot`, `hypotf`, `pow`, ...). Every such call in the port goes through this module so the libm choice is made in one place (see GAP-1). |
| `src/ostream.rs` | `format_g` / `format_double`: the default `std::ostream << double` formatting (`%g`), needed by `Format()` and the stream operators. |
| `src/test_utils.rs` | Ports of the shared C++ QA helpers (below) and Mersenne Twister / libstdc++ distribution replicas for porting randomised tests. |
| `src/stdsort.rs` | libstdc++-exact unstable sorts, heaps and priority queue (see the map above). Use these wherever the C++ sorts with a comparator that leaves ties and the order can matter; `std::stable_sort` is `slice::sort_by`. |

### Naming notes (beyond `PORTING.md`)

- Overloads on point vs. segment in `Seg`: the point version has the plain name, the segment
  version a `_seg` suffix (`nearest_point` / `nearest_point_seg`, `distance` / `distance_seg`,
  `squared_distance` / `squared_distance_seg`, `contains` / `contains_seg`).
- C++ default arguments: the plain name applies the defaults, a `_with...` variant takes them
  explicitly (`intersect` / `intersect_with( seg, ignore_endpoints, lines )`, `contains` /
  `contains_with_threshold`, `approx_collinear` / `approx_collinear_with_threshold`,
  `approx_parallel` / `..._with_threshold`, `approx_overlaps` / `..._with_threshold`,
  `line_distance` / `line_distance_sided( p, determine_side )`, `collide` / `collide_actual`).
- `Box2` overloads: see the table at the top of `src/math/box2.rs` (`contains` /
  `contains_xy` / `contains_box`, `intersects` / `intersects_segment` / `intersects_rotated`,
  `merge` / `merge_point`, `inflate` / `inflate_xy`, `inflated` / `inflated_xy`,
  `squared_distance_point` / `squared_distance_box`, `distance_point` / `distance_box`, ...).
  `Move()` is `move_()` (keyword).
- `trigo`: `rotate_point( &mut p, angle )` and `rotate_point_around( &mut p, centre, angle )` are
  generic over `Vector2I` / `Vector2D` (trait `Rotatable`); the raw coordinate overloads are
  `rotate_point_xy[_around][_d]`. `CalcArcCenter`: `calc_arc_center` (ints), `calc_arc_center_d`
  (doubles), `calc_arc_center_from_angle`.
- `Normalize90()` etc. are `normalize90()`, `normalize180()`, `normalize720()`,
  `IsCardinal90()` is `is_cardinal90()`.
- The module for `core/` headers is called `kicore` so it cannot shadow Rust's `core` crate.
- Shapes: `Collide( VECTOR2I )` / `Collide( SEG )` / `Collide( SHAPE* )` / `Collide( SHAPE*, int,
  VECTOR2I* aMTV )` are `collide_point` / `collide_seg` / `collide` / `collide_mtv`; constructors
  are named after their arguments (`ShapeArc::from_start_mid_end`, `from_center_start_angle`,
  `from_tangent_segments`, `with_width`; `ShapeRect::from_box`, `from_xywh`, `from_pos_size`,
  `from_corners`; ...).  `SHAPE_ARC::Intersect( CIRCLE )` / `( SHAPE_ARC )` are
  `intersect_circle` / `intersect_arc`, `NearestPoints( X )` is `nearest_points_<x>`,
  `Mirror( SEG )` is `mirror_seg`, `ConvertToPolyline( err, &actual )` is
  `convert_to_polyline_with_error`.  `CIRCLE::Intersect( SEG )` is `intersect_seg`, the
  `VECTOR2D` overloads of `NearestPoint` / `FurthestPoint` have a `_d` suffix.
- Getters keep their `get_` prefix where `PORTING.md` would produce a name that clashes with a
  `Shape` method or reads badly for shapes (`get_p0`, `get_center`, `get_radius`, `get_width`,
  `get_seg`, ...), so that the C++ names grep one to one.

### Out-parameters

| C++ | Rust |
|---|---|
| `SEG::NearestPoints( seg, aPtA, aPtB, aDistSq )` (always returns true) | `nearest_points( &seg ) -> ( pt_a, pt_b, dist_sq )` |
| `SEG::IntersectsLine( slope, offset, aIntersection )` | `intersects_line( slope, offset ) -> Option<Vector2I>` |
| `SEG::Collide( seg, clearance, int* aActual )` | `collide_actual( &seg, clearance ) -> ( bool, i32 )` (every C++ path sets `*aActual`) |
| `SEG::CanonicalCoefs( qA, qB, qC )` | `canonical_coefs() -> ( qa, qb, qc )` |
| `SegmentIntersectsSegment( ..., VECTOR2I* aIntersectionPoint )` | `segment_intersects_segment_with_point( ... ) -> ( bool, Option<Vector2I> )`; the point is `Some` whenever the lines are not parallel, exactly when the C++ writes to the pointer |
| `RotatePoint( int* pX, int* pY, ... )` | `rotate_point_xy( &mut x, &mut y, ... )` (in place, like the C++) |
| `SHAPE::Collide( ..., int* aActual, VECTOR2I* aLocation )` and the MTV variant | `Option<&mut i32>`, `Option<&mut Vector2I>`: the C++ behaves differently depending on whether a value is requested, so the nullable-pointer contract is kept |
| `SHAPE_ARC::NearestPoints( X, VECTOR2I& aPtA, VECTOR2I& aPtB, int64_t& aDistSq )` | `nearest_points_<x>( &x, &mut pt_a, &mut pt_b, &mut dist_sq ) -> bool`: in/out, the incoming points are used when no candidate is found |
| `SHAPE_ARC::IntersectLine( SEG, std::vector<VECTOR2I>* )` etc. | `intersect_line( &seg, &mut Vec<Vector2I> ) -> i32` (appends, returns the count) |
| `ClipLine( const BOX2I*, int& x1, ... )` | `clip_line( &box, &mut x1, ... )` (in place) |
| `SHAPE_LINE_CHAIN::Intersect( ..., std::vector<INTERSECTION>& aIp ... )` | `intersect_seg` / `intersect_chain( .., &mut Vec<Intersection>, .. ) -> i32` (appends; returns the total size, as C++) |
| `SHAPE_LINE_CHAIN::ClosestPoints( other, aPt0, aPt1 )`, static `ClosestPoints( ..., aDistSq )`, `ClosestSegments( ... )`, `ClosestSegmentsFast( other, aPt0, aPt1 )` | `Option<( pt0, pt1 )>` / `Option<( pt0, pt1, dist_sq )>`: `None` exactly where the C++ returns false (and leaves the points untouched) |
| `SHAPE_LINE_CHAIN::NearestPoint( const SEG&, int& dist )` | `nearest_point_seg( &seg, &mut dist )` (in/out: untouched for an empty chain, as in C++) |
| `SHAPE_LINE_CHAIN::Split( start, end, aPre, aMid, aPost )` | `split_range( start, end ) -> ( pre, mid, post )` |
| `SelfIntersecting()` / `SelfIntersectingWithArcs()` (`std::optional`) | `Option<Intersection>` |

## Ported C++ tests

| C++ test file | Rust | Cases |
|---|---|---|
| `qa/tests/libs/kimath/math/test_util.cpp` | `tests/test_util.rs` | 2 of 2 (+ 7 extra tests of `KiROUND` / `rescale` edge cases) |
| `qa/tests/libs/kimath/math/test_vector2.cpp` | `tests/test_vector2.rs` | 7 of 7 (including `VECTOR2<float>` / `VECTOR2<unsigned>` casting) |
| `qa/tests/libs/kimath/math/test_box2.cpp` | `tests/test_box2.rs` | 12 of 12 |
| `qa/tests/libs/kimath/geometry/test_segment.cpp` | `tests/test_segment.rs` | 30 of 30 |
| `qa/tests/libs/kimath/geometry/test_eda_angle.cpp` | `tests/test_eda_angle.rs` | 8 of 8 |
| `qa/tests/libs/kimath/geometry/test_vector_utils.cpp` | `tests/test_vector_utils.rs` | 4 of 4 |
| `qa/tests/libs/kimath/test_kimath.cpp` | `tests/test_kimath.rs`, `tests/test_kimath_hash.rs` | 5 of 5: `TestCalcArcCenter3Pts`, `TestInterceptsPositiveX`, `TestInterceptsNegativeX`; `TestHash128`, `TestMMH3Hash` (with a glibc `rand()` replica, so the data is the reference build's) in `test_kimath_hash.rs` (+2 extra: the `rand()` replica, `addData` in pieces) |
| `qa/tests/libs/kimath/math/test_wide_int.cpp` | `tests/test_wide_int.rs`, `tests/test_geometry_predicates.rs` | 7 of 7 (`OrientationSignBeyondDoublePrecision`, `InCircleDelaunayLegalExact` in `test_geometry_predicates.rs`) |
| `qa/tests/libs/kimath/geometry/test_bezier.cpp` | `tests/test_bezier.rs` | 2 of 2 |
| `qa/tests/libs/kimath/geometry/test_ellipse_to_bezier.cpp` | `tests/test_ellipse_to_bezier.rs` | 1 of 1 (all 5 data cases) |
| `qa/tests/libs/kimath/geometry/test_transform_trs.cpp` | `tests/test_transform_trs.rs` | 14 of 14 |
| `qa/tests/libs/kimath/geometry/test_distribute.cpp` | `tests/test_distribute.rs` | 6 of 6 |
| `qa/tests/libs/kimath/math/test_vector3.cpp` | `tests/test_vector3.rs` | 5 of 5 (+1 extra: norm, normalize, stream output) |
| `qa/tests/libs/kimath/math/test_matrix3x3.cpp` | - | not ported (`matrix3x3.h` skipped) |
| `qa/tests/libs/kimath/geometry/test_shape_arc.cpp` | `tests/test_shape_arc.rs` | all 35 test cases (the data tables in full; the chain-to-chain quarter of `CollideArc` is split out as `collide_arc_chain_to_chain`); nothing `#[ignore]`d since G5 |
| `qa/tests/libs/kimath/geometry/test_shape_line_chain.cpp` | `tests/test_shape_line_chain.rs` | 38 of 38 (all data tables; + 2 extra: `Format` / `Parse`, Clipper2 round trip) |
| `qa/tests/libs/kimath/geometry/test_shape_line_chain_collision.cpp` | `tests/test_shape_line_chain.rs` | 9 of 9 (G5): `Collide_LineToLine`, `_LineToArc`, `_ArcToArc`, `_WithClearance`, `_NoClearance` and the corpus tests `Collide_RealContourParity`, `_RealContourGateBoundaries`, `_RealContourSharedSegments`, `SegmentIndex_RealContourCandidateSet` (data `qa/data/triangulation`) |
| `qa/tests/libs/kimath/geometry/test_shape_compound_collision.cpp` | `tests/test_shape_compound_collision.rs` | 1 of 1 (+1 extra: `indexable_subshape`) |
| `qa/tests/libs/kimath/geometry/test_shape_rect_corner.cpp` | `tests/test_shape_compound_collision.rs` | 1 of 1 |
| `qa/tests/libs/kimath/geometry/test_shape_nearest_points.cpp` | `tests/test_shape_nearest_points.rs` | 35 of 35 |
| (`shape_index.h`, `shape_index_list.h`: no C++ tests) | `tests/test_shape_index.rs` | 2 extra |
| `qa/tests/libs/kimath/geometry/test_shape_ellipse.cpp` | `tests/test_shape_ellipse.rs` | 64 of 64 in 63 Rust tests (G6b; the 2 mirror-twice cases share one; randomised cases on the reference build's data via the libstdc++ replicas, GCC's right-to-left argument order; +1 extra check: exact `Format` texts); the 9 `SHAPE::Collide` cases are no longer `#[ignore]`d (G5) |
| `qa/tests/libs/kimath/geometry/test_chamfer.cpp` | `tests/test_chamfer.rs` | 5 of 5 |
| `qa/tests/libs/kimath/geometry/test_dogbone.cpp` | `tests/test_dogbone.rs` | 1 of 1 |
| `qa/tests/libs/kimath/geometry/test_fillet.cpp` | `tests/test_fillet.rs` | 2 of 2 (all 4 data cases each) |
| `qa/tests/libs/kimath/geometry/test_oval.cpp` | `tests/test_oval.rs` | 3 of 3 |
| `qa/tests/libs/kimath/geometry/test_roundrect.cpp` | `tests/test_roundrect.rs` | 6 of 6 |
| `qa/tests/libs/kimath/geometry/test_intersection.cpp` | `tests/test_intersection.rs` | 8 of 8 |
| `qa/tests/libs/kimath/geometry/test_circle.cpp` | `tests/test_circle.rs` | 7 of 7 |
| `qa/tests/libs/kimath/geometry/test_arc_chord_params.cpp` | `tests/test_arc_chord_params.rs` | 10 of 10 |
| `qa/tests/libs/kimath/geometry/test_half_line.cpp` | `tests/test_half_line.rs` | 3 of 3 |
| `qa/tests/libs/kimath/geometry/test_arc_construction.cpp` | `tests/test_arc_construction.rs` | 21 of 21 |
| `qa/qa_utils/include/qa_utils/wx_utils/unit_test_utils.h` (`CheckUnorderedMatches`), `BOOST_CHECK_CLOSE` / `BOOST_CHECK_SMALL`, `pcbIUScale.mmToIU` | `test_utils::{check_unordered_matches, is_close_pct, is_small, pcb_mm_to_iu}` | added in G2 |
| `qa/qa_utils/include/qa_utils/numeric.h` | `test_utils::{is_within, is_within_wrapped, is_within_bounds, is_within_and_below, is_within_and_above}` | all |
| `qa/qa_utils/include/qa_utils/geometry/geometry.h` | `test_utils::{is_vec_within_tol, is_box_within_tol}` | `ChainsAreCyclicallyEqual` and the printers need shapes: later |
| `qa/qa_utils/geometry/seg_construction.cpp` | `test_utils::{build_h_seg, build_v_seg}` | all |
| `qa/tests/libs/kimath/geometry/geom_test_utils.h/.cpp` | `test_utils::{Quadrant, is_in_quadrant, segment_completely_in_quadrant, segment_ends_in_quadrant, segment_completely_within_radius, is_point_at_distance, are_points_near_circle, are_perpendicular, segments_have_same_end_points, triangle_min_angle_deg, is_outline_valid}` | `IsOutlineValid` added in G3; `FilletPolySet`, `IsPolySetValid`, `toString( POINT_TYPE )`, `operator<<( TYPED_POINT2I )` need shapes / `point_types.h`: later |
| `qa/tests/libs/kimath/geometry/fixtures_geometry.h` | `test_utils::CommonTestData` (`unique_points`, `holey_points`, `holey_segments`) | the polygon sets are built by the shapes package |
| `qa/tests/libs/kimath/geometry/test_packed_rtree.cpp` | `tests/test_packed_rtree.rs` | 11 of 11 |
| `qa/tests/libs/kimath/geometry/test_dynamic_rtree.cpp` | `tests/test_dynamic_rtree.rs` | suite `DynamicRTree`: 22 of 22 (+1 extra: `Overlapping` order equals `Search` order); suite `CowRTree`: 0 of 6 (GAP-4b) |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set.cpp` | `tests/test_shape_poly_set.rs` | 3 of 3 |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_arcs.cpp` | `tests/test_shape_poly_set_arcs.rs` | 3 of 3 (the C++ cases commented out stay commented out) |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_iterator.cpp` | `tests/test_shape_poly_set_iterator.rs` | 6 of 6 |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_distance.cpp` | `tests/test_shape_poly_set_distance.rs` | 1 of 1 (all 7 data cases) |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_split_outlines.cpp` | `tests/test_shape_poly_set_split_outlines.rs` | 2 of 2 |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_collision.cpp` | `tests/test_shape_poly_set_collision.rs` | 8 of 8 (none needs the triangulation, so nothing is `#[ignore]`d) |
| `qa/tests/libs/kimath/geometry/test_shape_poly_set_triangle_index.cpp` | `tests/test_shape_poly_set_triangle_index.rs` | 6 of 6 (the cases relying on `Collide()` caching through `const_cast` use `collide_shape_mut`; pointer identity in `TriPointersSurviveMove` becomes "same triangles, moved") |
| `qa/tests/libs/kimath/geometry/test_poly_triangulation.cpp` | `tests/test_poly_triangulation.rs` | 39 of 39 (data under `qa/data/triangulation`); `FractureEdgeIndexMatchesLinearScan` can only check the indexed fracture (the linear variant is not ported, D-P7); `ConcurrentTriangulation` / `ParallelPartitionTriangulation` run sequentially (the latter with `cache_triangulation_with( .., true, .. )`) |
| `qa/tests/libs/kimath/geometry/test_poly_ystripes_index.cpp` | `tests/test_poly_ystripes_index.rs` | 3 of 3 (`std::mt19937` / `uniform_int_distribution` replicas) |
| `test_shape_poly_set_collision.cpp`, triangulation-dependent cases | - | none: all 8 cases use segments / points, already ported; the triangle-based collisions are covered by the two files above, `tri_collide` and the `coll_oracle` |
| `fixtures_geometry.h` (poly sets), `geom_test_utils.h` (`IsPolySetValid`, `FilletPolySet`), `qa_utils/geometry/poly_set_construction` / `line_chain_construction` (`BuildPolyset`, `BuildHollowSquare`, `BuildRectChain`, `BuildSquareChain`) | `tests/common_poly/mod.rs` | added in G4a (closes GAP-3 except `ChainsAreCyclicallyEqual`) |
| (Clipper2 fidelity patch, `gnu_sort.rs`) | `tests/test_stdsort.rs` | `std_sort` and the vendored Clipper2's private libstdc++ sort copy give identical permutations on 4000 tie-heavy arrays |

Notes on the ported tests:

- `static_assert`s (`Constexpr` tests) are checked at run time; only the constructors are
  `const fn`.
- Tests with randomised data use `test_utils::Mt19937` / `Mt19937_64` (standard engines) and
  replicas of the libstdc++ distributions, and draw values in the order GCC evaluates the C++
  constructor arguments, so they run on the same data as the reference build.
  `SquaredDistanceLongSegmentRandom` additionally asserts a checksum over all 2000 inputs and
  results that was produced by the C++ test code (GCC 13, x86-64).
- The two tests whose C++ reference value is computed in `long double`
  (`SquaredDistanceLongSegmentRandom`, `RandomAgainstLongDouble`) check the same bound exactly
  with 128-bit integers instead.
- `BasicDouble` in `test_box2.cpp` instantiates `IsVecWithinTol<VECTOR2I>` with double arguments
  (they are truncated to ints); the port reproduces that and adds the exact comparison.

In-module unit tests (`#[cfg(test)]`, 55 tests) cover additional behaviour of every module.

## Verification against the C++ code

Two oracles: `geom_oracle` (package G1, compiles the kimath sources it needs with stub wx
headers) and `shapes_oracle` (package G2, links the static libraries of the reference build).

### `conv_oracle` (G6b)

`tests/oracle/conv_oracle.cpp` links `libkimath.a` (+ `libcore.a`, `libclipper2.a`,
`libkicommon.so`) of the reference build and prints 11 sections, 250,000 cases (+ the libm
probe); replay: `tests/oracle_conv_diff.rs` with `tests/oracle/conv_sections.rs`, golden
`tests/golden/conv_oracle.txt`.  All sections match bit for bit:

| Section | Cases | Content |
|---|---|---|
| `cbrt` | 20000 | `std::cbrt` against the port of glibc's (D-G1): random bit patterns (incl. NaN, infinities, subnormals), scaled values; libm-independent, so it is checked on every target |
| `libm:conv_circle` | 20000 | both `TransformCircleToPolygon` overloads (into non-empty buffers too), `TransformRingToPolygon` (hole / no hole), `SHAPE_CIRCLE::TransformToPolygon`; radii 0..5e7 and negative, max errors 1..50000, both `ERROR_LOC`s, min segment counts 0..48 / 7 |
| `libm:conv_oval` | 20000 | `TransformOvalToPolygon`, `SHAPE_SEGMENT::TransformToPolygon`: zero / nm / board-length tracks, cardinal, 45 degree and arbitrary directions, widths 0.1..10 mm, tiny and odd |
| `libm:conv_rect` | 30000 | `TransformRoundChamferedRectToPolygon` (radii up to and beyond half the side, chamfer ratios 0..0.5, all 16 corner sets), `TransformTrapezoidToPolygon` (horizontal / vertical deltas, shrinking into triangles), `CornerListToPolygon` on convex corner lists with per-corner radii; inflation positive, zero and negative; every rotation class |
| `libm:conv_arc` | 20000 | both `ConvertArcToPolyline` overloads (incl. radial offsets), `TransformArcToPolygon`, `SHAPE_ARC::TransformToPolygon`; arcs from centre / angle (tiny, full, negative sweeps) and the shared degenerate `arcPoints` |
| `libm:conv_roundrect` | 20000 | `ROUNDRECT` (constructor with / without normalisation, `OutsetFrom`, `GetInflated`, getters, `BBox`, `TransformToPolygon` into empty and non-empty buffers; negative / zero sizes, radii beyond the limit), `SHAPE_RECT::TransformToPolygon` / `Outline` with radius |
| `libm:conv_corner_ops` | 20000 | `ComputeChamferPoints`, `ComputeDogbone` (with / without slots) on axis-aligned, collinear, short, random and non-coterminous segment pairs in both orientations |
| `libm:conv_shape_utils` | 20000 | every `KIGEOM` function of `shape_utils` (all directions incl. `LAST` / `UNDEFINED`), `GetOvalKeyPoints` (all flag sets), `ConvertToChain` |
| `libm:intersection` | 30000 | `INTERSECTION_VISITOR` on every ordered pair of the 7 kinds, with and without contact, both directions accumulating one contact; half the cases on a 100 nm grid (collinear, tangent, concentric contacts); `GetNearestPoint` (single and list) |
| `libm:shape_ellipse` | 30000 | `SHAPE_ELLIPSE`: all constructors (degenerate / swapped radii, DXF style), getters, `BBox` (with clearance), `GetLength`, `Format`, per case 3 queries (`PointInside` with accuracy, both `SquaredDistance`s, `NearestPoint`, `Collide( SEG )` with every out-parameter combination, `Intersect( SEG )` as segment and line), `ConvertToPolyline`, `TransformToPolygon`, `Intersect` with circles, arcs and ellipses / elliptical arcs, `Rotate`, `Mirror`, `Move`, the setters, `operator==` |

- Regenerate: `tests/oracle/build_conv_oracle.sh`.  A full dump is several GB; find a mismatch
  with `build_conv_oracle.sh --only SECTION --count N --dump cpp.txt` and
  `KICAD_CONV_ORACLE_ONLY=SECTION KICAD_CONV_ORACLE_COUNT=N KICAD_GEOM_ORACLE_DUMP=rust.txt`
  (both run just that section; regenerate the full golden afterwards).
- Inputs avoid C++ undefined behaviour (filtered identically on both sides): inflating
  trapezoids thinner than 1:50 (`( int )( aInflate / tanAngle2 )` out of range at the acute
  corners, then signed overflow), shrinking trapezoids with a half size of 0 (division by zero),
  `MakeRegularPolygonPoints` across flats with fewer than 3 sides (division by `cos( 90 deg )`),
  arcs whose rounded points are collinear (clamped centre near the `int` limits overflows in
  `SEG::LineProject`), `ConvertToChain` of zero-length ovals, `Outline()` of a rectangle whose
  polygonisation is empty, `GetLength()` of ellipses with a major radius above 1 mm (only slow:
  the adaptive integration runs to its depth limit).

### `coll_oracle` (G5)

`tests/oracle/coll_oracle.cpp` links `libkimath.a` (+ `libcore.a`, `libclipper2.a`,
`libkicommon.so`) and prints 7 sections; replay: `tests/oracle_coll_diff.rs` with
`tests/oracle/coll_sections.rs`, golden `tests/golden/coll_oracle.txt`.  Shapes of 13 kinds:
rectangle, rounded rectangle, segment (zero length, widths), circle, arc (widths, effectively
straight three-point arcs), simple polygon, line chain without arcs (short; 64..200 point random
walks for the index gates) and with arcs, poly set (star / rectangle outlines, holes; cold,
cached and indexed triangulation), compound (1..3 members of the first 8 kinds), poly-set
triangle, ellipse (full / arc, any rotation), null; at scales 20 .. 2e7 nm.  Every record holds
the inputs' bounding boxes and, for both orders, `Collide( b, cl )`, `( b, cl, &actual )`,
`( b, cl, &actual, &loc )`, `( b, cl, nullptr, &loc )` and `Collide( b, cl, &mtv )` (with preset
outputs, so untouched outputs are checked too), or `NearestPoints` in both orders.  All sections
match bit for bit:

| Section | Cases | Content |
|---|---|---|
| `libm:coll_matrix` | 100000 | random ordered pairs of all 13 x 13 kinds, clearance 0 / 1..10 / up to half the scale |
| `libm:coll_touching` | 40000 | the same with B moved so that its bounding box touches A's (+-2 nm, along an edge or at a corner) |
| `libm:coll_degenerate` | 20000 | degenerate inputs: rectangles of size -1..1, minimal rounded rectangles, zero-length segments, radius 0..3 circles, radius 1..3 arcs, collapsed simple polygons, empty / one-point / duplicate-point chains, empty / 1 nm poly sets, empty compounds, 1 nm triangles and ellipses |
| `libm:coll_nearest` | 60000 | `SHAPE::NearestPoints` on random pairs of all kinds (also null / ellipse: false, points untouched) |
| `libm:coll_nearest_touching` | 20000 | the same on touching pairs |
| `coll_chain_index` | 5000 | 20..300 point random walks against each other or against a +-2 nm shifted copy of a part of A, open / closed, clearances -1 .. 100 steps: every `SEGMENT_INDEX` gate of `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )` (probe, both index orientations, candidate / direct scan, exact-hit exit), plus `NearestPoints` |

- Regenerate: `tests/oracle/build_coll_oracle.sh` (about 45 s); find a mismatch with
  `--dump cpp.txt` and `KICAD_GEOM_ORACLE_DUMP=rust.txt`.  The replay takes about 35 s.
- Inputs avoid the C++ undefined behaviour (identically on both sides): an arc built from
  centre, start and angle whose points coincide after rounding (its three-point centre is
  clamped near the `int` limits and later arithmetic overflows) is replaced by a quarter circle
  (`safeArc`); effectively straight three-point arcs (clamped centre too) are generated only
  where the matrix routes them to segments, i.e. not for `NearestPoints` and not against
  ellipses (`ConvertToPolyline` converts their radius, see the `shapes_oracle` notes).
- The oracle found one bug of the G2 dispatch: line chain / segment / simple / triangle against
  an arc are `CollCaseReversed` in C++ (the MTV is negated although the arc functions never
  write it); fixed in G5.

### `tri_oracle` (G4b)

`tests/oracle/tri_oracle.cpp` links `libkimath.a` (+ `libcore.a`, `libkicommon.so` for
`ADVANCED_CFG`, run with an empty config directory = defaults) and prints 11 sections; replay:
`tests/oracle_tri_diff.rs` with `tests/oracle/tri_sections.rs`, golden `tests/golden/tri_oracle.txt`.
Every record holds the complete output (success flag, all result vertices incl. subdivision
Steiner points, every triangle's indices in order).  All sections match bit for bit:

| Section | Cases | Content |
|---|---|---|
| `tri_chain` | 40000 | `TesselatePolygon( SHAPE_LINE_CHAIN )` on random rings (self-intersecting, grids), x-monotone polygons (with / without duplicate points), serpentines, subdivided squares (collinear runs, decimation), star polygons, tiny / collinear / all-duplicate / below-simplification chains, jittered noisy outlines, 40..400-vertex outlines, both orientations, scales 60 nm .. 4e8 nm; 1/8 also with hint data (matching and non-matching) |
| `tri_polygon` | 20000 | `TesselatePolygon( POLYGON )`: rectangles with grid holes (rects, monotone, all-duplicate), monotone outlines with random (overlapping, crossing) rect holes, holes sharing an outline vertex (issue 24059), stars with holes; random orientations |
| `libm:tri_cache` | 10000 | `CacheTriangulation( simplify, {}, index )` on multi-outline sets with holes, Clipper2 zone fills (rect unions minus rect / star cut-outs), random rings, fractured or not; then nothing / `Move` + re-cache / `Rotate` (90 degree multiples) / `Mirror` / copy + `SetVertex` + re-cache; up-to-date flag, every triangulated polygon (source index, vertices, triangles), index presence |
| `tri_large` | 24 | 8k..30k vertex outlines (reflex index path) |
| `tri_partition` | 12 | 52k..120k vertex outlines through `CacheTriangulation`: balanced partition (incl. the x86 `int` wrap of `splitPolygonBalanced` for 8e8 nm wide outlines), with an (inline) task submitter and without |
| `tri_fractions` | 10000 | `PartitionAreaFractionsForTesting`, 0..8 leaves |
| `tri_refine` | 10000 | `Refine()` of jittered grid meshes with random diagonals, both orientations |
| `ystripes` | 5000 | `POLY_YSTRIPES_INDEX::Build` + 40 `Contains` queries (vertices, around the bbox; accuracies 0..10000) |
| `tri_collide` | 5000 | `Collide( SHAPE* )` (rect, 1x1 rect, segment, circle, compounds of segments / circles / rects; clearances -5..; all out-parameter combinations) on cold, cached and indexed sets, then `AnyTriangleHit( point )`; the port's `&self` variant is asserted equal to the caching one |
| `seg_intersects_line` | 20000 | `SEG::IntersectsLine` incl. infinite / NaN / huge slopes and offsets (GAP-P4) |

- Regenerate: `tests/oracle/build_tri_oracle.sh` (about 2.5 minutes); find a mismatch with
  `--dump cpp.txt` and `KICAD_GEOM_ORACLE_DUMP=rust.txt`.  The replay takes about 2.5 minutes.
- Not covered: arcs in triangulated sets (`ClearArcs` path; arc construction is libm and G2/G5
  territory); shapes other than rect / segment / circle / compound in `Collide( SHAPE* )` are
  covered by the `coll_oracle` (G5).

### `sps_oracle` (G4a)

`tests/oracle/sps_oracle.cpp` links `libkimath.a` of the reference build (which contains the
bundled Clipper2 built with `USINGZ`) and prints 9 sections, 328,146 cases (replay:
`tests/oracle_sps_diff.rs` with `tests/oracle/sps_sections.rs`, golden
`tests/golden/sps_oracle.txt`, 9 lines).  Every record holds the complete resulting polygon set
(points, arc shape indices, arcs) and the other results.  All sections match bit for bit:

| Section | Cases | Content |
|---|---|---|
| `sps_boolean` | 20000 | `BooleanAdd/Subtract/Intersection/Xor`, one- and two-argument overloads, `Simplify`, on random rings (degenerate grids, self-intersecting), grid-aligned pad rectangles (touching / overlapping, both orientations), zone rectangles with holes, combs, empty / one / two point sets; result, `Area`, `ArcCount` |
| `libm:sps_boolean_kicad` | 20000 | the same plus round pads (8..64 segments), chained track ovals and outlines with arcs (Z callback / arc buffer path) |
| `libm:sps_inflate` | 20000 | `Inflate` (with / without `aSimplify`), `Deflate`, `InflateWithLinkedHoles`, `OffsetLineChain`, `SHAPE_LINE_CHAIN::OffsetLine`; all five `CORNER_STRATEGY`s, max errors 500..25000, amounts from +-1 nm to +-2 mm |
| `libm:sps_fracture` | 5000 | `Fracture( true / false )`, `Unfracture`, fracture round trips, booleans and `Simplify` on fractured sets; zones with up to 40 holes and outlines of up to 2500 points (89 cases use the fracture edge index, 58 the bridge removal of `booleanOp`) |
| `sps_simplify` | 20000 | `Simplify`, `SimplifyOutlines`, `NormalizeAreaOutlines`, `RebuildHolesFromContours`, `BuildPolysetFromOrientedPaths` (NonZero / EvenOdd), `RemoveNullSegments`, `splitSelfTouchingOutlines`, exterior waists (`splitCollinearOutlines`) |
| `libm:sps_chamfer_fillet` | 13146 | `Chamfer`, `Fillet`, `ChamferPolygon`, `FilletPolygon` (distances 0..2 mm), and the set afterwards (null segments removed) |
| `libm:sps_query` | 10000 | `Area`, counts, `HasHoles`, `HasTouchingHoles`, self intersection, `BBox`, `BBoxFromCaches`, `GetHash`, `Format`, every global / relative vertex index (`GetRelativeIndices`, `GetGlobalIndex`, `CVertex`, `IsVertexInHole`, `GetNeighbourIndexes`), all iterators; per set 12 queries: `Contains` (with and without bbox caches), `PointInside`, `PointOnEdge`, `SquaredDistance` (+ nearest), `Collide` (point, segment, `SHAPE_SEGMENT`, `SHAPE_CIRCLE`; all out-parameter combinations), `CollideVertex` / `CollideEdge` (with / without closest index), `SquaredDistanceToSeg`, both `SquaredDistanceToPolygon`s; `GenerateHatchLines` |
| `libm:sps_edit` | 20000 | `InsertVertex`, `RemoveVertex`, `SetVertex`, `RemoveOutline`, `RemoveContour`, `DeletePolygon`, `Append` (set, arc with / without max error, point), `Subset`, `UnitSet`, both `Mirror`s, `Move` (hash caching: stale hash after an edit, recomputed in copies, `CloneDropTriangulation`), `Scale`, `Rotate`, `NewHole`, `Parse` of round-trip and corrupted texts; then the set and its hash |

- Regenerate: `tests/oracle/build_sps_oracle.sh`; find a mismatch with `--dump cpp.txt` and
  `KICAD_GEOM_ORACLE_DUMP=rust.txt`.
- Inputs avoid the C++ undefined behaviour (filtered identically on both sides): open-path
  offsets without two distinct points (Clipper2 crashes in `DoBevel`), `NormalizeAreaOutlines` /
  `RemoveNullSegments` of empty sets or sets with empty contours, chamfer / fillet of contours
  with fewer than 3 distinct points (division by zero), hatch lines with a line length of 0
  (`SEG::IntersectsLine` then sees an infinite slope, see GAP-P4).

### Clipper2 re-validation (G4a)

The vendored, patched crate was re-run against KiCad's bundled C++ Clipper2 (`thirdparty/clipper2`,
g++ 13, `-DUSINGZ`) with the research harness of `docs/research/artifacts/polygon-clipping/`
(driver `cpp/driver.cpp`, generator `gen_cases.py`, Rust driver `body_clipper2_rs.rs` built
against `rust/vendor/clipper2-rs`; scratch project in `rust/target/revalidate-clipper`): the
complete output files are byte-identical for seed 20261109 (9,867 cases) and seed 7 (9,873
cases), i.e. every category including all `kicad_*` and random ones, and for the 12,780-line
quirk set (zero-area offset groups).  The second quirk set contains the empty-path offset group
on which the C++ driver segfaults (known upstream undefined behaviour) and was not compared.

### `slc_oracle` (G3)

`tests/oracle/slc_oracle.cpp` links `libkimath.a` of the reference build (like `shapes_oracle`,
plus `-DUSINGZ` for the Clipper2 points; protected helpers are reached with
`#define protected public`).  Chains are built from points at several scales (grids with many
collinear / duplicate points, nm-scale, board-scale), arcs (three-point arcs, often degenerate;
full circles; arcs connected to the chain end, i.e. shared points; sub-chains appended), closed
on their first point (shared point merging, `fixIndicesRotation`) or not.  Every case has its
own generator (seeded with section seed and case index), so single cases can be skipped.
Sections (all bit for bit, `tests/oracle_slc_diff.rs`, golden `tests/golden/slc_oracle.txt`):

| Section | Cases | Content |
|---|---|---|
| `slc_build` / `libm:slc_build_arcs` | 49,598 / 51,661 | the chain state (points, shape pairs, arcs) and every per-point query (`ShapeCount`, `NextShape` incl. invalid indices, `IsSharedPt`, `IsPtOnArc`, `IsArcSegment`, `IsArcStart`, `IsArcEnd`, `ArcIndex`, `Length`, `Area`, `BBox`) after construction, after toggling `SetClosed` and back, and after a `convertToClipper2` / Clipper2-constructor round trip with randomly rotated paths |
| `libm:slc_edit` / `libm:slc_edit_arcs` | 30,000 / 39,996 | one edit, then the state: `Insert` (point, arc at vertex 0), `Replace` (point, chain), `Remove`, `RemoveShape`, `SetPoint`, `Split`, `ClearArcs`, `RemoveDuplicatePoints`, `Simplify( 0 / 1 / 10 / 1000 / 2 mm )`, `Simplify2`, `Reverse`, both `Mirror`s, `Rotate`, `Move`, `splitArc`, `convertArc`, `Remove` + `Insert` |
| `libm:slc_query` / `libm:slc_query_arcs` | 15,000 / 15,000 | `Find`, `FindSegment`, `PathLength`, `PointAlong`, both `NearestPoint`s, `NearestSegment`, `Distance` (both), `SquaredDistance`, `PointInside` (with and without bbox cache), `PointOnEdge`, `EdgeContainingPoint`, `CheckClearance`, `Collide` (point and segment, all out-parameter combinations), `Intersect` (segment; chain with both flags and a given bbox), `Intersects`, `SelfIntersecting`, `SelfIntersectingWithArcs`, `ClosestPoints`, `ClosestSegmentsFast`, `Format`, `operator!=`, `CompareGeometry` (plain, epsilon, cyclic), both `Slice`s, `Split( start, end )`, `POINT_INSIDE_TRACKER`, `Parse` (incl. the count sanity checks) |
| `x86_casts` | 6,000 | `SHAPE::Distance()` / `Distance( p, outline )` of chains without segments (`INT_MIN`), `VECTOR2I` / `VECTOR2L` / `VECTOR2<unsigned>` from `VECTOR2D` and `VECTOR2<float>` with NaN, infinities, the limits and values beyond, `BOX2D::GetSizeMax()`, `ConstructFromStartEndAngle`'s double width |

- Regenerate: `tests/oracle/build_slc_oracle.sh`; find a mismatch with `--dump cpp.txt` and
  `KICAD_GEOM_ORACLE_DUMP=rust.txt`.
- `tests/golden/slc_oracle_skip.txt` lists the 4 cases (of `libm:slc_edit_arcs`) on which the
  C++ overflows a signed integer (D-S7); the port panics on them with overflow checks.  The Rust
  test fails on any other panic; `KICAD_SLC_ORACLE_WRITE_SKIPS=1` rewrites the list (then
  regenerate the golden file), `KICAD_SLC_ORACLE_SHOW_PANICS=1` prints the messages.
- Inputs avoid the other C++ undefined behaviour (filtered identically on both sides):
  `Insert( arc )` at vertices > 0 (D-S1), `NearestPoint( p, false )` / `Split( start, end )` where
  the C++ reads past the shape array (D-S2), `Replace( chain )` of closed chains (D-S3), the
  Clipper2 constructor with consecutive equal points (D-S5), `Parse` reading past the end of the
  text (D-S6), `convertArc` with indices that stay negative, nearly straight three-point arcs
  (their clamped centre overflows `ConstructFromStartEndCenter` once they are edited).
- The `wxCHECK`s reached (`Segment()` and `Insert()` out of range, ...) print to stderr in the
  oracle; the port takes the same early returns.

### `helpers_oracle` (G6a)

`tests/oracle/helpers_oracle.cpp` links `libkimath.a` (+ `libcore.a`, `libkicommon.so`) of the
reference build, shares generator / record / hash with the other oracles (`oracle_common.h`)
and prints 13 sections, 540,000 cases; replay: `tests/oracle_helpers_diff.rs` with
`tests/oracle/helper_sections.rs`, golden `tests/golden/helpers_oracle.txt`.  All sections
match bit for bit:

| Section | Cases | Content |
|---|---|---|
| `libm_probe` | 200000 | libm gate (as in the other oracles) |
| `libm:bezier_poly_i` | 30000 | `BEZIER_POLY::GetPoly( VECTOR2I )`, quadratic and cubic, both constructors, max errors 1, 2, 5, 10, 100, 1000, 2500, 5000, 10000, 20000, 0, -5; random / collinear / nearly collinear / all-equal / closed-loop / control-on-endpoint / S-curve / duplicate-control polygons spanning 1..1e6 times the error |
| `libm:bezier_poly_d` | 20000 | `GetPoly( VECTOR2D )` (appending to a non-empty vector), fractional control points, max errors 0.01 .. 5000, 0, -1 |
| `bezier_template` | 20000 | `BEZIER<int>` / `BEZIER<double>`: `PointAt`, `Split`, `SubCurve` (t in and out of [0, 1], t1 <= t0) |
| `libm:ellipse` | 30000 | `ELLIPSE<int>` / `<double>`, both constructors, all methods, `Mirror` both ways, `TransformEllipseToBeziers` |
| `libm:convex_hull` | 30000 | 0..60 points: duplicates, collinear, ring, full board range; the poly-set overloads with position / rotation |
| `libm:transform_trs` | 40000 | `Apply` / `InverseApply` (int, double), round trips, `RescaleAround`, the scale predicates, linear scales, `==`, setters; mirrored (negative), non-uniform, ulp-noisy and zero scales |
| `libm:transform` | 30000 | `TRANSFORM`: coordinates, boxes, `InverseTransform`, `MapAngles`, `std::hash` |
| `distribute` | 20000 | both functions, 0..12 items, sorted and unsorted |
| `libm:grid_geometry` | 30000 | `Snap`, `Contains`, `Area`, `TakesPrecedenceOver`; degenerate pitch, negative extent |
| `geometry_predicates` | 40000 | all four predicates; full `int` range (double fallback), cocircular, collinear |
| `vector3` | 20000 | `VECTOR3I` / `VECTOR3D`: all methods and `operator<<` |
| `hashes` | 30000 | `MMH3_HASH` (`addData` in pieces mixed with `add( int32_t )`, `addDataV1`, string / vector overloads, `reset`), `MD5_HASH` (`Hash`, `Finalize` twice, `Format`, copies, `Init`), `HASH_128`, `std::hash` of `double` / `float` / `std::string`, `hash_val`, `hash_combine` on random byte strings of 0..1000 bytes |

- Regenerate: `tests/oracle/build_helpers_oracle.sh` (needs the reference build and wx-config);
  find a mismatch with `--dump` / `KICAD_GEOM_ORACLE_DUMP` as for the other oracles.
- Inputs avoid C++ undefined behaviour: no singular `TRANSFORM` (`InverseTransform` divides by
  the determinant), `add( int32_t )` only at a length that is a multiple of 4 (D-H2), no
  2- or 5-point `BEZIER_POLY` (`wxASSERT`).  `wxSetAssertHandler( nullptr )` silences the
  `KiROUND` overflow messages; results are unaffected.

### `shapes_oracle` (G2)

`tests/oracle/shapes_oracle.cpp` links `libkimath.a`, `libcore.a`, `libclipper2.a` and
`libkicommon.so` of the reference build (`/home/user/kicad-oracle/build`, see
`rust/docs/ORACLE.md`), so the collision code under test is the real, complete one.  It shares
generator, record format and hashing with `geom_oracle.cpp` (`tests/oracle/oracle_common.h`) and
prints 17 sections, 606,000 cases: `libm_probe`, `libm:circle`, `libm:circle_tan_tan_pt`,
`libm:arc_props` (all constructors incl. tangent-arc and `ConstructFromStartEnd*`, bbox,
angles, length, `Reversed`, `Reverse`, `BOX2I_MINMAX`), `libm:arc_polyline`
(`ConvertToPolyline` with actual error, `SHAPE_LINE_CHAIN( SHAPE_ARC )`), `libm:arc_collide`
(point and segment, all out-parameter combinations), `libm:arc_intersect` (`IntersectLine`,
`Intersect` with circles and arcs, `NearestPoint`, `NearestPoints` with segments, circles and
arcs), `libm:arc_transform`, `shape_collide` (rect / segment / circle / simple / null /
compound / line chain pairs through `SHAPE::Collide`, with distance, location and MTV where the
C++ supports it, and `GetClearance`), `libm:shape_collide_arc` (arcs against circles,
segments, simple polygons, line chains with and without arcs, arcs, straight arcs against
rectangles; both directions), `shape_distance` (direct `Collide`, `SquaredDistance`,
`Distance`, `PointInside`, `PointOnEdge`, `EdgeContainingPoint`, `BBox`, `Format`, rect helpers
and `Outline`), `libm:shape_segment` (`BySizeAndCenter`, `Is45Degree`, rotations),
`geometry_utils` (snapping, clamping, `ClipLine`, `BoxHitTest`), `libm:geometry_utils`
(`GetArcToSegmentCount`, `CircleToEndSegmentDeltaRadius`), `lines` (`HALF_LINE`, `LINE`,
`ClipHalfLineToBox`, `ClipLineToBox`), `libm:arc_chord_params`, `libm:arc_construction`.
All sections match bit for bit (`tests/oracle_shapes_diff.rs`, golden file
`tests/golden/shapes_oracle.txt`).

- Regenerate: `tests/oracle/build_shapes_oracle.sh` (needs the reference build and wx-config).
- Find a mismatch: `build_shapes_oracle.sh --dump cpp.txt`, run the test with
  `KICAD_GEOM_ORACLE_DUMP=/abs/path/rust.txt`, diff the files.
- Inputs avoid undefined behaviour of the C++ (the tests filter them identically on both sides):
  arcs whose radius is converted to `int` while out of range (`NearestPoint(s)`,
  `ConvertToPolyline` of arcs with radius >= INT_MAX/2), `ConstructFromStartEndAngle` /
  tangent-arc constructions with nearly straight results (the clamped center overflows
  `RotatePoint` / `p + KiROUND( distPC * cos )`), moving arcs so far that `update_values()`
  overflows, and `Distance()` when the squared distance is `ECOORD_MAX` (D4).
- The `wxCHECK`s reached by the inputs (`ConstructFromTanTanPt` without solution,
  `SHAPE_LINE_CHAIN::Segment()` out of range) print to stderr in the oracle; the port takes the
  same early returns.

### `geom_oracle` (G1)

`tests/oracle/geom_oracle.cpp` links the original kimath sources (`util.cpp`, `seg.cpp`,
`eda_angle.cpp`, `vector_utils.cpp`, `trigo.cpp` and the headers) with stub wx headers
(`tests/oracle/stub`), feeds them deterministic pseudo-random inputs and prints, for each of 26
sections, the number of cases and a hash over a textual record of all inputs and results
(1,015,001 cases).  The output is committed as `tests/golden/geom_oracle.txt`;
`tests/oracle_diff.rs` replays the same inputs through the port and compares the hashes, so
every result must match bit for bit (doubles are compared by bit pattern).  The inputs were
checked with UBSan to stay clear of undefined behaviour in the C++.

- Regenerate the golden file: `tests/oracle/build_oracle.sh` (needs g++, no wxWidgets).
- Find a mismatch: `build_oracle.sh --dump cpp.txt`, run the test with
  `KICAD_GEOM_ORACLE_DUMP=rust.txt`, diff the files.
- WebAssembly: `tests/oracle/run_wasm_check.sh` builds the same sections for
  `wasm32-unknown-unknown` and runs them under node.js.  Result at the time of writing: all
  libm-independent sections (and two of the libm-dependent ones) are bit-identical to the C++;
  the others differ as described in GAP-1.

**Spatial oracle (work package G7).** `tests/oracle/spatial_oracle.cpp` is compiled with g++
against the original headers / sources (`libs/kimath/include/geometry/rtree/*.h`,
`thirdparty/delaunator`, `thirdparty/nanoflann`) and hashes, per section, every result bit:
the final permutation of tagged elements (so the order of equal elements is checked), r-tree
iteration orders and query visit orders (with early termination), nearest neighbours,
delaunator triangles / half-edges / hull arrays / areas, kd-tree results in their order.
Output: `tests/golden/spatial_oracle.txt` (9 lines); replay: `tests/oracle_spatial.rs` with the
generators in `tests/oracle/spatial_sections.rs`.  All 10,040 cases match:

| Section | Cases | Content |
|---|---|---|
| `std_sort` | 3040 | lengths 0..6000 (incl. 15/16/17/32/33 thresholds), 1 key to all-distinct keys, sorted / reversed / organ pipe / sawtooth / interleaved patterns, McIlroy antiquicksort adversary up to 9850 elements (heap sort fallback) |
| `std_partial_sort_nth_element` | 1500 | random `middle` / `nth` on the same arrays |
| `std_heap_priority_queue` | 1000 | `make_heap`, `sort_heap`, random push/pop scripts on a min `priority_queue` |
| `packed_rtree_2d` / `_3d` | 400 / 150 | trees of 0..1500 boxes (node-boundary sizes, board-like, clustered with equal centres, pad grids, points, full `int` range, tiny overlapping, duplicates); iteration order + 25 queries each |
| `dynamic_rtree_2d` / `_3d` | 300 / 150 | insert, remove (exact box, wrong box = fallback path, missing item), re-insert, `BulkLoad` + insert/remove; after each step iteration order, 12 queries, `Overlapping`, `NearestNeighbors` |
| `delaunator` | 2500 | 3..1600 points: uniform, tiny ranges (duplicates, collinear triples), pad grids, full grids, nm jitter, circles, near-collinear lines, two rows, exactly collinear (error) |
| `nanoflann` | 1000 | outline end points (chains with nm gaps, coarse-grid duplicates, single axis, random); 24 queries each: sorted and unsorted `radiusSearch`, `knnSearch( 8 )` |

- Regenerate: `tests/oracle/build_spatial_oracle.sh`; find a mismatch with `--dump cpp.txt` and
  `KICAD_SPATIAL_ORACLE_DUMP=rust.txt` (one line per case).
- WebAssembly: `tests/oracle/run_wasm_spatial_check.sh`: all sections bit-identical (no libm
  is involved).
- The research verification of `docs/research/spatial.json` (65,550 packed / 187,650 dynamic
  queries, 37,096 delaunator sets, 192,000 kd-tree queries, all identical) was made with the
  research prototypes these modules were derived from.

Sections named `libm:*` depend on libm's transcendental functions.  They are compared only if
the `libm_probe` section shows that the test runs against the same libm as the golden file
(glibc 2.39 on x86-64 at the time of writing); otherwise they are skipped with a note.

Not covered by the oracle (unit tests only): `Box2IMinMax`, `kicore::mirror`,
`words_to_double`, the overflow behaviour of `ki_checked_cast` (deviation D1).

## Deviations

- **D-G1 `cmath::cbrt` is a port of glibc's `__cbrt`** (`sysdeps/ieee754/dbl-64/s_cbrt.c`).
  Rust's `f64::cbrt` does not call the system libm and differs from glibc by one ulp for about
  half of all inputs, which changed the root count of tangent ellipse / circle intersections.
  The port is bit-exact (verified on 220,000 bit patterns) and, being pure Rust, also on wasm.
- **D-G2 `ROUNDRECT::TransformToPolygon` replaces the buffer** with a plain assignment; the C++
  move assignment keeps the target's "hash failed" flag (`ShapePolySet::move_assign` is private
  to `shape_poly_set.rs`), so only that flag of the target can differ.  Geometry is identical
  (oracle).
- **D-G3 `CornerListToPolygon`** of an empty corner list adds an empty outline (C++ reads
  `aCorners[0]`); its `wxCHECK_RET` (no intersection, not reachable with valid input) returns
  from the whole function like the C++.  The out-of-range `( int )` conversions of `double`
  arguments to `VECTOR2I::Resize` and `GetArcToSegmentCount( ..., double aAccuracy, ... )` use
  `f64_to_i32_x86`.
- **D-G4 Collision matrix entry for `SH_ELLIPSE`.**  Adding `ShapeRef::Ellipse` required an arm
  in `shape_collisions::collide_single_shapes`; G6b added a placeholder that G5 has since
  replaced with the ported ellipse pairs.

- **D1 `KiCheckedCast<int64_t, int>` clamps on every platform.** The C++ only clamps when
  `in_type` is `long long int`. On LP64 Linux `int64_t` is `long`, so in the Linux build the
  instantiation used by `BOX2I` (`GetRight()`, `GetBottom()`, `Centre()`, `Normalize()`,
  `inflateAxis`, `Intersects( rect, angle )`) is a plain narrowing conversion that wraps, while
  the Windows and macOS builds clamp as documented ("Will clamp the return value to numerical
  type limits"). The port clamps everywhere. This only matters for boxes whose edges lie
  outside the `int` range, where the Linux result is garbage anyway. The single place to change
  it is `impl KiCheckedCast<i32> for i64` in `src/math/util.rs`.
- **D2 No overflow assertion.** `kimathLogOverflow` (a `wxFAIL_MSG`, debug builds only) and
  `kimathLogDebug` are no-ops, hence `KiROUND` has no `aQuiet` parameter. The debug-only range
  check in the `BOX2` constructor is not reproduced.
- **D3 Signed integer overflow.** Where the C++ would overflow a signed integer (undefined
  behaviour; e.g. `VECTOR2I` arithmetic on coordinates more than 2^31 apart) the port uses the
  plain operators as required by `PORTING.md`: a panic in builds with overflow checks,
  two's-complement wrapping otherwise. Exception: the intersection point of
  `SegmentIntersectsSegment` is accumulated with wrapping adds, because the C++ computes (and
  then discards) an overflowing point for perfectly valid nearly-parallel segments.
  `u32` vector arithmetic always wraps, as unsigned arithmetic does in C++.
- **D4 Conversions with undefined C++ behaviour** (revised in G3).  `Vector2::cast` follows the
  converting constructor (`std::clamp`, then `static_cast`) as the reference build executes it,
  verified by the `x86_casts` oracle section: a value beyond a clamp limit yields that limit
  (g++ converts the constant limit at compile time, with saturation: the maximum even where the
  limit is not representable, `double` 2^63 to `int64_t`, `float` 2^31 to `int`); a value
  inside the limits is converted at run time (`cvttsd2si`), which gives `INT_MIN` /
  `INT64_MIN` for NaN and for a value equal to an unrepresentable limit; unsigned targets get
  0 for NaN.  The `(int)` casts of doubles in `BOX2` (`GetSizeMax`, the corners of
  `Intersects( rect, angle )` for `BOX2D`) use `cmath::ToI32X86` (`INT_MIN` out of range,
  verified for `GetSizeMax`).
- **D5 `rescale<int64_t>`** is the `__int128` implementation used by GCC and Clang builds (the
  MSVC `_div128` path logs and returns 0 on quotient overflow; the generic fallback is not
  ported).
- **D6 `Hash` is derived for `Vector2<T>`** (integer `T`) and implemented for `Seg`, although
  the C++ deletes `std::hash<VECTOR2I>` on purpose. Ported algorithms must keep using ordered
  containers where the C++ does.
- **D7 `std::minmax( { a, b, c } )`** in `trigo.cpp` is reproduced with the comparison sequence
  of libstdc++, so even NaN input gives the reference build's result.
- **D8 Mixed-type vector operators** exist for all combinations of `i32`, `i64`, `f64` vectors
  and for the scalar combinations used by KiCad and its tests; `Vector2<f32>` / `Vector2<u32>`
  only mix with `i32`, `u32`, `f64` (and `i64` for `u32`) scalars. `{integer literal} *
  Vector2L` needs a typed literal (`2i32 * v`), because both `i32` and `i64` scalars are
  accepted.
- **D9 `Box2<T>` is not generic code**: the methods exist for `Box2I`, `Box2L`, `Box2D` only
  (the three instantiations of the C++).
- **D10 Floating point contraction.** The port never fuses `a * b + c` (and uses `mul_add` only
  where the C++ calls `std::fma`). This matches x86-64 builds of KiCad; C++ builds for targets
  with FMA in the baseline (e.g. aarch64 with GCC's default `-ffp-contract=fast`) may differ
  from it - and from the port - in the last bit.
- **D11 `DynamicRTree` moves its data items** instead of copying them through the
  temporary entry arrays of `forcedReinsert` / `splitNode` (C++ `DATATYPE` is a pointer in all
  uses).  The entries are sorted as plain data referring to a side table; since the libstdc++
  sort permutes as a function of the comparison results only, the arrangement is identical.
  `T` needs `PartialEq` for `remove` only.  `iter()` / `overlapping()` / `nearest_neighbors()`
  return references (C++: copies).  The C++ move constructor / assignment is `std::mem::take`.
- **D12 R-tree storage layout and C++ undefined behaviour.** Nodes are stored array-of-structs
  in an index arena instead of SoA slabs; the SSE2 / NEON / scalar overlap masks all compute
  the same closed-interval predicate, as does `overlap_mask`.  `memory_usage()` values are
  approximations of a different layout (they are not used for results).  Where the C++
  overflows `int64_t` (areas / overlaps / distances of boxes spanning most of the `int` range,
  undefined behaviour) the port wraps, which is what the reference build does; the
  `static_cast<int64_t>( double )` of the forced-reinsert distance reproduces x86-64
  `cvttsd2si` (`INT64_MIN` when out of range), verified by the full-`int`-range oracle cases.
- **D13 delaunator inputs with undefined C++ behaviour.** For fewer than two distinct points
  the C++ reads `m_points[INVALID_INDEX]`; the port returns
  `DelaunatorError::NotTriangulation` (the error the C++ throws for collinear input).  A hull
  hash without a hull edge (C++ `assert` + out-of-bounds read; not reachable with finite input)
  is `DelaunatorError::HullCorrupt`.  `get_triangle_area()` of an empty triangulation is 0
  (C++ reads `x[0]` of an empty vector).  `std::llround` of NaN in `hash_key` gives
  `LLONG_MIN` like glibc.  The struct keeps a copy of the input coordinates (C++: a reference).
- **D14 `stdsort` with invalid comparators** (not a strict weak ordering, e.g. NaN keys or
  `<=`): the C++ has undefined behaviour (unguarded scans run out of the range); the port stays
  memory safe, never panics and terminates, but the order is unspecified.  With valid
  comparators every comparison and move is the libstdc++ one.  `middle` / `nth` beyond the
  slice are clamped / ignored.

- **D11 Wrapping where the C++ overflows on valid input.** As for `SegmentIntersectsSegment`
  (D3), three subtractions use two's complement wrapping, because the reference build computes
  (and then mostly discards) an overflowing value for legal input: `SHAPE_ARC::Collide( SEG )`
  subtracts the clamped center of a nearly straight arc (`center - m_start`,
  `aSeg.A - center`, `aSeg.B - center`; covered by `CollideNearlyFlatArcDoesNotOverflow`), and
  `HALF_LINE::Intersect` subtracts the (arbitrarily far) intersection point of nearly parallel
  lines.  Other overflows of degenerate input keep the D3 rule (panic with overflow checks):
  notably `ConstructFromStartEndAngle` with an angle very close to a multiple of 360 degrees,
  the tangent-arc constructor for nearly parallel segments and `update_values()` for arcs with
  a radius near 1e9 nm centred near the coordinate limits.
- **D12 Conversions of out-of-range doubles to `int`** (revised in G3; UB in C++) use
  `cmath::f64_to_i32_x86` like the reference build (`INT_MIN`): `SHAPE::Distance()` of
  `ECOORD_MAX` (e.g. a line chain without segments; verified by the `x86_casts` oracle
  section), the `CIRCLE( center, radius )` / `SHAPE_CIRCLE` / `GetArcToSegmentCount( radius )` /
  `VECTOR2I( radius, 0 )` / `SEG::Square( radius - clearance )` conversions of `SHAPE_ARC` for
  radii beyond `INT_MAX`, `ConvertToPolyline`'s `seg360`, and `ConstructFromStartEndAngle`'s
  double width (verified).  The radius sites are not reached by the oracles (inputs with such
  radii overflow elsewhere first, see D11).
- **D13 `DISABLE_ARC_RADIUS_CORRECTION`** sets a thread-local flag (`DisableArcRadiusCorrection`
  guard) instead of a process-wide static; the port is single threaded, so this is the same.
- **D14 `const` methods writing `mutable` members** take `&mut self`
  (`ShapeLineChain::generate_bbox_cache`), so that all shapes are `Send + Sync`.
- **D15 Poly-set triangles** (`Tri`) store copies of their three vertices instead of a pointer to
  the parent triangulation (kept in G4b: every `TriangulatedPolygon` mutator - `add_triangle`,
  `set_triangles`, `set_vertices`, `move_`, `refine` - keeps the copies in sync; code writing the
  public fields directly must do the same).  `TRI` has no `Clone()` in C++ (it asserts);
  `Tri::clone_shape` returns a copy.
- **D16 `wxASSERT` / `wxCHECK` on input**: the tangent-arc constructor's "segments do not
  intersect" assertion and the "MTV not implemented" assertions of the collision matrix are not
  reproduced (release behaviour); `wxCHECK`s keep their early return.

- **D-H1 C++ traps and undefined behaviour in G6a code.** `TRANSFORM::InverseTransform` of a
  singular matrix (integer division by zero) returns the matrix unchanged; `VECTOR3<int>`
  division by zero leaves the components unchanged; `int` arithmetic that could overflow
  (`TRANSFORM::TransformCoordinate`, `distribute`, convex hull differences, `ELLIPSE<int>::Mirror`,
  `BEZIER<int>` differences) wraps as on x86.  Out-of-range `double` to `int` casts (`GetPoly`'s
  segment count, `TransformEllipseToBeziers`' Bezier count, `ELLIPSE<int>` minor radius, grid
  indices) use `cmath::f64_to_i32_x86`.  The `VECTOR2<int>( VECTOR2D )` conversion of NaN follows
  D4.  Inputs that make `BEZIER_POLY`'s `recursiveSegmentation` loop forever in C++ (a curve whose
  halves never become flat) do the same in the port.
- **D-H2 `MMH3_HASH::add( int32_t )` after an unaligned `addData`.** The C++ then hashes a block
  byte that no call wrote (its buffer is uninitialised); the port's buffer starts zeroed, so it
  hashes 0 or a stale byte.  Aligned mixing, which is what KiCad does, is bit-exact.
- **D-H3 `MD5_HASH::Format`** maps nibbles 10..15 to `B`..`G` (C++ quirk, reproduced).
- **D-H4 `std::hash` is libstdc++'s with a 64-bit `size_t` on every target** (also wasm32), so
  hashes computed by the port are those of the reference platform.

- **D-S1 `SHAPE_LINE_CHAIN::Insert( vertex, arc )` for `vertex > 0`.** The C++ scans the shape
  array backwards up to `m_shapes.rend() + aVertex`, i.e. `aVertex` elements before its
  beginning (out-of-bounds reads; the result is garbage and the following `m_arcs.insert` then
  writes out of bounds).  The port scans the whole array (exactly the C++ behaviour for
  `vertex == 0`, the only call in KiCad's tests).
- **D-S2 `NearestPoint( p, false )`** on the closing arc segment of a closed chain, with `p`
  nearer to its end (point 0): the C++ reads `m_shapes[PointCount()]` (past the end); the port
  uses point 0, which the code then returns (it is a shared arc point).
- **D-S3 `Replace( start, end, chain )` of a closed chain** whose first and last points become
  equal by the removal: the C++ inserts past the end of the arrays (crash); the port appends.
- **D-S4 Index arguments the C++ uses out of bounds.** `Remove( start, end )` with a start index
  that is still negative after adding `PointCount()`, and `convertArc` with an index that is still
  negative after adding `ArcCount()`, return without change.  Other out-of-range indices of
  methods without a `wxCHECK` (e.g. `ArcIndex`, `SetPoint`, `CPoint`) panic, where the C++ reads
  or writes out of bounds.
- **D-S5 Clipper2 constructor.** An arc index outside the arc buffer (`std::vector::at` throws
  in C++) and a z value of a point that `Append()` dropped as a duplicate (the C++ then writes
  past the shape array) leave the point a plain point.
- **D-S6 `Parse` extractions** (`StringStream`): a failed extraction yields 0 and fails all
  further ones; the C++ leaves the variables of later extractions untouched (uninitialised
  `int` / `ssize_t` / `double`).  Results are identical as long as the text holds the counted
  values (oracle).
- **D-S7 Signed overflow in arc editing.** Splitting arcs (`Remove`, `Replace`, `Insert`,
  `Split`, `splitArc`) can create sub-arcs so short that their recomputed centre is clamped
  near the `int` limits; editing such an arc again overflows `int` in
  `ConstructFromStartEndCenter` / `RotatePoint` (UB, wraps on x86).  The port follows D3
  (panic with overflow checks, wrapping otherwise; the wrapped results are not guaranteed to
  equal the C++ ones because `KiROUND` clamps differ).  4 of 70,000 random edit cases (skip list
  of the oracle).
- **D-S8 `Length()`** adds the `double` arc lengths to a `long long` as C++ does (conversion to
  `double` and back, with `f64_to_i64_x86`).

- **D-P1 C++ exceptions.** `CVertex( int )`, `RemoveVertex( int )`, `SetVertex( int, ... )` and
  `IterateFromVertexWithHoles()` throw `std::out_of_range` for a vertex that does not exist;
  the port panics with the same message (programmer error, not input).  `zValues.at()` in the
  boolean Z callback cannot throw for indices produced by the port itself; an index outside the
  buffer is treated as "no arc".
- **D-P2 Undefined behaviour of the C++ on degenerate sets.** `RemoveNullSegments()` of an
  empty set returns 0 (C++ dereferences the first vertex); `NormalizeAreaOutlines()` of an empty
  set returns 0 (C++ `Polygon( 0 )`); `fractureSingle()` leaves a polygon with an empty contour
  unchanged (C++ reads before the point array); `GetNeighbourIndexes()` reports -1 where the C++
  leaves an output uninitialised (contours with fewer than 3 points).  Empty contours reached by
  an iterator still panic in `c_point` (C++ reads out of bounds).  Clipper2 errors (C++: `bool`
  result ignored) give an empty result.
- **D-P3 Iterators.** `ITERATOR` and `CONST_ITERATOR` (and the segment pair) are one Rust type
  each, borrowing the set immutably (the C++ `ITERATOR` only gives read access too).  The
  `operator bool()` quirk is kept: a vertex iterator whose last polygon is not the last polygon
  of the set ends as soon as it reaches its last polygon.
- **D-P4 Copies.** `Clone` follows the copy constructor: the triangulation and hash are copied
  only while the triangulation is up to date, otherwise the copy's hash is invalid (so a stale
  cached hash is recomputed in copies, verified by `libm:sps_edit`).  `RebuildHolesFromContours`
  reproduces the move assignment (the failed-hash state is kept).
- **D-P5 `const` methods writing `mutable` state** take `&mut self`: `BuildBBoxCaches()`;
  `CacheTriangulation()` (G4b).  `Area()` (non-`const` in C++) takes `&self`.
- **D-P6 Release behaviour of assertions.** `wxFAIL_MSG` for booleans on curved polygons,
  `wxASSERT`s of `RebuildHolesFromContours` / `SquaredDistance( aOutlineOnly )` and
  `assert( aPoly.size() == 1 )` of `unfractureSingle` (compiled out with `NDEBUG`: a polygon
  whose fracture failed keeps its holes) are not reproduced; `wxLogWarning`s are dropped.
- **D-P7 Configuration.** The `ADVANCED_CFG` defaults are built in: cache-friendly fracture and
  the fracture edge index enabled (the slow fracture variant is not ported).  The edge index is
  output-neutral by construction and is used under the same conditions as in C++.
- **D-P8 No-ops of the C++ left out.** `Clipper2Lib::SimplifyPaths()` in `inflate2` /
  `inflateLine2` (its result is discarded), the `newIntersectPoints` map of the Z callback
  (never read), the thread-local arc tolerance table (a cache of the same value), mutexes and
  atomics (the port is sequential).
- **D-P9 Overflow.** Arithmetic follows D3 (plain operators); `std::abs( int )` of the offset
  amount and the negation in `Deflate` wrap like x86 (`INT_MIN`).  Double to `int` conversions
  that the C++ does implicitly (`*aActual = sqrt( dist )`, the fillet radius passed to
  `GetArcToSegmentCount`, `SHAPE_POLY_SET( BOX2D )`, `CollideVertex`'s squared norm) use
  `cmath::f64_to_i32_x86` / `f64_to_i64_x86`.

- **D-C1 Collision matrix (G5).** The "MTV not implemented" `wxASSERT_MSG`s are not reproduced
  (release behaviour: the MTV is left as passed in, then negated by `CollCaseReversed` exactly
  as in C++).  `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )` sorts its segments with
  the libstdc++-exact `std_sort` (ties of equal start points keep the reference order) and uses
  the ported `SEGMENT_INDEX` (same visit order).  `CIRCLE::IntersectLine` adds the chord
  vector to the projected centre with `int` wrap-around, which the reference build does for the
  clamped centre of a nearly straight arc (`NearestPoints_ArcToSegment`; same rule as D11).
- **D-C2 `SHAPE_INDEX` storage.** The C++ class stores its items in a `COW_RTREE` (copy-on-write
  dynamic R*-tree with shared nodes, not ported); the port uses `DynamicRTree` with the same
  node size, so `clone_index()` (C++ `Clone()`) is a deep copy.  Query results are the same set;
  their visit order is the one of `DynamicRTree`, which was not compared with `COW_RTREE` (the
  only user, the interactive router, is out of scope).  `SHAPE_INDEX_LIST` caches the item's
  `SHAPE*` in each entry; the port asks the item for its shape when needed.

- **D-T1 Vertex arena (G4b).** `VERTEX` objects live in `VertexSet::vertices` (creation order,
  as the C++ `std::deque`, which `intersectsPolygon` relies on) and link through indices; a null
  dereference of the C++ panics.  `void* m_userData` is a `usize` (0 = `nullptr`).  The vertex
  set keeps its state across `TesselatePolygon` calls exactly where the C++ does not clear it
  (early returns), so reused triangulators behave the same.  Tie order of `zSort`,
  `buildReflexIndex` and the hole sort is libstdc++'s (`stdsort`); `subdividePolygon`'s
  `std::set` (equal lengths collapse) is reproduced.  Out-of-range `double` to `int` conversions
  use `f64_to_i32_x86`; `int` arithmetic that overflows on valid input in
  `collectScanlineHits` / `splitPolygonBalanced` (outlines wider than about 143 mm) wraps like
  x86 (verified by `tri_partition`).
- **D-T2 Task submitter.** `TASK_SUBMITTER` is a flag (`a_has_submitter`): with it, partition
  leaves are triangulated one after the other in leaf order, which is the C++ result whatever
  the threads do; without it the leaves are re-queued as outlines as in C++ (a different but
  equally deterministic result).  No threads, mutexes or atomics.
- **D-T3 `Collide( const SHAPE* )` on `&self`.** The C++ caches the triangulation through a
  `const_cast`.  `collide_shape(&self)` (also `Shape::collide`) uses the cached triangulation
  when it is up to date (building a temporary index above `RTREE_THRESHOLD` triangles, as the
  C++ would), the stale failed one when the failed hash matches, and otherwise triangulates a
  copy; `collide_shape_mut` caches like the C++.  Results are identical (asserted on every
  `tri_collide` probe); callers with many queries should cache first
  (`cache_triangulation_with( false, false, true )`) or use `collide_shape_mut`.

## Known gaps

- **GAP-1 libm on targets without glibc (notably wasm32).** Results that pass through `sin`,
  `cos`, `tan`, `atan2`, `asin`, `acos`, `atan`, `hypot` or `pow` are bit-identical to the C++
  reference on Linux/glibc (the reference platform). On `wasm32-unknown-unknown` Rust uses its
  bundled pure-Rust libm, which differs from glibc by one ulp in roughly 1-18 % of calls
  depending on the function (measured: `atan` 1.4 %, `sin`/`cos` 3 %, `hypot` 14 %, `atan2`
  18 %). Affected: `EdaAngle::{sin, cos, tan, arccos, arcsin, arctan, arctan2, round}` and
  `from_vector` for directions other than the axes and diagonals; `rotate_point*` /
  `get_rotated*` for angles that are not multiples of 45 degrees; `Vector2D::euclidean_norm`,
  `Vector2::distance` (not the integer `euclidean_norm`, which is exact); `calc_arc_center*`,
  `is_straight_arc`, `calc_arc_mid`; `Seg::angle`; `Box2::intersects_rotated` /
  `bounding_box_rotated` for such angles; `vector_utils::get_nearest_endpoint`;
  `Box2IMinMax::diameter`. An integer result changes only if the one-ulp difference straddles a
  rounding boundary: estimated at the order of 1e-9 per rounded coordinate at board scale (a
  measurement of 20 million random rotations with both libms found no differing coordinate),
  so wasm output can in very rare cases differ from the reference by 1 nm. Making wasm bit-exact
  needs a port of glibc's implementations of these functions; all calls already go through
  `src/cmath.rs`, which is the single place to plug it in.
- **GAP-2** (closed in G2) `BOX2I_MINMAX( const SHAPE_ARC& )` is `Box2IMinMax::from_arc`.
- **GAP-3** Test helpers that need polygon sets (`FilletPolySet`,
  `IsPolySetValid`, `ChainsAreCyclicallyEqual`) wait for those ports (the hash and
  `geometry_predicates.h` tests were ported in G6a).
- **GAP-H1** (closed in G6b) `SHAPE_ELLIPSE` and `nearest`; `matrix3x3.h` stays skipped (no user
  in scope).
- **GAP-4** (closed in G5) the collision matrix and `SHAPE::NearestPoints`: see the map, D-C1
  and the `coll_oracle`.
- **GAP-5** (closed in G6b) `TransformToPolygon` of circles, segments, arcs and rounded
  rectangles (and so `ShapeRect::outline()` of rounded rectangles) go through
  `convert_basic_shapes_to_polygon` / `Roundrect`.
- **GAP-6** (closed in G4a) the `SHAPE_POLY_SET` skeleton; what is left is GAP-P1.
- **GAP-S1** (closed in G4a) `SHAPE_LINE_CHAIN::OffsetLine` is `ShapeLineChain::offset_line`
  (verified by `libm:sps_inflate`).
- **GAP-P1** (closed in G4b) the triangulation: see the map, D-T1..D-T3 and the `tri_oracle`.
- **GAP-P2** (closed in G5) poly-set pairs go through `ShapePolySet::collide_shape` like the
  C++ `collideSingleShapes`.
- **GAP-P3** (closed in G5) the corpus tests of `test_shape_line_chain_collision.cpp` are
  ported.
- **GAP-P4** (closed in G4b) `Seg::intersects_line` converts `aSlope * 1000` with
  `f64_to_i64_x86` (`INT64_MIN` for infinite / NaN / huge slopes, like the reference build) and
  computes the then overflowing cross product with wrapping arithmetic; verified by
  `seg_intersects_line`.
- **GAP-4b `COW_RTREE`** (`dynamic_rtree_cow.h`, 834 lines) is not ported: its only user is
  `SHAPE_INDEX` in the interactive router, which is out of scope (the port's `ShapeIndex` uses
  `DynamicRTree`, D-C2).  Its 6 tests (suite `CowRTree` of `test_dynamic_rtree.cpp`) are not
  ported either.
- **GAP-T5 `polygon_triangulation.h`** (closed in G4b; listed as GAP-5 in the G4b summary
  above, a different gap from the `TransformToPolygon` GAP-5 closed in G6b).
- **GAP-6 nanoflann subset.** Only what KiCad uses is ported: the static single-index kd-tree
  with the L2 metric (`L2_Simple_Adaptor`), `KNNResultSet` (without `NANOFLANN_FIRST_MATCH`),
  `RadiusResultSet`, `knnSearch`, `radiusSearch`, `findNeighbors` with `SearchParameters`.
  Not ported: dynamic indices, `rknnSearch`, other metrics (L1, SO2, SO3), `kdtree_get_bbox`
  support, save/load, the pool allocator and the multi-threaded build (which builds the same
  tree).
- **GAP-7 Tie order is libstdc++'s.** By project decision (PORTING.md section 9) every
  `std::sort` tie order, and therefore r-tree visit orders and delaunator outputs, match KiCad
  built with g++/libstdc++; KiCad builds with libc++ or MSVC differ from it (and from each
  other) in those cases.

## CLEAN-1 (consolidation)

- `stdsort::StdSet` / `stdsort::std_set_order`: the libstdc++ red-black tree insertion of
  `std::set` (`_M_insert_unique`, the hinted `_M_insert_unique_( end(), v )` of the range
  constructor, `_Rb_tree_insert_and_rebalance`), moved here from `kicad-pcb`'s writer, for
  `std::set<T, CMP>` with comparators that are not strict weak orderings
  (`EDA_SHAPE::Compare`). Verified against g++ 13 / libstdc++ by `tests/oracle_stdset.rs`
  (6,000 random sequences, hinted and plain insertion, transitive / epsilon / tournament /
  cyclic / neighbour-only comparators; driver `tests/oracle/stdset_oracle.cpp`): all match.
  Comparators must be irreflexive and asymmetric (otherwise libstdc++ itself has undefined
  behaviour).
- `mirror_val` / `mirror` / `mirror_point` (`MIRRORVAL` / `MIRROR`) wrap on `int` overflow
  like the reference build (trait `MirrorValue`: wrapping for the integer types, the plain
  expression for `f32` / `f64`); `ShapeLineChain::mirror` and `ShapeArc::mirror`
  (`-p + 2 * ref`) wrap as well. Verified by `tests/oracle_mirror.rs` against `MIRROR()` and
  libkimath's `SHAPE_LINE_CHAIN::Mirror` / `SHAPE_ARC::Mirror` (4,000 cases with coordinates
  whose mirror image leaves the `int` range; driver `tests/oracle/mirror_oracle.cpp`): all
  match. Known gap: `ShapeArc::update_values` (the arc constructor) still panics on overflow
  for points near the `int` limits (C++: undefined behaviour), so the arc cases of that oracle
  use in-range arcs.
- Docs: `docs/design/geom-shapes.md` no longer describes `unsupported_g5` as the route of
  not-yet-ported pairs; the second GAP-5 entry (triangulation) is renamed GAP-T5.
