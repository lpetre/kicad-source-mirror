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

//! Port of `pcbnew/drc/drc_test_provider_zone_connections.cpp`.
//!
//! This loads some rule resolvers for the ZONE_FILLER, and checks that pad thermal relief
//! connections have at least the required number of spokes.
//!
//! Errors generated:
//! - `DRCE_STARVED_THERMAL`
//!
//! The C++ tests the (zone, layer) pairs on the thread pool; the port tests them in the order
//! of the C++ list (`m_DRCCopperZones` order, then the zone's layers in `LSET` order), which
//! is the order of a single-threaded run (`drc.md` 4.12). The provider borrows the run's
//! connectivity (`GetConnectedTracks`, `GetConnectedItems`) and the cache generator's
//! `m_DRCCopperZones` / `m_ZoneIsolatedIslandsMap` ([`DrcTestProviderZoneConnections::new`]).

use std::sync::Arc;

use kicad_connectivity::data::{ConnectivityData, EXCLUDE_ZONES};
use kicad_core::Severity;
use kicad_core::base_units::ARC_LOW_DEF;
use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::Intersection;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::DRCE_STARVED_THERMAL;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::ids::{ItemId, ZoneId};
use kicad_pcb::padstack::PadShape;
use kicad_pcb::zone::IsolatedIslands;
use kicad_pcb::zone_settings::ZoneConnection;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::RunHost;
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_ZONE_CONNECTIONS`, with the run's connectivity and the cache
/// generator's caches borrowed (`'c`).
pub struct DrcTestProviderZoneConnections<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `board->GetConnectivity()`, rebuilt by the cache generator for this run. An empty
    /// `ConnectivityData` reproduces a board without connectivity (no connected items).
    connectivity: &'c mut ConnectivityData,
    /// The board caches of the cache generator (`m_DRCCopperZones`,
    /// `m_ZoneIsolatedIslandsMap`).
    caches: &'c DrcBoardCaches,
}

impl<'c> DrcTestProviderZoneConnections<'c> {
    /// `DRC_TEST_PROVIDER_ZONE_CONNECTIONS()` with the run's connectivity and board caches.
    pub fn new(connectivity: &'c mut ConnectivityData, caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderZoneConnections {
            base: DrcTestProviderBase::new("zone connections"),
            connectivity,
            caches,
        }
    }

    /// Port of `DRC_TEST_PROVIDER_ZONE_CONNECTIONS::Run`: false when the run was cancelled.
    ///
    /// The C++ submits one task per (zone, layer) to the thread pool and polls the progress
    /// every 250 ms; the port runs the tasks in list order (a cancelled run skips the remaining
    /// ones, as the C++ tasks do) and does not reproduce the timing-dependent progress calls.
    pub fn run(&mut self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking thermal reliefs...")
        {
            return false; // DRC cancelled
        }

        let mut zone_layers: Vec<(ZoneId, PcbLayerId)> = Vec::new();

        for &zone_id in &self.caches.copper_zones {
            let zone_ref = ctx
                .item(zone_id.into())
                .expect("m_DRCCopperZones holds board zones");
            let zone = zone_ref.as_zone().expect("a zone");

            if !zone.is_teardrop_area() {
                for layer in zone.layer_set(ctx).iter() {
                    zone_layers.push((zone_id, layer));
                }
            }
        }

        for &(zone_id, layer) in &zone_layers {
            if !host.is_cancelled() {
                self.test_zone_layer(engine, &mut host, ctx, zone_id, layer);
            }
        }

        !host.is_cancelled()
    }

    /// Port of `DRC_TEST_PROVIDER_ZONE_CONNECTIONS::testZoneLayer`.
    fn test_zone_layer(
        &mut self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) {
        let board = ctx.board;
        let zone_ref = ctx
            .item(zone_id.into())
            .expect("m_DRCCopperZones holds board zones");
        let zone = zone_ref.as_zone().expect("a zone");
        let zone_net = zone_ref.net_code().map_or(0, |n| n.0);

        let zone_fill = zone.filled_polys_list(layer);
        let mut isolated_islands = IsolatedIslands::default();

        if let Some(layers) = self.caches.zone_islands(zone_id)
            && let Some(islands) = layers.get(&layer)
        {
            isolated_islands = islands.clone();
        }

        for (fp_id, _) in board.footprints() {
            for (pad_id, pad) in board.footprint_pads(fp_id) {
                let pad_item_id: ItemId = pad_id.into();
                if host.is_error_limit_exceeded(DRCE_STARVED_THERMAL) {
                    return;
                }

                if host.is_cancelled() {
                    return;
                }

                let pad_ref = ctx.view(AnyItem::Pad(pad));
                let pad_net = pad_ref.net_code().map_or(0, |n| n.0);

                //
                // Quick tests for "connected":
                //

                if pad_net != zone_net || pad_net <= 0 {
                    continue;
                }

                let item_bbox = pad_ref.bounding_box();

                if !item_bbox.intersects(&zone_ref.bounding_box()) {
                    continue;
                }

                if !pad.flash_layer(ctx, layer) {
                    continue;
                }

                //
                // If those passed, do a thorough test:
                //

                let mut constraint =
                    engine.eval_zone_connection(ctx, pad_ref, Some(zone_ref), layer, None);
                let conn = constraint.zone_connection;

                if conn != ZoneConnection::Thermal {
                    continue;
                }

                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::MinResolvedSpokesConstraint,
                    Some(pad_ref),
                    Some(zone_ref),
                    layer,
                );
                let min_count = constraint.value().min();

                if constraint.severity() == Severity::Ignore || min_count <= 0 {
                    continue;
                }

                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::ThermalReliefGapConstraint,
                    Some(pad_ref),
                    Some(zone_ref),
                    layer,
                );
                let mid_gap = constraint.value().min() / 2;

                let mut pad_poly = ShapePolySet::new();
                pad_ref.transform_shape_to_polygon(
                    &mut pad_poly,
                    layer,
                    mid_gap,
                    ARC_LOW_DEF,
                    ErrorLoc::Outside,
                    false,
                );

                let pad_outline = pad_poly.c_outline(0);
                let pad_bbox = Shape::bbox(pad_outline, 0);
                let mut spokes: i32 = 0;
                let mut ignored_spokes: i32 = 0;
                let mut ignored_spoke_pos = Vector2I::default();

                for jj in 0..zone_fill.outline_count() {
                    let mut intersections: Vec<Intersection> = Vec::new();

                    zone_fill.c_outline(jj).intersect_chain(
                        pad_outline,
                        &mut intersections,
                        true,
                        Some(&pad_bbox),
                    );

                    let mut unique_intersections: Vec<Intersection> = Vec::new();

                    for i in &intersections {
                        let found = unique_intersections.iter().any(|j| j.p == i.p);

                        if !found {
                            unique_intersections.push(*i);
                        }
                    }

                    // If we connect to an island that only connects to a single item then we
                    // *are* that item.  Thermal spokes to this (otherwise isolated) island
                    // don't provide electrical connectivity to anything, so we don't count
                    // them.
                    if unique_intersections.len() >= 2 {
                        if isolated_islands.single_connection_outlines.contains(&jj) {
                            ignored_spokes += unique_intersections.len() as i32 / 2;
                            ignored_spoke_pos =
                                (unique_intersections[0].p + unique_intersections[1].p) / 2;
                        } else {
                            spokes += unique_intersections.len() as i32 / 2;
                        }
                    }
                }

                if spokes == 0 && ignored_spokes == 0 {
                    // Not connected at all
                    continue;
                }

                let mut custom_spokes = 0;

                if pad.shape(layer) == PadShape::Custom {
                    for primitive in pad.lib_padstack().primitives(layer) {
                        if primitive.is_proxy_item() && primitive.shape.shape() == ShapeT::Segment {
                            custom_spokes += 1;
                        }
                    }
                }

                let report = |host: &mut RunHost<'_, '_>, detail: String, pos: Vector2I| {
                    let mut drce =
                        item::create(DRCE_STARVED_THERMAL).expect("DRCE_STARVED_THERMAL item");
                    drce.set_error_detail(&detail);
                    drce.set_item_ids(vec![zone_ref.uuid(), pad_ref.uuid()]);
                    drce.violating_rule = constraint.parent_rule.clone();

                    self.base
                        .report_violation(host, Arc::new(drce), pos, layer.0, &no_path);
                };

                if custom_spokes > 0 {
                    if spokes < custom_spokes {
                        if ignored_spokes != 0 {
                            report(
                                host,
                                format!(
                                    "(layer {}; {} spokes connected to isolated island)",
                                    board.layer_name(layer),
                                    ignored_spokes
                                ),
                                ignored_spoke_pos,
                            );
                        } else {
                            report(
                                host,
                                format!(
                                    "(layer {}; {} custom spoke count {}; actual {})",
                                    board.layer_name(layer),
                                    constraint.name(),
                                    custom_spokes,
                                    spokes
                                ),
                                pad.position(ctx),
                            );
                        }
                    }

                    continue;
                }

                if spokes >= min_count {
                    // We already have enough
                    continue;
                }

                //
                // See if there are any other manual spokes added:
                //

                for track_id in self.connectivity.connected_tracks(pad_item_id) {
                    let track_ref = ctx.item(track_id).expect("a connected track");
                    let track = track_ref.as_track().expect("a track").as_track();

                    if pad_outline.point_inside(track.start(), 0, false) {
                        if zone
                            .filled_polys_list(layer)
                            .collide_point(track.end(), 0, None, None)
                        {
                            spokes += 1;
                        }
                    } else if pad_outline.point_inside(track.end(), 0, false)
                        && zone
                            .filled_polys_list(layer)
                            .collide_point(track.start(), 0, None, None)
                    {
                        spokes += 1;
                    }
                }

                let connected: Vec<ItemId> =
                    self.connectivity
                        .connected_items(&BoardCtx::bare(board), pad_item_id, EXCLUDE_ZONES);

                for item_id in connected {
                    let Some(shape_ref) = ctx.item(item_id) else {
                        continue;
                    };
                    let Some(shape) = shape_ref.as_shape() else {
                        continue;
                    };

                    if !shape_ref.is_on_layer(layer) {
                        continue;
                    }

                    let connection_pts = shape.connection_points(ctx);

                    for pt in &connection_pts {
                        if pad_outline.point_inside(*pt, 0, false) {
                            for other in &connection_pts {
                                if other != pt && zone_fill.collide_point(*other, 0, None, None) {
                                    spokes += 1;
                                    break;
                                }
                            }

                            break;
                        }
                    }
                }

                //
                // If we're *only* connected to isolated islands, then ignore the fact that
                // they're isolated.  (We leave that for the connectivity tester, which checks
                // connections on all layers.)
                //

                if spokes == 0 {
                    spokes += ignored_spokes;
                    ignored_spokes = 0;
                }

                //
                // And finally report it if there aren't enough:
                //

                if spokes < min_count {
                    if ignored_spokes != 0 {
                        report(
                            host,
                            format!(
                                "(layer {}; {} spokes connected to isolated island)",
                                board.layer_name(layer),
                                ignored_spokes
                            ),
                            ignored_spoke_pos,
                        );
                    } else {
                        report(
                            host,
                            format!(
                                "(layer {}; {} min spoke count {}; actual {})",
                                board.layer_name(layer),
                                constraint.name(),
                                min_count,
                                spokes
                            ),
                            pad.position(ctx),
                        );
                    }
                }
            }
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderZoneConnections<'_> {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(engine, run, ctx)
    }
}
