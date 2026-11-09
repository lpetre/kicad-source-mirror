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

//! Port of `common/jobs/job_pcb_upgrade.h` / `.cpp` (`JOB_PCB_UPGRADE`) and
//! `common/jobs/job_fp_upgrade.h` / `.cpp` (`JOB_FP_UPGRADE`).

use super::job::Job;

/// Port of `class JOB_PCB_UPGRADE : public JOB`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobPcbUpgrade {
    /// The `JOB` base.
    pub job: Job,
    /// `m_filename`
    pub filename: String,
    /// `m_force`
    pub force: bool,
}

impl JobPcbUpgrade {
    /// Port of `JOB_PCB_UPGRADE::JOB_PCB_UPGRADE()`.
    pub fn new() -> Self {
        JobPcbUpgrade {
            job: Job::new("upgrade", false),
            filename: String::new(),
            force: false,
        }
    }
}

impl Default for JobPcbUpgrade {
    fn default() -> Self {
        JobPcbUpgrade::new()
    }
}

/// Port of `class JOB_FP_UPGRADE : public JOB`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobFpUpgrade {
    /// The `JOB` base (output is a directory).
    pub job: Job,
    /// `m_libraryPath`
    pub library_path: String,
    /// `m_outputLibraryPath`
    pub output_library_path: String,
    /// `m_force`
    pub force: bool,
}

impl JobFpUpgrade {
    /// Port of `JOB_FP_UPGRADE::JOB_FP_UPGRADE()`.
    pub fn new() -> Self {
        JobFpUpgrade {
            job: Job::new("fpupgrade", true),
            library_path: String::new(),
            output_library_path: String::new(),
            force: false,
        }
    }
}

impl Default for JobFpUpgrade {
    fn default() -> Self {
        JobFpUpgrade::new()
    }
}
