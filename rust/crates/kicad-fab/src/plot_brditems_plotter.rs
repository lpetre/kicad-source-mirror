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

//! Port of `pcbnew/plot_brditems_plotter.cpp`: `BRDITEMS_PLOTTER` struct, `getColor`, pads, zones,
//! drill marks.
//!
//! FAB-03 defines the struct (`pcbplot.h` L61-162) with its options and the inline members;
//! every per-item entry point is a pending function that names the work package porting it
//! (FAB-04 pads / zones / colours, FAB-05 graphics, FAB-06 text, FAB-11 drill marks, FAB-13
//! drill symbols). They panic with a "port pending" message until that package lands.

use kicad_core::color4d::Color4D;
use kicad_core::plot_enums::PlotFormat;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::ShapePolySet;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::FootprintId;
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::Plotter;

/// Panic with a "port pending" message naming the C++ function and the work package that
/// ports it (the plotting counterpart of `kicad_pcb::port_pending!`).
macro_rules! fab_port_pending {
    ($cxx:literal $(, $arg:expr)* $(,)?) => {{
        $( let _ = &$arg; )*
        panic!(concat!("kicad-fab: port pending: ", $cxx))
    }};
}

pub(crate) use fab_port_pending;

/// Port of `BRDITEMS_PLOTTER` (`pcbplot.h`): a helper class to plot board items.
///
/// The C++ class *is-a* `PCB_PLOT_PARAMS` (a copy of the options, `opts` here) and holds the
/// plotter, the board and the layers being plotted. One instance is created per call of
/// `PlotStandardLayer`, `PlotPolySet` and `PlotBoardLayers` (drill marks); it has no caches.
/// The C++ member functions are `const` but draw through the plotter pointer; the Rust
/// methods take `&mut self` for the plotter borrow.
pub struct BrditemsPlotter<'a, 'p> {
    /// The `PCB_PLOT_PARAMS` base class.
    pub opts: PcbPlotParams,
    /// `m_plotter`
    pub plotter: &'p mut dyn Plotter,
    /// `m_board` (the board and its hooks: connectivity for pad flashing, rules for margins).
    pub ctx: &'a BoardCtx<'a>,
    /// `m_layerMask`: the layers being plotted (`SetLayerSet`).
    pub layer_mask: Lset,
}

impl<'a, 'p> BrditemsPlotter<'a, 'p> {
    /// `BRDITEMS_PLOTTER( PLOTTER* aPlotter, const BOARD* aBoard, const PCB_PLOT_PARAMS& )`.
    pub fn new(
        a_plotter: &'p mut dyn Plotter,
        a_board: &'a BoardCtx<'a>,
        a_plot_opts: &PcbPlotParams,
    ) -> Self {
        BrditemsPlotter {
            opts: a_plot_opts.clone(),
            plotter: a_plotter,
            ctx: a_board,
            layer_mask: Lset::new(),
        }
    }

    /// Port of `BRDITEMS_PLOTTER::getFineWidthAdj`: the PostScript width adjustment, 0 for
    /// every other format.
    pub fn fine_width_adj(&self) -> i32 {
        if self.opts.format == PlotFormat::Post {
            self.opts.width_adjust
        } else {
            0
        }
    }

    /// Port of `BRDITEMS_PLOTTER::SetLayerSet`.
    pub fn set_layer_set(&mut self, a_layer_mask: &Lset) {
        self.layer_mask = a_layer_mask.clone();
    }

    /// Port of `BRDITEMS_PLOTTER::hideDNPItems` (private in C++).
    pub fn hide_dnp_items(&self, a_layer: PcbLayerId) -> bool {
        self.opts.hide_dnp_fps_on_fab_layers
            && (a_layer == PcbLayerId::FFab || a_layer == PcbLayerId::BFab)
    }

    /// Port of `BRDITEMS_PLOTTER::crossoutDNPItems` (private in C++).
    pub fn crossout_dnp_items(&self, a_layer: PcbLayerId) -> bool {
        self.opts.crossout_dnp_fps_on_fab_layers
            && (a_layer == PcbLayerId::FFab || a_layer == PcbLayerId::BFab)
    }

    /// `BRDITEMS_PLOTTER::PlotFootprintGraphicItems` (FAB-05).
    pub fn plot_footprint_graphic_items(&mut self, a_footprint: FootprintId) {
        fab_port_pending!(
            "BRDITEMS_PLOTTER::PlotFootprintGraphicItems (FAB-05)",
            a_footprint
        )
    }

    /// `BRDITEMS_PLOTTER::PlotFootprintTextItems` (FAB-06).
    pub fn plot_footprint_text_items(&mut self, a_footprint: FootprintId) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotFootprintTextItems (FAB-06)", a_footprint)
    }

    /// `BRDITEMS_PLOTTER::PlotDimension` (FAB-05).
    pub fn plot_dimension(&mut self, a_dim: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotDimension (FAB-05)", a_dim)
    }

    /// `BRDITEMS_PLOTTER::PlotPcbTarget` (FAB-05).
    pub fn plot_pcb_target(&mut self, a_mire: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotPcbTarget (FAB-05)", a_mire)
    }

    /// `BRDITEMS_PLOTTER::PlotZone` (FAB-04). `a_zone` may be a detached copy (`ZONE dummy`).
    pub fn plot_zone(
        &mut self,
        a_zone: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_polys_list: &ShapePolySet,
    ) {
        fab_port_pending!(
            "BRDITEMS_PLOTTER::PlotZone (FAB-04)",
            a_zone,
            a_layer,
            a_polys_list
        )
    }

    /// `BRDITEMS_PLOTTER::PlotText` (FAB-06). `a_text` is the item owning the `EDA_TEXT`.
    pub fn plot_text(
        &mut self,
        a_text: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_is_knockout: bool,
        a_font_metrics: &kicad_font::Metrics,
        a_strikeout: bool,
    ) {
        fab_port_pending!(
            "BRDITEMS_PLOTTER::PlotText (FAB-06)",
            a_text,
            a_layer,
            a_is_knockout,
            a_font_metrics,
            a_strikeout
        )
    }

    /// `BRDITEMS_PLOTTER::PlotShape` (FAB-05).
    pub fn plot_shape(&mut self, a_shape: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotShape (FAB-05)", a_shape)
    }

    /// `BRDITEMS_PLOTTER::PlotTableBorders` (FAB-05).
    pub fn plot_table_borders(&mut self, a_table: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotTableBorders (FAB-05)", a_table)
    }

    /// `BRDITEMS_PLOTTER::PlotBarCode` (FAB-05).
    pub fn plot_bar_code(&mut self, a_bar_code: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotBarCode (FAB-05)", a_bar_code)
    }

    /// `BRDITEMS_PLOTTER::PlotPad` (FAB-04). `a_pad` may be a detached copy (`PAD dummy`).
    pub fn plot_pad(
        &mut self,
        a_pad: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_color: &Color4D,
        a_sketch_mode: bool,
    ) {
        fab_port_pending!(
            "BRDITEMS_PLOTTER::PlotPad (FAB-04)",
            a_pad,
            a_layer,
            a_color,
            a_sketch_mode
        )
    }

    /// `BRDITEMS_PLOTTER::PlotPadNumber` (FAB-04).
    pub fn plot_pad_number(&mut self, a_pad: ItemRef<'_>, a_color: &Color4D) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotPadNumber (FAB-04)", a_pad, a_color)
    }

    /// `BRDITEMS_PLOTTER::PlotBoardGraphicItem` (FAB-05).
    pub fn plot_board_graphic_item(&mut self, item: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotBoardGraphicItem (FAB-05)", item)
    }

    /// `BRDITEMS_PLOTTER::PlotDrillMarks` (FAB-11).
    pub fn plot_drill_marks(&mut self) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotDrillMarks (FAB-11)")
    }

    /// `BRDITEMS_PLOTTER::PlotDrillSymbols` (FAB-13, tier 3).
    pub fn plot_drill_symbols(&mut self, a_layer: PcbLayerId) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotDrillSymbols (FAB-13)", a_layer)
    }

    /// `BRDITEMS_PLOTTER::PlotChartSymbols` (FAB-13, tier 3).
    pub fn plot_chart_symbols(&mut self, a_chart: ItemRef<'_>) {
        fab_port_pending!("BRDITEMS_PLOTTER::PlotChartSymbols (FAB-13)", a_chart)
    }

    /// `BRDITEMS_PLOTTER::getColor` (FAB-04).
    pub fn get_color(&self, a_layer: i32) -> Color4D {
        fab_port_pending!("BRDITEMS_PLOTTER::getColor (FAB-04)", a_layer)
    }
}
