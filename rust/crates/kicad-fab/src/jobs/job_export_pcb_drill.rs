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

//! Port of the option part of `common/jobs/job_export_pcb_drill.h` / `.cpp`
//! (`JOB_EXPORT_PCB_DRILL`: members, enums and constructor defaults), used by
//! [`crate::jobs_handler::job_export_drill`] (work package FAB-02).
//!
//! The full job (the `JOB` base with output path, title block and text variable overrides,
//! the `JOB_PARAM` list and the JSON (de)serialisation) is `kicad_session::jobs::
//! JobExportPcbDrill`. `kicad-session` depends on this crate, so this crate cannot use that
//! struct; the session maps its job onto this one (same members, same defaults).

use kicad_plot::gbr_metadata::GbrCreationDate;

/// `GBR_DEFAULT_PRECISION`: default trailing digits in Gerber coordinates, when units are mm.
/// This is also the max usable precision (i.e. internal Pcbnew Units).
pub const GBR_DEFAULT_PRECISION: i32 = 6;

/// `JOB_EXPORT_PCB_DRILL::DRILL_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillFormat {
    /// `EXCELLON`
    #[default]
    Excellon,
    /// `GERBER`
    Gerber,
}

/// `JOB_EXPORT_PCB_DRILL::DRILL_ORIGIN`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillOrigin {
    /// `ABS`: absolute coordinates (offset 0, 0).
    #[default]
    Abs,
    /// `PLOT`: relative to the drill / place file (aux) origin.
    Plot,
}

/// `JOB_EXPORT_PCB_DRILL::DRILL_UNITS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrillUnits {
    /// `INCH` (the job default; the CLI default is mm)
    #[default]
    Inch,
    /// `MM`
    Mm,
}

/// `JOB_EXPORT_PCB_DRILL::ZEROS_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ZerosFormat {
    /// `DECIMAL`
    #[default]
    Decimal,
    /// `SUPPRESS_LEADING`
    SuppressLeading,
    /// `SUPPRESS_TRAILING`
    SuppressTrailing,
    /// `KEEP_ZEROS`
    KeepZeros,
}

/// `JOB_EXPORT_PCB_DRILL::MAP_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapFormat {
    /// `POSTSCRIPT`
    Postscript,
    /// `GERBER_X2`
    GerberX2,
    /// `DXF`
    Dxf,
    /// `SVG`
    Svg,
    /// `PDF`
    #[default]
    Pdf,
}

/// The options of `JOB_EXPORT_PCB_DRILL` (without the `JOB` base and `m_filename`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobExportPcbDrill {
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
    /// `m_reportPath` (empty: `<output dir>/<board>-drill.rpt`)
    pub report_path: String,
}

impl Default for JobExportPcbDrill {
    /// `JOB_EXPORT_PCB_DRILL::JOB_EXPORT_PCB_DRILL()`.
    fn default() -> Self {
        JobExportPcbDrill {
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
        }
    }
}

/// What the C++ takes from the environment while exporting drill files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillExportEnv {
    /// `GetBuildVersion()`, e.g. `10.99.0-unknown`.
    pub build_version: String,
    /// `wxDateTime::Now()` and the local UTC offset.
    pub creation_date: GbrCreationDate,
}
