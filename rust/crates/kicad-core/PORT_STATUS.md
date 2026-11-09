# kicad-core port status

Port of the KiCad `common/` basics: work package **C1** (identifiers and layers, the sections
below up to "Known gaps") and work package **C2** (units, number formatting, strings, colours,
text attributes, stroke, title block, page info: see "Work package C2" at the end).

Dependencies: `kicad-sexpr` (printf-exact double formatting, `OutputFormatter`, the stroke
lexer); dev-dependency `kicad-qa`. No `kicad-geom` dependency. Builds for
`wasm32-unknown-unknown`.

## C++ to Rust map

State: `ported`, `partial`, `stub`, `skipped: reason`.

### `kiid` (`include/kiid.h`, `common/kiid.cpp`) - `src/kiid.rs`

| C++ | Rust | State |
|---|---|---|
| `timestamp_t` | `kiid::Timestamp` (`u32`) | ported |
| `KIID` (16 byte `boost::uuids::uuid`) | `Kiid` (`Copy`, `Ord` = byte-wise like `boost::uuids::uuid::operator<`) | ported |
| `KIID::KIID()` | `Kiid::new( &mut KiidGenerator )` / `KiidGenerator::generate` | ported (explicit generator, see "UUID generation") |
| `KIID::KIID( int null )`, `niluuid`, `NilUuid()` | `Kiid::nil()`, `NILUUID`, `nil_uuid()` | ported |
| `KIID::KIID( const std::string& / const char* / const wxString& )` | `Kiid::from_string_or_random( s, &mut generator )`; the deterministic part is `Kiid::parse( s ) -> Option<Kiid>` (also `FromStr`) | ported |
| `KIID::KIID( timestamp_t )` | `Kiid::from_legacy_timestamp` | ported (see deviation 5) |
| `Clone( const KIID& )` | plain assignment (`Kiid` is `Copy`) | skipped: not needed |
| `Hash()` | `Kiid::hash_value` (Boost <= 1.85 `hash_value`, 64 bit) | ported |
| `FromName`, `FromBytes`, `AsBytes` | `from_name` (SHA-1 name based UUID, `src/sha1.rs`), `from_bytes`, `as_bytes` | ported |
| `IsLegacyTimestamp`, `AsLegacyTimestamp`, `AsString`, `AsStdString`, `AsLegacyTimestampString` | `is_legacy_timestamp`, `as_legacy_timestamp`, `as_string` (+ `Display`), `as_std_string`, `as_legacy_timestamp_string` | ported |
| `SniffTest` | `Kiid::sniff_test` | ported |
| `CreateNilUuids`, `SeedGenerator` | `KiidGenerator::create_nil_uuids`, `KiidGenerator::seeded` / `seed_generator` | ported |
| `ConvertTimestampToUuid` | `Kiid::convert_timestamp_to_uuid( &mut generator )` | ported |
| `Combine`, `FromDeterministicString`, `Increment` | `combine`, `from_deterministic_string`, `increment` | ported |
| `operator== != < >` | `PartialEq`, `Eq`, `PartialOrd`, `Ord` | ported |
| `KIID_PATH` (`std::vector<KIID>`) | `KiidPath( pub Vec<Kiid> )`, derefs to the `Vec` | ported |
| `KIID_PATH( const wxString& )` | `KiidPath::from_string_or_random( s, &mut generator )`; strict `KiidPath::parse( s ) -> Option<_>` (private port of `wxSplit`) | ported |
| `MakeRelativeTo`, `EndsWith`, `IsContainedWithin`, `AsString` | `make_relative_to`, `ends_with`, `is_contained_within`, `as_string` (+ `Display`) | ported |
| `KIID_PATH` `operator== < > += +` | `PartialEq`, `Ord` (size first, then elements), `AddAssign<&KiidPath>`, `Add<&KiidPath>` | ported |
| `KIID_NIL_SET_RESET` | `KiidNilSetReset` (RAII guard borrowing the generator) | ported |
| `std::hash<KIID>`, `std::hash<KIID_PATH>` | `Kiid::hash_value`, `KiidPath::hash_value`; both types also implement Rust `Hash` | ported |
| `to_json` / `from_json` | - | skipped: no JSON in this crate; they are `as_string()` / `from_string_or_random()` |
| `boost::mt19937` | `mt19937::Mt19937` (`src/mt19937.rs`) | ported (seed, seed-sequence seeding, `normalize_state`, `twist`, output) |
| `boost::uuids::string_generator`, `to_string`, `basic_random_generator<mt19937>`, `name_generator_sha1`, `hash_value` | private functions of `src/kiid.rs`, `src/sha1.rs` | ported (the parts KIID uses) |

### `layer_ids` (`include/layer_ids.h`, `common/layer_id.cpp`) - `src/layer_ids.rs`

| C++ | Rust | State |
|---|---|---|
| `enum PCB_LAYER_ID : int` | `PcbLayerId( pub i32 )` newtype with one associated constant per enumerator: `PcbLayerId::FCu`, `BCu`, `In1Cu` .. `In30Cu`, `FMask`, `BMask`, `FSilkS`, `BSilkS`, `FAdhes`, `BAdhes`, `FPaste`, `BPaste`, `DwgsUser`, `CmtsUser`, `Eco1User`, `Eco2User`, `EdgeCuts`, `Margin`, `BCrtYd`, `FCrtYd`, `BFab`, `FFab`, `Rescue`, `User1` .. `User45`, `UndefinedLayer`, `UnselectedLayer`, `PcbLayerIdCount` | ported (see "Design notes") |
| `UNDEFINED_LAYER`, `UNSELECTED_LAYER`, `PCB_LAYER_ID_COUNT`, `PCBNEW_LAYER_ID_START`, `MAX_CU_LAYERS`, `MAX_USER_DEFINED_LAYERS` | constants of the same name (`PCB_LAYER_ID_COUNT: i32 = 128`) | ported |
| `enum class FLASHING` | `Flashing` | ported |
| `LayerName( int )` | `layer_name` (English; `_()` strings untranslated) | ported |
| `IsValidLayer`, `IsPcbLayer`, `IsCopperLayer( int )`, `IsExternalCopperLayer`, `IsInnerCopperLayer`, `IsNonCopperLayer`, `IsSolderMaskLayer` | `is_valid_layer`, `is_pcb_layer`, `is_copper_layer`, `is_external_copper_layer`, `is_inner_copper_layer`, `is_non_copper_layer`, `is_solder_mask_layer` (all take `impl Into<i32>`, i.e. a `PcbLayerId` or an `i32`) | ported |
| `IsCopperLayer( int, bool )` | `is_copper_layer_incl_synthetic` | ported |
| `IsUserLayer`, `IsFrontLayer`, `IsBackLayer`, `IsCopperLayerLowerThan` | `is_user_layer`, `is_front_layer`, `is_back_layer`, `is_copper_layer_lower_than` | ported |
| `FlipLayer` | `flip_layer( layer, copper_layers_count )` (no default argument) | ported |
| `CopperLayerToOrdinal` | `copper_layer_to_ordinal` | ported |
| `BoardLayerFromLegacyId` | `board_layer_from_legacy_id` | ported |
| `ToLAYER_ID` (defined in `lset.cpp`) | `to_layer_id` | ported |
| `NETNAMES_LAYER_ID`, `GAL_LAYER_ID`, `SCH_LAYER_ID`, `GERBVIEW_LAYER_ID`, `LAYER_3D_ID` | plain `i32` constants with the C++ names (`LAYER_VIAS`, `LAYER_WIRE`, ...), `GAL_LAYER_ID_COUNT`, `SCH_LAYER_ID_COUNT`, `GERBER_DRAWLAYERS_COUNT`, `LAYER_ID_COUNT`, `GAL_UI_LAYER_COUNT`, `MIN_VISIBILITY_MASK` | ported (values only; GUI enums are not Rust types) |
| macros `NETNAMES_LAYER_INDEX`, `GAL_LAYER_INDEX`, `BITMAP_LAYER_FOR`, `ZONE_LAYER_FOR`, `PAD_COPPER_LAYER_FOR`, `VIA_COPPER_LAYER_FOR`, `CLEARANCE_LAYER_FOR`, `DRILL_SYMBOL_LAYER_FOR`, `BOARD_LAYER_FOR_DRILL_SYMBOL`, `POINT_LAYER_FOR`, `SCH_LAYER_INDEX`, `GERBER_DRAW_LAYER`, `GERBER_DCODE_LAYER`, `GERBER_DRAW_LAYER_INDEX` | functions with the snake_case name | ported |
| `IsViaPadLayer`, `IsHoleLayer`, `GetNetnameLayer`, `IsNetnameLayer`, `IsZoneFillLayer`, `IsPadCopperLayer`, `IsViaCopperLayer`, `IsClearanceLayer`, `IsDrillSymbolLayer`, `IsPointsLayer`, `IsDCodeLayer` | functions with the snake_case name | ported |
| `GAL_SET`, `GAL_BASE_SET`, `ToGalLayer`, `operator++` / `operator+` on `GAL_LAYER_ID` / `SCH_LAYER_ID` | - | skipped: GUI visibility state |
| `Map3DLayerToPCBLayer`, `MapPCBLayerTo3DLayer` | - | skipped: 3D viewer only |

### `layer_range` (`include/layer_range.h`) - `src/layer_range.rs`

| C++ | Rust | State |
|---|---|---|
| `LAYER_RANGE( start, stop, layer_count )` | `LayerRange::new -> Result<_, LayerRangeError>` (the `std::invalid_argument` case) | ported |
| `begin()` / `end()` iteration, `LAYER_RANGE_ITERATOR` | `LayerRange::iter`, `IntoIterator` | ported (see deviation 3) |
| static `Contains( int, int, int )`, member `Contains( int )`, `size()` | `LayerRange::contains_range`, `contains`, `size` | ported |

### `lseq` (`include/lseq.h`, `common/lseq.cpp`) - `src/lseq.rs`

| C++ | Rust | State |
|---|---|---|
| `LSEQ` (`std::vector<PCB_LAYER_ID>`) | `Lseq( pub Vec<PcbLayerId> )`, derefs to the `Vec`; `From<[PcbLayerId; N]>`, `FromIterator` | ported |
| `LSEQ::TestLayers` | `Lseq::test_layers` | ported |

### `base_set` (`include/base_set.h`, `thirdparty/dynamic_bitset`) - `src/base_set.rs`

| C++ | Rust | State |
|---|---|---|
| `BASE_SET( size_t size = 64 )` | `BaseSet::new( size )`, `Default` (64 bits) | ported |
| `set( pos )`, `set( pos, value )`, `set()` | `set`, `set_value`, `set_all` | ported |
| `reset( pos )`, `reset()`, `flip( pos )`, `flip()` | `reset`, `reset_all`, `flip`, `flip_all` | ported |
| `operator~ &= |= ^= & | ^` (with size mismatch handling) | `Not`, `BitAndAssign`, `BitOrAssign`, `BitXorAssign`, `BitAnd`, `BitOr`, `BitXor` for values and references | ported |
| `compare`, `operator<` | `BaseSet::compare`, `Ord` | ported (see deviation 7) |
| `operator==` (`sul::dynamic_bitset`: same size and same bits) | `PartialEq` / `Eq` | ported |
| `FmtBin`, `FmtHex`, `ParseHex( std::string )`, `ParseHex( const char*, int )` | `fmt_bin`, `fmt_hex`, `parse_hex`, `parse_hex_bytes` | ported |
| `begin()`..`end()` (bits), `set_bits_begin()`.., `set_bits_rbegin()`.. | `iter` (also `IntoIterator for &BaseSet`), `set_bits`, `set_bits_rev` | ported |
| `std::hash<BASE_SET>` | `BaseSet::hash_value` (Rust `Hash` is implemented separately) | ported |
| `sul::dynamic_bitset`: `size`, `empty`, `num_blocks`, `resize`, `clear`, `test`, `test_set`, `all`, `any`, `none`, `count`, `operator[]` (read), `is_subset_of`, `is_proper_subset_of`, `intersects`, `find_first`, `find_next`, `to_string`, `operator<<` | `size`, `is_empty`, `num_blocks`, `resize` / `resize_with_value`, `clear`, `test`, `test_set`, `all`, `any`, `none`, `count`, `Index<usize>`, `is_subset_of`, `is_proper_subset_of`, `intersects`, `find_first`, `find_next`, `to_bit_string`, `Display` | ported |
| rest of `sul::dynamic_bitset` (`push_back`, `append`, shifts, `operator-=`, range `set`/`reset`/`flip`, `to_ulong`, numeric `operator<`, stream input, ...) | - | skipped: not used by KiCad through `BASE_SET` |

`BitPos` is the trait standing in for the implicit conversion of `PCB_LAYER_ID` / `int` /
`size_t` to a bit position; it is implemented for `usize`, `i32`, `u32` and `PcbLayerId`.

### `lset` (`include/lset.h`, `common/lset.cpp`) - `src/lset.rs`

| C++ | Rust | State |
|---|---|---|
| `LSET()` | `Lset::new()` / `Default` (128 cleared bits) | ported |
| `LSET( const BASE_SET& )` | `Lset::from_base_set`, `From<BaseSet>` | ported |
| `LSET( std::initializer_list )`, `LSET( const std::vector& )` | `Lset::from_layers( &[..] )`, `From<[PcbLayerId; N]>`, `From<&[PcbLayerId]>`, `FromIterator` | ported |
| `LSET( const LSEQ& )`, `LSET( const LAYER_RANGE& )` | `Lset::from_lseq`, `Lset::from_layer_range` | ported |
| `Contains`, `ContainsAll` | `contains`, `contains_all` | ported |
| `Name`, `NameToLayer` | `Lset::name`, `Lset::name_to_layer` (private port of `wxString::ToLong`) | ported |
| `IsBetween` | `Lset::is_between` | ported, bug-compatible (see "C++ quirks reproduced") |
| `InternalCuMask`, `FrontAssembly`, `BackAssembly`, `AllCuMask()`, `ExternalCuMask`, `AllLayersMask`, `FrontTechMask`, `FrontBoardTechMask`, `BackTechMask`, `BackBoardTechMask`, `AllTechMask`, `AllBoardTechMask`, `FrontMask`, `BackMask`, `SideSpecificMask`, `UserMask`, `PhysicalLayersMask` | functions with the snake_case name returning `&'static Lset` | ported |
| `AllCuMask( int )`, `AllNonCuMask()`, `UserDefinedLayersMask( int )` | `all_cu_mask_n`, `all_non_cu_mask`, `user_defined_layers_mask` returning `Lset` | ported |
| `CuStack`, `TechAndUserUIOrder`, `UIOrder`, `Seq()`, `Seq( const LSEQ& )`, `SeqStackupTop2Bottom`, `SeqStackupForPlotting` | `cu_stack`, `tech_and_user_ui_order`, `ui_order`, `seq`, `seq_ordered`, `seq_stackup_top2bottom( selected )`, `seq_stackup_for_plotting` | ported |
| `RunOnLayers`, `ExtractLayer`, `FlipStandardLayers`, `LayerCount`, `ClearCopperLayers`, `ClearNonCopperLayers`, `ClearUserDefinedLayers` | `run_on_layers`, `extract_layer`, `flip_standard_layers( count )`, `layer_count`, `clear_copper_layers`, `clear_non_copper_layers`, `clear_user_defined_layers` | ported |
| `begin()`..`end()` (`all_set_layers_iterator`) | `Lset::iter`, `IntoIterator for &Lset` | ported |
| `copper_layers_begin()`.., `non_copper_layers_begin()`.. | `copper_layers()`, `non_copper_layers()` | ported (see deviation 2) |
| `GAL_SET::GAL_SET`, `GAL_SET::Seq`, `GAL_SET::DefaultVisible` | - | skipped: GUI visibility state |
| `LSET::ForbiddenFootprintLayers`, `LSET::Flip` | - | not present in this version of the C++ source |

### `legacy_layers` - `src/legacy_layers.rs`

| C++ | Rust | State |
|---|---|---|
| `LEGACY_PCB_LAYER_ID`, `s_legacyLayerIdMap`, `remapLegacyLayerLSET` (`pcbnew/pcb_plot_params.cpp`) | `LEGACY_PCB_LAYER_ID_COUNT`, `remap_legacy_layer_lset` | ported |
| `.brd` layer numbers, `is_leg_copperlayer_valid`, `PCB_IO_KICAD_LEGACY::leg_layer2new`, `leg_mask2new` (`pcbnew/pcb_io/kicad_legacy/pcb_io_kicad_legacy.cpp`) | constants (`LAYER_N_BACK`, ... `ALL_CU_LAYERS`), `is_leg_copperlayer_valid`, `leg_layer2new`, `leg_mask2new` | ported (the rest of the legacy `.brd` reader is out of scope) |

### `lib_id` (`include/lib_id.h`, `common/lib_id.cpp`, `include/name_validation.h`) - `src/lib_id.rs`

| C++ | Rust | State |
|---|---|---|
| `UTF8` (`libs/core/include/core/utf8.h`) | `String` / `&str`; byte iteration = `bytes()`, `uni_iter` = `chars()` | ported as a mapping (no type) |
| `GetLibIdForbiddenChars`, `GetLibFilenameForbiddenChars` | `get_lib_id_forbidden_chars`, `get_lib_filename_forbidden_chars` | ported |
| `LIB_ID()`, `LIB_ID( lib, item )` | `LibId::new`, `LibId::from_parts` | ported |
| `Parse( id, fix )` | `LibId::parse( id, fix ) -> i32` | ported |
| `GetLibNickname`, `GetUniStringLibNickname`, `SetLibNickname`, `GetLibItemName`, `GetUniStringLibItemName`, `SetLibItemName`, `GetSubLibraryName`, `GetUniStringSubLibraryName`, `SetSubLibraryName`, `GetFullLibraryName` | `lib_nickname`, `set_lib_nickname`, `lib_item_name`, `set_lib_item_name`, `sub_library_name`, `set_sub_library_name`, `full_library_name` | ported |
| `Format()`, `GetUniStringLibId()`, static `Format( lib, item )` | `LibId::format` (+ `Display`), `LibId::format_parts -> Result<String, LibIdParseError>` | ported |
| `IsValid`, `IsLegacy`, `clear`, `empty`, `compare`, `operator< > == !=` | `is_valid`, `is_legacy`, `clear`, `is_empty`, `compare`, `Ord` / `Eq` | ported |
| `HasIllegalChars`, `FixIllegalChars`, `FindIllegalLibraryNameChar`, `isLegalChar`, `isLegalLibraryNameChar` | `has_illegal_chars`, `fix_illegal_chars`, `find_illegal_library_name_char`, `is_legal_char`, `is_legal_library_name_char` | ported |
| `PARSE_ERROR` thrown by `Format` | `LibIdParseError` (same message text) | ported |

### `typeinfo` (`include/core/typeinfo.h`) - `src/typeinfo.rs`

| C++ | Rust | State |
|---|---|---|
| `enum KICAD_T` | `#[repr(i32)] enum KicadT` (`PCB_FOOTPRINT_T` is `KicadT::PcbFootprintT`), same values, `KicadT::from_i32`, `as_i32`, `TryFrom<i32>` | ported |
| `BaseType`, `IsNullType`, `IsGeneratedTableType`, `IsInstantiableType`, `IsEeschemaType`, `IsPcbnewType`, `IsSingleLayerType`, `IsGerbviewType`, `IsPageLayoutEditorType`, `IsMiscType`, `IsTypeCorrect` | `const fn` methods of `KicadT` with the snake_case name | ported |
| `IsA`, `dyn_cast` | - | skipped: C++ RTTI helpers |

### `report_severity` - `src/report_severity.rs`

| C++ | Rust | State |
|---|---|---|
| `enum SEVERITY` (`include/widgets/report_severity.h`) | `#[repr(i32)] enum Severity` (`RPT_SEVERITY_ERROR` is `Severity::Error`), same bit values | ported |
| `formatSeverities` (`common/widgets/report_severity.cpp`) | `format_severities( i32 )` | ported |
| `SeverityFromString`, `SeverityToString` (`common/widgets/ui_common.cpp`) | `severity_from_string`, `severity_to_string` | ported |
| `RC_ITEM::getSeverityString` (`common/rc_item.cpp`) | `get_severity_string` | ported |

### Not in this crate

| C++ | Where | Why |
|---|---|---|
| `FLIP_DIRECTION`, `MIRROR`, `MIRRORVAL` (`core/mirror.h`) | `kicad_geom::kicore::mirror` | the geometry crate needs them and cannot depend on this crate; not duplicated here |

## Design notes

### `PcbLayerId` is a newtype, not an enum

`PCB_LAYER_ID` is an unscoped enum with underlying type `int` and the C++ code relies on that:
`PCB_LAYER_ID( In1_Cu + 2 * i )`, `ToLAYER_ID( int )`, loops over all ids, `B_Cu` encoded as
`INT_MAX` in the connectivity R-tree, and `LAYER_RANGE` yielding ids up to 254 (required by
the ported test `MaxLayerCount`). A Rust `enum` cannot hold those values, so the type is
`#[repr(transparent)] struct PcbLayerId( pub i32 )` with associated constants. `PcbLayerId::FCu`
reads like an enum variant and works in patterns (`match` needs a `_` arm, like the C++
`default:`). `Debug` prints the enumerator name (`FCu`, `In3Cu`, `User12`) or
`PcbLayerId(254)`. `Default` is `F_Cu` (value 0).

### UUID generation

C++ keeps thread-local globals (`rng`, `randomGenerator`, `g_createNilUuids`) and seeds from
OS entropy. The port has an explicit, injectable `KiidGenerator` instead (no global state, no
OS access):

* `KiidGenerator::seeded( seed )`: deterministic; reproduces the id sequence of the C++ code
  after `KIID::SeedGenerator( seed )` exactly (checked against the C++ implementation for
  several seeds). `boost::mt19937` is implemented in `src/mt19937.rs`.
* `KiidGenerator::from_entropy( |buf| .. )`: non-deterministic; the caller fills
  `KiidGenerator::ENTROPY_BYTES` (2496) random bytes that seed the whole Mersenne Twister
  state, as `basic_random_generator`'s default constructor does from the OS.
* `KiidGenerator::default()` is `seeded( 5489 )` (the default seed of `boost::mt19937`):
  deterministic as PORTING.md section 3 requires. Two default generators produce the same ids,
  so hosts (CLI, wasm) should use `from_entropy` before creating items that get saved.
* How a random id is built (`basic_random_generator<mt19937>::operator()`): four consecutive
  32 bit outputs, each stored little-endian, then `byte[8] = (byte[8] & 0xBF) | 0x80` and
  `byte[6] = (byte[6] & 0x4F) | 0x40`. This is what Boost <= 1.85 produces on LP64 and LLP64
  (through `uniform_int<unsigned long>`), and what Boost >= 1.86 produces on little-endian
  machines.
* `Kiid::default()` is the nil id (so item structs can derive `Default`); the C++ default
  constructor (a fresh random id) is `Kiid::new( &mut generator )`.

### `BaseSet` storage

`sul::dynamic_bitset` always allocates. `BaseSet` stores up to two 64 bit blocks inline (an
`LSET` is 128 bits) and only uses the heap for larger sets. This is not observable.

### Static masks

The `LSET` mask factories that return `const LSET&` to a function-local static return
`&'static Lset` (lazily initialised with `std::sync::OnceLock`).

## Ported C++ tests

All in `tests/`, same names in snake_case, same data and expectations.

| C++ test file | Rust test file | Tests |
|---|---|---|
| `qa/tests/common/test_kiid.cpp` | `tests/test_kiid.rs` | 9 (`Seeding`, `KiidPathTest`, `LegacyTimestamp`, `CombineDeterministic`, `ByteRoundTrip`, `CombineWithKnownValues`, `CombineUniqueness`, `FromDeterministicStringIsStable`, `FromDeterministicStringPinnedOutput`) |
| `qa/tests/common/test_layer_ids.cpp` | `tests/test_layer_ids.rs` | 3 (`LseqTestLayers`, `CopperLayers`, `FlipLset`) |
| `qa/tests/common/test_layer_range.cpp` | `tests/test_layer_range.rs` | 14 |
| `qa/tests/common/test_lset.cpp` | `tests/test_lset.rs` | 16 |
| `qa/tests/common/test_base_set.cpp` | `tests/test_base_set.rs` | 15 |
| `qa/tests/common/test_lib_id.cpp` | `tests/test_lib_id.rs` | 8 |

65 ported tests. `InvalidLayerThrowsException` checks `Err( LayerRangeError )` instead of a
thrown `std::invalid_argument`.

## Additional tests

* `tests/test_cpp_oracle.rs`: differential test. `tests/oracle/core_oracle.cpp` is compiled
  against the real `common/lset.cpp`, `layer_id.cpp`, `lseq.cpp`, `kiid.cpp`, `lib_id.cpp`
  (by `tests/oracle/build_oracle.sh`) and dumps about 2150 lines of results
  (`tests/golden/core_oracle_dump.txt`): layer names and predicates for every id up to
  `LAYER_ID_COUNT`, `NameToLayer` for 320 strings, every static mask, sequences, flips, set
  operators, hashes and comparisons for 33 `LSET`s and 30 `BASE_SET`s of other sizes,
  `ParseHex` edge cases, `LayerCount` / `IsBetween` grids, 188 `LAYER_RANGE`s, seeded `KIID`
  sequences, 42 `KIID` strings, nil mode, `Combine` / `Increment` / ordering, `FromName`,
  `FromDeterministicString`, `KIID_PATH` parsing and relations, and `LIB_ID` parsing, fixing,
  formatting and comparison for 33 strings. The Rust test regenerates the dump and compares
  line by line.
* `tests/test_legacy_layers.rs` + `tests/oracle/legacy_oracle.cpp`: the same for
  `remapLegacyLayerLSET`, `leg_layer2new`, `leg_mask2new` (their source text is cut out of the
  pcbnew files by the build script); dump in `tests/golden/legacy_layers_dump.txt`.
* `tests/test_typeinfo.rs` + `tests/oracle/typeinfo_values.cpp` +
  `tests/golden/typeinfo_values.txt`: every `KICAD_T` value
  and every classification helper against a dump of the C++ header.
* `tests/test_layer_ids.rs` + `tests/oracle/layer_ids_values.cpp` +
  `tests/golden/layer_ids_values.txt`: the 250 GUI layer enum values against a dump of the
  C++ header.
* Unit tests in the modules for what the dumps do not reach (Mersenne Twister reference
  values, SHA-1 vectors, heap storage of large sets, `wxString::ToLong` / `wxSplit`
  semantics, severities, ...).

The golden dumps were made with Boost 1.83 and wxWidgets 3.2.4 on x86-64 Linux;
`tests/oracle/build_oracle.sh [outdir] regen` rebuilds the C++ programs and regenerates them.

## Deviations

1. **Bad positions in bit sets.** `BaseSet::test` returns `false` for a position outside the
   set, and `set` / `set_value` / `reset` / `flip` ignore negative positions (for example
   `UNDEFINED_LAYER`). C++ asserts, respectively converts the negative value to a huge
   `size_t` (undefined behaviour or an allocation failure). `is_subset_of` /
   `is_proper_subset_of` treat missing bits of the shorter operand as cleared instead of
   asserting equal sizes.
2. **Copper layer iteration of odd sized sets.** `LSET::copper_layers_iterator` never reaches
   its end iterator when the set has an odd number of bits (the end index is rounded up to an
   even number), so `CuStack()` etc. loop until memory is exhausted. The port stops at the end
   of the set. Sets of even size (64, 128, ...) behave identically.
3. **`LAYER_RANGE` with an unreachable stop layer.** When the stop layer is not part of the
   stack described by the layer count (for example `LAYER_RANGE( F_Cu, In5_Cu, 4 )`) the C++
   iterator cycles or runs off forever. The port ends the iteration at the end of the stack.
   Ranges inside the stack are identical, including the C++ quirk that
   `LAYER_RANGE( B_Cu, F_Cu, 2 )` is empty.
4. **`wxCHECK` / `wxASSERT`.** `CopperLayerToOrdinal` and `LSET::LayerCount` reproduce the
   `wxCHECK` early return without asserting; `ToLAYER_ID` does not assert. Layer numbers come
   from files and library code must not panic on bad input.
5. **`KIID( timestamp_t )`.** With Boost < 1.86 the C++ constructor leaves the first 12 bytes
   uninitialised (so `IsLegacyTimestamp()` may even return false). The port zero-fills them,
   which is the intent and what Boost >= 1.86 does.
6. **`KIID::Hash`.** Ported from Boost <= 1.85 with a 64 bit `size_t`; Boost >= 1.86 and 32 bit
   platforms compute other values. Only meant for in-memory hashing.
7. **`BASE_SET` ordering.** C++ defines only `operator<` (bit-lexicographic from bit 0); `a > b`
   silently resolves to the unrelated numeric comparison of `sul::dynamic_bitset`. The port
   uses the `operator<` / `compare()` ordering for all comparison operators.
8. **`LIB_ID::compare`** returns exactly -1 / 0 / 1 (C++ returns whatever
   `std::string::compare` returns; only the sign is specified).
9. **`LIB_ID::GetFullLibraryName`** formats the names through the C library locale in C++
   (`wxString::Format( "%s", const char* )`); the port treats them as UTF-8, which is the C++
   result in a UTF-8 locale.
10. **`KIID::FromDeterministicString`** hashes Unicode code points (C++ on platforms with a
    32 bit `wxChar`). Windows builds of KiCad hash UTF-16 code units and differ for characters
    outside the BMP.
11. **`wxString::ToLong` in `LSET::NameToLayer`**: leading white space is the C locale set
    (C++ uses `iswspace` of the current locale); `long` is 64 bit (LP64).
12. **Translations.** `LayerName` returns the English strings.
13. **GUI layer enums** are plain `i32` constants with their C++ names instead of Rust enum
    types (they are only used as integers).

## C++ quirks reproduced (bug compatibility)

* `LSET::IsBetween`: `std::numeric_limits<PCB_LAYER_ID>::max()` is 0 (not specialised for
  enums), so no inner layer is "between" `F_Cu` and `B_Cu`.
* `LSET( const LAYER_RANGE& )` starts from the 64 bit `BASE_SET` default size, not 128 bits.
* `BASE_SET::ParseHex` counts a terminating non-hex byte as consumed and then replaces the set
  (for example `"zz"` clears it and returns 1).
* `BASE_SET::reset( pos )` grows the set when `pos` is beyond its size.
* `KIID_PATH::MakeRelativeTo` leaves the path empty when the argument is longer.
* `KIID( "" )` and any other invalid string give a *random* id (`from_string_or_random`), even
  in "create nil UUIDs" mode.
* `LSET::NameToLayer( "In1" )` is `In1_Cu` (`wxString::RemoveLast( 3 )` on a shorter string
  removes nothing).
* `IsNonCopperLayer( UNDEFINED_LAYER )` is true (parity test only).
* `LSET::LayerCount` is ported statement by statement, including its uneven counting
  (`LayerCount( F_Cu, In1_Cu, n )` is 1, `LayerCount( In1_Cu, In2_Cu, n )` is 1,
  `LayerCount( F_Cu, B_Cu, n )` is `n`).

## Known gaps

* Units, number formatting, string utilities, text attributes, colours: later work package.
* `GAL_SET`, `Map3DLayerToPCBLayer`, `MapPCBLayerTo3DLayer` (GUI / 3D viewer).
* JSON conversion of `KIID` (`to_json` / `from_json`): trivial wrappers, left to the crate
  that owns JSON.
* The seeded id sequence is verified against Boost 1.83; a KiCad built on a big-endian machine
  with Boost >= 1.86 would produce byte-swapped ids.
* A Rust program that creates items draws ids in its own order; reproducing the ids of a
  seeded C++ *session* additionally requires constructing items in the same order as C++
  (every `EDA_ITEM` constructor draws one id).


---

# Work package C2: units, number formatting, strings, shared value types

## C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `include/base_units.h`: `EDA_IU_SCALE` (`IUTomm`, `mmToIU`, `MilsToIU`, `IUToMils`, `IUToNm`, `NmToIU`, `NmToIUD`), `pcbIUScale`, `schIUScale`, `gerbIUScale`, `drawSheetIUScale`, `unityScale`, `*_IU_PER_MM`, `ARC_LOW_DEF(_MM)`, `ARC_HIGH_DEF(_MM)` | `base_units::EdaIuScale` (`const fn`s), `PCB_IU_SCALE`, `SCH_IU_SCALE`, `GERB_IU_SCALE`, `DRAW_SHEET_IU_SCALE`, `UNITY_SCALE`, same constants | ported (out-of-range double to int saturates instead of UB) |
| `libs/core/include/core/minoptmax.h` `MINOPTMAX<T>` | `minoptmax::MinOptMax<T>` | ported |
| `printf` / `{fmt}` `%f %e %g` | `kicad_core::printf` = re-export of `kicad_sexpr::formatter::cformat` | reused, not duplicated |
| `EDA_UNITS`, `EDA_DATA_TYPE` | `eda_units::EdaUnits`, `EdaDataType` (`#[repr(i32)]`, C++ values, `ALL`) | ported |
| `EDA_UNIT_UTILS::IsImperialUnit`, `IsMetricUnit`, `Mm2mils`, `Mils2mm`, `Mils2IU`, `FetchUnitsFromString`, `GetText`, `GetLabel`, `GetScaleForInternalUnitType` | `eda_units::is_imperial_unit`, `is_metric_unit`, `mm2mils`, `mils2mm`, `mils2iu`, `fetch_units_from_string -> Option`, `get_text`, `get_label`, `get_scale_for_internal_unit_type` | ported |
| `FormatAngle( EDA_ANGLE )` | `format_angle( degrees: f64 )` | ported |
| `FormatInternalUnits( scale, int, type )`, `( scale, VECTOR2I )` | `format_internal_units`, `format_internal_units_distance`, `format_internal_units_point( scale, x, y )` | ported |
| `ParseInternalUnits` x2 (`#if 0` in C++) | `parse_internal_units`, `parse_internal_units_point` | ported (semantics of the disabled code) |
| `UI::ToUserUnit`, `FromUserUnit`, `StringFromValue`, `MessageTextFromValue` (double / int / long long / EDA_ANGLE), `MessageTextFromMinOptMax` | `eda_units::ui::to_user_unit`, `from_user_unit`, `string_from_value`, `message_text_from_value`, `message_text_from_value_int`, `message_text_from_value_i64`, `message_text_from_angle( degrees, bool )`, `message_text_from_min_opt_max` | ported |
| `UI::DoubleValueFromString` x3, `UI::ValueFromString` x2, static `removeTrailingZeros` | `double_value_from_string`, `double_value_from_string_unscaled`, `double_value_from_string_with_units -> Option`, `value_from_string`, `value_from_string_unscaled`, private `remove_trailing_zeros` | ported |
| `string_utils`: `ESCAPE_CONTEXT`, `EscapeString`, `UnescapeString` | `EscapeContext`, `escape_string`, `unescape_string` | ported (all 11 contexts) |
| `ConvertToNewOverbarNotation`, `ConvertSmartQuotesAndDashes`, `TitleCaps`, `InitialCaps`, `wxStringSplit`, `ReadDelimitedText` x2, `EscapedUTF8`, `EscapeHTML`, `UnescapeHTML`, `RemoveHTMLTags`, `NoPrintableChars`, `PrintableCharCount`, `StrPurge` | same names in snake_case (`read_delimited_text`, `read_delimited_text_buf` on byte slices) | ported (`UnescapeHTML`'s regex matched by hand) |
| `CMP_T`, `StrNumCmp`, `StrNumSort`, `WildCompareString`, `ValueStringCompare`, `QuacksLikeADuck`, `SplitString`, `GetTrailingInt`, file-local `ApplyModifier`, `convertSeparators`, `NUMERIC_EVALUATOR::IsOldSchoolDecimalSeparator` | `CmpT`, `str_num_cmp`, `str_num_sort`, `wild_compare_string`, `value_string_compare`, `quacks_like_a_duck -> Option<f64>`, `split_string -> (beg, digits, end)`, `get_trailing_int`, `apply_modifier`, `convert_separators`, `is_old_school_decimal_separator` | ported |
| `IsFullFileNameValid`, `GetIllegalFileNameWxChars`, `ReplaceIllegalFileNameChars` (std::string / wxString) | `is_full_file_name_valid` (Linux forbidden set `*?`), `get_illegal_file_name_wx_chars`, `replace_illegal_file_name_chars_std`, `replace_illegal_file_name_chars` | ported |
| `StripTrailingZeros`, `FormatDouble2Str`, `UIDouble2Str`, `FormatTrimmedDecimal`, `wxString::FromCDouble` | `strip_trailing_zeros`, `format_double2_str`, `ui_double2_str`, `format_trimmed_decimal`, `wx_from_c_double` | ported |
| `From_UTF8` x2, `TO_UTF8` | identity on `&str`; `from_utf8_bytes` for raw bytes | ported |
| `rsort_wxString`, `split`, `AccumulateDescription(s)` | `rsort_wx_string_less`, `split`, `accumulate_description(s)` | ported |
| `ExpandStackedPinNotation`, `CountStackedPinNotation`, `EscapeStackedPinItem`, `SplitStackedPinDisplayItems` (+ file-local helpers) | same in snake_case; the `bool* aValid` out-parameter is returned in a tuple | ported |
| `IsAutoGeneratedNetName`, `GetDefaultVariantName`, `GetVariantFileToken`, `ExpandVariantOutputPath`, `SortVariantNames` | same in snake_case | ported |
| `SplitGraphemeClusters` | - | skipped: needs harfbuzz Unicode tables (general category, combining class) |
| `LinkifyHTML`, `IsURL` | - | skipped: `wxRegEx` with `\w`, `\b`, `\s`; UI only |
| `GetLine` (`FILE*`), `GetISO8601CurrentDateTime` (clock), `NormalizeFileUri`, `ConvertPathToFileUri` (file system), `ConvertMarkdown2Html`, `ExtractLibraryLoadException`, `PrettyPrintForMenu` | - | skipped: I/O, clock or UI (PORTING.md 5) |
| `EDA_COLOR_T`, `StructColors`, `colorRefs()` | `color4d::EdaColorT` (newtype with the enumerators as consts), `StructColors`, `color_refs()` | ported |
| `KIGFX::COLOR4D`: constructors (RGBA, `EDA_COLOR_T`, `wxString`, `wxColour`), `FromCSSRGBA`, `SetFromWxString`, `ToCSSString`, `SetFromHexString`, `ToHexString`, `ToColour`, `LegacyMix`, `ToHSL`, `FromHSL`, `ToHSV`, `FromHSV`, `Saturate`, `Desaturate`, `Brighten(ed)`, `Darken(ed)`, `Invert(ed)`, `Mix`, `WithAlpha`, `GetBrightness`, `Distance`, `Compare`, `RelativeLuminance`, `ContrastRatio`, `FindNearestLegacyColor`, `UNSPECIFIED`/`WHITE`/`BLACK`/`CLEAR`, `operator== != <`, `operator<<`, `to_json`/`from_json` | `Color4D` (`new`, `from_eda_color` / `From<EdaColorT>`, `from_string`, `from_wx_colour`, `from_css_rgba`, `set_from_wx_string`, `to_css_string`, `set_from_hex_string`, `to_hex_string`, `to_colour -> WxColour`, `legacy_mix`, `to_hsl`, `from_hsl`, `to_hsv`, `from_hsv`, `saturate`, ..., `distance`, `compare`, `less_than`, `PartialEq`, `PartialOrd`, `Display`, `to_json_string`, `from_json_string`) | ported (`m_text` is `Option<String>`, so `Color4D` is `Clone`, not `Copy`) |
| `wxColour::Set( wxString )` on wxGTK 3.2: `gdk_rgba_parse` + `pango_color_parse` + `wxColourBase::FromString` | `color4d::wx_colour_from_string` (+ private ports) | partial: colour names not supported (see gaps) |
| `std::hash<COLOR4D>` | `impl Hash for Color4D` (same components, different function) | partial |
| `GR_TEXT_H_ALIGN_T`, `GR_TEXT_V_ALIGN_T` (values -1/0/1/2), `GetFlippedAlignment` x2, `ToHAlignment` | `text_attributes::GrTextHAlignT`, `GrTextVAlignT`, `.flipped()`, `GrTextHAlignT::from_sign` | ported |
| `TEXT_ATTRIBUTES` (fields, constructor, `Compare`, `== < >`) | `TextAttributes` (`font: Option<String>` = font name, `angle: f64` degrees normalised like `EDA_ORIENTATION::SetAngle` via `set_angle`, `size: (i32, i32)`), `compare`, `PartialEq`, `PartialOrd` | ported |
| `operator<<( TEXT_ATTRIBUTES )`, `std::hash<TEXT_ATTRIBUTES>` | - | skipped: debug output / hash of a font pointer |
| `LINE_STYLE`, `WIRE_STYLE`, `lineTypeNames`, `DEFAULT_LINE_STYLE_LABEL`, `DEFAULT_WIRE_STYLE_LABEL`, `INDETERMINATE_STYLE` | `stroke_params::LineStyle`, `WireStyle`, `LINE_TYPE_NAMES` (names only, no bitmaps), constants | ported |
| `STROKE_PARAMS` (accessors, `==`, `<`, `Format`, `GetLineStyleToken`, `GetMsgPanelInfo`) | `StrokeParams` (`format( &mut dyn OutputFormatter, &EdaIuScale )`, `get_msg_panel_info( scale, units, .. ) -> Vec<(label, text)>`) | ported |
| `STROKE_PARAMS_PARSER::ParseStroke`, `parseInt`, `parseDouble` | `StrokeParamsParser::parse_stroke( &mut StrokeParamsLexer, &mut StrokeParams )` | ported (use with `DsnLexer::with_sub_lexer`) |
| `STROKE_PARAMS::Stroke` (dash geometry) | - | not in this package (needs shapes; belongs with `kicad-geom` users) |
| `TITLE_BLOCK` (all accessors, `Clear`, `GetStrings`, `Format`, `GetContextualTextVars`, `TextVarResolver`) | `title_block::TitleBlock`; `text_var_resolver( &mut String, &TitleBlockClock )` | ported (date/time passed in; no project `ExpandTextVars`) |
| `TITLE_BLOCK::GetCurrentDate`, `GetCurrentTimeHHMMSS`, `GetCurrentTimeLocale` | fields of `TitleBlockClock` supplied by the caller | replaced (no clock in library crates) |
| `PAGE_SIZE_TYPE`, `PAGE_INFO` (both `SetType`, `GetTypeAsString`, sizes in mils / mm / IU, `SetPortrait`, `IsCustom`, `IsDefault`, `Format`, `GetPageFormatsList`, `MIN/MAX_PAGE_SIZE_*`), `wxPaperSize` ids | `page_info::PageSizeType`, `PageInfo`, `WxPaperSize` (wx 3.2 values), constants | ported (see deviation C2-2) |

## Ported C++ tests (C2)

| C++ test file | Rust | Notes |
|---|---|---|
| `qa/tests/common/test_format_units.cpp` | `tests/test_format_units.rs` | run for all three scales (EESCHEMA/GERBVIEW/PCBNEW `#ifdef`s) |
| `qa/tests/common/test_string_utils.cpp` | `tests/test_string_utils.rs` | all cases except `ConvertPathToFileUri_*` (file system) and `SplitGraphemeClustersKeepsCharactersWhole` (not ported) |
| `qa/tests/common/test_kicad_string.cpp` | `tests/test_kicad_string.rs` | all cases |
| `qa/tests/common/test_color4d.cpp` (+ `color4d_test_utils.h`) | `tests/test_color4d.rs` | all cases; hash checks use the Rust `Hash` |
| `qa/tests/common/test_text_attributes.cpp` | `tests/test_text_attributes.rs` | all comparisons; the `std::hash` checks are left out |
| `qa/tests/common/test_title_block.cpp` | `tests/test_title_block.rs` | all cases |

Other `qa/tests/common` files that mention these functions test GUI / plotter / eval code
(`test_eda_text`, `test_svg_plotter`, `text_eval/*`, ...) and belong to other packages.

## Differential oracle (C2)

`tests/oracle/units_strings_oracle.cpp` (built by `tests/oracle/build_c2_oracle.sh`, linked
against `libkicommon.so` of the reference build, wxWidgets 3.2.4 / GTK 3) runs 152 252 cases:
`FormatInternalUnits` (5 scales x 7 data types + points), `FormatAngle`, `FormatDouble2Str`,
`UIDouble2Str`, `FormatTrimmedDecimal`, `StringFromValue` / `MessageTextFromValue` (5 scales x
13 units x 7 types x both label flags), the int / long long / angle / MINOPTMAX overloads,
`ToUserUnit` / `FromUserUnit`, `Mm2mils`, `GetText` / `GetLabel`, every
`DoubleValueFromString` / `ValueFromString` / `FetchUnitsFromString` variant on generated
user input, `EscapeString` (all contexts) + round trip, `UnescapeString`, `StrNumCmp`,
`SortVariantNames`, `GetTrailingInt`, `ValueStringCompare`, `SplitString`, `QuacksLikeADuck`,
`WildCompareString`, `ConvertToNewOverbarNotation`, `TitleCaps`, `InitialCaps`, `EscapedUTF8`,
`EscapeHTML`, `UnescapeHTML`, `RemoveHTMLTags`, `PrintableCharCount`, `NoPrintableChars`,
`IsFullFileNameValid`, `ConvertSmartQuotesAndDashes`, both `ReplaceIllegalFileNameChars`,
`GetVariantFileToken`, both `ReadDelimitedText`, `StripTrailingZeros`, the stacked pin
functions, `COLOR4D` (string parsing, CSS / hex output, HSV / HSL, saturate, mix, compare,
luminance, nearest legacy colour, `EDA_COLOR_T` constructor), `PAGE_INFO` (all types,
`SetType` by name, size setters, `Format`) and `TITLE_BLOCK::Format`.
`tests/golden/c2_units_strings.txt` (13 KB) keeps the first 12 lines of each section and the
count and FNV-1a hash of all lines; `tests/test_c2_oracle.rs` replays it. **All 13 sections
match bit for bit.** The single input class excluded from the generator is a numeric HTML
reference to a surrogate code point (deviation C2-1).

## Deviations (C2)

- **C2-1** `UnescapeHTML`: a numeric reference to a value that is not a Unicode scalar value
  (a surrogate such as `&#xD800;`, or above 0x10FFFF) produces that raw value in a 32 bit
  `wxString`; Rust strings cannot hold it, so the port inserts U+FFFD.
- **C2-2** `PAGE_INFO::s_user_width` / `s_user_height` (static mutable state) are replaced by
  an explicit `CustomPageSize` argument (`PageInfo::new`, `set_type`, `set_type_by_name`);
  `CustomPageSize::default()` holds the C++ initial 17000 x 11000 mils.
- **C2-3** `TITLE_BLOCK::GetCurrentDate` & co. read the clock in C++; the port takes the texts
  from a caller-supplied `TitleBlockClock`, and `TextVarResolver` does not run the project's
  `ExpandTextVars` (callers with a project do).
- **C2-4** Locale: everything assumes KiCad's runtime setup, a UTF-8 `LC_CTYPE` and the "C"
  `LC_NUMERIC` (decimal point `.`). `towupper` / `towlower` are Rust's Unicode simple case
  mappings (`src/wxstr.rs`); glibc's tables could differ for exotic characters (the oracle
  covers ASCII, Latin-1 and Greek letters, which agree).
- **C2-5** Values the C++ converts with undefined behaviour (double to `int` / `unsigned char`
  out of range, signed overflow in `StrNumCmp`, `GetTrailingInt`, `TEXT_ATTRIBUTES::Compare`)
  saturate (`as`) or wrap (`wrapping_*`). The oracle includes `INT_MIN` / `INT_MAX` and
  overflowing digit runs and matches.
- **C2-6** `wxString` objects can contain embedded NULs; `WildCompareString` stops at the
  first NUL like the C++ (`GetData()`), the other functions treat NUL as an ordinary
  character (`CmpNoCase`'s chunk-wise NUL handling is not reproduced).
- **C2-7** `std::hash<COLOR4D>` (libstdc++ `_Hash_bytes` + `hash_combine`) is not reproduced;
  `Color4D` implements Rust `Hash` over the same components. No output depends on it.
- **C2-8** `COLOR4D::RelativeLuminance` calls `pow` as `f64::powf` directly (this crate must
  not depend on `kicad-geom`, which owns `cmath`); on wasm32 it may differ by one ulp.
- **C2-9** `ReplaceIllegalFileNameChars( std::string&, int )` with a replacement byte >= 0x80
  pushes that byte as a Latin-1 character (the C++ produces an invalid UTF-8 byte).

## C++ quirks reproduced (C2)

- `removeTrailingZeros` / `StripTrailingZeros` strip zeros even without a decimal separator;
  `StripTrailingZeros` without a separator wraps `(unsigned) -1` around (`"100"` -> `"1"`).
- `EscapedUTF8` prepares a copy with new lines replaced and then encodes the original.
- `ConvertToNewOverbarNotation` returns its input for any `~~{`.
- `DoubleValueFromString`: the `fs` check of the time units is a separate `if`;
  radians use `180.0f / M_PI`.
- `ApplyModifier`'s `if` chain (`a`/`f` and `p` are independent tests), `QuacksLikeADuck` /
  `ValueStringCompare` store partially parsed numbers (`wxString::ToCDouble` semantics), and
  `ValueStringCompare` returns the raw `CmpNoCase` difference in its last branch.
- `COLOR4D::Compare` never returns 0 when either colour has a text.
- `FindNearestLegacyColor` indexes `colorRefs()` with the enum value although the table is
  in a different order.
- `wxColour::Set` on wxGTK: GTK's parser first (channels go through 16 bit values: `#0FF000000`
  gives red 15, `rgb(127.5,0,0)` gives 128), then `wxColourBase::FromString` (`sscanf`, so
  `rgb(1,2,3` without `)` is accepted; `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`).

## Known gaps (C2)

- Colour names in `wxColour::Set` / `COLOR4D( wxString )` (`"red"`, X11 names via pango,
  `wxTheColourDatabase`) are not supported: they return false / keep the text. KiCad files and
  settings store colours as `rgb(...)` / `rgba(...)`, which are fully supported.
- `SplitGraphemeClusters`, `LinkifyHTML`, `IsURL` and the file system / clock helpers listed in
  the map are not ported.
- `wxstr::strtod` hexadecimal floats accumulate the mantissa in a double (exact up to 13 hex
  digits); decimal input is correctly rounded.

# Work package C3: JSON, pattern matching, settings parameters, small helpers

## C++ to Rust map (C3)

| C++ | Rust | State |
|---|---|---|
| nlohmann 3.11.3 `parse( ..., ALLOW_EXCEPTIONS, IGNORE_COMMENTS )` (lexer + parser: BOM, `//` `/* */`, strict UTF-8, escapes and surrogates, number classification, duplicate keys) | `json::parse_bytes`, `json::parse` | ported |
| `basic_json::dump( indent )`, `operator<<` with `std::setw`, `serializer::dump_escaped` (`ensure_ascii = false`), `dump_float` | `json::dump`, `json::dump_stream`, `json::dump_float` | ported |
| `detail::to_chars` (Grisu2) + `format_buffer` | `json/to_chars.rs` (`json::to_chars`) | ported (from the research prototype) |
| `nlohmann::json` vs `ordered_json` | `json::ObjectOrder` (`Sorted` = byte order of keys, `Insertion`), `json::sort_objects` | ported |
| `json_pointer` (`split`, `contains`, `get_checked`, `get_unchecked`, `array_index`) | `json::JsonPointer` | ported |
| `from_json` for bool / int / unsigned / int64 / uint64 / double / float / string / vector / map (`get<T>()`) and `to_json` | `json::FromJson`, `json::ToJson` | ported |
| `operator==`, `update( j, merge_objects )`, `type_name`, `clear` | `json::json_eq`, `json::update`, `json::type_name`, `json_settings::clear_value` | ported |
| `JSON_SETTINGS_INTERNALS` (`PointerFromString`, `Get`, `At`, `operator[]`, `m_original`) | `json_settings::JsonSettingsInternals` | ported |
| `JSON_SETTINGS::Load`, `LoadFromBuffer`, `parseAndMigrate`, `Migrate`, `registerMigration`, `Store`, `ResetToDefaults`, `formatFileContents`, `FormatAsString`, `GetJson`, `Get<T>`, `Get<wxString>`, `Set<T>`, `Contains`, `SetIfPresent` (4), `fetchOrDefault` | `json_settings::JsonSettings`, `json_settings::set_if_present` | ported |
| `NESTED_SETTINGS::LoadFromFile`, `SaveToFile` | `json_settings::NestedSettings::{load_from_parent, save_to_parent}` | ported (in memory) |
| `PARAM_BASE`, `PARAM<T>`, `PARAM_PATH`, `PARAM_ENUM`, `PARAM_LAMBDA`, `PARAM_SCALED`, `PARAM_LIST`, `PARAM_PATH_LIST`, `PARAM_SET`, `PARAM_MAP`, `PARAM_WXSTRING_MAP` | `settings_parameters::{Param, ParamT, ParamPath, ParamEnum, ParamLambda, ParamScaled, ParamList (+ path_list), ParamSet, ParamMap (+ wx_string_map)}`, `settings_field!` | ported |
| `JSON_SETTINGS::LoadFromFile` / `SaveToFile` (disk), `MigrateFromLegacy`, `fromLegacy*`, settings manager, `GetFileHistories`, `PARAM_LAYER_PRESET` & other board/UI specific params | - | skipped: file I/O and wxConfig are out of scope; board-specific parameters belong to the settings port |
| `eda_pattern_match.h/.cpp`: `SEARCH_TERM`, `FIND_RESULT`, `EDA_PATTERN_MATCH_SUBSTR`, `_REGEX`, `_REGEX_ANCHORED`, `_WILDCARD`, `_WILDCARD_ANCHORED`, `_RELATIONAL`, `EDA_COMBINED_MATCHER` (all contexts, `Find` x2, `StartsWith`, `ScoreTerms`) | `eda_pattern_match` | ported; the matchers are `Clone + Send + Sync` (`EdaPatternMatch::clone_box`, no C++ counterpart) so that a compiled `EdaCombinedMatcher` can be a member of the board's net settings |
| `wxRegEx::Compile( p, wxRE_ADVANCED )` / `Matches` / `GetMatch` (wxWidgets 3.2.4 `regex.cpp`: `ConvertMetasyntax`, `ConvertWordBoundaries`, PCRE2 10.42 with `UTF | ALT_BSUX | DOTALL`) | `eda_pattern_match::pcre` (`translate`, `WxRegex`) on `regress` 0.12 | ported (see deviations) |
| `common/common.cpp`: `ExpandTextVars( src, resolver, ctx, depth )`, `ResolveTextVars`, `FinalizeTextVarExpansion`, `NormalizeFilePathForTextVars`, `enum RESOLUTION_CONTEXT` | `text_vars` | ported (`EXPRESSION_EVALUATOR` is a callback) |
| `ExpandTextVars( src, PROJECT*, ctx )`, `KIwxExpandEnvVars`, `ExpandEnvVarSubstitutions` | - | skipped: need PROJECT / environment (later packages) |
| `include/units_provider.h` | `units_provider::UnitsProvider` | ported (no `ORIGIN_TRANSFORMS`; angles as degrees) |
| `include/case_insensitive_map.h` (`CASE_INSENSITIVE_COMPARER`, `CASE_INSENSITIVE_MAP`) | `case_insensitive_map` | ported |

## Design notes (C3)

- `serde_json` 1.0.154 with `preserve_order` + `float_roundtrip` is the value model only; the
  parser and writer are hand ports of nlohmann's (never `serde_json::from_str` /
  `to_string_pretty`). A `Value` keeps insertion order; `ObjectOrder::Sorted` sorts keys by
  bytes when dumping (and after parsing), which is `std::map<std::string, json>` order.
- C++ parameters hold member pointers; the port's parameters hold accessor fns
  (`settings_field!( Struct, member )`) and every operation takes the settings struct. A
  parameter `load` returns `Err` exactly where the C++ lets a nlohmann exception escape, and
  `JsonSettings::load` skips that parameter like the `catch( ... )` in `JSON_SETTINGS::Load`
  (for example a non-string value of a `PARAM<wxString>` keeps the member unchanged, while a
  non-number value of a `PARAM<int>` resets it to the default when `m_resetParamsIfMissing`).
- The implicit `PARAM<int>( "meta.version", &m_schemaVersion, ..., readOnly )` of the C++
  constructor is handled inside `JsonSettings::store`.
- Net class wildcards (`EDA_PATTERN_MATCH_WILDCARD[_ANCHORED]`) are matched by a hand port of the
  regex the C++ builds (greedy `.*`, leftmost start, DOTALL, `$` before a final newline), with a
  failure memo; the regex path is used only for patterns containing NUL.
- Regex patterns: the wx rewrites are ported literally, then a PCRE2 10.42 parser emits an
  ECMAScript pattern for `regress` (flags `u`, plus `i` when the whole pattern is caseless).
  Reproduced explicitly: ASCII `\d \w \s` and POSIX classes, `\h \v \R \N \C`, `$` / `\Z`
  before a final newline, multiline `^ $`, `.` with and without DOTALL, `ALT_BSUX` (`\x41`,
  `A`; `\x`, `\u`, `\U` alone are letters), octal vs backreference numbering, `\o{}`,
  `\N{U+..}`, `\cX`, `\Q..\E`, `(?#..)`, inline options `i m n s x U ^` (scoped and unscoped),
  wx's leading `(?b e q m n p w c t s i x)` letters and `***:` / `***=`, `\m \M \y \Y`,
  `[[:<:]] [[:>:]]`, named groups (three syntaxes) and `\k \g (?P=)` references, references to
  groups that are not set yet (always fail), atomic groups and possessive quantifiers
  (`(?=(X))\N` emulation), fixed-length lookbehind check, callouts (ignored), `(*UTF)` and
  harmless start-of-pattern settings, `(*FAIL)`, quantifier limits (65535, order), `{,n}`
  literal (10.42), and every compile error class the oracle exercises.
- `RELATIONAL` mirrors two C++ quirks: the reported position adds the matched token's offset to
  `wxStringTokenizer::GetPosition()` *before* that token (so leading delimiters are not
  counted), and a failed `SetPattern` keeps the key / relation / value parsed so far.

## Ported C++ tests (C3)

- `qa/tests/common/test_expand_text_vars.cpp`: suites `ExpandTextVarsTests` (14 cases) and
  `NormalizeFilePathForTextVarsTests` (4) in `tests/test_c3_qa.rs`. Not ported:
  `JobResolveOutputPath` (jobs), the environment-variable suites, and
  `ParallelResolveTextVarsWithMathExpressions` (needs `EXPRESSION_EVALUATOR`).
- `qa/tests/common/test_nested_settings.cpp`: `ReleaseDoesNotMarkParentModified`,
  `ExplicitSaveDetectsModifications`, `ReleaseFlushesDataToParent`,
  `AbsentDefaultIsModifiedWhenNotResetIfMissing`, `MigrationFlushesToParent`. Not ported: the
  three on-disk cases (`SaveToFileSkipsWriteWhenContentsMatch`,
  `ReleasedNestedChangePersistsToDisk`, `DefaultFillDoesNotChurnExistingFile`).
- `qa/tests/common/test_lib_tree_model_adapter.cpp`:
  `SingleLetterSearchMatchesDeviceNameOverKeywordCollision`, reduced to the `ScoreTerms`
  exact-match tier (the LIB_TREE node sorting is not part of this crate).
- Additional unit tests: JSON dump/parse rules, net class facts of `docs/analysis/settings.md`
  4.6, relational matcher, settings round trip, `UnitsProvider`, `CaseInsensitiveMap`.

## Differential oracle (C3)

Built by `tests/oracle/build_c3_oracle.sh` (`regen` rewrites the goldens).

- `tests/oracle/json_oracle.cpp` (bundled nlohmann 3.11.3) -> `tests/golden/c3_json.txt`
  (17 KB), replayed by `tests/test_c3_json_oracle.rs`: all 413 `.kicad_pro` / `.kicad_prl` /
  `.json` / `.gbrjob` files of `qa/data` and `demos` (sorted `dump(2)`, `dump()`, `setw(4)`,
  `setw(0)`, `dump(0)`; ordered `dump(2)`, `dump()`), 200 000 pseudo-random doubles, 50 000
  number texts (including malformed ones), 20 000 random documents (BOMs, comments, escapes,
  surrogates, invalid UTF-8, truncation, garbage; 5 843 of them invalid), 50 x 12 `get<T>()`
  conversions (including out-of-range casts), 37 dotted paths (`contains`, `at`, `operator[]`),
  `==` and `update()`. **All 7 sections identical.**
- `tests/oracle/pattern_oracle.cpp` (links `libkicommon.so`) -> `tests/golden/c3_pattern.txt`
  (39 KB), replayed by `tests/test_c3_pattern_oracle.rs`: 2 194 patterns (295 hand-written
  probes incl. every PCRE2 construct above, plus all 1 899 distinct net class patterns of the
  `.kicad_pro` files of `qa/data` and `demos`) x 181 subjects through all seven matcher kinds
  (15 358 lines) and `EDA_COMBINED_MATCHER` in all five contexts with `Find`, `StartsWith` and
  `ScoreTerms` (10 970 lines); 22 settings documents x 2 reset modes through a 24-parameter
  `JSON_SETTINGS` (every PARAM class, migrations, future / missing / wrong-typed versions,
  wrong types, nulls, out of range values, comments, BOM, non-object roots;
  values after load, `Store()`, `formatFileContents()`, `FormatAsString()`); 9 parent
  documents x 2 modes through `NESTED_SETTINGS` (load, migration incl. a throwing migrator,
  two `SaveToFile` rounds); 3 770 random texts through `ExpandTextVars` (8 contexts),
  `FinalizeTextVarExpansion`, `ResolveTextVars` and `NormalizeFilePathForTextVars`.
  **All sections identical**, except the four patterns listed in deviation C3-1, which the
  test checks separately (`deviations` section: the port must reject exactly those).

## Deviations (C3)

- C3-1: regex constructs PCRE2 accepts but the port rejects (the matcher is then not created,
  as for an invalid pattern): `\K`, `\X`, `(?R)` / `(?1)` / `(?&name)` / `(?P>name)` / `\g<..>`
  subroutine calls, conditionals `(?(..)..)`, `(?|..)` with capture groups, `(?J)`, `(?xx)`,
  `(?b)` (wx basic syntax), backtracking verbs other than `(*FAIL)`, start-of-pattern newline
  conventions other than LF, `(*UCP)`, `(*NOTEMPTY)`, quantified lookaround assertions,
  backreferences inside lookbehind, a backreference to a group that is still open inside a
  repetition, PCRE2 special properties `\p{Xan}` etc., caseless properties / large caseless
  ranges / caseless backreferences in patterns that also have case-sensitive parts. The oracle
  probes `\K`, `\X`, `(*ANY)a`, `(?R)`; none of them occurs in any project file.
- C3-2: a backreference to a group that is closed but did not participate (`(a)?\1`,
  `(a)|\1`) matches empty in ECMAScript and fails in PCRE2.
- C3-3: case-insensitive matching uses ECMAScript (`u` + `i`) simple case folding, or for
  mixed patterns an explicit variant list (simple upper/lower mapping plus the common
  multi-member orbits such as `k K U+212A`, `s S U+017F`); with `i`, `\w`, `\b` and `[a-z]`
  also match U+017F and U+212A (PCRE2: only through literal letters).
- C3-4: PCRE2's match limits (backtracking / heap) are not reproduced; a pathological pattern
  that makes PCRE2 give up (no match) may match or take long with `regress`.
- C3-5: nlohmann distinguishes a non-negative `number_integer` (created from a C++ `int`) from
  `number_unsigned`; `serde_json` cannot (`-0` parses to an integer 0 that the port classifies
  as unsigned). Output and comparisons are identical; only `is_number_unsigned()` /
  `SetIfPresent( unsigned& )` differ for values KiCad itself stored as `int`. The oracle prints
  integer kinds without that distinction.
- C3-6: parse error messages: the id (101) and position are reported, the message text is not
  nlohmann's verbatim (KiCad only logs id and byte). nlohmann's `out_of_range.406` (number
  overflow, which escapes KiCad's `parse_error` handler) is treated as a parse error by
  `JsonSettings::load_from_buffer`. Nesting deeper than 4 096 levels is rejected (nlohmann has
  no limit) so that dropping a document cannot overflow the stack.
- C3-7: a `std::exception` thrown by a `JSON_SETTINGS` migrator escapes `parseAndMigrate` in C++
  (only `parse_error` is caught); the port treats it as a failed migration. (In
  `NESTED_SETTINGS` both behave the same: the chain stops, modifications are kept.)
- C3-8: `NaN` / infinity stored with `Set<double>` become `null` immediately (nlohmann keeps the
  double and dumps `null`); the dump is identical.
- C3-9: `ExpandTextVars` / `ResolveTextVars` take `m_ResolveTextRecursionDepth` (default 6) and
  the math evaluator as `TextVarOptions`; without an evaluator `@{...}` is left unevaluated.

## Known gaps (C3)

- No file I/O for settings (`LoadFromFile`, `SaveToFile`, file-exists / modified-time logic,
  settings manager, legacy wxConfig migration).
- `EXPRESSION_EVALUATOR` (`common/text_eval/`) is not ported; `PROJECT`-based text variable
  resolvers come with the project / board ports.
- `UNITS_PROVIDER::GetOriginTransforms` is not ported.
- `PARAM_LIST<T>` / `PARAM_MAP<T>` work for every `FromJson` + `ToJson` type; the KiCad structs
  with their own `to_json` / `from_json` (`COLOR4D`, `GRID`, `BOM_*`, `JOBSET_*`, ...) need those
  conversions in their own ports.
