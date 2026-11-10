# GLUE-2: Second wiring pass (change requests of the packages that landed since GLUE-1)

Crate: kicad-pcb (files of finished packages), plus one-line call-site switches in kicad-drc,
kicad-fab, kicad-connectivity and kicad-session where a request says so

Like GLUE-1: finished packages asked for changes in files they did not own. Their requests
are collected in `rust/target/orchestrator/change_requests_2.txt` (one `=== <package>` block
each). An integration step (INT-2a, see rust/docs/design/integration-2.md and the
"Integration step 2" section of kicad-pcb/PORT_STATUS.md) has just handled the kernel-level
requests of the earlier packages and may already cover some of these: triage every request
first (`done` where / `apply` / `defer` to whom / `reject` why) and append the table to
rust/docs/design/integration-2.md under a "GLUE-2" heading.

Known items (verify each against the C++ before changing anything - the request text is a
hint, the C++ is the specification):
- `pcb_text::footprint_resolve_text_var` (and `pcb_field::footprint_field_value_for_variant`
  if still a stub): forward to M19's `Footprint::resolve_text_var[_for_variant]` /
  `field_value_for_variant`. This stub alone blocks 151 corpus boards in the DRC replays.
- Stale staging-stub tests: io/parser/tests.rs `staging_stubs` (expects footprints pending),
  io/writer/tests.rs `staging_stubs` (expects the footprint writer to fail),
  io/parser/tests.rs `oracle_board_section` (stitch_fp_rulearea net name while pads are
  skipped), netclass_embedded `oracle_corpus_sections` (footprint-level embedded files must
  be merged in write_sections as format(BOARD*) does - issue25265), the `pending.rs` doctest,
  footprint/geometry.rs tests using `crate::dump` without `cfg(feature = "dump")`.
- pcbio12 `oracle_corpus_blocks`: api_kitchen_sink drill_chart cells read '0.30' where the
  reference has '0.30 mm' (generated-table code): find the cause in the generated table /
  unit formatting and fix it.
- Barcode: io/writer/misc_items.rs writes (text_height) from the board-frame
  `b.text_size(ctx)`; io/parser/misc_items.rs uses the M25a setters (`set_text_size(env, h)`,
  `set_kind`, `set_error_correction`, `set_orientation`); board/compare.rs
  `pcb_barcode_compare` calls `PcbBarcode::compare`; dump/defaults.rs barcode fields and the
  kind names CODE_39 / CODE_128.
- Dump (src/dump): zone section `outline` = the stored frame (`z.outline()`), `hatch_lines` =
  the board-frame GetHatchLines(); FOOTPRINT::GetFpPadsLocalBbox, area, GetSide,
  GetLikelyAttribute, IsExcludedFromSim through the M19 / M20 methods; a
  `dump_board_ctx(ctx, opts)` entry so a caller can dump with connectivity and a rule
  resolver (kicad-session's pcb-dump-session then drops its work-arounds).
- board_connected_item.rs `net_class_name()`: C++ GetEffectiveNetClass()->GetName() (it
  returns the composite's `name` field today: 'Effective for net: /L' instead of
  'L,Default'); check every caller.
- `Board::uuid` (BOARD::m_Uuid: where the C++ creates it and what value a loaded board has)
  and the board as a resolvable report item (description 'PCB', position (0,0)): DRC-23's
  outline entries and PROP-1's 'Parent' need it. `Board::is_layer_visible`. A public writer
  for the fill of a detached zone (DRC-27). `convert_shape_list_to_polygon::collect_board_shapes`
  public. `PCB_VIA_STACK::IsSpanValid` in pcb_generator.rs (DRC-28 has a local copy).
  `Pad::delete_primitives_list` / `add_primitive_poly` (FAB-04 has local ports).
  `PcbShape::connection_points` if INT-2a did not add it.
- parse_pcb_shape: a footprint fp_poly's library polygon keeps the duplicated closing vertex
  the C++ drops (strip_microwave 1084 vs 1083 points, component_classes 4 vs 3).
- `Board::footprint_add` of a PCB_POINT: FOOTPRINT::Add -> PCB_POINT::SetParent recomputes
  the board position; the parser works around it with `on_footprint_transformed`.
- Board::ComputeBoundingBox( false ) on qa/data/pcbnew/issue24525 gives
  0,0,248866684,116025000 where C++ gives 99975000,29981232,148891684,86043768 (it may have
  been an item with an empty box at the origin - barcodes landed since): find and fix.
- CLEAN-1 switches: netclass.rs local `wx_string_matches` -> `kicad_core::string_utils`
  (also kicad-drc engine/implicit_condition.rs), foundation_stub.rs `FontRef` ->
  `kicad_font::FontRef`, pcb_reference_image.rs `mirror_point_wrapping` ->
  `kicad_geom::mirror_point`, kicad-fab plot_brditems_plotter.rs / plot_board_layers.rs to
  the default round-rect generator, kicad-drc pcbexpr `to_core_units`.
- (zone_connect N) with N outside -1..3: the C++ keeps and writes back the raw integer;
  the port reads INHERITED. Make the model keep the raw value if that is what the C++ does
  for a re-save (check with the reference on a crafted board).
- Unused-import / clippy / rustfmt leftovers the reports name in finished packages' files.

Not yours: COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments (package CC-1 is
doing it), the pad parser / writer (PCBIO-07: io/parser/pad.rs, io/writer/pad.rs), kicad-drc
provider files of running packages, kicad-zone. A reviewer (INT-2b) is reading and
occasionally fixing kicad-pcb files at the same time: re-read a file right before you edit
it, edit with exact replacements, keep the crate compiling.

Verification: `cargo test -p kicad-pcb --features dump` (all lib tests and integration tests
green, or each remaining failure explained with its owner); `cargo check` of every dependent
crate; clippy; check_item_shims.py; the defaults scoreboard and the io-corpus scoreboard
(tools/io-corpus/run.sh) before and after - no count gets worse; for every behaviour change
a differential case against the reference (pcb-dump or kicad-cli re-save).

## Left over from INT-2a (read its notes in docs/design/integration-2.md and the
"Integration step 2" section of kicad-pcb/PORT_STATUS.md first: several signatures changed)

- `PcbBarcode::style_from_settings` takes the env the trait now passes (M25a is finished);
  remove `Pad::is_free_pad_ctx` after switching kicad-drc providers/solder_mask.rs to
  `Pad::is_free_pad(ctx)`; io/parser/text.rs local `next_field_ordinal` -> the M19 method;
  rustfmt of dump/mod.rs; clippy collapsible-match at io/writer/pad.rs:568 only if PCBIO-07
  has finished (check that nobody is editing the file: its PORT_STATUS section exists);
  kicad-zone tests/tmp_dbg.rs is somebody's temporary file - leave it.
- tests/pcbio12_misc_items.rs `oracle_corpus_blocks` needs the drill_chart / stackup_table
  skip that `oracle_resave_blocks` has, IF the cause of the '0.30' vs '0.30 mm' cells is the
  reference's RefreshGeneratedTables on load (generated tables are not refreshed in this
  port, board-model.md 17) rather than a formatting bug: decide with the reference.
- kicad-fab/src/drill/via_access.rs -> the M17 `PcbVia` methods; kicad-plot `set_viewport`
  `debug_assert!(!a_mirror)` must go (the C++ wxASSERT only logs).
