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

//! Ports of the job classes of `common/jobs/` that headless board processing needs.
//!
//! Each C++ class becomes a struct that embeds its base class as the first field (`rc`,
//! `plot`, `gerber`, `job`), with the C++ constructor defaults in `new()`. The values the CLI
//! sets differ from several job defaults; `kicad-cli` applies them exactly as
//! `kicad/cli/command_*.cpp` does.

pub mod job;
pub mod job_export_pcb_drill;
pub mod job_export_pcb_gerber;
pub mod job_export_pcb_plot;
pub mod job_export_pcb_svg;
pub mod job_pcb_drc;
pub mod job_rc;
pub mod job_upgrade;

pub use job::{Job, JobOutput};
pub use job_export_pcb_drill::{
    DrillFormat, DrillOrigin, DrillUnits, GBR_DEFAULT_PRECISION, JobExportPcbDrill, MapFormat,
    ZerosFormat,
};
pub use job_export_pcb_gerber::{JobExportPcbGerber, JobExportPcbGerbers};
pub use job_export_pcb_plot::{JobExportPcbPlot, JobPlotFormat};
pub use job_export_pcb_svg::{
    JobExportPcbSvg, SVG_PRECISION_DEFAULT, SVG_PRECISION_MAX, SVG_PRECISION_MIN, SvgGenMode,
};
pub use job_pcb_drc::JobPcbDrc;
pub use job_rc::{JobRc, RcOutputFormat, RcUnits};
pub use job_upgrade::{JobFpUpgrade, JobPcbUpgrade};
