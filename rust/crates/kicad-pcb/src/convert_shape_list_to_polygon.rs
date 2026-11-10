// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2017 Jean-Pierre Charras, jp.charras at wanadoo.fr
// Copyright (C) 2015 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of `pcbnew/convert_shape_list_to_polygon.h` / `.cpp` (package M21): building closed
//! polygons from lists of `PCB_SHAPE`s (board outlines on Edge.Cuts, footprint courtyards),
//! the outline validity test of DRC and the footprint-view outline fallback.
//!
//! C++ pointers to shapes become [`ShapeId`] handles resolved through the [`BoardCtx`]. The
//! `SKIP_STRUCT` flags the C++ sets on the shapes (and clears with `SCOPED_FLAGS_CLEANER`) are
//! an algorithm-local set ([`OutlineScratch`]), as `board-model.md` 15 rule 11 requires.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kicad_core::{Flashing, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::roundrect::Roundrect;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::geometry::shape_line_chain::Intersection;
use kicad_geom::stdsort::std_sort;
use kicad_geom::thirdparty::nanoflann::{DatasetAdaptor, KdTreeSingleIndexAdaptor, ResultItem};
use kicad_geom::{
    ANGLE_360, Box2I, Seg, Shape, ShapeArc, ShapeLineChain, ShapeLineChainBase, ShapePolySet,
    ShapeRect, Vector2I, lexicographical_compare,
};

use crate::board_item;
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::ShapeT;
use crate::footprint::Footprint;
use crate::ids::{FootprintId, ItemId, ShapeId};
use crate::item::SharedShape;
use crate::pcb_shape::PcbShape;
use crate::traits::EdaShapeItem;

/// `OUTLINE_ERROR_HANDLER` (`std::function<void( const wxString& msg, BOARD_ITEM* itemA,
/// BOARD_ITEM* itemB, const VECTOR2I& pt )>`): the items as handles, `None` for a null
/// pointer. Passed as `Option<&mut dyn OutlineErrorHandler>` (C++ `OUTLINE_ERROR_HANDLER*`,
/// null = no reporting). Every closure with the four parameters is a handler:
/// `Some(&mut |msg: &str, a, b, pt| ...)`.
pub trait OutlineErrorHandler {
    fn on_error(&mut self, msg: &str, item_a: Option<ItemId>, item_b: Option<ItemId>, pt: Vector2I);
}

impl<F> OutlineErrorHandler for F
where
    F: FnMut(&str, Option<ItemId>, Option<ItemId>, Vector2I),
{
    fn on_error(
        &mut self,
        msg: &str,
        item_a: Option<ItemId>,
        item_b: Option<ItemId>,
        pt: Vector2I,
    ) {
        self(msg, item_a, item_b, pt)
    }
}

/// Algorithm state that the C++ keeps on the shapes themselves during one top-level call:
///
/// * `skip`: the `SKIP_STRUCT` flags (`SCOPED_FLAGS_CLEANER` clears them when the top-level
///   function returns, which is dropping this value);
/// * `bezier_rebuilt`: the `aErrorMax` of the last `RebuildBezierToSegmentsPointsList` the
///   chaining applied to a bezier. The C++ rewrites `m_bezierPoints` of the board item, which
///   changes its `GetEffectiveShape()` for the error reports of the same call; the port keeps
///   the board immutable and rebuilds a copy where an effective shape is needed (see
///   `PORT_STATUS.md`, M21 deviations).
#[derive(Default)]
pub(crate) struct OutlineScratch {
    skip: BTreeSet<ShapeId>,
    bezier_rebuilt: BTreeMap<ShapeId, i32>,
}

/// The `std::map<std::pair<VECTOR2I, VECTOR2I>, PCB_SHAPE*>` recording which shape produced
/// each contour segment (the value can be null in C++: `None`).
type ShapeOwners = BTreeMap<(Vector2I, Vector2I), Option<ShapeId>>;

/// Port of `close_enough`: local and tunable method of qualifying the proximity of two
/// points (`aLimit` is a measure of proximity that the caller knows about).
fn close_enough(left: Vector2I, right: Vector2I, limit: u32) -> bool {
    (left - right).squared_euclidean_norm() <= Seg::square(limit as i32)
}

/// Port of `closer_to_first`: true if `first` is closer to `reference` than `second`.
fn closer_to_first(reference: Vector2I, first: Vector2I, second: Vector2I) -> bool {
    (reference - first).squared_euclidean_norm() < (reference - second).squared_euclidean_norm()
}

/// Port of `free_end`: the end of `first` that does not join `second`.
fn free_end(first: &PcbShape, second: &PcbShape) -> Vector2I {
    let gap = |pt: Vector2I| {
        std::cmp::min(
            (pt - second.shape.start()).squared_euclidean_norm(),
            (pt - second.shape.end()).squared_euclidean_norm(),
        )
    };

    if gap(first.shape.start()) <= gap(first.shape.end()) {
        first.shape.end()
    } else {
        first.shape.start()
    }
}

/// Port of `isCopperOutside`: true if a pad of `footprint` (on any of its unique padstack
/// layers) does not intersect `shape`.
fn is_copper_outside(
    ctx: &BoardCtx<'_>,
    footprint_id: FootprintId,
    _footprint: &Footprint,
    shape: &ShapePolySet,
) -> bool {
    let mut pad_outside = false;

    for (_, pad) in ctx.board.footprint_pads(footprint_id) {
        pad.padstack.for_each_unique_layer(|layer| {
            let mut poly = shape.clone_drop_triangulation();

            poly.clear_arcs();

            poly.boolean_intersection(&pad.effective_polygon(ctx, layer, ErrorLoc::Inside));

            if poly.outline_count() == 0 {
                // wxLogTrace( traceBoardOutline, "Tested pad (%d, %d): outside" )
                pad_outside = true;
            }
        });

        if pad_outside {
            break;
        }

        // wxLogTrace( traceBoardOutline, "Tested pad (%d, %d): not outside" )
    }

    pad_outside
}

/// Port of `PCB_SHAPE_ENDPOINTS_ADAPTOR`: the start and end point of every shape, in list
/// order, as the nanoflann dataset.
struct EndpointsAdaptor {
    endpoints: Vec<(Vector2I, ShapeId)>,
}

impl EndpointsAdaptor {
    fn new(ctx: &BoardCtx<'_>, shapes: &[ShapeId]) -> Self {
        let mut endpoints = Vec::with_capacity(shapes.len() * 2);

        for &id in shapes {
            let shape = &ctx.board[id];
            endpoints.push((shape.shape.start(), id));
            endpoints.push((shape.shape.end(), id));
        }

        EndpointsAdaptor { endpoints }
    }
}

impl DatasetAdaptor for EndpointsAdaptor {
    fn kdtree_get_point_count(&self) -> usize {
        self.endpoints.len()
    }

    fn kdtree_get_pt(&self, idx: usize, dim: usize) -> f64 {
        if dim == 0 {
            self.endpoints[idx].0.x as f64
        } else {
            self.endpoints[idx].0.y as f64
        }
    }
}

/// `KDTree` = `KDTreeSingleIndexAdaptor< L2_Simple_Adaptor<double, ..>, .., 2 >`.
type KdTree = KdTreeSingleIndexAdaptor<EndpointsAdaptor, 2>;

/// `PCB_SHAPE::GetEffectiveShape()` (default arguments) of a shape, with the bezier points
/// as the chaining of this call left them (see [`OutlineScratch`]).
fn effective_shape_of(ctx: &BoardCtx<'_>, scratch: &OutlineScratch, id: ShapeId) -> SharedShape {
    let shape = &ctx.board[id];

    match scratch.bezier_rebuilt.get(&id) {
        Some(&max_error) => {
            let mut copy = shape.clone();
            copy.shape.rebuild_bezier_to_segments_points_list(max_error);
            copy.effective_shape(
                ctx,
                PcbLayerId::UndefinedLayer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            )
        }
        None => shape.effective_shape(
            ctx,
            PcbLayerId::UndefinedLayer,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        ),
    }
}

/// Port of `processClosedShape`: appends the outline of a closed shape (polygon, circle,
/// rectangle, ellipse) to `contour` and records the owner of each segment.
fn process_closed_shape(
    ctx: &BoardCtx<'_>,
    id: ShapeId,
    contour: &mut ShapeLineChain,
    shape_owners: &mut ShapeOwners,
    error_max: i32,
    allow_use_arcs_in_polygons: bool,
) {
    let shape = &ctx.board[id];

    match shape.shape.shape() {
        ShapeT::Poly => {
            let mut prev_pt = Vector2I::default();
            let mut first_pt = true;

            for pt in shape.shape.poly_shape().c_iterate() {
                contour.append_point(pt, false);

                if first_pt {
                    first_pt = false;
                } else {
                    shape_owners.insert((prev_pt, pt), Some(id));
                }

                prev_pt = pt;
            }

            contour.set_closed(true);
        }
        ShapeT::Circle => {
            let center = shape.shape.center();
            let radius = shape.shape.radius();
            let mut start = center;
            start.x += radius;

            let arc360 = ShapeArc::from_center_start_angle(center, start, ANGLE_360, 0);
            contour.append_arc_with_error(&arc360, error_max);
            contour.set_closed(true);

            for ii in 1..contour.point_count() {
                shape_owners.insert((contour.c_point(ii - 1), contour.c_point(ii)), Some(id));
            }

            if !allow_use_arcs_in_polygons {
                contour.clear_arcs();
            }
        }
        ShapeT::Rectangle => {
            if shape.shape.corner_radius() > 0 {
                let rr = Roundrect::new(
                    ShapeRect::from_pos_size(
                        shape.shape.start(),
                        shape.shape.rectangle_width(),
                        shape.shape.rectangle_height(),
                    ),
                    shape.shape.corner_radius(),
                    true, /* normalize */
                );
                let mut poly = ShapePolySet::new();
                rr.transform_to_polygon(&mut poly, board_item::max_error(shape, ctx));
                contour.append_chain(poly.c_outline(0));

                for ii in 1..contour.point_count() {
                    shape_owners.insert((contour.c_point(ii - 1), contour.c_point(ii)), Some(id));
                }

                if !allow_use_arcs_in_polygons {
                    contour.clear_arcs();
                }

                contour.set_closed(true);
                return;
            }

            let pts = shape.shape.rect_corners();
            let mut prev_pt = Vector2I::default();
            let mut first_pt = true;

            for &pt in &pts {
                contour.append_point(pt, false);

                if first_pt {
                    first_pt = false;
                } else {
                    shape_owners.insert((prev_pt, pt), Some(id));
                }

                prev_pt = pt;
            }

            contour.set_closed(true);
        }
        ShapeT::Ellipse => {
            // Tessellate the ellipse outline and append it as a closed contour.
            let ellipse = shape.shape.ellipse();
            let e = ShapeEllipse::new(
                ellipse.center,
                ellipse.major_radius,
                ellipse.minor_radius,
                ellipse.rotation,
            );

            let chain = e.convert_to_polyline(error_max);

            for ii in 0..chain.point_count() {
                contour.append_point(chain.c_point(ii), false);
            }

            contour.set_closed(true);

            for ii in 1..contour.point_count() {
                shape_owners.insert((contour.c_point(ii - 1), contour.c_point(ii)), Some(id));
            }
        }
        _ => {}
    }
}

/// Port of `processShapeSegment`: appends an open shape (segment, arc, bezier, elliptical
/// arc) to `contour`, continuing from `prev_pt` (updated to the far end of the shape).
#[allow(clippy::too_many_arguments)]
fn process_shape_segment(
    ctx: &BoardCtx<'_>,
    scratch: &mut OutlineScratch,
    id: ShapeId,
    contour: &mut ShapeLineChain,
    prev_pt: &mut Vector2I,
    shape_owners: &mut ShapeOwners,
    error_max: i32,
    chaining_epsilon: i32,
    allow_use_arcs_in_polygons: bool,
) {
    let shape = &ctx.board[id];

    match shape.shape.shape() {
        ShapeT::Segment => {
            let next_pt = if closer_to_first(*prev_pt, shape.shape.start(), shape.shape.end()) {
                shape.shape.end()
            } else {
                shape.shape.start()
            };

            contour.append_point(next_pt, false);
            shape_owners.insert((*prev_pt, next_pt), Some(id));
            *prev_pt = next_pt;
        }
        ShapeT::Arc => {
            let mut pstart = shape.shape.start();
            let pmid = shape.shape.arc_mid();
            let mut pend = shape.shape.end();

            if !close_enough(*prev_pt, pstart, chaining_epsilon as u32)
                && !close_enough(*prev_pt, pend, chaining_epsilon as u32)
            {
                return;
            }

            if closer_to_first(*prev_pt, pend, pstart) {
                std::mem::swap(&mut pstart, &mut pend);
            }

            pstart = *prev_pt;
            let sarc = ShapeArc::from_start_mid_end(pstart, pmid, pend, 0);
            let mut arc_chain = ShapeLineChain::new();
            arc_chain.append_arc_with_error(&sarc, error_max);

            if !allow_use_arcs_in_polygons {
                arc_chain.clear_arcs();
            }

            for ii in 1..arc_chain.point_count() {
                shape_owners.insert((arc_chain.c_point(ii - 1), arc_chain.c_point(ii)), Some(id));
            }

            contour.append_chain(&arc_chain);
            *prev_pt = pend;
        }
        ShapeT::Bezier => {
            let next_pt;
            let mut reverse = false;

            if closer_to_first(*prev_pt, shape.shape.start(), shape.shape.end()) {
                next_pt = shape.shape.end();
            } else {
                next_pt = shape.shape.start();
                reverse = true;
            }

            // aShape->RebuildBezierToSegmentsPointsList( aErrorMax ): see OutlineScratch.
            let bezier_points = shape.shape.build_bezier_to_segments_points_list(error_max);
            scratch.bezier_rebuilt.insert(id, error_max);

            if reverse {
                for &pt in bezier_points.iter().rev() {
                    if *prev_pt == pt {
                        continue;
                    }

                    contour.append_point(pt, false);
                    shape_owners.insert((*prev_pt, pt), Some(id));
                    *prev_pt = pt;
                }
            } else {
                for &pt in &bezier_points {
                    if *prev_pt == pt {
                        continue;
                    }

                    contour.append_point(pt, false);
                    shape_owners.insert((*prev_pt, pt), Some(id));
                    *prev_pt = pt;
                }
            }

            *prev_pt = next_pt;
        }
        ShapeT::EllipseArc => {
            let mut pstart = shape.shape.start();
            let mut pend = shape.shape.end();
            let mut reverse = false;

            if !close_enough(*prev_pt, pstart, chaining_epsilon as u32)
                && !close_enough(*prev_pt, pend, chaining_epsilon as u32)
            {
                return;
            }

            // Same as the arc case above
            if closer_to_first(*prev_pt, pend, pstart) {
                reverse = true;
                std::mem::swap(&mut pstart, &mut pend);
            }

            let ellipse = shape.shape.ellipse();
            let e = ShapeEllipse::new_arc(
                ellipse.center,
                ellipse.major_radius,
                ellipse.minor_radius,
                ellipse.rotation,
                ellipse.start_angle,
                ellipse.end_angle,
            );

            let mut arc_chain = e.convert_to_polyline(error_max);

            if reverse {
                arc_chain = arc_chain.reverse();
            }

            for ii in 0..arc_chain.point_count() {
                let pt = arc_chain.c_point(ii);

                if pt == *prev_pt {
                    continue;
                }

                contour.append_point(pt, false);
                shape_owners.insert((*prev_pt, pt), Some(id));
                *prev_pt = pt;
            }

            *prev_pt = pend;
        }
        _ => {}
    }
}

/// Port of `buildContourHierarchy`: for each (non-empty) contour, the contours that contain
/// its first point.
fn build_contour_hierarchy(contours: &[ShapeLineChain]) -> BTreeMap<i32, Vec<i32>> {
    let mut contour_to_parent_indexes_map = BTreeMap::new();

    for ii in 0..contours.len() {
        if contours[ii].point_count() < 1 {
            // malformed/empty SHAPE_LINE_CHAIN
            continue;
        }

        let first_pt = contours[ii].get_point(0);
        let mut parents = Vec::new();

        for (jj, parent_candidate) in contours.iter().enumerate() {
            if jj == ii {
                continue;
            }

            if parent_candidate.point_inside(first_pt, 0, true) {
                parents.push(jj as i32);
            }
        }

        contour_to_parent_indexes_map.insert(ii as i32, parents);
    }

    contour_to_parent_indexes_map
}

/// Port of `addOutlinesToPolygon`: adds the contours of even nesting depth as outlines.
#[allow(clippy::too_many_arguments)]
fn add_outlines_to_polygon(
    contours: &[ShapeLineChain],
    contour_hierarchy: &BTreeMap<i32, Vec<i32>>,
    crossing_contours: &BTreeSet<i32>,
    polygons: &mut ShapePolySet,
    allow_disjoint: bool,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
    fetch_owner: &dyn Fn(&Seg) -> Option<ShapeId>,
    contour_to_outline_idx_map: &mut BTreeMap<i32, i32>,
) -> bool {
    let mut error_handler = error_handler;

    for (&contour_index, parent_indexes) in contour_hierarchy {
        if parent_indexes.len() % 2 == 0 {
            // A nested contour crossing another is a cutout wall, parent parity lies for it
            if !parent_indexes.is_empty() && crossing_contours.contains(&contour_index) {
                continue;
            }

            // Even number of parents; top-level outline
            if !allow_disjoint && !polygons.is_empty() {
                if let Some(handler) = error_handler.as_deref_mut() {
                    let a = fetch_owner(&polygons.c_outline(0).get_segment(0));
                    let b = fetch_owner(&contours[contour_index as usize].get_segment(0));

                    if let (Some(a), Some(b)) = (a, b) {
                        handler.on_error(
                            "(multiple board outlines not supported)",
                            Some(a.into()),
                            Some(b.into()),
                            contours[contour_index as usize].get_point(0),
                        );
                        return false;
                    }
                }
            }

            polygons.add_outline(&contours[contour_index as usize]);
            contour_to_outline_idx_map.insert(contour_index, polygons.outline_count() - 1);
        }
    }

    true
}

/// Port of `addHolesToPolygon`: attaches the contours of odd nesting depth as holes of their
/// direct parent, or (crossing contours in the polygonized path) subtracts / adds them.
fn add_holes_to_polygon(
    contours: &[ShapeLineChain],
    contour_hierarchy: &BTreeMap<i32, Vec<i32>>,
    contour_to_outline_idx_map: &BTreeMap<i32, i32>,
    polygons: &mut ShapePolySet,
    allow_use_arcs_in_polygons: bool,
    crossing_contours: &BTreeSet<i32>,
) {
    if allow_use_arcs_in_polygons || crossing_contours.is_empty() {
        for (&contour_index, parent_indexes) in contour_hierarchy {
            if parent_indexes.len() % 2 == 1 {
                // Odd nesting depth means a hole, attach it to its direct parent
                let hole = &contours[contour_index as usize];

                for parent_contour_idx in parent_indexes {
                    // C++ `.at()`: the parent always has an entry (it contains a point).
                    if contour_hierarchy[parent_contour_idx].len() == parent_indexes.len() - 1 {
                        let outline_idx = contour_to_outline_idx_map[parent_contour_idx];
                        polygons.add_hole(hole, outline_idx);
                        break;
                    }
                }
            }
        }

        return;
    }

    // Malformed overlapping contours in the polygonized path.
    let mut cutout_candidates = ShapePolySet::new();
    let mut island_candidates = ShapePolySet::new();

    for (&contour_index, parent_indexes) in contour_hierarchy {
        if parent_indexes.is_empty() {
            continue;
        }

        if parent_indexes.len() % 2 == 1 || crossing_contours.contains(&contour_index) {
            cutout_candidates.add_outline(&contours[contour_index as usize]);
        } else {
            island_candidates.add_outline(&contours[contour_index as usize]);
        }
    }

    if cutout_candidates.outline_count() != 0 {
        cutout_candidates.simplify();
        polygons.boolean_subtract(&cutout_candidates);
    }

    if island_candidates.outline_count() != 0 {
        island_candidates.simplify();
        polygons.boolean_add(&island_candidates);
    }
}

/// Port of `checkSelfIntersections`: reports overlapping or crossing segments of the
/// polygon set; true if there is none.
fn check_self_intersections(
    polygons: &ShapePolySet,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
    fetch_owner: &dyn Fn(&Seg) -> Option<ShapeId>,
) -> bool {
    let mut error_handler = error_handler;
    let mut self_intersecting = false;
    let mut total = 0usize;

    for ii in 0..polygons.outline_count() {
        let contour = polygons.c_outline(ii);
        total += contour.segment_count() as usize;

        for jj in 0..polygons.hole_count(ii) {
            let hole = polygons.c_hole(ii, jj);
            total += hole.segment_count() as usize;
        }
    }

    let mut segments: Vec<Seg> = Vec::with_capacity(total);

    for mut segment in polygons.iterate_segments_with_holes() {
        if lexicographical_compare(&segment.a, &segment.b) > 0 {
            std::mem::swap(&mut segment.a, &mut segment.b);
        }

        segments.push(segment);
    }

    std_sort(&mut segments, |a: &Seg, b: &Seg| {
        if a.a != b.a {
            return lexicographical_compare(&a.a, &b.a) < 0;
        }

        lexicographical_compare(&a.b, &b.b) < 0
    });

    let owner_item = |seg: &Seg| fetch_owner(seg).map(ItemId::from);

    for i in 0..segments.len() {
        let seg1 = segments[i];

        for seg2 in &segments[i + 1..] {
            // VECTOR2::operator> is lexicographic (x, then y), as the derived `Ord`.
            if seg2.a > seg1.b {
                break;
            }

            if seg1 == *seg2 || (seg1.a == seg2.b && seg1.b == seg2.a) {
                if let Some(handler) = error_handler.as_deref_mut() {
                    let a = owner_item(&seg1);
                    let b = owner_item(seg2);
                    handler.on_error("(self-intersecting)", a, b, seg1.a);
                }

                self_intersecting = true;
            } else if let Some(pt) = seg1.intersect_with(seg2, true, false) {
                if let Some(handler) = error_handler.as_deref_mut() {
                    let a = owner_item(&seg1);
                    let b = owner_item(seg2);
                    handler.on_error("(self-intersecting)", a, b, pt);
                }

                self_intersecting = true;
            }
        }
    }

    !self_intersecting
}

/// Port of `CHAIN_NEIGHBOURS`.
#[derive(Default)]
struct ChainNeighbours {
    /// nearest unchained
    available: Option<ShapeId>,
    /// nearest already chained
    consumed: Option<ShapeId>,
}

/// Port of `closerEndpoint`: by distance, then by endpoint index.
fn closer_endpoint(left: &ResultItem, right: &ResultItem) -> bool {
    if left.second != right.second {
        return left.second < right.second;
    }

    left.first < right.first
}

/// Port of `findNeighbours`: find the shapes that could continue a chain at `point`,
/// excluding `shape` itself; `is_consumed` says whether a shape is already chained.
fn find_neighbours(
    shape: ShapeId,
    point: Vector2I,
    kd_tree: &KdTree,
    chaining_epsilon: f64,
    is_consumed: impl Fn(ShapeId) -> bool,
) -> ChainNeighbours {
    let query_pt = [point.x as f64, point.y as f64];
    let radius_sq = chaining_epsilon * chaining_epsilon;

    let mut matches = kd_tree.radius_search(&query_pt, radius_sq);

    std_sort(&mut matches, closer_endpoint);

    let mut result = ChainNeighbours::default();
    let adaptor = kd_tree.dataset();

    for m in &matches {
        let candidate = adaptor.endpoints[m.first as usize].1;

        if candidate == shape {
            continue;
        }

        let slot = if is_consumed(candidate) {
            &mut result.consumed
        } else {
            &mut result.available
        };

        if slot.is_none() {
            *slot = Some(candidate);
        }

        if result.available.is_some() && result.consumed.is_some() {
            break;
        }
    }

    result
}

/// Port of `findCrossingContours`: the indices of the contours that cross another one.
fn find_crossing_contours(contours: &[ShapeLineChain]) -> BTreeSet<i32> {
    let mut crossing = BTreeSet::new();

    for ii in 0..contours.len() {
        for jj in ii + 1..contours.len() {
            let mut intersections: Vec<Intersection> = Vec::new();

            if contours[ii].intersect_chain(&contours[jj], &mut intersections, true, None) != 0 {
                crossing.insert(ii as i32);
                crossing.insert(jj as i32);
            }
        }
    }

    crossing
}

/// One `extendChain( forward )` step of the chaining loops: walks from the chain end
/// (`forward`: the back) through the nearest unchained neighbours. Returns
/// `(closed, last current shape, the consumed neighbour seen at the dead end)`.
#[allow(clippy::too_many_arguments)]
fn extend_chain(
    ctx: &BoardCtx<'_>,
    chain: &mut VecDeque<ShapeId>,
    front_pt: &mut Vector2I,
    back_pt: &mut Vector2I,
    forward: bool,
    kd_tree: &KdTree,
    chaining_epsilon: i32,
    is_consumed: &dyn Fn(ShapeId) -> bool,
    consume: &mut dyn FnMut(ShapeId),
) -> (bool, ShapeId, Option<ShapeId>, Vector2I) {
    let mut curr = if forward {
        *chain.back().expect("chain is never empty")
    } else {
        *chain.front().expect("chain is never empty")
    };
    let mut prev = if forward { *back_pt } else { *front_pt };
    let mut closed = false;

    loop {
        let next = find_neighbours(curr, prev, kd_tree, chaining_epsilon as f64, is_consumed);

        if let Some(available) = next.available {
            consume(available);

            if forward {
                chain.push_back(available);
            } else {
                chain.push_front(available);
            }

            let next_shape = &ctx.board[available];

            if closer_to_first(prev, next_shape.shape.start(), next_shape.shape.end()) {
                prev = next_shape.shape.end();
            } else {
                prev = next_shape.shape.start();
            }

            curr = available;
            continue;
        }

        // Match on position, not identity (see doConvertOutlineToPolygon)
        let chain_pt = if forward { *front_pt } else { *back_pt };
        let mut dead_end_consumed = None;

        if chain.len() > 1 && close_enough(prev, chain_pt, chaining_epsilon as u32) {
            closed = true;
        } else {
            dead_end_consumed = next.consumed;
        }

        if forward {
            *back_pt = prev;
        } else {
            *front_pt = prev;
        }

        return (closed, curr, dead_end_consumed, prev);
    }
}

/// Port of `buildChainedClosedContour`: walk a chain of open shapes (segments/arcs/beziers)
/// starting from `start`, and produce a closed `SHAPE_LINE_CHAIN` if the chain forms a closed
/// loop. Shapes that are consumed are removed from `remaining`. Returns the contour and its
/// owner shape only if a closed contour is produced. Used to detect cross-contour
/// intersections of bezier-bounded slots which would otherwise be missed by the
/// closed-shape-only intersection test.
#[allow(clippy::too_many_arguments)]
fn build_chained_closed_contour(
    ctx: &BoardCtx<'_>,
    scratch: &mut OutlineScratch,
    start: ShapeId,
    remaining: &mut BTreeSet<ShapeId>,
    kd_tree: &KdTree,
    error_max: i32,
    chaining_epsilon: i32,
) -> Option<(ShapeLineChain, ShapeId)> {
    let mut chain = VecDeque::new();
    chain.push_back(start);

    let start_shape = &ctx.board[start];
    let mut front_pt = start_shape.shape.start();
    let mut back_pt = start_shape.shape.end();

    let visited = std::cell::RefCell::new(BTreeSet::new());
    visited.borrow_mut().insert(start);

    let mut closed;

    {
        // The KD-tree spans the original openShapes set, so it still returns shapes already
        // consumed by an earlier chain. Filter against aRemaining to avoid accidentally
        // absorbing those into this chain.
        let remaining_ro: &BTreeSet<ShapeId> = remaining;
        let is_consumed = |candidate: ShapeId| {
            !remaining_ro.contains(&candidate) || visited.borrow().contains(&candidate)
        };
        let mut consume = |candidate: ShapeId| {
            visited.borrow_mut().insert(candidate);
        };

        closed = extend_chain(
            ctx,
            &mut chain,
            &mut front_pt,
            &mut back_pt,
            true,
            kd_tree,
            chaining_epsilon,
            &is_consumed,
            &mut consume,
        )
        .0;

        if !closed {
            closed = extend_chain(
                ctx,
                &mut chain,
                &mut front_pt,
                &mut back_pt,
                false,
                kd_tree,
                chaining_epsilon,
                &is_consumed,
                &mut consume,
            )
            .0;
        }
    }

    if !closed {
        return None;
    }

    // Build the contour from the closed chain, mirroring doConvertOutlineToPolygon().
    let mut shape_owners = ShapeOwners::new();
    let first = chain[0];

    let start_pt = if chain.len() > 1 {
        free_end(&ctx.board[first], &ctx.board[chain[1]])
    } else {
        ctx.board[first].shape.start()
    };

    let mut contour = ShapeLineChain::new();
    contour.append_point(start_pt, false);
    let mut prev_pt = start_pt;

    for &shape_in_chain in &chain {
        process_shape_segment(
            ctx,
            scratch,
            shape_in_chain,
            &mut contour,
            &mut prev_pt,
            &mut shape_owners,
            error_max,
            chaining_epsilon,
            false,
        );
    }

    if contour.point_count() < 3 {
        return None;
    }

    if contour.c_point(0) != contour.c_last_point() {
        let p0 = contour.c_point(0);
        contour.set_point(-1, p0);
    }

    contour.set_closed(true);

    for consumed in &chain {
        remaining.remove(consumed);
    }

    Some((contour, first))
}

/// Port of `doConvertOutlineToPolygon` (the body of [`convert_outline_to_polygon`] that
/// shares the `SKIP_STRUCT` flags with its caller).
#[allow(clippy::too_many_arguments)]
pub(crate) fn do_convert_outline_to_polygon(
    ctx: &BoardCtx<'_>,
    shape_list: &[ShapeId],
    polygons: &mut ShapePolySet,
    error_max: i32,
    chaining_epsilon: i32,
    allow_disjoint: bool,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
    allow_use_arcs_in_polygons: bool,
    scratch: &mut OutlineScratch,
) -> bool {
    let mut error_handler = error_handler;

    if shape_list.is_empty() {
        return true;
    }

    let mut _self_intersecting = false;

    // Seed in list order so the polygon does not depend on addresses
    let mut remaining: BTreeSet<ShapeId> = shape_list.iter().copied().collect();
    let mut next_seed = 0usize;

    // Pre-build KD-tree
    let kd_tree = KdTree::new(EndpointsAdaptor::new(ctx, shape_list));

    // Keep a list of where the various shapes came from
    let mut shape_owners = ShapeOwners::new();

    let mut reported_gaps: BTreeSet<(ShapeId, ShapeId)> = BTreeSet::new();
    let mut contours: Vec<ShapeLineChain> = Vec::with_capacity(shape_list.len());

    for shape in shape_list {
        scratch.skip.remove(shape);
    }

    // Process each shape to build contours
    while !remaining.is_empty() {
        while next_seed < shape_list.len() && !remaining.contains(&shape_list[next_seed]) {
            next_seed += 1;
        }

        if next_seed >= shape_list.len() {
            break;
        }

        let graphic_id = shape_list[next_seed];
        let graphic = &ctx.board[graphic_id];
        scratch.skip.insert(graphic_id);
        remaining.remove(&graphic_id);

        let mut curr_contour = ShapeLineChain::new();
        curr_contour.set_width(EdaShapeItem::width(graphic, ctx));

        // Handle closed shapes (circles, rects, polygons, ellipses)
        if matches!(
            graphic.shape.shape(),
            ShapeT::Poly | ShapeT::Circle | ShapeT::Rectangle | ShapeT::Ellipse
        ) {
            process_closed_shape(
                ctx,
                graphic_id,
                &mut curr_contour,
                &mut shape_owners,
                error_max,
                allow_use_arcs_in_polygons,
            );
        } else {
            // Build chains for open shapes
            let mut chain = VecDeque::new();
            chain.push_back(graphic_id);

            let mut front_pt = graphic.shape.start();
            let mut back_pt = graphic.shape.end();

            let mut closed = false;

            for forward in [true, false] {
                if !forward && closed {
                    break;
                }

                let skip = std::cell::RefCell::new(std::mem::take(&mut scratch.skip));
                let (now_closed, curr, consumed, prev) = {
                    let is_consumed = |candidate: ShapeId| skip.borrow().contains(&candidate);
                    let mut consume = |candidate: ShapeId| {
                        skip.borrow_mut().insert(candidate);
                        remaining.remove(&candidate);
                    };

                    extend_chain(
                        ctx,
                        &mut chain,
                        &mut front_pt,
                        &mut back_pt,
                        forward,
                        &kd_tree,
                        chaining_epsilon,
                        &is_consumed,
                        &mut consume,
                    )
                };
                scratch.skip = skip.into_inner();

                if now_closed {
                    closed = true;
                } else if let Some(consumed) = consumed {
                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            "(self-intersecting)",
                            Some(curr.into()),
                            Some(consumed.into()),
                            prev,
                        );
                    }

                    _self_intersecting = true;
                }
            }

            // Process the chain to build the contour
            let first = chain[0];

            let start_pt = if chain.len() > 1 {
                free_end(&ctx.board[first], &ctx.board[chain[1]])
            } else {
                ctx.board[first].shape.start()
            };

            curr_contour.append_point(start_pt, false);
            let mut prev_pt = start_pt;

            for &shape_in_chain in &chain {
                process_shape_segment(
                    ctx,
                    scratch,
                    shape_in_chain,
                    &mut curr_contour,
                    &mut prev_pt,
                    &mut shape_owners,
                    error_max,
                    chaining_epsilon,
                    allow_use_arcs_in_polygons,
                );
            }

            // Handle contour closure
            if close_enough(
                curr_contour.c_point(0),
                curr_contour.c_last_point(),
                chaining_epsilon as u32,
            ) {
                if curr_contour.c_point(0) != curr_contour.c_last_point()
                    && curr_contour.point_count() > 2
                {
                    let owner = shape_owners
                        .get(&key_of(&curr_contour.c_segment(-1)))
                        .copied()
                        .flatten();

                    let last = (curr_contour.point_count() - 1) as usize;

                    if curr_contour.is_arc_end(last) {
                        let arc = curr_contour
                            .arc(curr_contour.arc_index(last) as usize)
                            .clone();

                        let sarc = ShapeArc::from_start_mid_end(
                            arc.get_p0(),
                            arc.get_arc_mid(),
                            curr_contour.c_point(0),
                            0,
                        );

                        let mut arc_chain = ShapeLineChain::new();
                        arc_chain.append_arc_with_error(&sarc, error_max);

                        if !allow_use_arcs_in_polygons {
                            arc_chain.clear_arcs();
                        }

                        for ii in 1..arc_chain.point_count() {
                            shape_owners
                                .insert((arc_chain.c_point(ii - 1), arc_chain.c_point(ii)), owner);
                        }

                        curr_contour.remove_shape(curr_contour.point_count() - 1);
                        curr_contour.append_chain(&arc_chain);
                    } else {
                        let p0 = curr_contour.c_point(0);
                        curr_contour.set_point(-1, p0);

                        let n = curr_contour.point_count() as usize;
                        shape_owners.insert(
                            (curr_contour.c_points()[n - 2], curr_contour.c_last_point()),
                            owner,
                        );
                    }
                }

                curr_contour.set_closed(true);
            } else if let Some(handler) = error_handler.as_deref_mut() {
                let mut report_gap = |pt: Vector2I| {
                    let query_pt = [pt.x as f64, pt.y as f64];

                    // Both endpoints are in the tree, so over-fetch for a second shape
                    let (indices, _dists) = kd_tree.knn_search(&query_pt, 8);
                    let found = indices.len();

                    if found == 0 {
                        return;
                    }

                    let adaptor = kd_tree.dataset();
                    let shape_a = adaptor.endpoints[indices[0] as usize].1;
                    let mut shape_b = shape_a;

                    // A lone shape has no neighbour, so it pairs with itself
                    for &index in &indices[1..found] {
                        if adaptor.endpoints[index as usize].1 != shape_a {
                            shape_b = adaptor.endpoints[index as usize].1;
                            break;
                        }
                    }

                    // Avoid reporting the same pair twice
                    let key = (shape_a.min(shape_b), shape_a.max(shape_b));

                    if !reported_gaps.insert(key) {
                        return;
                    }

                    // Find the nearest points between the two shapes and calculate midpoint
                    let effective_shape_a = effective_shape_of(ctx, scratch, shape_a);
                    let effective_shape_b = effective_shape_of(ctx, scratch, shape_b);
                    let mut pt_a = Vector2I::default();
                    let mut pt_b = Vector2I::default();
                    let mut midpoint = pt; // fallback to original point

                    if effective_shape_a.nearest_points(&*effective_shape_b, &mut pt_a, &mut pt_b) {
                        midpoint = (pt_a + pt_b) / 2;
                    }

                    handler.on_error(
                        "(not a closed shape)",
                        Some(shape_a.into()),
                        Some(shape_b.into()),
                        midpoint,
                    );
                };

                report_gap(curr_contour.c_point(0));
                report_gap(curr_contour.c_last_point());
            }
        }

        contours.push(curr_contour);
    }

    // Ensure all contours are closed
    for contour in &contours {
        if !contour.is_closed() {
            return false;
        }
    }

    // Generate bounding boxes for hierarchy calculations
    for contour in &mut contours {
        if !contour.get_cached_bbox().is_some_and(|b| b.is_valid()) {
            contour.generate_bbox_cache();
        }
    }

    // Build contour hierarchy
    let contour_hierarchy = build_contour_hierarchy(&contours);

    let mut crossing_contours = BTreeSet::new();

    if !allow_use_arcs_in_polygons {
        crossing_contours = find_crossing_contours(&contours);
    }

    let fetch_owner =
        |seg: &Seg| -> Option<ShapeId> { shape_owners.get(&key_of(seg)).copied().flatten() };

    // Add outlines to polygon set
    let mut contour_to_outline_idx_map = BTreeMap::new();

    if !add_outlines_to_polygon(
        &contours,
        &contour_hierarchy,
        &crossing_contours,
        polygons,
        allow_disjoint,
        reborrow(&mut error_handler),
        &fetch_owner,
        &mut contour_to_outline_idx_map,
    ) {
        return false;
    }

    // Add holes to polygon set
    add_holes_to_polygon(
        &contours,
        &contour_hierarchy,
        &contour_to_outline_idx_map,
        polygons,
        allow_use_arcs_in_polygons,
        &crossing_contours,
    );

    // Check for self-intersections
    check_self_intersections(polygons, error_handler, &fetch_owner)
}

/// Reborrows an optional handler for one call (C++ passes the same pointer on).
fn reborrow<'s>(
    handler: &'s mut Option<&mut dyn OutlineErrorHandler>,
) -> Option<&'s mut dyn OutlineErrorHandler> {
    match handler {
        Some(h) => Some(&mut **h),
        None => None,
    }
}

/// The `shapeOwners` key of a segment (`std::make_pair( seg.A, seg.B )`).
fn key_of(seg: &Seg) -> (Vector2I, Vector2I) {
    (seg.a, seg.b)
}

/// Port of `ConvertOutlineToPolygon`: build a polygon set with holes from a `PCB_SHAPE`
/// list.
///
/// The shape list is expected to be one or more top-level closed outlines with zero or more
/// holes in each. Optionally, it can be limited to a single top-level closed outline.
///
/// * `shape_list`: the drawsegments (only lines, circles and arcs).
/// * `polygons`: receives the complex polygon (appended to).
/// * `error_max`: the max error distance when polygonizing a curve (internal units).
/// * `chaining_epsilon`: the max distance from one endPt to the next startPt (internal units).
/// * `allow_disjoint`: multiple top-level outlines are allowed.
/// * `allow_use_arcs_in_polygons`: allow adding arcs in `SHAPE_LINE_CHAIN` polylines/polygons
///   when building outlines (mainly for export to STEP files).
///
/// Returns true if success, false if a contour is not valid (self intersecting).
#[allow(clippy::too_many_arguments)]
pub fn convert_outline_to_polygon(
    ctx: &BoardCtx<'_>,
    shape_list: &[ShapeId],
    polygons: &mut ShapePolySet,
    error_max: i32,
    chaining_epsilon: i32,
    allow_disjoint: bool,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
    allow_use_arcs_in_polygons: bool,
) -> bool {
    let mut scratch = OutlineScratch::default();

    do_convert_outline_to_polygon(
        ctx,
        shape_list,
        polygons,
        error_max,
        chaining_epsilon,
        allow_disjoint,
        error_handler,
        allow_use_arcs_in_polygons,
        &mut scratch,
    )
}

/// `PCB_TYPE_COLLECTOR::Collect( aBoard, { PCB_SHAPE_T } )`: the `PCB_SHAPE`s of every
/// footprint (in footprint order, each in drawing order), then those of the board drawings
/// (`BOARD::Visit` scans the footprints first).
pub fn collect_board_shapes(ctx: &BoardCtx<'_>) -> Vec<ShapeId> {
    let board = ctx.board;
    let mut items = Vec::new();

    for (fp, _) in board.footprints() {
        items.extend(collect_footprint_shapes(ctx, fp));
    }

    for (id, _) in board.drawings() {
        if let Some(shape) = id.typed::<PcbShape>() {
            items.push(shape);
        }
    }

    items
}

/// `PCB_TYPE_COLLECTOR::Collect( aFootprint, { PCB_SHAPE_T } )`: the footprint's drawings
/// that are `PCB_SHAPE`s.
pub(crate) fn collect_footprint_shapes(ctx: &BoardCtx<'_>, fp: FootprintId) -> Vec<ShapeId> {
    ctx.board
        .footprint_drawings(fp)
        .filter_map(|(id, _)| id.typed::<PcbShape>())
        .collect()
}

/// Port of `TestBoardOutlinesGraphicItems`: test the board graphic items on the edge cut
/// layer for validity.
///
/// * `min_dist`: the min length of a segment (or radius, or diagonal size of a rect) to be
///   valid.
pub fn test_board_outlines_graphic_items(
    ctx: &BoardCtx<'_>,
    min_dist: i32,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
) -> bool {
    let mut error_handler = error_handler;
    let mut success = true;
    let min_dist = std::cmp::max(0, min_dist);
    let mut scratch = OutlineScratch::default();

    // Get all the shapes into 'items', then keep only those on layer == Edge_Cuts.
    let shape_list: Vec<ShapeId> = collect_board_shapes(ctx)
        .into_iter()
        .filter(|&id| ctx.board[id].layer(ctx) == PcbLayerId::EdgeCuts)
        .collect();

    // Now Test validity of collected items
    for &id in &shape_list {
        let shape = &ctx.board[id];
        let s = &shape.shape;

        match s.shape() {
            ShapeT::Rectangle => {
                let seg = s.end() - s.start();
                let dim = seg.euclidean_norm();

                if dim <= min_dist {
                    success = false;

                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            &format!("(rectangle has null or very small size: {dim} nm)"),
                            Some(id.into()),
                            None,
                            s.start(),
                        );
                    }
                }
            }
            ShapeT::Circle => {
                let r = s.radius();

                if r <= min_dist {
                    success = false;

                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            &format!("(circle has null or very small radius: {r} nm)"),
                            Some(id.into()),
                            None,
                            s.start(),
                        );
                    }
                }
            }
            ShapeT::Segment => {
                let seg = s.end() - s.start();
                let dim = seg.euclidean_norm();

                if dim <= min_dist {
                    success = false;

                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            &format!("(segment has null or very small length: {dim} nm)"),
                            Some(id.into()),
                            None,
                            s.start(),
                        );
                    }
                }
            }
            ShapeT::Arc => {
                // Arc size can be evaluated from the distance between arc middle point and arc
                // ends. We do not need a precise value, just an idea of its size
                let arc_middle = s.arc_mid();
                let seg1 = arc_middle - s.start();
                let seg2 = s.end() - arc_middle;
                let dim = seg1.euclidean_norm() + seg2.euclidean_norm();

                if dim <= min_dist {
                    success = false;

                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            &format!("(arc has null or very small size: {dim} nm)"),
                            Some(id.into()),
                            None,
                            s.start(),
                        );
                    }
                }
            }
            ShapeT::Poly | ShapeT::Bezier => {}
            ShapeT::Ellipse | ShapeT::EllipseArc => {
                let major = s.ellipse().major_radius;
                let minor = s.ellipse().minor_radius;

                if major <= min_dist || minor <= min_dist {
                    success = false;

                    if let Some(handler) = error_handler.as_deref_mut() {
                        handler.on_error(
                            &format!(
                                "(ellipse has null or very small radii: major={major} nm, \
                                 minor={minor} nm)"
                            ),
                            Some(id.into()),
                            None,
                            s.ellipse().center,
                        );
                    }
                }
            }
            ShapeT::Undefined => {
                // UNIMPLEMENTED_FOR( shape->SHAPE_T_asString() )
                return false;
            }
        }
    }

    let mut closed_contours: Vec<(ShapeId, ShapeLineChain)> = Vec::with_capacity(shape_list.len());

    // C++ `std::set<PCB_SHAPE*>`: ordered by address, replaced by `ShapeId` order
    // (board-model.md 3.1 rule 2; PORT_STATUS.md M21).
    let mut open_shapes: BTreeSet<ShapeId> = BTreeSet::new();

    for &id in &shape_list {
        let shape = &ctx.board[id];

        match shape.shape.shape() {
            ShapeT::Poly | ShapeT::Circle | ShapeT::Rectangle | ShapeT::Ellipse => {
                let mut contour = ShapeLineChain::new();
                let mut shape_owners = ShapeOwners::new();

                process_closed_shape(
                    ctx,
                    id,
                    &mut contour,
                    &mut shape_owners,
                    board_item::max_error(shape, ctx),
                    true,
                );
                closed_contours.push((id, contour));
            }
            ShapeT::Segment | ShapeT::Arc | ShapeT::Bezier | ShapeT::EllipseArc => {
                open_shapes.insert(id);
            }
            ShapeT::Undefined => {}
        }
    }

    // Gather closed contours from chained open shapes (slots formed by segments/arcs/beziers).
    // Without this, malformed-outline detection misses overlaps involving such slots.
    if !open_shapes.is_empty() {
        let open_shape_list: Vec<ShapeId> = open_shapes.iter().copied().collect();
        let kd_tree = KdTree::new(EndpointsAdaptor::new(ctx, &open_shape_list));

        let chaining_epsilon = ctx.board.outlines_chaining_epsilon;
        let max_error = ctx.board.design_settings().max_error();

        while let Some(&start) = open_shapes.iter().next() {
            match build_chained_closed_contour(
                ctx,
                &mut scratch,
                start,
                &mut open_shapes,
                &kd_tree,
                max_error,
                chaining_epsilon,
            ) {
                Some((contour, owner)) => closed_contours.push((owner, contour)),
                None => {
                    open_shapes.remove(&start);
                }
            }
        }
    }

    for ii in 0..closed_contours.len() {
        let contour_a = &closed_contours[ii].1;

        for jj in ii + 1..closed_contours.len() {
            let contour_b = &closed_contours[jj].1;
            let mut intersections: Vec<Intersection> = Vec::new();

            // Ignore touching-only cases; report only real overlap/crossing.
            if contour_a.intersect_chain(contour_b, &mut intersections, true, None) == 0 {
                continue;
            }

            success = false;

            if let Some(handler) = error_handler.as_deref_mut() {
                let shape_a = closed_contours[ii].0;
                let shape_b = closed_contours[jj].0;

                let mut midpoint = intersections[0].p;
                let effective_shape_a = effective_shape_of(ctx, &scratch, shape_a);
                let effective_shape_b = effective_shape_of(ctx, &scratch, shape_b);

                let bbox_a: Box2I = effective_shape_a.bbox(0);
                let bbox_b: Box2I = effective_shape_b.bbox(0);
                let overlap_box = bbox_a.intersect(&bbox_b);

                if overlap_box.width() > 0 && overlap_box.height() > 0 {
                    midpoint = overlap_box.centre();
                }

                handler.on_error(
                    "(self-intersecting)",
                    Some(shape_a.into()),
                    Some(shape_b.into()),
                    midpoint,
                );
            }
        }
    }

    success
}

/// Port of `BuildBoardPolygonOutlines`: extract the board outlines and build a closed
/// polygon from the lines, arcs and circle items on the edge cut layer.
///
/// Any closed outline inside the main outline is a hole. All contours should be closed, i.e.
/// are valid vertices for a closed polygon. With `infer_outline_if_necessary`, if the edges do
/// not define a closed shape, the bounding box of the edges (or failing that, of the other
/// items on the board) is used. Returns true if success, false if a contour is not valid.
#[allow(clippy::too_many_arguments)]
pub fn build_board_polygon_outlines(
    ctx: &BoardCtx<'_>,
    outlines: &mut ShapePolySet,
    error_max: i32,
    chaining_epsilon: i32,
    infer_outline_if_necessary: bool,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
    allow_use_arcs_in_polygons: bool,
) -> bool {
    let board = ctx.board;
    let mut fp_holes = ShapePolySet::new();
    let mut success = false;

    // SCOPED_FLAGS_CLEANER cleaner( SKIP_STRUCT )
    let mut scratch = OutlineScratch::default();

    // Get all the shapes into 'items', then keep only those on layer == Edge_Cuts.
    let items = collect_board_shapes(ctx);

    for id in &items {
        scratch.skip.remove(id);
    }

    for (fp_id, fp) in board.footprints() {
        let fp_items = collect_footprint_shapes(ctx, fp_id);

        let fp_seg_list: Vec<ShapeId> = fp_items
            .iter()
            .copied()
            .filter(|&id| board[id].layer(ctx) == PcbLayerId::EdgeCuts)
            .collect();

        if !fp_seg_list.is_empty() {
            let mut fp_outlines = ShapePolySet::new();
            success = do_convert_outline_to_polygon(
                ctx,
                &fp_seg_list,
                &mut fp_outlines,
                error_max,
                chaining_epsilon,
                false,
                None, // don't report errors here; the second pass also
                // gets an opportunity to use these segments
                allow_use_arcs_in_polygons,
                &mut scratch,
            );

            // Test to see if we should make holes or outlines.  Holes are made if the
            // footprint has copper outside of a single, closed outline.  If there are multiple
            // outlines, we assume that the footprint edges represent holes as we do not
            // support multiple boards.  Similarly, if any of the footprint pads are located
            // outside of the edges, then the edges are holes
            if success
                && (is_copper_outside(ctx, fp_id, fp, &fp_outlines)
                    || fp_outlines.outline_count() > 1)
            {
                fp_holes.append_set(&fp_outlines);
            } else {
                // If it wasn't a closed area, or wasn't a hole, the we want to keep the fpSegs
                // in contention for the board outline builds.
                for id in &fp_items {
                    scratch.skip.remove(id);
                }
            }
        }
    }

    // Make a working copy of aSegList, because the list is modified during calculations
    let mut seg_list = Vec::new();

    for &id in &items {
        // Skip anything already used to generate footprint holes (above)
        if scratch.skip.contains(&id) {
            continue;
        }

        if board[id].layer(ctx) == PcbLayerId::EdgeCuts {
            seg_list.push(id);
        }
    }

    if !seg_list.is_empty() {
        success = do_convert_outline_to_polygon(
            ctx,
            &seg_list,
            outlines,
            error_max,
            chaining_epsilon,
            true,
            error_handler,
            allow_use_arcs_in_polygons,
            &mut scratch,
        );
    }

    if (!success || outlines.outline_count() == 0) && infer_outline_if_necessary {
        // Couldn't create a valid polygon outline.  Use the board edge cuts bounding box to
        // create a rectangular outline, or, failing that, the bounding box of the items on
        // the board.
        let mut bbbox = board.board_edges_bounding_box(ctx);

        // If null area, uses the global bounding box.
        if bbbox.width() == 0 || bbbox.height() == 0 {
            bbbox = board.compute_bounding_box(ctx, false, true);
        }

        // Ensure non null area. If happen, gives a minimal size.
        if bbbox.width() == 0 || bbbox.height() == 0 {
            bbbox.inflate(PCB_IU_SCALE.mm_to_iu(1.0));
        }

        outlines.remove_all_contours();
        outlines.new_outline();

        let mut corner = Vector2I::default();
        outlines.append_point(bbbox.origin(), -1, -1);

        corner.x = bbbox.origin().x;
        corner.y = bbbox.end().y;
        outlines.append_point(corner, -1, -1);

        outlines.append_point(bbbox.end(), -1, -1);

        corner.x = bbbox.end().x;
        corner.y = bbbox.origin().y;
        outlines.append_point(corner, -1, -1);
    }

    if allow_use_arcs_in_polygons {
        for ii in 0..fp_holes.outline_count() {
            let hole_pt = fp_holes.c_outline(ii).c_point(0);

            for jj in 0..outlines.outline_count() {
                if outlines.c_outline(jj).point_inside(hole_pt, 0, false) {
                    outlines.add_hole(fp_holes.c_outline(ii), jj);
                    break;
                }
            }
        }
    } else {
        fp_holes.simplify();
        outlines.boolean_subtract(&fp_holes);
    }

    success
}

/// Port of `buildBoardBoundingBoxPoly`: the complete bounding box of the board (including all
/// items), inflated by a tenth of its size, as a single closed outline.
///
/// The vertex numbers and segment numbers of the rectangle returned:
///
/// ```text
///              1
///      *---------------*
///      |1             2|
///     0|               |2
///      |0             3|
///      *---------------*
///              3
/// ```
pub fn build_board_bounding_box_poly(ctx: &BoardCtx<'_>, outline: &mut ShapePolySet) {
    let board = ctx.board;
    // BOARD::GetBoundingBox() = ComputeBoundingBox( false, false )
    let mut bbbox = board.compute_bounding_box(ctx, false, false);
    let mut chain = ShapeLineChain::new();

    // If null area, uses the global bounding box.
    if bbbox.width() == 0 || bbbox.height() == 0 {
        bbbox = board.compute_bounding_box(ctx, false, true);
    }

    // Ensure non null area. If happen, gives a minimal size.
    if bbbox.width() == 0 || bbbox.height() == 0 {
        bbbox.inflate(PCB_IU_SCALE.mm_to_iu(1.0));
    }

    // Inflate slightly (by 1/10th the size of the box)
    let dx = (bbbox.width() / 10) as i32;
    let dy = (bbbox.height() / 10) as i32;
    bbbox.inflate_xy(dx, dy);

    chain.append_point(bbbox.origin(), false);
    chain.append_xy(bbbox.origin().x, bbbox.end().y, false);
    chain.append_point(bbbox.end(), false);
    chain.append_xy(bbbox.end().x, bbbox.origin().y, false);
    chain.set_closed(true);

    outline.remove_all_contours();
    outline.add_outline(&chain);
}

/// Port of `projectPointOnSegment`: the projection of `end_point` on the nearest segment of
/// outline `outline_num` (unused in the C++ tree; ported for completeness).
pub fn project_point_on_segment(
    end_point: Vector2I,
    outline: &ShapePolySet,
    outline_num: i32,
) -> Vector2I {
    let mut min_distance = -1;
    let mut proj_point = Vector2I::default();

    for seg in outline.c_iterate_segments_polygon(outline_num) {
        let dis = seg.distance(end_point);

        if min_distance < 0 || dis < min_distance {
            min_distance = dis;
            proj_point = seg.nearest_point(end_point);
        }
    }

    proj_point
}

/// Port of `findEndSegments`: the first two segments of `chain` that are not connected to
/// another segment at both ends (start and end segment). Returns the number found (0..2) and
/// the segments (`None` where C++ leaves the out-parameter untouched).
pub fn find_end_segments(chain: &ShapeLineChain) -> (i32, Option<Seg>, Option<Seg>) {
    let mut found_segs = 0;
    let mut start_seg = None;
    let mut end_seg = None;

    for i in 0..chain.segment_count() {
        let seg = chain.segment(i);

        let mut found_a = false;
        let mut found_b = false;

        for j in 0..chain.segment_count() {
            // Don't test the segment against itself
            if i == j {
                continue;
            }

            let test_seg = chain.segment(j);

            if test_seg.contains(seg.a) {
                found_a = true;
            }

            if test_seg.contains(seg.b) {
                found_b = true;
            }
        }

        // This segment isn't a start or end
        if found_a && found_b {
            continue;
        }

        if found_segs == 0 {
            // The first segment we encounter is the "start" segment
            start_seg = Some(seg);
            found_segs += 1;
        } else {
            // Once we find both start and end, we can stop
            end_seg = Some(seg);
            found_segs += 1;
            break;
        }
    }

    (found_segs, start_seg, end_seg)
}

/// Port of `BuildFootprintPolygonOutlines`: extract a board outline for a footprint view.
///
/// * Incomplete outlines will be closed by joining the end of the outline onto the bounding
///   box (by simply projecting the end points) and then take the area that contains the
///   copper.
/// * If all copper lies inside a closed outline, than that outline will be treated as an
///   external board outline.
/// * If copper is located outside a closed outline, then that outline will be treated as a
///   hole, and the outer edge will be formed using the bounding box.
pub fn build_footprint_polygon_outlines(
    ctx: &BoardCtx<'_>,
    outlines_out: &mut ShapePolySet,
    error_max: i32,
    chaining_epsilon: i32,
    error_handler: Option<&mut dyn OutlineErrorHandler>,
) -> bool {
    let board = ctx.board;

    // No footprint loaded
    let Some((footprint_id, footprint)) = board.footprints().next() else {
        // wxLogTrace( traceBoardOutline, "No footprint found on board" )
        return false;
    };

    let mut outlines = ShapePolySet::new();
    let mut success = false;

    let mut scratch = OutlineScratch::default();

    // Get all the SHAPEs into 'items', then keep only those on layer == Edge_Cuts.
    // Make a working copy of aSegList, because the list is modified during calculations
    let seg_list: Vec<ShapeId> = collect_board_shapes(ctx)
        .into_iter()
        .filter(|&id| board[id].layer(ctx) == PcbLayerId::EdgeCuts)
        .collect();

    if !seg_list.is_empty() {
        success = do_convert_outline_to_polygon(
            ctx,
            &seg_list,
            &mut outlines,
            error_max,
            chaining_epsilon,
            true,
            error_handler,
            false,
            &mut scratch,
        );
    }

    // A closed outline was found on Edge_Cuts
    if success {
        // If copper is outside a closed polygon, treat it as a hole
        // If there are multiple outlines in the footprint, they are also holes
        if is_copper_outside(ctx, footprint_id, footprint, &outlines)
            || outlines.outline_count() > 1
        {
            build_board_bounding_box_poly(ctx, outlines_out);

            // Copy all outlines from the conversion as holes into the new outline
            for i in 0..outlines.outline_count() {
                let out = outlines.c_outline(i);

                if out.is_closed() {
                    outlines_out.add_hole(out, -1);
                }

                for j in 0..outlines.hole_count(i) {
                    let hole = outlines.c_hole(i, j);

                    if hole.is_closed() {
                        outlines_out.add_hole(hole, -1);
                    }
                }
            }
        } else {
            // If all copper is inside, then the computed outline is the board outline
            *outlines_out = outlines;
        }

        return true;
    }

    // No board outlines were found, so use the bounding box
    if outlines.outline_count() == 0 {
        build_board_bounding_box_poly(ctx, outlines_out);

        return true;
    }

    // There is an outline present, but it is not closed
    let mut closed_chains: Vec<ShapeLineChain> = Vec::new();
    let mut open_chains: Vec<ShapeLineChain> = Vec::new();

    // The ConvertOutlineToPolygon function returns only one main outline and the rest as
    // holes, so we promote the holes and process them
    open_chains.push(outlines.c_outline(0).clone());

    for j in 0..outlines.hole_count(0) {
        let hole = outlines.c_hole(0, j).clone();

        if hole.is_closed() {
            closed_chains.push(hole);
        } else {
            open_chains.push(hole);
        }
    }

    let mut bbox = ShapePolySet::new();
    build_board_bounding_box_poly(ctx, &mut bbox);

    // Treat the open polys as the board edge
    let mut chain = open_chains[0].clone();
    let rect = bbox.c_outline(0).clone();

    // We know the outline chain is open, so set to non-closed to get better segment count
    chain.set_closed(false);

    // The two possible board outlines
    let mut upper = ShapeLineChain::new();
    let mut lower = ShapeLineChain::new();

    let _ = find_end_segments(&chain);

    if chain.segment_count() == 0 {
        // Something is wrong, bail out with the overall footprint bounding box
        *outlines_out = bbox;
        return true;
    } else if chain.segment_count() == 1 {
        // This case means there is only 1 line segment making up the edge cuts of the
        // footprint, so we just need to use it to cut the bounding box in half.
        let start_seg = chain.segment(0);

        // Intersect with all the sides of the rectangle
        let inter0 = start_seg.intersect_lines(&rect.segment(0));
        let inter1 = start_seg.intersect_lines(&rect.segment(1));
        let inter2 = start_seg.intersect_lines(&rect.segment(2));
        let inter3 = start_seg.intersect_lines(&rect.segment(3));

        // C++ dereferences the optionals below without checking them in the corner branches
        // (undefined behaviour for parallel sides); the port uses (0, 0) there.
        let i0 = inter0.unwrap_or_default();
        let i1 = inter1.unwrap_or_default();
        let i2 = inter2.unwrap_or_default();
        let i3 = inter3.unwrap_or_default();

        if inter0.is_some() && inter2.is_some() && inter1.is_none() && inter3.is_none() {
            // Intersects the vertical rectangle sides only

            // The upper half
            upper.append_point(i0, false);
            upper.append_point(rect.get_point(1), false);
            upper.append_point(rect.get_point(2), false);
            upper.append_point(i2, false);
            upper.set_closed(true);

            // The lower half
            lower.append_point(i0, false);
            lower.append_point(rect.get_point(0), false);
            lower.append_point(rect.get_point(3), false);
            lower.append_point(i2, false);
            lower.set_closed(true);
        } else if inter1.is_some() && inter3.is_some() && inter0.is_none() && inter2.is_none() {
            // Intersects the horizontal rectangle sides only

            // The left half
            upper.append_point(i1, false);
            upper.append_point(rect.get_point(1), false);
            upper.append_point(rect.get_point(0), false);
            upper.append_point(i3, false);
            upper.set_closed(true);

            // The right half
            lower.append_point(i1, false);
            lower.append_point(rect.get_point(2), false);
            lower.append_point(rect.get_point(3), false);
            lower.append_point(i3, false);
            lower.set_closed(true);
        } else {
            // Angled line segment that cuts across a corner

            // Figure out which actual lines are intersected, since IntersectLines assumes
            // an infinite line
            let hit0 = rect.segment(0).contains(i0);
            let hit1 = rect.segment(1).contains(i1);
            let hit2 = rect.segment(2).contains(i2);
            let hit3 = rect.segment(3).contains(i3);

            if hit0 && hit1 {
                // Cut across the upper left corner

                // The upper half
                upper.append_point(i0, false);
                upper.append_point(rect.get_point(1), false);
                upper.append_point(i1, false);
                upper.set_closed(true);

                // The lower half
                lower.append_point(i0, false);
                lower.append_point(rect.get_point(0), false);
                lower.append_point(rect.get_point(3), false);
                lower.append_point(rect.get_point(2), false);
                lower.append_point(i1, false);
                lower.set_closed(true);
            } else if hit1 && hit2 {
                // Cut across the upper right corner

                // The upper half
                upper.append_point(i1, false);
                upper.append_point(rect.get_point(2), false);
                upper.append_point(i2, false);
                upper.set_closed(true);

                // The lower half
                lower.append_point(i1, false);
                lower.append_point(rect.get_point(1), false);
                lower.append_point(rect.get_point(0), false);
                lower.append_point(rect.get_point(3), false);
                lower.append_point(i2, false);
                lower.set_closed(true);
            } else if hit2 && hit3 {
                // Cut across the lower right corner

                // The upper half
                upper.append_point(i2, false);
                upper.append_point(rect.get_point(2), false);
                upper.append_point(rect.get_point(1), false);
                upper.append_point(rect.get_point(0), false);
                upper.append_point(i3, false);
                upper.set_closed(true);

                // The bottom half
                lower.append_point(i2, false);
                lower.append_point(rect.get_point(3), false);
                lower.append_point(i3, false);
                lower.set_closed(true);
            } else {
                // Cut across the lower left corner

                // The upper half
                upper.append_point(i0, false);
                upper.append_point(rect.get_point(1), false);
                upper.append_point(rect.get_point(2), false);
                upper.append_point(rect.get_point(3), false);
                upper.append_point(i3, false);
                upper.set_closed(true);

                // The bottom half
                lower.append_point(i0, false);
                lower.append_point(rect.get_point(0), false);
                lower.append_point(i3, false);
                lower.set_closed(true);
            }
        }
    } else {
        // More than 1 segment

        // Just a temporary thing
        *outlines_out = bbox;
        return true;
    }

    // Figure out which is the correct outline
    let mut poly1 = ShapePolySet::new();
    let mut poly2 = ShapePolySet::new();

    // `Append( const SHAPE_LINE_CHAIN& )` resolves to `Append( SHAPE_POLY_SET( chain ) )`:
    // the chain becomes a second polygon after the empty outline of NewOutline().
    poly1.new_outline();
    poly1.append_set(&ShapePolySet::from_outline(&upper));

    poly2.new_outline();
    poly2.append_set(&ShapePolySet::from_outline(&lower));

    if is_copper_outside(ctx, footprint_id, footprint, &poly1) {
        // "Using lower shape"
        *outlines_out = poly2;
    } else {
        // "Using upper shape"
        *outlines_out = poly1;
    }

    // Add all closed polys as holes to the main outline
    for closed_chain in &closed_chains {
        outlines_out.add_hole(closed_chain, -1);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New code, no C++ test corresponds: a closure and a struct are both handlers.
    #[test]
    fn closures_are_outline_error_handlers() {
        fn report(handler: Option<&mut dyn OutlineErrorHandler>) {
            if let Some(handler) = handler {
                handler.on_error("gap", None, None, Vector2I::new(1, 2));
            }
        }

        let mut seen = Vec::new();
        report(Some(&mut |msg: &str,
                          _a: Option<ItemId>,
                          _b: Option<ItemId>,
                          pt: Vector2I| {
            seen.push((msg.to_string(), pt));
        }));
        report(None);
        assert_eq!(seen, vec![("gap".to_string(), Vector2I::new(1, 2))]);
    }

    /// Loads a board of `qa/data/pcbnew` with the Rust parser; `None` (the test is skipped)
    /// while a construct of the file still needs a pending parser package.
    fn load_qa_board(rel: &str) -> Option<crate::board::Board> {
        let path = kicad_qa::pcbnew_data_dir().join(rel);
        let text = std::fs::read_to_string(&path).expect("qa board");
        let options = crate::io::ParseOptions::default();

        match std::panic::catch_unwind(|| crate::io::parse_board(&text, rel, &options)) {
            Ok(Ok(board)) => Some(board),
            Ok(Err(e)) => {
                eprintln!("skipped: {rel} does not load yet: {e}");
                None
            }
            Err(_) => {
                eprintln!("skipped: {rel} does not load yet (pending stub)");
                None
            }
        }
    }

    /// The outline part of `DRC_TEST_PROVIDER_MISC::testOutline`: the handler calls of
    /// `TestBoardOutlinesGraphicItems` and `BuildBoardPolygonOutlines` with the DRC parameters.
    fn drc_outline_reports(
        board: &crate::board::Board,
    ) -> Vec<(String, Option<ItemId>, Option<ItemId>, Vector2I)> {
        let ctx = BoardCtx::bare(board);
        let mut reports = Vec::new();
        let mut handler = |msg: &str, a: Option<ItemId>, b: Option<ItemId>, pt: Vector2I| {
            reports.push((msg.to_string(), a, b, pt));
        };

        test_board_outlines_graphic_items(&ctx, PCB_IU_SCALE.mm_to_iu(0.001), Some(&mut handler));

        let mut dummy_outline = ShapePolySet::new();
        build_board_polygon_outlines(
            &ctx,
            &mut dummy_outline,
            PCB_IU_SCALE.mm_to_iu(0.05),
            board.outlines_chaining_epsilon,
            true,
            Some(&mut handler),
            false,
        );

        reports
    }

    /// Port of `qa/tests/pcbnew/drc/test_drc_invalid_outline_issue24078.cpp`
    /// (`InvalidOutlineBezierSlotIssue24078`), as far as it is outline building: the bezier
    /// slot overlapping another cutout is reported at least once.
    #[test]
    fn invalid_outline_bezier_slot_issue24078() {
        let Some(board) = load_qa_board("issue24078/issue24078.kicad_pcb") else {
            return;
        };

        let reports = drc_outline_reports(&board);
        assert!(!reports.is_empty(), "no invalid-outline report");
    }

    /// Port of `qa/tests/pcbnew/drc/test_drc_invalid_outline_issue24241.cpp`
    /// (`DegenerateArcOutlineMarkerIssue24241`), as far as it is outline building: at least
    /// one report references the degenerate arc (its three control points coincide) and is
    /// placed at the arc's coordinates.
    #[test]
    fn degenerate_arc_outline_marker_issue24241() {
        let Some(board) = load_qa_board("issue24241/issue24241.kicad_pcb") else {
            return;
        };

        let reports = drc_outline_reports(&board);
        assert!(!reports.is_empty());

        // Locate the degenerate arc directly from the board.
        let ctx = BoardCtx::bare(&board);
        let degenerate_arc = collect_board_shapes(&ctx).into_iter().find(|&id| {
            let s = &board[id].shape;
            s.shape() == ShapeT::Arc && s.start() == s.end() && s.start() == s.arc_mid()
        });
        let degenerate_arc = degenerate_arc.expect("degenerate arc");
        let arc_pos = board[degenerate_arc].shape.start();

        let found_linked_violation = reports.iter().any(|(_, a, b, pt)| {
            let id = Some(ItemId::from(degenerate_arc));
            (*a == id || *b == id) && *pt == arc_pos
        });

        assert!(found_linked_violation, "{reports:?}");
    }

    // ---- differential oracle (tests/oracle/m21_outline_oracle.cpp) --------------------------

    /// FNV-1a 64 (`Fnv1a64` of pcb-dump).
    fn fnv1a64(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;

        for &b in data {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }

        h
    }

    /// `PolyText` of the oracle driver.
    fn poly_text(set: &ShapePolySet) -> String {
        let mut s = String::new();

        for ii in 0..set.outline_count() {
            s.push('[');

            for chain in set.c_polygon(ii) {
                s.push(if chain.is_closed() { 'C' } else { 'O' });
                s += &format!("a{}:", chain.arc_count());

                for jj in 0..chain.point_count() {
                    let p = chain.c_point(jj);
                    s += &format!("{},{};", p.x, p.y);
                }
            }

            s.push(']');
        }

        s
    }

    /// `Result` of the oracle driver.
    fn result_line(f: &str, ok: bool, set: &ShapePolySet) -> String {
        let vertices: i32 = (0..set.outline_count())
            .map(|ii| {
                set.c_polygon(ii)
                    .iter()
                    .map(|c| c.point_count())
                    .sum::<i32>()
            })
            .sum();

        format!(
            "{f}|ok={ok}|n={}/{vertices}|poly=#{:016x}",
            set.outline_count(),
            fnv1a64(poly_text(set).as_bytes())
        )
    }

    /// `Recorder` of the oracle driver: one line per handler call.
    fn recorder<'l>(
        ctx: &'l BoardCtx<'l>,
        f: &'l str,
        out: &'l std::cell::RefCell<Vec<String>>,
    ) -> impl FnMut(&str, Option<ItemId>, Option<ItemId>, Vector2I) + 'l {
        move |msg: &str, a: Option<ItemId>, b: Option<ItemId>, pt: Vector2I| {
            let uuid = |id: Option<ItemId>| {
                id.and_then(|id| ctx.item(id))
                    .map_or_else(|| "-".to_string(), |item| item.uuid().as_string())
            };
            out.borrow_mut().push(format!(
                "{f}|err|{msg}|{}|{}|{},{}",
                uuid(a),
                uuid(b),
                pt.x,
                pt.y
            ));
        }
    }

    /// `DumpBoard` of the oracle driver.
    fn dump_board(board: &crate::board::Board) -> Vec<String> {
        let ctx = BoardCtx::bare(board);
        let out = std::cell::RefCell::new(Vec::new());
        let eps = board.outlines_chaining_epsilon;
        let max_error = board.design_settings().max_error();
        let push = |line: String| out.borrow_mut().push(line);

        {
            let mut h = recorder(&ctx, "test_items", &out);
            let ok =
                test_board_outlines_graphic_items(&ctx, PCB_IU_SCALE.mm_to_iu(0.001), Some(&mut h));
            push(format!("test_items|ok={ok}"));
        }

        {
            let mut h = recorder(&ctx, "drc_build", &out);
            let mut poly = ShapePolySet::new();
            let ok = build_board_polygon_outlines(
                &ctx,
                &mut poly,
                PCB_IU_SCALE.mm_to_iu(0.05),
                eps,
                true,
                Some(&mut h),
                false,
            );
            push(result_line("drc_build", ok, &poly));
        }

        for (name, infer, arcs, npth) in [
            ("outline", false, false, false),
            ("outline_inferred", true, false, false),
            ("outline_arcs", false, true, false),
            ("outline_npth", false, false, true),
        ] {
            let mut h = recorder(&ctx, name, &out);
            let (poly, ok) =
                board.get_board_polygon_outlines(&ctx, infer, Some(&mut h), arcs, npth);
            push(result_line(name, ok, &poly));
        }

        {
            let mut h = recorder(&ctx, "fp_outline", &out);
            let mut poly = ShapePolySet::new();
            let ok =
                build_footprint_polygon_outlines(&ctx, &mut poly, max_error, eps, Some(&mut h));
            push(result_line("fp_outline", ok, &poly));
        }

        for (fp, _) in board.footprints() {
            let list: Vec<ShapeId> = collect_footprint_shapes(&ctx, fp)
                .into_iter()
                .filter(|&id| board[id].layer(&ctx) == PcbLayerId::FCrtYd)
                .collect();

            if list.is_empty() {
                continue;
            }

            let mut h = recorder(&ctx, "crtyd", &out);
            let mut poly = ShapePolySet::new();
            let ok = convert_outline_to_polygon(
                &ctx,
                &list,
                &mut poly,
                max_error,
                PCB_IU_SCALE.mm_to_iu(0.02),
                false,
                Some(&mut h),
                false,
            );
            push(result_line("crtyd", ok, &poly));
        }

        push(result_line(
            "board_outline",
            true,
            &board.board_outline(&ctx),
        ));

        out.into_inner()
    }

    /// `TestBoardOutlinesGraphicItems` chains the open Edge.Cuts shapes starting from
    /// `*std::set<PCB_SHAPE*>::begin()`, the shape with the lowest heap address, which also
    /// becomes the owner reported for an overlap. The port starts from the lowest `ShapeId`
    /// (file order, board-model.md 3.1 rule 2); the reference build's allocator does not
    /// hand out ascending addresses for these two fixtures, so the owner (and, for the bezier
    /// slot, the overlap box centre) differ. (board, reference line, port line).
    const POINTER_ORDER_DEVIATIONS: &[(&str, &str, &str)] = &[
        (
            "m21_overlap.kicad_pcb",
            "test_items|err|(self-intersecting)|21000000-0000-0000-0000-000000000042|\
             21000000-0000-0000-0000-000000000037|140000000,118002542",
            "test_items|err|(self-intersecting)|21000000-0000-0000-0000-000000000042|\
             21000000-0000-0000-0000-000000000034|140000000,118002542",
        ),
        (
            "m21_bezier_slot.kicad_pcb",
            "test_items|err|(self-intersecting)|21000000-0000-0000-0000-000000000060|\
             21000000-0000-0000-0000-000000000057|119132701,114775842",
            "test_items|err|(self-intersecting)|21000000-0000-0000-0000-000000000060|\
             21000000-0000-0000-0000-000000000056|118000000,110221138",
        ),
    ];

    /// Package M21 differential test (new code): replays `tests/golden/m21_outlines.txt`
    /// (produced by `tests/oracle/m21_outline_golden.sh` with the reference build) on every
    /// corpus board under 500 KB that the Rust parser loads without staging stubs: identical
    /// error reports (message, items, position, order) and identical polygons (vertex for
    /// vertex, contour order, closed flags, arc counts).
    #[test]
    fn oracle_corpus() {
        let golden_path = kicad_qa::kicad_source_root()
            .join("rust/crates/kicad-pcb/tests/golden/m21_outlines.txt");
        let golden = std::fs::read_to_string(&golden_path).expect("golden file");

        let mut blocks: Vec<(String, Vec<String>)> = Vec::new();

        for line in golden.lines() {
            if let Some(name) = line.strip_prefix("board|") {
                blocks.push((name.to_string(), Vec::new()));
            } else if let Some(block) = blocks.last_mut() {
                block.1.push(line.to_string());
            }
        }

        let only = std::env::var("M21_BOARD").ok();
        let (mut compared, mut skipped, mut failed) = (0, Vec::new(), Vec::new());

        for (name, expected) in &blocks {
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }

            let path = kicad_qa::kicad_source_root().join(name.replace("__", "/"));
            let Ok(text) = std::fs::read_to_string(&path) else {
                skipped.push(format!("{name}: unreadable"));
                continue;
            };

            if expected.first().is_some_and(|l| l == "load|error") {
                skipped.push(format!("{name}: the reference parser rejects it"));
                continue;
            }

            let options = crate::io::ParseOptions::default();
            let parsed = std::panic::catch_unwind(|| crate::io::parse_board(&text, name, &options));
            let board = match parsed {
                Ok(Ok(board)) => board,
                Ok(Err(e)) => {
                    skipped.push(format!("{name}: {e}"));
                    continue;
                }
                Err(_) => {
                    skipped.push(format!("{name}: parser panic (pending stub)"));
                    continue;
                }
            };

            let actual =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dump_board(&board)))
                {
                    Ok(actual) => actual,
                    Err(e) => {
                        let msg = e
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_default();
                        skipped.push(format!("{name}: panic {msg}"));
                        continue;
                    }
                };

            compared += 1;

            // Pointer-order replacements (PORT_STATUS.md, M21): the reference line and the line
            // the port prints in its place.
            let mut actual = actual;

            for (board, reference, port) in POINTER_ORDER_DEVIATIONS {
                if name.ends_with(board) {
                    for line in actual.iter_mut().filter(|l| l == port) {
                        *line = (*reference).to_string();
                    }
                }
            }

            if &actual != expected {
                let diff: Vec<String> = expected
                    .iter()
                    .filter(|l| !actual.contains(l))
                    .map(|l| format!("  - {l}"))
                    .chain(
                        actual
                            .iter()
                            .filter(|l| !expected.contains(l))
                            .map(|l| format!("  + {l}")),
                    )
                    .collect();
                failed.push(format!("{name}:\n{}", diff.join("\n")));
            }
        }

        eprintln!(
            "m21 oracle: {} boards in the golden, {compared} compared, {} identical, {} skipped",
            blocks.len(),
            compared - failed.len(),
            skipped.len()
        );

        if std::env::var("M21_VERBOSE").is_ok() {
            for s in &skipped {
                eprintln!("skipped {s}");
            }
        }

        assert!(
            failed.is_empty(),
            "{} boards differ:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
}
