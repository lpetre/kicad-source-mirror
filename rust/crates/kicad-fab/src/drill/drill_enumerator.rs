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

//! Port of `pcbnew/drill/drill_enumerator.h` / `drill_enumerator.cpp`: the holes of a board
//! for one drill span (`EnumerateDrillOperations`), the spans themselves
//! (`EnumerateDrillSpans`) and `IsDrillSlot`.
//!
//! Board access follows `board-model.md`: vias come from `board.tracks()` (C++ `Tracks()`
//! order), pads from `board.footprints()` x `board.footprint_pads()`; board-frame pad values
//! (`GetPosition`, `GetOrientation`, `GetDrillSize`, `HasHole`) through the context.

use std::collections::BTreeSet;

use kicad_core::layer_ids::{PcbLayerId, is_copper_layer_lower_than};
use kicad_geom::{ANGLE_0, Vector2I};
use kicad_pcb::BoardCtx;
use kicad_pcb::ids::{PadId, TrackId};
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{
    DrillProps, PadAttrib, PadDrillPostMachiningMode, PadDrillShape, PadProp, PostMachiningProps,
};
use kicad_pcb::pcb_track::{
    CappingMode, CoveringMode, FillingMode, PcbVia, PluggingMode, TentingMode, Track,
};

use super::drill_operation::{
    DrillOpKind, DrillOperation, DrillPostMachining, DrillQuery, DrillSource,
};
use super::drill_span::{DrillLayerPair, DrillSpan, HoleAttribute};
use super::via_access;

const F_CU: PcbLayerId = PcbLayerId::FCu;
const B_CU: PcbLayerId = PcbLayerId::BCu;
const UNDEFINED_LAYER: PcbLayerId = PcbLayerId::UndefinedLayer;

/// Port of the anonymous-namespace `fromPadstack( PADSTACK::POST_MACHINING_PROPS )`.
fn from_padstack(props: &PostMachiningProps) -> DrillPostMachining {
    DrillPostMachining {
        mode: props.mode.unwrap_or(PadDrillPostMachiningMode::Unknown),
        size: props.size,
        depth: props.depth,
        angle: props.angle,
    }
}

/// Port of the anonymous-namespace `isBackdrillSet( PADSTACK::DRILL_PROPS )`.
fn is_backdrill_set(drill: &DrillProps) -> bool {
    drill.start != UNDEFINED_LAYER
        && drill.end != UNDEFINED_LAYER
        && (drill.size.x > 0 || drill.size.y > 0)
}

/// Port of the anonymous-namespace `backdrillOperation`: the common part of a via or pad
/// backdrill whose stored start / end equal the span's.
fn backdrill_operation(
    drill: &DrillProps,
    kind: DrillOpKind,
    span: &DrillSpan,
    item: DrillSource,
    item_uuid: kicad_core::Kiid,
) -> Option<DrillOperation> {
    if !is_backdrill_set(drill)
        || drill.start != span.drill_start_layer()
        || drill.end != span.drill_end_layer()
    {
        return None;
    }

    let mut op = DrillOperation {
        kind,
        source_item: Some(item),
        source_id: item_uuid,
        not_plated: true,
        top_layer: span.top_layer(),
        bottom_layer: span.bottom_layer(),
        drill_start: drill.start,
        drill_end: drill.end,
        size_xy: drill.size,
        diameter: drill.size.x,
        ..DrillOperation::default()
    };

    if drill.size.y > 0 {
        op.diameter = if op.diameter > 0 { op.diameter.min(drill.size.y) } else { drill.size.y };
    }

    op.is_slot = drill.shape != PadDrillShape::Circle && drill.size.x != drill.size.y;
    Some(op)
}

/// Port of the anonymous-namespace `enumerateVia`.
fn enumerate_via(
    ctx: &BoardCtx<'_>,
    via_id: TrackId,
    via: &PcbVia,
    query: &DrillQuery,
    out: &mut Vec<DrillOperation>,
) {
    let span = &query.span;
    let via_uuid = via.track.base.item.uuid;

    let stub_length = |start: PcbLayerId, end: PcbLayerId| -> Option<i32> {
        if start == UNDEFINED_LAYER || end == UNDEFINED_LAYER {
            return None;
        }

        let stackup = ctx.board.design_settings().stackup_descriptor();

        Some(stackup.get_drill_depth(start, end))
    };

    if span.is_backdrill {
        let mut try_backdrill = |drill: &DrillProps, kind: DrillOpKind| {
            let Some(mut op) =
                backdrill_operation(drill, kind, span, DrillSource::Via(via_id), via_uuid)
            else {
                return;
            };

            op.attribute = HoleAttribute::HoleViaBackdrill;
            op.orientation = ANGLE_0;
            op.position = via.track.start;
            op.stub_length = stub_length(drill.start, drill.end);
            // Backdrills remove the via barrel, so via surface protection is not inherited
            op.filled = drill.is_filled.unwrap_or(false);
            op.capped = drill.is_capped.unwrap_or(false);
            out.push(op);
        };

        try_backdrill(via.padstack.secondary_drill(), DrillOpKind::SecondaryDrill);
        try_backdrill(via.padstack.tertiary_drill(), DrillOpKind::TertiaryDrill);
        return;
    }

    let hole_size = via_access::drill_value(ctx, via);

    if hole_size == 0 {
        return;
    }

    let (top_layer, bottom_layer) = via_access::layer_pair(ctx, via);

    if (top_layer, bottom_layer) != span.pair() && (bottom_layer, top_layer) != span.pair() {
        return;
    }

    let mut op = DrillOperation {
        source_item: Some(DrillSource::Via(via_id)),
        source_id: via_uuid,
        ..DrillOperation::default()
    };

    if span.pair() == (F_CU, B_CU) {
        op.attribute = HoleAttribute::HoleViaThrough;
    } else {
        op.attribute = HoleAttribute::HoleViaBuried;
    }

    op.orientation = ANGLE_0;
    op.diameter = hole_size;
    op.not_plated = false;
    op.size_xy = Vector2I::new(hole_size, hole_size);
    op.position = via.track.start;
    op.top_layer = top_layer;
    op.bottom_layer = bottom_layer;
    op.filled = via_access::resolve_filling_mode(ctx, via) == FillingMode::Filled;
    op.capped = via_access::resolve_capping_mode(ctx, via) == CappingMode::Capped;

    if op.top_layer == F_CU {
        op.top_covered = via_access::resolve_front_covering_mode(ctx, via) == CoveringMode::Covered;
        op.top_plugged = via_access::resolve_front_plugging_mode(ctx, via) == PluggingMode::Plugged;
        op.top_tented = via_access::resolve_front_tenting_mode(ctx, via) == TentingMode::Tented;
    }

    if op.bottom_layer == B_CU {
        op.bottom_covered =
            via_access::resolve_back_covering_mode(ctx, via) == CoveringMode::Covered;
        op.bottom_plugged =
            via_access::resolve_back_plugging_mode(ctx, via) == PluggingMode::Plugged;
        op.bottom_tented = via_access::resolve_back_tenting_mode(ctx, via) == TentingMode::Tented;
    }

    op.front_post_machining = from_padstack(via.padstack.front_post_machining());
    op.back_post_machining = from_padstack(via.padstack.back_post_machining());
    op.drill_start = via.padstack.drill().start;
    op.drill_end = bottom_layer;

    out.push(op);
}

/// Port of the anonymous-namespace `enumeratePad`.
///
/// `PAD::Padstack()` is the stored (library-frame) padstack, `pad.lib_padstack()` here; the
/// enumerator reads only its backdrill and post-machining properties from it, exactly as C++.
fn enumerate_pad(
    ctx: &BoardCtx<'_>,
    pad_id: PadId,
    pad: &Pad,
    query: &DrillQuery,
    out: &mut Vec<DrillOperation>,
) {
    let pad_uuid = pad.base.item.uuid;

    if query.span.is_backdrill {
        if !query.pad_machining {
            return;
        }

        let mut add_backdrill = |drill: &DrillProps, kind: DrillOpKind| {
            let Some(mut op) =
                backdrill_operation(drill, kind, &query.span, DrillSource::Pad(pad_id), pad_uuid)
            else {
                return;
            };

            op.attribute = HoleAttribute::HolePadBackdrill;
            op.position = pad.position(ctx);
            op.orientation = pad.orientation(ctx);
            out.push(op);
        };

        add_backdrill(pad.lib_padstack().secondary_drill(), DrillOpKind::SecondaryDrill);
        add_backdrill(pad.lib_padstack().tertiary_drill(), DrillOpKind::TertiaryDrill);
        return;
    }

    if !query.merge_pth_npth {
        if !query.non_plated_only && pad.attribute() == PadAttrib::Npth {
            return;
        }

        if query.non_plated_only && pad.attribute() != PadAttrib::Npth {
            return;
        }
    }

    if pad.drill_size(ctx).x == 0 {
        return;
    }

    let mut op = DrillOperation {
        source_item: Some(DrillSource::Pad(pad_id)),
        source_id: pad_uuid,
        not_plated: pad.attribute() == PadAttrib::Npth,
        ..DrillOperation::default()
    };

    if op.not_plated {
        op.attribute = HoleAttribute::HoleMechanical;
    } else if pad.property() == PadProp::Castellated {
        op.attribute = HoleAttribute::HolePadCastellated;
    } else if pad.property() == PadProp::Pressfit {
        op.attribute = HoleAttribute::HolePadPressfit;
    } else {
        op.attribute = HoleAttribute::HolePad;
    }

    op.orientation = pad.orientation(ctx);
    op.diameter = pad.drill_size(ctx).x.min(pad.drill_size(ctx).y);

    op.is_slot = is_drill_slot(ctx, pad);

    op.size_xy = pad.drill_size(ctx);
    op.position = pad.position(ctx);
    op.bottom_layer = B_CU;
    op.top_layer = F_CU;

    if query.pad_machining {
        op.front_post_machining = from_padstack(pad.lib_padstack().front_post_machining());
        op.back_post_machining = from_padstack(pad.lib_padstack().back_post_machining());
    }

    out.push(op);
}

/// Port of `IsDrillSlot( const PAD& )`: the pad has a hole whose shape is not a circle and
/// whose X and Y sizes differ.
///
/// `PAD::GetDrillShape()` is `m_padStack.Drill().shape`, read from the stored padstack.
pub fn is_drill_slot(ctx: &BoardCtx<'_>, pad: &Pad) -> bool {
    pad.has_hole(ctx)
        && pad.lib_padstack().drill_shape() != PadDrillShape::Circle
        && pad.drill_size(ctx).x != pad.drill_size(ctx).y
}

/// Port of `EnumerateDrillOperations( aBoard, aQuery )`: vias in `Tracks()` order (none when
/// `non_plated_only`), then, for the non-backdrill through span (or backdrill spans with pad
/// machining), pads in `Footprints()` x `Pads()` order.
pub fn enumerate_drill_operations(ctx: &BoardCtx<'_>, query: &DrillQuery) -> Vec<DrillOperation> {
    let mut operations = Vec::new();

    // wxASSERT
    debug_assert!(
        is_copper_layer_lower_than(query.span.bottom_layer(), query.span.top_layer())
            || query.span.bottom_layer() == query.span.top_layer()
    );

    if !query.non_plated_only {
        for (id, track) in ctx.board.tracks() {
            let Track::Via(via) = track else {
                continue;
            };

            enumerate_via(ctx, id, via, query, &mut operations);
        }
    }

    if (query.span.is_backdrill && query.pad_machining)
        || (!query.span.is_backdrill && query.span.pair() == (F_CU, B_CU))
    {
        for (fp_id, _footprint) in ctx.board.footprints() {
            for (pad_id, pad) in ctx.board.footprint_pads(fp_id) {
                enumerate_pad(ctx, pad_id, pad, query, &mut operations);
            }
        }
    }

    operations
}

/// Port of `EnumerateDrillSpans( aBoard, aPadBackdrills = false )`: the through span first,
/// then the blind / buried via spans and the backdrill spans in `DRILL_SPAN::operator<` order
/// (`std::set` -> `BTreeSet`).
pub fn enumerate_drill_spans(ctx: &BoardCtx<'_>, pad_backdrills: bool) -> Vec<DrillSpan> {
    let mut unique: BTreeSet<DrillSpan> = BTreeSet::new();

    let add_backdrill = |unique: &mut BTreeSet<DrillSpan>, drill: &DrillProps| {
        if is_backdrill_set(drill) {
            unique.insert(DrillSpan::new(drill.start, drill.end, true, false));
        }
    };

    for (_id, track) in ctx.board.tracks() {
        let Track::Via(via) = track else {
            continue;
        };

        let (top, bottom) = via_access::layer_pair(ctx, via);

        if (top, bottom) != (F_CU, B_CU) {
            unique.insert(DrillSpan::new(top, bottom, false, false));
        }

        add_backdrill(&mut unique, via.padstack.secondary_drill());
        add_backdrill(&mut unique, via.padstack.tertiary_drill());
    }

    if pad_backdrills {
        for (fp_id, _footprint) in ctx.board.footprints() {
            for (_pad_id, pad) in ctx.board.footprint_pads(fp_id) {
                add_backdrill(&mut unique, pad.lib_padstack().secondary_drill());
                add_backdrill(&mut unique, pad.lib_padstack().tertiary_drill());
            }
        }
    }

    let mut spans = vec![DrillSpan::new(F_CU, B_CU, false, false)];

    for span in &unique {
        if span.is_backdrill || span.pair() != (F_CU, B_CU) as DrillLayerPair {
            spans.push(*span);
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drill(start: PcbLayerId, end: PcbLayerId, x: i32, y: i32) -> DrillProps {
        DrillProps {
            size: Vector2I::new(x, y),
            shape: PadDrillShape::Circle,
            start,
            end,
            ..DrillProps::default()
        }
    }

    #[test]
    fn backdrill_set_needs_both_layers_and_a_size() {
        assert!(is_backdrill_set(&drill(B_CU, PcbLayerId::In2Cu, 300_000, 0)));
        assert!(is_backdrill_set(&drill(B_CU, PcbLayerId::In2Cu, 0, 300_000)));
        assert!(!is_backdrill_set(&drill(B_CU, PcbLayerId::In2Cu, 0, 0)));
        assert!(!is_backdrill_set(&drill(UNDEFINED_LAYER, PcbLayerId::In2Cu, 1, 1)));
        assert!(!is_backdrill_set(&drill(B_CU, UNDEFINED_LAYER, 1, 1)));
    }

    #[test]
    fn backdrill_operation_diameter_and_slot_rules() {
        let span = DrillSpan::new(B_CU, PcbLayerId::In2Cu, true, false);
        let mut board = kicad_pcb::Board::default();
        let via = Track::Via(Box::new(PcbVia::new(&mut kicad_core::KiidGenerator::default())));
        let src = DrillSource::Via(board.add_track(via, kicad_pcb::AddMode::Append).unwrap());
        let uuid = kicad_core::Kiid::default();

        // Stored start / end must equal the span's (a reversed span does not match).
        let d = drill(PcbLayerId::In2Cu, B_CU, 300_000, 300_000);
        assert!(backdrill_operation(&d, DrillOpKind::SecondaryDrill, &span, src, uuid).is_none());

        let d = drill(B_CU, PcbLayerId::In2Cu, 400_000, 0);
        let op = backdrill_operation(&d, DrillOpKind::TertiaryDrill, &span, src, uuid).unwrap();
        assert_eq!(op.diameter, 400_000);
        assert!(op.not_plated && op.is_backdrill());
        assert_eq!((op.top_layer, op.bottom_layer), (PcbLayerId::In2Cu, B_CU));
        assert_eq!((op.drill_start, op.drill_end), (B_CU, PcbLayerId::In2Cu));
        // Circle shape with x != y is still not a slot.
        assert!(!op.is_slot);

        let mut d = drill(B_CU, PcbLayerId::In2Cu, 0, 250_000);
        let op = backdrill_operation(&d, DrillOpKind::SecondaryDrill, &span, src, uuid).unwrap();
        assert_eq!(op.diameter, 250_000);
        d.size = Vector2I::new(500_000, 250_000);
        d.shape = PadDrillShape::Oblong;
        let op = backdrill_operation(&d, DrillOpKind::SecondaryDrill, &span, src, uuid).unwrap();
        assert_eq!(op.diameter, 250_000);
        assert!(op.is_slot);
        assert_eq!(op.size_xy, Vector2I::new(500_000, 250_000));
    }

    #[test]
    fn post_machining_from_padstack() {
        let pm = from_padstack(&PostMachiningProps {
            mode: None,
            size: 1,
            depth: 2,
            angle: 900,
        });
        assert!(!pm.is_set());
        assert_eq!((pm.size, pm.depth, pm.angle), (1, 2, 900));
        let pm = from_padstack(&PostMachiningProps {
            mode: Some(PadDrillPostMachiningMode::Countersink),
            size: 600_000,
            depth: 150_000,
            angle: 900,
        });
        assert!(pm.is_set());
        assert_eq!(pm.mode, PadDrillPostMachiningMode::Countersink);
    }
}
