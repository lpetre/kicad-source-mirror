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

//! Port of the footprint library tables of `common/libraries/*` and of the parts of
//! `pcbnew/footprint_library_adapter.cpp` and `FP_CACHE` (`pcb_io_kicad_sexpr.cpp`) that
//! kicad-cli's DRC uses (package DRC-LP): table files (`fp-lib-table`, global and project,
//! nested tables), URI expansion, and the loading of KiCad `.pretty` libraries.
//!
//! File access belongs to the host: everything reads through a [`LibrarySource`]
//! (`kicad-cli` implements it on the file system, the wasm bindings with
//! [`MemoryLibrarySource`]).

pub mod footprint_library_adapter;
pub mod library_manager;
pub mod library_table;

use std::collections::BTreeMap;

pub use footprint_library_adapter::{FootprintLibraryAdapter, LibStatus, LoadStatus, SharedLibrarySource};
pub use library_manager::{LibraryEnv, LibraryManager};
pub use library_table::{LibraryTable, LibraryTableRow, LibraryTableScope, LibraryTableType};

/// The file system as the library code sees it. Paths are absolute, `/`-separated.
pub trait LibrarySource {
    /// The contents of the regular file `path` (`wxFileName::IsFileReadable` and a read),
    /// `None` when it does not exist or cannot be read.
    fn read_file(&self, path: &str) -> Option<Vec<u8>>;

    /// `wxFileName::IsFileReadable( path )`.
    fn is_file_readable(&self, path: &str) -> bool {
        self.read_file(path).is_some()
    }

    /// `wxFileName::IsDirReadable( path )` / `wxDir( path ).IsOpened()`.
    fn is_dir_readable(&self, path: &str) -> bool {
        self.list_dir(path).is_some()
    }

    /// The names of the entries of directory `path` (`wxDir::GetFirst` / `GetNext` with
    /// `wxDIR_DEFAULT`: files, directories and hidden entries), `None` when it cannot be
    /// opened.
    fn list_dir(&self, path: &str) -> Option<Vec<String>>;

    /// `WX_FILENAME::ResolvePossibleSymlinks`: the path with a symbolic link resolved; the
    /// identity by default.
    fn resolve_symlinks(&self, path: &str) -> String {
        path.to_string()
    }
}

/// A [`LibrarySource`] over files held in memory (the wasm bindings, tests): a directory
/// exists when a file lies below it.
#[derive(Clone, Debug, Default)]
pub struct MemoryLibrarySource {
    /// File contents by absolute path.
    pub files: BTreeMap<String, Vec<u8>>,
}

impl MemoryLibrarySource {
    /// Add (or replace) the file `path`.
    pub fn insert(&mut self, path: &str, data: impl Into<Vec<u8>>) {
        self.files.insert(path.to_string(), data.into());
    }
}

impl LibrarySource for MemoryLibrarySource {
    fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }

    fn list_dir(&self, path: &str) -> Option<Vec<String>> {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let mut names: Vec<String> = Vec::new();
        let mut found = false;

        for key in self.files.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                found = true;
                let name = rest.split('/').next().unwrap_or_default();

                if !name.is_empty() && names.last().map(String::as_str) != Some(name) {
                    names.push(name.to_string());
                }
            }
        }

        found.then_some(names)
    }
}
