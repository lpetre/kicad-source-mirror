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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of `BuildCreepageBoardEdges()` (`pcbnew/drc/drc_creepage_utils.cpp`): the board-edge
//! items of the creepage graph, and the lookup of the `PCB_SHAPE` behind a board-edge item
//! (the C++ `static_cast<PCB_SHAPE*>` of an `m_boardEdge` entry).

use std::collections::BTreeSet;

use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::{Box2I, Shape, Vector2I};
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{AnyItem, BoardCtx, ItemId, MutEnv};

use super::shapes::CreepItem;

/// The `PCB_SHAPE` part of a board item, `dynamic_cast<PCB_SHAPE*>`: a graphic shape, or the
/// shape base of a text box / table cell.
pub fn item_as_pcb_shape(item: AnyItem<'_>) -> Option<&PcbShape> {
    match item {
        AnyItem::Shape(x) => Some(x),
        AnyItem::Textbox(x) => Some(&x.shape),
        AnyItem::Tablecell(x) => Some(&x.textbox.shape),
        _ => None,
    }
}

/// The `PCB_SHAPE` a board-edge entry refers to (`static_cast<PCB_SHAPE*>( aItem )`), `None`
/// for a stale handle or an item that is not a shape.
pub fn board_edge_shape<'a>(
    ctx: &BoardCtx<'a>,
    owned: &'a [PcbShape],
    item: CreepItem,
) -> Option<&'a PcbShape> {
    match item {
        CreepItem::Item(id) => item_as_pcb_shape(ctx.board.get(id)?),
        CreepItem::OwnedEdge(index) => owned.get(index),
    }
}

/// `PCB_SHAPE::GetBoundingBox()` (virtual) of a board-edge entry.
pub fn board_edge_bounding_box<'a>(
    ctx: &'a BoardCtx<'a>,
    owned: &[PcbShape],
    item: CreepItem,
) -> Option<Box2I> {
    match item {
        CreepItem::Item(id) => Some(ctx.view(ctx.board.get(id)?).bounding_box()),
        CreepItem::OwnedEdge(index) => Some(owned.get(index)?.bounding_box(ctx)),
    }
}

/// A transient `new PCB_SHAPE( nullptr, aShapeType )`.
fn owned_shape(shape: ShapeT) -> PcbShape {
    PcbShape::with_shape(shape)
}

/// Port of `BuildCreepageBoardEdges()`: collect the board-edge items used by the creepage graph.
///
/// Gathers Edge.Cuts drawings (flattening Bezier curves to owned segments) and NPTH pad holes
/// (circles and oblong slots) into `a_vector`, with transient owned shapes parked in `a_owned`
/// (referred to as [`CreepItem::OwnedEdge`] by their index there). When `a_exclude` is given,
/// board items owned by any excluded item (for example the pads of a footprint being dragged)
/// are skipped so the caller can treat them as moving geometry.
pub fn build_creepage_board_edges<'a>(
    ctx: &'a BoardCtx<'a>,
    a_vector: &mut Vec<CreepItem>,
    a_owned: &mut Vec<PcbShape>,
    a_exclude: Option<&BTreeSet<ItemId>>,
) {
    let board = ctx.board;
    let error_max = board.design_settings().max_error();
    let env = MutEnv::detached();

    let excluded = |id: ItemId, item: AnyItem<'_>| -> bool {
        let Some(exclude) = a_exclude else {
            return false;
        };

        if exclude.contains(&id) {
            return true;
        }

        item.base()
            .parent
            .item()
            .is_some_and(|parent| exclude.contains(&parent))
    };

    // The creepage graph only handles SEGMENT/ARC/CIRCLE/RECTANGLE/POLY, so Bezier curves must
    // be flattened to segments or they are silently ignored and creepage paths pass through them
    let mut add_edge_drawing = |id: ItemId, drawing: AnyItem<'a>| {
        if !ctx.view(drawing).is_on_layer(PcbLayerId::EdgeCuts) {
            return;
        }

        if excluded(id, drawing) {
            return;
        }

        // Downstream code static_casts every item in m_boardEdge to PCB_SHAPE, so non-shape
        // items (text, dimensions, ...) on Edge.Cuts must not enter the graph
        let Some(shape) = item_as_pcb_shape(drawing) else {
            return;
        };

        if shape.shape.shape() != ShapeT::Bezier {
            a_vector.push(CreepItem::Item(id));
            return;
        }

        // RebuildBezierToSegmentsPointsList( errorMax ) + GetBezierPoints(), without writing
        // the cache of the board's item
        let pts = shape.shape.build_bezier_to_segments_points_list(error_max);

        for i in 1..pts.len() {
            if pts[i - 1] == pts[i] {
                continue;
            }

            let mut seg = owned_shape(ShapeT::Segment);
            seg.set_start(&env, pts[i - 1]);
            seg.set_end(&env, pts[i]);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(seg);
        }
    };

    for (id, drawing) in board.drawings() {
        add_edge_drawing(id, drawing);
    }

    for (fp, _) in board.footprints() {
        for (id, drawing) in board.footprint_drawings(fp) {
            add_edge_drawing(id, drawing);
        }
    }

    for pad_id in board.get_pads() {
        let id = ItemId::from(pad_id);

        let Some(item) = board.get(id) else {
            continue;
        };

        let AnyItem::Pad(p) = item else {
            continue;
        };

        if p.attribute() != PadAttrib::Npth {
            continue;
        }

        if excluded(id, item) {
            continue;
        }

        // TODO: handle backdrilling and post-machining

        let Some(hole) =
            p.effective_hole_shape(ctx, PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint)
        else {
            continue;
        };

        let pt_a = hole.get_seg().a;
        let pt_b = hole.get_seg().b;
        let radius = hole.get_width() / 2;

        if pt_a == pt_b {
            let mut s = owned_shape(ShapeT::Circle);
            // EDA_SHAPE::SetRadius: SetEnd( m_start + ( aX, 0 ) ) through the virtual SetEnd
            let end = s.shape.start() + Vector2I::new(radius, 0);
            s.set_end(&env, end);
            s.set_position(&env, pt_a);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(s);
        } else {
            // Oblong slot outline as two straight sides and two semicircular end caps
            let axis = pt_b - pt_a;
            let perp = axis.perpendicular().resize(radius);

            let mut seg1 = owned_shape(ShapeT::Segment);
            seg1.set_start(&env, pt_a + perp);
            seg1.set_end(&env, pt_b + perp);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(seg1);

            let mut seg2 = owned_shape(ShapeT::Segment);
            seg2.set_start(&env, pt_a - perp);
            seg2.set_end(&env, pt_b - perp);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(seg2);

            let mid_a = pt_a - axis.resize(radius);
            let mut arc_a = owned_shape(ShapeT::Arc);
            arc_a.set_arc_geometry(&env, pt_a + perp, mid_a, pt_a - perp);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(arc_a);

            let mid_b = pt_b + axis.resize(radius);
            let mut arc_b = owned_shape(ShapeT::Arc);
            arc_b.set_arc_geometry(&env, pt_b - perp, mid_b, pt_b + perp);
            a_vector.push(CreepItem::OwnedEdge(a_owned.len()));
            a_owned.push(arc_b);
        }
    }
}
