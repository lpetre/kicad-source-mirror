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

//! Port of the per-zone fill entry points of `pcbnew/zone_filler.cpp`: `fillSingleZone`,
//! `dropSubResolutionOutlines`, and the pending copper computation (`fillCopperZone`,
//! `fillNonCopperZone`, `postKnockoutMinWidthPrune`), which is package ZF-03.

use kicad_core::PcbLayerId;
use kicad_geom::ShapePolySet;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::zone::Zone;

use crate::filler::ZoneFiller;

/// The result of [`ZoneFiller::fill_single_zone`] when it returns true in C++.
#[derive(Clone, Debug)]
pub struct SingleZoneFill {
    /// `aFillPolys`.
    pub polys: ShapePolySet,
    /// True when the C++ called `aZone->SetNeedRefill( false )` (the copper / non-copper fill
    /// succeeded); the caller applies it through `Board::zone_fill_mut`.
    pub clear_need_refill: bool,
}

/// Port of the file-static `dropSubResolutionOutlines( aPolys, aMaxError )`: drop the outlines
/// whose area is below `aMaxError^2`.
pub fn drop_sub_resolution_outlines(polys: &mut ShapePolySet, max_error: i32) {
    let noise_area = max_error as f64 * max_error as f64;

    for ii in (0..polys.outline_count()).rev() {
        if polys.c_outline(ii).area(true) < noise_area {
            polys.delete_polygon(ii);
        }
    }
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::fillSingleZone( aZone, aLayer, aFillPolys )`: `None` where the C++
    /// returns false (a degenerate zone), else the fill.
    ///
    /// `BuildSmoothedPoly`'s first output is the max extents and its apron output the smoothed
    /// outline, as in C++. `m_debugZoneFiller` (the `DebugZoneFiller` advanced setting) is
    /// always false in the port, so the debug-layer redirection is not reproduced.
    pub fn fill_single_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) -> Option<SingleZoneFill> {
        let zone = ctx.board.zone(zone_id)?;
        let board_outline = if self.brd_outlines_valid {
            Some(&self.board_outline)
        } else {
            None
        };
        let mut max_extents = ShapePolySet::new();
        let mut smoothed_poly = ShapePolySet::new();
        let debug_layer = PcbLayerId::UndefinedLayer;

        if !zone.build_smoothed_poly(
            ctx,
            &mut max_extents,
            layer,
            board_outline,
            Some(&mut smoothed_poly),
        ) {
            return None;
        }

        let mut fill_polys = ShapePolySet::new();

        let ok = if zone.is_on_copper_layer(ctx) {
            if self.trivial_copper_fill {
                self.trivial_fill(zone_id, layer, &smoothed_poly, &mut fill_polys)
            } else {
                self.fill_copper_zone(
                    ctx,
                    zone,
                    zone_id,
                    layer,
                    debug_layer,
                    &smoothed_poly,
                    &max_extents,
                    &mut fill_polys,
                )
            }
        } else {
            self.fill_non_copper_zone(ctx, zone, layer, &smoothed_poly, &mut fill_polys)
        };

        Some(SingleZoneFill {
            polys: fill_polys,
            clear_need_refill: ok,
        })
    }

    /// Port of `ZONE_FILLER::fillCopperZone( aZone, aLayer, aDebugLayer, aSmoothedOutline,
    /// aMaxExtents, aFillPolys )`. Package ZF-03: it must also store the
    /// `m_preKnockoutFillCache`, `m_sameNetApronCache` and `m_preHatchSolidFillCache` entries
    /// (keyed by `(zone_id, layer)`) that the iterative refill reads.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_copper_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        debug_layer: PcbLayerId,
        smoothed_outline: &ShapePolySet,
        max_extents: &ShapePolySet,
        fill_polys: &mut ShapePolySet,
    ) -> bool {
        port_pending!(
            "ZONE_FILLER::fillCopperZone (ZF-03)",
            ctx,
            zone,
            zone_id,
            layer,
            debug_layer,
            smoothed_outline,
            max_extents,
            fill_polys
        )
    }

    /// Port of `ZONE_FILLER::fillNonCopperZone( candidate, aLayer, aSmoothedOutline,
    /// aFillPolys )`. Package ZF-03.
    pub(crate) fn fill_non_copper_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        smoothed_outline: &ShapePolySet,
        fill_polys: &mut ShapePolySet,
    ) -> bool {
        port_pending!(
            "ZONE_FILLER::fillNonCopperZone (ZF-03)",
            ctx,
            zone,
            layer,
            smoothed_outline,
            fill_polys
        )
    }

    /// Port of `ZONE_FILLER::postKnockoutMinWidthPrune( aZone, aFillPolys, aSameNetApron )`.
    /// Package ZF-03 (it needs `connect_nearby_polys`).
    pub(crate) fn post_knockout_min_width_prune(
        &self,
        zone: &Zone,
        fill_polys: &mut ShapePolySet,
        same_net_apron: &ShapePolySet,
    ) {
        port_pending!(
            "ZONE_FILLER::postKnockoutMinWidthPrune (ZF-03)",
            zone,
            fill_polys,
            same_net_apron
        )
    }

    /// Test stand-in for `fillCopperZone` (not a C++ function; enabled only by
    /// [`ZoneFiller::set_trivial_copper_fill`]): the fill is the smoothed outline, fractured,
    /// with no knockouts, and the pre-knockout cache holds it so that the iterative refill can
    /// run. It lets the driver be tested before package ZF-03 lands.
    fn trivial_fill(
        &mut self,
        zone_id: ZoneId,
        layer: PcbLayerId,
        smoothed_outline: &ShapePolySet,
        fill_polys: &mut ShapePolySet,
    ) -> bool {
        *fill_polys = smoothed_outline.clone_drop_triangulation();
        fill_polys.fracture(true);

        self.pre_knockout_fill_cache
            .insert((zone_id, layer), fill_polys.clone());
        self.same_net_apron_cache
            .insert((zone_id, layer), ShapePolySet::new());
        true
    }
}
