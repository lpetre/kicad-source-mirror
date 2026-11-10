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
use kicad_geom::{EdaAngle, FlipDirection, Seg, Shape, ShapeNull, ShapePolySet, Vector2I};

use crate::board::{Board, LayerT};
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::footprint::Footprint;
use crate::ids::{ItemId, ItemKind};
use crate::item::{ParentRef, RecurseMode, SharedShape};
use crate::item_ref::{AnyItem, ItemRef};
use crate::pcb_generator::{
    GENERATOR_TYPE_TUNING_PATTERN, GeneratorData, MEANDER_SIDE_LEFT, MEANDER_SIDE_RIGHT,
    TuningPatternData,
};
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
pub fn set_fp_relative_position<B: BoardItemMut + ?Sized>(
    item: &mut B,
    env: &MutEnv,
    pos: Vector2I,
) {
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

/// `this` as the `BOARD_ITEM*` a base-class body hands to another subsystem (rule resolver,
/// connectivity): the stored item when the value carries a live handle, otherwise a view of
/// the value itself (`id() == None`, like the address of an unregistered C++ object).
pub fn with_this<B: BoardItem + ?Sized, R>(
    item: &B,
    ctx: &BoardCtx<'_>,
    f: impl FnOnce(ItemRef<'_>) -> R,
) -> R {
    if let Some(this) = item.base().id().and_then(|id| ctx.item(id)) {
        return f(this);
    }

    let mut f = Some(f);
    let mut result = None;

    item.with_any_item(&mut |any| {
        if let Some(f) = f.take() {
            result = Some(f(ctx.view(any)));
        }
    });

    result.expect("AsAnyItem::with_any_item calls its closure")
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
            ItemKind::Generator => self.generator_move(id, delta),
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
            ItemKind::Generator => self.generator_rotate(id, centre, angle),
            // PCB_TABLE::Rotate (package M11, pcb_table.rs)
            ItemKind::Table => {
                if let Some(table) = id.typed() {
                    self.table_rotate(table, centre, angle);
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
            ItemKind::Generator => self.generator_flip(id, centre, dir),
            // PCB_TABLE::Flip (package M11, pcb_table.rs)
            ItemKind::Table => {
                if let Some(table) = id.typed() {
                    self.table_flip(table, centre, dir);
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
            ItemKind::Generator => self.generator_mirror(id, centre, dir),
            // PCB_TABLE::Mirror (package M11, pcb_table.rs)
            ItemKind::Table => {
                if let Some(table) = id.typed() {
                    self.table_mirror(table, centre, dir);
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
            ItemKind::Generator => self.generator_set_layer(id, layer),
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

    /// `item->SetPosition( aPos )` on a stored item: `FOOTPRINT::SetPosition`,
    /// `PCB_GROUP::SetPosition` / `PCB_TABLE::SetPosition` (`Move( aPos - GetPosition() )`,
    /// the virtual `Move`), `PCB_GENERATOR::SetPosition` (`m_origin = aPos`), else the leaf's
    /// `SetPosition`. A stale handle is ignored.
    pub fn set_item_position(&mut self, id: ItemId, pos: Vector2I) {
        match id.kind() {
            ItemKind::Footprint => {
                if let Some(fp) = id.typed() {
                    self.footprint_set_position(fp, pos);
                }
            }
            ItemKind::Group | ItemKind::Table => {
                let current = {
                    let ctx = BoardCtx::bare(self);
                    let Some(item) = ctx.item(id) else { return };
                    item.position()
                };

                self.move_item(id, pos - current);
            }
            ItemKind::Generator => {
                self.with_generator_data(id, |data| *data.origin_mut() = pos);
            }
            _ => {
                self.modify(id, |mut item, env| {
                    if let Some(leaf) = item.as_leaf_mut() {
                        leaf.set_position(env, pos);
                    }
                });
            }
        }
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

/// The generator arms of the polymorphic mutators: `PCB_GENERATOR` (`pcbnew/pcb_generator.cpp`)
/// and the overrides of its subclasses in `pcbnew/generators/*.h` / `*.cpp`
/// (`PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_GENERATOR_POLY` / `PCB_VIA_STITCH`). The typed
/// members live in the property map: each arm reads them with `subclass_data`, changes them and
/// writes them back (`set_subclass_data`), as the C++ members would then be saved
/// (package FIX-EDIT-1). The `IN_EDIT` flag of the tuning pattern is never set outside the
/// interactive generator tool, so its guarded bodies always run.
impl Board {
    /// Runs `f` on the typed members of a stored generator and stores them again.
    fn with_generator_data(&mut self, id: ItemId, f: impl FnOnce(&mut GeneratorData)) -> bool {
        let Some(generator) = id.typed() else {
            return false;
        };

        self.modify_generator(generator, |g, _| {
            let mut data = g.subclass_data();
            f(&mut data);
            g.set_subclass_data(&data);
        })
        .is_some()
    }

    /// `PCB_GENERATOR::MarkDirty()` on a stored generator.
    fn generator_mark_dirty(&mut self, id: ItemId) {
        if let Some(generator) = id.typed() {
            self.modify_generator(generator, |g, _| g.mark_dirty());
        }
    }

    /// The subclass data of a stored generator, `None` for a stale handle.
    fn generator_data(&self, id: ItemId) -> Option<GeneratorData> {
        id.typed()
            .and_then(|g| self.generator(g))
            .map(|g| g.subclass_data())
    }

    /// `item->Move( aMoveVector )` on a generator.
    fn generator_move(&mut self, id: ItemId, delta: Vector2I) {
        match self.generator_data(id) {
            None => {}
            // PCB_VIA_STITCH::Move: only the outline and the origin; the child vias are not
            // moved (the next Update() adds / removes them on the stitch grid).
            Some(GeneratorData::ViaStitch(_)) => {
                self.with_generator_data(id, |data| {
                    if let GeneratorData::ViaStitch(d) = data {
                        if let Some(outline) = &mut d.outline {
                            outline.move_(delta);
                        }

                        d.origin += delta;
                    }
                });
                self.generator_mark_dirty(id);
            }
            // PCB_TUNING_PATTERN::Move: m_origin and m_end, PCB_GROUP::Move, the base lines.
            Some(GeneratorData::TuningPattern(_)) => {
                self.with_generator_data(id, |data| {
                    if let GeneratorData::TuningPattern(d) = data {
                        d.origin += delta;
                        d.end += delta;

                        if let Some(base_line) = &mut d.base_line {
                            base_line.move_(delta);
                        }

                        if let Some(coupled) = &mut d.base_line_coupled {
                            coupled.move_(delta);
                        }
                    }
                });

                for member in self.member_snapshot(id) {
                    self.move_item(member, delta);
                }
            }
            // PCB_GENERATOR::Move (m_origin += aMoveVector, then PCB_GROUP::Move); the via
            // stack also moves its hops.
            Some(_) => {
                self.with_generator_data(id, |data| {
                    *data.origin_mut() += delta;

                    if let GeneratorData::ViaStack(d) = data
                        && let Some(hops) = &mut d.hops
                    {
                        hops.move_(delta);
                    }
                });

                for member in self.member_snapshot(id) {
                    self.move_item(member, delta);
                }
            }
        }
    }

    /// `item->Rotate( aRotCentre, aAngle )` on a generator: `PCB_GENERATOR::Rotate`
    /// (`RotatePoint( m_origin )`, then `PCB_GROUP::Rotate`) plus the subclass members
    /// (`PCB_GENERATOR_POLY`: the outline first; tuning pattern: `m_end` and the base lines;
    /// via stack: the hops).
    fn generator_rotate(&mut self, id: ItemId, centre: Vector2I, angle: EdaAngle) {
        let Some(kind) = self.generator_data(id) else {
            return;
        };

        self.with_generator_data(id, |data| {
            kicad_geom::trigo::rotate_point_around(data.origin_mut(), centre, angle);

            match data {
                GeneratorData::ViaStitch(d) => {
                    if let Some(outline) = &mut d.outline {
                        outline.rotate(angle, centre);
                    }
                }
                GeneratorData::TuningPattern(d) => {
                    kicad_geom::trigo::rotate_point_around(&mut d.end, centre, angle);

                    if let Some(base_line) = &mut d.base_line {
                        base_line.rotate(angle, centre);
                    }

                    if let Some(coupled) = &mut d.base_line_coupled {
                        coupled.rotate(angle, centre);
                    }
                }
                GeneratorData::ViaStack(d) => {
                    if let Some(hops) = &mut d.hops {
                        hops.rotate(angle, centre);
                    }
                }
                GeneratorData::Base(_) => {}
            }
        });

        for member in self.member_snapshot(id) {
            self.rotate_item(member, centre, angle);
        }

        if matches!(kind, GeneratorData::ViaStitch(_)) {
            self.generator_mark_dirty(id);
        }
    }

    /// The virtual `SetLayer( aLayer )` of a generator: `PCB_GENERATOR::SetLayer` (`m_layer`);
    /// `PCB_TUNING_PATTERN::SetLayer` also sets the layer of every member.
    fn generator_set_layer(&mut self, id: ItemId, layer: PcbLayerId) {
        let Some(generator) = id.typed() else { return };
        let Some(is_tuning) = self
            .modify_generator(generator, |g, _| {
                g.base_mut().layer = layer;
                g.generator_type() == GENERATOR_TYPE_TUNING_PATTERN
            })
        else {
            return;
        };

        if is_tuning {
            for member in self.member_snapshot(id) {
                self.set_item_layer(member, layer);
            }
        }
    }

    /// `PCB_TUNING_PATTERN::baseMirror( aAxis )`: `PCB_GENERATOR::baseMirror` (the origin),
    /// the base lines (the origin and the end then follow the primary one), the initial side.
    fn tuning_base_mirror(d: &mut TuningPatternData, axis: &Seg) {
        d.origin = axis.reflect_point(d.origin);

        if let Some(base_line) = &mut d.base_line {
            base_line.mirror_seg(axis);
            d.origin = base_line.c_point(0);
            d.end = base_line.c_last_point();
        }

        if let Some(coupled) = &mut d.base_line_coupled {
            coupled.mirror_seg(axis);
        }

        if d.settings.initial_side == MEANDER_SIDE_RIGHT {
            d.settings.initial_side = MEANDER_SIDE_LEFT;
        } else {
            d.settings.initial_side = MEANDER_SIDE_RIGHT;
        }
    }

    /// `item->Flip( aCentre, aFlipDirection )` on a generator.
    ///
    /// `PCB_GENERATOR::Flip`: `baseMirror` (the origin; not virtual), the virtual
    /// `SetLayer( GetFlippedLayer( GetLayer() ) )` (a tuning pattern sets the flipped layer on
    /// its members, which `PCB_GROUP::Flip` then flips once more), `PCB_GROUP::Flip`.
    /// `PCB_GENERATOR_POLY` mirrors its outline first; `PCB_TUNING_PATTERN` runs its own
    /// `baseMirror` afterwards; `PCB_VIA_STACK` mirrors the hops and flips its start and end
    /// layers.
    fn generator_flip(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection) {
        let Some(kind) = self.generator_data(id) else {
            return;
        };
        let axis = mirror_axis(centre, dir);

        if let GeneratorData::ViaStitch(_) = kind {
            self.with_generator_data(id, |data| {
                if let GeneratorData::ViaStitch(d) = data
                    && let Some(outline) = &mut d.outline
                {
                    outline.mirror(centre, dir);
                }
            });
        }

        // PCB_GENERATOR::Flip
        self.with_generator_data(id, |data| {
            let origin = data.origin_mut();
            *origin = axis.reflect_point(*origin);
        });

        let Some(flipped) = self.flipped_layer_of(id) else {
            return;
        };

        self.generator_set_layer(id, flipped);

        for member in self.member_snapshot(id) {
            self.flip_item(member, centre, dir);
        }

        match kind {
            GeneratorData::ViaStitch(_) => self.generator_mark_dirty(id),
            GeneratorData::TuningPattern(_) => {
                self.with_generator_data(id, |data| {
                    if let GeneratorData::TuningPattern(d) = data {
                        Self::tuning_base_mirror(d, &axis);
                    }
                });
            }
            GeneratorData::ViaStack(_) => {
                let (start, end) = {
                    let ctx = BoardCtx::bare(self);
                    let (Some(item), Some(GeneratorData::ViaStack(d))) =
                        (ctx.item(id), self.generator_data(id))
                    else {
                        return;
                    };
                    (
                        item.flipped_layer(d.start_layer),
                        item.flipped_layer(d.end_layer),
                    )
                };

                self.with_generator_data(id, |data| {
                    if let GeneratorData::ViaStack(d) = data {
                        if let Some(hops) = &mut d.hops {
                            hops.mirror(centre, dir);
                        }

                        d.start_layer = start;
                        d.end_layer = end;
                    }
                });
            }
            GeneratorData::Base(_) => {}
        }
    }

    /// `item->Mirror( aCentre, aFlipDirection )` on a generator.
    ///
    /// `PCB_GENERATOR::Mirror`: `baseMirror` (the origin), then `PCB_GROUP::Mirror` (nothing
    /// when a footprint is among the members). `PCB_GENERATOR_POLY` mirrors its outline
    /// first; `PCB_VIA_STACK` mirrors its hops about `MIRROR_AXIS` afterwards;
    /// `PCB_TUNING_PATTERN` goes through `Mirror( MIRROR_AXIS( .. ) )`, i.e. the `SEG`
    /// overloads (for the members an axis-parallel `SEG` reflects exactly like the
    /// centre / direction overload used here), followed by its own `baseMirror`.
    fn generator_mirror(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection) {
        let Some(kind) = self.generator_data(id) else {
            return;
        };
        let axis = mirror_axis(centre, dir);

        self.with_generator_data(id, |data| {
            if let GeneratorData::ViaStitch(d) = data
                && let Some(outline) = &mut d.outline
            {
                outline.mirror(centre, dir);
            }

            let origin = data.origin_mut();
            *origin = axis.reflect_point(*origin);
        });

        if !self.group_has_footprint(id) {
            for member in self.member_snapshot(id) {
                self.mirror_item(member, centre, dir);
            }
        }

        match kind {
            GeneratorData::ViaStitch(_) => self.generator_mark_dirty(id),
            GeneratorData::TuningPattern(_) => {
                self.with_generator_data(id, |data| {
                    if let GeneratorData::TuningPattern(d) = data {
                        Self::tuning_base_mirror(d, &axis);
                    }
                });
            }
            GeneratorData::ViaStack(_) => {
                self.with_generator_data(id, |data| {
                    if let GeneratorData::ViaStack(d) = data
                        && let Some(hops) = &mut d.hops
                    {
                        hops.mirror_seg(&axis);
                    }
                });
            }
            GeneratorData::Base(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! No C++ unit test exercises these base-class bodies on their own (`qa/tests/pcbnew/
    //! test_board_item.cpp` tests the overrides of the concrete classes, ported with their
    //! packages). Each test names the C++ body it checks; expectations follow from that body.

    use super::*;
    use crate::board::AddMode;
    use crate::item::BoardUse;
    use crate::pcb_group::PcbGroup;
    use crate::pcb_point::PcbPoint;
    use crate::pcb_target::PcbTarget;
    use crate::pcb_track::{PcbTrack, Track};
    use crate::test_support;
    use crate::traits::BoardConnectedItem;
    use kicad_core::Kiid;

    fn board_track(board: &mut Board, layer: PcbLayerId) -> ItemId {
        let mut t = PcbTrack::new(&mut board.kiid_generator);
        t.base.item.layer = layer;
        board
            .add_track(Track::Trace(t), AddMode::Append)
            .unwrap()
            .into()
    }

    /// `BOARD_ITEM::IsLocked`: own flag on a board item; parent group lock wins; no board and
    /// footprint holder boards are never locked.
    #[test]
    fn is_locked_rules() {
        let mut board = Board::new();
        let track = board_track(&mut board, PcbLayerId::FCu);
        assert!(!BoardCtx::bare(&board).item(track).unwrap().is_locked());

        board.set_item_locked(track, true);
        assert!(BoardCtx::bare(&board).item(track).unwrap().is_locked());

        board.board_use = BoardUse::FpHolder;
        assert!(!BoardCtx::bare(&board).item(track).unwrap().is_locked());
        board.board_use = BoardUse::Normal;

        // Locked parent group: the member is locked whatever its own flag.
        board.set_item_locked(track, false);
        let mut group = PcbGroup::default();
        group.base.is_locked = true;
        let group = board.insert_item(group, ParentRef::Board);
        board
            .tracks
            .get_mut(track.typed().unwrap())
            .unwrap()
            .base_mut()
            .group = Some(group.into());
        assert!(BoardCtx::bare(&board).item(track).unwrap().is_locked());

        // No board: a detached value is never locked by its own flag.
        let mut target = PcbTarget::default();
        target.base.is_locked = true;
        assert!(!is_locked(&target, &BoardCtx::bare(&board)));
    }

    /// `BOARD_ITEM::IsOnCopperLayer` for a single-layer type, `IsSideSpecific` without a board.
    #[test]
    fn copper_and_side_specific() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut target = PcbTarget::default();

        target.base.layer = PcbLayerId::FCu;
        assert!(is_on_copper_layer(&target, &ctx));
        target.base.layer = PcbLayerId::FSilkS;
        assert!(!is_on_copper_layer(&target, &ctx));
        assert!(is_side_specific(&target, &ctx));
        target.base.layer = PcbLayerId::DwgsUser;
        assert!(!is_side_specific(&target, &ctx));
    }

    /// `BOARD_ITEM::LayerMaskDescribe` without a board (`MAX_CU_LAYERS`, `LayerName`).
    #[test]
    fn layer_mask_describe_boardless() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut target = PcbTarget::default();

        target.base.layer = PcbLayerId::FFab;
        assert_eq!(layer_mask_describe(&target, &ctx), "F.Fab");
        target.base.layer = PcbLayerId::BCu;
        assert_eq!(layer_mask_describe(&target, &ctx), "B.Cu");
        target.base.layer = PcbLayerId::UndefinedLayer;
        assert_eq!(layer_mask_describe(&target, &ctx), "no layers");
    }

    /// `EDA_ITEM::GetFriendlyName`, `GetItemDescription`, `BOARD_ITEM::GetStroke` base bodies.
    #[test]
    fn base_texts_and_stroke() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let target = PcbTarget::default();

        assert_eq!(friendly_name(&target, &ctx), "Target");
        assert_eq!(crate::eda_item::type_desc(KicadT::PcbArcT), "Track");
        assert_eq!(crate::eda_item::type_desc(KicadT::PcbDimLeaderT), "Leader");
        assert_eq!(stroke(&target, &ctx).width(), 100_000);
        assert!(!is_groupable_type(&target));
        assert!(is_groupable_type(&PcbPoint::default()));
    }

    fn board_target(board: &mut Board, layer: PcbLayerId) -> ItemId {
        let mut t = PcbTarget::new(&mut board.kiid_generator);
        t.base.layer = layer;
        board.insert_item(t, ParentRef::Board).into()
    }

    /// `BOARD_ITEM::ptr_cmp`: type, then layer sequence, then UUID, then pointer (`ItemId`).
    #[test]
    fn ptr_cmp_order() {
        let mut board = Board::new();
        let front = board_target(&mut board, PcbLayerId::FCu);
        let back = board_target(&mut board, PcbLayerId::BCu);
        let point: ItemId = board
            .insert_item(PcbPoint::default(), ParentRef::Board)
            .into();
        let ctx = BoardCtx::bare(&board);

        // PCB_TARGET_T < PCB_POINT_T
        assert_eq!(board_item_ptr_cmp(&ctx, front, point), Ordering::Less);
        assert_eq!(board_item_ptr_cmp(&ctx, point, back), Ordering::Greater);
        // F_Cu (0) < B_Cu (2)
        assert_eq!(board_item_ptr_cmp(&ctx, back, front), Ordering::Greater);
        assert_eq!(board_item_ptr_cmp(&ctx, front, front), Ordering::Equal);

        // Same type and layers: UUID order, then handle order for duplicate UUIDs.
        let mut board = Board::new();
        let a = board_target(&mut board, PcbLayerId::FCu);
        let b = board_target(&mut board, PcbLayerId::FCu);
        let (ua, ub) = {
            let ctx = BoardCtx::bare(&board);
            (ctx.item(a).unwrap().uuid(), ctx.item(b).unwrap().uuid())
        };
        assert_ne!(ua, ub);
        let ctx = BoardCtx::bare(&board);
        assert_eq!(board_item_ptr_cmp(&ctx, a, b), ua.cmp(&ub));
        assert_eq!(board_item_ptr_cmp(&ctx, b, a), ub.cmp(&ua));

        let mut board = Board::new();
        let a = board_target(&mut board, PcbLayerId::FCu);
        let b = board_target(&mut board, PcbLayerId::FCu);
        for id in [a, b] {
            let tid: crate::ids::TargetId = id.typed().unwrap();
            board.targets.get_mut(tid).unwrap().base.uuid = Kiid::default();
        }
        let ctx = BoardCtx::bare(&board);
        assert_eq!(board_item_ptr_cmp(&ctx, a, b), a.cmp(&b));
        assert_eq!(a.cmp(&b), Ordering::Less);
    }

    /// `BOARD_ITEM::GetParentFootprint` and `GetFPRelativePosition` on a footprint child.
    #[test]
    fn parent_footprint_and_relative_position() {
        let mut fx = test_support::rotated_flipped_scaled();
        let lib = Vector2I::new(1_000_000, -2_000_000);
        let transform = fx.board[fx.footprint].transform();
        let mut point = PcbPoint::default();
        point.pos = transform.apply(lib);
        let id = fx.add_child(point, |fp| &mut fp.points);

        let ctx = BoardCtx::bare(&fx.board);
        let point = &fx.board[id];
        let fp = parent_footprint(point, &ctx).expect("parent footprint");
        assert!(std::ptr::eq(fp, &fx.board[fx.footprint]));
        assert_eq!(
            fp_relative_position(point, &ctx),
            transform.inverse_apply(transform.apply(lib))
        );
        assert!(has_board(point, &ctx));
        assert!(parent_footprint(&PcbPoint::default(), &ctx).is_none());
        assert!(!has_board(&PcbPoint::default(), &ctx));
    }

    /// Leaf `Move` / `SetLayer` through the polymorphic board methods.
    #[test]
    fn leaf_move_and_layer() {
        let mut board = Board::new();
        let track = board_track(&mut board, PcbLayerId::FCu);
        board.move_item(track, Vector2I::new(10, 20));
        let tid: crate::ids::TrackId = track.typed().unwrap();
        let Track::Trace(t) = &board[tid] else {
            panic!("segment expected")
        };
        assert_eq!(t.start, Vector2I::new(10, 20));

        // Group SetLayer is a no-op; a stale handle is ignored.
        let group: ItemId = board
            .insert_item(PcbGroup::default(), ParentRef::Board)
            .into();
        board.set_item_layer(group, PcbLayerId::BCu);
        let gid: crate::ids::GroupId = group.typed().unwrap();
        assert_eq!(board[gid].base.layer, PcbLayerId::FCu);
    }

    /// `BOARD_CONNECTED_ITEM::GetEffectiveNetClass` / `GetOwnClearance` without a board or
    /// rules.
    #[test]
    fn connected_item_null_branches() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let track = PcbTrack::default();

        assert!(std::ptr::eq(
            track.effective_netclass(&ctx),
            crate::netclass::Netclass::fallback()
        ));
        assert_eq!(track.own_clearance(&ctx, PcbLayerId::FCu, true), (0, None));
        assert_eq!(
            crate::board_connected_item::netname_msg(&track, &ctx),
            "[** NO BOARD DEFINED **]"
        );
    }
}
