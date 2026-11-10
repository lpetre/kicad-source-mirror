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

//! Port of `pcbnew/drc/drc_test_provider_matched_length.cpp`: single-ended matched length,
//! skew, via count and net chain (trunk length, stub length, return path) tests.
//!
//! Errors generated: `DRCE_LENGTH_OUT_OF_RANGE`, `DRCE_SKEW_OUT_OF_RANGE`,
//! `DRCE_VIA_COUNT_OUT_OF_RANGE`, `DRCE_NET_CHAIN_STUB_TOO_LONG`,
//! `DRCE_NET_CHAIN_RETURN_PATH_BREAK`.
//!
//! Order notes (`rust/docs/analysis/drc.md` section 4):
//! - `itemSets` / `matches` are `std::map<DRC_RULE*, ...>` (address order of the rules): the
//!   port keys them by `DrcRuleInfo::rule_index` (rule file order). The order in which rules
//!   are reported is therefore not reproducible in C++; compare the violations as a set.
//! - `ENTRY::items` is a `std::set<BOARD_CONNECTED_ITEM*>` (address order): the port uses
//!   `ItemId` order. The marker position / layer of a length, skew or via count violation is
//!   that of the first item, the item list of a skew or via count violation is only the last
//!   item (`SetItems( item )` in a loop replaces the list), and the length calculation walks
//!   the items in this order. Compare those positions and items by membership in the net's
//!   items.
//! - The chain topology cache (`std::map<wxString, ...>`) and the return path zone unions
//!   (`std::map<std::pair<PCB_LAYER_ID, wxString>, ...>`) have value keys: order as in C++.
//!
//! The from-to cache (`GetFromToCache()->Rebuild( m_board )`) needs the connectivity mutably:
//! the caller rebuilds it right before running the provider (`run_flow`). The
//! `aDelayReportMode` of `runInternal` (the net inspector's delay report) is not ported: the
//! provider always runs the checks.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::Severity;
use kicad_core::base_units::ARC_HIGH_DEF;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::minoptmax::MinOptMax;
use kicad_core::string_utils::wild_compare_string;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::stdsort::std_sort;
use kicad_pcb::drc_api::{
    DRCE_LENGTH_OUT_OF_RANGE, DRCE_NET_CHAIN_RETURN_PATH_BREAK, DRCE_NET_CHAIN_STUB_TOO_LONG,
    DRCE_SKEW_OUT_OF_RANGE, DRCE_VIA_COUNT_OUT_OF_RANGE, DrcConstraint, DrcRuleInfo,
    drc_constraint_options,
};
use kicad_pcb::pcb_marker::RcItem;
use kicad_pcb::{BoardCtx, ItemId, ItemRef, NetCode};

use super::run_host::RunHost;
use crate::chain_topology::ChainTopology;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::length_delay_calculation::{
    LdcItemType, LengthDelayCalculation, LengthDelayDomainOpt, LengthDelayLayerOpt,
    PathOptimisations,
};
use crate::length_report::{DrcLengthReport, LengthReportEntry};
use crate::net_chain_bridging::board_chain_bridging;
use crate::test_provider::{DrcRunHost, DrcTestProviderBase, for_each_geometry_item, no_path};

/// `CONNECTION` (`DRC_LENGTH_REPORT::ENTRY`).
type Connection = LengthReportEntry;

/// The `DRC_RULE*` identity: the rule's index in the engine.
type RuleKey = u32;

/// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH`.
pub struct DrcTestProviderMatchedLength<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `m_board->GetConnectivity()` (the from-to cache must have been rebuilt).
    conn: &'c ConnectivityData,
    /// `m_report`
    report: DrcLengthReport,
    /// `m_chainTopoCache`
    chain_topo_cache: BTreeMap<String, Arc<ChainTopology>>,
    /// `m_refUnionCache`
    ref_union_cache: BTreeMap<(i32, String), ShapePolySet>,
}

/// A rule's constraint as `rule->FindConstraint( type )` gives it, with the parent rule as
/// the engine knows it (with its `rule_index`).
fn find_constraint(engine: &DrcEngine, rule: &DrcRuleInfo, type_: DrcConstraintT) -> Option<DrcConstraint> {
    let r = engine.rule(rule.rule_index?)?;
    let mut c = r.find_constraint(type_)?;
    c.parent_rule = Some(rule.clone());
    Some(c)
}

impl<'c> DrcTestProviderMatchedLength<'c> {
    /// `DRC_TEST_PROVIDER_MATCHED_LENGTH()` with the board connectivity.
    pub fn new(conn: &'c ConnectivityData) -> Self {
        DrcTestProviderMatchedLength {
            base: DrcTestProviderBase::new("length"),
            conn,
            report: DrcLengthReport::new(),
            chain_topo_cache: BTreeMap::new(),
            ref_union_cache: BTreeMap::new(),
        }
    }

    /// `m_report`: the length report of the last run.
    pub fn report(&self) -> &DrcLengthReport {
        &self.report
    }

    fn first_item<'a>(ctx: &'a BoardCtx<'a>, ent: &Connection) -> ItemRef<'a> {
        ctx.item(*ent.items.iter().next().expect("a connection item"))
            .expect("a board item")
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::checkLengths`.
    fn check_lengths(
        &self,
        host: &mut dyn DrcRunHost,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        matched_connections: &[Connection],
    ) {
        for ent in matched_connections {
            let mut min_violation = false;
            let mut max_violation = false;
            let mut min_len = 0;
            let mut max_len = 0;

            let is_time_domain = constraint.option(drc_constraint_options::TIME_DOMAIN);
            let data_type = if is_time_domain {
                EdaDataType::Time
            } else {
                EdaDataType::Distance
            };
            let value = constraint.value();

            let measured = if is_time_domain {
                ent.total_delay
            } else {
                ent.total
            };

            if value.has_min() && measured < f64::from(value.min()) {
                min_violation = true;
                min_len = value.min();
            } else if value.has_max() && measured > f64::from(value.max()) {
                max_violation = true;
                max_len = value.max();
            }

            if min_violation || max_violation {
                let mut drc_item = item::create(DRCE_LENGTH_OUT_OF_RANGE).expect("a DRC item");

                if min_violation {
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s min length %s; actual %s)",
                        &constraint.name(),
                        f64::from(min_len),
                        measured,
                        data_type,
                    ));
                } else if max_violation {
                    drc_item.set_error_detail(&self.base.format_msg(
                        "(%s max length %s; actual %s)",
                        &constraint.name(),
                        f64::from(max_len),
                        measured,
                        data_type,
                    ));
                }

                // AddItem for each offending track
                drc_item.set_item_ids(
                    ent.items
                        .iter()
                        .map(|&id| ctx.item(id).expect("a board item").uuid())
                        .collect(),
                );

                drc_item.violating_rule = constraint.parent_rule.clone();

                let first = Self::first_item(ctx, ent);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    first.position(),
                    first.layer().0,
                    &no_path,
                );
            }
        }
    }

    /// The `checkSkewsImpl` lambda of `checkSkews`.
    fn check_skews_impl(
        &self,
        host: &mut dyn DrcRunHost,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        connections: &[Connection],
    ) {
        let is_time_domain = constraint.option(drc_constraint_options::TIME_DOMAIN);
        let data_type = if is_time_domain {
            EdaDataType::Time
        } else {
            EdaDataType::Distance
        };

        let mut max_length = 0.0;
        let mut max_netname = String::new();

        // The reference is compared with itself, so the minimum must not be applied to it.
        let mut reference_entry: Option<usize> = None;

        for (i, ent) in connections.iter().enumerate() {
            let len = if is_time_domain {
                ent.total_delay
            } else {
                ent.total
            };

            if reference_entry.is_none() || len > max_length {
                max_length = len;
                max_netname = ent.netname.clone();
                reference_entry = Some(i);
            }
        }

        let units = self.base.units_provider();
        let value = constraint.value();

        for (i, ent) in connections.iter().enumerate() {
            let skew = if is_time_domain {
                kicad_geom::math::util::ki_round(ent.total_delay - max_length)
            } else {
                kicad_geom::math::util::ki_round(ent.total - max_length)
            };

            let mut fail_min = false;
            let mut fail_max = false;

            if value.has_max() && skew.abs() > value.max() {
                fail_max = true;
            } else if value.has_min() && Some(i) != reference_entry && skew.abs() < value.min() {
                fail_min = true;
            }

            if fail_min || fail_max {
                let mut drc_item = item::create(DRCE_SKEW_OUT_OF_RANGE).expect("a DRC item");

                let report_total = if is_time_domain {
                    ent.total_delay
                } else {
                    ent.total
                };

                let (kind, limit) = if fail_min {
                    ("min", value.min())
                } else {
                    ("max", value.max())
                };

                let msg = format!(
                    "({} {} skew {}; actual {}; target net length {} (from {}); actual {})",
                    constraint.name(),
                    kind,
                    units.message_text_from_value(f64::from(limit), true, data_type),
                    units.message_text_from_value(f64::from(skew), true, data_type),
                    units.message_text_from_value(max_length, true, data_type),
                    max_netname,
                    units.message_text_from_value(report_total, true, data_type),
                );

                drc_item.set_error_detail(&msg);

                // SetItems( offendingTrack ) in a loop: the last item remains.
                let last = *ent.items.iter().next_back().expect("a connection item");
                drc_item.set_item_ids(vec![ctx.item(last).expect("a board item").uuid()]);

                drc_item.violating_rule = constraint.parent_rule.clone();

                let first = Self::first_item(ctx, ent);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    first.position(),
                    first.layer().0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::checkSkews`.
    fn check_skews(
        &self,
        host: &mut dyn DrcRunHost,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        matched_connections: &[Connection],
    ) {
        if constraint.option(drc_constraint_options::SKEW_WITHIN_DIFF_PAIRS) {
            // Find all pairs of nets in the matched connections
            let mut netcode_map: BTreeMap<i32, Connection> = BTreeMap::new();

            for ent in matched_connections {
                netcode_map.insert(ent.netcode, ent.clone());
            }

            let mut matched_diff_pairs: Vec<Vec<Connection>> = Vec::new();

            // The C++ erases the coupled net from the map while iterating it (an entry
            // after the current one): walk the keys in order and skip erased ones.
            let keys: Vec<i32> = netcode_map.keys().copied().collect();

            for netcode in keys {
                let Some(connection) = netcode_map.get(&netcode).cloned() else {
                    continue;
                };

                let matched_net = connection
                    .netinfo
                    .and_then(|n| ctx.board.dp_coupled_net(NetCode(n)));

                if let Some(matched_net) = matched_net {
                    let matched_netcode = matched_net.0;

                    if let Some(other) = netcode_map.get(&matched_netcode).cloned() {
                        matched_diff_pairs.push(vec![connection, other]);
                        netcode_map.remove(&matched_netcode);
                    }
                }
            }

            // Test all found pairs of nets
            for matched_diff_pair in &matched_diff_pairs {
                self.check_skews_impl(host, ctx, constraint, matched_diff_pair);
            }
        } else {
            // Test all matched nets as a group
            self.check_skews_impl(host, ctx, constraint, matched_connections);
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::checkViaCounts`.
    fn check_via_counts(
        &self,
        host: &mut dyn DrcRunHost,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        matched_connections: &[Connection],
    ) {
        let value = constraint.value();

        for ent in matched_connections {
            let mut drc_item: Option<RcItem> = None;

            if value.has_max() && ent.via_count > value.max() {
                let mut it = item::create(DRCE_VIA_COUNT_OUT_OF_RANGE).expect("a DRC item");
                let msg = format!(
                    "({} max count {}; actual {})",
                    constraint.name(),
                    value.max(),
                    ent.via_count
                );

                it.set_error_message(&format!("Too many vias on a connection {msg}"));
                drc_item = Some(it);
            } else if value.has_min() && ent.via_count < value.min() {
                let mut it = item::create(DRCE_VIA_COUNT_OUT_OF_RANGE).expect("a DRC item");
                let msg = format!(
                    "({} min count {}; actual {})",
                    constraint.name(),
                    value.min(),
                    ent.via_count
                );

                it.set_error_message(&format!("Too few vias on a connection {msg}"));
                drc_item = Some(it);
            }

            if let Some(mut drc_item) = drc_item {
                // SetItems( offendingTrack ) in a loop: the last item remains.
                let last = *ent.items.iter().next_back().expect("a connection item");
                drc_item.set_item_ids(vec![ctx.item(last).expect("a board item").uuid()]);

                drc_item.violating_rule = constraint.parent_rule.clone();

                let first = Self::first_item(ctx, ent);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    first.position(),
                    first.layer().0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::chainTopologyFor`: the topology of the named
    /// chain, built from every track and pad of the board on the chain (cached per run).
    fn chain_topology_for(&mut self, calc: &LengthDelayCalculation<'_>, chain: &str) -> Arc<ChainTopology> {
        if let Some(topo) = self.chain_topo_cache.get(chain) {
            return topo.clone();
        }

        let ctx = calc.ctx();
        let board = ctx.board;
        let mut items: BTreeSet<ItemId> = BTreeSet::new();
        let on_chain = |id: ItemId| {
            ctx.item(id)
                .and_then(|i| i.net())
                .is_some_and(|n| n.net_chain() == chain)
        };

        for (t, _) in board.tracks() {
            if on_chain(t.item_id()) {
                items.insert(t.item_id());
            }
        }

        for (fp, _) in board.footprints() {
            for (p, _) in board.footprint_pads(fp) {
                if on_chain(p.item_id()) {
                    items.insert(p.item_id());
                }
            }
        }

        let topo = Arc::new(ChainTopology::new(calc, chain, &items, 0.0));
        self.chain_topo_cache
            .insert(chain.to_string(), topo.clone());
        topo
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::zoneUnionFor`: the union of the copper of the
    /// zones on the reference layer (whose net matches the pattern), cached per run.
    fn zone_union_for(&mut self, ctx: &BoardCtx<'_>, ref_layer: PcbLayerId, ref_net: &str) -> ShapePolySet {
        let key = (ref_layer.0, ref_net.to_string());

        if let Some(u) = self.ref_union_cache.get(&key) {
            return u.clone();
        }

        let mut ref_union = ShapePolySet::new();

        for (zone_id, zone) in ctx.board.zones() {
            let zone_ref = ctx.item(zone_id.item_id()).expect("a board zone");

            if !zone.is_on_layer(ctx, ref_layer) || zone.is_rule_area() {
                continue;
            }

            if !ref_net.is_empty() {
                let zone_net = zone_ref.netname();

                if !wild_compare_string(ref_net, &zone_net, false) {
                    continue;
                }
            }

            // Prefer the cached filled-fill polys (respect holes / islands).
            // Fall back to the zone outline for synthetic / unfilled boards
            // (test fixtures, freshly imported designs).  GetFill returns
            // nullptr when the zone hasn't been filled for this layer; using
            // GetFilledPolysList here would assert.
            match zone.get_fill(ref_layer) {
                Some(filled) if filled.outline_count() > 0 => ref_union.boolean_add(filled),
                _ => {
                    // GetBoardOutline() is expensive.  Only use it in DRC where we have to.
                    if zone_ref.parent_footprint().is_some() {
                        ref_union.boolean_add(&zone.board_outline(ctx));
                    } else {
                        ref_union.boolean_add(zone.outline());
                    }
                }
            }
        }

        ref_union.simplify();
        self.ref_union_cache.insert(key, ref_union.clone());
        ref_union
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::checkStubLengths`.
    fn check_stub_lengths(
        &mut self,
        host: &mut dyn DrcRunHost,
        calc: &LengthDelayCalculation<'_>,
        constraint: &DrcConstraint,
        rule: &DrcRuleInfo,
        matched_connections: &[Connection],
    ) {
        let ctx = calc.ctx();
        let is_time_domain = constraint.option(drc_constraint_options::TIME_DOMAIN);
        let data_type = if is_time_domain {
            EdaDataType::Time
        } else {
            EdaDataType::Distance
        };
        let range: MinOptMax<i32> = *constraint.value();

        if !range.has_max() && !range.has_min() {
            return;
        }

        let out_of_range = |measured: f64| {
            (range.has_max() && measured > f64::from(range.max()))
                || (range.has_min() && measured < f64::from(range.min()))
        };

        let units = self.base.units_provider();
        let net_of = |code: i32| ctx.board.find_net(NetCode(code));

        // Collect the distinct chains touched by the rule's matched items, then
        // dispatch each chain through the topology if it's valid, falling back
        // to the legacy proxy otherwise.
        let mut chain_names: BTreeSet<String> = BTreeSet::new();

        for conn in matched_connections {
            if let Some(ni) = conn.netinfo.and_then(net_of)
                && !ni.net_chain().is_empty()
            {
                chain_names.insert(ni.net_chain().to_string());
            }
        }

        let mut handled_by_topology: BTreeSet<String> = BTreeSet::new();

        for chain_name in &chain_names {
            let topo = self.chain_topology_for(calc, chain_name);

            if !topo.is_valid() {
                continue;
            }

            handled_by_topology.insert(chain_name.clone());

            for stub in topo.stubs() {
                let measured = if is_time_domain {
                    stub.delay
                } else {
                    stub.length
                };

                if !out_of_range(measured) {
                    continue;
                }

                let mut drc_item = item::create(DRCE_NET_CHAIN_STUB_TOO_LONG).expect("a DRC item");
                drc_item.set_error_message(&format!(
                    "Stub length ({}) out of range for net chain '{}'.",
                    units.message_text_from_value(measured, true, data_type),
                    chain_name
                ));
                drc_item.violating_rule = Some(rule.clone());

                drc_item.set_item_ids(
                    stub.items
                        .iter()
                        .map(|&id| ctx.item(id).expect("a board item").uuid())
                        .collect(),
                );

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    stub.branch_point,
                    stub.branch_layer.0,
                    &no_path,
                );
            }
        }

        // Legacy proxy fallback for chains where the topology builder reported an
        // invalid status (no terminal pads, disconnected, or cycle).  This
        // preserves behavior on rule sets that pre-date the terminal-pad model.
        for conn in matched_connections {
            let Some(net_info) = net_of(conn.netcode) else {
                continue;
            };

            if net_info.net_chain().is_empty() {
                continue;
            }

            if handled_by_topology.contains(net_info.net_chain()) {
                continue;
            }

            let terminal_net = |index: usize| {
                net_info
                    .terminal_pad(index)
                    .and_then(|p| ctx.item(p.item_id()))
                    .and_then(|p| p.net_code())
            };

            let on_trunk = terminal_net(0) == Some(NetCode(conn.netcode))
                || terminal_net(1) == Some(NetCode(conn.netcode));

            if on_trunk {
                continue;
            }

            let measured = if is_time_domain {
                conn.total_delay
            } else {
                conn.total
            };

            if !out_of_range(measured) {
                continue;
            }

            let mut drc_item = item::create(DRCE_NET_CHAIN_STUB_TOO_LONG).expect("a DRC item");
            drc_item.set_error_message(&format!(
                "Stub length ({}) out of range for net chain '{}' on net '{}'.",
                units.message_text_from_value(measured, true, data_type),
                net_info.net_chain(),
                net_info.netname()
            ));
            drc_item.violating_rule = Some(rule.clone());

            drc_item.set_item_ids(
                conn.items
                    .iter()
                    .map(|&id| ctx.item(id).expect("a board item").uuid())
                    .collect(),
            );

            let mut pos = Vector2I::default();
            let mut layer = PcbLayerId::UndefinedLayer;

            if !conn.items.is_empty() {
                let first = Self::first_item(ctx, conn);
                pos = first.position();
                layer = first.layer();
            }

            self.base
                .report_violation(host, Arc::new(drc_item), pos, layer.0, &no_path);
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::checkReturnPath`.
    fn check_return_path(
        &mut self,
        host: &mut dyn DrcRunHost,
        ctx: &BoardCtx<'_>,
        constraint: &DrcConstraint,
        rule: &DrcRuleInfo,
        chain_agg: &BTreeMap<String, Connection>,
    ) {
        let ref_layer_name = &constraint.reference_layer;
        let ref_net_pattern = &constraint.reference_net;

        let ref_layer = ctx.board.layer_id(ref_layer_name);

        if ref_layer == PcbLayerId::UndefinedLayer {
            return;
        }

        let ref_union = self.zone_union_for(ctx, ref_layer, ref_net_pattern);
        let ref_bbox = if ref_union.outline_count() > 0 {
            ref_union.bbox(0)
        } else {
            Box2I::default()
        };

        /// `FlaggedRegion`
        struct FlaggedRegion {
            poly: ShapePolySet,
            items: Vec<ItemId>,
            layer: PcbLayerId,
        }

        let flag_item = |item: ItemRef<'_>, flagged: &mut Vec<FlaggedRegion>, region: &ShapePolySet| {
            for o in 0..region.outline_count() {
                let mut poly = ShapePolySet::new();
                poly.add_outline(region.c_outline(o));

                for h in 0..region.hole_count(o) {
                    poly.add_hole(region.c_hole(o, h), -1);
                }

                flagged.push(FlaggedRegion {
                    poly,
                    items: vec![item.id().expect("a board item")],
                    layer: item.layer(),
                });
            }
        };

        for (chain_name, agg) in chain_agg {
            let mut flagged: Vec<FlaggedRegion> = Vec::new();

            for &id in &agg.items {
                let Some(item) = ctx.item(id) else {
                    continue;
                };

                if item.type_() != KicadT::PcbTraceT && item.type_() != KicadT::PcbArcT {
                    continue;
                }

                let item_bbox = item.bounding_box();

                let mut item_poly = ShapePolySet::new();
                item.transform_shape_to_polygon(
                    &mut item_poly,
                    item.layer(),
                    0,
                    ARC_HIGH_DEF,
                    ErrorLoc::Inside,
                    false,
                );

                if ref_union.outline_count() == 0 || !item_bbox.intersects(&ref_bbox) {
                    // Nothing on the reference layer covers this item.
                    flag_item(item, &mut flagged, &item_poly);
                    continue;
                }

                let mut diff = item_poly.clone();
                diff.boolean_subtract(&ref_union);
                diff.simplify();

                if diff.outline_count() == 0 {
                    continue;
                }

                flag_item(item, &mut flagged, &diff);
            }

            if flagged.is_empty() {
                continue;
            }

            // Coalesce flagged regions by spatial adjacency: two regions belong to
            // the same group if their bounding boxes intersect (within epsilon).
            // Provider-local union-find over the flagged set only — does NOT use
            // CONNECTIVITY_DATA, which would merge across covered tracks.
            let mut parent: Vec<usize> = (0..flagged.len()).collect();

            fn find(parent: &mut [usize], mut x: usize) -> usize {
                while parent[x] != x {
                    parent[x] = parent[parent[x]];
                    x = parent[x];
                }

                x
            }

            for i in 0..flagged.len() {
                let mut bi = flagged[i].poly.bbox(0);
                bi.inflate(1);

                for j in i + 1..flagged.len() {
                    let bj = flagged[j].poly.bbox(0);

                    // Cheap reject: if the inflated bboxes don't even touch,
                    // the polygons cannot be adjacent.
                    if !bi.intersects(&bj) {
                        continue;
                    }

                    // Confirm polygon-level adjacency: two flagged regions
                    // belong to the same group only if their copper polygons
                    // actually intersect (after a 1 IU inflate to bridge
                    // touching boundaries).  Bbox-touch alone can merge
                    // unrelated runs whose AABBs overlap but copper does not.
                    let mut probe = flagged[i].poly.clone();
                    probe.inflate(1, CornerStrategy::RoundAllCorners, ARC_HIGH_DEF, false);
                    probe.boolean_intersection(&flagged[j].poly);
                    probe.simplify();

                    if probe.outline_count() == 0 {
                        continue;
                    }

                    let ri = find(&mut parent, i);
                    let rj = find(&mut parent, j);

                    if ri != rj {
                        parent[ri] = rj;
                    }
                }
            }

            let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

            for i in 0..flagged.len() {
                let root = find(&mut parent, i);
                groups.entry(root).or_default().push(i);
            }

            for members in groups.values() {
                let mut union_poly = ShapePolySet::new();
                let mut items: Vec<ItemId> = Vec::new();
                let mut seen: BTreeSet<ItemId> = BTreeSet::new();
                let mut marker_layer = PcbLayerId::UndefinedLayer;

                for &idx in members {
                    union_poly.boolean_add(&flagged[idx].poly);

                    for &it in &flagged[idx].items {
                        if seen.insert(it) {
                            items.push(it);
                        }
                    }

                    if marker_layer == PcbLayerId::UndefinedLayer {
                        marker_layer = flagged[idx].layer;
                    }
                }

                union_poly.simplify();

                let marker_pos = if union_poly.outline_count() > 0 {
                    union_poly.bbox(0).centre()
                } else if let Some(&first) = items.first() {
                    ctx.item(first).expect("a board item").position()
                } else {
                    Vector2I::default()
                };

                let mut drc_item =
                    item::create(DRCE_NET_CHAIN_RETURN_PATH_BREAK).expect("a DRC item");

                let mut msg = format!(
                    "Net chain '{chain_name}' has no copper return path on reference layer '{ref_layer_name}'."
                );

                if !ref_net_pattern.is_empty() {
                    msg += &format!(" (net '{ref_net_pattern}')");
                }

                drc_item.set_error_message(&msg);
                drc_item.violating_rule = Some(rule.clone());

                drc_item.set_item_ids(
                    items
                        .iter()
                        .map(|&id| ctx.item(id).expect("a board item").uuid())
                        .collect(),
                );

                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    marker_pos,
                    marker_layer.0,
                    &no_path,
                );
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_MATCHED_LENGTH::Run` (`runInternal( false )`).
    pub fn run(&mut self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let board = ctx.board;
        self.report.clear();
        self.chain_topo_cache.clear();
        self.ref_union_cache.clear();

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Gathering length-constrained connections...")
        {
            return false;
        }

        let board_copper_layers = Lset::all_cu_mask_n(board.copper_layer_count());
        let mut item_sets: BTreeMap<RuleKey, (DrcRuleInfo, BTreeSet<ItemId>)> = BTreeMap::new();

        // ftCache->Rebuild( m_board ) ran in the caller (run_flow).
        let ft_cache = self.conn.from_to_cache();

        let progress_delta = 100;
        let mut count = 0usize;
        let mut ii = 0usize;

        let types = [
            KicadT::PcbTraceT,
            KicadT::PcbArcT,
            KicadT::PcbViaT,
            KicadT::PcbPadT,
        ];

        for_each_geometry_item(ctx, &types, &board_copper_layers, |_item| {
            count += 1;
            true
        });

        let mut cancelled = false;

        for_each_geometry_item(ctx, &types, &board_copper_layers, |item| {
            if !self.base.report_progress(&mut host, ii, count, progress_delta) {
                cancelled = true;
                return false;
            }

            ii += 1;

            for jj in [
                DrcConstraintT::LengthConstraint,
                DrcConstraintT::NetChainLengthConstraint,
                DrcConstraintT::NetChainStubLengthConstraint,
                DrcConstraintT::NetChainReturnPathConstraint,
                DrcConstraintT::SkewConstraint,
                DrcConstraintT::ViaCountConstraint,
            ] {
                let constraint = engine.eval_rules(ctx, jj, Some(item), None, item.layer());

                if constraint.is_null() {
                    continue;
                }

                // itemSets[ constraint.GetParentRule() ]: a constraint without a rule (or with
                // a rule outside the engine) cannot be grouped (the C++ would dereference a
                // null rule later).
                let Some(rule) = constraint.parent_rule.clone() else {
                    continue;
                };

                let Some(rule_index) = rule.rule_index else {
                    continue;
                };

                item_sets
                    .entry(rule_index)
                    .or_insert_with(|| (rule, BTreeSet::new()))
                    .1
                    .insert(item.id().expect("a board item"));
            }

            true
        });

        if cancelled {
            return false;
        }

        let calc = LengthDelayCalculation::new(ctx, Some(self.conn));

        let mut matches: BTreeMap<RuleKey, (DrcRuleInfo, Vec<Connection>)> = BTreeMap::new();

        for (&rule_key, (rule, rule_items)) in &item_sets {
            let mut net_map: BTreeMap<i32, BTreeSet<ItemId>> = BTreeMap::new();

            for &id in rule_items {
                let code = ctx.item(id).and_then(|i| i.net_code()).map_or(-1, |c| c.0);
                net_map.entry(code).or_default().insert(id);
            }

            for (&net_code, net_items) in &net_map {
                let mut length_items = Vec::with_capacity(net_items.len());

                let net = board.find_net(NetCode(net_code));

                let mut ent = Connection {
                    items: net_items.clone(),
                    netcode: net_code,
                    netname: net.map(|n| n.netname().to_string()).unwrap_or_default(),
                    netinfo: net.map(|n| n.net_code.0),
                    ..Connection::default()
                };

                for &id in net_items {
                    let length_item = calc.length_calculation_item(ctx.item(id).expect("an item"));

                    if length_item.type_() != LdcItemType::Unknown {
                        length_items.push(length_item);
                    }
                }

                let opts = PathOptimisations {
                    optimise_vias: true,
                    merge_tracks: true,
                    optimise_traces_in_pads: true,
                    infer_via_in_pad: false,
                };
                let details = calc.calculate_length_details(
                    &mut length_items,
                    opts,
                    None,
                    None,
                    LengthDelayLayerOpt::NoLayerDetail,
                    LengthDelayDomainOpt::WithDelayDetail,
                    None,
                );
                ent.via_count = details.num_vias;
                ent.total_via = details.via_length;
                ent.total_via_delay = details.via_delay;
                ent.total_route = details.track_length as f64;
                ent.total_route_delay = details.track_delay as f64;
                ent.total_pad_to_die = details.pad_to_die_length;
                ent.total_pad_to_die_delay = details.pad_to_die_delay;
                ent.total = ent.total_route + f64::from(ent.total_via) + f64::from(ent.total_pad_to_die);
                ent.total_delay = ent.total_route_delay
                    + ent.total_via_delay as f64
                    + ent.total_pad_to_die_delay as f64;
                ent.matching_rule = Some(rule.clone());

                if let Some(ft_path) = ft_cache.query_from_to_path(&ent.items) {
                    ent.from = ft_path.from_name.clone();
                    ent.to = ft_path.to_name.clone();
                } else {
                    ent.from = "<unconstrained>".to_string();
                    ent.to = "<unconstrained>".to_string();
                }

                self.report.add(&ent);
                matches
                    .entry(rule_key)
                    .or_insert_with(|| (rule.clone(), Vec::new()))
                    .1
                    .push(ent);
            }
        }

        drop(ft_cache);

        if !self
            .base
            .report_phase(&mut host, "Checking length constraints...")
        {
            return false;
        }

        ii = 0;
        count = matches.len();

        for (_, (rule, mut matched_connections)) in matches {
            if !self.base.report_progress(&mut host, ii, count, progress_delta) {
                return false;
            }

            ii += 1;

            std_sort(&mut matched_connections, |a, b| a.netname < b.netname);

            if host.log_reporter().is_some() {
                let units = self.base.units_provider();
                let mut lines = vec![format!(
                    "Length-constrained traces for rule '{}':",
                    rule.name
                )];

                for ent in &matched_connections {
                    lines.push(format!(
                        " - net: {}, from: {}, to: {}, {} matching items, total: {} (tracks: {}, vias: {}, pad-to-die: {}), vias: {}",
                        ent.netname,
                        ent.from,
                        ent.to,
                        ent.items.len(),
                        units.message_text_from_value(ent.total, true, EdaDataType::Distance),
                        units.message_text_from_value(ent.total_route, true, EdaDataType::Distance),
                        units.message_text_from_value(f64::from(ent.total_via), true, EdaDataType::Distance),
                        units.message_text_from_value(f64::from(ent.total_pad_to_die), true, EdaDataType::Distance),
                        ent.via_count
                    ));
                }

                if let Some(r) = host.log_reporter() {
                    for line in &lines {
                        r.report(line, Severity::Info);
                    }
                }
            }

            // Check both net-level and net-chain-level length constraints
            let length_constraint = find_constraint(engine, &rule, DrcConstraintT::LengthConstraint);
            let net_chain_length_constraint =
                find_constraint(engine, &rule, DrcConstraintT::NetChainLengthConstraint);

            // Build per-chain aggregates (kept for the length report).  The
            // shared CHAIN_TOPOLOGY is fetched lazily from the provider's
            // cache so two rules constraining the same chain don't rebuild
            // the graph.
            let mut chain_agg: BTreeMap<String, Connection> = BTreeMap::new();
            let stub_constraint =
                find_constraint(engine, &rule, DrcConstraintT::NetChainStubLengthConstraint);
            let return_path_constraint =
                find_constraint(engine, &rule, DrcConstraintT::NetChainReturnPathConstraint);

            let active = |c: &Option<DrcConstraint>| {
                c.as_ref().is_some_and(|c| c.severity() != Severity::Ignore)
            };

            let needs_chain_agg = active(&net_chain_length_constraint)
                || active(&stub_constraint)
                || active(&return_path_constraint);

            if needs_chain_agg {
                for conn in &matched_connections {
                    let Some(netinfo) = conn.netinfo.and_then(|n| board.find_net(NetCode(n))) else {
                        continue;
                    };

                    let chain_name = netinfo.net_chain();

                    if chain_name.is_empty() {
                        continue;
                    }

                    if let Some(agg) = chain_agg.get_mut(chain_name) {
                        agg.total += conn.total;
                        agg.total_delay += conn.total_delay;
                        agg.total_route += conn.total_route;
                        agg.total_route_delay += conn.total_route_delay;
                        agg.total_via += conn.total_via;
                        agg.total_via_delay += conn.total_via_delay;
                        agg.total_pad_to_die += conn.total_pad_to_die;
                        agg.total_pad_to_die_delay += conn.total_pad_to_die_delay;
                        agg.via_count += conn.via_count;
                        agg.items.extend(conn.items.iter().copied());
                    } else {
                        let mut agg = conn.clone();
                        agg.netname = chain_name.to_string();
                        chain_agg.insert(chain_name.to_string(), agg);
                    }
                }
            }

            if let Some(nccl) = net_chain_length_constraint.as_ref()
                && nccl.severity() != Severity::Ignore
            {
                let mut chain_connections: Vec<Connection> = Vec::with_capacity(chain_agg.len());

                for (chain_name, agg) in &chain_agg {
                    let mut constraint_input = agg.clone();
                    let topo = self.chain_topology_for(&calc, chain_name);

                    if topo.is_valid() {
                        // Trunk semantics: constrain the matched-length signal
                        // path between terminal pads, not the sum of all member
                        // segments.  Bridging is folded into the topology graph.
                        constraint_input.total = topo.trunk_length();
                        constraint_input.total_delay = topo.trunk_delay();
                        constraint_input.total_route = topo.trunk_length();
                        constraint_input.total_route_delay = topo.trunk_delay();
                    } else {
                        // Aggregate fallback (legacy) — preserves behavior for
                        // chains without terminal pads or with looped routing.
                        // Add bridging length/delay through series passives so
                        // the time-domain rule sees a compensated chain.
                        let (bridging, bridging_delay) = board_chain_bridging(ctx, chain_name, 0.0);

                        constraint_input.total += bridging;
                        constraint_input.total_route += bridging;
                        constraint_input.total_delay += bridging_delay;
                        constraint_input.total_route_delay += bridging_delay;
                    }

                    chain_connections.push(constraint_input);
                }

                self.check_lengths(&mut host, ctx, nccl, &chain_connections);
            }

            if let Some(lc) = length_constraint.as_ref()
                && lc.severity() != Severity::Ignore
            {
                self.check_lengths(&mut host, ctx, lc, &matched_connections);
            }

            let skew_constraint = find_constraint(engine, &rule, DrcConstraintT::SkewConstraint);

            if let Some(sc) = skew_constraint.as_ref()
                && sc.severity() != Severity::Ignore
            {
                self.check_skews(&mut host, ctx, sc, &matched_connections);
            }

            let via_count_constraint =
                find_constraint(engine, &rule, DrcConstraintT::ViaCountConstraint);

            if let Some(vc) = via_count_constraint.as_ref()
                && vc.severity() != Severity::Ignore
            {
                self.check_via_counts(&mut host, ctx, vc, &matched_connections);
            }

            if let Some(rp) = return_path_constraint.as_ref()
                && rp.severity() != Severity::Ignore
                && !rp.reference_layer.is_empty()
            {
                self.check_return_path(&mut host, ctx, rp, &rule, &chain_agg);
            }

            if let Some(sc) = stub_constraint.as_ref()
                && sc.severity() != Severity::Ignore
            {
                self.check_stub_lengths(&mut host, &calc, sc, &rule, &matched_connections);
            }
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderMatchedLength<'_> {
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
