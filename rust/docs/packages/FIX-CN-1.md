# FIX-CN-1: Ratsnest ends that follow the heap in the reference (decision + comparator)

Crate: kicad-connectivity (`src/ratsnest.rs`, `tests/`), and the DRC differential comparison
(whoever owns the unconnected-items comparison; kicad-drc `tests/` or the campaign scripts)

Origin: REV-CN finding F-1 (`rust/docs/design/review-connectivity.md`, sections 2 and 3, sites
P7, P8, P9). Read that document first; it holds the measurements.

## The problem

Three places of `RN_NET` take the first of several equally good candidates in an order that is
heap-address order in C++:

* `optimizeZoneAnchor` (`ratsnest_data.cpp:351`): the connected zone layer whose outline vertex
  is nearest; two zone layers tie when their outlines share that vertex.
* `optimizeZoneToZoneAnchors` (`:406-487`): the first three candidate pairs by centre distance,
  the last of them wins; pairs tie when zone islands have equal bounding boxes.
* the chain sort of `TRIANGULATOR_STATE::Triangulate` (`:253`): coincident anchors of different
  clusters, sorted by `CN_CLUSTER` address.

The port uses list order / cluster creation order. That is what the reference gives right
after loading (all goldens match). In a `kicad-cli pcb drc` run the connectivity is rebuilt
(`DRC_CACHE_GENERATOR::Run`) and the reference then answers differently:
`tests/golden/cn04/rev_order_probe.kicad_pcb` gives "Zone [N2] on B.Cu" where list order gives
the F.Cu zone, and the two zones of net N3 in the other order with other end points. For the
chain sort the reference is not even stable from run to run (`rn_random/rn_coincident`: 17 to
32 of 41 chains differ, two different ratsnests in three runs).

No deterministic rule reproduces the heap. The DRC report of the port can therefore differ from
the reference in the *second item* of an unconnected-items violation (and in the order of the
two items) on boards with such ties. None of the six demo boards measured has one.

## Work

1. Tie detection in the port (kicad-connectivity): let `RnNet::optimize_rn_edges` and
   `RnNet::triangulate` record, per ratsnest edge, whether its ends were chosen among tied
   candidates (a flag on `CnEdge`, exposed through `UnconnectedEdge`; for the chain case: the
   edge joins coincident anchors of different clusters, weight 1). No behaviour change.
   Unit-test it on `rev_order_probe` (both nets flagged) and on `rev_ratsnest_zones`
   (none flagged: its ties are inside one outline and resolved by vertex order).
2. Comparator (DRC differential tests / campaign): for a violation built from a flagged edge
   accept any of the tied candidates: compare the net and the non-tied end exactly, the tied
   end by membership. List the affected violations in the campaign report instead of counting
   them as mismatches.
3. Evidence run: `cn-order-oracle --loader --drc --queries` on the campaign boards; a board
   whose `rn` lines differ between the `load` and `drc` phases has a heap-dependent ratsnest
   and must be covered by step 2. Record the list in
   `crates/kicad-connectivity/PORT_STATUS.md`.

Not in scope: emulating the allocator (the measurements show small boards coming out reversed
in the rebuild and larger ones mixed; an emulation would only move the mismatches).

## Verification

`cargo test -p kicad-connectivity` (goldens unchanged: step 1 must not change any edge), the
new unit tests, and the DRC comparison of `rev_order_probe` against
`kicad-cli pcb drc --format json` passing with the tie rule and failing without it.
