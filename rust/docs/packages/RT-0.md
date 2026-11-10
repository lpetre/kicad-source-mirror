# RT-0: Analysis of the interactive router (push-and-shove kernel) for a port

Crate: none - this is an ANALYSIS package. You write one document,
rust/docs/analysis/router.md, and change no code. (The standard result fields still apply:
`decisions` = the scoping and design choices you propose, `known_gaps` = what you could not
establish.)

Background: the port covers board load / save, connectivity, DRC, zone fill and
fabrication output, all verified byte for byte against a reference kicad-cli. The last
tier of the plan (rust/docs/PLAN.md: "Tier 3 (stretch) = push-and-shove router") is the
interactive router kernel, pcbnew/router/ (PNS: about 45,000 lines in 91 files), so that a
client - a web app or an autorouter front end running the wasm build - can route a track
the way KiCad's interactive router does: start on an item, head for a point, walk around
or shove obstacles, drag, place vias, route differential pairs, tune lengths. KiCad has no
headless autorouter; the kernel is driven by ROUTER_TOOL from mouse events.

Read first: rust/PORTING.md (the fidelity rules every port package follows),
rust/docs/analysis/connectivity-zones.md and drc.md as models of what an analysis document
for this project looks like (execution-order pipelines, order-sensitivity tables, risk
lists, work-package tables with C++ line ranges), rust/docs/design/board-model.md
(sections 0-9: how the Rust board model works: arenas, handles, ctx, mutation through the
board), rust/crates/kicad-geom/PORT_STATUS.md (what of SHAPE_LINE_CHAIN, SHAPE_INDEX,
SHAPE_ARC, SHAPE_POLY_SET, collisions exists), rust/docs/design/review-connectivity.md
(what the reviews established about heap-order dependence in the reference).

The document must contain:
1. Inventory: every file of pcbnew/router/ with line count, role, and tier (kernel needed
   for basic single-track routing / walkaround / shove / drag / via placement / diff pair /
   meander and tuning / component dragging / topology and optimizer / debug / GUI glue that
   is not ported), plus what the kernel uses outside its directory (kimath geometry -
   list every SHAPE_LINE_CHAIN / SHAPE_* / SEG function it calls and whether kicad-geom has
   it; DRC engine queries through PNS_KICAD_IFACE / PNS::RULE_RESOLVER; board items;
   connectivity; settings: PNS::ROUTING_SETTINGS, PNS::SIZES_SETTINGS, meander settings -
   the board model keeps MEANDER_SETTINGS as JSON today).
2. Architecture as it executes: the world model (NODE, branching and revert, JOINT, ITEM
   kinds, LINE / SEGMENT / ARC / VIA / SOLID / HOLE, ITEM_SET ownership, the spatial
   INDEX), ROUTER and its modes, LINE_PLACER (head / tail, the routing step per cursor move,
   posture solver, fanout / follow-mouse logic), WALKAROUND, SHOVE (the line stack, hull
   generation, via pushing, the iteration limits), OPTIMIZER, DRAGGER, DIFF_PAIR(_PLACER),
   the MEANDER placers, TOPOLOGY, the sync with the board (PNS_KICAD_IFACE_BASE::SyncWorld,
   Commit: which board items are created / removed, in which order, with which UUIDs).
   For each: the call pipeline in execution order with the tolerances, iteration limits
   and time limits that decide results (PNS has wall-clock time limits in walkaround /
   shove / optimizer: list every one - a port must replace them by something deterministic;
   say what the reference's tests do about them).
3. Order sensitivity and reproducibility: every container keyed or sorted by pointer,
   every unordered_map / set whose iteration reaches a result, every std::sort with a
   comparator that is not a strict weak order, uninitialised reads; for each, whether the
   result is deterministic in the reference at all (the earlier reviews found several
   places in KiCad where it is not).
4. Oracles: the regression infrastructure that exists - qa/tools/pns (pns_log_file,
   pns_log_player, qa_pns_regressions_main, the debug decorator) and
   qa/data/pcbnew/pns_regressions (list the cases, what a log contains: the board, the
   recorded events, the expected committed result; how the player decides pass / fail; how
   deterministic a replay is across runs on this machine - RUN the reference tool if it is
   built or can be built in the oracle build tree /home/user/kicad-oracle/build under the
   machine-wide flock (rust/docs/ORACLE.md, PORTING.md section 7: C++ builds only under
   `flock /home/user/kicad-oracle/cxx-build.lock`, at most -j2; check the CMake options the
   tree was configured with: QA tools may be off - say what it would take, do not
   reconfigure the shared build tree; a separate small driver that links the built router
   objects is the pattern other packages used), and what unit tests exist under
   qa/tests/pcbnew for the router. Propose the differential method for a port: replay the
   same event log through the Rust kernel and compare the committed items (and
   intermediate head / tail geometry per event, if the C++ can dump it) bit for bit.
5. A headless API for the port (kicad-session / wasm): the minimal set of calls a client
   needs (start routing at a point / item with a net, layer and width; move; place via /
   switch layer; fix a segment; finish; cancel; drag a segment / via / corner; settings),
   mapped onto ROUTER's entry points, and how results reach the board (through the
   model's mutation API with connectivity updated incrementally: kicad-connectivity has the
   add / remove paths).
6. Work packages in dependency order (like section 10 of the other analysis documents):
   id, title, C++ files with line ranges, size, dependencies (on kicad-geom functions that
   must be added first - name them), tier, verification (which regression logs / tests).
   Mark a FIRST MILESTONE that is useful alone: single-track walkaround routing with via
   placement on a board with obstacles, committed to the board, verified by replayed logs.
7. Risks and open questions, with your recommendation for each.

Be concrete: C++ file and line references throughout, numbers measured rather than
estimated where you can run something. The document will be reviewed and then used to
hand out port packages, so everything a port agent needs to start must be in it.
