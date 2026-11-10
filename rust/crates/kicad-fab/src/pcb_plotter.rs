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

//! Port of `pcbnew/pcb_plotter.h` / `pcb_plotter.cpp`: `PCB_PLOTTER::Plot` (L68-354, the
//! multi-file driver used by the SVG job: [`plot`]) and the helpers for layer sequences and
//! output file names (`BuildPlotFileName`, `getPlotSequence`, `copperLayerShouldBeSkipped`;
//! L355-423).
//!
//! Work packages FAB-03 (the helpers), FAB-07 (`PlotJobToPlotOpts`, in `jobs_handler.rs`),
//! FAB-11 (`PCB_PLOTTER::Plot`) (`docs/analysis/plot-fab.md` 10.2).
//!
//! Out of scope: `PCB_PLOTTER::DialogLayerSelection` (GUI dialog support, analysis 2.2).

use kicad_core::plot_enums::PlotFormat;
use kicad_core::report_severity::Severity;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{Shape, Vector2I};
use kicad_pcb::BoardCtx;
use kicad_pcb::board::Board;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::{GerberPlotter, SvgPlotter};

use crate::color_settings::ColorSettings;
use crate::exporters::gendrill_excellon_writer::reborrow;
use crate::exporters::gerber_jobfile_writer::GerberJobfileWriter;
use crate::pcbplot::{PlotEnv, gerber_protel_extension};
use crate::plot_board_layers::{PadPlotState, plot_board_layers, start_plot_board};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile, Reporter};

/// `FILEEXT::GerberJobFileExtension`.
const GERBER_JOB_FILE_EXTENSION: &str = "gbrjob";

/// The result of [`plot`] (`PCB_PLOTTER::Plot`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlotOutcome {
    /// The C++ return value: `false` for no layer, a failed plot, or a failure of `EndPlot`.
    pub success: bool,
    /// A part outside the port stopped the plot (`StartPlotBoard` of a format or a drawing
    /// sheet the port does not have). `files` holds what was produced before.
    pub error: Option<FabError>,
    /// Every file, in the order the C++ writes them.
    pub files: Vec<OutputFile>,
    /// `aOutputFiles` (`push_back( fn.GetFullPath() )` in order).
    pub outputs: Vec<String>,
}

/// `GetDefaultPlotExtension( aFormat )` for the formats of the port (`svg`, `gbr`); the other
/// formats cannot be plotted by the port (`StartPlotBoard` rejects them), their extension is
/// kept for the file name.
fn default_plot_extension(a_format: PlotFormat) -> &'static str {
    match a_format {
        PlotFormat::Gerber => GerberPlotter::default_file_extension(),
        PlotFormat::Svg => SvgPlotter::default_file_extension(),
        PlotFormat::Dxf => "dxf",
        PlotFormat::Post => "ps",
        PlotFormat::Pdf => "pdf",
        PlotFormat::Png => "png",
        PlotFormat::Hpgl => "plt",
        #[allow(unreachable_patterns)]
        _ => "",
    }
}

/// Port of `PCB_PLOTTER::Plot( aOutputPath, aLayersToPlot, aCommonLayers,
/// aUseGerberFileExtensions, aOutputPathIsSingle, aLayerName, aSheetName, aSheetPath,
/// aOutputFiles )`: plot `a_layers_to_plot` into one file per layer (or, with
/// `a_output_path_is_single`, the first layer with the others as common layers into the file
/// `a_output_path`).
///
/// `a_ctx` is `m_board`, `a_plot_opts` / `a_colors` are `m_plotOpts` and its
/// `ColorSettings()` (the C++ member is a reference: `SetUseAuxOrigin( true )` of an SVG
/// fit-to-board plot stays in the caller's options), `a_reporter` is `m_reporter`, `a_env`
/// replaces `GetBuildVersion()` and the clock.
///
/// The caller runs `RefreshGeneratedTables( *m_board )` beforehand (it needs the mutable
/// board; the C++ runs it right after the "No layers selected" check, which cannot fail on
/// the job path). The SVG fit-to-board page change and the aux origin reset (restored at the
/// end in C++) are made on a copy of the board. `PlotInteractiveLayer` (PDF only) and the
/// PDF / DXF multi-page branches are not ported: those formats are out of scope and
/// `StartPlotBoard` rejects them. `SetTitle` receives the title block title without
/// `ExpandTextVars` (only PDF uses it).
#[allow(clippy::too_many_arguments)]
pub fn plot(
    a_ctx: &BoardCtx<'_>,
    a_plot_opts: &mut PcbPlotParams,
    a_colors: ColorSettings,
    a_output_path: &str,
    a_layers_to_plot: &[PcbLayerId],
    a_common_layers: &[PcbLayerId],
    a_use_gerber_file_extensions: bool,
    a_output_path_is_single: bool,
    a_layer_name: Option<&str>,
    a_sheet_name: Option<&str>,
    a_sheet_path: Option<&str>,
    a_env: &PlotEnv,
    mut a_reporter: Option<&mut dyn Reporter>,
) -> PlotOutcome {
    let mut outcome = PlotOutcome::default();

    // sanity, ensure one layer to print
    if a_layers_to_plot.is_empty() {
        if let Some(reporter) = reborrow(&mut a_reporter) {
            reporter.report("No layers selected for plotting.", Severity::Error);
        }

        return outcome;
    }

    // RefreshGeneratedTables( *m_board ): the caller's.

    // Page is board boundary size
    let fitted_board: Option<Board> =
        if a_plot_opts.format == PlotFormat::Svg && a_plot_opts.svg_fit_page_to_board {
            let board = a_ctx.board;
            let mut bbox = board.compute_bounding_box(a_ctx, false, false);

            // Board outline geometry is better if it exists so that origin is not influenced by
            // Edge.Cuts line width
            let (board_outlines, ok) =
                board.get_board_polygon_outlines(a_ctx, false, None, false, false);

            if ok && board_outlines.outline_count() > 0 {
                bbox = board_outlines.bbox(0);
            }

            let mut curr_page_info = board.paper.clone();

            curr_page_info.set_width_mils(bbox.width() as f64 / PCB_IU_SCALE.iu_per_mils);
            curr_page_info.set_height_mils(bbox.height() as f64 / PCB_IU_SCALE.iu_per_mils);

            let mut copy = board.clone();
            copy.paper = curr_page_info;
            a_plot_opts.use_aux_origin = true;

            // Keep the origin at the board origin so it lands on the SVG origin. The SVG
            // plotter was given the bounding box to build its viewBox from elsewhere.
            copy.design_settings_mut()
                .set_aux_origin(Vector2I::new(0, 0));
            Some(copy)
        } else {
            None
        };

    let board_ctx: BoardCtx<'_> = match fitted_board.as_ref() {
        Some(board) => BoardCtx {
            board,
            rules: a_ctx.rules,
            connectivity: a_ctx.connectivity,
        },
        None => *a_ctx,
    };

    let ctx = &board_ctx;
    let board = ctx.board;

    // To reuse logic, in single plot mode, we want to kick any extra layers from the main
    // list to commonLayers
    let mut layers_to_plot: Vec<PcbLayerId> = Vec::new();
    let mut common_layers: Vec<PcbLayerId> = Vec::new();

    let is_pdf_multi_page = a_plot_opts.format == PlotFormat::Pdf && a_plot_opts.pdf_single;

    if a_output_path_is_single
        && !a_plot_opts.dxf_export_as_multi_layered_file
        && !is_pdf_multi_page
    {
        layers_to_plot.push(a_layers_to_plot[0]);

        if a_layers_to_plot.len() > 1 {
            common_layers.extend_from_slice(&a_layers_to_plot[1..]);
        }
    } else {
        layers_to_plot = a_layers_to_plot.to_vec();
        common_layers = a_common_layers.to_vec();
    }

    let mut final_page_count = 0;
    let mut layers_to_export: Vec<(PcbLayerId, String)> = Vec::new();

    // Skip the disabled copper layers and build the layer ID -> layer name mapping for
    // plotter. DXF plotter will use this information to name its layers
    for &layer in &layers_to_plot {
        if copper_layer_should_be_skipped(board, layer) {
            continue;
        }

        final_page_count += 1;
        layers_to_export.push((layer, board.layer_name(layer)));
    }

    let mut jobfile_writer: Option<GerberJobfileWriter<'_, '_>> =
        if a_plot_opts.format == PlotFormat::Gerber && !a_output_path_is_single {
            Some(GerberJobfileWriter::new(*ctx, None))
        } else {
            None
        };

    let plot_format = a_plot_opts.format;
    let mut file_ext = default_plot_extension(a_plot_opts.format).to_string();
    let mut sheet_path = String::new();
    let mut success = true;
    let mut page_num = 1;
    // The board's pads as the plot of the previous files left them (C++ plots all files from
    // the one loaded board, see `PadPlotState`)
    let pad_state = PadPlotState::new();

    for i in 0..layers_to_plot.len() {
        let layer = layers_to_plot[i];

        if copper_layer_should_be_skipped(board, layer) {
            continue;
        }

        let plot_sequence = get_plot_sequence(layer, &common_layers);
        let mut layer_name = board.layer_name(layer);
        let mut fn_: WxFileName;

        if a_output_path_is_single {
            fn_ = WxFileName::new(a_output_path);
        } else {
            let brd_fn = WxFileName::new(&board.file_name);
            fn_ = WxFileName::default();
            fn_.set_path(a_output_path);
            fn_.set_name(brd_fn.name());
            fn_.set_ext(&file_ext);

            // Use Gerber Extensions based on layer number
            // (See http://en.wikipedia.org/wiki/Gerber_File)
            if a_plot_opts.format == PlotFormat::Gerber && a_use_gerber_file_extensions {
                file_ext = gerber_protel_extension(layer.0);
            }

            if plot_format == PlotFormat::Pdf && a_plot_opts.pdf_single {
                fn_.set_ext(default_plot_extension(PlotFormat::Pdf));
            } else if plot_format == PlotFormat::Dxf && a_plot_opts.dxf_export_as_multi_layered_file
            {
                fn_.set_ext(default_plot_extension(PlotFormat::Dxf));
            } else {
                build_plot_file_name(&mut fn_, a_output_path, &layer_name, &file_ext);
            }
        }

        if let Some(writer) = jobfile_writer.as_mut() {
            let fullname = fn_.full_name();
            writer.add_gbr_file(layer, &fullname);
        }

        // PDF single document / DXF multi-layer pages (formats out of scope) would reuse the
        // plotter of page 1 here; every ported format starts a new plot per layer.
        let page_number = format!("{page_num}");
        let mut page_name = layer_name.clone();
        let mut sheet_name = layer_name.clone();

        if let Some(name) = a_layer_name {
            layer_name = name.to_string();
            page_name = name.to_string();
        }

        if let Some(name) = a_sheet_name {
            sheet_name = name.to_string();
        }

        if let Some(path) = a_sheet_path {
            sheet_path = path.to_string();
        }

        a_plot_opts.layers_to_export = layers_to_export.clone();
        let full_path = fn_.full_path();

        let plotter = start_plot_board(
            ctx,
            a_plot_opts,
            layer.0,
            &layer_name,
            &full_path,
            &sheet_name,
            &sheet_path,
            &page_name,
            &page_number,
            final_page_count,
            &a_colors,
            a_env,
        );

        match plotter {
            Ok(mut plotter) => {
                plotter.set_layer(layer);
                // ExpandTextVars( GetTitleBlock().GetTitle() ): only PDF reads the title
                plotter.set_title(board.titles.title());

                // m_PDFMetadata (AUTHOR / SUBJECT): PDF only.

                plot_board_layers(
                    ctx,
                    plotter.as_mut(),
                    &plot_sequence,
                    a_plot_opts,
                    a_colors,
                    &pad_state,
                );

                // PlotInteractiveLayer: PDF only. PDF page break: PDF only.

                // last page
                success &= plotter.end_plot();

                if let Some(reporter) = reborrow(&mut a_reporter) {
                    reporter.report(&format!("Plotted to '{full_path}'."), Severity::Action);
                }

                outcome.files.push(OutputFile {
                    name: full_path.clone(),
                    contents: plotter.output().as_bytes().to_vec(),
                });
                outcome.outputs.push(full_path);
            }
            Err(err) => {
                // The C++ reports "Failed to create file '%s'." when the file cannot be
                // created; the port's errors are parts outside the port: stop here.
                outcome.error = Some(err);
                return outcome;
            }
        }

        page_num += 1;
    }

    if let Some(writer) = jobfile_writer.as_mut()
        && a_plot_opts.create_gerber_job_file
    {
        // Pick the basename from the board file
        let mut fn_ = WxFileName::new(&board.file_name);

        // Build gerber job file from basename
        build_plot_file_name(&mut fn_, a_output_path, "job", GERBER_JOB_FILE_EXTENSION);
        let full_path = fn_.full_path();
        outcome
            .files
            .push(writer.create_job_file(&full_path, a_env));
        outcome.outputs.push(full_path);
    }

    if let Some(reporter) = reborrow(&mut a_reporter) {
        reporter.report("Done.", Severity::Info);
    }

    // SVG fit to board: the original page and aux origin are untouched (the copy goes).

    outcome.success = success;
    outcome
}

/// `wxFileName::GetForbiddenChars( wxPATH_DOS )` (`*?` plus `\/:"<>|`) followed by the two
/// characters `BuildPlotFileName` appends (`%.`).
const BUILD_PLOT_FILE_NAME_BAD_CHARS: &[char] =
    &['*', '?', '\\', '/', ':', '"', '<', '>', '|', '%', '.'];

/// `wxString::Trim()`: the characters `wxSafeIsspace` accepts (ASCII white space only).
fn is_wx_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r')
}

/// Port of `PCB_PLOTTER::BuildPlotFileName`: complete a plot file name.
///
/// `a_filename` contains the base filename only (without path and extension) when calling
/// this function. It is expected to be a valid filename (this is usually the board
/// filename). The directory becomes `a_output_dir`, the extension `a_extension`; the suffix,
/// trimmed and with the characters not allowed in file names replaced by `_`, is appended to
/// the name as `-<suffix>` when something survives.
pub fn build_plot_file_name(
    a_filename: &mut WxFileName,
    a_output_dir: &str,
    a_suffix: &str,
    a_extension: &str,
) {
    a_filename.set_path(a_output_dir);

    // Set the file extension
    a_filename.set_ext(a_extension);

    // remove leading and trailing spaces if any from the suffix, if
    // something survives add it to the name;
    // also the suffix can contain some not allowed chars in filename (/ \ . : and some others),
    // so change them to underscore
    // Remember it can be called from a python script, so the illegal chars
    // have to be filtered here.
    let suffix: String = a_suffix
        .trim_end_matches(is_wx_space)
        .trim_start_matches(is_wx_space)
        .chars()
        .map(|c| {
            if BUILD_PLOT_FILE_NAME_BAD_CHARS.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    if !suffix.is_empty() {
        let name = format!("{}-{}", a_filename.name(), suffix);
        a_filename.set_name(&name);
    }
}

/// Port of `PCB_PLOTTER::getPlotSequence`: the layer to plot first, then the layers of
/// `a_plot_with_all_layers_seq` that are not already in the sequence (in their order).
pub fn get_plot_sequence(
    a_layer_to_plot: PcbLayerId,
    a_plot_with_all_layers_seq: &[PcbLayerId],
) -> Vec<PcbLayerId> {
    let mut plot_sequence = Vec::new();

    // Base layer always gets plotted first.
    plot_sequence.push(a_layer_to_plot);

    for &layer in a_plot_with_all_layers_seq {
        // Don't plot the same layer more than once;
        if plot_sequence.contains(&layer) {
            continue;
        }

        plot_sequence.push(layer);
    }

    plot_sequence
}

/// Port of `PCB_PLOTTER::copperLayerShouldBeSkipped`: true for a copper layer the board does
/// not enable.
pub fn copper_layer_should_be_skipped(a_board: &Board, a_layer_to_plot: PcbLayerId) -> bool {
    let mask: Lset = Lset::all_cu_mask() & &!&a_board.enabled_layers();
    mask.contains(a_layer_to_plot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(board: &str, dir: &str, suffix: &str, ext: &str) -> String {
        let mut fn_ = WxFileName::new(board);
        build_plot_file_name(&mut fn_, dir, suffix, ext);
        fn_.full_path()
    }

    #[test]
    fn plot_file_names() {
        assert_eq!(
            name("/a/demo.kicad_pcb", "out", "F.Cu", "gtl"),
            "out/demo-F_Cu.gtl"
        );
        assert_eq!(
            name("demo.kicad_pcb", "out/", " Inner Top \t", "g1"),
            "out/demo-Inner Top.g1"
        );
        assert_eq!(
            name("demo.kicad_pcb", "o", "My:Layer.x*?\\/\"<>|%", "gbr"),
            "o/demo-My_Layer_x_________.gbr"
        );
        assert_eq!(name("demo.kicad_pcb", "o", "  ", "gbrjob"), "o/demo.gbrjob");
        // wxString::Trim only removes ASCII white space
        assert_eq!(
            name("demo.kicad_pcb", "o", "\u{a0}x", "gbr"),
            "o/demo-\u{a0}x.gbr"
        );
    }

    #[test]
    fn plot_sequence() {
        let seq = get_plot_sequence(
            PcbLayerId::FCu,
            &[PcbLayerId::EdgeCuts, PcbLayerId::FCu, PcbLayerId::EdgeCuts],
        );
        assert_eq!(seq, vec![PcbLayerId::FCu, PcbLayerId::EdgeCuts]);
    }
}
