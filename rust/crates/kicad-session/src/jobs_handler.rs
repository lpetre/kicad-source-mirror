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

//! Port of the board jobs of `pcbnew/pcbnew_jobs_handler.cpp` (`PCBNEW_JOBS_HANDLER`):
//! `JobExportDrc`, `JobExportGerbers`, `JobExportDrill`, `JobExportSvg`, `JobUpgrade`,
//! `JobExportFpUpgrade`, with the helpers `convertLayerArg`, `resolveJobOutputPath` and the
//! layer selection of `JobExportGerbers` / `preparePlotLayers`.
//!
//! Each `run_*` takes the loaded [`Session`] (the C++ `getBoard()` result; loading failures
//! are `Session::load_board` errors and map to `ERR_INVALID_INPUT_FILE` in the host), the job,
//! and a [`Reporter`] that receives every `m_reporter->Report()` message in order. The result
//! carries the C++ exit code and the produced files as `(name, bytes)` pairs, named with the
//! paths the C++ would write (the host writes them; this crate has no filesystem).
//!
//! The steps run in the C++ order; the first step whose subsystem has not landed yet returns
//! `SessionError::NotYetPorted` naming it.

use std::collections::BTreeMap;

use kicad_core::layer_ids::layer_name;
use kicad_core::{Lseq, Lset, PcbLayerId, Severity};
use serde::Serialize;

use crate::error::SessionError;
use crate::exit_codes;
use crate::jobs::{
    DrillFormat, JobExportPcbDrill, JobExportPcbGerbers, JobExportPcbPlot, JobExportPcbSvg,
    JobFpUpgrade, JobPcbDrc, JobPcbUpgrade, RcOutputFormat, RcUnits,
};
use crate::reporter::Reporter;
use crate::session::{Session, file_dir, file_full_name, file_stem};

/// `SEXPR_BOARD_FILE_VERSION` (the current board file format version), used by `JobUpgrade`.
pub use kicad_pcb::io::SEXPR_BOARD_FILE_VERSION;

/// A produced file: the path the C++ would write it to and its contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutputFile {
    /// Output path as the C++ builds it (relative paths are relative to the host's working
    /// directory, `/`-separated).
    pub name: String,
    /// The file contents.
    pub contents: Vec<u8>,
}

/// The result of a job run that reached its end (successfully or with a C++ error exit).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JobOutcome {
    /// The `CLI::EXIT_CODES` value the C++ handler returns.
    pub exit_code: i32,
    /// The produced files in the order the C++ writes them (`JOB::GetOutputs()` order).
    pub files: Vec<OutputFile>,
    /// The report text (DRC report, drill report), when the job produces one.
    pub report_text: Option<String>,
    /// The directory the C++ job creates before it writes (`PATHS::EnsurePathExists`, or the
    /// `wxFileName::Mkdir` of `FP_CACHE::Save`); the host creates it and nothing else, so a
    /// file outside it (e.g. a drill report in a directory that does not exist) fails to open
    /// as in C++. `None`: the job creates no directory.
    #[serde(skip)]
    pub output_dir: Option<OutputDir>,
}

/// A directory a job creates before writing its files (see [`JobOutcome::output_dir`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDir {
    /// The directory, as the job resolved it (relative to the host's working directory;
    /// empty: the working directory itself).
    pub path: String,
    /// `wxPATH_MKDIR_FULL`: missing parent directories are created too.
    pub full: bool,
    /// What the C++ reports (`RPT_SEVERITY_ERROR`) when the directory cannot be created;
    /// the job's other messages are not printed then (the C++ returns before them).
    pub failure_message: String,
    /// The exit code the C++ returns then.
    pub failure_exit_code: i32,
}

impl OutputDir {
    /// Port of `PATHS::EnsurePathExists( aPath, aPathToFile )` as a request to the host: the
    /// directory `aPath` (`aPathToFile == false`) or the directory part of the file `aPath`
    /// (`wxFileName::GetPath()`), created with `wxPATH_MKDIR_FULL`; the job handlers report
    /// `Failed to create output directory` and return `ERR_INVALID_OUTPUT_CONFLICT` when it
    /// fails.
    pub fn ensure_path_exists(a_path: &str, a_path_to_file: bool) -> OutputDir {
        let path = if a_path_to_file {
            file_dir(a_path).to_string()
        } else {
            a_path.to_string()
        };

        OutputDir {
            path,
            full: true,
            failure_message: "Failed to create output directory\n".to_string(),
            failure_exit_code: exit_codes::ERR_INVALID_OUTPUT_CONFLICT,
        }
    }
}

impl JobOutcome {
    /// An outcome with only an exit code (a C++ early `return CLI::EXIT_CODES::...`).
    pub fn exit(exit_code: i32) -> Self {
        JobOutcome {
            exit_code,
            ..JobOutcome::default()
        }
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::convertLayerArg( aLayerString, aBoard )`.
///
/// `user_layer_name` stands for `aBoard->GetLayerName( layer )` (the board's user layer names);
/// `None` is the C++ `aBoard == nullptr`. Tokens are split at `,` (`wxStringTokenizer` default
/// mode: empty tokens in the middle are returned, a trailing empty token is not), trimmed on
/// both ends, then looked up in the user names, the canonical file names (plus the `*`-groups)
/// and the GUI names, in that order. A miss reports `Invalid layer name '%s'`.
pub fn convert_layer_arg(
    layer_string: &str,
    user_layer_name: Option<&dyn Fn(PcbLayerId) -> String>,
    reporter: &mut Reporter,
) -> Lseq {
    let mut layer_user_masks: BTreeMap<String, Lset> = BTreeMap::new();
    let mut layer_masks: BTreeMap<String, Lset> = BTreeMap::new();
    let mut layer_gui_masks: BTreeMap<String, Lset> = BTreeMap::new();

    // Build list of layer names and their layer mask:
    for layer in Lset::all_layers_mask().seq().0 {
        // Add user layer name
        if let Some(user_name) = user_layer_name {
            layer_user_masks.insert(user_name(layer), Lset::from_layers(&[layer]));
        }

        // Add layer name used in pcb files
        layer_masks.insert(Lset::name(layer), Lset::from_layers(&[layer]));
        // Add layer name using GUI canonical layer name
        layer_gui_masks.insert(layer_name(layer), Lset::from_layers(&[layer]));
    }

    // Add list of grouped layer names used in pcb files
    layer_masks.insert("*".to_string(), Lset::all_layers_mask().clone());
    layer_masks.insert("*.Cu".to_string(), Lset::all_cu_mask().clone());
    layer_masks.insert("*In.Cu".to_string(), Lset::internal_cu_mask().clone());
    layer_masks.insert(
        "F&B.Cu".to_string(),
        Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]),
    );
    layer_masks.insert(
        "*.Adhes".to_string(),
        Lset::from_layers(&[PcbLayerId::BAdhes, PcbLayerId::FAdhes]),
    );
    layer_masks.insert(
        "*.Paste".to_string(),
        Lset::from_layers(&[PcbLayerId::BPaste, PcbLayerId::FPaste]),
    );
    layer_masks.insert(
        "*.Mask".to_string(),
        Lset::from_layers(&[PcbLayerId::BMask, PcbLayerId::FMask]),
    );
    layer_masks.insert(
        "*.SilkS".to_string(),
        Lset::from_layers(&[PcbLayerId::BSilkS, PcbLayerId::FSilkS]),
    );
    layer_masks.insert(
        "*.Fab".to_string(),
        Lset::from_layers(&[PcbLayerId::BFab, PcbLayerId::FFab]),
    );
    layer_masks.insert(
        "*.CrtYd".to_string(),
        Lset::from_layers(&[PcbLayerId::BCrtYd, PcbLayerId::FCrtYd]),
    );

    // Add list of grouped layer names using GUI canonical layer names
    layer_gui_masks.insert(
        "*.Adhesive".to_string(),
        Lset::from_layers(&[PcbLayerId::BAdhes, PcbLayerId::FAdhes]),
    );
    layer_gui_masks.insert(
        "*.Silkscreen".to_string(),
        Lset::from_layers(&[PcbLayerId::BSilkS, PcbLayerId::FSilkS]),
    );
    layer_gui_masks.insert(
        "*.Courtyard".to_string(),
        Lset::from_layers(&[PcbLayerId::BCrtYd, PcbLayerId::FCrtYd]),
    );

    let mut layer_mask = Lseq::default();

    if !layer_string.is_empty() {
        for raw in wx_tokenize_ret_empty(layer_string, ',') {
            let token = wx_trim(raw);

            let set = if let Some(set) = layer_user_masks.get(token) {
                Some(set)
            } else if let Some(set) = layer_masks.get(token) {
                Some(set)
            } else {
                layer_gui_masks.get(token)
            };

            match set {
                Some(set) => {
                    for layer in set.seq().0 {
                        layer_mask.0.push(layer);
                    }
                }
                None => {
                    reporter.report_undefined(format!("Invalid layer name '{token}'\n"));
                }
            }
        }
    }

    layer_mask
}

/// `wxStringTokenizer( aString, aDelim )` in its default mode for a non-whitespace delimiter
/// (`wxTOKEN_RET_EMPTY`): empty tokens between delimiters are returned, the empty token after
/// a trailing delimiter is not.
fn wx_tokenize_ret_empty(s: &str, delim: char) -> Vec<&str> {
    let mut tokens: Vec<&str> = s.split(delim).collect();

    if s.ends_with(delim) {
        tokens.pop();
    }

    tokens
}

/// `wxString::Trim( true ).Trim( false )`: strip the `isspace` characters (space, `\t`, `\n`,
/// `\v`, `\f`, `\r`) from both ends.
fn wx_trim(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

/// The layer selection of `PCBNEW_JOBS_HANDLER::JobExportGerbers` (L1784-1830): converts the
/// `--layers` / `--common-layers` arguments into the job's sequences and returns
/// `LSET( ResolvePlotLayers( enabled ) ) & enabled`, the set whose `UIOrder()` is plotted.
///
/// `board_plot_params` is `Some( (layerSelection, plotOnAllLayersSequence) )` of the board's
/// `PCB_PLOT_PARAMS` when `m_useBoardPlotParams` is set.
pub fn gerbers_layers_to_plot(
    job: &mut JobExportPcbGerbers,
    enabled_layers: &Lset,
    user_layer_name: Option<&dyn Fn(PcbLayerId) -> String>,
    board_plot_params: Option<(&Lset, &Lseq)>,
    reporter: &mut Reporter,
) -> Lset {
    let plot = &mut job.gerber.plot;

    // will be true if the user layer list is not empty
    let mut has_layer_list_specified = false;

    if let Some(arg_layers) = plot.arg_layers.clone() {
        if !arg_layers.is_empty() {
            plot.plot_layer_sequence = convert_layer_arg(&arg_layers, user_layer_name, reporter);
            has_layer_list_specified = true;
        } else {
            plot.plot_layer_sequence = Lset::all_layers_mask().seq_stackup_for_plotting();
        }
    }

    if let Some(arg_common_layers) = plot.arg_common_layers.clone() {
        plot.plot_on_all_layers_sequence =
            convert_layer_arg(&arg_common_layers, user_layer_name, reporter);
    }

    if job.use_board_plot_params {
        if let Some((layer_selection, plot_on_all)) = board_plot_params {
            // The board plot options are saved with all copper layers enabled, even those that
            // don't exist in the current stackup. This is done so the layers are automatically
            // enabled in the plot dialog when the user enables them. We need to filter out these
            // not-enabled layers here so we don't plot 32 layers when we only have 4, etc.
            let plot_layers = (layer_selection.clone() & Lset::all_non_cu_mask())
                | (enabled_layers.clone() & Lset::all_cu_mask().clone());
            plot.plot_layer_sequence = plot_layers.seq_stackup_for_plotting();
            plot.plot_on_all_layers_sequence = plot_on_all.clone();
        }
    } else {
        // default to the board enabled layers, but only if the user has not specifed a layer
        // list ( m_plotLayerSequence can be empty with a broken user layer list)
        if plot.plot_layer_sequence.0.is_empty()
            && !has_layer_list_specified
            && !plot.plot_all_copper_layers
        {
            plot.plot_layer_sequence = enabled_layers.seq_stackup_for_plotting();
        }
    }

    // Ensure layers to plot are restricted to enabled layers of the board to plot
    Lset::from(&plot.resolve_plot_layers(enabled_layers)) & enabled_layers.clone()
}

/// Port of `PCBNEW_JOBS_HANDLER::preparePlotLayers` without the zone refill (the caller does
/// it first, as the C++ does): converts the layer arguments and resolves the layers to plot.
/// `None` after reporting `At least one layer must be specified` when the list is empty.
pub fn prepare_plot_layers(
    job: &mut JobExportPcbPlot,
    enabled_layers: &Lset,
    user_layer_name: Option<&dyn Fn(PcbLayerId) -> String>,
    reporter: &mut Reporter,
) -> Option<Lseq> {
    if let Some(arg_layers) = job.arg_layers.clone() {
        job.plot_layer_sequence = convert_layer_arg(&arg_layers, user_layer_name, reporter);
    }

    if let Some(arg_common_layers) = job.arg_common_layers.clone() {
        job.plot_on_all_layers_sequence =
            convert_layer_arg(&arg_common_layers, user_layer_name, reporter);
    }

    let layers_to_plot = job.resolve_plot_layers(enabled_layers);

    if layers_to_plot.0.is_empty() {
        reporter.report("At least one layer must be specified\n", Severity::Error);
        return None;
    }

    Some(layers_to_plot)
}

/// The default DRC report name of `JobExportDrc` (L2869-2880): `<board name>-drc.json` or
/// `<board name>-drc.rpt`, without directory (`fn.GetFullName()`, so it lands in the working
/// directory, not next to the board).
pub fn drc_default_output_name(board_file_name: &str, format: RcOutputFormat) -> String {
    let ext = match format {
        // FILEEXT::JsonFileExtension
        RcOutputFormat::Json => "json",
        // FILEEXT::ReportFileExtension
        RcOutputFormat::Report => "rpt",
    };

    format!("{}-drc.{}", file_stem(board_file_name), ext)
}

/// The default drill report path of `JobExportDrill` (L2370-2378):
/// `<outPath>/<board name>-drill.rpt`.
pub fn drill_default_report_path(out_path: &str, board_file_name: &str) -> String {
    // wxFileName::DirName( outPath ); SetFullName( brd->GetFileName() ) keeps only the name
    let name = format!("{}-drill.rpt", file_stem(file_full_name(board_file_name)));

    if out_path.is_empty() {
        name
    } else if out_path.ends_with('/') {
        format!("{out_path}{name}")
    } else {
        format!("{out_path}/{name}")
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::resolveJobOutputPath( aJob, aBoard, aDrawingSheet )`: copy
/// the title block into the job, apply the `-D` overrides to the project text variables
/// (`PROJECT::ApplyTextVars`: they stay applied, so board texts resolve them too),
/// `BOARD::SynchronizeProperties`, and resolve the output path with the project's
/// `TextVarResolver`.
///
/// The override drawing sheet (`loadOverrideDrawingSheet`, `--drawing-sheet`) is loaded by the
/// plot jobs right after this ([`Session::load_override_drawing_sheet`]).
fn resolve_job_output_path(job: &mut crate::jobs::Job, session: &mut Session) -> String {
    job.set_title_block(session.board.titles.clone());

    let board = &mut session.board;

    if let Some(project) = board.project.as_mut() {
        project.apply_text_vars(&job.var_overrides);
    }

    board.synchronize_properties();

    let clock = board.text_var_clock.clone();

    match board.project.as_ref() {
        Some(project) => {
            let resolver = |token: &mut String| project.text_var_resolver(&clock, token);
            job.full_output_path_with(&clock, Some(&resolver))
        }
        // GetFullOutputPath( nullptr ): no project (Session::new_empty)
        None => job.full_output_path_with(&clock, None),
    }
}

/// The C++ `EDA_UNITS` of `JobExportDrc`'s `switch( drcJob->m_units )`.
pub fn drc_units(units: RcUnits) -> kicad_core::EdaUnits {
    match units {
        RcUnits::Inch => kicad_core::EdaUnits::Inch,
        RcUnits::Mils => kicad_core::EdaUnits::Mils,
        RcUnits::Mm => kicad_core::EdaUnits::Mm,
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportDrc`.
///
/// The DRC itself (violation handler, exclusions, `RunTests`, the marker providers and the
/// report writers) is `kicad_drc::run_flow::run_drc_job`. Checks the port cannot run yet are
/// listed in the report (`kicad_drc::run_flow::CheckNotRun`) and, one line each, as a
/// warning on the reporter after the C++ messages; without such checks the messages and the
/// report are those of the C++.
///
/// `--schematic-parity` behaves as a build without the schematic kiface: without a schematic
/// file "Failed to fetch schematic netlist for parity tests.", with one the netlister is
/// missing and the annotation message is reported; parity is then off, as in C++.
pub fn run_drc(
    session: &mut Session,
    job: &mut JobPcbDrc,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    use kicad_drc::run_flow::{DrcJobOptions, DrcReportFormat, RunTestsOptions, run_drc_job};

    // Running DRC requires libraries be loaded, so make sure they have been: the host
    // preloads them into `session.footprint_libraries` (`Session::load_footprint_libraries`).

    if job.rc.job.configured_output_path().is_empty() {
        let name = drc_default_output_name(&session.source_name, job.rc.format);
        job.rc.job.set_working_output_path(&name);
    }

    let out_path = resolve_job_output_path(&mut job.rc.job, session);

    // PATHS::EnsurePathExists( outPath, true ): the host creates the directory
    // (JobOutcome::output_dir) before it writes the file.
    let output_dir = OutputDir::ensure_path_exists(&out_path, true);

    let units = drc_units(job.rc.units);

    if session.drc_engine.is_none() {
        session.drc_engine = Some(kicad_drc::engine::DrcEngine::new());
    }

    let rules_valid = session
        .drc_engine
        .as_ref()
        .is_some_and(|engine| engine.rules_valid());

    if !rules_valid {
        // drcEngine->InitEngine( brd->GetDesignRulesPath() ): the project's rule file path
        // (absolute in kicad-cli, see LoadInputs::project_path)
        let mut rules_path = session.board.get_design_rules_path();

        if rules_path.is_empty() {
            // a session without a project (Session::new_empty)
            rules_path = crate::board_loader::with_extension(&session.source_name, "kicad_dru");
        }
        let layer_choices = kicad_drc::rule_parser::board_layer_choices(&session.board);
        let tuning_profiles = crate::board_loader::tuning_profiles(&session.board);
        let engine = session.drc_engine.as_mut().expect("engine");

        if let Err(error) = engine.init_engine(
            &mut session.board,
            session
                .rules_text
                .as_deref()
                .map(|t| (t, rules_path.as_str())),
            layer_choices,
            &tuning_profiles,
            None,
        ) {
            reporter.report(
                format!(
                    "DRC incomplete: could not compile custom design rules.\n{}\n",
                    error.what()
                ),
                Severity::Error,
            );
            return Ok(JobOutcome::exit(exit_codes::ERR_INVALID_INPUT_FILE));
        }
    }

    // drcEngine->SetDrawingSheet( getDrawingSheetProxyView( brd ) ): the page settings, project
    // and title block of the board; the misc provider builds the draw item list
    // (`kicad_plot::drawing_sheet::drc_drawing_sheet_texts`).
    {
        let board = &session.board;
        let project = |token: &mut String| -> bool {
            board
                .project
                .as_ref()
                .is_some_and(|p| p.text_var_resolver(&board.text_var_clock, token))
        };
        let project: Option<&kicad_core::text_vars::TextVarResolver<'_>> =
            board.project.as_ref().map(|_| &project as _);
        let texts = kicad_plot::drawing_sheet::drc_drawing_sheet_texts(
            &mut session.drawing_sheet,
            &board.paper,
            &board.titles,
            project,
            &board.text_var_clock,
        );
        let drawing_sheet = kicad_drc::providers::misc::DrawingSheetTexts {
            // The DS_PROXY_VIEW_ITEM's own (random) KIID
            proxy_uuid: Default::default(),
            texts: texts
                .into_iter()
                .map(|t| kicad_drc::providers::misc::DrawingSheetText {
                    text: t.text,
                    shown_text: t.shown_text,
                    position: t.position,
                })
                .collect(),
        };

        if let Some(engine) = session.drc_engine.as_mut() {
            engine.set_drawing_sheet(Some(drawing_sheet));
        }
    }

    let mut check_parity = job.parity;

    if check_parity {
        let annotate_msg = "Schematic parity tests require a fully annotated schematic.";

        if !job.schematic_exists {
            reporter.report(
                "Failed to fetch schematic netlist for parity tests.\n",
                Severity::Error,
            );
        } else {
            // GetKifaceService<KIFACE_NETLIST_SCHEMATIC_SERVICE>() is null without the
            // schematic kiface: netlist_str stays annotateMsg.
            reporter.report(format!("{annotate_msg}\n"), Severity::Error);
        }

        check_parity = false;
    }

    if job.refill_zones {
        session.refill_zones()?;
    }

    let tuning_profiles = crate::board_loader::tuning_profiles(&session.board);
    let options = DrcJobOptions {
        run: RunTestsOptions {
            units,
            report_all_track_errors: job.report_all_track_errors,
            test_footprints: check_parity,
            tuning_profiles: &tuning_profiles,
            footprint_libraries: session.footprint_libraries.as_ref(),
        },
        severities: job.rc.severity,
        format: match job.rc.format {
            RcOutputFormat::Json => DrcReportFormat::Json,
            RcOutputFormat::Report => DrcReportFormat::Text,
        },
        date: job.report_date.clone(),
        kicad_version: kicad_drc::report::KICAD_MAJOR_MINOR_PATCH_VERSION.to_string(),
        marker_uuid_seed: job.marker_uuid_seed,
    };

    let engine = session.drc_engine.as_ref().expect("engine");
    let result = run_drc_job(
        engine,
        &mut session.board,
        &mut session.connectivity,
        &options,
    );

    reporter.report(
        format!("Found {} violations\n", result.violations),
        Severity::Info,
    );
    reporter.report(
        format!("Found {} unconnected items\n", result.unconnected_items),
        Severity::Info,
    );

    if check_parity {
        reporter.report(
            format!("Found {} schematic parity issues\n", result.parity_issues),
            Severity::Info,
        );
    }

    let mut outcome = JobOutcome {
        exit_code: exit_codes::SUCCESS,
        files: vec![OutputFile {
            name: out_path.clone(),
            contents: result.report.clone().into_bytes(),
        }],
        report_text: Some(result.report.clone()),
        output_dir: Some(output_dir),
    };

    job.rc.job.add_output(&out_path);

    reporter.report(
        format!("Saved DRC Report to {out_path}\n"),
        Severity::Action,
    );

    for check in &result.checks_not_run {
        reporter.report(
            format!(
                "DRC check '{}' was not run: {}\n",
                check.provider, check.reason
            ),
            Severity::Warning,
        );
    }

    if let Some(code) = drc_save_board(session, job, reporter, &mut outcome)? {
        return Ok(JobOutcome::exit(code));
    }

    if job.rc.exit_code_violations
        && (result.violations > 0 || result.unconnected_items > 0 || result.parity_issues > 0)
    {
        outcome.exit_code = exit_codes::ERR_RC_VIOLATIONS;
    }

    Ok(outcome)
}

/// The `if( drcJob->m_refillZones && drcJob->m_saveBoard )` block at the end of
/// `PCBNEW_JOBS_HANDLER::JobExportDrc` (after the report is written):
/// `BOARD_LOADER::SaveBoard( drcJob->m_filename, *brd )` ([`crate::board_loader::save_board`]),
/// "Saved board" / "Failed to save board." on the reporter. The saved board is pushed onto
/// `outcome.files` under the input file name (the C++ overwrites the board in place).
///
/// Returns `Some( exit code )` for the C++ early `return` (save failure), `None` to go on
/// with the `--exit-code-violations` check.
pub fn drc_save_board(
    session: &mut Session,
    job: &JobPcbDrc,
    reporter: &mut Reporter,
    outcome: &mut JobOutcome,
) -> Result<Option<i32>, SessionError> {
    if !(job.refill_zones && job.save_board) {
        return Ok(None);
    }

    match crate::board_loader::save_board(&mut session.board, &mut session.connectivity) {
        Ok(text) => {
            outcome.files.push(OutputFile {
                name: job.rc.filename.clone(),
                contents: text.into_bytes(),
            });
            reporter.report("Saved board\n", Severity::Action);
            Ok(None)
        }
        Err(SessionError::Save { .. }) => {
            reporter.report("Failed to save board.\n", Severity::Error);
            Ok(Some(exit_codes::ERR_INVALID_OUTPUT_CONFLICT))
        }
        Err(e) => Err(e),
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportGerbers`.
pub fn run_export_gerbers(
    session: &mut Session,
    job: &mut JobExportPcbGerbers,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    if !job.gerber.plot.variant.is_empty() {
        session.board.set_current_variant(&job.gerber.plot.variant);
    }

    let out_path = resolve_job_output_path(&mut job.gerber.plot.job, session);
    // resolveJobOutputPath( aJob, brd, &aGerberJob->m_drawingSheet )
    session.load_override_drawing_sheet(&job.gerber.plot.drawing_sheet, reporter);

    // PATHS::EnsurePathExists( outPath, false ): the host creates the directory
    // (JobOutcome::output_dir) before it writes the files.
    let output_dir = OutputDir::ensure_path_exists(&out_path, false);

    if job.gerber.plot.check_zones_before_plot {
        session.refill_zones()?;
    }

    let enabled = session.board.enabled_layers();
    let board = &session.board;
    let user_name = |layer: PcbLayerId| board.layer_name(layer);
    let plot_params = &board.plot_options;
    let plot_on_all = Lseq(plot_params.plot_on_all_layers_sequence.clone());
    let layers_to_plot = gerbers_layers_to_plot(
        job,
        &enabled,
        Some(&user_name),
        Some((&plot_params.layer_selection, &plot_on_all)),
        reporter,
    );

    // Once for the run, not per file, so an update rebuilds each chart once
    crate::board_loader::refresh_generated_tables(&mut session.board)?;

    let fab_job = fab_gerbers_job(job);
    let env = kicad_fab::pcbplot::PlotEnv {
        build_version: crate::build_version::build_version().to_string(),
        creation_date: job.gerber.plot.creation_date,
        drawing_sheet: session.drawing_sheet.clone(),
    };

    let mut ctx =
        kicad_pcb::ctx::BoardCtx::bare(&session.board).with_connectivity(&session.connectivity);

    if let Some(engine) = session.drc_engine.as_ref() {
        ctx = ctx.with_rules(engine);
    }

    // `plotOpts = boardPlotOptions` with a stored HPGL format: StartPlotBoard fails for every
    // layer (F-01)
    if job.use_board_plot_params
        && session.board.plot_options.format == kicad_core::plot_enums::PlotFormat::Hpgl
    {
        let mut outcome =
            gerbers_start_plot_fails(job, ctx, &layers_to_plot, &out_path, &env, reporter);
        outcome.output_dir = Some(output_dir);
        return Ok(outcome);
    }

    let mut fab_reporter = FabReporter(reporter);
    let result = kicad_fab::jobs_handler::job_export_gerbers(
        ctx,
        &fab_job,
        &layers_to_plot,
        &out_path,
        &env,
        Some(&mut fab_reporter),
    );

    for output in &result.outputs {
        job.gerber.plot.job.add_output(output);
    }

    let files: Vec<OutputFile> = result
        .files
        .into_iter()
        .map(|f| OutputFile {
            name: f.name,
            contents: f.contents,
        })
        .collect();

    let exit_code = if result.success {
        exit_codes::OK
    } else {
        exit_codes::ERR_INVALID_OUTPUT_CONFLICT
    };

    match result.error {
        None => Ok(JobOutcome {
            exit_code,
            files,
            report_text: None,
            output_dir: Some(output_dir),
        }),
        Some(err) => Err(SessionError::not_yet_ported(format!("kicad-fab: {err}"))),
    }
}

/// The layer loop of `PCBNEW_JOBS_HANDLER::JobExportGerbers` (L1817-1919) for a run in which
/// `StartPlotBoard` returns `nullptr` for every layer: `m_useBoardPlotParams` with a stored
/// `PLOT_FORMAT::HPGL` (`plot_board_layers.cpp` L1456-1458). Per layer the file name is built
/// and registered in the job file, `wxLogError( "HPGL plotting is no longer supported as of
/// KiCad 10.0" )` and `Failed to plot to '<file>'.` are reported and the exit code becomes
/// `ERR_INVALID_OUTPUT_CONFLICT`; the Gerber job file is still written.
///
/// kicad-fab's `job_export_gerbers` stops at the first layer the port cannot plot (its other
/// errors are formats outside the port), so this case runs here. The `wxLogError` line goes to
/// the reporter as an error, formatted as kicad-cli's log target prints it
/// (`HH:MM:SS: Error: <text>`, the time of `creation_date`), so that it keeps its place among
/// the `Failed to plot` lines on stderr.
fn gerbers_start_plot_fails(
    job: &mut JobExportPcbGerbers,
    ctx: kicad_pcb::ctx::BoardCtx<'_>,
    layers_to_plot: &Lset,
    out_path: &str,
    env: &kicad_fab::pcbplot::PlotEnv,
    reporter: &mut Reporter,
) -> JobOutcome {
    use kicad_fab::exporters::gerber_jobfile_writer::GerberJobfileWriter;
    use kicad_fab::pcb_plotter::build_plot_file_name;
    use kicad_fab::wx_file_name::WxFileName;

    let brd = ctx.board;
    let plot_opts = &brd.plot_options;
    let date = &job.gerber.plot.creation_date;
    let mut jobfile_writer = GerberJobfileWriter::new(ctx, None);
    let mut files = Vec::new();
    let mut exit_code = exit_codes::OK;

    for layer in layers_to_plot.ui_order().0 {
        // Pick the basename from the board file
        let mut fn_ = WxFileName::new(&brd.file_name);
        let layer_name = brd.layer_name(layer);

        let file_ext = if plot_opts.use_gerber_protel_extensions {
            kicad_fab::pcbplot::gerber_protel_extension(layer.0)
        } else {
            // FILEEXT::GerberFileExtension
            kicad_plot::GerberPlotter::default_file_extension().to_string()
        };

        build_plot_file_name(&mut fn_, out_path, &layer_name, &file_ext);
        jobfile_writer.add_gbr_file(layer, &fn_.full_name());

        // StartPlotBoard: case PLOT_FORMAT::HPGL
        reporter.report(
            format!(
                "{:02}:{:02}:{:02}: Error: HPGL plotting is no longer supported as of KiCad 10.0\n",
                date.hour, date.minute, date.second
            ),
            Severity::Error,
        );
        reporter.report(
            format!("Failed to plot to '{}'.\n", fn_.full_path()),
            Severity::Error,
        );
        exit_code = exit_codes::ERR_INVALID_OUTPUT_CONFLICT;
    }

    if job.create_jobs_file {
        let mut fn_ = WxFileName::new(&brd.file_name);

        // Build gerber job file from basename
        build_plot_file_name(
            &mut fn_,
            out_path,
            "job",
            kicad_fab::jobs_handler::GERBER_JOB_FILE_EXTENSION,
        );
        let full_path = fn_.full_path();
        let job_file = jobfile_writer.create_job_file(&full_path, env);
        files.push(OutputFile {
            name: job_file.name,
            contents: job_file.contents,
        });
        job.gerber.plot.job.add_output(&full_path);
    }

    JobOutcome {
        exit_code,
        files,
        report_text: None,
        output_dir: None,
    }
}

/// `m_reporter` as the `REPORTER` of kicad-fab.
struct FabReporter<'a>(&'a mut Reporter);

impl kicad_fab::Reporter for FabReporter<'_> {
    fn report(&mut self, text: &str, severity: Severity) {
        self.0.report(text, severity);
    }
}

/// The session's `JOB_EXPORT_PCB_GERBERS` as kicad-fab's option struct (same members; the
/// layer sequences after [`gerbers_layers_to_plot`]).
fn fab_gerbers_job(
    job: &JobExportPcbGerbers,
) -> kicad_fab::jobs::job_export_pcb_gerbers::JobExportPcbGerbers {
    use kicad_fab::jobs::job_export_pcb_gerbers as fab;

    let plot = fab_plot_job(&job.gerber.plot);

    fab::JobExportPcbGerbers {
        gerber: fab::JobExportPcbGerber {
            plot,
            include_netlist_attributes: job.gerber.include_netlist_attributes,
            use_x2_format: job.gerber.use_x2_format,
            disable_aperture_macros: job.gerber.disable_aperture_macros,
            use_protel_file_extension: job.gerber.use_protel_file_extension,
            precision: job.gerber.precision,
        },
        use_board_plot_params: job.use_board_plot_params,
        create_jobs_file: job.create_jobs_file,
    }
}

/// The session's `JOB_EXPORT_PCB_PLOT` as kicad-fab's option struct (same members).
fn fab_plot_job(p: &JobExportPcbPlot) -> kicad_fab::jobs::job_export_pcb_plot::JobExportPcbPlot {
    use kicad_fab::jobs::job_export_pcb_plot as fab_plot;

    let plot_format = match p.plot_format {
        crate::jobs::JobPlotFormat::Hpgl => fab_plot::JobPlotFormat::Hpgl,
        crate::jobs::JobPlotFormat::Gerber => fab_plot::JobPlotFormat::Gerber,
        crate::jobs::JobPlotFormat::Post => fab_plot::JobPlotFormat::Post,
        crate::jobs::JobPlotFormat::Dxf => fab_plot::JobPlotFormat::Dxf,
        crate::jobs::JobPlotFormat::Pdf => fab_plot::JobPlotFormat::Pdf,
        crate::jobs::JobPlotFormat::Svg => fab_plot::JobPlotFormat::Svg,
        crate::jobs::JobPlotFormat::Png => fab_plot::JobPlotFormat::Png,
    };

    fab_plot::JobExportPcbPlot {
        plot_format,
        color_theme: p.color_theme.clone(),
        mirror: p.mirror,
        black_and_white: p.black_and_white,
        negative: p.negative,
        scale: p.scale,
        sketch_pads_on_fab_layers: p.sketch_pads_on_fab_layers,
        hide_dnp_fps_on_fab_layers: p.hide_dnp_fps_on_fab_layers,
        sketch_dnp_fps_on_fab_layers: p.sketch_dnp_fps_on_fab_layers,
        crossout_dnp_fps_on_fab_layers: p.crossout_dnp_fps_on_fab_layers,
        plot_footprint_values: p.plot_footprint_values,
        plot_ref_des: p.plot_ref_des,
        plot_drawing_sheet: p.plot_drawing_sheet,
        subtract_solder_mask_from_silk: p.subtract_solder_mask_from_silk,
        plot_pad_numbers: p.plot_pad_numbers,
        plot_layer_sequence: p.plot_layer_sequence.clone(),
        plot_all_copper_layers: p.plot_all_copper_layers,
        plot_on_all_layers_sequence: p.plot_on_all_layers_sequence.clone(),
        drill_shape_option: p.drill_shape_option,
        use_drill_origin: p.use_drill_origin,
        configured_output_path: p.job.configured_output_path().to_string(),
        var_overrides: p.job.var_overrides.clone(),
    }
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportDrill`.
///
/// The writers (`EXCELLON_WRITER` / `GERBER_WRITER`: option mapping, drill and map files,
/// `GenDrillReportFile`) are `kicad_fab::jobs_handler::job_export_drill`; this function maps
/// the job onto it (`resolveJobOutputPath`, the report path through
/// `JOB::ResolveOutputPath( reportPathSetting, false, project )`, the build version and the
/// creation date of the files) and records `AddOutput` in the C++ order.
///
/// A writer failure returns `ERR_INVALID_OUTPUT_CONFLICT` as in C++ (the files written before
/// the failure are still returned); a part kicad-fab has not ported (e.g. a PDF drill map) is
/// `NotYetPorted`.
pub fn run_export_drill(
    session: &mut Session,
    job: &mut JobExportPcbDrill,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    use kicad_fab::jobs::job_export_pcb_drill as fab;

    let out_path = resolve_job_output_path(&mut job.job, session);

    // PATHS::EnsurePathExists( outPath ): the host creates the directory
    // (JobOutcome::output_dir) before it writes the files. The report path is not created:
    // GenDrillReportFile fails to open a file in a directory that does not exist (F-04).
    let output_dir = OutputDir::ensure_path_exists(&out_path, false);

    let fab_job = fab::JobExportPcbDrill {
        excellon_mirror_y: job.excellon_mirror_y,
        excellon_minimal_header: job.excellon_minimal_header,
        excellon_combine_pth_npth: job.excellon_combine_pth_npth,
        excellon_oval_drill_route: job.excellon_oval_drill_route,
        format: match job.format {
            DrillFormat::Excellon => fab::DrillFormat::Excellon,
            DrillFormat::Gerber => fab::DrillFormat::Gerber,
        },
        drill_origin: match job.drill_origin {
            crate::jobs::DrillOrigin::Abs => fab::DrillOrigin::Abs,
            crate::jobs::DrillOrigin::Plot => fab::DrillOrigin::Plot,
        },
        drill_units: match job.drill_units {
            crate::jobs::DrillUnits::Inch => fab::DrillUnits::Inch,
            crate::jobs::DrillUnits::Mm => fab::DrillUnits::Mm,
        },
        zero_format: match job.zero_format {
            crate::jobs::ZerosFormat::Decimal => fab::ZerosFormat::Decimal,
            crate::jobs::ZerosFormat::SuppressLeading => fab::ZerosFormat::SuppressLeading,
            crate::jobs::ZerosFormat::SuppressTrailing => fab::ZerosFormat::SuppressTrailing,
            crate::jobs::ZerosFormat::KeepZeros => fab::ZerosFormat::KeepZeros,
        },
        map_format: match job.map_format {
            crate::jobs::MapFormat::Postscript => fab::MapFormat::Postscript,
            crate::jobs::MapFormat::GerberX2 => fab::MapFormat::GerberX2,
            crate::jobs::MapFormat::Dxf => fab::MapFormat::Dxf,
            crate::jobs::MapFormat::Svg => fab::MapFormat::Svg,
            crate::jobs::MapFormat::Pdf => fab::MapFormat::Pdf,
        },
        gerber_precision: job.gerber_precision,
        generate_map: job.generate_map,
        generate_tenting: job.generate_tenting,
        generate_report: job.generate_report,
        report_path: job.report_path.clone(),
    };

    let env = fab::DrillExportEnv {
        build_version: crate::build_version::build_version().to_string(),
        creation_date: job.creation_date,
    };

    // aDrillJob->ResolveOutputPath( reportPathSetting, false, brd->GetProject() ): the
    // project's text variables (with the -D overrides resolveJobOutputPath applied)
    let board = &session.board;
    let resolver_job = &job.job;
    let resolve = |path: &str| {
        let clock = &board.text_var_clock;

        match board.project.as_ref() {
            Some(project) => {
                let resolver = |token: &mut String| project.text_var_resolver(clock, token);
                resolver_job.resolve_output_path_with(path, false, clock, Some(&resolver))
            }
            None => resolver_job.resolve_output_path_with(path, false, clock, None),
        }
    };

    let mut ctx =
        kicad_pcb::ctx::BoardCtx::bare(&session.board).with_connectivity(&session.connectivity);

    if let Some(engine) = session.drc_engine.as_ref() {
        ctx = ctx.with_rules(engine);
    }

    let mut fab_reporter = FabReporter(reporter);
    let result = kicad_fab::jobs_handler::job_export_drill(
        ctx,
        &fab_job,
        &out_path,
        &env,
        Some(&resolve),
        Some(&mut fab_reporter),
    );

    if let Some(err) = result.error {
        return Err(SessionError::not_yet_ported(format!("kicad-fab: {err}")));
    }

    for output in &result.outputs {
        job.job.add_output(output);
    }

    let files: Vec<OutputFile> = result
        .files
        .into_iter()
        .map(|f| OutputFile {
            name: f.name,
            contents: f.contents,
        })
        .collect();

    Ok(JobOutcome {
        exit_code: if result.success {
            exit_codes::OK
        } else {
            exit_codes::ERR_INVALID_OUTPUT_CONFLICT
        },
        files,
        report_text: None,
        output_dir: Some(output_dir),
    })
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportSvg`.
pub fn run_export_svg(
    session: &mut Session,
    job: &mut JobExportPcbSvg,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    if !job.plot.variant.is_empty() {
        session.board.set_current_variant(&job.plot.variant);
    }

    // resolvePlotOutputPath: single output without a configured path -> <board name>.svg
    let is_single = job.gen_mode == crate::jobs::SvgGenMode::Single;

    if is_single && job.plot.job.configured_output_path().is_empty() {
        let name = format!("{}.svg", file_stem(&session.source_name));
        job.plot.job.set_working_output_path(&name);
    }

    let out_path = resolve_job_output_path(&mut job.plot.job, session);
    // resolveJobOutputPath( aJob, brd, &aSvgJob->m_drawingSheet )
    session.load_override_drawing_sheet(&job.plot.drawing_sheet, reporter);

    // PATHS::EnsurePathExists( outPath, isSingle ): the host creates the directory
    // (JobOutcome::output_dir) before it writes the files.
    let output_dir = OutputDir::ensure_path_exists(&out_path, is_single);

    if job.plot.check_zones_before_plot {
        session.refill_zones()?;
    }

    let enabled = session.board.enabled_layers();
    let board = &session.board;
    let user_name = |layer: PcbLayerId| board.layer_name(layer);

    let Some(layers_to_plot) =
        prepare_plot_layers(&mut job.plot, &enabled, Some(&user_name), reporter)
    else {
        return Ok(JobOutcome {
            output_dir: Some(output_dir),
            ..JobOutcome::exit(exit_codes::ERR_ARGS)
        });
    };

    // PCB_PLOTTER::Plot: RefreshGeneratedTables( *m_board ) before plotting (it needs the
    // mutable board, so it runs here)
    crate::board_loader::refresh_generated_tables(&mut session.board)?;

    let fab_job = kicad_fab::jobs::job_export_pcb_svg::JobExportPcbSvg {
        plot: fab_plot_job(&job.plot),
        fit_page_to_board: job.fit_page_to_board,
        precision: job.precision,
        gen_mode: match job.gen_mode {
            crate::jobs::SvgGenMode::Single => {
                kicad_fab::jobs::job_export_pcb_svg::SvgGenMode::Single
            }
            crate::jobs::SvgGenMode::Multi => {
                kicad_fab::jobs::job_export_pcb_svg::SvgGenMode::Multi
            }
        },
    };
    let env = kicad_fab::pcbplot::PlotEnv {
        build_version: crate::build_version::build_version().to_string(),
        creation_date: job.plot.creation_date,
        drawing_sheet: session.drawing_sheet.clone(),
    };

    let mut ctx =
        kicad_pcb::ctx::BoardCtx::bare(&session.board).with_connectivity(&session.connectivity);

    if let Some(engine) = session.drc_engine.as_ref() {
        ctx = ctx.with_rules(engine);
    }

    let mut fab_reporter = FabReporter(reporter);
    let result = kicad_fab::jobs_handler::job_export_svg(
        ctx,
        &fab_job,
        &layers_to_plot.0,
        &out_path,
        &env,
        Some(&mut fab_reporter),
    );

    if let Some(err) = result.error {
        return Err(SessionError::not_yet_ported(format!("kicad-fab: {err}")));
    }

    // plotJob: a failed plot returns before AddOutput
    if !result.success {
        return Ok(JobOutcome {
            output_dir: Some(output_dir),
            ..JobOutcome::exit(exit_codes::ERR_UNKNOWN)
        });
    }

    for output in &result.outputs {
        job.plot.job.add_output(output);
    }

    let files: Vec<OutputFile> = result
        .files
        .into_iter()
        .map(|f| OutputFile {
            name: f.name,
            contents: f.contents,
        })
        .collect();

    Ok(JobOutcome {
        exit_code: exit_codes::OK,
        files,
        report_text: None,
        output_dir: Some(output_dir),
    })
}

/// Port of `PCBNEW_JOBS_HANDLER::JobUpgrade` for a loaded session (`getBoard`:
/// [`Session::load_board`], the full `BOARD_LOADER::Load` sequence): decides whether to save
/// and returns the re-saved board (`PCB_IO_KICAD_SEXPR::SaveBoard`, no `BOARD_LOADER::SaveBoard`
/// pre-steps) as the one output file (named like the input: the C++ overwrites the board in
/// place). `RefreshGeneratedTables` rebuilding a table marks the board as changed.
pub fn run_pcb_upgrade(
    session: &mut Session,
    job: &mut JobPcbUpgrade,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    let mut should_save = job.force;

    if session.board.file_format_version_at_load < SEXPR_BOARD_FILE_VERSION {
        should_save = true;
    }

    // A chart is derived data, so the upgrade brings every one up to date and saves if
    // that changed anything
    if crate::board_loader::refresh_generated_tables(&mut session.board)? > 0 {
        should_save = true;
    }

    if should_save {
        let text = match session.save() {
            Ok(text) => text,
            Err(SessionError::Save { message }) => {
                reporter.report(
                    format!("Error saving board file '{}'.\n{}", job.filename, message),
                    Severity::Error,
                );
                return Ok(JobOutcome::exit(exit_codes::ERR_UNKNOWN));
            }
            Err(e) => return Err(e),
        };
        reporter.report(
            "Successfully saved board file using the latest format\n",
            Severity::Info,
        );

        return Ok(JobOutcome {
            exit_code: exit_codes::SUCCESS,
            files: vec![OutputFile {
                name: job.filename.clone(),
                contents: text.into_bytes(),
            }],
            report_text: None,
            // the board is saved in place: no directory to create
            output_dir: None,
        });
    }

    reporter.report("Board file was not updated\n", Severity::Error);
    Ok(JobOutcome::exit(exit_codes::SUCCESS))
}

/// A footprint library as the host read it: `(file name, text)` of each `.kicad_mod` of the
/// `.pretty` directory, in directory order.
pub type FootprintLibrary = Vec<(String, String)>;

/// Port of `PCBNEW_JOBS_HANDLER::JobExportFpUpgrade` for a KiCad (`.pretty`) library the host
/// has read. The checks on the filesystem (`Output path must not conflict with existing path`,
/// `Footprint library path does not exist or is not accessible`, legacy formats) are done by
/// the host before calling.
pub fn run_fp_upgrade(
    library: &FootprintLibrary,
    job: &mut JobFpUpgrade,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    use kicad_pcb::io::writer::CTL_FOR_LIBRARY;
    use kicad_pcb::io::{FormatOptions, ParseOptions, SEXPR_FOOTPRINT_LIB_FILE_VERSION};

    // FP_CACHE::Load: every file of the library, keyed by its base name (the first file of a
    // name wins, boost::ptr_map::insert). Errors are queued so that only the files that fail
    // to parse don't get loaded; any error fails the load.
    let mut fp_lib: BTreeMap<
        String,
        (String, kicad_pcb::board::Board, kicad_pcb::ids::FootprintId),
    > = BTreeMap::new();
    let mut cache_error = String::new();

    for (full_name, text) in library {
        let full_path = format!("{}/{}", job.library_path, full_name);

        match kicad_pcb::io::parse_footprint_file(text, &full_path, &ParseOptions::default()) {
            Ok((mut holder, _warnings)) => {
                // wxFileName::GetName(): the name without the extension
                let fp_name = match full_name.rfind('.') {
                    Some(dot) if dot > 0 => full_name[..dot].to_string(),
                    _ => full_name.clone(),
                };

                let Some(fp) = holder.footprints().next().map(|(id, _)| id) else {
                    continue;
                };

                // The parser's entry has no file name, and saving uses the one cached here
                holder.modify_footprint(fp, |footprint, _| {
                    footprint.fpid = kicad_core::LibId::from_parts("", &fp_name);
                });

                fp_lib
                    .entry(fp_name)
                    .or_insert((full_name.clone(), holder, fp));
            }
            Err(e) => {
                if !cache_error.is_empty() {
                    cache_error.push_str("\n\n");
                }

                cache_error.push_str(&format!("Unable to read file '{full_path}'\n"));
                cache_error.push_str(e.what());
            }
        }
    }

    if !cache_error.is_empty() {
        reporter.report("Unable to load library\n", Severity::Error);
        return Ok(JobOutcome::exit(exit_codes::ERR_UNKNOWN));
    }

    let mut should_save = job.force;

    for (_, holder, fp) in fp_lib.values() {
        let version = holder
            .footprint(*fp)
            .map_or(0, |f| f.file_format_version_at_load);

        if version < SEXPR_FOOTPRINT_LIB_FILE_VERSION {
            should_save = true;
        }
    }

    if !should_save {
        reporter.report("Footprint library was not updated\n", Severity::Error);
        return Ok(JobOutcome::exit(exit_codes::SUCCESS));
    }

    // fpLib.SetPath( output ) when given, then FP_CACHE::Save with the PCB_IO_KICAD_SEXPR(
    // CTL_FOR_LIBRARY ) owner: each footprint to <path>/<its file name>, in key order.
    let lib_path = if job.output_library_path.is_empty() {
        &job.library_path
    } else {
        &job.output_library_path
    };

    let options = FormatOptions {
        ctl: CTL_FOR_LIBRARY,
        ..FormatOptions::default()
    };

    // FP_CACHE::Save: `if( !m_lib_path.DirExists() && !m_lib_path.Mkdir() )` (the library
    // directory only, no wxPATH_MKDIR_FULL) throws, JobExportFpUpgrade reports "Unable to save
    // library" and returns ERR_UNKNOWN
    let output_dir = OutputDir {
        path: lib_path.clone(),
        full: false,
        failure_message: "Unable to save library\n".to_string(),
        failure_exit_code: exit_codes::ERR_UNKNOWN,
    };

    let mut files = Vec::new();

    for (full_name, holder, fp) in fp_lib.values() {
        match kicad_pcb::io::format_footprint_lib_file(holder, *fp, &options) {
            Ok((text, _)) => files.push(OutputFile {
                name: format!("{lib_path}/{full_name}"),
                contents: text.into_bytes(),
            }),
            Err(_) => {
                // The files saved before the failure stay written, as in C++.
                reporter.report("Unable to save library\n", Severity::Error);
                return Ok(JobOutcome {
                    exit_code: exit_codes::ERR_UNKNOWN,
                    files,
                    report_text: None,
                    output_dir: Some(output_dir),
                });
            }
        }
    }

    Ok(JobOutcome {
        exit_code: exit_codes::SUCCESS,
        files,
        report_text: None,
        output_dir: Some(output_dir),
    })
}

/// The board's directory part of the session's file name (`wxFileName::GetPath()`).
pub fn board_dir(session: &Session) -> &str {
    file_dir(&session.source_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_names() {
        assert_eq!(
            drc_default_output_name("/x/y/board.kicad_pcb", RcOutputFormat::Report),
            "board-drc.rpt"
        );
        assert_eq!(
            drc_default_output_name("board.kicad_pcb", RcOutputFormat::Json),
            "board-drc.json"
        );
        assert_eq!(
            drill_default_report_path("out", "/x/board.kicad_pcb"),
            "out/board-drill.rpt"
        );
        assert_eq!(
            drill_default_report_path("", "board.kicad_pcb"),
            "board-drill.rpt"
        );
    }

    #[test]
    fn tokenizer_and_trim() {
        assert_eq!(wx_tokenize_ret_empty("a,,b", ','), vec!["a", "", "b"]);
        assert_eq!(wx_tokenize_ret_empty("a,", ','), vec!["a"]);
        assert_eq!(wx_tokenize_ret_empty(",a", ','), vec!["", "a"]);
        assert_eq!(wx_trim(" \tF.Cu \r\n"), "F.Cu");
    }

    #[test]
    fn empty_layer_list_is_an_error() {
        let mut job = JobExportPcbSvg::new();
        job.plot.arg_layers = Some("Bogus".to_string());
        let mut rep = Reporter::new();
        let enabled = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]);
        assert!(prepare_plot_layers(&mut job.plot, &enabled, None, &mut rep).is_none());
        assert_eq!(
            rep.text(),
            "Invalid layer name 'Bogus'\nAt least one layer must be specified\n"
        );
    }
}
