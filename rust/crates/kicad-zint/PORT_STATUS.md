# kicad-zint port status

## Package ZINT-1: barcode symbol encoder (zint 2.15.0 subset used by PCB_BARCODE)

Port of the zint code (`thirdparty/zint/backend`, BSD-3-Clause, licence text carried in every
file) reached by `PCB_BARCODE::ComputeBarcode` (pcbnew/pcb_barcode.cpp): `ZBarcode_Create()`,
`input_mode = UNICODE_MODE`, `show_hrt = 0`, `option_1` = ECC for QR / Micro QR, `eci = 26` for
non-ASCII QR / Data Matrix text, `ZBarcode_Encode()`, `ZBarcode_Buffer_Vector( symbol, 0 )`.
Entry point: `kicad_zint::encode(&Options, &[u8]) -> Result<Symbol, EncodeError>`.

| C | Rust | state |
|---|---|---|
| library.c `set_symbol_defaults` / `ZBarcode_Create`, `ZBarcode_Encode` -> `ZBarcode_Encode_Segs` (1 segment: empty / too long / ECI checks, UTF-8 check, `strip_bom`), `error_tag`, `supports_eci`, `extended_or_reduced_charset`, `reduced_charset`, `ZBarcode_Buffer_Vector` | `library.rs` | ported |
| common.c `flgs` / `is_chr` / `not_sane` / `not_sane_lookup`, `ctoi`, `to_upper`, `posn`, `bin_append_posn`, `module_is_set` / `set_module`, `expand`, `errtxt` / `errtxt_adj`, `set_height`, `stripf`, `decode_utf8`, `is_valid_utf8`, `utf8_to_unicode`, `is_twodigits`, `hrt_cpy_iso8859_1` (warning only) | `common.rs`, `code128.rs` | ported |
| eci.c `is_eci_convertible`, `get_eci_length`, `utf8_to_eci` (ECI 0/1/3 and 20), `eci_u_lookup_uro_int`, `u_sjis_int`, `sjis_utf8`, `sjis_cpy`, `sjis_utf8_to_eci`; sjis.h | `eci.rs`, `sjis_tables.rs` (generated) | ported |
| reedsol.c `rs_init_gf` (0x11d, 0x12d), `rs_init_code`, `rs_encode`; reedsol_logs.h | `reedsol.rs`, `reedsol_logs.rs` (generated) | ported |
| code.c `code39` (BARCODE_CODE39) | `code.rs` | ported |
| code128.c `code128`, `c128_cost`, `c128_cost_ab`, `c128_set_values`, `c128_expand`, `c128_set_priority` | `code128.rs` | ported |
| qr.c `qrcode`, `microqr` and helpers (`qr_define_mode`, `qr_in_numeric/alpha`, `qr_head_costs`, mode / cci / terminator bits, `qr_binary(_segs)`, `qr_add_ecc`, grid setup / populate, `qr_evaluate`, `qr_apply_bitmask`, format / version info, `qr_calc_binlen`, `qr_prep_data`, `microqr_end`, `microqr_*`); qr.h | `qr.rs`, `qr_tables.rs` (generated) | ported |
| dmatrix.c `datamatrix` / `dm_ecc200`, `dm_encode_segs`, `dm_encode`, `dm_minimalenc`, `dm_define_mode` and edges, `dm_get_symbolsize`, `dm_codewords_remaining`, buffer transfers, `dm_ecc`, `dm_placement*`, `dm_add_tail`; dmatrix.h | `dmatrix.rs`, `dmatrix_tables.rs` (generated) | ported |
| vector.c `plot_vector` (rectangle path), `vector_reduce_rectangles`, `vector_scale`; output.c `out_large_bar_height` (si 0), `out_set_whitespace_offsets`, `out_quiet_zones` (5 symbologies) | `vector.rs` | ported |

Tables are generated verbatim from the C headers by `tools/gen_tables.py`.

Left out because KiCad's settings cannot reach them (each returns `Stage::NotPorted` or is a
compile-time constant path):
- other symbologies, rMQR / UPNQR, MaxiCode hexagons, Ultracode, dotty mode, text / HRT
  (show_hrt 0), UPC/EAN, bind / box / whitespace, rotation, file output;
- ESCAPE_MODE / EXTRA_ESCAPE_MODE, GS1_MODE, FAST_MODE (`dm_isoenc`, `dm_look_ahead_test`),
  multiple segments, structured append, `primary`, raw text, `WARN_FAIL_ALL`, debug output;
- QR `gs1` percent / FNC1 handling, user masks / versions (`option_2` / `option_3` stay 0;
  the range checks are ported), DMRE / square options;
- `get_best_eci_segs` / `get_best_eci` and the ECI converters other than ISO/IEC 8859-1 and
  Shift JIS: only reached when a QR / Data Matrix text that is not Latin-1 is encoded with ECI 0,
  which KiCad never does (it sets ECI 26 for every non-ASCII QR / Data Matrix text).

Deviations:
- Data Matrix macro input of exactly `[)>\x1e05\x1d\x1e\x04` (or `06`): zint encodes an empty
  segment past the end of its edge array (undefined behaviour); the port returns
  `Stage::NotPorted`.
- `errtxtf` formatting is done with `format!` for the `%d` / `%c` / `%s` conversions zint
  uses here (identical output); the `%.4g` message of `set_height` is unreachable (max height
  is always 0) and not produced.

Tests:
- `tests/zint_oracle.rs` `zint_matches_oracle`: differential replay of
  `tests/oracle/zint_oracle.c`, a C driver linked against the reference build's `libzint.a`
  (the zint objects `kicad-cli` uses, -O3 -DNDEBUG), built by `tests/oracle/build_oracle.sh`
  under the machine-wide lock. 319,061 generated cases, encoded exactly as `ComputeBarcode`
  does, each compared as a line (return code, `errtxt`, rows, width, height bits,
  option_1/2/3 feedback, row heights, module bitmap, `ZBarcode_Buffer_Vector` return / errtxt,
  vector size and every rectangle as float bits, hexagons), FNV-1a hashed per block of 1000
  (golden `tests/golden/zint_hashes.txt`, 321 lines):
  * sweep: every symbology x ECC (QR / Micro QR 1..4) x class (digits, QR alphanumerics,
    printable ASCII, any ASCII incl. control / NUL, Latin-1, kanji) x length 1 .. beyond the
    capacity (step 1 below 400 characters, then 41 (QR) / 29 (DM)), plus every QR version
    capacity (digits / alphanumerics / bytes, -1 / 0 / +1, 4 ECC levels) and every Data Matrix
    size capacity (digits, C40 text, -1 / 0 / +1);
  * 300,000 pseudo-random inputs (fixed seed, splitmix64 mirrored in C and Rust): symbology,
    ECC (incl. Micro QR H), KiCad's ECI rule or none (10 %), 1-3 character classes in runs
    (adding 2-byte, 3-byte, 4-byte UTF-8, Shift JIS specials, digit-heavy mixes), lengths up
    to 7200 (QR) / 3200 (DM) characters.
  Result: all 321 blocks identical (full run, `ZINT_ORACLE_FULL=1`, 580 s; the default run
  replays every 8th block, about 60 s). The cases that need the not-ported ECI retry are
  compared as "not ported" on both sides (the C driver detects the retry by the changed
  `symbol->eci`).
- `zint_cases_match_reference`: 35 hand-picked inputs (`tests/golden/zint_cases.txt`, from
  `zint_oracle input`): every error / warning text KiCad can show, invalid UTF-8, BOM,
  Shift JIS conversion warnings, Data Matrix macro 05, NUL, ECI range check.
- `common::tests::utf8_dfa_matches_std`: the UTF-8 DFA table accepts exactly valid UTF-8.
- End to end (kicad-pcb `pcb_barcode::tests::barcodes_match_reference_dump`, feature `dump`):
  `pcb-dump board --hash --sections misc,bbox` of `barcode_frames` / `barcode_cases`
  (`tests/data`, 32 barcodes incl. every error path), `issue24525`, `api_kitchen_sink`,
  `project_barcode` (golden `tests/golden/pcb_barcode_dump.txt`): barcode `poly`,
  `symbol_poly`, `text_poly`, `last_error` and every `bbox` line of those boards (barcodes and
  footprints around them): 600 / 600 equal. The kicad-fab plot corpus (`project_barcode`
  F.Silkscreen, `issue24525` Edge.Cuts) and the kicad-session gerbers job corpus now report
  0 known differences (the barcode layers are byte-identical).
