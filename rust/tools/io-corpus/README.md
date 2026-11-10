# io-corpus: corpus differential harness of the board reader / writer (PCBIO-17)

Drives the `.kicad_pcb` parser and writer packages (PCBIO-01 .. PCBIO-16) against the reference
`kicad-cli` (`docs/ORACLE.md`) on the whole board corpus, as specified in
`docs/analysis/pcb-io.md` 9.3 (D2).

```
tools/io-corpus/run.sh                     # all boards: oracle cache + Rust re-save + scoreboard
tools/io-corpus/run.sh --board issue18     # one board: report + unified diff O2 vs rust(O1)
tools/io-corpus/run.sh --update-golden     # full run, then rewrite tests/golden/io_corpus/
tools/io-corpus/run.sh --dump issue18 [--o1] [--sections pad,track] [--hash]
                                           # item level: pcb-dump --no-init vs pcb-dump-rs
```

Run from anywhere; it calls cargo in `rust/` (one cargo command, `-p kicad-pcb`).

## What it does

1. **Oracle cache** (`kicad_qa::io_corpus`, under `rust/target/io-corpus/boards/<path with
   / as __>/`, never in the repository). For every `.kicad_pcb` below `qa/data/pcbnew` and
   `demos`: `O1` = `kicad-cli pcb upgrade --force` of a copy of the board file alone (no
   `.kicad_pro` / `.kicad_dru`), `O2` = the same re-save of a copy of `O1` (stored only when it
   differs from `O1`; `meta` records `saved fixed` / `saved changed` / `failed`). The key is the
   FNV-1a 64 hash and size of the input plus the modification times of the oracle wrapper, the
   real binary and `_pcbnew.kiface`, so a re-run calls the oracle only for new or changed boards
   or after an oracle rebuild (the first full run takes about 10 minutes, later runs about one).
   At most two oracle processes run at a time (`IO_CORPUS_JOBS`), under `nice -n 15`, each with
   a private `KICAD_ORACLE_HOME` (`oracle-home-N/`). Without an oracle everything is skipped
   silently.
   Skip list: boards of 10 MB or more (`vme-wren` twice, the Allegro `VCU118` import,
   `jetson-agx-thor`) and the known-bad `plugins/fakeboard.kicad_pcb` (not a board; the parser
   tests check that the port rejects it).
   A board is a **fixed point** when `O1 == O2`; only those are strictly byte-exact targets, but
   every board is compared with `O2` (the oracle's re-save of the same input the port reads).
2. **Rust side** (`crates/kicad-pcb/tests/io_corpus.rs`, test `corpus_scoreboard`, ignored in
   the default run): parse `O1` and format the board with `allow_pending` (staging stubs skip
   their constructs and report them), catching `port_pending!` panics of model stubs. Status per
   board, best first:
   * `identical`: byte for byte equal to `O2`;
   * `identical-uuid-masked`: equal after masking UUIDs that do not occur in `O1`
     (`kicad_qa::uuid_canon`);
   * `identical-modulo-pending`: equal after removing, from both texts, every top-level block
     of a kind that a staging stub skipped (the top-level ancestor of each parser-skipped
     construct; each top-level kind that is or contains a writer-skipped item). Everything the
     port writes today is right;
   * `differs`: different even then (the interesting case: ported code produces wrong bytes, or
     the oracle's post-load steps changed something the parse + format path cannot reproduce
     yet, see below);
   * `model-pending`: a `port_pending!` model stub stopped the re-save (named);
   * `parse-error`, `format-error`, `panic`, `oracle-error`, `skipped`.
3. **Scoreboard**: `rust/target/io-corpus/scoreboard.md` (also printed): counts per status
   (and how many are fixed points), histograms of the pending constructs (parser side, writer
   side), of the model stubs hit, of the first differences (modulo pending, and of the full
   texts) by `top-level block > keyword`, of errors, and one row per board.
4. **Golden set** (`crates/kicad-pcb/tests/golden/io_corpus/`): `O1` (`<stem>.kicad_pcb`) and,
   when not a fixed point, `O2` (`<stem>_o2.kicad_pcb`) of a dozen small boards covering the
   format versions 2020 .. current, with the status recorded in `expected.txt`. The default
   `cargo test -p kicad-pcb` runs `golden_staging` (each board must reach at least its recorded
   status; improvements are printed) and the `strict_<stem>` tests (parse + format **without**
   staging must reproduce `O2` byte for byte), which stay `#[ignore]`d naming the packages the
   board waits for. When a package lands: `run.sh --update-golden`, then remove the `#[ignore]`
   of each board `golden_staging` now reports `identical`.
5. **Item level** (`--dump`): copies the board (or with `--o1` its cached `O1`) to a scratch
   directory, runs `pcb-dump board --no-init` (parser only, the path the Rust loader takes
   today) and `pcb-dump-rs board`, and feeds both to `tools/pcb-dump/compare.py` (per-section
   match / mismatch / pending / missing / extra counts). Boards of 2 MB or more need `--hash`
   and `--sections`. The dumps stay in `rust/target/io-corpus/dump/` unless over 100 MB.

## Reading the results

* `rust_resave` is parse + format. `kicad-cli pcb upgrade` additionally runs
  `BOARD_LOADER::initializeLoadedBoard` (connectivity, net propagation, ...), which is PCBIO-15
  / `kicad-connectivity`. Differences in `(net ..)` tokens or track order that come from it are
  reported as `differs`; they disappear when PCBIO-15 lands, they are not masked.
* Boards whose `O1` holds outline-font render caches or embedded fonts carry a caveat: the
  oracle regenerates glyphs from the host fonts (`pcb-io.md` 9.3 skip list).
* `run.sh --board NAME` prints the full report (pending constructs with counts and packages,
  both first differences with context) and `diff -u O2 rust(O1)`; the files stay in
  `rust/target/io-corpus/board/`.
* Legacy-input comparison (D3: `rust_resave( original )` vs `O1` with
  `kicad_qa::uuid_canon::canonicalise_uuids_sorted`) is provided by the canonicaliser but not
  run by the scoreboard yet: it only becomes informative once the tier-1 packages have landed.
