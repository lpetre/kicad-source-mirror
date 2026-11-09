# kicad-sexpr: port status

Rust port of KiCad's s-expression machinery: the `DSNLEXER` tokenizer with its line readers,
errors and keyword tables, the `OUTPUTFORMATTER` family with `KICAD_FORMAT::Prettify`, and the
generic `SEXPR` tree of `libs/sexpr`. No dependencies; builds for `wasm32-unknown-unknown`.

## Module map

| C++ | Rust | State |
|---|---|---|
| `include/ki_exception.h`, `common/exceptions.cpp` | `src/ki_exception.rs` | |
| `IO_ERROR` (+ `THROW_IO_ERROR`, `THROW_IO_ERRORF`) | `IoError::Io( IoErrorBase )`, `IoError::new` | ported (see D1, D2) |
| `IO_CANCELLED` (+ `THROW_IO_CANCELLED[_MSG]`) | `IoError::Cancelled`, `IoError::cancelled[_msg]` | ported |
| `PARSE_ERROR` (+ `THROW_PARSE_ERROR`) | `ParseError`, `IoError::Parse`, `IoError::parse` | ported |
| `FUTURE_FORMAT_ERROR` (3 constructors) | `FutureFormatError::{new, from_parse_error, from_future_format_error}`, `IoError::to_future_format_error` | ported |
| `KI_PARAM_ERROR` | `KiParamError` | ported |
| `include/richio.h`, `common/richio.cpp` | `src/lexer/line_reader.rs`, `src/formatter/richio.rs` | |
| `LINE_READER` | trait `LineReader` | ported |
| `STRING_LINE_READER` (both constructors, `ReadLine`) | `StringLineReader::{new, from_starting_point, with_max_line_length}` | ported (D7) |
| `LINE_READER_LINE_DEFAULT_MAX`, `..._INITIAL_SIZE` | same names (consts) | ported |
| `FILE_LINE_READER`, `INPUTSTREAM_LINE_READER`, `SafeReadFile` | - | skipped: file system / wx streams (PORTING.md 5) |
| `OUTPUTFORMATTER` (`Print` x2, `Indent`, `GetQuoteChar` x2, `Quotes`, `Quotew`, `Finish`, `write`) | trait `OutputFormatter` (`print`, `print_nest`, `indent`, `get_quote_char`, `quotes`, `quotew`, `finish`, `write`), fn `get_quote_char` | ported (D5) |
| `STRING_FORMATTER` | `StringFormatter` | ported |
| `PRETTIFIED_FILE_OUTPUTFORMATTER` | `PrettifiedOutputFormatter` | partial: in memory, see D6 |
| `FILE_OUTPUTFORMATTER`, `SIBLING_TEMP_FILE` | - | skipped: file system (atomic temp file + rename) |
| `include/dsnlexer.h`, `common/dsnlexer.cpp` | `src/lexer/dsnlexer.rs` | |
| `DSN_SYNTAX_T` | consts `DSN_NONE`..`DSN_EOF`, enum `DsnSyntaxT` | ported |
| `DSNLEXER` constructors (string; `LINE_READER*`; no keyword table) | `DsnLexer::<T>::{new, from_line_reader}`, `DsnLexer::<i32>::{with_keywords, with_keywords_from_line_reader}`, `DsnLexer::<DsnSyntaxT>` | ported |
| `DSNLEXER( ..., FILE*, wxString )` | - | skipped: file system |
| `NextTok` (complete state machine) | `next_tok` | ported |
| `NeedSYMBOL`, `NeedSYMBOLorNUMBER`, `NeedNUMBER`, `NeedLEFT`, `NeedRIGHT`, `NeedBAR` | `need_symbol`, `need_symbol_or_number`, `need_number`, `need_left`, `need_right`, `need_bar` | ported |
| `Expecting` x2, `Unexpected` x2, `Duplicate` | `expecting`, `expecting_str`, `unexpected`, `unexpected_str`, `duplicate` (return the error) | ported |
| `CurTok`, `PrevTok`, `GetCurStrAsToken`, `CurText`, `CurStr`, `CurSeparator`, `FromUTF8`, `CurLineNumber`, `CurLine`, `CurSource`, `CurOffset` | `cur_tok`, `prev_tok`, `cur_str_as_token`, `cur_text`, `cur_str`, `cur_separator`, `from_utf8`, `cur_line_number`, `cur_line`, `cur_source`, `cur_offset` (+ `cur_bytes`) | ported (D3) |
| `SetSpecctraMode`, `SetStringDelimiter`, `SetSpaceInQuotedTokens`, `SetKnowsBar`, `SetCommentsAreTokens`, `InitParserState` | snake_case of the same | ported |
| `ReadCommentLines` | `read_comment_lines` | ported |
| `PushReader`, `PopReader`, `SyncLineReaderWith` | `push_reader`, `pop_reader`, `sync_line_reader_with` (+ `with_sub_lexer`, `reclaim_line_reader_from`) | ported (D4) |
| `IsSymbol`, `IsNumber`, `Syntax`, `GetTokenText`, `GetTokenString` | `is_symbol`, `is_number`, fn `syntax`, `token_text`, `token_string` | ported |
| `isNumber( cp, limit )` (protected static) | fn `is_number_text` | ported |
| `parseDouble()`, `parseDouble( const char* )`, `parseDouble( T )` | `parse_double`, `parse_double_expected`, `parse_double_tok` | ported |
| `parseInt()`, `parseInt( const char* )`, `parseHex()` of `PCB_IO_KICAD_SEXPR_PARSER` (not in `DSNLEXER`) | `parse_int`, `parse_int_expected`, `parse_hex` | ported as conveniences |
| `fast_float::from_chars` (syntax and errors as used by `parseDouble`), `strtol`, `strtoll`, `strtod` | `src/lexer/c_numeric.rs`: `from_chars_double`, `strtol`, `strtoll`, `strtod_prefix` | ported (accepted syntax; conversion by `str::parse::<f64>`) |
| `cmake/BuildSteps/TokenList2DsnLexer.cmake` | `src/lexer/keywords.rs`: trait `KeywordSet`, macro `keywords!`, `parse_token_list`, `generate_keywords_module` | ported (D8) |
| generated `pcb_lexer.h` (`PCB_KEYS_T`, `PCB_LEXER`) and 9 more | `src/lexer/keywords/{pcb_keys_t, pcbplotparams_t, strokeparams_t, embedded_files_t, drcrule_t, lib_table_t, drawingsheet_t, nl_t, tfield_t, dsn}.rs` | ported (generated) |
| `include/hashtables.h` `KEYWORD_MAP` | binary search in the sorted table | replaced, same results |
| `include/io/kicad/kicad_io_utils.h`, `common/io/kicad/kicad_io_utils.cpp` | `src/formatter/kicad_io_utils.rs` | |
| `KICAD_FORMAT::FormatBool`, `FormatOptBool` | `format_bool`, `format_opt_bool` | ported |
| `KICAD_FORMAT::FormatUuid` | `format_uuid` (takes `KIID::AsString()`) | ported |
| `KICAD_FORMAT::FormatCustomProperties` | `format_custom_properties` (takes the key/value pairs) | ported |
| `KICAD_FORMAT::FormatStreamData` (+ `wxBase64Encode`) | `format_stream_data`, `base64_encode` | ported |
| `KICAD_FORMAT::FORMAT_MODE`, `KICAD_FORMAT::Prettify` | `FormatMode`, `prettify`, `prettify_in_place`, `prettify_bytes` | ported |
| `LoadFileToMemory` | - | skipped: file system |
| `printf`/`{fmt}` `%g`, `%f`, `%e` | `src/formatter/cformat.rs`: `format_g`, `format_f`, `format_e` | ported (exact output) |
| `libs/sexpr/include/sexpr/*.h`, `libs/sexpr/*.cpp` | `src/tree/` | |
| `SEXPR`, `SEXPR_LIST`, `SEXPR_INTEGER`, `SEXPR_DOUBLE`, `SEXPR_STRING`, `SEXPR_SYMBOL`, `SEXPR_TYPE` | `Sexpr` (enum), `SexprList`, `SexprInteger`, `SexprDouble`, `SexprString`, `SexprSymbol`, `SexprType` | ported |
| `SEXPR::AsString` and all accessors | `as_string`, `children`, `child`, `number_of_children`, `long_integer`, `integer`, `float`, `double`, `string`, `symbol`, `list`, `line_number`, `add_child` | ported (D9) |
| `_OUT_STRING`, `_IN_STRING`, `AsSymbol`, `AsString` | `OutString`, `InString`, `as_symbol`, `as_string`, `as_symbol_in`, `as_string_in` | ported |
| `SEXPR_LIST::Scan`, `AddChildren`, variadic constructor, `SEXPR_SCAN_ARG`, `SEXPR_CHILDREN_ARG` | `SexprList::{scan, add_children, from_children}`, `SexprScanArg`, `SexprChildrenArg` | ported (D9) |
| `operator<<` (9 overloads) | `SexprList::push`, `push_sexprable` | ported |
| `operator>>` (7 overloads) | `SexprList::{read_i32, read_i64, read_f32, read_f64, read_string, read_in_string, read_sexprable}` | ported |
| `ISEXPRABLE` | trait `ISexprable` | ported |
| `PARSE_EXCEPTION`, `INVALID_TYPE_EXCEPTION` (+ `std::invalid_argument`) | `ParseException`, `InvalidTypeException`, `SexprException` | ported |
| `PARSER::Parse` | `Parser::parse` | ported |
| `PARSER::ParseFromFile`, `PARSER::GetFileContents` | - | skipped: file system |

The parameters of the keyword table generator (how a `*.keywords` file becomes token numbers,
and that a keyword can only be `[a-z][_0-9a-z]*`, so there is no mangling of `-`, `+`, `.` or
leading digits to port) are documented in `src/lexer/keywords.rs`.

`DSNLEXER::parseDouble` in this tree uses `fast_float` only; there are no locale tolerant
fallbacks left to port.

## Ported C++ tests

| C++ test | Rust test |
|---|---|
| `qa/tests/libs/sexpr/test_sexpr.cpp` (suite `Sexpr`, 6 cases) | `tests/test_sexpr.rs` |
| `qa/tests/libs/sexpr/test_sexpr_parser.cpp` (suite `SexprParser`, 7 cases) | `tests/test_sexpr_parser.rs` |
| `qa/tests/libs/sexpr/sexpr_test_utils.h` | `tests/sexpr_test_utils/mod.rs` |
| `qa/tests/common/test_richio.cpp` (suite `RichIO`, 4 cases) | `tests/test_richio.rs` |
| `qa/tests/pcbnew/test_prettifier.cpp` (`BoardAndFootprintPrettifier`) | `tests/test_prettifier.rs::board_and_footprint_prettifier` - golden comparison as in C++; "parses the same way" is checked on token streams because the board parser is not in this crate |
| `qa/tests/common/test_library_tables.cpp` (formatting check of `ParseFromFile`) | `tests/test_prettifier.rs::library_tables_are_fixed_points_of_library_table_mode` - partial: `LIBRARY_TABLE` is not in this crate, the files are checked to be fixed points of `FORMAT_MODE::LIBRARY_TABLE` |

There are no C++ unit tests for `DSNLEXER`, `KICAD_FORMAT::FormatBool` etc. The other tests in
`qa/` that mention `Prettify` or `STRING_FORMATTER` test higher level classes (board plugin,
`LINE_ENDING`, ...) and belong to the crates that port those.

## Differential tests against the C++ code

`tests/oracle/oracle.cpp` is a program that links the *original* `common/dsnlexer.cpp`,
`common/richio.cpp`, `common/exceptions.cpp`, `common/io/kicad/kicad_io_utils.cpp`,
`libs/sexpr/*.cpp`, the bundled `fast_float` and `{fmt}`, and the ten lexers the real
`TokenList2DsnLexer.cmake` generates (`tests/oracle/build.sh`; needs g++, CMake and wxWidgets
base). `tests/oracle/generate_goldens.sh` ran it over the inputs in `tests/golden/*.txt`; the
outputs are committed as `tests/golden/*.out`, so `cargo test` needs no C++ toolchain.

| Rust test (`tests/oracle_goldens.rs`, `tests/corpus.rs`) | What must equal the C++ output byte for byte |
|---|---|
| `lexer_script_matches_cpp` | a script of 431 lexer commands: tokens, texts, separators, offsets, line numbers and every error message for KiCad mode, specctra mode, comments, escapes, unterminated strings, the bar token, `Need*`/`Expecting`/`Unexpected`/`Duplicate`, `parseDouble`, `parseInt`/`parseHex`, `ReadCommentLines`, the reader stack, two lexers on one reader, end of input, the 16 000 000 byte line limit |
| `numbers_match_cpp` | `isNumber`, `parseDouble` (bit patterns), `strtol` base 10/16, `strtoll` base 0 on 127 texts |
| `printf_conversions_match_cpp` | `%g`/`%f`/`%e` and `{fmt}` on ~1500 value/precision pairs (the two C++ libraries agree on all of them) |
| `quotes_match_cpp` | `Quotes`, `GetQuoteChar` |
| `format_helpers_match_cpp` | `FormatBool`, `FormatOptBool`, `FormatUuid`, `FormatCustomProperties`, `FormatStreamData`, `Print`, `Indent`, `StripUseless`, and the texts of `IO_ERROR`, `IO_CANCELLED`, `PARSE_ERROR`, `FUTURE_FORMAT_ERROR` |
| `sexpr_parser_matches_cpp`, `sexpr_api_matches_cpp` | `SEXPR::PARSER` + `AsString` + node dump on 50 texts; stream operators, `Scan`, accessors and their exception texts |
| `prettify_matches_cpp` | `Prettify` in all three modes on 60 hand-made texts |
| `keyword_tables_match_cmake_generator` | the ten keyword tables against the CMake generator's output |
| `corpus_matches_cpp` (+ `big_corpus_matches_cpp`, `--ignored`) | full token dump and `Prettify` (normal and compact) of 479 real files: all 375 `.kicad_pcb` of `qa/data/pcbnew` and `demos`, 13 `.kicad_mod`, 9 `.kicad_sym`, 69 `.kicad_dru`, 6 `.kicad_wks`, 7 library tables (compared through FNV-1a hashes) |

The same comparisons can be run on any other input set: put `*.txt` files in a directory, make
the `*.out` files with the oracle and run `KICAD_SEXPR_GOLDEN_DIR=<dir> cargo test -p kicad-sexpr
--test oracle_goldens`. This was used for differential fuzzing while porting (not committed):
11 rounds of 2 500 random texts through `Prettify` (x3 modes), 1 500 random lexer runs (KiCad
and specctra mode, all setters), 2 500 texts through `SEXPR::PARSER`, 4 000 number strings and
1 500 strings through `Quotes`, plus 30 000 random `printf`/`{fmt}` float conversions with
precisions 0..25. No difference was found.

`tests/corpus.rs::token_streams_are_sane_and_prettify_is_idempotent` is the property test that
needs no golden: balanced parentheses / EOF / no error and `prettify( prettify( x ) ) ==
prettify( x )` for all boards up to 12 MB (371 files) plus footprints, symbols, rules and
drawing sheets.

### Are KiCad's saved files fixed points of `prettify`?

Yes, for files as KiCad saves them. Evidence from the 375 boards of the corpus:

- 294 have a format version from 20240108 on (KiCad 8, the first that saves through
  `Prettify`). 263 of them (89 %) satisfy `prettify( x ) == x`, among them every board of the
  newest versions in the tree (20260831: 18/18, 20260901: 17/17, 20261017, 20261022).
- The other 31 are not fixed points of the **C++** `Prettify` either: the oracle changes each of
  them, to the same bytes as the port. They are test data that was not written by a normal
  save: 5 were saved in the compact mode (they are fixed points of
  `COMPACT_TEXT_PROPERTIES`), 7 are indented with spaces, 1 has CR LF line ends, 1 lacks the
  final newline, and 17 carry other manual edits (several lists on one line, blank lines, text
  behind the closing parenthesis, e.g. `qa/data/pcbnew/teardrop_offcenter_two_segment.kicad_pcb`
  ends in `</content></invoke>`).
- Of the 81 boards in older formats (written by the pre-`Prettify` writers) one is a fixed
  point.

The property test asserts the first point strictly for the newest version, and for every other
modern board that is not a fixed point it requires that the C++ oracle agrees.

## Deviations

- **D1 `Where()`**: `IO_ERROR::Where()` is the C++ throw site (`from file.cpp : func() line
  n`). The port records the Rust source location of the error constructor instead (`from
  dsnlexer.rs : line n`, no function name). It only appears in `IoError::what_debug()`.
- **D2 `What()`**: `IoError::what()` / `Display` are the release build behaviour (the problem
  text). The `DEBUG` build variant (`"IO_ERROR: " + problem + "\n\n" + where`) is
  `what_debug()`. Messages are the untranslated English texts (no `_()` catalogue).
- **D3 token text is a `String`**: input is `&str`/`String` (valid UTF-8), C++ lexes raw bytes.
  A token can still become non-UTF-8 through `\xHH`/octal escapes in a quoted string; then
  `cur_bytes()` has the exact bytes, `cur_text()` the lossy conversion and `from_utf8()` is
  empty (as `wxString::FromUTF8`). Files that are not valid UTF-8 have to be converted by the
  caller first (C++ would lex them and produce empty `wxString`s for the affected tokens).
  `set_string_delimiter` takes a byte; only ASCII delimiters make sense.
- **D4 reader ownership**: C++ lexers hold raw `LINE_READER*` and may share one. Here a reader
  has one owner: `sync_line_reader_with` hands the reader over together with the position
  (the C++ call sequence ports unchanged), `with_sub_lexer` wraps the whole idiom, and
  `reclaim_line_reader_from` returns the reader without position where C++ leaves by exception.
  A lexer without reader (C++: null pointer dereference in `CurLine()` etc.) returns EOF, line
  0 and empty strings.
- **D5 `Print`**: takes `std::fmt::Arguments` instead of a `printf` format; `%g`/`%f`/`%e`
  call sites use `format_g`/`format_f`/`format_e`. `FormatBool`'s `%ls` conversion of the key
  (which depends on the C locale for non-ASCII keys) is a plain copy.
- **D6 `PrettifiedOutputFormatter`**: no file. `finish()` prettifies the buffer, the caller
  stores `string()`. `ADVANCED_CFG::m_CompactSave` is the constructor argument `compact_save`.
- **D7 line length**: a single line of 4 GiB or more is reported as too long; the C++ length
  computation would wrap in `unsigned`.
- **D8 keyword enums keep the C++ spelling** (`T::T_at`, `T::T_LEFT`) instead of the
  UpperCamelCase of PORTING.md 2: syntax tokens and keywords of one enum differ only in case
  (`T_LEFT`/`T_left`, `T_NONE`/`T_none`, `T_NUMBER`/`T_number`, ...), and the C++ names can be
  grepped as they are. In `parse_token_list`, CMake's splitting of a line at `;` is not
  reproduced (such a line is an invalid token).
- **D9 undefined behaviour of `libs/sexpr` made defined**: `GetChild( i )` beyond the end and
  `operator>>` past the last child return `SexprException::OutOfRange`; `Scan` stops at the end
  of the list and stores an `int32_t*` argument as integer (C++ writes a `double` through the
  pointer); atoms like `.` or `-.` and out of range values, for which C++ reads an
  uninitialised `double`, parse as 0 resp. the saturated value; `operator<<` of an
  `ISEXPRABLE` that serialises to `nullptr` appends nothing. `DeserializeSEXPR` takes the list
  (`&mut SexprList`) rather than `SEXPR&`.
- **D10 no recursion**: `Parser::parse`, `as_string` and dropping a tree are iterative (C++
  recurses per nesting level), so deeply nested input cannot overflow the stack. `prettify`
  caches the end of a whitespace run instead of rescanning it for every character (C++ is
  quadratic in the length of a run). Results are identical.
- **D11 `isalpha` in `Prettify`**: taken as ASCII letters (true for the "C" and UTF-8 locales
  KiCad runs in).
- **D12 `strtol`**: follows LP64 (`long` = 64 bit, Linux/macOS). On 64 bit Windows C++ clamps
  `strtol` results to 32 bit.

## Known gaps

- Everything that touches the file system: `FILE_LINE_READER`, `INPUTSTREAM_LINE_READER`,
  `SafeReadFile` (including its UTF-16 / CP1252 detection), `FILE_OUTPUTFORMATTER`, the temp
  file handling of `PRETTIFIED_FILE_OUTPUTFORMATTER`, `LoadFileToMemory`,
  `PARSER::ParseFromFile`. The CLI / wasm bindings read and write the bytes.
- The derived `Clone`, `PartialEq` and `Debug` of `Sexpr` recurse; only parsing, `as_string`
  and drop are safe for arbitrarily deep trees.
- Keyword enums are provided for the `*.keywords` files under `common/` and for
  `specctra.keywords`; the eeschema ones (`schematic.keywords`, `dialog_bom_cfg.keywords`) are
  not generated (out of scope). `generate_keywords_module` makes them on demand.
