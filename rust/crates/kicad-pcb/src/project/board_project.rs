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

//! Port of the project binding of `pcbnew/board.cpp` (`docs/analysis/settings.md` 4.9, 4.15):
//! `BOARD::SetProject` (L379-417) with the legacy design-settings / net-class transfer,
//! `BOARD::ClearProject` (L420-434), `BOARD::GetDesignRulesPath` (L437-445), and the two
//! post-load helpers the load sequences need that have no other owner:
//! `BOARD::SynchronizeComponentClasses` (L3284-3291) and `BOARD::UpdateUserUnits`
//! (L2050-2069). Package SET-5.
//!
//! Ownership (C++ shares by `shared_ptr`): `set_project` takes the [`Project`]; its
//! `NET_SETTINGS` become the board's `design_settings.net_settings` (C++:
//! `bds.m_NetSettings = project.NetSettings()`), everything else the board reads is kept in
//! [`BoardProject`]. [`Board::clear_project`] gives the project back.

use std::rc::Rc;

use kicad_core::json::JsonError;

use crate::board::{Board, BoardProject};
use crate::ctx::BoardCtx;
use crate::pcb_dimension::DimUnitsMode;
use crate::project::board_design_settings_json::{DrcExclusionCodec, load_board_design_settings};
use crate::project::project::{DESIGN_RULES_FILE_EXTENSION, Project, absolute_path, split_path};
use crate::project::project_file::ProjectFile;
use crate::project::project_local_settings::ProjectLocalSettings;

impl Board {
    /// Port of `BOARD::SetProject( aProject, aReferenceOnly = false )`.
    ///
    /// Binds `project`: unless the board file carried pre-V6 `(setup ...)` design rules
    /// (`legacy_design_settings_loaded`), `board.design_settings` of the project document is
    /// loaded into the board's design settings (SET-4: a key missing there keeps the board's
    /// value); if the board file had `(net_class ...)` sections (`legacy_netclasses_loaded`)
    /// its default class, classes and pattern assignments replace the project's; then the
    /// project's net settings become the board's.
    ///
    /// `codec` converts DRC exclusions (`kicad-drc`; `RawDrcExclusionCodec` without it).
    ///
    /// `Err` is the exception that escapes `BOARD_DESIGN_SETTINGS::LoadFromFile` in C++: as
    /// there, the project is bound (C++ `m_project` is set first) but the net settings were
    /// not transferred (the rest of the function is skipped). C++'s `m_project == aProject`
    /// early return has no counterpart (the project is moved in); a project bound before is
    /// released first (`ClearProject`). `aReferenceOnly` (footprint editor) is not ported.
    pub fn set_project(
        &mut self,
        project: Project,
        codec: Rc<dyn DrcExclusionCodec>,
    ) -> Result<(), JsonError> {
        if self.project.is_some() {
            self.clear_project();
        }

        let full_name = project.project_full_name().to_string();
        let project_name = project.project_name();
        let Project {
            project_file,
            local_settings,
            ..
        } = project;
        let ProjectFile {
            internals: mut document,
            text_vars,
            board_drawing_sheet_file,
            net_settings: mut project_net_settings,
            file_synced,
            ..
        } = project_file;

        // m_project = aProject
        let mut bound = BoardProject {
            text_vars,
            prototype_zone_fill: local_settings.prototype_zone_fill,
            design_rules_path: String::new(),
            full_name,
            project_name,
            visible_layers: local_settings.visible_layers,
            board_drawing_sheet_file,
            document: Default::default(),
            file_synced,
            vcs_hash: None,
        };
        bound.design_rules_path = design_rules_path(&self.file_name, Some(&bound));

        // Link the design settings object to the project file (project.m_BoardSettings) and
        // set parent, which also will load the values from JSON stored in the project if we
        // don't have legacy design settings loaded already
        let result = if !self.legacy_design_settings_loaded {
            let (_settings, result) =
                load_board_design_settings(&mut document, self.design_settings_mut(), codec);
            result.map(|_| ())
        } else {
            Ok(())
        };

        bound.document = document;

        if let Err(e) = result {
            // The exception leaves SetProject here: bound, net settings not transferred (the
            // project's NET_SETTINGS stay with the project object, which C++ keeps alive in
            // the settings manager; the port drops them).
            self.project = Some(bound);
            return Err(e);
        }

        // The DesignSettings' netclasses pointer will be pointing to its internal netclasses
        // list at this point. If we loaded anything into it from a legacy board file then we
        // want to transfer it over to the project netclasses list.
        if self.legacy_netclasses_loaded {
            let legacy_settings = &mut self.design_settings.net_settings;

            project_net_settings
                .set_default_netclass(std::sync::Arc::clone(legacy_settings.default_netclass()));
            project_net_settings.set_netclasses(legacy_settings.netclasses().clone());
            project_net_settings.set_netclass_pattern_assignments(std::mem::take(
                &mut legacy_settings.netclass_pattern_assignments,
            ));
        }

        // Now update the DesignSettings' netclass pointer to point into the project.
        self.design_settings_mut().net_settings = project_net_settings;
        self.project = Some(bound);
        Ok(())
    }

    /// Port of `BOARD::ClearProject`: unbinds the project and returns it with its net settings
    /// (taken back from the design settings). C++ then leaves `bds.m_NetSettings` null; the
    /// board keeps a default `NetSettings` instead. `None` without a project.
    pub fn clear_project(&mut self) -> Option<Project> {
        let bound = self.project.take()?;
        let net_settings = std::mem::take(&mut self.design_settings_mut().net_settings);

        let mut project = Project::default();
        project.set_project_full_name(&bound.full_name);
        project.project_file = ProjectFile {
            internals: bound.document,
            text_vars: bound.text_vars,
            board_drawing_sheet_file: bound.board_drawing_sheet_file,
            net_settings,
            file_synced: bound.file_synced,
            is_future_format: false,
        };
        project.local_settings = ProjectLocalSettings {
            visible_layers: bound.visible_layers,
            prototype_zone_fill: bound.prototype_zone_fill,
            file_synced: false,
        };
        Some(project)
    }

    /// Port of `BOARD::GetDesignRulesPath()`: empty without a file name or a project, else
    /// `<board base name>.kicad_dru` resolved against the project directory.
    pub fn get_design_rules_path(&self) -> String {
        design_rules_path(&self.file_name, self.project.as_ref())
    }

    /// Port of `BOARD::SynchronizeComponentClasses( {} )` (the loaders' call); see
    /// [`Board::synchronize_component_classes_for_sheets`].
    pub fn synchronize_component_classes(&mut self) -> bool {
        self.synchronize_component_classes_for_sheets(&std::collections::BTreeSet::new())
    }

    /// Port of `BOARD::SynchronizeComponentClasses( aNewSheetPaths )`: the project's
    /// component class assignment rules (`COMPONENT_CLASS_SETTINGS`, read from the project
    /// file document) installed by `COMPONENT_CLASS_MANAGER::
    /// SyncDynamicComponentClassAssignments`. False when a rule did not compile.
    ///
    /// C++ dereferences the project unconditionally; without a project the default settings
    /// (no assignments, no sheet classes) are used here.
    pub fn synchronize_component_classes_for_sheets(
        &mut self,
        new_sheet_paths: &std::collections::BTreeSet<String>,
    ) -> bool {
        let settings = crate::component_class::ComponentClassSettings::load_from_parent(
            self.project.as_ref().map(|p| &p.document),
        );

        let sheet_names: Vec<String> = self
            .footprints()
            .map(|(_, fp)| fp.sheetname.clone())
            .collect();

        self.component_class_manager
            .sync_dynamic_component_class_assignments(
                &settings.component_class_assignments,
                settings.enable_sheet_component_classes,
                new_sheet_paths,
                sheet_names.iter().map(String::as_str),
            )
    }

    /// Port of `BOARD::UpdateUserUnits( aBoard, nullptr )` for the whole board: every
    /// dimension (board and footprint level) in `AUTOMATIC` units mode runs `UpdateUnits()`.
    /// Bumps the time stamp when a dimension was updated (C++ has no time-stamp change here;
    /// the port's caches are keyed on it).
    pub fn update_user_units(&mut self) {
        let ids: Vec<_> = self
            .dimensions
            .iter()
            .filter(|(_, dim)| dim.units_mode() == DimUnitsMode::Automatic)
            .map(|(id, _)| id)
            .collect();

        if ids.is_empty() {
            return;
        }

        for id in ids {
            let Some(mut dimension) = self.dimensions.get(id).cloned() else {
                continue;
            };

            dimension.update_units(&BoardCtx::bare(self));

            if let Some(slot) = self.dimensions.get_mut(id) {
                *slot = dimension;
            }
        }

        self.increment_time_stamp();
    }
}

/// Body of `BOARD::GetDesignRulesPath` for a board file name and its project.
fn design_rules_path(board_file_name: &str, project: Option<&BoardProject>) -> String {
    let Some(project) = project else {
        return String::new();
    };

    if board_file_name.is_empty() {
        return String::new();
    }

    // wxFileName fn( m_fileName ); fn.SetExt( kicad_dru ); fn.GetFullName()
    let name = split_path(board_file_name).1;
    let rules = format!("{name}.{DESIGN_RULES_FILE_EXTENSION}");
    absolute_path(&project.full_name, &rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netclass::Netclass;
    use crate::project::board_design_settings_json::RawDrcExclusionCodec;
    use crate::project::project::load_project;
    use std::sync::Arc;

    fn net_class_names(ns: &crate::net_settings::NetSettings) -> Vec<String> {
        ns.netclasses().keys().cloned().collect()
    }

    fn codec() -> Rc<dyn DrcExclusionCodec> {
        Rc::new(RawDrcExclusionCodec)
    }

    const PRO: &str = r#"{
        "meta": { "version": 4 },
        "text_variables": { "VERSION": "1.2" },
        "board": { "design_settings": { "meta": { "version": 2 },
                   "rules": { "min_clearance": 0.25 } } },
        "net_settings": { "meta": { "version": 5 },
            "classes": [ { "name": "Default", "clearance": 0.3, "priority": 2147483647 },
                         { "name": "PWR", "clearance": 0.5, "priority": 0 } ],
            "netclass_patterns": [ { "netclass": "PWR", "pattern": "+5V" } ] } }"#;

    #[test]
    fn set_project_loads_design_and_net_settings() {
        let mut board = Board::new();
        board.file_name = "/w/b.kicad_pcb".into();
        let project = load_project(Some(PRO), None, "/w/b.kicad_pro");
        board.set_project(project, codec()).unwrap();

        assert_eq!(board.design_settings().min_clearance, 250000);
        let ns = &board.design_settings().net_settings;
        assert_eq!(net_class_names(ns), vec!["PWR".to_string()]);
        assert_eq!(ns.default_netclass().clearance(), 300000);
        let bp = board.project.as_ref().unwrap();
        assert_eq!(bp.project_name, "b");
        assert_eq!(bp.text_vars["VERSION"], "1.2");
        assert_eq!(bp.design_rules_path, "/w/b.kicad_dru");
        assert_eq!(board.get_design_rules_path(), "/w/b.kicad_dru");

        let mut token = "PROJECTNAME".to_string();
        assert!(bp.text_var_resolver(&board.text_var_clock, &mut token));
        assert_eq!(token, "b");

        let project = board.clear_project().unwrap();
        assert!(board.project.is_none());
        assert_eq!(board.get_design_rules_path(), "");
        assert!(project.project_file.net_settings.has_netclass("PWR"));
        assert_eq!(project.text_vars()["VERSION"], "1.2");
    }

    /// "any pre-V6 setup token disables reading board.design_settings entirely"
    #[test]
    fn legacy_design_settings_are_not_overwritten() {
        let mut board = Board::new();
        board.legacy_design_settings_loaded = true;
        board.design_settings_mut().min_clearance = 123;
        board
            .set_project(load_project(Some(PRO), None, "/w/b.kicad_pro"), codec())
            .unwrap();
        assert_eq!(board.design_settings().min_clearance, 123);
        // The project's net classes are still used.
        assert!(board.design_settings().net_settings.has_netclass("PWR"));
    }

    /// "(net_class) sections replace the project's classes and patterns" (default class too;
    /// label assignments of the project are kept).
    #[test]
    fn legacy_netclasses_replace_the_projects() {
        let mut board = Board::new();
        board.legacy_netclasses_loaded = true;
        {
            let ns = &mut board.design_settings_mut().net_settings;
            ns.set_netclass("LEGACY", Arc::new(Netclass::new("LEGACY", true)));
            ns.set_netclass_pattern_assignment("/A", "LEGACY");
        }
        let board_default = Arc::clone(board.design_settings().net_settings.default_netclass());

        let pro = PRO.replace(
            r#""netclass_patterns""#,
            r#""netclass_assignments": { "N1": [ "PWR" ] }, "netclass_patterns""#,
        );
        board
            .set_project(load_project(Some(&pro), None, "/w/b.kicad_pro"), codec())
            .unwrap();

        let ns = &board.design_settings().net_settings;
        assert_eq!(net_class_names(ns), vec!["LEGACY".to_string()]);
        assert!(Arc::ptr_eq(ns.default_netclass(), &board_default));
        assert_eq!(ns.netclass_pattern_assignments().len(), 1);
        assert_eq!(ns.netclass_pattern_assignments()[0].1, "LEGACY");
        assert!(ns.has_netclass_label_assignment("N1"));
    }

    #[test]
    fn rebinding_releases_the_previous_project() {
        let mut board = Board::new();
        board
            .set_project(load_project(Some(PRO), None, "/w/b.kicad_pro"), codec())
            .unwrap();
        board
            .set_project(load_project(None, None, "/w/c.kicad_pro"), codec())
            .unwrap();
        let bp = board.project.as_ref().unwrap();
        assert_eq!(bp.project_name, "c");
        assert!(bp.text_vars.is_empty());
        assert!(board.design_settings().net_settings.netclasses().is_empty());
        // The design settings loaded from the first project stay (a missing key keeps the
        // value, settings.md 0.1).
        assert_eq!(board.design_settings().min_clearance, 250000);
    }
}
