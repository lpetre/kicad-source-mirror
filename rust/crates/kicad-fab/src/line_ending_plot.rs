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

//! Port of `common/line_ending.cpp` L240-265 (`LINE_ENDING::Plot`).
//!
//! `LINE_ENDING::GetShapes` (L122-190) and the size rules are owned by the board model
//! (`kicad_pcb::line_ending::LineEnding::shapes`); the plotting half lives here because the
//! model crate cannot depend on the plotters.

use kicad_core::plot_enums::FillT;
use kicad_geom::{EdaAngle, Vector2I};
use kicad_pcb::line_ending::{LineEnding, LineEndingStyle};
use kicad_plot::Plotter;
use kicad_plot::plotter::PlotData;

/// Port of `LINE_ENDING::Plot( aPlotter, aPoint, aTangent, aLineWidth, aData )`: plot the
/// ending at `a_point`, pointing along `a_tangent` (away from the body). An open arrow is a
/// polyline with the ending's own stroke width (or the line width); the other styles are
/// filled polygons, outlined with the ending's stroke width when it has one.
pub fn line_ending_plot(
    a_ending: &LineEnding,
    a_plotter: &mut dyn Plotter,
    a_point: Vector2I,
    a_tangent: EdaAngle,
    a_line_width: i32,
    a_data: PlotData,
) {
    if a_ending.style() == LineEndingStyle::None {
        return;
    }

    let polygon = a_ending.shapes(a_point, a_tangent, a_line_width);

    if polygon.is_empty() {
        return;
    }

    if a_ending.style() == LineEndingStyle::ArrowOpen {
        let plot_width = if a_ending.stroke_width() > 0 {
            a_ending.stroke_width()
        } else {
            a_line_width
        };
        a_plotter.plot_poly(&polygon, FillT::NoFill, plot_width, a_data);
    } else if a_ending.stroke_width() > 0 {
        a_plotter.plot_poly(&polygon, FillT::FilledShape, a_ending.stroke_width(), a_data);
    } else {
        a_plotter.plot_poly(&polygon, FillT::FilledShape, 0, a_data);
    }
}
