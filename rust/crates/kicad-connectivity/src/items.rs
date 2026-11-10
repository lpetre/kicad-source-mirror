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

//! Port of `pcbnew/connectivity/connectivity_items.h` and `connectivity_items.cpp`:
//! `CN_ANCHOR`, `CN_ITEM`, `CN_ZONE_LAYER`, `CN_LIST` and `CN_CLUSTER`.
//!
//! Ownership (board-model.md 13, connectivity-zones.md 10.1): the C++ heap objects linked by
//! raw pointers become values in the [`CnList`] arena, named by [`CnItemId`] (the arena slot).
//! Slots are handed out in creation order and never reused, and the list only ever appends and
//! compacts in order, so `CnItemId` order is always list order: it is the sort key of
//! `CN_ITEM::m_connected`, which C++ sorts by heap address (connectivity-zones.md R-1,
//! board-model.md 3.1 rule 1). `CN_ZONE_LAYER`, a `CN_ITEM` subclass, is a [`CnItem`] whose
//! `zone_layer` holds the zone-specific part. Board items are read through `&BoardCtx`; the
//! parent is an [`ItemId`].

use std::collections::{BTreeSet, HashMap};

use kicad_core::layer_ids::{MAX_CU_LAYERS, copper_layer_to_ordinal, to_layer_id};
use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::cmath;
use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_line_chain::ShapeLineChain;
use kicad_geom::geometry::shape_poly_set::TriangulatedPolygon;
use kicad_geom::geometry::shape_poly_tri_index::{
    TriangulationIndex, any_triangle_hit, any_triangle_hit_point,
};
use kicad_geom::geometry::shape_utils::get_circle_key_points;
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::util::ki_round;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::ids::{ItemId, PadId, TrackId, ZoneId};
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::teardrop_parameters::TeardropType;

use crate::rtree::{CnBulkEntry, CnRtree};

/// Handle of a [`CnItem`] in its [`CnList`] (C++ `CN_ITEM*`). Ordered like the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CnItemId(pub u32);

impl CnItemId {
    /// The handle of an item that is not (yet) in a list.
    pub const NONE: CnItemId = CnItemId(u32::MAX);

    /// The arena slot.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Handle of a `CN_CLUSTER` owned by the algorithm (C++ `shared_ptr<CN_CLUSTER>`); assigned
/// to anchors by the ratsnest (`RN_NET::AddCluster`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CnClusterId(pub u32);

/// Names one anchor stored in a [`CnItem`] (C++ `shared_ptr<CN_ANCHOR>` of `m_anchors`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CnAnchorRef {
    pub item: CnItemId,
    pub index: u32,
}

// =================================================================================================
// CN_ANCHOR
// =================================================================================================

/// Port of `CN_ANCHOR`: a physical location that can be connected: a pad or a
/// track / arc / via end point.
#[derive(Clone, Debug, PartialEq)]
pub struct CnAnchor {
    /// `m_pos`: position of the anchor.
    pos: Vector2I,
    /// `m_item`: pad or track / arc / via owning the anchor; `None` = C++ `nullptr` (the item
    /// was deleted, or the anchor is not in a list yet).
    item: Option<CnItemId>,
    /// `m_tag`: tag for quick connection resolution.
    tag: i32,
    /// `m_noline`: whether the node can be a target for ratsnest lines.
    noline: bool,
    /// `m_cluster`: cluster to which the anchor belongs.
    cluster: Option<CnClusterId>,
}

impl CnAnchor {
    /// Tag used for unconnected items (`TAG_UNCONNECTED`).
    pub const TAG_UNCONNECTED: i32 = -1;

    /// Port of `CN_ANCHOR( aPos, aItem )`.
    pub fn new(pos: Vector2I, item: Option<CnItemId>) -> Self {
        CnAnchor {
            pos,
            item,
            tag: -1,
            noline: false,
            cluster: None,
        }
    }

    /// `Valid()`: the owning item exists and is valid.
    pub fn valid(&self, list: &CnList) -> bool {
        match self.item {
            None => false,
            Some(id) => list.item(id).valid(),
        }
    }

    /// `Dirty()`: `!Valid() || m_item->Dirty()`.
    pub fn dirty(&self, list: &CnList) -> bool {
        !self.valid(list) || self.item.is_some_and(|id| list.item(id).dirty())
    }

    /// `Item()`.
    pub fn item(&self) -> Option<CnItemId> {
        self.item
    }

    /// `SetItem()`.
    pub fn set_item(&mut self, item: Option<CnItemId>) {
        self.item = item;
    }

    /// `Parent()` (C++ asserts `m_item->Valid()`).
    pub fn parent(&self, list: &CnList) -> ItemId {
        let item = list.item(self.item.expect("CN_ANCHOR::Parent: no item"));
        debug_assert!(item.valid());
        item.parent()
    }

    /// `Pos()`.
    pub fn pos(&self) -> Vector2I {
        self.pos
    }

    /// `Move()`.
    pub fn move_by(&mut self, delta: Vector2I) {
        self.pos += delta;
    }

    /// Port of `Dist()`: `( m_pos - aSecond.Pos() ).EuclideanNorm()` as `unsigned`.
    pub fn dist(&self, second: &CnAnchor) -> u32 {
        // int -> unsigned conversion of a norm that is never negative.
        (self.pos - second.pos()).euclidean_norm() as u32
    }

    /// `GetTag()`: a common identifier for connected nodes.
    pub fn tag(&self) -> i32 {
        self.tag
    }

    /// `SetTag()`.
    pub fn set_tag(&mut self, tag: i32) {
        self.tag = tag;
    }

    /// `GetNoLine()`: true if this node can be a target for ratsnest lines.
    pub fn no_line(&self) -> bool {
        self.noline
    }

    /// `SetNoLine()`.
    pub fn set_no_line(&mut self, enable: bool) {
        self.noline = enable;
    }

    /// `GetCluster()`.
    pub fn cluster(&self) -> Option<CnClusterId> {
        self.cluster
    }

    /// `SetCluster()`.
    pub fn set_cluster(&mut self, cluster: Option<CnClusterId>) {
        self.cluster = cluster;
    }

    /// Port of `CN_ANCHOR::IsDangling()`.
    ///
    /// The anchor point is dangling if the parent is a track and this anchor point is not
    /// connected to another item (track, vias pad or zone) or if the parent is a via and this
    /// anchor point is connected to only one track and not to another item.
    pub fn is_dangling(&self, ctx: &BoardCtx<'_>, list: &CnList) -> bool {
        let mut accuracy = 0;

        if self.cluster.is_none() {
            return true;
        }

        let item = list.item(self.item.expect("CN_ANCHOR::IsDangling: no item"));

        // the minimal number of items connected to item_ref
        // at this anchor point to decide the anchor is *not* dangling
        let minimal_count: usize = 1;
        let mut connected_count = item.connected_items().len();

        // a via can be removed if connected to only one other item.
        if item.parent_type() == KicadT::PcbViaT {
            return connected_count < 2;
        }

        if item.anchor_count() == 1 {
            return connected_count < minimal_count;
        }

        if let Some(parent) = ctx.item(item.parent()) {
            if let Some(track) = parent.as_track() {
                if matches!(item.parent_type(), KicadT::PcbTraceT | KicadT::PcbArcT) {
                    accuracy = ki_round(track.width() as f64 / 2.0);
                }
            } else if let Some(shape) = parent.as_shape() {
                if item.parent_type() == KicadT::PcbShapeT {
                    accuracy = ki_round(shape.width(ctx) as f64 / 2.0);
                }
            }
        }

        // Items with multiple anchors have usually items connected to each anchor.
        // We want only the item count of this anchor point
        connected_count = 0;

        for &c in item.connected_items() {
            if anchor_hits_item(ctx, list.item(c), self.pos, accuracy) {
                connected_count += 1;
            }
        }

        connected_count < minimal_count
    }

    /// Port of `CN_ANCHOR::ConnectedItemsCount()`: the count of connected items whose parent
    /// is hit at this anchor (zones: their fill on the item's layer).
    pub fn connected_items_count(&self, ctx: &BoardCtx<'_>, list: &CnList) -> i32 {
        if self.cluster.is_none() {
            return 0;
        }

        let item = list.item(self.item.expect("CN_ANCHOR::ConnectedItemsCount: no item"));
        let mut connected_count = 0;

        for &c in item.connected_items() {
            if anchor_hits_item(ctx, list.item(c), self.pos, 0) {
                connected_count += 1;
            }
        }

        connected_count
    }
}

/// The hit test shared by `IsDangling` and `ConnectedItemsCount`: zones test their fill on the
/// connected item's board layer (`ZONE::HitTestFilledArea`), other parents `HitTest( pos,
/// accuracy )`.
fn anchor_hits_item(ctx: &BoardCtx<'_>, item: &CnItem, pos: Vector2I, accuracy: i32) -> bool {
    let Some(parent) = ctx.item(item.parent()) else {
        return false;
    };

    if item.parent_type() == KicadT::PcbZoneT {
        let zone = parent.as_zone().expect("PCB_ZONE_T parent is a zone");
        return zone.hit_test_filled_area(ctx, item.board_layer(), pos, accuracy);
    }

    parent.as_dyn().hit_test(ctx, pos, accuracy)
}

// =================================================================================================
// CN_ITEM / CN_ZONE_LAYER
// =================================================================================================

/// Port of `CN_ZONE_LAYER` (the part beyond `CN_ITEM`): a single outline of a zone fill on a
/// particular layer; `subpoly_index` indicates which outline of the fill's `SHAPE_POLY_SET`.
#[derive(Clone, Debug)]
pub struct CnZoneLayer {
    /// `m_zone`
    zone: ZoneId,
    /// `m_subpolyIndex`
    subpoly_index: i32,
    /// `m_layer`
    layer: PcbLayerId,
    /// `m_outline`: cached copy of the zone outline.
    outline: ShapeLineChain,
    /// `m_zone->IsTeardropArea()`, read once at construction (the zone does not change while
    /// the connectivity data lives; C++ reads it on every contact test).
    teardrop_area: bool,
    /// `m_triangulatedPolys`: owned copies of the fill's triangulated polygons of this
    /// outline.
    triangulated_polys: Vec<TriangulatedPolygon>,
    /// `m_triIndex` (`None` = null `unique_ptr`).
    tri_index: Option<TriangulationIndex>,
}

impl CnZoneLayer {
    /// `GetZone()` (the `m_zone` member; C++ has no getter, `Parent()` is the same zone).
    pub fn zone(&self) -> ZoneId {
        self.zone
    }

    /// `SubpolyIndex()`.
    pub fn subpoly_index(&self) -> i32 {
        self.subpoly_index
    }

    /// `GetLayer()`.
    pub fn layer(&self) -> PcbLayerId {
        self.layer
    }

    /// Port of `CN_ZONE_LAYER::BuildIndexes()`: copies the fill's cached triangulation of this
    /// outline and indexes it. Teardrop zones build no index (their contact tests use the
    /// outline). The fill must have been triangulated before (`BOARD::CacheTriangulation`);
    /// without a triangulation the item has no triangles and contains nothing.
    pub fn build_indexes(&mut self, ctx: &BoardCtx<'_>) {
        if self.teardrop_area {
            return;
        }

        self.triangulated_polys.clear();
        self.tri_index = None;

        let Some(zone) = ctx.item(self.zone.into()).and_then(|z| z.as_zone()) else {
            return;
        };

        let Some(fill_poly) = zone.fill().filled_polys_list.get(&self.layer) else {
            return;
        };

        for ii in 0..fill_poly.triangulated_poly_count() {
            let triangle_set = fill_poly.triangulated_polygon(ii as i32);

            if triangle_set.source_outline_index() != self.subpoly_index {
                continue;
            }

            // Deep copy the triangulated polygon (the C++ copy constructor copies the vertex
            // storage so that the triangles stay valid if the zone is refilled).
            self.triangulated_polys.push(triangle_set.clone());
        }

        self.tri_index = Some(TriangulationIndex::new(&self.triangulated_polys));
    }

    /// Port of `CN_ZONE_LAYER::ContainsPoint()`: any triangle of the outline contains `p`
    /// (teardrop zones: the outline collides with `p`).
    pub fn contains_point(&self, p: Vector2I) -> bool {
        if self.outline.point_count() == 0 {
            return false;
        }

        if self.teardrop_area {
            return self.outline.collide_point(p, 0, None, None);
        }

        any_triangle_hit_point(self.tri_index.as_ref(), &self.triangulated_polys, p)
    }

    /// Port of `CN_ZONE_LAYER::Collide( SHAPE* )`: any triangle of the outline collides with
    /// `ref_shape` (teardrop zones: the shape collides with the outline).
    pub fn collide(&self, ref_shape: &dyn Shape) -> bool {
        if self.outline.point_count() == 0 {
            return false;
        }

        if self.teardrop_area {
            return ref_shape.collide(&self.outline, 0, None, None);
        }

        any_triangle_hit(
            self.tri_index.as_ref(),
            &self.triangulated_polys,
            ref_shape,
            0,
            None,
            None,
        )
    }

    /// `HasValidOutline()`.
    pub fn has_valid_outline(&self) -> bool {
        self.outline.point_count() > 0
    }

    /// `GetOutline()`.
    pub fn outline(&self) -> &ShapeLineChain {
        &self.outline
    }

    /// `OutlinePointCount()`.
    pub fn outline_point_count(&self) -> i32 {
        self.outline.point_count()
    }

    /// `OutlinePoint()`.
    pub fn outline_point(&self, index: i32) -> Vector2I {
        self.outline.c_point(index)
    }

    /// The owned triangulation copies (`m_triangulatedPolys`).
    pub fn triangulated_polys(&self) -> &[TriangulatedPolygon] {
        &self.triangulated_polys
    }
}

/// Port of `CN_ITEM`: a `BOARD_CONNECTED_ITEM` in the connectivity system (a pad, track / arc
/// / via, copper shape, or one outline of a zone fill on one layer).
#[derive(Clone, Debug)]
pub struct CnItem {
    /// This item's own handle (C++ `this`); `CnItemId::NONE` until added to a list.
    id: CnItemId,
    /// `m_parent`
    parent: ItemId,
    /// `m_parent->Type()`, cached (the parent kind never changes).
    parent_type: KicadT,
    /// `m_parent->GetNetCode()`. C++ reads the parent's net code live; the port keeps it here
    /// and the net propagation (`SetNetCode` of the parent) updates it through
    /// [`CnItem::set_net`], so that the algorithm sees its own net changes before the caller
    /// applies them to the board (board-model.md 13).
    net: i32,
    /// `m_connected`: physically touching items, sorted by `CnItemId` (C++: by address, R-1),
    /// without duplicates.
    connected: Vec<CnItemId>,
    /// `m_anchors`
    anchors: Vec<CnAnchor>,
    /// `m_canChangeNet`: can the net propagator modify the netcode?
    can_change_net: bool,
    /// `m_valid`: used to identify garbage items (lazy removal).
    valid: bool,
    /// `m_dirty`: recently added item not yet scanned into the connectivity search.
    dirty: bool,
    /// `m_start_layer` (copper ordinal).
    start_layer: i32,
    /// `m_end_layer` (copper ordinal).
    end_layer: i32,
    /// `m_bbox`: the parent's bounding box.
    bbox: Box2I,
    /// `m_listIndex`: position in the owning list, -1 outside a list.
    list_index: i32,
    /// The `CN_ZONE_LAYER` part, for zone items.
    zone_layer: Option<Box<CnZoneLayer>>,
}

impl CnItem {
    /// Port of `CN_ITEM( aParent, aCanChangeNet, aAnchorCount )`: `parent_type` and `net` are
    /// the parent's `Type()` and `GetNetCode()`, `bbox` its `GetBoundingBox()`.
    pub fn new(
        parent: ItemId,
        parent_type: KicadT,
        net: i32,
        bbox: Box2I,
        can_change_net: bool,
    ) -> Self {
        CnItem {
            id: CnItemId::NONE,
            parent,
            parent_type,
            net,
            connected: Vec::new(),
            anchors: Vec::new(),
            can_change_net,
            valid: true,
            dirty: true,
            start_layer: 0,
            end_layer: i32::MAX,
            bbox,
            list_index: -1,
            zone_layer: None,
        }
    }

    /// Port of `CN_ZONE_LAYER( aParent, aLayer, aSubpolyIndex )`: outline `subpoly_index` of
    /// the zone's fill on `layer` (none if there is no such outline). `None` for a stale zone
    /// handle.
    pub fn new_zone_layer(
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
        subpoly_index: i32,
    ) -> Option<Self> {
        let zone_ref = ctx.item(zone_id.into())?;
        let zone = zone_ref.as_zone()?;
        let mut item = CnItem::new(
            zone_id.into(),
            KicadT::PcbZoneT,
            net_of(&zone_ref),
            zone_ref.bounding_box(),
            false,
        );

        let mut outline = ShapeLineChain::default();

        if let Some(fill_poly) = zone.fill().filled_polys_list.get(&layer) {
            if subpoly_index < fill_poly.outline_count() {
                outline = fill_poly.c_outline(subpoly_index).clone();
            }
        }

        item.set_layers(layer.0, layer.0);
        item.zone_layer = Some(Box::new(CnZoneLayer {
            zone: zone_id,
            subpoly_index,
            layer,
            outline,
            teardrop_area: zone.teardrop_type != TeardropType::TdNone,
            triangulated_polys: Vec::new(),
            tri_index: None,
        }));

        Some(item)
    }

    /// This item's handle in its list (`CnItemId::NONE` outside a list).
    pub fn id(&self) -> CnItemId {
        self.id
    }

    /// Port of `AddAnchor()`: appends an anchor owned by this item and returns its index.
    pub fn add_anchor(&mut self, pos: Vector2I) -> u32 {
        let owner = (self.id != CnItemId::NONE).then_some(self.id);
        self.anchors.push(CnAnchor::new(pos, owner));
        (self.anchors.len() - 1) as u32
    }

    /// `Anchors()`.
    pub fn anchors(&self) -> &[CnAnchor] {
        &self.anchors
    }

    /// `Anchors()` (mutable: tags and clusters set by the ratsnest).
    pub fn anchors_mut(&mut self) -> &mut [CnAnchor] {
        &mut self.anchors
    }

    /// `SetValid()`.
    pub fn set_valid(&mut self, valid: bool) {
        self.valid = valid;
    }

    /// `Valid()`.
    pub fn valid(&self) -> bool {
        self.valid
    }

    /// `SetDirty()`.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// `Dirty()`.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Port of `SetLayers()`: the layers spanned by the item, stored as copper ordinals.
    ///
    /// Copper layer ids do not run in stack order, so store the ordinal. The R-tree keys on
    /// this, and an ordinal keeps a through item's span 31 wide rather than INT_MAX wide. A
    /// span that large overflows the int64 volume the tree splits on, which wrecks the split
    /// and leaves queries walking most of the tree.
    pub fn set_layers(&mut self, start_layer: i32, end_layer: i32) {
        self.start_layer = copper_layer_to_ordinal(to_layer_id(start_layer)) as i32;
        self.end_layer = copper_layer_to_ordinal(to_layer_id(end_layer)) as i32;
    }

    /// `SetLayer()`: a single layer.
    pub fn set_layer(&mut self, layer: i32) {
        self.set_layers(layer, layer);
    }

    /// `StartLayer()` (copper ordinal).
    pub fn start_layer(&self) -> i32 {
        self.start_layer
    }

    /// `EndLayer()` (copper ordinal).
    pub fn end_layer(&self) -> i32 {
        self.end_layer
    }

    /// `Layer()`: the item's layer, for single-layered items only. N.B. this is a copper layer
    /// ordinal and not a `PCB_LAYER_ID`.
    pub fn layer(&self) -> i32 {
        self.start_layer
    }

    /// Port of `BoardLayerFromOrdinal()`: converts a copper layer ordinal back to the board
    /// layer it stands for.
    pub fn board_layer_from_ordinal(ordinal: i32) -> PcbLayerId {
        if ordinal <= 0 {
            return PcbLayerId::FCu;
        }

        if ordinal >= MAX_CU_LAYERS - 1 {
            return PcbLayerId::BCu;
        }

        to_layer_id(PcbLayerId::BCu.0 + 2 * ordinal)
    }

    /// `GetBoardLayer()`: the board layer of a single-layered item.
    pub fn board_layer(&self) -> PcbLayerId {
        Self::board_layer_from_ordinal(self.layer())
    }

    /// `BBox()`: the parent's bounding box, taken when the item was added to the list. C++
    /// recomputes it lazily (and clears the dirty flag) on the first call after the item was
    /// marked dirty; the parent cannot change while the connectivity data lives, so the value
    /// is the same. The side effect on the dirty flag only made the C++ contact search test a
    /// symmetric pair from both sides, which changes nothing (connectivity-zones.md 4.2).
    pub fn bbox(&self) -> &Box2I {
        &self.bbox
    }

    /// Re-reads the parent's bounding box (the lazy recomputation of `BBox()` after the
    /// parent changed).
    pub fn update_bbox(&mut self, ctx: &BoardCtx<'_>) {
        if let Some(parent) = ctx.item(self.parent) {
            self.bbox = parent.bounding_box();
        }
    }

    /// `Parent()`.
    pub fn parent(&self) -> ItemId {
        self.parent
    }

    /// `Parent()->Type()`.
    pub fn parent_type(&self) -> KicadT {
        self.parent_type
    }

    /// `ConnectedItems()`.
    pub fn connected_items(&self) -> &[CnItemId] {
        &self.connected
    }

    /// `ClearConnections()`.
    pub fn clear_connections(&mut self) {
        self.connected.clear();
    }

    /// `CanChangeNet()`.
    pub fn can_change_net(&self) -> bool {
        self.can_change_net
    }

    /// Port of `Connect()`: inserts `b` into the sorted connected list unless present.
    pub fn connect(&mut self, b: CnItemId) {
        match self.connected.binary_search(&b) {
            Ok(_) => {}
            Err(i) => self.connected.insert(i, b),
        }
    }

    /// Port of `CN_ITEM::RemoveInvalidRefs()`: drops connections to invalid items.
    pub fn remove_invalid_refs(&mut self, valid: impl Fn(CnItemId) -> bool) {
        self.connected.retain(|&c| valid(c));
    }

    /// Port of `CN_ITEM::AnchorCount()` (and of the `CN_ZONE_LAYER` override).
    pub fn anchor_count(&self) -> i32 {
        if let Some(zl) = &self.zone_layer {
            if !self.valid || !zl.has_valid_outline() {
                return 0;
            }

            return if zl.outline.point_count() != 0 { 1 } else { 0 };
        }

        if !self.valid {
            return 0;
        }

        match self.parent_type {
            KicadT::PcbTraceT | KicadT::PcbArcT => 2, // start and end
            KicadT::PcbShapeT => self.anchors.len() as i32,
            _ => 1,
        }
    }

    /// Port of `CN_ITEM::GetAnchor( n )` (and of the `CN_ZONE_LAYER` override): pads give
    /// their position, tracks / arcs start (`n == 0`) or end, vias their start, shapes their
    /// stored anchors, zone layers the first outline point.
    pub fn anchor(&self, ctx: &BoardCtx<'_>, n: i32) -> Vector2I {
        if let Some(zl) = &self.zone_layer {
            if !self.valid || !zl.has_valid_outline() {
                return Vector2I::default();
            }

            return zl.outline.c_point(0);
        }

        if !self.valid {
            return Vector2I::default();
        }

        let parent = ctx.item(self.parent);

        match self.parent_type {
            KicadT::PcbPadT => parent.map_or_else(Vector2I::default, |p| p.position()),
            KicadT::PcbTraceT | KicadT::PcbArcT => {
                let track = parent.as_ref().and_then(|p| p.as_track());

                match track {
                    Some(t) if n == 0 => t.as_track().start(),
                    Some(t) => t.as_track().end(),
                    None => Vector2I::default(),
                }
            }
            KicadT::PcbViaT => parent
                .as_ref()
                .and_then(|p| p.as_track())
                .map_or_else(Vector2I::default, |t| t.as_track().start()),
            KicadT::PcbShapeT => usize::try_from(n)
                .ok()
                .and_then(|i| self.anchors.get(i))
                .map_or_else(Vector2I::default, CnAnchor::pos),
            // UNIMPLEMENTED_FOR( m_parent->GetClass() )
            _ => Vector2I::default(),
        }
    }

    /// Port of `Net()`: the parent's net code, -1 for an invalid item.
    pub fn net(&self) -> i32 {
        if !self.valid { -1 } else { self.net }
    }

    /// Records a net change of the parent (`m_parent->SetNetCode( .. )` in the net
    /// propagation); the caller applies the change to the board.
    pub fn set_net(&mut self, net: i32) {
        self.net = net;
    }

    /// `ListIndex()`: position of this item in its owning list.
    pub fn list_index(&self) -> i32 {
        self.list_index
    }

    /// The `CN_ZONE_LAYER` part (`dynamic_cast<CN_ZONE_LAYER*>`).
    pub fn zone_layer(&self) -> Option<&CnZoneLayer> {
        self.zone_layer.as_deref()
    }

    /// Mutable `CN_ZONE_LAYER` part.
    pub fn zone_layer_mut(&mut self) -> Option<&mut CnZoneLayer> {
        self.zone_layer.as_deref_mut()
    }

    /// Port of `CN_ZONE_LAYER::HasSingleConnection()`: exactly one valid connected item.
    /// `false` for an item that is not a zone layer.
    pub fn has_single_connection(&self, list: &CnList) -> bool {
        if self.zone_layer.is_none() {
            return false;
        }

        let mut count = 0;

        for &c in &self.connected {
            if list.item(c).valid() {
                count += 1;
            }

            if count > 1 {
                break;
            }
        }

        count == 1
    }
}

/// `GetNetCode()` of a connected item (0 for other kinds, which are never connectivity
/// parents).
fn net_of(item: &ItemRef<'_>) -> i32 {
    item.net_code().map_or(0, |n| n.0)
}

/// Port of `PCB_SHAPE::GetConnectionPoints()` for a board-level shape (stored coordinates are
/// board coordinates; the getters it calls are not overridden by `PCB_SHAPE`).
///
/// The model crate has no port of this member yet (change request: move it to
/// `kicad_pcb::pcb_shape` as `PcbShape::connection_points`).
pub fn pcb_shape_connection_points(shape: &PcbShape) -> Vec<Vector2I> {
    let s = &shape.shape;
    let mut ret = Vec::new();

    // For filled shapes, we may as well use a centroid
    if s.is_solid_fill() {
        ret.push(s.center());
        return ret;
    }

    match s.shape() {
        ShapeT::Circle => {
            let circle = Circle::new(s.center(), s.radius());

            for pt in get_circle_key_points(&circle, false) {
                ret.push(pt.point);
            }
        }
        ShapeT::Arc => {
            ret.push(s.arc_mid());
            ret.push(s.start());
            ret.push(s.end());
        }
        ShapeT::Segment | ShapeT::Bezier => {
            ret.push(s.start());
            ret.push(s.end());
        }
        ShapeT::Poly => {
            for pt in s.poly_shape().c_iterate() {
                ret.push(pt);
            }
        }
        ShapeT::Rectangle => {
            for pt in s.rect_corners() {
                ret.push(pt);
            }
        }
        ShapeT::Ellipse => {
            let e = s.ellipse();
            let phi = e.rotation.as_radians();
            let cos_phi = cmath::cos(phi);
            let sin_phi = cmath::sin(phi);
            let a = e.major_radius;
            let b = e.minor_radius;
            let c = e.center;

            ret.push(c + Vector2I::new(ki_round(a as f64 * cos_phi), ki_round(a as f64 * sin_phi)));
            ret.push(
                c + Vector2I::new(
                    ki_round(-(a as f64) * cos_phi),
                    ki_round(-(a as f64) * sin_phi),
                ),
            );
            ret.push(
                c + Vector2I::new(
                    ki_round(-(b as f64) * sin_phi),
                    ki_round(b as f64 * cos_phi),
                ),
            );
            ret.push(
                c + Vector2I::new(
                    ki_round(b as f64 * sin_phi),
                    ki_round(-(b as f64) * cos_phi),
                ),
            );
        }
        ShapeT::EllipseArc => {
            let e = s.ellipse();
            let a = e.major_radius as f64;
            let b = e.minor_radius as f64;
            let phi = e.rotation.as_radians();
            let cos_phi = cmath::cos(phi);
            let sin_phi = cmath::sin(phi);
            let c = e.center;

            let eval = |theta: f64| -> Vector2I {
                let lx = a * cmath::cos(theta);
                let ly = b * cmath::sin(theta);
                c + Vector2I::new(
                    ki_round(lx * cos_phi - ly * sin_phi),
                    ki_round(lx * sin_phi + ly * cos_phi),
                )
            };

            let theta_start = e.start_angle.as_radians();
            let mut theta_end = e.end_angle.as_radians();

            if theta_end < theta_start {
                theta_end += 2.0 * std::f64::consts::PI;
            }

            ret.push(eval(theta_start));
            ret.push(eval(theta_end));
            ret.push(eval(0.5 * (theta_start + theta_end)));
        }
        // UNIMPLEMENTED_FOR( SHAPE_T_asString() )
        ShapeT::Undefined => {}
    }

    ret
}

// =================================================================================================
// CN_LIST
// =================================================================================================

/// Port of `CN_LIST`: the connectivity items in list order (the iteration order of everything
/// downstream) plus their r-tree.
#[derive(Default)]
pub struct CnList {
    /// Item arena, indexed by `CnItemId`; slots are never reused.
    arena: Vec<CnItem>,
    /// `m_items`: the list, in order.
    items: Vec<CnItemId>,
    /// `m_dirty`
    dirty: bool,
    /// `m_hasInvalid`
    has_invalid: bool,
    /// `m_bulkAdd`: see [`CnList::start_bulk_add`].
    bulk_add: bool,
    /// `m_index`
    index: CnRtree<CnItemId>,
}

impl CnList {
    /// `CN_LIST()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `Clear()`: deletes every item and empties the index.
    pub fn clear(&mut self) {
        self.arena.clear();
        self.items.clear();
        self.index.remove_all();
        self.bulk_add = false;
    }

    /// Port of `StartBulkAdd()`: defer spatial indexing until [`CnList::finish_bulk_add`],
    /// which packs the index in one pass. Only legal on an empty list, because the bulk load
    /// replaces the whole index (prefer [`CnList::bulk_add`]).
    ///
    /// Returns `true` if the list entered bulk mode, `false` to keep indexing as usual.
    pub fn start_bulk_add(&mut self) -> bool {
        if !self.items.is_empty() || self.has_invalid {
            return false;
        }

        self.bulk_add = true;
        true
    }

    /// Port of `FinishBulkAdd()`: index everything queued since `start_bulk_add()`. A no-op
    /// outside bulk mode.
    pub fn finish_bulk_add(&mut self) {
        if !self.bulk_add {
            return;
        }

        self.bulk_add = false;

        let mut entries = Vec::with_capacity(self.items.len());

        for &id in &self.items {
            // BBox() clears the dirty flag that Add() deliberately leaves set; the port's
            // bbox() has no side effect.
            let item = &self.arena[id.index()];
            let bbox = item.bbox();

            entries.push(CnBulkEntry {
                min: [item.start_layer(), bbox.x(), bbox.y()],
                max: [item.end_layer(), bbox.right(), bbox.bottom()],
                data: id,
            });
        }

        self.index.bulk_load(entries);
    }

    /// Port of `BULK_ADD_SCOPE`: holds bulk mode for the duration of `f`.
    pub fn bulk_add<R>(&mut self, f: impl FnOnce(&mut CnList) -> R) -> R {
        self.start_bulk_add();
        let r = f(self);
        self.finish_bulk_add();
        r
    }

    /// The list in order (`begin()` .. `end()`).
    pub fn iter(&self) -> impl Iterator<Item = &CnItem> + '_ {
        self.items.iter().map(move |&id| &self.arena[id.index()])
    }

    /// The handles of the list, in order.
    pub fn ids(&self) -> &[CnItemId] {
        &self.items
    }

    /// `operator[]( aIndex )`: the item at list position `index`.
    pub fn at(&self, index: usize) -> CnItemId {
        self.items[index]
    }

    /// The item behind a handle (C++ dereferences the pointer). Items removed by
    /// [`CnList::remove_invalid_items`] stay reachable (invalid) until [`CnList::clear`].
    pub fn item(&self, id: CnItemId) -> &CnItem {
        &self.arena[id.index()]
    }

    /// Mutable access to an item.
    pub fn item_mut(&mut self, id: CnItemId) -> &mut CnItem {
        &mut self.arena[id.index()]
    }

    /// Two items at once (`a != b`), for `a->Connect( b ); b->Connect( a )`.
    pub fn connect_pair(&mut self, a: CnItemId, b: CnItemId) {
        self.arena[a.index()].connect(b);
        self.arena[b.index()].connect(a);
    }

    /// Port of `FindNearby( aItem, aFunc )`: visits every indexed item whose box intersects
    /// the item's box within its copper ordinal range.
    pub fn find_nearby<F>(&self, item: CnItemId, func: F)
    where
        F: FnMut(&CnItemId) -> bool,
    {
        let it = &self.arena[item.index()];
        self.index
            .query(it.bbox(), it.start_layer(), it.end_layer(), func);
    }

    /// `SetHasInvalid()`.
    pub fn set_has_invalid(&mut self, invalid: bool) {
        self.has_invalid = invalid;
    }

    /// `SetDirty()`.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// `IsDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Port of `RemoveInvalidItems( aGarbage )`: compacts the list, restamps the list indices
    /// and removes the invalid items from the index; returns them (the garbage). The items
    /// stay in the arena, invalid.
    pub fn remove_invalid_items(&mut self) -> Vec<CnItemId> {
        let mut garbage = Vec::new();

        if !self.has_invalid {
            return garbage;
        }

        let arena = &self.arena;
        self.items.retain(|&id| {
            if !arena[id.index()].valid() {
                garbage.push(id);
                return false;
            }

            true
        });

        // Compaction shifted everything past the first hole, so restamp before anything reads
        for (ii, &id) in self.items.iter().enumerate() {
            self.arena[id.index()].list_index = ii as i32;
        }

        for &id in &garbage {
            let item = &self.arena[id.index()];
            self.index
                .remove(item.bbox(), item.start_layer(), item.end_layer(), &id);
        }

        self.has_invalid = false;
        garbage
    }

    /// Port of `ClearDirtyFlags()`.
    pub fn clear_dirty_flags(&mut self) {
        for &id in &self.items {
            self.arena[id.index()].set_dirty(false);
        }

        self.set_dirty(false);
    }

    /// `Size()`.
    pub fn size(&self) -> i32 {
        self.items.len() as i32
    }

    /// Port of `appendItem()`, the sole place list numbering is assigned, merged with the
    /// arena insertion that stands for the C++ `new`: gives the item its handle, binds its
    /// anchors to it and appends it.
    fn append_item(&mut self, mut item: CnItem) -> CnItemId {
        let id = CnItemId(self.arena.len() as u32);
        item.id = id;
        item.list_index = self.items.len() as i32;

        for anchor in &mut item.anchors {
            anchor.item = Some(id);
        }

        self.arena.push(item);
        self.items.push(id);
        id
    }

    /// Port of `addItemtoTree()`.
    fn add_item_to_tree(&mut self, id: CnItemId) {
        if !self.bulk_add {
            let item = &self.arena[id.index()];
            self.index
                .insert(item.bbox(), item.start_layer(), item.end_layer(), id);
        }
    }

    /// The common tail of every `Add()`: index, re-mark dirty (C++: after tree insertion,
    /// since `BBox()` clears the dirty flag), mark the list dirty.
    fn finish_add(&mut self, id: CnItemId) -> CnItemId {
        self.add_item_to_tree(id);
        self.arena[id.index()].set_dirty(true);
        self.set_dirty(true);
        id
    }

    /// Port of `CN_LIST::Add( PAD* )`: `None` for a pad not on a copper layer (or a stale
    /// handle).
    ///
    /// Anchors: the distinct `ShapePos( layer )` of the padstack's unique layers, sorted (the
    /// C++ `std::set<VECTOR2I>`). Layers: F_Cu .. B_Cu; for SMD, NPTH, CONN and NON_COMPONENT
    /// pads the first copper layer of the layer set.
    pub fn add_pad(&mut self, ctx: &BoardCtx<'_>, pad_id: PadId) -> Option<CnItemId> {
        let pad_ref = ctx.item(pad_id.into())?;
        let pad = pad_ref.as_pad()?;

        if !pad.is_on_copper_layer(ctx) {
            return None;
        }

        let mut item = CnItem::new(
            pad_id.into(),
            KicadT::PcbPadT,
            net_of(&pad_ref),
            pad_ref.bounding_box(),
            false,
        );

        // std::set<VECTOR2I>: lexicographic (x, y) order, duplicates merged.
        let mut unique_anchors: BTreeSet<(i32, i32)> = BTreeSet::new();
        pad.lib_padstack().for_each_unique_layer(|layer| {
            let p = pad.shape_pos(ctx, layer);
            unique_anchors.insert((p.x, p.y));
        });

        for (x, y) in unique_anchors {
            item.add_anchor(Vector2I::new(x, y));
        }

        item.set_layers(PcbLayerId::FCu.0, PcbLayerId::BCu.0);

        match pad.attribute() {
            PadAttrib::Smd | PadAttrib::Npth | PadAttrib::Conn | PadAttrib::NonComponent => {
                let lmsk = pad.layer_set(ctx).cu_stack();

                if let Some(front) = lmsk.0.first() {
                    item.set_layer(front.0);
                }
            }
            PadAttrib::Pth => {}
        }

        let id = self.append_item(item);
        Some(self.finish_add(id))
    }

    /// Port of `CN_LIST::Add( PCB_TRACK* )`, `Add( PCB_ARC* )` and `Add( PCB_VIA* )` (the
    /// overload is chosen by the track's kind): tracks and arcs get their start and end as
    /// anchors and their layer; vias their start, the `TopLayer() .. BottomLayer()` span and
    /// `CanChangeNet() = !GetIsFree()`. `None` for a stale handle.
    pub fn add_track(&mut self, ctx: &BoardCtx<'_>, track_id: TrackId) -> Option<CnItemId> {
        let track_ref = ctx.item(track_id.into())?;
        let track = track_ref.as_track()?;
        let base = track.as_track();
        let parent_type = track.type_();

        let item = if let Some(via) = track.as_via() {
            let mut item = CnItem::new(
                track_id.into(),
                parent_type,
                net_of(&track_ref),
                track_ref.bounding_box(),
                !via.is_free(),
            );
            item.add_anchor(base.start());
            item.set_layers(via.top_layer().0, via.bottom_layer().0);
            item
        } else {
            let mut item = CnItem::new(
                track_id.into(),
                parent_type,
                net_of(&track_ref),
                track_ref.bounding_box(),
                true,
            );
            item.add_anchor(base.start());
            item.add_anchor(base.end());
            item.set_layer(track_ref.layer().0);
            item
        };

        // (C++ appends before adding the anchors; the result is the same.)
        let id = self.append_item(item);
        Some(self.finish_add(id))
    }

    /// Port of `CN_LIST::Add( ZONE*, aLayer )`: one zone layer item per outline of the zone's
    /// fill on `layer`, with its indexes built and every outline vertex as an anchor (unlike
    /// the items `CN_CONNECTIVITY_ALGO::Build` creates, which have no anchors,
    /// connectivity-zones.md R-5). Empty for a stale handle or a layer without fill.
    pub fn add_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        zone_id: ZoneId,
        layer: PcbLayerId,
    ) -> Vec<CnItemId> {
        let mut rv = Vec::new();

        let Some(zone) = ctx.item(zone_id.into()).and_then(|z| z.as_zone()) else {
            return rv;
        };

        // GetFilledPolysList( aLayer ) is std::map::at in C++ (throws for a missing layer).
        let Some(polys) = zone.fill().filled_polys_list.get(&layer) else {
            return rv;
        };

        for j in 0..polys.outline_count() {
            let Some(mut zitem) = CnItem::new_zone_layer(ctx, zone_id, layer, j) else {
                continue;
            };

            if let Some(zl) = zitem.zone_layer_mut() {
                zl.build_indexes(ctx);
            }

            for &pt in polys.c_outline(j).c_points() {
                zitem.add_anchor(pt);
            }

            rv.push(self.add_zone_layer(zitem));
        }

        rv
    }

    /// Port of `CN_LIST::Add( CN_ZONE_LAYER* )`.
    pub fn add_zone_layer(&mut self, zitem: CnItem) -> CnItemId {
        debug_assert!(zitem.zone_layer.is_some());
        let id = self.append_item(zitem);
        self.finish_add(id)
    }

    /// Port of `CN_LIST::Add( PCB_SHAPE* )`: the shape's connection points as anchors, its
    /// layer. `None` for a stale handle or an item that is not a `PCB_SHAPE`.
    pub fn add_shape(&mut self, ctx: &BoardCtx<'_>, shape_id: ItemId) -> Option<CnItemId> {
        let shape_ref = ctx.item(shape_id)?;

        if shape_ref.type_() != KicadT::PcbShapeT {
            return None;
        }

        let shape = shape_ref.as_shape()?;
        let mut item = CnItem::new(
            shape_id,
            KicadT::PcbShapeT,
            net_of(&shape_ref),
            shape_ref.bounding_box(),
            true,
        );

        for point in pcb_shape_connection_points(shape) {
            item.add_anchor(point);
        }

        item.set_layer(shape_ref.layer().0);

        let id = self.append_item(item);
        Some(self.finish_add(id))
    }
}

// =================================================================================================
// CN_CLUSTER
// =================================================================================================

/// Port of `CN_CLUSTER`: a connected group of items, with the net that originates it.
#[derive(Clone, Debug)]
pub struct CnCluster {
    /// `m_conflicting`
    conflicting: bool,
    /// `m_originNet`
    origin_net: i32,
    /// `m_originPad`
    origin_pad: Option<CnItemId>,
    /// `m_items`, in insertion order.
    items: Vec<CnItemId>,
    /// `m_netRanks` (lookups only).
    net_ranks: HashMap<i32, i32>,
}

impl Default for CnCluster {
    fn default() -> Self {
        Self::new()
    }
}

impl CnCluster {
    /// Port of `CN_CLUSTER()`.
    pub fn new() -> Self {
        CnCluster {
            conflicting: false,
            origin_net: -1,
            origin_pad: None,
            items: Vec::with_capacity(64),
            net_ranks: HashMap::new(),
        }
    }

    /// `HasValidNet()`.
    pub fn has_valid_net(&self) -> bool {
        self.origin_net > 0
    }

    /// `OriginNet()`.
    pub fn origin_net(&self) -> i32 {
        self.origin_net
    }

    /// `m_originPad`.
    pub fn origin_pad(&self) -> Option<CnItemId> {
        self.origin_pad
    }

    /// Port of `OriginNetName()`: the origin pad's net name, `<none>` without a valid origin
    /// pad.
    pub fn origin_net_name(&self, ctx: &BoardCtx<'_>, list: &CnList) -> String {
        match self.origin_pad {
            Some(pad) if list.item(pad).valid() => ctx
                .item(list.item(pad).parent())
                .map_or_else(String::new, |p| p.netname().to_string()),
            _ => "<none>".to_string(),
        }
    }

    /// Port of `Contains( const CN_ITEM* )`.
    pub fn contains(&self, item: CnItemId) -> bool {
        self.items.contains(&item)
    }

    /// Port of `Contains( const BOARD_CONNECTED_ITEM* )`: a valid member with that parent.
    pub fn contains_parent(&self, list: &CnList, parent: ItemId) -> bool {
        self.items.iter().any(|&i| {
            let item = list.item(i);
            item.valid() && item.parent() == parent
        })
    }

    /// `Size()`.
    pub fn size(&self) -> i32 {
        self.items.len() as i32
    }

    /// `IsOrphaned()`: no origin pad.
    pub fn is_orphaned(&self) -> bool {
        self.origin_pad.is_none()
    }

    /// `IsConflicting()`.
    pub fn is_conflicting(&self) -> bool {
        self.conflicting
    }

    /// The members in insertion order (`begin()` .. `end()`).
    pub fn items(&self) -> &[CnItemId] {
        &self.items
    }

    /// Port of `CN_CLUSTER::Add()`: appends the item and ranks its net. The first item with a
    /// net sets the origin net; every pad that is not a free pad adds one to its net's rank and
    /// becomes the origin pad when there is none yet or its rank is strictly higher than the
    /// origin net's; a pad of another net than the origin makes the cluster conflicting.
    pub fn add(&mut self, ctx: &BoardCtx<'_>, list: &CnList, item_id: CnItemId) {
        self.items.push(item_id);

        let item = list.item(item_id);
        let net_code = item.net();

        if net_code <= 0 {
            return;
        }

        if self.origin_net <= 0 {
            self.origin_net = net_code;
            self.net_ranks.insert(self.origin_net, 0);
        }

        if item.parent_type() == KicadT::PcbPadT && !is_free_pad(ctx, item.parent()) {
            let rank = {
                let r = self.net_ranks.entry(net_code).or_insert(0);
                *r += 1;
                *r
            };

            // m_netRanks[m_originNet] (operator[] inserts 0 for a missing key)
            let origin_rank = *self.net_ranks.entry(self.origin_net).or_insert(0);

            if self.origin_pad.is_none() || rank > origin_rank {
                self.origin_pad = Some(item_id);
                self.origin_net = net_code;
            }

            if self.origin_pad.is_some() && item.net() != self.origin_net {
                self.conflicting = true;
            }
        }
    }
}

/// `static_cast<PAD*>( item->Parent() )->IsFreePad()`.
fn is_free_pad(ctx: &BoardCtx<'_>, pad: ItemId) -> bool {
    ctx.item(pad)
        .and_then(|p| p.as_pad().map(|pad| pad.is_free_pad(ctx)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_core::lset::Lset;
    use kicad_pcb::io::{ParseOptions, parse_board};

    /// A small two-layer board: a track, a normal and a free via, and copper graphics; the
    /// footprint (an SMD pad on B.Cu of net A, a through-hole pad of net B and a paste-only
    /// pad) is added through the model API (the footprint parser is PCBIO-06).
    const BOARD: &str = r#"(kicad_pcb (version 20250901) (generator "pcbnew") (generator_version "9.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (13 "F.Paste" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "A") (net 2 "B")
  (segment (start 9 10) (end 20 10) (width 0.25) (layer "B.Cu") (net 1)
    (uuid "00000000-0000-0000-0000-000000000010"))
  (via (at 20 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1)
    (uuid "00000000-0000-0000-0000-000000000011"))
  (via (at 30 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (free yes) (net 0)
    (uuid "00000000-0000-0000-0000-000000000012"))
  (gr_line (start 0 20) (end 5 20) (stroke (width 0.2) (type solid)) (layer "F.Cu")
    (uuid "00000000-0000-0000-0000-000000000020"))
  (gr_circle (center 10 20) (end 12 20) (stroke (width 0.2) (type solid)) (fill no)
    (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000021"))
  (gr_rect (start 20 20) (end 24 22) (stroke (width 0.2) (type solid)) (fill yes)
    (layer "B.Cu") (uuid "00000000-0000-0000-0000-000000000022"))
)"#;

    fn board() -> kicad_pcb::Board {
        use kicad_pcb::pad::Pad;
        use kicad_pcb::{AddMode, NetCode, OwnedItem, ParentRef};

        let mut board = parse_board(BOARD, "", &ParseOptions::default())
            .unwrap_or_else(|e| panic!("{}", e.what()));
        let fp = board.create_footprint();
        let env = board.child_env(ParentRef::Item(fp.into()));

        let pads: [(PadAttrib, Lset, Vector2I, i32); 3] = [
            (
                PadAttrib::Smd,
                Lset::from_layers(&[PcbLayerId::BCu]),
                p(9.0, 10.0),
                1,
            ),
            (PadAttrib::Pth, Pad::pth_mask(), p(12.0, 10.0), 2),
            (
                PadAttrib::Smd,
                Lset::from_layers(&[PcbLayerId::FPaste]),
                p(15.0, 10.0),
                0,
            ),
        ];

        for (attr, layers, pos, net) in pads {
            let mut pad = Pad::new(&mut board.kiid_generator);
            pad.set_attribute(attr);
            pad.set_layer_set(&env, layers);
            pad.set_position(&env, pos);
            pad.base.net = NetCode(net);
            assert!(
                board
                    .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Append)
                    .is_ok()
            );
        }

        board.attach_footprint(fp, AddMode::Append);
        board
    }

    fn mm(v: f64) -> i32 {
        (v * 1_000_000.0) as i32
    }

    fn p(x: f64, y: f64) -> Vector2I {
        Vector2I::new(mm(x), mm(y))
    }

    /// All items in `CN_CONNECTIVITY_ALGO::Build` order (tracks, pads, copper drawings).
    fn build(ctx: &BoardCtx<'_>) -> (CnList, Vec<Option<CnItemId>>) {
        let board = ctx.board;
        let mut list = CnList::new();
        let mut ids = Vec::new();

        for (id, _) in board.tracks() {
            ids.push(list.add_track(ctx, id));
        }

        for (fp, _) in board.footprints() {
            for (pad, _) in board.footprint_pads(fp) {
                ids.push(list.add_pad(ctx, pad));
            }
        }

        for (id, _) in board.drawings() {
            ids.push(list.add_shape(ctx, id));
        }

        (list, ids)
    }

    #[test]
    fn anchors_layers_and_nets_per_item_kind() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let (list, ids) = build(&ctx);

        // track, via, free via, pad 1, pad 2, (pad 3 not on copper), line, circle, rect
        assert_eq!(list.size(), 8);
        assert!(ids[5].is_none(), "a pad without copper layers has no item");

        let track = list.item(ids[0].unwrap());
        assert_eq!(track.parent_type(), KicadT::PcbTraceT);
        assert_eq!(track.anchor_count(), 2);
        assert_eq!((track.start_layer(), track.end_layer()), (31, 31));
        assert_eq!(track.board_layer(), PcbLayerId::BCu);
        assert_eq!(track.anchor(&ctx, 0), p(9.0, 10.0));
        assert_eq!(track.anchor(&ctx, 1), p(20.0, 10.0));
        assert!(track.can_change_net() && track.dirty() && track.valid());
        assert_eq!(track.net(), 1);

        let via = list.item(ids[1].unwrap());
        assert_eq!((via.start_layer(), via.end_layer()), (0, 31));
        assert_eq!(via.anchors().len(), 1);
        assert_eq!(via.anchor_count(), 1);
        assert!(via.can_change_net());
        assert!(!list.item(ids[2].unwrap()).can_change_net(), "free via");

        let smd = list.item(ids[3].unwrap());
        assert_eq!((smd.start_layer(), smd.end_layer()), (31, 31));
        assert!(!smd.can_change_net());
        assert_eq!(smd.anchors()[0].pos(), p(9.0, 10.0));
        assert_eq!(smd.anchor(&ctx, 0), p(9.0, 10.0));

        let tht = list.item(ids[4].unwrap());
        assert_eq!((tht.start_layer(), tht.end_layer()), (0, 31));
        assert_eq!(tht.anchors().len(), 1, "one unique ShapePos");
        assert_eq!(tht.net(), 2);

        let line = list.item(ids[6].unwrap());
        let pos: Vec<_> = line.anchors().iter().map(CnAnchor::pos).collect();
        assert_eq!(pos, vec![p(0.0, 20.0), p(5.0, 20.0)]);
        assert_eq!(line.anchor_count(), 2);
        assert_eq!(line.net(), 0);

        let circle = list.item(ids[7].unwrap());
        assert_eq!(circle.anchor_count(), 4, "cardinal key points");

        let rect = list.item(ids[8].unwrap());
        let pos: Vec<_> = rect.anchors().iter().map(CnAnchor::pos).collect();
        assert_eq!(pos, vec![p(22.0, 21.0)], "solid fill: centroid");
        assert_eq!((rect.start_layer(), rect.end_layer()), (31, 31));

        // list order == handle order, anchors know their item
        for (n, it) in list.iter().enumerate() {
            assert_eq!(it.list_index(), n as i32);
            assert_eq!(it.id(), list.at(n));
            assert!(it.anchors().iter().all(|a| a.item() == Some(it.id())));
        }
    }

    #[test]
    fn ordinals_round_trip() {
        assert_eq!(CnItem::board_layer_from_ordinal(-3), PcbLayerId::FCu);
        assert_eq!(CnItem::board_layer_from_ordinal(0), PcbLayerId::FCu);
        assert_eq!(CnItem::board_layer_from_ordinal(1), PcbLayerId::In1Cu);
        assert_eq!(CnItem::board_layer_from_ordinal(30), PcbLayerId::In30Cu);
        assert_eq!(CnItem::board_layer_from_ordinal(31), PcbLayerId::BCu);
        assert_eq!(CnItem::board_layer_from_ordinal(i32::MAX), PcbLayerId::BCu);

        let board = board();
        let track: ItemId = board.tracks().next().unwrap().0.into();
        let mut it = CnItem::new(track, KicadT::PcbTraceT, 0, Box2I::default(), true);
        assert_eq!((it.start_layer(), it.end_layer()), (0, i32::MAX));
        it.set_layers(PcbLayerId::In2Cu.0, PcbLayerId::BCu.0);
        assert_eq!((it.start_layer(), it.end_layer()), (2, 31));
        // INT_MAX is B_Cu for ToLAYER_ID
        it.set_layer(i32::MAX);
        assert_eq!((it.start_layer(), it.end_layer()), (31, 31));
    }

    #[test]
    fn connect_keeps_sorted_unique() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let (mut list, _) = build(&ctx);
        let a = list.at(0);

        for n in [5, 2, 7, 2, 5, 1] {
            let b = list.at(n);
            list.item_mut(a).connect(b);
        }

        let got: Vec<_> = list.item(a).connected_items().iter().map(|i| i.0).collect();
        assert_eq!(got, vec![1, 2, 5, 7]);

        list.item_mut(list.at(2)).set_valid(false);
        let valid: Vec<bool> = (0..list.arena.len())
            .map(|i| list.arena[i].valid())
            .collect();
        list.item_mut(a).remove_invalid_refs(|c| valid[c.index()]);
        let got: Vec<_> = list.item(a).connected_items().iter().map(|i| i.0).collect();
        assert_eq!(got, vec![1, 5, 7]);
        assert_eq!(list.item(list.at(2)).net(), -1, "invalid item has net -1");
        assert_eq!(list.item(list.at(2)).anchor_count(), 0);
    }

    #[test]
    fn remove_invalid_items_compacts_and_unindexes() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let (mut list, ids) = build(&ctx);
        let via = ids[1].unwrap();

        let near = |list: &CnList, id| {
            let mut v = Vec::new();
            list.find_nearby(id, |&c| {
                v.push(c);
                true
            });
            v.sort();
            v
        };

        // the track (B.Cu, ends at the via) and the via (through) see each other
        let track = ids[0].unwrap();
        assert!(near(&list, track).contains(&via));
        // the F.Cu line does not see the B.Cu rect even where boxes would overlap in x/y
        assert!(!near(&list, ids[6].unwrap()).contains(&ids[8].unwrap()));

        assert!(list.remove_invalid_items().is_empty(), "nothing flagged");
        list.item_mut(via).set_valid(false);
        list.set_has_invalid(true);
        assert_eq!(list.remove_invalid_items(), vec![via]);
        assert_eq!(list.size(), 7);
        assert!(!list.ids().contains(&via));
        assert!(!near(&list, track).contains(&via));

        for (n, it) in list.iter().enumerate() {
            assert_eq!(it.list_index(), n as i32);
        }

        list.clear_dirty_flags();
        assert!(list.iter().all(|i| !i.dirty()) && !list.is_dirty());
    }

    #[test]
    fn bulk_add_indexes_at_finish() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let mut list = CnList::new();
        let ids = list.bulk_add(|list| {
            let ids: Vec<_> = board
                .tracks()
                .map(|(id, _)| list.add_track(&ctx, id).unwrap())
                .collect();
            assert!(list.index.is_empty(), "deferred while in bulk mode");
            ids
        });
        assert_eq!(list.index.len(), 3);
        assert!(list.iter().all(CnItem::dirty), "Add leaves items dirty");
        let mut seen = Vec::new();
        list.find_nearby(ids[0], |&c| {
            seen.push(c);
            true
        });
        seen.sort();
        assert_eq!(seen, vec![ids[0], ids[1]]);
        // bulk mode is refused on a non-empty list
        assert!(!list.start_bulk_add());
    }

    /// `CN_CLUSTER::Add` origin net ranking (connectivity_items.cpp L476-511).
    #[test]
    fn cluster_origin_net_ranking() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let (list, ids) = build(&ctx);
        let (track, line, smd, tht) = (
            ids[0].unwrap(),
            ids[6].unwrap(),
            ids[3].unwrap(),
            ids[4].unwrap(),
        );

        // net 0 items do not set an origin
        let mut c = CnCluster::new();
        c.add(&ctx, &list, line);
        assert_eq!(c.origin_net(), -1);
        assert!(!c.has_valid_net() && c.is_orphaned());
        assert_eq!(c.origin_net_name(&ctx, &list), "<none>");

        // the first item with a net sets the origin net, a pad makes it non-orphaned
        c.add(&ctx, &list, track);
        assert_eq!(c.origin_net(), 1);
        assert!(c.is_orphaned());
        c.add(&ctx, &list, tht); // net 2 pad: no origin pad yet -> origin
        assert_eq!(c.origin_net(), 2);
        assert_eq!(c.origin_pad(), Some(tht));
        assert!(!c.is_conflicting());
        // origin_net_name() with a pad needs BOARD_CONNECTED_ITEM::GetNetname (model stub).
        c.add(&ctx, &list, smd); // net 1 pad: rank 1, not > 1 -> stays, conflicting
        assert_eq!(c.origin_net(), 2);
        assert!(c.is_conflicting());
        assert_eq!(c.size(), 4);
        assert!(c.contains(smd) && c.contains_parent(&list, list.item(smd).parent()));

        // a second pad of net 1 (same pad again) outranks net 2
        c.add(&ctx, &list, smd);
        assert_eq!(c.origin_net(), 1);
        assert_eq!(c.origin_pad(), Some(smd));
    }

    #[test]
    fn dangling_and_counts_need_a_cluster() {
        let board = board();
        let ctx = BoardCtx::bare(&board);
        let (mut list, ids) = build(&ctx);
        let (track, via) = (ids[0].unwrap(), ids[1].unwrap());

        let a = list.item(track).anchors()[1].clone();
        assert!(a.is_dangling(&ctx, &list), "no cluster: dangling");
        assert_eq!(a.connected_items_count(&ctx, &list), 0);

        list.connect_pair(track, via);
        for t in [track, via] {
            for an in list.item_mut(t).anchors_mut() {
                an.set_cluster(Some(CnClusterId(0)));
            }
        }

        let start = list.item(track).anchors()[0].clone();
        let end = list.item(track).anchors()[1].clone();
        assert!(start.is_dangling(&ctx, &list), "the via is at the end only");
        assert!(!end.is_dangling(&ctx, &list));
        assert_eq!(end.connected_items_count(&ctx, &list), 1);
        assert_eq!(start.connected_items_count(&ctx, &list), 0);

        // a via needs two connections
        let v = list.item(via).anchors()[0].clone();
        assert!(v.is_dangling(&ctx, &list));
        assert_eq!(v.dist(&start), (mm(11.0)) as u32);
    }
}
