# FIX-ZF-1: what the zone filler review (REV-ZF) left open

Crate: kicad-zone (tests), kicad-session / kicad-cli (one line each). Findings and coverage
map: `rust/docs/design/review-zone-fill.md`. Nothing here is a known wrong result; these are
the branches of `pcbnew/zone_filler.cpp` that still have no differential evidence, and two
small plumbing items in files REV-ZF did not own.

1. Crafted boards for the two rows of the coverage map marked FIX-ZF-1 (add them to
   `crates/kicad-zone/tests/oracle/gen_rev_zf.py`, regenerate with `make_rev_zf_goldens.sh`,
   commit board and golden, list them in `tests/rev_zf_crafted.rs`):
   * knockouts of `PCB_TABLE`, drill chart and stackup table items on a filled layer
     (`ZONE_FILLER::addKnockout`, graphic overload; `fillNonCopperZone` for a table with
     knockout cells on a technical layer). Needs those items to load and render in the port.
   * an iterative refill that does not converge within 8 passes (`Fill()` L1633-1657): the
     warning text is the `wxLogWarning` of the C++ and must reach the caller
     (`ZoneFiller::warnings()`). Constructing one needs zones whose island removal and
     reclaim keep alternating; if none can be built, say so in the review document.
2. F-4: `crates/kicad-zone/tests/zone_fill_corpus.rs` `load_cli` passes `None` for the
   `.kicad_prl` text to `load_project`; read `<board>.kicad_prl` next to the input like
   `tests/rev_zf_crafted.rs` does (the `prototype` board through `ZONE_CORPUS_DIR` then
   matches).
3. F-8: `FillAllZonesOutcome::warnings` (non-convergence, dependency cycle F-2): make sure
   `Session::refill_zones` / the jobs print them the way kicad-cli prints a `wxLogWarning`
   (check what the reference prints on a non-converging board once item 1 has one).

Verification: `cargo test -p kicad-zone --test rev_zf_crafted`, the zone corpus
(`tools/zone-corpus/run.sh`) and `kicad-session/tests/zf06_refill_save.rs`: no count gets
worse (state at REV-ZF: 687 zone layers identical, 0 different; 262 saved boards identical,
1 different with a known cause).
