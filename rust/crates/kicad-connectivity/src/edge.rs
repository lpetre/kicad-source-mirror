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

//! Port of `CN_EDGE` (`pcbnew/connectivity/connectivity_algo.h`), incl. `StableSortCompare`.
//!
//! C++ holds the two end points as `shared_ptr<const CN_ANCHOR>`. They are either anchors
//! stored in a `CN_ITEM` (ratsnest nodes, shared with the item list so tags and clusters set
//! later are visible through the edge) or anchors created by `RN_NET::OptimizeRNEdges` on a
//! zone outline that no item owns. [`CnEdgeNode`] names both kinds; every accessor that needs
//! the anchor itself resolves a stored anchor through the [`CnList`].

use std::cmp::Ordering;

use kicad_geom::math::vector2::Vector2I;

use crate::items::{CnAnchor, CnAnchorRef, CnList};

/// One end of a [`CnEdge`] (C++ `shared_ptr<const CN_ANCHOR>`, non-null).
#[derive(Clone, Debug, PartialEq)]
pub enum CnEdgeNode {
    /// An anchor stored in an item of the list (`CN_ITEM::Anchors()`).
    Anchor(CnAnchorRef),
    /// An anchor created by `OptimizeRNEdges` (`std::make_shared<CN_ANCHOR>( pt, zoneLayer )`):
    /// owned by the edge only.
    Optimized(CnAnchor),
}

impl CnEdgeNode {
    /// The anchor behind this node.
    pub fn resolve<'a>(&'a self, list: &'a CnList) -> &'a CnAnchor {
        match self {
            CnEdgeNode::Anchor(r) => anchor_of(list, *r),
            CnEdgeNode::Optimized(a) => a,
        }
    }

    /// The stored anchor this node names, if any.
    pub fn anchor_ref(&self) -> Option<CnAnchorRef> {
        match self {
            CnEdgeNode::Anchor(r) => Some(*r),
            CnEdgeNode::Optimized(_) => None,
        }
    }
}

/// The stored anchor `r` (C++ dereference of the `shared_ptr<CN_ANCHOR>`).
pub fn anchor_of(list: &CnList, r: CnAnchorRef) -> &CnAnchor {
    &list.item(r.item).anchors()[r.index as usize]
}

/// Mutable access to the stored anchor `r`.
pub fn anchor_of_mut(list: &mut CnList, r: CnAnchorRef) -> &mut CnAnchor {
    &mut list.item_mut(r.item).anchors_mut()[r.index as usize]
}

/// Port of `CN_EDGE`: a point-to-point connection, whether realized or unrealized (ie: tracks
/// etc. or a ratsnest line).
#[derive(Clone, Debug, PartialEq)]
pub struct CnEdge {
    /// `m_source` (`None` = C++ null, after `RemoveInvalidRefs`).
    source: Option<CnEdgeNode>,
    /// `m_target`
    target: Option<CnEdgeNode>,
    /// `m_weight`
    weight: u32,
    /// `m_visible`
    visible: bool,
    /// Not in C++: set when an end of this edge was chosen among candidates the reference
    /// orders by heap address (FIX-CN-1, review-connectivity.md F-1); `None` for the edges
    /// whose ends do not depend on that order.
    tie: Option<Box<CnEdgeTie>>,
}

/// Not in C++ (FIX-CN-1): why and how the ends of a ratsnest edge depend on the heap order of
/// the reference build, which no deterministic rule reproduces (review-connectivity.md 2.2).
///
/// A differential comparison accepts, for an edge carrying this record, any reference end
/// among the candidates of that end: an end with an empty candidate list is exact (the edge's
/// own node), otherwise the reference end is one of the candidates (item and position). With
/// [`CnEdgeTie::unordered`] the two ends may also be swapped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CnEdgeTie {
    /// The edge joins coincident anchors of different clusters (weight 1) of one chain of
    /// `TRIANGULATOR_STATE::Triangulate`, sorted by `CN_CLUSTER` address in C++ (site P7).
    pub coincident_clusters: bool,
    /// An end was moved by `optimizeZoneAnchor` and several zone layers offer the closest
    /// outline vertex at the same distance (`ConnectedItems()` order decides, site P8).
    pub zone_anchor: bool,
    /// The ends were moved (or not) by `optimizeZoneToZoneAnchors` and candidate pairs with
    /// equal centre distances decide the result (`ConnectedItems()` order, site P9).
    pub zone_to_zone: bool,
    /// The reference may have the two ends the other way round (chain case).
    pub unordered: bool,
    /// The possible source ends (incl. the port's own); empty: the source end is exact.
    pub source_candidates: Vec<CnEdgeNode>,
    /// The possible target ends (incl. the port's own); empty: the target end is exact.
    pub target_candidates: Vec<CnEdgeNode>,
}

impl Default for CnEdge {
    /// Port of `CN_EDGE()`: no end points, weight 0, visible.
    fn default() -> Self {
        CnEdge {
            source: None,
            target: None,
            weight: 0,
            visible: true,
            tie: None,
        }
    }
}

impl CnEdge {
    /// Port of `CN_EDGE( aSource, aTarget, aWeight = 0 )`.
    pub fn new(source: CnEdgeNode, target: CnEdgeNode, weight: u32) -> Self {
        CnEdge {
            source: Some(source),
            target: Some(target),
            weight,
            visible: true,
            tie: None,
        }
    }

    /// `CN_EDGE( aSource, aTarget, aWeight )` between two stored anchors.
    pub fn between(source: CnAnchorRef, target: CnAnchorRef, weight: u32) -> Self {
        CnEdge::new(
            CnEdgeNode::Anchor(source),
            CnEdgeNode::Anchor(target),
            weight,
        )
    }

    /// Port of `operator<`: sort-by-weight for the ratsnest operation.
    pub fn weight_less(&self, other: &CnEdge) -> bool {
        self.weight < other.weight
    }

    /// Port of `StableSortCompare()`: true if this edge comes before `other`.
    ///
    /// Comparison order:
    /// 1. Compare source nodes by position (x, then y)
    /// 2. Then compare by weight
    /// 3. Then by visibility
    /// 4. If everything is equal, return false for stable ordering
    pub fn stable_sort_compare(&self, other: &CnEdge, list: &CnList) -> bool {
        let this_pos = self.source_pos(list);
        let other_pos = other.source_pos(list);

        // First compare by source node position
        if this_pos.x != other_pos.x {
            return this_pos.x < other_pos.x;
        }

        if this_pos.y != other_pos.y {
            return this_pos.y < other_pos.y;
        }

        // Then compare by weight
        if self.weight != other.weight {
            return self.weight < other.weight;
        }

        // Then by visibility
        if self.visible != other.visible {
            return self.visible && !other.visible;
        }

        // If everything is equal, return false for stable ordering
        false
    }

    /// `StableSortCompare` as an [`Ordering`] for `slice::sort_by` (stable, like
    /// `std::stable_sort`).
    pub fn stable_sort_ordering(&self, other: &CnEdge, list: &CnList) -> Ordering {
        if self.stable_sort_compare(other, list) {
            Ordering::Less
        } else if other.stable_sort_compare(self, list) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }

    /// `GetSourceNode()`.
    pub fn source_node(&self) -> Option<&CnEdgeNode> {
        self.source.as_ref()
    }

    /// `GetTargetNode()`.
    pub fn target_node(&self) -> Option<&CnEdgeNode> {
        self.target.as_ref()
    }

    /// `*GetSourceNode()`, resolved through the list.
    pub fn source_anchor<'a>(&'a self, list: &'a CnList) -> Option<&'a CnAnchor> {
        self.source.as_ref().map(|n| n.resolve(list))
    }

    /// `*GetTargetNode()`, resolved through the list.
    pub fn target_anchor<'a>(&'a self, list: &'a CnList) -> Option<&'a CnAnchor> {
        self.target.as_ref().map(|n| n.resolve(list))
    }

    /// `SetSourceNode()`.
    pub fn set_source_node(&mut self, node: CnEdgeNode) {
        self.source = Some(node);
    }

    /// `SetTargetNode()`.
    pub fn set_target_node(&mut self, node: CnEdgeNode) {
        self.target = Some(node);
    }

    /// Port of `RemoveInvalidRefs()`: drops the end points whose anchor is no longer valid.
    pub fn remove_invalid_refs(&mut self, list: &CnList) {
        if self
            .source
            .as_ref()
            .is_some_and(|n| !n.resolve(list).valid(list))
        {
            self.source = None;
        }

        if self
            .target
            .as_ref()
            .is_some_and(|n| !n.resolve(list).valid(list))
        {
            self.target = None;
        }
    }

    /// `SetWeight()`.
    pub fn set_weight(&mut self, weight: u32) {
        self.weight = weight;
    }

    /// `GetWeight()`.
    pub fn weight(&self) -> u32 {
        self.weight
    }

    /// `SetVisible()`.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// `IsVisible()`.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Not in C++ (FIX-CN-1): the heap-order dependence of this edge's ends, if any.
    pub fn tie(&self) -> Option<&CnEdgeTie> {
        self.tie.as_deref()
    }

    /// Not in C++ (FIX-CN-1): whether an end of this edge was chosen among tied candidates.
    pub fn is_tied(&self) -> bool {
        self.tie.is_some()
    }

    /// Not in C++ (FIX-CN-1): the tie record of this edge, created empty if missing.
    pub fn tie_mut(&mut self) -> &mut CnEdgeTie {
        self.tie.get_or_insert_with(Default::default)
    }

    /// `GetSourcePos()` (C++ dereferences the source unchecked).
    pub fn source_pos(&self, list: &CnList) -> Vector2I {
        self.source_anchor(list)
            .expect("CN_EDGE::GetSourcePos: no source")
            .pos()
    }

    /// `GetTargetPos()` (C++ dereferences the target unchecked).
    pub fn target_pos(&self, list: &CnList) -> Vector2I {
        self.target_anchor(list)
            .expect("CN_EDGE::GetTargetPos: no target")
            .pos()
    }

    /// Port of `GetLength()`: `( target - source ).EuclideanNorm()` as `unsigned`.
    pub fn length(&self, list: &CnList) -> u32 {
        // int -> unsigned conversion of a norm that is never negative.  The int subtraction
        // wraps in the reference build for anchors more than 2^31 nm apart (review-connectivity
        // F-8, FIX-FAB-3), so it wraps here too.
        let t = self.target_pos(list);
        let s = self.source_pos(list);
        Vector2I::new(t.x.wrapping_sub(s.x), t.y.wrapping_sub(s.y)).euclidean_norm() as u32
    }
}
