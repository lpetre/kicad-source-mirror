# FIX-RESAVE-1: The last differing boards of the re-save comparison

Crate: kicad-session (tests, board_loader.rs) and whichever finished package's file holds
the cause (kicad-pcb parser / writer / model, kicad-connectivity)

State: `kicad-rs pcb upgrade --force` (load through the full CLI load sequence, save) is
byte-identical to the reference `kicad-cli pcb upgrade --force` on 358 of the 370 corpus
boards (kicad-session `upgrade_corpus`, an ignored test; reference results cached by the
io-corpus harness under rust/target/io-corpus/: O1 = reference re-save of the input, O2 =
reference re-save of O1). 11 boards differ and 1 does not load. The parser / writer level
comparison (tools/io-corpus/run.sh) shows the same 11: first differences `zone > net` (4
boards), `zone > priority` (3), `gr_text > render_cache` (3), `footprint > fp_text >
render_cache` (1).

Work:
1. For each of the 11 boards and the unloadable one: the complete list of differing lines
   (not only the first), grouped by cause, and the cause down to the C++ statement that
   produces the reference bytes:
   - `zone > priority` / `zone > net`: which step of the reference changes them between
     parse and save (candidates: the post-load sequence in pcbnew/board_loader.cpp and
     what BOARD::BuildConnectivity / net propagation / SynchronizeNetsAndNetClasses do to
     zones, teardrop zones and their priorities, zones whose net is missing or renamed,
     duplicate priorities fixed at load or at save - read pcb_io_kicad_sexpr.cpp format(ZONE)
     and the loader for anything that renumbers), and why the port does not do the same.
     Fix it where it belongs. Package STAB-CN is verifying net propagation inside
     kicad-connectivity at the same time: if the cause is in that crate, give it the board
     and the statement in change_requests instead of editing there.
   - render caches: the reference regenerates the outline-font render cache at save from
     the fonts installed on the oracle machine (ORACLE.md caveat). Establish exactly what
     it writes when the font is not installed / is installed (which fonts does the oracle
     machine have: `fc-list`), what the port writes (it keeps or drops the cache it read?),
     and define the comparison rule: after masking `(render_cache ...)` blocks nothing else
     may differ on these boards. If the port can reproduce the reference bytes without a
     font engine in some of the cases (e.g. the reference drops the cache, or rewrites the
     one it read), make it do so.
2. Make `upgrade_corpus` a non-ignored test over the boards under 300 KB with a committed
   compact golden (board name, hash of the reference result, expected status) so a
   regression shows up in the normal test run; the full corpus stays an ignored test.
3. `kicad-rs pcb upgrade` behaviour around the save: the "Board file was not updated" case
   (second run on an upgraded board), `--force`, the legacy-version warnings and exit
   codes vs the reference on ten boards of different file versions.

Verification / report: per board: identical / identical after the render-cache mask /
differs (with cause). Goal: 370 of 370 explained, and every difference that is not the
font caveat fixed.
