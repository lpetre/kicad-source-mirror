# FIX-DRC-1: Rule identity behind `DRC_CONSTRAINT::GetParentRule()`

Crate: kicad-pcb (kernel file `src/drc_api.rs`, one field) and kicad-drc (`src/rule.rs`,
`src/engine/mod.rs`, `src/engine/eval_rules.rs`, plus the consumers
`src/providers/matched_length.rs` and `src/length_report.rs` if they have landed)

Origin: finding R-01 of `rust/docs/design/review-drc-core.md` (package REV-DRC).

## Problem

In C++ a `DRC_CONSTRAINT` returned by `DRC_ENGINE::EvalRules` holds `DRC_RULE*` pointers
(`m_parentRule`, `m_minRule`, `m_optRule`, `m_maxRule`). The port copies what the pointer gives
access to into `kicad_pcb::drc_api::DrcRuleInfo { name, severity, implicit }`. That is enough
for messages and severities, but the pointer is also an *identity*:

* `drc_test_provider_matched_length.cpp` L370 / L405 / L413: `std::map<DRC_RULE*,
  std::set<BOARD_CONNECTED_ITEM*>> itemSets` and `std::map<DRC_RULE*, std::vector<CONNECTION>>
  matches` group the items by the rule that matched them and are iterated (address order);
  `drc_length_report.h` L37 keeps `DRC_RULE* matchingRule`.
* `drc_test_provider_diff_pair_coupling.cpp` L237 / L239 carries `gapRule` / `uncoupledRule`.

The rule file parser accepts several rules with the same name (the review board
`tests/data/review/rev_explicit.kicad_dru` has two rules `"dup name"`; the reference loads
both), and every implicit netclass rule of a class is called `netclass '<name>'`. Two such
rules with the same severity are equal as `DrcRuleInfo`, so a provider that groups by the rule
merges their item sets (length / skew / via-count violations change), and there is nothing to
order the groups by.

## Work

1. `DrcRuleInfo`: add `pub rule_index: Option<u32>`: the index of the rule in
   `DRC_ENGINE::m_rules` (`DrcEngine::rules()`), `None` for a rule that is not in an engine (a
   rule under construction in the parser, the synthetic infos of providers such as the solder
   mask bridge rule). `Default` = `None`. It takes part in `PartialEq`.
2. `DrcEngine::compile_rules`: fill it for `DrcEngineConstraint::rule_info` and for the four
   rule copies inside `DrcEngineConstraint::constraint` (so that `QueryWorstConstraint` results
   carry it as well); the netclass fast path (`netclass_clearances`) must return the same info
   as the general path (`fast_path_equals_general_path` extended to compare the index).
   After a failed `InitEngine` the constraints left over from the first pass keep the indices
   of the discarded rule list (C++: they keep the old rules alive); document it.
3. Consumers group by `rule_index` in a `BTreeMap` (rule creation order). This replaces the
   C++ address order, which is not specified; record it in PORT_STATUS.md as the deterministic
   order (drc.md 4.12) and compare such reports as sets.
4. `DrcEngine::rule( index )` accessor so that a consumer can reach the full `DrcRule`
   (`matchingRule->m_Name`, constraints) from the info.

## Verification

* The engine and EvalRules goldens (`engine_corpus.txt`, `evalrules_corpus.txt`,
  `evalrules_review.txt`) do not print the index and must replay unchanged.
* New unit test in `tests/evalrules_oracle.rs`: on `rev_explicit` the two `"dup name"` rules
  yield different `rule_index` values for a via on F.Cu (min from the second, max from the
  first), and equal names.
* matched_length: a board with two same-named length rules against the reference
  `kicad-cli pcb drc` (set comparison).
