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
//! `fillCopperZone`, `fillNonCopperZone`, `postKnockoutMinWidthPrune` and
//! `dropSubResolutionOutlines`.

use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::transform_circle_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::poly_ystripes_index::PolyYStripesIndex;
use kicad_geom::{Box2I, Shape, ShapeLineChain, ShapePolySet};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::{ItemId, PadId, ZoneId};
use kicad_pcb::pad::Pad;
use kicad_pcb::pcb_field::FieldT;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::ZoneFillMode;

use crate::connect::connect_nearby_polys;
use crate::filler::ZoneFiller;
use crate::index::board_and_footprint_zones;
use crate::knockouts::{append_zone_outline_without_arcs, is_zone_fill_keepout};

/// Port of the `DUMP_POLYS_TO_COPPER_LAYER( a, b, c )` macro of `zone_filler.cpp`: with the
/// zone filler debug mode on and `debug_layer == b`, the fill becomes a fractured copy of the
/// intermediate polygons `a` and the function returns false. (`m_board->SetLayerName( b, c )`
/// is not reproduced: the port does not rename board layers during a fill.)
macro_rules! dump_polys_to_copper_layer {
    ($self:ident, $a:expr, $b:expr, $c:literal, $debug_layer:ident, $fill:ident) => {
        if $self.debug_zone_filler && $debug_layer == $b {
            let _ = $c;
            let mut d = ($a).clone();
            d.fracture(true);
            *$fill = d;
            return false;
        }
    };
}

pub(crate) use dump_polys_to_copper_layer;

/// The result of [`ZoneFiller::fill_single_zone`] when it returns true in C++.
#[derive(Clone, Debug)]
pub struct SingleZoneFill {
    /// `aFillPolys`.
    pub polys: ShapePolySet,
    /// True when the C++ called `aZone->SetNeedRefill( false )` (the copper / non-copper fill
    /// succeeded); the caller applies it through `Board::zone_fill_mut`.
    pub clear_need_refill: bool,
}

/// `pcbIUScale.mmToIU( 0.001 )`: the min-width epsilon of the fill.
fn min_width_epsilon() -> i32 {
    PCB_IU_SCALE.mm_to_iu(0.001)
}

/// Port of the file-static `dropSubResolutionOutlines( aPolys, aMaxError )`: subtracting a
/// neighbouring zone's fill along an edge the two share sheds sub-micron contours that
/// survive Fracture() and are then counted as islands; drop the outlines whose area is below
/// `aMaxError^2`.
pub fn drop_sub_resolution_outlines(polys: &mut ShapePolySet, max_error: i32) {
    let noise_area = max_error as f64 * max_error as f64;

    for ii in (0..polys.outline_count()).rev() {
        if polys.c_outline(ii).area(true) < noise_area {
            polys.delete_polygon(ii);
        }
    }
}

/// The blob drop of `fillCopperZone` / `postKnockoutMinWidthPrune`: min-thickness is the web
/// thickness.  On the other hand, a blob min-thickness by min-thickness is not useful.  Since
/// there's no obvious definition of web vs. blob, we arbitrarily choose "at least 2X
/// min-thickness on one axis".  (Since we're doing this during the deflated state, that means
/// we test for "at least min-thickness".)
fn drop_blobs(fill_polys: &mut ShapePolySet, min_thickness: i32) {
    for ii in (0..fill_polys.outline_count()).rev() {
        let mut island_extents = Box2I::default();

        for &pt in fill_polys.c_polygon(ii)[0].c_points() {
            island_extents.merge_point(pt);

            if island_extents.size_max() > min_thickness as _ {
                break;
            }
        }

        if island_extents.size_max() < min_thickness as _ {
            fill_polys.delete_polygon(ii);
        }
    }
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::fillSingleZone( aZone, aLayer, aFillPolys )`: `None` where the C++
    /// returns false (a degenerate zone), else the fill.
    ///
    /// `BuildSmoothedPoly`'s first output is the max extents and its apron output the smoothed
    /// outline, as in C++. With the zone filler debug mode (`DebugZoneFiller`, see
    /// [`ZoneFiller::set_debug_zone_filler`]) an inner copper layer `In<k>.Cu` receives the
    /// intermediate polygons of stage `k` of the F.Cu fill instead of its own fill.
    pub fn fill_single_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) -> Option<SingleZoneFill> {
        self.fill_single_zone_impl(ctx, zone_id, layer)
            .map(|(fill, _)| fill)
    }

    /// [`ZoneFiller::fill_single_zone`] that also reports whether the fill may seed the refill
    /// result cache: true if `fillCopperZone` ran to its end and its tail after the
    /// pre-knockout cache insert applied no knockout (see `fill_copper_zone`).
    pub(crate) fn fill_single_zone_impl(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) -> Option<(SingleZoneFill, bool)> {
        let zone = ctx.board.zone(zone_id)?;
        let mut layer = layer;
        let board_outline = if self.brd_outlines_valid {
            Some(&self.board_outline)
        } else {
            None
        };
        let mut max_extents = ShapePolySet::new();
        let mut smoothed_poly = ShapePolySet::new();
        let mut debug_layer = PcbLayerId::UndefinedLayer;

        if self.debug_zone_filler && Lset::internal_cu_mask().contains(layer) {
            debug_layer = layer;
            layer = PcbLayerId::FCu;
        }

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
        let mut knockout_free = false;

        let ok = if self.trivial_copper_fill {
            self.trivial_fill(zone_id, layer, &smoothed_poly, &mut fill_polys)
        } else if zone.is_on_copper_layer(ctx) {
            self.fill_copper_zone(
                ctx,
                zone,
                zone_id,
                layer,
                debug_layer,
                &smoothed_poly,
                &max_extents,
                &mut fill_polys,
                &mut knockout_free,
            )
        } else {
            self.fill_non_copper_zone(ctx, zone, layer, &smoothed_poly, &mut fill_polys)
        };

        Some((
            SingleZoneFill {
                polys: fill_polys,
                clear_need_refill: ok,
            },
            knockout_free,
        ))
    }

    /// Port of `ZONE_FILLER::fillCopperZone( aZone, aLayer, aDebugLayer, aSmoothedOutline,
    /// aMaxExtents, aFillPolys )`. Also stores the `m_preKnockoutFillCache`,
    /// `m_sameNetApronCache` and `m_preHatchSolidFillCache` entries (keyed by
    /// `(zone_id, layer)`) that the iterative refill reads.
    ///
    /// Note that `smoothed_outline` is larger than the zone where it intersects with other,
    /// same-net zones.  This is to prevent the re-inflation post min-width trimming from
    /// creating divots between adjacent zones.  The final `max_extents` trimming will remove
    /// these areas from the final fill.
    ///
    /// `knockout_free` (port addition, for the refill result cache) is set to true when the
    /// function reaches its final `Fracture()` with the iterative refill on, the debug mode off
    /// and no knockout applied after the pre-knockout cache insert (no different-net zone
    /// clearance, no same-net higher-priority zone subtracted). The fill is then exactly
    /// `dropSubResolutionOutlines` + `Fracture()` of the cached pre-knockout fill.
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
        knockout_free: &mut bool,
    ) -> bool {
        *knockout_free = false;

        // Features which are min_width should survive pruning; features that are *less* than
        // min_width should not.  Therefore we subtract epsilon from the min_width when
        // deflating/inflating.
        let half_min_width = zone.min_thickness() / 2;
        let epsilon = min_width_epsilon();

        // Solid polygons are deflated and inflated during calculations.  Deflating doesn't
        // cause issues, but inflate is tricky as it can create excessively long and narrow
        // spikes for acute angles.
        // ALLOW_ACUTE_CORNERS cannot be used due to the spike problem.
        // CHAMFER_ACUTE_CORNERS is tempting, but can still produce spikes in some unusual
        // circumstances (https://gitlab.com/kicad/code/kicad/-/issues/5581).
        // It's unclear if ROUND_ACUTE_CORNERS would have the same issues, but is currently
        // avoided as a "less-safe" option.
        // ROUND_ALL_CORNERS produces the uniformly nicest shapes, but also a lot of segments.
        // CHAMFER_ALL_CORNERS improves the segment count.
        let fast_corner_strategy = CornerStrategy::ChamferAllCorners;
        let corner_strategy = CornerStrategy::RoundAllCorners;

        let mut thermal_connection_pads: Vec<ItemId> = Vec::new();
        let mut no_connection_pads: Vec<PadId> = Vec::new();
        let mut solid_connection_items: Vec<ItemId> = Vec::new();
        let mut thermal_spokes: Vec<ShapeLineChain> = Vec::new();
        let mut clearance_holes = ShapePolySet::new();

        *fill_polys = smoothed_outline.clone();
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In1Cu,
            "smoothed-outline",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Knockout thermal reliefs.

        self.knockout_thermal_reliefs(
            ctx,
            zone,
            zone_id,
            layer,
            fill_polys,
            &mut thermal_connection_pads,
            &mut no_connection_pads,
            &mut solid_connection_items,
        );
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In2Cu,
            "minus-thermal-reliefs",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // For hatch zones, add thermal rings around pads with thermal relief.
        // The rings are clipped to the zone boundary and provide the connection point
        // for the hatch webbing instead of connecting directly to the pad.

        let mut thermal_rings = ShapePolySet::new();

        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            self.build_hatch_zone_thermal_rings(
                ctx,
                zone,
                zone_id,
                layer,
                smoothed_outline,
                &thermal_connection_pads,
                fill_polys,
                &mut thermal_rings,
            );
            dump_polys_to_copper_layer!(
                self,
                *fill_polys,
                PcbLayerId::In2Cu,
                "plus-thermal-rings",
                debug_layer,
                fill_polys
            );
        }

        // ---------------------------------------------------------------------------------
        // Knockout electrical clearances.

        // When iterative refill is enabled, we build zone-to-zone clearances separately so we
        // can cache the fill before zone knockouts are applied (issue 21746).  Keepout zones
        // are always included in clearanceHoles regardless of the iterative refill setting so
        // they are subtracted before the deflate/inflate min-width cycle.  Subtracting
        // keepouts after that cycle and running a second deflate/inflate pass creates
        // artifacts along curved keepout boundaries (issue 23515).
        let iterative_refill = self.iterative_refill;

        self.build_copper_item_clearances(
            ctx,
            zone,
            zone_id,
            layer,
            &no_connection_pads,
            &mut clearance_holes,
            !iterative_refill, // include zone clearances only if not iterative
        );

        if iterative_refill && self.collect_keepout_holes(ctx, zone, layer, &mut clearance_holes) {
            clearance_holes.simplify();
        }

        dump_polys_to_copper_layer!(
            self,
            clearance_holes,
            PcbLayerId::In3Cu,
            "clearance-holes",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Add thermal relief spokes.

        self.build_thermal_spokes(
            ctx,
            zone,
            zone_id,
            layer,
            &thermal_connection_pads,
            &mut thermal_spokes,
        );

        // When iterative refill is enabled, zone-to-zone clearances are not included in
        // clearanceHoles (they're applied later to allow pre-knockout caching).  But we still
        // need to account for them when testing spoke endpoints, otherwise spokes will be kept
        // that point into areas that will be knocked out by higher-priority zones.
        let mut zone_clearances = ShapePolySet::new();

        if iterative_refill {
            self.build_different_net_zone_clearances(
                ctx,
                zone,
                zone_id,
                layer,
                &mut zone_clearances,
            );
        }

        let mut debug_spokes = ShapePolySet::new();

        // The spoke test area costs a clone, two booleans and a deflate/inflate cycle. Build
        // it only when a spoke exists. The debug filler still needs the intermediate dumps.
        if !thermal_spokes.is_empty() || self.debug_zone_filler {
            // Create a temporary zone that we can hit-test spoke-ends against.  It's only
            // temporary because the "real" subtract-clearance-holes has to be done after the
            // spokes are added.
            let mut test_areas = fill_polys.clone_drop_triangulation();
            test_areas.boolean_subtract(&clearance_holes);

            if zone_clearances.outline_count() > 0 {
                test_areas.boolean_subtract(&zone_clearances);
            }

            dump_polys_to_copper_layer!(
                self,
                test_areas,
                PcbLayerId::In4Cu,
                "minus-clearance-holes",
                debug_layer,
                fill_polys
            );

            // Prune features that don't meet minimum-width criteria
            if half_min_width - epsilon > epsilon {
                test_areas.deflate(
                    half_min_width - epsilon,
                    fast_corner_strategy,
                    self.max_error,
                );
                dump_polys_to_copper_layer!(
                    self,
                    test_areas,
                    PcbLayerId::In5Cu,
                    "spoke-test-deflated",
                    debug_layer,
                    fill_polys
                );

                test_areas.inflate(
                    half_min_width - epsilon,
                    fast_corner_strategy,
                    self.max_error,
                    false,
                );
                dump_polys_to_copper_layer!(
                    self,
                    test_areas,
                    PcbLayerId::In6Cu,
                    "spoke-test-reinflated",
                    debug_layer,
                    fill_polys
                );
            }

            // Build a Y-stripe spatial index for O(sqrt(V)) spoke endpoint containment queries
            // instead of O(V) brute-force ray-casting with bbox caches.
            let mut spoke_test_index = PolyYStripesIndex::new();
            spoke_test_index.build(&test_areas);

            for (si, spoke) in thermal_spokes.iter().enumerate() {
                let test_pt = spoke.c_point(3);

                // Hit-test against zone body
                if spoke_test_index.contains(test_pt, 1) {
                    if self.debug_zone_filler {
                        debug_spokes.add_outline(spoke);
                    }

                    fill_polys.add_outline(spoke);
                    continue;
                }

                // Hit-test against other spokes
                for (oi, other) in thermal_spokes.iter().enumerate() {
                    // Hit test in both directions to avoid interactions with round-off errors.
                    // (See https://gitlab.com/kicad/code/kicad/-/issues/13316.)
                    if oi != si
                        && other.point_inside(test_pt, 1, false)
                        && spoke.point_inside(other.c_point(3), 1, false)
                    {
                        if self.debug_zone_filler {
                            debug_spokes.add_outline(spoke);
                        }

                        fill_polys.add_outline(spoke);
                        break;
                    }
                }
            }
        }

        dump_polys_to_copper_layer!(
            self,
            debug_spokes,
            PcbLayerId::In7Cu,
            "spokes",
            debug_layer,
            fill_polys
        );

        fill_polys.boolean_subtract(&clearance_holes);
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In8Cu,
            "after-spoke-trimming",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Prune features that don't meet minimum-width criteria

        if half_min_width - epsilon > epsilon {
            fill_polys.deflate(
                half_min_width - epsilon,
                fast_corner_strategy,
                self.max_error,
            );

            // Also deflate thermal rings to match, for correct hatch hole notching
            if thermal_rings.outline_count() > 0 {
                thermal_rings.deflate(
                    half_min_width - epsilon,
                    fast_corner_strategy,
                    self.max_error,
                );
            }
        }

        drop_blobs(fill_polys, zone.min_thickness());

        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In9Cu,
            "deflated",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Process the hatch pattern (note that we do this while deflated)

        let prototype_zone_fill = ctx
            .board
            .project
            .as_ref()
            .is_some_and(|p| p.prototype_zone_fill);

        if zone.fill_mode() == ZoneFillMode::HatchPattern && !prototype_zone_fill {
            // Combine thermal rings with clearance holes (non-connected pad clearances) so that
            // the hatch hole-dropping logic considers both types of rings
            let mut rings_to_protect = thermal_rings.clone();
            rings_to_protect.boolean_add(&clearance_holes);

            // Drop the hatch hole around each fully connected via so it stays on the
            // webbing.  Feed only the hole-drop set, not the fill, so wider vias are left
            // untouched.
            for &item in &solid_connection_items {
                let Some(item_ref) = ctx.item(item) else {
                    continue;
                };

                if !item_ref.is_on_layer(layer) {
                    continue;
                }

                let Some(via) = item_ref.as_via() else {
                    continue;
                };

                let mut disc = ShapePolySet::new();
                transform_circle_to_polygon(
                    &mut disc,
                    via.position(ctx),
                    via.width(layer) / 2,
                    self.max_error,
                    ErrorLoc::Outside,
                    0,
                );
                disc.boolean_intersection(smoothed_outline);
                rings_to_protect.boolean_add(&disc);
            }

            // The refiller needs the un-hatched extent to re-border zones it later carves
            // (issue 24758).
            if self.iterative_refill {
                let mut solid = fill_polys.clone_drop_triangulation();

                if half_min_width - epsilon > epsilon {
                    solid.inflate(
                        half_min_width - epsilon,
                        corner_strategy,
                        self.max_error,
                        true,
                    );
                }

                solid.boolean_intersection(max_extents);
                solid.boolean_subtract(&clearance_holes);

                self.pre_hatch_solid_fill_cache
                    .insert((zone_id, layer), solid);
            }

            if !self.add_hatch_fill_type_on_zone(
                ctx,
                zone,
                layer,
                debug_layer,
                fill_polys,
                &rings_to_protect,
            ) {
                return false;
            }
        } else if zone.fill_mode() == ZoneFillMode::CopperThieving {
            if !self.add_copper_thieving_pattern(ctx, zone, layer, fill_polys) {
                return false;
            }
        } else {
            // -----------------------------------------------------------------------------
            // Connect nearby polygons with zero-width lines in order to ensure correct
            // re-inflation.
            fill_polys.fracture(true);
            connect_nearby_polys(fill_polys, zone.min_thickness() as f64);

            dump_polys_to_copper_layer!(
                self,
                *fill_polys,
                PcbLayerId::In10Cu,
                "connected-nearby-polys",
                debug_layer,
                fill_polys
            );
        }

        // ---------------------------------------------------------------------------------
        // Finish minimum-width pruning by re-inflating

        if half_min_width - epsilon > epsilon {
            fill_polys.inflate(
                half_min_width - epsilon,
                corner_strategy,
                self.max_error,
                true,
            );
        }

        // The deflation/inflation process can leave notches in the outline.  Remove these by
        // doing a union with the original ring
        fill_polys.boolean_add(&thermal_rings);

        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In15Cu,
            "after-reinflating",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Ensure additive changes (thermal stubs and inflating acute corners) do not add copper
        // outside the zone boundary, inside the clearance holes, or between otherwise isolated
        // islands

        for &item in &thermal_connection_pads {
            if let Some(pad) = item.typed::<Pad>().and_then(|id| ctx.board.pad(id)) {
                self.add_hole_knockout(ctx, pad, 0, &mut clearance_holes);
            }
        }

        fill_polys.boolean_intersection(max_extents);
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In16Cu,
            "after-trim-to-outline",
            debug_layer,
            fill_polys
        );
        fill_polys.boolean_subtract(&clearance_holes);
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In17Cu,
            "after-trim-to-clearance-holes",
            debug_layer,
            fill_polys
        );

        // Cache the pre-knockout fill for iterative refill optimization (issue 21746).
        // The cache stores the fill BEFORE zone-to-zone knockouts so the iterative refill can
        // reclaim space when higher-priority zones have islands removed.
        let mut knockouts_applied = false;
        let mut same_net_apron = ShapePolySet::new();

        if iterative_refill {
            // The band the aMaxExtents trim just took away but an abutting same-net zone still
            // pours into (issue 23790)
            same_net_apron = smoothed_outline.clone_drop_triangulation();
            same_net_apron.boolean_subtract(max_extents);
            same_net_apron.boolean_subtract(&clearance_holes);

            self.pre_knockout_fill_cache
                .insert((zone_id, layer), fill_polys.clone());
            self.same_net_apron_cache
                .insert((zone_id, layer), same_net_apron.clone());

            // Reuse the zone clearances already computed for spoke endpoint testing
            if zone_clearances.outline_count() > 0 {
                fill_polys.boolean_subtract(&zone_clearances);
                same_net_apron.boolean_subtract(&zone_clearances);
                knockouts_applied = true;
            }
        }

        // ---------------------------------------------------------------------------------
        // Re-prune minimum-width violations introduced by different-net zone knockouts.
        //
        // This must run BEFORE subtracting same-net higher-priority zones.  The fill no longer
        // reaches into overlapping same-net zone areas once trimmed to aMaxExtents, so
        // sameNetApron stands in for that overlap and keeps the deflate/inflate cycle from
        // carving divots at same-net zone boundaries (the same role aSmoothedOutline plays in
        // the initial pass).

        if knockouts_applied {
            self.post_knockout_min_width_prune(zone, fill_polys, &same_net_apron);
        }

        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In18Cu,
            "after-post-knockout-min-width",
            debug_layer,
            fill_polys
        );

        // ---------------------------------------------------------------------------------
        // Lastly give any same-net but higher-priority zones control over their own area.

        let same_net_subtracted = self.subtract_higher_priority_zones(ctx, zone, layer, fill_polys);
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In19Cu,
            "minus-higher-priority-zones",
            debug_layer,
            fill_polys
        );

        drop_sub_resolution_outlines(fill_polys, self.max_error);

        fill_polys.fracture(true);

        *knockout_free = iterative_refill
            && !knockouts_applied
            && !same_net_subtracted
            && !self.debug_zone_filler;
        true
    }

    /// Port of `ZONE_FILLER::fillNonCopperZone( aZone, aLayer, aSmoothedOutline, aFillPolys )`.
    pub(crate) fn fill_non_copper_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        smoothed_outline: &ShapePolySet,
        fill_polys: &mut ShapePolySet,
    ) -> bool {
        let board = ctx.board;
        let zone_boundingbox = zone.bounding_box(ctx);
        let mut clearance_holes = ShapePolySet::new();

        let mut knockout_graphic_item = |item: ItemId| {
            let Some(item_ref) = ctx.item(item) else {
                return;
            };

            if item_ref.as_dyn().is_knockout()
                && item_ref.is_on_layer(layer)
                && item_ref.bounding_box().intersects(&zone_boundingbox)
            {
                self.add_graphic_knockout(ctx, item, layer, 0, true, &mut clearance_holes);
            }
        };

        for (fp_id, footprint) in board.footprints() {
            // &footprint->Reference() / &footprint->Value()
            for field in [FieldT::Reference, FieldT::Value] {
                if let Some(id) = footprint.field(ctx, field) {
                    knockout_graphic_item(id.into());
                }
            }

            for (item, _) in board.footprint_drawings(fp_id) {
                knockout_graphic_item(item);
            }
        }

        for (item, _) in board.drawings() {
            knockout_graphic_item(item);
        }

        *fill_polys = smoothed_outline.clone();
        fill_polys.boolean_subtract(&clearance_holes);

        let mut keepout_holes = ShapePolySet::new();

        for keepout_id in board_and_footprint_zones(ctx) {
            let Some(candidate) = board.zone(keepout_id) else {
                continue;
            };

            if !is_zone_fill_keepout(ctx, candidate, layer, &zone_boundingbox) {
                continue;
            }

            append_zone_outline_without_arcs(ctx, candidate, &mut keepout_holes);
        }

        if keepout_holes.outline_count() > 0 {
            fill_polys.boolean_subtract(&keepout_holes);
        }

        // Features which are min_width should survive pruning; features that are *less* than
        // min_width should not.  Therefore we subtract epsilon from the min_width when
        // deflating/inflating.
        let half_min_width = zone.min_thickness() / 2;
        let epsilon = min_width_epsilon();

        fill_polys.deflate(
            half_min_width - epsilon,
            CornerStrategy::ChamferAllCorners,
            self.max_error,
        );

        // Remove the non filled areas due to the hatch pattern
        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            let no_thermal_rings = ShapePolySet::new(); // Non-copper zones have no thermal reliefs

            if !self.add_hatch_fill_type_on_zone(
                ctx,
                zone,
                layer,
                layer,
                fill_polys,
                &no_thermal_rings,
            ) {
                return false;
            }
        } else if zone.fill_mode() == ZoneFillMode::CopperThieving
            && !self.add_copper_thieving_pattern(ctx, zone, layer, fill_polys)
        {
            return false;
        }

        // Re-inflate after pruning of areas that don't meet minimum-width criteria
        if half_min_width - epsilon > epsilon {
            fill_polys.inflate(
                half_min_width - epsilon,
                CornerStrategy::RoundAllCorners,
                self.max_error,
                false,
            );
        }

        fill_polys.fracture(true);
        true
    }

    /// Port of `ZONE_FILLER::postKnockoutMinWidthPrune( aZone, aFillPolys, aSameNetApron )`:
    /// the min-width deflate / inflate cycle after the different-net zone knockouts, with the
    /// same-net apron standing in for abutting same-net zones.
    pub(crate) fn post_knockout_min_width_prune(
        &self,
        zone: &Zone,
        fill_polys: &mut ShapePolySet,
        same_net_apron: &ShapePolySet,
    ) {
        let half_min_width = zone.min_thickness() / 2;
        let epsilon = min_width_epsilon();

        if half_min_width - epsilon <= epsilon {
            return;
        }

        // Captured before the apron goes in so the closing intersection strips the apron back
        // off
        let pre_deflate = fill_polys.clone_drop_triangulation();

        if same_net_apron.outline_count() > 0 {
            // Overlap rather than abut the fill, or a rounding slit along the shared edge
            // opens into a notch
            let mut apron = same_net_apron.clone_drop_triangulation();
            apron.inflate(
                epsilon,
                CornerStrategy::RoundAllCorners,
                self.max_error,
                false,
            );
            fill_polys.boolean_add(&apron);
        }

        fill_polys.deflate(
            half_min_width - epsilon,
            CornerStrategy::ChamferAllCorners,
            self.max_error,
        );

        fill_polys.fracture(true);
        connect_nearby_polys(fill_polys, zone.min_thickness() as f64);

        drop_blobs(fill_polys, zone.min_thickness());

        fill_polys.inflate(
            half_min_width - epsilon,
            CornerStrategy::RoundAllCorners,
            self.max_error,
            true,
        );
        fill_polys.boolean_intersection(&pre_deflate);
    }

    /// Test stand-in for `fillCopperZone` / `fillNonCopperZone` (not a C++ function; enabled only by
    /// [`ZoneFiller::set_trivial_copper_fill`]): the fill is the smoothed outline, fractured,
    /// with no knockouts, and the pre-knockout cache holds it so that the iterative refill can
    /// run. It lets the driver be tested without the knockouts.
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
