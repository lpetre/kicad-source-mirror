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

//! Port of `common/project.cpp` and `include/project.h` (`PROJECT`), the parts the board side
//! needs (`docs/analysis/settings.md` 3.2): name and path (L244-298), `GetTextVars`,
//! `ApplyTextVars` (L229-241), `TextVarResolver` (L178-220, on the bound
//! [`BoardProject`](crate::board::BoardProject)), `AbsolutePath` (L501-517), and the project
//! creation of `SETTINGS_MANAGER::LoadProject` (`PROJECT_FILE` + `PROJECT_LOCAL_SETTINGS`) as
//! the in-memory [`load_project`]. Package SET-5.
//!
//! Git, library tables and the `m_elems` cache are out of scope.

use std::collections::BTreeMap;

use kicad_core::title_block::TitleBlockClock;

use crate::board::BoardProject;
use crate::project::project_file::ProjectFile;
use crate::project::project_local_settings::ProjectLocalSettings;

/// `FILEEXT::ProjectFileExtension`.
pub const PROJECT_FILE_EXTENSION: &str = "kicad_pro";
/// `FILEEXT::ProjectLocalSettingsFileExtension`.
pub const PROJECT_LOCAL_SETTINGS_FILE_EXTENSION: &str = "kicad_prl";
/// `FILEEXT::DesignRulesFileExtension`.
pub const DESIGN_RULES_FILE_EXTENSION: &str = "kicad_dru";

/// Port of `PROJECT` as `SETTINGS_MANAGER::LoadProject` leaves it: the full file name, the
/// `PROJECT_FILE` and the `PROJECT_LOCAL_SETTINGS`. Plain data: the board takes it over in
/// `Board::set_project` (C++ shares it by pointer).
#[derive(Clone, Debug, Default)]
pub struct Project {
    /// `m_project_name` (`GetProjectFullName()`), with the extension forced to `kicad_pro`.
    full_name: String,
    /// `m_projectFile`.
    pub project_file: ProjectFile,
    /// `m_localSettings`.
    pub local_settings: ProjectLocalSettings,
}

/// Loads a project from the texts of its `.kicad_pro` and `.kicad_prl` (each `None` when the
/// file does not exist) - the in-memory form of `SETTINGS_MANAGER::LoadProject( fullName )`
/// (`settings_manager.cpp:1020-1134`: `PROJECT::setProjectFullName`, `loadProjectFile` =
/// `PROJECT_FILE::LoadFromFile`, then `PROJECT_LOCAL_SETTINGS::LoadFromFile`).
///
/// `full_name` is the project file name, e.g. `/dir/board.kicad_pro` (kicad-cli uses the board
/// path with the extension `kicad_pro`); another extension is replaced by `kicad_pro`. As in
/// C++ the project exists also when neither file does (all defaults).
pub fn load_project(
    pro_json_text: Option<&str>,
    prl_json_text: Option<&str>,
    full_name: &str,
) -> Project {
    let mut project = Project::default();
    project.set_project_full_name(full_name);
    project.project_file = ProjectFile::load_from_str(pro_json_text);
    project.local_settings = ProjectLocalSettings::load_from_str(prl_json_text);
    project
}

/// `wxFileName( aPath )` split for the reference platform (Unix): (directory without the
/// trailing separator, name, extension). A name starting with a dot has no extension.
pub(crate) fn split_path(path: &str) -> (&str, &str, &str) {
    let (dir, full) = match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    };

    match full.rfind('.') {
        Some(0) | None => (dir, full, ""),
        Some(dot) => (dir, &full[..dot], &full[dot + 1..]),
    }
}

impl Project {
    /// Port of `PROJECT::setProjectFullName`: stores the name; an extension other than
    /// `kicad_pro` is replaced by it (`Clear()` of the cached project data has no counterpart:
    /// the port has none).
    pub fn set_project_full_name(&mut self, full_path_and_name: &str) {
        let ext = split_path(full_path_and_name).2;

        if !ext.is_empty() && ext != PROJECT_FILE_EXTENSION {
            let stem_end = full_path_and_name.len() - ext.len();
            self.full_name = format!(
                "{}{}",
                &full_path_and_name[..stem_end],
                PROJECT_FILE_EXTENSION
            );
        } else {
            self.full_name = full_path_and_name.to_string();
        }
    }

    /// `PROJECT::GetProjectFullName()`.
    pub fn project_full_name(&self) -> &str {
        &self.full_name
    }

    /// `PROJECT::GetProjectName()`: the file name without directory and extension.
    pub fn project_name(&self) -> String {
        split_path(&self.full_name).1.to_string()
    }

    /// `PROJECT::GetProjectPath()`: the directory with a trailing separator ("" when the name
    /// has no directory).
    pub fn project_path(&self) -> String {
        path_with_sep(&self.full_name)
    }

    /// `PROJECT::GetProjectDirectory()`: the directory without the trailing separator.
    pub fn project_directory(&self) -> String {
        split_path(&self.full_name).0.to_string()
    }

    /// `PROJECT::IsNullProject()`: the name is empty.
    pub fn is_null_project(&self) -> bool {
        self.project_name().is_empty()
    }

    /// `PROJECT::GetTextVars()`.
    pub fn text_vars(&self) -> &BTreeMap<String, String> {
        &self.project_file.text_vars
    }

    /// `PROJECT::GetTextVars()` for writing.
    pub fn text_vars_mut(&mut self) -> &mut BTreeMap<String, String> {
        &mut self.project_file.text_vars
    }

    /// Port of `PROJECT::ApplyTextVars`: create or update the given variables (`-D` overrides).
    pub fn apply_text_vars(&mut self, vars: &BTreeMap<String, String>) {
        apply_text_vars(&mut self.project_file.text_vars, vars);
    }

    /// Port of `PROJECT::AbsolutePath`, see [`absolute_path`].
    pub fn absolute_path(&self, file_name: &str) -> String {
        absolute_path(&self.full_name, file_name)
    }
}

/// `wxFileName::GetPathWithSep()` of `path`.
pub(crate) fn path_with_sep(path: &str) -> String {
    let dir = split_path(path).0;

    if dir.is_empty() {
        // An absolute name in the root directory keeps its separator.
        if path.starts_with('/') {
            "/".to_string()
        } else {
            String::new()
        }
    } else {
        format!("{dir}/")
    }
}

/// Body of `PROJECT::ApplyTextVars`.
pub(crate) fn apply_text_vars(
    existing: &mut BTreeMap<String, String>,
    vars: &BTreeMap<String, String>,
) {
    if vars.is_empty() {
        return;
    }

    for (key, value) in vars {
        // create or update the existing vars
        existing.insert(key.clone(), value.clone());
    }
}

/// Port of `PROJECT::AbsolutePath( aFileName )` for the project file `project_full_name`.
///
/// A name starting with `${` is returned unchanged (paths which start with an unresolved
/// variable reference are more likely to be absolute than relative); an absolute name is
/// returned as is; a relative one is resolved against the project directory
/// (`wxFileName::Normalize( FN_NORMALIZE_FLAGS | wxPATH_NORM_ENV_VARS, proDir )`). Of the
/// normalisations only `.` / `..` removal is applied: `~` and `$VAR` expansion need the
/// environment, and a project without a directory would need the process working directory
/// (library code has neither; `PORT_STATUS.md`).
pub fn absolute_path(project_full_name: &str, file_name: &str) -> String {
    // Paths which start with an unresolved variable reference are more likely to be
    // absolute than relative.
    if file_name.starts_with("${") {
        return file_name.to_string();
    }

    if file_name.starts_with('/') {
        return file_name.to_string();
    }

    let pro_dir = split_path(project_full_name).0;
    let joined = if pro_dir.is_empty() {
        if project_full_name.starts_with('/') {
            format!("/{file_name}")
        } else {
            file_name.to_string()
        }
    } else {
        format!("{pro_dir}/{file_name}")
    };

    normalize_dots(&joined)
}

/// `wxPATH_NORM_DOTS`: drops `.` components and resolves `..` against the previous component.
fn normalize_dots(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(&p) if p != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            p => parts.push(p),
        }
    }

    let body = parts.join("/");

    if absolute { format!("/{body}") } else { body }
}

impl BoardProject {
    /// Port of `PROJECT::TextVarResolver( aToken )` (project.cpp:178-220) for the bound
    /// project. `CURRENT_DATE` / `CURRENT_TIME_*` come from `clock` (library code has no wall
    /// clock); `VCSHASH` / `VCSSHORTHASH` resolve to "no hash" (no git access in library code;
    /// that is what `PROJECT_GIT_UTILS::GetCurrentHash` returns for a project outside a
    /// repository, and `aShort` takes its first 8 characters: the same 7).
    pub fn text_var_resolver(&self, clock: &TitleBlockClock, token: &mut String) -> bool {
        match token.as_str() {
            "PROJECTNAME" => {
                *token = self.project_name.clone();
                true
            }
            "CURRENT_DATE" => {
                *token = clock.current_date.clone();
                true
            }
            "CURRENT_TIME_HH_MM_SS" => {
                *token = clock.current_time_hhmmss.clone();
                true
            }
            "CURRENT_TIME_LOCALE" => {
                *token = clock.current_time_locale.clone();
                true
            }
            "VCSHASH" | "VCSSHORTHASH" => {
                *token = "no hash".to_string();
                true
            }
            _ => match self.text_vars.get(token.as_str()) {
                Some(value) => {
                    *token = value.clone();
                    true
                }
                None => false,
            },
        }
    }

    /// Port of `PROJECT::ApplyTextVars` on the bound project.
    pub fn apply_text_vars(&mut self, vars: &BTreeMap<String, String>) {
        apply_text_vars(&mut self.text_vars, vars);
    }

    /// `PROJECT::AbsolutePath` of the bound project.
    pub fn absolute_path(&self, file_name: &str) -> String {
        absolute_path(&self.full_name, file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        let p = load_project(None, None, "/work/demo/board.kicad_pcb");
        assert_eq!(p.project_full_name(), "/work/demo/board.kicad_pro");
        assert_eq!(p.project_name(), "board");
        assert_eq!(p.project_path(), "/work/demo/");
        assert_eq!(p.project_directory(), "/work/demo");
        assert!(!p.is_null_project());
        assert!(!p.project_file.file_synced);

        let p = load_project(None, None, "");
        assert!(p.is_null_project());
        assert_eq!(p.project_path(), "");

        let p = load_project(None, None, "/a/b.c.kicad_pro");
        assert_eq!(p.project_name(), "b.c");
    }

    #[test]
    fn absolute_paths() {
        let pro = "/work/demo/board.kicad_pro";
        assert_eq!(
            absolute_path(pro, "board.kicad_dru"),
            "/work/demo/board.kicad_dru"
        );
        assert_eq!(absolute_path(pro, "../x/./y.txt"), "/work/x/y.txt");
        assert_eq!(absolute_path(pro, "/abs/z"), "/abs/z");
        assert_eq!(absolute_path(pro, "${KIPRJMOD}/z"), "${KIPRJMOD}/z");
    }

    #[test]
    fn apply_text_vars_creates_and_updates() {
        let mut p = load_project(
            Some(r#"{ "meta": { "version": 4 }, "text_variables": { "A": "1", "B": "2" } }"#),
            None,
            "/x/p.kicad_pro",
        );
        let overrides: BTreeMap<String, String> = [("B", "20"), ("C", "30")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        p.apply_text_vars(&overrides);
        assert_eq!(p.text_vars()["A"], "1");
        assert_eq!(p.text_vars()["B"], "20");
        assert_eq!(p.text_vars()["C"], "30");
    }
}
