# LIVE-2: Persistent DRC state, item-scoped checks and affected-only zone refill

Crate: kicad-session (src/board_api), kicad-wasm, kicad-drc and kicad-zone (NEW entry
points in new files where possible; no change to existing behaviour), rust/docs/WASM_API.md,
crates/kicad-bench

Continuation of LIVE-1 (transactions with incremental connectivity, checkpoints): the
other two costs of a router's inner loop are the DRC answer and the zone refill.

1. Persistent DRC state. A DRC run rebuilds everything each time: rule compilation
   (InitEngine), the cache generator (copper item r-tree, zone r-trees, clearance and
   worst-constraint caches), provider-local trees. Keep a live DRC state on the session
   that survives between calls and is updated from the transaction's change report:
   items inserted into / removed from the trees, caches keyed by item invalidated for
   changed items only, rules recompiled only when the rule text changes. The first call
   after load (or after `finalize()`) builds it. Results must equal a fresh run.
2. Item-scoped check: `checkItems(handles, {providers})` returns the violations that
   involve at least one of the given items, for the providers where that is well defined
   (copper clearance incl. holes and shorts, edge clearance, hole-to-hole, track width,
   via diameter / annular width, disallow / keepouts, connectivity: dangling, unconnected
   for the items' nets). Define the result as: the subset of a full run with
   report-all-track-errors semantics whose entries involve a given item, and prove
   exactly that (below). Say for each provider what its C++ loop reports once per item
   vs once per pair, and how the scoped form keeps the same entries.
3. Affected-only refill: `refillZones({affectedBy: changeReport | handles})` refills only
   the zones whose fill can change: zones whose bounding box inflated by the worst
   clearance meets a changed item on a shared layer, closed under the fill dependency
   graph of ZONE_FILLER (zones that read other zones' fills, same-net overlaps, the
   iterative refill), with islands and via / pad flashing recomputed where connectivity
   changed. The result must be identical, vertex for vertex, to a full refill of the
   edited board; where you cannot prove a zone unaffected, refill it.
4. Bench: extend the kicad-bench `edit` pipeline with `checkItems`, scoped refill,
   `updateTeardrops`; before / after tables (native and wasm).

Verification:
a. Persistent state == fresh run: on thirty corpus boards with 50 seeded random edit
   batches each, after every commit compare `runDrc` on the live state with `runDrc` on a
   freshly loaded copy of the saved board (text report byte for byte, modulo the
   documented random-UUID lines).
b. Scoped == subset of full: same boards and batches plus injected violations (tracks
   moved into pads, vias dropped on other nets and on keepouts, holes too close, tracks
   over the edge): the set `checkItems` returns equals the entries of a full
   `runDrc({allTrackErrors: true})` that involve those items, per provider.
c. Affected-only refill == full refill: same batches on the boards with zones (incl.
   priorities, same-net overlaps, hatched, thermal reliefs, teardrops, islands): fills
   identical vertex for vertex and need-refill flags equal; then save: byte-identical to
   the board saved after a full refill.
d. End state against the reference: after a sequence of batches + `finalize()` + full
   refill, `kicad-cli pcb drc` on the port's saved board gives the same report as
   `kicad-rs pcb drc`, on twenty boards.

Other agents: the performance loop edits existing files of kicad-drc / kicad-zone /
kicad-geom / kicad-connectivity (and its judge may revert files with git): add your
entry points in NEW files (e.g. kicad-drc src/live.rs, kicad-zone src/affected.rs) plus
one `mod` line, re-read before editing, and list every existing file you had to touch.

Closing note: item 1 (DRC state kept between runs) is not built; `checkItems` is a filtered full run of its providers and saves no time over `quickDrc` with the same providers (`crates/kicad-session/PORT_STATUS.md`, LIVE-2).
