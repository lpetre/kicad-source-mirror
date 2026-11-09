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

//! Port of `common/jobs/job_rc.h` and `common/jobs/job_rc.cpp` (`JOB_RC`, the base of the DRC
//! and ERC jobs).

use kicad_core::Severity;

use super::job::Job;

/// Port of `JOB_RC::UNITS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RcUnits {
    /// `INCH` (`"in"`)
    Inch,
    /// `MM` (`"mm"`)
    #[default]
    Mm,
    /// `MILS` (`"mils"`)
    Mils,
}

impl RcUnits {
    /// The token of `NLOHMANN_JSON_SERIALIZE_ENUM( JOB_RC::UNITS, ... )`.
    pub fn token(self) -> &'static str {
        match self {
            RcUnits::Inch => "in",
            RcUnits::Mm => "mm",
            RcUnits::Mils => "mils",
        }
    }
}

/// Port of `JOB_RC::OUTPUT_FORMAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RcOutputFormat {
    /// `REPORT` (`"report"`)
    #[default]
    Report,
    /// `JSON` (`"json"`)
    Json,
}

impl RcOutputFormat {
    /// The token of `NLOHMANN_JSON_SERIALIZE_ENUM( JOB_RC::OUTPUT_FORMAT, ... )`.
    pub fn token(self) -> &'static str {
        match self {
            RcOutputFormat::Report => "report",
            RcOutputFormat::Json => "json",
        }
    }
}

/// Port of `class JOB_RC : public JOB`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobRc {
    /// The `JOB` base.
    pub job: Job,
    /// `m_filename`
    pub filename: String,
    /// `m_units`
    pub units: RcUnits,
    /// `m_severity`: a mask of `SEVERITY` bits.
    pub severity: i32,
    /// `m_format`
    pub format: RcOutputFormat,
    /// `m_exitCodeViolations`
    pub exit_code_violations: bool,
}

impl JobRc {
    /// Port of `JOB_RC::JOB_RC( aType )`.
    pub fn new(type_: &str) -> Self {
        JobRc {
            job: Job::new(type_, false),
            filename: String::new(),
            units: RcUnits::Mm,
            severity: Severity::Error.as_i32() | Severity::Warning.as_i32(),
            format: RcOutputFormat::Report,
            // this ensures GUI jobs default to treat violations as job failure, cli will default
            // false elsewhere
            exit_code_violations: true,
        }
    }
}
