# GLUE-1: wire finished ports into their call sites (small fixes, no signature changes)

Crate: kicad-pcb

Several packages have finished and reported call sites in files they did not own that still
call a `port_pending!` stub although the real function now exists, plus a few tests that
assert "this construct is still a staging stub" for constructs that have since been ported.
You apply these small, mechanical fixes. You may edit any file of `rust/crates/kicad-pcb` for
this purpose EXCEPT files whose package is still running (do not touch: `src/pcb_shape/*`,
`src/pcb_textbox.rs`, `src/pcb_table.rs`, `src/pcb_tablecell.rs`, `src/zone/*`,
`src/zone_utils.rs`, `src/pcb_dimension.rs`, `src/pad/shapes.rs`, `src/io/parser/zone.rs`,
`src/io/writer/zone.rs`, `src/io/parser/dimension.rs`, `src/io/writer/dimension.rs`,
`src/io/parser/setup.rs`, `src/io/writer/setup.rs`, `src/io/parser/group.rs`,
`src/io/writer/group.rs`, `src/io/parser/netclass_embedded.rs`,
`src/io/writer/netclass_embedded.rs`, `src/io/parser/misc_items.rs`,
`src/io/writer/misc_items.rs`, `src/project/*`). Do NOT change any trait or public function
signature (a separate, later integration step does that): if a fix would need one, skip it and
list it.

Work list (verify each against the code as it is now; some may already be done):
1. `src/board/compare.rs` `eda_shape_compare`: call `EdaShape::compare`
   (`src/eda_shape/geometry.rs`) instead of `port_pending!("EDA_SHAPE::Compare")`.
   `BOARD::cmp_drawings` calls `shape->Compare(other)`; check the C++ for the exact arguments.
   This unblocks the board-level drawing save order (about 267 corpus cases in the PCBIO-03
   oracle `tests/pcbio03_tracks_shapes.rs`: re-run it and report the new counts).
2. `src/padstack.rs` `CopperLayerProps::compare`: replace
   `port_pending!("EDA_SHAPE::Compare")` with `EdaShape::compare` per custom shape, as the C++
   does.
3. `src/item_ref.rs`: `ItemRef::fp_relative_position`, `netname` (and any other forwarder
   there that is still `port_pending!` although `board_item.rs` / `board_connected_item.rs`
   now has the function: `layer_name`, net / netclass lookups ...): forward to the real
   functions. This file is a frozen kernel file; these forwarders are the sanctioned exception.
4. `src/board/nets.rs` `Board::apply_net_chain_netclasses`: the functions it waited for exist
   in `src/net_settings.rs` (`clear_chain_pattern_assignments`,
   `set_chain_pattern_assignment`, `has_netclass`, `has_chain_pattern_assignments`): replace
   the `port_pending!` tail with the C++ body (`pcbnew/board.cpp`).
5. `src/board/text_vars.rs` / `src/pcb_text.rs` / `src/pcb_field.rs` / `src/board/queries.rs`:
   local helper stubs for functions that now exist elsewhere (grep `port_pending!` in these
   files and check each name against the crate with `rg "fn <name>"`). Leave those whose
   target really is not ported yet (FOOTPRINT::ResolveTextVar etc. belong to the footprint
   packages M19 / M20, not started).
6. `src/dump/*.rs`: every `no_api(..)` call whose getter now exists in the model (check each
   name: tracks, arcs, vias (`src/pcb_track/via.rs`), EDA_SHAPE getters, pad getters of
   `src/pad/mod.rs` / `derive.rs`, footprint getters only if they exist) is replaced by the
   real call, exactly as the C++ tool `rust/tools/pcb-dump/dump_items.cpp` calls it; the
   LAYER_T formatter must print `LT_SIGNAL` / `LT_POWER` / ... as the C++ dump does (package
   M22 reported `SIGNAL`); emit the `meander.*` lines if the data is available as JSON
   (compare with `pcb-dump defaults`), otherwise leave them pending.
7. Tests that assert staging-stub behaviour for constructs that are now ported:
   `src/io/parser/tests.rs` `staging_stubs` (uses `(segment ..)`: switch to a construct that
   is still a stub, or to an artificial unknown token if none is left), `src/io/writer/tests.rs`
   `staging_stubs` / `oracle_resave` (they expect the setup section to be a stub). Keep the
   intent of each test.
8. Run `cargo test -p kicad-pcb --features dump` (and without the feature) and report the
   exact list of failing tests with the owning package of each, after your fixes.

Verification: the tests of the touched modules; the defaults scoreboard
(`cargo test -p kicad-pcb --features dump --test pcb_dump_defaults -- --nocapture`) must show
0 mismatches and fewer pending lines than before (report before / after); the PCBIO-03 corpus
oracle counts before / after; `python3 crates/kicad-pcb/tools/check_item_shims.py` still ok.
