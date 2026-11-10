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

//! Port of the loading half of `LIBRARY_MANAGER_ADAPTER` (`common/libraries/library_manager.cpp`:
//! `AsyncLoad` + `BlockUntilLoaded` run sequentially, `loadQueuedRow`, `loadIfNeeded`,
//! `loadFromScope`, `fetchIfLoaded`, `HasLibrary`, `IsLibraryLoaded`) and of
//! `FOOTPRINT_LIBRARY_ADAPTER` (`pcbnew/footprint_library_adapter.cpp`: `createPlugin`,
//! `enumerateLibrary`, `LoadFootprint`) with the KiCad plugin's `FootprintEnumerate` /
//! `FP_CACHE::Load` (`pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp`), package DRC-LP.
//!
//! Only the KiCad s-expression plugin (`.pretty` directories) is ported. A row of another
//! known plugin type (`Legacy`, `Eagle`, `gEDA / Lepton EDA`, ...) fails to load in the
//! port; see `PORT_STATUS.md`.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::LibrarySource;
use super::library_manager::{LibraryManager, join_path};
use super::library_table::{LibraryTableRow, LibraryTableScope};
use crate::board::Board;
use crate::io::{ParseOptions, parse_footprint_file};

/// `LOAD_STATUS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadStatus {
    #[default]
    Invalid,
    Loading,
    Loaded,
    LoadError,
}

/// `LIB_STATUS`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibStatus {
    pub load_status: LoadStatus,
    /// `error->message`.
    pub error: Option<String>,
}

/// `LIB_DATA` with the adapter's `PreloadedFootprints` entry of the library: the footprint
/// files found by the enumeration, by footprint name. The port keeps the file paths and
/// parses a footprint again when it is loaded (the parse is deterministic), instead of
/// holding every parsed footprint of every library.
#[derive(Clone, Debug, Default)]
struct LibData {
    row: LibraryTableRow,
    status: LibStatus,
    /// `plugin != nullptr` (the slot was created by `loadFromScope`).
    has_plugin: bool,
    footprints: BTreeMap<String, String>,
}

/// `PCB_IO_MGR` plugin names (`pcb_io_mgr.cpp` registrations), matched case-insensitively
/// by `PCB_IO_MGR::EnumFromStr`. Only the first one is ported.
const PLUGIN_NAMES: [&str; 24] = [
    "KiCad",
    "Legacy",
    "Allegro",
    "Altium Circuit Maker",
    "Altium Circuit Studio",
    "Altium Designer",
    "CADSTAR PCB Archive",
    "Eagle",
    "EasyEDA / JLCEDA Std",
    "EasyEDA / JLCEDA Pro",
    "EasyEDA / JLCEDA Pro v3",
    "Fabmaster",
    "gEDA / Lepton EDA",
    "P-Cad",
    "Solidworks PCB",
    "IPC-2581",
    "ODB++",
    "PADS",
    "Sprint Layout",
    "DipTrace",
    "Easy-PC",
    "TARGET 3001!",
    "Protel Autotrax",
    "PADS Binary",
];

/// `FILEEXT::KiCadFootprintFileExtension`.
const FOOTPRINT_FILE_EXTENSION: &str = "kicad_mod";

/// The host's file access, shared by the adapter and its users.
pub type SharedLibrarySource = Arc<dyn LibrarySource + Send + Sync>;

/// The footprint library adapter after `AsyncLoad()` + `BlockUntilLoaded()`.
#[derive(Clone, Default)]
pub struct FootprintLibraryAdapter {
    /// `m_manager`.
    pub manager: LibraryManager,
    /// `m_libraries` (project scope).
    libraries: BTreeMap<String, LibData>,
    /// `globalLibs()` (global scope).
    global_libs: BTreeMap<String, LibData>,
    /// The parser options the host loads its board with (the port's staging switch).
    parse_options: ParseOptions,
    /// Where the library files are read (`None`: no file is readable).
    source: Option<SharedLibrarySource>,
}

impl std::fmt::Debug for FootprintLibraryAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FootprintLibraryAdapter")
            .field("manager", &self.manager)
            .field("libraries", &self.libraries)
            .field("global_libs", &self.global_libs)
            .finish_non_exhaustive()
    }
}

/// The source of no files.
struct NoFiles;

impl LibrarySource for NoFiles {
    fn read_file(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }

    fn list_dir(&self, _path: &str) -> Option<Vec<String>> {
        None
    }
}

impl FootprintLibraryAdapter {
    /// An adapter over `manager` reading the libraries through `source`, with nothing
    /// loaded; [`Self::load`] runs the preload.
    pub fn new(
        manager: LibraryManager,
        source: SharedLibrarySource,
        parse_options: ParseOptions,
    ) -> Self {
        FootprintLibraryAdapter {
            manager,
            parse_options,
            source: Some(source),
            ..Default::default()
        }
    }

    fn source(&self) -> SharedLibrarySource {
        self.source.clone().unwrap_or_else(|| Arc::new(NoFiles))
    }

    /// Port of `adapter->AsyncLoad(); adapter->BlockUntilLoaded();` (`JobExportDrc`), run
    /// sequentially in row order: every enabled row of `Rows()` is loaded through
    /// `loadQueuedRow`.
    pub fn load(&mut self) {
        let source = self.source();
        let source: &dyn LibrarySource = &*source;

        struct Work {
            nickname: String,
            scope: LibraryTableScope,
            uri: String,
        }

        let mut work_queue: Vec<Work> = Vec::new();

        for row in self.manager.rows(LibraryTableScope::Both, false) {
            // Disabled libraries must never be contacted
            if row.disabled {
                continue;
            }

            let check = |map: &BTreeMap<String, LibData>| {
                map.get(&row.nickname).is_some_and(|lib| {
                    matches!(
                        lib.status.load_status,
                        LoadStatus::Loaded | LoadStatus::Loading
                    )
                })
            };

            if check(&self.libraries) {
                continue;
            }

            if check(&self.global_libs) && row.scope != LibraryTableScope::Project {
                continue;
            }

            work_queue.push(Work {
                nickname: row.nickname.clone(),
                scope: row.scope,
                uri: self.manager.expand_uri(&row.uri),
            });
        }

        for work in work_queue {
            self.load_queued_row(source, &work.nickname, work.scope, &work.uri);
        }
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::loadQueuedRow`.
    fn load_queued_row(
        &mut self,
        source: &dyn LibrarySource,
        nickname: &str,
        scope: LibraryTableScope,
        uri: &str,
    ) {
        match self.load_if_needed(nickname) {
            Ok(lib_scope) => {
                let result = self.enumerate_library(source, uri);
                let lib = self
                    .target_mut(lib_scope)
                    .get_mut(nickname)
                    .expect("lib slot");

                match result {
                    Ok(footprints) => {
                        lib.footprints = footprints;
                        lib.status.load_status = LoadStatus::Loaded;
                    }
                    Err(error) => {
                        lib.status.load_status = LoadStatus::LoadError;
                        lib.status.error = Some(error);
                    }
                }
            }
            Err(error) => {
                // queuedRowEntry( aWork ).status = LOAD_ERROR
                let entry = self
                    .target_mut(scope)
                    .entry(nickname.to_string())
                    .or_default();
                entry.status = LibStatus {
                    load_status: LoadStatus::LoadError,
                    error: Some(error),
                };
            }
        }
    }

    fn target_mut(&mut self, scope: LibraryTableScope) -> &mut BTreeMap<String, LibData> {
        if scope == LibraryTableScope::Global {
            &mut self.global_libs
        } else {
            &mut self.libraries
        }
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::loadIfNeeded`: the scope whose slot now holds the
    /// library (project first, then global), or the error message.
    fn load_if_needed(&mut self, nickname: &str) -> Result<LibraryTableScope, String> {
        for scope in [LibraryTableScope::Project, LibraryTableScope::Global] {
            if self.load_from_scope(nickname, scope)? {
                return Ok(scope);
            }
        }

        Err(format!("Library {nickname} not found"))
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::loadFromScope`: true when the scope's slot holds a
    /// plugin for the library (created now from the scope's row if needed).
    fn load_from_scope(
        &mut self,
        nickname: &str,
        scope: LibraryTableScope,
    ) -> Result<bool, String> {
        if self
            .target_mut(scope)
            .get(nickname)
            .is_some_and(|lib| lib.has_plugin)
        {
            return Ok(true);
        }

        let Some(row) = self.manager.get_row(nickname, scope).cloned() else {
            return Ok(false);
        };

        create_plugin(&row)?;

        let slot = self
            .target_mut(scope)
            .entry(row.nickname.clone())
            .or_default();
        if !slot.has_plugin {
            slot.status.load_status = LoadStatus::Loading;
            slot.row = row;
            slot.has_plugin = true;
        }

        Ok(true)
    }

    /// Port of `FOOTPRINT_LIBRARY_ADAPTER::enumerateLibrary` with
    /// `PCB_IO_KICAD_SEXPR::FootprintEnumerate( aBestEfforts = false )` and `FP_CACHE::Load`:
    /// every `*.kicad_mod` entry of the directory is parsed; one that fails makes the whole
    /// library fail (the queued `cacheError`).
    fn enumerate_library(
        &self,
        source: &dyn LibrarySource,
        uri: &str,
    ) -> Result<BTreeMap<String, String>, String> {
        let Some(entries) = source.list_dir(uri) else {
            return Err(format!("Footprint library '{uri}' not found."));
        };

        let mut footprints = BTreeMap::new();
        let mut cache_error = String::new();
        let suffix = format!(".{FOOTPRINT_FILE_EXTENSION}");

        for full_name in entries {
            // fileSpec "*.kicad_mod" (wxMatchWild, case-sensitive)
            if !full_name.ends_with(&suffix) {
                continue;
            }

            let path = join_path(uri, &full_name);
            let fp_name = full_name[..full_name.len() - suffix.len()].to_string();

            match self.parse_footprint(source, &path) {
                Ok(_) => {
                    footprints.insert(fp_name, path);
                }
                Err(what) => {
                    if !cache_error.is_empty() {
                        cache_error.push_str("\n\n");
                    }

                    cache_error.push_str(&format!("Unable to read file '{path}'\n"));
                    cache_error.push_str(&what);
                }
            }
        }

        if !cache_error.is_empty() {
            return Err(cache_error);
        }

        Ok(footprints)
    }

    /// The per-file body of `FP_CACHE::Load`: `ParseFootprintLib` of the file into a holder
    /// board, or the `IO_ERROR::What()`.
    fn parse_footprint(&self, source: &dyn LibrarySource, path: &str) -> Result<Board, String> {
        // FILE_LINE_READER: a file that cannot be opened is an IO_ERROR
        let Some(data) = source.read_file(path) else {
            return Err(format!("Unable to open file '{path}'"));
        };

        let text = String::from_utf8_lossy(&data);

        parse_footprint_file(&text, path, &self.parse_options)
            .map(|(board, _report)| board)
            .map_err(|e| e.to_string())
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::fetchIfLoaded`.
    fn fetch_if_loaded(&self, nickname: &str) -> Option<&LibData> {
        if let Some(lib) = self.libraries.get(nickname) {
            return (lib.status.load_status == LoadStatus::Loaded).then_some(lib);
        }

        if self
            .manager
            .get_row(nickname, LibraryTableScope::Project)
            .is_some()
        {
            return None; // project row exists but is not loaded
        }

        if let Some(lib) = self.global_libs.get(nickname) {
            return (lib.status.load_status == LoadStatus::Loaded).then_some(lib);
        }

        None
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::GetRow( aNickname, BOTH )`.
    pub fn get_row(&self, nickname: &str) -> Option<&LibraryTableRow> {
        self.manager.get_row(nickname, LibraryTableScope::Both)
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::HasLibrary`.
    pub fn has_library(&self, nickname: &str, check_enabled: bool) -> bool {
        match self.fetch_if_loaded(nickname) {
            Some(lib) => !check_enabled || !lib.row.disabled,
            None => false,
        }
    }

    /// Port of `LIBRARY_MANAGER_ADAPTER::IsLibraryLoaded`.
    pub fn is_library_loaded(&self, nickname: &str) -> bool {
        if let Some(lib) = self.libraries.get(nickname) {
            return lib.status.load_status == LoadStatus::Loaded;
        }

        if let Some(lib) = self.global_libs.get(nickname) {
            return lib.status.load_status == LoadStatus::Loaded;
        }

        false
    }

    /// `LIBRARY_MANAGER_ADAPTER::GetLibraryStatus`.
    pub fn library_status(&self, nickname: &str) -> Option<&LibStatus> {
        self.libraries
            .get(nickname)
            .or_else(|| self.global_libs.get(nickname))
            .map(|lib| &lib.status)
    }

    /// Port of `FOOTPRINT_LIBRARY_ADAPTER::LoadFootprint( aNickname, aName, aKeepUUID = true )`:
    /// a holder board (`BoardUse::FpHolder`) with the library footprint, its `LIB_ID` set to
    /// `nickname:name`; `None` when the library is not loaded or has no such footprint.
    pub fn load_footprint(&self, nickname: &str, name: &str) -> Option<Board> {
        let lib = self.fetch_if_loaded(nickname)?;
        let path = lib.footprints.get(name)?;
        let source = self.source();
        let mut board = self.parse_footprint(&*source, path).ok()?;

        let fp = board.footprints().next().map(|(id, _)| id)?;

        board.modify_footprint(fp, |footprint, _| {
            // FP_CACHE::Load: LIB_ID( wxEmptyString, fpName ); enumerateLibrary: the nickname
            footprint.fpid = kicad_core::lib_id::LibId::from_parts(&lib.row.nickname, name);
        });

        Some(board)
    }
}

/// Port of `FOOTPRINT_LIBRARY_ADAPTER::createPlugin` for a library row (nested tables never
/// reach it: `Rows()` flattens them): the error message of a row whose plugin cannot be
/// created.
fn create_plugin(row: &LibraryTableRow) -> Result<(), String> {
    let Some(name) = PLUGIN_NAMES
        .iter()
        .find(|name| name.eq_ignore_ascii_case(&row.type_))
    else {
        // "FP: Plugin type %s is unknown!"
        return Err(format!("Unknown library type {} ", row.type_));
    };

    if *name != "KiCad" {
        return Err(format!(
            "The footprint library plugin '{name}' is not ported (only KiCad .pretty libraries)"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::MemoryLibrarySource;
    use super::super::library_manager::LibraryEnv;
    use super::*;

    const FP: &str =
        "(footprint \"R1\" (version 20241229) (generator \"pcbnew\") (layer \"F.Cu\"))\n";

    fn source() -> MemoryLibrarySource {
        let mut src = MemoryLibrarySource::default();
        src.insert(
            "/p/fp-lib-table",
            "(fp_lib_table\n (version 7)\n (lib (name \"a\")(type \"KiCad\")(uri \"${KIPRJMOD}/a.pretty\")(options \"\")(descr \"\"))\n (lib (name \"off\")(type \"KiCad\")(uri \"${KIPRJMOD}/a.pretty\")(options \"\")(descr \"\")(disabled))\n (lib (name \"gone\")(type \"KiCad\")(uri \"${KIPRJMOD}/gone.pretty\")(options \"\")(descr \"\"))\n (lib (name \"bad\")(type \"KiCad\")(uri \"${KIPRJMOD}/bad.pretty\")(options \"\")(descr \"\"))\n (lib (name \"odd\")(type \"Nope\")(uri \"${KIPRJMOD}/a.pretty\")(options \"\")(descr \"\"))\n (lib (name \"sub\")(type \"Table\")(uri \"sub/fp-lib-table\")(options \"\")(descr \"\"))\n)\n",
        );
        src.insert(
            "/p/sub/fp-lib-table",
            "(fp_lib_table (lib (name \"n\")(type \"KiCad\")(uri \"${KIPRJMOD}/a.pretty\")))",
        );
        src.insert("/p/a.pretty/R1.kicad_mod", FP);
        src.insert("/p/a.pretty/notes.txt", "x");
        src.insert("/p/bad.pretty/X.kicad_mod", "(footprint");
        src
    }

    #[test]
    fn load_states() {
        let src = source();
        let mut env = LibraryEnv {
            cwd: "/".into(),
            ..Default::default()
        };
        env.env_vars.insert("KIPRJMOD".into(), "/p".into());
        let mut manager = LibraryManager::new(env);
        manager.load_global_tables(&src, "/cfg");
        manager.load_project_tables(&src, "/p");

        let mut adapter =
            FootprintLibraryAdapter::new(manager, Arc::new(src), ParseOptions::default());
        adapter.load();

        assert!(adapter.get_row("a").is_some());
        assert!(adapter.has_library("a", true));
        assert!(adapter.get_row("off").is_some());
        assert!(!adapter.has_library("off", true));
        assert!(adapter.get_row("gone").is_some() && !adapter.has_library("gone", true));
        assert!(adapter.get_row("bad").is_some() && !adapter.has_library("bad", true));
        assert!(adapter.get_row("odd").is_some() && !adapter.has_library("odd", true));
        assert!(adapter.has_library("n", true), "nested table row");
        assert!(adapter.get_row("missing").is_none());

        let board = adapter.load_footprint("a", "R1").expect("footprint");
        let (_, fp) = board.footprints().next().unwrap();
        assert_eq!(fp.fpid.lib_nickname(), "a");
        assert_eq!(fp.fpid.lib_item_name(), "R1");
        assert!(adapter.load_footprint("a", "R2").is_none());
    }
}
