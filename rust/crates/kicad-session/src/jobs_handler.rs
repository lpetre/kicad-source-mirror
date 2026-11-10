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

/// `SEXPR_BOARD_FILE_VERSION` of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h` (the current
/// board file format version). Used by `JobUpgrade`; replace by the `kicad_pcb::io` constant
/// when the writer lands.
pub const SEXPR_BOARD_FILE_VERSION: i32 = 20261031;

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
/// the title block into the job, apply the `-D` overrides to the project text variables and
/// resolve the output path.
///
/// The override drawing sheet (`loadOverrideDrawingSheet`) and `BOARD::SynchronizeProperties`
/// are not ported yet; the project text variables are the job's overrides only until the
/// project binding lands (`PORT_STATUS.md`).
fn resolve_job_output_path(job: &mut crate::jobs::Job, session: &Session) -> String {
    job.set_title_block(session.board.titles.clone());
    let project_vars = job.var_overrides.clone();
    job.full_output_path(Some(&project_vars))
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
pub fn run_drc(
    session: &mut Session,
    job: &mut JobPcbDrc,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    let _ = &reporter;

    // Running DRC requires libraries be loaded (footprint library parity only; not ported).

    if job.rc.job.configured_output_path().is_empty() {
        let name = drc_default_output_name(&session.source_name, job.rc.format);
        job.rc.job.set_working_output_path(&name);
    }

    let out_path = resolve_job_output_path(&mut job.rc.job, session);
    let _units = drc_units(job.rc.units);
    let _ = out_path;

    // drcEngine->InitEngine( brd->GetDesignRulesPath() ) when the rules are not valid, then the
    // schematic parity netlist, the zone refill, RunTests, the marker providers and the report.
    if job.refill_zones {
        session.refill_zones()?;
    }

    Err(SessionError::not_yet_ported(
        "kicad-drc: DRC_ENGINE::InitEngine / RunTests and DRC_REPORT",
    ))
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportGerbers`.
pub fn run_export_gerbers(
    session: &mut Session,
    job: &mut JobExportPcbGerbers,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    if !job.gerber.plot.variant.is_empty() {
        return Err(SessionError::not_yet_ported(
            "kicad-pcb: BOARD::SetCurrentVariant",
        ));
    }

    let _out_path = resolve_job_output_path(&mut job.gerber.plot.job, session);

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
    let _ = layers_to_plot.ui_order();

    // RefreshGeneratedTables, then per layer: PlotJobToPlotOpts, BuildPlotFileName,
    // StartPlotBoard, PlotBoardLayers, EndPlot; then the Gerber job file.
    Err(SessionError::not_yet_ported(
        "kicad-fab: StartPlotBoard / PlotBoardLayers / GERBER_JOBFILE_WRITER",
    ))
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportDrill`.
pub fn run_export_drill(
    session: &mut Session,
    job: &mut JobExportPcbDrill,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    let _ = &reporter;
    let out_path = resolve_job_output_path(&mut job.job, session);

    // Keep the generated default out of the job so it is not saved into the jobset
    let mut report_path_setting = job.report_path.clone();

    if job.generate_report && report_path_setting.is_empty() {
        report_path_setting = drill_default_report_path(&out_path, &session.source_name);
    }

    let _ = report_path_setting;

    Err(SessionError::not_yet_ported(match job.format {
        DrillFormat::Excellon => "kicad-fab: EXCELLON_WRITER::CreateDrillandMapFilesSet",
        DrillFormat::Gerber => "kicad-fab: GERBER_WRITER::CreateDrillandMapFilesSet",
    }))
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportSvg`.
pub fn run_export_svg(
    session: &mut Session,
    job: &mut JobExportPcbSvg,
    reporter: &mut Reporter,
) -> Result<JobOutcome, SessionError> {
    if !job.plot.variant.is_empty() {
        return Err(SessionError::not_yet_ported(
            "kicad-pcb: BOARD::SetCurrentVariant",
        ));
    }

    // resolvePlotOutputPath: single output without a configured path -> <board name>.svg
    let is_single = job.gen_mode == crate::jobs::SvgGenMode::Single;

    if is_single && job.plot.job.configured_output_path().is_empty() {
        let name = format!("{}.svg", file_stem(&session.source_name));
        job.plot.job.set_working_output_path(&name);
    }

    let _out_path = resolve_job_output_path(&mut job.plot.job, session);

    if job.plot.check_zones_before_plot {
        session.refill_zones()?;
    }

    let enabled = session.board.enabled_layers();
    let board = &session.board;
    let user_name = |layer: PcbLayerId| board.layer_name(layer);

    if prepare_plot_layers(&mut job.plot, &enabled, Some(&user_name), reporter).is_none() {
        return Ok(JobOutcome::exit(exit_codes::ERR_ARGS));
    }

    Err(SessionError::not_yet_ported(
        "kicad-fab: PCB_PLOTTER::PlotJobToPlotOpts / PCB_PLOTTER::Plot (SVG)",
    ))
}

/// Port of `PCBNEW_JOBS_HANDLER::JobUpgrade` for a loaded session: decides whether to save
/// and returns the re-saved board as the one output file (named like the input: the C++
/// overwrites the board in place).
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
    // that changed anything (RefreshGeneratedTables: generated tables are not refreshed yet,
    // board-model.md 17).

    if should_save {
        let text = session.save()?;
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
    let _ = (library, &job, &reporter);

    // FP_CACHE::Load (PCB_IO_KICAD_SEXPR CTL_FOR_LIBRARY), the version check against
    // SEXPR_FOOTPRINT_LIB_FILE_VERSION and FP_CACHE::Save.
    Err(SessionError::not_yet_ported(
        "kicad-pcb: footprint library parser / writer (FP_CACHE::Load / Save)",
    ))
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
