# Board access and editing API for external routers (WASM-API-1)

The WebAssembly build (`crates/kicad-wasm`, built with `rust/scripts/build-wasm.sh` into
`web/pkg` / `web/pkg-node`) gives a client that routes by itself everything around the
routing that would otherwise need a KiCad installation: reading the board and its resolved
design rules, editing copper, teardrops, zone refill, DRC and saving. The same API exists
natively as `kicad_session::board_api` (plain Rust structs, serde); the JavaScript calls are
thin wrappers (`crates/kicad-wasm/src/board_api.rs`). TypeScript typings of every JSON shape
ship in `pkg/kicad_wasm.d.ts` (source: `crates/kicad-wasm/types/board_api.d.ts`).

**Not available**: KiCad's interactive (push-and-shove) router. The client routes; the API
reads, writes, checks and saves.

## Conventions

| What | Unit / form |
|---|---|
| coordinates, lengths, widths | KiCad internal units: integer nanometres (1 mm = 1 000 000), y axis down |
| points | `[x, y]` |
| boxes | `[x, y, w, h]` (`BOX2I` position and size as stored) |
| angles | degrees (`EDA_ANGLE::AsDegrees`) |
| layers | canonical names (`LSET::Name`): `F.Cu`, `In1.Cu` ... `B.Cu`, `Edge.Cuts`, `F.CrtYd` |
| item ids | `id`: a numeric handle (< 2^53), valid for this `Board` object until the item is removed; `uuid`: the item UUID string, what survives `save()` |
| polygons | `[{ outline: [pt...], holes?: [[pt...]...], width? }]`, KiCad's vertex order (`SHAPE_POLY_SET`), no repeated closing vertex; `width` is the chain stroke width (courtyards) |
| absent values | omitted / `undefined` (serde-wasm-bindgen) |
| enums | the C++ enumerator names: `PTH`, `ROUNDRECT`, `CHAMFERED_RECT`, `THERMAL`, ... |

Every bulk call returns one JSON value: never one wasm call per item. Every edit call returns
`EditResult` `{ added, addedUuids, removed, changed }` (handles) and is one transaction (LIVE-1:
`BOARD_COMMIT::Push` with incremental connectivity, see "Live router loop"); `applyEdits`
takes any number of edits for one transaction.
Errors are JavaScript `Error`s named `<Kind>Error` (`InvalidArgumentError`, `StaleIdError`,
`NotYetPortedError` with a `subsystem` property, ...).

## Loading, output (existing calls)

| Call | Returns |
|---|---|
| `loadBoard(text, { name, project, rules, localSettings, uuidSeed, date })` | a `Board` after `BOARD_LOADER::Load` (project, DRC engine with the rules, connectivity) |
| `board.save()` | the `.kicad_pcb` text (`PCB_IO_KICAD_SEXPR::SaveBoard`) |
| `board.runDrc({ format: 'json' \| 'report', units, severities, allTrackErrors, refillZones })` | `{ exitCode, report, files, messages }` as `kicad-cli pcb drc` |
| `board.exportGerbers({...})`, `exportDrill({...})`, `plotSvg({...})` | fabrication outputs as `kicad-cli pcb export ...` |
| `board.refillZones()` | `ZONE_FILLER_TOOL::FillAllZones` (headless: full teardrop update, fill, connectivity rebuild) |
| `board.ratsnest()` | `Int32Array` `[x1, y1, x2, y2] * n` of the unconnected edges |

## Read

| Call | Returns (TypeScript name) | C++ source of the values |
|---|---|---|
| `boardInfo()` | `BoardInfo`: `copperLayerCount`, `copperLayers` (top to bottom: `name`, `userName`, `layerType` `signal`/`power`/`mixed`/`jumper`, `layerId`, `depth` = stackup distance from F.Cu), `enabledLayers`, `outline` + `outlineOk` (`GetBoardPolygonOutlines( .., true )`), `bbox`, `edgesBbox`, `rules` (`minClearance`, `trackMinWidth`, `minConnection`, `viasMinAnnularWidth`, `viasMinSize`, `minThroughDrill`, `microViasMinSize`, `microViasMinDrill`, `copperEdgeClearance`, `holeClearance`, `holeToHoleMin`, `maxError`), `boardThickness`, `stackup` (`name`, `typeName`, `layer`, `thickness`) | `BOARD`, `BOARD_DESIGN_SETTINGS`, `BOARD_STACKUP` |
| `netDetails()` | `NetDetail[]`: `code`, `name`, `netclass` (effective), `netclasses` (constituents) | `NETINFO_ITEM::GetNetClass()` |
| `netclasses()` | `NetclassInfo[]`: `name`, `priority`, `clearance`, `trackWidth`, `viaDiameter`, `viaDrill`, `microviaDiameter`, `microviaDrill`, `diffPairWidth`, `diffPairGap`, `diffPairViaGap` (unset = absent) | `NET_SETTINGS` |
| `pads({ polygons })` | `PadInfo[]`: `id`, `uuid`, `footprint`, `footprintUuid`, `reference`, `number`, `net`, `position`, `orientation`, `layers`, `attribute`, `drill` (`{ shape: CIRCLE\|OBLONG, size }`), `shapes` (per padstack layer: `shape`, `size`, `offset`, `cornerRadius`, `chamferRatio`, `chamferPositions`), `polygons` (per copper layer: `layer`, `flashed`, `polygon` = `GetEffectivePolygon( layer, ERROR_INSIDE )` with the board's max error), `teardrops`, `locked` | `PAD` |
| `footprints()` | `FootprintInfo[]`: `reference`, `value`, `position`, `orientation`, `side`, `courtyardFront`, `courtyardBack` (`GetCourtyard`), `bbox`, `locked`, `dnp`, `pads` | `FOOTPRINT` |
| `routeItems()` | `RouteItem[]`: `kind` `track`/`arc`/`via`, `net`, `start`, `end`, `mid` (arcs), `width` (via: diameter), `layer`, `locked`, `solderMask`, `solderMaskMargin`, `via` (`viaType` `through`/`blind`/`buried`/`micro`, `top`, `bottom`, `diameters` `[[layer, d]...]`, `drill`, `free`, `finish` (tenting/covering/plugging pairs, capping, filling: `from_board`/`yes`/`no`), `unconnectedLayers`, `zoneLayerConnections`, `teardrops`) | `PCB_TRACK`, `PCB_ARC`, `PCB_VIA` |
| `zones({ polygons })` | `ZoneInfo[]` (board zones, then footprint zones): `name`, `net`, `layers`, `priority`, `ruleArea`, `keepout` (`tracks`, `vias`, `pads`, `copperPour`, `footprints`), `teardrop`, `locked`, `clearance`, `minThickness`, `connection`, `islandMode`, `outline` (board frame), `filled`, `fills` (`[{ layer, polygons }]`) | `ZONE` |
| `copperObstacles()` | `ObstacleInfo[]`: graphic shapes and texts on copper layers (board and footprint), one entry per layer: `kind`, `layer`, `net`, `footprint`, `polygon` (`TransformShapeToPolygon( layer, 0, maxError, ERROR_INSIDE )`) | `PCB_SHAPE`, `PCB_TEXT`, `PCB_TEXTBOX` |
| `edgeShapes()` | `ObstacleInfo[]` of the Edge.Cuts shapes | |
| `connectivity()` | `ConnectivityInfo`: `unconnected` count, `edges` (`net`, `source`, `target`, `sourcePos`, `targetPos`), `nets` (`net`, `pads`, `unconnected`, `clusters`: lists of item handles already connected) | `CONNECTIVITY_DATA::RecalculateRatsnest`, `RunOnUnconnectedEdges`, `GetClusters` |

## Resolved rules

| Call | Returns |
|---|---|
| `constraint({ kind, a, b?, layer })` | `ConstraintResult`: `constraintType` (`NULL_CONSTRAINT` when no rule applies), `min`, `opt`, `max`, `name` (rule name), `minRule`, `disallow`, `implicitMin`, `severity`, `severityCode` - `DRC_ENGINE::EvalRules( type, a, b, layer )`, custom rules included. `kind` is a `DRC_CONSTRAINT_T` without `_CONSTRAINT`, lower case: `clearance`, `hole_clearance`, `hole_to_hole`, `edge_clearance`, `track_width`, `via_diameter`, `hole_size`, `annular_width`, `physical_clearance`, `diff_pair_gap`, `disallow`, ... `a` / `b` are `{ id }` (a board item) or `{ net, dummy?: 'track' \| 'via' }` (a dummy item of that net, as KiCad's router's rule resolver builds one). `layer: ''` = `UNDEFINED_LAYER`. |
| `routerSizes(net, layer)` | `RouterSizes`: `clearance`, `trackWidth`, `viaDiameter`, `viaDrill`, `microviaDiameter`, `microviaDrill` - the sizes KiCad's router starts with (`PNS_KICAD_IFACE_BASE::ImportSizes`, net class branches; micro vias as `ROUTER_TOOL` sizes them with `PCB_VIA::SetSizeFromRules`) |

## Edit

| Call | Takes |
|---|---|
| `applyEdits(batch)` | `EditBatch` `{ remove?: [id], removeRouting?: { nets?, keepLocked?, vias? }, change?: [ItemChange], add?: [NewItem] }`, applied in that order; one transaction (new items appended) |
| `addTrackSegment(t)`, `addArc(a)`, `placeVia(v)` | one `NewTrack` / `NewArc` / `NewVia` |
| `changeItem(c)` | `ItemChange` `{ id, start?, end?, mid?, width?, drill?, layer?, bottom?, net?, locked?, free? }` (a via: `start` moves it, `layer` / `bottom` set the layer pair) |
| `removeItems([id])`, `removeRouting({ nets, keepLocked, vias })` | |
| `rebuildConnectivity()` | `BOARD::BuildConnectivity` |

`NewItem` = `{ kind: 'track', start, end, layer, net?, width?, uuid?, locked?, solderMask?,
solderMaskMargin? }` | `{ kind: 'arc', ..., mid }` | `{ kind: 'via', at, viaType?, top?, bottom?,
diameter?, drill?, net?, free?, uuid?, locked?, finish?, unconnectedLayers?,
zoneLayerConnections?, teardrops? }`. An omitted `width` / `diameter` / `drill` is the
`routerSizes` value of the net and layer; an omitted `uuid` is a fresh one. A `uuid` already
on the board is refused. `RouteItem` carries every field a `NewItem` takes, so recording and
re-adding the routing gives the board back byte for byte (see Verification).

Validation: the handles of `remove` and `change` are checked before anything changes; an
invalid addition stops the batch after the edits before it (the error names the problem);
every call rebuilds connectivity once at its end, also on error. Net propagation is KiCad's:
a track that touches pads takes their net at the rebuild.

## Teardrops

| Call | |
|---|---|
| `teardropSettings()` / `setTeardropSettings(s)` | `TeardropSettings`: `targetVias`, `targetPthPads`, `targetSmdPads`, `useRoundShapesOnly`, `round` / `rect` / `track` (`TeardropParams`: `enabled`, `maxLen`, `maxWidth`, `bestLengthRatio`, `bestWidthRatio`, `widthToSizeFilterRatio`, `curvedEdges`, `allowTwoTracks`, `onPadsInZones`) |
| `setItemTeardrops([id], enabled)` | per pad / via switch (`GetTeardropParams().m_Enabled`); returns the handles changed |
| `updateTeardrops()` | `TEARDROP_MANAGER::UpdateTeardrops` (full update): `{ teardrops, candidates, without: [id] }` - the teardrop zones now on the board, the enabled pads / vias with connected tracks, and those that got none |

`refillZones()` runs the same full teardrop update first (as KiCad's `FillAllZones`).

## Check

| Call | Returns |
|---|---|
| `runDrc({ format: 'json' })`, then `drcViolations({ severities })` | the KiCad report, then `DrcSummary` `{ violations, markers, unconnected, parity, checksNotRun }` from the run's markers; `Violation` = `{ violationType, code, severity, description, position, layer, items: [{ uuid, id }], rule, section }` |
| `quickDrc({ providers, allTrackErrors, severities })` | `DrcSummary` of a run of only the named providers (`clearance`, `connectivity`, `hole_to_hole_clearance`, `edge_clearance`, `width`, `diameter`, `annular_width`, `disallow`, `hole_size`, ... = `DRC_TEST_PROVIDER::GetName()`), markers untouched - for the inner loop |

## The router loop (worked example)

```js
const kicad = require('./web/pkg-node/kicad_wasm.js');
const b = kicad.loadBoard(pcbText, { name: 'board.kicad_pcb', project: proText });

// 1. baseline DRC
const baseline = JSON.parse(b.runDrc({ format: 'json' }).report);

// 2. extract
const info = b.boardInfo();                       // layers, outline, rule minimums
const pads = b.pads({ polygons: true });          // effective copper per layer
const fps = b.footprints();                       // courtyards
const zones = b.zones({ polygons: true });        // keepouts (ruleArea + keepout), GND fills
const nets = b.netDetails();
const gap = b.constraint({ kind: 'clearance', a: { net: 3 }, b: { net: 5 }, layer: 'F.Cu' }).min;
const sizes = b.routerSizes(3, 'F.Cu');           // trackWidth, viaDiameter, viaDrill
const todo = b.connectivity();                    // ratsnest edges and clusters

// 3. route with your engine, then write the candidate back in one call
const r = b.applyEdits({
    removeRouting: { keepLocked: true },
    add: [
        { kind: 'track', start: [100e6, 50e6], end: [102e6, 50e6], layer: 'F.Cu', net: 3 },
        { kind: 'arc', start: [102e6, 50e6], mid: [103e6, 50.4e6], end: [104e6, 51e6],
          layer: 'F.Cu', net: 3 },
        { kind: 'via', at: [104e6, 51e6], net: 3 },          // router default sizes
        // ... ~1000 more
    ],
});
console.log(`removed ${r.removed.length} added ${r.added.length}`);

// 4. stitching vias, teardrops
b.applyEdits({ add: stitchPoints.map((at) => ({ kind: 'via', at, net: gndCode })) });
const td = b.updateTeardrops();                   // { teardrops, candidates, without }

// 5. inner-loop check, then the full check
let q = b.quickDrc({ providers: ['clearance', 'connectivity'] });
// ... move offending items with changeItem / applyEdits({ change: [...] }) and repeat
b.refillZones();
const report = b.runDrc({ format: 'json' });      // the KiCad JSON report for download
const clean = b.drcViolations({}).violations.length === 0;
const routed = b.save();                          // the routed .kicad_pcb
```

`web/test/router_loop.mjs` runs this loop (record, remove all routing, re-add, teardrops,
refill, DRC, save) on three boards and compares every result with the native run.

## Live router loop (package LIVE-1)

A candidate loop per routing attempt: start from the same base board each time, edit in
transactions (connectivity is updated incrementally, the ratsnest of the dirty nets only),
read connectivity / quick DRC, roll back; for the winner, `finalize()` before the full refill,
DRC and save.

| Call | Does |
|---|---|
| `new Transaction()` | an empty staged commit; `remove(handles)` (`Float64Array` or numbers), `removeRouting(opts)`, `change(ItemChange)`, `add(NewItem)`, `stage(EditBatch)`, `addTracks(Int32Array)`, `addArcs(Int32Array)`, `addVias(Int32Array)`; `length`; `append = true` adds at the end of the track list (`ADD_MODE::APPEND`) instead of the front (`BULK_INSERT`, as `Push`); `skipConnectivity = true` is `SKIP_CONNECTIVITY` |
| `commit(txn)` | `BOARD_COMMIT::Push`: entries in staging order (`Add` / `Remove` / `MarkItemNetAsDirty` + `Update`), `RecalculateRatsnest`, `UpdateRatsnestExclusions`; returns `CommitReport` = `EditResult` + `{ dirtyNets, netUnconnected: [[net, edges]], unconnected, unconnectedVisible, netChanges: [{ id, from, to }] }` (`netChanges`: items whose net the propagation changed) |
| `commitBulk(txn)` | the same, typed arrays: `added` / `removed` / `changed` `Float64Array`, `dirtyNets` `Int32Array`, `netUnconnected` `Int32Array` pairs, `netChanges` `Float64Array` triples `[id, from, to]`, no UUID strings |
| `checkpoint()` | saves board and connectivity (deep copy), returns a token |
| `rollback(token)` | restores board and connectivity exactly: same items, handles, UUIDs, nets, fills, UUID generator state; the checkpoint stays usable; `releaseCheckpoint(token)` frees it |
| `finalize()` | `BOARD::BuildConnectivity()`, what a reload of the saved board computes |
| `routeRecords()` | `{ trackIds, tracks, arcIds, arcs, viaIds, vias }`: the routing as flat records in board order |

Flat records (`Int32Array`, layers as `PCB_LAYER_ID` values: `F.Cu` 0, `B.Cu` 2, `In1.Cu` 4,
...): track `x1, y1, x2, y2, width, layer, net` (7), arc the same plus `midX, midY` (9), via
`x, y, diameter, drill, top, bottom, net, type` (8; type 0 through, 1 blind, 2 buried, 3
micro). Every other property is the `NewTrack` / `NewArc` / `NewVia` default.

```js
const base = b.checkpoint();
for (const candidate of candidates) {
    b.rollback(base);
    const t = new Transaction();
    t.remove(candidate.ripUp);            // Float64Array of handles
    t.addTracks(candidate.segments);      // Int32Array, 7 values per segment
    t.addVias(candidate.vias);            // Int32Array, 8 values per via
    const r = b.commitBulk(t);            // incremental connectivity
    score(candidate, r.unconnected, b.quickDrc({ providers: ['clearance'] }));
}
b.rollback(base);
b.commit(best);                           // or replay the best candidate
b.finalize();                             // full rebuild, as a reload would
b.updateTeardrops(); b.refillZones(); b.runDrc({ format: 'json' }); b.save();
```

Semantics. The incremental state is the reference's own incremental state: after every commit,
nets, clusters, ratsnest nodes and edge weights, unconnected counts, dangling ends and
`IsConnectedOnLayer` equal what KiCad's `BOARD_COMMIT::Push` leaves (verified against the
reference build, `crates/kicad-session/tests/live1_oracle.rs`). Like KiCad, that state can
differ from a full rebuild of the same board (net propagation and the ends of ratsnest edges
depend on the build history); `finalize()` is the full rebuild. Teardrops are not updated by
a commit (`SKIP_TEARDROPS`): call `updateTeardrops()`.

Cost (release / wasm-release, medium board `complex_hierarchy`, median of 60 commits that
remove or re-add a tenth of the routing): commit 0.50 ms native / 0.99 ms wasm, against 4.5 /
8.0 ms with a full rebuild per commit; checkpoint 0.9 / 1.2 ms; rollback 1.6 / 2.5 ms
(`docs/PERF.md`, LIVE-1).

## Scoped checks and affected-only refill (package LIVE-2)

| Call | Does |
|---|---|
| `checkItems(handles, { providers?, severities? })` | the violations that involve one of the items: the subset of a run of the providers (default `annular_width`, `disallow`, `connectivity`, `clearance`, `edge_clearance`, `hole_to_hole_clearance`, `width`, `diameter`) with `aReportAllTrackErrors = true` whose entries name one of the handles; the connectivity entries `unconnected_items`, `isolated_copper`, `track_dangling`, `via_dangling` also when an item they name has the net of one (after the run); `isolated_copper` (it names the zone only) also when the zone's bounding box meets a handle's, inflated by the worst clearance, on a shared layer; returns `DrcSummary`, whose `providersNotRun` lists every DRC provider not run (their violations are not in the result: in particular `physical_clearance`, whose entries carry the same `clearance` / `hole_clearance` types, `solder_mask_issues`, `hole_size`, `copper width`, `miscellaneous`, `track angle`, `segment_length`, `microvia_stack`, `via_stitch_overlap`, `zone connections`, `length`, `diff_pair_coupling`, `creepage`, `sliver checker`); exclusions are not resolved |
| `refillZonesAffected()` | `FillAllZones` with `ZONE_FILLER::Fill` on the zones the changes since the last refill can affect; the board ends up as `refillZones()` leaves it, fills vertex for vertex, flags, teardrops and saved text; returns `RefillReport { full, refilled, reused, kept, reasons }` |
| `zoneDamage()` | the changes the next `refillZonesAffected()` refills for (`ChangeDamage { full, areas: [{ bbox, layers }], nets }`) |
| `commit(t).damage`, `quickDrc(..).damage` | what a commit changed (areas and nets of removed, changed, added and re-netted items), and the nets a DRC run's connectivity build gave to items |

The session keeps the damage itself (natively too: `Session::refill_zones_affected( extra )`
refills for what the session recorded plus `extra`): every `commit` / `commitBulk`, `quickDrc`,
`checkItems` and `finalize` adds what it changed, `checkpoint` / `rollback` save and restore it
with the board, a refill (`refillZones()` included) resets it. The edit calls (`applyEdits`,
`addTrackSegment`, `addArc`, `placeVia`, `changeItem`, `removeItems`, `removeRouting`) are
commits and are tracked like `commit`. The record is valid only while the board's time stamp
and the rule text are those the last of these calls left, so any other change that bumps the
time stamp (`updateTeardrops` when it adds, changes or removes a teardrop - on a board without
a teardrop target it changes nothing and the record stays known -, the markers of `runDrc`,
design and teardrop settings, a direct edit of `Session::board` through a `Board` mutator
natively, ...) and the load make the next affected refill a full one. Writers of derived state
are excluded: `Board::zone_fill_mut` (fill polygons and flags, written by the filler and the
connectivity) does not bump the time stamp, as in C++, so fills written through it natively
outside a refill are not noticed. The refill is also a full one when a commit removed a pad or
a zone (rule areas and teardrops included: they set the worst clearance, knock holes out of
every copper layer, or change rule clearances by area membership), when the board's worst
clearance differs from that of the last refill, and when the custom rule text contains
`fromTo` in any letter case (every DRC run rebuilds the from-to cache that function reads; the
test is on the text because the compiled conditions of `kicad-drc` do not expose the functions
they call). The precondition of the identity, fills that are those of a refill of the board
before the changes, is therefore checked by the session for every tracked or time-stamped
change, not left to the caller.

Which zones are refilled (`kicad_zone::affected`): the items the refill's own connectivity
build gives another net (a commit with `skipConnectivity`, a pad-less cluster taking the net
of its first item) are changed areas too; every zone sharing a layer with a changed
area within the knockout reach (twice the worst clearance and thermal relief, minimum width,
corner radii, 1 mm), every zone of a net whose connectivity cluster near a change meets the
zone (islands), every unfilled zone and every new teardrop; closed under the
`zoneKnockoutMayInteract` coupling of the filler (a teardrop is pulled in only by other
teardrops: it never reads a regular zone's fill) and under the nets of island-tracking zones.
Re-created teardrops (`FillAllZones` re-creates all of them) identical to their predecessor
keep its fill. A single ground pour that covers the board is refilled by any edit inside it,
so the gain is on boards with several zones and edits away from the big pours:

```js
const base = b.checkpoint();                 // after a refillZones()
for (const c of candidates) {
    b.rollback(base);                        // board, fills and damage back
    const r = b.commitBulk(c.txn);
    const v = b.checkItems(r.added);         // only the new routing's violations
    b.refillZonesAffected();                 // only the zones the candidate touches
    score(c, r.unconnected, v);
}
```

The DRC itself keeps no state between runs beyond the compiled rules (`DRC_ENGINE`, reused
while valid): each `quickDrc` / `checkItems` / `runDrc` runs the cache generator (connectivity
build, r-trees) as `RunTests` does, so `checkItems` costs a run of its providers (it saves
nothing over `quickDrc` with the same providers; it only filters). A persistent cache
generator is not built (package item 1 is open, see `crates/kicad-session/PORT_STATUS.md`,
LIVE-2).

## Verification (package WASM-API-1)

* Read API vs the reference `pcb-dump board --loader` (`crates/kicad-session/tests/board_api_read.rs`,
  golden `tests/golden/board_api_read.txt`): 15 750 values on 32 boards (custom rules, blind /
  micro vias, slots, custom pads, rule areas, teardrops) identical, pad polygons vertex for
  vertex, courtyards, zone outlines and fills.
* `constraint` vs `DRC_ENGINE::EvalRules` (`tests/board_api_rules.rs`, golden from
  `pcb-dump --sections evalrules`): 530 sampled single-item and item-pair constraints on 9
  boards with rule files identical.
* Round trip (`tests/board_api_round_trip.rs`): record, remove all routing, re-add with the
  recorded UUIDs, save: byte-identical to the save of the loaded board (which equals the
  reference re-save, `upgrade_corpus.rs`) on 290 corpus boards; DRC JSON identical on ecc83
  and 20 corpus boards.
* `updateTeardrops` vs the reference (`tests/board_api_edit.rs`, cached
  `kicad-cli pcb drc --refill-zones --save-board` boards): the teardrop zones of 72 boards
  identical (as saved).
* `quickDrc( clearance, connectivity )` equals the full DRC's violations of those providers
  (3 boards).
* Native vs wasm: `web/test/router_loop.mjs` vs `crates/kicad-wasm/tests/router_loop.rs`
  (3 boards: saved board and DRC JSON hashes, counts).

## Known limits

* Group / generator membership of a removed track (tuning patterns, via-stitching
  generators) is not restored when the track is re-added.
* `NewVia` has no per-layer padstack (`padstack` mode other than normal), backdrill or
  post-machining; a via using them is re-created with a plain padstack.
* Footprint fields on copper layers are not in `copperObstacles()`.
* Right after `updateTeardrops()` the in-memory outline of a few teardrop zones
  (`teardrop_issue_JPC2`, `teardrop_qfn_thermal_pad_zone`) read through `zones()` differs from
  the same board saved and re-read (the saved board equals the reference); not investigated.
