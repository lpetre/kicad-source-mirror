# kicad-core port status

Port of the KiCad `common/` basics. This file covers work package **C1** (identifiers and
layers). Units, number formatting, string utilities, text attributes and colours are a later
work package and are not in the crate yet.

The crate has no dependencies (dev-dependency: `kicad-qa`) and builds for
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
