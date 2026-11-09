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

//! Port of the pure helpers of `common/gr_text.cpp` / `include/gr_text.h` (pen sizes, pen size
//! clamping, `GRTextWidth`, the knockout margin) and of the text size limits of
//! `include/eda_text.h`.  `GRPrintText` (wxDC printing) is out of scope.

use kicad_geom::{Vector2I, ki_round};

use crate::font::{Font, FontExt};
use crate::font_metrics::Metrics;

/// `TEXT_MIN_SIZE_MM`: minimum text size (1 micron).  These are only here for algorithmic
/// safety, not to tell the user what to do.
pub const TEXT_MIN_SIZE_MM: f64 = 0.001;

/// `TEXT_MAX_SIZE_MM`: maximum text size in mm (~10 inches).
pub const TEXT_MAX_SIZE_MM: f64 = 250.0;

/// `DEFAULT_SIZE_TEXT`: default text height (in mils, i.e. 1/1000").
pub const DEFAULT_SIZE_TEXT: i32 = 50;

/// Port of `BOLD_STROKE_MULTIPLIER`.
pub const BOLD_STROKE_MULTIPLIER: f64 = 1.6;

/// Port of `GetPenSizeForBold( int aTextSize )`.
pub fn get_pen_size_for_bold(a_text_size: i32) -> i32 {
    ki_round(a_text_size as f64 / 5.0)
}

/// Port of `GetPenSizeForBold( const wxSize& aTextSize )`.
pub fn get_pen_size_for_bold_size(a_text_size: Vector2I) -> i32 {
    get_pen_size_for_bold(std::cmp::min(a_text_size.x, a_text_size.y))
}

/// Port of `GetPenSizeForDemiBold( int aTextSize )`.
pub fn get_pen_size_for_demi_bold(a_text_size: i32) -> i32 {
    ki_round(a_text_size as f64 / 6.0)
}

/// Port of `GetPenSizeForDemiBold( const wxSize& aTextSize )`.
pub fn get_pen_size_for_demi_bold_size(a_text_size: Vector2I) -> i32 {
    get_pen_size_for_demi_bold(std::cmp::min(a_text_size.x, a_text_size.y))
}

/// Port of `GetPenSizeForNormal( int aTextSize )`.
pub fn get_pen_size_for_normal(a_text_size: i32) -> i32 {
    ki_round(a_text_size as f64 / 8.0)
}

/// Port of `GetPenSizeForNormal( const wxSize& aTextSize )`.
pub fn get_pen_size_for_normal_size(a_text_size: Vector2I) -> i32 {
    get_pen_size_for_normal(std::cmp::min(a_text_size.x, a_text_size.y))
}

/// Port of `ClampTextPenSize( int aPenSize, int aSize, bool aStrict )`: limit the pen size to
/// 25 % (18 % if strict) of the text size.
pub fn clamp_text_pen_size(a_pen_size: i32, a_size: i32, a_strict: bool) -> i32 {
    let scale: f64 = if a_strict { 0.18 } else { 0.25 };
    let max_width: i32 = ki_round(a_size as f64 * scale);

    std::cmp::min(a_pen_size, max_width)
}

/// Port of `ClampTextPenSize( float aPenSize, int aSize, bool aStrict )`.
pub fn clamp_text_pen_size_f32(a_pen_size: f32, a_size: i32, a_strict: bool) -> f32 {
    let scale: f64 = if a_strict { 0.18 } else { 0.25 };
    let max_width: f32 = (a_size as f32 as f64 * scale) as f32;

    // std::min( a, b ) returns a unless b < a
    if max_width < a_pen_size {
        max_width
    } else {
        a_pen_size
    }
}

/// Port of `ClampTextPenSize( int aPenSize, const VECTOR2I& aSize, bool aStrict )`: clamp to
/// the smaller of the absolute width and height.
pub fn clamp_text_pen_size_vec(a_pen_size: i32, a_size: Vector2I, a_strict: bool) -> i32 {
    let size = std::cmp::min(a_size.x.wrapping_abs(), a_size.y.wrapping_abs());

    clamp_text_pen_size(a_pen_size, size, a_strict)
}

/// Port of `GetKnockoutTextMargin`: the margin around knockout text.
pub fn get_knockout_text_margin(a_size: Vector2I, a_thickness: i32) -> i32 {
    std::cmp::max(
        ki_round(a_thickness as f64 / 2.0),
        ki_round(a_size.y as f64 / 9.0),
    )
}

/// Port of `GRTextWidth`: the width of a single line of text, `None` font meaning the default
/// (stroke) font.
///
/// The C++ evaluates `@{...}` expressions first (`EXPRESSION_EVALUATOR`); text_eval is not
/// ported, so such text is measured unevaluated (see `PORT_STATUS.md`).
pub fn gr_text_width(
    a_text: &str,
    a_font: Option<&dyn Font>,
    a_size: Vector2I,
    a_thickness: i32,
    a_bold: bool,
    a_italic: bool,
    a_font_metrics: &Metrics,
) -> i32 {
    let font: &dyn Font = match a_font {
        Some(f) => f,
        None => crate::font::default_font(),
    };

    // KiROUND( int ) is the identity
    font.string_boundary_limits(
        a_text,
        a_size,
        a_thickness,
        a_bold,
        a_italic,
        a_font_metrics,
    )
    .x
}
