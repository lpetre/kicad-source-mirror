//! Port of `plot_vector( symbol, 0, OUT_BUFFER )` of `thirdparty/zint/backend/vector.c` for the
//! rectangle-plotted symbologies KiCad uses, with the helpers of `output.c` it calls
//! (`out_large_bar_height` with `si` 0, `out_set_whitespace_offsets`, `out_quiet_zones`).
//!
//! With KiCad's settings (no BARCODE_QUIET_ZONES / BOX / BIND / DOTTY_MODE, `show_hrt` 0, no
//! whitespace or border, rotation 0) the plot is: one rectangle per horizontal run of dark
//! modules, row by row, `vector_reduce_rectangles()` merging vertically aligned ones, then
//! `vector_scale()` by `2 * scale`. The text, UPC/EAN, MaxiCode (hexagons / circles), Ultracode,
//! dotty, bind / box and rotation branches are not reached and not ported.

use crate::common::{ZINT_ERROR_INVALID_OPTION, errtxt, module_is_set, stripf};
use crate::library::{
    BARCODE_CODE39, BARCODE_CODE128, BARCODE_DATAMATRIX, BARCODE_MICROQR, BARCODE_QRCODE,
    ZintSymbol,
};

/// `BARCODE_QUIET_ZONES` (zint.h).
const BARCODE_QUIET_ZONES: i32 = 0x01000;
/// `BARCODE_NO_QUIET_ZONES` (zint.h).
const BARCODE_NO_QUIET_ZONES: i32 = 0x02000;
/// `BARCODE_BOX` (zint.h).
const BARCODE_BOX: i32 = 0x0004;
/// `BARCODE_BIND` (zint.h).
const BARCODE_BIND: i32 = 0x0002;
/// `BARCODE_BIND_TOP` (zint.h).
const BARCODE_BIND_TOP: i32 = 0x00080;

/// One `struct zint_vector_rect` (zint `float`s).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VectorRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// -1 for the default (foreground) colour.
    pub colour: i32,
}

/// One `struct zint_vector_hexagon` (MaxiCode only: never produced by the ported symbologies).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VectorHexagon {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub rotation: i32,
}

/// `struct zint_vector`: the vector rendering, lists in zint's order.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Vector {
    pub width: f32,
    pub height: f32,
    pub rectangles: Vec<VectorRect>,
    pub hexagons: Vec<VectorHexagon>,
}

/// Port of `out_quiet_zones()` for the ported symbologies: they only have quiet zones with
/// BARCODE_QUIET_ZONES set. Returns (left, right, top, bottom).
fn out_quiet_zones(symbol: &ZintSymbol) -> (f32, f32, f32, f32) {
    // None of the ported symbologies is in the "always have quiet zones" list.
    // Only do others if flag set
    if (symbol.output_options & BARCODE_QUIET_ZONES) == 0
        || (symbol.output_options & BARCODE_NO_QUIET_ZONES) != 0
    {
        return (0.0, 0.0, 0.0, 0.0);
    }

    match symbol.symbology {
        // ISO/IEC 16388:2007 Section 4.4 (d)
        BARCODE_CODE39 => (10.0, 10.0, 0.0, 0.0),
        // ISO/IEC 15417:2007 4.4.2
        BARCODE_CODE128 => (10.0, 10.0, 0.0, 0.0),
        // ISO/IEC 18004:2015 Section 9.1
        BARCODE_QRCODE => (4.0, 4.0, 4.0, 4.0),
        // ISO/IEC 18004:2015 Section 9.1
        BARCODE_MICROQR => (2.0, 2.0, 2.0, 2.0),
        // ISO/IEC 16022:2006 Section 7.1
        BARCODE_DATAMATRIX => (1.0, 1.0, 1.0, 1.0),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

/// Port of `out_set_whitespace_offsets()` (scaler 0): left (x), top (y), right and bottom
/// offsets for whitespace.
fn out_set_whitespace_offsets(symbol: &ZintSymbol) -> (f32, f32, f32, f32) {
    let (qz_left, qz_right, qz_top, qz_bottom) = out_quiet_zones(symbol);

    let mut xoffset = symbol.whitespace_width as f32 + qz_left;
    let mut roffset = symbol.whitespace_width as f32 + qz_right;
    if symbol.output_options & BARCODE_BOX != 0 {
        xoffset += symbol.border_width as f32;
        roffset += symbol.border_width as f32;
    }

    let mut yoffset = symbol.whitespace_height as f32 + qz_top;
    let mut boffset = symbol.whitespace_height as f32 + qz_bottom;
    if symbol.output_options & (BARCODE_BOX | BARCODE_BIND | BARCODE_BIND_TOP) != 0 {
        yoffset += symbol.border_width as f32;
        if symbol.output_options & BARCODE_BIND_TOP == 0 {
            // Trumps BARCODE_BOX & BARCODE_BIND
            boffset += symbol.border_width as f32;
        }
    }

    (xoffset, yoffset, roffset, boffset)
}

/// Port of `out_large_bar_height()` with `si` 0: the height of the rows with no fixed height.
fn out_large_bar_height(symbol: &mut ZintSymbol) -> f32 {
    let mut fixed_height = 0.0f32;
    let mut zero_count = 0;
    let mut large_bar_height = 0.0f32; // Not used if zero_count zero

    for i in 0..symbol.rows as usize {
        if symbol.row_height[i] != 0.0 {
            fixed_height += symbol.row_height[i];
        } else {
            zero_count += 1;
        }
    }
    if zero_count != 0 {
        large_bar_height = stripf((symbol.height - fixed_height) / zero_count as f32);
        debug_assert!(large_bar_height >= 0.5); // Min row height as set by `set_height()`
        symbol.height = stripf(large_bar_height * zero_count as f32 + fixed_height);
    }

    large_bar_height
}

/// Port of `vector_reduce_rectangles()`: looks for vertically aligned rectangles and merges
/// them together.
fn vector_reduce_rectangles(rects: &mut Vec<VectorRect>) {
    // The C list is singly linked; `next[k]` is the index of the rectangle after `k`.
    const NONE: usize = usize::MAX;
    let n = rects.len();
    let mut next: Vec<usize> = (1..=n).map(|k| if k < n { k } else { NONE }).collect();
    let mut head = if n > 0 { 0 } else { NONE };

    let mut rect = head;
    while rect != NONE {
        let mut prev = rect;
        let mut target = next[prev];

        while target != NONE {
            let (rc, tg) = (rects[rect], rects[target]);
            if rc.x == tg.x
                && rc.width == tg.width
                && stripf(rc.y + rc.height) == tg.y
                && rc.colour == tg.colour
            {
                rects[rect].height += tg.height;
                next[prev] = next[target];
            } else {
                prev = target;
            }
            target = next[prev];
        }

        rect = next[rect];
    }

    let mut out = Vec::with_capacity(n);
    while head != NONE {
        out.push(rects[head]);
        head = next[head];
    }
    *rects = out;
}

/// Port of `vector_scale()` (not EMF).
fn vector_scale(symbol: &ZintSymbol, vector: &mut Vector) {
    let mut scale = symbol.scale * 2.0;

    if scale < 0.2 {
        // Minimum vector scale 0.1
        scale = 0.2;
    }

    vector.width = stripf(vector.width * scale);
    vector.height = stripf(vector.height * scale);

    for rect in &mut vector.rectangles {
        rect.x = stripf(rect.x * scale);
        rect.y = stripf(rect.y * scale);
        rect.height = stripf(rect.height * scale);
        rect.width = stripf(rect.width * scale);
    }

    for hex in &mut vector.hexagons {
        hex.x = stripf(hex.x * scale);
        hex.y = stripf(hex.y * scale);
        hex.diameter = stripf(hex.diameter * scale);
    }
}

/// Port of `plot_vector( symbol, 0, OUT_BUFFER )` for the ported symbologies.
pub(crate) fn plot_vector(symbol: &mut ZintSymbol) -> i32 {
    // Free any previous rendering structures
    symbol.vector = None;

    // Sanity check colours: the default "000000" / "ffffff" pass `out_check_colour_options()`.
    if symbol.rows <= 0 {
        return errtxt(ZINT_ERROR_INVALID_OPTION, symbol, 697, "No rows");
    }

    let mut vector = Vector::default();

    let large_bar_height = out_large_bar_height(symbol);

    // Not composite, not UPC/EAN (upceanflag 0)
    let hide_text = symbol.show_hrt == 0 || symbol.text_length == 0;

    let (xoffset, yoffset, roffset, boffset) = out_set_whitespace_offsets(symbol);

    // Not BARCODE_DOTTY_MODE
    let dot_overspill = 0.0f32;

    vector.width = symbol.width as f32 + dot_overspill + (xoffset + roffset);

    // Note font sizes scaled by 2 so really twice these values
    let font_height = 7.0f32; // Not SMALL_TEXT
    let antialias_fudge_factor = 0.02f32;
    let antialias_fudge = font_height * antialias_fudge_factor;
    let text_gap = symbol.text_gap;
    let guard_descent = 0.0f32;

    let textoffset = if hide_text {
        guard_descent
    } else {
        font_height + stripf(text_gap + antialias_fudge)
    };

    vector.height = symbol.height + textoffset + dot_overspill + (yoffset + boffset);

    // Plot rectangles - most symbols created here
    let mut yposn = yoffset;
    for r in 0..symbol.rows {
        let row_height = if symbol.row_height[r as usize] != 0.0 {
            symbol.row_height[r as usize]
        } else {
            large_bar_height
        };

        let mut i = 0;
        while i < symbol.width {
            let fill = module_is_set(symbol, r, i);
            let mut block_width = 1;
            while i + block_width < symbol.width && module_is_set(symbol, r, i + block_width) == fill
            {
                block_width += 1;
            }
            if fill {
                // a bar
                vector.rectangles.push(VectorRect {
                    x: i as f32 + xoffset,
                    y: yposn,
                    width: block_width as f32,
                    height: row_height,
                    colour: -1, // Default colour
                });
            }
            i += block_width;
        }
        yposn += row_height;
    }

    // The text is not added: `hide_text` (show_hrt 0).

    vector_reduce_rectangles(&mut vector.rectangles);

    vector_scale(symbol, &mut vector);

    // `vector_rotate( symbol, 0 )`: no rotation needed.

    symbol.vector = Some(vector);

    0
}
