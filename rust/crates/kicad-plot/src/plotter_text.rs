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

//! Port of `PLOTTER::Text` and `PLOTTER::PlotText` (`common/plotters/plotter.cpp` L548-667)
//! with the `CALLBACK_GAL` sink they create (`common/callback_gal.cpp`).
//!
//! The text layout (multi-line splitting, justification, rotation, mirroring, italic, bold,
//! overbar, sub/superscript, KiROUND interline) is done by `kicad-font`'s `FONT::Draw`; the
//! plotter only turns each stroke segment (its points truncated to `VECTOR2I`, as in
//! `CALLBACK_GAL::DrawGlyph`) into `MoveTo` / `LineTo` / `PenFinish`, and each outline glyph
//! into a filled `PlotPoly`.
//!
//! Not ported: the `@{...}` expression evaluation (`EXPRESSION_EVALUATOR`), text is plotted
//! unevaluated (see `PORT_STATUS.md`). Outline (TrueType) fonts are not available in this
//! port: a render-settings default font naming one falls back to the stroke font and is
//! recorded in [`PlotterState::outline_font_fallbacks`](crate::PlotterState).

use kicad_core::color4d::Color4D;
use kicad_core::plot_enums::FillT;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::callback_gal::to_vector2i;
use kicad_font::gr_text::get_pen_size_for_bold;
use kicad_font::{Font, FontExt, Glyph, Metrics, TextGal, get_font};
use kicad_geom::{EdaAngle, Vector2I};

use crate::plotter::{PlotData, Plotter};

/// The `CALLBACK_GAL` of `PLOTTER::Text` / `PlotText`: the two C++ lambdas capture the plotter
/// by reference; here the sink holds it and dispatches in `draw_glyph`.
struct PlotterGal<'p, 'd, P: Plotter + ?Sized> {
    plotter: &'p mut P,
    /// `Some( aPenWidth )` for `PLOTTER::Text`, whose stroke callback calls
    /// `SetCurrentLineWidth( aPenWidth )` (without metadata) before every segment; `None` for
    /// `PlotText`, which selects the pen once before drawing.
    stroke_pen_width: Option<i32>,
    /// `aData`, given to the outline (polygon) callback.
    data: PlotData<'d>,
    /// `GAL::m_lineWidth` (default 1.0)
    line_width: f32,
}

impl<P: Plotter + ?Sized> TextGal for PlotterGal<'_, '_, P> {
    fn set_line_width(&mut self, a_line_width: f32) {
        self.line_width = a_line_width;
    }

    fn line_width(&self) -> f32 {
        self.line_width
    }

    /// Port of `CALLBACK_GAL::DrawGlyph` (stroke mode, no triangulation) with the callbacks of
    /// `PLOTTER::Text` / `PlotText`.
    fn draw_glyph(&mut self, a_glyph: &Glyph, _a_nth: i32, _a_total: i32) {
        match a_glyph {
            Glyph::Stroke(glyph) => {
                for point_list in &glyph.strokes {
                    for ii in 1..point_list.len() {
                        // const VECTOR2I& parameters of the callback: implicit truncation
                        let pt1 = to_vector2i(point_list[ii - 1]);
                        let pt2 = to_vector2i(point_list[ii]);

                        if let Some(pen_width) = self.stroke_pen_width {
                            self.plotter.set_current_line_width(pen_width, None);
                        }

                        self.plotter.move_to(pt1);
                        self.plotter.line_to(pt2);
                        self.plotter.pen_finish();
                    }
                }
            }
            Glyph::Outline(glyph) => {
                // OUTLINE_GLYPH glyph = aGlyph (a copy), fractured when it has holes
                let mut poly = glyph.poly.clone();

                if poly.has_holes() {
                    poly.fracture(true);
                }

                for ii in 0..poly.outline_count() {
                    let outline = poly.c_outline(ii).clone();
                    self.plotter
                        .plot_poly_chain(&outline, FillT::FilledShape, 0, self.data);
                }
            }
        }
    }
}

/// `if( !aFont ) aFont = KIFONT::FONT::GetFont( m_renderSettings->GetDefaultFont() );`
///
/// An outline font name cannot be honoured (no outline font engine): the stroke font is used,
/// as C++ does when `OUTLINE_FONT::LoadFont` fails, and the name is recorded in
/// `PlotterState::outline_font_fallbacks`.
fn resolve_font<'f, P: Plotter + ?Sized>(p: &mut P, a_font: Option<&'f dyn Font>) -> &'f dyn Font {
    if let Some(font) = a_font {
        return font;
    }

    let name = p
        .state()
        .render_settings
        .as_ref()
        .map(|rs| rs.default_font().to_string())
        .unwrap_or_default();
    let lookup = get_font(&name, false, false, None, false);

    if lookup.is_outline_unavailable() {
        p.state_mut().outline_font_fallbacks.push(name);
    }

    lookup.font()
}

/// Port of the base implementation of `PLOTTER::Text`.
#[allow(clippy::too_many_arguments)]
pub fn text_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_pos: Vector2I,
    a_color: &Color4D,
    a_text: &str,
    a_orient: EdaAngle,
    a_size: Vector2I,
    a_h_justify: GrTextHAlignT,
    a_v_justify: GrTextVAlignT,
    a_pen_width: i32,
    a_italic: bool,
    a_bold: bool,
    _a_multiline_allowed: bool,
    a_font: Option<&dyn Font>,
    a_font_metrics: &Metrics,
    a_data: PlotData,
) {
    // `@{...}` expressions are not evaluated (EXPRESSION_EVALUATOR is not ported).
    let text = a_text;
    let mut a_pen_width = a_pen_width;

    p.set_color(a_color);

    if a_pen_width == 0 && a_bold {
        // Use default values if aPenWidth == 0
        a_pen_width = get_pen_size_for_bold(a_size.x.min(a_size.y));
    }

    if a_pen_width < 0 {
        a_pen_width = a_pen_width.wrapping_neg();
    }

    let mut attributes = TextAttributes::default();
    attributes.set_angle(a_orient.as_degrees());
    attributes.stroke_width = a_pen_width;
    attributes.italic = a_italic;
    attributes.bold = a_bold;
    attributes.halign = a_h_justify;
    attributes.valign = a_v_justify;
    attributes.size = (a_size.x, a_size.y);

    // if Size.x is < 0, the text is mirrored (we have no other param to know a text is
    // mirrored)
    if attributes.size.0 < 0 {
        attributes.size.0 = attributes.size.0.wrapping_neg();
        attributes.mirrored = true;
    }

    let font = resolve_font(p, a_font);

    let mut gal = PlotterGal {
        plotter: p,
        stroke_pen_width: Some(a_pen_width),
        data: a_data,
        line_width: 1.0,
    };

    font.draw(&mut gal, text, a_pos, &attributes, a_font_metrics);
}

/// Port of the base implementation of `PLOTTER::PlotText`.
#[allow(clippy::too_many_arguments)]
pub fn plot_text_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_pos: Vector2I,
    a_color: &Color4D,
    a_text: &str,
    a_attributes: &TextAttributes,
    a_font: Option<&dyn Font>,
    a_font_metrics: &Metrics,
    a_data: PlotData,
) {
    // `@{...}` expressions are not evaluated (EXPRESSION_EVALUATOR is not ported).
    let text = a_text;

    let mut attributes = a_attributes.clone();
    let mut pen_width = attributes.stroke_width;

    p.set_color(a_color);
    p.set_current_line_width(pen_width, a_data);

    if pen_width == 0 && attributes.bold {
        // Use default values if aPenWidth == 0
        pen_width = get_pen_size_for_bold(attributes.size.0.min(attributes.size.1));
    }

    if pen_width < 0 {
        pen_width = pen_width.wrapping_neg();
    }

    attributes.stroke_width = pen_width;

    let font = resolve_font(p, a_font);

    let mut gal = PlotterGal {
        plotter: p,
        stroke_pen_width: None,
        data: a_data,
        line_width: 1.0,
    };

    font.draw(&mut gal, text, a_pos, &attributes, a_font_metrics);
}
