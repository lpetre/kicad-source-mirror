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

//! Port of `pcbnew/drill/drill_chart_model.h` / `.cpp`: `DRILL_CHART_GROUP`,
//! `DRILL_CHART_TOTALS`, `DRILL_CHART_ROW_SPEC` and `DRILL_CHART_MODEL`, which turns the
//! board's drill operations into chart rows for one symbol profile.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::Vector2I;

use crate::ctx::BoardCtx;
use crate::pcb_generated_table::DrillChartFilter;

use super::drill_enumerator::enumerate_drill_operations;
use super::drill_operation::{
    DrillOpKind, DrillOperation, DrillOperationId, DrillPostMachining, DrillQuery, DrillSpan,
    HoleAttribute,
};
use super::drill_symbol_profile::{DrillSymbolAssignment, DrillSymbolProfile};

/// `DRILL_CHART_GROUP`: one chart row.
///
/// Operations and sites are counted separately because they differ on any board with
/// backdrills, where several machining actions share one XY location.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillChartGroup {
    /// m_Key: identity of this row. May be finer than the symbol key when the chart splits
    /// further.
    pub key: String,
    /// m_SymbolKey: identity of the symbol, which always belongs to the profile's grouping.
    /// Two rows can legitimately share one mark when a chart splits a profile group.
    pub symbol_key: String,

    pub diameter: i32,     // m_Diameter
    pub size_xy: Vector2I, // m_SizeXY
    pub is_slot: bool,     // m_IsSlot
    pub not_plated: bool,  // m_NotPlated

    pub top_layer: PcbLayerId,    // m_TopLayer
    pub bottom_layer: PcbLayerId, // m_BottomLayer
    pub kind: DrillOpKind,        // m_Kind
    pub attribute: HoleAttribute, // m_Attribute

    pub stub_length: Option<i32>, // m_StubLength

    pub filled: bool,         // m_Filled
    pub capped: bool,         // m_Capped
    pub top_covered: bool,    // m_TopCovered
    pub bottom_covered: bool, // m_BottomCovered
    pub top_plugged: bool,    // m_TopPlugged
    pub bottom_plugged: bool, // m_BottomPlugged
    pub top_tented: bool,     // m_TopTented
    pub bottom_tented: bool,  // m_BottomTented

    pub front_post_machining: DrillPostMachining, // m_FrontPostMachining
    pub back_post_machining: DrillPostMachining,  // m_BackPostMachining

    pub operation_count: i32, // m_OperationCount
    pub site_count: i32,      // m_SiteCount
    pub slot_count: i32,      // m_SlotCount

    pub symbol: DrillSymbolAssignment, // m_Symbol

    pub members: Vec<DrillOperationId>, // m_Members

    /// m_Sites: distinct hole locations, kept so totals can union them rather than sum site
    /// counts.
    pub sites: Vec<Vector2I>,
}

impl Default for DrillChartGroup {
    /// The in-class initialisers.
    fn default() -> Self {
        DrillChartGroup {
            key: String::new(),
            symbol_key: String::new(),
            diameter: 0,
            size_xy: Vector2I::new(0, 0),
            is_slot: false,
            not_plated: false,
            top_layer: PcbLayerId::FCu,
            bottom_layer: PcbLayerId::BCu,
            kind: DrillOpKind::PrimaryDrill,
            attribute: HoleAttribute::HoleUnknown,
            stub_length: None,
            filled: false,
            capped: false,
            top_covered: false,
            bottom_covered: false,
            top_plugged: false,
            bottom_plugged: false,
            top_tented: false,
            bottom_tented: false,
            front_post_machining: DrillPostMachining::default(),
            back_post_machining: DrillPostMachining::default(),
            operation_count: 0,
            site_count: 0,
            slot_count: 0,
            symbol: DrillSymbolAssignment::default(),
            members: Vec::new(),
            sites: Vec::new(),
        }
    }
}

/// `DRILL_CHART_TOTALS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DrillChartTotals {
    pub operations: i32, // m_Operations
    pub sites: i32,      // m_Sites
    pub groups: i32,     // m_Groups
}

/// `DRILL_CHART_ROW_SPEC`: what a particular chart wants out of the model.
///
/// Filtering has to happen per operation rather than per finished group. With a grouping key
/// disabled, one group can hold both plated and non-plated operations, and rejecting the group
/// afterwards either drops holes that were asked for or keeps holes that were not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DrillChartRowSpec {
    pub filter: DrillChartFilter, // m_Filter
}

/// `DRILL_CHART_MODEL`: turns the board's drill operations into chart rows for one symbol
/// profile. Rows come back sorted by plating, diameter, then by the group key, so a chart is
/// stable across rebuilds even when the board changes underneath it.
#[derive(Clone, Debug)]
pub struct DrillChartModel<'p> {
    profile: &'p DrillSymbolProfile, // m_profile
    groups: Vec<DrillChartGroup>,    // m_groups
    totals: DrillChartTotals,        // m_totals
}

/// Port of the anonymous-namespace `matchesFilter`: whether one operation belongs in a chart
/// carrying this filter.
fn matches_filter(op: &DrillOperation, filter: &DrillChartFilter) -> bool {
    if op.not_plated && !filter.non_plated {
        return false;
    }

    if !op.not_plated && !filter.plated {
        return false;
    }

    if op.is_slot && !filter.slots {
        return false;
    }

    if op.kind != DrillOpKind::PrimaryDrill && !filter.backdrills {
        return false;
    }

    if op.attribute == HoleAttribute::HolePadCastellated && !filter.castellated {
        return false;
    }

    let is_via = op.attribute == HoleAttribute::HoleViaThrough
        || op.attribute == HoleAttribute::HoleViaBuried;

    if is_via && !filter.vias {
        return false;
    }

    true
}

impl<'p> DrillChartModel<'p> {
    /// `DRILL_CHART_MODEL( aProfile )`.
    pub fn new(profile: &'p DrillSymbolProfile) -> Self {
        DrillChartModel {
            profile,
            groups: Vec::new(),
            totals: DrillChartTotals::default(),
        }
    }

    /// `Groups()`.
    pub fn groups(&self) -> &[DrillChartGroup] {
        &self.groups
    }

    /// `Totals()`.
    pub fn totals(&self) -> DrillChartTotals {
        self.totals
    }

    /// Port of `Build( aBoard, aSpans )`: [`build_with_spec`](Self::build_with_spec) with a
    /// default `DRILL_CHART_ROW_SPEC` (every hole).
    pub fn build(&mut self, ctx: &BoardCtx<'_>, spans: &[DrillSpan]) {
        self.build_with_spec(ctx, spans, &DrillChartRowSpec::default());
    }

    /// Port of `Build( aBoard, aSpans, aSpec )`: per span the plated, then the non-plated
    /// operations, grouped by the profile's `GroupKeyString` (`std::map` order), then sorted.
    pub fn build_with_spec(
        &mut self,
        ctx: &BoardCtx<'_>,
        spans: &[DrillSpan],
        spec: &DrillChartRowSpec,
    ) {
        self.groups.clear();
        self.totals = DrillChartTotals::default();

        let mut by_key: BTreeMap<String, DrillChartGroup> = BTreeMap::new();
        let mut sites_by_key: BTreeMap<String, BTreeSet<(i32, i32)>> = BTreeMap::new();
        let profile = self.profile;

        let mut collect = |query: &DrillQuery| {
            for op in enumerate_drill_operations(ctx, query) {
                if !matches_filter(&op, &spec.filter) {
                    continue;
                }

                let key = profile.group_key_string(&op);
                let inserted = !by_key.contains_key(&key);
                let group = by_key.entry(key.clone()).or_default();

                if inserted {
                    group.key = key.clone();
                    group.symbol_key = key.clone();
                    group.diameter = op.diameter;
                    group.size_xy = op.size_xy;
                    group.is_slot = op.is_slot;
                    group.not_plated = op.not_plated;
                    group.top_layer = op.top_layer;
                    group.bottom_layer = op.bottom_layer;
                    group.kind = op.kind;
                    group.attribute = op.attribute;
                    group.stub_length = op.stub_length;
                    group.filled = op.filled;
                    group.capped = op.capped;
                    group.top_covered = op.top_covered;
                    group.bottom_covered = op.bottom_covered;
                    group.top_plugged = op.top_plugged;
                    group.bottom_plugged = op.bottom_plugged;
                    group.top_tented = op.top_tented;
                    group.bottom_tented = op.bottom_tented;
                    group.front_post_machining = op.front_post_machining;
                    group.back_post_machining = op.back_post_machining;
                }

                group.operation_count += 1;

                if op.is_slot {
                    group.slot_count += 1;
                }

                group.members.push(op.id());
                sites_by_key
                    .entry(key)
                    .or_default()
                    .insert((op.position.x, op.position.y));
            }
        };

        for span in spans {
            let plated = DrillQuery {
                span: *span,
                ..DrillQuery::default()
            };
            collect(&plated);

            let non_plated = DrillQuery {
                span: *span,
                non_plated_only: true,
                ..DrillQuery::default()
            };
            collect(&non_plated);
        }

        let mut all_sites: BTreeSet<(i32, i32)> = BTreeSet::new();

        for (key, group) in by_key.iter_mut() {
            let sites = sites_by_key.entry(key.clone()).or_default();
            group.site_count = sites.len() as i32;

            for &(x, y) in sites.iter() {
                group.sites.push(Vector2I::new(x, y));
            }

            all_sites.extend(sites.iter().copied());
            self.totals.operations += group.operation_count;
            self.groups.push(group.clone());
        }

        self.totals.sites = all_sites.len() as i32;
        self.totals.groups = self.groups.len() as i32;

        // The keys are unique (one group per map key), so the comparator is a strict total
        // order and the stable sort gives the std::sort result.
        self.groups.sort_by(|a, b| {
            if a.not_plated != b.not_plated {
                return if !a.not_plated {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            if a.diameter != b.diameter {
                return a.diameter.cmp(&b.diameter);
            }

            a.key.cmp(&b.key)
        });
    }
}
