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

## ZF-06: headless FillAllZones, refill in the CLI jobs, save

| C++ | Rust | State |
|---|---|---|
| `ZONE_FILLER_TOOL::FillAllZones( nullptr, reporter, true )` (`tools/zone_filler_tool.cpp`) | `tool.rs` `fill_all_zones` | ported (headless path) |
| `PostFillRefresh( true )` / `rebuildConnectivity` (`BOARD::BuildConnectivity`) | `fill_all_zones` | ported |
| `board()->OnZonesFilled`, `RegenerateDirtyGenerators`, `refillAroundGenerators` | - | nothing to do headless: the jobs handler's tool manager has no `GENERATOR_TOOL`, so no generator is regenerated and nothing refilled; `PCB_VIA_STITCH::OnZoneFillChanged` only sets an unsaved dirty flag |
| `TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, true )` | `tool.rs` `update_teardrops` | partial: the legacy-teardrop early return and the no-op case (no teardrop zone, no pad / via with enabled teardrops, track target disabled) are ported; every other board stops in `port_pending!` (TD-01 / TD-02) |
| `JobExportDrc` refill / `--save-board` block, gerbers / SVG `--check-zones` | kicad-session `Session::refill_zones` / `fill_all_zones`, `jobs_handler::drc_save_board` | ported (see kicad-session PORT_STATUS) |

Tests: `tests/zf06_tool.rs` (`fill_all_zones` = the bare filler's fill; the teardrop guard and
its legacy early return); `kicad-session/tests/zf06_refill_save.rs` (end to end, below).

Verification (2026-11-10, reference `kicad-cli pcb drc --refill-zones --save-board` cached by
`tools/zone-corpus/run.sh`):

- Saved boards (`cargo test -p kicad-session --release --test zf06_refill_save -- --ignored
  --nocapture`, report `target/zone-corpus/save_report.txt`): load (kicad-cli sequence) +
  `Session::refill_zones` + `drc_save_board` vs the reference's saved board, byte for byte after
  replacing UUIDs that are not in the input (freshly generated on both sides): 141 boards,
  **125 identical**, 1 different, 15 not run. On 80 of the 125 the refill changes the fills
  relative to the input file (so the comparison exercises the fill), all identical. The one
  difference is `cli__artwork_generation_regressions__ZoneFill-4.0.7`: every footprint has
  `(tstamp 3)`, the duplicates get fresh random UUIDs and the writer sorts footprints by UUID
  (`BOARD_ITEM::ptr_cmp`), so the footprint order is random on both sides; its fills are
  identical. Not run: 13 need teardrop regeneration (TD-01 hook), 2 copper thieving (ZF-07).
- Zone-layer scoreboard (`tools/zone-corpus/run.sh`) after the chain: **336 identical, 0 same
  area, 0 different** (DRC-21 has landed: `issue23339_zone_layer_rules` now matches). Harness
  fixes: the DRC engine is initialised with the board's layer names
  (`board_layer_choices`, as `BOARD_LOADER::Load`), and zones whose UUID the saved file does not
  carry (teardrop areas: the writer omits it, so the reference reader generates a fresh one;
  `issue17967`, legacy teardrops) are matched by `Zones()` position - those were the 2
  "different" layers of `issue17967`, not a fill difference.

### Deviations

- `commit.Revert()` on a failed fill is not reproduced: `ZoneFiller::fill` cannot fail without
  a progress reporter (`debug_assert!`).
- The `m_fillInProgress` re-entrancy guard, the infobar about invalid DRC rules, the progress
  dialog and the focus handling are GUI-only and not ported.

### Known gaps

- Teardrop regeneration (TD-01 / TD-02): boards with teardrop zones or enabled teardrop
  parameters (not legacy) stop in `update_teardrops` with `port_pending!`.
- `kicad-rs pcb drc --refill-zones --save-board` end to end still stops at the DRC run
  (`run_drc` returns `NotYetPorted` until DRC-12 wires `RunTests` and the report): the refill
  and the save are verified through the session API instead. DRC-12 must call
  `jobs_handler::drc_save_board` after writing the report.

## TD-01: teardrops (`TEARDROP_MANAGER`, manager and outline geometry; TD-01 + TD-02 rows)

Supersedes the `partial` teardrop row and the teardrop known gap of ZF-06: `tool.rs`
`update_teardrops` now runs the full `UpdateTeardrops( commit, nullptr, nullptr, true )`.

| C++ (`pcbnew/teardrop/`) | Rust (`src/teardrop/`) | State |
|---|---|---|
| `TEARDROP_MANAGER` members, constructor, `TEARDROP_VARIANT` | `mod.rs` `TeardropManager`, `TeardropVariant` | ported |
| `teardropUuid`, `maskUuidFor`, `buildCrossingStub` | `mod.rs` | ported |
| `createTeardrop`, `createTeardropMask`, `createAndAddTeardropWithMask`, `tryCreateTrackTeardrop` | `mod.rs` | ported |
| `RemoveTeardrops`, `UpdateTeardrops`, `DeleteTrackToTrackTeardrops`, `setTeardropPriorities`, `AddTeardropsOnTracks` | `mod.rs` | ported |
| `BOARD::BulkRemoveStaleTeardrops` | `mod.rs` `bulk_remove_stale_teardrops` | ported (local helper) |
| `TRACK_BUFFER`, `GetWidth`, `IsRound`, `BuildTrackCaches`, `ensureCopperIndex`, `copperNetcode`, `pairClearance`, `zoneConnections`, `areItemsInSameZone` | `utils.rs` | ported |
| `collidesWithOtherNets`, `computeFittedTeardropPolygon`, `computeChordThroughShape`, `findTouchingTrack` | `utils.rs` | ported |
| `computeCurvedForRoundShape`, `computeCurvedForRectShape`, corner / oval-end helpers, `computeAnchorPoints`, `findAnchorPointsOnTrack`, `computeTeardropPolygon` | `utils.rs` | ported |
| `IsUniformlyRound` | - | skipped: GUI-only (teardrop dialog); needs a public `Pad` padstack accessor |
| `TEARDROP_PARAMETERS`, `TEARDROP_PARAMETERS_LIST`, `teardrop_types.h` | `kicad_pcb::teardrop_parameters` | ported earlier (board model) |

Ported C++ tests (`tests/td01_teardrops.rs`): `test_teardrop_uuid.cpp` (all 4),
`test_teardrop_shorts.cpp` `NoTeardropOnZoneConnectedThermalPad`, `test_zone_filler.cpp`
`CrossingTrackTeardropOnViaStaysOnTrack` / `CrossingTrackTeardropOnPadStaysOnTrack`. The board
cases of `test_zone_filler.cpp` (L1090-2790: roundrect, spike, custom pad anchor, arc anchor,
oval, large circle, off-centre symmetry, elongated pad, two-segment, close via) use the
`teardrop_*` / `*_teardrop` boards, whose teardrops are compared vertex for vertex with the
reference below (a stronger check than their geometric assertions); they are not ported
one by one.

Verification (2026-11-10, reference `kicad-cli pcb drc --refill-zones --save-board`, cached by
`tools/zone-corpus/run.sh`):

- `tests/td01_teardrops.rs` `teardrop_corpus` (`--ignored`; report
  `target/zone-corpus/td01_report.txt`): every teardrop zone after `UpdateTeardrops` (type,
  layer, net, priority, outline vertices) against the saved reference, in the writer's save
  order. 25 `qa/data` boards (all boards under 500 KB that regenerate teardrops, plus the two
  with track-to-track teardrops enabled in the project) in six parameter variants (`base`;
  `curved`: curved / straight flipped; `sized`: length / width ratios and limits changed, two
  segments and pads-in-zones flipped; `trackend`: track-to-track enabled; `mask`: tracks with
  solder mask (mask teardrops); `wide`: no width / length limit, so 30 candidates go through
  the clearance fit and its bisection) plus two synthetic crossing boards (crossing stubs):
  **283 / 283 teardrop zones identical**. Golden: `tests/golden/td01_teardrops.txt` (made with
  `TD01_WRITE_GOLDEN=1`; variant inputs with `TD01_WRITE_VARIANTS=1` into
  `target/td01-variants/`), replayed by the default test `teardrop_golden`.
- Zone corpus (`tools/zone-corpus/run.sh`, `needs_teardrops` filter removed; the harness runs
  `update_teardrops` before the fill and matches teardrop zones by save order): **656 zone
  layers identical, 0 same area, 0 different** (incl. the variant boards); 2 thieving boards
  stop in ZF-07.
- Saved boards (`kicad-session` `zf06_refill_save`): **233 identical**, 1 different (the known
  random-footprint-UUID case `ZoneFill-4.0.7`), 2 not run (ZF-07). Every teardrop board is now
  byte identical after UUID normalisation.

### Deviations

- No `BOARD_COMMIT`: zones are added to / removed from the board and connectivity directly
  (`aCommit.Added` / `Removed` only record in C++). `STRUCT_DELETED` is a local set.
- Address-ordered containers: `GetConnectedPadsAndVias` (creation order of the teardrops) uses
  the CN-01 list order (R-1), `dirtyTracks` / `std::set<PCB_TRACK*>` are `BTreeSet<ItemId>`. The
  creation order only matters where a later teardrop's clearance fit sees an earlier one
  (`m_createdTdRTree`); priorities (`setTeardropPriorities` sorts by layer / area / UUID) and the
  save order (UUID) do not depend on it. No corpus case differs.
- `m_createdTdRTree` (dynamic r-tree) is a list scanned with the same inclusive box test: only
  "does any created teardrop collide" is asked, so the visit order does not matter.
- `computeChordThroughShape` reproduces `( hit.p - extStart ).Dot( dir )`, which converts the
  `VECTOR2D` direction to `VECTOR2I` (clamp + truncation) before the integer dot product.
- `findAnchorPointsOnTrack` / `computeChordThroughShape` read `Outline( 0 )` unconditionally in
  C++; a pad without copper on the layer (no outline) returns "no teardrop" instead of UB.
- `wxCHECK2( radius != 0, radius = 1 )` applies the fix without a debug assertion.
- The stub `PCB_TRACK` and the teardrop zones draw a UUID from `Board::kiid_generator` like the
  C++ constructors (overwritten for the zones).

### Known gaps

- `IsUniformlyRound` (GUI only) is not ported.
- `RemoveTeardrops` / incremental `UpdateTeardrops` (dirty lists) and
  `DeleteTrackToTrackTeardrops` are ported but only reached by editor flows; they are not
  verified against the reference (no CLI path runs them).

## ZF-07: copper thieving (`addCopperThievingPattern`)

Supersedes the "ZF-07 stub" mentions in the ZF-01 / ZF-03 / ZF-06 / TD-01 sections above.

| C++ | Rust | State |
|---|---|---|
| `ZONE_FILLER::addCopperThievingPattern` (zone_filler.cpp L4158-4352: dots, squares, hatch; grid origin / stride / stagger, per-layer `hatching_offset` lookup, orientation rotate-in / rotate-out, containment deflate, hatch interior deflate and void clipping, bad-parameter bail-out) | `thermal.rs` `add_copper_thieving_pattern` | ported |
| callers in `fillCopperZone` / `fillNonCopperZone`, no-island-tracking for thieving zones in `Fill` (initial map and iterative refill) | `copper_fill.rs`, `filler.rs`, `refill.rs` | checked against the C++, unchanged |

Tests: `tests/zf07_thieving.rs` ports the seven `CopperThievingZone_*` cases of
`qa/tests/pcbnew/test_zone_filler.cpp` (HatchSurvivesTrackBisection, NonCopperLayerStampsNotSolid,
DotsGrid, StaggerProducesDifferentLayout, SquaresGrid, HighDensityPerformance without the time
bound, HatchPattern). `test_drc_copper_thieving.cpp` is ported in kicad-drc
(`tests/drc07_connectivity.rs`).

Verification (reference kicad-cli `pcb drc --refill-zones --save-board`, 2026-11-10):
- Corpus boards `zone_thieving_track_bisection` and `drc_copper_thieving/copper_thieving`: fills
  vertex-identical (`tests/zone_fill_corpus.rs`).
- 25 crafted boards from `tests/oracle/gen_zf07_thieving.py` (dots / squares / hatch; stagger;
  orientations 0, 17.5, 30, 45, 60, 90, 135; negative coordinates; element size below min
  thickness; zone-local and board-default `hatch_position` offsets; tracks and vias cutting the
  fill; silkscreen layer; 50 x 50 mm dense grid; narrow hatch zone; gap 0 / size 0 in the file):
  **25 / 25 zone layers identical**.
- Saved boards (`kicad-session` `zf06_refill_save --ignored`, whole cached corpus incl. the
  crafted boards): 262 identical, 1 different (the known `ZoneFill-4.0.7` UUID case), 0 not run;
  the two former "not run: ZF-07" boards and all 25 crafted boards are byte identical.
  Regenerate with
  `python3 -I crates/kicad-zone/tests/oracle/gen_zf07_thieving.py target/zf07-boards &&
  tools/zone-corpus/run.sh target/zf07-boards/*.kicad_pcb`.

### Deviations

- `wxCHECK( aZone->IsCopperThieving(), false )`: early return `false` plus `debug_assert!`.

### Known gaps

- The `gap <= 0` / `element_size <= 0` / `line_width <= 0` bail-out is not reachable from a
  file: the parser ignores non-positive values and keeps the constructor defaults (the crafted
  `gap_zero` / `size_zero_squares` boards fill like the reference), so that branch is only
  checked by reading.
- Thieving zones with two copper layers in the file are reduced to one layer by the model
  (`SetLayerSet`), so the board-default offset was only exercised on F.Cu.

## REV-ZF: review of the zone filler port

Findings, coverage map and method: `rust/docs/design/review-zone-fill.md`. The reading of
`zone_filler.cpp` against `filler.rs`, `copper_fill.rs`, `knockouts.rs`, `thermal.rs`,
`connect.rs`, `refill.rs`, `index.rs`, `tool.rs` found no deviation on inputs the reference
survives.

Crafted boards (`tests/data/rev_zf/`, generator `tests/oracle/gen_rev_zf.py`, goldens from
the reference through `tests/oracle/make_rev_zf_goldens.sh`, replay `tests/rev_zf_crafted.rs`):
15 boards, 65 zone layers, all vertex-identical to `kicad-cli pcb drc --refill-zones
--save-board`: custom-pad spoke templates and convex-hull knockouts (`custom_spokes`), hatch
thermal rings / spokes of vias and every pad shape, smoothing levels 2 and 3, hatch offsets
(`hatch_vias`), non-copper hatch / zero min thickness / knockout text (`hatch_noncu`),
conditional flashing against hatch fills incl. `START_END_ONLY` (`hatch_flashing`), island
removal by area and unconnected pours (`islands`), zones owned by footprints (`fp_zones`),
a zone on copper and technical layers, Margin graphics (`mixed_layers`), rule-driven zone
connections and thermal constraints (`rules_conn`), backdrilled / post-machined pads and vias
(`backdrill`), arc outlines, chamfer / fillet, out-of-board fills (`arcs_smooth`,
`arcs_extfillet`), priorities and the UUID tie-break (`priority`), zones too thin for the
min-width cycle (`thin`), the prototype zone fill setting (`prototype`), courtyard knockouts,
coincident item deduplication, net ties (`dedupe_courtyard`). The same boards through
`kicad-rs pcb drc --refill-zones --save-board`: DRC reports identical as sets, saved boards
identical except generated UUIDs.

### Deviations (REV-ZF)

- F-1: an island index the fill no longer has is skipped in `remove_islands`. The C++
  indexes unchecked (undefined behaviour; the reference segfaults on `mixed_isolated`: a zone
  on F.Cu and F.SilkS with isolated copper, whose outline index is listed twice because the
  connectivity maps a technical layer to copper ordinal 0). While the index exists the C++
  double deletion is reproduced.
- F-2: a dependency cycle of the fill DAG (interacting different-net zones with equal
  priority and equal UUID) is broken by filling the remaining items in `toFill` order, with a
  warning in `ZoneFiller::warnings()`. The reference never returns on such a board
  (`dup_uuid`).

### Known gaps (REV-ZF)

- F-3: interacting equal-priority zones of different nets without a UUID in the file have a
  random result in the reference (random UUIDs decide `HigherPriority`); the port's
  generated UUIDs give one of the possible results.
- The ZF-01 gap "`GetFilledPolysList( layer )` ... throws in C++" cannot occur: every layer
  of the layer set has a fill entry (`ZONE::SetLayerSet`), the fallbacks are unreachable.

## FIX-ZF-1: open items of the zone filler review

- Two more crafted boards (`tests/rev_zf_crafted.rs`, goldens from the reference; now 17
  boards, 69 zone layers, all vertex-identical):
  * `tables`: plain `PCB_TABLE`s on F.Cu (with and without border / separators) and on
    Margin knocked out of a copper zone (`addKnockout` graphic overload,
    `PCB_TABLE::TransformShapeToPolygon`); a drill chart, a stackup table and plain tables
    whose cells are knockout under Dwgs.User / F.SilkS zones (`fillNonCopperZone`): no
    knockouts, because `PCB_TABLE::IsKnockout()` is never true (the parser sets the flag on
    the cells only) and generated tables can only sit on documentation layers, so the
    `PCB_DRILL_CHART_T` / `PCB_STACKUP_TABLE_T` cases of `addKnockout` are unreachable from
    either fill path.
  * `no_converge`: the iterative refill flips between two states with period 2 (an island of
    a high-priority zone blocks the corridor of a different-net zone whose cut-off part is
    removed, which lets a low-priority same-net zone connect the island) and stops after 8
    passes; `outcome.warnings` equals the reference's `wxLogWarning` text, the final fills are
    identical.
- `tests/zone_fill_corpus.rs` `load_cli` passes `<board>.kicad_prl` to `load_project`
  (review F-4): `prototype` through `ZONE_CORPUS_DIR=target/zone-corpus-rev` is identical.
- The filler warnings reach the CLI (review F-8; kicad-session `Session::log_warnings`,
  kicad-cli `host::print_log_warnings` after the DRC / Gerber / SVG jobs): printed on stderr
  as `<HH:MM:SS>: Warning: <text>`, byte-identical to the reference on `no_converge` apart
  from the time. The port's dependency-cycle warning (F-2) is printed the same way; the
  reference never gets there (it hangs).
