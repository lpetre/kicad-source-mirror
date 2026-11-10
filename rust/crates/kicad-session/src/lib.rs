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

//! The headless session layer shared by `kicad-cli` and `kicad-wasm`.
//!
//! | module | ports |
//! |---|---|
//! | [`session`] | `PCBNEW_JOBS_HANDLER::getBoard`, `BOARD_LOADER::Load` on in-memory texts; item access for clients |
//! | [`board_loader`] | `pcbnew/board_loader.cpp` (`BOARD_LOADER::Load`, `initializeLoadedBoard`, `SaveBoard`), `KI_TEST::LoadBoard` |
//! | [`jobs`] | `common/jobs/job*.{h,cpp}`: `JOB`, `JOB_RC`, `JOB_PCB_DRC`, `JOB_EXPORT_PCB_PLOT`, `..._GERBER(S)`, `..._DRILL`, `..._SVG`, `JOB_PCB_UPGRADE`, `JOB_FP_UPGRADE` |
//! | [`jobs_handler`] | `pcbnew/pcbnew_jobs_handler.cpp`: one `run_*` per job, `convertLayerArg` |
//! | [`exit_codes`] | `include/cli/exit_codes.h` |
//! | [`build_version`] | `common/build_version.cpp` (version strings) |
//! | [`reporter`] | the `REPORTER` messages of a job run |
//!
//! No filesystem, process or clock access (`PORTING.md` 5): the host reads the board, its
//! `.kicad_pro` and `.kicad_dru`, passes their text in, and writes the
//! [`OutputFile`]s that come back. Subsystems that have not landed yet are reported as
//! [`SessionError::NotYetPorted`].

pub mod board_loader;
pub mod build_version;
pub mod error;
pub mod exit_codes;
pub mod footprint_libraries;
pub mod host_clock;
pub mod jobs;
pub mod jobs_handler;
pub mod reporter;
pub mod session;

pub use board_loader::{LoadInputs, LoadedBoard};
pub use error::SessionError;
pub use footprint_libraries::LibraryHost;
pub use host_clock::HostClock;
pub use jobs_handler::{
    FootprintLibrary, JobOutcome, OutputFile, convert_layer_arg, run_drc, run_export_drill,
    run_export_gerbers, run_export_svg, run_fp_upgrade, run_pcb_upgrade,
};
pub use reporter::{ReportLine, Reporter};
pub use session::{NetInfo, Session, TrackInfo, TrackSpec, ViaSpec};
