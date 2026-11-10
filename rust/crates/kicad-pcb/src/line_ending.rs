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
//! `GetShortenDepth`, `GetShapes`, ...) and the token tables: package M7. `Draw`, `Plot` and
//! `Format` belong to their consumers (GAL is not ported; plotter and writer packages).

use kicad_core::color4d::Color4D;
use kicad_core::{LineStyle, StrokeParams};
use kicad_geom::{EdaAngle, Vector2I, cmath, ki_round, rotate_point};

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

    /// `LINE_ENDING::GetEffectiveSize( aLineWidth )`: the (along-line, perpendicular) size,
    /// an unset axis taking the other one, both unset meaning the default ratios times the
    /// line width.
    pub fn effective_size(&self, line_width: i32) -> Vector2I {
        let auto_len: i32 = ki_round(line_width as f64 * Self::DEFAULT_RATIO_LENGTH);
        let auto_wid: i32 = ki_round(line_width as f64 * Self::DEFAULT_RATIO_WIDTH);

        let len = if self.length > 0 {
            self.length
        } else if self.width > 0 {
            self.width
        } else {
            auto_len
        };

        let wid = if self.width > 0 {
            self.width
        } else if self.length > 0 {
            self.length
        } else {
            auto_wid
        };

        Vector2I::new(len, wid)
    }

    /// `LINE_ENDING::GetShortenDepth( aLineWidth )`: how far the line body is shortened at
    /// this end.
    pub fn shorten_depth(&self, line_width: i32) -> i32 {
        if self.style == LineEndingStyle::None {
            return 0;
        }

        let sz = self.effective_size(line_width);

        let mut raw = if self.style == LineEndingStyle::Arrow {
            sz.x
        } else if self.style == LineEndingStyle::ArrowOpen {
            0
        } else {
            sz.x / 2
        };

        if self.stroke.width() > 0 {
            raw = 0.max(raw - self.stroke.width() / 2);
        }

        raw
    }

    /// `LINE_ENDING::GetCurveOrientationDepth( aLineWidth )`.
    pub fn curve_orientation_depth(&self, line_width: i32) -> i32 {
        if self.style == LineEndingStyle::ArrowOpen {
            return self.effective_size(line_width).x;
        }

        self.shorten_depth(line_width)
    }

    /// `LINE_ENDING::GetShapes( aPoint, aTangent, aLineWidth, aPolygon )`: the polygon
    /// vertices (an open 3-point polyline for `ArrowOpen`). C++ leaves `aPolygon` untouched
    /// for `NONE`; callers pass an empty vector, so this returns an empty one.
    pub fn shapes(&self, point: Vector2I, tangent: EdaAngle, line_width: i32) -> Vec<Vector2I> {
        let mut polygon: Vec<Vector2I> = Vec::new();

        if self.style == LineEndingStyle::None {
            return polygon;
        }

        let size = self.effective_size(line_width);
        let size_x = size.x;
        let size_y = size.y;

        match self.style {
            LineEndingStyle::Arrow => {
                let half_width = size_y / 2;

                polygon.push(Vector2I::new(0, 0));
                polygon.push(Vector2I::new(-size_x, half_width));
                polygon.push(Vector2I::new(-size_x, -half_width));
                polygon.push(Vector2I::new(0, 0));
            }
            LineEndingStyle::ArrowOpen => {
                let half_width = size_y / 2;

                polygon.push(Vector2I::new(-size_x, -half_width));
                polygon.push(Vector2I::new(0, 0));
                polygon.push(Vector2I::new(-size_x, half_width));
            }
            LineEndingStyle::Circle => {
                let half_x = size_x / 2;
                let half_y = size_y / 2;
                let n_seg = 32;

                for i in 0..=n_seg {
                    let angle = 2.0 * std::f64::consts::PI * i as f64 / n_seg as f64;
                    let px: i32 = ki_round(half_x as f64 * cmath::cos(angle));
                    let py: i32 = ki_round(half_y as f64 * cmath::sin(angle));
                    polygon.push(Vector2I::new(px, py));
                }
            }
            LineEndingStyle::Square => {
                let half_x = size_x / 2;
                let half_y = size_y / 2;

                polygon.push(Vector2I::new(half_x, half_y));
                polygon.push(Vector2I::new(half_x, -half_y));
                polygon.push(Vector2I::new(-half_x, -half_y));
                polygon.push(Vector2I::new(-half_x, half_y));
                polygon.push(Vector2I::new(half_x, half_y));
            }
            LineEndingStyle::None => return polygon,
        }

        for pt in &mut polygon {
            rotate_point(pt, -tangent);
            *pt += point;
        }

        polygon
    }

    /// `LINE_ENDING::StyleToToken`.
    pub fn style_to_token(style: LineEndingStyle) -> &'static str {
        match style {
            LineEndingStyle::None => "none",
            LineEndingStyle::Arrow => "arrow",
            LineEndingStyle::Circle => "circle",
            LineEndingStyle::Square => "square",
            LineEndingStyle::ArrowOpen => "arrow_open",
        }
    }

    /// `LINE_ENDING::TokenToStyle`: unknown tokens are `NONE`.
    pub fn token_to_style(token: &str) -> LineEndingStyle {
        match token {
            "arrow" => LineEndingStyle::Arrow,
            "circle" => LineEndingStyle::Circle,
            "square" => LineEndingStyle::Square,
            "arrow_open" => LineEndingStyle::ArrowOpen,
            _ => LineEndingStyle::None,
        }
    }

    /// `LINE_ENDING::s_defaultChoiceOrder`: the order of the styles in the UI choice lists.
    pub const DEFAULT_CHOICE_ORDER: [LineEndingStyle; 5] = [
        LineEndingStyle::None,
        LineEndingStyle::Arrow,
        LineEndingStyle::ArrowOpen,
        LineEndingStyle::Circle,
        LineEndingStyle::Square,
    ];

    /// `LINE_ENDING::StyleToChoiceIndex`.
    pub fn style_to_choice_index(style: LineEndingStyle) -> i32 {
        for (i, s) in Self::DEFAULT_CHOICE_ORDER.iter().enumerate() {
            if *s == style {
                return i as i32;
            }
        }

        0
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

    // Ported from qa/tests/common/test_line_ending.cpp.

    fn ending(style: LineEndingStyle, length: i32, width: i32, stroke: i32) -> LineEnding {
        LineEnding::new(style, length, width, stroke)
    }

    /// `GetEffectiveSizeUsesConfiguredAndAutoAxes`.
    #[test]
    fn get_effective_size_uses_configured_and_auto_axes() {
        let check = |l, w, e: (i32, i32)| {
            let s = ending(LineEndingStyle::Arrow, l, w, 0).effective_size(100);
            assert_eq!((s.x, s.y), e);
        };
        check(700, 300, (700, 300));
        check(700, 0, (700, 700));
        check(0, 300, (300, 300));
        check(0, 0, (500, 500));
    }

    /// `GetShapesProducesStyleGeometry`.
    #[test]
    fn get_shapes_produces_style_geometry() {
        let endpoint = Vector2I::new(1000, 2000);
        let tangent = EdaAngle::from_degrees(0.0);
        let pts = |e: LineEnding| -> Vec<(i32, i32)> {
            e.shapes(endpoint, tangent, 100)
                .iter()
                .map(|p| (p.x, p.y))
                .collect()
        };

        assert!(pts(ending(LineEndingStyle::None, 300, 200, 0)).is_empty());
        assert_eq!(
            pts(ending(LineEndingStyle::Arrow, 300, 200, 0)),
            vec![(1000, 2000), (700, 2100), (700, 1900), (1000, 2000)]
        );
        assert_eq!(
            pts(ending(LineEndingStyle::ArrowOpen, 300, 200, 0)),
            vec![(700, 1900), (1000, 2000), (700, 2100)]
        );
        assert_eq!(
            pts(ending(LineEndingStyle::Square, 400, 200, 0)),
            vec![
                (1200, 2100),
                (1200, 1900),
                (800, 1900),
                (800, 2100),
                (1200, 2100)
            ]
        );
        let circle = pts(ending(LineEndingStyle::Circle, 400, 200, 0));
        assert_eq!(circle.len(), 33);
        assert_eq!(circle[0], (1200, 2000));
        assert_eq!(circle[32], (1200, 2000));
    }

    /// `OpenArrowOrientationUsesVisualLength`.
    #[test]
    fn open_arrow_orientation_uses_visual_length() {
        use LineEndingStyle::*;
        let cod = |s, l, w, k| ending(s, l, w, k).curve_orientation_depth(100);
        assert_eq!(cod(None, 800, 300, 0), 0);
        assert_eq!(cod(Arrow, 800, 300, 0), 800);
        assert_eq!(cod(Circle, 800, 300, 0), 400);
        assert_eq!(cod(Square, 800, 300, 0), 400);
        assert_eq!(ending(ArrowOpen, 800, 300, 0).shorten_depth(100), 0);
        assert_eq!(cod(ArrowOpen, 800, 300, 0), 800);
        assert_eq!(cod(ArrowOpen, 0, 0, 0), 500);
    }

    /// `GetShortenDepthUsesEndpointPolicy`.
    #[test]
    fn get_shorten_depth_uses_endpoint_policy() {
        use LineEndingStyle::*;
        let sd = |s, l, w, k| ending(s, l, w, k).shorten_depth(100);
        assert_eq!(sd(None, 800, 300, 0), 0);
        assert_eq!(sd(Arrow, 800, 300, 0), 800);
        assert_eq!(sd(ArrowOpen, 800, 300, 0), 0);
        assert_eq!(sd(Circle, 800, 300, 0), 400);
        assert_eq!(sd(Square, 800, 300, 0), 400);
        assert_eq!(sd(Arrow, 0, 0, 0), 500);
        assert_eq!(sd(Circle, 0, 0, 0), 250);
        assert_eq!(sd(Square, 0, 0, 0), 250);
        assert_eq!(sd(Arrow, 800, 300, 100), 750);
        assert_eq!(sd(Circle, 800, 300, 100), 350);
        assert_eq!(sd(Square, 100, 300, 300), 0);
    }

    /// `LINE_ENDING::StyleToToken` / `TokenToStyle` round trip.
    #[test]
    fn tokens_round_trip() {
        for s in LineEnding::DEFAULT_CHOICE_ORDER {
            assert_eq!(LineEnding::token_to_style(LineEnding::style_to_token(s)), s);
        }
        assert_eq!(LineEnding::token_to_style("bogus"), LineEndingStyle::None);
        assert_eq!(
            LineEnding::style_to_choice_index(LineEndingStyle::Circle),
            3
        );
    }
}
