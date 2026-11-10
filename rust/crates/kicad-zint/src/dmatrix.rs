//! Port of `datamatrix()` (BARCODE_DATAMATRIX, ECC 200) of `thirdparty/zint/backend/dmatrix.c`:
//! the default minimal encodation (`dm_define_mode`, Dijkstra-based algorithm by Alex Geller,
//! and `dm_minimalenc`), end-of-data handling (`dm_encode`), the macro 05/06 header, automatic
//! size selection, padding, Reed-Solomon ECC (incl. the 144x144 skew) and the Annex F placement.
//!
//! Not reached with KiCad's settings (and not ported): the Annex J based `dm_isoenc` (FAST_MODE /
//! MAILMARK_2D only) and its look-ahead test, GS1 (`gs1` is always 0), structured append,
//! READER_INIT, DMRE / square-only size options and user-selected sizes (`option_2` 0).

use crate::common::{
    ZINT_ERROR_INVALID_OPTION, ZINT_ERROR_TOO_LONG, ctoi, errtxt, is_twodigits, posn, set_module,
    z_isdigit, z_islower, z_isupper,
};
use crate::dmatrix_tables::{
    DM_C40_SHIFT, DM_C40_VALUE, DM_INTSYMBOL, DM_ISDMRE, DM_MATRIXBYTES, DM_MATRIXDATABLOCK,
    DM_MATRIXFH, DM_MATRIXFW, DM_MATRIXH, DM_MATRIXRSBLOCK, DM_MATRIXW, DM_TEXT_SHIFT,
    DM_TEXT_VALUE,
};
use crate::library::{FAST_MODE, NOT_PORTED, ZintSeg, ZintSymbol};
use crate::reedsol::Rs;

const DM_ASCII: i32 = 1;
const DM_C40: i32 = 2;
const DM_TEXT: i32 = 3;
const DM_X12: i32 = 4;
const DM_EDIFACT: i32 = 5;
const DM_BASE256: i32 = 6;

/// `DMSIZESCOUNT`.
const DMSIZESCOUNT: usize = 48;
/// `DMINTSYMBOL144`: number of 144x144 for special interlace.
const DMINTSYMBOL144: usize = 47;

/// `DM_ISO_144` (zint.h).
const DM_ISO_144: i32 = 128;
/// `DM_SQUARE` (zint.h).
const DM_SQUARE: i32 = 100;
/// `DM_DMRE` (zint.h).
const DM_DMRE: i32 = 101;

/// Port of `dm_placementbit()`: Annex F placement algorithm low level.
fn dm_placementbit(array: &mut [i32], nr: i32, nc: i32, mut r: i32, mut c: i32, p: i32, b: i32) {
    if r < 0 {
        r += nr;
        c += 4 - ((nr + 4) % 8);
    }
    if c < 0 {
        c += nc;
        r += 4 - ((nc + 4) % 8);
    }
    // Necessary for DMRE (ISO/IEC 21471:2020 Annex E)
    if r >= nr {
        r -= nr;
    }
    // Check index limits
    debug_assert!(r < nr);
    debug_assert!(c < nc);
    // Check double-assignment
    debug_assert_eq!(0, array[(r * nc + c) as usize]);
    array[(r * nc + c) as usize] = (p << 3) + b;
}

/// Port of `dm_placementblock()`.
fn dm_placementblock(array: &mut [i32], nr: i32, nc: i32, r: i32, c: i32, p: i32) {
    dm_placementbit(array, nr, nc, r - 2, c - 2, p, 7);
    dm_placementbit(array, nr, nc, r - 2, c - 1, p, 6);
    dm_placementbit(array, nr, nc, r - 1, c - 2, p, 5);
    dm_placementbit(array, nr, nc, r - 1, c - 1, p, 4);
    dm_placementbit(array, nr, nc, r - 1, c, p, 3);
    dm_placementbit(array, nr, nc, r, c - 2, p, 2);
    dm_placementbit(array, nr, nc, r, c - 1, p, 1);
    dm_placementbit(array, nr, nc, r, c, p, 0);
}

/// Port of `dm_placementcornerA()`.
fn dm_placementcorner_a(array: &mut [i32], nr: i32, nc: i32, p: i32) {
    dm_placementbit(array, nr, nc, nr - 1, 0, p, 7);
    dm_placementbit(array, nr, nc, nr - 1, 1, p, 6);
    dm_placementbit(array, nr, nc, nr - 1, 2, p, 5);
    dm_placementbit(array, nr, nc, 0, nc - 2, p, 4);
    dm_placementbit(array, nr, nc, 0, nc - 1, p, 3);
    dm_placementbit(array, nr, nc, 1, nc - 1, p, 2);
    dm_placementbit(array, nr, nc, 2, nc - 1, p, 1);
    dm_placementbit(array, nr, nc, 3, nc - 1, p, 0);
}

/// Port of `dm_placementcornerB()`.
fn dm_placementcorner_b(array: &mut [i32], nr: i32, nc: i32, p: i32) {
    dm_placementbit(array, nr, nc, nr - 3, 0, p, 7);
    dm_placementbit(array, nr, nc, nr - 2, 0, p, 6);
    dm_placementbit(array, nr, nc, nr - 1, 0, p, 5);
    dm_placementbit(array, nr, nc, 0, nc - 4, p, 4);
    dm_placementbit(array, nr, nc, 0, nc - 3, p, 3);
    dm_placementbit(array, nr, nc, 0, nc - 2, p, 2);
    dm_placementbit(array, nr, nc, 0, nc - 1, p, 1);
    dm_placementbit(array, nr, nc, 1, nc - 1, p, 0);
}

/// Port of `dm_placementcornerC()`.
fn dm_placementcorner_c(array: &mut [i32], nr: i32, nc: i32, p: i32) {
    dm_placementbit(array, nr, nc, nr - 3, 0, p, 7);
    dm_placementbit(array, nr, nc, nr - 2, 0, p, 6);
    dm_placementbit(array, nr, nc, nr - 1, 0, p, 5);
    dm_placementbit(array, nr, nc, 0, nc - 2, p, 4);
    dm_placementbit(array, nr, nc, 0, nc - 1, p, 3);
    dm_placementbit(array, nr, nc, 1, nc - 1, p, 2);
    dm_placementbit(array, nr, nc, 2, nc - 1, p, 1);
    dm_placementbit(array, nr, nc, 3, nc - 1, p, 0);
}

/// Port of `dm_placementcornerD()`.
fn dm_placementcorner_d(array: &mut [i32], nr: i32, nc: i32, p: i32) {
    dm_placementbit(array, nr, nc, nr - 1, 0, p, 7);
    dm_placementbit(array, nr, nc, nr - 1, nc - 1, p, 6);
    dm_placementbit(array, nr, nc, 0, nc - 3, p, 5);
    dm_placementbit(array, nr, nc, 0, nc - 2, p, 4);
    dm_placementbit(array, nr, nc, 0, nc - 1, p, 3);
    dm_placementbit(array, nr, nc, 1, nc - 3, p, 2);
    dm_placementbit(array, nr, nc, 1, nc - 2, p, 1);
    dm_placementbit(array, nr, nc, 1, nc - 1, p, 0);
}

/// Port of `dm_placement()`: Annex F placement algorithm main function.
fn dm_placement(array: &mut [i32], nr: i32, nc: i32) {
    // start
    let mut p = 1;
    let mut r = 4;
    let mut c = 0;
    loop {
        // check corner
        if r == nr && c == 0 {
            dm_placementcorner_a(array, nr, nc, p);
            p += 1;
        }
        if r == nr - 2 && c == 0 && nc % 4 != 0 {
            dm_placementcorner_b(array, nr, nc, p);
            p += 1;
        }
        if r == nr - 2 && c == 0 && (nc % 8) == 4 {
            dm_placementcorner_c(array, nr, nc, p);
            p += 1;
        }
        if r == nr + 4 && c == 2 && (nc % 8) == 0 {
            dm_placementcorner_d(array, nr, nc, p);
            p += 1;
        }
        // up/right
        loop {
            if r < nr && c >= 0 && array[(r * nc + c) as usize] == 0 {
                dm_placementblock(array, nr, nc, r, c, p);
                p += 1;
            }
            r -= 2;
            c += 2;
            if !(r >= 0 && c < nc) {
                break;
            }
        }
        r += 1;
        c += 3;
        // down/left
        loop {
            if r >= 0 && c < nc && array[(r * nc + c) as usize] == 0 {
                dm_placementblock(array, nr, nc, r, c, p);
                p += 1;
            }
            r += 2;
            c -= 2;
            if !(r < nr && c >= 0) {
                break;
            }
        }
        r += 3;
        c += 1;
        if !(r < nr || c < nc) {
            break;
        }
    }
    // unfilled corner
    let last = (nr * nc - 1) as usize;
    if array[last] == 0 {
        array[last] = 1;
        array[(nr * nc - nc - 2) as usize] = 1;
    }
}

/// Port of `dm_ecc()`: calculates and appends the ECC code, interleaving if necessary.
fn dm_ecc(binary: &mut [u8], bytes: usize, datablock: usize, rsblock: usize, skew: bool) {
    let blocks = (bytes + 2) / datablock;
    let rsblocks = rsblock * blocks;

    let mut rs = Rs::init_gf(0x12d);
    rs.init_code(rsblock, 1);
    for b in 0..blocks {
        let mut buf = [0u8; 256];
        let mut ecc = [0u8; 256];
        let mut p = 0;
        let mut n = b;
        while n < bytes {
            buf[p] = binary[n];
            p += 1;
            n += blocks;
        }
        rs.encode(&buf[..p], &mut ecc);
        if skew {
            // Rotate ecc data to make 144x144 size symbols acceptable
            // See http://groups.google.com/group/postscriptbarcode/msg/5ae8fda7757477da
            // or https://github.com/nu-book/zxing-cpp/issues/259
            let mut n = b;
            let mut p = 0;
            while n < rsblocks {
                if b < 8 {
                    binary[bytes + n + 2] = ecc[p];
                } else {
                    binary[bytes + n - 8] = ecc[p];
                }
                n += blocks;
                p += 1;
            }
        } else {
            let mut n = b;
            let mut p = 0;
            while n < rsblocks {
                binary[bytes + n] = ecc[p];
                n += blocks;
                p += 1;
            }
        }
    }
}

/// Port of `dm_isc40()`: is basic (non-shifted) C40?
fn dm_isc40(input: u8) -> bool {
    if input <= b'9' {
        return input >= b'0' || input == b' ';
    }
    z_isupper(u32::from(input))
}

/// Port of `dm_istext()`: is basic (non-shifted) TEXT?
fn dm_istext(input: u8) -> bool {
    if input <= b'9' {
        return input >= b'0' || input == b' ';
    }
    z_islower(u32::from(input))
}

/// Port of `dm_isc40text()`: is basic (non-shifted) C40/TEXT?
fn dm_isc40text(current_mode: i32, input: u8) -> bool {
    if current_mode == DM_C40 { dm_isc40(input) } else { dm_istext(input) }
}

/// Port of `dm_isX12()`: whether a character is valid in the X12 set.
fn dm_is_x12(input: u8) -> bool {
    dm_isc40(input) || input == 13 || input == b'*' || input == b'>'
}

/// Port of `dm_isedifact()`: whether a character is valid in the EDIFACT set.
fn dm_isedifact(input: u8) -> bool {
    input >= b' ' && input <= b'^'
}

/// Port of `dm_ctx_buffer_xfer()`: copies C40/TEXT/X12 triplets from the buffer to the target.
/// Returns elements left in buffer (< 3).
fn dm_ctx_buffer_xfer(
    process_buffer: &mut [i32; 8],
    mut process_p: usize,
    target: &mut [u8],
    tp: &mut usize,
) -> usize {
    let process_e = (process_p / 3) * 3;

    let mut i = 0;
    while i < process_e {
        let iv = (1600 * process_buffer[i]) + (40 * process_buffer[i + 1]) + process_buffer[i + 2] + 1;
        target[*tp] = (iv >> 8) as u8;
        *tp += 1;
        target[*tp] = (iv & 0xFF) as u8;
        *tp += 1;
        i += 3;
    }

    process_p -= process_e;

    if process_p != 0 {
        process_buffer.copy_within(process_e..process_e + process_p, 0);
    }

    process_p
}

/// Port of `dm_edi_buffer_xfer()`: copies EDIFACT quadruplets from the buffer to the target.
/// Returns elements left in buffer (< 4).
fn dm_edi_buffer_xfer(
    process_buffer: &mut [i32; 8],
    mut process_p: usize,
    target: &mut [u8],
    tp: &mut usize,
    empty: bool,
) -> usize {
    let process_e = (process_p / 4) * 4;
    let mut push = |target: &mut [u8], v: i32| {
        target[*tp] = v as u8;
        *tp += 1;
    };

    let mut i = 0;
    while i < process_e {
        let pb = &*process_buffer;
        push(target, pb[i] << 2 | (pb[i + 1] & 0x30) >> 4);
        push(target, (pb[i + 1] & 0x0f) << 4 | (pb[i + 2] & 0x3c) >> 2);
        push(target, (pb[i + 2] & 0x03) << 6 | pb[i + 3]);
        i += 4;
    }

    process_p -= process_e;

    if process_p != 0 {
        process_buffer.copy_within(process_e..process_e + process_p, 0);
        if empty {
            // Note zint reads `process_buffer[i]` with `i == process_e` after the move, i.e. the
            // elements' old positions, which the move left unchanged.
            let pb = &*process_buffer;
            if process_p == 3 {
                push(target, pb[i] << 2 | (pb[i + 1] & 0x30) >> 4);
                push(target, (pb[i + 1] & 0x0f) << 4 | (pb[i + 2] & 0x3c) >> 2);
                push(target, (pb[i + 2] & 0x03) << 6);
            } else if process_p == 2 {
                push(target, pb[i] << 2 | (pb[i + 1] & 0x30) >> 4);
                push(target, (pb[i + 1] & 0x0f) << 4);
            } else {
                push(target, pb[i] << 2);
            }
            process_p = 0;
        }
    }

    process_p
}

/// Port of `dm_get_symbolsize()`: index of the symbol size in `dm_matrixbytes`, as specified
/// or else the smallest containing `minimum` codewords.
fn dm_get_symbolsize(symbol: &ZintSymbol, minimum: i32) -> usize {
    if symbol.option_2 >= 1 && symbol.option_2 <= DMSIZESCOUNT as i32 {
        return DM_INTSYMBOL[(symbol.option_2 - 1) as usize] as usize;
    }
    if minimum > 1304 {
        return if minimum <= 1558 { DMSIZESCOUNT - 1 } else { 0 };
    }
    let mut i = if minimum >= 62 { 23 } else { 0 };
    while minimum > i32::from(DM_MATRIXBYTES[i]) {
        i += 1;
    }

    if (symbol.option_3 & 0x7F) == DM_DMRE {
        return i;
    }
    if (symbol.option_3 & 0x7F) == DM_SQUARE {
        // Skip rectangular symbols in square only mode
        while DM_MATRIXH[i] != DM_MATRIXW[i] {
            i += 1;
        }
        return i;
    }
    // Skip DMRE symbols in no dmre mode
    while DM_ISDMRE[i] != 0 {
        i += 1;
    }
    i
}

/// Port of `dm_codewords_remaining()`: number of codewords remaining in a particular version
/// (may be negative).
fn dm_codewords_remaining(symbol: &ZintSymbol, tp: i32, process_p: i32) -> i32 {
    let symbolsize = dm_get_symbolsize(symbol, tp + process_p); // Allow for the remaining data characters

    i32::from(DM_MATRIXBYTES[symbolsize]) - tp
}

/// Port of `dm_c40text_cnt()` (gs1 0): number of C40/TEXT elements needed to encode `input`.
fn dm_c40text_cnt(current_mode: i32, mut input: u8) -> i32 {
    let mut cnt = 1;
    if input & 0x80 != 0 {
        cnt += 2;
        input -= 128;
    }
    if (current_mode == DM_C40 && DM_C40_SHIFT[input as usize] != 0)
        || (current_mode == DM_TEXT && DM_TEXT_SHIFT[input as usize] != 0)
    {
        cnt += 1;
    }

    cnt
}

/// Port of `dm_update_b256_field_length()`: updates the Base 256 field length.
fn dm_update_b256_field_length(target: &mut [u8], mut tp: usize, b256_start: usize) -> usize {
    let b256_count = tp - (b256_start + 1);
    if b256_count <= 249 {
        target[b256_start] = b256_count as u8;
    } else {
        // Insert extra codeword
        target.copy_within(b256_start + 1..b256_start + 1 + b256_count, b256_start + 2);
        target[b256_start] = (249 + (b256_count / 250)) as u8;
        target[b256_start + 1] = (b256_count % 250) as u8;
        tp += 1;
    }

    tp
}

/// Port of `dm_switch_mode()`: switches from ASCII or Base 256 to another mode.
fn dm_switch_mode(next_mode: i32, target: &mut [u8], mut tp: usize, p_b256_start: &mut usize) -> usize {
    match next_mode {
        DM_C40 => {
            target[tp] = 230;
            tp += 1;
        }
        DM_TEXT => {
            target[tp] = 239;
            tp += 1;
        }
        DM_X12 => {
            target[tp] = 238;
            tp += 1;
        }
        DM_EDIFACT => {
            target[tp] = 240;
            tp += 1;
        }
        DM_BASE256 => {
            target[tp] = 231;
            tp += 1;
            *p_b256_start = tp;
            target[tp] = 0; // Byte count holder (may be expanded to 2 codewords)
            tp += 1;
        }
        _ => {} // DM_ASCII
    }

    tp
}

const DM_NUM_MODES: usize = 6;

/// Port of `struct dm_edge`. The C fields are `unsigned char` / `unsigned short`; the values
/// stay within those ranges (input length <= 3116, sizes <= 1558 + a few).
#[derive(Clone, Copy, Default)]
struct DmEdge {
    mode: u8,
    end_mode: u8, // Mode returned by `dm_getEndMode()`
    #[allow(dead_code)] // Only read by zint's trace output
    from: u16, // Position in input data, 0-based
    len: u16,
    size: u16,     // Cumulative number of codewords
    bytes: u16,    // DM_BASE256 byte count, kept to avoid runtime calc
    previous: u16, // Index into edges array
}

/// Port of `dm_last_ascii()`: whether the next 1 to 4 chars are at EOD and can be encoded as 1
/// or 2 ASCII codewords.
fn dm_last_ascii(source: &[u8], from: usize) -> i32 {
    let length = source.len();
    if from >= length || length - from > 4 {
        return 0;
    }
    let d = |i: usize| z_isdigit(u32::from(source[i]));
    let hi = |i: usize| source[i] & 0x80 != 0;
    if length - from == 1 {
        if hi(from) {
            return 0;
        }
        return 1;
    }
    if length - from == 2 {
        if hi(from) || hi(from + 1) {
            return 0;
        }
        if d(from) && d(from + 1) {
            return 1;
        }
        return 2;
    }
    if length - from == 3 {
        if d(from) && d(from + 1) && !hi(from + 2) {
            return 2;
        }
        if d(from + 1) && d(from + 2) && !hi(from) {
            return 2;
        }
        return 0;
    }
    if d(from) && d(from + 1) && d(from + 2) && d(from + 3) {
        return 2;
    }
    0
}

/// Context of the minimal encodation of one segment.
struct DmCtx<'a> {
    symbol: &'a ZintSymbol,
    source: &'a [u8],
    last_seg: bool,
}

impl DmCtx<'_> {
    /// Port of `dm_getEndMode()`: treats EDIFACT edges specially, returning DM_ASCII mode if
    /// not full (i.e. encoding < 4 chars), or if full and at EOD where 1 or 2 ASCII chars can
    /// be encoded.
    fn get_end_mode(&self, mode: i32, from: usize, len: usize, size: i32) -> i32 {
        if mode == DM_EDIFACT {
            if len < 4 {
                return DM_ASCII;
            }
            if self.last_seg {
                let last_ascii = dm_last_ascii(self.source, from + len);
                if last_ascii != 0 {
                    // At EOD with remaining chars ASCII-encodable in 1 or 2 codewords
                    let symbols_left = dm_codewords_remaining(self.symbol, size + last_ascii, 0);
                    // If no codewords left and 1 or 2 ASCII-encodables or 1 codeword left and 1
                    // ASCII-encodable
                    if symbols_left <= 2 - last_ascii {
                        return DM_ASCII;
                    }
                }
            }
        }
        mode
    }

    /// Port of `dm_new_Edge()`: initializes a new edge, returning it (its `end_mode` is the
    /// C return value).
    fn new_edge(
        &self,
        edges: &[DmEdge],
        mode: i32,
        from: usize,
        len: usize,
        previous: Option<usize>,
        cwds: i32,
    ) -> DmEdge {
        let source = self.source;
        let length = source.len();
        let mut edge = DmEdge {
            mode: mode as u8,
            end_mode: mode as u8,
            from: from as u16,
            len: len as u16,
            bytes: 0,
            ..DmEdge::default()
        };
        let previous_mode: i32;
        let mut size: i32;
        if let Some(pi) = previous {
            let prev = &edges[pi];
            debug_assert!(prev.mode != 0 && prev.len != 0 && prev.size != 0 && prev.end_mode != 0);
            previous_mode = i32::from(prev.end_mode);
            edge.previous = pi as u16;
            size = i32::from(prev.size);
        } else {
            previous_mode = DM_ASCII;
            edge.previous = 0;
            size = 0;
        }

        match mode {
            DM_ASCII => {
                debug_assert!(previous_mode != DM_EDIFACT);
                size += 1;
                if source[from] & 0x80 != 0 {
                    size += 1;
                }
                if previous_mode != DM_ASCII && previous_mode != DM_BASE256 {
                    size += 1; // Unlatch to ASCII
                }
            }
            DM_BASE256 => {
                debug_assert!(previous_mode != DM_EDIFACT);
                size += 1;
                if previous_mode != DM_BASE256 {
                    size += 2; // Byte count + latch to BASE256
                    if previous_mode != DM_ASCII {
                        size += 1; // Unlatch to ASCII
                    }
                    edge.bytes = 1;
                } else {
                    let prev = &edges[previous.expect("previous edge")];
                    edge.bytes = 1 + prev.bytes;
                    if edge.bytes == 250 {
                        size += 1; // Extra byte count
                    }
                }
            }
            DM_C40 | DM_TEXT => {
                debug_assert!(previous_mode != DM_EDIFACT);
                size += cwds;
                if previous_mode != mode {
                    size += 1; // Latch to this mode
                    if previous_mode != DM_ASCII && previous_mode != DM_BASE256 {
                        size += 1; // Unlatch to ASCII
                    }
                }
                if self.last_seg && from + len + 2 >= length {
                    // If less than batch of 3 away from EOD
                    let last_ascii = dm_last_ascii(source, from + len);
                    let symbols_left = dm_codewords_remaining(self.symbol, size + last_ascii, 0);
                    if symbols_left > 0 {
                        size += 1; // We need an extra unlatch at the end
                    }
                }
            }
            DM_X12 => {
                debug_assert!(previous_mode != DM_EDIFACT);
                size += 2;
                if previous_mode != DM_X12 {
                    size += 1; // Latch to this mode
                    if previous_mode != DM_ASCII && previous_mode != DM_BASE256 {
                        size += 1; // Unlatch to ASCII
                    }
                }
                if self.last_seg && from + len + 2 >= length {
                    // If less than batch of 3 away from EOD
                    let last_ascii = dm_last_ascii(source, from + len);
                    if last_ascii == 2 {
                        // Only 1 ASCII-encodable allowed at EOD for X12, unlike C40/TEXT
                        size += 1; // We need an extra unlatch at the end
                    } else {
                        let symbols_left = dm_codewords_remaining(self.symbol, size + last_ascii, 0);
                        if symbols_left > 0 {
                            size += 1; // We need an extra unlatch at the end
                        }
                    }
                }
            }
            DM_EDIFACT => {
                size += 3;
                if previous_mode != DM_EDIFACT {
                    size += 1; // Latch to this mode
                    if previous_mode != DM_ASCII && previous_mode != DM_BASE256 {
                        size += 1; // Unlatch to ASCII
                    }
                }
                edge.end_mode = self.get_end_mode(mode, from, len, size) as u8;
            }
            _ => {}
        }
        edge.size = size as u16;

        edge
    }

    /// Port of `dm_addEdge()`: adds an edge for a mode at a vertex if no existing edge or if
    /// more optimal than the existing edge.
    fn add_edge(
        &self,
        edges: &mut [DmEdge],
        mode: i32,
        from: usize,
        len: usize,
        previous: Option<usize>,
        cwds: i32,
    ) {
        let edge = self.new_edge(edges, mode, from, len, previous, cwds);
        let vertex_index = from + len;
        let v_ij = vertex_index * DM_NUM_MODES + edge.end_mode as usize - 1;

        if edges[v_ij].mode == 0 || edges[v_ij].size > edge.size {
            edges[v_ij] = edge;
        }
    }

    /// Port of `dm_addEdges()` (gs1 0): adds edges for the various modes at a vertex.
    fn add_edges(&self, edges: &mut [DmEdge], from: usize, previous: Option<usize>) {
        let source = self.source;
        let length = source.len();

        // Not possible to unlatch a full EDF edge to something else
        if previous.is_none_or(|p| i32::from(edges[p].end_mode) != DM_EDIFACT) {
            const C40TEXT_MODES: [i32; 2] = [DM_C40, DM_TEXT];

            if z_isdigit(u32::from(source[from]))
                && from + 1 < length
                && z_isdigit(u32::from(source[from + 1]))
            {
                self.add_edge(edges, DM_ASCII, from, 2, previous, 0);
                // If ASCII vertex, don't bother adding other edges as this will be optimal;
                // suggested by Alex Geller
                if previous.is_some_and(|p| i32::from(edges[p].mode) == DM_ASCII) {
                    return;
                }
            } else {
                self.add_edge(edges, DM_ASCII, from, 1, previous, 0);
            }

            for &m in &C40TEXT_MODES {
                let (cwds, len) = dm_get_number_of_c40_words(source, from, m);
                if cwds != 0 {
                    self.add_edge(edges, m, from, len, previous, cwds);
                }
            }

            if from + 2 < length
                && dm_is_x12(source[from])
                && dm_is_x12(source[from + 1])
                && dm_is_x12(source[from + 2])
            {
                self.add_edge(edges, DM_X12, from, 3, previous, 0);
            }

            // `gs1 != 1 || source[from] != '\x1D'` (gs1 0)
            self.add_edge(edges, DM_BASE256, from, 1, previous, 0);
        }

        if dm_isedifact(source[from]) {
            // We create 3 EDF edges, 2, 3 or 4 characters length. The 4-char normally doesn't have
            // a latch to ASCII unless it is 2 characters away from the end of the input.
            let mut i = 1;
            let mut pos = from + i;
            while i < 4 && pos < length && dm_isedifact(source[pos]) {
                self.add_edge(edges, DM_EDIFACT, from, i + 1, previous, 0);
                i += 1;
                pos += 1;
            }
        }
    }

    /// Port of `dm_define_mode()`: calculates optimized encoding modes.
    fn define_mode(&self, modes: &mut [i32]) {
        let length = self.source.len();
        let mut edges = vec![DmEdge::default(); (length + 1) * DM_NUM_MODES];
        self.add_edges(&mut edges, 0, None);

        for i in 1..length {
            let v_i = i * DM_NUM_MODES;
            for j in 0..DM_NUM_MODES {
                if edges[v_i + j].mode != 0 {
                    self.add_edges(&mut edges, i, Some(v_i + j));
                }
            }
        }

        let v_i = length * DM_NUM_MODES;
        let mut minimal_j: Option<usize> = None;
        let mut minimal_size = i32::MAX;
        for j in 0..DM_NUM_MODES {
            let edge = &edges[v_i + j];
            if edge.mode != 0 && i32::from(edge.size) < minimal_size {
                minimal_size = i32::from(edge.size);
                minimal_j = Some(j);
            }
        }
        let minimal_j = minimal_j.expect("dm_define_mode: no final edge");

        let mut edge = Some(v_i + minimal_j);
        let mut mode_len = 0usize;
        let mut mode_end = length;
        while let Some(ei) = edge {
            let current_mode = i32::from(edges[ei].mode);
            mode_len += edges[ei].len as usize;
            // DM_PREVIOUS: 1st row of edges not used so valid previous cannot point there
            edge = if edges[ei].previous != 0 { Some(edges[ei].previous as usize) } else { None };
            if edge.is_none_or(|p| i32::from(edges[p].mode) != current_mode) {
                for m in modes.iter_mut().take(mode_end).skip(mode_end - mode_len) {
                    *m = current_mode;
                }
                mode_end -= mode_len;
                mode_len = 0;
            }
        }
        debug_assert_eq!(mode_end, 0);
    }
}

/// Port of `dm_getNumberOfC40Words()`: number of C40/TEXT codewords needed to encode characters
/// in full batches of 3 (or less if EOD), with the number of characters encoded.
fn dm_get_number_of_c40_words(source: &[u8], from: usize, mode: i32) -> (i32, usize) {
    let length = source.len();
    let mut thirds_count = 0;

    for i in from..length {
        let ci = source[i];

        if dm_isc40text(mode, ci) {
            thirds_count += 1; // Native
        } else if ci & 0x80 == 0 {
            thirds_count += 2; // Shift
        } else if dm_isc40text(mode, ci & 0x7F) {
            thirds_count += 3; // Shift, Upper shift
        } else {
            thirds_count += 4; // Shift, Upper shift, shift
        }

        let remainder = thirds_count % 3;
        if remainder == 0 || (remainder == 2 && i + 1 == length) {
            return (((thirds_count + 2) / 3) * 2, i - from + 1);
        }
    }
    (0, 0)
}

/// Encoder state carried between `dm_minimalenc()` and the end of `dm_encode()`.
struct DmState {
    sp: usize,
    tp: usize,
    process_buffer: [i32; 8],
    process_p: usize,
    b256_start: usize,
    current_mode: i32,
}

/// Port of `dm_minimalenc()` (gs1 0): the default minimal encodation.
fn dm_minimalenc(
    symbol: &mut ZintSymbol,
    source: &[u8],
    last_seg: bool,
    target: &mut [u8],
    st: &mut DmState,
) -> i32 {
    let length = source.len();
    let mut modes = vec![0i32; length];

    debug_assert!(length <= 10921); // Can only handle (10921 + 1) * 6 = 65532 < 65536 (2*16) due to sizeof(previous)

    DmCtx {
        symbol,
        source,
        last_seg,
    }
    .define_mode(&mut modes);

    let mut sp = st.sp;
    let mut tp = st.tp;
    let mut process_p = st.process_p;
    let mut current_mode = st.current_mode;
    let process_buffer = &mut st.process_buffer;

    while sp < length {
        if modes[sp] != current_mode {
            match current_mode {
                DM_C40 | DM_TEXT | DM_X12 => {
                    process_p = 0; // Throw away buffer if any
                    target[tp] = 254; // Unlatch
                    tp += 1;
                }
                DM_EDIFACT => {
                    if last_seg {
                        let last_ascii = dm_last_ascii(source, sp);
                        if last_ascii == 0 {
                            process_buffer[process_p] = 31; // Unlatch
                            process_p += 1;
                        } else {
                            let symbols_left =
                                dm_codewords_remaining(symbol, tp as i32 + last_ascii, process_p as i32);
                            if symbols_left > 2 - last_ascii {
                                process_buffer[process_p] = 31; // Unlatch
                                process_p += 1;
                            }
                        }
                    }
                    process_p = dm_edi_buffer_xfer(process_buffer, process_p, target, &mut tp, true);
                }
                DM_BASE256 => {
                    tp = dm_update_b256_field_length(target, tp, st.b256_start);
                    // B.2.1 255-state randomising algorithm
                    for i in st.b256_start..tp {
                        let prn = ((149 * (i + 1)) % 255) + 1;
                        target[i] = ((target[i] as usize + prn) & 0xFF) as u8;
                    }
                }
                _ => {}
            }
            tp = dm_switch_mode(modes[sp], target, tp, &mut st.b256_start);
        }

        current_mode = modes[sp];
        debug_assert!(current_mode != 0);

        if current_mode == DM_ASCII {
            if is_twodigits(source, sp) {
                target[tp] = ((10 * ctoi(source[sp])) + ctoi(source[sp + 1]) + 130) as u8;
                tp += 1;
                sp += 2;
            } else {
                if source[sp] & 0x80 != 0 {
                    target[tp] = 235; // FNC4
                    tp += 1;
                    target[tp] = (source[sp] - 128) + 1;
                    tp += 1;
                } else {
                    target[tp] = source[sp] + 1;
                    tp += 1;
                }
                sp += 1;
            }
        } else if current_mode == DM_C40 || current_mode == DM_TEXT {
            let (ct_shift, ct_value) = if current_mode == DM_C40 {
                (&DM_C40_SHIFT, &DM_C40_VALUE)
            } else {
                (&DM_TEXT_SHIFT, &DM_TEXT_VALUE)
            };

            let shift_set: i32;
            let value: i32;
            if source[sp] & 0x80 != 0 {
                process_buffer[process_p] = 1;
                process_p += 1;
                process_buffer[process_p] = 30; // Upper Shift
                process_p += 1;
                shift_set = i32::from(ct_shift[(source[sp] - 128) as usize]);
                value = i32::from(ct_value[(source[sp] - 128) as usize]);
            } else {
                shift_set = i32::from(ct_shift[source[sp] as usize]);
                value = i32::from(ct_value[source[sp] as usize]);
            }

            if shift_set != 0 {
                process_buffer[process_p] = shift_set - 1;
                process_p += 1;
            }
            process_buffer[process_p] = value;
            process_p += 1;

            if process_p >= 3 {
                process_p = dm_ctx_buffer_xfer(process_buffer, process_p, target, &mut tp);
            }
            sp += 1;
        } else if current_mode == DM_X12 {
            const X12_NONALPHANUM_CHARS: &[u8] = b"\x0d*> ";
            let c = source[sp];
            let value = if z_isdigit(u32::from(c)) {
                i32::from(c - b'0') + 4
            } else if z_isupper(u32::from(c)) {
                i32::from(c - b'A') + 14
            } else {
                posn(X12_NONALPHANUM_CHARS, c)
            };

            process_buffer[process_p] = value;
            process_p += 1;

            if process_p >= 3 {
                process_p = dm_ctx_buffer_xfer(process_buffer, process_p, target, &mut tp);
            }
            sp += 1;
        } else if current_mode == DM_EDIFACT {
            let mut value = i32::from(source[sp]);

            if value >= 64 {
                // '@'
                value -= 64;
            }

            process_buffer[process_p] = value;
            process_p += 1;
            sp += 1;

            if process_p >= 4 {
                process_p = dm_edi_buffer_xfer(process_buffer, process_p, target, &mut tp, false);
            }
        } else if current_mode == DM_BASE256 {
            target[tp] = source[sp];
            tp += 1;
            sp += 1;
        }

        if tp > 1558 {
            return errtxt(
                ZINT_ERROR_TOO_LONG,
                symbol,
                729,
                "Input too long, requires too many codewords (maximum 1558)",
            );
        }
    }

    st.sp = sp;
    st.tp = tp;
    st.process_p = process_p;
    st.current_mode = current_mode;

    0
}

/// Port of `dm_encode()` (gs1 0): encodes one segment into `target` from `*p_tp`.
fn dm_encode(
    symbol: &mut ZintSymbol,
    source: &[u8],
    eci: i32,
    last_seg: bool,
    target: &mut [u8],
    p_tp: &mut usize,
) -> i32 {
    let length = source.len();
    let mut st = DmState {
        sp: 0,
        tp: *p_tp,
        process_buffer: [0; 8],
        process_p: 0,
        b256_start: 0,
        current_mode: DM_ASCII,
    };

    if eci > 0 {
        // Encode ECI numbers according to Table 6
        let mut push = |v: i32| {
            target[st.tp] = v as u8;
            st.tp += 1;
        };
        push(241); // ECI Character
        if eci <= 126 {
            push(eci + 1);
        } else if eci <= 16382 {
            push((eci - 127) / 254 + 128);
            push((eci - 127) % 254 + 1);
        } else {
            push((eci - 16383) / 64516 + 192);
            push(((eci - 16383) / 254) % 254 + 1);
            push((eci - 16383) % 254 + 1);
        }
    }

    // If FAST_MODE or MAILMARK_2D, do Annex J-based encodation
    if symbol.input_mode & FAST_MODE != 0 {
        return NOT_PORTED; // `dm_isoenc()` not ported
    }
    // Do default minimal encodation
    let error_number = dm_minimalenc(symbol, source, last_seg, target, &mut st);
    if error_number != 0 {
        return error_number;
    }

    let DmState {
        mut sp,
        mut tp,
        mut process_buffer,
        mut process_p,
        b256_start,
        current_mode,
    } = st;

    let symbols_left = if last_seg {
        dm_codewords_remaining(symbol, tp as i32, process_p as i32)
    } else {
        3
    };

    let push = |target: &mut [u8], tp: &mut usize, v: i32| {
        target[*tp] = v as u8;
        *tp += 1;
    };

    if current_mode == DM_C40 || current_mode == DM_TEXT {
        // NOTE: changed to follow spec exactly here, only using Shift 1 padded triplets when 2
        // symbol chars remain. This matches the behaviour of BWIPP but not TEC-IT, nor figures
        // 4.15.1-1 and 4.15-1-2 in GS1 General Specifications 21.0.1.
        if process_p == 0 {
            if symbols_left > 0 {
                push(target, &mut tp, 254); // Unlatch
            }
        } else if process_p == 2 && symbols_left == 2 {
            // 5.2.5.2 (b)
            process_buffer[process_p] = 0; // Shift 1
            process_p += 1;
            let _ = dm_ctx_buffer_xfer(&mut process_buffer, process_p, target, &mut tp);
        } else if process_p == 1 && symbols_left <= 2 && dm_isc40text(current_mode, source[length - 1])
        {
            // 5.2.5.2 (c)/(d)
            if symbols_left > 1 {
                // 5.2.5.2 (c)
                push(target, &mut tp, 254); // Unlatch and encode remaining data in ASCII.
            }
            push(target, &mut tp, i32::from(source[length - 1]) + 1);
        } else {
            let mut total_cnt = 0;
            let mut process_p_i = process_p as i32;
            // Backtrack to last complete triplet (same technique as BWIPP)
            while sp > 0 && process_p_i % 3 != 0 {
                sp -= 1;
                let cnt = dm_c40text_cnt(current_mode, source[sp]);
                total_cnt += cnt;
                process_p_i -= cnt;
            }
            tp -= ((total_cnt / 3) * 2) as usize;

            push(target, &mut tp, 254); // Unlatch
            while sp < length {
                if is_twodigits(source, sp) {
                    push(target, &mut tp, (10 * ctoi(source[sp])) + ctoi(source[sp + 1]) + 130);
                    sp += 1;
                } else if source[sp] & 0x80 != 0 {
                    push(target, &mut tp, 235); // FNC4
                    push(target, &mut tp, i32::from(source[sp] - 128) + 1);
                } else {
                    push(target, &mut tp, i32::from(source[sp]) + 1);
                }
                sp += 1;
            }
        }
    } else if current_mode == DM_X12 {
        if symbols_left == 1 && process_p == 1 {
            // Unlatch not required!
            push(target, &mut tp, i32::from(source[length - 1]) + 1);
        } else {
            if symbols_left > 0 {
                push(target, &mut tp, 254); // Unlatch.
            }

            if process_p == 1 {
                push(target, &mut tp, i32::from(source[length - 1]) + 1);
            } else if process_p == 2 {
                push(target, &mut tp, i32::from(source[length - 2]) + 1);
                push(target, &mut tp, i32::from(source[length - 1]) + 1);
            }
        }
    } else if current_mode == DM_EDIFACT {
        if symbols_left <= 2 && process_p as i32 <= symbols_left {
            // Unlatch not required!
            if process_p == 1 {
                push(target, &mut tp, i32::from(source[length - 1]) + 1);
            } else if process_p == 2 {
                push(target, &mut tp, i32::from(source[length - 2]) + 1);
                push(target, &mut tp, i32::from(source[length - 1]) + 1);
            }
        } else {
            // Append EDIFACT unlatch value (31) and empty buffer
            if process_p <= 3 {
                process_buffer[process_p] = 31;
                process_p += 1;
            }
            let _ = dm_edi_buffer_xfer(&mut process_buffer, process_p, target, &mut tp, true);
        }
    } else if current_mode == DM_BASE256 {
        if symbols_left > 0 {
            tp = dm_update_b256_field_length(target, tp, b256_start);
        }
        // B.2.1 255-state randomising algorithm
        for i in b256_start..tp {
            let prn = ((149 * (i + 1)) % 255) + 1;
            target[i] = ((target[i] as usize + prn) & 0xFF) as u8;
        }
    }

    *p_tp = tp;

    0
}

/// Port of `dm_encode_segs()` (one segment, no structured append / GS1 / READER_INIT): the
/// macro 05/06 header check and `dm_encode()`.
fn dm_encode_segs(symbol: &mut ZintSymbol, seg: &ZintSeg, target: &mut [u8], p_binlen: &mut usize) -> i32 {
    let mut tp = 0usize;
    let mut in_macro = false;
    let source = &seg.source;
    let i = source.len();

    if i > 3116 {
        // Max is 3166 digits
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            719,
            &format!("Input length {i} too long (maximum 3116)"),
        );
    }

    // Check for Macro05/Macro06
    // "[)>[RS]05[GS]...[RS][EOT]" -> CW 236
    // "[)>[RS]06[GS]...[RS][EOT]" -> CW 237
    let len = source.len();
    if tp == 0
        && len >= 9
        && source[0] == b'['
        && source[1] == b')'
        && source[2] == b'>'
        && source[3] == 0x1e // RS
        && source[4] == b'0'
        && (source[5] == b'5' || source[5] == b'6')
        && source[6] == 0x1d // GS
        && source[len - 1] == 0x04 // EOT
        && source[len - 2] == 0x1e
    // RS
    {
        // Output macro Codeword
        if source[5] == b'5' {
            target[tp] = 236;
        } else {
            target[tp] = 237;
        }
        tp += 1;
        // Remove macro characters from input string
        in_macro = true;
    }

    let (src_inc, len_dec) = if in_macro { (7, 7 + 2) } else { (0, 0) };
    let body = &source[src_inc..src_inc + (len - len_dec)];
    if body.is_empty() {
        // "[)>\x1e05\x1d\x1e\x04" exactly: zint encodes an empty segment past the end of its edge
        // array (undefined behaviour); the port reports it instead.
        return NOT_PORTED;
    }
    let error_number = dm_encode(symbol, body, seg.eci, true, target, &mut tp);
    if error_number != 0 {
        return error_number;
    }

    *p_binlen = tp;

    0
}

/// Port of `dm_add_tail()`: adds pad bits.
fn dm_add_tail(target: &mut [u8], mut tp: usize, tail_length: usize) {
    target[tp] = 129; // Pad
    tp += 1;
    for _ in 1..tail_length {
        // B.1.1 253-state randomising algorithm
        let prn = ((149 * (tp + 1)) % 253) + 1;
        let temp = 129 + prn;
        if temp <= 254 {
            target[tp] = temp as u8;
        } else {
            target[tp] = (temp - 254) as u8;
        }
        tp += 1;
    }
}

/// Port of `dm_ecc200()`.
fn dm_ecc200(symbol: &mut ZintSymbol, seg: &ZintSeg) -> i32 {
    let mut binary = [0u8; 2200];
    let mut binlen = 0usize;
    let mut skew = false;

    // `length` may be decremented by 2 if macro character is used
    let error_number = dm_encode_segs(symbol, seg, &mut binary, &mut binlen);
    if error_number != 0 {
        return error_number;
    }

    let symbolsize = dm_get_symbolsize(symbol, binlen as i32);

    if binlen > DM_MATRIXBYTES[symbolsize] as usize {
        if symbol.option_2 >= 1 && symbol.option_2 <= DMSIZESCOUNT as i32 {
            // The symbol size was given by --ver (option_2)
            let msg = format!(
                "Input too long for Version {}, requires {binlen} codewords (maximum {})",
                symbol.option_2, DM_MATRIXBYTES[symbolsize]
            );
            return errtxt(ZINT_ERROR_TOO_LONG, symbol, 522, &msg);
        }
        let msg = format!("Input too long, requires {binlen} codewords (maximum 1558)");
        return errtxt(ZINT_ERROR_TOO_LONG, symbol, 523, &msg);
    }

    // Feedback options
    if symbol.option_2 < 1 || symbol.option_2 > DMSIZESCOUNT as i32 {
        let i = DM_INTSYMBOL
            .iter()
            .position(|&s| s as usize == symbolsize)
            .expect("size in dm_intsymbol");
        symbol.option_2 = i as i32 + 1;
    }

    let h = DM_MATRIXH[symbolsize] as i32;
    let w = DM_MATRIXW[symbolsize] as i32;
    let fh = DM_MATRIXFH[symbolsize] as i32;
    let fw = DM_MATRIXFW[symbolsize] as i32;
    let bytes = DM_MATRIXBYTES[symbolsize] as usize;
    let datablock = DM_MATRIXDATABLOCK[symbolsize] as usize;
    let rsblock = DM_MATRIXRSBLOCK[symbolsize] as usize;

    let taillength = bytes - binlen;

    if taillength != 0 {
        dm_add_tail(&mut binary, binlen, taillength);
    }

    // ecc code
    if symbolsize == DMINTSYMBOL144 && (symbol.option_3 & DM_ISO_144) == 0 {
        skew = true;
    }
    dm_ecc(&mut binary, bytes, datablock, rsblock, skew);

    // placement
    {
        let nc = w - 2 * (w / fw);
        let nr = h - 2 * (h / fh);
        let mut places = vec![0i32; (nc * nr) as usize];
        dm_placement(&mut places, nr, nc);
        let mut y = 0;
        while y < h {
            for x in 0..w {
                set_module(symbol, (h - y) - 1, x);
            }
            let mut x = 0;
            while x < w {
                set_module(symbol, y, x);
                x += 2;
            }
            y += fh;
        }
        let mut x = 0;
        while x < w {
            for y in 0..h {
                set_module(symbol, (h - y) - 1, x);
            }
            let mut y = 0;
            while y < h {
                set_module(symbol, (h - y) - 1, x + fw - 1);
                y += 2;
            }
            x += fw;
        }
        for y in 0..nr {
            for x in 0..nc {
                let v = places[((nr - y - 1) * nc + x) as usize];
                if v == 1 || (v > 7 && (binary[((v >> 3) - 1) as usize] & (1 << (v & 7))) != 0) {
                    set_module(
                        symbol,
                        h - (1 + y + 2 * (y / (fh - 2))) - 1,
                        1 + x + 2 * (x / (fw - 2)),
                    );
                }
            }
        }
        for y in 0..h as usize {
            symbol.row_height[y] = 1.0;
        }
    }

    symbol.height = h as f32;
    symbol.rows = h;
    symbol.width = w;

    error_number
}

/// Port of `datamatrix()`.
pub(crate) fn datamatrix(symbol: &mut ZintSymbol, segs: &mut [ZintSeg]) -> i32 {
    if symbol.option_1 <= 1 {
        // ECC 200
        let seg = segs[0].clone();
        return dm_ecc200(symbol, &seg);
    }
    // ECC 000 - 140
    errtxt(
        ZINT_ERROR_INVALID_OPTION,
        symbol,
        524,
        "Older Data Matrix standards are no longer supported",
    )
}
