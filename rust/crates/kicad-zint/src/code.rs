//! Port of `code39()` of `thirdparty/zint/backend/code.c` (BARCODE_CODE39 only: the LOGMARS,
//! HIBC_39, check-digit and COMPLIANT_HEIGHT branches are unreachable with `option_2` 0 and the
//! default output options; the human readable text is not ported, `show_hrt` is 0).

use crate::common::{
    ZINT_ERROR_INVALID_DATA, ZINT_ERROR_TOO_LONG, errtxt, expand, not_sane_lookup, set_height,
    to_upper,
};
use crate::library::ZintSymbol;

/// `SILVER` (code.c).
const SILVER: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%abcd";

/// `C39Table` (code.c): widths of each character, the last is the Start (full 10) / Stop (first
/// 9) character.
const C39_TABLE: [&[u8; 10]; 44] = [
    b"1112212111", b"2112111121", b"1122111121", b"2122111111", b"1112211121", b"2112211111",
    b"1122211111", b"1112112121", b"2112112111", b"1122112111", b"2111121121", b"1121121121",
    b"2121121111", b"1111221121", b"2111221111", b"1121221111", b"1111122121", b"2111122111",
    b"1121122111", b"1111222111", b"2111111221", b"1121111221", b"2121111211", b"1111211221",
    b"2111211211", b"1121211211", b"1111112221", b"2111112211", b"1121112211", b"1111212211",
    b"2211111121", b"1221111121", b"2221111111", b"1211211121", b"2211211111", b"1221211111",
    b"1211112121", b"2211112111", b"1221112111", b"1212121111", b"1212111211", b"1211121211",
    b"1112121211", b"1211212111",
];

/// Port of `code39()` for BARCODE_CODE39 with `option_2` 0.
pub(crate) fn code39(symbol: &mut ZintSymbol, source: &mut [u8]) -> i32 {
    let length = source.len();

    if symbol.option_2 < 0 || symbol.option_2 > 2 {
        symbol.option_2 = 0;
    }

    if length > 86 {
        // 13 (Start) + 86*13 + 12 (Stop) = 1143
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            323,
            &format!("Input length {length} too long (maximum 86)"),
        );
    }

    to_upper(source);
    let posns = match not_sane_lookup(&SILVER[..43], source) {
        Ok(p) => p,
        Err(i) => {
            return errtxt(
                ZINT_ERROR_INVALID_DATA,
                symbol,
                324,
                &format!(
                    "Invalid character at position {i} in input (alphanumerics, space and \"-.$/+%\" only)"
                ),
            );
        }
    };

    let mut dest: Vec<u8> = Vec::with_capacity(890);

    // Start character
    dest.extend_from_slice(C39_TABLE[43]);

    let mut counter = 0usize;
    for &p in &posns {
        dest.extend_from_slice(C39_TABLE[p]);
        counter += p;
    }

    if symbol.option_2 == 1 || symbol.option_2 == 2 {
        // Visible or hidden check digit (not reached: KiCad leaves option_2 0)
        counter %= 43;
        dest.extend_from_slice(C39_TABLE[counter]);
    }

    // Stop character
    dest.extend_from_slice(&C39_TABLE[43][..9]);

    expand(symbol, &dest);

    // Not COMPLIANT_HEIGHT
    let _ = set_height(symbol, 0.0, 50.0, 0.0, true);

    0
}
