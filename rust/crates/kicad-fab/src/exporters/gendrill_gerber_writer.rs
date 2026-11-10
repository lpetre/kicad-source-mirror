// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2017 Jean_Pierre Charras <jp.charras at wanadoo.fr>
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

//! Port of `pcbnew/exporters/gendrill_gerber_writer.h` / `.cpp` (`GERBER_WRITER`: Gerber X2
//! drill files and the IPC-4761 via protection files; work package FAB-08).
//!
//! The drill files are plotted through the Gerber plotter of `kicad-plot`: one flash per round
//! hole, one routed segment per oblong hole (`FLASH_OVAL_HOLE == 0`), with the aperture
//! attributes `ViaDrill` / `BackDrill` / `ComponentDrill` / `CastellatedDrill` /
//! `ComponentDrill,PressFit` / `ComponentDrill,Slot`-style oblong drill and the `TO.C` object
//! attribute of pad holes. The protection files (filling, capping, covering, plugging,
//! tenting) flash one circle per via having the feature.
//!
//! Differences to the C++ class, all forced by the port's rules (no file system, no clock):
//! finished files are appended to `base.outputs`; the build version (`GetBuildVersion()`) and
//! the creation date (`wxDateTime::Now()`) are set by the caller. Hole lists come through a
//! [`HoleListSource`] (the board by default); the pad / via data the writer reads besides the
//! hole list (pad property, footprint reference, via widths) always comes from the board.

use kicad_core::PCB_IU_SCALE;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::report_severity::Severity;
use kicad_geom::trigo::rotate_point;
use kicad_geom::{ANGLE_90, EdaAngle, Vector2I};
use kicad_pcb::BoardCtx;
use kicad_pcb::padstack::PadProp;
use kicad_pcb::pcb_track::Track;
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrCreationDate, GbrMetadata};
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;
use kicad_plot::{GerberPlotter, Plotter};

use super::gendrill_excellon_writer::reborrow;
use super::gendrill_report::{BoardHoleSource, HoleListSource};
use super::gendrill_writer_base::{
    GendrillWriterBase, Ipc4761Features, TypeFile, ZerosFmt, layer_pair_name,
};
use crate::drill::{DrillOperation, DrillSource, DrillSpan};
use crate::pcbplot::{PlotEnv, add_gerber_x2_header};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile, Reporter};

/// The extension of Gerber drill files (`m_drillFileExtension = wxT( "gbr" )`).
pub const GERBER_DRILL_FILE_EXTENSION: &str = "gbr";

/// `GERBER_WRITER`: create Gerber X2 drill files, via protection files, drill maps and the
/// drill report.
#[derive(Clone)]
pub struct GerberWriter<'a> {
    /// The `GENDRILL_WRITER_BASE` part.
    pub base: GendrillWriterBase<'a>,
    /// `GetBuildVersion()` (a parameter in the port).
    pub build_version: String,
    /// `wxDateTime::Now()` with the local UTC offset (a parameter in the port).
    pub creation_date: GbrCreationDate,
}

impl<'a> GerberWriter<'a> {
    /// Port of `GERBER_WRITER::GERBER_WRITER( aPcb )`.
    pub fn new(pcb: BoardCtx<'a>) -> Self {
        let mut base = GendrillWriterBase::new(pcb);
        base.zero_format = ZerosFmt::SuppressLeading;
        base.conversion_units = 1.0;
        base.units_metric = true;
        base.drill_file_extension = GERBER_DRILL_FILE_EXTENSION.to_string();
        base.merge_pth_npth = false;

        GerberWriter {
            base,
            build_version: String::new(),
            creation_date: GbrCreationDate::default(),
        }
    }

    /// Sets the text `GetBuildVersion()` returns in C++ (`TF.GenerationSoftware`, the
    /// `G04 Created by` comment).
    pub fn set_build_version(&mut self, version: &str) {
        self.build_version = version.to_string();
    }

    /// Sets the date and time C++ takes from the clock (`TF.CreationDate`, the `G04 Created by`
    /// comment, the report).
    pub fn set_creation_date(&mut self, date: GbrCreationDate) {
        self.creation_date = date;
    }

    /// Port of `GERBER_WRITER::SetFormat( aRightDigits )`: initialize internal parameters to
    /// match the given format. `right_digits` is the number of digits for the mantissa part of
    /// coordinates (5 or 6; anything else gives 5).
    pub fn set_format(&mut self, right_digits: i32) {
        /* Set conversion scale depending on drill file units */
        self.base.conversion_units = 1.0 / PCB_IU_SCALE.iu_per_mm; // Gerber units = mm

        // Set precision (unit is mm).
        self.base.precision.lhs = 4;
        self.base.precision.rhs = if right_digits == 6 { 6 } else { 5 };
    }

    /// Port of `GERBER_WRITER::SetOptions( aOffset )`: initialize internal parameters to match
    /// drill options. PTH and NPTH are always separate files in Gerber format.
    pub fn set_options(&mut self, offset: Vector2I) {
        self.base.offset = offset;
        self.base.merge_pth_npth = false;
    }

    /// The values `AddGerberX2Header` and the plotter read from the build and the clock.
    fn plot_env(&self) -> PlotEnv {
        PlotEnv {
            build_version: self.build_version.clone(),
            creation_date: self.creation_date,
            drawing_sheet: Default::default(),
        }
    }

    /// Port of `GERBER_WRITER::CreateDrillandMapFilesSet( aPlotDirectory, aGenDrill, aGenMap,
    /// aGenTenting, aReporter )`: create the full set of Gerber drill files for the board
    /// (file names are computed from the board name and layer ids), the via protection files
    /// (with `gen_tenting` also the tenting files) and, with `gen_map`, the drill maps.
    ///
    /// The files are appended to `base.outputs` (with `plot_directory` as directory part) and
    /// their names to `base.created_files` (in the C++ `AddCreatedFile` order). C++ returns
    /// `false` on failure; the port returns the error. Without a file system, only the drill
    /// map can fail (FAB-12 is not ported: `FabError::NotPorted`); as in C++ the other files
    /// are kept and `Done.` is reported before returning the failure.
    pub fn create_drill_and_map_files_set(
        &mut self,
        plot_directory: &str,
        gen_drill: bool,
        gen_map: bool,
        gen_tenting: bool,
        reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), FabError> {
        self.create_drill_and_map_files_set_from(
            &BoardHoleSource,
            plot_directory,
            gen_drill,
            gen_map,
            gen_tenting,
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
        gen_tenting: bool,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), FabError> {
        let mut success: Result<(), FabError> = Ok(());
        // Note: In Gerber drill files, NPTH and PTH are always separate files
        self.base.merge_pth_npth = false;

        let mut hole_sets = source.drill_spans(&self.base);
        let mut side_protection_holes: Vec<DrillOperation> = Vec::new();

        hole_sets.push(DrillSpan::new(
            PcbLayerId::FCu,
            PcbLayerId::BCu,
            false,
            true,
        ));

        for span in &hole_sets {
            let doing_npth = span.is_non_plated_file;

            source.build_holes_list(&mut self.base, span, doing_npth);

            if self.base.holes_count() == 0 {
                continue;
            }

            let mut fn_ =
                WxFileName::new(&self.drill_file_name(span, doing_npth, self.base.merge_pth_npth));
            fn_.set_path(plot_directory);

            if gen_drill {
                let full_filename = fn_.full_path();
                let is_non_plated = doing_npth || span.is_backdrill;

                // createDrillFile cannot fail without a file system ("Failed to create file").
                self.create_drill_file(&full_filename, is_non_plated, span);

                if let Some(rep) = reborrow(&mut reporter) {
                    let msg = format!("Created file '{full_filename}'.");
                    rep.report(&msg, Severity::Action);
                }

                if span.is_backdrill {
                    self.write_backdrill_layer_pair_file(
                        plot_directory,
                        reborrow(&mut reporter),
                        span,
                    );
                }

                self.base.add_created_file(&full_filename);
            }

            if doing_npth {
                continue;
            }

            // Side files name only the outer layer, so every plated span must share one file
            if !span.is_backdrill {
                for hole in &self.base.hole_list_buffer {
                    if matches!(hole.source_item, Some(DrillSource::Via(_))) {
                        side_protection_holes.push(hole.clone());
                    }
                }
            }

            self.write_protection(
                Ipc4761Features::Filled,
                span,
                plot_directory,
                reborrow(&mut reporter),
            );
            self.write_protection(
                Ipc4761Features::Capped,
                span,
                plot_directory,
                reborrow(&mut reporter),
            );
        }

        std::mem::swap(&mut self.base.hole_list_buffer, &mut side_protection_holes);
        let outer_span = DrillSpan::new(PcbLayerId::FCu, PcbLayerId::BCu, false, false);

        for feature in [
            Ipc4761Features::CoveredBack,
            Ipc4761Features::CoveredFront,
            Ipc4761Features::PluggedBack,
            Ipc4761Features::PluggedFront,
            Ipc4761Features::TentedBack,
            Ipc4761Features::TentedFront,
        ] {
            if !gen_tenting
                && (feature == Ipc4761Features::TentedBack
                    || feature == Ipc4761Features::TentedFront)
            {
                continue;
            }

            self.write_protection(
                feature,
                &outer_span,
                plot_directory,
                reborrow(&mut reporter),
            );
        }

        std::mem::swap(&mut self.base.hole_list_buffer, &mut side_protection_holes);

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

    /// The `writeProtection` lambda of `CreateDrillandMapFilesSet`: write the protection file
    /// of `feature` for `span` if a via of the current hole list has the feature.
    fn write_protection(
        &mut self,
        feature: Ipc4761Features,
        span: &DrillSpan,
        plot_directory: &str,
        reporter: Option<&mut dyn Reporter>,
    ) {
        if !self.has_via_type(feature) {
            return;
        }

        let mut fn_ = WxFileName::new(&self.base.protection_file_name(span, feature));
        fn_.set_path(plot_directory);

        let full_filename = fn_.full_path();

        // createProtectionFile fails only on a file error, impossible without a file system.
        self.create_protection_file(&full_filename, feature, span.pair());

        if let Some(rep) = reporter {
            let msg = format!("Created file '{full_filename}'.");
            rep.report(&msg, Severity::Action);
        }

        self.base.add_created_file(&full_filename);
    }

    /// `GENDRILL_WRITER_BASE::GenDrillReportFile( aFullFileName, aReporter )` for this writer
    /// (the `GERBER_WRITER` file names, the writer's creation date, the board's hole lists);
    /// see [`GendrillWriterBase::gen_drill_report_file`].
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
            &gerber_drill_file_name,
            source,
            reporter,
        )
    }

    /// Port of `GERBER_WRITER::getDrillFileName( aSpan, aNPTH, aMerge_PTH_NPTH )` (the
    /// override of the base-class version): Gerber files extension is always `.gbr`, so
    /// `-drl` is added to the name to mark drill files.
    pub fn drill_file_name(&self, span: &DrillSpan, npth: bool, merge_pth_npth: bool) -> String {
        gerber_drill_file_name(&self.base, span, npth, merge_pth_npth)
    }

    /// A new Gerber plotter set up as `createDrillFile` / `createProtectionFile` do before
    /// adding their `TF.FileFunction` line.
    fn new_plotter(&self) -> GerberPlotter {
        let env = self.plot_env();
        let mut plotter = GerberPlotter::new();
        plotter.set_build_version(&env.build_version);
        plotter.set_creation_date(&env.gerber_comment_date());

        // Gerber drill file imply X2 format:
        plotter.use_x2format(true);
        plotter.use_x2_net_attributes(true);
        plotter.disable_apert_macros(false);

        // Add the standard X2 header, without FileFunction
        add_gerber_x2_header(&mut plotter, self.base.pcb.board, &env, false);
        plotter.set_viewport(
            self.base.offset,
            PCB_IU_SCALE.iu_per_mils / 10.0,
            /* scale */ 1.0,
            /* mirror */ false,
        );

        // has meaning only for gerber plotter. Must be called only after SetViewport
        plotter.set_gerber_coordinates_format(self.base.precision.rhs, false);
        plotter.set_creator("PCBNEW");

        plotter
    }

    /// Moves a finished plot to `base.outputs` (the C++ file written by `EndPlot`).
    fn flush_plotter(&mut self, plotter: &GerberPlotter, full_filename: &str) {
        self.base.outputs.push(OutputFile {
            name: full_filename.to_string(),
            contents: plotter.output().as_bytes().to_vec(),
        });
    }

    /// Port of `GERBER_WRITER::createProtectionFile( aFullFilename, aFeature, aLayerPair )`:
    /// create a Gerber X2 file for the via protection feature `feature` from the vias of the
    /// current hole list. Returns the hole count (C++: -1 when the file cannot be created).
    fn create_protection_file(
        &mut self,
        full_filename: &str,
        feature: Ipc4761Features,
        layer_pair: (PcbLayerId, PcbLayerId),
    ) -> i32 {
        let _ = layer_pair; // unused in C++ as well
        let mut plotter = self.new_plotter();

        // Add the standard X2 FileFunction for drill files
        // %TF.FileFunction,Plated[NonPlated],layer1num,layer2num,PTH[NPTH][Blind][Buried],Drill[Rout][Mixed]*%
        let mut text = String::from("%TF,FileFunction,Other,");

        let attrib = match feature {
            Ipc4761Features::Capped => {
                text.push_str("Capping");
                "Capping"
            }
            Ipc4761Features::Filled => {
                text.push_str("Filling");
                "Filling"
            }
            Ipc4761Features::CoveredBack => {
                text.push_str("Covering-Back");
                "Covering"
            }
            Ipc4761Features::CoveredFront => {
                text.push_str("Covering-Front");
                "Covering"
            }
            Ipc4761Features::PluggedBack => {
                text.push_str("Plugging-Back");
                "Plugging"
            }
            Ipc4761Features::PluggedFront => {
                text.push_str("Plugging-Front");
                "Plugging"
            }
            Ipc4761Features::TentedBack => {
                text.push_str("Tenting-Back");
                "Tenting"
            }
            Ipc4761Features::TentedFront => {
                text.push_str("Tenting-Front");
                "Tenting"
            }
        };
        text.push_str("*%");
        plotter.add_line_to_header(&text);

        // Add file polarity (positive)
        plotter.add_line_to_header("%TF.FilePolarity,Positive*%");

        plotter.open_file(full_filename);
        plotter.start_plot("1");

        let mut holes_count = 0;
        let board = self.base.pcb.board;

        for hole_descr in &self.base.hole_list_buffer {
            let Some(DrillSource::Via(via_id)) = hole_descr.source_item else {
                continue;
            };

            let Some(Track::Via(via)) = board.track(via_id) else {
                continue;
            };

            let mut diameter = hole_descr.diameter;
            let cont = match feature {
                Ipc4761Features::Filled => !hole_descr.filled,
                Ipc4761Features::Capped => !hole_descr.capped,
                Ipc4761Features::CoveredBack => {
                    diameter = via.width(via.bottom_layer());
                    !hole_descr.bottom_covered
                }
                Ipc4761Features::CoveredFront => {
                    diameter = via.width(via.top_layer());
                    !hole_descr.top_covered
                }
                Ipc4761Features::PluggedBack => !hole_descr.bottom_plugged,
                Ipc4761Features::PluggedFront => !hole_descr.top_plugged,
                Ipc4761Features::TentedBack => {
                    diameter = via.width(via.bottom_layer());
                    !hole_descr.bottom_tented
                }
                Ipc4761Features::TentedFront => {
                    diameter = via.width(via.top_layer());
                    !hole_descr.top_tented
                }
            };

            if cont {
                continue;
            }

            let mut gbr_metadata = GbrMetadata::new();

            gbr_metadata.set_aperture_attrib_custom(attrib);

            plotter.flash_pad_circle(hole_descr.position, diameter, Some(&gbr_metadata));

            holes_count += 1;
        }

        plotter.end_plot();
        self.flush_plotter(&plotter, full_filename);

        holes_count
    }

    /// Port of `GERBER_WRITER::createDrillFile( aFullFilename, aIsNpth, aSpan )`: create a
    /// Gerber drill file from the current hole list. Returns the hole count (C++: -1 when the
    /// file cannot be created).
    fn create_drill_file(&mut self, full_filename: &str, is_npth: bool, span: &DrillSpan) -> i32 {
        let mut plotter = self.new_plotter();

        // Add the standard X2 FileFunction for drill files
        // %TF.FileFunction,Plated[NonPlated],layer1num,layer2num,PTH[NPTH][Blind][Buried],Drill[Rout][Mixed]*%
        let text = self.base.build_file_function_attribute_string(
            span,
            if is_npth {
                TypeFile::NpthFile
            } else {
                TypeFile::PthFile
            },
            false,
        );
        plotter.add_line_to_header(&text);

        // Add file polarity (positive)
        plotter.add_line_to_header("%TF.FilePolarity,Positive*%");

        plotter.open_file(full_filename);
        plotter.start_plot("1");

        let mut holes_count = 0;

        let ctx = &self.base.pcb;
        let mut last_item_is_via = true; // a flag to clear object attributes when a via hole is created.

        for hole_descr in &self.base.hole_list_buffer {
            let hole_pos = hole_descr.position;

            // Manage the aperture attributes: in drill files 3 attributes can be used:
            // "ViaDrill", only for vias, not pads
            // "ComponentDrill", only for Through Holes pads
            // "Slot" for oblong holes;
            let mut gbr_metadata = GbrMetadata::new();

            match hole_descr.source_item {
                Some(DrillSource::Via(_)) => {
                    if hole_descr.is_backdrill() {
                        gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Backdrill);
                    } else {
                        gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Viadrill);
                    }

                    if !last_item_is_via {
                        // be sure the current object attribute is cleared for vias
                        plotter.end_block(None);
                    }

                    last_item_is_via = true;
                }
                Some(DrillSource::Pad(pad_id)) => {
                    if let Some(pad) = ctx.board.pad(pad_id) {
                        last_item_is_via = false;

                        if pad.property() == PadProp::Castellated {
                            gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Castellateddrill);
                        } else if pad.property() == PadProp::Pressfit {
                            gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Pressfitdrill);
                        } else {
                            // Good practice of oblong pad holes (slots) is to use a specific
                            // aperture for routing, not used in drill commands.
                            if hole_descr.is_slot {
                                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::CmpOblongDrill);
                            } else {
                                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::CmpDrill);
                            }
                        }

                        // Add object attribute: component reference to pads (mainly useful
                        // for users)
                        let reference = pad
                            .parent_footprint(ctx)
                            .map(|fp| fp.reference(ctx))
                            .unwrap_or_default();

                        gbr_metadata.set_cmp_reference(&reference);
                        gbr_metadata.set_net_attrib_type(gbr_netinfo::CMP);
                    }
                }
                None => {}
            }

            if hole_descr.is_slot {
                // Use routing for oblong hole (Slots)
                let (start, end) =
                    convert_oblong_2_segment(hole_descr.size_xy, hole_descr.orientation);
                let width = hole_descr.size_xy.x.min(hole_descr.size_xy.y);

                if width == 0 {
                    continue;
                }

                plotter.thick_segment(start + hole_pos, end + hole_pos, width, Some(&gbr_metadata));
            } else {
                let diam = hole_descr.size_xy.x.min(hole_descr.size_xy.y);
                plotter.flash_pad_circle(hole_pos, diam, Some(&gbr_metadata));
            }

            holes_count += 1;
        }

        plotter.end_plot();
        self.flush_plotter(&plotter, full_filename);

        holes_count
    }

    /// Port of `GERBER_WRITER::getBackdrillLayerPairFileName( aSpan )`.
    fn backdrill_layer_pair_file_name(&self, span: &DrillSpan) -> WxFileName {
        let mut fn_ = WxFileName::new(&self.base.pcb.board.file_name);
        let pair_name = layer_pair_name(span.pair());

        fn_.set_name(&format!("{}-{}-backdrill-drl", fn_.name(), pair_name));
        fn_.set_ext(&self.base.drill_file_extension);

        fn_
    }

    /// Port of `GERBER_WRITER::writeBackdrillLayerPairFile( aPlotDirectory, aReporter, aSpan )`:
    /// the backdrill span's holes written again (as a non-plated file) under the layer pair
    /// name. Cannot fail without a file system (C++ returns `false` when the file cannot be
    /// created); returns `true`.
    fn write_backdrill_layer_pair_file(
        &mut self,
        plot_directory: &str,
        reporter: Option<&mut dyn Reporter>,
        span: &DrillSpan,
    ) -> bool {
        let mut fn_ = self.backdrill_layer_pair_file_name(span);
        fn_.set_path(plot_directory);

        let full_filename = fn_.full_path();

        self.create_drill_file(&full_filename, true, span);

        if let Some(rep) = reporter {
            let msg = format!("Created file '{full_filename}'.");
            rep.report(&msg, Severity::Action);
        }

        self.base.add_created_file(&full_filename);

        true
    }

    /// Port of `GERBER_WRITER::hasViaType( aFeature )`: true if at least one via of the
    /// current hole list has the given feature.
    fn has_via_type(&self, feature: Ipc4761Features) -> bool {
        for hole_descr in &self.base.hole_list_buffer {
            if !matches!(hole_descr.source_item, Some(DrillSource::Via(_))) {
                continue;
            }

            let has = match feature {
                Ipc4761Features::Filled => hole_descr.filled,
                Ipc4761Features::Capped => hole_descr.capped,
                Ipc4761Features::CoveredBack => hole_descr.bottom_covered,
                Ipc4761Features::CoveredFront => hole_descr.top_covered,
                Ipc4761Features::PluggedBack => hole_descr.bottom_plugged,
                Ipc4761Features::PluggedFront => hole_descr.top_plugged,
                Ipc4761Features::TentedBack => hole_descr.bottom_tented,
                Ipc4761Features::TentedFront => hole_descr.top_tented,
            };

            if has {
                return true;
            }
        }

        false
    }
}

/// Port of `GERBER_WRITER::getDrillFileName( aSpan, aNPTH, aMerge_PTH_NPTH )` as a free
/// function of the base part (the virtual call the drill report makes).
pub fn gerber_drill_file_name(
    base: &GendrillWriterBase<'_>,
    span: &DrillSpan,
    npth: bool,
    merge_pth_npth: bool,
) -> String {
    // Gerber files extension is always .gbr.
    // Therefore, to mark drill files, add "-drl" to the filename.
    let mut fname = WxFileName::new(&base.drill_file_name(span, npth, merge_pth_npth));
    fname.set_name(&format!("{}-drl", fname.name()));

    fname.full_path()
}

/// Port of the `static` `convertOblong2Segment( aSize, aOrient, aStart, aEnd )`: the segment
/// (relative to the hole centre) along which an oblong hole of size `size` and orientation
/// `orient` is routed. Returns `(start, end)`.
pub fn convert_oblong_2_segment(size: Vector2I, orient: EdaAngle) -> (Vector2I, Vector2I) {
    let mut size = size;
    let mut orient = orient;

    /* The pad will be drawn as an oblong shape with size.y > size.x
     * (Oval vertical orientation 0)
     */
    if size.x > size.y {
        std::mem::swap(&mut size.x, &mut size.y);
        orient += ANGLE_90;
    }

    let deltaxy = size.y - size.x; // distance between centers of the oval
    let mut start = Vector2I::new(0, deltaxy / 2);
    rotate_point(&mut start, orient);

    let mut end = Vector2I::new(0, -deltaxy / 2);
    rotate_point(&mut end, orient);

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_pcb::board::Board;

    #[test]
    fn oblong_segment_follows_the_long_axis() {
        // Vertical oval: along Y.
        let (s, e) =
            convert_oblong_2_segment(Vector2I::new(1000, 3000), EdaAngle::from_degrees(0.0));
        assert_eq!((s, e), (Vector2I::new(0, 1000), Vector2I::new(0, -1000)));
        // Horizontal oval: swapped and rotated by 90 degrees.
        let (s, e) =
            convert_oblong_2_segment(Vector2I::new(3000, 1000), EdaAngle::from_degrees(0.0));
        assert_eq!((s, e), (Vector2I::new(1000, 0), Vector2I::new(-1000, 0)));
        // Odd difference: integer division truncates as in C++.
        let (s, e) =
            convert_oblong_2_segment(Vector2I::new(1000, 2001), EdaAngle::from_degrees(0.0));
        assert_eq!((s, e), (Vector2I::new(0, 500), Vector2I::new(0, -500)));
    }

    #[test]
    fn file_names_and_format() {
        let mut board = Board::default();
        board.file_name = "/p/brd.kicad_pcb".to_string();
        let mut w = GerberWriter::new(BoardCtx::bare(&board));
        w.set_format(6);
        assert_eq!((w.base.precision.lhs, w.base.precision.rhs), (4, 6));
        w.set_format(7);
        assert_eq!(w.base.precision.rhs, 5);
        let span = DrillSpan::new(PcbLayerId::FCu, PcbLayerId::BCu, false, false);
        assert_eq!(w.drill_file_name(&span, false, false), "brd-PTH-drl.gbr");
        assert_eq!(w.drill_file_name(&span, true, false), "brd-NPTH-drl.gbr");
    }
}
