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

//! Port of the data part of `include/stroke_params.h`, `include/stroke_params_parser.h` and
//! `common/stroke_params.cpp`: `LINE_STYLE`, `STROKE_PARAMS`, its file format writer and
//! `STROKE_PARAMS_PARSER::ParseStroke`.
//!
//! `STROKE_PARAMS::Stroke` (dashed line geometry) needs the shape classes of `kicad-geom` and
//! lives with the shapes; `GetMsgPanelInfo` returns `(label, text)` pairs instead of
//! `MSG_PANEL_ITEM`s.

use kicad_sexpr::keywords::strokeparams_t::{StrokeParamsLexer, T};
use kicad_sexpr::{IoError, OutputFormatter};

use crate::base_units::EdaIuScale;
use crate::color4d::Color4D;
use crate::eda_units::{self, EdaDataType, EdaUnits};
use crate::string_utils::format_double2_str;
use crate::wxstr;

/// Port of `enum class LINE_STYLE`: dashed line types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum LineStyle {
    /// `DEFAULT`
    #[default]
    Default = -1,
    /// `SOLID` (`FIRST_TYPE`)
    Solid = 0,
    /// `DASH`
    Dash = 1,
    /// `DOT`
    Dot = 2,
    /// `DASHDOT`
    DashDot = 3,
    /// `DASHDOTDOT` (`LAST_TYPE`)
    DashDotDot = 4,
}

impl LineStyle {
    /// `LINE_STYLE::FIRST_TYPE`
    pub const FIRST_TYPE: LineStyle = LineStyle::Solid;
    /// `LINE_STYLE::LAST_TYPE`
    pub const LAST_TYPE: LineStyle = LineStyle::DashDotDot;
}

/// Port of `enum class WIRE_STYLE`: a cover of `LINE_STYLE` for the properties manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum WireStyle {
    /// `DEFAULT`
    #[default]
    Default = -1,
    /// `SOLID`
    Solid = 0,
    /// `DASH`
    Dash = 1,
    /// `DOT`
    Dot = 2,
    /// `DASHDOT`
    DashDot = 3,
    /// `DASHDOTDOT`
    DashDotDot = 4,
}

/// Port of `lineTypeNames` (a `std::map`, so in `LINE_STYLE` order): the UI names (the
/// bitmaps are not ported).
pub const LINE_TYPE_NAMES: [(LineStyle, &str); 5] = [
    (LineStyle::Solid, "Solid"),
    (LineStyle::Dash, "Dashed"),
    (LineStyle::Dot, "Dotted"),
    (LineStyle::DashDot, "Dash-Dot"),
    (LineStyle::DashDotDot, "Dash-Dot-Dot"),
];

/// `DEFAULT_LINE_STYLE_LABEL`
pub const DEFAULT_LINE_STYLE_LABEL: &str = "Solid";
/// `DEFAULT_WIRE_STYLE_LABEL`
pub const DEFAULT_WIRE_STYLE_LABEL: &str = "Default";
/// `INDETERMINATE_STYLE`
pub const INDETERMINATE_STYLE: &str = "Leave unchanged";

/// Port of `class STROKE_PARAMS`: line width, style and colour of a graphic stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeParams {
    width: i32,
    line_style: LineStyle,
    color: Color4D,
}

impl Default for StrokeParams {
    fn default() -> Self {
        StrokeParams::new(0, LineStyle::Default, Color4D::UNSPECIFIED)
    }
}

impl StrokeParams {
    /// `STROKE_PARAMS( int aWidth, LINE_STYLE aLineStyle, const COLOR4D& aColor )`.
    pub fn new(width: i32, line_style: LineStyle, color: Color4D) -> Self {
        StrokeParams {
            width,
            line_style,
            color,
        }
    }

    /// `GetWidth`
    pub fn width(&self) -> i32 {
        self.width
    }

    /// `SetWidth`
    pub fn set_width(&mut self, width: i32) {
        self.width = width;
    }

    /// `GetLineStyle`
    pub fn line_style(&self) -> LineStyle {
        self.line_style
    }

    /// `SetLineStyle`
    pub fn set_line_style(&mut self, line_style: LineStyle) {
        self.line_style = line_style;
    }

    /// `GetColor`
    pub fn color(&self) -> &Color4D {
        &self.color
    }

    /// `SetColor`
    pub fn set_color(&mut self, color: Color4D) {
        self.color = color;
    }

    /// Port of `STROKE_PARAMS::operator<`.
    pub fn less_than(&self, other: &StrokeParams) -> bool {
        if self.width != other.width {
            return self.width < other.width;
        }

        if self.line_style != other.line_style {
            return self.line_style < other.line_style;
        }

        self.color.less_than(&other.color)
    }

    /// Port of `STROKE_PARAMS::GetLineStyleToken`: the file format token.
    pub fn get_line_style_token(style: LineStyle) -> &'static str {
        match style {
            LineStyle::Dash => "dash",
            LineStyle::Dot => "dot",
            LineStyle::DashDot => "dash_dot",
            LineStyle::DashDotDot => "dash_dot_dot",
            LineStyle::Solid => "solid",
            LineStyle::Default => "default",
        }
    }

    /// Port of `STROKE_PARAMS::Format`: `(stroke (width W) (type T))`, plus
    /// `(color R G B A)` unless the colour is unspecified.
    pub fn format(
        &self,
        formatter: &mut dyn OutputFormatter,
        iu_scale: &EdaIuScale,
    ) -> Result<(), IoError> {
        let width = eda_units::format_internal_units_distance(iu_scale, self.width);
        let token = Self::get_line_style_token(self.line_style);

        if self.color == Color4D::UNSPECIFIED {
            formatter.print(format_args!("(stroke (width {width}) (type {token}))"))?;
        } else {
            formatter.print(format_args!(
                "(stroke (width {width}) (type {token}) (color {} {} {} {}))",
                wxstr::ki_round_i32(self.color.r * 255.0),
                wxstr::ki_round_i32(self.color.g * 255.0),
                wxstr::ki_round_i32(self.color.b * 255.0),
                format_double2_str(self.color.a)
            ))?;
        }

        Ok(())
    }

    /// Port of `STROKE_PARAMS::GetMsgPanelInfo`: `("Line Style", name)` and
    /// `("Line Width", value)` (through `UNITS_PROVIDER::MessageTextFromValue`, given here
    /// as the scale and user units).
    pub fn get_msg_panel_info(
        &self,
        iu_scale: &EdaIuScale,
        units: EdaUnits,
        include_style: bool,
        include_width: bool,
    ) -> Vec<(String, String)> {
        let mut list = Vec::new();

        if include_style {
            let msg = LINE_TYPE_NAMES
                .iter()
                .find(|(style, _)| *style == self.line_style)
                .map_or("Default", |(_, name)| name);

            list.push(("Line Style".to_string(), msg.to_string()));
        }

        if include_width {
            list.push((
                "Line Width".to_string(),
                eda_units::ui::message_text_from_value_int(
                    iu_scale,
                    units,
                    self.width,
                    true,
                    EdaDataType::Distance,
                ),
            ));
        }

        list
    }
}

/// Port of `STROKE_PARAMS_PARSER`: parses the content of a `(stroke ...)` list (after the
/// `stroke` keyword) with a lexer using the `stroke_params.keywords` table.
pub struct StrokeParamsParser {
    iu_per_mm: i32,
}

impl StrokeParamsParser {
    /// `STROKE_PARAMS_PARSER( LINE_READER* aReader, int iuPerMM )`: the lexer is passed to
    /// [`StrokeParamsParser::parse_stroke`] (typically through `DsnLexer::with_sub_lexer`).
    pub fn new(iu_per_mm: i32) -> Self {
        StrokeParamsParser { iu_per_mm }
    }

    /// Port of `STROKE_PARAMS_PARSER::ParseStroke`.
    pub fn parse_stroke(
        &self,
        lexer: &mut StrokeParamsLexer,
        stroke: &mut StrokeParams,
    ) -> Result<(), IoError> {
        let mut token = lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(lexer.expecting(T::T_LEFT));
            }

            token = lexer.next_tok()?;

            match token {
                T::T_width => {
                    let w = Self::parse_double(lexer, "stroke width")? * self.iu_per_mm as f64;
                    stroke.set_width(wxstr::ki_round_i32(w));
                    lexer.need_right()?;
                }
                T::T_type => {
                    token = lexer.next_tok()?;

                    let style = match token {
                        T::T_dash => LineStyle::Dash,
                        T::T_dot => LineStyle::Dot,
                        T::T_dash_dot => LineStyle::DashDot,
                        T::T_dash_dot_dot => LineStyle::DashDotDot,
                        T::T_solid => LineStyle::Solid,
                        T::T_default => LineStyle::Default,
                        _ => {
                            return Err(lexer.expecting_str(
                                "solid, dash, dash_dot, dash_dot_dot, dot or default",
                            ));
                        }
                    };

                    stroke.set_line_style(style);
                    lexer.need_right()?;
                }
                T::T_color => {
                    let r = Self::parse_int(lexer, "red")? as f64 / 255.0;
                    let g = Self::parse_int(lexer, "green")? as f64 / 255.0;
                    let b = Self::parse_int(lexer, "blue")? as f64 / 255.0;

                    // std::clamp( parseDouble( "alpha" ), 0.0, 1.0 )
                    let a = Self::parse_double(lexer, "alpha")?.clamp(0.0, 1.0);

                    stroke.set_color(Color4D::new(r, g, b, a));
                    lexer.need_right()?;
                }
                _ => return Err(lexer.expecting_str("width, type, or color")),
            }

            token = lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `STROKE_PARAMS_PARSER::parseInt`: `atoi( CurText() )` of a number token.
    fn parse_int(lexer: &mut StrokeParamsLexer, text: &str) -> Result<i32, IoError> {
        let token = lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(lexer.expecting_str(text));
        }

        let (value, _) = kicad_sexpr::lexer::c_numeric::strtol(lexer.cur_text().as_bytes(), 10);

        Ok(value as i32)
    }

    /// Port of `STROKE_PARAMS_PARSER::parseDouble`.
    fn parse_double(lexer: &mut StrokeParamsLexer, text: &str) -> Result<f64, IoError> {
        let token = lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(lexer.expecting_str(text));
        }

        lexer.parse_double()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_units::PCB_IU_SCALE;
    use kicad_sexpr::StringFormatter;

    #[test]
    fn format_and_parse_round_trip() {
        let mut f = StringFormatter::new();
        StrokeParams::new(150000, LineStyle::Dash, Color4D::UNSPECIFIED)
            .format(&mut f, &PCB_IU_SCALE)
            .unwrap();
        assert_eq!(f.string(), "(stroke (width 0.15) (type dash))");

        let mut f = StringFormatter::new();
        let stroke = StrokeParams::new(0, LineStyle::DashDotDot, Color4D::new(1.0, 0.5, 0.0, 0.25));
        stroke.format(&mut f, &PCB_IU_SCALE).unwrap();
        assert_eq!(
            f.string(),
            "(stroke (width 0) (type dash_dot_dot) (color 255 128 0 0.25))"
        );

        // ParseStroke starts after "(stroke"
        let mut lexer =
            StrokeParamsLexer::new("(width 0.2) (type dot) (color 10 20 30 1.5))", "test");
        let mut parsed = StrokeParams::default();
        StrokeParamsParser::new(1_000_000)
            .parse_stroke(&mut lexer, &mut parsed)
            .unwrap();
        assert_eq!(parsed.width(), 200000);
        assert_eq!(parsed.line_style(), LineStyle::Dot);
        assert_eq!(
            *parsed.color(),
            Color4D::new(10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0)
        );

        let mut lexer = StrokeParamsLexer::new("(type wavy))", "test");
        let err = StrokeParamsParser::new(1_000_000)
            .parse_stroke(&mut lexer, &mut parsed)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Expecting solid, dash, dash_dot, dash_dot_dot, dot or default")
        );
    }

    #[test]
    fn tokens_and_order() {
        assert_eq!(
            StrokeParams::get_line_style_token(LineStyle::Default),
            "default"
        );
        assert_eq!(
            StrokeParams::get_line_style_token(LineStyle::DashDot),
            "dash_dot"
        );
        let a = StrokeParams::new(1, LineStyle::Solid, Color4D::BLACK);
        let b = StrokeParams::new(1, LineStyle::Dash, Color4D::BLACK);
        assert!(a.less_than(&b) && !b.less_than(&a));
        let info = a.get_msg_panel_info(&PCB_IU_SCALE, EdaUnits::Mm, true, true);
        assert_eq!(info[0], ("Line Style".to_string(), "Solid".to_string()));
        assert_eq!(info[1].1, "1.000e-06 mm");
    }
}
