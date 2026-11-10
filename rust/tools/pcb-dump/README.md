# pcb-dump

A C++ command-line tool linked against the real pcbnew code of the reference build
(`/home/user/kicad-oracle/build`) that loads boards headlessly and prints the board data model in
the stable line format of [`rust/docs/PCB_DUMP_FORMAT.md`](../../docs/PCB_DUMP_FORMAT.md). It is
the ground truth for the Rust board model below the `kicad-cli` level (bounding boxes, effective
shapes, polygons, courtyards, text strokes, layer sets, flashing, defaults, save order).

## Build

```
rust/tools/pcb-dump/build.sh          # -> /home/user/kicad-oracle/bin/pcb-dump
```

* Objects go to `/home/user/kicad-oracle/build-pcb-dump/`, compiles run under `nice -n 15` with
  `JOBS=2` (override with `JOBS=`, `OBJ_DIR=`, `OUT=`, `ORACLE_BUILD=`). A full rebuild takes
  about 2 minutes, an incremental one (one file) about 40 s plus a 20 s link.
* Compile flags are copied from `compile_commands.json` (the entry of
  `pcbnew_kiface_objects/board_loader.cpp`), the link line is ninja's `_pcbnew.kiface` link line
  with `-shared`, `-o` and `pcbnew.cpp.o` (the real KIFACE) replaced by our objects. Nothing in the
  C++ tree or the reference build is modified.
* Reused QA harness sources, compiled unchanged: `qa/mocks/kicad/common_mocks.cpp` (mock
  `Kiface()`), `qa/mocks/include/mock_pgm_base.h` (`MOCK_PGM_BASE`), `qa/qa_utils/mock_utils.cpp`
  (`SetMockConfigDir`), `qa/qa_utils/stdstream_line_reader.cpp`,
  `qa/qa_utils/wx_utils/{unit_test_utils,wx_assert}.cpp`, `qa/pcbnew_utils/board_file_utils.cpp`
  (`KI_TEST::ReadBoardFromFileOrStream`). The machine has no Boost.Test library, so
  `boost_test_impl.cpp` compiles the header-only Boost.Test implementation (no runner is started).

## Run

```
pcb-dump defaults [--hash]
pcb-dump board [--sections item,bbox,track,pad,shape,text,dim,zone,footprint,misc,board,order]
               [--hash] [--loader | --no-init] FILE...
```

**Copy the inputs to a scratch directory first** (ORACLE.md rule). Settings writes are inhibited
but project loading takes lock files next to the board.

Initialisation is that of `qa/tests/pcbnew/test_module.cpp`: `KI_TEST::SetMockConfigDir()` (config
from `qa/data/config`, `KICAD_INHIBIT_SETTINGS_WRITES=1`), `MOCK_PGM_BASE`, `wxAppConsole`,
`InitPgm( true, true )`, `PCBNEW_SETTINGS`, `LoadProject( "" )`. The tool also forces
`LC_ALL=C.UTF-8` and unsets `DISPLAY`/`WAYLAND_DISPLAY`.

Load paths:

* default: `KI_TEST::LoadBoard` (`qa/pcbnew_utils/board_test_utils.cpp`) minus the Boost.Test
  macros: if `<base>.kicad_pro` (else `<base>.pro`) exists it is loaded with
  `SETTINGS_MANAGER::LoadProject` and bound with `BOARD::SetProject`; the board is parsed by
  `PCB_IO_KICAD_SEXPR_PARSER` via `ReadBoardFromFileOrStream`; then a `DRC_ENGINE` is initialised
  with `<base>.kicad_dru` (or none) and stored in the design settings, followed by
  `BuildListOfNets()`, `BuildConnectivity()`, `SynchronizeTuningProfileProperties()` and, when a
  project is bound, `SynchronizeComponentClasses( {} )`. After each file the empty project is
  loaded again so every file starts from the same state.
* `--no-init`: the parser only (no project, no DRC engine, no connectivity).

Opt-in sections (printed only when listed in `--sections`): `connectivity`
(`dump_connectivity.cpp`) and `engine` (`dump_engine.cpp`: the rules, compiled constraint lists
and provider order of the load path's `DRC_ENGINE`); formats in `PCB_DUMP_FORMAT.md` sections 8
and 9.
* `--loader`: the kicad-cli path (`PCBNEW_JOBS_HANDLER::getBoard`): project
  `<base>.kicad_pro` (created in memory if missing), `BOARD_LOADER::Load` with the plugin type of
  `PCB_IO_MGR::FindPluginTypeFromBoardPath`.

Robustness: load errors (`IO_ERROR` and others) give `error|load|<file>|...` and the next file is
processed (exit code 1 if any load failed). wx assertions while loading are counted (`warn|load|`)
and ignored, as in kicad-cli. While dumping, wx assertions throw (as in the QA tests) and every
field is computed inside a guard (`EMIT`): an exception, assertion or crash signal
(SIGSEGV/SIGBUS/SIGFPE, recovered with `siglongjmp`) is reported on an `error|` line for that
field and dumping continues.

## Extending

* A new field: add `EMIT( "name", V( getter ) );` in the matching `dump_*.cpp` function. Use the
  `V*` formatters of `pcb_dump.h` only, so the format stays uniform; document new composite forms
  in `PCB_DUMP_FORMAT.md`.
* A new section: write `void DumpFoo( BOARD_ITEM* )` (declare it in `pcb_dump.h`), call
  `section( "foo", DumpFoo )` for the item types it applies to in `DumpItemSections`
  (`dump_board.cpp`), add it to `dumpDefaultItem` (`dump_defaults.cpp`) if it should appear for
  default-constructed items, and list it in the format document.
* New source files in this directory are picked up by `build.sh` automatically (`*.cpp`).

## Files

| File | Content |
|---|---|
| `main.cpp` | CLI, headless initialisation, the three load paths |
| `pcb_dump.h`, `format.cpp` | output sink, value formatters, item walk and keys, crash guard |
| `dump_board.cpp` | per-board driver, `item` and `bbox` sections |
| `dump_items.cpp` | `track`, `pad`, `shape`, `text`, `dim`, `zone`, `footprint` |
| `dump_settings.cpp` | design settings, netclass, stackup, plot params, teardrops, zone settings, `board`, `order` |
| `dump_defaults.cpp` | `misc` section, `defaults` command |
| `boost_test_impl.cpp` | header-only Boost.Test implementation for the reused QA sources |

## Rust side (`pcb-dump-rs`) and comparing

`kicad_pcb::dump` (crate feature `dump`, bin `pcb-dump-rs`) prints the Rust board model in the
same format; fields whose Rust code is still a `port_pending!` stub print
`pending|SECTION|KEY|FIELD=CXX::Name` instead of a value (`(no Rust API)` = the getter does not
exist in Rust yet: add it and replace the `no_api(..)` call in `crates/kicad-pcb/src/dump/`),
other panics `rust-error|...`. `compare.py REF RUST [--section S] [--show N] [--summary-json]`
prints match / mismatch / pending / rust-error / missing / extra per section (per class for
`defaults`) and exits 0 only without mismatch / rust-error / missing / extra lines.

Workflow for a package (run cargo from `rust/`, one cargo command at a time):

```
S=$(mktemp -d)                                   # never run the tools on repository files
cp path/to/small.kicad_pcb $S/                   # plus .kicad_pro / .kicad_dru if any
/home/user/kicad-oracle/bin/pcb-dump board --sections pad $S/small.kicad_pcb > $S/ref.txt 2>/dev/null
cargo run -q -p kicad-pcb --features dump --bin pcb-dump-rs -- board --sections pad $S/small.kicad_pcb > $S/rust.txt
python3 tools/pcb-dump/compare.py $S/ref.txt $S/rust.txt --section pad --show 20
```

`board FILE` needs the `.kicad_pcb` parser: until the parser package fills in
`kicad_pcb::dump::load_board_for_dump` it prints `error|load|...`. In unit tests, call
`kicad_pcb::dump::dump_board(&board, &opts)` on a board built in memory and compare with
`kicad_pcb::dump::compare::compare(golden, &rust)` (same rules as `compare.py`).

`defaults` works now: `cargo test -p kicad-pcb --features dump --test pcb_dump_defaults --
--nocapture` compares `pcb-dump-rs defaults` with the committed golden
`crates/kicad-pcb/tests/golden/pcb_dump_defaults.txt` (regenerate with
`pcb-dump defaults`, keep the `#` header) and fails on any mismatch / rust-error / missing /
extra line; a field turning from pending into a wrong value fails it, so run it after porting a
constructor or a getter used there.
