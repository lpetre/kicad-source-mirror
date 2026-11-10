# kicad-connectivity: port status

Crate layout (connectivity-zones.md 10.1): one module per C++ file. `algo`, `edge`,
`ratsnest`, `data`, `from_to` are skeletons filled by CN-02 .. CN-05.

## CN-01: connectivity items, item list, r-tree

| C++ | Rust | State |
|---|---|---|
| `connectivity_items.h/.cpp` `CN_ANCHOR` (incl. `IsDangling`, `ConnectedItemsCount`, `Dist`) | `items::CnAnchor` | ported |
| `CN_ITEM` (layers as copper ordinals, `BoardLayerFromOrdinal`, `AnchorCount`, `GetAnchor`, `Connect`, `RemoveInvalidRefs`, `Net`) | `items::CnItem` | ported |
| `CN_ZONE_LAYER` (`BuildIndexes`, `ContainsPoint`, `Collide`, `HasSingleConnection`, anchors) | `items::CnZoneLayer` (the zone part of a `CnItem`) | ported |
| `CN_LIST` (`Add` for pad / track / arc / via / zone / zone layer / shape, bulk add, `RemoveInvalidItems`, `ClearDirtyFlags`, `FindNearby`, `appendItem` / list indices) | `items::CnList` | ported |
| `CN_CLUSTER` (`Add` net ranking, origin pad, conflicting, `Contains`) | `items::CnCluster` | ported |
| `PCB_SHAPE::GetConnectionPoints` (board-level shapes) | moved to kicad-pcb (`PcbShape::connection_points`, the CN-01 change request); `CnList::add_shape` calls it | ported (in kicad-pcb); verified through the anchors of the `rev_shapes` / `test_copper_graphics` goldens |
| `connectivity_rtree.h` `CN_RTREE` | `rtree::CnRtree` over `kicad_geom` `DynamicRTree<_, 3>` | ported |

Ported tests / verification:

- `tests/cn01_items_golden.rs`: replays the `cn_item` lines of the reference pcb-dump
  `connectivity` section (goldens in `tests/golden/`, generator `tests/oracle/gen_goldens.sh`,
  format in `docs/PCB_DUMP_FORMAT.md` section 8) on 19 corpus boards: parent, type,
  `CanChangeNet`, ordinal layer span, board layer, `BBox`, stored anchors, `AnchorCount`,
  `GetAnchor`, zone outline size, net of pads / zones. Currently 16 of 19 boards compared
  (742 fields, all equal); 3 boards are blocked by the `EDA_TEXT::ResolveFont` model stub, and
  pads (footprint parser pending) and the zones of boards whose zone parse is pending are left
  out on both sides. Those are compared automatically once the parser packages land.
- Unit tests in `items.rs` / `rtree.rs`: anchors / layers / nets per item kind (pads built
  through the model API: SMD on B.Cu, PTH, paste-only), sorted unique `Connect`, ordinal round
  trip, bulk add, `RemoveInvalidItems`, dangling / counts without cluster, `CN_CLUSTER::Add`
  ranking, r-tree layer separation.
- The `connected`, `anchor_connected_count`, `anchor_dangling`, `zone_single_connection`
  fields and the `cn_cluster` / `rn_net` / `cn_dangling` / `cn_island` sections need the
  contact search; they are replayed by CN-02 .. CN-04.
- No C++ Boost test applies to the item layer alone (`test_connectivity_clusters` etc. need
  CN-02 / CN-03).

### Deviations (CN-01)

- `CN_ITEM::m_connected` is sorted by `CnItemId` (= list order) instead of heap address
  (connectivity-zones.md R-1, board-model.md 3.1). The dump prints `connected` in list order
  for the same reason. REV-CN measured the reference: its order is neither list order nor
  stable (see "REV-CN" below and `docs/design/review-connectivity.md`).
- `CnItem::bbox()` has no side effect: C++ `BBox()` recomputes the box lazily and clears the
  dirty flag; the parent cannot change while the list lives, so the value is identical; the
  only effect of the flag in C++ is a symmetric pair being tested from both sides
  (connectivity-zones.md 4.2). `update_bbox()` re-reads the parent explicitly.
- `CnItem` caches the parent's net code; the net propagation updates it with `set_net` (the
  caller applies the change to the board), board-model.md 13.
- `CN_ZONE_LAYER` reads `IsTeardropArea()` once at construction.
- `CN_CLUSTER::m_netRanks` is a `HashMap` used for lookups only (no iteration).

### Known gaps (CN-01)

- `ZONE::CacheTriangulation` is a model stub; the golden test triangulates the fills directly.
- Pad items are verified only by unit tests until the footprint parser (PCBIO-06) lands.

## CN-02: connectivity build, contact rules, clusters, net propagation, islands

| C++ | Rust | State |
|---|---|---|
| `KI_UNION_FIND` (`libs/core/include/core/union_find.h`) | `algo::KiUnionFind` | ported (single-threaded form of the same statements) |
| `CN_CONNECTIVITY_ALGO::Build` | `CnConnectivityAlgo::build` | ported (sequential `BuildIndexes`; progress reporter not ported) |
| `Add` / `Remove` / `markItemNetAsDirty` / `MarkNetAsDirty` / `RemoveInvalidRefs` / `Clear` | `add` / `remove` / `mark_net_as_dirty` / ... | ported |
| `ITEM_MAP_ENTRY`, `ItemExists`, `ItemEntry`, `IsNetDirty`, `ClearDirtyFlags`, `GetDirtyClusters`, `NetCount`, `ForEachItem`, `ForEachAnchor` | same names | ported |
| `searchConnections` (incl. deferred via-net rule) | `search_connections` (private, run lazily by `search_clusters*`) | ported, sequential in list order |
| `CN_VISITOR` (`operator()`, `checkZoneItemConnection`, `checkZoneZoneConnection`), `flashingFor`, `ItemsTouchOnLayer` | `algo::cn_visit`, `items_touch_on_layer` | ported |
| `SearchClusters` (both overloads, all modes) | `search_clusters`, `search_clusters_ex` | ported; final sort with `kicad_geom::stdsort::std_sort` |
| `PropagateNets`, `propagateConnections`, `updateJumperPads` | `propagate_nets` | ported |
| `FillIsolatedIslandsMap` (both paths), `GetClusters` | `fill_isolated_islands_map`, `get_clusters` | ported |
| `LocalBuild`, `m_isLocal`, `m_globalConnectivityData`, `SetProgressReporter` | - | skipped: tier 3 (CN-06) / no progress reporting |

Clusters (`shared_ptr<CN_CLUSTER>`) live in an arena of the algorithm (`cluster(id)`), ids in
creation order (R-2 key for CN-03). `SetNetCode` calls (deferred via nets, propagation) update
the cached net of the item and are recorded; the caller applies `take_net_changes()` with
`Board::set_item_net( id, net, false )` (board-model.md 13).

Verification:

- `tests/cn02_algo_golden.rs` replays the reference pcb-dump `connectivity` goldens after the
  load path (`Build`, `PropagateNets`, `GetClusters`): `cn_item` `connected` (as list
  positions), `net`, `zone_single_connection`, the `ratsnest#k` / `propagate#k` clusters
  (origin net and name, orphaned, conflicting, members in order) and `cn_island`
  (`FillIsolatedIslandsMap` with re-added zones, map as `ZONE_FILLER::Fill` builds it).
  Goldens for 11 footprint-free boards were added (`gen_goldens.sh`). State at hand-over:
  15 of 30 boards load, 9 fully (via_dangling, issue18839, via_stacks, issue10697,
  issue19574, teardrop_multi_inside_via, teardrop_offcenter_two_segment,
  tuning_generators_load_save, via_stack_track_width), 812 fields, 0 mismatches. Boards whose
  parse skipped footprints are compared on `connected` (restricted to items both sides have)
  and on the propagation clusters whose golden members all exist on the Rust side. Every
  board with a zone currently stops in the parser at the `ZONE::SetLayerSetAndRemoveUnusedFills`
  model stub, so at CN-02 hand-over the zone contact paths (`checkZoneItemConnection`,
  `checkZoneZoneConnection`), the deferred via-net rule and the island map were not
  verified against C++. **They are now** (zone parser landed; REV-CN boards `rev_*`, see
  "REV-CN" below); pads still wait for the pad parser (PCBIO-07).
- `tests/cn02_union_find.rs`: port of `qa/tests/common/test_union_find.cpp` (7 cases; the
  two threaded cases interleave the edges serially as 4 threads would).
- `cn02_clusters_are_connected_components` (same file as the golden test): port of the
  cluster half of `qa/tests/pcbnew/test_connectivity_clusters.cpp` on its boards (where they
  load) plus the golden boards; fills are read from the files (`FillZones` is ZF-*). The
  ratsnest half is CN-03's.

### Deviations (CN-02)

- Contact search: C++ skips a dirty candidate whose *address* is below the visiting item's;
  `BBox()` clears the dirty flag of an item when its own search starts, so a pair of dirty
  items is always tested by the visitor that runs later and also by the earlier one unless
  the later item has the lower address. The port compares `CnItemId` (list order), keeps the
  flags until the end of the search and visits each dirty pair once. The zone tests are
  symmetric; the item / item test is **not** (`SHAPE_SEGMENT` collision rounds the half
  widths as `( wA + 1 ) / 2 + wB / 2`), so for a pair of dirty items the port evaluates
  `ItemsTouchOnLayer` with both operand orders (REV-CN finding F-2; before that fix an
  odd-width track 1 nm inside the touching distance of an even-width one was left
  unconnected when it came first in the board). Where the C++ runs only one of the two
  visitors on such a pair its result depends on the heap; the port gives the result of both.
- Candidates of one r-tree query are collected before being visited (the visitor mutates the
  list); nothing the visitor reads changes during a search, so the order is irrelevant.
- Deferred via nets are grouped by `CnItemId` instead of sorted by pointer (groups are
  independent; the result is "keep the existing net if any zone matches, else the minimum").
- `updateJumperPads` iterates footprints by `FootprintId` instead of address (independent per
  footprint).
- `FillIsolatedIslandsMap` takes `BTreeMap<ZoneId, ..>` (board order) instead of a map keyed
  by `ZONE*`: the zones are re-added in that order, which decides the list position of the
  new zone items and therefore the push order of `m_IsolatedOutlines` /
  `m_SingleConnectionOutlines` (callers sort the former; the latter's order follows the C++
  allocation order there too).
- The incremental `FillIsolatedIslandsMap` path (Remove/Add of the zones) is ported as is,
  not replaced by a full rebuild (R-6 is therefore moot); `searchConnections` drops the
  invalid items from the list but keeps them in the arena (C++ deletes them). The
  `RN_NET::RemoveInvalidRefs` part of `m_parentConnectivityData->RemoveInvalidRefs()` belongs
  to CN-04.
- `IsNetDirty` / `GetDirtyClusters` return "not dirty" for a net beyond the table (C++
  indexes unchecked).

### Known gaps (CN-02)

- Pads await the pad parser (PCBIO-07); zones and the via-net rule are verified (REV-CN).
- `ZONE::CacheTriangulation` is a model stub: callers (and the tests) triangulate the fills
  before `build`.

## CN-03: ratsnest (`RN_NET`, `CN_EDGE`, Filter-Kruskal)

| C++ | Rust | State |
|---|---|---|
| `CN_EDGE` (`connectivity_algo.h`: ctors, `operator<`, `StableSortCompare`, nodes, `RemoveInvalidRefs`, weight, visible, `GetLength`) | `edge::CnEdge`, end points `edge::CnEdgeNode` | ported |
| `CN_PTR_CMP` + `RN_NET::m_nodes` multiset | `ratsnest::RnNet` (`BTreeMap` keyed `(x, y, insertion seq)` = libstdc++ upper-bound insertion) | ported |
| `RN_NET::Clear`, `AddCluster`, `UpdateNet`, `compute`, `kruskalMST`, `OptimizeRNEdges` (both lambdas), `RemoveInvalidRefs`, `GetEdges` (stable sort per call), `GetUnsortedEdges`, `GetTotalAirlineLength`, `GetNodeCount`, `IsDirty` | `RnNet` methods of the same names | ported |
| `RN_NET::TRIANGULATOR_STATE` (`Triangulate`, `areNodesColinear`) | `RnNet::triangulate`, `are_nodes_colinear` over `kicad_geom::thirdparty::delaunator` | ported |
| `RN_NET::NearestBicoloredPair` | `RnNet::nearest_bicolored_pair` | ported, not verified (GUI only, tier 3) |
| `KI_MST::FilterKruskal` (`libs/core/include/core/filter_kruskal.h`) | `ratsnest::filter_kruskal::filter_kruskal` (+ libstdc++ `std::partition` port `std_partition`) | ported (lives here because `KI_UNION_FIND` is in `algo`) |

API: nodes are `CnAnchorRef`s into the item list; every method touching anchors takes the
`CnList` (`update_net(ctx, list_mut)`, `optimize_rn_edges(ctx, list)`, `edges(list)`).
`add_cluster(list_mut, cluster_id, &items)` takes the members separately (the cluster and the
list both belong to the algorithm). Anchors created by `OptimizeRNEdges` are
`CnEdgeNode::Optimized(CnAnchor)`, owned by the edge.

Ported tests / verification:

- `tests/cn03_filter_kruskal.rs`: port of `qa/tests/common/test_filter_kruskal.cpp` (8 cases,
  same `mt19937` graphs; `std::shuffle` replaced by Fisher-Yates over the same generator, the
  case only needs some permutation).
- `tests/cn03_ratsnest_golden.rs`:
  - `cn03_ratsnest_matches_cpp`: the `rn_net` lines of the CN-01/02 pcb-dump goldens
    (name, node_count, nodes in multiset order, edges in `GetEdges()` order with source,
    target, weight, visible; board unconnected counts). 15 of 30 boards load completely
    (the others skip footprints in the parser), 117 fields, 0 mismatches; incl. issue2512
    (zone anchor optimisation) and issue10697 (20 unconnected edges).
  - `cn03_ratsnest_random_boards_match_cpp`: 7 synthetic boards (`tests/golden/rn_random/`,
    generator `tests/oracle/gen_rn_random.py`, goldens `gen_rn_random.sh` = reference
    pcb-dump `rn_net` lines): 1500-via net (> `KRUSKAL_THRESHOLD` candidates, recursion and
    filter path), exactly collinear and near-collinear sets on large coordinates, coincident
    F.Cu/B.Cu track ends of different clusters (20 weight-1 chain edges, R-2), single-position
    nets with and without common layers, 1- and 2-node nets, filled zones of two nets with
    vias inside (zone-to-zone `ClosestSegmentsFast`) and lone items outside (zone vertex
    anchor). 2177 unconnected edges in total, 141 fields, 0 mismatches.
  - `cn03_ratsnest_spans_clusters`: port of `checkRatsnestSpansClusters`
    (`qa/tests/pcbnew/test_connectivity_clusters.cpp`) on the C++ test's boards (none loads
    yet: footprints) plus the golden boards: 15 boards, 18 nets.
- The ratsnest build in the tests reproduces `internalRecalculateRatsnest` + `updateRatsnest`
  (sequential); the real facade is CN-04's.

### Deviations (CN-03)

- R-2: coincident-anchor chains are `std::sort`ed (libstdc++ port) by `CnClusterId` (cluster
  creation order) instead of `CN_CLUSTER*` heap address. The synthetic coincident board
  matches the reference build after loading (measured by REV-CN: 0 of its 41 multi-cluster
  chains have an address order other than the creation order). In a DRC run of the reference
  (connectivity rebuilt by `DRC_CACHE_GENERATOR`) 17 to 32 of the 41 chains come out in
  another order, differently from run to run: there the C++ result is not reproducible
  (review-connectivity.md P7, F-1).
- `kruskalMST` computes each candidate's `orderKey` once instead of on every comparison
  (positions and tags do not change during the MST: identical results).
- Delaunator failure (`std::runtime_error` in C++, swallowed by the thread pool's `wait()` in
  `updateRatsnest`): `update_net` returns the error and leaves the net as the exception does
  (edges, tags, dirty flag unchanged). Not exercised by any oracle case.
- `m_nodes` keys cache the anchor position at insertion; C++ `CONNECTIVITY_DATA::Move`
  (GUI, CN-06) moves anchors inside the multiset (same delta for all, so the order is kept).

### Known gaps (CN-03)

- `NearestBicoloredPair` and `RemoveInvalidRefs` are not verified against C++ (GUI /
  incremental paths, CN-04/CN-06).
- Edge visibility comes from `BOARD::UpdateRatsnestExclusions` (CN-04); every compared board
  has all edges visible.
- Boards with footprints are not compared until the footprint parser lands; rerun
  `cargo test -p kicad-connectivity --test cn03_ratsnest_golden -- --nocapture` then.

## CN-04: `CONNECTIVITY_DATA` facade and queries

| C++ | Rust | State |
|---|---|---|
| `CONNECTIVITY_DATA::Build( BOARD* )` | `data::ConnectivityData::build( ctx )` (no triangulation, nets recorded), `build_board( &mut Board )` (+ fill triangulation, net changes applied, stamp re-recorded) | ported |
| `BOARD::BuildConnectivity`, `BOARD::UpdateRatsnestExclusions` (`board.cpp`) | `build_connectivity`, `update_ratsnest_exclusions` | ported |
| `BOARD::CacheTriangulation` (fills) | `data::cache_board_triangulation` | ported (sequential; outline triangulation left to the DRC cache generator, connectivity never reads it) |
| `Add` / `Remove` / `Update`, `PropagateNets`, `FillIsolatedIslandsMap`, `RecalculateRatsnest`, `internalRecalculateRatsnest`, `updateRatsnest`, `addRatsnestCluster` | same names | ported (sequential) |
| `GetNetCount`, `GetRatsnestForNet`, `ClearRatsnest`, `GetUnconnectedCount`, `RunOnUnconnectedEdges`, `GetNodeCount`, `GetPadCount` | `net_count`, `ratsnest_for_net(_mut)` / `net_edges`, `clear_ratsnest`, `unconnected_count`, `run_on_unconnected_edges`, `node_count_of`, `pad_count_of` | ported |
| `IsConnectedOnLayer` (type filter, footprint / pad-pad / ItemsTouchOnLayer skips, pad and via zone-island tests) | `is_connected_on_layer` + `impl ConnectivityQuery` | ported |
| `GetConnectedItems` (`IGNORE_NETS`, `EXCLUDE_ZONES`), `GetNetItems`, `GetConnectedTracks`, `GetConnectedPads` (both), `GetConnectedPadsAndVias`, `GetZoneIslandConnections`, `GetConnectedItemsAtAnchor` | `connected_items`, `net_items`, `connected_tracks`, `connected_pads(_into)`, `connected_pads_and_vias`, `zone_island_connections`, `connected_items_at_anchor` | ported |
| `TestTrackEndpointDangling`, `getMinDist` | `test_track_endpoint_dangling` (zone R-tree cache through the `CopperZoneRtrees` trait), `min_dist` | ported |
| `MarkItemNetAsDirty`, `RemoveInvalidRefs`, `RefreshNetcodeMap`, `HasNetNameForNetCode`, `GetNetNameForNetCode`, `GetRatsnestForItems` / `ForPad` / `ForComponent` | same names | ported |
| `Build( global, localItems )`, `ComputeLocalRatsnest`, `ClearLocalRatsnest`, `HideLocalRatsnest`, `GetLocalRatsnest`, `BlockRatsnestItems`, `Move`, `SetProgressReporter`, `GetNetSettings`, `GetLock` | - | skipped: tier 3 (CN-06) / GUI only |
| `GetFromToCache` | - | CN-05 (`from_to`) |

Client API (autorouter, DRC unconnected test): `unconnected_edges()` returns per net, in
`GetEdges()` order, the point pairs with the items they join (`UnconnectedEdge`), edges with a
missing or dirty node left out.

Verification (`tests/cn04_data_golden.rs`, goldens `tests/oracle/gen_cn04.sh`, which only runs
the existing pcb-dump):

- `cn04_golden_boards_match_cpp`: the connectivity goldens of `gen_goldens.sh` through the
  facade (`BuildConnectivity`): `rn_net` (nodes, edges in `GetEdges()` order, unconnected
  counts incl. visible), `cn_dangling` (both `aIgnoreTracksInPads` values, `aPos`), and the
  `flash.<layer>` lines of vias / pads (`tests/golden/cn04_flash/`, `FlashLayer` with this
  connectivity = `IsConnectedOnLayer`). 15 of 30 boards load (the others: footprints skipped
  by the parser), 931 fields, 0 mismatches (incl. issue10697: blind vias with
  `remove_unused_layers` stacked on through vias).
- `cn04_synthetic_boards_match_cpp`: 4 four-layer boards of `gen_cn04.py` (conditionally
  flashed through / blind / buried vias touching tracks, arcs, other vias on some layers only,
  other-net and near-miss tracks, net-less items, a via inside a zone; dangling: free ends,
  ends on vias, short tracks inside one via with and without a second item, chains, tracks in
  a zone fill, vias on one / two layers, net-less via, arcs, zero-length tracks; two random
  boards with 250 items each): 3052 fields, 0 mismatches.
- `issue25621::*`: port of `qa/tests/pcbnew/test_issue25621_via_flashing_contact_layer.cpp`
  (2 cases, real build: the pad case builds a FRONT_INNER_BACK PTH pad through the model API).
- `cn04_queries_are_consistent`: self-consistency of the queries without a dump line
  (`GetConnectedItems`, `GetConnectedTracks`, `GetConnectedPadsAndVias`, `GetNetItems`,
  `GetZoneIslandConnections`, `GetRatsnestForItems` / `ForPad`, `RunOnUnconnectedEdges`,
  `GetNodeCount`) on the synthetic boards.
- `cn04_corpus_build` (`--ignored`): build + dangling test over every corpus board the parser
  loads (qa/data/pcbnew + demos): 375 files, 372 built, 3 not parsed, 0 panics, 9.5 s in total
  (vme-wren 3.3 s, jetson 1.3 s, VCU118 1.2 s; test profile). Footprints are still skipped by
  the parser, so these boards have no pads.

### Pointer-order replacements (CN-04)

Reviewed by REV-CN: the complete site table with what the reference build does at each site
is in `docs/design/review-connectivity.md` (P13 .. P20 are the sites of
`connectivity_data.cpp`). In short: `GetNetItems`, `GetConnectedTracks`, `GetConnectedPads`,
`GetZoneIslandConnections` return `ItemId` order, `GetConnectedPadsAndVias` and
`GetConnectedItemsAtAnchor` list order; the members are verified against the reference
(`tests/rev_order_golden.rs`); the reference's own order is heap order, which the measurements
show to be neither the port's order nor stable. `IsConnectedOnLayer` and
`TestTrackEndpointDangling` do not depend on the order.

### Deviations (CN-04)

- `GetUnconnectedCount` counts `GetUnsortedEdges()` (C++ sorts first; the count is the same).
- The `ConnectivityQuery::pad_count` hook (no context) compares the pad items' cached net (pads
  never change net, so it equals `GetNetCode()` after a build); `pad_count_of( ctx, net )`
  reads the board like C++.
- `GetNetNameForNetCode` returns `None` for an unknown code (C++ `std::map::at` throws).
- `IsConnectedOnLayer` with a zone island: a missing fill layer is skipped (C++ dereferences
  a null `GetFill()`), and an empty pad hull contains no point (C++ `Outline( 0 )` of an empty
  set is undefined).
- `TestTrackEndpointDangling` reads item shapes with `ctx.connectivity = self` (C++ items reach
  this object through `GetBoard()->GetConnectivity()`); a stale connected parent is skipped.
- Build with a spin lock that cannot be contended: always returns true.

### Known gaps (CN-04)

- Pads and footprint boards are not compared until the pad parser (PCBIO-07) lands; the
  goldens are in place and are compared automatically then (`cn_item` pads, the pad `flash`
  lines of `tests/golden/cn04_flash/`, the pad queries of `tests/golden/order/`).
- No compared board has an excluded ratsnest marker, so `UpdateRatsnestExclusions` hiding an
  edge is not verified against C++ (it needs the project loader to create the markers; the
  code was compared by reading, review-connectivity.md F-9). The zone R-tree path of
  `TestTrackEndpointDangling` is implemented in `kicad-drc` (`CopperZoneRtrees`); in the
  reference both paths give the same answers on the boards measured (review-connectivity.md 2.3).

## REV-CN: review of CN-01 .. CN-04

Findings, the pointer-order site table and the measurements of the reference build:
`docs/design/review-connectivity.md`. Follow-up package: `docs/packages/FIX-CN-1.md`.

Changes made by the review:

| What | Where | Why |
|---|---|---|
| A pair of dirty items is tested with both operand orders of `ItemsTouchOnLayer` | `algo::cn_visit`, `algo::items_touch_from_both_visitors` | F-2: `SHAPE::Collide` is not symmetric and C++ tests such a pair from both visitors |
| Golden tests accept the parser's skipped `pad` constructs (pads dropped from the comparison) | `tests/cn0[1-4]_*.rs` | F-3: the footprint parser landed before the pad parser; 6 tests failed on the tree as found |
| Two self-consistency assertions restricted to what C++ guarantees | `tests/cn03_ratsnest_golden.rs` (`cn03_ratsnest_spans_clusters`), `tests/cn04_data_golden.rs` (`cn04_queries_are_consistent`) | F-4: the reference build violates them on boards with pad-less zone clusters / shorted zones |

New oracle and coverage:

- `tests/oracle/cn_order_oracle.cpp` + `build_order_oracle.sh` (binary
  `/home/user/kicad-oracle/bin/cn-order-oracle`): links the reference pcbnew; per board and per
  phase (after load, after a second `Build`, after a real `DRC_ENGINE::RunTests`) it measures
  the heap order at every pointer-ordered site (CN_ITEM and BOARD_ITEM addresses against list /
  board order, `m_connected` as stored, cluster addresses, the chains of `rn.cpp:253`), prints
  hashes of the ratsnest and of `TestTrackEndpointDangling`, the DRC violations that follow the
  connected order, and with `--queries` every query of `CONNECTIVITY_DATA` that has no pcb-dump
  line (pointer-ordered results in the port's order, so that the goldens do not depend on the
  heap).
- Four synthetic boards in `tests/oracle/gen_cn04.py` (`tests/golden/cn04/rev_*.kicad_pcb`):
  `rev_zone_contacts` (zone / item contact by anchor and by shape incl. exact-touch and 1 nm
  cases, conditionally flashed, blind and buried vias, arcs; the deferred via-net rule in all
  its branches incl. free vias; zone / zone contact incl. shared corner, shared edge, crossing
  bars, other net, other layer; several fill outlines per layer and a two-layer zone; a net-less
  zone; a teardrop zone, whose contact tests use the outline), `rev_ratsnest_zones`
  (`OptimizeRNEdges`: both lambdas, more than three candidate pairs, zone-only nets),
  `rev_shapes` (copper graphics of every kind as items, net propagation of pad-less clusters,
  free vias, the asymmetric track pairs of F-2), `rev_order_probe` (results that follow
  `m_connected`). Their full pcb-dump `connectivity` sections (`tests/golden/rev_*.txt`) are
  replayed by the CN-01 .. CN-04 tests.
- `tests/rev_order_golden.rs` replays `tests/golden/order/*.txt` (cn-order-oracle
  `--queries`, load phase): `GetConnectedTracks`, `GetConnectedPads`,
  `GetConnectedPadsAndVias`, `GetConnectedItems` (3 flag sets), `GetNetItems`,
  `GetZoneIslandConnections`, `IsConnectedOnLayer` per copper layer without a type filter
  (the zone island branches) and with the `FlashLayer` types, the ratsnest with the items of
  the edge ends, `FillIsolatedIslandsMap( map, true )` (the DRC path) in push order. 22 boards
  (6 synthetic, 16 corpus boards with pads); 7 load today (841 lines, 0 mismatches), the
  others wait for the pad parser.

State after the review (`cargo test -p kicad-connectivity`): cn01 23 of 23 boards, 2578 fields;
cn02 34 of 34 boards (19 fully), 2711 fields; cn03 19 boards, 185 fields + 7 random boards,
141 fields; cn04 15 golden boards, 931 fields + 8 synthetic boards, 3538 fields; rev order 7
boards, 841 lines; 0 mismatches everywhere. `cn04_corpus_build`: 375 files, 374 built, 0 panics.

### Deviations (REV-CN)

- Every result that follows the order of `CN_ITEM::m_connected`, of `std::set` / `std::sort`
  of item pointers or of `CN_CLUSTER` addresses is heap dependent in the reference build and
  cannot be reproduced: the port uses list / `ItemId` / cluster creation order. Outputs this
  reaches (review-connectivity.md, F-1): the item and position of a ratsnest edge end moved by
  `OptimizeRNEdges` when candidates tie, the edges between coincident anchors of different
  clusters, and in the consumers the "first connected item" choices of the DRC connectivity
  provider, the first pad of a via in the length calculation and the creation order of
  teardrops.

### Known gaps (REV-CN)

- Pads (PCBIO-07): contact rules of pads, `CN_LIST::Add( PAD* )` anchors, the origin pad
  ranking and conflicting clusters, `HasSingleConnection` (a zone island is only not orphaned
  with a pad), jumper pads, the pad branches of `IsConnectedOnLayer`: verified by unit tests
  only; goldens waiting.
- Ratsnest exclusions (F-9), `NearestBicoloredPair`, `CN_ANCHOR::IsDangling` (tier 3).

## FIX-CN-1: ratsnest ends that follow the heap in the reference (tie flags, comparison rule)

Origin: review-connectivity.md F-1 (sites P7, P8, P9). No behaviour change: every edge, end
and order is as before (all goldens unchanged); the port only *records* where the reference
result depends on heap order.

| C++ | Rust | State |
|---|---|---|
| `TRIANGULATOR_STATE::Triangulate` chain sort by `CN_CLUSTER*` (`ratsnest_data.cpp:253`) | `ratsnest.rs` `RnNet::triangulate`: every weight-1 chain edge carries `CnEdgeTie { coincident_clusters, unordered, candidates = the whole chain }` | ported + flag |
| `optimizeZoneAnchor` (`:351`) | `optimize_zone_anchor` also returns one anchor per zone layer whose closest vertex is at the winning distance (its first such vertex); more than one: `zone_anchor`, candidates of the moved end (`record_zone_anchor_tie`) | ported + flag |
| `optimizeZoneToZoneAnchors` (`:406-487`) | `optimize_zone_to_zone_anchors` also returns every result another input order could give (`zone_to_zone_outcomes`: groups of equal centre distance wholly inside the first 3 can come out in any order, the group straddling position 3 can contribute any calling member or, when enough of its members make no call, fall back to the groups before it; "no call" keeps the original ends); more than one: `zone_to_zone`, candidates of both ends (`record_zone_to_zone_tie`) | ported + flag |
| - | `edge.rs` `CnEdgeTie`, `CnEdge::tie()` / `is_tied()` / `tie_mut()`; `data.rs` `UnconnectedEdge::tie` (`UnconnectedEdgeTie`: candidates resolved to `( ItemId, Vector2I )`), `UnconnectedEdge::accepts`, `source_item_candidates`, `target_item_candidates` | new (additions only) |

### Comparison rule (for DRC-12 and the differential campaign)

An unconnected-items violation is `( source item, target item )` of one ratsnest edge, marker at
the source position (`drc_test_provider_connectivity.cpp`, `RunOnUnconnectedEdges`). For the
reference violations of a board:

1. Edges without a tie (`UnconnectedEdge::tie == None`, or `CnEdge::tie() == None` inside the
   DRC provider) compare exactly: net, both items, item order, marker position.
2. A flagged edge matches a reference violation of the same net when every end with an empty
   candidate list is exactly the edge's own item / position, and every other end is a member
   of its candidate list (items; positions, for the marker, from the same list). With
   `unordered` (chain ties) the two items may also be swapped. `UnconnectedEdge::accepts( a, b )`
   implements this for the items.
3. Flagged edges change the `GetEdges()` order (it sorts by source position), so the
   violations of a board are matched as a one-to-one assignment (bipartite matching), not by
   position in the list; report the flagged matches in a separate "tie" list instead of
   counting them as matches or mismatches.

Candidates come from the port's own data: the DRC provider sees them on the `CnEdge` it gets
from `run_on_unconnected_edges` (`edge.tie()`, resolve with `CnEdgeNode::resolve( list )`,
item `parent( list )`), an external client on `ConnectivityData::unconnected_edges()`.

### Verification (FIX-CN-1)

- `tests/fix_cn1_ties.rs`: `rev_order_probe` has both nets flagged (N2 `zone_anchor`, one end
  with 2 candidates; N3 `zone_to_zone`, both ends with 3 candidates), `rev_ratsnest_zones` has
  none (its ties are inside one outline), the 41 coincident chains of `rn_coincident` give
  weight-1 `coincident_clusters` edges only.
- The DRC comparison against the reference: `tests/golden/fix_cn1_unconnected.txt`
  (`tests/oracle/gen_fix_cn1.sh`, `kicad-cli pcb drc --format json`, `unconnected_items`:
  `rev_order_probe` and `rev_ratsnest_zones` once, `rn_coincident` three runs, which differ in
  the reference). With the rule every board and run matches; without it (ties stripped)
  `rev_order_probe` and all three `rn_coincident` runs fail and `rev_ratsnest_zones` still
  passes; every untied edge is in the reference exactly.
- Unit tests of `zone_to_zone_outcomes` (`ratsnest.rs`).

### Evidence run (step 3)

`cn-order-oracle --loader --drc --queries`, one run, on the 365 boards of the DRC campaign
corpus (`crates/kicad-drc/tests/golden/drc_corpus.txt`; it holds none of the large demo
boards): the `rn_hash` and every `rn` line of the `load` and `drc` phases are identical on
**364 boards** (79 of them have ratsnest edges; `issue16182` and `pns_regressions/ultrasound`
have 6 / 13 multi-cluster coincident chains whose address order is the creation order in both
phases); `qa/data/pcbnew/plugins/fakeboard.kicad_pcb` gives no data (not a loadable board).
**No campaign board has a heap-dependent ratsnest** in this run. Positive controls:
`rev_order_probe` (rn_hash load `9b0d876e..`, drc `b0dc7998..`) and `rn_coincident` (14 of 41
chains reordered in the DRC phase) differ, `rev_ratsnest_zones` does not. Since the heap order
varies between runs (review 2.1), a single run cannot prove a board tie-free; the port's flags
are the per-board criterion (a board without flagged edges needs no tie handling).

### Deviations (FIX-CN-1)

- None in results. `CnEdge` carries the extra `tie` record (part of its `PartialEq`).

### Known gaps (FIX-CN-1)

- Second-order effects are not flagged: a chain tie that swaps the ends of an edge before
  `OptimizeRNEdges` (the optimisation then starts from the other end), and a weight-1 chain
  edge competing in Kruskal with a non-chain edge of the same key. For an edge that is
  already tied and then optimised without a tie, the optimised end's candidates are its one
  result.
- The rule is not applied in kicad-drc yet (change request to DRC-12 / the campaign).
