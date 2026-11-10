# Review 2: port decisions of kicad-pcb (package INT-2b)

Reviewer pass over `rust/crates/kicad-pcb` (board model, board file reader / writer, project
settings) after integration step 2. Yardstick: can the entry change a board re-save, a DRC
result, a Gerber / drill file or connectivity. The connectivity, DRC, zone and fabrication
crates are out of scope except where a call was followed.

State: 5 findings fixed with a regression case against the reference, 1 report answered
without a change, 10 findings open (1 with a FIX package, 4 for owners of running packages /
other crates, 5 documented differences that need no work now). Severity counts over all 16:
1 blocker (fixed), 4 major (fixed), 11 minor.

## 1. Method and evidence

1. `PORT_STATUS.md` of the crate: every "Deviations" / notes entry of every package section
   (238 entries in 50 sections) read against the C++ it names; verdicts in section 5.
2. Whole-corpus differential runs that did not exist before (each package had verified its own
   slice on a few boards):
   * **D3, legacy input** (`tests/int2b_review.rs` `d3_legacy_scoreboard`, ignored test): Rust
     parse + format of the *original* corpus file against O1 (reference `kicad-cli pcb upgrade
     --force` of the same file), UUIDs canonicalised with `kicad_qa::uuid_canon`. The PCBIO-17
     scoreboard reads O1, which is always in the newest format; D3 is the only run through the
     legacy branches of the parser. Result: **361 of 370 identical**. The 9 others, all
     explained: 4 outline-font render caches (R2-06), 2 `net_chains` terminal pads and 2 zone
     net / priority (post-load steps of the reference: `NETINFO_LIST::buildListOfNets` clears
     the terminal pad pointers, teardrop zones; the model ports of those steps are faithful,
     they run in the loader, not in parse + format), 1 drill chart cell text (R2-12).
   * **Item level** (`pcb-dump board --no-init --hash` against `pcb-dump-rs board --hash`
     through `tools/pcb-dump/compare.py`, every section, one reference process at a time): 317
     corpus boards under 400 KB (3.0 million values) and 30 boards of 400 KB .. 1.5 MB
     (4.3 million values). Before the fixes: 873 mismatching values on 45 boards. Classes:
     back-side pad margins (R2-03, 12 boards), cells of a footprint table (R2-04, 389 values
     on one board), `lib.poly` (R2-08, a dump artifact, about 30 boards), and the known gaps
     (barcode symbol, outline fonts, generated tables, UUID-less items). After the fixes no
     other class is left. `rust-error` lines: 0.
   * Targeted reference runs for the reports of section 2 and for text variables (R2-01,
     R2-02), commands in section 6.
3. Code searches for the cross-cutting traps, section 4.
4. Side-by-side reading of the functions the runs above cannot reach (text variables,
   differential-pair suffix, net list rebuild, outline build order sets).

Not covered: mutators that no load path runs (interactive `Move` / `Rotate` / `Flip` of
tracks, vias, shapes; INT-2a's oracle covers text, text box, dimension and zone), hit tests,
`GetItemDescription` texts, and the files of the running packages (footprint geometry /
compare / shapes, courtyard index, properties, barcode, `fp_lib`), which were only read where
a finding led there.

## 2. Reports from other packages

| report | answer |
|---|---|
| DRC-24: render cache of a `gr_text` gone after the parse (issue22475, text ff749b73) | **Correct, no change (R2-06).** `parsePCB_TEXT` ends with `SetVisible( true )` for board text (parser.cpp:4656) and `EDA_TEXT::SetVisible` calls `ClearRenderCache()` (eda_text.cpp:346); footprint text loses it through `SetLibTextSize` / `SetTextAngle`. The reference never uses the stored glyphs: its re-save of issue22475 writes other polygons than the file holds (regenerated from the substitute font, "Font 'Franklin Gothic Medium' not found; substituting 'Inter Regular'"), and for a crafted board whose `gr_text` and `gr_text_box` carry a marker triangle as render cache, both `kicad-cli pcb upgrade` and `pcb-dump --no-init` (text polygons, effective shape) show regenerated glyphs and no triangle. The port does the same up to the missing outline font engine (stroke font instead of the host font). `io::parser::text::tests::board_text_drops_render_cache` / `footprint_text_drops_render_cache` pin the parse behaviour. Decision left to the orchestrator: drawing outline-font text from the *stored* glyphs would be closer to the designer's geometry than the stroke font, but it is not what the reference does either; it only makes sense as the documented stand-in until `kicad-font` has an outline engine. |
| DRC-06: the default-seeded `KiidGenerator` reproduces UUIDs stored in boards | **Fixed (R2-05).** Rule: *a generator never returns a reserved UUID; the board file parser reserves every UUID-shaped token of the file before the first draw.* Draw points are unchanged, so a seeded run stays reproducible; with nothing reserved the sequence is the C++ one. |
| DRC-24: `Board::is_layer_visible` | Present (`board/queries.rs`, added by a concurrent package together with `visible_layers()` reading the project's `.kicad_prl` mask; checked against `pcb-dump --loader` with a `.kicad_prl` hiding F.Cu: `visible_layers` and the board boxes agree). |
| DRC-27: fill writer of a detached zone | Present: `Zone::fill_mut()` (`zone/mod.rs:435`). |

## 3. Findings

Severity: blocker = wrong fabrication / DRC output on ordinary boards; major = wrong output for
a class of boards; minor = corner input, no output reached on the corpus, or a tooling issue.

| id | sev. | package | file:line | C++ reference | what differs, for which input | status |
|---|---|---|---|---|---|---|
| R2-01 | major | M22 / SET-5 | `board/text_vars.rs:81`, `:424` | `BOARD::ResolveTextVar` (board.cpp:753), `PROJECT::TextVarResolver` (project.cpp:178) | `${PROJECTNAME}` was the board file stem and `${CURRENT_DATE}` / `${CURRENT_TIME_*}` always empty: the board used a private resolver, `BoardProject::project_name` and `Board::text_var_clock` (both added by SET-5) were never read. QA load path with `<name>.kicad_pro`: reference "P=pn", port "P=" (shown text, so text geometry). | fixed: the board calls `BoardProject::text_var_resolver` with the board clock; unit test with the reference values |
| R2-02 | major | SET-5 | `project/project.rs:269` | `PROJECT_GIT_UTILS::GetCurrentHash` (project_git_utils.cpp:114) | `${VCSHASH}` / `${VCSSHORTHASH}` resolved to "" ; the reference gives "no hash" for a project outside a repository (`pcb-dump`: "H=no hash"). Every text using the variable was 7 characters shorter. | fixed ("no hash"; a real hash stays a host matter); unit test |
| R2-03 | blocker | M14 | `pad/derive.rs:1452`, `:1463`, `:1474` | `PAD::SetLocalSolderMaskMargin`, `SetLocalSolderPasteMargin`, `SetLocalSolderPasteMarginRatio` (pad.h:609-630) | The setters wrote the front outer layer only; C++ writes `F_Mask` and `B_Mask` (`F_Paste`, `B_Paste`). Every pad with a local mask / paste margin had no back margin: `GetSolderMaskExpansion( B_Mask )`, `GetSolderPasteMargin( B_Paste )`, the B.Mask shapes (KiCAD_Fiducial: 1000000 vs 0). 12 corpus boards. Not visible in the re-save (the writer prints the front value). | fixed; golden replay `int2b_matches_oracle`. FIX-PAD-1 names the same defect; its remaining items (other two-sided accessors, primitive parents) stay with that package |
| R2-04 | major | PCBIO-05 / PCBIO-12 | `io/parser/text.rs:693` | `bakeTextBoxLib` (parser.cpp:4981: `GetParentFootprint()`) | The cells of a table inside a footprint stayed at their library coordinates: the parser tested "parent is a footprint", a cell's parent is its table. Position, box, shape, text position of every such cell and the table / footprint boxes (embedded_table_rotated_legacy_v10: 389 values). | fixed (`env.parent`, the frame found through the parent chain); golden replay |
| R2-05 | major | kernel / PCBIO-01 | `kicad-core/src/kiid.rs:628`, `io/parser/mod.rs:295` | `KIID::KIID()` (entropy-seeded generator) | A fresh UUID could equal a UUID of the file (boards written by a KiCad whose generator started from the `boost::mt19937` default seed, e.g. issue10697): duplicate UUIDs, `ResolveItem` returning the wrong item, a changed save order. | fixed: `KiidGenerator::reserve_all`, the parser reserves the file's UUIDs; tests in both crates (the kicad-pcb test fails with "duplicate UUID 5cbb91d0-..." without the reservation) |
| R2-06 | minor | PCBIO-05 / M10 | `io/parser/text.rs` (`parse_pcb_text`), `eda_text.rs:827` | `EDA_TEXT::SetVisible` -> `ClearRenderCache` | Stored render caches are dropped at load exactly as in C++ (section 2). What differs is the font the glyphs are regenerated from (host font vs stroke font): 4 corpus boards differ in `(render_cache ..)` and in the text shapes (issue14549: 38 values). | ok-documented-difference; open as the outline font gap of `kicad-font` |
| R2-07 | minor | M21 | `convert_shape_list_to_polygon.rs` (`process_shape_segment`) | `RebuildBezierToSegmentsPointsList( aErrorMax )` (convert_shape_list_to_polygon.cpp:365) | C++ leaves the re-approximated points in the board item; later `GetBoundingBox` (eda_shape.cpp:1741), `GetLength`, hit tests of an Edge.Cuts Bezier read them. Differs only when the project's `max_error` is not the 0.005 mm the points were parsed with (10 corpus projects, none with an Edge.Cuts Bezier): the Bezier box, hence board-edge boxes, by up to the approximation difference. | open: FIX-BEZ-1 |
| R2-08 | minor | DUMP-RS | `dump/items.rs:862` | `rust/tools/pcb-dump/dump_items.cpp:528` (`GetLibraryPolyShape()`) | The dump prints `lib_poly_shape()` (the stored `m_libPoly`, which keeps the closing point of the file) where the C++ tool prints `GetLibraryPolyShape()` (board polygon mapped back): about 30 boards show `lib.poly` mismatches that are not model differences. Use `library_poly_shape(ctx)`. | open: change request (the dump files are being edited by another package) |
| R2-09 | minor | M11 / PCBIO-12 | `pcb_textbox.rs` (`on_footprint_transformed`) | `PCB_TEXTBOX::OnFootprintTransformed` (pcb_textbox.cpp) | A polygon-shaped cell (table in a rotated footprint) keeps the constructor rectangle in `lib.start` / `lib.end`; the reference holds that rectangle pushed through the footprint transform (16 values on one board). The rectangle is not read while the shape is a polygon. | open, unproven (no reader found); leave unless a reader appears |
| R2-10 | minor | PCBIO-08 / PCBIO-03 | `io/writer/mod.rs` (`sorted_set`) | `BOARD_ITEM::ptr_cmp`, `FOOTPRINT::cmp_drawings` | Items without a UUID in the file (teardrop zones, legacy drawings) get generated UUIDs, which are a sort key: `order|zones` of issue25580 and one footprint's drawing order in issue4139 differ from the reference run. The reference order is itself random from run to run. | ok-documented-difference (compare such boards with `canonicalise_uuids_sorted`) |
| R2-11 | minor | kicad-drc, kicad-fab | `kicad-drc` harness, `kicad-fab/src/drill/drill_enumerator.rs:435` | - | Code outside kicad-pcb draws UUIDs from private default-seeded generators (the DRC harness works around collisions with its own seed). With R2-05 the board's generator is safe after a load: draw from `board.kiid_generator`. | open: change request |
| R2-12 | minor | PCBIO-12 / M26 | `pcb_generated_table.rs` | `PCB_DRILL_CHART` refresh on load | Generated tables are not refreshed: the reference re-save of api_kitchen_sink rewrites the drill chart cells ("0.30 mm", port keeps "0.30" of the file), and the item-level dump shows 649 lines and an item count that differ. | open: belongs to the generated table package (M26) |
| R2-13 | minor | M25a | `pcb_barcode.rs` | zint | No symbol encoder: bounding boxes of non-empty barcodes (issue24525, api_kitchen_sink). Recorded by the package. | open (known gap) |
| R2-14 | minor | PCBIO-10 / PCBIO-06 / SET-4 | `io/parser/dimension.rs`, `io/parser/footprint.rs` | `static_cast` of file integers to enums | Out-of-range enum integers (`units_format 3`, `precision > 9`, `text_position_mode 3`, `zone_connect N`, `pad_connection`) are normalised; the C++ writes the raw integer back. Changes a re-save of such a file only; none in the corpus, KiCad never writes them. | ok-documented-difference |
| R2-15 | minor | M23 | `pcb_generator.rs` (`TuningPatternData`) | `PCB_TUNING_PATTERN::SetProperties` (pcb_tuning_pattern.cpp:2011, :2031) | `val` / `int_val` are read uninitialised when `target_length` / `target_skew` are missing; the port starts them at 0. The carry-over between keys is reproduced (golden of 35 generators). | unproven: no corpus pattern lacks the first key; an oracle case would only record a stack value |
| R2-16 | minor | M7 | `eda_shape/geometry.rs` (hit tests) | `EDA_SHAPE::hitTest` | Out-of-range `double -> int` of the hit-test distance uses the saturating cast; reachable only with accuracies near `INT_MAX`. | ok-documented-difference |

## 4. Cross-cutting traps

* **Iteration order.** Every `unordered_map` / `unordered_set` / pointer-keyed `std::set` /
  `std::map` of the ported C++ files was listed (`grep` over board, footprint, pad, zone,
  zone_utils, pcb_group, pcb_track, netinfo, net_settings, component classes, outline builder,
  parser, writer, table) and its use read. Iterated with an observable order: the writer sets
  (ported `_Rb_tree`), `openShapes` of `TestBoardOutlinesGraphicItems` (documented, 2 fixtures),
  `GetEffectiveNetClass` (documented), the groups sanity check (documented), group members
  (documented), `m_copperProps` (reproduced). Lookup or membership only: `remaining` and
  `reportedGaps` of `doConvertOutlineToPolygon` (the C++ seeds in list order on purpose),
  `mergedFills`, the `AutoAssignZonePriorities` maps, `CollectMicroviaColumns` sets, the
  `preserved*` maps of `buildListOfNets`, `ptrMap`, the table flip maps. Nothing missing from
  `board-model.md` 3.1 / 10.1. No `HashMap` / `HashSet` of the crate is iterated.
* **Numbers.** `as i32` on floating values outside tests: all sites read; each is a C++
  truncation (`UpdateHeight`, `GEOMETRY_MIN_SIZE`, `FAB_LAYER_COLOR`, cell index `floor`) or an
  `i64 -> i32` narrowing that wraps in both languages. No missing `KiROUND`. The item-level
  runs compare every derived coordinate of 347 boards bit for bit.
* **`unwrap_or` / early returns where C++ dereferences.** The entries of the status file were
  judged individually (null dereference, `at()` throw, `wxCHECK` return); none is reachable on
  a board the reference loads. Not searched exhaustively in the code.
* **Side-effect state.** R2-07 (Bezier points). The hatching cache (M8) and the footprint
  time-stamp caches (M19) were judged from the C++ call graph: a stale C++ cache is only read
  after an edit without regeneration, which no CLI flow does. The pad shape caches are keyed
  by `max_error` in the port and built once in C++; the first build happens after the project
  is bound in both load sequences.
* **Strings.** `MatchDpSuffix`, the text variable resolvers and the net list rebuild were read
  statement by statement (R2-01, R2-02 came from there). `KIID( string )` for a reference that
  is not a UUID draws a random id in C++ and resolves nothing; the port's `Kiid::parse` gives
  the same result without the draw.
* **Defaults.** The defaults scoreboard covers what `pcb-dump defaults` prints. R2-03 is the
  class it cannot see: an inline header setter with two writes. `grep` for the other two-sided
  accessors of `pad.h` / `padstack.h` / `pcb_track.h` / `footprint.h` found only these three.

## 5. Status file entries: verdicts per package

`ok` = equivalent for every input (by reading, or exercised bit for bit by the corpus runs of
section 1); `doc` = ok-documented-difference (cannot reach an output, or only for input the
reference rejects / never writes); `wrong` = fixed here or by the named package; `unproven` =
listed in section 3. Superseded entries (INT-2a) count as `ok`.

| package | entries | ok | doc | wrong | unproven | found outside the entries |
|---|---:|---:|---:|---:|---:|---|
| crate-wide | 8 | 4 | 4 | 0 | 0 | R2-05 |
| M0b, M0c, M0d, M0e | 27 | 25 | 2 | 0 | 0 | |
| M1, M2, M3 | 17 | 11 | 6 | 0 | 0 | |
| M4, M5, M6 | 14 | 11 | 3 | 0 | 0 | |
| M7, M8, M9 | 15 | 10 | 5 | 0 | 0 | |
| M10, M11, M12 | 19 | 16 | 2 | 0 | 1 | R2-04, R2-09 |
| M13, M14, M15 | 13 | 13 | 0 | 0 | 0 | R2-03 |
| M16, M17, M18 | 11 | 11 | 0 | 0 | 0 | |
| M19, M20 | 11 | 9 | 2 | 0 | 0 | |
| M21 | 5 | 3 | 1 | 0 | 1 | |
| M22 | 7 | 3 | 2 | 2 | 0 | |
| M23, M24, M25a | 16 | 8 | 7 | 0 | 1 | |
| PCBIO-01 .. PCBIO-05 | 19 | 15 | 4 | 0 | 0 | |
| PCBIO-06 .. PCBIO-13, PCBIO-17 | 30 | 24 | 6 | 0 | 0 | R2-12 |
| SET-3, SET-4, SET-5 | 12 | 8 | 3 | 1 | 0 | |
| GLUE-1, PROP-1, CC-1 | 14 | 10 | 4 | 0 | 0 | R2-08 |

238 entries: 181 ok, 51 doc, 3 wrong, 3 unproven.

* `wrong`: M22 "every layer is visible" (`BOARD::GetVisibleLayers`; fixed by a concurrent
  package once SET-5 had loaded the `.kicad_prl`), M22 "PROJECTNAME is the board file name,
  VCSHASH is empty, no clock" (R2-01, R2-02), SET-5 "VCSHASH resolves to an empty string"
  (R2-02).
* `unproven`: M12 "`Update()` after the context-free mutators runs against an empty board and
  is skipped for footprint children" (known gap of INT-2a, no load path reaches it), M21
  Bezier points (R2-07), M23 uninitialised locals (R2-15).
* The blocker and one major finding (R2-03, R2-04) were not behind any entry: both packages
  had verified on boards without the case. They came out of the corpus runs.
* The entries of the running packages (M20, M25a, PROP-1, PCBIO-06, PCBIO-07, CC-1) were
  judged from the text and the corpus runs only; their code was not read.

## 6. Reproducing the evidence

```
# D3 (needs the oracle cache of tools/io-corpus/run.sh); result in target/int2b/d3.md
cargo test -p kicad-pcb --test int2b_review -- --ignored d3_legacy_scoreboard
D3_BOARD=issue25065 cargo test -p kicad-pcb --test int2b_review -- --ignored d3_legacy_scoreboard

# item level, one board (scratch copy; add --hash for boards over 100 KB)
pcb-dump board --no-init <copy> > ref.txt; pcb-dump-rs board <copy> > rs.txt
python3 -I rust/tools/pcb-dump/compare.py ref.txt rs.txt --show 20

# regression goldens of the fixes
rust/crates/kicad-pcb/tests/oracle/int2b_golden.sh
cargo test -p kicad-pcb --features dump --test int2b_review
```

Text variables (R2-01, R2-02): a board with the text
`P=${PROJECTNAME} F=${FILENAME} FP=${FILEPATH} V=${VARIANT} T=${TITLE} D=${CURRENT_DATE} H=${VCSHASH} X=${NOPE}`
and a `.kicad_pro` with `"text_variables": {"NOPE": "yes"}`; `pcb-dump board --sections text`
(QA path) prints `shown_text="P=pn F= FP= V= T= D=2026-11-10 H=no hash X=yes"`, with
`--loader` `"P=pn F=pn.kicad_pcb FP=pn.kicad_pcb ..."`. The port prints the same except the
date (host clock, `Board::text_var_clock`; `pcb-dump-rs` does not set it).
