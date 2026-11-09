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

//! Port of `GetDefaultPlotExtension` (`common/plotters/common_plot_functions.cpp` L34-46) and
//! of the `GetDefaultFileExtension()` statics of the plotter classes it calls.
//!
//! `PlotDrawingSheet` (the rest of the file) is the tier 3 package PLOT-08.

use kicad_core::plot_enums::PlotFormat;

/// Port of `GetDefaultPlotExtension`: the default plot extension for a format.
///
/// HPGL and `UNDEFINED` hit `wxFAIL` in C++ and return an empty string.
pub fn get_default_plot_extension(a_format: PlotFormat) -> &'static str {
    match a_format {
        PlotFormat::Dxf => "dxf",    // DXF_PLOTTER::GetDefaultFileExtension
        PlotFormat::Post => "ps",    // PS_PLOTTER::GetDefaultFileExtension
        PlotFormat::Pdf => "pdf",    // PDF_PLOTTER::GetDefaultFileExtension
        PlotFormat::Gerber => "gbr", // GERBER_PLOTTER::GetDefaultFileExtension
        PlotFormat::Svg => "svg",    // SVG_PLOTTER::GetDefaultFileExtension
        PlotFormat::Png => "png",    // PNG_PLOTTER::GetDefaultFileExtension
        PlotFormat::Hpgl | PlotFormat::Undefined => "",
    }
}
