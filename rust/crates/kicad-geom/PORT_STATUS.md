# kicad-geom port status

Port of KiCad's `libs/kimath` (integer 2D math, shapes, polygon sets, collisions).

State after **work package G1 - math foundation (kimath part 1)**: utilities and rounding,
vectors, boxes, angles, trigonometry helpers, segments and vector utilities are ported, tested
with the ported C++ unit tests and verified bit for bit against the original C++ code with a
differential oracle (about one million pseudo-random cases, see "Verification").  Shapes
(`SHAPE_*`, `circle.h`, ...) are the next work package and are not started.

Everything in this crate builds for `wasm32-unknown-unknown`, has no dependencies and no
`unsafe`.

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
| `libs/kimath/include/math/box2_minmax.h` | `src/math/box2_minmax.rs` (`Box2IMinMax`) | ported except the `SHAPE_ARC` constructor (next package) |
| `libs/kimath/include/geometry/eda_angle.h`, `src/geometry/eda_angle.cpp` | `src/geometry/eda_angle.rs` | ported |
| - `EDA_ANGLE`, `EDA_ANGLE_T`, `ANGLE_*`, `FULL_CIRCLE` | `EdaAngle`, `EdaAngleT` (+ consts `DEGREES_T`, `RADIANS_T`, `TENTHS_OF_A_DEGREE_T`), `ANGLE_*`, `FULL_CIRCLE` | ported; all methods and operators, `std::abs` is `EdaAngle::abs` |
| - `EDA_ORIENTATION` | `EdaOrientation` | ported |
| `libs/kimath/include/trigo.h`, `src/trigo.cpp` | `src/trigo.rs` | ported (everything in the files; see the overload table in the module docs) |
| `libs/kimath/include/geometry/seg.h`, `src/geometry/seg.cpp` | `src/geometry/seg.rs` (`Seg`, `OptVector2I`, `Ecoord`) | ported; all methods (this version of `SEG` has no methods taking shapes) |
| `libs/kimath/include/geometry/vector_utils.h`, `src/geometry/vector_utils.cpp` | `src/geometry/vector_utils.rs` | ported |
| `include/core/mirror.h` (`FLIP_DIRECTION`, `MIRRORVAL`, `MIRROR`, `MIRROR_AXIS`) | `src/kicore/mirror.rs` (`FlipDirection`, `mirror_val`, `mirror`, `mirror_point`, `mirror_axis`) | ported |
| `libs/core/include/core/kicad_algo.h` | - | skipped: nothing in it is used by the files of this package (`alg::pair_contains` / `alg::run_on_pair` are used by shape code) |
| `libs/kimath/include/math/vector2wx.h` | - | skipped: wxWidgets glue |
| `libs/kimath/include/math/matrix3x3.h`, `vector3.h` | - | not part of this package |
| all shape code (`geometry/shape*.h`, `circle.h`, `half_line.h`, `line.h`, `intersection.h`, `geometry_predicates.h`, ...), `convert_basic_shapes_to_polygon`, `bezier_curves`, `transform`, hashes | - | not started (next packages) |

Support modules without a C++ counterpart:

| Rust | Purpose |
|---|---|
| `src/cmath.rs` | The transcendental libm functions the C++ calls (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `hypot`, `hypotf`, `pow`, ...). Every such call in the port goes through this module so the libm choice is made in one place (see GAP-1). |
| `src/ostream.rs` | `format_g` / `format_double`: the default `std::ostream << double` formatting (`%g`), needed by `Format()` and the stream operators. |
| `src/test_utils.rs` | Ports of the shared C++ QA helpers (below) and Mersenne Twister / libstdc++ distribution replicas for porting randomised tests. |

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

### Out-parameters

| C++ | Rust |
|---|---|
| `SEG::NearestPoints( seg, aPtA, aPtB, aDistSq )` (always returns true) | `nearest_points( &seg ) -> ( pt_a, pt_b, dist_sq )` |
| `SEG::IntersectsLine( slope, offset, aIntersection )` | `intersects_line( slope, offset ) -> Option<Vector2I>` |
| `SEG::Collide( seg, clearance, int* aActual )` | `collide_actual( &seg, clearance ) -> ( bool, i32 )` (every C++ path sets `*aActual`) |
| `SEG::CanonicalCoefs( qA, qB, qC )` | `canonical_coefs() -> ( qa, qb, qc )` |
| `SegmentIntersectsSegment( ..., VECTOR2I* aIntersectionPoint )` | `segment_intersects_segment_with_point( ... ) -> ( bool, Option<Vector2I> )`; the point is `Some` whenever the lines are not parallel, exactly when the C++ writes to the pointer |
| `RotatePoint( int* pX, int* pY, ... )` | `rotate_point_xy( &mut x, &mut y, ... )` (in place, like the C++) |

## Ported C++ tests

| C++ test file | Rust | Cases |
|---|---|---|
| `qa/tests/libs/kimath/math/test_util.cpp` | `tests/test_util.rs` | 2 of 2 (+ 7 extra tests of `KiROUND` / `rescale` edge cases) |
| `qa/tests/libs/kimath/math/test_vector2.cpp` | `tests/test_vector2.rs` | 7 of 7 (including `VECTOR2<float>` / `VECTOR2<unsigned>` casting) |
| `qa/tests/libs/kimath/math/test_box2.cpp` | `tests/test_box2.rs` | 12 of 12 |
| `qa/tests/libs/kimath/geometry/test_segment.cpp` | `tests/test_segment.rs` | 30 of 30 |
| `qa/tests/libs/kimath/geometry/test_eda_angle.cpp` | `tests/test_eda_angle.rs` | 8 of 8 |
| `qa/tests/libs/kimath/geometry/test_vector_utils.cpp` | `tests/test_vector_utils.rs` | 4 of 4 |
| `qa/tests/libs/kimath/test_kimath.cpp` | `tests/test_kimath.rs` | 3 of 5: `TestCalcArcCenter3Pts`, `TestInterceptsPositiveX`, `TestInterceptsNegativeX`. `TestHash128` and `TestMMH3Hash` test `hash_128.h` / `mmh3_hash.h` (not in this package). |
| `qa/tests/libs/kimath/math/test_wide_int.cpp` | `tests/test_wide_int.rs` | 5 of 7. `OrientationSignBeyondDoublePrecision` and `InCircleDelaunayLegalExact` test `geometry_predicates.h` (next package). |
| `qa/qa_utils/include/qa_utils/numeric.h` | `test_utils::{is_within, is_within_wrapped, is_within_bounds, is_within_and_below, is_within_and_above}` | all |
| `qa/qa_utils/include/qa_utils/geometry/geometry.h` | `test_utils::{is_vec_within_tol, is_box_within_tol}` | `ChainsAreCyclicallyEqual` and the printers need shapes: later |
| `qa/qa_utils/geometry/seg_construction.cpp` | `test_utils::{build_h_seg, build_v_seg}` | all |
| `qa/tests/libs/kimath/geometry/geom_test_utils.h/.cpp` | `test_utils::{Quadrant, is_in_quadrant, segment_completely_in_quadrant, segment_ends_in_quadrant, segment_completely_within_radius, is_point_at_distance, are_points_near_circle, are_perpendicular, segments_have_same_end_points, triangle_min_angle_deg}` | `FilletPolySet`, `IsOutlineValid`, `IsPolySetValid`, `toString( POINT_TYPE )`, `operator<<( TYPED_POINT2I )` need shapes / `point_types.h`: later |
| `qa/tests/libs/kimath/geometry/fixtures_geometry.h` | `test_utils::CommonTestData` (`unique_points`, `holey_points`, `holey_segments`) | the polygon sets are built by the shapes package |

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

Sections named `libm:*` depend on libm's transcendental functions.  They are compared only if
the `libm_probe` section shows that the test runs against the same libm as the golden file
(glibc 2.39 on x86-64 at the time of writing); otherwise they are skipped with a note.

Not covered by the oracle (unit tests only): `Box2IMinMax`, `kicore::mirror`,
`words_to_double`, the overflow behaviour of `ki_checked_cast` (deviation D1).

## Deviations

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
- **D4 Conversions with undefined C++ behaviour.** `Vector2::cast` follows the converting
  constructor (clamp, then truncate). For NaN, and where the clamp bound itself is not
  representable in the target type (`double` 2^63 to `int64_t`, `float` 2^31 to `int`), the C++
  result is undefined (x86 yields `INT_MIN`); the port yields 0 for NaN and saturates. The same
  applies to the few `(int)` casts of doubles in `BOX2` (`GetSizeMax`, `Intersects( rect,
  angle )` for `BOX2D`).
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
- **GAP-2** `BOX2I_MINMAX( const SHAPE_ARC& )` waits for `SHAPE_ARC`.
- **GAP-3** Test helpers that need shapes (see the table above) and the tests of `hash_128.h`,
  `mmh3_hash.h`, `geometry_predicates.h` wait for those ports.
