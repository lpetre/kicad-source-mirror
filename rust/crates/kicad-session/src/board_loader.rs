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

//! Port of `pcbnew/board_loader.cpp` and `board_loader.h` (`BOARD_LOADER::Load`,
//! `initializeLoadedBoard`, `SaveBoard`) and of the post-parse half of
//! `qa/pcbnew_utils/board_test_utils.cpp` `KI_TEST::LoadBoard`, on in-memory texts
//! (package PCBIO-15).
//!
//! The model steps of both sequences and their order are `kicad_pcb::project::board_loader`
//! (package SET-5); this module performs the *session steps* at their place in that order,
//! because they need crates above `kicad-pcb`:
//!
//! * `InitDrcEngine`: `DRC_ENGINE` creation and `InitEngine( <board>.kicad_dru )`
//!   (`kicad-drc`);
//! * `ResolveDrcExclusions`: `BOARD::ResolveDRCExclusions( true )` and `Add( marker )`
//!   (`kicad-drc`, CLI path only);
//! * `BuildConnectivity`: `BOARD::BuildConnectivity()` (`kicad-connectivity`), which also
//!   propagates nets onto tracks, vias and zones (and so changes what the writer saves).
//!
//! The C++ also loads the drawing sheet (`DS_DATA_MODEL::LoadDrawingSheet`) and fills the
//! layer enum of the property system (`ENUM_MAP<PCB_LAYER_ID>`) first; the port has no global
//! drawing sheet (plotters take it explicitly) and the enum map only feeds the rule parser's
//! layer names, which [`board_layer_choices`] reproduces for `InitEngine`.

use std::rc::Rc;

use kicad_connectivity::data::ConnectivityData;
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::exclusion::{KiapiDrcExclusionCodec, resolve_drc_exclusions};
use kicad_drc::rule_parser::{LayerChoices, board_layer_choices, default_layer_choices};
use kicad_pcb::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::ParseOptions;
use kicad_pcb::project::board_design_settings_json::DrcExclusionCodec;
use kicad_pcb::project::{LoadPath, LoadStep, Project, load_project};

use crate::error::SessionError;
use crate::session::{file_full_name, io_error};

/// What the host read for one board load: the texts of the board and of its sibling project
/// files, and the board's file name. The C++ entry points read these files themselves.
#[derive(Clone, Debug, Default)]
pub struct LoadInputs<'a> {
    /// Text of the `.kicad_pcb` file.
    pub text: &'a str,
    /// The board's file name as the host knows it. kicad-cli makes it absolute
    /// (`wxFileName::MakeAbsolute`) before deriving the project name from it; the host should
    /// pass an absolute path for the same project name and path (`${PROJECTNAME}`, ...).
    pub source_name: &'a str,
    /// Text of `<board>.kicad_pro`, `None` when the file does not exist.
    pub project_json: Option<&'a str>,
    /// Text of `<board>.kicad_prl`, `None` when the file does not exist.
    pub local_settings_json: Option<&'a str>,
    /// QA path only: a legacy `<board>.pro` exists (and no `.kicad_pro`). `KI_TEST::LoadBoard`
    /// then loads and binds a project; its INI migration is not ported, so the project is
    /// empty.
    pub legacy_project_exists: bool,
    /// Text of `<board>.kicad_dru`, `None` when the file does not exist.
    pub rules_text: Option<&'a str>,
    /// Parser options. The C++ parser has none; `allow_pending` is the staging switch of the
    /// port (constructs whose parser package has not landed are skipped instead of failing
    /// the load) for tools that compare what is ported so far.
    pub parse_options: ParseOptions,
}

/// A board after one of the two load sequences: the board with the state the C++ keeps next
/// to it (`bds.m_DRCEngine`, `BOARD::m_connectivity`).
pub struct LoadedBoard {
    /// The board.
    pub board: Board,
    /// `BOARD::GetConnectivity()`, built by the `BuildConnectivity` step.
    pub connectivity: ConnectivityData,
    /// `BOARD_DESIGN_SETTINGS::m_DRCEngine` after `InitEngine`.
    pub drc_engine: DrcEngine,
    /// The `IO_ERROR::What()` of a failed `InitEngine` on the CLI path, which
    /// `initializeLoadedBoard` swallows ("Only matters if user tries to run DRC later; will
    /// be reported at that time"). The engine then holds the implicit rules only.
    pub drc_init_error: Option<String>,
    /// Messages the C++ sends to `BOARD_LOADER::OPTIONS::reporter` (warning severity). kicad-cli
    /// passes no reporter, so it never prints them.
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for LoadedBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedBoard")
            .field("file_name", &self.board.file_name)
            .field("drc_init_error", &self.drc_init_error)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

/// `wxFileName( aPath ).SetExt( aExt )` on a `/` or `\` separated path: the extension of the
/// last path component replaced (or added).
pub fn with_extension(path: &str, ext: &str) -> String {
    let name = file_full_name(path);
    let dir_len = path.len() - name.len();

    let stem = match name.rfind('.') {
        Some(0) | None => name,
        Some(dot) => &name[..dot],
    };

    format!("{}{stem}.{ext}", &path[..dir_len])
}

/// Runs one step of a load sequence: `run` performs the step. [`load_guarded`] lets test tools
/// wrap each step (e.g. to report a `port_pending!` stub and go on with the next step, as the
/// reference-dump drivers do); the library entry points call `run` directly.
pub type StepGuard<'g> = &'g mut dyn FnMut(LoadStep, &mut dyn FnMut());

/// Port of `BOARD_LOADER::Load( aFileName, KICAD_SEXP, aProject )` with the default
/// `OPTIONS` (no reporter, `initialize_after_load = true`), as `PCBNEW_JOBS_HANDLER::getBoard`
/// calls it, on in-memory texts.
///
/// The project is always created: `getProjectForBoard` loads `<board>.kicad_pro` (all
/// defaults when it does not exist) and its `.kicad_prl`. Then `PCB_IO::LoadBoard` (new
/// board, `SetFileName`, parse) and [`initialize_loaded_board`] on the CLI path.
///
/// # Errors
///
/// `SessionError::Load` with the `IO_ERROR::What()` text of the parser or of an exception
/// escaping `BOARD::SetProject`; `NotYetPorted` for a construct whose parser package has not
/// landed (without `allow_pending`).
pub fn load(inputs: &LoadInputs<'_>) -> Result<LoadedBoard, SessionError> {
    load_guarded(LoadPath::Cli, inputs, None)
}

/// Port of `KI_TEST::LoadBoard` (`qa/pcbnew_utils/board_test_utils.cpp:90-181`) on in-memory
/// texts: the project only when `<board>.kicad_pro` (or a legacy `.pro`) exists, then
/// `ReadBoardFromFileOrStream` (a bare parser run: the board's file name stays empty), then
/// the QA sequence of [`initialize_loaded_board`].
///
/// # Errors
///
/// As [`load`]; in addition a rule file that does not parse is an error here (the QA harness
/// does not catch `InitEngine` exceptions).
pub fn load_qa(inputs: &LoadInputs<'_>) -> Result<LoadedBoard, SessionError> {
    load_guarded(LoadPath::Qa, inputs, None)
}

/// [`load`] (`LoadPath::Cli`) or [`load_qa`] (`LoadPath::Qa`), with every post-parse step run
/// through `guard` when one is given.
///
/// # Errors
///
/// As [`load`] / [`load_qa`].
pub fn load_guarded(
    path: LoadPath,
    inputs: &LoadInputs<'_>,
    guard: Option<StepGuard<'_>>,
) -> Result<LoadedBoard, SessionError> {
    let pro_name = with_extension(inputs.source_name, "kicad_pro");

    let (board, project) = match path {
        LoadPath::Cli => {
            // getProjectForBoard: <board>.kicad_pro (MakeAbsolute is the host's job, see
            // LoadInputs); SETTINGS_MANAGER::LoadProject also reads the .kicad_prl
            let project = load_project(inputs.project_json, inputs.local_settings_json, &pro_name);

            // PCB_IO::LoadBoard: newBoard->SetFileName( aFileName ), then loadBoard()
            let mut board = parse(inputs)?;
            board.file_name = inputs.source_name.to_string();
            (board, Some(project))
        }
        LoadPath::Qa => {
            // projectFile.Exists() / legacyProject.Exists(): LoadProject
            let project = (inputs.project_json.is_some() || inputs.legacy_project_exists)
                .then(|| load_project(inputs.project_json, inputs.local_settings_json, &pro_name));

            // ReadBoardFromFileOrStream: the parser alone, no SetFileName
            (parse(inputs)?, project)
        }
    };

    initialize_loaded_board_guarded(
        board,
        path,
        project,
        inputs.rules_text,
        &with_extension(inputs.source_name, "kicad_dru"),
        guard,
    )
}

/// `PCB_IO_KICAD_SEXPR::DoLoad` for a new load, errors mapped as `getBoard` does.
fn parse(inputs: &LoadInputs<'_>) -> Result<Board, SessionError> {
    kicad_pcb::io::parse_board(inputs.text, inputs.source_name, &inputs.parse_options)
        .map_err(|e| io_error(&e, true))
}

/// Port of `BOARD_LOADER::initializeLoadedBoard` (`path == LoadPath::Cli`) and of the
/// post-parse half of `KI_TEST::LoadBoard` (`LoadPath::Qa`): the steps of
/// `kicad_pcb::project::LoadPath::steps` in C++ order, the model steps through
/// `kicad_pcb::project::run_model_step` (SET-5), the session steps here.
///
/// `project` is what the C++ binds (`None`: the QA harness found no project file).
/// `rules_text` is the text of the rule file (`None`: it does not exist) and `rules_path` its
/// full name, used in rule parse errors.
///
/// # Errors
///
/// `SessionError::Load` for an exception escaping `BOARD::SetProject`, and on the QA path for
/// a rule file that does not parse (the remaining steps are not run, as in C++).
pub fn initialize_loaded_board(
    board: Board,
    path: LoadPath,
    project: Option<Project>,
    rules_text: Option<&str>,
    rules_path: &str,
) -> Result<LoadedBoard, SessionError> {
    initialize_loaded_board_guarded(board, path, project, rules_text, rules_path, None)
}

/// [`initialize_loaded_board`] with every step run through `guard` when one is given.
///
/// # Errors
///
/// As [`initialize_loaded_board`].
pub fn initialize_loaded_board_guarded(
    mut board: Board,
    path: LoadPath,
    project: Option<Project>,
    rules_text: Option<&str>,
    rules_path: &str,
    mut guard: Option<StepGuard<'_>>,
) -> Result<LoadedBoard, SessionError> {
    use kicad_pcb::project::{StepOutcome, run_model_step};

    // The DRC_EXCLUSION conversions of the project-file reader (BOARD_DESIGN_SETTINGS).
    let codec: Rc<dyn DrcExclusionCodec> = Rc::new(KiapiDrcExclusionCodec::default());

    let mut project = project;
    let mut loaded = LoadedBoard {
        board: Board::new(),
        connectivity: ConnectivityData::new(),
        drc_engine: DrcEngine::new(),
        drc_init_error: None,
        warnings: Vec::new(),
    };

    for &step in path.steps() {
        let mut model_result = None;
        let mut rules_error = None;

        {
            let mut run = || {
                let outcome = match run_model_step(&mut board, path, step, &mut project, &codec) {
                    Ok(StepOutcome::Session) => {
                        rules_error = run_session_step(
                            &mut board,
                            &mut loaded,
                            path,
                            step,
                            rules_text,
                            rules_path,
                        );
                        Ok(StepOutcome::Done)
                    }
                    other => other,
                };
                model_result = Some(outcome);
            };

            match guard.as_mut() {
                Some(g) => g(step, &mut run),
                None => run(),
            }
        }

        match model_result {
            // The guard swallowed the step (a stub it reported).
            None => {}
            Some(Ok(StepOutcome::Warning(w))) => loaded.warnings.push(w),
            Some(Ok(_)) => {}
            // The exception escapes BOARD::SetProject: the load fails.
            Some(Err(e)) => return Err(SessionError::Load { message: e.message }),
        }

        // KI_TEST::LoadBoard does not catch InitEngine exceptions: the test aborts there.
        if let Some(message) = rules_error {
            return Err(SessionError::Load { message });
        }
    }

    loaded.board = board;
    Ok(loaded)
}

/// One session step (`LoadStep::is_session_step`). Returns the rule error that escapes the QA
/// harness's `InitEngine`.
fn run_session_step(
    board: &mut Board,
    loaded: &mut LoadedBoard,
    path: LoadPath,
    step: LoadStep,
    rules_text: Option<&str>,
    rules_path: &str,
) -> Option<String> {
    match step {
        LoadStep::InitDrcEngine => {
            // bds.m_DRCEngine = std::make_shared<DRC_ENGINE>( aBoard, &bds )
            loaded.drc_engine = DrcEngine::new();

            // The CLI loader has filled ENUM_MAP<PCB_LAYER_ID> with the canonical and the
            // board's layer names just before; the QA harness never touches it.
            let layer_choices: LayerChoices = match path {
                LoadPath::Cli => board_layer_choices(board),
                LoadPath::Qa => default_layer_choices(),
            };

            // TUNING_PROFILES are not in the port's project model yet (SET-11).
            let tuning_profiles = TuningProfiles::default();

            let result = loaded.drc_engine.init_engine(
                board,
                rules_text.map(|t| (t, rules_path)),
                layer_choices,
                &tuning_profiles,
                None,
            );

            if let Err(e) = result {
                match path {
                    // catch( ... ): only matters if user tries to run DRC later; will be
                    // reported at that time
                    LoadPath::Cli => loaded.drc_init_error = Some(e.what().to_string()),
                    LoadPath::Qa => return Some(e.what().to_string()),
                }
            }
        }
        LoadStep::ResolveDrcExclusions => {
            for marker in resolve_drc_exclusions(board, true) {
                board.add_marker(marker);
            }
        }
        LoadStep::BuildConnectivity => {
            loaded.connectivity.build_connectivity(board);
        }
        _ => {}
    }

    None
}

/// Port of `RefreshGeneratedTables( BOARD& )` (`pcbnew/board_tables/generated_table_refresh.cpp`):
/// rebuilds every stale generated table (drill chart, stackup table) among the board's
/// drawings and returns how many were rebuilt.
///
/// # Errors
///
/// `NotYetPorted` when the board holds a generated table: `PCB_GENERATED_TABLE::IsStale` and
/// `RebuildCells` are tier 2 (package M26). A board without one is never changed (0), as in
/// the C++.
pub fn refresh_generated_tables(board: &Board) -> Result<i32, SessionError> {
    let ctx = BoardCtx::bare(board);

    for (id, _) in board.drawings() {
        let Some(item) = ctx.item(id) else {
            continue;
        };

        if !item.type_().is_generated_table_type() {
            continue;
        }

        return Err(SessionError::not_yet_ported(
            "kicad-pcb: RefreshGeneratedTables (PCB_GENERATED_TABLE::IsStale / RebuildCells, M26)",
        ));
    }

    Ok(0)
}

/// Port of `BOARD_LOADER::SaveBoard( aFileName, aBoard )` up to the writer: the pre-steps
/// `BuildConnectivity`, `SynchronizeNetsAndNetClasses( false )`, `SynchronizeProperties`,
/// `RefreshGeneratedTables`, then `PCB_IO_MGR::Save( KICAD_SEXP, ... )` to a string (the host
/// writes the file). Used by `pcb drc --save-board`; `pcb upgrade` calls the writer directly.
///
/// # Errors
///
/// `SessionError::Save` with the writer's `IO_ERROR::What()` (the C++ logs "Cannot save board
/// '%s': %s" and returns false); `NotYetPorted` as [`refresh_generated_tables`].
pub fn save_board(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
) -> Result<String, SessionError> {
    connectivity.build_connectivity(board);
    board.synchronize_nets_and_netclasses(false);
    board.synchronize_properties();
    refresh_generated_tables(board)?;

    crate::session::format_board_text(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_ext() {
        assert_eq!(
            with_extension("/a/b.kicad_pcb", "kicad_pro"),
            "/a/b.kicad_pro"
        );
        assert_eq!(
            with_extension("b.v2.kicad_pcb", "kicad_dru"),
            "b.v2.kicad_dru"
        );
        assert_eq!(
            with_extension("dir.x/noext", "kicad_pro"),
            "dir.x/noext.kicad_pro"
        );
        assert_eq!(with_extension("c:\\x\\y.kicad_pcb", "pro"), "c:\\x\\y.pro");
    }
}
