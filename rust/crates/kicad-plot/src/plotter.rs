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
//
// Original file: Copyright (C) 2016 Jean-Pierre Charras, jp.charras at wanadoo.fr

//! Port of `include/plotters/plotter.h` and `common/plotters/plotter.cpp` L1-546: the
//! `PLOTTER` base class.
//!
//! `PLOTTER` becomes the trait [`Plotter`] plus the shared state struct [`PlotterState`]
//! (the protected members). Every C++ virtual is a trait method; where the base class has an
//! implementation the trait method is provided and forwards to a free `*_base` function, so
//! that an engine overriding the method can still call `PLOTTER::X` explicitly (as
//! `GERBER_PLOTTER::ThickSegment` calls `PLOTTER::ThickSegment`). Pure virtuals are required
//! methods. Non-virtual C++ members (`MoveTo`, `Marker`, ...) are provided methods that must
//! not be overridden. See `rust/docs/design/plot-engines.md`.
//!
//! General rule: all the interface with the caller is done in IU, the IU size is specified
//! with `SetViewport`. Internal and output processing is usually done in decimils (or
//! whatever unit the effective engine class needs to use).
//!
//! The C++ `void* aData` parameter is `Option<&GbrMetadata>` (it is a `GBR_METADATA*` for the
//! Gerber plotter, a `PLOT_PARAMS*` only for DXF, which is out of scope).
//!
//! Output: C++ writes to a `FILE*` opened by `OpenFile`; the port writes into the in-memory
//! [`PlotterState::output`] string and the caller writes the bytes to a file.
//!
//! Not here: `Text` / `PlotText` (`plotter.cpp` L548-667, work package PLOT-05, they will be
//! added as provided methods), `ThickArc( const EDA_SHAPE& )` (no caller in the board plot
//! code), the PDF-only hyperlink/bookmark hooks, `m_project`, `m_layersToExport` and
//! `m_plottedFootprints`. See `PORT_STATUS.md`.

use kicad_core::color4d::Color4D;
use kicad_core::page_info::PageInfo;
use kicad_core::plot_enums::{FillT, PlotFormat, PlotTextMode};
use kicad_core::stroke_params::LineStyle;
use kicad_core::{PcbLayerId, UNDEFINED_LAYER};
use kicad_geom::bezier_curves::BezierPoly;
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, Box2I, DEGREES_T, EdaAngle, ShapeLineChain, ShapePolySet,
    Vector2D, Vector2I, calc_arc_center_d, ki_round, rotate_point,
};

use crate::drill_markers::{MarkerPartType, build_marker};
use crate::plotter_text::{plot_text_base, text_base};
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::{Font, Metrics};
use crate::gbr_metadata::GbrMetadata;
use crate::render_settings::RenderSettings;

/// `PLOTTER::DO_NOT_SET_LINE_WIDTH`: skip pen or aperture selection.
pub const DO_NOT_SET_LINE_WIDTH: i32 = -2;

/// `PLOTTER::USE_DEFAULT_LINE_WIDTH`: use the default pen.
pub const USE_DEFAULT_LINE_WIDTH: i32 = -1;

/// `PLOTTER::MARKER_COUNT`: number of drill marker shapes.
pub const MARKER_COUNT: u32 = 58;

/// The auxiliary data parameter (`void* aData`) of the plot primitives.
pub type PlotData<'a> = Option<&'a GbrMetadata>;

/// The protected members of `PLOTTER` ("variables used in most of plotters").
#[derive(Clone, Debug)]
pub struct PlotterState {
    /// `m_plotScale`: plot scale - chosen by the user (even implicitly with 'fit in a4')
    pub plot_scale: f64,
    /// `m_IUsPerDecimil`: caller scale (how many IUs in a decimil - always); it's a double
    /// because in Eeschema there are 0.1 IUs in a decimil (Eeschema always works in mils
    /// internally) while PcbNew can work in decimil or nanometers, so this value would be
    /// >= 1
    pub ius_per_decimil: f64,
    /// `m_iuPerDeviceUnit`: device scale (from IUs to plotter device units; usually decimils)
    pub iu_per_device_unit: f64,
    /// `m_plotOffset`: plot offset (in IUs)
    pub plot_offset: Vector2I,
    /// `m_plotMirror`: X axis orientation (SVG) and plot mirrored (only for PS, PDF and SVG)
    pub plot_mirror: bool,
    /// `m_mirrorIsHorizontal`: true to mirror horizontally (else vertically)
    pub mirror_is_horizontal: bool,
    /// `m_yaxisReversed`: true if the Y axis is top to bottom (SVG)
    pub yaxis_reversed: bool,
    /// The output sink replacing `m_outputFile`.
    pub output: String,
    /// `m_outputFile != nullptr`: set by [`Plotter::open_file`], cleared by the engine's
    /// `EndPlot`.
    pub output_open: bool,
    /// `m_colorMode`: true to plot in color, otherwise black & white
    pub color_mode: bool,
    /// `m_negativeMode`: true to generate a negative image (PS mode mainly)
    pub negative_mode: bool,
    /// `m_currentPenWidth`
    pub current_pen_width: i32,
    /// `m_penState`: current pen state: 'U', 'D' or 'Z' (see `PenTo`)
    pub pen_state: char,
    /// `m_penLastpos`: last pen position; -1,-1 when pen is at rest
    pub pen_lastpos: Vector2I,
    /// `m_creator`
    pub creator: String,
    /// `m_filename`
    pub filename: String,
    /// `m_title`
    pub title: String,
    /// `m_author`
    pub author: String,
    /// `m_subject`
    pub subject: String,
    /// `m_pageInfo`
    pub page_info: PageInfo,
    /// `m_paperSize`: paper size in IU - not in mils
    pub paper_size: Vector2I,
    /// `m_headerExtraLines`: a set of string to print in header file
    pub header_extra_lines: Vec<String>,
    /// `m_renderSettings` (a non-owning pointer in C++, null until set)
    pub render_settings: Option<RenderSettings>,
    /// `m_layer`
    pub layer: PcbLayerId,
    /// Port-only: the outline (TrueType) font names that `Text` / `PlotText` were asked to use
    /// through the render settings' default font and that were replaced by the stroke font,
    /// because this port has no outline font engine (`kicad-font` D1). Empty for every plot
    /// that only uses the stroke font. Callers report these substitutions.
    pub outline_font_fallbacks: Vec<String>,
}

impl Default for PlotterState {
    fn default() -> Self {
        Self::new()
    }
}

impl PlotterState {
    /// Port of `PLOTTER::PLOTTER()`.
    pub fn new() -> Self {
        PlotterState {
            plot_scale: 1.0,
            // Temporary init to avoid not initialized vars, will be set later
            ius_per_decimil: 1.0,
            iu_per_device_unit: 1.0,
            plot_offset: Vector2I::new(0, 0),
            plot_mirror: false,
            mirror_is_horizontal: true,
            yaxis_reversed: false,
            output: String::new(),
            output_open: false,
            color_mode: false, // Starts as a BW plot
            negative_mode: false,
            current_pen_width: -1, // To-be-set marker
            pen_state: 'Z',        // End-of-path idle
            pen_lastpos: Vector2I::new(0, 0),
            creator: String::new(),
            filename: String::new(),
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            page_info: PageInfo::default(),
            paper_size: Vector2I::new(0, 0),
            header_extra_lines: Vec::new(),
            render_settings: None,
            layer: UNDEFINED_LAYER,
            outline_font_fallbacks: Vec::new(),
        }
    }

    /// `m_renderSettings`, or the default settings when none were set (C++ would dereference
    /// a null pointer).
    pub fn render_settings_or_default(&self) -> RenderSettings {
        self.render_settings.clone().unwrap_or_default()
    }
}

/// Port of `PLOTTER`: the base plotter engine.
///
/// Required methods are the C++ pure virtuals. Provided methods are either C++ virtuals with
/// a base implementation (override freely; the base stays reachable through the `*_base`
/// free functions of this module) or non-virtual C++ members (marked "non-virtual", do not
/// override).
pub trait Plotter {
    /// Access to the shared base-class members.
    fn state(&self) -> &PlotterState;

    /// Mutable access to the shared base-class members.
    fn state_mut(&mut self) -> &mut PlotterState;

    /// Down-cast to the Gerber engine (C++ `dynamic_cast<GERBER_PLOTTER*>`): `None` for every
    /// other engine.
    fn as_gerber_mut(&mut self) -> Option<&mut crate::plotter_gerber::GerberPlotter> {
        None
    }

    /// Port of `PLOTTER::GetPlotterType`: the effective plot engine in use.
    fn plotter_type(&self) -> PlotFormat;

    /// Port of `PLOTTER::StartPlot`.
    fn start_plot(&mut self, a_page_number: &str) -> bool;

    /// Port of `PLOTTER::EndPlot`.
    fn end_plot(&mut self) -> bool;

    /// Port of `PLOTTER::SetNegative`.
    fn set_negative(&mut self, a_negative: bool) {
        self.state_mut().negative_mode = a_negative;
    }

    /// Port of `PLOTTER::SetColorMode`: plot in B/W (`false`) or color (`true`).
    fn set_color_mode(&mut self, a_color_mode: bool) {
        self.state_mut().color_mode = a_color_mode;
    }

    /// Port of `PLOTTER::GetColorMode` (non-virtual).
    fn color_mode(&self) -> bool {
        self.state().color_mode
    }

    /// Port of `PLOTTER::SetRenderSettings` (non-virtual). The settings are copied.
    fn set_render_settings(&mut self, a_settings: Option<RenderSettings>) {
        self.state_mut().render_settings = a_settings;
    }

    /// Port of `PLOTTER::RenderSettings` (non-virtual).
    fn render_settings(&self) -> Option<&RenderSettings> {
        self.state().render_settings.as_ref()
    }

    /// Port of `PLOTTER::SetPageSettings`.
    fn set_page_settings(&mut self, a_page_settings: &PageInfo) {
        self.state_mut().page_info = a_page_settings.clone();
    }

    /// Port of `PLOTTER::PageSettings` (non-virtual).
    fn page_settings(&self) -> &PageInfo {
        &self.state().page_info
    }

    /// Port of `PLOTTER::SetPlotMirrored` (non-virtual).
    fn set_plot_mirrored(&mut self, a_mirror: bool) {
        self.state_mut().plot_mirror = a_mirror;
    }

    /// Port of `PLOTTER::GetPlotMirrored` (non-virtual).
    fn plot_mirrored(&self) -> bool {
        self.state().plot_mirror
    }

    /// Port of `PLOTTER::SetCurrentLineWidth`: set the line width (in IUs) for the next
    /// drawing. `a_data` is an auxiliary parameter, mainly used in gerber plotter.
    fn set_current_line_width(&mut self, width: i32, a_data: PlotData);

    /// Port of `PLOTTER::GetCurrentLineWidth`.
    fn current_line_width(&self) -> i32 {
        self.state().current_pen_width
    }

    /// Port of `PLOTTER::SetColor`.
    fn set_color(&mut self, color: &Color4D);

    /// Port of `PLOTTER::SetDash`.
    fn set_dash(&mut self, a_line_width: i32, a_line_style: LineStyle);

    /// Port of `PLOTTER::SetCreator`.
    fn set_creator(&mut self, a_creator: &str) {
        self.state_mut().creator = a_creator.to_string();
    }

    /// Port of `PLOTTER::SetTitle`.
    fn set_title(&mut self, a_title: &str) {
        self.state_mut().title = a_title.to_string();
    }

    /// Port of `PLOTTER::SetAuthor`.
    fn set_author(&mut self, a_author: &str) {
        self.state_mut().author = a_author.to_string();
    }

    /// Port of `PLOTTER::SetSubject`.
    fn set_subject(&mut self, a_subject: &str) {
        self.state_mut().subject = a_subject.to_string();
    }

    /// Port of `PLOTTER::AddLineToHeader` (non-virtual): add a line to the list of free lines
    /// to print at the beginning of the file.
    fn add_line_to_header(&mut self, a_extra_string: &str) {
        self.state_mut()
            .header_extra_lines
            .push(a_extra_string.to_string());
    }

    /// Port of `PLOTTER::ClearHeaderLinesList` (non-virtual).
    fn clear_header_lines_list(&mut self) {
        self.state_mut().header_extra_lines.clear();
    }

    /// Port of `PLOTTER::SetViewport`: set the plot offset and scaling for the current plot.
    ///
    /// `a_offset` is the plot offset, `a_ius_per_decimil` gives the scaling factor from IUs
    /// to device units, `a_scale` is the user set plot scaling factor (either explicitly or
    /// using 'fit to A4'), `a_mirror` flips the plot in the Y direction (useful for toner
    /// transfers or some kind of film).
    fn set_viewport(
        &mut self,
        a_offset: Vector2I,
        a_ius_per_decimil: f64,
        a_scale: f64,
        a_mirror: bool,
    );

    /// Port of `PLOTTER::GetLayer` (non-virtual).
    fn layer(&self) -> PcbLayerId {
        self.state().layer
    }

    /// Port of `PLOTTER::SetLayer` (non-virtual).
    fn set_layer(&mut self, a_layer: PcbLayerId) {
        self.state_mut().layer = a_layer;
    }

    /// Port of `PLOTTER::OpenFile`: the C++ opens `a_full_filename` for writing. The port only
    /// records the file name and starts an empty output buffer; the caller writes
    /// [`PlotterState::output`] to the file. Always succeeds.
    fn open_file(&mut self, a_full_filename: &str) -> bool {
        let state = self.state_mut();
        state.filename = a_full_filename.to_string();
        debug_assert!(!state.output_open);
        state.output.clear();
        state.output_open = true;
        true
    }

    /// The plot output written so far.
    fn output(&self) -> &str {
        &self.state().output
    }

    /// Port of `PLOTTER::GetIUsPerDecimil` (non-virtual): the IUs per decimil are an
    /// essential scaling factor when plotting; they are set and saved when establishing the
    /// viewport.
    fn ius_per_decimil(&self) -> f64 {
        self.state().ius_per_decimil
    }

    /// Port of `PLOTTER::GetPlotterArcLowDef` (non-virtual).
    fn plotter_arc_low_def(&self) -> i32 {
        f64_to_i32_x86(self.state().ius_per_decimil * 8.0)
    }

    /// Port of `PLOTTER::GetPlotterArcHighDef` (non-virtual).
    fn plotter_arc_high_def(&self) -> i32 {
        f64_to_i32_x86(self.state().ius_per_decimil * 2.0)
    }

    // Low level primitives

    /// Port of `PLOTTER::Rect` (C++ default `aCornerRadius = 0`).
    fn rect(&mut self, p1: Vector2I, p2: Vector2I, fill: FillT, width: i32, a_corner_radius: i32);

    /// Port of `PLOTTER::Circle`.
    fn circle(&mut self, pos: Vector2I, diametre: i32, fill: FillT, width: i32);

    /// Port of `PLOTTER::Arc( const VECTOR2D& aStart, const VECTOR2D& aMid, const VECTOR2D&
    /// aEnd, ... )`: arc by three points.
    fn arc_3pt(
        &mut self,
        a_start: Vector2D,
        a_mid: Vector2D,
        a_end: Vector2D,
        a_fill: FillT,
        a_width: i32,
    ) {
        arc_3pt_base(self, a_start, a_mid, a_end, a_fill, a_width);
    }

    /// Port of `PLOTTER::Arc( const VECTOR2D& aCenter, const EDA_ANGLE& aStartAngle, const
    /// EDA_ANGLE& aAngle, double aRadius, ... )`: arc by centre, angles and radius.
    fn arc(
        &mut self,
        a_center: Vector2D,
        a_start_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_fill: FillT,
        a_width: i32,
    ) {
        arc_base(
            self,
            a_center,
            a_start_angle,
            a_angle,
            a_radius,
            a_fill,
            a_width,
        );
    }

    /// Port of `PLOTTER::BezierCurve`: generic fallback, cubic Bezier curve rendered as a
    /// polyline. In KiCad the bezier curves have 4 control points: start ctrl1 ctrl2 end.
    fn bezier_curve(
        &mut self,
        a_start: Vector2I,
        a_control1: Vector2I,
        a_control2: Vector2I,
        a_end: Vector2I,
        a_tolerance: i32,
        a_line_thickness: i32,
    ) {
        bezier_curve_base(
            self,
            a_start,
            a_control1,
            a_control2,
            a_end,
            a_tolerance,
            a_line_thickness,
        );
    }

    /// Port of `PLOTTER::PenTo`: moveto/lineto primitive, moves the 'pen' to the specified
    /// direction. `plume` specifies the kind of motion: 'U' only moves the pen, 'D' draws a
    /// line from the current position and 'Z' finishes the drawing and returns the 'pen' to
    /// rest (flushes the trace).
    fn pen_to(&mut self, pos: Vector2I, plume: char);

    /// Port of `PLOTTER::MoveTo` (non-virtual).
    fn move_to(&mut self, pos: Vector2I) {
        self.pen_to(pos, 'U');
    }

    /// Port of `PLOTTER::LineTo` (non-virtual).
    fn line_to(&mut self, pos: Vector2I) {
        self.pen_to(pos, 'D');
    }

    /// Port of `PLOTTER::FinishTo` (non-virtual).
    fn finish_to(&mut self, pos: Vector2I) {
        self.pen_to(pos, 'D');
        self.pen_to(pos, 'Z');
    }

    /// Port of `PLOTTER::PenFinish` (non-virtual).
    fn pen_finish(&mut self) {
        // The point is not important with Z motion
        self.pen_to(Vector2I::new(0, 0), 'Z');
    }

    /// Port of `PLOTTER::PlotPoly( const std::vector<VECTOR2I>& aCornerList, ... )`: draw a
    /// polygon (filled or not). `a_data` is an auxiliary info (mainly for gerber format).
    fn plot_poly(
        &mut self,
        a_corner_list: &[Vector2I],
        a_fill: FillT,
        a_width: i32,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::PlotPoly( const SHAPE_LINE_CHAIN& aLineChain, ... )`: draw a polygon
    /// (filled or not). `a_line_chain` is a list of segments and arcs; it must be closed
    /// (`IsClosed() == true`) for a polygon, otherwise this is a polyline.
    fn plot_poly_chain(
        &mut self,
        a_line_chain: &ShapeLineChain,
        a_fill: FillT,
        a_width: i32,
        a_data: PlotData,
    ) {
        plot_poly_chain_base(self, a_line_chain, a_fill, a_width, a_data);
    }

    /// Port of `PLOTTER::PlotImage`: plot a bitmap. Plotters that cannot embed a bitmap plot
    /// a placeholder rectangle instead.
    ///
    /// The `wxImage` is replaced by its pixel size (`a_image_width`, `a_image_height`); the
    /// pixel data is not needed by the base implementation (bitmap embedding is out of
    /// scope). `a_pos` is the position of the center of the bitmap, `a_scale_factor` the scale
    /// factor to apply to the bitmap size (not the plot scale factor), `a_alpha` the opacity.
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

    // Higher level primitives -- can be drawn as line, sketch or 'filled'

    /// Port of `PLOTTER::ThickSegment`.
    fn thick_segment(&mut self, start: Vector2I, end: Vector2I, width: i32, a_data: PlotData) {
        thick_segment_base(self, start, end, width, a_data);
    }

    /// Port of `PLOTTER::ThickArc( const VECTOR2D& aCentre, const EDA_ANGLE& aStAngle, const
    /// EDA_ANGLE& aAngle, double aRadius, int aWidth, void* aData )`.
    fn thick_arc(
        &mut self,
        a_centre: Vector2D,
        a_st_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_width: i32,
        a_data: PlotData,
    ) {
        thick_arc_base(
            self, a_centre, a_st_angle, a_angle, a_radius, a_width, a_data,
        );
    }

    /// Port of `PLOTTER::ThickRect`.
    fn thick_rect(&mut self, p1: Vector2I, p2: Vector2I, width: i32, a_data: PlotData) {
        thick_rect_base(self, p1, p2, width, a_data);
    }

    /// Port of `PLOTTER::ThickCircle`.
    fn thick_circle(&mut self, pos: Vector2I, diametre: i32, width: i32, a_data: PlotData) {
        thick_circle_base(self, pos, diametre, width, a_data);
    }

    /// Port of `PLOTTER::FilledCircle`.
    fn filled_circle(&mut self, pos: Vector2I, diametre: i32, a_data: PlotData) {
        filled_circle_base(self, pos, diametre, a_data);
    }

    /// Port of `PLOTTER::ThickOval`.
    fn thick_oval(
        &mut self,
        a_pos: Vector2I,
        a_size: Vector2I,
        a_orient: EdaAngle,
        a_width: i32,
        a_data: PlotData,
    ) {
        thick_oval_base(self, a_pos, a_size, a_orient, a_width, a_data);
    }

    /// Port of `PLOTTER::ThickPoly`.
    fn thick_poly(&mut self, a_poly: &ShapePolySet, a_width: i32, a_data: PlotData) {
        thick_poly_base(self, a_poly, a_width, a_data);
    }

    // Flash primitives

    /// Port of `PLOTTER::FlashPadCircle`: `a_pad_pos` is the position of the shape,
    /// `a_diameter` the diameter of round pad, `a_data` an auxiliary info (mainly for gerber
    /// format attributes).
    fn flash_pad_circle(&mut self, a_pad_pos: Vector2I, a_diameter: i32, a_data: PlotData);

    /// Port of `PLOTTER::FlashPadOval`: `a_size` is the size of oblong shape, `a_pad_orient`
    /// the rotation of the shape.
    fn flash_pad_oval(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::FlashPadRect`.
    fn flash_pad_rect(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::FlashPadRoundRect`: `a_corner_radius` is the radius of the rounded
    /// corners.
    fn flash_pad_round_rect(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_corner_radius: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::FlashPadCustom`: `a_size` is the size of round reference pad,
    /// `a_pad_orient` the pad rotation, used only with aperture macros (Gerber plotter),
    /// `a_polygons` the shape as polygon set.
    fn flash_pad_custom(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_pad_orient: EdaAngle,
        a_polygons: &ShapePolySet,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::FlashPadTrapez`: flash a trapezoidal pad. `a_corners` is the list of
    /// 4 corners positions, relative to the shape position, pad orientation 0.
    fn flash_pad_trapez(
        &mut self,
        a_pad_pos: Vector2I,
        a_corners: &[Vector2I; 4],
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::FlashRegularPolygon`: flash a regular polygon. Useful only in Gerber
    /// files. `a_diameter` is the diameter of the circle containing the polygon,
    /// `a_corner_count` the number of vertices.
    fn flash_regular_polygon(
        &mut self,
        a_shape_pos: Vector2I,
        a_diameter: i32,
        a_corner_count: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    );

    /// Port of `PLOTTER::Text`: draw a text with the stroke font (or the polygons of an
    /// outline font) through the plotter primitives.
    ///
    /// `a_size.x < 0` means mirrored text (there is no other parameter for it).
    /// `a_pen_width` is the stroke width (0 and bold: the default bold pen; negative: its
    /// absolute value). `a_multiline_allowed` is unused, as in C++ (the attributes keep their
    /// default `m_Multiline = true`). `a_font` `None` means the render settings' default font.
    /// `a_data` is the Gerber metadata, given to the polygons of outline glyphs only.
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
        a_pen_width: i32,
        a_italic: bool,
        a_bold: bool,
        a_multiline_allowed: bool,
        a_font: Option<&dyn Font>,
        a_font_metrics: &Metrics,
        a_data: PlotData,
    ) {
        text_base(
            self,
            a_pos,
            a_color,
            a_text,
            a_orient,
            a_size,
            a_h_justify,
            a_v_justify,
            a_pen_width,
            a_italic,
            a_bold,
            a_multiline_allowed,
            a_font,
            a_font_metrics,
            a_data,
        );
    }

    /// Port of `PLOTTER::PlotText`: draw a text described by its attributes. The pen
    /// (aperture) is selected with `a_attributes.stroke_width` and `a_data` *before* the bold
    /// / negative width adjustment.
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
        plot_text_base(
            self,
            a_pos,
            a_color,
            a_text,
            a_attributes,
            a_font,
            a_font_metrics,
            a_data,
        );
    }

    /// Port of `PLOTTER::Marker` (non-virtual): draw a pattern shape number `a_shape_id`, to
    /// coord position (used for the drill map).
    fn marker(&mut self, position: Vector2I, diametre: i32, a_shape_id: u32) {
        let radius = diametre / 2;

        for part in build_marker(position, radius, a_shape_id) {
            match part.type_ {
                MarkerPartType::Segment => {
                    self.move_to(part.points[0]);
                    self.finish_to(part.points[part.points.len() - 1]);
                }
                MarkerPartType::Polyline => {
                    let width = self.current_line_width();
                    self.plot_poly(&part.points, FillT::NoFill, width, None);
                }
                MarkerPartType::Circle => {
                    let width = self.current_line_width();
                    self.circle(position, part.radius * 2, FillT::NoFill, width);
                }
            }
        }
    }

    /// Port of `PLOTTER::SetLayerPolarity`: set the current Gerber layer polarity to positive
    /// or negative by writing `%LPD*%` or `%LPC*%` to the Gerber file, respectively. NOP for
    /// most plotters since they can't 'scratch' the film like photoplotter imagers do.
    fn set_layer_polarity(&mut self, _a_positive: bool) {
        // NOP for most plotters
    }

    /// Port of `PLOTTER::SetTextMode`: change the current text mode.
    fn set_text_mode(&mut self, _mode: PlotTextMode) {
        // NOP for most plotters.
    }

    /// Port of `PLOTTER::SetGerberCoordinatesFormat` (C++ default `aUseInches = false`).
    fn set_gerber_coordinates_format(&mut self, _a_resolution: i32, _a_use_inches: bool) {
        // NOP for most plotters. Only for Gerber plotter
    }

    /// Port of `PLOTTER::SetSvgCoordinatesFormat`: set the number of digits for mantissa in
    /// coordinates in mm for SVG plotter.
    fn set_svg_coordinates_format(&mut self, _a_precision: u32) {
        // NOP for most plotters. Only for SVG plotter
    }

    /// Port of `PLOTTER::SetPlotBBox`: set an explicit bounding box for the plotted content
    /// (in IUs). Used by the SVG plotter to derive its viewBox from the content instead of
    /// the page size, so the content origin can stay at the SVG origin.
    fn set_plot_bbox(&mut self, _a_bbox_iu: &Box2I) {
        // NOP for most plotters. Only for SVG plotter
    }

    /// Port of `PLOTTER::StartBlock`: define the beginning of a group of drawing items, for
    /// instance in SVG or Gerber format (example: group all segments of a letter or a text).
    /// For most of plotters: do nothing.
    fn start_block(&mut self, _a_data: PlotData) {}

    /// Port of `PLOTTER::EndBlock`: define the end of a group of drawing items started by
    /// `StartBlock()`. For most of plotters: do nothing.
    fn end_block(&mut self, _a_data: PlotData) {}

    /// Port of `PLOTTER::GetPlotOffsetUserUnits` (non-virtual): the plot offset in IUs, set
    /// by `SetViewport()` and used to plot items.
    fn plot_offset_user_units(&self) -> Vector2I {
        self.state().plot_offset
    }

    /// Port of `PLOTTER::GetPlotScale` (non-virtual): the plot scale set by `SetViewport()`.
    fn plot_scale(&self) -> f64 {
        self.state().plot_scale
    }

    /// Port of `PLOTTER::polyArc` (protected): generic fallback, arc rendered as a polyline.
    ///
    /// Note also `a_centre` and `a_radius` are double to avoid creating rounding issues due to
    /// the fact an arc is defined in KiCad by a start point, an end point and third point,
    /// not angles and radius. In some plotters (i.e. dxf) we need a good precision when
    /// calculating an arc without error introduced by rounding, to avoid moving the end
    /// points, usually important in outlines when plotting an arc given by center, radius and
    /// angles. Winding direction: counter-clockwise in right-down coordinate system.
    fn poly_arc(
        &mut self,
        a_centre: Vector2D,
        a_start_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_fill: FillT,
        a_width: i32,
    ) {
        poly_arc_base(
            self,
            a_centre,
            a_start_angle,
            a_angle,
            a_radius,
            a_fill,
            a_width,
        );
    }

    /// Port of `PLOTTER::markerCircle` (protected, non-virtual): plot a circle centered on
    /// the position. Building block for markers.
    fn marker_circle(&mut self, position: Vector2I, radius: i32) {
        let width = self.current_line_width();
        self.circle(position, radius * 2, FillT::NoFill, width);
    }

    /// Port of `PLOTTER::markerHBar` (protected, non-virtual): plot a - bar centered on the
    /// position.
    fn marker_h_bar(&mut self, pos: Vector2I, radius: i32) {
        self.move_to(Vector2I::new(pos.x - radius, pos.y));
        self.finish_to(Vector2I::new(pos.x + radius, pos.y));
    }

    /// Port of `PLOTTER::markerSlash` (protected, non-virtual): plot a / bar centered on the
    /// position.
    fn marker_slash(&mut self, pos: Vector2I, radius: i32) {
        self.move_to(Vector2I::new(pos.x - radius, pos.y - radius));
        self.finish_to(Vector2I::new(pos.x + radius, pos.y + radius));
    }

    /// Port of `PLOTTER::markerBackSlash` (protected, non-virtual): plot a \ bar centered on
    /// the position.
    fn marker_back_slash(&mut self, pos: Vector2I, radius: i32) {
        self.move_to(Vector2I::new(pos.x + radius, pos.y - radius));
        self.finish_to(Vector2I::new(pos.x - radius, pos.y + radius));
    }

    /// Port of `PLOTTER::markerVBar` (protected, non-virtual): plot a | bar centered on the
    /// position.
    fn marker_v_bar(&mut self, pos: Vector2I, radius: i32) {
        self.move_to(Vector2I::new(pos.x, pos.y - radius));
        self.finish_to(Vector2I::new(pos.x, pos.y + radius));
    }

    /// Port of `PLOTTER::markerSquare` (protected, non-virtual): plot a square centered on
    /// the position.
    fn marker_square(&mut self, position: Vector2I, radius: i32) {
        let r: f64 = ki_round(radius as f64 / crate::drill_markers::SQRT2_APPROX) as f64;
        // int + double assigned to an int member: truncation
        let at = |px: i32, py: i32, dx: f64, dy: f64| {
            Vector2I::new(
                f64_to_i32_x86(px as f64 + dx),
                f64_to_i32_x86(py as f64 + dy),
            )
        };
        let corner_list = vec![
            at(position.x, position.y, r, r),
            at(position.x, position.y, r, -r),
            at(position.x, position.y, -r, -r),
            at(position.x, position.y, -r, r),
            at(position.x, position.y, r, r),
        ];

        let width = self.current_line_width();
        self.plot_poly(&corner_list, FillT::NoFill, width, None);
    }

    /// Port of `PLOTTER::markerLozenge` (protected, non-virtual): plot a lozenge centered on
    /// the position.
    fn marker_lozenge(&mut self, position: Vector2I, radius: i32) {
        let corner_list = vec![
            Vector2I::new(position.x, position.y + radius),
            Vector2I::new(position.x + radius, position.y),
            Vector2I::new(position.x, position.y - radius),
            Vector2I::new(position.x - radius, position.y),
            Vector2I::new(position.x, position.y + radius),
        ];

        let width = self.current_line_width();
        self.plot_poly(&corner_list, FillT::NoFill, width, None);
    }

    // Coordinate and scaling conversion functions

    /// Port of `PLOTTER::userToDeviceCoordinates` (protected): modify coordinates according
    /// to the orientation, scale factor, and offsets trace. Also convert from a `VECTOR2I` to
    /// `VECTOR2D`, since some output engines needs floating point coordinates.
    fn user_to_device_coordinates(&mut self, a_coordinate: Vector2I) -> Vector2D {
        user_to_device_coordinates_base(self.state(), a_coordinate)
    }

    /// Port of `PLOTTER::userToDeviceSize( const VECTOR2I& )` (protected): modify size
    /// according to the plotter scale factors (returns a `VECTOR2D`).
    fn user_to_device_size(&mut self, size: Vector2I) -> Vector2D {
        user_to_device_size_base(self.state(), size)
    }

    /// Port of `PLOTTER::userToDeviceSize( double )` (protected): modify size according to the
    /// plotter scale factors (simple double version).
    fn user_to_device_size_f(&self, size: f64) -> f64 {
        user_to_device_size_f_base(self.state(), size)
    }

    /// Port of `PLOTTER::GetDotMarkLenIU` (protected, non-virtual).
    fn dot_mark_len_iu(&self, a_line_width: i32) -> f64 {
        let len = self
            .state()
            .render_settings_or_default()
            .dot_length(a_line_width);
        self.user_to_device_size_f(len)
    }

    /// Port of `PLOTTER::GetDashMarkLenIU` (protected, non-virtual).
    fn dash_mark_len_iu(&self, a_line_width: i32) -> f64 {
        let len = self
            .state()
            .render_settings_or_default()
            .dash_length(a_line_width);
        self.user_to_device_size_f(len)
    }

    /// Port of `PLOTTER::GetDashGapLenIU` (protected, non-virtual).
    fn dash_gap_len_iu(&self, a_line_width: i32) -> f64 {
        let len = self
            .state()
            .render_settings_or_default()
            .gap_length(a_line_width);
        self.user_to_device_size_f(len)
    }
}

/// Port of the base implementation of `PLOTTER::userToDeviceCoordinates`.
pub fn user_to_device_coordinates_base(s: &PlotterState, a_coordinate: Vector2I) -> Vector2D {
    // int subtraction: wraps on the reference build (x86-64) for the clamped VECTOR2I of a far
    // away arc centre (`Arc( start, mid, end )` of nearly collinear points)
    let pos = Vector2I::new(
        a_coordinate.x.wrapping_sub(s.plot_offset.x),
        a_coordinate.y.wrapping_sub(s.plot_offset.y),
    );

    let mut x = pos.x as f64 * s.plot_scale;
    let mut y = s.paper_size.y as f64 - pos.y as f64 * s.plot_scale;

    if s.plot_mirror {
        if s.mirror_is_horizontal {
            x = s.paper_size.x as f64 - pos.x as f64 * s.plot_scale;
        } else {
            y = pos.y as f64 * s.plot_scale;
        }
    }

    if s.yaxis_reversed {
        y = s.paper_size.y as f64 - y;
    }

    x *= s.iu_per_device_unit;
    y *= s.iu_per_device_unit;

    Vector2D::new(x, y)
}

/// Port of the base implementation of `PLOTTER::userToDeviceSize( const VECTOR2I& )`.
pub fn user_to_device_size_base(s: &PlotterState, size: Vector2I) -> Vector2D {
    Vector2D::new(
        size.x as f64 * s.plot_scale * s.iu_per_device_unit,
        size.y as f64 * s.plot_scale * s.iu_per_device_unit,
    )
}

/// Port of the base implementation of `PLOTTER::userToDeviceSize( double )`.
pub fn user_to_device_size_f_base(s: &PlotterState, size: f64) -> f64 {
    size * s.plot_scale * s.iu_per_device_unit
}

/// Port of the base implementation of `PLOTTER::Arc( aStart, aMid, aEnd, aFill, aWidth )`.
pub fn arc_3pt_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_start: Vector2D,
    a_mid: Vector2D,
    a_end: Vector2D,
    a_fill: FillT,
    a_width: i32,
) {
    let a_center = calc_arc_center_d(a_start, a_mid, a_end);

    let start_angle = EdaAngle::from_vector(a_start - a_center);
    let end_angle = EdaAngle::from_vector(a_end - a_center);

    // < 0: left, 0 : on the line, > 0 : right
    let det = (a_end - a_start).cross(a_mid - a_start);

    let cw = det <= 0.0;
    let mut angle = end_angle - start_angle;

    if cw {
        angle.normalize();
    } else {
        angle.normalize_negative();
    }

    let radius = (a_start - a_center).euclidean_norm();
    p.arc(a_center, start_angle, angle, radius, a_fill, a_width);
}

/// Port of the base implementation of `PLOTTER::Arc( aCenter, aStartAngle, aAngle, aRadius,
/// aFill, aWidth )`.
pub fn arc_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_center: Vector2D,
    a_start_angle: EdaAngle,
    a_angle: EdaAngle,
    a_radius: f64,
    a_fill: FillT,
    a_width: i32,
) {
    p.poly_arc(a_center, a_start_angle, a_angle, a_radius, a_fill, a_width);
}

/// Port of the base implementation of `PLOTTER::polyArc`.
pub fn poly_arc_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_center: Vector2D,
    a_start_angle: EdaAngle,
    a_angle: EdaAngle,
    a_radius: f64,
    a_fill: FillT,
    a_width: i32,
) {
    let mut start_angle = a_start_angle;
    let mut end_angle = start_angle + a_angle;
    let delta = EdaAngle::new(5.0, DEGREES_T); // increment to draw arc
    let sign: i32 = 1;

    if a_angle < ANGLE_0 {
        std::mem::swap(&mut start_angle, &mut end_angle);
    }

    p.set_current_line_width(a_width, None);

    let start = Vector2I::new(
        ki_round(a_center.x + a_radius * start_angle.cos()),
        ki_round(a_center.y + sign as f64 * a_radius * start_angle.sin()),
    );

    // VECTOR2I( VECTOR2D ) truncates
    let center_i: Vector2I = a_center.cast();

    if a_fill != FillT::NoFill {
        p.move_to(center_i);
        p.line_to(start);
    } else {
        p.move_to(start);
    }

    let mut ii = start_angle + delta;

    while ii < end_angle {
        let end = Vector2I::new(
            ki_round(a_center.x + a_radius * ii.cos()),
            ki_round(a_center.y + sign as f64 * a_radius * ii.sin()),
        );
        p.line_to(end);
        ii += delta;
    }

    let end = Vector2I::new(
        ki_round(a_center.x + a_radius * end_angle.cos()),
        ki_round(a_center.y + sign as f64 * a_radius * end_angle.sin()),
    );

    if a_fill != FillT::NoFill {
        p.line_to(end);
        p.finish_to(center_i);
    } else {
        p.finish_to(end);
    }
}

/// Port of the base implementation of `PLOTTER::BezierCurve`: generic fallback, Bezier curve
/// plotted as a polyline (`BEZIER_POLY::GetPoly` with `a_tolerance` as maximum error).
pub fn bezier_curve_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_start: Vector2I,
    a_control1: Vector2I,
    a_control2: Vector2I,
    a_end: Vector2I,
    a_tolerance: i32,
    a_line_thickness: i32,
) {
    let ctrl_points = [a_start, a_control1, a_control2, a_end];
    let bezier_converter = BezierPoly::from_points_i(&ctrl_points);

    let mut approx_points: Vec<Vector2I> = Vec::new();
    bezier_converter.get_poly_i(&mut approx_points, a_tolerance);

    bezier_curve_from_points(p, a_start, a_end, &approx_points, a_line_thickness);
}

/// The drawing part of `PLOTTER::BezierCurve` for an already approximated polyline
/// (`approxPoints` from `BEZIER_POLY::GetPoly`).
pub fn bezier_curve_from_points<P: Plotter + ?Sized>(
    p: &mut P,
    a_start: Vector2I,
    a_end: Vector2I,
    approx_points: &[Vector2I],
    a_line_thickness: i32,
) {
    p.set_current_line_width(a_line_thickness, None);
    p.move_to(a_start);

    // C++ loops `ii < approxPoints.size()-1` on an unsigned size
    for ii in 1..approx_points.len().saturating_sub(1) {
        p.line_to(approx_points[ii]);
    }

    p.finish_to(a_end);
}

/// Port of the base implementation of `PLOTTER::PlotImage`: the fallback plotter
/// implementation for plotting an image as a rectangle.
pub fn plot_image_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_image_width: i32,
    a_image_height: i32,
    a_pos: Vector2I,
    a_scale_factor: f64,
    _a_alpha: f64,
) {
    // VECTOR2I( double, double ): implicit conversions (truncation)
    let size = Vector2I::new(
        f64_to_i32_x86(a_image_width as f64 * a_scale_factor),
        f64_to_i32_x86(a_image_height as f64 * a_scale_factor),
    );

    let mut start = a_pos;
    start.x -= size.x / 2;
    start.y -= size.y / 2;

    let mut end = start;
    end.x += size.x;
    end.y += size.y;

    p.rect(start, end, FillT::NoFill, USE_DEFAULT_LINE_WIDTH, 0);
}

/// Port of the base implementation of `PLOTTER::ThickOval`.
pub fn thick_oval_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_pos: Vector2I,
    a_size: Vector2I,
    a_orient: EdaAngle,
    a_width: i32,
    a_data: PlotData,
) {
    p.set_current_line_width(a_width, a_data);

    let mut orient = a_orient;
    let mut size = a_size;

    if size.x > size.y {
        std::mem::swap(&mut size.x, &mut size.y);
        orient += ANGLE_90;
    }

    let deltaxy = size.y - size.x; /* distance between centers of the oval */
    let radius = size.x / 2;

    // Build a vertical oval shape giving the start and end points of arcs and edges,
    // and the middle point of arcs
    // Shape is (x = corner and arc ends, c = arc centre)
    //  xcx
    //
    //  xcx
    let half_height = deltaxy / 2;
    let mut corners = [
        Vector2I::new(-radius, -half_height),
        Vector2I::new(-radius, half_height),
        Vector2I::new(0, half_height),
        Vector2I::new(radius, half_height),
        Vector2I::new(radius, -half_height),
        Vector2I::new(0, -half_height),
    ];

    // Rotate and move to the actual position
    for corner in corners.iter_mut() {
        rotate_point(corner, orient);
        *corner += a_pos;
    }

    // Gen shape (2 lines and 2 180 deg arcs):
    p.move_to(corners[0]);
    p.finish_to(corners[1]);

    p.arc(
        corners[2].into(),
        -orient,
        ANGLE_180,
        radius as f64,
        FillT::NoFill,
        a_width,
    );

    p.move_to(corners[3]);
    p.finish_to(corners[4]);

    p.arc(
        corners[5].into(),
        -orient,
        -ANGLE_180,
        radius as f64,
        FillT::NoFill,
        a_width,
    );
}

/// Port of the base implementation of `PLOTTER::ThickSegment`.
pub fn thick_segment_base<P: Plotter + ?Sized>(
    p: &mut P,
    start: Vector2I,
    end: Vector2I,
    width: i32,
    a_data: PlotData,
) {
    if start == end {
        // The width parameter doubles as a sentinel. DO_NOT_SET_LINE_WIDTH means the
        // caller already configured the pen so we use the live pen width as the diameter.
        // USE_DEFAULT_LINE_WIDTH must be resolved through SetCurrentLineWidth so the pen
        // is also set to the default value, otherwise we would emit whatever the prior
        // pen width happened to be.
        let mut diameter = width;

        if width == USE_DEFAULT_LINE_WIDTH {
            p.set_current_line_width(width, a_data);
            diameter = p.current_line_width();
        } else if width == DO_NOT_SET_LINE_WIDTH {
            diameter = p.current_line_width();
        }

        // wxCHECK2_MSG: "Plotter called with unresolved line width sentinel"
        if diameter < 0 {
            return;
        }

        p.circle(start, diameter, FillT::FilledShape, 0);
    } else {
        p.set_current_line_width(width, None);
        p.move_to(start);
        p.finish_to(end);
    }
}

/// Port of the base implementation of `PLOTTER::ThickArc( aCentre, aStAngle, aAngle, aRadius,
/// aWidth, aData )`.
pub fn thick_arc_base<P: Plotter + ?Sized>(
    p: &mut P,
    centre: Vector2D,
    a_start_angle: EdaAngle,
    a_angle: EdaAngle,
    a_radius: f64,
    a_width: i32,
    _a_data: PlotData,
) {
    p.arc(
        centre,
        a_start_angle,
        a_angle,
        a_radius,
        FillT::NoFill,
        a_width,
    );
}

/// Port of the base implementation of `PLOTTER::ThickRect`.
pub fn thick_rect_base<P: Plotter + ?Sized>(
    p: &mut P,
    p1: Vector2I,
    p2: Vector2I,
    width: i32,
    _a_data: PlotData,
) {
    p.rect(p1, p2, FillT::NoFill, width, 0);
}

/// Port of the base implementation of `PLOTTER::ThickCircle`.
pub fn thick_circle_base<P: Plotter + ?Sized>(
    p: &mut P,
    pos: Vector2I,
    diametre: i32,
    width: i32,
    _a_data: PlotData,
) {
    p.circle(pos, diametre, FillT::NoFill, width);
}

/// Port of the base implementation of `PLOTTER::FilledCircle`.
pub fn filled_circle_base<P: Plotter + ?Sized>(
    p: &mut P,
    pos: Vector2I,
    diametre: i32,
    _a_data: PlotData,
) {
    p.circle(pos, diametre, FillT::FilledShape, 0);
}

/// Port of the base implementation of `PLOTTER::ThickPoly`: plots the first outline only.
///
/// C++ calls `COutline( 0 )` unconditionally; on an empty set the port plots nothing.
pub fn thick_poly_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_poly: &ShapePolySet,
    a_width: i32,
    a_data: PlotData,
) {
    if a_poly.outline_count() == 0 {
        return;
    }

    p.plot_poly_chain(a_poly.c_outline(0), FillT::NoFill, a_width, a_data);
}

/// Port of the base implementation of `PLOTTER::PlotPoly( const SHAPE_LINE_CHAIN& ... )`:
/// the chain's points (arcs as their stored points), closed with the first point when the
/// chain is closed.
pub fn plot_poly_chain_base<P: Plotter + ?Sized>(
    p: &mut P,
    a_line_chain: &ShapeLineChain,
    a_fill: FillT,
    a_width: i32,
    a_data: PlotData,
) {
    let mut corner_list: Vec<Vector2I> =
        Vec::with_capacity(a_line_chain.point_count().max(0) as usize);

    for ii in 0..a_line_chain.point_count() {
        corner_list.push(a_line_chain.c_point(ii));
    }

    // C++ reads front()/back() of a possibly empty vector here (undefined behaviour); an
    // empty chain is passed on unchanged.
    if a_line_chain.is_closed()
        && !corner_list.is_empty()
        && corner_list[0] != corner_list[corner_list.len() - 1]
    {
        corner_list.push(a_line_chain.c_point(0));
    }

    p.plot_poly(&corner_list, a_fill, a_width, a_data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinels() {
        assert_eq!(DO_NOT_SET_LINE_WIDTH, -2);
        assert_eq!(USE_DEFAULT_LINE_WIDTH, -1);
        assert_eq!(MARKER_COUNT, crate::drill_markers::MARKER_COUNT);
    }
}
