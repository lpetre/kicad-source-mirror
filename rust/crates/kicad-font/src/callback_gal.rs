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

//! Port of `common/callback_gal.cpp` / `include/callback_gal.h` and of the glyph drawing part
//! of `KIGFX::GAL` (`SetLineWidth`, `GetLineWidth`, `DrawGlyph`, `DrawGlyphs`).
//!
//! [`TextGal`] replaces the `KIGFX::GAL*` the C++ font code draws through.  [`CallbackGal`]
//! is `CALLBACK_GAL` (stroke segments handed to a callback as `VECTOR2I`, converted from the
//! `VECTOR2D` glyph points by truncation), [`StrokeCollector`] records everything it is
//! handed.

use kicad_geom::{Vector2D, Vector2I};

use crate::glyph::{Glyph, OutlineGlyph};

/// The drawing interface the layout engine needs (the `KIGFX::GAL` subset).
pub trait TextGal {
    /// Port of `GAL::SetLineWidth( float )`.
    fn set_line_width(&mut self, a_line_width: f32);

    /// Port of `GAL::GetLineWidth()`.
    fn line_width(&self) -> f32;

    /// Port of `GAL::DrawGlyph( const KIFONT::GLYPH&, int aNth, int aTotal )`.
    fn draw_glyph(&mut self, a_glyph: &Glyph, a_nth: i32, a_total: i32);

    /// Port of `GAL::DrawGlyphs`: draw the glyphs in order (`DrawGlyph( *aGlyphs[i], i,
    /// aGlyphs.size() )`; the hover colour switching of the C++ has no geometric effect).
    fn draw_glyphs(&mut self, a_glyphs: &[Glyph]) {
        let total = a_glyphs.len() as i32;

        for (i, glyph) in a_glyphs.iter().enumerate() {
            self.draw_glyph(glyph, i as i32, total);
        }
    }
}

/// `VECTOR2I( const VECTOR2D& )`: clamp to the `int` range, then truncate toward zero.
pub fn to_vector2i(a_point: Vector2D) -> Vector2I {
    a_point.cast()
}

/// Port of `CALLBACK_GAL` in its stroke modes (the `( stroke, triangle )` and
/// `( stroke, outline )` constructors, `m_stroke = true`).
///
/// Stroke glyphs: for each polyline, the stroke callback is called for each pair of
/// consecutive points (single-point polylines emit nothing).  Outline glyphs are handed to
/// the outline-glyph callback unchanged; the C++ triangulates them (`m_triangulate`) or
/// fractures them into outlines, which is left to the caller (see `PORT_STATUS.md`).
pub struct CallbackGal<'a> {
    stroke_callback: Box<dyn FnMut(Vector2I, Vector2I) + 'a>,
    outline_glyph_callback: Box<dyn FnMut(&OutlineGlyph) + 'a>,
    line_width: f32,
}

impl<'a> CallbackGal<'a> {
    /// `CALLBACK_GAL( aDisplayOptions, aStrokeCallback, ... )`.
    pub fn new(
        stroke_callback: impl FnMut(Vector2I, Vector2I) + 'a,
        outline_glyph_callback: impl FnMut(&OutlineGlyph) + 'a,
    ) -> Self {
        CallbackGal {
            stroke_callback: Box::new(stroke_callback),
            outline_glyph_callback: Box::new(outline_glyph_callback),
            line_width: 1.0,
        }
    }

    /// A `CALLBACK_GAL` with only a stroke callback (outline glyphs are ignored).
    pub fn stroke_only(stroke_callback: impl FnMut(Vector2I, Vector2I) + 'a) -> Self {
        Self::new(stroke_callback, |_| {})
    }
}

impl TextGal for CallbackGal<'_> {
    fn set_line_width(&mut self, a_line_width: f32) {
        self.line_width = a_line_width;
    }

    fn line_width(&self) -> f32 {
        self.line_width
    }

    /// Port of `CALLBACK_GAL::DrawGlyph`.
    fn draw_glyph(&mut self, a_glyph: &Glyph, _a_nth: i32, _a_total: i32) {
        match a_glyph {
            Glyph::Stroke(glyph) => {
                for point_list in &glyph.strokes {
                    for ii in 1..point_list.len() {
                        (self.stroke_callback)(
                            to_vector2i(point_list[ii - 1]),
                            to_vector2i(point_list[ii]),
                        );
                    }
                }
            }
            Glyph::Outline(glyph) => (self.outline_glyph_callback)(glyph),
        }
    }
}

/// A [`TextGal`] that records what it is given: the line width, every glyph, and the stroke
/// segments exactly as `CALLBACK_GAL` would hand them to its stroke callback.
#[derive(Clone, Debug, Default)]
pub struct StrokeCollector {
    /// The last `SetLineWidth` value (`GAL` default 1.0).
    pub line_width: f32,
    /// The glyphs in drawing order.
    pub glyphs: Vec<Glyph>,
    /// The stroke segments (truncated to `VECTOR2I`) in drawing order.
    pub segments: Vec<(Vector2I, Vector2I)>,
}

impl StrokeCollector {
    /// An empty collector.
    pub fn new() -> Self {
        StrokeCollector {
            line_width: 1.0,
            ..Default::default()
        }
    }

    /// The stroke polylines of all stroke glyphs in drawing order, in `VECTOR2D`.
    pub fn polylines(&self) -> Vec<&Vec<Vector2D>> {
        self.glyphs
            .iter()
            .filter_map(Glyph::as_stroke)
            .flat_map(|g| g.strokes.iter())
            .collect()
    }
}

impl TextGal for StrokeCollector {
    fn set_line_width(&mut self, a_line_width: f32) {
        self.line_width = a_line_width;
    }

    fn line_width(&self) -> f32 {
        self.line_width
    }

    fn draw_glyph(&mut self, a_glyph: &Glyph, _a_nth: i32, _a_total: i32) {
        if let Glyph::Stroke(glyph) = a_glyph {
            for point_list in &glyph.strokes {
                for ii in 1..point_list.len() {
                    self.segments
                        .push((to_vector2i(point_list[ii - 1]), to_vector2i(point_list[ii])));
                }
            }
        }

        self.glyphs.push(a_glyph.clone());
    }
}
