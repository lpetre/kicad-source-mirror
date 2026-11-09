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

//! Port of `include/plotters/gbr_plotter_apertures.h` (`APERTURE`, `APER_MACRO_FREEPOLY`,
//! `APER_MACRO_FREEPOLY_LIST`) and of their implementation at the end of
//! `common/plotters/GERBER_plotter.cpp` (`polyCompare`, `APER_MACRO_FREEPOLY::Format`,
//! `APER_MACRO_FREEPOLY_LIST::*`).
//!
//! Class to handle a D_CODE when plotting a board using Standard Aperture Templates (complex
//! apertures need aperture macros to be flashed). 5 types: circle (round), rectangle, obround
//! (oval), regular polygon. We need round apertures to plot lines, so we also defined an
//! aperture type for plotting. Other aperture types are aperture macros.

use kicad_core::printf::format_f;
use kicad_geom::{EdaAngle, Vector2I};

/// `FIRST_DCODE_VALUE`: D_CODE < 10 is a command, D_CODE >= 10 is a tool.
pub const FIRST_DCODE_VALUE: i32 = 10;

/// `AM_FREEPOLY_BASENAME` (`GERBER_plotter.cpp`): name prefix of the on-the-fly free polygon
/// aperture macros.
pub const AM_FREEPOLY_BASENAME: &str = "FreePoly";

/// Port of `APERTURE::APERTURE_TYPE`. The integer values matter for the regular polygon
/// arithmetic (`AT_REGULAR_POLY3 - 3 + n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum ApertureType {
    /// `AT_CIRCLE`: round aperture, to flash pads
    AtCircle = 1,
    /// `AT_RECT`: rect aperture, to flash pads
    AtRect = 2,
    /// `AT_PLOTTING`: round aperture, to plot lines
    AtPlotting = 3,
    /// `AT_OVAL`: oval aperture, to flash pads
    AtOval = 4,
    /// `AT_REGULAR_POLY`: regular polygon (n vertices, n = 3 .. 12, with rotation)
    AtRegularPoly = 5,
    /// `AT_REGULAR_POLY3`: regular polygon 3 vertices, with rotation
    AtRegularPoly3,
    /// `AT_REGULAR_POLY4`
    AtRegularPoly4,
    /// `AT_REGULAR_POLY5`
    AtRegularPoly5,
    /// `AT_REGULAR_POLY6`
    AtRegularPoly6,
    /// `AT_REGULAR_POLY7`
    AtRegularPoly7,
    /// `AT_REGULAR_POLY8`
    AtRegularPoly8,
    /// `AT_REGULAR_POLY9`
    AtRegularPoly9,
    /// `AT_REGULAR_POLY10`
    AtRegularPoly10,
    /// `AT_REGULAR_POLY11`
    AtRegularPoly11,
    /// `AT_REGULAR_POLY12`
    AtRegularPoly12,
    /// `AM_ROUND_RECT`: aperture macro for round rect pads
    AmRoundRect,
    /// `AM_ROT_RECT`: aperture macro for rotated rect pads
    AmRotRect,
    /// `APER_MACRO_OUTLINE4P`: aperture macro for trapezoid pads (outline with 4 corners)
    AperMacroOutline4p,
    /// `APER_MACRO_OUTLINE5P`: aperture macro for pad polygons with 5 corners (chamfered pads)
    AperMacroOutline5p,
    /// `APER_MACRO_OUTLINE6P`: aperture macro for pad polygons with 6 corners (chamfered pads)
    AperMacroOutline6p,
    /// `APER_MACRO_OUTLINE7P`: aperture macro for pad polygons with 7 corners (chamfered pads)
    AperMacroOutline7p,
    /// `APER_MACRO_OUTLINE8P`: aperture macro for pad polygons with 8 corners (chamfered pads)
    AperMacroOutline8p,
    /// `AM_ROTATED_OVAL`: aperture macro for rotated oval pads (not rotated uses a primitive)
    AmRotatedOval,
    /// `AM_FREE_POLYGON`: aperture macro to create on the fly a free polygon, with only one
    /// parameter: rotation
    AmFreePolygon,
}

impl ApertureType {
    const ALL: [ApertureType; 24] = [
        Self::AtCircle,
        Self::AtRect,
        Self::AtPlotting,
        Self::AtOval,
        Self::AtRegularPoly,
        Self::AtRegularPoly3,
        Self::AtRegularPoly4,
        Self::AtRegularPoly5,
        Self::AtRegularPoly6,
        Self::AtRegularPoly7,
        Self::AtRegularPoly8,
        Self::AtRegularPoly9,
        Self::AtRegularPoly10,
        Self::AtRegularPoly11,
        Self::AtRegularPoly12,
        Self::AmRoundRect,
        Self::AmRotRect,
        Self::AperMacroOutline4p,
        Self::AperMacroOutline5p,
        Self::AperMacroOutline6p,
        Self::AperMacroOutline7p,
        Self::AperMacroOutline8p,
        Self::AmRotatedOval,
        Self::AmFreePolygon,
    ];

    /// `(APERTURE_TYPE) value`; `None` for a value outside the enum.
    pub fn from_i32(value: i32) -> Option<ApertureType> {
        usize::try_from(value - 1)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }

    /// The C++ integer value.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Port of `APERTURE`: one D-code of a Gerber file.
#[derive(Clone, Debug, PartialEq)]
pub struct Aperture {
    /// Type (line, rect, circular, oval, poly 3 to 12 vertices, aperture macro)
    pub type_: ApertureType,
    /// horiz and vert size
    pub size: Vector2I,
    /// list of corners for polygon shape
    pub corners: Vec<Vector2I>,
    /// Radius for polygon and round rect shape
    pub radius: i32,
    /// Rotation in degrees
    pub rotation: EdaAngle,
    /// code number ( >= 10 )
    pub d_code: i32,
    /// the attribute attached to this aperture. Only one attribute is allowed by aperture.
    /// 0 = no specific aperture attribute (the int value of a `GbrApertureAttrib`)
    pub aperture_attribute: i32,
    /// the text of a custom (`Other`) aperture attribute
    pub custom_attribute: String,
}

impl Aperture {
    /// A new aperture of type `a_type` with all other members zero / empty (the C++ class
    /// has no constructor; `GERBER_PLOTTER::GetOrCreateAperture` sets every member).
    pub fn new(a_type: ApertureType) -> Self {
        Aperture {
            type_: a_type,
            size: Vector2I::new(0, 0),
            corners: Vec::new(),
            radius: 0,
            rotation: EdaAngle::default(),
            d_code: 0,
            aperture_attribute: 0,
            custom_attribute: String::new(),
        }
    }

    /// Port of `APERTURE::SetSize`.
    pub fn set_size(&mut self, a_size: Vector2I) {
        self.size = a_size;
    }

    /// Port of `APERTURE::GetSize`.
    pub fn size(&self) -> Vector2I {
        self.size
    }

    /// Port of `APERTURE::SetDiameter`.
    pub fn set_diameter(&mut self, a_diameter: i32) {
        self.radius = a_diameter / 2;
    }

    /// Port of `APERTURE::GetDiameter`.
    pub fn diameter(&self) -> i32 {
        // For round primitive, the diameter is the m_Size.x ot m_Size.y
        if self.type_ == ApertureType::AtCircle || self.type_ == ApertureType::AtPlotting {
            return self.size.x;
        }

        // For rounded shapes (macro apertures), return m_Radius * 2
        // but usually they use the radius (m_Radius)
        self.radius * 2
    }

    /// Port of `APERTURE::SetRegPolyVerticeCount`.
    pub fn set_reg_poly_vertice_count(&mut self, a_count: i32) {
        let a_count = a_count.clamp(3, 12);
        self.type_ = ApertureType::from_i32(ApertureType::AtRegularPoly3 as i32 - 3 + a_count)
            .unwrap_or(ApertureType::AtRegularPoly3);
    }

    /// Port of `APERTURE::GetRegPolyVerticeCount`.
    pub fn reg_poly_vertice_count(&self) -> i32 {
        self.type_ as i32 - ApertureType::AtRegularPoly3 as i32 + 3
    }

    /// Port of `APERTURE::SetRotation`.
    pub fn set_rotation(&mut self, a_rotation: EdaAngle) {
        self.rotation = a_rotation;
    }

    /// Port of `APERTURE::GetRotation`.
    pub fn rotation(&self) -> EdaAngle {
        self.rotation
    }
}

/// Port of `polyCompare` (`GERBER_plotter.cpp`): true if both polygons have the same size and
/// every corner matches within 2 IU.
pub fn poly_compare(a_polygon: &[Vector2I], a_test_polygon: &[Vector2I]) -> bool {
    // fast test: polygon sizes must be the same:
    if a_test_polygon.len() != a_polygon.len() {
        return false;
    }

    const MARGIN: i32 = 2;

    for jj in 0..a_polygon.len() {
        if (a_polygon[jj].x - a_test_polygon[jj].x).abs() > MARGIN
            || (a_polygon[jj].y - a_test_polygon[jj].y).abs() > MARGIN
        {
            return false;
        }
    }

    true
}

/// Port of `APER_MACRO_FREEPOLY`: an aperture macro based on a free polygon, i.e. using a
/// primitive 4 to describe a free polygon with a rotation.
///
/// The aperture macro has only one parameter: rotation and is defined on the fly for a Gerber
/// file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AperMacroFreepoly {
    /// the polygon corners
    pub corners: Vec<Vector2I>,
    /// the macro id (`FreePoly<id>`)
    pub id: i32,
}

impl AperMacroFreepoly {
    /// Port of `APER_MACRO_FREEPOLY::APER_MACRO_FREEPOLY`.
    pub fn new(a_polygon: &[Vector2I], a_id: i32) -> Self {
        AperMacroFreepoly {
            corners: a_polygon.to_vec(),
            id: a_id,
        }
    }

    /// Port of `APER_MACRO_FREEPOLY::IsSamePoly`: true if `a_polygon` is the same as this.
    pub fn is_same_poly(&self, a_polygon: &[Vector2I]) -> bool {
        poly_compare(&self.corners, a_polygon)
    }

    /// Port of `APER_MACRO_FREEPOLY::Format`: print the aperture macro definition to
    /// `a_output`. `a_iu2_gbr_macro_unit` is the scaling factor from coordinates value to the
    /// Gerber file macros units (always mm or inches).
    pub fn format(&self, a_output: &mut String, a_iu2_gbr_macro_unit: f64) {
        // Write aperture header
        a_output.push_str(&format!("%AM{}{}*\n", AM_FREEPOLY_BASENAME, self.id));
        a_output.push_str(&format!("4,1,{},", self.corners.len() as i32));

        // Insert a newline after curr_line_count_max coordinates.
        let mut curr_line_corner_count = 0;
        let curr_line_count_max = 20; // <= 0 to disable newlines

        for ii in 0..=self.corners.len() {
            let jj = if ii >= self.corners.len() { 0 } else { ii };

            // Note: parameter values are always mm or inches
            a_output.push_str(&format_f(
                self.corners[jj].x as f64 * a_iu2_gbr_macro_unit,
                6,
            ));
            a_output.push(',');
            a_output.push_str(&format_f(
                self.corners[jj].y.wrapping_neg() as f64 * a_iu2_gbr_macro_unit,
                6,
            ));
            a_output.push(',');

            curr_line_corner_count += 1;

            if curr_line_count_max >= 0 && curr_line_corner_count >= curr_line_count_max {
                a_output.push('\n');
                curr_line_corner_count = 0;
            }
        }

        // output rotation parameter
        a_output.push_str("$1*%\n");
    }
}

/// Port of `APER_MACRO_FREEPOLY_LIST`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AperMacroFreepolyList {
    /// the macros in creation order
    pub am_list: Vec<AperMacroFreepoly>,
}

impl AperMacroFreepolyList {
    /// Port of `APER_MACRO_FREEPOLY_LIST::APER_MACRO_FREEPOLY_LIST`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `APER_MACRO_FREEPOLY_LIST::ClearList`.
    pub fn clear_list(&mut self) {
        self.am_list.clear();
    }

    /// Port of `APER_MACRO_FREEPOLY_LIST::AmCount`.
    pub fn am_count(&self) -> i32 {
        self.am_list.len() as i32
    }

    /// Port of `APER_MACRO_FREEPOLY_LIST::Append`: append a new `APER_MACRO_FREEPOLY`
    /// containing the polygon `a_polygon` to the current list.
    pub fn append(&mut self, a_polygon: &[Vector2I]) {
        let id = self.am_count();
        self.am_list.push(AperMacroFreepoly::new(a_polygon, id));
    }

    /// Port of `APER_MACRO_FREEPOLY_LIST::FindAm`: the index of the macro having the same
    /// polygon as `a_polygon`, or -1.
    pub fn find_am(&self, a_polygon: &[Vector2I]) -> i32 {
        for idx in 0..self.am_count() {
            if self.am_list[idx as usize].is_same_poly(a_polygon) {
                return idx;
            }
        }

        -1
    }

    /// Port of `APER_MACRO_FREEPOLY_LIST::Format`: print the aperture macro list.
    pub fn format(&self, a_output: &mut String, a_iu2_gbr_macro_unit: f64) {
        for am in &self.am_list {
            am.format(a_output, a_iu2_gbr_macro_unit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aperture_helpers() {
        let mut ap = Aperture::new(ApertureType::AtCircle);
        ap.set_size(Vector2I::new(200, 200));
        assert_eq!(ap.diameter(), 200);
        ap.set_reg_poly_vertice_count(2);
        assert_eq!(ap.type_, ApertureType::AtRegularPoly3);
        assert_eq!(ap.reg_poly_vertice_count(), 3);
        ap.set_reg_poly_vertice_count(40);
        assert_eq!(ap.type_, ApertureType::AtRegularPoly12);
        ap.set_diameter(301);
        assert_eq!(ap.diameter(), 300);
    }

    #[test]
    fn freepoly_format() {
        let mut list = AperMacroFreepolyList::new();
        let poly = [
            Vector2I::new(0, 0),
            Vector2I::new(1000, 0),
            Vector2I::new(1000, -1000),
        ];
        list.append(&poly);
        assert_eq!(
            list.find_am(&[
                Vector2I::new(2, -2),
                Vector2I::new(998, 0),
                Vector2I::new(1000, -1000)
            ]),
            0
        );
        assert_eq!(
            list.find_am(&[
                Vector2I::new(3, 0),
                Vector2I::new(1000, 0),
                Vector2I::new(1000, -1000)
            ]),
            -1
        );
        let mut out = String::new();
        list.format(&mut out, 1e-6);
        assert_eq!(
            out,
            "%AMFreePoly0*\n4,1,3,0.000000,0.000000,0.001000,0.000000,0.001000,0.001000,0.000000,0.000000,$1*%\n"
        );
    }
}
