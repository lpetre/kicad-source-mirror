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

//! Port of `pcbnew/teardrop/teardrop.cpp` and `teardrop.h` (`TEARDROP_MANAGER`: teardrop
//! creation, removal, priorities, track-to-track teardrops); the geometry of
//! `teardrop_utils.cpp` is in [`utils`]. `TEARDROP_PARAMETERS` lives in
//! `kicad_pcb::teardrop_parameters`.
//!
//! A teardrop area is a polygonal area (a copper ZONE) having:
//! 2 points on the track connected to a pad or via,
//! 2 points on the outline of this pad or via,
//! 1 point near the pad/via position (calculated in order to have this pad/via position
//! inside the area).
//! The 2 sides joining a point on the track to the corresponding point on the pad/via
//! outline can be a straight line or a curved shape (defined from a Bezier curve).
//! Size of area (height and length) are defined from the pad/via size or for pads having
//! a size X and a size Y, the smallest of X,Y size.
//! For a custom pad that size and position describe only the anchor; the copper may lie far
//! away and need not be convex or connected, so anchor-derived geometry must be clamped to that
//! copper.
//!
//! Differences in form (see `PORT_STATUS.md`): there is no `BOARD_COMMIT` (the board is edited
//! directly, as the commit's `Added` / `Removed` only record), `STRUCT_DELETED` is a local set,
//! the board and its connectivity are passed to each call instead of being held, and the
//! address-ordered containers of the C++ (`GetConnectedPadsAndVias`, `std::set<PCB_TRACK*>`)
//! use the CN-01 / `ItemId` order (connectivity-zones.md R-1).

pub mod utils;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use kicad_connectivity::data::ConnectivityData;
use kicad_core::layer_ids::is_external_copper_layer;
use kicad_core::{Kiid, PCB_IU_SCALE, PcbLayerId};
use kicad_drc::rtree::DrcRtree;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::stdsort::std_sort;
use kicad_geom::{Box2I, Seg, Shape, ShapeArc, Vector2I, ki_round};
use kicad_pcb::board::Board;
use kicad_pcb::board::containers::AddMode;
use kicad_pcb::ctx::{BoardCtx, RuleResolver};
use kicad_pcb::ids::{ItemId, TrackId, ZoneId};
use kicad_pcb::item::ParentRef;
use kicad_pcb::pcb_track::{PcbTrack, Track};
use kicad_pcb::teardrop_parameters::{TargetTd, TeardropParameters, TeardropType};
use kicad_pcb::traits::BoardItem;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{
    IslandRemovalMode, ZoneBorderDisplayStyle, ZoneConnection, ZoneSettings,
};

pub use utils::{TrackBuffer, get_width, is_round};

use utils::{copper_netcode, outline_bbox};

/// The first priority level of a teardrop area (arbitrary value).
pub const MAGIC_TEARDROP_ZONE_ID: u32 = 30000;

/// `TEARDROP_MANAGER::TEARDROP_VARIANT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeardropVariant {
    /// `TD_TYPE_PADVIA`: a teardrop on a pad or via.
    PadVia,
    /// `TD_TYPE_TRACKEND`: a teardrop on a round end of a wide track.
    TrackEnd,
}

/// The `PCB_TRACK* aTrack` of the geometry functions: a board track, or the crossing stub of
/// `buildCrossingStub` (a local `PCB_TRACK` that is not on the board).
#[derive(Clone, Copy, Debug)]
pub enum TdTrack<'s> {
    /// A track (segment or arc) of `Board::tracks`.
    Board(TrackId),
    /// A stub segment that is not on the board.
    Stub(&'s Track),
}

impl<'s> TdTrack<'s> {
    /// The track value (`None` for a stale handle).
    pub(crate) fn get<'b>(self, board: &'b Board) -> Option<&'b Track>
    where
        's: 'b,
    {
        match self {
            TdTrack::Board(id) => board.track(id),
            TdTrack::Stub(track) => Some(track),
        }
    }

    /// Pointer equality of the C++ (`aTrack != originalTrack`).
    pub(crate) fn same_as(&self, other: &TdTrack<'_>) -> bool {
        match (self, other) {
            (TdTrack::Board(a), TdTrack::Board(b)) => a == b,
            (TdTrack::Stub(a), TdTrack::Stub(b)) => std::ptr::eq(*a, *b),
            _ => false,
        }
    }
}

/// Port of `TEARDROP_MANAGER`: manage and build teardrop areas.
#[derive(Debug, Default)]
pub struct TeardropManager {
    /// m_tolerance: max dist between track end point and pad/via center to see them connected
    /// to ut a teardrop.
    pub(crate) tolerance: i32,
    /// m_tracksRTree.
    pub(crate) tracks_rtree: DrcRtree,
    /// m_trackLookupList.
    pub(crate) track_lookup_list: TrackBuffer,
    /// m_createdTdList: list of new created teardrops.
    pub(crate) created_td_list: Vec<ZoneId>,
    /// m_createdTdRTree: the teardrops of this pass by layer, since m_copperRTree cannot take
    /// inserts once built. A list of (outline bbox, zone): only "does any collide" is asked of
    /// it, so the visit order of the dynamic r-tree does not matter.
    pub(crate) created_td_rtree: BTreeMap<PcbLayerId, Vec<(Box2I, ZoneId)>>,
    /// m_copperRTree: every copper item on the board before this pass, to keep teardrops off
    /// other nets.
    pub(crate) copper_rtree: DrcRtree,
    /// m_copperIndexed.
    pub(crate) copper_indexed: bool,
    /// m_zoneConnectionCache.
    pub(crate) zone_connection_cache: HashMap<(ZoneId, PcbLayerId), Vec<BTreeSet<ItemId>>>,
    /// m_pairClearanceCache.
    pub(crate) pair_clearance_cache: HashMap<(TrackId, ItemId, PcbLayerId), i32>,
}

/// The `BoardCtx` the C++ code sees through `m_board`: the board, its connectivity and the
/// DRC engine of the design settings.
fn make_ctx<'a>(
    board: &'a Board,
    connectivity: &'a ConnectivityData,
    rules: Option<&'a dyn RuleResolver>,
) -> BoardCtx<'a> {
    let ctx = BoardCtx::bare(board).with_connectivity(connectivity);

    match rules {
        Some(r) => ctx.with_rules(r),
        None => ctx,
    }
}

/// `BOARD::BulkRemoveStaleTeardrops`: removes (back to front) the teardrop zones of
/// `deleted` from the board and from connectivity.
fn bulk_remove_stale_teardrops(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    deleted: &HashSet<ZoneId>,
) {
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    for &id in zones.iter().rev() {
        let is_teardrop = board.zone(id).is_some_and(|z| z.is_teardrop_area());

        if is_teardrop && deleted.contains(&id) {
            // m_itemIndex.Unbind, m_zones.erase, IncrementTimeStamp, m_connectivity->Remove:
            // the connectivity removal needs the zone, so it comes first here.
            connectivity.remove(&BoardCtx::bare(board), id.into());
            board.remove(id.into());

            // aCommit.Removed( zone ): no commit.
        }
    }
}

impl TeardropManager {
    /// Port of `TEARDROP_MANAGER::TEARDROP_MANAGER( aBoard, aToolManager )`
    /// (`m_prmsList` is read from the board's design settings where it is used).
    pub fn new() -> Self {
        TeardropManager::default()
    }

    /// Port of `TEARDROP_MANAGER::teardropUuid`: the UUID a teardrop is created with.
    ///
    /// Deriving the UUID from the pair keeps teardrop ordering stable across save/load.  A
    /// crossing track yields two from one pair, so each takes a slot, and a slot spans two
    /// UUIDs.
    pub fn teardrop_uuid(track: &Kiid, candidate: &Kiid, slot: i32) -> Kiid {
        let mut uuid = Kiid::combine(track, candidate);

        for _ in 0..2 * slot {
            uuid.increment();
        }

        uuid
    }

    /// Port of `TEARDROP_MANAGER::maskUuidFor`: the UUID of the mask sibling of `copper_uuid`.
    pub fn mask_uuid_for(copper_uuid: &Kiid) -> Kiid {
        let mut uuid = *copper_uuid;
        uuid.increment();
        uuid
    }

    /// Port of `TEARDROP_MANAGER::buildCrossingStub`: `stub` set up as the segment from one
    /// end of `track` (set by the caller) to the point of `track` nearest `crossed_pos`, for a
    /// track that crosses the pad or via it connects to.
    pub fn build_crossing_stub(stub: &mut PcbTrack, track: &Track, crossed_pos: Vector2I) {
        // Copying aTrack would slice a PCB_ARC while the copy kept PCB_ARC_T, so the arc
        // branches would read a mid-point that is not there.
        let t = track.as_track();
        stub.base.item.layer = t.base.item.layer;
        stub.set_width(t.width);
        stub.base.net = t.base.net;
        stub.set_has_solder_mask(t.has_solder_mask());
        stub.set_local_solder_mask_margin(t.local_solder_mask_margin());

        // Aim the stub along the track centreline. The crossed centre can sit anywhere within
        // the track width, and the teardrop follows the stub.
        if let Track::Arc(arc) = track {
            let shape = ShapeArc::from_start_mid_end(arc.track.start, arc.mid, arc.track.end, 0);

            stub.set_end(shape.nearest_point(crossed_pos));
        } else {
            stub.set_end(Seg::new(t.start, t.end).nearest_point(crossed_pos));
        }
    }

    /// `aSourceTrack->GetSolderMaskExpansion()`.
    fn solder_mask_expansion(ctx: &BoardCtx<'_>, track: &Track) -> i32 {
        match track {
            Track::Arc(arc) => arc.solder_mask_expansion(ctx),
            _ => track.as_track().solder_mask_expansion(ctx),
        }
    }

    /// `new ZONE( m_board )` followed by
    /// `ZONE_SETTINGS::GetDefaultSettings().ExportSetting( *teardrop, false )` and the
    /// settings `createTeardrop` / `createTeardropMask` share.
    fn new_teardrop_zone(board: &mut Board, variant: TeardropVariant, uuid: Kiid) -> Zone {
        let board_defaults = board.design_settings().default_zone_settings().clone();
        let mut teardrop =
            Zone::new_with_settings(&mut board.kiid_generator, &board_defaults, false);

        teardrop.base.item.uuid = uuid;

        // Pristine rather than the board's, so nothing the user set up for a pour (hatch fill,
        // rule area, locking) leaks into a teardrop.
        ZoneSettings::default_settings().export_setting(&mut teardrop, false);

        teardrop.teardrop_type = match variant {
            TeardropVariant::PadVia => TeardropType::TdViapad,
            TeardropVariant::TrackEnd => TeardropType::TdTrackend,
        };

        teardrop
    }

    /// Port of `TEARDROP_MANAGER::createTeardrop`: a teardrop zone from its polygonal shape,
    /// track netcode and layer.
    fn create_teardrop(
        board: &mut Board,
        variant: TeardropVariant,
        points: &[Vector2I],
        source_track: TrackId,
        uuid: Kiid,
    ) -> Option<Zone> {
        let source = board.track(source_track)?.as_track();
        let layer = source.base.item.layer;
        let net = source.base.net;

        let mut teardrop = Self::new_teardrop_zone(board, variant, uuid);
        let env = kicad_pcb::ctx::MutEnv::detached();

        // Add zone properties (priority will be fixed later)
        teardrop.set_layer(&env, layer);
        // SetNetCode( aSourceTrack->GetNetCode(), true ): the track's net exists on the board.
        teardrop.base.net = net;
        teardrop.zone_clearance = 0; // SetLocalClearance( 0 )
        teardrop.set_min_thickness(PCB_IU_SCALE.mm_to_iu(0.0254)); // The minimum zone thickness
        teardrop.pad_connection = ZoneConnection::Full;
        teardrop.fill_mut().set_is_filled(false);
        teardrop.island_removal_mode = IslandRemovalMode::Never;
        teardrop.set_border_display_style(&env, ZoneBorderDisplayStyle::InvisibleBorder, 0, false);

        let outline = teardrop.lib_outline_mut();
        outline.new_outline();

        for pt in points {
            outline.append(pt.x, pt.y, -1, -1, false);
        }

        // Until we know better (ie: pay for a potentially very expensive zone refill), the
        // teardrop fill is the same as its outline.
        let fill = teardrop.library_outline().clone();
        teardrop.fill_mut().set_filled_polys_list(layer, &fill);
        teardrop.fill_mut().set_is_filled(true);

        // Used in priority calculations:
        teardrop.calculate_filled_area();

        Some(teardrop)
    }

    /// Port of `TEARDROP_MANAGER::createTeardropMask`: the solder mask opening of a teardrop
    /// on an external layer.
    fn create_teardrop_mask(
        board: &mut Board,
        variant: TeardropVariant,
        points: &[Vector2I],
        source_track: TrackId,
        uuid: Kiid,
    ) -> Option<Zone> {
        let (layer, expansion) = {
            let ctx = BoardCtx::bare(board);
            let source = board.track(source_track)?;
            (
                source.as_track().base.item.layer,
                Self::solder_mask_expansion(&ctx, source),
            )
        };
        let max_error = board.design_settings().max_error();

        // The second UUID of the slot, so the mask differs from the copper it covers.
        // As for the copper teardrop.  The ZONE constructor imports the board's zone
        // defaults, which follow the last pour the user set up.
        let mut teardrop = Self::new_teardrop_zone(board, variant, Self::mask_uuid_for(&uuid));
        let env = kicad_pcb::ctx::MutEnv::detached();

        teardrop.set_layer(
            &env,
            if layer == PcbLayerId::FCu {
                PcbLayerId::FMask
            } else {
                PcbLayerId::BMask
            },
        );
        teardrop.set_min_thickness(PCB_IU_SCALE.mm_to_iu(0.0254)); // The minimum zone thickness
        teardrop.fill_mut().set_is_filled(false);
        teardrop.island_removal_mode = IslandRemovalMode::Never;
        teardrop.set_border_display_style(&env, ZoneBorderDisplayStyle::InvisibleBorder, 0, false);

        let outline = teardrop.lib_outline_mut();
        outline.new_outline();

        for pt in points {
            outline.append(pt.x, pt.y, -1, -1, false);
        }

        if expansion != 0 {
            // The zone-min-thickness deflate/reinflate is going to round corners, so it's more
            // efficient to allow acute corners on the solder mask expansion here, and delegate
            // the rounding to the deflate/reinflate.
            let min_thickness = teardrop.min_thickness().max(expansion);
            teardrop.set_min_thickness(min_thickness);

            teardrop.lib_outline_mut().inflate(
                expansion,
                CornerStrategy::AllowAcuteCorners,
                max_error,
                false,
            );
        }

        // Until we know better (ie: pay for a potentially very expensive zone refill), the
        // teardrop fill is the same as its outline.
        let fill = teardrop.library_outline().clone();
        let mask_layer = teardrop.first_layer();
        teardrop.fill_mut().set_filled_polys_list(mask_layer, &fill);
        teardrop.fill_mut().set_is_filled(true);

        Some(teardrop)
    }

    /// Port of `TEARDROP_MANAGER::createAndAddTeardropWithMask`: creates and adds a teardrop
    /// with optional mask to the board.
    fn create_and_add_teardrop_with_mask(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        variant: TeardropVariant,
        points: &[Vector2I],
        source_track: TrackId,
        uuid: Kiid,
    ) {
        let Some(new_teardrop) = Self::create_teardrop(board, variant, points, source_track, uuid)
        else {
            return;
        };

        let bbox = outline_bbox(new_teardrop.library_outline());
        let id = board.add_zone(new_teardrop, AddMode::BulkInsert);
        connectivity.add(&BoardCtx::bare(board), id.into());

        self.created_td_list.push(id);

        let (layer, has_mask) = {
            let source = board.track(source_track).expect("source track").as_track();
            (source.base.item.layer, source.has_solder_mask())
        };

        // The next teardrop has to see this one, or two of them flare into the same gap.
        self.created_td_rtree
            .entry(layer)
            .or_default()
            .push((bbox, id));

        // aCommit.Added( new_teardrop ): no commit.

        if has_mask && is_external_copper_layer(layer) {
            if let Some(mask) =
                Self::create_teardrop_mask(board, variant, points, source_track, uuid)
            {
                let mask_id = board.add_zone(mask, AddMode::BulkInsert);
                connectivity.add(&BoardCtx::bare(board), mask_id.into());
            }
        }
    }

    /// Port of `TEARDROP_MANAGER::tryCreateTrackTeardrop`: attempts to create a teardrop on
    /// `track` (the board track `source_track`, or a stub of it) towards `candidate`.
    #[allow(clippy::too_many_arguments)]
    fn try_create_track_teardrop(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: Option<&dyn RuleResolver>,
        params: &TeardropParameters,
        variant: TeardropVariant,
        track: TdTrack<'_>,
        source_track: TrackId,
        candidate: ItemId,
        pos: Vector2I,
        uuid: Kiid,
    ) -> bool {
        let points = {
            let ctx = make_ctx(board, connectivity, rules);
            self.compute_fitted_teardrop_polygon(&ctx, params, track, source_track, candidate, pos)
        };

        if let Some(points) = points {
            self.create_and_add_teardrop_with_mask(
                board,
                connectivity,
                variant,
                &points,
                source_track,
                uuid,
            );
            return true;
        }

        false
    }

    /// Port of `TEARDROP_MANAGER::RemoveTeardrops`: remove teardrops on dirty pads, vias or
    /// tracks, and any whose neighbouring copper moved, dirtying the anchors of the latter.
    /// Call this BEFORE connectivity is updated.
    ///
    /// `dirty_pads_and_vias` and `dirty_tracks` are extended with the anchors of teardrops
    /// whose neighbours moved.
    pub fn remove_teardrops(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        dirty_pads_and_vias: &mut Vec<ItemId>,
        dirty_tracks: &mut BTreeSet<ItemId>,
        dirty_copper: Option<&[ItemId]>,
    ) {
        /// `TEARDROP_ANCHORS`.
        #[derive(Default)]
        struct TeardropAnchors {
            pads: Vec<ItemId>,
            vias: Vec<ItemId>,
            tracks: Vec<ItemId>,
        }

        /// `DIRTY_COPPER`.
        struct DirtyCopper {
            bbox: Box2I,
            netcode: i32,
        }

        let mut deleted: HashSet<ZoneId> = HashSet::new();

        {
            let ctx = BoardCtx::bare(board).with_connectivity(&*connectivity);

            let mut masks: Vec<ZoneId> = Vec::new();
            let mut copper_teardrops: Vec<ZoneId> = Vec::new();
            let mut anchors: HashMap<ZoneId, TeardropAnchors> = HashMap::new();

            for (zone_id, zone) in board.zones() {
                if !zone.is_teardrop_area() {
                    continue;
                }

                // Connectivity knows nothing of a mask layer, so a mask teardrop is never stale
                // on its own.  It goes when the copper it covers goes.
                if !zone.is_on_copper_layer(&ctx) {
                    masks.push(zone_id);
                    continue;
                }

                copper_teardrops.push(zone_id);

                let zone_anchors = anchors.entry(zone_id).or_default();

                connectivity.connected_pads_and_vias(
                    zone_id.into(),
                    &mut zone_anchors.pads,
                    &mut zone_anchors.vias,
                );
                zone_anchors.tracks = connectivity.connected_tracks(zone_id.into());
            }

            // A footprint move pushes every copper descendant, and the test below runs against
            // the whole list once per teardrop.
            let mut dirty_copper_by_layer: BTreeMap<PcbLayerId, Vec<DirtyCopper>> = BTreeMap::new();

            if let Some(dirty_copper) = dirty_copper {
                for &item in dirty_copper {
                    let Some(item_ref) = ctx.item(item) else {
                        continue;
                    };

                    for layer in item_ref.layer_set().cu_stack().iter() {
                        dirty_copper_by_layer
                            .entry(*layer)
                            .or_default()
                            .push(DirtyCopper {
                                bbox: item_ref.bounding_box(),
                                netcode: copper_netcode(&item_ref),
                            });
                    }
                }
            }

            let max_clearance = board.get_max_clearance_value(&ctx);

            // A width fitted to the neighbours goes stale when one of them moves, though the
            // teardrop anchors on neither.  Pre- and post-edit geometry both count, so moving
            // away counts too.
            let foreign_neighbour_moved = |zone_id: ZoneId| -> bool {
                let Some(zone) = board.zone(zone_id) else {
                    return false;
                };
                let layer = zone.first_layer();

                let Some(items) = dirty_copper_by_layer.get(&layer) else {
                    return false;
                };

                // The fit resolves clearance per pair, so no one number bounds the
                // neighbourhood. Take the widest anything can demand; over-retiring only costs
                // a rebuild.
                let mut reach = zone.bounding_box(&ctx);

                reach.inflate(max_clearance);

                for item in items {
                    // Net 0 is "no net", not a net that every unassigned item shares.
                    if zone.base.net.0 > 0 && item.netcode == zone.base.net.0 {
                        continue;
                    }

                    if reach.intersects(&item.bbox) {
                        return true;
                    }
                }

                false
            };

            let mut dirty_pad_via_set: HashSet<ItemId> =
                dirty_pads_and_vias.iter().copied().collect();

            // Dirty the anchors first, or the rebuild passes these teardrops by and they are
            // lost. Doing it here also lets the staleness pass below see the lists
            // UpdateTeardrops() will.
            for &zone_id in &copper_teardrops {
                if !foreign_neighbour_moved(zone_id) {
                    continue;
                }

                let zone_anchors = &anchors[&zone_id];

                for &pad in &zone_anchors.pads {
                    if dirty_pad_via_set.insert(pad) {
                        dirty_pads_and_vias.push(pad);
                    }
                }

                for &via in &zone_anchors.vias {
                    if dirty_pad_via_set.insert(via) {
                        dirty_pads_and_vias.push(via);
                    }
                }

                for &track in &zone_anchors.tracks {
                    dirty_tracks.insert(track);
                }
            }

            let is_stale = |zone_anchors: &TeardropAnchors| -> bool {
                zone_anchors
                    .pads
                    .iter()
                    .any(|item| dirty_pad_via_set.contains(item))
                    || zone_anchors
                        .vias
                        .iter()
                        .any(|item| dirty_pad_via_set.contains(item))
                    || zone_anchors
                        .tracks
                        .iter()
                        .any(|track| dirty_tracks.contains(track))
            };

            let mut surviving_copper: BTreeMap<PcbLayerId, Vec<ZoneId>> = BTreeMap::new();
            let mut mask_survives: HashMap<Kiid, bool> = HashMap::new();

            for &zone_id in &copper_teardrops {
                let stale = is_stale(&anchors[&zone_id]);
                let zone = board.zone(zone_id).expect("live zone");

                // A slot spans both UUIDs, so the pairing is exact rather than guessed from
                // geometry.
                mask_survives.insert(Self::mask_uuid_for(&zone.base.item.uuid), !stale);

                if stale {
                    deleted.insert(zone_id);
                } else {
                    surviving_copper
                        .entry(zone.first_layer())
                        .or_default()
                        .push(zone_id);
                }
            }

            for &mask_id in &masks {
                let mask = board.zone(mask_id).expect("live zone");

                let covers = if let Some(&survives) = mask_survives.get(&mask.base.item.uuid) {
                    survives
                } else {
                    // A mask predating the UUID spacing pairs with nothing, so fall back to
                    // concentricity (the expansion can be negative).  Erring towards a spare
                    // mask, not a lost opening.
                    let copper_layer = if mask.first_layer() == PcbLayerId::FMask {
                        PcbLayerId::FCu
                    } else {
                        PcbLayerId::BCu
                    };
                    let mask_bbox = mask.library_outline().bbox(0);

                    surviving_copper
                        .get(&copper_layer)
                        .into_iter()
                        .flatten()
                        .any(|&copper_id| {
                            let copper_bbox =
                                board.zone(copper_id).expect("live zone").bounding_box(&ctx);

                            mask_bbox.contains(copper_bbox.center())
                                && copper_bbox.contains(mask_bbox.center())
                        })
                };

                if !covers {
                    deleted.insert(mask_id);
                }
            }
        }

        bulk_remove_stale_teardrops(board, connectivity, &deleted);
    }

    /// Port of `TEARDROP_MANAGER::UpdateTeardrops( aCommit, dirtyPadsAndVias, dirtyTracks,
    /// aForceFullUpdate )`: update teardrops on a list of items (`None` lists are the C++
    /// null pointers, only valid with `force_full_update`).
    ///
    /// `rules` is `bds.m_DRCEngine` (used by the clearance fit; `None` reproduces a board
    /// without an engine).
    pub fn update_teardrops(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: Option<&dyn RuleResolver>,
        dirty_pads_and_vias: Option<&[ItemId]>,
        dirty_tracks: Option<&BTreeSet<ItemId>>,
        force_full_update: bool,
    ) {
        if board.legacy_teardrops {
            return;
        }

        // Init parameters:
        self.tolerance = PCB_IU_SCALE.mm_to_iu(0.01);

        // Old teardrops must be removed, to ensure a clean teardrop rebuild.  Before the caches
        // are built, or they index zones that are no longer on the board.
        if force_full_update {
            let deleted: HashSet<ZoneId> = board
                .zones()
                .filter(|(_, zone)| zone.is_teardrop_area())
                .map(|(id, _)| id)
                .collect();

            bulk_remove_stale_teardrops(board, connectivity, &deleted);
        }

        {
            let ctx = make_ctx(board, connectivity, rules);
            self.build_track_caches(&ctx);
        }

        let dirty_pad_via_set: HashSet<ItemId> = dirty_pads_and_vias
            .map(|list| list.iter().copied().collect())
            .unwrap_or_default();

        let tracks: Vec<TrackId> = board
            .tracks()
            .filter(|(_, track)| matches!(track, Track::Trace(_) | Track::Arc(_)))
            .map(|(id, _)| id)
            .collect();

        for track_id in tracks {
            let mut connected_pads: Vec<ItemId> = Vec::new();
            let mut connected_vias: Vec<ItemId> = Vec::new();

            connectivity.connected_pads_and_vias(
                track_id.into(),
                &mut connected_pads,
                &mut connected_vias,
            );

            let force_update = force_full_update
                || dirty_tracks.is_some_and(|set| set.contains(&ItemId::from(track_id)));

            for &pad_id in &connected_pads {
                if !force_update && !dirty_pad_via_set.contains(&pad_id) {
                    continue;
                }

                let Some(plan) =
                    self.plan_pad_via_teardrop(board, connectivity, rules, track_id, pad_id, true)
                else {
                    continue;
                };

                self.run_plan(board, connectivity, rules, track_id, pad_id, plan);
            }

            for &via_id in &connected_vias {
                if !force_update && !dirty_pad_via_set.contains(&via_id) {
                    continue;
                }

                let Some(plan) =
                    self.plan_pad_via_teardrop(board, connectivity, rules, track_id, via_id, false)
                else {
                    continue;
                };

                self.run_plan(board, connectivity, rules, track_id, via_id, plan);
            }
        }

        let track_target_enabled = board
            .design_settings()
            .teardrop_params_list()
            .parameters(TargetTd::Track)
            .enabled;

        if (force_full_update || dirty_tracks.is_some_and(|set| !set.is_empty()))
            && track_target_enabled
        {
            self.add_teardrops_on_tracks(
                board,
                connectivity,
                rules,
                dirty_tracks,
                force_full_update,
                false,
            );
        }

        // Now set priority of teardrops now all teardrops are added
        self.set_teardrop_priorities(board);
    }

    /// The checks of the pad (`is_pad`) or via loop of `UpdateTeardrops` for one track and
    /// one connected item: `None` where the C++ `continue`s.
    fn plan_pad_via_teardrop(
        &mut self,
        board: &Board,
        connectivity: &ConnectivityData,
        rules: Option<&dyn RuleResolver>,
        track_id: TrackId,
        item: ItemId,
        is_pad: bool,
    ) -> Option<TeardropPlan> {
        let ctx = make_ctx(board, connectivity, rules);
        let track = board.track(track_id)?;
        let t = track.as_track();
        let layer = t.base.item.layer;
        let item_ref = ctx.item(item)?;

        let (td_params, annular_width, position) = if is_pad {
            let pad = item_ref.as_pad()?;
            let pad_size = pad.size(&ctx, layer);
            (
                pad.base.teardrop_params.clone(),
                pad_size.x.min(pad_size.y),
                pad.position(&ctx),
            )
        } else {
            let via = item_ref.as_via()?;
            (
                via.track.base.teardrop_params.clone(),
                via.width(layer),
                via.position(&ctx),
            )
        };

        if !td_params.enabled {
            return None;
        }

        // Ensure a teardrop shape can be built: track width must be < teardrop width and
        // filter width.  A max width of 0 means no limit, not "nothing fits".
        let width = t.width as f64;

        if (td_params.td_max_width > 0 && t.width >= td_params.td_max_width)
            || width >= annular_width as f64 * td_params.best_width_ratio
            || width >= annular_width as f64 * td_params.width_to_size_filter_ratio
        {
            return None;
        }

        let (start_hits, end_hits) = if is_pad {
            let pad = item_ref.as_pad()?;
            (
                pad.hit_test_layer(&ctx, t.start, 0, layer),
                pad.hit_test_layer(&ctx, t.end, 0, layer),
            )
        } else {
            let via = item_ref.as_via()?;
            (via.hit_test(&ctx, t.start, 0), via.hit_test(&ctx, t.end, 0))
        };

        // The track is entirely inside the pad / via; cannot create a teardrop
        if start_hits && end_hits {
            return None;
        }

        // Reject tangential grazes, but keep short radial entries.
        if start_hits != end_hits
            && self.compute_chord_through_shape(
                &ctx,
                track,
                item,
                layer,
                if start_hits { t.start } else { t.end },
            ) < t.width
        {
            return None;
        }

        // Skip case where pad and the track are within a copper zone with the same net
        // (and the pad can be connected to the zone)
        if is_pad
            && !td_params.td_on_pads_in_zones
            && self.are_items_in_same_zone(&ctx, connectivity, item, track_id)
        {
            return None;
        }

        // A track crossing the pad earns one teardrop per side, not one over the whole track.
        let crossing = !start_hits && !end_hits && BoardItem::hit_test(track, &ctx, position, 0);

        Some(TeardropPlan {
            params: td_params,
            position,
            crossing,
        })
    }

    /// The teardrop creation of the pad / via loops of `UpdateTeardrops`.
    fn run_plan(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: Option<&dyn RuleResolver>,
        track_id: TrackId,
        item: ItemId,
        plan: TeardropPlan,
    ) {
        let (track_uuid, item_uuid) = {
            let Some(track) = board.track(track_id) else {
                return;
            };
            let Some(item_ref) = board.get(item) else {
                return;
            };
            (track.as_track().base.item.uuid, item_ref.base().uuid)
        };

        if plan.crossing {
            let track = board.track(track_id).expect("live track").clone();
            let mut stub = PcbTrack::new(&mut board.kiid_generator);
            stub.base.item.parent = ParentRef::Board;

            Self::build_crossing_stub(&mut stub, &track, plan.position);

            stub.set_start(track.as_track().end);
            let stub_track = Track::Trace(stub.clone());
            self.try_create_track_teardrop(
                board,
                connectivity,
                rules,
                &plan.params,
                TeardropVariant::PadVia,
                TdTrack::Stub(&stub_track),
                track_id,
                item,
                plan.position,
                Self::teardrop_uuid(&track_uuid, &item_uuid, 0),
            );

            stub.set_start(track.as_track().start);
            let stub_track = Track::Trace(stub);
            self.try_create_track_teardrop(
                board,
                connectivity,
                rules,
                &plan.params,
                TeardropVariant::PadVia,
                TdTrack::Stub(&stub_track),
                track_id,
                item,
                plan.position,
                Self::teardrop_uuid(&track_uuid, &item_uuid, 1),
            );
        } else {
            self.try_create_track_teardrop(
                board,
                connectivity,
                rules,
                &plan.params,
                TeardropVariant::PadVia,
                TdTrack::Board(track_id),
                track_id,
                item,
                plan.position,
                Self::teardrop_uuid(&track_uuid, &item_uuid, 0),
            );
        }
    }

    /// Port of `TEARDROP_MANAGER::DeleteTrackToTrackTeardrops`.
    pub fn delete_track_to_track_teardrops(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
    ) {
        let deleted: HashSet<ZoneId> = board
            .zones()
            .filter(|(_, zone)| {
                zone.is_teardrop_area() && zone.teardrop_type == TeardropType::TdTrackend
            })
            .map(|(id, _)| id)
            .collect();

        bulk_remove_stale_teardrops(board, connectivity, &deleted);
    }

    /// Port of `TEARDROP_MANAGER::setTeardropPriorities`: set priority of created teardrops;
    /// smaller have bigger priority.
    pub(crate) fn set_teardrop_priorities(&mut self, board: &mut Board) {
        // Note: a teardrop area is on only one layer, so using GetFirstLayer() is OK
        // to know the zone layer of a teardrop

        let mut priority_base: u32 = MAGIC_TEARDROP_ZONE_ID;

        for &td in &self.created_td_list {
            // ZONE::CalculateOutlineArea(): m_outlinearea = std::abs( m_Poly->Area() )
            let Some(area) = board.zone(td).map(|z| z.library_outline().area().abs()) else {
                continue;
            };

            board.modify_zone(td, |zone, _| zone.outline_area = area);
        }

        // The sort function to sort by increasing copper layers. Group by layers.
        // For same layers sort by decreasing areas
        {
            let compare_less = |a: &ZoneId, b: &ZoneId| -> bool {
                let (Some(a), Some(b)) = (board.zone(*a), board.zone(*b)) else {
                    return false;
                };

                if a.first_layer() == b.first_layer() {
                    if a.outline_area() != b.outline_area() {
                        return a.outline_area() > b.outline_area();
                    }

                    return a.base.item.uuid < b.base.item.uuid; // stable tiebreak
                }

                a.first_layer().0 < b.first_layer().0
            };

            std_sort(&mut self.created_td_list, compare_less);
        }

        // Survivors of an incremental update keep their priorities, and equal-priority zones
        // of different nets do not clear each other, so hand out what the layer still has free.
        let created: BTreeSet<ZoneId> = self.created_td_list.iter().copied().collect();
        let mut taken: BTreeMap<i32, BTreeSet<u32>> = BTreeMap::new();

        for (zone_id, zone) in board.zones() {
            if zone.is_teardrop_area() && !created.contains(&zone_id) {
                taken
                    .entry(zone.first_layer().0)
                    .or_default()
                    .insert(zone.assigned_priority());
            }
        }

        let mut curr_layer: i32 = -1;
        let empty = BTreeSet::new();

        for &td in &self.created_td_list {
            let Some(first_layer) = board.zone(td).map(|z| z.first_layer().0) else {
                continue;
            };

            if first_layer != curr_layer {
                curr_layer = first_layer;
                priority_base = MAGIC_TEARDROP_ZONE_ID;
            }

            let layer_taken = taken.get(&curr_layer).unwrap_or(&empty);

            while layer_taken.contains(&priority_base) && priority_base < u32::MAX {
                priority_base += 1;
            }

            let priority = priority_base;
            board.modify_zone(td, |zone, _| zone.set_assigned_priority(priority));

            if priority_base < u32::MAX {
                priority_base += 1;
            }
        }
    }

    /// Port of `TEARDROP_MANAGER::AddTeardropsOnTracks`: add teardrops on tracks of different
    /// sizes connected by their end.
    ///
    /// `set_priorities` is false when the caller numbers them itself once the rest are built.
    pub fn add_teardrops_on_tracks(
        &mut self,
        board: &mut Board,
        connectivity: &mut ConnectivityData,
        rules: Option<&dyn RuleResolver>,
        tracks: Option<&BTreeSet<ItemId>>,
        force_full_update: bool,
        set_priorities: bool,
    ) {
        let mut params = board
            .design_settings()
            .teardrop_params_list()
            .parameters(TargetTd::Track)
            .clone();

        let groups: Vec<i32> = self.track_lookup_list.map_tracks.keys().copied().collect();

        // Explore groups (a group is a set of tracks on the same layer and the same net):
        for key in groups {
            let (_layer, _netcode) = TrackBuffer::netcode_and_layer_from_index(key);

            let sublist_len = self.track_lookup_list.map_tracks[&key].len();

            if sublist_len <= 1 {
                // We need at least 2 track segments
                continue;
            }

            // The sort function to sort by increasing track widths
            {
                let sublist = self
                    .track_lookup_list
                    .map_tracks
                    .get_mut(&key)
                    .expect("group");
                let width_of = |id: &TrackId| board.track(*id).map_or(0, |t| t.width());

                std_sort(sublist, |a, b| width_of(a) < width_of(b));
            }

            let sublist: Vec<TrackId> = self.track_lookup_list.map_tracks[&key].clone();
            let width_of = |board: &Board, id: TrackId| board.track(id).map_or(0, |t| t.width());

            let mut min_width = width_of(board, sublist[0]);
            let max_width = width_of(board, sublist[sublist.len() - 1]);

            // Skip groups having the same track thickness
            if max_width == min_width {
                continue;
            }

            for ii in 0..sublist.len() - 1 {
                let track_id = sublist[ii];
                let (track_len, track_uuid) = {
                    let Some(track) = board.track(track_id) else {
                        continue;
                    };
                    min_width = track.width();
                    (
                        utils::trunc_i32(track.length()),
                        track.as_track().base.item.uuid,
                    )
                };
                let track_needs_update = force_full_update
                    || tracks.is_some_and(|set| set.contains(&ItemId::from(track_id)));

                // to avoid creating a teardrop between 2 tracks having similar widths give a
                // threshold
                params.width_to_size_filter_ratio = if params.width_to_size_filter_ratio < 0.1 {
                    0.1
                } else {
                    params.width_to_size_filter_ratio
                };
                let th = 1.0 / params.width_to_size_filter_ratio;
                min_width = ki_round(min_width as f64 * th);

                for &candidate_id in &sublist[ii + 1..] {
                    // Search candidates with thickness > curr thickness
                    let Some(candidate) = board.track(candidate_id) else {
                        continue;
                    };

                    if min_width >= candidate.width() {
                        continue;
                    }

                    // Cannot build a teardrop on a too short track segment.
                    // The min len is > candidate radius
                    if track_len <= candidate.width() / 2 {
                        continue;
                    }

                    // Now test end to end connection:
                    // match_points: the end point EDA_ITEM_FLAGS: 0, STARTPOINT, ENDPOINT
                    let track = board.track(track_id).expect("live track").as_track();

                    let mut pos = candidate.as_track().start;
                    let mut match_points = track.is_point_on_ends(pos, self.tolerance);

                    if match_points == 0 {
                        pos = candidate.as_track().end;
                        match_points = track.is_point_on_ends(pos, self.tolerance);
                    }

                    if match_points == 0 {
                        continue;
                    }

                    // An untouched pair's teardrop was not removed as stale, so building
                    // another would duplicate it, and its UUID, on every edit elsewhere on the
                    // board.
                    if !track_needs_update
                        && !tracks.is_some_and(|set| set.contains(&ItemId::from(candidate_id)))
                    {
                        continue;
                    }

                    // Pads/vias have priority for teardrops; ensure there isn't one at our
                    // position
                    let mut existing_pad_or_via = false;
                    let mut connected_pads: Vec<ItemId> = Vec::new();
                    let mut connected_vias: Vec<ItemId> = Vec::new();

                    connectivity.connected_pads_and_vias(
                        track_id.into(),
                        &mut connected_pads,
                        &mut connected_vias,
                    );

                    {
                        let ctx = make_ctx(board, connectivity, rules);

                        for &pad in &connected_pads {
                            if ctx
                                .item(pad)
                                .and_then(|r| r.as_pad())
                                .is_some_and(|p| p.hit_test(&ctx, pos, 0))
                            {
                                existing_pad_or_via = true;
                            }
                        }

                        for &via in &connected_vias {
                            if ctx
                                .item(via)
                                .and_then(|r| r.as_via())
                                .is_some_and(|v| v.hit_test(&ctx, pos, 0))
                            {
                                existing_pad_or_via = true;
                            }
                        }
                    }

                    if existing_pad_or_via {
                        continue;
                    }

                    let candidate_uuid = candidate.as_track().base.item.uuid;

                    self.try_create_track_teardrop(
                        board,
                        connectivity,
                        rules,
                        &params,
                        TeardropVariant::TrackEnd,
                        TdTrack::Board(track_id),
                        track_id,
                        candidate_id.into(),
                        pos,
                        Self::teardrop_uuid(&track_uuid, &candidate_uuid, 0),
                    );
                }
            }
        }

        // The global edit dialog calls this directly, and a teardrop left at the default
        // priority is outranked by every pour.  UpdateTeardrops() has more to add, so it
        // numbers them itself.
        if set_priorities {
            self.set_teardrop_priorities(board);
        }
    }
}

/// What the checks of a pad / via loop iteration of `UpdateTeardrops` hand to the creation.
struct TeardropPlan {
    /// The pad's / via's `TEARDROP_PARAMETERS`.
    params: TeardropParameters,
    /// `pad->GetPosition()` / `via->GetPosition()`.
    position: Vector2I,
    /// The track crosses the pad / via (one teardrop per side).
    crossing: bool,
}
