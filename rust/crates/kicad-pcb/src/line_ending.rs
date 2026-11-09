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

//! Port of `include/line_ending.h` and the data part of `common/line_ending.cpp`
//! (`LINE_ENDING_STYLE`, `LINE_ENDING`). Types: package M0c; the geometry (`GetEffectiveSize`,
//! `GetShortenDepth`, `GetShapes`, ...) and the token tables: package M7.

use kicad_core::color4d::Color4D;
use kicad_core::{LineStyle, StrokeParams};
use kicad_geom::{EdaAngle, Vector2I};

/// `LINE_ENDING_STYLE`; the values are written to files through their tokens.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum LineEndingStyle {
    #[default]
    None = 0,
    Arrow,
    Circle,
    Square,
    ArrowOpen,
}

/// `LINE_ENDING`: the decoration at one end of an open graphic shape.
#[derive(Clone, Debug, PartialEq)]
pub struct LineEnding {
    pub style: LineEndingStyle, // m_style
    /// m_length: along-line axis, 0 = auto (ratio * line width).
    pub length: i32,
    /// m_width: perpendicular axis, 0 = auto.
    pub width: i32,
    pub stroke: StrokeParams, // m_stroke
}

impl Default for LineEnding {
    /// `LINE_ENDING()`: no ending, automatic size, stroke `( 0, DEFAULT, UNSPECIFIED )`.
    fn default() -> Self {
        LineEnding::new(LineEndingStyle::None, 0, 0, 0)
    }
}

impl LineEnding {
    /// `LINE_ENDING::DEFAULT_RATIO_LENGTH`.
    pub const DEFAULT_RATIO_LENGTH: f64 = 5.0;
    /// `LINE_ENDING::DEFAULT_RATIO_WIDTH`.
    pub const DEFAULT_RATIO_WIDTH: f64 = 5.0;

    /// `LINE_ENDING( aStyle, aLength, aWidth, aStrokeWidth )`.
    pub fn new(style: LineEndingStyle, length: i32, width: i32, stroke_width: i32) -> Self {
        LineEnding {
            style,
            length,
            width,
            stroke: StrokeParams::new(stroke_width, LineStyle::Default, Color4D::UNSPECIFIED),
        }
    }

    /// `GetStyle()`.
    pub fn style(&self) -> LineEndingStyle {
        self.style
    }

    /// `SetStyle()`.
    pub fn set_style(&mut self, style: LineEndingStyle) {
        self.style = style;
    }

    /// `GetLength()`.
    pub fn length(&self) -> i32 {
        self.length
    }

    /// `SetLength()`.
    pub fn set_length(&mut self, length: i32) {
        self.length = length;
    }

    /// `GetWidth()`.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// `SetWidth()`.
    pub fn set_width(&mut self, width: i32) {
        self.width = width;
    }

    /// `SetUniformSize()`.
    pub fn set_uniform_size(&mut self, size: i32) {
        self.length = size;
        self.width = size;
    }

    /// `GetStrokeWidth()`.
    pub fn stroke_width(&self) -> i32 {
        self.stroke.width()
    }

    /// `SetStrokeWidth()`.
    pub fn set_stroke_width(&mut self, width: i32) {
        self.stroke.set_width(width);
    }

    /// `GetStroke()`.
    pub fn stroke(&self) -> &StrokeParams {
        &self.stroke
    }

    /// `SetStroke()`.
    pub fn set_stroke(&mut self, stroke: StrokeParams) {
        self.stroke = stroke;
    }

    /// `LINE_ENDING::GetEffectiveSize( aLineWidth )`.
    pub fn effective_size(&self, line_width: i32) -> Vector2I {
        crate::port_pending!("LINE_ENDING::GetEffectiveSize", line_width)
    }

    /// `LINE_ENDING::GetShortenDepth( aLineWidth )`.
    pub fn shorten_depth(&self, line_width: i32) -> i32 {
        crate::port_pending!("LINE_ENDING::GetShortenDepth", line_width)
    }

    /// `LINE_ENDING::GetCurveOrientationDepth( aLineWidth )`.
    pub fn curve_orientation_depth(&self, line_width: i32) -> i32 {
        crate::port_pending!("LINE_ENDING::GetCurveOrientationDepth", line_width)
    }

    /// `LINE_ENDING::GetShapes( aPoint, aTangent, aLineWidth, aPolygon )`: the polygon
    /// vertices (an open 3-point polyline for `ArrowOpen`).
    pub fn shapes(&self, point: Vector2I, tangent: EdaAngle, line_width: i32) -> Vec<Vector2I> {
        crate::port_pending!("LINE_ENDING::GetShapes", point, tangent, line_width)
    }

    /// `LINE_ENDING::StyleToToken`.
    pub fn style_to_token(style: LineEndingStyle) -> &'static str {
        crate::port_pending!("LINE_ENDING::StyleToToken", style)
    }

    /// `LINE_ENDING::TokenToStyle`.
    pub fn token_to_style(token: &str) -> LineEndingStyle {
        crate::port_pending!("LINE_ENDING::TokenToStyle", token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LINE_ENDING()` (include/line_ending.h).
    #[test]
    fn default_is_the_cpp_constructor() {
        let e = LineEnding::default();
        assert_eq!(e.style, LineEndingStyle::None);
        assert_eq!((e.length, e.width, e.stroke_width()), (0, 0, 0));
        assert_eq!(e.stroke.line_style(), LineStyle::Default);
        assert_eq!(LineEndingStyle::ArrowOpen as i32, 4);
    }
}
