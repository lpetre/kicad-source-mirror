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

//! Port of `include/font/font_metrics.h` (`KIFONT::METRICS`) and `METRICS::Default()` from
//! `common/font/font.cpp`.

/// Port of `KIFONT::METRICS`: font-independent vertical metrics, as multiples of the glyph
/// height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// `m_InterlinePitch`
    pub interline_pitch: f64,
    /// `m_OverbarHeight`
    pub overbar_height: f64,
    /// `m_UnderlineOffset`
    pub underline_offset: f64,
}

impl Metrics {
    /// The default metrics (`g_defaultMetrics`, returned by `METRICS::Default()`).
    pub const DEFAULT: Metrics = Metrics {
        interline_pitch: 1.68,
        overbar_height: 1.23,
        underline_offset: -0.16,
    };

    /// Port of `METRICS::Default()`.
    pub fn default_metrics() -> &'static Metrics {
        &Self::DEFAULT
    }

    /// Port of `METRICS::GetOverbarVerticalPosition`: the distance between the text baseline
    /// and the overbar.
    pub fn overbar_vertical_position(&self, glyph_height: f64) -> f64 {
        glyph_height * self.overbar_height
    }

    /// Port of `METRICS::GetUnderlineVerticalPosition`: the distance between the text baseline
    /// and the underline.
    pub fn underline_vertical_position(&self, glyph_height: f64) -> f64 {
        glyph_height * self.underline_offset
    }

    /// Port of `METRICS::GetInterline`.
    pub fn interline(&self, font_height: f64) -> f64 {
        font_height * self.interline_pitch
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::DEFAULT
    }
}
