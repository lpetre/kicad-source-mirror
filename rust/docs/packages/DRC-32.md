# DRC-32: Creepage utilities, part 1

Crate: kicad-drc

C++ (2500 lines; DRC-32 row of rust/docs/analysis/drc.md section 10):
pcbnew/drc/drc_creepage_utils.h and drc_creepage_utils.cpp L1-1600: the creepage graph
model (nodes, connections, the shape classes with their geometry: point, track segment,
circle, arc, the board-edge shapes), path / tangent constructions between shape pairs with
their exact floating-point expression order, the pruning rules. This package has no
provider yet: it is verified by a C++ driver that links the reference build (the pattern
of kicad-drc tests/oracle: one driver, built under the machine-wide flock) and dumps the
graph (nodes, connections with lengths as hex doubles) for crafted shape pairs and for the
qa creepage boards, replayed by a Rust test; plus the C++ unit tests
test_drc_creepage_circle_prune.cpp and test_drc_creepage_issue24524.cpp.

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
