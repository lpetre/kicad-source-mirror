# DRC-LP: Footprint library parity (the largest remaining difference of `pcb drc`)

Crate: kicad-drc (src/providers/library_parity.rs - a placeholder today - and its tests),
kicad-pcb (a new module for the footprint library table: project / global `fp-lib-table`
files), kicad-session + kicad-cli (hosts supply the table files and library footprints;
minimal edits)

State: `kicad-rs pcb drc` is compared with the reference kicad-cli over the corpus
(kicad-cli/tests/oracle/e2e_cli.py, kicad-drc/tests/oracle/drc12_cli_compare.py). The
largest remaining difference: 175 of 308 boards differ only by `lib_footprint_issues` /
`lib_footprint_mismatch` entries, because the library parity provider is reported as "not
run". (The tier-3 package DRC-34 was going to port a stub of it; it no longer does: this
package owns the whole provider.)

C++ to port statement by statement:
- pcbnew/drc/drc_test_provider_library_parity.cpp (1,250 lines): Run() with its three
  `lib_footprint_issues` messages (library not in the configuration / not enabled or not
  found / footprint not found in the library) and the `lib_footprint_mismatch` path: the
  library footprint is loaded, `FOOTPRINT::FootprintNeedsUpdate` with every helper of that
  file (the property comparison macros, pad / shape / text / zone / model comparison with
  their epsilon and flags, the REPORT texts) decides; the per-library cache; error limits;
  item positions; `PrintZoneConnection` with the RAW zone_connect accessors of the model
  (`*_raw`; the enum accessors return FULL for out-of-range values).
- The library side: common/libraries/* as far as kicad-cli uses it for footprints here
  (LIBRARY_TABLE / LIBRARY_TABLE_ROW / the `fp-lib-table` s-expression parser incl. legacy
  forms, global + project table and their precedence, `${KIPRJMOD}` and environment
  variable expansion in URIs, the "disabled" / "hidden" flags, nested table references),
  FOOTPRINT_LIBRARY_ADAPTER::GetRow / HasLibrary / LoadFootprint as used by the provider
  (KiCad `.pretty` directories only; other plugin types: say what the reference does and
  mirror the message it produces when a library cannot be loaded).
  File access belongs to the host: kicad-pcb / kicad-drc take a "library source" trait
  (table texts; list / read of `<lib>.pretty/<name>.kicad_mod`); kicad-cli implements it on
  the file system (project directory, the config directory kicad-cli would read for the
  global table - check which one the reference build uses with the hermetic HOME of
  rust/docs/ORACLE.md; the environment variables kicad-cli defines, e.g.
  KICAD10_FOOTPRINT_DIR, and what is unset in the oracle environment), kicad-wasm takes
  them from the caller (API: optional library files; without them the provider behaves as
  with an empty table).
  The footprint file parser exists (`kicad_pcb::io::parse_footprint_file`, package
  PCBIO-14); `Footprint::eq_item` in kicad-pcb footprint/compare.rs is only partly ported
  and is NOT what the provider uses - port FootprintNeedsUpdate from the provider file.
- Wire it: the registry slot in kicad-drc run_flow.rs (remove it from UNPORTED_PROVIDERS
  and from the UNPORTED list in tests/oracle/drc12_cli_compare.py), the session load.

Other agents in kicad-drc right now: DRC-31 (matched length) and DRC-33 (creepage) in their
own new files, a second stabilisation pass (STAB-DRC-2) in finished provider files,
run_flow.rs and the harness: you own library_parity.rs, your tests and the library table
module; edits to run_flow.rs / the harness are small and atomic, re-read first.

Verification (reference kicad-cli on COPIES, ORACLE.md):
1. The whole corpus through `kicad-rs pcb drc --format json --severity-all` vs the
   reference: every `lib_footprint_issues` / `lib_footprint_mismatch` entry identical
   (message, item, position), in report order where the C++ order is deterministic
   (footprint order); the 175 boards that differed only by these codes become identical or
   move to another named cause. Most corpus boards have no table next to them (first
   message); demos with a project `fp-lib-table` exercise the others - list which.
2. Crafted cases for `lib_footprint_mismatch`: a project with its own `fp-lib-table` and
   `.pretty` library next to the board (committed under tests/data), footprints on the
   board that differ from the library in every property group the C++ compares (one
   difference per footprint: pad moved / resized / shape / drill / layers / net-tie
   groups, graphics, text, fields, attributes, zones, 3D model, courtyard ...): the
   reference report gives the expected entries; identical. Also: disabled library row,
   missing library directory, missing footprint, legacy table format, nested table.
3. The QA false-negative regression table of kicad-session (tests/drc12_regressions.rs) -
   its 23 counts were not asserted while this provider was "not run": they must assert
   now; report the result.
4. `cargo test` of the touched crates, clippy, wasm32 builds.
