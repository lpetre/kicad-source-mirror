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

//! Port of `pcbnew/plot_board_layers.cpp` L49-125, L1020-1275 (solder mask minimum width,
//! clipped silk, `GenerateLayerPoly`).
//!
//! Work package FAB-10 (`docs/analysis/plot-fab.md` 10.2): `PlotPolySet`,
//! `PlotSolderMaskLayer` (the `m_SolderMaskMinWidth > 0` path; the `== 0` early return stays
//! in `plot_board_layers.rs` with its caller), `PlotClippedSilkLayer` and `GenerateLayerPoly`.
//! The polygons come from Clipper operations whose operand order decides the output bytes:
//! every item kind is converted in the C++ order, into the same buffers.

use kicad_core::stroke_params::LineStyle;
use kicad_core::{KicadT, Lset, PcbLayerId};
use kicad_geom::ShapePolySet;
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::DisableArcRadiusCorrection;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::eda_shape::shapes::make_effective_shapes_for_stroking;
use kicad_pcb::item::ParentRef;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::pcb_textbox::PcbTextbox;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::stroke_params::{StrokePatternLengths, stroke};
use kicad_pcb::traits::BoardItem;
use kicad_pcb::zone::Zone;
use kicad_pcb::{AnyItem, ItemRef};
use kicad_plot::{Plotter, RenderSettings};

use crate::color_settings::ColorSettings;
use crate::plot_brditems_plotter::BrditemsPlotter;

/// `lineStyle <= LINE_STYLE::FIRST_TYPE` (`DEFAULT` and `SOLID`).
fn is_solid_style(line_style: LineStyle) -> bool {
    (line_style as i32) <= (LineStyle::Solid as i32)
}

/// The three pattern lengths `STROKE_PARAMS::Stroke` reads from `aRenderSettings` for
/// `a_width`. `StartPlotBoard` always installs render settings before items are plotted.
fn stroke_lengths(rs: Option<&RenderSettings>, a_width: i32) -> StrokePatternLengths {
    let rs = rs.expect("STROKE_PARAMS::Stroke: the plotter has render settings");
    StrokePatternLengths {
        dash: rs.dash_length(a_width),
        dot: rs.dot_length(a_width),
        gap: rs.gap_length(a_width),
    }
}

/// Port of `PlotPolySet`: plot `a_poly_set` (fractured in place) on `a_layer` as the filled
/// areas of a dummy zone.
pub fn plot_poly_set(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
    a_poly_set: &mut ShapePolySet,
    a_layer: PcbLayerId,
) {
    let mut itemplotter = BrditemsPlotter::new(a_plotter, a_board, a_plot_opt, a_colors);
    let layers = Lset::from_layers(&[a_layer]);

    itemplotter.set_layer_set(&layers);

    // To avoid a lot of code, use a ZONE to handle and plot polygons, because our polygons look
    // exactly like filled areas in zones.
    // Note, also this code is not optimized: it creates a lot of copy/duplicate data.
    // However it is not complex, and fast enough for plot purposes (copy/convert data is only a
    // very small calculation time for these calculations).
    //
    // `ZONE zone( aBoard )`: PlotZone only reads the layer (never copper here) and the net
    // (none), so the board's default zone settings exported by the constructor do not matter.
    let env = a_board.board.child_env(ParentRef::Board);
    let mut zone = Zone::default();
    zone.base.item.parent = ParentRef::Board;
    zone.set_min_thickness(0);
    zone.set_layer(&env, a_layer);

    a_poly_set.fracture(true);
    itemplotter.plot_zone(a_board.view(AnyItem::Zone(&zone)), a_layer, a_poly_set);
}

/// Port of `PlotSolderMaskLayer` for `m_SolderMaskMinWidth != 0` (the caller in
/// `plot_board_layers.rs` keeps the `== 0` early return to `PlotLayer`): the layer polygon
/// with too-close shapes merged, plotted as regions.
pub fn plot_solder_mask_layer_min_width(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
) {
    let mut solder_mask = ShapePolySet::new();
    let layer = if a_layer_mask.contains(PcbLayerId::BMask) {
        PcbLayerId::BMask
    } else {
        PcbLayerId::FMask
    };

    generate_layer_poly(
        &mut solder_mask,
        a_board,
        &*a_plotter,
        layer,
        a_plot_opt.plot_fp_text,
        a_plot_opt.plot_reference,
        a_plot_opt.plot_value,
    );

    plot_poly_set(
        a_board,
        a_plotter,
        a_plot_opt,
        a_colors,
        &mut solder_mask,
        layer,
    );
}

/// Port of `PlotClippedSilkLayer`: the silk layer polygon minus the solder mask polygon of the
/// same side (`--subtract-soldermask` when the silk is not the primary layer of a Gerber file,
/// or for SVG).
pub fn plot_clipped_silk_layer(
    a_board: &BoardCtx<'_>,
    a_plotter: &mut dyn Plotter,
    a_layer_mask: &Lset,
    a_plot_opt: &PcbPlotParams,
    a_colors: ColorSettings,
) {
    let mut silkscreen = ShapePolySet::new();
    let mut solder_mask = ShapePolySet::new();
    let front = a_layer_mask.contains(PcbLayerId::FSilkS);
    let silk_layer = if front {
        PcbLayerId::FSilkS
    } else {
        PcbLayerId::BSilkS
    };
    let mask_layer = if front {
        PcbLayerId::FMask
    } else {
        PcbLayerId::BMask
    };

    generate_layer_poly(
        &mut silkscreen,
        a_board,
        &*a_plotter,
        silk_layer,
        a_plot_opt.plot_fp_text,
        a_plot_opt.plot_reference,
        a_plot_opt.plot_value,
    );
    generate_layer_poly(
        &mut solder_mask,
        a_board,
        &*a_plotter,
        mask_layer,
        a_plot_opt.plot_fp_text,
        a_plot_opt.plot_reference,
        a_plot_opt.plot_value,
    );

    silkscreen.boolean_subtract(&solder_mask);
    plot_poly_set(
        a_board,
        a_plotter,
        a_plot_opt,
        a_colors,
        &mut silkscreen,
        silk_layer,
    );
}

/// The state the lambdas of `GenerateLayerPoly` capture (`handlePcbShape`,
/// `handleFPTextItem`, `handleTextBoxItem`), minus the two buffers, which are passed in.
struct LayerPolyGen<'c, 'b> {
    ctx: &'c BoardCtx<'b>,
    render_settings: Option<&'c RenderSettings>,
    layer: PcbLayerId,
    max_error: i32,
    inflate: i32,
    plot_fp_text: bool,
    plot_references: bool,
    plot_values: bool,
}

impl LayerPolyGen<'_, '_> {
    /// `handlePcbShape`: plot a non filled basic `PCB_SHAPE`, according to the line style.
    fn handle_pcb_shape(
        &self,
        a_pcb_shape: &PcbShape,
        result: &mut ShapePolySet,
        exact_polys: &mut ShapePolySet,
    ) {
        let ctx = self.ctx;
        let line_style = a_pcb_shape.stroke(ctx).line_style();
        let (inflate, max_error) = (self.inflate, self.max_error);

        if is_solid_style(line_style) {
            if inflate != 0 {
                a_pcb_shape.transform_shape_to_poly_set(
                    ctx,
                    exact_polys,
                    self.layer,
                    0,
                    max_error,
                    ErrorLoc::Outside,
                );
            }

            a_pcb_shape.transform_shape_to_poly_set(
                ctx,
                result,
                self.layer,
                inflate,
                max_error,
                ErrorLoc::Outside,
            );
        } else {
            let thickness = a_pcb_shape.width(ctx);
            let shapes = make_effective_shapes_for_stroking(a_pcb_shape, ctx, thickness);
            let min_seg_count = 8;
            let lengths = stroke_lengths(self.render_settings, thickness);

            for shape in &shapes {
                stroke(&**shape, line_style, thickness, &lengths, &mut |a, b| {
                    if inflate != 0 {
                        transform_oval_to_polygon(
                            exact_polys,
                            a,
                            b,
                            thickness,
                            max_error,
                            ErrorLoc::Outside,
                            min_seg_count,
                        );
                    }

                    transform_oval_to_polygon(
                        result,
                        a,
                        b,
                        thickness + inflate,
                        max_error,
                        ErrorLoc::Outside,
                        min_seg_count,
                    );
                });
            }
        }
    }

    /// `handleFPTextItem` for a footprint field or footprint text (`static_cast<const
    /// PCB_TEXT&>`): skipped when footprint texts are not plotted, and the
    /// `${REFERENCE}` / `${VALUE}` texts with their plot options.
    fn handle_fp_text_item(
        &self,
        a_text: ItemRef<'_>,
        result: &mut ShapePolySet,
        exact_polys: &mut ShapePolySet,
    ) {
        if !self.plot_fp_text {
            return;
        }

        let text = a_text
            .as_eda_text()
            .expect("handleFPTextItem: a PCB_TEXT")
            .eda_text()
            .text();

        if text == "${REFERENCE}" && !self.plot_references {
            return;
        }

        if text == "${VALUE}" && !self.plot_values {
            return;
        }

        if self.inflate != 0 {
            self.transform_text_to_poly_set(a_text, exact_polys, 0);
        }

        self.transform_text_to_poly_set(a_text, result, self.inflate);
    }

    /// `PCB_TEXT::TransformTextToPolySet( aBuffer, aClearance, maxError, ERROR_OUTSIDE )` of
    /// a field or a text.
    fn transform_text_to_poly_set(
        &self,
        a_text: ItemRef<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
    ) {
        let ctx = self.ctx;

        match a_text.any() {
            AnyItem::Field(field) => field.transform_text_to_poly_set(
                ctx,
                buffer,
                clearance,
                self.max_error,
                ErrorLoc::Outside,
            ),
            AnyItem::Text(text) => text.transform_text_to_poly_set(
                ctx,
                buffer,
                clearance,
                self.max_error,
                ErrorLoc::Outside,
            ),
            _ => unreachable!("TransformTextToPolySet: a PCB_TEXT"),
        }
    }

    /// `handleTextBoxItem`: the text, then the box outline around it (as a `PCB_SHAPE`
    /// polygon) when the border is enabled and the text box is not knocked out.
    fn handle_text_box_item(
        &self,
        a_text_box: &PcbTextbox,
        result: &mut ShapePolySet,
        exact_polys: &mut ShapePolySet,
    ) {
        let ctx = self.ctx;

        if self.inflate != 0 {
            a_text_box.transform_text_to_poly_set(
                ctx,
                exact_polys,
                0,
                self.max_error,
                ErrorLoc::Outside,
            );
        }

        a_text_box.transform_text_to_poly_set(
            ctx,
            result,
            self.inflate,
            self.max_error,
            ErrorLoc::Outside,
        );

        // plot box outline around text
        if a_text_box.is_border_enabled() && !BoardItem::is_knockout(a_text_box) {
            let shapebox = a_text_box.polygonal_box_shape(ctx);
            self.handle_pcb_shape(&shapebox, result, exact_polys);
        }
    }

    /// The "Shold not occur" branch: `item->TransformShapeToPolySet( .., aRenderSettings )`
    /// (`with_render_settings`: the board drawings pass `aPlotter->RenderSettings()`, the
    /// footprint items nothing). Only `PCB_TABLE` reads the render settings.
    fn transform_other_item(
        &self,
        item: ItemRef<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        with_render_settings: bool,
    ) {
        let ctx = self.ctx;

        match item.any() {
            AnyItem::Table(table) if with_render_settings => table
                .transform_shape_to_poly_set_with_render_settings(
                    ctx,
                    buffer,
                    self.layer,
                    clearance,
                    self.max_error,
                    ErrorLoc::Outside,
                    self.render_settings,
                ),
            _ => item.as_dyn().transform_shape_to_poly_set(
                ctx,
                buffer,
                self.layer,
                clearance,
                self.max_error,
                ErrorLoc::Outside,
            ),
        }
    }

    /// A board drawing or footprint graphic item that is a text box, a shape or anything
    /// else (the common tail of both item loops of `GenerateLayerPoly`).
    fn handle_graphic_item(
        &self,
        item: ItemRef<'_>,
        result: &mut ShapePolySet,
        exact_polys: &mut ShapePolySet,
        with_render_settings: bool,
    ) {
        if item.type_() == KicadT::PcbTextboxT {
            let textbox = item.as_textbox().expect("PCB_TEXTBOX");
            self.handle_text_box_item(textbox, result, exact_polys);
        } else if let AnyItem::Shape(pcb_shape) = item.any() {
            // dynamic_cast<const PCB_SHAPE*>: of the PCB_SHAPE subclasses only PCB_TEXTBOX
            // (handled above) and PCB_TABLECELL (never a direct child) exist.
            self.handle_pcb_shape(pcb_shape, result, exact_polys);
        } else {
            // Shold not occur
            if self.inflate != 0 {
                self.transform_other_item(item, exact_polys, 0, with_render_settings);
            }

            self.transform_other_item(item, result, self.inflate, with_render_settings);
        }
    }
}

/// Port of `GenerateLayerPoly`: a `SHAPE_POLY_SET` representing the plotted items on
/// `a_layer`. On the mask layers every shape is inflated by half the minimum width (less 1
/// nm), the union deflated again and the exact shapes added back, which merges shapes closer
/// than the minimum width.
pub fn generate_layer_poly(
    a_result: &mut ShapePolySet,
    a_board: &BoardCtx<'_>,
    a_plotter: &dyn Plotter,
    a_layer: PcbLayerId,
    a_plot_fp_text: bool,
    a_plot_references: bool,
    a_plot_values: bool,
) {
    let ctx = a_board;
    let board = ctx.board;
    let max_error = board.design_settings().max_error;
    let mut inflate = 0;

    if a_layer == PcbLayerId::FMask || a_layer == PcbLayerId::BMask {
        // We remove 1nm as we expand both sides of the shapes, so allowing for a strictly greater
        // than or equal comparison in the shape separation (boolean add)
        inflate = board.design_settings().solder_mask_min_width / 2 - 1;
    }

    // Build polygons for each pad shape.  The size of the shape on solder mask should be size
    // of pad + clearance around the pad, where clearance = solder mask clearance + extra margin.
    // Extra margin is half the min width for solder mask, which is used to merge too-close shapes
    // (distance < SolderMaskMinWidth).

    // Will contain exact shapes of all items on solder mask.  We add this back in at the end just
    // to make sure that any artefacts introduced by the inflate/deflate don't remove parts of the
    // individual shapes.
    let mut exact_polys = ShapePolySet::new();

    let r#gen = LayerPolyGen {
        ctx,
        render_settings: a_plotter.render_settings(),
        layer: a_layer,
        max_error,
        inflate,
        plot_fp_text: a_plot_fp_text,
        plot_references: a_plot_references,
        plot_values: a_plot_values,
    };

    // Generate polygons with arcs inside the shape or exact shape to minimize shape changes
    // created by arc to segment size correction.
    // (The C++ disabler lives until the end of the function: the merge below runs with it.)
    let _disabler = DisableArcRadiusCorrection::new();
    {
        // Plot footprint pads and graphics
        for (footprint_id, footprint) in board.footprints() {
            if inflate != 0 {
                footprint.transform_pads_to_poly_set(
                    ctx,
                    &mut exact_polys,
                    a_layer,
                    0,
                    max_error,
                    ErrorLoc::Outside,
                );
            }

            // int outlinesBefore = aResult->OutlineCount(); ... MarkFootprintPlotted(): PDF
            // bookmarks only (out of scope).
            footprint.transform_pads_to_poly_set(
                ctx,
                a_result,
                a_layer,
                inflate,
                max_error,
                ErrorLoc::Outside,
            );

            for (field_id, field) in board.footprint_fields(footprint_id) {
                let Some(item) = ctx.item(field_id.into()) else {
                    continue; // wxCHECK2( field, continue )
                };

                if field.is_reference() && !a_plot_references {
                    continue;
                }

                if field.is_value() && !a_plot_values {
                    continue;
                }

                let visible = item
                    .as_eda_text()
                    .expect("PCB_FIELD")
                    .eda_text()
                    .is_visible();

                if visible && item.is_on_layer(a_layer) {
                    // aPlotter->MarkFootprintPlotted( footprint ): PDF only.
                    r#gen.handle_fp_text_item(item, a_result, &mut exact_polys);
                }
            }

            for (id, _) in board.footprint_drawings(footprint_id) {
                let Some(item) = ctx.item(id) else {
                    continue;
                };

                if item.is_on_layer(a_layer) {
                    // aPlotter->MarkFootprintPlotted( footprint ): PDF only.

                    if item.type_() == KicadT::PcbTextT {
                        r#gen.handle_fp_text_item(item, a_result, &mut exact_polys);
                    } else {
                        r#gen.handle_graphic_item(item, a_result, &mut exact_polys, false);
                    }
                }
            }
        }

        // Plot untented vias and tracks
        for (track_id, track) in board.tracks() {
            let Some(item) = ctx.item(track_id.into()) else {
                continue;
            };

            // Note: IsOnLayer() checks relevant mask layers of untented vias and tracks
            if !item.is_on_layer(a_layer) {
                continue;
            }

            // `track->GetSolderMaskExpansion()` through a `const PCB_TRACK*`: the function is
            // not virtual, so a via runs the PCB_TRACK body (rules, local margin of the track
            // base, board setting, clamped to -m_width / 2), not PCB_VIA's.
            let clearance = match track {
                Track::Trace(segment) => segment.solder_mask_expansion(ctx),
                Track::Arc(arc) => arc.solder_mask_expansion(ctx),
                Track::Via(via) => via.track.solder_mask_expansion(ctx),
            };

            if inflate != 0 {
                item.transform_shape_to_polygon(
                    &mut exact_polys,
                    a_layer,
                    clearance,
                    max_error,
                    ErrorLoc::Outside,
                    false,
                );
            }

            item.transform_shape_to_polygon(
                a_result,
                a_layer,
                clearance + inflate,
                max_error,
                ErrorLoc::Outside,
                false,
            );
        }

        for (id, _) in board.drawings() {
            let Some(item) = ctx.item(id) else {
                continue;
            };

            if item.is_on_layer(a_layer) {
                if item.type_() == KicadT::PcbTextT {
                    if inflate != 0 {
                        r#gen.transform_text_to_poly_set(item, &mut exact_polys, 0);
                    }

                    r#gen.transform_text_to_poly_set(item, a_result, inflate);
                } else {
                    r#gen.handle_graphic_item(item, a_result, &mut exact_polys, true);
                }
            }
        }

        // Add filled zone areas.
        for (_, zone) in board.zones() {
            if zone.is_rule_area() {
                continue;
            }

            if !zone.is_on_layer(ctx, a_layer) {
                continue;
            }

            let Some(fill_data) = zone.get_fill(a_layer) else {
                continue;
            };

            let mut area = fill_data.clone();

            if inflate != 0 {
                exact_polys.append_set(&area);
            }

            area.inflate(inflate, CornerStrategy::ChamferAllCorners, max_error, false);
            a_result.append_set(&area);
        }
    }

    // Merge all polygons
    a_result.simplify();

    if inflate != 0 {
        a_result.deflate(inflate, CornerStrategy::ChamferAllCorners, max_error);
        // Add back in the exact polys. This is mandatory because inflate/deflate transform is
        // not perfect, and we want the initial areas perfectly kept.
        a_result.boolean_add(&exact_polys);
    }
}
