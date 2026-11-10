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

//! Port of `SVG_PLOTTER` (`include/plotters/plotters_pslike.h` L712-867,
//! `common/plotters/SVG_plotter.cpp`).
//!
//! Every `fmt::print` of the C++ is reproduced literally (attribute strings, blanks, line
//! breaks, `{:.{}f}` with `m_precision` versus plain `{:f}`), so that the files are byte
//! identical. The lazy style handling is kept: setters only set `m_graphics_changed`; the next
//! primitive prints `</g>\n<g style="...">` (closing the previous style group).
//!
//! Differences (see `PORT_STATUS.md`): the date of the `<title>` comes from
//! [`SvgPlotter::set_creation_date`] instead of the wall clock; `PlotImage` of a non-empty
//! bitmap plots the base placeholder rectangle (no PNG encoder); `@{...}` text expressions are
//! not evaluated.

// Note:
// During tests, we (JPC) found issues when the coordinates used 6 digits in mantissa
// especially for stroke-width using very small (but not null) values < 0.00001 mm
// So to avoid this king of issue, we are using 4 digits in mantissa
// The resolution (m_precision ) is 0.1 micron, that looks enough for a SVG file

use std::f64::consts::PI;
use std::fmt;
use std::sync::Arc;

use kicad_core::color4d::Color4D;
use kicad_core::plot_enums::{FillT, PlotFormat, PlotTextMode};
use kicad_core::printf::format_f;
use kicad_core::stroke_params::LineStyle;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::gr_text::gr_text_width;
use kicad_font::{Font, Metrics};
use kicad_geom::cmath::{f64_to_i32_x86, f64_to_i64_x86};
use kicad_geom::{
    ANGLE_180, Box2D, Box2I, EdaAngle, ShapePolySet, ShapeRect, Vector2D, Vector2I, rotate_point,
};

use crate::plotter::{
    DO_NOT_SET_LINE_WIDTH, PlotData, Plotter, PlotterState, USE_DEFAULT_LINE_WIDTH,
    plot_image_base, plot_poly_chain_base, user_to_device_coordinates_base,
};
use crate::plotter_gerber::{RoundChamferedRectToPolygonFn, round_chamfered_rect_to_polygon};
use crate::plotter_text::text_base;
use crate::plotters_pslike::{
    PsLikePlotter, pslike_flash_pad_circle, pslike_flash_pad_custom, pslike_flash_pad_oval,
    pslike_flash_pad_rect, pslike_flash_pad_round_rect_from_outline, pslike_flash_pad_trapez,
    pslike_flash_regular_polygon, pslike_set_color, pslike_text_mode,
};

/// Port of the file-local `XmlEsc`: translates '<' to "&lt;", '>' to "&gt;" and so on,
/// according to the spec: <http://www.w3.org/TR/2000/WD-xml-c14n-20000119.html#charescaping>
pub fn xml_esc(a_str: &str, is_attribute: bool) -> String {
    let mut escaped = String::with_capacity(a_str.len());

    for c in a_str.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '\r' => escaped.push_str("&#xD;"),
            _ => {
                if is_attribute {
                    match c {
                        '"' => escaped.push_str("&quot;"),
                        '\t' => escaped.push_str("&#x9;"),
                        '\n' => escaped.push_str("&#xA;"),
                        _ => escaped.push(c),
                    }
                } else {
                    escaped.push(c);
                }
            }
        }
    }

    escaped
}

/// `{:.{}f}` of `{fmt}`.
fn fp(v: f64, precision: u32) -> String {
    format_f(v, precision as usize)
}

/// `{:f}` of `{fmt}` (6 decimals).
fn f6(v: f64) -> String {
    format_f(v, 6)
}

/// Port of `SVG_PLOTTER`.
#[derive(Clone)]
pub struct SvgPlotter {
    state: PlotterState,
    /// `PSLIKE_PLOTTER::m_textMode`: how to draw text (SVG forces `STROKE`).
    text_mode: PlotTextMode,
    /// `m_fillMode`: true if the current contour rect, arc, circle, polygon must be filled
    fill_mode: FillT,
    /// `m_pen_rgb_color`: current rgb color value: each color has a value 0 ... 255, and the
    /// 3 colors are grouped in a 3x8 bits value (written in hex to svg files)
    pen_rgb_color: u32,
    /// `m_brush_rgb_color`: same as `m_pen_rgb_color`, used to fill some contours.
    brush_rgb_color: u32,
    /// `m_brush_alpha`
    brush_alpha: f64,
    /// `m_graphics_changed`: true if a pen/brush parameter is modified color, pen size, fill
    /// mode ... the new SVG stype must be output on file
    graphics_changed: bool,
    /// `m_dashed`: plot line style
    dashed: LineStyle,
    /// `m_precision`: how fine the step size is. Use 3-6 (3 means um precision, 6 nm
    /// precision) in PcbNew, 3-4 in other modules (avoid values >4 to avoid overflow).
    precision: u32,
    /// `m_plotBBoxIU`: explicit plot content bounding box (IUs), when set.
    plot_bbox_iu: Option<Box2I>,
    /// Port-only: the `GetISO8601CurrentDateTime()` text of the `<title>`.
    creation_date: String,
    /// Port-only: `TransformRoundChamferedRectToPolygon` (not in `kicad-geom` yet) for
    /// `FlashPadRoundRect`.
    round_rect_polygon_fn: Option<Arc<RoundChamferedRectToPolygonFn<'static>>>,
}

impl fmt::Debug for SvgPlotter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SvgPlotter")
            .field("state", &self.state)
            .field("text_mode", &self.text_mode)
            .field("fill_mode", &self.fill_mode)
            .field("pen_rgb_color", &self.pen_rgb_color)
            .field("brush_rgb_color", &self.brush_rgb_color)
            .field("brush_alpha", &self.brush_alpha)
            .field("graphics_changed", &self.graphics_changed)
            .field("dashed", &self.dashed)
            .field("precision", &self.precision)
            .field("plot_bbox_iu", &self.plot_bbox_iu)
            .field("creation_date", &self.creation_date)
            .field(
                "round_rect_polygon_fn",
                &self.round_rect_polygon_fn.is_some(),
            )
            .finish()
    }
}

impl Default for SvgPlotter {
    fn default() -> Self {
        Self::new()
    }
}

impl SvgPlotter {
    /// Port of `SVG_PLOTTER::SVG_PLOTTER()`.
    pub fn new() -> Self {
        let mut p = SvgPlotter {
            state: PlotterState::new(),
            // PSLIKE_PLOTTER()
            text_mode: PlotTextMode::Phantom,
            fill_mode: FillT::NoFill, // or FILLED_SHAPE or FILLED_WITH_BG_BODYCOLOR
            pen_rgb_color: 0,         // current color value (black)
            brush_rgb_color: 0,       // current color value with alpha(black)
            brush_alpha: 1.0,
            graphics_changed: true,
            dashed: LineStyle::Solid,
            precision: 4, // default: 4 digits in mantissa.
            plot_bbox_iu: None,
            creation_date: String::new(),
            round_rect_polygon_fn: None,
        };

        p.set_text_mode(PlotTextMode::Stroke);
        p
    }

    /// Port of `SVG_PLOTTER::GetDefaultFileExtension`.
    pub fn default_file_extension() -> &'static str {
        "svg"
    }

    /// The text written as the date in the `<title>` (C++: `GetISO8601CurrentDateTime()`,
    /// i.e. `YYYY-MM-DDTHH:MM:SS` of the wall clock).
    pub fn set_creation_date(&mut self, a_date: &str) {
        self.creation_date = a_date.to_string();
    }

    /// Override the `TransformRoundChamferedRectToPolygon` generator (arguments: position,
    /// size, rotation, corner radius, chamfer ratio, chamfer corners, max error; inflate 0,
    /// `ERROR_INSIDE`) used by `FlashPadRoundRect`.  `None` (the default) uses
    /// [`crate::plotter_gerber::round_chamfered_rect_to_polygon`]; the oracle tests install
    /// one that replays the C++ polygons.
    pub fn set_round_rect_polygon_fn(
        &mut self,
        a_fn: Option<Arc<RoundChamferedRectToPolygonFn<'static>>>,
    ) {
        self.round_rect_polygon_fn = a_fn;
    }

    /// `PSLIKE_PLOTTER::m_textMode`.
    pub fn text_mode(&self) -> PlotTextMode {
        self.text_mode
    }

    /// `m_precision`.
    pub fn precision(&self) -> u32 {
        self.precision
    }

    /// Port of `SVG_PLOTTER::setFillMode`: prepare parameters for `setSVGPlotStyle()`.
    fn set_fill_mode(&mut self, fill: FillT) {
        if self.fill_mode != fill {
            self.graphics_changed = true;
            self.fill_mode = fill;
        }
    }

    /// Port of `SVG_PLOTTER::setSVGPlotStyle`: output the string which define pen and brush
    /// color, shape, transparency. `a_is_group` false: do not form a new group for the style;
    /// `a_extra_style` is added into the style string before closing.
    fn set_svg_plot_style(&mut self, a_line_width: i32, a_is_group: bool, a_extra_style: &str) {
        let prec = self.precision;
        let mut out = String::new();

        if a_is_group {
            out.push_str("</g>\n<g ");
        }

        out.push_str("style=\"");

        if self.fill_mode == FillT::NoFill {
            out.push_str("fill:none; ");
        } else {
            // output the background fill color
            out.push_str(&format!("fill:#{:06X}; ", self.brush_rgb_color));

            match self.fill_mode {
                FillT::FilledShape | FillT::FilledWithBgBodycolor | FillT::FilledWithColor => {
                    out.push_str(&format!("fill-opacity:{}; ", fp(self.brush_alpha, prec)));
                }
                _ => {}
            }
        }

        let pen_w = self.user_to_device_size_f(a_line_width as f64);

        if pen_w <= 0.0 {
            out.push_str("stroke:none;");
        } else {
            // Fix a strange issue found in Inkscape: aWidth < 100 nm create issues on
            // degrouping objects.
            // So we use only 4 digits in mantissa for stroke-width.
            // TODO: perhaps used only 3 or 4 digits in mantissa for all values in mm, because
            // some issues were previously reported reported when using nm as integer units
            out.push_str(&format!(
                "\nstroke:#{:06X}; stroke-width:{}; stroke-opacity:{}; \n",
                self.pen_rgb_color,
                fp(pen_w, prec),
                fp(self.brush_alpha, prec)
            ));
            out.push_str("stroke-linecap:round; stroke-linejoin:round;");

            //set any extra attributes for non-solid lines
            match self.dashed {
                LineStyle::Dash => {
                    out.push_str(&format!(
                        "stroke-dasharray:{},{};",
                        fp(self.dash_mark_len_iu(a_line_width), prec),
                        fp(self.dash_gap_len_iu(a_line_width), prec)
                    ));
                }
                LineStyle::Dot => {
                    out.push_str(&format!(
                        "stroke-dasharray:{},{};",
                        f6(self.dot_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width))
                    ));
                }
                LineStyle::DashDot => {
                    out.push_str(&format!(
                        "stroke-dasharray:{},{},{},{};",
                        f6(self.dash_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width)),
                        f6(self.dot_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width))
                    ));
                }
                LineStyle::DashDotDot => {
                    out.push_str(&format!(
                        "stroke-dasharray:{},{},{},{},{},{};",
                        f6(self.dash_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width)),
                        f6(self.dot_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width)),
                        f6(self.dot_mark_len_iu(a_line_width)),
                        f6(self.dash_gap_len_iu(a_line_width))
                    ));
                }
                LineStyle::Default | LineStyle::Solid => {
                    // Explicitly reset the dash pattern: stroke-dasharray is inherited in SVG,
                    // so inline path styles inside a dashed group would otherwise pick it up.
                    out.push_str("stroke-dasharray:none;");
                }
            }
        }

        if !a_extra_style.is_empty() {
            out.push_str(a_extra_style);
        }

        out.push('"');

        if a_is_group {
            out.push('>');
            self.graphics_changed = false;
        }

        out.push('\n');
        self.state.output.push_str(&out);
    }

    /// `setSVGPlotStyle( GetCurrentLineWidth() )` if `m_graphics_changed`.
    fn flush_style(&mut self) {
        if self.graphics_changed {
            let w = self.state.current_pen_width;
            self.set_svg_plot_style(w, true, "");
        }
    }

    fn print(&mut self, s: &str) {
        self.state.output.push_str(s);
    }
}

impl PsLikePlotter for SvgPlotter {
    /// Port of `SVG_PLOTTER::emitSetRGBColor`: initialize `m_pen_rgb_color` from reduced
    /// values r, g, b (reduced values are 0.0 to 1.0).
    fn emit_set_rgb_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        // (uint32_t) of a double: cvttsd2si into a 64-bit register on x86-64
        let red = f64_to_i64_x86(255.0 * r) as u32;
        let green = f64_to_i64_x86(255.0 * g) as u32;
        let blue = f64_to_i64_x86(255.0 * b) as u32;
        let rgb_color = (red << 16) | (green << 8) | blue;

        if self.pen_rgb_color != rgb_color || self.brush_alpha != a {
            self.graphics_changed = true;
            self.pen_rgb_color = rgb_color;

            // Currently, use the same color for brush and pen (i.e. to draw and fill a
            // contour).
            self.brush_rgb_color = rgb_color;
            self.brush_alpha = a;
        }
    }
}

impl Plotter for SvgPlotter {
    fn state(&self) -> &PlotterState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut PlotterState {
        &mut self.state
    }

    fn plotter_type(&self) -> PlotFormat {
        PlotFormat::Svg
    }

    /// Port of `SVG_PLOTTER::StartPlot`: create SVG file header.
    fn start_plot(&mut self, _a_page_number: &str) -> bool {
        debug_assert!(self.state.output_open);
        let prec = self.precision;

        let header = "<?xml version=\"1.0\" standalone=\"no\"?>\n \
<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \n \
\"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"> \n\
<svg\n  \
xmlns:svg=\"http://www.w3.org/2000/svg\"\n  \
xmlns=\"http://www.w3.org/2000/svg\"\n  \
xmlns:xlink=\"http://www.w3.org/1999/xlink\"\n  \
xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"\n  \
version=\"1.1\"\n";

        // Write header.
        self.print(header);

        // Write viewport pos and size.  The SVG width/height and viewBox are in mm (device
        // units). When a plot bounding box was supplied, the viewBox is that box
        // transformed by the viewport, so the content keeps its origin at the SVG
        // origin even when it extends to negative coordinates. Otherwise the page
        // size is used and the origin is the viewbox (which is also the page) corner.
        let s = &self.state;
        let mut view_box_origin = Vector2D::new(0.0, 0.0);
        let mut viewbox_size = Vector2D::new(
            s.paper_size.x as f64 * s.iu_per_device_unit,
            s.paper_size.y as f64 * s.iu_per_device_unit,
        );

        if let Some(bbox) = &self.plot_bbox_iu {
            let device_scale = s.plot_scale * s.iu_per_device_unit;

            view_box_origin.x = bbox.left().wrapping_sub(s.plot_offset.x) as f64 * device_scale;
            view_box_origin.y = bbox.top().wrapping_sub(s.plot_offset.y) as f64 * device_scale;
            viewbox_size.x = bbox.width() as f64 * device_scale;
            viewbox_size.y = bbox.height() as f64 * device_scale;
        }

        // When mirroring, the viewbox also needs to be mirrored, so that the same content is
        // still visible in it.
        if s.plot_mirror {
            if s.mirror_is_horizontal {
                view_box_origin.x = -view_box_origin.x - viewbox_size.x;
            } else {
                view_box_origin.y = -view_box_origin.y - viewbox_size.y;
            }
        }

        let line = format!(
            "  width=\"{}mm\" height=\"{}mm\" viewBox=\"{} {} {} {}\">\n",
            fp(viewbox_size.x, prec),
            fp(viewbox_size.y, prec),
            fp(view_box_origin.x, prec),
            fp(view_box_origin.y, prec),
            fp(viewbox_size.x, prec),
            fp(viewbox_size.y, prec)
        );
        self.print(&line);

        // Write title. wxFileName( m_filename ).GetFullName(): the last path component.
        let full_name = self
            .state
            .filename
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        let line = format!(
            "<title>SVG Image created as {} date {} </title>\n",
            xml_esc(&full_name, false),
            self.creation_date
        );
        self.print(&line);

        // End of header
        let line = format!(
            "  <desc>Image generated by {} </desc>\n",
            xml_esc(&self.state.creator, false)
        );
        self.print(&line);

        // output the pen and brush color (RVB values in hex) and opacity
        let opacity = 1.0; // 0.0 (transparent to 1.0 (solid)
        let line = format!(
            "<g style=\"fill:#{:06X}; fill-opacity:{};stroke:#{:06X}; stroke-opacity:{};\n",
            self.brush_rgb_color,
            fp(self.brush_alpha, prec),
            self.pen_rgb_color,
            fp(opacity, prec)
        );
        self.print(&line);

        // output the pen cap and line joint
        self.print("stroke-linecap:round; stroke-linejoin:round;\"\n");
        self.print(" transform=\"translate(0 0) scale(1 1)\">\n");
        true
    }

    /// Port of `SVG_PLOTTER::EndPlot`.
    fn end_plot(&mut self) -> bool {
        self.print("</g> \n</svg>\n");
        self.state.output_open = false;
        true
    }

    /// Port of `SVG_PLOTTER::SetCurrentLineWidth`: set the current line width (in IUs) for
    /// the next plot.
    fn set_current_line_width(&mut self, a_width: i32, _a_data: PlotData) {
        let mut a_width = a_width;

        if a_width == DO_NOT_SET_LINE_WIDTH {
            return;
        } else if a_width == USE_DEFAULT_LINE_WIDTH {
            a_width = self.state.render_settings_or_default().default_pen_width();
        }

        // Note: aWidth == 0 is fine: used for filled shapes with no outline thickness
        // (C++: wxASSERT_MSG( aWidth >= 0, "Plotter called to set negative pen width" ))

        if a_width != self.state.current_pen_width {
            self.graphics_changed = true;
            self.state.current_pen_width = a_width;
        }
    }

    /// Port of `PSLIKE_PLOTTER::SetColor`.
    fn set_color(&mut self, color: &Color4D) {
        pslike_set_color(self, color);
    }

    /// Port of `SVG_PLOTTER::SetDash`: SVG supports dashed lines.
    fn set_dash(&mut self, _a_line_width: i32, a_line_style: LineStyle) {
        if self.dashed != a_line_style {
            self.graphics_changed = true;
            self.dashed = a_line_style;
        }
    }

    /// Port of `SVG_PLOTTER::SetViewport`.
    fn set_viewport(
        &mut self,
        a_offset: Vector2I,
        a_ius_per_decimil: f64,
        a_scale: f64,
        a_mirror: bool,
    ) {
        let s = &mut self.state;
        s.plot_mirror = a_mirror;
        s.yaxis_reversed = true; // unlike other plotters, SVG has Y axis reversed
        s.plot_offset = a_offset;
        s.plot_scale = a_scale;
        s.ius_per_decimil = a_ius_per_decimil;

        // Compute the paper size in IUs. for historical reasons the page size is in mils
        let (w, h) = s.page_info.size_mils();
        // VECTOR2I = VECTOR2D: truncation with clamping
        s.paper_size = Vector2D::new(w, h).cast::<i32>();
        // int *= double
        s.paper_size.x = f64_to_i32_x86(s.paper_size.x as f64 * (10.0 * a_ius_per_decimil));
        s.paper_size.y = f64_to_i32_x86(s.paper_size.y as f64 * (10.0 * a_ius_per_decimil));

        // gives now a default value to iuPerDeviceUnit (because the units of the caller is
        // now known)
        let ius_per_mm = s.ius_per_decimil / 2.54 * 1000.0;
        s.iu_per_device_unit = 1.0 / ius_per_mm;

        self.set_svg_coordinates_format(4);
    }

    /// Port of `SVG_PLOTTER::SetSvgCoordinatesFormat`: select SVG coordinate precision
    /// (number of digits needed for 1 mm; SVG units are always mm). Should be called only
    /// after `SetViewport()`.
    fn set_svg_coordinates_format(&mut self, a_precision: u32) {
        // Only number of digits in mantissa are adjustable.
        // SVG units are always mm
        self.precision = a_precision;
    }

    /// Port of `SVG_PLOTTER::SetPlotBBox`: set an explicit bounding box for the plotted
    /// content (in IUs). When set, the SVG width/height and viewBox are derived from this box
    /// transformed to device units.
    fn set_plot_bbox(&mut self, a_bbox_iu: &Box2I) {
        self.plot_bbox_iu = Some(*a_bbox_iu);
    }

    /// Port of `PSLIKE_PLOTTER::SetTextMode`.
    fn set_text_mode(&mut self, mode: PlotTextMode) {
        self.text_mode = pslike_text_mode(self.text_mode, mode);
    }

    /// Port of `SVG_PLOTTER::StartBlock`: we can't use `<g></g>` for blocks because we're
    /// already using it for graphics context, and our graphics context handling is lazy (ie:
    /// it leaves the last group open until the context changes).
    fn start_block(&mut self, _a_data: PlotData) {}

    /// Port of `SVG_PLOTTER::EndBlock`.
    fn end_block(&mut self, _a_data: PlotData) {}

    /// Port of `SVG_PLOTTER::userToDeviceCoordinates`.
    fn user_to_device_coordinates(&mut self, a_coordinate: Vector2I) -> Vector2D {
        let s = &self.state;
        let mut pos = user_to_device_coordinates_base(s, a_coordinate);

        if s.plot_mirror {
            if s.mirror_is_horizontal {
                pos.x -= s.paper_size.x as f64 * s.iu_per_device_unit;
            } else {
                pos.y -= s.paper_size.y as f64 * s.iu_per_device_unit;
            }
        }

        pos
    }

    /// Port of `SVG_PLOTTER::Rect`.
    fn rect(&mut self, p1: Vector2I, p2: Vector2I, fill: FillT, width: i32, a_corner_radius: i32) {
        let rect = Box2I::new(p1, Vector2I::new(p2.x - p1.x, p2.y - p1.y));

        let org_dev = self.user_to_device_coordinates(rect.origin());
        let end_dev = self.user_to_device_coordinates(rect.end());
        let size_dev = end_dev - org_dev;

        // Ensure size of rect in device coordinates is > 0
        // I don't know if this is a SVG issue or a Inkscape issue, but
        // Inkscape has problems with negative or null values for width and/or height, so avoid
        // them
        let rect_dev = Box2D::new(org_dev, size_dev);

        self.set_fill_mode(fill);
        self.set_current_line_width(width, None);
        self.flush_style();

        let prec = self.precision;

        // Rectangles having a 0 size value for height or width are just not drawn on
        // Inkscape, so use a line when happens.
        if rect_dev.size().x == 0.0 || rect_dev.size().y == 0.0 {
            // Draw a line
            let line = format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" />\n",
                fp(rect_dev.position().x, prec),
                fp(rect_dev.position().y, prec),
                fp(rect_dev.end().x, prec),
                fp(rect_dev.end().y, prec)
            );
            self.print(&line);
        } else if a_corner_radius > 0 {
            let bx = Box2I::new(p1, Vector2I::new(p2.x - p1.x, p2.y - p1.y));
            let mut rounded_rect = ShapeRect::from_box(&bx);
            rounded_rect.set_radius(a_corner_radius);
            plot_poly_chain_base(self, &rounded_rect.outline(), fill, width, None);
        } else {
            let size = rect_dev.size();
            let line = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" />\n",
                f6(rect_dev.position().x),
                f6(rect_dev.position().y),
                f6(size.x),
                f6(size.y),
                f6(self.user_to_device_size_f(a_corner_radius as f64))
            );
            self.print(&line);
        }
    }

    /// Port of `SVG_PLOTTER::Circle`.
    fn circle(&mut self, pos: Vector2I, diametre: i32, fill: FillT, width: i32) {
        let pos_dev = self.user_to_device_coordinates(pos);
        let mut radius = self.user_to_device_size_f(diametre as f64 / 2.0);

        self.set_fill_mode(fill);
        self.set_current_line_width(width, None);
        self.flush_style();

        // If diameter is less than width, switch to filled mode
        if fill == FillT::NoFill && diametre < self.state.current_pen_width {
            self.set_fill_mode(FillT::FilledShape);
            let width = self.state.current_pen_width;
            self.set_current_line_width(0, None);

            radius = self.user_to_device_size_f((diametre as f64 / 2.0) + (width as f64 / 2.0));
        }

        let prec = self.precision;
        let line = format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" /> \n",
            fp(pos_dev.x, prec),
            fp(pos_dev.y, prec),
            fp(radius, prec)
        );
        self.print(&line);
    }

    /// Port of `SVG_PLOTTER::Arc`: draws an arc of a circle, centered on `a_center`. The
    /// current pen is used for the outline and the current brush for filling the shape. The
    /// arc is drawn in an anticlockwise direction from the start point to the end point.
    fn arc(
        &mut self,
        a_center: Vector2D,
        a_start_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_fill: FillT,
        a_width: i32,
    ) {
        if a_radius <= 0.0 {
            // const VECTOR2I& parameter: implicit truncation of the VECTOR2D
            self.circle(a_center.cast::<i32>(), a_width, FillT::FilledShape, 0);
            return;
        }

        let mut start_angle = -a_start_angle;
        let mut end_angle = start_angle - a_angle;

        if end_angle < start_angle {
            std::mem::swap(&mut start_angle, &mut end_angle);
        }

        // Calculate start point.
        let centre_device = self.user_to_device_coordinates(a_center.cast::<i32>());
        let radius_device = self.user_to_device_size_f(a_radius);

        if self.state.plot_mirror {
            if self.state.mirror_is_horizontal {
                std::mem::swap(&mut start_angle, &mut end_angle);
                start_angle = ANGLE_180 - start_angle;
                end_angle = ANGLE_180 - end_angle;
            } else {
                start_angle = -start_angle;
                end_angle = -end_angle;
            }
        }

        let mut start = Vector2D::new(radius_device, 0.0);
        rotate_point(&mut start, start_angle);
        let mut end = Vector2D::new(radius_device, 0.0);
        rotate_point(&mut end, end_angle);
        start += centre_device;
        end += centre_device;

        let mut theta1 = start_angle.as_radians();

        if theta1 < 0.0 {
            theta1 += PI * 2.0;
        }

        let mut theta2 = end_angle.as_radians();

        if theta2 < 0.0 {
            theta2 += PI * 2.0;
        }

        if theta2 < theta1 {
            theta2 += PI * 2.0;
        }

        let mut flg_arc = 0; // flag for large or small arc. 0 means less than 180 degrees

        if (theta2 - theta1).abs() > PI {
            flg_arc = 1;
        }

        let flg_sweep = 0; // flag for sweep always 0
        let prec = self.precision;

        // Draw a single arc: an arc is one of 3 curve commands (2 other are 2 bezier curves)
        // params are start point, radius1, radius2, X axe rotation,
        // flag arc size (0 = small arc > 180 deg, 1 = large arc > 180 deg),
        // sweep arc ( 0 = CCW, 1 = CW),
        // end point
        if a_fill != FillT::NoFill {
            // Filled arcs (in Eeschema) consist of the pie wedge and a stroke only on the arc
            // This needs to be drawn in two steps.
            self.set_fill_mode(a_fill);
            self.set_current_line_width(0, None);
            self.flush_style();

            let line = format!(
                "<path d=\"M{} {} A{} {} 0.0 {} {} {} {} L {} {} Z\" />\n",
                fp(start.x, prec),
                fp(start.y, prec),
                fp(radius_device, prec),
                fp(radius_device, prec),
                flg_arc,
                flg_sweep,
                fp(end.x, prec),
                fp(end.y, prec),
                fp(centre_device.x, prec),
                fp(centre_device.y, prec)
            );
            self.print(&line);
        }

        self.set_fill_mode(FillT::NoFill);
        self.set_current_line_width(a_width, None);
        self.flush_style();

        let line = format!(
            "<path d=\"M{} {} A{} {} 0.0 {} {} {} {}\" />\n",
            fp(start.x, prec),
            fp(start.y, prec),
            fp(radius_device, prec),
            fp(radius_device, prec),
            flg_arc,
            flg_sweep,
            fp(end.x, prec),
            fp(end.y, prec)
        );
        self.print(&line);
    }

    /// Port of `SVG_PLOTTER::BezierCurve`: a native cubic curve.
    fn bezier_curve(
        &mut self,
        a_start: Vector2I,
        a_control1: Vector2I,
        a_control2: Vector2I,
        a_end: Vector2I,
        _a_tolerance: i32,
        a_line_thickness: i32,
    ) {
        self.set_fill_mode(FillT::NoFill);
        self.set_current_line_width(a_line_thickness, None);
        self.flush_style();

        let start = self.user_to_device_coordinates(a_start);
        let ctrl1 = self.user_to_device_coordinates(a_control1);
        let ctrl2 = self.user_to_device_coordinates(a_control2);
        let end = self.user_to_device_coordinates(a_end);
        let prec = self.precision;

        // Generate a cubic curve: start point and 3 other control points.
        let line = format!(
            "<path d=\"M{},{} C{},{} {},{} {},{}\" />\n",
            fp(start.x, prec),
            fp(start.y, prec),
            fp(ctrl1.x, prec),
            fp(ctrl1.y, prec),
            fp(ctrl2.x, prec),
            fp(ctrl2.y, prec),
            fp(end.x, prec),
            fp(end.y, prec)
        );
        self.print(&line);
    }

    /// Port of `SVG_PLOTTER::PlotPoly`.
    fn plot_poly(
        &mut self,
        a_corner_list: &[Vector2I],
        a_fill: FillT,
        a_width: i32,
        _a_data: PlotData,
    ) {
        if a_corner_list.len() <= 1 {
            return;
        }

        self.set_fill_mode(a_fill);
        self.set_current_line_width(a_width, None);
        self.print("<path ");

        match a_fill {
            FillT::NoFill | FillT::Hatch | FillT::ReverseHatch | FillT::CrossHatch => {
                self.set_svg_plot_style(a_width, false, "fill:none");
            }
            FillT::FilledWithBgBodycolor | FillT::FilledShape | FillT::FilledWithColor => {
                self.set_svg_plot_style(a_width, false, "fill-rule:evenodd;");
            }
        }

        let prec = self.precision;
        let pos = self.user_to_device_coordinates(a_corner_list[0]);
        let line = format!("d=\"M {},{}\n", fp(pos.x, prec), fp(pos.y, prec));
        self.print(&line);

        for corner in &a_corner_list[1..a_corner_list.len() - 1] {
            let pos = self.user_to_device_coordinates(*corner);
            let line = format!("{},{}\n", fp(pos.x, prec), fp(pos.y, prec));
            self.print(&line);
        }

        // If the corner list ends where it begins, then close the poly
        let back = a_corner_list[a_corner_list.len() - 1];

        if a_corner_list[0] == back {
            self.print("Z\" /> \n");
        } else {
            let pos = self.user_to_device_coordinates(back);
            let line = format!("{},{}\n\" /> \n", fp(pos.x, prec), fp(pos.y, prec));
            self.print(&line);
        }
    }

    /// Port of `SVG_PLOTTER::PlotImage`. Only the placeholder branch is ported: an empty
    /// requested size plots `PLOTTER::PlotImage` as in C++; a non-empty bitmap (C++: PNG
    /// encoded with wxImage, base64 embedded in an `<image>`) also plots the base placeholder
    /// rectangle, since the image pixels are not available (see `PORT_STATUS.md`).
    fn plot_image(
        &mut self,
        a_image_width: i32,
        a_image_height: i32,
        a_pos: Vector2I,
        a_scale_factor: f64,
        a_alpha: f64,
    ) {
        plot_image_base(
            self,
            a_image_width,
            a_image_height,
            a_pos,
            a_scale_factor,
            a_alpha,
        );
    }

    /// Port of `SVG_PLOTTER::PenTo`.
    fn pen_to(&mut self, pos: Vector2I, plume: char) {
        let prec = self.precision;

        if plume == 'Z' {
            if self.state.pen_state != 'Z' {
                self.print("\" />\n");
                self.state.pen_state = 'Z';
                self.state.pen_lastpos.x = -1;
                self.state.pen_lastpos.y = -1;
            }

            return;
        }

        if self.state.pen_state == 'Z' {
            // here plume = 'D' or 'U'
            let pos_dev = self.user_to_device_coordinates(pos);

            // Ensure we do not use a fill mode when moving the pen,
            // in SVG mode (i;e. we are plotting only basic lines, not a filled area
            if self.fill_mode != FillT::NoFill {
                self.set_fill_mode(FillT::NoFill);
            }

            self.flush_style();

            let line = format!(
                "<path d=\"M{} {}\n",
                fp(pos_dev.x, prec),
                fp(pos_dev.y, prec)
            );
            self.print(&line);
        } else if self.state.pen_state != plume || pos != self.state.pen_lastpos {
            self.flush_style();

            let pos_dev = self.user_to_device_coordinates(pos);

            let line = format!("L{} {}\n", fp(pos_dev.x, prec), fp(pos_dev.y, prec));
            self.print(&line);
        }

        self.state.pen_state = plume;
        self.state.pen_lastpos = pos;
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadCircle`.
    fn flash_pad_circle(&mut self, a_pad_pos: Vector2I, a_diameter: i32, a_data: PlotData) {
        pslike_flash_pad_circle(self, a_pad_pos, a_diameter, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadOval`.
    fn flash_pad_oval(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    ) {
        pslike_flash_pad_oval(self, a_pad_pos, a_size, a_pad_orient, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadRect`.
    fn flash_pad_rect(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    ) {
        pslike_flash_pad_rect(self, a_pad_pos, a_size, a_pad_orient, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadRoundRect`: the outline of
    /// `TransformRoundChamferedRectToPolygon` ([`round_chamfered_rect_to_polygon`], or the
    /// generator installed with [`SvgPlotter::set_round_rect_polygon_fn`]).
    fn flash_pad_round_rect(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_corner_radius: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        let error = self.plotter_arc_high_def();
        let outline: ShapePolySet = match self.round_rect_polygon_fn.clone() {
            Some(transform) => transform(
                a_pad_pos,
                a_size,
                a_orient,
                a_corner_radius,
                0.0,
                0,
                error,
            ),
            None => round_chamfered_rect_to_polygon(
                a_pad_pos,
                a_size,
                a_orient,
                a_corner_radius,
                0.0,
                0,
                error,
            ),
        };

        pslike_flash_pad_round_rect_from_outline(self, &outline, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadCustom`.
    fn flash_pad_custom(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_polygons: &ShapePolySet,
        a_data: PlotData,
    ) {
        pslike_flash_pad_custom(self, a_pad_pos, a_size, a_pad_orient, a_polygons, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashPadTrapez`.
    fn flash_pad_trapez(
        &mut self,
        a_pad_pos: Vector2I,
        a_corners: &[Vector2I; 4],
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    ) {
        pslike_flash_pad_trapez(self, a_pad_pos, a_corners, a_pad_orient, a_data);
    }

    /// Port of `PSLIKE_PLOTTER::FlashRegularPolygon` (does nothing).
    fn flash_regular_polygon(
        &mut self,
        a_shape_pos: Vector2I,
        a_diameter: i32,
        a_corner_count: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        pslike_flash_regular_polygon(
            self,
            a_shape_pos,
            a_diameter,
            a_corner_count,
            a_orient,
            a_data,
        );
    }

    /// Port of `SVG_PLOTTER::Text`: a hidden `<text>` element (for search and editing), then
    /// the stroked glyphs inside `<g class="stroked-text"><desc>...</desc>` ... `</g>`.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        a_pos: Vector2I,
        a_color: &Color4D,
        a_text: &str,
        a_orient: EdaAngle,
        a_size: Vector2I,
        a_h_justify: GrTextHAlignT,
        a_v_justify: GrTextVAlignT,
        a_width: i32,
        a_italic: bool,
        a_bold: bool,
        a_multiline_allowed: bool,
        a_font: Option<&dyn Font>,
        a_font_metrics: &Metrics,
        _a_data: PlotData,
    ) {
        self.set_fill_mode(FillT::NoFill);
        self.set_color(a_color);
        self.set_current_line_width(a_width, None);

        // `@{...}` expressions are not evaluated (EXPRESSION_EVALUATOR is not ported).
        let text = a_text;

        self.flush_style();

        let mut text_pos = a_pos;

        let hjust = match a_h_justify {
            GrTextHAlignT::Center => "middle",
            GrTextHAlignT::Right => "end",
            GrTextHAlignT::Left => "start",
            // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
            GrTextHAlignT::Indeterminate => "start",
        };

        match a_v_justify {
            GrTextVAlignT::Center => text_pos.y = text_pos.y.wrapping_add(a_size.y / 2),
            GrTextVAlignT::Top => text_pos.y = text_pos.y.wrapping_add(a_size.y),
            GrTextVAlignT::Bottom => {}
            // wxFAIL_MSG( "Indeterminate state legal only in dialogs." )
            GrTextVAlignT::Indeterminate => {}
        }

        // aSize.x or aSize.y is < 0 for mirrored texts.
        // The actual text size value is the absolute value
        let text_size = Vector2I::new(
            gr_text_width(
                text,
                a_font,
                a_size,
                self.state.current_pen_width,
                a_bold,
                a_italic,
                a_font_metrics,
            )
            .wrapping_abs(),
            // Hershey font height to em size conversion
            (a_size.x.wrapping_mul(4) / 3).wrapping_abs(),
        );
        let anchor_pos_dev = self.user_to_device_coordinates(a_pos);
        let text_pos_dev = self.user_to_device_coordinates(text_pos);
        let sz_dev = self.user_to_device_size(text_size);
        let prec = self.precision;

        // Output the text as a hidden string (opacity = 0).  This allows WYSIWYG search to
        // highlight a selection in approximately the right area.  It also makes it easier for
        // those that need to edit the text (as text) in subsequent processes.
        {
            if !a_orient.is_zero() {
                let line = format!(
                    "<g transform=\"rotate({} {} {})\">\n",
                    f6(if self.state.plot_mirror {
                        a_orient.as_degrees()
                    } else {
                        -a_orient.as_degrees()
                    }),
                    fp(anchor_pos_dev.x, prec),
                    fp(anchor_pos_dev.y, prec)
                );
                self.print(&line);
            }

            let line = format!(
                "<text x=\"{}\" y=\"{}\"\n",
                fp(text_pos_dev.x, prec),
                fp(text_pos_dev.y, prec)
            );
            self.print(&line);

            // If the text is mirrored, we should also mirror the hidden text to match
            if self.state.plot_mirror != (a_size.x < 0) {
                let line = format!(
                    "transform=\"scale(-1 1) translate({} 0)\"\n",
                    f6(-2.0 * text_pos_dev.x)
                );
                self.print(&line);
            }

            let line = format!(
                "textLength=\"{}\" font-size=\"{}\" lengthAdjust=\"spacingAndGlyphs\"\n\
                 text-anchor=\"{}\" opacity=\"0\" stroke-opacity=\"0\">{}</text>\n",
                fp(sz_dev.x, prec),
                fp(sz_dev.y, prec),
                hjust,
                xml_esc(text, false)
            );
            self.print(&line);

            if !a_orient.is_zero() {
                self.print("</g>\n");
            }
        }

        // Output the text again as graphics with a <desc> tag (for non-WYSIWYG search and for
        // screen readers)
        {
            let line = format!(
                "<g class=\"stroked-text\"><desc>{}</desc>\n",
                xml_esc(text, false)
            );
            self.print(&line);

            let pen_width = self.state.current_pen_width;
            text_base(
                self,
                a_pos,
                a_color,
                text,
                a_orient,
                a_size,
                a_h_justify,
                a_v_justify,
                pen_width,
                a_italic,
                a_bold,
                a_multiline_allowed,
                a_font,
                a_font_metrics,
                None,
            );

            self.print("</g>");
        }
    }

    /// Port of `SVG_PLOTTER::PlotText`: forwards to `SVG_PLOTTER::Text` with the mirror flag
    /// encoded as a negative X size.
    fn plot_text(
        &mut self,
        a_pos: Vector2I,
        a_color: &Color4D,
        a_text: &str,
        a_attributes: &TextAttributes,
        a_font: Option<&dyn Font>,
        a_font_metrics: &Metrics,
        a_data: PlotData,
    ) {
        let mut size = Vector2I::new(a_attributes.size.0, a_attributes.size.1);

        if a_attributes.mirrored {
            size.x = size.x.wrapping_neg();
        }

        self.text(
            a_pos,
            a_color,
            a_text,
            EdaAngle::from_degrees(a_attributes.angle),
            size,
            a_attributes.halign,
            a_attributes.valign,
            a_attributes.stroke_width,
            a_attributes.italic,
            a_attributes.bold,
            a_attributes.multiline,
            a_font,
            a_font_metrics,
            a_data,
        );
    }
}
