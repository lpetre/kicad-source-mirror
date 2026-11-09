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

//! Port of the layer functions of `pcbnew/board.cpp` (`GetCopperLayerCount`, `FlipLayer`,
//! `GetLayerName`, `SetLayerDescr`, ...) and of `LAYER` / `LAYER_T` (`pcbnew/board.h`).
//! Package M22 (types: M0d); M0a wrote the signatures the kernel calls.

use kicad_core::PcbLayerId;

use crate::board::Board;
use crate::ctx::LayerEnv;

/// `LAYER_T`; the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum LayerT {
    #[default]
    Undefined = -1,
    Signal = 0,
    Power,
    Mixed,
    Jumper,
    Aux,
    Front,
    Back,
}

/// `LAYER` (`pcbnew/board.h`).
///
/// PLACEHOLDER (M0a): M0d replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct Layer {}

impl Board {
    /// `BOARD::GetCopperLayerCount()`.
    pub fn copper_layer_count(&self) -> i32 {
        crate::port_pending!("BOARD::GetCopperLayerCount")
    }

    /// `BOARD::FlipLayer( aLayer )` (copper-count aware, user-layer opposites included).
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId {
        crate::port_pending!("BOARD::FlipLayer", layer)
    }

    /// Build the `LayerEnv` snapshot from the current settings and layers
    /// (`board-model-types.md` 0: copper count, enabled layers, `FlipLayer` table).
    pub(crate) fn build_layer_env(&self) -> LayerEnv {
        crate::port_pending!(
            "LayerEnv from BOARD::GetCopperLayerCount / GetEnabledLayers / FlipLayer"
        )
    }
}
