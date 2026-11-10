# FAB-GLUE: Drill enumeration on the real via model, and the fabrication crate's leftovers

Crate: kicad-fab (all files: every fabrication port package FAB-01 .. FAB-09 is finished;
read PORT_STATUS.md), plus the small kicad-pcb additions named below

1. FIRST and most important - drill export for boards with vias does not work end to end:
   the drill enumerator (src/drill/drill_enumerator.rs, package FAB-01) still calls the
   placeholders of src/drill/via_access.rs (written before the via model existed), so every
   board with vias stops there: 309 of the Gerber-drill corpus runs and the ignored
   end-to-end Excellon goldens are pending. The via model (kicad-pcb src/pcb_track/via.rs,
   package M17) now has `PcbVia::drill_value(ctx)`, `layer_pair(..)`, and the
   `resolve_filling_mode / capping / front|back covering / plugging / tenting` methods.
   Switch the enumerator to them, delete via_access.rs, and verify against the reference:
   `kicad-cli pcb export drill` Excellon (default, --excellon-separate-th,
   --excellon-oval-format alternate, -u in, --excellon-zeros-format each,
   --drill-origin plot, --excellon-min-header, --excellon-mirror-y, --generate-report) and
   `--format gerber` (default, --generate-tenting, --gerber-precision 5) on EVERY corpus
   board under 300 KB with holes through the real path (parser, enumeration, drill job;
   tests/gerber_drill.rs, tests/excellon_oracle.rs and the FAB-01 drill goldens have the
   harness and the cached reference: un-ignore what was waiting) - files byte-identical
   after the date / version normalisation, same names, same order, same report text.
   Backdrill, blind / buried / micro vias, tertiary drills, post-machining, via protection
   files (filling, capping, covering, plugging, tenting) must be covered by a crafted
   board if the corpus has no case.
   The drill enumerator must not draw UUIDs from `KiidGenerator::default()` (review
   finding R2-11, rust/docs/design/review-2.md): use the board's generator as the C++
   does, or prove no UUID is needed.
2. Local ports to replace by the model: plot_board_layers.rs `pad_delete_primitives_list` /
   `pad_add_primitive_poly` -> `Pad::delete_primitives_list` / `Pad::add_primitive_poly`
   (called on `&mut Pad` with the pad env); plot_brditems_plotter.rs ->
   `flash_pad_chamfer_round_rect_default`, remove the duplicate
   `round_chamfered_rect_to_polygon` and the `set_round_rect_polygon_fn` call; dimension
   text knockout / strike-out through the new `PcbDimension::transform_text_to_poly_set`
   (a pending panic today); private `expand_project_text_vars` copies in pcbplot.rs and
   exporters/gerber_jobfile_writer.rs -> one public function in kicad-pcb
   (board/text_vars.rs: make the PROJECT text-var resolver reachable; small additive edit
   in kicad-pcb, re-read the file first: other agents work in that crate);
   `Board::pad_with_castellated_attr_count` / `pad_with_press_fit_attr_count` and
   `Board::drill_symbol_layers` in kicad-pcb in place of the local helpers (additive).
3. Tests that carry stale expectations: KNOWN_DIFFERENCES in tests/plot_driver_corpus.rs and
   tests/plot_graphics_corpus.rs (the three B.Mask files were the missing rule resolver:
   run these corpus tests with the DRC engine as `RuleResolver` and connectivity, the way
   the session does - kicad-drc is a dev-dependency candidate; the table-cell board
   embedded_table_rotated_legacy_v10 matches since the model fix), the FAB-07 PORT_STATUS
   note about NOT_YET_PORTED, plot_pads_qa.rs module doc. Rerun every corpus replay and
   report the counts before and after.
4. `cargo clippy -p kicad-fab --all-targets`, rustfmt, the wasm32 build.

Other agents: kicad-pcb has running packages (FONT-RC-1 in the text files and in
plot_brditems_text.rs of THIS crate - do not edit that file; M26 generated tables; ZINT-1
barcode encoder): re-read a kicad-pcb file right before you edit it.
