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

//! Port of `pcbnew/zone_settings.h` and `pcbnew/zone_settings.cpp` (`ZONE_SETTINGS`) and of
//! `ZONE_CONNECTION` (`pcbnew/zones.h`).
//!
//! Placeholder written by M0a (`ZoneConnection` is complete: `drc_api` needs it); M0b fills in
//! `ZoneSettings` and the other zone enums (`board-model.md` 16).

/// `ZONE_CONNECTION`; the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ZoneConnection {
    /// `ZONE_CONNECTION::INHERITED`.
    #[default]
    Inherited = -1,
    /// `ZONE_CONNECTION::NONE`: pads are not covered.
    None = 0,
    /// `ZONE_CONNECTION::THERMAL`: use thermal relief for pads.
    Thermal,
    /// `ZONE_CONNECTION::FULL`: pads are covered by copper.
    Full,
    /// `ZONE_CONNECTION::THT_THERMAL`: thermal relief only for THT pads.
    ThtThermal,
}

/// `ZONE_SETTINGS`.
///
/// PLACEHOLDER (M0a): M0b replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct ZoneSettings {}
