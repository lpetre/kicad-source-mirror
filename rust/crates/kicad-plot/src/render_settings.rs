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

//! Port of the part of `include/render_settings.h` / `common/render_settings.cpp`
//! (`KIGFX::RENDER_SETTINGS`) that the plot engines read: default and minimum pen width and
//! the dash / dot / gap lengths of dashed lines.
//!
//! The C++ class is an abstract base of the GAL painters' settings (colours per layer,
//! highlighting, ...). The plotters only need the members below; the board plot code fills
//! them from `PCB_PLOT_PARAMS` (dash and gap ratios).

use kicad_geom::std_max;

/// Port of the line-length correction constant of `render_settings.cpp`: matches ISO 128-2,
/// but can create issues on GTK and MSW: "dots" are not always visible depending on the zoom
/// level because they create 0 length lines, so they will be drawn as segments, even with
/// correction = 1.0.
const CORRECTION: f64 = 1.0;

/// Subset of `KIGFX::RENDER_SETTINGS` used by the plot engines.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSettings {
    /// `m_defaultPenWidth`
    pub default_pen_width: i32,
    /// `m_minPenWidth`: some clients (such as PDF) don't like ultra-thin lines
    pub min_pen_width: i32,
    /// `m_dashLengthRatio` (from ISO 128-2)
    pub dash_length_ratio: f64,
    /// `m_gapLengthRatio` (from ISO 128-2)
    pub gap_length_ratio: f64,
    /// `m_defaultFont`: the font name used by `PLOTTER::Text` / `PlotText` when no font is
    /// given (empty = the stroke font).
    pub default_font: String,
}

impl Default for RenderSettings {
    /// Port of `RENDER_SETTINGS::RENDER_SETTINGS()` (the members kept here).
    fn default() -> Self {
        RenderSettings {
            default_pen_width: 0,
            min_pen_width: 0,
            dash_length_ratio: 12.0,
            gap_length_ratio: 3.0,
            default_font: String::new(),
        }
    }
}

impl RenderSettings {
    /// Port of `RENDER_SETTINGS::GetDefaultPenWidth`.
    pub fn default_pen_width(&self) -> i32 {
        self.default_pen_width
    }

    /// Port of `RENDER_SETTINGS::SetDefaultPenWidth`.
    pub fn set_default_pen_width(&mut self, a_width: i32) {
        self.default_pen_width = a_width;
    }

    /// Port of `RENDER_SETTINGS::GetMinPenWidth`.
    pub fn min_pen_width(&self) -> i32 {
        self.min_pen_width
    }

    /// Port of `RENDER_SETTINGS::SetMinPenWidth`.
    pub fn set_min_pen_width(&mut self, a_width: i32) {
        self.min_pen_width = a_width;
    }

    /// Port of `RENDER_SETTINGS::GetDashLengthRatio`.
    pub fn dash_length_ratio(&self) -> f64 {
        self.dash_length_ratio
    }

    /// Port of `RENDER_SETTINGS::SetDashLengthRatio`.
    pub fn set_dash_length_ratio(&mut self, a_ratio: f64) {
        self.dash_length_ratio = a_ratio;
    }

    /// Port of `RENDER_SETTINGS::GetGapLengthRatio`.
    pub fn gap_length_ratio(&self) -> f64 {
        self.gap_length_ratio
    }

    /// Port of `RENDER_SETTINGS::SetGapLengthRatio`.
    pub fn set_gap_length_ratio(&mut self, a_ratio: f64) {
        self.gap_length_ratio = a_ratio;
    }

    /// Port of `RENDER_SETTINGS::GetDefaultFont`.
    pub fn default_font(&self) -> &str {
        &self.default_font
    }

    /// Port of `RENDER_SETTINGS::SetDefaultFont`.
    pub fn set_default_font(&mut self, a_font: &str) {
        self.default_font = a_font.to_string();
    }

    /// Port of `RENDER_SETTINGS::GetDashLength`.
    pub fn dash_length(&self, a_line_width: i32) -> f64 {
        std_max(self.dash_length_ratio - CORRECTION, 1.0) * a_line_width as f64
    }

    /// Port of `RENDER_SETTINGS::GetDotLength`. The minimal length scale is arbitrary set to
    /// 0.2 after trials; 0 length can create drawing issues.
    pub fn dot_length(&self, a_line_width: i32) -> f64 {
        std_max(1.0 - CORRECTION, 0.2) * a_line_width as f64
    }

    /// Port of `RENDER_SETTINGS::GetGapLength`.
    pub fn gap_length(&self, a_line_width: i32) -> f64 {
        std_max(self.gap_length_ratio + CORRECTION, 1.0) * a_line_width as f64
    }
}
