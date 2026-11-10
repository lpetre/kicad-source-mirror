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

//! The footprint library tables of a session (package DRC-LP): what kicad-cli does with the
//! library manager around a DRC job. `PGM_KICAD::OnPgmStart` loads the global tables
//! (`GetLibraryManager().LoadGlobalTables()`), `SETTINGS_MANAGER::LoadProject` sets
//! `KIPRJMOD` and loads the project tables (`LIBRARY_MANAGER::ProjectChanged`), and
//! `PCBNEW_JOBS_HANDLER::JobExportDrc` preloads the libraries (`AsyncLoad()` +
//! `BlockUntilLoaded()`). The host supplies the file access, the environment and the
//! directories.

use std::collections::BTreeMap;

use kicad_pcb::libraries::library_manager::{dir_of, versioned_env_var_name};
use kicad_pcb::libraries::{FootprintLibraryAdapter, LibraryEnv, LibraryManager};

use crate::session::Session;

pub use kicad_pcb::io::ParseOptions;
pub use kicad_pcb::libraries::{LibrarySource, MemoryLibrarySource, SharedLibrarySource};

/// What the host knows about the process: the environment and the directories that
/// kicad-cli reads.
#[derive(Clone, Debug, Default)]
pub struct LibraryHost {
    /// The process environment (`wxGetEnv`), before kicad-cli's own definitions.
    pub env_vars: BTreeMap<String, String>,
    /// The working directory.
    pub cwd: String,
    /// The home directory.
    pub home: String,
    /// `PATHS::GetUserSettingsPath()`: the directory of the global `fp-lib-table`
    /// (`$KICAD_CONFIG_HOME/<major.minor>` or `$XDG_CONFIG_HOME/kicad/<major.minor>`).
    pub user_settings_path: String,
    /// `PATHS::GetStockFootprintsPath()`: the default of `KICAD<major>_FOOTPRINT_DIR` when
    /// the environment does not define it (`COMMON_SETTINGS` default, exported by
    /// `PGM_BASE::SetLocalEnvVariables`). `None`: left undefined.
    pub stock_footprints_path: Option<String>,
}

impl Session {
    /// The project directory as kicad-cli sees it (`PROJECT::GetProjectPath()` of the
    /// absolute project file name), `None` without a project.
    pub fn library_project_dir(&self, host: &LibraryHost) -> Option<String> {
        let project = self.board.project.as_ref()?;
        let env = LibraryEnv {
            cwd: host.cwd.clone(),
            home: host.home.clone(),
            ..Default::default()
        };

        Some(dir_of(&env.make_absolute(&project.full_name, None)).to_string())
    }

    /// The [`LibraryEnv`] of this session's project: the host environment plus `KIPRJMOD`
    /// (`wxSetEnv( PROJECT_VAR_NAME, projectPath.GetPath() )`) and the default
    /// `KICAD<major>_FOOTPRINT_DIR`; the project's `TextVarResolver` tokens.
    pub fn library_env(&self, host: &LibraryHost) -> LibraryEnv {
        let mut env_vars = host.env_vars.clone();
        let footprint_dir = versioned_env_var_name("FOOTPRINT_DIR");

        if let Some(stock) = &host.stock_footprints_path
            && !env_vars.contains_key(&footprint_dir)
        {
            env_vars.insert(footprint_dir, stock.clone());
        }

        let mut project_vars = None;

        if let Some(project_dir) = self.library_project_dir(host)
            && let Some(project) = &self.board.project
        {
            env_vars.insert("KIPRJMOD".to_string(), project_dir);

            // PROJECT::TextVarResolver: the fixed tokens, then the project's text variables
            let mut vars: BTreeMap<String, String> = project.text_vars.clone();

            for token in [
                "PROJECTNAME",
                "CURRENT_DATE",
                "CURRENT_TIME_HH_MM_SS",
                "CURRENT_TIME_LOCALE",
                "VCSHASH",
                "VCSSHORTHASH",
            ] {
                let mut value = token.to_string();

                if project.text_var_resolver(&self.board.text_var_clock, &mut value) {
                    vars.insert(token.to_string(), value);
                }
            }

            project_vars = Some(vars);
        }

        LibraryEnv {
            env_vars,
            project_vars,
            cwd: host.cwd.clone(),
            home: host.home.clone(),
        }
    }

    /// Load the global and project footprint tables and preload their libraries
    /// (`LoadGlobalTables`, `LoadProjectTables`, `AsyncLoad` + `BlockUntilLoaded`) into
    /// [`Session::footprint_libraries`], which the DRC's library parity test reads.
    /// `parse_options` are those the board was loaded with (the port's staging switch).
    pub fn load_footprint_libraries(
        &mut self,
        source: SharedLibrarySource,
        host: &LibraryHost,
        parse_options: &kicad_pcb::io::ParseOptions,
    ) {
        let mut manager = LibraryManager::new(self.library_env(host));
        manager.load_global_tables(&*source, &host.user_settings_path);

        if let Some(project_dir) = self.library_project_dir(host) {
            manager.load_project_tables(&*source, &project_dir);
        }

        let mut adapter = FootprintLibraryAdapter::new(manager, source, parse_options.clone());
        adapter.load();
        self.footprint_libraries = Some(adapter);
    }
}
