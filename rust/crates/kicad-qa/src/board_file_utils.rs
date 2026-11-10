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

//! Port of the file-system part of `qa/pcbnew_utils/board_file_utils.cpp` /
//! `include/pcbnew_utils/board_file_utils.h` (namespace `KI_TEST`).
//!
//! `kicad-qa` cannot depend on `kicad-pcb` (it is that crate's dev-dependency), so the helpers
//! that need the board parser or writer (`ReadBoardFromFile`, `RequireBoardFromPcbnewQAData`,
//! `DumpBoardToFile`, `ReadFootprintFromFile`, `DumpFootprintToFile`) live next to their users
//! in `crates/kicad-pcb/tests/io_corpus/board_file_utils.rs` and build on the functions here.

use std::path::{Path, PathBuf};

/// Port of `KI_TEST::GetPcbnewTestDataDir`: the pcbnew QA data directory, from the environment
/// variable `KICAD_TEST_PCBNEW_DATA_DIR` if set, else the compiled-in location
/// (`<kicad>/qa/data/pcbnew`, [`crate::pcbnew_data_dir`]). Ends in `/` to force a directory
/// interpretation, as in C++.
pub fn get_pcbnew_test_data_dir() -> String {
    let mut fn_ = match std::env::var("KICAD_TEST_PCBNEW_DATA_DIR") {
        // Use whatever was given in the env var
        Ok(env) => env,
        // Use the compiled-in location of the data dir (i.e. where the files were at build time)
        Err(_) => crate::pcbnew_data_dir().to_string_lossy().into_owned(),
    };

    // Ensure the string ends in / to force a directory interpretation
    fn_.push('/');
    fn_
}

/// The path `KI_TEST::RequireBoardFromPcbnewQAData( aFilename )` reads:
/// `GetPcbnewTestDataDir() + aFilename`.
pub fn pcbnew_qa_data_path(file_name: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(format!(
        "{}{}",
        get_pcbnew_test_data_dir(),
        file_name.as_ref().to_string_lossy()
    ))
}

/// The text of a board or footprint file, as the C++ line reader sees it: the bytes, with
/// invalid UTF-8 replaced (`wxString::FromUTF8` would drop such lines; no corpus file has any).
///
/// Returns `None` when the file cannot be opened (`ReadBoardFromFileOrStream` returns
/// `nullptr` then).
pub fn read_file_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;

    Some(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcbnew_test_data_dir() {
        // The environment variable is not set in the test run (the C++ CMake setup sets it to
        // the same directory).
        if std::env::var_os("KICAD_TEST_PCBNEW_DATA_DIR").is_none() {
            let dir = get_pcbnew_test_data_dir();
            assert!(dir.ends_with('/'));
            assert!(Path::new(&dir).is_dir());
            assert!(pcbnew_qa_data_path("issue18.kicad_pcb").is_file());
        }
    }
}
