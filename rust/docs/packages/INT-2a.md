# INT-2a: Integration step 2 - kernel change requests (integrator role)

Crate: kicad-pcb (kernel files), plus the call sites in every crate that a changed signature
reaches (kicad-connectivity, kicad-drc, kicad-zone, kicad-fab, kicad-session, kicad-cli,
kicad-wasm).

You are the integrator, not a port package: for this assignment the "files you own" rule is
replaced by the rules below. You may edit the frozen kernel files of kicad-pcb (lib.rs,
items_macro.rs, ids.rs, arena.rs, cache.rs, item.rs, traits.rs, ctx.rs, item_ref.rs,
pending.rs, drc_api.rs, test_support.rs, foundation_stub.rs, Cargo.toml), the design
documents, and - for mechanical call-site updates and for wiring finished ports into stale
stubs - any file of a FINISHED package. Read first: rust/docs/design/board-model.md
(completely; you will bump it to rev 2.3), the "Wave 0b integration review" and "GLUE-1"
sections of rust/crates/kicad-pcb/PORT_STATUS.md (what the previous integration steps did),
and rust/docs/PLAN.md ("Fable integration #2").

## Input

`rust/target/orchestrator/change_requests.txt`: the change requests of all 52 finished
packages (43 KB; one `=== <package>` block each). Many are already done (GLUE-1 and later
packages picked them up) or obsolete. Later reports that are not in that file yet: CN-04 asks
for `pub use data::ConnectivityData;` in kicad-connectivity/src/lib.rs plus a module-table
update, for kicad-drc's `CacheGeneratorConnectivity::build` to take `&mut Board` (or apply
`take_net_changes()`), and for an `impl kicad_connectivity::data::CopperZoneRtrees for
DrcCaches`; CN-01 asks for `PcbShape::connection_points` (PCB_SHAPE::GetConnectionPoints; a
temporary port lives in kicad-connectivity items.rs) and for ZONE::CacheTriangulation;
PCBIO-13 asks for a `&Track` entry point in io/writer/tracks.rs so the template via of a
generator can be written (then remove the stripped-hash fallback in
tests/pcbio13_generators.rs); M19's dynamic component-class path waits for a rule matcher
hook.

## Work

1. Triage every request: `done` (say where), `apply` (you do it now), `defer` (name the
   package or step that will do it, and why not now), `reject` (why). Write the table to
   `rust/docs/design/integration-2.md` (one line per request, grouped by target file).
2. Apply. The known kernel items (verify each against the C++ before changing anything; the
   request text is a hint, the C++ is the specification):
   - traits.rs: `EdaShapeItem::corners_in_sequence(ctx)`; `BoardItemMut::style_from_settings`
     taking `&MutEnv`; a `SetNet` virtual on the connected-item trait so the ZONE
     copper-thieving override applies where BOARD::Remove(net) / MapNets call SetNet; a way to
     reach AnyItem / ItemRef from a generic `&B: BoardItem` (detached item for
     `RuleResolver::cached_own_clearance`).
   - ctx.rs: per-layer LAYER_T in `LayerEnv` (BOARD_ITEM::IsSideSpecific in mutators);
     `ConnectivityQuery::net_items(net, types)`.
   - item.rs: `ItemFlags::STARTPOINT` / `ENDPOINT` (pcb_track has local copies).
   - drc_api.rs: `DrcConstraint::name()` and `DrcConstraint::merge` (private copies in
     pcb_track and via.rs to remove).
   - `Pad::is_free_pad` takes ctx (`is_free_pad_ctx` is the port); `Track::eq_item` /
     `similarity` dispatchers; `PcbVia::validate_via_parameters` used by
     BoardDesignSettings; NET_SETTINGS operator== and with it BDS operator== if now possible;
     `PcbDrillMap::symbol_extent(ctx)`; generator arms of move/rotate/flip/mirror/set_layer
     (M23 request); PCB_TABLE Rotate / Flip / Mirror cascade bodies if M11 left them.
   - dump (src/dump): every `no_api(..)` whose getter now exists; LAYER_T and stackup item
     type spellings; then the defaults scoreboard must not lose a match (GLUE-1 left it at
     3533 match / 149 pending / 0 mismatch).
   - `rg -n 'port_pending!' crates/kicad-pcb/src`: for each remaining stub decide whether its
     dependency has landed; wire the ones that have.
   - rustfmt / clippy leftovers in finished packages' files of kicad-pcb.
   NOT yours (another agent is doing it right now): moving `wx_string_matches` to kicad_core
   and the libstdc++ `std::set` tree to kicad_geom (package CLEAN-1); the barcode model
   (M25a); footprint geometry (M20); the footprint and pad parser / writer (PCBIO-06 / 07);
   the property tables (PROP-1).
3. Bump rust/docs/design/board-model.md to rev 2.3 (changed signatures, section 17), update
   board-model-types.md where a type changed, PORTING.md section 9 if a rule becomes
   project-wide, and add an "Integration step 2" section to kicad-pcb/PORT_STATUS.md.

## Working next to running packages (important)

About ten agents are compiling against kicad-pcb right now, and these files are being edited
by a running package: footprint/geometry.rs, footprint/shapes.rs, footprint/compare.rs,
footprint_courtyard_index.rs (M20); io/parser/footprint.rs, io/parser/pad.rs,
io/parser/fp_lib.rs, io/writer/footprint.rs, io/writer/pad.rs, io/writer/fp_lib.rs
(PCBIO-06 then PCBIO-07); src/properties/ (PROP-1); pcb_barcode.rs (M25a);
io/writer/mod.rs `sorted_set` (CLEAN-1); in kicad-drc src/providers/* and the engine files
(DRC-06.., DRC-21.., DRC-23..29), in kicad-fab the plot files (FAB-04..), kicad-zone
(ZF-01..), kicad-session (PCBIO-15).
- Prefer additive changes: when a new parameter would force edits in files of a running
  package, add the new method and keep the old one as a forwarding wrapper, and record the
  removal of the wrapper as deferred.
- A signature change must be atomic: prepare all edits, apply them back to back, and finish
  with `cargo check` of kicad-pcb and of every dependent crate (`-p` one crate at a time). The
  workspace must not stay broken for more than a few minutes. In a running package's file
  make only the one-line mechanical edit the signature forces (Edit tool, exact replacement -
  never rewrite or reformat such a file).
- If a dependent crate fails to compile for a reason that is not yours (an agent in mid-edit),
  wait and retry as PORTING.md section 7 says.

## Verification

`cargo check -p <crate>` for kicad-pcb, kicad-connectivity, kicad-drc, kicad-zone, kicad-fab,
kicad-session, kicad-cli, kicad-wasm and the wasm32 build of kicad-pcb;
`cargo test -p kicad-pcb --lib` (report the failures that exist before you start separately:
run it once first); `python3 crates/kicad-pcb/tools/check_item_shims.py`; the defaults
scoreboard (`pcb-dump defaults` vs `pcb-dump-rs defaults` through
rust/tools/pcb-dump/compare.py); and the board-level dump comparison on five small corpus
boards before and after (no new mismatch). Where a change alters behaviour (SetNet virtual,
LAYER_T in mutators, table cascades) prove it against the reference with pcb-dump or a
ported C++ test.

Report (in the standard result fields): `decisions` = every signature or design change you
made; `known_gaps` = deferred requests with their target; `notes_for_next` = what running
packages need to know about changed APIs (it is forwarded to the review step INT-2b).
