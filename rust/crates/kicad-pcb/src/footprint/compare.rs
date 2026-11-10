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

//! Port of the comparison part of `pcbnew/footprint.cpp`: `FOOTPRINT::cmp_drawings`,
//! `cmp_pads`, `cmp_zones` (save-order comparators, `board-model-types.md` 18) and
//! `FOOTPRINT::operator==` (package M20).

use std::cmp::Ordering;

use kicad_core::Lset;
use kicad_core::string_utils::{CmpT, str_num_cmp};
use kicad_geom::Vector2I;

use crate::ctx::BoardCtx;
use crate::eda_shape::ShapeT;
use crate::footprint::Footprint;
use crate::ids::ItemId;
use crate::item_ref::{AnyItem, ItemRef};
use crate::padstack::PadstackMode;
use crate::traits::EdaTextItem;

/// `cmp_points_opt( aPtA, aPtB )`: the order of two points by x then y, `None` when equal.
fn cmp_points_opt(a: Vector2I, b: Vector2I) -> Option<Ordering> {
    if a.x != b.x {
        return Some(a.x.cmp(&b.x));
    }

    if a.y != b.y {
        return Some(a.y.cmp(&b.y));
    }

    None
}

/// `aFirst.Seq() < aSecond.Seq()`: lexicographic order of the layer sequences.
fn cmp_layer_seq(a: &Lset, b: &Lset) -> Ordering {
    a.seq()
        .0
        .iter()
        .map(|l| l.0)
        .cmp(b.seq().0.iter().map(|l| l.0))
}

/// The common tail of the comparators: `m_Uuid`, then pointer order, which is `ItemId`
/// order (`board-model.md` 3.1).
fn cmp_uuid_then_handle(
    a_ref: &ItemRef<'_>,
    b_ref: &ItemRef<'_>,
    a: ItemId,
    b: ItemId,
) -> Ordering {
    if a_ref.uuid() != b_ref.uuid() {
        return a_ref.uuid().cmp(&b_ref.uuid());
    }

    a.cmp(&b)
}

/// `FOOTPRINT::cmp_pads` as an ordering (`Less` where the C++ functor returns true): pad
/// number (`StrNumCmp`), footprint-relative position, the copper size / shape of every unique
/// layer of the more complex padstack (the last differing layer wins, as in C++), the layer
/// sequence, `m_Uuid`, then `ItemId` order. A stale handle sorts by `ItemId` only.
pub fn footprint_cmp_pads(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(a_ref), Some(b_ref)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };
    let (Some(first), Some(second)) = (a_ref.as_pad(), b_ref.as_pad()) else {
        return a.cmp(&b);
    };

    if first.number() != second.number() {
        return str_num_cmp(first.number(), second.number(), CmpT::CaseSensitive).cmp(&0);
    }

    if let Some(cmp) = cmp_points_opt(
        first.fp_relative_position(ctx),
        second.fp_relative_position(ctx),
    ) {
        return cmp;
    }

    let mut pad_copper_matches: Option<Ordering> = None;

    // Pick the "most complex" padstack to iterate
    let mut check_pad = first;

    if second.padstack.mode() == PadstackMode::Custom
        || (second.padstack.mode() == PadstackMode::FrontInnerBack
            && first.padstack.mode() == PadstackMode::Normal)
    {
        check_pad = second;
    }

    check_pad.padstack.for_each_unique_layer(|layer| {
        let (size_a, size_b) = (first.size(ctx, layer), second.size(ctx, layer));

        if size_a.x != size_b.x {
            pad_copper_matches = Some(size_a.x.cmp(&size_b.x));
        } else if size_a.y != size_b.y {
            pad_copper_matches = Some(size_a.y.cmp(&size_b.y));
        } else if first.shape(layer) != second.shape(layer) {
            pad_copper_matches =
                Some((first.shape(layer) as i32).cmp(&(second.shape(layer) as i32)));
        }
    });

    if let Some(cmp) = pad_copper_matches {
        return cmp;
    }

    let (layers_a, layers_b) = (first.layer_set(ctx), second.layer_set(ctx));

    if layers_a != layers_b {
        return cmp_layer_seq(&layers_a, &layers_b);
    }

    cmp_uuid_then_handle(&a_ref, &b_ref, a, b)
}

/// `FOOTPRINT::cmp_drawings` as an ordering: `Type()`, layer, then for shapes the shape type
/// and library geometry (the board-frame ellipse members, as in C++) and the width, for texts
/// the footprint-relative position, angle, size, thickness, style, line spacing and text;
/// then `m_Uuid` and `ItemId` order. A stale handle sorts by `ItemId` only.
pub fn footprint_cmp_drawings(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(item_a), Some(item_b)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };

    if item_a.type_() != item_b.type_() {
        return (item_a.type_() as i32).cmp(&(item_b.type_() as i32));
    }

    if item_a.layer() != item_b.layer() {
        return item_a.layer().0.cmp(&item_b.layer().0);
    }

    match (item_a.any(), item_b.any()) {
        (AnyItem::Shape(dwg_a), AnyItem::Shape(dwg_b)) => {
            let (shape_a, shape_b) = (dwg_a.shape.shape(), dwg_b.shape.shape());

            if shape_a != shape_b {
                return (shape_a as i32).cmp(&(shape_b as i32));
            }

            if shape_a != ShapeT::Poly {
                if let Some(cmp) = cmp_points_opt(dwg_a.lib_start(), dwg_b.lib_start()) {
                    return cmp;
                }

                if let Some(cmp) = cmp_points_opt(dwg_a.lib_end(), dwg_b.lib_end()) {
                    return cmp;
                }
            }

            if shape_a == ShapeT::Arc {
                if let Some(cmp) =
                    cmp_points_opt(dwg_a.library_arc_mid(ctx), dwg_b.library_arc_mid(ctx))
                {
                    return cmp;
                }
            } else if shape_a == ShapeT::Bezier {
                if let Some(cmp) =
                    cmp_points_opt(dwg_a.library_bezier_c1(ctx), dwg_b.library_bezier_c1(ctx))
                {
                    return cmp;
                }

                if let Some(cmp) =
                    cmp_points_opt(dwg_a.library_bezier_c2(ctx), dwg_b.library_bezier_c2(ctx))
                {
                    return cmp;
                }
            } else if shape_a == ShapeT::Poly {
                let a_lib = dwg_a.library_poly_shape(ctx);
                let b_lib = dwg_b.library_poly_shape(ctx);

                if a_lib.total_vertices() != b_lib.total_vertices() {
                    return a_lib.total_vertices().cmp(&b_lib.total_vertices());
                }

                for ii in 0..a_lib.total_vertices() {
                    if let Some(cmp) =
                        cmp_points_opt(a_lib.c_vertex_global(ii), b_lib.c_vertex_global(ii))
                    {
                        return cmp;
                    }
                }
            } else if shape_a == ShapeT::Ellipse || shape_a == ShapeT::EllipseArc {
                let (ea, eb) = (dwg_a.shape.ellipse(), dwg_b.shape.ellipse());

                if let Some(cmp) = cmp_points_opt(ea.center, eb.center) {
                    return cmp;
                }

                if ea.major_radius != eb.major_radius {
                    return ea.major_radius.cmp(&eb.major_radius);
                }

                if ea.minor_radius != eb.minor_radius {
                    return ea.minor_radius.cmp(&eb.minor_radius);
                }

                let tenths = |angle: kicad_geom::EdaAngle| angle.as_tenths_of_a_degree();

                if tenths(ea.rotation) != tenths(eb.rotation) {
                    return tenths(ea.rotation).cmp(&tenths(eb.rotation));
                }

                if shape_a == ShapeT::EllipseArc {
                    if tenths(ea.start_angle) != tenths(eb.start_angle) {
                        return tenths(ea.start_angle).cmp(&tenths(eb.start_angle));
                    }

                    if tenths(ea.end_angle) != tenths(eb.end_angle) {
                        return tenths(ea.end_angle).cmp(&tenths(eb.end_angle));
                    }
                }
            }

            let (width_a, width_b) = (dwg_a.width(ctx), dwg_b.width(ctx));

            if width_a != width_b {
                return width_a.cmp(&width_b);
            }
        }
        (AnyItem::Text(text_a), AnyItem::Text(text_b)) => {
            if let Some(cmp) =
                cmp_points_opt(item_a.fp_relative_position(), item_b.fp_relative_position())
            {
                return cmp;
            }

            let (angle_a, angle_b) = (text_a.text_angle(ctx), text_b.text_angle(ctx));

            if angle_a != angle_b {
                // EDA_ANGLE::operator< on the stored degrees (never NaN)
                return angle_a.partial_cmp(&angle_b).unwrap_or(Ordering::Equal);
            }

            if let Some(cmp) = cmp_points_opt(text_a.text_size(ctx), text_b.text_size(ctx)) {
                return cmp;
            }

            let (thick_a, thick_b) = (text_a.text_thickness(ctx), text_b.text_thickness(ctx));

            if thick_a != thick_b {
                return thick_a.cmp(&thick_b);
            }

            let (ta, tb) = (text_a.eda_text(), text_b.eda_text());

            if ta.is_bold() != tb.is_bold() {
                return ta.is_bold().cmp(&tb.is_bold());
            }

            if ta.is_italic() != tb.is_italic() {
                return ta.is_italic().cmp(&tb.is_italic());
            }

            if ta.is_mirrored() != tb.is_mirrored() {
                return ta.is_mirrored().cmp(&tb.is_mirrored());
            }

            if ta.line_spacing() != tb.line_spacing() {
                return ta
                    .line_spacing()
                    .partial_cmp(&tb.line_spacing())
                    .unwrap_or(Ordering::Equal);
            }

            if ta.text() != tb.text() {
                // wxString::Cmp: code point order, which UTF-8 byte order preserves
                return ta.text().cmp(tb.text());
            }
        }
        _ => {
            // These items don't have their own specific sorting criteria.
        }
    }

    cmp_uuid_then_handle(&item_a, &item_b, a, b)
}

/// `FOOTPRINT::cmp_zones` as an ordering: assigned priority, layer sequence, the library
/// outline vertices, `m_Uuid`, then `ItemId` order. A stale handle sorts by `ItemId` only.
pub fn footprint_cmp_zones(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(a_ref), Some(b_ref)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };
    let (Some(first), Some(second)) = (a_ref.as_zone(), b_ref.as_zone()) else {
        return a.cmp(&b);
    };

    if first.assigned_priority() != second.assigned_priority() {
        return first.assigned_priority().cmp(&second.assigned_priority());
    }

    let (layers_a, layers_b) = (first.layer_set(ctx), second.layer_set(ctx));

    if layers_a != layers_b {
        return cmp_layer_seq(&layers_a, &layers_b);
    }

    let a_lib = first.library_outline();
    let b_lib = second.library_outline();

    if a_lib.total_vertices() != b_lib.total_vertices() {
        return a_lib.total_vertices().cmp(&b_lib.total_vertices());
    }

    for ii in 0..a_lib.total_vertices() {
        if let Some(cmp) = cmp_points_opt(a_lib.c_vertex_global(ii), b_lib.c_vertex_global(ii)) {
            return cmp;
        }
    }

    cmp_uuid_then_handle(&a_ref, &b_ref, a, b)
}

/// `BOARD_ITEM::operator==( const BOARD_ITEM& )` of two footprint children (the virtual
/// dispatch of `*aLeft == *aRight`): false for different types, else the type's own
/// `operator==`. Types whose `operator==` has no Rust counterpart yet are `port_pending!`.
fn eq_child(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> bool {
    let (Some(left), Some(right)) = (ctx.item(a), ctx.item(b)) else {
        return false;
    };

    if left.type_() != right.type_() {
        return false;
    }

    match (left.any(), right.any()) {
        // PAD::operator==( const PAD& ) (the static type is PAD on both sides, so the overload
        // without the parent check is the one called): padstack, position, attribute.
        (AnyItem::Pad(pa), AnyItem::Pad(pb)) => {
            pa.padstack.eq_padstack(&pb.padstack)
                && pa.position(ctx) == pb.position(ctx)
                && pa.attribute() == pb.attribute()
        }
        (AnyItem::Shape(sa), AnyItem::Shape(sb)) => sa.eq_item(sb),
        (AnyItem::Zone(za), AnyItem::Zone(zb)) => za.eq_item(ctx, zb),
        (AnyItem::Group(ga), AnyItem::Group(gb)) => ga.eq_item(ctx, gb),
        (AnyItem::Point(pa), AnyItem::Point(pb)) => pa.eq_item(pb),
        _ => crate::port_pending!("operator== of a footprint child", left.type_()),
    }
}

/// `std::ranges::equal( aLeft, aRight, equalItems )` over two child order lists.
fn eq_children(ctx: &BoardCtx<'_>, a: &[ItemId], b: &[ItemId]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| eq_child(ctx, x, y))
}

/// The `equalPadGroups` lambda: delimiters are not significant, "1,2" and "1, 2" tie the
/// same pads (`wxStringTokenizer( .., ", \t\r\n", wxTOKEN_STRTOK )`).
fn equal_pad_groups(left: &str, right: &str) -> bool {
    let tokens = |s: &str| -> Vec<String> {
        s.split([',', ' ', '\t', '\r', '\n'])
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    };

    tokens(left) == tokens(right)
}

impl Footprint {
    /// `FOOTPRINT::operator==( const FOOTPRINT& )` (children compared through `ctx`).
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &Footprint) -> bool {
        let pads = |f: &Footprint| -> Vec<ItemId> { f.pads.iter().map(|&p| p.into()).collect() };
        let zones = |f: &Footprint| -> Vec<ItemId> { f.zones.iter().map(|&z| z.into()).collect() };
        let groups =
            |f: &Footprint| -> Vec<ItemId> { f.groups.iter().map(|&g| g.into()).collect() };
        let bodies =
            |f: &Footprint| -> Vec<ItemId> { f.bodies.iter().map(|&b| b.into()).collect() };
        let drawings = |f: &Footprint| -> Vec<ItemId> { f.drawings.iter().copied().collect() };

        if !eq_children(ctx, &pads(self), &pads(other)) {
            return false;
        }

        if !eq_children(ctx, &drawings(self), &drawings(other)) {
            return false;
        }

        if !eq_children(ctx, &zones(self), &zones(other)) {
            return false;
        }

        if !eq_children(ctx, &groups(self), &groups(other)) {
            return false;
        }

        if self.net_tie_pad_groups.len() != other.net_tie_pad_groups.len()
            || !self
                .net_tie_pad_groups
                .iter()
                .zip(&other.net_tie_pad_groups)
                .all(|(l, r)| equal_pad_groups(l, r))
        {
            return false;
        }

        if self.jumper_pad_groups != other.jumper_pad_groups {
            return false;
        }

        if self.points.len() != other.points.len() {
            return false;
        }

        if !eq_children(ctx, &bodies(self), &bodies(other)) {
            return false;
        }

        if self.height != other.height {
            return false;
        }

        // Compare fields in ordinally-sorted order
        let fields = self.fields_sorted(ctx, false);
        let other_fields = other.fields_sorted(ctx, false);

        if fields.len() != other_fields.len() {
            return false;
        }

        for (&f, &o) in fields.iter().zip(&other_fields) {
            if !eq_field(ctx, f.into(), o.into()) {
                return false;
            }
        }

        true
    }
}

/// `PCB_FIELD::operator==( const PCB_FIELD& )`: no Rust counterpart yet (package M10).
fn eq_field(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> bool {
    crate::port_pending!("PCB_FIELD::operator==", ctx, a, b)
}
