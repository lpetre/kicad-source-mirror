# STAB-DRC: DRC against the reference with pads - every provider, every corpus board

Crate: kicad-drc (all files; every provider package is finished when you start - read
PORT_STATUS.md, rust/docs/design/review-drc-core.md and review-connectivity.md first: they
list the places where the reference itself is not reproducible and must be compared as a
set / by count / tie-aware)

The providers were ported while no board with footprints or pads could be loaded; each was
proven on the footprint-free part of the corpus (about 100 of 365 boards) and on synthetic
boards. Pads parse since PCBIO-07, and with them the first real comparisons arrived: several
provider tests went red (drc07 connectivity corpus, drc24 courtyard cases, drc25 synthetic,
drc27 solder mask, drc08a). Your job is the fix loop until the whole crate is green and the
corpus scoreboard is clean.

1. Baseline: `cargo test -p kicad-drc --no-fail-fast` and every corpus replay (the shared
   harness tests/drc_harness prints a scoreboard per error code: boards compared, matched /
   missing / extra / order). Record it. If the DRC job (DRC-12: `kicad-rs pcb drc`, the full
   provider registry in engine::PROVIDER_ORDER with the cache generator and connectivity) has
   landed, also run the whole-report comparison: `kicad-cli pcb drc --format json
   --severity-all` and `--format report` vs the port on every corpus board under 5 MB
   (reference results are cached under rust/target/drc-corpus/; COPIES; one at a time).
2. For every difference find the cause down to the statement:
   - a provider defect (fix, regression case = the board + error code);
   - an engine / EvalRules defect in a pad branch (the review lists the pad branches that
     were unverified: R-06) - rerun the EvalRules and cache-generator oracles with pads
     (`pcb-dump board --sections evalrules,engine` goldens: regenerate hashed for boards with
     pads) and fix;
   - a kicad-pcb model defect (pad shapes, footprint courtyards, text boxes ...): prove it
     with pcb-dump vs pcb-dump-rs on the smallest board and fix it there if it is local and
     nobody is editing the file (package STAB-PCB is doing the model scoreboard at the same
     time: re-read before editing; otherwise report it in change_requests);
   - a kicad-connectivity defect (package STAB-CN is doing the same for connectivity):
     report with the board;
   - a documented non-reproducible site: make the comparison set-based / tie-aware there
     (FIX-CN-1 rule for unconnected items) and list it;
   - stale test expectations from the time before pads: update them.
3. Harness: the shared harness must load boards the way the CLI does (project binding, rule
   file, BuildConnectivity, cache generator) - if packages left private wrappers that do the
   wiring per test (ConnectivityWithCaches and the like), consolidate them.
4. Order: where the C++ order is deterministic the port must match it; report every place
   where you had to fall back to set comparison and why.

Report: baseline and final scoreboard per error code (boards compared, matched, missing,
extra, order differences); every fixed defect in one line with its board; every open
difference with the smallest reproducing board and your diagnosis.
