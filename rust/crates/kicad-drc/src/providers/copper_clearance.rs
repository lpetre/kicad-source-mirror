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

//! Port of `pcbnew/drc/drc_test_provider_copper_clearance.cpp`.
//!
//! Copper clearance test. Checks all copper items (pads, vias, tracks, drawings, zones) for
//! their electrical clearance.
//!
//! Errors generated:
//! - `DRCE_CLEARANCE`
//! - `DRCE_HOLE_CLEARANCE`
//! - `DRCE_TRACKS_CROSSING`
//! - `DRCE_SHORTING_ITEMS`
//!
//! Work package DRC-08a ported `Run`, `collectZonesNear`, `testSingleLayerItemAgainstItem`,
//! `testItemAgainstZone`, `testTrackClearances`, `testPadAgainstItem` and
//! `testPadClearances`. The graphic, knockout text, zone-to-zone and teardrop phases
//! (`testGraphicClearances`, `testKnockoutTextAgainstZone`, `testZonesToZones`,
//! `testTeardropClearances`) are work package DRC-08b and still `port_pending!`.
//!
//! C++ runs one thread-pool task per track and per footprint; the port is sequential in board
//! order (track list order, footprint list order), which is the order of a single-threaded
//! C++ run (`drc.md` 4.12). The free-pad claims of `testTrackClearances` (`freePadsUsageMap`,
//! first track net to touch a free pad wins) follow the same order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{Flashing, PcbLayerId, is_copper_layer};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{
    DRCE_CLEARANCE, DRCE_HOLE_CLEARANCE, DRCE_SHORTING_ITEMS, DRCE_TRACKS_CROSSING, DrcConstraint,
};
use kicad_pcb::ids::{ItemId, NetCode, PadId, ZoneId};
use kicad_pcb::item::SharedShape;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::{BoardCtx, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::diff_pair::is_net_tie_exclusion;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};
use crate::zone_index::DrcZoneIndex;

/// `"(%s clearance %s; actual %s)"`
const CLEARANCE_MSG: &str = "(%s clearance %s; actual %s)";

/// `GetNet()` of an item as a value: `None` is the null pointer of an item that is not
/// connected (`IsConnected()` false), else the net code (each code has one `NETINFO_ITEM`, so
/// comparing codes compares the pointers).
fn connected_net(item: ItemRef<'_>) -> Option<NetCode> {
    if item.is_connected() {
        item.net_code()
    } else {
        None
    }
}

/// `"(nets %s and %s)"` with `<no net>` for net code 0.
fn nets_detail(a_code: i32, a_name: &str, b_code: i32, b_name: &str) -> String {
    format!(
        "(nets {} and {})",
        if a_code != 0 { a_name } else { "<no net>" },
        if b_code != 0 { b_name } else { "<no net>" }
    )
}

/// Port of `DRC_TEST_PROVIDER_COPPER_CLEARANCE`.
///
/// `'c` is the lifetime of the borrowed per-run board caches (`m_DRCMaxClearance`,
/// `m_DRCCopperZonesByLayer`, `m_CopperItemRTreeCache`, the copper zone trees), which the
/// cache generator builds before the providers run.
pub struct DrcTestProviderCopperClearance<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches built by the cache generator for this run.
    caches: &'c DrcBoardCaches,
    /// `m_drcEpsilon`
    drc_epsilon: i32,
    /// `m_zoneIndex`
    zone_index: DrcZoneIndex,
    /// Whether `Run` executes the DRC-08b phases (graphics, zone-to-zone, teardrops). Not a
    /// C++ member: false only for [`DrcTestProviderCopperClearance::new_without_pending`].
    run_pending_phases: bool,
}

impl<'c> DrcTestProviderCopperClearance<'c> {
    /// `DRC_TEST_PROVIDER_COPPER_CLEARANCE()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderCopperClearance {
            base: DrcTestProviderBase::new("clearance"),
            caches,
            drc_epsilon: 0,
            zone_index: DrcZoneIndex::new(),
            run_pending_phases: true,
        }
    }

    /// Verification aid until work package DRC-08b lands: a provider whose `Run` executes only
    /// the phases ported so far (track & via clearances, pad clearances) and skips the
    /// graphic, zone-to-zone and teardrop phases, which are still `port_pending!`.
    pub fn new_without_pending(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderCopperClearance {
            run_pending_phases: false,
            ..Self::new(caches)
        }
    }

    /// `sub_e( aClearance )`: the clearance relaxed by the DRC epsilon, not below 0.
    fn sub_e(&self, clearance: i32) -> i32 {
        0.max(clearance - self.drc_epsilon)
    }

    /// Port of `DRC_TEST_PROVIDER_COPPER_CLEARANCE::Run`: false when the run was cancelled.
    pub fn run(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        let caches = self.caches;

        if caches.max_clearance <= 0 {
            report_aux(run, "No Clearance constraints found. Tests not run.");
            return true; // continue with other tests
        }

        self.drc_epsilon = ctx.board.design_settings().get_drc_epsilon();

        self.zone_index
            .build(&caches.copper_zones_by_layer, |zone| {
                ctx.item(ItemId::from(zone))
                    .expect("m_DRCCopperZonesByLayer holds board zones")
                    .bounding_box()
            });

        let result = self.run_phases(engine, &mut RunHost(run), ctx);

        // ZONE_INDEX_GUARD
        self.zone_index.clear();

        result
    }

    /// The body of `Run` after the zone index is built.
    fn run_phases(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) -> bool {
        if !host.is_error_limit_exceeded(DRCE_CLEARANCE) {
            if !self
                .base
                .report_phase(host, "Checking track & via clearances...")
            {
                return false; // DRC cancelled
            }

            self.test_track_clearances(engine, host, ctx);
        } else if !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE) {
            if !self.base.report_phase(host, "Checking hole clearances...") {
                return false; // DRC cancelled
            }

            self.test_track_clearances(engine, host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_CLEARANCE) {
            if !self.base.report_phase(host, "Checking pad clearances...") {
                return false; // DRC cancelled
            }

            self.test_pad_clearances(engine, host, ctx);
        } else if !host.is_error_limit_exceeded(DRCE_SHORTING_ITEMS)
            || !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE)
        {
            if !self.base.report_phase(host, "Checking pads...") {
                return false; // DRC cancelled
            }

            self.test_pad_clearances(engine, host, ctx);
        }

        if !self.run_pending_phases {
            return !host.is_cancelled();
        }

        if !host.is_error_limit_exceeded(DRCE_CLEARANCE) {
            if !self
                .base
                .report_phase(host, "Checking copper graphic clearances...")
            {
                return false; // DRC cancelled
            }

            self.test_graphic_clearances(engine, host, ctx);
        } else if !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE) {
            if !self
                .base
                .report_phase(host, "Checking copper graphic hole clearances...")
            {
                return false; // DRC cancelled
            }

            self.test_graphic_clearances(engine, host, ctx);
        }

        if !host.is_error_limit_exceeded(DRCE_CLEARANCE) {
            if !self
                .base
                .report_phase(host, "Checking copper zone clearances...")
            {
                return false; // DRC cancelled
            }

            self.test_zones_to_zones(engine, host, ctx);

            if !self
                .base
                .report_phase(host, "Checking teardrop clearances...")
            {
                return false; // DRC cancelled
            }

            self.test_teardrop_clearances(engine, host, ctx);
        }

        !host.is_cancelled()
    }

    /// `testGraphicClearances` (work package DRC-08b).
    fn test_graphic_clearances(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        crate::port_pending!(
            "DRC_TEST_PROVIDER_COPPER_CLEARANCE::testGraphicClearances (DRC-08b)",
            engine,
            host,
            ctx
        );
    }

    /// `testZonesToZones` (work package DRC-08b).
    fn test_zones_to_zones(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        crate::port_pending!(
            "DRC_TEST_PROVIDER_COPPER_CLEARANCE::testZonesToZones (DRC-08b)",
            engine,
            host,
            ctx
        );
    }

    /// `testTeardropClearances` (work package DRC-08b).
    fn test_teardrop_clearances(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        crate::port_pending!(
            "DRC_TEST_PROVIDER_COPPER_CLEARANCE::testTeardropClearances (DRC-08b)",
            engine,
            host,
            ctx
        );
    }

    /// Port of `collectZonesNear`: copper zones on `layer` reachable from `item` at the worst
    /// clearance, in zone-list order.
    fn collect_zones_near(&self, item: ItemRef<'_>, layer: PcbLayerId) -> Vec<ZoneId> {
        // Tested before the bounding box, which costs an effective-shape build for arcs
        if !self.zone_index.has_layer(layer) {
            return Vec::new();
        }

        let mut query = item.bounding_box();
        query.inflate(self.caches.max_clearance);

        self.zone_index.query(layer, &query)
    }

    /// Port of `testSingleLayerItemAgainstItem`: checks for track / via / hole clearance of
    /// `item` (shape `item_shape` on `layer`) against `other` (`prepared_other`: the shape
    /// the copper tree holds for it). False if a violation was reported.
    #[allow(clippy::too_many_arguments)]
    fn test_single_layer_item_against_item<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        item_shape: &dyn Shape,
        layer: PcbLayerId,
        other: ItemRef<'x>,
        prepared_other: &dyn Shape,
    ) -> bool {
        let mut item = item;
        let mut other = other;
        let mut item_shape = item_shape;
        let mut test_clearance = !host.is_error_limit_exceeded(DRCE_CLEARANCE);
        let mut test_shorting = !host.is_error_limit_exceeded(DRCE_SHORTING_ITEMS);
        let test_holes = !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE);
        let mut constraint = DrcConstraint::default();
        let mut clearance = -1;
        let mut actual = 0;
        let mut pos = Vector2I::default();
        let mut has_error = false;
        let mut item_net = connected_net(item);
        let mut other_net = connected_net(other);

        if item_net == other_net {
            test_clearance = false;
            test_shorting = false;
        }

        let mut other_shape_shared_ptr: Option<Arc<ShapeSegment>> = None;

        if other.type_() == KicadT::PcbPadT {
            let pad = other.as_pad().expect("PCB_PAD_T is a pad");

            if !pad.flash_layer(ctx, layer) {
                if pad.attribute() == PadAttrib::Npth {
                    test_clearance = false;
                    test_shorting = false;
                }

                other_shape_shared_ptr =
                    other.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint);
            }
        } else if other.type_() == KicadT::PcbViaT {
            let via = other.as_via().expect("PCB_VIA_T is a via");

            if !via.flash_layer(ctx, layer) {
                other_shape_shared_ptr =
                    other.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint);
            }
        }

        // The explicit hole shape, where one was selected above, still outranks the prepared
        // parent
        let mut other_shape: &dyn Shape = match &other_shape_shared_ptr {
            Some(hole) => hole.as_ref(),
            None => prepared_other,
        };

        // Collide (and generate violations) based on a well-defined order so that exclusion
        // checking against previously-generated violations will work.
        if item.uuid() > other.uuid() {
            std::mem::swap(&mut item, &mut other);
            std::mem::swap(&mut item_shape, &mut other_shape);
            std::mem::swap(&mut item_net, &mut other_net);
        }

        if test_clearance || test_shorting {
            constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::ClearanceConstraint,
                Some(item),
                Some(other),
                layer,
            );
            clearance = constraint.value().min();
        }

        if constraint.severity() != Severity::Ignore && clearance > 0 {
            // Special processing for track:track intersections
            if item.type_() == KicadT::PcbTraceT && other.type_() == KicadT::PcbTraceT {
                let track = item.as_track().expect("PCB_TRACE_T is a track").as_track();
                let other_track = other.as_track().expect("PCB_TRACE_T is a track").as_track();

                let track_seg = Seg::new(track.start(), track.end());
                let other_seg = Seg::new(other_track.start(), other_track.end());

                if let Some(intersection) = track_seg.intersect(&other_seg) {
                    let mut drc_item =
                        item::create(DRCE_TRACKS_CROSSING).expect("DRCE_TRACKS_CROSSING item");
                    drc_item.set_item_ids(vec![item.uuid(), other.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_point_geometry(
                        host,
                        Arc::new(drc_item),
                        intersection,
                        intersection,
                        intersection,
                        layer,
                    );
                    return false;
                }
            }

            if item_shape.collide(
                other_shape,
                self.sub_e(clearance),
                Some(&mut actual),
                Some(&mut pos),
            ) {
                if item_net.is_some_and(|n| is_net_tie_exclusion(ctx, n, layer, pos, other))
                    || other_net.is_some_and(|n| is_net_tie_exclusion(ctx, n, layer, pos, item))
                {
                    // Collision occurred as a copper item entered a pad marked as a net-tie.
                    // We allow these regardless of which side DRC happened to test first.
                } else if actual == 0 && other_net.is_some() && test_shorting {
                    let item_net_code = item_net.map_or(0, |n| n.0);
                    let other_net_code = other_net.map_or(0, |n| n.0);

                    let mut drc_item =
                        item::create(DRCE_SHORTING_ITEMS).expect("DRCE_SHORTING_ITEMS item");
                    drc_item.set_error_detail(&nets_detail(
                        item_net_code,
                        item.netname(),
                        other_net_code,
                        other.netname(),
                    ));
                    drc_item.set_item_ids(vec![item.uuid(), other.uuid()]);
                    self.base.report_two_point_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        pos,
                        pos,
                        layer,
                    );
                    has_error = true;

                    if !host.report_all_track_errors() {
                        return false;
                    }
                } else if test_clearance {
                    let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        CLEARANCE_MSG,
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![item.uuid(), other.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_shape_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        item_shape,
                        other_shape,
                        layer,
                        actual,
                    );
                    has_error = true;

                    if !host.report_all_track_errors() {
                        return false;
                    }
                }
            }
        }

        if test_holes && (item.has_hole() || other.has_hole()) {
            let a = [item, other];
            let b = [other, item];
            let b_net = [other_net, item_net];
            let a_shape: [&dyn Shape; 2] = [item_shape, other_shape];

            for ii in 0..2 {
                let hole_shape: Option<Arc<ShapeSegment>> = if b[ii].type_() == KicadT::PcbViaT {
                    if b[ii].layer_set().contains(layer) {
                        b[ii].effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                    } else {
                        continue;
                    }
                } else if b[ii].has_hole() {
                    b[ii].effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                } else {
                    continue;
                };

                // C++ dereferences the hole shape unchecked; an item with a hole always has one
                let Some(hole_shape) = hole_shape else {
                    continue;
                };

                let netcode = b_net[ii].unwrap_or(NetCode(0));

                // Footprint-internal net-tie pads can be left unassigned, so net code 0 must be
                // tested too. For example, a net tie that connects across two layers by a PTH
                // pad that isn't connected to the external nets directly.
                if is_net_tie_exclusion(ctx, netcode, layer, hole_shape.centre(), a[ii]) {
                    continue;
                }

                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::HoleClearanceConstraint,
                    Some(b[ii]),
                    Some(a[ii]),
                    layer,
                );
                clearance = constraint.value().min();

                // Test for hole to item clearance even if clearance is 0, because the item
                // cannot be inside (or intersect) the hole.
                if constraint.severity() != Severity::Ignore
                    && a_shape[ii].collide(
                        hole_shape.as_ref(),
                        self.sub_e(clearance),
                        Some(&mut actual),
                        Some(&mut pos),
                    )
                {
                    let mut drc_item =
                        item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        if clearance != 0 {
                            CLEARANCE_MSG
                        } else {
                            "(%s clearance %s; actual < 0)"
                        },
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![a[ii].uuid(), b[ii].uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_shape_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        a_shape[ii],
                        hole_shape.as_ref(),
                        layer,
                        actual,
                    );
                    return false;
                }
            }
        }

        !has_error
    }

    /// Port of `testItemAgainstZone`: clearance and hole clearance of `item` against the fill
    /// of `zone` on `layer`.
    fn test_item_against_zone<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) {
        let zone_ref = ctx
            .item(ItemId::from(zone_id))
            .expect("m_DRCCopperZonesByLayer holds board zones");
        let zone_net_code = zone_ref.net_code().map_or(0, |n| n.0);

        if !zone_ref.layer_set().contains(layer) {
            return;
        }

        if zone_net_code != 0
            && item.is_connected()
            && Some(NetCode(zone_net_code)) == item.net_code()
        {
            return;
        }

        let item_bbox = item.bounding_box();
        let mut worst_case_bbox = item_bbox;

        worst_case_bbox.inflate(self.caches.max_clearance);

        if !worst_case_bbox.intersects(&zone_ref.bounding_box()) {
            return;
        }

        // Ignore graphic items which implement a net-tie to the zone's net on the layer being
        // tested.  (dynamic_cast<PCB_SHAPE*>: shapes, text boxes and table cells.)
        if let Some(parent_fp_ref) = item.parent_footprint()
            && let Some(parent_fp) = parent_fp_ref.as_footprint()
            && parent_fp.is_net_tie()
            && matches!(
                item.type_(),
                KicadT::PcbShapeT | KicadT::PcbTextboxT | KicadT::PcbTablecellT
            )
            && let Some(fp_id) = parent_fp_ref.id().and_then(|id| id.typed())
        {
            let mut allowed_net_tie_pads: BTreeSet<PadId> = BTreeSet::new();

            for (pad_id, _) in ctx.board.footprint_pads(fp_id) {
                let pad_ref = ctx.item(ItemId::from(pad_id)).expect("footprint pad");

                if pad_ref.net_code().map_or(0, |n| n.0) == zone_net_code && zone_net_code != 0 {
                    if pad_ref.is_on_layer(layer) {
                        allowed_net_tie_pads.insert(pad_id);
                    }

                    for other in parent_fp.net_tie_pads(ctx, pad_id) {
                        if ctx
                            .item(ItemId::from(other))
                            .is_some_and(|o| o.is_on_layer(layer))
                        {
                            allowed_net_tie_pads.insert(other);
                        }
                    }
                }
            }

            if !allowed_net_tie_pads.is_empty() {
                let item_shape = item.effective_shape(
                    PcbLayerId::UndefinedLayer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );

                for pad_id in allowed_net_tie_pads {
                    let pad_ref = ctx.item(ItemId::from(pad_id)).expect("footprint pad");
                    let pad = pad_ref.as_pad().expect("a pad");

                    if pad.bounding_box_layer(ctx, layer).intersects(&item_bbox)
                        && pad_ref
                            .effective_shape(
                                layer,
                                Flashing::Default,
                                DrcConstraintT::NullConstraint,
                            )
                            .collide(item_shape.as_ref(), 0, None, None)
                    {
                        return;
                    }
                }
            }
        }

        let mut test_clearance = !host.is_error_limit_exceeded(DRCE_CLEARANCE);
        let test_holes = !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE);

        if !test_clearance && !test_holes {
            return;
        }

        let Some(zone_tree) = self.caches.copper_zone_rtree(zone_id) else {
            return;
        };

        let mut constraint = DrcConstraint::default();
        let mut clearance = -1;

        if item.type_() == KicadT::PcbPadT {
            let pad = item.as_pad().expect("PCB_PAD_T is a pad");
            let flashed_pad = pad.flash_layer(ctx, layer);
            let plated_hole = item.has_hole() && pad.attribute() == PadAttrib::Pth;

            if !flashed_pad && !plated_hole {
                test_clearance = false;
            }
        }

        if test_clearance {
            constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::ClearanceConstraint,
                Some(item),
                Some(zone_ref),
                layer,
            );
            clearance = constraint.value().min();
        }

        if constraint.severity() != Severity::Ignore && clearance > 0 {
            let item_shape =
                item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

            if let Some((actual, pos)) = zone_tree.query_colliding_box_actual(
                &item_bbox,
                item_shape.as_ref(),
                layer,
                self.sub_e(clearance),
            ) {
                let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                drc_item.set_error_detail(&self.base.format_msg(
                    CLEARANCE_MSG,
                    &constraint.name(),
                    clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![item.uuid(), zone_ref.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();
                self.base.report_two_item_geometry(
                    host,
                    Arc::new(drc_item),
                    pos,
                    item,
                    zone_ref,
                    layer,
                    actual,
                );
            }
        }

        if test_holes && item.has_hole() {
            let hole_shape: Option<Arc<ShapeSegment>> = if item.type_() == KicadT::PcbViaT {
                if item.layer_set().contains(layer) {
                    item.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                } else {
                    None
                }
            } else {
                item.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
            };

            if let Some(hole_shape) = hole_shape {
                constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::HoleClearanceConstraint,
                    Some(item),
                    Some(zone_ref),
                    layer,
                );
                clearance = constraint.value().min();

                if constraint.severity() != Severity::Ignore
                    && clearance > 0
                    && let Some((actual, pos)) = zone_tree.query_colliding_box_actual(
                        &item_bbox,
                        hole_shape.as_ref(),
                        layer,
                        self.sub_e(clearance),
                    )
                {
                    let mut drc_item =
                        item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        CLEARANCE_MSG,
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![item.uuid(), zone_ref.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();

                    let zone_shape = zone_ref.effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );
                    self.base.report_two_shape_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        hole_shape.as_ref(),
                        zone_shape.as_ref(),
                        layer,
                        actual,
                    );
                }
            }
        }
    }

    /// Port of `testTrackClearances`: every track / arc / via against the copper item tree
    /// and the zones near, per board copper layer of the track.
    fn test_track_clearances(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        let board = ctx.board;
        // std::map<BOARD_ITEM*, int> (lookups only); claimed in track order (drc.md 4.12)
        let mut free_pads_usage_map: BTreeMap<ItemId, Option<NetCode>> = BTreeMap::new();
        let count = board.tracks().count();

        report_aux(host.0, &format!("Testing {count} tracks & vias..."));

        let board_copper_layers = Lset::all_cu_mask_n(board.copper_layer_count());
        let tree = self.caches.copper_item_rtree();
        let track_ids: Vec<ItemId> = board.tracks().map(|(id, _)| ItemId::from(id)).collect();

        for track_id in track_ids {
            let track = ctx.item(track_id).expect("board track");

            for layer in (&track.layer_set() & &board_copper_layers).iter() {
                let track_shape =
                    track.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

                // Filter:
                let mut filter = |other_id: ItemId| -> bool {
                    let other = ctx
                        .item(other_id)
                        .expect("DRC_RTREE entries are board items");

                    if other.is_connected() && other.net_code() == track.net_code() {
                        return false;
                    }

                    // Dedup on UUID, not heap address, so that exclusion checking against
                    // previously-generated violations will work
                    let other_type = other.type_();

                    if (other_type == KicadT::PcbTraceT
                        || other_type == KicadT::PcbArcT
                        || other_type == KicadT::PcbViaT)
                        && track.uuid() > other.uuid()
                    {
                        return false;
                    }

                    true
                };

                // Visitor:
                let mut visitor = |other_id: ItemId, prepared_other: &SharedShape| -> bool {
                    if host.is_cancelled() {
                        return false;
                    }

                    let other = ctx
                        .item(other_id)
                        .expect("DRC_RTREE entries are board items");

                    if other.type_() == KicadT::PcbPadT
                        && other.as_pad().is_some_and(|p| p.is_free_pad(ctx))
                        && prepared_other.collide(track_shape.as_ref(), 0, None, None)
                    {
                        match free_pads_usage_map.get(&other_id) {
                            None => {
                                free_pads_usage_map.insert(other_id, track.net_code());
                                return true; // Continue colliding tests
                            }
                            Some(&net) if net == track.net_code() => {
                                return true; // Continue colliding tests
                            }
                            Some(_) => {}
                        }
                    }

                    if !self.test_single_layer_item_against_item(
                        engine,
                        host,
                        ctx,
                        track,
                        track_shape.as_ref(),
                        layer,
                        other,
                        prepared_other.as_ref(),
                    ) && !host.report_all_track_errors()
                    {
                        return false;
                    }

                    !host.is_cancelled()
                };

                tree.query_colliding_prepared_copper(
                    track,
                    &track_shape,
                    layer,
                    Some(&mut filter),
                    Some(&mut visitor),
                    self.caches.max_clearance,
                    true,
                );

                for zone in self.collect_zones_near(track, layer) {
                    self.test_item_against_zone(engine, host, ctx, track, zone, layer);

                    if host.is_cancelled() {
                        break;
                    }
                }
            }
        }
    }

    /// Port of `testPadAgainstItem`: clearance, shorting and hole clearance of `pad` (shape
    /// `pad_shape` on `layer`) against `other` (`prepared_other`: its tree shape).
    #[allow(clippy::too_many_arguments)]
    fn test_pad_against_item<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
        pad_ref: ItemRef<'x>,
        pad_shape: &SharedShape,
        layer: PcbLayerId,
        other: ItemRef<'x>,
        prepared_other: &dyn Shape,
    ) -> bool {
        let pad = pad_ref.as_pad().expect("a pad");
        let mut test_clearance = !host.is_error_limit_exceeded(DRCE_CLEARANCE);
        let mut test_shorting = !host.is_error_limit_exceeded(DRCE_SHORTING_ITEMS);
        let mut test_holes = !host.is_error_limit_exceeded(DRCE_HOLE_CLEARANCE);

        // Disable some tests for net-tie objects in a footprint
        let pad_parent = pad_ref.parent().and_then(|p| p.id());

        if pad_parent.is_some()
            && other.parent().and_then(|p| p.id()) == pad_parent
            && let Some(fp) = pad_ref.parent_footprint().and_then(|f| f.as_footprint())
        {
            let pad_to_net_tie_group_map = fp.map_pad_numbers_to_net_tie_groups(ctx);
            // std::map::operator[] inserts 0 for a number that is not in the map
            let group_of =
                |number: &str| pad_to_net_tie_group_map.get(number).copied().unwrap_or(0);
            let pad_group_idx = group_of(pad.number());

            if other.type_() == KicadT::PcbPadT {
                let other_pad = other.as_pad().expect("PCB_PAD_T is a pad");

                if pad_group_idx >= 0 && pad_group_idx == group_of(other_pad.number()) {
                    test_clearance = false;
                    test_shorting = false;
                }

                if pad.same_logical_pad_as(other_pad) {
                    test_holes = false;
                }
            }

            if other.type_() == KicadT::PcbShapeT && pad_group_idx >= 0 {
                test_clearance = false;
                test_shorting = false;
            }
        }

        let other_is_connected = other.is_connected();
        let other_pad = if other.type_() == KicadT::PcbPadT {
            other.as_pad()
        } else {
            None
        };
        let other_via = if other.type_() == KicadT::PcbViaT {
            other.as_via()
        } else {
            None
        };

        if !is_copper_layer(layer) {
            test_clearance = false;
            test_shorting = false;
        }

        // A NPTH has no cylinder, but it may still have pads on some layers
        if pad.attribute() == PadAttrib::Npth && !pad.flash_layer(ctx, layer) {
            test_clearance = false;
            test_shorting = false;
        }

        if let Some(op) = other_pad
            && op.attribute() == PadAttrib::Npth
            && !op.flash_layer(ctx, layer)
        {
            test_clearance = false;
            test_shorting = false;
        }

        // Track clearances are tested in testTrackClearances()
        if matches!(
            other.type_(),
            KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT
        ) {
            test_clearance = false;
            test_shorting = false;
        }

        // Graphic clearances are tested in testGraphicClearances()
        if matches!(other.type_(), KicadT::PcbShapeT | KicadT::PcbTextboxT) {
            test_clearance = false;
            test_shorting = false;
        }

        let pad_net = pad_ref.net_code().map_or(0, |n| n.0);
        let other_net = if other_is_connected {
            other.net_code().map_or(0, |n| n.0)
        } else {
            0
        };

        // Other objects of the same (defined) net get a waiver on clearance and hole tests
        if other_net != 0 && other_net == pad_net {
            test_clearance = false;
            test_shorting = false;
            test_holes = false;
        }

        if !(pad.drill_size(ctx).x > 0)
            && !other_pad.is_some_and(|op| op.drill_size(ctx).x > 0)
            && !other_via.is_some_and(|ov| ov.drill() > 0)
        {
            test_holes = false;
        }

        if !test_clearance && !test_shorting && !test_holes {
            return true;
        }

        let other_shape = prepared_other;
        let mut constraint = DrcConstraint::default();
        let mut clearance;
        let mut actual = 0;
        let mut pos = Vector2I::default();
        let mut has_error = false;

        if let Some(op) = other_pad
            && pad.same_logical_pad_as(op)
        {
            // If pads are equivalent (ie: from the same footprint with the same pad number)...
            // ... and have "real" nets...
            // then they must be the same net
            if test_shorting {
                if pad_net == 0 || Some(NetCode(pad_net)) == other.net_code() {
                    return true;
                }

                if kicad_pcb::board_connected_item::short_netname(pad, ctx)
                    .starts_with("unconnected-(")
                    && kicad_pcb::board_connected_item::short_netname(op, ctx)
                        .starts_with("unconnected-(")
                {
                    return true;
                }

                let mut drc_item =
                    item::create(DRCE_SHORTING_ITEMS).expect("DRCE_SHORTING_ITEMS item");
                drc_item.set_error_detail(&nets_detail(
                    pad_net,
                    pad_ref.netname(),
                    other_net,
                    other.netname(),
                ));
                drc_item.set_item_ids(vec![pad_ref.uuid(), other.uuid()]);
                self.base.report_violation(
                    host,
                    Arc::new(drc_item),
                    other.position(),
                    layer.0,
                    &no_path,
                );
                has_error = true;
            }

            return !has_error;
        }

        if test_clearance || test_shorting {
            constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::ClearanceConstraint,
                Some(pad_ref),
                Some(other),
                layer,
            );
            clearance = constraint.value().min();

            if constraint.severity() != Severity::Ignore
                && clearance > 0
                && pad_shape.collide(
                    other_shape,
                    self.sub_e(clearance),
                    Some(&mut actual),
                    Some(&mut pos),
                )
            {
                if pad_ref
                    .net_code()
                    .is_some_and(|n| is_net_tie_exclusion(ctx, n, layer, pos, other))
                {
                    // Pads connected to pads of a net-tie footprint are allowed to collide
                    // with the net-tie footprint's graphics.
                } else if actual == 0 && pad_net != 0 && other_net != 0 && test_shorting {
                    let mut drc_item =
                        item::create(DRCE_SHORTING_ITEMS).expect("DRCE_SHORTING_ITEMS item");
                    drc_item.set_error_detail(&nets_detail(
                        pad_net,
                        pad_ref.netname(),
                        other_net,
                        other.netname(),
                    ));
                    drc_item.set_item_ids(vec![pad_ref.uuid(), other.uuid()]);
                    self.base.report_two_point_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        pos,
                        pos,
                        layer,
                    );
                    has_error = true;
                    test_holes = false; // No need for multiple violations
                } else if test_clearance {
                    let mut drc_item = item::create(DRCE_CLEARANCE).expect("DRCE_CLEARANCE item");
                    drc_item.set_error_detail(&self.base.format_msg(
                        CLEARANCE_MSG,
                        &constraint.name(),
                        clearance as f64,
                        actual as f64,
                        EdaDataType::Distance,
                    ));
                    drc_item.set_item_ids(vec![pad_ref.uuid(), other.uuid()]);
                    drc_item.violating_rule = constraint.parent_rule.clone();
                    self.base.report_two_item_geometry(
                        host,
                        Arc::new(drc_item),
                        pos,
                        pad_ref,
                        other,
                        layer,
                        actual,
                    );
                    has_error = true;
                    test_holes = false; // No need for multiple violations
                }
            }
        }

        // The doTestHole lambda: true when it reported a violation.
        let do_test_hole = |host: &mut RunHost<'_, '_>,
                            constraint: &DrcConstraint,
                            item: ItemRef<'x>,
                            shape: &dyn Shape,
                            other_item: ItemRef<'x>,
                            other_hole: &ShapeSegment,
                            clearance: i32|
         -> bool {
            let mut actual = 0;
            let mut pos = Vector2I::default();

            if shape.collide(
                other_hole,
                self.sub_e(clearance),
                Some(&mut actual),
                Some(&mut pos),
            ) {
                let mut drc_item =
                    item::create(DRCE_HOLE_CLEARANCE).expect("DRCE_HOLE_CLEARANCE item");
                drc_item.set_error_detail(&self.base.format_msg(
                    CLEARANCE_MSG,
                    &constraint.name(),
                    clearance as f64,
                    actual as f64,
                    EdaDataType::Distance,
                ));
                drc_item.set_item_ids(vec![item.uuid(), other_item.uuid()]);
                drc_item.violating_rule = constraint.parent_rule.clone();
                self.base.report_two_shape_geometry(
                    host,
                    Arc::new(drc_item),
                    pos,
                    shape,
                    other_hole,
                    layer,
                    actual,
                );
                return true;
            }

            false
        };

        if test_holes {
            constraint = engine.eval_rules(
                ctx,
                DrcConstraintT::HoleClearanceConstraint,
                Some(pad_ref),
                Some(other),
                layer,
            );

            if constraint.severity() == Severity::Ignore {
                test_holes = false;
            }
        }

        if test_holes && other_pad.is_some() && other.has_hole() && pad.flash_layer(ctx, layer) {
            clearance = constraint.value().min();

            if clearance > 0
                && let Some(hole) =
                    other.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                && do_test_hole(
                    host,
                    &constraint,
                    pad_ref,
                    pad_shape.as_ref(),
                    other,
                    &hole,
                    clearance,
                )
            {
                has_error = true;
                test_holes = false; // No need for multiple violations
            }
        }

        // Pad pairs are deduplicated by UUID order in testPadClearances.
        // Run the swapped direction so we don't miss any violations.
        if test_holes
            && pad_ref.has_hole()
            && other_pad.is_some_and(|op| op.flash_layer(ctx, layer))
        {
            clearance = constraint.value().min();

            if clearance > 0
                && let Some(hole) =
                    pad_ref.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                && do_test_hole(
                    host,
                    &constraint,
                    other,
                    other_shape,
                    pad_ref,
                    &hole,
                    clearance,
                )
            {
                has_error = true;
                test_holes = false; // No need for multiple violations
            }
        }

        if test_holes && other_via.is_some() && other.has_hole() {
            clearance = constraint.value().min();

            if !other.is_on_layer(layer) {
                clearance = 0;
            }

            if clearance > 0
                && let Some(hole) =
                    other.effective_hole_shape(layer, DrcConstraintT::HoleClearanceConstraint)
                && do_test_hole(
                    host,
                    &constraint,
                    pad_ref,
                    pad_shape.as_ref(),
                    other,
                    &hole,
                    clearance,
                )
            {
                has_error = true;
            }
        }

        !has_error
    }

    /// Port of `testPadClearances`: every pad (footprint order, pad order) against the copper
    /// item tree and the zones near, per board copper layer of the pad.
    fn test_pad_clearances(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
    ) {
        let board = ctx.board;
        let board_copper_layers = Lset::all_cu_mask_n(board.copper_layer_count());
        let tree = self.caches.copper_item_rtree();

        for (fp_id, _) in board.footprints() {
            for (pad_id, _) in board.footprint_pads(fp_id) {
                let pad = ctx.item(ItemId::from(pad_id)).expect("footprint pad");

                // Through-hole pads are tested per-layer, so overlapping TH pads may produce
                // one violation per shared copper layer. This is intentional for parallelized
                // DRC to avoid cross-layer shared state.
                for layer in (&pad.layer_set() & &board_copper_layers).iter() {
                    if host.is_cancelled() {
                        return;
                    }

                    let pad_shape = pad.effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );

                    // Filter:
                    let mut filter = |other_id: ItemId| -> bool {
                        let other = ctx
                            .item(other_id)
                            .expect("DRC_RTREE entries are board items");

                        // Dedup on UUID, not heap address, so that exclusion checking against
                        // previously-generated violations will work
                        !(other.type_() == KicadT::PcbPadT && pad.uuid() > other.uuid())
                    };

                    // Visitor
                    let mut visitor = |other_id: ItemId, prepared_other: &SharedShape| -> bool {
                        let other = ctx
                            .item(other_id)
                            .expect("DRC_RTREE entries are board items");

                        self.test_pad_against_item(
                            engine,
                            host,
                            ctx,
                            pad,
                            &pad_shape,
                            layer,
                            other,
                            prepared_other.as_ref(),
                        );

                        !host.is_cancelled()
                    };

                    tree.query_colliding_prepared_copper(
                        pad,
                        &pad_shape,
                        layer,
                        Some(&mut filter),
                        Some(&mut visitor),
                        self.caches.max_clearance,
                        true,
                    );

                    for zone in self.collect_zones_near(pad, layer) {
                        self.test_item_against_zone(engine, host, ctx, pad, zone, layer);

                        if host.is_cancelled() {
                            return;
                        }
                    }
                }
            }
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderCopperClearance<'_> {
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
