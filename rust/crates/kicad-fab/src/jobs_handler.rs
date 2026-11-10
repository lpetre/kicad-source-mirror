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

//! Port of the drill / Gerber / SVG parts of `pcbnew/pcbnew_jobs_handler.cpp` (`convertLayerArg`,
//! `JobExportGerbers`, `JobExportDrill`, `JobExportSvg`), pure: board in, `OutputFile`s out.
//!
//! Work packages FAB-02 (`JobExportDrill`: [`job_export_drill`]), FAB-07 (`JobExportGerbers`:
//! [`job_export_gerbers`], and `PCB_PLOTTER::PlotJobToPlotOpts` of `pcbnew/pcb_plotter.cpp`:
//! [`plot_job_to_plot_opts`]), FAB-11 (`JobExportSvg` with `plotJob`: [`job_export_svg`]).
//!
//! `convertLayerArg` and the layer selection of `JobExportGerbers` (L1784-1830) are ported in
//! `kicad-session` (`jobs_handler::convert_layer_arg`, `gerbers_layers_to_plot`), which owns
//! the raw `--layers` arguments; [`job_export_gerbers`] continues from `layersToPlot`.
//!
//! The host (`kicad-session`) loads the board (`getBoard`), resolves the output directory
//! (`resolveJobOutputPath`), creates it (`PATHS::EnsurePathExists`), writes the returned files
//! and turns the outcome into the CLI exit code.

use kicad_core::plot_enums::{DrillMarks, PlotFormat};
use kicad_core::report_severity::Severity;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_pcb::BoardCtx;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::GerberPlotter;

use crate::color_settings::{ColorSettings, get_color_settings};
use crate::exporters::gendrill_excellon_writer::{ExcellonWriter, reborrow};
use crate::exporters::gendrill_gerber_writer::GerberWriter;
use crate::exporters::gendrill_report::{BoardHoleSource, HoleListSource};
use crate::exporters::gendrill_writer_base::{DrillPrecision, ZerosFmt};
use crate::exporters::gerber_jobfile_writer::GerberJobfileWriter;
use crate::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillFormat, DrillOrigin, DrillUnits, JobExportPcbDrill, MapFormat, ZerosFormat,
};
use crate::jobs::job_export_pcb_gerbers::JobExportPcbGerbers;
use crate::jobs::job_export_pcb_plot::{JobExportPcbPlot, JobPlotFormat};
use crate::jobs::job_export_pcb_svg::{JobExportPcbSvg, SvgGenMode};
use crate::pcb_plotter::build_plot_file_name;
use crate::pcb_plotter::{PlotOutcome, plot};
use crate::pcbplot::{PlotEnv, gerber_protel_extension};
use crate::plot_board_layers::{PadPlotState, plot_board_layers, start_plot_board};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile, Reporter};

/// `FILEEXT::ReportFileExtension`.
pub const REPORT_FILE_EXTENSION: &str = "rpt";

/// `precisionListForInches` of `pcbnew_jobs_handler.cpp`.
pub const PRECISION_LIST_FOR_INCHES: DrillPrecision = DrillPrecision::new(2, 4);

/// `precisionListForMetric` of `pcbnew_jobs_handler.cpp`.
pub const PRECISION_LIST_FOR_METRIC: DrillPrecision = DrillPrecision::new(3, 3);

/// The result of [`job_export_drill`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillJobOutcome {
    /// `false` where the C++ returns `CLI::EXIT_CODES::ERR_INVALID_OUTPUT_CONFLICT` (a writer
    /// reported failure); `error` then says why.
    pub success: bool,
    /// Why the export failed (the port's errors: a part that is not ported yet, such as the
    /// drill map).
    pub error: Option<FabError>,
    /// Every file written, in the order the C++ creates them (drill files, maps, report).
    pub files: Vec<OutputFile>,
    /// `aDrillJob->AddOutput( ... )` in order: `GetCreatedFiles()`, then the report.
    pub outputs: Vec<String>,
}

/// The default drill report path of `JobExportDrill` when `m_reportPath` is empty:
/// `wxFileName::DirName( outPath )`, `SetFullName( brd->GetFileName() )` (only the name part
/// is kept), name + `-drill`, extension `rpt`.
pub fn default_drill_report_path(out_path: &str, board_file_name: &str) -> String {
    let mut fn_ = WxFileName::new(board_file_name);
    fn_.set_path(out_path);
    let name = format!("{}-drill", fn_.name());
    fn_.set_name(&name);
    fn_.set_ext(REPORT_FILE_EXTENSION);

    fn_.full_path()
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportDrill( aJob )` after the board is loaded and the
/// output directory resolved: create the drill files (and maps and the report, as
/// requested) of the board of `ctx` in `out_path`.
///
/// * `env`: the build version and the date the C++ takes from the environment.
/// * `resolve_output_path`: `aDrillJob->ResolveOutputPath( path, false, project )` (text
///   variable expansion of the report path); `None` keeps the path as is.
/// * `reporter`: the job handler's `m_reporter` (`Created file '...'`, `Done.`).
///
/// Drill maps (FAB-12) are plotted in Gerber X2 and SVG; a PDF, PostScript or DXF map ends
/// the export with `FabError::Unsupported`. The Gerber format runs `GERBER_WRITER` (FAB-08).
pub fn job_export_drill(
    ctx: BoardCtx<'_>,
    job: &JobExportPcbDrill,
    out_path: &str,
    env: &DrillExportEnv,
    resolve_output_path: Option<&dyn Fn(&str) -> String>,
    reporter: Option<&mut dyn Reporter>,
) -> DrillJobOutcome {
    job_export_drill_from(
        &BoardHoleSource,
        ctx,
        job,
        out_path,
        env,
        resolve_output_path,
        reporter,
    )
}

/// [`job_export_drill`] with the drill spans and hole lists taken from `source` (see
/// [`HoleListSource`]).
pub fn job_export_drill_from(
    source: &dyn HoleListSource,
    ctx: BoardCtx<'_>,
    job: &JobExportPcbDrill,
    out_path: &str,
    env: &DrillExportEnv,
    resolve_output_path: Option<&dyn Fn(&str) -> String>,
    mut reporter: Option<&mut dyn Reporter>,
) -> DrillJobOutcome {
    let brd = ctx.board;
    let mut outcome = DrillJobOutcome::default();

    let offset = if job.drill_origin == DrillOrigin::Abs {
        Vector2I::new(0, 0)
    } else {
        brd.design_settings().aux_origin()
    };

    let map_format = match job.map_format {
        MapFormat::Postscript => PlotFormat::Post,
        MapFormat::GerberX2 => PlotFormat::Gerber,
        MapFormat::Dxf => PlotFormat::Dxf,
        MapFormat::Svg => PlotFormat::Svg,
        MapFormat::Pdf => PlotFormat::Pdf,
    };

    let mut report_path_setting = job.report_path.clone();

    // Keep the generated default out of the job so it is not saved into the jobset
    if job.generate_report && report_path_setting.is_empty() {
        report_path_setting = default_drill_report_path(out_path, &brd.file_name);
    }

    match job.format {
        DrillFormat::Excellon => {
            let zero_fmt = match job.zero_format {
                ZerosFormat::KeepZeros => ZerosFmt::KeepZeros,
                ZerosFormat::SuppressLeading => ZerosFmt::SuppressLeading,
                ZerosFormat::SuppressTrailing => ZerosFmt::SuppressTrailing,
                ZerosFormat::Decimal => ZerosFmt::DecimalFormat,
            };

            let precision = if job.drill_units == DrillUnits::Inch {
                PRECISION_LIST_FOR_INCHES
            } else {
                PRECISION_LIST_FOR_METRIC
            };

            let mut excellon_writer = ExcellonWriter::new(ctx);
            excellon_writer.set_build_version(&env.build_version);
            excellon_writer.set_creation_date(env.creation_date);

            excellon_writer.set_format(
                job.drill_units == DrillUnits::Mm,
                zero_fmt,
                precision.lhs,
                precision.rhs,
            );
            excellon_writer.set_options(
                job.excellon_mirror_y,
                job.excellon_minimal_header,
                offset,
                job.excellon_combine_pth_npth,
            );
            excellon_writer.set_route_mode_for_oval_holes(job.excellon_oval_drill_route);
            excellon_writer.base.set_map_file_format(map_format);

            let result = excellon_writer.create_drill_and_map_files_set_from(
                source,
                out_path,
                true,
                job.generate_map,
                reborrow(&mut reporter),
            );

            // The files written before a failure stay on disk in C++.
            outcome.files = std::mem::take(&mut excellon_writer.base.outputs);

            if let Err(err) = result {
                outcome.error = Some(err);
                return outcome;
            }

            outcome
                .outputs
                .extend(excellon_writer.base.created_files().iter().cloned());

            if job.generate_report {
                let report_path = match resolve_output_path {
                    Some(resolve) => resolve(&report_path_setting),
                    None => report_path_setting,
                };

                if !excellon_writer.gen_drill_report_file_from(source, &report_path, None) {
                    outcome.files.append(&mut excellon_writer.base.outputs);
                    return outcome;
                }

                outcome.files.append(&mut excellon_writer.base.outputs);
                outcome.outputs.push(report_path);
            }
        }

        DrillFormat::Gerber => {
            // Set gerber precision: only 5 or 6 digits for mantissa are allowed
            // (SetFormat() accept 5 or 6, and any other value set the precision to 5)
            // the integer part precision is always 4, and units always mm
            let mut gerber_writer = GerberWriter::new(ctx);
            gerber_writer.set_build_version(&env.build_version);
            gerber_writer.set_creation_date(env.creation_date);

            gerber_writer.set_format(job.gerber_precision);
            gerber_writer.set_options(offset);
            gerber_writer.base.set_map_file_format(map_format);

            let result = gerber_writer.create_drill_and_map_files_set_from(
                source,
                out_path,
                true,
                job.generate_map,
                job.generate_tenting,
                reborrow(&mut reporter),
            );

            // The files written before a failure stay on disk in C++.
            outcome.files = std::mem::take(&mut gerber_writer.base.outputs);

            if let Err(err) = result {
                outcome.error = Some(err);
                return outcome;
            }

            outcome
                .outputs
                .extend(gerber_writer.base.created_files().iter().cloned());

            if job.generate_report {
                let report_path = match resolve_output_path {
                    Some(resolve) => resolve(&report_path_setting),
                    None => report_path_setting,
                };

                if !gerber_writer.gen_drill_report_file_from(source, &report_path, None) {
                    outcome.files.append(&mut gerber_writer.base.outputs);
                    return outcome;
                }

                outcome.files.append(&mut gerber_writer.base.outputs);
                outcome.outputs.push(report_path);
            }
        }
    }

    outcome.success = true;
    outcome
}

pub use crate::color_settings::{COLOR_BUILTIN_CLASSIC, COLOR_BUILTIN_DEFAULT, DEFAULT_THEME};

/// `FILEEXT::GerberJobFileExtension`.
pub const GERBER_JOB_FILE_EXTENSION: &str = "gbrjob";

/// The members of the derived job classes `PCB_PLOTTER::PlotJobToPlotOpts` reads through
/// `static_cast` (C++ dispatches on `m_plotFormat`; the port passes the derived part).
#[derive(Clone, Copy, Debug)]
pub enum PlotJobFormatOptions<'a> {
    /// `JOB_EXPORT_PCB_GERBERS` (`m_plotFormat == GERBER`).
    Gerber(&'a JobExportPcbGerbers),
    /// `JOB_EXPORT_PCB_SVG::m_precision`, `m_fitPageToBoard` (`m_plotFormat == SVG`).
    Svg {
        /// `m_precision`
        precision: u32,
        /// `m_fitPageToBoard`
        fit_page_to_board: bool,
    },
    /// Any other format: the DXF / PDF / PostScript / PNG job members are not part of the port
    /// (those formats are out of scope, `StartPlotBoard` rejects them).
    Other,
}

/// Port of `scaleToSelection` (`pcbnew/pcb_plotter.cpp`).
pub fn scale_to_selection(scale: f64) -> i32 {
    let mut selection = 1;

    if scale == 0.0 {
        selection = 0;
    } else if scale == 1.5 {
        selection = 2;
    } else if scale == 2.0 {
        selection = 3;
    } else if scale == 3.0 {
        selection = 4;
    }

    selection
}

/// Port of `PCB_PLOTTER::PlotJobToPlotOpts( aOpts, aJob, aReporter )`
/// (`pcbnew/pcb_plotter.cpp` L426-555): copy the job options into `a_opts`.
///
/// `a_format_options` is the derived job part read for the Gerber and SVG formats. The DXF,
/// PDF, PostScript and PNG branches are not ported (formats out of scope). An empty theme
/// takes the PCB editor theme, which is [`COLOR_BUILTIN_DEFAULT`] in a fresh configuration
/// (`APP_SETTINGS_BASE::m_ColorTheme` default).
///
/// `aOpts.SetColorSettings( colors )`: the port's `PcbPlotParams` has no colour settings, so
/// the theme (`::GetColorSettings( theme )`, [`get_color_settings`]) is returned; the caller
/// passes it to the plot functions.
pub fn plot_job_to_plot_opts(
    a_opts: &mut PcbPlotParams,
    a_job: &JobExportPcbPlot,
    a_format_options: PlotJobFormatOptions<'_>,
    a_reporter: Option<&mut dyn Reporter>,
) -> ColorSettings {
    if a_job.plot_format == JobPlotFormat::Gerber {
        if let PlotJobFormatOptions::Gerber(g_job) = a_format_options {
            a_opts.gerber_disable_apert_macros = g_job.gerber.disable_aperture_macros;
            a_opts.use_gerber_protel_extensions = g_job.gerber.use_protel_file_extension;
            a_opts.use_gerber_x2_format = g_job.gerber.use_x2_format;
            a_opts.include_gerber_netlist_info = g_job.gerber.include_netlist_attributes;
            a_opts.create_gerber_job_file = g_job.create_jobs_file;
            a_opts.set_gerber_precision(g_job.gerber.precision);
        }

        // Always disable plot pad holes
        a_opts.drill_marks = DrillMarks::NoDrillShape;
    } else {
        // Scale, doesn't apply to GERBER
        a_opts.scale = a_job.scale;
        a_opts.auto_scale = a_job.scale == 0.0;
        a_opts.scale_selection = scale_to_selection(a_job.scale);

        // Drill marks doesn't apply to GERBER
        a_opts.drill_marks = match a_job.drill_shape_option {
            DrillMarks::NoDrillShape => DrillMarks::NoDrillShape,
            DrillMarks::SmallDrillShape => DrillMarks::SmallDrillShape,
            DrillMarks::FullDrillShape => DrillMarks::FullDrillShape,
        };
    }

    if a_job.plot_format == JobPlotFormat::Svg {
        if let PlotJobFormatOptions::Svg {
            precision,
            fit_page_to_board,
        } = a_format_options
        {
            a_opts.set_svg_precision(precision);
            a_opts.svg_fit_page_to_board = fit_page_to_board;
        }
    }

    let mut theme = a_job.color_theme.as_str();

    // Theme may be empty when running from a job in GUI context, so use the GUI settings.
    if theme.is_empty() {
        theme = COLOR_BUILTIN_DEFAULT;
    }

    let colors = get_color_settings(theme);

    if colors.filename() != theme && !a_opts.black_and_white {
        if let Some(reporter) = a_reporter {
            reporter.report(
                &format!("Color theme '{theme}' not found, will use theme from PCB Editor.\n"),
                Severity::Warning,
            );
        }
    }

    a_opts.use_aux_origin = a_job.use_drill_origin;
    a_opts.plot_drawing_sheet = a_job.plot_drawing_sheet;
    a_opts.subtract_mask_from_silk = a_job.subtract_solder_mask_from_silk;
    a_opts.plot_reference = a_job.plot_ref_des;
    a_opts.plot_value = a_job.plot_footprint_values;
    a_opts.sketch_pads_on_fab_layers = a_job.sketch_pads_on_fab_layers;
    a_opts.hide_dnp_fps_on_fab_layers = a_job.hide_dnp_fps_on_fab_layers;
    a_opts.sketch_dnp_fps_on_fab_layers = a_job.sketch_dnp_fps_on_fab_layers;
    a_opts.crossout_dnp_fps_on_fab_layers = a_job.crossout_dnp_fps_on_fab_layers;
    a_opts.plot_pad_numbers = a_job.plot_pad_numbers;

    a_opts.black_and_white = a_job.black_and_white;
    a_opts.mirror = a_job.mirror;
    a_opts.negative = a_job.negative;

    a_opts.set_layer_selection(&Lset::from(&a_job.plot_layer_sequence));
    a_opts.set_plot_on_all_layers_sequence(a_job.plot_on_all_layers_sequence.0.clone());

    match a_job.plot_format {
        JobPlotFormat::Gerber => a_opts.format = PlotFormat::Gerber,
        JobPlotFormat::Post => a_opts.format = PlotFormat::Post,
        JobPlotFormat::Svg => a_opts.format = PlotFormat::Svg,
        JobPlotFormat::Dxf => a_opts.format = PlotFormat::Dxf,
        JobPlotFormat::Hpgl => { /* no longer supported */ }
        JobPlotFormat::Pdf => a_opts.format = PlotFormat::Pdf,
        JobPlotFormat::Png => a_opts.format = PlotFormat::Png,
    }

    a_opts.output_directory = a_job.configured_output_path.clone();

    colors
}

/// The result of [`job_export_gerbers`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GerbersJobOutcome {
    /// `false` where the C++ returns `CLI::EXIT_CODES::ERR_INVALID_OUTPUT_CONFLICT` ("Failed to
    /// plot to ..."; the C++ continues with the next layer). The port has no file system, so
    /// this only stays `true` today.
    pub success: bool,
    /// A part of the port that is not available yet stopped the export (`StartPlotBoard` of
    /// a format or drawing sheet outside the port). `files` holds what was produced before.
    pub error: Option<FabError>,
    /// Every Gerber file, in the order the C++ writes them (`layersToPlot.UIOrder()`), then the
    /// Gerber job file when `m_createJobsFile` is set.
    pub files: Vec<OutputFile>,
    /// `aJob->AddOutput( ... )` in order.
    pub outputs: Vec<String>,
    /// `jobfile_writer.AddGbrFile( layer, fullname )` in order: the input of the Gerber job
    /// file writer (FAB-09).
    pub gbr_files: Vec<(PcbLayerId, String)>,
    /// The path of the Gerber job file (`<board>-job.gbrjob`) when `m_createJobsFile` is set.
    pub job_file_path: Option<String>,
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportGerbers( aJob )` (L1750-1923) from `layersToPlot`
/// on (L1832): plot one Gerber file per layer of `a_layers_to_plot` in `UIOrder()` into
/// `a_out_path`, then name the Gerber job file.
///
/// The caller (the session) has loaded the board (`getBoard`), set the variant, resolved and
/// created `a_out_path` (`resolveJobOutputPath`), refilled the zones (`m_checkZonesBeforePlot`),
/// converted the layer arguments into `a_job`'s `m_plotLayerSequence` /
/// `m_plotOnAllLayersSequence` (incl. the `--board-plot-params` replacement), computed
/// `a_layers_to_plot = LSET( ResolvePlotLayers( enabled ) ) & enabled` and run
/// `RefreshGeneratedTables`.
///
/// * `a_env`: `GetBuildVersion()` and the clock.
/// * `a_reporter`: the job handler's `m_reporter` (`Plotted to '...'.`).
///
/// With `m_createJobsFile` the Gerber job file (`GERBER_JOBFILE_WRITER`, without reporter as
/// in C++) is the last of `files` / `outputs`.
pub fn job_export_gerbers(
    a_ctx: BoardCtx<'_>,
    a_gerber_job: &JobExportPcbGerbers,
    a_layers_to_plot: &Lset,
    a_out_path: &str,
    a_env: &PlotEnv,
    mut a_reporter: Option<&mut dyn Reporter>,
) -> GerbersJobOutcome {
    let brd = a_ctx.board;
    let mut outcome = GerbersJobOutcome {
        success: true,
        ..GerbersJobOutcome::default()
    };

    let mut jobfile_writer = GerberJobfileWriter::new(a_ctx, None);

    let board_plot_options = &brd.plot_options;
    let var_overrides = &a_gerber_job.gerber.plot.var_overrides;
    // The board's pads as the plot of the previous files left them (C++ plots all files from
    // the one loaded board, see `PadPlotState`)
    let pad_state = PadPlotState::new();

    for layer in a_layers_to_plot.ui_order().0 {
        let mut plot_sequence: Vec<PcbLayerId> = Vec::new();

        // Base layer always gets plotted first.
        plot_sequence.push(layer);

        // Now all the "include on all" layers
        for &layer_all in &a_gerber_job.gerber.plot.plot_on_all_layers_sequence.0 {
            // Don't plot the same layer more than once;
            if plot_sequence.contains(&layer_all) {
                continue;
            }

            plot_sequence.push(layer_all);
        }

        // Pick the basename from the board file
        let mut fn_ = WxFileName::new(&brd.file_name);
        let mut layer_name = brd.layer_name(layer);
        let mut sheet_name = String::new();
        let mut sheet_path = String::new();
        let mut plot_opts = PcbPlotParams::default();
        // `plotOpts.ColorSettings()`: the options' own (never loaded) colour settings unless
        // PlotJobToPlotOpts installs the job's theme
        let mut colors = ColorSettings::default();

        if a_gerber_job.use_board_plot_params {
            plot_opts = board_plot_options.clone();
        } else {
            colors = plot_job_to_plot_opts(
                &mut plot_opts,
                &a_gerber_job.gerber.plot,
                PlotJobFormatOptions::Gerber(a_gerber_job),
                reborrow(&mut a_reporter),
            );
        }

        let file_ext = if plot_opts.use_gerber_protel_extensions {
            gerber_protel_extension(layer.0)
        } else {
            // FILEEXT::GerberFileExtension
            GerberPlotter::default_file_extension().to_string()
        };

        build_plot_file_name(&mut fn_, a_out_path, &layer_name, &file_ext);
        let fullname = fn_.full_name();

        jobfile_writer.add_gbr_file(layer, &fullname);
        outcome.gbr_files.push((layer, fullname));

        if let Some(value) = var_overrides.get("LAYER") {
            layer_name = value.clone();
        }

        if let Some(value) = var_overrides.get("SHEETNAME") {
            sheet_name = value.clone();
        }

        if let Some(value) = var_overrides.get("SHEETPATH") {
            sheet_path = value.clone();
        }

        let full_path = fn_.full_path();

        // We are feeding it one layer at the start here to silence a logic check
        let plotter = start_plot_board(
            &a_ctx,
            &plot_opts,
            layer.0,
            &layer_name,
            &full_path,
            &sheet_name,
            &sheet_path,
            "1",
            "",
            1,
            &colors,
            a_env,
        );

        match plotter {
            Ok(mut plotter) => {
                if let Some(reporter) = reborrow(&mut a_reporter) {
                    reporter.report(&format!("Plotted to '{full_path}'.\n"), Severity::Action);
                }

                plot_board_layers(
                    &a_ctx,
                    plotter.as_mut(),
                    &plot_sequence,
                    &plot_opts,
                    colors,
                    &pad_state,
                );
                plotter.end_plot();

                outcome.files.push(OutputFile {
                    name: full_path.clone(),
                    contents: plotter.output().as_bytes().to_vec(),
                });
                outcome.outputs.push(full_path);
            }
            Err(err) => {
                // The C++ reports "Failed to plot to '%s'." when the file cannot be created;
                // the port's errors are parts outside the port: stop here.
                outcome.error = Some(err);
                return outcome;
            }
        }
    }

    if a_gerber_job.create_jobs_file {
        let mut fn_ = WxFileName::new(&brd.file_name);

        // Build gerber job file from basename
        build_plot_file_name(&mut fn_, a_out_path, "job", GERBER_JOB_FILE_EXTENSION);
        let full_path = fn_.full_path();
        let job_file = jobfile_writer.create_job_file(&full_path, a_env);
        outcome.files.push(job_file);
        outcome.outputs.push(full_path.clone());
        outcome.job_file_path = Some(full_path);
    }

    outcome
}

/// Port of `PCBNEW_JOBS_HANDLER::JobExportSvg( aJob )` (L1542-1579) from
/// `PCB_PLOT_PARAMS plotOpts` on, with the static `plotJob` (L1506-1539): the plot options
/// from the job (`PlotJobToPlotOpts`), then `PCB_PLOTTER::Plot` into `a_out_path` (a file in
/// single mode, a directory in multi mode); the `LAYER` / `SHEETNAME` / `SHEETPATH` variable
/// overrides are used in single mode only.
///
/// The caller (the session) has loaded the board (`getBoard`), set the variant, resolved the
/// output path (`resolvePlotOutputPath`), created the directory, refilled the zones,
/// converted the layer arguments (`preparePlotLayers`: `a_layers_to_plot` is
/// `ResolvePlotLayers( enabled )`, not empty) and run `RefreshGeneratedTables` (see
/// [`plot`]).
///
/// `success == false` is `CLI::EXIT_CODES::ERR_UNKNOWN`; `outputs` are the `AddOutput` calls.
pub fn job_export_svg(
    a_ctx: BoardCtx<'_>,
    a_svg_job: &JobExportPcbSvg,
    a_layers_to_plot: &[PcbLayerId],
    a_out_path: &str,
    a_env: &PlotEnv,
    mut a_reporter: Option<&mut dyn Reporter>,
) -> PlotOutcome {
    let is_single = a_svg_job.gen_mode == SvgGenMode::Single;

    let mut plot_opts = PcbPlotParams::default();
    let colors = plot_job_to_plot_opts(
        &mut plot_opts,
        &a_svg_job.plot,
        PlotJobFormatOptions::Svg {
            precision: a_svg_job.precision,
            fit_page_to_board: a_svg_job.fit_page_to_board,
        },
        reborrow(&mut a_reporter),
    );

    // plotJob( aSvgJob, plotter, layersToPlot, outPath, isSingle, isSingle )
    let use_overrides = is_single;
    let overrides = &a_svg_job.plot.var_overrides;
    let mut layer_name: Option<&str> = None;
    let mut sheet_name: Option<&str> = None;
    let mut sheet_path: Option<&str> = None;

    if use_overrides {
        layer_name = overrides.get("LAYER").map(String::as_str);
        sheet_name = overrides.get("SHEETNAME").map(String::as_str);
        sheet_path = overrides.get("SHEETPATH").map(String::as_str);
    }

    plot(
        &a_ctx,
        &mut plot_opts,
        colors,
        a_out_path,
        a_layers_to_plot,
        &a_svg_job.plot.plot_on_all_layers_sequence.0,
        false,
        is_single,
        layer_name,
        sheet_name,
        sheet_path,
        a_env,
        a_reporter,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_report_path() {
        assert_eq!(
            default_drill_report_path("out/", "/x/y/board.kicad_pcb"),
            "out/board-drill.rpt"
        );
        assert_eq!(
            default_drill_report_path("out", "board.kicad_pcb"),
            "out/board-drill.rpt"
        );
        assert_eq!(
            default_drill_report_path("", "/x/board.kicad_pcb"),
            "board-drill.rpt"
        );
    }

    #[test]
    fn scale_selection() {
        assert_eq!(scale_to_selection(0.0), 0);
        assert_eq!(scale_to_selection(1.0), 1);
        assert_eq!(scale_to_selection(1.5), 2);
        assert_eq!(scale_to_selection(2.0), 3);
        assert_eq!(scale_to_selection(3.0), 4);
        assert_eq!(scale_to_selection(2.5), 1);
    }

    /// `PlotJobToPlotOpts` for the `gerbers` job as `kicad-cli` fills it.
    #[test]
    fn plot_job_to_plot_opts_gerbers() {
        let mut job = JobExportPcbGerbers::new();
        job.gerber.precision = 6;
        job.gerber.use_x2_format = false;
        job.gerber.plot.use_drill_origin = true;
        job.gerber.plot.sketch_dnp_fps_on_fab_layers = false;
        job.gerber.plot.plot_layer_sequence =
            kicad_core::Lseq(vec![PcbLayerId::BCu, PcbLayerId::FCu, PcbLayerId::EdgeCuts]);
        job.gerber.plot.plot_on_all_layers_sequence = kicad_core::Lseq(vec![PcbLayerId::FFab]);
        job.gerber.plot.configured_output_path = "out/".to_string();

        let mut opts = PcbPlotParams::default();
        let mut reporter = crate::VecReporter::default();
        plot_job_to_plot_opts(
            &mut opts,
            &job.gerber.plot,
            PlotJobFormatOptions::Gerber(&job),
            Some(&mut reporter),
        );

        assert!(reporter.messages.is_empty());
        assert_eq!(opts.format, PlotFormat::Gerber);
        assert_eq!(opts.drill_marks, DrillMarks::NoDrillShape);
        assert_eq!(opts.gerber_precision(), 6);
        assert!(!opts.use_gerber_x2_format);
        assert!(opts.use_gerber_protel_extensions);
        assert!(opts.include_gerber_netlist_info);
        assert!(opts.create_gerber_job_file);
        assert!(!opts.gerber_disable_apert_macros);
        assert!(opts.use_aux_origin);
        assert!(!opts.plot_drawing_sheet);
        assert!(!opts.sketch_dnp_fps_on_fab_layers);
        assert!(opts.crossout_dnp_fps_on_fab_layers);
        assert!(!opts.black_and_white);
        // the scale is not touched for Gerber
        assert_eq!(opts.scale, PcbPlotParams::default().scale);
        assert_eq!(
            opts.layer_selection(),
            &Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::EdgeCuts])
        );
        assert_eq!(opts.plot_on_all_layers_sequence, vec![PcbLayerId::FFab]);
        assert_eq!(opts.output_directory, "out/");
    }

    /// The colour theme warning needs a theme the settings manager does not know and a
    /// colour plot (`GetBlackAndWhite()` of the options *before* the job's value is copied).
    #[test]
    fn plot_job_to_plot_opts_theme_warning() {
        let mut job = JobExportPcbPlot::new(JobPlotFormat::Svg);
        job.color_theme = "nonexistent".to_string();
        job.scale = 0.0;

        let mut opts = PcbPlotParams::default();
        let mut reporter = crate::VecReporter::default();
        plot_job_to_plot_opts(
            &mut opts,
            &job,
            PlotJobFormatOptions::Svg {
                precision: 4,
                fit_page_to_board: true,
            },
            Some(&mut reporter),
        );
        // PCB_PLOT_PARAMS starts black and white: no warning
        assert!(reporter.messages.is_empty());
        assert!(opts.auto_scale);
        assert_eq!(opts.scale_selection, 0);
        assert_eq!(opts.drill_marks, DrillMarks::FullDrillShape);
        assert_eq!(opts.svg_precision(), 4);
        assert!(opts.svg_fit_page_to_board);
        assert_eq!(opts.format, PlotFormat::Svg);

        opts.black_and_white = false;
        plot_job_to_plot_opts(
            &mut opts,
            &job,
            PlotJobFormatOptions::Other,
            Some(&mut reporter),
        );
        assert_eq!(
            reporter.messages,
            vec![(
                "Color theme 'nonexistent' not found, will use theme from PCB Editor.\n"
                    .to_string(),
                Severity::Warning
            )]
        );

        job.color_theme = COLOR_BUILTIN_CLASSIC.to_string();
        reporter.messages.clear();
        opts.black_and_white = false;
        plot_job_to_plot_opts(
            &mut opts,
            &job,
            PlotJobFormatOptions::Other,
            Some(&mut reporter),
        );
        assert!(reporter.messages.is_empty());
    }

    /// Reduced port of `qa/tests/pcbnew/test_plot_job_layers.cpp`
    /// `AllCopperLayersSurviveSmallerBoard`: a job that plots all copper layers
    /// (`m_plotAllCopperLayers`, which `FromJson` derives from a legacy job listing every copper
    /// layer; job JSON is not ported) saved on a 2-layer board plots all copper of a 4-layer
    /// board. The boards are replaced by their enabled layer sets.
    #[test]
    fn all_copper_layers_survive_smaller_board() {
        let two_layer = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::FFab]);
        let four_layer = Lset::from_layers(&[
            PcbLayerId::FCu,
            PcbLayerId::In1Cu,
            PcbLayerId::In2Cu,
            PcbLayerId::BCu,
            PcbLayerId::FFab,
        ]);

        let mut job = JobExportPcbGerbers::new();
        let layers = Lset::all_cu_mask().clone()
            | Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask, PcbLayerId::EdgeCuts]);
        // DialogLayerSelection( ..., twoLayer, true ): the checked layers the board shows
        job.gerber.plot.plot_layer_sequence = (layers & two_layer).seq_stackup_for_plotting();
        job.gerber.plot.plot_all_copper_layers = true;

        let plotted = Lset::from(&job.gerber.plot.resolve_plot_layers(&four_layer));
        let four_layer_copper = four_layer.clone() & Lset::all_cu_mask().clone();

        assert_eq!(plotted & Lset::all_cu_mask().clone(), four_layer_copper);
    }
}
