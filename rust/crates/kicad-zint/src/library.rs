//! Port of the parts of `thirdparty/zint/backend/library.c` used by KiCad's `PCB_BARCODE`:
//! `ZBarcode_Create` defaults (`set_symbol_defaults`), `ZBarcode_Encode` /
//! `ZBarcode_Encode_Segs` (one segment) with its input checks, `error_tag`,
//! `extended_or_reduced_charset` / `reduced_charset`, `supports_eci`, `strip_bom` and
//! `ZBarcode_Buffer_Vector` / `check_output_args`.
//!
//! Not ported (unreachable from KiCad's settings, which are fixed by [`crate::Options`]):
//! ESCAPE_MODE / EXTRA_ESCAPE_MODE processing, GS1_MODE, multiple segments, `primary`,
//! structured append, `map_invalid_symbology` (only the five symbologies are accepted), debug
//! printing, raw text, and `WARN_FAIL_ALL`.

use crate::code::code39;
use crate::code128::code128;
use crate::common::{
    ZINT_COLS_MAX, ZINT_ERROR, ZINT_ERROR_ENCODING_PROBLEM, ZINT_ERROR_INVALID_DATA,
    ZINT_ERROR_INVALID_OPTION, ZINT_ERROR_TOO_LONG, ZINT_ROWS_MAX, ZINT_WARN_USES_ECI,
    errtxt, errtxt_adj_prefix, is_valid_utf8, set_height,
};
use crate::dmatrix::datamatrix;
use crate::eci::{is_eci_convertible, utf8_to_eci};
use crate::qr::{microqr, qrcode};
use crate::vector::{Vector, plot_vector};

/// `BARCODE_CODE39`.
pub const BARCODE_CODE39: i32 = 8;
/// `BARCODE_CODE128`.
pub const BARCODE_CODE128: i32 = 20;
/// `BARCODE_QRCODE`.
pub const BARCODE_QRCODE: i32 = 58;
/// `BARCODE_DATAMATRIX`.
pub const BARCODE_DATAMATRIX: i32 = 71;
/// `BARCODE_MICROQR`.
pub const BARCODE_MICROQR: i32 = 97;

/// `DATA_MODE`.
pub const DATA_MODE: i32 = 0;
/// `UNICODE_MODE`.
pub const UNICODE_MODE: i32 = 1;
/// `HEIGHTPERROW_MODE`.
pub(crate) const HEIGHTPERROW_MODE: i32 = 0x0040;
/// `FAST_MODE`.
pub(crate) const FAST_MODE: i32 = 0x0080;

/// `ZINT_MAX_DATA_LEN`.
const ZINT_MAX_DATA_LEN: usize = 17400;

/// The members of `struct zint_symbol` the ported code reads or writes.
pub(crate) struct ZintSymbol {
    pub symbology: i32,
    /// Barcode height in X-dimensions (ignored for fixed-width barcodes).
    pub height: f32,
    pub scale: f32,
    pub whitespace_width: i32,
    pub whitespace_height: i32,
    pub border_width: i32,
    pub output_options: i32,
    pub option_1: i32,
    pub option_2: i32,
    pub option_3: i32,
    pub show_hrt: i32,
    pub input_mode: i32,
    pub eci: i32,
    pub dot_size: f32,
    pub text_gap: f32,
    pub guard_descent: f32,
    /// `text_length` of the (not kept) human readable text.
    pub text_length: i32,
    pub rows: i32,
    pub width: i32,
    pub encoded_data: Vec<[u8; ZINT_COLS_MAX]>,
    pub row_height: [f32; ZINT_ROWS_MAX],
    pub errtxt: String,
    pub vector: Option<Vector>,
}

impl ZintSymbol {
    /// Port of `ZBarcode_Create()` / `set_symbol_defaults()`.
    pub(crate) fn create() -> ZintSymbol {
        ZintSymbol {
            symbology: BARCODE_CODE128,
            height: 0.0,
            scale: 1.0,
            whitespace_width: 0,
            whitespace_height: 0,
            border_width: 0,
            output_options: 0,
            option_1: -1,
            option_2: 0,
            option_3: 0,
            show_hrt: 1, // Show human readable text
            input_mode: DATA_MODE,
            eci: 0, // Default 0 uses ECI 3
            dot_size: 0.8, // 0.4 / 0.5
            text_gap: 1.0,
            guard_descent: 5.0,
            text_length: 0,
            rows: 0,
            width: 0,
            encoded_data: vec![[0u8; ZINT_COLS_MAX]; ZINT_ROWS_MAX],
            row_height: [0.0; ZINT_ROWS_MAX],
            errtxt: String::new(),
            vector: None,
        }
    }
}

/// Port of `error_tag()`: prefixes `errtxt` with "Error " / "Warning " (`warn_level`
/// WARN_DEFAULT, so warnings are not converted to errors). `error_string` set = `errtxt(err_id)`.
pub(crate) fn error_tag(
    error_number: i32,
    symbol: &mut ZintSymbol,
    err_id: i32,
    error_string: Option<&str>,
) -> i32 {
    if error_number != 0 {
        if let Some(s) = error_string {
            errtxt(0, symbol, err_id, s);
        }
        if error_number >= ZINT_ERROR {
            errtxt_adj_prefix(0, symbol, "Error ");
        } else {
            errtxt_adj_prefix(0, symbol, "Warning ");
        }
    }

    error_number
}

/// Port of `supports_eci()` for the symbologies of this port.
pub(crate) fn supports_eci(symbology: i32) -> bool {
    matches!(symbology, BARCODE_DATAMATRIX | BARCODE_QRCODE)
}

/// Port of `ZBarcode_ValidID()` restricted to the ported symbologies.
fn valid_id(symbology: i32) -> bool {
    matches!(
        symbology,
        BARCODE_CODE39 | BARCODE_CODE128 | BARCODE_QRCODE | BARCODE_DATAMATRIX | BARCODE_MICROQR
    )
}

/// One input segment (`struct zint_seg`).
#[derive(Clone)]
pub(crate) struct ZintSeg {
    pub source: Vec<u8>,
    pub eci: i32,
}

/// Result of the barcode functions when the port reaches a path it does not include.
pub(crate) const NOT_PORTED: i32 = -1;

/// The `barcode_src_funcs` / `barcode_seg_funcs` dispatch for the ported symbologies.
fn call_symbology(symbol: &mut ZintSymbol, segs: &mut [ZintSeg]) -> i32 {
    match symbol.symbology {
        BARCODE_CODE39 => {
            let mut src = segs[0].source.clone();
            code39(symbol, &mut src)
        }
        BARCODE_CODE128 => {
            let src = segs[0].source.clone();
            code128(symbol, &src)
        }
        BARCODE_DATAMATRIX => datamatrix(symbol, segs),
        _ => unreachable!(),
    }
}

/// Port of `extended_or_reduced_charset()`: main dispatch, checking for barcodes which handle
/// ECIs/character sets themselves, otherwise calling `reduced_charset()`.
fn extended_or_reduced_charset(symbol: &mut ZintSymbol, segs: &mut [ZintSeg]) -> i32 {
    match symbol.symbology {
        // These are the "elite" standards which have support for specific character sets + ECI
        BARCODE_QRCODE => qrcode(symbol, segs),
        // These are the standards which have support for specific character sets but not ECI
        BARCODE_MICROQR => {
            let src = segs[0].source.clone();
            microqr(symbol, &src)
        }
        _ => reduced_charset(symbol, segs),
    }
}

/// Port of `reduced_charset()`: the "norm" standards which only support Latin-1 at most,
/// though a few support ECI.
fn reduced_charset(symbol: &mut ZintSymbol, segs: &mut [ZintSeg]) -> i32 {
    // One segment, so `segs_cpy()` is a plain copy.
    let convertible = is_eci_convertible(segs[0].eci);
    if (symbol.input_mode & 0x07) == UNICODE_MODE && convertible {
        let mut local_segs = segs.to_vec();
        match utf8_to_eci(local_segs[0].eci, &local_segs[0].source) {
            Ok(Ok(preprocessed)) => local_segs[0].source = preprocessed,
            Ok(Err(error_number)) => {
                if local_segs[0].eci != 0 {
                    let msg = format!("Invalid character in input for ECI '{}'", local_segs[0].eci);
                    return errtxt(error_number, symbol, 244, &msg);
                }
                return errtxt(
                    error_number,
                    symbol,
                    204,
                    "Invalid character in input (ISO/IEC 8859-1 only)",
                );
            }
            Err(_) => return NOT_PORTED,
        }
        call_symbology(symbol, &mut local_segs)
    } else {
        call_symbology(symbol, segs)
    }
}

/// Port of `strip_bom()`: removes a Unicode BOM at the start of the data (not if it is the only
/// data).
fn strip_bom(source: &mut Vec<u8>) {
    if source.len() > 3 && source[0] == 0xef && source[1] == 0xbb && source[2] == 0xbf {
        // BOM at start of input data, strip in accordance with RFC 3629
        source.drain(0..3);
    }
}

/// Port of `ZBarcode_Encode()` -> `ZBarcode_Encode_Segs()` with one segment. Returns zint's
/// return code (0, a warning < `ZINT_ERROR`, or an error); [`NOT_PORTED`] when the input needs
/// an ECI conversion the port omits.
pub(crate) fn zbarcode_encode(symbol: &mut ZintSymbol, source: &[u8]) -> i32 {
    let warn_number = 0;
    let mut have_zero_eci = false;

    if (symbol.input_mode & 0x07) > 2 {
        // Not reached: the input mode is fixed to UNICODE_MODE.
        return NOT_PORTED;
    }

    // Check the symbology field
    if !valid_id(symbol.symbology) {
        return NOT_PORTED;
    }

    // `escape_mode` is false (no ESCAPE_MODE / EXTRA_ESCAPE_MODE).

    // Check segment lengths (`length` > 0 is passed, so no `ustrlen()`)
    let mut local_seg = ZintSeg {
        source: source.to_vec(),
        eci: symbol.eci,
    };
    if local_seg.source.is_empty() {
        if supports_eci(symbol.symbology) {
            errtxt(0, symbol, 228, "No input data (segment 0 empty)");
        } else {
            errtxt(0, symbol, 778, "No input data");
        }
        return error_tag(ZINT_ERROR_INVALID_DATA, symbol, -1, None);
    }
    if local_seg.source.len() > ZINT_MAX_DATA_LEN {
        return error_tag(ZINT_ERROR_TOO_LONG, symbol, 777, Some("Input too long"));
    }

    // Reconcile symbol ECI and first segment ECI if both set: they are the same here.

    // Check ECI(s)
    if local_seg.eci != 0 {
        if !supports_eci(symbol.symbology) {
            return error_tag(
                ZINT_ERROR_INVALID_OPTION,
                symbol,
                217,
                Some("Symbology does not support ECI switching"),
            );
        }
        let eci = local_seg.eci;
        if eci < 0 || eci == 1 || eci == 2 || eci == 14 || eci == 19 || eci > 999999 {
            errtxt(
                0,
                symbol,
                218,
                &format!("ECI code '{eci}' out of range (0 to 999999, excluding 1, 2, 14 and 19)"),
            );
            return error_tag(ZINT_ERROR_INVALID_OPTION, symbol, -1, None);
        }
    } else {
        have_zero_eci = true;
    }

    // Check other symbol fields: all at their `ZBarcode_Create()` defaults, which pass
    // (scale 1, dot size 0.8, height 0, guard descent 5, text gap 1, no whitespace / border,
    // rows 0, not GS1_MODE).

    if (symbol.input_mode & 0x07) == UNICODE_MODE {
        if !is_valid_utf8(&local_seg.source) {
            return error_tag(ZINT_ERROR_INVALID_DATA, symbol, 245, Some("Invalid UTF-8 in input"));
        }
        // Only strip BOM on first segment
        strip_bom(&mut local_seg.source);
    }

    let mut local_segs = vec![local_seg];
    let mut error_number = extended_or_reduced_charset(symbol, &mut local_segs);
    if error_number == NOT_PORTED {
        return NOT_PORTED;
    }

    if error_number == ZINT_ERROR_INVALID_DATA
        && have_zero_eci
        && supports_eci(symbol.symbology)
        && (symbol.input_mode & 0x07) == UNICODE_MODE
    {
        // Try another ECI mode: `get_best_eci_segs()` is not ported (KiCad sets ECI 26 for every
        // non-ASCII QR Code / Data Matrix text, so ECI 0 input never fails to convert).
        return NOT_PORTED;
    }
    let _ = ZINT_WARN_USES_ECI;

    if error_number == 0 {
        error_number = warn_number; // Already tagged
    } else {
        error_number = error_tag(error_number, symbol, -1, None);
    }

    if error_number < ZINT_ERROR && symbol.height < 0.5 {
        // Absolute minimum
        let _ = set_height(symbol, 0.0, 50.0, 0.0, true);
    }

    let _ = ZINT_ERROR_ENCODING_PROBLEM;
    error_number
}

/// Port of `ZBarcode_Buffer_Vector( symbol, 0 )`: `check_output_args()` (rotation 0, no
/// BARCODE_DOTTY_MODE) then `plot_vector( symbol, 0, OUT_BUFFER )`.
pub(crate) fn zbarcode_buffer_vector(symbol: &mut ZintSymbol) -> i32 {
    let error_number = plot_vector(symbol);
    error_tag(error_number, symbol, -1, None)
}
