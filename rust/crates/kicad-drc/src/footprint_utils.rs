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

//! Port of the board-level checks of `pcbnew/footprint_utils.cpp` (`FOOTPRINT_UTILS`) used by
//! the footprint checks DRC provider: `CheckFootprintAttributes`, `CheckPads`, `CheckNetTies`
//! and `CheckNetTiePadGroups`.
//!
//! Not ported (no DRC caller): `ComputeFootprintShift` (footprint update), `CheckShortingPads`
//! and `CheckClippedSilk` (footprint editor checker).

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::layer_ids::Flashing;
use kicad_core::lset::Lset;
use kicad_core::units_provider::UnitsProvider;
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::footprint::footprint_attr::{FP_SMD, FP_THROUGH_HOLE};
use kicad_pcb::ids::{FootprintId, ItemId, PadId};
use kicad_pcb::item::RecurseMode;
use kicad_pcb::pad::Pad;
use kicad_pcb::pad_utils::{PadCheckResult, check_pad};
use kicad_pcb::{AnyItem, BoardCtx, ItemRef};

use crate::constraint_types::DrcConstraintT;

/// Port of `FOOTPRINT_UTILS::CheckFootprintAttributes`: reports a footprint type (SMD /
/// through hole) that does not match its pads.
pub fn check_footprint_attributes(
    ctx: &BoardCtx<'_>,
    fp: FootprintId,
    error_handler: &mut dyn FnMut(&str),
) {
    let Some(footprint) = ctx.board.footprint(fp) else {
        return;
    };

    let likely_attr = footprint.likely_attribute(ctx) & (FP_SMD | FP_THROUGH_HOLE);
    let set_attr = footprint.attributes() & (FP_SMD | FP_THROUGH_HOLE);

    if set_attr != 0 && likely_attr != 0 && set_attr != likely_attr {
        let mut msg = String::new();

        match likely_attr {
            FP_THROUGH_HOLE => {
                msg = format!(
                    "(expected 'Through hole'; actual '{}')",
                    footprint.type_name()
                );
            }
            FP_SMD => {
                msg = format!("(expected 'SMD'; actual '{}')", footprint.type_name());
            }
            _ => {}
        }

        error_handler(&msg);
    }
}

/// Port of `FOOTPRINT_UTILS::CheckPads`: `PAD_UTILS::CheckPad( pad, aUnitsProvider, false, ... )`
/// for every pad of the footprint, in pad order.
pub fn check_pads(
    ctx: &BoardCtx<'_>,
    fp: FootprintId,
    units: &UnitsProvider<'_>,
    error_handler: &mut dyn FnMut(PadId, &Pad, &PadCheckResult),
) {
    for (id, pad) in ctx.board.footprint_pads(fp) {
        check_pad(ctx, pad, units, false, &mut |result: &PadCheckResult| {
            error_handler(id, pad, result)
        });
    }
}

/// Port of `FOOTPRINT_UTILS::CheckNetTies`: per copper layer, every outline of the footprint's
/// copper that touches pads of different net-tie groups (or of no group) is reported with
/// (shorting item, first pad, other pad) or (first pad, other pad) and the position.
pub fn check_net_ties(
    ctx: &BoardCtx<'_>,
    fp: FootprintId,
    error_handler: &mut dyn FnMut(Option<ItemId>, Option<ItemId>, Option<ItemId>, Vector2I),
) {
    let Some(footprint) = ctx.board.footprint(fp) else {
        return;
    };

    // First build a map from pad numbers to allowed-shorting-group indexes.  This ends up being
    // something like O(3n), but it still beats O(n^2) for large numbers of pads.

    let pad_number_to_group_idx_map: BTreeMap<String, i32> =
        footprint.map_pad_numbers_to_net_tie_groups(ctx);

    // std::map::operator[]: a number missing from the map reads as group 0.
    let group_of = |number: &str| {
        pad_number_to_group_idx_map
            .get(number)
            .copied()
            .unwrap_or(0)
    };

    // Now collect all the footprint items which are on copper layers

    let mut copper_items: Vec<ItemRef<'_>> = Vec::new();

    for (id, _) in ctx.board.footprint_drawings(fp) {
        let Some(item) = ctx.item(id) else {
            continue;
        };

        if item.as_dyn().is_on_copper_layer(ctx) {
            copper_items.push(item);
        }

        for descendent in item.children(RecurseMode::Recurse) {
            if descendent.as_dyn().is_on_copper_layer(ctx) {
                copper_items.push(descendent);
            }
        }
    }

    for (_, zone) in ctx.board.footprint_zones(fp) {
        let item = ctx.view(AnyItem::Zone(zone));

        if !zone.is_rule_area() && item.as_dyn().is_on_copper_layer(ctx) {
            copper_items.push(item);
        }
    }

    for (_, field) in ctx.board.footprint_fields(fp) {
        let item = ctx.view(AnyItem::Field(field));

        if item.as_dyn().is_on_copper_layer(ctx) {
            copper_items.push(item);
        }
    }

    // Any copper layer which carries footprint copper can form a shorting group, including the
    // inner layers used by buried graphics and non-component pads.

    let mut copper_layers = Lset::new();

    for item in &copper_items {
        copper_layers = &copper_layers | &item.layer_set();
    }

    let pads: Vec<(PadId, &Pad)> = ctx.board.footprint_pads(fp).collect();

    for (_, pad) in &pads {
        copper_layers = &copper_layers | &pad.layer_set(ctx);
    }

    let max_error = ctx.max_error();

    for layer in copper_layers.cu_stack().iter().copied() {
        // Next, build a polygon-set for the copper on this layer.  We don't really care about
        // nets here, we just want to end up with a set of outlines describing the distinct
        // copper polygons of the footprint.

        let mut copper_outlines = ShapePolySet::new();
        let mut outline_idx_to_pads_map: BTreeMap<i32, Vec<(PadId, &Pad)>> = BTreeMap::new();

        for item in &copper_items {
            if item.is_on_layer(layer) {
                item.transform_shape_to_polygon(
                    &mut copper_outlines,
                    layer,
                    0,
                    max_error,
                    ErrorLoc::Outside,
                    false,
                );
            }
        }

        copper_outlines.simplify();

        // Index each pad to the outline in the set that it is part of.

        for &(pad_id, pad) in &pads {
            // PAD::GetEffectiveShape() will report the shape of the pad inferred for the given
            // layer, but for single-layer pads like SMD/Non-component, this does not mean that
            // the pad actually exists as copper on this layer.
            // So check that there is actually copper here before assuming it can collide with
            // the outline.
            if !pad.is_on_layer(ctx, layer) {
                continue;
            }

            let pad_shape = pad.effective_shape(
                ctx,
                layer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );

            for ii in 0..copper_outlines.outline_count() {
                if pad_shape.collide(copper_outlines.c_outline(ii), 0, None, None) {
                    outline_idx_to_pads_map
                        .entry(ii)
                        .or_default()
                        .push((pad_id, pad));
                }
            }
        }

        // Finally, ensure that each outline which contains multiple pads has all its pads
        // listed in an allowed-shorting group.

        for (&outline_idx, outline_pads) in &outline_idx_to_pads_map {
            if outline_pads.len() > 1 {
                let (first_id, first_pad) = outline_pads[0];
                let first_group_idx = group_of(first_pad.number());

                for &(this_id, this_pad) in &outline_pads[1..] {
                    let this_group_idx = group_of(this_pad.number());

                    if this_group_idx < 0 || this_group_idx != first_group_idx {
                        let mut shorting_item: Option<ItemId> = None;
                        let mut pos = (first_pad.position(ctx) + this_pad.position(ctx)) / 2;

                        pos = copper_outlines
                            .c_outline(outline_idx)
                            .nearest_point(pos, true);

                        for item in &copper_items {
                            if item.as_dyn().hit_test(ctx, pos, 1) {
                                shorting_item = item.id();
                                break;
                            }
                        }

                        if shorting_item.is_some() {
                            error_handler(
                                shorting_item,
                                Some(first_id.into()),
                                Some(this_id.into()),
                                pos,
                            );
                        } else {
                            error_handler(Some(first_id.into()), Some(this_id.into()), None, pos);
                        }
                    }
                }
            }
        }
    }
}

/// Port of `FOOTPRINT_UTILS::CheckNetTiePadGroups`: reports net-tie group entries naming no
/// pad of the footprint (and pads in more than one group).
pub fn check_net_tie_pad_groups(
    ctx: &BoardCtx<'_>,
    fp: FootprintId,
    error_handler: &mut dyn FnMut(&str),
) {
    let Some(footprint) = ctx.board.footprint(fp) else {
        return;
    };

    let mut pad_numbers: BTreeSet<String> = BTreeSet::new();

    for pad_number in footprint.map_pad_numbers_to_net_tie_groups(ctx).keys() {
        let pad = footprint
            .find_pad_by_number(ctx, pad_number, None)
            .and_then(|id| ctx.board.pad(id));

        match pad {
            None => {
                let msg = format!("(net-tie pad group contains unknown pad number {pad_number})");
                error_handler(&msg);
            }
            Some(pad) => {
                if !pad_numbers.insert(pad.number().to_string()) {
                    let msg =
                        format!("(pad {pad_number} appears in more than one net-tie pad group)");
                    error_handler(&msg);
                }
            }
        }
    }
}
