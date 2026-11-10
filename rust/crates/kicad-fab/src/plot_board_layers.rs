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

//! Port of `pcbnew/plot_board_layers.cpp` L128-171, L247-931, L1278-1595 (`PlotBoardLayers`,
//! `PlotOneBoardLayer`, `PlotStandardLayer`, `initializePlotter`, `StartPlotBoard`).
//!
//! FAB-03 ports the driver: `StartPlotBoard` (plotter creation for Gerber and SVG, viewport,
//! Gerber X2 header), `initializePlotter`, `FillNegativeKnockout`, `PlotBoardLayers`,
//! `PlotOneBoardLayer` (the dispatch per layer kind) and the iteration skeleton of
//! `PlotStandardLayer` (container order, blocks, Gerber metadata set-up). The per-item bodies
//! of `PlotStandardLayer` (pads, DNP cross, vias, tracks, zones) are FAB-04 and are pending
//! functions here; the solder mask minimum width path and the clipped silk are FAB-10.
//!
//! Out of scope (analysis 2.2): DXF / PostScript / PDF / PNG / HPGL plotters and their
//! branches (`PlotLayer`'s DXF outline mode, `setupPlotterNewPDFPage`, `plotBackgroundColor`,
//! `PlotInteractiveLayer`, `ClearPlottedFootprints`), the drawing sheet (PLOT-08).

use std::sync::Arc;

use kicad_core::color4d::Color4D;
use kicad_core::page_info::{CustomPageSize, PageInfo, PageSizeType};
use kicad_core::plot_enums::{DrillMarks, FillT, PlotFormat};
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::transform_round_chamfered_rect_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, ShapePolySet, Vector2I, ki_round};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::{FootprintId, TrackId, ZoneId};
use kicad_pcb::AnyItem;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::pcb_track::Track;
use kicad_plot::gbr_metadata::GbrMetadata;
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;
use kicad_plot::gbr_metadata::GbrApertureAttrib;
use kicad_plot::{GerberPlotter, Plotter, RenderSettings, SvgPlotter};

use crate::pcb_plotter::build_plot_file_name;
use crate::pcbplot::{PlotEnv, add_gerber_x2_attribute, gerber_protel_extension};
use crate::plot_brditems_plotter::{BrditemsPlotter, fab_port_pending};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile};

/// Port of `PlotBoardLayers`: plot a sequence of board layers into one plotter (the first
/// layer is the "primary" layer of the file).
pub fn plot_board_layers(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layers: &[PcbLayerId],
    a_plot_options: &PcbPlotParams,
) {
    if a_layers.is_empty() {
        return;
    }

    // aPlotter->ClearPlottedFootprints(): PDF bookmarks only (out of scope).

    let mut plot_options = a_plot_options.clone();

    // Gerber carries no colour, so a drill mark lands as ink instead of knocking out its pad
    // The plot dialog forces them off for gerber but API and CLI callers arrive here directly
    if a_plotter.plotter_type() == PlotFormat::Gerber {
        plot_options.drill_marks = DrillMarks::NoDrillShape;
    }

    for &layer in a_layers {
        plot_one_board_layer(
            a_board,
            a_plotter,
            layer,
            &plot_options,
            layer == a_layers[0],
        );
    }

    // Drill symbols go after the normal layers but before the physical marks, so a symbol is
    // never sitting under a knockout
    let map_layers = drill_symbol_layers(a_board) & Lset::from_layers(a_layers);

    if map_layers.any() {
        let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, &plot_options);
        itemplotter.set_layer_set(&Lset::from_layers(a_layers));

        for layer in map_layers.seq().0 {
            itemplotter.plot_drill_symbols(layer);
        }
    }

    // Drill marks are plotted in white to knockout the pad if any layers of the pad are
    // being plotted, and in black if the pad is not being plotted. For the former, this
    // must happen after all other layers are plotted.

    // One global knockout pass, so a plotted layer carrying a map skips them rather than
    // punching through the symbols they would annotate
    if plot_options.drill_marks != DrillMarks::NoDrillShape && !map_layers.any() {
        let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, &plot_options);
        itemplotter.set_layer_set(&Lset::from_layers(a_layers));
        itemplotter.plot_drill_marks();
    }
}

/// `BOARD::DrillSymbolLayers()`: the layers of the board's drill map items. C++ keeps
/// `m_drillSymbolLayers`, refreshed (`RefreshDrillSymbolLayers`) after loading and whenever a
/// drill map changes, so it always equals the layers of the `PCB_DRILL_MAP` items in
/// `m_drawings`; the board model has no such field, so the port computes it on demand.
fn drill_symbol_layers(a_board: &BoardCtx<'_>) -> Lset {
    let mut layers = Lset::new();

    for (id, item) in a_board.board.drawings() {
        if !matches!(item, AnyItem::DrillMap(_)) {
            continue;
        }

        if let Some(item) = a_board.item(id) {
            layers.set(item.layer());
        }
    }

    layers
}

/// Port of `PlotOneBoardLayer`: plot one layer with the function its kind needs.
pub fn plot_one_board_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer: PcbLayerId,
    a_plot_opt: &PcbPlotParams,
    is_primary_layer: bool,
) {
    let mut plot_opt = a_plot_opt.clone();

    // Set a default color and the text mode for this layer
    a_plotter.set_color(&Color4D::BLACK);
    a_plotter.set_text_mode(a_plot_opt.text_mode);

    // Specify that the contents of the "Edges Pcb" layer are to be plotted in addition to the
    // contents of the currently specified layer.
    let mut layer_mask = Lset::from_layers(&[a_layer]);

    if is_copper_layer(a_layer) {
        // Skip NPTH pads on copper layers ( only if hole size == pad size ):
        // Drill mark will be plotted if drill mark is SMALL_DRILL_SHAPE or FULL_DRILL_SHAPE
        if plot_opt.format == PlotFormat::Dxf {
            plot_opt.dxf_polygon_mode = true;
        } else {
            plot_opt.skip_npth_pads = true;
        }

        plot_layer(a_board, a_plotter, &layer_mask, &plot_opt);
    } else {
        match a_layer {
            PcbLayerId::BMask | PcbLayerId::FMask => {
                // Use outline mode for DXF
                plot_opt.dxf_polygon_mode = true;

                // Plot solder mask:
                plot_solder_mask_layer(a_board, a_plotter, &layer_mask, &plot_opt);
            }

            PcbLayerId::BAdhes | PcbLayerId::FAdhes | PcbLayerId::BPaste | PcbLayerId::FPaste => {
                // Disable plot pad holes
                plot_opt.drill_marks = DrillMarks::NoDrillShape;

                // Use outline mode for DXF
                plot_opt.dxf_polygon_mode = true;

                plot_layer(a_board, a_plotter, &layer_mask, &plot_opt);
            }

            PcbLayerId::FSilkS | PcbLayerId::BSilkS => {
                if plot_opt.subtract_mask_from_silk {
                    if a_plotter.plotter_type() == PlotFormat::Gerber && is_primary_layer {
                        // Use old-school, positive/negative mask plotting which preserves
                        // utilization of Gerber aperture masks.  This method can only be used
                        // when the given silk layer is the primary layer as the negative mask
                        // will also knockout any other (non-silk) layers that were plotted
                        // before the silk layer.

                        plot_standard_layer(a_board, a_plotter, &layer_mask, &plot_opt);

                        // Create the mask to subtract by creating a negative layer polarity
                        a_plotter.set_layer_polarity(false);

                        // Disable plot pad holes
                        plot_opt.drill_marks = DrillMarks::NoDrillShape;

                        // Plot the mask
                        layer_mask = if a_layer == PcbLayerId::FSilkS {
                            Lset::from_layers(&[PcbLayerId::FMask])
                        } else {
                            Lset::from_layers(&[PcbLayerId::BMask])
                        };
                        plot_solder_mask_layer(a_board, a_plotter, &layer_mask, &plot_opt);

                        // Disable the negative polarity
                        a_plotter.set_layer_polarity(true);
                    } else {
                        plot_clipped_silk_layer(a_board, a_plotter, &layer_mask, &plot_opt);
                    }
                } else {
                    plot_layer(a_board, a_plotter, &layer_mask, &plot_opt);
                }
            }

            // Dwgs_User, Cmts_User, Eco1_User, Eco2_User, Edge_Cuts, Margin, F/B_CrtYd,
            // F/B_Fab and every other layer
            _ => {
                plot_layer(a_board, a_plotter, &layer_mask, &plot_opt);
            }
        }
    }
}

/// Port of `PlotLayer` (`plot_board_layers.cpp` L53-61). `PlotLayerOutlines()` is designed
/// only for DXF plotters (out of scope), so this is always `PlotStandardLayer`.
fn plot_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    layer_mask: &Lset,
    plot_opts: &PcbPlotParams,
) {
    if plot_opts.format == PlotFormat::Dxf && plot_opts.dxf_polygon_mode {
        fab_port_pending!("PlotLayerOutlines (DXF, out of scope)")
    } else {
        plot_standard_layer(a_board, a_plotter, layer_mask, plot_opts);
    }
}

/// Port of `PlotSolderMaskLayer` (L91-107): solder mask layers have a minimum thickness value
/// and cannot be drawn like standard layers, unless the minimum thickness is 0. The polygon
/// path for a non-zero minimum width is FAB-10 (`plot_layer_poly.rs`).
fn plot_solder_mask_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
) {
    if a_board.board.design_settings().solder_mask_min_width == 0 {
        plot_layer(a_board, a_plotter, a_layer_mask, a_plot_opt);
        return;
    }

    fab_port_pending!(
        "PlotSolderMaskLayer: GenerateLayerPoly / PlotPolySet path (FAB-10)",
        a_layer_mask
    )
}

/// `PlotClippedSilkLayer` (L110-125, FAB-10).
fn plot_clipped_silk_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
) {
    fab_port_pending!(
        "PlotClippedSilkLayer (FAB-10)",
        a_board.board,
        a_plotter,
        a_layer_mask,
        a_plot_opt
    )
}

/// The layer flags `PlotStandardLayer` computes once (L361-371) and its per-item bodies read.
#[derive(Clone, Debug)]
pub struct StandardLayerFlags {
    /// `maxError` (`m_MaxError` of the design settings).
    pub max_error: i32,
    /// `onCopperLayer`
    pub on_copper_layer: bool,
    /// `onSolderMaskLayer`
    pub on_solder_mask_layer: bool,
    /// `onSolderPasteLayer`
    pub on_solder_paste_layer: bool,
    /// `onFrontFab`
    pub on_front_fab: bool,
    /// `onBackFab`
    pub on_back_fab: bool,
    /// `sketchPads`
    pub sketch_pads: bool,
    /// `variantName` (`BOARD::GetCurrentVariant()`).
    pub variant_name: String,
}

/// Port of `PlotStandardLayer`: plot any layer EXCEPT a solder-mask with an enforced minimum
/// width.
///
/// The iteration order of this function defines the object order in the file and, through the
/// aperture list, all D-codes; it is ported literally. The per-item bodies are FAB-04
/// (`plot_standard_layer_footprint`, `plot_standard_layer_via`, `plot_standard_layer_track`,
/// `plot_standard_layer_zone`), FAB-05 (`PlotBoardGraphicItem`, `PlotFootprintGraphicItems`)
/// and FAB-06 (`PlotFootprintTextItems`).
pub fn plot_standard_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
) {
    let board = a_board.board;
    let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, a_plot_opt);
    let max_error = board.design_settings().max_error;

    itemplotter.set_layer_set(a_layer_mask);

    let on_copper_layer = (Lset::all_cu_mask() & a_layer_mask).any();
    let on_solder_mask_layer =
        (Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]) & a_layer_mask).any();
    let on_solder_paste_layer =
        (Lset::from_layers(&[PcbLayerId::FPaste, PcbLayerId::BPaste]) & a_layer_mask).any();
    let on_front_fab = (Lset::from_layers(&[PcbLayerId::FFab]) & a_layer_mask).any();
    let on_back_fab = (Lset::from_layers(&[PcbLayerId::BFab]) & a_layer_mask).any();
    let sketch_pads = (on_front_fab || on_back_fab) && a_plot_opt.sketch_pads_on_fab_layers;
    let variant_name = board.get_current_variant().to_string();

    let flags = StandardLayerFlags {
        max_error,
        on_copper_layer,
        on_solder_mask_layer,
        on_solder_paste_layer,
        on_front_fab,
        on_back_fab,
        sketch_pads,
        variant_name,
    };

    // Plot edge layer and graphic items
    for (id, _) in board.drawings() {
        if let Some(item) = a_board.item(id) {
            itemplotter.plot_board_graphic_item(item);
        }
    }

    // Draw footprint texts:
    for (footprint, _) in board.footprints() {
        itemplotter.plot_footprint_text_items(footprint);
    }

    // Draw footprint other graphic items:
    for (footprint, _) in board.footprints() {
        itemplotter.plot_footprint_graphic_items(footprint);
    }

    // Plot footprint pads
    for (footprint, _) in board.footprints() {
        itemplotter.plotter.start_block(None);

        plot_standard_layer_footprint(&mut itemplotter, footprint, &flags);

        itemplotter.plotter.end_block(None);
    }

    // Plot vias on copper layers, and if aPlotOpt.GetPlotViaOnMaskLayer() is true,

    let mut gbr_metadata = GbrMetadata::new();

    if on_copper_layer {
        gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Viapad);
        gbr_metadata.set_net_attrib_type(gbr_netinfo::NET);
    }

    itemplotter.plotter.start_block(None);

    for (track, item) in board.tracks() {
        if !matches!(item, Track::Via(_)) {
            continue;
        }

        plot_standard_layer_via(&mut itemplotter, &mut gbr_metadata, track, &flags);
    }

    itemplotter.plotter.end_block(None);
    itemplotter.plotter.start_block(None);

    if on_copper_layer {
        gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Conductor);
        gbr_metadata.set_net_attrib_type(gbr_netinfo::NET);
    } else {
        // Reset attributes if non-copper (soldermask) layer
        gbr_metadata.set_aperture_attrib(GbrApertureAttrib::None);
        gbr_metadata.set_net_attrib_type(gbr_netinfo::UNSPECIFIED);
    }

    // Plot tracks (not vias) :
    for (track, item) in board.tracks() {
        if matches!(item, Track::Via(_)) {
            continue;
        }

        plot_standard_layer_track(&mut itemplotter, &mut gbr_metadata, track, &flags);
    }

    itemplotter.plotter.end_block(None);

    // Plot filled ares
    itemplotter.plotter.start_block(None);

    for (zone, _) in board.zones() {
        plot_standard_layer_zone(&mut itemplotter, zone, &flags);
    }

    itemplotter.plotter.end_block(None);
}

/// `PlotStandardLayer` L385-740: the pads of one footprint (sketch pads, pad numbers, the
/// margin / shape adjustments of `plotPadLayer` per layer of `SeqStackupForPlotting()`) and
/// the DNP cross, between the `StartBlock` / `EndBlock` of the caller (FAB-04).
pub fn plot_standard_layer_footprint(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    footprint: FootprintId,
    flags: &StandardLayerFlags,
) {
    fab_port_pending!(
        "PlotStandardLayer: footprint pads and DNP cross (FAB-04)",
        itemplotter.layer_mask,
        footprint,
        flags
    )
}

/// `PlotStandardLayer` L768-831: the body of the via loop for one via (FAB-04).
pub fn plot_standard_layer_via(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    gbr_metadata: &mut GbrMetadata,
    via: TrackId,
    flags: &StandardLayerFlags,
) {
    fab_port_pending!(
        "PlotStandardLayer: via (FAB-04)",
        itemplotter.layer_mask,
        gbr_metadata,
        via,
        flags
    )
}

/// `PlotStandardLayer` L848-889: the body of the track loop for one segment or arc (FAB-04).
pub fn plot_standard_layer_track(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    gbr_metadata: &mut GbrMetadata,
    track: TrackId,
    flags: &StandardLayerFlags,
) {
    fab_port_pending!(
        "PlotStandardLayer: track (FAB-04)",
        itemplotter.layer_mask,
        gbr_metadata,
        track,
        flags
    )
}

/// `PlotStandardLayer` L898-928: the body of the zone loop for one zone (rule areas skipped,
/// fills per layer of the mask, islands with a dummy no-net copy) (FAB-04).
pub fn plot_standard_layer_zone(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    zone: ZoneId,
    flags: &StandardLayerFlags,
) {
    fab_port_pending!(
        "PlotStandardLayer: zone (FAB-04)",
        itemplotter.layer_mask,
        zone,
        flags
    )
}

/// `TransformRoundChamferedRectToPolygon( buffer, pos, size, rotation, radius, chamferRatio,
/// chamferCorners, 0, maxError, ERROR_INSIDE )` in the shape the plot engines take
/// (`kicad_plot::plotter_gerber::RoundChamferedRectToPolygonFn`): installed into the SVG
/// plotter for `FlashPadRoundRect`, and the argument FAB-04 passes to
/// `GerberPlotter::flash_pad_chamfer_round_rect`.
pub fn round_chamfered_rect_to_polygon(
    a_position: Vector2I,
    a_size: Vector2I,
    a_rotation: EdaAngle,
    a_corner_radius: i32,
    a_chamfer_ratio: f64,
    a_chamfer_corners: i32,
    a_error: i32,
) -> ShapePolySet {
    let mut buffer = ShapePolySet::new();
    transform_round_chamfered_rect_to_polygon(
        &mut buffer,
        a_position,
        a_size,
        a_rotation,
        a_corner_radius,
        a_chamfer_ratio,
        a_chamfer_corners,
        0,
        a_error,
        ErrorLoc::Inside,
    );
    buffer
}

/// Port of `initializePlotter` (`static`): set up most plot options for plotting a board
/// (especially the viewport).
///
/// Important thing: page size is the 'drawing' page size, paper size is the physical page
/// size.
fn initialize_plotter(
    a_plotter: &mut dyn Plotter,
    a_board: &BoardCtx<'_>,
    a_plot_opts: &PcbPlotParams,
) {
    let board = a_board.board;
    let page_a4 = PageInfo::new(PageSizeType::A4, false, &CustomPageSize::default());
    let page_info = &board.paper;
    let sheet_info: &PageInfo;
    let paperscale: f64; // Page-to-paper ratio
    let paper_size_iu: Vector2I;
    let page_size_iu = size_iu_to_vector2i(page_info.size_iu(PCB_IU_SCALE.iu_per_mils));
    let autocenter: bool;

    // Special options: to fit the sheet to an A4 sheet replace the paper size. However there
    // is a difference between the autoscale and the a4paper option:
    //  - Autoscale fits the board to the paper size
    //  - A4paper fits the original paper size to an A4 sheet
    //  - Both of them fit the board to an A4 sheet
    if a_plot_opts.a4_output {
        sheet_info = &page_a4;
        paper_size_iu = size_iu_to_vector2i(page_a4.size_iu(PCB_IU_SCALE.iu_per_mils));
        paperscale = paper_size_iu.x as f64 / page_size_iu.x as f64;
        autocenter = true;
    } else {
        sheet_info = page_info;
        paper_size_iu = page_size_iu;
        paperscale = 1.0;

        // Need autocentering only if scale is not 1:1
        autocenter = (a_plot_opts.scale != 1.0) || a_plot_opts.auto_scale;
    }

    let bbox: Box2I = board.compute_bounding_box(a_board, false, false);
    let board_center: Vector2I = bbox.centre();
    let size = bbox.size();
    // VECTOR2I boardSize = bbox.GetSize(): the int64 size clamped to int
    let board_size = Vector2I::new(clamp_i64_to_i32(size.x), clamp_i64_to_i32(size.y));

    // Fit to 80% of the page if asked; it could be that the board is empty, in this case
    // regress to 1:1 scale
    let compound_scale: f64 = if a_plot_opts.auto_scale && board_size.x > 0 && board_size.y > 0 {
        let xscale = (paper_size_iu.x as f64 * 0.8) / board_size.x as f64;
        let yscale = (paper_size_iu.y as f64 * 0.8) / board_size.y as f64;

        xscale.min(yscale) * paperscale
    } else {
        a_plot_opts.scale * paperscale
    };

    // For the plot offset we have to keep in mind the auxiliary origin too: if autoscaling is
    // off we check that plot option (i.e. autoscaling overrides auxiliary origin)
    let mut offset = Vector2I::new(0, 0);

    if autocenter {
        offset.x = ki_round(board_center.x as f64 - (paper_size_iu.x as f64 / 2.0) / compound_scale);
        offset.y = ki_round(board_center.y as f64 - (paper_size_iu.y as f64 / 2.0) / compound_scale);
    } else if a_plot_opts.use_aux_origin {
        offset = board.design_settings().aux_origin();
    }

    a_plotter.set_page_settings(sheet_info);

    a_plotter.set_viewport(
        offset,
        PCB_IU_SCALE.iu_per_mils / 10.0,
        compound_scale,
        a_plot_opts.mirror,
    );

    // For SVG fit-to-board plots the page is the board bounding box and the origin is at
    // (0,0), so the SVG viewBox must be that bounding box (it can extend to negative
    // coordinates relative to the origin, and the origin doesn't need to be on the page).
    if a_plot_opts.format == PlotFormat::Svg && a_plot_opts.svg_fit_page_to_board {
        a_plotter.set_plot_bbox(&bbox);
    }

    // Has meaning only for gerber plotter. Must be called only after SetViewport
    a_plotter.set_gerber_coordinates_format(a_plot_opts.gerber_precision(), false);

    // Has meaning only for SVG plotter. Must be called only after SetViewport
    a_plotter.set_svg_coordinates_format(a_plot_opts.svg_precision());

    a_plotter.set_creator("PCBNEW");
    a_plotter.set_color_mode(!a_plot_opts.black_and_white); // default is plot in Black and White.
    a_plotter.set_text_mode(a_plot_opts.text_mode);
}

/// `VECTOR2I( PAGE_INFO::GetSizeIU( ... ) )`: the `VECTOR2D` to `VECTOR2I` conversion clamps
/// to the `int` range and truncates.
fn size_iu_to_vector2i(size: (f64, f64)) -> Vector2I {
    let conv = |v: f64| v.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    Vector2I::new(conv(size.0), conv(size.1))
}

/// The `int64_t` to `int` conversion of `VECTOR2<int>( VECTOR2<int64_t> )` (clamping).
fn clamp_i64_to_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Port of `FillNegativeKnockout` (`static`): prefill in black an area a little bigger than
/// the board to prepare for the negative plot.
fn fill_negative_knockout(a_plotter: &mut dyn Plotter, a_bbbox: &Box2I) {
    let margin = (5.0 * PCB_IU_SCALE.iu_per_mm) as i32; // Add a 5 mm margin around the board
    a_plotter.set_negative(true);
    a_plotter.set_color(&Color4D::WHITE); // Which will be plotted as black

    let mut area = *a_bbbox;
    area.inflate(margin);
    a_plotter.rect(area.origin(), area.end(), FillT::FilledShape, 0, 0);
    a_plotter.set_color(&Color4D::BLACK);
}

/// Port of `StartPlotBoard`: open a new plot file using the options (and especially the
/// format) specified in the options and prepare the page for plotting.
///
/// Returns the plotter, ready for `PlotBoardLayers`; the caller ends the plot
/// (`end_plot`) and takes the file text from the plotter output. The C++ returns `nullptr`
/// when the file cannot be created; the port has no file system, so the errors are the
/// formats outside the port (DXF, PostScript, PDF, PNG; HPGL is not supported by C++ either)
/// and the drawing sheet (PLOT-08).
///
/// `a_layer_name`, `a_sheet_name`, `a_sheet_path`, `a_page_number` and `a_page_count` are
/// only used by the drawing sheet in C++ (`RENDER_SETTINGS::SetLayerName`,
/// `PlotDrawingSheet`); `a_env` replaces `GetBuildVersion()` and the clock.
#[allow(clippy::too_many_arguments)]
pub fn start_plot_board(
    a_board: &BoardCtx<'_>,
    a_plot_opts: &PcbPlotParams,
    a_layer: i32,
    a_layer_name: &str,
    a_full_file_name: &str,
    a_sheet_name: &str,
    a_sheet_path: &str,
    a_page_name: &str,
    a_env: &PlotEnv,
) -> Result<Box<dyn Plotter>, FabError> {
    let board = a_board.board;
    let _ = (a_layer_name, a_sheet_name, a_sheet_path);

    // Create the plotter driver and set the few plotter specific options
    let mut plotter: Box<dyn Plotter> = match a_plot_opts.format {
        PlotFormat::Dxf | PlotFormat::Post | PlotFormat::Pdf | PlotFormat::Png => {
            return Err(FabError::Unsupported(format!(
                "plot format {:?} is not part of the port (Gerber and SVG only)",
                a_plot_opts.format
            )));
        }

        PlotFormat::Hpgl => {
            return Err(FabError::Unsupported(
                "HPGL plotting is no longer supported as of KiCad 10.0".to_string(),
            ));
        }

        PlotFormat::Gerber => {
            // For Gerber plotter, a valid board layer must be set, in order to create a valid
            // Gerber header, especially the TF.FileFunction and .FilePolarity data.
            // C++: wxLogError( "Invalid board layer %d, cannot build a valid Gerber file
            // header" ) for a layer outside [PCBNEW_LAYER_ID_START, PCB_LAYER_ID_COUNT) and
            // continues; the port has no log (the header is written all the same).
            let mut gbr = GerberPlotter::new();
            gbr.set_build_version(&a_env.build_version);
            gbr.set_creation_date(&a_env.gerber_comment_date());
            Box::new(gbr)
        }

        PlotFormat::Svg => {
            let mut svg = SvgPlotter::new();
            svg.set_creation_date(&a_env.iso8601_date());
            svg.set_round_rect_polygon_fn(Some(Arc::new(round_chamfered_rect_to_polygon)));
            Box::new(svg)
        }

        #[allow(unreachable_patterns)]
        _ => {
            return Err(FabError::Unsupported(format!(
                "unknown plot format {:?}",
                a_plot_opts.format
            )));
        }
    };

    // KIGFX::PCB_RENDER_SETTINGS: LoadColors( ColorSettings() ) and SetLayerName( aLayerName )
    // only matter for the colour plotters and the drawing sheet (colour themes: FAB-11).
    let mut render_settings = RenderSettings::default();
    render_settings.set_default_pen_width(PCB_IU_SCALE.mm_to_iu(0.0212)); // Hairline at 1200dpi
    render_settings.set_dash_length_ratio(a_plot_opts.dashed_line_dash_ratio);
    render_settings.set_gap_length_ratio(a_plot_opts.dashed_line_gap_ratio);

    plotter.set_render_settings(Some(render_settings));

    // Compute the viewport and set the other options

    // page layout is not mirrored, so temporarily change mirror option for the page layout
    let mut plot_opts = a_plot_opts.clone();

    if plot_opts.plot_drawing_sheet {
        if plot_opts.mirror {
            plot_opts.mirror = false;
        }
        if plot_opts.scale != 1.0 {
            plot_opts.scale = 1.0;
        }
        if plot_opts.auto_scale {
            plot_opts.auto_scale = false;
        }
    }

    initialize_plotter(plotter.as_mut(), a_board, &plot_opts);

    if plotter.open_file(a_full_file_name) {
        plotter.clear_header_lines_list();

        // For the Gerber "file function" attribute, set the layer number
        if plotter.plotter_type() == PlotFormat::Gerber {
            let use_x2mode = plot_opts.use_gerber_x2_format;

            if let Some(gbrplotter) = plotter.as_gerber_mut() {
                gbrplotter.disable_apert_macros(plot_opts.gerber_disable_apert_macros);
                gbrplotter.use_x2format(use_x2mode);
                gbrplotter.use_x2_net_attributes(plot_opts.include_gerber_netlist_info);
            }

            // Attributes can be added using X2 format or as comment (X1 format)
            add_gerber_x2_attribute(plotter.as_mut(), board, a_layer, a_env, !use_x2mode);
        }

        // The C++ catches exceptions of StartPlot (none of the ported engines throws).
        let start_plot_success = plotter.start_plot(a_page_name);

        if start_plot_success {
            // PDF / PNG: plotBackgroundColor (out of scope).

            // Plot the frame reference if requested
            if a_plot_opts.plot_drawing_sheet {
                return Err(FabError::NotPorted(
                    "PlotDrawingSheet (drawing sheet / title block, PLOT-07 / PLOT-08)",
                ));
            }

            // When plotting a negative board: draw a black rectangle (background for plot board
            // in white) and switch the current color to WHITE; note the color inversion is
            // actually done in the driver (if supported)
            if a_plot_opts.negative {
                let bbox = board.compute_bounding_box(a_board, false, false);
                fill_negative_knockout(plotter.as_mut(), &bbox);
            }

            return Ok(plotter);
        }
    }

    Err(FabError::Unsupported(format!(
        "cannot start the plot of '{a_full_file_name}'"
    )))
}

/// One Gerber layer file of `kicad-cli pcb export gerbers`, end to end: the per-layer body of
/// `PCBNEW_JOBS_HANDLER::JobExportGerbers` (`pcbnew_jobs_handler.cpp` L1852-1905) for
/// already prepared plot options: file name (`GetGerberProtelExtension` or `gbr`,
/// `BuildPlotFileName` with the board's layer name), `StartPlotBoard`, `PlotBoardLayers`
/// over `a_plot_sequence` (the layer, then the common layers: `getPlotSequence`), `EndPlot`.
///
/// The `LAYER` / `SHEETNAME` / `SHEETPATH` variable overrides of the job only reach the
/// drawing sheet and are not taken here. The job-file registration and the reporter messages
/// are FAB-07 / FAB-09.
pub fn plot_gerber_layer_file(
    a_board: &BoardCtx<'_>,
    a_plot_opts: &PcbPlotParams,
    a_layer: PcbLayerId,
    a_plot_sequence: &[PcbLayerId],
    a_out_path: &str,
    a_env: &PlotEnv,
) -> Result<OutputFile, FabError> {
    let board = a_board.board;

    // Pick the basename from the board file
    let mut fn_ = WxFileName::new(&board.file_name);
    let layer_name = board.layer_name(a_layer);

    let file_ext = if a_plot_opts.use_gerber_protel_extensions {
        gerber_protel_extension(a_layer.0)
    } else {
        GerberPlotter::default_file_extension().to_string()
    };

    build_plot_file_name(&mut fn_, a_out_path, &layer_name, &file_ext);

    let full_path = fn_.full_path();
    let mut plotter = start_plot_board(
        a_board,
        a_plot_opts,
        a_layer.0,
        &layer_name,
        &full_path,
        "",
        "",
        "1",
        a_env,
    )?;

    plot_board_layers(a_board, plotter.as_mut(), a_plot_sequence, a_plot_opts);
    plotter.end_plot();

    Ok(OutputFile {
        name: full_path,
        contents: plotter.output().as_bytes().to_vec(),
    })
}
