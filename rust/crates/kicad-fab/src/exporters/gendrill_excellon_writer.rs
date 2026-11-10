// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2018 Jean_Pierre Charras <jp.charras at wanadoo.fr>
// Copyright (C) 2015 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of `pcbnew/exporters/gendrill_excellon_writer.h` / `.cpp` (`EXCELLON_WRITER`):
//! Excellon drill files (work package FAB-02).
//!
//! For the Excellon format, see <http://www.excellon.com/manuals/program.htm> and the CNC-7
//! manual.
//!
//! Oblong holes can be drilled by a "canned slot" command (G85) or a routing command; a
//! linear routing command (G01) is perhaps more usual for drill files.
//! `use_route_mode_for_oval == false` uses a canned slot hole (old way), `true` (preferred
//! mode, the writer's default; the CLI default is G85) a linear routed hole (new way).
//!
//! Differences to the C++ class, all forced by the port's rules (no file system, no clock):
//! the file being written (`m_file`) is a `String`; finished files are appended to
//! `base.outputs`; the build version (`GetBuildVersion()`) and the creation date
//! (`wxDateTime::Now()`) are set by the caller. Hole lists come through a
//! [`HoleListSource`] (the board by default).

use kicad_core::PCB_IU_SCALE;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::printf::format_f;
use kicad_core::report_severity::Severity;
use kicad_geom::trigo::rotate_point_xy_around;
use kicad_geom::{Vector2I, ki_round};
use kicad_pcb::BoardCtx;
use kicad_pcb::padstack::PadDrillPostMachiningMode;
use kicad_plot::gbr_metadata::{
    GbrCreationDate, GbrNcStringFormat, gbr_make_creation_date_attribute_string,
};

use super::gendrill_report::{BoardHoleSource, HoleListSource};
use super::gendrill_writer_base::{
    GendrillWriterBase, TypeFile, USE_ATTRIB_FOR_HOLES, ZerosFmt, layer_pair_name,
};
use crate::drill::{DrillOperation, DrillSpan, HoleAttribute};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile, Reporter};

/// Reborrows an optional reporter for one call (shortens the trait object lifetime, which
/// `Option::as_deref_mut` cannot do behind `&mut`).
pub(crate) fn reborrow<'s>(
    reporter: &'s mut Option<&mut dyn Reporter>,
) -> Option<&'s mut dyn Reporter> {
    match reporter {
        Some(r) => Some(&mut **r),
        None => None,
    }
}

/// `FILEEXT::DrillFileExtension`.
pub const DRILL_FILE_EXTENSION: &str = "drl";

/// `EXCELLON_WRITER`: create Excellon drill, drill map, and drill report files.
#[derive(Clone)]
pub struct ExcellonWriter<'a> {
    /// The `GENDRILL_WRITER_BASE` part.
    pub base: GendrillWriterBase<'a>,
    /// The output file being written (`m_file`).
    file: String,
    /// True to use minimal header (`m_minimalHeader`).
    pub minimal_header: bool,
    /// `m_mirror`: Y coordinates are not negated.
    pub mirror: bool,
    /// True to use a route command for oval holes, false to use a G85 canned mode for oval
    /// holes (`m_useRouteModeForOval`).
    pub use_route_mode_for_oval: bool,
    /// Max number of digits printed in float numbers (`m_mantissaLenght`, C++ spelling).
    pub mantissa_lenght: i32,
    /// `GetBuildVersion()` (a parameter in the port).
    pub build_version: String,
    /// `wxDateTime::Now()` with the local UTC offset (a parameter in the port).
    pub creation_date: GbrCreationDate,
}

impl<'a> ExcellonWriter<'a> {
    /// Port of `EXCELLON_WRITER::EXCELLON_WRITER( aPcb )`.
    pub fn new(pcb: BoardCtx<'a>) -> Self {
        let mut base = GendrillWriterBase::new(pcb);
        base.zero_format = ZerosFmt::DecimalFormat;
        base.conversion_units = 0.0001;
        base.merge_pth_npth = false;
        base.drill_file_extension = DRILL_FILE_EXTENSION.to_string();

        ExcellonWriter {
            base,
            file: String::new(),
            mirror: false,
            minimal_header: false,
            use_route_mode_for_oval: true,
            mantissa_lenght: 3, // suitable to print coordinates in mm
            build_version: String::new(),
            creation_date: GbrCreationDate::default(),
        }
    }

    /// Sets the text `GetBuildVersion()` returns in C++ (header comments).
    pub fn set_build_version(&mut self, version: &str) {
        self.build_version = version.to_string();
    }

    /// Sets the date and time C++ takes from the clock (header comments, report).
    pub fn set_creation_date(&mut self, date: GbrCreationDate) {
        self.creation_date = date;
    }

    /// `GetOffset`: the plot offset (usually the position of the auxiliary axis).
    pub fn offset(&self) -> Vector2I {
        self.base.offset
    }

    /// `SetRouteModeForOvalHoles`.
    pub fn set_route_mode_for_oval_holes(&mut self, use_route_mode_for_oval_holes: bool) {
        self.use_route_mode_for_oval = use_route_mode_for_oval_holes;
    }

    /// Port of `EXCELLON_WRITER::SetFormat( aMetric, aZerosFmt, aLeftDigits, aRightDigits )`:
    /// initialize internal parameters to match the given format. `left_digits` /
    /// `right_digits` <= 0 select a suitable value depending on units (3:3 mm, 2:4 inch).
    pub fn set_format(
        &mut self,
        metric: bool,
        zeros_fmt: ZerosFmt,
        mut left_digits: i32,
        mut right_digits: i32,
    ) {
        self.base.units_metric = metric;
        self.base.zero_format = zeros_fmt;

        // Set conversion scale depending on drill file units
        if self.base.units_metric {
            self.base.conversion_units = 1.0 / PCB_IU_SCALE.iu_per_mm; // EXCELLON units = mm
        } else {
            self.base.conversion_units = 0.001 / PCB_IU_SCALE.iu_per_mils; // EXCELLON units = in
        }

        // Set the zero counts. if aZerosFmt == DECIMAL_FORMAT, these values
        // will be set, but not used.
        if left_digits <= 0 {
            left_digits = if self.base.units_metric { 3 } else { 2 };
        }

        if right_digits <= 0 {
            right_digits = if self.base.units_metric { 3 } else { 4 };
        }

        self.base.precision.lhs = left_digits;
        self.base.precision.rhs = right_digits;
    }

    /// Port of `EXCELLON_WRITER::SetOptions( aMirror, aMinimalHeader, aOffset,
    /// aMerge_PTH_NPTH )`: `mirror` creates mirrored coordinates (Y coordinates not negated),
    /// `minimal_header` a header without comments, `merge_pth_npth` one file for PTH and NPTH
    /// instead of two separate files.
    pub fn set_options(
        &mut self,
        mirror: bool,
        minimal_header: bool,
        offset: Vector2I,
        merge_pth_npth: bool,
    ) {
        self.mirror = mirror;
        self.base.offset = offset;
        self.minimal_header = minimal_header;
        self.base.merge_pth_npth = merge_pth_npth;
    }

    /// Port of `EXCELLON_WRITER::CreateDrillandMapFilesSet( aPlotDirectory, aGenDrill, aGenMap,
    /// aReporter )`: create the full set of Excellon drill files for the board (file names
    /// are computed from the board name and layer ids) and, with `gen_map`, the drill maps.
    ///
    /// The files are appended to `base.outputs` (with `plot_directory` as directory part) and
    /// their names to `base.created_files` (in the C++ `AddCreatedFile` order). C++ returns
    /// `false` on failure; the port returns the error. Without a file system, only the drill
    /// map can fail (FAB-12 is not ported: `FabError::NotPorted`); as in C++ the drill files
    /// are kept and `Done.` is reported before returning the failure.
    pub fn create_drill_and_map_files_set(
        &mut self,
        plot_directory: &str,
        gen_drill: bool,
        gen_map: bool,
        reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), FabError> {
        self.create_drill_and_map_files_set_from(
            &BoardHoleSource,
            plot_directory,
            gen_drill,
            gen_map,
            reporter,
        )
    }

    /// [`Self::create_drill_and_map_files_set`] with the drill spans and hole lists taken
    /// from `source` (drill maps still enumerate the board).
    pub fn create_drill_and_map_files_set_from(
        &mut self,
        source: &dyn HoleListSource,
        plot_directory: &str,
        gen_drill: bool,
        gen_map: bool,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), FabError> {
        let mut success: Result<(), FabError> = Ok(());

        let mut hole_sets = source.drill_spans(&self.base);

        if !self.base.merge_pth_npth {
            hole_sets.push(DrillSpan::new(
                PcbLayerId::FCu,
                PcbLayerId::BCu,
                false,
                true,
            ));
        }

        for span in &hole_sets {
            let doing_npth = if self.base.merge_pth_npth {
                false
            } else {
                span.is_non_plated_file
            };

            source.build_holes_list(&mut self.base, span, doing_npth);

            // The file is created if it has holes, or if it is the non plated drill file to be
            // sure the NPTH file is up to date in separate files mode.
            // Also a PTH drill/map file is always created, to be sure at least one plated hole
            // drill file is created (do not create any PTH drill file can be seen as not working
            // drill generator).
            if self.base.holes_count() > 0
                || doing_npth
                || span.pair() == (PcbLayerId::FCu, PcbLayerId::BCu)
            {
                let mut fn_ = WxFileName::new(&self.base.drill_file_name(
                    span,
                    doing_npth,
                    self.base.merge_pth_npth,
                ));
                fn_.set_path(plot_directory);

                if gen_drill {
                    let full_filename = fn_.full_path();

                    // wxFopen cannot fail without a file system ("Failed to create file").
                    if let Some(rep) = reborrow(&mut reporter) {
                        let msg = format!("Created file '{full_filename}'");
                        rep.report(&msg, Severity::Action);
                    }

                    let mut file_type = TypeFile::PthFile;

                    if span.pair() == (PcbLayerId::FCu, PcbLayerId::BCu) && !span.is_backdrill {
                        if self.base.merge_pth_npth {
                            file_type = TypeFile::MixedFile;
                        } else if doing_npth {
                            file_type = TypeFile::NpthFile;
                        }
                    } else if span.is_backdrill {
                        file_type = TypeFile::NpthFile;
                    }

                    // fmt::print write errors ("Failed to write file") cannot happen either.
                    self.create_drill_file(span, file_type, false);
                    self.flush_file(&full_filename);

                    if span.is_backdrill && self.base.holes_count() > 0 {
                        self.write_backdrill_layer_pair_file(
                            plot_directory,
                            reborrow(&mut reporter),
                            span,
                        );
                    }

                    self.base.add_created_file(&full_filename);
                }
            }
        }

        if gen_map {
            if let Err(err) = self
                .base
                .create_map_files_set(plot_directory, reborrow(&mut reporter))
            {
                success = Err(err);
            }
        }

        // REPORTER::ReportTail
        if let Some(rep) = reborrow(&mut reporter) {
            rep.report("Done.", Severity::Info);
        }

        success
    }

    /// `GENDRILL_WRITER_BASE::GenDrillReportFile( aFullFileName, aReporter )` for this writer
    /// (base-class file names, the writer's creation date, the board's hole lists); see
    /// [`GendrillWriterBase::gen_drill_report_file`].
    pub fn gen_drill_report_file(
        &mut self,
        full_file_name: &str,
        reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        self.gen_drill_report_file_from(&BoardHoleSource, full_file_name, reporter)
    }

    /// [`Self::gen_drill_report_file`] with the hole lists taken from `source`.
    pub fn gen_drill_report_file_from(
        &mut self,
        source: &dyn HoleListSource,
        full_file_name: &str,
        reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        let date = self.creation_date;
        self.base.gen_drill_report_file(
            full_file_name,
            &date,
            &|w, span, npth, merge| w.drill_file_name(span, npth, merge),
            source,
            reporter,
        )
    }

    /// Moves the finished `m_file` buffer to `base.outputs` (the C++ `fclose`).
    fn flush_file(&mut self, full_filename: &str) {
        let contents = std::mem::take(&mut self.file).into_bytes();
        self.base.outputs.push(OutputFile {
            name: full_filename.to_string(),
            contents,
        });
    }

    /// Port of `EXCELLON_WRITER::writeHoleAttribute( aAttribute )`: write a comment string
    /// giving the hole attribute. Hole attributes are comments (lines starting by ';') in the
    /// drill files. For tools (file header), they are similar to X2 apertures attributes; for
    /// attributes added in coordinate list, they are just comments.
    fn write_hole_attribute(&mut self, attribute: HoleAttribute) {
        if !self.minimal_header {
            let text = match attribute {
                HoleAttribute::HoleViaThrough => "; #@! TA.AperFunction,Plated,PTH,ViaDrill\n",
                HoleAttribute::HoleViaBuried => "; #@! TA.AperFunction,Plated,Buried,ViaDrill\n",
                HoleAttribute::HoleViaBackdrill | HoleAttribute::HolePadBackdrill => {
                    "; #@! TA.AperFunction,NonPlated,BackDrill\n"
                }
                HoleAttribute::HolePad => "; #@! TA.AperFunction,Plated,PTH,ComponentDrill\n",
                HoleAttribute::HolePadCastellated => {
                    "; #@! TA.AperFunction,Plated,PTH,CastelletedDrill\n"
                }
                HoleAttribute::HolePadPressfit => {
                    "; #@! TA.AperFunction,Plated,PTH,ComponentDrill,PressFit\n"
                }
                HoleAttribute::HoleMechanical => {
                    "; #@! TA.AperFunction,NonPlated,NPTH,ComponentDrill\n"
                }
                HoleAttribute::HoleUnknown => "; #@! TD\n",
            };

            self.file.push_str(text);
        }
    }

    /// Port of `EXCELLON_WRITER::createDrillFile( aFile, aSpan, aHolesType,
    /// aTagBackdrillHit )`: write an Excellon drill file for the current hole list into the
    /// `m_file` buffer. `tag_backdrill_hit` adds a `; backdrill` comment before each backdrill
    /// hit. Returns the hole count.
    fn create_drill_file(
        &mut self,
        span: &DrillSpan,
        holes_type: TypeFile,
        tag_backdrill_hit: bool,
    ) -> i32 {
        // if units are mm, the resolution is 0.001 mm (3 digits in mantissa)
        // if units are inches, the resolution is 0.1 mil (4 digits in mantissa)
        self.mantissa_lenght = if self.base.units_metric { 3 } else { 4 };

        self.file.clear();

        self.write_excellon_header(span, holes_type);

        let mut holes_count = 0;
        let mantissa = self.mantissa_lenght as usize;

        // Write the tool list
        for ii in 0..self.base.tool_list_buffer.len() {
            let tool_descr = self.base.tool_list_buffer[ii].clone();

            if USE_ATTRIB_FOR_HOLES {
                self.write_hole_attribute(tool_descr.hole_attribute);
            }

            self.file.push_str(&format!(
                "T{}C{}\n",
                ii + 1,
                format_f(
                    f64::from(tool_descr.diameter) * self.base.conversion_units,
                    mantissa
                )
            ));

            if !self.minimal_header {
                if tool_descr.is_backdrill {
                    let format_stub = |stub_length: i32| -> String {
                        let stub_mm = PCB_IU_SCALE.iu_to_mm(stub_length);
                        let stub_inches = stub_mm / 25.4;
                        format!(
                            "{}mm ({}\")",
                            format_f(stub_mm, 3),
                            format_f(stub_inches, 4)
                        )
                    };

                    let mut comment = String::from("; Backdrill");

                    if let Some(min_stub) = tool_descr.min_stub_length {
                        comment.push_str(" stub ");
                        comment.push_str(&format_stub(min_stub));

                        if let Some(max_stub) = tool_descr.max_stub_length {
                            if tool_descr.max_stub_length != tool_descr.min_stub_length {
                                comment.push_str(" to ");
                                comment.push_str(&format_stub(max_stub));
                            }
                        }
                    }

                    if tool_descr.has_post_machining {
                        comment.push_str(", post-machining");
                    }

                    comment.push('\n');
                    self.file.push_str(&comment);
                } else if tool_descr.has_post_machining {
                    self.file.push_str("; Post-machining\n");
                }
            }
        }

        self.file.push_str("%\n"); // End of header info
        self.file.push_str("G90\n"); // Absolute mode
        self.file.push_str("G05\n"); // Drill mode

        // Read the hole list and generate data for normal holes (oblong
        // holes will be created later)
        let mut tool_reference = -2;

        for ii in 0..self.base.hole_list_buffer.len() {
            let hole_descr = &self.base.hole_list_buffer[ii];

            if hole_descr.is_slot {
                continue; // oblong holes will be created later
            }

            if tool_reference != self.base.hole_tool_references[ii] {
                tool_reference = self.base.hole_tool_references[ii];
                self.file.push_str(&format!("T{tool_reference}\n"));
            }

            let hole_descr = &self.base.hole_list_buffer[ii];
            let x0 = hole_descr.position.x - self.base.offset.x;
            let mut y0 = hole_descr.position.y - self.base.offset.y;

            if !self.mirror {
                y0 *= -1;
            }

            let xt = f64::from(x0) * self.base.conversion_units;
            let yt = f64::from(y0) * self.base.conversion_units;
            let comments = self.hole_comments(hole_descr, tag_backdrill_hit);
            let line = self.write_coordinates(xt, yt);

            self.file.push_str(&comments);
            self.file.push_str(&line);
            holes_count += 1;
        }

        // Read the hole list and generate data for oblong holes
        tool_reference = -2; // set to a value not used for m_holeToolReferences[ii]

        for ii in 0..self.base.hole_list_buffer.len() {
            let hole_descr = &self.base.hole_list_buffer[ii];

            if !hole_descr.is_slot {
                continue; // wait for oblong holes
            }

            if tool_reference != self.base.hole_tool_references[ii] {
                tool_reference = self.base.hole_tool_references[ii];
                self.file.push_str(&format!("T{tool_reference}\n"));
            }

            let hole_descr = &self.base.hole_list_buffer[ii];
            let diam = hole_descr.size_xy.x.min(hole_descr.size_xy.y);

            if diam == 0 {
                continue;
            }

            // Compute the hole coordinates:
            let xc = hole_descr.position.x - self.base.offset.x;
            let yc = hole_descr.position.y - self.base.offset.y;
            let (mut x0, mut xf) = (xc, xc);
            let (mut y0, mut yf) = (yc, yc);

            // Compute the start and end coordinates for the shape
            if hole_descr.size_xy.x < hole_descr.size_xy.y {
                let delta = (hole_descr.size_xy.y - hole_descr.size_xy.x) / 2;
                y0 -= delta;
                yf += delta;
            } else {
                let delta = (hole_descr.size_xy.x - hole_descr.size_xy.y) / 2;
                x0 -= delta;
                xf += delta;
            }

            rotate_point_xy_around(&mut x0, &mut y0, xc, yc, hole_descr.orientation);
            rotate_point_xy_around(&mut xf, &mut yf, xc, yc, hole_descr.orientation);

            if !self.mirror {
                y0 *= -1;
                yf *= -1;
            }

            let mut xt = f64::from(x0) * self.base.conversion_units;
            let mut yt = f64::from(y0) * self.base.conversion_units;
            let comments = self.hole_comments(hole_descr, tag_backdrill_hit);
            self.file.push_str(&comments);

            if self.use_route_mode_for_oval {
                self.file.push_str("G00"); // Select the routing mode
            }

            let mut line = self.write_coordinates(xt, yt);

            if !self.use_route_mode_for_oval {
                // remove the '\n' from end of line, because we must add the "G85"
                // command to the line: the C++ truncates at the first control character.
                if let Some(cut) = line.find(|c: char| c < ' ') {
                    line.truncate(cut);
                }

                self.file.push_str(&line);
                self.file.push_str("G85"); // add the "G85" command
            } else {
                self.file.push_str(&line);
                self.file.push_str("M15\nG01"); // tool down and linear routing from last coordinates
            }

            xt = f64::from(xf) * self.base.conversion_units;
            yt = f64::from(yf) * self.base.conversion_units;
            line = self.write_coordinates(xt, yt);

            self.file.push_str(&line);

            if self.use_route_mode_for_oval {
                self.file.push_str("M16\n"); // Tool up (end routing)
            }

            self.file.push_str("G05\n"); // Select drill mode
            holes_count += 1;
        }

        self.write_excellon_end_of_file();

        holes_count
    }

    /// Port of `EXCELLON_WRITER::writeCoordinates( aLine, aLineSize, aCoordX, aCoordY )`:
    /// the `X<x>Y<y>\n` line for coordinates already converted to file units, according to
    /// the selected zero format (`base.zero_format`, `base.precision`) and
    /// `mantissa_lenght`.
    pub fn write_coordinates(&self, mut coord_x: f64, mut coord_y: f64) -> String {
        let mut xpad = (self.base.precision.lhs + self.base.precision.rhs) as usize;
        let mut ypad = xpad;

        // if units are mm, the resolution is 0.001 mm (3 digits in mantissa)
        // if units are inches, the resolution is 0.1 mil (4 digits in mantissa)
        // in DECIMAL_FORMAT we could use more digits.

        match self.base.zero_format {
            ZerosFmt::DecimalFormat => {
                // In Excellon files, resolution is 1/1000 mm or 1/10000 inch (0.1 mil)
                // Although in decimal format, Excellon specifications do not specify
                // clearly the resolution. However it seems to be usually 1/1000mm or 0.1 mil
                // like in non decimal formats, so we trunk coordinates to m_mantissaLenght in
                // mantissa. Decimal format just prohibit useless leading 0:
                // 0.45 or .45 is right, but 00.54 is incorrect.
                let mantissa = self.mantissa_lenght as usize;
                let mut xs = format_f(coord_x, mantissa);
                let mut ys = format_f(coord_y, mantissa);

                // Remove useless trailing 0
                while xs.ends_with('0') {
                    xs.pop();
                }

                if xs.ends_with('.') {
                    // however keep a trailing 0 after the floating point separator
                    xs.push('0');
                }

                while ys.ends_with('0') {
                    ys.pop();
                }

                if ys.ends_with('.') {
                    ys.push('0');
                }

                format!("X{xs}Y{ys}\n")
            }

            ZerosFmt::SuppressLeading => {
                for _ in 0..self.base.precision.rhs {
                    coord_x *= 10.0;
                    coord_y *= 10.0;
                }

                format!("X{}Y{}\n", ki_round(coord_x), ki_round(coord_y))
            }

            ZerosFmt::SuppressTrailing => {
                for _ in 0..self.base.precision.rhs {
                    coord_x *= 10.0;
                    coord_y *= 10.0;
                }

                if coord_x < 0.0 {
                    xpad += 1;
                }

                if coord_y < 0.0 {
                    ypad += 1;
                }

                let mut xs = format!("{:0xpad$}", ki_round(coord_x));
                let mut ys = format!("{:0ypad$}", ki_round(coord_y));

                // while( xs[j] == '0' && j ) xs.Truncate( j-- ): keeps the first character
                while xs.len() > 1 && xs.ends_with('0') {
                    xs.pop();
                }

                while ys.len() > 1 && ys.ends_with('0') {
                    ys.pop();
                }

                format!("X{xs}Y{ys}\n")
            }

            ZerosFmt::KeepZeros => {
                for _ in 0..self.base.precision.rhs {
                    coord_x *= 10.0;
                    coord_y *= 10.0;
                }

                if coord_x < 0.0 {
                    xpad += 1;
                }

                if coord_y < 0.0 {
                    ypad += 1;
                }

                let xs = format!("{:0xpad$}", ki_round(coord_x));
                let ys = format!("{:0ypad$}", ki_round(coord_y));
                format!("X{xs}Y{ys}\n")
            }
        }
    }

    /// Port of `EXCELLON_WRITER::writeEXCELLONHeader( aSpan, aHolesType )`: print the DRILL
    /// file header. The full header is something like:
    /// ```text
    /// M48
    /// ;DRILL file {PCBNEW (2007-11-29-b)} date 17/1/2008-21:02:35
    /// ;FORMAT={ <precision> / absolute / <units> / <numbers format>}
    /// ; #@! TF.FileFunction,Plated,1,4,PTH
    /// ; #@! TF.CreationDate,2018-11-23T15:59:51+01:00
    /// ; #@! TF.GenerationSoftware,Kicad,Pcbnew,2017.04
    /// FMAT,2
    /// INCH,TZ
    /// ```
    fn write_excellon_header(&mut self, span: &DrillSpan, holes_type: TypeFile) {
        self.file.push_str("M48\n"); // The beginning of a header

        if !self.minimal_header {
            // The next lines in EXCELLON files are comments:
            let msg = format!("KiCad {}", self.build_version);

            self.file.push_str(&format!(
                "; DRILL file {} date {}\n",
                msg,
                self.creation_date.format_iso_combined()
            ));
            let mut msg = String::from("; FORMAT={");

            // Print precision:
            // Note in decimal format the precision is not used.
            // the floating point notation has higher priority than the precision.
            if self.base.zero_format != ZerosFmt::DecimalFormat {
                msg.push_str(&self.base.precision.precision_string());
            } else {
                msg.push_str("-:-"); // in decimal format the precision is irrelevant
            }

            msg.push_str("/ absolute / ");
            msg.push_str(if self.base.units_metric {
                "metric"
            } else {
                "inch"
            });

            // Adding numbers notation format.
            // this is same as m_Choice_Zeros_Format strings, but NOT translated
            // because some EXCELLON parsers do not like non ASCII values
            // so we use ONLY English (ASCII) strings.
            // if new options are added in m_Choice_Zeros_Format, they must also
            // be added here
            msg.push_str(" / ");

            const ZERO_FMT: [&str; 4] = [
                "decimal",
                "suppress leading zeros",
                "suppress trailing zeros",
                "keep zeros",
            ];

            msg.push_str(ZERO_FMT[self.base.zero_format as usize]);
            msg.push_str("}\n");
            self.file.push_str(&msg);

            // add the structured comment TF.CreationDate:
            // The attribute value must conform to the full version of the ISO 8601
            let msg = gbr_make_creation_date_attribute_string(
                GbrNcStringFormat::Ncdrill,
                &self.creation_date,
            ) + "\n";
            self.file.push_str(&msg);

            // Add the application name that created the drill file
            let msg = format!(
                "; #@! TF.GenerationSoftware,Kicad,Pcbnew,{}\n",
                self.build_version
            );
            self.file.push_str(&msg);

            // Add the standard X2 FileFunction for drill files
            // TF.FileFunction,Plated[NonPlated],layer1num,layer2num,PTH[NPTH]
            let msg = self
                .base
                .build_file_function_attribute_string(span, holes_type, true)
                + "\n";
            self.file.push_str(&msg);

            self.file.push_str("FMAT,2\n"); // Use Format 2 commands (version used since 1979)
        }

        self.file.push_str(if self.base.units_metric {
            "METRIC"
        } else {
            "INCH"
        });

        match self.base.zero_format {
            ZerosFmt::DecimalFormat => self.file.push('\n'),
            ZerosFmt::SuppressLeading => self.file.push_str(",TZ\n"),
            ZerosFmt::SuppressTrailing => self.file.push_str(",LZ\n"),
            // write nothing, but TZ is acceptable when all zeros are kept
            ZerosFmt::KeepZeros => self.file.push('\n'),
        }
    }

    /// The text [`Self::write_excellon_header`] writes (for the differential tests).
    pub fn excellon_header_text(&mut self, span: &DrillSpan, holes_type: TypeFile) -> String {
        let saved = std::mem::take(&mut self.file);
        self.write_excellon_header(span, holes_type);
        std::mem::replace(&mut self.file, saved)
    }

    /// Port of `EXCELLON_WRITER::writeEXCELLONEndOfFile`.
    fn write_excellon_end_of_file(&mut self) {
        // add if minimal here
        self.file.push_str("M30\n");
        // fclose( m_file ): the caller moves the buffer to the outputs.
    }

    /// Port of `EXCELLON_WRITER::getBackdrillLayerPairFileName( aSpan )`:
    /// `<board>-<pair>-backdrill.<ext>` (no directory).
    fn backdrill_layer_pair_file_name(&self, span: &DrillSpan) -> WxFileName {
        let mut fn_ = WxFileName::new(&self.base.pcb.board.file_name);
        let extend = format!("-{}-backdrill", layer_pair_name(span.pair()));

        fn_.set_name(&format!("{}{}", fn_.name(), extend));
        fn_.set_ext(&self.base.drill_file_extension);

        fn_
    }

    /// Port of `EXCELLON_WRITER::writeBackdrillLayerPairFile( aPlotDirectory, aReporter,
    /// aSpan )`: the backdrill span's holes written again under the layer pair name, with a
    /// `; backdrill` comment before each hit. Cannot fail without a file system (C++ returns
    /// `false` when the file cannot be created or written); returns `true`.
    fn write_backdrill_layer_pair_file(
        &mut self,
        plot_directory: &str,
        reporter: Option<&mut dyn Reporter>,
        span: &DrillSpan,
    ) -> bool {
        let mut fn_ = self.backdrill_layer_pair_file_name(span);
        fn_.set_path(plot_directory);

        let full_filename = fn_.full_path();

        if let Some(rep) = reporter {
            let msg = format!("Created file '{full_filename}'");
            rep.report(&msg, Severity::Action);
        }

        self.create_drill_file(span, TypeFile::NpthFile, true);
        self.flush_file(&full_filename);

        self.base.add_created_file(&full_filename);

        true
    }

    /// Port of `EXCELLON_WRITER::writeHoleComments( aHole, aTagBackdrillHit )`: the comment
    /// lines written before a hit (returned instead of written).
    fn hole_comments(&self, hole: &DrillOperation, tag_backdrill_hit: bool) -> String {
        let mut text = String::new();

        if tag_backdrill_hit && hole.is_backdrill() {
            text.push_str("; backdrill\n");
        }

        text.push_str(&self.post_machining_comment(
            hole.front_post_machining.mode,
            hole.front_post_machining.size,
            hole.front_post_machining.depth,
            hole.front_post_machining.angle,
            "front",
        ));

        text.push_str(&self.post_machining_comment(
            hole.back_post_machining.mode,
            hole.back_post_machining.size,
            hole.back_post_machining.depth,
            hole.back_post_machining.angle,
            "back",
        ));

        text
    }

    /// Port of `EXCELLON_WRITER::writePostMachiningComment( aMode, aSizeIU, aDepthIU,
    /// aAngleDeciDegree, aSideLabel )`: the `; Post-machining ...` line (with its newline),
    /// or an empty string when the mode is neither counterbore nor countersink.
    pub fn post_machining_comment(
        &self,
        mode: PadDrillPostMachiningMode,
        size_iu: i32,
        depth_iu: i32,
        angle_deci_degree: i32,
        side_label: &str,
    ) -> String {
        if mode != PadDrillPostMachiningMode::Counterbore
            && mode != PadDrillPostMachiningMode::Countersink
        {
            return String::new();
        }

        let mut comment = format!(
            "; Post-machining {} {}",
            side_label,
            if mode == PadDrillPostMachiningMode::Countersink {
                "countersink"
            } else {
                "counterbore"
            }
        );

        let size_str = self.format_linear_value(size_iu);

        if !size_str.is_empty() {
            comment.push_str(" dia ");
            comment.push_str(&size_str);
        }

        let depth_str = self.format_linear_value(depth_iu);

        if !depth_str.is_empty() {
            comment.push_str(" depth ");
            comment.push_str(&depth_str);
        }

        if mode == PadDrillPostMachiningMode::Countersink && angle_deci_degree > 0 {
            let angle = f64::from(angle_deci_degree) / 10.0;

            let angle_str = if angle_deci_degree % 10 == 0 {
                format!("{}deg", format_f(angle, 0))
            } else {
                format!("{}deg", format_f(angle, 1))
            };

            comment.push_str(" angle ");
            comment.push_str(&angle_str);
        }

        comment.push('\n');
        comment
    }

    /// Port of `EXCELLON_WRITER::formatLinearValue( aValueIU )`: `<value><mm|in>` with
    /// `mantissa_lenght` decimals, empty for values <= 0.
    pub fn format_linear_value(&self, value_iu: i32) -> String {
        if value_iu <= 0 {
            return String::new();
        }

        let converted = f64::from(value_iu) * self.base.conversion_units;
        let mut value = format_f(converted, self.mantissa_lenght as usize);
        value.push_str(if self.base.units_metric { "mm" } else { "in" });

        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_pcb::Board;

    fn board(file: &str, copper: i32) -> Board {
        let mut b = Board::default();
        b.file_name = file.to_string();
        b.set_copper_layer_count(copper);
        b
    }

    #[test]
    fn set_format_defaults_and_conversion() {
        let b = board("/p/demo.kicad_pcb", 2);
        let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
        assert_eq!(w.base.drill_file_extension, "drl");
        assert_eq!(w.base.conversion_units, 0.0001);
        assert!(w.use_route_mode_for_oval);
        w.set_format(true, ZerosFmt::KeepZeros, 0, 0);
        assert_eq!((w.base.precision.lhs, w.base.precision.rhs), (3, 3));
        assert_eq!(w.base.conversion_units, 1.0 / 1_000_000.0);
        w.set_format(false, ZerosFmt::KeepZeros, -1, 5);
        assert_eq!((w.base.precision.lhs, w.base.precision.rhs), (2, 5));
        assert_eq!(w.base.conversion_units, 0.001 / 25_400.0);
    }

    #[test]
    fn coordinates_in_every_zero_format() {
        let b = board("/p/demo.kicad_pcb", 2);
        let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
        w.set_format(true, ZerosFmt::DecimalFormat, 0, 0);
        assert_eq!(w.write_coordinates(200.0, -148.25), "X200.0Y-148.25\n");
        assert_eq!(w.write_coordinates(-0.0001, 0.0), "X-0.0Y0.0\n");
        w.set_format(true, ZerosFmt::SuppressLeading, 0, 0);
        assert_eq!(w.write_coordinates(1.5, -0.0005), "X1500Y-1\n");
        w.set_format(true, ZerosFmt::KeepZeros, 0, 0);
        assert_eq!(w.write_coordinates(1.5, -0.12), "X001500Y-000120\n");
        w.set_format(true, ZerosFmt::SuppressTrailing, 0, 0);
        assert_eq!(w.write_coordinates(1.5, -0.12), "X0015Y-00012\n");
        assert_eq!(w.write_coordinates(0.0, -0.0001), "X0Y0\n");
    }

    #[test]
    fn post_machining_comment_texts() {
        let b = board("/p/demo.kicad_pcb", 2);
        let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
        w.set_format(true, ZerosFmt::DecimalFormat, 0, 0);
        // qa/tests/pcbnew/test_cam_backdrill.cpp BackdrillCamOutputs
        assert_eq!(
            w.post_machining_comment(
                PadDrillPostMachiningMode::Countersink,
                600_000,
                150_000,
                900,
                "front"
            ),
            "; Post-machining front countersink dia 0.600mm depth 0.150mm angle 90deg\n"
        );
        assert_eq!(
            w.post_machining_comment(PadDrillPostMachiningMode::Countersink, 0, 0, 825, "back"),
            "; Post-machining back countersink angle 82.5deg\n"
        );
        assert_eq!(
            w.post_machining_comment(PadDrillPostMachiningMode::NotPostMachined, 1, 1, 1, "x"),
            ""
        );
    }
}
