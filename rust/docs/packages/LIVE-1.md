# LIVE-1: Transactions with incremental connectivity, checkpoints and bulk I/O for a router loop

Crate: kicad-session (src/board_api: you own it; the previous package WASM-API-1 is
finished - read its PORT_STATUS section and rust/docs/WASM_API.md), kicad-wasm (bindings,
TypeScript types), kicad-connectivity (fixes to the incremental paths if the oracle finds
any; additive otherwise), rust/web/test, rust/docs/WASM_API.md, crates/kicad-bench (an
"edit loop" pipeline)

Why: the first client of the port is a browser autorouter. Per candidate it removes a few
hundred tracks, adds about a thousand segments, moves vias, then needs connectivity, a
zone refill and a DRC answer - several candidates per run, each starting from the same
base board. Today `board_api::apply_edits` rebuilds the whole connectivity after every
batch (BOARD::BuildConnectivity) and there is no way to return to the base state except
reloading the file. KiCad's editor does it differently: BOARD_COMMIT::Push
(pcbnew/board_commit.cpp) tells CONNECTIVITY_DATA which items were added / removed /
changed (`Add`, `Remove`, `Update`, `MarkItemNetAsDirty`), then `RecalculateRatsnest`
re-does only the dirty nets, and undo restores the previous items. The port has those
incremental entry points (`kicad_connectivity::ConnectivityData::add / remove / update /
mark_item_net_as_dirty / recalculate_ratsnest / propagate_nets`) and a change journal on
the board (kicad-pcb src/board/journal.rs), but nothing uses them together and they have
never been compared with the reference for edits (package FIX-EDIT-1 left that part
open).

Design (decided; implement it, and report where the C++ forces a deviation):
1. A transaction layer in board_api - the port of BOARD_COMMIT's data flow without the
   GUI: `begin()` -> stage removals, additions and modifications (the existing EditBatch
   content) -> `commit()`:
   applies them to the board in BOARD_COMMIT::Push order (read Push statement by
   statement: the order of connectivity Remove / Add / Update calls relative to the board
   edits, MarkItemNetAsDirty before a change, net propagation, which flags make it skip
   the ratsnest or the connectivity), updates connectivity INCREMENTALLY, recalculates the
   ratsnest of the dirty nets only, and returns a change report: new item handles, nets
   whose connectivity changed, per-net and total unconnected counts, items whose net was
   changed by propagation. `apply_edits` keeps its signature and becomes
   begin + stage + commit. A `rebuild_connectivity()` (full) stays available.
2. Checkpoints: `checkpoint()` returns a token; `rollback(token)` puts board AND
   connectivity back exactly (same items with the same handles and UUIDs, same nets, same
   zone fills, same UUID generator state), without reparsing. Choose the mechanism by
   measuring on a medium board: undo through the journal like BOARD_COMMIT::Revert, or a
   snapshot (the model is plain data in arenas: `Board: Clone`; connectivity cloned or
   rebuilt) - whichever is faster and exact; document the cost.
3. Bulk transfer: besides the JSON forms, typed-array forms for the hot calls -
   tracks / vias in and out as flat Int32Array records (x1, y1, x2, y2, width, layer
   index, net code [, mid x, mid y for arcs]) and handle arrays as Float64Array /
   BigUint64Array - one wasm call per batch, no per-item JS objects. JSON forms keep working.
4. `finalize()`: full connectivity rebuild (as BOARD::BuildConnectivity at load), so that
   what follows - full refill, full DRC, save - is exactly what kicad-cli computes on the
   saved board. The fast paths above must give the same connectivity facts; where the
   reference's own results depend on build history (docs/design/review-connectivity.md:
   ratsnest ends under ties, order of connected items), "same" means equal as sets, and
   finalize() is what makes reports byte-identical.

Verification (all three are required; ORACLE.md, PORTING.md section 7 for C++ builds):
a. Incremental == full, in the port: on thirty corpus boards, 200 seeded random batches
   each (remove random tracks / vias, add segments and vias incl. ones that connect two
   nets' worth of copper, split and merge clusters, move vias, change nets, edit inside
   and outside zones, touch free vias and net ties), after EVERY commit compare the live
   connectivity with a fresh full build on a clone of the board: net of every item,
   cluster partition as sets, per-net ratsnest node sets and edge sets with weights
   (exact order where no tie record exists), unconnected counts, dangling flags, the
   isolated-island map, `IsConnectedOnLayer` for every via and pad layer. Any difference
   is a defect in the incremental path (or a documented history dependence of the C++:
   prove it with the driver of b).
b. Incremental == reference: a C++ driver against the reference build (the pattern of
   kicad-pcb tests/oracle/editmut_oracle.cpp: links the pcb-dump objects, one compile and
   link under the machine-wide flock) that loads a board, applies scripted batches through
   BOARD::Add / Remove + CONNECTIVITY_DATA::Add / Remove / Update + RecalculateRatsnest
   exactly as BOARD_COMMIT::Push does, and dumps the connectivity section after each;
   Rust replay through the transaction layer: identical (set comparison only at the
   documented heap-order sites) on ten boards x 20 batches.
c. Checkpoints: after any sequence of commits, `rollback` gives a board whose save is
   byte-identical to the base save and whose full dump (pcb-dump-rs all sections) and
   connectivity dump equal the base ones; a second run of the same batches after rollback
   gives the same results as the first (UUIDs included).
d. Round trip and reference end state: the WASM-API-1 router loop
   (web/test/router_loop.mjs, tests/router_loop.rs) through transactions + finalize():
   saved board and DRC report unchanged (290-board round trip still byte-identical).
e. Timing: add an `edit` pipeline to kicad-bench (remove all routing of the board in ten
   batches and add it back in ten batches; stages: commit incl. connectivity, checkpoint,
   rollback) and report before / after in docs/PERF.md format (native and wasm_bench.mjs)
   for the small and medium boards. Do not edit other sections of docs/PERF.md.

Other agents: a performance loop is optimising library crates (kicad-geom, kicad-zone,
kicad-drc, kicad-connectivity, kicad-pcb) at the same time, one small change per file
set, and its judge occasionally reverts files with git: you own
crates/kicad-session/src/board_api, crates/kicad-wasm and your new test / oracle files;
in kicad-connectivity keep fixes minimal, re-read a file right before editing, and list
every file you touched there in your result so the orchestrator can tell the loop.
