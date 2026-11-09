# Porting rules

This workspace (`<kicad>/rust`) is a Rust port of selected core features of KiCad:
loading/saving `.kicad_pcb` boards, connectivity/ratsnest, design rule checking (DRC),
zone filling, and fabrication output (Gerber, Excellon drill). It must build for
`wasm32-unknown-unknown` as well as natively. The C++ original is in the parent directory and
is the **specification**: when in doubt, do what the C++ does.

Everyone (human or agent) working in this workspace follows the rules below.

## 1. Fidelity

- This is a *port*, not a re-imagining. Port the algorithm the C++ uses, statement by statement
  where practical, so that results are identical (same violations, same coordinates, same
  output bytes). Do not substitute a "simpler" or "more idiomatic" algorithm when it could change
  results in any corner case (rounding, tie-breaking, iteration order, tolerances/epsilons).
- Every Rust source file starts with the licence header below and a `//!` module comment naming
  the C++ file(s) it ports, e.g. ``//! Port of `libs/kimath/src/geometry/seg.cpp` and `seg.h`.``
- Every non-trivial function has a doc comment naming its C++ origin, e.g.
  ``/// Port of `SHAPE_LINE_CHAIN::Simplify2`.`` This is how reviewers map Rust back to C++.
- Carry over the C++ comments that explain *why* (tolerances, bug references, ordering
  constraints). Issue links such as `https://gitlab.com/kicad/code/kicad/-/issues/NNNN` are
  valuable; keep them.
- Anything you intentionally do differently, leave out, or stub goes into the crate's
  `PORT_STATUS.md` (see section 8). Never leave a silent `todo!()`/`unimplemented!()` on a code
  path reachable from supported features; if something is out of scope return a clear error or
  document the fallback in `PORT_STATUS.md`.

## 2. Naming

Mechanical mapping so that anyone can grep from one side to the other:

| C++ | Rust |
|---|---|
| `SHAPE_LINE_CHAIN`, `PCB_TRACK`, `VECTOR2I` | `ShapeLineChain`, `PcbTrack`, `Vector2I` (UpperCamelCase of the same words) |
| `BOX2I`, `VECTOR2D`, `VECTOR2L`, `SEG` | `Box2I`, `Vector2D`, `Vector2L`, `Seg` |
| method `GetBoundingBox()` | `bounding_box()` (snake_case, leading `Get` dropped) |
| method `SetWidth()` / `IsClosed()` / `HasHoles()` | `set_width()` / `is_closed()` / `has_holes()` |
| method `Collide()` / `NearestPoint()` | `collide()` / `nearest_point()` |
| a getter whose snake_case name is a keyword (`GetType()`) | append underscore: `type_()` |
| member `m_Width` / `m_width` | field `width` |
| enum `PCB_LAYER_ID::F_Cu`, `SH_LINE_CHAIN` | `PcbLayerId::FCu`, `ShapeType::LineChain` (drop the redundant prefix, keep the word) |
| free function `TransformCircleToPolygon` | `transform_circle_to_polygon` |
| C++ overloads | one Rust fn per overload with a descriptive suffix; mention the overload in the doc comment |
| out-parameters (`VECTOR2I* aLocation`) | return values (`Option<..>`, tuples or small result structs); keep the same information |

## 3. Numbers

- Board coordinates are **integers in nanometres** (`pcbIUScale`: 1 mm = 1 000 000 IU), stored
  as `i32` exactly like C++ `int`. `Vector2I` is `Vector2<i32>`; where C++ uses
  `VECTOR2I::extended_type` / `int64_t` / `ecoord` use `i64`; where it uses `double` use `f64`.
  Use exactly the same integer widths and the same int/double conversion points as the C++.
- Rounding: use the ported `ki_round` (C++ `KiROUND`, round-half-away-from-zero with clamping)
  wherever the C++ uses `KiROUND`; use truncation where C++ casts with `(int)`; never "improve".
- C++ signed overflow is UB and the C++ code avoids it by widening; do the same widening. Dev and
  test builds have overflow checks enabled on purpose. If the C++ really relies on wrapping
  (hashes), use `wrapping_*` explicitly.
- Angles: port `EDA_ANGLE` semantics exactly (degrees stored as `f64`, tenths-of-degree legacy
  constructors, normalisation ranges).
- Floating point: keep the same expression structure/order as the C++ (no algebraic
  rearrangement, no `mul_add`), so results match bit for bit. For formatting numbers into files
  use the ported KiCad helpers (`FormatDouble2Str`, `EDA_UNIT_UTILS::FormatInternalUnits`,
  `FormatAngle`, ...), never ad-hoc `format!("{:.6}")`.
- Determinism: where C++ iterates a `std::map`/`std::set` use `BTreeMap`/`BTreeSet` (or sort) so
  the order matches; where C++ iterates an unordered container and the order can affect output,
  note it in `PORT_STATUS.md` and pick a deterministic order. No `HashMap` iteration may
  influence results. No randomness, no wall-clock time (timestamps are passed in by the caller).

## 4. Rust style

- Edition 2024, `#![forbid(unsafe_code)]` (workspace lint). No `unsafe`.
- Library crates never panic on bad *input* (malformed files, degenerate geometry): return
  `Result` with a descriptive error type (keep KiCad's error texts; parsers report line/column
  like `PARSE_ERROR`). `debug_assert!`/`assert!` only where C++ has `wxASSERT`/`wxCHECK` on
  programmer errors (and for `wxCHECK( cond, retval )` reproduce the early return).
- Inheritance hierarchies become enums or traits as decided per crate in `rust/docs/design/`.
  Prefer plain data + enums over `Rc<RefCell<..>>` graphs. Raw-pointer parent/child links
  become indices/ids or borrowed context parameters.
- Keep modules roughly one-to-one with C++ files so the mapping stays obvious.
- `cargo fmt` formatting; `cargo clippy` must be warning-free with the workspace lint config.
- Public API gets doc comments. No emojis anywhere.

## 5. WebAssembly

Everything except `kicad-cli` must compile for `wasm32-unknown-unknown`:

- No filesystem, environment, process, thread, clock or locale access in library crates. I/O is
  done on in-memory `&str`/`&[u8]`/`String`/`Vec<u8>`; the CLI and the wasm bindings do the
  actual reading/writing. Things C++ reads from the environment (date, user name, config dirs)
  become explicit parameters with deterministic defaults.
- No C/C++ dependencies (pure-Rust crates only). Check every new dependency builds for
  `wasm32-unknown-unknown` and is GPL-3.0 compatible (MIT/Apache-2.0/BSD/Zlib/BSL/MPL ok).
- Multi-threading in C++ (thread pools in DRC, zone fill, connectivity) is ported as
  deterministic sequential code. Optional `rayon` parallelism may be added later behind a
  non-default `parallel` feature, never required.
- Dependency policy: only crates recommended in `rust/docs/THIRD_PARTY_EQUIVALENTS.md` (once it
  exists) or tiny, well-known, pure-Rust crates. When C++ uses a third-party library whose exact
  behaviour affects results (Clipper2, rtree ordering, delaunator, fmt/double formatting), the
  Rust equivalent must reproduce that behaviour; otherwise port the needed part by hand.

## 6. Tests

- Port the matching C++ Boost tests from `<kicad>/qa/tests/...` (same test names, same data
  tables, same expectations) into `#[test]`s, in the crate's `tests/` dir or `#[cfg(test)]`
  modules. Name the C++ test file in a comment. Do not weaken an expectation to make it pass:
  a failing ported test means the port is wrong (or record a justified deviation).
- Test data: use the files already in `<kicad>/qa/data` and `<kicad>/demos` via the
  `kicad-qa` dev-dependency (`kicad_qa::qa_data_dir()`, `pcbnew_data_dir()`, `demos_dir()`).
  Do not copy large data files into `rust/`.
- A reference `kicad-cli` built from the C++ tree may be available (`kicad_qa::oracle_cli()`),
  see `rust/docs/ORACLE.md` once it exists. Tests that need it must skip silently when it is
  absent. Golden files produced with it are committed under the crate's `tests/golden/` (keep
  them small) so the default test run needs no oracle.
- Required to be green before you report a work package as done (run from `<kicad>/rust`):
  - `cargo build -p <crate>` and `cargo test -p <crate>`
  - `cargo build -p <crate> --target wasm32-unknown-unknown` (library crates)
  - `cargo clippy -p <crate> --all-targets` without warnings
  - `cargo fmt -p <crate>`

## 7. Working alongside other agents

Several agents work in this workspace at the same time, in different crates or different files.

- You own exactly the files/modules named in your assignment. Do not edit other files, with two
  exceptions: (a) a genuine bug in another crate that blocks you - fix it minimally, add a test
  for it there, make sure `cargo test -p <that crate>` still passes, and report it; (b) adding a
  dependency line to your own crate's `Cargo.toml`. Never edit the workspace root `Cargo.toml`
  (report what you need instead).
- Keep the workspace compiling: make edits in complete increments and run `cargo check` right
  after each burst of edits. If `cargo` fails because of errors in a file you do not own, it is
  another agent mid-edit: wait a minute or two (`sleep 60`) and retry instead of touching it.
  Cargo may also print "Blocking waiting for file lock on build directory" - that is normal,
  wait.
- Never run `git commit`, `git add`, `git checkout`, `git stash`, `git reset`, `git clean` or
  anything else that changes the index/working tree state; the orchestrator commits. Never
  modify anything outside `<kicad>/rust/` (the C++ tree is read-only reference).
- Scratch files go under `rust/target/` or the system temp dir, not into the source tree.
- Do not use `cargo install` or `rustup` without being asked to.

## 8. `PORT_STATUS.md` (one per crate, owned by the crate's agents)

A table mapping C++ files/classes/functions to their Rust location and state
(`ported`, `partial`, `stub`, `skipped: reason`), the list of ported C++ tests, plus a
"Deviations" section listing every intentional behavioural difference and a "Known gaps"
section. Keep it current; it is what the reviewers and the final README are built from.

## 9. Project-wide decisions (binding)

- **Reference platform**: results must match KiCad built with g++/libstdc++ on Linux (the
  oracle, `docs/ORACLE.md`). Where C++ behaviour is implementation-defined (tie order of
  `std::sort`, `int64_t` being `long`), the libstdc++/Linux behaviour is the specification.
- **`std::sort` tie order**: whenever the C++ calls `std::sort`/`std::partial_sort`/
  `std::nth_element` with a comparator under which distinct elements can compare equal and the
  resulting order can influence output, use the libstdc++-exact ports in `kicad_geom::stdsort`
  (not `slice::sort*`). `std::stable_sort` maps to `slice::sort_by` (both stable).
- **libm**: call the `f64` std methods through `kicad_geom::cmath` only. Natively this is glibc,
  identical to the oracle. On wasm32 Rust's bundled libm is used, which can differ from glibc by
  one ulp in `sin/cos/atan2/acos/exp/pow`; this is an accepted, documented wasm-only deviation.
- **Clipper2**: the vendored, fidelity-patched `clipper2-rs` in `rust/vendor/clipper2-rs` is the
  only polygon clipper. See `docs/research/polygon.json`.
- **Spatial indices, delaunator, nanoflann**: hand ports living in `kicad-geom` (query visit
  order matters). No external spatial crates. See `docs/research/spatial.json`.
- **Self-verification**: every work package that ports numerical or text-producing code extends
  its crate's differential oracle (`tests/oracle/`: a C++ driver linking the original sources,
  golden output committed under `tests/golden/`, a Rust test replaying it). A package is not done
  until its oracle sections match bit for bit, or each mismatch is explained in
  `PORT_STATUS.md`.

## 10. Licence header

KiCad is GPL-3.0-or-later and so is this port. Start every `.rs` file with:

```rust
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
```

If the ported C++ file carries additional copyright lines (e.g. CERN, individual authors, or a
third-party licence such as Boost/MIT for bundled libraries), carry those lines over as well.
