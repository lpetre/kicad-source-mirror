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

//! Port of `pcbnew/plot_brditems_plotter.cpp`: `PlotBoardGraphicItem`, `PlotFootprintGraphicItems`,
//! `PlotShape`, `PlotDimension`, `PlotPcbTarget`, `PlotBarCode`, `PlotTableBorders`
//! (L489-783, L960-1406).
//!
//! Work package FAB-05 (`docs/analysis/plot-fab.md` 4.10, 10.2). The methods belong to
//! [`BrditemsPlotter`] (an `impl` block of the struct defined in `plot_brditems_plotter.rs`).
//! Text (`PlotText`, called here for text boxes, table cells and dimensions) is FAB-06.

use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::layer_ids::is_external_copper_layer;
use kicad_core::plot_enums::{FillT, PlotFormat, PlotTextMode};
use kicad_core::{KicadT, LineStyle, Lset, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::{
    Box2I, ShapeLineChain, ShapePolySet, ShapeRect, ShapeRef, ShapeSegment, Vector2D, Vector2I,
};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::eda_shape::shapes::{make_effective_shapes_for_stroking, shortened_bezier_polyline};
use kicad_pcb::eda_shape::{EdaShape, hatching_data};
use kicad_pcb::ids::FootprintId;
use kicad_pcb::item_ref::{AnyItem, ItemRef};
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::stroke_params::{StrokePatternLengths, stroke};
use kicad_pcb::traits::EdaShapeItem;
use kicad_pcb::{MutEnv, ParentRef};
use kicad_plot::PlotData;
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;

use crate::line_ending_plot::line_ending_plot;
use crate::plot_brditems_plotter::{BrditemsPlotter, fab_port_pending};

/// `plotMetadata( m_plotter, gbr_metadata, this )` (the `getMetadata()` macro): the Gerber
/// metadata for a Gerber plotter, nothing for SVG. (DXF, which receives the plot options,
/// is out of scope and never reaches this file.)
fn get_metadata<'m>(plot_format: PlotFormat, gbr_metadata: &'m GbrMetadata) -> PlotData<'m> {
    if plot_format == PlotFormat::Gerber {
        Some(gbr_metadata)
    } else {
        None
    }
}

/// `lineStyle <= LINE_STYLE::FIRST_TYPE` (`DEFAULT` and `SOLID`).
fn is_solid_style(line_style: LineStyle) -> bool {
    (line_style as i32) <= (LineStyle::Solid as i32)
}

/// The three pattern lengths `STROKE_PARAMS::Stroke` reads from `m_plotter->RenderSettings()`
/// for `a_width`. `StartPlotBoard` always installs render settings before items are plotted.
fn stroke_lengths(plotter: &dyn kicad_plot::Plotter, a_width: i32) -> StrokePatternLengths {
    let rs = plotter
        .render_settings()
        .expect("STROKE_PARAMS::Stroke: the plotter has render settings");
    StrokePatternLengths {
        dash: rs.dash_length(a_width),
        dot: rs.dot_length(a_width),
        gap: rs.gap_length(a_width),
    }
}

impl BrditemsPlotter<'_, '_> {
    /// Port of `BRDITEMS_PLOTTER::PlotBoardGraphicItem`: plot a board-level drawing (the
    /// callee checks the layer).
    pub fn plot_board_graphic_item(&mut self, item: ItemRef<'_>) {
        let ctx = item.ctx;

        match item.type_() {
            KicadT::PcbShapeT => self.plot_shape(item),

            KicadT::PcbTextT => {
                let text = item.as_eda_text().expect("PCB_TEXT");
                self.plot_text(
                    item,
                    item.layer(),
                    item.as_dyn().is_knockout(),
                    text.font_metrics(ctx),
                    false,
                );
            }

            KicadT::PcbTextboxT => {
                self.plotter.set_text_mode(PlotTextMode::Stroke);

                let textbox = item.as_textbox().expect("PCB_TEXTBOX");
                let text = item.as_eda_text().expect("PCB_TEXTBOX");
                self.plot_text(
                    item,
                    item.layer(),
                    item.as_dyn().is_knockout(),
                    text.font_metrics(ctx),
                    false,
                );

                if textbox.is_border_enabled() {
                    // PlotShape works only for textbox rotated by 0 or 180 degrees.
                    // This is not always the case so use the polygonal shape of the box of
                    // the text box.
                    let dummy = textbox.polygonal_box_shape(ctx);
                    self.plot_shape(ctx.view(AnyItem::Shape(&dummy)));
                }

                self.plotter.set_text_mode(self.opts.text_mode);
            }

            KicadT::PcbBarcodeT => self.plot_bar_code(item),

            KicadT::PcbTableT | KicadT::PcbDrillChartT | KicadT::PcbStackupTableT => {
                self.plotter.set_text_mode(PlotTextMode::Stroke);

                self.plot_table_cells(item);

                self.plot_table_borders(item);

                if item.type_() == KicadT::PcbDrillChartT {
                    self.plot_chart_symbols(item);
                }

                self.plotter.set_text_mode(self.opts.text_mode);
            }

            KicadT::PcbDimAlignedT
            | KicadT::PcbDimCenterT
            | KicadT::PcbDimRadialT
            | KicadT::PcbDimOrthogonalT
            | KicadT::PcbDimLeaderT => {
                self.plotter.set_text_mode(PlotTextMode::Stroke);

                self.plot_dimension(item);

                self.plotter.set_text_mode(self.opts.text_mode);
            }

            KicadT::PcbTargetT => self.plot_pcb_target(item),

            _ => {}
        }
    }

    /// `for( const PCB_TABLECELL* cell : table->GetCells() ) PlotText( cell, ... )`.
    fn plot_table_cells(&mut self, a_table: ItemRef<'_>) {
        let ctx = a_table.ctx;
        let table = a_table.as_table().expect("PCB_TABLE");

        for &cell_id in &table.cells {
            let Some(cell) = ctx.item(cell_id.into()) else {
                continue;
            };
            let text = cell.as_eda_text().expect("PCB_TABLECELL");
            self.plot_text(
                cell,
                cell.layer(),
                cell.as_dyn().is_knockout(),
                text.font_metrics(ctx),
                false,
            );
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotDimension`: the text, then every segment and circle of
    /// `GetShapes()` as a parentless solid `PCB_SHAPE` of the dimension's line thickness.
    pub fn plot_dimension(&mut self, a_dim: ItemRef<'_>) {
        let ctx = a_dim.ctx;

        if !self.layer_mask.contains(a_dim.layer()) {
            return;
        }

        let dim = a_dim.as_dimension().expect("PCB_DIMENSION_BASE");
        let color = self.color_settings_color(a_dim.layer().0);

        // Set plot color (change WHITE to LIGHTGRAY because
        // the white items are not seen on a white paper or screen
        self.plotter.set_color(&if color != Color4D::WHITE {
            color
        } else {
            Color4D::from_eda_color(EdaColorT::LIGHTGRAY)
        });

        let text = a_dim.as_eda_text().expect("PCB_DIMENSION_BASE");
        self.plot_text(a_dim, a_dim.layer(), false, text.font_metrics(ctx), false);

        let env = MutEnv::detached();
        let mut temp_item = PcbShape::default();

        temp_item.set_stroke(
            &env,
            StrokeParams::new(dim.line_thickness, LineStyle::Solid, Color4D::UNSPECIFIED),
        );
        temp_item.set_layer(&env, a_dim.layer());

        for shape in dim.shapes() {
            match shape.as_shape_ref() {
                ShapeRef::Segment(segment) => {
                    let seg = segment.get_seg();

                    temp_item.set_shape(&env, ShapeT::Segment);
                    temp_item.set_start(&env, seg.a);
                    temp_item.set_end(&env, seg.b);

                    self.plot_shape(ctx.view(AnyItem::Shape(&temp_item)));
                }

                ShapeRef::Circle(circle) => {
                    let start = circle.get_center();
                    let radius = circle.get_radius();

                    temp_item.set_shape(&env, ShapeT::Circle);
                    temp_item.shape.set_filled(false);
                    temp_item.set_start(&env, start);
                    temp_item.set_end(&env, Vector2I::new(start.x + radius, start.y));

                    self.plot_shape(ctx.view(AnyItem::Shape(&temp_item)));
                }

                _ => {}
            }
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotPcbTarget`: a circle (radius `size / 3`, `size / 2` for
    /// the X shape) and two segments (+ or X), as parentless solid `PCB_SHAPE`s.
    pub fn plot_pcb_target(&mut self, a_mire: ItemRef<'_>) {
        let ctx = a_mire.ctx;
        let AnyItem::Target(mire) = a_mire.any() else {
            panic!("PlotPcbTarget: a PCB_TARGET");
        };

        if !self.layer_mask.contains(a_mire.layer()) {
            return;
        }

        let color = self.get_color(a_mire.layer().0);
        self.plotter.set_color(&color);

        let env = MutEnv::detached();
        let mut temp_item = PcbShape::default();

        temp_item.set_shape(&env, ShapeT::Circle);
        temp_item.shape.set_filled(false);
        temp_item.set_stroke(
            &env,
            StrokeParams::new(mire.width(), LineStyle::Solid, Color4D::UNSPECIFIED),
        );
        temp_item.set_layer(&env, a_mire.layer());
        temp_item.set_start(&env, mire.position(ctx));
        let mut radius = mire.size() / 3;

        if mire.shape() != 0 {
            // temp_item X
            radius = mire.size() / 2;
        }

        // Draw the circle
        let start = temp_item.shape.start();
        temp_item.set_end(&env, Vector2I::new(start.x + radius, start.y));

        self.plot_shape(ctx.view(AnyItem::Shape(&temp_item)));

        temp_item.set_shape(&env, ShapeT::Segment);

        radius = mire.size() / 2;
        let mut dx1 = radius;
        let mut dy1 = 0;
        let mut dx2 = 0;
        let mut dy2 = radius;

        if mire.shape() != 0 {
            // Shape X
            dx1 = radius;
            dy1 = radius;
            dx2 = dx1;
            dy2 = -dy1;
        }

        let mire_pos = mire.position(ctx);

        // Draw the X or + temp_item:
        temp_item.set_start(&env, Vector2I::new(mire_pos.x - dx1, mire_pos.y - dy1));
        temp_item.set_end(&env, Vector2I::new(mire_pos.x + dx1, mire_pos.y + dy1));
        self.plot_shape(ctx.view(AnyItem::Shape(&temp_item)));

        temp_item.set_start(&env, Vector2I::new(mire_pos.x - dx2, mire_pos.y - dy2));
        temp_item.set_end(&env, Vector2I::new(mire_pos.x + dx2, mire_pos.y + dy2));
        self.plot_shape(ctx.view(AnyItem::Shape(&temp_item)));
    }

    /// Port of `BRDITEMS_PLOTTER::PlotFootprintGraphicItems`: the footprint's
    /// `GraphicalItems()` (shapes, text boxes, barcodes, tables, dimensions) that are not on
    /// a private layer, not hidden as DNP and on a plotted layer. Footprint texts are
    /// plotted by `PlotFootprintTextItems`, reference images never.
    pub fn plot_footprint_graphic_items(&mut self, a_footprint: FootprintId) {
        let ctx = self.ctx;
        let board = ctx.board;
        let Some(footprint) = board.footprint(a_footprint) else {
            return;
        };
        let variant_name = board.get_current_variant().to_string();
        let dnp = footprint.dnp_for_variant(&variant_name);

        for (id, _) in board.footprint_drawings(a_footprint) {
            let Some(item) = ctx.item(id) else {
                continue;
            };
            let item_layer = item.layer();

            if footprint.private_layers.contains(item_layer) {
                continue;
            }

            if dnp && self.hide_dnp_items(item_layer) {
                continue;
            }

            if !(&self.layer_mask & &item.layer_set()).any() {
                continue;
            }

            // m_plotter->MarkFootprintPlotted( aFootprint ): PDF only (out of scope).

            match item.type_() {
                KicadT::PcbShapeT => self.plot_shape(item),

                KicadT::PcbTextboxT => {
                    let textbox = item.as_textbox().expect("PCB_TEXTBOX");
                    let text = item.as_eda_text().expect("PCB_TEXTBOX");

                    self.plotter.set_text_mode(PlotTextMode::Stroke);

                    self.plot_text(
                        item,
                        item.layer(),
                        item.as_dyn().is_knockout(),
                        text.font_metrics(ctx),
                        false,
                    );

                    if textbox.is_border_enabled() {
                        // PlotShape works only for textbox rotated by 0 or 180 degrees.
                        // This is not always the case so use the polygonal shape
                        let dummy = textbox.polygonal_box_shape(ctx);
                        self.plot_shape(ctx.view(AnyItem::Shape(&dummy)));
                    }

                    self.plotter.set_text_mode(self.opts.text_mode);
                }

                KicadT::PcbBarcodeT => self.plot_bar_code(item),

                KicadT::PcbTableT | KicadT::PcbDrillChartT | KicadT::PcbStackupTableT => {
                    self.plotter.set_text_mode(PlotTextMode::Stroke);

                    self.plot_table_cells(item);

                    self.plot_table_borders(item);

                    self.plotter.set_text_mode(self.opts.text_mode);
                }

                KicadT::PcbDimAlignedT
                | KicadT::PcbDimCenterT
                | KicadT::PcbDimRadialT
                | KicadT::PcbDimOrthogonalT
                | KicadT::PcbDimLeaderT => self.plot_dimension(item),

                KicadT::PcbTextT => {
                    // Plotted in PlotFootprintTextItems()
                }

                KicadT::PcbReferenceImageT => {
                    // Not plotted at all
                }

                _ => {
                    // UNIMPLEMENTED_FOR( item->GetClass() ): a debug message only.
                }
            }
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotShape`: plot a `PCB_SHAPE` (board drawing, footprint
    /// graphic, or a temporary shape of a dimension / target / barcode / text box border) with
    /// its Gerber attributes: solid strokes per shape type, dashed strokes through
    /// `STROKE_PARAMS::Stroke`, line endings, hatched fills; mask expansion on mask layers.
    pub fn plot_shape(&mut self, a_shape: ItemRef<'_>) {
        let ctx = a_shape.ctx;
        let shape_item = a_shape.as_shape().expect("PlotShape: a PCB_SHAPE");
        let host: &dyn EdaShapeItem = a_shape.as_eda_shape().expect("PlotShape: an EDA_SHAPE");
        let es: &EdaShape = &shape_item.shape;
        let plot_format = self.plotter.plotter_type();

        if !(&self.layer_mask & &a_shape.layer_set()).any() {
            return;
        }

        if plot_format == PlotFormat::Dxf {
            fab_port_pending!("BRDITEMS_PLOTTER::PlotShape: DXF (out of scope)")
        }

        let shape_layer = a_shape.layer();
        let mut thickness = host.width(ctx);
        let margin = thickness; // unclamped thickness (can be negative)
        let mut margin = margin;
        let line_style = es.stroke.line_style();
        let on_copper_layer = (Lset::all_cu_mask() & &self.layer_mask).any();
        let on_solder_mask_layer =
            (Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]) & &self.layer_mask).any();
        let mut is_solid_fill = es.is_solid_fill();
        let mut is_hatched_fill = es.is_hatched_fill();

        if on_solder_mask_layer
            && shape_item.has_solder_mask()
            && is_external_copper_layer(shape_layer)
        {
            margin += 2 * shape_item.solder_mask_expansion(ctx);
            thickness = margin.max(0);

            if is_hatched_fill {
                is_solid_fill = true;
                is_hatched_fill = false;
            }
        }

        let color = self.get_color(shape_layer.0);
        self.plotter.set_color(&color);

        let parent_fp = a_shape.parent_footprint().and_then(|fp| fp.as_footprint());
        let mut gbr_metadata = GbrMetadata::new();
        let variant_name = ctx.board.get_current_variant().to_string();
        let parent_dnp = parent_fp.is_some_and(|fp| fp.dnp_for_variant(&variant_name));

        if let Some(fp) = parent_fp {
            gbr_metadata.set_cmp_reference(&fp.reference(ctx));
            gbr_metadata.set_net_attrib_type(gbr_netinfo::CMP);
        }

        if parent_fp.is_some()
            && parent_dnp
            && self.opts.sketch_dnp_fps_on_fab_layers
            && (shape_layer == PcbLayerId::FFab || shape_layer == PcbLayerId::BFab)
        {
            thickness = self.opts.sketch_pad_line_width;
            is_solid_fill = false;
            is_hatched_fill = false;
        }

        if shape_layer == PcbLayerId::EdgeCuts {
            gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Edgecut);
        } else if on_copper_layer {
            if parent_fp.is_some() {
                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Etchedcmp);
                gbr_metadata.set_copper(true);
            } else if a_shape.net_code().is_some_and(|n| n.0 > 0) {
                gbr_metadata.set_copper(true);
                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Conductor);
                gbr_metadata.set_net_attrib_type(gbr_netinfo::NET);
                gbr_metadata.set_net_name(a_shape.netname());
            } else {
                // Graphic items (PCB_SHAPE, TEXT) having no net have the NonConductor
                // attribute. Graphic items having a net have the Conductor attribute, but are
                // not (yet?) supported in Pcbnew
                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Nonconductor);
            }
        }

        let max_error = host.shape_max_error(ctx);

        if is_solid_style(line_style) {
            match es.shape() {
                ShapeT::Segment => {
                    let mut seg_start = es.start();
                    let mut seg_end = es.end();

                    if EdaShape::shorten_segment_for_endings(
                        &mut seg_start,
                        &mut seg_end,
                        &es.start_ending(),
                        &es.end_ending(),
                        thickness,
                    ) {
                        self.plotter.thick_segment(
                            seg_start,
                            seg_end,
                            thickness,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    }
                }

                ShapeT::Circle => {
                    if is_solid_fill {
                        let mut diameter = es.radius() * 2 + thickness;

                        if margin < 0 {
                            diameter += margin;
                            diameter = diameter.max(0);
                        }

                        self.plotter.filled_circle(
                            es.start(),
                            diameter,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    } else {
                        self.plotter.thick_circle(
                            es.start(),
                            es.radius() * 2,
                            thickness,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    }
                }

                ShapeT::Arc => {
                    // when startAngle == endAngle ThickArc() doesn't know whether it's 0 deg
                    // and 360 deg but it is a circle
                    if es.arc_angle().as_degrees().abs() == 360.0 {
                        self.plotter.thick_circle(
                            es.center(),
                            es.radius() * 2,
                            thickness,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    } else {
                        let (mut start_angle, end_angle) = es.calc_arc_angles();

                        let mut arc_angle = end_angle - start_angle;

                        if es.shorten_arc_for_endings(
                            &mut start_angle,
                            &mut arc_angle,
                            es.radius() as f64,
                            thickness,
                        ) {
                            self.plotter.thick_arc(
                                Vector2D::from_vec(es.center()),
                                start_angle,
                                arc_angle,
                                es.radius() as f64,
                                thickness,
                                get_metadata(plot_format, &gbr_metadata),
                            );
                        }
                    }
                }

                ShapeT::Bezier => {
                    if let Some(curve) = es.shortened_bezier_curve(thickness) {
                        if plot_format == PlotFormat::Svg {
                            self.plotter.bezier_curve(
                                Vector2I::from_vec(curve.start),
                                Vector2I::from_vec(curve.c1),
                                Vector2I::from_vec(curve.c2),
                                Vector2I::from_vec(curve.end),
                                max_error,
                                thickness,
                            );
                        } else {
                            let pts = shortened_bezier_polyline(host, ctx, thickness);

                            let mut i = 0;

                            while i + 1 < pts.len() {
                                self.plotter.thick_segment(
                                    Vector2I::from_vec(pts[i]),
                                    Vector2I::from_vec(pts[i + 1]),
                                    thickness,
                                    get_metadata(plot_format, &gbr_metadata),
                                );
                                i += 1;
                            }
                        }
                    }
                }

                ShapeT::Poly => {
                    if es.is_poly_shape_valid() {
                        // (The DXF sketch branch, ThickPoly, is out of scope.)
                        self.plotter
                            .set_current_line_width(thickness, Some(&gbr_metadata));

                        let orig_poly = es.poly_shape();

                        // Stroke the unfractured outline so degenerate near-zero-width spikes
                        // (which Fracture() collapses but the editor still draws as thick
                        // lines) are preserved in the plot output (issue #24143).
                        if thickness > 0 {
                            for jj in 0..orig_poly.outline_count() {
                                self.plot_stroke_outline(
                                    es,
                                    orig_poly.c_outline(jj),
                                    jj,
                                    thickness,
                                    get_metadata(plot_format, &gbr_metadata),
                                );
                            }

                            thickness = 0;
                        }

                        if is_solid_fill {
                            // Fracture before plotting the fill to avoid invalid gerber
                            // regions generated from self-intersecting or overlapping
                            // outlines.
                            let mut tmp_poly = orig_poly.clone_drop_triangulation();

                            tmp_poly.fracture(true);

                            if margin < 0 {
                                tmp_poly.inflate(
                                    margin / 2,
                                    CornerStrategy::RoundAllCorners,
                                    max_error,
                                    false,
                                );
                            }

                            for jj in 0..tmp_poly.outline_count() {
                                let poly = tmp_poly.outline(jj);
                                poly.set_closed(true);

                                // Width 0; the stroke was already plotted from the unfractured
                                // outline.
                                if plot_format == PlotFormat::Gerber {
                                    self.plotter
                                        .as_gerber_mut()
                                        .expect("a Gerber plotter")
                                        .plot_poly_as_region(
                                            poly,
                                            FillT::FilledShape,
                                            thickness,
                                            Some(&gbr_metadata),
                                        );
                                } else {
                                    self.plotter.plot_poly_chain(
                                        poly,
                                        FillT::FilledShape,
                                        thickness,
                                        get_metadata(plot_format, &gbr_metadata),
                                    );
                                }
                            }
                        }
                    }
                }

                ShapeT::Rectangle => {
                    let mut radius = es.corner_radius();

                    // (The DXF sketch branch, ThickRect, is out of scope.)
                    let mut bbox = Box2I::new(
                        es.start(),
                        Vector2I::new(es.end().x - es.start().x, es.end().y - es.start().y),
                    );
                    bbox.normalize();

                    if margin < 0 {
                        bbox.inflate(margin);
                        radius += margin;
                    }

                    let mut rect = ShapeRect::from_box(&bbox);
                    rect.set_radius(radius);

                    let outline = rect.outline();
                    let poly = ShapePolySet::from_outline(&outline);

                    let fill_mode = if is_solid_fill {
                        FillT::FilledShape
                    } else {
                        FillT::NoFill
                    };

                    if poly.outline_count() > 0 {
                        if plot_format == PlotFormat::Gerber {
                            self.plotter
                                .as_gerber_mut()
                                .expect("a Gerber plotter")
                                .plot_poly_as_region(
                                    poly.c_outline(0),
                                    fill_mode,
                                    thickness,
                                    Some(&gbr_metadata),
                                );
                        } else {
                            // TODO: PlotPoly needs to handle arcs...
                            self.plotter.plot_poly_chain(
                                poly.c_outline(0),
                                fill_mode,
                                thickness,
                                get_metadata(plot_format, &gbr_metadata),
                            );
                        }
                    }
                }

                ShapeT::Ellipse | ShapeT::EllipseArc => {
                    let is_closed = es.shape() == ShapeT::Ellipse;

                    let e = es.build_shape_ellipse();

                    let mut chain: ShapeLineChain = e.convert_to_polyline(max_error);

                    if is_closed {
                        chain.set_closed(true);
                    }

                    let fill = if is_closed && is_solid_fill {
                        FillT::FilledShape
                    } else {
                        FillT::NoFill
                    };

                    if plot_format == PlotFormat::Gerber && is_closed {
                        self.plotter
                            .as_gerber_mut()
                            .expect("a Gerber plotter")
                            .plot_poly_as_region(&chain, fill, thickness, Some(&gbr_metadata));
                    } else {
                        self.plotter.plot_poly_chain(
                            &chain,
                            fill,
                            thickness,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    }
                }

                _ => {
                    // UNIMPLEMENTED_FOR( aShape->SHAPE_T_asString() ): a debug message only.
                }
            }
        } else {
            let shapes = make_effective_shapes_for_stroking(host, ctx, thickness);
            let width = host.width(ctx);
            let lengths = stroke_lengths(&*self.plotter, width);

            for shape in &shapes {
                let plotter = &mut *self.plotter;
                stroke(&**shape, line_style, width, &lengths, &mut |a, b| {
                    plotter.thick_segment(
                        a,
                        b,
                        thickness,
                        get_metadata(plot_format, &gbr_metadata),
                    );
                });
            }
        }

        // Plot line endings for open shapes.
        let start_ending = es.start_ending();
        let end_ending = es.end_ending();

        if start_ending.style() != kicad_pcb::line_ending::LineEndingStyle::None
            || end_ending.style() != kicad_pcb::line_ending::LineEndingStyle::None
        {
            let (start_tangent, end_tangent) = es.ending_tangents(thickness);

            if let Some((start_pt, end_pt)) = es.line_ending_endpoints() {
                line_ending_plot(
                    &start_ending,
                    &mut *self.plotter,
                    start_pt,
                    start_tangent,
                    thickness,
                    get_metadata(plot_format, &gbr_metadata),
                );
                line_ending_plot(
                    &end_ending,
                    &mut *self.plotter,
                    end_pt,
                    end_tangent,
                    thickness,
                    get_metadata(plot_format, &gbr_metadata),
                );
            }
        }

        if is_hatched_fill {
            let hatch = hatching_data(host, ctx);

            for ii in 0..hatch.hatching.outline_count() {
                if plot_format == PlotFormat::Gerber {
                    self.plotter
                        .as_gerber_mut()
                        .expect("a Gerber plotter")
                        .plot_poly_as_region(
                            hatch.hatching.c_outline(ii),
                            FillT::FilledShape,
                            0,
                            Some(&gbr_metadata),
                        );
                } else {
                    self.plotter.plot_poly_chain(
                        hatch.hatching.c_outline(ii),
                        FillT::FilledShape,
                        0,
                        get_metadata(plot_format, &gbr_metadata),
                    );
                }
            }
        }
    }

    /// The `plotStrokeOutline` lambda of `PlotShape` (polygons): stroke one outline of the
    /// unfractured polygon, shortened for the line endings when they need it.
    fn plot_stroke_outline(
        &mut self,
        es: &EdaShape,
        a_outline: &ShapeLineChain,
        a_outline_idx: i32,
        thickness: i32,
        a_data: PlotData,
    ) {
        if a_outline.point_count() < 2 {
            return;
        }

        if es.start_ending().shorten_depth(thickness) > 0
            || es.end_ending().shorten_depth(thickness) > 0
        {
            let Some(pts) = es.shortened_body_poly_points(a_outline, a_outline_idx, thickness)
            else {
                return;
            };

            let mut shortened = ShapeLineChain::new();

            for pt in &pts {
                shortened.append_point(*pt, false);
            }

            shortened.set_closed(a_outline.is_closed());
            self.plotter
                .plot_poly_chain(&shortened, FillT::NoFill, thickness, a_data);
        } else {
            self.plotter
                .plot_poly_chain(a_outline, FillT::NoFill, thickness, a_data);
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotBarCode`: the barcode's polygon
    /// (`TransformShapeToPolySet( ..., 0, 0, ERROR_INSIDE )`) as a filled, zero-width
    /// polygon `PCB_SHAPE` with the barcode's parent.
    pub fn plot_bar_code(&mut self, a_bar_code: ItemRef<'_>) {
        let ctx = a_bar_code.ctx;

        if !self.layer_mask.contains(a_bar_code.layer()) {
            return;
        }

        // To avoid duplicate code, build a PCB_SHAPE to plot the polygon shape
        let parent: ParentRef = a_bar_code.base().parent;
        let env = ctx.board.child_env(parent);
        let mut dummy = PcbShape::with_shape(ShapeT::Poly);
        dummy.base.item.parent = parent;
        dummy.set_layer(&env, a_bar_code.layer());
        dummy.shape.set_fill_mode(kicad_pcb::eda_shape::FillT::FilledShape);
        dummy.set_width(&env, 0);

        let mut shape = ShapePolySet::new();
        a_bar_code.as_dyn().transform_shape_to_poly_set(
            ctx,
            &mut shape,
            a_bar_code.layer(),
            0,
            0,
            ErrorLoc::Inside,
        );
        dummy.set_poly_shape(&env, shape);

        self.plot_shape(ctx.view(AnyItem::Shape(&dummy)));
    }

    /// Port of `BRDITEMS_PLOTTER::PlotTableBorders`: the borders and separators of
    /// `PCB_TABLE::DrawBorders` as thick segments (dashed styles through
    /// `STROKE_PARAMS::Stroke`), with the component attribute for footprint tables.
    pub fn plot_table_borders(&mut self, a_table: ItemRef<'_>) {
        let ctx = a_table.ctx;
        let table = a_table.as_table().expect("PlotTableBorders: a PCB_TABLE");

        if !self.layer_mask.contains(a_table.layer()) {
            return;
        }

        let mut gbr_metadata = GbrMetadata::new();

        if let Some(parent_fp) = a_table.parent_footprint().and_then(|fp| fp.as_footprint()) {
            gbr_metadata.set_cmp_reference(&parent_fp.reference(ctx));
            gbr_metadata.set_net_attrib_type(gbr_netinfo::CMP);
        }

        let plot_format = self.plotter.plotter_type();
        let plotter = &mut *self.plotter;

        table.draw_borders(ctx, &mut |pt_a, pt_b, stroke_params| {
            let line_width = stroke_params.width();
            let line_style = stroke_params.line_style();

            if is_solid_style(line_style) {
                plotter.thick_segment(
                    pt_a,
                    pt_b,
                    line_width,
                    get_metadata(plot_format, &gbr_metadata),
                );
            } else {
                let seg = ShapeSegment::new(pt_a, pt_b, 0);
                let lengths = stroke_lengths(&*plotter, line_width);

                stroke(&seg, line_style, line_width, &lengths, &mut |a, b| {
                    plotter.thick_segment(
                        a,
                        b,
                        line_width,
                        get_metadata(plot_format, &gbr_metadata),
                    );
                });
            }
        });
    }
}
