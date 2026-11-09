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

//! Port of `include/font/text_attributes.h` and `common/font/text_attributes.cpp`: text
//! alignment enums and `TEXT_ATTRIBUTES`.
//!
//! Representation choices (this crate has no font or geometry types):
//!
//! * `m_Font` (`KIFONT::FONT*`) is the font *name* (`FONT::GetName()`), `None` for no font.
//!   `Compare` only ever looks at the name, so ordering is unchanged.
//! * `m_Angle` (`EDA_ORIENTATION`) is an `f64` in degrees, normalised to [0, 360) with -0
//!   turned into 0 by [`TextAttributes::set_angle`], exactly like `EDA_ORIENTATION::SetAngle`.
//! * `m_Size` (`VECTOR2I`) is an `(i32, i32)` pair.

use std::cmp::Ordering;

use crate::color4d::Color4D;

/// Port of `enum GR_TEXT_H_ALIGN_T`.  The values -1, 0, 1 are used in computations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum GrTextHAlignT {
    /// `GR_TEXT_H_ALIGN_LEFT`
    Left = -1,
    /// `GR_TEXT_H_ALIGN_CENTER`
    #[default]
    Center = 0,
    /// `GR_TEXT_H_ALIGN_RIGHT`
    Right = 1,
    /// `GR_TEXT_H_ALIGN_INDETERMINATE`
    Indeterminate = 2,
}

/// Port of `enum GR_TEXT_V_ALIGN_T`.  The values -1, 0, 1 are used in computations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum GrTextVAlignT {
    /// `GR_TEXT_V_ALIGN_TOP`
    Top = -1,
    /// `GR_TEXT_V_ALIGN_CENTER`
    #[default]
    Center = 0,
    /// `GR_TEXT_V_ALIGN_BOTTOM`
    Bottom = 1,
    /// `GR_TEXT_V_ALIGN_INDETERMINATE`
    Indeterminate = 2,
}

impl GrTextHAlignT {
    /// Port of `GetFlippedAlignment( GR_TEXT_H_ALIGN_T )`.
    pub const fn flipped(self) -> Self {
        match self {
            GrTextHAlignT::Left => GrTextHAlignT::Right,
            GrTextHAlignT::Right => GrTextHAlignT::Left,
            other => other,
        }
    }

    /// Port of `ToHAlignment( int )`: the alignment for the sign of `x`.
    pub const fn from_sign(x: i32) -> Self {
        if x < 0 {
            GrTextHAlignT::Left
        } else if x > 0 {
            GrTextHAlignT::Right
        } else {
            GrTextHAlignT::Center
        }
    }
}

impl GrTextVAlignT {
    /// Port of `GetFlippedAlignment( GR_TEXT_V_ALIGN_T )`.
    pub const fn flipped(self) -> Self {
        match self {
            GrTextVAlignT::Bottom => GrTextVAlignT::Top,
            GrTextVAlignT::Top => GrTextVAlignT::Bottom,
            other => other,
        }
    }
}

/// `EDA_ORIENTATION::SetAngle`: `EDA_ANGLE::Normalize()` (fold, then into [0, 360)) and -0
/// becomes 0.
pub fn normalize_orientation(degrees: f64) -> f64 {
    // foldToTwoPeriods
    let mut v = if !degrees.is_finite() {
        0.0
    } else if degrees.abs() < 2.0 * 360.0 {
        degrees
    } else {
        (degrees % 360.0) + 360.0_f64.copysign(degrees)
    };

    while v < -0.0 {
        v += 360.0;
    }

    while v >= 360.0 {
        v -= 360.0;
    }

    // Normalize -0 to 0 to avoid -0 in display and serialization.
    if v == 0.0 { 0.0 } else { v }
}

/// Port of `class TEXT_ATTRIBUTES`.
#[derive(Clone, Debug)]
pub struct TextAttributes {
    /// `m_Font`: the font name (`KIFONT::FONT::GetName()`), `None` for no font.
    pub font: Option<String>,
    /// `m_Halign`
    pub halign: GrTextHAlignT,
    /// `m_Valign`
    pub valign: GrTextVAlignT,
    /// `m_Angle`: orientation in degrees in [0, 360); set it with
    /// [`TextAttributes::set_angle`] to keep it normalised.
    pub angle: f64,
    /// `m_LineSpacing`
    pub line_spacing: f64,
    /// `m_StrokeWidth`
    pub stroke_width: i32,
    /// `m_Italic`
    pub italic: bool,
    /// `m_Bold`
    pub bold: bool,
    /// `m_Underlined`
    pub underlined: bool,
    /// `m_Hover`
    pub hover: bool,
    /// `m_Color`
    pub color: Color4D,
    /// `m_Mirrored`
    pub mirrored: bool,
    /// `m_Multiline`
    pub multiline: bool,
    /// `m_Size` (x, y)
    pub size: (i32, i32),
    /// `m_KeepUpright`: if true, keep rotation angle between -90...90 degrees for readability
    pub keep_upright: bool,
}

impl Default for TextAttributes {
    /// Port of `TEXT_ATTRIBUTES( KIFONT::FONT* aFont = nullptr )`.
    fn default() -> Self {
        TextAttributes {
            font: None,
            halign: GrTextHAlignT::Center,
            valign: GrTextVAlignT::Center,
            angle: 0.0,
            line_spacing: 1.0,
            stroke_width: 0,
            italic: false,
            bold: false,
            underlined: false,
            hover: false,
            color: Color4D::UNSPECIFIED,
            mirrored: false,
            multiline: true,
            size: (0, 0),
            keep_upright: false,
        }
    }
}

impl TextAttributes {
    /// `TEXT_ATTRIBUTES( aFont )` with a font name.
    pub fn with_font(font: Option<String>) -> Self {
        TextAttributes {
            font,
            ..Default::default()
        }
    }

    /// `m_Angle = EDA_ANGLE( degrees, DEGREES_T )` (normalising assignment of
    /// `EDA_ORIENTATION`).
    pub fn set_angle(&mut self, degrees: f64) {
        self.angle = normalize_orientation(degrees);
    }

    /// Port of `TEXT_ATTRIBUTES::Compare`: negative, 0 or positive.
    pub fn compare(&self, rhs: &TextAttributes) -> i32 {
        let font_name = self.font.as_deref().unwrap_or("");
        let rhs_font_name = rhs.font.as_deref().unwrap_or("");

        // wxString::Cmp
        match font_name.cmp(rhs_font_name) {
            Ordering::Less => return -1,
            Ordering::Greater => return 1,
            Ordering::Equal => {}
        }

        if self.size.0 != rhs.size.0 {
            return self.size.0.wrapping_sub(rhs.size.0);
        }

        if self.size.1 != rhs.size.1 {
            return self.size.1.wrapping_sub(rhs.size.1);
        }

        if self.stroke_width != rhs.stroke_width {
            return self.stroke_width.wrapping_sub(rhs.stroke_width);
        }

        if self.angle != rhs.angle {
            return if self.angle < rhs.angle { -1 } else { 1 };
        }

        if self.line_spacing != rhs.line_spacing {
            return if self.line_spacing < rhs.line_spacing {
                -1
            } else {
                1
            };
        }

        if self.halign != rhs.halign {
            return self.halign as i32 - rhs.halign as i32;
        }

        if self.valign != rhs.valign {
            return self.valign as i32 - rhs.valign as i32;
        }

        if self.italic != rhs.italic {
            return self.italic as i32 - rhs.italic as i32;
        }

        if self.bold != rhs.bold {
            return self.bold as i32 - rhs.bold as i32;
        }

        if self.underlined != rhs.underlined {
            return self.underlined as i32 - rhs.underlined as i32;
        }

        let retv = self.color.compare(&rhs.color);

        if retv != 0 {
            return retv;
        }

        if self.mirrored != rhs.mirrored {
            return self.mirrored as i32 - rhs.mirrored as i32;
        }

        if self.multiline != rhs.multiline {
            return self.multiline as i32 - rhs.multiline as i32;
        }

        self.keep_upright as i32 - rhs.keep_upright as i32
    }
}

/// `operator==`: `Compare( aRhs ) == 0`.
impl PartialEq for TextAttributes {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other) == 0
    }
}

/// `operator<` / `operator>` through `Compare`.
impl PartialOrd for TextAttributes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.compare(other).cmp(&0))
    }
}
