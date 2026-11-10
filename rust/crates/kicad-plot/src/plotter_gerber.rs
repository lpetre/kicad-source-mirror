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

//! Port of `include/plotters/plotter_gerber.h` and `common/plotters/GERBER_plotter.cpp`
//! (L1-1805, L1849-1856): the Gerber RS-274X (X1/X2) plot engine `GERBER_PLOTTER`.
//!
//! The C++ writes the body to a temporary work file and, in `EndPlot`, copies it to the real
//! file, inserting the aperture macros and the aperture list after the line
//! `G04 APERTURE LIST*`. The port writes the body into [`PlotterState::output`] and performs
//! the same splice in memory in [`GerberPlotter::end_plot`] (same chunking as the C++
//! `fgets( line, 1024, workFile )` loop, so even pathological inputs give the same bytes).
//!
//! Things C++ reads from the environment are parameters: the build version
//! (`GetBuildVersion()`) and the creation date (`wxDateTime::Now()`), see
//! [`GerberPlotter::set_build_version`] and [`GerberPlotter::set_creation_date`].
//!
//! `Text` / `PlotText` (`GERBER_plotter.cpp` L1807-1846) only emit the net attributes and
//! forward to the base implementation (`plotter_text.rs`). `FlashPadChamferRoundRect` takes the
//! polygon generator (`TransformRoundChamferedRectToPolygon`) as a parameter until
//! `kicad-geom` provides it (see `PORT_STATUS.md`).

use kicad_core::color4d::Color4D;
use kicad_core::plot_enums::{FillT, PlotFormat};
use kicad_core::printf::format_f;
use kicad_core::stroke_params::LineStyle;
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::convert_basic_shapes_to_polygon::transform_round_chamfered_rect_to_polygon;
use kicad_geom::geometry::approximation::ERROR_INSIDE;
use kicad_geom::{
    ANGLE_0, ANGLE_90, ANGLE_180, EdaAngle, ShapeArc, ShapeLineChain, ShapePolySet, Vector2D,
    Vector2I, ki_round, rotate_point,
};

use crate::gbr_metadata::{
    GbrApertureAttrib, GbrApertureMetadata, GbrMetadata, format_net_attribute,
};
use crate::gbr_netlist_metadata::GbrNetlistMetadata;
use crate::gbr_plotter_aperture_macros::*;
use crate::gbr_plotter_apertures::{
    AM_FREEPOLY_BASENAME, AperMacroFreepolyList, Aperture, ApertureType, poly_compare,
};
use crate::plotter::{
    DO_NOT_SET_LINE_WIDTH, PlotData, Plotter, PlotterState, USE_DEFAULT_LINE_WIDTH, thick_arc_base,
    thick_circle_base, thick_poly_base, thick_rect_base, thick_segment_base,
};
use crate::plotter_text::{plot_text_base, text_base};
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::{Font, Metrics};

/// `GBR_MACRO_FOR_CUSTOM_PAD_MAX_CORNER_COUNT`: max count of corners to create a aperture
/// macro for a custom shape.
///
/// Provided just in case a aperture macro type free polygon creates issues when the number of
/// corners is too high (1 corner = up to 24 chars). Gerber doc say max corners 5000. We use a
/// slightly smaller value. If a custom shape needs more than this, it will be plot using a
/// region.
pub const GBR_MACRO_FOR_CUSTOM_PAD_MAX_CORNER_COUNT: usize = 4990;

/// The line after which `EndPlot` inserts the aperture macros and the aperture list.
const APERTURE_LIST_MARKER: &[u8] = b"G04 APERTURE LIST*";

/// The signature of `TransformRoundChamferedRectToPolygon( aBuffer, aPosition, aSize,
/// aRotation, aCornerRadius, aChamferRatio, aChamferCorners, aInflate, aError, aErrorLoc )`
/// as used by [`GerberPlotter::flash_pad_chamfer_round_rect`] (`aInflate` is always 0 and
/// `aErrorLoc` always `ERROR_INSIDE` there): `(position, size, rotation, corner radius,
/// chamfer ratio, chamfer corners, max error) -> polygon set`.
pub type RoundChamferedRectToPolygonFn<'a> =
    dyn Fn(Vector2I, Vector2I, EdaAngle, i32, f64, i32, i32) -> ShapePolySet + 'a;

/// `TransformRoundChamferedRectToPolygon( outline, aPosition, aSize, aRotation,
/// aCornerRadius, aChamferRatio, aChamferCorners, 0, aError, ERROR_INSIDE )` in the shape of
/// [`RoundChamferedRectToPolygonFn`]: the generator the plotters use by default
/// ([`kicad_geom::convert_basic_shapes_to_polygon::transform_round_chamfered_rect_to_polygon`]).
pub fn round_chamfered_rect_to_polygon(
    a_position: Vector2I,
    a_size: Vector2I,
    a_rotation: EdaAngle,
    a_corner_radius: i32,
    a_chamfer_ratio: f64,
    a_chamfer_corners: i32,
    a_error: i32,
) -> ShapePolySet {
    let mut outline = ShapePolySet::new();
    transform_round_chamfered_rect_to_polygon(
        &mut outline,
        a_position,
        a_size,
        a_rotation,
        a_corner_radius,
        a_chamfer_ratio,
        a_chamfer_corners,
        0,
        a_error,
        ERROR_INSIDE,
    );
    outline
}

/// Port of `GERBER_PLOTTER`.
#[derive(Clone, Debug)]
pub struct GerberPlotter {
    state: PlotterState,

    /// `m_objectAttributesDictionary`: the attributes dictionary created/modified by `%TO`,
    /// attached to objects, when they are created by D01, D03, G36/G37 commands. Standard
    /// attributes are .P, .C and .N. This is used by gerber readers when creating a new
    /// object. Cleared by `%TD` command. It can store more than one attribute: the string
    /// stores the line(s) actually written to the gerber file (a .P, .C or .N attribute, or 2
    /// or 3 attributes, separated by a `\n` char).
    object_attributes_dictionary: String,

    /// `m_apertureAttribute`: the last aperture attribute generated (only one aperture
    /// attribute can be set).
    aperture_attribute: i32,

    /// The text written before `StartPlot` (C++ `finalFile` already holds it); the body
    /// written between `StartPlot` and `EndPlot` lives in `state.output` (the work file).
    final_file: String,

    /// `m_apertures`: the list of available apertures.
    apertures: Vec<Aperture>,
    /// `m_currentApertureIdx`: the index of the current aperture in `apertures` (-1: none).
    current_aperture_idx: i32,
    /// `m_hasApertureRoundRect`: true is at least one round rect aperture is in use.
    has_aperture_round_rect: bool,
    /// `m_hasApertureRotOval`: true is at least one oval rotated aperture is in use.
    has_aperture_rot_oval: bool,
    /// `m_hasApertureRotRect`: true is at least one rect. rotated aperture is in use.
    has_aperture_rot_rect: bool,
    /// `m_hasApertureOutline4P`: true is at least one 4 corners outline (free polygon with 4
    /// corners) aperture is in use.
    has_aperture_outline4p: bool,
    /// `m_hasApertureChamferedRect`: true is at least one chamfered rect is in use (with no
    /// rounded corner).
    has_aperture_chamfered_rect: bool,

    /// `m_gerberUnitInch`: true if the gerber units are inches, false for mm.
    gerber_unit_inch: bool,
    /// `m_gerberUnitFmt`: number of digits in mantissa. Usually 6 in Inches and 5 or 6 in mm.
    gerber_unit_fmt: i32,
    /// `m_gerberDisableApertMacros`: true to disable Aperture Macro (AM) command, for broken
    /// Gerber Readers. Regions will be used instead of AM shapes.
    gerber_disable_apert_macros: bool,
    /// `m_useX2format`: add X2 file header attributes. If false, attributes will be added as
    /// comments.
    use_x2format: bool,
    /// `m_useNetAttributes`: in recent gerber files, netlist info can be added. It will be
    /// added if this param is true, using X2 or X1 format.
    use_net_attributes: bool,

    /// `m_am_freepoly_list`: a list of aperture macros defined "on the fly" because the
    /// number of parameters is not defined: this is the case of the macro using the primitive
    /// 4 to create a polygon. The number of vertices is not known for free polygonal shapes,
    /// and an aperture macro must be created for each specific polygon.
    am_freepoly_list: AperMacroFreepolyList,

    /// `GetBuildVersion()` (C++ global); a parameter in the port.
    build_version: String,
    /// `wxDateTime::Now().FormatISOCombined( ' ' )` (`YYYY-MM-DD HH:MM:SS`); a parameter in the
    /// port.
    creation_date: String,
}

impl Default for GerberPlotter {
    fn default() -> Self {
        Self::new()
    }
}

impl GerberPlotter {
    /// Port of `GERBER_PLOTTER::GERBER_PLOTTER()`.
    pub fn new() -> Self {
        GerberPlotter {
            state: PlotterState::new(),
            object_attributes_dictionary: String::new(),
            aperture_attribute: 0,
            final_file: String::new(),
            apertures: Vec::new(),
            current_aperture_idx: -1,
            has_aperture_round_rect: false,
            has_aperture_rot_oval: false,
            has_aperture_rot_rect: false,
            has_aperture_outline4p: false,
            has_aperture_chamfered_rect: false,
            // number of digits after the point (number of digits of the mantissa
            // Be careful: the Gerber coordinates are stored in an integer
            // so 6 digits (inches) or 5 digits (mm) is a good value
            // To avoid overflow, 7 digits (inches) or 6 digits is a max.
            // with lower values than 6 digits (inches) or 5 digits (mm),
            // Creating self-intersecting polygons from non-intersecting polygons
            // happen easily.
            gerber_unit_inch: false,
            gerber_unit_fmt: 6,
            gerber_disable_apert_macros: false,
            use_x2format: true,
            use_net_attributes: true,
            am_freepoly_list: AperMacroFreepolyList::new(),
            build_version: String::new(),
            creation_date: String::new(),
        }
    }

    /// Port of `GERBER_PLOTTER::GetDefaultFileExtension`.
    pub fn default_file_extension() -> &'static str {
        "gbr"
    }

    /// The text C++ takes from `GetBuildVersion()` for the `G04 Created by KiCad` comment.
    pub fn set_build_version(&mut self, a_version: &str) {
        self.build_version = a_version.to_string();
    }

    /// The date C++ takes from `wxDateTime::Now().FormatISOCombined( ' ' )` for the
    /// `G04 Created by KiCad` comment (`YYYY-MM-DD HH:MM:SS`).
    pub fn set_creation_date(&mut self, a_date: &str) {
        self.creation_date = a_date.to_string();
    }

    /// Port of `GERBER_PLOTTER::UseX2format`.
    pub fn use_x2format(&mut self, a_enable: bool) {
        self.use_x2format = a_enable;
    }

    /// Port of `GERBER_PLOTTER::UseX2NetAttributes`.
    pub fn use_x2_net_attributes(&mut self, a_enable: bool) {
        self.use_net_attributes = a_enable;
    }

    /// Port of `GERBER_PLOTTER::DisableApertMacros`: disable Aperture Macro (AM) command,
    /// only for broken Gerber Readers. Regions will be used instead of AM shapes to draw
    /// complex shapes.
    pub fn disable_apert_macros(&mut self, a_disable: bool) {
        self.gerber_disable_apert_macros = a_disable;
    }

    /// The aperture list (`m_apertures`), in creation order.
    pub fn apertures(&self) -> &[Aperture] {
        &self.apertures
    }

    /// Append to the output (C++ `fmt::print( m_outputFile, ... )`).
    fn print(&mut self, a_text: &str) {
        self.state.output.push_str(a_text);
    }

    /// Append a line to the output (C++ `fmt::println( m_outputFile, ... )`).
    fn println(&mut self, a_text: &str) {
        self.state.output.push_str(a_text);
        self.state.output.push('\n');
    }

    /// Port of `GERBER_PLOTTER::emitDcode`: emit a D-Code record, using proper conversions to
    /// format a leading zero omitted gerber coordinate. For n decimal positions, see header
    /// generation in `start_plot`.
    fn emit_dcode(&mut self, pt: Vector2D, dcode: i32) {
        let line = format!("X{}Y{}D{:02}*", ki_round(pt.x), ki_round(pt.y), dcode);
        self.println(&line);
    }

    /// Port of `GERBER_PLOTTER::ClearAllAttributes`: remove (clear) all attributes from object
    /// attributes dictionary (TO. and TA commands). Similar to `clearNetAttribute()`, this is
    /// an unconditional reset of TO. and TA. attributes.
    pub fn clear_all_attributes(&mut self) {
        // Remove all attributes from object attributes dictionary (TO. and TA commands)
        if self.use_x2format {
            self.println("%TD*%");
        } else {
            self.println("G04 #@! TD*");
        }

        self.object_attributes_dictionary.clear();
    }

    /// Port of `GERBER_PLOTTER::clearNetAttribute`: clear a Gerber net attribute record (clear
    /// object attribute dictionary) and output the clear object attribute dictionary command
    /// to gerber file. Has effect only if a net attribute is stored in
    /// `m_objectAttributesDictionary`.
    fn clear_net_attribute(&mut self) {
        // disable a Gerber net attribute (exists only in X2 with net attributes mode).
        if self.object_attributes_dictionary.is_empty() {
            // No net attribute or not X2 mode
            return;
        }

        // Remove all net attributes from object attributes dictionary
        if self.use_x2format {
            self.println("%TD*%");
        } else {
            self.println("G04 #@! TD*");
        }

        self.object_attributes_dictionary.clear();
    }

    /// Port of `GERBER_PLOTTER::formatNetAttribute`: print a Gerber net attribute object
    /// record. In a gerber file, a net attribute is owned by a graphic object;
    /// `formatNetAttribute` must be called before creating the object. The generated string
    /// depends on the type of netlist info.
    fn format_net_attribute(&mut self, a_data: Option<&GbrNetlistMetadata>) {
        // print a Gerber net attribute record.
        // it is added to the object attributes dictionary
        // On file, only modified or new attributes are printed.
        let Some(a_data) = a_data else {
            return;
        };

        if !self.use_net_attributes {
            return;
        }

        let use_x1_structured_comment = !self.use_x2format;

        let Some(result) = format_net_attribute(
            &mut self.object_attributes_dictionary,
            Some(a_data),
            use_x1_structured_comment,
        ) else {
            return;
        };

        if result.clear_previous_attributes {
            self.clear_net_attribute();
        }

        if let Some(short_attribute_string) = &result.printed_text
            && !short_attribute_string.is_empty()
        {
            self.print(short_attribute_string);
        }

        if self.use_x2format && !a_data.extra_data.is_empty() {
            self.print(&a_data.extra_data);
        }
    }

    /// Port of `GERBER_PLOTTER::GetOrCreateAperture( const VECTOR2I& aSize, int aRadius, ... )`:
    /// an index to the aperture in aperture list which meets the size and type of tool. If
    /// the aperture does not exist, it is created and entered in aperture list.
    ///
    /// `a_radius` is the radius used for some shapes tool (oval, roundrect macros),
    /// `a_rotation` the rotation of tool (primitives round, oval rect accept only 0.0),
    /// `a_aperture_attribute` an aperture attribute of the tool (a tool can have only one
    /// attribute, 0 = no specific attribute), `a_custom_attribute` a string describing
    /// custom tools.
    pub fn get_or_create_aperture(
        &mut self,
        a_size: Vector2I,
        a_radius: i32,
        a_rotation: EdaAngle,
        a_type: ApertureType,
        a_aperture_attribute: i32,
        a_custom_attribute: &str,
    ) -> i32 {
        let mut last_d_code = 9;

        // Search an existing aperture
        for (idx, tool) in self.apertures.iter().enumerate() {
            last_d_code = tool.d_code;

            if tool.type_ == a_type
                && tool.size == a_size
                && tool.radius == a_radius
                && tool.rotation == a_rotation
                && tool.aperture_attribute == a_aperture_attribute
                && tool.custom_attribute == a_custom_attribute
            {
                return idx as i32;
            }
        }

        // Allocate a new aperture
        let mut new_tool = Aperture::new(a_type);
        new_tool.size = a_size;
        new_tool.type_ = a_type;
        new_tool.radius = a_radius;
        new_tool.rotation = a_rotation;
        new_tool.d_code = last_d_code + 1;
        new_tool.aperture_attribute = a_aperture_attribute;
        new_tool.custom_attribute = a_custom_attribute.to_string();

        self.apertures.push(new_tool);

        self.apertures.len() as i32 - 1
    }

    /// Port of `GERBER_PLOTTER::GetOrCreateAperture( const std::vector<VECTOR2I>& aCorners,
    /// ... )`: same for tools that manage a list of corners (polygon). Corner lists are
    /// compared with `polyCompare` (2 IU tolerance).
    pub fn get_or_create_aperture_corners(
        &mut self,
        a_corners: &[Vector2I],
        a_rotation: EdaAngle,
        a_type: ApertureType,
        a_aperture_attribute: i32,
        a_custom_attribute: &str,
    ) -> i32 {
        let mut last_d_code = 9;

        // For APERTURE::AM_FREE_POLYGON aperture macros, we need to create the macro
        // on the fly, because due to the fact the vertex count is not a constant we
        // cannot create a static definition.
        if a_type == ApertureType::AmFreePolygon {
            let idx = self.am_freepoly_list.find_am(a_corners);

            if idx < 0 {
                self.am_freepoly_list.append(a_corners);
            }
        }

        // Search an existing aperture
        for (idx, tool) in self.apertures.iter().enumerate() {
            last_d_code = tool.d_code;

            if tool.type_ == a_type
                && tool.corners.len() == a_corners.len()
                && tool.rotation == a_rotation
                && tool.aperture_attribute == a_aperture_attribute
                && tool.custom_attribute == a_custom_attribute
            {
                // A candidate is found. the corner lists must be similar
                let is_same = poly_compare(&tool.corners, a_corners);

                if is_same {
                    return idx as i32;
                }
            }
        }

        // Allocate a new aperture
        let mut new_tool = Aperture::new(a_type);

        new_tool.corners = a_corners.to_vec();
        new_tool.size = Vector2I::new(0, 0); // Not used
        new_tool.type_ = a_type;
        new_tool.radius = 0; // Not used
        new_tool.rotation = a_rotation;
        new_tool.d_code = last_d_code + 1;
        new_tool.aperture_attribute = a_aperture_attribute;
        new_tool.custom_attribute = a_custom_attribute.to_string();

        self.apertures.push(new_tool);

        self.apertures.len() as i32 - 1
    }

    /// Port of `GERBER_PLOTTER::selectAperture( const VECTOR2I& aSize, ... )`: pick an existing
    /// aperture or create a new one, matching the size, type and attributes. Write the DCode
    /// selection on gerber file.
    fn select_aperture(
        &mut self,
        a_size: Vector2I,
        a_radius: i32,
        a_rotation: EdaAngle,
        a_type: ApertureType,
        a_aperture_attribute: i32,
        a_custom_attribute: &str,
    ) {
        let mut change = match self.current() {
            None => true,
            Some(cur) => {
                cur.type_ != a_type
                    || cur.size != a_size
                    || cur.radius != a_radius
                    || cur.rotation != a_rotation
            }
        };

        if !change && let Some(cur) = self.current() {
            change = cur.aperture_attribute != a_aperture_attribute
                || cur.custom_attribute != a_custom_attribute;
        }

        if change {
            // Pick an existing aperture or create a new one
            self.current_aperture_idx = self.get_or_create_aperture(
                a_size,
                a_radius,
                a_rotation,
                a_type,
                a_aperture_attribute,
                a_custom_attribute,
            );
            let line = format!(
                "D{}*",
                self.apertures[self.current_aperture_idx as usize].d_code
            );
            self.println(&line);
        }
    }

    /// Port of `GERBER_PLOTTER::selectAperture( const std::vector<VECTOR2I>& aCorners, ... )`:
    /// pick an existing aperture or create a new one, matching the corners, rotation, type and
    /// attributes (corner lists compared exactly with the current aperture). Write the DCode
    /// selection on gerber file.
    fn select_aperture_corners(
        &mut self,
        a_corners: &[Vector2I],
        a_rotation: EdaAngle,
        a_type: ApertureType,
        a_aperture_attribute: i32,
        a_custom_attribute: &str,
    ) {
        let mut change = match self.current() {
            None => true,
            Some(cur) => {
                cur.type_ != a_type
                    || cur.corners.len() != a_corners.len()
                    || cur.rotation != a_rotation
            }
        };

        if !change && let Some(cur) = self.current() {
            // Compare corner lists
            change = a_corners
                .iter()
                .zip(cur.corners.iter())
                .any(|(a, b)| a != b);
        }

        if !change && let Some(cur) = self.current() {
            change = cur.aperture_attribute != a_aperture_attribute
                || cur.custom_attribute != a_custom_attribute;
        }

        if change {
            // Pick an existing aperture or create a new one
            self.current_aperture_idx = self.get_or_create_aperture_corners(
                a_corners,
                a_rotation,
                a_type,
                a_aperture_attribute,
                a_custom_attribute,
            );
            let line = format!(
                "D{}*",
                self.apertures[self.current_aperture_idx as usize].d_code
            );
            self.println(&line);
        }
    }

    /// The current aperture (`m_apertures[m_currentApertureIdx]`), if any.
    fn current(&self) -> Option<&Aperture> {
        usize::try_from(self.current_aperture_idx)
            .ok()
            .and_then(|i| self.apertures.get(i))
    }

    /// Port of `GERBER_PLOTTER::selectApertureWithAttributes`: pick an aperture or create a
    /// new one and emits the DCode, the net attributes and the flash (`D03`).
    fn select_aperture_with_attributes(
        &mut self,
        a_pos: Vector2I,
        a_gbr_metadata: PlotData,
        a_size: Vector2I,
        a_radius: i32,
        a_angle: EdaAngle,
        a_type: ApertureType,
    ) {
        let pos_dev = self.user_to_device_coordinates(a_pos);

        let mut aperture_attribute = 0;
        let mut custom_attribute = String::new();

        if let Some(md) = a_gbr_metadata {
            aperture_attribute = md.aperture_attrib() as i32;
            custom_attribute = md.custom_attribute().to_string();
        }

        self.select_aperture(
            a_size,
            a_radius,
            a_angle,
            a_type,
            aperture_attribute,
            &custom_attribute,
        );

        if let Some(md) = a_gbr_metadata {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.emit_dcode(pos_dev, 3);
    }

    /// The aperture size scale (`fscale`): aperture sizes are in inch or mm, regardless the
    /// coordinates format.
    fn aperture_scale(&self) -> f64 {
        let mut fscale = 0.0001 * self.state.plot_scale / self.state.ius_per_decimil; // inches

        if !self.gerber_unit_inch {
            fscale *= 25.4; // size in mm
        }

        fscale
    }

    /// Port of `GERBER_PLOTTER::writeApertureList`: generate the table of D codes.
    fn write_aperture_list(&mut self) {
        let use_x1_structured_comment = !self.use_x2format;
        let mut out = String::new();

        // aperture sizes are in inch or mm, regardless the coordinates format
        let fscale = self.aperture_scale();
        let f = |v: f64| format_f(v, 6);

        // Init
        for tool in &self.apertures {
            let attribute = tool.aperture_attribute;

            if attribute != self.aperture_attribute {
                // C++ casts the int to GBR_APERTURE_ATTRIB; values come from that enum.
                let attr = GbrApertureAttrib::from_i32(attribute).unwrap_or_default();
                out += &GbrApertureMetadata::format_attribute_static(
                    attr,
                    use_x1_structured_comment,
                    &tool.custom_attribute,
                );
            }

            out += &format!("%ADD{}", tool.d_code);

            // Please note: the Gerber specs for mass parameters say that exponential syntax
            // is *not* allowed and the decimal point should also be always inserted. So the
            // %g format is ruled out, but %f is fine (the # modifier forces the decimal
            // point). Sadly the %f formatter can't remove trailing zeros but that's not a
            // problem, since nothing forbid it (the file is only slightly longer)
            match tool.type_ {
                ApertureType::AtCircle => {
                    out += &format!("C,{}*%\n", f(tool.diameter() as f64 * fscale));
                }
                ApertureType::AtRect => {
                    out += &format!(
                        "R,{}X{}*%\n",
                        f(tool.size.x as f64 * fscale),
                        f(tool.size.y as f64 * fscale)
                    );
                }
                ApertureType::AtPlotting => {
                    out += &format!("C,{}*%\n", f(tool.size.x as f64 * fscale));
                }
                ApertureType::AtOval => {
                    out += &format!(
                        "O,{}X{}*%\n",
                        f(tool.size.x as f64 * fscale),
                        f(tool.size.y as f64 * fscale)
                    );
                }
                ApertureType::AtRegularPoly
                | ApertureType::AtRegularPoly3
                | ApertureType::AtRegularPoly4
                | ApertureType::AtRegularPoly5
                | ApertureType::AtRegularPoly6
                | ApertureType::AtRegularPoly7
                | ApertureType::AtRegularPoly8
                | ApertureType::AtRegularPoly9
                | ApertureType::AtRegularPoly10
                | ApertureType::AtRegularPoly11
                | ApertureType::AtRegularPoly12 => {
                    out += &format!(
                        "P,{}X{}X{}*%\n",
                        f(tool.diameter() as f64 * fscale),
                        tool.reg_poly_vertice_count(),
                        f(tool.rotation().as_degrees())
                    );
                }
                ApertureType::AmRoundRect => {
                    // Aperture macro for round rect pads
                    // The aperture macro needs coordinates of the centers of the 4 corners
                    let mut half_size =
                        Vector2I::new(tool.size.x / 2 - tool.radius, tool.size.y / 2 - tool.radius);

                    // Ensure half_size.x and half_size.y > minimal value to avoid shapes
                    // with null size (especially the rectangle with coordinates corners)
                    // Because the minimal value for a non nul Gerber coord in 10nm
                    // in format 4.5, use 10 nm as minimal value.
                    // (Even in 4.6 format, use 10 nm, because gerber viewers can have
                    // a internal unit bigger than 1 nm)
                    let min_size_value = 10;
                    half_size.x = half_size.x.max(min_size_value);
                    half_size.y = half_size.y.max(min_size_value);

                    let mut corners = [
                        Vector2I::new(-half_size.x, -half_size.y),
                        Vector2I::new(half_size.x, -half_size.y),
                        Vector2I::new(half_size.x, half_size.y),
                        Vector2I::new(-half_size.x, half_size.y),
                    ];

                    // Rotate the corner coordinates:
                    for corner in corners.iter_mut() {
                        rotate_point(corner, -tool.rotation);
                    }

                    out += &format!(
                        "{},{}X",
                        APER_MACRO_ROUNDRECT_NAME,
                        f(tool.radius as f64 * fscale)
                    );

                    // Add each corner
                    for corner in &corners {
                        out += &format!(
                            "{}X{}X",
                            f(corner.x as f64 * fscale),
                            f(corner.y as f64 * fscale)
                        );
                    }

                    out += "0*%\n";
                }
                ApertureType::AmRotRect => {
                    // Aperture macro for rotated rect pads
                    out += &format!(
                        "{},{}X{}X{}*%\n",
                        APER_MACRO_ROT_RECT_NAME,
                        f(tool.size.x as f64 * fscale),
                        f(tool.size.y as f64 * fscale),
                        f(tool.rotation.as_degrees())
                    );
                }
                ApertureType::AperMacroOutline4p
                | ApertureType::AperMacroOutline5p
                | ApertureType::AperMacroOutline6p
                | ApertureType::AperMacroOutline7p
                | ApertureType::AperMacroOutline8p => {
                    // Aperture macro for trapezoid pads (4P) and chamfered rect pads (5P-8P)
                    out += match tool.type_ {
                        ApertureType::AperMacroOutline4p => APER_MACRO_OUTLINE4P_NAME,
                        ApertureType::AperMacroOutline5p => APER_MACRO_OUTLINE5P_NAME,
                        ApertureType::AperMacroOutline6p => APER_MACRO_OUTLINE6P_NAME,
                        ApertureType::AperMacroOutline7p => APER_MACRO_OUTLINE7P_NAME,
                        _ => APER_MACRO_OUTLINE8P_NAME,
                    };

                    // Add separator after aperture macro name
                    out += ",";

                    // Output all corners (should be 4 to 8 corners)
                    // Remember: the Y coordinate must be negated, due to the fact in Pcbnew
                    // the Y axis is from top to bottom
                    for corner in &tool.corners {
                        out += &format!(
                            "{}X{}X",
                            f(corner.x as f64 * fscale),
                            f(corner.y.wrapping_neg() as f64 * fscale)
                        );
                    }

                    // close outline and output rotation
                    out += &format!("{}*%\n", f(tool.rotation.as_degrees()));
                }
                ApertureType::AmRotatedOval => {
                    // Aperture macro for rotated oval pads (not rotated is a primitive)
                    // m_Size.x = full length; m_Size.y = width, and the macro aperture
                    // expects the position of ends

                    // the seg_len is the distance between the 2 circle centers
                    let seg_len = tool.size.x - tool.size.y;

                    // Center of the circle on the segment start point:
                    let mut start = Vector2I::new(seg_len / 2, 0);

                    // Center of the circle on the segment end point:
                    let mut end = Vector2I::new(-(seg_len / 2), 0);

                    rotate_point(&mut start, tool.rotation);
                    rotate_point(&mut end, tool.rotation);

                    out += &format!(
                        "{},{}X{}X{}X{}X{}X0*%\n",
                        APER_MACRO_SHAPE_OVAL_NAME,
                        f(tool.size.y as f64 * fscale), // width
                        f(start.x as f64 * fscale),     // X,Y corner start pos
                        f(start.y.wrapping_neg() as f64 * fscale),
                        f(end.x as f64 * fscale), // X,Y corner end pos
                        f(end.y.wrapping_neg() as f64 * fscale)
                    );
                }
                ApertureType::AmFreePolygon => {
                    // Find the aperture macro name in the list of aperture macro
                    // created on the fly for this polygon:
                    let idx = self.am_freepoly_list.find_am(&tool.corners);

                    // Write DCODE id ( "%ADDxx" is already in buffer) and rotation
                    // the full line is something like :%ADD12FreePoly1,45.000000*%
                    out += &format!(
                        "{}{},{}*%\n",
                        AM_FREEPOLY_BASENAME,
                        idx,
                        f(tool.rotation.as_degrees())
                    );
                }
            }

            self.aperture_attribute = attribute;

            // Currently reset the aperture attribute. Perhaps a better optimization
            // is to store the last attribute
            if attribute != 0 {
                if self.use_x2format {
                    out += "%TD*%\n";
                } else {
                    out += "G04 #@! TD*\n";
                }

                self.aperture_attribute = 0;
            }
        }

        self.state.output.push_str(&out);
    }

    /// Port of `GERBER_PLOTTER::plotArc( const SHAPE_ARC& aArc, bool aPlotInRegion )`: plot a
    /// Gerber arc. If `a_plot_in_region` is true, the current pen position will not be
    /// initialized to the arc start position, and therefore the arc can be used to define a
    /// region outline item; a line will be created from current position to arc start point.
    /// The line thickness is not initialized in `plotArc`, and must be initialized before
    /// calling it if needed.
    fn plot_arc_shape(&mut self, a_arc: &ShapeArc, a_plot_in_region: bool) {
        let start = a_arc.get_p0();
        let end = a_arc.get_p1();
        let center = a_arc.get_center();

        if !a_plot_in_region {
            self.move_to(start);
        } else {
            self.line_to(start);
        }

        let dev_end = self.user_to_device_coordinates(end);

        // devRelCenter is the position on arc center relative to the arc start, in Gerber
        // coord. Warning: it is **not** userToDeviceCoordinates( center - start ) when the
        // plotter has an offset.
        let dev_rel_center =
            self.user_to_device_coordinates(center) - self.user_to_device_coordinates(start);

        // We need to know if the arc is CW or CCW in device coordinates, so build this arc.
        // (SHAPE_ARC takes VECTOR2I: the VECTOR2D device points are truncated.)
        let dev_start: Vector2I = self.user_to_device_coordinates(start).cast();
        let dev_mid: Vector2I = self.user_to_device_coordinates(a_arc.get_arc_mid()).cast();
        let device_arc = ShapeArc::from_start_mid_end(dev_start, dev_mid, dev_end.cast(), 0);

        self.println("G75*"); // Multiquadrant (360 degrees) mode

        if device_arc.is_clockwise() {
            self.println("G02*"); // Active circular interpolation, CW
        } else {
            self.println("G03*"); // Active circular interpolation, CCW
        }

        let line = format!(
            "X{}Y{}I{}J{}D01*",
            ki_round(dev_end.x),
            ki_round(dev_end.y),
            ki_round(dev_rel_center.x),
            ki_round(dev_rel_center.y)
        );
        self.println(&line);

        self.println("G01*"); // Back to linear interpolate (perhaps useless here).
    }

    /// Port of `GERBER_PLOTTER::plotArc( const VECTOR2I& aCenter, const EDA_ANGLE&
    /// aStartAngle, const EDA_ANGLE& aEndAngle, double aRadius, bool aPlotInRegion )`.
    fn plot_arc(
        &mut self,
        a_center: Vector2I,
        a_start_angle: EdaAngle,
        a_end_angle: EdaAngle,
        a_radius: f64,
        a_plot_in_region: bool,
    ) {
        let start = Vector2I::new(
            ki_round(a_center.x as f64 + a_radius * a_start_angle.cos()),
            ki_round(a_center.y as f64 + a_radius * a_start_angle.sin()),
        );

        if !a_plot_in_region {
            self.move_to(start);
        } else {
            self.line_to(start);
        }

        let end = Vector2I::new(
            ki_round(a_center.x as f64 + a_radius * a_end_angle.cos()),
            ki_round(a_center.y as f64 + a_radius * a_end_angle.sin()),
        );
        let dev_end = self.user_to_device_coordinates(end);

        // devRelCenter is the position on arc center relative to the arc start, in Gerber
        // coord.
        let dev_rel_center =
            self.user_to_device_coordinates(a_center) - self.user_to_device_coordinates(start);

        self.println("G75*"); // Multiquadrant (360 degrees) mode

        if a_start_angle > a_end_angle {
            self.println("G03*"); // Active circular interpolation, CCW
        } else {
            self.println("G02*"); // Active circular interpolation, CW
        }

        let line = format!(
            "X{}Y{}I{}J{}D01*",
            ki_round(dev_end.x),
            ki_round(dev_end.y),
            ki_round(dev_rel_center.x),
            ki_round(dev_rel_center.y)
        );
        self.println(&line);

        self.println("G01*"); // Back to linear interpolate (perhaps useless here).
    }

    /// Print the `TA.AperFunction` of a region (if any); true if it must be cleared after the
    /// region (the common head of `PlotGerberRegion` and the region branch of
    /// `FlashPadRoundRect`).
    fn print_region_aperture_attribute(&mut self, a_gbr_metadata: Option<&GbrMetadata>) -> bool {
        if let Some(md) = a_gbr_metadata {
            let attrib = md.aperture_metadata.format_attribute(!self.use_x2format);

            if !attrib.is_empty() {
                self.print(&attrib);
                return true;
            }
        }

        false
    }

    /// Clear the TA attribute, to avoid the next item to inherit it.
    fn clear_region_aperture_attribute(&mut self) {
        if self.use_x2format {
            self.println("%TD.AperFunction*%");
        } else {
            self.println("G04 #@! TD.AperFunction*");
        }
    }

    /// Port of `GERBER_PLOTTER::PlotGerberRegion( const SHAPE_LINE_CHAIN& aPoly, GBR_METADATA*
    /// aGbrMetadata )`: plot a Gerber region: similar to `PlotPoly` but plot only filled
    /// polygon, and add the TA.AperFunction if `a_gbr_metadata` contains this attribute, and
    /// clear it after plotting.
    pub fn plot_gerber_region_chain(
        &mut self,
        a_poly: &ShapeLineChain,
        a_gbr_metadata: Option<&GbrMetadata>,
    ) {
        if a_poly.point_count() <= 2 {
            return;
        }

        // true if a TA.AperFunction is used
        let clear_ta_aper_function = self.print_region_aperture_attribute(a_gbr_metadata);

        self.plot_poly_chain(a_poly, FillT::FilledShape, 0, a_gbr_metadata);

        // Clear the TA attribute, to avoid the next item to inherit it:
        if clear_ta_aper_function {
            self.clear_region_aperture_attribute();
        }
    }

    /// Port of `GERBER_PLOTTER::PlotGerberRegion( const std::vector<VECTOR2I>& aCornerList,
    /// GBR_METADATA* aGbrMetadata )`.
    pub fn plot_gerber_region(
        &mut self,
        a_corner_list: &[Vector2I],
        a_gbr_metadata: Option<&GbrMetadata>,
    ) {
        if a_corner_list.len() <= 2 {
            return;
        }

        // true if a TA.AperFunction is used
        let clear_ta_aper_function = self.print_region_aperture_attribute(a_gbr_metadata);

        self.plot_poly(a_corner_list, FillT::FilledShape, 0, a_gbr_metadata);

        // Clear the TA attribute, to avoid the next item to inherit it:
        if clear_ta_aper_function {
            self.clear_region_aperture_attribute();
        }
    }

    /// Port of `GERBER_PLOTTER::PlotPolyAsRegion`: similar to `PlotPoly()`, plot a filled
    /// polygon using Gerber region, therefore adding X2 attributes to the region object, like
    /// TA.xxx.
    pub fn plot_poly_as_region(
        &mut self,
        a_poly: &ShapeLineChain,
        a_fill: FillT,
        a_width: i32,
        a_gbr_metadata: Option<&GbrMetadata>,
    ) {
        // plot a filled polygon using Gerber region, therefore adding X2 attributes
        // to the solid polygon
        if a_width != 0 || a_fill == FillT::NoFill {
            self.plot_poly_chain(a_poly, FillT::NoFill, a_width, a_gbr_metadata);
        }

        if a_fill != FillT::NoFill {
            self.plot_gerber_region_chain(a_poly, a_gbr_metadata);
        }
    }

    /// The points of a line chain from index 1, with arcs plotted as Gerber arcs (the loop
    /// shared by both branches of `GERBER_PLOTTER::PlotPoly( const SHAPE_LINE_CHAIN& )`).
    fn plot_chain_points(&mut self, a_poly: &ShapeLineChain) {
        let count = a_poly.point_count();
        let mut ii = 1;

        while ii < count {
            let arcindex = a_poly.arc_index(ii as usize);

            if arcindex < 0 {
                // Plain point
                self.line_to(a_poly.c_point(ii));
            } else {
                let arc = a_poly.arc(arcindex as usize).clone();

                self.plot_arc_shape(&arc, true);

                // skip points on arcs, since we plot the arc itself
                while ii + 1 < count && arcindex == a_poly.arc_index((ii + 1) as usize) {
                    ii += 1;
                }
            }

            ii += 1;
        }
    }

    /// Port of `GERBER_PLOTTER::plotRoundRectAsRegion`: plot a round rect (a round rect shape
    /// in fact) as a Gerber region using lines and arcs for corners. Only the G36 ... G37
    /// region is created.
    fn plot_round_rect_as_region(
        &mut self,
        a_rect_center: Vector2I,
        a_size: Vector2I,
        a_corner_radius: i32,
        a_orient: EdaAngle,
    ) {
        // The region outline is generated by 4 sides and 4 90 deg arcs
        //  1 --- 2
        //  |  c  |
        //  4 --- 3

        // Note also in user coordinates the Y axis is from top to bottom
        // for historical reasons.

        // A helper structure to handle outlines coordinates (segments and arcs)
        // in user coordinates
        #[derive(Clone, Copy)]
        struct RrEdge {
            start: Vector2I,
            end: Vector2I,
            center: Vector2I,
            arc_angle_start: EdaAngle,
        }

        let hsize_x = a_size.x / 2;
        let hsize_y = a_size.y / 2;
        let r = a_corner_radius;

        // Build outline coordinates, relative to rectangle center, rotation 0:
        let mut rr_outline = [
            // Top left corner 1 (and 4 to 1 left vertical side @ x=-hsizeX)
            RrEdge {
                start: Vector2I::new(-hsize_x, hsize_y - r),
                end: Vector2I::new(-hsize_x, -hsize_y + r),
                center: Vector2I::new(-hsize_x + r, -hsize_y + r),
                arc_angle_start: a_orient + ANGLE_180,
            },
            // Top right corner 2 (and 1 to 2 top horizontal side @ y=-hsizeY)
            RrEdge {
                start: Vector2I::new(-hsize_x + r, -hsize_y),
                end: Vector2I::new(hsize_x - r, -hsize_y),
                center: Vector2I::new(hsize_x - r, -hsize_y + r),
                arc_angle_start: a_orient + ANGLE_90,
            },
            // bottom right corner 3 (and 2 to 3 right vertical side @ x=hsizeX)
            RrEdge {
                start: Vector2I::new(hsize_x, -hsize_y + r),
                end: Vector2I::new(hsize_x, hsize_y - r),
                center: Vector2I::new(hsize_x - r, hsize_y - r),
                arc_angle_start: a_orient + ANGLE_0,
            },
            // bottom left corner 4 (and 3 to 4 bottom horizontal side @ y=hsizeY)
            RrEdge {
                start: Vector2I::new(hsize_x - r, hsize_y),
                end: Vector2I::new(-hsize_x + r, hsize_y),
                center: Vector2I::new(-hsize_x + r, hsize_y - r),
                arc_angle_start: a_orient - ANGLE_90,
            },
        ];

        // Move relative coordinates to the actual location and rotation:
        let mut arc_last_center = Vector2I::new(0, 0);
        let arc_last_angle = rr_outline[3].arc_angle_start - ANGLE_90;

        for rr_edge in rr_outline.iter_mut() {
            rotate_point(&mut rr_edge.start, a_orient);
            rotate_point(&mut rr_edge.end, a_orient);
            rotate_point(&mut rr_edge.center, a_orient);
            rr_edge.start += a_rect_center;
            rr_edge.end += a_rect_center;
            rr_edge.center += a_rect_center;
            arc_last_center = rr_edge.center;
        }

        // Ensure the region is a closed polygon, i.e. the end point of last segment
        // (end of arc) is the same as the first point. Rounding issues can create a
        // small difference, mainly for rotated pads.
        // calculate last point (end of last arc):
        let last_pt = Vector2I::new(
            arc_last_center.x + ki_round(a_corner_radius as f64 * arc_last_angle.cos()),
            arc_last_center.y - ki_round(a_corner_radius as f64 * arc_last_angle.sin()),
        );

        self.println("G36*"); // Start region
        self.println("G01*"); // Set linear interpolation.
        let first_pt = last_pt;
        self.move_to(first_pt); // Start point of region, must be same as end point

        for rr_edge in &rr_outline {
            if a_corner_radius != 0 {
                // Guard: ensure we do not create arcs with radius = 0
                // LineTo( rr_edge.m_end ); // made in plotArc()
                self.plot_arc(
                    rr_edge.center,
                    -rr_edge.arc_angle_start,
                    -rr_edge.arc_angle_start + ANGLE_90,
                    a_corner_radius as f64,
                    true,
                );
            } else {
                self.line_to(rr_edge.end);
            }
        }

        self.println("G37*"); // Close region
    }

    /// Port of `GERBER_PLOTTER::FlashPadChamferRoundRect`: flash a chamfered round rect pad.
    ///
    /// `a_shape_pos` is the position of the pad shape, `a_pad_size` the size of the rectangle,
    /// `a_corner_radius` the radius of rounded corners, `a_chamfer_ratio` the chamfer value
    /// (ratio < 0.5 between smallest size and chamfer), `a_chamfer_positions` the identifier
    /// of the corners to chamfer (0 = no chamfer, 1 = TOP_LEFT, 2 = TOP_RIGHT, 4 =
    /// BOTTOM_LEFT, 8 = BOTTOM_RIGHT), `a_pad_orient` the rotation of the shape, `a_data` the
    /// Gerber attributes.  The polygon comes from `TransformRoundChamferedRectToPolygon`
    /// ([`round_chamfered_rect_to_polygon`]).
    #[allow(clippy::too_many_arguments)]
    pub fn flash_pad_chamfer_round_rect_default(
        &mut self,
        a_shape_pos: Vector2I,
        a_pad_size: Vector2I,
        a_corner_radius: i32,
        a_chamfer_ratio: f64,
        a_chamfer_positions: i32,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    ) {
        self.flash_pad_chamfer_round_rect_impl(
            a_shape_pos,
            a_pad_size,
            a_corner_radius,
            a_chamfer_ratio,
            a_chamfer_positions,
            a_pad_orient,
            a_data,
            &round_chamfered_rect_to_polygon,
        );
    }

    /// [`GerberPlotter::flash_pad_chamfer_round_rect_default`] with a caller-supplied polygon
    /// generator `a_transform` in place of `TransformRoundChamferedRectToPolygon` (see
    /// [`RoundChamferedRectToPolygonFn`]; the oracle tests replay the C++ polygons with it).
    /// Kept for the callers written before `kicad-geom` had the generator.
    #[allow(clippy::too_many_arguments)]
    pub fn flash_pad_chamfer_round_rect(
        &mut self,
        a_shape_pos: Vector2I,
        a_pad_size: Vector2I,
        a_corner_radius: i32,
        a_chamfer_ratio: f64,
        a_chamfer_positions: i32,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
        a_transform: &RoundChamferedRectToPolygonFn,
    ) {
        self.flash_pad_chamfer_round_rect_impl(
            a_shape_pos,
            a_pad_size,
            a_corner_radius,
            a_chamfer_ratio,
            a_chamfer_positions,
            a_pad_orient,
            a_data,
            a_transform,
        );
    }

    /// The body of `GERBER_PLOTTER::FlashPadChamferRoundRect`, with the polygon generator as
    /// a parameter.
    #[allow(clippy::too_many_arguments)]
    fn flash_pad_chamfer_round_rect_impl(
        &mut self,
        a_shape_pos: Vector2I,
        a_pad_size: Vector2I,
        a_corner_radius: i32,
        a_chamfer_ratio: f64,
        a_chamfer_positions: i32,
        a_pad_orient: EdaAngle,
        a_data: PlotData,
        a_transform: &RoundChamferedRectToPolygonFn,
    ) {
        let gbr_metadata = a_data.cloned().unwrap_or_default();

        let pos_device = self.user_to_device_coordinates(a_shape_pos);
        let mut corner_list: Vec<Vector2I> = Vec::new();

        let has_rounded_corner = a_corner_radius != 0 && a_chamfer_positions != 15;

        // Round rect shape or Apert Macros disabled
        if has_rounded_corner || self.gerber_disable_apert_macros {
            let outline = a_transform(
                a_shape_pos,
                a_pad_size,
                a_pad_orient,
                a_corner_radius,
                a_chamfer_ratio,
                a_chamfer_positions,
                self.plotter_arc_high_def(),
            );

            // Build the corner list
            // (C++ reads Outline( 0 ) and cornerList[0] unconditionally.)
            if outline.outline_count() == 0 || outline.c_outline(0).point_count() == 0 {
                return;
            }

            let corners = outline.c_outline(0);

            for ii in 0..corners.point_count() {
                corner_list.push(corners.c_point(ii));
            }

            // Close the polygon
            corner_list.push(corner_list[0]);

            if self.gerber_disable_apert_macros {
                self.plot_gerber_region(&corner_list, Some(&gbr_metadata));
            } else {
                // An AM will be created. the shape must be in position 0,0 and orientation 0
                // to be able to reuse the same AM for pads having the same shape
                for corner in corner_list.iter_mut() {
                    *corner -= a_shape_pos;
                    rotate_point(corner, -a_pad_orient);
                }

                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AmFreePolygon,
                    gbr_metadata.aperture_attrib() as i32,
                    gbr_metadata.custom_attribute(),
                );
                self.format_net_attribute(Some(&gbr_metadata.netlist_metadata));

                self.emit_dcode(pos_device, 3);
            }

            return;
        }

        // Build the chamfered polygon (4 to 8 corners )
        let outline = a_transform(
            Vector2I::new(0, 0),
            a_pad_size,
            ANGLE_0,
            0,
            a_chamfer_ratio,
            a_chamfer_positions,
            self.plotter_arc_high_def(),
        );

        // Build the corner list
        // Generate the polygon (4 to 8 corners )
        if outline.outline_count() > 0 {
            let corners = outline.c_outline(0);

            for ii in 0..corners.point_count() {
                corner_list.push(corners.c_point(ii));
            }
        }

        let attrib = gbr_metadata.aperture_attrib() as i32;
        let custom = gbr_metadata.custom_attribute().to_string();

        match corner_list.len() {
            4 => {
                self.has_aperture_outline4p = true;
                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AperMacroOutline4p,
                    attrib,
                    &custom,
                );
            }
            5 => {
                self.has_aperture_chamfered_rect = true;
                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AperMacroOutline5p,
                    attrib,
                    &custom,
                );
            }
            6 => {
                self.has_aperture_chamfered_rect = true;
                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AperMacroOutline6p,
                    attrib,
                    &custom,
                );
            }
            7 => {
                self.has_aperture_chamfered_rect = true;
                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AperMacroOutline7p,
                    attrib,
                    &custom,
                );
            }
            8 => {
                self.has_aperture_chamfered_rect = true;
                self.select_aperture_corners(
                    &corner_list,
                    a_pad_orient,
                    ApertureType::AperMacroOutline8p,
                    attrib,
                    &custom,
                );
            }
            _ => {
                // wxLogMessage( "FlashPadChamferRoundRect(): Unexpected number of corners
                // (%d)" ): a log message only, nothing written to the file.
            }
        }

        self.format_net_attribute(Some(&gbr_metadata.netlist_metadata));

        self.emit_dcode(pos_device, 3);
    }
}

impl Plotter for GerberPlotter {
    fn state(&self) -> &PlotterState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut PlotterState {
        &mut self.state
    }

    fn as_gerber_mut(&mut self) -> Option<&mut GerberPlotter> {
        Some(self)
    }

    fn plotter_type(&self) -> PlotFormat {
        PlotFormat::Gerber
    }

    /// Port of `GERBER_PLOTTER::StartPlot`: write GERBER header to file.
    fn start_plot(&mut self, _a_page_number: &str) -> bool {
        self.has_aperture_round_rect = false; // true is at least one round rect aperture is in use
        self.has_aperture_rot_oval = false; // true is at least one oval rotated aperture is in use
        self.has_aperture_rot_rect = false; // true is at least one rect. rotated aperture is in use
        self.has_aperture_outline4p = false; // true is at least one rotated rect/trapezoid aperture
        // is in use
        self.has_aperture_chamfered_rect = false; // true is at least one chamfered rect is in use
        self.am_freepoly_list.clear_list();

        // the actual gerber file will be created later
        self.final_file = std::mem::take(&mut self.state.output);

        // The C++ creates a temp work file here; the port writes the body to `output`.
        for ii in 0..self.state.header_extra_lines.len() {
            if !self.state.header_extra_lines[ii].is_empty() {
                let line = self.state.header_extra_lines[ii].clone();
                self.println(&line);
            }
        }

        // Set coordinate format to 3.6 or 4.5 absolute, leading zero omitted
        // the number of digits for the integer part of coordinates is needed
        // in gerber format, but is not very important when omitting leading zeros
        // It is fixed here to 3 (inch) or 4 (mm), but is not actually used
        let leading_digit_count = if self.gerber_unit_inch { 3 } else { 4 };
        let fmt = self.gerber_unit_fmt;

        self.println(&format!(
            "%FSLAX{leading_digit_count}{fmt}Y{leading_digit_count}{fmt}*%"
        ));
        self.println(&format!(
            "G04 Gerber Fmt {}.{}, Leading zero omitted, Abs format (unit {})*",
            leading_digit_count,
            fmt,
            if self.gerber_unit_inch { "inch" } else { "mm" }
        ));

        let title = format!("{} {}", self.state.creator, self.build_version);

        // In gerber files, ASCII7 chars only are allowed.
        // So use a ISO date format (using a space as separator between date and time),
        // not a localized date format
        let line = format!(
            "G04 Created by KiCad ({}) date {}*",
            title, self.creation_date
        );
        self.println(&line);

        // Mass parameter: unit = IN/MM
        if self.gerber_unit_inch {
            self.println("%MOIN*%");
        } else {
            self.println("%MOMM*%");
        }

        // Be sure the usual dark polarity is selected:
        self.println("%LPD*%");

        // Set initial interpolation mode: always G01 (linear):
        self.println("G01*");

        // Add aperture list start point
        self.println("G04 APERTURE LIST*");

        // Give a minimal value to the default pen size, used to plot items in sketch mode
        let ius_per_decimil = self.state.ius_per_decimil;

        if let Some(rs) = self.state.render_settings.as_mut() {
            // for min width = 0.1 mm
            let pen_min = f64_to_i32_x86(0.1 * ius_per_decimil * 10000.0 / 25.4);
            rs.set_default_pen_width(rs.default_pen_width().max(pen_min));
        }

        true
    }

    /// Port of `GERBER_PLOTTER::EndPlot`: append `M02*` and copy the body to the output,
    /// inserting the aperture macros and the aperture list after `G04 APERTURE LIST*`.
    fn end_plot(&mut self) -> bool {
        // Outfile is actually a temporary file i.e. workFile
        self.println("M02*");

        let work_file = std::mem::take(&mut self.state.output);
        let bytes = work_file.as_bytes();
        let mut out: Vec<u8> = std::mem::take(&mut self.final_file).into_bytes();
        out.reserve(bytes.len() + 4096);

        // Placement of apertures in RS274X
        // C++: while( fgets( line, 1024, workFile ) ): at most 1023 bytes per chunk, a
        // chunk ends after a '\n'.
        let mut pos = 0;

        while pos < bytes.len() {
            let max_end = (pos + 1023).min(bytes.len());
            let end = match bytes[pos..max_end].iter().position(|&c| c == b'\n') {
                Some(nl) => pos + nl + 1,
                None => max_end,
            };
            let line = &bytes[pos..end];
            pos = end;

            // fmt::print( "{}", line ) of a char*: stops at a NUL byte
            let printed = match line.iter().position(|&c| c == 0) {
                Some(n) => &line[..n],
                None => line,
            };
            out.extend_from_slice(printed);

            // strtok( line, "\n\r" ): the first token of non-delimiter bytes (up to a NUL)
            let tok_start = printed.iter().position(|&c| c != b'\n' && c != b'\r');

            let is_marker = match tok_start {
                Some(s) => {
                    let rest = &printed[s..];
                    let tok_len = rest
                        .iter()
                        .position(|&c| c == b'\n' || c == b'\r')
                        .unwrap_or(rest.len());
                    &rest[..tok_len] == APERTURE_LIST_MARKER
                }
                None => false,
            };

            if is_marker {
                self.state.output.clear();
                self.write_aperture_macros_and_list();
                out.extend_from_slice(self.state.output.as_bytes());
                self.state.output.clear();
            }
        }

        self.state.output = match String::from_utf8(out) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };

        true
    }

    /// Port of `GERBER_PLOTTER::SetCurrentLineWidth`.
    fn set_current_line_width(&mut self, a_width: i32, a_data: PlotData) {
        let mut a_width = a_width;

        if a_width == DO_NOT_SET_LINE_WIDTH {
            return;
        } else if a_width == USE_DEFAULT_LINE_WIDTH {
            a_width = self.state.render_settings_or_default().default_pen_width();
        }

        // wxASSERT_MSG( aWidth >= 0, "Plotter called to set negative pen width" ): the C++
        // only asserts and continues (negative widths reach here from PlotText), so no
        // assertion here.

        let mut aperture_attribute = 0;
        let mut custom_attribute = String::new();

        if let Some(md) = a_data {
            aperture_attribute = md.aperture_attrib() as i32;
            custom_attribute = md.custom_attribute().to_string();
        }

        self.select_aperture(
            Vector2I::new(a_width, a_width),
            0,
            ANGLE_0,
            ApertureType::AtPlotting,
            aperture_attribute,
            &custom_attribute,
        );
        self.state.current_pen_width = a_width;
    }

    /// RS274X has no colors.
    fn set_color(&mut self, _color: &Color4D) {}

    /// Port of `GERBER_PLOTTER::Text`: the net attributes of `a_data`, then `PLOTTER::Text`.
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
        a_data: PlotData,
    ) {
        if let Some(gbr_metadata) = a_data {
            self.format_net_attribute(Some(&gbr_metadata.netlist_metadata));
        }

        text_base(
            self,
            a_pos,
            a_color,
            a_text,
            a_orient,
            a_size,
            a_h_justify,
            a_v_justify,
            a_width,
            a_italic,
            a_bold,
            a_multiline_allowed,
            a_font,
            a_font_metrics,
            a_data,
        );
    }

    /// Port of `GERBER_PLOTTER::PlotText`: the net attributes of `a_data`, then
    /// `PLOTTER::PlotText`.
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
        if let Some(gbr_metadata) = a_data {
            self.format_net_attribute(Some(&gbr_metadata.netlist_metadata));
        }

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

    /// RS274X has no dashing.
    fn set_dash(&mut self, _a_line_width: i32, _a_line_style: LineStyle) {}

    /// Port of `GERBER_PLOTTER::SetViewport`: currently, `a_scale` and `a_mirror` are not used
    /// in gerber plotter.
    fn set_viewport(
        &mut self,
        a_offset: Vector2I,
        a_ius_per_decimil: f64,
        a_scale: f64,
        a_mirror: bool,
    ) {
        // wxASSERT( aMirror == false ) / wxASSERT( aScale == 1 ) only log in the reference
        // build: a board whose stored plot parameters ask for a mirror or a scale plots
        // normally there, so no assertion here.
        let _ = a_mirror;
        let _ = a_scale; // aScale parameter is not used in Gerber
        self.state.plot_mirror = false;
        self.state.plot_offset = a_offset;
        self.state.plot_scale = 1.0; // Plot scale is *always* 1.0

        self.state.ius_per_decimil = a_ius_per_decimil;

        // gives now a default value to iuPerDeviceUnit (because the units of the caller is
        // now known) which could be modified later by calling SetGerberCoordinatesFormat()
        self.state.iu_per_device_unit =
            10.0_f64.powi(self.gerber_unit_fmt) / (self.state.ius_per_decimil * 10000.0);

        // We don't handle the filmbox, and it's more useful to keep the
        // origin at the origin
        self.state.paper_size = Vector2I::new(0, 0);
    }

    /// Port of `GERBER_PLOTTER::SetGerberCoordinatesFormat`: selection of Gerber units and
    /// resolution (number of digits in mantissa). Should be called only after
    /// `SetViewport()` is called. `a_resolution` is the number of digits in mantissa of
    /// coordinate: use 5 or 6 for mm and 6 or 7 for inches; do not use value > 6 (mm) or > 7
    /// (in) to avoid overflow.
    fn set_gerber_coordinates_format(&mut self, a_resolution: i32, a_use_inches: bool) {
        self.gerber_unit_inch = a_use_inches;
        self.gerber_unit_fmt = a_resolution;

        self.state.iu_per_device_unit =
            10.0_f64.powi(self.gerber_unit_fmt) / (self.state.ius_per_decimil * 10000.0);

        if !self.gerber_unit_inch {
            self.state.iu_per_device_unit *= 25.4; // gerber output in mm
        }
    }

    /// Port of `GERBER_PLOTTER::Rect`. Note: do not use `Rect()` to plot rounded-corner
    /// rectangles. Use `PlotPolyAsRegion()` instead.
    fn rect(&mut self, p1: Vector2I, p2: Vector2I, fill: FillT, width: i32, a_corner_radius: i32) {
        // wxFAIL_MSG: GERBER_PLOTTER must use PlotPolyAsRegion() for rounded-corner rectangles
        let _ = a_corner_radius;

        // Build corners list
        let corner_list = [
            p1,
            Vector2I::new(p1.x, p2.y),
            p2,
            Vector2I::new(p2.x, p1.y),
            p1,
        ];

        self.plot_poly(&corner_list, fill, width, None);
    }

    /// Port of `GERBER_PLOTTER::Circle`: two arcs of 180 degrees.
    fn circle(&mut self, a_center: Vector2I, a_diameter: i32, a_fill: FillT, a_width: i32) {
        self.arc(
            a_center.into(),
            ANGLE_0,
            ANGLE_180,
            (a_diameter / 2) as f64,
            a_fill,
            a_width,
        );
        self.arc(
            a_center.into(),
            ANGLE_180,
            ANGLE_180,
            (a_diameter / 2) as f64,
            a_fill,
            a_width,
        );
    }

    /// Port of `GERBER_PLOTTER::Arc( const VECTOR2D& aCenter, const EDA_ANGLE& aStartAngle,
    /// const EDA_ANGLE& aAngle, double aRadius, FILL_T aFill, int aWidth )`.
    fn arc(
        &mut self,
        a_center: Vector2D,
        a_start_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_fill: FillT,
        a_width: i32,
    ) {
        self.set_current_line_width(a_width, None);

        let arc_length = (a_radius * a_angle.as_radians()).abs();

        if arc_length < 100.0 || a_angle.as_degrees().abs() < 0.1 {
            // Prevent plotting very short arcs as full circles, especially with 4.5 mm
            // precision. Also reduce the risk of integer overflow issues.
            let width = self.current_line_width();
            self.poly_arc(a_center, a_start_angle, a_angle, a_radius, a_fill, width);
        } else {
            let end_angle = a_start_angle + a_angle;

            // aFill is not used here.
            // (VECTOR2D -> VECTOR2I conversion of the centre truncates.)
            self.plot_arc(a_center.cast(), a_start_angle, end_angle, a_radius, false);
        }
    }

    /// Port of `GERBER_PLOTTER::PenTo`.
    fn pen_to(&mut self, a_pos: Vector2I, plume: char) {
        let pos_dev = self.user_to_device_coordinates(a_pos);

        match plume {
            'U' => self.emit_dcode(pos_dev, 2),
            'D' => self.emit_dcode(pos_dev, 1),
            _ => {} // 'Z'
        }

        self.state.pen_state = plume;
    }

    /// Port of `GERBER_PLOTTER::PlotPoly( const std::vector<VECTOR2I>& aCornerList, ... )`:
    /// Gerber polygon: they can (and *should*) be filled with the appropriate G36/G37
    /// sequence.
    fn plot_poly(
        &mut self,
        a_corner_list: &[Vector2I],
        a_fill: FillT,
        a_width: i32,
        a_data: PlotData,
    ) {
        if a_corner_list.len() <= 1 {
            return;
        }

        // Gerber format does not know filled polygons with thick outline
        // Therefore, to plot a filled polygon with outline having a thickness,
        // one should plot outline as thick segments
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        let first = a_corner_list[0];
        let last = a_corner_list[a_corner_list.len() - 1];

        if a_fill != FillT::NoFill {
            self.println("G36*");

            self.move_to(first);
            self.println("G01*"); // Set linear interpolation.

            for &pt in &a_corner_list[1..] {
                self.line_to(pt);
            }

            // If the polygon is not closed, close it:
            if first != last {
                self.finish_to(first);
            }

            self.println("G37*");
        }

        if a_width != 0 || a_fill == FillT::NoFill {
            // Draw the polyline/polygon outline
            self.set_current_line_width(a_width, a_data);

            self.move_to(first);

            for &pt in &a_corner_list[1..] {
                self.line_to(pt);
            }

            // Ensure the thick outline is closed for filled polygons
            // (if not filled, could be only a polyline)
            if a_fill != FillT::NoFill && last != first {
                self.line_to(first);
            }

            self.pen_finish();
        }
    }

    /// Port of `GERBER_PLOTTER::PlotPoly( const SHAPE_LINE_CHAIN& aPoly, ... )`: arcs of the
    /// chain are plotted as Gerber arcs; a filled chain is not stroked.
    fn plot_poly_chain(
        &mut self,
        a_poly: &ShapeLineChain,
        a_fill: FillT,
        a_width: i32,
        a_data: PlotData,
    ) {
        if a_poly.c_points().len() <= 1 {
            return;
        }

        // Gerber format does not know filled polygons with thick outline
        // Therefore, to plot a filled polygon with outline having a thickness,
        // one should plot outline as thick segments
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        let first = a_poly.c_point(0);
        let last = a_poly.c_last_point();

        if a_fill != FillT::NoFill {
            self.println("G36*");

            self.move_to(first);

            self.println("G01*"); // Set linear interpolation.

            self.plot_chain_points(a_poly);

            // If the polygon is not closed, close it:
            if first != last {
                self.finish_to(first);
            }

            self.println("G37*");
        } else if a_width != 0 {
            // Draw the polyline/polygon outline
            self.set_current_line_width(a_width, a_data);

            self.move_to(first);

            self.plot_chain_points(a_poly);

            // Ensure the thick outline is closed for filled polygons
            // (if not filled, could be only a polyline)
            if first != last && (a_poly.is_closed() || a_fill != FillT::NoFill) {
                self.line_to(first);
            }

            self.pen_finish();
        }
    }

    /// Port of `GERBER_PLOTTER::ThickSegment`.
    fn thick_segment(&mut self, start: Vector2I, end: Vector2I, width: i32, a_data: PlotData) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.set_current_line_width(width, a_data);

        // A zero-length segment is plotted as a single flash of the segment's aperture.
        // Falling through to PLOTTER::ThickSegment would emit a filled circle stroked with
        // the (now sentinel) DO_NOT_SET_LINE_WIDTH width, creating a spurious 0-size aperture.
        if start == end {
            self.move_to(start);
            self.finish_to(end);
            return;
        }

        thick_segment_base(self, start, end, DO_NOT_SET_LINE_WIDTH, a_data);
    }

    /// Port of `GERBER_PLOTTER::ThickArc( const VECTOR2D& aCentre, ... )`.
    fn thick_arc(
        &mut self,
        a_centre: Vector2D,
        a_start_angle: EdaAngle,
        a_angle: EdaAngle,
        a_radius: f64,
        a_width: i32,
        a_data: PlotData,
    ) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.set_current_line_width(a_width, a_data);
        thick_arc_base(
            self,
            a_centre,
            a_start_angle,
            a_angle,
            a_radius,
            DO_NOT_SET_LINE_WIDTH,
            a_data,
        );
    }

    /// Port of `GERBER_PLOTTER::ThickRect`.
    fn thick_rect(&mut self, p1: Vector2I, p2: Vector2I, width: i32, a_data: PlotData) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.set_current_line_width(width, a_data);
        thick_rect_base(self, p1, p2, DO_NOT_SET_LINE_WIDTH, a_data);
    }

    /// Port of `GERBER_PLOTTER::ThickCircle`.
    fn thick_circle(&mut self, pos: Vector2I, diametre: i32, width: i32, a_data: PlotData) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.set_current_line_width(width, a_data);
        thick_circle_base(self, pos, diametre, DO_NOT_SET_LINE_WIDTH, a_data);
    }

    /// Port of `GERBER_PLOTTER::FilledCircle`: a filled circle is a graphic item, not a pad.
    /// So it is drawn, not flashed.
    fn filled_circle(&mut self, pos: Vector2I, diametre: i32, a_data: PlotData) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        // Draw a circle of diameter = diameter/2 with a line thickness = radius,
        // To create a filled circle
        self.set_current_line_width(diametre / 2, a_data);
        self.circle(pos, diametre / 2, FillT::NoFill, DO_NOT_SET_LINE_WIDTH);
    }

    /// Port of `GERBER_PLOTTER::ThickPoly`.
    fn thick_poly(&mut self, a_poly: &ShapePolySet, a_width: i32, a_data: PlotData) {
        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
        }

        self.set_current_line_width(a_width, a_data);
        thick_poly_base(self, a_poly, DO_NOT_SET_LINE_WIDTH, a_data);
    }

    /// Port of `GERBER_PLOTTER::FlashPadCircle`: filled circular flashes are stored as
    /// apertures.
    fn flash_pad_circle(&mut self, pos: Vector2I, diametre: i32, a_data: PlotData) {
        let size = Vector2I::new(diametre, diametre);

        self.select_aperture_with_attributes(pos, a_data, size, 0, ANGLE_0, ApertureType::AtCircle);
    }

    /// Port of `GERBER_PLOTTER::FlashPadOval`.
    fn flash_pad_oval(
        &mut self,
        a_pos: Vector2I,
        a_size: Vector2I,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        let mut size = a_size;
        let mut orient = a_orient;
        orient.normalize();

        // Flash a vertical or horizontal shape (this is a basic aperture).
        if orient.is_cardinal() {
            if orient.is_cardinal90() {
                std::mem::swap(&mut size.x, &mut size.y);
            }

            self.select_aperture_with_attributes(
                a_pos,
                a_data,
                size,
                0,
                ANGLE_0,
                ApertureType::AtOval,
            );
        } else {
            // Plot pad as region.
            // Only regions and flashed items accept a object attribute TO.P for the pin name
            if !self.gerber_disable_apert_macros {
                self.has_aperture_rot_oval = true;
                // We are using a aperture macro that expect size.y < size.x
                // i.e draw a horizontal line for rotation = 0.0
                // size.x = length, size.y = width
                if size.x < size.y {
                    std::mem::swap(&mut size.x, &mut size.y);
                    orient += ANGLE_90;

                    if orient > ANGLE_180 {
                        orient -= ANGLE_180;
                    }
                }

                self.select_aperture_with_attributes(
                    a_pos,
                    a_data,
                    size,
                    0,
                    orient,
                    ApertureType::AmRotatedOval,
                );
                return;
            }

            // Draw the oval as round rect pad with a radius = 50% min size)
            // In gerber file, it will be drawn as a region with arcs, and can be
            // detected as pads (similar to a flashed pad)
            self.flash_pad_round_rect(a_pos, a_size, a_size.x.min(a_size.y) / 2, orient, a_data);
        }
    }

    /// Port of `GERBER_PLOTTER::FlashPadRect`.
    fn flash_pad_rect(
        &mut self,
        pos: Vector2I,
        a_size: Vector2I,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        let mut size = a_size;

        // Horizontal / vertical rect can use a basic aperture (not a macro)
        // so use it for rotation n*90 deg
        if a_orient.is_cardinal() {
            // Build the not rotated equivalent shape:
            if a_orient.is_cardinal90() {
                std::mem::swap(&mut size.x, &mut size.y);
            }

            self.select_aperture_with_attributes(
                pos,
                a_data,
                size,
                0,
                ANGLE_0,
                ApertureType::AtRect,
            );
        } else if !self.gerber_disable_apert_macros {
            self.has_aperture_rot_rect = true;

            self.select_aperture_with_attributes(
                pos,
                a_data,
                size,
                0,
                a_orient,
                ApertureType::AmRotRect,
            );
        } else {
            // plot pad shape as Gerber region
            // coord[0] is assumed the lower left
            // coord[1] is assumed the upper left
            // coord[2] is assumed the upper right
            // coord[3] is assumed the lower right
            let coord = [
                Vector2I::new(-size.x / 2, size.y / 2),  // lower left
                Vector2I::new(-size.x / 2, -size.y / 2), // upper left
                Vector2I::new(size.x / 2, -size.y / 2),  // upper right
                Vector2I::new(size.x / 2, size.y / 2),   // lower right
            ];

            self.flash_pad_trapez(pos, &coord, a_orient, a_data);
        }
    }

    /// Port of `GERBER_PLOTTER::FlashPadRoundRect`.
    fn flash_pad_round_rect(
        &mut self,
        a_pad_pos: Vector2I,
        a_size: Vector2I,
        a_corner_radius: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        if !self.gerber_disable_apert_macros {
            self.has_aperture_round_rect = true;

            self.select_aperture_with_attributes(
                a_pad_pos,
                a_data,
                a_size,
                a_corner_radius,
                a_orient,
                ApertureType::AmRoundRect,
            );
            return;
        }

        // A Pad RoundRect is plotted as a Gerber region.
        // Initialize region metadata:
        let mut clear_ta_aper_function = false; // true if a TA.AperFunction is used

        if let Some(md) = a_data {
            self.format_net_attribute(Some(&md.netlist_metadata));
            clear_ta_aper_function = self.print_region_aperture_attribute(Some(md));
        }

        // Plot the region using arcs in corners.
        self.plot_round_rect_as_region(a_pad_pos, a_size, a_corner_radius, a_orient);

        // Clear the TA attribute, to avoid the next item to inherit it:
        if clear_ta_aper_function {
            self.clear_region_aperture_attribute();
        }
    }

    /// Port of `GERBER_PLOTTER::FlashPadCustom`: a pad custom is plotted as polygon (a region
    /// in Gerber language) or as a free polygon aperture macro.
    fn flash_pad_custom(
        &mut self,
        a_pad_pos: Vector2I,
        _a_size: Vector2I,
        a_orient: EdaAngle,
        a_polygons: &ShapePolySet,
        a_data: PlotData,
    ) {
        // A Pad custom is plotted as polygon (a region in Gerber language).
        let gbr_metadata = a_data.cloned().unwrap_or_default();

        let mut corner_list: Vec<Vector2I> = Vec::new();

        for cnt in 0..a_polygons.outline_count() {
            let poly = a_polygons.c_outline(cnt);

            corner_list.clear();

            for ii in 0..poly.point_count() {
                corner_list.push(poly.c_point(ii));
            }

            // Close polygon (C++ reads cornerList[0] of an empty outline: skipped here)
            let Some(&first) = corner_list.first() else {
                continue;
            };
            corner_list.push(first);

            if self.gerber_disable_apert_macros
                || corner_list.len() > GBR_MACRO_FOR_CUSTOM_PAD_MAX_CORNER_COUNT
            {
                self.plot_gerber_region(&corner_list, Some(&gbr_metadata));
            } else {
                // An AM will be created. the shape must be in position 0,0 and orientation 0
                // to be able to reuse the same AM for pads having the same shape
                for corner in corner_list.iter_mut() {
                    *corner -= a_pad_pos;
                    rotate_point(corner, -a_orient);
                }

                let pos_dev = self.user_to_device_coordinates(a_pad_pos);
                self.select_aperture_corners(
                    &corner_list,
                    a_orient,
                    ApertureType::AmFreePolygon,
                    gbr_metadata.aperture_attrib() as i32,
                    gbr_metadata.custom_attribute(),
                );
                self.format_net_attribute(Some(&gbr_metadata.netlist_metadata));

                self.emit_dcode(pos_dev, 3);
            }
        }
    }

    /// Port of `GERBER_PLOTTER::FlashPadTrapez`.
    fn flash_pad_trapez(
        &mut self,
        a_pad_pos: Vector2I,
        a_corners: &[Vector2I; 4],
        a_pad_orient: EdaAngle,
        a_data: PlotData,
    ) {
        // polygon corners list
        let mut corner_list: Vec<Vector2I> = a_corners.to_vec();

        // Draw the polygon and fill the interior as required
        for corner in corner_list.iter_mut() {
            rotate_point(corner, a_pad_orient);
            *corner += a_pad_pos;
        }

        // Close the polygon
        corner_list.push(corner_list[0]);

        let metadata = a_data.cloned().unwrap_or_default();

        // Plot a filled polygon:
        if !self.gerber_disable_apert_macros {
            self.has_aperture_outline4p = true;
            let pos_dev = self.user_to_device_coordinates(a_pad_pos);

            // polygon corners list
            let corners = a_corners.to_vec();
            let mut aperture_attribute = 0;
            let mut custom_attribute = String::new();

            if let Some(md) = a_data {
                aperture_attribute = md.aperture_attrib() as i32;
                custom_attribute = md.custom_attribute().to_string();
            }

            self.select_aperture_corners(
                &corners,
                a_pad_orient,
                ApertureType::AperMacroOutline4p,
                aperture_attribute,
                &custom_attribute,
            );

            if let Some(md) = a_data {
                self.format_net_attribute(Some(&md.netlist_metadata));
            }

            self.emit_dcode(pos_dev, 3);
            return;
        }

        self.plot_gerber_region(&corner_list, Some(&metadata));
    }

    /// Port of `GERBER_PLOTTER::FlashRegularPolygon`.
    ///
    /// The aperture type is `AT_REGULAR_POLY3 + aCornerCount - 3`; C++ only asserts the range.
    /// A corner count whose type is not an `APERTURE_TYPE` value at all plots nothing here.
    fn flash_regular_polygon(
        &mut self,
        a_shape_pos: Vector2I,
        a_diameter: i32,
        a_corner_count: i32,
        a_orient: EdaAngle,
        a_data: PlotData,
    ) {
        let Some(apert_type) = ApertureType::from_i32(
            (ApertureType::AtRegularPoly3 as i32).wrapping_add(a_corner_count.wrapping_sub(3)),
        ) else {
            return;
        };

        debug_assert!(
            apert_type >= ApertureType::AtRegularPoly3
                && apert_type <= ApertureType::AtRegularPoly12
        );

        self.select_aperture_with_attributes(
            a_shape_pos,
            a_data,
            Vector2I::new(0, 0),
            a_diameter / 2,
            a_orient,
            apert_type,
        );
    }

    /// Port of `GERBER_PLOTTER::SetLayerPolarity`: change the plot polarity and begin a new
    /// layer. Used to 'scratch off' silk screen away from solder mask.
    fn set_layer_polarity(&mut self, a_positive: bool) {
        if a_positive {
            self.println("%LPD*%");
        } else {
            self.println("%LPC*%");
        }
    }

    /// Port of `GERBER_PLOTTER::StartBlock`: currently, it is the same as `EndBlock()`: clear
    /// all aperture net attributes.
    fn start_block(&mut self, a_data: PlotData) {
        self.end_block(a_data);
    }

    /// Port of `GERBER_PLOTTER::EndBlock`: remove all net attributes from object attributes
    /// dictionary.
    fn end_block(&mut self, _a_data: PlotData) {
        self.clear_net_attribute();
    }
}

impl GerberPlotter {
    /// The block `EndPlot` inserts after `G04 APERTURE LIST*`: the aperture macros (if any),
    /// the aperture list and `G04 APERTURE END LIST*`.
    fn write_aperture_macros_and_list(&mut self) {
        // Add aperture list macro:
        if self.has_aperture_round_rect
            || self.has_aperture_rot_oval
            || self.has_aperture_outline4p
            || self.has_aperture_rot_rect
            || self.has_aperture_chamfered_rect
            || self.am_freepoly_list.am_count() != 0
        {
            self.println("G04 Aperture macros list*");

            if self.has_aperture_round_rect {
                self.print(APER_MACRO_ROUNDRECT_HEADER);
            }

            if self.has_aperture_rot_oval {
                self.print(APER_MACRO_SHAPE_OVAL_HEADER);
            }

            if self.has_aperture_rot_rect {
                self.print(APER_MACRO_ROT_RECT_HEADER);
            }

            if self.has_aperture_outline4p {
                self.print(APER_MACRO_OUTLINE4P_HEADER);
            }

            if self.has_aperture_chamfered_rect {
                self.print(APER_MACRO_OUTLINE5P_HEADER);
                self.print(APER_MACRO_OUTLINE6P_HEADER);
                self.print(APER_MACRO_OUTLINE7P_HEADER);
                self.print(APER_MACRO_OUTLINE8P_HEADER);
            }

            if self.am_freepoly_list.am_count() != 0 {
                // aperture sizes are in inch or mm, regardless the
                // coordinates format
                let fscale = self.aperture_scale();
                let mut text = String::new();
                self.am_freepoly_list.format(&mut text, fscale);
                self.print(&text);
            }

            self.println("G04 Aperture macros list end*");
        }

        self.write_aperture_list();
        self.println("G04 APERTURE END LIST*");
    }
}
