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

//! Port of the option part of `common/jobs/job_export_pcb_plot.h` / `.cpp`
//! (`JOB_EXPORT_PCB_PLOT`: members, `PLOT_FORMAT`, constructor defaults, `ResolvePlotLayers`),
//! as far as `PCB_PLOTTER::PlotJobToPlotOpts` and the plot jobs of the jobs handler read it
//! (work package FAB-07).
//!
//! The full job (the `JOB` base with output path resolution, title block, the raw `--layers`
//! arguments, the `JOB_PARAM` list and the JSON (de)serialisation) is
//! `kicad_session::jobs::JobExportPcbPlot`. `kicad-session` depends on this crate, so this
//! crate cannot use that struct; the session maps its job onto this one (same members, same
//! defaults) after converting the layer arguments.

use std::collections::BTreeMap;

use kicad_core::plot_enums::DrillMarks;
use kicad_core::{Lseq, Lset};

/// The creation date the plot jobs take from the clock in C++ (`wxDateTime::Now()`).
pub use kicad_plot::gbr_metadata::GbrCreationDate;

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

/// The options of `JOB_EXPORT_PCB_PLOT` that reach the plot (without `m_filename`, the raw
/// layer arguments and `m_drawingSheet`, which the session handles).
#[derive(Clone, Debug, PartialEq)]
pub struct JobExportPcbPlot {
    /// `m_plotFormat`
    pub plot_format: JobPlotFormat,
    /// `m_colorTheme`
    pub color_theme: String,
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
    /// `m_plotLayerSequence` (after the jobs handler converted `m_argLayers`)
    pub plot_layer_sequence: Lseq,
    /// `m_plotAllCopperLayers`
    pub plot_all_copper_layers: bool,
    /// `m_plotOnAllLayersSequence`: layers to include on all individual layer prints.
    pub plot_on_all_layers_sequence: Lseq,
    /// `m_drillShapeOption`: used by SVG & PDF.
    pub drill_shape_option: DrillMarks,
    /// `m_useDrillOrigin`: used by SVG/DXF/PDF/Gerbers.
    pub use_drill_origin: bool,
    /// `JOB::GetConfiguredOutputPath()` (stored into `PCB_PLOT_PARAMS::m_outputDirectory`).
    pub configured_output_path: String,
    /// `JOB::GetVarOverrides()` (`-D KEY=VALUE`; `LAYER`, `SHEETNAME`, `SHEETPATH` are read
    /// by `JobExportGerbers`).
    pub var_overrides: BTreeMap<String, String>,
}

impl JobExportPcbPlot {
    /// Port of `JOB_EXPORT_PCB_PLOT::JOB_EXPORT_PCB_PLOT( aFormat, aType, aOutputIsDirectory )`
    /// (the member initialisers).
    pub fn new(format: JobPlotFormat) -> Self {
        JobExportPcbPlot {
            plot_format: format,
            color_theme: String::new(),
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
            configured_output_path: String::new(),
            var_overrides: BTreeMap::new(),
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
