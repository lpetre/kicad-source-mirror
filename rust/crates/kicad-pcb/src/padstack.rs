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

//! Port of `pcbnew/padstack.h` and `pcbnew/padstack.cpp`.
//!
//! Placeholder written by M0a; the type package fills in the definitions and `Default`
//! (`board-model.md` 16).

/// `PADSTACK`.
///
/// PLACEHOLDER (M0a): M0b replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct Padstack {}

impl Padstack {
    /// The padstack of a new `PCB_VIA`: `PADSTACK( this )` for a via followed by the padstack
    /// statements of the `PCB_VIA` constructor (drill start F_Cu, end B_Cu, `SetDrillDefault()`,
    /// `UNCONNECTED_LAYER_MODE::KEEP_ALL`, layer set reset, circle shape on ALL_LAYERS).
    /// PLACEHOLDER (M0a): M0b ports the body; `PcbVia::default()` calls it.
    pub fn for_new_via() -> Padstack {
        Padstack::default()
    }
}
