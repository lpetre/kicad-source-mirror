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

//! Port of the option part of `common/jobs/job_export_pcb_svg.h` / `.cpp`
//! (`JOB_EXPORT_PCB_SVG`): members and constructor defaults (work package FAB-11).
//!
//! The full job (JSON, registry) is `kicad_session::jobs::JobExportPcbSvg`; the session maps
//! it onto this struct (see [`super::job_export_pcb_plot`]).

use super::job_export_pcb_plot::{JobExportPcbPlot, JobPlotFormat};

/// `SVG_PRECISION_DEFAULT`
pub const SVG_PRECISION_DEFAULT: u32 = 4;

/// Port of `JOB_EXPORT_PCB_SVG::GEN_MODE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SvgGenMode {
    /// `SINGLE`: the job default (`TODO change to MULTI for V10` in the C++).
    #[default]
    Single,
    /// `MULTI`
    Multi,
}

/// The options of `JOB_EXPORT_PCB_SVG`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbSvg {
    /// The `JOB_EXPORT_PCB_PLOT` base.
    pub plot: JobExportPcbPlot,
    /// `m_fitPageToBoard`
    pub fit_page_to_board: bool,
    /// `m_precision`
    pub precision: u32,
    /// `m_genMode`
    pub gen_mode: SvgGenMode,
}

impl JobExportPcbSvg {
    /// Port of `JOB_EXPORT_PCB_SVG::JOB_EXPORT_PCB_SVG()`.
    pub fn new() -> Self {
        let mut plot = JobExportPcbPlot::new(JobPlotFormat::Svg);
        plot.plot_drawing_sheet = true;

        JobExportPcbSvg {
            plot,
            fit_page_to_board: false,
            precision: SVG_PRECISION_DEFAULT,
            gen_mode: SvgGenMode::Single,
        }
    }
}

impl Default for JobExportPcbSvg {
    fn default() -> Self {
        JobExportPcbSvg::new()
    }
}
