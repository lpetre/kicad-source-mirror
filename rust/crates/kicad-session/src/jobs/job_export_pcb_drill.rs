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

//! Port of `common/jobs/job_export_pcb_drill.h` and `common/jobs/job_export_pcb_drill.cpp`
//! (`JOB_EXPORT_PCB_DRILL`).

use super::job::Job;

/// `GBR_DEFAULT_PRECISION`: default trailing digits in Gerber coordinates, when units are mm.
/// This is also the max usable precision (i.e. internal Pcbnew Units).
pub const GBR_DEFAULT_PRECISION: i32 = 6;

/// Port of `JOB_EXPORT_PCB_DRILL::DRILL_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillFormat {
    /// `EXCELLON` (`"excellon"`)
    #[default]
    Excellon,
    /// `GERBER` (`"gerber"`)
    Gerber,
}

/// Port of `JOB_EXPORT_PCB_DRILL::DRILL_ORIGIN`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillOrigin {
    /// `ABS` (`"abs"`)
    #[default]
    Abs,
    /// `PLOT` (`"plot"`)
    Plot,
}

/// Port of `JOB_EXPORT_PCB_DRILL::DRILL_UNITS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillUnits {
    /// `INCH` (`"in"`); the job default (the CLI default is mm)
    #[default]
    Inch,
    /// `MM` (`"mm"`)
    Mm,
}

/// Port of `JOB_EXPORT_PCB_DRILL::ZEROS_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ZerosFormat {
    /// `DECIMAL` (`"decimal"`)
    #[default]
    Decimal,
    /// `SUPPRESS_LEADING` (`"surpress_leading"`, sic)
    SuppressLeading,
    /// `SUPPRESS_TRAILING` (`"surpress_trailing"`, sic)
    SuppressTrailing,
    /// `KEEP_ZEROS` (`"keep_zeros"`)
    KeepZeros,
}

/// Port of `JOB_EXPORT_PCB_DRILL::MAP_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapFormat {
    /// `POSTSCRIPT` (`"postscript"`)
    Postscript,
    /// `GERBER_X2` (`"gerberx2"`)
    GerberX2,
    /// `DXF` (`"dxf"`)
    Dxf,
    /// `SVG` (`"svg"`)
    Svg,
    /// `PDF` (`"pdf"`)
    #[default]
    Pdf,
}

/// Port of `class JOB_EXPORT_PCB_DRILL : public JOB`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbDrill {
    /// The `JOB` base (output is a directory).
    pub job: Job,
    /// `m_filename`
    pub filename: String,
    /// `m_excellonMirrorY`
    pub excellon_mirror_y: bool,
    /// `m_excellonMinimalHeader`
    pub excellon_minimal_header: bool,
    /// `m_excellonCombinePTHNPTH`
    pub excellon_combine_pth_npth: bool,
    /// `m_excellonOvalDrillRoute`
    pub excellon_oval_drill_route: bool,
    /// `m_format`
    pub format: DrillFormat,
    /// `m_drillOrigin`
    pub drill_origin: DrillOrigin,
    /// `m_drillUnits`
    pub drill_units: DrillUnits,
    /// `m_zeroFormat`
    pub zero_format: ZerosFormat,
    /// `m_mapFormat`
    pub map_format: MapFormat,
    /// `m_gerberPrecision`
    pub gerber_precision: i32,
    /// `m_generateMap`
    pub generate_map: bool,
    /// `m_generateTenting`
    pub generate_tenting: bool,
    /// `m_generateReport`
    pub generate_report: bool,
    /// `m_reportPath`
    pub report_path: String,
    /// The file date the C++ writers take from the clock (`wxDateTime::Now()`, local time and
    /// UTC offset; Excellon `; DRILL file ... date ...` and Gerber `%TF.CreationDate`): set by
    /// the host (no clock in library crates); the default is all zeros.
    pub creation_date: kicad_fab::jobs::job_export_pcb_plot::GbrCreationDate,
}

impl JobExportPcbDrill {
    /// Port of `JOB_EXPORT_PCB_DRILL::JOB_EXPORT_PCB_DRILL()`.
    pub fn new() -> Self {
        JobExportPcbDrill {
            job: Job::new("drill", true),
            filename: String::new(),
            excellon_mirror_y: false,
            excellon_minimal_header: false,
            excellon_combine_pth_npth: true,
            excellon_oval_drill_route: false,
            format: DrillFormat::Excellon,
            drill_origin: DrillOrigin::Abs,
            drill_units: DrillUnits::Inch,
            zero_format: ZerosFormat::Decimal,
            map_format: MapFormat::Pdf,
            gerber_precision: GBR_DEFAULT_PRECISION,
            generate_map: false,
            generate_tenting: false,
            generate_report: false,
            report_path: String::new(),
            creation_date: Default::default(),
        }
    }

    /// Port of `JOB_EXPORT_PCB_DRILL::GetDefaultDescription()`.
    pub fn default_description(&self) -> &'static str {
        "Export drill data"
    }

    /// Port of `JOB_EXPORT_PCB_DRILL::GetSettingsDialogTitle()`.
    pub fn settings_dialog_title(&self) -> &'static str {
        "Export Drill Data Job Settings"
    }
}

impl Default for JobExportPcbDrill {
    fn default() -> Self {
        JobExportPcbDrill::new()
    }
}
