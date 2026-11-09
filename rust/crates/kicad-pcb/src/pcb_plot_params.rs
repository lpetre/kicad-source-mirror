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

//! Port of `pcbnew/pcb_plot_params.h` and `pcbnew/pcb_plot_params.cpp` (`PCB_PLOT_PARAMS`):
//! data, constructor defaults and accessors (package M0d). `Format` / `Parse` belong to the
//! board I/O packages, `IsSameAs` to package M6.

use kicad_core::plot_enums::{DrillMarks, DxfOutlineMode, DxfUnits, PlotFormat, PlotTextMode};
use kicad_core::{Color4D, Lset, PCB_IU_SCALE, PcbLayerId};

/// `GBR_DEFAULT_PRECISION` (`common/jobs/job_export_pcb_drill.h`).
pub const GBR_DEFAULT_PRECISION: i32 = 6;
/// `SVG_PRECISION_MIN` (`common/jobs/job_export_pcb_svg.h`).
pub const SVG_PRECISION_MIN: u32 = 3;
/// `SVG_PRECISION_MAX`.
pub const SVG_PRECISION_MAX: u32 = 6;
/// `SVG_PRECISION_DEFAULT`.
pub const SVG_PRECISION_DEFAULT: u32 = 4;
/// `DEFAULT_PNG_DPI` (`include/plotters/plotter_png.h`).
pub const DEFAULT_PNG_DPI: i32 = 300;

/// `PCB_PLOT_PARAMS` (`pcbnew/pcb_plot_params.h`); the `(pcbplotparams ...)` block of
/// `(setup ...)`. Dropped: `m_colors`, `m_default_colors` (colour themes are supplied by the
/// caller of the plotter).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbPlotParams {
    pub format: PlotFormat,                           // m_format
    pub layer_selection: Lset,                        // m_layerSelection
    pub plot_on_all_layers_sequence: Vec<PcbLayerId>, // m_plotOnAllLayersSequence (LSEQ)
    pub skip_npth_pads: bool,                         // m_skipNPTH_Pads
    pub plot_pad_numbers: bool,                       // m_plotPadNumbers
    pub drill_marks: DrillMarks,                      // m_drillMarks
    pub text_mode: PlotTextMode,                      // m_textMode
    pub dxf_plot_mode: DxfOutlineMode,                // m_DXFPlotMode
    pub dxf_units: DxfUnits,                          // m_DXFUnits
    pub dxf_polygon_mode: bool,                       // m_DXFPolygonMode
    pub a4_output: bool,                              // m_A4Output
    pub auto_scale: bool,                             // m_autoScale
    pub scale: f64,                                   // m_scale
    pub mirror: bool,                                 // m_mirror
    pub negative: bool,                               // m_negative
    pub black_and_white: bool,                        // m_blackAndWhite
    pub plot_drawing_sheet: bool,                     // m_plotDrawingSheet
    pub plot_via_on_mask_layer: Option<bool>,         // m_plotViaOnMaskLayer (legacy read)
    pub subtract_mask_from_silk: bool,                // m_subtractMaskFromSilk
    pub use_gerber_protel_extensions: bool,           // m_useGerberProtelExtensions
    pub use_gerber_x2_format: bool,                   // m_useGerberX2format
    pub gerber_disable_apert_macros: bool,            // m_gerberDisableApertMacros
    pub include_gerber_netlist_info: bool,            // m_includeGerberNetlistInfo
    pub create_gerber_job_file: bool,                 // m_createGerberJobFile
    pub gerber_precision: i32,                        // m_gerberPrecision
    pub svg_precision: u32,                           // m_svgPrecision
    pub svg_fit_page_to_board: bool,                  // m_svgFitPageToBoard
    pub png_dpi: i32,                                 // m_pngDPI
    pub png_antialias: bool,                          // m_pngAntialias
    pub use_aux_origin: bool,                         // m_useAuxOrigin
    pub output_directory: String,                     // m_outputDirectory
    pub scale_selection: i32,                         // m_scaleSelection
    pub plot_reference: bool,                         // m_plotReference
    pub plot_value: bool,                             // m_plotValue
    pub plot_fp_text: bool,                           // m_plotFPText
    pub sketch_pads_on_fab_layers: bool,              // m_sketchPadsOnFabLayers
    pub sketch_pad_line_width: i32,                   // m_sketchPadLineWidth
    pub hide_dnp_fps_on_fab_layers: bool,             // m_hideDNPFPsOnFabLayers
    pub sketch_dnp_fps_on_fab_layers: bool,           // m_sketchDNPFPsOnFabLayers
    pub crossout_dnp_fps_on_fab_layers: bool,         // m_crossoutDNPFPsOnFabLayers
    pub fine_scale_adjust_x: f64,                     // m_fineScaleAdjustX
    pub fine_scale_adjust_y: f64,                     // m_fineScaleAdjustY
    pub width_adjust: i32,                            // m_widthAdjust
    pub dashed_line_dash_ratio: f64,                  // m_dashedLineDashRatio
    pub dashed_line_gap_ratio: f64,                   // m_dashedLineGapRatio
    pub dxf_export_as_multi_layered_file: bool,       // m_DXFExportAsMultiLayeredFile
    pub layers_to_export: Vec<(PcbLayerId, String)>,  // m_layersToExport
    /// m_layer: not initialised by the C++ constructor; `UNDEFINED_LAYER` here.
    pub layer: PcbLayerId,
    // The four PDF switches are read from and written to `(pcbplotparams ...)`
    // (pdf_front_fp_property_popups, pdf_back_fp_property_popups, pdf_metadata,
    // pdf_single_document): required for a byte-exact re-save.
    pub pdf_front_fp_property_popups: bool, // m_PDFFrontFPPropertyPopups
    pub pdf_back_fp_property_popups: bool,  // m_PDFBackFPPropertyPopups
    pub pdf_metadata: bool,                 // m_PDFMetadata
    pub pdf_single: bool,                   // m_PDFSingle
    pub background_color: Color4D,          // m_backgroundColor
}

impl Default for PcbPlotParams {
    /// `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS()` (`pcbnew/pcb_plot_params.cpp:47`).
    fn default() -> Self {
        let mut layer_selection = Lset::from_layers(&[
            PcbLayerId::FSilkS,
            PcbLayerId::BSilkS,
            PcbLayerId::FMask,
            PcbLayerId::BMask,
            PcbLayerId::FPaste,
            PcbLayerId::BPaste,
            PcbLayerId::EdgeCuts,
        ]);
        layer_selection |= Lset::all_cu_mask();

        PcbPlotParams {
            format: PlotFormat::Gerber,
            layer_selection,
            plot_on_all_layers_sequence: Vec::new(),
            // This parameter controls if the NPTH pads will be plotted or not; it is a
            // "local" parameter.
            skip_npth_pads: false,
            plot_pad_numbers: false,
            drill_marks: DrillMarks::SmallDrillShape,
            text_mode: PlotTextMode::Default,
            dxf_plot_mode: DxfOutlineMode::Filled,
            dxf_units: DxfUnits::Inch,
            dxf_polygon_mode: true,
            a4_output: false,
            auto_scale: false,
            scale: 1.0,
            mirror: false,
            negative: false,
            black_and_white: true,
            plot_drawing_sheet: false,
            plot_via_on_mask_layer: None,
            subtract_mask_from_silk: false,
            use_gerber_protel_extensions: false,
            use_gerber_x2_format: true,
            gerber_disable_apert_macros: false,
            include_gerber_netlist_info: true,
            create_gerber_job_file: true,
            gerber_precision: GBR_DEFAULT_PRECISION,
            // we used 0.1mils for SVG step before, but nm precision is more accurate
            svg_precision: SVG_PRECISION_DEFAULT,
            svg_fit_page_to_board: false,
            png_dpi: DEFAULT_PNG_DPI,
            png_antialias: true,
            use_aux_origin: false,
            output_directory: String::new(),
            scale_selection: 1,
            plot_reference: true,
            plot_value: true,
            plot_fp_text: true,
            sketch_pads_on_fab_layers: false,
            // line width to plot items in outline mode
            sketch_pad_line_width: PCB_IU_SCALE.mm_to_iu(0.1),
            hide_dnp_fps_on_fab_layers: false,
            sketch_dnp_fps_on_fab_layers: true,
            crossout_dnp_fps_on_fab_layers: true,
            fine_scale_adjust_x: 1.0,
            fine_scale_adjust_y: 1.0,
            width_adjust: 0,
            dashed_line_dash_ratio: 12.0, // From ISO 128-2
            dashed_line_gap_ratio: 3.0,   // From ISO 128-2
            dxf_export_as_multi_layered_file: false,
            layers_to_export: Vec::new(),
            layer: PcbLayerId::UndefinedLayer,
            pdf_front_fp_property_popups: true,
            pdf_back_fp_property_popups: true,
            pdf_metadata: true,
            pdf_single: false,
            background_color: Color4D::UNSPECIFIED,
        }
    }
}

impl PcbPlotParams {
    /// `PCB_PLOT_PARAMS::SetGerberPrecision`: only 5 or 6 (the default) are accepted.
    pub fn set_gerber_precision(&mut self, precision: i32) {
        self.gerber_precision = if precision == GBR_DEFAULT_PRECISION - 1 {
            GBR_DEFAULT_PRECISION - 1
        } else {
            GBR_DEFAULT_PRECISION
        };
    }

    /// `PCB_PLOT_PARAMS::SetSvgPrecision`: clamped to `SVG_PRECISION_MIN ..= MAX`.
    pub fn set_svg_precision(&mut self, precision: u32) {
        self.svg_precision = precision.clamp(SVG_PRECISION_MIN, SVG_PRECISION_MAX);
    }

    /// `PCB_PLOT_PARAMS::IsSameAs`.
    pub fn is_same_as(&self, other: &PcbPlotParams) -> bool {
        crate::port_pending!("PCB_PLOT_PARAMS::IsSameAs", other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS()` (`pcbnew/pcb_plot_params.cpp:47`).
    #[test]
    fn constructor_defaults() {
        let mut p = PcbPlotParams::default();
        assert_eq!(p.format, PlotFormat::Gerber);
        assert!(p.layer_selection.contains(PcbLayerId::EdgeCuts));
        assert!(p.layer_selection.contains(PcbLayerId::In30Cu));
        assert!(!p.layer_selection.contains(PcbLayerId::FFab));
        assert_eq!(p.gerber_precision, 6);
        assert_eq!(p.svg_precision, 4);
        assert_eq!(p.png_dpi, 300);
        assert_eq!(p.sketch_pad_line_width, 100_000);
        assert!(p.black_and_white && p.use_gerber_x2_format && p.include_gerber_netlist_info);
        assert!(p.sketch_dnp_fps_on_fab_layers && p.crossout_dnp_fps_on_fab_layers);
        assert_eq!(p.drill_marks, DrillMarks::SmallDrillShape);
        assert_eq!(p.dashed_line_dash_ratio, 12.0);
        assert_eq!(p.scale_selection, 1);
        assert!(p.pdf_metadata && !p.pdf_single);
        p.set_gerber_precision(4);
        assert_eq!(p.gerber_precision, 6);
        p.set_gerber_precision(5);
        assert_eq!(p.gerber_precision, 5);
        p.set_svg_precision(9);
        assert_eq!(p.svg_precision, 6);
    }
}
