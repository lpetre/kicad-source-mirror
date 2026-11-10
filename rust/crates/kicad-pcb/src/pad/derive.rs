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

//! Port of the board-frame derivation part of `pcbnew/pad.cpp` and `pcbnew/pad.h`: frames,
//! layers, attributes, `FlashLayer`, clearance / mask / paste / zone-connection overrides,
//! transforms and `OnFootprint*` (package M14). Inherent methods of `Pad`; the ones that back a
//! trait method have exactly the trait's signature (rule C1, `board-model.md` 6.3).
//!
//! Frames (`board-model.md` 5): `lib_pos`, `lib_orientation` and the padstack sizes, offsets
//! and drill are library-frame values; every board-frame value is derived on read through the
//! parent footprint's `TRANSFORM_TRS` (readers take `&BoardCtx`, writers `&MutEnv`, whose
//! `parent` is the C++ `GetParentFootprint()` of a mutator). `padstack.orientation` is the
//! baked board-frame orientation, written where C++ writes `m_padStack.SetOrientation`.

use kicad_core::layer_ids::{
    is_back_layer, is_copper_layer, is_external_copper_layer, is_front_layer,
};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Lset, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::{
    RECT_CHAMFER_BOTTOM_LEFT, RECT_CHAMFER_BOTTOM_RIGHT, RECT_CHAMFER_TOP_LEFT,
    RECT_CHAMFER_TOP_RIGHT,
};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{
    ANGLE_0, EdaAngle, FlipDirection, Seg, Vector2I, ki_round, mirror_point, rotate_point,
    rotate_point_around, std_clamp,
};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::footprint::Footprint;
use crate::item::{BoardUse, ParentRef};
use crate::item_ref::AnyItem;
use crate::pad::Pad;
use crate::pad_utils::remove_all_drills_from_padstack;
use crate::padstack::{
    CustomShapeZoneMode, PadAttrib, PadDrillShape, PadShape, Padstack, PadstackMode,
    UnconnectedLayerMode,
};
use crate::pcb_track::ZoneLayerOverride;
use crate::zone_settings::ZoneConnection;

// ---- frame helpers (the anonymous namespace of pad.cpp) ------------------------------------

/// Port of `scaleLength( aLength, aFactor )`: `KiROUND( aLength * aFactor )`.
fn scale_length(length: i32, factor: f64) -> i32 {
    ki_round(f64::from(length) * factor)
}

/// Port of `scaleInChildFrame`: express a footprint-frame (Sx, Sy) scale in a frame rotated
/// by `rel_orient` (diagonal part).
fn scale_in_child_frame(sx: f64, sy: f64, rel_orient: EdaAngle) -> (f64, f64) {
    let c = rel_orient.cos();
    let s = rel_orient.sin();
    let local_sx = sx * c * c + sy * s * s;
    let local_sy = sx * s * s + sy * c * c;
    (local_sx, local_sy)
}

/// Port of `derivePadBoardSize`: circular pads use a uniform mean of Sx and Sy so they stay
/// circular under non-uniform scale.
fn derive_pad_board_size(
    lib_size: Vector2I,
    shape: PadShape,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let sx = xform.scale_x().abs();
    let sy = xform.scale_y().abs();

    if shape == PadShape::Circle {
        let uniform = (sx + sy) * 0.5;
        return Vector2I::new(
            scale_length(lib_size.x, uniform),
            scale_length(lib_size.y, uniform),
        );
    }

    // Size is in the pad frame, so conjugate the footprint scale by the pad orientation.
    let (local_sx, local_sy) = scale_in_child_frame(sx, sy, rel_orient);
    Vector2I::new(
        scale_length(lib_size.x, local_sx),
        scale_length(lib_size.y, local_sy),
    )
}

/// Port of `derivePadLibSize` (inverse of `derivePadBoardSize`).
fn derive_pad_lib_size(
    board_size: Vector2I,
    shape: PadShape,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let sx = xform.scale_x().abs();
    let sy = xform.scale_y().abs();

    if shape == PadShape::Circle {
        let uniform = (sx + sy) * 0.5;
        return Vector2I::new(
            scale_length(board_size.x, 1.0 / uniform),
            scale_length(board_size.y, 1.0 / uniform),
        );
    }

    let (local_sx, local_sy) = scale_in_child_frame(sx, sy, rel_orient);
    Vector2I::new(
        scale_length(board_size.x, 1.0 / local_sx),
        scale_length(board_size.y, 1.0 / local_sy),
    )
}

/// Port of `derivePadBoardDrill`: circular drills use a uniform mean so they stay circular.
fn derive_pad_board_drill(
    lib_drill: Vector2I,
    drill_shape: PadDrillShape,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let sx = xform.scale_x().abs();
    let sy = xform.scale_y().abs();

    if drill_shape == PadDrillShape::Circle {
        let uniform = (sx + sy) * 0.5;
        return Vector2I::new(
            scale_length(lib_drill.x, uniform),
            scale_length(lib_drill.y, uniform),
        );
    }

    let (local_sx, local_sy) = scale_in_child_frame(sx, sy, rel_orient);
    Vector2I::new(
        scale_length(lib_drill.x, local_sx),
        scale_length(lib_drill.y, local_sy),
    )
}

/// Port of `derivePadLibDrill` (inverse of `derivePadBoardDrill`).
fn derive_pad_lib_drill(
    board_drill: Vector2I,
    drill_shape: PadDrillShape,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let sx = xform.scale_x().abs();
    let sy = xform.scale_y().abs();

    if drill_shape == PadDrillShape::Circle {
        let uniform = (sx + sy) * 0.5;
        return Vector2I::new(
            scale_length(board_drill.x, 1.0 / uniform),
            scale_length(board_drill.y, 1.0 / uniform),
        );
    }

    let (local_sx, local_sy) = scale_in_child_frame(sx, sy, rel_orient);
    Vector2I::new(
        scale_length(board_drill.x, 1.0 / local_sx),
        scale_length(board_drill.y, 1.0 / local_sy),
    )
}

/// Port of `derivePadBoardOffset`: the offset is a pad-frame vector, so conjugate the
/// footprint scale by the pad orientation.
fn derive_pad_board_offset(
    lib_offset: Vector2I,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let (local_sx, local_sy) =
        scale_in_child_frame(xform.scale_x().abs(), xform.scale_y().abs(), rel_orient);
    Vector2I::new(
        scale_length(lib_offset.x, local_sx),
        scale_length(lib_offset.y, local_sy),
    )
}

/// Port of `derivePadLibOffset` (inverse of `derivePadBoardOffset`).
fn derive_pad_lib_offset(
    board_offset: Vector2I,
    xform: &TransformTrs,
    rel_orient: EdaAngle,
) -> Vector2I {
    let (local_sx, local_sy) =
        scale_in_child_frame(xform.scale_x().abs(), xform.scale_y().abs(), rel_orient);
    Vector2I::new(
        scale_length(board_offset.x, 1.0 / local_sx),
        scale_length(board_offset.y, 1.0 / local_sy),
    )
}

/// The `mirrorBitFlags` lambda of `PAD::mirrorChamferPositions`: swap bits `a` and `b`.
fn mirror_bit_flags(bitfield: &mut i32, a: i32, b: i32) {
    let temp = *bitfield & a != 0;

    if *bitfield & b != 0 {
        *bitfield |= a;
    } else {
        *bitfield &= !a;
    }

    if temp {
        *bitfield |= b;
    } else {
        *bitfield &= !b;
    }
}

/// `MIRROR_AXIS( aCentre, aFlipDirection )` (board_item.h).
fn mirror_axis_seg(centre: Vector2I, dir: FlipDirection) -> Seg {
    if dir == FlipDirection::LeftRight {
        return Seg::new(centre, centre + Vector2I::new(0, 1));
    }

    Seg::new(centre, centre + Vector2I::new(1, 0))
}

/// The environment handed to a custom-shape primitive's mutators. Primitives are in the pad
/// frame and C++ `PCB_SHAPE::transformFp()` is null for them, so they see no parent footprint
/// frame (`board-model.md` 5 rule 6); the board facts (flip table, max error) are the pad's.
fn primitive_env(env: &MutEnv) -> MutEnv {
    MutEnv {
        parent: None,
        ..env.clone()
    }
}

impl Pad {
    // ---- relations ----------------------------------------------------------------------

    /// `GetParentFootprint()` (a pad's parent is its footprint or nothing).
    pub fn parent_footprint<'a>(&self, ctx: &BoardCtx<'a>) -> Option<&'a Footprint> {
        let id = self.base.item.parent.item()?.typed::<Footprint>()?;
        ctx.board.footprint(id)
    }

    /// The parent footprint's `GetTransform()`, `None` without a parent footprint.
    fn parent_transform(&self, ctx: &BoardCtx<'_>) -> Option<TransformTrs> {
        self.parent_footprint(ctx).map(|fp| fp.transform())
    }

    /// The parent footprint's `GetTransform()` as a mutator sees it.
    fn env_transform(env: &MutEnv) -> Option<TransformTrs> {
        env.parent.map(|frame| frame.transform)
    }

    /// The handle of this pad as the C++ `this` owner of its padstack primitives.
    fn self_parent_ref(&self) -> ParentRef {
        match self.base.item.id() {
            Some(id) => ParentRef::Item(id),
            None => ParentRef::None,
        }
    }

    /// `PAD::IsFlipped()`: the parent footprint is on the back layer.
    pub fn is_flipped(&self, ctx: &BoardCtx<'_>) -> bool {
        self.parent_footprint(ctx)
            .is_some_and(|fp| fp.layer(ctx) == PcbLayerId::BCu)
    }

    /// `PAD::SameLogicalPadAs( aOther )`.
    pub fn same_logical_pad_as(&self, other: &Pad) -> bool {
        // hide tricks behind sensible API
        self.base.item.parent.item() == other.base.item.parent.item()
            && self.base.item.parent.item().is_some()
            && !self.number.is_empty()
            && self.number == other.number
    }

    /// `PAD::SharesNetTieGroup( aOther )`.
    pub fn shares_net_tie_group(&self, ctx: &BoardCtx<'_>, other: &Pad) -> bool {
        let parent_id = self.base.item.parent.item();

        if let Some(parent_fp) = self.parent_footprint(ctx) {
            if parent_fp.is_net_tie()
                && other.parent_footprint(ctx).is_some()
                && other.base.item.parent.item() == parent_id
            {
                let pad_to_net_tie_group_map = parent_fp.map_pad_numbers_to_net_tie_groups(ctx);
                // std::map::operator[] value-initialises a missing number to 0.
                let this_net_tie_group = pad_to_net_tie_group_map
                    .get(self.number())
                    .copied()
                    .unwrap_or(0);
                let other_net_tie_group = pad_to_net_tie_group_map
                    .get(other.number())
                    .copied()
                    .unwrap_or(0);

                return this_net_tie_group >= 0 && this_net_tie_group == other_net_tie_group;
            }
        }

        false
    }

    // ---- frames (board-model.md 5) ------------------------------------------------------

    /// `PAD::GetPosition()`: `m_libPos` through the parent footprint's transform.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        if let Some(xform) = self.parent_transform(ctx) {
            return xform.apply(self.lib_pos);
        }

        self.lib_pos
    }

    /// `PAD::GetPosition()` with the parent frame of a mutator.
    fn position_env(&self, env: &MutEnv) -> Vector2I {
        if let Some(xform) = Self::env_transform(env) {
            return xform.apply(self.lib_pos);
        }

        self.lib_pos
    }

    /// `PAD::SetPosition()`: stores the library-frame position of `pos`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        if let Some(xform) = Self::env_transform(env) {
            self.lib_pos = xform.inverse_apply(pos);
        } else {
            self.lib_pos = pos;
        }

        self.set_dirty();
    }

    /// `PAD::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `GetFPRelativePosition()` (the `BOARD_ITEM` body: the inverse transform of
    /// `GetPosition()`).
    pub fn fp_relative_position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::board_item::fp_relative_position(self, ctx)
    }

    /// `SetFPRelativePosition()` (the `BOARD_ITEM` body: `SetPosition` of the transformed
    /// point).
    pub fn set_fp_relative_position(&mut self, env: &MutEnv, pos: Vector2I) {
        if let Some(xform) = Self::env_transform(env) {
            self.set_position(env, xform.apply(pos));
        } else {
            self.set_position(env, pos);
        }
    }

    /// `PAD::GetOrientation()`: the board-frame orientation.
    pub fn orientation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        if let Some(parent_fp) = self.parent_footprint(ctx) {
            let angle = self.lib_orientation.angle() + parent_fp.orientation();
            return angle.normalized();
        }

        self.lib_orientation.angle()
    }

    /// `PAD::GetOrientation()` with the parent frame of a mutator.
    fn orientation_env(&self, env: &MutEnv) -> EdaAngle {
        if let Some(xform) = Self::env_transform(env) {
            let angle = self.lib_orientation.angle() + xform.rotate();
            return angle.normalized();
        }

        self.lib_orientation.angle()
    }

    /// `PAD::GetOrientationDegrees()`.
    pub fn orientation_degrees(&self, ctx: &BoardCtx<'_>) -> f64 {
        self.orientation(ctx).as_degrees()
    }

    /// `PAD::SetOrientation()`: library orientation from the board angle, re-bakes
    /// `padstack.orientation`, marks the shapes dirty.
    pub fn set_orientation(&mut self, env: &MutEnv, angle: EdaAngle) {
        if let Some(xform) = Self::env_transform(env) {
            self.lib_orientation.set_angle(angle - xform.rotate());
        } else {
            self.lib_orientation.set_angle(angle);
        }

        self.padstack.set_orientation(angle);
        self.set_dirty();
    }

    /// `PAD::GetFPRelativeOrientation()`.
    pub fn fp_relative_orientation(&self, _ctx: &BoardCtx<'_>) -> EdaAngle {
        self.lib_orientation.angle()
    }

    /// `PAD::SetFPRelativeOrientation()`.
    pub fn set_fp_relative_orientation(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.lib_orientation.set_angle(angle);

        if let Some(xform) = Self::env_transform(env) {
            self.padstack.set_orientation(angle + xform.rotate());
        } else {
            self.padstack.set_orientation(angle);
        }

        self.set_dirty();
    }

    /// `PAD::GetSize( aLayer )`: the board-frame size of the shape on `layer`.
    pub fn size(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        self.size_in(self.parent_transform(ctx), layer)
    }

    /// `PAD::GetSize( aLayer )` for a given parent transform.
    fn size_in(&self, xform: Option<TransformTrs>, layer: PcbLayerId) -> Vector2I {
        if let Some(xform) = xform {
            return derive_pad_board_size(
                self.padstack.size(layer),
                self.shape(layer),
                &xform,
                self.lib_orientation(),
            );
        }

        self.padstack.size(layer)
    }

    /// `PAD::SetSize( aLayer, aSize )`: board-frame size, stored in the library frame.
    pub fn set_size(&mut self, env: &MutEnv, layer: PcbLayerId, size: Vector2I) {
        if let Some(xform) = Self::env_transform(env) {
            let lib_size =
                derive_pad_lib_size(size, self.shape(layer), &xform, self.lib_orientation());
            self.padstack.set_size(lib_size, layer);
        } else {
            self.padstack.set_size(size, layer);
        }

        self.set_dirty();
    }

    /// `PAD::SetLibSize( aLayer, aSize )` (parser).
    pub fn set_lib_size(&mut self, layer: PcbLayerId, size: Vector2I) {
        self.padstack.set_size(size, layer);
        self.set_dirty();
    }

    /// `PAD::SetSizeXProp( aX )`: ignored for `aX <= 0`; circles stay circles.
    pub fn set_size_x_prop(&mut self, env: &MutEnv, x: i32) {
        if x <= 0 {
            return;
        }

        let mut y = self
            .size_in(Self::env_transform(env), Padstack::ALL_LAYERS)
            .y;

        if self.shape(Padstack::ALL_LAYERS) == PadShape::Circle {
            y = x;
        }

        self.set_size(env, Padstack::ALL_LAYERS, Vector2I::new(x, y));
    }

    /// `PAD::SetSizeYProp( aY )`: ignored for `aY <= 0`; circles stay circles.
    pub fn set_size_y_prop(&mut self, env: &MutEnv, y: i32) {
        if y <= 0 {
            return;
        }

        let mut x = self
            .size_in(Self::env_transform(env), Padstack::ALL_LAYERS)
            .x;

        if self.shape(Padstack::ALL_LAYERS) == PadShape::Circle {
            x = y;
        }

        self.set_size(env, Padstack::ALL_LAYERS, Vector2I::new(x, y));
    }

    /// `PAD::GetOffset( aLayer )`: board-frame offset.
    pub fn offset(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        self.offset_in(self.parent_transform(ctx), layer)
    }

    /// `PAD::GetOffset( aLayer )` for a given parent transform.
    fn offset_in(&self, xform: Option<TransformTrs>, layer: PcbLayerId) -> Vector2I {
        if let Some(xform) = xform {
            return derive_pad_board_offset(
                self.padstack.offset(layer),
                &xform,
                self.lib_orientation(),
            );
        }

        self.padstack.offset(layer)
    }

    /// `PAD::SetOffset( aLayer, aOffset )`.
    pub fn set_offset(&mut self, env: &MutEnv, layer: PcbLayerId, offset: Vector2I) {
        if let Some(xform) = Self::env_transform(env) {
            let lib_offset = derive_pad_lib_offset(offset, &xform, self.lib_orientation());
            *self.padstack.offset_mut(layer) = lib_offset;
        } else {
            *self.padstack.offset_mut(layer) = offset;
        }

        self.set_dirty();
    }

    /// `PAD::SetLibOffset( aLayer, aOffset )` (parser).
    pub fn set_lib_offset(&mut self, layer: PcbLayerId, offset: Vector2I) {
        *self.padstack.offset_mut(layer) = offset;
        self.set_dirty();
    }

    /// `PAD::ShapePos( aLayer )`: position of the shape centre (position + rotated offset).
    pub fn shape_pos(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        let pos = self.position(ctx);
        let mut loc_offset = self.offset(ctx, layer);

        if loc_offset.x == 0 && loc_offset.y == 0 {
            return pos;
        }

        rotate_point(&mut loc_offset, self.orientation(ctx));

        pos + loc_offset
    }

    /// `PAD::GetDrillSize()` (= `GetPrimaryDrillSize()`): board-frame drill size.
    pub fn drill_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.drill_size_in(self.parent_transform(ctx))
    }

    /// `PAD::GetPrimaryDrillSize()` for a given parent transform.
    fn drill_size_in(&self, xform: Option<TransformTrs>) -> Vector2I {
        if let Some(xform) = xform {
            return derive_pad_board_drill(
                self.padstack.drill().size,
                self.drill_shape(),
                &xform,
                self.lib_orientation(),
            );
        }

        self.padstack.drill().size
    }

    /// `PAD::SetDrillSize()` (= `SetPrimaryDrillSize()`).
    pub fn set_drill_size(&mut self, env: &MutEnv, size: Vector2I) {
        if let Some(xform) = Self::env_transform(env) {
            self.padstack.drill_mut().size =
                derive_pad_lib_drill(size, self.drill_shape(), &xform, self.lib_orientation());
        } else {
            self.padstack.drill_mut().size = size;
        }

        self.set_dirty();
    }

    /// `PAD::SetPrimaryDrillSizeX( aX )`: circular drills stay circular.
    pub fn set_drill_size_x(&mut self, env: &MutEnv, x: i32) {
        let mut drill = self.drill_size_in(Self::env_transform(env));
        drill.x = x;

        if self.drill_shape() == PadDrillShape::Circle {
            drill.y = x;
        }

        self.set_drill_size(env, drill);
    }

    /// `PAD::SetPrimaryDrillSizeY( aY )`.
    pub fn set_drill_size_y(&mut self, env: &MutEnv, y: i32) {
        let mut drill = self.drill_size_in(Self::env_transform(env));
        drill.y = y;
        self.set_drill_size(env, drill);
    }

    /// `PAD::SetLibDrillSize()` (parser).
    pub fn set_lib_drill_size(&mut self, size: Vector2I) {
        self.padstack.drill_mut().size = size;
        self.set_dirty();
    }

    /// `PAD::GetPrimitiveLibScale()`: the (x, y) scale that re-bakes pad-frame primitives.
    pub fn primitive_lib_scale(&self, ctx: &BoardCtx<'_>) -> (f64, f64) {
        Self::primitive_lib_scale_in(self.parent_transform(ctx), self.lib_orientation())
    }

    /// `PAD::GetPrimitiveLibScale()` for a given parent transform.
    fn primitive_lib_scale_in(xform: Option<TransformTrs>, rel_orient: EdaAngle) -> (f64, f64) {
        let mut scale = (1.0, 1.0);

        if let Some(xform) = xform {
            scale = scale_in_child_frame(xform.scale_x().abs(), xform.scale_y().abs(), rel_orient);
        }

        scale
    }

    // ---- drill and slot accessors (pad.h / pad.cpp) -------------------------------------

    /// `PAD::GetDrillShape()` (= `GetPrimaryDrillShape()`).
    pub fn drill_shape(&self) -> PadDrillShape {
        self.padstack.drill().shape
    }

    /// `PAD::SetDrillShape()` (= `SetPrimaryDrillShape()`): a circular drill gets
    /// `size.y = size.x`.
    pub fn set_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.drill_mut().shape = shape;

        if shape == PadDrillShape::Circle {
            let x = self.padstack.drill().size.x;
            self.padstack.drill_mut().size.y = x;
        }

        self.set_dirty();
    }

    /// `PAD::GetPrimaryDrillStartLayer()`.
    pub fn drill_start_layer(&self) -> PcbLayerId {
        self.padstack.drill().start
    }

    /// `PAD::SetPrimaryDrillStartLayer()`.
    pub fn set_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.drill_mut().start = layer;
        self.set_dirty();
    }

    /// `PAD::GetPrimaryDrillEndLayer()`.
    pub fn drill_end_layer(&self) -> PcbLayerId {
        self.padstack.drill().end
    }

    /// `PAD::SetPrimaryDrillEndLayer()`.
    pub fn set_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.drill_mut().end = layer;
        self.set_dirty();
    }

    /// `PAD::SetPrimaryDrillFilled()` (also `SetPrimaryDrillFilledFlag` with `Some`).
    pub fn set_drill_filled(&mut self, filled: Option<bool>) {
        self.padstack.drill_mut().is_filled = filled;
        self.set_dirty();
    }

    /// `PAD::GetPrimaryDrillFilledFlag()`.
    pub fn drill_filled_flag(&self) -> bool {
        self.padstack.drill().is_filled.unwrap_or(false)
    }

    /// `PAD::SetPrimaryDrillCapped()` (also `SetPrimaryDrillCappedFlag` with `Some`).
    pub fn set_drill_capped(&mut self, capped: Option<bool>) {
        self.padstack.drill_mut().is_capped = capped;
        self.set_dirty();
    }

    /// `PAD::GetPrimaryDrillCappedFlag()`.
    pub fn drill_capped_flag(&self) -> bool {
        self.padstack.drill().is_capped.unwrap_or(false)
    }

    /// `PAD::GetSecondaryDrillSize()` (library frame, as C++).
    pub fn secondary_drill_size(&self) -> Vector2I {
        self.padstack.secondary_drill().size
    }

    /// `PAD::SetSecondaryDrillSize()`.
    pub fn set_secondary_drill_size(&mut self, size: Vector2I) {
        self.padstack.secondary_drill_mut().size = size;
        self.set_dirty();
    }

    /// `PAD::SetSecondaryDrillSizeX()`: circular drills stay circular.
    pub fn set_secondary_drill_size_x(&mut self, x: i32) {
        self.padstack.secondary_drill_mut().size.x = x;

        if self.padstack.secondary_drill().shape == PadDrillShape::Circle {
            self.padstack.secondary_drill_mut().size.y = x;
        }

        self.set_dirty();
    }

    /// `PAD::SetSecondaryDrillSizeY()`.
    pub fn set_secondary_drill_size_y(&mut self, y: i32) {
        self.padstack.secondary_drill_mut().size.y = y;
        self.set_dirty();
    }

    /// `PAD::ClearSecondaryDrillSize()`.
    pub fn clear_secondary_drill_size(&mut self) {
        self.padstack.secondary_drill_mut().size = Vector2I::new(0, 0);
        self.set_dirty();
    }

    /// `PAD::SetSecondaryDrillShape()`.
    pub fn set_secondary_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.secondary_drill_mut().shape = shape;
        self.set_dirty();
    }

    /// `PAD::SetSecondaryDrillStartLayer()`.
    pub fn set_secondary_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.secondary_drill_mut().start = layer;
        self.set_dirty();
    }

    /// `PAD::SetSecondaryDrillEndLayer()`.
    pub fn set_secondary_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.secondary_drill_mut().end = layer;
        self.set_dirty();
    }

    /// `PAD::GetTertiaryDrillSize()` (library frame, as C++).
    pub fn tertiary_drill_size(&self) -> Vector2I {
        self.padstack.tertiary_drill().size
    }

    /// `PAD::SetTertiaryDrillSize()`.
    pub fn set_tertiary_drill_size(&mut self, size: Vector2I) {
        self.padstack.tertiary_drill_mut().size = size;
        self.set_dirty();
    }

    /// `PAD::SetTertiaryDrillSizeX()`: circular drills stay circular.
    pub fn set_tertiary_drill_size_x(&mut self, x: i32) {
        self.padstack.tertiary_drill_mut().size.x = x;

        if self.padstack.tertiary_drill().shape == PadDrillShape::Circle {
            self.padstack.tertiary_drill_mut().size.y = x;
        }

        self.set_dirty();
    }

    /// `PAD::SetTertiaryDrillSizeY()`.
    pub fn set_tertiary_drill_size_y(&mut self, y: i32) {
        self.padstack.tertiary_drill_mut().size.y = y;
        self.set_dirty();
    }

    /// `PAD::ClearTertiaryDrillSize()`.
    pub fn clear_tertiary_drill_size(&mut self) {
        self.padstack.tertiary_drill_mut().size = Vector2I::new(0, 0);
        self.set_dirty();
    }

    /// `PAD::SetTertiaryDrillShape()`.
    pub fn set_tertiary_drill_shape(&mut self, shape: PadDrillShape) {
        self.padstack.tertiary_drill_mut().shape = shape;
        self.set_dirty();
    }

    /// `PAD::SetTertiaryDrillStartLayer()`.
    pub fn set_tertiary_drill_start_layer(&mut self, layer: PcbLayerId) {
        self.padstack.tertiary_drill_mut().start = layer;
        self.set_dirty();
    }

    /// `PAD::SetTertiaryDrillEndLayer()`.
    pub fn set_tertiary_drill_end_layer(&mut self, layer: PcbLayerId) {
        self.padstack.tertiary_drill_mut().end = layer;
        self.set_dirty();
    }

    /// `PAD::GetBackdrillOrPostMachiningKnockout( aLayer )`: 0 off copper or without a board.
    pub fn backdrill_or_post_machining_knockout(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
    ) -> i32 {
        if !is_copper_layer(layer) || !crate::board_item::has_board(self, ctx) {
            return 0;
        }

        self.padstack.machining_knockout(ctx, layer)
    }

    /// `PAD::IsBackdrilledOrPostMachined( aLayer )`.
    pub fn is_backdrilled_or_post_machined(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.backdrill_or_post_machining_knockout(ctx, layer) > 0
    }

    /// `PAD::GetPostMachiningKnockout( aLayer )`: 0 off copper or without a board.
    pub fn post_machining_knockout(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        if !is_copper_layer(layer) || !crate::board_item::has_board(self, ctx) {
            return 0;
        }

        self.padstack
            .post_machining_knockout(ctx.board.design_settings().stackup_descriptor(), layer)
    }

    // ---- per-layer shape accessors forwarding to the padstack (pad.h / pad.cpp) ---------

    /// `PAD::GetAnchorPadShape( aLayer )`.
    pub fn anchor_pad_shape(&self, layer: PcbLayerId) -> PadShape {
        self.padstack.anchor_shape(layer)
    }

    /// `PAD::SetShape( aLayer, aShape )`.
    pub fn set_shape(&mut self, layer: PcbLayerId, shape: PadShape) {
        self.padstack.set_shape(shape, layer);
        self.set_dirty();
    }

    /// `PAD::SetAnchorPadShape( aLayer, aShape )`: only circle and rectangle anchors.
    pub fn set_anchor_pad_shape(&mut self, layer: PcbLayerId, shape: PadShape) {
        let anchor = if shape == PadShape::Rectangle {
            PadShape::Rectangle
        } else {
            PadShape::Circle
        };
        self.padstack.set_anchor_shape(anchor, layer);
        self.set_dirty();
    }

    /// `PAD::GetDelta( aLayer )` (trapezoid delta, library frame as C++).
    pub fn delta(&self, layer: PcbLayerId) -> Vector2I {
        self.padstack.trapezoid_delta_size(layer)
    }

    /// `PAD::SetDelta( aLayer, aSize )`.
    pub fn set_delta(&mut self, layer: PcbLayerId, size: Vector2I) {
        *self.padstack.trapezoid_delta_size_mut(layer) = size;
        self.set_dirty();
    }

    /// `PAD::GetRoundRectCornerRadius( aLayer )` (library frame size, as C++).
    pub fn round_rect_corner_radius(&self, layer: PcbLayerId) -> i32 {
        self.padstack.round_rect_radius(layer)
    }

    /// `PAD::SetRoundRectCornerRadius( aLayer, aRadius )`.
    pub fn set_round_rect_corner_radius(&mut self, layer: PcbLayerId, radius: f64) {
        self.padstack.set_round_rect_radius(radius, layer);
    }

    /// `PAD::GetRoundRectRadiusRatio( aLayer )`.
    pub fn round_rect_radius_ratio(&self, layer: PcbLayerId) -> f64 {
        self.padstack.round_rect_radius_ratio(layer)
    }

    /// `PAD::SetRoundRectRadiusRatio( aLayer, aRadiusScale )`: clamped to [0, 0.5].
    pub fn set_round_rect_radius_ratio(&mut self, layer: PcbLayerId, radius_scale: f64) {
        self.padstack
            .set_round_rect_radius_ratio(std_clamp(radius_scale, 0.0, 0.5), layer);
        self.set_dirty();
    }

    /// `PAD::SetFrontRoundRectRadiusRatio( aRadiusScale )`: clamped to [0, 0.5].
    pub fn set_front_round_rect_radius_ratio(&mut self, radius_scale: f64) {
        debug_assert!(
            self.padstack.mode() == PadstackMode::Normal,
            "Set front radius only meaningful for normal padstacks"
        );

        self.padstack
            .set_round_rect_radius_ratio(std_clamp(radius_scale, 0.0, 0.5), PcbLayerId::FCu);
        self.set_dirty();
    }

    /// `PAD::SetFrontRoundRectRadiusSize( aRadius )`: ratio of the board-frame F_Cu size.
    pub fn set_front_round_rect_radius_size(&mut self, env: &MutEnv, radius: i32) {
        let size = self.size_in(Self::env_transform(env), PcbLayerId::FCu);
        let min_size = size.x.min(size.y);
        let new_ratio = f64::from(radius) / f64::from(min_size);

        self.set_front_round_rect_radius_ratio(new_ratio);
    }

    /// `PAD::GetFrontRoundRectRadiusSize()`: `KiROUND( ratio * min( board-frame size ) )`.
    pub fn front_round_rect_radius_size(&self, ctx: &BoardCtx<'_>) -> i32 {
        let size = self.size(ctx, PcbLayerId::FCu);
        let min_size = size.x.min(size.y);
        let ratio = self.padstack.round_rect_radius_ratio(PcbLayerId::FCu);

        ki_round(ratio * f64::from(min_size))
    }

    /// `PAD::GetChamferRectRatio( aLayer )`.
    pub fn chamfer_rect_ratio(&self, layer: PcbLayerId) -> f64 {
        self.padstack.chamfer_ratio(layer)
    }

    /// `PAD::SetChamferRectRatio( aLayer, aChamferScale )`.
    pub fn set_chamfer_rect_ratio(&mut self, layer: PcbLayerId, chamfer_scale: f64) {
        self.padstack.set_chamfer_ratio(chamfer_scale, layer);
        self.set_dirty();
    }

    /// `PAD::GetChamferPositions( aLayer )`.
    pub fn chamfer_positions(&self, layer: PcbLayerId) -> i32 {
        self.padstack.chamfer_positions(layer)
    }

    /// `PAD::SetChamferPositions( aLayer, aPositions )`.
    pub fn set_chamfer_positions(&mut self, layer: PcbLayerId, positions: i32) {
        self.padstack.set_chamfer_positions(positions, layer);
    }

    /// `PAD::GetCustomShapeInZoneOpt()`.
    pub fn custom_shape_in_zone_opt(&self) -> CustomShapeZoneMode {
        self.padstack.custom_shape_in_zone_mode()
    }

    /// `PAD::SetCustomShapeInZoneOpt()`.
    pub fn set_custom_shape_in_zone_opt(&mut self, option: CustomShapeZoneMode) {
        self.padstack.set_custom_shape_in_zone_mode(option);
    }

    /// `PAD::GetUnconnectedLayerMode()`.
    pub fn unconnected_layer_mode(&self) -> UnconnectedLayerMode {
        self.padstack.unconnected_layer_mode()
    }

    /// `PAD::SetUnconnectedLayerMode()`.
    pub fn set_unconnected_layer_mode(&mut self, mode: UnconnectedLayerMode) {
        self.padstack.set_unconnected_layer_mode(mode);
    }

    /// `PAD::GetRemoveUnconnected()`.
    pub fn remove_unconnected(&self) -> bool {
        self.padstack.unconnected_layer_mode() != UnconnectedLayerMode::KeepAll
    }

    /// `PAD::GetKeepTopBottom()`.
    pub fn keep_top_bottom(&self) -> bool {
        self.padstack.unconnected_layer_mode() == UnconnectedLayerMode::RemoveExceptStartAndEnd
    }

    /// `PAD::ConditionallyFlashed( aLayer )`.
    pub fn conditionally_flashed(&self, layer: PcbLayerId) -> bool {
        match self.padstack.unconnected_layer_mode() {
            UnconnectedLayerMode::KeepAll => false,
            UnconnectedLayerMode::RemoveAll => true,
            UnconnectedLayerMode::RemoveExceptStartAndEnd | UnconnectedLayerMode::StartEndOnly => {
                layer != self.padstack.drill().start && layer != self.padstack.drill().end
            }
        }
    }

    // ---- layers -------------------------------------------------------------------------

    /// `PAD::GetLayer()`: the principal layer.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.principal_layer(ctx)
    }

    /// `PAD::GetLayerSet()`: the padstack layer set.
    pub fn layer_set(&self, _ctx: &BoardCtx<'_>) -> Lset {
        self.padstack.layer_set.clone()
    }

    /// `PAD::SetLayerSet()`. The C++ `InvalidateClearanceCache` has no counterpart: the DRC
    /// engine revalidates by time stamp (`board-model.md` 7).
    pub fn set_layer_set(&mut self, _env: &MutEnv, layers: Lset) {
        self.set_layer_set_impl(&layers);
    }

    /// The body of `PAD::SetLayerSet()` (no environment needed).
    fn set_layer_set_impl(&mut self, layers: &Lset) {
        self.padstack.set_layer_set(layers.clone());
        self.set_dirty();

        // In theory m_layer should never be read, but set it just to be safe.
        let m_layer = self.base.item.layer;

        if m_layer == PcbLayerId::UndefinedLayer || !layers.contains(m_layer) {
            let seq = layers.seq();

            self.base.item.layer = match seq.0.first() {
                Some(&front) => front,
                None => PcbLayerId::UndefinedLayer,
            };
        }
    }

    /// `PAD::IsOnLayer()`.
    pub fn is_on_layer(&self, _ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.padstack.layer_set.contains(layer)
    }

    /// `PAD::IsOnCopperLayer()`: an NPTH pad without annular ring (in the library frame, as
    /// C++ reads `m_padStack` directly) is not on copper.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        if self.attribute == PadAttrib::Npth {
            // NPTH pads have no plated hole cylinder.  If their annular ring size is 0 or
            // negative, then they have no annular ring either.
            let mut has_annular_ring = true;
            let ps = &self.padstack;

            ps.for_each_unique_layer(|layer| match self.shape(layer) {
                PadShape::Circle
                    if ps.offset(layer) == Vector2I::new(0, 0)
                        && ps.size(layer).x <= ps.drill().size.x =>
                {
                    has_annular_ring = false;
                }
                PadShape::Oval
                    if ps.offset(layer) == Vector2I::new(0, 0)
                        && ps.size(layer).x <= ps.drill().size.x
                        && ps.size(layer).y <= ps.drill().size.y =>
                {
                    has_annular_ring = false;
                }
                _ => {
                    // We could subtract the hole polygon from the shape polygon for these,
                    // but it would be expensive and we're probably well out of the common use
                    // cases....
                }
            });

            if !has_annular_ring {
                return false;
            }
        }

        let mut copper = self.padstack.layer_set.clone();
        copper &= Lset::all_cu_mask();
        copper.any()
    }

    /// `PAD::FlashLayer( aLayer )`: whether the pad has copper on `layer` (asks
    /// `ctx.connectivity` and the zone-layer overrides for conditionally flashed pads).
    pub fn flash_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.flash_layer_impl(ctx, layer.0, false)
    }

    /// `PAD::FlashLayer( aLayer, aOnlyCheckIfPermitted = true )` (= `CanFlashLayer`).
    pub fn flash_layer_permitted(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.flash_layer_impl(ctx, layer.0, true)
    }

    /// Port of `PAD::FlashLayer( int aLayer, bool aOnlyCheckIfPermitted )`.
    fn flash_layer_impl(
        &self,
        ctx: &BoardCtx<'_>,
        a_layer: i32,
        only_check_if_permitted: bool,
    ) -> bool {
        let mut a_layer = a_layer;

        if a_layer == PcbLayerId::UndefinedLayer.0 {
            return true;
        }

        // Sometimes this is called with GAL layers and should just return true
        if a_layer > PCB_LAYER_ID_COUNT {
            return true;
        }

        let layer = PcbLayerId(a_layer);

        if !self.is_on_layer(ctx, layer) {
            return false;
        }

        if self.attribute == PadAttrib::Npth && is_copper_layer(a_layer) {
            if self.shape(layer) == PadShape::Circle && self.drill_shape() == PadDrillShape::Circle
            {
                if self.offset(ctx, layer) == Vector2I::new(0, 0)
                    && self.drill_size(ctx).x >= self.size(ctx, layer).x
                {
                    return false;
                }
            } else if self.shape(layer) == PadShape::Oval
                && self.drill_shape() == PadDrillShape::Oblong
            {
                let drill = self.drill_size(ctx);
                let size = self.size(ctx, layer);

                if self.offset(ctx, layer) == Vector2I::new(0, 0)
                    && drill.x >= size.x
                    && drill.y >= size.y
                {
                    return false;
                }
            }
        }

        if self.attribute == PadAttrib::Pth
            && (layer == PcbLayerId::FMask || layer == PcbLayerId::BMask)
        {
            return true;
        }

        if Lset::front_board_tech_mask().contains(PcbLayerId(a_layer)) {
            a_layer = PcbLayerId::FCu.0;
        } else if Lset::back_board_tech_mask().contains(PcbLayerId(a_layer)) {
            a_layer = PcbLayerId::BCu.0;
        }

        if self.attribute == PadAttrib::Pth && is_copper_layer(a_layer) {
            let mode = self.padstack.unconnected_layer_mode();

            if mode == UnconnectedLayerMode::KeepAll {
                return true;
            }

            // Plated through hole pads need copper on the top/bottom layers for proper
            // soldering. Unless the user has removed them in the pad dialog
            if mode == UnconnectedLayerMode::StartEndOnly {
                return a_layer == self.padstack.drill().start.0
                    || a_layer == self.padstack.drill().end.0;
            }

            if mode == UnconnectedLayerMode::RemoveExceptStartAndEnd
                && is_external_copper_layer(a_layer)
            {
                return true;
            }

            if crate::board_item::has_board(self, ctx) {
                // A forced flash (zone filler) or a permission-only query flashes.
                if self.zone_layer_override(layer) == ZoneLayerOverride::ForceFlashed
                    || only_check_if_permitted
                {
                    return true;
                } else {
                    // Must be static to keep from raising its ugly head in performance profiles
                    const NON_ZONE_TYPES: [KicadT; 4] = [
                        KicadT::PcbTraceT,
                        KicadT::PcbArcT,
                        KicadT::PcbViaT,
                        KicadT::PcbPadT,
                    ];

                    // ctx.connectivity == None is an empty CONNECTIVITY_DATA: not connected.
                    let Some(connectivity) = ctx.connectivity else {
                        return false;
                    };

                    let local = *ctx;
                    let this = local.view(AnyItem::Pad(self));

                    return connectivity.is_connected_on_layer(
                        &local,
                        this,
                        PcbLayerId(a_layer),
                        &NON_ZONE_TYPES,
                    );
                }
            }
        }

        true
    }

    /// `PAD::FlashLayer( const LSET& aLayers )`: true if flashed on any of `layers`.
    pub fn flash_layer_set(&self, ctx: &BoardCtx<'_>, layers: &Lset) -> bool {
        for layer in layers.iter() {
            if self.flash_layer(ctx, layer) {
                return true;
            }
        }

        false
    }

    /// `PAD::GetPrincipalLayer()`.
    pub fn principal_layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        let layer_set = self.layer_set(ctx);

        if self.attribute == PadAttrib::Smd
            || self.attribute == PadAttrib::Conn
            || self.attribute == PadAttrib::NonComponent
            || layer_set.none()
        {
            self.base.item.layer
        } else {
            layer_set.seq().0[0]
        }
    }

    /// `PAD::IsAperturePad()`: no copper layer in the padstack layer set.
    pub fn is_aperture_pad(&self) -> bool {
        let mut copper = self.padstack.layer_set.clone();
        copper &= Lset::all_cu_mask();
        copper.none()
    }

    /// `PAD::CanHaveNumber()`.
    pub fn can_have_number(&self) -> bool {
        // Aperture pads don't get a number
        if self.is_aperture_pad() {
            return false;
        }

        // NPTH pads don't get numbers
        if self.attribute == PadAttrib::Npth {
            return false;
        }

        true
    }

    /// `PAD::IsNPTHWithNoCopper()` (board-frame sizes, as C++).
    pub fn is_npth_with_no_copper(&self, ctx: &BoardCtx<'_>) -> bool {
        if self.attribute != PadAttrib::Npth {
            return false;
        }

        let mut has_copper = false;
        let drill = self.drill_size(ctx);

        self.padstack.for_each_unique_layer(|layer| {
            if self.shape(layer) == PadShape::Circle {
                if self.size(ctx, layer).x > drill.x {
                    has_copper = true;
                }
            } else if self.shape(layer) == PadShape::Oval {
                let size = self.size(ctx, layer);

                if size.x > drill.x || size.y > drill.y {
                    has_copper = true;
                }
            } else {
                has_copper = true;
            }
        });

        !has_copper
    }

    /// `PAD::IsNoConnectPad()`.
    pub fn is_no_connect_pad(&self) -> bool {
        self.pin_type.contains("no_connect")
    }

    /// `PAD::IsFreePad()`: an "unconnected-(" net and a "free" pin type. Takes the context
    /// because `GetShortNetname()` reads the net (rev 2.3).
    pub fn is_free_pad(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::board_connected_item::short_netname(self, ctx).starts_with("unconnected-(")
            && self.pin_type == "free"
    }

    // ---- state / attributes -------------------------------------------------------------

    /// `PAD::IsLocked()`: parent group, footprint holder, footprint or own lock.
    pub fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool {
        if let ParentRef::Item(parent) = self.base.item.parent {
            if ctx.item(parent).is_some_and(|p| p.is_locked()) {
                return true;
            }
        }

        crate::board_item::is_locked(self, ctx)
    }

    /// `PAD::HasHole()`: `GetDrillSize().x > 0 && GetDrillSize().y > 0`.
    pub fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        let drill = self.drill_size(ctx);
        drill.x > 0 && drill.y > 0
    }

    /// `PAD::HasDrilledHole()`: `HasHole()` with a round hole.
    pub fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        let drill = self.drill_size(ctx);
        self.has_hole(ctx) && drill.x == drill.y
    }

    /// `PAD::SetAttribute()`: also resets the layer set to the attribute's default mask when
    /// the attribute changes, as C++. The C++ `InvalidateClearanceCache` has no counterpart.
    pub fn set_attribute(&mut self, attribute: crate::padstack::PadAttrib) {
        if self.attribute != attribute {
            self.attribute = attribute;

            match attribute {
                PadAttrib::Pth => {
                    // Plump up to all copper layers
                    *self.padstack.layer_set_mut() |= Lset::all_cu_mask();
                }
                PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent => {
                    // Trim down to no more than one copper layer
                    let mut copper_layers = self.padstack.layer_set.clone();
                    copper_layers &= Lset::all_cu_mask();

                    if copper_layers.count() > 1 {
                        let not_cu = !Lset::all_cu_mask().clone();
                        *self.padstack.layer_set_mut() &= &not_cu;

                        if copper_layers.contains(PcbLayerId::BCu) {
                            self.base.item.layer = PcbLayerId::BCu;
                        } else {
                            self.base.item.layer = copper_layers.seq().0[0];
                        }

                        let m_layer = self.base.item.layer;
                        self.padstack.layer_set_mut().set(m_layer);
                    }

                    // No hole, so no drilling of any kind
                    remove_all_drills_from_padstack(&mut self.padstack);
                }
                PadAttrib::Npth => {
                    // No number; no net. (SetNetCode( UNCONNECTED ) always ends on net 0:
                    // FindNet( 0 ) of a board or OrphanedItem() without one.)
                    self.number = String::new();
                    self.base.net = crate::ids::NetCode::UNCONNECTED;
                }
            }
        }

        self.set_dirty();
    }

    /// `PAD::SetPinFunction()`.
    pub fn set_pin_function(&mut self, name: &str) {
        self.pin_function = name.to_string();
    }

    /// `PAD::SetPinType()`.
    pub fn set_pin_type(&mut self, pin_type: &str) {
        self.pin_type = pin_type.to_string();
    }

    /// `PAD::SetPadToDieLength()`.
    pub fn set_pad_to_die_length(&mut self, length: i32) {
        self.length_pad_to_die = length;
    }

    /// `PAD::SetPadToDieDelay()`.
    pub fn set_pad_to_die_delay(&mut self, delay: i32) {
        self.delay_pad_to_die = delay;
    }

    /// `PAD::SetSimElectricalType()`.
    pub fn set_sim_electrical_type(&mut self, sim_type: crate::padstack::PadSimElectricalType) {
        self.sim_electrical_type = sim_type;
    }

    /// `PAD::SetPadstack( aPadstack )`: `PADSTACK::operator=` (primitives re-parented to this
    /// pad).
    pub fn set_padstack(&mut self, padstack: &Padstack) {
        let parent = self.self_parent_ref();
        self.padstack.assign_from(padstack, parent);
    }

    /// `PAD::ImportSettingsFrom( aMasterPad )`: `env` is this pad's environment, `ctx` the
    /// board the master pad belongs to (clone-edit-store, `board-model.md` 8.2).
    pub fn import_settings_from(&mut self, env: &MutEnv, ctx: &BoardCtx<'_>, master: &Pad) {
        self.set_padstack(&master.padstack);
        // Layer Set should be updated before calling SetAttribute()
        self.set_layer_set(env, master.layer_set(ctx));
        self.set_attribute(master.attribute());
        // Unfortunately, SetAttribute() can change m_layerMask.
        // Be sure we keep the original mask by calling SetLayerSet() after SetAttribute()
        self.set_layer_set(env, master.layer_set(ctx));
        self.set_property(master.property());

        // Must be after setting attribute and layerSet
        if !self.can_have_number() {
            self.set_number("");
        }

        // I am not sure the m_LengthPadToDie should be imported, because this is a parameter
        // really specific to a given pad (JPC).

        // The pad orientation, for historical reasons is the pad rotation + parent rotation.
        let mut pad_rot = master.orientation(ctx);

        if let Some(master_fp) = master.parent_footprint(ctx) {
            pad_rot -= master_fp.orientation();
        }

        if let Some(xform) = Self::env_transform(env) {
            pad_rot += xform.rotate();
        }

        self.set_orientation(env, pad_rot);

        for layer in self.padstack.unique_layers() {
            // Ensure that circles are circles
            // Stay in the library frame, since a board-frame round trip under a scale rounds
            // the size
            if master.shape(layer) == PadShape::Circle {
                let x = self.padstack.size(layer).x;
                self.set_lib_size(layer, Vector2I::new(x, x));
            }
        }

        match master.attribute() {
            PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent => {
                // These pads have no hole, or any kind of drilling.
                remove_all_drills_from_padstack(&mut self.padstack);
            }
            _ => {}
        }

        // copy also local settings:
        self.set_local_clearance(master.local_clearance());
        self.set_local_solder_mask_margin(master.local_solder_mask_margin());
        self.set_local_solder_paste_margin(master.local_solder_paste_margin());
        self.set_local_solder_paste_margin_ratio(master.local_solder_paste_margin_ratio());

        self.set_local_zone_connection(master.local_zone_connection());
        self.set_local_thermal_spoke_width_override(master.local_thermal_spoke_width_override());
        self.set_thermal_spoke_angle(master.thermal_spoke_angle());
        self.set_local_thermal_gap_override(master.local_thermal_gap());

        self.set_custom_shape_in_zone_opt(master.custom_shape_in_zone_opt());

        self.base.teardrop_params = master.base.teardrop_params.clone();

        self.set_dirty();
    }

    // ---- clearances and overrides -------------------------------------------------------

    /// `PAD::GetLocalClearance()`: the padstack clearance on F_Cu.
    pub fn local_clearance(&self) -> Option<i32> {
        self.padstack.clearance(PcbLayerId::FCu)
    }

    /// `PAD::GetLocalClearance( aSource )`: with the source text "pad".
    pub fn local_clearance_with_source(&self, want_source: bool) -> Option<(i32, Option<String>)> {
        let clearance = self.padstack.clearance(PcbLayerId::FCu)?;
        Some((clearance, want_source.then(|| "pad".to_string())))
    }

    /// `PAD::SetLocalClearance()`.
    pub fn set_local_clearance(&mut self, clearance: Option<i32>) {
        *self.padstack.clearance_mut(PcbLayerId::FCu) = clearance;
    }

    /// `PAD::GetLocalSolderMaskMargin()`.
    pub fn local_solder_mask_margin(&self) -> Option<i32> {
        self.padstack.solder_mask_margin(PcbLayerId::FCu)
    }

    /// `PAD::SetLocalSolderMaskMargin()`.
    pub fn set_local_solder_mask_margin(&mut self, margin: Option<i32>) {
        *self.padstack.solder_mask_margin_mut(PcbLayerId::FCu) = margin;
    }

    /// `PAD::GetLocalSolderPasteMargin()`.
    pub fn local_solder_paste_margin(&self) -> Option<i32> {
        self.padstack.solder_paste_margin(PcbLayerId::FCu)
    }

    /// `PAD::SetLocalSolderPasteMargin()`.
    pub fn set_local_solder_paste_margin(&mut self, margin: Option<i32>) {
        *self.padstack.solder_paste_margin_mut(PcbLayerId::FCu) = margin;
    }

    /// `PAD::GetLocalSolderPasteMarginRatio()`.
    pub fn local_solder_paste_margin_ratio(&self) -> Option<f64> {
        self.padstack.solder_paste_margin_ratio(PcbLayerId::FCu)
    }

    /// `PAD::SetLocalSolderPasteMarginRatio()`.
    pub fn set_local_solder_paste_margin_ratio(&mut self, ratio: Option<f64>) {
        *self.padstack.solder_paste_margin_ratio_mut(PcbLayerId::FCu) = ratio;
    }

    /// `PAD::GetLocalZoneConnection()`: `INHERITED` when unset.
    pub fn local_zone_connection(&self) -> ZoneConnection {
        self.padstack
            .zone_connection(PcbLayerId::FCu)
            .unwrap_or(ZoneConnection::Inherited)
    }

    /// `PAD::SetLocalZoneConnection()`.
    pub fn set_local_zone_connection(&mut self, connection: ZoneConnection) {
        *self.padstack.zone_connection_mut(PcbLayerId::FCu) = Some(connection);
    }

    /// `PAD::GetLocalThermalSpokeWidthOverride()`.
    pub fn local_thermal_spoke_width_override(&self) -> Option<i32> {
        self.padstack.thermal_spoke_width(PcbLayerId::FCu)
    }

    /// `PAD::SetLocalThermalSpokeWidthOverride()`.
    pub fn set_local_thermal_spoke_width_override(&mut self, width: Option<i32>) {
        *self.padstack.thermal_spoke_width_mut(PcbLayerId::FCu) = width;
    }

    /// `PAD::SetThermalSpokeAngle()`.
    pub fn set_thermal_spoke_angle(&mut self, angle: EdaAngle) {
        self.padstack
            .set_thermal_spoke_angle(angle, PcbLayerId::FCu);
    }

    /// `PAD::GetThermalSpokeAngleDegrees()`.
    pub fn thermal_spoke_angle_degrees(&self) -> f64 {
        self.padstack
            .thermal_spoke_angle(PcbLayerId::FCu)
            .as_degrees()
    }

    /// `PAD::GetThermalGap()`: 0 when unset.
    pub fn thermal_gap(&self) -> i32 {
        self.padstack.thermal_gap(PcbLayerId::FCu).unwrap_or(0)
    }

    /// `PAD::SetThermalGap()`.
    pub fn set_thermal_gap(&mut self, gap: i32) {
        *self.padstack.thermal_gap_mut(PcbLayerId::FCu) = Some(gap);
    }

    /// `PAD::GetLocalThermalGapOverride()` (the `std::optional` overload).
    pub fn local_thermal_gap(&self) -> Option<i32> {
        self.padstack.thermal_gap(PcbLayerId::FCu)
    }

    /// `PAD::SetLocalThermalGapOverride()`.
    pub fn set_local_thermal_gap_override(&mut self, gap: Option<i32>) {
        *self.padstack.thermal_gap_mut(PcbLayerId::FCu) = gap;
    }

    /// `PAD::GetOwnClearance( aLayer, aSource )`: `ctx.rules`, 0 without rules (the
    /// `BOARD_CONNECTED_ITEM` body; the pad passes itself as `ctx.view( AnyItem::Pad )` so that
    /// a detached copy works too). "The NPTH vs regular pad logic is handled in
    /// DRC_ENGINE::GetCachedOwnClearance".
    pub fn own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        if crate::board_item::has_board(self, ctx) {
            if let Some(rules) = ctx.rules {
                let local = *ctx;
                let this = local.view(AnyItem::Pad(self));
                return rules.cached_own_clearance(&local, this, layer, want_source);
            }
        }

        (0, None)
    }

    /// `PAD::GetClearanceOverrides( aSource )`: pad, then footprint local clearance
    /// (`FOOTPRINT::GetClearanceOverrides` = `GetLocalClearance( aSource )`, source "footprint
    /// <reference>").
    pub fn clearance_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> Option<(i32, Option<String>)> {
        if self.padstack.clearance(PcbLayerId::FCu).is_some() {
            return self.local_clearance_with_source(want_source);
        }

        if let Some(parent_footprint) = self.parent_footprint(ctx) {
            let clearance = parent_footprint.local_clearance()?;
            let source =
                want_source.then(|| format!("footprint {}", parent_footprint.reference(ctx)));
            return Some((clearance, source));
        }

        None
    }

    /// `PAD::GetSolderMaskExpansion( aLayer )`.
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        // Pads defined only on mask layers (and perhaps on other tech layers) use the shape
        // defined by the pad settings only.  ALL other pads, even those that don't actually
        // have any copper (such as NPTH pads with holes the same size as the pad) get mask
        // expansion.
        let mut copper = self.padstack.layer_set.clone();
        copper &= Lset::all_cu_mask();

        if copper.none() {
            return 0;
        }

        let layer = if is_front_layer(layer) {
            PcbLayerId::FMask
        } else if is_back_layer(layer) {
            PcbLayerId::BMask
        } else {
            return 0;
        };

        let mut margin: Option<i32> = None;
        let has_board = crate::board_item::has_board(self, ctx);
        let rules = ctx.rules.filter(|r| {
            has_board
                && r.has_rules_for_constraint_type(DrcConstraintT::SolderMaskExpansionConstraint)
        });

        if let Some(drc_engine) = rules {
            let local = *ctx;
            let this = local.view(AnyItem::Pad(self));
            let constraint = drc_engine.eval_rules(
                &local,
                DrcConstraintT::SolderMaskExpansionConstraint,
                Some(this),
                None,
                layer,
            );

            if constraint.value.has_opt() {
                margin = Some(constraint.value.opt());
            }
        } else {
            margin = self.padstack.solder_mask_margin(layer);

            if margin.is_none() {
                if let Some(parent_footprint) = self.parent_footprint(ctx) {
                    margin = parent_footprint.solder_mask_margin;
                }
            }

            if margin.is_none() && has_board {
                margin = Some(ctx.board.design_settings().solder_mask_expansion);
            }
        }

        let mut margin_value = margin.unwrap_or(0);

        let cu_layer = if layer == PcbLayerId::BMask {
            PcbLayerId::BCu
        } else {
            PcbLayerId::FCu
        };

        // ensure mask have a size always >= 0
        if margin_value < 0 {
            let size = self.padstack.size(cu_layer);
            let minsize = -size.x.min(size.y) / 2;

            if margin_value < minsize {
                margin_value = minsize;
            }
        }

        margin_value
    }

    /// `PAD::GetSolderPasteMargin( aLayer )`.
    pub fn solder_paste_margin(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        // Pads defined only on mask layers (and perhaps on other tech layers) use the shape
        // defined by the pad settings only.  ALL other pads, even those that don't actually
        // have any copper (such as NPTH pads with holes the same size as the pad) get paste
        // expansion.
        let mut copper = self.padstack.layer_set.clone();
        copper &= Lset::all_cu_mask();

        if copper.none() {
            return Vector2I::new(0, 0);
        }

        let layer = if is_front_layer(layer) {
            PcbLayerId::FPaste
        } else if is_back_layer(layer) {
            PcbLayerId::BPaste
        } else {
            return Vector2I::new(0, 0);
        };

        let mut margin: Option<i32> = None;
        let mut mratio: Option<f64> = None;

        let has_board = crate::board_item::has_board(self, ctx);
        let drc_engine = if has_board { ctx.rules } else { None };

        let has_abs_rules = drc_engine.is_some_and(|e| {
            e.has_rules_for_constraint_type(DrcConstraintT::SolderPasteAbsMarginConstraint)
        });
        let has_rel_rules = drc_engine.is_some_and(|e| {
            e.has_rules_for_constraint_type(DrcConstraintT::SolderPasteRelMarginConstraint)
        });

        if let Some(drc_engine) = drc_engine.filter(|_| has_abs_rules || has_rel_rules) {
            let local = *ctx;

            if has_abs_rules {
                let constraint = drc_engine.eval_rules(
                    &local,
                    DrcConstraintT::SolderPasteAbsMarginConstraint,
                    Some(local.view(AnyItem::Pad(self))),
                    None,
                    layer,
                );

                if constraint.value.has_opt() {
                    margin = Some(constraint.value.opt());
                }
            }

            if has_rel_rules {
                let constraint = drc_engine.eval_rules(
                    &local,
                    DrcConstraintT::SolderPasteRelMarginConstraint,
                    Some(local.view(AnyItem::Pad(self))),
                    None,
                    layer,
                );

                if constraint.value.has_opt() {
                    mratio = Some(f64::from(constraint.value.opt()) / 1000.0);
                }
            }
        }

        if margin.is_none() {
            margin = self.padstack.solder_paste_margin(layer);

            if margin.is_none() {
                if let Some(parent_footprint) = self.parent_footprint(ctx) {
                    margin = parent_footprint.solder_paste_margin;
                }
            }

            if margin.is_none() && has_board {
                margin = Some(ctx.board.design_settings().solder_paste_margin);
            }
        }

        if mratio.is_none() {
            mratio = self.padstack.solder_paste_margin_ratio(layer);

            if mratio.is_none() {
                if let Some(parent_footprint) = self.parent_footprint(ctx) {
                    mratio = parent_footprint.solder_paste_margin_ratio;
                }
            }

            if mratio.is_none() && has_board {
                mratio = Some(ctx.board.design_settings().solder_paste_margin_ratio);
            }
        }

        let cu_layer = if layer == PcbLayerId::BPaste {
            PcbLayerId::BCu
        } else {
            PcbLayerId::FCu
        };
        let pad_size = self.size(ctx, cu_layer);

        let margin = margin.unwrap_or(0);
        let mratio = mratio.unwrap_or(0.0);
        let mut pad_margin = Vector2I::new(
            margin + ki_round(f64::from(pad_size.x) * mratio),
            margin + ki_round(f64::from(pad_size.y) * mratio),
        );

        // ensure paste have a size always >= 0
        if self.padstack.shape(layer) != PadShape::Custom {
            if pad_margin.x < -pad_size.x / 2 {
                pad_margin.x = -pad_size.x / 2;
            }

            if pad_margin.y < -pad_size.y / 2 {
                pad_margin.y = -pad_size.y / 2;
            }
        }

        pad_margin
    }

    /// `PAD::GetZoneConnectionOverrides( aSource )`: pad, then footprint
    /// (`FOOTPRINT::GetZoneConnectionOverrides`, source "footprint <reference>").
    pub fn zone_connection_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (ZoneConnection, Option<String>) {
        let mut connection = self
            .padstack
            .zone_connection(PcbLayerId::FCu)
            .unwrap_or(ZoneConnection::Inherited);
        let mut source = None;

        if connection != ZoneConnection::Inherited && want_source {
            source = Some("pad".to_string());
        }

        if connection == ZoneConnection::Inherited {
            if let Some(parent_footprint) = self.parent_footprint(ctx) {
                connection = parent_footprint.zone_connection;

                if connection != ZoneConnection::Inherited && want_source {
                    source = Some(format!("footprint {}", parent_footprint.reference(ctx)));
                }
            }
        }

        (connection, source)
    }

    /// `PAD::GetLocalThermalGapOverride( aSource )`.
    pub fn local_thermal_gap_override(&self, want_source: bool) -> (i32, Option<String>) {
        let gap = self.padstack.thermal_gap(PcbLayerId::FCu);
        let source = (gap.is_some() && want_source).then(|| "pad".to_string());

        (gap.unwrap_or(0), source)
    }

    /// `PAD::GetLocalSpokeWidthOverride( aSource )`.
    pub fn local_spoke_width_override(&self, want_source: bool) -> (i32, Option<String>) {
        let width = self.padstack.thermal_spoke_width(PcbLayerId::FCu);
        let source = (width.is_some() && want_source).then(|| "pad".to_string());

        (width.unwrap_or(0), source)
    }

    /// `PAD::GetThermalSpokeAngle()`.
    pub fn thermal_spoke_angle(&self) -> EdaAngle {
        self.padstack.thermal_spoke_angle(PcbLayerId::FCu)
    }

    // ---- transforms ---------------------------------------------------------------------

    /// `PAD::Move()`: `SetPosition( GetPosition() + aMoveVector )`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        let pos = self.position_env(env) + delta;
        self.set_position(env, pos);
    }

    /// `PAD::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        let mut new_pos = self.position_env(env);
        rotate_point_around(&mut new_pos, centre, angle);
        self.set_position(env, new_pos);

        let orientation = self.orientation_env(env) + angle;
        self.set_orientation(env, orientation);
    }

    /// `PAD::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        if let Some(xform) = Self::env_transform(env) {
            // Mirror in the footprint's library frame (rotation-independent).
            let lib_centre = xform.inverse_apply(centre);
            mirror_point(&mut self.lib_pos, lib_centre, dir);
        } else {
            let mut new_pos = self.position_env(env);
            mirror_point(&mut new_pos, centre, dir);
            self.set_position(env, new_pos);
        }

        for layer in self.padstack.unique_layers() {
            mirror_point(self.padstack.offset_mut(layer), Vector2I::new(0, 0), dir);
            mirror_point(
                self.padstack.trapezoid_delta_size_mut(layer),
                Vector2I::new(0, 0),
                dir,
            );
        }

        let fp_relative = -self.lib_orientation();
        self.set_fp_relative_orientation(env, fp_relative);

        self.mirror_chamfer_positions(dir);

        self.padstack.flip_layers(&env.layers);

        // Flip pads layers after padstack geometry
        let mut flipped = Lset::new();

        for layer in self.padstack.layer_set.iter() {
            flipped.set(env.layers.flip_layer(layer));
        }

        self.set_layer_set(env, flipped);

        // Flip the basic shapes, in custom pads
        self.flip_primitives(env, dir);

        self.set_dirty();
    }

    /// `PAD::FlipPrimitives()`: flip the custom-shape primitives about the pad origin.
    pub fn flip_primitives(&mut self, env: &MutEnv, dir: FlipDirection) {
        let parent = self.self_parent_ref();
        let prim_env = primitive_env(env);

        for layer in self.padstack.unique_layers() {
            for primitive in self.padstack.primitives_mut(layer) {
                // Ensure the primitive parent is up to date. Flip uses GetBoard() that
                // imply primitive parent is valid
                primitive.base.item.parent = parent;
                primitive.flip(&prim_env, Vector2I::new(0, 0), dir);
            }
        }

        self.set_dirty();
    }

    /// `PAD::Mirror( aCentre, aFlipDirection )` = `Mirror( MIRROR_AXIS( aCentre, dir ) )`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(env, &mirror_axis_seg(centre, dir));
    }

    /// `PAD::Mirror( const SEG& aAxis )`: unlike Flip(), a mirror keeps the pad on its side,
    /// so only the geometry moves, which is relative to the pad's own origin.
    pub fn mirror_seg(&mut self, env: &MutEnv, axis: &Seg) {
        let axis_angle = -EdaAngle::from_vector(axis.b - axis.a);

        let pos = axis.reflect_point(self.position_env(env));
        self.set_position(env, pos);
        let orientation = self.orientation_env(env).reflect(axis_angle);
        self.set_orientation(env, orientation);

        self.mirror_chamfer_positions(FlipDirection::TopBottom);

        let parent = self.self_parent_ref();
        let prim_env = primitive_env(env);

        for layer in self.padstack.unique_layers() {
            mirror_point(
                self.padstack.offset_mut(layer),
                Vector2I::new(0, 0),
                FlipDirection::TopBottom,
            );
            mirror_point(
                self.padstack.trapezoid_delta_size_mut(layer),
                Vector2I::new(0, 0),
                FlipDirection::TopBottom,
            );

            for primitive in self.padstack.primitives_mut(layer) {
                primitive.base.item.parent = parent;
                // primitive->Mirror( padXAxis ): PCB_SHAPE::Mirror( centre, dir ) is
                // Mirror( MIRROR_AXIS( centre, dir ) ), and the axis through (0, 0) along
                // (1, 0) is MIRROR_AXIS( (0, 0), TOP_BOTTOM ).
                primitive.mirror(&prim_env, Vector2I::new(0, 0), FlipDirection::TopBottom);
            }
        }

        self.set_dirty();
    }

    /// `PAD::mirrorChamferPositions( aFlipDirection )`.
    fn mirror_chamfer_positions(&mut self, dir: FlipDirection) {
        for layer in self.padstack.unique_layers() {
            let positions = self.padstack.chamfer_positions_mut(layer);

            if dir == FlipDirection::LeftRight {
                mirror_bit_flags(positions, RECT_CHAMFER_TOP_LEFT, RECT_CHAMFER_TOP_RIGHT);
                mirror_bit_flags(
                    positions,
                    RECT_CHAMFER_BOTTOM_LEFT,
                    RECT_CHAMFER_BOTTOM_RIGHT,
                );
            } else {
                mirror_bit_flags(positions, RECT_CHAMFER_TOP_LEFT, RECT_CHAMFER_BOTTOM_LEFT);
                mirror_bit_flags(positions, RECT_CHAMFER_TOP_RIGHT, RECT_CHAMFER_BOTTOM_RIGHT);
            }
        }
    }

    /// `PAD::SwapShapePositions( aLhs, aRhs, aActiveLayer )`: the new positions of the two
    /// pads, to be stored with `set_position` (clone-edit-store).
    pub fn swap_shape_positions(
        ctx: &BoardCtx<'_>,
        lhs: &Pad,
        rhs: &Pad,
        active_layer: PcbLayerId,
    ) -> (Vector2I, Vector2I) {
        // This routine favours the center of the visual copper over the center of the hole.
        // The downside is that said center is ill-defined when there is more than one shape
        // (ie: for complex padstackes).  In that case we swap based on the active layer's
        // copper shape, following the principle of least surprise.
        let lhs_shape_pos = lhs.shape_pos(ctx, active_layer);
        let rhs_shape_pos = rhs.shape_pos(ctx, active_layer);

        let mut lhs_offset = lhs.offset(ctx, active_layer);
        let mut rhs_offset = rhs.offset(ctx, active_layer);

        rotate_point(&mut lhs_offset, lhs.orientation(ctx));
        rotate_point(&mut rhs_offset, rhs.orientation(ctx));

        (rhs_shape_pos - lhs_offset, lhs_shape_pos - rhs_offset)
    }

    /// `PAD::OnFootprintTransformed()`: re-bake the board-frame orientation, mark dirty.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        let mut parent_orient = ANGLE_0;

        if let Some(xform) = Self::env_transform(env) {
            parent_orient = xform.rotate();
        }

        let orientation = self.lib_orientation() + parent_orient;
        self.padstack.set_orientation(orientation);
        self.set_dirty();
    }

    /// `PAD::OnFootprintRescaled()`: pad size, drill and offset auto-derive from lib-frame
    /// padstack values through the parent transform on read. Custom-shape primitives carry
    /// their own geometry in the pad frame and are excluded from the read-time transform
    /// (transformFp returns null for pad children), so rescale them explicitly
    /// (`rebakePrimitiveToFootprint`).
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        _ratio_x: f64,
        _ratio_y: f64,
        _linear_factor: f64,
        _anchor: Vector2I,
        _parent_rotate: EdaAngle,
    ) {
        if let Some(xform) = Self::env_transform(env) {
            let (local_sx, local_sy) =
                Self::primitive_lib_scale_in(Some(xform), self.lib_orientation());
            let prim_env = primitive_env(env);

            for layer in self.padstack.unique_layers() {
                for prim in self.padstack.primitives_mut(layer) {
                    prim.rebake_with_scale(&prim_env, local_sx, local_sy);
                }
            }
        }

        self.on_footprint_transformed(env);
    }

    // ---- descriptions and comparison ----------------------------------------------------

    /// `PAD::ShowPadShape( aShape )` (English; `_()` is not translated).
    pub fn show_pad_shape(shape: PadShape) -> &'static str {
        match shape {
            PadShape::Circle => "Circle",
            PadShape::Oval => "Oval",
            PadShape::Rectangle => "Rectangle",
            PadShape::Trapezoid => "Trapezoid",
            PadShape::Roundrect => "Rounded rectangle",
            PadShape::ChamferedRect => "Chamfered rectangle",
            PadShape::Custom => "Custom shape",
        }
    }

    /// `PAD::ShowLegacyPadShape( aLayer )`.
    pub fn show_legacy_pad_shape(&self, layer: PcbLayerId) -> &'static str {
        match self.shape(layer) {
            PadShape::Circle => "Circle",
            PadShape::Oval => "Oval",
            PadShape::Rectangle => "Rect",
            PadShape::Trapezoid => "Trap",
            PadShape::Roundrect => "Roundrect",
            PadShape::ChamferedRect => "Chamferedrect",
            PadShape::Custom => "CustomShape",
        }
    }

    /// `PAD::ShowPadAttr()`.
    pub fn show_pad_attr(&self) -> &'static str {
        match self.attribute {
            PadAttrib::Pth => "PTH",
            PadAttrib::Smd => "SMD",
            PadAttrib::Conn => "Conn",
            PadAttrib::Npth => "NPTH",
            PadAttrib::NonComponent => "Non-component",
        }
    }

    /// `PAD::GetItemDescription()` (English; `_()` is not translated).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let mut parent_fp = self.parent_footprint(ctx);

        // Don't report parent footprint info from footprint editor, viewer, etc.
        if crate::board_item::has_board(self, ctx) && ctx.board.board_use == BoardUse::FpHolder {
            parent_fp = None;
        }

        let is_smd_like = matches!(
            self.attribute,
            PadAttrib::Smd | PadAttrib::Conn | PadAttrib::NonComponent
        );

        if self.attribute == PadAttrib::Npth {
            match parent_fp {
                Some(fp) => format!("NPTH pad of {}", fp.reference(ctx)),
                None => "NPTH pad".to_string(),
            }
        } else if self.number.is_empty() {
            if is_smd_like {
                match parent_fp {
                    Some(fp) => format!(
                        "Pad {} of {} on {}",
                        crate::board_connected_item::netname_msg(self, ctx),
                        fp.reference(ctx),
                        crate::board_item::layer_mask_describe(self, ctx)
                    ),
                    None => format!(
                        "Pad on {}",
                        crate::board_item::layer_mask_describe(self, ctx)
                    ),
                }
            } else {
                match parent_fp {
                    Some(fp) => format!(
                        "PTH pad {} of {}",
                        crate::board_connected_item::netname_msg(self, ctx),
                        fp.reference(ctx)
                    ),
                    None => "PTH pad".to_string(),
                }
            }
        } else if is_smd_like {
            match parent_fp {
                Some(fp) => format!(
                    "Pad {} {} of {} on {}",
                    self.number,
                    crate::board_connected_item::netname_msg(self, ctx),
                    fp.reference(ctx),
                    crate::board_item::layer_mask_describe(self, ctx)
                ),
                None => format!(
                    "Pad {} on {}",
                    self.number,
                    crate::board_item::layer_mask_describe(self, ctx)
                ),
            }
        } else {
            match parent_fp {
                Some(fp) => format!(
                    "PTH pad {} {} of {}",
                    self.number,
                    crate::board_connected_item::netname_msg(self, ctx),
                    fp.reference(ctx)
                ),
                None => format!("PTH pad {}", self.number),
            }
        }
    }

    /// `PAD::Compare( aPadRef, aPadCmp )`: attribute, then `PADSTACK::Compare`.
    pub fn compare(_ctx: &BoardCtx<'_>, pad_ref: &Pad, pad_cmp: &Pad) -> i32 {
        let diff = pad_ref.attribute as i32 - pad_cmp.attribute as i32;

        if diff != 0 {
            return diff;
        }

        Padstack::compare(&pad_ref.padstack, &pad_cmp.padstack)
    }

    /// `PAD::operator==( const BOARD_ITEM& )`: same parent UUID (when both have a parent),
    /// then `PAD::operator==( const PAD& )` (padstack, position, attribute).
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Pad) -> bool {
        let parent_uuid = |parent: ParentRef| match parent {
            ParentRef::None => None,
            // The BOARD's own m_Uuid: the same object for both pads of one board.
            ParentRef::Board => Some(None),
            ParentRef::Item(id) => Some(ctx.item(id).map(|p| p.uuid())),
        };

        if let (Some(a), Some(b)) = (
            parent_uuid(self.base.item.parent),
            parent_uuid(other.base.item.parent),
        ) {
            if a != b {
                return false;
            }
        }

        if !self.padstack.eq_padstack(&other.padstack) {
            return false;
        }

        if self.position(ctx) != other.position(ctx) {
            return false;
        }

        if self.attribute != other.attribute {
            return false;
        }

        true
    }

    /// `PAD::Similarity( const BOARD_ITEM& )`.
    pub fn similarity(&self, ctx: &BoardCtx<'_>, other: &Pad) -> f64 {
        let parent_uuid = |parent: ParentRef| match parent {
            ParentRef::Item(id) => ctx.item(id).map(|p| p.uuid()),
            _ => None,
        };

        if parent_uuid(self.base.item.parent) != parent_uuid(other.base.item.parent) {
            return 0.0;
        }

        let mut similarity = 1.0;

        if self.position(ctx) != other.position(ctx) {
            similarity *= 0.9;
        }

        if self.attribute != other.attribute {
            similarity *= 0.9;
        }

        similarity *= self.padstack.similarity(&other.padstack);

        similarity
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Debug;

    use kicad_core::{Kiid, Lset, PCB_IU_SCALE, PCB_LAYER_ID_COUNT, PcbLayerId};
    use kicad_geom::geometry::transform_trs::TransformTrs;
    use kicad_geom::{EdaAngle, EdaOrientation, FlipDirection, Vector2I};

    use crate::board::Board;
    use crate::ctx::BoardCtx;
    use crate::footprint::Footprint;
    use crate::ids::{FootprintId, NetCode, PadId};
    use crate::item::{BoardUse, ParentRef};
    use crate::pad::Pad;
    use crate::padstack::{
        PadAttrib, PadDrillPostMachiningMode, PadDrillShape, PadProp, PadShape, Padstack,
        PadstackMode, UnconnectedLayerMode,
    };
    use crate::zone_settings::ZoneConnection;

    // ---- the pcb-dump value formats the golden uses (dump/format.rs is behind the `dump`
    // feature) -------------------------------------------------------------------------------

    fn v_int(v: impl Into<i64>) -> String {
        v.into().to_string()
    }

    fn v_bool(v: bool) -> String {
        v.to_string()
    }

    fn v_vec(v: Vector2I) -> String {
        format!("{},{}", v.x, v.y)
    }

    fn v_opt_int(v: Option<i32>) -> String {
        v.map_or_else(|| "none".to_string(), |v| v.to_string())
    }

    /// Angles and doubles are compared as numbers (`%.17g` round-trips): `f:` marks them.
    fn v_angle(a: EdaAngle) -> String {
        format!("f:{}", a.as_degrees())
    }

    fn v_layer(l: PcbLayerId) -> String {
        if l == PcbLayerId::UndefinedLayer {
            return "UNDEFINED_LAYER".to_string();
        }

        Lset::name(l)
    }

    /// `upper_snake` of the variant name (the dump's enum format for these enums).
    fn v_enum<E: Debug>(e: E) -> String {
        let name = format!("{e:?}");
        let chars: Vec<char> = name.chars().collect();
        let mut r = String::new();

        for (i, &c) in chars.iter().enumerate() {
            if i > 0 && c.is_ascii_uppercase() && chars[i - 1].is_ascii_lowercase() {
                r.push('_');
            }

            r.push(c.to_ascii_uppercase());
        }

        r
    }

    /// Compare a derived value with the golden text (numbers marked `f:` numerically).
    fn same(actual: &str, expected: &str) -> bool {
        if let Some(a) = actual.strip_prefix("f:") {
            let a: Vec<f64> = a.split(',').map(|x| x.parse().unwrap()).collect();
            let e: Vec<f64> = expected.split(',').map(|x| x.parse().unwrap()).collect();
            return a == e;
        }

        actual == expected
    }

    const GOLDEN: &str = include_str!("../../tests/golden/m14_pad_frames.txt");

    /// `SECTION|KEY|FIELD=VALUE` lines grouped by `(section, key)`.
    fn golden() -> BTreeMap<(String, String), BTreeMap<String, String>> {
        let mut map: BTreeMap<(String, String), BTreeMap<String, String>> = BTreeMap::new();

        for line in GOLDEN.lines() {
            let mut parts = line.splitn(3, '|');
            let (Some(section), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (field, value) = rest.split_once('=').expect("FIELD=VALUE");
            map.entry((section.to_string(), key.to_string()))
                .or_default()
                .insert(field.to_string(), value.to_string());
        }

        map
    }

    fn layer(name: &str) -> PcbLayerId {
        (-1..PCB_LAYER_ID_COUNT)
            .map(PcbLayerId)
            .find(|l| v_layer(*l) == name)
            .unwrap_or_else(|| panic!("unknown layer {name}"))
    }

    fn vec(s: &str) -> Vector2I {
        let (x, y) = s.split_once(',').expect("x,y");
        Vector2I::new(x.parse().unwrap(), y.parse().unwrap())
    }

    fn opt_i32(s: &str) -> Option<i32> {
        (s != "none").then(|| s.parse().unwrap())
    }

    fn opt_f64(s: &str) -> Option<f64> {
        (s != "none").then(|| s.parse().unwrap())
    }

    fn pick<E: Debug + Copy>(s: &str, candidates: &[E]) -> E {
        *candidates
            .iter()
            .find(|e| v_enum(**e) == s)
            .unwrap_or_else(|| panic!("unknown enum value {s}"))
    }

    const SHAPES: [PadShape; 7] = [
        PadShape::Circle,
        PadShape::Rectangle,
        PadShape::Oval,
        PadShape::Trapezoid,
        PadShape::Roundrect,
        PadShape::ChamferedRect,
        PadShape::Custom,
    ];
    const ZONE_CONNECTIONS: [ZoneConnection; 5] = [
        ZoneConnection::Inherited,
        ZoneConnection::None,
        ZoneConnection::Thermal,
        ZoneConnection::Full,
        ZoneConnection::ThtThermal,
    ];

    fn layer_names(s: &str) -> Lset {
        s.trim_matches(['[', ']'])
            .split(',')
            .filter(|n| !n.is_empty())
            .map(layer)
            .collect()
    }

    /// Insert a footprint on `board` as `board_with_footprint` does (test_support.rs).
    fn add_footprint(board: &mut Board, fields: &BTreeMap<String, String>) -> FootprintId {
        let (rot, scale) = (&fields["transform.rotate"], &fields["transform.scale"]);
        let (sx, sy) = scale.split_once(',').unwrap();
        let mut footprint = Footprint::default();
        footprint.base.uuid = Kiid::new(&mut board.kiid_generator);
        footprint.base.layer = layer(&fields["layer"]);
        footprint.transform = TransformTrs::new(
            vec(&fields["transform.translate"]),
            EdaAngle::from_degrees(rot.parse().unwrap()),
            sx.parse().unwrap(),
            sy.parse().unwrap(),
        );
        footprint.flipped = fields["flipped"] == "true";
        footprint.clearance = opt_i32(&fields["local_clearance"]);
        footprint.solder_mask_margin = opt_i32(&fields["local_solder_mask_margin"]);
        footprint.solder_paste_margin = opt_i32(&fields["local_solder_paste_margin"]);
        footprint.solder_paste_margin_ratio = opt_f64(&fields["local_solder_paste_margin_ratio"]);
        footprint.zone_connection = pick(&fields["local_zone_connection"], &ZONE_CONNECTIONS);

        let fp = board.insert_item(footprint, ParentRef::Board);
        board.footprint_list.push_back(fp);
        fp
    }

    /// The pad as `pcb-dump` saw its members (library frame, padstack as stored).
    fn pad_from_members(
        pad_fields: &BTreeMap<String, String>,
        item_fields: &BTreeMap<String, String>,
    ) -> Pad {
        let f = |k: &str| -> &str { &pad_fields[k] };
        let mut pad = Pad::default();
        pad.number = f("number").trim_matches('"').to_string();
        pad.pin_type = f("pin_type").trim_matches('"').to_string();
        pad.attribute = pick(
            f("attribute"),
            &[
                PadAttrib::Pth,
                PadAttrib::Smd,
                PadAttrib::Conn,
                PadAttrib::Npth,
                PadAttrib::NonComponent,
            ],
        );
        pad.property = pick(f("property"), &[PadProp::None, PadProp::Testpoint]);
        pad.lib_pos = vec(f("library_position"));
        pad.lib_orientation = EdaOrientation::new(EdaAngle::from_degrees(
            f("fp_relative_orientation").parse().unwrap(),
        ));
        pad.base.item.layer = layer(&item_fields["layer"]);
        pad.base.net = NetCode(item_fields["net"].parse().unwrap());

        let ps = &mut pad.padstack;
        ps.set_mode(pick(
            f("padstack.mode"),
            &[
                PadstackMode::Normal,
                PadstackMode::FrontInnerBack,
                PadstackMode::Custom,
            ],
        ));
        ps.set_layer_set(layer_names(f("padstack.layers")));
        ps.set_unconnected_layer_mode(pick(
            f("unconnected_layer_mode"),
            &[
                UnconnectedLayerMode::KeepAll,
                UnconnectedLayerMode::StartEndOnly,
                UnconnectedLayerMode::RemoveAll,
                UnconnectedLayerMode::RemoveExceptStartAndEnd,
            ],
        ));
        ps.drill_mut().size = vec(f("padstack.drill.size"));
        ps.drill_mut().shape = pick(
            f("padstack.drill.shape"),
            &[
                PadDrillShape::Undefined,
                PadDrillShape::Circle,
                PadDrillShape::Oblong,
            ],
        );
        ps.drill_mut().start = layer(f("padstack.drill.start"));
        ps.drill_mut().end = layer(f("padstack.drill.end"));

        for (side, props) in [
            ("front_outer", PcbLayerId::FCu),
            ("back_outer", PcbLayerId::BCu),
        ] {
            *ps.solder_mask_margin_mut(props) =
                opt_i32(f(&format!("padstack.{side}.solder_mask_margin")));
            *ps.solder_paste_margin_mut(props) =
                opt_i32(f(&format!("padstack.{side}.solder_paste_margin")));
            *ps.solder_paste_margin_ratio_mut(props) =
                opt_f64(f(&format!("padstack.{side}.solder_paste_margin_ratio")));
        }

        let copper_layers: Vec<String> = pad_fields
            .keys()
            .filter_map(|k| k.strip_prefix("padstack.copper."))
            .filter_map(|k| k.strip_suffix(".shape"))
            .map(str::to_string)
            .collect();

        for name in copper_layers {
            let g =
                |field: &str| -> &str { &pad_fields[&format!("padstack.copper.{name}.{field}")] };
            let props = ps.copper_props_mut(layer(&name));
            props.shape.shape = pick(g("shape"), &SHAPES);
            props.shape.anchor_shape = pick(g("anchor_shape"), &SHAPES);
            props.shape.size = vec(g("size"));
            props.shape.offset = vec(g("offset"));
            props.shape.round_rect_radius_ratio = g("round_rect_radius_ratio").parse().unwrap();
            props.shape.chamfered_rect_ratio = g("chamfered_rect_ratio").parse().unwrap();
            props.shape.chamfered_rect_positions = g("chamfered_rect_positions").parse().unwrap();
            props.shape.trapezoid_delta_size = vec(g("trapezoid_delta_size"));
            let zc = pick(g("zone_connection"), &ZONE_CONNECTIONS);
            props.zone_connection = (zc != ZoneConnection::Inherited).then_some(zc);
            props.thermal_spoke_width = opt_i32(g("thermal_spoke_width"));
            props.thermal_spoke_angle = Some(EdaOrientation::new(EdaAngle::from_degrees(
                g("thermal_spoke_angle").parse().unwrap(),
            )));
            props.thermal_gap = opt_i32(g("thermal_gap"));
            props.clearance = opt_i32(g("clearance"));
        }

        pad
    }

    /// The derived values of `pad`, keyed like the `pcb-dump` fields.
    fn derived(ctx: &BoardCtx<'_>, pad: &Pad, field: &str) -> Option<String> {
        let suffix_layer = |prefix: &str| field.strip_prefix(prefix).map(layer);

        if let Some(l) = suffix_layer("solder_mask_expansion.") {
            return Some(v_int(pad.solder_mask_expansion(ctx, l)));
        }
        if let Some(l) = suffix_layer("solder_paste_margin.") {
            return Some(v_vec(pad.solder_paste_margin(ctx, l)));
        }
        if let Some(l) = suffix_layer("on_layer.") {
            return Some(v_bool(pad.is_on_layer(ctx, l)));
        }
        if let Some(l) = suffix_layer("flash.") {
            return Some(v_bool(pad.flash_layer(ctx, l)));
        }
        if let Some(l) = suffix_layer("flash_permitted.") {
            return Some(v_bool(pad.flash_layer_permitted(ctx, l)));
        }
        if let Some(rest) = field.strip_prefix("layer.") {
            let (name, what) = rest.rsplit_once('.').unwrap();
            let l = layer(name);
            return Some(match what {
                "shape" => v_enum(pad.shape(l)),
                "size" => v_vec(pad.size(ctx, l)),
                "offset" => v_vec(pad.offset(ctx, l)),
                "delta" => v_vec(pad.delta(l)),
                "round_rect_corner_radius" => v_int(pad.round_rect_corner_radius(l)),
                _ => return None,
            });
        }

        Some(match field {
            "position" => v_vec(pad.position(ctx)),
            "orientation" => v_angle(pad.orientation(ctx)),
            "fp_relative_orientation" => v_angle(pad.fp_relative_orientation(ctx)),
            "primitive_lib_scale" => {
                let (sx, sy) = pad.primitive_lib_scale(ctx);
                format!("f:{sx},{sy}")
            }
            "flipped" => v_bool(pad.is_flipped(ctx)),
            "aperture" => v_bool(pad.is_aperture_pad()),
            "npth_no_copper" => v_bool(pad.is_npth_with_no_copper(ctx)),
            "no_connect" => v_bool(pad.is_no_connect_pad()),
            "free" => v_bool(pad.is_free_pad(ctx)),
            "can_have_number" => v_bool(pad.can_have_number()),
            "principal_layer" => v_layer(pad.principal_layer(ctx)),
            "drill_size" => v_vec(pad.drill_size(ctx)),
            "local_zone_connection" => v_enum(pad.local_zone_connection()),
            "zone_connection_overrides" => v_enum(pad.zone_connection_overrides(ctx, false).0),
            "clearance_overrides" => v_opt_int(pad.clearance_overrides(ctx, false).map(|c| c.0)),
            "local_thermal_spoke_width" => v_opt_int(pad.local_thermal_spoke_width_override()),
            "local_spoke_width_override" => v_int(pad.local_spoke_width_override(false).0),
            "thermal_spoke_angle" => v_angle(pad.thermal_spoke_angle()),
            "thermal_gap" => v_int(pad.thermal_gap()),
            "local_thermal_gap" => v_opt_int(pad.local_thermal_gap()),
            "item.layer" => v_layer(pad.layer(ctx)),
            "item.fp_relative_position" => v_vec(pad.fp_relative_position(ctx)),
            "item.locked" => v_bool(pad.is_locked(ctx)),
            "item.on_copper" => v_bool(pad.is_on_copper_layer(ctx)),
            "item.has_hole" => v_bool(pad.has_hole(ctx)),
            "item.has_drilled_hole" => v_bool(pad.has_drilled_hole(ctx)),
            _ => return None,
        })
    }

    /// Package M14 differential test: the pads of `tests/oracle/m14_pad_frames.kicad_pcb`
    /// (rotated, flipped and non-uniformly scaled footprints; NORMAL, FRONT_INNER_BACK and
    /// CUSTOM padstacks; NPTH, SMD, connector and aperture pads; local and footprint
    /// overrides) rebuilt from their dumped members, every derived value compared with the
    /// reference `pcb-dump` (`tests/oracle/m14_pad_golden.sh`). Connectivity is `None`, which
    /// equals the reference here (no tracks, no zones: conditionally flashed layers are not
    /// connected).
    #[test]
    fn oracle_pad_frames() {
        let golden = golden();
        let mut board = Board::new();
        board.set_copper_layer_count(4);
        {
            // (setup (pad_to_mask_clearance 0.05) (pad_to_paste_clearance -0.03)
            //        (pad_to_paste_clearance_ratio -0.1))
            let bds = board.design_settings_mut();
            bds.solder_mask_expansion = PCB_IU_SCALE.mm_to_iu(0.05);
            bds.solder_paste_margin = PCB_IU_SCALE.mm_to_iu(-0.03);
            bds.solder_paste_margin_ratio = -0.1;
        }
        board.find_or_add_net("GND", None);
        board.find_or_add_net("unconnected-(U1-Pad3)", None);

        let mut footprints: BTreeMap<String, FootprintId> = BTreeMap::new();

        for ((section, key), fields) in &golden {
            if section == "footprint" {
                footprints.insert(key.clone(), add_footprint(&mut board, fields));
            }
        }

        let mut pads: Vec<(String, PadId)> = Vec::new();

        for ((section, key), fields) in &golden {
            if section != "pad" {
                continue;
            }

            let item_fields = &golden[&("item".to_string(), key.clone())];
            let parent = item_fields
                .get("parent")
                .cloned()
                .unwrap_or_else(|| fp_of_pad(key));
            let fp = footprints[&parent];
            let pad = pad_from_members(fields, item_fields);
            let id = board.insert_item(pad, ParentRef::Item(fp.into()));
            board.footprints.get_mut(fp).unwrap().pads.push_back(id);
            pads.push((key.clone(), id));
        }

        assert_eq!(pads.len(), 15);
        board.check_invariants().unwrap();

        let ctx = BoardCtx::bare(&board);
        let mut checked = 0;
        let mut mismatches = Vec::new();

        for (key, id) in &pads {
            let pad = &board[*id];

            for (section, prefix) in [("pad", ""), ("item", "item.")] {
                for (field, expected) in &golden[&(section.to_string(), key.clone())] {
                    let Some(actual) = derived(&ctx, pad, &format!("{prefix}{field}")) else {
                        continue;
                    };

                    checked += 1;

                    if !same(&actual, expected) {
                        mismatches.push(format!("{section}|{key}|{field}: {actual} != {expected}"));
                    }
                }
            }
        }

        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert!(checked > 700, "only {checked} values compared");
    }

    /// The footprint of a pad of the oracle board (the golden keeps no `item|parent` line):
    /// pad UUIDs a1.. / b1.. / c1.. belong to footprints a0.. / b0.. / c0...
    fn fp_of_pad(pad: &str) -> String {
        format!("{}0000000-0000-0000-0000-000000000001", &pad[..1])
    }

    // ---- qa/tests/pcbnew/test_pad_setters.cpp -------------------------------------------

    /// A footprint holder board with one unrotated footprint (`std::make_unique<FOOTPRINT>(
    /// &board )`) and a pad of it as a local value (`std::make_unique<PAD>( fp.get() )`).
    fn holder_pad() -> (Board, FootprintId, Pad) {
        let mut board = Board::new();
        board.board_use = BoardUse::FpHolder;
        let fp = board.insert_item(Footprint::default(), ParentRef::Board);
        board.footprint_list.push_back(fp);
        let mut pad = Pad::default();
        pad.base.item.parent = ParentRef::Item(fp.into());
        (board, fp, pad)
    }

    fn mm(v: f64) -> i32 {
        PCB_IU_SCALE.mm_to_iu(v)
    }

    #[test]
    fn set_size_x_on_circle_pad_updates_y() {
        let (board, fp, mut pad) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.padstack.set_mode(PadstackMode::Normal);
        pad.set_attribute(PadAttrib::Pth);
        pad.set_layer_set(&env, Pad::pth_mask());
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Circle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(1.0)));
        pad.set_size_x_prop(&env, mm(1.5));
        let ctx = BoardCtx::bare(&board);
        assert_eq!(
            pad.size(&ctx, Padstack::ALL_LAYERS),
            Vector2I::new(mm(1.5), mm(1.5))
        );
    }

    #[test]
    fn set_size_y_on_circle_pad_updates_x() {
        let (board, fp, mut pad) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.set_attribute(PadAttrib::Pth);
        pad.set_layer_set(&env, Pad::pth_mask());
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Circle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(1.0)));
        pad.set_size_y_prop(&env, mm(1.5));
        let ctx = BoardCtx::bare(&board);
        assert_eq!(
            pad.size(&ctx, Padstack::ALL_LAYERS),
            Vector2I::new(mm(1.5), mm(1.5))
        );
    }

    #[test]
    fn set_size_x_on_rect_pad_leaves_y_alone() {
        let (board, fp, mut pad) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.set_attribute(PadAttrib::Smd);
        pad.set_layer_set(&env, Lset::from_layers(&[PcbLayerId::FCu]));
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Rectangle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(0.5)));
        pad.set_size_x_prop(&env, mm(1.5));
        let ctx = BoardCtx::bare(&board);
        assert_eq!(
            pad.size(&ctx, Padstack::ALL_LAYERS),
            Vector2I::new(mm(1.5), mm(0.5))
        );
    }

    #[test]
    fn set_attribute_clears_all_drilling() {
        let (board, fp, mut pad) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        pad.set_attribute(PadAttrib::Pth);
        pad.set_layer_set(&env, Pad::pth_mask());
        pad.set_drill_size(&env, Vector2I::new(mm(1.0), mm(1.0)));
        pad.padstack.set_backdrill_size(true, Some(mm(1.2)));
        pad.padstack
            .set_backdrill_end_layer(true, PcbLayerId::In1Cu);
        pad.padstack.front_post_machining_mut().mode = Some(PadDrillPostMachiningMode::Counterbore);
        pad.padstack.front_post_machining_mut().size = mm(1.5);
        pad.set_attribute(PadAttrib::Smd);

        let ctx = BoardCtx::bare(&board);
        assert_eq!(pad.drill_size(&ctx), Vector2I::new(0, 0));
        assert_eq!(pad.padstack.secondary_drill().size.x, 0);
        assert!(pad.padstack.front_post_machining().mode.is_none());
        assert_eq!(pad.padstack.front_post_machining().size, 0);
    }

    // ---- qa/tests/pcbnew/test_pad_swap_positions.cpp ------------------------------------

    /// `makeRoundPad( aParent, aPos, aOffset, aOrient )`.
    fn make_round_pad(
        board: &Board,
        fp: FootprintId,
        pos: Vector2I,
        offset: Vector2I,
        orient: EdaAngle,
    ) -> Pad {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pad = Pad::default();
        pad.base.item.parent = ParentRef::Item(fp.into());
        pad.padstack.set_mode(PadstackMode::Normal);
        pad.set_attribute(PadAttrib::Pth);
        pad.set_shape(Padstack::ALL_LAYERS, PadShape::Circle);
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(2.0), mm(2.0)));
        pad.set_drill_size(&env, Vector2I::new(mm(0.8), mm(0.8)));
        pad.set_orientation(&env, orient);
        pad.set_position(&env, pos);
        pad.set_offset(&env, Padstack::ALL_LAYERS, offset);
        pad
    }

    /// `PAD::SwapShapePositions( aLhs, aRhs, aLayer )` applied to the two pads.
    fn swap(board: &Board, fp: FootprintId, a: &mut Pad, b: &mut Pad, layer: PcbLayerId) {
        let env = board.child_env(ParentRef::Item(fp.into()));
        let ctx = BoardCtx::bare(board);
        let (pos_a, pos_b) = Pad::swap_shape_positions(&ctx, a, b, layer);
        a.set_position(&env, pos_a);
        b.set_position(&env, pos_b);
    }

    fn swap_case(
        pos_a: Vector2I,
        off_a: Vector2I,
        rot_a: f64,
        pos_b: Vector2I,
        off_b: Vector2I,
        rot_b: f64,
    ) {
        let (board, fp, _) = holder_pad();
        let mut a = make_round_pad(&board, fp, pos_a, off_a, EdaAngle::from_degrees(rot_a));
        let mut b = make_round_pad(&board, fp, pos_b, off_b, EdaAngle::from_degrees(rot_b));
        let ctx = BoardCtx::bare(&board);
        let (shape_a, shape_b) = (
            a.shape_pos(&ctx, PcbLayerId::FCu),
            b.shape_pos(&ctx, PcbLayerId::FCu),
        );
        swap(&board, fp, &mut a, &mut b, PcbLayerId::FCu);
        let ctx = BoardCtx::bare(&board);
        assert_eq!(a.shape_pos(&ctx, PcbLayerId::FCu), shape_b);
        assert_eq!(b.shape_pos(&ctx, PcbLayerId::FCu), shape_a);
        assert_eq!(a.offset(&ctx, PcbLayerId::FCu), off_a);
        assert_eq!(b.offset(&ctx, PcbLayerId::FCu), off_b);
    }

    #[test]
    fn swap_pads_without_offset() {
        swap_case(
            Vector2I::new(mm(-1.5), 0),
            Vector2I::new(0, 0),
            0.0,
            Vector2I::new(0, 0),
            Vector2I::new(0, 0),
            0.0,
        );
    }

    #[test]
    fn swap_pads_when_one_has_shape_offset() {
        swap_case(
            Vector2I::new(mm(-1.5), mm(0.6)),
            Vector2I::new(mm(0.6), 0),
            90.0,
            Vector2I::new(0, 0),
            Vector2I::new(0, 0),
            0.0,
        );
    }

    #[test]
    fn swap_pads_with_different_offsets() {
        swap_case(
            Vector2I::new(mm(-2.0), mm(0.5)),
            Vector2I::new(mm(0.3), mm(0.1)),
            45.0,
            Vector2I::new(mm(2.0), mm(-0.5)),
            Vector2I::new(mm(-0.2), mm(0.4)),
            -30.0,
        );
    }

    fn involutive_case(flip: bool) {
        let (board, fp, _) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        let off_a = Vector2I::new(mm(0.6), 0);
        let mut a = make_round_pad(
            &board,
            fp,
            Vector2I::new(mm(-1.5), mm(0.6)),
            off_a,
            EdaAngle::from_degrees(90.0),
        );
        let mut b = make_round_pad(
            &board,
            fp,
            Vector2I::new(0, 0),
            Vector2I::new(0, 0),
            EdaAngle::from_degrees(0.0),
        );
        let layer = if flip {
            let ctx = BoardCtx::bare(&board);
            let (pa, pb) = (a.position(&ctx), b.position(&ctx));
            a.flip(&env, pa, FlipDirection::LeftRight);
            b.flip(&env, pb, FlipDirection::LeftRight);
            PcbLayerId::BCu
        } else {
            PcbLayerId::FCu
        };
        let ctx = BoardCtx::bare(&board);
        let (orig_a, orig_b) = (a.position(&ctx), b.position(&ctx));
        swap(&board, fp, &mut a, &mut b, layer);
        swap(&board, fp, &mut a, &mut b, layer);
        let ctx = BoardCtx::bare(&board);
        assert_eq!(a.position(&ctx), orig_a);
        assert_eq!(b.position(&ctx), orig_b);
    }

    #[test]
    fn swap_is_involutive_for_two_pads() {
        involutive_case(false);
    }

    #[test]
    fn swap_is_involutive_for_two_pads_swapped_on_back_layer() {
        involutive_case(true);
    }

    // ---- qa/tests/pcbnew/test_pad_flashing.cpp (connectivity-free part) -----------------

    /// `PadsInSameFootprintDoNotForceInnerLayerFlashing` / `...DifferentFootprints...`
    /// without a connectivity query (`ctx.connectivity == None`, the C++ empty
    /// CONNECTIVITY_DATA): the start / end layers flash through the unconnected-layer mode,
    /// an inner layer asks connectivity and is not flashed. With the real connectivity the
    /// C++ result is the same (the two pads do not force flashing); that part needs
    /// kicad-connectivity.
    #[test]
    fn pads_do_not_force_inner_layer_flashing_without_connectivity() {
        let (mut board, fp, _) = holder_pad();
        let net = board.find_or_add_net("P1", Some(NetCode(1)));
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pad = Pad::default();
        pad.base.item.parent = ParentRef::Item(fp.into());
        pad.set_attribute(PadAttrib::Pth);
        pad.padstack.set_mode(PadstackMode::Normal);
        pad.set_layer_set(&env, Lset::all_cu_mask().clone());
        pad.set_size(&env, Padstack::ALL_LAYERS, Vector2I::new(mm(1.0), mm(1.0)));
        pad.set_drill_size(&env, Vector2I::new(mm(0.5), mm(0.5)));
        pad.set_position(&env, Vector2I::new(0, 0));
        pad.set_unconnected_layer_mode(UnconnectedLayerMode::RemoveExceptStartAndEnd);
        pad.base.net = net;
        let id = board.insert_item(pad, ParentRef::Item(fp.into()));
        board.footprints.get_mut(fp).unwrap().pads.push_back(id);

        let ctx = BoardCtx::bare(&board);
        let pad = &board[id];
        assert!(pad.flash_layer(&ctx, PcbLayerId::FCu));
        assert!(pad.flash_layer(&ctx, PcbLayerId::BCu));
        assert!(!pad.flash_layer(&ctx, PcbLayerId::In1Cu));
        assert!(pad.flash_layer_permitted(&ctx, PcbLayerId::In1Cu));
        // GAL layers and UNDEFINED_LAYER are always "flashed".
        assert!(pad.flash_layer(&ctx, PcbLayerId::UndefinedLayer));
        assert!(pad.flash_layer(&ctx, PcbLayerId(PCB_LAYER_ID_COUNT + 1)));
    }

    // ---- frame round trips and mutators (rotated / flipped / scaled footprint) ----------

    #[test]
    fn frame_setters_round_trip_lib_values() {
        let mut fx = crate::test_support::rotated_flipped_scaled();
        let id = fx.child(crate::ids::ItemKind::Pad).typed::<Pad>().unwrap();
        let env = fx.board.mut_env(id.into());
        let mut pad = fx.board[id].clone();
        pad.lib_pos = Vector2I::new(1_000_000, -2_000_000);
        pad.set_fp_relative_orientation(&env, EdaAngle::from_degrees(20.0));
        // padstack orientation = lib + parent (30 degrees).
        assert_eq!(pad.padstack.orientation(), EdaAngle::from_degrees(50.0));
        fx.set_child(id, pad);

        let ctx = BoardCtx::bare(&fx.board);
        let pad = fx.board[id].clone();
        let xform = crate::test_support::rotated_scaled_transform();
        assert_eq!(
            pad.position(&ctx),
            xform.apply(Vector2I::new(1_000_000, -2_000_000))
        );
        assert_eq!(pad.orientation(&ctx), EdaAngle::from_degrees(50.0));
        assert_eq!(
            pad.fp_relative_position(&ctx),
            xform.inverse_apply(pad.position(&ctx))
        );

        // SetOrientation keeps the library orientation relative to the parent.
        let mut moved = pad.clone();
        moved.set_orientation(&env, EdaAngle::from_degrees(100.0));
        assert_eq!(moved.lib_orientation(), EdaAngle::from_degrees(70.0));
        moved.on_footprint_transformed(&env);
        assert_eq!(moved.padstack.orientation(), EdaAngle::from_degrees(100.0));

        // Rotating by a full turn in four steps comes back to the same library frame.
        let centre = Vector2I::new(3_000_000, 4_000_000);
        for _ in 0..4 {
            moved.rotate(&env, centre, EdaAngle::from_degrees(90.0));
        }
        assert_eq!(moved.lib_orientation(), EdaAngle::from_degrees(70.0));

        // Board-frame size of a rect pad: lib size scaled in the pad frame.
        let (sx, sy) = pad.primitive_lib_scale(&ctx);
        let lib = pad.padstack.size(PcbLayerId::FCu);
        let mut rect = pad.clone();
        rect.padstack
            .set_shape(PadShape::Rectangle, PcbLayerId::FCu);
        assert_eq!(
            rect.size(&ctx, PcbLayerId::FCu),
            Vector2I::new(
                kicad_geom::ki_round(f64::from(lib.x) * sx),
                kicad_geom::ki_round(f64::from(lib.y) * sy)
            )
        );
        // A circle uses the mean scale (1.5 + 0.75) / 2.
        assert_eq!(
            pad.size(&ctx, PcbLayerId::FCu).x,
            kicad_geom::ki_round(f64::from(lib.x) * 1.125)
        );
        fx.board.check_invariants().unwrap();
    }

    #[test]
    fn flip_mirrors_lib_frame_layers_and_chamfers() {
        let (board, fp, _) = holder_pad();
        let env = board.child_env(ParentRef::Item(fp.into()));
        let mut pad = Pad::default();
        pad.base.item.parent = ParentRef::Item(fp.into());
        pad.set_attribute(PadAttrib::Smd);
        pad.set_layer_set(&env, Pad::smd_mask());
        pad.set_shape(PcbLayerId::FCu, PadShape::ChamferedRect);
        pad.set_chamfer_positions(PcbLayerId::FCu, 1 | 8); // top left, bottom right
        pad.set_position(&env, Vector2I::new(1_000, 2_000));
        pad.set_lib_offset(PcbLayerId::FCu, Vector2I::new(100, 200));
        pad.set_fp_relative_orientation(&env, EdaAngle::from_degrees(30.0));
        pad.flip(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);

        assert_eq!(pad.lib_pos(), Vector2I::new(-1_000, 2_000));
        assert_eq!(
            pad.padstack.offset(PcbLayerId::FCu),
            Vector2I::new(-100, 200)
        );
        assert_eq!(pad.lib_orientation(), EdaAngle::from_degrees(330.0));
        assert_eq!(pad.chamfer_positions(PcbLayerId::FCu), 2 | 4);
        assert_eq!(
            pad.padstack.layer_set,
            Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::BPaste, PcbLayerId::BMask])
        );
        assert_eq!(pad.base.item.layer, PcbLayerId::BCu);

        // Mirror keeps the side and flips the geometry top / bottom about the pad origin.
        let mut mirrored = pad.clone();
        mirrored.mirror(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);
        assert_eq!(mirrored.lib_pos(), Vector2I::new(1_000, 2_000));
        assert_eq!(
            mirrored.padstack.offset(PcbLayerId::FCu),
            Vector2I::new(-100, -200)
        );
        assert_eq!(mirrored.chamfer_positions(PcbLayerId::FCu), 8 | 1);
        assert_eq!(mirrored.padstack.layer_set, pad.padstack.layer_set);
    }

    #[test]
    fn attribute_trims_layers_and_npth_drops_number_and_net() {
        let mut pad = Pad::default();
        pad.set_number("4");
        pad.base.net = NetCode(3);
        pad.set_attribute(PadAttrib::Smd);
        // PTHMask has F_Cu and B_Cu: B_Cu wins.
        assert_eq!(pad.base.item.layer, PcbLayerId::BCu);
        let mut copper = pad.padstack.layer_set.clone();
        copper &= Lset::all_cu_mask();
        assert_eq!(copper, Lset::from_layers(&[PcbLayerId::BCu]));
        assert_eq!(pad.padstack.drill().size, Vector2I::new(0, 0));

        pad.set_attribute(PadAttrib::Npth);
        assert!(pad.number().is_empty());
        assert_eq!(pad.base.net, NetCode::UNCONNECTED);
        assert!(!pad.can_have_number());
    }

    #[test]
    fn compare_and_equality() {
        let (board, _, mut a) = holder_pad();
        let ctx = BoardCtx::bare(&board);
        let b = a.clone();
        assert_eq!(Pad::compare(&ctx, &a, &b), 0);
        assert!(a.eq_item(&ctx, &b));
        a.attribute = PadAttrib::Smd;
        assert_eq!(
            Pad::compare(&ctx, &a, &b),
            PadAttrib::Smd as i32 - PadAttrib::Pth as i32
        );
        assert!(!a.eq_item(&ctx, &b));
        assert_eq!(a.similarity(&ctx, &b), 0.9);
    }

    #[test]
    fn zone_layer_override_writers() {
        let mut fx = crate::test_support::rotated_flipped_scaled();
        let id = fx.child(crate::ids::ItemKind::Pad);
        let stamp = fx.board.time_stamp();
        fx.board.set_zone_layer_override(
            id,
            PcbLayerId::In2Cu,
            crate::pcb_track::ZoneLayerOverride::ForceFlashed,
        );
        let pad_id = id.typed::<Pad>().unwrap();
        assert_eq!(
            fx.board[pad_id].zone_layer_override(PcbLayerId::In2Cu),
            crate::pcb_track::ZoneLayerOverride::ForceFlashed
        );
        fx.board.set_zone_layer_override(
            id,
            PcbLayerId::BCu,
            crate::pcb_track::ZoneLayerOverride::ForceNoZoneConnection,
        );
        fx.board.clear_zone_layer_overrides(id);
        // LAYER_RANGE( F_Cu, B_Cu, BoardCopperLayerCount() ) of a two-layer board: the inner
        // entry is kept, as C++.
        assert_eq!(
            fx.board[pad_id].zone_layer_override(PcbLayerId::BCu),
            crate::pcb_track::ZoneLayerOverride::None
        );
        assert_eq!(fx.board.copper_layer_count(), 2);
        assert_eq!(
            fx.board[pad_id].zone_layer_override(PcbLayerId::In2Cu),
            crate::pcb_track::ZoneLayerOverride::ForceFlashed
        );
        assert_eq!(fx.board.time_stamp(), stamp);

        // A forced flash makes a conditionally flashed inner layer flash without connectivity.
        let mut pad = fx.board[pad_id].clone();
        pad.set_unconnected_layer_mode(UnconnectedLayerMode::RemoveAll);
        fx.set_child(pad_id, pad);
        fx.board.set_zone_layer_override(
            id,
            PcbLayerId::In1Cu,
            crate::pcb_track::ZoneLayerOverride::ForceFlashed,
        );
        let ctx = BoardCtx::bare(&fx.board);
        assert!(fx.board[pad_id].flash_layer(&ctx, PcbLayerId::In1Cu));
        assert!(!fx.board[pad_id].flash_layer(&ctx, PcbLayerId::In3Cu));
    }

    #[test]
    fn item_description_texts() {
        let (board, _, mut pad) = holder_pad();
        let ctx = BoardCtx::bare(&board);
        let units =
            kicad_core::units_provider::UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        pad.set_number("7");
        // Footprint holder board: no parent footprint info.
        assert_eq!(pad.item_description(&ctx, &units, true), "PTH pad 7");
        pad.set_attribute(PadAttrib::Npth);
        assert_eq!(pad.item_description(&ctx, &units, true), "NPTH pad");
    }
}
