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

//! Port of the version queries of `common/build_version.cpp` (`GetMajorMinorPatchVersion`,
//! `GetCommitHash`, `GetBuildVersion`) with the values of the generated
//! `kicad_build_version.h` of the reference build (`docs/ORACLE.md`).
//!
//! CMake generates the C++ values from `cmake/KiCadVersion.cmake` and `git describe`; the
//! port pins the values of the source tree it was ported from, so that the reported version
//! is the version whose behaviour the port reproduces.

/// `KICAD_MAJOR_MINOR_PATCH_VERSION`.
pub const KICAD_MAJOR_MINOR_PATCH_VERSION: &str = "10.99.0";

/// `KICAD_MAJOR_MINOR_VERSION`.
pub const KICAD_MAJOR_MINOR_VERSION: &str = "10.99";

/// `KICAD_VERSION_FULL` (`KICAD_SEMANTIC_VERSION` of `cmake/KiCadVersion.cmake`; the
/// reference build has no git tag, hence `-unknown`).
pub const KICAD_VERSION_FULL: &str = "10.99.0-unknown";

/// `KICAD_COMMIT_HASH`: the last commit of the C++ tree this port follows (the commit the
/// reference `kicad-cli` was built from).
pub const KICAD_COMMIT_HASH: &str = "f87615475ec8b349c979b22474eb1c3d2d284f65";

/// Port of `GetMajorMinorPatchVersion()`.
pub fn major_minor_patch_version() -> &'static str {
    KICAD_MAJOR_MINOR_PATCH_VERSION
}

/// Port of `GetMajorMinorVersion()`.
pub fn major_minor_version() -> &'static str {
    KICAD_MAJOR_MINOR_VERSION
}

/// Port of `GetCommitHash()`.
pub fn commit_hash() -> &'static str {
    KICAD_COMMIT_HASH
}

/// Port of `GetBuildVersion()`: the full version string written into generated files
/// (`G04 Created by KiCad (PCBNEW <build version>)`, report headers).
pub fn build_version() -> &'static str {
    KICAD_VERSION_FULL
}

/// Reduced port of `GetVersionInfoData( aTitle, aHtml = false, aBrief = false )`.
///
/// The C++ report lists the host platform, library versions (wxWidgets, FreeType, ...),
/// build date and locale. None of that exists in the port (no wx, no C libraries) and host
/// queries are not allowed in library crates (`PORTING.md` 5), so only the application line
/// and the version line keep the C++ layout; the rest names the port. Recorded in
/// `PORT_STATUS.md`.
pub fn version_info_data(title: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("Application: {title} (Rust port of KiCad)\n"));
    s.push('\n');
    s.push_str(&format!("Version: {KICAD_VERSION_FULL}, release build\n"));
    s.push('\n');
    s.push_str("Ported from:\n");
    s.push_str(&format!("\tKiCad commit {KICAD_COMMIT_HASH}\n"));
    s
}
