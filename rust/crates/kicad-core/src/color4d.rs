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
// Copyright (C) 2017 CERN
// @author Maciej Suminski <maciej.suminski@cern.ch>

//! Port of `include/gal/color4d.h` and `common/gal/color4d.cpp`: the legacy `EDA_COLOR_T`
//! palette and the RGBA colour `KIGFX::COLOR4D`.
//!
//! `COLOR4D` holds an optional text (`m_text`, a `std::shared_ptr<wxString>` used for
//! evaluated colours); here it is an `Option<String>`, so [`Color4D`] is `Clone` but not
//! `Copy`.  The parts of `wxColour` the port needs (`ToColour`, string parsing of
//! `wxColour::Set`) are reproduced in this module.

use std::fmt;

use crate::printf::format_f;
use crate::wxstr;

/// Port of `enum EDA_COLOR_T`: the legacy colour numbers.  A newtype over the C++ value so
/// that the flags (`HIGHLIGHT_FLAG`, `MASKCOLOR`) and out-of-range values stay
/// representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdaColorT(pub i32);

#[allow(missing_docs)]
impl EdaColorT {
    pub const UNSPECIFIED_COLOR: EdaColorT = EdaColorT(-1);
    pub const BLACK: EdaColorT = EdaColorT(0);
    pub const DARKDARKGRAY: EdaColorT = EdaColorT(1);
    pub const DARKGRAY: EdaColorT = EdaColorT(2);
    pub const LIGHTGRAY: EdaColorT = EdaColorT(3);
    pub const WHITE: EdaColorT = EdaColorT(4);
    pub const LIGHTYELLOW: EdaColorT = EdaColorT(5);
    pub const DARKBLUE: EdaColorT = EdaColorT(6);
    pub const DARKGREEN: EdaColorT = EdaColorT(7);
    pub const DARKCYAN: EdaColorT = EdaColorT(8);
    pub const DARKRED: EdaColorT = EdaColorT(9);
    pub const DARKMAGENTA: EdaColorT = EdaColorT(10);
    pub const DARKBROWN: EdaColorT = EdaColorT(11);
    pub const BLUE: EdaColorT = EdaColorT(12);
    pub const GREEN: EdaColorT = EdaColorT(13);
    pub const CYAN: EdaColorT = EdaColorT(14);
    pub const RED: EdaColorT = EdaColorT(15);
    pub const MAGENTA: EdaColorT = EdaColorT(16);
    pub const BROWN: EdaColorT = EdaColorT(17);
    pub const LIGHTBLUE: EdaColorT = EdaColorT(18);
    pub const LIGHTGREEN: EdaColorT = EdaColorT(19);
    pub const LIGHTCYAN: EdaColorT = EdaColorT(20);
    pub const LIGHTRED: EdaColorT = EdaColorT(21);
    pub const LIGHTMAGENTA: EdaColorT = EdaColorT(22);
    pub const YELLOW: EdaColorT = EdaColorT(23);
    pub const PUREBLUE: EdaColorT = EdaColorT(24);
    pub const PUREGREEN: EdaColorT = EdaColorT(25);
    pub const PURECYAN: EdaColorT = EdaColorT(26);
    pub const PURERED: EdaColorT = EdaColorT(27);
    pub const PUREMAGENTA: EdaColorT = EdaColorT(28);
    pub const PUREYELLOW: EdaColorT = EdaColorT(29);
    pub const LIGHTERORANGE: EdaColorT = EdaColorT(30);
    pub const DARKORANGE: EdaColorT = EdaColorT(31);
    pub const ORANGE: EdaColorT = EdaColorT(32);
    pub const LIGHTORANGE: EdaColorT = EdaColorT(33);
    pub const PUREORANGE: EdaColorT = EdaColorT(34);
    /// Number of colors
    pub const NBCOLORS: EdaColorT = EdaColorT(35);
    pub const HIGHLIGHT_FLAG: EdaColorT = EdaColorT(1 << 19);
    /// mask for color index into colorRefs()[]
    pub const MASKCOLOR: EdaColorT = EdaColorT(31);
}

/// Port of `struct StructColors`: one entry of the legacy palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructColors {
    /// `m_Blue`
    pub blue: u8,
    /// `m_Green`
    pub green: u8,
    /// `m_Red`
    pub red: u8,
    /// `m_Numcolor`
    pub numcolor: EdaColorT,
    /// `m_ColorName` (untranslated)
    pub color_name: &'static str,
    /// `m_LightColor`
    pub light_color: EdaColorT,
}

const fn sc(
    blue: u8,
    green: u8,
    red: u8,
    numcolor: EdaColorT,
    color_name: &'static str,
    light_color: EdaColorT,
) -> StructColors {
    StructColors {
        blue,
        green,
        red,
        numcolor,
        color_name,
        light_color,
    }
}

/// The table behind `colorRefs()`.  Note that its order is not the order of `EDA_COLOR_T`
/// (`LIGHTERORANGE` is entry 6); the C++ relies on both orders in different places.
static COLOR_REFS: [StructColors; 35] = {
    use EdaColorT as E;
    [
        sc(0, 0, 0, E::BLACK, "Black", E::DARKDARKGRAY),
        sc(72, 72, 72, E::DARKDARKGRAY, "Gray 1", E::DARKGRAY),
        sc(132, 132, 132, E::DARKGRAY, "Gray 2", E::LIGHTGRAY),
        sc(194, 194, 194, E::LIGHTGRAY, "Gray 3", E::WHITE),
        sc(255, 255, 255, E::WHITE, "White", E::WHITE),
        sc(194, 255, 255, E::LIGHTYELLOW, "L.Yellow", E::WHITE),
        sc(191, 229, 255, E::LIGHTERORANGE, "L.Orange", E::WHITE),
        sc(72, 0, 0, E::DARKBLUE, "Blue 1", E::BLUE),
        sc(0, 72, 0, E::DARKGREEN, "Green 1", E::GREEN),
        sc(72, 72, 0, E::DARKCYAN, "Cyan 1", E::CYAN),
        sc(0, 0, 72, E::DARKRED, "Red 1", E::RED),
        sc(72, 0, 72, E::DARKMAGENTA, "Magenta 1", E::MAGENTA),
        sc(0, 72, 72, E::DARKBROWN, "Brown 1", E::BROWN),
        sc(0, 77, 128, E::DARKORANGE, "Orange 1", E::ORANGE),
        sc(132, 0, 0, E::BLUE, "Blue 2", E::LIGHTBLUE),
        sc(0, 132, 0, E::GREEN, "Green 2", E::LIGHTGREEN),
        sc(132, 132, 0, E::CYAN, "Cyan 2", E::LIGHTCYAN),
        sc(0, 0, 132, E::RED, "Red 2", E::LIGHTRED),
        sc(132, 0, 132, E::MAGENTA, "Magenta 2", E::LIGHTMAGENTA),
        sc(0, 132, 132, E::BROWN, "Brown 2", E::YELLOW),
        sc(0, 102, 204, E::ORANGE, "Orange 2", E::LIGHTORANGE),
        sc(194, 0, 0, E::LIGHTBLUE, "Blue 3", E::PUREBLUE),
        sc(0, 194, 0, E::LIGHTGREEN, "Green 3", E::PUREGREEN),
        sc(194, 194, 0, E::LIGHTCYAN, "Cyan 3", E::PURECYAN),
        sc(0, 0, 194, E::LIGHTRED, "Red 3", E::PURERED),
        sc(194, 0, 194, E::LIGHTMAGENTA, "Magenta 3", E::PUREMAGENTA),
        sc(0, 194, 194, E::YELLOW, "Yellow 3", E::PUREYELLOW),
        sc(0, 133, 221, E::LIGHTORANGE, "Orange 3", E::PUREORANGE),
        sc(255, 0, 0, E::PUREBLUE, "Blue 4", E::WHITE),
        sc(0, 255, 0, E::PUREGREEN, "Green 4", E::WHITE),
        sc(255, 255, 0, E::PURECYAN, "Cyan 4", E::WHITE),
        sc(0, 0, 255, E::PURERED, "Red 4", E::WHITE),
        sc(255, 0, 255, E::PUREMAGENTA, "Magenta 4", E::WHITE),
        sc(0, 255, 255, E::PUREYELLOW, "Yellow 4", E::WHITE),
        sc(0, 153, 255, E::PUREORANGE, "Orange 4", E::WHITE),
    ]
};

/// Port of `colorRefs()`: the global list of legacy colour names.
pub fn color_refs() -> &'static [StructColors; 35] {
    &COLOR_REFS
}

/// The 8 bit RGBA channels of a `wxColour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct WxColour {
    /// `Red()`
    pub red: u8,
    /// `Green()`
    pub green: u8,
    /// `Blue()`
    pub blue: u8,
    /// `Alpha()` (`wxALPHA_OPAQUE` = 255)
    pub alpha: u8,
}

/// `wxALPHA_OPAQUE`
pub const WX_ALPHA_OPAQUE: u8 = 255;

/// Port of `KIGFX::COLOR4D`: an RGBA colour with `f64` components in 0 .. 1, or the text of
/// an evaluated colour expression.
#[derive(Clone, Debug)]
pub struct Color4D {
    /// Red component
    pub r: f64,
    /// Green component
    pub g: f64,
    /// Blue component
    pub b: f64,
    /// Alpha component
    pub a: f64,
    /// `m_text`: optional text for evaluated colors
    pub text: Option<String>,
}

impl Default for Color4D {
    /// `COLOR4D()`: black, opaque.
    fn default() -> Self {
        Color4D {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
            text: None,
        }
    }
}

/// `std::min` / `std::max` for doubles (the first argument wins ties and NaN comparisons).
fn std_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn std_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `wxRound`: round half away from zero.
fn wx_round(x: f64) -> i32 {
    wxstr::ki_round_i32(x)
}

/// A minimal `sscanf` cursor over the characters of a `wxString`, for the formats used by
/// `wxColour::Set` and `COLOR4D::SetFromHexString`.
struct Scanner<'a> {
    s: &'a [char],
    pos: usize,
}

impl Scanner<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.s.len()
            && (self.s[self.pos] as u32) < 128
            && wxstr::is_space(self.s[self.pos])
        {
            self.pos += 1;
        }
    }

    /// A literal format character: must match exactly.
    fn literal(&mut self, c: char) -> bool {
        if self.s.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// The digits of an integer conversion with optional sign; returns the value with C
    /// `strtol` / `strtoul` semantics (saturating, negation for unsigned).
    fn integer(&mut self, base: u32) -> Option<(bool, u64, bool)> {
        self.skip_ws();

        let start = self.pos;
        let mut negative = false;

        if let Some(&c) = self.s.get(self.pos) {
            if c == '+' || c == '-' {
                negative = c == '-';
                self.pos += 1;
            }
        }

        if base == 16
            && self.s.get(self.pos) == Some(&'0')
            && matches!(self.s.get(self.pos + 1), Some('x') | Some('X'))
            && self
                .s
                .get(self.pos + 2)
                .is_some_and(|c| c.is_ascii_hexdigit())
        {
            self.pos += 2;
        }

        let mut value: u64 = 0;
        let mut overflow = false;
        let digits_start = self.pos;

        while let Some(d) = self.s.get(self.pos).and_then(|c| c.to_digit(base)) {
            match value
                .checked_mul(base as u64)
                .and_then(|v| v.checked_add(d as u64))
            {
                Some(v) => value = v,
                None => overflow = true,
            }

            self.pos += 1;
        }

        if self.pos == digits_start {
            self.pos = start;
            return None;
        }

        Some((negative, value, overflow))
    }

    /// `%d`: `strtol` clamped to `long`, then truncated to `int`.
    fn int_d(&mut self) -> Option<i32> {
        let (negative, value, overflow) = self.integer(10)?;
        let v: i64 = if overflow || value > i64::MAX as u64 + negative as u64 {
            if negative { i64::MIN } else { i64::MAX }
        } else if negative {
            (value as i64).wrapping_neg()
        } else {
            value as i64
        };

        Some(v as i32)
    }

    /// `%lx`: `strtoul` (overflow gives `ULONG_MAX`, a minus sign negates).
    fn long_x(&mut self) -> Option<u64> {
        let (negative, value, overflow) = self.integer(16)?;

        if overflow {
            return Some(u64::MAX);
        }

        Some(if negative {
            value.wrapping_neg()
        } else {
            value
        })
    }
}

impl Color4D {
    /// For legacy support; used as a value to indicate color hasn't been set yet
    pub const UNSPECIFIED: Color4D = Color4D::new(0.0, 0.0, 0.0, 0.0);
    /// `COLOR4D::WHITE`
    pub const WHITE: Color4D = Color4D::new(1.0, 1.0, 1.0, 1.0);
    /// `COLOR4D::BLACK`
    pub const BLACK: Color4D = Color4D::new(0.0, 0.0, 0.0, 1.0);
    /// `COLOR4D::CLEAR`
    pub const CLEAR: Color4D = Color4D::new(1.0, 0.0, 1.0, 0.0);

    /// `COLOR4D( double aRed, double aGreen, double aBlue, double aAlpha )` (the range
    /// assertions are debug-only in C++ and not repeated).
    pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Color4D {
            r,
            g,
            b,
            a,
            text: None,
        }
    }

    /// Port of `COLOR4D( EDA_COLOR_T aColor )`.
    pub fn from_eda_color(color: EdaColorT) -> Self {
        if color <= EdaColorT::UNSPECIFIED_COLOR || color >= EdaColorT::NBCOLORS {
            return Color4D::UNSPECIFIED;
        }

        let Some(candidate) = COLOR_REFS.iter().find(|c| c.numcolor == color) else {
            return Color4D::UNSPECIFIED;
        };

        Color4D::new(
            candidate.red as f64 / 255.0,
            candidate.green as f64 / 255.0,
            candidate.blue as f64 / 255.0,
            1.0,
        )
    }

    /// Port of `COLOR4D( const wxString& aColorStr )`: a hex (`#RRGGBB[AA]`) or CSS / wx colour
    /// string; anything else is kept as the colour text (components stay black).
    pub fn from_string(color_str: &str) -> Self {
        let mut c = Color4D::default();

        if !c.set_from_hex_string(color_str) && !c.set_from_wx_string(color_str) {
            c.text = Some(color_str.to_string());
        }

        c
    }

    /// Port of `COLOR4D( const wxColour& aColor )`.
    pub fn from_wx_colour(color: WxColour) -> Self {
        Color4D::new(
            color.red as f64 / 255.0,
            color.green as f64 / 255.0,
            color.blue as f64 / 255.0,
            color.alpha as f64 / 255.0,
        )
    }

    /// Port of `COLOR4D::FromCSSRGBA`.
    pub fn from_css_rgba(&mut self, red: i32, green: i32, blue: i32, alpha: f64) -> &mut Self {
        self.r = red.clamp(0, 255) as f64 / 255.0;
        self.g = green.clamp(0, 255) as f64 / 255.0;
        self.b = blue.clamp(0, 255) as f64 / 255.0;
        // std::clamp( aAlpha, 0.0, 1.0 ) (NaN stays NaN in both)
        self.a = alpha.clamp(0.0, 1.0);
        self.text = None;
        self
    }

    /// Port of `COLOR4D::SetFromWxString` (`wxColour::Set( const wxString& )`): accepts the CSS
    /// forms `rgb(r, g, b)` and `rgba(r, g, b, a)` and `#RRGGBB`.
    ///
    /// wx colour names (`wxTheColourDatabase`, "red", "LIGHT BLUE", ...) are not supported,
    /// see `PORT_STATUS.md`.
    pub fn set_from_wx_string(&mut self, color_string: &str) -> bool {
        match wx_colour_from_string(color_string) {
            Some(c) => {
                self.r = c.red as f64 / 255.0;
                self.g = c.green as f64 / 255.0;
                self.b = c.blue as f64 / 255.0;
                self.a = c.alpha as f64 / 255.0;
                self.text = None;
                true
            }
            None => false,
        }
    }

    /// Port of `COLOR4D::ToCSSString`: `rgb(r, g, b)` or `rgba(r, g, b, a)` with three
    /// decimals of alpha.
    pub fn to_css_string(&self) -> String {
        let c = self.to_colour();

        if c.alpha == WX_ALPHA_OPAQUE {
            format!("rgb({}, {}, {})", c.red, c.green, c.blue)
        } else {
            // The wxC2S_CSS_SYNTAX is particularly sensitive to ','s (as it uses them for
            // value delimiters), and wxWidgets is known to be buggy in this respect when
            // dealing with Serbian and Russian locales (at least), so we enforce an extra
            // level of safety.
            let alpha_str = format_f(c.alpha as f64 / 255.0, 3).replace(',', ".");

            format!("rgba({}, {}, {}, {})", c.red, c.green, c.blue, alpha_str)
        }
    }

    /// Port of `COLOR4D::SetFromHexString`: `#RRGGBB` or `#RRGGBBAA` (after trimming).
    pub fn set_from_hex_string(&mut self, color_string: &str) -> bool {
        let str_: Vec<char> = wxstr::strip_both(color_string).chars().collect();

        if str_.len() < 7 || str_[0] != '#' {
            return false;
        }

        let mut scanner = Scanner {
            s: &str_[1..],
            pos: 0,
        };
        let Some(tmp) = scanner.long_x() else {
            return false;
        };

        if str_.len() >= 9 {
            self.r = ((tmp >> 24) & 0xFF) as f64 / 255.0;
            self.g = ((tmp >> 16) & 0xFF) as f64 / 255.0;
            self.b = ((tmp >> 8) & 0xFF) as f64 / 255.0;
            self.a = (tmp & 0xFF) as f64 / 255.0;
        } else {
            self.r = ((tmp >> 16) & 0xFF) as f64 / 255.0;
            self.g = ((tmp >> 8) & 0xFF) as f64 / 255.0;
            self.b = (tmp & 0xFF) as f64 / 255.0;
            self.a = 1.0;
        }

        self.text = None;
        true
    }

    /// Port of `COLOR4D::ToHexString`: `#RRGGBBAA`.
    pub fn to_hex_string(&self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            wxstr::ki_round_i32(self.r * 255.0),
            wxstr::ki_round_i32(self.g * 255.0),
            wxstr::ki_round_i32(self.b * 255.0),
            wxstr::ki_round_i32(self.a * 255.0)
        )
    }

    /// Port of `COLOR4D::ToColour`: 8 bit channels, `(unsigned char)( c * 255 + 0.5 )`
    /// (saturating instead of undefined behaviour outside 0 .. 1).
    pub fn to_colour(&self) -> WxColour {
        WxColour {
            red: (self.r * 255.0 + 0.5) as u8,
            green: (self.g * 255.0 + 0.5) as u8,
            blue: (self.b * 255.0 + 0.5) as u8,
            alpha: (self.a * 255.0 + 0.5) as u8,
        }
    }

    /// Port of `COLOR4D::LegacyMix`: OR the 8 bit RGB values, average the alphas.
    pub fn legacy_mix(&self, color: &Color4D) -> Color4D {
        let mix = |x: f64, y: f64| ((255.0 * x) as u32 | (255.0 * y) as u32) as f64 / 255.0;

        Color4D::new(
            mix(self.r, color.r),
            mix(self.g, color.g),
            mix(self.b, color.b),
            // the alpha channel can be reinitialized but what is the best value?
            (color.a + self.a) / 2.0,
        )
    }

    /// Port of `COLOR4D::ToHSL`: returns `(hue, saturation, lightness)`.
    pub fn to_hsl(&self) -> (f64, f64, f64) {
        let (r, g, b) = (self.r, self.g, self.b);
        let min = std_min(r, std_min(g, b));
        let max = std_max(r, std_max(g, b));
        let diff = max - min;

        let lightness = (max + min) / 2.0;

        let saturation = if lightness == 0.0 || lightness >= 1.0 {
            0.0
        } else {
            diff / (1.0 - (2.0 * lightness - 1.0).abs())
        };

        let hue = if diff <= 0.0 {
            0.0
        } else if max == r {
            (g - b) / diff
        } else if max == g {
            (b - r) / diff + 2.0
        } else {
            (r - g) / diff + 4.0
        };

        let mut out_hue = if hue > 0.0 {
            hue * 60.0
        } else {
            hue * 60.0 + 360.0
        };

        while out_hue < 0.0 {
            out_hue += 360.0;
        }

        (out_hue, saturation, lightness)
    }

    /// Port of `COLOR4D::FromHSL`.
    pub fn from_hsl(&mut self, in_hue: f64, in_saturation: f64, in_lightness: f64) {
        let p = (1.0 - (2.0 * in_lightness - 1.0).abs()) * in_saturation;
        let scaled_hue = in_hue / 60.0;
        let q = p * (1.0 - ((scaled_hue % 2.0) - 1.0).abs());

        let base = in_lightness - p / 2.0;
        self.r = base;
        self.g = base;
        self.b = base;

        if scaled_hue < 1.0 {
            self.r += p;
            self.g += q;
        } else if scaled_hue < 2.0 {
            self.r += q;
            self.g += p;
        } else if scaled_hue < 3.0 {
            self.g += p;
            self.b += q;
        } else if scaled_hue < 4.0 {
            self.g += q;
            self.b += p;
        } else if scaled_hue < 5.0 {
            self.r += q;
            self.b += p;
        } else {
            self.r += p;
            self.b += q;
        }

        self.text = None;
    }

    /// Port of `COLOR4D::ToHSV`: returns `(hue, saturation, value)`.  The hue of a grey is
    /// NaN unless `always_define_hue`, then 0.
    pub fn to_hsv(&self, always_define_hue: bool) -> (f64, f64, f64) {
        let (r, g, b) = (self.r, self.g, self.b);

        let mut min = if r < g { r } else { g };
        min = if min < b { min } else { b };

        let mut max = if r > g { r } else { g };
        max = if max > b { max } else { b };

        let out_value = max; // value
        let delta = max - min;

        let out_saturation = if max > 0.0 {
            delta / max
        } else {
            // for black color (r = g = b = 0 )  saturation is set to 0.
            return (
                if always_define_hue { 0.0 } else { f64::NAN },
                0.0,
                out_value,
            );
        };

        // Hue in degrees (0...360) is coded according to this table
        // 0 or 360 : red, 60 : yellow, 120 : green, 180 : cyan, 240 : blue, 300 : magenta
        let out_hue = if delta != 0.0 {
            let mut h = if r >= max {
                (g - b) / delta // between yellow & magenta
            } else if g >= max {
                2.0 + (b - r) / delta // between cyan & yellow
            } else {
                4.0 + (r - g) / delta // between magenta & cyan
            };

            h *= 60.0; // degrees

            if h < 0.0 {
                h += 360.0;
            }

            h
        } else {
            // delta = 0 means r = g = b. hue is set to 0.0
            if always_define_hue { 0.0 } else { f64::NAN }
        };

        (out_hue, out_saturation, out_value)
    }

    /// Port of `COLOR4D::FromHSV`.
    pub fn from_hsv(&mut self, in_h: f64, in_s: f64, in_v: f64) {
        if in_s <= 0.0 {
            self.r = in_v;
            self.g = in_v;
            self.b = in_v;
            return;
        }

        let mut hh = in_h;

        while hh >= 360.0 {
            hh -= 360.0;
        }

        hh /= 60.0;

        let i = hh as i32;
        let ff = hh - i as f64;

        let p = in_v * (1.0 - in_s);
        let q = in_v * (1.0 - (in_s * ff));
        let t = in_v * (1.0 - (in_s * (1.0 - ff)));

        let (r, g, b) = match i {
            0 => (in_v, t, p),
            1 => (q, in_v, p),
            2 => (p, in_v, t),
            3 => (p, q, in_v),
            4 => (t, p, in_v),
            _ => (in_v, p, q),
        };

        self.r = r;
        self.g = g;
        self.b = b;
        self.text = None;
    }

    /// Port of `COLOR4D::Saturate`.
    pub fn saturate(&mut self, factor: f64) -> &mut Self {
        // One can saturate a color only when r, v, b are not equal
        if self.r == self.g && self.r == self.b {
            return self;
        }

        let (h, _s, _v) = self.to_hsv(true);
        self.from_hsv(h, factor, 1.0);
        self
    }

    /// Port of `COLOR4D::Desaturate`.
    pub fn desaturate(&mut self) -> &mut Self {
        // One can desaturate a color only when r, v, b are not equal
        if self.r == self.g && self.r == self.b {
            return self;
        }

        let (h, _s, l) = self.to_hsl();
        self.from_hsl(h, 0.0, l);
        self
    }

    /// Port of `COLOR4D::Brighten`.
    pub fn brighten(&mut self, factor: f64) -> &mut Self {
        self.r = self.r * (1.0 - factor) + factor;
        self.g = self.g * (1.0 - factor) + factor;
        self.b = self.b * (1.0 - factor) + factor;
        self
    }

    /// Port of `COLOR4D::Darken`.
    pub fn darken(&mut self, factor: f64) -> &mut Self {
        self.r *= 1.0 - factor;
        self.g *= 1.0 - factor;
        self.b *= 1.0 - factor;
        self
    }

    /// Port of `COLOR4D::Invert`.
    pub fn invert(&mut self) -> &mut Self {
        self.r = 1.0 - self.r;
        self.g = 1.0 - self.g;
        self.b = 1.0 - self.b;
        self
    }

    /// Port of `COLOR4D::Brightened` (the text is not kept, like the C++ constructor).
    pub fn brightened(&self, factor: f64) -> Color4D {
        Color4D::new(
            self.r * (1.0 - factor) + factor,
            self.g * (1.0 - factor) + factor,
            self.b * (1.0 - factor) + factor,
            self.a,
        )
    }

    /// Port of `COLOR4D::Darkened`.
    pub fn darkened(&self, factor: f64) -> Color4D {
        Color4D::new(
            self.r * (1.0 - factor),
            self.g * (1.0 - factor),
            self.b * (1.0 - factor),
            self.a,
        )
    }

    /// Port of `COLOR4D::Mix`: `color * ( 1 - factor ) + this * factor`, keeping this alpha.
    pub fn mix(&self, color: &Color4D, factor: f64) -> Color4D {
        Color4D::new(
            color.r * (1.0 - factor) + self.r * factor,
            color.g * (1.0 - factor) + self.g * factor,
            color.b * (1.0 - factor) + self.b * factor,
            self.a,
        )
    }

    /// Port of `COLOR4D::WithAlpha`.
    pub fn with_alpha(&self, alpha: f64) -> Color4D {
        Color4D::new(self.r, self.g, self.b, alpha)
    }

    /// Port of `COLOR4D::Inverted`.
    pub fn inverted(&self) -> Color4D {
        Color4D::new(1.0 - self.r, 1.0 - self.g, 1.0 - self.b, self.a)
    }

    /// Port of `COLOR4D::GetBrightness` (weighted W3C formula).
    pub fn brightness(&self) -> f64 {
        self.r * 0.299 + self.g * 0.587 + self.b * 0.117
    }

    /// Port of `COLOR4D::Distance`: squared RGB distance.
    pub fn distance(&self, other: &Color4D) -> f64 {
        (self.r - other.r) * (self.r - other.r)
            + (self.g - other.g) * (self.g - other.g)
            + (self.b - other.b) * (self.b - other.b)
    }

    /// Port of `COLOR4D::Compare`: -1, 0 or 1.  Colours with a text compare by text and never
    /// compare equal (C++ returns -1 or 1).
    pub fn compare(&self, rhs: &Color4D) -> i32 {
        if self.text.is_some() || rhs.text.is_some() {
            let l = self.text.as_deref().unwrap_or("");
            let r = rhs.text.as_deref().unwrap_or("");

            return if l < r { -1 } else { 1 };
        }

        if self.r != rhs.r {
            return if self.r < rhs.r { -1 } else { 1 };
        }

        if self.g != rhs.g {
            return if self.g < rhs.g { -1 } else { 1 };
        }

        if self.b != rhs.b {
            return if self.b < rhs.b { -1 } else { 1 };
        }

        if self.a != rhs.a {
            return if self.a < rhs.a { -1 } else { 1 };
        }

        0
    }

    /// Port of `operator<( const COLOR4D&, const COLOR4D& )`.
    pub fn less_than(&self, rhs: &Color4D) -> bool {
        if self.text.is_some() || rhs.text.is_some() {
            return self.text.as_deref().unwrap_or("") < rhs.text.as_deref().unwrap_or("");
        }

        if self.r != rhs.r {
            return self.r < rhs.r;
        }

        if self.g != rhs.g {
            return self.g < rhs.g;
        }

        if self.b != rhs.b {
            return self.b < rhs.b;
        }

        self.a < rhs.a
    }

    /// Port of `COLOR4D::RelativeLuminance`
    /// (<https://www.w3.org/TR/WCAG21/#dfn-relative-luminance>).
    pub fn relative_luminance(&self) -> f64 {
        let lin = |c: f64| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };

        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// Port of `COLOR4D::ContrastRatio` (<https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio>).
    pub fn contrast_ratio(left: &Color4D, right: &Color4D) -> f64 {
        let a_rl = left.relative_luminance();
        let b_rl = right.relative_luminance();

        if a_rl > b_rl {
            (a_rl + 0.05) / (b_rl + 0.05)
        } else {
            (b_rl + 0.05) / (a_rl + 0.05)
        }
    }

    /// Port of `COLOR4D::FindNearestLegacyColor`: the legacy colour whose components are all
    /// at least the given ones and nearest to them.
    ///
    /// Like the C++ the palette is indexed with the `EDA_COLOR_T` value, although the table
    /// is not in enum order.
    pub fn find_nearest_legacy_color(r: i32, g: i32, b: i32) -> EdaColorT {
        let mut candidate = EdaColorT::BLACK;

        // Find the 'nearest' color in the palette. This is fun. There is a gazilion of
        // metrics for the color space and no one of the useful one is in the RGB color space.
        // Who cares, this is a CAD, not a photosomething...
        //
        // I hereby declare that the distance is the sum of the square of the component
        // difference. Think about the RGB color cube. Now get the euclidean distance, but
        // without the square root... for ordering purposes it's the same, obviously. Also
        // each component can't be less of the target one, since I found this currently work
        // better...
        let mut nearest_distance = 255 * 255 * 3 + 1; // Can't beat this

        for trying in 0..EdaColorT::NBCOLORS.0 {
            let c = &COLOR_REFS[trying as usize];
            let (cr, cg, cb) = (c.red as i32, c.green as i32, c.blue as i32);
            let distance = (r - cr) * (r - cr) + (g - cg) * (g - cg) + (b - cb) * (b - cb);

            if distance < nearest_distance && cr >= r && cg >= g && cb >= b {
                nearest_distance = distance;
                candidate = EdaColorT(trying);
            }
        }

        candidate
    }

    /// Port of `to_json( nlohmann::json&, const COLOR4D& )`: the JSON string value.
    pub fn to_json_string(&self) -> String {
        match &self.text {
            Some(t) => t.clone(),
            None => self.to_css_string(),
        }
    }

    /// Port of `from_json( const nlohmann::json&, COLOR4D& )`: [`Color4D::set_from_wx_string`].
    pub fn from_json_string(&mut self, value: &str) {
        self.set_from_wx_string(value);
    }
}

/// Port of `operator==( const COLOR4D&, const COLOR4D& )`.
impl PartialEq for Color4D {
    fn eq(&self, rhs: &Self) -> bool {
        if self.text.is_some() || rhs.text.is_some() {
            return self.text.as_deref().unwrap_or("") == rhs.text.as_deref().unwrap_or("");
        }

        self.a == rhs.a && self.r == rhs.r && self.g == rhs.g && self.b == rhs.b
    }
}

/// Ordering by `operator<`.
impl PartialOrd for Color4D {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;

        if self.less_than(other) {
            Some(Ordering::Less)
        } else if other.less_than(self) {
            Some(Ordering::Greater)
        } else if self == other {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
}

/// `std::hash<COLOR4D>` hashes `r, b, g, a`; this hash uses the same components (with the
/// libstdc++ rule that 0.0 and -0.0 hash alike) but not the same function.
impl std::hash::Hash for Color4D {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for v in [self.r, self.b, self.g, self.a] {
            (if v == 0.0 { 0u64 } else { v.to_bits() }).hash(state);
        }
    }
}

/// Port of `operator<<( std::ostream&, const COLOR4D& )`: the text, or the CSS string.
impl fmt::Display for Color4D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.text {
            Some(t) => f.write_str(t),
            None => f.write_str(&self.to_css_string()),
        }
    }
}

impl From<EdaColorT> for Color4D {
    fn from(c: EdaColorT) -> Self {
        Color4D::from_eda_color(c)
    }
}

/// Port of `wxColour::Set( const wxString& )` / `wxColour::FromString` of wxGTK 3.2 (the
/// reference platform): GTK's `gdk_rgba_parse` first, then `wxColourBase::FromString`.
///
/// Colour names (pango's X11 table and `wxTheColourDatabase`) are not supported, see
/// `PORT_STATUS.md`.
pub fn wx_colour_from_string(str_: &str) -> Option<WxColour> {
    if let Some((r, g, b, a)) = gdk_rgba_parse(str_) {
        // wxColour( const GdkRGBA& ): the channels go through 16 bit GdkColor values
        let q = |d: f64| (((d * 65535.0 + 0.5) as u32 as u16) >> 8) as u8;

        return Some(WxColour {
            red: q(r),
            green: q(g),
            blue: q(b),
            alpha: q(a),
        });
    }

    wx_colour_base_from_string(str_)
}

/// `g_ascii_isspace`
fn g_ascii_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'\x0b')
}

/// `g_ascii_strtod` at `pos`: the value, the end position and whether `errno` is `ERANGE`.
fn g_ascii_strtod(s: &str, pos: usize) -> (f64, usize, bool) {
    let r = wxstr::strtod(&s[pos..]);

    (r.value, pos + r.consumed, r.erange)
}

/// Port of GTK 3's `gdk_rgba_parse` (`gdk/gdkrgba.c`): `rgb(r,g,b)` / `rgba(r,g,b,a)` with
/// numbers or percentages, and pango's `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`.
/// Returns the clamped channels in 0 .. 1.
fn gdk_rgba_parse(spec: &str) -> Option<(f64, f64, f64, f64)> {
    let bytes = spec.as_bytes();
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let skip_ws = |mut i: usize| {
        while g_ascii_isspace(at(i)) {
            i += 1;
        }

        i
    };

    let (has_alpha, mut i) = if spec.starts_with("rgba") {
        (true, 4)
    } else if spec.starts_with("rgb") {
        (false, 3)
    } else {
        // Resort on PangoColor for rgb.txt color map and '#' prefixed colors
        let (r, g, b) = pango_color_parse(spec)?;

        return Some((r as f64 / 65535., g as f64 / 65535., b as f64 / 65535., 1.0));
    };

    // parse_rgb_value
    let parse_rgb_value = |i: usize| -> Option<(f64, usize)> {
        let (number, end, erange) = g_ascii_strtod(spec, i);

        if erange || end == i || number.is_infinite() || number.is_nan() {
            return None;
        }

        let p = skip_ws(end);
        let clamp01 = |x: f64| x.clamp(0.0, 1.0);

        if at(p) == b'%' {
            Some((clamp01(number / 100.), p + 1))
        } else {
            Some((clamp01(number / 255.), end))
        }
    };

    i = skip_ws(i);

    if at(i) != b'(' {
        return None;
    }

    let mut channels = [0.0; 3];

    for (n, ch) in channels.iter_mut().enumerate() {
        i += 1; // past '(' or ','
        i = skip_ws(i);

        let (v, end) = parse_rgb_value(i)?;
        *ch = v;
        i = skip_ws(end);

        if n < 2 && at(i) != b',' {
            return None;
        }
    }

    let mut a = 1.0;

    if has_alpha {
        if at(i) != b',' {
            return None;
        }

        i = skip_ws(i + 1);

        let (v, end, erange) = g_ascii_strtod(spec, i);

        if erange {
            return None;
        }

        a = v;
        i = skip_ws(end);
    }

    if at(i) != b')' {
        return None;
    }

    i = skip_ws(i + 1);

    if i != bytes.len() {
        return None;
    }

    // CLAMP( x, 0, 1 ): NaN stays NaN
    let clamp = |x: f64| x.clamp(0.0, 1.0);

    Some((channels[0], channels[1], channels[2], clamp(a)))
}

/// Port of `pango_color_parse` for `#` colours (3, 6, 9 or 12 hex digits): 16 bit channels.
/// The X11 colour name table is not ported.
fn pango_color_parse(spec: &str) -> Option<(u32, u32, u32)> {
    let hex = spec.strip_prefix('#')?;
    let len = hex.len();

    if len % 3 != 0 || !(3..=12).contains(&len) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let n = len / 3;
    let parse = |k: usize| u32::from_str_radix(&hex[k * n..(k + 1) * n], 16).unwrap_or(0);
    let (mut r, mut g, mut b) = (parse(0), parse(1), parse(2));
    let mut bits = n as u32 * 4;

    r <<= 16 - bits;
    g <<= 16 - bits;
    b <<= 16 - bits;

    while bits < 16 {
        r |= r >> bits;
        g |= g >> bits;
        b |= b >> bits;
        bits *= 2;
    }

    Some((r, g, b))
}

/// Port of `wxColourBase::FromString` (wxWidgets 3.2, `src/common/colourcmn.cpp`) without
/// the colour database: `rgb(...)`, `rgba(...)` (case-insensitive prefix, `sscanf` parsing)
/// and `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`.
fn wx_colour_base_from_string(str_: &str) -> Option<WxColour> {
    let chars: Vec<char> = str_.chars().collect();

    if chars.is_empty() {
        return None; // invalid or empty string
    }

    let clip = |v: i32| v.clamp(0, 255) as u8;

    if chars.len() >= 3 && wxstr::is_same_as_no_case(&chars[..3].iter().collect::<String>(), "rgb")
    {
        // CSS-like RGB specification; according to http://www.w3.org/TR/css3-color/#colorunits
        // values outside 0-255 range are allowed but should be clipped
        let has_alpha = chars.len() > 3 && (chars[3] == 'a' || chars[3] == 'A');
        let mut sc = Scanner {
            s: &chars[if has_alpha { 4 } else { 3 }..],
            pos: 0,
        };

        // "( %d , %d , %d" [ " , %255[^)]" ] " )"
        let mut ints = [0i32; 3];

        if !sc.literal('(') {
            return None;
        }

        for (i, slot) in ints.iter_mut().enumerate() {
            if i > 0 {
                sc.skip_ws();

                if !sc.literal(',') {
                    return None;
                }
            }

            *slot = sc.int_d()?;
        }

        let mut alpha = WX_ALPHA_OPAQUE as i32;

        if has_alpha {
            sc.skip_ws();

            if !sc.literal(',') {
                return None;
            }

            sc.skip_ws();

            let start = sc.pos;

            while sc.pos < sc.s.len() && sc.s[sc.pos] != ')' && sc.pos - start < 255 {
                sc.pos += 1;
            }

            if sc.pos == start {
                return None;
            }

            let alpha_str: String = sc.s[start..sc.pos].iter().collect();
            let (value, complete) = wxstr::to_double(&alpha_str);

            if !complete {
                return None;
            }

            alpha = wx_round(value.unwrap_or(0.0) * 255.0);
        }

        // the closing ")" is not checked: sscanf has already counted its conversions
        Some(WxColour {
            red: clip(ints[0]),
            green: clip(ints[1]),
            blue: clip(ints[2]),
            alpha: clip(alpha),
        })
    } else if chars[0] == '#' {
        // hexadecimal prefixed with # ("#RGB", "#RGBA", "#RRGGBB" or "#RRGGBBAA")
        let mut sc = Scanner {
            s: &chars[1..],
            pos: 0,
        };
        let tmp = sc.long_x()?;
        let nib = |shift: u32| (((tmp >> shift) & 0xF) * 0x11) as u8;

        match chars.len() {
            4 => Some(WxColour {
                red: nib(8),
                green: nib(4),
                blue: nib(0),
                alpha: WX_ALPHA_OPAQUE,
            }),
            5 => Some(WxColour {
                red: nib(12),
                green: nib(8),
                blue: nib(4),
                alpha: nib(0),
            }),
            7 => Some(WxColour {
                red: (tmp >> 16) as u8,
                green: (tmp >> 8) as u8,
                blue: tmp as u8,
                alpha: WX_ALPHA_OPAQUE,
            }),
            9 => Some(WxColour {
                red: (tmp >> 24) as u8,
                green: (tmp >> 16) as u8,
                blue: (tmp >> 8) as u8,
                alpha: tmp as u8,
            }),
            _ => None,
        }
    } else {
        // a colour name: wxTheColourDatabase is not ported
        None
    }
}
