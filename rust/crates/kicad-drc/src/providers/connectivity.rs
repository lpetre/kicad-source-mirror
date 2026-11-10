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

//! Port of `pcbnew/drc/drc_test_provider_connectivity.cpp`.
//!
//! Connectivity test provider. Not rule-driven. Errors generated:
//! - `DRCE_DANGLING_TRACK`
//! - `DRCE_DANGLING_VIA`
//! - `DRCE_TRACK_ON_POST_MACHINED_LAYER`
//! - `DRCE_TRACK_NOT_CENTERED_ON_VIA`
//! - `DRCE_ISOLATED_COPPER`
//! - `DRCE_UNCONNECTED_ITEMS`
//!
//! The C++ reads the board's `CONNECTIVITY_DATA` (rebuilt by the cache generator at the start
//! of the run) and the cache generator's `m_ZoneIsolatedIslandsMap` / `m_CopperZoneRTreeCache`.
//! The board model keeps both outside the board (`board-model.md` 13), so the provider borrows
//! them for the run ([`DrcTestProviderConnectivity::new`]). The item shapes of the run are read
//! with that connectivity as the board's (`ctx.connectivity`), as C++ `GetBoard()->
//! GetConnectivity()` is the same object.
//!
//! Order notes (`drc.md` 4.10.3): the `ConnectedItems()` lists are in item-list order instead
//! of `CN_ITEM*` address order (kicad-connectivity deviation R-1), which decides the item named
//! by `track_on_post_machined_layer` / `track_not_centered_on_via` when a track touches several
//! candidates; the islands map is iterated in board zone order instead of `ZONE*` address
//! order (`drc.md` 4.12).

use std::sync::Arc;

use kicad_connectivity::data::{ConnectivityData, CopperZoneRtrees};
use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::{UNDEFINED_LAYER, is_copper_layer};
use kicad_core::{KicadT, PcbLayerId, Severity};
use kicad_geom::{Shape, ShapeCircle, Vector2I};
use kicad_pcb::drc_api::{
    DRCE_DANGLING_TRACK, DRCE_DANGLING_VIA, DRCE_ISOLATED_COPPER, DRCE_TRACK_NOT_CENTERED_ON_VIA,
    DRCE_TRACK_ON_POST_MACHINED_LAYER, DRCE_UNCONNECTED_ITEMS,
};
use kicad_pcb::ids::{ItemId, ZoneId};
use kicad_pcb::item::ItemFlags;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::RunHost;
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// `BOARD::GetCopperZoneRTree( zone )->QueryColliding( aShape.BBox(), &aShape, aLayer )` as
/// `CONNECTIVITY_DATA::TestTrackEndpointDangling` calls it, over the run's
/// `m_CopperZoneRTreeCache`.
pub struct DrcZoneRtrees<'c>(pub &'c DrcBoardCaches);

impl CopperZoneRtrees for DrcZoneRtrees<'_> {
    fn query_colliding(
        &self,
        zone: ZoneId,
        shape: &ShapeCircle,
        layer: PcbLayerId,
    ) -> Option<bool> {
        self.0
            .copper_zone_rtree(zone)
            .map(|rtree| rtree.query_colliding_box(&shape.bbox(0), shape, layer))
    }
}

/// Port of `DRC_TEST_PROVIDER_CONNECTIVITY`, with the run's connectivity and cache generator
/// caches borrowed (`'c`).
pub struct DrcTestProviderConnectivity<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `board->GetConnectivity()`, rebuilt by the cache generator for this run.
    connectivity: &'c mut ConnectivityData,
    /// The board caches of the cache generator (`m_ZoneIsolatedIslandsMap`,
    /// `m_CopperZoneRTreeCache`).
    caches: &'c DrcBoardCaches,
}

/// `IS_DELETED` set on the item.
fn is_deleted(item: &kicad_pcb::item_ref::ItemRef<'_>) -> bool {
    item.base().flags.0 & ItemFlags::IS_DELETED != 0
}

impl<'c> DrcTestProviderConnectivity<'c> {
    /// `DRC_TEST_PROVIDER_CONNECTIVITY()` with the run's connectivity and board caches.
    pub fn new(connectivity: &'c mut ConnectivityData, caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderConnectivity {
            base: DrcTestProviderBase::new("connectivity"),
            connectivity,
            caches,
        }
    }

    /// The first `CN_ITEM` of `ItemEntry( item ).GetItems()`, if any. (C++ `operator[]`
    /// creates an empty entry for an unknown item: no items.)
    fn first_cn_item(&self, item: ItemId) -> Option<kicad_connectivity::CnItemId> {
        self.connectivity
            .algo()
            .item_entry(item)
            .and_then(|entry| entry.items.first().copied())
    }

    /// Port of `DRC_TEST_PROVIDER_CONNECTIVITY::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking pad, via and zone connections...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;

        let progress_delta: usize = 250;
        let mut ii: usize = 0;
        let mut count: usize =
            board.tracks().count() + self.caches.zone_isolated_islands.iter().count();

        ii += count; // We gave half of this phase to CONNECTIVITY_DATA::Build()
        count += count;

        if !self.check_tracks(engine, &mut host, ctx, &mut ii, count, progress_delta) {
            return false; // DRC cancelled
        }

        /* test starved zones */
        for (zone_id, zone_islands) in self.caches.zone_isolated_islands.iter() {
            if host.is_error_limit_exceeded(DRCE_ISOLATED_COPPER) {
                break;
            }

            let progress = ii;
            ii += 1;

            if !self
                .base
                .report_progress(&mut host, progress, count, progress_delta)
            {
                return false; // DRC cancelled
            }

            let zone = board.zone(zone_id).expect("zone of the islands map");

            // Copper-thieving fills are netless dummy copper by definition.  Every
            // stamp is an "isolated island" but that is the intended geometry; flagging
            // would produce thousands of false violations on a real board.
            if zone.is_copper_thieving() {
                continue;
            }

            let zone_uuid = ctx.view(AnyItem::Zone(zone)).uuid();

            for (&layer, layer_islands) in zone_islands {
                if !is_copper_layer(layer) {
                    continue;
                }

                for &poly_idx in &layer_islands.isolated_outlines {
                    if host.is_error_limit_exceeded(DRCE_ISOLATED_COPPER) {
                        break;
                    }

                    let poly = zone.filled_polys_list(layer);

                    let mut drc_item =
                        item::create(DRCE_ISOLATED_COPPER).expect("DRCE_ISOLATED_COPPER item");
                    drc_item.set_item_ids(vec![zone_uuid]);
                    self.base.report_violation(
                        &mut host,
                        Arc::new(drc_item),
                        poly.c_outline(poly_idx).c_point(0),
                        layer.0,
                        &no_path,
                    );
                }
            }
        }

        if host.is_error_limit_exceeded(DRCE_UNCONNECTED_ITEMS) {
            return true; // continue with other tests
        }

        if !self
            .base
            .report_phase(&mut host, "Checking net connections...")
        {
            return false; // DRC cancelled
        }

        let mut ii: usize = 0;
        let count = self.connectivity.unconnected_count(false) as usize;
        let base = &self.base;

        self.connectivity.run_on_unconnected_edges(|edge, list| {
            if host.is_error_limit_exceeded(DRCE_UNCONNECTED_ITEMS) {
                return false;
            }

            let progress = ii;
            ii += 1;

            if !base.report_progress(&mut host, progress, count, progress_delta) {
                return false; // DRC cancelled
            }

            // wxCHECK( edge.GetSourceNode() && !edge.GetSourceNode()->Dirty(), true )
            let Some(source) = edge.source_anchor(list).filter(|n| !n.dirty(list)) else {
                debug_assert!(false, "unconnected edge without a valid source node");
                return true;
            };

            // wxCHECK( edge.GetTargetNode() && !edge.GetTargetNode()->Dirty(), true )
            let Some(target) = edge.target_anchor(list).filter(|n| !n.dirty(list)) else {
                debug_assert!(false, "unconnected edge without a valid target node");
                return true;
            };

            let uuid_of = |id: ItemId| ctx.item(id).map(|i| i.uuid()).unwrap_or_default();

            let mut drc_item =
                item::create(DRCE_UNCONNECTED_ITEMS).expect("DRCE_UNCONNECTED_ITEMS item");
            drc_item.set_item_ids(vec![
                uuid_of(source.parent(list)),
                uuid_of(target.parent(list)),
            ]);
            base.report_violation(
                &mut host,
                Arc::new(drc_item),
                source.pos(),
                UNDEFINED_LAYER.0,
                &no_path,
            );

            true
        });

        !host.is_cancelled()
    }

    /// The three track loops of `Run`: dangling tracks / vias, tracks connecting to
    /// post-machined or backdrilled layers, tracks not centered on a via. False when the run
    /// was cancelled.
    // The C++ `if / else if` chain of the dangling loop is kept as it is.
    #[allow(clippy::if_same_then_else)]
    fn check_tracks(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        ii: &mut usize,
        count: usize,
        progress_delta: usize,
    ) -> bool {
        // Item shapes ask the board's connectivity (FlashLayer): this one.
        let cctx = BoardCtx {
            board: ctx.board,
            rules: ctx.rules,
            connectivity: Some(&*self.connectivity),
        };
        let ctx = &cctx;
        let board = ctx.board;
        let zone_rtrees = DrcZoneRtrees(self.caches);
        let list = self.connectivity.algo().item_list();

        for (_, track) in board.tracks() {
            let exceed_t = host.is_error_limit_exceeded(DRCE_DANGLING_TRACK);
            let exceed_v = host.is_error_limit_exceeded(DRCE_DANGLING_VIA);
            let is_via = matches!(track, Track::Via(_));

            if exceed_v && exceed_t {
                break;
            } else if is_via && exceed_v {
                continue;
            } else if !is_via && exceed_t {
                continue;
            }

            let progress = *ii;
            *ii += 1;

            if !self
                .base
                .report_progress(host, progress, count, progress_delta)
            {
                return false; // DRC cancelled
            }

            // Test for dangling items
            let code = if is_via {
                DRCE_DANGLING_VIA
            } else {
                DRCE_DANGLING_TRACK
            };
            let track_ref = ctx.view(AnyItem::Track(track));
            let track_id = track_ref.id().expect("board track handle");

            let (dangling, pos) = self.connectivity.test_track_endpoint_dangling(
                ctx,
                track_id,
                true,
                Some(&zone_rtrees),
            );

            if dangling {
                let mut drc_item;

                if is_via {
                    let constraint = engine.eval_rules(
                        ctx,
                        DrcConstraintT::ViaDanglingConstraint,
                        Some(track_ref),
                        None,
                        track_ref.layer(),
                    );

                    if constraint.severity() == Severity::Ignore {
                        continue;
                    }

                    drc_item = item::create(code).expect("DRCE_DANGLING_VIA item");
                    drc_item.violating_rule = constraint.parent_rule.clone();
                } else {
                    drc_item = item::create(code).expect("DRCE_DANGLING_TRACK item");
                }

                drc_item.set_item_ids(vec![track_ref.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    pos.unwrap_or_default(),
                    track_ref.layer().0,
                    &no_path,
                );
            }
        }

        // Test for tracks connecting to post-machined or backdrilled layers
        if !host.is_error_limit_exceeded(DRCE_TRACK_ON_POST_MACHINED_LAYER) {
            for (_, track) in board.tracks() {
                if host.is_error_limit_exceeded(DRCE_TRACK_ON_POST_MACHINED_LAYER) {
                    break;
                }

                // Only check traces and arcs (not vias)
                if matches!(track, Track::Via(_)) {
                    continue;
                }

                let track_ref = ctx.view(AnyItem::Track(track));
                let t = track.as_track();
                let layer = track_ref.layer();

                // Get items connected to this track
                let Some(citem) = self.first_cn_item(track_ref.id().expect("board track handle"))
                else {
                    continue;
                };

                let citem = list.item(citem);

                if !citem.valid() {
                    continue;
                }

                for &connected in citem.connected_items() {
                    let Some(item) = ctx.item(list.item(connected).parent()) else {
                        continue;
                    };

                    if is_deleted(&item) {
                        continue;
                    }

                    let is_post_machined = if let Some(pad) = item.as_pad() {
                        pad.is_backdrilled_or_post_machined(ctx, layer)
                    } else if let Some(via) = item.as_via() {
                        via.is_backdrilled_or_post_machined(ctx, layer)
                    } else {
                        false
                    };

                    if is_post_machined {
                        let mut drc_item = item::create(DRCE_TRACK_ON_POST_MACHINED_LAYER)
                            .expect("DRCE_TRACK_ON_POST_MACHINED_LAYER item");
                        drc_item.set_item_ids(vec![track_ref.uuid(), item.uuid()]);

                        let mut pos: Vector2I = (t.start() + t.end()) / 2;

                        // Use the endpoint that's closer to the pad/via
                        if item.as_dyn().hit_test(ctx, t.start(), 0) {
                            pos = t.start();
                        } else if item.as_dyn().hit_test(ctx, t.end(), 0) {
                            pos = t.end();
                        }

                        self.base.report_violation(
                            host,
                            Arc::new(drc_item),
                            pos,
                            layer.0,
                            &no_path,
                        );
                        break; // Only report once per track
                    }
                }
            }
        }

        if !host.is_error_limit_exceeded(DRCE_TRACK_NOT_CENTERED_ON_VIA) {
            for (_, track) in board.tracks() {
                if host.is_error_limit_exceeded(DRCE_TRACK_NOT_CENTERED_ON_VIA) {
                    break;
                }

                if matches!(track, Track::Via(_)) {
                    continue;
                }

                let track_ref = ctx.view(AnyItem::Track(track));
                let t = track.as_track();

                let Some(citem) = self.first_cn_item(track_ref.id().expect("board track handle"))
                else {
                    continue;
                };

                let citem = list.item(citem);

                if !citem.valid() {
                    continue;
                }

                for &connected in citem.connected_items() {
                    let via_id = list.item(connected).parent();
                    let Some(item) = ctx.item(via_id) else {
                        continue;
                    };

                    if is_deleted(&item) {
                        continue;
                    }

                    let Some(via) = item.as_via() else {
                        continue;
                    };

                    let via_pos = item.position();

                    let start_in_via = via.hit_test(ctx, t.start(), 0);
                    let end_in_via = via.hit_test(ctx, t.end(), 0);

                    if !start_in_via && !end_in_via {
                        continue;
                    }

                    // Check if any track on the same layer connected to this VIA
                    // reaches its center. If so, this side is properly connected.
                    let mut layer_has_centered_track = false;
                    let track_layer = track_ref.layer();

                    if let Some(via_entry) = self.first_cn_item(via_id) {
                        for &via_connected in list.item(via_entry).connected_items() {
                            let Some(conn_item) = ctx.item(list.item(via_connected).parent())
                            else {
                                continue;
                            };

                            if conn_item.type_() != KicadT::PcbTraceT
                                && conn_item.type_() != KicadT::PcbArcT
                            {
                                continue;
                            }

                            let conn_track = conn_item.as_track().expect("track type").as_track();

                            if conn_item.layer() != track_layer {
                                continue;
                            }

                            if conn_track.start() == via_pos || conn_track.end() == via_pos {
                                layer_has_centered_track = true;
                                break;
                            }
                        }
                    }

                    if layer_has_centered_track {
                        continue;
                    }

                    if (start_in_via && t.start() != via_pos) || (end_in_via && t.end() != via_pos)
                    {
                        let start_violation = start_in_via && t.start() != via_pos;
                        let pos = if start_violation { t.start() } else { t.end() };

                        let mut drc_item = item::create(DRCE_TRACK_NOT_CENTERED_ON_VIA)
                            .expect("DRCE_TRACK_NOT_CENTERED_ON_VIA item");
                        drc_item.set_item_ids(vec![track_ref.uuid(), item.uuid()]);
                        self.base.report_violation(
                            host,
                            Arc::new(drc_item),
                            pos,
                            track_layer.0,
                            &no_path,
                        );
                        break;
                    }
                }
            }
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderConnectivity<'_> {
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
