# Review of the zone filler port (REV-ZF)

Scope: `rust/crates/kicad-zone` (ZF-01 driver, ZF-03 copper fill / knockouts / thermals /
hatching, ZF-06 `FillAllZones`): `filler.rs`, `copper_fill.rs`, `knockouts.rs`, `thermal.rs`,
`connect.rs`, `refill.rs`, `index.rs`, `tool.rs`, read side by side with
`pcbnew/zone_filler.cpp` / `.h`, the `zone.cpp` helpers the filler calls (`HigherPriority`,
`UnFill`, `BuildHashValue`, `IsIsland`, `GetInteractingZones`, `BuildSmoothedPoly`; their
port lives in `kicad-pcb`), `DRC_ENGINE::EvalZoneConnection` and the zone-connection branches
of `EvalRules` (`drc_engine.cpp`), `ZONE_FILLER_TOOL::FillAllZones` and its callers in
`pcbnew_jobs_handler.cpp`. Not in scope: `src/teardrop/` (TD-01) and
`addCopperThievingPattern` (ZF-07), both being written while this review ran.

Judged by: can it change a fill, an island flag, the saved board, or what DRC and the plots
see after a refill.

Result: the statement-by-statement reading found no deviation from the C++ on any input
the reference survives. Every fill branch the corpus does not reach now has a crafted board,
(the two rows of FIX-ZF-1 in section 2 followed); all 17 crafted boards are vertex-identical
to the reference, and `kicad-rs pcb drc --refill-zones --save-board` equals the reference on
them end to end. Two inputs make the reference itself crash or hang; the port panicked /
silently left zones empty there and is fixed (F-1, F-2). One class of inputs has no
reproducible reference result (F-3). 0 blockers, 0 major, 3 minor (2 fixed, 1 open in a test
harness), 5 notes.

## 1. Method

* Reading: all 4820 lines of `zone_filler.cpp` against the port, function by function
  (section 4 lists what was compared for each).
* Corpus census: a feature count over the 151 `qa/data` inputs of the zone corpus
  (zones by fill mode / smoothing / island mode / priority / layers / owner, pad and via
  properties, graphics per layer, rule files, project settings). The "corpus" column of the
  coverage map comes from this census (which inputs contain the feature), not from branch
  counters: a feature present in a board whose fill is vertex-identical counts as exercised.
* Crafted boards: `crates/kicad-zone/tests/oracle/gen_rev_zf.py` writes 17 small boards to
  `tests/data/rev_zf/`; `make_rev_zf_goldens.sh` refills copies with the reference
  (`kicad-cli pcb drc --refill-zones --save-board`) and `tests/rev_zf_crafted.rs` compares
  the port with the committed goldens (`tests/golden/rev_zf/*.txt`: per zone layer and
  outline the point count, island flag and a hash of the vertices; 6 KB in total). 15 boards
  have a reference golden (65 zone layers), 2 pin the port's behaviour where the reference
  crashes or hangs.
* End to end: `kicad-rs pcb drc --refill-zones --save-board` against the reference on the
  15 crafted boards: DRC reports identical as sets (242 violations; 2 `unconnected_items`
  entries of `islands` name their two items in the other order, the heap-order case F-1 of
  review-connectivity.md), saved boards identical except for the lines of freshly generated
  UUIDs. This covers the state the saved file does not carry (section 5).
* The same DRC comparison after a refill on the 36 corpus boards with conditionally flashed
  pads / vias or hatch zones: 32 identical as sets; the other 4 differ in one line that has
  nothing to do with fills (the port's `invalid_outline` entry lacks the item line
  `@(0.0000 mm, 0.0000 mm): PCB`, change request to the DRC report owners).
* Reference stability: 4 crafted and 3 corpus boards with interacting zones refilled 3 to 4
  times each by the reference: identical saved boards every time.
* Before / after (no count got worse): zone corpus 656 -> 687 zone layers identical, 0 same
  area, 0 different (the growth is boards other packages added meanwhile);
  `kicad-session/tests/zf06_refill_save.rs` 233 -> 262 saved boards identical, 1 different
  (the known random footprint order of `ZoneFill-4.0.7`), 2 -> 0 not run.

## 2. Coverage map

"corpus": number of corpus inputs that contain the feature (all compared identical);
"crafted": the board of `tests/data/rev_zf/` that reaches the branch. Every crafted board is
identical to the reference unless a finding is named.

### 2.1 `Fill()` driver (L590-1981)

| Branch | Corpus | Crafted |
|---|---|---|
| Board outline valid / not valid (fallback rectangle) | 121 / 30 | all valid; arc edges in `arcs_*` |
| Pre-pass, vias with remove-unconnected: outline test, same-net preference | 8 | `hatch_flashing` |
| Pre-pass: via / pad inside a no-fill keepout (`isInPourKeepoutArea`) | not known | `hatch_flashing` |
| Pre-pass: `START_END_ONLY` vias, blind / micro vias | 0 / 7 | `hatch_flashing` |
| Pre-pass, pads with remove-unconnected | 27 | `hatch_flashing` |
| `toFill`: rule areas skipped, degenerate zone (2 corners) skipped but `SetIsFilled` | 31 / not known | `priority` (`two_corners`) |
| Fill DAG: different-net higher-priority dependency, chains of three | 47 with priorities | `priority` |
| Fill DAG: dependency cycle | 0 | `dup_uuid`: reference hangs, F-2 |
| Teardrop zones (`connectivity->Update`, excluded from islands and refill) | 8 + TD-01 variants | - |
| Islands: ALWAYS / NEVER / AREA (kept and flagged above the threshold, deleted below) | 40 / 26 / 1 | `islands` |
| Islands: zone isolated on every layer is preserved; isolated on one of two layers | 11 net-less | `islands` (`unconnected`, `back_iso`) |
| `IsIsland` of a net-less zone (every outline) | 11 | `islands` (`nonet`) |
| Islands of a zone on copper and technical layers (aliased layer of the zone item) | 0 | `mixed_layers` (NEVER); `mixed_isolated`: reference segfaults, F-1 |
| Iterative refill: island-removal seeds, same-net overlap seeds (issue 23790) | yes (`zone_refill_cascading_islands`, `issue21746`, `issue23790` ...) | `priority` |
| Iterative refill: no convergence after 8 passes (warning) | 0 | `no_converge` (FIX-ZF-1: period-2 flip of three zones; warning text identical, also on the kicad-rs stderr) |
| Out-of-board islands (chamfer / fillet zones) | 11 | `arcs_smooth`, `arcs_extfillet` (zone mostly outside the edge) |
| Second flashing pass (solid fills) | 27 + 8 | - |
| Second flashing pass against a hatched fill | 0 | `hatch_flashing` |
| `aCheck` dialogs | GUI only | - |

### 2.2 `fillSingleZone`, `fillCopperZone`, `fillNonCopperZone`

| Branch | Corpus | Crafted |
|---|---|---|
| Copper zone that also has technical layers (`fillCopperZone` on F.SilkS / F.Mask) | 0 | `mixed_layers` |
| Non-copper zone: solid, knockout text, keepout | 4 | `hatch_noncu` |
| Non-copper zone: hatch pattern, min thickness 0 (the unconditional deflate inflates) | 1 / 0 | `hatch_noncu` |
| Spoke filter: endpoint in the test area / kept through another spoke (issue 13316) | yes / not known | `custom_spokes`, `rules_conn` |
| Min-width cycle skipped (`half_min_width - epsilon <= epsilon`), also in the post-knockout prune | 0 | `thin` (0.003, 0.0041 and 0 mm) |
| Hatch: orientation 0 / 45 / other, smoothing level 0 / 1 / 2 / 3 | 2 / 2 / 0, 3 / 1 / 0 / 0 | `hatch_vias` (30 deg level 2, 0 deg level 3), `hatch_noncu` (-20 deg level 1), `hatch_flashing` (15 deg) |
| Hatch: `hatch_position` of the zone and of the board defaults | 3 | `hatch_vias`, `hatch_noncu` |
| Hatch: holes dropped around thermal rings, clearance holes and fully connected vias | 1 pad | `hatch_vias`, `backdrill` |
| Hatch + `m_PrototypeZoneFill` (`.kicad_prl`): filled solid | 0 | `prototype` |
| Pre-knockout / apron / pre-hatch caches, post-knockout prune | yes | `priority`, `backdrill` (hatch zone above a solid one) |
| `subtractHigherPriorityZones` (also for a zone owned by a footprint) | yes / 0 | `priority`, `fp_zones` |
| Copper thieving | 2 | ZF-07 |
| `DebugZoneFiller` stage dumps | stage corpus (ZF-03) | not reachable from the CLI |

### 2.3 Knockouts (`knockoutThermalReliefs`, `buildCopperItemClearances`, `addKnockout`)

| Branch | Corpus | Crafted |
|---|---|---|
| Pad kinds: other net, net-less zone, override, NPTH (also on mask layers only) | yes / 11 / 27 / 19 | `mixed_layers`, `rules_conn`, `hatch_vias` |
| Pad backdrilled or post-machined on the layer (both functions) | 0 | `backdrill` (backdrill, tertiary drill, counterbore, countersink; same and other net) |
| Via backdrilled / post-machined | 1 / 1 | `backdrill` (solid and hatch zone) |
| Solid zone, connection NONE: flashed pad, unflashed pad with hole | 3 (pad override) | `rules_conn`, `hatch_flashing` |
| Hatch zone, pads: THERMAL, THERMAL without permitted flashing, NONE, FULL | 1 pad | `hatch_vias` (13 pads), `hatch_flashing` |
| Hatch zone, vias: other net, THERMAL with gap > 0, FULL (`aSolidConnectionItems`), START_END_ONLY, backdrill disc | 0 | `hatch_vias`, `hatch_flashing`, `backdrill` |
| Custom pad knockout: outline / convex hull (also with a thermal gap) | 13 / 0 | `custom_spokes` |
| Custom pad whose primitives do not join the anchor (two outlines) | not known | `custom_spokes` (U5) |
| Tracks, arcs, vias: clearance / physical / hole clearance, same net, override | yes (arcs: 10) | `backdrill`, `rules_conn` |
| Graphics on the layer, with nets, on Edge.Cuts | 37+ / 2 / 121 | `mixed_layers` |
| Graphics on Margin | 0 | `mixed_layers` |
| Texts, knockout text, text box, target, bar code, dimension | 7 / 2 / 1 / 1 / 1 / 2 | `hatch_noncu` (knockout text) |
| Table, drill chart, stackup table knockouts | 1 table (on Dwgs.User) / 0 / 0 | `tables` (FIX-ZF-1: plain tables on F.Cu and Margin; generated tables can only sit on documentation layers, and `PCB_TABLE::IsKnockout()` is never true, so tables with knockout cells and generated tables under Dwgs.User / F.SilkS zones are no knockouts there) |
| Courtyard knockouts (`physical_clearance` against a footprint): gap 0, gap > 0, back side, inner layer | 1 | `dedupe_courtyard` |
| Net-tie footprints: graphic touching an allowed pad skipped, others knocked out | 7 (skip taken: not known) | `dedupe_courtyard` |
| Coincident pad / via / track deduplication (same and different nets, reversed ends, arcs: F-7) | not known | `dedupe_courtyard` |
| Keepouts: board zones, footprint zones (rotated, arcs), smoothed keepouts | 14 / 6 / 0 | `fp_zones`, `arcs_smooth` |
| Zone of a footprint that is not a keepout (never filled by `FillAllZones`, still a knockout candidate) | 0 | `fp_zones` |
| Different-net zone clearances, teardrop knockouts | yes / 8 | `priority` |

### 2.4 Thermal spokes and rings

| Branch | Corpus | Crafted |
|---|---|---|
| Spokes of circular pads (circle, oval with equal sides), spoke angle rotation | yes (angles: 16) | `hatch_vias`, `rules_conn` |
| Non-circular pads at the pad's spoke angle, rotated footprints | yes | `hatch_vias`, `custom_spokes` |
| Spoke width: clamp to `[Min, Max]`, pad minor axis, skip below the min thickness | yes | `rules_conn` (rule with min / opt / max), `hatch_vias` (pad and via narrower than the limits) |
| Custom-pad spoke templates (`gr_vector`): rotation, reversed template, template outside the pad, trimming, hole with shape offset | 0 | `custom_spokes` (footprints at 0, 30, 270 degrees) |
| Hatch zone spokes of pads and vias | 1 pad | `hatch_vias`, `hatch_flashing` |
| Hatch thermal rings: circular, non-circular (shape difference), vias | 1 pad | `hatch_vias` |

### 2.5 Constraints (`EvalZoneConnection`, `EvalRules`)

| Branch | Corpus | Crafted |
|---|---|---|
| Zone default; pad `zone_connect`; footprint `zone_connect` | yes / yes / 9 | `rules_conn`, `hatch_vias` |
| Rule `zone_connection` on the pad; on the footprint (inheritance pass) | 0 / 0 | `rules_conn` |
| `THT_THERMAL` (zone or pad): PTH pad thermal, SMD pad full, via full | 4 | `rules_conn` |
| Thermal gap / spoke width: zone, pad override, rule | yes / 4 / 0 | `rules_conn` |
| `physical_clearance`, `physical_hole_clearance`, `hole_clearance`, `edge_clearance`, layer rule `clearance` | 1 / 1 / 3 / 3 / 8 | `rules_conn` |

### 2.6 Zone geometry helpers (kicad-pcb)

| Branch | Corpus | Crafted |
|---|---|---|
| Arc outlines (zones, keepouts) | 0 | `arcs_smooth`, `fp_zones` |
| Chamfer / fillet, `zones_allow_external_fillets` | 3 / 8, 5 | `arcs_smooth`, `arcs_extfillet` |
| `HigherPriority` UUID tie-break between equal priorities | not known | `priority` (three zones) |
| Multi-layer zones | 20 | several |

## 3. Order and determinism

* Thread pool sites and their sequential replacement: the fill DAG (`run_fill_waves`), the
  refill waves (no dependencies, snapshot), the out-of-board check (`submit_blocks`, results
  applied in block order), inline tessellation. The port runs Kahn order with roots in
  `toFill` order, refills in candidate order and one block.
* Is the real fill thread-dependent in the reference? No. A fill reads another zone's fill
  in three places only (`buildCopperItemClearances` for teardrops,
  `buildDifferentNetZoneClearances`, `refillZoneFromCache`), each behind
  `other->HigherPriority( zone ) && !SameNet && zoneKnockoutMayInteract( zone, other )`; the
  dependency edge is created by `!zone->HigherPriority( other ) && !SameNet &&
  zoneKnockoutMayInteract( zone, other )` for every unfilled zone of the query box. Both
  agree whenever `HigherPriority` is a strict total order (distinct UUIDs), and the refill
  reads a snapshot. Everything else a fill reads is an outline, a static item, a zone-layer
  override written before the DAG, or the connectivity built before it (`FlashLayer`).
  Measured: 7 boards with interacting zones, 3 to 4 reference runs each, identical. The
  thread-dependent stage dumps ZF-03 saw exist only in `DebugZoneFiller` mode.
  Any topological order is therefore one the reference produces; the port's is one.
* Where `HigherPriority` is not a strict order the reference does not terminate (F-2), and
  where the UUIDs are generated on load its result is random (F-3).
* `ZONE*`-keyed and other pointer / hash containers: `oldFillHashes`, `zoneOutlineIndices`,
  `fillItemsByLayer`, the four caches, the snapshot, `netTiePads`, `filledZonesByNetLayer`,
  the knockout key sets, `visited`: lookups or any-tests only. `isolatedIslandsMap`,
  `refillIslandsMap`: iterated, each zone handled independently (the Remove / Add order
  inside `FillIsolatedIslandsMap` is review-connectivity.md P6: no effect).
  `changedZoneLayers` (`std::set` in address order): decides only the discovery order of
  `zonesToRefill`; the candidate set, the snapshot refills and the hash comparison do not
  depend on it. `std::set<RESULTS>` is ordered by value. The port uses `ZoneId` (board
  order) everywhere.
* `queryIndex` sorts by `m_seq`, unique inside an index: no tie order.

## 4. Statement-level reading

| Part | Compared | Verified against the reference by |
|---|---|---|
| `Fill()` set-up, order of connectivity build, worst clearance, outline, indexes, pre-pass | statement order, every predicate, `holeRadius = drill / 2 + 1`, first-wins priority ties | corpus; `hatch_flashing` + DRC e2e |
| `buildItemIndexes`, `queryIndex` | sequence numbering (footprint shares the reference field's number), per-layer indexes | corpus (knockout append order shows in the vertices) |
| Fill DAG, `UnFill`, hashes, fill flags, `SetIsFilled` (also for degenerate zones) | reading | saved boards (`(fill yes)`), `priority` |
| Island removal, refill loop, `refillZoneFromCache` | descending index order, AREA threshold as `double < long long`, stale flags, snapshot keys, gates | `islands`, `priority`, corpus |
| Out-of-board islands | area tests, bounding-box shortcut, deletion order | `arcs_*`, corpus |
| Second flashing pass | reach = `max( hole radius, min pad size / 2 )`, inflated bbox test | DRC e2e (51 boards) |
| `knockoutThermalReliefs`, `buildCopperItemClearances`, `buildDifferentNetZoneClearances`, `subtractHigherPriorityZones`, `addKnockout` (3 overloads) | every constraint type and operand order (`EvalRules( c, pad, zone )` in the first, `( c, zone, item )` in the others), `>` vs `>=` on gaps (`gap > 0` for flashed vias, `>= 0` elsewhere), `extra_margin`, strictly-greater zone clearance, backdrill radius `GetMaxHoleSize() / 2 + clearance` | corpus; `backdrill`, `rules_conn`, `mixed_layers`, `custom_spokes` |
| `fillCopperZone`, `fillNonCopperZone`, `postKnockoutMinWidthPrune` | every Clipper operation: operand order, corner strategy (CHAMFER for deflate and the spoke test area, ROUND with simplify for re-inflation), max error, simplify flag, the `Simplify()` after keepouts only | corpus (656 layers), stage corpus, `thin`, `hatch_*`, `mixed_layers` |
| `buildThermalSpokes`, `buildSpokesFromOrigin`, `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` | `std::clamp` with libstdc++ semantics, `KiROUND` sites, truncating `%` of the grid origin, `BBox( 0 )` = clearance 0 (F-5), smoothing switch, min hole area | `custom_spokes`, `hatch_vias`, `hatch_noncu`, `rules_conn` |
| `VERTEX_CONNECTOR`, `connect_nearby_polys` | z-order window, contour distance, `RESULTS` order, stable sort of the insertions | corpus (every solid fill) |
| `EvalZoneConnection` | line by line; pad taken from `a` first, then `b`; no pad or not PTH -> FULL | `rules_conn`, `hatch_vias` |
| Zone-connection order of `EvalRules` (pad local override, rules on the item, footprint inheritance pass, footprint local, zone) | read in C++; the port is kicad-drc's (REV-DRC) | `rules_conn` (each level decides at least one pad) |
| `FillAllZones` headless, callers in the jobs handler (`FillAllZones`, never `CheckAllZones`) | reading | zf06 saved boards, DRC e2e |

Numeric rules: integer halving (`GetMinThickness() / 2`, `spoke_w / 2`, `knockoutSize / 2`),
`mmToIU` constants, `(int) GetCornerRadius()`, `double` areas, the `double -> int`
conversion of `connect_nearby_polys`' distance and `SquaredEuclideanNorm -> ecoord` are as in
C++. No `sqrt` / trigonometry outside kicad-geom. `unwrap_or` / "treated as empty": the zone
local clearance always has a value (`ZONE::GetLocalClearance`), missing cache entries are the
C++ `find() == end()` branches, a missing fill entry cannot occur (F-6).

## 5. State after `Fill()`

| State | C++ | Port | Evidence |
|---|---|---|---|
| Fills, island flags, `m_isFilled` | written per zone layer; flags never renumbered | same | saved boards identical |
| `m_needRefill` | cleared when the fill function returned true | same | reading (not saved) |
| Fill flags, hashes, triangulation | per layer after publishing; `UpdateTriangulationDataHash` after island removal | same | refill convergence (hash compare) identical on `priority`, corpus |
| Zone-layer overrides of pads / vias | cleared for all, pre-pass, demoted in the second pass; not saved | same, collected then applied | DRC after refill: 15 crafted + 36 corpus boards |
| Connectivity | rebuilt before the fill, zones re-added by the island map, full `BuildConnectivity` afterwards (`PostFillRefresh`) | same (`tool.rs`) | DRC e2e (`unconnected_items`, `isolated_copper`, `via_dangling`, `starved_thermal` counts and items) |
| Time stamp | `IncrementTimeStamp` once, before `Fill`; fills and overrides do not bump it | same | reading |
| `m_area` (`CalculateFilledArea`) | computed | not computed | only the GUI search panel reads it |
| Caches of the filler | pre-knockout cache survives a `Fill()`, the others are cleared | same; the refill result cache is omitted (pure) | reading |
| Project local setting `m_PrototypeZoneFill` | read through the project | `board.project.prototype_zone_fill`, bound by the session loader | `prototype` (test and `kicad-rs`) |

## 6. Findings

| Id | Severity | Where | C++ reference | What differs, for which input | Status |
|---|---|---|---|---|---|
| F-1 | minor | `filler.rs` `remove_islands` | `zone_filler.cpp:1286-1304`, `:1597-1607`; `connectivity_items.h:164-171` | A zone on a copper layer and a technical layer (a hand-written or scripted board). `CN_ITEM::SetLayers` maps the technical layer to copper ordinal 0, so the zone item of F.SilkS answers `GetBoardLayer() == F_Cu` and `FillIsolatedIslandsMap` lists its outline index a second time under F.Cu. With an isolated copper fill the C++ deletes the same index twice: undefined behaviour, the reference segfaults in `SHAPE_POLY_SET::DeletePolygonAndTriangulationData` (`mixed_isolated`, backtrace taken). The port panicked (`removal index (is 0) should be < len (is 0)`). | fixed: an index the fill no longer has is skipped; test `rev_zf_mixed_layer_zone_with_isolated_copper_does_not_panic`. While the index exists the double deletion (a second, unrelated outline goes) is reproduced; `mixed_layers` (NEVER mode) is identical |
| F-2 | minor | `filler.rs` `run_fill_dag` | `zone_filler.cpp:930-964`, `:1143-1185`; `zone.cpp:545-556` | Two interacting zones of different nets with equal priority and the same UUID (the parser does not make zone UUIDs unique; legacy `(tstamp 0)` zones all get the nil UUID). Neither is `HigherPriority`, each waits for the other and `Fill()` never leaves its drain loop: the reference was killed after 40 s (`dup_uuid`). The port left both zones without a fill and said nothing. | fixed: the items of a cycle are filled in `toFill` order (no fill reads the other one's: the read gate needs `HigherPriority` too) and `ZoneFiller::warnings()` reports it; test `rev_zf_duplicate_zone_uuids_terminate` |
| F-3 | note | kicad-pcb UUID generation on load; `Zone::higher_priority` | `zone.cpp:555`, `kiid.cpp` | Zones without a `(uuid ..)` get random UUIDs in the reference; for interacting zones of equal priority and different nets the winner of the overlap is then random: 6 reference runs of one board gave 2 different fills (3 each). The port's generated UUIDs are deterministic, its result is one of the two. | documented; such boards cannot be compared exactly (none in the corpus differs) |
| F-4 | minor | `tests/zone_fill_corpus.rs` `load_cli` | `SETTINGS_MANAGER::LoadProject` | The corpus harness loads the project without its `.kicad_prl`, so `m_PrototypeZoneFill` is never set there (`prototype` through `ZONE_CORPUS_DIR`: 2165 instead of 197 points). The session / CLI loader passes it and is identical. No corpus board has a `.kicad_prl`. | fixed (FIX-ZF-1): `load_cli` reads `<board>.kicad_prl`; `prototype` through `ZONE_CORPUS_DIR` is identical |
| F-5 | note (docs) | `docs/analysis/connectivity-zones.md` 4.12 | `zone_filler.cpp:4378` | "`bbox = filled.BBox( 0 )` (outline 0 only)": the argument is the clearance, the box spans all outlines. The port is right. | change request |
| F-6 | note | `filler.rs` `fill_outline_count`, `remove_out_of_board_islands`, `flashing_second_pass`; PORT_STATUS "throws in C++" | `zone.h:701` | `GetFilledPolysList( layer )` cannot miss: `ZONE::SetLayerSet` creates an entry for every layer and `UnFill` keeps them. The port's "treated as empty" fallbacks are unreachable. | accepted |
| F-7 | note | `knockouts.rs` track deduplication | `zone_filler.cpp:2641` | The key of a track is (canonical ends, width); an arc and a straight track, or two different arcs, with the same ends and width share one knockout (the first in board order), the other gets none: the reference fills over the second arc (`dedupe_courtyard`: its own DRC then reports "Clearance violation (zone clearance 0.3000 mm; actual 0.0000 mm)" for both such arcs). An upstream defect. | reproduced, identical to the reference (`dedupe_courtyard`) |
| F-8 | note | `tool.rs` `fill_all_zones`, kicad-session | `zone_filler.cpp:1656` (`wxLogWarning`) | The filler's warnings (non-convergence, F-2) are returned in `FillAllZonesOutcome::warnings`; whether the CLI prints them is the session's business. | fixed (FIX-ZF-1): `Session::refill_zones` keeps them in `Session::log_warnings`, kicad-rs prints them after the DRC / Gerber / SVG jobs as `<HH:MM:SS>: Warning: <text>` on stderr, like the reference (`no_converge`) |

Checked and found equivalent (no finding): everything in section 4; the hatch-zone via walk
over `Tracks()` (not the index); `ZONE_CONNECTION::NONE` vias falling through to the
clearance knockouts; the keepout test of the pre-pass using `HasKeepoutParametersSet` while
the knockout of `buildCopperItemClearances` does not; `fillSingleZone` choosing the copper
path by `IsOnCopperLayer()` of the zone, not of the layer; `SetNeedRefill( false )` only on a
true return; the cache keys under `DebugZoneFiller` (F.Cu); `postKnockoutMinWidthPrune`'s
apron inflation by epsilon; the hatch border restoration of the refill; `sameNetOverlapSeeds`
including footprint zones as the higher zone.

## 7. For the differential campaigns

* Fills: compare exactly, except boards with interacting equal-priority zones of different
  nets whose UUIDs are not in the file (F-3).
* A board on which the reference dies or hangs is F-1 / F-2 until shown otherwise; the port
  must terminate with fills.
* DRC after a refill: compare as sets; `unconnected_items` between coincident anchors of two
  zone layers may name the items in either order (review-connectivity.md F-1).
* Crafted boards are regenerated with `gen_rev_zf.py`; add a branch there, rerun
  `make_rev_zf_goldens.sh`, commit board and golden.
