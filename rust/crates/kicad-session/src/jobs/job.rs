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

//! Port of `common/jobs/job.h` and `common/jobs/job.cpp` (`JOB`, `JOB_OUTPUT`).
//!
//! The `JOB_PARAM` machinery (JSON load/save for jobsets) is not ported: headless runs fill the
//! job structs directly (`PORT_STATUS.md`). Paths are plain strings with `/` separators; the
//! session never touches the filesystem, so the parts of `ResolveOutputPath` that read the
//! environment (`ExpandEnvVarSubstitutions`, `~`) are left to the host (`kicad-cli`).

use std::collections::BTreeMap;

use kicad_core::TitleBlock;
use kicad_core::text_vars::{
    ResolutionContext, expand_text_vars, normalize_file_path_for_text_vars,
};
use kicad_core::title_block::TitleBlockClock;

/// Port of `JOB_OUTPUT`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobOutput {
    /// `m_outputPath`
    pub output_path: String,
}

/// Port of `class JOB`: the members every job has.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// `m_type`
    pub type_: String,
    /// `m_varOverrides` (`std::map`, hence sorted).
    pub var_overrides: BTreeMap<String, String>,
    /// `m_titleBlock`: set from the board by `resolveJobOutputPath`.
    pub title_block: TitleBlock,
    /// `m_tempOutputDirectory`
    pub temp_output_directory: String,
    /// `m_outputPath`: the configured output path.
    pub output_path: String,
    /// `m_outputPathIsDirectory`
    pub output_path_is_directory: bool,
    /// `m_description`
    pub description: String,
    /// `m_workingOutputPath`: transient path that takes priority over the configured one.
    pub working_output_path: String,
    /// `m_outputs`
    pub outputs: Vec<JobOutput>,
}

impl Job {
    /// Port of `JOB::JOB( aType, aOutputIsDirectory )`.
    pub fn new(type_: &str, output_is_directory: bool) -> Self {
        Job {
            type_: type_.to_string(),
            var_overrides: BTreeMap::new(),
            title_block: TitleBlock::default(),
            temp_output_directory: String::new(),
            output_path: String::new(),
            output_path_is_directory: output_is_directory,
            description: String::new(),
            working_output_path: String::new(),
            outputs: Vec::new(),
        }
    }

    /// `JOB::GetType()`.
    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// `JOB::SetVarOverrides()`.
    pub fn set_var_overrides(&mut self, var_overrides: BTreeMap<String, String>) {
        self.var_overrides = var_overrides;
    }

    /// `JOB::SetTitleBlock()`.
    pub fn set_title_block(&mut self, title_block: TitleBlock) {
        self.title_block = title_block;
    }

    /// `JOB::ClearExistingOutputs()`.
    pub fn clear_existing_outputs(&mut self) {
        self.outputs.clear();
    }

    /// `JOB::AddOutput()`.
    pub fn add_output(&mut self, output_path: &str) {
        self.outputs.push(JobOutput {
            output_path: output_path.to_string(),
        });
    }

    /// `JOB::SetTempOutputDirectory()`.
    pub fn set_temp_output_directory(&mut self, base: &str) {
        self.temp_output_directory = base.to_string();
    }

    /// Port of `JOB::SetConfiguredOutputPath()`: a newly configured path must take precedence
    /// over any transient working path left over from a prior run that fell back to a generated
    /// filename.
    pub fn set_configured_output_path(&mut self, path: &str) {
        self.output_path = path.to_string();
        self.working_output_path.clear();
    }

    /// `JOB::GetConfiguredOutputPath()`.
    pub fn configured_output_path(&self) -> &str {
        &self.output_path
    }

    /// `JOB::SetWorkingOutputPath()`.
    pub fn set_working_output_path(&mut self, path: &str) {
        self.working_output_path = path.to_string();
    }

    /// `JOB::GetWorkingOutputPath()`.
    pub fn working_output_path(&self) -> &str {
        &self.working_output_path
    }

    /// `JOB::GetOutputPathIsDirectory()`.
    pub fn output_path_is_directory(&self) -> bool {
        self.output_path_is_directory
    }

    /// Port of `JOB::ResolveOutputPath( aPath, aPathIsDirectory, aProject )`.
    ///
    /// `project_vars` stands for `PROJECT::TextVarResolver` (the project's text variables
    /// after `ApplyTextVars( varOverrides )`). Text variables are resolved with the title block
    /// first, then the project, exactly as the C++ lambda; `ExpandEnvVarSubstitutions` and the
    /// `~` expansion of `ExpandPathVariables` read the process environment and are applied by the
    /// host afterwards.
    pub fn resolve_output_path(
        &self,
        path: &str,
        path_is_directory: bool,
        project_vars: Option<&BTreeMap<String, String>>,
    ) -> String {
        let clock = TitleBlockClock::default();
        let text_resolver = |token: &mut String| -> bool {
            if self.title_block.text_var_resolver(token, &clock) {
                return true;
            }

            if let Some(vars) = project_vars {
                if let Some(value) = vars.get(token.as_str()) {
                    *token = value.clone();
                    return true;
                }
            }

            false
        };

        // ExpandPathVariables( aPath, aProject, &textResolver )
        let normalized = normalize_file_path_for_text_vars(path);
        let out_path = expand_text_vars(
            &normalized,
            Some(&text_resolver),
            ResolutionContext::Internal,
        );

        if !self.temp_output_directory.is_empty() {
            if path_is_directory {
                // Absolute directories are used as given, like absolute file paths
                if out_path.is_empty() {
                    return assign_dir(&self.temp_output_directory);
                } else if is_absolute(&out_path) {
                    return assign_dir(&out_path);
                } else {
                    return prepend_directory_to_path(&out_path, &self.temp_output_directory);
                }
            } else if is_absolute(&out_path) {
                // uhhh, do nothing
                // its a full path passed by cli, so we return as-is
                // the job handlers should have fixed empty paths
                return out_path;
            } else {
                return prepend_directory_to_path(&out_path, &self.temp_output_directory);
            }
        }

        out_path
    }

    /// Port of `JOB::GetFullOutputPath( aProject )`.
    pub fn full_output_path(&self, project_vars: Option<&BTreeMap<String, String>>) -> String {
        let path = if self.working_output_path.is_empty() {
            &self.output_path
        } else {
            &self.working_output_path
        };

        self.resolve_output_path(path, self.output_path_is_directory, project_vars)
    }
}

/// `wxFileName::IsAbsolute()` for `/`-separated paths (and `X:` drive paths).
fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('\\')
        || (path.len() >= 2
            && path.as_bytes()[1] == b':'
            && path.as_bytes()[0].is_ascii_alphabetic())
}

/// `wxFileName fn; fn.AssignDir( aDir ); fn.GetFullPath()`: the directory with a trailing
/// separator.
fn assign_dir(dir: &str) -> String {
    if dir.ends_with('/') {
        dir.to_string()
    } else {
        format!("{dir}/")
    }
}

/// Port of the file-local `PrependDirectoryToPath( aFileName, aDirPath )` of `job.cpp`.
fn prepend_directory_to_path(file_name: &str, dir_path: &str) -> String {
    format!("{dir_path}/{file_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_path_clears_working_path() {
        let mut job = Job::new("drc", false);
        job.set_working_output_path("board-drc.rpt");
        assert_eq!(job.full_output_path(None), "board-drc.rpt");
        job.set_configured_output_path("out.rpt");
        assert_eq!(job.working_output_path(), "");
        assert_eq!(job.full_output_path(None), "out.rpt");
    }

    #[test]
    fn temp_output_directory() {
        let mut job = Job::new("gerbers", true);
        job.set_temp_output_directory("/tmp/run");
        assert_eq!(job.full_output_path(None), "/tmp/run/");
        job.set_configured_output_path("gbr");
        assert_eq!(job.full_output_path(None), "/tmp/run/gbr");
        job.set_configured_output_path("/abs/gbr");
        assert_eq!(job.full_output_path(None), "/abs/gbr/");
    }

    #[test]
    fn text_variables_from_title_block_and_project() {
        let mut job = Job::new("drc", false);
        let mut tb = TitleBlock::default();
        tb.set_revision("B");
        job.set_title_block(tb);
        let mut vars = BTreeMap::new();
        vars.insert("STAGE".to_string(), "proto".to_string());
        job.set_configured_output_path("out-${REVISION}-${STAGE}-${UNKNOWN}.rpt");
        assert_eq!(
            job.full_output_path(Some(&vars)),
            "out-B-proto-${UNKNOWN}.rpt"
        );
    }
}
