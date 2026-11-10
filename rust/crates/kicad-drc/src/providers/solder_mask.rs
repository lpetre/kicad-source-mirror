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

//! Port of `pcbnew/drc/drc_test_provider_solder_mask.cpp`.
//!
//! Solder mask tests. Checks for silkscreen which is clipped by mask openings and for bridges
//! between mask apertures with different nets.
//! Errors generated:
//! - `DRCE_SILK_MASK_CLEARANCE`
//! - `DRCE_SOLDERMASK_BRIDGE`
//!
//! The C++ runs the bridge tests on the thread pool; the port runs them in item order (one of
//! the valid C++ interleavings). The C++ already collects the bridges and reports them sorted
//! (layer, item UUIDs, position), so the reported set and order do not depend on scheduling.
//! The board needs the per-run caches of the cache generator ([`DrcBoardCaches`]: copper zones
//! and their trees), which the provider borrows.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{Flashing, PcbLayerId, is_front_layer};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_core::{Kiid, NILUUID};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeType};
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::stdsort::{std_pop_heap, std_push_heap, std_sort, std_sort_heap};
use kicad_pcb::drc_api::{DRCE_SILK_MASK_CLEARANCE, DRCE_SOLDERMASK_BRIDGE, DrcConstraint};
use kicad_pcb::eda_shape::shapes::EdaShapeShapesExt;
use kicad_pcb::item::SharedShape;
use kicad_pcb::pad::Pad;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardConnectedItem, BoardCtx, ItemId, ItemRef, NetCode};

use super::run_host::{RunHost, report_aux};
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::rtree::DrcRtree;
use crate::rule::{DrcConstraintExt, DrcRuleInfo};
use crate::test_provider::{
    DrcTestProviderBase, all_basic_items, all_basic_items_but_zones, for_each_geometry_item,
    is_invisible_text, no_path,
};

/// Port of the static `addItemPolysWithEndings`: a `PCB_SHAPE` with its line endings, any other
/// item through `TransformShapeToPolygon`.
fn add_item_polys_with_endings(
    item: ItemRef<'_>,
    buffer: &mut ShapePolySet,
    layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
) {
    if item.type_() == KicadT::PcbShapeT {
        let shape = item.as_shape().expect("PCB_SHAPE_T is a PCB_SHAPE");
        shape.transform_with_line_endings_to_polygon(
            item.ctx, buffer, clearance, error, error_loc, false,
        );
    } else {
        item.transform_shape_to_polygon(buffer, layer, clearance, error, error_loc, false);
    }
}

/// Port of `isNPTHPadWithNoCopper`.
fn is_npth_pad_with_no_copper(item: ItemRef<'_>) -> bool {
    if item.type_() == KicadT::PcbPadT {
        return item
            .as_pad()
            .expect("PCB_PAD_T is a PAD")
            .is_npth_with_no_copper(item.ctx);
    }

    false
}

/// Port of `isMaskAperture`.
///
/// Simple mask apertures aren't associated with copper items, so they only constitute a bridge
/// when they expose other copper items having at least two distinct nets.
///
/// Note that this algorithm is also used for free pads.
fn is_mask_aperture(item: ItemRef<'_>) -> bool {
    if item.type_() == KicadT::PcbPadT
        && item
            .as_pad()
            .expect("PCB_PAD_T is a PAD")
            .is_free_pad_ctx(item.ctx)
    {
        return true;
    }

    let saved = Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]);

    let mask_layers = &item.layer_set() & &saved;
    let copper_layers = &(&item.layer_set() & &!saved) & Lset::all_cu_mask();

    mask_layers.count() > 0 && copper_layers.count() == 0
}

/// `BOARD_CONNECTED_ITEM::GetNetCode()` of a connected item, -1 otherwise (the
/// `if( aItem->IsConnected() )` pattern of the provider).
fn net_of(item: ItemRef<'_>) -> i32 {
    if item.is_connected() {
        item.net_code().map_or(-1, |n| n.0)
    } else {
        -1
    }
}

/// `PCB_LAYER_ID` of the board mask polygon slot: F_Mask 0, B_Mask 1.
fn mask_slot(layer: PcbLayerId) -> usize {
    if layer == PcbLayerId::FMask { 0 } else { 1 }
}

/// `MASK_APERTURE_ITEM`: an (item, net) exposed by a mask aperture.
#[derive(Clone, Copy, Debug)]
struct MaskApertureItem {
    item: ItemId,
    net: i32,
    pos: Vector2I,
}

/// `PENDING_BRIDGE`: a bridge collected during the tests, reported in a deterministic order.
#[derive(Clone, Copy, Debug)]
struct PendingBridge {
    a: ItemId,
    b: ItemId,
    c: Option<ItemId>,
    pos: Vector2I,
    layer: PcbLayerId,
    ids: [Kiid; 3],
}

impl PendingBridge {
    /// `PENDING_BRIDGE::operator<`: layer, then the sorted UUIDs, then the position.
    fn less(&self, rhs: &PendingBridge) -> bool {
        if self.layer != rhs.layer {
            return self.layer.0 < rhs.layer.0;
        }

        for ii in 0..self.ids.len() {
            if self.ids[ii] != rhs.ids[ii] {
                return self.ids[ii] < rhs.ids[ii];
            }
        }

        if self.pos.x != rhs.pos.x {
            return self.pos.x < rhs.pos.x;
        }

        self.pos.y < rhs.pos.y
    }
}

/// `m_fullSolderMaskRTree`: a `DRC_RTREE` holding only the `m_SolderMaskBridges` zone on F_Mask
/// and B_Mask.
///
/// The scratch zone is not a board item (no handle), which [`DrcRtree::insert`] cannot index,
/// so its two `Insert` calls and the one query the provider makes
/// (`QueryColliding( aBox, aRefShape, aLayer, aClearance, &actual, &pos )`) are reproduced here
/// on the same packed tree type with the same entry boxes and visit order.
#[derive(Default)]
struct FullSolderMaskTree {
    /// Per layer (`m_tree`): the tree (payload = index into `entries`), the indexed shape and
    /// the entries (sub-shape index of the shape, `None` for the whole shape).
    layers: Vec<(
        PcbLayerId,
        PackedRTree<u32, 2, 16>,
        SharedShape,
        Vec<Option<u32>>,
    )>,
}

impl FullSolderMaskTree {
    /// `Insert( solderMask, aLayer, NULL_CONSTRAINT )` (worst clearance 0) followed by the
    /// `Build()` of that layer: the zone's effective shape on `layer` is its fill.
    fn insert_and_build(&mut self, layer: PcbLayerId, shape: SharedShape) {
        let mut builder: PackedRTreeBuilder<u32, 2, 16> = PackedRTreeBuilder::new();
        let mut entries = Vec::new();

        let subshapes: Vec<Option<u32>> = if shape.has_indexable_subshapes() {
            (0..shape.indexable_subshape_count() as u32)
                .map(Some)
                .collect()
        } else {
            vec![None]
        };

        for sub in subshapes {
            let subshape: &dyn Shape = match sub {
                None => shape.as_ref(),
                Some(i) => shape
                    .indexable_subshape(i as usize)
                    .expect("indexable sub-shape"),
            };

            if subshape.type_() == ShapeType::Null {
                continue;
            }

            let mut bbox = subshape.bbox(0);
            bbox.inflate(0);

            let mmin = [bbox.x(), bbox.y()];
            let mmax = [bbox.right(), bbox.bottom()];

            builder.add(&mmin, &mmax, entries.len() as u32);
            entries.push(sub);
        }

        self.layers.push((layer, builder.build(), shape, entries));
    }

    /// `DRC_RTREE::QueryColliding( aBox, aRefShape, aLayer, aClearance, aActual, aPos )`: the
    /// minimal actual distance over all colliding entries and its position (the first minimum
    /// in search order wins), clamped at 0; `None` when nothing collides. Stops at the first
    /// true collision. Same body as [`DrcRtree::query_colliding_box_actual`].
    fn query_colliding(
        &self,
        a_box: &Box2I,
        ref_shape: &dyn Shape,
        layer: PcbLayerId,
        clearance: i32,
    ) -> Option<(i32, Vector2I)> {
        let mut bbox = *a_box;
        bbox.inflate(clearance);

        let min = [bbox.x(), bbox.y()];
        let max = [bbox.right(), bbox.bottom()];

        let mut collision = false;
        let mut actual = i32::MAX;
        let mut pos = Vector2I::default();

        if let Some((_, tree, shape, entries)) = self.layers.iter().find(|l| l.0 == layer) {
            tree.search(&min, &max, |&i: &u32| {
                let subshape: &dyn Shape = match entries[i as usize] {
                    None => shape.as_ref(),
                    Some(s) => shape
                        .indexable_subshape(s as usize)
                        .expect("indexable sub-shape"),
                };

                let mut cur_actual = 0;
                let mut cur_pos = Vector2I::default();

                if ref_shape.collide(
                    subshape,
                    clearance,
                    Some(&mut cur_actual),
                    Some(&mut cur_pos),
                ) {
                    collision = true;

                    if cur_actual < actual {
                        actual = cur_actual;
                        pos = cur_pos;
                    }

                    // Stop looking after we have a true collision
                    if actual <= 0 {
                        return false;
                    }
                }

                true
            });
        }

        if collision {
            Some((0.max(actual), pos))
        } else {
            None
        }
    }
}

/// Port of `DRC_TEST_PROVIDER_SOLDER_MASK`.
///
/// `'c` is the lifetime of the borrowed per-run board caches (`BOARD::m_DRCCopperZones`,
/// `GetCopperZoneRTree`).
pub struct DrcTestProviderSolderMask<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches built by the cache generator for this run.
    caches: &'c DrcBoardCaches,
    /// `m_bridgeRule`
    bridge_rule: DrcRuleInfo,
    /// `m_webWidth`
    web_width: i32,
    /// `m_maxError`
    max_error: i32,
    /// `m_largestClearance`
    largest_clearance: i32,
    /// The fills of `m_SolderMaskBridges` on F_Mask and B_Mask (the whole-board mask). See
    /// [`DrcTestProviderSolderMask::solder_mask`].
    solder_mask: [ShapePolySet; 2],
    /// `m_fullSolderMaskRTree`
    full_solder_mask_rtree: FullSolderMaskTree,
    /// `m_itemTree`
    item_tree: DrcRtree,
    /// `m_checkedPairs` (canonical order by handle instead of address: dedup only).
    checked_pairs: HashMap<(ItemId, ItemId), Lset>,
    /// `m_maskApertureItems` (keys are sorted before use).
    mask_aperture_items: HashMap<(ItemId, PcbLayerId), Vec<MaskApertureItem>>,
    /// `m_bridgeViolations`: a bounded max-heap under [`PendingBridge::less`].
    bridge_violations: Vec<PendingBridge>,
    /// `m_bridgeLimit`
    bridge_limit: i32,
}

impl<'c> DrcTestProviderSolderMask<'c> {
    /// `DRC_TEST_PROVIDER_SOLDER_MASK()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        let bridge_rule = DrcRuleInfo {
            name: "board setup solder mask min width".to_string(),
            ..DrcRuleInfo::default()
        };

        DrcTestProviderSolderMask {
            base: DrcTestProviderBase::new("solder_mask_issues"),
            caches,
            bridge_rule,
            web_width: 0,
            max_error: 0,
            largest_clearance: 0,
            solder_mask: [ShapePolySet::new(), ShapePolySet::new()],
            full_solder_mask_rtree: FullSolderMaskTree::default(),
            item_tree: DrcRtree::new(),
            checked_pairs: HashMap::new(),
            mask_aperture_items: HashMap::new(),
            bridge_violations: Vec::new(),
            bridge_limit: 0,
        }
    }

    /// The whole-board solder mask of the last run on F_Mask or B_Mask (the fill the C++ leaves
    /// in `BOARD::m_SolderMaskBridges` for display), triangulated.
    pub fn solder_mask(&self, layer: PcbLayerId) -> &ShapePolySet {
        &self.solder_mask[mask_slot(layer)]
    }

    /// Port of `addItemToRTrees`.
    fn add_item_to_rtrees(&mut self, item: ItemRef<'_>) {
        let ctx = item.ctx;

        // Rule areas are purely logical: no copper, no mask, no silk.  Skip them entirely
        // so they cannot contribute to solder-mask bridge or silk-to-mask collisions.
        if item.type_() == KicadT::PcbZoneT && item.as_zone().is_some_and(|z| z.is_rule_area()) {
            return;
        }

        for layer in [PcbLayerId::FMask, PcbLayerId::BMask] {
            if !item.is_on_layer(layer) {
                continue;
            }

            let solder_mask = &mut self.solder_mask[mask_slot(layer)];

            if item.type_() == KicadT::PcbZoneT {
                let zone = item.as_zone().expect("PCB_ZONE_T is a ZONE");

                // GetFilledPolysList( layer ): a zone on a layer always has a fill entry
                // (ZONE::SetLayerSet creates them).
                if let Some(fill) = zone.get_fill(layer) {
                    solder_mask.boolean_add(fill);
                } else {
                    debug_assert!(false, "zone on a mask layer without a fill entry");
                }
            } else {
                let mut clearance = self.web_width / 2;

                match item.any() {
                    AnyItem::Pad(pad) if item.type_() == KicadT::PcbPadT => {
                        clearance += pad.solder_mask_expansion(ctx, layer);
                    }
                    AnyItem::Track(Track::Via(via)) => {
                        clearance += via.solder_mask_expansion(ctx);
                    }
                    AnyItem::Track(Track::Trace(track)) => {
                        clearance += track.solder_mask_expansion(ctx);
                    }
                    AnyItem::Shape(shape) if item.type_() == KicadT::PcbShapeT => {
                        clearance += shape.solder_mask_expansion(ctx);
                    }
                    _ => {}
                }

                match item.any() {
                    AnyItem::Field(field) if item.type_() == KicadT::PcbFieldT => {
                        field.transform_text_to_poly_set(
                            ctx,
                            solder_mask,
                            clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                        );
                    }
                    AnyItem::Text(text) if item.type_() == KicadT::PcbTextT => {
                        text.transform_text_to_poly_set(
                            ctx,
                            solder_mask,
                            clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                        );
                    }
                    _ => {
                        add_item_polys_with_endings(
                            item,
                            solder_mask,
                            layer,
                            clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                        );
                    }
                }

                self.item_tree.insert(
                    item,
                    layer,
                    DrcConstraintT::NullConstraint,
                    self.largest_clearance,
                    false,
                );
            }
        }
    }

    /// Port of `buildRTrees`.
    fn build_rtrees<'x>(&mut self, host: &mut RunHost<'_, '_>, ctx: &'x BoardCtx<'x>) {
        let base = self.base.clone();
        let layers = Lset::from_layers(&[
            PcbLayerId::FMask,
            PcbLayerId::BMask,
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        ]);

        let progress_delta = 500;
        let mut count = 0;
        let mut ii = 0;

        self.solder_mask[0].remove_all_contours();
        self.solder_mask[1].remove_all_contours();

        self.full_solder_mask_rtree = FullSolderMaskTree::default();
        self.item_tree = DrcRtree::new();

        for_each_geometry_item(ctx, all_basic_items(), &layers, |_item| {
            count += 1;
            true
        });

        for_each_geometry_item(ctx, all_basic_items(), &layers, |item| {
            let i = ii;
            ii += 1;

            if !base.report_progress(host, i, count, progress_delta) {
                return false;
            }

            self.add_item_to_rtrees(item);
            true
        });

        self.solder_mask[0].simplify();
        self.solder_mask[1].simplify();

        if self.web_width > 0 {
            self.solder_mask[0].deflate(
                self.web_width / 2,
                CornerStrategy::ChamferAllCorners,
                self.max_error,
            );
            self.solder_mask[1].deflate(
                self.web_width / 2,
                CornerStrategy::ChamferAllCorners,
                self.max_error,
            );
        }

        // solderMask->SetFillFlag( F_Mask / B_Mask, true ), SetIsFilled( true ): display state
        // of the scratch zone (see PORT_STATUS.md).

        // solderMask->CacheTriangulation()
        self.solder_mask[0].cache_triangulation(false);
        self.solder_mask[1].cache_triangulation(false);

        let front: SharedShape = Arc::new(self.solder_mask[0].clone());
        let back: SharedShape = Arc::new(self.solder_mask[1].clone());

        self.full_solder_mask_rtree
            .insert_and_build(PcbLayerId::FMask, front);
        self.full_solder_mask_rtree
            .insert_and_build(PcbLayerId::BMask, back);

        self.item_tree.build();

        self.checked_pairs.clear();
    }

    /// Port of `testSilkToMaskClearance`.
    fn test_silk_to_mask_clearance<'x>(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
    ) {
        let silk_layers = Lset::from_layers(&[PcbLayerId::FSilkS, PcbLayerId::BSilkS]);

        // If we have no minimum web width then we delegate to the silk checker which does
        // object-to-object testing (instead of object-to-solder-mask-zone-fill checking that
        // we do here).
        if self.web_width <= 0 {
            return;
        }

        let progress_delta = 250;
        let mut count = 0;
        let mut ii = 0;

        for_each_geometry_item(ctx, all_basic_items(), &silk_layers, |_item| {
            count += 1;
            true
        });

        for_each_geometry_item(ctx, all_basic_items(), &silk_layers, |item| {
            if host.is_error_limit_exceeded(DRCE_SILK_MASK_CLEARANCE) {
                return false;
            }

            let i = ii;
            ii += 1;

            if !self.base.report_progress(host, i, count, progress_delta) {
                return false;
            }

            if is_invisible_text(item) {
                return true;
            }

            for layer in silk_layers.seq() {
                if !item.is_on_layer(layer) {
                    continue;
                }

                let mask_layer = if layer == PcbLayerId::FSilkS {
                    PcbLayerId::FMask
                } else {
                    PcbLayerId::BMask
                };
                let item_bbox = item.bounding_box();
                let constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::SilkClearanceConstraint,
                    Some(item),
                    None,
                    mask_layer,
                );
                let clearance = constraint.value().min();

                if constraint.severity() == Severity::Ignore || clearance < 0 {
                    return true;
                }

                let item_shape =
                    item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

                if let Some((actual, pos)) = self.full_solder_mask_rtree.query_colliding(
                    &item_bbox,
                    item_shape.as_ref(),
                    mask_layer,
                    clearance,
                ) {
                    let mut drce = item::create(DRCE_SILK_MASK_CLEARANCE)
                        .expect("DRCE_SILK_MASK_CLEARANCE item");

                    if clearance > 0 {
                        drce.set_error_detail(&self.base.format_msg(
                            "(%s clearance %s; actual %s)",
                            &constraint.name(),
                            clearance as f64,
                            actual as f64,
                            EdaDataType::Distance,
                        ));
                    }

                    drce.set_item_ids(vec![item.uuid()]);
                    drce.violating_rule = constraint.parent_rule.clone();

                    self.base
                        .report_violation(host, Arc::new(drce), pos, layer.0, &no_path);
                }
            }

            true
        });
    }

    /// Port of `testItemAgainstItems`.
    fn test_item_against_items<'x>(
        &mut self,
        engine: &DrcEngine,
        run: &DrcRunState<'_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        ref_layer: PcbLayerId,
        target_layer: PcbLayerId,
    ) {
        let pad = if item.type_() == KicadT::PcbPadT {
            item.as_pad()
        } else {
            None
        };
        let item_net = net_of(item);
        let item_id = item.id().expect("DRC item with a handle");

        let mut item_constraint: Option<DrcConstraint> = None;

        let item_shape =
            item.effective_shape(ref_layer, Flashing::Default, DrcConstraintT::NullConstraint);

        let web_width = self.web_width;
        let bridge_limit = self.bridge_limit;
        let item_tree = &self.item_tree;
        let checked_pairs = &mut self.checked_pairs;
        let aperture_items = &mut self.mask_aperture_items;
        let bridges = &mut self.bridge_violations;

        // Filter:
        let mut filter = |other_id: ItemId| -> bool {
            let other = ctx.item(other_id).expect("tree entry of a board item");
            let item_fp = item.parent_footprint();
            let other_pad = if other.type_() == KicadT::PcbPadT {
                other.as_pad()
            } else {
                None
            };
            let other_net = net_of(other);

            if other_net > 0 && other_net == item_net {
                return false;
            }

            if is_npth_pad_with_no_copper(other) {
                return false;
            }

            if let Some(item_fp) = item_fp
                && item_fp.id() == other.parent_footprint().and_then(|fp| fp.id())
            {
                // Board-wide exclusion
                if item_fp.has_board()
                    && ctx.board.design_settings().allow_soldermask_bridges_in_fps
                {
                    return false;
                }

                // Footprint-specific exclusion
                if item_fp
                    .as_footprint()
                    .is_some_and(|fp| fp.allow_solder_mask_bridges())
                {
                    return false;
                }
            }

            if let (Some(pad), Some(other_pad)) = (pad, other_pad)
                && (pad.same_logical_pad_as(other_pad) || pad.shares_net_tie_group(ctx, other_pad))
            {
                return false;
            }

            if let Some(fp) = item_fp.and_then(|fp| fp.as_footprint())
                && fp.is_net_tie()
            {
                let nets = fp.net_tie_cache(item_id);

                if other_net < 0 || nets.contains(&NetCode(other_net)) {
                    return false;
                }
            }

            if let Some(other_fp) = other.parent_footprint().and_then(|fp| fp.as_footprint())
                && other_fp.is_net_tie()
            {
                let nets = other_fp.net_tie_cache(other_id);

                if item_net < 0 || nets.contains(&NetCode(item_net)) {
                    return false;
                }
            }

            // store canonical order so we don't collide in both directions (a:b and b:a)
            let key = if other_id < item_id {
                (other_id, item_id)
            } else {
                (item_id, other_id)
            };

            let layers = checked_pairs.entry(key).or_default();

            if layers.test(target_layer) {
                false
            } else {
                layers.set(target_layer);
                true
            }
        };

        // Visitor:
        let mut visitor = |other_id: ItemId| -> bool {
            let other = ctx.item(other_id).expect("tree entry of a board item");
            let other_item_shape = other.effective_shape(
                target_layer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            let other_net = net_of(other);

            let mut actual = 0;
            let mut pos = Vector2I::default();

            let clearance = if ref_layer == PcbLayerId::FMask || ref_layer == PcbLayerId::BMask {
                // Aperture-to-aperture must enforce web-min-width
                web_width
            } else {
                // ( aRefLayer == F_Cu || aRefLayer == B_Cu )
                // Copper-to-aperture uses the solder-mask-to-copper-clearance
                ctx.board.design_settings().solder_mask_to_copper_clearance
            };

            if item_shape.collide(
                other_item_shape.as_ref(),
                clearance,
                Some(&mut actual),
                Some(&mut pos),
            ) {
                let item_c = item_constraint.get_or_insert_with(|| {
                    engine.eval_rules(
                        ctx,
                        DrcConstraintT::BridgedMaskConstraint,
                        Some(item),
                        None,
                        ref_layer,
                    )
                });

                let other_constraint = engine.eval_rules(
                    ctx,
                    DrcConstraintT::BridgedMaskConstraint,
                    Some(other),
                    None,
                    target_layer,
                );

                let item_constraint_ignored = item_c.severity() == Severity::Ignore;
                let other_constraint_ignored = other_constraint.severity() == Severity::Ignore;

                // Mask apertures are ignored on their own; in other cases both participants
                // must be ignored
                if (is_mask_aperture(item) && item_constraint_ignored)
                    || (is_mask_aperture(other) && other_constraint_ignored)
                    || (item_constraint_ignored && other_constraint_ignored)
                {
                    return !run.is_cancelled();
                }

                // Simple mask apertures aren't associated with copper items, so they only
                // constitute a bridge when they expose other copper items having at least
                // two distinct nets.  Record the colliding item now and decide/report bridges
                // deterministically once all threads have finished.
                if is_mask_aperture(item) {
                    record_mask_aperture(aperture_items, item, other, ref_layer, other_net, pos);
                } else if is_mask_aperture(other) {
                    record_mask_aperture(aperture_items, other, item, ref_layer, item_net, pos);
                } else if check_item_mask(other, item_net) {
                    collect_bridge(bridges, bridge_limit, item, other, None, pos, target_layer);
                }
            }

            !run.is_cancelled()
        };

        item_tree.query_colliding(
            item,
            ref_layer,
            target_layer,
            Some(&mut filter),
            Some(&mut visitor),
            self.largest_clearance,
        );
    }

    /// Port of `testMaskItemAgainstZones`.
    #[allow(clippy::too_many_arguments)]
    fn test_mask_item_against_zones<'x>(
        &mut self,
        run: &DrcRunState<'_>,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        item_bbox: &Box2I,
        mask_layer: PcbLayerId,
        target_layer: PcbLayerId,
    ) {
        let item_shape = item.effective_shape(
            mask_layer,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        let mut aperture_bbox = *item_bbox;

        aperture_bbox.merge(&item_shape.bbox(0));

        for &zone_id in &self.caches.copper_zones {
            let zone_ref = ctx
                .item(zone_id.into())
                .expect("m_DRCCopperZones holds board zones");
            let zone = zone_ref.as_zone().expect("a zone");

            if !zone.layer_set(ctx).test(target_layer) {
                continue;
            }

            let zone_net = zone_ref.net_code().map_or(-1, |n| n.0);

            if item.is_connected() && zone_net == net_of(item) && zone_net > 0 {
                continue;
            }

            let mut inflated_bbox = aperture_bbox;
            let clearance = ctx.board.design_settings().solder_mask_to_copper_clearance;

            inflated_bbox.inflate(clearance);

            if !inflated_bbox.intersects(&zone_ref.bounding_box()) {
                continue;
            }

            let zone_tree = self.caches.copper_zone_rtree(zone_id);

            if let Some(zone_tree) = zone_tree
                && let Some((_actual, pos)) = zone_tree.query_colliding_box_actual(
                    &aperture_bbox,
                    item_shape.as_ref(),
                    target_layer,
                    clearance,
                )
            {
                // Simple mask apertures aren't associated with copper items, so they only
                // constitute a bridge when they expose other copper items having at least two
                // distinct nets.
                if is_mask_aperture(item) && zone_net >= 0 {
                    record_mask_aperture(
                        &mut self.mask_aperture_items,
                        item,
                        zone_ref,
                        mask_layer,
                        zone_net,
                        pos,
                    );
                } else {
                    collect_bridge(
                        &mut self.bridge_violations,
                        self.bridge_limit,
                        item,
                        zone_ref,
                        None,
                        pos,
                        target_layer,
                    );
                }
            }

            if run.is_cancelled() {
                return;
            }
        }
    }

    /// Port of `testMaskBridges`.
    fn test_mask_bridges<'x>(
        &mut self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
    ) {
        let copper_and_mask_layers = Lset::from_layers(&[
            PcbLayerId::FMask,
            PcbLayerId::BMask,
            PcbLayerId::FCu,
            PcbLayerId::BCu,
        ]);
        let mut test_items: Vec<ItemRef<'x>> = Vec::new();

        for_each_geometry_item(
            ctx,
            all_basic_items_but_zones(),
            &copper_and_mask_layers,
            |item| {
                test_items.push(item);
                true
            },
        );

        // tp.submit_loop( 0, test_items.size(), ... ), run in index order.
        for &item in &test_items {
            let run: &DrcRunState<'_> = host;

            if run.is_cancelled() {
                // The C++ task returns; every later task does the same.
                continue;
            }

            let item_bbox = item.bounding_box();

            if item.is_on_layer(PcbLayerId::FMask) && !is_npth_pad_with_no_copper(item) {
                // Test for aperture-to-aperture collisions
                self.test_item_against_items(
                    engine,
                    run,
                    ctx,
                    item,
                    PcbLayerId::FMask,
                    PcbLayerId::FMask,
                );

                // Test for aperture-to-zone collisions
                self.test_mask_item_against_zones(
                    run,
                    ctx,
                    item,
                    &item_bbox,
                    PcbLayerId::FMask,
                    PcbLayerId::FCu,
                );
            } else if item.is_on_layer(PcbLayerId::FCu) {
                // Test for copper-item-to-aperture collisions
                self.test_item_against_items(
                    engine,
                    run,
                    ctx,
                    item,
                    PcbLayerId::FCu,
                    PcbLayerId::FMask,
                );
            }

            if item.is_on_layer(PcbLayerId::BMask) && !is_npth_pad_with_no_copper(item) {
                // Test for aperture-to-aperture collisions
                self.test_item_against_items(
                    engine,
                    run,
                    ctx,
                    item,
                    PcbLayerId::BMask,
                    PcbLayerId::BMask,
                );

                // Test for aperture-to-zone collisions
                self.test_mask_item_against_zones(
                    run,
                    ctx,
                    item,
                    &item_bbox,
                    PcbLayerId::BMask,
                    PcbLayerId::BCu,
                );
            } else if item.is_on_layer(PcbLayerId::BCu) {
                // Test for copper-item-to-aperture collisions
                self.test_item_against_items(
                    engine,
                    run,
                    ctx,
                    item,
                    PcbLayerId::BCu,
                    PcbLayerId::BMask,
                );
            }
        }

        // The progress polling of the C++ wait loop (every 100 ms) is not reproduced.

        // Decide mask aperture bridges now that all threads have completed and the full set of
        // items exposed by each aperture is known, then emit every collected bridge in a
        // deterministic order.
        self.report_mask_aperture_bridges(engine, host, ctx);
        self.flush_bridge_violations(host, ctx);
    }

    /// Port of `flushBridgeViolations`.
    fn flush_bridge_violations<'x>(&mut self, host: &mut RunHost<'_, '_>, ctx: &'x BoardCtx<'x>) {
        // The collection is a max-heap holding at most the reportable count, so this puts it
        // in reporting order without ever having sorted the bridges the cap discards.
        std_sort_heap(&mut self.bridge_violations, |a, b| a.less(b));

        let front_msg = "Front solder mask aperture bridges items with different nets";
        let back_msg = "Rear solder mask aperture bridges items with different nets";

        for bridge in &self.bridge_violations {
            if host.is_error_limit_exceeded(DRCE_SOLDERMASK_BRIDGE) || host.is_cancelled() {
                break;
            }

            let mut drce =
                item::create(DRCE_SOLDERMASK_BRIDGE).expect("DRCE_SOLDERMASK_BRIDGE item");

            drce.set_error_message(if is_front_layer(bridge.layer) {
                front_msg
            } else {
                back_msg
            });

            let item_a = ctx.item(bridge.a).expect("bridge item");
            let item_b = ctx.item(bridge.b).expect("bridge item");
            let item_c = bridge.c.map(|c| ctx.item(c).expect("bridge item"));

            // SetItems skips null participants, so the two-item case needs no separate call.
            let mut ids = vec![item_a.uuid(), item_b.uuid()];

            if let Some(c) = item_c {
                ids.push(c.uuid());
            }

            drce.set_item_ids(ids);
            drce.violating_rule = Some(self.bridge_rule.clone());

            // Recompute the marker position from the two (UUID-ordered) copper items so it does
            // not depend on which worker observed the collision.  Query the copper layer, where
            // every participant (including zones) has a real shape.
            let item_x = if item_c.is_some() { item_b } else { item_a };
            let item_y = item_c.unwrap_or(item_b);
            let copper_layer = if is_front_layer(bridge.layer) {
                PcbLayerId::FCu
            } else {
                PcbLayerId::BCu
            };
            let mut marker_pos = item_x.position();

            let shape_x = item_x.effective_shape(
                copper_layer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            let shape_y = item_y.effective_shape(
                copper_layer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            let mut pt_x = Vector2I::default();
            let mut pt_y = Vector2I::default();

            if shape_x.nearest_points(shape_y.as_ref(), &mut pt_x, &mut pt_y) {
                marker_pos = Seg::new(pt_x, pt_y).center();
            }

            self.base
                .report_violation(host, Arc::new(drce), marker_pos, bridge.layer.0, &no_path);
        }
    }

    /// Port of `reportMaskApertureBridges`.
    fn report_mask_aperture_bridges<'x>(
        &mut self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &'x BoardCtx<'x>,
    ) {
        let _ = engine;
        let run: &DrcRunState<'_> = host;

        // Visit apertures in a stable order (heap addresses and worker order are not
        // reproducible).
        let mut aperture_keys: Vec<(ItemId, PcbLayerId)> =
            self.mask_aperture_items.keys().copied().collect();

        let uuid_of = |id: ItemId| ctx.item(id).expect("aperture item").uuid();

        std_sort(&mut aperture_keys, |a, b| {
            if uuid_of(a.0) != uuid_of(b.0) {
                return uuid_of(a.0) < uuid_of(b.0);
            }

            a.1.0 < b.1.0
        });

        let report_all_tracks = run.report_all_track_errors();

        for key in &aperture_keys {
            // Pairing is quadratic in the items an aperture exposes, so stay interruptible.
            if run.is_cancelled() {
                return;
            }

            let aperture = ctx.item(key.0).expect("aperture item");
            let mask_layer = key.1;

            // Built once per aperture rather than per candidate pair; the map covers every pad
            // in the footprint and the pairing below is quadratic.
            let aperture_footprint = aperture.parent_footprint();
            let mut net_tie_groups = std::collections::BTreeMap::new();

            if let Some(fp) = aperture_footprint.and_then(|fp| fp.as_footprint()) {
                net_tie_groups = fp.map_pad_numbers_to_net_tie_groups(ctx);
            }

            let items = self
                .mask_aperture_items
                .get_mut(key)
                .expect("aperture key of the map");

            std_sort(items, |a, b| {
                if uuid_of(a.item) != uuid_of(b.item) {
                    return uuid_of(a.item) < uuid_of(b.item);
                }

                a.net < b.net
            });

            // The first positive net exposed by the aperture is the reference net; any item on a
            // different positive net bridges it.
            let mut ref_item: Option<ItemId> = None;
            let mut ref_net = -1;

            for entry in items.iter() {
                if entry.net > 0 {
                    ref_item = Some(entry.item);
                    ref_net = entry.net;
                    break;
                }
            }

            let Some(ref_item) = ref_item else {
                continue;
            };

            let ref_item = ctx.item(ref_item).expect("aperture entry");
            let aperture_fp_id = aperture_footprint.and_then(|fp| fp.id());
            let mut reported_pairs: HashSet<(ItemId, ItemId)> = HashSet::new();

            for collision in items.iter() {
                if run.is_cancelled() {
                    return;
                }

                if collision.net == ref_net {
                    continue;
                }

                let collision_item = ctx.item(collision.item).expect("aperture entry");

                // Footprint-local exclusions need the colliding item inside the aperture's
                // footprint, which is invariant across the inner loop.
                let collision_in_footprint = aperture_fp_id.is_some()
                    && collision_item.parent_footprint().and_then(|fp| fp.id()) == aperture_fp_id;

                if collision_in_footprint
                    && mask_aperture_bridge_excluded(
                        aperture_fp_id,
                        &net_tie_groups,
                        ref_item,
                        collision_item,
                    )
                {
                    continue;
                }

                let mut reported_any_track = false;

                for entry in items.iter() {
                    if entry.net == collision.net {
                        continue;
                    }

                    let entry_item = ctx.item(entry.item).expect("aperture entry");

                    if collision_in_footprint
                        && mask_aperture_bridge_excluded(
                            aperture_fp_id,
                            &net_tie_groups,
                            entry_item,
                            collision_item,
                        )
                    {
                        continue;
                    }

                    let entry_is_track = entry_item.type_() == KicadT::PcbTraceT
                        || entry_item.type_() == KicadT::PcbArcT;

                    // Track throttling is per-collision, so it must gate emission before the
                    // pair is marked reported.  Marking a suppressed pair here would drop it for
                    // good under a later collision ordering, making the total count depend on
                    // the reference net.
                    if entry_is_track && reported_any_track && !report_all_tracks {
                        continue;
                    }

                    // Deduplicate unordered pairs so (B, C) and (C, B) are reported once.
                    let mut lo = entry.item;
                    let mut hi = collision.item;

                    if hi < lo {
                        std::mem::swap(&mut lo, &mut hi);
                    }

                    if !reported_pairs.insert((lo, hi)) {
                        continue;
                    }

                    collect_bridge(
                        &mut self.bridge_violations,
                        self.bridge_limit,
                        aperture,
                        entry_item,
                        Some(collision_item),
                        collision.pos,
                        mask_layer,
                    );

                    if entry_is_track {
                        reported_any_track = true;
                    }
                }
            }
        }
    }

    /// Port of `DRC_TEST_PROVIDER_SOLDER_MASK::Run`: false when the run was cancelled.
    pub fn run<'x>(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &'x BoardCtx<'x>,
    ) -> bool {
        if run.is_error_limit_exceeded(DRCE_SILK_MASK_CLEARANCE)
            && run.is_error_limit_exceeded(DRCE_SOLDERMASK_BRIDGE)
        {
            report_aux(run, "Solder mask violations ignored. Tests not run.");
            return true; // continue with other tests
        }

        let board = ctx.board;
        let bds = board.design_settings();

        self.web_width = bds.solder_mask_min_width;
        self.max_error = bds.max_error;
        self.largest_clearance = 0;

        let mut largest_clearance = 0;
        let mut update_largest_clearance = |clearance: i32| {
            largest_clearance = largest_clearance.max(clearance);
        };

        for (fp_id, _footprint) in board.footprints() {
            for (_, pad) in board.footprint_pads(fp_id) {
                pad.lib_padstack().for_each_unique_layer(|layer| {
                    update_largest_clearance(pad.solder_mask_expansion(ctx, layer));
                });
            }

            for (_, item) in board.footprint_drawings(fp_id) {
                if let AnyItem::Shape(shape) = item
                    && ctx.view(item).type_() == KicadT::PcbShapeT
                {
                    update_largest_clearance(shape.solder_mask_expansion(ctx));
                }
            }
        }

        // PCB_TRACK::GetSolderMaskExpansion is not virtual: a via in Tracks() takes the
        // PCB_TRACK body too.
        for (_, track) in board.tracks() {
            let expansion = match track {
                Track::Trace(t) => t.solder_mask_expansion(ctx),
                Track::Arc(a) => a.solder_mask_expansion(ctx),
                Track::Via(_) => track.as_track().solder_mask_expansion(ctx),
            };

            update_largest_clearance(expansion);
        }

        for (_, item) in board.drawings() {
            if let AnyItem::Shape(shape) = item
                && ctx.view(item).type_() == KicadT::PcbShapeT
            {
                update_largest_clearance(shape.solder_mask_expansion(ctx));
            }
        }

        self.largest_clearance = largest_clearance;

        // Order is important here: m_webWidth must be added in before m_largestClearance is
        // maxed with the various clearance constraints.
        self.largest_clearance += self.largest_clearance + self.web_width;

        // Include SolderMaskToCopperClearance so R-tree queries find copper items that are
        // within the required distance of mask apertures. Without this, tracks passing near pad
        // apertures from different nets would not be found if SolderMaskToCopperClearance >
        // m_largestClearance.
        self.largest_clearance = self
            .largest_clearance
            .max(bds.solder_mask_to_copper_clearance);

        if let Some(worst) =
            engine.query_worst_constraint(DrcConstraintT::SilkClearanceConstraint, false)
        {
            self.largest_clearance = self.largest_clearance.max(worst.value.min());
        }

        let mut host = RunHost(run);

        if !self.base.report_phase(&mut host, "Building solder mask...") {
            return false; // DRC cancelled
        }

        self.checked_pairs.clear();
        self.mask_aperture_items.clear();
        self.bridge_violations.clear();

        // Snapshotting the cap keeps the bridge collection bounded without contending on the
        // engine's error-limit mutex from the worker threads.  Nothing else reports
        // DRCE_SOLDERMASK_BRIDGE, so the budget cannot shrink under us.
        self.bridge_limit = host.error_limit(DRCE_SOLDERMASK_BRIDGE);
        self.bridge_violations
            .reserve(self.bridge_limit.max(0) as usize);

        self.build_rtrees(&mut host, ctx);

        if !self
            .base
            .report_phase(&mut host, "Checking solder mask to silk clearance...")
        {
            return false; // DRC cancelled
        }

        self.test_silk_to_mask_clearance(engine, &mut host, ctx);

        if self.bridge_limit > 0 {
            if !self
                .base
                .report_phase(&mut host, "Checking solder mask web integrity...")
            {
                return false; // DRC cancelled
            }

            self.test_mask_bridges(engine, &mut host, ctx);
        }

        !host.is_cancelled()
    }
}

/// Port of `recordMaskAperture`.
fn record_mask_aperture(
    map: &mut HashMap<(ItemId, PcbLayerId), Vec<MaskApertureItem>>,
    mask_item: ItemRef<'_>,
    test_item: ItemRef<'_>,
    test_layer: PcbLayerId,
    test_net: i32,
    pos: Vector2I,
) {
    // Only positive nets can bridge, and the pairing below is quadratic in what is recorded
    // here.
    if test_net <= 0 {
        return;
    }

    if test_layer == PcbLayerId::FMask && !test_item.is_on_layer(PcbLayerId::FCu) {
        return;
    }

    if test_layer == PcbLayerId::BMask && !test_item.is_on_layer(PcbLayerId::BCu) {
        return;
    }

    // Mask apertures in footprints which allow soldermask bridges are ignored entirely.
    if mask_item
        .parent_footprint()
        .and_then(|fp| fp.as_footprint())
        .is_some_and(|fp| fp.allow_solder_mask_bridges())
    {
        return;
    }

    let mask_layer = if is_front_layer(test_layer) {
        PcbLayerId::FMask
    } else {
        PcbLayerId::BMask
    };
    let key = (mask_item.id().expect("aperture with a handle"), mask_layer);

    map.entry(key).or_default().push(MaskApertureItem {
        item: test_item.id().expect("aperture entry with a handle"),
        net: test_net,
        pos,
    });
}

/// Port of `maskApertureBridgeExcluded`.
///
/// Items belonging to the same net-tie group (or the same logical pad) may legitimately share a
/// mask aperture, so a pairing between them is not a bridge.
fn mask_aperture_bridge_excluded(
    aperture_footprint: Option<ItemId>,
    net_tie_groups: &std::collections::BTreeMap<String, i32>,
    ref_item: ItemRef<'_>,
    test_item: ItemRef<'_>,
) -> bool {
    if aperture_footprint.is_none()
        || test_item.parent_footprint().and_then(|fp| fp.id()) != aperture_footprint
    {
        return false;
    }

    let pad_a: Option<&Pad> = if ref_item.type_() == KicadT::PcbPadT {
        ref_item.as_pad()
    } else {
        None
    };
    let pad_b: Option<&Pad> = if test_item.type_() == KicadT::PcbPadT {
        test_item.as_pad()
    } else {
        None
    };

    if let (Some(pad_a), Some(pad_b)) = (pad_a, pad_b) {
        return pad_a.same_logical_pad_as(pad_b) || pad_a.shares_net_tie_group(ref_item.ctx, pad_b);
    }

    if let Some(pad_a) = pad_a
        && test_item.type_() == KicadT::PcbShapeT
    {
        net_tie_groups.contains_key(pad_a.number())
    } else if let Some(pad_b) = pad_b
        && ref_item.type_() == KicadT::PcbShapeT
    {
        net_tie_groups.contains_key(pad_b.number())
    } else {
        false
    }
}

/// Port of `checkItemMask`.
fn check_item_mask(item: ItemRef<'_>, test_net: i32) -> bool {
    let ctx = item.ctx;

    if let Some(fp) = item.parent_footprint().and_then(|fp| fp.as_footprint()) {
        // If we're allowing bridges then we're allowing bridges.  Nothing to check.
        if fp.allow_solder_mask_bridges() {
            return false;
        }

        // Items belonging to a net-tie may share the mask aperture of pads in the same group.
        if item.type_() == KicadT::PcbPadT && fp.is_net_tie() {
            let pad = item.as_pad().expect("PCB_PAD_T is a PAD");
            let pad_number_to_group_idx_map = fp.map_pad_numbers_to_net_tie_groups(ctx);
            // std::map::operator[] value-initialises a missing number to 0.
            let group_idx = pad_number_to_group_idx_map
                .get(pad.number())
                .copied()
                .unwrap_or(0);

            if group_idx >= 0 {
                if test_net < 0 {
                    return false;
                }

                if pad.net_code().0 == test_net {
                    return false;
                }

                let pad_id = item
                    .id()
                    .and_then(|id| id.typed::<Pad>())
                    .expect("pad with a handle");

                for other in fp.net_tie_pads(ctx, pad_id) {
                    if ctx
                        .board
                        .pad(other)
                        .is_some_and(|p| p.net_code().0 == test_net)
                    {
                        return false;
                    }
                }
            }
        }
    }

    true
}

/// Port of `collectBridge`: adds a bridge to the bounded max-heap `bridges`.
fn collect_bridge(
    bridges: &mut Vec<PendingBridge>,
    bridge_limit: i32,
    item_a: ItemRef<'_>,
    item_b: ItemRef<'_>,
    item_c: Option<ItemRef<'_>>,
    pos: Vector2I,
    layer: PcbLayerId,
) {
    let mut a = item_a;
    let mut b = item_b;
    let mut c = item_c;

    // Canonicalize the item order so the reported violation is identical no matter which
    // worker observed the collision.  The aperture (when present) stays first; the copper items
    // are ordered by UUID.
    match &mut c {
        None => {
            if b.uuid() < a.uuid() {
                std::mem::swap(&mut a, &mut b);
            }
        }
        Some(cc) => {
            if cc.uuid() < b.uuid() {
                std::mem::swap(&mut b, cc);
            }
        }
    }

    // Only the third item is ever absent, and the swaps above already ordered the copper
    // items, so the key needs at most the aperture inserted.  The unused slot stays trailing.
    let mut ids = [a.uuid(), b.uuid(), c.map_or(NILUUID, |c| c.uuid())];

    if ids[1] < ids[0] {
        ids.swap(0, 1);
    }

    if c.is_some() && ids[2] < ids[1] {
        ids.swap(1, 2);

        if ids[1] < ids[0] {
            ids.swap(0, 1);
        }
    }

    let bridge = PendingBridge {
        a: a.id().expect("bridge item with a handle"),
        b: b.id().expect("bridge item with a handle"),
        c: c.map(|c| c.id().expect("bridge item with a handle")),
        pos,
        layer,
        ids,
    };

    if bridges.len() as i32 == bridge_limit {
        // Anything sorting after the worst kept bridge can never reach the report.
        if !bridge.less(&bridges[0]) {
            return;
        }

        std_pop_heap(bridges, |x, y| x.less(y));
        bridges.pop();
    }

    bridges.push(bridge);
    std_push_heap(bridges, |x, y| x.less(y));
}

impl DrcTestProviderRun for DrcTestProviderSolderMask<'_> {
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
