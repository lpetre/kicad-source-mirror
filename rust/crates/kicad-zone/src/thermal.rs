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

//! Port of the thermal relief and hatch parts of `pcbnew/zone_filler.cpp`:
//! `buildThermalSpokes`, `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` and
//! `addCopperThievingPattern`.

use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::{
    transform_circle_to_polygon, transform_ring_to_polygon, transform_trapezoid_to_polygon,
};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::eda_angle::{ANGLE_0, ANGLE_90, EdaAngle};
use kicad_geom::geometry::shape_line_chain::Intersection;
use kicad_geom::math::vector2::ki_round_xy;
use kicad_geom::trigo::rotate_point;
use kicad_geom::{Box2I, Seg, Shape, ShapeLineChain, ShapePolySet, Vector2I, ki_round};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::ids::{ItemId, ZoneId};
use kicad_pcb::item::DetachedCopy;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::PadShape;
use kicad_pcb::pcb_track::PcbVia;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{ThievingPattern, ThievingSettings, ZoneFillMode};

use crate::copper_fill::dump_polys_to_copper_layer;
use crate::filler::ZoneFiller;
use crate::knockouts::eval_rules;

/// `std::clamp( v, lo, hi )` as libstdc++ evaluates it (no assertion when `lo > hi`).
fn std_clamp(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// A thermally connected item of `fillCopperZone` (`BOARD_ITEM*` of
/// `aThermalConnectionPads`): a pad or a via.
enum ThermalItem<'a> {
    Pad(&'a Pad),
    Via(&'a PcbVia),
}

fn thermal_item<'a>(ctx: &BoardCtx<'a>, item: ItemId) -> Option<ThermalItem<'a>> {
    if let Some(pad_id) = item.typed::<Pad>() {
        return ctx.board.pad(pad_id).map(ThermalItem::Pad);
    }

    let track_id = item.typed::<kicad_pcb::pcb_track::Track>()?;
    ctx.board
        .track(track_id)
        .and_then(|t| t.as_via())
        .map(ThermalItem::Via)
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::buildThermalSpokes( aZone, aLayer, aSpokedPadsList,
    /// aSpokesList )`: thermal spokes are square-ended segments from the pad centre to points
    /// just outside the thermal relief. The outside end has an extra centre point (at index 3)
    /// which is used for testing whether or not the spoke connects to copper in the parent
    /// zone.
    pub(crate) fn build_thermal_spokes(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        spoked_pads_list: &[ItemId],
        spokes_list: &mut Vec<ShapeLineChain>,
    ) {
        let bds = ctx.board.design_settings();
        let zone_item: ItemId = zone_id.into();
        let mut zone_bb = zone.bounding_box(ctx);
        let mut zone_half_width = zone.min_thickness() / 2;

        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            zone_half_width = zone.hatch_thickness / 2;
        }

        zone_bb.inflate(
            bds.get_biggest_clearance_value(ctx.rules)
                .max(zone.local_clearance().unwrap_or(0)),
        );

        // Is a point on the boundary of the polygon inside or outside?
        // The boundary may be off by MaxError
        let epsilon = bds.max_error();

        for &item in spoked_pads_list {
            let Some(item_ref) = ctx.item(item) else {
                continue;
            };

            // We currently only connect to pads, not pad holes
            if !item_ref.is_on_layer(layer) {
                continue;
            }

            let Some(thermal) = thermal_item(ctx, item) else {
                continue;
            };

            let thermal_relief_gap;
            let mut spoke_w;
            let mut circular = false;

            let (pad, via) = match thermal {
                ThermalItem::Pad(pad) => {
                    let pad_size = pad.size(ctx, layer);

                    if pad.shape(layer) == PadShape::Circle
                        || (pad.shape(layer) == PadShape::Oval && pad_size.x == pad_size.y)
                    {
                        circular = true;
                    }

                    (Some(pad), None)
                }
                ThermalItem::Via(via) => {
                    circular = true;
                    (None, Some(via))
                }
            };

            // For hatch zones, use proper DRC constraints for thermal gap and spoke width,
            // just like solid zones. This ensures consistent thermal relief appearance and
            // respects pad-specific thermal spoke settings.
            if zone.fill_mode() == ZoneFillMode::HatchPattern {
                if let Some(pad) = pad {
                    thermal_relief_gap = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalReliefGapConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .min();

                    let constraint = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalSpokeWidthConstraint,
                        item,
                        zone_item,
                        layer,
                    );
                    spoke_w = constraint.value().opt();

                    let pad_size = pad.size(ctx, layer);
                    let spoke_max_allowed_w = pad_size.x.min(pad_size.y);
                    spoke_w =
                        std_clamp(spoke_w, constraint.value().min(), constraint.value().max());
                    spoke_w = spoke_w.min(spoke_max_allowed_w);

                    if spoke_w < zone.min_thickness() {
                        continue;
                    }
                } else if let Some(via) = via {
                    thermal_relief_gap = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalReliefGapConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .min();

                    spoke_w = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalSpokeWidthConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .opt();

                    spoke_w = spoke_w.min(via.width(layer));

                    if spoke_w < zone.min_thickness() {
                        continue;
                    }
                } else {
                    continue;
                }
            } else if let Some(pad) = pad {
                thermal_relief_gap = eval_rules(
                    ctx,
                    DrcConstraintT::ThermalReliefGapConstraint,
                    item,
                    zone_item,
                    layer,
                )
                .value()
                .min();

                let constraint = eval_rules(
                    ctx,
                    DrcConstraintT::ThermalSpokeWidthConstraint,
                    item,
                    zone_item,
                    layer,
                );
                spoke_w = constraint.value().opt();

                // Spoke width should ideally be smaller than the pad minor axis.
                // Otherwise the thermal shape is not really a thermal relief,
                // and the algo to count the actual number of spokes can fail
                let pad_size = pad.size(ctx, layer);
                let spoke_max_allowed_w = pad_size.x.min(pad_size.y);

                spoke_w = std_clamp(spoke_w, constraint.value().min(), constraint.value().max());

                // ensure the spoke width is smaller than the pad minor size
                spoke_w = spoke_w.min(spoke_max_allowed_w);

                // Cannot create stubs having a width < zone min thickness
                if spoke_w < zone.min_thickness() {
                    continue;
                }
            } else {
                // We don't currently support via thermal connections *except* in a hatched
                // zone.
                continue;
            }

            let spoke_half_w = spoke_w / 2;

            // Quick test here to possibly save us some work
            let mut item_bb = item_ref.bounding_box();
            item_bb.inflate(thermal_relief_gap + epsilon);

            if !item_bb.intersects(&zone_bb) {
                continue;
            }

            let mut custom_spokes = false;

            if let Some(pad) = pad.filter(|p| p.shape(layer) == PadShape::Custom) {
                for primitive in pad.lib_padstack().primitives(layer) {
                    if primitive.is_proxy_item() && primitive.shape.shape() == ShapeT::Segment {
                        custom_spokes = true;
                        break;
                    }
                }
            }

            if custom_spokes {
                let pad = pad.expect("custom spokes need a pad");
                let mut thermal_poly = ShapePolySet::new();
                let mut thermal_outline = ShapeLineChain::new();

                pad.transform_shape_to_polygon(
                    ctx,
                    &mut thermal_poly,
                    layer,
                    thermal_relief_gap + epsilon,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );

                if thermal_poly.outline_count() != 0 {
                    thermal_outline = thermal_poly.c_outline(0).clone();
                }

                let effective_polygon = pad.effective_polygon(ctx, layer, ErrorLoc::Outside);
                // C++ reads Outline( 0 ) unconditionally (undefined behaviour on an empty set).
                let pad_outline = if effective_polygon.outline_count() > 0 {
                    effective_polygon.c_outline(0).clone()
                } else {
                    ShapeLineChain::new()
                };

                let trim_to_outline = |segment: &mut Seg| -> bool {
                    let mut intersections: Vec<Intersection> = Vec::new();

                    if pad_outline.intersect_seg(segment, &mut intersections) != 0 {
                        intersections.clear();

                        // Trim the segment to the thermal outline
                        if thermal_outline.intersect_seg(segment, &mut intersections) != 0 {
                            segment.b = intersections[0].p;
                            return true;
                        }
                    }

                    false
                };

                for primitive in pad.lib_padstack().primitives(layer) {
                    if !(primitive.is_proxy_item() && primitive.shape.shape() == ShapeT::Segment) {
                        continue;
                    }

                    let mut seg = Seg::new(primitive.shape.start(), primitive.shape.end());

                    rotate_point(&mut seg.a, pad.orientation(ctx));
                    rotate_point(&mut seg.b, pad.orientation(ctx));
                    seg.a += pad.shape_pos(ctx, layer);
                    seg.b += pad.shape_pos(ctx, layer);

                    // Make sure seg.A is the origin
                    if !effective_polygon.contains(seg.a, -1, 0, false) {
                        // Do not create this spoke if neither point is in the pad.
                        if !effective_polygon.contains(seg.b, -1, 0, false) {
                            continue;
                        }

                        seg.reverse();
                    }

                    // Trim segment to pad and thermal outline polygon.
                    // If there is no intersection with the pad, don't create the spoke.
                    if trim_to_outline(&mut seg) {
                        let mut direction = (seg.b - seg.a).resize(spoke_half_w);
                        let offset = direction.perpendicular().resize(spoke_half_w);
                        // Extend the spoke edges by half the spoke width to capture convex pad
                        // shapes with a maximum of 45 degrees.
                        let mut seg_l =
                            Seg::new(seg.a - direction - offset, seg.b + direction - offset);
                        let mut seg_r =
                            Seg::new(seg.a - direction + offset, seg.b + direction + offset);

                        // Only create this spoke if both edges intersect the pad and thermal
                        // outline
                        if trim_to_outline(&mut seg_l) && trim_to_outline(&mut seg_r) {
                            // Extend the spoke by the minimum thickness for the zone to ensure
                            // full connection width
                            direction = direction.resize(zone.min_thickness());

                            let mut spoke = ShapeLineChain::new();

                            spoke.append_point(seg.a + offset, false);
                            spoke.append_point(seg.a - offset, false);

                            spoke.append_point(seg_l.b + direction, false);
                            spoke.append_point(seg.b + direction, false); // test pt at index 3.
                            spoke.append_point(seg_r.b + direction, false);

                            spoke.set_closed(true);
                            spokes_list.push(spoke);
                        }
                    }
                }
            } else {
                let mut thermal_spoke_angle = ANGLE_0;

                // Use pad's thermal spoke angle for both solid and hatch zones.
                // This ensures custom thermal spoke templates are respected.
                if let Some(pad) = pad {
                    thermal_spoke_angle = pad.thermal_spoke_angle();
                }

                let mut spokes_box = Box2I::default();
                let mut position = Vector2I::default();
                let mut orientation = ANGLE_0;

                // Since the bounding-box needs to be correclty rotated we use a dummy pad to
                // keep from dirtying the real pad's cached shapes.
                if let Some(pad) = pad {
                    let env = ctx.board.mut_env(item);
                    let mut dummy_pad = pad.detached_copy();
                    dummy_pad.set_orientation(&env, ANGLE_0);

                    // Spokes are from center of pad shape, not from hole. So the dummy pad has
                    // no shape offset and is at position 0,0
                    dummy_pad.set_position(&env, Vector2I::new(0, 0));
                    dummy_pad.set_offset(&env, layer, Vector2I::new(0, 0));

                    spokes_box = dummy_pad.bounding_box_layer(ctx, layer);
                    position = pad.shape_pos(ctx, layer);
                    orientation = pad.orientation(ctx);
                } else if let Some(via) = via {
                    let env = ctx.board.mut_env(item);
                    let mut dummy_via = via.detached_copy();
                    dummy_via.set_position(&env, Vector2I::new(0, 0));

                    spokes_box = dummy_via.bounding_box_layer(ctx, layer);
                    position = via.position(ctx);
                }

                // Add half the zone mininum width to the inflate amount to account for the fact
                // that the deflation procedure will shrink the results by half the half the zone
                // min width.
                spokes_box.inflate(thermal_relief_gap + epsilon + zone_half_width);

                // Yet another wrinkle: the bounding box for circles will overshoot the mark
                // considerably when the spokes are near a 45 degree increment.  So we build the
                // spokes at 0 degrees and then rotate them to the correct position.
                if circular {
                    build_spokes_from_origin(&spokes_box, ANGLE_0, spoke_half_w, spokes_list);

                    if thermal_spoke_angle != ANGLE_0 {
                        // Rotate the last four elements of aspokeslist
                        let n = spokes_list.len();

                        for spoke in spokes_list[n - 4..].iter_mut().rev() {
                            spoke.rotate(thermal_spoke_angle, Vector2I::new(0, 0));
                        }
                    }
                } else {
                    build_spokes_from_origin(
                        &spokes_box,
                        thermal_spoke_angle,
                        spoke_half_w,
                        spokes_list,
                    );
                }

                let n = spokes_list.len();

                for spoke in spokes_list[n - 4..].iter_mut().rev() {
                    spoke.rotate(orientation, Vector2I::new(0, 0));
                    spoke.move_(position);
                }
            }
        }

        for spoke in spokes_list.iter_mut() {
            spoke.generate_bbox_cache();
        }
    }

    /// Port of `ZONE_FILLER::buildHatchZoneThermalRings( aZone, aLayer, aSmoothedOutline,
    /// aThermalConnectionPads, aFillPolys, aThermalRings )`: for hatch zones, the thermal
    /// rings around pads with thermal relief, clipped to the zone boundary. They provide the
    /// connection point for the hatch webbing instead of connecting directly to the pad.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_hatch_zone_thermal_rings(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        smoothed_outline: &ShapePolySet,
        thermal_connection_pads: &[ItemId],
        fill_polys: &mut ShapePolySet,
        thermal_rings: &mut ShapePolySet,
    ) {
        let zone_item: ItemId = zone_id.into();

        for &item in thermal_connection_pads {
            let Some(item_ref) = ctx.item(item) else {
                continue;
            };

            if !item_ref.is_on_layer(layer) {
                continue;
            }

            let Some(thermal) = thermal_item(ctx, item) else {
                continue;
            };

            let mut pad_radius = 0;
            let is_circular;
            let thermal_gap;
            let mut spoke_width;
            let position;
            let mut the_pad = None;

            match thermal {
                ThermalItem::Pad(pad) => {
                    the_pad = Some(pad);
                    let pad_size = pad.size(ctx, layer);
                    position = pad.shape_pos(ctx, layer);

                    is_circular = pad.shape(layer) == PadShape::Circle
                        || (pad.shape(layer) == PadShape::Oval && pad_size.x == pad_size.y);

                    if is_circular {
                        pad_radius = pad_size.x.max(pad_size.y) / 2;
                    }

                    thermal_gap = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalReliefGapConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .min();

                    spoke_width = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalSpokeWidthConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .opt();

                    // Clamp spoke width to pad size
                    let spoke_max_width = pad_size.x.min(pad_size.y);
                    spoke_width = spoke_width.min(spoke_max_width);
                }
                ThermalItem::Via(via) => {
                    position = via.position(ctx);
                    is_circular = true;
                    pad_radius = via.width(layer) / 2;

                    thermal_gap = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalReliefGapConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .min();

                    spoke_width = eval_rules(
                        ctx,
                        DrcConstraintT::ThermalSpokeWidthConstraint,
                        item,
                        zone_item,
                        layer,
                    )
                    .value()
                    .opt();

                    // Clamp spoke width to via diameter
                    spoke_width = spoke_width.min(pad_radius * 2);
                }
            }

            // Don't create a ring if spoke width is too small
            if spoke_width < zone.min_thickness() {
                continue;
            }

            let mut thermal_ring = ShapePolySet::new();

            if is_circular {
                // For circular pads/vias: create an arc ring
                // Ring inner radius = pad radius + thermal gap
                // Ring width = spoke width
                let ring_inner_radius = pad_radius + thermal_gap;
                let ring_width = spoke_width;

                transform_ring_to_polygon(
                    &mut thermal_ring,
                    position,
                    ring_inner_radius + ring_width / 2,
                    ring_width,
                    self.max_error,
                    ErrorLoc::Outside,
                );
            } else if let Some(pad) = the_pad {
                // For non-circular pads: create ring by inflating pad to outer radius,
                // then subtracting pad inflated to inner radius
                let mut outer_shape = ShapePolySet::new();
                let mut inner_shape = ShapePolySet::new();

                // Outer ring edge = pad + thermal gap + spoke width
                pad.transform_shape_to_polygon(
                    ctx,
                    &mut outer_shape,
                    layer,
                    thermal_gap + spoke_width,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );

                // Inner ring edge = pad + thermal gap (this is already knocked out)
                pad.transform_shape_to_polygon(
                    ctx,
                    &mut inner_shape,
                    layer,
                    thermal_gap,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );

                thermal_ring = outer_shape;
                thermal_ring.boolean_subtract(&inner_shape);
            }

            // Clip the thermal ring to the zone boundary so it doesn't overflow
            thermal_ring.boolean_intersection(smoothed_outline);

            // Add the thermal ring to the fill
            fill_polys.boolean_add(&thermal_ring);

            // Also collect thermal rings for hatch hole notching to ensure connectivity
            thermal_rings.boolean_add(&thermal_ring);
        }
    }

    /// Port of `ZONE_FILLER::addHatchFillTypeOnZone( aZone, aLayer, aDebugLayer, aFillPolys,
    /// aThermalRings )`: punch the hatch pattern holes into the (deflated) fill. False only
    /// for a debug-mode stage dump.
    pub(crate) fn add_hatch_fill_type_on_zone(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        debug_layer: PcbLayerId,
        fill_polys: &mut ShapePolySet,
        thermal_rings: &ShapePolySet,
    ) -> bool {
        // Build grid:

        // obviously line thickness must be > zone min thickness.
        // It can happens if a board file was edited by hand by a python script
        // Use 1 micron margin to be *sure* there is no issue in Gerber files
        // (Gbr file unit = 1 or 10 nm) due to some truncation in coordinates or calculations
        // This margin also avoid problems due to rounding coordinates in next calculations
        // that can create incorrect polygons
        let thickness = zone
            .hatch_thickness
            .max(zone.min_thickness() + PCB_IU_SCALE.mm_to_iu(0.001));

        let gridsize = thickness + zone.hatch_gap;
        let mut max_error = ctx.board.design_settings().max_error();

        let mut filled_polys = fill_polys.clone_drop_triangulation();

        // Use a area that contains the rotated bbox by orientation, and after rotate the result
        // by -orientation.
        if !zone.hatch_orientation().is_zero() {
            filled_polys.rotate(-zone.hatch_orientation(), Vector2I::new(0, 0));
        }

        let bbox = filled_polys.bbox(0);

        // Build hole shape
        // the hole size is aZone->GetHatchGap(), but because the outline thickness
        // is aZone->GetMinThickness(), the hole shape size must be larger
        let mut hole_base = ShapeLineChain::new();
        let hole_size = zone.hatch_gap + zone.min_thickness();
        let mut corner = Vector2I::new(0, 0);
        hole_base.append_point(corner, false);
        corner.x += hole_size;
        hole_base.append_point(corner, false);
        corner.y += hole_size;
        hole_base.append_point(corner, false);
        corner.x = 0;
        hole_base.append_point(corner, false);
        hole_base.set_closed(true);

        // Calculate minimal area of a grid hole.
        // All holes smaller than a threshold will be removed
        let minimal_hole_area = hole_base.area(true) * zone.hatch_hole_min_area;

        // Now convert this hole to a smoothed shape:
        if zone.hatch_smoothing_level > 0 {
            // the actual size of chamfer, or rounded corner radius is the half size
            // of the HatchFillTypeGap scaled by aZone->GetHatchSmoothingValue()
            // aZone->GetHatchSmoothingValue() = 1.0 is the max value for the chamfer or the
            // radius of corner (radius = half size of the hole)
            let mut smooth_value: i32 =
                ki_round(zone.hatch_gap as f64 * zone.hatch_smoothing_value / 2.0);

            // Minimal optimization:
            // make smoothing only for reasonable smooth values, to avoid a lot of useless
            // segments and if the smooth value is small, use chamfer even if fillet is
            // requested
            const SMOOTH_MIN_VAL_MM: f64 = 0.02;
            const SMOOTH_SMALL_VAL_MM: f64 = 0.04;

            if smooth_value > PCB_IU_SCALE.mm_to_iu(SMOOTH_MIN_VAL_MM) {
                let mut smooth_hole = ShapePolySet::new();
                smooth_hole.add_outline(&hole_base);
                let mut smooth_level = zone.hatch_smoothing_level;

                if smooth_value < PCB_IU_SCALE.mm_to_iu(SMOOTH_SMALL_VAL_MM) && smooth_level > 1 {
                    smooth_level = 1;
                }

                // Use a larger smooth_value to compensate the outline tickness
                // (chamfer is not visible is smooth value < outline thickess)
                smooth_value += zone.min_thickness() / 2;

                // smooth_value cannot be bigger than the half size oh the hole:
                smooth_value = smooth_value.min(zone.hatch_gap / 2);

                // the error to approximate a circle by segments when smoothing corners by a arc
                max_error = (max_error * 2).max(smooth_value / 20);

                match smooth_level {
                    1 => {
                        // Chamfer() uses the distance from a corner to create a end point
                        // for the chamfer.
                        hole_base = smooth_hole.chamfer(smooth_value).c_outline(0).clone();
                    }
                    0 => {}
                    _ => {
                        if zone.hatch_smoothing_level > 2 {
                            max_error /= 2; // Force better smoothing
                        }

                        hole_base = smooth_hole
                            .fillet(smooth_value, max_error)
                            .c_outline(0)
                            .clone();
                    }
                }
            }
        }

        // Build holes
        let mut holes = ShapePolySet::new();

        let default_offsets = &ctx.board.design_settings().zone_layer_properties;
        let local_offsets = &zone.layer_properties;

        let mut offset = Vector2I::default();

        if let Some(props) = default_offsets.get(&layer) {
            offset = props.hatching_offset.unwrap_or_default();
        }

        if let Some(local) = local_offsets.get(&layer).and_then(|p| p.hatching_offset) {
            offset = local;
        }

        let x_offset = bbox.x() - bbox.x() % gridsize - gridsize;
        let y_offset = bbox.y() - bbox.y() % gridsize - gridsize;

        let mut xx = x_offset;

        while xx <= bbox.right() {
            let mut yy = y_offset;

            while yy <= bbox.bottom() {
                // Generate hole
                let mut hole = hole_base.clone();
                hole.move_(Vector2I::new(xx, yy));

                if !zone.hatch_orientation().is_zero() {
                    hole.rotate(zone.hatch_orientation(), Vector2I::new(0, 0));
                }

                hole.move_(Vector2I::new(offset.x % gridsize, offset.y % gridsize));

                holes.add_outline(&hole);

                yy += gridsize;
            }

            xx += gridsize;
        }

        holes.clear_arcs();

        dump_polys_to_copper_layer!(
            self,
            holes,
            PcbLayerId::In10Cu,
            "hatch-holes",
            debug_layer,
            fill_polys
        );

        let mut deflated_thickness = zone.hatch_thickness - zone.min_thickness();

        // Don't let thickness drop below maxError * 2 or it might not get reinflated.
        deflated_thickness = deflated_thickness.max(max_error * 2);

        // The fill has already been deflated to ensure GetMinThickness() so we just have to
        // account for anything beyond that.
        let mut deflated_filled_polys = fill_polys.clone_drop_triangulation();
        deflated_filled_polys.clear_arcs();
        deflated_filled_polys.deflate(
            deflated_thickness,
            CornerStrategy::ChamferAllCorners,
            max_error,
        );
        holes.boolean_intersection(&deflated_filled_polys);
        dump_polys_to_copper_layer!(
            self,
            holes,
            PcbLayerId::In11Cu,
            "fill-clipped-hatch-holes",
            debug_layer,
            fill_polys
        );

        let mut deflated_outline = zone.board_outline(ctx);
        deflated_outline.clear_arcs();
        deflated_outline.deflate(
            zone.min_thickness(),
            CornerStrategy::ChamferAllCorners,
            max_error,
        );
        holes.boolean_intersection(&deflated_outline);
        dump_polys_to_copper_layer!(
            self,
            holes,
            PcbLayerId::In12Cu,
            "outline-clipped-hatch-holes",
            debug_layer,
            fill_polys
        );

        // Now filter truncated holes to avoid small holes in pattern
        // It happens for holes near the zone outline
        let mut ii = 0;

        while ii < holes.outline_count() {
            let area = holes.c_outline(ii).area(true);

            if area < minimal_hole_area {
                // The current hole is too small: remove it
                holes.delete_polygon(ii);
            } else {
                ii += 1;
            }
        }

        // Drop any holes that completely enclose a thermal ring to ensure thermal reliefs
        // stay connected to the hatch webbing. Only drop holes where the thermal ring is
        // entirely inside the hole; partial overlaps are kept to preserve the hatch pattern.
        if thermal_rings.outline_count() > 0 {
            let thermal_bbox = thermal_rings.bbox(0);

            // Iterate through holes (backwards since we may delete)
            for hole_idx in (0..holes.outline_count()).rev() {
                let hole = holes.c_outline(hole_idx).clone();
                let hole_bbox = hole.bbox(0);

                // Quick rejection: skip if hole bbox doesn't intersect thermal rings bbox
                if !hole_bbox.intersects(&thermal_bbox) {
                    continue;
                }

                // Check if ANY thermal ring is completely enclosed by this hole
                for ring_idx in 0..thermal_rings.outline_count() {
                    let ring = thermal_rings.c_outline(ring_idx);
                    let ring_bbox = ring.bbox(0);
                    let ring_center = ring_bbox.centre();

                    // Quick rejection: hole bbox must contain ring bbox
                    if !hole_bbox.contains_box(&ring_bbox) {
                        continue;
                    }

                    // Check 1: Is the ring center inside the hole?
                    if !hole.point_inside(ring_center, 0, false) {
                        continue;
                    }

                    // Check 2: Is at least one point on the ring inside the hole?
                    if ring.point_count() == 0 || !hole.point_inside(ring.c_point(0), 0, false) {
                        continue;
                    }

                    // Check 3: Does the ring outline NOT intersect the hole outline?
                    // If there's no intersection, the ring is fully enclosed (not touching
                    // edges)
                    let mut intersections: Vec<Intersection> = Vec::new();
                    ring.intersect_chain(&hole, &mut intersections, false, None);

                    if intersections.is_empty() {
                        // This hole completely encloses a ring - drop it
                        holes.delete_polygon(hole_idx);
                        break; // Move to next hole
                    }
                }
            }
        }

        // create grid. Useto
        // generate strictly simple polygons needed by Gerber files and Fracture()
        let fill_copy = fill_polys.clone();
        fill_polys.boolean_subtract_sets(&fill_copy, &holes);
        dump_polys_to_copper_layer!(
            self,
            *fill_polys,
            PcbLayerId::In14Cu,
            "after-hatching",
            debug_layer,
            fill_polys
        );

        true
    }

    /// Port of `ZONE_FILLER::addCopperThievingPattern( aZone, aLayer, aFillPolys )`
    /// (package ZF-07): replace (dots, squares) or perforate (hatch) the deflated fill of a
    /// copper-thieving zone with its thieving pattern.  Returns `false` only for the
    /// `wxCHECK( aZone->IsCopperThieving(), false )` programmer error.
    pub(crate) fn add_copper_thieving_pattern(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        fill_polys: &mut ShapePolySet,
    ) -> bool {
        if !zone.is_copper_thieving() {
            debug_assert!(false, "addCopperThievingPattern on a non-thieving zone");
            return false;
        }

        let settings: &ThievingSettings = &zone.thieving_settings;
        let orientation = settings.orientation.angle();

        // Constructor defaults are positive but a malformed file or test board could still
        // produce a zero gap, which would deadlock the grid loop below.  Bail out without
        // touching aFillPolys so the zone simply has no fill, matching POLYGONS-with-bad-poly.
        // element_size is meaningful for dots and squares only.  Hatch uses line_width.
        let needs_element_size = settings.pattern != ThievingPattern::Hatch;
        let needs_line_width = settings.pattern == ThievingPattern::Hatch;

        if settings.gap <= 0
            || (needs_element_size && settings.element_size <= 0)
            || (needs_line_width && settings.line_width <= 0)
        {
            fill_polys.remove_all_contours();
            return true;
        }

        let mut filled_region = fill_polys.clone_drop_triangulation();

        if filled_region.outline_count() == 0 {
            fill_polys.remove_all_contours();
            return true;
        }

        // Rotate the clip region into the pattern's local frame so the grid iterates
        // axis-aligned; the resulting stamps get rotated back into the zone's frame below.
        if !orientation.is_zero() {
            filled_region.rotate(-orientation, Vector2I::new(0, 0));
        }

        // BBox() over all outlines -- the post-clearance fill region may be split
        // into several pieces (e.g. by a track cutting across the zone) and the
        // void grid has to cover every piece.
        let bbox = filled_region.bbox(0);

        // Per-layer phase offset (hatching_offset) -- same lookup the hatch generator uses
        // so thieving on multiple copper layers can be de-correlated through the stack-up.
        // Board-default offsets apply first; per-zone local offsets override.
        let default_offsets = &ctx.board.design_settings().zone_layer_properties;
        let local_offsets = &zone.layer_properties;
        let mut offset = Vector2I::default();

        if let Some(props) = default_offsets.get(&layer) {
            offset = props.hatching_offset.unwrap_or_default();
        }

        if let Some(local) = local_offsets.get(&layer).and_then(|p| p.hatching_offset) {
            offset = local;
        }

        if !orientation.is_zero() {
            rotate_point(&mut offset, -orientation);
        }

        // Gap is edge-to-edge; grid stride is element_size + gap (dots/squares) or
        // line_width + gap (crosshatch).
        let dot_stride = settings.element_size + settings.gap;

        // The filler stamps thieving shapes while aFillPolys is deflated by
        // half_min_width and then later re-inflates by the same amount.  Pre-compensate
        // the dot radius so the final stamp matches element_size exactly.  If the
        // user's element_size is smaller than min_thickness, fall back to a 1 IU
        // radius so the reinflate produces approximately min_thickness diameter.
        let half_min_width = zone.min_thickness() / 2;
        let dot_radius = (settings.element_size / 2 - half_min_width).max(1);
        let max_error = ctx.board.design_settings().max_error();

        // Collect every stamp into a single SHAPE_POLY_SET, then BooleanIntersect once.
        // Per-stamp boolean ops would explode in cost on a 10k-dot zone.
        let mut stamps = ShapePolySet::new();

        // Hatch is subtractive: keep the zone outline as a perimeter border around
        // the mesh by carving voids out of aFillPolys.  Dots and squares are
        // additive: replace aFillPolys with the stamp set, clipped to the zone.
        if settings.pattern == ThievingPattern::Hatch {
            // Void size in the deflated frame is gap + min_thickness so that the
            // generic reinflate at the end of fillCopperZone shrinks the void by
            // min_thickness and the final edge-to-edge spacing equals user gap.
            let void_size = settings.gap + zone.min_thickness();
            let line_stride = settings.line_width + settings.gap;

            // Deflate aFillPolys by line_width to define an interior region that
            // can receive voids.  The unaltered annulus between aFillPolys and
            // interior becomes the perimeter outline of the mesh, matching how
            // the existing HATCH_PATTERN fill mode produces a border.  This also
            // protects narrow post-clearance fragments (e.g. a thin strip on the
            // opposite side of a track) from being entirely consumed by voids.
            let mut interior = fill_polys.clone_drop_triangulation();
            interior.deflate(
                settings.line_width,
                CornerStrategy::ChamferAllCorners,
                self.max_error,
            );

            if interior.outline_count() == 0 {
                return true;
            }

            // Walk a starting position backwards into the bbox so we never miss a
            // void on the negative side after the modulo step.  bbox already
            // contains the rotated filledRegion bounds, which slightly overcover
            // the interior; extra voids get clipped to interior below.
            let mut x_void = bbox.left() - (bbox.left() % line_stride) + offset.x + line_stride / 2;
            let mut y_void = bbox.top() - (bbox.top() % line_stride) + offset.y + line_stride / 2;

            while x_void - void_size / 2 > bbox.left() {
                x_void -= line_stride;
            }

            while y_void - void_size / 2 > bbox.top() {
                y_void -= line_stride;
            }

            let mut voids = ShapePolySet::new();

            let mut yy = y_void;

            while yy <= bbox.bottom() + void_size {
                let mut xx = x_void;

                while xx <= bbox.right() + void_size {
                    let mut rect = ShapeLineChain::new();
                    rect.append_xy(xx - void_size / 2, yy - void_size / 2, false);
                    rect.append_xy(xx + void_size / 2, yy - void_size / 2, false);
                    rect.append_xy(xx + void_size / 2, yy + void_size / 2, false);
                    rect.append_xy(xx - void_size / 2, yy + void_size / 2, false);
                    rect.set_closed(true);
                    voids.add_outline(&rect);

                    xx += line_stride;
                }

                yy += line_stride;
            }

            if !orientation.is_zero() {
                voids.rotate(orientation, Vector2I::new(0, 0));
            }

            // Clip voids to interior so the perimeter border survives the
            // subtraction.  Without this clamp, voids on the edge punch through
            // the border, and narrow post-clearance pieces of aFillPolys are
            // consumed entirely.
            voids.boolean_intersection(&interior);

            // Carve the voids out of the zone fill region.  No island removal: the
            // hatch mesh is a single connected piece with its zone-outline border.
            fill_polys.boolean_subtract(&voids);
            return true;
        }

        let mut x_start = bbox.left() - (bbox.left() % dot_stride) + offset.x;
        let mut y_start = bbox.top() - (bbox.top() % dot_stride) + offset.y;

        while x_start > bbox.left() {
            x_start -= dot_stride;
        }

        while y_start > bbox.top() {
            y_start -= dot_stride;
        }

        // Dots and squares: drop any stamp transected by an obstacle or touching the
        // zone outline.  Deflating the fill region by stampHalfExtent + 1 IU yields
        // the set of centres where a full stamp fits without touching the boundary.
        let side_len = (settings.element_size - zone.min_thickness()).max(1);
        let square_size = Vector2I::new(side_len, side_len);

        let containment_inset = if settings.pattern == ThievingPattern::Squares {
            side_len / 2
        } else {
            dot_radius
        } + 1;

        filled_region.deflate(
            containment_inset,
            CornerStrategy::ChamferAllCorners,
            max_error,
        );

        if filled_region.outline_count() == 0 {
            fill_polys.remove_all_contours();
            return true;
        }

        filled_region.build_bbox_caches();

        let mut row_index = 0;
        let mut yy = y_start;

        while yy <= bbox.bottom() + dot_radius {
            let row_offset = if settings.stagger && (row_index & 1) != 0 {
                dot_stride / 2
            } else {
                0
            };

            let mut xx = x_start + row_offset;

            while xx <= bbox.right() + dot_radius {
                let centre = Vector2I::new(xx, yy);

                if filled_region.contains(centre, -1, 0, true) {
                    if settings.pattern == ThievingPattern::Squares {
                        transform_trapezoid_to_polygon(
                            &mut stamps,
                            centre,
                            square_size,
                            ANGLE_0,
                            0,
                            0,
                            0,
                            max_error,
                            ErrorLoc::Outside,
                        );
                    } else {
                        transform_circle_to_polygon(
                            &mut stamps,
                            centre,
                            dot_radius,
                            max_error,
                            ErrorLoc::Outside,
                            0,
                        );
                    }
                }

                xx += dot_stride;
            }

            row_index += 1;
            yy += dot_stride;
        }

        if !orientation.is_zero() {
            stamps.rotate(orientation, Vector2I::new(0, 0));
        }

        *fill_polys = stamps;
        true
    }
}

/// The `buildSpokesFromOrigin( box, angle )` lambda of `buildThermalSpokes`: four spokes
/// from the box centre at `angle + 0 / 90 / 180 / 270` degrees, each ending on the box edge.
fn build_spokes_from_origin(
    bbox: &Box2I,
    angle: EdaAngle,
    spoke_half_w: i32,
    spokes_list: &mut Vec<ShapeLineChain>,
) {
    let center = bbox.center();
    let half_size = ki_round_xy(bbox.width() as f64 / 2.0, bbox.height() as f64 / 2.0);

    // Function to find intersection of line with box edge
    let intersect_bbox = |spoke_angle: EdaAngle, spoke_side: &mut Vector2I| -> Vector2I {
        let dx = spoke_angle.cos();
        let dy = spoke_angle.sin();

        // Short-circuit the axis cases because they will be degenerate in the
        // intersection test
        if dx == 0.0 {
            *spoke_side = Vector2I::new(spoke_half_w, 0);
            return ki_round_xy(0.0, dy * half_size.y as f64);
        } else if dy == 0.0 {
            *spoke_side = Vector2I::new(0, spoke_half_w);
            return ki_round_xy(dx * half_size.x as f64, 0.0);
        }

        // We are going to intersect with one side or the other.  Whichever
        // we hit first is the fraction of the spoke length we keep
        let dist_x = half_size.x as f64 / dx.abs();
        let dist_y = half_size.y as f64 / dy.abs();

        if dist_x < dist_y {
            *spoke_side = ki_round_xy(0.0, spoke_half_w as f64 / (ANGLE_90 - spoke_angle).sin());
            ki_round_xy(dx * dist_x, dy * dist_x)
        } else {
            *spoke_side = ki_round_xy(spoke_half_w as f64 / spoke_angle.sin(), 0.0);
            ki_round_xy(dx * dist_y, dy * dist_y)
        }
    };

    // Precalculate angles for four cardinal directions
    let angles = [
        EdaAngle::from_degrees(0.0) + angle,   // Right
        EdaAngle::from_degrees(90.0) + angle,  // Up
        EdaAngle::from_degrees(180.0) + angle, // Left
        EdaAngle::from_degrees(270.0) + angle, // Down
    ];

    // Generate four spokes in cardinal directions
    for spoke_angle in angles {
        let mut spoke_side = Vector2I::default();
        let intersection = intersect_bbox(spoke_angle, &mut spoke_side);

        let mut spoke = ShapeLineChain::new();
        spoke.append_point(center + spoke_side, false);
        spoke.append_point(center - spoke_side, false);
        spoke.append_point(center + intersection - spoke_side, false);
        spoke.append_point(center + intersection, false); // test pt
        spoke.append_point(center + intersection + spoke_side, false);
        spoke.set_closed(true);
        spokes_list.push(spoke);
    }
}
