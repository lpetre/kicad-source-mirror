# DRC-34: DRC leftovers

Crate: kicad-drc

C++ (900 lines; DRC-34 row of rust/docs/analysis/drc.md section 10): the `EvalRules`
REPORT branches of drc_engine.cpp (the reporter texts the clearance resolution tool and
`pcb drc` verbose paths print - port them so the engine can explain a constraint; verify
the texts with a C++ driver through DRC_ENGINE::EvalRules with a WX_STRING_REPORTER on the
EvalRules oracle boards), drc_engine.cpp L3267-3410 (the own-clearance cache and the
remaining engine helpers not yet ported: list what the engine core left as gaps in
PORT_STATUS.md and close them), drc_test_provider_net_chain_tuning_profiles.cpp,
drc_test_provider_via_stitch.cpp, the stub of drc_test_provider_library_parity.cpp
(L1154-1216: what the provider does in kicad-cli where no library table is loaded - port
that behaviour, i.e. which violations and messages appear, with the raw zone_connect
accessors the model review asks for), drc_item.cpp L628-710 (`GetViolationLayers`).
Tests: test_drc_net_chain_tuning_profiles.cpp, test_drc_violation_layers_issue24941.cpp;
corpus replay for the error codes these providers emit.

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
