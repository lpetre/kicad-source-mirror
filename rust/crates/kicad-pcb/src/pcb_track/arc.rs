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

use std::sync::Arc;

use kicad_core::layer_ids::is_solder_mask_layer;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaDataType, KiidGenerator, Lset, MinOptMax, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::transform_arc_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{
    ANGLE_0, ANGLE_360, Box2I, EdaAngle, FlipDirection, Seg, ShapeArc, ShapePolySet, ShapeSegment,
    Vector2D, Vector2I, Vector2L, calc_arc_center, is_straight_arc, rotate_point_around,
};

use crate::board_connected_item;
use crate::board_item::{self, mirror_axis};
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::SharedShape;
use crate::pcb_track::{PcbArc, PcbTrack, Track};
use crate::traits::{BoardItemMut, Flashing};

impl PcbArc {
    /// Port of `PCB_ARC( aParent, const SHAPE_ARC* aArc )`: start, end and mid point of the
    /// arc, a fresh UUID (the parent is set when the item is added to a board).
    pub fn from_shape_arc(kiid_generator: &mut KiidGenerator, arc: &ShapeArc) -> Self {
        let mut a = PcbArc::new(kiid_generator);
        a.track.start = arc.get_p0();
        a.track.end = arc.get_p1();
        a.mid = arc.get_arc_mid();
        a
    }

    /// Port of `PCB_ARC::CreateFromThreePoints`: a straight `PCB_TRACK` when the three points
    /// are (nearly) collinear (`IsStraightArc`), else a `PCB_ARC`; both with a fresh UUID.
    pub fn create_from_three_points(
        kiid_generator: &mut KiidGenerator,
        start: Vector2I,
        mid: Vector2I,
        end: Vector2I,
    ) -> Track {
        if is_straight_arc(start, mid, end) {
            let mut track = PcbTrack::new(kiid_generator);

            track.set_start(start);
            track.set_end(end);
            return Track::Trace(track);
        }

        let mut arc = PcbArc::new(kiid_generator);

        arc.track.set_start(start);
        arc.set_mid(mid);
        arc.track.set_end(end);
        Track::Arc(arc)
    }

    /// `SetMid()`.
    pub fn set_mid(&mut self, mid: Vector2I) {
        self.mid = mid;
    }

    /// `GetMid()`.
    pub fn mid(&self) -> Vector2I {
        self.mid
    }

    /// `PCB_ARC::GetLength()`: `GetRadius() * std::abs( GetAngle().AsRadians() )`.
    pub fn length(&self) -> f64 {
        self.radius() * self.angle().as_radians().abs()
    }

    /// `PCB_TRACK::GetWidthConstraint (PCB_ARC)`: the `PCB_TRACK` body with the arc as `this`.
    pub fn width_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        super::width_constraint_of(&self.track, ctx, want_source, || Track::Arc(self.clone()))
    }

    /// `PCB_TRACK::GetSolderMaskExpansion (PCB_ARC)`: the `PCB_TRACK` body with the arc as
    /// `this` (the rule resolver sees an arc).
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>) -> i32 {
        super::solder_mask_expansion_of(&self.track, ctx, || Track::Arc(self.clone()))
    }

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

    /// `PCB_ARC::GetPosition()`: the arc centre, `CalcArcCenter( m_Start, m_Mid, m_End )`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.arc_center()
    }

    /// `CalcArcCenter( m_Start, m_Mid, m_End )` (the body of `PCB_ARC::GetPosition`, which
    /// needs no context).
    fn arc_center(&self) -> Vector2I {
        calc_arc_center(self.track.start, self.mid, self.track.end)
    }

    /// Port of `PCB_ARC::GetRadius`: the distance centre - start, clamped at `INT_MAX / 2`.
    pub fn radius(&self) -> f64 {
        let center = calc_arc_center(self.track.start, self.mid, self.track.end);
        let dist = center.distance(self.track.start);
        let max = i32::MAX as f64 / 2.0;

        // std::min( a, b ): b < a ? b : a
        if max < dist { max } else { dist }
    }

    /// Port of `PCB_ARC::GetAngle`: the signed central angle start -> mid -> end, each half
    /// normalised to (-180, 180].
    pub fn angle(&self) -> EdaAngle {
        let center = Vector2D::from(self.arc_center());
        let mut angle1 = EdaAngle::from_vector(sub_d(self.mid, center))
            - EdaAngle::from_vector(sub_d(self.track.start, center));
        let mut angle2 = EdaAngle::from_vector(sub_d(self.track.end, center))
            - EdaAngle::from_vector(sub_d(self.mid, center));

        angle1.normalize180() + angle2.normalize180()
    }

    /// Port of `PCB_ARC::GetArcAngleStart`: the angle of the start point seen from the
    /// centre, normalised to [0, 360).
    pub fn arc_angle_start(&self) -> EdaAngle {
        let pos = Vector2D::from(self.arc_center());
        let mut angle_start = EdaAngle::from_vector(sub_d(self.track.start, pos));

        angle_start.normalize()
    }

    /// Port of `PCB_ARC::GetArcAngleEnd`: the angle of the end point seen from the centre,
    /// normalised to [0, 360).
    pub fn arc_angle_end(&self) -> EdaAngle {
        let pos = Vector2D::from(self.arc_center());
        let mut angle_end = EdaAngle::from_vector(sub_d(self.track.end, pos));

        angle_end.normalize()
    }

    /// Port of `PCB_ARC::IsCCW`: `( m_End - m_Start ).Cross( m_Mid - m_Start ) < 0` in 64 bits.
    pub fn is_ccw(&self) -> bool {
        let start = Vector2L::from(self.track.start);
        let start_end = Vector2L::from(self.track.end) - start;
        let start_mid = Vector2L::from(self.mid) - start;

        start_end.cross(start_mid) < 0
    }

    /// Port of `PCB_ARC::IsDegenerated( aThreshold )` (C++ default threshold 5): the radius
    /// overflows an int, or both chords start-mid and mid-end are shorter than `threshold`.
    pub fn is_degenerated(&self, threshold: i32) -> bool {
        // We have lots of code that will blow up if the radius overflows an int.
        if self.radius() >= i32::MAX as f64 / 2.0 {
            return true;
        }

        // Too small arcs cannot be really handled: arc center (and arc radius)
        // cannot be safely computed if the distance between mid and end points
        // is too small (a few internal units)

        // len of both segments must be < aThreshold to be a very small degenerated arc
        (self.mid - self.track.start).euclidean_norm() < threshold
            && (self.mid - self.track.end).euclidean_norm() < threshold
    }

    /// `PCB_TRACK::GetBoundingBox (PCB_ARC)`: the bounding box of the arc's effective shape,
    /// grown by the rounded-up half width.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let arc = self.effective_shape(
            ctx,
            PcbLayerId::UndefinedLayer,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );

        super::bounding_box_of(&self.track, Some(arc.bbox(0)))
    }

    /// Port of `PCB_ARC::GetEffectiveShape`: a `SHAPE_ARC` of the track width (plus twice the
    /// solder mask expansion on mask layers), or a `SHAPE_SEGMENT` when the arc is effectively
    /// a line.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut width = self.track.width();

        if is_solder_mask_layer(layer.0) {
            width += 2 * self.solder_mask_expansion(ctx);
        }

        let arc = ShapeArc::from_start_mid_end(self.track.start, self.mid, self.track.end, width);

        if arc.is_effective_line() {
            return Arc::new(ShapeSegment::new(self.track.start, self.track.end, width));
        }

        Arc::new(arc)
    }

    /// `PCB_TRACK::TransformShapeToPolygon (PCB_ARC)`: `TransformArcToPolygon` of start, mid,
    /// end with the track width plus twice the clearance (plus twice the solder mask expansion
    /// on mask layers).
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
        debug_assert!(
            !ignore_line_width,
            "IgnoreLineWidth has no meaning for tracks."
        );

        let mut width = self.track.width + (2 * clearance);

        if is_solder_mask_layer(layer.0) {
            width += 2 * self.solder_mask_expansion(ctx);
        }

        transform_arc_to_polygon(
            buffer,
            self.track.start,
            self.mid,
            self.track.end,
            width,
            error,
            error_loc,
        );
    }

    /// Port of `PCB_ARC::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        let max_dist: f64 = accuracy as f64 + (self.track.width() as f64 / 2.0);

        // Short-circuit common cases where the arc is connected to a track or via at an endpoint
        if self.track.start.distance(pos) <= max_dist || self.track.end.distance(pos) <= max_dist {
            return true;
        }

        let center = Vector2L::from(self.arc_center());
        let relpos: Vector2L = Vector2L::from(pos) - center;
        let dist: i64 = relpos.euclidean_norm();
        let radius: f64 = self.radius();

        if (dist as f64 - radius).abs() > max_dist {
            return false;
        }

        let arc_angle = self.angle();
        let arc_angle_start = self.arc_angle_start(); // Always 0.0 ... 360 deg
        let mut arc_hittest =
            EdaAngle::from_vector(Vector2D::new(relpos.x as f64, relpos.y as f64));

        // Calculate relative angle between the starting point of the arc, and the test point
        arc_hittest -= arc_angle_start;

        // Normalise arc_hittest between 0 ... 360 deg
        arc_hittest.normalize();

        if arc_angle < ANGLE_0 {
            return arc_hittest >= ANGLE_360 + arc_angle;
        }

        arc_hittest <= arc_angle
    }

    /// Port of `PCB_ARC::HitTest( aRect, aContained, aAccuracy )`: the box of start, mid and
    /// end grown by half the width, tested against the inflated rectangle.
    pub fn hit_test_rect(
        &self,
        _ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.inflate(accuracy);

        let mut bbox = Box2I::from_pos(self.track.start);
        bbox.merge_point(self.mid);
        bbox.merge_point(self.track.end);

        bbox.inflate(self.track.width() / 2);

        if contained {
            arect.contains_box(&bbox)
        } else {
            arect.intersects(&bbox)
        }
    }

    /// `PCB_TRACK::GetItemDescription (PCB_ARC)`: "Track (arc) <net> on <layer>, length <len>".
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        format!(
            "Track (arc) {} on {}, length {}",
            board_connected_item::netname_msg(self, ctx),
            board_item::layer_name(self, ctx),
            units.message_text_from_value(self.length(), true, EdaDataType::Distance)
        )
    }

    /// Port of `PCB_ARC::Rotate`: start, end and mid rotated about `centre`.
    pub fn rotate(&mut self, _env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        rotate_point_around(&mut self.track.start, centre, angle);
        rotate_point_around(&mut self.track.end, centre, angle);
        rotate_point_around(&mut self.mid, centre, angle);
    }

    /// Port of `PCB_ARC::Flip`: mirror the three points about `centre` and move to the
    /// flipped layer (`GetFlippedLayer`, from the board's flip table in `env`).
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        let (start, end, mid) = (&mut self.track.start, &mut self.track.end, &mut self.mid);

        if dir == FlipDirection::LeftRight {
            start.x = centre.x - (start.x - centre.x);
            end.x = centre.x - (end.x - centre.x);
            mid.x = centre.x - (mid.x - centre.x);
        } else {
            start.y = centre.y - (start.y - centre.y);
            end.y = centre.y - (end.y - centre.y);
            mid.y = centre.y - (mid.y - centre.y);
        }

        let layer = env.layers.flip_layer(self.track.base.item.layer);
        BoardItemMut::set_layer(self, env, layer);
    }

    /// Port of `PCB_ARC::Mirror( aCentre, aFlipDirection )`: `Mirror( MIRROR_AXIS( .. ) )`.
    pub fn mirror(&mut self, _env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(&mirror_axis(centre, dir));
    }

    /// Port of `PCB_ARC::Mirror( const SEG& aAxis )`: reflect start, end and mid.
    pub fn mirror_seg(&mut self, axis: &Seg) {
        self.track.start = axis.reflect_point(self.track.start);
        self.track.end = axis.reflect_point(self.track.end);
        self.mid = axis.reflect_point(self.mid);
    }

    /// `PCB_TRACK::IsOnLayer (PCB_ARC)`: the `PCB_TRACK` body (no virtual involved).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.track.is_on_layer(ctx, layer)
    }

    /// `PCB_TRACK::GetLayerSet (PCB_ARC)`: the `PCB_TRACK` body.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        self.track.layer_set(ctx)
    }

    /// `PCB_TRACK::SetLayerSet (PCB_ARC)`: the `PCB_TRACK` body (`SetLayer` is the
    /// `BOARD_CONNECTED_ITEM` one for both).
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        self.track.set_layer_set(env, layers)
    }

    /// `PCB_TRACK::GetFriendlyName (PCB_ARC)`: "Track (arc)".
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Track (arc)".to_string()
    }

    /// Port of `PCB_ARC::operator==( const PCB_ARC& )`: start, end, mid, layer, width, solder
    /// mask flag and margin.
    pub fn eq_item(&self, other: &PcbArc) -> bool {
        self.track.start == other.track.start
            && self.track.end == other.track.end
            && self.mid == other.mid
            && self.track.base.item.layer == other.track.base.item.layer
            && self.track.width() == other.track.width()
            && self.track.has_solder_mask == other.track.has_solder_mask
            && self.track.solder_mask_margin == other.track.solder_mask_margin
    }

    /// Port of `PCB_ARC::Similarity` for two arcs (the C++ returns 0.0 for another type).
    pub fn similarity(&self, other: &PcbArc) -> f64 {
        let mut similarity = 1.0;

        if self.track.base.item.layer != other.track.base.item.layer {
            similarity *= 0.9;
        }

        if self.track.width() != other.track.width() {
            similarity *= 0.9;
        }

        if self.track.start != other.track.start {
            similarity *= 0.9;
        }

        if self.track.end != other.track.end {
            similarity *= 0.9;
        }

        if self.mid != other.mid {
            similarity *= 0.9;
        }

        if self.track.has_solder_mask != other.track.has_solder_mask {
            similarity *= 0.9;
        }

        if self.track.solder_mask_margin != other.track.solder_mask_margin {
            similarity *= 0.9;
        }

        similarity
    }
}

/// `VECTOR2I - VECTOR2D` (the C++ mixed operator computes in the common type, `double`).
fn sub_d(a: Vector2I, b: Vector2D) -> Vector2D {
    Vector2D::new(a.x as f64 - b.x, a.y as f64 - b.y)
}
