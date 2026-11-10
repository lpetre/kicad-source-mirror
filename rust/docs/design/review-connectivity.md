# Review of the connectivity port (REV-CN)

Scope: `rust/crates/kicad-connectivity` (packages CN-01 .. CN-04) read side by side with
`pcbnew/connectivity/connectivity_{items,algo,data}.{h,cpp}`, `connectivity_rtree.h`,
`pcbnew/ratsnest/ratsnest_data.{h,cpp}`, `libs/core/include/core/{union_find,filter_kruskal}.h`
and the consumers of the order-sensitive queries in `pcbnew/drc`, `pcbnew/teardrop`,
`pcbnew/length_delay_calculation`, `pcbnew/board.cpp`, `pcbnew/zone_filler.cpp`.

Judged by: can it change the saved board, a DRC report, a zone fill or a teardrop.

Result: 1 major finding open (F-1, inherent: the reference result follows the heap), 0
blockers; 3 minor findings fixed (F-2 .. F-4), 1 documentation finding (F-5), 5 minor findings
open (F-6 .. F-10), 4 notes (F-11 .. F-14).

## 1. Method

* Statement-by-statement reading of the output-relevant paths (section 4 lists what was
  checked and how each part is verified).
* A new C++ driver linked against the reference pcbnew,
  `crates/kicad-connectivity/tests/oracle/cn_order_oracle.cpp` (build:
  `build_order_oracle.sh`, binary `/home/user/kicad-oracle/bin/cn-order-oracle`). It measures
  the heap order at every pointer-ordered site in three phases: `load` (the connectivity the
  load path built), `rebuild` (`ClearRatsnest(); Build( board )` once more, what
  `DRC_CACHE_GENERATOR::Run` does) and `drc` (after a real `DRC_ENGINE::RunTests`); it prints
  the DRC violations whose items follow the connected order, hashes of the ratsnest and of
  `TestTrackEndpointDangling`, and with `--queries` every `CONNECTIVITY_DATA` query that has no
  pcb-dump line.
* Four new synthetic boards through the existing generator (`tests/oracle/gen_cn04.py`,
  `tests/golden/cn04/rev_*.kicad_pcb`) and their goldens from the reference pcb-dump and from
  cn-order-oracle; one new replay test (`tests/rev_order_golden.rs`), the boards added to the
  CN-01 .. CN-04 golden tests.

## 2. What the reference build does with heap order

### 2.1 Measurements

`cn_addr`: adjacent inversions of the `CN_ITEM` addresses along the item list. `m_connected`:
items whose stored `m_connected` (>= 2 entries) is not in list order. `tracks` / `pads`:
adjacent inversions of the `PCB_TRACK` / `PAD` addresses along `Tracks()` / footprint pads.
Loader path (`BOARD_LOADER::Load`, as kicad-cli), one run each.

| board | items | cn_addr load | m_connected load | cn_addr DRC | m_connected DRC | tracks | pads |
|---|---|---|---|---|---|---|---|
| demos/ecc83-pp | 93 | 3 | 8 of 67 | 20 | 34 of 67 | 1 of 59 | 0 of 33 |
| demos/pic_programmer (QA path) | 619 | 7 | 13 of 426 | 37 | 177 of 426 | 9 of 376 | 6 of 247 |
| demos/complex_hierarchy | 696 | 10 | 31 of 666 | 44 | 375 of 666 | 1 of 364 | 0 of 165 |
| demos/interf_u | 1217 | 8 | 22 of 922 | 36 | 386 of 922 | 2 of 815 | 0 of 379 |
| demos/StickHub | 1664 | 6 | 67 of 1598 | 50 | 756 of 1598 | 5 of 1380 | 0 of 278 |
| demos/kit-dev-coldfire | 4034 | 9 | 80 of 3452 | 38 | 834 of 3452 | 6 of 3188 | 1 of 825 |

Small boards are worse: `qa/data/pcbnew/issue10697` (33 items) has 14 inversions after
loading, `via_stacks` (5 items) 4.

Three runs of the same binary on the same nine boards gave three different outputs: the
positions of the inversions change from run to run (even those of the `PCB_TRACK` objects the
parser allocates), in every phase.

Across item kinds the board item addresses interleave (`pads~tracks`, `tracks~drawings` on five
of the six boards): roughly file order (footprints, graphics, tracks, zones), not kind order.

Ratsnest cluster addresses (`rn_random/rn_coincident`, 75 clusters, 41 coincident-anchor
chains that span clusters): after loading 0 inversions and 0 chains whose address order is not
the creation order; after the DRC rebuild 8 to 10 inversions and 17 to 32 such chains, with two
different ratsnests in three runs.

### 2.2 Conclusions

1. The C++ order at these sites is **not** the allocation order and **not** list / file
   order. After loading it is close to it (2 to 12 % of the `m_connected` vectors of the demo
   boards deviate); after the rebuild a DRC run performs, a quarter to more than a half deviate, because
   the items of the second build reuse the chunks the first build freed.
2. It is **not deterministic**: the same binary on the same file gives different address
   orders from run to run. Small groups of same-sized items (the zone layer items of a board
   with a few zones) do come out the same every time in a DRC run, typically reversed (2.4).
3. Therefore there is no "reference order" a port could reproduce. The port's replacements
   (list index, `ItemId`, cluster creation index) are one of the orders the C++ can produce,
   and the most likely one after loading. What matters is which outputs the order reaches
   (section 3) and that differential tests do not compare those blindly (section 6).
4. The statements "heap order approximates allocation order, which is file order"
   (board-model.md 3.1, connectivity-zones.md R-1 / R-2) and "the address order differs from run
   to run because items are connected from a thread pool" (PCB_DUMP_FORMAT.md, dump source) are
   to be read with these numbers (F-5).

### 2.3 What does not depend on it (measured)

On the six demo boards the ratsnest (every edge with the items of its ends) and
`TestTrackEndpointDangling` of every track (both values of `aIgnoreTracksInPads`) are identical
between the load phase and the DRC phase and between runs. The DRC phase answers the zone
contacts of `TestTrackEndpointDangling` from the copper zone R-trees of the DRC cache, the
load phase from the effective shapes: both C++ paths agree on these boards and on the
synthetic ones.

### 2.4 Where it does reach an output (measured)

`tests/golden/cn04/rev_order_probe.kicad_pcb`:

* net N2: a lone via and a via inside two zones (F.Cu, B.Cu) with the same outline.
  `optimizeZoneAnchor` moves the edge end to the nearest outline vertex; the two zones tie.
  Loaded: the end is on the F.Cu zone (list order). `kicad-cli pcb drc`: "Via [N2] ... / Zone
  [N2] on **B.Cu**", in every run and with both load paths: in the rebuild the two zone layer
  items have swapped addresses.
* net N3: two islands each on F.Cu and B.Cu with equal bounding boxes but different outlines.
  `optimizeZoneToZoneAnchors` has four candidate pairs at one centre distance, takes the first
  three and keeps the last. Loaded: ends at (20, 55) / (40, 55) mm on B.Cu island 0 and F.Cu
  island 1. DRC run: ends (and marker) at (20, 57) / (40, 57) on F.Cu island 0 and B.Cu
  island 1; the kicad-cli report lists "Zone [N3] on F.Cu" first, list order gives the B.Cu
  zone first. (The report prints the items with their own positions, so the moved marker
  position itself is not in the text / JSON report.)
* A board of 12 tracks whose two ends lie off-centre in two vias, with
  `track_not_centered_on_via` enabled (not kept as a golden): the DRC connectivity provider
  reports the first offending via in `m_connected` order. Reference DRC, three identical runs:
  the via later in the board for 9 tracks, the earlier one for 3.

## 3. Pointer-order sites

"Port" is the replacement the port uses. Verdict `none`: the order cannot reach a result.
`heap`: the C++ result follows the heap, the port cannot match it in general.

| # | Site (C++) | C++ order | Port | Reaches | Verdict | Evidence |
|---|---|---|---|---|---|---|
| P1 | `CN_ITEM::Connect`, `m_connected` (`connectivity_items.h:232`) | `CN_ITEM*` address | `CnItemId` = list index | everything that walks `ConnectedItems()`: P4, P8 .. P11, P16, P18, P19, P21 | see those | 2.1 |
| P2 | `CN_VISITOR::operator()` skip `aCandidate->Dirty() && aCandidate < m_item` (`connectivity_algo.cpp:1039`) | address, and the time the candidate's own search starts (`BBox()` clears the flag) | each dirty pair once, from the lower list index, with both operand orders | whether an asymmetric contact test connects | fixed (F-2); where C++ runs one visitor only its result is heap / timing dependent | `rev_shapes` golden (4 pairs), 4 reference runs |
| P3 | `deferredNetCodes` `std::sort` by `CN_ITEM*` (`:336`) | address | grouped by `CnItemId` | nothing: groups are independent (keep the net if any zone matches, else the minimum) | none | `rev_zone_contacts` golden (9 vias) |
| P4 | `SearchClusters` neighbour walk (`:449`) | `m_connected` order | list order | nothing: union-find roots are the minimum index, clusters are numbered by their first member, members added in list order | none | cn02 goldens |
| P5 | `updateJumperPads` `std::map<FOOTPRINT*, ..>` (`:1099`) | address | `FootprintId` | nothing: footprints independent, `Connect` idempotent | none | reading |
| P6 | `FillIsolatedIslandsMap` `std::map<ZONE*, ..>` (`:758`, `:807`), `zoneItems` `unordered_map` (lookup) | zone address | `BTreeMap<ZoneId>` (board order) | list position of the re-added zone items, hence the push order of `m_IsolatedOutlines` / `m_SingleConnectionOutlines` | none: the filler sorts the first (`zone_filler.cpp:1276`, `:1590`), DRC tests membership of the second (`drc_test_provider_zone_connections.cpp:160`) and emits one marker per entry of the first (`drc_test_provider_connectivity.cpp:284`: marker order only) | `cn_island` goldens, `island` lines (both paths) |
| P7 | `TRIANGULATOR_STATE::Triangulate` chain `std::sort` by `GetCluster().get()` (`ratsnest_data.cpp:253`) | `CN_CLUSTER` address | cluster creation index, `stdsort` | orientation and pairing of the weight-1 edges between coincident anchors of different clusters: `rn_net`, DRC unconnected items (which two items) | matches the reference after loading; **heap** in a DRC run (not reproducible, differs between runs) | 2.1 last paragraph; `rn_coincident` golden |
| P8 | `optimizeZoneAnchor` walk of `ConnectedItems()` with strict `<` (`ratsnest_data.cpp:351`) | address | list order | which zone layer owns the new edge end when two zone outlines have their nearest vertex at one distance: the item of a DRC unconnected-items violation | **heap** (F-1) | 2.4 |
| P9 | `optimizeZoneToZoneAnchors` candidate pairs in `ConnectedItems()` order, `std::sort` by centre distance, first 3, last wins (`:406-487`) | address (+ libstdc++ sort) | list order, `stdsort` | position and item of both edge ends when centre distances tie (zone layers with equal bounding boxes): DRC marker position and items | **heap** (F-1) | 2.4 |
| P10 | `CN_ANCHOR::IsDangling`, `ConnectedItemsCount` (`connectivity_items.cpp:368`, `:394`) | address | list order | nothing (counts) | none | `anchor_*` fields of the cn01 / cn02 goldens |
| P11 | `IsConnectedOnLayer` (`connectivity_data.cpp:440`) | list of `CN_ITEM`s of the item, then address | list order | nothing: returns true for the first qualifying item, no side effect | none | flash goldens; `icl` lines (no type filter, zone island branches of vias) |
| P12 | `GetConnectedItems` (`:582`) | cluster order (list order since the index-ordered cluster search) | same | DRC zone connections (spoke count), `BOARD::GetTrackLength` | none (no pointer) | `ci0/ci2/ci3` lines |
| P13 | `GetNetItems` `std::sort` + `unique` of pointers (`:631`) | board item address (interleaved across kinds, 2.1) | `ItemId` (pads, tracks, zones, graphics; board order within a kind) | in scope only `NETINFO_ITEM::GetBoundingBox` (a merge); API / GUI otherwise | none in scope; the C++ order was not the port's for 2 to 37 nets per demo board | `net` lines (members) |
| P14 | `GetConnectedTracks` `std::set<PCB_TRACK*>` (`:642`) | track address | `ItemId` (track arena = board order) | DRC zone connections (count), DRC track angle (one violation per failing neighbour: emission order of markers of one track), teardrops (anchor list of a zone) | none for results; marker / creation order only. C++ differs from the port for 1 to 79 results per demo board | `ct` lines (members) |
| P15 | `GetConnectedPads` `std::set<PAD*>` (`:664`, `:680`) | pad address | `ItemId` (pad arena = footprint / file order) | `length_delay_calculation.cpp:707` takes the **first** pad of a via as the coincident pad | heap when a via touches two pads (F-6, consumer) | `cp` lines (members; pads pending) |
| P16 | `GetConnectedPadsAndVias` (`:690`) | `m_connected` order | list order | teardrop creation order per track (`teardrop.cpp:477`); `:781` and `board_commit.cpp:384` only test membership | creation order only: teardrop UUIDs derive from the parents and the writer sorts zones; GUI flow, no CLI reference | `cpv` lines (members) |
| P17 | `GetZoneIslandConnections` `std::set<const BOARD_ITEM*>` (`:711`) | address | `BTreeSet<ItemId>` | `TEARDROP_MANAGER::areItemsInSameZone` (`count`) | none | `zic` lines |
| P18 | `GetConnectedItemsAtAnchor` (`:948`) | `m_connected` order | list order | `fab_test_points.cpp:55` (size), edit tool | none in scope | reading |
| P19 | `TestTrackEndpointDangling` (`:805`) | `m_connected` order | list order | nothing: early returns are per item or monotone in the counts; `aPos` is written only on true from the final counts | none | `cn_dangling` goldens, 2.3 |
| P20 | `GetRatsnestForItems` / `ForComponent` `std::set` (`:1025`, `:1106`) | address | `BTreeSet<ItemId>` | membership only | none | reading |
| P21 | DRC connectivity provider walks `ConnectedItems()` and stops at the first hit (`drc_test_provider_connectivity.cpp:138`, `:198`; `:227` is an any-test) | `m_connected` order | list order (kicad-drc) | the pad / via named in `track_on_post_machined_layer` and `track_not_centered_on_via` violations and the marker position | **heap** (F-7, consumer) | 2.4 |
| P22 | `BOARD::m_ZoneIsolatedIslandsMap` `std::map<ZONE*, ..>` walked by the DRC connectivity provider (`:265`) | zone address | `ZoneId` | emission order of isolated-copper markers | order only (consumer) | reading |
| P23 | `CN_CLUSTER::m_netRanks`, `m_itemMap` (`unordered_map`) | - | `HashMap` | lookups only | none | reading |
| P24 | `RN_NET::m_nodes` (`multiset`, `CN_PTR_CMP` compares positions) | position, then insertion order (libstdc++) | `BTreeMap<(x, y, seq)>` | node order, tags | no pointer involved; reproduced | `nodes` of the `rn_net` goldens |
| P25 | `SearchClusters` final `std::sort` by `OriginNet` (`:508`) | libstdc++ introsort, many ties | `stdsort::std_sort` on clusters in creation order | cluster order in a net: node insertion order, tags, board edges | no pointer involved; reproduced | 1500-via net of `rn_dense`, all cluster goldens |

Not connectivity sites, for the owners: `ZONE_FILLER` keeps `std::map<ZONE*, ..>` /
`std::set<std::pair<ZONE*, layer>>` (connectivity-zones.md 4.0: no effect on fills).

## 4. The algorithm, part by part

| Part | Checked | Verified against the reference by |
|---|---|---|
| Item list: anchors, layers as copper ordinals, `canChangeNet`, list order of `Build` (zone layers, tracks, pads, copper shapes), bulk load | reading | cn01 goldens (23 boards, 2578 fields); pads: unit tests only |
| Contact rules, zone / item: `IsOnLayer`, conditional flashing + `ZLO_FORCE_NO_ZONE_CONNECTION`, backdrill, anchors first, then `ALWAYS_FLASHED` shape for pads / vias; teardrop zones use the outline | reading | `rev_zone_contacts` (anchor inside, body only, cap touching exactly: no contact, 1 nm short, other layer, other net, ring overlap, ring touching exactly: no contact, conditionally flashed, blind, buried, arc; teardrop zone by anchor and by shape) |
| Contact rules, zone / zone: same layer, bounding boxes, a vertex of one inside the triangles of the other, both directions | reading | `rev_zone_contacts`: overlap, shared corner (contact), shared edge (contact), crossing bars without a vertex inside (no contact, F-14), other net, other layer |
| Contact rules, item / item: common enabled layers in `LSET` order, `flashingFor`, clearance 0 | reading; F-2 found and fixed | cn04 flash boards, random boards, `rev_shapes` |
| Deferred via-net rule | reading | `rev_zone_contacts`: net-less via (lowest zone net), matching net kept, other net replaced, via with a track untouched, free via, blind via, net-less via with a net-less track, via whose net beats its track's in the cluster |
| `SearchClusters` (modes, `aExcludeZones`, `aSingleNet`), final sort | reading | cluster goldens, `ci*` lines |
| `CN_CLUSTER::Add` ranking, `PropagateNets` | reading | pad-less clusters: `rev_shapes` (track before graphic, graphic as the only netted item, first of two track nets, free via, two graphics); pads: unit test `cluster_origin_net_ranking` only |
| `updateJumperPads` (incl. the self-connection of `i == j`) | reading | not verified (pads) |
| Ratsnest: node multiset, 1 / 2 node nets (the single edge has weight 0, F-12), single position, collinear, delaunator half-edge walk, chains, board edges, Kruskal key and emission order, `GetEdges` stable sort | reading | cn03 / cn04 goldens, `rn_random`, `rev_*` |
| `OptimizeRNEdges` | reading | `rev_ratsnest_zones` (both lambdas, vertex ties inside one outline, more than 3 pairs), `issue2512`, `rn_zones`; ties between zone layers: F-1 |
| `IsConnectedOnLayer` | reading | vias: flash goldens and `icl` lines without type filter (zone island branch); pads: goldens waiting |
| `TestTrackEndpointDangling`, `getMinDist` | reading; order-independent | `cn_dangling` goldens; R-tree path lives in kicad-drc |
| Island map, both paths (`aConnectivityAlreadyRebuilt` false / true) | reading | `cn_island` (re-add path), `island` lines (DRC path, push order); several outlines per layer, two-layer zone, net-less zone. `HasSingleConnection` needs a pad: goldens waiting |
| `UpdateRatsnestExclusions`, visible flag | reading: identical | not verified (F-9) |

Numeric rules: `KiROUND( width / 2.0 )`, the `int` truncation of `getMinDist`, `int64`
squares, `unsigned` weights, `EuclideanNorm` through kicad-geom are as in C++. No
`sqrt` / `hypot` outside kicad-geom. `unwrap_or` / `map_or`: only where C++ would dereference a
stale pointer or index out of range (`IsNetDirty` beyond the table, stale parents); none can
change a result on valid input. One open point: F-8.

Lazy C++ state made explicit: `CN_ITEM::BBox()` (bounding box taken when the item is added;
the dirty flag is not cleared by reading it, which is what makes P2 differ), the parent's net
code cached in the item and updated by the propagation (`take_net_changes`), the triangulation
of the fills (`cache_board_triangulation` before `build`), `RN_NET::GetEdges()` sorting on every
call (kept).

## 5. Findings

| Id | Severity | Where | C++ reference | What differs, for which input | Status |
|---|---|---|---|---|---|
| F-1 | major | `ratsnest.rs` `optimize_zone_anchor` (candidate walk), `optimize_zone_to_zone_anchors` (pair order), `triangulate` (chain sort); kicad-drc unconnected-items test downstream | `ratsnest_data.cpp:351`, `:406-487`, `:253` | The item (and for zone-to-zone the position) of a ratsnest edge end follows `m_connected` heap order when candidates tie: two zones whose nearest outline vertex is the same point, zone islands with equal bounding boxes; edges between coincident anchors of different clusters follow cluster addresses. `kicad-cli pcb drc` on `rev_order_probe` reports "Zone [N2] on B.Cu" and, for N3, the F.Cu zone first with the marker at (20, 57) mm; the port (and the reference after loading) gives the zone on F.Cu, and the B.Cu zone first at (20, 55) mm. Ties need exact coincidence; none occurs on the six demo boards (2.3). | open; not fixable by a deterministic rule (2.2). `FIX-CN-1` proposes how the DRC comparison should treat these cases |
| F-2 | minor | `algo.rs` `cn_visit` | `connectivity_algo.cpp:1039`, `shape_collisions.cpp:733`, `shape_segment.h:84` | `SHAPE_SEGMENT` / `SHAPE_SEGMENT` collision uses `( wA + 1 ) / 2 + wB / 2`: two tracks with an odd and an even width (nm) whose axes are exactly `wOdd / 2 + wEven / 2` (integer halves) apart touch only with the odd one as first operand. C++ tests a dirty pair from both visitors; the port tested once, with the later item first, and left the pair unconnected when the odd-width track came first in the board. | fixed: both operand orders for a dirty pair; regression: `rev_shapes` (4 pairs) in the cn01 .. cn04 goldens |
| F-3 | minor | `tests/cn0[1-4]_*.rs` | - | 6 golden tests failed on the tree as found: the footprint parser landed, pads are still skipped (`pad`), the filters panicked. | fixed: `pad` treated like a skipped footprint |
| F-4 | minor | `tests/cn03_ratsnest_golden.rs`, `tests/cn04_data_golden.rs` | `qa/tests/pcbnew/test_connectivity_clusters.cpp` | Two self-consistency assertions (edges = clusters - 1 per net; an edge is found through the net of its end item) do not hold in the reference either for pad-less zone clusters (no anchors after a full build) and for edge ends moved onto a zone of another net (F-13). | fixed in the tests |
| F-5 | minor (docs) | `PORT_STATUS.md`, board-model.md 3.1, connectivity-zones.md R-1 / R-2, PCB_DUMP_FORMAT.md 8 | - | "Heap order approximates allocation = file order" and "list order is the best approximation of `m_connected`" are not what the reference does (2.1). | `PORT_STATUS.md` corrected; change requests for the other three documents |
| F-6 | minor | consumer of `connected_pads` | `length_delay_calculation.cpp:707` | The coincident pad of a via is the pad with the lowest address; with two pads on one via the choice (pad-to-die length, layer span) is heap dependent in C++, `ItemId` order in the port. | open (consumer; rare) |
| F-7 | minor | kicad-drc connectivity provider | `drc_test_provider_connectivity.cpp:138`, `:198` | "First connected item" choices follow `m_connected`: in the reference DRC run 9 of 12 probe tracks name the via that is later in the board (2.4). | open (REV-DRC); compare such violations by track and set of candidates |
| F-8 | minor | `items.rs` `CnAnchor::dist`, `edge.rs` `CnEdge::length`, `ratsnest.rs` `optimize_zone_anchor` | `connectivity_items.h:76`, `ratsnest_data.cpp:347`, `:365` | `VECTOR2I` subtraction of two anchors: for anchors more than 2.147 m apart C++ overflows silently, the port panics in builds with overflow checks. Likewise `getMinDist` truncation beyond `INT_MAX`. | open; boards of that size are outside what KiCad supports |
| F-9 | minor | `data.rs` `update_ratsnest_exclusions` | `board.cpp:538` | Not exercised by any oracle (needs the project loader to create excluded markers). Code identical by reading. | open |
| F-10 | minor | pads everywhere | `connectivity_items.cpp:139`, `:471`, `connectivity_algo.cpp:1096`, `connectivity_data.cpp:465`, `:494` | No pad path is verified against the reference yet (pad parser PCBIO-07 pending). | open; goldens waiting (`tests/golden/order/` for 16 corpus boards, pad `flash` lines, `cn_item` pads) |
| F-11 | note | `algo.rs` `item_entry` | `connectivity_algo.h` `ItemEntry` | C++ `m_itemMap[ item ]` inserts an empty entry on lookup, so `ItemExists` turns true for an item that was only queried; the port does not insert. No consumer in scope. | - |
| F-12 | note | `ratsnest.rs` `compute` | `ratsnest_data.cpp:281-296` | The single edge of a 2-node net has weight 0 (default constructor), unlike every other unconnected edge. Reproduced. | - |
| F-13 | note | `ratsnest.rs` `optimize_zone_to_zone_anchors` | `ratsnest_data.cpp:406` | The zone-to-zone optimisation does not look at nets: an edge end can land on a zone of another net that touches a via of the net (`rev_zone_contacts`, net 2). Reproduced; DRC then names that zone. | - |
| F-14 | note | `algo.rs` `check_zone_zone_connection` | `connectivity_algo.cpp:946` | Two fills that cross without a vertex of one inside the other are not connected; fills sharing one corner or an edge are. Reproduced. | - |

## 6. For the differential campaigns

* Ratsnest / unconnected items: compare edge positions and items exactly, except where the
  board has (a) an edge end on a zone whose candidates tie, (b) coincident anchors of different
  clusters. cn-order-oracle shows both: run it with `--drc` and compare the `rn` lines of the
  `load` and `drc` phases; a board where they differ has a heap-dependent ratsnest.
* DRC violations of `track_not_centered_on_via` / `track_on_post_machined_layer`: the second
  item is the first hit in heap order.
* Marker order is heap order in several providers (P14, P22); compare as sets.
