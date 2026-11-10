//! Port of `qrcode()` (BARCODE_QRCODE) and `microqr()` (BARCODE_MICROQR) of
//! `thirdparty/zint/backend/qr.c` with their helpers: the mode optimisation
//! (`qr_define_mode`, adapted from Project Nayuki), binary length estimation, bit stream,
//! Reed-Solomon blocks, grid setup / population, mask evaluation and format / version
//! information.
//!
//! Not reached with KiCad's settings (one segment, UNICODE_MODE, no GS1, no structured append,
//! `option_2` / `option_3` 0, no FAST_MODE) and therefore not ported: rMQR and UPNQR, the GS1
//! percent / FNC1 handling (`gs1` is always 0), structured append, user-selected masks and
//! versions beyond the range checks zint performs.
//!
//! `qr_define_mode()` contains code under the MIT licence:
//!
//! Copyright (c) Project Nayuki. (MIT License)
//! <https://www.nayuki.io/page/qr-code-generator-library>
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of
//! this software and associated documentation files (the "Software"), to deal in
//! the Software without restriction, including without limitation the rights to
//! use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software is furnished to do so,
//! subject to the following conditions:
//! - The above copyright notice and this permission notice shall be included in
//!   all copies or substantial portions of the Software.

use crate::common::{
    IS_AST_F, IS_CLI_F, IS_MNS_F, IS_NUM_F, IS_PLS_F, IS_SIL_F, IS_SPC_F, IS_UPR_F,
    ZINT_ERROR_INVALID_DATA, ZINT_ERROR_INVALID_OPTION, ZINT_ERROR_TOO_LONG,
    ZINT_WARN_NONCOMPLIANT, bin_append, ctoi, errtxt, is_chr, not_sane, set_module, z_isdigit,
};
use crate::eci::{get_eci_length, sjis_cpy, sjis_utf8, sjis_utf8_to_eci};
use crate::library::{DATA_MODE, FAST_MODE, NOT_PORTED, ZintSeg, ZintSymbol};
use crate::qr_tables::{
    MICROQR_DATA, MICROQR_SIZES, QR_ALIGN_LOOPSIZE, QR_ALPHANUMERIC, QR_ANNEX_C, QR_ANNEX_C1,
    QR_ANNEX_D, QR_BLOCKS, QR_DATA_CODEWORDS, QR_SIZES, QR_TABLE_E1, QR_TOTAL_CODEWORDS,
};
use crate::reedsol::Rs;

/// `QR_ALPHA`: the Alphanumeric mode character set.
const QR_ALPHA: u16 =
    IS_NUM_F | IS_UPR_F | IS_SPC_F | IS_AST_F | IS_PLS_F | IS_MNS_F | IS_SIL_F | IS_CLI_F;

const QR_LEVEL_L: usize = 0;
const QR_LEVEL_M: usize = 1;
const QR_LEVEL_Q: usize = 2;
const QR_LEVEL_H: usize = 3;

/// `qr_ecc_level_names`.
const QR_ECC_LEVEL_NAMES: [char; 4] = ['L', 'M', 'Q', 'H'];

/// `RMQR_VERSION`.
const RMQR_VERSION: i32 = 41;
/// `MICROQR_VERSION`.
const MICROQR_VERSION: i32 = 73;

/// `ZINT_FULL_MULTIBYTE`.
const ZINT_FULL_MULTIBYTE: i32 = 200;

/// `NEON_F` (common.h): digits only.
const NEON_F: u16 = IS_NUM_F;

/// Port of `qr_is_alpha()` (gs1 0): whether the glyph is in the Alphanumeric set.
fn qr_is_alpha(glyph: u32) -> bool {
    is_chr(QR_ALPHA, glyph)
}

/// `QR_MULT`: bits multiplied by this for costs, so as to be whole integer divisible by 2 and 3.
const QR_MULT: u32 = 6;

/// Port of `qr_in_numeric()`: whether in numeric or not. If in numeric, `*p_end` is set to the
/// position after numeric, and `*p_cost` to the per-numeric cost.
fn qr_in_numeric(ddata: &[u32], in_posn: usize, p_end: &mut u32, p_cost: &mut u32) -> bool {
    let length = ddata.len();

    if (in_posn as i32) < *p_end as i32 {
        return true;
    }

    // Attempt to calculate the average 'cost' of using numeric mode in number of bits
    // (times QR_MULT)
    let mut i = in_posn;
    while i < length && i < in_posn + 3 && z_isdigit(ddata[i]) {
        i += 1;
    }

    let digit_cnt = i - in_posn;

    if digit_cnt == 0 {
        *p_end = 0;
        return false;
    }
    *p_end = i as u32;
    *p_cost = if digit_cnt == 1 {
        24 // 4 * QR_MULT
    } else if digit_cnt == 2 {
        21 // (7 / 2) * QR_MULT
    } else {
        20 // (10 / 3) * QR_MULT
    };
    true
}

/// Port of `qr_in_alpha()` (gs1 0): whether in alpha or not. If in alpha, `*p_end` is set to
/// the position after alpha, and `*p_cost` to the per-alpha cost.
fn qr_in_alpha(ddata: &[u32], in_posn: usize, p_end: &mut u32, p_cost: &mut u32) -> bool {
    let length = ddata.len();
    let last = in_posn + 1 == length;

    // Attempt to calculate the average 'cost' of using alphanumeric mode in number of bits
    // (times QR_MULT)
    if (in_posn as i32) < *p_end as i32 {
        *p_cost = if !last { 33 } else { 36 }; // (11 / 2) * QR_MULT : 6 * QR_MULT
        return true;
    }

    if !qr_is_alpha(ddata[in_posn]) {
        *p_end = 0;
        return false;
    }

    let two_alphas = !last && qr_is_alpha(ddata[in_posn + 1]);

    *p_end = if two_alphas { in_posn + 2 } else { in_posn + 1 } as u32;
    *p_cost = if two_alphas { 33 } else { 36 }; // (11 / 2) * QR_MULT : 6 * QR_MULT
    true
}

// Indexes into qr_mode_types array (and state array)
const QR_N: usize = 0; // Numeric
const QR_A: usize = 1; // Alphanumeric
const QR_B: usize = 2; // Byte
const QR_K: usize = 3; // Kanji

/// `qr_mode_types`: must be in same order as QR_N etc.
const QR_MODE_TYPES: [u8; 4] = [b'N', b'A', b'B', b'K'];

const QR_NUM_MODES: usize = 4;

/// Costs set to this for invalid MICROQR modes for versions M1 and M2.
/// 128 is the max number of data bits for M4-L (ISO/IEC 18004:2015 Table 7).
const QR_MICROQR_MAX: u32 = 774; // (128 + 1) * QR_MULT

/// Port of `qr_head_costs()`: initial mode costs (QR Code and Micro QR only).
fn qr_head_costs(version: i32) -> [u32; QR_NUM_MODES] {
    const HEAD_COSTS: [[u32; QR_NUM_MODES]; 7] = [
        // N A B K
        [(10 + 4) * QR_MULT, (9 + 4) * QR_MULT, (8 + 4) * QR_MULT, (8 + 4) * QR_MULT], // QR
        [(12 + 4) * QR_MULT, (11 + 4) * QR_MULT, (16 + 4) * QR_MULT, (10 + 4) * QR_MULT],
        [(14 + 4) * QR_MULT, (13 + 4) * QR_MULT, (16 + 4) * QR_MULT, (12 + 4) * QR_MULT],
        [3 * QR_MULT, QR_MICROQR_MAX, QR_MICROQR_MAX, QR_MICROQR_MAX], // MICROQR
        [(4 + 1) * QR_MULT, (3 + 1) * QR_MULT, QR_MICROQR_MAX, QR_MICROQR_MAX],
        [(5 + 2) * QR_MULT, (4 + 2) * QR_MULT, (4 + 2) * QR_MULT, (3 + 2) * QR_MULT],
        [(6 + 3) * QR_MULT, (5 + 3) * QR_MULT, (5 + 3) * QR_MULT, (4 + 3) * QR_MULT],
    ];

    if version < RMQR_VERSION {
        // QRCODE
        if version < 10 {
            HEAD_COSTS[0]
        } else if version < 27 {
            HEAD_COSTS[1]
        } else {
            HEAD_COSTS[2]
        }
    } else {
        debug_assert!(version >= MICROQR_VERSION, "rMQR not ported");
        HEAD_COSTS[3 + (version - MICROQR_VERSION) as usize]
    }
}

/// Port of `qr_define_mode()` (gs1 0): calculates optimized encoding modes. Adapted from
/// Project Nayuki.
fn qr_define_mode(mode: &mut [u8], ddata: &[u32], version: i32) {
    let length = ddata.len();
    // state[QR_N_END], state[QR_N_COST], state[QR_A_END], state[QR_A_COST]
    let mut n_end = 0u32;
    let mut n_cost = 0u32;
    let mut a_end = 0u32;
    let mut a_cost = 0u32;
    let mut char_modes: Vec<[u8; QR_NUM_MODES]> = vec![[0; QR_NUM_MODES]; length];

    // Head costs kept in states 0..3
    let state = qr_head_costs(version);

    // At the beginning of each iteration of the loop below, prev_costs[j] is the minimum number
    // of 1/6 (1/QR_MULT) bits needed to encode the entire string prefix of length i, and end in
    // qr_mode_types[j]
    let mut prev_costs = state;

    // Calculate costs using dynamic programming
    for i in 0..length {
        let mut cur_costs = [0u32; QR_NUM_MODES];

        let m1 = version == MICROQR_VERSION;
        let m2 = version == MICROQR_VERSION + 1;

        if ddata[i] > 0xFF {
            cur_costs[QR_B] = prev_costs[QR_B] + if m1 || m2 { QR_MICROQR_MAX } else { 96 }; // 16 * QR_MULT
            char_modes[i][QR_B] = b'B';
            cur_costs[QR_K] = prev_costs[QR_K] + if m1 || m2 { QR_MICROQR_MAX } else { 78 }; // 13 * QR_MULT
            char_modes[i][QR_K] = b'K';
        } else {
            if qr_in_numeric(ddata, i, &mut n_end, &mut n_cost) {
                cur_costs[QR_N] = prev_costs[QR_N] + n_cost;
                char_modes[i][QR_N] = b'N';
            }
            if qr_in_alpha(ddata, i, &mut a_end, &mut a_cost) {
                cur_costs[QR_A] = prev_costs[QR_A] + if m1 { QR_MICROQR_MAX } else { a_cost };
                char_modes[i][QR_A] = b'A';
            }
            cur_costs[QR_B] = prev_costs[QR_B] + if m1 || m2 { QR_MICROQR_MAX } else { 48 }; // 8 * QR_MULT
            char_modes[i][QR_B] = b'B';
        }

        // Start new segment at the end to switch modes
        for j in 0..QR_NUM_MODES {
            // To mode
            for k in 0..QR_NUM_MODES {
                // From mode
                if j != k && char_modes[i][k] != 0 {
                    let new_cost = cur_costs[k] + state[j]; // Switch costs same as head costs
                    if char_modes[i][j] == 0 || new_cost < cur_costs[j] {
                        cur_costs[j] = new_cost;
                        char_modes[i][j] = QR_MODE_TYPES[k];
                    }
                }
            }
        }

        prev_costs = cur_costs;
    }

    // Find optimal ending mode
    let mut min_cost = prev_costs[0];
    let mut cur_mode = QR_MODE_TYPES[0];
    for i in 1..QR_NUM_MODES {
        if prev_costs[i] < min_cost {
            min_cost = prev_costs[i];
            cur_mode = QR_MODE_TYPES[i];
        }
    }

    // Get optimal mode for each code point by tracing backwards
    for i in (0..length).rev() {
        let j = mode_index(cur_mode);
        cur_mode = char_modes[i][j];
        mode[i] = cur_mode;
    }
}

/// `posn(qr_mode_types, mode)`.
fn mode_index(mode: u8) -> usize {
    QR_MODE_TYPES
        .iter()
        .position(|&m| m == mode)
        .expect("valid QR mode")
}

/// Port of `qr_mode_indicator()`: mode indicator based on version and mode.
fn qr_mode_indicator(version: i32, mode: u8) -> i32 {
    const MODE_INDICATORS: [[i32; QR_NUM_MODES]; 6] = [
        // N  A  B  K
        [1, 2, 4, 8], // QRCODE
        [1, 2, 3, 4], // RMQR
        [0, 0, 0, 0], // MICROQR
        [0, 1, 0, 0],
        [0, 1, 2, 3],
        [0, 1, 2, 3],
    ];

    let mode_index = mode_index(mode);

    if version < RMQR_VERSION {
        return MODE_INDICATORS[0][mode_index]; // QRCODE
    }
    if version < MICROQR_VERSION {
        return MODE_INDICATORS[1][mode_index]; // RMQR
    }
    MODE_INDICATORS[(2 + version - MICROQR_VERSION) as usize][mode_index] // MICROQR
}

/// Port of `qr_mode_bits()`: mode indicator bits based on version.
fn qr_mode_bits(version: i32) -> i32 {
    if version < RMQR_VERSION {
        return 4; // QRCODE
    }
    if version < MICROQR_VERSION {
        return 3; // RMQR
    }
    version - MICROQR_VERSION // MICROQR
}

/// Port of `qr_cci_bits()`: character count indicator bits based on version and mode.
fn qr_cci_bits(version: i32, mode: u8) -> i32 {
    const CCI_BITS: [[i32; QR_NUM_MODES]; 7] = [
        // N   A   B   K
        [10, 9, 8, 8], // QRCODE
        [12, 11, 16, 10],
        [14, 13, 16, 12],
        [3, 0, 0, 0], // MICROQR
        [4, 3, 0, 0],
        [5, 4, 4, 3],
        [6, 5, 5, 4],
    ];
    let mode_index = mode_index(mode);

    if version < RMQR_VERSION {
        // QRCODE
        if version < 10 {
            return CCI_BITS[0][mode_index];
        }
        if version < 27 {
            return CCI_BITS[1][mode_index];
        }
        return CCI_BITS[2][mode_index];
    }
    debug_assert!(version >= MICROQR_VERSION, "rMQR not ported");
    CCI_BITS[(3 + (version - MICROQR_VERSION)) as usize][mode_index] // MICROQR
}

/// Port of `qr_terminator_bits()`: terminator bits based on version.
fn qr_terminator_bits(version: i32) -> i32 {
    if version < RMQR_VERSION {
        return 4; // QRCODE
    }
    if version < MICROQR_VERSION {
        return 3; // RMQR
    }
    3 + (version - MICROQR_VERSION) * 2 // MICROQR
}

/// Port of `qr_binary()` (gs1 0): converts input data to a binary stream ('0' / '1' bytes
/// appended to `binary`).
fn qr_binary(binary: &mut Vec<u8>, version: i32, mode: &[u8], ddata: &[u32], eci: i32) {
    let length = ddata.len();
    let mut position = 0usize;

    if eci != 0 {
        // Not applicable to MICROQR
        bin_append(7, if version < RMQR_VERSION { 4 } else { 3 }, binary); // ECI (Table 4)
        if eci <= 127 {
            bin_append(eci, 8, binary); // 000000 to 000127
        } else if eci <= 16383 {
            bin_append(0x8000 + eci, 16, binary); // 000128 to 016383
        } else {
            bin_append(0xC00000 + eci, 24, binary); // 016384 to 999999
        }
    }

    let modebits = qr_mode_bits(version);

    loop {
        let data_block = mode[position];
        let mut short_data_block_length = 0usize;
        let mut double_byte = 0usize;
        loop {
            if data_block == b'B' && ddata[position + short_data_block_length] > 0xFF {
                double_byte += 1;
            }
            short_data_block_length += 1;
            if !((short_data_block_length + position) < length
                && mode[position + short_data_block_length] == data_block)
            {
                break;
            }
        }

        // Mode indicator
        if modebits != 0 {
            bin_append(qr_mode_indicator(version, data_block), modebits, binary);
        }

        match data_block {
            b'K' => {
                // Kanji mode
                // Character count indicator
                bin_append(short_data_block_length as i32, qr_cci_bits(version, data_block), binary);

                // Character representation
                for i in 0..short_data_block_length {
                    let mut jis = ddata[position + i];

                    if (0x8140..=0x9ffc).contains(&jis) {
                        jis -= 0x8140;
                    } else if (0xe040..=0xebbf).contains(&jis) {
                        jis -= 0xc140;
                    }

                    let prod = ((jis >> 8) * 0xc0) + (jis & 0xff);

                    bin_append(prod as i32, 13, binary);
                }
            }
            b'B' => {
                // Byte mode
                // Character count indicator
                bin_append(
                    (short_data_block_length + double_byte) as i32,
                    qr_cci_bits(version, data_block),
                    binary,
                );

                // Character representation
                for i in 0..short_data_block_length {
                    let byte = ddata[position + i];

                    bin_append(byte as i32, if byte > 0xFF { 16 } else { 8 }, binary);
                }
            }
            b'A' => {
                // Alphanumeric mode (gs1 0: no percent doubling, no FNC1)
                // Character count indicator
                bin_append(short_data_block_length as i32, qr_cci_bits(version, data_block), binary);

                // Character representation
                let mut i = 0;
                while i < short_data_block_length {
                    let first = i32::from(QR_ALPHANUMERIC[(ddata[position + i] - 32) as usize]);
                    let mut count = 1;
                    i += 1;
                    let mut prod = first;

                    if i < short_data_block_length && mode[position + i] == b'A' {
                        let second = i32::from(QR_ALPHANUMERIC[(ddata[position + i] - 32) as usize]);
                        count = 2;
                        i += 1;
                        prod = (first * 45) + second;
                    }

                    bin_append(prod, 1 + (5 * count), binary);
                }
            }
            b'N' => {
                // Numeric mode
                // Character count indicator
                bin_append(short_data_block_length as i32, qr_cci_bits(version, data_block), binary);

                // Character representation
                let mut i = 0;
                while i < short_data_block_length {
                    let first = ctoi(ddata[position + i] as u8);
                    let mut count = 1;
                    let mut prod = first;

                    if i + 1 < short_data_block_length && mode[position + i + 1] == b'N' {
                        let second = ctoi(ddata[position + i + 1] as u8);
                        count = 2;
                        prod = (prod * 10) + second;

                        if i + 2 < short_data_block_length && mode[position + i + 2] == b'N' {
                            let third = ctoi(ddata[position + i + 2] as u8);
                            count = 3;
                            prod = (prod * 10) + third;
                        }
                    }

                    bin_append(prod, 1 + (3 * count), binary);

                    i += count as usize;
                }
            }
            _ => {}
        }

        position += short_data_block_length;
        if position >= length {
            break;
        }
    }
}

/// Port of `qr_binary_segs()` (one segment, no structured append, no gs1): the bit stream with
/// terminator and padding packed into `target_codewords` codewords (QR Code), or the bare bit
/// stream (Micro QR, which does its own terminating / padding).
fn qr_binary_segs(
    version: i32,
    target_codewords: usize,
    mode: &[u8],
    ddata: &[u32],
    eci: i32,
) -> (Vec<u8>, Vec<u8>) {
    let mut binary: Vec<u8> = Vec::new();

    qr_binary(&mut binary, version, mode, ddata, eci);

    if version >= MICROQR_VERSION && version < MICROQR_VERSION + 4 {
        // MICROQR does its own terminating/padding
        return (Vec::new(), binary);
    }

    let mut bp = binary.len() as i32;
    // Terminator
    let mut termbits = 8 - bp % 8;
    if termbits == 8 {
        termbits = 0;
    }
    let mut current_bytes = (bp + termbits) / 8;
    if termbits != 0 || current_bytes < target_codewords as i32 {
        let max_termbits = qr_terminator_bits(version);
        termbits = if termbits < max_termbits && current_bytes == target_codewords as i32 {
            termbits
        } else {
            max_termbits
        };
        bin_append(0, termbits, &mut binary);
        bp = binary.len() as i32;
    }

    // Padding bits
    let mut padbits = 8 - bp % 8;
    if padbits == 8 {
        padbits = 0;
    }
    if padbits != 0 {
        current_bytes = (bp + padbits) / 8;
        bin_append(0, padbits, &mut binary); // Last use so not setting bp
    }

    // Put data into 8-bit codewords
    let mut datastream = vec![0u8; target_codewords];
    for i in 0..current_bytes as usize {
        let j = i * 8;
        datastream[i] = 0x00;
        for p in 0..8 {
            if binary[j + p] == b'1' {
                datastream[i] |= 0x80 >> p;
            }
        }
    }

    // Add pad codewords
    let mut toggle = false;
    for i in current_bytes as usize..target_codewords {
        if !toggle {
            datastream[i] = 0xec;
            toggle = true;
        } else {
            datastream[i] = 0x11;
            toggle = false;
        }
    }

    (datastream, binary)
}

/// Port of `qr_add_ecc()`: splits data into blocks, adds error correction and then interleaves
/// the blocks and error correction data.
fn qr_add_ecc(datastream: &[u8], version: i32, data_cw: usize, blocks: usize) -> Vec<u8> {
    let ecc_cw = QR_TOTAL_CODEWORDS[(version - 1) as usize] as usize - data_cw;

    debug_assert!(blocks > 0);

    let short_data_block_length = data_cw / blocks;
    let qty_long_blocks = data_cw % blocks;
    let qty_short_blocks = blocks - qty_long_blocks;
    let ecc_block_length = ecc_cw / blocks;

    let mut data_block = vec![0u8; short_data_block_length + 1];
    let mut ecc_block = vec![0u8; ecc_block_length];
    let mut interleaved_data = vec![0u8; data_cw];
    let mut interleaved_ecc = vec![0u8; ecc_cw];

    let mut rs = Rs::init_gf(0x11d);
    rs.init_code(ecc_block_length, 0);

    let mut in_posn = 0;

    for i in 0..blocks {
        let length_this_block = if i < qty_short_blocks {
            short_data_block_length
        } else {
            short_data_block_length + 1
        };

        data_block[..length_this_block]
            .copy_from_slice(&datastream[in_posn..in_posn + length_this_block]);

        rs.encode(&data_block[..length_this_block], &mut ecc_block);

        for j in 0..short_data_block_length {
            interleaved_data[(j * blocks) + i] = data_block[j];
        }

        if i >= qty_short_blocks {
            interleaved_data[(short_data_block_length * blocks) + (i - qty_short_blocks)] =
                data_block[short_data_block_length];
        }

        for j in 0..ecc_block_length {
            interleaved_ecc[(j * blocks) + i] = ecc_block[j];
        }

        in_posn += length_this_block;
    }

    let mut fullstream = Vec::with_capacity(data_cw + ecc_cw + 1);
    fullstream.extend_from_slice(&interleaved_data);
    fullstream.extend_from_slice(&interleaved_ecc);
    fullstream
}

/// Port of `qr_place_finder()`.
fn qr_place_finder(grid: &mut [u8], size: usize, x: usize, y: usize) {
    const FINDER: [u8; 7] = [0x7F, 0x41, 0x5D, 0x5D, 0x5D, 0x41, 0x7F];

    for xp in 0..7 {
        for yp in 0..7 {
            grid[((yp + y) * size) + (xp + x)] =
                if FINDER[yp] & (0x40 >> xp) != 0 { 0x11 } else { 0x10 };
        }
    }
}

/// Port of `qr_place_align()`: input values represent centre of pattern.
fn qr_place_align(grid: &mut [u8], size: usize, x: usize, y: usize) {
    const ALIGNMENT: [u8; 5] = [0x1F, 0x11, 0x15, 0x11, 0x1F];

    let x = x - 2;
    let y = y - 2; // Input values represent centre of pattern

    for xp in 0..5 {
        for yp in 0..5 {
            grid[((yp + y) * size) + (xp + x)] =
                if ALIGNMENT[yp] & (0x10 >> xp) != 0 { 0x11 } else { 0x10 };
        }
    }
}

/// Port of `qr_setup_grid()`.
fn qr_setup_grid(grid: &mut [u8], size: usize, version: i32) {
    let mut toggle = true;

    // Add timing patterns
    for i in 0..size {
        if toggle {
            grid[(6 * size) + i] = 0x21;
            grid[(i * size) + 6] = 0x21;
            toggle = false;
        } else {
            grid[(6 * size) + i] = 0x20;
            grid[(i * size) + 6] = 0x20;
            toggle = true;
        }
    }

    // Add finder patterns
    qr_place_finder(grid, size, 0, 0);
    qr_place_finder(grid, size, 0, size - 7);
    qr_place_finder(grid, size, size - 7, 0);

    // Add separators
    for i in 0..7 {
        grid[(7 * size) + i] = 0x10;
        grid[(i * size) + 7] = 0x10;
        grid[(7 * size) + (size - 1 - i)] = 0x10;
        grid[(i * size) + (size - 8)] = 0x10;
        grid[((size - 8) * size) + i] = 0x10;
        grid[((size - 1 - i) * size) + 7] = 0x10;
    }
    grid[(7 * size) + 7] = 0x10;
    grid[(7 * size) + (size - 8)] = 0x10;
    grid[((size - 8) * size) + 7] = 0x10;

    // Add alignment patterns
    if version != 1 {
        // Version 1 does not have alignment patterns
        let loopsize = QR_ALIGN_LOOPSIZE[(version - 1) as usize] as usize;
        for x in 0..loopsize {
            for y in 0..loopsize {
                let xcoord = QR_TABLE_E1[((version - 2) * 7) as usize + x] as usize;
                let ycoord = QR_TABLE_E1[((version - 2) * 7) as usize + y] as usize;

                if grid[(ycoord * size) + xcoord] & 0x10 == 0 {
                    qr_place_align(grid, size, xcoord, ycoord);
                }
            }
        }
    }

    // Reserve space for format information
    for i in 0..8 {
        grid[(8 * size) + i] |= 0x20;
        grid[(i * size) + 8] |= 0x20;
        grid[(8 * size) + (size - 1 - i)] = 0x20;
        grid[((size - 1 - i) * size) + 8] = 0x20;
    }
    grid[(8 * size) + 8] |= 0x20;
    grid[((size - 1 - 7) * size) + 8] = 0x21; // Dark Module from Figure 25

    // Reserve space for version information
    if version >= 7 {
        for i in 0..6 {
            grid[((size - 9) * size) + i] = 0x20;
            grid[((size - 10) * size) + i] = 0x20;
            grid[((size - 11) * size) + i] = 0x20;
            grid[(i * size) + (size - 9)] = 0x20;
            grid[(i * size) + (size - 10)] = 0x20;
            grid[(i * size) + (size - 11)] = 0x20;
        }
    }
}

/// Port of `qr_cwbit()`.
fn qr_cwbit(fullstream: &[u8], i: usize) -> u8 {
    u8::from(fullstream[i >> 3] & (0x80 >> (i & 0x07)) != 0)
}

/// Port of `qr_populate_grid()` for square (non-rMQR) symbols.
fn qr_populate_grid(grid: &mut [u8], size: usize, fullstream: &[u8], cw: usize) {
    let h_size = size as i32;
    let v_size = size as i32;
    let x_start = h_size - 2;
    let mut direction = true; // up
    let mut row = 0; // right hand side

    let n = cw * 8;
    let mut y = v_size - 1;
    let mut i = 0;
    while i < n {
        let mut x = x_start - (row * 2);
        let r = (y * h_size) as usize;

        if x < 6 {
            x -= 1; // skip over vertical timing pattern
        }

        if grid[r + (x + 1) as usize] & 0xf0 == 0 {
            grid[r + (x + 1) as usize] = qr_cwbit(fullstream, i);
            i += 1;
        }

        if i < n && grid[r + x as usize] & 0xf0 == 0 {
            grid[r + x as usize] = qr_cwbit(fullstream, i);
            i += 1;
        }

        if direction {
            y -= 1;
            if y == -1 {
                // reached the top
                row += 1;
                y = 0;
                direction = false;
            }
        } else {
            y += 1;
            if y == v_size {
                // reached the bottom
                row += 1;
                y = v_size - 1;
                direction = true;
            }
        }
    }
}

/// Port of `qr_evaluate()`: the mask penalty score.
fn qr_evaluate(local: &[u8], size: usize) -> i32 {
    const H1011101: [u8; 7] = [1, 0, 1, 1, 1, 0, 1];
    let mut result: i32 = 0;

    debug_assert!(size > 0);

    // Test 1: Adjacent modules in row/column in same colour
    // Vertical
    for x in 0..size {
        let mut block = 0;
        let mut state = 0u8;
        for y in 0..size {
            if local[(y * size) + x] == state {
                block += 1;
            } else {
                if block >= 5 {
                    result += block - 2;
                }
                block = 1;
                state = local[(y * size) + x];
            }
        }
        if block >= 5 {
            result += block - 2;
        }
    }

    // Horizontal
    let mut dark_mods: i32 = 0; // Count dark mods simultaneously (see Test 4 below)
    for y in 0..size {
        let r = y * size;
        let mut block = 0;
        let mut state = 0u8;
        for x in 0..size {
            if local[r + x] == state {
                block += 1;
            } else {
                if block >= 5 {
                    result += block - 2;
                }
                block = 1;
                state = local[r + x];
            }
            if state != 0 {
                dark_mods += 1;
            }
        }
        if block >= 5 {
            result += block - 2;
        }
    }

    // Test 2: Block of modules in same color
    for x in 0..size - 1 {
        for y in 0..size - 1 {
            let k = local[(y * size) + x];
            if k == local[((y + 1) * size) + x]
                && k == local[(y * size) + (x + 1)]
                && k == local[((y + 1) * size) + (x + 1)]
            {
                result += 3;
            }
        }
    }

    // Test 3: 1:1:3:1:1 ratio pattern in row/column
    let isize = size as i32;
    let at = |y: i32, x: i32| local[(y * isize + x) as usize];
    // Vertical
    for x in 0..isize {
        let mut y = 0;
        while y <= isize - 7 {
            if at(y, x) != 0
                && at(y + 1, x) == 0
                && at(y + 2, x) != 0
                && at(y + 3, x) != 0
                && at(y + 4, x) != 0
                && at(y + 5, x) == 0
                && at(y + 6, x) != 0
            {
                // Pattern found, check before and after
                let mut before_count = 0;
                let mut b = y - 1;
                while b >= y - 4 {
                    if b < 0 {
                        // Count < edge as whitespace
                        before_count = 4;
                        break;
                    }
                    if at(b, x) != 0 {
                        break;
                    }
                    before_count += 1;
                    b -= 1;
                }
                if before_count == 4 {
                    // Pattern is preceded by light area 4 modules wide
                    result += 40;
                } else {
                    let mut after_count = 0;
                    let mut a = y + 7;
                    while a <= y + 10 {
                        if a >= isize {
                            // Count > edge as whitespace
                            after_count = 4;
                            break;
                        }
                        if at(a, x) != 0 {
                            break;
                        }
                        after_count += 1;
                        a += 1;
                    }
                    if after_count == 4 {
                        // Pattern is followed by light area 4 modules wide
                        result += 40;
                    }
                }
                y += 3; // Skip to next possible match
            }
            y += 1;
        }
    }

    // Horizontal
    for y in 0..isize {
        let r = (y * isize) as usize;
        let mut x = 0;
        while x <= isize - 7 {
            if local[r + x as usize..r + x as usize + 7] == H1011101 {
                // Pattern found, check before and after
                let mut before_count = 0;
                let mut b = x - 1;
                while b >= x - 4 {
                    if b < 0 {
                        // Count < edge as whitespace
                        before_count = 4;
                        break;
                    }
                    if local[r + b as usize] != 0 {
                        break;
                    }
                    before_count += 1;
                    b -= 1;
                }

                if before_count == 4 {
                    // Pattern is preceded by light area 4 modules wide
                    result += 40;
                } else {
                    let mut after_count = 0;
                    let mut a = x + 7;
                    while a <= x + 10 {
                        if a >= isize {
                            // Count > edge as whitespace
                            after_count = 4;
                            break;
                        }
                        if local[r + a as usize] != 0 {
                            break;
                        }
                        after_count += 1;
                        a += 1;
                    }
                    if after_count == 4 {
                        // Pattern is followed by light area 4 modules wide
                        result += 40;
                    }
                }
                x += 3; // Skip to next possible match
            }
            x += 1;
        }
    }

    // Test 4: Proportion of dark modules in entire symbol
    let percentage = (100.0 * f64::from(dark_mods)) / f64::from(isize * isize);
    let k = ((percentage - 50.0).abs() / 5.0) as i32;

    result += 10 * k;

    result
}

/// Port of `qr_add_format_info()`: adds format information to grid.
fn qr_add_format_info(grid: &mut [u8], size: usize, ecc_level: usize, pattern: usize) {
    let mut format = pattern;

    match ecc_level {
        QR_LEVEL_L => format |= 0x08,
        QR_LEVEL_Q => format |= 0x18,
        QR_LEVEL_H => format |= 0x10,
        _ => {}
    }

    let seq = u32::from(QR_ANNEX_C[format]);

    for i in 0..6 {
        grid[(i * size) + 8] |= ((seq >> i) & 0x01) as u8;
    }

    for i in 0..8 {
        grid[(8 * size) + (size - i - 1)] |= ((seq >> i) & 0x01) as u8;
    }

    for i in 0..6 {
        grid[(8 * size) + (5 - i)] |= ((seq >> (i + 9)) & 0x01) as u8;
    }

    for i in 0..7 {
        grid[(((size - 7) + i) * size) + 8] |= ((seq >> (i + 8)) & 0x01) as u8;
    }

    grid[(7 * size) + 8] |= ((seq >> 6) & 0x01) as u8;
    grid[(8 * size) + 8] |= ((seq >> 7) & 0x01) as u8;
    grid[(8 * size) + 7] |= ((seq >> 8) & 0x01) as u8;
}

/// Port of `qr_apply_bitmask()`: masks the data, choosing the pattern with the lowest penalty
/// unless `user_mask` is given. Returns the pattern.
fn qr_apply_bitmask(
    grid: &mut [u8],
    size: usize,
    ecc_level: usize,
    user_mask: i32,
    fast_encode: bool,
) -> usize {
    let size_squared = size * size;
    let mut mask = vec![0u8; size_squared];
    let mut local = vec![0u8; size_squared];
    let mut penalty = [0i32; 8];
    let best_pattern: usize;

    // Perform data masking
    for y in 0..size {
        let r = y * size;
        for x in 0..size {
            // all eight bitmask variants are encoded in the 8 bits of the bytes that make up the
            // mask array.
            if grid[r + x] & 0xf0 == 0 {
                // exclude areas not to be masked.
                if ((y + x) & 1) == 0 {
                    mask[r + x] |= 0x01;
                }
                if !fast_encode && (y & 1) == 0 {
                    mask[r + x] |= 0x02;
                }
                if (x % 3) == 0 {
                    mask[r + x] |= 0x04;
                }
                if !fast_encode && ((y + x) % 3) == 0 {
                    mask[r + x] |= 0x08;
                }
                if (((y / 2) + (x / 3)) & 1) == 0 {
                    mask[r + x] |= 0x10;
                }
                if !fast_encode {
                    if (y * x) % 6 == 0 {
                        // Equivalent to (y * x) % 2 + (y * x) % 3 == 0
                        mask[r + x] |= 0x20;
                    }
                    if ((((y * x) & 1) + ((y * x) % 3)) & 1) == 0 {
                        mask[r + x] |= 0x40;
                    }
                }
                if ((((y + x) & 1) + ((y * x) % 3)) & 1) == 0 {
                    mask[r + x] |= 0x80;
                }
            }
        }
    }

    if user_mask != 0 {
        best_pattern = (user_mask - 1) as usize;
    } else {
        // all eight bitmask variants have been encoded in the 8 bits of the bytes
        // that make up the mask array. select them for evaluation according to the
        // desired pattern.
        let mut best = 0usize;
        for pattern in 0..8 {
            if fast_encode && pattern != 0 && pattern != 2 && pattern != 4 && pattern != 7 {
                continue;
            }
            let bit = 1u8 << pattern;
            for k in 0..size_squared {
                if mask[k] & bit != 0 {
                    local[k] = grid[k] ^ 0x01;
                } else {
                    local[k] = grid[k] & 0x0f;
                }
            }
            qr_add_format_info(&mut local, size, ecc_level, pattern);

            penalty[pattern] = qr_evaluate(&local, size);

            if penalty[pattern] < penalty[best] {
                best = pattern;
            }
        }
        best_pattern = best;
    }

    // Apply mask
    if user_mask == 0 && best_pattern == 7 {
        // Reuse last
        grid.copy_from_slice(&local);
    } else {
        let bit = 1u8 << best_pattern;
        for y in 0..size_squared {
            if mask[y] & bit != 0 {
                grid[y] ^= 0x01;
            }
        }
    }

    best_pattern
}

/// Port of `qr_add_version_info()`.
fn qr_add_version_info(grid: &mut [u8], size: usize, version: i32) {
    let version_data = QR_ANNEX_D[(version - 7) as usize];
    for i in 0..6 {
        grid[((size - 11) * size) + i] |= ((version_data >> (i * 3)) & 1) as u8;
        grid[((size - 10) * size) + i] |= ((version_data >> ((i * 3) + 1)) & 1) as u8;
        grid[((size - 9) * size) + i] |= ((version_data >> ((i * 3) + 2)) & 1) as u8;
        grid[(i * size) + (size - 11)] |= ((version_data >> (i * 3)) & 1) as u8;
        grid[(i * size) + (size - 10)] |= ((version_data >> ((i * 3) + 1)) & 1) as u8;
        grid[(i * size) + (size - 9)] |= ((version_data >> ((i * 3) + 2)) & 1) as u8;
    }
}

/// Port of `qr_blockLength()`: the length of the block starting from `start`.
fn qr_block_length(start: usize, mode: &[u8], length: usize) -> usize {
    let mut count = 0;
    let start_mode = mode[start];

    loop {
        count += 1;
        if !((start + count) < length && mode[start + count] == start_mode) {
            break;
        }
    }

    count
}

/// Port of `qr_calc_binlen()` (mode_preset 0, gs1 0): defines the modes for `version` and
/// returns the actual bit length of the proposed binary string.
fn qr_calc_binlen(version: i32, mode: &mut [u8], ddata: &[u32], eci: i32) -> i32 {
    let length = ddata.len();
    let mut count: i32 = 0;

    qr_define_mode(mode, ddata, version);

    let mut current_mode = b' '; // Null

    if eci != 0 {
        // Not applicable to MICROQR
        count += 4;
        if eci <= 127 {
            count += 8;
        } else if eci <= 16383 {
            count += 16;
        } else {
            count += 24;
        }
    }

    for i in 0..length {
        if mode[i] != current_mode {
            count += qr_mode_bits(version) + qr_cci_bits(version, mode[i]);
            let blocklength = qr_block_length(i, mode, length) as i32;
            match mode[i] {
                b'K' => count += blocklength * 13,
                b'B' => {
                    for j in i..i + blocklength as usize {
                        if ddata[j] > 0xff {
                            count += 16;
                        } else {
                            count += 8;
                        }
                    }
                }
                b'A' => {
                    let alphalength = blocklength;
                    match alphalength % 2 {
                        0 => count += (alphalength / 2) * 11,
                        _ => {
                            count += ((alphalength - 1) / 2) * 11;
                            count += 6;
                        }
                    }
                }
                b'N' => match blocklength % 3 {
                    0 => count += (blocklength / 3) * 10,
                    1 => {
                        count += ((blocklength - 1) / 3) * 10;
                        count += 4;
                    }
                    _ => {
                        count += ((blocklength - 2) / 3) * 10;
                        count += 7;
                    }
                },
                _ => {}
            }
            current_mode = mode[i];
        }
    }

    count
}

/// Port of `qr_prep_data()` (one segment): processes the source data into code values, trying
/// the segment's ECI (0 = ISO/IEC 8859-1) first and Shift JIS second.
fn qr_prep_data(symbol: &mut ZintSymbol, seg: &ZintSeg, ddata: &mut Vec<u32>) -> i32 {
    let mut warn_number = 0;
    // If ZINT_FULL_MULTIBYTE use Kanji mode in DATA_MODE or for non-Shift JIS in UNICODE_MODE
    let full_multibyte = (symbol.option_3 & 0xFF) == ZINT_FULL_MULTIBYTE;

    if (symbol.input_mode & 0x07) == DATA_MODE {
        sjis_cpy(&seg.source, ddata, full_multibyte);
    } else {
        let mut done = false;
        if seg.eci != 20 {
            // Unless ECI 20 (Shift JIS) or have multiple segments
            // Try other encodings (ECI 0 defaults to ISO/IEC 8859-1)
            let error_number = match sjis_utf8_to_eci(seg.eci, &seg.source, ddata, full_multibyte) {
                Ok(e) => e,
                Err(_) => return NOT_PORTED,
            };
            if error_number == 0 {
                done = true;
            } else if seg.eci != 0 {
                let msg = format!("Invalid character in input for ECI '{}'", seg.eci);
                return errtxt(error_number, symbol, 575, &msg);
            }
        }
        if !done {
            // Try Shift-JIS
            let error_number = sjis_utf8(symbol, &seg.source, ddata);
            if error_number != 0 {
                return error_number;
            }
            if seg.eci != 20 {
                warn_number = errtxt(
                    ZINT_WARN_NONCOMPLIANT,
                    symbol,
                    760,
                    "Converted to Shift JIS but no ECI specified",
                );
            }
        }
    }

    warn_number
}

/// Port of `qrcode()` (BARCODE_QRCODE, one segment).
pub(crate) fn qrcode(symbol: &mut ZintSymbol, segs: &mut [ZintSeg]) -> i32 {
    let fast_encode = symbol.input_mode & FAST_MODE != 0;
    let seg = segs[0].clone();
    let eci_length_segs = get_eci_length(seg.eci, &seg.source);
    let mut ddata: Vec<u32> = Vec::with_capacity(eci_length_segs);

    let mut user_mask = (symbol.option_3 >> 8) & 0x0F; // User mask is pattern + 1, so >= 1 and <= 8
    if user_mask > 8 {
        user_mask = 0; // Ignore
    }

    let warn_number = qr_prep_data(symbol, &seg, &mut ddata);
    if warn_number == NOT_PORTED || warn_number >= crate::common::ZINT_ERROR {
        return warn_number;
    }
    let length = ddata.len();
    let eci = seg.eci;

    // Structured Append: `symbol->structapp.count` is 0.

    let mut mode = vec![0u8; length];
    let mut est_binlen = qr_calc_binlen(40, &mut mode, &ddata, eci);

    let mut ecc_level = if symbol.option_1 >= 1 && symbol.option_1 <= 4 {
        (symbol.option_1 - 1) as usize
    } else {
        QR_LEVEL_L
    };
    let max_cw = i32::from(QR_DATA_CODEWORDS[ecc_level][39]);

    if est_binlen > (8 * max_cw) {
        if ecc_level == QR_LEVEL_L {
            let msg = format!(
                "Input too long, requires {} codewords (maximum {max_cw})",
                (est_binlen + 7) / 8
            );
            return errtxt(ZINT_ERROR_TOO_LONG, symbol, 567, &msg);
        }
        let msg = format!(
            "Input too long for ECC level {}, requires {} codewords (maximum {max_cw})",
            QR_ECC_LEVEL_NAMES[ecc_level],
            (est_binlen + 7) / 8
        );
        return errtxt(ZINT_ERROR_TOO_LONG, symbol, 561, &msg);
    }

    let mut autosize: i32 = 40;
    for i in (0..40).rev() {
        if (8 * i32::from(QR_DATA_CODEWORDS[ecc_level][i])) >= est_binlen {
            autosize = i as i32 + 1;
        }
    }
    if autosize != 40 {
        // Save version 40 estimate in case incorrect costings in `qr_define_mode()` lead to its
        // `mode` being better than current lower version one
        let prev_est_binlen = est_binlen;
        est_binlen = qr_calc_binlen(autosize, &mut mode, &ddata, eci);
        if prev_est_binlen < est_binlen {
            // Shouldn't happen
            // Defensively use version 40 `mode` to avoid crashes (ticket #300)
            est_binlen = qr_calc_binlen(40, &mut mode, &ddata, eci);
            debug_assert_eq!(est_binlen, prev_est_binlen);
        }
    }

    // Now see if the optimised binary will fit in a smaller symbol.
    let mut can_shrink = true;

    loop {
        if autosize == 1 {
            can_shrink = false;
        } else {
            let prev_est_binlen = est_binlen;
            let prev_mode = mode.clone();
            est_binlen = qr_calc_binlen(autosize - 1, &mut mode, &ddata, eci);

            if (8 * i32::from(QR_DATA_CODEWORDS[ecc_level][(autosize - 2) as usize])) < est_binlen {
                can_shrink = false;
            }

            if can_shrink {
                // Optimisation worked - data will fit in a smaller symbol
                autosize -= 1;
            } else {
                // Data did not fit in the smaller symbol, revert to original size
                est_binlen = prev_est_binlen;
                mode = prev_mode;
            }
        }
        if !can_shrink {
            break;
        }
    }

    let mut version = autosize;

    if symbol.option_2 >= 1 && symbol.option_2 <= 40 {
        // If the user has selected a larger symbol than the smallest available,
        // then use the size the user has selected, and re-optimise for this
        // symbol size.
        if symbol.option_2 > version {
            version = symbol.option_2;
            est_binlen = qr_calc_binlen(symbol.option_2, &mut mode, &ddata, eci);
        }

        if symbol.option_2 < version {
            let msg = format!(
                "Input too long for Version {}-{}, requires {} codewords (maximum {})",
                symbol.option_2,
                QR_ECC_LEVEL_NAMES[ecc_level],
                (est_binlen + 7) / 8,
                QR_DATA_CODEWORDS[ecc_level][(symbol.option_2 - 1) as usize]
            );
            return errtxt(ZINT_ERROR_TOO_LONG, symbol, 569, &msg);
        }
    }

    // Ensure maxium error correction capacity unless user-specified
    if symbol.option_1 == -1 || symbol.option_1 - 1 != ecc_level as i32 {
        let v = (version - 1) as usize;
        if est_binlen <= i32::from(QR_DATA_CODEWORDS[QR_LEVEL_H][v]) * 8 {
            ecc_level = QR_LEVEL_H;
        } else if est_binlen <= i32::from(QR_DATA_CODEWORDS[QR_LEVEL_Q][v]) * 8 {
            ecc_level = QR_LEVEL_Q;
        } else if est_binlen <= i32::from(QR_DATA_CODEWORDS[QR_LEVEL_M][v]) * 8 {
            ecc_level = QR_LEVEL_M;
        }
    }

    let v = (version - 1) as usize;
    let target_codewords = QR_DATA_CODEWORDS[ecc_level][v] as usize;
    let blocks = QR_BLOCKS[ecc_level][v] as usize;

    let (datastream, _) = qr_binary_segs(version, target_codewords, &mode, &ddata, eci);
    let fullstream = qr_add_ecc(&datastream, version, target_codewords, blocks);

    let size = QR_SIZES[v] as usize;
    let size_squared = size * size;

    let mut grid = vec![0u8; size_squared];

    qr_setup_grid(&mut grid, size, version);
    qr_populate_grid(&mut grid, size, &fullstream, QR_TOTAL_CODEWORDS[v] as usize);

    if version >= 7 {
        qr_add_version_info(&mut grid, size, version);
    }

    let bitmask = qr_apply_bitmask(&mut grid, size, ecc_level, user_mask, fast_encode);

    // Feedback options
    symbol.option_1 = ecc_level as i32 + 1;
    symbol.option_2 = version;
    symbol.option_3 = (symbol.option_3 & 0xFF) | ((bitmask as i32 + 1) << 8);

    qr_add_format_info(&mut grid, size, ecc_level, bitmask);

    symbol.width = size as i32;
    symbol.rows = size as i32;

    for i in 0..size {
        let r = i * size;
        for j in 0..size {
            if grid[r + j] & 0x01 != 0 {
                set_module(symbol, i as i32, j as i32);
            }
        }
        symbol.row_height[i] = 1.0;
    }
    symbol.height = size as f32;

    warn_number
}

/// Port of `microqr_end()`: writes terminator, padding & ECC.
fn microqr_end(binary_data: &mut Vec<u8>, ecc_level: usize, version: usize) {
    let terminator_bits = qr_terminator_bits(MICROQR_VERSION + version as i32);
    let bits_total = i32::from(MICROQR_DATA[ecc_level][version][0]);
    let data_codewords = MICROQR_DATA[ecc_level][version][1] as usize;
    let ecc_codewords = MICROQR_DATA[ecc_level][version][2] as usize;
    let bits_end = if version == 0 || version == 2 { 4 } else { 8 };
    let bp = |b: &Vec<u8>| b.len() as i32;

    // Add terminator
    let mut bits_left = bits_total - bp(binary_data);
    if bits_left <= terminator_bits {
        if bits_left != 0 {
            bin_append(0, bits_left, binary_data);
            bits_left = 0;
        }
    } else {
        bin_append(0, terminator_bits, binary_data);
        bits_left -= terminator_bits;
    }

    // Manage last (4-bit) block
    if bits_end == 4 && bits_left != 0 && bits_left <= 4 {
        bin_append(0, bits_left, binary_data);
        bits_left = 0;
    }

    if bits_left != 0 {
        // Complete current byte
        let remainder = 8 - (bp(binary_data) % 8);
        if remainder != 8 {
            bin_append(0, remainder, binary_data);
            bits_left -= remainder;
        }

        // Add padding
        if bits_end == 4 && bits_left > 4 {
            bits_left -= 4;
        }
        let remainder = bits_left / 8;
        for i in 0..remainder {
            bin_append(if i & 1 != 0 { 0x11 } else { 0xEC }, 8, binary_data);
        }
        if bits_end == 4 {
            bin_append(0, 4, binary_data);
        }
    }
    debug_assert_eq!(bp(binary_data) & 0x07, 8 - bits_end);

    // Copy data into codewords
    let mut data_blocks = [0u8; 17];
    for i in 0..data_codewords {
        let bits = if i + 1 == data_codewords { bits_end } else { 8 };
        data_blocks[i] = 0;

        for j in 0..bits as usize {
            if binary_data[(i * 8) + j] == b'1' {
                data_blocks[i] |= 0x80 >> j;
            }
        }
    }

    // Calculate Reed-Solomon error codewords
    let mut ecc_blocks = [0u8; 15];
    let mut rs = Rs::init_gf(0x11d);
    rs.init_code(ecc_codewords, 0);
    rs.encode(&data_blocks[..data_codewords], &mut ecc_blocks);

    // Add Reed-Solomon codewords to binary data
    for &e in ecc_blocks.iter().take(ecc_codewords) {
        bin_append(i32::from(e), 8, binary_data);
    }
}

/// Port of `microqr_setup_grid()`.
fn microqr_setup_grid(grid: &mut [u8], size: usize) {
    let mut toggle = true;

    // Add timing patterns
    for i in 0..size {
        if toggle {
            grid[i] = 0x21;
            grid[i * size] = 0x21;
            toggle = false;
        } else {
            grid[i] = 0x20;
            grid[i * size] = 0x20;
            toggle = true;
        }
    }

    // Add finder patterns
    qr_place_finder(grid, size, 0, 0);

    // Add separators
    for i in 0..7 {
        grid[(7 * size) + i] = 0x10;
        grid[(i * size) + 7] = 0x10;
    }
    grid[(7 * size) + 7] = 0x10;

    // Reserve space for format information
    for i in 0..8 {
        grid[(8 * size) + i] |= 0x20;
        grid[(i * size) + 8] |= 0x20;
    }
    grid[(8 * size) + 8] |= 20; // sic: decimal 20 (0x14) in zint
}

/// Port of `microqr_populate_grid()`.
fn microqr_populate_grid(grid: &mut [u8], size: usize, full_stream: &[u8], bp: usize) {
    let size_i = size as i32;
    let mut direction = true; // up
    let mut row = 0; // right hand side
    let mut y = size_i - 1;
    let mut i = 0;
    loop {
        let x = (size_i - 2) - (row * 2);
        let r = (y * size_i) as usize;

        if grid[r + (x + 1) as usize] & 0xf0 == 0 {
            grid[r + (x + 1) as usize] = u8::from(full_stream[i] == b'1');
            i += 1;
        }

        if i < bp && grid[r + x as usize] & 0xf0 == 0 {
            grid[r + x as usize] = u8::from(full_stream[i] == b'1');
            i += 1;
        }

        if direction {
            y -= 1;
        } else {
            y += 1;
        }
        if y == 0 {
            // reached the top
            row += 1;
            y = 1;
            direction = false;
        }
        if y == size_i {
            // reached the bottom
            row += 1;
            y = size_i - 1;
            direction = true;
        }
        if i >= bp {
            break;
        }
    }
}

/// Port of `microqr_evaluate()`.
fn microqr_evaluate(grid: &[u8], size: usize, pattern: usize) -> i32 {
    let filter: u8 = match pattern {
        0 => 0x01,
        1 => 0x02,
        2 => 0x04,
        _ => 0x08,
    };

    let mut sum1 = 0;
    let mut sum2 = 0;
    for i in 1..size {
        if grid[(i * size) + size - 1] & filter != 0 {
            sum1 += 1;
        }
        if grid[((size - 1) * size) + i] & filter != 0 {
            sum2 += 1;
        }
    }

    if sum1 <= sum2 { (sum1 * 16) + sum2 } else { (sum2 * 16) + sum1 }
}

/// Port of `microqr_apply_bitmask()`.
fn microqr_apply_bitmask(grid: &mut [u8], size: usize, user_mask: i32) -> usize {
    let size_squared = size * size;
    let mut mask = vec![0u8; size_squared];
    let mut eval = vec![0u8; size_squared];
    let best_pattern: usize;

    // Perform data masking
    for y in 0..size {
        let r = y * size;
        for x in 0..size {
            if grid[r + x] & 0xf0 == 0 {
                if (y & 1) == 0 {
                    mask[r + x] |= 0x01;
                }

                if (((y / 2) + (x / 3)) & 1) == 0 {
                    mask[r + x] |= 0x02;
                }

                if ((((y * x) & 1) + ((y * x) % 3)) & 1) == 0 {
                    mask[r + x] |= 0x04;
                }

                if ((((y + x) & 1) + ((y * x) % 3)) & 1) == 0 {
                    mask[r + x] |= 0x08;
                }
            }
        }
    }

    if user_mask != 0 {
        best_pattern = (user_mask - 1) as usize;
    } else {
        for k in 0..size_squared {
            if grid[k] & 0x01 != 0 {
                eval[k] = mask[k] ^ 0xff;
            } else {
                eval[k] = mask[k];
            }
        }

        // Evaluate result
        let mut value = [0i32; 4];
        let mut best = 0usize;
        for pattern in 0..4 {
            value[pattern] = microqr_evaluate(&eval, size, pattern);
            if value[pattern] > value[best] {
                best = pattern;
            }
        }
        best_pattern = best;
    }

    // Apply mask
    let bit = 1u8 << best_pattern;
    for k in 0..size_squared {
        if mask[k] & bit != 0 {
            grid[k] ^= 0x01;
        }
    }

    best_pattern
}

/// Port of `microqr()` (BARCODE_MICROQR).
pub(crate) fn microqr(symbol: &mut ZintSymbol, source: &[u8]) -> i32 {
    let length = source.len();
    let mut ddata: Vec<u32> = Vec::with_capacity(40);
    let mut alpha_used = false;
    let mut byte_or_kanji_used = false;
    let mut version_valid = [true; 4];
    let mut binary_count = [0i32; 4];

    if length > 35 {
        return errtxt(
            ZINT_ERROR_TOO_LONG,
            symbol,
            562,
            &format!("Input length {length} too long (maximum 35)"),
        );
    }

    // Check option 1 in combination with option 2
    let mut ecc_level = QR_LEVEL_L;
    if symbol.option_1 >= 1 && symbol.option_1 <= 4 {
        if symbol.option_1 == 4 {
            return errtxt(
                ZINT_ERROR_INVALID_OPTION,
                symbol,
                566,
                "Error correction level H not available",
            );
        }
        if symbol.option_2 >= 1 && symbol.option_2 <= 4 {
            if symbol.option_2 == 1 && symbol.option_1 != 1 {
                return errtxt(
                    ZINT_ERROR_INVALID_OPTION,
                    symbol,
                    574,
                    "Version M1 supports error correction level L only",
                );
            }
            if symbol.option_2 != 4 && symbol.option_1 == 3 {
                return errtxt(
                    ZINT_ERROR_INVALID_OPTION,
                    symbol,
                    563,
                    "Error correction level Q requires Version M4",
                );
            }
        }
        ecc_level = (symbol.option_1 - 1) as usize;
    }

    // If ZINT_FULL_MULTIBYTE use Kanji mode in DATA_MODE or for non-Shift JIS in UNICODE_MODE
    let full_multibyte = (symbol.option_3 & 0xFF) == ZINT_FULL_MULTIBYTE;
    let mut user_mask = (symbol.option_3 >> 8) & 0x0F; // User mask is pattern + 1, so >= 1 and <= 4
    if user_mask > 4 {
        user_mask = 0; // Ignore
    }

    if (symbol.input_mode & 0x07) == DATA_MODE {
        sjis_cpy(source, &mut ddata, full_multibyte);
    } else {
        // Try ISO 8859-1 conversion first
        let error_number = match sjis_utf8_to_eci(3, source, &mut ddata, full_multibyte) {
            Ok(e) => e,
            Err(_) => return NOT_PORTED,
        };
        if error_number != 0 {
            // Try Shift-JIS
            let error_number = sjis_utf8(symbol, source, &mut ddata);
            if error_number != 0 {
                return error_number;
            }
        }
    }
    let length = ddata.len();

    // Determine if alpha (excluding numerics), byte or kanji used
    let mut i = 0;
    while i < length && (!alpha_used || !byte_or_kanji_used) {
        if !z_isdigit(ddata[i]) {
            if qr_is_alpha(ddata[i]) {
                alpha_used = true;
            } else {
                byte_or_kanji_used = true;
            }
        }
        i += 1;
    }

    // Eliminate possible versions depending on type of content
    if byte_or_kanji_used {
        version_valid[0] = false;
        version_valid[1] = false;
    } else if alpha_used {
        version_valid[0] = false;
    }

    // Eliminate possible versions depending on error correction level specified
    if ecc_level == QR_LEVEL_Q {
        version_valid[0] = false;
        version_valid[1] = false;
        version_valid[2] = false;
    } else if ecc_level == QR_LEVEL_M {
        version_valid[0] = false;
    }

    // Determine length of binary data
    let mut mode = vec![0u8; length];
    for i in 0..4 {
        if version_valid[i] {
            binary_count[i] = qr_calc_binlen(MICROQR_VERSION + i as i32, &mut mode, &ddata, 0);
        } else {
            binary_count[i] = 128 + 1;
        }
    }

    // Eliminate possible versions depending on binary length and error correction level
    // specified
    if binary_count[3] > i32::from(MICROQR_DATA[ecc_level][3][0]) {
        let msg = format!(
            "Input too long for Version M4-{}, requires {} codewords (maximum {})",
            QR_ECC_LEVEL_NAMES[ecc_level],
            (binary_count[3] + 7) / 8,
            MICROQR_DATA[ecc_level][3][1]
        );
        return errtxt(ZINT_ERROR_TOO_LONG, symbol, 565, &msg);
    }
    for i in 0..3 {
        if binary_count[i] > i32::from(MICROQR_DATA[ecc_level][i][0]) {
            version_valid[i] = false;
        }
    }

    // Auto-select lowest valid size
    let mut version = 3usize;
    if version_valid[2] {
        version = 2;
    }
    if version_valid[1] {
        version = 1;
    }
    if version_valid[0] {
        version = 0;
    }

    // Get version from user
    if symbol.option_2 >= 1 && symbol.option_2 <= 4 {
        if symbol.option_2 == 1 {
            let i = not_sane(NEON_F, &source[..length.min(source.len())]);
            if i != 0 {
                let msg = format!(
                    "Invalid character at position {i} in input for Version M1 (digits only)"
                );
                return errtxt(ZINT_ERROR_INVALID_DATA, symbol, 758, &msg);
            }
        } else if symbol.option_2 == 2 && not_sane(QR_ALPHA, &source[..length.min(source.len())]) != 0 {
            return errtxt(
                ZINT_ERROR_INVALID_DATA,
                symbol,
                759,
                "Invalid character in input for Version M2 (digits, A-Z, space and \"$%*+-./:\" only)",
            );
        }
        if (symbol.option_2 - 1) as usize >= version {
            version = (symbol.option_2 - 1) as usize;
        } else {
            let msg = format!(
                "Input too long for Version M{}-{}, requires {} codewords (maximum {})",
                symbol.option_2,
                QR_ECC_LEVEL_NAMES[ecc_level],
                (binary_count[version] + 7) / 8,
                MICROQR_DATA[ecc_level][(symbol.option_2 - 1) as usize][1]
            );
            return errtxt(ZINT_ERROR_TOO_LONG, symbol, 570, &msg);
        }
    }

    // If there is enough unused space then increase the error correction level, unless
    // user-specified
    if version != 0 && (symbol.option_1 == -1 || symbol.option_1 - 1 != ecc_level as i32) {
        if binary_count[version] <= i32::from(MICROQR_DATA[QR_LEVEL_Q][version][0]) {
            ecc_level = QR_LEVEL_Q;
        } else if binary_count[version] <= i32::from(MICROQR_DATA[QR_LEVEL_M][version][0]) {
            ecc_level = QR_LEVEL_M;
        }
    }

    qr_define_mode(&mut mode, &ddata, MICROQR_VERSION + version as i32);

    let (_, mut full_stream) = qr_binary_segs(MICROQR_VERSION + version as i32, 0, &mode, &ddata, 0);

    microqr_end(&mut full_stream, ecc_level, version);
    let bp = full_stream.len();

    let size = MICROQR_SIZES[version] as usize;
    let size_squared = size * size;

    let mut grid = vec![0u8; size_squared];

    microqr_setup_grid(&mut grid, size);
    microqr_populate_grid(&mut grid, size, &full_stream, bp);
    let bitmask = microqr_apply_bitmask(&mut grid, size, user_mask);

    // Feedback options
    symbol.option_1 = ecc_level as i32 + 1;
    symbol.option_2 = version as i32 + 1;
    symbol.option_3 = (symbol.option_3 & 0xFF) | ((bitmask as i32 + 1) << 8);

    // Add format data
    let format = if version != 0 { (version - 1) * 2 + ecc_level + 1 } else { 0 };

    let format_full = u32::from(QR_ANNEX_C1[(format << 2) + bitmask]);

    for i in 1..=8 {
        grid[(8 * size) + i] |= ((format_full >> (15 - i)) & 0x01) as u8;
    }
    for i in (1..=7).rev() {
        grid[(i * size) + 8] |= ((format_full >> (i - 1)) & 0x01) as u8;
    }

    symbol.width = size as i32;
    symbol.rows = size as i32;

    for i in 0..size {
        for j in 0..size {
            if grid[(i * size) + j] & 0x01 != 0 {
                set_module(symbol, i as i32, j as i32);
            }
        }
        symbol.row_height[i] = 1.0;
    }
    symbol.height = size as f32;

    0
}
