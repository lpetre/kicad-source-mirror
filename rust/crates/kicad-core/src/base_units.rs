// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// Copyright (C) 2012-2016 Jean-Pierre Charras, jp.charras at wanadoo.fr
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

//! Port of `include/base_units.h`.
//!
//! Note about internal units and max size for boards and items
//!
//! The largest distance that we (and Kicad) can support is INT_MAX, since it represents
//! distance often in a wxCoord or wxSize. As a scalar, a distance is always
//! positive. Because int is 32 bits and INT_MAX is
//! 2147483647. The most difficult distance for a virtual (world) cartesian
//! space is the hypotenuse, or diagonal measurement at a 45 degree angle. This
//! puts the most stress on the distance magnitude within the bounded virtual
//! space. So if we allow this distance to be our constraint of <= INT_MAX, this
//! constraint then propagates to the maximum distance in X and in Y that can be
//! supported on each axis. Remember that the hypotenuse of a 1x1 square is
//! sqrt( 1x1 + 1x1 ) = sqrt(2) = 1.41421356.
//!
//! hypotenuse of any square = sqrt(2) * deltaX;
//!
//! Let maximum supported hypotenuse be INT_MAX, then:
//!
//! MAX_AXIS = INT_MAX / sqrt(2) = 2147483647 / 1.41421356 = 1518500251
//!
//! The next choice is what to use for internal units (IU), sometimes called
//! world units.  If nanometers, then the virtual space must be limited to
//! about 1.5 x 1.5 meters square.  This is 1518500251 divided by 1e9 nm/meter.
//!
//! Pcbnew uses nanometers because we need to convert coordinates and size between
//! millimeters and inches. using a iu = 1 nm avoid rounding issues
//!
//! Gerbview uses iu = 10 nm because we can have coordinates far from origin, and
//! 1 nm is too small to avoid int overflow.
//! (Conversions between millimeters and inches are not critical)

/// Gerbview IU is 10 nanometers.
pub const GERB_IU_PER_MM: f64 = 1e5;
/// Pcbnew IU is 1 nanometer.
pub const PCB_IU_PER_MM: f64 = 1e6;
/// Internal units in micron (should be enough).
pub const PL_IU_PER_MM: f64 = 1e3;
/// Schematic internal units 1=100nm.
pub const SCH_IU_PER_MM: f64 = 1e4;

/// Port of `struct EDA_IU_SCALE`: the conversion factors between internal units and
/// millimetres / mils of one application.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdaIuScale {
    /// `IU_PER_MM`
    pub iu_per_mm: f64,
    /// `IU_PER_MILS`
    pub iu_per_mils: f64,
    /// `IU_PER_PS`: internal time units are attoseconds.
    pub iu_per_ps: f64,
    /// `IU_PER_PS_PER_MM`: internal delay units are attoseconds/mm.
    pub iu_per_ps_per_mm: f64,
    /// `MM_PER_IU`
    pub mm_per_iu: f64,
    /// `MILS_PER_IU`
    pub mils_per_iu: f64,
}

impl EdaIuScale {
    /// Port of `EDA_IU_SCALE::EDA_IU_SCALE( double aIUPerMM )`.
    pub const fn new(iu_per_mm: f64) -> Self {
        let iu_per_mils = iu_per_mm * 0.0254;

        EdaIuScale {
            iu_per_mm,
            iu_per_mils,
            iu_per_ps: 1e6,
            iu_per_ps_per_mm: 1e6,
            mm_per_iu: 1.0 / iu_per_mm,
            mils_per_iu: 1.0 / iu_per_mils,
        }
    }

    /// Port of `EDA_IU_SCALE::IUTomm`.
    #[inline]
    pub const fn iu_to_mm(&self, iu: i32) -> f64 {
        iu as f64 / self.iu_per_mm
    }

    /// Port of `EDA_IU_SCALE::mmToIU`: round half away from zero by adding 0.5 and
    /// truncating, exactly like the C++ (this is not `KiROUND`).
    ///
    /// A result outside the range of `int` is undefined behaviour in C++; the port saturates.
    #[inline]
    pub const fn mm_to_iu(&self, mm: f64) -> i32 {
        (if mm < 0.0 {
            mm * self.iu_per_mm - 0.5
        } else {
            mm * self.iu_per_mm + 0.5
        }) as i32
    }

    /// Port of `EDA_IU_SCALE::MilsToIU`.
    #[inline]
    pub const fn mils_to_iu(&self, mils: i32) -> i32 {
        let x = mils as f64 * self.iu_per_mils;

        (if x < 0.0 { x - 0.5 } else { x + 0.5 }) as i32
    }

    /// Port of `EDA_IU_SCALE::IUToMils`.
    #[inline]
    pub const fn iu_to_mils(&self, iu: i32) -> i32 {
        let mils = iu as f64 / self.iu_per_mils;

        (if mils < 0.0 { mils - 0.5 } else { mils + 0.5 }) as i32
    }

    /// Port of `EDA_IU_SCALE::IUToNm`.
    #[inline]
    pub const fn iu_to_nm(&self, iu: i32) -> i64 {
        iu as i64 * ((1000000.0 / self.iu_per_mm) as i64)
    }

    /// Port of `EDA_IU_SCALE::NmToIU`.
    #[inline]
    pub const fn nm_to_iu(&self, nm: i64) -> i32 {
        let iu = nm as f64 * self.iu_per_mm / 1000000.0;

        (if iu < 0.0 { iu - 0.5 } else { iu + 0.5 }) as i32
    }

    /// Port of `EDA_IU_SCALE::NmToIUD`.
    #[inline]
    pub const fn nm_to_iu_d(&self, nm: i64) -> f64 {
        nm as f64 * self.iu_per_mm / 1000000.0
    }
}

/// `gerbIUScale`
pub const GERB_IU_SCALE: EdaIuScale = EdaIuScale::new(GERB_IU_PER_MM);
/// `pcbIUScale`
pub const PCB_IU_SCALE: EdaIuScale = EdaIuScale::new(PCB_IU_PER_MM);
/// `drawSheetIUScale`
pub const DRAW_SHEET_IU_SCALE: EdaIuScale = EdaIuScale::new(PL_IU_PER_MM);
/// `schIUScale`
pub const SCH_IU_SCALE: EdaIuScale = EdaIuScale::new(SCH_IU_PER_MM);
/// `unityScale`
pub const UNITY_SCALE: EdaIuScale = EdaIuScale::new(1.0);

/// Allowed error to approximate an arc by segments, in millimeters.
pub const ARC_LOW_DEF_MM: f64 = 0.02;
/// Allowed error to approximate an arc by segments, in millimeters.
pub const ARC_HIGH_DEF_MM: f64 = 0.005;

// The max error is the distance between the middle of a segment, and the circle
// for circle/arc to segment approximation.
// Warning: too small values can create very long calculation time in zone filling
// 0.05 to 0.005 mm are reasonable values

/// Allowed error to approximate an arc by segments, in Pcbnew IU.
pub const ARC_LOW_DEF: i32 = PCB_IU_SCALE.mm_to_iu(ARC_LOW_DEF_MM);
/// Allowed error to approximate an arc by segments, in Pcbnew IU.
pub const ARC_HIGH_DEF: i32 = PCB_IU_SCALE.mm_to_iu(ARC_HIGH_DEF_MM);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales() {
        assert_eq!(PCB_IU_SCALE.iu_per_mm, 1e6);
        assert_eq!(PCB_IU_SCALE.iu_per_mils, 25400.0);
        assert_eq!(SCH_IU_SCALE.iu_per_mils, 254.0);
        assert_eq!(PCB_IU_SCALE.mm_to_iu(1.0), 1_000_000);
        assert_eq!(PCB_IU_SCALE.mm_to_iu(-0.0000005), -1);
        assert_eq!(PCB_IU_SCALE.mm_to_iu(0.0000004), 0);
        assert_eq!(PCB_IU_SCALE.mils_to_iu(10), 254_000);
        assert_eq!(PCB_IU_SCALE.iu_to_mils(254_000), 10);
        assert_eq!(PCB_IU_SCALE.iu_to_mils(-12_700), -1);
        assert_eq!(PCB_IU_SCALE.iu_to_mm(2_500_000), 2.5);
        assert_eq!(SCH_IU_SCALE.iu_to_nm(3), 300);
        assert_eq!(SCH_IU_SCALE.nm_to_iu(250), 3);
        assert_eq!(SCH_IU_SCALE.nm_to_iu_d(250), 2.5);
        assert_eq!(ARC_LOW_DEF, 20_000);
        assert_eq!(ARC_HIGH_DEF, 5_000);
        assert_eq!(UNITY_SCALE.mm_per_iu, 1.0);
    }
}
