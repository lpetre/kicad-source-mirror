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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/geometry/grid_geometry.h` and
//! `libs/kimath/src/geometry/grid_geometry.cpp` (`GRID_GEOMETRY`).

use core::f64::consts::PI;

use crate::cmath;
use crate::geometry::eda_angle::{EdaAngle, RADIANS_T};
use crate::math::vector2::Vector2D;
use crate::trigo::get_rotated;

/// Port of `GRID_GEOMETRY::KIND`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridKind {
    /// `CARTESIAN`
    #[default]
    Cartesian,
    /// `POLAR`
    Polar,
}

/// `std::clamp( int v, lo, hi )` as GCC 13's libstdc++ implements it, also for `hi < lo`
/// (undefined in C++): `std::min( std::max( v, lo ), hi )`.
#[inline]
fn clamp_i(v: i32, lo: i32, hi: i32) -> i32 {
    let m = if v < lo { lo } else { v };
    if hi < m { hi } else { m }
}

/// `static_cast<int>( std::round( v ) )` on the reference build.
#[inline]
fn round_to_int(v: f64) -> i32 {
    cmath::f64_to_i32_x86(v.round())
}

/// Port of `GRID_GEOMETRY`: geometry of a regular grid (cartesian or polar) and the math
/// operations for snapping and coverage testing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridGeometry {
    /// `kind`
    pub kind: GridKind,
    /// `origin`
    pub origin: Vector2D,
    /// `pitch`: Cartesian: (dx, dy); polar: (dr, dPhi rad).
    pub pitch: Vector2D,
    /// `orientation`: rotation about origin, radians CCW.
    pub orientation: f64,
    /// `extent`: Cartesian: (dx, dy) = size/2 from origin.  Polar: (rMax, phiMax rad).
    pub extent: Vector2D,
    /// `priority`: higher wins where grids overlap.  0 is reserved for the background grid;
    /// see `takes_precedence_over`.
    pub priority: u32,
}

impl GridGeometry {
    /// Port of `GRID_GEOMETRY::Snap`: snap a point to the nearest on-grid position.
    ///
    /// Returns `a_point` unchanged for degenerate geometry (non-positive pitch).
    pub fn snap(&self, a_point: Vector2D) -> Vector2D {
        let local: Vector2D = get_rotated(
            a_point - self.origin,
            EdaAngle::new(-self.orientation, RADIANS_T),
        );
        let candidate: Vector2D = match self.kind {
            GridKind::Polar => {
                let dr = self.pitch.x;
                let d_phi = self.pitch.y;
                let r_max = self.extent.x;
                let phi_max = self.extent.y;

                if dr <= 0.0 || d_phi <= 0.0 {
                    return a_point;
                }

                let r = cmath::hypot(local.x, local.y);
                let max_r_idx = cmath::f64_to_i32_x86(r_max / dr);
                let r_idx = clamp_i(round_to_int(r / dr), 0, max_r_idx);

                // Normalise phi to [0, 2*pi) and clamp to the last drawn spoke; otherwise
                // a cursor just inside the wedge rounds up past the last spoke.
                let mut phi = cmath::atan2(local.y, local.x);

                if phi < 0.0 {
                    phi += 2.0 * PI;
                }

                let max_p_idx = cmath::f64_to_i32_x86(phi_max / d_phi);
                let p_idx = clamp_i(round_to_int(phi / d_phi), 0, max_p_idx);

                let phi_q = p_idx as f64 * d_phi;
                let r_snapped = r_idx as f64 * dr;

                Vector2D::new(r_snapped * cmath::cos(phi_q), r_snapped * cmath::sin(phi_q))
            }
            GridKind::Cartesian => {
                let dx = self.pitch.x;
                let dy = self.pitch.y;

                if dx <= 0.0 || dy <= 0.0 {
                    return a_point;
                }

                // Centred on origin; extent is size/2.  Indices go -N..+N.
                let n_x = cmath::f64_to_i32_x86(self.extent.x / dx);
                let n_y = cmath::f64_to_i32_x86(self.extent.y / dy);
                let x_idx = clamp_i(round_to_int(local.x / dx), n_x.wrapping_neg(), n_x);
                let y_idx = clamp_i(round_to_int(local.y / dy), n_y.wrapping_neg(), n_y);

                Vector2D::new(x_idx as f64 * dx, y_idx as f64 * dy)
            }
        };

        get_rotated(candidate, EdaAngle::new(self.orientation, RADIANS_T)) + self.origin
    }

    /// Port of `GRID_GEOMETRY::Contains`: true if `a_point` is within `a_tolerance` of the
    /// grid's coverage.
    pub fn contains(&self, a_point: Vector2D, a_tolerance: f64) -> bool {
        let local: Vector2D = get_rotated(
            a_point - self.origin,
            EdaAngle::new(-self.orientation, RADIANS_T),
        );

        match self.kind {
            GridKind::Polar => {
                let r = cmath::hypot(local.x, local.y);

                if r > self.extent.x + a_tolerance {
                    return false;
                }

                let phi_max = self.extent.y;
                let mut phi = cmath::atan2(local.y, local.x);

                while phi < 0.0 {
                    phi += 2.0 * PI;
                }

                phi <= phi_max.abs()
            }
            GridKind::Cartesian => {
                local.x >= -self.extent.x - a_tolerance
                    && local.x <= self.extent.x + a_tolerance
                    && local.y >= -self.extent.y - a_tolerance
                    && local.y <= self.extent.y + a_tolerance
            }
        }
    }

    /// Port of `GRID_GEOMETRY::Area`: the area of the grid's coverage region.  Used as a
    /// tiebreaker between overlapping grids of equal priority - smaller wins (more specific).
    pub fn area(&self) -> f64 {
        match self.kind {
            // Polar wedge: 0.5 * r^2 * phi.
            GridKind::Polar => 0.5 * self.extent.x * self.extent.x * self.extent.y.abs(),
            // Centred rectangle: 4 * (size/2).x * (size/2).y.
            GridKind::Cartesian => (4.0 * self.extent.x * self.extent.y).abs(),
        }
    }

    /// Port of `GRID_GEOMETRY::TakesPrecedenceOver`.
    ///
    /// True if this grid owns a point that both grids cover: higher priority first, then the
    /// smaller (more specific) area.  The Compare for `std::stable_sort` - the winner sorts
    /// FIRST, ties keep document order.  The background grid is the only one at priority 0,
    /// so it always sorts last.
    pub fn takes_precedence_over(&self, a_other: &GridGeometry) -> bool {
        if self.priority != a_other.priority {
            return self.priority > a_other.priority;
        }

        self.area() < a_other.area()
    }
}
