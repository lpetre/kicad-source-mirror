# PCBIO-15: Load / save orchestration (BOARD_LOADER glue, re-save command)

Crate: kicad-session

Specification: rust/docs/analysis/pcb-io.md sections on the loading flow around the parser
(what happens after parse in each entry point: project binding, legacy fix-ups,
BuildListOfNets, SynchronizeNetsAndNetClasses, BuildConnectivity - which propagates nets onto
tracks and so changes what the writer saves -, zone-fill validity flags, UUID fix-ups,
RefreshGeneratedTables) and the PCBIO-15 row of section 10; rust/docs/analysis/settings.md on
the two load sequences (kicad-cli BOARD_LOADER vs the QA harness KI_TEST::LoadBoard). The
model-side steps are already exposed in order by kicad-pcb's project package (SET-5: read its
section in rust/crates/kicad-pcb/PORT_STATUS.md), connectivity is kicad-connectivity (CN-04:
read its PORT_STATUS.md), the DRC engine is kicad-drc.

You own the loader modules of rust/crates/kicad-session (session.rs
`Session::initialize_loaded_board` and its PENDING_LOAD_STEPS list, a new board_loader.rs)
and the upgrade job; read rust/crates/kicad-session/PORT_STATUS.md first.

C++: pcbnew/board_loader.cpp + .h (BOARD_LOADER::Load and every step in order, the options),
pcbnew/pcb_io/pcb_io_mgr.cpp and pcb_io.cpp load / save wrappers with their error mapping,
pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp LoadBoard / DoLoad / SaveBoard pre-steps,
pcbnew/pcbnew_jobs_handler.cpp JobUpgrade and the common 'load board for a job' helper
(getBoard / loadBoard with the project, rule file and variable overrides -D), and
qa/pcbnew_utils/board_test_utils.cpp KI_TEST::LoadBoard as a second, separately named entry
point (tests use it; the reference dump tool's default mode is this path, `--loader` is the
kicad-cli path).

Deliver: `Session::load_board` running the complete kicad-cli sequence (so that
PENDING_LOAD_STEPS is empty), `Session::load_board_qa` running the QA sequence, both pure
(texts in, no filesystem), and the re-save path used by `kicad-rs pcb upgrade`.

Verification: (1) `pcb-dump board --loader` and `pcb-dump board` (QA path) against
`pcb-dump-rs` driven through your two entry points (extend the dump driver's loader hook:
kicad-pcb cannot depend on kicad-session, so add a small bin or test helper in kicad-session
that loads through the session and calls kicad_pcb::dump::dump_board) on every completely
loadable corpus board under 500 KB: sections board, item, track, connectivity summary - zero
mismatches, in particular net codes / net names of tracks after propagation; (2) byte level:
`kicad-rs pcb upgrade --force` vs the reference `kicad-cli pcb upgrade --force` over the
corpus through the io-corpus harness (rust/tools/io-corpus/run.sh): report the scoreboard
(boards byte-identical / differing with first-difference tokens / failing) and fix every
difference that belongs to the load / save orchestration; list the others with their owning
package.
