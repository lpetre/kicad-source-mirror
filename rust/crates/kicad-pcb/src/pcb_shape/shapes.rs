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

//! Port of the shape part of `pcbnew/pcb_shape.cpp`. Package M9.
//!
//! Created empty by M0a so that the module tree is complete; the named package fills it.
//!
//! Package M9: the friendly name, `getHatchingKnockouts`, `UpdateHatching`,
//! `GetFocusPosition`, `GetConnectionPoints`, `cmp_drawings`, `operator==` and `Similarity`.

use std::cmp::Ordering;

use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::circle::Circle;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::geometry::shape_utils::get_circle_key_points;
use kicad_geom::{ShapePolySet, Vector2I, ki_round};

use crate::ctx::BoardCtx;
use crate::eda_shape::{EdaShape, ShapeT, hatching_data};
use crate::ids::ItemId;
use crate::item_ref::{AnyItem, ItemRef};
use crate::pcb_shape::PcbShape;
use crate::item::RecurseMode;
use crate::traits::EdaShapeItem;
use crate::board_item;

/// Port of `EDA_SHAPE::getFriendlyName( FRAME_PCB_EDITOR )` (English; `_()` untranslated).
/// `is_proxy` is the virtual `IsProxyItem()` of the host.
pub fn shape_friendly_name(shape: &EdaShape, is_proxy: bool) -> String {
    if is_proxy {
        match shape.shape {
            ShapeT::Rectangle => "Pad Number Box",
            ShapeT::Segment => "Thermal Spoke Template",
            _ => "Unrecognized",
        }
        .to_string()
    } else {
        match shape.shape {
            ShapeT::Circle => "Circle",
            ShapeT::Arc => "Arc",
            ShapeT::Bezier => "Curve",
            ShapeT::Poly => "Polygon",
            ShapeT::Rectangle => "Rectangle",
            ShapeT::Segment => "Segment",
            ShapeT::Ellipse => "Ellipse",
            ShapeT::EllipseArc => "Elliptical Arc",
            ShapeT::Undefined => "Unrecognized",
        }
        .to_string()
    }
}

/// Port of `PCB_SHAPE::getHatchingKnockouts()`: the board items whose shapes knock out a
/// hatched fill on the shape's layer (board drawings: fields, texts, text boxes and shapes;
/// footprint fields and shapes of other footprints; courtyards on a courtyard layer).
pub fn hatching_knockouts(shape: &PcbShape, ctx: &BoardCtx<'_>) -> ShapePolySet {
    let mut knockouts = ShapePolySet::new();

    if !board_item::has_board(shape, ctx) {
        return knockouts;
    }

    let layer = shape.base.item.layer;
    let bbox = shape.bounding_box(ctx);
    let max_error = ARC_LOW_DEF;
    let this_id = shape.base.item.id();

    let knockout_item = |item: ItemRef<'_>, knockouts: &mut ShapePolySet| {
        let mut margin = shape.hatch_line_spacing(ctx) / 2;

        if item.type_() == KicadT::PcbTextboxT {
            margin = 0;
        }

        item.transform_shape_to_polygon(knockouts, layer, margin, max_error, ErrorLoc::Outside, false);
    };

    for (id, _) in ctx.board.drawings() {
        if Some(id) == this_id {
            continue;
        }

        let Some(item) = ctx.item(id) else {
            continue;
        };

        if matches!(
            item.type_(),
            KicadT::PcbFieldT | KicadT::PcbTextT | KicadT::PcbTextboxT | KicadT::PcbShapeT
        ) && item.layer() == layer
            && item.bounding_box().intersects(&bbox)
        {
            knockout_item(item, &mut knockouts);
        }
    }

    let parent_fp = board_item::parent_footprint(shape, ctx).and_then(|fp| fp.base.id());

    for (fp_id, footprint) in ctx.board.footprints() {
        if Some(ItemId::from(fp_id)) == parent_fp {
            continue;
        }

        // GetCourtyard() returns the front courtyard for any non-back layer, so only knock it
        // out when the hatched shape actually lives on a courtyard layer.
        if layer == PcbLayerId::FCrtYd || layer == PcbLayerId::BCrtYd {
            knockouts.append_set(&footprint.courtyard(ctx, layer));
        }

        // Knockout footprint fields
        footprint.run_on_children(ctx, RecurseMode::Recurse, &mut |item: ItemRef<'_>| {
            let kind = item.type_();
            let invisible_field = kind == KicadT::PcbFieldT
                && !item
                    .as_eda_text()
                    .is_some_and(|text| text.eda_text().is_visible());

            if (kind == KicadT::PcbFieldT || kind == KicadT::PcbShapeT)
                && item.layer() == layer
                && !invisible_field
                && item.bounding_box().intersects(&bbox)
            {
                knockout_item(item, &mut knockouts);
            }
        });
    }

    knockouts
}

impl PcbShape {
    /// Port of `PCB_SHAPE::UpdateHatching()`: force an update (the damage from all the things
    /// that might knock out parts of the hatching is not propagated), then
    /// `EDA_SHAPE::UpdateHatching()`.
    pub fn update_hatching(&self, ctx: &BoardCtx<'_>) {
        self.shape.set_hatching_dirty();
        let _ = hatching_data(self, ctx);
    }

    /// Port of `PCB_SHAPE::GetFocusPosition()`: the visual centre for some shapes, but a point
    /// on the outline for unfilled polygonal shapes (whose centre is usually far from the
    /// shape).
    pub fn focus_position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        let es = &self.shape;

        match es.shape {
            ShapeT::Circle => {
                if !es.is_any_fill() {
                    let center = self.center(ctx);
                    Vector2I::new(center.x + es.radius(), center.y)
                } else {
                    self.center(ctx)
                }
            }
            ShapeT::Rectangle => {
                if !es.is_any_fill() {
                    es.start()
                } else {
                    self.center(ctx)
                }
            }
            ShapeT::Poly => {
                if !es.is_any_fill() {
                    es.poly_shape().c_outline(0).c_point(0)
                } else {
                    self.center(ctx)
                }
            }
            ShapeT::Arc => es.arc_mid(),
            ShapeT::Bezier => es.start(),
            _ => self.center(ctx),
        }
    }

    /// Port of `PCB_SHAPE::GetConnectionPoints()`: points where this shape can form
    /// electrical connections to other shapes that are natural "start/end" points (may be
    /// empty).
    pub fn connection_points(&self, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        let es = &self.shape;
        let mut ret = Vec::new();

        // For filled shapes, we may as well use a centroid
        if es.is_solid_fill() {
            ret.push(self.center(ctx));
            return ret;
        }

        match es.shape {
            ShapeT::Circle => {
                let circle = Circle::new(self.center(ctx), es.radius());

                for pt in get_circle_key_points(&circle, false) {
                    ret.push(pt.point);
                }
            }
            ShapeT::Arc => {
                ret.push(es.arc_mid());
                ret.push(es.start());
                ret.push(es.end());
            }
            ShapeT::Segment | ShapeT::Bezier => {
                ret.push(es.start());
                ret.push(es.end());
            }
            ShapeT::Poly => ret.extend(es.poly_shape().c_iterate()),
            ShapeT::Rectangle => ret.extend(es.rect_corners()),
            ShapeT::Ellipse => {
                let e = &es.ellipse;
                let phi = e.rotation.as_radians();
                let cos_phi = kicad_geom::cmath::cos(phi);
                let sin_phi = kicad_geom::cmath::sin(phi);
                let a = e.major_radius;
                let b = e.minor_radius;
                let c = e.center;

                ret.push(c + Vector2I::new(ki_round(a as f64 * cos_phi), ki_round(a as f64 * sin_phi)));
                ret.push(
                    c + Vector2I::new(ki_round(-a as f64 * cos_phi), ki_round(-a as f64 * sin_phi)),
                );
                ret.push(
                    c + Vector2I::new(ki_round(-b as f64 * sin_phi), ki_round(b as f64 * cos_phi)),
                );
                ret.push(
                    c + Vector2I::new(ki_round(b as f64 * sin_phi), ki_round(-b as f64 * cos_phi)),
                );
            }
            ShapeT::EllipseArc => {
                let e = &es.ellipse;
                let a = e.major_radius as f64;
                let b = e.minor_radius as f64;
                let phi = e.rotation.as_radians();
                let cos_phi = kicad_geom::cmath::cos(phi);
                let sin_phi = kicad_geom::cmath::sin(phi);
                let c = e.center;

                let eval = |theta: f64| {
                    let lx = a * kicad_geom::cmath::cos(theta);
                    let ly = b * kicad_geom::cmath::sin(theta);
                    c + Vector2I::new(
                        ki_round(lx * cos_phi - ly * sin_phi),
                        ki_round(lx * sin_phi + ly * cos_phi),
                    )
                };

                let theta_start = e.start_angle.as_radians();
                let mut theta_end = e.end_angle.as_radians();

                if theta_end < theta_start {
                    theta_end += 2.0 * std::f64::consts::PI;
                }

                ret.push(eval(theta_start));
                ret.push(eval(theta_end));
                ret.push(eval(0.5 * (theta_start + theta_end)));
            }
            // UNIMPLEMENTED_FOR( SHAPE_T_asString() )
            ShapeT::Undefined => {}
        }

        ret
    }

    /// Port of `PCB_SHAPE::operator==( const PCB_SHAPE& )` (also `operator==( const
    /// BOARD_ITEM& )` once the type matched): layer, knockout, lock, flags, forced
    /// visibility (`m_forceVisible` is not in the model: never set by
    /// non-UI code), net, solder mask, mask margin, then `EDA_SHAPE::operator==`.
    pub fn eq_item(&self, other: &PcbShape) -> bool {
        let (a, b) = (&self.base.item, &other.base.item);

        a.layer == b.layer
            && a.is_knockout == b.is_knockout
            && a.is_locked == b.is_locked
            && a.flags == b.flags
            && self.base.net == other.base.net
            && self.has_solder_mask == other.has_solder_mask
            && self.solder_mask_margin == other.solder_mask_margin
            && self.shape == other.shape
    }

    /// Port of `PCB_SHAPE::Similarity( const BOARD_ITEM& )` for another `PCB_SHAPE` (other
    /// types: 0.0).
    pub fn similarity(&self, other: &PcbShape) -> f64 {
        let (a, b) = (&self.base.item, &other.base.item);
        let mut similarity = 1.0;

        if a.layer != b.layer {
            similarity *= 0.9;
        }

        if a.is_knockout != b.is_knockout {
            similarity *= 0.9;
        }

        if a.is_locked != b.is_locked {
            similarity *= 0.9;
        }

        if a.flags != b.flags {
            similarity *= 0.9;
        }

        if self.base.net != other.base.net {
            similarity *= 0.9;
        }

        if self.has_solder_mask != other.has_solder_mask {
            similarity *= 0.9;
        }

        if self.solder_mask_margin != other.solder_mask_margin {
            similarity *= 0.9;
        }

        similarity *= self.shape.similarity(&other.shape);

        similarity
    }
}

/// Port of `PCB_SHAPE::cmp_drawings::operator()` as an `Ordering`: `Type()`, `GetLayer()`,
/// for two shapes `GetShape()`, then `m_Uuid`; equal UUIDs fall back to `ItemId` order (the
/// pointer order of `board-model.md` 3.1). A stale handle sorts by `ItemId` only. (Unused by
/// the C++ writer, which sorts with `BOARD::cmp_drawings` / `FOOTPRINT::cmp_drawings`.)
pub fn pcb_shape_cmp_drawings(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering {
    let (Some(first), Some(second)) = (ctx.item(a), ctx.item(b)) else {
        return a.cmp(&b);
    };

    if first.type_() != second.type_() {
        return (first.type_() as i32).cmp(&(second.type_() as i32));
    }

    if first.layer() != second.layer() {
        return first.layer().0.cmp(&second.layer().0);
    }

    if first.type_() == KicadT::PcbShapeT {
        if let (AnyItem::Shape(dwg_a), AnyItem::Shape(dwg_b)) = (first.any(), second.any()) {
            if dwg_a.shape.shape != dwg_b.shape.shape {
                return (dwg_a.shape.shape as i32).cmp(&(dwg_b.shape.shape as i32));
            }
        }
    }

    if first.uuid() != second.uuid() {
        return first.uuid().cmp(&second.uuid());
    }

    a.cmp(&b)
}
