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

//! Port of the option part of `common/jobs/job_export_pcb_gerber.h` / `.cpp`
//! (`JOB_EXPORT_PCB_GERBER`) and `common/jobs/job_export_pcb_gerbers.h` / `.cpp`
//! (`JOB_EXPORT_PCB_GERBERS`): members and constructor defaults (work package FAB-07).
//!
//! The full jobs are `kicad_session::jobs::{JobExportPcbGerber, JobExportPcbGerbers}`; the
//! session maps them onto these (see [`super::job_export_pcb_plot`]).

use super::job_export_pcb_plot::{JobExportPcbPlot, JobPlotFormat};

/// The options of `JOB_EXPORT_PCB_GERBER`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbGerber {
    /// The `JOB_EXPORT_PCB_PLOT` base.
    pub plot: JobExportPcbPlot,
    /// `m_includeNetlistAttributes`
    pub include_netlist_attributes: bool,
    /// `m_useX2Format`
    pub use_x2_format: bool,
    /// `m_disableApertureMacros`
    pub disable_aperture_macros: bool,
    /// `m_useProtelFileExtension`
    pub use_protel_file_extension: bool,
    /// `m_precision` (job default 5; `kicad-cli` always sets it, default 6)
    pub precision: i32,
}

impl Default for JobExportPcbGerber {
    /// Port of `JOB_EXPORT_PCB_GERBER::JOB_EXPORT_PCB_GERBER( aType )`: the plot base with
    /// `m_plotDrawingSheet = false`.
    fn default() -> Self {
        let mut plot = JobExportPcbPlot::new(JobPlotFormat::Gerber);
        plot.plot_drawing_sheet = false;

        JobExportPcbGerber {
            plot,
            include_netlist_attributes: true,
            use_x2_format: true,
            disable_aperture_macros: false,
            use_protel_file_extension: true,
            precision: 5,
        }
    }
}

/// The options of `JOB_EXPORT_PCB_GERBERS`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbGerbers {
    /// The `JOB_EXPORT_PCB_GERBER` base.
    pub gerber: JobExportPcbGerber,
    /// `m_useBoardPlotParams`
    pub use_board_plot_params: bool,
    /// `m_createJobsFile` (no CLI flag; not taken from the board even with
    /// `--board-plot-params`)
    pub create_jobs_file: bool,
}

impl JobExportPcbGerbers {
    /// Port of `JOB_EXPORT_PCB_GERBERS::JOB_EXPORT_PCB_GERBERS()`.
    pub fn new() -> Self {
        JobExportPcbGerbers {
            gerber: JobExportPcbGerber::default(),
            use_board_plot_params: false,
            create_jobs_file: true,
        }
    }
}

impl Default for JobExportPcbGerbers {
    fn default() -> Self {
        JobExportPcbGerbers::new()
    }
}
