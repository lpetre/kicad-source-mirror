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

//! Port of the `PCB_ARC` part of `pcbnew/pcb_track.cpp` (arc geometry and arc shapes).
//! Package M16; M0a wrote the signatures. Methods named `PCB_TRACK::X (PCB_ARC)` are the
//! `PCB_TRACK` bodies as they run for an arc (`Type() == PCB_ARC_T`).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapePolySet, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::pcb_track::PcbArc;
use crate::traits::Flashing;

impl PcbArc {
    /// `PCB_ARC::SetPosition()`: `m_Start = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.track.start = pos;
    }

    /// `PCB_ARC::Move()`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.track.start += delta;
        self.mid += delta;
        self.track.end += delta;
    }

    /// `PCB_TRACK::IsOnCopperLayer()`: always true.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// `PCB_ARC::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `PCB_ARC::GetPosition()`: the arc centre.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_ARC::GetPosition", ctx)
    }

    /// `PCB_TRACK::GetBoundingBox (PCB_ARC)`.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_TRACK::GetBoundingBox (PCB_ARC)", ctx)
    }

    /// `PCB_ARC::GetEffectiveShape`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        crate::port_pending!("PCB_ARC::GetEffectiveShape", ctx, layer, flash, usage)
    }

    /// `PCB_TRACK::TransformShapeToPolygon (PCB_ARC)`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        crate::port_pending!(
            "PCB_TRACK::TransformShapeToPolygon (PCB_ARC)",
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width
        )
    }

    /// `PCB_ARC::HitTest`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        crate::port_pending!("PCB_ARC::HitTest", ctx, pos, accuracy)
    }

    /// `PCB_ARC::HitTest (rect)`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        crate::port_pending!("PCB_ARC::HitTest (rect)", ctx, rect, contained, accuracy)
    }

    /// `PCB_TRACK::GetItemDescription (PCB_ARC)`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_TRACK::GetItemDescription (PCB_ARC)", ctx, units, full)
    }

    /// `PCB_ARC::Rotate`.
    pub fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        crate::port_pending!("PCB_ARC::Rotate", env, centre, angle)
    }

    /// `PCB_ARC::Flip`.
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_ARC::Flip", env, centre, dir)
    }

    /// `PCB_ARC::Mirror`.
    pub fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        crate::port_pending!("PCB_ARC::Mirror", env, centre, dir)
    }

    /// `PCB_TRACK::IsOnLayer (PCB_ARC)`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("PCB_TRACK::IsOnLayer (PCB_ARC)", ctx, layer)
    }

    /// `PCB_TRACK::GetLayerSet (PCB_ARC)`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PCB_TRACK::GetLayerSet (PCB_ARC)", ctx)
    }

    /// `PCB_TRACK::SetLayerSet (PCB_ARC)`.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        crate::port_pending!("PCB_TRACK::SetLayerSet (PCB_ARC)", env, layers)
    }

    /// `PCB_TRACK::GetFriendlyName (PCB_ARC)`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("PCB_TRACK::GetFriendlyName (PCB_ARC)", ctx)
    }
}
