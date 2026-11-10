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

//! Port of `pcbnew/plot_brditems_plotter.cpp`: `PlotFootprintTextItems` (L401-486) and
//! `PlotText` (L786-899).
//!
//! Work package FAB-06 (`docs/analysis/plot-fab.md` 10.2, `docs/analysis/text-font.md` 4.10
//! (c)). The methods belong to [`BrditemsPlotter`] (an `impl` block of the struct defined in
//! `plot_brditems_plotter.rs`).
//!
//! Outline fonts: this port has no outline font engine. Text whose draw font is an outline
//! font is drawn from the render cache stored in the board file, the stand-in for the glyphs
//! the C++ regenerates (`kicad_pcb::eda_text::render_cache`, package FONT-RC-1), while it is
//! valid for the text; otherwise (edited text, missing cache, or fonts embedded in the board,
//! where C++ lays out with the outline font) the stroke font is used, as the model's
//! `FontLookup` documents, and the text is listed by `eda_text::outline_font_fallbacks`. See
//! `PORT_STATUS.md` (FAB-06, deviations) and kicad-pcb `PORT_STATUS.md` (FONT-RC-1).

use kicad_core::PcbLayerId;
use kicad_core::base_units::ARC_LOW_DEF;
use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, is_copper_layer};
use kicad_core::plot_enums::{FillT, PlotFormat};
use kicad_core::string_utils::wx_string_split;
use kicad_core::text_vars::ResolutionContext;
use kicad_font::{CallbackGal, FontLookup, Metrics, TextGal, get_font};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::trigo::rotate_point_around;
use kicad_geom::{Shape, ShapeLineChain, ShapePolySet, Vector2I};
use kicad_pcb::eda_text::{EdaTextExt, render_cache};
use kicad_pcb::ids::FootprintId;
use kicad_pcb::item_ref::{AnyItem, ItemRef};
use kicad_plot::PlotData;
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};

use crate::plot_brditems_plotter::BrditemsPlotter;

/// `plotMetadata( m_plotter, gbr_metadata, this )` (the `getMetadata()` macro): the Gerber
/// metadata for a Gerber plotter, nothing for SVG (DXF is out of scope).
fn get_metadata<'m>(plot_format: PlotFormat, gbr_metadata: &'m GbrMetadata) -> PlotData<'m> {
    if plot_format == PlotFormat::Gerber {
        Some(gbr_metadata)
    } else {
        None
    }
}

/// `dynamic_cast<const PCB_TEXT*>( aText )->TransformTextToPolySet( ... )` (`PCB_TEXT`, its
/// subclasses `PCB_FIELD` and `PCB_DIMENSION_BASE`). Returns false when `a_text` is not a
/// `PCB_TEXT` (the C++ cast yields nullptr).
fn pcb_text_transform_text_to_poly_set(
    a_text: ItemRef<'_>,
    a_buffer: &mut ShapePolySet,
    a_clearance: i32,
    a_max_error: i32,
    a_error_loc: ErrorLoc,
) -> bool {
    let ctx = a_text.ctx;

    match a_text.any() {
        AnyItem::Text(text) => {
            text.transform_text_to_poly_set(ctx, a_buffer, a_clearance, a_max_error, a_error_loc);
            true
        }
        AnyItem::Field(field) => {
            field.transform_text_to_poly_set(ctx, a_buffer, a_clearance, a_max_error, a_error_loc);
            true
        }
        AnyItem::Dimension(dim) => {
            dim.transform_text_to_poly_set(ctx, a_buffer, a_clearance, a_max_error, a_error_loc);
            true
        }
        _ => false,
    }
}

/// One drawing call of the outline-font branch of `PlotText`, recorded from the
/// `CALLBACK_GAL` callbacks (they cannot borrow the plotter while the GAL is alive) and
/// replayed in the same order.
enum GlyphOp {
    /// Stroke callback: `ThickSegment( aPt1, aPt2, ... )`.
    Segment(Vector2I, Vector2I),
    /// Polygon callback: `PlotPoly( aPoly, FILLED_SHAPE, 0, ... )`.
    Poly(ShapeLineChain),
}

impl BrditemsPlotter<'_, '_> {
    /// Port of `BRDITEMS_PLOTTER::PlotFootprintTextItems`: the reference and value (with their
    /// plot options), the other visible fields and the footprint's text items (`PCB_TEXT`,
    /// including dimensions) on the plotted layers; DNP rules on the fabrication layers.
    pub fn plot_footprint_text_items(&mut self, a_footprint: FootprintId) {
        if !self.opts.plot_fp_text {
            return;
        }

        let ctx = self.ctx;
        let board = ctx.board;
        let Some(footprint) = board.footprint(a_footprint) else {
            return;
        };

        let variant_name = board.get_current_variant().to_string();
        let dnp = footprint.dnp_for_variant(&variant_name);

        // `&aFootprint->Reference()` / `&aFootprint->Value()`: the mandatory fields always
        // exist in C++; a footprint built without them has nothing to plot here.
        let reference = footprint
            .field(ctx, kicad_pcb::pcb_field::FieldT::Reference)
            .and_then(|id| ctx.item(id.into()));

        if let Some(reference) = reference {
            let ref_layer = reference.layer();
            let ref_text = reference.as_eda_text().expect("PCB_FIELD");

            // Reference and value have special controls for forcing their plotting
            if self.opts.plot_reference
                && self.layer_mask.contains(ref_layer)
                && ref_text.eda_text().is_visible()
                && !(dnp && self.hide_dnp_items(ref_layer))
            {
                // m_plotter->MarkFootprintPlotted( aFootprint ): PDF only (out of scope).
                self.plot_text(
                    reference,
                    ref_layer,
                    reference.as_dyn().is_knockout(),
                    ref_text.font_metrics(ctx),
                    dnp && self.crossout_dnp_items(ref_layer),
                );
            }
        }

        let value = footprint
            .field(ctx, kicad_pcb::pcb_field::FieldT::Value)
            .and_then(|id| ctx.item(id.into()));

        if let Some(value) = value {
            let value_layer = value.layer();
            let value_text = value.as_eda_text().expect("PCB_FIELD");

            if self.opts.plot_value
                && self.layer_mask.contains(value_layer)
                && value_text.eda_text().is_visible()
                && !(dnp && self.hide_dnp_items(value_layer))
            {
                // m_plotter->MarkFootprintPlotted( aFootprint ): PDF only (out of scope).
                self.plot_text(
                    value,
                    value_layer,
                    value.as_dyn().is_knockout(),
                    value_text.font_metrics(ctx),
                    false,
                );
            }
        }

        let mut texts: Vec<ItemRef<'_>> = Vec::new();

        // Skip the reference and value texts that are handled specially
        for (field_id, field) in board.footprint_fields(a_footprint) {
            if field.is_reference() || field.is_value() {
                continue;
            }

            if field.text.text.is_visible() {
                if let Some(item) = ctx.item(field_id.into()) {
                    texts.push(item);
                }
            }
        }

        for (id, _) in board.footprint_drawings(a_footprint) {
            let Some(item) = ctx.item(id) else {
                continue;
            };

            // dynamic_cast<PCB_TEXT*>: PCB_TEXT and its subclass PCB_DIMENSION_BASE (not
            // PCB_TEXTBOX, which is a PCB_SHAPE)
            if matches!(item.any(), AnyItem::Text(_) | AnyItem::Dimension(_)) {
                texts.push(item);
            }
        }

        for text in texts {
            let text_layer = text.layer();
            let mut strikeout = false;

            if text_layer == PcbLayerId::EdgeCuts || text_layer.0 >= PCB_LAYER_ID_COUNT {
                continue;
            }

            if dnp && self.hide_dnp_items(text_layer) {
                continue;
            }

            if !self.layer_mask.contains(text_layer)
                || footprint.private_layers.contains(text_layer)
            {
                continue;
            }

            let eda_text = text.as_eda_text().expect("PCB_TEXT");

            if eda_text.eda_text().text() == "${REFERENCE}" {
                if !self.opts.plot_reference {
                    continue;
                }

                strikeout = dnp && self.crossout_dnp_items(text_layer);
            }

            if eda_text.eda_text().text() == "${VALUE}" && !self.opts.plot_value {
                continue;
            }

            // m_plotter->MarkFootprintPlotted( aFootprint ): PDF only (out of scope).
            self.plot_text(
                text,
                text_layer,
                text.as_dyn().is_knockout(),
                eda_text.font_metrics(ctx),
                strikeout,
            );
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotText`: plot the text of `a_text` (the item owning the
    /// `EDA_TEXT`: text, field, dimension, text box or table cell) on `a_layer`: knockout text
    /// as filled polygons, outline-font text from its render cache, multi-line text line by
    /// line, and the DNP strike-out line when `a_strikeout`.
    pub fn plot_text(
        &mut self,
        a_text: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_is_knockout: bool,
        a_font_metrics: &Metrics,
        a_strikeout: bool,
    ) {
        let ctx = a_text.ctx;
        let text = a_text.as_eda_text().expect("PlotText: an EDA_TEXT");
        let et = text.eda_text();
        let max_error = ctx.board.design_settings().max_error();

        // EDA_TEXT::GetDrawFont( m_plotter->RenderSettings() )
        let font: FontLookup = if et.attributes().font.is_some() {
            et.draw_font_lookup()
        } else {
            let default_font = self
                .plotter
                .render_settings()
                .map_or(String::new(), |rs| rs.default_font().to_string());
            get_font(&default_font, et.is_bold(), et.is_italic(), None, false)
        };

        let shown_text = text.shown_text(ctx, ResolutionContext::ForCanvas, 0);

        // dynamic_cast<const PCB_TEXTBOX*>: text boxes and table cells
        let knockout_box = if a_is_knockout {
            a_text.as_textbox()
        } else {
            None
        };

        if shown_text.is_empty() && knockout_box.is_none() {
            return;
        }

        if !self.layer_mask.contains(a_layer) {
            return;
        }

        let mut gbr_metadata = GbrMetadata::new();

        if is_copper_layer(a_layer.0) {
            gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Nonconductor);
        }

        let plot_format = self.plotter.plotter_type();
        let color = self.get_color(a_layer.0);
        self.plotter.set_color(&color);

        let pos = text.text_pos(ctx);

        let mut attrs = et.attributes().clone();
        let size = text.text_size(ctx);
        attrs.size = (size.x, size.y);
        attrs.stroke_width = text.effective_text_pen_width(ctx, 0);
        attrs.set_angle(text.draw_rotation(ctx).as_degrees());
        attrs.multiline = false;

        self.plotter
            .set_current_line_width(attrs.stroke_width, None);

        if a_is_knockout {
            let mut final_poly = ShapePolySet::new();

            if knockout_box.is_some() {
                match a_text.any() {
                    AnyItem::Textbox(textbox) => textbox.transform_text_to_poly_set(
                        ctx,
                        &mut final_poly,
                        0,
                        max_error,
                        ErrorLoc::Inside,
                    ),
                    AnyItem::Tablecell(cell) => cell.transform_text_to_poly_set(
                        ctx,
                        &mut final_poly,
                        0,
                        max_error,
                        ErrorLoc::Inside,
                    ),
                    _ => unreachable!("as_textbox: a text box or a table cell"),
                }
            } else {
                pcb_text_transform_text_to_poly_set(
                    a_text,
                    &mut final_poly,
                    0,
                    max_error,
                    ErrorLoc::Inside,
                );
            }

            final_poly.fracture(true);

            for ii in 0..final_poly.outline_count() {
                self.plotter.plot_poly_chain(
                    final_poly.c_outline(ii),
                    FillT::FilledShape,
                    0,
                    get_metadata(plot_format, &gbr_metadata),
                );
            }

            return;
        }

        let cache =
            if font.is_outline_unavailable() && !ctx.board.embedded_files().are_fonts_embedded() {
                // The C++ regenerates the cache with the outline font; without an outline font
                // engine the glyphs stored in the file stand in while valid (FONT-RC-1), else
                // the stroke font below is the fallback.
                render_cache(text, ctx, &shown_text, Vector2I::default())
            } else {
                None
            };

        if let Some(cache) = cache {
            let mut ops: Vec<GlyphOp> = Vec::new();

            {
                let ops_cell = std::cell::RefCell::new(&mut ops);
                let mut callback_gal = CallbackGal::new(
                    // Stroke callback
                    |pt1, pt2| ops_cell.borrow_mut().push(GlyphOp::Segment(pt1, pt2)),
                    // Polygon callback (the non-triangulating outline branch of
                    // CALLBACK_GAL::DrawGlyph: a copy, fractured if it has holes)
                    |glyph| {
                        let mut copy = glyph.poly.clone();

                        if copy.has_holes() {
                            copy.fracture(true);
                        }

                        for ii in 0..copy.outline_count() {
                            ops_cell
                                .borrow_mut()
                                .push(GlyphOp::Poly(copy.c_outline(ii).clone()));
                        }
                    },
                );

                callback_gal.draw_glyphs(&cache.glyphs);
            }

            for op in ops {
                match op {
                    GlyphOp::Segment(pt1, pt2) => self.plotter.thick_segment(
                        pt1,
                        pt2,
                        attrs.stroke_width,
                        get_metadata(plot_format, &gbr_metadata),
                    ),
                    GlyphOp::Poly(poly) => self.plotter.plot_poly_chain(
                        &poly,
                        FillT::FilledShape,
                        0,
                        get_metadata(plot_format, &gbr_metadata),
                    ),
                }
            }
        } else if et.is_multiline_allowed() {
            let strings_list = wx_string_split(&shown_text, '\n');
            let positions = text.line_positions(ctx, strings_list.len() as i32);

            for (ii, txt) in strings_list.iter().enumerate() {
                self.plotter.plot_text(
                    positions[ii],
                    &color,
                    txt,
                    &attrs,
                    Some(font.font()),
                    a_font_metrics,
                    get_metadata(plot_format, &gbr_metadata),
                );
            }

            if a_strikeout && strings_list.len() == 1 {
                self.strikeout_text(a_text, attrs.stroke_width, &gbr_metadata);
            }
        } else {
            self.plotter.plot_text(
                pos,
                &color,
                &shown_text,
                &attrs,
                Some(font.font()),
                a_font_metrics,
                get_metadata(plot_format, &gbr_metadata),
            );

            if a_strikeout {
                self.strikeout_text(a_text, attrs.stroke_width, &gbr_metadata);
            }
        }
    }

    /// The `strikeoutText` lambda of `PlotText`: a line of the text's pen width through the
    /// middle of its (unrotated) polygon bounding box, extended by the pen width at both ends
    /// (DNP footprints on fabrication layers). `a_text` is a `PCB_TEXT` (`static_cast`).
    fn strikeout_text(&mut self, a_text: ItemRef<'_>, a_stroke_width: i32, gbr: &GbrMetadata) {
        let ctx = a_text.ctx;
        let text = a_text.as_eda_text().expect("strikeoutText: a PCB_TEXT");
        let mut text_poly = ShapePolySet::new();

        pcb_text_transform_text_to_poly_set(
            a_text,
            &mut text_poly,
            0,
            ARC_LOW_DEF,
            ErrorLoc::Inside,
        );
        text_poly.rotate(-text.draw_rotation(ctx), text.draw_pos(ctx));

        let rect = text_poly.bbox(0);
        let mut start = Vector2I::new(
            rect.left() - a_stroke_width,
            (rect.top() + rect.bottom()) / 2,
        );
        let mut end = Vector2I::new(
            rect.right() + a_stroke_width,
            (rect.top() + rect.bottom()) / 2,
        );

        rotate_point_around(&mut start, text.draw_pos(ctx), text.draw_rotation(ctx));
        rotate_point_around(&mut end, text.draw_pos(ctx), text.draw_rotation(ctx));

        let plot_format = self.plotter.plotter_type();
        self.plotter
            .thick_segment(start, end, a_stroke_width, get_metadata(plot_format, gbr));
    }
}
