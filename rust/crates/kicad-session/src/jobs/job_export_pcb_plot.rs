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

//! Port of `common/jobs/job_export_pcb_plot.h` and `common/jobs/job_export_pcb_plot.cpp`
//! (`JOB_EXPORT_PCB_PLOT`, the base of the Gerber, SVG, PDF, ... plot jobs).

use kicad_core::plot_enums::DrillMarks;
use kicad_core::{Lseq, Lset};

use super::job::Job;

/// Port of `JOB_EXPORT_PCB_PLOT::PLOT_FORMAT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobPlotFormat {
    /// `HPGL`
    Hpgl,
    /// `GERBER`
    Gerber,
    /// `POST`
    Post,
    /// `DXF`
    Dxf,
    /// `PDF`
    Pdf,
    /// `SVG`
    Svg,
    /// `PNG`
    Png,
}

/// Port of `class JOB_EXPORT_PCB_PLOT : public JOB`.
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbPlot {
    /// The `JOB` base.
    pub job: Job,
    /// `m_argLayers`: the raw `--layers` argument, converted by the jobs handler.
    pub arg_layers: Option<String>,
    /// `m_argCommonLayers`: the raw `--common-layers` argument.
    pub arg_common_layers: Option<String>,
    /// `m_plotFormat`
    pub plot_format: JobPlotFormat,
    /// `m_filename`
    pub filename: String,
    /// `m_colorTheme`
    pub color_theme: String,
    /// `m_drawingSheet`
    pub drawing_sheet: String,
    /// `m_variant`
    pub variant: String,
    /// `m_mirror`
    pub mirror: bool,
    /// `m_blackAndWhite`
    pub black_and_white: bool,
    /// `m_negative`
    pub negative: bool,
    /// `m_scale`
    pub scale: f64,
    /// `m_sketchPadsOnFabLayers`
    pub sketch_pads_on_fab_layers: bool,
    /// `m_hideDNPFPsOnFabLayers`
    pub hide_dnp_fps_on_fab_layers: bool,
    /// `m_sketchDNPFPsOnFabLayers` (job default `true`; the CLI overwrites it)
    pub sketch_dnp_fps_on_fab_layers: bool,
    /// `m_crossoutDNPFPsOnFabLayers` (job default `true`; the CLI overwrites it)
    pub crossout_dnp_fps_on_fab_layers: bool,
    /// `m_plotFootprintValues`
    pub plot_footprint_values: bool,
    /// `m_plotRefDes`
    pub plot_ref_des: bool,
    /// `m_plotDrawingSheet`
    pub plot_drawing_sheet: bool,
    /// `m_subtractSolderMaskFromSilk`
    pub subtract_solder_mask_from_silk: bool,
    /// `m_plotPadNumbers`
    pub plot_pad_numbers: bool,
    /// `m_plotLayerSequence`
    pub plot_layer_sequence: Lseq,
    /// `m_plotAllCopperLayers`: plot every copper layer of the board in addition to
    /// `plot_layer_sequence`.
    pub plot_all_copper_layers: bool,
    /// `m_plotOnAllLayersSequence`: layers to include on all individual layer prints.
    pub plot_on_all_layers_sequence: Lseq,
    /// `m_drillShapeOption`: used by SVG & PDF.
    pub drill_shape_option: DrillMarks,
    /// `m_useDrillOrigin`: used by SVG/DXF/PDF/Gerbers.
    pub use_drill_origin: bool,
    /// `m_checkZonesBeforePlot`
    pub check_zones_before_plot: bool,
    /// The plot date the C++ takes from the clock (`wxDateTime::Now()`, local time and UTC
    /// offset): set by the host (no clock in library crates); the default is all zeros.
    pub creation_date: kicad_fab::jobs::job_export_pcb_plot::GbrCreationDate,
}

impl JobExportPcbPlot {
    /// Port of `JOB_EXPORT_PCB_PLOT::JOB_EXPORT_PCB_PLOT( aFormat, aType, aOutputIsDirectory )`.
    pub fn new(format: JobPlotFormat, type_: &str, output_is_directory: bool) -> Self {
        JobExportPcbPlot {
            job: Job::new(type_, output_is_directory),
            arg_layers: None,
            arg_common_layers: None,
            plot_format: format,
            filename: String::new(),
            color_theme: String::new(),
            drawing_sheet: String::new(),
            variant: String::new(),
            mirror: false,
            black_and_white: false,
            negative: false,
            scale: 1.0,
            sketch_pads_on_fab_layers: false,
            hide_dnp_fps_on_fab_layers: false,
            sketch_dnp_fps_on_fab_layers: true,
            crossout_dnp_fps_on_fab_layers: true,
            plot_footprint_values: true,
            plot_ref_des: true,
            plot_drawing_sheet: true,
            subtract_solder_mask_from_silk: false,
            plot_pad_numbers: false,
            plot_layer_sequence: Lseq::default(),
            plot_all_copper_layers: false,
            plot_on_all_layers_sequence: Lseq::default(),
            drill_shape_option: DrillMarks::FullDrillShape,
            use_drill_origin: false,
            check_zones_before_plot: false,
            creation_date: Default::default(),
        }
    }

    /// Port of `JOB_EXPORT_PCB_PLOT::ResolvePlotLayers( aEnabledLayers )`: the layers to plot on
    /// a board with `enabled_layers`. With `plot_all_copper_layers` set this adds every copper
    /// layer the board enables, so one job serves any layer count.
    pub fn resolve_plot_layers(&self, enabled_layers: &Lset) -> Lseq {
        if !self.plot_all_copper_layers {
            return self.plot_layer_sequence.clone();
        }

        let mut layers = Lset::from(&self.plot_layer_sequence);
        layers |= &(enabled_layers.clone() & Lset::all_cu_mask().clone());

        layers.seq_stackup_for_plotting()
    }
}
