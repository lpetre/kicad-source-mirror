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
//! of `PlotStandardLayer` (pads, DNP cross, vias, tracks, zones, L385-928) are FAB-04; the
//! solder mask minimum width path and the clipped silk (FAB-10) are in `plot_layer_poly.rs`.
//!
//! Out of scope (analysis 2.2): DXF / PostScript / PDF / PNG / HPGL plotters and their
//! branches (`PlotLayer`'s DXF outline mode, `setupPlotterNewPDFPage`, `plotBackgroundColor`,
//! `PlotInteractiveLayer`, `ClearPlottedFootprints`), the drawing sheet (PLOT-08).

use std::cell::RefCell;
use std::collections::BTreeMap;

use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::layer_ids::{
    Flashing, LAYER_DNP_MARKER, LAYER_DRAWINGSHEET, LAYER_VIAS, is_copper_layer,
};
use kicad_core::page_info::{CustomPageSize, PageInfo, PageSizeType};
use kicad_core::plot_enums::{DrillMarks, FillT, PlotFormat};
use kicad_core::text_vars::TextVarResolver;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::trigo::rotate_point_around;
use kicad_geom::{ANGLE_0, Box2I, Shape, ShapePolySet, Vector2D, Vector2I, ki_round};
use kicad_pcb::AnyItem;
use kicad_pcb::board_design_settings::LAYER_CLASS_FAB;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::ids::{FootprintId, NetCode, PadId, TrackId, ZoneId};
use kicad_pcb::item::DetachedCopy;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadAttrib, PadShape};
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::pcb_track::Track;
use kicad_plot::common_plot_functions::{PlotDrawingSheetArgs, plot_drawing_sheet};
use kicad_plot::gbr_metadata::GbrApertureAttrib;
use kicad_plot::gbr_metadata::GbrMetadata;
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;
use kicad_plot::plotter::PlotData;
use kicad_plot::{GerberPlotter, Plotter, RenderSettings, SvgPlotter};

use crate::color_settings::ColorSettings;
use crate::pcb_plotter::build_plot_file_name;
use crate::pcbplot::{PlotEnv, add_gerber_x2_attribute, gerber_protel_extension};
use crate::plot_brditems_plotter::{BrditemsPlotter, fab_port_pending};
use crate::plot_layer_poly::{plot_clipped_silk_layer, plot_solder_mask_layer_min_width};
use crate::wx_file_name::WxFileName;
use crate::{FabError, OutputFile};

/// The board's pads as the `plotPadLayer` lambda of `PlotStandardLayer` (L446-681) leaves
/// them for the layers plotted after it.
///
/// The C++ plots the board's own pad: it sets the plotted size (and shape / radius), plots and
/// restores with `SetSize( aLayer, padSize )`, `SetDelta`, `SetShape` and
/// `SetRoundRectRadiusRatio`. `PAD::SetSize` is a board-frame setter that stores
/// `derivePadLibSize( size )`, so for a pad of a footprint with a scale other than (1, 1) the
/// restore is not exact (the library size becomes `KiROUND( KiROUND( L * s ) / s )`), and every
/// layer plotted later from the same loaded board (the other layers of one file, and the other
/// files of one job, `pcbnew_jobs_handler.cpp` L1835-1912) starts from that pad. The port keeps
/// the board immutable (`board-model.md` 13): this overlay holds the restored pad of every pad
/// plotted so far, keyed by its id, and `PlotStandardLayer` reads a pad from here before it
/// falls back to the board. One instance belongs to one plot job (one loaded board), shared by
/// all its files in plot order.
#[derive(Debug, Default)]
pub struct PadPlotState {
    pads: RefCell<BTreeMap<PadId, Pad>>,
}

impl PadPlotState {
    /// An empty state: every pad as in the board.
    pub fn new() -> Self {
        Self::default()
    }

    /// The pad `id` as the plot left it, `None` while it has not been plotted (it is the
    /// board's pad then).
    pub fn pad(&self, id: PadId) -> Option<Pad> {
        self.pads.borrow().get(&id).cloned()
    }

    /// The number of pads the plot has modified.
    pub fn len(&self) -> usize {
        self.pads.borrow().len()
    }

    /// `true` while no pad has been plotted.
    pub fn is_empty(&self) -> bool {
        self.pads.borrow().is_empty()
    }

    /// Removes and returns the stored pad `id` (put back with [`Self::put`]).
    fn take(&self, id: PadId) -> Option<Pad> {
        self.pads.borrow_mut().remove(&id)
    }

    /// Stores `pad` as the current state of the board's pad `id`.
    fn put(&self, id: PadId, pad: Pad) {
        self.pads.borrow_mut().insert(id, pad);
    }
}

/// Port of `PlotBoardLayers`: plot a sequence of board layers into one plotter (the first
/// layer is the "primary" layer of the file).
///
/// `a_pad_state` is the state of the board's pads that the C++ carries in the board itself
/// ([`PadPlotState`]): the caller passes the same instance for every file of one job.
pub fn plot_board_layers(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layers: &[PcbLayerId],
    a_plot_options: &PcbPlotParams,
    a_colors: ColorSettings,
    a_pad_state: &PadPlotState,
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
            a_colors,
            layer == a_layers[0],
            a_pad_state,
        );
    }

    // Drill symbols go after the normal layers but before the physical marks, so a symbol is
    // never sitting under a knockout
    let map_layers = a_board.board.drill_symbol_layers() & Lset::from_layers(a_layers);

    if map_layers.any() {
        let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, &plot_options, a_colors);
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
        let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, &plot_options, a_colors);
        itemplotter.set_layer_set(&Lset::from_layers(a_layers));
        itemplotter.plot_drill_marks();
    }
}

/// Port of `PlotOneBoardLayer`: plot one layer with the function its kind needs.
// The silk arm keeps the C++ `if( GetSubtractMaskFromSilk() ) { ... break; }` structure.
#[allow(clippy::collapsible_match)]
pub fn plot_one_board_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer: PcbLayerId,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
    is_primary_layer: bool,
    a_pad_state: &PadPlotState,
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

        plot_layer(
            a_board,
            a_plotter,
            &layer_mask,
            &plot_opt,
            a_colors,
            a_pad_state,
        );
    } else {
        match a_layer {
            PcbLayerId::BMask | PcbLayerId::FMask => {
                // Use outline mode for DXF
                plot_opt.dxf_polygon_mode = true;

                // Plot solder mask:
                plot_solder_mask_layer(
                    a_board,
                    a_plotter,
                    &layer_mask,
                    &plot_opt,
                    a_colors,
                    a_pad_state,
                );
            }

            PcbLayerId::BAdhes | PcbLayerId::FAdhes | PcbLayerId::BPaste | PcbLayerId::FPaste => {
                // Disable plot pad holes
                plot_opt.drill_marks = DrillMarks::NoDrillShape;

                // Use outline mode for DXF
                plot_opt.dxf_polygon_mode = true;

                plot_layer(
                    a_board,
                    a_plotter,
                    &layer_mask,
                    &plot_opt,
                    a_colors,
                    a_pad_state,
                );
            }

            PcbLayerId::FSilkS | PcbLayerId::BSilkS => {
                if plot_opt.subtract_mask_from_silk {
                    if a_plotter.plotter_type() == PlotFormat::Gerber && is_primary_layer {
                        // Use old-school, positive/negative mask plotting which preserves
                        // utilization of Gerber aperture masks.  This method can only be used
                        // when the given silk layer is the primary layer as the negative mask
                        // will also knockout any other (non-silk) layers that were plotted
                        // before the silk layer.

                        plot_standard_layer(
                            a_board,
                            a_plotter,
                            &layer_mask,
                            &plot_opt,
                            a_colors,
                            a_pad_state,
                        );

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
                        plot_solder_mask_layer(
                            a_board,
                            a_plotter,
                            &layer_mask,
                            &plot_opt,
                            a_colors,
                            a_pad_state,
                        );

                        // Disable the negative polarity
                        a_plotter.set_layer_polarity(true);
                    } else {
                        plot_clipped_silk_layer(
                            a_board,
                            a_plotter,
                            &layer_mask,
                            &plot_opt,
                            a_colors,
                        );
                    }
                } else {
                    plot_layer(
                        a_board,
                        a_plotter,
                        &layer_mask,
                        &plot_opt,
                        a_colors,
                        a_pad_state,
                    );
                }
            }

            // Dwgs_User, Cmts_User, Eco1_User, Eco2_User, Edge_Cuts, Margin, F/B_CrtYd,
            // F/B_Fab and every other layer
            _ => {
                plot_layer(
                    a_board,
                    a_plotter,
                    &layer_mask,
                    &plot_opt,
                    a_colors,
                    a_pad_state,
                );
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
    a_colors: ColorSettings,
    a_pad_state: &PadPlotState,
) {
    if plot_opts.format == PlotFormat::Dxf && plot_opts.dxf_polygon_mode {
        fab_port_pending!("PlotLayerOutlines (DXF, out of scope)")
    } else {
        plot_standard_layer(
            a_board,
            a_plotter,
            layer_mask,
            plot_opts,
            a_colors,
            a_pad_state,
        );
    }
}

/// Port of `PlotSolderMaskLayer` (L91-107): solder mask layers have a minimum thickness value
/// and cannot be drawn like standard layers, unless the minimum thickness is 0. The polygon
/// path for a non-zero minimum width is in `plot_layer_poly.rs` (FAB-10).
fn plot_solder_mask_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
    a_pad_state: &PadPlotState,
) {
    if a_board.board.design_settings().solder_mask_min_width == 0 {
        plot_layer(
            a_board,
            a_plotter,
            a_layer_mask,
            a_plot_opt,
            a_colors,
            a_pad_state,
        );
        return;
    }

    plot_solder_mask_layer_min_width(a_board, a_plotter, a_layer_mask, a_plot_opt, a_colors);
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

impl StandardLayerFlags {
    /// The preamble of `PlotStandardLayer` (L361-371) for `a_layer_mask`.
    pub fn new(a_board: &BoardCtx<'_>, a_layer_mask: &Lset, a_plot_opt: &PcbPlotParams) -> Self {
        let board = a_board.board;
        let max_error = board.design_settings().max_error;

        let on_copper_layer = (Lset::all_cu_mask() & a_layer_mask).any();
        let on_solder_mask_layer =
            (Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]) & a_layer_mask).any();
        let on_solder_paste_layer =
            (Lset::from_layers(&[PcbLayerId::FPaste, PcbLayerId::BPaste]) & a_layer_mask).any();
        let on_front_fab = (Lset::from_layers(&[PcbLayerId::FFab]) & a_layer_mask).any();
        let on_back_fab = (Lset::from_layers(&[PcbLayerId::BFab]) & a_layer_mask).any();
        let sketch_pads = (on_front_fab || on_back_fab) && a_plot_opt.sketch_pads_on_fab_layers;
        let variant_name = board.get_current_variant().to_string();

        StandardLayerFlags {
            max_error,
            on_copper_layer,
            on_solder_mask_layer,
            on_solder_paste_layer,
            on_front_fab,
            on_back_fab,
            sketch_pads,
            variant_name,
        }
    }
}

/// Port of `PlotStandardLayer`: plot any layer EXCEPT a solder-mask with an enforced minimum
/// width.
///
/// The iteration order of this function defines the object order in the file and, through the
/// aperture list, all D-codes; it is ported literally. The board graphics and footprint
/// texts / graphics are FAB-05 (`PlotBoardGraphicItem`, `PlotFootprintGraphicItems`) and
/// FAB-06 (`PlotFootprintTextItems`); the rest (L385-930) is
/// [`plot_standard_layer_pads_vias_tracks_zones`]. `a_pad_state`: the board's pads as the
/// earlier layers of the plot job left them ([`PadPlotState`]), updated here.
pub fn plot_standard_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
    a_pad_state: &PadPlotState,
) {
    let board = a_board.board;
    let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, a_plot_opt, a_colors);
    itemplotter.pad_state = Some(a_pad_state);

    itemplotter.set_layer_set(a_layer_mask);

    let flags = StandardLayerFlags::new(a_board, a_layer_mask, a_plot_opt);

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

    plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
}

/// `PlotStandardLayer` L383-930: footprint pads (one block per footprint), vias, tracks and
/// zone fills, each container in its own `StartBlock` / `EndBlock`. Public so that the
/// verification can run it on layers without FAB-05 / FAB-06 content while those packages
/// are pending.
pub fn plot_standard_layer_pads_vias_tracks_zones(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    flags: &StandardLayerFlags,
) {
    let board = itemplotter.ctx.board;
    let on_copper_layer = flags.on_copper_layer;

    // Plot footprint pads
    for (footprint, _) in board.footprints() {
        itemplotter.plotter.start_block(None);

        plot_standard_layer_footprint(itemplotter, footprint, flags);

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

        plot_standard_layer_via(itemplotter, &mut gbr_metadata, track, flags);
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

        plot_standard_layer_track(itemplotter, &mut gbr_metadata, track, flags);
    }

    itemplotter.plotter.end_block(None);

    // Plot filled ares
    itemplotter.plotter.start_block(None);

    for (zone, _) in board.zones() {
        plot_standard_layer_zone(itemplotter, zone, flags);
    }

    itemplotter.plotter.end_block(None);
}

/// `PlotStandardLayer` L385-740: the pads of one footprint (sketch pads, pad numbers, the
/// margin / shape adjustments of `plotPadLayer` per layer of `SeqStackupForPlotting()`) and
/// the DNP cross, between the `StartBlock` / `EndBlock` of the caller.
pub fn plot_standard_layer_footprint(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    footprint_id: FootprintId,
    flags: &StandardLayerFlags,
) {
    let ctx = itemplotter.ctx;
    let board = ctx.board;
    let Some(footprint) = board.footprint(footprint_id) else {
        return;
    };
    let dnp = footprint.dnp_for_variant(&flags.variant_name);
    let on_front_fab = flags.on_front_fab;
    let on_back_fab = flags.on_back_fab;

    // Without a job's state (a caller of `plot_standard_layer_pads_vias_tracks_zones` that
    // does not install one) the pads start as in the board.
    let local_pad_state;
    let pad_state = match itemplotter.pad_state {
        Some(state) => state,
        None => {
            local_pad_state = PadPlotState::new();
            &local_pad_state
        }
    };

    for (pad_id, board_pad) in board.footprint_pads(footprint_id) {
        // `pad` is the board's pad as the earlier `plotPadLayer` calls of the job left it.
        let stored = pad_state.take(pad_id);
        let restored = plot_footprint_pad(
            itemplotter,
            pad_id,
            stored.as_ref().unwrap_or(board_pad),
            flags,
        );

        if let Some(pad) = restored.or(stored) {
            pad_state.put(pad_id, pad);
        }
    }

    if dnp
        && !itemplotter.opts.hide_dnp_fps_on_fab_layers
        && itemplotter.opts.crossout_dnp_fps_on_fab_layers
        && ((on_front_fab && footprint.layer(ctx) == PcbLayerId::FCu)
            || (on_back_fab && footprint.layer(ctx) == PcbLayerId::BCu))
    {
        // aPlotter->MarkFootprintPlotted( footprint ): PDF only (out of scope).

        let courtyard = footprint.courtyard(ctx, footprint.layer(ctx));
        let center = footprint.position(ctx);
        let orient = footprint.orientation();

        // Compute a tight oriented bounding box by un-rotating the shape into the
        // footprint's local frame, taking the axis-aligned BBox there, then rotating
        // the four corners back into world coordinates.
        let local_rect: Box2I = if courtyard.is_empty() {
            let mut shape = footprint
                .effective_shape(
                    ctx,
                    PcbLayerId::UndefinedLayer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                )
                .clone_shape();
            shape.rotate(-orient, center);
            shape.bbox(0)
        } else {
            let mut temp = courtyard.clone();
            Shape::rotate(&mut temp, -orient, center);
            Shape::bbox(&temp, 0)
        };

        let mut corner1 = Vector2I::new(local_rect.left(), local_rect.top());
        let mut corner2 = Vector2I::new(local_rect.right(), local_rect.top());
        let mut corner3 = Vector2I::new(local_rect.right(), local_rect.bottom());
        let mut corner4 = Vector2I::new(local_rect.left(), local_rect.bottom());

        rotate_point_around(&mut corner1, center, orient);
        rotate_point_around(&mut corner2, center, orient);
        rotate_point_around(&mut corner3, center, orient);
        rotate_point_around(&mut corner4, center, orient);

        let width = board.design_settings().line_thickness[LAYER_CLASS_FAB];

        // Use DNP cross color from color scheme
        let dnp_marker_color = itemplotter.color_settings_color(LAYER_DNP_MARKER);

        if dnp_marker_color != Color4D::UNSPECIFIED {
            itemplotter.plotter.set_color(&dnp_marker_color);
        } else {
            let color = itemplotter.color_settings_color(if on_front_fab {
                PcbLayerId::FFab.0
            } else {
                PcbLayerId::BFab.0
            });
            itemplotter.plotter.set_color(&color);
        }

        itemplotter
            .plotter
            .thick_segment(corner1, corner3, width, None);
        itemplotter
            .plotter
            .thick_segment(corner2, corner4, width, None);
    }
}

/// The body of the pad loop of `PlotStandardLayer` (L387-447 and the `plotPadLayer` calls) for
/// one pad of a footprint: `pad` is the board's pad `pad_id` as the plot job left it so far.
/// Returns the pad as the `plotPadLayer` calls leave it, `None` when no call modified it (the
/// pad is skipped, or every layer returned before modifying it).
fn plot_footprint_pad(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    pad_id: PadId,
    pad: &Pad,
    flags: &StandardLayerFlags,
) -> Option<Pad> {
    let ctx = itemplotter.ctx;
    let a_layer_mask = itemplotter.layer_mask.clone();
    let sketch_pads = flags.sketch_pads;
    let on_front_fab = flags.on_front_fab;
    let on_back_fab = flags.on_back_fab;
    let mut do_sketch_pads = false;
    let pad_layers = pad.layer_set(ctx);

    if !(&pad_layers & &a_layer_mask).any() {
        if sketch_pads
            && ((on_front_fab && pad_layers.contains(PcbLayerId::FCu))
                || (on_back_fab && pad_layers.contains(PcbLayerId::BCu)))
        {
            do_sketch_pads = true;
        } else {
            return None;
        }
    }

    if flags.on_copper_layer && !pad.is_on_copper_layer(ctx) {
        return None;
    }

    // pads not connected to copper are optionally not drawn
    if flags.on_copper_layer && !pad.flash_layer_set(ctx, &a_layer_mask) {
        return None;
    }

    // TODO(JE) padstacks - different behavior for single layer or multilayer

    let mut color = Color4D::BLACK;

    // If we're plotting a single layer, the color for that layer can be used directly.
    if a_layer_mask.count() == 1 {
        color = itemplotter.color_settings_color(a_layer_mask.seq()[0].0);
    } else {
        let pad_on_mask = &pad_layers & &a_layer_mask;

        if pad_on_mask.contains(PcbLayerId::BCu) {
            color = itemplotter.color_settings_color(PcbLayerId::BCu.0);
        }

        if pad_on_mask.contains(PcbLayerId::FCu) {
            color = color.legacy_mix(&itemplotter.color_settings_color(PcbLayerId::FCu.0));
        }

        if sketch_pads && a_layer_mask.contains(PcbLayerId::FFab) {
            color = itemplotter.color_settings_color(PcbLayerId::FFab.0);
        } else if sketch_pads && a_layer_mask.contains(PcbLayerId::BFab) {
            color = itemplotter.color_settings_color(PcbLayerId::BFab.0);
        }
    }

    if sketch_pads
        && ((on_front_fab && pad_layers.contains(PcbLayerId::FCu))
            || (on_back_fab && pad_layers.contains(PcbLayerId::BCu)))
        && itemplotter.opts.plot_pad_numbers
    {
        itemplotter.plot_pad_number(ctx.view(AnyItem::Pad(pad)), &color);
    }

    let mut restored: Option<Pad> = None;

    for layer in a_layer_mask.seq_stackup_for_plotting().iter() {
        let current = restored.as_ref().unwrap_or(pad);

        if let Some(pad_after) = plot_pad_layer(
            itemplotter,
            pad_id,
            current,
            *layer,
            &color,
            do_sketch_pads,
            flags,
        ) {
            restored = Some(pad_after);
        }
    }

    restored
}

/// The `plotPadLayer` lambda of `PlotStandardLayer` (L446-681): plot `pad` on `a_layer` with
/// its size, shape and corner radius adjusted by the mask / paste margin.
///
/// C++ modifies the board's pad in place and restores the modified members afterwards (or
/// plots a modified copy, `PAD dummy( *pad )`); the port works on a detached copy of `pad`
/// (`board-model.md` 13), applies the C++ restore sequence to it and returns it: the pad as
/// the C++ leaves it on the board (the restore of the size is not exact for a footprint with a
/// scale, see [`PadPlotState`]). `None`: the zero-size exit, which returns before anything is
/// modified.
fn plot_pad_layer(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    pad_id: PadId,
    pad: &Pad,
    a_layer: PcbLayerId,
    color: &Color4D,
    do_sketch_pads: bool,
    flags: &StandardLayerFlags,
) -> Option<Pad> {
    let ctx: BoardCtx<'_> = *itemplotter.ctx;
    let ctx = &ctx;
    let max_error = flags.max_error;
    let mut margin = Vector2I::new(0, 0);
    let mut width_adj = 0;

    if flags.on_copper_layer {
        width_adj = itemplotter.fine_width_adj();
    }

    if flags.on_solder_mask_layer {
        let expansion = pad.solder_mask_expansion(ctx, a_layer);
        margin.x = expansion;
        margin.y = expansion;
    }

    if flags.on_solder_paste_layer {
        margin = pad.solder_paste_margin(ctx, a_layer);
    }

    // not all shapes can have a different margin for x and y axis
    // in fact only oval and rect shapes can have different values.
    // Round shape have always the same x,y margin
    // so define a unique value for other shapes that do not support different values
    let mask_clearance = margin.x;
    // When clearance is same for x and y pad axis, calculations are more easy
    let same_xy_clearance = margin.x == margin.y;

    // Now offset the pad size by margin + width_adj
    let pad_plots_size: Vector2I =
        pad.size(ctx, a_layer) + margin * 2 + Vector2I::new(width_adj, width_adj);

    // Store these parameters that can be modified to plot inflated/deflated pads shape
    let pad_shape = pad.shape(a_layer);
    let pad_size = pad.size(ctx, a_layer);
    let pad_delta = pad.delta(a_layer); // has meaning only for trapezoidal pads
    // CornerRadius and CornerRadiusRatio can be modified
    // the radius is built from the ratio, so saving/restoring the ratio is enough
    let pad_corner_radius_ratio = pad.round_rect_radius_ratio(a_layer);

    // Don't draw a 0 sized pad.
    // Note: a custom pad can have its pad anchor with size = 0
    if pad_shape != PadShape::Custom && (pad_plots_size.x <= 0 || pad_plots_size.y <= 0) {
        return None;
    }

    // The mutators' view of the pad's parent footprint (`GetParentFootprint()`, board).
    let env = ctx.board.mut_env(pad_id.into());
    // `pad` as C++ modifies it: a detached copy, restored at the end and returned.
    let mut work = pad.detached_copy();

    match pad_shape {
        PadShape::Circle | PadShape::Oval => {
            work.set_size(&env, a_layer, pad_plots_size);

            // C++ `break`: the restore below still runs
            let skip = itemplotter.opts.skip_npth_pads
                && itemplotter.opts.drill_marks == DrillMarks::NoDrillShape
                && work.size(ctx, a_layer) == work.drill_size(ctx)
                && work.attribute() == PadAttrib::Npth;

            if !skip {
                itemplotter.plot_pad(
                    ctx.view(AnyItem::Pad(&work)),
                    a_layer,
                    color,
                    do_sketch_pads,
                );
            }
        }

        PadShape::Rectangle => {
            work.set_size(&env, a_layer, pad_plots_size);

            if mask_clearance > 0 {
                work.set_shape(a_layer, PadShape::Roundrect);
                work.set_round_rect_corner_radius(a_layer, f64::from(mask_clearance));
            }

            itemplotter.plot_pad(
                ctx.view(AnyItem::Pad(&work)),
                a_layer,
                color,
                do_sketch_pads,
            );
        }

        PadShape::Trapezoid => {
            // inflate/deflate a trapezoid is a bit complex.
            // so if the margin is not null, build a similar polygonal pad shape,
            // and inflate/deflate the polygonal shape
            // because inflating/deflating using different values for y and y
            // we are using only margin.x as inflate/deflate value
            if mask_clearance == 0 {
                itemplotter.plot_pad(
                    ctx.view(AnyItem::Pad(&work)),
                    a_layer,
                    color,
                    do_sketch_pads,
                );
            } else {
                let mut dummy = pad.detached_copy();
                dummy.set_anchor_pad_shape(a_layer, PadShape::Circle);
                dummy.set_shape(a_layer, PadShape::Custom);
                let mut outline = ShapePolySet::new();
                outline.new_outline();
                let dx = pad_size.x / 2;
                let dy = pad_size.y / 2;
                let ddx = pad_delta.x / 2;
                let ddy = pad_delta.y / 2;

                outline.append(-dx - ddy, dy + ddx, -1, -1, false);
                outline.append(dx + ddy, dy - ddx, -1, -1, false);
                outline.append(dx - ddy, -dy + ddx, -1, -1, false);
                outline.append(-dx + ddy, -dy - ddx, -1, -1, false);

                // Shape polygon can have holes so use InflateWithLinkedHoles(), not Inflate()
                // which can create bad shapes if margin.x is < 0
                outline.inflate_with_linked_holes(
                    mask_clearance,
                    CornerStrategy::RoundAllCorners,
                    max_error,
                );
                dummy.delete_primitives_list(PcbLayerId::UndefinedLayer);
                dummy.add_primitive_poly(&env, a_layer, &outline, 0, true);

                // Be sure the anchor pad is not bigger than the deflated shape because this
                // anchor will be added to the pad shape when plotting the pad. So now the
                // polygonal shape is built, we can clamp the anchor size
                dummy.set_size(&env, a_layer, Vector2I::new(0, 0));

                itemplotter.plot_pad(
                    ctx.view(AnyItem::Pad(&dummy)),
                    a_layer,
                    color,
                    do_sketch_pads,
                );
            }
        }

        PadShape::Roundrect => {
            // The Minkowski sum of a rounded rectangle with a disk of radius R is
            // another rounded rectangle whose sides grow by 2R and whose corner
            // radius grows by R. Preserving the original radius_ratio instead
            // produces visibly inconsistent expansion at the corners (issue 24327).
            if same_xy_clearance {
                let original_radius = work.round_rect_corner_radius(a_layer);
                let new_radius = 0.max(original_radius + mask_clearance + width_adj / 2);
                work.set_size(&env, a_layer, pad_plots_size);
                work.set_round_rect_corner_radius(a_layer, f64::from(new_radius));
            } else {
                // Asymmetric X/Y clearance (e.g. solder paste ratio on a
                // non-square pad) is not a Minkowski sum with a disk. Fall back
                // to the historical behavior of scaling both axes by the per-axis
                // margin while keeping the radius_ratio. This is approximate at
                // the corners but preserves the bounding box, which is the
                // dimension users rely on for paste apertures.
                let radius_ratio = work.round_rect_radius_ratio(a_layer);
                work.set_size(&env, a_layer, pad_plots_size);
                work.set_round_rect_radius_ratio(a_layer, radius_ratio);
            }

            itemplotter.plot_pad(
                ctx.view(AnyItem::Pad(&work)),
                a_layer,
                color,
                do_sketch_pads,
            );
        }

        PadShape::ChamferedRect => {
            // for smaller/same rect size than initial shape (i.e. mask_clearance <= 0)
            // use the rect with size set to padPlotsSize. It gives a good shape
            if mask_clearance <= 0 {
                // the size can be slightly inflated by width_adj (PS/PDF only)
                work.set_size(&env, a_layer, pad_plots_size);
                itemplotter.plot_pad(
                    ctx.view(AnyItem::Pad(&work)),
                    a_layer,
                    color,
                    do_sketch_pads,
                );
            } else {
                // Due to the polygonal shape of a CHAMFERED_RECT pad, the best way is to
                // convert the pad shape to a full polygon and inflate it
                // and use a dummy  CUSTOM pad to plot the final shape.
                // However one can inflate polygon only if X,Y has same inflate value
                // if not the case, just use a rectangle having the padPlotsSize new size
                let mut dummy = pad.detached_copy();
                // Build the dummy pad outline with coordinates relative to the pad position
                // pad offset and orientation 0. The actual pos, offset and rotation will be
                // taken in account later by the plot function
                dummy.set_position(&env, Vector2I::new(0, 0));
                dummy.set_offset(&env, a_layer, Vector2I::new(0, 0));

                if !same_xy_clearance {
                    dummy.set_size(&env, a_layer, pad_plots_size);
                }

                dummy.set_orientation(&env, ANGLE_0);
                let mut outline = ShapePolySet::new();
                dummy.transform_shape_to_polygon(
                    ctx,
                    &mut outline,
                    a_layer,
                    0,
                    max_error,
                    ErrorLoc::Inside,
                    false,
                );

                if same_xy_clearance {
                    outline.inflate_with_linked_holes(
                        mask_clearance,
                        CornerStrategy::RoundAllCorners,
                        max_error,
                    );
                }

                // Initialize the dummy pad shape:
                dummy.set_anchor_pad_shape(a_layer, PadShape::Circle);
                dummy.set_shape(a_layer, PadShape::Custom);
                dummy.delete_primitives_list(PcbLayerId::UndefinedLayer);
                dummy.add_primitive_poly(&env, a_layer, &outline, 0, true);

                // Be sure the anchor pad is not bigger than the deflated shape because this
                // anchor will be added to the pad shape when plotting the pad.
                // So we set the anchor size to 0
                dummy.set_size(&env, a_layer, Vector2I::new(0, 0));
                // Restore pad position and offset
                dummy.set_position(&env, pad.position(ctx));
                dummy.set_offset(&env, a_layer, pad.offset(ctx, a_layer));
                dummy.set_orientation(&env, pad.orientation(ctx));

                itemplotter.plot_pad(
                    ctx.view(AnyItem::Pad(&dummy)),
                    a_layer,
                    color,
                    do_sketch_pads,
                );
            }
        }

        PadShape::Custom => {
            // inflate/deflate a custom shape is a bit complex.
            // so build a similar pad shape, and inflate/deflate the polygonal shape
            let mut dummy = pad.detached_copy();
            // dummy.SetParentGroup( nullptr ): no effect on plotting.

            let mut shape = ShapePolySet::new();
            pad.merge_primitives_as_polygon(ctx, a_layer, &mut shape, ErrorLoc::Inside);

            // Shape polygon can have holes so use InflateWithLinkedHoles(), not Inflate()
            // which can create bad shapes if margin.x is < 0
            shape.inflate_with_linked_holes(
                mask_clearance,
                CornerStrategy::RoundAllCorners,
                max_error,
            );
            dummy.delete_primitives_list(PcbLayerId::UndefinedLayer);
            dummy.add_primitive_poly(&env, a_layer, &shape, 0, true);

            // Be sure the anchor pad is not bigger than the deflated shape because this
            // anchor will be added to the pad shape when plotting the pad. So now the
            // polygonal shape is built, we can clamp the anchor size
            if mask_clearance < 0 {
                // we expect margin.x = margin.y for custom pads
                dummy.set_size(
                    &env,
                    a_layer,
                    Vector2I::new(0.max(pad_plots_size.x), 0.max(pad_plots_size.y)),
                );
            }

            itemplotter.plot_pad(
                ctx.view(AnyItem::Pad(&dummy)),
                a_layer,
                color,
                do_sketch_pads,
            );
        }
    }

    // Restore the pad parameters modified by the plot code.
    // The board's pad keeps its identity (`work` is a detached copy without the arena id), so
    // the restore is applied to a clone of `pad`: the plot code above modifies `work` only
    // through the members restored here (size, shape, corner radius ratio of `a_layer`), each
    // of which the restore overwrites, so the result is the pad C++ leaves on the board.
    let mut restored = pad.clone();
    restored.set_size(&env, a_layer, pad_size);
    restored.set_delta(a_layer, pad_delta);
    restored.set_shape(a_layer, pad_shape);
    restored.set_round_rect_radius_ratio(a_layer, pad_corner_radius_ratio);

    Some(restored)
}

/// The `getMetadata` lambda of `PlotStandardLayer`: the Gerber metadata for a Gerber
/// plotter, nothing for the other engines (DXF, which gets the plot options, is out of
/// scope).
fn plot_metadata<'m>(plotter: &dyn Plotter, gbr_metadata: &'m GbrMetadata) -> PlotData<'m> {
    if plotter.plotter_type() == PlotFormat::Gerber {
        Some(gbr_metadata)
    } else {
        None
    }
}

/// `PlotStandardLayer` L768-831: the body of the via loop for one via.
pub fn plot_standard_layer_via(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    gbr_metadata: &mut GbrMetadata,
    via_id: TrackId,
    flags: &StandardLayerFlags,
) {
    let ctx = itemplotter.ctx;
    let a_layer_mask = itemplotter.layer_mask.clone();
    let Some(item) = ctx.item(via_id.into()) else {
        return;
    };
    let Some(via) = item.as_via() else {
        return;
    };

    // vias are not plotted if not on selected layer
    let via_mask_layer = via.layer_set(ctx);

    if !(&via_mask_layer & &a_layer_mask).any() {
        return;
    }

    let mut via_margin = 0;
    let mut width_adj: f64 = 0.0;

    // TODO(JE) padstacks - separate top/bottom margin
    if flags.on_solder_mask_layer {
        via_margin = via.solder_mask_expansion(ctx);
    }

    if (&a_layer_mask & Lset::all_cu_mask()).any() {
        width_adj = f64::from(itemplotter.fine_width_adj());
    }

    // Vias not connected to copper are optionally not drawn
    if flags.on_copper_layer && !via.flash_layer_set(ctx, &a_layer_mask) {
        return;
    }

    let mut diameter = 0;

    for layer in a_layer_mask.iter() {
        diameter = diameter.max(via.width(layer));
    }

    // diameter += 2 * via_margin + width_adj: int += double
    diameter = f64_to_i32_x86(f64::from(diameter) + (f64::from(2 * via_margin) + width_adj));

    // Don't draw a null size item :
    if diameter <= 0 {
        return;
    }

    // Some vias can be not connected (no net).
    // Set the m_NotInNet for these vias to force a empty net name in gerber file
    gbr_metadata.netlist_metadata.not_in_net = item.netname().is_empty();

    gbr_metadata.set_net_name(item.netname());

    // If we're plotting a single layer, the color for that layer can be used directly.
    let mut color = if a_layer_mask.count() == 1 {
        itemplotter.color_settings_color(a_layer_mask.seq()[0].0)
    } else {
        itemplotter.color_settings_color(LAYER_VIAS + via.via_type() as i32)
    };

    // Change UNSPECIFIED or WHITE to LIGHTGRAY because the white items are not seen on a
    // white paper or screen
    if color == Color4D::UNSPECIFIED || color == Color4D::WHITE {
        color = Color4D::from_eda_color(EdaColorT::LIGHTGRAY);
    }

    itemplotter.plotter.set_color(&color);
    let start = via.track.start();
    let metadata = plot_metadata(&*itemplotter.plotter, gbr_metadata);
    itemplotter
        .plotter
        .flash_pad_circle(start, diameter, metadata);
}

/// `PlotStandardLayer` L848-889: the body of the track loop for one segment or arc.
pub fn plot_standard_layer_track(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    gbr_metadata: &mut GbrMetadata,
    track_id: TrackId,
    flags: &StandardLayerFlags,
) {
    let ctx = itemplotter.ctx;
    let a_layer_mask = itemplotter.layer_mask.clone();
    let Some(item) = ctx.item(track_id.into()) else {
        return;
    };
    let Some(track) = item.as_track() else {
        return;
    };

    if !(&a_layer_mask & &item.layer_set()).any() {
        return;
    }

    // Some track segments can be not connected (no net).
    // Set the m_NotInNet for these segments to force a empty net name in gerber file
    gbr_metadata.netlist_metadata.not_in_net = item.netname().is_empty();

    gbr_metadata.set_net_name(item.netname());

    let mut margin = 0;

    if flags.on_solder_mask_layer {
        margin = match track {
            Track::Trace(segment) => segment.solder_mask_expansion(ctx),
            Track::Arc(arc) => arc.solder_mask_expansion(ctx),
            Track::Via(via) => via.solder_mask_expansion(ctx),
        };
    }

    let width = track.width() + 2 * margin + itemplotter.fine_width_adj();

    let color = itemplotter.get_color(item.layer().0);
    itemplotter.plotter.set_color(&color);

    let segment = track.as_track();
    let metadata = plot_metadata(&*itemplotter.plotter, gbr_metadata);

    if let Track::Arc(arc) = track {
        // Too small arcs cannot be really handled: arc center (and arc radius)
        // cannot be safely computed
        if !arc.is_degenerated(10 /* in IU */) {
            itemplotter.plotter.thick_arc(
                Vector2D::from(arc.center(ctx)),
                arc.arc_angle_start(),
                arc.angle(),
                arc.radius(),
                width,
                metadata,
            );
        } else {
            // Approximate this very small arc by a segment.
            itemplotter
                .plotter
                .thick_segment(segment.start(), segment.end(), width, metadata);
        }
    } else {
        itemplotter
            .plotter
            .thick_segment(segment.start(), segment.end(), width, metadata);
    }
}

/// `PlotStandardLayer` L898-928: the body of the zone loop for one zone (rule areas skipped,
/// fills per layer of the mask, islands with a dummy no-net copy).
pub fn plot_standard_layer_zone(
    itemplotter: &mut BrditemsPlotter<'_, '_>,
    zone_id: ZoneId,
    flags: &StandardLayerFlags,
) {
    let _ = flags;
    let ctx: BoardCtx<'_> = *itemplotter.ctx;
    let ctx = &ctx;
    let a_layer_mask = itemplotter.layer_mask.clone();
    let Some(item) = ctx.item(zone_id.into()) else {
        return;
    };
    let Some(zone) = item.as_zone() else {
        return;
    };

    if zone.is_rule_area() {
        return;
    }

    for layer in zone.layer_set(ctx).seq().iter().copied() {
        if !a_layer_mask.contains(layer) {
            continue;
        }

        let mut main_area = zone.filled_polys_list(layer).clone_drop_triangulation();
        let mut islands = ShapePolySet::new();

        for i in (0..main_area.outline_count()).rev() {
            if zone.is_island(layer, i) {
                let outline = main_area.c_polygon(i)[0].clone();
                islands.add_outline(&outline);
                main_area.delete_polygon(i);
            }
        }

        itemplotter.plot_zone(item, layer, &main_area);

        if !islands.is_empty() {
            // ZONE dummy( *zone ); dummy.SetNet( &nonet ): `NETINFO_ITEM nonet( aBoard )` has
            // the net code -1 and an empty name, which no board net carries.
            let mut dummy = zone.detached_copy();
            dummy.base.net = NetCode::ORPHANED;
            itemplotter.plot_zone(ctx.view(AnyItem::Zone(&dummy)), layer, &islands);
        }
    }
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
        offset.x =
            ki_round(board_center.x as f64 - (paper_size_iu.x as f64 / 2.0) / compound_scale);
        offset.y =
            ki_round(board_center.y as f64 - (paper_size_iu.y as f64 / 2.0) / compound_scale);
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
/// `PlotDrawingSheet`; the C++ defaults are `wxEmptyString` and 1); `a_env` replaces
/// `GetBuildVersion()`, the clock and the drawing sheet singleton; `a_colors` is
/// `aPlotOpts->ColorSettings()` (the port's `PcbPlotParams` has none), read for the drawing
/// sheet colour.
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
    a_page_number: &str,
    a_page_count: i32,
    a_colors: &ColorSettings,
    a_env: &PlotEnv,
) -> Result<Box<dyn Plotter>, FabError> {
    let board = a_board.board;

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
                let variant_name = board.get_current_variant().to_string();
                let variant_desc = board.get_variant_description(&variant_name);

                // renderSettings->GetLayerColor( LAYER_DRAWINGSHEET ): PCB_RENDER_SETTINGS::
                // LoadColors copies the GAL layer colours of the theme unchanged.
                let color = a_colors.get_color(LAYER_DRAWINGSHEET);

                let project = |token: &mut String| -> bool {
                    board
                        .project
                        .as_ref()
                        .is_some_and(|p| p.text_var_resolver(&board.text_var_clock, token))
                };
                let mut sheet = a_env.drawing_sheet.clone();
                let base_version = sheet.base_version.clone();
                let args = PlotDrawingSheetArgs {
                    project: board
                        .project
                        .as_ref()
                        .map(|_| &project as &TextVarResolver<'_>),
                    title_block: &board.titles,
                    page_info: &board.paper,
                    properties: Some(&board.properties),
                    sheet_number: a_page_number,
                    sheet_count: a_page_count,
                    sheet_name: a_sheet_name,
                    sheet_path: a_sheet_path,
                    filename: &board.file_name,
                    color,
                    is_first_page: true,
                    variant_name: &variant_name,
                    variant_desc: &variant_desc,
                    layer_name: a_layer_name,
                    kicad_version: base_version.as_deref(),
                    clock: &board.text_var_clock,
                };

                if plot_drawing_sheet(plotter.as_mut(), &mut sheet.model, &args).is_err() {
                    return Err(FabError::NotPorted(
                        "PlotDrawingSheet: a drawing sheet bitmap in a plot that embeds images (SVG)",
                    ));
                }

                if a_plot_opts.mirror || a_plot_opts.scale != 1.0 || a_plot_opts.auto_scale {
                    initialize_plotter(plotter.as_mut(), a_board, a_plot_opts);
                }
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
        "",
        1,
        &ColorSettings::builtin_default(),
        a_env,
    )?;

    // The colour theme of `kicad-cli` (`_builtin_default`); Gerber output ignores colours.
    // One file from a freshly loaded board: the pads start as in the board.
    plot_board_layers(
        a_board,
        plotter.as_mut(),
        a_plot_sequence,
        a_plot_opts,
        ColorSettings::builtin_default(),
        &PadPlotState::new(),
    );
    plotter.end_plot();

    Ok(OutputFile {
        name: full_path,
        contents: plotter.output().as_bytes().to_vec(),
    })
}
