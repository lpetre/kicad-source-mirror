# Third-party libraries: Rust equivalents

Status: research report, 2026-11-09. Scope is the port described in `docs/PLAN.md` (Tier 1-3).
Reference platform is the g++/libstdc++ Linux build used as oracle (`docs/ORACLE.md`,
PORTING.md section 9).

Sources of the numbers below:

- `docs/research/polygon.json` and `docs/research/spatial.json` (Clipper2, r-trees,
  delaunator, nanoflann, dynamic_bitset, libpopcnt, triangulation, `std::sort` tie order). Their
  conclusions are binding (PORTING.md section 9) and are condensed in section 3.1-3.2.
- Differential experiments left by an earlier, interrupted research run in
  `rust/target/research/serialisation/` (JSON, number formatting/parsing, zstd, MMH3, UUID, regex)
  and `rust/target/research/parsers/` (markup grammar, lemon/libeval). Their C++ drivers link the
  bundled thirdparty sources; I re-ran the comparison scripts on the existing outputs for this
  report (numbers below are from those re-runs).
- New experiments for this report in `rust/target/research/report/` (wasm32 build of the whole
  recommended crate set, FreeType/HarfBuzz vs harfrust/skrifa, wasm-bindgen toolchain).

All scratch projects carry their own empty `[workspace]` table; the workspace `Cargo.toml` was
not touched. "not verified" means the statement is from knowledge or reading only.

Workspace MSRV note: the workspace declares `rust-version = "1.85"` (toolchain installed: rustc
1.97). With resolver 3 the MSRV-aware resolver picks older releases when a dependency is
unconstrained; the versions recommended below that need more than 1.85 are marked (MSRV x.y).
Either pin the older line or raise `rust-version`; this is an orchestrator decision.

## 1. Decisions

Libraries core to the port (Tier 1-2). "Hand port" means KiCad's or the library's algorithm is
ported statement by statement into the named crate.

| C++ library | Rust choice | Version | Licence | wasm32 | Fidelity risk |
|---|---|---|---|---|---|
| clipper2 | vendored, fidelity-patched `clipper2-rs` in `rust/vendor/clipper2-rs` (feature `usingz`) | 0.3.0 + 2 patches | BSL-1.0 | verified (build + run in node) | low (97,893 cases byte-identical) |
| KIRTREE (packed/dynamic r-tree, KiCad code) | hand port in kicad-geom | - | GPL-3.0+ | verified | low |
| rtree (thirdparty) | not needed (unused by production code) | - | - | - | none |
| delaunator | hand port in kicad-geom / kicad-connectivity | - | MIT (orig.) | verified | low |
| nanoflann | hand port of the ~400-line subset in kicad-geom | - | BSD (orig.) | verified | low |
| `std::sort` / `std::partial_sort` (libstdc++) | hand port `kicad_geom::stdsort` | - | GPL-3.0+ (libstdc++ has the GCC runtime exception; algorithm port) | verified | low (libstdc++-specific by design) |
| polygon_triangulation.h (KiCad code) | hand port in kicad-geom | - | GPL-3.0+ | n/a | low if ported exactly (not yet compared against C++) |
| dynamic_bitset + libpopcnt | std (`Vec<u64>` / `[u64; 2]`, `u64::count_ones`) in kicad-core `base_set.rs` | - | - | verified | none |
| nlohmann_json (parse, value model, sorted and ordered objects) | `serde_json` with features `preserve_order` + `float_roundtrip`; objects sorted with `Value::sort_all_objects()` where C++ uses `nlohmann::json` | 1.0.154 | MIT OR Apache-2.0 | verified (build + run in node) | low with the hand-written formatter below |
| nlohmann_json `dump()` / `operator<<` (indent, Grisu2 floats) | hand port: `serde_json::ser::Formatter` + port of nlohmann `to_chars` (Grisu2) and `format_buffer` | - | MIT (orig.) | builds (pure integer code) | low (identical on 1,253,424 doubles and 478/479 documents) |
| nlohmann `ignore_comments` | hand-written comment stripper (or `json_comments` 0.2.2, Apache-2.0, not verified) | - | - | not verified | low |
| json_schema_validator | not needed (API plugins, remote providers) | - | - | - | none |
| fmt | hand port: kicad-sexpr `formatter/cformat.rs` (production); research prototype `serialisation/src/cfmt.rs` | - | - | verified | low |
| fast_float | hand port: kicad-sexpr `lexer/c_numeric.rs` (`from_chars_double`) on top of `str::parse::<f64>` | - | - | verified | none on tested inputs |
| magic_enum | plain `match` / `TryFrom<i32>` per enum (optionally `strum` 0.28.0, MIT) | - | - | strum verified | none |
| picosha2 | not needed in scope (embedded files use MMH3; SHA-256 only in git/OAuth); if needed `sha2` | 0.11.1 (MSRV 1.88; 0.10.x for 1.85) | MIT OR Apache-2.0 | verified | none |
| MMH3_HASH (KiCad code, embedded-file checksum) | hand port in kicad-core (both `addData` and legacy `addDataV1`) | - | GPL-3.0+ | not built yet | low (prototype matches all 107 stored checksums) |
| zstd (embedded files, fonts) | `ruzstd` (pure Rust decoder + encoder) | 0.9.1 (MSRV 1.87; 0.8.x for 1.85) | MIT | verified (build + run in node) | none for load/save (stored frames are re-written verbatim); new compressions differ from libzstd level 15 |
| zlib / gzip-hpp | `flate2` with `default-features = false, features = ["rust_backend"]` (miniz_oxide) - only if a gzip path enters scope | 1.1.11 | MIT OR Apache-2.0 | verified | not applicable (no in-scope user) |
| wxBase64 | `base64` | 0.23.1 | MIT OR Apache-2.0 | verified | none (107/107 round trips) |
| boost::uuid + boost::mt19937 | hand port in kicad-core (`kiid.rs`, `mt19937.rs`) | - | BSL-1.0 (orig.) | verified by its crate | none (3,827/3,827 identical) |
| lemon (libeval grammar) | hand port of the generated LALR tables in kicad-libeval (`lemon.rs`, `libeval_compiler/grammar.rs`) | - | public domain (orig.) | verified by its crate | none (8,211/8,211 identical) |
| DSNLEXER keyword generator (TokenList2DsnLexer.cmake) | generated keyword modules committed in kicad-sexpr (`lexer/keywords/*.rs`) | - | - | verified by its crate | none |
| pegtl (markup grammar `include/markup_parser.h`) | hand-written recursive descent with packrat memo of `braces<>` (Tier 2 text) | - | - | not built in a crate yet | none on 11,287 cases |
| wxRegEx (PCRE2) for netclass patterns | wildcard: hand port; regex: open decision (`regress` closest; no exact pure-Rust PCRE2) | regress 0.12.0 | MIT OR Apache-2.0 | not verified | medium for regex netclass patterns |
| freetype + harfbuzz (outline fonts, Tier 2) | `harfrust` (shaping, with hb-ft emulation through `FontFuncs`) + `skrifa` (outlines, hinting) + hand ports (FT size request, `FT_Outline_Decompose` closing segment, `FT_Outline_Embolden`, fixed-point unhinted scaling) | harfrust 0.15.0, skrifa 0.49.0 | MIT; MIT OR Apache-2.0 | verified (build + run in node) | low for upem-2048 TrueType fonts, medium for upem-1000 and CFF fonts (see 3.9) |
| fontconfig | not ported: font bytes come from embedded files or are supplied by the caller (name -> bytes map) | - | - | - | medium (font substitution differs from fontconfig) |
| thread-pool (BS::thread_pool) | sequential code (PORTING.md 5); optional `rayon` 1.12.0 behind a non-default `parallel` feature | 1.12.0 | MIT OR Apache-2.0 | builds; runtime on wasm not verified | none if sequential |
| expected (tl::expected) | `Result` | std | - | - | none |
| argparse (kicad-cli) | `clap` (native-only `kicad-cli`) | 4.6.7 | MIT OR Apache-2.0 | not needed | low (help/usage text differs) |
| wasm toolchain | `wasm-bindgen` =0.2.130 + `wasm-bindgen-cli` 0.2.130, `js-sys`/`web-sys` 0.3.107, `console_error_panic_hook` 0.1.7, `serde-wasm-bindgen` 0.6.5, `getrandom` 0.4.3 (`wasm_js`) only in kicad-wasm | see section 4 | MIT OR Apache-2.0 | verified | n/a |

## 2. Every thirdparty directory and system dependency

Scope: core = needed for Tier 1/2 output; optional = Tier 3 or useful later; out = not in the
port. "wasm" refers to the named Rust equivalent.

### 2.1 `thirdparty/` (all 38 directories)

| Directory | What KiCad uses it for (call sites) | Scope | Rust equivalent | Pure Rust | wasm32 |
|---|---|---|---|---|---|
| 3dxware_sdk | 3Dconnexion SpaceMouse input (`common/spacenav/*`, 3D viewer) | out | none needed | - | - |
| argparse | kicad-cli command line (`kicad/kicad_cli.cpp`, `kicad/cli/command*.cpp`) | core (CLI only) | `clap` 4.6.7 | yes | not needed (native CLI) |
| clipper2 | SHAPE_POLY_SET booleans/offsets (`libs/kimath/src/geometry/shape_poly_set.cpp`, `shape_line_chain.cpp`) | core | vendored patched `clipper2-rs` 0.3.0 | yes | verified |
| compoundfilereader | OLE compound files: Altium, EasyPC import (`common/io/altium/altium_binary_parser.h`, `common/io/easypc/easypc_file.cpp`) | out | `cfb` crate (not verified) | yes (not verified) | not verified |
| delaunator | ratsnest triangulation (`pcbnew/ratsnest/ratsnest_data.cpp:47,228`) | core | hand port | yes | verified |
| dxflib_qcad | DXF import (`common/import_gfx/dxf_import_plugin.cpp`) | out | `dxf` crate (not verified) | not verified | not verified |
| dynamic_bitset | `BASE_SET`/`LSET` base class (`include/base_set.h:34,51`) | core | std `Vec<u64>` (kicad-core `base_set.rs`) | yes | verified |
| eigen | linear algebra for planegcs (`thirdparty/planegcs`), constraint solver | out | `nalgebra` (not verified) | yes | not verified |
| expected | `tl::expected` (31 files, e.g. `common/libraries/library_table_parser.cpp`, API handlers) | core (idiom) | `Result` | yes | yes |
| fast_float | number parsing in `common/dsnlexer.cpp`, `pcb_io_kicad_legacy.cpp`, `text_eval_wrapper.cpp` | core | hand port in kicad-sexpr `c_numeric.rs` over `str::parse::<f64>` | yes | verified |
| fmt | string/number formatting, 98 files (`common/string_utils.cpp` FormatDouble2Str/UIDouble2Str, `libs/sexpr`) | core | hand port in kicad-sexpr `cformat.rs` | yes | verified |
| glad | OpenGL loader for GAL (`common/gal/opengl/*`) | out | n/a (no GPU rendering) | - | - |
| gzip-hpp | gzip of STEP/VRML models (`pcbnew/exporters/step/step_pcb_model.cpp:3688`) | out | `flate2` 1.1.11 (rust_backend) | yes | verified |
| json_schema_validator | API plugin manifests, remote provider metadata (`common/api/api_plugin.cpp`, `common/remote_provider_*.cpp`, `common/json_schema_validator.cpp`) | out | `jsonschema` 0.61.0 (MIT, not verified) | not verified | not verified |
| lemon | parser generator for libeval/numeric evaluator (`common/libeval*/grammar.lemon`) | core | hand port in kicad-libeval | yes | verified |
| libcontext | coroutines for the tool framework (`include/tool/coroutine.h`) | out (Tier 3 router is ported without coroutines) | none | - | - |
| libemf2svg | EMF images in Altium/OrCAD schematic import (`eeschema/sch_io/ole_emf.cpp`) | out | none mainstream | - | - |
| libpopcnt | popcount inside `dynamic_bitset::count()` | core | `u64::count_ones` | yes | verified |
| libwmf | WMF images in OrCAD import (`libwmf_orcad` target, `eeschema/CMakeLists.txt:734`) | out | none mainstream | - | - |
| magic_enum | enum<->string/int, 53 files (`pcb_io_kicad_sexpr_parser.cpp:8387` `enum_cast<ZONE_CONNECTION>`, `pad.cpp`, `padstack.cpp` names in messages) | core | `match`/`TryFrom` (or `strum` 0.28.0) | yes | verified (strum) |
| markdown2html | Markdown help text in dialogs (`eeschema/sch_label.cpp`, BOM/simulator dialogs) | out | `pulldown-cmark` (not verified) | yes (not verified) | not verified |
| nanodbc | ODBC database libraries (`common/database/odbc_backend.cpp`) | out | none (no ODBC on wasm) | - | - |
| nanosvg | SVG import (`common/import_gfx/svg_import_plugin.cpp:24`) | out | `usvg` (not verified) | yes (not verified) | not verified |
| nanoflann | kd-tree in outline building (`pcbnew/convert_shape_list_to_polygon.cpp:44`), board-shape repair tool | core | hand port | yes | verified |
| nlohmann_json | settings (`common/settings/json_settings.cpp`), DRC JSON report (`pcbnew/drc/drc_report.cpp:220`), gbrjob (`pcbnew/exporters/gerber_jobfile_writer.cpp`), 234 files total | core | `serde_json` 1.0.154 + hand-written formatter | yes | verified |
| pegtl | markup parser (`common/markup_parser.cpp`, `include/markup_parser.h`), library table parser (`common/libraries/library_table_parser.cpp`), CADSTAR parts library | core (markup, Tier 2 text); library tables out | hand-written recursive descent | yes | not built in a crate yet |
| picosha2 | SHA-256 in `common/embedded_files.cpp:56` (legacy path), git known hosts, OAuth PKCE | optional | `sha2` 0.11.1 | yes | verified |
| planegcs | geometric constraint solver (`pcbnew/constraints/*`) | out | none | - | - |
| potrace | bitmap tracing to footprints (`common/bitmap_tracing.cpp`) | out | none needed | - | - |
| rapidcsv | CSV import (`common/io/csv.cpp`) | out | `csv` crate (not verified) | yes (not verified) | not verified |
| rectpack2d | footprint spreading on netlist import (`pcbnew/autorouter/spread_footprints.cpp:40`) | out | none (port by hand if ever needed) | - | - |
| rtree | only a QA benchmark (`qa/tests/libs/kimath/geometry/bench_spatial_index.cpp:22`) | out | not needed | - | - |
| sentry-native | crash reporting (`common/single_top.cpp`, data-collection panel) | out | none | - | - |
| thread-pool | `BS::thread_pool` (`GetKiCadThreadPool`), 63 files incl. `pcbnew/drc/drc_engine.cpp`, DRC providers, zone filler, connectivity | core (as sequential code) | sequential; optional `rayon` 1.12.0 | yes | builds (runtime not verified) |
| tinyspline_lib | B-splines in DXF import (`common/import_gfx/dxf_import_plugin.cpp`) | out | none needed | - | - |
| turtle | header-only mock library for tests; included by `pcbnew/router/pns_meander.h` | out (test-only) | not needed | - | - |
| vma | Vulkan memory allocator for the new RHI (`common/rhi/rhi_vma_impl.cpp`) | out | none | - | - |
| zint | barcodes on boards (`pcbnew/pcb_barcode.cpp`: Code 39, Code 128, Data Matrix, QR, Micro QR); PCB_BARCODE is parsed, written and seen by DRC providers | optional (needed if boards with barcodes must plot/DRC identically) | hand port of the used symbologies, or `qrcode`/`datamatrix` crates (not verified, will not match zint's symbol choices exactly) | not verified | not verified |

### 2.2 System dependencies

| Dependency | Use in KiCad | Scope | Rust equivalent | Pure Rust | wasm32 |
|---|---|---|---|---|---|
| wxWidgets | GUI, strings (`wxString`), `wxRegEx`, `wxBase64`, file I/O, locale | core pieces only | `String`/`&str` (kicad-core `wxstr.rs`), `base64` 0.23.1, regex: see 3.7 | yes | verified (base64) |
| boost (uuid, random) | `KIID` (`common/kiid.cpp`) | core | hand port in kicad-core | yes | verified |
| boost (ptr_container) | netlist containers (`pcbnew/netlist_reader/pcb_netlist.h`, 13 files) | out (netlist import) | `Vec<Box<T>>` | yes | yes |
| boost (bimap) | no include in this tree | out | - | - | - |
| boost (others: ptree, geometry, algorithm, ...) | 373 files; ptree in `common/ptree.cpp` (legacy formats) | out unless a ported file needs a piece; then hand port | - | - | - |
| cairo, pixman | Cairo GAL and PNG plotter (`include/gal/cairo/*`, `include/plotters/plotter_png.h`) | out | `tiny-skia` if raster output is ever wanted (not verified) | yes | not verified |
| curl | HTTP libraries, remote providers, updates (`common/kicad_curl/*`) | out | none (browser fetch in kicad-wasm if needed) | - | - |
| libgit2 | project git integration (`common/git/*`, 26 files) | out | none | - | - |
| OCCT (OpenCASCADE) | STEP/VRML/U3D export, 3D models (`pcbnew/exporters/step/*`, `u3d/*`) | out | none (no pure-Rust B-rep kernel) | - | - |
| ngspice | circuit simulation (eeschema) | out | none | - | - |
| protobuf | IPC API (`common/api/*`, `pcb_shape.cpp`/`pcb_track.cpp` Serialize) | out | `prost` if the API is ever ported (not verified) | yes | not verified |
| nng | IPC API transport (`libs/kinng`, `common/api/api_server.cpp`) | out | none | - | - |
| sqlite | database libraries (`common/database/sqlite_backend.cpp`) | out | none | - | - |
| ODBC (unixODBC) | database libraries via nanodbc | out | none | - | - |
| Python / SWIG | scripting bindings | out | none | - | - |
| OpenGL / GLEW / glad | GAL rendering, 3D viewer | out | none | - | - |
| GLM | vector maths in 3D viewer, RHI, PDF 3D (`include/rhi/*`, `common/plotters/PDF_plotter.cpp`) | out | `glam` (not verified) | yes | not verified |
| freetype | outline glyph loading (`common/font/outline_font.cpp`, `outline_decomposer.cpp`) | Tier 2 | `skrifa` 0.49.0 + hand ports | yes | verified |
| harfbuzz | text shaping (`common/font/outline_font.cpp:348-380`) | Tier 2 | `harfrust` 0.15.0 | yes | verified |
| fontconfig | font discovery/substitution (`common/font/fontconfig.cpp`, 43 files) | Tier 2 (lookup only) | caller-supplied font map; `fontdb` 0.24.0 (MIT) as optional helper (not verified) | yes | not verified |
| zstd | embedded files (`common/embedded_files.cpp:274-344`, level 15) | core | `ruzstd` 0.9.1 | yes | verified |
| zlib | gzip for STEP/VRML, PNG | out | `flate2` 1.1.11 rust_backend | yes | verified |
| potrace, tinyspline, dxflib, nanosvg, zint, libemf2svg, libwmf, eigen/planegcs, sentry, markdown2html, rapidcsv, compoundfilereader, rectpack2d, libcontext, 3dxware, vma, glad | bundled copies in `thirdparty/`, see 2.1 | see 2.1 | see 2.1 | - | - |

## 3. Details for the core libraries

### 3.1 Polygon clipping: Clipper2 (binding decision)

Condensed from `docs/research/polygon.json`; experiments in `rust/target/research/polygon-clipping/`
(`run_all.sh` reproduces them).

- Bundled version: `clipper.version.h` says 2.0.1, but the copy is a post-tag upstream snapshot
  (engine dated 21 Feb 2026) with KiCad-local patches in `clipper.engine.h/.cpp` (StripeIndex
  point-in-polygon index for PolyTree nesting, `IntersectListSort` functor, `<cmath>`). Built with
  `-DUSINGZ`.
- Usage: only `libs/kimath` talks to Clipper2 (`shape_poly_set.cpp`, `shape_line_chain.cpp`);
  everything else goes through SHAPE_POLY_SET (`Inflate(` 317 call lines, `Simplify()` 148,
  `BooleanSubtract` 113, `Fracture()` 92, ...).
- Candidates: clipper2-rs 0.3.0 (pure Rust, usingz, PolyTree, ClipperOffset, wasm ok);
  clipper2-rust 1.2.0 (83.92% exact, old FixSelfIntersects, different `area()`); clipper2 0.6.0,
  clipper2c-sys 0.2.0, clipper2-sys 1.0.0, geo-clipper 0.9.0 (C++ wrappers, wasm build fails);
  i_overlay 9.1.0 (6.2% exact), geo 0.33.1 (4.1% exact); polyclip, polystrata,
  tippecanoe-rs-clipper2, solidmesh-clipper not run.
- Experiments: 97,893 randomised and degenerate cases (KiCad-like zones, tracks, pads, thermal
  spokes, offsets with all join/end types, upstream Polygons.txt, ...). Unpatched clipper2-rs:
  67,467/78,174 exact (86.30%); rebuilding the C++ with `std::stable_sort` for the intersection
  list made it match 78,174/78,174, isolating the cause. Patch 1 ports libstdc++ `std::sort`
  (introsort, insertion sort below 16 elements) for the intersection list; patch 2 restores
  `delta_ = abs(delta_)` for zero-area groups (756 -> 1,260/1,260 targeted cases). With both:
  byte-identical (z-callback sequence, PolyTree shape, vertex/contour order, z values) on all
  97,893 cases natively and on every case run under wasm32 in node.
- Caveats: (1) the tie order is libstdc++'s; KiCad built with libc++/MSVC differs. (2) wasm libm:
  Rust's bundled libm differs from glibc by one ulp in about 3% of sin/cos, 18% of atan2, 7.6% of
  acos, 10% of exp/powf calls (sqrt, cbrt, round, fmod identical); for ClipperOffset round joins
  `step_sin` differed in 31 and `step_cos` in 15 of 17,323 combinations, derived step counts were
  identical in all 121,261 combinations, and no output difference appeared in any wasm run, but a
  rare 1 nm or vertex-count difference on wasm is possible. (3) upstream UB (empty-path offset
  group segfaults in C++) cannot be matched. (4) KiCad's StripeIndex patch is empirically
  output-neutral (19,080 cases with index forced on/off). (5) young single-maintainer crate: the
  vendored copy is owned code; re-run the harness when KiCad updates `thirdparty/clipper2`.

### 3.2 Spatial indices, triangulation, bit sets, sort order (binding decisions)

Condensed from `docs/research/spatial.json`; experiments in `rust/target/research/spatial/`.

- **rtree (thirdparty)**: unused by production code (only
  `qa/tests/libs/kimath/geometry/bench_spatial_index.cpp`). Not ported.
- **KIRTREE** (`libs/kimath/include/geometry/rtree/`, KiCad code): PACKED_RTREE (DRC_RTREE,
  SEGMENT_INDEX, TRIANGULATION_INDEX, zone filler, ...), DYNAMIC_RTREE (CN_RTREE, ...),
  COW_RTREE. DRC results depend on query visit order. Crates: rstar 0.14.0 gives equal result sets
  on 20,850/20,850 queries but the PACKED order on only 438 of 6,927 multi-hit queries;
  static_aabb2d_index 2.1.0 435/6,927; geo-index 0.4.0 657/6,927; integer coordinates overflow
  (panics with overflow checks). Hand ports: PackedRTree iteration order 437/437 trees and visit
  order 10,925/10,925 queries (65,550 over 6 seeds); DynamicRTree 20,850/20,850 for BulkLoad,
  Insert and Remove in 2D and 3D (187,650 comparisons), including full-int-range cases. Porting
  notes: HilbertXY2D needs wrapping u32 arithmetic; R*-tree area/overlap products need
  `wrapping_mul` (int64 overflow is UB in C++ and wraps in practice). wasm checksums equal native.
- **delaunator** (abellgithub/delaunator-cpp snapshot; one user, `ratsnest_data.cpp`): the
  `delaunator` crate 1.1.0 is a different variant (robust orient2d, other seed/hash/sort
  details): identical arrays on 896/2,721 conforming sets, identical edge sets 2,112; KiCad's MST
  differs on 125/2,721 (all near-collinear: 57/284 board-scale near-collinear sets). Hand port:
  byte-identical on 37,096 cases (15 seeds), including the 142/2,721 near-collinear cases where
  the C++ leaves anchors out (the port reproduces the missing ratsnest edges). Notes: do size_t
  maths in u64 so wasm32 matches; `hash_key` must map NaN to 2^63 like glibc `llround`; keep the
  floating expression order.
- **nanoflann** 1.7.1: KiCad uses `radiusSearch` (re-sorted by KiCad, only the set matters,
  strict `<`) and `knnSearch(8)` for the "not a closed shape" report (order of equidistant
  neighbours matters). Crates (kiddo 6.3.0, kd-tree 0.6.2, rstar) get the radius sets right
  (rstar only with a strict post-filter) but knn order on 12,475-13,515 of 48,000 queries. Hand
  port: 48,000/48,000 for both queries on 4 seeds (192,000 queries).
- **rectpack2d**: out of scope (footprint spreading); rectangle-pack, crunch, guillotiere,
  etagere build for wasm but implement other packers.
- **libpopcnt / dynamic_bitset** 1.3.3: only `count()` and the basic bit operations are used by
  `BASE_SET`/`LSET`. A std-only `Vec<u64>` matched sul::dynamic_bitset (with libpopcnt) after
  200,000/200,000 random operations; fixedbitset 0.5.9 also matched but adds nothing; bitvec
  1.1.1 does not compile for wasm32 with u64 storage. The risk is in KiCad's own layer
  (`FmtHex`/`ParseHex`, compare, hash, auto-resize) which is ported from `base_set.h`.
- **polygon_triangulation.h** (KiCad code): hand port. earcutr 0.5.0, earcut 0.4.12,
  i_triangle 0.51.0, spade 2.15.1 all give different triangle sets (first triangle differs on a
  square with a hole); triangle order influences collision witness points. Not yet compared
  against the C++.
- **`std::sort` tie order**: `sort_unstable_by` matched libstdc++ on 1,197 of 3,102 arrays (the
  tie-free ones). A 330-line port of libstdc++ introsort/partial_sort matched 4,602/4,602 arrays
  (2,322 with ties, McIlroy adversarial inputs driving the heapsort fallback 109 times, 1,500
  partial_sort cases). `__adjust_heap` relies on signed truncating division for lengths 0 and 1
  (use `isize`). `std::stable_sort` maps to `slice::sort_by`.

### 3.3 JSON: nlohmann_json

**Usage in scope.**

| Output | Type | Dump call | Indent |
|---|---|---|---|
| `.kicad_pro`, `.kicad_prl` and other settings (`common/settings/json_settings.cpp:641`) | `nlohmann::json` (std::map: keys sorted by byte value) via `JSON_SETTINGS_INTERNALS : public nlohmann::json` | `toSave.dump( 2 ) + '\n'` (`FormatAsString`: `std::setw( 2 ) << j << std::endl`) | 2 |
| DRC JSON report (`pcbnew/drc/drc_report.cpp:220-221`) | `nlohmann::json` (sorted) | `std::setw( 4 ) << saveJson << std::endl` | 4 |
| Gerber job file (`pcbnew/exporters/gerber_jobfile_writer.cpp:169,186`) | `nlohmann::ordered_json` (insertion order) | `std::setw( 2 ) << m_json << std::endl` | 2 |

Settings are parsed with `nlohmann::json::parse( ..., nullptr, ALLOW_EXCEPTIONS, IGNORE_COMMENTS )`
(`json_settings.cpp:291,419,667`). `operator<<` with `setw(n)` is `dump(n)`. Bundled version
3.11.3.

**Experiment** (`rust/target/research/serialisation/`: `cpp/json_driver.cpp` parses each file
with `nlohmann::json` and `nlohmann::ordered_json` and writes `dump(2)+'\n'`, `dump()`,
`dump(0)`, `setw(4)`, `setw(2)`; `src/bin/json_driver.rs` does the same with serde_json and the
hand-written formatter `src/nlohmann.rs`). Input: 480 documents = 386 `.kicad_pro` from `demos/`
and `qa/`, 33 other KiCad JSON files, 61 synthetic documents with extreme numbers (`-0`, `1e23`,
`5e-324`, `18446744073709551616`, `-9223372036854775809`, 30-digit integers, ...), unicode keys
and escapes.

| Comparison | Result |
|---|---|
| serde_json (no preserve_order) + hand formatter vs `nlohmann::json`, each of 5 dump styles | 478/480 identical |
| serde_json `preserve_order` + hand formatter vs `nlohmann::ordered_json`, each style | 478/480 identical |
| serde_json `preserve_order` + `sort_all_objects()` + hand formatter vs `nlohmann::json` | 478/479 identical |
| stock `serde_json::to_string_pretty` (2 spaces) vs `dump(2)+'\n'` | 417/479 identical; every differing line is a number |
| the 2 failures | `board_data_registry.json` has `//` comments (nlohmann with `ignore_comments` accepts it, serde_json rejects it); `triangulation_status.json` contains `-0`, which nlohmann stores as integer 0 and dumps as `0` while serde_json stores float -0.0 and prints `-0.0` |

**Numbers.** Over 1,253,424 finite doubles (`data/doubles.txt`: KiCad-typical values, boundary
values, random bit patterns):

- serde_json 1.0.154 (`to_string`, same text as `zmij`) differs from nlohmann's dump on 86,544
  (6.905%); 4,712 of them have different digits (Grisu2 is not always shortest: nlohmann prints
  `9.999999999999999e+22` and `4.9999999999999996e+22` where serde_json prints `1e+23` and
  `5e+22`); the rest is layout (`1e-05` vs `0.00001`, `e-07` vs `e-7`, `3.8779288371222075e+15`
  vs `3877928837122207.5`). ryu: 18.681% text mismatch.
- The hand port of nlohmann `detail::to_chars` (Grisu2) + `format_buffer` (583 lines, integer-only
  code): 0 mismatches on all 1,253,424 values.
- Parsing (570,907 valid number strings): serde_json with `float_roundtrip` equals nlohmann in
  value and type except `-0` (37 occurrences, type int vs float); without `float_roundtrip`
  136,113 values differ by one ulp. So `float_roundtrip` is required.

**Decision.** serde_json 1.0.154 with `preserve_order` and `float_roundtrip` (both features unify
across the workspace, so enable them in one place; `preserve_order` pulls `indexmap`). Hand-write:

1. A `serde_json::ser::Formatter` reproducing `basic_json::dump(indent)` (indent char, `": "`,
   empty `[]`/`{}`, nlohmann's string escaping as implemented in the prototype, which matched on
   the synthetic documents with control characters, `\u007f` and non-ASCII text) with the Grisu2 port for floats. Prototype: `serialisation/src/nlohmann.rs`.
2. Sorting: call `Value::sort_all_objects()` before dumping where C++ uses `nlohmann::json`
   (settings, DRC report); keep insertion order for `ordered_json` (gbrjob). The DRC report is
   built from structs: serialise to `Value` first, then sort.
3. Comment stripping for `ignore_comments` (`//` and `/* */`) before `serde_json::from_str`.
4. `-0` must parse as integer 0 (nlohmann) - a two-line special case in the number visitor or a
   post-pass.
5. JSON_SETTINGS semantics (json pointers, `update( ..., merge_objects = true )`, defaults,
   migrations) are KiCad code and are ported with the settings, not provided by any crate.

`json_schema_validator` is only used by the API plugin and remote-provider code: not needed.

### 3.4 Number formatting and parsing: fmt, fast_float, printf

- **fmt 12.2.0** and glibc printf: the port has exact replacements in kicad-sexpr
  `src/formatter/cformat.rs` (`format_g`, `format_f`, `format_e`; covered by its tests
  `printf_conversions_match_cpp` and 30,000 random conversions, see kicad-sexpr PORT_STATUS.md).
  The research prototype `serialisation/src/cfmt.rs` was compared against a C++ driver
  (`cpp/fmt_driver.cpp`, fmt header-only + glibc + verbatim copies of `FormatDouble2Str`,
  `UIDouble2Str`) on 1,253,424 finite and 93 non-finite doubles for 32 conversions (`{}`, `{:g}`,
  `{:.10g}`, `{:.3g}`, `{:#f}`, `{:f}`, `{:.16f}`...`{:.0f}`, `{:9.6f}`, `{:e}`, `%g`, `%.10g`,
  `%.16f`, `%f`, `%.4f`, ...): 0 finite mismatches in every conversion; the only differences were
  the 93 non-finite values in `{:9.6f}` (padding side of `inf`/`nan`) - irrelevant for board
  files but worth a test if `cformat.rs` ever formats non-finite values with a width. Rust's own
  `{}` differs from fmt `{}` in digits on 1,903 values and in layout on 31%; never use it for file
  output (PORTING.md section 3).
- **gbrjob `mapValue`** (`GERBER_JOBFILE_WRITER::mapValue`: `snprintf("%.4f")` then
  `sscanf("%Lg")` into `long double`, then to double): double rounding through x87 long double
  could differ from a direct parse. Exhaustive check over all 10^8 decimals `N/10^4` for
  N in [0, 10^8) (values 0 .. 9,999.9999): 0 differences, so `str::parse::<f64>` of the `%.4f`
  text is exact for every realistic board dimension; 59,955 of the 1.25M random doubles (huge
  magnitudes) do differ. On wasm32 there is no long double; the direct parse is what the port
  should do.
- **fast_float** (`DSNLEXER::parseDouble`): kicad-sexpr's `c_numeric.rs` scanner +
  `str::parse::<f64>` (the "hand scanner" in the experiment) matched fast_float in error code,
  consumed length and bits on 570,907 valid and 93 malformed strings (0 mismatches); fast_float
  itself matched glibc `strtod` on all valid strings. Plain `str::parse` alone rejects partial
  inputs (`1e`, `+1`, leading space) that fast_float accepts or rejects differently (17 of 93
  weird cases); fast-float2 0.2.4 and lexical-core 1.0.6 also deviate on the weird set (8 and 13
  cases). No crate needed.

### 3.5 Compression, hashing, encoding: zstd, zlib/gzip-hpp, picosha2, MMH3, base64

**Usage.** Embedded files (`common/embedded_files.cpp`): `ZSTD_compress(..., 15)` + wxBase64 on
embed (`CompressAndEncode`, line 271); on load `wxBase64Decode` + `ZSTD_getFrameContentSize` +
`ZSTD_decompress` (`DecompressAndDecode`, line 306) and a checksum check with MMH3 (seed
`EMBEDDED_FILES::Seed()`), falling back to the legacy `MMH3_HASH::addDataV1` (line 380-383).
On save the stored `compressedEncodedData` is written back verbatim (line 220-252); compression
only happens when a new file is embedded. gzip (gzip-hpp + zlib) is only used for STEP/VRML
models (`step_pcb_model.cpp:3688`), out of scope. picosha2 appears in `embedded_files.cpp:56`
(SHA-256 of decompressed data, legacy path), git known-hosts and OAuth.

**Experiment** (`serialisation/src/bin/embed_driver.rs`, 107 embedded payloads extracted from
KiCad files in the tree, references produced with the zstd 1.5.5 CLI):

- `base64` 0.23.1 decode equals the reference and re-encode equals the stored text: 107/107.
- `ruzstd` 0.9.1 `StreamingDecoder`: output identical 107/107, frame content size header
  correct 107/107.
- MMH3: hand port of `addData` matched 12/107 stored checksums, the hand port of the legacy
  `addDataV1` matched the other 95/107 (so both variants must be ported, as KiCad tries both);
  the `murmur3` 0.5.2 crate equals the current variant only (12/107). Prototype:
  `serialisation/src/mmh3.rs`.
- ruzstd encoder (only `CompressionLevel::Fastest` and a few levels are implemented): valid
  frames, 28,134,547 bytes vs 22,201,007 for KiCad's level 15 on 63,576,900 raw bytes. Output is
  not byte-identical to libzstd; only relevant if the port embeds new files (then the bytes in
  the saved board differ from what KiCad would write, the content is the same).
- `zstd` 0.14.3 (bindings to C libzstd, BSD-3-Clause) does build for wasm32-unknown-unknown
  here (clang 18 present), but it is a C dependency and is excluded by PORTING.md section 5.

**Decision.** ruzstd 0.9.1 (MSRV 1.87; the 0.8 line for MSRV 1.85 was not tested), base64 0.23.1,
hand-ported MMH3 in kicad-core (both variants). flate2 1.1.11 with the miniz_oxide backend if a
gzip path is ever needed (gzip round trip verified in the wasm smoke test). sha2 0.11.1 only if
the SHA-256 legacy path is ported (MSRV 1.88; 0.10.x for 1.85).

### 3.6 Enums, UUIDs, error types, CLI: magic_enum, boost::uuid, expected, argparse

- **magic_enum**: used for `enum_cast<T>(int)` when parsing (`pcb_io_kicad_sexpr_parser.cpp:8387`
  zone connection), enum names in messages (`pad.cpp:1180,1465,3139`) and in job/settings code.
  The names printed are the C++ enumerator identifiers (`PAD_SHAPE::ROUNDRECT` -> `ROUNDRECT`),
  so a hand-written `fn name(self) -> &'static str` with the C++ spelling is the faithful choice;
  `strum` 0.28.0 (`IntoStaticStr` with `#[strum(serialize = "...")]`, `FromRepr`) works and builds
  for wasm32 but would need a per-variant attribute because the Rust variant names follow
  PORTING.md section 2 naming. Recommendation: plain `match`.
- **boost::uuid / mt19937**: hand port in kicad-core `kiid.rs`/`mt19937.rs`; the research driver
  (`cpp/uuid_driver.cpp`, system Boost 1.83) and the hand port produced identical output on
  3,827/3,827 lines (random generation from a seeded mt19937, string parsing, nil UUIDs); the
  `uuid` 1.29.2 crate differs (302 differing lines in the random-generation block) because
  Boost's generator consumes the mt19937 stream differently. No crate.
- **expected**: `Result`.
- **argparse** (kicad-cli): `clap` 4.6.7 for the native `kicad-cli` only. Help texts and error
  messages will differ from argparse; not part of any output compared by the oracle.
- **boost::ptr_container**: only in netlist import code (`pcb_netlist.h`); `Vec<Box<T>>` if ever
  needed. **boost::bimap**: no include in this tree.

### 3.7 Parsers: pegtl, lemon, DSNLEXER keyword generator, wxRegEx

- **lemon**: KiCad generates LALR parsers for libeval (`common/libeval/grammar.lemon`) and the
  numeric evaluator. kicad-libeval ports the generated tables and the lemon driver (`lemon.rs`,
  `libeval_compiler/grammar.rs`). Experiment (`parsers/`): 8,211 token sequences (valid and
  invalid, deep nesting) through the C++ lemon parser and Rust alternatives: table-driven port
  identical 8,211/8,211 (tree, error position); pratt parser 41 differing lines, pomelo 0.2.3 71
  (including a stack overflow at depth 99 where lemon reports an error at 100), lalrpop 0.23.2
  495. The table port is the only exact one, including lemon's error-recovery positions.
- **DSNLEXER keyword generator** (`cmake/BuildSteps/TokenList2DsnLexer.cmake`): the generated
  token enums and keyword tables are committed in kicad-sexpr `src/lexer/keywords/*.rs`
  (`pcb_keys_t.rs`, `embedded_files_t.rs`, ...). No build-time generator in Rust.
- **pegtl**: in scope only for the markup grammar `include/markup_parser.h` (sub/superscript,
  overbar, `${...}` variables, URLs; used by text rendering and text-to-polygon conversion,
  Tier 2). Experiment: 11,287 inputs incl. random nesting and malformed braces; hand-written
  recursive descent, pest 2.9.5, winnow 1.0.5, nom 8.0.0, peg 0.8.6 and chumsky 0.13.0 all
  produced trees identical to the C++ on 11,287/11,287 and on the pathological inputs. Timing on
  the pathological input (`~{` repeated, run here): C++ PEGTL 3.5 s for 14 levels and 30.7 s for
  16 (exponential re-evaluation of `not_at< markup >`), hand-written parser 6-9 ms with or
  without memoisation, pest 27-65 ms, winnow 0.97-8.6 s. Recommendation: hand-written recursive
  descent (prototype `parsers/src/markup/hand.rs`, packrat memo on `braces<>` by start offset).
  The library-table PEGTL grammar and CADSTAR parser are out of scope.
- **wxRegEx** (PCRE2 backend in wxWidgets 3.2) for netclass pattern assignments
  (`common/eda_pattern_match.cpp`, `EDA_COMBINED_MATCHER`). Experiment (`cpp/regex_driver.cpp`,
  207 patterns x subject strings): wildcard matcher - all three Rust engines agree on compile
  for 207/207 and differ only for `*`, `a**`, `a??` on 6 of 28,566 cells (a hand port of
  `EDA_PATTERN_MATCH_WILDCARD_ANCHORED` removes even that). Anchored regex: `regex` 1.13.1
  compiles 145/207 the same way and differs on 14/55 realistic patterns; `fancy-regex` 0.20.0
  171/207, 9/55; `regress` 0.12.0 179/207, 2/55. No pure-Rust PCRE2 exists; this is an open
  decision for the orchestrator (recommendation: `regress` for regex patterns with the
  differences listed in `PORT_STATUS.md`; wildcard patterns, the common case, hand-ported).

### 3.8 Threads: thread-pool

`BS::thread_pool` 5.0.0 via `GetKiCadThreadPool()` in 63 files, including `drc_engine.cpp`, most
DRC providers, the zone filler and connectivity. PORTING.md section 5 requires deterministic
sequential ports. `rayon` 1.12.0 builds for wasm32-unknown-unknown (verified build only); on
wasm32 without atomics/threads it cannot spawn workers (behaviour at runtime not verified; real
parallelism needs a `+atomics` build with `wasm-bindgen-rayon` and cross-origin isolation in the
browser). Keep it behind a non-default `parallel` feature, never on the wasm path.

### 3.9 Outline fonts: freetype + harfbuzz + fontconfig (Tier 2)

**Usage** (`common/font/outline_font.cpp`, `outline_decomposer.cpp`):

- `FT_Set_Char_Size( face, 0, faceSize, GLYPH_RESOLUTION = 1152, 0 )` with
  `faceSize = 16 * 64 * 1.4` truncated to 1433 (26.6 pt), i.e. 358.25 ppem; subscript sizes use
  `KiROUND( faceSize * 0.64 )`.
- Shaping: `hb_buffer_add_utf8`, `hb_buffer_guess_segment_properties`,
  `hb_ft_font_create_referenced( face )`, `hb_shape` with no features (lines 348-380); positions in
  26.6 at the FreeType size.
- Outlines: `FT_Load_Glyph( face, gid, FT_LOAD_NO_BITMAP )` (default hinting), optional
  `FT_Outline_Embolden( outline, 1 << 6 )` for fake bold, `FT_Outline_Decompose` with shift 0.
- Fonts are found through fontconfig (`common/font/fontconfig.cpp`) or come from the board's
  embedded files.

**Candidates.** Shaping: `harfrust` 0.15.0 (MIT, HarfBuzz port on read-fonts, exposes
`FontFuncs` and `set_scale`, so hb-ft can be emulated), `rustybuzz` 0.20.1 (MIT, ttf-parser
based). Outlines: `skrifa` 0.49.0 (MIT OR Apache-2.0; glyf/CFF/CFF2 loading, TrueType bytecode
interpreter, CFF hinter, autohinter, written to match FreeType), `ttf-parser` 0.25.1 (unhinted
outlines in font units only), `swash` 0.2.10 (shaping + scaling, older stack; not tested).
Discovery: `fontdb` 0.24.0 (not tested).

**Experiment** (`rust/target/research/report/`): `cpp/font_driver.cpp` (system FreeType, pkg-config
version 26.1.20, and HarfBuzz 8.3.0; built with `g++ -O2 -std=c++17 cpp/font_driver.cpp
$(pkg-config --cflags --libs freetype2 harfbuzz)`; that the oracle links the same libraries is
assumed, not checked) reproduces KiCad's calls
and prints (S) hb-ft shaping at KiCad's size, (U) hb-ot shaping in font units, (G) hinted and (N)
unhinted outlines in 26.6. `src/bin/font_cmp.rs` produces the same with harfrust (S: hb-ft
emulated with a `FontFuncs` implementation returning `(FT_MulDiv(adv, x_scale, 64) + 512) >> 10`
and `set_scale(((x_scale * upem) + 2^15) >> 16)` as `_hb_ft_hb_font_changed` does; U: plain
harfbuzz-style font units), rustybuzz (U) and skrifa (G: `HintingInstance` with
`Engine::Interpreter`, `Target::Smooth{Normal}`; N: unhinted). `cmp_font.py` compares, ignoring
FreeType's explicit closing `line_to` (skrifa's pen gets `close()` instead). 400 random strings
(Latin incl. kerning pairs and ligatures, combining marks, Greek, Cyrillic, Hebrew, Arabic,
symbols), all glyphs up to 6,000 per font.

| Font | upem | FreeType x_scale | S harfrust (hb-ft emulation) | U harfrust | U rustybuzz | G hinted outlines skrifa | N unhinted outlines skrifa |
|---|---|---|---|---|---|---|---|
| DejaVuSans.ttf (hinted TrueType) | 2048 | 733696 | 400/400 | 400/400 | 400/400 | 6,000/6,000 exact | 6,000/6,000 exact |
| LiberationSerif-Regular.ttf (hinted TrueType) | 2048 | 733696 | 400/400 | 400/400 | 400/400 | 2,602/2,602 exact | 2,602/2,602 exact |
| NotoSans-Regular.ttf (qa/resources, unhinted TrueType) | 1000 | 1502609 | 397/400 | 400/400 | 400/400 | 85 exact, 3,102 within 1/64 px, 130 larger | same as G |
| SourceSans3-Regular.otf (CFF) | 1000 | 1502609 | 396/400 | 400/400 | 396/400 | 196 exact, 2,229 within 1/64 px, 53 larger | 44 exact, 2,422 within 1/64 px, 12 larger |

FreeType's hinting is really active in the DejaVu case (hinted != unhinted for 5,937 of 6,000
glyphs) and skrifa's interpreter reproduces it exactly. Findings:

1. Shaping is exact in font units with harfrust (1,600/1,600 strings over 4 fonts); rustybuzz
   misses mark offsets in 4 SourceSans3 strings (e.g. `x_offset/y_offset` 291/152 vs 0/0).
2. The hb-ft emulation is exact except for mark attachment on combining sequences (7 strings,
   e.g. `y_offset` 3910 vs 3897) while the same strings are exact in font units. Probable cause
   (not verified): hb-ft supplies glyph extents and GPOS anchor contour points from the scaled
   FreeType outline, which the emulation does not provide yet (`glyph_extents` and contour-point
   callbacks would be implemented on the skrifa outline).
3. For upem 2048 the FreeType scale is exact in 16.16 and skrifa matches bit for bit. For
   upem 1000 (x_scale 1502609.4 truncated) skrifa's coordinates are off by 1 (and on 130/3,317
   glyphs by more, typically at implied on-curve midpoints) in 26.6 units. `Fixed` division in
   skrifa gives the same 1502609 scale (checked with `src/bin/fixcheck.rs`), so the deviation is
   in skrifa's drawing path (not traced further). At KiCad's scaling one 26.6 unit is about
   40 nm for 1 mm text, which is visible in Gerber coordinates. Fix: hand-port FreeType's
   TrueType load-and-scale (`FT_MulFix` per point, phantom points, midpoint computation in
   `FT_Outline_Decompose`) on top of the raw glyf points from read-fonts for unhinted glyphs, or
   patch skrifa; CFF needs FreeType's CFF scaling (harder; not investigated).
4. Still to hand-port in any case: FreeType's size request (`FT_Set_Char_Size` -> x_scale,
   integer-ppem rounding when head flag bit 3 is set), `FT_Outline_Decompose`'s closing
   `line_to` and start-point handling, `FT_Outline_Embolden` (fake bold), hb-ft's nominal-glyph
   and symbol-charmap handling (`OUTLINE_FONT::SelectCharmap`).
5. fontconfig substitution cannot be reproduced on wasm; the port takes a caller-supplied map
   from font name to bytes (embedded fonts first, then caller fonts, then KiCad's fallback for a
   missing font, to be taken from `common/font/font.cpp`). Any substitution difference changes the output; document
   it as a deviation.

**Decision.** harfrust 0.15.0 + skrifa 0.49.0 (both MSRV 1.85, shared read-fonts 0.46.0 stack,
verified for wasm32 and run in node), with the hand ports listed above in kicad-font. Fidelity
is exact for hinted upem-2048 TrueType fonts in this experiment and within 1/64 px otherwise
until item 3 is done. rustybuzz, ttf-parser and swash are not needed.

## 4. WebAssembly toolchain

Verified on this machine (rustc/cargo 1.97.0, target `wasm32-unknown-unknown` installed, node
22.23.4 at `/opt/node22/bin/node`).

**Whole crate set builds for wasm32.** `rust/target/research/report/` (lib touching serde_json
with `preserve_order`+`float_roundtrip`, flate2 rust_backend, ruzstd, sha2, base64, strum,
harfrust, rustybuzz, skrifa, ttf-parser; 71 crates in the tree):

```
cd rust/target/research/report
cargo build --target wasm32-unknown-unknown        # Finished, no warnings
```

**wasm-bindgen.** The CLI version must equal the `wasm-bindgen` crate version exactly; pin the
crate with `=`.

```
cargo install wasm-bindgen-cli --version 0.2.130 --locked      # 1 min 24 s; installs
                                                               # wasm-bindgen, wasm-bindgen-test-runner, wasm2es6js
cd rust/target/research/report/wasm-hello
cargo build --release --target wasm32-unknown-unknown          # cdylib, opt-level "s", lto
wasm-bindgen --target nodejs --out-dir pkg-node target/wasm32-unknown-unknown/release/wasm_hello.wasm
wasm-bindgen --target web    --out-dir pkg-web  target/wasm32-unknown-unknown/release/wasm_hello.wasm
/opt/node22/bin/node run.cjs <font.ttf>                        # CommonJS, pkg-node
/opt/node22/bin/node run_web.mjs                               # ES module, pkg-web via initSync()
```

`wasm-hello/Cargo.toml` dependencies (all resolved and working):

| Crate | Version | Notes |
|---|---|---|
| wasm-bindgen | =0.2.130 | `#[wasm_bindgen]` exports, `#[wasm_bindgen(start)]` |
| js-sys | 0.3.107 | `js_sys::Date::now()` |
| web-sys | 0.3.107, feature `console` | `console::log_1` (works in node and browsers) |
| console_error_panic_hook | 0.1.7 | `set_once()` in the start function |
| serde-wasm-bindgen | 0.6.5 | `to_value(&struct)` returns a plain JS object |
| getrandom | 0.4.3, feature `wasm_js` | builds and `getrandom::fill` succeeds in node; no extra `--cfg` flag was needed. Library crates must not depend on it (PORTING.md section 5); only kicad-wasm, if at all |

Output: `{"greeting":"hello kicad","sum":6,"random_ok":true,"sin1":0.8414709848078965}`, and the
`smoke()` export ran serde_json pretty printing with a 4-space `PrettyFormatter`, a gzip round
trip, a ruzstd compress/decompress round trip, SHA-256, base64, strum, harfrust and rustybuzz
shaping and a skrifa hinted outline inside WebAssembly with the expected results. The release
`.wasm` is 3,410,658 bytes before and 2,915,309 bytes after wasm-bindgen (no wasm-opt).

**wasm-opt.** Not installed. Options: `apt-get install binaryen` (candidate version 108-1, old;
whether it accepts the features rustc 1.97 enables by default for wasm32 - bulk memory, sign
extension, multi-value, reference types - is not verified; it may need
`--enable-bulk-memory --enable-sign-ext ...` or fail), the `wasm-opt` crate 0.116.1 (`cargo install
wasm-opt`, builds binaryen 116 from C++ sources; not tried), or a binaryen release binary.
Optional for the project; size optimisation only.

**Other wasm notes.**

- libm: on wasm32 Rust's bundled libm is used; one-ulp differences to glibc are an accepted,
  documented deviation (PORTING.md section 9; numbers in 3.1).
- `bitvec` with `u64` storage does not compile for wasm32 (3.2); `kiddo` needs MSRV 1.89.
- No crate recommended here needs `getrandom`, threads, the filesystem or a clock; fontdb's
  system-font loading and rayon's thread pool are the parts that do not work on
  wasm32-unknown-unknown.
