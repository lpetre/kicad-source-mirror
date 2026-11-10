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

//! Port of `pcbnew/drc/drc_cache_generator.h` / `drc_cache_generator.cpp`
//! (`DRC_CACHE_GENERATOR::Run`).
//!
//! The C++ generator writes into `BOARD` members; the port returns a fresh
//! [`DrcBoardCaches`] (`rust/docs/analysis/drc.md` 3.8, `board-model.md` 17). It runs in three
//! borrow phases, in the C++ order wherever an earlier step can influence a later one:
//!
//! 1. `&mut Board` ([`prepare_board`]): `IncrementTimeStamp()`, then the derived-state writes of
//!    the C++ run (bounding-box caches of the copper zone outlines and fills, the triangulation
//!    of every zone, the footprint net-tie caches). None of them changes what the read phase
//!    computes (they touch zones and net-tie data only, which neither the clearance values nor
//!    the copper item tree read), so they are done first; the zone triangulation still precedes
//!    the zone trees and each bounding-box cache precedes its triangulation, as in C++.
//! 2. `&Board` with the engine as rule resolver: the clearance values, the zone lists, the
//!    copper item tree, the courtyard caches, the zone bounding-box caches and zone trees, the
//!    isolated-islands map, the board outline.
//! 3. The connectivity rebuild ([`CacheGeneratorConnectivity`]).
//!
//! The C++ thread-pool tasks (the copper tree in one task, one task per zone) run sequentially
//! in submission order (`drc.md` 8). The C++ `allZones` is a `std::set<ZONE*>` (address order)
//! used only to schedule independent per-zone work; the port visits board zones, then
//! footprint zones (`drc.md` 4.12: no effect on results).

use std::sync::Arc;

use kicad_core::layer_ids::{PcbLayerId, is_copper_layer};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::{Board, BoardCtx, ConnectivityQuery, ItemRef};

use crate::caches::{DrcBoardCaches, ZoneIslandsMap};
use crate::constraint_types::DrcConstraintT;
use crate::engine::DrcEngine;
use crate::rtree::DrcRtree;
use crate::test_provider::{DrcRunHost, DrcTestProviderBase, for_each_geometry_item};

/// The item types of the copper item tree (`itemTypes` of `DRC_CACHE_GENERATOR::Run`).
pub const COPPER_ITEM_TYPES: [KicadT; 12] = [
    KicadT::PcbTraceT,
    KicadT::PcbArcT,
    KicadT::PcbViaT,
    KicadT::PcbPadT,
    KicadT::PcbShapeT,
    KicadT::PcbFieldT,
    KicadT::PcbTextT,
    KicadT::PcbTextboxT,
    KicadT::PcbTableT,
    KicadT::PcbTablecellT,
    KicadT::PcbDimensionT,
    KicadT::PcbBarcodeT,
];

/// `INT_MAX / 3`: the upper bound of both clearance values ("algorithmic safety").
pub const MAX_CACHED_CLEARANCE: i32 = i32::MAX / 3;

/// The `CONNECTIVITY_DATA` calls of `DRC_CACHE_GENERATOR::Run` (`board->GetConnectivity()`).
/// Implemented by the connectivity peer of the board (`kicad_connectivity::ConnectivityData`
/// once it is ported); the board model keeps connectivity outside the board
/// (`board-model.md` 13).
pub trait CacheGeneratorConnectivity {
    /// The connectivity as it is before the rebuild (the C++ board's connectivity, built when
    /// the board was loaded): item shapes ask it whether a pad or via flashes on a layer.
    fn query(&self) -> Option<&dyn ConnectivityQuery>;

    /// `CONNECTIVITY_DATA::ClearRatsnest()`.
    fn clear_ratsnest(&mut self);

    /// `CONNECTIVITY_DATA::Build( aBoard, aReporter )`.
    fn build(&mut self, board: &Board) -> bool;

    /// `CONNECTIVITY_DATA::FillIsolatedIslandsMap( aMap, aConnectivityAlreadyRebuilt )`.
    fn fill_isolated_islands_map(
        &mut self,
        board: &Board,
        map: &mut ZoneIslandsMap,
        connectivity_already_rebuilt: bool,
    );
}

/// Port of `DRC_CACHE_GENERATOR` (a `DRC_TEST_PROVIDER` without a name).
#[derive(Clone, Debug, Default)]
pub struct DrcCacheGenerator {
    /// The provider base (`reportPhase`, `forEachGeometryItem`).
    pub base: DrcTestProviderBase,
}

/// The zones of the board in the order the C++ visits them: board zones, then the zones of
/// each footprint in footprint order. The flag is true for a footprint zone
/// (`GetParentFootprint() != nullptr`).
pub fn zones_in_drc_order(board: &Board) -> Vec<(ZoneId, bool)> {
    let mut zones: Vec<(ZoneId, bool)> = board.zones().map(|(id, _)| (id, false)).collect();

    for (fp, _) in board.footprints() {
        zones.extend(board.footprint_zones(fp).map(|(id, _)| (id, true)));
    }

    zones
}

/// `LSET::AllCuMask( m_board->GetCopperLayerCount() )`.
fn board_copper_layers(board: &Board) -> Lset {
    Lset::all_cu_mask_n(board.copper_layer_count())
}

fn layers_of(set: &Lset) -> Vec<PcbLayerId> {
    let mut out = Vec::new();
    set.run_on_layers(|layer| out.push(layer));
    out
}

/// Phase 1 of `DRC_CACHE_GENERATOR::Run` (the `&mut Board` part, see the module comment):
///
/// * `m_board->IncrementTimeStamp()`;
/// * `cacheBBoxes`: for each zone that is not a rule area and has a board copper layer, the
///   outline's bounding-box caches (board-level zones only) and those of its fills on the board
///   copper layers;
/// * per footprint `BuildNetTieCache()` (`BuildCourtyardCaches()` is a class-1 cache and runs
///   in the read phase, [`build_courtyard_caches`]);
/// * per zone `CacheTriangulation()` (`UNDEFINED_LAYER`: every fill and the outline).
pub fn prepare_board(board: &mut Board) {
    // Everything below appends to the board's DRC caches rather than replacing them, so they
    // have to start out empty.
    board.increment_time_stamp();

    let board_copper = board_copper_layers(board);
    let zones = zones_in_drc_order(board);

    // cacheBBoxes( zone, zoneCopperLayers ) for the copper zones of m_DRCZones
    let mut bbox_work: Vec<(ZoneId, bool, Vec<PcbLayerId>)> = Vec::new();

    {
        let ctx = BoardCtx::bare(board);

        for &(id, in_footprint) in &zones {
            let zone = ctx.board.zone(id).expect("zone handle of the board walk");

            if !zone.is_rule_area() {
                let zone_copper_layers = &zone.layer_set(&ctx) & &board_copper;

                if zone_copper_layers.any() {
                    bbox_work.push((id, in_footprint, layers_of(&zone_copper_layers)));
                }
            }
        }
    }

    for (id, in_footprint, layers) in bbox_work {
        if !in_footprint {
            board.modify_zone(id, |zone, env| zone.outline_mut(env).build_bbox_caches());
        }

        let fill = board.zone_fill_mut(id);

        for layer in layers {
            if let Some(poly) = fill.filled_polys_list.get_mut(&layer) {
                Arc::make_mut(poly).build_bbox_caches();
            }
        }
    }

    // footprint->BuildNetTieCache()
    for fp in board.footprint_ids() {
        board.build_net_tie_cache(fp);
    }

    // cache_zones: aZone->CacheTriangulation() (the bounding box is cached in the read phase,
    // after the last time stamp change)
    for &(id, _) in &zones {
        board
            .zone_fill_mut(id)
            .cache_triangulation(PcbLayerId::UndefinedLayer);
        board.modify_zone(id, |zone, _env| zone.cache_outline_triangulation());
    }
}

/// The clearance part of `DRC_CACHE_GENERATOR::Run`: (`m_DRCMaxClearance`,
/// `m_DRCMaxPhysicalClearance`), starting from the cleared values 0 and clamped to
/// `INT_MAX / 3`. `ctx.rules` must be the engine (`GetMaxClearanceValue` asks
/// `BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue`, which queries the board's engine).
pub fn compute_max_clearances(ctx: &BoardCtx<'_>, engine: &DrcEngine) -> (i32, i32) {
    let mut largest_clearance = 0;
    let mut largest_physical_clearance = 0;

    largest_clearance = largest_clearance.max(ctx.board.get_max_clearance_value(ctx));

    // Only consider unconditional constraints for the global maximum.  Conditional constraints
    // (like the barcode physical clearance default) apply only to specific item types and
    // should not inflate the R-tree query radius for all items on the board.
    if let Some(worst) =
        engine.query_worst_constraint(DrcConstraintT::PhysicalClearanceConstraint, true)
    {
        largest_physical_clearance = worst.value().min();
    }

    if let Some(worst) =
        engine.query_worst_constraint(DrcConstraintT::PhysicalHoleClearanceConstraint, true)
    {
        largest_physical_clearance = largest_physical_clearance.max(worst.value().min());
    }

    // If the unconditional max is 0, check for conditional constraints that may still apply.
    // User-defined conditional rules always need the test to run.  The implicit barcode rule
    // only needs the test if barcodes actually exist on the board.
    if largest_physical_clearance <= 0 {
        let mut conditional_max = 0;

        if let Some(worst) =
            engine.query_worst_constraint(DrcConstraintT::PhysicalClearanceConstraint, false)
        {
            conditional_max = worst.value().min();
        }

        if let Some(worst) =
            engine.query_worst_constraint(DrcConstraintT::PhysicalHoleClearanceConstraint, false)
        {
            conditional_max = conditional_max.max(worst.value().min());
        }

        if conditional_max > 0 {
            if engine.has_user_defined_physical_constraint() {
                largest_physical_clearance = conditional_max;
            } else {
                let mut has_matching_items = false;

                for_each_geometry_item(
                    ctx,
                    &[KicadT::PcbBarcodeT],
                    Lset::all_layers_mask(),
                    |_item| {
                        has_matching_items = true;
                        false
                    },
                );

                if has_matching_items {
                    largest_physical_clearance = conditional_max;
                }
            }
        }
    }

    // Ensure algorithmic safety
    largest_clearance = largest_clearance.min(MAX_CACHED_CLEARANCE);
    largest_physical_clearance = largest_physical_clearance.min(MAX_CACHED_CLEARANCE);

    (largest_clearance, largest_physical_clearance)
}

/// The zone-list part of `DRC_CACHE_GENERATOR::Run`: `m_DRCZones`, `m_DRCCopperZones` and
/// `m_DRCCopperZonesByLayer` (appended to `caches`, which starts empty).
pub fn build_zone_lists(ctx: &BoardCtx<'_>, caches: &mut DrcBoardCaches) {
    let board_copper = board_copper_layers(ctx.board);

    for (id, _) in zones_in_drc_order(ctx.board) {
        let zone = ctx.board.zone(id).expect("zone handle of the board walk");

        if !zone.is_rule_area() {
            caches.zones.push(id);

            let zone_copper_layers = &zone.layer_set(ctx) & &board_copper;

            if zone_copper_layers.any() {
                caches.copper_zones.push(id);
            }
        }
    }

    for &id in &caches.copper_zones {
        let zone = ctx.board.zone(id).expect("zone handle of the board walk");
        let zone_copper_layers = &zone.layer_set(ctx) & &board_copper;

        for layer in layers_of(&zone_copper_layers) {
            caches
                .copper_zones_by_layer
                .entry(layer)
                .or_default()
                .push(id);
        }
    }
}

/// The copper layers an item is inserted on (`addToCopperTree`): its board copper layers, all
/// of them for a pad with a hole.
pub fn copper_tree_layers(item: &ItemRef<'_>, board_copper: &Lset) -> Lset {
    let mut copper_layers = &item.layer_set() & board_copper;

    // Special-case pad holes which pierce all the copper layers
    if item.type_() == KicadT::PcbPadT && item.has_hole() {
        copper_layers = board_copper.clone();
    }

    copper_layers
}

/// The "Gathering copper items..." task of `DRC_CACHE_GENERATOR::Run`: a new `DRC_RTREE` with
/// every item of [`COPPER_ITEM_TYPES`] on each of its board copper layers
/// (`Insert( item, layer, CLEARANCE_CONSTRAINT, largestClearance )`), built.
pub fn build_copper_item_rtree(ctx: &BoardCtx<'_>, largest_clearance: i32) -> DrcRtree {
    let board_copper = board_copper_layers(ctx.board);

    // Always start from an empty tree: the traversal below re-inserts every copper item, and a
    // DRC_RTREE stops accepting inserts once it has been built.
    let mut tree = DrcRtree::new();

    for_each_geometry_item(ctx, &COPPER_ITEM_TYPES, &board_copper, |item| {
        copper_tree_layers(&item, &board_copper).run_on_layers(|layer| {
            tree.insert(
                item,
                layer,
                DrcConstraintT::ClearanceConstraint,
                largest_clearance,
                false,
            );
        });

        true
    });

    tree.build();
    tree
}

/// `footprint->BuildCourtyardCaches()` for every footprint ("Tessellating copper zones...").
pub fn build_courtyard_caches(ctx: &BoardCtx<'_>) {
    for (_, footprint) in ctx.board.footprints() {
        footprint.build_courtyard_caches(ctx, None);
    }
}

/// The read part of `cache_zones` for every zone: `CacheBoundingBox()`, and for a zone that is
/// not a rule area and is on a copper layer, its `DRC_RTREE` with the zone on each of its
/// copper layers (`Insert( aZone, layer, CLEARANCE_CONSTRAINT )`), stored in
/// `m_CopperZoneRTreeCache`. The triangulation was cached by [`prepare_board`].
pub fn build_zone_caches(ctx: &BoardCtx<'_>, caches: &mut DrcBoardCaches) {
    for (id, _) in zones_in_drc_order(ctx.board) {
        let zone = ctx.board.zone(id).expect("zone handle of the board walk");

        zone.cache_bounding_box(ctx);

        if !zone.is_rule_area() && zone.is_on_copper_layer(ctx) {
            let mut rtree = DrcRtree::new();
            let item = ctx.item(id.into()).expect("zone handle of the board walk");

            zone.layer_set(ctx).run_on_layers(|layer| {
                if is_copper_layer(layer) {
                    rtree.insert(item, layer, DrcConstraintT::ClearanceConstraint, 0, false);
                }
            });

            rtree.build();
            caches.copper_zone_rtrees.insert(id, rtree);
        }
    }
}

/// The `m_ZoneIsolatedIslandsMap` part of `DRC_CACHE_GENERATOR::Run`: cleared, then an empty
/// `ISOLATED_ISLANDS` per layer of every board zone (not footprint zones) that is not a rule
/// area, a teardrop or a copper-thieving zone.
pub fn init_zone_islands_map(ctx: &BoardCtx<'_>, map: &mut ZoneIslandsMap) {
    map.clear();

    for (id, zone) in ctx.board.zones() {
        if !zone.is_rule_area() && !zone.is_teardrop_area() && !zone.is_copper_thieving() {
            zone.layer_set(ctx).run_on_layers(|layer| {
                map.entry(id).insert(layer, Default::default());
            });
        }
    }
}

impl DrcCacheGenerator {
    /// `DRC_CACHE_GENERATOR()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `DRC_CACHE_GENERATOR::Run()`: discards the board's run-time DRC caches and
    /// regenerates them from scratch. Returns the caches, or `None` when the run was cancelled
    /// (`reportPhase` returned false).
    ///
    /// The C++ checks of `m_drcEngine->IsCancelled()` inside the item and zone loops and the
    /// `reportProgress` calls of the 250 ms wait loops are not reproduced: they depend on
    /// thread timing and only matter with an interactive progress reporter; cancellation is
    /// observed through `reportPhase`.
    pub fn run(
        &self,
        board: &mut Board,
        engine: &DrcEngine,
        connectivity: &mut dyn CacheGeneratorConnectivity,
        host: &mut dyn DrcRunHost,
    ) -> Option<DrcBoardCaches> {
        prepare_board(board);

        let mut caches = DrcBoardCaches::new();

        {
            let mut ctx = BoardCtx::bare(board).with_rules(engine);

            if let Some(query) = connectivity.query() {
                ctx = ctx.with_connectivity(query);
            }

            (caches.max_clearance, caches.max_physical_clearance) =
                compute_max_clearances(&ctx, engine);

            build_zone_lists(&ctx, &mut caches);

            if !self.base.report_phase(host, "Gathering copper items...") {
                return None; // DRC cancelled
            }

            caches.copper_item_rtree =
                Some(Arc::new(build_copper_item_rtree(&ctx, caches.max_clearance)));

            if !self.base.report_phase(host, "Tessellating copper zones...") {
                return None; // DRC cancelled
            }

            // Cache zone bounding boxes, triangulation, copper zone rtrees, and footprint
            // courtyards before we start.
            build_courtyard_caches(&ctx);
            build_zone_caches(&ctx, &mut caches);

            init_zone_islands_map(&ctx, &mut caches.zone_isolated_islands);

            // m_board->UpdateBoardOutline(); BoardOutline()->GetOutline().BuildBBoxCaches():
            // the board model rebuilds the outline lazily per time stamp
            // (`Board::board_outline`).
            let _ = ctx.board.board_outline(&ctx);
        }

        connectivity.clear_ratsnest();
        connectivity.build(board);
        connectivity.fill_isolated_islands_map(board, &mut caches.zone_isolated_islands, true);

        caches.time_stamp = board.time_stamp();
        Some(caches)
    }
}
