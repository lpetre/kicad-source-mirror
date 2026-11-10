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

//! `.kicad_pro` / settings loaders (`docs/analysis/settings.md`), owned by the settings packages.
//!
//! Created empty by M0a so that the module tree is complete; the named package fills it.

pub mod net_settings_json;

pub mod board_design_settings_json;

// Package SET-5: project file, project, binding and load sequences.
pub mod board_loader;
pub mod board_project;
#[allow(clippy::module_inception)] // `project/project.rs` = `common/project.cpp`
pub mod project;
pub mod project_file;
pub mod project_local_settings;

pub use board_loader::{LoadPath, LoadStep, StepOutcome, initialize_loaded_board, run_model_step};
pub use project::{Project, load_project};
pub use project_file::ProjectFile;
pub use project_local_settings::ProjectLocalSettings;
