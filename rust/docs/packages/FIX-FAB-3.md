# FIX-FAB-3: overflow panics on boards beyond 1.07 m (kicad-geom, kicad-pcb)

Crate: kicad-geom (`src/geometry/shape_collisions.rs`), kicad-pcb (`src/pcb_shape.rs`). No
change in kicad-fab.

Origin: findings X-1, X-2 of `rust/docs/design/review-fab.md` (package REV-FAB). Found with a
fabrication test board, but the defect is in the load path: neither export can start.

## 1. `set( &mut out, value )` evaluates `value` when nobody asked (X-1)

`shape_collisions.cpp` writes its optional results as

```cpp
if( aLocation )
    *aLocation = ( aA.GetCenter() + aB.GetCenter() ) / 2;
```

The port uses a helper `fn set<T>( a_out: &mut Option<&mut T>, v: T )` and passes the
expression as `v`, so it is computed on every call. In `collide_circle_circle` (L61-92) that is
`a_a.get_center() + a_b.get_center()` (L81), `(dist_sq as f64).sqrt() as i32 - r - r` (L79) and
the `resize` argument (L85). For two circles whose centres are both above 2^30 nm (1073.7 mm)
the addition overflows `i32`. The C++ does not execute it when `aLocation` is null (the
connectivity search, `CN_VISITOR`), and when it does execute it the reference build wraps.

Effect: a board with two touching vias / round pads right of or below 1073.7 mm (an A0 sheet
is 1189 mm wide) panics in `ConnectivityData::build` during the board load of every `kicad-rs`
command in dev / test builds ("attempt to add with overflow"). The release build and the wasm
module have no overflow checks and produce output identical to the reference on such a board
(`rev_far`: 23 Gerber / job files and the drill file compared).

Work:

1. Make the optional results lazy: `if let Some(o) = a_actual.as_deref_mut() { *o = ...; }`
   (or a `set_with( &mut out, || value )` helper) in every function of the file that uses
   `set`; the expressions then run exactly when the C++ runs them.
2. Where the C++ expression itself is an `int` addition / subtraction of coordinates that the
   reference build wraps (`a + b` of two centres, `end - start`), use `wrapping_add` /
   `wrapping_sub` on the components, with the C++ line in a comment (PORTING.md section 3;
   `kicad-plot` `user_to_device_coordinates_base` is the model). Go through the whole file for
   the pattern `( p + q ) / 2` and `p - q` on `Vector2I`.
3. Extend the kicad-geom collision oracle with centres at +/-1.2e9, +/-2.0e9 for every
   shape pair that reports a location, with and without the out-parameters.

## 2. `PcbShape::normalize_env` (X-2)

`(gr_rect (start -510 -310) (end 2010 2010))` (2520 mm wide, more than 2^31 nm): the parser
panics with "attempt to subtract with overflow" in `PcbShape::normalize_env` (dev build); the
reference loads, plots and saves the board (`PCB_SHAPE::Normalize`: `end - start` wraps). Such a
board is not valid, but a library crate must not panic on input (PORTING.md section 4): use the
wrapping subtraction the reference performs, or widen if the following code allows it without
changing a result for valid boards, and add the rectangle to the shape oracle.

## Verification

* `REV_FAR=1 python3 rust/docs/design/review-fab/gen/gen_extreme.py > rev_far.kicad_pcb`
  (coordinates up to 1.6 m), canonicalised with `kicad-cli pcb upgrade --force`; `tools/fabcmp.py gerbers` and `drill` with the **dev** build: identical
  (today: exit code 2, "internal error").
* `cargo test -p kicad-geom -p kicad-pcb -p kicad-connectivity` green, the geometry oracle
  bit-exact including the new cases.
