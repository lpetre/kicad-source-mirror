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

//! Port of the `PCB_VIA` part of `pcbnew/pcb_track.cpp` (layers, tenting / covering /
//! plugging / capping / filling, backdrill, post-machining, FlashLayer, ValidateViaParameters).
//! Package M17; M0a wrote the signatures. Methods named `PCB_TRACK::X (PCB_VIA)` are the
//! `PCB_TRACK` bodies as they run for a via. Via shapes are in `shapes.rs`.

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Lset, MinOptMax, PcbLayerId};
use kicad_geom::{Box2I, EdaAngle, FlipDirection, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::pcb_track::{PcbVia, Viatype};

impl PcbVia {
    /// `PCB_VIA::GetWidth()` (the override without a layer argument): `wxCHECK_MSG( false, .. )`
    /// answering with the `PADSTACK::ALL_LAYERS` size. Use `width( layer )`.
    pub fn width_no_layer(&self) -> i32 {
        crate::port_pending!("PCB_VIA::GetWidth()")
    }

    /// `PCB_VIA::SetWidth( aWidth )` (the override without a layer argument): sets the
    /// `PADSTACK::ALL_LAYERS` size and asserts. Use `set_width( layer, width )`.
    pub fn set_width_no_layer(&mut self, width: i32) {
        crate::port_pending!("PCB_VIA::SetWidth( aWidth )", width)
    }

    /// `PCB_VIA::GetWidthConstraint( aSource )`: `VIA_DIAMETER_CONSTRAINT` merged over the
    /// via's layer range.
    pub fn width_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        crate::port_pending!("PCB_VIA::GetWidthConstraint", ctx, want_source)
    }

    /// `PCB_VIA::GetPosition()`: `m_Start`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.track.start
    }

    /// `PCB_VIA::SetPosition()`: `m_Start = m_End = aPoint`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.track.start = pos;
        self.track.end = pos;
    }

    /// `PCB_TRACK::Move()` (inherited).
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.track.start += delta;
        self.track.end += delta;
    }

    /// `PCB_VIA::IsOnCopperLayer()`: always true.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `PCB_VIA::HasHole()`: always true.
    pub fn has_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `PCB_VIA::HasDrilledHole()`: through, blind or buried.
    pub fn has_drilled_hole(&self, _ctx: &BoardCtx<'_>) -> bool {
        matches!(
            self.via_type,
            Viatype::Through | Viatype::Blind | Viatype::Buried
        )
    }

    /// `PCB_VIA::GetLayer()`: `Padstack().Drill().start`.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        crate::port_pending!("PCB_VIA::GetLayer", ctx)
    }

    /// `PCB_VIA::SetLayer()`: `Padstack().Drill().start = aLayer`.
    pub fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        crate::port_pending!("PCB_VIA::SetLayer", env, layer)
    }

    /// `PCB_VIA::GetLayerSet`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PCB_VIA::GetLayerSet", ctx)
    }

    /// `PCB_VIA::SetLayerSet`.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("PCB_VIA::SetLayerSet", env, layers)
    }

    /// `PCB_VIA::IsOnLayer`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PCB_VIA::IsOnLayer", ctx, layer)
    }

    /// `PCB_VIA::FitsEnabledLayers`.
    pub fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        crate::port_pending!(
            "PCB_VIA::FitsEnabledLayers",
            ctx,
            enabled,
            copper_layer_count
        )
    }

    /// `PCB_VIA::GetBoundingBox`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_VIA::GetBoundingBox", ctx)
    }

    /// `PCB_VIA::IsTented`.
    pub fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PCB_VIA::IsTented", ctx, layer)
    }

    /// `PCB_VIA::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_VIA::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_VIA::HitTest (rect)`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_VIA::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `PCB_VIA::GetItemDescription`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_VIA::GetItemDescription", ctx, units, full)
    }

    /// `PCB_TRACK::GetFriendlyName (PCB_VIA)`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_TRACK::GetFriendlyName (PCB_VIA)", ctx)
    }

    /// `PCB_VIA::LayerMaskDescribe`.
    pub fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_VIA::LayerMaskDescribe", ctx)
    }

    /// `PCB_TRACK::Rotate (PCB_VIA)`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_TRACK::Rotate (PCB_VIA)", env, centre, angle)
    }

    /// `PCB_VIA::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_VIA::Flip", env, centre, dir)
    }

    /// `PCB_TRACK::Mirror (PCB_VIA)`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_TRACK::Mirror (PCB_VIA)", env, centre, dir)
    }
}
