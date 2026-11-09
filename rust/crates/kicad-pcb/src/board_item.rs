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

//! Port of `pcbnew/board_item.cpp` and of the bodies in `include/board_item.h`
//! (`BOARD_ITEM`): the base-class bodies the `BoardItem` / `BoardItemMut` trait defaults
//! forward to (`board-model.md` 6.2), generic over the item. Package M2 ports the bodies; M0a
//! wrote the signatures and the header-inline ones.
//!
//! An override that calls its base class explicitly (C++ `BOARD_ITEM::FitsEnabledLayers( .. )`
//! inside `PCB_VIA::FitsEnabledLayers`) calls the function here
//! (`board_item::fits_enabled_layers( self, .. )`).

use std::cmp::Ordering;
use std::sync::Arc;

use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::layer_ids::{MAX_CU_LAYERS, PCB_LAYER_ID_COUNT, PCBNEW_LAYER_ID_START};
use kicad_core::stroke_params::LineStyle;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Color4D, KicadT, Lset, PcbLayerId, StrokeParams};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::{EdaAngle, FlipDirection, Seg, ShapeNull, ShapePolySet, Vector2I};

use crate::board::{Board, LayerT};
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::footprint::Footprint;
use crate::ids::{ItemId, ItemKind};
use crate::item::{ParentRef, RecurseMode, SharedShape};
use crate::item_ref::{AnyItem, ItemRef};
use crate::traits::{BoardItem, BoardItemMut, Flashing};

/// Port of `BOARD_ITEM::GetLayerSet` (header inline): `LSET( { m_layer } )`, empty for
/// `UNDEFINED_LAYER`.
pub fn layer_set<B: BoardItem + ?Sized>(item: &B, _ctx: &BoardCtx<'_>) -> Lset {
    let layer = item.base().layer;

    if layer == PcbLayerId::UndefinedLayer {
        Lset::new()
    } else {
        Lset::from_layers(&[layer])
    }
}

/// Port of `BOARD_ITEM::IsOnLayer` (header inline): `m_layer == aLayer`.
pub fn is_on_layer<B: BoardItem + ?Sized>(
    item: &B,
    _ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
) -> bool {
    item.base().layer == layer
}

/// Port of `BOARD_ITEM::FitsEnabledLayers`:
/// `IsLayerAgnostic() || ( GetLayerSet() & aEnabledLayers ).any()`.
pub fn fits_enabled_layers<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    enabled: Lset,
    _copper_layer_count: i32,
) -> bool {
    if item.is_layer_agnostic() {
        return true;
    }

    let mut layers = item.layer_set(ctx);
    layers &= &enabled;
    layers.any()
}

/// Port of `BOARD_ITEM::SetLayer` (header inline): `m_layer = aLayer`.
pub fn set_layer<B: BoardItemMut + ?Sized>(item: &mut B, _env: &MutEnv, layer: PcbLayerId) {
    item.base_mut().layer = layer;
}

/// Port of `BOARD_ITEM::SetLayerSet` (header inline): a single-layer set becomes `SetLayer`
/// (the virtual one); anything else is `UNIMPLEMENTED_FOR` in C++ (ignored).
pub fn set_layer_set<B: BoardItemMut + ?Sized>(item: &mut B, env: &MutEnv, layers: Lset) {
    if layers.count() == 1 {
        if let Some(layer) = layers.seq().first().copied() {
            item.set_layer(env, layer);
        }
    }
}


/// `GetBoard() != nullptr` for an item: true when its parent chain reaches the board
/// (`ParentRef::Board`, or an item parent that does). An item with `ParentRef::None` (a value
/// being built, a dummy without parent) is the C++ null-board case.
pub fn has_board<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    match item.base().parent {
        ParentRef::None => false,
        ParentRef::Board => true,
        ParentRef::Item(parent) => ctx.item(parent).is_some_and(|p| p.has_board()),
    }
}

/// Port of `BOARD_ITEM::GetParentFootprint` (`findParent( PCB_FOOTPRINT_T )`: walks the whole
/// parent chain, e.g. through a table for a cell).
pub fn parent_footprint<'a, B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'a>,
) -> Option<&'a Footprint> {
    let board: &'a Board = ctx.board;
    let mut parent = item.base().parent.item();

    while let Some(id) = parent {
        let any = board.get(id)?;

        if let AnyItem::Footprint(fp) = any {
            return Some(fp);
        }

        parent = any.as_dyn().base().parent.item();
    }

    None
}

/// Port of `BOARD_ITEM::IsGroupableType`.
pub fn is_groupable_type<B: BoardItem + ?Sized>(item: &B) -> bool {
    matches!(
        item.type_(),
        KicadT::PcbFootprintT
            | KicadT::PcbPadT
            | KicadT::PcbShapeT
            | KicadT::PcbReferenceImageT
            | KicadT::PcbFieldT
            | KicadT::PcbTextT
            | KicadT::PcbTextboxT
            | KicadT::PcbTableT
            | KicadT::PcbDrillChartT
            | KicadT::PcbStackupTableT
            | KicadT::PcbDrillMapT
            | KicadT::PcbGroupT
            | KicadT::PcbGeneratorT
            | KicadT::PcbTraceT
            | KicadT::PcbViaT
            | KicadT::PcbArcT
            | KicadT::PcbDimensionT
            | KicadT::PcbDimAlignedT
            | KicadT::PcbDimLeaderT
            | KicadT::PcbDimCenterT
            | KicadT::PcbDimRadialT
            | KicadT::PcbDimOrthogonalT
            | KicadT::PcbZoneT
            | KicadT::PcbBarcodeT
            | KicadT::PcbPointT
            | KicadT::PcbGridItemT
    )
}

/// Port of `BOARD_ITEM::GetMaxError`: the board's `m_MaxError`, `ARC_HIGH_DEF` without a
/// board.
pub fn max_error<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> i32 {
    if has_board(item, ctx) {
        ctx.max_error()
    } else {
        ARC_HIGH_DEF
    }
}

/// Port of `BOARD_ITEM::GetFlippedLayer`: `board->FlipLayer( aLayer )`, the standard
/// `::FlipLayer` without a board.
pub fn flipped_layer<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
) -> PcbLayerId {
    if has_board(item, ctx) {
        ctx.flip_layer(layer)
    } else {
        kicad_core::layer_ids::flip_layer(layer, 0)
    }
}

/// Port of `BOARD_ITEM::BoardLayerCount`: `board->GetLayerSet().count()`, 64 without a board.
pub fn board_layer_count<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> i32 {
    if has_board(item, ctx) {
        ctx.board.enabled_layers().count() as i32
    } else {
        64
    }
}

/// Port of `BOARD_ITEM::BoardCopperLayerCount`: 32 without a board.
pub fn board_copper_layer_count<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> i32 {
    if has_board(item, ctx) {
        ctx.copper_layer_count()
    } else {
        32
    }
}

/// Port of `BOARD_ITEM::BoardLayerSet`: the enabled layers, `LSET::AllLayersMask()` without a
/// board.
pub fn board_layer_set<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> Lset {
    if has_board(item, ctx) {
        ctx.board.enabled_layers()
    } else {
        Lset::all_layers_mask().clone()
    }
}

/// Port of `BOARD_ITEM::GetLayerName()`: the name of `m_layer` (the stored layer, not the
/// virtual `GetLayer()`).
pub fn layer_name<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    layer_name_of(item, ctx, item.base().layer)
}

/// Port of `BOARD_ITEM::GetLayerName( aLayer )`: the board's (user) name, the standard name
/// without a board.
pub fn layer_name_of<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
) -> String {
    if has_board(item, ctx) {
        return ctx.board.layer_name(layer);
    }

    // If no parent, return standard name
    Board::standard_layer_name(layer)
}

/// Port of `BOARD_ITEM::GetFPRelativePosition`: the position in the parent footprint's frame
/// (`GetTransform().InverseApply( GetPosition() )`), the board position without one.
pub fn fp_relative_position<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> Vector2I {
    if let Some(parent_fp) = parent_footprint(item, ctx) {
        return parent_fp.transform().inverse_apply(item.position(ctx));
    }

    item.position(ctx)
}

/// Port of `BOARD_ITEM::SetFPRelativePosition`: `SetPosition( transform.Apply( aPos ) )` with
/// the parent footprint's transform from `env`, else `SetPosition( aPos )`.
pub fn set_fp_relative_position<B: BoardItemMut + ?Sized>(item: &mut B, env: &MutEnv, pos: Vector2I) {
    if let Some(frame) = env.parent {
        item.set_position(env, frame.transform.apply(pos));
    } else {
        item.set_position(env, pos);
    }
}

/// Port of `BOARD_ITEM::IsOnCopperLayer` (header inline): `IsCopperLayer( GetLayer() )` for a
/// single-layer type, else any copper layer in `GetLayerSet()`.
pub fn is_on_copper_layer<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    if item.type_().is_single_layer_type() {
        kicad_core::layer_ids::is_copper_layer(item.layer(ctx))
    } else {
        item.layer_set(ctx)
            .seq()
            .0
            .iter()
            .any(|&layer| kicad_core::layer_ids::is_copper_layer(layer))
    }
}

/// Port of `BOARD_ITEM::IsSideSpecific`: on a side-specific layer, or (with a board) the
/// principal layer `m_layer` is of type `LT_FRONT` / `LT_BACK`.
pub fn is_side_specific<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    let mut layers = item.layer_set(ctx);
    layers &= Lset::side_specific_mask();

    if layers.any() {
        return true;
    }

    if has_board(item, ctx) {
        let principal_layer_type = ctx.board.layer_type(item.base().layer);

        if principal_layer_type == LayerT::Front || principal_layer_type == LayerT::Back {
            return true;
        }
    }

    false
}

/// Port of `BOARD_ITEM::GetEffectiveShape` (`UNIMPLEMENTED_FOR`). C++ returns an empty
/// `shared_ptr`; `SharedShape` cannot be null, so this returns a `SHAPE_NULL` (no geometry).
pub fn effective_shape<B: BoardItem + ?Sized>(
    _item: &B,
    _ctx: &BoardCtx<'_>,
    _layer: PcbLayerId,
    _flash: Flashing,
    _usage: DrcConstraintT,
) -> SharedShape {
    Arc::new(ShapeNull::new())
}

/// Port of `BOARD_ITEM::TransformShapeToPolygon` (`wxCHECK_MSG( false, ... )`: the base class
/// adds nothing to the buffer).
#[allow(clippy::too_many_arguments)]
pub fn transform_shape_to_polygon<B: BoardItem + ?Sized>(
    _item: &B,
    _ctx: &BoardCtx<'_>,
    _buffer: &mut ShapePolySet,
    _layer: PcbLayerId,
    _clearance: i32,
    _error: i32,
    _error_loc: ErrorLoc,
    _ignore_line_width: bool,
) {
}

/// Port of `BOARD_ITEM::TransformShapeToPolySet` (header inline): the virtual
/// `TransformShapeToPolygon` with `ignoreLineWidth = false` (the render settings argument is
/// not ported).
pub fn transform_shape_to_poly_set<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
) {
    item.transform_shape_to_polygon(ctx, buffer, layer, clearance, error, error_loc, false);
}

/// Port of `BOARD_ITEM::GetStroke` (`wxFAIL_MSG`): `STROKE_PARAMS( mmToIU( DEFAULT_LINE_WIDTH ) )`.
pub fn stroke<B: BoardItem + ?Sized>(_item: &B, _ctx: &BoardCtx<'_>) -> StrokeParams {
    StrokeParams::new(
        PCB_IU_SCALE.mm_to_iu(crate::pcb_shape::DEFAULT_LINE_WIDTH),
        LineStyle::Default,
        Color4D::UNSPECIFIED,
    )
}

/// Port of `BOARD_ITEM::IsLocked`: locked when the parent group is locked; never locked
/// without a board or in a footprint holder board; else `m_isLocked`.
pub fn is_locked<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    if let Some(group) = item.parent_group(ctx).and_then(|g| ctx.item(g)) {
        if group.is_locked() {
            return true;
        }
    }

    if !has_board(item, ctx) || ctx.board.board_use == crate::item::BoardUse::FpHolder {
        return false;
    }

    item.base().is_locked
}

/// Port of `EDA_ITEM::GetItemDescription` (`wxFAIL_MSG`): "Undefined item description for
/// <class>".
pub fn item_description<B: BoardItem + ?Sized>(
    item: &B,
    _ctx: &BoardCtx<'_>,
    _units: &UnitsProvider<'_>,
    _full: bool,
) -> String {
    format!("Undefined item description for {}", item.class_name())
}

/// Port of `EDA_ITEM::GetFriendlyName` (`GetTypeDesc()`).
pub fn friendly_name<B: BoardItem + ?Sized>(item: &B, _ctx: &BoardCtx<'_>) -> String {
    crate::eda_item::type_desc(item.type_()).to_string()
}

/// Port of `BOARD_ITEM::LayerMaskDescribe` (English texts; `_()` is not translated).
pub fn layer_mask_describe<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    let board = has_board(item, ctx);
    let mut layers = item.layer_set(ctx);
    let mut copper_layer_count = MAX_CU_LAYERS;

    if board {
        layers &= &ctx.board.enabled_layers();
        copper_layer_count = ctx.board.copper_layer_count();
    }

    let mut copper_layers = layers.clone();
    copper_layers &= Lset::all_cu_mask();
    let mut tech_layers = layers.clone();
    tech_layers &= Lset::all_tech_mask();

    // Try to be smart and useful.  Check all copper first.
    if copper_layers.count() as i32 == copper_layer_count {
        return "all copper layers".to_string();
    }

    for test_layers in [&copper_layers, &tech_layers, &layers] {
        for layer in PCBNEW_LAYER_ID_START.0..PCB_LAYER_ID_COUNT {
            let layer = PcbLayerId(layer);

            if test_layers.contains(layer) {
                let mut layer_info = if board {
                    ctx.board.layer_name(layer)
                } else {
                    kicad_core::layer_ids::layer_name(layer)
                };

                if test_layers.count() > 1 {
                    layer_info.push_str(" and others");
                }

                return layer_info;
            }
        }
    }

    // No copper, no technicals: no layer
    "no layers".to_string()
}

/// Port of `BOARD_ITEM::ptr_cmp::operator()` as an `Ordering`: `Type()`, then
/// `GetLayerSet().Seq()` compared as vectors (only when the sets differ), then `m_Uuid`, then
/// pointer order, which is `ItemId` order (`board-model.md` 3.1). A stale handle sorts by
/// `ItemId` only.
pub fn board_item_ptr_cmp(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(ra), Some(rb)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };

    if ra.type_() != rb.type_() {
        return (ra.type_() as i32).cmp(&(rb.type_() as i32));
    }

    let (set_a, set_b) = (ra.layer_set(), rb.layer_set());

    if set_a != set_b {
        let seq_a: Vec<i32> = set_a.seq().0.iter().map(|l| l.0).collect();
        let seq_b: Vec<i32> = set_b.seq().0.iter().map(|l| l.0).collect();
        return seq_a.cmp(&seq_b);
    }

    // UUIDs *should* always be unique (for valid boards anyway)
    if ra.uuid() != rb.uuid() {
        return ra.uuid().cmp(&rb.uuid());
    }

    // But just in case; ptrs are guaranteed to be different
    a.cmp(&b)
}

/// `MIRROR_AXIS( aCentre, aFlipDirection )` (`include/board_item.h`).
pub fn mirror_axis(centre: Vector2I, dir: FlipDirection) -> Seg {
    if dir == FlipDirection::LeftRight {
        return Seg::new(centre, centre + Vector2I::new(0, 1));
    }

    Seg::new(centre, centre + Vector2I::new(1, 0))
}

/// The polymorphic mutating virtuals that may cascade (footprint, group, generator, table:
/// `board-model.md` 8.2); leaf items go through `AnyItemMut::as_leaf_mut` inside `modify`.
/// A stale handle is ignored.
impl Board {
    /// `item->Move( aMoveVector )` on a stored item (footprints, groups, generators and
    /// tables cascade to their children / members).
    pub fn move_item(&mut self, id: ItemId, delta: Vector2I) {
        match id.kind() {
            ItemKind::Footprint => {
                if let Some(fp) = id.typed() {
                    self.footprint_move(fp, delta);
                }
            }
            // PCB_GROUP::Move
            ItemKind::Group => {
                for member in self.member_snapshot(id) {
                    self.move_item(member, delta);
                }
            }
            // PCB_GENERATOR::Move: m_origin += aMoveVector, then PCB_GROUP::Move
            ItemKind::Generator => {
                let Some(generator) = id.typed() else { return };

                if self
                    .modify_generator(generator, |g, _| g.origin += delta)
                    .is_some()
                {
                    for member in self.member_snapshot(id) {
                        self.move_item(member, delta);
                    }
                }
            }
            // PCB_TABLE::Move: every cell
            ItemKind::Table => {
                for cell in self.cell_snapshot(id) {
                    self.move_item(cell, delta);
                }
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.move_by(env, delta);
                    }
                });
            }
        }
    }

    /// `item->Rotate( aRotCentre, aAngle )` on a stored item.
    pub fn rotate_item(&mut self, id: ItemId, centre: Vector2I, angle: EdaAngle) {
        match id.kind() {
            ItemKind::Footprint => {
                if let Some(fp) = id.typed() {
                    self.footprint_rotate(fp, centre, angle);
                }
            }
            // PCB_GROUP::Rotate
            ItemKind::Group => {
                for member in self.member_snapshot(id) {
                    self.rotate_item(member, centre, angle);
                }
            }
            // PCB_GENERATOR::Rotate: RotatePoint( m_origin, aRotCentre, aAngle ), then
            // PCB_GROUP::Rotate
            ItemKind::Generator => {
                let Some(generator) = id.typed() else { return };

                if self
                    .modify_generator(generator, |g, _| {
                        kicad_geom::trigo::rotate_point_around(&mut g.origin, centre, angle)
                    })
                    .is_some()
                {
                    for member in self.member_snapshot(id) {
                        self.rotate_item(member, centre, angle);
                    }
                }
            }
            ItemKind::Table => {
                if self.contains(id) {
                    crate::port_pending!("PCB_TABLE::Rotate (rotate_item)", id, centre, angle)
                }
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.rotate(env, centre, angle);
                    }
                });
            }
        }
    }

    /// `item->Flip( aCentre, aFlipDirection )` on a stored item.
    pub fn flip_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection) {
        match id.kind() {
            ItemKind::Footprint => {
                if let Some(fp) = id.typed() {
                    self.footprint_flip(fp, centre, dir);
                }
            }
            // PCB_GROUP::Flip
            ItemKind::Group => {
                for member in self.member_snapshot(id) {
                    self.flip_item(member, centre, dir);
                }
            }
            // PCB_GENERATOR::Flip: baseMirror, SetLayer( GetFlippedLayer( GetLayer() ) ), then
            // PCB_GROUP::Flip
            ItemKind::Generator => {
                let Some(generator) = id.typed() else { return };
                let Some(flipped) = self.flipped_layer_of(id) else {
                    return;
                };

                self.modify_generator(generator, |g, _| {
                    g.origin = mirror_axis(centre, dir).reflect_point(g.origin);
                    g.base_mut().layer = flipped;
                });

                for member in self.member_snapshot(id) {
                    self.flip_item(member, centre, dir);
                }
            }
            ItemKind::Table => {
                if self.contains(id) {
                    crate::port_pending!("PCB_TABLE::Flip (flip_item)", id, centre, dir)
                }
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.flip(env, centre, dir);
                    }
                });
            }
        }
    }

    /// `item->Mirror( aCentre, aFlipDirection )` on a stored item. A footprint has no
    /// `Mirror` (the `BOARD_ITEM` base body is a message box: nothing happens).
    pub fn mirror_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection) {
        match id.kind() {
            ItemKind::Footprint => {}
            // PCB_GROUP::Mirror
            ItemKind::Group => {
                // Footprints have no mirror, only flip. If the group holds one, leave the whole
                // group alone rather than mirror the rest and tear it apart.
                if self.group_has_footprint(id) {
                    return;
                }

                for member in self.member_snapshot(id) {
                    self.mirror_item(member, centre, dir);
                }
            }
            // PCB_GENERATOR::Mirror: baseMirror, then PCB_GROUP::Mirror
            ItemKind::Generator => {
                let Some(generator) = id.typed() else { return };

                if self
                    .modify_generator(generator, |g, _| {
                        g.origin = mirror_axis(centre, dir).reflect_point(g.origin)
                    })
                    .is_none()
                {
                    return;
                }

                if self.group_has_footprint(id) {
                    return;
                }

                for member in self.member_snapshot(id) {
                    self.mirror_item(member, centre, dir);
                }
            }
            ItemKind::Table => {
                if self.contains(id) {
                    crate::port_pending!("PCB_TABLE::Mirror (mirror_item)", id, centre, dir)
                }
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.mirror(env, centre, dir);
                    }
                });
            }
        }
    }

    /// `item->SetLayer( aLayer )` on a stored item: `FOOTPRINT::SetLayer` (the side, children
    /// untouched), `PCB_GROUP::SetLayer` (nothing), `PCB_GENERATOR::SetLayer` (`m_layer`),
    /// `PCB_TABLE::SetLayer` (`m_layer`, then every cell), else the leaf's `SetLayer`.
    pub fn set_item_layer(&mut self, id: ItemId, layer: PcbLayerId) {
        match id.kind() {
            ItemKind::Footprint => {
                if let Some(fp) = id.typed() {
                    self.footprint_set_layer(fp, layer);
                }
            }
            ItemKind::Group => {}
            ItemKind::Generator => {
                if let Some(generator) = id.typed() {
                    self.modify_generator(generator, |g, _| g.base_mut().layer = layer);
                }
            }
            ItemKind::Table => {
                let Some(table) = id.typed() else { return };

                if self
                    .modify_table(table, |t, _| t.base_mut().layer = layer)
                    .is_some()
                {
                    for cell in self.cell_snapshot(id) {
                        self.set_item_layer(cell, layer);
                    }
                }
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.set_layer(env, layer);
                    }
                });
            }
        }
    }

    /// `item->SetLocked( aLocked )` on a stored item: `BOARD_ITEM::SetLocked` (`m_isLocked`)
    /// for every kind (`PCB_GROUP::SetLocked` deliberately does not touch its members).
    pub fn set_item_locked(&mut self, id: ItemId, locked: bool) {
        self.modify(id, |mut item, _| {
            item.as_dyn_mut().base_mut().is_locked = locked;
        });
    }

    /// Snapshot of a group's / generator's members (`m_items`, insertion order).
    fn member_snapshot(&self, group: ItemId) -> Vec<ItemId> {
        self.group_members(group).collect()
    }

    /// Snapshot of a table's cells (`m_cells` order).
    fn cell_snapshot(&self, table: ItemId) -> Vec<ItemId> {
        table
            .typed()
            .map(|t| self.table_cells(t).map(|(c, _)| c.into()).collect())
            .unwrap_or_default()
    }

    /// `GetFlippedLayer( GetLayer() )` of a stored item.
    fn flipped_layer_of(&self, id: ItemId) -> Option<PcbLayerId> {
        let ctx = BoardCtx::bare(self);
        let item = ctx.item(id)?;
        Some(item.flipped_layer(item.layer()))
    }

    /// `groupHasFootprint( aGroup )` (`pcb_group.cpp`): a footprint among the recursive
    /// children.
    fn group_has_footprint(&self, group: ItemId) -> bool {
        let ctx = BoardCtx::bare(self);
        let Some(item) = ctx.item(group) else {
            return false;
        };
        let mut found = false;
        item.as_dyn()
            .run_on_children(&ctx, RecurseMode::Recurse, &mut |child: ItemRef<'_>| {
                if child.type_() == KicadT::PcbFootprintT {
                    found = true;
                }
            });
        found
    }
}
