# Board file reader and writer (`.kicad_pcb` / `.kicad_mod`) - porting spec

Analyst document for the port of `pcbnew/pcb_io/kicad_sexpr/` (KiCad master, board file format
`SEXPR_BOARD_FILE_VERSION 20261031`). Everything below was verified against the C++ tree in this
repository; line numbers refer to the files as checked in. Abbreviations used throughout:

- **P** = `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (10757 lines)
- **W** = `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` (4332 lines)
- **H** = `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h` (590 lines, holds the version history table)
- "oracle" = reference `kicad-cli` built from this tree (`/home/user/kicad-oracle/bin/kicad-cli`).

Read sections 1, 4, 6, 7 and 11 before touching any work package; sections 2, 3, 5 are lookup
tables; section 10 is the work breakdown.

---------------------------------------------------------------------------------------------------

## 1. Purpose and entry points

The subsystem turns the text of a `.kicad_pcb` (board) or `.kicad_mod` (footprint library file)
into the board model, and the board model back into text. Every other feature of the port
(connectivity, DRC, zone fill, plotting) receives its input through it.

### 1.1 Callers in the C++

| Caller | Path | What it does |
|---|---|---|
| every `kicad-cli pcb ...` job | `kicad/cli/command_pcb_*.cpp` -> `JOB_*` (`common/jobs/`) -> `PCBNEW_JOBS_HANDLER::getBoard` (`pcbnew/pcbnew_jobs_handler.cpp:540-630`) -> `BOARD_LOADER::Load` (`pcbnew/board_loader.cpp:43-88`) -> `PCB_IO_MGR::Load` (`pcbnew/pcb_io/pcb_io_mgr.cpp:210`) -> `PCB_IO::LoadBoard` (`pcbnew/pcb_io/pcb_io.cpp:72-83`) -> `PCB_IO_KICAD_SEXPR::loadBoard` (W:3809) -> `DoLoad` (W:3836) -> `PCB_IO_KICAD_SEXPR_PARSER::Parse` (P:1088) | load + post-load initialisation (section 4.3) |
| `kicad-cli pcb upgrade [--force] INPUT` | `kicad/cli/command_pcb_upgrade.cpp` -> `JOB_PCB_UPGRADE` (`m_filename`, `m_force`, default `false`) -> `PCBNEW_JOBS_HANDLER::JobUpgrade` (`pcbnew_jobs_handler.cpp:3308-3352`) | `getBoard()`, then save **in place** with `PCB_IO_KICAD_SEXPR::SaveBoard` if `GetFileFormatVersionAtLoad() < SEXPR_BOARD_FILE_VERSION` or `--force` or `RefreshGeneratedTables(*brd) > 0`. Otherwise prints "Board file was not updated" (as an error-severity message, exit code still success). No output-file option. |
| `kicad-cli fp upgrade INPUT_DIR [-o OUT_DIR] [--force]` | `command_fp_upgrade.cpp` -> `JOB_FP_UPGRADE` -> `JobExportFpUpgrade` (`pcbnew_jobs_handler.cpp:2659-2748`) | `PCB_IO_KICAD_SEXPR pcb_io( CTL_FOR_LIBRARY ); FP_CACHE fpLib; fpLib.Load(); fpLib.Save();` - a **pure parse + write** of every `*.kicad_mod` in the directory (no connectivity, no project). Saves when any footprint's version is older than `SEXPR_FOOTPRINT_LIB_FILE_VERSION` or `--force`. |
| `kicad-cli pcb drc --save-board` | `JobExportDrc` (`pcbnew_jobs_handler.cpp:2852`, save at 3067) | `BOARD_LOADER::SaveBoard` (`board_loader.cpp:184-209`): `BuildConnectivity`, `SynchronizeNetsAndNetClasses(false)`, `SynchronizeProperties`, `RefreshGeneratedTables`, then `PCB_IO_MGR::Save`. |
| QA helpers | `qa/pcbnew_utils/board_file_utils.cpp` | `ReadBoardFromFileOrStream` = bare `PCB_IO_KICAD_SEXPR_PARSER( reader, nullptr, nullptr ).Parse()` (parser creates the `BOARD`); `DumpBoardToFile` = `PCB_IO_KICAD_SEXPR io; io.SaveBoard(...)`. `KI_TEST::LoadBoard` (`board_test_utils.cpp:90`) adds project load, `SetProject`, DRC engine, `BuildListOfNets`, `BuildConnectivity`. |
| GUI | `pcbnew/files.cpp` (load at ~700-905), clipboard (`PCB_IO_KICAD_SEXPR::Parse`, W:333), footprint editor, autosave (`FormatBoardToFormatter`) | OUT of scope, but note the GUI runs extra fix-ups (`repairBoard`, `rehatchShapes`, `inferLegacyEdgeClearance`) that the CLI path does **not** run. |

There is **no** kicad-cli command that performs a bare parse + write of a board: `pcb upgrade` goes
through `BOARD_LOADER::Load`, i.e. project binding and `BuildConnectivity()` (which propagates
nets, section 4.3). `fp upgrade` is the only pure I/O oracle. Section 9.3 builds the differential
tests around this.

### 1.2 Options and their defaults

- `PCB_IO_KICAD_SEXPR( int aControlFlags = CTL_FOR_BOARD )` (H:444). `CTL_*` bits are in
  `include/ctl_flags.h`; the combinations are (H:256-264):
  - `CTL_FOR_BOARD = CTL_OMIT_INITIAL_COMMENTS | CTL_OMIT_FOOTPRINT_VERSION`
  - `CTL_FOR_LIBRARY = CTL_OMIT_PAD_NETS | CTL_OMIT_UUIDS | CTL_OMIT_PATH | CTL_OMIT_AT | CTL_OMIT_LIBNAME`
  - `CTL_FOR_CLIPBOARD = CTL_OMIT_INITIAL_COMMENTS`
- Load properties (`std::map<std::string, UTF8>`): only
  `PCB_IO_LOAD_PROPERTIES::APPEND_PRESERVE_DESTINATION_STACKUP` (W:3842) and, for footprint saves,
  `"skip_cache_validation"` (W:4123). Both irrelevant for a plain load.
- Parser constructor (`pcb_io_kicad_sexpr_parser.h:107`): `aBoard` (null => parser creates one for
  `kicad_pcb`, footprints parse without a board), `aQueryUserCallback` (null in CLI/QA),
  `aProgressReporter = nullptr`, `aLineCount = 0`, `aPreserveDestinationStackup = false`,
  `aAppendToExisting = false`.
- Output prettifier mode: `KICAD_FORMAT::FORMAT_MODE::NORMAL`; switches to
  `COMPACT_TEXT_PROPERTIES` only when `ADVANCED_CFG::m_CompactSave` is set (default `false`,
  `common/advanced_config.cpp:287`; `common/richio.cpp:695-704`). The port needs `NORMAL` for
  oracle equality and may offer the compact mode as an option.
- Header strings: generator `"pcbnew"` (board) / `"kicad_footprint_editor"` (footprint library
  file), `generator_version` = `GetMajorMinorVersion()` = `KICAD_MAJOR_MINOR_VERSION`
  (`common/build_version.cpp:117`; the tree's `cmake/KiCadVersion.cmake` says `10.99.0-unknown`, so
  `"10.99"`). Make it a constant with that default and overridable by the caller, and confirm
  against the first oracle output.

---------------------------------------------------------------------------------------------------

## 2. File inventory

### 2.1 In scope (port)

| File | Lines | Role |
|---|---:|---|
| `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` | 10757 | Recursive-descent parser over the `PCB_LEXER` token stream. One `parseXXX` per construct. |
| `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.h` | 581 | Parser class, `GROUP_INFO` / `GENERATOR_INFO` / `CONSTRAINT_INFO`, inline `parseInt`/`parseHex`/`getNetCode`. |
| `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` | 4332 | Writer (`format(...)` overloads, W:283-3790), load entry (`loadBoard`/`DoLoad`, W:3809-3944), footprint-library API and `FP_CACHE` (W:80-260, 3947-4333). |
| `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h` | 590 | Version history table (H:70-228), version boundary constants (H:230-243), `CTL_FOR_*`. |
| `common/pcb.keywords` | 507 | Keyword list from which `PCB_LEXER` / `PCB_KEYS_T::T` is generated (one keyword per line, sorted; token id = index). |
| `pcbnew/pcb_plot_params.cpp` (+ `common/pcb_plot_params.keywords`, 55) | 899 | `(pcbplotparams ...)` formatter (`Format`, 133-203) and its own parser (`PCB_PLOT_PARAMS_PARSER::Parse`, 551-830) incl. legacy layer-bitmask remap (474-548). |
| `pcbnew/board_stackup_manager/board_stackup.cpp` | 1050 | `BOARD_STACKUP::FormatBoardStackup` (821, writer half of `(stackup ...)`), `BuildDefaultStackupList` (699). |
| `common/embedded_files.cpp` (+ `common/embedded_files.keywords`, 12) | 748 | `ClearEmbeddedFonts` (197), `WriteEmbeddedFiles` (210), `DecompressAndDecode` (306), `EMBEDDED_FILES_PARSER::ParseEmbedded` (440-600). |
| `common/stroke_params.cpp` (+ `stroke_params.keywords`, 9), `common/line_ending.cpp` | 572, 336 | `(stroke ...)` parser/formatter, `(start_shape ...)`/`(end_shape ...)` formatter (`LINE_ENDING::Format`, 301). |
| `common/eda_text.cpp` (`EDA_TEXT::Format` 1081-1137, `ResolveFont` 480, render cache 684-745, `MigrateLegacyBoldStrokeWidth` 333) | 1467 | `(effects ...)` writer and text state the parser sets. |
| `common/page_info.cpp` (`Format` 234), `common/title_block.cpp` (`Format` 28) | 257, 200 | `(paper ...)`, `(title_block ...)` formatters. |
| `common/io/kicad/kicad_io_utils.cpp` | 397 | `FormatBool`, `FormatOptBool`, `FormatUuid`, `FormatCustomProperties`, `FormatStreamData`, **`Prettify`** (belongs to `kicad-sexpr`; listed because the output bytes depend on it). |
| `common/richio.cpp` (`OUTPUTFORMATTER::Print/Quotes/Quotew` 432-512, `PRETTIFIED_FILE_OUTPUTFORMATTER` 695-735) | 736 | Output plumbing (belongs to `kicad-sexpr`). |
| `common/dsnlexer.cpp`, `include/dsnlexer.h` | 885, 597 | Lexer (belongs to `kicad-sexpr`); the parser depends on its exact token classification (section 5.1). |
| `common/kiid.cpp` | 443 | UUID parsing incl. legacy time stamps (belongs to `kicad-core`). |
| `pcbnew/pcb_io/pcb_io.cpp`, `pcb_io.h`, `pcb_io_mgr.cpp`, `pcb_io_mgr.h` | 217, 408, 449, 259 | Plugin base (`LoadBoard` creates the `BOARD`, sets file name) and registry. Only the KiCad s-expression path is ported; the registry collapses to direct calls. |
| `pcbnew/board_loader.cpp`, `board_loader.h` | 215, 83 | Post-load initialisation and pre-save steps (section 4.3 / 4.5). |
| `pcbnew/netinfo_list.cpp` (`AppendNet`, `getFreeNetCode`, `buildListOfNets`) | 389 | Net-code assignment; determines track output order (section 4.6). |
| `pcbnew/pcb_generator.cpp` 150-185, `pcbnew/generators/pcb_tuning_pattern.cpp` 1941-2060, `pcb_via_stack.cpp` 989-1060, `pcb_via_stitch.cpp` 609-700 | - | `GetProperties`/`SetProperties` of the three generator types - needed for byte-exact `(generated ...)` round trip (tier 2). |
| `qa/pcbnew_utils/board_file_utils.cpp`, `board_test_utils.cpp` | 188, 877 | Test helpers to mirror in `kicad-qa`. |

### 2.2 Out of scope (do not port; record in `PORT_STATUS.md`)

| File / class | Why |
|---|---|
| `pcbnew/pcb_io/kicad_sexpr/writers/pcb_writer_v9.cpp` (2392), `pcb_writer_v10.cpp` (2727), `legacy_item_order.h` (159), `pcbnew/downgrade/board_downgrade.cpp` | Frozen writers for `kicad-cli pcb downgrade` (save in KiCad 9 / 10 format). Not a core feature. Useful only as documentation of what old writers emitted. |
| `FP_CACHE` directory scanning, time stamps, `FootprintEnumerate/Delete/CreateLibrary/DeleteLibrary/IsLibraryWritable` (W:80-260, 3947-4333) | File-system and cache management; library crates do no file I/O (PORTING.md section 5). Keep only the pure functions: parse a `.kicad_mod` text, format a footprint library file, and the `FootprintSave` normalisation (W:4176-4202). |
| Append/paste mode (`m_appendToExisting`, `remapAppendedLayers` P:2711-2806, `revertPartialAppend` W:3862-3896, `m_resetKIIDMap`) | Editor feature ("append board", design blocks). Tier 3. Keep the `append` flag plumbed so the code shape matches, return an error if set until ported. |
| Undefined-layer rescue dialog branch (P:1639-1725, `Pgm().IsGUI() && m_queryUserCallback`) | GUI only. Headless behaviour is the `else` branch: `IO_ERROR` "One or more items were found on undefined layers (%s). Open the board in the PCB Editor to resolve." (P:1728). Port the error; optionally offer the rescue as an explicit load option. |
| `checkpoint()` progress reporting (P:151-170), `FONTCONFIG_REPORTER_SCOPE`, `LOAD_INFO_REPORTER` singleton | UI plumbing; wall clock. Parse warnings become a returned `Vec<String>`. |
| `PCB_IO_KICAD_SEXPR::Parse( clipboard )` (W:333), `SetQueryUserCallback`, `GroupsSanityCheck` prompt in `SaveBoard` (W:286-298) | GUI. `SaveBoard` in headless mode has no callback, so the sanity result is ignored. |
| `BOARD::EmbedFonts()` (called from `FormatBoardToFormatter`, W:315) and outline-font render-cache **regeneration** (`formatRenderCache`, W:573) | Need fontconfig/FreeType/HarfBuzz. See section 11 (R7) for the required fallback. |
| Other importers (`pcbnew/pcb_io/altium`, `eagle`, ..., `kicad_legacy`) | Not core. Legacy `.brd` files are not supported; say so in the error. |

---------------------------------------------------------------------------------------------------

## 3. Data structures

### 3.1 Parser state (`PCB_IO_KICAD_SEXPR_PARSER`, header lines 548-577)

| Member | Type | Meaning / lifetime |
|---|---|---|
| `m_board` | `BOARD*` | Target board; null while parsing a library footprint. Many branches test it (`parseNet`, `parsePAD` net, `StyleFromSettings`, component classes). |
| `m_layerIndices` | `unordered_map<string, PCB_LAYER_ID>` | Layer name -> id. Pre-seeded in `init()` (P:104-148) with all 128 canonical English names (`LSET::Name`); board `(layers ...)` adds user/translated names. Lookup only, never iterated -> `HashMap` is fine. |
| `m_layerMasks` | `unordered_map<string, LSET>` | Same plus wildcards `*.Cu` (all 32 copper layers, not just the board's), `*In.Cu`, `F&B.Cu`, `*.Adhes`, `*.Paste`, `*.Mask`, `*.SilkS`, `*.Fab`, `*.CrtYd`, and `Inner1.Cu`..`Inner14.Cu` -> `PCB_LAYER_ID( In15_Cu - 2*i )`. |
| `m_undefinedLayers` | `std::set<wxString>` | Names not found by `lookUpLayer` (ordered -> the error message lists them sorted). |
| `m_netCodes` | `vector<int>` | Legacy file net code -> board net code (`pushValueIntoMap`, `getNetCode`). |
| `m_requiredVersion`, `m_tooRecent`, `m_generatorVersion` | `int`, `bool`, `wxString` | File version (0 until a `(version)` is seen for footprints; boards default to 20201115 only in a code path that in practice errors out, see 7.1). |
| `m_appendToExisting`, `m_preserveDestinationStackup`, `m_layerMappingHandler`, `m_resetKIIDMap`, `m_linkedZones` | | Append mode (tier 3) except `m_linkedZones` (`std::set<KIID>`, zones with a link set/anchor, used in post-processing P:1750-1765). |
| `m_showLegacySegmentZoneWarning`, `m_showLegacy5ZoneWarning` | `bool` | One-shot warning flags. |
| `m_groupInfos`, `m_generatorInfos`, `m_constraintInfos` | `vector<...>` | Deferred group / generator / constraint declarations, resolved in `finishItem` (P:1138) in **vector order** (groups first, then generators). Cleared after every top-level item (important for multi-footprint library files). |
| `m_parseWarnings` | `vector<wxString>` | Non-fatal warnings (embedded-file parse errors, zero-sized pads, legacy zone fills). |

`GROUP_INFO` = parent, name, locked, uuid, libId, member uuids (file order), custom properties.
`GENERATOR_INFO` adds layer, type string, `STRING_ANY_MAP properties`, named template items
(owned `BOARD_ITEM`s, currently only vias). `CONSTRAINT_INFO` = parent, uuid, type, members
(`KIID`, anchor, index), optional value, driving flag, custom properties.

### 3.2 Writer state (`PCB_IO_KICAD_SEXPR`, H:576-587)

`m_ctl` (control bits), `m_board` (set in `FormatBoardToFormatter` *after* `init()`; null when
formatting a library footprint - several branches depend on it: `formatLayers` copper mask,
pad `zone_layer_connections`, custom padstack layer enumeration, group member validation),
`m_out` (`OUTPUTFORMATTER*`). No caches. All ordering is done with temporary `std::set`s built
per call (section 4.6).

### 3.3 Board-side structures whose exact semantics the I/O relies on

- `NETINFO_LIST` (`pcbnew/netinfo.h:225-226`): `m_netNames` = `std::map<wxString, NETINFO_ITEM*>`
  (iteration of `board->GetNetInfo()` is **by name**), `m_netCodes` = `std::map<int, ...>`.
  Net 0 (`""`) always exists.
- `FOOTPRINT::m_fields` = `std::deque<PCB_FIELD*>`; constructor adds Reference (F.SilkS,
  visible), Value (F.Fab, visible), Datasheet (F.Fab, hidden), Description (F.Fab, hidden) in
  that order (`pcbnew/footprint.cpp:181-192`); the writer iterates the deque in insertion order.
- `FOOTPRINT::m_variants` = `CASE_INSENSITIVE_MAP<FOOTPRINT_VARIANT>` (case-insensitive key
  order); variant fields `std::map<wxString, wxString>`.
- `BOARD::m_properties`, `EDA_ITEM::m_customProperties`, `BOARD_DESIGN_SETTINGS::m_ZoneLayerProperties`,
  `ZONE::LayerProperties()`: all `std::map` (ordered by key / by `PCB_LAYER_ID` value).
- `EMBEDDED_FILES::m_files`: `std::map<wxString, EMBEDDED_FILE*>` (by file name).
- `STRING_ANY_MAP` (`include/string_any_map.h`): `std::map<std::string, wxAny>`, generator
  properties are written in key order.
- `PCB_GENERATED_TABLE::RowKeys()` `std::map<int, std::string>`, `PCB_DRILL_CHART::RowShapes()`
  `std::map<int,int>`, `DRILL_SYMBOL_PROFILE::GroupKeys()` `std::set<DRILL_GROUP_KEY>`,
  `Assignments()` `std::map<std::string, ...>`.
- `JUMPER_GROUP_SET` / `JUMPER_GROUP`: groups of pad names (`std::set<wxString>` inside).

All of these are ordered containers, so `BTreeMap`/`BTreeSet` with the same key ordering
reproduces the output. `wxString operator<` on Linux compares UTF-32 code points, which equals
UTF-8 byte order, so `String` keys are fine.

---------------------------------------------------------------------------------------------------

## 4. Control flow and algorithms

### 4.1 Load, in execution order

1. `PCB_IO::LoadBoard` (`pcb_io.cpp:72`): `new BOARD`, `SetFileName`, `loadBoard(..., aIsNewLoad = true)`.
2. `PCB_IO_KICAD_SEXPR::DoLoad` (W:3836-3936): constructs the parser; on `PARSE_ERROR` with
   `parser.IsTooRecent()` rethrows as `FUTURE_FORMAT_ERROR( parse_error, GetRequiredVersion() )`;
   if the parsed item is not the board: `PARSE_ERROR` "This file does not contain a PCB.";
   finally copies `GetParseWarnings()` to the load reporter.
3. `Parse()` (P:1088-1135): `ReadCommentLines()` (leading `#` lines, kept only for footprints),
   expect `(`; then `kicad_pcb` -> `parseBOARD()`, `module`/`footprint` -> `parseFOOTPRINT()` +
   `SetLocked(false)`; anything else `PARSE_ERROR` "Unknown token '%s'". Then `finishItem()`.
4. `parseBOARD_unchecked()` (P:1293-1795):
   1. `parseHeader()` (P:1940): `(version N)` -> `m_requiredVersion`, `m_tooRecent`,
      `SetKnowsBar( version >= 20240706 )` (`|` becomes a separator), `SetFileFormatVersionAtLoad`.
   2. Token loop (P:1313-1598). Items are added with `m_board->Add( item, ADD_MODE::BULK_APPEND, true )`
      in **file order** (containers are `push_back`, see `BOARD::Add`, `pcbnew/board.cpp:1520`).
      Version gate `checkVersion()` (throws `FUTURE_FORMAT_ERROR(version, generatorVersion)` when
      `m_requiredVersion > SEXPR_BOARD_FILE_VERSION`) runs at `generator_version` and at `general`.
      Special cases: `segment`/`arc` returning null are skipped (non-copper layer); a `zone` with
      `GetNumCorners() == 0` is silently deleted (P:1515); groups, generators and constraints are
      only recorded (deferred); `(property k v)` is collected into a local `std::map` (so a
      duplicate key keeps the **first** value: `map::insert`).
   3. `FinalizeBulkAdd`, `SetProperties( properties )`.
   4. Re-run `AssembleBarcode()` for every barcode (board drawings, then footprint graphics).
   5. Undefined layers (P:1624-1732): headless => `IO_ERROR` (see 2.2).
   6. For every zone: `SetLayerSetAndRemoveUnusedFills( zoneLayers & enabledLayers )` (P:1735-1744).
   7. Linked-zone anchor remap (only does something in append mode), `FixupEmbeddedData()`
      (footprint embedded-file entries get the data stored at board level, P:1768),
      `net->ResolveTerminalPads()` for every net, `RefreshDrillSymbolLayers()`.
5. `finishItem()` (P:1138-1157): `GetEmbeddedFiles()->UpdateFontFiles()`; for every `EDA_TEXT`
   child (recursive) `ResolveFont( embeddedFonts )`; `resolveGroups()` (P:1798-1937);
   `resolveConstraints()` (P:8678-8762); clear the deferred lists.
6. `resolveGroups`: pass 1 creates every group/generator object in order (`PCB_GROUP`, or
   `GENERATORS_MGR::CreateFromType( genType )` - unknown type => `IO_ERROR` "Cannot create
   generated object of type '%s'"), sets uuid, custom properties, lib id, locked, and adds it to
   its parent with `ADD_MODE::INSERT`. Pass 2 resolves member uuids (`BOARD::ResolveItem` or a
   per-footprint map) and calls `AddItem` only when the item exists **and**
   `item->GetParentFootprint() == group->GetParentFootprint()`. For generators with
   `LayerFollowsMembers()`, the layer is taken from the first `PCB_TRACK` in `GetBoardItems()`
   (an unordered set in C++; only done when all members share a layer, so order is harmless).
   Finally `m_board->GroupsSanityCheck( true )` (repair mode; breaks cycles).

### 4.2 Parser helper semantics that affect results

- `parseBoardUnits()` (P:206-233): `KiROUND( clamp( parseDouble() * 1e6, -INT_LIMIT, INT_LIMIT ) )`
  with `INT_LIMIT = INT_MAX - 10` as a `double` (P:99). The data-type overload uses
  `GetScaleForInternalUnitType` (`TIME` -> `IU_PER_PS`, `LENGTH_DELAY` -> `IU_PER_PS_PER_MM`,
  else `IU_PER_MM`; `common/eda_units.cpp:233`).
- `parseDouble()` is `DSNLEXER::parseDouble` (`common/dsnlexer.cpp:868`): `fast_float::from_chars`
  with `skip_white_space | general`; the result pointer is **not** checked, so a valid prefix is
  accepted; failure => `PARSE_ERROR` "Invalid floating point number".
- `parseInt()` = `strtol( CurText(), nullptr, 10 )` (prefix parse, 0 on garbage, `long` -> `int`
  truncation); `parseHex()` = `NextTok(); strtol( ..., 16 )`.
- `parseBool()` accepts only `yes`/`no`. `parseMaybeAbsentBool( def )` (P:271-299): if the
  previous token was `(`, accepts `)` (=> `def`), `yes`/`true`, `no`/`false` and consumes the
  closing `)`; if the keyword was bare (previous token not `(`) returns `def` without consuming
  anything. This one function implements all "old bare flag vs. new `(flag yes)`" compatibility.
- `parseOptBool()`: `yes`/`no`/`none`. `parseFrontBackOptBool( allowLegacy )` (P:9463-9520):
  new form `(tenting (front yes) (back none))`; legacy form `(tenting front back)` /
  `(tenting none)`.
- `lookUpLayer` (P:2820): unknown name => inserts into `m_undefinedLayers` and returns `Rescue`;
  a name that maps to `Rescue` is also recorded. `lookUpLayerSet` (P:2809): unknown name returns
  `LSET{Rescue}` **without** recording it (so an unknown name inside `(layers ...)` does not
  trigger the undefined-layer error; the item silently lands on `Rescue`).
- `parseLayersForCuItemWithSoldermask` (P:2873): exactly one copper layer, at most one mask
  layer, mask only on outer layers and on the same side - otherwise `Expecting(...)` errors.
- `parseNet` (P:302-348): number token => legacy net code via `getNetCode`, clamped `max(0, ..)`,
  `SetNetCode(code, aNoAssert=true)`; otherwise a symbol => net **name**; `FindNet(name)` or
  create `NETINFO_ITEM( board, name )` and `Add( net, INSERT, true )` (net code assigned by
  `AppendNet`, section 4.6). Without a board the name is ignored.
- `CurStrToKIID` (P:10737): `KIID( string )` (legacy 8-hex time stamps and malformed strings, see
  7.3) - or a fresh random `KIID` in append mode.
- `parseOutlinePoints` (P:395): `(xy x y)` -> `SHAPE_LINE_CHAIN::Append( x, y )`;
  `(arc (start ..) (mid ..) (end ..))` in any sub-token order (all three required) ->
  `Append( SHAPE_ARC( start, mid, end, 0 ) )`.
- `skipCurrent` (P:173): skip to the `)` closing the current list.

### 4.3 Post-load initialisation outside the parser (`BOARD_LOADER::initializeLoadedBoard`, `board_loader.cpp:99-170`)

Runs for every kicad-cli job. In order: drawing sheet load (ignore), layer enum map (GUI
property system, ignore), **`SetProject`** (`pcbnew/board.cpp:379`, `BOARD::SetProject`: binds
`BOARD_DESIGN_SETTINGS` to the project file - loading the JSON values unless
`m_LegacyDesignSettingsLoaded`; if `m_LegacyNetclassesLoaded`, the netclasses parsed from the board
are moved into the project's `NET_SETTINGS`), DRC engine creation + `InitEngine( <board>.kicad_dru )`,
`ResolveDRCExclusions` (markers, not saved), **`BuildConnectivity()`**, **`BuildListOfNets()`**
(`NETINFO_LIST::buildListOfNets`, `netinfo_list.cpp:187`: clears per-net derived data but keeps
chain names/terminal pads, then `SynchronizeNetsAndNetClasses(false)` and
`SetAreasNetCodesFromNetNames()`), `SynchronizeNetsAndNetClasses( true )`,
`SynchronizeComponentClasses`, `SynchronizeTuningProfileProperties`, `UpdateUserUnits`.

Effects on a subsequent save (what makes `pcb upgrade` differ from a bare parse + write):

- `BuildConnectivity` -> `CONNECTIVITY_DATA::Build` -> `internalRecalculateRatsnest` ->
  `CN_CONNECTIVITY_ALGO::PropagateNets` -> `propagateConnections`
  (`pcbnew/connectivity/connectivity_algo.cpp:685`): in every non-conflicting cluster with a valid
  net, items that `CanChangeNet()` (tracks, vias, zones, graphic copper) get the cluster's origin
  net. Boards whose tracks carry a different net than the pad they touch are **rewritten** on
  `pcb upgrade`; this also changes the track sort order (net code is the first sort key).
- `SetProject`: a sibling `.kicad_pro` may change board-file settings through the
  `BOARD_DESIGN_SETTINGS` schema migration 1->2 (`pcbnew/board_design_settings.cpp:1150-1180`:
  `rules.solder_mask_clearance` etc. are moved "back to the board"), and supplies netclasses
  (used by `via->GetDrillValue()` for vias with undefined drill) and text variables (used for
  the render-cache key and dimension/barcode text).
- `JobUpgrade` additionally calls `RefreshGeneratedTables` (drill charts / stackup tables are
  re-derived).

Where this glue lives in the Rust workspace is an open question (section 11, Q1): it needs
`kicad-connectivity`, which sits above `kicad-pcb`.

### 4.4 Save, in execution order

1. `SaveBoard` (W:283): `GroupsSanityCheck()` (result only used with a GUI callback), create
   `PRETTIFIED_FILE_OUTPUTFORMATTER`, `FormatBoardToFormatter`, `Finish()`.
2. `FormatBoardToFormatter` (W:306-330): `init()`; `m_board = aBoard`; **mutates the board**:
   `EmbedFonts()` if `GetAreFontsEmbedded()` else `GetEmbeddedFiles()->ClearEmbeddedFonts()`
   (removes embedded files of type font); prints
   `(kicad_pcb (version 20261031) (generator "pcbnew") (generator_version "10.99")`, then
   `Format( board )`, then `)`.
3. `format( const BOARD* )` (W:908-1064): build the sorted sets (4.6); `formatHeader`
   (`formatGeneral` + page + title block, `formatBoardLayers`, `formatSetup`,
   `formatProperties`, `formatVariants`); footprints; drawings; points; tracks; zones; groups;
   generators; constraints; `(net_chains ...)` (W:962-1038); `(embedded_fonts yes|no)`;
   `(embedded_files ...)` built from the board's files plus every footprint's files merged
   into one name-keyed map (W:1040-1063), written **with data** (`m_ctl & CTL_FOR_BOARD`).
4. `Finish()` (`richio.cpp:710`): `KICAD_FORMAT::Prettify( buffer, mode )`, write, atomic rename.

### 4.5 `BOARD_LOADER::SaveBoard` pre-steps (`board_loader.cpp:184`)

`BuildConnectivity()`, `SynchronizeNetsAndNetClasses( false )`, `SynchronizeProperties()` (board
properties := project text variables, only when a real project is attached),
`RefreshGeneratedTables`. Not used by `pcb upgrade`, used by `pcb drc --save-board`.

### 4.6 Ordering rules (every place where order reaches the output)

All orderings are `std::set` with a comparator; the last tie-break is always the object address
(`a < b`), reachable only when two items have equal uuids. The port must replace the address
tie-break by **insertion index** (document as a deviation; it only matters for corrupt files).

| Where | Comparator | Keys in order |
|---|---|---|
| Board footprints, zones, groups, generators, constraints (W:910-925) | `BOARD_ITEM::ptr_cmp` (`pcbnew/board_item.cpp:427`) | `Type()`; `GetLayerSet().Seq()` compared as vectors (lexicographic, `Seq()` = ascending layer id); `m_Uuid`. |
| Board drawings (W:912) | `BOARD::cmp_drawings` (`pcbnew/board.cpp:4041`) | `Type()`; `GetLayer()` (only for single-layer types); then per type `PCB_SHAPE::Compare`, `PCB_TEXT::Compare`, `PCB_SHAPE::Compare`+`EDA_TEXT::Compare` (text box), `PCB_TABLE::Compare`, `PCB_BARCODE::Compare`; then `m_Uuid`. |
| Board tracks (W:914) | `PCB_TRACK::cmp_tracks` (`pcbnew/pcb_track.cpp:3227`) | **`GetNetCode()`**; `GetLayer()`; `Type()` (trace < via < arc); `m_Uuid`. |
| Points (board and footprint) | `PCB_POINT::cmp_points` (`pcbnew/pcb_point.cpp:100`) | layer; position x; y; size; uuid. |
| Footprint pads (W:1710) | `FOOTPRINT::cmp_pads` (`pcbnew/footprint.cpp:4836`) | number via `StrNumCmp`; FP-relative position (x, then y); per unique padstack layer (of the more complex of the two padstacks; the last differing layer wins): size x, size y, shape; `GetLayerSet().Seq()`; uuid. |
| Footprint graphics (W:1712) | `FOOTPRINT::cmp_drawings` (`footprint.cpp:4702`) | `Type()`; `GetLayer()`; shapes: `GetShape()`, library start, library end (both skipped for polygons), arc mid / bezier controls / polygon vertex count + vertices / ellipse parameters, width; texts: FP-relative position, angle, size, thickness, bold, italic, mirrored, line spacing, text (`wxString::Cmp`); uuid. |
| Footprint zones | `FOOTPRINT::cmp_zones` (`footprint.cpp:4937`) | assigned priority; layer set `Seq()`; library outline vertex count, vertices; uuid. |
| Footprint groups, constraints | `PCB_GROUP::ptr_cmp` | (same as `BOARD_ITEM::ptr_cmp`). |
| Group / generator members (W:3039, 3227) | `wxArrayString::Sort()` of uuid strings | plain string order of the lower-case uuid text. |
| Net chains (W:970-975) | `ValueStringCompare( a, b ) < 0` on chain names | members inside a chain are in `GetNetInfo()` (net **name**) order. |
| Layer lists | `formatBoardLayers` (W:765): `GetEnabledLayers().CuStack()` (F.Cu, In1..InN, B.Cu) then `TechAndUserUIOrder()` (`common/lset.cpp:272`: F.Adhes, B.Adhes, F.Paste, B.Paste, F.SilkS, B.SilkS, F.Mask, B.Mask, Dwgs.User, Cmts.User, Eco1.User, Eco2.User, Edge.Cuts, Margin, F.CrtYd, B.CrtYd, F.Fab, B.Fab, then User.N ascending); `formatLayers` (W:1887): wildcards in fixed order then remaining layers by ascending id. |
| `KICAD_T` values | `include/core/typeinfo.h` | The numeric enum order is part of the output order (e.g. `PCB_SHAPE_T` < `PCB_REFERENCE_IMAGE_T` < `PCB_FIELD_T` < `PCB_GENERATOR_T` < `PCB_TEXT_T` < `PCB_TEXTBOX_T` < `PCB_TABLE_T` < ... < `PCB_BARCODE_T` < dimension types < `PCB_TARGET_T` < `PCB_ZONE_T` ..., and the late additions `PCB_CONSTRAINT_T`, `PCB_GRID_ITEM_T`, `PCB_DRILL_CHART_T`, `PCB_DRILL_MAP_T`, `PCB_3D_BODY_T`, `PCB_STACKUP_TABLE_T` at the very end). Give the Rust enum the same explicit discriminants. |
| `KIID` `<` | `boost::uuids::uuid` `operator<` | byte-wise lexicographic on the 16 bytes (= order of the canonical hex string). |

**Net codes** (first key of `cmp_tracks`) are assigned by `NETINFO_LIST::AppendNet`
(`netinfo_list.cpp:154`): a new net keeps its requested code only if it equals the current net
count, otherwise `getFreeNetCode()` (`netinfo_list.cpp:308`: pre-increments the persistent
counter `m_newNetCode` until an unused code is found; a duplicate name is rejected and the
existing net is used). Hence:

- files >= 20251028 (no `(net N "name")` declarations): codes follow the **first occurrence of
  each net name in file order** (footprint pads first, because footprints are written first);
- older files: `parseNETINFO_ITEM` (P:3619) calls `FindOrAddNet( name, code )` in declaration
  order and records the mapping file code -> actual code;
- consequence: re-saving a legacy file and re-saving the result again can order tracks
  differently (codes change once the declarations are gone). This is C++ behaviour, not a bug of
  the port.

### 4.7 Derived data recomputed during parse (must match because it is written back)

- `PCB_DIMENSION_BASE::Update()` at the end of `parseDIMENSION` (P:6228): recomputes the
  dimension text (unless overridden) and, depending on the text-position mode, the text
  position/angle. The `(gr_text ...)` inside a `(dimension ...)` in the output is this recomputed
  state. Needs `pcbnew/pcb_dimension.cpp` ported faithfully in the model.
- Footprint children hold two representations: library-frame values (what the file stores)
  and baked board-frame values. At load: shapes via `OverrideLibCoords`/`OverrideLibPoly`/
  `OverrideLibBezier` + `RebakeFromLib` (P:4424-4453); text boxes via `bakeTextBoxLib` (P:4972);
  pads via `SetLibSize`/`SetFPRelativePosition`/`SetLibOffset`/`SetLibDrillSize`; points via
  `FOOTPRINT::Add`. At save the writer is **not uniform**:
  - stored library values are written verbatim for line/rect/circle/arc/ellipse shapes
    (`GetLibraryStart()` etc., W:1190-1265), pad size/offset (`Padstack().Size( F_Cu )`,
    W:2049, 2085), points (`GetLibraryPosition()`, W:1375), text-box corners
    (`GetLibraryStart/End`, `GetLibPoly`, W:2712-2728), footprint zone outlines (`ZONE::Outline()`
    of a footprint zone *is* the library outline, `pcbnew/zone.cpp:947`);
  - board-frame values are **un-baked** with `GetTransform().InverseApply(...)` for pad
    positions (`GetFPRelativePosition()`, W:2042, `pcbnew/board_item.cpp:462`), `fp_poly`
    vertices (`formatPolyPts( outline, parentFP )`, W:1227), `fp_curve` points (W:1237-1242),
    footprint text positions (W:2639); drill sizes are divided by the footprint scale
    (`unbakeSize`/`unbakeSizeUniform`, W:498-516), stroke widths by the mean scale
    (`unbakeLinear`, W:519); text-box angle is `GetTextAngle() - fpOrientation` normalised with
    `Normalize720` (W:2743-2749); pad and text angles are written as absolute board angles.
  Every bake/un-bake pair must use the exact `TRANSFORM_TRS` arithmetic
  (`libs/kimath/include/geometry/transform_trs.h`), otherwise coordinates of rotated or scaled
  footprints drift by 1 nm relative to the oracle.
- Legacy footprint text (P:4794-4807): `SetTextAngle( angle - fpOrientation )`,
  `Rotate( {0,0}, fpOrientation )`, `Move( fpPosition )` (only if an `(at ...)` was read).
- Zones: `SetBorderDisplayStyle( style, pitch, true )` rebuilds hatch lines (not written);
  `CalculateFilledArea()`; legacy stroked fills are inflated (7.2).
- `FOOTPRINT::FixUpPadsForBoard( m_board )` (P:7219, `pcbnew/footprint.cpp:5214`): for footprints in
  `EXPAND_INNER_LAYERS` stackup mode every PTH pad gets all copper layers of the board OR-ed
  into its layer set - changes what `(layers ...)` prints.
- Collinear track arcs become plain segments (`IsStraightArc`, P:9051) keeping the uuid.
- Reference image scale migration (P:4573), bold thickness migration (P:899), pad thermal spoke
  angle defaults (P:7973-7995), zero-size pad clamp to 0.001 mm with a warning (P:8005).

---------------------------------------------------------------------------------------------------

## 5. What this subsystem needs from other subsystems

### 5.1 `kicad-sexpr` (lexer, formatter)

The C++ parser is **not** a tree walker; it pulls tokens. Port it the same way (a pull lexer
with one-token state), because several branches depend on token class and on the previous token:

- `NextTok()`, `CurTok()`, `PrevTok()`, `CurText()` (raw text), `CurStr()`, `FromUTF8()`,
  `CurLine()`, `CurLineNumber()`, `CurOffset()`, `CurSource()`.
- Token classes: `T_LEFT`, `T_RIGHT`, `T_EOF`, `T_NUMBER` (regex in `DSNLEXER::isNumber`,
  `dsnlexer.cpp:495`: `[-+]?digits[.digits]([eE][-+]?digits)?` covering the whole word),
  `T_STRING` (quoted; escapes `\"`, `\\`, `\a \b \f \n \r \t \v`, `\xHH`, octal - lines 655-715),
  `T_SYMBOL`, `T_BAR` (only when `SetKnowsBar(true)`), keyword tokens. A quoted word is always
  `T_STRING` even if it spells a keyword; an **unquoted** word that spells a keyword is that
  keyword. `IsSymbol( tok )` is true for `T_SYMBOL`, `T_STRING` **and any keyword**
  (`dsnlexer.cpp:340`). The parser relies on these distinctions, e.g. `parseLayer` (user name
  must be `T_STRING`, `hide` bare), `parseGROUP` (name `T_STRING`, bare `locked`), legacy bare
  `locked` before a text value (P:4626).
- `NeedLEFT/NeedRIGHT/NeedSYMBOL/NeedSYMBOLorNUMBER/NeedNUMBER(expected)/NeedBAR`,
  `Expecting( text | token )`, `Unexpected(...)` producing `PARSE_ERROR` with source, line,
  offset; `parseDouble()`, `parseDouble( expected )`.
- `SetKnowsBar`, `ReadCommentLines`, `SyncLineReaderWith( other lexer )`: sub-parsers with
  **their own keyword tables** take over the same input position: `STROKE_PARAMS_PARSER`
  (`stroke_params.keywords`), `PCB_PLOT_PARAMS_PARSER` (`pcb_plot_params.keywords`),
  `EMBEDDED_FILES_PARSER` (`embedded_files.keywords`). A word such as `width` is a different
  token id in each table.
- Keyword table for `common/pcb.keywords` (507 entries), `GetTokenText/GetTokenString`.
- Output: an `OUTPUTFORMATTER` equivalent (append-only byte buffer), `Quotes()`/`Quotew()`
  (always wraps in `"` and escapes only `\n`, `\r`, `\\`, `"`; `richio.cpp:466`), `Prettify`
  (both modes), `FormatBool`, `FormatOptBool`, `FormatUuid`, `FormatCustomProperties`,
  `FormatStreamData` (base64 in 76-character quoted chunks).
- Error types: `IO_ERROR`, `PARSE_ERROR`, `FUTURE_FORMAT_ERROR( requiredVersion, generatorVersion )`.

### 5.2 `kicad-core`

- `KIID`: `from_str` with the legacy rules (7.3), `as_string`, `Ord` = byte order, `NIL`,
  `KIID_PATH` (split on `/`, `AsString`), and an **injected deterministic generator** for new ids.
- `LIB_ID`: `Parse( name, aFix = true )` returning the bad position, `Format()`,
  `GetLibItemName()`, `GetUniStringLibItemName()`, `IsValid()`.
- Units: `EDA_UNIT_UTILS::FormatInternalUnits( scale, int [, data type] )`,
  `FormatInternalUnits( scale, VECTOR2I )`, `FormatAngle`, `GetScaleForInternalUnitType`,
  `FetchUnitsFromString`; `FormatDouble2Str`; `pcbIUScale` (`IU_PER_MM = 1e6`, `IU_PER_PS`,
  `mmToIU`); `KiROUND`; printf-compatible `%.10g`, `%.10f`, `%.16f`, `%g`, `%.4f`.
- `EDA_ANGLE` (degrees constructor, `IsZero`, `AsDegrees`, `AsTenthsOfADegree`, `Normalize720`,
  comparisons), `ANGLE_0/45/90`.
- `PCB_LAYER_ID`, `LSET`, `LSEQ`: `Name`, `NameToLayer`, `AllCuMask( n )`, `InternalCuMask`,
  `AllLayersMask`, `AllTechMask`, `Seq`, `CuStack`, `TechAndUserUIOrder`, `FmtHex`/`ParseHex`,
  `IsCopperLayer`, `IsExternalCopperLayer`, `IsBackLayer`, `LAYER_RANGE( F_Cu, B_Cu, count )`,
  `MAX_CU_LAYERS`, legacy layer id map (`pcb_plot_params.cpp:476`).
- Strings: `ConvertToNewOverbarNotation` (`string_utils.cpp:87`), `StrNumCmp` (911),
  `ValueStringCompare` (1293), `From_UTF8`, `GetUserFieldName( n, UNTRANSLATED )`
  (`template_fieldnames.cpp:90`), `ReplaceIllegalFileNameChars`, `wxString::Cmp`,
  `wxString::Replace`, `StartsWith`.
- `PAGE_INFO` (`SetType`, user size in mm with clamps `MIN_PAGE_SIZE_MM`/`MAX_PAGE_SIZE_PCBNEW_MM`,
  portrait, `Format`), `TITLE_BLOCK` (`Format`).
- `STROKE_PARAMS` (+ parser, `Format`, `GetLineStyleToken`), `LINE_ENDING` (`Format`, style
  tokens), `KIGFX::COLOR4D` (parse `#RRGGBB[AA]`, `WithAlpha`, `UNSPECIFIED`).
- `EDA_TEXT`: attributes, `SetTextSize` (clamps to 0.001 mm .. 250 mm), thickness (0 = auto),
  bold/italic flags, justification, mirror, visibility, line spacing, unresolved font name,
  `ResolveFont`, render cache (`SetupRenderCache`, `AddRenderCacheGlyph`, `GetRenderCache`,
  invalidation by setters), `MigrateLegacyBoldStrokeWidth` (`BOLD_STROKE_MULTIPLIER = 1.6`),
  `Format( formatter, ctl )`, `Compare`, `GetShownText` (text-variable expansion).
- `EMBEDDED_FILES` (+ parser/writer, `AddFile`, `ClearEmbeddedFonts`, `UpdateFontFiles`,
  `Get/SetAreFontsEmbedded`, `DecompressAndDecode`: base64 -> zstd -> MurmurHash3 x64-128 check).
- `STRING_ANY_MAP` equivalent: typed values bool / f64 / string / `Vector2I` / `ShapeLineChain` /
  `Vec<Vector2I>`, with `set_iu` (value / IU scale) and `get_to_iu` (multiply, `KiROUND` for
  integral targets).
- `BITMAP_BASE` / `REFERENCE_IMAGE`: keep the original image bytes verbatim; pixel size, PPI and
  "legacy PPI" (`common/bitmap_base.cpp`: PNG `pHYs`, JPEG density) for scale handling.
- `KIFONT::FONT::GetFont( name, bold, italic )`, `IsStroke( name )`, `IsOutline()`, `GetName()`,
  `NameAsToken()` (from `kicad-font`).
- `GetMajorMinorVersion()` constant; base64 encode/decode with wx semantics.

### 5.3 `kicad-geom`

`VECTOR2I`, `SEG`, `SHAPE_ARC( start, mid, end, width )` + `GetP0/GetArcMid/GetP1`;
`SHAPE_LINE_CHAIN`: `Append( x, y )`, `Append( SHAPE_ARC )` (approximates the arc into points
and keeps the arc record; degenerate arcs become plain points - must match exactly because the
writer re-emits `(arc ...)` only for points that still carry an arc index), `PointCount`,
`CPoint`, `ArcIndex`, `Arc( i )`, `SetClosed`; `SHAPE_POLY_SET`: `NewOutline`, `Outline( i )`,
`COutline`, `AddOutline`, `AddHole`, `OutlineCount`, `Polygon( 0 )`, `IterateWithHoles` +
`SetVertex`, `TotalVertices`, `CVertex`, `RemoveAllContours`, `IsEmpty`,
`InflateWithLinkedHoles( d, CORNER_STRATEGY::ROUND_ALL_CORNERS, ARC_HIGH_DEF / 2 )`,
`BooleanAdd`, `Fracture`; `TransformOvalToPolygon( poly, a, b, width, ARC_HIGH_DEF, ERROR_OUTSIDE )`;
`IsStraightArc( start, mid, end )`; `TRANSFORM_TRS` (`Apply`, `InverseApply`, `GetScaleX/Y`);
`RotatePoint`.

### 5.4 Board model (`kicad-pcb` model modules)

- `BOARD`: constructor defaults; `Add( item, mode, skipConnectivity )` (container per type,
  uuid index incl. footprint/table children), `FinalizeBulkAdd`, `ResolveItem( uuid, true )`,
  `IsItemIndexedById`, `FindNet( name | code )`, `FindOrAddNet`, `GetNetInfo()` (name-ordered
  iteration, `AppendNet` code rules), `GetNetCount`, `FindPadByUuid`; layers: `SetLayerDescr`,
  `GetLayerName`, `GetLayerType`, `IsLayerEnabled`, `Set/GetEnabledLayers`,
  `Set/GetCopperLayerCount`, `IsBackLayer`; `SetProperties/GetProperties`,
  `SetPageSettings`, `SetTitleBlock`, `SetPlotOptions`, `SetGenerator`,
  `SetFileFormatVersionAtLoad`, `Set/LegacyTeardrops`, variants (`AddVariant`,
  `SetVariantDescription`, `GetVariantNames`, `GetVariantDescription`), `GetEmbeddedFiles`,
  `FixupEmbeddedData`, `EmbedFonts`, `GetComponentClassManager().GetNoneComponentClass()`,
  `GroupsSanityCheck( repair )`, `RefreshDrillSymbolLayers`, legacy flags
  (`m_LegacyDesignSettingsLoaded`, `m_LegacyNetclassesLoaded`, `m_LegacyVisibleLayers`,
  `m_LegacyVisibleItems`), containers `Footprints/Drawings/Tracks/Zones/Points/Groups/
  Generators/Constraints`, comparators (4.6).
- `BOARD_DESIGN_SETTINGS`: every field touched in `parseSetup`/`parseDefaults`/`formatSetup`
  (`m_SolderMaskExpansion`, `m_SolderMaskMinWidth`, `m_SolderPasteMargin`,
  `m_SolderPasteMarginRatio`, `m_AllowSoldermaskBridgesInFPs`, `m_Tent/Cover/PlugViasFront/Back`,
  `m_CapVias`, `m_FillVias`, `m_ZoneLayerProperties`, aux/grid origin, `m_HasStackup`,
  `GetStackupDescriptor()`, `GetBoardThickness`, track/via/diff-pair lists, min values,
  `m_LineThickness[]`, `m_TextSize[]`, `m_TextThickness[]`, `m_TextItalic[]`, `m_TextUpright[]`,
  `m_DimensionUnitsMode`, `m_DimensionPrecision`, `m_MaxError`, `m_CopperEdgeClearance`,
  `GetDefaultZoneSettings()`, `GetDrillSymbolProfile()`, `m_NetSettings`), `GetTextSize( layer )`
  etc. for `StyleFromSettings`.
- `BOARD_STACKUP`, `BOARD_STACKUP_ITEM` (type, layer id, dielectric id, sublayers, thickness
  + locked, material, epsilon_r, loss tangent, spec frequency, dielectric model, color,
  `IsThicknessEditable`, `IsColorEditable`, `HasMaterialValue`, ...), `BuildDefaultStackupList`,
  `FormatBoardStackup`.
- `NET_SETTINGS` / `NETCLASS`: default netclass, `HasNetclass`, `SetNetclass`,
  `SetDefaultNetclass`, `SetNetclassPatternAssignment`, setters used by `parseNETCLASS` and the
  legacy setup tokens.
- `NETINFO_ITEM`: name, code, net chain name, terminal pad uuids/pointers, `ResolveTerminalPads`.
- Items with every getter/setter used by P and W: `FOOTPRINT` (position/orientation/transform
  scale, fields, attributes, overrides, stackup mode/layers, private layers, net-tie groups,
  jumper groups, unit info, variants, 3D models + extruded bodies, component classes, embedded
  files, initial comments, `FixUpPadsForBoard`, `Flip`, `ClearAllNets`), `PAD` + `PADSTACK`
  (modes NORMAL / FRONT_INNER_BACK / CUSTOM, per-layer shape/size/offset/delta/ratios/chamfers/
  primitives/thermal settings, drill + secondary + tertiary drill, post-machining, outer-layer
  mask flags, unconnected-layer mode, zone layer overrides, `ForEachUniqueLayer`,
  `HasExplicitDefinitionForLayer`, `DefaultThermalSpokeAngleForShape`, `CanHaveNumber`),
  `PCB_SHAPE` (library coordinates, `RebakeFromLib`, `SetArcAngleAndEnd`, `SetArcOrSegment`,
  fill modes, line endings, proxy flag, `Compare`), `PCB_TEXT`/`PCB_FIELD` (lib text pos/size,
  keep-upright, knockout, `StyleFromSettings`, `Compare`), `PCB_TEXTBOX`, `PCB_TABLE`/
  `PCB_TABLECELL`, `PCB_DIMENSION_*` (`Update`, `UpdateHeight`, units helpers), `PCB_TRACK`/
  `PCB_ARC`/`PCB_VIA` (`SanitizeLayers`, `LayerPair`, `GetDrillValue`, teardrop params),
  `ZONE` (+ `ZONE_SETTINGS` defaults, layer properties, fills, island flags,
  `SetLayerSetAndRemoveUnusedFills`, `HatchBorder`, thieving settings, placement/link options),
  `PCB_GROUP`, `PCB_GENERATOR` + the three concrete types, `PCB_CONSTRAINT`, `PCB_TARGET`,
  `PCB_POINT`, `PCB_GRID_ITEM`, `PCB_BARCODE` (`AssembleBarcode`), `PCB_REFERENCE_IMAGE`,
  `PCB_DRILL_CHART`/`PCB_STACKUP_TABLE`/`PCB_DRILL_MAP`, `TEARDROP_PARAMETERS`,
  `DRILL_SYMBOL_PROFILE`, `PCB_PLOT_PARAMS`, `COMPONENT_CLASS`.

### 5.5 Higher layers (only for the oracle-equivalent re-save)

`BOARD::BuildConnectivity()` including net propagation; project file loading
(`.kicad_pro` -> `BOARD_DESIGN_SETTINGS`, `NET_SETTINGS`, text variables);
`RefreshGeneratedTables`. The DRC engine is created by `BOARD_LOADER` but does not affect I/O.

---------------------------------------------------------------------------------------------------

## 6. Output format, exactly

### 6.1 Two stages

1. The writer emits a flat token stream into a buffer with `Print( fmt, ... )`. Almost no
   newlines or indentation are emitted (a few historical `\n` remain, e.g. W:652, 1448, 3780);
   adjacent lists are concatenated without spaces (`(xy 1 2)(xy 3 4)`), optional words create
   double spaces or a space before `)` (e.g. `"(layer %s %s)"` with an empty knockout word).
2. `KICAD_FORMAT::Prettify` (`kicad_io_utils.cpp:112-365`) re-flows the whole buffer. It
   discards all whitespace outside quotes and decides layout only from the token structure, so
   stage-1 whitespace never reaches the file **except inside quoted strings**. Rules: tab
   indentation, one tab per depth; each `(` starts a new line unless it is the first character
   of the file, or it continues an `(xy ` run (kept on one line up to column 99), or (compact
   mode only) it is inside `font`/`stroke`/`fill`/`teardrop`/`offset`/`rotate`/`scale`; a list
   without sub-lists closes on the same line; a list with sub-lists closes on its own line;
   consecutive atoms wrap to a new line (indented at list depth) once the column reaches 72
   (this is what lays out long `(members ...)`, `(layers ...)`, `(data ...)` blocks); lines
   starting with `#` at depth 0 are copied verbatim (footprint initial comments); the file ends
   with exactly one `\n`. `column` counts bytes.

Implementers: emit the same token sequence as the C++ (one Rust statement per `Print`), then
call the ported `Prettify`. Do not hand-format.

### 6.2 Number and string formatting

| Value | Call | Result |
|---|---|---|
| lengths / coordinates (nm) | `formatInternalUnits( int )` = `EDA_UNIT_UTILS::FormatInternalUnits( pcbIUScale, v )` (`eda_units.cpp:191`) | `v / 1e6` as double; if `!= 0 && |x| <= 0.0001`: `%.10f` with trailing zeros (and a trailing `.`) stripped; else `%.10g`. |
| points | `formatInternalUnits( VECTOR2I )` | `X Y` separated by one space. |
| footprint-child points given in board frame | `formatInternalUnits( pt, parentFP )` (W:487) | `parentFP->GetTransform().InverseApply( pt )` first. |
| time (pad `die_delay`) | `formatInternalUnits( v, EDA_DATA_TYPE::TIME )` | divide by `IU_PER_PS`. |
| angles | `EDA_UNIT_UTILS::FormatAngle` | `%.10g` of degrees. |
| ratios and other doubles | `FormatDouble2Str` (`string_utils.cpp:1593`) | as lengths but `%.16f` in the small branch; `-0.0` prints `-0`. |
| generator numeric properties | `fmt::format( "{:.10g}" )` (W:3140) | |
| reference image scale / opacity | `fmt::format( "(scale {:g})" )` (W:1354-1357) | `%g` (6 significant digits). |
| 3D model opacity | `fmt::format( "(opacity {:.4f})" )` (W:1859) | |
| post-machining angle | `FormatDouble2Str( angle / 10.0 )` (W:2125) | stored in tenths of a degree. |
| zone `island_area_min` | `formatInternalUnits( GetMinIslandArea() / pcbIUScale.IU_PER_MM )` (W:3682) | `long long / double` implicitly truncated to `int`, then formatted as a length. Parser inverse: `SetMinIslandArea( parseBoardUnits * IU_PER_MM )` (P:10002). |
| constraint value | `FormatDouble2Str`, lengths divided by `IU_PER_MM` first (W:3090) | |
| page size | `FormatDouble2Str( GetWidthMils() * 25.4 / 1000.0 )` (`page_info.cpp`) | |
| enums printed as integers | `%d`: dimension `units`, `units_format`, `precision`, `text_position_mode`, `orientation`, `text_frame`; `zone_connect`; zone `priority`, `island_removal_mode`, `hatch_smoothing_level`; board layer numbers; plot params ints; grid item `priority`/`tick_interval` (`%u`). | |
| layer bit masks | `0x%s` with `LSET::FmtHex()` (`pcb_plot_params.cpp`) | |
| strings | `Quotew` - always quoted. Exceptions written bare: keywords/enumerators, generator `(type %s)` (W:3122, raw UTF-8 of the type string), generator property **keys** (W:3143, 3199), group `(lib_id "%s")` (manual quotes, no escaping, W:3037), footprint lib `generator_version "%s"` (W:365, manual quotes). | |
| booleans | `KICAD_FORMAT::FormatBool` -> `(key yes)` / `(key no)`; `FormatOptBool` adds `(key none)`. | |
| uuids | `(uuid "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx")` (lower case). | |

### 6.3 Board file skeleton (after Prettify)

```
(kicad_pcb
	(version 20261031)
	(generator "pcbnew")
	(generator_version "10.99")
	(general
		(thickness 1.6)
		(legacy_teardrops no)
	)
	(paper "A4")
	[(title_block ...)]                 only if any field is non-empty
	(layers
		(0 "F.Cu" signal)               (id "canonical name" type ["user name" if different])
		(2 "B.Cu" signal)
		(9 "F.Adhes" user "F.Adhesive")
		...
	)
	(setup
		[(stackup ...)]                 only if bds.m_HasStackup
		(pad_to_mask_clearance 0)
		[(solder_mask_min_width ..)] [(pad_to_paste_clearance ..)] [(pad_to_paste_clearance_ratio ..)]
		(allow_soldermask_bridges_in_footprints no)
		(tenting (front yes) (back yes)) (covering ..) (plugging ..) (capping no) (filling no)
		[(zone_defaults ...)] [(aux_axis_origin x y)] [(grid_origin x y)] [(drill_symbol_profile ...)]
		(pcbplotparams ...)
	)
	{(property "k" "v")}
	[(variants ...)]
	{footprints} {drawings} {points} {tracks/vias/arcs} {zones} {groups} {generators} {constraints}
	[(net_chains ...)]
	(embedded_fonts no)
	[(embedded_files ...)]
)
```

Non-copper layer type column (W:786-811): `user` unless the layer id is >= `User_1` and its
board type is `LT_FRONT`/`LT_BACK` (then `front`/`back` via `LAYER::ShowType`).

### 6.4 Footprint library file

`formatInitialComments` (verbatim `#...` lines, only when `CTL_OMIT_INITIAL_COMMENTS` is not set),
then `(kicad_footprint_lib (version 20261031) (generator "kicad_footprint_editor")
(generator_version "10.99")` + `(footprint ...)` formatted with `aInLibFile = true` (no
version/generator inside the footprint) + `)` (W:354-371). With `CTL_FOR_LIBRARY`: name without
library nickname, no footprint `(uuid)`, no `(transform ...)`, no `(path)`, no pad/shape/zone/
track nets nor `pinfunction`/`pintype`, embedded files written **with** data
(`!( m_ctl & CTL_FOR_BOARD )`, W:1794). File naming: `<footprint name with illegal file-name
characters replaced by '_'>.kicad_mod`; `FP_CACHE::Load` sets the footprint id to the **file
base name** (W:175-183), not the name inside the file.

### 6.5 Optional-token rules worth knowing up front

- Written only when non-default: `locked`, `placed`, `descr`, `tags`, `height`, overrides
  (`has_value()`), `zone_connect` (!= INHERITED), `attr` (any flag), `stackup` (footprint, mode
  != EXPAND_INNER_LAYERS), `private_layers`, `net_tie_pad_groups` (`IsNetTie()`),
  `jumper_pad_groups`, text `(unlocked yes)` (footprint text with keep-upright off), field
  `(hide yes)`, effects `line_spacing` (!= 1.0), `thickness` (not auto), `bold`, `italic`,
  `justify`, shape `radius` (> 0), `net` on shapes (code > 0), pad `rect_delta`, `drill`
  (W:2067), `property`, thermal tokens, `teardrops` (non-default parameters), `tenting`/
  `covering`/`plugging` (any side has a value), via type word, `free`, zone `priority` (> 0),
  `name`, hatch-mode blocks, dimension `override_value`, `suppress_zeroes`, `keep_text_aligned`.
- Always written: footprint `duplicate_pad_numbers_are_jumpers`, `embedded_fonts`; pad
  `remove_unused_layers` (PTH only) and, if set, `keep_end_layers` and (with a board)
  `zone_layer_connections`; text box `border`, `knockout`; barcode `hide`, `knockout`; zone
  `hatch`, `connect_pads (clearance ..)`, `min_thickness`, `fill (thermal_gap ..)
  (thermal_bridge_width ..) (island_removal_mode N)`; track/via `(net "name")` even for the
  empty net (W:3490) - unlike pads/shapes/zones which omit net 0.
- Never written: zone/teardrop uuid and name for teardrop areas (W:3524-3528); empty groups;
  member-less `tuning_pattern`/`via_stack` generators; invalid polygons; 3D models with an
  empty file name; extruded bodies with height <= 0; markers; net declarations; net classes.
- Zone layers: `layers &= board enabled layers`; more than one layer => `formatLayers( layers,
  enumerate = IsOnCopperLayer(), isZone = true )`, else `(layer "X")`. A zone with any copper
  layer enumerates every layer by name, so the `"F&B.Cu"` branch of `formatLayers` is not
  reachable for zones in practice; the wildcard is still accepted on read
  (`Issue19775_ZoneLayerWildcards`).
- `formatLayers` (pads, multi-layer shapes/tracks): if the mask contains every copper layer of
  the **board** (`AllCuMask( board copper count )`, or all 32 without a board) -> `"*.Cu"` and all
  copper bits are cleared; else if exactly F+B within the board mask -> `"*.Cu"` (or `"F&B.Cu"`
  for zones); then `*.Adhes`, `*.Paste`, `*.SilkS`, `*.Mask`, `*.CrtYd`, `*.Fab` pairs; then the
  remaining layers by id.

---------------------------------------------------------------------------------------------------

## 7. Format as implemented, and backward compatibility

### 7.1 Token tree (reader). `L` = legacy, read but never written. `@v` = version gate.

Unless noted, sub-lists may appear in any order and an unknown token is a hard `PARSE_ERROR`
(`Expecting(...)`). Function and line range in brackets.

```
kicad_pcb                                   [parseBOARD_unchecked 1293]
  (version N)                               must be the first list (parseHeader 1940); if the first list is
                                            something else its keyword is swallowed, the version becomes
                                            20201115 and the parse derails (in practice an error)
  (host "name" ["build"])                   L; second word required @<20200825
  (generator "s") (generator_version "s")
  (general (thickness mm) (legacy_teardrops [yes|no])   [1969]
           <any other flat list>)           L, skipped if it holds only symbols/numbers (links, no_connects,
                                            area, drawings, tracks, zones, modules, nets)
  (paper "A4" | "User" w h [portrait])      [2005]; (page ...) accepted @<=20200119
  (title_block (title "s") (date "s") (rev "s") (company "s") {(comment 1..9 "s")})   [2061]
  (layers {(N "name" type ["user name" | hide])})        [2556, parseLayer 2169]
  (setup ...)                               [3033] see below
  (property "key" "value")                  board text variables
  (variants {(variant (name "s") [(description "s")])})  [538]
  (net N "name")                            L @<20251028 [3619]
  (net_class "name" "descr" {(clearance mm)|(trace_width mm)|(via_dia mm)|(via_drill mm)|(uvia_dia mm)
             |(uvia_drill mm)|(diff_pair_width mm)|(diff_pair_gap mm)|(add_net "net")})   L [3788]
  (net_chains {(net_chain (name "s") (members {(net "name"|N) | (member (net ..)) L})
               {(terminal_pad "uuid")} max 2)})          [3648] unknown entries skipped
  (footprint ...) | (module ...) L          [6250]
  (gr_line|gr_rect|gr_circle|gr_arc|gr_poly|gr_curve|gr_ellipse|gr_ellipse_arc ...)   [3940]
  (gr_text ...) (gr_text_box ...) (table ...) (image ...) (barcode ...) (dimension ...)
  (drill_chart ...) (stackup_table ...) (drill_map ...) (target ...) (point ...) (grid_item ...)
  (segment ...) (arc ...) (via ...) (zone ...)
  (group ...) (generated ...) (constraint ...)
  (embedded_fonts yes|no)
  (embedded_files {(file (name "s") (type datasheet|font|model|worksheet|other)
                         [(data |base64 ... |)] (checksum "s"))})   [embedded_files.cpp 440]

setup                                       [3033]
  (stackup {(layer "F.Cu"|"dielectric N"|... (type "s") {(color "s") (thickness mm [locked]) (material "s")
                   (epsilon_r N) (loss_tangent N) (spec_frequency N)
                   (dielectric_model constant|djordjevic_sarkar) | addsublayer | <unknown: skipped>})}
           (copper_finish "s") (dielectric_constraints yes|no) (edge_connector yes|bevelled)
           (edge_plating yes|no) (castellated_pads ..) L | <unknown: skipped>)        [2226]
  (pad_to_mask_clearance mm) (solder_mask_min_width mm) (pad_to_paste_clearance mm)
  (pad_to_paste_clearance_ratio N) (allow_soldermask_bridges_in_footprints yes|no)
  (tenting (front B) (back B)) | (tenting [front] [back] | none) L
  (covering (front B) (back B)) (plugging (front B) (back B)) (capping B) (filling B)
  (zone_defaults {(property (layer "L") (hatch_position (xy mm mm)))})                [3450-3501]
  (aux_axis_origin mm mm) (grid_origin mm mm)
  (drill_symbol_profile (name "s") (group_by ..) (default_marks ..) (size mm) (width mm)
        (freeze_assignments B) {(assignment (key "s") (mark mode [N|"s"]) (descr "s"))} | <unknown: skipped>)  [2899]
  (pcbplotparams ...)                       own parser, pcb_plot_params.cpp 551
  L: last_trace_width user_trace_width trace_clearance zone_clearance zone_45_only clearance_min trace_min
     via_size via_drill via_min_annulus via_min_size through_hole_min via_min_drill hole_to_hole_min
     user_via(size drill) uvia_size uvia_drill uvias_allowed blind_buried_vias_allowed uvia_min_size
     uvia_min_drill user_diff_pair(w gap viagap) segment_width edge_width mod_edge_width pcb_text_width
     mod_text_width pcb_text_size(w h) mod_text_size(w h) pad_size(w h) pad_drill visible_elements(hex)
     max_error filled_areas_thickness
     (defaults (edge_clearance mm) (copper_line_width mm) (copper_text_dims (size h w) (thickness mm)
               [italic] [keep_upright]) (courtyard_line_width mm) (edge_cuts_line_width mm) (silk_line_width mm)
               (silk_text_dims ..) (fab_layers_line_width mm) (fab_layers_text_dims ..)
               (other_layers_line_width mm) (other_layers_text_dims ..) (dimension_units N)
               (dimension_precision N) | <unknown: skipped>)                          [3504-3616]
  anything else: Unexpected

pcbplotparams (tokens of pcb_plot_params.keywords)
  layerselection plot_on_all_layers_selection disableapertmacros usegerberextensions usegerberattributes
  usegerberadvancedattributes creategerberjobfile gerberprecision dashed_line_dash_ratio dashed_line_gap_ratio
  svgprecision pngdpi pngantialias plotframeref mode useauxorigin pdf_front_fp_property_popups
  pdf_back_fp_property_popups pdf_metadata pdf_single_document dxfpolygonmode dxfimperialunits
  dxfusepcbnewfont psnegative psa4output plot_black_and_white sketchpadsonfab plotpadnumbers hidednponfab
  sketchdnponfab crossoutdnponfab subtractmaskfromsilk outputformat mirror drillshape scaleselection
  outputdirectory
  L: svguseinch excludeedgelayer viasonmask hpglpennumber hpglpenspeed hpglpenoverlay pscolor
     plotinvisibletext; numeric layerselection (pretty-3 format)

footprint "lib:name" | module ...           [6250]
  (version N) (generator "s") (generator_version "s")       only in stand-alone footprint text
  (locked [B]) (placed [B]) (layer "F.Cu"|"B.Cu")           any other layer is coerced to F.Cu
  (tedit HEX) L   (tstamp ID) L | (uuid "id")
  (at x y [angle]) L | (transform (translate x y) (rotate deg) (scale sx sy))
  (descr "s") (tags "s") (height mm)
  (property "name" "value" <text effects body: at, layer, hide, uuid, unlocked, effects, render_cache ...>)
  (property ki_fp_filters "s")
  (path "/uuid/uuid") (sheetname "s") (sheetfile "s")
  (units {(unit (name "s") (pins "1" "2" ...))})            unknown sub-entries skipped
  (autoplace_cost90 N) (autoplace_cost180 N) L
  (private_layers "L" ...) (net_tie_pad_groups "1, 2" ...)
  (duplicate_pad_numbers_are_jumpers B) (jumper_pad_groups ("1" "2") ...)
  (solder_mask_margin mm) (solder_paste_margin mm) (solder_paste_margin_ratio N) | (solder_paste_ratio N) L
  (clearance mm) (zone_connect N) (thermal_width mm) L ignored (thermal_gap mm) L ignored
  (attr [smd] [through_hole] [virtual L] [board_only] [exclude_from_pos_files] [exclude_from_bom]
        [exclude_from_sim] [allow_missing_courtyard] [dnp] [allow_soldermask_bridges])
  (stackup {(layer "name")})                                [7256]
  (component_classes {(class "s")})
  (fp_text reference|value|user "text" ...) L for reference/value (now properties)
  (fp_text_box ...) (table ...) (fp_line|fp_rect|fp_circle|fp_arc|fp_poly|fp_curve|fp_ellipse|fp_ellipse_arc ...)
  (image ...) (barcode ...) (dimension ...) (pad ...) (zone ...) (group ...) (constraint ...) (point ...)
  (variant (name "s") (dnp [B]) (exclude_from_bom [B]) (exclude_from_sim [B]) (exclude_from_pos_files [B])
           {(field (name "s") (value "s"))})                [594]
  (embedded_fonts B) (embedded_files ...)
  (model "file" (hide [B]) (opacity N) (at (xyz ..)) L inches (offset (xyz ..)) (scale (xyz ..)) (rotate (xyz ..)))  [954]
  (model (type extruded) (uuid ..) (hide B) (overall_height mm) (body_pcb_gap mm)
         (layer pad_bbox|auto|"L") (material plastic|matte|metal|copper)
         (color r g b a | unspecified) (offset (xyz ..)) (scale (xyz ..)) (rotate (xyz ..)))                [6909-7080]
  (custom_property "k" "v")

gr_*/fp_* shape                              [3940]
  gr_line|fp_line|gr_vector: [locked L] (start x y) (end x y)
  gr_rect|fp_rect|gr_bbox:   [locked L] (start x y) (end x y)
  gr_circle|fp_circle:       [locked L] (center x y) (end x y)
  gr_arc|fp_arc:             [locked L] (start x y) (mid x y) (end x y)
                             @<=20210925: (start = centre) (end = arc start) (angle deg)
  gr_poly|fp_poly:           [locked L] (pts {(xy x y) | (arc (start ..) (mid ..) (end ..))})
  gr_curve|fp_curve:         [locked L] (pts (xy) (xy) (xy) (xy))
  gr_ellipse*, fp_ellipse*:  (center x y) (major_radius mm) (minor_radius mm) (rotation_angle deg)
                             [(start_angle deg) (end_angle deg)]
  then any of: (angle deg) L ignored, (layer "L"), (layers "L" ...), (solder_mask_margin mm), (width mm) L,
  (radius mm), (stroke (width mm) (type ..) [(color ..)]), (tstamp id) L | (uuid "id"),
  (fill yes|no|solid|none|hatch|reverse_hatch|cross_hatch), (status HEX) L, (locked [B]), (net "name" | N),
  (start_shape style (length mm) (width mm) (stroke ..)), (end_shape ...), (custom_property ..)
  gr_bbox / gr_vector exist only inside pad (primitives ...) and are marked as proxy items.

gr_text|fp_text ["type" for fp] [locked L] "text" ...        [4587, effects 4663]
  (at x y [angle] [unlocked L]) (layer "L" [knockout]) (tstamp|uuid) (hide [B]) fp only
  (locked B) (unlocked B) fp only  (effects ...) (render_cache "text" angle {(polygon {(pts ...)})})
  (custom_property ..)
effects                                      [775]
  (font (face "s") (size h w) (line_spacing N) (thickness mm) (bold [B]) (italic [B]))
  (justify [left|right] [top|bottom] [mirror]) (hide [B])
gr_text_box|fp_text_box|table_cell [locked L] "text" ...     [5017]
  (locked [B]) (start x y) (end x y) | (pts ...) (angle deg) (stroke ..) (border B) (margins l t r b)
  (layer "L") (knockout B) (span cols rows) cells only, (tstamp|uuid) (effects ..) (render_cache ..)
  (custom_property ..)
table (uuid) (locked B) (layer) (angle) L ignored (column_count N) (column_widths mm..) (row_heights mm..)
      (cells {(table_cell ...)}) (border (external B) (header B) (stroke ..))
      (separators (rows B) (cols B) (stroke ..)) (custom_property ..)                  [5191-5372, 5753]
drill_chart|stackup_table: (uuid) (locked B) (layer) (units ..) (precision N)
      {(column (id tok) (name "s") (justify ..) (width mm) (hide B))} (row_keys {(key N "s")})
      (filter (plated B) (npth B) (vias B) (slots B) (backdrill B) (castellated B)) (totals B)
      (row_shapes (column N) {(shape row idx)}) + table geometry tokens; unknown tokens skipped  [5495-5750]
drill_map (uuid) (locked B) (layer) (offset x y) (size mm) (span all | "L1" "L2" [backdrill] [npth])
      (outline_slots B) (guide_cross B) (custom_property ..); unknown skipped                   [5401]
image (at x y) (layer) (rotate deg) (opacity N) (scale N) (data "base64" ...) (locked B) (uuid) (custom_property ..)  [4459]
barcode (at x y [angle]) (layer) (size w h) (text "s") (text_height mm) (type code39|code128|datamatrix|qr|microqr)
      (ecc_level L|M|Q|H) (locked [B]) (tstamp|uuid) (hide B) (knockout B) (margins x y) (custom_property ..)  [4821]
dimension [locked L] (type aligned|orthogonal|leader|center|radial) | (width mm) L (old aligned dimension)
      (layer) (tstamp|uuid) (gr_text ...) (pts (xy) (xy)) (height mm) (leader_length mm) (orientation N)
      (format (prefix "s") (suffix "s") (units N) (units_format N) (precision N) (override_value "s")
              (suppress_zeroes [B]))
      (style (thickness mm) (arrow_length mm) (arrow_direction inward|outward) (text_position_mode N)
             (extension_height mm) (extension_offset mm) (keep_text_aligned [B]) (text_frame N))
      (locked [B]) (custom_property ..)
      L: (feature1 (pts ..)) (feature2 (pts ..)) (crossbar (pts ..)) (arrow1a|arrow1b|arrow2a|arrow2b (pts ..))  [5766]
target x|plus (at x y) (size mm) (width mm) (layer) (tstamp|uuid) (custom_property ..)          [10536]
point (at x y) (size mm) (layer) (uuid) (custom_property ..)                                    [10483]
grid_item xy|polar (at x y) (extent a b) (spacing a b) (angle deg) (priority N) (tick_interval N)
      (affects (cursor B) (routing B) (placement B)) (locked B) (uuid) (custom_property ..)     [10603]

pad "number" thru_hole|smd|connect|non_component|np_thru_hole circle|rect|oval|trapezoid|roundrect|custom  [7320]
  [locked L] (at x y [angle]) (size w h) (rect_delta dx dy)
  (drill [oval] [d1 [d2]] [(offset x y)])
  (backdrill (size mm) (layers "L" "L")) (tertiary_drill ...) (front_post_machining counterbore|countersink
      (size mm) (depth mm) (angle deg)) (back_post_machining ...)
  (layers "L" ...) (net [N] "name") (pinfunction "s") (pintype "s") (sim_electrical_type source|sink)
  (die_length mm) (die_delay ..) (solder_mask_margin mm) (solder_paste_margin mm)
  (solder_paste_margin_ratio N) (clearance mm) (zone_connect N) (thermal_bridge_width mm) | (thermal_width mm) L
  (thermal_bridge_angle deg) (thermal_gap mm) (roundrect_rratio N) (chamfer_ratio N)
  (chamfer [top_left] [top_right] [bottom_left] [bottom_right])
  (property pad_prop_bga|pad_prop_fiducial_glob|pad_prop_fiducial_loc|pad_prop_testpoint|pad_prop_heatsink
            |pad_prop_castellated|pad_prop_mechanical|pad_prop_pressfit|none)
  (options (clearance outline|convexhull) (anchor rect|circle)) (primitives {gr_* shapes incl. gr_bbox, gr_vector})
  (remove_unused_layers [B]) (keep_end_layers [B]) (zone_layer_connections "L" ...)
  (teardrops ...) (tenting ...) (locked [B]) L ignored (tstamp|uuid) (custom_property ..)
  (padstack (mode front_inner_back|custom) {(layer "Inner"|"L" (shape ..) (size w h) (offset x y)
            (rect_delta ..) (roundrect_rratio N) (chamfer_ratio N) (chamfer ..) (thermal_bridge_width mm)
            (thermal_gap mm) (thermal_bridge_angle deg) (zone_connect N) (clearance mm) (tenting ..)
            (options ..) (primitives ..))})                                            [8169]
teardrops (enabled [B]) (allow_two_segments [B]) (prefer_zone_connections [B]) (best_length_ratio N)
          (max_length mm) (best_width_ratio N) (max_width mm) (curved_edges [B]) | (curve_points N) L
          (filter_ratio N)                                                             [707]

segment [locked L] (start x y) (end x y) (width mm) (layer "L") | (layers "Cu" "Mask") (solder_mask_margin mm)
        (net "name"|N) (tstamp|uuid) (status HEX) L (locked [B]) (custom_property ..)  [9065]
arc     same plus (mid x y)                                                            [8951]
via [locked L] [blind|buried|micro] (at x y) (size mm) (drill mm) (layers "L1" "L2") (net ..)
    (remove_unused_layers [B]) (keep_end_layers [B]) (start_end_only [B]) (zone_layer_connections "L" ...)
    (padstack (mode ..) {(layer "Inner"|"L" (size mm))}) (teardrops ..) (tenting ..) (covering ..) (plugging ..)
    (filling ..) (capping ..) (tstamp|uuid) (status HEX) L (locked [B]) (free [B])
    (backdrill ..) (tertiary_drill ..) (front_post_machining ..) (back_post_machining ..) (custom_property ..)  [9162]

zone [locked L]                                                                        [9610]
  (net "name"|N) (net_name "s") L (layer "L") | (layers "L" ...) (property (layer "L") (hatch_position (xy ..)))
  (tstamp|uuid) (name "s") (hatch none|edge|full pitch) (priority N) (locked B)
  (attr (teardrop (type padvia|track_end)))
  (connect_pads [yes|no|thru_hole_only] (clearance mm)) (min_thickness mm) (filled_areas_thickness B) L
  (keepout (tracks allowed|not_allowed) (vias ..) (pads ..) (copperpour ..) (footprints ..))
  (placement (enabled B) (sheetname "s") | (component_class "s") | (group "s")
             (link "uuid" (placement B) (routing B) (connected_routing B) (other_items B) (group_items B)
                   (locked_items B) (anchor "uuid")))
  (fill [yes] (mode hatch|polygon|segment L|thieving @>=20260513) (arc_segments N) L ignored
        (thermal_gap mm) (thermal_bridge_width mm) (smoothing none|chamfer|fillet) (radius mm)
        (island_removal_mode 0|1|2) (island_area_min N) (hatch_thickness mm) (hatch_gap mm)
        (hatch_orientation deg) (hatch_smoothing_level N) (hatch_smoothing_value N)
        (hatch_border_algorithm hatch_thickness|min_thickness) (hatch_min_hole_area N)
        (thieving (type dots|squares|hatch) (size mm) (gap mm) (width mm) (stagger B) (orientation deg)))
  (polygon (pts ...))            first = outline, further = holes
  (filled_polygon [(layer "L")] [(island [B])] (pts ...))
  (fill_segments {(pts (xy) (xy))}) L
  (custom_property ..)

group "name" [locked L] (uuid|id "id") (locked B) (lib_id "lib:name") (members "uuid" ...) (custom_property ..)  [8489]
generated (uuid|id "id") first, then (type sym) (name "s") (layer "L") (locked yes|no) (members ..)
          (templates {(template (name "s") (via ...))}) (custom_property ..)
          and any other (key value): value = yes|no | number | "string" | (xy x y) | (pts ...) |
          (cells {(ij col row)})  -> stored as generator properties                    [8765]
constraint (type tok) (uuid) (members {(member "uuid" anchor [index])}) (value N) (driving B) (custom_property ..)  [8579]
```

Stand-alone footprint files: either a bare `(footprint ...)`/`(module ...)` (older files) or
`(kicad_footprint_lib (version N) (generator ..) (generator_version ..) {(footprint ...)})`
(`ParseFootprintLib`, P:1160-1274; `version` is mandatory there and must precede the first
footprint).

### 7.2 Every `m_requiredVersion` branch

| P line | Condition | Behaviour |
|---:|---|---|
| 1951-1965 | header | `(version N)`; else 20201115; `m_tooRecent = N > 20261031`; `SetKnowsBar( N >= 20240706 )`. |
| 1303 | `> SEXPR_BOARD_FILE_VERSION` | `FUTURE_FORMAT_ERROR` at `generator_version` / `general`; any other `PARSE_ERROR` while `m_tooRecent` is converted too (P:1277-1290). |
| 1322, 2007 | `<= 20200119` | `(page ...)` is `(paper ...)`. |
| 1332 | `< 20200825` | `(host name build)` has a second word. |
| 2214 | `>= 20200922` | layer entries carry a separate user name; older: name doubles as user name. |
| 2456 | `< 20210824` | stackup colours `#RRGGBB` get an alpha appended (`DEFAULT_SOLDERMASK_OPACITY` for mask layers, 1.0 otherwise) and are re-printed as `#%02X%02X%02X%02X`. |
| 333, 789, 3630, 3850, 7654 | `< 20210606` | `ConvertToNewOverbarNotation` on net names (item nets, net declarations, netclass members, pad nets) and on text item content. |
| 3413 | passes version | `PCB_PLOT_PARAMS_PARSER`: `< 20240819` layer hex masks use the legacy layer numbering and are remapped (`pcb_plot_params.cpp:588-635`). |
| 3980 | `<= 20210925` (`LEGACY_ARC_FORMATTING`) | arcs: `(start centre) (end start) (angle)` -> `SetCenter`, `SetStart`, `SetArcAngleAndEnd( angle, true )`. |
| 4573 | `< 20260623` | reference image scale `*= PPI / legacyPPI` when they differ. |
| 4690, 4794, 4809 | `>=`/`<` 20260616 (`FIRST_FP_AFFINE_TRANSFORM`) | footprint text: new files store library position (`SetLibTextPos`) and library size/thickness; old files store FP-relative position + absolute angle and are rotated/moved into board frame. |
| 5175 | `< 20230825` | text box border enabled iff stroke width >= 0. |
| 6286, 6304-6306 | footprint `(version)` | `max( current, this )`; too recent vs `SEXPR_FOOTPRINT_LIB_FILE_VERSION`. A footprint file without version parses with version 0 (all legacy branches on). |
| 6459, 6536 | `< 20230620` | footprint `property`: `ki_keywords`/`ki_locked` dropped; `ki_description` -> Description field; `Sheetfile`/`Sheet file`, `Sheetname`/`Sheet name` -> sheet file/name; properties default to **hidden**. |
| 6650 | `< 20220427` | `Edge.Cuts` and `Margin` removed from private layers. |
| 6716-6747, 7730-7760 | `<= 20240201` | footprint and pad overrides (`solder_mask_margin`, `solder_paste_margin`, `solder_paste_margin_ratio`, `clearance`): value 0 means "not set". |
| 7103 | `< 20260616` | footprint zone outline vertices are board-frame: `InverseApply` each, then `HatchBorder()`. |
| 7136 | `< 20260616` | footprint points are board-frame (`SetPosition( filePos )` after adding). |
| 7226 | `< 20200826` | no attributes => `FP_THROUGH_HOLE`. |
| 7229 | `<= 20220815` (`LEGACY_NET_TIES`) | keywords starting with `net tie` => one pad group listing all pad numbers joined by `", "`. |
| 7717 | `<= 20250926` | pad `die_delay` parsed with the length scale instead of the time scale. |
| 7986 | `<= 20211014` | default thermal spoke angle of custom pads with circular anchor is 90 instead of 45 degrees. |
| 9179 | `< 20250228` | via `covering`/`plugging`/`filling`/`capping` default to explicit `false` (so they are written back as `no`, not `none`). |
| 9629 | `< 20250210` | zone fills are "stroked" unless `(filled_areas_thickness no)` is present -> polygons inflated by `min_thickness / 2` with `InflateWithLinkedHoles` (P:10370-10389) + warning. |
| 9792, 9856 | `< 20260513` | `thieving` fill mode rejected. |
| 10469 | `< 20230517` | a teardrop-area zone sets `board->SetLegacyTeardrops( true )`. |
| 899 | `< 20260826` | `MigrateLegacyBoldStrokeWidth()`: bold stroke-font text with explicit thickness > 1 gets `max( 2, KiROUND( t / 1.6 ) )`. |

`FIRST_NORMALIZED_VERISON` (20230924) is defined in H but not referenced by the parser; the
boolean normalisation is handled token-locally by `parseMaybeAbsentBool`.

### 7.3 Legacy handling that is token-driven (no version check)

- `module` = `footprint`; `tstamp` = `uuid`; group/generator `id` = `uuid`; `host`; `tedit`;
  `status`; `autoplace_cost*`; bare `locked` before coordinates (shapes, text, dimensions, text
  boxes, tracks, vias, zones, groups); `(locked)` on pads (ignored: pad locking no longer exists).
- `KIID( string )` (`common/kiid.cpp:108`): 1..8 hex characters => legacy time stamp stored in
  bytes 12..15 (prints as `00000000-0000-0000-0000-0000xxxxxxxx`); otherwise the boost string
  parser; on failure a **random** uuid. Items without any uuid token keep the random uuid given
  by their constructor.
- Copper stack: copper layers are identified by position, not number (`parseLayers`,
  P:2578-2634): first copper entry = F.Cu, last = B.Cu, inner ones by canonical name if
  recognised else `(index + 1) * 2`; the list of copper entries ends at the first layer whose
  type is not one of `signal|power|mixed|jumper|auxiliary|front|back`. Layer count must be even
  and >= 2 (`PARSE_ERROR` "%d is not a valid layer count"). `Inner1.Cu`..`Inner14.Cu` names;
  Italian/Polish/French translated technical layer names (`createOldLayerMapping`, P:2506).
  Unknown non-copper layer name => `IO_ERROR` "Layer '%s' in file '%s' at line %d is not in
  fixed layer hash.".
- Legacy setup tokens set `m_LegacyDesignSettingsLoaded` (so the project file does not override
  them later); `net_class` sets `m_LegacyNetclassesLoaded`; duplicate netclass name => `IO_ERROR`.
  `viasonmask` in plot params => board tenting flags (P:3423-3428). `visible_elements`,
  `hide` in layer entries => `m_LegacyVisible*` (not written back).
- Shapes without `(fill ...)` (P:4401-4415): rect/circle with stroke width 0 => filled; polygon
  not on Edge.Cuts => filled. Stroke width <= 0 on an unfilled shape => `DEFAULT_LINE_WIDTH`.
- Text: `%V` -> `${VALUE}`, `%R` -> `${REFERENCE}` in every text value (P:4636); missing font size
  => 1.524 mm; hidden `fp_text user` => converted to a hidden user field named
  `GetUserFieldName( field count )`; board-level hidden text is made visible; `fp_text reference`
  / `fp_text value` overwrite the mandatory fields (keeping the uuid).
- Footprint property named `Footprint` is parsed and discarded; `ki_fp_filters` sets the filter
  string (effects, if any, are discarded).
- Pads: `thru_hole` starts with drill 1 nm in case `(drill)` is absent; drill forced to 0 for
  smd/connect/non_component; number cleared when `!CanHaveNumber()`; legacy `(net N "name")`
  with a mismatching name => orphaned net.
- Zones: `net_name` (legacy, used to repair a wrong/missing net: existing net by name, else a new
  net with code `GetNetCount()`), `fill_segments` (converted with `TransformOvalToPolygon` +
  `BooleanAdd`), `(mode segment)` => polygons, non-copper or rule-area zones forced to net 0.
- Dimensions: old `(dimension <value> (width ..) ... (feature1 ..) (feature2 ..) (crossbar ..)
  (arrow* ..))` => aligned dimension, start/end from feature lines, height from the crossbar
  (`UpdateHeight( point2, point1 )`), units guessed from the text.
- 3D models: `(at (xyz ..))` offset in inches (x25.4).
- Embedded files: `(data)` without bars (9.0.0 writer bug) tolerated.

### 7.4 Error handling and unknown-token tolerance

- Default: unknown token => `PARSE_ERROR` via `Expecting`. The whole load fails; nothing is
  partially returned (for a new load the board is simply dropped).
- Tolerant places (`skipCurrent`): stackup (both levels), `drill_symbol_profile`, `defaults`,
  `net_chains`, footprint `units`, drill map, generated tables (+ filter), and the flat skip in
  `general`. Generators accept arbitrary keys as properties.
- Embedded-file parse errors (bad checksum etc.) are caught, pushed to the warnings, and the
  rest of the section is skipped by paren counting (P:1561-1591, 7149-7175).
- Version too new => `FUTURE_FORMAT_ERROR`.
- Silently dropped: tracks/arcs on non-copper layers, zones without corners, groups without
  resolvable members (at save), ghost generators, members from another footprint scope.
- `wxCHECK` guards at the top of each `parseXXX` are programmer-error asserts (`debug_assert!`).

---------------------------------------------------------------------------------------------------

## 8. Multi-threading

The parser and writer are strictly single-threaded ("not thread safe, but re-entrant", H:359).
Thread-related C++ around them, none of which needs porting:

- `KIID` generators are `thread_local` because footprint libraries are loaded on worker threads;
  `FP_CACHE::Load` comments mention worker threads for warnings. Sequential in the port.
- `checkpoint()` uses the steady clock for progress - drop.
- `BuildConnectivity()` (post-load) uses the thread pool for triangulation caching; results do
  not depend on scheduling (see the connectivity analysis).

Determinism requirements for the Rust port: (a) no hash-map iteration reaches output (the two
layer maps are lookup-only; `componentClassNames` is an `unordered_set` handed to
`SetTransientComponentClassNames`/`ResolveComponentClassNames` - the model must sort before
deriving anything ordered); (b) new uuids come from an injected deterministic generator;
(c) pointer-address tie-breaks become insertion-index tie-breaks.

---------------------------------------------------------------------------------------------------

## 9. Validation

### 9.1 Existing C++ tests to port (all under `qa/tests/pcbnew/` unless noted)

| Test file | Cases | What it asserts / data |
|---|---|---|
| `pcb_io/kicad_sexpr/test_kicad_sexpr.cpp` (1105) | `Issue19775_ZoneLayerWildcards` | `plugins/kicad_sexpr/Issue19775_ZoneLayers/LayerWildcard.kicad_pcb` and `LayerEnumerate.kicad_pcb`: zone layer sets after load and after a save/reload. |
| | `Issue23125_EmptyZoneDiscarded` | `EmptyZone.kicad_pcb`: only 1 zone survives. |
| | `CoincidentDrawingsSurviveSave` | two identical drawings differing only by uuid both survive save (comparator must fall through to uuid). |
| | `ScientificNotationLoading` | `ScientificNotation.kicad_pcb`: numbers like `1e-3` parse. |
| | `Issue23625_CorruptedStackupCapped`, `Issue24133_DuplicatedStackupNotInflated` | `corrupted_stackup.kicad_pcb`, `duplicated_stackup.kicad_pcb`: stackup item count capped / 9 items, thickness 1.6 mm. |
| | `CopperThievingZone_*` (4) | thieving round trip, all patterns, rejection in old version, malformed geometry. |
| | `MalformedDimensionTextThrowsCleanly` | malformed input => `IO_ERROR`, no crash. |
| | `FootprintSave_OmitsNetsOnAllBoardConnectedItems`, `LibraryFootprintKeepsGroupsAndLoadsUnlocked`, `ParseWarningDoesNotFailTheLibrary`, `LegacyModuleFileImports`, `MissingLibraryThrowsOnEveryEnumerate` | footprint library behaviour (`qa/data/libraries/Resistor_SMD.pretty`, `qa/data/pcbnew/legacy_module/Crystal_SMD_HC49-US.kicad_mod`). |
| | `Issue23752_*`, `Issue24642_*` (3), `Issue24955_*`, `Issue24911_*` | append mode (tier 3). |
| `test_prettifier.cpp` | `BoardAndFootprintPrettifier` | `qa/data/pcbnew/prettifier/{Reverb_BTDR-1V, Samtec_HLE-...}.kicad_mod`, `group_and_image.kicad_pcb` vs `*_formatted.*` goldens: `Prettify( input ) == golden` byte for byte, and parse( input ) == parse( prettified ). First test to port. |
| `test_save_load.cpp` | `RegressionSaveLoadTests` | boards `issue18, issue832, issue2568, issue5313, issue5854, issue6260, issue6945, issue7267, padstacks, padstacks_normal, padstacks_complex`: load, save, reload without error. |
| `test_board_loader.cpp` | `BoardLoaderWithInit`, `BoardLoaderWithoutInit`, `CollinearTrackArcLoadsAsTrack` | loader options; collinear arc => track. |
| `test_issue25650_collinear_arc.cpp` | 2 | `IsStraightArc` boundaries. |
| `test_footprint_load_save.cpp` | `FootprintLoadSave`, `FootprintFieldLockRoundTrip`, `FootprintUserFieldKeepsAutoThickness`, `LegacyFootprintPropertyTakesBoardDefaults` | footprint round trips; legacy property styling from board defaults (P:6545). |
| `test_fp_lib_load_save.cpp`, `test_graphics_load_save.cpp`, `test_group_load_save.cpp`, `group_saveload.cpp`, `test_generator_load_save.cpp` | | load expectations + save/reload for library footprints, graphics, groups (healthy / invalid), generators. |
| `test_pad_roundtrip.cpp`, `test_custom_pad_primitive_roundtrip.cpp` (521), `test_footprint_pad_fixup.cpp`, `test_pad_tenting.cpp`, `test_backdrill_properties.cpp` (`ReadsLegacyV10SlotLayout`, `ReadsLegacyV10TopOnly`) | | pad/padstack round-trip stability on transformed/scaled footprints, PTH layer expansion, tenting, backdrill legacy layouts. |
| `test_ellipse_roundtrip.cpp`, `test_line_ending_load_save.cpp`, `test_barcode_load_save.cpp`, `test_extruded_body_load_save.cpp`, `test_reference_image_load.cpp` (`ReferenceImagePpiScaleMigration`), `test_custom_property_roundtrip.cpp`, `test_via_stack_saveload.cpp` (`GoldenFileStillLoads`, `MemberlessStackIsNotSaved`), `constraints/test_constraint_serialization.cpp`, `test_dimension_zero_suppression.cpp`, `test_footprint_rescue_layer.cpp`, `test_teardrop_uuid.cpp`, `test_variant.cpp` (file parts), `net_chains/test_net_chain_*sexpr*.cpp` | | per-item round trips (tier 2 items). |
| `qa/tests/common/test_richio.cpp`, `test_format_units.cpp`, `test_kiid.cpp`, `test_lset.cpp`, `test_eda_text.cpp` | | lower-crate tests the I/O depends on. |

### 9.2 Data

`qa/data` + `demos` contain **447** `.kicad_pcb` (355 directly under `qa/data/pcbnew`, 20 demos,
the rest under `qa/data/{eeschema,downgrade,cli,fuzz,diff_merge,libraries}`) and **426**
`.kicad_mod`. Version distribution of the boards: 2 x version 4, 26 x 2020-2021 (5.99/6.0),
40 x 7.0-era, 54 x 8.0-era, 76 x 9.0-era, 142 x 10.0-era, 106 x 11-dev, 1 without header
(`qa/data/pcbnew/plugins/fakeboard.kicad_pcb`, must fail). Feature counts (boards containing the
token): zones 212, filled polygons 147, `(model` 219, legacy numeric `(net N` 215, `(stackup` 175,
vias 164, `tstamp` 76, `(angle ...)` tokens (legacy arcs, text boxes) 15, dimensions 40, groups 31, teardrops 26,
generators 24 (407 tuning patterns, 7 via stacks, 5 via stitches), text boxes 13, embedded files
13, legacy `net_class` 7, images 6, render caches (outline fonts) 5, `(module` 5, padstacks 4,
barcodes 3, constraints 3, tables 2, drill charts 2. Only **one** board
(`qa/data/cli/artwork_generation_regressions/ZoneFill-4.0.7.kicad_pcb`) exercises the stroked
zone-fill inflation and `fill_segments`. 94 boards under `qa/data/pcbnew` have a sibling
`.kicad_pro`, 25 a `.kicad_dru`. Files under `qa/data/fuzz` and `plugins/fakeboard.kicad_pcb`
("A fake Board file for testing") are not valid boards: the corpus runner must record for every
file whether the oracle loads it, and require the port to fail on exactly the files the oracle
rejects (same error class; message text where practical).

### 9.3 Differential tests against the oracle

Byte equality is only meaningful when the oracle's own non-determinism and its post-load steps
are controlled. Use four layers, cheapest first:

- **D0 - Prettify**: `prettify( file ) == file` for every corpus file that was written by a
  recent KiCad (idempotence), plus the three goldens of `test_prettifier.cpp`. No oracle needed.
- **D1 - footprints, pure I/O**: copy each `.pretty` directory (or single `.kicad_mod` into a
  temp `.pretty`), run `kicad-cli fp upgrade --force <dir>` (in-place) or with `-o`, and compare
  with `format_footprint_lib_file( parse_footprint_lib( text ) )` under `CTL_FOR_LIBRARY`.
  Expect byte equality for files in which every item has a uuid; for legacy files compare after
  uuid masking (below). Remember the footprint name becomes the file base name.
- **D2 - boards, normalised input**: for each corpus board, copy **only the board file** to a
  fresh temp dir (no `.kicad_pro`, `.kicad_prl`, `.kicad_dru`), run
  `kicad-cli pcb upgrade --force` once to obtain `O1` (now every item has a uuid and the format
  is current), copy `O1` to another fresh dir and run again to obtain `O2`. Test:
  `rust_resave( O1 ) == O2` byte for byte. Do **not** assume `O2 == O1` (net codes are
  re-derived once the legacy net declarations are gone, so tracks can be re-ordered once).
  Until `kicad-connectivity` is available, `rust_resave` = parse + write, and boards where the
  oracle's net propagation changes something will differ only in `(net ...)` tokens and track
  order; classify those mismatches explicitly instead of masking them. With connectivity
  available run the same steps as `BOARD_LOADER::initializeLoadedBoard`.
- **D3 - boards, legacy input**: `rust_resave( original )` vs `O1`, after canonicalising both
  sides: replace every uuid that does not literally occur in the original file by a placeholder,
  and sort sibling blocks that differ only in such placeholders (the oracle orders them by the
  random value). This validates the legacy branches of section 7.
- Skip lists (document in the test): boards with `render_cache` blocks or `(face ...)` fonts
  (outline fonts, 5 boards; the oracle regenerates glyph outlines from whatever fonts the host
  has), boards with `drill_chart`/`drill_map`/`stackup_table` until generated tables are ported,
  boards with `(embedded_fonts yes)`.
- Golden files for the default test run (no oracle): commit a handful of small `O1`/`O2` pairs
  under `crates/kicad-pcb/tests/golden/` (e.g. one 2-layer board with zones, one with padstacks,
  one footprint).

---------------------------------------------------------------------------------------------------

## 10. Rust module layout and work packages

### 10.1 Layout (crate `kicad-pcb`, following PORTING.md "one module per C++ file", split by topic where a file is too large)

```
crates/kicad-pcb/src/io/
  mod.rs                     load_board(text, opts) / save_board(&mut Board, opts) entry points (PCB_IO::LoadBoard, DoLoad, SaveBoard)
  kicad_sexpr/
    mod.rs                   port of pcb_io_kicad_sexpr.h: version constants, CTL_* flags, error mapping
    parser/
      mod.rs                 parser struct, init, helpers (P:104-535, 2809-2896, 9463-9520, 10737-10757)
      board.rs               Parse, finishItem, parseBOARD*, header, general, page, title block, layers (P:1069-1157, 1277-1795, 1940-2223, 2506-2708)
      setup.rs               stackup, setup, defaults, zone defaults, drill symbol profile (P:2226-2503, 2899-3616)
      nets.rs                net, net_class, net_chains, variants (P:538-591, 3619-3883)
      shape.rs               line ending, shapes, reference image (P:3886-4584)
      text.rs                effects, render cache, text, text box (P:775-951, 4587-4818, 4968-5186)
      table.rs               tables, generated tables, drill map, barcode (P:4821-4965, 5188-5764)
      dimension.rs           (P:5766-6231)
      footprint.rs           footprint, 3D model, fp stackup, fp variant, ParseFootprintLib (P:594-704, 954-1066, 1160-1274, 6234-7317)
      pad.rs                 pad, primitives, options, post machining, padstack, teardrops (P:707-772, 7320-8428)
      track.rs               arc, segment, via, viastack (P:8951-9460, 9523-9607)
      zone.rs                (P:3450-3501, 9610-10480)
      group.rs               group, generator, constraint, resolveGroups/Constraints (P:1798-1937, 8431-8948)
      misc.rs                point, target, grid item (P:10483-10734)
    writer/
      mod.rs                 Format dispatch, helpers, formatLayer(s), formatPolyPts (W:374-570, 1887-1969)
      board.rs               header/general/layers/setup/properties/variants, format(BOARD) (W:601-1064)
      footprint.rs           (W:1452-1884) + FormatFootprintLibFile (W:354-371)
      pad.rs                 (W:1972-2549)
      shape.rs               shapes, image, point, target, grid item (W:1183-1449)
      text.rs                text, text box, render cache (W:573-598, 2619-2773)
      table.rs               table data, drill map, generated tables, barcode (W:2552-2616, 2776-2999)
      dimension.rs           (W:1067-1180)
      track.rs               (W:871-905, 3241-3496)
      zone.rs                (W:3499-3790)
      group.rs               group, constraint, generator (W:3002-3238)
  pcb_plot_params.rs         port of pcbnew/pcb_plot_params.cpp (struct, Format, parser)
```

Suggested API (pure, wasm-safe): `parse_board( text: &str, opts: &LoadOptions ) -> Result<(Board, Vec<String>), IoError>`,
`parse_footprint_lib( text ) -> Result<Vec<Footprint>, IoError>`,
`format_board( board: &mut Board, opts: &SaveOptions ) -> Result<String, IoError>` (mutable
because of the embedded-font step; or clone internally), `format_footprint_lib_file`.
`LoadOptions` carries the uuid generator and the "rescue undefined layers" switch; `SaveOptions`
the control flags, generator version string and prettify mode.

### 10.2 Work packages

Line counts are C++ lines to port (parser + writer halves). External prerequisites are named by
crate. Recommended order for the earliest end-to-end slice: 01 -> 02 -> 03 -> 15 (minimal) -> 17
(harness, D0) -> 04, 05 -> 06 -> 07 -> 08 -> 09 -> 10 -> 11, then tier 2.

| Id | Title | C++ (lines) | ~Lines | Depends on | Tier | Validating tests |
|---|---|---|---:|---|:-:|---|
| PCBIO-01 | Parser core and board skeleton | parser.h; P:104-535, 1069-1157, 1277-1795, 1940-2223, 2506-2708, 2809-2896, 3619-3645, 10737-10757 | 2400 | kicad-sexpr lexer + keywords; kicad-core (KIID, LSET, units, page, title block); model: Board, layers, nets | 1 | header/layers of all corpus boards parse; `ScientificNotationLoading`; future-version and "not a PCB" errors; layer-count error |
| PCBIO-02 | Writer core and board skeleton | pcb_io_kicad_sexpr.h; W:283-330, 374-570, 748-868, 908-960, 1040-1064, 1887-1969; comparators `BOARD_ITEM::ptr_cmp`, `BOARD::cmp_drawings`, `PCB_TRACK::cmp_tracks` | 900 | PCBIO-01; kicad-sexpr formatter + Prettify | 1 | `BoardAndFootprintPrettifier`; D0; header round trip on a hand-made minimal board |
| PCBIO-03 | Tracks, vias, arcs, graphic shapes | P:707-772, 3886-4456, 8123-8166, 8951-9607; W:871-905, 1183-1331, 3241-3496 | 2100 | 01, 02; kicad-core stroke/line-ending; kicad-geom SHAPE_LINE_CHAIN/arc | 1 | `CoincidentDrawingsSurviveSave`, `CollinearTrackArcLoadsAsTrack`, `test_issue25650_collinear_arc`, `test_graphics_load_save`, `test_line_ending_load_save`, `test_ellipse_roundtrip` |
| PCBIO-04 | Setup section, stackup, plot parameters | P:2226-2503, 2899-3616; W:601-745; `pcb_plot_params.cpp` (899); `board_stackup.cpp` FormatBoardStackup | 2400 | 01, 02; model: BOARD_DESIGN_SETTINGS, BOARD_STACKUP, NET_SETTINGS | 1 | `Issue23625_CorruptedStackupCapped`, `Issue24133_DuplicatedStackupNotInflated`; D2 on boards without footprints |
| PCBIO-05 | Text, text boxes, render cache | P:775-951, 4587-4818, 4968-5186; W:573-598, 2619-2773; `EDA_TEXT::Format` | 900 | 01, 02; kicad-core EDA_TEXT; kicad-font | 1 | `test_pcb_textbox` (I/O parts), `FootprintUserFieldKeepsAutoThickness` |
| PCBIO-06 | Footprints | P:594-704, 954-1066, 6234-7317; W:1452-1884 | 1800 | 03, 05; model: Footprint, fields, transform | 1 | `test_footprint_load_save`, `LegacyFootprintPropertyTakesBoardDefaults`, `test_extruded_body_load_save`, `test_footprint_rescue_layer` |
| PCBIO-07 | Pads and padstacks | P:7320-8428; W:1972-2549 | 1700 | 06; model: Pad, Padstack | 1 | `test_pad_roundtrip`, `test_custom_pad_primitive_roundtrip`, `test_footprint_pad_fixup`, `test_pad_tenting`, `test_backdrill_properties` (legacy read cases), `RegressionSaveLoadTests` (padstacks*) |
| PCBIO-08 | Zones | P:3450-3501, 9610-10480; W:3499-3790 | 1250 | 01, 02; model: Zone; kicad-geom SHAPE_POLY_SET (inflate only for the stroked legacy path) | 1 | `Issue19775_ZoneLayerWildcards`, `Issue23125_EmptyZoneDiscarded`, `CopperThievingZone_*` |
| PCBIO-09 | Groups and deferred resolution | P:1798-1937, 8431-8441, 8489-8576; W:3002-3050 | 400 | 01, 02 | 1 | `test_group_load_save`, `group_saveload.cpp` (HealthyGroups, SingleMemberGroupsSaved, InvalidGroups) |
| PCBIO-10 | Dimensions | P:5766-6231; W:1067-1180 | 600 | 05; model: `pcb_dimension.cpp` `Update()` | 1 | `MalformedDimensionTextThrowsCleanly`, `test_dimension_zero_suppression`; D2 on the 40 boards with dimensions |
| PCBIO-11 | Net classes (legacy), net chains, variants, embedded files | P:538-591, 3647-3883, 1547-1591, 7142-7175; W:828-850, 962-1063; `embedded_files.cpp` parser/writer | 1100 | 01, 02; kicad-core EMBEDDED_FILES (zstd decode, MurmurHash3) | 1 (net chains: 2) | `net_chains/test_net_chain_*`, `test_variant` (file parts), boards with `embedded_files` in D2 |
| PCBIO-12 | Other items: reference image, barcode, target, point, grid item, tables, generated tables, drill map | P:4459-4584, 4821-4965, 5188-5764, 10483-10734; W:1334-1449, 2552-2616, 2776-2999 | 1800 | 05; model types | 2 | `test_reference_image_load`, `test_barcode_load_save`, `test_custom_property_roundtrip`, `test_drill_chart` (I/O parts) |
| PCBIO-13 | Generators and constraints | P:8444-8486, 8579-8948; W:3053-3238; `pcb_generator.cpp` 150-185; `pcb_tuning_pattern.cpp` 1941-2060; `pcb_via_stack.cpp` 989-1060; `pcb_via_stitch.cpp` 609-700 | 1000 | 09, 03 | 2 | `test_generator_load_save`, `test_via_stack_saveload` (`GoldenFileStillLoads`, `MemberlessStackIsNotSaved`, `PropertiesAndMembersRoundTrip`), `test_constraint_serialization`; D2 on the 24 boards with generators |
| PCBIO-14 | Footprint library files | P:1160-1274; W:80-260 (semantics only), 354-371, 4097-4209 (FootprintSave normalisation) | 500 | 06, 07 | 2 | `LegacyModuleFileImports`, `LibraryFootprintKeepsGroupsAndLoadsUnlocked`, `ParseWarningDoesNotFailTheLibrary`, `FootprintSave_OmitsNetsOnAllBoardConnectedItems`, `test_fp_lib_load_save`; D1 |
| PCBIO-15 | Load/save orchestration and re-save command | W:3809-3944; `pcb_io.cpp` 72-100; `board_loader.cpp`; `pcbnew_jobs_handler.cpp` 540-630, 3308-3352 | 500 | 01, 02 (+ kicad-connectivity for the full `initializeLoadedBoard`) | 1 | `test_board_loader` (`BoardLoaderWithInit/WithoutInit`), `RegressionSaveLoadTests`, D2 |
| PCBIO-16 | Append mode, layer remapping, undefined-layer rescue | P:2711-2806 and all `m_appendToExisting` branches; W:3850-3924 | 400 | all tier 1 | 3 | `Issue23752_*`, `Issue24642_*`, `Issue24955_*`, `Issue24911_*` |
| PCBIO-17 | Corpus harness and ported I/O tests | `qa/pcbnew_utils/board_file_utils.cpp`, `board_test_utils.cpp` (helpers); test files of 9.1 | (tests) | 02 for D0; grows with each package | 1 | D0-D3 runners, uuid canonicaliser, skip lists, small goldens |

---------------------------------------------------------------------------------------------------

## 11. Risks, traps, open questions

### 11.1 What can break "parse + write == oracle re-save"

- **R1 Random uuids.** Items without a uuid in the file (legacy files, mandatory fields that are
  missing, malformed ids) get random uuids in the oracle, different on every run, and uuid is a
  sort key. Byte equality is only achievable on inputs where every item already has a uuid
  (hence D2). The port must generate ids from an injected deterministic source; record as a
  deviation.
- **R2 Net codes drive track order** (4.6). `AppendNet`/`getFreeNetCode` and the order of first
  occurrence must be reproduced exactly, including legacy `(net N name)` declarations with gaps
  or duplicates and the zone `net_name` repair path.
- **R3 Oracle post-load steps** (4.3): net propagation by `BuildConnectivity`, project-file
  influence, `RefreshGeneratedTables`. Run the oracle on isolated copies; implement or classify.
- **R4 Comparators.** Any deviation in `PCB_SHAPE::Compare`, `PCB_TEXT::Compare`,
  `EDA_TEXT::Compare`, `StrNumCmp`, `LSET::Seq()` vector comparison or the `KICAD_T` numbering
  re-orders output. Port them literally, including which coordinates (library vs board frame)
  each one reads (`FOOTPRINT::cmp_drawings` uses library coordinates for shapes but
  `GetFPRelativePosition()` for texts).
- **R5 Number formatting.** `%.10g` vs the small-value `%.10f` branch; `-0` from
  `FormatDouble2Str( -0.0 )` (appears in 3D model rotations); doubles must be parsed and kept as
  `f64` without normalisation; integer truncation in `island_area_min`; angles are printed from
  whatever the model setter stored (setters such as `FOOTPRINT::SetOrientation`,
  `PAD::SetOrientation`, text angle setters normalise - the model must normalise identically).
- **R6 Derived state written back** (4.7): dimension text/geometry, footprint-child bake/un-bake
  through `TRANSFORM_TRS` (libm `sin`/`cos` differences between glibc and the wasm libm can flip
  a rounding for non-cardinal rotations - handle cardinal angles exactly like the C++ and test
  both targets), PTH pad layer expansion, pad drill un-baking, zone layer masking with enabled
  layers, text size clamps, zero-size pad fix, via `SanitizeLayers`.
- **R7 Outline fonts.** `(render_cache ...)` is re-emitted from the cached glyphs only while the
  cache key (resolved text incl. text variables, font, angle, mirror) is unchanged; board-level
  `gr_text` loses its cache at load (`SetVisible( true )` in `parsePCB_TEXT`, P:4656, calls
  `ClearRenderCache()`), so the oracle regenerates it from the host's fonts. The port cannot
  render outline fonts: keep the parsed polygons verbatim and re-emit them (documented
  deviation), and exclude such boards from byte comparison. `(face "...")` is written from the
  resolved font's name; without font resolution keep the unresolved name. `EmbedFonts()` becomes
  a no-op that keeps existing embedded font files; `ClearEmbeddedFonts()` must still be done
  when the flag is off.
- **R8 Legacy conversions needing heavy geometry**: stroked zone fills (`InflateWithLinkedHoles`,
  Clipper2-exact) and `fill_segments` - one corpus board; legacy arcs (centre/start/angle ->
  mid/end via trigonometry, 15 boards); `SHAPE_LINE_CHAIN::Append( arc )` degeneracy rules for
  zone/polygon outlines with arcs.
- **R9 Generators** (24 boards): properties are not stored as read; each type re-derives its
  property map from typed members (`GetProperties`), including defaults for absent keys and
  `set_iu`/`get_to_iu` scaling. A generic "keep the map" implementation will not be byte-exact.
- **R10 Prettify corner cases**: column counting in bytes, the 72/99 thresholds, backslash-quote
  parity, `(data` blocks, `#` comment lines, and the single trailing newline.
- **R11 Layer semantics**: `*.Cu` means all 32 copper layers on read but "all board copper
  layers" on write; `lookUpLayerSet` silently maps unknown names to `Rescue`; footprint layer is
  coerced to F.Cu/B.Cu; the stackup parser classifies copper by "even layer id".
- **R12 String handling**: every string is re-quoted with the minimal escape set, so the lexer's
  unescape must be the exact inverse superset (`\t`, `\xHH`, octal are read but written raw);
  invalid UTF-8 handling of `FromUTF8()`; `%V`/`%R` substitution; overbar conversion.
- **R13 Token-class dependence**: keyword-vs-string distinctions (5.1) are lost if the port
  first builds a generic tree without a "was quoted" flag. Use the pull-lexer design.
- **R14 Items the reader drops or rewrites** (zones without corners, non-copper tracks, hidden
  text -> fields, `Footprint` property, teardrop-zone uuids/names, missing mandatory fields being
  added) - all intentional C++ behaviour; do not "preserve" them.
- **R15 `via` drill**: vias with `UNDEFINED_DRILL_DIAMETER` are written with `GetDrillValue()`,
  which consults the net class (project or legacy board netclasses).
- **R16 Reference images**: the image bytes are stored and re-emitted verbatim, but the
  `< 20260623` scale migration needs PPI / legacy PPI extracted from the image header exactly as
  wxWidgets reports them (`common/bitmap_base.cpp`), and `%g` formatting of the scale.

### 11.2 Open questions for the orchestrator

- **Q1** Where does `BOARD_LOADER::initializeLoadedBoard` live? It needs `kicad-connectivity`
  (and creates the DRC engine), both above `kicad-pcb` in the crate graph. Proposal: `kicad-pcb`
  exposes the pure `io::load_board`/`save_board`; a small `board_loader` module in `kicad-cli`
  (shared with `kicad-wasm`) performs the post-load sequence.
- **Q2** Project files: is `.kicad_pro` loading (design settings, netclasses, text variables) in
  tier 1? It affects DRC far more than I/O; for I/O tests run the oracle without project files.
- **Q3** A bare "parse + write" C++ oracle for boards would make D2 independent of connectivity.
  If the oracle build tree has QA targets, a 20-line tool around `KI_TEST::ReadBoardFromFile` +
  `DumpBoardToFile` (or the existing `qa_pcbnew` save/load tests) would provide it.
- **Q4** Confirm the `generator_version` string and the default paper/layer output of the oracle
  build before freezing goldens.
- **Q5** Scope of generated tables (`drill_chart`, `stackup_table`, `drill_map`): parsing and
  writing them verbatim is tier 2; regenerating their content (`RefreshGeneratedTables`) is a
  separate feature and should stay out unless requested.
- **Q6** zstd: reading embedded files needs a decoder (pure Rust, decode only); nothing in the
  re-save path needs a compressor because the compressed payload is written back verbatim.
