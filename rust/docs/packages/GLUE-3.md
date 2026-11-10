# GLUE-3: Cross-crate leftovers (third wiring pass)

Crate: kicad-drc, kicad-fab, kicad-zone, kicad-session (call-site switches) and kicad-pcb
(the few model additions they need)

Run when no port package is editing kicad-drc / kicad-fab / kicad-zone any more. Collected
from the reports of GLUE-2, INT-2b (rust/docs/design/review-2.md), FIX-PAD-1, CLEAN-1,
DRC-23..28, FAB-04..06, ZF-03, PCBIO-15. Triage first (some may be done), then apply:

- kicad-fab: plot_board_layers.rs local `pad_delete_primitives_list` /
  `pad_add_primitive_poly` -> `Pad::delete_primitives_list` / `Pad::add_primitive_poly`;
  plot_brditems_plotter.rs -> `flash_pad_chamfer_round_rect_default`, remove the duplicate
  `round_chamfered_rect_to_polygon` and the `set_round_rect_polygon_fn` call;
  drill/via_access.rs -> the `PcbVia` methods; drill/drill_enumerator.rs must not draw UUIDs
  from `KiidGenerator::default()` (review R2-11: use the board's generator);
  tests: drop KNOWN_DIFFERENCES entries that the pad-margin and table-cell fixes resolved
  (rerun the plot corpus and report the new counts).
- kicad-drc: local copies -> model functions (`Board::is_layer_visible`,
  `ViaStackData::is_span_valid`, `collect_board_shapes`, `Zone::fill_mut`,
  `Pad::is_free_pad(ctx)`), `to_core_units` identity, `wild_compare_string` /
  `wx_string_matches` from kicad_core; marker and exclusion UUIDs from the board's generator
  (R2-11); `Board::uuid` (BOARD::m_Uuid) in kicad-pcb and the board as a resolvable report
  item (description 'PCB', position (0,0)) for the outline entries of the misc provider.
- `RuleResolver::eval_zone_connection` in kicad-pcb ctx.rs, implemented by
  `DrcEngine::eval_zone_connection`; kicad-zone knockouts.rs calls it instead of its
  re-expression (prove equal results on the zone corpus first).
- kicad-session: pcb-dump-session -> `kicad_pcb::dump::dump_board_ctx` with connectivity and
  rules; drop the `ds.biggest_clearance` rewrite.
- FIX-BEZ-1 (rust/docs/packages/FIX-BEZ-1.md, review R2-07): Bezier points re-approximated
  by the outline build - do it here if it is as small as that file says.
- dump: `lib.poly` from `library_poly_shape(ctx)` (R2-08) if STAB-PCB has not done it.

Verification: every touched crate's tests and corpus replays before and after (no count
gets worse; list the counts), clippy, wasm32 builds.
