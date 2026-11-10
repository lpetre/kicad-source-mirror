# DRC-33: Creepage utilities part 2, engine and provider

Crate: kicad-drc

C++ (2540 lines; DRC-33 row of rust/docs/analysis/drc.md section 10):
pcbnew/drc/drc_creepage_utils.cpp L1601-3185, drc_creepage_engine.h / .cpp,
drc_test_provider_creepage.cpp: the remaining shape-pair constructions, the graph build
from the board (nets, layers, board edges and slots as obstacles), the shortest-path search
with its tie order, the CREEPAGE constraint evaluation per net pair, violation paths
(marker path points), messages and positions. The C++ runs net pairs on the thread pool:
the port is sequential; compare as documented for parallel providers.
Tests (drc.md section 9): all test_drc_creepage*.cpp and test_creepage_engine_*.cpp with
their boards; corpus replay for the `creepage` error code against the reference reports
(boards with creepage rules: grep -l creepage qa/data/pcbnew/*.kicad_dru).

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
