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

//! The two post-parse load sequences (`docs/analysis/settings.md` 1.3, 1.4), model side:
//! `BOARD_LOADER::initializeLoadedBoard` (`pcbnew/board_loader.cpp:99-170`, the kicad-cli path)
//! and `KI_TEST::LoadBoard` (`qa/pcbnew_utils/board_test_utils.cpp:90-181`, the QA harness).
//! Package SET-5.
//!
//! Each sequence is a list of [`LoadStep`]s in the exact C++ order. The steps that need crates
//! above `kicad-pcb` (DRC engine creation and rule loading, `ResolveDRCExclusions`, connectivity)
//! are *session steps*: [`run_model_step`] returns [`StepOutcome::Session`] for them and the
//! session glue (package PCBIO-15) performs them at that point of the sequence;
//! [`initialize_loaded_board`] chains everything, calling back for the session steps.
//!
//! CLI order ([`CLI_LOAD_STEPS`]; the project always exists, possibly empty):
//! 1. (not model) drawing sheet load and the layer enum map of the property system;
//! 2. [`LoadStep::BindProject`] - `BOARD::SetProject( project )`;
//! 3. [`LoadStep::InitDrcEngine`] (session) - `DRC_ENGINE` with `<board path>.kicad_dru`,
//!    exceptions swallowed;
//! 4. [`LoadStep::ResolveDrcExclusions`] (session) - `ResolveDRCExclusions( true )`, markers
//!    added;
//! 5. [`LoadStep::BuildConnectivity`] (session);
//! 6. [`LoadStep::BuildListOfNets`] - includes `SynchronizeNetsAndNetClasses( false )` and
//!    `SetAreasNetCodesFromNetNames()`;
//! 7. [`LoadStep::SynchronizeNetsAndNetClasses`] - with `aResetTrackAndViaSizes = true`;
//! 8. [`LoadStep::SynchronizeComponentClasses`] - failure is a warning ("Could not load
//!    component class assignment rules");
//! 9. [`LoadStep::SynchronizeTuningProfileProperties`] - cache invalidation only, no state in
//!    the port (tuning profiles: SET-11);
//! 10. [`LoadStep::UpdateUserUnits`].
//!
//! QA order ([`QA_LOAD_STEPS`]; a project only when a `.kicad_pro` / `.pro` exists - without one
//! net classes are never synchronised):
//! 1. [`LoadStep::BindProject`] (skipped without a project);
//! 2. [`LoadStep::InitDrcEngine`] (session) - `<path>.kicad_dru` if it exists, else none;
//! 3. [`LoadStep::BuildListOfNets`];
//! 4. [`LoadStep::BuildConnectivity`] (session; after the nets, unlike the CLI);
//! 5. [`LoadStep::SynchronizeTuningProfileProperties`];
//! 6. [`LoadStep::SynchronizeComponentClasses`] - only with a project.

use std::rc::Rc;

use kicad_core::json::JsonError;

use crate::board::Board;
use crate::project::board_design_settings_json::DrcExclusionCodec;
use crate::project::project::Project;

/// One step of a load sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoadStep {
    /// `BOARD::SetProject`.
    BindProject,
    /// `DRC_ENGINE` creation + `InitEngine` (session step).
    InitDrcEngine,
    /// `BOARD::ResolveDRCExclusions( true )` + `Add( marker )` (session step).
    ResolveDrcExclusions,
    /// `BOARD::BuildConnectivity` (session step).
    BuildConnectivity,
    /// `BOARD::BuildListOfNets`.
    BuildListOfNets,
    /// `BOARD::SynchronizeNetsAndNetClasses( true )`.
    SynchronizeNetsAndNetClasses,
    /// `BOARD::SynchronizeComponentClasses( {} )`.
    SynchronizeComponentClasses,
    /// `LENGTH_DELAY_CALCULATION::SynchronizeTuningProfileProperties`.
    SynchronizeTuningProfileProperties,
    /// `BOARD::UpdateUserUnits( board, nullptr )`.
    UpdateUserUnits,
}

impl LoadStep {
    /// Whether the session glue performs this step (it needs a crate above `kicad-pcb`).
    pub fn is_session_step(self) -> bool {
        matches!(
            self,
            LoadStep::InitDrcEngine | LoadStep::ResolveDrcExclusions | LoadStep::BuildConnectivity
        )
    }
}

/// Which C++ sequence to reproduce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadPath {
    /// `BOARD_LOADER::Load` with `initialize_after_load` (kicad-cli, API).
    Cli,
    /// `KI_TEST::LoadBoard` (QA tests, `pcb-dump`'s default path).
    Qa,
}

/// `BOARD_LOADER::initializeLoadedBoard` order.
pub const CLI_LOAD_STEPS: [LoadStep; 9] = [
    LoadStep::BindProject,
    LoadStep::InitDrcEngine,
    LoadStep::ResolveDrcExclusions,
    LoadStep::BuildConnectivity,
    LoadStep::BuildListOfNets,
    LoadStep::SynchronizeNetsAndNetClasses,
    LoadStep::SynchronizeComponentClasses,
    LoadStep::SynchronizeTuningProfileProperties,
    LoadStep::UpdateUserUnits,
];

/// `KI_TEST::LoadBoard` order.
pub const QA_LOAD_STEPS: [LoadStep; 6] = [
    LoadStep::BindProject,
    LoadStep::InitDrcEngine,
    LoadStep::BuildListOfNets,
    LoadStep::BuildConnectivity,
    LoadStep::SynchronizeTuningProfileProperties,
    LoadStep::SynchronizeComponentClasses,
];

impl LoadPath {
    /// The steps of this path in C++ order.
    pub fn steps(self) -> &'static [LoadStep] {
        match self {
            LoadPath::Cli => &CLI_LOAD_STEPS,
            LoadPath::Qa => &QA_LOAD_STEPS,
        }
    }
}

/// What [`run_model_step`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The model step ran (or is a no-op on this path).
    Done,
    /// A session step: the caller performs it now.
    Session,
    /// The step ran and reported a warning (C++ reporter text).
    Warning(String),
}

/// Runs one model step of `path` on `board`. `project` is consumed by
/// [`LoadStep::BindProject`] (`None`: the QA harness without a project file; the CLI always
/// passes one, see [`crate::project::project::load_project`]).
///
/// `Err` is the exception C++ lets escape from `BOARD::SetProject` (the load fails).
pub fn run_model_step(
    board: &mut Board,
    path: LoadPath,
    step: LoadStep,
    project: &mut Option<Project>,
    codec: &Rc<dyn DrcExclusionCodec>,
) -> Result<StepOutcome, JsonError> {
    if step.is_session_step() {
        return Ok(StepOutcome::Session);
    }

    match step {
        LoadStep::BindProject => {
            // CLI: initializeLoadedBoard returns early without a project; the QA harness binds
            // one only when a project file exists.
            if let Some(p) = project.take() {
                board.set_project(p, Rc::clone(codec))?;
            }
        }
        LoadStep::BuildListOfNets => board.build_list_of_nets(),
        LoadStep::SynchronizeNetsAndNetClasses => board.synchronize_nets_and_netclasses(true),
        LoadStep::SynchronizeComponentClasses => {
            if path == LoadPath::Qa && board.project.is_none() {
                return Ok(StepOutcome::Done);
            }

            // Apply component class assignment rules from the project. Without this,
            // conditions like A.hasComponentClass('Inductor') in custom DRC rules never match
            // because no footprints have any dynamic component class assigned.
            if !board.synchronize_component_classes() && path == LoadPath::Cli {
                return Ok(StepOutcome::Warning(
                    "Could not load component class assignment rules".to_string(),
                ));
            }
        }
        // m_tuningProfileParameters->OnSettingsChanged(): drops cached tuning-profile
        // geometry, which the port does not hold.
        LoadStep::SynchronizeTuningProfileProperties => {}
        LoadStep::UpdateUserUnits => board.update_user_units(),
        LoadStep::InitDrcEngine | LoadStep::ResolveDrcExclusions | LoadStep::BuildConnectivity => {
            return Ok(StepOutcome::Session);
        }
    }

    Ok(StepOutcome::Done)
}

/// Runs the whole sequence of `path` in C++ order, calling `session_step` for the session
/// steps at their place. Returns the warnings of the model steps.
pub fn initialize_loaded_board(
    board: &mut Board,
    path: LoadPath,
    project: Option<Project>,
    codec: Rc<dyn DrcExclusionCodec>,
    session_step: &mut dyn FnMut(LoadStep, &mut Board),
) -> Result<Vec<String>, JsonError> {
    let mut project = project;
    let mut warnings = Vec::new();

    for &step in path.steps() {
        match run_model_step(board, path, step, &mut project, &codec)? {
            StepOutcome::Done => {}
            StepOutcome::Session => session_step(step, board),
            StepOutcome::Warning(w) => warnings.push(w),
        }
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::board_design_settings_json::RawDrcExclusionCodec;
    use crate::project::project::load_project;

    fn codec() -> Rc<dyn DrcExclusionCodec> {
        Rc::new(RawDrcExclusionCodec)
    }

    /// The session steps come back at their C++ positions.
    #[test]
    fn session_steps_in_order() {
        let cli: Vec<_> = CLI_LOAD_STEPS
            .iter()
            .filter(|s| s.is_session_step())
            .collect();
        assert_eq!(
            cli,
            vec![
                &LoadStep::InitDrcEngine,
                &LoadStep::ResolveDrcExclusions,
                &LoadStep::BuildConnectivity
            ]
        );
        let qa = QA_LOAD_STEPS
            .iter()
            .position(|s| *s == LoadStep::BuildConnectivity);
        let nets = QA_LOAD_STEPS
            .iter()
            .position(|s| *s == LoadStep::BuildListOfNets);
        assert!(nets < qa);
    }

    /// The QA path without a project neither binds nor synchronises net classes.
    #[test]
    fn qa_without_project_binds_nothing() {
        let mut board = Board::new();
        let mut project = None;
        for &step in &QA_LOAD_STEPS {
            if matches!(
                step,
                LoadStep::BindProject | LoadStep::SynchronizeComponentClasses
            ) {
                let r = run_model_step(&mut board, LoadPath::Qa, step, &mut project, &codec());
                assert_eq!(r.unwrap(), StepOutcome::Done);
            }
        }
        assert!(board.project.is_none());
    }

    /// The CLI path binds the (empty) project.
    #[test]
    fn cli_binds_the_empty_project() {
        let mut board = Board::new();
        let mut project = Some(load_project(None, None, "/x/b.kicad_pro"));
        let r = run_model_step(
            &mut board,
            LoadPath::Cli,
            LoadStep::BindProject,
            &mut project,
            &codec(),
        );
        assert_eq!(r.unwrap(), StepOutcome::Done);
        assert!(project.is_none());
        assert_eq!(board.project.as_ref().unwrap().project_name, "b");
    }
}
