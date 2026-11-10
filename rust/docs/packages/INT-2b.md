# INT-2b: Review of the port decisions in kicad-pcb (reviewer role)

Crate: kicad-pcb (review of all finished packages; fixes in finished packages' files)

The board model, the board file reader / writer and the project settings of kicad-pcb were
ported by about fifty independent agents. Each recorded its decisions, deviations and known
gaps in rust/crates/kicad-pcb/PORT_STATUS.md (2,800 lines, one section per package). Nobody
has yet read them side by side with the C++ with a reviewer's eye. That is your job. The
goal of the project is output identical to KiCad's: byte-identical board re-save, identical
DRC results, identical Gerber / drill files, identical connectivity. Judge everything by
whether it can change one of those outputs.

## What to review

1. Every "Decisions" and "Deviations" entry of every package section of
   kicad-pcb/PORT_STATUS.md. For each: is it really equivalent to the C++ for every input, or
   only for the inputs its author tested? Verdicts: `ok`, `ok-documented-difference` (cannot
   reach an output; say why), `wrong` (fix), `unproven` (needs an oracle case: add one).
2. Cross-cutting traps - search the code, do not rely on the status file:
   - iteration order: every place where the C++ iterates a `std::set` / `std::map` keyed by
     pointer, an `unordered_map` / `unordered_set`, or sorts by pointer, and the order can
     reach an output (file order, report order, first-match-wins logic, floating-point
     accumulation order). board-model.md section 3.1 has the replacement rule and the list
     that was known at design time; find the ones that are missing from it and check that
     each port does what the rule says (and that the rule is right for that site).
   - numeric rules of PORTING.md (sections 3 and 9): `KiROUND`, plain C casts of doubles
     (`f64_to_i32_x86` vs Rust's saturating `as`), integer overflow that wraps in the
     reference build, `abs` / negation of INT_MIN, glibc libm through `kicad_geom::cmath`,
     the order of floating-point operations, `%g`-style number formatting.
   - `unwrap_or`, `unwrap_or_default`, early returns and "treated as 0 / empty" in places
     where the C++ would dereference, assert, throw or read uninitialised memory: is the
     chosen behaviour what the release build actually does on real boards?
   - state the C++ mutates as a side effect and the port keeps immutable or caches (design
     section 8): e.g. `RebuildBezierToSegmentsPointsList(aErrorMax)` during the outline build
     (package M21 keeps the board unchanged; in C++ later readers of the bezier points see
     the re-approximated points - decide with the C++ call graph of DRC and plotting whether
     an output can differ, and fix or prove), lazily built caches whose build order or build
     parameters differ, the time-stamp re-stamping in the footprint cascades (M19), dirty
     flags, `m_effectiveShapes`-style caches built with the first caller's parameters.
   - string handling: wxString semantics (code points vs bytes, case folding, locale-free
     number parsing, `wxString::Format` vs Rust formatting, `Matches`, tokenizers), UTF-8
     edge cases in names and text.
   - default values: a field whose Rust default differs from the C++ constructor (the
     defaults scoreboard covers what pcb-dump prints; look for members it does not print).
3. Spot checks of code against C++: for each package pick the functions with the highest
   output risk (formatters, comparators, geometry builders, anything with tolerance or
   rounding) and read them side by side with the C++. Depth over breadth: a function you
   check, you check completely.

The connectivity, DRC, zone and fabrication crates get their own reviews later; stay inside
kicad-pcb except to follow a call.

## What to do with findings

- Write `rust/docs/design/review-2.md`: one table row per finding (id, severity blocker /
  major / minor, package, file:line, the C++ reference, what differs and for which input,
  status). Keep "ok" verdicts out of the table; give a per-package count instead.
- Fix what you can prove: in files of finished packages, fix it and add a regression test
  that reproduces the difference against the reference (pcb-dump vs pcb-dump-rs, kicad-cli
  re-save, or a small C++ driver - PORTING.md section 9 self-verification applies to your
  fixes too). Correct the PORT_STATUS.md entry.
- Files of running packages (do not edit; list the finding for the orchestrator instead):
  footprint/geometry.rs, footprint/shapes.rs, footprint/compare.rs,
  footprint_courtyard_index.rs, io/parser/footprint.rs, io/parser/pad.rs, io/parser/fp_lib.rs,
  io/writer/footprint.rs, io/writer/pad.rs, io/writer/fp_lib.rs, src/properties/,
  pcb_barcode.rs.
- A finding too large to fix inside this review: write a work package file
  `rust/docs/packages/FIX-<n>.md` in the format of the other files in that directory (crate,
  files, C++ reference, what is wrong, how to verify) and list it in your result; the
  orchestrator launches it.
- Do not "fix" a documented design decision of board-model.md (arenas, handles, ctx, time
  stamp) unless it changes an output; then describe it as a finding for the orchestrator.

## Verification

Before you start: `cargo test -p kicad-pcb --lib` once, to know the pre-existing failures.
After: the same plus the integration tests whose files you touched, clippy on kicad-pcb,
check_item_shims.py, the defaults scoreboard and the io corpus harness summary
(`rust/tools/io-corpus/run.sh`, see its README: the identical / fixed-point counts must not
go down).

Report: `summary` with the counts per severity; `decisions` = the fixes you made (one line
each, with the evidence); `known_gaps` = open findings with their id; `change_requests` =
findings in running packages' files; `notes_for_next` = the FIX packages you wrote.

## Reports from other packages to check during the review (added by the orchestrator)

- DRC-24: "the render cache of a gr_text is gone right after parse_board_report" on
  qa/data/pcbnew/issue22475, text ff749b73-2383-44e8-88dd-1d9c1b0c1a89
  (`text.eda_text().render_cache_data()` is None after the parse). Find the setter or
  finishing step that clears it; decide with the C++ (EDA_TEXT::ClearRenderCache call sites,
  what the reference re-save writes for such a text when the font file is not installed)
  what the correct behaviour is, fix and add a regression case.
- DRC-06: the board's KiidGenerator starts from a default seed that reproduces UUIDs stored
  in some QA boards (issue10697), so freshly generated UUIDs can collide with item UUIDs.
  Check how fresh UUIDs are drawn during parse / load (the C++ draws random UUIDs at defined
  points; the port must draw at the same points so a seeded test run is reproducible, but a
  generated UUID must never equal one that is in the file): propose the rule (e.g. the
  generator skips UUIDs present in the board's UUID index) and apply it if it is local.
- DRC-24 asks for `Board::is_layer_visible` (BOARD::IsLayerVisible); DRC-27 asks for a public
  writer for the fill of a detached zone (`Zone::fill_mut` or `set_filled_polys_list`).
  INT-2a may already have added them; add them if not.
