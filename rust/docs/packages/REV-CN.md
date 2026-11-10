# REV-CN: Review of the connectivity port (reviewer role)

Crate: kicad-connectivity (review of CN-01 .. CN-04; fixes in its files - all four packages
are finished, nobody else edits this crate)

kicad-connectivity (items, r-tree, the connectivity algorithm, ratsnest, the data facade)
was ported by four agents in sequence; each verified its part against the reference
pcb-dump (`connectivity` section) and recorded decisions and deviations in
rust/crates/kicad-connectivity/PORT_STATUS.md. DRC, zone fill, teardrops and the board
re-save (net propagation onto tracks changes what the writer saves) build on it. Nobody has
yet read the port side by side with the C++ with a reviewer's eye. The goal of the project
is output identical to KiCad's; judge everything by whether it can change the saved board,
a DRC report, a zone fill or a teardrop.

Read: rust/docs/analysis/connectivity-zones.md (sections on connectivity, the
order-sensitivity table, risks R-1 .. R-7), rust/docs/design/board-model.md section 3.1
(pointer-order replacement rule) and 8, the crate's PORT_STATUS.md, then the code next to
pcbnew/connectivity/*.cpp/.h (connectivity_algo, connectivity_items, connectivity_data,
ratsnest_data, from_to_cache is tier 3) and pcbnew/ratsnest/*.

## What to review

1. Pointer order. The C++ keeps `CN_ITEM::m_connected` and several result containers ordered
   or iterated by heap address (std::set<CN_ITEM*>, sorts of pointers, std::map keyed by
   pointer, unordered_map / unordered_set iteration). The port replaced these by list /
   ItemId order (R-1, R-2). For EVERY such site: (a) can the order reach an output (which
   cluster becomes the origin of a net, which net wins a conflict in PropagateNets, the
   order of ratsnest nodes / edges with equal weight, dangling results, the track order
   teardrops see through GetConnectedPadsAndVias / GetConnectedTracks, the island map)?
   (b) in the C++, is that order in practice the allocation order (items allocated in board
   load order on a fresh heap) and does the replacement reproduce exactly that, or a
   different order? Where the C++ result depends on the heap, say which order the reference
   kicad-cli produces on a freshly loaded board and make sure the port matches THAT (the
   differential oracles run on freshly loaded boards). Produce the complete table of sites.
2. The algorithm, statement by statement for the output-relevant parts: the contact rules
   of the visitor (per item-type pair, the layer logic incl. conditionally flashed pads /
   vias and zone layers, tolerances), the deferred via-net rule, cluster search modes and
   the final sort (comparator!), PropagateNets conflict resolution, the ratsnest (delaunator
   input order, duplicate-point handling, the Kruskal edge sort comparator and tie order,
   the excluded / visible flags, anchor tags), IsConnectedOnLayer for every item type,
   TestTrackEndpointDangling, the island map and the zone single-connection logic.
3. Numeric rules of PORTING.md (KiROUND, casts, overflow, sqrt / hypot through
   `kicad_geom::cmath`), `unwrap_or` in places where the C++ dereferences, and state the C++
   mutates lazily (dirty flags, bbox caches) that the port made explicit.
4. Coverage of the oracles: what they do not exercise (the status file lists: pads until the
   pad parser lands, ratsnest exclusions, the zone r-tree path, zone islands in
   IsConnectedOnLayer). Close the gaps you can with new synthetic boards through the
   existing generator (tests/oracle/gen_cn04.py runs the existing pcb-dump; no C++ build):
   pads cannot be parsed yet, but zones, vias, tracks, arcs and copper shapes can. If the
   footprint / pad parser has landed by the time you run (try a board with footprints),
   rerun all connectivity goldens with pads and analyse every mismatch.

## Findings

Write rust/docs/design/review-connectivity.md: the pointer-order site table (site, C++
order, port order, reaches which output, verdict, evidence) and one row per finding (id,
severity blocker / major / minor, file:line, C++ reference, what differs for which input,
status). Fix what you can prove, with a regression case against the reference; correct the
PORT_STATUS.md entries. Larger work: write `rust/docs/packages/FIX-CN-<n>.md` in the format
of the other files there and list it in your result.

Verification: `cargo test -p kicad-connectivity` before and after, clippy, the wasm32
build. Report: `summary` with counts per severity; `decisions` = fixes made; `known_gaps` =
open findings; `notes_for_next` = what the zone-fill, teardrop and DRC packages must know.
