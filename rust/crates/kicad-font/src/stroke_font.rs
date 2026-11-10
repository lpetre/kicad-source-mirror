// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `common/font/stroke_font.cpp` and `include/font/stroke_font.h`: the built-in
//! Newstroke ("KiCad Font") stroke font.

use std::sync::OnceLock;

use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::{Box2D, Box2I, EdaAngle, Vector2D, Vector2I, ki_round};

use crate::font::{
    Font, KICAD_FONT_NAME, TextStyleFlags, is_subscript, is_superscript, text_style,
};
use crate::font_metrics::Metrics;
use crate::glyph::{Glyph, StrokeGlyph};
use crate::newstroke_font::{NEWSTROKE_FONT, NEWSTROKE_FONT_BUFSIZE};

/// Scale factor for a glyph.
const STROKE_FONT_SCALE: f64 = 1.0 / 21.0;

/// Offset (in stroke font units) to move the origin to the baseline.
const FONT_OFFSET: i32 = -8;

/// The decoded glyph table (`g_defaultFontGlyphs`, `g_defaultFontGlyphBoundingBoxes`).
#[derive(Debug)]
pub struct StrokeFontData {
    /// `g_defaultFontGlyphs`
    pub glyphs: Vec<StrokeGlyph>,
    /// `g_defaultFontGlyphBoundingBoxes`
    pub glyph_bounding_boxes: Vec<Box2D>,
    /// `m_maxGlyphWidth` of the font that performed the (only) load.
    pub max_glyph_width: f64,
}

/// `g_defaultFontInitialized` + the global tables, filled once (`g_defaultFontLoadMutex`).
static DEFAULT_FONT_DATA: OnceLock<StrokeFontData> = OnceLock::new();

/// Port of `buildGlyphBoundingBox`: x spans `[0, glyph width]`, y spans all the points (the
/// points may overhang the box in x).
fn build_glyph_bounding_box(a_glyph: &mut StrokeGlyph, a_glyph_width: f64) {
    let mut min = Vector2D::new(0.0, 0.0);
    let mut max = Vector2D::new(a_glyph_width, 0.0);

    for point_list in &a_glyph.strokes {
        for point in point_list {
            // std::min / std::max
            min.y = if point.y < min.y { point.y } else { min.y };
            max.y = if max.y < point.y { point.y } else { max.y };
        }
    }

    a_glyph.set_bounding_box(Box2D::new(min, max - min));
}

/// Port of the glyph decoding loop of `STROKE_FONT::loadNewStrokeFont` for one glyph string.
///
/// Returns the glyph (with its bounding box) and its width.
pub fn decode_glyph(a_glyph_string: &str) -> (StrokeGlyph, f64) {
    let bytes = a_glyph_string.as_bytes();
    let mut glyph = StrokeGlyph::new();

    let mut glyph_start_x: f64 = 0.0;
    let mut glyph_width: f64 = 0.0;

    // `char` is signed on x86; the data is ASCII so the values are the same.
    let at = |k: usize| -> i32 { bytes.get(k).map_or(0, |&b| b as i8 as i32) };

    let mut i = 0;

    while i < bytes.len() && bytes[i] != 0 {
        let coordinate = [at(i), at(i + 1)];

        if i < 2 {
            // The first two values contain the width of the char
            glyph_start_x = (coordinate[0] - 'R' as i32) as f64 * STROKE_FONT_SCALE;
            let glyph_end_x = (coordinate[1] - 'R' as i32) as f64 * STROKE_FONT_SCALE;
            glyph_width = glyph_end_x - glyph_start_x;
        } else if coordinate[0] == ' ' as i32 && coordinate[1] == 'R' as i32 {
            glyph.raise_pen();
        } else {
            // In stroke font, coordinates values are coded as <value> + 'R', where
            // <value> is an ASCII char.
            // therefore every coordinate description of the Hershey format has an offset,
            // it has to be subtracted
            // Note:
            //  * the stroke coordinates are stored in reduced form (-1.0 to +1.0),
            //    and the actual size is stroke coordinate * glyph size
            //  * a few shapes have a height slightly bigger than 1.0 ( like '{' '[' )
            let x = (coordinate[0] - 'R' as i32) as f64 * STROKE_FONT_SCALE - glyph_start_x;

            // FONT_OFFSET is here for historical reasons, due to the way the stroke font
            // was built. It allows shapes coordinates like W M ... to be >= 0
            // Only shapes like j y have coordinates < 0
            let y = (coordinate[1] - 'R' as i32 + FONT_OFFSET) as f64 * STROKE_FONT_SCALE;

            glyph.add_point(Vector2D::new(x, y));
        }

        i += 2;
    }

    glyph.finalize();

    // Compute the bounding box of the glyph
    build_glyph_bounding_box(&mut glyph, glyph_width);

    (glyph, glyph_width)
}

/// Port of `STROKE_FONT::loadNewStrokeFont` (the part guarded by `g_defaultFontInitialized`).
fn load_new_stroke_font(a_new_stroke_font: &[&str]) -> StrokeFontData {
    let mut glyphs = Vec::with_capacity(a_new_stroke_font.len());
    let mut boxes = Vec::with_capacity(a_new_stroke_font.len());
    let mut max_glyph_width: f64 = 0.0;

    for s in a_new_stroke_font {
        let (glyph, glyph_width) = decode_glyph(s);

        boxes.push(glyph.bounding_box());
        glyphs.push(glyph);
        // std::max( m_maxGlyphWidth, glyphWidth )
        max_glyph_width = if max_glyph_width < glyph_width {
            glyph_width
        } else {
            max_glyph_width
        };
    }

    StrokeFontData {
        glyphs,
        glyph_bounding_boxes: boxes,
        max_glyph_width,
    }
}

/// The process-wide decoded Newstroke table, decoded on first use.
pub fn default_font_data() -> &'static StrokeFontData {
    DEFAULT_FONT_DATA
        .get_or_init(|| load_new_stroke_font(&NEWSTROKE_FONT[..NEWSTROKE_FONT_BUFSIZE]))
}

/// Port of `KIFONT::STROKE_FONT`.
#[derive(Clone, Copy, Debug)]
pub struct StrokeFont {
    /// `m_glyphs` / `m_glyphBoundingBoxes`
    data: &'static StrokeFontData,
}

impl StrokeFont {
    /// `m_superSubSize`
    pub const SUPER_SUB_SIZE: f64 = 0.8;
    /// `m_subscriptBaselineOffset`
    pub const SUBSCRIPT_BASELINE_OFFSET: f64 = 0.15;
    /// `m_superscriptBaselineOffset`
    pub const SUPERSCRIPT_BASELINE_OFFSET: f64 = -0.35;

    /// Port of `STROKE_FONT::LoadFont`: the empty name gives the Newstroke font; any other name
    /// gives `None` ("if we ever supported other stroke fonts, the code would go here").
    pub fn load_font(a_font_name: &str) -> Option<StrokeFont> {
        if a_font_name.is_empty() {
            Some(StrokeFont {
                data: default_font_data(),
            })
        } else {
            None
        }
    }

    /// The decoded glyph table.
    pub fn data(&self) -> &'static StrokeFontData {
        self.data
    }

    /// Port of `STROKE_FONT::GetGlyphCount`.
    pub fn glyph_count(&self) -> u32 {
        self.data.glyphs.len() as u32
    }

    /// Port of `STROKE_FONT::GetGlyph`.
    pub fn glyph(&self, a_index: u32) -> Option<&'static StrokeGlyph> {
        self.data.glyphs.get(a_index as usize)
    }

    /// Port of `STROKE_FONT::GetGlyphBoundingBox`: an empty box for an invalid index.
    pub fn glyph_bounding_box(&self, a_index: u32) -> Box2D {
        self.data
            .glyph_bounding_boxes
            .get(a_index as usize)
            .copied()
            .unwrap_or_default()
    }
}

impl Font for StrokeFont {
    fn is_stroke(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        KICAD_FONT_NAME
    }

    /// Port of `STROKE_FONT::GetInterline`.
    fn interline(&self, a_glyph_height: f64, a_font_metrics: &Metrics) -> f64 {
        const LEGACY_FACTOR: f64 = 0.9583; // Adjustment to match legacy spacing

        a_font_metrics.interline(a_glyph_height) * LEGACY_FACTOR
    }

    fn super_sub_size_multiplier(&self) -> f64 {
        Self::SUPER_SUB_SIZE
    }

    fn super_sub_baseline_offset(&self, a_text_style: TextStyleFlags) -> f64 {
        if is_subscript(a_text_style) {
            Self::SUBSCRIPT_BASELINE_OFFSET
        } else if is_superscript(a_text_style) {
            Self::SUPERSCRIPT_BASELINE_OFFSET
        } else {
            0.0
        }
    }

    /// Port of `STROKE_FONT::GetTextAsGlyphs`: lay out one run of text (no markup, no line
    /// breaks) starting at `a_position`; returns the position after the run (on the baseline
    /// of `a_position`).
    fn text_as_glyphs(
        &self,
        a_bbox: Option<&mut Box2I>,
        mut a_glyphs: Option<&mut Vec<Glyph>>,
        a_text: &str,
        a_size: Vector2I,
        a_position: Vector2I,
        a_angle: EdaAngle,
        a_mirror: bool,
        a_origin: Vector2I,
        a_text_style: TextStyleFlags,
    ) -> Vector2I {
        const TAB_WIDTH: i32 = 4;
        const INTER_CHAR: f64 = 0.2;

        let boxes = &self.data.glyph_bounding_boxes;
        let mut cursor: Vector2I = a_position;
        let mut glyph_size: Vector2D = a_size.cast();
        let tilt: f64 = if a_text_style & text_style::ITALIC != 0 {
            crate::font::ITALIC_TILT
        } else {
            0.0
        };
        let space_width: f64 = boxes[0].width(); // First char is space
        let mut char_count: i32 = 0;

        if a_text_style & text_style::SUBSCRIPT != 0 || a_text_style & text_style::SUPERSCRIPT != 0
        {
            glyph_size *= self.super_sub_size_multiplier();
            // int += double: the sum is truncated
            cursor.y = f64_to_i32_x86(
                cursor.y as f64 + glyph_size.y * self.super_sub_baseline_offset(a_text_style),
            );
        }

        for ch in a_text.chars() {
            let mut c = ch as i32;

            // Handle tabs as locked to the next 4th column (in base-widths).
            if c == '\t' as i32 {
                char_count = (char_count / TAB_WIDTH + 1) * TAB_WIDTH - 1;

                // int + int * int (wrapping like the C++ on x86), then + double, truncated
                let int_part = a_position.x.wrapping_add(a_size.x.wrapping_mul(char_count));
                let mut new_cursor: i32 =
                    f64_to_i32_x86(int_part as f64 + a_size.x as f64 * space_width);

                // A zero-width tab never advances, so it cannot pass the cursor
                while a_size.x > 0 && new_cursor <= cursor.x {
                    char_count = char_count.wrapping_add(TAB_WIDTH);
                    new_cursor = new_cursor.wrapping_add(a_size.x.wrapping_mul(TAB_WIDTH));
                }

                cursor.x = new_cursor;
            } else if c == ' ' as i32 {
                // 'space' character - draw nothing, advance cursor position
                cursor.x = cursor.x.wrapping_add(ki_round(glyph_size.x * space_width));
            } else {
                // dd is the index into bounding boxes table
                let mut dd: i32 = c - ' ' as i32;

                // Filtering non existing glyphs and non printable chars
                if dd < 0 || dd >= boxes.len() as i32 {
                    c = '?' as i32;
                    dd = c - ' ' as i32;
                }

                let source: &StrokeGlyph = &self.data.glyphs[dd as usize];

                if let Some(glyphs) = a_glyphs.as_deref_mut() {
                    glyphs.push(Glyph::Stroke(
                        source.transform(glyph_size, cursor, tilt, a_angle, a_mirror, a_origin),
                    ));
                }

                let mut glyph_extents: Vector2D = source.bounding_box().end();

                glyph_extents.x *= glyph_size.x;
                glyph_extents.y *= glyph_size.y;

                cursor.x = cursor.x.wrapping_add(ki_round(glyph_extents.x));
            }

            char_count = char_count.wrapping_add(1);
        }

        if let Some(bbox) = a_bbox {
            bbox.set_origin(a_position);
            // SetEnd( coord_type, coord_type ): the double argument is truncated
            bbox.set_end_xy(
                cursor.x.wrapping_sub(ki_round(glyph_size.x * INTER_CHAR)),
                f64_to_i32_x86(cursor.y as f64 - glyph_size.y),
            );
            bbox.normalize();
        }

        Vector2I::new(cursor.x, a_position.y)
    }
}
