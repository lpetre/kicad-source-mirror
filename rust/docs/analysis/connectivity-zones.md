# Connectivity, ratsnest and zone filling - porting analysis

Analyst document for the port of `pcbnew/connectivity/`, `pcbnew/ratsnest/ratsnest_data.*`,
`pcbnew/zone_filler.*` and the parts of `pcbnew/zone.cpp` / `pcbnew/teardrop/` that the filler
needs, to the Rust crates `kicad-connectivity` and `kicad-zone`. The C++ tree is the
specification; all line numbers refer to the files as checked in (KiCad master, board file
format 20261031). `zone_filler.cpp:L1314` means line 1314 of `pcbnew/zone_filler.cpp`;
`algo.cpp` = `pcbnew/connectivity/connectivity_algo.cpp`, `data.cpp` =
`connectivity_data.cpp`, `items.*` = `connectivity_items.*`, `rn.cpp` =
`pcbnew/ratsnest/ratsnest_data.cpp`.

Read `rust/PORTING.md` (esp. section 9) and `rust/docs/analysis/drc.md` (sections 3.8, 4.5,
4.12, 5.4) first. This document gives the map, the traps and the work packages; an implementer
reads this document plus the C++ files of the package.

**Reading guide**

| If you implement | Read |
|---|---|
| any package | 1, 3.1, 4.0, 8, 10.1, and the R-items of 11 that name your files |
| CN-01 / CN-02 (items, build, clusters, nets) | 3.1-3.4, 4.1-4.4 |
| CN-03 (ratsnest) | 3.5, 4.5 |
| CN-04 (CONNECTIVITY_DATA queries) | 4.6, 4.7 |
| ZF-* (zone filler) | 3.6-3.7, 4.8-4.15, 5.2-5.4 |
| TD-* (teardrops) | 4.16 |
| tests | 9 |

Contents: 1 Purpose/entry points - 2 File inventory - 3 Data structures - 4 Control flow and
algorithms - 5 Needs from other subsystems - 6 Output formats - 7 Backward compatibility -
8 Multi-threading - 9 Tests and differential tests - 10 Rust layout and work packages - 11 Risks.

---

## 1. Purpose and entry points

**Connectivity** (`CONNECTIVITY_DATA` + `CN_CONNECTIVITY_ALGO`) answers "which copper items
physically touch", groups them into clusters (connected components), *propagates nets* from pads
onto tracks/vias/shapes, and feeds the ratsnest. **Ratsnest** (`RN_NET`, one per net) computes
the unrouted connections between the clusters of a net (Delaunay triangulation + Kruskal MST);
these are DRC's "unconnected items". **Zone filler** (`ZONE_FILLER`) computes the copper fill
polygons of every zone (`filled_polygon` in the file), using DRC rules for clearances.

### 1.1 Callers that matter for the port

| Caller | Calls | Effect on output |
|---|---|---|
| `BOARD_LOADER::Load...` (`pcbnew/board_loader.cpp:L151`) | `BOARD::BuildConnectivity()` then `BuildListOfNets()`, `SynchronizeNetsAndNetClasses( true )` | **net propagation changes track/via/shape net codes that the writer saves** |
| `BOARD_LOADER::SaveBoard` (`board_loader.cpp:L186`) | `BuildConnectivity()` before save | same |
| `kicad-cli pcb drc` (`pcbnew_jobs_handler.cpp:L2993-2998`, L3065) | `--refill-zones`: `ZONE_FILLER_TOOL::FillAllZones( nullptr, reporter, true )`; `--save-board`: `BOARD_LOADER::SaveBoard` | new fills, teardrops, flashing overrides |
| DRC cache generator / providers | `BuildConnectivity`, `FillIsolatedIslandsMap`, `TestTrackEndpointDangling`, `RunOnUnconnectedEdges`, `GetConnectedItems`, `IsConnectedOnLayer` (through `PAD/PCB_VIA::FlashLayer`) ... | see drc.md 5.4 |
| `PAD::FlashLayer`, `PCB_VIA::FlashLayer` (`pad.cpp:L697`, `pcb_track.cpp:L2356`) | `GetConnectivity()->IsConnectedOnLayer( this, layer, {TRACE, ARC, VIA, PAD} )` | **shapes of vias/pads with "remove unconnected layers" - used by DRC, plotting and the filler** |
| Plot/gerber export with `--check-zones` | `FillAllZones` (`pcbnew_jobs_handler.cpp:L1472/1781/2260/3105`) | fills in plots |

`BOARD::BuildConnectivity( reporter )` (`board.cpp:L369-376`) = `GetConnectivity()->Build( this )`
then `UpdateRatsnestExclusions()` (`board.cpp:L538-566`, sets `CN_EDGE::m_visible = false` for
edges whose (source parent UUID, target parent UUID) pair matches an excluded
`MARKER_RATSNEST` marker, both orientations).

### 1.2 `FillAllZones` headless flow (the `--refill-zones` path), in execution order

`tools/zone_filler_tool.cpp:L180-270`:

1. `TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, forceFullUpdate = true )`
   (`teardrop.cpp:L437`): unless `board->LegacyTeardrops()`, **deletes every teardrop zone** and
   regenerates teardrops for every track end on pads/vias with enabled `TEARDROP_PARAMETERS`
   (4.16). Uses the load-time connectivity.
2. `board->IncrementTimeStamp()` (invalidates bbox / clearance caches).
3. `toFill = board->Zones()` (board-level zones only, in board order; **footprint zones are not
   filled by this path**).
4. `ZONE_FILLER( board, &commit ).Fill( toFill )` (4.8). `aCheck = false`.
5. On success: `board->OnZonesFilled( toFill )` (generators, e.g. via stitching -
   `PCB_GENERATOR::OnZoneFillChanged`), `RegenerateDirtyGenerators`, `commit.Push(
   SKIP_CONNECTIVITY | ZONE_FILL_OP )`, `refillAroundGenerators( regenerated )` (refills zones a
   regenerated generator names; out of scope until generators are ported - see R-17).
6. `PostFillRefresh( headless )` -> `board()->BuildConnectivity()` (full rebuild, 4.1).

Then the DRC run (its cache generator rebuilds connectivity again), then `SaveBoard` (rebuilds
again, then writes).

---

## 2. File inventory

| File | Lines | Role | Scope |
|---|---|---|---|
| `pcbnew/connectivity/connectivity_items.h` | 586 | `CN_ANCHOR`, `CN_ITEM`, `CN_ZONE_LAYER`, `CN_LIST` (+ bulk load), `CN_CLUSTER` | T1 |
| `pcbnew/connectivity/connectivity_items.cpp` | 511 | anchors per item type, `CN_LIST::Add(...)`, `IsDangling`, `ConnectedItemsCount`, `CN_CLUSTER::Add` (origin net ranking) | T1 |
| `pcbnew/connectivity/connectivity_rtree.h` | 124 | `CN_RTREE<T>` = `KIRTREE::DYNAMIC_RTREE<T,int,3>` over (copper ordinal, x, y) | T1 |
| `pcbnew/connectivity/connectivity_algo.h` | 372 | `CN_EDGE`, `CN_CONNECTIVITY_ALGO`, `ITEM_MAP_ENTRY`, `CN_VISITOR`, `ItemsTouchOnLayer` | T1 |
| `pcbnew/connectivity/connectivity_algo.cpp` | 1147 | Add/Remove, `Build`, `searchConnections`, `SearchClusters`, `PropagateNets`, `FillIsolatedIslandsMap`, `CN_VISITOR` contact rules, jumper pads | T1 (`LocalBuild` L659-683: T3) |
| `pcbnew/connectivity/connectivity_data.h` | 315 | `CONNECTIVITY_DATA` facade, `RN_DYNAMIC_LINE`, `CN_DISJOINT_NET_ENTRY`, `PROPAGATE_MODE` | T1 |
| `pcbnew/connectivity/connectivity_data.cpp` | 1153 | `Build`, ratsnest recalculation, all queries (`IsConnectedOnLayer`, `TestTrackEndpointDangling`, `GetConnectedItems`, `GetNetItems`, ...) | T1; local ratsnest (`ComputeLocalRatsnest` L332-416, `Move`, `BlockRatsnestItems`, `GetRatsnestForItems/Pad/Component`) T3 (GUI) |
| `pcbnew/connectivity/from_to_cache.h/.cpp` | 77 / 280 | pad-to-pad path cache for the `fromTo()` rule function | T2 |
| `pcbnew/connectivity/topo_match.h/.cpp` | 262 / 1485 | multichannel topology matching (layout replication tool) | **out of scope** (GUI tool, no CLI output) |
| `pcbnew/ratsnest/ratsnest_data.h` | 166 | `RN_NET`, `CN_PTR_CMP` (node order), `GetEdges()` stable sort | T1 |
| `pcbnew/ratsnest/ratsnest_data.cpp` | 668 | triangulation, Kruskal MST, zone-anchor optimisation, `AddCluster` | T1 (`NearestBicoloredPair` L596-668: T3, GUI) |
| `pcbnew/ratsnest/ratsnest_view_item.*` | 416 / 118 | drawing | **out of scope** (GAL) |
| `libs/core/include/core/filter_kruskal.h` | 135 | `KI_MST::FilterKruskal` (threshold 1024, `nth_element` split) | T1 |
| `libs/core/include/core/union_find.h` | 174 | `KI_UNION_FIND` (root = minimum index of the component) | T1 |
| `thirdparty/delaunator/delaunator.hpp/.cpp` | 147 / 648 | Delaunay triangulation | T1 (hand port in kicad-geom G7, `docs/research/spatial.json`) |
| `pcbnew/zone_filler.h` | 300 | `ZONE_FILLER` state, caches, `INDEXED_ITEM`, `ITEM_RTREE = PACKED_RTREE<INDEXED_ITEM,int,2>` | T2 |
| `pcbnew/zone_filler.cpp` | 4820 | the whole fill pipeline (map in 4.8) | T2 (thieving L4158-4352: T3; `DUMP_POLYS_TO_COPPER_LAYER` debug: never) |
| `pcbnew/zone.cpp` (subset) | ~600 of 2699 | `HigherPriority`, `SameNet`, `UnFill`, bbox, hashes, `GetBoardOutline`, `HitTestFilledArea`, `CacheTriangulation`, `IsIsland`, `GetInteractingZones`, `BuildSmoothedPoly`, `TransformSmoothedOutlineToPolygon`, `GetEffectiveShape`, `TransformShapeToPolygon` | board model (T1 parts) + ZF-01 (T2) |
| `pcbnew/tools/zone_filler_tool.cpp` (subset) | ~120 | `FillAllZones`, `PostFillRefresh` | T2 |
| `pcbnew/teardrop/teardrop.cpp/.h` | 809 / 355 | `TEARDROP_MANAGER::UpdateTeardrops` and creation | T2 (4.16) |
| `pcbnew/teardrop/teardrop_utils.cpp` | 1755 | teardrop outline geometry | T2 |
| `pcbnew/teardrop/teardrop_parameters.*`, `teardrop_types.h` | 58+187+38 | parameters | T2 |
| `libs/kimath/include/geometry/vertex_set.h` + `src/geometry/vertex_set.cpp` | 374 / 395 | `VERTEX_SET` (z-order linked list, `isEar`) used by `VERTEX_CONNECTOR` | kicad-geom (G4b triangulation) |
| `libs/kimath/include/geometry/poly_ystripes_index.h` | 280 | `POLY_YSTRIPES_INDEX::Build/Contains( pt, accuracy )` | kicad-geom |
| `libs/kimath/include/geometry/shape_poly_tri_index.h` | 223 | `TRIANGULATION_INDEX`, `AnyTriangleHit` | kicad-geom |
| `libs/kimath/include/geometry/rtree/dynamic_rtree.h`, `packed_rtree.h`, `rtree_node.h` | 1812 / 528 / 719 | R-trees | kicad-geom G7 |

---

## 3. Data structures

### 3.1 `CN_ITEM` (`items.h:L95-255`)

One per connectable board item (one per (zone, layer, fill outline) for zones). Fields:
`m_parent: BOARD_CONNECTED_ITEM*`, `m_connected: vector<CN_ITEM*>` (**kept sorted by pointer
value**: `Connect()` uses `std::lower_bound` and skips duplicates, `items.h:L170-178`),
`m_anchors: vector<shared_ptr<CN_ANCHOR>>`, `m_canChangeNet`, `m_valid` (lazy removal),
`m_dirty` (atomic), `m_start_layer` / `m_end_layer` (**copper ordinals**, `SetLayers` maps through
`CopperLayerToOrdinal`: F_Cu -> 0, B_Cu -> 31 (`MAX_CU_LAYERS - 1`), InN_Cu -> N;
`include/layer_ids.h:L986-996`; defaults 0 / INT_MAX), `m_bbox` (cached `GetBoundingBox()` while
dirty), `m_listIndex` (position in `CN_LIST`).

`Layer()` = `StartLayer()` (ordinal); `GetBoardLayer()` = `BoardLayerFromOrdinal( Layer() )`
(0 -> F_Cu, >= 31 -> B_Cu, else `B_Cu + 2*ord`). `Net()` = parent net code or -1 if invalid.

Per type (`items.cpp:L139-286`):

| Parent | anchors | layers | `canChangeNet` |
|---|---|---|---|
| `PAD` (only if `IsOnCopperLayer()`) | **unique `ShapePos( layer )` over `Padstack().ForEachUniqueLayer`, inserted into `std::set<VECTOR2I>`** (so sorted by (x, y), duplicates merged); `AnchorCount()` = 1 and `GetAnchor()` = `GetPosition()` regardless (`items.cpp:L36-84`) | F_Cu..B_Cu; for SMD / NPTH / CONN / NON_COMPONENT attribute: the first layer of `GetLayerSet().CuStack()` | false |
| `PCB_TRACK` | start, end | `GetLayer()` | true |
| `PCB_ARC` | start, end | `GetLayer()` | true |
| `PCB_VIA` | `GetStart()` | `TopLayer()..BottomLayer()` | `!GetIsFree()` |
| `PCB_SHAPE` (copper layer only) | `GetConnectionPoints()`; `AnchorCount()` = number of anchors | `GetLayer()` | true |
| `CN_ZONE_LAYER` | none when created by `Build()`; every vertex of fill outline `j` when added by `CN_LIST::Add( ZONE*, layer )` (R-5). `AnchorCount()` = 1 and `GetAnchor(0)` = first outline vertex either way; `RN_NET::AddCluster` uses at most `anchors[0]` | the zone layer | false |

### 3.2 `CN_ANCHOR` (`items.h:L40-90`)

`m_pos`, `m_item` (owning `CN_ITEM*`), `m_tag` (scratch, set by the ratsnest), `m_noline`
(blocked ratsnest target, GUI), `m_cluster: shared_ptr<CN_CLUSTER>` (set by
`RN_NET::AddCluster`). `Dist()` = `( a - b ).EuclideanNorm()` -> `unsigned` (int vector norm =
`RoundedHypot`, saturating; kicad-geom G1).

### 3.3 `CN_ZONE_LAYER` (`items.h:L258-395`)

`m_zone`, `m_subpolyIndex` (outline index in `GetFilledPolysList( layer )`), `m_layer`,
`m_outline` (copy of the outline, no holes - fills are fractured), `m_triangulatedPolys`
(copies of the zone fill's cached triangulation polygons whose `GetSourceOutlineIndex() ==
m_subpolyIndex`) and `m_triIndex` (`TRIANGULATION_INDEX`). Contact tests:
`ContainsPoint(p)` / `Collide(shape)` use `AnyTriangleHit` over the triangles - **except
teardrop zones, which use `m_outline.Collide(p)` / `shape->Collide( &m_outline, 0 )`** and do not
build indexes. Triangle containment differs from polygon containment on edges, so the fill
triangulation must be ported exactly (kicad-geom G4b, `SHAPE_POLY_SET::CacheTriangulation(
false )`).

`HasSingleConnection()` (`items.cpp:L116-131`): exactly one valid connected item.

### 3.4 `CN_LIST`, `ITEM_MAP_ENTRY`, `CN_CLUSTER`

* `CN_LIST` (`items.h:L398-540`): `m_items: vector<CN_ITEM*>` (**the list order is the iteration
  order of everything downstream**), `m_index: CN_RTREE<CN_ITEM*>`, `m_dirty`, `m_hasInvalid`,
  `m_bulkAdd`. `FindNearby( item, f )` queries the tree with `item->BBox()` and the item's
  ordinal range. During `Build` a `BULK_ADD_SCOPE` defers insertion and bulk-loads the tree at
  the end (`FinishBulkAdd`). R-tree visit order is irrelevant (see 4.2).
* `ITEM_MAP_ENTRY` (`algo.h:L180-206`): `std::list<CN_ITEM*>`; map
  `std::unordered_map<const BOARD_ITEM*, ITEM_MAP_ENTRY> m_itemMap` (lookups only).
* `CN_CLUSTER` (`items.h:L543-584`, `items.cpp:L411-511`): `m_items` (in insertion order),
  `m_originNet`, `m_originPad`, `m_conflicting`, `m_netRanks` (lookups only). `Add( item )`:
  push; if `net <= 0` stop; first item with net > 0 sets `m_originNet` (rank 0). For a pad that is
  not `IsFreePad()`: rank of its net += 1; **if no origin pad yet, or rank > rank of the current
  origin net (strict), this pad becomes origin** and `m_originNet = its net`; then if the pad's
  net != origin net -> `m_conflicting = true`. `IsOrphaned()` = no origin pad.
  `HasValidNet()` = `m_originNet > 0`.

### 3.5 `CN_EDGE` and `RN_NET`

* `CN_EDGE` (`algo.h:L64-160`): source/target `shared_ptr<const CN_ANCHOR>`, `m_weight:
  unsigned`, `m_visible`. `StableSortCompare`: source pos x, then y, then weight, then visible
  first.
* `RN_NET` (`rn.h:L59-165`): `m_nodes: std::multiset<shared_ptr<CN_ANCHOR>, CN_PTR_CMP>`
  (`CN_PTR_CMP` = (x, y) lexicographic; **equal positions keep insertion order** - libstdc++
  multiset inserts at the upper bound), `m_boardEdges` (weight-0 edges joining all anchors of one
  cluster), `m_rnEdges` (result), `m_dirty`, `m_triangulator`. **`GetEdges()` stable-sorts
  `m_rnEdges` with `StableSortCompare` on every call** (`rn.h:L95-114`); `GetUnsortedEdges()`
  does not. `GetNodeCount()` = `m_nodes.size()`.

### 3.6 `ZONE` fill state (`zone.h`)

`m_FilledPolysList: std::map<PCB_LAYER_ID, shared_ptr<SHAPE_POLY_SET>>`, `m_filledPolysHash:
map<layer, HASH_128>`, `m_insulatedIslands: map<layer, set<int>>` (outline indices flagged
`island`), `m_fillFlags: LSET bitset` (per-layer "fill done" for the scheduler), `m_isFilled`,
`m_needRefill`, `m_area`. Accessors: `GetFilledPolysList( layer )` (asserts presence),
`HasFilledPolysForLayer`, `SetFilledPolysList( layer, poly )` (**replaces the shared_ptr**),
`SetIsIsland( layer, idx )` (insert only), `IsIsland( layer, idx )` (**true for every outline if
net code < 1**, `zone.cpp:L1769-1778`), `UnFill()` (`zone.cpp:L565-593`: every layer gets a new
empty poly set, islands cleared, `m_isFilled = false`, fill flags reset).

Footprint zones keep their outline in **library coordinates** (`m_Poly`); `GetBoardOutline()`
(`zone.cpp:L953-975`) applies `fp->GetTransform()` to every vertex; `GetBoundingBox()` is the
board-outline bbox for footprint zones and `m_Poly->BBox()` otherwise (`zone.cpp:L815-856`,
cached per board timestamp). Fills are always in board coordinates.

### 3.7 `ZONE_FILLER` state (`zone_filler.h:L175-300`)

`m_board`, `m_boardOutline` + `m_brdOutlinesValid` (from `GetBoardPolygonOutlines( outline,
true )`), `m_maxError = bds.m_MaxError`, `m_worstClearance = board->GetMaxClearanceValue()`
(`board.cpp:L1325-1355`), `m_zoneKnockoutSlack = mmToIU( ExtraClearance = 0.0005 ) + m_maxError`
(i.e. 500 nm + maxError), `m_maxZoneCornerRadius`, five indexes
(`m_graphicIndex`, `m_footprintIndex`, `m_padIndex`, `m_trackIndex[layer]`, `m_zoneIndex[layer]`),
and four caches keyed by (zone, layer): `m_preKnockoutFillCache` (**not cleared per `Fill()`**),
`m_preHatchSolidFillCache`, `m_sameNetApronCache`, `m_refillResultCache` (cleared at the start
of every `Fill()`, `zone_filler.cpp:L596-598`).

`INDEXED_ITEM { m_item, m_owner (footprint of a graphic), m_seq }`. **`queryIndex` always sorts
hits by `m_seq`** (`zone_filler.cpp:L404-429`), so R-tree visit order never reaches the output.
`buildItemIndexes` (L432-516) assigns `m_seq`:
* graphics + footprints share one counter: per footprint in board order: footprint (index
  `footprints`, seq = current, not incremented), reference (seq++), value (seq++), every graphical
  item (seq++); then board drawings (seq++). Pads have their own counter in footprint/pad order.
* tracks: one seq per track in `board->Tracks()` order, inserted into `m_trackIndex[layer]` for
  every layer of `GetLayerSet() & AllCuMask( copperLayerCount )`.
* zones: board zones then footprint zones (`forEachBoardAndFootprintZone`, L342-353), one seq per
  zone, in `m_zoneIndex[layer]` for every layer of `GetLayerSet()`.

Advanced config defaults that the filler reads (`common/advanced_config.cpp`):
`m_ExtraClearance = 0.0005` mm, `m_ZoneFillIterativeRefill = true`, `m_DebugZoneFiller = false`,
`m_TriangulateSimplificationLevel = 50` (used by `VERTEX_CONNECTOR`'s `VERTEX_SET`). The port
hard-codes these defaults (no `kicad_advanced` file support).

---

## 4. Control flow and algorithms

### 4.0 Order-sensitivity summary (where iteration order reaches the output)

| Place | Container / sort | What it affects | Port rule |
|---|---|---|---|
| `CN_LIST` order (4.1) | build order: zone layers, tracks, pads, copper drawings | cluster order, origin net of pad-less clusters, ratsnest node insertion | reproduce exactly |
| `CN_ITEM::m_connected` (3.1) | sorted by **heap address** | `GetConnectedPadsAndVias` order (teardrop creation order, 4.16), "first" connected item in DRC providers, `OptimizeRNEdges` zone candidate order on ties | see R-1 |
| `SearchClusters` final `std::sort` by `OriginNet` (algo.cpp:L507-512) | unstable sort, many ties | cluster order inside a net -> ratsnest node order, tags, MST ties, `m_boardEdges` order | `kicad_geom::stdsort` (libstdc++ port) |
| `deferredNetCodes` `std::sort` by `CN_ITEM*` (algo.cpp:L326) | address order of groups | none (each group independent; result is min or existing net) | any order |
| `TRIANGULATOR_STATE` chain sort by `CN_CLUSTER*` (rn.cpp:L254-259) | **cluster heap address** | weight-1 edges between coincident anchors of different clusters | R-2 |
| `GetNetItems` (`std::sort` + `unique` by pointer, data.cpp:L629-630), `GetConnectedTracks` / `GetConnectedPads` (`std::set<T*>`) | address order | callers' iteration order (DRC, teardrops) | R-1 |
| `ZONE_FILLER` index queries | sorted by `m_seq` | knockout append order | reproduce |
| `isolatedIslandsMap` `std::map<ZONE*,...>`, `changedZoneLayers` `std::set<pair<ZONE*,layer>>` (zone_filler.cpp) | address order | `FillIsolatedIslandsMap` Remove/Add order (connectivity list order only); refill candidate order | no effect on fills (each zone independent; refills read a snapshot) - any deterministic order |
| `unordered_set` knockout dedup keys | membership only | none | any |
| Polygon append order before every boolean | vector order | **Clipper2 output vertex order and start point** | reproduce append order exactly |

### 4.1 Full build: `CONNECTIVITY_DATA::Build( board )` (data.cpp:L100-143)

1. `aBoard->CacheTriangulation()` (`board.cpp:L1357-1420`): for every board zone and every layer
   in `GetFilledLayers()` (keys of `m_FilledPolysList`), `zone->CacheTriangulation( layer )` ->
   `poly->CacheTriangulation( false )`.
2. Delete `m_nets`; new `CN_CONNECTIVITY_ALGO`; `m_connAlgo->Build( board )` (algo.cpp:L518-657):
   * for each **board** zone with `IsOnCopperLayer()`: create its map entry, mark its net dirty;
     for each layer of `GetEnabledLayers() & GetLayerSet() & AllCuMask()` (LSET order = layer id
     order: F_Cu 0, B_Cu 2, In1 4, ...), for each fill outline `j`: `new CN_ZONE_LAYER( zone,
     layer, j )`; then `BuildIndexes()` for all (parallel, independent); then `m_itemList.Add(
     zitem )` and link in the map, **in creation order**. Note: `CN_LIST::Add( ZONE*, layer )`
     (items.cpp:L236-256, used by incremental `Add`) also adds every outline vertex as anchor;
     the `Build` path does not: its zone items have **no `m_anchors`** (while `AnchorCount()`
     still returns 1 from the outline), see R-5.
   * `for track in board->Tracks()`: `Add( track )` (tracks, arcs, vias in board order).
   * `for fp in Footprints(), for pad in fp->Pads()`: `Add( pad )` (skips non-copper pads,
     `FP_JUST_ADDED` footprints).
   * `for drawing in board->Drawings()`: if `PCB_SHAPE` and `IsOnCopperLayer()`: `Add( shape )`
     (requires `IsCopperLayer( GetLayer() )`). **Footprint graphics and footprint zones are never
     connectivity items.**
   * `Add()` marks the item's net dirty (`MarkNetAsDirty`: grows `m_dirtyNets` to `net + 1`,
     setting every new entry dirty).
3. `m_netSettings`, `RefreshNetcodeMap` (net code -> name map).
4. `internalRecalculateRatsnest()` (data.cpp:L235-283):
   1. `m_connAlgo->PropagateNets()` (4.3).
   2. `lastNet = NetCount()` (= `m_dirtyNets.size()`); grow `m_nets` with `new RN_NET` (or
      `Clear()` the tail); `clusters = GetClusters()` (= `SearchClusters( CSM_RATSNEST )`); clear
      every dirty net; for each cluster in order: skip if `IsOrphaned() && Size() == 1 && item is
      CN_ZONE_LAYER` (kept zone islands); if `IsNetDirty( OriginNet )`:
      `m_nets[OriginNet]->AddCluster( c )`.
   3. `ClearDirtyFlags()`; unless `m_skipRatsnestUpdate`: `updateRatsnest()`: for nets 1.. that
      are dirty and have nodes: `UpdateNet()` then (second pass) `OptimizeRNEdges()` (4.5).
5. `BOARD::BuildConnectivity` then calls `UpdateRatsnestExclusions()` (1.1).

`searchConnections` runs lazily inside the first `SearchClusters` (because `m_itemList.IsDirty()`).

### 4.2 `searchConnections()` (algo.cpp:L253-390) and the contact rules

1. Garbage collection: `RemoveInvalidRefs()` on connectivity data (and global data when local),
   `m_itemList.RemoveInvalidItems()` (compacts, restamps `m_listIndex`).
2. `dirtyItems` = items with `Dirty()` in list order (a full build: all).
3. If the list is dirty: for each dirty item (parallel `submit_loop`): `CN_VISITOR v( item )`;
   `m_itemList.FindNearby( item, v )`.
4. Apply deferred via net codes (L326-370): sort by item pointer, group by item; skip the group
   if the via has any connected item that is **not** a zone; otherwise if any entry equals the
   via's current net keep it, else set the via's net to the **minimum** zone net of the group.
   (A via that touches only zones gets a zone's net.)
5. `ClearDirtyFlags()`.

`CN_VISITOR::operator()( candidate )` (algo.cpp:L1021-1079), with `m_item` = the visiting item:
* skip invalid, same parent, both `!CanChangeNet()` with different nets;
* **skip if `candidate->Dirty() && candidate < m_item` (pointer comparison)**: each dirty-dirty
  pair is tested once, from one side. All contact tests below are symmetric and `Connect()` is
  idempotent and order-normalising, so the result does not depend on which side tests (the port
  may use list index instead of address).
* zone-zone: `checkZoneZoneConnection` (L946-997): same layer, bboxes intersect; any vertex of
  outline A inside bbox B and `B.ContainsPoint` -> connect; else the same with B's vertices in A.
* zone-item: `checkZoneItemConnection( zoneLayer, item )` (L870-944):
  * `item->IsOnLayer( layer )` required;
  * pad / via: skip if `ConditionallyFlashed( layer ) && GetZoneLayerOverride( layer ) ==
    ZLO_FORCE_NO_ZONE_CONNECTION`; skip if `IsBackdrilledOrPostMachined( layer )`;
  * connect if any anchor `i < AnchorCount()` (pads: position; tracks: start/end; vias: start;
    shapes: connection points) satisfies `zoneLayer->ContainsPoint( anchor )`;
  * else pad / via: connect if `zoneLayer->Collide( item->GetEffectiveShape( layer,
    FLASHING::ALWAYS_FLASHED ) )`; other items: `Collide( item->GetEffectiveShape( layer ) )`;
  * `connect()` also queues `(via, zone net)` for vias with `CanChangeNet()`.
* item-item: `commonLayers = A.GetLayerSet() & B.GetLayerSet() & board->GetEnabledLayers()`; in
  LSET order, first layer where `ItemsTouchOnLayer( A, B, layer )` -> connect.
  `ItemsTouchOnLayer` (L1012-1019): `A.GetEffectiveShape( layer, flashingFor( A, layer ) )
  ->Collide( B.GetEffectiveShape( layer, flashingFor( B, layer ) ) )`, `flashingFor` = pads/vias:
  `ALWAYS_FLASHED` if `!ConditionallyFlashed( layer )` else `NEVER_FLASHED`; others irrelevant.
  (Clearance 0: touching counts. Track end-point rules are therefore purely geometric: segment
  shapes with round caps; arcs by `SHAPE_ARC`; a track in a pad connects when the segment shape
  collides with the pad shape on that layer. Conditionally flashed pad/via layers count only the
  hole/barrel - `NEVER_FLASHED` shape.)

Net ties: no special connectivity code - the tie footprint's pads simply have different nets and
connect only through shared copper which is not a connectivity item (footprint graphics are not
added), so they stay separate clusters. **Jumper pads** (`updateJumperPads`, algo.cpp:L1096-1147,
run at the start of `PropagateNets`): per footprint (iterated by `std::map<FOOTPRINT*,...>` =
address order; independent per footprint), pads grouped by number (`std::map<wxString,...>`);
if `GetDuplicatePadNumbersAreJumpers()`, all pads with the same number are connected pairwise,
then every `JUMPER_GROUP` connects all pads of its numbers. The double loop is `i in 0..n, j in
1..n`, so it also calls `item->Connect( item )` for `i == j >= 1` (**a pad becomes connected to
itself**; this inflates `ConnectedItems().size()` for dangling tests - reproduce).

### 4.3 `SearchClusters( mode, excludeZones, singleNet )` and net propagation

`SearchClusters` (algo.cpp:L400-515):
* `withinAnyNet = mode != CSM_PROPAGATE`. Participants (list order): valid, and if
  `withinAnyNet`: net > 0 and (singleNet < 0 or net == singleNet); not a zone if
  `excludeZones`. `CSM_PROPAGATE` default excludes zones; `CSM_CONNECTIVITY_CHECK` and
  `CSM_RATSNEST` include them.
* Union-find over `ConnectedItems()` restricted to participants (and same net when
  `withinAnyNet`). `KI_UNION_FIND`'s root is the minimum index, so cluster `k` is created when
  its first (lowest list index) member is reached; members are added in list order.
* `singleNet >= 0`: keep clusters with a selected member.
* **`std::sort( clusters, OriginNet < )` - unstable (libstdc++ introsort; insertion sort below 17
  elements is stable but larger inputs are not).** Use `kicad_geom::stdsort::sort_by`.

`PropagateNets` (algo.cpp:L740-746) = `updateJumperPads()`, `m_connClusters =
SearchClusters( CSM_PROPAGATE )`, `propagateConnections()` (L685-738): for each cluster that is
not conflicting and `HasValidNet()`: every valid item with `CanChangeNet()` and a different net
gets `SetNetCode( OriginNet )` (both nets marked dirty). Consequences:
* tracks/arcs/free-less vias/shapes connected to a pad get the pad's net (majority among
  non-free pads, first pad on ties);
* a pad-less cluster gets the net of its **first member with net > 0 in list order** (tracks
  before shapes; vias/tracks in board order) - all its changeable items are renamed;
* conflicting clusters (two pad nets) are left untouched;
* pads and zones never change net.

### 4.4 Isolated islands: `FillIsolatedIslandsMap( map, alreadyRebuilt )` (algo.cpp:L748-839)

Input `std::map<ZONE*, std::map<PCB_LAYER_ID, ISOLATED_ISLANDS>>`. If `!alreadyRebuilt`: for each
zone (map order): `Remove( zone ); Add( zone )` (incremental re-add with fresh fills - new
`CN_ZONE_LAYER`s appended at the end of the list, with every outline vertex as anchor). Then
`m_connClusters = SearchClusters( CSM_CONNECTIVITY_CHECK )` (runs the incremental
`searchConnections` on the dirty zone items). For each cluster, `orphaned = IsOrphaned()`; every
member whose parent is a map key is recorded as (item, orphaned) under `item->GetBoardLayer()`.
Then for each (zone, layer) in the map: skip if the fill is empty; if the layer has recorded items:
orphaned -> `m_IsolatedOutlines.push_back( SubpolyIndex )`, else if `HasSingleConnection()` ->
`m_SingleConnectionOutlines.push_back( SubpolyIndex )`; if the layer has no recorded item and is
copper -> `m_IsolatedOutlines.push_back( 0 )`.

"Orphaned" = the cluster (same net, including zones) contains no non-free pad. A zone outline that
connects only through vias/tracks to nothing with a pad is an island.

**Full-build equivalence**: the incremental path gives the same connection graph as a full
rebuild (non-zone pairs unchanged since load; every new zone item is dirty and is tested against
all candidates; the via deferred-net rule sees the same pairs). The list order differs (re-added
zones last), which only changes the order of entries before callers sort them. The port may do a
full rebuild here (R-6). The caller sorts `m_IsolatedOutlines` descending before use.

### 4.5 Ratsnest (`rn.cpp`)

`AddCluster( cluster )` (L565-594): for each member in cluster order, take its anchors (zone
items: only `anchors[0]`; i.e. `min( 1, anchors.size() )`), set the anchor's cluster, insert into
`m_nodes`; the first anchor of the cluster is `firstAnchor`, every later anchor gets a board edge
`( firstAnchor, anchor, weight 0 )`. Because `CN_LIST::Add( CN_ZONE_LAYER* )` from `Build`
adds no anchors (4.1), zone items from a full build contribute **no ratsnest node** (R-5).

`compute()` (L277-337):
* `m_nodes.size() <= 2`: clear edges; if no board edges and exactly 2 nodes: tags 0/1 and one
  edge `(first, second)` in multiset order with weight 0; else all tags 0. Return.
* Else triangulate (below), append `m_boardEdges`, `kruskalMST`.

`TRIANGULATOR_STATE::Triangulate` (L128-275), nodes in multiset order:
* dedupe consecutive equal positions: `anchors` = first anchor of each distinct position,
  `anchorChains[k]` = all anchors at that position (in multiset order); `node_pts` = doubles.
* exactly one distinct position: for consecutive nodes `(prev, n)` add weight-1 edge **only if
  their parents share no layer** (`!( prev.LayerSet & n.LayerSet ).any()`); return (no chains).
* collinear (`areNodesColinear`: all cross products with `v0 = p1 - p0` are 0, using `int64`
  `VECTOR2I::Cross`): edges between consecutive distinct anchors, weight = `Dist`.
* else `delaunator::Delaunator( node_pts )`; for each half-edge `e` in `triangles` order: skip if
  `halfedges[e] != INVALID && halfedges[e] < e`; edge `( anchors[triangles[e]],
  anchors[triangles[next(e)]] )`, `next(e) = e % 3 == 2 ? e - 2 : e + 1`, weight `Dist`.
* chains with >= 2 anchors: `std::sort` by `GetCluster().get()` (address), then consecutive
  pairs `( chain[j-1], chain[j] )` with weight `clusters differ ? 1 : 0`.

`kruskalMST( edges )` (L49-125): tag = index in `m_nodes`; weight-0 edges are unioned up front
(not emitted); the rest compacted in order and passed to `KI_MST::FilterKruskal` with key
`( weight, min( (sx, sy, stag), (tx, ty, ttag) ), max(...) )` - a total order on distinct edges,
so the selected set and the **emission order (ascending key)** are independent of input order
and of `nth_element`/`partition` internals; **edge orientation is preserved from the input**
(triangle half-edge orientation, chain order). Emitted edges -> `m_rnEdges`.

`OptimizeRNEdges()` (L340-527), per edge in `m_rnEdges` order:
* if `source->ConnectedItemsCount() == 0` (`items.cpp:L387-409`: number of connected items
  whose parent hit-tests the anchor position, zones by `HitTestFilledArea( GetBoardLayer(), pos
  )`): `optimizeZoneAnchor( source.pos, source.parent.layerSet, target )`: among the target
  item's connected `CN_ZONE_LAYER`s of the same net on one of those layers, the outline vertex
  with the smallest squared distance (strict `<`, initial = current target distance) becomes a
  new target anchor `CN_ANCHOR( pt, zoneLayer )`;
* else if target count is 0: symmetric, replaces the source;
* else `optimizeZoneToZoneAnchors`: centres of the bboxes of every connected zone layer of
  source and target items; all (a, b) pairs with their squared distance; `std::sort` by distance
  (**unstable, ties possible** - use `stdsort`); for the first 3 pairs with different zone
  layers: `shapeA.ClosestSegmentsFast( shapeB, ptA, ptB )` -> replace both anchors (later pairs
  overwrite earlier ones).

Ordered output: `GetEdges()` stable sort (3.5). Count: `GetUnconnectedCount( visibleOnly )`
counts edges over all nets. `RunOnUnconnectedEdges( f )` (data.cpp:L771-785): nets in net-code
order (index 0 included), edges via `GetEdges()` (sorted), stop when `f` returns false.

### 4.6 Queries used by DRC, the filler, flashing and teardrops (data.cpp)

* `IsConnectedOnLayer( item, layer, types )` (L424-553): for each `CN_ITEM` of the item, each
  connected item with valid, ordinal range containing `layer`, type in `types` (empty = any), same
  net; skip itself, skip items of the same parent footprint; pad-pad skip when both padstacks are
  conditionally flashed (`REMOVE_EXCEPT_START_AND_END` or `REMOVE_ALL`); for pad/via connected
  items require `ItemsTouchOnLayer( item, other, layer )`; pad with a connected zone layer: true
  if the zone is filled and **some vertex of that fill outline is outside** the pad's
  `GetEffectivePolygon( layer, ERROR_INSIDE )->Outline( 0 )`; via with a zone layer: same with
  `SHAPE_CIRCLE( center, width( layer ) / 2 )`; otherwise true. Used by `FlashLayer` with types
  `{TRACE, ARC, VIA, PAD}` (zones excluded).
* `TestTrackEndpointDangling( track, ignoreTracksInPads, &pos )` (L805-962): tracks/arcs:
  `accuracy = KiROUND( width / 2.0 )`; for each connected item (skip `IS_DELETED`): zones use
  `board->GetCopperZoneRTree( zone )` (DRC cache, may be null -> effective shape) with
  `SHAPE_CIRCLE( end, accuracy )`, else `shape->Collide( end, accuracy )`; both ends hit: zone ->
  not dangling; pad/via and `ignoreTracksInPads` -> not dangling; otherwise count the nearer end
  (`getMinDist`); dangling unless both counts > 0; `pos` = start if start count is 0 else end.
  Vias: no connections -> dangling only if net > 0; else dangling if every non-deleted connected
  item has the same `Layer()` (**ordinal start layer; a through-hole pad's is 0 = F_Cu**).
  Order-independent.
* `GetConnectedItems( item, flags )` (L582-610): `SearchClusters( IGNORE_NETS ? CSM_PROPAGATE :
  CSM_CONNECTIVITY_CHECK, EXCLUDE_ZONES, IGNORE_NETS ? -1 : item net )`; items of every cluster
  containing the item, cluster order.
* `GetNetItems` (sorted by address), `GetConnectedTracks`, `GetConnectedPads` (`std::set` by
  address), `GetConnectedPadsAndVias` (connected order = address order), `GetConnectedItemsAtAnchor`
  (L966-998), `GetZoneIslandConnections` (L711-732), `GetNodeCount`, `GetPadCount`,
  `GetRatsnestForNet`.
* `CN_ANCHOR::IsDangling()` (items.cpp:L340-385; used by track cleanup, router - T3).

### 4.7 Incremental update vs full build

C++ keeps the graph alive and updates it with `Add/Remove/Update` (lazy invalidation,
`m_hasInvalid`, dirty items, dirty nets, partial ratsnest recompute, `RN_NET::RemoveInvalidRefs`).
**The port always does full builds** (CLI semantics). Lost: speed for interactive editing,
`LocalBuild` / dynamic ratsnest (GUI move), `BlockRatsnestItems`. Kept identical: every output of
the CLI flows (load, fill, DRC, save), provided R-5 (zone anchors) is honoured: after
`FillIsolatedIslandsMap` the C++ graph holds re-added zone items **with** anchors until the next
full `Build`; the CLI always runs a full `Build` (`PostFillRefresh`, DRC, `SaveBoard`) before
anything reads ratsnest anchors, so a full build is equivalent.

### 4.8 `ZONE_FILLER::Fill( zones, check = false )` (zone_filler.cpp:L590-1981)

In execution order:

1. Lock; clear `m_refillResultCache`, `m_preHatchSolidFillCache`, `m_sameNetApronCache`.
   Create a `DRC_ENGINE` if the board has none (CLI always has one).
2. `connectivity->ClearRatsnest()`; `m_worstClearance = GetMaxClearanceValue()`;
   `m_boardOutline = GetBoardPolygonOutlines( true )` (validity flag).
3. Cache zone bboxes; for every pad with `IsDirty()`: `BuildEffectiveShapes()` +
   `BuildEffectivePolygon( ERROR_OUTSIDE )`; per footprint `BuildCourtyardCaches()`,
   `BuildNetTieCache()` (pure caches for the port).
4. `buildItemIndexes()` (3.7). `boardCuMask = AllCuMask( copperLayerCount )`.
5. `zoneOutlineIndices[zone] = POLY_YSTRIPES_INDEX( zone->GetBoardOutline() )` for board zones
   with > 2 corners.
6. **Conditional via flashing, pre-pass** (L793-845), vias in `Tracks()` order:
   `ClearZoneLayerOverrides()`; skip unless `GetRemoveUnconnected()`; for each layer of
   `GetLayerSet() & boardCuMask` with `ConditionallyFlashed( layer )`:
   * inside a pour keepout (`isInPourKeepoutArea`, L757-788: board rule areas with keepout
     params and `GetDoNotAllowZoneFills`, on the layer, > 2 corners, bbox intersects the via bbox,
     `zoneOutlineIndices[keepout].Contains( center )`) -> `ZLO_FORCE_NO_ZONE_CONNECTION`;
   * else `zone = findHighestPriorityZone( viaBBox, layer, net, test )` with test
     `zone->GetBoardOutline().Contains( center, -1, holeRadius = drill / 2 + 1 )`;
     `findHighestPriorityZone` (L698-755): board zones, skip rule areas / other layers /
     <= 2 corners / bbox miss / failed test; tracks best same-net (strictly greater
     `GetAssignedPriority()`, first wins ties) and best overall; returns the same-net best if any
     else the overall best;
   * `ZLO_FORCE_FLASHED` if that zone has the via's net and (`UnconnectedLayerMode() !=
     START_END_ONLY` or the layer is the drill start/end), else `ZLO_FORCE_NO_ZONE_CONNECTION`.
7. **Pads, pre-pass** (L847-893): same with pad bbox, test
   `zoneOutlineIndices[zone].Contains( center )` (fallback `GetBoardOutline().Contains(
   center )`), `FORCE_FLASHED` iff the zone has the pad's net.
8. For each zone in `aZones` (skip rule areas, <= 2 corners): `m_commit->Modify`; for each layer
   of `GetLayerSet()` (LSET order): `BuildHashValue( layer )` into `oldFillHashes`;
   `toFill.push_back( {zone, layer} )`; if not copper thieving and not teardrop:
   `isolatedIslandsMap[zone][layer] = {}`. Then `zone->UnFill()`.
9. **Fill DAG** (`run_fill_waves`, L1026-1193): item `j` waits for item `i` iff
   `fill_item_dependency( j, i )`: different zones, same layer, and `zone_fill_dependency(
   zone_j, layer, zone_i, true )` (L930-964): false if `zone_i` already has `GetFillFlag( layer
   )`, is a rule area, has <= 2 corners, is not on the layer, if `zone_j->HigherPriority( zone_i
   )`, if same net; else `zoneKnockoutMayInteract( zone_j, zone_i )`. Candidates are found
   through `m_zoneIndex[layer]` with `zoneKnockoutQueryBox( zone_j )` plus all fill items whose
   zone is not in the index. Each item: `fill_lambda` = `fillSingleZone( zone, layer, polys )`
   then `zone->SetFilledPolysList( layer, polys )`; then successors are released; then
   `tesselate_lambda` = `zone->CacheTriangulation( layer )`, `SetFillFlag( layer, true )`.
   Sequential port: any topological order (e.g. Kahn in `toFill` order) gives identical fills,
   because a fill reads another zone's fill only through the same `zoneKnockoutMayInteract` gate
   that created the dependency (comment L960-963, R-9).
10. For teardrop zones in `aZones`: `connectivity->Update( zone )`. Then
    `connectivity->FillIsolatedIslandsMap( isolatedIslandsMap )` (4.4). Every non-rule-area zone:
    `SetIsFilled( true )`.
11. **Island removal, initial** (L1248-1312), zones in map order: a layer is "initially fully
    isolated" if `m_IsolatedOutlines.size() == outline count`; if every layer of the zone is fully
    isolated the zone is skipped entirely (unconnected pours are preserved). Otherwise per layer
    with islands: indices sorted **descending**; per index: `ALWAYS` -> `DeletePolygonAndTriangulationData(
    idx, false )`; `AREA` and `outline.Area( true ) < GetMinIslandArea()` -> delete; else
    `SetIsIsland( layer, idx )` (NEVER mode, or large enough). Deletions are recorded in
    `zonesWithRemovedIslandLayers`. Then `UpdateTriangulationDataHash()`, `CalculateFilledArea()`.
    **Trap**: an island index flagged before a lower index is deleted is not renumbered (R-10).
12. **Iterative refill** (4.13) when `m_ZoneFillIterativeRefill` and (islands were removed or
    `sameNetOverlapSeeds` is non-empty).
13. **Out-of-board islands** (L1661-1790): only zones with `mayHoldOutOfBoardCopper` (board
    outline valid, not teardrop, corner smoothing CHAMFER or FILLET; L519-532). `minArea =
    minThickness^2 * 3` (double). For each zone copper layer (`GetLayerSet() & AllCuMask( count
    )`): for each outline `jj` from last to first: `test_poly = Polygon( jj ).front()`,
    `island_area = test_poly.Area()`; skip if `< minArea`; `bboxInBoard =
    BooleanIntersection( m_boardOutline, BoxToLineChain( bbox ) )`; `outOfBoardArea = bboxArea -
    inBoardArea`; skip if `outOfBoardArea < island_area / 2`; delete if `inBoardArea <
    island_area / 2`; else `intersection = m_boardOutline & test_poly`, delete if `.Area() <
    island_area / 2`. Deletions applied afterwards per poly in descending index order with
    `DeletePolygonAndTriangulationData( idx, true )`. Then `CalculateFilledArea()` for all zones.
14. **Flashing, second pass** (L1795-1914): per (net, layer) list of filled board zones with a
    `POLY_YSTRIPES_INDEX` of the fill; via layers with `ZLO_FORCE_FLASHED` are demoted to
    `ZLO_FORCE_NO_ZONE_CONNECTION` unless some same-net fill on that layer has `bbox.GetInflated(
    reach ).Contains( center )` and `index.Contains( center, reach )`, `reach = max( drill / 2,
    width( layer ) / 2 )`; pads likewise with `reach = max( min( drill.x, drill.y ) / 2 if hole,
    min( size.x, size.y ) / 2 )`. (These overrides are not saved but change via/pad flashing for
    DRC and plotting.)
15. `aCheck` handling (GUI only), return true.

### 4.9 `fillSingleZone` and `BuildSmoothedPoly` (zone_filler.cpp:L3634-3665, zone.cpp:L1835-1993)

`fillSingleZone( zone, layer )`: `BuildSmoothedPoly( maxExtents, layer, boardOutline-or-null,
&smoothedPoly )` - note the **first output is used as `aMaxExtents` and the apron output as
`aSmoothedOutline`** - then `fillCopperZone` (copper zones) or `fillNonCopperZone`; on success
`SetNeedRefill( false )`.

`BuildSmoothedPoly( out, layer, boardOutline, apronOut )`:
1. `GetNumCorners() <= 2` -> false. `flattened = GetBoardOutline()`, `ClearArcs()`. Rule area:
   `out = flattened`, return.
2. `smooth_requested` = CHAMFER or FILLET and not teardrop. `smooth( p )` = `p.Chamfer(
   cornerRadius )` or `p.Fillet( cornerRadius, GetMaxError() )` (board `m_MaxError`).
3. `maxExtents = &flattened`; if `bds.m_ZoneKeepExternalFillets && smooth_requested`:
   `withFillets = smooth( flattened ) BooleanAdd flattened`, `maxExtents = &withFillets`.
4. `out = flattened`. `GetInteractingZones( layer )` (zone.cpp:L1781-1833; board zones in board
   order, not itself, on the layer, not rule area/teardrop, bbox (inflated by 1 um) intersects;
   same net -> `selfOutline->Collide( candidateOutline )` -> same-net list; different net -> diff
   list). For each same-net zone: `diffNetPoly` = union (`BooleanAdd`, one at a time, list order)
   of outlines (arcs cleared) of diff-net zones with `HigherPriority( sameNetZone )` and bbox
   intersecting the same-net zone's bbox; `isolated = diffNetPoly` non-empty and `(this outline -
   diffNetPoly)` empty; if not isolated `out.BooleanAdd( sameNetOutline )`.
5. If board outline: `out.BooleanIntersection( boardOutline.CloneDropTriangulation() with arcs
   cleared )`.
6. `withSameNet = out.CloneDropTriangulation()`; `smooth( out )`.
7. Apron: `poly = maxExtents.Clone(); poly.Inflate( minThickness, ROUND_ALL_CORNERS, maxError
   )`; if `!keepExternalFillets`: `poly &= withSameNet`; `apron = out & poly`.
8. `out.BooleanIntersection( *maxExtents )`.

### 4.10 `fillCopperZone( zone, layer, debugLayer, smoothedOutline, maxExtents, fill )` (L3110-3513)

Constants: `half_min_width = minThickness / 2`, `epsilon = mmToIU( 0.001 ) = 1000`,
`fastCornerStrategy = CHAMFER_ALL_CORNERS`, `cornerStrategy = ROUND_ALL_CORNERS`, `iterative =
true`. The min-width cycle runs only if `half_min_width - epsilon > epsilon`.

1. `fill = smoothedOutline`.
2. `knockoutThermalReliefs( zone, layer, fill, thermalPads, noConnPads, solidItems )` (4.11).
3. Hatch zones: `buildHatchZoneThermalRings( ..., smoothedOutline, thermalPads, fill, rings )`
   (4.12).
4. `buildCopperItemClearances( zone, layer, noConnPads, clearanceHoles, includeZoneClearances =
   !iterative = false )` (4.11). Iterative: for every board+footprint zone (L3196-3210) with
   `isZoneFillKeepout( candidate, layer, zoneBBox )` (L355-362) and the filled zone not a
   teardrop: `candidate->TransformSmoothedOutlineToPolygon( clearanceHoles, layer, 0, maxError,
   ERROR_OUTSIDE, nullptr )`; if any added, `clearanceHoles.Simplify()`.
5. `buildThermalSpokes( zone, layer, thermalPads, spokes )` (4.12).
6. Iterative: `buildDifferentNetZoneClearances( zone, layer, zoneClearances )` (4.11).
7. Spoke filter (only if spokes exist): `testAreas = fill.Clone() - clearanceHoles -
   zoneClearances(if any)`; min-width cycle `Deflate( hmw - eps, CHAMFER_ALL, maxError )` then
   `Inflate( hmw - eps, CHAMFER_ALL, maxError )`; `POLY_YSTRIPES_INDEX( testAreas )`. For each
   spoke (deque order): keep (`fill.AddOutline( spoke )`) if `index.Contains( spoke.CPoint( 3 ), 1
   )`; else keep if some other spoke `o` has `o.PointInside( testPt, 1 ) &&
   spoke.PointInside( o.CPoint( 3 ), 1 )`.
8. `fill.BooleanSubtract( clearanceHoles )`.
9. Min-width: `fill.Deflate( hmw - eps, CHAMFER_ALL, maxError )` (rings too, if any).
10. Drop blobs (L3340-3355): from last outline to first, merge outline-0 points into a bbox until
    `GetSizeMax() > minThickness`; delete if `GetSizeMax() < minThickness`.
11. Pattern: hatch (and not `m_PrototypeZoneFill` in project local settings): `ringsToProtect =
    rings + clearanceHoles`; for every solid-connection via on the layer: disc
    `TransformCircleToPolygon( pos, width( layer ) / 2, maxError, ERROR_OUTSIDE ) &
    smoothedOutline` added; iterative: `m_preHatchSolidFillCache = (fill inflated by hmw - eps,
    ROUND_ALL, simplify) & maxExtents - clearanceHoles`; `addHatchFillTypeOnZone( ..., fill,
    ringsToProtect )`. Thieving: `addCopperThievingPattern` (T3). Solid: `fill.Fracture()`,
    `connect_nearby_polys( fill, minThickness )` (4.14).
12. Re-inflate: `fill.Inflate( hmw - eps, ROUND_ALL_CORNERS, maxError, simplify = true )`;
    `fill.BooleanAdd( rings )`.
13. Holes of thermally connected pads: `addHoleKnockout( pad, 0, clearanceHoles )`;
    `fill.BooleanIntersection( maxExtents )`; `fill.BooleanSubtract( clearanceHoles )`.
14. Iterative: `sameNetApron = smoothedOutline - maxExtents - clearanceHoles`; store
    `m_preKnockoutFillCache = fill`, `m_sameNetApronCache = apron`; if `zoneClearances` non-empty:
    `fill -= zoneClearances`, `apron -= zoneClearances`, `postKnockoutMinWidthPrune( zone, fill,
    apron )` (L3043-3089: `pre = fill.Clone()`; if apron: `apron.Inflate( eps, ROUND_ALL )`, `fill
    += apron`; `Deflate( hmw - eps, CHAMFER_ALL )`; `Fracture()`; `connect_nearby_polys(
    minThickness )`; blob drop as step 10; `Inflate( hmw - eps, ROUND_ALL, maxError, true )`; `fill
    &= pre`; skipped entirely if `hmw - eps <= eps`).
15. `subtractHigherPriorityZones( zone, layer, fill )` (L2942-2979): zones from
    `m_zoneIndex[layer]` queried with the zone bbox, same net and **strictly higher assigned
    priority**, not teardrop, bbox intersects: their board outlines (arcs cleared) appended; one
    `BooleanSubtract`.
16. `dropSubResolutionOutlines( fill, maxError )` (L3031-3040): delete outlines with `Area() <
    maxError^2` (from last). `fill.Fracture()`.

`fillNonCopperZone` (L3516-3626): knockouts (`addKnockout( item, layer, 0, true )`) of every
footprint reference / value / graphic and board drawing that `IsKnockout()`, is on the layer and
intersects the zone bbox (footprints first, unindexed board order); `fill = smoothedOutline -
holes`; keepout outlines (board + footprint zones passing `isZoneFillKeepout`, arcs cleared) are
subtracted; `Deflate( hmw - eps, CHAMFER_ALL )` **unconditionally**; hatch/thieving patterns;
`Inflate( hmw - eps, ROUND_ALL )` only if `hmw - eps > eps`; `Fracture()`.

### 4.11 Knockouts and DRC queries

**`knockoutThermalReliefs`** (L2108-2403). Pads from `m_padIndex` queried with the zone bbox
inflated by `m_worstClearance`, in seq order:
* skip unless `IsOnLayer( layer )` or NPTH with drill > 0; skip if pad bbox inflated by
  `m_worstClearance` misses the zone bbox;
* dedupe (non-custom shapes): key (position, effective size = for circles `max( size.x,
  size.y, max drill )` square, else size; shape; orientation; net);
* `noConnection` = different net, or (not teardrop zone and (zone net 0 or pad override
  `ZLO_FORCE_NO_ZONE_CONNECTION`)), or `IsBackdrilledOrPostMachined( layer )` -> append to
  `noConnPads`, continue;
* hatch zone: `EvalZoneConnection( pad, zone, layer )` (DRC_ENGINE, drc_engine.cpp:L999-1029:
  `ZONE_CONNECTION_CONSTRAINT`; `THT_THERMAL` -> THERMAL for PTH pads else FULL); THERMAL and
  `!CanFlashLayer` -> NONE; THERMAL: if `fill.Collide( pad->GetEffectiveShape( layer,
  ALWAYS_FLASHED ), 0 )`: gap = `EvalRules( THERMAL_RELIEF_GAP_CONSTRAINT, pad, zone, layer
  ).Min()`, push pad to `thermalPads`, `addKnockout( pad, layer, gap, holes )`; NONE -> noConnPads;
  FULL: nothing;
* solid zone: connection = FULL for teardrop zones else `EvalZoneConnection`; THERMAL with
  `!CanFlashLayer` -> NONE; THERMAL as above; NONE: `clr = max( EvalRules(
  PHYSICAL_CLEARANCE_CONSTRAINT ).Min(), zone local clearance )` (strictly greater wins); if
  `FlashLayer( layer )`: `addKnockout( pad, layer, clr )`; else if drill: `hclr = max( EvalRules(
  PHYSICAL_HOLE_CLEARANCE_CONSTRAINT ).Min(), clr )`, `TransformHoleToPolygon( holes, hclr,
  maxError, ERROR_OUTSIDE )`; FULL: nothing.
* Hatch zones also walk **all vias in `Tracks()` order** (L2313-2400): on layer, bbox test,
  dedupe (position, max( drill, width( layer ) ), net); `noConnection` = other net, or
  START_END_ONLY and not a drill end, or override NO_CONNECTION; backdrilled/post-machined: also
  `TransformCircleToPolygon( holes, pos, GetMaxHoleSize() / 2 + zone local clearance, maxError,
  ERROR_OUTSIDE )`; THERMAL with gap > 0 -> thermal + knockout; NONE nothing (handled later);
  FULL -> `solidItems`.
* Finally `fill.BooleanSubtract( holes )`.

`addKnockout( item, layer, gap, holes )` (L1988-2016): custom pads: `TransformShapeToPolygon(
poly, layer, gap, maxError, ERROR_OUTSIDE )`, then convex hull (`BuildConvexHull`) when
`GetCustomShapeInZoneOpt() == CONVEXHULL` else append; others: `TransformShapeToPolygon( holes,
layer, gap, maxError, ERROR_OUTSIDE )`. `addHoleKnockout` = `TransformHoleToPolygon( holes, gap,
maxError, ERROR_OUTSIDE )`. Graphic `addKnockout( item, layer, gap, ignoreLineWidth, holes )`
(L2033-2101): fields/texts (visible only; knockout text: copy with knockout off,
`TransformTextToPolySet( holes, 0, maxError, ERROR_INSIDE )`; else `TransformShapeToPolygon`);
shapes: `TransformWithLineEndingsToPolygon( holes, gap, maxError, ERROR_OUTSIDE,
ignoreLineWidth )`; textbox/table/drill chart/stackup table/target: `TransformShapeToPolygon(
..., ignoreLineWidth )`; barcode: `GetBoundingHull`; dimensions: shape + `PCB_TEXT::TransformShapeToPolygon`.

**`buildCopperItemClearances`** (L2410-2868). `extra = mmToIU( 0.0005 ) = 500`; `zoneBox = zone
bbox inflated by m_worstClearance + extra`; `eval( c, zone, item, layer )` = `EvalRules( c, a =
zone, b = item, layer )` -> `-1` if the constraint is null else `Min()`.
* no-connection pads (deduped like above): `gap = eval( PHYSICAL_CLEARANCE )`; if flashed on the
  layer or plated hole: `gap = max( gap, eval( CLEARANCE ) )`; if flashed and gap >= 0:
  `addKnockout( pad, layer, gap + extra )`; if drill: `holeGap = max( eval(
  PHYSICAL_HOLE_CLEARANCE ), eval( HOLE_CLEARANCE ) )`; NPTH resets `gap` to the physical value;
  `gap = max( gap, holeGap )`; if >= 0 `addHoleKnockout( pad, gap + extra )`; backdrilled:
  circle `GetMaxHoleSize() / 2 + max( gap, 0 ) + extra`.
* tracks/arcs/vias from `m_trackIndex[layer]` (zoneBox, seq order), on the layer; dedupe vias
  (position, max( drill, width ), net) and tracks (canonical endpoints, width; arcs dedupe as
  tracks by endpoints+width); bbox must intersect zoneBox; `sameNet` = same net (false when zone
  net 0 and not teardrop, or via override NO_CONNECTION); `gap = eval( PHYSICAL_CLEARANCE )`,
  `!sameNet -> max( gap, eval( CLEARANCE ) )`; via: if `FlashLayer( layer ) && gap > 0`:
  `via->TransformShapeToPolygon( holes, layer, gap + extra, maxError, ERROR_OUTSIDE )`; `holeGap
  = eval( PHYSICAL_HOLE_CLEARANCE )` (+ `HOLE_CLEARANCE` if !sameNet); `gap = max( gap, holeGap
  )`; if >= 0 circle `drill / 2 + gap + extra`; backdrill circle as pads; track/arc: if gap >= 0
  `TransformShapeToPolygon( holes, layer, gap + extra, maxError, ERROR_OUTSIDE )`.
* graphics merged with footprint courtyards by seq (`fp.seq <= gfx.seq` -> courtyard first,
  L2765-2809): courtyard: `gap = eval( PHYSICAL_CLEARANCE, zone, footprint )`; side = footprint
  layer for inner copper else the layer; gap 0 -> append `GetCourtyard( side )`; gap > 0 ->
  courtyard `Inflate( gap, ROUND_ALL, maxError )`. Graphic: footprint graphics (not ref/value)
  that are on the layer are skipped when they collide with an allowed net-tie pad (`allowedNetTiePads`:
  net-tie footprints, pads with the zone net on the layer and their `GetNetTiePads`); otherwise:
  on layer / Edge_Cuts / Margin and bbox in zoneBox: `gap = eval( PHYSICAL_CLEARANCE )`; on layer
  and different net (shape net != zone net; non-shapes have net -1): `max( CLEARANCE )`; else
  Edge_Cuts: `max( EDGE_CLEARANCE )`, `ignoreLineWidth = true`; else Margin: `max(
  EDGE_CLEARANCE )`; if >= 0 `addKnockout( item, layer, gap + extra, ignoreLineWidth )`.
* zones from `m_zoneIndex[layer]` with `zoneKnockoutQueryBox( zone )`; when
  `includeZoneClearances` is false only **teardrop** zones are processed: rule areas with
  do-not-fill (filled zone not teardrop, bbox in zoneBox): `TransformSmoothedOutlineToPolygon(
  holes, layer, 0, maxError, ERROR_OUTSIDE, nullptr )`; `HigherPriority`, different net,
  `zoneKnockoutMayInteract`: `gap = max( eval( PHYSICAL ), eval( CLEARANCE ) )`, if >= 0
  `other->TransformShapeToPolygon( poly, layer, gap + extra, maxError, ERROR_OUTSIDE )` (**reads
  the other zone's fill**: `InflateWithLinkedHoles( gap + extra + maxError, ROUND_ALL, maxError )`,
  zone.cpp:L2096-2131), appended.
* `holes.Simplify()`.

`buildDifferentNetZoneClearances` (L2875-2935): the zone part of the above for non-rule,
non-teardrop zones; `holes.Simplify()`.

`zoneKnockoutQueryBox( z )` (L534-548): bbox inflated by `m_worstClearance + slack + z.minThickness
(+ z.cornerRadius + m_maxZoneCornerRadius if keepExternalFillets)`. `zoneKnockoutMayInteract( z,
k )` (L551-580): reach as above but adding the corner radius of each of z, k that is
chamfer/fillet smoothed; bbox test then `z.GetBoardOutline().Collide( &k.GetBoardOutline(), reach )`.

`HigherPriority( other )` (zone.cpp:L545-556): teardrop vs non-teardrop -> compare teardrop type
ints; else priority; else **`m_Uuid > other.m_Uuid`** (KIID ordering, kicad-core).

### 4.12 Thermal spokes, hatch rings, hatch pattern

`buildThermalSpokes` (L3671-4044). `zone_half_width = minThickness / 2` (hatch: `hatchThickness /
2`); `zoneBB` inflated by `max( bds.GetBiggestClearanceValue(), zone local clearance )`;
`epsilon = bds.m_MaxError`. Per thermal item on the layer:
* circular = circle pad, or oval with equal sides, or via;
* gap = `EvalRules( THERMAL_RELIEF_GAP ).Min()`; `spoke_w = EvalRules( THERMAL_SPOKE_WIDTH
  ).Opt()`; pads: `clamp( spoke_w, Min, Max )`, `min( spoke_w, min( size.x, size.y ) )`; vias (hatch
  only): `min( spoke_w, width( layer ) )`; skip if `spoke_w < minThickness`; vias in solid zones
  are skipped;
* `itemBB` inflated by `gap + epsilon` must intersect `zoneBB`;
* custom pad with proxy segment primitives: spokes along the primitives (L3884-3970): segment
  rotated by pad orientation, moved to `ShapePos`, reversed so A is inside the pad polygon,
  trimmed to the pad outline and to `TransformShapeToPolygon( gap + epsilon )` outline 0 (first
  intersection); side segments offset by `spoke_w / 2` must also trim; 5-point chain (test point
  index 3) extended by `minThickness`;
* otherwise (L3971-4038): `spokesBox` = bbox of a copy of the pad at (0,0), orientation 0,
  layer offset 0 (`PAD dummy( *pad )`, `GetBoundingBox( layer )`), or a via copy at (0,0);
  inflated by `gap + epsilon + zone_half_width`; `buildSpokesFromOrigin( box, angle )` builds 4
  spokes at `angle + 0/90/180/270` deg with `intersectBBox` (cos/sin of `EDA_ANGLE`, axis cases
  exact, else the nearer side, `KiROUND`); chain = `center + side, center - side, center + hit -
  side, center + hit (test pt), center + hit + side`, closed; circular: build at 0 then rotate the
  last 4 by the pad's `GetThermalSpokeAngle()` when non-zero; non-circular: build at that angle;
  then rotate the last 4 by the pad orientation (vias: 0) and move to `ShapePos( layer )` /
  via position;
* finally `GenerateBBoxCache()` on all spokes.

`buildHatchZoneThermalRings` (L4047-4155): circular: `TransformRingToPolygon( ring, pos, padRadius
+ gap + spokeW / 2, spokeW, maxError, ERROR_OUTSIDE )`; else `pad.TransformShapeToPolygon( gap +
spokeW ) - pad.TransformShapeToPolygon( gap )`; clipped to `smoothedOutline`; `fill +=` and
`rings +=` (one `BooleanAdd` per ring).

`addHatchFillTypeOnZone` (L4355-4584): `thickness = max( hatchThickness, minThickness + 1000 )`,
`gridsize = thickness + hatchGap`; `filled` rotated by `-hatchOrientation`, `bbox = filled.BBox(
0 )` (**outline 0 only**); hole square of `hatchGap + minThickness`; `minimal_hole_area = square
area * hatchHoleMinArea`; smoothing (level > 0 and `smooth_value = KiROUND( gap * smoothing / 2 )
> 0.02 mm`; level forced 1 below 0.04 mm; `+= minThickness / 2`, `min( gap / 2 )`; `maxError =
max( 2 * maxError, smooth / 20 )`; level 1 `Chamfer`, else `Fillet` (maxError / 2 if level > 2));
layer `hatching_offset` from board defaults then zone override; grid from `x_offset = bbox.x -
bbox.x % gridsize - gridsize` (C++ `%` truncates toward zero, same as Rust) to `<= right/bottom`,
x outer loop, y inner; each hole moved, rotated by the orientation, moved by `offset % gridsize`;
`ClearArcs`; `holes &= (fill deflated by max( hatchThickness - minThickness, 2 * maxError ),
CHAMFER_ALL)`; `holes &= (board outline deflated by minThickness, CHAMFER_ALL)`; drop holes with
`Area() < minimal`; drop holes fully enclosing a thermal ring (bbox containment, centre and first
point inside, no intersections); `fill.BooleanSubtract( fill, holes )`.

### 4.13 Iterative refill (L1314-1659, `refillZoneFromCache` L4587-4820)

Seeds: `zonesWithRemovedIslandLayers` plus `sameNetOverlapSeeds` (L1327-1379: for each
non-rule, non-teardrop zone of `aZones`, every board/footprint zone of the same net with strictly
higher assigned priority whose bbox intersects, on shared copper layers where both have fill).
Up to 8 iterations:
1. Candidates (L1411-1467): for each changed (zone, layer) (set order), each zone of `aZones`
   that is not a rule area / teardrop, has the layer, and (is the changed zone, or the changed zone
   is `HigherPriority` than it, or same net); different-net candidates must pass
   `zoneKnockoutMayInteract( candidate, changed )`; others the bbox inflated by
   `m_worstClearance`. Deduplicated in discovery order.
2. Hash every candidate (`BuildHashValue`); snapshot every non-rule zone's non-empty fill on the
   candidate layers (`CloneDropTriangulation`).
3. Each candidate (no dependencies, any order): `refillZoneFromCache( zone, layer, &snapshot )`:
   start from `m_preKnockoutFillCache` (missing -> fail); for zones from `m_zoneIndex[layer]`
   (`zoneKnockoutQueryBox`): skip itself, other layers, teardrops, not `HigherPriority`; same net:
   bbox vs zone bbox inflated by `m_worstClearance + extra`; else `zoneKnockoutMayInteract`;
   take the snapshot fill (missing -> skip; empty -> skip); same net: hatch zones of strictly
   higher priority contribute their outline, others their fill (`sameNetKnockouts`); different
   net: fill `Inflate( gap + extra + maxError, ROUND_ALL, maxError )` with `gap = max( PHYSICAL,
   CLEARANCE )` (skip if < 0) into `diffNetKnockouts`. Result-cache lookup by hash (may be skipped
   by the port - pure function). `fill -= diffNet`; if any diff-net: apron from cache minus
   diff-net, `postKnockoutMinWidthPrune`; `fill -= sameNet`; hatch zones: `border = (diff +
   same).Inflate( minThickness, ROUND_ALL ) - knockouts & m_preHatchSolidFillCache`, `fill +=
   border`; `dropSubResolutionOutlines`; `Fracture()`. Then `SetFilledPolysList`,
   `SetFillFlag( false )`, then `CacheTriangulation` + `SetFillFlag( true )`.
4. Island detection on the candidates (not thieving/teardrop) with `FillIsolatedIslandsMap`;
   removal as 4.8 step 11 except: layers that were "initially fully isolated" and are still fully
   isolated are kept. Note `m_insulatedIslands` is **not cleared** by the refill (R-10).
5. Changed = candidates whose hash changed; stop if none; after 8 iterations `wxLogWarning`
   ("Zone fills may be incorrect: iterative refill did not converge after 8 passes...").

### 4.14 `connect_nearby_polys( polys, distance )` (L2982-3026, `VERTEX_CONNECTOR` L95-245)

`VERTEX_CONNECTOR( polys.BBoxFromCaches(), polys, distance )` builds a `VERTEX_SET` with
simplification level 50 (`createList( outline, tail, userData = outline index )`, then
`updateList()` = z-order sort); per outline the cumulative contour lengths (`EuclideanNorm`
doubles). `FindResults`: walk `m_vertices.front().next` around the list; skip non-ears (`isEar()`);
if not visited and `getPoint( p )` (z-order neighbours within `[zOrder( p - d ), zOrder( p + d )]`,
skipping same-outline points closer than `d` along the contour, keeping the nearest with `0 <
dist2 < d^2` that `isEar( true )`) returns q not visited: insert `RESULTS( outline(p),
outline(q), p.i, q.i )` into a `std::set` and mark p, q and their +-2 neighbours visited.
Results in set order: both points appended to `insertion_points[outline1]` as `(v1, pt1)`,
`(v1, pt2)`; per outline (map order) stable-sorted by vertex descending and inserted with
`line.Insert( vertex + 1, pt )` - so each bridge becomes `v1, pt2, pt1` (zero-width spike that
the re-inflate turns into a bridge). Exact port of `VERTEX_SET`/`VERTEX` (kicad-geom G4b) is
required, including the simplification behaviour of `createList`.

### 4.15 Copper thieving (T3)

`addCopperThievingPattern` (L4158-4352): dots/squares stamped on a grid (stride `element + gap`,
optional stagger, phase from `hatching_offset`), centres accepted with `filledRegion` deflated by
the stamp half extent + 1 (`Contains( c, -1, 0, true )`); hatch variant subtracts square voids
clipped to the fill deflated by `line_width`. Thieving zones skip island detection.

### 4.16 Teardrops (only where fill / DRC interact)

* Teardrop zones are ordinary `ZONE`s with `IsTeardropArea()` (type `TD_VIAPAD` / `TD_TRACKEND`).
  In the filler: `HigherPriority` than any normal zone; connection FULL; no corner smoothing;
  `mayHoldOutOfBoardCopper` false; excluded from island maps and refill candidates; zone net 0
  rules do not apply; in iterative mode teardrops are still knocked out of other zones as
  different-net zones through `buildCopperItemClearances` (only teardrops pass the
  `!includeZoneClearances` filter). Connectivity uses their outline (3.3).
* `UpdateTeardrops( commit, null, null, true )` (teardrop.cpp:L437-617) runs before every
  `FillAllZones`: deletes all teardrop zones (`BulkRemoveStaleTeardrops`) and rebuilds them for
  each track/arc (board order) x connected pads and vias (`GetConnectedPadsAndVias`, connected-item
  order = address order, R-1) with enabled parameters, then `setTeardropPriorities()`
  (L631-695) and track-to-track teardrops (`AddTeardropsOnTracks`, L697). UUIDs are derived
  deterministically (`teardropUuid`, L58-79). Geometry in `teardrop_utils.cpp`. New zones are
  appended to `board->Zones()`, so the **saved file order** depends on creation order.
* Boards whose teardrop zones predate 20230517 set `LegacyTeardrops` (parser) and are left
  alone.

---

## 5. What it needs from other subsystems (exhaustive)

### 5.1 kicad-geom

* Math: `VECTOR2I` (`EuclideanNorm` = `RoundedHypot` saturating, `SquaredEuclideanNorm` (int64,
  saturating), `Cross`, `Resize`, `Perpendicular`, `Distance`), `KiROUND`, `BOX2I` (`Inflate`,
  `GetInflated`, `Intersects`, `Contains` (point/box), `Merge`, `GetSizeMax`, `Centre`,
  `GetCenter`), `EDA_ANGLE` (`Cos`, `Sin`, `IsZero`, `AsDegrees`), `RotatePoint`.
* `SHAPE_POLY_SET`: `BooleanAdd/Subtract/Intersection` (1- and 2-operand forms),
  `Inflate( amount, CORNER_STRATEGY, maxError, simplify = false )`, `Deflate( amount, strategy,
  maxError )` (= `Inflate( -amount )`), `InflateWithLinkedHoles( amount, ROUND_ALL, maxError )`,
  `Fracture( simplify = true )`, `Simplify()`, `Chamfer( dist )`, `Fillet( radius, maxError )`,
  `ClearArcs()`, `ArcCount()`, `Append`, `AddOutline`, `NewOutline`, `Outline(i)`, `COutline`,
  `Polygon(i)`, `OutlineCount`, `IsEmpty`, `RemoveAllContours`, `DeletePolygon( i )`,
  `DeletePolygonAndTriangulationData( i, updateHash )`, `UpdateTriangulationDataHash`, `Area()`,
  `BBox()`, `BBox( 0 )`, `BBoxFromCaches()`, `BuildBBoxCaches()`, `Rotate( angle )`,
  `Contains( pt, subpoly = -1, accuracy, useBBoxCaches )`, `Collide( SHAPE*, clearance )`,
  `Collide( &poly, reach )`, `CloneDropTriangulation()`, `CacheTriangulation( false )` +
  `TriangulatedPolyCount`/`TriangulatedPolygon(i)`/`GetSourceOutlineIndex`, `GetHash()`
  (`checksum` MMH3 over counts and points - or structural equality, see R-13),
  `IterateWithHoles`/`SetVertex`.
* `SHAPE_LINE_CHAIN`: `Append`, `SetClosed`, `Insert`, `CPoint`, `CPoints`, `PointCount`,
  `Area( absolute )`, `BBox`, `PointInside( pt, accuracy )`, `Collide( pt )`, `Intersect( SEG,
  vec )`, `Intersect( chain, vec )`, `ClosestSegmentsFast`, `Rotate`, `Move`, `GenerateBBoxCache`.
* Shapes and collisions: `SHAPE_CIRCLE`, `SHAPE_SEGMENT`, `SHAPE_ARC`, `SHAPE_COMPOUND`,
  `SHAPE_NULL`, `TRI::Collide`, `SHAPE::Collide( pt, clearance )` / `( shape )` (G5).
* Converters: `TransformCircleToPolygon`, `TransformRingToPolygon`,
  `TransformTrapezoidToPolygon`, `TransformOvalToPolygon` (parser), `BuildConvexHull`,
  `KIGEOM::BoxToLineChain`.
* Indexes: `KIRTREE::DYNAMIC_RTREE<T,int,3>` (insert/remove/bulk load/search),
  `KIRTREE::PACKED_RTREE<T,int,2>` (builder, search, iteration), `TRIANGULATION_INDEX` +
  `AnyTriangleHit`, `POLY_YSTRIPES_INDEX::Build( poly )` / `Contains( pt, accuracy = 0 )`,
  `VERTEX_SET`/`VERTEX` (`createList`, `updateList`, `zOrder`, `isEar( bool )`, `nextZ/prevZ`,
  `i`, user data), `delaunator::Delaunator` (`triangles`, `halfedges`, `INVALID_INDEX`).
* `kicad_geom::stdsort` (`sort_by`, `nth_element` not required, see 4.5) for the unstable sorts
  in 4.0.

### 5.2 kicad-core

`KI_UNION_FIND` (`Unite`, `FindCompress`, `Connected`, `ComponentCount`), `KI_MST::FilterKruskal`,
`KIID` ordering (`operator>`), `HASH_128` / `MMH3_HASH` (if hashes are ported), `LSET` (iteration
in layer-id order, `Seq()`, `CuStack()`, `AllCuMask( n )`, `InternalCuMask`, `FrontBoardTechMask`),
`CopperLayerToOrdinal`, `IsCopperLayer`, `IsInnerCopperLayer`, `pcbIUScale.mmToIU`.

### 5.3 kicad-pcb (board model)

* Board: `Zones()`, `Tracks()`, `Footprints()`, `Drawings()`, `Markers()`, `GetEnabledLayers()`,
  `GetCopperLayerCount()`, `GetNetInfo()`, `GetDesignSettings()` (`m_MaxError`,
  `m_ZoneKeepExternalFillets`, `m_ZoneLayerProperties`, `GetBiggestClearanceValue()`,
  `m_DRCEngine`, `m_NetSettings`), `GetMaxClearanceValue()` (board.cpp:L1325-1355),
  `GetBoardPolygonOutlines( outline, inferIfNecessary = true )` (board.cpp:L3623,
  `convert_shape_list_to_polygon.cpp`; owner: see drc.md 4.10 edge clearance / plot-fab),
  `GetCopperZoneRTree( zone )` (DRC cache), `LegacyTeardrops()`, `GetProject()->GetLocalSettings(
  ).m_PrototypeZoneFill` (CLI: false), `IncrementTimeStamp()`.
* Items: `Type()`, `GetNetCode()/SetNetCode()`, `GetLayer()`, `GetLayerSet()`, `IsOnLayer`,
  `IsOnCopperLayer`, `GetBoundingBox()`, `GetPosition()`, `HitTest( pos, accuracy )`,
  `GetFlags() & IS_DELETED`, `m_Uuid`, `GetEffectiveShape( layer, FLASHING )`,
  `TransformShapeToPolygon( buf, layer, clearance, maxError, ERROR_OUTSIDE, ignoreLineWidth )`,
  `IsKnockout()`, `GetParentFootprint()`.
* PAD: `Padstack()` (`ForEachUniqueLayer`, `UnconnectedLayerMode`, `Drill().start/end`,
  `GetMaxHoleSize`), `ShapePos( layer )`, `GetAttribute`, `IsFreePad`, `GetNumber`,
  `GetShape( layer )`, `GetSize( layer )`, `GetDrillSize`, `GetDrillShape`, `GetOrientation`,
  `GetOffset/SetOffset`, `HasHole`, `ConditionallyFlashed`, `FlashLayer` (pad.cpp:L697 -
  **calls connectivity `IsConnectedOnLayer`**), `CanFlashLayer`, `IsBackdrilledOrPostMachined`,
  `Get/Set/ClearZoneLayerOverride(s)` (pad.cpp:L521-540), `GetRemoveUnconnected`,
  `GetEffectivePolygon( layer, ERROR_INSIDE/OUTSIDE )`, `TransformHoleToPolygon`,
  `GetPrimitives( layer )` (+ `IsProxyItem`), `GetCustomShapeInZoneOpt`, `GetThermalSpokeAngle`,
  `GetTeardropParams`, `GetClearanceOverrides`, copy construction (dummy pad for spokes),
  `GetBoundingBox( layer )`.
* PCB_VIA: the same flashing API (pcb_track.cpp:L2344-2440), `GetIsFree`, `TopLayer/BottomLayer`,
  `GetDrillValue`, `GetWidth( layer )`, `GetCenter`. PCB_TRACK/ARC: start, end, width.
* PCB_SHAPE: `GetConnectionPoints()`, `GetWidth()`, `TransformWithLineEndingsToPolygon`.
  Texts: `IsVisible`, `TransformTextToPolySet` (kicad-font). Barcodes: `GetBoundingHull`.
  Dimensions, text boxes, tables, targets: `TransformShapeToPolygon`.
* FOOTPRINT: `Pads()`, `Zones()`, `GraphicalItems()`, `Reference()`, `Value()`,
  `GetCourtyard( side )`, `IsNetTie()`, `GetNetTiePads( pad )`, `GetDuplicatePadNumbersAreJumpers`,
  `JumperPadGroups()`, `GetTransform()`, `GetAttributes() & FP_JUST_ADDED`, `GetLayer()`.
* ZONE: all of 3.6 plus `GetIsRuleArea`, `HasKeepoutParametersSet`, `GetDoNotAllowZoneFills`,
  `IsTeardropArea`, `IsCopperThieving`, `GetAssignedPriority`, `GetNumCorners`, `GetMinThickness`,
  `GetLocalClearance()` (always has a value: 0 for rule areas), `GetCornerSmoothingType`,
  `GetCornerRadius`, `GetFillMode`, hatch getters (`GetHatchThickness/Gap/Orientation/
  SmoothingLevel/SmoothingValue/HoleMinArea`), `LayerProperties()`, `GetThievingSettings`,
  `GetIslandRemovalMode`, `GetMinIslandArea`, `Outline()`.

### 5.4 kicad-drc

`DRC_ENGINE::EvalRules( type, a, b, layer )` returning `DRC_CONSTRAINT` (`IsNull()`,
`GetValue().Min()/Opt()`, `Value().Max()`) for `PHYSICAL_CLEARANCE_CONSTRAINT`,
`CLEARANCE_CONSTRAINT`, `PHYSICAL_HOLE_CLEARANCE_CONSTRAINT`, `HOLE_CLEARANCE_CONSTRAINT`,
`EDGE_CLEARANCE_CONSTRAINT`, `THERMAL_RELIEF_GAP_CONSTRAINT`, `THERMAL_SPOKE_WIDTH_CONSTRAINT`;
`EvalZoneConnection( item, zone, layer )`; `QueryWorstConstraint` (via
`GetBiggestClearanceValue`). Argument order matters for rule conditions: the filler calls
clearances as `( zone, item )` and thermal/zone-connection as `( pad|via, zone )`. The engine must
be initialised (`InitEngine`) before filling. The DRC side needs from connectivity what drc.md 5.4
lists.

---

## 6. Output formats

No file format of its own. Effects visible in outputs:
* `.kicad_pcb` (writer `pcb_io_kicad_sexpr.cpp:L3636-3770`): `(fill yes ...)` iff `IsFilled()`;
  per layer in `GetLayerSet().Seq()` order, per outline of `GetFilledPolysList( layer )`:
  `(filled_polygon (layer "...") [(island yes)] (pts ...))` with `IsIsland( layer, ii )` (true
  for all outlines of net-less zones). Teardrop zones (count, order, geometry). Net codes of
  tracks/vias/shapes after propagation.
* DRC report/JSON: unconnected-items entries (edge source/target items, order 4.5), dangling
  tracks/vias, isolated copper, starved thermals (all in drc.md).
* Warning text on non-convergence (4.13) goes to wxLog (stderr in CLI).

---

## 7. Backward-compatibility behaviour

* Parser-side (owned by pcb-io): `filled_areas_thickness yes` (pre-v6 stroked fills) inflates
  the loaded fills by `minThickness / 2` (`InflateWithLinkedHoles`, `ARC_HIGH_DEF / 2`); legacy
  `fill_segments` converted with `TransformOvalToPolygon`; single-layer legacy `filled_polygon`
  without `(layer)` goes to the first layer; `island` flags loaded into `m_insulatedIslands`;
  non-copper and rule-area zones get net 0; teardrop zones from files older than 20230517 set
  `LegacyTeardrops( true )` which disables `UpdateTeardrops` (`pcb_io_kicad_sexpr_parser.cpp:L10365-10473`).
* `SetNeedRefill( false )` after load; the CLI does not test `NeedRefill` (it refills only when
  asked).
* Old boards without `remove_unconnected` padstack data: flashing defaults to `KEEP_ALL`
  (`ConditionallyFlashed` false) - no overrides.
* `m_ZoneKeepExternalFillets` (board setting, 5.1-era behaviour) changes `BuildSmoothedPoly` and
  the knockout reach.

---

## 8. Multi-threading in the C++ and the sequential port

| C++ | Where | Sequential port |
|---|---|---|
| `BOARD::CacheTriangulation` per (zone, layer) tasks | board.cpp:L1357 | loop; independent |
| `CN_ZONE_LAYER::BuildIndexes` `submit_loop` | algo.cpp:L571-587 | loop |
| `searchConnections` `submit_loop` over dirty items; `Connect()` under per-item mutex; deferred via nets under a mutex, then sorted | algo.cpp:L298-372 | loop in list order; `m_connected` kept sorted by the chosen key (R-1); deferred nets: group by item, same rule |
| `updateRatsnest` `submit_loop` UpdateNet, then OptimizeRNEdges | data.cpp:L178-214 | loop over dirty nets; per-net state only |
| Zone fill DAG with `detach_task`, atomics, inline tessellation | zone_filler.cpp:L1026-1193 | Kahn topological order (roots in `toFill` order, successors in discovery order); identical results because fills read only completed dependencies or outlines (R-9) |
| Refill waves read a snapshot | L1508-1548 | snapshot then refill in candidate order |
| Out-of-board island check `submit_blocks` | L1694-1790 | loop in `polys_to_check` order, then deletions |
| Bbox / effective-shape / courtyard caches warmed up "to avoid races" | L653-676 | compute lazily or eagerly; pure |
| `KI_UNION_FIND` lock-free concurrent `Unite` | union_find.h | single-threaded version; same roots (minimum index) |

No thread-scheduling effect reaches any output once R-1/R-2 are handled.

---

## 9. Tests and differential validation

### 9.1 C++ unit tests to port (`qa/tests/`)

| Test | Asserts | Package |
|---|---|---|
| `common/test_union_find.cpp` | reference partition, root = component minimum, compression keeps partition, degenerate sizes, reset | CN-03 / kicad-core |
| `common/test_filter_kruskal.cpp` | equals plain Kruskal; equal weights; few distinct weights; disconnected forest; pre-seeded forest; reproducible; independent of input order and orientation; degenerate | CN-03 |
| `pcbnew/test_connectivity_clusters.cpp` (213) | on `issue17429`, `issue16182`, `issue14559`, `issue8909` (+ list at L45-58) after `FillZones` + `BuildConnectivity`: every item in exactly one cluster, clusters are connected components of the adjacency, no adjacency crosses clusters; per net, ratsnest edge count == cluster count - 1 (excluding kept single zone islands) | CN-02, CN-03 (fills from the file if ZF not ready) |
| `pcbnew/test_tracks_cleaner.cpp` | uses connectivity + `IsDangling` (cleaner is T3) | reference only |
| `pcbnew/test_issue25621_via_flashing_contact_layer.cpp` | via flashing per contact layer (`FlashLayer` / `IsConnectedOnLayer`) | CN-04 |
| `pcbnew/test_board_connected_items.cpp` | board-model connected-item enumeration incl. footprint zones/shapes; thieving zones reject nets | board model |
| `pcbnew/test_zone_filler.cpp` (4588, 62 cases, fixture `KI_TEST::FillZones` = `ZONE_FILLER::Fill( board zones )` + `BuildConnectivity`, **no teardrop update**) | `BasicZoneFills` (`zone_filler` board: grow pads 2/4/6 and arcs 8/12 -> exactly those clearance violations), `NotchedZones`, `RegressionZoneFillTests` (issue18, 2568, 3812, 5102, 5313, 5320, 5567, 5830, 6039, 6260, 6284, 7086, 14294, fill_bad: no `DRCE_CLEARANCE` after fill), `RegressionSliverZoneFillTests`, `RegressionTeardropFill`, net ties, priority/islands, via flashing, hatch thermals and via connection modes, shallow arcs, keepouts with iterative refill, TH inner-layer flashing, 45-degree thermals, coincident pads, layer-specific rules (`issue23339_zone_layer_rules` + `.kicad_dru`), min width after knockout, same/different-net overlaps, thermal reliefs to nowhere, cascading island refill (`zone_refill_cascading_islands`), convergence limit (`zone_refill_convergence_limit`), thieving cases (`zone_thieving_track_bisection` ...), scheduler / knockout margin / narrow bridge / same-net border cases (L3777-4588) and teardrop geometry cases (L1090-2790) | ZF-*, TD-* |
| `pcbnew/drc/test_drc_copper_thieving.cpp`, `test_drc_backdrill_postmachining.cpp`, `test_drc_npth_slot_clearance_issue24799.cpp` | fills with thieving, backdrill knockouts, NPTH slot clearance then DRC | ZF-03, ZF-06 |
| `pcbnew/drc/test_drc_copper_conn.cpp`, `test_drc_regressions.cpp` (see drc.md 9.2) | unconnected / dangling counts | CN-03/04 with DRC |

Data: `qa/data/pcbnew/*.kicad_pcb` named in the tests (zone_filler, notched_zones, fill_bad,
issue*.kicad_pcb, teardrop_*.kicad_pcb, zone_refill_*.kicad_pcb, intersectingzones, ...).

### 9.2 Differential tests against the oracle (`rust/docs/ORACLE.md`)

* **Net propagation / connectivity at load**: `kicad-cli pcb upgrade --force` re-save (runs
  `BuildConnectivity`) vs Rust load + build + save: compare `(net N)` of every
  segment/arc/via/shape (board re-save byte equality, shared with pcb-io).
* **Ratsnest / unconnected**: `pcb drc --format json --severity-all` on `demos/*` and
  `qa/data/pcbnew/*`: compare the `unconnected_items` list (count, order, item UUIDs, positions)
  and dangling entries. Add a C++ oracle driver (`tests/oracle/` in `kicad-connectivity`) linking
  `connectivity_*.cpp` + `ratsnest_data.cpp` that dumps, per net, the edges `(source pos, target
  pos, source parent uuid, target parent uuid, weight)` in `GetEdges()` order and the cluster
  membership per item - the strongest check for R-1/R-2.
* **Zone fill**: copy board + project to scratch, `kicad-cli pcb drc --refill-zones --save-board
  in.kicad_pcb`, extract every zone's `(filled_polygon ...)` sections (by zone UUID, layer,
  outline index, `island` flag, point list) and compare with the Rust refill of the same input
  **vertex for vertex**. Normalise nothing inside polygons. Start with boards without teardrops
  (or with `LegacyTeardrops`), then teardrop boards once TD-* is done. Report the first differing
  outline and the first differing point. Boards: `demos/*/*.kicad_pcb`, the test boards of 9.1.
* Per-stage C++ oracle (`tests/oracle/` in `kicad-zone`): a driver linking `zone_filler.cpp` that
  dumps the intermediate polygons of `fillCopperZone` for a chosen (zone, layer) (after thermal
  knockout, clearance holes, spokes kept, deflate, connect_nearby_polys, inflate, final) - lets a
  mismatch be located at the first diverging stage.

---

## 10. Proposed Rust module layout and work packages

### 10.1 Modules

`crates/kicad-connectivity/src/`:

```
lib.rs
items.rs          CnAnchor, CnItem, CnZoneLayer, CnList, CnCluster      (connectivity_items.*)
rtree.rs          CnRtree over kicad-geom DynamicRtree<_, i32, 3>       (connectivity_rtree.h)
algo.rs           ConnectivityAlgo: build, search_connections, visitor, search_clusters,
                  propagate_nets, jumper pads, fill_isolated_islands_map (connectivity_algo.*)
edge.rs           CnEdge (+ stable_sort_compare)                         (connectivity_algo.h)
ratsnest.rs       RnNet: add_cluster, triangulate, kruskal_mst, optimize (ratsnest_data.*)
data.rs           ConnectivityData facade + queries                      (connectivity_data.*)
from_to.rs        FromToCache (T2)                                       (from_to_cache.*)
```

Items reference board items through the board-model handle type (`docs/design/`). `CnItem` lives
in an arena (`Vec<CnItem>`), referenced by `CnItemId(u32)`; `m_connected` is a sorted
`Vec<CnItemId>` (R-1 decides the sort key). Anchors: `Vec<CnAnchor>` per item with
`(item, anchor_idx)` ids; edges hold anchor ids plus optional "synthetic" anchors created by
`OptimizeRNEdges` (store `(pos, CnItemId)` inline).

`crates/kicad-zone/src/`:

```
lib.rs
zone_ops.rs       BuildSmoothedPoly, GetInteractingZones, TransformSmoothedOutlineToPolygon,
                  zone TransformShapeToPolygon (zone.cpp subset; may live in kicad-pcb)
index.rs          INDEXED_ITEM, PackedRtree indexes, query_index (seq sort)
filler.rs         Fill(): pre/post flashing passes, scheduler, island removal, out-of-board
knockouts.rs      knockoutThermalReliefs, buildCopperItemClearances,
                  buildDifferentNetZoneClearances, subtractHigherPriorityZones, addKnockout*
copper_fill.rs    fillCopperZone, fillNonCopperZone, fillSingleZone, postKnockoutMinWidthPrune,
                  dropSubResolutionOutlines
connect_polys.rs  VERTEX_CONNECTOR, connect_nearby_polys
thermal.rs        buildThermalSpokes, buildHatchZoneThermalRings
hatch.rs          addHatchFillTypeOnZone
thieving.rs       addCopperThievingPattern (T3)
refill.rs         iterative refill loop + refillZoneFromCache
tool.rs           fill_all_zones (FillAllZones headless flow)
teardrop/         TEARDROP_MANAGER + utils (TD-01, TD-02)
```

### 10.2 Work packages

Line counts are C++ lines to port. Thin slice: CN-01 -> CN-02 -> CN-04a gives load-time net
propagation (byte-equal re-save); + CN-03 gives DRC unconnected items; ZF-01..ZF-04 give a solid
and hatched refill; ZF-05 makes it match the default (iterative) C++ behaviour; TD-* handle
teardrop boards.

| Id | Title | C++ files (line ranges) | ~Lines | Depends on | Tier | Validating tests |
|---|---|---|---|---|---|---|
| CN-01 | Connectivity items, list, R-tree | `connectivity_items.h` (all), `connectivity_items.cpp` (all), `connectivity_rtree.h` | 1200 | kicad-geom G7 DynamicRtree, G4b triangulation + `TRIANGULATION_INDEX`/`AnyTriangleHit`; board model | 1 | unit tests per item type (anchors, layer ordinals, `CN_CLUSTER::Add` ranking) |
| CN-02 | Build, contact rules, clusters, net propagation, islands | `connectivity_algo.h` (all), `connectivity_algo.cpp` L49-657, L685-1147 | 1450 | CN-01; G5 collisions; item `GetEffectiveShape`; `KI_UNION_FIND`; `stdsort` | 1 | `test_connectivity_clusters` (cluster part); oracle re-save net equality (9.2); C++ oracle cluster dump |
| CN-03 | Ratsnest | `ratsnest_data.h` (all), `ratsnest_data.cpp` L1-594, `filter_kruskal.h`, `union_find.h` (if not in kicad-core) | 1100 | CN-02; G7 delaunator; `stdsort` | 1 | `test_union_find`, `test_filter_kruskal`, `test_connectivity_clusters` (ratsnest part); C++ oracle edge dump; DRC unconnected JSON diff |
| CN-04 | CONNECTIVITY_DATA facade and queries | `connectivity_data.h` (all), `connectivity_data.cpp` L1-330, L418-1012, L1147-1153; `board.cpp` L369-376, L538-566 | 1100 | CN-02, CN-03 | 1 | `test_issue25621_via_flashing_contact_layer`; DRC dangling/unconnected regressions (drc.md 9.2) |
| CN-05 | From-to cache | `from_to_cache.h/.cpp` | 360 | CN-04 | 2 | DRC rule tests using `fromTo()` |
| ZF-01 | Zone geometry helpers used by the filler | `zone.cpp` L545-593, L815-975, L1105-1133, L1715-2140 | 600 | board model, kicad-geom G4a | 2 | unit tests vs C++ oracle dumps of `BuildSmoothedPoly` (chamfer/fillet, same-net merge, board clip, apron) |
| ZF-02 | Filler driver: indexes, flashing passes, sequential DAG, islands, out-of-board, second flashing pass | `zone_filler.h`; `zone_filler.cpp` L247-590 (keys, indexes, gates), L590-1312 and L1661-1981 | 1350 | ZF-01, CN-04 (`FillIsolatedIslandsMap`), kicad-drc `EvalRules`, `POLY_YSTRIPES_INDEX`, `GetBoardPolygonOutlines` | 2 | `RegressionZoneFillTests`, `RegressionZonePriorityIsolatedIslands`, `RegressionViaFlashingUnreachableZone`, `OverlappingPriorityPadFlashing`, `RegressionTHPadInnerLayerFlashing` |
| ZF-03 | Copper fill core and knockouts | `zone_filler.cpp` L1988-3110 (incl. `connect_nearby_polys`, L69-245 `VERTEX_CONNECTOR`), L3110-3666 | 1900 | ZF-02, kicad-geom `VERTEX_SET` | 2 | `BasicZoneFills`, `NotchedZones`, `ZoneFillClearsLineEndings`, `RegressionNetTie`, `RegressionCoincidentPadClearance`, `ZoneViaNetClearance`, `ZoneLayerSpecificRules`, `RegressionKeepoutBoundaryMissingFill`, backdrill DRC tests; vertex-for-vertex refill diff (9.2) |
| ZF-04 | Thermal spokes, hatch rings, hatch pattern | `zone_filler.cpp` L3671-4155, L4355-4584 | 750 | ZF-03 | 2 | `RegressionThermalReliefAnnularRing45`, `RegressionThermalReliefsToNowhere`, `ThermalOverrideOnSolidZoneKeepsSpokes`, `HatchZone*`, `SameNetBorderIdenticalAcrossFillModes` |
| ZF-05 | Iterative refill | `zone_filler.cpp` L1314-1659, L4587-4820 | 600 | ZF-03, ZF-04 | 2 | `RegressionZoneClearanceWithIterativeRefill`, `RegressionCascadingIslandRefill`, `IterativeRefillConvergenceLimit`, `RegressionIterativeRefillRespectsKeepouts`, `RegressionSameNet*`, `RegressionHatchedZonePriorityRefill`, `RegressionIterativeRefillFullWidthBridge` |
| ZF-06 | Headless FillAllZones + CLI `--refill-zones` | `tools/zone_filler_tool.cpp` L180-270, L526-541; `pcbnew_jobs_handler.cpp` L2993-2998, L3065-3076 | 150 | ZF-05, TD-01 (or a guard), kicad-cli | 2 | end-to-end `pcb drc --refill-zones --save-board` diff (9.2) |
| ZF-07 | Copper thieving | `zone_filler.cpp` L4158-4352 | 200 | ZF-03 | 3 | `CopperThievingZone_*`, `test_drc_copper_thieving` |
| TD-01 | Teardrop manager | `teardrop/teardrop.h/.cpp`, `teardrop_parameters.*`, `teardrop_types.h` | 1450 | CN-04, board model (commit-less add/remove of zones) | 2 | `RegressionTeardropFill`, teardrop geometry cases in `test_zone_filler.cpp` |
| TD-02 | Teardrop geometry | `teardrop/teardrop_utils.cpp` | 1750 | TD-01 | 2 | as TD-01, plus refill diff on teardrop boards |
| CN-06 | Incremental connectivity, local/dynamic ratsnest | `connectivity_algo.cpp` L659-683, `connectivity_data.cpp` L154-176, L285-317, L332-416, L1016-1145; `rn.cpp` L596-668 | 600 | CN-04 | 3 | GUI only; not needed by CLI |

Interim guard for ZF-06 before TD-01: if any pad/via has enabled teardrop parameters (and the
board is not `LegacyTeardrops`), report "teardrop regeneration not supported" instead of silently
deleting teardrops.

---

## 11. Risks and traps

* **R-1 Pointer-ordered connected lists.** `CN_ITEM::m_connected` is sorted by `CN_ITEM*`
  address; `GetConnectedTracks/Pads` (`std::set<T*>`) and `GetNetItems` sort by `BOARD_ITEM*`
  address. Observable via `GetConnectedPadsAndVias` (teardrop creation order -> zone order in
  the saved file), `OptimizeRNEdges` (which zone vertex wins exact ties, which 3 zone pairs are
  tried), and DRC "first connected item" choices (drc.md 4.12). Allocation order in C++: CN_ITEMs
  of pads/tracks/shapes are `new`'d in list order, zone layers earlier, so **sort by list index
  (`m_listIndex`)** is the best approximation; for board items use board order (tracks, then
  footprints/pads, then drawings). Record every mismatch the oracle finds in `PORT_STATUS.md`.
* **R-2 Cluster-address sort in the triangulator** (`rn.cpp:L254-259`). Only matters for anchors
  of different clusters at the exact same position (e.g. items on different layers at one point).
  Clusters are `make_shared` in creation order inside `SearchClusters` (before the OriginNet
  sort), so use the creation index as the key. glibc may reuse freed buffers (the `clusters`
  vector growth, `m_items.reserve( 64 )`), so the C++ order is not guaranteed - document
  mismatches found by the oracle.
* **R-3 Unstable `std::sort`** in `SearchClusters` (OriginNet ties) and
  `optimizeZoneToZoneAnchors` (distance ties): use `kicad_geom::stdsort` (PORTING.md 9).
* **R-4 Layer ordinals.** Connectivity layer ranges are copper ordinals (B_Cu = 31), not layer
  ids; `CN_ITEM::Layer()` returns the ordinal; `GetBoardLayer()` maps back. THT pads are
  `0..31`, so a via connected to a THT pad and a F_Cu track counts as "single layer" in the via
  dangling test.
* **R-5 Zone items built by `Build()` have no anchors**, those added by `CN_LIST::Add( ZONE*,
  layer )` (incremental) have all outline vertices. Ratsnest nodes therefore never come from zone
  items after a full build. Reproduce both paths exactly; never "fix" it.
* **R-6 Full rebuild instead of incremental** is equivalent for every CLI output (4.4, 4.7) - but
  only if `FillIsolatedIslandsMap` sees the fresh fills *and* the via deferred-net rule is
  re-applied; test with boards having net-less vias inside zones.
* **R-7 Net propagation changes saved data** (pad-less clusters take the first member's net in
  list order; conflicting clusters untouched). Board item order must match the file.
* **R-8 Circular dependency fill <-> connectivity <-> flashing.** `FlashLayer` queries
  connectivity (`IsConnectedOnLayer`, zones excluded); the filler uses `FlashLayer` and the
  zone-layer overrides it set itself; connectivity contact tests use the overrides
  (`ZLO_FORCE_NO_ZONE_CONNECTION`). The order is: load build -> fill pre-pass overrides -> fills
  -> islands (connectivity with overrides) -> second flashing pass -> full rebuild. Overrides are
  run-time state on pads/vias (not saved); they must survive into the DRC run.
* **R-9 Scheduler determinism.** The C++ fill is deterministic only because every read of another
  zone's fill is gated by the same predicate that creates the dependency edge
  (`HigherPriority`, different net, `zoneKnockoutMayInteract`). Keep that gate identical in the
  three places (dependency discovery, `buildCopperItemClearances` /
  `buildDifferentNetZoneClearances`, `refillZoneFromCache`). Any zone read outside the gate (a
  same-net zone, a lower-priority zone) must read only its outline, never its fill, or the result
  would depend on fill order.
* **R-10 Island flags are not renumbered.** `SetIsIsland( idx )` followed by deleting a lower
  index (descending loop with AREA mode), later out-of-board deletions, and the iterative refill
  replacing fills without clearing `m_insulatedIslands` all leave stale indices. The saved
  `(island yes)` flags reflect this; port literally.
* **R-11 `m_preKnockoutFillCache` survives across `Fill()` calls** of the same filler object (only
  matters if the port reuses a filler).
* **R-12 Polygon operation parameters.** Every boolean / inflate must use the exact corner
  strategy, max error, simplify flag and operand append order listed in 4.9-4.14; e.g. the
  min-width cycle deflates with `CHAMFER_ALL_CORNERS` and re-inflates with `ROUND_ALL_CORNERS`
  and `simplify = true`; the spoke test area uses CHAMFER both ways and no simplify; non-copper
  zones deflate unconditionally; zone `TransformShapeToPolygon` adds `maxError` for
  `ERROR_OUTSIDE` and uses `InflateWithLinkedHoles`; `TransformSmoothedOutlineToPolygon` uses
  `Inflate` (not linked holes) and `Fracture()`.
* **R-13 Hashes.** `ZONE::BuildHashValue` uses `SHAPE_POLY_SET::GetHash()`, which returns the
  cached `m_hash` when valid (set by triangulation / `UpdateTriangulationDataHash`), else the MMH3
  checksum. The iterative convergence test only compares before/after; structural equality of
  the point lists is equivalent barring collisions. The refill result cache is a pure
  optimisation and may be omitted.
* **R-14 Triangulation-based contact tests** (`CN_ZONE_LAYER`, DRC zone R-trees): containment of
  points on triangle edges and the triangulator's own output must match `CacheTriangulation(
  false )` exactly, otherwise island classification and dangling tests flip on edge cases.
* **R-15 `VERTEX_SET` simplification** (`m_TriangulateSimplificationLevel = 50`) inside
  `connect_nearby_polys` decides which vertices may be bridged; it must be the same code as the
  triangulator's list builder.
* **R-16 Footprint zones**: not connectivity items, not filled by `FillAllZones`, but they are
  knockout sources/keepouts and dependency candidates (`forEachBoardAndFootprintZone`) and their
  outlines are in library coordinates (`GetBoardOutline()` transform).
* **R-17 Generators** (`OnZonesFilled`, `RegenerateDirtyGenerators`, `refillAroundGenerators`):
  via-stitching/tuning generators can change the board after fill. Out of scope; detect boards
  with generators that react to fills and report.
* **R-18 Integer arithmetic**: C++ `int` overflow is UB but does not occur in practice; use `i32`
  with the same intermediate widths (`int64_t` in `DIST_PAIR`, `SEG::ecoord` squares), `double`
  for `minArea`, island areas and `distances` in `VERTEX_CONNECTOR`. C++ `%` and Rust `%` agree on
  negatives (hatch/thieving grid origin).
* **R-19 `GetLocalClearance().value()`** for zones always has a value (rule areas 0); do not model
  it as a fallible optional in the port.
* **R-20 `EvalRules` argument order and layer** (`( zone, item )` for clearances,
  `( item, zone )` for thermal/connection) change rule matching for custom rules with `A`/`B`
  conditions.
* **R-21 Spoke test point** is chain index 3 in both spoke builders; the custom-pad spoke uses
  `seg.B + direction` (extended by min thickness). Keep the chain point order.
