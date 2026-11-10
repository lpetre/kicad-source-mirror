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

//! Port of `pcbnew/drc/drc_test_provider_connection_width.cpp`.
//!
//! Checks for copper connections that are less than the specified minimum width. Errors
//! generated:
//! - `DRCE_CONNECTION_WIDTH`
//!
//! Order (`drc.md` 4.12): the C++ keeps the per (net, layer) copper in an `unordered_map`
//! (`NETCODE_LAYER_CACHE_KEY` hash order), the items of one entry in a `std::set<BOARD_ITEM*>`
//! (address order), runs one thread-pool task per (entry, minimum width) and collects the
//! items at a neck from `DRC_RTREE::GetObjectsAt` (an `unordered_set`) and from
//! `m_CopperZoneRTreeCache` (an `unordered_map`). The port runs sequentially in (net code,
//! layer) order, adds the items of an entry in collection order (copper zones, tracks, pads),
//! and takes the contributing items in the order libstdc++ iterates the `unordered_set` when
//! no bucket collision reorders it (reverse insertion order of the distinct parents the tree
//! search visits), then zones in `ZoneId` order. When the tree search finds one or two
//! distinct parents at a neck, the order inside a violation (which also decides which two
//! items `EvalRules` sees) is therefore the C++ one. With three or more found (even if only
//! two of them contribute) it depends on heap addresses in C++: an insert into a
//! non-empty bucket is linked after that bucket's predecessor node instead of at the front of
//! the list (address modulo the bucket count: 13 buckets for up to 13 items, then 29, 59, ...), and
//! a rehash re-links the whole list; the port's order is the collision-free one. The zone part
//! (`unordered_map` over all zone trees) and the report order are not reproducible either; the
//! necks found per entry are (`m_hits` is a `std::set`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kicad_core::Lset;
use kicad_core::PcbLayerId;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_geom::cmath::f64_to_i64_x86;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::geometry::vertex_set::{NIL, VertexId, VertexSet};
use kicad_geom::math::vector2::{Vector2D, Vector2I};
use kicad_pcb::drc_api::DRCE_CONNECTION_WIDTH;
use kicad_pcb::ids::ItemId;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use super::sliver_checker::add_item_polys_with_endings;
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::DrcTestProviderBase;

/// Default of `ADVANCED_CFG::m_ExtraClearance` (mm, `common/advanced_config.cpp`); the port
/// has no advanced configuration file.
pub const ADVANCED_CFG_EXTRA_CLEARANCE: f64 = 0.0005;

/// Port of `POLYGON_TEST`: the neck search of one outline over a `VERTEX_SET`.
pub struct PolygonTest {
    /// The `VERTEX_SET` base (`VERTEX_SET( 0 )`: no simplification).
    vs: VertexSet,
    /// `m_limit`
    limit: i32,
    /// `m_hits`: the chain point indices of every neck found.
    hits: BTreeSet<(i32, i32)>,
}

impl PolygonTest {
    /// Port of `POLYGON_TEST( int aLimit )`.
    pub fn new(limit: i32) -> Self {
        PolygonTest {
            vs: VertexSet::new(0),
            limit,
            hits: BTreeSet::new(),
        }
    }

    /// Port of `POLYGON_TEST::FindPairs`: true when the outline has a neck narrower than the
    /// limit.
    pub fn find_pairs(&mut self, poly: &ShapeLineChain) -> bool {
        self.hits.clear();
        self.vs = VertexSet::new(0);
        self.vs.bbox = poly.bbox(0);

        self.vs.create_list(poly, NIL, 0);

        // m_vertices.front() of an empty deque is undefined in C++; a chain without points
        // has no neck.
        if self.vs.vertices.is_empty() {
            return false;
        }

        let front: VertexId = 0;
        self.vs.update_list(front);

        let mut p = self.vs.next(front);
        let mut all_hits: BTreeSet<VertexId> = BTreeSet::new();

        while p != front {
            // Only run the expensive search if we don't already have a match for the point
            if all_hits.is_empty() || !all_hits.contains(&p) {
                let matched = self.get_kink(p);

                if matched != NIL
                    && !all_hits.contains(&matched)
                    && self.hits.insert((self.vs.v(p).i, self.vs.v(matched).i))
                {
                    all_hits.insert(p);
                    all_hits.insert(matched);
                    all_hits.insert(self.vs.next(p));
                    all_hits.insert(self.vs.prev(p));
                    all_hits.insert(self.vs.next(matched));
                    all_hits.insert(self.vs.prev(matched));
                }
            }

            p = self.vs.next(p);
        }

        !self.hits.is_empty()
    }

    /// `POLYGON_TEST::GetVertices`: the necks found by the last [`Self::find_pairs`].
    pub fn vertices(&self) -> &BTreeSet<(i32, i32)> {
        &self.hits
    }

    /// Port of `POLYGON_TEST::isSubstantial`: checks to see if there is a "substantial"
    /// protrusion in each polygon produced by the cut from `a_a` to `a_b`. Substantial in this
    /// case means that the polygon bulges out to a wider cross-section than the distance from
    /// `a_a` to `a_b`. True if the two polygons are both "substantial".
    fn is_substantial(&self, a_a: VertexId, a_b: VertexId) -> bool {
        let vs = &self.vs;
        let limit = self.limit as f64;
        let mut x_change = false;
        let mut y_change = false;

        // This is a failsafe in case of invalid lists.  Never check
        // more than the total number of points in m_vertices
        let mut checked: usize = 0;
        let total_pts = vs.vertices.len();

        let p0 = a_a;
        let mut p = vs.get_next_outline_vertex(p0);

        while !vs.same_point(p, a_b)        // We've reached the other inflection point
            && !vs.same_point(p, a_a)       // We've gone around in a circle
            && checked < total_pts          // Fail-safe for invalid lists
            && !(x_change && y_change)
        // We've found a substantial change in both directions
        {
            let diff_x = (vs.v(p).x - vs.v(p0).x).abs();
            let diff_y = (vs.v(p).y - vs.v(p0).y).abs();

            // Check for a substantial change in the x or y direction
            // This is measured by the set value of the minimum connection width
            if diff_x > limit {
                x_change = true;
            }

            if diff_y > limit {
                y_change = true;
            }

            p = vs.get_next_outline_vertex(p);

            checked += 1;
        }

        // wxCHECK_MSG( checked < total_pts, false, "Invalid polygon detected.  Missing points
        // to check" )
        if checked >= total_pts {
            return false;
        }

        if !vs.same_point(p, a_a) && (!x_change || !y_change) {
            return false;
        }

        p = vs.get_prev_outline_vertex(p0);

        x_change = false;
        y_change = false;
        checked = 0;

        while !vs.same_point(p, a_b)        // We've reached the other inflection point
            && !vs.same_point(p, a_a)       // We've gone around in a circle
            && checked < total_pts          // Fail-safe for invalid lists
            && !(x_change && y_change)
        // We've found a substantial change in both directions
        {
            let diff_x = (vs.v(p).x - vs.v(p0).x).abs();
            let diff_y = (vs.v(p).y - vs.v(p0).y).abs();

            // Floating point zeros can have a negative sign, so we need to
            // ensure that only substantive diversions count for a direction
            // change
            if diff_x > limit {
                x_change = true;
            }

            if diff_y > limit {
                y_change = true;
            }

            p = vs.get_prev_outline_vertex(p);

            checked += 1;
        }

        // wxCHECK_MSG( checked < total_pts, false, ... )
        if checked >= total_pts {
            return false;
        }

        vs.same_point(p, a_a) || (x_change && y_change)
    }

    /// One candidate of the two z-order walks of `getKink`.
    fn kink_candidate(
        &self,
        a_pt: VertexId,
        p: VertexId,
        limit2: i64,
        min_dist: i64,
    ) -> Option<i64> {
        let vs = &self.vs;
        let (pv, av) = (vs.v(p), vs.v(a_pt));
        let delta_i = (pv.i - av.i).abs();
        let diff = Vector2D::new(pv.x - av.x, pv.y - av.y);
        let dist2 = f64_to_i64_x86(diff.squared_euclidean_norm());

        if delta_i > 1
            && dist2 < limit2
            && dist2 < min_dist
            && dist2 > 0
            && vs.locally_inside(p, a_pt)
            && self.is_substantial(p, a_pt)
            && self.is_substantial(a_pt, p)
        {
            return Some(dist2);
        }

        None
    }

    /// Port of `POLYGON_TEST::getKink`: the nearest vertex across a neck from `a_pt`, `NIL`
    /// for none.
    fn get_kink(&self, a_pt: VertexId) -> VertexId {
        let vs = &self.vs;

        // The point needs to be at a concave surface
        if vs.locally_inside(vs.prev(a_pt), vs.next(a_pt)) {
            return NIL;
        }

        // z-order range for the current point +/- limit bounding box
        let av = vs.v(a_pt);
        let max_z = vs.z_order(av.x + self.limit as f64, av.y + self.limit as f64);
        let min_z = vs.z_order(av.x - self.limit as f64, av.y - self.limit as f64);
        let limit2 = Seg::square(self.limit);

        // first look for points in increasing z-order
        let mut p = av.next_z;
        let mut min_dist = i64::MAX;
        let mut retval = NIL;

        while p != NIL && vs.v(p).z <= max_z {
            if let Some(dist2) = self.kink_candidate(a_pt, p, limit2, min_dist) {
                min_dist = dist2;
                retval = p;
            }

            p = vs.v(p).next_z;
        }

        p = av.prev_z;

        while p != NIL && vs.v(p).z >= min_z {
            if let Some(dist2) = self.kink_candidate(a_pt, p, limit2, min_dist) {
                min_dist = dist2;
                retval = p;
            }

            p = vs.v(p).prev_z;
        }

        retval
    }
}

/// `ITEMS_POLY`: the items of one (net, layer) and the union of their copper.
#[derive(Default)]
struct ItemsPoly {
    /// `Items` (`std::set<BOARD_ITEM*>`; here in collection order, without duplicates).
    items: Vec<ItemId>,
    /// `Poly`
    poly: ShapePolySet,
}

impl ItemsPoly {
    /// `Items.emplace( item )`.
    fn emplace(&mut self, id: ItemId) {
        if !self.items.contains(&id) {
            self.items.push(id);
        }
    }
}

/// Port of `DRC_TEST_PROVIDER_CONNECTION_WIDTH`. `'c` is the lifetime of the per-run board
/// caches (`m_DRCCopperZones`, `m_CopperItemRTreeCache`, `m_CopperZoneRTreeCache`).
pub struct DrcTestProviderConnectionWidth<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches built by the cache generator for this run.
    caches: &'c DrcBoardCaches,
}

impl<'c> DrcTestProviderConnectionWidth<'c> {
    /// `DRC_TEST_PROVIDER_CONNECTION_WIDTH()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderConnectionWidth {
            base: DrcTestProviderBase::new("copper width"),
            caches,
        }
    }

    /// Port of `DRC_TEST_PROVIDER_CONNECTION_WIDTH::layerDesc`.
    fn layer_desc(ctx: &BoardCtx<'_>, layer: PcbLayerId) -> String {
        format!("({})", ctx.board.layer_name(layer))
    }

    /// Port of `DRC_TEST_PROVIDER_CONNECTION_WIDTH::Run`: false when the run was cancelled.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_CONNECTION_WIDTH) {
            report_aux(run, "Connection width violations ignored. Tests not run.");
            return true; // Continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking nets for minimum connection width...")
        {
            return false; // DRC cancelled
        }

        let board = ctx.board;
        let mut epsilon = board.design_settings().get_drc_epsilon();

        // Zone knockouts can be approximated, and always have extra clearance built in
        epsilon +=
            board.design_settings().max_error + PCB_IU_SCALE.mm_to_iu(ADVANCED_CFG_EXTRA_CLEARANCE);

        // A neck in a zone fill can be between two knockouts. In this case it will be epsilon
        // smaller on -each- side.
        epsilon *= 2;

        // Build a set of distinct minWidths specified by various DRC rules.  We'll run a test
        // for each distinct minWidth, and then decide if any copper which failed that minWidth
        // actually was required to abide by it or not.
        let distinct_min_widths =
            engine.query_distinct_constraints(DrcConstraintT::ConnectionWidthConstraint);

        if host.is_cancelled() {
            return false; // DRC cancelled
        }

        let mut dataset: BTreeMap<(i32, PcbLayerId), ItemsPoly> = BTreeMap::new();
        let net_of =
            |id: ItemId| -> i32 { ctx.item(id).and_then(|i| i.net_code()).map_or(-1, |n| n.0) };

        for layer in Lset::all_cu_mask_n(board.copper_layer_count()).seq() {
            for &zone_id in &self.caches.copper_zones {
                let zone = board.zone(zone_id).expect("m_DRCCopperZones entry");
                let id: ItemId = zone_id.into();

                if !zone.is_rule_area() && ctx.item(id).is_some_and(|z| z.is_on_layer(layer)) {
                    dataset.entry((net_of(id), layer)).or_default().emplace(id);
                }
            }

            for (track_id, track) in board.tracks() {
                let id: ItemId = track_id.into();

                if let Track::Via(via) = track {
                    if via.flash_layer(ctx, layer) {
                        dataset.entry((net_of(id), layer)).or_default().emplace(id);
                    }
                } else if ctx.view(AnyItem::Track(track)).is_on_layer(layer) {
                    dataset.entry((net_of(id), layer)).or_default().emplace(id);
                }
            }

            for (fp_id, _) in board.footprints() {
                for (pad_id, pad) in board.footprint_pads(fp_id) {
                    let id: ItemId = pad_id.into();

                    if pad.flash_layer(ctx, layer) {
                        dataset.entry((net_of(id), layer)).or_default().emplace(id);
                    }
                }

                // Footprint zones are also in the m_DRCCopperZones cache
            }
        }

        // The build_netlayer_polys tasks, in submission (key) order.
        for (&(_, layer), items_poly) in dataset.iter_mut() {
            if host.is_cancelled() {
                break;
            }

            for &id in &items_poly.items {
                let item = ctx.item(id).expect("dataset item");
                add_item_polys_with_endings(
                    item,
                    &mut items_poly.poly,
                    layer,
                    0,
                    ARC_HIGH_DEF,
                    ErrorLoc::Outside,
                );
            }

            items_poly.poly.fracture(true);
        }

        // The min_checker tasks, in submission order.
        for (&(_, layer), items_poly) in &dataset {
            for &min_width in &distinct_min_widths {
                if min_width - epsilon <= 0 {
                    continue;
                }

                self.min_checker(
                    engine, &mut host, ctx, items_poly, layer, min_width, epsilon,
                );
            }
        }

        !host.is_cancelled()
    }

    /// The `min_checker` lambda of `Run`: examine all necks in a given polygon set which fail
    /// a given minimum width.
    #[allow(clippy::too_many_arguments)]
    fn min_checker(
        &self,
        engine: &DrcEngine,
        host: &mut RunHost<'_, '_>,
        ctx: &BoardCtx<'_>,
        items_poly: &ItemsPoly,
        layer: PcbLayerId,
        min_width: i32,
        epsilon: i32,
    ) {
        if host.is_cancelled() {
            return;
        }

        let test_width = min_width - epsilon;

        let mut test = PolygonTest::new(test_width);

        for ii in 0..items_poly.poly.outline_count() {
            let chain = items_poly.poly.c_outline(ii);

            test.find_pairs(chain);

            for &(first, second) in test.vertices() {
                // We've found a neck that fails the given aMinWidth.  We now need to know if
                // the objects the produced the copper at this location are required to abide
                // by said aMinWidth or not.  (If so, we have a violation.)
                //
                // We find the contributingItems by hit-testing at the choke point (the centre
                // point of the neck), and then run the rules engine on those
                // contributingItems.  If the reported constraint matches aMinWidth, then we've
                // got a violation.
                let span = Seg::new(chain.c_point(first), chain.c_point(second));
                let location: Vector2I = (span.a + span.b) / 2;
                let dist = (span.a - span.b).euclidean_norm();

                let mut contributing_items: Vec<ItemId> = Vec::new();

                // GetObjectsAt() returns a std::unordered_set<BOARD_ITEM*>; iterating it with
                // libstdc++ visits the distinct parents in reverse insertion order (each new
                // node goes to the front of the list, or of its bucket's group) whenever no
                // new item lands in the bucket of an item other than the list's first. That is
                // exact for one or two items; see the module comment for three or more.
                let mut objects = self
                    .caches
                    .copper_item_rtree()
                    .get_objects_at(location, layer, min_width);
                objects.reverse();

                for id in objects {
                    let item = ctx.item(id).expect("copper item tree entry");

                    if item.as_dyn().hit_test(ctx, location, min_width) {
                        contributing_items.push(id);
                    }
                }

                for (&zone_id, rtree) in &self.caches.copper_zone_rtrees {
                    let obj_list = rtree.get_objects_at(location, layer, min_width);
                    let zone = ctx
                        .board
                        .zone(zone_id)
                        .expect("m_CopperZoneRTreeCache zone");

                    if !obj_list.is_empty()
                        && zone.hit_test_filled_area(ctx, layer, location, min_width)
                    {
                        contributing_items.push(zone_id.into());
                    }
                }

                if !contributing_items.is_empty() {
                    let item1 = ctx.item(contributing_items[0]);
                    let item2 = contributing_items.get(1).and_then(|&id| ctx.item(id));
                    let c = engine.eval_rules(
                        ctx,
                        DrcConstraintT::ConnectionWidthConstraint,
                        item1,
                        item2,
                        layer,
                    );

                    if c.value().min() == min_width {
                        let mut drc_item =
                            item::create(DRCE_CONNECTION_WIDTH).expect("DRCE_CONNECTION_WIDTH");

                        let mut msg = self.base.format_msg(
                            "(%s minimum connection width %s; actual %s)",
                            &c.name(),
                            c.value().min() as f64,
                            dist as f64,
                            EdaDataType::Distance,
                        );

                        msg += " ";
                        msg += &Self::layer_desc(ctx, layer);

                        drc_item.set_error_detail(&msg);
                        drc_item.violating_rule = c.parent_rule.clone();

                        drc_item.set_item_ids(
                            contributing_items
                                .iter()
                                .map(|&id| ctx.item(id).expect("contributing item").uuid())
                                .collect(),
                        );

                        self.base.report_two_point_geometry(
                            host,
                            Arc::new(drc_item),
                            location,
                            span.a,
                            span.b,
                            layer,
                        );
                    }
                }
            }
        }
    }
}

impl DrcTestProviderRun for DrcTestProviderConnectionWidth<'_> {
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
