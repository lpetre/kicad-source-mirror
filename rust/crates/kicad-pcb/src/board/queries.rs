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

//! Port of the query part of `pcbnew/board.cpp` (`GetPad*`, `FindFootprintBy*`, bounding
//! boxes, max clearance, zone lists, groups sanity check). `board_bounding_box.cpp`
//! (`BOARD_BOUNDING_BOX`, a view-only `EDA_ITEM` wrapper of a box) has nothing to port.
//! Package M22; M0d wrote the signatures (`board-model-types.md` 18).

use std::collections::BTreeSet;

use kicad_core::{KicadT, Kiid, KiidPath, Lset, PcbLayerId};
use kicad_geom::stdsort::std_sort;
use kicad_geom::{Box2I, Vector2I};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::ids::{FootprintId, ItemId, NetCode, PadId, TrackId, ZoneId};
use crate::item::BoardUse;
use crate::traits::BoardConnectedItem;

impl Board {
    /// `BOARD::FindFootprintByReference`: the first footprint (board order) whose reference
    /// is exactly `reference`.
    pub fn find_footprint_by_reference(&self, reference: &str) -> Option<FootprintId> {
        let ctx = BoardCtx::bare(self);

        self.footprints()
            .find(|(_, footprint)| reference == footprint.reference(&ctx))
            .map(|(id, _)| id)
    }

    /// `BOARD::FindFootprintByPath`: the first footprint whose `m_path` equals `path`.
    pub fn find_footprint_by_path(&self, path: &KiidPath) -> Option<FootprintId> {
        self.footprints()
            .find(|(_, footprint)| footprint.path == *path)
            .map(|(id, _)| id)
    }

    /// `BOARD::FindPadByUuid`: the first pad with this UUID, footprints in board order, each
    /// footprint's pads in its order (`FOOTPRINT::FindPadByUuid`).
    pub fn find_pad_by_uuid(&self, uuid: &Kiid) -> Option<PadId> {
        let ctx = BoardCtx::bare(self);

        for (_, footprint) in self.footprints() {
            if let Some(pad) = footprint.find_pad_by_uuid(&ctx, uuid) {
                return Some(pad);
            }
        }

        None
    }

    /// `BOARD::GetPad( aPosition, aLayerSet )`: the first pad hit at `pos` on one of
    /// `layers` (all copper layers when `layers` is empty), among the footprints whose
    /// bounding box contains `pos` (`FOOTPRINT::GetPad( aPosition, aLayerMask )`).
    pub fn get_pad_at(&self, ctx: &BoardCtx<'_>, pos: Vector2I, layers: Lset) -> Option<PadId> {
        let mask = if layers.any() {
            layers
        } else {
            Lset::all_cu_mask().clone()
        };

        for (_, footprint) in self.footprints() {
            if !footprint.hit_test(ctx, pos, 0) {
                continue;
            }

            if let Some(pad) = footprint.get_pad_at(ctx, pos, &mask) {
                return Some(pad);
            }
        }

        None
    }

    /// `BOARD::GetPad( aPadList, aPosition, aLayerSet )`: binary search of a pad list sorted
    /// by `get_sorted_pad_list_by_x_then_y_coord` for a pad at exactly `pos` sharing a layer
    /// with `layers`.
    pub fn get_pad_in_sorted_list(
        &self,
        ctx: &BoardCtx<'_>,
        pad_list: &[PadId],
        pos: Vector2I,
        layers: &Lset,
    ) -> Option<PadId> {
        let position = |id: PadId| self.pads.get(id).map(|pad| pad.position(ctx));
        let layer_set = |id: PadId| self.pads.get(id).map(|pad| pad.layer_set(ctx));
        let matches = |id: PadId| layer_set(id).is_some_and(|set| (layers & &set).any());

        // Search aPadList for aPosition
        // aPadList is sorted by X then Y values, and a fast binary search is used
        let idxmax = pad_list.len() as i32 - 1;
        let mut delta = pad_list.len() as i32;
        let mut idx: i32 = 0; // Starting index is the beginning of list

        while delta != 0 {
            // Calculate half size of remaining interval to test.
            // Ensure the computed value is not truncated (too small)
            if (delta & 1) != 0 && delta > 1 {
                delta += 1;
            }

            delta /= 2;

            let pad = pad_list[idx as usize];
            let pad_pos = position(pad)?;

            if pad_pos == pos {
                // candidate found
                // The pad must match the layer mask:
                if matches(pad) {
                    return Some(pad);
                }

                // More than one pad can be at aPosition
                // search for a pad at aPosition that matched this mask

                // search next
                for ii in idx + 1..=idxmax {
                    let pad = pad_list[ii as usize];

                    if position(pad)? != pos {
                        break;
                    }

                    if matches(pad) {
                        return Some(pad);
                    }
                }

                // search previous
                for ii in (0..idx).rev() {
                    let pad = pad_list[ii as usize];

                    if position(pad)? != pos {
                        break;
                    }

                    if matches(pad) {
                        return Some(pad);
                    }
                }

                // Not found:
                return None;
            }

            if pad_pos.x == pos.x {
                // Must search considering Y coordinate
                if pad_pos.y < pos.y {
                    // Must search after this item
                    idx += delta;

                    if idx > idxmax {
                        idx = idxmax;
                    }
                } else {
                    // Must search before this item
                    idx -= delta;

                    if idx < 0 {
                        idx = 0;
                    }
                }
            } else if pad_pos.x < pos.x {
                // Must search after this item
                idx += delta;

                if idx > idxmax {
                    idx = idxmax;
                }
            } else {
                // Must search before this item
                idx -= delta;

                if idx < 0 {
                    idx = 0;
                }
            }
        }

        None
    }

    /// `BOARD::GetSortedPadListByXthenYCoord( aVector, aNetCode )`; `None` = every net. The
    /// list is sorted with `std::sort` and `sortPadsByXthenYCoord` (the libstdc++ port, so
    /// that pads at the same position keep the reference order).
    pub fn get_sorted_pad_list_by_x_then_y_coord(
        &self,
        ctx: &BoardCtx<'_>,
        net: Option<NetCode>,
    ) -> Vec<PadId> {
        let mut pads: Vec<(PadId, Vector2I)> = Vec::new();

        for (fp, _) in self.footprints() {
            for (pad_id, pad) in self.footprint_pads(fp) {
                if net.is_none_or(|code| pad.net_code() == code) {
                    pads.push((pad_id, pad.position(ctx)));
                }
            }
        }

        // sortPadsByXthenYCoord
        std_sort(&mut pads, |(_, lh), (_, rh)| {
            if lh.x == rh.x {
                return lh.y < rh.y;
            }

            lh.x < rh.x
        });

        pads.into_iter().map(|(id, _)| id).collect()
    }

    /// `BOARD::GetZoneList( aIncludeZonesInFootprints )`: the board zones, then (optionally)
    /// the zones of every footprint in board order.
    pub fn get_zone_list(&self, include_zones_in_footprints: bool) -> Vec<ZoneId> {
        let mut zones: Vec<ZoneId> = self.zones().map(|(id, _)| id).collect();

        if include_zones_in_footprints {
            for (fp, _) in self.footprints() {
                zones.extend(self.footprint_zones(fp).map(|(id, _)| id));
            }
        }

        zones
    }

    /// `BOARD::GetMaxClearanceValue`: the biggest clearance of the design settings (with the
    /// rules of `ctx`), of every zone's local clearance and of every pad's clearance override.
    /// Cached until the next time stamp bump, like `m_maxClearanceValue`: the first caller's
    /// rule resolver decides the value, exactly as the engine attached at the first C++ call
    /// does (the DRC engine set-up bumps the time stamp).
    pub fn get_max_clearance_value(&self, ctx: &BoardCtx<'_>) -> i32 {
        *self.caches.max_clearance_value.get_or_build(
            |_| true,
            || {
                let mut worst_clearance =
                    self.design_settings.get_biggest_clearance_value(ctx.rules);

                // ZONE::GetLocalClearance() (no source) always has a value: 0 for a rule area.
                for (_, zone) in self.zones() {
                    worst_clearance = worst_clearance.max(zone.local_clearance().unwrap_or(0));
                }

                for (fp, _) in self.footprints() {
                    for (_, pad) in self.footprint_pads(fp) {
                        if let Some((value, _)) = pad.clearance_overrides(ctx, false) {
                            worst_clearance = worst_clearance.max(value);
                        }
                    }

                    for (_, zone) in self.footprint_zones(fp) {
                        worst_clearance = worst_clearance.max(zone.local_clearance().unwrap_or(0));
                    }
                }

                worst_clearance
            },
        )
    }

    /// `BOARD::GetVisibleLayers()`: the project's local-settings visible layers
    /// (`m_project->GetLocalSettings().m_VisibleLayers`), else every layer.
    pub fn visible_layers(&self) -> Lset {
        match self.project.as_ref() {
            Some(p) => p.visible_layers.clone(),
            None => Lset::all_layers_mask().clone(),
        }
    }

    /// Port of `BOARD::IsLayerVisible`: an enabled layer that is visible in the project's
    /// local settings.
    pub fn is_layer_visible(&self, layer: PcbLayerId) -> bool {
        // If there is no project, assume layer is visible always
        self.design_settings().is_layer_enabled(layer)
            && self
                .project
                .as_ref()
                .is_none_or(|p| p.visible_layers.contains(layer))
    }

    /// `BOARD::IsFootprintHolder()`.
    pub fn is_footprint_holder(&self) -> bool {
        self.board_use == BoardUse::FpHolder
    }

    /// `BOARD::ComputeBoundingBox( aBoardEdgesOnly, aPhysicalLayersOnly )`.
    pub fn compute_bounding_box(
        &self,
        ctx: &BoardCtx<'_>,
        board_edges_only: bool,
        physical_layers_only: bool,
    ) -> Box2I {
        let mut bbox = Box2I::default();
        let mut visible = self.visible_layers();

        if physical_layers_only {
            visible &= Lset::physical_layers_mask();
        }

        // If the board is just showing a footprint, we want all footprint layers included in the
        // bounding box
        if self.is_footprint_holder() {
            visible.set_all();
        }

        if board_edges_only {
            visible.set(PcbLayerId::EdgeCuts);
        }

        // Check shapes, dimensions, texts, and fiducials
        for (id, _) in self.drawings() {
            let Some(item) = ctx.item(id) else {
                continue;
            };

            if board_edges_only
                && (item.type_() != KicadT::PcbShapeT || item.layer() != PcbLayerId::EdgeCuts)
            {
                continue;
            }

            if (&item.layer_set() & &visible).any() {
                bbox.merge(&item.bounding_box());
            }
        }

        // Check footprints
        for (fp, footprint) in self.footprints() {
            if board_edges_only {
                for (edge_id, _) in self.footprint_drawings(fp) {
                    let Some(edge) = ctx.item(edge_id) else {
                        continue;
                    };

                    if edge.layer() == PcbLayerId::EdgeCuts && edge.type_() == KicadT::PcbShapeT {
                        bbox.merge(&edge.bounding_box());
                    }
                }
            } else if ctx
                .item(fp.into())
                .is_some_and(|item| (&item.layer_set() & &visible).any())
            {
                bbox.merge(&footprint.bounding_box_with_text(ctx, true));
            }
        }

        if !board_edges_only {
            // Check tracks
            for (id, _) in self.tracks() {
                if let Some(track) = ctx.item(id.into()) {
                    if (&track.layer_set() & &visible).any() {
                        bbox.merge(&track.bounding_box());
                    }
                }
            }

            // Check zones
            for (id, _) in self.zones() {
                if let Some(zone) = ctx.item(id.into()) {
                    if (&zone.layer_set() & &visible).any() {
                        bbox.merge(&zone.bounding_box());
                    }
                }
            }

            if !physical_layers_only {
                for (id, _) in self.board_points() {
                    if let Some(point) = ctx.item(id.into()) {
                        bbox.merge(&point.bounding_box());
                    }
                }
            }
        }

        bbox
    }

    /// `BOARD::GetBoardEdgesBoundingBox` = `ComputeBoundingBox( true, true )`.
    pub fn board_edges_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        self.compute_bounding_box(ctx, true, true)
    }

    /// `BOARD::GetTrackLength( aTrack )`: (count, track length, pad-to-die length, track delay,
    /// pad-to-die delay).
    ///
    /// Stub: needs `CONNECTIVITY_DATA::GetConnectedItems` and `LENGTH_DELAY_CALCULATION`
    /// (connectivity crate and the length calculation, neither ported nor reachable from
    /// `BoardCtx::connectivity` yet).
    pub fn get_track_length(
        &self,
        ctx: &BoardCtx<'_>,
        track: TrackId,
    ) -> (i32, f64, f64, f64, f64) {
        crate::port_pending!("BOARD::GetTrackLength", ctx.board.time_stamp(), track)
    }

    /// `BOARD::GroupsSanityCheck( repair )`: the error text (empty when sane). With `repair`
    /// every group closing a cycle is removed until none is left.
    pub fn groups_sanity_check(&mut self, repair: bool) -> String {
        if repair {
            while !self.groups_sanity_check_internal(repair).is_empty() {}

            return String::new();
        }

        self.groups_sanity_check_internal(repair)
    }

    /// `BOARD::GroupsSanityCheckInternal( repair )`.
    ///
    /// The C++ sets are `std::unordered_set<EDA_GROUP*>` and the chain walk starts at
    /// `toCheckGroups.begin()` (heap-address hash order); here they are ordered by `ItemId`
    /// (`board-model.md` 3.1). The result text does not depend on it; with `repair` the group
    /// removed from a cycle can differ.
    pub fn groups_sanity_check_internal(&mut self, repair: bool) -> String {
        // Cycle detection
        //
        // Each group has at most one parent group.
        // So we start at group 0 and traverse the parent chain, marking groups seen along the way.
        // If we ever see a group that we've already marked, that's a cycle.
        // If we reach the end of the chain, we know all groups in that chain are not part of any cycle.
        //
        // Algorithm below is linear in the # of groups because each group is visited only once.
        // There may be extra time taken due to the container access calls and iterators.
        //
        // Groups we know are cycle free
        let mut known_cycle_free_groups: BTreeSet<ItemId> = BTreeSet::new();
        // Groups in the current chain we're exploring.
        let mut current_chain_groups: BTreeSet<ItemId> = BTreeSet::new();
        // Groups we haven't checked yet.
        let mut to_check_groups: BTreeSet<ItemId> = BTreeSet::new();

        // Initialize set of groups and generators to check that could participate in a cycle.
        for (group, _) in self.groups() {
            to_check_groups.insert(group.into());
        }

        for (generator, _) in self.generators() {
            to_check_groups.insert(generator.into());
        }

        let mut cycle_group: Option<ItemId> = None;

        {
            let ctx = BoardCtx::bare(self);

            'outer: while let Some(&first) = to_check_groups.first() {
                current_chain_groups.clear();
                let mut group = first;

                loop {
                    if current_chain_groups.contains(&group) {
                        cycle_group = Some(group);
                        break 'outer;
                    } else if known_cycle_free_groups.contains(&group) {
                        // Parent is a group we know does not lead to a cycle
                        break;
                    }

                    current_chain_groups.insert(group);
                    // We haven't visited currIdx yet, so it must be in toCheckGroups
                    to_check_groups.remove(&group);

                    match ctx.item(group).and_then(|item| item.parent_group()) {
                        Some(parent) => match parent.id() {
                            Some(id) => group = id,
                            None => break,
                        },
                        None => {
                            // end of chain and no cycles found in this chain
                            break;
                        }
                    }
                }

                // No cycles found in chain, so add it to set of groups we know don't participate
                // in a cycle.
                known_cycle_free_groups.extend(current_chain_groups.iter().copied());
            }
        }

        if let Some(group) = cycle_group {
            if repair {
                self.remove(group);
            }

            return "Cycle detected in group membership".to_string();
        }

        // Success
        String::new()
    }
}

/// Package M22 tests. `InvalidGroups` is ported from `qa/tests/pcbnew/group_saveload.cpp`
/// (the cycles are built directly, as `createBoard( .., aAllowInvalidGroups = true )` does).
#[cfg(test)]
mod m22_tests {
    use kicad_core::{KiidPath, Lset, PcbLayerId};
    use kicad_geom::Vector2I;

    use super::*;
    use crate::board::AddMode;
    use crate::ids::{GroupId, ItemKind};
    use crate::pcb_group::PcbGroup;
    use crate::pcb_track::{PcbTrack, Track};

    fn add_groups(board: &mut Board, n: usize) -> Vec<GroupId> {
        (0..n)
            .map(|_| {
                let g = PcbGroup::new(&mut board.kiid_generator);
                board.add_group(g, AddMode::Append)
            })
            .collect()
    }

    /// Make `child` a member of `parent` on both sides, bypassing the cycle check of
    /// `add_to_group` (`PCB_GROUP::AddItem` without the board check).
    fn force_member(board: &mut Board, parent: GroupId, child: GroupId) {
        board
            .groups
            .get_mut(parent)
            .unwrap()
            .members
            .push(child.into());
        board.groups.get_mut(child).unwrap().base.group = Some(parent.into());
    }

    /// `InvalidGroups`.
    #[test]
    fn invalid_groups() {
        // A cycle
        let mut board = Board::default();
        let g = add_groups(&mut board, 2);
        force_member(&mut board, g[0], g[1]);
        force_member(&mut board, g[1], g[0]);
        assert_eq!(
            board.groups_sanity_check(false),
            "Cycle detected in group membership"
        );

        // More complex cycle: 0 -> 1, 2 -> 4 -> 3 -> 2
        let mut board = Board::default();
        let g = add_groups(&mut board, 5);
        force_member(&mut board, g[0], g[1]);
        force_member(&mut board, g[2], g[4]);
        force_member(&mut board, g[3], g[2]);
        force_member(&mut board, g[4], g[3]);
        assert_eq!(
            board.groups_sanity_check(false),
            "Cycle detected in group membership"
        );

        // Repair removes groups until no cycle is left.
        assert_eq!(board.groups_sanity_check(true), "");
        assert_eq!(board.groups_sanity_check(false), "");
        assert!(board.groups().count() < 5);
    }

    #[test]
    fn healthy_groups_are_sane() {
        let mut board = Board::default();
        assert_eq!(board.groups_sanity_check(false), "");
        let g = add_groups(&mut board, 3);
        force_member(&mut board, g[0], g[1]);
        force_member(&mut board, g[1], g[2]);
        assert_eq!(board.groups_sanity_check(false), "");
        assert_eq!(board.groups().count(), 3);
    }

    #[test]
    fn footprint_and_pad_lookup() {
        let mut fx = crate::test_support::rotated_flipped_scaled();
        let pad = fx.child(ItemKind::Pad);
        let path = KiidPath::parse("/00000000-0000-0000-0000-0000000000aa").unwrap();
        fx.board.footprints.get_mut(fx.footprint).unwrap().path = path.clone();

        let board = &fx.board;
        assert_eq!(board.find_footprint_by_path(&path), Some(fx.footprint));
        assert_eq!(board.find_footprint_by_path(&KiidPath::new()), None);

        let uuid = board.uuid_of(pad).unwrap();
        assert_eq!(board.find_pad_by_uuid(&uuid).map(ItemId::from), Some(pad));
        assert_eq!(board.find_pad_by_uuid(&Kiid::default()), None);

        assert_eq!(board.get_zone_list(false).len(), 0);
        assert_eq!(
            board.get_zone_list(true).len(),
            board.footprint_zones(fx.footprint).count()
        );

        // One pad: the sorted list holds it, and the binary search finds it at its position on
        // its layers only.
        let ctx = BoardCtx::bare(board);
        let pads = board.get_sorted_pad_list_by_x_then_y_coord(&ctx, None);
        assert_eq!(pads.len(), board.footprint_pads(fx.footprint).count());
        let pad_id = pads[0];
        let pos = board.pads.get(pad_id).unwrap().position(&ctx);
        let layers = board.pads.get(pad_id).unwrap().layer_set(&ctx);
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &pads, pos, &layers),
            Some(pad_id)
        );
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &pads, pos + Vector2I::new(1, 0), &layers),
            None
        );
        assert_eq!(
            board.get_pad_in_sorted_list(
                &ctx,
                &pads,
                pos,
                &Lset::from_layers(&[PcbLayerId::Rescue])
            ),
            None
        );
        assert_eq!(board.get_pad_in_sorted_list(&ctx, &[], pos, &layers), None);
        assert!(
            board
                .get_sorted_pad_list_by_x_then_y_coord(&ctx, Some(NetCode(12345)))
                .is_empty()
        );
    }

    /// `sortPadsByXthenYCoord` through the libstdc++ sort, and the binary search over several
    /// pads, including two at the same position on different layers. The fixture's own pad
    /// sits at the origin on every copper layer.
    #[test]
    fn sorted_pad_binary_search() {
        let mut fx = crate::test_support::board_with_footprint(Default::default(), false);
        let positions = [(5, 5), (1, 9), (5, 1), (3, 3), (5, 1)];
        let mut ids = Vec::new();

        for (i, &(x, y)) in positions.iter().enumerate() {
            let mut pad = crate::pad::Pad::default();
            pad.lib_pos = Vector2I::new(x, y);
            let layer = if i == 4 {
                PcbLayerId::BCu
            } else {
                PcbLayerId::FCu
            };
            pad.padstack.layer_set = Lset::from_layers(&[layer]);
            ids.push(PadId::from(fx.add_child(pad, |f| &mut f.pads)));
        }

        let board = &fx.board;
        let ctx = BoardCtx::bare(board);
        let sorted = board.get_sorted_pad_list_by_x_then_y_coord(&ctx, None);
        let got: Vec<Vector2I> = sorted
            .iter()
            .map(|&p| board.pads.get(p).unwrap().position(&ctx))
            .collect();
        let want: Vec<Vector2I> = [(0, 0), (1, 9), (3, 3), (5, 1), (5, 1), (5, 5)]
            .iter()
            .map(|&(x, y)| Vector2I::new(x, y))
            .collect();
        assert_eq!(got, want);

        let b_cu = Lset::from_layers(&[PcbLayerId::BCu]);
        let f_cu = Lset::from_layers(&[PcbLayerId::FCu]);
        let at = Vector2I::new(5, 1);
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &sorted, at, &b_cu),
            Some(ids[4])
        );
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &sorted, at, &f_cu),
            Some(ids[2])
        );
        let at = Vector2I::new(1, 9);
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &sorted, at, &f_cu),
            Some(ids[1])
        );
        assert_eq!(board.get_pad_in_sorted_list(&ctx, &sorted, at, &b_cu), None);
        assert_eq!(
            board.get_pad_in_sorted_list(&ctx, &sorted, Vector2I::new(0, 0), &b_cu),
            Some(sorted[0])
        );
    }

    fn segment(
        board: &mut Board,
        layer: PcbLayerId,
        start: (i32, i32),
        end: (i32, i32),
    ) -> TrackId {
        let mut t = PcbTrack::new(&mut board.kiid_generator);
        t.start = Vector2I::new(start.0, start.1);
        t.end = Vector2I::new(end.0, end.1);
        t.base.item.layer = layer;
        board.add_track(Track::Trace(t), AddMode::Append).unwrap()
    }

    #[test]
    fn bounding_box_of_tracks() {
        let mut board = Board::default();
        assert_eq!(
            board.compute_bounding_box(&BoardCtx::bare(&board), false, false),
            Box2I::default()
        );
        segment(&mut board, PcbLayerId::FCu, (0, 0), (1_000_000, 0));
        segment(
            &mut board,
            PcbLayerId::BCu,
            (-500_000, 2_000_000),
            (0, 3_000_000),
        );

        let ctx = BoardCtx::bare(&board);
        let mut expected = Box2I::default();
        for (id, _) in board.tracks() {
            expected.merge(&ctx.item(id.into()).unwrap().bounding_box());
        }
        assert_eq!(board.compute_bounding_box(&ctx, false, false), expected);
        assert_eq!(board.compute_bounding_box(&ctx, false, true), expected);
        assert_eq!(board.board_edges_bounding_box(&ctx), Box2I::default());
        assert!(!board.is_footprint_holder());
    }

    #[test]
    fn max_clearance_value_is_cached_per_time_stamp() {
        let mut board = Board::default();
        let ctx = BoardCtx::bare(&board);
        let expected = board.design_settings().get_biggest_clearance_value(None);
        assert_eq!(board.get_max_clearance_value(&ctx), expected);

        board.design_settings_mut().min_clearance = 12_345_678;
        let ctx = BoardCtx::bare(&board);
        assert_eq!(board.get_max_clearance_value(&ctx), 12_345_678);
    }
}
