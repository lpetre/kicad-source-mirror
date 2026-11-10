# DRC-31: Matched length, skew, via count, net chains

Crate: kicad-drc (and kicad-pcb / kicad-connectivity for the length calculation it needs)

C++ (2040 lines; DRC-31 row of rust/docs/analysis/drc.md section 10):
pcbnew/drc/drc_test_provider_matched_length.cpp, drc_length_report.h,
drc_chain_topology.h / .cpp, and the length calculation they call:
pcbnew/length_delay_calculation/* (LENGTH_DELAY_CALCULATION::CalculateLengthDetails, pad-to-die
lengths, via lengths through the stackup, the optimisation of track ends inside pads / vias,
time-domain delay with tuning profiles only as far as the length rules reach it - say what
you leave out) - check first what exists in the port (rg length_delay / GetTrackLength in
rust/crates) and port the missing parts into kicad-pcb or a module of kicad-drc where the
dependency direction allows (kicad-connectivity is available to kicad-drc; the review of
connectivity lists F-6: `length_delay_calculation.cpp:707` takes the first pad of
GetConnectedPads(via) - heap order in C++, ItemId order in the port).
The provider: LENGTH / SKEW / VIA_COUNT constraints, grouping of items by the matching rule
(group by `DrcRuleInfo::rule_index` in a BTreeMap - package FIX-DRC-1 work item 3 is yours:
providers/matched_length.rs and length_report.rs are placeholders), net chains
(drc_chain_topology), the report texts with their unit formatting, positions and items.
Tests (drc.md section 9): test_drc_lengths.cpp, test_drc_skew*.cpp, test_drc_chain_*.cpp,
test_drc_stub_length.cpp, test_drc_return_path_*.cpp, test_drc_length_*.cpp with their
boards; plus a board with two same-named length rules against the reference (set
comparison) as FIX-DRC-1 asks.

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
