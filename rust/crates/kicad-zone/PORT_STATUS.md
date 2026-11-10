# kicad-zone port status

Specification: `rust/docs/analysis/connectivity-zones.md` (4.8 - 4.14, 8, 11). The zone.cpp
helpers the filler uses (`BuildSmoothedPoly`, `GetInteractingZones`,
`TransformSmoothedOutlineToPolygon`, hashes, `UnFill`, ...) live in `kicad-pcb` (M18).

## ZF-01: zone fill driver (indexes, fill order, flashing passes, island removal, refill)

| C++ | Rust | State |
|---|---|---|
| `zone_filler.h` `ZONE_FILLER` state, caches | `filler.rs` `ZoneFiller` | ported (`m_refillResultCache` omitted, see deviations) |
| `ZONE_FILLER::ZONE_FILLER` | `ZoneFiller::new` | ported (no commit object) |
| `INDEXED_ITEM`, `ITEM_RTREE`, `queryIndex`, `buildItemIndexes` | `index.rs` | ported |
| `forEachBoardAndFootprintZone` | `index::board_and_footprint_zones` (`GetZoneList( true )`) | ported |
| `isZoneFillKeepout`, PAD/VIA/TRACK knockout keys | - | ZF-03 (used only by the knockouts) |
| `appendZoneOutlineWithoutArcs` | `refill.rs` | ported |
| `mayHoldOutOfBoardCopper`, `zoneKnockoutQueryBox`, `zoneKnockoutMayInteract` | `filler.rs` | ported |
| `Fill()` L590-760 (set-up, connectivity rebuild, worst clearance, board outline, caches) | `ZoneFiller::fill` | ported (pad / courtyard caches are lazy in the model; net-tie cache built) |
| `findHighestPriorityZone`, `isInPourKeepoutArea`, via / pad flashing pre-pass | `flashing_pre_pass`, `find_highest_priority_zone` | ported |
| `toFill`, `BuildHashValue`, isolated islands map, `UnFill` | `ZoneFiller::fill` | ported |
| `zone_fill_dependency`, `fill_item_dependency`, `run_fill_waves` (DAG) | `zone_fill_dependency`, `fill_dependencies`, `run_fill_dag` | ported, sequential Kahn order |
| teardrop `connectivity->Update`, `FillIsolatedIslandsMap`, `SetIsFilled` | `ZoneFiller::fill` | ported |
| initial island removal (L1248-1312) | `ZoneFiller::fill`, `remove_islands` | ported |
| `sameNetOverlapSeeds`, iterative refill loop (L1314-1659) | `same_net_overlap_seeds_of`, `refill.rs` `iterative_refill_loop` | ported |
| `refillZoneFromCache` (L4587-4820) | `refill.rs` | ported (result cache omitted) |
| out-of-board islands (L1661-1790) | `remove_out_of_board_islands` | ported |
| second flashing pass (L1795-1914) | `flashing_second_pass` | ported |
| `aCheck` branch (L1916-1972, GUI dialogs) | - | skipped: GUI only (`kicad-cli` passes `aCheck = false`) |
| `fillSingleZone` | `copper_fill.rs` | ported |
| `dropSubResolutionOutlines` | `copper_fill.rs` | ported |
| `fillCopperZone`, `fillNonCopperZone`, `postKnockoutMinWidthPrune` | `copper_fill.rs` | stub (`port_pending!`, package ZF-03) |

Ported C++ tests (`qa/tests/pcbnew/test_zone_filler.cpp`): `ZoneFillDependencyKnockoutMargin`
(dependency / order part as `zone_fill_dependency_knockout_margin_order`; the fill assertion
`zone_fill_dependency_knockout_margin_fills` is `#[ignore]`d until ZF-03). Driver tests on
synthetic boards with the test stand-in fill (`tests/zf01_driver.rs`: flashing passes,
unconnected pours, same-net overlap refill) and unit tests of island removal (the three modes,
stale flags R-10) and out-of-board removal (`filler.rs`).

Corpus harness: `rust/tools/zone-corpus/run.sh` caches the reference refill
(`kicad-cli pcb drc --refill-zones --save-board` on a copy) of every `qa/data` board with zones
under 500 KB in `rust/target/zone-corpus/`, then `tests/zone_fill_corpus.rs` refills the input
with the port and compares every zone layer vertex for vertex (identical / same area /
different, first differing vertex, island flags). Report: `target/zone-corpus/report.txt`.

### Deviations

- `m_refillResultCache` (refill output keyed by a hash of the knockouts) is not ported: a pure
  optimisation of a pure function (R-13), results are identical barring hash collisions.
- `ZONE::CalculateFilledArea()` calls are not made: `m_area` is GUI-only state (search panel)
  and the model has no derived-state writer for it (change request filed).
- Container orders: `isolatedIslandsMap`, `changedZoneLayers`, the seeds and the refill sets are
  keyed by `ZoneId` (arena order) instead of `ZONE*` (address order). connectivity-zones.md 4.0:
  no effect on fills (each wave reads a snapshot; island detection is per zone).
- The fill DAG runs sequentially in Kahn order (roots in `toFill` order, successors in discovery
  order); identical fills because fills only read dependencies' published fills (R-9).
- The flashing passes collect `(item, layer, override)` and apply them after the read pass
  (every via / pad only writes its own overrides; the reads never look at overrides).
- `DRC_ENGINE` creation when the board has none: the caller passes the engine
  (`ZoneFiller::fill(.., rules, ..)`).
- `m_debugZoneFiller` / `DebugZoneFiller` is always false; `ADVANCED_CFG` defaults hard-coded
  (`ExtraClearance = 0.0005` mm, `ZoneFillIterativeRefill = true`, settable with
  `set_iterative_refill`).
- Non-convergence of the iterative refill: the `wxLogWarning` text is collected in
  `ZoneFiller::warnings()`.
- `set_trivial_copper_fill` (doc-hidden test hook, not C++): fill = smoothed outline, fractured,
  pre-knockout cache = that fill, no min-width prune in the refill. Exists only so that the
  driver can be tested before ZF-03.

### Known gaps

- Every copper fill reaches `fillCopperZone` (ZF-03 pending): the corpus comparison reports a
  fill panic per board until then; fills, island flags and second-pass overrides are verified
  against the reference only after ZF-03.
- Teardrop regeneration before the fill (TD-01) is not part of the driver (ZF-06 calls it);
  the corpus harness skips boards that need it.
- `GetFilledPolysList( layer )` on a layer without a fill entry throws in C++; the port treats
  it as an empty fill (no output path reaches it on loaded boards).

### Corpus scoreboard (ZF-01, 2026-11-10)

141 `qa/data` boards with zones under 500 KB cached: 97 do not load yet (pad parser PCBIO-07
pending), 6 need the rule expression binding (DRC-21), 12 need teardrop regeneration (TD-01),
26 load (component-class sync step skipped by the harness: pending, no footprints to act on).
Of these, 7 have no fillable zone (rule areas only) and 19 reach `fillCopperZone` /
`fillNonCopperZone` (ZF-03 pending). Driver smoke run with the stand-in fill on those 19: all
driver passes run without error on every board. Zone layers compared vertex for vertex: 0
(waiting for ZF-03).

## ZF-03: copper fill core (knockouts, clearances, min width, thermal reliefs, hatching)

Supersedes the `stub (ZF-03)` rows above: `fillCopperZone`, `fillNonCopperZone` and
`postKnockoutMinWidthPrune` are ported.

| C++ (`pcbnew/zone_filler.cpp`) | Rust | State |
|---|---|---|
| `RESULTS`, `VERTEX_CONNECTOR` (L69-245), `connect_nearby_polys` (L2982-3026) | `connect.rs` | ported (on `kicad_geom` `VertexSet`) |
| `PAD_KNOCKOUT_KEY`, `VIA_KNOCKOUT_KEY`, `TRACK_KNOCKOUT_KEY`, `isZoneFillKeepout`, `appendZoneOutlineWithoutArcs` | `knockouts.rs` | ported (`HashSet` membership only) |
| `addKnockout` (pad / via and graphic overloads), `addHoleKnockout` | `knockouts.rs` | ported |
| `knockoutThermalReliefs`, `buildCopperItemClearances`, `buildDifferentNetZoneClearances`, `subtractHigherPriorityZones`, keepout holes of `fillCopperZone` | `knockouts.rs` | ported |
| `DRC_ENGINE::EvalZoneConnection` | `knockouts::eval_zone_connection` | same body over `RuleResolver::eval_rules` (the trait has no `EvalZoneConnection`) |
| `fillCopperZone`, `fillNonCopperZone`, `postKnockoutMinWidthPrune`, blob drop, `DUMP_POLYS_TO_COPPER_LAYER` | `copper_fill.rs` | ported (incl. the `DebugZoneFiller` stage dumps, `ZoneFiller::set_debug_zone_filler`) |
| `buildThermalSpokes`, `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` | `thermal.rs` | ported |
| `addCopperThievingPattern` | `thermal.rs` | stub (`port_pending!`, package ZF-07) |

Ported tests: `ZoneFillDependencyKnockoutMargin` fill assertion
(`zone_fill_dependency_knockout_margin_fills`, no longer ignored). The other cases of
`test_zone_filler.cpp` assert through DRC runs / QA board loading and are covered by the corpus
comparisons below instead.

Verification (2026-11-10, reference `kicad-cli pcb drc --refill-zones --save-board`):

- Main corpus (`tools/zone-corpus/run.sh`): 330 zone layers identical vertex for vertex, 0 same
  area, 6 different. The 6 are on `issue23339_zone_layer_rules` and `issue17967`: their custom
  rules (`A.hasNetclass(..)`, `A.NetClass == ..` with `(layer outer)`) need the rule expression
  binding (DRC-21); `EvalRules` answers the zone clearance there instead of the rule. Two
  thieving boards stop in `addCopperThievingPattern` (ZF-07). 13 boards are skipped for teardrop
  regeneration (TD-01; the harness now also detects teardrop zones written as
  `(attr\n (teardrop ...))`, e.g. `issue24649`, whose stale teardrop zone the reference deletes).
- Stage corpus (`tools/zone-corpus/stages.sh`, new): KiCad's own `DebugZoneFiller` mode on
  22-layer variants of the 2-layer corpus boards (every pipeline stage of the F.Cu fill lands on
  In1..In19): 1308 stage layers identical, 50 different, all on 4 multi-zone boards and only at
  stages that read another zone's fill (In5/In6 spoke test areas, In14 full fill, In18/In19).
  In debug mode the C++ task of a `(zone, In<k>)` item reads other zones' F.Cu fills without a
  DAG dependency on them, so those reference stages depend on thread timing; the same boards'
  real F.Cu fills are identical in the main corpus.

### Deviations

- `PCB_TEXT textCopy = *text` for a knockout field: the port clones the embedded `PcbText`, the
  same slicing as the C++ copy.
- `dim->PCB_TEXT::TransformShapeToPolygon`: called on the embedded `PcbText` of the dimension
  (every virtual that body reaches is the inherited `PCB_TEXT` one).
- `std::deque<SHAPE_LINE_CHAIN>` of spokes is a `Vec` (push back, last four rotated).
- Progress reporter / cancellation checks are not ported (no reporter in the port).
- Debug mode: `m_board->SetLayerName( b, c )` of `DUMP_POLYS_TO_COPPER_LAYER` is not reproduced.
- A context without a rule resolver evaluates every constraint as the null constraint (the
  C++ filler always has a DRC engine).

### Known gaps

- Copper thieving (`addCopperThievingPattern`, ZF-07) panics with `port_pending!`.
- Boards whose custom rule conditions need DRC-21 differ until it lands (see above).
- Hatched fills are verified on one corpus board (`hatch_thermal_connectivity`: final fill and
  all 21 stage layers identical); the thermal-ring / hatch-hole-drop paths with vias and the
  custom-pad spoke path (`customSpokes`) are not exercised by any loadable corpus board yet.
