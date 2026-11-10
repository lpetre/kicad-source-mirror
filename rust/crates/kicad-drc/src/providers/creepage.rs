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

//! Port of `pcbnew/drc/drc_test_provider_creepage.cpp`.
//!
//! Physical creepage tests. Errors generated:
//! - `DRCE_CREEPAGE`
//!
//! The net pairs run sequentially in both C++ and port (the graph is shared and truncated
//! between pairs). Inside `CREEPAGE_GRAPH::GeneratePaths` the C++ may use the thread pool;
//! see `creepage_utils::solve` for why the reference runs it inline and for the
//! pointer-ordered places that make equally short paths a tie.

use std::collections::BTreeSet;
use std::sync::Arc;

use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::kiid::Kiid;
use kicad_core::layer_ids::PcbLayerId;
use kicad_pcb::drc_api::{DRCE_CREEPAGE, DrcConstraint};
use kicad_pcb::ids::NetCode;
use kicad_pcb::item::ParentRef;
use kicad_pcb::pcb_marker::PcbMarker;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::pcb_track::{PcbTrack, Track};
use kicad_pcb::{AnyItem, BoardCtx, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::constraint_types::DrcConstraintT;
use crate::creepage_engine::{CreepageEngine, CreepageResult};
use crate::creepage_utils::shapes::vi;
use crate::creepage_utils::{ConnId, CreepItem, CreepageGraph, build_creepage_board_edges};
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::DrcTestProviderBase;

/// Port of `DRC_TEST_PROVIDER_CREEPAGE`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderCreepage {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `ADVANCED_CFG::m_RealtimeCreepage` (default `false`; the port has no advanced
    /// configuration file): run the net pairs through [`CreepageEngine`].
    pub realtime_creepage: bool,
    /// `ADVANCED_CFG::m_EnableCreepageSlot` (default `false`): use the board's minimum groove
    /// width.
    pub enable_creepage_slot: bool,
    /// `m_reportedPairs`
    reported_pairs: BTreeSet<(CreepItem, CreepItem)>,
}

impl Default for DrcTestProviderCreepage {
    fn default() -> Self {
        DrcTestProviderCreepage {
            base: DrcTestProviderBase::new("creepage"),
            realtime_creepage: false,
            enable_creepage_slot: false,
            reported_pairs: BTreeSet::new(),
        }
    }
}

/// The `BOARD_ITEM*` of a creepage item.
fn creep_item_ref<'a>(
    ctx: &'a BoardCtx<'a>,
    owned: &'a [PcbShape],
    item: CreepItem,
) -> Option<ItemRef<'a>> {
    match item {
        CreepItem::Item(id) => ctx.item(id),
        CreepItem::OwnedEdge(i) => owned.get(i).map(|s| ctx.view(AnyItem::Shape(s))),
    }
}

/// `m_Uuid` of a creepage item (`DRC_ITEM::SetItems`).
fn creep_item_uuid(ctx: &BoardCtx<'_>, owned: &[PcbShape], item: CreepItem) -> Kiid {
    match item {
        CreepItem::Item(id) => ctx.item(id).map(|r| r.uuid()).unwrap_or_default(),
        CreepItem::OwnedEdge(i) => owned.get(i).map(|s| s.base.item.uuid).unwrap_or_default(),
    }
}

/// A geometry-less `PCB_TRACK( m_board )` placeholder with a net and a layer.
fn placeholder_track(net_code: i32, layer: PcbLayerId) -> Track {
    let mut track = PcbTrack::default();
    track.base.item.parent = ParentRef::Board;
    // SetNetCode( aNetCode ): the codes come from the board's net list, so the net exists
    track.base.net = NetCode(net_code);
    track.base.item.layer = layer;
    Track::Trace(track)
}

impl DrcTestProviderCreepage {
    /// Port of `DRC_TEST_PROVIDER_CREEPAGE::Run`: false when the run was cancelled.
    pub fn run(&mut self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        self.reported_pairs.clear();

        if !engine.has_rules_for_constraint_type(DrcConstraintT::CreepageConstraint) {
            report_aux(run, "No creepage constraints found. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !host.is_error_limit_exceeded(DRCE_CREEPAGE) {
            if !self.base.report_phase(&mut host, "Checking creepage...") {
                return false; // DRC cancelled
            }

            self.test_creepage_all(engine, &mut host, ctx);
        }

        !host.is_cancelled()
    }

    /// Port of `placeholderConstraint()`: the constraint from geometry-less net / layer
    /// placeholders; exact only when no rule is conditional.
    pub fn placeholder_constraint(
        engine: &DrcEngine,
        ctx: &BoardCtx<'_>,
        a_net_code_a: i32,
        a_net_code_b: i32,
        a_layer: PcbLayerId,
    ) -> DrcConstraint {
        let bci1 = placeholder_track(a_net_code_a, a_layer);
        let bci2 = placeholder_track(a_net_code_b, a_layer);
        let c: &BoardCtx<'_> = ctx;

        engine.eval_rules(
            c,
            DrcConstraintT::CreepageConstraint,
            Some(c.view(AnyItem::Track(&bci1))),
            Some(c.view(AnyItem::Track(&bci2))),
            a_layer,
        )
    }

    /// Port of `testCreepage( CREEPAGE_GRAPH&, aNetCodeA, aNetCodeB, aLayer, aMaxCreepage,
    /// aHasConditional )`: 1 when the graph was changed (net elements added), else 0.
    #[allow(clippy::too_many_arguments)]
    fn test_creepage<'a>(
        &mut self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        a_graph: &mut CreepageGraph<'a>,
        a_net_code_a: i32,
        a_net_code_b: i32,
        a_layer: PcbLayerId,
        a_max_creepage: f64,
        a_has_conditional: bool,
    ) -> i32 {
        let ctx = a_graph.ctx;

        // Placeholders at the origin never satisfy intersectsArea(), so conditional rules
        // need a worst-case target and a per-path resolve below. Path cost scales with
        // target, hence the split
        let mut net_constraint = DrcConstraint::default();
        let target: f64;

        if a_has_conditional {
            target = a_max_creepage;
        } else {
            net_constraint =
                Self::placeholder_constraint(engine, ctx, a_net_code_a, a_net_code_b, a_layer);
            target = net_constraint.value().min() as f64;
        }

        a_graph.set_target(target);

        if target <= 0.0 {
            return 0;
        }

        // Let's make a quick "clearance test"
        let (Some(net_a), Some(net_b)) = (
            ctx.board.find_net(NetCode(a_net_code_a)),
            ctx.board.find_net(NetCode(a_net_code_b)),
        ) else {
            return 0;
        };

        if net_a
            .bounding_box(ctx)
            .distance_box(&net_b.bounding_box(ctx)) as f64
            > target
        {
            return 0;
        }

        // AddNetElements( int, PCB_LAYER_ID, int aMaxCreepage ): the double converts to int
        let max_creepage = kicad_geom::cmath::f64_to_i32_x86(target);
        let net_a_node = a_graph.add_net_elements(a_net_code_a, a_layer, max_creepage);
        let net_b_node = a_graph.add_net_elements(a_net_code_b, a_layer, max_creepage);

        a_graph.generate_paths(target, a_layer, None);

        CreepageEngine::connect_children(a_graph);

        let mut shortest_path: Vec<ConnId> = Vec::new();
        let distance = a_graph.solve(Some(net_a_node), Some(net_b_node), &mut shortest_path);

        if shortest_path.is_empty() || shortest_path.len() < 4 {
            return 1;
        }

        let gc1 = a_graph.connections[shortest_path[1].0].clone();
        let gc2 = a_graph.connections[shortest_path[shortest_path.len() - 2].0].clone();

        let item1 = a_graph.nodes[gc1.n1.0]
            .parent
            .and_then(|p| a_graph.shape(p).parent());
        let item2 = a_graph.nodes[gc2.n2.0]
            .parent
            .and_then(|p| a_graph.shape(p).parent());

        let owned: &[PcbShape] = &a_graph.owned_board_edges;

        let constraint: DrcConstraint = if !a_has_conditional {
            net_constraint
        } else if let (Some(i1), Some(i2)) = (item1, item2) {
            engine.eval_rules(
                ctx,
                DrcConstraintT::CreepageConstraint,
                creep_item_ref(ctx, owned, i1),
                creep_item_ref(ctx, owned, i2),
                a_layer,
            )
        } else {
            Self::placeholder_constraint(engine, ctx, a_net_code_a, a_net_code_b, a_layer)
        };

        let creepage_value = constraint.value().min() as f64;

        if creepage_value <= 0.0 || distance - creepage_value >= 0.0 {
            return 1;
        }

        let mut drc_item = item::create(DRCE_CREEPAGE).expect("DRCE_CREEPAGE item");
        drc_item.set_error_detail(&self.base.format_msg(
            "(%s creepage %s; actual %s)",
            &constraint.name(),
            creepage_value,
            distance,
            EdaDataType::Distance,
        ));
        drc_item.violating_rule = constraint.parent_rule.clone();

        if let (Some(i1), Some(i2)) = (item1, item2) {
            if self.reported_pairs.insert((i1, i2)) {
                drc_item.set_item_ids(vec![
                    creep_item_uuid(ctx, owned, i1),
                    creep_item_uuid(ctx, owned, i2),
                ]);
            } else {
                return 1;
            }
        }

        let start_point = vi(gc1.path.a2);
        let end_point = vi(gc2.path.a2);
        let mut path: Vec<PcbShape> = Vec::new();

        for &gc in &shortest_path {
            a_graph.connection_shapes(gc, &mut path);
        }

        self.base.report_violation(
            host,
            Arc::new(drc_item),
            vi(gc1.path.a2),
            a_layer.0,
            &|marker: &mut PcbMarker| {
                marker.set_path(path.clone(), start_point, end_point);
            },
        );

        1
    }

    /// Port of `GetMaxConstraint()`: an upper bound to size the graph only; the governing
    /// rule is resolved per path in `testCreepage`. Per-pair placeholder eval would miss
    /// conditional rules raising the value inside an area.
    pub fn max_constraint(engine: &DrcEngine) -> f64 {
        engine
            .query_worst_constraint(DrcConstraintT::CreepageConstraint, false)
            .map_or(0.0, |worst| worst.value().min() as f64)
    }

    /// Port of `CollectNetCodes()`: every net code of the board, in net-code order (net 0
    /// included).
    fn collect_net_codes(ctx: &BoardCtx<'_>) -> Vec<i32> {
        ctx.board.nets_by_netcode().map(|n| n.net_code.0).collect()
    }

    /// Port of `testCreepage()` (the board-level overload).
    fn test_creepage_all(&mut self, engine: &DrcEngine, host: &mut RunHost<'_, '_>, ctx: &BoardCtx<'_>) -> i32 {
        let netcodes = Self::collect_net_codes(ctx);
        let max_constraint = Self::max_constraint(engine);

        if max_constraint <= 0.0 {
            return 0;
        }

        // No conditional rule means the per-pair placeholder target is exact and the graph
        // stays tight
        let has_conditional =
            engine.has_conditional_constraint(DrcConstraintT::CreepageConstraint);

        if self.realtime_creepage {
            return self.test_creepage_v2(engine, host, ctx, &netcodes, max_constraint, has_conditional);
        }

        let board = ctx.board;

        // Subtract NPTH holes from the outline polygon so candidate-path midpoint tests
        // reject creepage segments routed through slot interiors. Without subtraction,
        // a midpoint inside an NPTH oval still counts as "inside the board" and the
        // creepage validator accepts straight-through-slot paths (issue #24286).
        let (outline, has_valid_outline) =
            board.get_board_polygon_outlines(ctx, false, None, false, true);

        let mut graph = CreepageGraph::new(ctx);

        if self.enable_creepage_slot {
            graph.min_groove_width = board.design_settings().min_groove_width;
        } else {
            graph.min_groove_width = 0;
        }

        graph.board_outline = has_valid_outline.then_some(outline);

        // CollectBoardEdges()
        build_creepage_board_edges(
            ctx,
            &mut graph.board_edge,
            &mut graph.owned_board_edges,
            None,
        );
        graph.transform_edge_to_creep_shapes();
        graph.remove_duplicated_shapes();
        graph.transform_creep_shapes_to_nodes();

        graph.generate_paths(max_constraint, PcbLayerId::EdgeCuts, None);

        let be_node_size = graph.nodes.len();
        let be_connections_size = graph.connections.len();
        let mut prev_test_changed_graph = false;

        let mut current: usize = 0;
        let total = (netcodes.len() * netcodes.len().wrapping_sub(1)) / 2
            * board.copper_layer_count() as usize;
        let layers = board.enabled_layers();

        // alg::for_all_pairs
        for i in 0..netcodes.len() {
            for j in i + 1..netcodes.len() {
                let (net1, net2) = (netcodes[i], netcodes[j]);

                if net1 == net2 {
                    continue;
                }

                for layer in layers.copper_layers() {
                    self.base.report_progress(host, current, total, 1);
                    current += 1;

                    if prev_test_changed_graph {
                        graph.truncate_to_prefix(be_node_size, be_connections_size);
                    }

                    prev_test_changed_graph = self.test_creepage(
                        engine,
                        host,
                        &mut graph,
                        net1,
                        net2,
                        layer,
                        max_constraint,
                        has_conditional,
                    ) != 0;
                }
            }
        }

        1
    }

    /// Port of `reportCreepageViolation()`.
    fn report_creepage_violation(
        &mut self,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        a_result: &CreepageResult,
        a_constraint: &DrcConstraint,
        a_layer: PcbLayerId,
    ) {
        let (Some(item_a), Some(item_b)) = (a_result.item_a, a_result.item_b) else {
            return;
        };

        if !self.reported_pairs.insert((item_a, item_b)) {
            return;
        }

        // The engine's graph is gone: an owned board edge cannot be a copper item, its UUID
        // is the nil one of a transient shape.
        let owned: &[PcbShape] = &[];
        let mut drc_item = item::create(DRCE_CREEPAGE).expect("DRCE_CREEPAGE item");
        drc_item.set_error_detail(&self.base.format_msg(
            "(%s creepage %s; actual %s)",
            &a_constraint.name(),
            a_constraint.value().min() as f64,
            a_result.distance,
            EdaDataType::Distance,
        ));
        drc_item.violating_rule = a_constraint.parent_rule.clone();
        drc_item.set_item_ids(vec![
            creep_item_uuid(ctx, owned, item_a),
            creep_item_uuid(ctx, owned, item_b),
        ]);

        // reportViolation invokes the callback inline, so the marker can reference aResult
        // directly
        self.base.report_violation(
            host,
            Arc::new(drc_item),
            a_result.start,
            a_layer.0,
            &|marker: &mut PcbMarker| {
                marker.set_path(a_result.path.clone(), a_result.start, a_result.end);
            },
        );
    }

    /// Port of `testCreepageV2()`: the realtime (V2) batch path, gated by the
    /// `RealtimeCreepage` advanced config flag.
    fn test_creepage_v2(
        &mut self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        a_netcodes: &[i32],
        a_max_creepage: f64,
        a_has_conditional: bool,
    ) -> i32 {
        let board = ctx.board;
        let mut creepage_engine = CreepageEngine::new();

        if self.enable_creepage_slot {
            creepage_engine.set_min_groove_width(board.design_settings().min_groove_width);
        }

        let layers = board.enabled_layers();
        let mut current: usize = 0;
        let total = (a_netcodes.len() * a_netcodes.len().wrapping_sub(1)) / 2
            * board.copper_layer_count() as usize;

        for i in 0..a_netcodes.len() {
            for j in i + 1..a_netcodes.len() {
                let (net1, net2) = (a_netcodes[i], a_netcodes[j]);

                if net1 == net2 {
                    continue;
                }

                for layer in layers.copper_layers() {
                    self.base.report_progress(host, current, total, 1);
                    current += 1;

                    // Conditional rules solve at worst-case so no path is pruned, then
                    // resolve from the conductors the engine returns; otherwise the
                    // placeholder is exact
                    let mut net_constraint = DrcConstraint::default();
                    let target: f64;

                    if a_has_conditional {
                        target = a_max_creepage;
                    } else {
                        net_constraint =
                            Self::placeholder_constraint(engine, ctx, net1, net2, layer);
                        target = net_constraint.value().min() as f64;
                    }

                    let Some(result) =
                        creepage_engine.solve_net_pair_whole_board(ctx, net1, net2, layer, target)
                    else {
                        continue;
                    };

                    let (Some(item_a), Some(item_b)) = (result.item_a, result.item_b) else {
                        continue;
                    };

                    let constraint = if a_has_conditional {
                        engine.eval_rules(
                            ctx,
                            DrcConstraintT::CreepageConstraint,
                            creep_item_ref(ctx, &[], item_a),
                            creep_item_ref(ctx, &[], item_b),
                            layer,
                        )
                    } else {
                        net_constraint
                    };

                    let creepage_value = constraint.value().min() as f64;

                    if creepage_value > 0.0 && result.distance - creepage_value < 0.0 {
                        self.report_creepage_violation(host, ctx, &result, &constraint, layer);
                    }
                }
            }
        }

        1
    }
}

impl DrcTestProviderRun for DrcTestProviderCreepage {
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
