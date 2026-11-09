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

//! Port of KiCad's plotters: PLOTTER base, Gerber (RS-274X/X2) and SVG.
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md`,
//! `rust/docs/design/plot-engines.md` and `PORT_STATUS.md`.

pub mod common_plot_functions;
pub mod drill_markers;
pub mod gbr_metadata;
pub mod gbr_netlist_metadata;
pub mod gbr_plotter_apertures;
pub mod plotter;
pub mod render_settings;

pub use kicad_core::plot_enums::{
    DrillMarks, DxfLayerOutputMode, DxfOutlineMode, DxfUnits, FillT, PlotFormat, PlotTextMode,
};
pub use plotter::{DO_NOT_SET_LINE_WIDTH, PlotData, Plotter, PlotterState, USE_DEFAULT_LINE_WIDTH};
pub use render_settings::RenderSettings;
