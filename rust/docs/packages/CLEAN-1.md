# CLEAN-1: Consolidation in the finished foundation crates

Crate: kicad-geom, kicad-core, kicad-font, kicad-plot, kicad-libeval (finished crates; no other agent is editing them, but a dozen agents compile against them: keep every existing public item and signature working - additions and internal changes only)

A list of small follow-ups collected from the package reports. Do each one, keep every crate's
tests and differential oracles green, and update the PORT_STATUS.md of each crate you touch.

1. libstdc++ `std::set` insertion order: `rust/crates/kicad-pcb/src/io/writer/mod.rs` contains
   an exact port of the libstdc++ red-black tree insert (`StdRbTree` / `sorted_set`), needed
   wherever the C++ builds a `std::set<T, CMP>` with a comparator that is not a strict weak
   ordering. Move the generic tree into `kicad_geom::stdsort` (e.g. `std_set_order`), with a
   differential test against g++ (`std::set` with deliberately non-transitive comparators,
   several thousand random sequences: compare iteration order; C++ scratch files under
   rust/target/, built under the machine-wide flock), and leave `sorted_set` in kicad-pcb as a
   thin wrapper calling it (that one file in kicad-pcb is the only kicad-pcb file you edit).
2. `wxString::Matches`: hand-ported as `wx_string_matches` in
   `rust/crates/kicad-pcb/src/netclass.rs`. Add the canonical port to
   `kicad_core::string_utils` (verify against the wx implementation semantics with a small
   differential test through libkicommon.so or wx directly - see
   rust/crates/kicad-core/tests/oracle for the build pattern) and report the call site so it
   can be switched (do not edit netclass.rs).
3. kicad-libeval carries local copies of helpers that now exist in kicad-core
   (`src/eda_units.rs`, `src/string_utils.rs`, `src/crt.rs`: see "Duplicates to consolidate"
   in its PORT_STATUS.md). Make kicad-libeval depend on kicad-core and delete the duplicates
   where the kicad-core function is identical (prove it with the existing oracle goldens of
   kicad-libeval: all 39,556 cases must still match); keep a local copy only where behaviour
   differs, and say why.
4. kicad-font was ported before the "out-of-range double to integer casts follow x86" rule
   (PORTING.md section 9): find its saturating `as` casts of doubles and its wrapping
   arithmetic notes (PORT_STATUS deviation D3), apply `kicad_geom::cmath::f64_to_i32_x86`
   where the C++ does a plain cast, and extend the font oracle with out-of-range inputs to
   confirm the reference behaviour at each site. Also: the board model has a stand-in
   `FontRef` in `rust/crates/kicad-pcb/src/foundation_stub.rs` ("REMOVE when kicad-font
   provides X"): add the real type to kicad-font with the same fields and semantics and
   report it (do not edit kicad-pcb).
5. kicad-plot: `GerberPlotter::flash_pad_chamfer_round_rect` and
   `SvgPlotter::set_round_rect_polygon_fn` take the polygon generator as a closure because
   `TransformRoundChamferedRectToPolygon` did not exist in kicad-geom when they were written.
   It exists now (`kicad_geom::convert_basic_shapes_to_polygon`): make the plotters use it by
   default (keep the closure-taking entry points as thin wrappers so existing callers
   compile), make SVG `Rect` with a corner radius plot the rounded rectangle through
   `ShapeRect::outline` / ROUNDRECT as the C++ does (currently plots nothing), remove the
   `debug_assert` on a negative stroke width in the Gerber PlotText path (the C++ only asserts
   and continues), and extend the Gerber and SVG scripted oracles to cover round-rect and
   chamfered pads and rounded rects through the default path (identical bytes).
6. kicad-geom: `geometry::mirror` / `mirror_point` panic on i32 overflow where the reference
   build wraps (reported by the reference-image package): confirm with the oracle and make
   them wrap; update `docs/design/geom-shapes.md` where it still describes the
   `unsupported_g5` routing as current, and the stale GAP-5 entry of kicad-geom's
   PORT_STATUS.md.
7. Run `cargo fmt --check -p <crate>` and `cargo clippy -p <crate> --all-targets` for the five
   crates and fix what is reported in them (not in other crates).

Verification: every existing test and differential oracle of the five crates passes
(`cargo test -p kicad-geom`, `-p kicad-core`, `-p kicad-font`, `-p kicad-plot`,
`-p kicad-libeval`; kicad-geom's oracle replays take several minutes), the wasm32 builds
pass, plus the new differential tests named above. Report per item: done / partly / skipped
with the reason, and the list of call sites in other crates that should now be switched.
