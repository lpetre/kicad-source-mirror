# Review of the DRC engine core (package REV-DRC)

Reviewer pass over the finished core packages of `kicad-drc` (DRC-20 rule parser, DRC-11
markers / reports, DRC-02 r-tree and provider base, DRC-03 engine core, DRC-04 `EvalRules`,
DRC-05 cache generator), read side by side with `pcbnew/drc/drc_engine.cpp/.h`, `drc_rule.*`,
`drc_rule_condition.cpp`, `drc_rule_parser.cpp`, `drc_rtree.h`, `drc_item.cpp`,
`drc_report.cpp`, `common/rc_item.cpp`, `drc_cache_generator.cpp`, `drc_test_provider.cpp` and
the three functions of `pcbexpr_functions.cpp` the implicit rules use. Everything is judged by
whether it can change a `kicad-cli pcb drc` report: a violation missing or extra, a different
constraint value, message text, position or order.

State of the tree: 2026-11-10, footprints load (PCBIO-06), pads do not (PCBIO-07), the
expression binding (DRC-21) and the keepout predicate (DRC-10) are `port_pending!`.

## 1. Result

No blocker and no wrong result was found in the core: the statement-by-statement reading
turned up no deviation from the C++ that is not already recorded in
`crates/kicad-drc/PORT_STATUS.md`, and the one area that had no differential coverage (user
rules: the corpus boards with rule conditions are skipped as "needs DRC-21") now has it and
matches. What remains are one design gap at the seam to the providers (R-01), the pad-related
branches that nobody can verify before the pad parser exists (R-06), and notes for the
packages built on top.

| Severity | Count | Ids |
|---|---|---|
| blocker | 0 | |
| major | 2 | R-01 (open), R-06 (open, verification) |
| minor | 7 | R-02, R-03, R-04, R-07, R-08 (open / accepted), R-05, R-09 (fixed) |

## 2. Findings

Line numbers are those of 2026-11-10 (other packages edit the same crate; the function name
is the stable reference).

| Id | Sev. | Where (Rust) | C++ reference | What differs, for which input | Status |
|---|---|---|---|---|---|
| R-01 | major | `kicad-pcb/src/drc_api.rs:122` `DrcRuleInfo`; filled by `kicad-drc/src/rule.rs` `DrcRule::info`, `engine/mod.rs` `compile_rules` (`rule_info`), `engine/eval_rules.rs` fast path | `drc_rule.h` `DRC_CONSTRAINT::GetParentRule()` returns `DRC_RULE*`; `drc_test_provider_matched_length.cpp:370,405,413` (`std::map<DRC_RULE*, ...>`), `drc_length_report.h:37`, `drc_test_provider_diff_pair_coupling.cpp:237,239` | The rule copy carried by a constraint has name, severity and the implicit flag but no identity. Two user rules with the same name (the parser accepts that: `rev_explicit.kicad_dru` has two rules `"dup name"`, the reference loads both) and the same severity are indistinguishable, so a provider grouping items by matching rule (length / skew / via count) merges two groups and reports different violations; there is also nothing to order the groups by (C++: address order). No effect on the core's own results or on any message text. | open: `docs/packages/FIX-DRC-1.md` (needs the kernel file `drc_api.rs`) |
| R-02 | minor | `engine/mod.rs` `DrcEngine::run_tests` (L997) | `drc_engine.cpp:936-994` `RunTests`; `drc_cache_generator.cpp:325-329` | `run_tests` takes the providers as a ready slice and borrows the connectivity for the whole call, but providers that use the per-run caches are constructed from them (`DrcTestProviderConnectivity::new( connectivity, caches )`, physical clearance), i.e. after the cache generator, which also has to rebuild the connectivity. The function therefore cannot drive a real run (already a DRC-05 known gap); callers use `prepare_run` + `DrcCacheGenerator::run` + `run_providers`. Nothing checks that the slice is in `PROVIDER_ORDER`; the order decides the report order and which provider's violations are dropped at a shared error limit (clearance: copper + physical; shorting items; silk over copper). | open: DRC-12 (CLI flow) must build the providers after the cache generator and pass them in `PROVIDER_ORDER` (`test_provider::DRC_TEST_PROVIDERS` has the ranks); `run_tests` should become a factory-taking wrapper or go |
| R-03 | minor | `rtree.rs` `get_objects_at` (L624) | `drc_rtree.h:526` returns `std::unordered_set<BOARD_ITEM*>`; consumers `drc_test_provider_connection_width.cpp:430-450` (`contributingItems[0]`, `[1]`: the pair `EvalRules` is asked about and the item order of the marker), `drc_test_provider_edge_clearance.cpp:164` (first hit = the edge named in the violation) | C++ iterates in hash order of heap addresses (not reproducible between runs); the port returns tree search order without duplicates. Where several items qualify, the reference itself can name different items from run to run. | accepted (PORT_STATUS DRC-02); consumers must be compared with the items of a violation as a set |
| R-04 | minor | `engine/mod.rs` `init_engine_with` error path (L674-689), `compile_rules` | `drc_engine.cpp:906-927`, `724-768` | After a `PARSE_ERROR` from `compileRules` the C++ clears `m_rules` but not `m_constraintMap`: the constraints compiled before the bad rule (all implicit rules and the user rules before it) stay, then the implicit rules are appended a second time, *after* those user rules. The port reproduces this. It is reachable only with a log reporter (without one the parser already throws from `loadRules`, before anything is compiled), and `kicad-cli pcb drc` aborts on the error, so no report is affected. | accepted (same as C++); read only, not replayable before DRC-21 |
| R-05 | minor | `tests/evalrules_oracle.rs`, `tests/golden/evalrules_corpus.txt` | `drc_engine.cpp:1667-1927` `processConstraint`, `2025` explicit return | The `EvalRules` oracle compared 99 boards completely, but the 56 corpus boards whose rule files use conditions were skipped as a whole, so the user-rule half of `EvalRules` (layer clause test, last rule wins per field, explicit rule returned before local clearances and the board minimum, rule severities, `MAXIMUM_CLEARANCE` clamp, unconditional disallow mask and layer logic, hole-to-hole operand test, `DIFF_PAIR_GAP` raised to the pair clearance with a user rule) had no systematic differential coverage, only a Rust unit test. | fixed: four review boards with unconditional rules, golden from the reference, `evalrules_review_boards_match_cxx` (section 5): 32 categories, 0 mismatches, nothing pending |
| R-06 | major | `engine/eval_rules.rs`, `engine/diff_pair.rs`, `rtree.rs`, `cache_generator.rs`, `rc_item.rs` | section 4 | Every pad-related branch is ported statement by statement but has never run against the reference: the Rust board has no pads yet. | open until PCBIO-07; the oracles compare the pad categories by themselves once the key counts match (620 `EvalRules` categories, 8 cache generator boards, 6 r-tree boards, 2 report scenarios) |
| R-07 | minor | `engine/eval_rules.rs` `OwnClearanceCache` | `drc_engine.cpp:3212-3293` | Key `(uuid, layer)` as in C++; dropped when the board time stamp moves instead of by `InvalidateClearanceCache` from item setters. Checked for the CLI flow (load, `InitEngine`, optional refill, `RunTests`): nothing changes an input of `EvalRules( CLEARANCE, item, nullptr )` between a C++ cache fill and a later read, so the stricter invalidation cannot change a value. Items sharing a UUID share the entry in both (first evaluation wins; in C++ the first evaluator depends on thread scheduling). | accepted (PORT_STATUS DRC-04) |
| R-08 | minor | `engine/mod.rs` `DrcRunState::report_violation` | `drc_engine.cpp:2366-2380` | The per-code cap (199; 499 for clearance and unconnected items) keeps the first violations reported. With the C++ thread pools the survivors of a provider that reports from workers differ from run to run; the port is sequential and deterministic. A board that exceeds a cap cannot be compared entry by entry, only by count. | accepted (drc.md section 8) |
| R-09 | minor | `PORT_STATUS.md` | - | Stale numbers and reasons in the core sections (oracles that tightened since the packages reported). | fixed (REV-DRC section there) |

Checked and found equivalent (no finding): the implicit rule list (order, names, layers,
values, the value-less courtyard constraint, `std::sort` of the netclass clearance / track
width list with libstdc++ tie order, composite netclasses, the diff-pair clearance override
flag, tuning-profile rules, keepout rule names and flags); `compileRules` flags; the netclass
fast path against the general path; `applyConstraint` (per-field merge, `m_ImplicitMin`,
clamp, value rules); the condition memo (keyed by expression, per call, cleared before the
footprint inheritance pass, unused with a reporter); the commutative retry of
`DRC_RULE_CONDITION::EvaluateFor`; the native implicit conditions against
`hasExactNetclassFunc`, `inDiffPairFunc`, `isCoupledDiffPairFunc`, `VALUE::EqualTo` /
`NotEqualTo` on an undefined property; clearance of zero versus unset (`override_val != 0`,
`SetMin( 0 )` of a net tie has a minimum); `MatchDpSuffix`; `QueryWorstConstraint`
(unconditional = no compiled condition object); error limits; `ReportViolation` log lines;
`forEachGeometryItem` (track callbacks ignore the return value; else-if order of the drawing
branches; table before barcode); `formatMsg`; report writers; the 77 item titles and settings
keys and the `allItemTypes` order (cross-checked by script against `drc_item.cpp`).

## 3. Order-sensitive sites of the core

| Site | C++ | Port | Verdict |
|---|---|---|---|
| Provider run order | registry = link order of the provider translation units | `engine::PROVIDER_ORDER`, printed by every reference engine dump | identical; not enforced on the caller's slice (R-02) |
| Rule order (`m_rules`) | implicit rules in a fixed sequence; netclasses and composites in `std::map<wxString>` order; rule file order | same sequence; `BTreeMap<String>` (UTF-8 byte order = code point order) | identical (367-board engine corpus, review boards) |
| Netclass clearance / track width rule list | `std::sort` by `m_Constraints[0].Min()`, ties in libstdc++ order | `kicad_geom::stdsort::std_sort` | identical (ties decide which class is named, not the value) |
| Constraint lists per type | vector in rule order, iterated forward, last applicable wins per field | same | identical |
| `m_constraintMap`, `m_explicitConstraints` | `std::map` by type; iterated only to derive flags | `BTreeMap` | order-insensitive |
| `m_netclassClearances`, `m_ownClearanceCache`, `conditionCache` | `std::unordered_map`, lookups only | `BTreeMap`, lookups only | order-insensitive |
| `DRC_RTREE` search order | `KIRTREE::PACKED_RTREE` | hand port in kicad-geom | identical on tracks, arcs, vias, zones, graphics (r-tree oracle, incl. Hilbert ties); pad boards pending |
| `DRC_RTREE::QueryColliding*` dedup sets | `unordered_set` / `unordered_map`, lookups only | `HashSet` / `HashMap`, lookups only | order-insensitive |
| `DRC_RTREE::QueryCollidingPairs` | pair list in tree order; dedup key ordered by address | pair list in tree order; key ordered by `ItemId` | identical visit sequence (oracle) |
| `DRC_RTREE::GetObjectsAt` | `std::unordered_set<BOARD_ITEM*>` iterated by callers | search order, deduplicated | C++ not reproducible (R-03) |
| Cache generator `allZones` | `std::set<ZONE*>`, schedules independent per-zone work | board zones, then footprint zones | order-insensitive |
| `m_DRCZones`, `m_DRCCopperZones`, `m_DRCCopperZonesByLayer` | vectors in board walk order | same | identical (oracle) |
| `m_CopperZoneRTreeCache` | `std::unordered_map<ZONE*>`, iterated by the connection width provider | `ZoneId` order | C++ not reproducible; affects only which items a connection-width marker names first |
| `m_ZoneIsolatedIslandsMap` | `std::map<ZONE*>` (address order), iterated by the connectivity code | board zone order | deterministic choice; address order usually equals creation order but is not guaranteed: compare as sets |
| Copper item tree storage order | `forEachGeometryItem` order, layer by layer | same | identical (oracle) |
| `DRC_ITEMS_PROVIDER` | board marker order, `std::stable_sort` by severity, highest first | `sort_by` (stable), same key | identical (report oracle) |
| Marker order on the board | handler call order through the commit, `m_markers.push_back` | DRC-12 | not core; the handler order is the provider order times each provider's own order |
| Exclusion set | `std::set` ordered by the serialised sort key; unmatched exclusions become markers in that order | same key bytes | identical (report oracle, 39 exclusions) |
| Ignored checks, JSON keys | `allItemTypes` order; nlohmann sorted keys | same | identical (report oracle) |
| Rule-pointer keyed maps of providers | `std::map<DRC_RULE*, ...>` | no rule identity yet | R-01 |

## 4. Unverified: the pad-related branches

Ported from the C++ text, never executed against the reference (the reference results are
already in the goldens):

* `EvalRules`: the net-tie exemption (`footprints[]`, `GetNetTieCache`, same-footprint test);
  the local override block for `CLEARANCE` / `HOLE_CLEARANCE` (`PAD::GetClearanceOverrides`:
  pad, else parent footprint; the larger of A and B; raise to `m_MinClearance` /
  `m_HoleClearance`; names `pad`, `footprint <ref>`, `board minimum`, `board minimum hole`;
  an override of exactly 0 falling through); pad overrides of `ZONE_CONNECTION`,
  `THERMAL_RELIEF_GAP`, `THERMAL_SPOKE_WIDTH` (with the zone's minimum thickness);
  `SOLDER_MASK_EXPANSION` and both paste margins from the pad, else its footprint (returned in
  `opt`, `KiROUND( ratio * 1000 )`); the footprint inheritance pass (pad replaced by its
  footprint, memo cleared, second walk) and the `board setup` defaults; the legacy local
  clearance of a pad (`pad`); the footprint's local zone connection; a zone's pad connection
  and thermal values with a pad as the other item.
* `processConstraint`: `DISALLOW` masks for pads, footprints (plus the courtyard layers) and
  `HOLE_PROXY` items; the exemption of a footprint from its own keepout
  (`implicitKeepoutZone->GetParentFootprint()`); `HOLE_TO_HOLE` with pad holes.
* `EvalZoneConnection` (`THT_THERMAL` -> `THERMAL` for a PTH pad, else `FULL`);
  `GetCachedOwnClearance` of an NPTH pad (`HOLE_CLEARANCE`); `IsNetTieExclusion`.
* `DRC_RTREE::Insert` pad hole entries and the null pad shape; the copper item tree with pads
  on all layers; `GetMaxClearanceValue` with pad overrides; item lines of the reports.
* Not pad-related but still pending elsewhere: `A.Layer == '...'` of tuning-profile rules
  (DRC-21), `A.intersectsKeepout()` (DRC-10), every user rule condition (DRC-21), the
  `REPORTER` texts of `EvalRules` (DRC-34, never part of a CLI report).

## 5. Verification done by this review

* `cargo test -p kicad-drc` before the review: all core tests green (2 failures in
  `drc24_courtyard_text`, a provider package in progress).
* New differential regression (closes R-05): `tests/data/review/rev_{implicit,fastpath,
  explicit,clearance}.kicad_pcb` + `.kicad_pro` (+ `.kicad_dru`), written by
  `tests/oracle/gen_review_boards.py`: one pad-less 4-layer board (36 tracks of nine nets on
  every copper layer, through / blind / buried / micro vias, copper and silk graphics and
  texts, three copper zones with local clearances 0.6 / 0.1 / 0, two keepouts), five
  netclasses by pattern (one net in two classes = composite class), with / without the
  diff-pair clearance override, 23 unconditional user rules of 17 constraint types
  (`(layer inner|outer|"B.Cu")`, two and three rules per type, `(severity ...)`, a 600 mm
  hole clearance, `disallow`, two rules with the same name), and clearance rules per layer
  (700 mm clamped to 500 mm). Reference: `pcb-dump board --hash --sections evalrules`
  reduced by `gen_evalrules_goldens.py --out tests/golden/evalrules_review.txt`. Replay:
  `evalrules_oracle.rs::evalrules_review_boards_match_cxx`: 4 boards, 8 categories each
  (track, via, zone, drawing, disallow, pair_tt, pair_zt, pair_zz; 1570 result lines per
  board), all identical, nothing pending.
* `EVALRULES_RAW_DIR=<dir>` makes the replay write the unhashed Rust lines per board, to diff
  a future mismatch against the unhashed reference dump.
* Corpus replays today: `EvalRules` 1148 categories match on 99 complete boards, 0
  mismatches (pad categories and 36 keepout boards pending); the r-tree and cache generator
  oracles now also compare the zone / graphic boards (`connection_width_rules`, `issue24649`,
  `issue18142`, `two_segment_teardrop`, `via_stacks`), 0 mismatches.
