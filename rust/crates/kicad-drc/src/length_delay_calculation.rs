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

//! Port of `pcbnew/length_delay_calculation/length_delay_calculation.cpp` / `.h`,
//! `length_delay_calculation_item.cpp` / `.h` and
//! `tuning_profile_parameters_user_defined.cpp` / `.h` (the default
//! `TUNING_PROFILE_PARAMETERS_IFACE` provider).
//!
//! The calculator lives in `kicad-drc` (not `kicad-pcb`) because it queries the connectivity
//! (`IsConnectedOnLayer`, `GetConnectedPads`), which `kicad-pcb` cannot depend on.
//!
//! Differences in representation:
//! - `LENGTH_DELAY_CALCULATION_ITEM` holds the board item as an [`ItemId`] and the effective
//!   net class as its tuning profile name (the only member of the class the calculation reads).
//! - The `std::map<VECTOR2I, std::unordered_set<ITEM*>>` position maps become `BTreeMap`s keyed
//!   by `(x, y)` (the C++ `VECTOR2I::operator<` is lexicographic) holding item indices. The
//!   results never depend on the iteration order of the unordered sets (a merge needs exactly
//!   one other unmerged line on the layer; the via span only reads the set of layers).
//! - `GetConnectedPads( via )` (heap order in C++, review-connectivity F-6) takes the first pad
//!   in `ItemId` order.
//! - The tuning profile caches are built when the calculator is constructed (C++:
//!   `SynchronizeTuningProfileProperties()` at load), from the bound project's
//!   `TUNING_PROFILES`; without a project they are empty, as in C++.
//! - The `PNS_TUNE` trace output is not ported.

use std::collections::BTreeMap;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::base_units::PCB_IU_PER_MM;
use kicad_core::layer_ids::{Flashing, PcbLayerId, is_copper_layer_lower_than};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::{Shape, ShapeRef};
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::board_stackup::BoardStackup;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::project::tuning_profiles_json::{TuningProfile, load_tuning_profiles};
use kicad_pcb::{BoardCtx, ItemId, ItemRef};

use crate::constraint_types::DrcConstraintT;

/// Port of `LENGTH_DELAY_STATS`: length measurement result details and statistics (the
/// per-layer detail maps are `None` unless asked for).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LengthDelayStats {
    /// `NumPads`
    pub num_pads: i32,
    /// `NumVias`
    pub num_vias: i32,
    /// `ViaLength`
    pub via_length: i32,
    /// `TrackLength`
    pub track_length: i64,
    /// `PadToDieLength`
    pub pad_to_die_length: i32,
    /// `LayerLengths`
    pub layer_lengths: Option<BTreeMap<i32, i64>>,
    /// `ViaDelay`
    pub via_delay: i64,
    /// `TrackDelay`
    pub track_delay: i64,
    /// `PadToDieDelay`
    pub pad_to_die_delay: i64,
    /// `LayerDelays`
    pub layer_delays: Option<BTreeMap<i32, i64>>,
}

impl LengthDelayStats {
    /// `TotalLength()`: the total electrical length.
    pub fn total_length(&self) -> i64 {
        i64::from(self.via_length) + self.track_length + i64::from(self.pad_to_die_length)
    }

    /// `TotalDelay()`: the total electrical propagation delay.
    pub fn total_delay(&self) -> i64 {
        self.via_delay + self.track_delay + self.pad_to_die_delay
    }
}

/// Port of `PATH_OPTIMISATIONS`: which optimisations the length calculation runs on the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathOptimisations {
    /// `OptimiseVias`: effective via span and trace clipping inside via pads.
    pub optimise_vias: bool,
    /// `MergeTracks`: merge all contiguous (end-to-end, same layer) tracks.
    pub merge_tracks: bool,
    /// `OptimiseTracesInPads`: optimise the electrical length of tracks within pads.
    pub optimise_traces_in_pads: bool,
    /// `InferViaInPad`: a via-in-pad present on the board but not in the item set.
    pub infer_via_in_pad: bool,
}

/// Port of `LENGTH_DELAY_LAYER_OPT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthDelayLayerOpt {
    /// `NO_LAYER_DETAIL`
    NoLayerDetail,
    /// `WITH_LAYER_DETAIL`
    WithLayerDetail,
}

/// Port of `LENGTH_DELAY_DOMAIN_OPT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthDelayDomainOpt {
    /// `NO_DELAY_DETAIL`
    NoDelayDetail,
    /// `WITH_DELAY_DETAIL`
    WithDelayDetail,
}

/// Port of `LENGTH_DELAY_ITEM_DETAILS`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LengthDelayItemDetails {
    /// `LengthsAndDelays`
    pub lengths_and_delays: Vec<(i64, i64)>,
    /// `InferredStartViaLength`
    pub inferred_start_via_length: i64,
    /// `InferredEndViaLength`
    pub inferred_end_via_length: i64,
    /// `InferredStartViaDelay`
    pub inferred_start_via_delay: i64,
    /// `InferredEndViaDelay`
    pub inferred_end_via_delay: i64,
}

/// `LENGTH_DELAY_CALCULATION_ITEM::TYPE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LdcItemType {
    /// `UNKNOWN`
    #[default]
    Unknown,
    /// `PAD`
    Pad,
    /// `LINE`
    Line,
    /// `VIA`
    Via,
}

/// `LENGTH_DELAY_CALCULATION_ITEM::MERGE_STATUS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeStatus {
    /// `UNMERGED`
    #[default]
    Unmerged,
    /// `MERGED_IN_USE`
    MergedInUse,
    /// `MERGED_RETIRED`
    MergedRetired,
}

/// Port of `LENGTH_DELAY_CALCULATION_ITEM`: one item of a length calculation path.
#[derive(Clone, Debug)]
pub struct LengthDelayCalculationItem {
    /// `m_line`
    line: ShapeLineChain,
    /// `m_pad`
    pad: Option<ItemId>,
    /// `m_via`
    via: Option<ItemId>,
    /// `m_layerStart`
    layer_start: PcbLayerId,
    /// `m_layerEnd`
    layer_end: PcbLayerId,
    /// `m_mergeStatus`
    merge_status: MergeStatus,
    /// `m_type`
    type_: LdcItemType,
    /// `m_netClass->GetTuningProfile()` (`None`: no net class).
    tuning_profile: Option<String>,
}

impl Default for LengthDelayCalculationItem {
    fn default() -> Self {
        LengthDelayCalculationItem {
            line: ShapeLineChain::new(),
            pad: None,
            via: None,
            layer_start: PcbLayerId::UndefinedLayer,
            layer_end: PcbLayerId::UndefinedLayer,
            merge_status: MergeStatus::Unmerged,
            type_: LdcItemType::Unknown,
            tuning_profile: None,
        }
    }
}

impl LengthDelayCalculationItem {
    /// `Type()`.
    pub fn type_(&self) -> LdcItemType {
        self.type_
    }

    /// `SetPad()`.
    pub fn set_pad(&mut self, pad: ItemId) {
        self.pad = Some(pad);
        self.type_ = LdcItemType::Pad;
    }

    /// `GetPad()`.
    pub fn pad(&self) -> Option<ItemId> {
        self.pad
    }

    /// `SetLine()`.
    pub fn set_line(&mut self, line: ShapeLineChain) {
        self.line = line;
        self.type_ = LdcItemType::Line;
    }

    /// `GetLine()`.
    pub fn line(&self) -> &ShapeLineChain {
        &self.line
    }

    /// `GetLine()` (mutable).
    pub fn line_mut(&mut self) -> &mut ShapeLineChain {
        &mut self.line
    }

    /// `SetVia()`.
    pub fn set_via(&mut self, via: ItemId) {
        self.via = Some(via);
        self.type_ = LdcItemType::Via;
    }

    /// `GetVia()`.
    pub fn via(&self) -> Option<ItemId> {
        self.via
    }

    /// `SetLayers( aStart, aEnd )`: always stored in copper layer order (F_Cu to B_Cu).
    pub fn set_layers(&mut self, start: PcbLayerId, end: PcbLayerId) {
        if end != PcbLayerId::UndefinedLayer && is_copper_layer_lower_than(start, end) {
            self.layer_start = end;
            self.layer_end = start;
        } else {
            self.layer_start = start;
            self.layer_end = end;
        }
    }

    /// `SetMergeStatus()`.
    pub fn set_merge_status(&mut self, status: MergeStatus) {
        self.merge_status = status;
    }

    /// `GetMergeStatus()`.
    pub fn merge_status(&self) -> MergeStatus {
        self.merge_status
    }

    /// `GetLayers()`.
    pub fn layers(&self) -> (PcbLayerId, PcbLayerId) {
        (self.layer_start, self.layer_end)
    }

    /// `GetStartLayer()`.
    pub fn start_layer(&self) -> PcbLayerId {
        self.layer_start
    }

    /// `GetEndLayer()`.
    pub fn end_layer(&self) -> PcbLayerId {
        self.layer_end
    }

    /// `SetEffectiveNetClass()`: the class's tuning profile name.
    pub fn set_effective_tuning_profile(&mut self, profile: Option<String>) {
        self.tuning_profile = profile;
    }

    /// `GetEffectiveNetClass()->GetTuningProfile()` (empty without a class).
    pub fn effective_tuning_profile(&self) -> &str {
        self.tuning_profile.as_deref().unwrap_or("")
    }

    /// Port of `LENGTH_DELAY_CALCULATION_ITEM::CalculateViaLayers`: the electrical span of the
    /// via from the layers where traces connect to it.
    pub fn calculate_via_layers(&mut self, ctx: &BoardCtx<'_>, conn: Option<&ConnectivityData>) {
        // Only consider trace connections when determining via electrical span. Pads are
        // excluded because they have their own pad-to-die length handling, and including them
        // would cause through-hole vias to incorrectly span to layers where TH pads exist
        // (e.g., connector pins) rather than just the layers where traces actually connect.
        let trace_types = [KicadT::PcbTraceT, KicadT::PcbArcT];

        let mut top_layer = PcbLayerId::UndefinedLayer;
        let mut bottom_layer = PcbLayerId::UndefinedLayer;

        let via_id = self.via.expect("a via item");
        let via_ref = ctx.item(via_id).expect("a board via");
        let via = via_ref.as_via().expect("a via");

        let layers = ctx.board.design_settings().enabled_layers().clone();

        for layer in layers.copper_layers() {
            let connected =
                conn.is_some_and(|c| c.is_connected_on_layer(ctx, via_ref, layer.0, &trace_types));

            if connected {
                if top_layer == PcbLayerId::UndefinedLayer {
                    top_layer = layer;
                } else {
                    bottom_layer = layer;
                }
            }
        }

        if top_layer == PcbLayerId::UndefinedLayer {
            top_layer = via.top_layer();
        }

        if bottom_layer == PcbLayerId::UndefinedLayer {
            bottom_layer = via.bottom_layer();
        }

        self.set_layers(top_layer, bottom_layer);
    }
}

/// `MERGE_POINT`: whether track merging is attempted from the start or end of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergePoint {
    Start,
    End,
}

/// The position maps of `CalculateLengthDetails`: item indices per endpoint.
type PositionMap = BTreeMap<(i32, i32), Vec<usize>>;

fn pos_key(p: Vector2I) -> (i32, i32) {
    (p.x, p.y)
}

/// `std::unordered_set::insert` on a bucket (no duplicates).
fn bucket_insert(map: &mut PositionMap, p: Vector2I, idx: usize) {
    let bucket = map.entry(pos_key(p)).or_default();

    if !bucket.contains(&idx) {
        bucket.push(idx);
    }
}

/// `VIA_OVERRIDE_CACHE_KEY`: signal start / end and via start / end layers.
type ViaOverrideKey = (i32, i32, i32, i32);

/// Port of `LENGTH_DELAY_CALCULATION` with its default provider of tuning profile parameters
/// (`TUNING_PROFILE_PARAMETERS_USER_DEFINED`).
pub struct LengthDelayCalculation<'a> {
    /// `m_board`
    ctx: &'a BoardCtx<'a>,
    /// `m_board->GetConnectivity()`
    conn: Option<&'a ConnectivityData>,
    /// `m_delayProfilesCache`
    delay_profiles: BTreeMap<String, TuningProfile>,
    /// `m_viaOverridesCache`
    via_overrides: BTreeMap<String, BTreeMap<ViaOverrideKey, i64>>,
}

impl<'a> LengthDelayCalculation<'a> {
    /// `LENGTH_DELAY_CALCULATION( aBoard )` followed by `SynchronizeTuningProfileProperties()`
    /// (`TUNING_PROFILE_PARAMETERS_USER_DEFINED::rebuildCaches`).
    pub fn new(ctx: &'a BoardCtx<'a>, conn: Option<&'a ConnectivityData>) -> Self {
        let mut calc = LengthDelayCalculation {
            ctx,
            conn,
            delay_profiles: BTreeMap::new(),
            via_overrides: BTreeMap::new(),
        };

        calc.rebuild_caches();
        calc
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::rebuildCaches`.
    fn rebuild_caches(&mut self) {
        self.delay_profiles.clear();
        self.via_overrides.clear();

        if let Some(project) = self.ctx.board.project.as_ref() {
            let (params, _) = load_tuning_profiles(Some(&project.document));

            for profile in params.tuning_profiles {
                let overrides = self
                    .via_overrides
                    .entry(profile.profile_name.clone())
                    .or_default();

                for via_override in &profile.via_overrides {
                    overrides.insert(
                        (
                            via_override.signal_layer_from.0,
                            via_override.signal_layer_to.0,
                            via_override.via_layer_from.0,
                            via_override.via_layer_to.0,
                        ),
                        i64::from(via_override.delay),
                    );
                }

                self.delay_profiles
                    .insert(profile.profile_name.clone(), profile);
            }
        }
    }

    /// The board context of the calculation.
    pub fn ctx(&self) -> &'a BoardCtx<'a> {
        self.ctx
    }

    fn item(&self, id: ItemId) -> ItemRef<'a> {
        self.ctx
            .item(id)
            .expect("a board item of the length calculation")
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::GetTuningProfile`.
    pub fn tuning_profile(&self, name: &str) -> Option<&TuningProfile> {
        self.delay_profiles.get(name)
    }

    /// Port of `LENGTH_DELAY_CALCULATION::findArcPadIntersection`.
    fn find_arc_pad_intersection(
        arc: &ShapeArc,
        pad_shape: &ShapePolySet,
        inside_point: Vector2I,
    ) -> Option<Vector2I> {
        let mut found = None;
        let mut best_dist_sq = i64::MAX;

        for i in 0..pad_shape.outline_count() {
            let outline = pad_shape.c_outline(i);

            for j in 0..outline.segment_count() {
                let seg = outline.c_segment(j);
                let mut intersections = Vec::new();

                if arc.intersect_line(&seg, &mut intersections) == 0 {
                    continue;
                }

                for &pt in &intersections {
                    if !seg.contains(pt) {
                        continue;
                    }

                    let dist_sq = (pt - inside_point).squared_euclidean_norm();

                    if dist_sq < best_dist_sq {
                        best_dist_sq = dist_sq;
                        found = Some(pt);
                    }
                }
            }
        }

        found
    }

    /// Port of `LENGTH_DELAY_CALCULATION::clipLineToPad`.
    fn clip_line_to_pad(
        &self,
        line: &mut ShapeLineChain,
        pad: ItemRef<'_>,
        layer: PcbLayerId,
        forward: bool,
    ) {
        debug_assert!(line.point_count() >= 2);

        let start = if forward { 0 } else { line.point_count() - 1 };
        let delta = if forward { 1 } else { -1 };

        let pad_obj = pad.as_pad().expect("a pad");
        let shape = pad_obj.effective_polygon(self.ctx, layer, ErrorLoc::Inside);

        // Find the first point OUTSIDE the pad
        let mut first_outside = -1;
        let mut intersection_pt = Vector2I::default();
        let mut has_intersection = false;

        let mut vertex = start + delta;

        while if forward {
            vertex < line.point_count()
        } else {
            vertex >= 0
        } {
            if !shape.contains(line.c_point(vertex), -1, 0, false) {
                first_outside = vertex;
                let prev_vertex = vertex - delta;

                // Check if the crossing segment is part of an arc
                let arc_idx = line.arc_index(prev_vertex as usize);

                if arc_idx >= 0 && line.arc_index(vertex as usize) == arc_idx {
                    if let Some(pt) = Self::find_arc_pad_intersection(
                        line.arc(arc_idx as usize),
                        &shape,
                        line.c_point(prev_vertex),
                    ) {
                        intersection_pt = pt;
                        has_intersection = true;
                    }
                }

                // Fallback to segment intersection if arc intersection didn't work
                if !has_intersection {
                    let seg = Seg::new(line.c_point(vertex), line.c_point(prev_vertex));
                    let mut loc = Vector2I::default();

                    if shape.collide_seg(&seg, 0, None, Some(&mut loc)) {
                        intersection_pt = loc;
                        has_intersection = true;
                    }
                }

                break;
            }

            vertex += delta;
        }

        if first_outside < 0 {
            return; // All points inside pad, nothing to clip
        }

        // Build new chain using Slice (preserves arcs correctly without index corruption)
        let mut new_chain = ShapeLineChain::new();
        let pad_pos = pad.position();

        if forward {
            // Chain: padCenter -> intersection -> [firstOutside to end]
            new_chain.append_point(pad_pos, false);

            if has_intersection {
                new_chain.append_point(intersection_pt, false);
            }

            new_chain.append_chain(&line.slice(first_outside, -1));
        } else {
            // Chain: [0 to firstOutside] -> intersection -> padCenter
            new_chain.append_chain(&line.slice(0, first_outside));

            if has_intersection {
                new_chain.append_point(intersection_pt, false);
            }

            new_chain.append_point(pad_pos, false);
        }

        *line = new_chain;
    }

    /// Port of `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`: the electrical length (and,
    /// with `domain`, the delay) of the given items after the asked optimisations. The items
    /// are modified in place (merged / clipped lines, optimised via spans), as in C++.
    #[allow(clippy::too_many_arguments)]
    pub fn calculate_length_details(
        &self,
        items: &mut [LengthDelayCalculationItem],
        optimisations: PathOptimisations,
        start_pad: Option<ItemId>,
        end_pad: Option<ItemId>,
        layer_opt: LengthDelayLayerOpt,
        domain: LengthDelayDomainOpt,
        mut per_item: Option<&mut LengthDelayItemDetails>,
    ) -> LengthDelayStats {
        // If this set of items has not been optimised, optimise for shortest electrical path
        // (the C++ tests MergeTracks twice; OptimiseTracesInPads alone does not trigger it)
        if optimisations.optimise_vias || optimisations.merge_tracks || optimisations.merge_tracks {
            let mut pads: Vec<usize> = Vec::new();
            let mut lines: Vec<usize> = Vec::new();
            let mut vias: Vec<usize> = Vec::new();

            // Map of line endpoints to line objects
            let mut lines_position_map = PositionMap::new();

            // Map of pad positions to pad objects
            let mut pads_position_map = PositionMap::new();

            for (idx, item) in items.iter().enumerate() {
                match item.type_() {
                    LdcItemType::Pad => {
                        pads.push(idx);
                        let pos = self.item(item.pad().expect("a pad")).position();
                        bucket_insert(&mut pads_position_map, pos, idx);
                    }
                    LdcItemType::Via => vias.push(idx),
                    LdcItemType::Line => {
                        lines.push(idx);
                        bucket_insert(&mut lines_position_map, item.line().c_point(0), idx);
                        bucket_insert(&mut lines_position_map, item.line().c_last_point(), idx);
                    }
                    LdcItemType::Unknown => {}
                }
            }

            if optimisations.optimise_vias {
                self.optimise_vias(items, &vias, &lines_position_map, &pads_position_map);
            }

            if optimisations.merge_tracks {
                Self::merge_lines(items, &lines, &lines_position_map);
            }

            // Clip traces inside VIA pads after merging
            if optimisations.optimise_vias {
                for &via in &vias {
                    let pcb_via = self.item(items[via].via().expect("a via"));

                    for &line_item in &lines {
                        if items[line_item].merge_status() != MergeStatus::MergedInUse {
                            continue;
                        }

                        let layer = items[line_item].start_layer();
                        self.optimise_trace_in_via(items[line_item].line_mut(), pcb_via, layer);
                    }
                }
            }

            if optimisations.optimise_traces_in_pads {
                self.optimise_traces_in_pads(items, &pads, &lines);
            }
        }

        let mut details = LengthDelayStats::default();

        // Create the layer detail maps if required
        if layer_opt == LengthDelayLayerOpt::WithLayerDetail {
            details.layer_lengths = Some(BTreeMap::new());

            if domain == LengthDelayDomainOpt::WithDelayDetail {
                details.layer_delays = Some(BTreeMap::new());
            }
        }

        let use_height = self.ctx.board.design_settings().use_height_for_length_calcs;

        // If this is a contiguous set of items, check if we have an inferred fanout via at
        // either end. Note that this condition only arises as a result of how PNS assembles
        // tuning paths - for DRC / net inspector calculations these fanout vias will be present
        // in the object set and therefore do not need to be inferred
        if optimisations.infer_via_in_pad && use_height {
            let with_delay = domain == LengthDelayDomainOpt::WithDelayDetail;
            let inferred_start =
                self.infer_via_in_pad(start_pad, &items[0], &mut details, with_delay);
            let inferred_end =
                self.infer_via_in_pad(end_pad, &items[items.len() - 1], &mut details, with_delay);

            if let Some(per_item) = per_item.as_deref_mut() {
                per_item.inferred_start_via_length = inferred_start.0;
                per_item.inferred_start_via_delay = inferred_start.1;
                per_item.inferred_end_via_length = inferred_end.0;
                per_item.inferred_end_via_delay = inferred_end.1;
            }
        }

        // Output per-item details if required
        if let Some(per_item) = per_item.as_deref_mut() {
            per_item.lengths_and_delays.clear();
            per_item.lengths_and_delays.resize(items.len(), (0, 0));
        }

        for (i, item) in items.iter().enumerate() {
            // Don't include merged items
            if item.merge_status() == MergeStatus::MergedRetired
                || item.type_() == LdcItemType::Unknown
            {
                continue;
            }

            // Calculate the space domain statistics
            if item.type_() == LdcItemType::Line {
                let length = item.line().length();

                if let Some(per_item) = per_item.as_deref_mut() {
                    per_item.lengths_and_delays[i].0 = length;
                }

                details.track_length += length;

                if let Some(layer_lengths) = details.layer_lengths.as_mut() {
                    *layer_lengths.entry(item.start_layer().0).or_default() += length;
                }
            } else if item.type_() == LdcItemType::Via && use_height {
                let (layer_start, layer_end) = item.layers();
                let via_height = i64::from(self.stackup_height(layer_start, layer_end));

                if let Some(per_item) = per_item.as_deref_mut() {
                    per_item.lengths_and_delays[i].0 = via_height;
                }

                details.via_length = details.via_length.wrapping_add(via_height as i32);
                details.num_vias += 1;
            } else if item.type_() == LdcItemType::Pad {
                let pad = self.item(item.pad().expect("a pad"));
                let pad_to_die = i64::from(pad.as_pad().expect("a pad").pad_to_die_length());

                if let Some(per_item) = per_item.as_deref_mut() {
                    per_item.lengths_and_delays[i].0 = pad_to_die;
                }

                details.pad_to_die_length =
                    details.pad_to_die_length.wrapping_add(pad_to_die as i32);
                details.num_pads += 1;
            }
        }

        // Calculate the time domain statistics if required
        if domain == LengthDelayDomainOpt::WithDelayDetail && !items.is_empty() {
            let item_delays = self.get_propagation_delays(items);

            debug_assert_eq!(item_delays.len(), items.len());

            for (i, item) in items.iter().enumerate() {
                if item.merge_status() == MergeStatus::MergedRetired
                    || item.type_() == LdcItemType::Unknown
                {
                    continue;
                }

                if let Some(per_item) = per_item.as_deref_mut() {
                    per_item.lengths_and_delays[i].1 = item_delays[i];
                }

                if item.type_() == LdcItemType::Line {
                    details.track_delay += item_delays[i];

                    if let Some(layer_delays) = details.layer_delays.as_mut() {
                        *layer_delays.entry(item.start_layer().0).or_default() += item_delays[i];
                    }
                } else if item.type_() == LdcItemType::Via && use_height {
                    details.via_delay += item_delays[i];
                } else if item.type_() == LdcItemType::Pad {
                    details.pad_to_die_delay += item_delays[i];
                }
            }
        }

        details
    }

    /// Port of `LENGTH_DELAY_CALCULATION::inferViaInPad`: adds a via in `pad` to `details`
    /// when the line starts on a layer the pad is not on. Returns the inferred (length, delay).
    fn infer_via_in_pad(
        &self,
        pad: Option<ItemId>,
        item: &LengthDelayCalculationItem,
        details: &mut LengthDelayStats,
        with_delay_detail: bool,
    ) -> (i64, i64) {
        let mut inferred = (0, 0);

        if let Some(pad) = pad
            && item.type_() == LdcItemType::Line
        {
            let start_bottom_layer = item.start_layer();
            let pad_ref = self.item(pad);
            let pad_layers = pad_ref.layer_set();

            if !pad_layers.contains(start_bottom_layer) {
                // This must be either F_Cu or B_Cu
                let pad_layer = if pad_layers.contains(PcbLayerId::FCu) {
                    PcbLayerId::FCu
                } else {
                    PcbLayerId::BCu
                };

                details.num_vias += 1;
                let height = i64::from(self.stackup_height(start_bottom_layer, pad_layer));
                details.via_length = (i64::from(details.via_length) + height) as i32;
                inferred.0 = height;

                // Look up via delay details if required
                if with_delay_detail {
                    let delay = self.get_via_propagation_delay(
                        start_bottom_layer,
                        pad_layer,
                        PcbLayerId::FCu,
                        PcbLayerId::BCu,
                        item.effective_tuning_profile(),
                    );
                    details.via_delay += delay;
                    inferred.1 = delay;
                }
            }
        }

        inferred
    }

    /// Port of `LENGTH_DELAY_CALCULATION::CalculateLength`.
    pub fn calculate_length(
        &self,
        items: &mut [LengthDelayCalculationItem],
        optimisations: PathOptimisations,
        start_pad: Option<ItemId>,
        end_pad: Option<ItemId>,
    ) -> i64 {
        self.calculate_length_details(
            items,
            optimisations,
            start_pad,
            end_pad,
            LengthDelayLayerOpt::NoLayerDetail,
            LengthDelayDomainOpt::NoDelayDetail,
            None,
        )
        .total_length()
    }

    /// Port of `LENGTH_DELAY_CALCULATION::CalculateDelay`.
    pub fn calculate_delay(
        &self,
        items: &mut [LengthDelayCalculationItem],
        optimisations: PathOptimisations,
        start_pad: Option<ItemId>,
        end_pad: Option<ItemId>,
    ) -> i64 {
        self.calculate_length_details(
            items,
            optimisations,
            start_pad,
            end_pad,
            LengthDelayLayerOpt::NoLayerDetail,
            LengthDelayDomainOpt::WithDelayDetail,
            None,
        )
        .total_delay()
    }

    /// Port of `LENGTH_DELAY_CALCULATION::StackupHeight`: the stackup distance between the two
    /// layers (0 when the design settings disallow stackup height calculations).
    pub fn stackup_height(&self, first_layer: PcbLayerId, second_layer: PcbLayerId) -> i32 {
        let bds = self.ctx.board.design_settings();

        if !bds.use_height_for_length_calcs {
            return 0;
        }

        if bds.has_stackup {
            bds.stackup_descriptor()
                .get_layer_distance(first_layer, second_layer)
        } else {
            let mut stackup = BoardStackup::default();
            stackup.build_default_stackup_list(Some(bds), self.ctx.board.copper_layer_count());
            stackup.get_layer_distance(first_layer, second_layer)
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::mergeLines`: merges any lines (traces) that are
    /// contiguous, on one layer, and with no junctions.
    fn merge_lines(
        items: &mut [LengthDelayCalculationItem],
        lines: &[usize],
        lines_position_map: &PositionMap,
    ) {
        // Attempts to merge unmerged lines in to the primary line
        let try_merge = |items: &mut [LengthDelayCalculationItem],
                         merge_point: MergePoint,
                         merge_pos: Vector2I,
                         primary: usize|
         -> bool {
            let Some(bucket) = lines_position_map.get(&pos_key(merge_pos)) else {
                return false;
            };

            // The map is keyed on position alone, so other layers share the bucket.
            let mut line_to_merge = None;
            let mut same_layer = 0;

            for &item in bucket {
                if items[item].start_layer() != items[primary].start_layer() {
                    continue;
                }

                same_layer += 1;

                if item != primary && items[item].merge_status() == MergeStatus::Unmerged {
                    line_to_merge = Some(item);
                }
            }

            // Two ends on this layer is a simple joint. More is a branch.
            let Some(line_to_merge) = line_to_merge else {
                return false;
            };

            if same_layer != 2 {
                return false;
            }

            // Merge the lines
            items[line_to_merge].set_merge_status(MergeStatus::MergedRetired);
            let secondary = items[line_to_merge].line().clone();
            Self::merge_shape_line_chains(items[primary].line_mut(), &secondary, merge_point);
            true
        };

        // Cluster all lines in to contiguous entities
        for &primary in lines {
            // Don't start with an already merged line
            if items[primary].merge_status() != MergeStatus::Unmerged {
                continue;
            }

            // Merge all endpoints
            items[primary].set_merge_status(MergeStatus::MergedInUse);
            let mut merge_complete = false;

            while !merge_complete {
                let start_pos = items[primary].line().c_point(0);
                let end_pos = items[primary].line().c_last_point();

                let start_merged = try_merge(items, MergePoint::Start, start_pos, primary);
                let end_merged = try_merge(items, MergePoint::End, end_pos, primary);

                merge_complete = !start_merged && !end_merged;
            }
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::mergeShapeLineChains`: `secondary` is merged in to
    /// `primary` at their shared endpoint.
    fn merge_shape_line_chains(
        primary: &mut ShapeLineChain,
        secondary: &ShapeLineChain,
        merge_point: MergePoint,
    ) {
        // Append carries the arcs across and drops the shared point. Orient the
        // secondary so its joining end meets the primary.
        if merge_point == MergePoint::Start {
            // Secondary joins the primary's start, so build secondary + primary.
            let mut merged = if secondary.c_point(0) == primary.c_point(0) {
                secondary.reverse()
            } else {
                secondary.clone()
            };

            debug_assert!(merged.c_last_point() == primary.c_point(0));

            merged.append_chain(primary);
            *primary = merged;
        } else if secondary.c_point(0) == primary.c_last_point() {
            primary.append_chain(secondary);
        } else {
            debug_assert!(secondary.c_last_point() == primary.c_last_point());
            primary.append_chain(&secondary.reverse());
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::optimiseTracesInPads`.
    fn optimise_traces_in_pads(
        &self,
        items: &mut [LengthDelayCalculationItem],
        pads: &[usize],
        lines: &[usize],
    ) {
        for &pad_item in pads {
            let pad = self.item(items[pad_item].pad().expect("a pad"));

            for &line_item in lines {
                // Ignore merged lines
                if items[line_item].merge_status() != MergeStatus::MergedInUse {
                    continue;
                }

                let pcb_layer = items[line_item].start_layer();
                self.optimise_trace_in_pad(items[line_item].line_mut(), pad, pcb_layer);
            }
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::optimiseVias`: vias that are routed through only on
    /// one layer do not count towards the total length.
    fn optimise_vias(
        &self,
        items: &mut [LengthDelayCalculationItem],
        vias: &[usize],
        lines_position_map: &PositionMap,
        _pads_position_map: &PositionMap,
    ) {
        for &via in vias {
            let via_id = items[via].via().expect("a via");
            let via_ref = self.item(via_id);
            let pcb_via = via_ref.as_via().expect("a via");

            let mut connected_lines: Vec<usize> = Vec::new();
            let insert = |idx: usize, lines: &mut Vec<usize>| {
                if !lines.contains(&idx) {
                    lines.push(idx);
                }
            };

            let via_pos = via_ref.position();

            if let Some(exact) = lines_position_map.get(&pos_key(via_pos)) {
                for &idx in exact {
                    insert(idx, &mut connected_lines);
                }
            }

            let mut max_radius = 0;

            pcb_via.padstack().for_each_unique_layer(|layer| {
                max_radius = max_radius.max(pcb_via.width(layer) / 2);
            });

            let max_radius_sq = i64::from(max_radius) * i64::from(max_radius);

            for (&(x, y), line_set) in lines_position_map {
                let pos = Vector2I::new(x, y);

                if pos == via_pos {
                    continue;
                }

                if (pos - via_pos).squared_euclidean_norm() > max_radius_sq {
                    continue;
                }

                for &line_item in line_set {
                    let layer = items[line_item].start_layer();
                    let shape = via_ref.effective_shape(
                        layer,
                        Flashing::AlwaysFlashed,
                        DrcConstraintT::NullConstraint,
                    );

                    if shape.collide_point(pos, 0, None, None) {
                        insert(line_item, &mut connected_lines);
                    }
                }
            }

            // GetConnectedPads( via ): the first pad (heap order in C++, ItemId order here).
            let coincident_pad = self
                .conn
                .and_then(|c| c.connected_pads(via_id).into_iter().next())
                .and_then(|id| self.ctx.item(id));

            let mut span_layers = Lset::new();

            for &line_item in &connected_lines {
                span_layers.set(items[line_item].start_layer());
            }

            if let Some(pad_ref) = coincident_pad {
                let pad = pad_ref.as_pad().expect("a pad");

                // *CuStack().begin() of a pad without copper layers is undefined in C++; the
                // port leaves the span unchanged then.
                let pad_side_layer = if matches!(
                    pad.attribute(),
                    PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent
                ) {
                    pad_ref.layer_set().cu_stack().0.first().copied()
                } else {
                    pad_ref.parent_footprint().map(|fp| fp.layer())
                };

                if let Some(pad_side_layer) = pad_side_layer {
                    span_layers.set(pad_side_layer);
                }
            }

            let cu_stack = span_layers.cu_stack().0;

            if cu_stack.is_empty() {
                // Nothing connects to this via
                let layer = via_ref.layer();
                items[via].set_layers(layer, layer);
            } else if cu_stack.len() == 1 {
                // Stub via
                items[via].set_layers(cu_stack[0], cu_stack[0]);
            } else {
                // Signal transitions layers
                items[via].set_layers(cu_stack[0], cu_stack[cu_stack.len() - 1]);
            }
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::OptimiseTraceInPad`: optimises the given trace to
    /// minimise the electrical path length within the pad.
    pub fn optimise_trace_in_pad(
        &self,
        line: &mut ShapeLineChain,
        pad: ItemRef<'_>,
        pcb_layer: PcbLayerId,
    ) {
        let pad_pos = pad.position();

        // Only consider lines which terminate in the pad
        if line.c_point(0) != pad_pos && line.c_last_point() != pad_pos {
            return;
        }

        let pad_obj = pad.as_pad().expect("a pad");

        if !pad_obj.flash_layer(self.ctx, pcb_layer) {
            return;
        }

        let shape = pad_obj.effective_polygon(self.ctx, pcb_layer, ErrorLoc::Inside);

        if shape.contains(line.c_point(0), -1, 0, false) {
            self.clip_line_to_pad(line, pad, pcb_layer, true);
        } else if shape.contains(line.c_last_point(), -1, 0, false) {
            self.clip_line_to_pad(line, pad, pcb_layer, false);
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::clipLineToVia`.
    fn clip_line_to_via(
        line: &mut ShapeLineChain,
        via: ItemRef<'_>,
        layer: PcbLayerId,
        forward: bool,
    ) {
        debug_assert!(line.point_count() >= 2);

        let start = if forward { 0 } else { line.point_count() - 1 };
        let delta = if forward { 1 } else { -1 };

        let via_shape = via.effective_shape(
            layer,
            Flashing::AlwaysFlashed,
            DrcConstraintT::NullConstraint,
        );

        let ShapeRef::Circle(via_circle) = via_shape.as_shape_ref() else {
            return;
        };

        // Find the first point OUTSIDE the via pad
        let mut first_outside = -1;
        let mut intersection_pt = Vector2I::default();
        let mut has_intersection = false;

        let mut vertex = start + delta;

        while if forward {
            vertex < line.point_count()
        } else {
            vertex >= 0
        } {
            if !via_shape.collide_point(line.c_point(vertex), 0, None, None) {
                first_outside = vertex;
                let prev_vertex = vertex - delta;

                let seg = Seg::new(line.c_point(vertex), line.c_point(prev_vertex));
                let circle = Circle::new(via_circle.get_center(), via_circle.get_radius());
                let pts = circle.intersect_seg(&seg);

                if !pts.is_empty() {
                    // Pick the intersection closest to the outside vertex
                    let outside = line.c_point(vertex);
                    intersection_pt = pts[0];

                    for &pt in &pts[1..] {
                        if (pt - outside).squared_euclidean_norm()
                            < (intersection_pt - outside).squared_euclidean_norm()
                        {
                            intersection_pt = pt;
                        }
                    }

                    has_intersection = true;
                }

                break;
            }

            vertex += delta;
        }

        if first_outside < 0 {
            return;
        }

        let mut new_chain = ShapeLineChain::new();
        let via_pos = via.position();

        if forward {
            // viaCenter -> intersection -> [firstOutside to end]
            new_chain.append_point(via_pos, false);

            if has_intersection {
                new_chain.append_point(intersection_pt, false);
            }

            new_chain.append_chain(&line.slice(first_outside, -1));
        } else {
            // [0 to firstOutside] -> intersection -> viaCenter
            new_chain.append_chain(&line.slice(0, first_outside));

            if has_intersection {
                new_chain.append_point(intersection_pt, false);
            }

            new_chain.append_point(via_pos, false);
        }

        *line = new_chain;
    }

    /// Port of `LENGTH_DELAY_CALCULATION::OptimiseTraceInVia`: clips trace portions inside a
    /// via pad and replaces them with a straight segment to the via centre.
    pub fn optimise_trace_in_via(
        &self,
        line: &mut ShapeLineChain,
        via: ItemRef<'_>,
        layer: PcbLayerId,
    ) {
        let via_shape = via.effective_shape(
            layer,
            Flashing::AlwaysFlashed,
            DrcConstraintT::NullConstraint,
        );

        if via_shape.collide_point(line.c_point(0), 0, None, None) {
            Self::clip_line_to_via(line, via, layer, true);
        } else if via_shape.collide_point(line.c_last_point(), 0, None, None) {
            Self::clip_line_to_via(line, via, layer, false);
        }
    }

    /// Port of `LENGTH_DELAY_CALCULATION::IsPointInsideViaPad`.
    pub fn is_point_inside_via_pad(via: ItemRef<'_>, point: Vector2I, layer: PcbLayerId) -> bool {
        let shape = via.effective_shape(
            layer,
            Flashing::AlwaysFlashed,
            DrcConstraintT::NullConstraint,
        );
        shape.collide_point(point, 0, None, None)
    }

    /// Port of `LENGTH_DELAY_CALCULATION::GetLengthCalculationItem`: the calculation item of a
    /// board item (`UNKNOWN` for anything but tracks, arcs, vias and pads).
    pub fn length_calculation_item(&self, board_item: ItemRef<'_>) -> LengthDelayCalculationItem {
        let netclass_profile = |item: &ItemRef<'_>| {
            item.effective_netclass()
                .map(|nc| nc.tuning_profile().to_string())
        };

        if let Some(track) = board_item.as_track() {
            let id = board_item.id().expect("a board item");

            match track {
                Track::Via(_) => {
                    let mut item = LengthDelayCalculationItem::default();
                    item.set_via(id);
                    item.calculate_via_layers(self.ctx, self.conn);
                    item.set_effective_tuning_profile(netclass_profile(&board_item));
                    return item;
                }
                Track::Arc(arc) => {
                    let base = track.as_track();
                    let shape_arc = ShapeArc::from_start_mid_end(
                        base.start(),
                        arc.mid(),
                        base.end(),
                        base.width(),
                    );
                    let chain_arc = ShapeLineChain::from_arc(&shape_arc, false, None);

                    let mut item = LengthDelayCalculationItem::default();
                    item.set_line(chain_arc);
                    item.set_layers(board_item.layer(), PcbLayerId::UndefinedLayer);
                    item.set_effective_tuning_profile(netclass_profile(&board_item));
                    return item;
                }
                Track::Trace(t) => {
                    let points = [t.start(), t.end()];
                    let shape = ShapeLineChain::from_points(&points, false);

                    let mut item = LengthDelayCalculationItem::default();
                    item.set_line(shape);
                    item.set_layers(board_item.layer(), PcbLayerId::UndefinedLayer);
                    item.set_effective_tuning_profile(netclass_profile(&board_item));
                    return item;
                }
            }
        } else if board_item.as_pad().is_some() {
            let mut item = LengthDelayCalculationItem::default();
            item.set_pad(board_item.id().expect("a board item"));

            let layers = board_item.layer_set();
            let mut first_layer = PcbLayerId::UndefinedLayer;
            let mut second_layer = PcbLayerId::UndefinedLayer;

            for layer in layers.copper_layers() {
                if first_layer == PcbLayerId::UndefinedLayer {
                    first_layer = layer;
                } else {
                    second_layer = layer;
                }
            }

            item.set_layers(first_layer, second_layer);
            item.set_effective_tuning_profile(netclass_profile(&board_item));
            return item;
        }

        LengthDelayCalculationItem::default()
    }

    /// Port of `LENGTH_DELAY_CALCULATION::CalculateLengthForDelay` (user-defined provider:
    /// `GetTrackLengthForPropagationDelay`).
    pub fn calculate_length_for_delay(
        &self,
        desired_delay: i64,
        tuning_profile: &str,
        layer: PcbLayerId,
    ) -> i64 {
        let delay_unit = self.track_delay_per_mm(tuning_profile, layer);

        if delay_unit <= 0.0 {
            return 0;
        }

        let length_in_mm = desired_delay as f64 / delay_unit; // MM
        (length_in_mm * PCB_IU_PER_MM) as i64 // Length IU
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::getTrackDelayPerMM` (the context's net
    /// class given as its tuning profile name).
    fn track_delay_per_mm(&self, tuning_profile: &str, layer: PcbLayerId) -> f64 {
        let Some(profile) = self.tuning_profile(tuning_profile) else {
            return 0.0;
        };

        match profile.track_propagation_entries_map.get(&layer.0) {
            Some(entry) => f64::from(entry_delay(entry)),
            None => 0.0,
        }
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::GetPropagationDelays`: the delay of
    /// every item, from the tuning profile of the first item's net class.
    pub fn get_propagation_delays(&self, items: &[LengthDelayCalculationItem]) -> Vec<i64> {
        if items.is_empty() {
            return Vec::new();
        }

        let delay_profile_name = items[0].effective_tuning_profile();

        let Some(delay_profile) = self.tuning_profile(delay_profile_name) else {
            return vec![0; items.len()];
        };

        items
            .iter()
            .map(|item| self.propagation_delay(item, delay_profile))
            .collect()
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::getPropagationDelay`.
    fn propagation_delay(
        &self,
        item: &LengthDelayCalculationItem,
        delay_profile: &TuningProfile,
    ) -> i64 {
        if item.merge_status() == MergeStatus::MergedRetired {
            return 0;
        }

        match item.type_() {
            LdcItemType::Line => {
                let mut delay_unit = 0.0;

                if let Some(entry) = delay_profile
                    .track_propagation_entries_map
                    .get(&item.start_layer().0)
                {
                    delay_unit = f64::from(entry_delay(entry));
                }

                (delay_unit * (item.line().length() as f64 / PCB_IU_PER_MM)) as i64
            }
            LdcItemType::Via => {
                if !delay_profile.enable_time_domain_tuning {
                    return 0;
                }

                let via = self.item(item.via().expect("a via"));
                let padstack = via.as_via().expect("a via").padstack();

                self.via_propagation_delay(
                    item.start_layer(),
                    item.end_layer(),
                    padstack.start_layer(),
                    padstack.end_layer(),
                    delay_profile,
                )
            }
            LdcItemType::Pad => {
                let pad = self.item(item.pad().expect("a pad"));
                i64::from(pad.as_pad().expect("a pad").pad_to_die_delay())
            }
            LdcItemType::Unknown => 0,
        }
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::GetViaPropagationDelay` (the context's
    /// net class given as its tuning profile name).
    pub fn get_via_propagation_delay(
        &self,
        signal_start_layer: PcbLayerId,
        signal_end_layer: PcbLayerId,
        via_start_layer: PcbLayerId,
        via_end_layer: PcbLayerId,
        tuning_profile: &str,
    ) -> i64 {
        let Some(profile) = self.tuning_profile(tuning_profile) else {
            return 0;
        };

        self.via_propagation_delay(
            signal_start_layer,
            signal_end_layer,
            via_start_layer,
            via_end_layer,
            profile,
        )
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::getViaPropagationDelay`.
    fn via_propagation_delay(
        &self,
        mut signal_start_layer: PcbLayerId,
        mut signal_end_layer: PcbLayerId,
        mut via_start_layer: PcbLayerId,
        mut via_end_layer: PcbLayerId,
        tuning_profile: &TuningProfile,
    ) -> i64 {
        // Ensure ordering as per the via save order
        if is_copper_layer_lower_than(signal_start_layer, signal_end_layer) {
            std::mem::swap(&mut signal_start_layer, &mut signal_end_layer);
        }

        if is_copper_layer_lower_than(via_start_layer, via_end_layer) {
            std::mem::swap(&mut via_start_layer, &mut via_end_layer);
        }

        // First check for a layer-to-layer override - this assumes that the layers are already
        // in CuStack() order
        let via_overrides = &self.via_overrides[&tuning_profile.profile_name];

        if let Some(&delay) = via_overrides.get(&(
            signal_start_layer.0,
            signal_end_layer.0,
            via_start_layer.0,
            via_end_layer.0,
        )) {
            return delay;
        }

        // Otherwise, return the tuning profile default
        let distance = f64::from(self.stackup_height(signal_start_layer, signal_end_layer));
        (f64::from(tuning_profile.via_propagation_delay) * (distance / PCB_IU_PER_MM)) as i64
    }

    /// Port of `TUNING_PROFILE_PARAMETERS_USER_DEFINED::CalculatePropagationDelayForShapeLineChain`.
    pub fn calculate_propagation_delay_for_shape_line_chain(
        &self,
        shape: &ShapeLineChain,
        tuning_profile: &str,
        layer: PcbLayerId,
    ) -> i64 {
        let delay_unit = self.track_delay_per_mm(tuning_profile, layer);
        (delay_unit * (shape.length() as f64 / PCB_IU_PER_MM)) as i64
    }
}

/// `DELAY_PROFILE_TRACK_PROPAGATION_ENTRY::GetDelay()`: the delay, 0 unless time domain tuning
/// is enabled for the entry.
fn entry_delay(
    entry: &kicad_pcb::project::tuning_profiles_json::DelayProfileTrackPropagationEntry,
) -> i32 {
    if entry.enable_time_domain_tuning {
        entry.delay
    } else {
        0
    }
}
