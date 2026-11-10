# STAB-PCB: Board model against the reference on the whole corpus - scoreboard to zero

Crate: kicad-pcb (all files of finished packages; every port package of this crate is
finished - read PORT_STATUS.md, rust/docs/design/integration-2.md and, if it exists,
rust/docs/design/review-2.md first)

Each model package verified its own slice against the reference dump on a few boards.
Whole boards with footprints and pads load only since the pad parser landed, so the model
has never been compared as a whole. Do that now, on everything, and fix what differs.

1. Dump scoreboard. For every corpus board under 1 MB (qa/data/pcbnew, qa/data/cli, demos;
   COPIES in a scratch dir; hashed output; one board at a time; PORTING.md section 7 limits):
   `pcb-dump board --no-init --hash --sections item,bbox,track,pad,shape,text,dim,zone,footprint,misc,board,order`
   vs `pcb-dump-rs` through rust/tools/pcb-dump/compare.py (tools/io-corpus/run.sh --dump
   does one board; write a driver script under rust/tools/pcb-dump/ that does the corpus,
   caches the reference side under rust/target/dump-corpus/ and prints one line per board
   and section: match / mismatch / pending / missing, plus totals). Commit the script and a
   compact summary (tests/golden/dump_scoreboard.txt), not the dumps.
2. Triage every mismatch class (group by section and field; start with the most frequent):
   the reference value, the Rust value, the smallest board that shows it, the cause down to
   the statement. Fix defects of the model / parser in place with a committed regression
   case (rerun the unhashed dump for the board to see values). `pending` lines whose getter
   now exists get wired; `rust-error` lines are panics to fix. Known open items: effective
   shapes and polygons of pads in rotated / scaled / flipped footprints, footprint bounding
   boxes and hulls with text variables, dimension text, zone hatch lines, table cells,
   barcode boxes (no symbol encoder: expected, list them), meander settings (not modelled:
   expected).
3. The io corpus (tools/io-corpus/run.sh): 359 of 370 identical when the pad parser landed.
   Refresh its goldens (`--update-golden`), remove the #[ignore] from the staging boards
   that are identical now, and analyse the 11 differing boards: differences caused by the
   reference's post-load steps (zone net / priority after BuildConnectivity) belong to the
   session-level comparison (kicad-session upgrade corpus) - confirm that for each and list
   them; outline-font render caches are the documented caveat (ORACLE.md) - confirm for
   each that nothing else differs after masking them.
4. Large boards once, by hand, hashed, one at a time (machine rules): the demos over 1 MB
   through the io corpus byte comparison only.

Other agents: a reviewer (INT-2b) may still be reading kicad-pcb and writing findings;
packages CC-1 (component_class.rs) and PCBIO-14 (io/*/fp_lib.rs) may be running: do not edit
their files, re-read any file right before you edit it.

Report: the scoreboard totals before and after per section; every mismatch class with cause
and status (fixed / expected with reason / open with the smallest board); io corpus counts.
Goal: zero unexplained mismatches.
