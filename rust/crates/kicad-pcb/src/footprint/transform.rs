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

//! Port of the placement cascades of `pcbnew/footprint.cpp` (`FOOTPRINT::SetPosition`,
//! `SetOrientation`, `Flip`, `SetTransformScale`, `RescaleAroundPoint`,
//! `MoveAnchorPosition`, `SetLayerAndFlip`, `Move`, `Rotate`) as `Board::footprint_*`
//! (`board-model.md` 8.3, package M19). Each updates `Footprint::transform` / `flipped`,
//! builds one `MutEnv` with the new `ParentFrame`, runs the C++ loop over the children in C++
//! order and patches the footprint caches where C++ patches them. They work the same on a
//! created-but-unattached footprint.

use kicad_core::PcbLayerId;
use kicad_geom::{EdaAngle, FlipDirection, Vector2I};

use crate::board::Board;
use crate::ids::FootprintId;

impl Board {
    /// `FOOTPRINT::SetPosition( aPos )`.
    pub fn footprint_set_position(&mut self, fp: FootprintId, pos: Vector2I) {
        crate::port_pending!("FOOTPRINT::SetPosition", fp, pos)
    }

    /// `FOOTPRINT::SetOrientation( aNewAngle )`.
    pub fn footprint_set_orientation(&mut self, fp: FootprintId, angle: EdaAngle) {
        crate::port_pending!("FOOTPRINT::SetOrientation", fp, angle)
    }

    /// `FOOTPRINT::Flip( aCentre, aFlipDirection )`.
    pub fn footprint_flip(&mut self, fp: FootprintId, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("FOOTPRINT::Flip", fp, centre, dir)
    }

    /// `FOOTPRINT::SetTransformScale( aScaleX, aScaleY )`.
    pub fn footprint_set_transform_scale(&mut self, fp: FootprintId, scale_x: f64, scale_y: f64) {
        crate::port_pending!("FOOTPRINT::SetTransformScale", fp, scale_x, scale_y)
    }

    /// `FOOTPRINT::RescaleAroundPoint( aCenter, aSx, aSy )`.
    pub fn footprint_rescale_around_point(
        &mut self,
        fp: FootprintId,
        centre: Vector2I,
        sx: f64,
        sy: f64,
    ) {
        crate::port_pending!("FOOTPRINT::RescaleAroundPoint", fp, centre, sx, sy)
    }

    /// `FOOTPRINT::MoveAnchorPosition( aMoveVector )`: move the anchor, keep the children
    /// where they are on the board.
    pub fn footprint_move_anchor_position(&mut self, fp: FootprintId, delta: Vector2I) {
        crate::port_pending!("FOOTPRINT::MoveAnchorPosition", fp, delta)
    }

    /// `FOOTPRINT::SetLayerAndFlip( aLayer )`.
    pub fn footprint_set_layer_and_flip(&mut self, fp: FootprintId, layer: PcbLayerId) {
        crate::port_pending!("FOOTPRINT::SetLayerAndFlip", fp, layer)
    }

    /// `FOOTPRINT::Move( aMoveVector )`.
    pub fn footprint_move(&mut self, fp: FootprintId, delta: Vector2I) {
        crate::port_pending!("FOOTPRINT::Move", fp, delta)
    }

    /// `FOOTPRINT::Rotate( aRotCentre, aAngle )`.
    pub fn footprint_rotate(&mut self, fp: FootprintId, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("FOOTPRINT::Rotate", fp, centre, angle)
    }

    /// `FOOTPRINT::SetLayer( aLayer )` (the side, without flipping the children).
    pub fn footprint_set_layer(&mut self, fp: FootprintId, layer: PcbLayerId) {
        crate::port_pending!("FOOTPRINT::SetLayer", fp, layer)
    }
}
