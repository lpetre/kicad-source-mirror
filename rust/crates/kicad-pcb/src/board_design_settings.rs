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

//! Port of `pcbnew/board_design_settings.h` and `pcbnew/board_design_settings.cpp`.
//!
//! Placeholder written by M0a; the type package fills in the definitions and `Default`
//! (`board-model.md` 16).

/// `BOARD_DESIGN_SETTINGS`.
///
/// PLACEHOLDER (M0a): M0d replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct BoardDesignSettings {}

impl BoardDesignSettings {
    /// `m_MaxError` (what `BOARD_ITEM::GetMaxError()` returns for an item on a board).
    /// PLACEHOLDER (M0a): returns the constructor value `ARC_HIGH_DEF`; M0d returns the field.
    pub fn max_error(&self) -> i32 {
        kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF
    }
}
