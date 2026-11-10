# CC-1: Component class synchronisation (dynamic assignment rules)

Crate: kicad-pcb (src/component_class.rs and the board / project functions that call it),
kicad-drc (one new module for the rule compiler / matcher), kicad-session (the wiring in
board_loader.rs)

`COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments` is a `port_pending!` stub in
rust/crates/kicad-pcb/src/component_class.rs, and EVERY board load of the CLI path reaches
it (BOARD_LOADER -> BOARD::SynchronizeComponentClasses, pcbnew/board.cpp L3284): at the
moment `kicad-rs pcb upgrade` cannot load any board with a project because of it
(kicad-session works around it in tests with KNOWN_PENDING_STEPS; the zone-fill harness
skips the step). Closing it is the point of this package.

C++ to port: pcbnew/component_classes/component_class_manager.cpp
SyncDynamicComponentClassAssignments, CompileAssignmentRule, GetDynamicComponentClassesForFootprint
and what is still missing of the manager; component_class_assignment_rule.cpp / .h;
common/project/component_class_settings.cpp COMPONENT_CLASS_ASSIGNMENT_DATA
(GetAssignmentInDRCLanguage: the exact rule text it generates for every condition type,
the JSON load of the assignments if the project reader does not have it yet - check
rust/crates/kicad-pcb/src/project/); BOARD::SynchronizeComponentClasses; in the DRC rule
parser `ParseComponentClassAssignmentRules` (check what kicad-drc's rule_parser already has:
package DRC-20 ported the parser and pcb-dump has a `rules --component-classes` mode);
FOOTPRINT::GetComponentClass's dynamic path (`Footprint::component_class` is
`port_pending!` when assignment rules exist; `Board::footprint_component_class(fp,
rule_matches)` is the faithful version waiting for a matcher).

Design constraint: kicad-drc depends on kicad-pcb, not the other way round. The manager in
kicad-pcb therefore takes the rule compiler as a hook (a small trait or closure defined in
component_class.rs: rule source text -> compiled rule or failure; and for matching: rule +
footprint -> bool), kicad-drc implements it on top of its rule parser and
`DRC_RULE_CONDITION::EvaluateFor( footprint, nullptr, 0, footprint->GetSide(), nullptr )`
(the expression binding is package DRC-21, being finished right now in
kicad-drc/src/pcbexpr: use it; if a piece is still pending call it anyway), and
kicad-session's load sequence passes the hook in. With NO assignments and sheet-class
generation off, the function must run without any hook (plain kicad-pcb callers and tests
keep working): that is the common case and it must work first.

Files you own: kicad-pcb/src/component_class.rs; a new kicad-drc/src/component_class_rules.rs
(+ its `mod` line); minimal edits in the kicad-pcb board / project function that calls the
manager, in footprint/mod.rs `component_class` (the dynamic path only), in
kicad-session/src/board_loader.rs (pass the hook; remove the KNOWN_PENDING_STEPS entry in
kicad-session/tests/common/mod.rs) and in the zone-corpus harness if it skips the step by
name. An integrator and a reviewer are editing other kicad-pcb files at the same time:
re-read a file right before you edit it and keep edits minimal.

Verification: (1) the reference loader path - `pcb-dump board --loader --sections footprint`
(and `rules --component-classes`) vs the Rust session dump (`pcb-dump-session`, see
kicad-session PORT_STATUS.md PCBIO-15) on the boards with component class assignments
(grep -l component_class under qa/data/pcbnew/*.kicad_pro and the DRC test projects, e.g.
the boards of qa/tests/pcbnew/drc/test_drc_component_classes.cpp and
test_component_classes*.cpp): the component class of every footprint must match (if the
footprint dump does not print the effective class, extend the reference dump with one field
as tools/pcb-dump/README.md "Extending" describes - one small C++ edit, rebuilt under the
machine-wide flock - and its Rust counterpart); footprints and pads are landing in the
parser right now, so use whatever loads; (2) port the applicable cases of
qa/tests/pcbnew/test_component_classes.cpp (dynamic assignment and sheet-class cases);
(3) `cargo test -p kicad-session` upgrade_corpus / upgrade_oracle without the
KNOWN_PENDING_STEPS entry: at least the boards that passed before still pass, and
`kicad-rs pcb upgrade --force` on a copy of qa/data/pcbnew/issue10697.kicad_pcb runs end to
end and is byte-identical to the reference.
