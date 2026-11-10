# WASM-API-1: Board access and editing API for an external autorouter

Crate: kicad-session (the host-independent API), kicad-wasm (JavaScript bindings and
TypeScript typings), rust/web (demo and tests), rust/docs/WASM_API.md (new)

The concrete client: a browser autorouter that takes a `.kicad_pcb` + `.kicad_pro`,
runs a baseline DRC, extracts the board (nets, pads, tracks, arcs converted to chords, vias,
keepouts, courtyards, GND copper; on its demo board about a hundred nets and a few hundred
pads and tracks), routes with its own engine (several seeds; grid routing, then a
relaxation that produces curved traces, i.e. many short segments or arcs), then writes each
candidate back (a few hundred tracks removed, about a thousand added, some vias moved),
adds stitching vias, enables teardrops on pads and vias away from keepouts,
refills zones, runs DRC again until the result is clean, and offers the routed
`.kicad_pcb` and the DRC report (JSON) for download. Today it needs a KiCad for all of
that; the port should be able to replace it in the browser.

What exists (read crates/kicad-wasm/src/lib.rs and kicad-session's PORT_STATUS.md):
`loadBoard(text, {name, project, rules, ...})`, `Board.runDrc`, `exportGerbers`,
`exportDrill`, `plotSvg`, `save`, `refillZones`, `ratsnest`, `nets()`, `tracks()`,
`addTrack`, `addVia`, `remove(id)`. That is too thin for a router. Package FIX-EDIT-1
(running in kicad-pcb) is verifying the model's mutators against the reference; build on
the model API, do not re-implement model logic here.

Design the API in kicad-session (plain Rust structs, serde-serialisable; coordinates as
KiCad internal units = integer nanometres, angles in degrees as the file has them, ids =
the item UUID string plus a compact numeric handle valid for the session) and bind it in
kicad-wasm. Bulk calls return typed arrays or one JSON value per call - never one wasm
call per item.

1. Read:
   - board: copper layers in stackup order with names and types, enabled layers, board
     outline polygons (with holes), bounding box, design rule minimums, the stackup
     thicknesses a via needs;
   - nets (code, name, netclass names) and net classes (clearance, track width, via
     diameter / drill, microvia, diff pair width / gap) plus the RESOLVED constraints a
     router must honour through the DRC engine (custom rules included):
     `clearance(itemA | netA, itemB | netB, layer)`, track width and via size constraints
     for a net on a layer, hole-to-hole and hole clearance, edge clearance - backed by
     `DRC_ENGINE::EvalRules` (kicad-drc), so results equal KiCad's;
   - pads: footprint reference and UUID, pad number, net, position, orientation, layer
     set, attribute (SMD / PTH / NPTH ...), drill (round / slot, size, offset), per-layer
     shape both as a primitive description (kind, size, corner radius, chamfers, offset)
     and as the effective polygon KiCad's DRC uses (PAD::TransformShapeToPolygon with the
     board's max error), teardrop settings, locked;
   - footprints: reference, value, position, orientation, side, courtyard polygons front /
     back, bounding box, locked, DNP;
   - tracks, arcs (start / mid / end), vias (type, layer pair, diameter per layer, drill,
     tenting, free flag), each with net, width, layer, locked;
   - zones and rule areas: outline polygons per layer, net, priority, clearance, min
     thickness, connection style, island mode, the keepout flags (tracks / vias / pads /
     copper pour / footprints), fill polygons per layer after a fill, teardrop zones marked;
   - other copper obstacles: graphic shapes and text on copper layers as polygons, with
     their net if any; Edge.Cuts shapes;
   - connectivity: ratsnest edges (net, the two items and anchor points), per-net
     connected / unconnected counts, clusters (which items are already connected) so a
     router knows what remains.
2. Edit (each returns the new / affected ids; all go through the model's mutation API so
   caches and connectivity stay consistent):
   - add track segment, add arc, add via (through / blind-buried / micro, layer pair,
     diameter, drill, net, free), with defaults from the net class when omitted, exactly
     as KiCad's router would size them;
   - move a via / track end, set width / layer / net, lock / unlock, remove by id, remove
     all tracks of a net or all routing (with a "keep locked" option);
   - `applyEdits(batch)`: many removals, additions and moves in one call with one
     connectivity update (the client writes ~1000 segments per candidate);
   - teardrops: read / set the board teardrop parameters, enable or disable per pad / via /
     track end, `updateTeardrops()` (TEARDROP_MANAGER, kicad-zone) reporting how many were
     created and which items got none;
   - `refillZones()`, `rebuildConnectivity()` where the edit calls do not already do it.
3. Check and output (exist; make them fit the loop): `runDrc` with options for severity,
   `allTrackErrors`, a quick mode restricted to chosen providers (e.g. clearance +
   unconnected only) for inner-loop use, returning structured violations (type, severity,
   description, position, layer, item ids) besides the KiCad JSON / text report; `save()`
   (the `.kicad_pcb` text), Gerber / drill / SVG export.
4. `rust/docs/WASM_API.md`: every call with its JSON shape and units, a worked example of
   the router loop above, and what is NOT available (no push-and-shove routing: the client
   routes); `kicad-wasm` ships TypeScript typings (`pkg/*.d.ts`: check what wasm-bindgen
   emits and add a hand-written `.d.ts` for the JSON shapes).
5. Demo and tests: rust/web gets a small "router loop" page or node script
   (rust/web/test/router_loop.mjs): load demos/ecc83/ecc83-pp (and one 4-layer board with
   zones and vias), record its routing through the read API, remove all routing, add it
   back through `applyEdits`, update teardrops if the board uses them, refill, DRC, save.

Verification against the reference (ORACLE.md; COPIES):
- Read API: every value against the reference dump (`pcb-dump board --loader`, sections
  item, track, pad, shape, zone, footprint, board, connectivity, evalrules) on thirty
  corpus boards incl. boards with custom rules, blind / micro vias, slots, custom pads,
  rule areas: identical (pad polygons vertex for vertex).
- The round trip above: the saved board must equal the reference re-save of the original
  byte for byte modulo the fresh UUIDs of re-added items (and with UUIDs passed in by the
  client: exactly), `kicad-cli pcb drc` on the port's saved board must give the same
  report as on the original, and `kicad-rs pcb drc` the same as the reference - on twenty
  boards.
- Resolved constraints: `clearance(...)` etc. against the EvalRules oracle of kicad-drc
  (pcb-dump `evalrules` section) for sampled item pairs on the boards with rule files.
- Native vs wasm: the node test runs the same loop on three boards and compares saved
  board and DRC JSON with the native session API.

Other agents: FIX-EDIT-1 in kicad-pcb (mutators; if a mutator you need is not verified or
not ported yet, call it and list it in known_gaps - do not work around it), GLUE-3 and
FIX-FAB-2 make small edits in kicad-session / kicad-cli: re-read before editing.
