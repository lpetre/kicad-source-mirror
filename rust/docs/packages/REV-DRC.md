# REV-DRC: Review of the DRC engine core (reviewer role)

Crate: kicad-drc (review of the finished packages DRC-20 rule parser, DRC-11 markers /
reports, DRC-02 r-tree and provider base, DRC-03 engine core, DRC-04 EvalRules, DRC-05 cache
generator; fixes in THEIR files only)

The DRC engine core was ported by six agents; about ten provider packages are being written
on top of it right now (they own src/providers/* and the newer module files: do not edit
those; findings there go into your result). Each core package verified its part against
the reference (the rule dump of pcb-dump, the EvalRules oracle with 1128 matching
categories, report writers against kicad-cli) and recorded decisions and deviations in
rust/crates/kicad-drc/PORT_STATUS.md. Nobody has yet read the core side by side with the
C++ with a reviewer's eye. The goal of the project: DRC reports (text and JSON) identical
to `kicad-cli pcb drc`. Judge everything by whether it can change a report: a violation
missing or extra, a different constraint value, a different message text, position or
order.

Read: rust/docs/analysis/drc.md (the precedence note, the engine sections, section 4
"order and threading", the risk list), rust/docs/design/board-model.md sections 7 and 13,
the crate's PORT_STATUS.md, then the code next to pcbnew/drc/drc_engine.cpp/.h, drc_rule*.,
drc_rule_parser.cpp, drc_rule_condition.cpp, drc_rtree.h, drc_item.cpp, drc_report.cpp,
drc_cache_generator.cpp, drc_test_provider.cpp and pcbnew/pcbexpr_evaluator.cpp /
pcbexpr_functions.cpp as far as they are ported.

## What to review

1. `EvalRules` (drc_engine.cpp): the implicit rule set built by loadImplicitRules in its
   exact order and with its exact names (they appear in reports), rule sorting and the
   reverse iteration, the per-constraint-type processing (`processConstraint`) incl.
   every special case (netclass and pad / zone local overrides, clearance of zero vs unset,
   the hole-to-hole and edge cases, differential pair gap handling, `a.` / `b.` swapping,
   layer conditions, the "explicit" flag logic, min / opt / max merging, the final fallback
   and the BOARD_DESIGN_SETTINGS minimums), `EvalZoneConnection`, `IsNetTieExclusion`, the
   clearance cache key and when the C++ consults / fills it (design: the resolver / run-state
   split - is anything cached that the C++ recomputes with different inputs, or the other
   way round?), `GetCachedOwnClearance`, the worst-constraint queries (QueryWorstConstraint
   and its cache), unit handling in constraint values.
2. The rule file parser and conditions: token-level fidelity incl. error messages and
   positions (they are printed), severity parsing, the layer clause, expression
   compilation errors; how conditions bind to the expression evaluator (PROP-1 / DRC-21 /
   DRC-22 are in progress: review the seam, not their files).
3. Order: drc.md section 4 lists where iteration order or thread scheduling reaches the
   report. For the core: the order providers run and report in, marker ordering in the
   report writers (the sort comparator in drc_report.cpp / RC_ITEMS_PROVIDER, incl. ties),
   exclusion matching, the r-tree query visit order of DrcRTree (it decides which of
   several violating pairs is reported first when a provider stops at the first hit),
   `std::unordered_map` / pointer-keyed containers in the C++ core and what the port does.
4. Numeric rules of PORTING.md, message formatting (MessageTextFromValue, units, the
   "%s" assembly of violation descriptions - byte exact), JSON report serialisation order
   and number formatting.
5. Coverage of the oracles: pads and footprints are not parsed yet, so every pad-related
   branch of EvalRules is unverified - list those branches; if the footprint / pad parser
   has landed by the time you run (try a board with footprints), rerun the EvalRules oracle
   and the cache-generator oracle with pads and analyse every mismatch.

## Findings

Write rust/docs/design/review-drc-core.md: one row per finding (id, severity blocker /
major / minor, file:line, C++ reference, what differs for which input, status), plus the
table of order-sensitive sites with verdicts. Fix what you can prove in the core packages'
files, with a regression case against the reference; correct the PORT_STATUS.md entries.
Findings in provider files or other running packages' files: list them in
`change_requests`. Larger work: write `rust/docs/packages/FIX-DRC-<n>.md` in the format of
the other files there and list it in your result.

Verification: `cargo test -p kicad-drc` before and after (other agents are adding provider
tests: report failures that are not yours separately), clippy, the wasm32 build. Report:
`summary` with counts per severity; `decisions` = fixes made; `known_gaps` = open findings;
`notes_for_next` = what the provider packages must know.
