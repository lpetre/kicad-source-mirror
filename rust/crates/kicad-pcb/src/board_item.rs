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

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Lset, PcbLayerId, StrokeParams};
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::approximation::ErrorLoc;

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
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

/// Port of `BOARD_ITEM::IsOnCopperLayer` (header inline, uses `IsSingleLayerType`).
pub fn is_on_copper_layer<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    crate::port_pending!("BOARD_ITEM::IsOnCopperLayer", item, ctx)
}

/// Port of `BOARD_ITEM::IsSideSpecific`.
pub fn is_side_specific<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    crate::port_pending!("BOARD_ITEM::IsSideSpecific", item, ctx)
}

/// Port of `BOARD_ITEM::GetEffectiveShape` (`UNIMPLEMENTED_FOR`; C++ returns a null pointer).
pub fn effective_shape<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    flash: Flashing,
    usage: DrcConstraintT,
) -> SharedShape {
    crate::port_pending!(
        "BOARD_ITEM::GetEffectiveShape",
        item,
        ctx,
        layer,
        flash,
        usage
    )
}

/// Port of `BOARD_ITEM::TransformShapeToPolygon` (`wxCHECK_MSG( false, ... )`).
pub fn transform_shape_to_polygon<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
    ignore_line_width: bool,
) {
    crate::port_pending!(
        "BOARD_ITEM::TransformShapeToPolygon",
        item,
        ctx,
        buffer,
        layer,
        clearance,
        error,
        error_loc,
        ignore_line_width
    )
}

/// Port of `BOARD_ITEM::TransformShapeToPolySet`.
pub fn transform_shape_to_poly_set<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    buffer: &mut ShapePolySet,
    layer: PcbLayerId,
    clearance: i32,
    error: i32,
    error_loc: ErrorLoc,
) {
    crate::port_pending!(
        "BOARD_ITEM::TransformShapeToPolySet",
        item,
        ctx,
        buffer,
        layer,
        clearance,
        error,
        error_loc
    )
}

/// Port of `BOARD_ITEM::GetStroke`.
pub fn stroke<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> StrokeParams {
    crate::port_pending!("BOARD_ITEM::GetStroke", item, ctx)
}

/// Port of `BOARD_ITEM::IsLocked`.
pub fn is_locked<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> bool {
    crate::port_pending!("BOARD_ITEM::IsLocked", item, ctx)
}

/// Port of `EDA_ITEM::GetItemDescription`.
pub fn item_description<B: BoardItem + ?Sized>(
    item: &B,
    ctx: &BoardCtx<'_>,
    units: &UnitsProvider<'_>,
    full: bool,
) -> String {
    crate::port_pending!("EDA_ITEM::GetItemDescription", item, ctx, units, full)
}

/// Port of `EDA_ITEM::GetFriendlyName` (`GetTypeDesc()`).
pub fn friendly_name<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    crate::port_pending!("EDA_ITEM::GetFriendlyName", item, ctx)
}

/// Port of `BOARD_ITEM::LayerMaskDescribe`.
pub fn layer_mask_describe<B: BoardItem + ?Sized>(item: &B, ctx: &BoardCtx<'_>) -> String {
    crate::port_pending!("BOARD_ITEM::LayerMaskDescribe", item, ctx)
}
