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

//! Port of the `PSLIKE_PLOTTER` parts that `SVG_PLOTTER` inherits
//! (`include/plotters/plotters_pslike.h` L41-143, `common/plotters/PS_plotter.cpp` L45-226):
//! colour handling and the pad flashes drawn with lower level primitives.
//!
//! `PSLIKE_PLOTTER` is an intermediate class; in Rust it is the trait [`PsLikePlotter`]
//! (the pure virtual `emitSetRGBColor`) plus free functions holding the shared bodies, which
//! the engines call from their `Plotter` implementation.
//!
//! Not ported (PostScript / PDF only, never reached from `SVG_PLOTTER`): `SetScaleAdjust`,
//! `computeTextParameters`, `encodeStringForPlotter`, `returnPostscriptTextWidth`,
//! `postscriptTextAscent` and the Helvetica width tables.

use kicad_core::color4d::Color4D;
use kicad_core::plot_enums::{FillT, PlotTextMode};
use kicad_geom::{ANGLE_90, EdaAngle, ShapePolySet, Vector2I, rotate_point, rotate_point_around};

use crate::plotter::{PlotData, Plotter};

/// The `PSLIKE_PLOTTER` interface on top of [`Plotter`].
pub trait PsLikePlotter: Plotter {
    /// Port of `PSLIKE_PLOTTER::emitSetRGBColor` (pure virtual): emit the operator that sets
    /// the colour (reduced values 0.0 to 1.0).
    fn emit_set_rgb_color(&mut self, r: f64, g: f64, b: f64, a: f64);
}

/// Port of `PSLIKE_PLOTTER::SetTextMode`: PS and PDF fully implement native text (for the
/// Latin-1 subset); `DEFAULT` keeps the current mode. Returns the new mode.
pub fn pslike_text_mode(a_current: PlotTextMode, a_mode: PlotTextMode) -> PlotTextMode {
    if a_mode != PlotTextMode::Default {
        a_mode
    } else {
        a_current
    }
}

/// Port of `PSLIKE_PLOTTER::SetColor`: the PSLIKE computes the rgb values, the subclass emits
/// the operator to actually do it.
pub fn pslike_set_color<P: PsLikePlotter + ?Sized>(p: &mut P, color: &Color4D) {
    let color_mode = p.state().color_mode;
    let negative_mode = p.state().negative_mode;

    if color_mode {
        if negative_mode {
            p.emit_set_rgb_color(1.0 - color.r, 1.0 - color.g, 1.0 - color.b, color.a);
        } else {
            p.emit_set_rgb_color(color.r, color.g, color.b, color.a);
        }
    } else {
        // B/W Mode - Use BLACK or WHITE for all items
        // note the 2 colors are used in B&W mode, mainly by Pcbnew to draw
        // holes in white on pads in black
        let mut k = 1.0; // White

        if *color != Color4D::WHITE {
            k = 0.0;
        }

        if negative_mode {
            p.emit_set_rgb_color(1.0 - k, 1.0 - k, 1.0 - k, 1.0);
        } else {
            p.emit_set_rgb_color(k, k, k, 1.0);
        }
    }
}

/// Port of `PSLIKE_PLOTTER::FlashPadOval`.
pub fn pslike_flash_pad_oval<P: Plotter + ?Sized>(
    p: &mut P,
    a_pad_pos: Vector2I,
    a_size: Vector2I,
    a_pad_orient: EdaAngle,
    a_data: PlotData,
) {
    let mut size = a_size;
    let mut orient = a_pad_orient;

    // The pad is reduced to an oval by dy > dx
    if size.x > size.y {
        std::mem::swap(&mut size.x, &mut size.y);
        orient += ANGLE_90;
    }

    let delta = size.y.wrapping_sub(size.x);
    let mut a = Vector2I::new(0, -delta / 2);
    let mut b = Vector2I::new(0, delta / 2);

    rotate_point(&mut a, orient);
    rotate_point(&mut b, orient);

    p.thick_segment(a + a_pad_pos, b + a_pad_pos, size.x, a_data);
}

/// Port of `PSLIKE_PLOTTER::FlashPadCircle`.
pub fn pslike_flash_pad_circle<P: Plotter + ?Sized>(
    p: &mut P,
    a_pad_pos: Vector2I,
    a_diameter: i32,
    _a_data: PlotData,
) {
    p.circle(a_pad_pos, a_diameter, FillT::FilledShape, 0);
}

/// Port of `PSLIKE_PLOTTER::FlashPadRect`.
pub fn pslike_flash_pad_rect<P: Plotter + ?Sized>(
    p: &mut P,
    a_pad_pos: Vector2I,
    a_size: Vector2I,
    a_pad_orient: EdaAngle,
    a_data: PlotData,
) {
    let mut corner_list: Vec<Vector2I> = Vec::with_capacity(5);

    let dx = a_size.x / 2;
    let dy = a_size.y / 2;

    corner_list.push(Vector2I::new(a_pad_pos.x - dx, a_pad_pos.y + dy));
    corner_list.push(Vector2I::new(a_pad_pos.x - dx, a_pad_pos.y - dy));
    corner_list.push(Vector2I::new(a_pad_pos.x + dx, a_pad_pos.y - dy));
    corner_list.push(Vector2I::new(a_pad_pos.x + dx, a_pad_pos.y + dy));

    for corner in corner_list.iter_mut() {
        rotate_point_around(corner, a_pad_pos, a_pad_orient);
    }

    corner_list.push(corner_list[0]);

    p.plot_poly(&corner_list, FillT::FilledShape, 0, a_data);
}

/// Port of `PSLIKE_PLOTTER::FlashPadRoundRect` with the result of
/// `TransformRoundChamferedRectToPolygon( outline, aPadPos, aSize, aOrient, aCornerRadius, 0.0,
/// 0, 0, GetPlotterArcHighDef(), ERROR_INSIDE )` passed in as `a_outline` (that function is not
/// in `kicad-geom` yet).
pub fn pslike_flash_pad_round_rect_from_outline<P: Plotter + ?Sized>(
    p: &mut P,
    a_outline: &ShapePolySet,
    a_data: PlotData,
) {
    // TransformRoundRectToPolygon creates only one convex polygon. C++ reads Outline( 0 ) and
    // cornerList[0] unconditionally (undefined behaviour for an empty result): nothing is
    // plotted then.
    if a_outline.outline_count() == 0 {
        return;
    }

    let poly = a_outline.c_outline(0);

    if poly.point_count() == 0 {
        return;
    }

    let mut corner_list: Vec<Vector2I> = Vec::with_capacity(poly.point_count() as usize + 1);

    for ii in 0..poly.point_count() {
        corner_list.push(poly.c_point(ii));
    }

    // Close polygon
    corner_list.push(corner_list[0]);

    p.plot_poly(&corner_list, FillT::FilledShape, 0, a_data);
}

/// Port of `PSLIKE_PLOTTER::FlashPadCustom`.
pub fn pslike_flash_pad_custom<P: Plotter + ?Sized>(
    p: &mut P,
    _a_pad_pos: Vector2I,
    _a_size: Vector2I,
    _a_orient: EdaAngle,
    a_polygons: &ShapePolySet,
    a_data: PlotData,
) {
    let mut corner_list: Vec<Vector2I> = Vec::new();

    for cnt in 0..a_polygons.outline_count() {
        let poly = a_polygons.c_outline(cnt);
        corner_list.clear();

        for ii in 0..poly.point_count() {
            corner_list.push(poly.c_point(ii));
        }

        // C++ reads cornerList[0] of an empty outline (undefined behaviour): skipped.
        if corner_list.is_empty() {
            continue;
        }

        // Close polygon
        corner_list.push(corner_list[0]);

        p.plot_poly(&corner_list, FillT::FilledShape, 0, a_data);
    }
}

/// Port of `PSLIKE_PLOTTER::FlashPadTrapez`.
pub fn pslike_flash_pad_trapez<P: Plotter + ?Sized>(
    p: &mut P,
    a_pad_pos: Vector2I,
    a_corners: &[Vector2I; 4],
    a_pad_orient: EdaAngle,
    a_data: PlotData,
) {
    let mut corner_list: Vec<Vector2I> = a_corners.to_vec();

    for corner in corner_list.iter_mut() {
        rotate_point(corner, a_pad_orient);
        *corner += a_pad_pos;
    }

    corner_list.push(corner_list[0]);
    p.plot_poly(&corner_list, FillT::FilledShape, 0, a_data);
}

/// Port of `PSLIKE_PLOTTER::FlashRegularPolygon`: does nothing (the C++ asserts).
pub fn pslike_flash_regular_polygon<P: Plotter + ?Sized>(
    _p: &mut P,
    _a_shape_pos: Vector2I,
    _a_radius: i32,
    _a_corner_count: i32,
    _a_orient: EdaAngle,
    _a_data: PlotData,
) {
    // Do nothing
}
