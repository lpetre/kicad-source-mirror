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

//! Port of the board-frame derivation part of `pcbnew/pad.cpp`: frames, layers, attributes,
//! `FlashLayer`, clearance / mask / paste / zone-connection overrides, transforms and
//! `OnFootprint*` (package M14). Inherent methods of `Pad`; the ones that back a trait method
//! have exactly the trait's signature (rule C1, `board-model.md` 6.3).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::{EdaAngle, FlipDirection, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::footprint::Footprint;
use crate::pad::Pad;
use crate::zone_settings::ZoneConnection;

impl Pad {
    // ---- relations ----------------------------------------------------------------------

    /// `GetParentFootprint()` (a pad's parent is its footprint or nothing).
    pub fn parent_footprint<'a>(&self, ctx: &BoardCtx<'a>) -> Option<&'a Footprint> {
        let id = self.base.item.parent.item()?.typed::<Footprint>()?;
        ctx.board.footprint(id)
    }

    // ---- frames (board-model.md 5) ------------------------------------------------------

    /// `PAD::GetPosition()`: `m_libPos` through the parent footprint's transform.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PAD::GetPosition", ctx)
    }

    /// `PAD::SetPosition()`: stores the library-frame position of `pos`.
    pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PAD::SetPosition", env, pos)
    }

    /// `PAD::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `GetFPRelativePosition()`.
    pub fn fp_relative_position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PAD::GetFPRelativePosition", ctx)
    }

    /// `SetFPRelativePosition()`.
    pub fn set_fp_relative_position(&mut self, env: &MutEnv, pos: Vector2I) {
        crate::port_pending!("PAD::SetFPRelativePosition", env, pos)
    }

    /// `PAD::GetOrientation()`: the board-frame orientation.
    pub fn orientation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PAD::GetOrientation", ctx)
    }

    /// `PAD::SetOrientation()`: library orientation from the board angle, re-bakes
    /// `padstack.orientation`, marks the shapes dirty.
    pub fn set_orientation(&mut self, env: &MutEnv, angle: EdaAngle) {
        crate::port_pending!("PAD::SetOrientation", env, angle)
    }

    /// `PAD::GetFPRelativeOrientation()`.
    pub fn fp_relative_orientation(&self, ctx: &BoardCtx<'_>) -> EdaAngle {
        crate::port_pending!("PAD::GetFPRelativeOrientation", ctx)
    }

    /// `PAD::SetFPRelativeOrientation()`.
    pub fn set_fp_relative_orientation(&mut self, env: &MutEnv, angle: EdaAngle) {
        crate::port_pending!("PAD::SetFPRelativeOrientation", env, angle)
    }

    /// `PAD::GetSize( aLayer )`: the board-frame size of the shape on `layer`.
    pub fn size(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        crate::port_pending!("PAD::GetSize", ctx, layer)
    }

    /// `PAD::SetSize( aLayer, aSize )`: board-frame size, stored in the library frame.
    pub fn set_size(&mut self, env: &MutEnv, layer: PcbLayerId, size: Vector2I) {
        crate::port_pending!("PAD::SetSize", env, layer, size)
    }

    /// `PAD::SetLibSize( aLayer, aSize )` (parser).
    pub fn set_lib_size(&mut self, layer: PcbLayerId, size: Vector2I) {
        crate::port_pending!("PAD::SetLibSize", layer, size)
    }

    /// `PAD::GetOffset( aLayer )`: board-frame offset.
    pub fn offset(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        crate::port_pending!("PAD::GetOffset", ctx, layer)
    }

    /// `PAD::SetOffset( aLayer, aOffset )`.
    pub fn set_offset(&mut self, env: &MutEnv, layer: PcbLayerId, offset: Vector2I) {
        crate::port_pending!("PAD::SetOffset", env, layer, offset)
    }

    /// `PAD::SetLibOffset( aLayer, aOffset )` (parser).
    pub fn set_lib_offset(&mut self, layer: PcbLayerId, offset: Vector2I) {
        crate::port_pending!("PAD::SetLibOffset", layer, offset)
    }

    /// `PAD::ShapePos( aLayer )`: position of the shape centre (position + rotated offset).
    pub fn shape_pos(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        crate::port_pending!("PAD::ShapePos", ctx, layer)
    }

    /// `PAD::GetDrillSize()` (= `GetPrimaryDrillSize()`): board-frame drill size.
    pub fn drill_size(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PAD::GetPrimaryDrillSize", ctx)
    }

    /// `PAD::SetDrillSize()` (= `SetPrimaryDrillSize()`).
    pub fn set_drill_size(&mut self, env: &MutEnv, size: Vector2I) {
        crate::port_pending!("PAD::SetPrimaryDrillSize", env, size)
    }

    /// `PAD::SetLibDrillSize()` (parser).
    pub fn set_lib_drill_size(&mut self, size: Vector2I) {
        crate::port_pending!("PAD::SetLibDrillSize", size)
    }

    /// `PAD::GetPrimitiveLibScale()`: the (x, y) scale that re-bakes pad-frame primitives.
    pub fn primitive_lib_scale(&self, ctx: &BoardCtx<'_>) -> (f64, f64) {
        crate::port_pending!("PAD::GetPrimitiveLibScale", ctx)
    }

    // ---- layers -------------------------------------------------------------------------

    /// `PAD::GetLayer()`: the principal layer.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        crate::port_pending!("PAD::GetLayer", ctx)
    }

    /// `PAD::GetLayerSet()`: the padstack layer set, flipped with the footprint.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PAD::GetLayerSet", ctx)
    }

    /// `PAD::SetLayerSet()`.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("PAD::SetLayerSet", env, layers)
    }

    /// `PAD::IsOnLayer()`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PAD::IsOnLayer", ctx, layer)
    }

    /// `PAD::IsOnCopperLayer()`.
    pub fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("PAD::IsOnCopperLayer", ctx)
    }

    /// `PAD::FlashLayer( aLayer )`: whether the pad has copper on `layer` (asks
    /// `ctx.connectivity` and the zone-layer overrides for conditionally flashed pads).
    pub fn flash_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PAD::FlashLayer", ctx, layer)
    }

    /// `PAD::FlashLayer( aLayer, aOnlyCheckIfPermitted = true )`.
    pub fn flash_layer_permitted(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PAD::FlashLayer (only check if permitted)", ctx, layer)
    }

    /// `PAD::FlashLayer( const LSET& aLayers )`: true if flashed on any of `layers`.
    pub fn flash_layer_set(&self, ctx: &BoardCtx<'_>, layers: &Lset) -> bool {
        crate::port_pending!("PAD::FlashLayer (LSET)", ctx, layers)
    }

    /// `PAD::GetPrincipalLayer()`.
    pub fn principal_layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        crate::port_pending!("PAD::GetPrincipalLayer", ctx)
    }

    /// `PAD::IsAperturePad()`: no copper layer in the padstack layer set.
    pub fn is_aperture_pad(&self) -> bool {
        crate::port_pending!("PAD::IsAperturePad")
    }

    /// `PAD::CanHaveNumber()`.
    pub fn can_have_number(&self) -> bool {
        crate::port_pending!("PAD::CanHaveNumber")
    }

    /// `PAD::IsNoConnectPad()`.
    pub fn is_no_connect_pad(&self) -> bool {
        crate::port_pending!("PAD::IsNoConnectPad")
    }

    /// `PAD::IsFreePad()`.
    pub fn is_free_pad(&self) -> bool {
        crate::port_pending!("PAD::IsFreePad")
    }

    // ---- state / attributes -------------------------------------------------------------

    /// `PAD::IsLocked()`: parent group, footprint holder, footprint or own lock.
    pub fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("PAD::IsLocked", ctx)
    }

    /// `PAD::HasHole()`: `GetDrillSize().x > 0 && GetDrillSize().y > 0`.
    pub fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("PAD::HasHole", ctx)
    }

    /// `PAD::HasDrilledHole()`: `HasHole()` with a round hole.
    pub fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        crate::port_pending!("PAD::HasDrilledHole", ctx)
    }

    /// `PAD::SetAttribute()`: also resets the layer set to the attribute's default mask when
    /// the attribute changes, as C++.
    pub fn set_attribute(&mut self, attribute: crate::padstack::PadAttrib) {
        crate::port_pending!("PAD::SetAttribute", attribute)
    }

    // ---- clearances and overrides -------------------------------------------------------

    /// `PAD::GetLocalClearance()`: the padstack clearance on F_Cu.
    pub fn local_clearance(&self) -> Option<i32> {
        crate::port_pending!("PAD::GetLocalClearance")
    }

    /// `PAD::GetOwnClearance( aLayer, aSource )`: `ctx.rules`, 0 without rules.
    pub fn own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        crate::port_pending!("PAD::GetOwnClearance", ctx, layer, want_source)
    }

    /// `PAD::GetClearanceOverrides( aSource )`: pad, then footprint local clearance.
    pub fn clearance_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> Option<(i32, Option<String>)> {
        crate::port_pending!("PAD::GetClearanceOverrides", ctx, want_source)
    }

    /// `PAD::GetSolderMaskExpansion( aLayer )`.
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        crate::port_pending!("PAD::GetSolderMaskExpansion", ctx, layer)
    }

    /// `PAD::GetSolderPasteMargin( aLayer )`.
    pub fn solder_paste_margin(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Vector2I {
        crate::port_pending!("PAD::GetSolderPasteMargin", ctx, layer)
    }

    /// `PAD::GetZoneConnectionOverrides( aSource )`.
    pub fn zone_connection_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (ZoneConnection, Option<String>) {
        crate::port_pending!("PAD::GetZoneConnectionOverrides", ctx, want_source)
    }

    /// `PAD::GetLocalThermalGapOverride( aSource )`.
    pub fn local_thermal_gap_override(&self, want_source: bool) -> (i32, Option<String>) {
        crate::port_pending!("PAD::GetLocalThermalGapOverride", want_source)
    }

    /// `PAD::GetLocalSpokeWidthOverride( aSource )`.
    pub fn local_spoke_width_override(&self, want_source: bool) -> (i32, Option<String>) {
        crate::port_pending!("PAD::GetLocalSpokeWidthOverride", want_source)
    }

    /// `PAD::GetThermalSpokeAngle()`.
    pub fn thermal_spoke_angle(&self) -> EdaAngle {
        crate::port_pending!("PAD::GetThermalSpokeAngle")
    }

    // ---- transforms ---------------------------------------------------------------------

    /// `PAD::Move()`.
    pub fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        crate::port_pending!("PAD::Move", env, delta)
    }

    /// `PAD::Rotate()`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PAD::Rotate", env, centre, angle)
    }

    /// `PAD::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PAD::Flip", env, centre, dir)
    }

    /// `PAD::Mirror()`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PAD::Mirror", env, centre, dir)
    }

    /// `PAD::OnFootprintTransformed()`: re-bake the board-frame orientation, mark dirty.
    pub fn on_footprint_transformed(&mut self, env: &MutEnv) {
        crate::port_pending!("PAD::OnFootprintTransformed", env)
    }

    /// `PAD::OnFootprintRescaled()`.
    pub fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        crate::port_pending!(
            "PAD::OnFootprintRescaled",
            env,
            ratio_x,
            ratio_y,
            linear_factor,
            anchor,
            parent_rotate
        )
    }

    // ---- descriptions and comparison ----------------------------------------------------

    /// `PAD::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PAD::GetItemDescription", ctx, units, full)
    }

    /// `PAD::Compare( aPadRef, aPadCmp )`.
    pub fn compare(ctx: &BoardCtx<'_>, pad_ref: &Pad, pad_cmp: &Pad) -> i32 {
        crate::port_pending!("PAD::Compare", ctx, pad_ref, pad_cmp)
    }

    /// `PAD::operator==( const BOARD_ITEM& )`.
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Pad) -> bool {
        crate::port_pending!("PAD::operator==", ctx, other)
    }
}
