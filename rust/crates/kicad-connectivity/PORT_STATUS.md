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
| `PCB_SHAPE::GetConnectionPoints` (board-level shapes) | `items::pcb_shape_connection_points` | ported here temporarily (belongs to kicad-pcb, change request) |
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
  for the same reason.
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
  model stub, so **the zone contact paths (`checkZoneItemConnection`,
  `checkZoneZoneConnection`), the deferred via-net rule and the island map are not yet
  verified against C++**; pads (footprint parser) likewise. The test compares them
  automatically once those parser packages land.
- `tests/cn02_union_find.rs`: port of `qa/tests/common/test_union_find.cpp` (7 cases; the
  two threaded cases interleave the edges serially as 4 threads would).
- `cn02_clusters_are_connected_components` (same file as the golden test): port of the
  cluster half of `qa/tests/pcbnew/test_connectivity_clusters.cpp` on its boards (where they
  load) plus the golden boards; fills are read from the files (`FillZones` is ZF-*). The
  ratsnest half is CN-03's.

### Deviations (CN-02)

- Contact search: C++ skips a dirty candidate whose *address* is below the visiting item's
  (and `BBox()` clears the dirty flag of items already visited, so some pairs are tested from
  both sides); the port compares `CnItemId` (list order) and keeps the flags until the end of
  the search, so each dirty pair is tested once. Contact tests are symmetric and `Connect()`
  is order-normalising, so the graph is the same (R-1).
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

- See verification: zones, pads and the via-net rule await the parser packages.
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
  matches the reference build; glibc may reuse freed blocks so the address order is not
  guaranteed in general (connectivity-zones.md R-2).
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
