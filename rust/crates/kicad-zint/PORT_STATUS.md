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

Verification: see "Oracle" below.
