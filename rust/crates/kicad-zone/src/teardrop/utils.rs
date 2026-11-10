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

//! Port of `pcbnew/teardrop/teardrop_utils.cpp`: the track caches, the collision tests and the
//! teardrop outline geometry of `TEARDROP_MANAGER` (anchor points, curved / straight sides,
//! clipping to the pad shape, the width fit against neighbouring copper).
//!
//! Some calculations (mainly computeCurvedForRoundShape) are derived from
//! <https://github.com/NilujePerchut/kicad_scripts/tree/master/teardrops>.
//!
//! Integer / floating point conversions follow the C++ expressions exactly: `VECTOR2I( double,
//! double )` truncates ([`trunc_i32`]), `VECTOR2I / 2` rounds with `KiROUND`, and the
//! `VECTOR2I::Dot( VECTOR2D )` of `computeChordThroughShape` converts the direction to integers
//! first (see there).

use std::collections::{BTreeSet, HashMap};

use kicad_connectivity::data::ConnectivityData;
use kicad_core::layer_ids::Flashing;
use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::bezier_curves::BezierPoly;
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::convert_basic_shapes_to_polygon::transform_circle_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::convex_hull::build_convex_hull;
use kicad_geom::geometry::shape_line_chain::Intersection;
use kicad_geom::stdsort::std_sort;
use kicad_geom::{
    Box2I, EdaAngle, Seg, Shape, ShapeArc, ShapeLineChain, ShapePolySet, Vector2D, Vector2I,
    ki_round, rotate_point,
};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::ids::{ItemId, TrackId, ZoneId};
use kicad_pcb::item::{RecurseMode, SharedShape};
use kicad_pcb::item_ref::{AnyItem, ItemRef};
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::PadShape;
use kicad_pcb::pcb_track::{STARTPOINT, Track};
use kicad_pcb::teardrop_parameters::TeardropParameters;

use super::{TdTrack, TeardropManager};

/// C++ `VECTOR2I( double, double )` / an implicit `double` -> `int` conversion: truncation
/// (`cvttsd2si`, `INT_MIN` out of range, PORTING.md 9).
#[inline]
pub(crate) fn trunc_i32(v: f64) -> i32 {
    f64_to_i32_x86(v)
}

/// `std::min( a, b )` for doubles (`b < a ? b : a`).
#[inline]
fn min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max( a, b )` for doubles (`a < b ? b : a`).
#[inline]
fn max_f64(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// The pad of `item`, if it is one.
fn as_pad<'b>(ctx: &BoardCtx<'b>, item: ItemId) -> Option<&'b Pad> {
    match ctx.board.get(item) {
        Some(AnyItem::Pad(pad)) => Some(pad),
        _ => None,
    }
}

/// `aItem->GetPosition()`.
fn item_position(ctx: &BoardCtx<'_>, item: ItemId) -> Vector2I {
    ctx.item(item).map_or(Vector2I::new(0, 0), |r| r.position())
}

/// `aItem->HitTest( aPos, aAccuracy )` (the virtual `BOARD_ITEM` overload).
fn item_hit_test(ctx: &BoardCtx<'_>, item: ItemId, pos: Vector2I, accuracy: i32) -> bool {
    ctx.item(item)
        .is_some_and(|r| r.as_dyn().hit_test(ctx, pos, accuracy))
}

/// `PAD::TransformShapeToPolygon( aBuffer, aLayer, 0, aMaxError, ERROR_INSIDE )`.
fn pad_polygon(ctx: &BoardCtx<'_>, pad: &Pad, layer: PcbLayerId, buffer: &mut ShapePolySet) {
    pad.transform_shape_to_polygon(
        ctx,
        buffer,
        layer,
        0,
        ctx.max_error(),
        ErrorLoc::Inside,
        false,
    );
}

/// Port of the file-static `NormalizeVector`: a vector of unit length from `vector`.
fn normalize_vector(vector: Vector2I) -> Vector2D {
    let vect = Vector2D::from(vector);
    let norm = vect.euclidean_norm();
    vect / norm
}

/// Port of `TRACK_BUFFER`: tracks grouped by layer and net code.
#[derive(Clone, Debug, Default)]
pub struct TrackBuffer {
    /// m_map_tracks: track buffer, tracks are grouped by layer+netcode.
    pub(crate) map_tracks: std::collections::BTreeMap<i32, Vec<TrackId>>,
}

impl TrackBuffer {
    /// Port of `TRACK_BUFFER::AddTrack`: add a track in buffer, in space grouping tracks
    /// having the same netcode and the same layer.
    pub fn add_track(&mut self, track: TrackId, layer: i32, netcode: i32) {
        self.map_tracks
            .entry(Self::idx_from_lay_net(layer, netcode))
            .or_default()
            .push(track);
    }

    /// Port of `TRACK_BUFFER::GetBuffer`.
    pub fn buffer(&self) -> &std::collections::BTreeMap<i32, Vec<TrackId>> {
        &self.map_tracks
    }

    /// Port of `TRACK_BUFFER::GetNetcodeAndLayerFromIndex`: (layer, netcode).
    pub fn netcode_and_layer_from_index(idx: i32) -> (i32, i32) {
        (idx & 0xFF, idx >> 8)
    }

    /// Port of `TRACK_BUFFER::idxFromLayNet`: build an index from the layer id and the
    /// netcode, to store a track in buffer.
    fn idx_from_lay_net(layer: i32, netcode: i32) -> i32 {
        (netcode << 8) + (layer & 0xFF)
    }
}

/// Port of `TEARDROP_MANAGER::GetWidth`.
pub fn get_width(ctx: &BoardCtx<'_>, item: ItemId, layer: PcbLayerId) -> i32 {
    match ctx.board.get(item) {
        Some(AnyItem::Track(Track::Via(via))) => via.width(layer),
        Some(AnyItem::Pad(pad)) => {
            let size = pad.size(ctx, layer);
            size.x.min(size.y)
        }
        Some(AnyItem::Track(track)) => track.width(),
        _ => 0,
    }
}

/// Port of `TEARDROP_MANAGER::IsRound`.
pub fn is_round(ctx: &BoardCtx<'_>, item: ItemId, layer: PcbLayerId) -> bool {
    match ctx.board.get(item) {
        Some(AnyItem::Track(Track::Via(_))) => true,
        Some(AnyItem::Pad(pad)) => {
            let size = pad.size(ctx, layer);

            pad.shape(layer) == PadShape::Circle
                || (pad.shape(layer) == PadShape::Oval && size.x == size.y)
        }
        _ => true,
    }
}

/// Port of `TEARDROP_MANAGER::copperNetcode`: the item's netcode, or
/// `NETINFO_LIST::UNCONNECTED`.
pub(crate) fn copper_netcode(item: &ItemRef<'_>) -> i32 {
    match item.as_connected() {
        Some(_) => item.net_code().map_or(0, |n| n.0),
        None => 0,
    }
}

/// Port of the file-static `computeCornerTangentControlPoint`: a control point for a teardrop
/// anchor on a rounded rectangle corner, along the tangent to the corner arc at the anchor,
/// in the direction that best aligns with `desired_dir`.
fn compute_corner_tangent_control_point(
    anchor: Vector2I,
    corner_center: Vector2I,
    bias: f64,
    desired_dir: Vector2I,
) -> Vector2I {
    let radial = anchor - corner_center;

    if radial.euclidean_norm() == 0 {
        return anchor;
    }

    // Tangent is perpendicular to the radius. There are two perpendicular directions:
    // (radial.y, -radial.x) and (-radial.y, radial.x)
    // Choose the one that best aligns with the desired direction (toward the track)
    let tangent1 = Vector2I::new(radial.y, -radial.x);
    let tangent2 = Vector2I::new(-radial.y, radial.x);

    // Use dot product to determine which tangent direction aligns better with desired direction
    let dot1 = tangent1.x as i64 * desired_dir.x as i64 + tangent1.y as i64 * desired_dir.y as i64;
    let dot2 = tangent2.x as i64 * desired_dir.x as i64 + tangent2.y as i64 * desired_dir.y as i64;

    let tangent = if dot1 > dot2 { tangent1 } else { tangent2 };

    anchor + tangent.resize(ki_round(bias))
}

/// Port of the file-static `isPointOnOvalEnd`: the semicircle centre if `point` is on a
/// curved end of an oval pad.
fn is_point_on_oval_end(
    point: Vector2I,
    pad_pos: Vector2I,
    pad_size: Vector2I,
    rotation: EdaAngle,
) -> Option<Vector2I> {
    // Transform point to pad-local coordinates (unrotated)
    let mut local_pt = point - pad_pos;
    rotate_point(&mut local_pt, rotation);

    let half_w = pad_size.x / 2;
    let half_h = pad_size.y / 2;

    // Oval geometry: semicircle radius is min dimension / 2
    // The semicircle centers are offset along the major axis
    let radius = half_w.min(half_h);
    let is_horizontal = half_w > half_h;

    let mut arc_center;

    if is_horizontal {
        // Semicircles at left and right ends
        let center_offset = half_w - radius;

        // Check if point is in the curved region (beyond the straight sides)
        if local_pt.x.abs() <= center_offset {
            return None;
        }

        // Determine which end
        let center_x = if local_pt.x > 0 {
            center_offset
        } else {
            -center_offset
        };
        arc_center = Vector2I::new(center_x, 0);
    } else {
        // Semicircles at top and bottom ends
        let center_offset = half_h - radius;

        // Check if point is in the curved region (beyond the straight sides)
        if local_pt.y.abs() <= center_offset {
            return None;
        }

        // Determine which end
        let center_y = if local_pt.y > 0 {
            center_offset
        } else {
            -center_offset
        };
        arc_center = Vector2I::new(0, center_y);
    }

    // Transform arc center back to board coordinates
    rotate_point(&mut arc_center, -rotation);
    arc_center += pad_pos;

    Some(arc_center)
}

/// Port of the file-static `isPointOnRoundedCorner`: the corner arc centre if `point` is in a
/// corner arc region of a rounded rectangle pad.
fn is_point_on_rounded_corner(
    point: Vector2I,
    pad_pos: Vector2I,
    pad_size: Vector2I,
    corner_radius: i32,
    rotation: EdaAngle,
) -> Option<Vector2I> {
    // Transform point to pad-local coordinates (unrotated)
    let mut local_pt = point - pad_pos;
    rotate_point(&mut local_pt, rotation);

    // Half-sizes minus corner radius define the inner rectangle
    let half_w = pad_size.x / 2;
    let half_h = pad_size.y / 2;
    let inner_half_w = half_w - corner_radius;
    let inner_half_h = half_h - corner_radius;

    // Point is in corner region if it's outside the inner rectangle in both dimensions
    let in_corner_x = local_pt.x.abs() > inner_half_w;
    let in_corner_y = local_pt.y.abs() > inner_half_h;

    if !in_corner_x || !in_corner_y {
        return None;
    }

    // Determine which corner
    let corner_x = if local_pt.x > 0 {
        inner_half_w
    } else {
        -inner_half_w
    };
    let corner_y = if local_pt.y > 0 {
        inner_half_h
    } else {
        -inner_half_h
    };

    let mut corner_center = Vector2I::new(corner_x, corner_y);

    // Transform corner center back to board coordinates
    rotate_point(&mut corner_center, -rotation);
    corner_center += pad_pos;

    Some(corner_center)
}

impl TeardropManager {
    /// Port of `TEARDROP_MANAGER::BuildTrackCaches`.
    pub fn build_track_caches(&mut self, ctx: &BoardCtx<'_>) {
        for (id, track) in ctx.board.tracks() {
            if matches!(track, Track::Trace(_) | Track::Arc(_)) {
                let base = &track.as_track().base;
                let layer = base.item.layer;

                if let Some(item) = ctx.item(id.into()) {
                    self.tracks_rtree.insert(
                        item,
                        layer,
                        DrcConstraintT::ClearanceConstraint,
                        0,
                        false,
                    );
                }

                self.track_lookup_list.add_track(id, layer.0, base.net.0);
            }
        }

        self.tracks_rtree.build();
    }

    /// Port of `TEARDROP_MANAGER::ensureCopperIndex`: build the copper collision index,
    /// deferred so a pass with no teardrop candidate never pays for it.
    pub(crate) fn ensure_copper_index(&mut self, ctx: &BoardCtx<'_>) {
        if self.copper_indexed {
            return;
        }

        self.copper_indexed = true;

        let mut items: Vec<ItemId> = Vec::new();

        for (id, _) in ctx.board.tracks() {
            items.push(id.into());
        }

        for (id, _) in ctx.board.zones() {
            items.push(id.into());
        }

        // Graphics, text and dimensions all plot as copper when they sit on a copper layer.
        for (id, _) in ctx.board.drawings() {
            items.push(id);
            ctx.board
                .item_run_on_children(id, RecurseMode::Recurse, &mut |child| items.push(child));
        }

        for (fp, _) in ctx.board.footprints() {
            ctx.board
                .item_run_on_children(fp.into(), RecurseMode::Recurse, &mut |child| {
                    items.push(child)
                });
        }

        for id in items {
            let Some(item) = ctx.item(id) else {
                continue;
            };

            // The filler's priority rules knock a different-net pour back around a teardrop,
            // but nothing keeps two teardrops out of the same gap.
            if item.type_() == KicadT::PcbGroupT {
                continue;
            }

            if item.type_() == KicadT::PcbZoneT
                && !item.as_zone().is_some_and(|z| z.is_teardrop_area())
            {
                continue;
            }

            for layer in item.layer_set().cu_stack().iter() {
                self.copper_rtree.insert(
                    ctx.item(id).expect("live item"),
                    *layer,
                    DrcConstraintT::ClearanceConstraint,
                    0,
                    false,
                );
            }
        }

        self.copper_rtree.build();
    }

    /// Port of `TEARDROP_MANAGER::pairClearance`: the clearance `source_track` owes `item` on
    /// `layer`, less the DRC epsilon, so that geometry sitting exactly at the limit is not
    /// rejected where DRC would pass it.
    pub(crate) fn pair_clearance(
        &mut self,
        ctx: &BoardCtx<'_>,
        source_track: TrackId,
        item: ItemId,
        layer: PcbLayerId,
    ) -> i32 {
        let key = (source_track, item, layer);

        if let Some(&clearance) = self.pair_clearance_cache.get(&key) {
            return clearance;
        }

        let bds = ctx.board.design_settings();
        let mut clearance = 0;

        if let Some(rules) = ctx.rules {
            let constraint = rules.eval_rules(
                ctx,
                DrcConstraintT::ClearanceConstraint,
                ctx.item(source_track.into()),
                ctx.item(item),
                layer,
            );

            if constraint.value.has_min() {
                clearance = constraint.value.min();
            }

            // DRC allows geometry exactly at the limit; rejecting it here would silently
            // delete a teardrop the rules permit.
            clearance = 0.max(clearance - bds.get_drc_epsilon());
        }

        self.pair_clearance_cache.insert(key, clearance);

        clearance
    }

    /// Port of `TEARDROP_MANAGER::zoneConnections`: one set per filled island of `zone` on
    /// `layer`, holding what that island reaches.
    fn zone_connections(
        &mut self,
        connectivity: &ConnectivityData,
        zone: ZoneId,
        layer: PcbLayerId,
    ) -> &Vec<BTreeSet<ItemId>> {
        self.zone_connection_cache
            .entry((zone, layer))
            .or_insert_with(|| connectivity.zone_island_connections(zone.into(), layer))
    }

    /// Port of `TEARDROP_MANAGER::areItemsInSameZone`: true if `pad_or_via` + `track` is
    /// located inside a zone of the same netname.
    pub(crate) fn are_items_in_same_zone(
        &mut self,
        ctx: &BoardCtx<'_>,
        connectivity: &ConnectivityData,
        pad_or_via: ItemId,
        track: TrackId,
    ) -> bool {
        let Some(track_item) = ctx.board.track(track) else {
            return false;
        };
        let layer = track_item.as_track().base.item.layer;
        let netcode = track_item.as_track().base.net;

        for (zone_id, zone) in ctx.board.zones() {
            // Skip teardrops
            if zone.is_teardrop_area() {
                continue;
            }

            // Only consider zones on the same layer as the track
            if !zone.is_on_layer(ctx, layer) {
                continue;
            }

            if zone.base.net != netcode {
                continue;
            }

            // No fill on this layer means no copper here, so it joins nothing here.
            if !zone.has_filled_polys_for_layer(layer) {
                continue;
            }

            // One island has to reach both; a pad here and a track the pour reaches elsewhere
            // are not connected by it.
            for island in self.zone_connections(connectivity, zone_id, layer) {
                if island.contains(&pad_or_via) && island.contains(&ItemId::from(track)) {
                    return true;
                }
            }
        }

        false
    }

    /// Port of `TEARDROP_MANAGER::collidesWithOtherNets`: true if a teardrop with the outline
    /// `points` would touch copper of another net.
    ///
    /// `exempt` are the items it overlaps by construction, its anchor and its track(s);
    /// without them a no-net teardrop collides with its own anchor, no net being shared with
    /// none.
    pub(crate) fn collides_with_other_nets(
        &mut self,
        ctx: &BoardCtx<'_>,
        points: &[Vector2I],
        source_track: TrackId,
        exempt: &[ItemRef<'_>],
    ) -> bool {
        if points.len() < 3 {
            return false;
        }

        let mut teardrop = ShapePolySet::new();
        teardrop.new_outline();

        for pt in points {
            teardrop.append(pt.x, pt.y, -1, -1, false);
        }

        // The shape can self-intersect on awkward entries, and the collision test triangulates.
        teardrop.simplify();

        if teardrop.outline_count() == 0 {
            return false;
        }

        self.ensure_copper_index(ctx);

        let Some(source) = ctx.board.track(source_track) else {
            return false;
        };
        let layer = source.as_track().base.item.layer;
        let netcode = source.as_track().base.net.0;

        // Net 0 is no net at all, so geometry stands in for it: copper touching the anchor or
        // the track is the same conductor, and anything carrying a net is foreign whatever it
        // touches.
        let mut anchor_shapes: Vec<SharedShape> = Vec::new();
        let mut touches_anchor: HashMap<ItemId, bool> = HashMap::new();

        if netcode <= 0 {
            for item in exempt {
                anchor_shapes.push(item.effective_shape(
                    layer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                ));
            }
        }

        let exempt_ids: Vec<ItemId> = exempt.iter().filter_map(|r| r.id()).collect();

        // `collides( aItem, aShape )`; `self` is split so that the caches can be borrowed.
        let mut collides = |mgr: &mut TeardropManager,
                            teardrop: &mut ShapePolySet,
                            item: ItemId,
                            shape: &dyn Shape|
         -> bool {
            if exempt_ids.contains(&item) {
                return false;
            }

            let Some(item_ref) = ctx.item(item) else {
                return false;
            };

            // isSameConductor( aItem )
            let item_net = copper_netcode(&item_ref);

            let same_conductor = if netcode > 0 {
                item_net == netcode
            } else {
                item_net <= 0 && {
                    // touchesTeardropAnchor( aItem )
                    *touches_anchor.entry(item).or_insert_with(|| {
                        let shape = item_ref.effective_shape(
                            layer,
                            Flashing::Default,
                            DrcConstraintT::NullConstraint,
                        );

                        anchor_shapes
                            .iter()
                            .any(|anchor| anchor.collide(shape.as_ref(), 0, None, None))
                    })
                }
            };

            if same_conductor {
                return false;
            }

            let clearance = mgr.pair_clearance(ctx, source_track, item, layer);
            teardrop.collide_shape_mut(shape, clearance, None, None)
        };

        // The radius has to cover the widest clearance anything can demand, or an obstacle
        // with a generous one of its own is never tested.
        let mut bbox = teardrop.bbox(0);
        bbox.inflate(ctx.board.get_max_clearance_value(ctx));

        let candidates: Vec<(ItemId, usize)> = {
            let entries = self.copper_rtree.overlapping(layer, &bbox);
            entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.parent, i))
                .collect()
        };

        if !candidates.is_empty() {
            // The tree cannot stay borrowed while the clearance cache is written: take it out.
            let tree = std::mem::take(&mut self.copper_rtree);

            let hit = {
                let entries = tree.overlapping(layer, &bbox);
                let mut hit = false;

                for entry in entries {
                    if collides(self, &mut teardrop, entry.parent, entry.shape()) {
                        hit = true;
                        break;
                    }
                }

                hit
            };

            self.copper_rtree = tree;

            if hit {
                return true;
            }
        }

        let Some(created) = self.created_td_rtree.get(&layer).cloned() else {
            return false;
        };

        let bmin = [bbox.x(), bbox.y()];
        let bmax = [bbox.right(), bbox.bottom()];

        for (created_box, created_zone) in created {
            // KIRTREE::DYNAMIC_RTREE::Search: boxes overlapping [bmin, bmax], bounds included.
            if created_box.x() > bmax[0]
                || created_box.right() < bmin[0]
                || created_box.y() > bmax[1]
                || created_box.bottom() < bmin[1]
            {
                continue;
            }

            let Some(zone_ref) = ctx.item(created_zone.into()) else {
                continue;
            };

            let shape =
                zone_ref.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

            if collides(self, &mut teardrop, created_zone.into(), shape.as_ref()) {
                return true;
            }
        }

        false
    }

    /// Port of `TEARDROP_MANAGER::computeFittedTeardropPolygon`: widen a teardrop as far as
    /// the surrounding copper allows. The requested width knows nothing of what is nearby, so
    /// bisect down from it; the result is within a few percent. `None` if no width down to the
    /// track width clears the neighbouring copper.
    pub(crate) fn compute_fitted_teardrop_polygon(
        &mut self,
        ctx: &BoardCtx<'_>,
        params: &TeardropParameters,
        track: TdTrack<'_>,
        source_track: TrackId,
        other: ItemId,
        other_pos: Vector2I,
    ) -> Option<Vec<Vector2I>> {
        // A teardrop overlaps its anchor and its track by construction.  aTrack is a stub or a
        // two-segment extension when it differs from aSourceTrack, so both have to be named.
        let track_value = track.get(ctx.board)?;
        let track_ref = match track {
            TdTrack::Board(id) => ctx.item(id.into())?,
            TdTrack::Stub(stub) => ctx.view(AnyItem::Track(stub)),
        };
        let exempt = [ctx.item(other)?, ctx.item(source_track.into())?, track_ref];

        let mut points = Vec::new();

        if !self.compute_teardrop_polygon(
            ctx,
            params,
            &mut points,
            track,
            source_track,
            other,
            other_pos,
        ) {
            return None;
        }

        if !self.collides_with_other_nets(ctx, &points, source_track, &exempt) {
            return Some(points);
        }

        // Bisect between the requested width and the track width, the narrowest worth keeping.
        let layer = track_value.as_track().base.item.layer;
        let preferred = ki_round(get_width(ctx, other, layer) as f64 * params.best_width_ratio);
        let mut hi = if params.td_max_width > 0 {
            params.td_max_width.min(preferred)
        } else {
            preferred
        };
        let mut lo = track_value.width();

        // Nothing narrower than the track is kept, so a track at or over the bound leaves
        // nothing that fits.  Only the pad and via paths reject this before they get here.
        if lo >= hi {
            return None;
        }

        let mut fit_params = params.clone();
        let mut fitted: Vec<Vector2I> = Vec::new();

        let mut try_width = |mgr: &mut TeardropManager, width: i32| -> bool {
            let mut candidate = Vec::new();

            fit_params.td_max_width = width;

            if !mgr.compute_teardrop_polygon(
                ctx,
                &fit_params,
                &mut candidate,
                track,
                source_track,
                other,
                other_pos,
            ) || mgr.collides_with_other_nets(ctx, &candidate, source_track, &exempt)
            {
                return false;
            }

            fitted = candidate;
            true
        };

        // Midpoints alone never probe the bottom of the range, losing a teardrop that only
        // fits close to the track width.
        if !try_width(self, lo) {
            return None;
        }

        let mut ii = 0;

        while ii < 4 && hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;

            if try_width(self, mid) {
                lo = mid;
            } else {
                hi = mid;
            }

            ii += 1;
        }

        if fitted.is_empty() {
            None
        } else {
            Some(fitted)
        }
    }

    /// Port of `TEARDROP_MANAGER::computeChordThroughShape`: the centerline chord length
    /// through `other`'s copper span at `inside_point`. Degenerate, arc, or non-crossing cases
    /// return `INT_MAX` to avoid rejection.
    pub(crate) fn compute_chord_through_shape(
        &self,
        ctx: &BoardCtx<'_>,
        track: &Track,
        other: ItemId,
        layer: PcbLayerId,
        inside_point: Vector2I,
    ) -> i32 {
        // Arcs are genuine entries, not the short straight grazes this filter targets.
        if matches!(track, Track::Arc(_)) {
            return i32::MAX;
        }

        let t = track.as_track();
        let delta = Vector2D::from(t.end - t.start);
        let len = delta.euclidean_norm();

        if len == 0.0 {
            return i32::MAX;
        }

        let max_error = ctx.max_error();
        let radius = get_width(ctx, other, layer) / 2;
        let mut shapebuffer = ShapePolySet::new();

        if is_round(ctx, other, layer) {
            transform_circle_to_polygon(
                &mut shapebuffer,
                item_position(ctx, other),
                radius,
                max_error,
                ErrorLoc::Inside,
                16,
            );
        } else {
            // wxCHECK_MSG( aOther->Type() == PCB_PAD_T, 0, "Expected non-round item to be PAD" )
            let Some(pad) = as_pad(ctx, other) else {
                debug_assert!(false, "Expected non-round item to be PAD");
                return 0;
            };

            pad_polygon(ctx, pad, layer, &mut shapebuffer);
        }

        // Measure the chord on the extended centerline, not the short track segment.
        // The bbox-diagonal reach spans rotated elongated pads.
        let dir = delta / len;
        let mid = (t.start + t.end) / 2;
        let reach = ki_round(shapebuffer.bbox(0).diagonal() as f64 + len);
        let ext_start = mid
            - Vector2I::new(
                ki_round(dir.x * reach as f64),
                ki_round(dir.y * reach as f64),
            );
        let ext_end = mid
            + Vector2I::new(
                ki_round(dir.x * reach as f64),
                ki_round(dir.y * reach as f64),
            );

        // Include every contour and hole in the boundary crossings.
        let mut pts: Vec<Intersection> = Vec::new();
        let ext = Seg::new(ext_start, ext_end);

        for ii in 0..shapebuffer.outline_count() {
            let outline = shapebuffer.outline(ii);
            outline.set_closed(true);
            outline.intersect_seg(&ext, &mut pts);

            for jj in 0..shapebuffer.hole_count(ii) {
                let hole = shapebuffer.hole(ii, jj);
                hole.set_closed(true);
                hole.intersect_seg(&ext, &mut pts);
            }
        }

        // Degenerate/tangent-only crossings should not drop the teardrop.
        if pts.len() < 2 {
            return i32::MAX;
        }

        // Adjacent projected crossings bound copper/air spans.
        // Use the copper span bracketing the inside endpoint.
        //
        // C++ `( hit.p - extStart ).Dot( dir )` is `VECTOR2I::Dot( const VECTOR2I& )`: the
        // VECTOR2D `dir` goes through the implicit converting constructor (clamp, truncation)
        // before the integer dot product. Reproduced as written.
        let dir_i = Vector2I::from_vec(dir);
        let mut proj: Vec<f64> = pts
            .iter()
            .map(|hit| (hit.p - ext_start).dot(dir_i) as f64)
            .collect();

        std_sort(&mut proj, |a, b| a < b);

        let inside_proj = (Vector2D::from(inside_point) - Vector2D::from(ext_start)).dot(dir);

        let mut ii = 0;

        while ii + 1 < proj.len() {
            let span_mid = ext_start
                + Vector2I::new(
                    ki_round(dir.x * (proj[ii] + proj[ii + 1]) / 2.0),
                    ki_round(dir.y * (proj[ii] + proj[ii + 1]) / 2.0),
                );

            if shapebuffer.contains(span_mid, -1, 0, false)
                && inside_proj >= proj[ii]
                && inside_proj <= proj[ii + 1]
            {
                return ki_round(proj[ii + 1] - proj[ii]);
            }

            ii += 1;
        }

        // Boundary-touch fallback: keep the teardrop.
        i32::MAX
    }

    /// Port of `TEARDROP_MANAGER::findTouchingTrack`: a track connected to the end point
    /// `end_point` of `track_ref` (`source_track` excluded), with the end point id
    /// (`STARTPOINT` or `ENDPOINT`). With a Y junction the longest segment wins.
    pub(crate) fn find_touching_track(
        &self,
        ctx: &BoardCtx<'_>,
        track_ref: TdTrack<'_>,
        source_track: TrackId,
        end_point: Vector2I,
    ) -> Option<(TrackId, u32)> {
        let mut matches = 0; // Count of candidates: only 1 is acceptable
        let mut candidate: Option<(TrackId, u32)> = None; // a reference to the track connected

        let track_value = track_ref.get(ctx.board)?;
        let layer = track_value.as_track().base.item.layer;
        let ref_item = match track_ref {
            TdTrack::Board(id) => ctx.item(id.into())?,
            TdTrack::Stub(stub) => ctx.view(AnyItem::Track(stub)),
        };
        let ref_id = ref_item.id();
        let source_id = ItemId::from(source_track);

        // Filter:
        // A stub shares an endpoint with the track it stands in for, and continuing back onto
        // that is not a continuation.
        let mut filter = |item: ItemId| -> bool { Some(item) != ref_id && item != source_id };

        // Visitor
        let mut visitor = |item: ItemId| -> bool {
            let Some(curr_id) = item.typed::<Track>() else {
                return true;
            };
            let Some(curr_track) = ctx.board.track(curr_id) else {
                return true;
            };

            // IsPointOnEnds() returns 0, EDA_ITEM_FLAGS::STARTPOINT or EDA_ITEM_FLAGS::ENDPOINT
            let m = curr_track
                .as_track()
                .is_point_on_ends(end_point, self.tolerance);

            if m != 0 {
                // if faced with a Y junction, choose the track longest segment as candidate
                matches += 1;

                if matches > 1 {
                    let previous_len = candidate
                        .and_then(|(id, _)| ctx.board.track(id))
                        .map_or(0.0, |t| t.length());
                    let curr_len = curr_track.length();

                    if previous_len >= curr_len {
                        return true;
                    }
                }

                candidate = Some((curr_id, m));
            }

            true
        };

        self.tracks_rtree.query_colliding(
            ref_item,
            layer,
            layer,
            Some(&mut filter),
            Some(&mut visitor),
            0,
        );

        candidate
    }

    /// Port of `TEARDROP_MANAGER::computeCurvedForRoundShape`: the curve part points for
    /// teardrops connected to a round shape. The Bezier curve control points are optimized for
    /// a round pad/via shape, and do not give a good curve shape for other pad shapes.
    ///
    /// For large circles where the teardrop width is constrained, the anchor points are
    /// projected onto the circle edge to ensure proper tangent calculation.
    #[allow(clippy::too_many_arguments)]
    fn compute_curved_for_round_shape(
        &self,
        ctx: &BoardCtx<'_>,
        params: &TeardropParameters,
        poly: &mut Vec<Vector2I>,
        layer: PcbLayerId,
        track_half_width: i32,
        track_dir: Vector2D,
        other: ItemId,
        other_pos: Vector2I,
        pts: &mut [Vector2I],
    ) {
        let max_error = ctx.max_error();

        // in pts:
        // A and B are points on the track ( pts[0] and  pts[1] )
        // C and E are points on the aViaPad ( pts[2] and  pts[4] )
        // D is the aViaPad centre ( pts[3] )
        let mut vpercent = params.best_width_ratio;
        let td_height = ki_round(get_width(ctx, other, layer) as f64 * vpercent);

        // First, calculate a aVpercent equivalent to the td_height clamped by aTdMaxHeight
        // We cannot use the initial aVpercent because it gives bad shape with points
        // on aViaPad calculated for a clamped aViaPad size
        if params.td_max_width > 0 && params.td_max_width < td_height {
            vpercent *= params.td_max_width as f64 / td_height as f64;
        }

        let mut radius = get_width(ctx, other, layer) / 2;

        // Don't divide by zero.  No good can come of that.
        // wxCHECK2( radius != 0, radius = 1 );
        if radius == 0 {
            radius = 1;
        }

        let min_vpercent = track_half_width as f64 / radius as f64;
        let weaken = (vpercent - min_vpercent) / (1.0 - min_vpercent) / radius as f64;

        // For large circles where teardrop width is constrained, the anchor points from the
        // convex hull may not be exactly on the circle. Project them onto the circle edge
        // to ensure proper tangent calculation for smooth curves.
        let mut vec_c = pts[2] - other_pos;
        let dist_c = vec_c.euclidean_norm() as f64;

        if dist_c > 0.0 && (dist_c - radius as f64).abs() > max_error as f64 {
            // Point is not on the circle - project it to the circle edge
            pts[2] = other_pos + vec_c.resize(radius);
            vec_c = pts[2] - other_pos;
        }

        let mut vec_e = pts[4] - other_pos;
        let dist_e = vec_e.euclidean_norm() as f64;

        if dist_e > 0.0 && (dist_e - radius as f64).abs() > max_error as f64 {
            // Point is not on the circle - project it to the circle edge
            pts[4] = other_pos + vec_e.resize(radius);
            vec_e = pts[4] - other_pos;
        }

        let bias_bc = 0.5 * Seg::new(pts[1], pts[2]).length() as f64;
        let bias_ae = 0.5 * Seg::new(pts[4], pts[0]).length() as f64;

        let tangent_c = Vector2I::new(
            trunc_i32(pts[2].x as f64 - vec_c.y as f64 * bias_bc * weaken),
            trunc_i32(pts[2].y as f64 + vec_c.x as f64 * bias_bc * weaken),
        );
        let tangent_e = Vector2I::new(
            trunc_i32(pts[4].x as f64 + vec_e.y as f64 * bias_ae * weaken),
            trunc_i32(pts[4].y as f64 - vec_e.x as f64 * bias_ae * weaken),
        );

        let tangent_b = Vector2I::new(
            trunc_i32(pts[1].x as f64 - track_dir.x * bias_bc),
            trunc_i32(pts[1].y as f64 - track_dir.y * bias_bc),
        );
        let tangent_a = Vector2I::new(
            trunc_i32(pts[0].x as f64 - track_dir.x * bias_ae),
            trunc_i32(pts[0].y as f64 - track_dir.y * bias_ae),
        );

        let mut curve_pts: Vec<Vector2I> = Vec::new();
        BezierPoly::new(pts[1], tangent_b, tangent_c, pts[2]).get_poly_i(&mut curve_pts, max_error);

        poly.extend_from_slice(&curve_pts);

        poly.push(pts[3]);

        curve_pts.clear();
        BezierPoly::new(pts[4], tangent_e, tangent_a, pts[0]).get_poly_i(&mut curve_pts, max_error);

        poly.extend_from_slice(&curve_pts);
    }

    /// Port of `TEARDROP_MANAGER::computeCurvedForRectShape`: the curve part points for
    /// teardrops connected to a rectangular/polygonal shape. For rounded rectangles, control
    /// points are computed to be tangent to corner arcs, preventing the teardrop curve from
    /// intersecting the pad's corner radius.
    #[allow(clippy::too_many_arguments)]
    fn compute_curved_for_rect_shape(
        &self,
        ctx: &BoardCtx<'_>,
        poly: &mut Vec<Vector2I>,
        pts: &[Vector2I],
        intersection: Vector2I,
        other: ItemId,
        other_pos: Vector2I,
        layer: PcbLayerId,
    ) {
        let max_error = ctx.max_error();

        // in aPts:
        // A and B are points on the track ( pts[0] and pts[1] )
        // C and E are points on the pad/via ( pts[2] and pts[4] )
        // D is the aViaPad centre ( pts[3] )

        // side1 is( aPts[1], aPts[2] );  from track to via
        let side1 = pts[2] - pts[1]; // vector from track to via
        // side2 is ( aPts[4], aPts[0] ); from via to track
        let side2 = pts[4] - pts[0]; // vector from track to via

        let track_dir = intersection - (pts[0] + pts[1]) / 2;

        // Check if this is a rounded rectangle or oval pad (both have curved regions)
        let mut is_round_rect = false;
        let mut is_oval = false;
        let mut corner_radius = 0;
        let mut pad_size = Vector2I::new(0, 0);
        let mut pad_rotation = EdaAngle::default();

        if let Some(pad) = as_pad(ctx, other) {
            let shape = pad.shape(layer);

            if shape == PadShape::Roundrect {
                is_round_rect = true;
                corner_radius = pad.round_rect_corner_radius(layer);
                pad_size = pad.size(ctx, layer);
                pad_rotation = pad.orientation(ctx);
            } else if shape == PadShape::Oval {
                is_oval = true;
                pad_size = pad.size(ctx, layer);
                pad_rotation = pad.orientation(ctx);
            }
        }

        let mut curve_pts: Vec<Vector2I> = Vec::new();

        // Compute control points for the first Bezier curve (track point B to pad point C)
        let mut ctrl1 = pts[1] + track_dir.resize(side1.euclidean_norm() / 4);

        // Direction from pad anchor toward track (opposite of trackDir which goes pad-ward)
        let toward_track = -track_dir;

        // Default control point - midpoint approach
        let mut ctrl2 = (pts[2] + intersection) / 2;

        if is_round_rect && corner_radius > 0 {
            if let Some(corner_center) =
                is_point_on_rounded_corner(pts[2], other_pos, pad_size, corner_radius, pad_rotation)
            {
                // Anchor is on a corner arc - use tangent-based control point
                let bias = 0.5 * side1.euclidean_norm() as f64;
                ctrl2 =
                    compute_corner_tangent_control_point(pts[2], corner_center, bias, toward_track);
            }
        } else if is_oval {
            if let Some(arc_center) =
                is_point_on_oval_end(pts[2], other_pos, pad_size, pad_rotation)
            {
                // Anchor is on a curved end - use tangent-based control point
                let bias = 0.5 * side1.euclidean_norm() as f64;
                ctrl2 =
                    compute_corner_tangent_control_point(pts[2], arc_center, bias, toward_track);
            }
        }

        BezierPoly::new(pts[1], ctrl1, ctrl2, pts[2]).get_poly_i(&mut curve_pts, max_error);

        poly.extend_from_slice(&curve_pts);

        poly.push(pts[3]);

        // Compute control points for second Bezier curve (pad point E to track point A)
        curve_pts.clear();

        // Default control point - midpoint approach
        ctrl1 = (pts[4] + intersection) / 2;

        if is_round_rect && corner_radius > 0 {
            if let Some(corner_center) =
                is_point_on_rounded_corner(pts[4], other_pos, pad_size, corner_radius, pad_rotation)
            {
                // Anchor is on a corner arc - use tangent-based control point
                let bias = 0.5 * side2.euclidean_norm() as f64;
                ctrl1 =
                    compute_corner_tangent_control_point(pts[4], corner_center, bias, toward_track);
            }
        } else if is_oval {
            if let Some(arc_center) =
                is_point_on_oval_end(pts[4], other_pos, pad_size, pad_rotation)
            {
                // Anchor is on a curved end - use tangent-based control point
                let bias = 0.5 * side2.euclidean_norm() as f64;
                ctrl1 =
                    compute_corner_tangent_control_point(pts[4], arc_center, bias, toward_track);
            }
        }

        ctrl2 = pts[0] + track_dir.resize(side2.euclidean_norm() / 4);

        BezierPoly::new(pts[4], ctrl1, ctrl2, pts[0]).get_poly_i(&mut curve_pts, max_error);

        poly.extend_from_slice(&curve_pts);
    }

    /// Port of `TEARDROP_MANAGER::computeAnchorPoints`: the 2 points on pad/via of the
    /// teardrop shape (`pts[2]` and `pts[4]`; `pts[0]`, `pts[1]` are on the track, `pts[3]` is
    /// the midpoint behind the pad/via centre). False if these 2 points are not found.
    fn compute_anchor_points(
        &self,
        ctx: &BoardCtx<'_>,
        params: &TeardropParameters,
        layer: PcbLayerId,
        item: ItemId,
        pos: Vector2I,
        pts: &mut [Vector2I],
    ) -> bool {
        let max_error = ctx.max_error();

        // Compute the 2 anchor points on pad/via/track of the teardrop shape

        let mut c_buffer = ShapePolySet::new();

        // m_BestWidthRatio is the factor to calculate the teardrop preferred width.
        // teardrop width = pad, via or track size * m_BestWidthRatio (m_BestWidthRatio <= 1.0)
        // For rectangular (and similar) shapes, the preferred_width is calculated from the min
        // dim of the rectangle

        let mut preferred_width =
            ki_round(get_width(ctx, item, layer) as f64 * params.best_width_ratio);

        // force_clip = true to force the pad/via/track polygon to be clipped to follow
        // constraints
        // Clipping is also needed for rectangular shapes, because the teardrop shape is
        // restricted to a polygonal area smaller than the pad area (the teardrop height use the
        // smaller value of X and Y sizes).
        let mut force_clip = params.best_width_ratio < 1.0;

        // To find the anchor points on the pad/via/track shape, we build the polygonal shape,
        // and clip the polygon to the max size (preferred_width or m_TdMaxWidth) by a rectangle
        // centered on the axis of the expected teardrop shape.
        // (only reduce the size of polygonal shape does not give good anchor points)
        if is_round(ctx, item, layer) {
            transform_circle_to_polygon(
                &mut c_buffer,
                pos,
                get_width(ctx, item, layer) / 2,
                max_error,
                ErrorLoc::Inside,
                16,
            );
        } else {
            // Only PADS can have a not round shape
            let Some(pad) = as_pad(ctx, item) else {
                debug_assert!(false, "Expected non-round item to be PAD");
                return false;
            };

            force_clip = true;

            preferred_width =
                ki_round(get_width(ctx, item, layer) as f64 * params.best_width_ratio);
            pad_polygon(ctx, pad, layer, &mut c_buffer);
        }

        // Clip the pad/via/track shape to match the m_TdMaxWidth constraint, and for non-round
        // pads, clip the shape to the smallest of size.x and size.y values.
        if force_clip || (params.td_max_width > 0 && params.td_max_width < preferred_width) {
            // A max width of 0 means no limit, so a min against it would clip the shape to
            // nothing.
            let halfsize = (if params.td_max_width > 0 {
                params.td_max_width.min(preferred_width)
            } else {
                preferred_width
            }) / 2;

            // teardrop_axis is the line from anchor point on the track and the end point
            // of the teardrop in the pad/via
            // this is the teardrop_axis of the teardrop shape to build
            let ref_on_track = (pts[0] + pts[1]) / 2;
            let teardrop_axis = pts[3] - ref_on_track;

            let orient = EdaAngle::from_vector(teardrop_axis);
            let len = teardrop_axis.euclidean_norm();

            // Build the constraint polygon: a rectangle with
            // length = dist between the point on track and the pad/via pos
            // height = m_TdMaxWidth or aViaPad.m_Width
            let mut clipping_rect = ShapePolySet::new();
            clipping_rect.new_outline();

            // Build a horizontal rect: it will be rotated later
            clipping_rect.append(0, -halfsize, -1, -1, false);
            clipping_rect.append(0, halfsize, -1, -1, false);
            clipping_rect.append(len, halfsize, -1, -1, false);
            clipping_rect.append(len, -halfsize, -1, -1, false);

            Shape::rotate(&mut clipping_rect, -orient, Vector2I::new(0, 0));
            clipping_rect.move_(ref_on_track);

            // Clip the shape to the max allowed teadrop area
            c_buffer.boolean_intersection(&clipping_rect);
        }

        /* in aPts:
         * A and B are points on the track ( aPts[0] and  aPts[1] )
         * C and E are points on the aViaPad ( aPts[2] and  aPts[4] )
         * D is midpoint behind the aViaPad centre ( aPts[3] )
         */

        if c_buffer.outline_count() == 0 {
            return false;
        }

        let points: Vec<Vector2I> = c_buffer.c_outline(0).c_points().to_vec();

        let mut initial_points: Vec<Vector2I> = vec![pts[0], pts[1]];
        initial_points.extend_from_slice(&points);

        let mut hull: Vec<Vector2I> = Vec::new();
        build_convex_hull(&mut hull, &initial_points);

        if hull.is_empty() {
            return false;
        }

        // Search for end points of segments starting at aPts[0] or aPts[1]
        // In some cases, in convex hull, only one point (aPts[0] or aPts[1]) is still in list
        let mut point_c = Vector2I::new(0, 0);
        let mut point_e = Vector2I::new(0, 0);
        let mut found_start: i64 = -1; // 2 points (one start and one end) should be found
        let mut found_end: i64 = -1;

        let start = pts[0];
        let pend = pts[1];
        let hull_len = hull.len() as i64;

        for ii in 0..hull_len {
            let mut next = ii + 1;

            if next >= hull_len {
                next = 0;
            }

            let mut prev = ii - 1;

            if prev < 0 {
                prev = hull_len - 1;
            }

            if hull[ii as usize] == start {
                // the previous or the next point is candidate:
                if hull[next as usize] != pend {
                    point_e = hull[next as usize];
                } else {
                    point_e = hull[prev as usize];
                }

                found_start = ii;
            }

            if hull[ii as usize] == pend {
                if hull[next as usize] != start {
                    point_c = hull[next as usize];
                } else {
                    point_c = hull[prev as usize];
                }

                found_end = ii;
            }
        }

        if found_start < 0 {
            // PointE was not initialized, because start point does not exit
            let mut ii = found_end - 1;

            if ii < 0 {
                ii = hull_len - 1;
            }

            point_e = hull[ii as usize];
        }

        if found_end < 0 {
            // PointC was not initialized, because end point does not exit
            let mut ii = found_start - 1;

            if ii < 0 {
                ii = hull_len - 1;
            }

            point_c = hull[ii as usize];
        }

        pts[2] = point_c;
        pts[4] = point_e;

        // Now we have to know if the choice aPts[2] = PointC is the best, or if
        // aPts[2] = PointE is better.
        // A criteria is to calculate the polygon area in these 2 cases, and choose the case
        // that gives the bigger area, because the segments starting at PointC and PointE
        // maximize their distance.
        let dummy1 = ShapeLineChain::from_points(pts, true);
        let area1 = dummy1.area(true);

        pts.swap(2, 4);
        let dummy2 = ShapeLineChain::from_points(pts, true);
        let area2 = dummy2.area(true);

        if area1 > area2 {
            // The first choice (without swapping) is the better.
            pts.swap(2, 4);
        }

        true
    }

    /// Port of `TEARDROP_MANAGER::findAnchorPointsOnTrack`: the reference segment
    /// (`start` inside the teardrop, `end` outside) on `track` (replaced by a connected track
    /// when the first one is too short), the point where the track's centerline meets the
    /// pad/via edge, and the effective teardrop length. `None` if no anchor point can be
    /// found.
    #[allow(clippy::too_many_arguments)]
    fn find_anchor_points_on_track<'s>(
        &self,
        ctx: &BoardCtx<'_>,
        params: &TeardropParameters,
        track: &mut TdTrack<'s>,
        source_track: TrackId,
        other: ItemId,
        other_pos: Vector2I,
    ) -> Option<AnchorOnTrack> {
        let found = true;
        let first = track.get(ctx.board)?;
        let mut start = first.as_track().start; // one reference point on the track, inside teardrop
        let mut end = first.as_track().end; // the second reference point on the track, outside teardrop
        let layer = first.as_track().base.item.layer;
        let radius = get_width(ctx, other, layer) / 2;
        let max_error = ctx.max_error();

        // Requested length of the teardrop:
        let mut target_length =
            ki_round(get_width(ctx, other, layer) as f64 * params.best_length_ratio);

        if params.td_max_len > 0 {
            target_length = params.td_max_len.min(target_length);
        }

        // actualTdLen is the distance between start and the teardrop point on the segment from
        // start to end
        let mut actual_td_len: i32;
        let mut need_swap = false; // true if the start and end points of the current track are swapped

        // aTrack is expected to have one end inside the via/pad and the other end outside
        // so ensure the start point is inside the via/pad
        if !item_hit_test(ctx, other, start, 0) {
            std::mem::swap(&mut start, &mut end);
            need_swap = true;
        }

        let mut shapebuffer = ShapePolySet::new();

        if is_round(ctx, other, layer) {
            transform_circle_to_polygon(
                &mut shapebuffer,
                other_pos,
                radius,
                max_error,
                ErrorLoc::Inside,
                16,
            );
        } else {
            let Some(pad) = as_pad(ctx, other) else {
                debug_assert!(false, "Expected non-round item to be PAD");
                return None;
            };

            pad_polygon(ctx, pad, layer, &mut shapebuffer);
        }

        // C++ reads Outline( 0 ) unconditionally; a pad without copper on the layer has none.
        if shapebuffer.outline_count() == 0 {
            return None;
        }

        let outline = shapebuffer.outline(0);
        outline.set_closed(true);

        // Search the intersection point between the pad/via shape and the current track
        // This this the starting point to define the teardrop length
        let mut pts: Vec<Intersection> = Vec::new();

        let pt_count = if let Track::Arc(arc_track) = first {
            // To find the starting point we convert the arc to a polyline
            // and compute the intersection point with the pad/via shape
            let arc = ShapeArc::from_start_mid_end(
                arc_track.track.start,
                arc_track.mid,
                arc_track.track.end,
                arc_track.track.width,
            );

            let poly = arc.convert_to_polyline(max_error);
            outline.intersect_chain(&poly, &mut pts, false, None)
        } else {
            outline.intersect_seg(&Seg::new(start, end), &mut pts)
        };

        // Ensure a intersection point was found, otherwise we cannot built the teardrop
        // using this track (it is fully outside or inside the pad/via shape)
        if pt_count < 1 {
            return None;
        }

        let intersection = pts[0].p;
        start = intersection; // This is currently the reference point of the teardrop length

        // actualTdLen for now the distance between start and the teardrop point on the (start
        // end)segment. It cannot be bigger than the lenght of this segment
        actual_td_len = target_length.min(Seg::new(start, end).length());
        let ref_lenght_point = start; // the reference point of actualTdLen

        // If the first track is too short to allow a teardrop having the requested length
        // explore the connected track(s), and try to find a anchor point at targetLength from
        // initial start
        if actual_td_len < target_length && params.allow_use_two_tracks {
            let mut consumed = 0;

            #[allow(clippy::never_loop)]
            while actual_td_len + consumed < target_length {
                let Some((connected_id, match_type)) =
                    self.find_touching_track(ctx, *track, source_track, end)
                else {
                    break;
                };

                let Some(connected_track) = ctx.board.track(connected_id) else {
                    break;
                };
                let connected = connected_track.as_track();

                // Reject the extension if the angle between segments is too large.
                // Large angles cause the teardrop shape to bend sharply at the junction.
                // The junction transition code handles bends up to ~60 degrees, so use
                // cos(60) = 0.5 as the threshold.
                const K_MIN_COS_FOR_TWO_SEGMENT_EXTENSION: f64 = 0.5;

                let first_dir = normalize_vector(end - ref_lenght_point);

                let second_dir = if match_type == STARTPOINT {
                    normalize_vector(connected.end - connected.start)
                } else {
                    normalize_vector(connected.start - connected.end)
                };

                let cos_angle = first_dir.x * second_dir.x + first_dir.y * second_dir.y;

                if cos_angle < K_MIN_COS_FOR_TWO_SEGMENT_EXTENSION {
                    break;
                }

                consumed += actual_td_len;
                // actualTdLen is the new distance from new start point and the teardrop anchor
                // point
                actual_td_len = (target_length - consumed).min(trunc_i32(connected_track.length()));
                *track = TdTrack::Board(connected_id);
                end = connected.end;
                start = connected.start;
                need_swap = false;

                if match_type != STARTPOINT {
                    std::mem::swap(&mut start, &mut end);
                    need_swap = true;
                }

                // If we do not want to explore more than one connected track, stop search here
                break;
            }
        }

        // if aTrack is an arc, find the best teardrop end point on the arc
        // It is currently on the segment from arc start point to arc end point,
        // therefore not really on the arc, because we have used only the track end points.
        if let Some(Track::Arc(arc_track)) = track.get(ctx.board) {
            // To find the best start and end points to build the teardrop shape, we convert
            // the arc to segments, and search for the segment having its start point at a dist
            // < actualTdLen, and its end point at adist > actualTdLen:
            let mut arc = ShapeArc::from_start_mid_end(
                arc_track.track.start,
                arc_track.mid,
                arc_track.track.end,
                arc_track.track.width,
            );

            if need_swap {
                arc.reverse();
            }

            let mut poly = arc.convert_to_polyline(max_error);

            // The conversion places its corner points slightly outside the arc. Move them
            // onto the arc so they can be used as points on the track centerline.
            for ii in 0..poly.point_count() {
                let p = arc.nearest_point(poly.c_point(ii));
                poly.set_point(ii, p);
            }

            // Now, find the segment of the arc at a distance < actualTdLen from
            // ref_lenght_point. We just search for the first segment (starting from the farest
            // segment) with its start point at a distance < actualTdLen dist
            // This is basic, but it is probably enough.
            if poly.point_count() > 2 {
                // Note: the first point is inside or near the pad/via shape
                // The last point is outside and the farest from the ref_lenght_point
                // So we explore segments from the last to the first
                let mut ii = poly.point_count() - 1;

                while ii >= 0 {
                    let dist_from_start = (poly.c_point(ii) - start).euclidean_norm();

                    // The first segment at a distance of the reference point < actualTdLen is
                    // OK and is suitable to define the reference segment of the teardrop
                    // anchor.
                    if dist_from_start < actual_td_len || ii == 0 {
                        start = poly.c_point(ii);

                        if ii < poly.point_count() - 1 {
                            end = poly.c_point(ii + 1);
                        }

                        // actualTdLen is the distance between start (the reference segment
                        // start point) and the point on track of the teardrop.
                        // This is the difference between the initial actualTdLen value and the
                        // distance between start and ref_lenght_point.
                        actual_td_len -= (start - ref_lenght_point).euclidean_norm();

                        // Ensure validity of actualTdLen: >= 0, and <= segment lenght
                        if actual_td_len < 0 {
                            // should not happen, but...
                            actual_td_len = 0;
                        }

                        actual_td_len = actual_td_len.min((end - start).euclidean_norm());

                        break;
                    }

                    ii -= 1;
                }
            }
        }

        // aStartPoint and aEndPoint will define later a segment to build the 2 anchors points
        // of the teardrop on the aTrack shape.
        // they are two points (both outside the pad/via shape) of aTrack if aTrack is a
        // segment, or a small segment on aTrack if aTrack is an ARC
        let _ = found;

        Some(AnchorOnTrack {
            start,
            end,
            intersection,
            effective_teardrop_len: actual_td_len,
        })
    }

    /// Port of `TEARDROP_MANAGER::computeTeardropPolygon`: all teardrop points of the
    /// polygon shape, appended to `corners`. False if not buildable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute_teardrop_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        params: &TeardropParameters,
        corners: &mut Vec<Vector2I>,
        track: TdTrack<'_>,
        source_track: TrackId,
        other: ItemId,
        other_pos: Vector2I,
    ) -> bool {
        // Note: aTrack can be modified if the initial track is too short.
        // Save the original pointer so we can detect two-segment extension.
        let original_track = track;
        let mut track = track;

        let Some(anchor) = self.find_anchor_points_on_track(
            ctx,
            params,
            &mut track,
            source_track,
            other,
            other_pos,
        ) else {
            return false;
        };

        // Start and end points of the track anchor of the teardrop
        // the start point is inside the teardrop shape
        // the end point is outside.
        let start = anchor.start;
        let end = anchor.end;
        // Where the track centerline intersects the pad/via edge
        let intersection = anchor.intersection;
        // the dist between the start point and the anchor point on the track
        let track_stub_len = anchor.effective_teardrop_len;

        // The start and end points must be different to calculate a valid polygon shape
        if start == end {
            return false;
        }

        let Some(track_value) = track.get(ctx.board) else {
            return false;
        };

        let vec_t = normalize_vector(end - start);

        // When spanning two segments, findAnchorPointsOnTrack replaces aTrack with the
        // connected track. The start point becomes the junction between the two segments,
        // which differs from intersection (where the first segment meets the pad/via edge).
        // Use the first segment's direction for all via-side geometry so that the teardrop
        // shape is oriented correctly relative to how the track enters the pad/via.
        // Note: for arcs, start also moves away from intersection during arc refinement, but
        // aTrack remains the same pointer, so arc tracks are correctly excluded here.
        let two_segments = !track.same_as(&original_track);

        // vecVia is the direction the track enters the pad/via, used for via-side geometry.
        // When the first segment is so short that the junction coincides with the pad edge
        // intersection, start == intersection produces a zero vector whose normalization is
        // NaN. Fall back to the second segment's direction in that case.
        let mut vec_via = vec_t;

        if two_segments && start != intersection {
            vec_via = normalize_vector(start - intersection);
        }

        // find the 2 points on the track, sharp end of the teardrop
        let mut track_halfwidth = track_value.width() / 2;

        // The canvas draws an arc track as short straight lines that can sit up to the max
        // deviation inside the true edge. Keep the corners inside the track by that amount
        // so they cannot show past the drawn edge.
        if matches!(track_value, Track::Arc(_)) {
            let max_error = ctx.max_error();
            track_halfwidth = (track_value.width() / 4).max(track_value.width() / 2 - max_error);
        }

        let point_b = start
            + Vector2I::new(
                trunc_i32(vec_t.x * track_stub_len as f64 + vec_t.y * track_halfwidth as f64),
                trunc_i32(vec_t.y * track_stub_len as f64 - vec_t.x * track_halfwidth as f64),
            );
        let point_a = start
            + Vector2I::new(
                trunc_i32(vec_t.x * track_stub_len as f64 - vec_t.y * track_halfwidth as f64),
                trunc_i32(vec_t.y * track_stub_len as f64 + vec_t.x * track_halfwidth as f64),
            );

        let layer = track_value.as_track().base.item.layer;

        // To build a polygonal valid shape pointA and point B must be outside the pad
        // It can be inside with some pad shapes having very different X and X sizes
        if !is_round(ctx, other, layer) {
            let Some(pad) = as_pad(ctx, other) else {
                return false;
            };

            if pad.hit_test_layer(ctx, point_a, 0, layer) {
                return false;
            }

            if pad.hit_test_layer(ctx, point_b, 0, layer) {
                return false;
            }
        }

        // Compute pointD, the "back" point of the teardrop behind the pad/via center.
        // For off-center track connections (where the track doesn't pass through the pad
        // center), we project the pad center onto the track axis so the teardrop is built
        // symmetrically about the track rather than being skewed toward the pad center.
        let pad_radius = get_width(ctx, other, layer) / 2;
        let int_to_pad = Vector2D::from(other_pos - intersection);
        let proj_on_track = -(int_to_pad.x * vec_via.x + int_to_pad.y * vec_via.y);
        let offset = kicad_core::PCB_IU_SCALE.mm_to_iu(0.001);

        // A custom pad's position is only its anchor, so projecting it yields a depth
        // unrelated to the lobe the track enters; padRadius comes from that same anchor and is
        // the consistent bound
        let is_custom_pad =
            as_pad(ctx, other).is_some_and(|pad| pad.shape(layer) == PadShape::Custom);

        let mut effective_dist = if is_custom_pad {
            pad_radius as f64
        } else {
            max_f64(proj_on_track, pad_radius as f64)
        };

        // For non-round pads, clamp effectiveDist so pointD stays inside the copper the track
        // enters rather than spiking out the far side on an oblique entry that only grazes a
        // corner
        let non_round_pad = if is_round(ctx, other, layer) {
            None
        } else {
            as_pad(ctx, other)
        };

        if let Some(pad) = non_round_pad {
            let mut pad_poly = ShapePolySet::new();
            pad_polygon(ctx, pad, layer, &mut pad_poly);

            // Cast the into-pad ray from the intersection well past the candidate point so a
            // chord through the pad always produces an exit crossing to clamp against. The
            // reach must span the longest possible chord from the entry, so use the pad's
            // circumscribed radius rather than the minor half-axis (padRadius). On an elongated
            // pad entered along its long axis the exit sits up to two major half-axes away, and
            // a reach scaled by the minor axis stops short of it, leaving no crossing and
            // wrongly collapsing the teardrop.
            let reach = effective_dist + 2.0 * pad.bounding_radius(ctx) as f64 + offset as f64;
            let ray_end = intersection
                + Vector2I::new(ki_round(-vec_via.x * reach), ki_round(-vec_via.y * reach));

            // A custom pad's copper can be several disjoint outlines and the ray may cross a
            // hole, so gather crossings from every contour rather than outline 0 alone
            let mut hits: Vec<Intersection> = Vec::new();
            let ray = Seg::new(intersection, ray_end);

            for ii in 0..pad_poly.outline_count() {
                let pad_outline = pad_poly.outline(ii);
                pad_outline.set_closed(true);
                pad_outline.intersect_seg(&ray, &mut hits);

                for jj in 0..pad_poly.hole_count(ii) {
                    let hole = pad_poly.hole(ii, jj);
                    hole.set_closed(true);
                    hole.intersect_seg(&ray, &mut hits);
                }
            }

            let mut crossings: Vec<f64> = Vec::with_capacity(hits.len());

            for hit in &hits {
                // Ignore the crossing at the intersection point itself.
                let d = (hit.p - intersection).euclidean_norm() as f64;

                if d > offset as f64 {
                    crossings.push(d);
                }
            }

            std_sort(&mut crossings, |a, b| a < b);

            // Concave copper is re-entered further along the ray, so the last crossing can sit
            // in an unrelated lobe; probe past each crossing so a vertex graze does not count
            // as the exit
            let mut exit_edge = 0.0;

            for &d in &crossings {
                let probe = intersection
                    + Vector2I::new(
                        ki_round(-vec_via.x * (d + offset as f64)),
                        ki_round(-vec_via.y * (d + offset as f64)),
                    );

                if !pad_poly.contains(probe, -1, 0, false) {
                    exit_edge = d;
                    break;
                }
            }

            // exitEdge == 0 means -vecVia does not penetrate the pad (a tangential graze);
            // collapse pointD onto the entry so the teardrop simply flares from the track to
            // the pad edge.
            effective_dist = min_f64(
                effective_dist,
                max_f64(0.0, exit_edge - 2.0 * offset as f64),
            );
        } else {
            // For round pads/vias, clamp effectiveDist so pointD stays inside the pad circle.
            // The minimum-of-padRadius floor used for projOnTrack overshoots the pad when the
            // teardrop axis (vecVia) is not radial: e.g. a two-segment teardrop where the first
            // segment grazes the pad tangentially produces a projection close to zero, but the
            // floor still pushes pointD outward by padRadius along -vecVia, creating a spike.
            // Solve for the far intersection of the ray (intersection, -vecVia) with the pad
            // circle and use that as the upper bound.
            let r = pad_radius as f64;
            let cx = int_to_pad.x; // (aOtherPos - intersection)
            let cy = int_to_pad.y;
            let dist_center_sq = cx * cx + cy * cy;

            // Quadratic for ||intersection + (-vecVia)*t - aOtherPos||^2 = R^2
            // expands to t^2 - 2*projOnTrack*t + (distCenterSq - R^2) = 0.
            // The far root is projOnTrack + sqrt(projOnTrack^2 - (distCenterSq - R^2)).
            let disc = proj_on_track * proj_on_track - (dist_center_sq - r * r);

            if disc >= 0.0 {
                let far_edge = proj_on_track + disc.sqrt();
                let max_allowed = max_f64(0.0, far_edge - 2.0 * offset as f64);

                if effective_dist > max_allowed {
                    effective_dist = max_allowed;
                }
            }
        }

        let point_d = intersection
            + Vector2I::new(
                ki_round(-vec_via.x * (effective_dist + offset as f64)),
                ki_round(-vec_via.y * (effective_dist + offset as f64)),
            );

        let point_c = Vector2I::new(0, 0); // Point on pad/via outlines
        let point_e = Vector2I::new(0, 0);

        // For two-segment teardrops, compute junction edge points where the track
        // changes direction, and use the first segment side of the junction for
        // the convex hull anchor so that C and E are oriented to the via entry axis.
        let mut junction_b_seg2 = Vector2I::new(0, 0);
        let mut junction_b_seg1 = Vector2I::new(0, 0);
        let mut junction_a_seg2 = Vector2I::new(0, 0);
        let mut junction_a_seg1 = Vector2I::new(0, 0);

        if two_segments {
            let hw = track_halfwidth as f64;
            junction_b_seg2 =
                start + Vector2I::new(ki_round(vec_t.y * hw), ki_round(-vec_t.x * hw));
            junction_a_seg2 =
                start + Vector2I::new(ki_round(-vec_t.y * hw), ki_round(vec_t.x * hw));
            junction_b_seg1 =
                start + Vector2I::new(ki_round(vec_via.y * hw), ki_round(-vec_via.x * hw));
            junction_a_seg1 =
                start + Vector2I::new(ki_round(-vec_via.y * hw), ki_round(vec_via.x * hw));
        }

        // On the inside of a bend, the seg2 junction point backtracks relative to
        // seg1 and causes self-intersection. Detect with a dot product test and skip
        // the seg2 point on whichever side would backtrack.
        let mut skip_junction_a = false;
        let mut skip_junction_b = false;

        if two_segments {
            let trans_a = Vector2D::from(junction_a_seg2 - junction_a_seg1);
            let anchor_dir_a = Vector2D::from(point_a - junction_a_seg1);
            skip_junction_a = (trans_a.x * anchor_dir_a.x + trans_a.y * anchor_dir_a.y) < 0.0;

            let trans_b = Vector2D::from(junction_b_seg2 - junction_b_seg1);
            let anchor_dir_b = Vector2D::from(point_b - junction_b_seg1);
            skip_junction_b = (trans_b.x * anchor_dir_b.x + trans_b.y * anchor_dir_b.y) < 0.0;
        }

        let anchor_a = if two_segments {
            junction_a_seg1
        } else {
            point_a
        };
        let anchor_b = if two_segments {
            junction_b_seg1
        } else {
            point_b
        };

        let mut pts: Vec<Vector2I> = vec![anchor_a, anchor_b, point_c, point_d, point_e];

        // On failure the pad-side anchors are left at the origin, stretching the shape to (0, 0).
        if !self.compute_anchor_points(ctx, params, layer, other, other_pos, &mut pts) {
            return false;
        }

        // For off-center track connections, the convex hull produces asymmetric anchor points
        // (C and E at different distances from the track axis). Recompute them to be symmetric
        // so the teardrop flares out evenly from the track on both sides.
        if is_round(ctx, other, layer) {
            let perp_via = Vector2D::new(-vec_via.y, vec_via.x);

            // Perpendicular distance from pad center to the track axis
            let pad_offset = Vector2D::from(other_pos - intersection);
            let perp_dist_to_center = pad_offset.x * perp_via.x + pad_offset.y * perp_via.y;

            // Only apply the symmetric adjustment when the track is significantly off-center.
            if perp_dist_to_center.abs() > pad_radius as f64 * 0.1 {
                let d = perp_dist_to_center.abs();

                if d < pad_radius as f64 {
                    // The maximum symmetric half-width is limited by the shorter side, which is
                    // the distance from the track axis to the nearest circle edge (R - d).
                    let max_symmetric = pad_radius as f64 - d;

                    // Apply the configured width ratio and max width constraints
                    let preferred_width =
                        ki_round(get_width(ctx, other, layer) as f64 * params.best_width_ratio);
                    let mut max_half_width = preferred_width / 2;

                    if params.td_max_width > 0 {
                        max_half_width = max_half_width.min(params.td_max_width / 2);
                    }

                    let sym_half_width = min_f64(max_symmetric, max_half_width as f64);

                    if sym_half_width > track_halfwidth as f64 {
                        let center = Vector2D::from(other_pos);
                        let r = pad_radius as f64;

                        // Find C on the circle at perpendicular distance +symHalfWidth from
                        // track. Line: p = (perpFoot + perpVia*symHalfWidth) + t * vecVia
                        // Intersect with circle (center, R) and pick the point closest to
                        // the intersection point (track entry side).
                        let find_circle_line_intersection = |perp_dist: f64| -> Vector2I {
                            let proj_along_track =
                                pad_offset.x * vec_via.x + pad_offset.y * vec_via.y;
                            let line_origin = Vector2D::from(intersection)
                                + vec_via * proj_along_track
                                + perp_via * perp_dist;

                            let oc = line_origin - center;
                            let b_coeff = oc.x * vec_via.x + oc.y * vec_via.y;
                            let c_coeff = oc.x * oc.x + oc.y * oc.y - r * r;
                            let disc = b_coeff * b_coeff - c_coeff;

                            if disc < 0.0 {
                                return Vector2I::new(
                                    ki_round(line_origin.x),
                                    ki_round(line_origin.y),
                                );
                            }

                            let sqrt_disc = disc.sqrt();
                            let t1 = -b_coeff - sqrt_disc;
                            let t2 = -b_coeff + sqrt_disc;

                            // Pick the point on the intersection side (closer to the track
                            // entry)
                            let p1 = line_origin + vec_via * t1;
                            let p2 = line_origin + vec_via * t2;
                            let int_pt = Vector2D::from(intersection);

                            if (p1 - int_pt).euclidean_norm() < (p2 - int_pt).euclidean_norm() {
                                return Vector2I::new(ki_round(p1.x), ki_round(p1.y));
                            }

                            Vector2I::new(ki_round(p2.x), ki_round(p2.y))
                        };

                        // pointA is offset in +perpVia from the track axis, pointB in -perpVia
                        // (see the VECTOR2I pointA/pointB construction above). pts[2] is C,
                        // which lies adjacent to pointB in the teardrop walk A->B->C->D->E->A,
                        // so it must sit on pointB's (-perpVia) side. Likewise pts[4] is E,
                        // adjacent to pointA on the +perpVia side. Assigning the opposite signs
                        // folds the polygon into a bowtie and produces self-intersecting edges
                        // whenever the track is off-center enough to trigger this branch.
                        pts[2] = find_circle_line_intersection(-sym_half_width);
                        pts[4] = find_circle_line_intersection(sym_half_width);
                    }
                }
            }
        }

        if !params.curved_edges {
            if two_segments {
                corners.push(point_a);
                corners.push(point_b);

                if !skip_junction_b {
                    corners.push(junction_b_seg2);
                }

                corners.push(pts[1]); // junctionB_seg1
                corners.push(pts[2]); // C
                corners.push(pts[3]); // D
                corners.push(pts[4]); // E
                corners.push(pts[0]); // junctionA_seg1

                if !skip_junction_a {
                    corners.push(junction_a_seg2);
                }
            } else {
                *corners = pts;
            }

            return true;
        }

        // See if we can use curved teardrop shape
        if is_round(ctx, other, layer) {
            if two_segments {
                let mut curve_poly: Vec<Vector2I> = Vec::new();
                self.compute_curved_for_round_shape(
                    ctx,
                    params,
                    &mut curve_poly,
                    layer,
                    track_halfwidth,
                    vec_via,
                    other,
                    other_pos,
                    &mut pts,
                );

                corners.push(point_b);

                if !skip_junction_b {
                    corners.push(junction_b_seg2);
                }

                corners.extend_from_slice(&curve_poly);

                if !skip_junction_a {
                    corners.push(junction_a_seg2);
                }

                corners.push(point_a);
            } else {
                self.compute_curved_for_round_shape(
                    ctx,
                    params,
                    corners,
                    layer,
                    track_halfwidth,
                    vec_t,
                    other,
                    other_pos,
                    &mut pts,
                );
            }
        } else {
            // td_width (computed by the C++ for computeCurvedForRectShape, which ignores it)
            if two_segments {
                let mut curve_poly: Vec<Vector2I> = Vec::new();
                self.compute_curved_for_rect_shape(
                    ctx,
                    &mut curve_poly,
                    &pts,
                    intersection,
                    other,
                    other_pos,
                    layer,
                );

                corners.push(point_b);

                if !skip_junction_b {
                    corners.push(junction_b_seg2);
                }

                corners.extend_from_slice(&curve_poly);

                if !skip_junction_a {
                    corners.push(junction_a_seg2);
                }

                corners.push(point_a);
            } else {
                self.compute_curved_for_rect_shape(
                    ctx,
                    corners,
                    &pts,
                    intersection,
                    other,
                    other_pos,
                    layer,
                );
            }
        }

        true
    }
}

/// What `findAnchorPointsOnTrack` returns through its out-parameters.
#[derive(Clone, Copy, Debug)]
struct AnchorOnTrack {
    /// aStartPoint: inside the teardrop.
    start: Vector2I,
    /// aEndPoint: outside the teardrop.
    end: Vector2I,
    /// aIntersection: where the track's centerline meets the pad/via edge.
    intersection: Vector2I,
    /// aEffectiveTeardropLen.
    effective_teardrop_len: i32,
}

/// `BOX2I` of a teardrop outline as `createAndAddTeardropWithMask` inserts it in
/// `m_createdTdRTree` (`Outline()->BBox()`).
pub(crate) fn outline_bbox(outline: &ShapePolySet) -> Box2I {
    outline.bbox(0)
}
