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

//! Port of the item indexes of `pcbnew/zone_filler.cpp` (`ZONE_FILLER::INDEXED_ITEM`,
//! `ITEM_RTREE`, `queryIndex`, `buildItemIndexes`, `forEachBoardAndFootprintZone`).
//!
//! The indexes hold item handles (`ItemId`) instead of `BOARD_ITEM*`; query results are sorted
//! by their board-order sequence number exactly like the C++, so the R-tree visit order never
//! reaches the output.

use std::collections::BTreeMap;

use kicad_core::{Lset, PcbLayerId};
use kicad_geom::Box2I;
use kicad_geom::geometry::rtree::packed_rtree::{PackedRTree, PackedRTreeBuilder};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::ids::{FootprintId, ItemId, ZoneId};
use kicad_pcb::pcb_field::FieldT;

use crate::filler::ZoneFiller;

/// Port of `ZONE_FILLER::INDEXED_ITEM`: an item in one of the fill indexes. `seq` is the item
/// position in board order; query results are sorted back into it because the boolean engine
/// depends on contour order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexedItem {
    /// `m_item`.
    pub item: ItemId,
    /// `m_owner`: the footprint that owns a graphic item, else `None`.
    pub owner: Option<FootprintId>,
    /// `m_seq`.
    pub seq: i32,
}

/// `ITEM_RTREE = KIRTREE::PACKED_RTREE<INDEXED_ITEM, int, 2>`.
pub type ItemRtree = PackedRTree<IndexedItem, 2>;

/// Port of `ZONE_FILLER::queryIndex( aIndex, aBBox, aResult )`: the items whose bounding box
/// overlaps `bbox`, in board order (`m_seq`).
pub fn query_index(index: &ItemRtree, bbox: &Box2I) -> Vec<IndexedItem> {
    let mut result = Vec::new();

    if index.is_empty() {
        return result;
    }

    let min = [bbox.left(), bbox.top()];
    let max = [bbox.right(), bbox.bottom()];

    index.search(&min, &max, |entry| {
        result.push(*entry);
        true
    });

    // std::sort by m_seq: the sequence numbers inside one index are unique (a track or zone
    // on several layers is in several per-layer indexes, once each), so the unstable sort
    // has no ties and any sort gives the same order.
    result.sort_by(|a, b| a.seq.cmp(&b.seq));
    result
}

/// Port of the anonymous `forEachBoardAndFootprintZone( aBoard, aFunc )`: the board zones in
/// board order, then the zones of every footprint in footprint order.
pub fn board_and_footprint_zones(ctx: &BoardCtx<'_>) -> Vec<ZoneId> {
    ctx.board.get_zone_list(true)
}

/// The `aBuilder.Add( min, max, INDEXED_ITEM{ ... } )` lambda of `buildItemIndexes`.
fn add(
    builder: &mut PackedRTreeBuilder<IndexedItem, 2>,
    ctx: &BoardCtx<'_>,
    item: ItemId,
    owner: Option<FootprintId>,
    seq: i32,
) {
    let Some(item_ref) = ctx.item(item) else {
        return;
    };

    let bbox = item_ref.as_dyn().bounding_box(ctx);
    let min = [bbox.left(), bbox.top()];
    let max = [bbox.right(), bbox.bottom()];

    builder.add(&min, &max, IndexedItem { item, owner, seq });
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::buildItemIndexes()`: index the static board items once per fill.
    pub(crate) fn build_item_indexes(&mut self, ctx: &BoardCtx<'_>) {
        let board = ctx.board;

        self.max_zone_corner_radius = 0;

        for zone in board_and_footprint_zones(ctx) {
            if let Some(zone) = board.zone(zone) {
                // (int) zone->GetCornerRadius()
                self.max_zone_corner_radius =
                    self.max_zone_corner_radius.max(zone.corner_radius as i32);
            }
        }

        let mut graphics = PackedRTree::builder();
        let mut footprints = PackedRTree::builder();
        let mut pads = PackedRTree::builder();
        let mut seq = 0;
        let mut pad_seq = 0;

        // Keep the walk order of the linear scans this replaces.
        for (fp_id, footprint) in board.footprints() {
            add(&mut footprints, ctx, fp_id.into(), Some(fp_id), seq);

            // &footprint->Reference() / &footprint->Value(): the mandatory fields (a footprint
            // without one would crash the C++; the port skips the entry but keeps the count).
            if let Some(reference) = footprint.field(ctx, FieldT::Reference) {
                add(&mut graphics, ctx, reference.into(), Some(fp_id), seq);
            }

            seq += 1;

            if let Some(value) = footprint.field(ctx, FieldT::Value) {
                add(&mut graphics, ctx, value.into(), Some(fp_id), seq);
            }

            seq += 1;

            for (item, _) in board.footprint_drawings(fp_id) {
                add(&mut graphics, ctx, item, Some(fp_id), seq);
                seq += 1;
            }

            for (pad, _) in board.footprint_pads(fp_id) {
                add(&mut pads, ctx, pad.into(), Some(fp_id), pad_seq);
                pad_seq += 1;
            }
        }

        for (item, _) in board.drawings() {
            add(&mut graphics, ctx, item, None, seq);
            seq += 1;
        }

        self.graphic_index = graphics.build();
        self.footprint_index = footprints.build();
        self.pad_index = pads.build();

        let board_cu = Lset::all_cu_mask_n(board.copper_layer_count());

        let mut tracks: BTreeMap<PcbLayerId, PackedRTreeBuilder<IndexedItem, 2>> = BTreeMap::new();
        seq = 0;

        for (track_id, _) in board.tracks() {
            let id: ItemId = track_id.into();

            if let Some(track) = ctx.item(id) {
                let mut track_layers = track.as_dyn().layer_set(ctx);
                track_layers &= &board_cu;

                for layer in track_layers.iter() {
                    add(tracks.entry(layer).or_default(), ctx, id, None, seq);
                }
            }

            seq += 1;
        }

        self.track_index.clear();

        for (layer, mut builder) in tracks {
            self.track_index.insert(layer, builder.build());
        }

        let mut zones: BTreeMap<PcbLayerId, PackedRTreeBuilder<IndexedItem, 2>> = BTreeMap::new();
        seq = 0;

        for zone_id in board_and_footprint_zones(ctx) {
            if let Some(zone) = board.zone(zone_id) {
                for layer in zone.layer_set.iter() {
                    add(
                        zones.entry(layer).or_default(),
                        ctx,
                        zone_id.into(),
                        None,
                        seq,
                    );
                }
            }

            seq += 1;
        }

        self.zone_index.clear();

        for (layer, mut builder) in zones {
            self.zone_index.insert(layer, builder.build());
        }
    }
}
