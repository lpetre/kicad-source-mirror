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

//! Port of the knockout part of `pcbnew/zone_filler.cpp`: the coincident-item deduplication
//! keys (`PAD_KNOCKOUT_KEY`, `VIA_KNOCKOUT_KEY`, `TRACK_KNOCKOUT_KEY`), `isZoneFillKeepout`,
//! `addKnockout` (both overloads), `addHoleKnockout`, `knockoutThermalReliefs`,
//! `buildCopperItemClearances`, `buildDifferentNetZoneClearances` and
//! `subtractHigherPriorityZones`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use kicad_core::layer_ids::{Flashing, is_inner_copper_layer};
use kicad_core::typeinfo::KicadT;
use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::transform_circle_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::convex_hull::build_convex_hull_poly_set;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::{Box2I, ShapePolySet, Vector2I};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::{DrcConstraint, DrcConstraintT};
use kicad_pcb::eda_shape::shapes::EdaShapeShapesExt;
use kicad_pcb::ids::{FootprintId, ItemId, PadId, ZoneId};
use kicad_pcb::item_ref::AnyItem;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{CustomShapeZoneMode, PadAttrib, PadShape, UnconnectedLayerMode};
use kicad_pcb::pcb_field::FieldT;
use kicad_pcb::pcb_track::{Track, ZoneLayerOverride};
use kicad_pcb::traits::BoardConnectedItem;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{ZoneConnection, ZoneFillMode};

use crate::filler::{EXTRA_CLEARANCE_MM, ZoneFiller};
use crate::index::{board_and_footprint_zones, query_index};

/// Port of the anonymous `PAD_KNOCKOUT_KEY`: key for deduplicating coincident pads.
///
/// For circular pads it uses the max of drill and pad size, for non-circular pads the pad
/// size only. The net code is included so pads at the same position with different nets are
/// not deduplicated, since they require different zone treatment (thermal vs clearance).
/// `orientation` holds the bits of the angle in degrees (with `-0.0` folded onto `0.0`, so
/// that equality is the `double ==` of `EDA_ANGLE::operator==`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PadKnockoutKey {
    position: Vector2I,
    effective_size: Vector2I,
    shape: i32,
    orientation: u64,
    net_code: i32,
}

/// Port of the anonymous `VIA_KNOCKOUT_KEY` (vias are circular, so the max of drill and
/// width is used).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ViaKnockoutKey {
    position: Vector2I,
    effective_size: i32,
    net_code: i32,
}

/// Port of the anonymous `TRACK_KNOCKOUT_KEY` (same endpoints, width). Endpoints are
/// canonicalised so that start < end lexicographically.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TrackKnockoutKey {
    start: Vector2I,
    end: Vector2I,
    width: i32,
}

impl TrackKnockoutKey {
    /// Port of `TRACK_KNOCKOUT_KEY( aStart, aEnd, aWidth )`.
    fn new(start: Vector2I, end: Vector2I, width: i32) -> Self {
        // Canonicalize endpoint order for consistent hashing
        if start.x < end.x || (start.x == end.x && start.y <= end.y) {
            TrackKnockoutKey { start, end, width }
        } else {
            TrackKnockoutKey {
                start: end,
                end: start,
                width,
            }
        }
    }
}

/// The `PAD_KNOCKOUT_KEY` of `pad` on `layer`; `None` for custom pads (they have complex
/// shapes and are never deduplicated).
fn pad_knockout_key(ctx: &BoardCtx<'_>, pad: &Pad, layer: PcbLayerId) -> Option<PadKnockoutKey> {
    let pad_shape_type = pad.shape(layer);

    if pad_shape_type == PadShape::Custom {
        return None;
    }

    // For circular pads: use max of drill and pad size; otherwise just pad size
    let pad_size = pad.size(ctx, layer);

    let effective_size = if pad_shape_type == PadShape::Circle {
        let drill_size = pad.drill_size(ctx);
        let drill = drill_size.x.max(drill_size.y);
        let max_dim = pad_size.x.max(pad_size.y).max(drill);
        Vector2I::new(max_dim, max_dim)
    } else {
        pad_size
    };

    // -0.0 == 0.0 for the C++ double comparison.
    let degrees = pad.orientation(ctx).as_degrees() + 0.0;

    Some(PadKnockoutKey {
        position: pad.position(ctx),
        effective_size,
        shape: pad_shape_type as i32,
        orientation: degrees.to_bits(),
        net_code: pad.net_code().0,
    })
}

/// Port of the anonymous `isZoneFillKeepout( aZone, aLayer, aBBox )`.
pub(crate) fn is_zone_fill_keepout(
    ctx: &BoardCtx<'_>,
    zone: &Zone,
    layer: PcbLayerId,
    bbox: &Box2I,
) -> bool {
    zone.is_rule_area()
        && zone.has_keepout_parameters_set()
        && zone.do_not_allow_zone_fills
        && zone.is_on_layer(ctx, layer)
        && zone.bounding_box(ctx).intersects(bbox)
}

/// `bds.m_DRCEngine->EvalRules( aConstraint, a, b, aLayer )`. A context without a rule
/// resolver answers the null constraint (the C++ filler always has an engine).
pub(crate) fn eval_rules(
    ctx: &BoardCtx<'_>,
    constraint: DrcConstraintT,
    a: ItemId,
    b: ItemId,
    layer: PcbLayerId,
) -> DrcConstraint {
    match ctx.rules {
        Some(rules) => rules.eval_rules(ctx, constraint, ctx.item(a), ctx.item(b), layer),
        None => DrcConstraint::default(),
    }
}

/// Port of `DRC_ENGINE::EvalZoneConnection( a, b, aLayer )` (the `RuleResolver` trait has
/// only `EvalRules`; this is the same body without the reporter).
pub(crate) fn eval_zone_connection(
    ctx: &BoardCtx<'_>,
    a: ItemId,
    b: ItemId,
    layer: PcbLayerId,
) -> DrcConstraint {
    let mut constraint = eval_rules(ctx, DrcConstraintT::ZoneConnectionConstraint, a, b, layer);

    if constraint.zone_connection == ZoneConnection::ThtThermal {
        let pad = if a.typed::<Pad>().is_some() {
            ctx.item(a).and_then(|i| i.as_pad())
        } else if b.typed::<Pad>().is_some() {
            ctx.item(b).and_then(|i| i.as_pad())
        } else {
            None
        };

        if pad.is_some_and(|p| p.attribute() == PadAttrib::Pth) {
            constraint.zone_connection = ZoneConnection::Thermal;
        } else {
            constraint.zone_connection = ZoneConnection::Full;
        }
    }

    constraint
}

/// The `evalRulesForItems` lambda of `buildCopperItemClearances` /
/// `buildDifferentNetZoneClearances`: -1 for a null constraint, else its minimum.
fn eval_rules_for_items(
    ctx: &BoardCtx<'_>,
    constraint: DrcConstraintT,
    a: ItemId,
    b: ItemId,
    layer: PcbLayerId,
) -> i32 {
    let c = eval_rules(ctx, constraint, a, b, layer);

    if c.is_null() { -1 } else { c.value().min() }
}

impl ZoneFiller {
    /// Port of `ZONE_FILLER::addKnockout( BOARD_ITEM* aItem, PCB_LAYER_ID aLayer, int aGap,
    /// SHAPE_POLY_SET& aHoles )`: add a knockout for a pad or via. The knockout is `gap`
    /// larger than the pad (which might be either the thermal clearance or the electrical
    /// clearance).
    pub(crate) fn add_knockout(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemId,
        layer: PcbLayerId,
        gap: i32,
        holes: &mut ShapePolySet,
    ) {
        let Some(item_ref) = ctx.item(item) else {
            return;
        };

        if let Some(pad) = item_ref.as_pad().filter(|p| p.shape(layer) == PadShape::Custom) {
            let mut poly = ShapePolySet::new();
            pad.transform_shape_to_polygon(
                ctx,
                &mut poly,
                layer,
                gap,
                self.max_error,
                ErrorLoc::Outside,
                false,
            );

            // the pad shape in zone can be its convex hull or the shape itself
            if pad.custom_shape_in_zone_opt() == CustomShapeZoneMode::Convexhull {
                let mut convex_hull = Vec::new();
                build_convex_hull_poly_set(&mut convex_hull, &poly);

                holes.new_outline();

                for pt in convex_hull {
                    holes.append_point(pt, -1, -1);
                }
            } else {
                holes.append_set(&poly);
            }
        } else {
            item_ref.transform_shape_to_polygon(
                holes,
                layer,
                gap,
                self.max_error,
                ErrorLoc::Outside,
                false,
            );
        }
    }

    /// Port of `ZONE_FILLER::addHoleKnockout( aPad, aGap, aHoles )`: add a knockout for a
    /// pad's hole.
    pub(crate) fn add_hole_knockout(
        &self,
        ctx: &BoardCtx<'_>,
        pad: &Pad,
        gap: i32,
        holes: &mut ShapePolySet,
    ) {
        pad.transform_hole_to_polygon(ctx, holes, gap, self.max_error, ErrorLoc::Outside);
    }

    /// Port of `ZONE_FILLER::addKnockout( BOARD_ITEM* aItem, PCB_LAYER_ID aLayer, int aGap,
    /// bool aIgnoreLineWidth, SHAPE_POLY_SET& aHoles )`: add a knockout for a graphic item.
    /// The knockout is `gap` larger than the item (which might be either the electrical
    /// clearance or the board edge clearance).
    pub(crate) fn add_graphic_knockout(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemId,
        layer: PcbLayerId,
        gap: i32,
        ignore_line_width: bool,
        holes: &mut ShapePolySet,
    ) {
        let Some(item_ref) = ctx.item(item) else {
            return;
        };

        match item_ref.type_() {
            KicadT::PcbFieldT | KicadT::PcbTextT => {
                // static_cast<PCB_TEXT*>: the PCB_TEXT part of a field.
                let Some(text) = item_ref.as_text() else {
                    return;
                };

                if text.text.is_visible() {
                    if item_ref.as_dyn().is_knockout() {
                        // Knockout text should only leave holes where the text is, not where
                        // the copper fill around it would be.  `PCB_TEXT textCopy = *text`
                        // slices a field to its PCB_TEXT part, as here.
                        let mut text_copy = text.clone();
                        text_copy.base.is_knockout = false;
                        text_copy.transform_text_to_poly_set(
                            ctx,
                            holes,
                            0,
                            self.max_error,
                            ErrorLoc::Inside,
                        );
                    } else {
                        item_ref.transform_shape_to_polygon(
                            holes,
                            layer,
                            gap,
                            self.max_error,
                            ErrorLoc::Outside,
                            false,
                        );
                    }
                }
            }

            KicadT::PcbShapeT => {
                if let AnyItem::Shape(shape) = item_ref.any() {
                    shape.transform_with_line_endings_to_polygon(
                        ctx,
                        holes,
                        gap,
                        self.max_error,
                        ErrorLoc::Outside,
                        ignore_line_width,
                    );
                }
            }

            KicadT::PcbTextboxT
            | KicadT::PcbTableT
            | KicadT::PcbDrillChartT
            | KicadT::PcbStackupTableT
            | KicadT::PcbTargetT => {
                item_ref.transform_shape_to_polygon(
                    holes,
                    layer,
                    gap,
                    self.max_error,
                    ErrorLoc::Outside,
                    ignore_line_width,
                );
            }

            KicadT::PcbBarcodeT => {
                if let AnyItem::Barcode(barcode) = item_ref.any() {
                    barcode.bounding_hull(
                        ctx,
                        holes,
                        layer,
                        gap,
                        self.max_error,
                        ErrorLoc::Outside,
                    );
                }
            }

            KicadT::PcbDimAlignedT
            | KicadT::PcbDimLeaderT
            | KicadT::PcbDimCenterT
            | KicadT::PcbDimRadialT
            | KicadT::PcbDimOrthogonalT => {
                if let Some(dim) = item_ref.as_dimension() {
                    dim.transform_shape_to_polygon(
                        ctx,
                        holes,
                        layer,
                        gap,
                        self.max_error,
                        ErrorLoc::Outside,
                        false,
                    );
                    // dim->PCB_TEXT::TransformShapeToPolygon(): every virtual it reaches is
                    // the inherited PCB_TEXT body, so the embedded PCB_TEXT answers the same.
                    dim.text.transform_shape_to_polygon(
                        ctx,
                        holes,
                        layer,
                        gap,
                        self.max_error,
                        ErrorLoc::Outside,
                        false,
                    );
                }
            }

            _ => {}
        }
    }

    /// Port of `ZONE_FILLER::knockoutThermalReliefs( aZone, aLayer, aFill,
    /// aThermalConnectionPads, aNoConnectionPads, aSolidConnectionItems )`: removes thermal
    /// reliefs from the shape for any pads connected to the zone. Does NOT add in spokes,
    /// which must be done later.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn knockout_thermal_reliefs(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        fill: &mut ShapePolySet,
        thermal_connection_pads: &mut Vec<ItemId>,
        no_connection_pads: &mut Vec<PadId>,
        solid_connection_items: &mut Vec<ItemId>,
    ) {
        let zone_item: ItemId = zone_id.into();
        let mut holes = ShapePolySet::new();

        // Deduplication sets for coincident pads and vias
        let mut processed_pads: HashSet<PadKnockoutKey> = HashSet::new();
        let mut processed_vias: HashSet<ViaKnockoutKey> = HashSet::new();

        let zone_bbox = zone.bounding_box(ctx);
        let zone_net = zone.base.net.0;

        // Inflating the query window is equivalent to inflating each pad box below.
        let mut pad_query_box = zone_bbox;
        pad_query_box.inflate(self.worst_clearance);

        let pad_hits = query_index(&self.pad_index, &pad_query_box);

        for pad_hit in pad_hits {
            let Some(pad_id) = pad_hit.item.typed::<Pad>() else {
                continue;
            };
            let Some(pad) = ctx.board.pad(pad_id) else {
                continue;
            };
            let pad_item: ItemId = pad_id.into();

            // NPTH pads with a drill hole affect all copper layers even when they carry no
            // copper on that layer (e.g. layers limited to "*.Mask"). The physical hole still
            // requires a clearance knockout, so skip only pads that are truly irrelevant to
            // this layer.
            let npth_with_hole = pad.attribute() == PadAttrib::Npth && pad.drill_size(ctx).x > 0;

            if !pad.is_on_layer(ctx, layer) && !npth_with_hole {
                continue;
            }

            let mut pad_bbox = pad.bounding_box(ctx);
            pad_bbox.inflate(self.worst_clearance);

            if !pad_bbox.intersects(&zone_bbox) {
                continue;
            }

            // Deduplicate coincident pads (skip custom pads - they have complex shapes)
            if let Some(pad_key) = pad_knockout_key(ctx, pad, layer) {
                if !processed_pads.insert(pad_key) {
                    continue;
                }
            }

            let mut no_connection = pad.net_code().0 != zone_net;

            if !zone.is_teardrop_area()
                && (zone_net == 0
                    || pad.zone_layer_override(layer) == ZoneLayerOverride::ForceNoZoneConnection)
            {
                no_connection = true;
            }

            // Check if the pad is backdrilled or post-machined on this layer
            if pad.is_backdrilled_or_post_machined(ctx, layer) {
                no_connection = true;
            }

            if no_connection {
                // collect these for knockout in buildCopperItemClearances()
                no_connection_pads.push(pad_id);
                continue;
            }

            // For hatch zones, respect the zone connection type just like solid zones
            // Pads with THERMAL connection get thermal rings; FULL connections get no
            // knockout; NONE connections get handled later in buildCopperItemClearances.
            if zone.fill_mode() == ZoneFillMode::HatchPattern {
                let mut connection =
                    eval_zone_connection(ctx, pad_item, zone_item, layer).zone_connection;

                if connection == ZoneConnection::Thermal && !pad.flash_layer_permitted(ctx, layer)
                {
                    connection = ZoneConnection::None;
                }

                match connection {
                    ZoneConnection::Thermal => {
                        let pad_shape = pad.effective_shape(
                            ctx,
                            layer,
                            Flashing::AlwaysFlashed,
                            DrcConstraintT::NullConstraint,
                        );

                        if fill.collide_shape_mut(pad_shape.as_ref(), 0, None, None) {
                            // Get the thermal relief gap
                            let thermal_gap = eval_rules(
                                ctx,
                                DrcConstraintT::ThermalReliefGapConstraint,
                                pad_item,
                                zone_item,
                                layer,
                            )
                            .value()
                            .min();

                            // Knock out the thermal gap only - the thermal ring will be added
                            // separately
                            thermal_connection_pads.push(pad_item);
                            self.add_knockout(ctx, pad_item, layer, thermal_gap, &mut holes);
                        }
                    }
                    ZoneConnection::None => {
                        // Will be handled by buildCopperItemClearances
                        no_connection_pads.push(pad_id);
                    }
                    _ => {
                        // No knockout - pad connects directly to the hatch
                    }
                }

                continue;
            }

            let mut connection = if zone.is_teardrop_area() {
                ZoneConnection::Full
            } else {
                eval_zone_connection(ctx, pad_item, zone_item, layer).zone_connection
            };

            if connection == ZoneConnection::Thermal && !pad.flash_layer_permitted(ctx, layer) {
                connection = ZoneConnection::None;
            }

            match connection {
                ZoneConnection::Thermal => {
                    let pad_shape = pad.effective_shape(
                        ctx,
                        layer,
                        Flashing::AlwaysFlashed,
                        DrcConstraintT::NullConstraint,
                    );

                    if fill.collide_shape_mut(pad_shape.as_ref(), 0, None, None) {
                        let pad_clearance = eval_rules(
                            ctx,
                            DrcConstraintT::ThermalReliefGapConstraint,
                            pad_item,
                            zone_item,
                            layer,
                        )
                        .value()
                        .min();

                        thermal_connection_pads.push(pad_item);
                        self.add_knockout(ctx, pad_item, layer, pad_clearance, &mut holes);
                    }
                }

                ZoneConnection::None => {
                    let constraint = eval_rules(
                        ctx,
                        DrcConstraintT::PhysicalClearanceConstraint,
                        pad_item,
                        zone_item,
                        layer,
                    );
                    let zone_clearance = zone.local_clearance().unwrap_or(0);

                    let pad_clearance = if constraint.value().min() > zone_clearance {
                        constraint.value().min()
                    } else {
                        zone_clearance
                    };

                    if pad.flash_layer(ctx, layer) {
                        self.add_knockout(ctx, pad_item, layer, pad_clearance, &mut holes);
                    } else if pad.drill_size(ctx).x > 0 {
                        let constraint = eval_rules(
                            ctx,
                            DrcConstraintT::PhysicalHoleClearanceConstraint,
                            pad_item,
                            zone_item,
                            layer,
                        );

                        let hole_clearance = if constraint.value().min() > pad_clearance {
                            constraint.value().min()
                        } else {
                            pad_clearance
                        };

                        pad.transform_hole_to_polygon(
                            ctx,
                            &mut holes,
                            hole_clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                        );
                    }
                }

                _ => {
                    // No knockout
                    continue;
                }
            }
        }

        // For hatch zones, vias also need thermal treatment to prevent isolation inside hatch
        // holes.  We respect the zone connection type just like pads: THERMAL gets a relief
        // knockout, FULL connects directly to the webbing, NONE is handled in
        // buildCopperItemClearances.
        if zone.fill_mode() == ZoneFillMode::HatchPattern {
            for (track_id, track) in ctx.board.tracks() {
                let Track::Via(via) = track else {
                    continue;
                };
                let via_item: ItemId = track_id.into();

                if !via.is_on_layer(ctx, layer) {
                    continue;
                }

                let mut via_bbox = via.bounding_box(ctx);
                via_bbox.inflate(self.worst_clearance);

                if !via_bbox.intersects(&zone_bbox) {
                    continue;
                }

                // Deduplicate coincident vias (circular, so use max of drill and width)
                let via_effective_size = via.drill_value(ctx).max(via.width(layer));
                let via_key = ViaKnockoutKey {
                    position: via.position(ctx),
                    effective_size: via_effective_size,
                    net_code: track.net_code().0,
                };

                if !processed_vias.insert(via_key) {
                    continue;
                }

                let padstack = via.padstack();
                let mut no_connection = track.net_code().0 != zone_net
                    || (padstack.unconnected_layer_mode() == UnconnectedLayerMode::StartEndOnly
                        && layer != padstack.drill().start
                        && layer != padstack.drill().end);

                if via.zone_layer_override(layer) == ZoneLayerOverride::ForceNoZoneConnection {
                    no_connection = true;
                }

                // Check if this layer is affected by backdrill or post-machining
                if via.is_backdrilled_or_post_machined(ctx, layer) {
                    no_connection = true;

                    // Either drill slot may hold a bottom backdrill, so read the whole
                    // padstack
                    let knockout_size = padstack.max_hole_size();

                    if knockout_size > 0 {
                        let clearance = zone.local_clearance().unwrap_or(0);

                        transform_circle_to_polygon(
                            &mut holes,
                            via.position(ctx),
                            knockout_size / 2 + clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                            0,
                        );
                    }
                }

                if no_connection {
                    continue;
                }

                let connection =
                    eval_zone_connection(ctx, via_item, zone_item, layer).zone_connection;

                match connection {
                    ZoneConnection::Thermal => {
                        let thermal_gap = eval_rules(
                            ctx,
                            DrcConstraintT::ThermalReliefGapConstraint,
                            via_item,
                            zone_item,
                            layer,
                        )
                        .value()
                        .min();

                        // Only force thermal if the via is small enough to be isolated in a
                        // hatch hole.  A via wider than the hole width will always touch the
                        // webbing naturally.
                        if thermal_gap > 0 {
                            thermal_connection_pads.push(via_item);
                            self.add_knockout(ctx, via_item, layer, thermal_gap, &mut holes);
                        }
                    }
                    ZoneConnection::None => {
                        // Will be handled by buildCopperItemClearances
                    }
                    _ => {
                        // No knockout. A small via in a hatch hole would be isolated, so
                        // register it to drop that hole and keep the via on the webbing.
                        solid_connection_items.push(via_item);
                    }
                }
            }
        }

        fill.boolean_subtract(&holes);
    }

    /// Port of `ZONE_FILLER::buildCopperItemClearances( aZone, aLayer, aNoConnectionPads,
    /// aHoles, aIncludeZoneClearances )`: removes clearance from the shape for copper items
    /// which share the zone's layer but are not connected to it.
    pub(crate) fn build_copper_item_clearances(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        no_connection_pads: &[PadId],
        holes: &mut ShapePolySet,
        include_zone_clearances: bool,
    ) {
        let zone_item: ItemId = zone_id.into();
        let zone_net = zone.base.net.0;

        // Deduplication sets for coincident items
        let mut processed_pads: HashSet<PadKnockoutKey> = HashSet::new();
        let mut processed_vias: HashSet<ViaKnockoutKey> = HashSet::new();
        let mut processed_tracks: HashSet<TrackKnockoutKey> = HashSet::new();

        // A small extra clearance to be sure actual track clearances are not smaller than
        // requested clearance due to many approximations in calculations, like arc to segment
        // approx, rounding issues, etc.
        let mut zone_boundingbox = zone.bounding_box(ctx);
        let extra_margin = PCB_IU_SCALE.mm_to_iu(EXTRA_CLEARANCE_MM);

        // Items outside the zone bounding box are skipped, so it needs to be inflated by the
        // largest clearance value found in the netclasses and rules
        zone_boundingbox.inflate(self.worst_clearance + extra_margin);

        let eval = |constraint: DrcConstraintT, item: ItemId| -> i32 {
            eval_rules_for_items(ctx, constraint, zone_item, item, layer)
        };

        // Add non-connected pad clearances
        //
        let knockout_pad_clearance = |pad_id: PadId, pad: &Pad, holes: &mut ShapePolySet| {
            let pad_item: ItemId = pad_id.into();
            let init_gap = eval(DrcConstraintT::PhysicalClearanceConstraint, pad_item);
            let mut gap = init_gap;
            let has_hole = pad.drill_size(ctx).x > 0;
            let flash_layer = pad.flash_layer(ctx, layer);
            let plated_hole = has_hole && pad.attribute() == PadAttrib::Pth;

            if flash_layer || plated_hole {
                gap = gap.max(eval(DrcConstraintT::ClearanceConstraint, pad_item));
            }

            if flash_layer && gap >= 0 {
                self.add_knockout(ctx, pad_item, layer, gap + extra_margin, holes);
            }

            if has_hole {
                let mut hole_gap = eval(DrcConstraintT::PhysicalHoleClearanceConstraint, pad_item);
                hole_gap = hole_gap.max(eval(DrcConstraintT::HoleClearanceConstraint, pad_item));

                // NPTH do not need copper clearance gaps to their holes
                if pad.attribute() == PadAttrib::Npth {
                    gap = init_gap;
                }

                gap = gap.max(hole_gap);

                if gap >= 0 {
                    self.add_hole_knockout(ctx, pad, gap + extra_margin, holes);
                }
            }

            // Handle backdrill and post-machining knockouts
            if pad.is_backdrilled_or_post_machined(ctx, layer) {
                let knockout_size = pad.lib_padstack().max_hole_size();

                if knockout_size > 0 {
                    // The bore widens the drill, so it keeps the drill's copper clearance
                    let clearance = gap.max(0) + extra_margin;

                    transform_circle_to_polygon(
                        holes,
                        pad.position(ctx),
                        knockout_size / 2 + clearance,
                        self.max_error,
                        ErrorLoc::Outside,
                        0,
                    );
                }
            }
        };

        for &pad_id in no_connection_pads {
            let Some(pad) = ctx.board.pad(pad_id) else {
                continue;
            };

            // Deduplicate coincident pads (skip custom pads - they have complex shapes)
            if let Some(pad_key) = pad_knockout_key(ctx, pad, layer) {
                if !processed_pads.insert(pad_key) {
                    continue;
                }
            }

            knockout_pad_clearance(pad_id, pad, holes);
        }

        // Add non-connected track clearances
        //
        let knockout_track_clearance = |track_item: ItemId, track: &Track, holes: &mut ShapePolySet| {
            let Some(track_ref) = ctx.item(track_item) else {
                return;
            };

            if !track_ref.bounding_box().intersects(&zone_boundingbox) {
                return;
            }

            let mut same_net = track.net_code().0 == zone_net;

            if !zone.is_teardrop_area() && zone_net == 0 {
                same_net = false;
            }

            let mut gap = eval(DrcConstraintT::PhysicalClearanceConstraint, track_item);

            if let Track::Via(via) = track {
                if via.zone_layer_override(layer) == ZoneLayerOverride::ForceNoZoneConnection {
                    same_net = false;
                }
            }

            if !same_net {
                gap = gap.max(eval(DrcConstraintT::ClearanceConstraint, track_item));
            }

            if let Track::Via(via) = track {
                if via.flash_layer(ctx, layer) && gap > 0 {
                    track_ref.transform_shape_to_polygon(
                        holes,
                        layer,
                        gap + extra_margin,
                        self.max_error,
                        ErrorLoc::Outside,
                        false,
                    );
                }

                let mut hole_gap = eval(DrcConstraintT::PhysicalHoleClearanceConstraint, track_item);

                if !same_net {
                    hole_gap = hole_gap.max(eval(DrcConstraintT::HoleClearanceConstraint, track_item));
                }

                gap = gap.max(hole_gap);

                if gap >= 0 {
                    let radius = via.drill_value(ctx) / 2;

                    transform_circle_to_polygon(
                        holes,
                        via.position(ctx),
                        radius + gap + extra_margin,
                        self.max_error,
                        ErrorLoc::Outside,
                        0,
                    );
                }

                // Handle backdrill and post-machining knockouts
                if via.is_backdrilled_or_post_machined(ctx, layer) {
                    let knockout_size = via.padstack().max_hole_size();

                    if knockout_size > 0 {
                        // The bore widens the drill, so it keeps the drill's copper clearance
                        let clearance = gap.max(0) + extra_margin;

                        transform_circle_to_polygon(
                            holes,
                            via.position(ctx),
                            knockout_size / 2 + clearance,
                            self.max_error,
                            ErrorLoc::Outside,
                            0,
                        );
                    }
                }
            } else if gap >= 0 {
                track_ref.transform_shape_to_polygon(
                    holes,
                    layer,
                    gap + extra_margin,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );
            }
        };

        let hits = match self.track_index.get(&layer) {
            Some(index) => query_index(index, &zone_boundingbox),
            None => Vec::new(),
        };

        for hit in &hits {
            let Some(track_ref) = ctx.item(hit.item) else {
                continue;
            };
            let Some(track) = track_ref.as_track() else {
                continue;
            };

            if !track_ref.is_on_layer(layer) {
                continue;
            }

            // Deduplicate coincident tracks and vias
            if let Track::Via(via) = track {
                let via_effective_size = via.drill_value(ctx).max(via.width(layer));
                let via_key = ViaKnockoutKey {
                    position: via.position(ctx),
                    effective_size: via_effective_size,
                    net_code: track.net_code().0,
                };

                if !processed_vias.insert(via_key) {
                    continue;
                }
            } else {
                let t = track.as_track();
                let track_key = TrackKnockoutKey::new(t.start(), t.end(), track.width());

                if !processed_tracks.insert(track_key) {
                    continue;
                }
            }

            knockout_track_clearance(hit.item, track, holes);
        }

        // Add graphic item clearances.
        //
        let knockout_graphic_clearance = |item: ItemId, holes: &mut ShapePolySet| {
            let Some(item_ref) = ctx.item(item) else {
                return;
            };

            let mut shape_net = -1;

            if item_ref.type_() == KicadT::PcbShapeT {
                shape_net = item_ref.net_code().map_or(-1, |n| n.0);
            }

            let mut same_net = shape_net == zone_net;

            if !zone.is_teardrop_area() && zone_net == 0 {
                same_net = false;
            }

            // A item on the Edge_Cuts or Margin is always seen as on any layer:
            if item_ref.is_on_layer(layer)
                || item_ref.is_on_layer(PcbLayerId::EdgeCuts)
                || item_ref.is_on_layer(PcbLayerId::Margin)
            {
                if item_ref.bounding_box().intersects(&zone_boundingbox) {
                    let mut ignore_line_widths = false;
                    let mut gap = eval(DrcConstraintT::PhysicalClearanceConstraint, item);

                    if item_ref.is_on_layer(layer) && !same_net {
                        gap = gap.max(eval(DrcConstraintT::ClearanceConstraint, item));
                    } else if item_ref.is_on_layer(PcbLayerId::EdgeCuts) {
                        gap = gap.max(eval(DrcConstraintT::EdgeClearanceConstraint, item));
                        ignore_line_widths = true;
                    } else if item_ref.is_on_layer(PcbLayerId::Margin) {
                        gap = gap.max(eval(DrcConstraintT::EdgeClearanceConstraint, item));
                    }

                    if gap >= 0 {
                        gap += extra_margin;
                        self.add_graphic_knockout(ctx, item, layer, gap, ignore_line_widths, holes);
                    }
                }
            }
        };

        let knockout_courtyard_clearance = |fp_id: FootprintId, holes: &mut ShapePolySet| {
            let fp_item: ItemId = fp_id.into();
            let (Some(fp_ref), Some(footprint)) = (ctx.item(fp_item), ctx.board.footprint(fp_id))
            else {
                return;
            };

            if fp_ref.bounding_box().intersects(&zone_boundingbox) {
                let gap = eval(DrcConstraintT::PhysicalClearanceConstraint, fp_item);

                // For internal copper layers, GetCourtyard( aLayer ) always returns the
                // front courtyard because IsBackLayer() is false for all internal layers.
                // Use the footprint's own layer to select the correct courtyard instead.
                let courtyard_side = if is_inner_copper_layer(layer) {
                    footprint.layer(ctx)
                } else {
                    layer
                };

                if gap == 0 {
                    holes.append_set(&footprint.courtyard(ctx, courtyard_side));
                } else if gap > 0 {
                    let mut hole = footprint.courtyard(ctx, courtyard_side);
                    hole.inflate(gap, CornerStrategy::RoundAllCorners, self.max_error, false);
                    holes.append_set(&hole);
                }
            }
        };

        // Don't knock out holes for graphic items which implement a net-tie to the zone's
        // net on the layer being filled.  Net-tie footprints are rare, so build the set on
        // first use.
        let mut net_tie_pads: BTreeMap<FootprintId, BTreeSet<PadId>> = BTreeMap::new();

        let mut allowed_net_tie_pads = |fp_id: FootprintId| -> BTreeSet<PadId> {
            if let Some(set) = net_tie_pads.get(&fp_id) {
                return set.clone();
            }

            let mut set = BTreeSet::new();

            if let Some(footprint) = ctx.board.footprint(fp_id) {
                if footprint.is_net_tie() {
                    for (pad_id, pad) in ctx.board.footprint_pads(fp_id) {
                        let mut same_net = pad.net_code().0 == zone_net;

                        if !zone.is_teardrop_area() && zone_net == 0 {
                            same_net = false;
                        }

                        if same_net {
                            if pad.is_on_layer(ctx, layer) {
                                set.insert(pad_id);
                            }

                            for other in footprint.net_tie_pads(ctx, pad_id) {
                                if ctx.board.pad(other).is_some_and(|o| o.is_on_layer(ctx, layer)) {
                                    set.insert(other);
                                }
                            }
                        }
                    }
                }
            }

            net_tie_pads.insert(fp_id, set.clone());
            set
        };

        let gfx_hits = query_index(&self.graphic_index, &zone_boundingbox);
        let fp_hits = query_index(&self.footprint_index, &zone_boundingbox);

        // Merge back into the original order: each footprint courtyard, then its graphics.
        let mut gi = 0;
        let mut fi = 0;

        while gi < gfx_hits.len() || fi < fp_hits.len() {
            if fi < fp_hits.len() && (gi >= gfx_hits.len() || fp_hits[fi].seq <= gfx_hits[gi].seq)
            {
                if let Some(fp_id) = fp_hits[fi].item.typed::<kicad_pcb::footprint::Footprint>() {
                    knockout_courtyard_clearance(fp_id, holes);
                }

                fi += 1;
                continue;
            }

            let hit = gfx_hits[gi];
            gi += 1;

            let item = hit.item;
            let mut skip_item = false;

            // Only a footprint's own graphics can form the net tie.
            if let Some(owner) = hit.owner {
                let is_ref_or_value = ctx.board.footprint(owner).is_some_and(|fp| {
                    [FieldT::Reference, FieldT::Value]
                        .into_iter()
                        .any(|f| fp.field(ctx, f).map(ItemId::from) == Some(item))
                });

                if !is_ref_or_value && ctx.item(item).is_some_and(|i| i.is_on_layer(layer)) {
                    let allowed = allowed_net_tie_pads(owner);

                    if !allowed.is_empty() {
                        if let Some(item_ref) = ctx.item(item) {
                            let item_bbox = item_ref.bounding_box();
                            let item_shape = item_ref.effective_shape(
                                PcbLayerId::UndefinedLayer,
                                Flashing::Default,
                                DrcConstraintT::NullConstraint,
                            );

                            for pad_id in &allowed {
                                let Some(pad) = ctx.board.pad(*pad_id) else {
                                    continue;
                                };

                                if pad.bounding_box(ctx).intersects(&item_bbox)
                                    && pad
                                        .effective_shape(
                                            ctx,
                                            layer,
                                            Flashing::Default,
                                            DrcConstraintT::NullConstraint,
                                        )
                                        .collide(item_shape.as_ref(), 0, None, None)
                                {
                                    skip_item = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if !skip_item {
                knockout_graphic_clearance(item, holes);
            }
        }

        // Add non-connected zone clearances
        //
        let knockout_zone_clearance = |knockout_id: ZoneId, knockout: &Zone, holes: &mut ShapePolySet| {
            // If the zones share no common layers
            if !knockout.layer_set.test(layer) {
                return;
            }

            if knockout.is_rule_area() {
                if knockout.bounding_box(ctx).intersects(&zone_boundingbox)
                    && knockout.do_not_allow_zone_fills
                    && !zone.is_teardrop_area()
                {
                    // Keepouts use outline with no clearance
                    knockout.transform_smoothed_outline_to_polygon(
                        ctx,
                        holes,
                        layer,
                        0,
                        self.max_error,
                        ErrorLoc::Outside,
                        None,
                    );
                }
            } else if knockout.higher_priority(zone)
                && !knockout.same_net(zone)
                && self.zone_knockout_may_interact(ctx, zone, knockout)
            {
                let knockout_item: ItemId = knockout_id.into();
                let mut gap = eval(DrcConstraintT::PhysicalClearanceConstraint, knockout_item);
                gap = gap.max(eval(DrcConstraintT::ClearanceConstraint, knockout_item));

                // Negative clearance permits zones to short
                if gap < 0 {
                    return;
                }

                let mut poly = ShapePolySet::new();
                knockout.transform_shape_to_polygon(
                    ctx,
                    &mut poly,
                    layer,
                    gap + extra_margin,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );
                holes.append_set(&poly);
            }
        };

        if let Some(index) = self.zone_index.get(&layer) {
            // The knockout reach is the wider of the two windows tested above.
            let hits = query_index(index, &self.zone_knockout_query_box(ctx, zone));

            for hit in hits {
                let Some(other_id) = hit.item.typed::<Zone>() else {
                    continue;
                };
                let Some(other_zone) = ctx.board.zone(other_id) else {
                    continue;
                };

                // Zone knockouts are deferred past the min-width cycle so the refill can cache
                // the fill before them. A teardrop cannot move, so knock it out here with the
                // tracks.
                if !include_zone_clearances && !other_zone.is_teardrop_area() {
                    continue;
                }

                knockout_zone_clearance(other_id, other_zone, holes);
            }
        }

        holes.simplify();
    }

    /// Port of `ZONE_FILLER::buildDifferentNetZoneClearances( aZone, aLayer, aHoles )`:
    /// builds clearance knockout holes for higher-priority zones on different nets. This is
    /// separated from buildCopperItemClearances to allow caching before zone knockouts.
    pub(crate) fn build_different_net_zone_clearances(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        zone_id: ZoneId,
        layer: PcbLayerId,
        holes: &mut ShapePolySet,
    ) {
        let zone_item: ItemId = zone_id.into();
        let extra_margin = PCB_IU_SCALE.mm_to_iu(EXTRA_CLEARANCE_MM);

        // Keepout zones (rule areas) are excluded here because they are subtracted earlier in
        // the fill process, before the deflate/inflate min-width cycle.  Subtracting them here
        // would trigger a second deflate/inflate pass that creates artifacts along curved
        // keepout boundaries (issue 23515).
        let knockout_zone_clearance = |knockout_id: ZoneId, knockout: &Zone, holes: &mut ShapePolySet| {
            if knockout.is_rule_area() {
                return;
            }

            // buildCopperItemClearances() knocks teardrops out before the min-width cycle.
            if knockout.is_teardrop_area() {
                return;
            }

            if !knockout.layer_set.test(layer) {
                return;
            }

            if knockout.higher_priority(zone)
                && !knockout.same_net(zone)
                && self.zone_knockout_may_interact(ctx, zone, knockout)
            {
                let knockout_item: ItemId = knockout_id.into();
                let mut gap = eval_rules_for_items(
                    ctx,
                    DrcConstraintT::PhysicalClearanceConstraint,
                    zone_item,
                    knockout_item,
                    layer,
                );
                gap = gap.max(eval_rules_for_items(
                    ctx,
                    DrcConstraintT::ClearanceConstraint,
                    zone_item,
                    knockout_item,
                    layer,
                ));

                if gap < 0 {
                    return;
                }

                let mut poly = ShapePolySet::new();
                knockout.transform_shape_to_polygon(
                    ctx,
                    &mut poly,
                    layer,
                    gap + extra_margin,
                    self.max_error,
                    ErrorLoc::Outside,
                    false,
                );
                holes.append_set(&poly);
            }
        };

        if let Some(index) = self.zone_index.get(&layer) {
            let hits = query_index(index, &self.zone_knockout_query_box(ctx, zone));

            for hit in hits {
                let Some(other_id) = hit.item.typed::<Zone>() else {
                    continue;
                };

                if let Some(other_zone) = ctx.board.zone(other_id) {
                    knockout_zone_clearance(other_id, other_zone, holes);
                }
            }
        }

        holes.simplify();
    }

    /// Port of `ZONE_FILLER::subtractHigherPriorityZones( aZone, aLayer, aRawFill )`: removes
    /// the outlines of higher-priority zones with the same net.  These zones should be in
    /// charge of the fill parameters within their own outlines.
    pub(crate) fn subtract_higher_priority_zones(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        raw_fill: &mut ShapePolySet,
    ) {
        let zone_bbox = zone.bounding_box(ctx);
        let mut knockouts = ShapePolySet::new();

        if let Some(index) = self.zone_index.get(&layer) {
            let hits = query_index(index, &zone_bbox);

            for hit in hits {
                let Some(other_id) = hit.item.typed::<Zone>() else {
                    continue;
                };
                let Some(other_zone) = ctx.board.zone(other_id) else {
                    continue;
                };

                // Don't use `HigherPriority()` here because we only want explicitly-higher
                // priorities, not equal-priority zones.
                let higher_priority_same_net = other_zone.same_net(zone)
                    && other_zone.assigned_priority() > zone.assigned_priority();

                if higher_priority_same_net && !other_zone.is_teardrop_area() {
                    // collectZoneOutline
                    if !other_zone.layer_set.test(layer) {
                        continue;
                    }

                    if other_zone.bounding_box(ctx).intersects(&zone_bbox) {
                        append_zone_outline_without_arcs(ctx, other_zone, &mut knockouts);
                    }
                }
            }
        }

        if knockouts.outline_count() > 0 {
            raw_fill.boolean_subtract(&knockouts);
        }
    }

    /// The keepout holes of `fillCopperZone` (`collectKeepoutHoles`, iterative refill only):
    /// the smoothed outlines of every board / footprint keepout that forbids zone fills on
    /// `layer`. Returns whether any was added.
    pub(crate) fn collect_keepout_holes(
        &self,
        ctx: &BoardCtx<'_>,
        zone: &Zone,
        layer: PcbLayerId,
        clearance_holes: &mut ShapePolySet,
    ) -> bool {
        let zone_boundingbox = zone.bounding_box(ctx);
        let mut added_keepout_holes = false;

        for candidate_id in board_and_footprint_zones(ctx) {
            let Some(candidate) = ctx.board.zone(candidate_id) else {
                continue;
            };

            if zone.is_teardrop_area() {
                continue;
            }

            if !is_zone_fill_keepout(ctx, candidate, layer, &zone_boundingbox) {
                continue;
            }

            candidate.transform_smoothed_outline_to_polygon(
                ctx,
                clearance_holes,
                layer,
                0,
                self.max_error,
                ErrorLoc::Outside,
                None,
            );
            added_keepout_holes = true;
        }

        added_keepout_holes
    }
}

/// The anonymous `appendZoneOutlineWithoutArcs( aZone, aPolys )`.
pub(crate) fn append_zone_outline_without_arcs(
    ctx: &BoardCtx<'_>,
    zone: &Zone,
    polys: &mut ShapePolySet,
) {
    let mut outline = zone.board_outline(ctx);

    if outline.arc_count() != 0 {
        outline.clear_arcs();
    }

    polys.append_set(&outline);
}
