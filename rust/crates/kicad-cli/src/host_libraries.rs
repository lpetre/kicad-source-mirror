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

//! The host side of the footprint library tables (package DRC-LP): the file system as a
//! [`LibrarySource`], and the environment and directories kicad-cli reads
//! (`PATHS::GetUserSettingsPath`, `PATHS::GetStockFootprintsPath`, `wxGetEnv`).

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_session::footprint_libraries::{LibrarySource, ParseOptions};
use kicad_session::{LibraryHost, Session};

/// [`LibrarySource`] on the file system.
pub struct FsLibrarySource;

impl LibrarySource for FsLibrarySource {
    fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let meta = std::fs::metadata(path).ok()?;

        if !meta.is_file() {
            return None;
        }

        std::fs::read(path).ok()
    }

    fn list_dir(&self, path: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(path).ok()?;

        Some(
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
        )
    }

    /// `WX_FILENAME::ResolvePossibleSymlinks`: `realpath()` of a symbolic link.
    fn resolve_symlinks(&self, path: &str) -> String {
        let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());

        if is_link && let Ok(real) = std::fs::canonicalize(path) {
            return real.to_string_lossy().into_owned();
        }

        path.to_string()
    }
}

/// `wxGetEnv( name )` with an empty value treated as unset (the C++ checks `!IsEmpty()`).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The process inputs of the library manager.
pub fn library_host() -> LibraryHost {
    let env_vars: BTreeMap<String, String> = std::env::vars().collect();
    let home = env_nonempty("HOME").unwrap_or_default();
    let version = kicad_session::build_version::major_minor_version();

    // PATHS::CalculateUserSettingsPath(): KICAD_CONFIG_HOME, else the user config directory
    // (XDG layout) + "kicad"; then the major.minor version
    let base = match env_nonempty("KICAD_CONFIG_HOME") {
        Some(dir) => dir,
        None => {
            let config =
                env_nonempty("XDG_CONFIG_HOME").unwrap_or_else(|| format!("{home}/.config"));
            format!("{config}/kicad")
        }
    };

    // PATHS::GetStockEDALibraryPath(): $APPDIR/share/kicad, else KICAD_LIBRARY_DATA (the
    // install prefix of a distribution build)
    let stock = match std::env::var("APPDIR") {
        Ok(appdir) => format!("{appdir}/share/kicad"),
        Err(_) => "/usr/share/kicad".to_string(),
    };

    LibraryHost {
        env_vars,
        cwd: std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        home,
        user_settings_path: format!("{}/{version}", base.trim_end_matches('/')),
        stock_footprints_path: Some(format!("{stock}/footprints/")),
    }
}

/// `GetLibraryManager().LoadGlobalTables()` (kicad-cli start), the project tables of the
/// loaded project and `adapter->AsyncLoad(); adapter->BlockUntilLoaded();` (`JobExportDrc`).
pub fn load_footprint_libraries(session: &mut Session) {
    session.load_footprint_libraries(
        Arc::new(FsLibrarySource),
        &library_host(),
        &ParseOptions::default(),
    );
}
