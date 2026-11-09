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

//! Port of the query part of `pcbnew/board.cpp` (`GetPad*`, `FindFootprintBy*`, bounding
//! boxes, max clearance, zone lists, groups sanity check) and `board_bounding_box.cpp`.
//! Package M22; M0d wrote the signatures (`board-model-types.md` 18).

use kicad_core::{Kiid, KiidPath, Lset};
use kicad_geom::{Box2I, Vector2I};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::{FootprintId, NetCode, PadId, TrackId, ZoneId};

impl Board {
    /// `BOARD::FindFootprintByReference`.
    pub fn find_footprint_by_reference(&self, reference: &str) -> Option<FootprintId> {
        crate::port_pending!("BOARD::FindFootprintByReference", reference)
    }

    /// `BOARD::FindFootprintByPath`.
    pub fn find_footprint_by_path(&self, path: &KiidPath) -> Option<FootprintId> {
        crate::port_pending!("BOARD::FindFootprintByPath", path)
    }

    /// `BOARD::FindPadByUuid` (`ResolveItem` restricted to pads).
    pub fn find_pad_by_uuid(&self, uuid: &Kiid) -> Option<PadId> {
        crate::port_pending!("BOARD::FindPadByUuid", uuid)
    }

    /// `BOARD::GetPad( aPosition, aLayerMask )`.
    pub fn get_pad_at(&self, ctx: &BoardCtx<'_>, pos: Vector2I, layers: Lset) -> Option<PadId> {
        crate::port_pending!("BOARD::GetPad", ctx.board.time_stamp(), pos, layers)
    }

    /// `BOARD::GetSortedPadListByXthenYCoord( aVector, aNetCode )`; `None` = every net.
    pub fn get_sorted_pad_list_by_x_then_y_coord(
        &self,
        ctx: &BoardCtx<'_>,
        net: Option<NetCode>,
    ) -> Vec<PadId> {
        crate::port_pending!(
            "BOARD::GetSortedPadListByXthenYCoord",
            ctx.board.time_stamp(),
            net
        )
    }

    /// `BOARD::GetZoneList( aIncludeZonesInFootprints )`.
    pub fn get_zone_list(&self, include_zones_in_footprints: bool) -> Vec<ZoneId> {
        crate::port_pending!("BOARD::GetZoneList", include_zones_in_footprints)
    }

    /// `BOARD::GetMaxClearanceValue` (cached per time stamp in `BoardCaches`).
    pub fn get_max_clearance_value(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("BOARD::GetMaxClearanceValue", ctx.board.time_stamp())
    }

    /// `BOARD::ComputeBoundingBox( aBoardEdgesOnly, aPhysicalLayersOnly )`.
    pub fn compute_bounding_box(
        &self,
        ctx: &BoardCtx<'_>,
        board_edges_only: bool,
        physical_layers_only: bool,
    ) -> Box2I {
        crate::port_pending!(
            "BOARD::ComputeBoundingBox",
            ctx.board.time_stamp(),
            board_edges_only,
            physical_layers_only
        )
    }

    /// `BOARD::GetBoardEdgesBoundingBox` = `ComputeBoundingBox( true, true )`.
    pub fn board_edges_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        self.compute_bounding_box(ctx, true, true)
    }

    /// `BOARD::GetTrackLength( aTrack )`: (count, track length, pad-to-die length, track delay,
    /// pad-to-die delay).
    pub fn get_track_length(
        &self,
        ctx: &BoardCtx<'_>,
        track: TrackId,
    ) -> (i32, f64, f64, f64, f64) {
        crate::port_pending!("BOARD::GetTrackLength", ctx.board.time_stamp(), track)
    }

    /// `BOARD::GroupsSanityCheck( repair )`: the error text (empty when sane).
    pub fn groups_sanity_check(&mut self, repair: bool) -> String {
        crate::port_pending!("BOARD::GroupsSanityCheck", repair)
    }
}
