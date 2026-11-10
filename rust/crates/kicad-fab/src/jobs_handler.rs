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
//! Work packages FAB-02 (`JobExportDrill`: [`job_export_drill`]), FAB-07, FAB-11
//! (`docs/analysis/plot-fab.md` 10.2; the latter two not started).
//!
//! The host (`kicad-session`) loads the board (`getBoard`), resolves the output directory
//! (`resolveJobOutputPath`), creates it (`PATHS::EnsurePathExists`), writes the returned files
//! and turns the outcome into the CLI exit code.

use kicad_core::plot_enums::PlotFormat;
use kicad_geom::Vector2I;
use kicad_pcb::BoardCtx;

use crate::exporters::gendrill_excellon_writer::{ExcellonWriter, reborrow};
use crate::exporters::gendrill_report::{BoardHoleSource, HoleListSource};
use crate::exporters::gendrill_writer_base::{DrillPrecision, ZerosFmt};
use crate::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillFormat, DrillOrigin, DrillUnits, JobExportPcbDrill, MapFormat, ZerosFormat,
};
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
    /// drill map or the Gerber drill writer).
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
/// The Gerber format (`GERBER_WRITER`, FAB-08) and the drill maps (FAB-12) are not ported
/// yet: they end the export with `FabError::NotPorted`.
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
            outcome.error = Some(FabError::NotPorted(
                "GERBER_WRITER::CreateDrillandMapFilesSet (FAB-08)",
            ));
            return outcome;
        }
    }

    outcome.success = true;
    outcome
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
}
