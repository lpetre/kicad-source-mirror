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

//! Port of `pcbnew/drc/drc_zone_index.h`: `CollectOverlappingPairs` and `DRC_ZONE_INDEX`.
//!
//! Both use `KIRTREE::PACKED_RTREE<size_t, int, 2>`; the payloads are positions in the input
//! vectors, so the results follow the order of the input (the `std::sort` of `size_t`s has no
//! ties to worry about).

use std::collections::BTreeMap;

use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::geometry::rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_geom::math::box2::Box2I;
use kicad_pcb::ids::ZoneId;

type PairTree = PackedRTree<usize, 2, 16>;

/// Port of `CollectOverlappingPairs`: the pairs of `ordered_indices` whose boxes overlap, in the
/// order a nested loop over the vector would have produced them.
///
/// `box_fn` supplies each entry's box and returns `None` for an entry which has none; such an
/// entry appears in no pair. Boxes are normalized before comparison and overlap is inclusive,
/// so the result matches `BOX2I::Intersects()` including the degenerate origin box a zone with
/// no cached bounds reports.
pub fn collect_overlapping_pairs(
    ordered_indices: &[usize],
    mut box_fn: impl FnMut(usize) -> Option<Box2I>,
) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut boxes: Vec<Box2I> = vec![Box2I::default(); ordered_indices.len()];
    let mut present: Vec<bool> = vec![false; ordered_indices.len()];
    let mut builder: PackedRTreeBuilder<usize, 2, 16> = PackedRTreeBuilder::new();

    builder.reserve(ordered_indices.len());

    for pos in 0..ordered_indices.len() {
        let Some(mut b) = box_fn(ordered_indices[pos]) else {
            continue;
        };

        b.normalize();
        boxes[pos] = b;
        present[pos] = true;

        let min = [b.x(), b.y()];
        let max = [b.right(), b.bottom()];
        builder.add(&min, &max, pos);
    }

    let tree: PairTree = builder.build();
    let mut overlaps: Vec<usize> = Vec::new();

    for pos in 0..ordered_indices.len() {
        if !present[pos] {
            continue;
        }

        overlaps.clear();

        let min = [boxes[pos].x(), boxes[pos].y()];
        let max = [boxes[pos].right(), boxes[pos].bottom()];

        // Positions, not caller indices, so the emission order matches the vector
        tree.search(&min, &max, |&other: &usize| {
            if other > pos {
                overlaps.push(other);
            }

            true
        });

        overlaps.sort_unstable();

        for &other in &overlaps {
            pairs.push((ordered_indices[pos], ordered_indices[other]));
        }
    }

    pairs
}

/// Port of `DRC_ZONE_INDEX`: per copper layer, a packed r-tree of the zones of that layer
/// (`m_DRCCopperZonesByLayer`), queried in zone-list order.
#[derive(Clone, Debug, Default)]
pub struct DrcZoneIndex {
    /// `m_trees`
    trees: BTreeMap<PcbLayerId, PairTree>,
    /// `m_zones`
    zones: BTreeMap<PcbLayerId, Vec<ZoneId>>,
}

impl DrcZoneIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `Build`: `layers` maps each layer to its zones in order; `bounding_box` is
    /// `ZONE::GetBoundingBox()` (the cached bounds the cache generator computed).
    pub fn build(
        &mut self,
        layers: &BTreeMap<PcbLayerId, Vec<ZoneId>>,
        mut bounding_box: impl FnMut(ZoneId) -> Box2I,
    ) {
        self.clear();

        for (layer, zones) in layers {
            let mut builder: PackedRTreeBuilder<usize, 2, 16> = PackedRTreeBuilder::new();
            builder.reserve(zones.len());
            self.zones.insert(*layer, zones.clone());

            for (ordinal, &zone) in zones.iter().enumerate() {
                let mut bbox = bounding_box(zone);
                bbox.normalize();
                let min = [bbox.x(), bbox.y()];
                let max = [bbox.right(), bbox.bottom()];
                builder.add(&min, &max, ordinal);
            }

            // m_trees.emplace(): the first tree of a layer stays (keys are unique here anyway)
            self.trees.entry(*layer).or_insert_with(|| builder.build());
        }
    }

    /// Port of `HasLayer`: true when the layer holds at least one zone (list), so a caller can
    /// skip building a query box.
    pub fn has_layer(&self, layer: PcbLayerId) -> bool {
        self.trees.contains_key(&layer)
    }

    /// Port of `Query`: the zones of `layer` whose boxes overlap `a_box`, in zone-list order.
    pub fn query(&self, layer: PcbLayerId, a_box: &Box2I) -> Vec<ZoneId> {
        let mut out = Vec::new();

        let (Some(tree), Some(zones)) = (self.trees.get(&layer), self.zones.get(&layer)) else {
            return out;
        };

        let mut bbox = *a_box;
        bbox.normalize();
        let min = [bbox.x(), bbox.y()];
        let max = [bbox.right(), bbox.bottom()];
        let mut ordinals: Vec<usize> = Vec::new();

        tree.search(&min, &max, |&ordinal: &usize| {
            ordinals.push(ordinal);
            true
        });

        ordinals.sort_unstable();
        out.reserve(ordinals.len());

        for ordinal in ordinals {
            out.push(zones[ordinal]);
        }

        out
    }

    /// Port of `Clear`.
    pub fn clear(&mut self) {
        self.trees.clear();
        self.zones.clear();
    }
}
