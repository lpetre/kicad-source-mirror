# clipper2-rs, vendored for the KiCad Rust port

This directory is a vendored, patched copy of the `clipper2-rs` crate. By project decision
(`rust/PORTING.md` section 9, `rust/docs/research/polygon.json`) it is the only polygon clipper
used by the port; `kicad-geom` depends on it by path with the `usingz` feature (KiCad compiles
its bundled Clipper2 with `USINGZ` and carries arc indices in the Z values).

## Provenance

- Crate: `clipper2-rs` 0.3.0 from crates.io (`clipper2-rs-0.3.0.crate`), upstream repository
  `https://github.com/serdarvaran/clipper2-rs`, packaged from commit
  `edf38422bcd94afb7991429cd301659befd63f5e` (`.cargo_vcs_info.json`).
- It is a pure Rust port of Angus Johnson's Clipper2 (2.0.1 plus later upstream fixes,
  upstream commit `b496b45`), i.e. the same post-2.0.1 behaviour as KiCad's
  `thirdparty/clipper2`.
- Licence: BSL-1.0 (`LICENSE`, unchanged), compatible with GPL-3.0-or-later.

## Local changes

1. `rust/docs/research/artifacts/polygon-clipping/clipper2-rs-0.3.0-kicad-fidelity.patch`
   applied unchanged:
   - new `src/gnu_sort.rs`, a port of libstdc++'s `std::sort` (introsort), used by
     `engine/intersect.rs` to sort the intersection list. Upstream clipper2-rs uses a stable
     sort; Clipper2 C++ uses `std::sort`, whose tie order changes output vertex order (and in
     rare cases geometry) when a scanbeam holds more than 16 intersections with ties.
   - `src/offset.rs`: restores the C++ `delta_ = std::abs( delta_ )` quirk for zero-area
     groups, so a deflate of a set holding a degenerate polygon behaves like KiCad.
2. `Cargo.toml`: the `criterion` dev-dependency (benchmarks, not part of the published crate)
   was removed. `Cargo.lock` and `.cargo-ok` were removed.

With both patches the crate reproduces KiCad's bundled C++ Clipper2 (g++ 13, `-DUSINGZ`)
byte for byte on the research harness (97,893 cases natively, wasm32 under node); see
`rust/docs/research/polygon.json` and the re-validation numbers in
`rust/crates/kicad-geom/PORT_STATUS.md`.

Treat this copy as owned code: re-run the harness in
`rust/docs/research/artifacts/polygon-clipping/` whenever KiCad updates `thirdparty/clipper2`.
