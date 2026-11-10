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

//! Port of `common/project/project_file.cpp` and `include/project/project_file.h`
//! (`PROJECT_FILE`, the `.kicad_pro` document), load side only and only the parts the board
//! side reads headlessly (`docs/analysis/settings.md` 3.2, 4.1): the document itself (parent
//! of the nested `board.design_settings`), `text_variables`, `pcbnew.page_layout_descr_file`,
//! the root schema migrations and the nested `net_settings` (package SET-3). Package SET-5.
//!
//! The text is passed in by the caller (no filesystem access). Not ported (GUI, schematic,
//! other exporters; `PORT_STATUS.md`): `sheets`, `schematic.*`, `libraries.*`, `cvpcb.*`,
//! `pcbnew.last_paths.*`, `board.layer_presets` / `viewports` / `layer_pairs`,
//! `board.ipc2581.*`, the nested `COMPONENT_CLASS_SETTINGS` (SET-10) and `TUNING_PROFILES`
//! (SET-11), the legacy `.pro` migration and the writer (SET-13).

use std::collections::BTreeMap;

use kicad_core::json::Value;
use kicad_core::json_settings::{JsonSettings, JsonSettingsInternals};
use kicad_core::settings_field;
use kicad_core::settings_parameters::{ParamMap, ParamPath};

use crate::net_settings::NetSettings;
use crate::project::net_settings_json::load_net_settings;

/// `projectFileSchemaVersion` (project_file.cpp:40): update whenever a migration is required.
pub const PROJECT_FILE_SCHEMA_VERSION: i32 = 4;

/// The members of `PROJECT_FILE` bound to the ported parameters.
#[derive(Clone, Debug, Default)]
struct ProjectFileParams {
    text_vars: BTreeMap<String, String>,
    board_drawing_sheet_file: String,
}

/// Port of `PROJECT_FILE` (load side): the parsed and migrated document plus the members the
/// board side reads. `PROJECT_FILE::m_BoardSettings` has no counterpart: the board owns its
/// `BoardDesignSettings`, loaded from [`ProjectFile::internals`] by `Board::set_project`.
#[derive(Clone, Debug)]
pub struct ProjectFile {
    /// `m_internals`: the document after the root migrations (the parent document of the
    /// nested settings).
    pub internals: JsonSettingsInternals,
    /// `m_TextVars` (`text_variables`).
    pub text_vars: BTreeMap<String, String>,
    /// `m_BoardDrawingSheetFile` (`pcbnew.page_layout_descr_file`).
    pub board_drawing_sheet_file: String,
    /// `m_NetSettings` (nested at `net_settings`), loaded by `LoadFromFile`.
    pub net_settings: NetSettings,
    /// `JSON_SETTINGS::LoadFromFile`'s result (`m_fileSynced`): false when there was no file,
    /// it did not parse, or `meta.version` was missing / not an integer.
    pub file_synced: bool,
    /// `m_isFutureFormat`.
    pub is_future_format: bool,
}

impl Default for ProjectFile {
    /// `PROJECT_FILE` loaded without a file (`kicad-cli` with no `.kicad_pro`).
    fn default() -> Self {
        Self::load_from_str(None)
    }
}

/// The `PROJECT_FILE` constructor's parameter list (project_file.cpp:43-325), restricted to the
/// ported members, in registration order, with the root migrations.
fn build_settings() -> JsonSettings<ProjectFileParams> {
    // JSON_SETTINGS( aFullPath, SETTINGS_LOC::PROJECT, projectFileSchemaVersion ); the
    // meta.filename the constructor stores is cleared by LoadFromFile before anything reads it.
    let mut settings = JsonSettings::new(PROJECT_FILE_SCHEMA_VERSION, None);

    settings.add_param(Box::new(ParamMap::wx_string_map(
        "text_variables",
        settings_field!(ProjectFileParams, text_vars),
        BTreeMap::new(),
        true, /* array behavior, even though stored as a map */
    )));

    settings.add_param(Box::new(ParamPath::new(
        "pcbnew.page_layout_descr_file",
        settings_field!(ProjectFileParams, board_drawing_sheet_file),
        "",
    )));

    // PROJECT_FILE::migrateSchema1To2 / migrateSchema2To3 rewrite the entries of
    // `board.layer_presets` (PARAM_LAYER_PRESET::MigrateToV9Layers / MigrateToNamedRenderLayers),
    // GUI state the port does not read; the version still advances as in C++.
    settings.register_migration(1, 2, Box::new(|_| Ok(true)));
    settings.register_migration(2, 3, Box::new(|_| Ok(true)));
    settings.register_migration(3, 4, Box::new(migrate_schema_3_to_4));

    settings
}

/// Port of `PROJECT_FILE::migrateSchema3To4`: an empty `schematic.bus_aliases` object written
/// by older project-only saves becomes `null`.
fn migrate_schema_3_to_4(
    internals: &mut JsonSettingsInternals,
) -> Result<bool, kicad_core::json::JsonError> {
    let aliases = internals.get_json("schematic.bus_aliases")?;

    if let Some(Value::Object(map)) = aliases {
        if map.is_empty() {
            // Older project-only saves wrote empty tables before importing legacy sheet aliases
            internals.set("schematic.bus_aliases", &Value::Null)?;
        }
    }

    Ok(true)
}

impl ProjectFile {
    /// Port of `PROJECT_FILE::LoadFromFile` (`JSON_SETTINGS::LoadFromFile`, json_settings.cpp:
    /// 162-345, then the nested settings in registration order) on the text of the
    /// `.kicad_pro`; `None` = no file (all defaults, `file_synced == false`).
    ///
    /// A text that does not parse is treated like a missing file (C++ retries it as a legacy
    /// INI file, which yields nothing for a broken JSON document; `PORT_STATUS.md`). The
    /// legacy `.pro` migration is not ported: callers pass `None` for a lone `.pro`.
    /// The top-level-sheet fix-ups that follow in `PROJECT_FILE::LoadFromFile` concern the
    /// schematic only.
    pub fn load_from_str(json_text: Option<&str>) -> ProjectFile {
        let mut settings = build_settings();
        let mut params = ProjectFileParams::default();

        let success = match json_text {
            Some(text) => settings.load_from_buffer(text.as_bytes(), &mut params),
            None => {
                // First, load all params to default values
                settings.internals.clear();
                settings.load(&mut params);

                // No file (and no legacy file): nothing to parse.
                // Now that we have new data in the JSON structure, load the params again
                settings.load(&mut params);
                false
            }
        };

        // And finally load any nested settings: NET_SETTINGS (COMPONENT_CLASS_SETTINGS and
        // TUNING_PROFILES are not ported).
        let (net_settings, _) = load_net_settings(Some(&settings.internals));

        ProjectFile {
            internals: settings.internals,
            text_vars: params.text_vars,
            board_drawing_sheet_file: params.board_drawing_sheet_file,
            net_settings,
            file_synced: success,
            is_future_format: settings.is_future_format,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_all_defaults() {
        let pf = ProjectFile::load_from_str(None);
        assert!(!pf.file_synced);
        assert!(pf.text_vars.is_empty());
        assert_eq!(pf.board_drawing_sheet_file, "");
        assert!(pf.net_settings.netclasses().is_empty());
        assert_eq!(
            pf.net_settings.default_netclass().name,
            crate::netclass::NETCLASS_DEFAULT_NAME
        );
    }

    #[test]
    fn text_variables_and_drawing_sheet() {
        let pf = ProjectFile::load_from_str(Some(
            r#"{ "meta": { "version": 4 },
                 "text_variables": { "B": "2", "A": "1" },
                 "pcbnew": { "page_layout_descr_file": "sheet.kicad_wks" } }"#,
        ));
        assert!(pf.file_synced);
        assert_eq!(
            pf.text_vars.iter().collect::<Vec<_>>(),
            vec![(&"A".to_string(), &"1".to_string()), (&"B".to_string(), &"2".to_string())]
        );
        assert_eq!(pf.board_drawing_sheet_file, "sheet.kicad_wks");
    }

    /// PARAM_WXSTRING_MAP: a value that is not a string aborts the load with the map partially
    /// filled (key order), a non-object leaves the (default) map unchanged.
    #[test]
    fn text_variables_partial_and_wrong_type() {
        let pf = ProjectFile::load_from_str(Some(
            r#"{ "meta": { "version": 4 }, "text_variables": { "A": "1", "B": 2, "C": "3" } }"#,
        ));
        assert_eq!(pf.text_vars.len(), 1);
        assert_eq!(pf.text_vars["A"], "1");

        let pf = ProjectFile::load_from_str(Some(
            r#"{ "meta": { "version": 4 }, "text_variables": [ "A" ] }"#,
        ));
        assert!(pf.text_vars.is_empty());
    }

    #[test]
    fn root_migrations_and_versions() {
        let pf = ProjectFile::load_from_str(Some(
            r#"{ "meta": { "version": 3 }, "schematic": { "bus_aliases": {} } }"#,
        ));
        assert!(pf.file_synced);
        assert_eq!(
            pf.internals.get_json("schematic.bus_aliases").unwrap(),
            Some(Value::Null)
        );
        assert_eq!(pf.internals.get::<i32>("meta.version").unwrap(), Some(4));

        // No migrator for version 0: the chain stops, the document keeps its version.
        let pf = ProjectFile::load_from_str(Some(r#"{ "meta": { "version": 0 } }"#));
        assert!(pf.file_synced);
        assert_eq!(pf.internals.get::<i32>("meta.version").unwrap(), Some(0));

        // Missing meta.version: not synced, no migration, values still read.
        let pf = ProjectFile::load_from_str(Some(r#"{ "text_variables": { "X": "y" } }"#));
        assert!(!pf.file_synced);
        assert_eq!(pf.text_vars["X"], "y");

        let pf = ProjectFile::load_from_str(Some(r#"{ "meta": { "version": 9 } }"#));
        assert!(pf.is_future_format);

        // Parse error: treated as no file.
        let pf = ProjectFile::load_from_str(Some("{ broken"));
        assert!(!pf.file_synced);
        assert!(pf.text_vars.is_empty());
    }

    /// The nested `net_settings` are loaded from the document (SET-3 loader).
    #[test]
    fn nested_net_settings_are_loaded() {
        let pf = ProjectFile::load_from_str(Some(
            r#"{ "meta": { "version": 4 },
                 "net_settings": { "meta": { "version": 5 },
                   "classes": [ { "name": "Default", "clearance": 0.3, "priority": 2147483647 },
                                { "name": "HV", "clearance": 1.0, "priority": 0 } ],
                   "netclass_patterns": [ { "netclass": "HV", "pattern": "/HV*" } ] } }"#,
        ));
        assert_eq!(pf.net_settings.default_netclass().clearance(), 300000);
        assert!(pf.net_settings.has_netclass("HV"));
        assert_eq!(pf.net_settings.netclass_pattern_assignments().len(), 1);
    }
}
