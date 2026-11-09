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

//! Port of `libs/kimath/include/geometry/arc_chord_params.h` and
//! `libs/kimath/src/geometry/arc_chord_params.cpp`.

use crate::geometry::eda_angle::EdaAngle;
use crate::math::util::std_clamp;
use crate::math::vector2::{Vector2D, Vector2I};

/// Port of `ARC_CHORD_PARAMS`: arc geometry computed from a chord-based coordinate system.
///
/// This class computes arc parameters using a coordinate system where:
/// - u: unit vector along the chord from start to end
/// - n: unit vector perpendicular to the chord, pointing toward the arc center
///
/// This representation allows arc points to be generated without computing the
/// center point explicitly, which provides better numerical stability for
/// shallow arcs where the center point may be very far from the arc itself.
///
/// The class can compute the center point when needed via `center_point()`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArcChordParams {
    chord_len: f64,
    half_chord: f64,
    /// Unit vector along chord (from start to end)
    ux: f64,
    uy: f64,
    /// Unit vector perpendicular to chord, toward arc center
    nx: f64,
    ny: f64,
    sagitta: f64,
    radius: f64,
    /// Distance from chord midpoint to arc center along n
    center_offset: f64,
    /// Chord midpoint coordinates
    midx: f64,
    midy: f64,
    valid: bool,
}

impl ArcChordParams {
    /// A default (invalid) set of parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `ARC_CHORD_PARAMS::Compute()`: compute arc geometry from three points defining
    /// the arc.  Returns false if degenerate (collinear points).
    pub fn compute(&mut self, a_start: Vector2I, a_mid: Vector2I, a_end: Vector2I) -> bool {
        self.valid = false;

        let dx: f64 = a_end.x as f64 - a_start.x as f64;
        let dy: f64 = a_end.y as f64 - a_start.y as f64;
        self.chord_len = (dx * dx + dy * dy).sqrt();

        if self.chord_len <= 0.0 {
            return false;
        }

        let mx: f64 = a_mid.x as f64 - a_start.x as f64;
        let my: f64 = a_mid.y as f64 - a_start.y as f64;
        let cross: f64 = mx * dy - my * dx;

        if cross == 0.0 {
            return false;
        }

        self.sagitta = cross.abs() / self.chord_len;

        if self.sagitta <= 0.0 {
            return false;
        }

        self.half_chord = self.chord_len / 2.0;
        self.radius = (self.half_chord * self.half_chord + self.sagitta * self.sagitta) / (2.0 * self.sagitta);

        if self.radius <= 0.0 {
            return false;
        }

        self.ux = dx / self.chord_len;
        self.uy = dy / self.chord_len;
        self.nx = -self.uy;
        self.ny = self.ux;

        if cross < 0.0 {
            self.nx = -self.nx;
            self.ny = -self.ny;
        }

        self.center_offset = self.radius - self.sagitta;
        self.midx = (a_start.x as f64 + a_end.x as f64) * 0.5;
        self.midy = (a_start.y as f64 + a_end.y as f64) * 0.5;
        self.valid = true;
        true
    }

    /// Port of `IsValid()`.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Port of `GetChordLength()`: distance from start to end.
    pub fn chord_length(&self) -> f64 {
        self.chord_len
    }

    /// Port of `GetHalfChord()`.
    pub fn half_chord(&self) -> f64 {
        self.half_chord
    }

    /// Port of `GetChordUnitVec()`: unit vector along the chord (from start to end).
    pub fn chord_unit_vec(&self) -> Vector2D {
        Vector2D::new(self.ux, self.uy)
    }

    /// Port of `GetNormalUnitVec()`: unit vector perpendicular to the chord, pointing toward
    /// the arc center.
    pub fn normal_unit_vec(&self) -> Vector2D {
        Vector2D::new(self.nx, self.ny)
    }

    /// Port of `GetSagitta()`: perpendicular distance from chord midpoint to arc.
    pub fn sagitta(&self) -> f64 {
        self.sagitta
    }

    /// Port of `GetRadius()`.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Port of `GetCenterOffset()`: distance from chord midpoint to arc center along the normal.
    pub fn center_offset(&self) -> f64 {
        self.center_offset
    }

    /// Port of `GetChordMidpoint()`.
    pub fn chord_midpoint(&self) -> Vector2D {
        Vector2D::new(self.midx, self.midy)
    }

    /// Port of `GetCenterPoint()`.  Note: for shallow arcs, the center may be very far from the
    /// arc itself.
    pub fn center_point(&self) -> Vector2D {
        // Center is at chord midpoint + center_offset along the normal direction
        Vector2D::new(
            self.midx + self.nx * self.center_offset,
            self.midy + self.ny * self.center_offset,
        )
    }

    /// Port of `GetStartAngle()`: angle from arc center to the start point.
    ///
    /// In the chord coordinate system, start is at u = -half_chord, so the direction
    /// from center to start is: -sin(half_angle) * u - cos(half_angle) * n
    pub fn start_angle(&self) -> EdaAngle {
        let sin_half: f64 = self.half_chord / self.radius;
        let cos_half: f64 = self.center_offset / self.radius;

        // Direction from center to start: -sin(half_angle) * u - cos(half_angle) * n
        EdaAngle::from_vector(Vector2D::new(
            -sin_half * self.ux - cos_half * self.nx,
            -sin_half * self.uy - cos_half * self.ny,
        ))
    }

    /// Port of `GetEndAngle()`: angle from arc center to the end point.
    ///
    /// In the chord coordinate system, end is at u = +half_chord, so the direction
    /// from center to end is: sin(half_angle) * u - cos(half_angle) * n
    pub fn end_angle(&self) -> EdaAngle {
        let sin_half: f64 = self.half_chord / self.radius;
        let cos_half: f64 = self.center_offset / self.radius;

        // Direction from center to end: sin(half_angle) * u - cos(half_angle) * n
        EdaAngle::from_vector(Vector2D::new(
            sin_half * self.ux - cos_half * self.nx,
            sin_half * self.uy - cos_half * self.ny,
        ))
    }

    /// Port of `GetArcAngle()`: the total angle swept by the arc, in radians.
    pub fn arc_angle(&self) -> f64 {
        let ratio: f64 = std_clamp(self.half_chord / self.radius, 0.0, 1.0);
        let base_angle: f64 = 2.0 * crate::cmath::asin(ratio);

        if self.sagitta > self.radius {
            return 2.0 * std::f64::consts::PI - base_angle;
        }

        base_angle
    }

    /// Port of `GetUx()`.
    pub fn ux(&self) -> f64 {
        self.ux
    }

    /// Port of `GetUy()`.
    pub fn uy(&self) -> f64 {
        self.uy
    }

    /// Port of `GetNx()`.
    pub fn nx(&self) -> f64 {
        self.nx
    }

    /// Port of `GetNy()`.
    pub fn ny(&self) -> f64 {
        self.ny
    }

    /// Port of `GetMidX()`.
    pub fn mid_x(&self) -> f64 {
        self.midx
    }

    /// Port of `GetMidY()`.
    pub fn mid_y(&self) -> f64 {
        self.midy
    }
}
