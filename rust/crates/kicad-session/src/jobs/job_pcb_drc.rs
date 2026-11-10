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

//! Port of `common/jobs/job_pcb_drc.h` and `common/jobs/job_pcb_drc.cpp` (`JOB_PCB_DRC`).

use super::job_rc::JobRc;

/// Port of `class JOB_PCB_DRC : public JOB_RC`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobPcbDrc {
    /// The `JOB_RC` base.
    pub rc: JobRc,
    /// `m_reportAllTrackErrors`
    pub report_all_track_errors: bool,
    /// `m_parity`: the job default is true; `kicad-cli pcb drc` overwrites it with
    /// `--schematic-parity`.
    pub parity: bool,
    /// `m_refillZones`
    pub refill_zones: bool,
    /// `m_saveBoard`
    pub save_board: bool,
    /// Host input (not a `JOB_PCB_DRC` member): whether the board's schematic
    /// (`<board>.kicad_sch`, else `<board>.sch`) exists, `schematicPath.Exists()` in
    /// `JobExportDrc`. False in hosts without a filesystem.
    pub schematic_exists: bool,
    /// Host input: `GetISO8601CurrentDateTime()`, the report's creation date (the library has
    /// no clock). Empty by default.
    pub report_date: String,
    /// Host input: seed of the DRC marker UUIDs (random KIIDs in C++; they never appear in a
    /// report).
    pub marker_uuid_seed: u32,
}

impl JobPcbDrc {
    /// Port of `JOB_PCB_DRC::JOB_PCB_DRC()`.
    pub fn new() -> Self {
        JobPcbDrc {
            rc: JobRc::new("drc"),
            report_all_track_errors: false,
            parity: true,
            refill_zones: false,
            save_board: false,
            schematic_exists: false,
            report_date: String::new(),
            marker_uuid_seed: 0x4452_4331,
        }
    }

    /// Port of `JOB_PCB_DRC::GetDefaultDescription()`.
    pub fn default_description(&self) -> &'static str {
        "Perform DRC"
    }

    /// Port of `JOB_PCB_DRC::GetSettingsDialogTitle()`.
    pub fn settings_dialog_title(&self) -> &'static str {
        "DRC Job Settings"
    }
}

impl Default for JobPcbDrc {
    fn default() -> Self {
        JobPcbDrc::new()
    }
}
