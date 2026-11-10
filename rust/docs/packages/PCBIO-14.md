# PCBIO-14: Footprint library files (.kicad_mod parse + write, fp upgrade)

Crate: kicad-pcb

You own src/io/parser/fp_lib.rs and src/io/writer/fp_lib.rs (staging stubs created by the
parser / writer core packages), and the fp upgrade job wiring in kicad-session (minimal
sanctioned edits in run_fp_upgrade).

C++: the PCBIO-14 row of rust/docs/analysis/pcb-io.md section 10: ParseFootprintLib /
the footprint-file entry of the parser (a .kicad_mod is one (footprint ...) expression with
its own version header rules; per the design the footprint is parsed into a holder Board with
one attached footprint - the C++ library parser runs with a null board, so every
`GetBoard() == nullptr` default touched during parsing must be shown equal: this is the
recorded risk of the design review), FormatFootprintLibFile and the CTL_FOR_LIBRARY output
differences against a board footprint (which tokens are omitted or written differently),
the FootprintSave normalisation steps (pure functions only), JobExportFpUpgrade
(pcbnew/pcbnew_jobs_handler.cpp) with its messages and exit codes. The footprint and pad
parser / writer packages (PCBIO-06, PCBIO-07) must have landed: read their sections in
rust/crates/kicad-pcb/PORT_STATUS.md.

Board file I/O package rules: specification rust/docs/analysis/pcb-io.md (precedence note;
sections 3-7). Faithful port: every token, every version-dependent branch, the same error
texts; on the writer side the same token order, optional-token rules and number formatting.

Verification: `kicad-cli fp upgrade --force -o OUT IN` is a pure parse-and-write reference:
for every .kicad_mod in the tree (the corpus has 426: find qa/data demos -name '*.kicad_mod';
copies in a scratch dir, at most two reference processes at a time) the output of
`kicad-rs fp upgrade --force` must be byte-identical; report identical / differing with the
first-difference token / failing, and fix what belongs to footprint-file code (list the rest
with the owning package). Port the C++ tests the analysis row names (test_fp_lib_load_save.cpp
etc.).
