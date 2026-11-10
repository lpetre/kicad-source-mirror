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

//! Port of `pcbnew/zone_filler.h` (the `ZONE_FILLER` state) and of the driver part of
//! `pcbnew/zone_filler.cpp`: the constructor, `mayHoldOutOfBoardCopper`,
//! `zoneKnockoutQueryBox`, `zoneKnockoutMayInteract` and `ZONE_FILLER::Fill` (flashing
//! pre-pass, fill DAG, island removal, out-of-board islands, second flashing pass). The
//! iterative refill is in [`crate::refill`].
//!
//! Sequential port of the thread pool (connectivity-zones.md section 8): the fill DAG is run
//! in Kahn order (roots in `toFill` order, successors released in discovery order); every fill
//! reads other zones' fills only through the `zoneKnockoutMayInteract` gate that created the
//! dependency edge (R-9), so the result does not depend on the order. Board writes follow the
//! read passes as in C++ (board-model.md 13): a pass that writes the item it reads (flashing
//! overrides) collects `(id, layer, value)` first and applies them afterwards.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use kicad_connectivity::algo::IsolatedIslandsMap;
use kicad_connectivity::data::ConnectivityData;
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::poly_ystripes_index::PolyYStripesIndex;
use kicad_geom::geometry::shape_utils::box_to_line_chain;
use kicad_geom::{Box2I, Shape, ShapePolySet, Vector2I};
use kicad_pcb::board::Board;
use kicad_pcb::ctx::{BoardCtx, RuleResolver};
use kicad_pcb::ids::{ItemId, ZoneId};
use kicad_pcb::padstack::UnconnectedLayerMode;
use kicad_pcb::pcb_track::{Track, ZoneLayerOverride};
use kicad_pcb::zone::{IsolatedIslands, Zone};
use kicad_pcb::zone_settings::{CornerSmoothing, IslandRemovalMode};

use crate::index::{ItemRtree, board_and_footprint_zones};

/// A `(zone, layer)` fill item: `std::pair<ZONE*, PCB_LAYER_ID>` with the zone handle in place
/// of the pointer (ordered containers keyed by it iterate in arena order instead of address
/// order, which connectivity-zones.md 4.0 shows does not reach the fills).
pub type FillKey = (ZoneId, PcbLayerId);

/// `ADVANCED_CFG::m_ExtraClearance` (mm), hard-coded default (no `kicad_advanced` support).
pub(crate) const EXTRA_CLEARANCE_MM: f64 = 0.0005;

/// `maxIterations` of the iterative refill.
pub(crate) const MAX_REFILL_ITERATIONS: i32 = 8;

/// Port of `ZONE_FILLER`. One filler may be reused for several fills; like C++, the
/// pre-knockout fill cache then survives between them (R-11).
pub struct ZoneFiller {
    /// `m_boardOutline`: the board outlines, if they exist.
    pub(crate) board_outline: ShapePolySet,
    /// `m_brdOutlinesValid`: true if `board_outline` is well-formed.
    pub(crate) brd_outlines_valid: bool,
    /// `m_graphicIndex`: footprint reference / value / graphics + board drawings.
    pub(crate) graphic_index: ItemRtree,
    /// `m_footprintIndex`: footprints, for the courtyard knockout.
    pub(crate) footprint_index: ItemRtree,
    /// `m_padIndex`.
    pub(crate) pad_index: ItemRtree,
    /// `m_trackIndex`.
    pub(crate) track_index: BTreeMap<PcbLayerId, ItemRtree>,
    /// `m_zoneIndex`.
    pub(crate) zone_index: BTreeMap<PcbLayerId, ItemRtree>,
    /// `m_maxError`.
    pub(crate) max_error: i32,
    /// `m_worstClearance`.
    pub(crate) worst_clearance: i32,
    /// `m_zoneKnockoutSlack`: ExtraClearance plus max approximation error, part of the
    /// knockout reach.
    pub(crate) zone_knockout_slack: i32,
    /// `m_maxZoneCornerRadius`: largest corner radius on the board. Widens the knockout query
    /// window.
    pub(crate) max_zone_corner_radius: i32,
    /// `m_debugZoneFiller` (`ADVANCED_CFG::m_DebugZoneFiller`, default false).
    pub(crate) debug_zone_filler: bool,
    /// `ADVANCED_CFG::m_ZoneFillIterativeRefill` (default true).
    pub(crate) iterative_refill: bool,
    /// `m_preKnockoutFillCache`: fill before the higher-priority zone knockout, per (zone,
    /// layer), for the iterative refill (issue 21746). Written by `fillCopperZone`.
    pub(crate) pre_knockout_fill_cache: BTreeMap<FillKey, ShapePolySet>,
    /// `m_preHatchSolidFillCache`: un-hatched extent per (zone, layer); lets the refiller
    /// re-border carved hatch zones (#24758).
    pub(crate) pre_hatch_solid_fill_cache: BTreeMap<FillKey, ShapePolySet>,
    /// `m_sameNetApronCache`: band just outside each (zone, layer) that an abutting same-net
    /// zone pours into (#23790).
    pub(crate) same_net_apron_cache: BTreeMap<FillKey, ShapePolySet>,
    /// Test stand-in for the pending copper fill (see `set_trivial_copper_fill`).
    pub(crate) trivial_copper_fill: bool,
    /// The `wxLogWarning` messages of the last fill.
    pub(crate) warnings: Vec<String>,
    /// The order in which the fill DAG ran the fill items of the last fill.
    pub(crate) fill_order: Vec<FillKey>,
}

/// A filled zone layer of the second flashing pass (`INDEXED_ZONE`).
struct IndexedZone {
    bbox: Box2I,
    index: PolyYStripesIndex,
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::ZONE_FILLER( aBoard, aCommit )` (the commit is not modelled: the
    /// port writes fills straight into the board, like the CLI commit does).
    pub fn new(board: &Board) -> Self {
        let max_error = board.design_settings().max_error();

        ZoneFiller {
            board_outline: ShapePolySet::new(),
            brd_outlines_valid: false,
            graphic_index: ItemRtree::new(),
            footprint_index: ItemRtree::new(),
            pad_index: ItemRtree::new(),
            track_index: BTreeMap::new(),
            zone_index: BTreeMap::new(),
            max_error,
            worst_clearance: 0,
            zone_knockout_slack: PCB_IU_SCALE.mm_to_iu(EXTRA_CLEARANCE_MM) + max_error,
            max_zone_corner_radius: 0,
            debug_zone_filler: false,
            iterative_refill: true,
            pre_knockout_fill_cache: BTreeMap::new(),
            pre_hatch_solid_fill_cache: BTreeMap::new(),
            same_net_apron_cache: BTreeMap::new(),
            trivial_copper_fill: false,
            warnings: Vec::new(),
            fill_order: Vec::new(),
        }
    }

    /// `ADVANCED_CFG::m_ZoneFillIterativeRefill` (the QA tests toggle it).
    pub fn set_iterative_refill(&mut self, enabled: bool) {
        self.iterative_refill = enabled;
    }

    /// Test hook, not a C++ API: replace the pending copper fill (`fillCopperZone`, package
    /// ZF-03) by the smoothed outline without knockouts, so that the driver can be tested.
    #[doc(hidden)]
    pub fn set_trivial_copper_fill(&mut self, enabled: bool) {
        self.trivial_copper_fill = enabled;
    }

    /// The `wxLogWarning` texts of the last [`ZoneFiller::fill`] (the iterative refill
    /// non-convergence warning).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The fill items in the order the (sequential) fill DAG of the last fill ran them.
    pub fn fill_order(&self) -> &[FillKey] {
        &self.fill_order
    }

    /// `m_worstClearance` of the last fill.
    pub fn worst_clearance(&self) -> i32 {
        self.worst_clearance
    }

    // ---- gates ------------------------------------------------------------------------------

    /// Port of `ZONE_FILLER::mayHoldOutOfBoardCopper( aZone )`: true if the fill of the zone
    /// can reach outside the board outline.
    pub(crate) fn may_hold_out_of_board_copper(&self, zone: &Zone) -> bool {
        if !self.brd_outlines_valid {
            return true;
        }

        // BuildSmoothedPoly() clips to the board outline and then smooths, and it closes
        // against the zone extents. Only a chamfer or a fillet can put copper back outside the
        // edge.
        if zone.is_teardrop_area() {
            return false;
        }

        zone.corner_smoothing_type == CornerSmoothing::Chamfer
            || zone.corner_smoothing_type == CornerSmoothing::Fillet
    }

    /// Port of `ZONE_FILLER::zoneKnockoutQueryBox( aZone )`: a window that holds every zone
    /// `zone_knockout_may_interact` can accept for `zone`.
    pub(crate) fn zone_knockout_query_box(&self, ctx: &BoardCtx<'_>, zone: &Zone) -> Box2I {
        // Dependency discovery requires this box to cover every indexed zone that can pass
        // zoneKnockoutMayInteract().  Bound the unknown candidate radius by the board maximum.
        let mut reach = self.worst_clearance + self.zone_knockout_slack + zone.min_thickness();

        if ctx.board.design_settings().zone_keep_external_fillets {
            reach += zone.corner_radius as i32 + self.max_zone_corner_radius;
        }

        let mut bbox = zone.bounding_box(ctx);
        bbox.inflate(reach);
        bbox
    }

    /// Port of `ZONE_FILLER::zoneKnockoutMayInteract( aZone, aKnockout )`: whether the fill of
    /// `knockout` can knock out any part of `zone`'s fill.
    ///
    /// Every read of another zone's fill must gate on this one predicate, or a read races the
    /// writer and the fill is non-deterministic.  Reach spans the knockout inflation and apron.
    pub(crate) fn zone_knockout_may_interact(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        knockout: &Zone,
    ) -> bool {
        let mut reach = self.worst_clearance + self.zone_knockout_slack + zone.min_thickness();

        if ctx.board.design_settings().zone_keep_external_fillets {
            for z in [zone, knockout] {
                if z.corner_smoothing_type == CornerSmoothing::Chamfer
                    || z.corner_smoothing_type == CornerSmoothing::Fillet
                {
                    reach += z.corner_radius as i32;
                }
            }
        }

        let mut bbox = zone.bounding_box(ctx);
        bbox.inflate(reach);

        if !bbox.intersects(&knockout.bounding_box(ctx)) {
            return false;
        }

        // SHAPE_POLY_SET::Collide( const SHAPE* ) caches the triangulation of the (local) zone
        // outline as a side effect.
        let mut zone_outline = zone.board_outline(ctx);
        let knockout_outline = knockout.board_outline(ctx);

        zone_outline.collide_shape_mut(&knockout_outline, reach, None, None)
    }

    // ---- Fill() -----------------------------------------------------------------------------

    /// Port of `ZONE_FILLER::Fill( aZones, aCheck = false )`: fills the given zones (handles
    /// of board or footprint zones) and writes the results into `board` (fills, island flags,
    /// fill flags, hashes, zone-layer overrides of pads and vias).
    ///
    /// `rules` is the board's DRC engine (`bds.m_DRCEngine`; the C++ creates one when the
    /// board has none, the port requires the caller to pass it). `connectivity` is rebuilt
    /// from scratch first, like the C++; the caller is responsible for rebuilding it again
    /// afterwards. `aCheck` (the GUI "fills out of date" dialogs) is not ported. Returns true
    /// (the C++ returns false only on cancellation or from the `aCheck` dialogs).
    pub fn fill(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: &dyn RuleResolver,
        zones: &[ZoneId],
    ) -> bool {
        // Keyed on knockout geometry only; valid for this fill's passes (pre-knockout fill is
        // rebuilt below). The refill result cache is a pure optimisation that the port omits.
        self.pre_hatch_solid_fill_cache.clear();
        self.same_net_apron_cache.clear();
        self.warnings.clear();
        self.fill_order.clear();

        let mut to_fill: Vec<FillKey> = Vec::new();
        let mut isolated_islands_map: IsolatedIslandsMap = BTreeMap::new();

        // Rebuild (from scratch, ignoring dirty flags) just in case. This really needs to be
        // reliable.
        connectivity.clear_ratsnest();
        connectivity.build_board(board);

        {
            let ctx = BoardCtx::bare(board).with_rules(rules);

            self.worst_clearance = board.get_max_clearance_value(&ctx);

            // The board outlines is used to clip solid areas inside the board (when outlines
            // are valid)
            let (outline, valid) = board.get_board_polygon_outlines(&ctx, true, None, false, false);
            self.board_outline = outline;
            self.brd_outlines_valid = valid;
        }

        // Update and cache zone bounding boxes and pad effective shapes so that we don't have
        // to make them thread-safe: pure caches in the port, except the net-tie cache, which
        // is derived state written through the board.
        for fp in board.footprint_ids() {
            // Rules may depend on insideCourtyard() or other expressions
            board.build_net_tie_cache(fp);
        }

        let (clears, overrides) = {
            let ctx = BoardCtx::bare(board)
                .with_rules(rules)
                .with_connectivity(&*connectivity);

            self.build_item_indexes(&ctx);
            self.flashing_pre_pass(&ctx)
        };

        for id in clears {
            board.clear_zone_layer_overrides(id);
        }

        for (id, layer, value) in overrides {
            board.set_zone_layer_override(id, layer, value);
        }

        for &zone_id in zones {
            let Some(zone) = board.zone(zone_id) else {
                continue;
            };

            // Rule areas are not filled
            if zone.is_rule_area() {
                continue;
            }

            // Degenerate zones will cause trouble; skip them
            if zone.num_corners() <= 2 {
                continue;
            }

            let layers: Vec<PcbLayerId> = zone.layer_set.iter().collect();
            let track_islands = !zone.is_copper_thieving() && !zone.is_teardrop_area();
            let fill = board.zone_fill_mut(zone_id);

            // calculate the hash value for filled areas. it will be used later to know if the
            // current filled areas are up to date
            for layer in layers {
                fill.build_hash_value(layer);

                // Add the zone to the list of zones to test or refill
                to_fill.push((zone_id, layer));

                // Copper-thieving fills are intentionally disconnected stamps; do not
                // track them through the isolated-islands pass or every stamp gets
                // classified as removable.  A teardrop sits on the track and pad it fillets,
                // so it is connected by construction and is ISLAND_REMOVAL_MODE::NEVER.
                if track_islands {
                    isolated_islands_map
                        .entry(zone_id)
                        .or_default()
                        .insert(layer, IsolatedIslands::default());
                }
            }

            // Remove existing fill first to prevent drawing invalid polygons on some platforms
            fill.unfill_locked();
        }

        self.run_fill_dag(board, connectivity, rules, &to_fill);

        // The islands map is what re-adds a zone to the connectivity graph, and teardrops are
        // no longer in it. Their fill is final here, so one pass keeps the graph correct.
        for &zone_id in zones {
            if board.zone(zone_id).is_some_and(|z| z.is_teardrop_area()) {
                connectivity.update(&BoardCtx::bare(board), zone_id.into());
            }
        }

        connectivity.fill_isolated_islands_map(
            &BoardCtx::bare(board),
            &mut isolated_islands_map,
            false,
        );

        for &zone_id in zones {
            // Keepout zones are not filled
            if board.zone(zone_id).is_none_or(|z| z.is_rule_area()) {
                continue;
            }

            board.zone_fill_mut(zone_id).set_is_filled(true);
        }

        // Now remove isolated copper islands according to the isolated islands strategy
        // assigned by the user (always, never, below-certain-size).
        //
        // Track zone-layer pairs that had islands removed for potential iterative refill.
        let mut zones_with_removed_island_layers: BTreeSet<FillKey> = BTreeSet::new();

        // Per-layer tracking: a zone-layer pair is "initially fully isolated" when every fill
        // outline on that layer was an island in the initial pass (i.e. the zone has no pad
        // connectivity on that layer).  Used in the iterative loop to distinguish legitimately
        // unconnected pours - which must be preserved - from zones that became fully isolated
        // only because other fills changed.
        let mut initially_fully_isolated_layers: BTreeSet<FillKey> = BTreeSet::new();

        for (&zone_id, zone_islands) in &isolated_islands_map {
            // Track per-layer isolation, and skip island removal on layers where every
            // outline is an island (unconnected pour - must be preserved as-is).
            let mut all_layers_fully_isolated = true;

            for (&layer, layer_islands) in zone_islands {
                let layer_fully_isolated = layer_islands.isolated_outlines.len()
                    == fill_outline_count(board, zone_id, layer) as usize;

                if layer_fully_isolated {
                    initially_fully_isolated_layers.insert((zone_id, layer));
                } else {
                    all_layers_fully_isolated = false;
                }
            }

            if all_layers_fully_isolated {
                continue;
            }

            for (&layer, layer_islands) in zone_islands {
                if self.debug_zone_filler && Lset::internal_cu_mask().contains(layer) {
                    continue;
                }

                if layer_islands.isolated_outlines.is_empty() {
                    continue;
                }

                if remove_islands(board, zone_id, layer, &layer_islands.isolated_outlines) {
                    zones_with_removed_island_layers.insert((zone_id, layer));
                }
            }
        }

        // Iterative refill: when islands are removed, overlapping zones may be able to reclaim
        // the freed space.  Repeat until fills stabilise (convergence), up to a safety limit.
        //
        // The initial fill subtracts a higher-priority same-net zone's outline, but
        // refillZoneFromCache() subtracts its actual fill; seed the refill with overlapping
        // lower zones so they reclaim any notch the higher zone left unfilled (issue 23790).
        let mut same_net_overlap_seeds: BTreeSet<FillKey> = BTreeSet::new();

        if self.iterative_refill {
            same_net_overlap_seeds = same_net_overlap_seeds_of(board, zones);
        }

        if self.iterative_refill
            && (!zones_with_removed_island_layers.is_empty() || !same_net_overlap_seeds.is_empty())
        {
            // Seed: island-removal changes plus same-net overlap reclaims (see above).
            let mut changed_zone_layers = zones_with_removed_island_layers;
            changed_zone_layers.extend(same_net_overlap_seeds);

            self.iterative_refill_loop(
                board,
                connectivity,
                rules,
                zones,
                changed_zone_layers,
                &initially_fully_isolated_layers,
            );
        }

        self.remove_out_of_board_islands(board, zones);

        // for( ZONE* zone : aZones ) zone->CalculateFilledArea(): m_area is GUI-only state
        // (search panel); see PORT_STATUS.md.

        self.flashing_second_pass(board, rules, connectivity);

        true
    }

    /// The conditional via and pad flashing pre-pass of `Fill()` (zone_filler.cpp L760-893):
    /// the items whose overrides are cleared (every via and every pad, in board order) and
    /// the `SetZoneLayerOverride` calls, in C++ order.
    ///
    /// This is now done completely deterministically prior to filling due to the pathological
    /// case presented in https://gitlab.com/kicad/code/kicad/-/issues/12964.
    #[allow(clippy::type_complexity)]
    fn flashing_pre_pass(
        &self,
        ctx: &BoardCtx<'_>,
    ) -> (Vec<ItemId>, Vec<(ItemId, PcbLayerId, ZoneLayerOverride)>) {
        let board = ctx.board;
        let board_cu_mask = Lset::all_cu_mask_n(board.copper_layer_count());

        // Pre-build Y-stripe spatial indices for zone outline containment queries.
        // Amortizes build cost across the thousands of via/pad flash checks below.
        let mut zone_outline_indices: HashMap<ZoneId, PolyYStripesIndex> = HashMap::new();

        for (zone_id, zone) in board.zones() {
            if zone.num_corners() <= 2 {
                continue;
            }

            let mut index = PolyYStripesIndex::new();
            index.build(&zone.board_outline(ctx));
            zone_outline_indices.insert(zone_id, index);
        }

        let is_in_pour_keepout_area = |bbox: &Box2I, item_layer: PcbLayerId, test_point| {
            for (zone_id, zone) in board.zones() {
                if !zone.is_rule_area() {
                    continue;
                }

                if !zone.has_keepout_parameters_set() {
                    continue;
                }

                if !zone.do_not_allow_zone_fills {
                    continue;
                }

                if !zone.layer_set.contains(item_layer) {
                    continue;
                }

                // Degenerate zones will cause trouble; skip them
                if zone.num_corners() <= 2 {
                    continue;
                }

                if !zone.bounding_box(ctx).intersects(bbox) {
                    continue;
                }

                if zone_outline_indices
                    .get(&zone_id)
                    .is_some_and(|index| index.contains(test_point, 0))
                {
                    return true;
                }
            }

            false
        };

        let mut clears = Vec::new();
        let mut overrides = Vec::new();

        // Determine state of conditional via flashing
        for (track_id, track) in board.tracks() {
            let Track::Via(via) = track else {
                continue;
            };

            let id: ItemId = track_id.into();
            let padstack = &via.padstack;

            clears.push(id);

            if !via.remove_unconnected() {
                continue;
            }

            let bbox = via.bounding_box(ctx);
            let center = via.position(ctx);
            let hole_radius = via.drill_value(ctx) / 2 + 1;
            let netcode = via.track.base.net.0;
            let mut layers = via.layer_set(ctx);
            layers &= &board_cu_mask;

            // Checking if the via hole touches the zone outline
            let via_test_fn = |_: ZoneId, zone: &Zone| {
                zone.board_outline(ctx)
                    .contains(center, -1, hole_radius, false)
            };

            for layer in layers.iter() {
                if !via.conditionally_flashed(layer) {
                    continue;
                }

                if is_in_pour_keepout_area(&bbox, layer, center) {
                    overrides.push((id, layer, ZoneLayerOverride::ForceNoZoneConnection));
                } else {
                    let zone = find_highest_priority_zone(ctx, &bbox, layer, netcode, &via_test_fn);

                    if zone.is_some_and(|z| {
                        z.base.net == via.track.base.net
                            && (padstack.unconnected_layer_mode()
                                != UnconnectedLayerMode::StartEndOnly
                                || layer == padstack.drill().start
                                || layer == padstack.drill().end)
                    }) {
                        overrides.push((id, layer, ZoneLayerOverride::ForceFlashed));
                    } else {
                        overrides.push((id, layer, ZoneLayerOverride::ForceNoZoneConnection));
                    }
                }
            }
        }

        // Determine state of conditional pad flashing
        for (fp_id, _) in board.footprints() {
            for (pad_id, pad) in board.footprint_pads(fp_id) {
                let id: ItemId = pad_id.into();

                clears.push(id);

                if !pad.remove_unconnected() {
                    continue;
                }

                let bbox = pad.bounding_box(ctx);
                let center = pad.position(ctx);
                let netcode = pad.base.net.0;
                let mut layers = pad.layer_set(ctx);
                layers &= &board_cu_mask;

                let pad_test_fn =
                    |zone_id: ZoneId, zone: &Zone| match zone_outline_indices.get(&zone_id) {
                        Some(index) => index.contains(center, 0),
                        None => zone.board_outline(ctx).contains(center, -1, 0, false),
                    };

                for layer in layers.iter() {
                    if !pad.conditionally_flashed(layer) {
                        continue;
                    }

                    if is_in_pour_keepout_area(&bbox, layer, center) {
                        overrides.push((id, layer, ZoneLayerOverride::ForceNoZoneConnection));
                    } else {
                        let zone =
                            find_highest_priority_zone(ctx, &bbox, layer, netcode, &pad_test_fn);

                        if zone.is_some_and(|z| z.base.net == pad.base.net) {
                            overrides.push((id, layer, ZoneLayerOverride::ForceFlashed));
                        } else {
                            overrides.push((id, layer, ZoneLayerOverride::ForceNoZoneConnection));
                        }
                    }
                }
            }
        }

        (clears, overrides)
    }

    /// The `zone_fill_dependency` lambda of `Fill()`: whether the fill of `zone` on `layer`
    /// has to wait for `other_zone` (a higher-priority zone whose filled areas it knocks out).
    fn zone_fill_dependency(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        other_zone: &Zone,
        require_completed_other_fill: bool,
    ) -> bool {
        // Check to see if we have to knock-out the filled areas of a higher-priority
        // zone.  If so we have to wait until said zone is filled before we can fill.

        // If the other zone is already filled on the requested layer then we're
        // good-to-go
        if require_completed_other_fill && other_zone.fill().fill_flag(layer) {
            return false;
        }

        // Even if keepouts exclude copper pours, the exclusion is by outline rather than
        // filled area, so we're good-to-go here too
        if other_zone.is_rule_area() {
            return false;
        }

        // If the other zone is never going to be filled then don't wait for it
        if other_zone.num_corners() <= 2 {
            return false;
        }

        // If the zones share no common layers
        if !other_zone.layer_set.test(layer) {
            return false;
        }

        if zone.higher_priority(other_zone) {
            return false;
        }

        // Same-net zones always use outlines to produce determinate results
        if other_zone.same_net(zone) {
            return false;
        }

        // Must be the same gate the knockout reads use, or the read races the writer.
        self.zone_knockout_may_interact(ctx, zone, other_zone)
    }

    /// The `fill_item_dependency` lambda of `Fill()` (via `check_fill_dependency`).
    fn fill_item_dependency(
        &self,
        ctx: &BoardCtx<'_>,
        waiter: FillKey,
        dependency: FillKey,
    ) -> bool {
        if waiter.0 == dependency.0 || waiter.1 != dependency.1 {
            return false;
        }

        let (Some(zone), Some(other)) = (ctx.board.zone(waiter.0), ctx.board.zone(dependency.0))
        else {
            return false;
        };

        self.zone_fill_dependency(ctx, zone, waiter.1, other, true)
    }

    /// The dependency discovery of `run_fill_waves` (`aAnyDependencies == true`): for every
    /// fill item the items that wait for it (in waiter order) and every item's in-degree.
    fn fill_dependencies(
        &self,
        ctx: &BoardCtx<'_>,
        fill_items: &[FillKey],
    ) -> (Vec<Vec<usize>>, Vec<i32>) {
        #[derive(Default)]
        struct LayerFillItems {
            indices: HashMap<ZoneId, Vec<usize>>,
            indexed: HashSet<ZoneId>,
            unindexed: Vec<usize>,
        }

        let count = fill_items.len();
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
        let mut in_degree: Vec<i32> = vec![0; count];

        // Lookups only: the hash containers never decide an order.
        let mut fill_items_by_layer: HashMap<PcbLayerId, LayerFillItems> = HashMap::new();

        // Fill() also accepts zones outside the index, including omitted layers.
        for (layer, index) in &self.zone_index {
            let indexed = &mut fill_items_by_layer.entry(*layer).or_default().indexed;

            for item in index.iter() {
                if let Some(zone) = item.item.typed::<Zone>() {
                    indexed.insert(zone);
                }
            }
        }

        for (i, &(zone, layer)) in fill_items.iter().enumerate() {
            let items = fill_items_by_layer.entry(layer).or_default();
            items.indices.entry(zone).or_default().push(i);

            if !items.indexed.contains(&zone) {
                items.unindexed.push(i);
            }
        }

        let mut last_seen: Vec<usize> = vec![count; count];

        // Only waiter order determines successor order; spatial hit order is irrelevant.
        for (j, &(zone_id, layer)) in fill_items.iter().enumerate() {
            let layer_items = &fill_items_by_layer[&layer];

            let mut add_dependency = |i: usize| {
                // Keep distinct fill entries even if a zone gains multiple index entries.
                if i == j || last_seen[i] == j {
                    return;
                }

                last_seen[i] = j;

                if self.fill_item_dependency(ctx, fill_items[j], fill_items[i]) {
                    successors[i].push(j);
                    in_degree[j] += 1;
                }
            };

            for &i in &layer_items.unindexed {
                add_dependency(i);
            }

            if let (Some(index), Some(zone)) =
                (self.zone_index.get(&layer), ctx.board.zone(zone_id))
            {
                let bbox = self.zone_knockout_query_box(ctx, zone);
                let min = [bbox.left(), bbox.top()];
                let max = [bbox.right(), bbox.bottom()];

                index.search(&min, &max, |hit| {
                    if let Some(items) = hit
                        .item
                        .typed::<Zone>()
                        .and_then(|z| layer_items.indices.get(&z))
                    {
                        for &i in items {
                            add_dependency(i);
                        }
                    }

                    true
                });
            }
        }

        (successors, in_degree)
    }

    /// `run_fill_waves( toFill, fill_lambda, tesselate_lambda, fill_item_dependency, true )`,
    /// sequential: Kahn order, roots in `toFill` order, successors released in discovery
    /// order before the item's own tessellation (as in C++).
    fn run_fill_dag(
        &mut self,
        board: &mut Board,
        connectivity: &ConnectivityData,
        rules: &dyn RuleResolver,
        fill_items: &[FillKey],
    ) {
        if fill_items.is_empty() {
            return;
        }

        let (successors, mut in_degree) = {
            let ctx = BoardCtx::bare(board).with_rules(rules);
            self.fill_dependencies(&ctx, fill_items)
        };

        let mut queue: VecDeque<usize> = (0..fill_items.len())
            .filter(|&i| in_degree[i] == 0)
            .collect();

        while let Some(idx) = queue.pop_front() {
            let (zone_id, layer) = fill_items[idx];

            // fill_lambda
            let result = {
                let ctx = BoardCtx::bare(board)
                    .with_rules(rules)
                    .with_connectivity(connectivity);
                self.fill_single_zone(&ctx, zone_id, layer)
            };

            let filled = result.is_some();

            if let Some(result) = result {
                let fill = board.zone_fill_mut(zone_id);

                if result.clear_need_refill {
                    fill.set_need_refill(false);
                }

                fill.move_filled_polys_list(layer, result.polys);
            }

            // Release dependents; their fills read this one's now-published result.
            for &succ in &successors[idx] {
                in_degree[succ] -= 1;

                if in_degree[succ] == 0 {
                    queue.push_back(succ);
                }
            }

            // tesselate_lambda
            if filled {
                let fill = board.zone_fill_mut(zone_id);
                fill.cache_triangulation(layer);
                fill.set_fill_flag(layer, true);
            }

            self.fill_order.push(fill_items[idx]);
        }
    }

    /// The out-of-board island removal of `Fill()` (zone_filler.cpp L1661-1790): islands
    /// which are either outside the board edge or fail to meet the minimum area requirements.
    fn remove_out_of_board_islands(&self, board: &mut Board, zones: &[ZoneId]) {
        let mut polys_to_check: Vec<(FillKey, f64)> = Vec::new();

        for &zone_id in zones {
            let Some(zone) = board.zone(zone_id) else {
                continue;
            };

            if !self.may_hold_out_of_board_copper(zone) {
                continue;
            }

            // Don't check for connections on layers that only exist in the zone but
            // were disabled in the board
            let mut zone_copper_layers = zone.layer_set.clone();
            zone_copper_layers &= &Lset::all_cu_mask_n(board.copper_layer_count());

            // Min-thickness is the web thickness.  On the other hand, a blob min-thickness by
            // min-thickness is not useful.  Since there's no obvious definition of web vs.
            // blob, we arbitrarily choose "at least 3X the area".
            let min_area = zone.min_thickness() as f64 * zone.min_thickness() as f64 * 3.0;

            for layer in zone_copper_layers.iter() {
                if self.debug_zone_filler && Lset::internal_cu_mask().contains(layer) {
                    continue;
                }

                polys_to_check.push(((zone_id, layer), min_area));
            }
        }

        // island_lambda over all of polys_to_check (one block in the sequential port)
        let mut actions: Vec<(FillKey, i32)> = Vec::new();

        for &(key, min_area) in &polys_to_check {
            // GetFilledPolysList( layer ) (std::map::at: every zone layer has an entry after
            // UnFill() unless the zone never had one; such a layer has nothing to check)
            let Some(poly) = board.zone(key.0).and_then(|z| z.get_fill(key.1)) else {
                continue;
            };

            for jj in (0..poly.outline_count()).rev() {
                let test_poly = &poly.c_polygon(jj)[0];
                let island_area = test_poly.area(true);

                if island_area < min_area {
                    continue;
                }

                // Clipping a fractured hatch plane against the outline takes minutes, so
                // settle the test from the bounding box whenever its area bounds suffice
                let bbox = test_poly.bbox(0);
                let mut bbox_in_board = ShapePolySet::new();

                bbox_in_board.boolean_intersection_sets(
                    &self.board_outline,
                    &ShapePolySet::from_outline(&box_to_line_chain(&bbox)),
                );

                let in_board_area = bbox_in_board.area();
                let bbox_area = bbox.width() as f64 * bbox.height() as f64;
                let out_of_board_area = bbox_area - in_board_area;

                if out_of_board_area < island_area / 2.0 {
                    continue;
                }

                if in_board_area < island_area / 2.0 {
                    actions.push((key, jj));
                    continue;
                }

                let mut island = ShapePolySet::new();
                let mut intersection = ShapePolySet::new();

                island.add_outline(test_poly);
                intersection.boolean_intersection_sets(&self.board_outline, &island);

                // Nominally, all of these areas should be either inside or outside the
                // board outline.  So this test should be able to just compare areas (if
                // they are equal, you are inside).  But in practice, we sometimes have
                // slight overlap at the edges, so testing against half-size area acts as
                // a fail-safe.
                if intersection.area() < island_area / 2.0 {
                    actions.push((key, jj));
                }
            }
        }

        for ((zone_id, layer), idx) in actions {
            if let Some(poly) = board
                .zone_fill_mut(zone_id)
                .filled_polys_list
                .get_mut(&layer)
            {
                Arc::make_mut(poly).delete_polygon_and_triangulation_data(idx, true);
            }
        }
    }

    /// The second flashing pass of `Fill()` (zone_filler.cpp L1795-1914): re-evaluate via and
    /// pad flashing based on the actual filled polygons.
    ///
    /// The first pass (before filling) marks vias as ZLO_FORCE_FLASHED if they're within the
    /// zone outline. However, if the fill doesn't actually reach the via (due to obstacles like
    /// tracks), we should not flash the via. See https://gitlab.com/kicad/code/kicad/-/issues/22010
    fn flashing_second_pass(
        &self,
        board: &mut Board,
        rules: &dyn RuleResolver,
        connectivity: &ConnectivityData,
    ) {
        let demotions: Vec<(ItemId, PcbLayerId)> = {
            let ctx = BoardCtx::bare(board)
                .with_rules(rules)
                .with_connectivity(connectivity);
            let board = ctx.board;
            let board_cu_mask = Lset::all_cu_mask_n(board.copper_layer_count());

            // Build a spatial index per filled zone-layer for O(log V) containment queries.
            // The per-key lists are in board zone order, as in C++; the map is only looked up.
            let mut filled_zones_by_net_layer: BTreeMap<(i32, PcbLayerId), Vec<IndexedZone>> =
                BTreeMap::new();

            for (_, zone) in board.zones() {
                if zone.is_rule_area() {
                    continue;
                }

                for layer in zone.layer_set.iter() {
                    let Some(fill) = zone.get_fill(layer) else {
                        continue;
                    };

                    if fill.is_empty() {
                        continue;
                    }

                    let mut index = PolyYStripesIndex::new();
                    index.build(fill);

                    filled_zones_by_net_layer
                        .entry((zone.base.net.0, layer))
                        .or_default()
                        .push(IndexedZone {
                            bbox: fill.bbox(0),
                            index,
                        });
                }
            }

            let zone_reaches_point =
                |netcode: i32, layer: PcbLayerId, center: Vector2I, radius: i32| -> bool {
                    let Some(list) = filled_zones_by_net_layer.get(&(netcode, layer)) else {
                        return false;
                    };

                    for iz in list {
                        if !iz.bbox.inflated(radius).contains(center) {
                            continue;
                        }

                        if iz.index.contains(center, radius) {
                            return true;
                        }
                    }

                    false
                };

            let mut demotions = Vec::new();

            for (track_id, track) in board.tracks() {
                let Track::Via(via) = track else {
                    continue;
                };

                let center = via.position(&ctx);
                let hole_radius = via.drill_value(&ctx) / 2;
                let netcode = via.track.base.net.0;
                let mut layers = via.layer_set(&ctx);
                layers &= &board_cu_mask;

                for layer in layers.iter() {
                    if via.zone_layer_override(layer) != ZoneLayerOverride::ForceFlashed {
                        continue;
                    }

                    let reach = hole_radius.max(via.width(layer) / 2);

                    if !zone_reaches_point(netcode, layer, center, reach) {
                        demotions.push((track_id.into(), layer));
                    }
                }
            }

            for (fp_id, _) in board.footprints() {
                for (pad_id, pad) in board.footprint_pads(fp_id) {
                    let center = pad.position(&ctx);
                    let netcode = pad.base.net.0;
                    let mut layers = pad.layer_set(&ctx);
                    layers &= &board_cu_mask;

                    let mut hole_radius = 0;

                    if pad.has_hole(&ctx) {
                        let drill = pad.drill_size(&ctx);
                        hole_radius = drill.x.min(drill.y) / 2;
                    }

                    for layer in layers.iter() {
                        if pad.zone_layer_override(layer) != ZoneLayerOverride::ForceFlashed {
                            continue;
                        }

                        // A thermal spoke reaches the pad copper edge. Testing only the hole
                        // radius lands on the spoke endpoint and rounds out for some hole
                        // sizes, dropping a connected pad's flashing (issue 24865). Use the
                        // pad copper radius, still inside the gap.
                        let pad_size = pad.size(&ctx, layer);
                        let reach = hole_radius.max(pad_size.x.min(pad_size.y) / 2);

                        if !zone_reaches_point(netcode, layer, center, reach) {
                            demotions.push((pad_id.into(), layer));
                        }
                    }
                }
            }

            demotions
        };

        for (id, layer) in demotions {
            board.set_zone_layer_override(id, layer, ZoneLayerOverride::ForceNoZoneConnection);
        }
    }
}

/// The `findHighestPriorityZone` lambda of `Fill()`: the highest-priority board zone on
/// `item_layer` whose bounding box meets `bbox` and that passes `test_fn`, preferring any
/// same-net zone over a higher-priority different-net zone.
///
/// A higher-priority different-net zone only knocks out same-net fill where it actually
/// fills; where it has no copper (e.g. behind a barrier track) the same-net zone keeps copper
/// around the item, so the item must still flash.
/// https://gitlab.com/kicad/code/kicad/-/issues/24175
fn find_highest_priority_zone<'a>(
    ctx: &BoardCtx<'a>,
    bbox: &Box2I,
    item_layer: PcbLayerId,
    netcode: i32,
    test_fn: &dyn Fn(ZoneId, &Zone) -> bool,
) -> Option<&'a Zone> {
    let mut highest_same_net_priority: u32 = 0;
    let mut highest_same_net_zone: Option<&'a Zone> = None;
    let mut highest_priority: u32 = 0;
    let mut highest_priority_zone: Option<&'a Zone> = None;

    for (zone_id, zone) in ctx.board.zones() {
        // Rule areas are not filled
        if zone.is_rule_area() {
            continue;
        }

        if !zone.layer_set.contains(item_layer) {
            continue;
        }

        let priority = zone.assigned_priority();
        let same_net = zone.base.net.0 == netcode;

        // Skip candidates that cannot improve either the same-net or the fall-back best.
        if same_net {
            if highest_same_net_zone.is_some() && priority < highest_same_net_priority {
                continue;
            }
        } else if highest_priority_zone.is_some() && priority < highest_priority {
            continue;
        }

        // Degenerate zones will cause trouble; skip them
        if zone.num_corners() <= 2 {
            continue;
        }

        if !zone.bounding_box(ctx).intersects(bbox) {
            continue;
        }

        if !test_fn(zone_id, zone) {
            continue;
        }

        if same_net && (highest_same_net_zone.is_none() || priority > highest_same_net_priority) {
            highest_same_net_priority = priority;
            highest_same_net_zone = Some(zone);
        }

        if highest_priority_zone.is_none() || priority > highest_priority {
            highest_priority = priority;
            highest_priority_zone = Some(zone);
        }
    }

    highest_same_net_zone.or(highest_priority_zone)
}

/// `zone->GetFilledPolysList( layer )->OutlineCount()` (0 for a layer without a fill entry,
/// where the C++ `std::map::at` would throw).
pub(crate) fn fill_outline_count(board: &Board, zone_id: ZoneId, layer: PcbLayerId) -> i32 {
    board
        .zone(zone_id)
        .and_then(|z| z.get_fill(layer))
        .map_or(0, |p| p.outline_count())
}

/// The island removal of one zone layer (`Fill()` L1286-1311 and the refill L1597-1617):
/// `islands` sorted descending (deleting from last to first keeps the remaining indices
/// valid); `ALWAYS` deletes, `AREA` deletes outlines smaller than the minimum island area, the
/// others are flagged with `SetIsIsland` (not renumbered after later deletions, R-10). Then
/// `UpdateTriangulationDataHash()`. Returns true if a polygon was deleted.
#[allow(clippy::if_same_then_else)] // keeps the C++ statement structure
pub(crate) fn remove_islands(
    board: &mut Board,
    zone_id: ZoneId,
    layer: PcbLayerId,
    isolated_outlines: &[i32],
) -> bool {
    let Some(zone) = board.zone(zone_id) else {
        return false;
    };

    let mut islands: Vec<i32> = isolated_outlines.to_vec();

    // The list of polygons to delete must be explored from last to first in list,
    // to allow deleting a polygon from list without breaking the remaining of the list
    islands.sort_by(|a, b| b.cmp(a));

    let min_area = zone.min_island_area;
    let mode = zone.island_removal_mode;
    let fill = board.zone_fill_mut(zone_id);
    let mut removed = false;
    let mut flagged: Vec<i32> = Vec::new();

    let Some(poly) = fill.filled_polys_list.get_mut(&layer) else {
        return false;
    };

    let poly = Arc::make_mut(poly);

    for idx in islands {
        if mode == IslandRemovalMode::Always {
            poly.delete_polygon_and_triangulation_data(idx, false);
            removed = true;
        } else if mode == IslandRemovalMode::Area
            && poly.c_outline(idx).area(true) < min_area as f64
        {
            poly.delete_polygon_and_triangulation_data(idx, false);
            removed = true;
        } else {
            flagged.push(idx);
        }
    }

    poly.update_triangulation_data_hash();

    for idx in flagged {
        fill.set_is_island(layer, idx);
    }

    // zone->CalculateFilledArea(): GUI-only state, see PORT_STATUS.md.
    removed
}

/// The `sameNetOverlapSeeds` of `Fill()` (L1327-1379): for each non-rule, non-teardrop zone
/// of `zones`, the shared copper layers with every board / footprint zone of the same net with
/// strictly higher assigned priority whose bounding box intersects, where both have a fill.
fn same_net_overlap_seeds_of(board: &Board, zones: &[ZoneId]) -> BTreeSet<FillKey> {
    let ctx = BoardCtx::bare(board);
    let board_cu = Lset::all_cu_mask_n(board.copper_layer_count());
    let mut seeds = BTreeSet::new();

    // Bucket by net so each lower zone scans only its own net.
    let mut zones_by_net: BTreeMap<i32, Vec<ZoneId>> = BTreeMap::new();

    for zone_id in board_and_footprint_zones(&ctx) {
        if let Some(zone) = board.zone(zone_id)
            && !zone.is_rule_area()
            && !zone.is_teardrop_area()
        {
            zones_by_net
                .entry(zone.base.net.0)
                .or_default()
                .push(zone_id);
        }
    }

    for &lower_id in zones {
        let Some(lower_zone) = board.zone(lower_id) else {
            continue;
        };

        if lower_zone.is_rule_area() || lower_zone.is_teardrop_area() {
            continue;
        }

        let Some(net_zones) = zones_by_net.get(&lower_zone.base.net.0) else {
            continue;
        };

        let mut lower_layers = lower_zone.layer_set.clone();
        lower_layers &= &board_cu;

        for &higher_id in net_zones {
            let Some(higher_zone) = board.zone(higher_id) else {
                continue;
            };

            if higher_id == lower_id
                || higher_zone.assigned_priority() <= lower_zone.assigned_priority()
            {
                continue;
            }

            if !lower_zone
                .bounding_box(&ctx)
                .intersects(&higher_zone.bounding_box(&ctx))
            {
                continue;
            }

            let mut shared_layers = lower_layers.clone();
            shared_layers &= &higher_zone.layer_set;

            for layer in shared_layers.iter() {
                // Without a higher-zone fill in the snapshot the lower zone would pour
                // through the higher zone's outline.
                if lower_zone.has_filled_polys_for_layer(layer)
                    && higher_zone.has_filled_polys_for_layer(layer)
                {
                    seeds.insert((lower_id, layer));
                }
            }
        }
    }

    seeds
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::ShapeLineChain;
    use kicad_pcb::io::{ParseOptions, parse_board};

    /// A board with a 100 x 100 mm Edge.Cuts rectangle and one GND zone on F.Cu with the
    /// given `fill` options.
    fn board(fill: &str) -> (Board, ZoneId) {
        let text = format!(
            r#"(kicad_pcb (version 20260206) (generator "pcbnew") (generator_version "10.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "GND")
  (gr_rect (start 0 0) (end 100 100) (layer "Edge.Cuts")
    (uuid "aaaaaaaa-0000-0000-0000-000000000001") (stroke (width 0.05) (type default)))
  (zone (net "GND") (layer "F.Cu") (uuid "bbbbbbbb-0000-0000-0000-000000000001")
    (hatch edge 0.5) (connect_pads (clearance 0.2)) (min_thickness 0.25)
    (fill yes {fill})
    (polygon (pts (xy 10 10) (xy 40 10) (xy 40 40) (xy 10 40)))))"#
        );
        let board = parse_board(&text, "t.kicad_pcb", &ParseOptions::default()).unwrap();
        let zone = board.zones().next().unwrap().0;
        (board, zone)
    }

    fn square(x: i32, y: i32, size: i32) -> ShapeLineChain {
        let mut chain = ShapeLineChain::new();
        chain.append_xy(x, y, false);
        chain.append_xy(x + size, y, false);
        chain.append_xy(x + size, y + size, false);
        chain.append_xy(x, y + size, false);
        chain.set_closed(true);
        chain
    }

    /// Three outlines of 1, 4 and 9 mm2 at indices 0, 1, 2.
    fn three_islands() -> ShapePolySet {
        let mm = 1_000_000;
        let mut poly = ShapePolySet::new();
        poly.add_outline(&square(20 * mm, 20 * mm, mm));
        poly.add_outline(&square(25 * mm, 20 * mm, 2 * mm));
        poly.add_outline(&square(30 * mm, 20 * mm, 3 * mm));
        poly
    }

    fn areas(board: &Board, zone: ZoneId) -> Vec<i64> {
        let poly = board.zone(zone).unwrap().filled_polys_list(PcbLayerId::FCu);
        (0..poly.outline_count())
            .map(|i| (poly.c_outline(i).area(true) / 1e12).round() as i64)
            .collect()
    }

    /// `ALWAYS` deletes every isolated outline.
    #[test]
    fn island_removal_always() {
        let (mut board, zone) = board("(island_removal_mode 0)");
        board
            .zone_fill_mut(zone)
            .set_filled_polys_list(PcbLayerId::FCu, &three_islands());

        assert!(remove_islands(&mut board, zone, PcbLayerId::FCu, &[0, 2]));
        assert_eq!(areas(&board, zone), vec![4]);
        assert!(!board.zone(zone).unwrap().is_island(PcbLayerId::FCu, 0));
    }

    /// `AREA` deletes isolated outlines below the limit and flags the others; the flags are
    /// not renumbered after the deletion of a lower index (R-10).
    #[test]
    fn island_removal_area_keeps_stale_flags() {
        let (mut board, zone) = board("(island_removal_mode 2) (island_area_min 5)");
        board
            .zone_fill_mut(zone)
            .set_filled_polys_list(PcbLayerId::FCu, &three_islands());

        assert!(remove_islands(&mut board, zone, PcbLayerId::FCu, &[0, 2]));
        assert_eq!(areas(&board, zone), vec![4, 9]);

        let z = board.zone(zone).unwrap();
        // Index 2 (9 mm2) was flagged before index 0 was deleted: the flag stays on 2.
        assert!(z.is_island(PcbLayerId::FCu, 2));
        assert!(!z.is_island(PcbLayerId::FCu, 1));
    }

    /// `NEVER` flags every isolated outline.
    #[test]
    fn island_removal_never() {
        let (mut board, zone) = board("(island_removal_mode 1)");
        board
            .zone_fill_mut(zone)
            .set_filled_polys_list(PcbLayerId::FCu, &three_islands());

        assert!(!remove_islands(&mut board, zone, PcbLayerId::FCu, &[0, 2]));
        assert_eq!(areas(&board, zone), vec![1, 4, 9]);
        let z = board.zone(zone).unwrap();
        assert!(z.is_island(PcbLayerId::FCu, 0) && z.is_island(PcbLayerId::FCu, 2));
    }

    /// Out-of-board islands: only zones with a chamfer / fillet; an outline mostly outside
    /// the board outline is deleted, one inside is kept, one below 3 * min_thickness^2 is not
    /// tested.
    #[test]
    fn out_of_board_islands() {
        let mm = 1_000_000;

        for (smoothing, expect) in [
            ("", vec![900, 100, 0]),
            ("(smoothing fillet) (radius 1)", vec![900, 0]),
        ] {
            let (mut board, zone) = board(smoothing);
            let mut poly = ShapePolySet::new();
            poly.add_outline(&square(10 * mm, 10 * mm, 30 * mm)); // inside
            poly.add_outline(&square(97 * mm, 50 * mm, 10 * mm)); // mostly outside
            poly.add_outline(&square(150 * mm, 50 * mm, mm / 10)); // tiny, outside
            board
                .zone_fill_mut(zone)
                .set_filled_polys_list(PcbLayerId::FCu, &poly);

            let mut filler = ZoneFiller::new(&board);
            let ctx = BoardCtx::bare(&board);
            let (outline, valid) = board.get_board_polygon_outlines(&ctx, true, None, false, false);
            assert!(valid);
            filler.board_outline = outline;
            filler.brd_outlines_valid = valid;

            filler.remove_out_of_board_islands(&mut board, &[zone]);
            assert_eq!(areas(&board, zone), expect, "{smoothing}");
        }
    }
}
