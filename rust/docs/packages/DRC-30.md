# DRC-30: Differential pair coupling

Crate: kicad-drc

C++: pcbnew/drc/drc_test_provider_diff_pair_coupling.cpp (778 lines; DRC-30 row of
rust/docs/analysis/drc.md section 10): the extraction of coupled segments per differential
pair (the pairing by net name suffixes through DRC_ENGINE::MatchDpSuffix / IsNetADiffPair -
check what the engine core already has), the gap measurement per coupled run, the
DIFF_PAIR_GAP constraint with min / opt / max and the MAX_UNCOUPLED length check, the
`gapRule` / `uncoupledRule` identity (use `DrcRuleInfo::rule_index`, package FIX-DRC-1),
violation positions and messages byte-exact, the report order.
Tests: qa boards issue7975, issue8407, issue17967/issue17967, diff_pair_uncoupled_tuning_drc
and the C++ tests that use them (drc.md section 9).

DRC package rules and verification: the paragraphs "DRC package rules" and "Verification is
against the reference kicad-cli DRC reports" of rust/docs/packages/DRC-12.md apply verbatim
(read them there, including the orchestrator notes at its end about the run sequence, the
expression cache scope, rule identity and the places where the reference itself is not
reproducible). State when you start: every tier-1 and tier-2 provider is ported, the
expression binding and all rule functions exist (kicad-drc/src/pcbexpr), pads and
footprints load, the shared harness tests/drc_harness is being consolidated by the
stabilisation package STAB-DRC (it may run at the same time as you: you own only the NEW
module files of your package and your test files; if you need a harness change, make it
additive and re-read the file right before editing).
