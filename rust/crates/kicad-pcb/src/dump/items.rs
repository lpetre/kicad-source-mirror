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

//! The per-item sections `track`, `pad`, `shape`, `text`, `dim`, `zone`, `footprint`
//! (`dump_items.cpp`). Getters the Rust model does not have yet are reported as pending with
//! the C++ name and the suffix `(no Rust API)` ([`no_api`]).

use kicad_core::text_vars::ResolutionContext;
use kicad_core::{Flashing, KicadT, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{ShapePolySet, Vector2I};

use super::board::item_copper_count;
use super::format::{
    Out, copper_layers, emit, no_api, v_angle, v_bool, v_box, v_enum, v_f64, v_int, v_layer,
    v_layer_names, v_layers, v_lset, v_opt_bool, v_opt_enum, v_opt_f64, v_opt_int, v_points,
    v_polyset, v_shape, v_str, v_vec,
};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::{EdaShapeExt, ShapeT};
use crate::eda_text::EdaTextExt;
use crate::item_ref::{AnyItem, ItemRef};
use crate::padstack::{DrillProps, MaskLayerProps, Padstack, PostMachiningProps};
use crate::pcb_dimension::DimensionKind;
use crate::pcb_track::Track;
use crate::traits::{BoardConnectedItem, EdaShapeItem};

/// One (clearance, maxError, errorLoc) combination for TransformShapeToPolygon dumps.
pub(crate) struct PolyCombo {
    pub clearance: i32,
    pub max_error: i32,
    pub error_loc: ErrorLoc,
    pub name: &'static str,
}

/// `PolyCombos()`.
pub(crate) const POLY_COMBOS: [PolyCombo; 3] = [
    PolyCombo {
        clearance: 0,
        max_error: 5000,
        error_loc: ErrorLoc::Inside,
        name: "c0_e5000_in",
    },
    PolyCombo {
        clearance: 0,
        max_error: 1000,
        error_loc: ErrorLoc::Outside,
        name: "c0_e1000_out",
    },
    PolyCombo {
        clearance: 100_000,
        max_error: 5000,
        error_loc: ErrorLoc::Outside,
        name: "c100000_e5000_out",
    },
];

/// `DumpTransformedPolys`.
pub(crate) fn dump_transformed_polys(o: &mut Out, it: ItemRef<'_>, layer: PcbLayerId) {
    for c in &POLY_COMBOS {
        o.guard("tpoly", |o| {
            emit!(o, format!("tpoly.{}.{}", v_layer(layer), c.name), {
                let mut buf = ShapePolySet::new();
                it.transform_shape_to_polygon(
                    &mut buf,
                    layer,
                    c.clearance,
                    c.max_error,
                    c.error_loc,
                    false,
                );
                v_polyset(&buf)
            });
        });
    }
}

/// `DumpEffectiveShape`.
pub(crate) fn dump_effective_shape(
    o: &mut Out,
    field: &str,
    it: ItemRef<'_>,
    layer: PcbLayerId,
    flash: Flashing,
) {
    emit!(o, field, {
        let s = it.effective_shape(layer, flash, DrcConstraintT::NullConstraint);
        v_shape(Some(s.as_ref()))
    });
}

fn dump_drill(o: &mut Out, p: &str, d: &DrillProps) {
    emit!(o, format!("{p}.size"), v_vec(d.size));
    emit!(o, format!("{p}.shape"), v_enum(d.shape));
    emit!(o, format!("{p}.start"), v_layer(d.start));
    emit!(o, format!("{p}.end"), v_layer(d.end));
    emit!(o, format!("{p}.is_filled"), v_opt_bool(d.is_filled));
    emit!(o, format!("{p}.is_capped"), v_opt_bool(d.is_capped));
}

fn dump_post_machining(o: &mut Out, p: &str, d: &PostMachiningProps) {
    emit!(o, format!("{p}.mode"), v_opt_enum(d.mode));
    emit!(o, format!("{p}.size"), v_int(d.size));
    emit!(o, format!("{p}.depth"), v_int(d.depth));
    emit!(o, format!("{p}.angle"), v_int(d.angle));
}

fn dump_mask(o: &mut Out, p: &str, m: &MaskLayerProps) {
    emit!(
        o,
        format!("{p}.solder_mask_margin"),
        v_opt_int(m.solder_mask_margin)
    );
    emit!(
        o,
        format!("{p}.solder_paste_margin"),
        v_opt_int(m.solder_paste_margin)
    );
    emit!(
        o,
        format!("{p}.solder_paste_margin_ratio"),
        v_opt_f64(m.solder_paste_margin_ratio)
    );
    emit!(
        o,
        format!("{p}.has_solder_mask"),
        v_opt_bool(m.has_solder_mask)
    );
    emit!(
        o,
        format!("{p}.has_solder_paste"),
        v_opt_bool(m.has_solder_paste)
    );
    emit!(o, format!("{p}.has_covering"), v_opt_bool(m.has_covering));
    emit!(o, format!("{p}.has_plugging"), v_opt_bool(m.has_plugging));
}

/// `DumpPadstack` (the padstack as stored).
pub(crate) fn dump_padstack(o: &mut Out, p: &str, ps: &Padstack) {
    emit!(o, format!("{p}mode"), v_enum(ps.mode()));
    emit!(o, format!("{p}layerset"), v_lset(ps.layer_set()));
    emit!(o, format!("{p}layers"), v_layer_names(ps.layer_set()));
    emit!(o, format!("{p}start_layer"), v_layer(ps.start_layer()));
    emit!(o, format!("{p}end_layer"), v_layer(ps.end_layer()));
    emit!(o, format!("{p}name"), v_str(&ps.name()));
    emit!(o, format!("{p}orientation"), v_angle(ps.orientation()));
    emit!(
        o,
        format!("{p}unconnected_layer_mode"),
        v_enum(ps.unconnected_layer_mode())
    );
    emit!(
        o,
        format!("{p}custom_shape_in_zone_mode"),
        v_enum(ps.custom_shape_in_zone_mode())
    );
    emit!(o, format!("{p}max_hole_size"), v_int(ps.max_hole_size()));
    dump_drill(o, &format!("{p}drill"), ps.drill());
    dump_drill(o, &format!("{p}secondary_drill"), ps.secondary_drill());
    dump_drill(o, &format!("{p}tertiary_drill"), ps.tertiary_drill());
    dump_post_machining(
        o,
        &format!("{p}front_post_machining"),
        ps.front_post_machining(),
    );
    dump_post_machining(
        o,
        &format!("{p}back_post_machining"),
        ps.back_post_machining(),
    );
    emit!(o, format!("{p}backdrill_mode"), v_enum(ps.backdrill_mode()));
    dump_mask(o, &format!("{p}front_outer"), ps.front_outer_layers());
    dump_mask(o, &format!("{p}back_outer"), ps.back_outer_layers());
    emit!(
        o,
        format!("{p}is_tented.F"),
        v_opt_bool(ps.is_tented(PcbLayerId::FCu))
    );
    emit!(
        o,
        format!("{p}is_tented.B"),
        v_opt_bool(ps.is_tented(PcbLayerId::BCu))
    );
    emit!(
        o,
        format!("{p}is_covered.F"),
        v_opt_bool(ps.is_covered(PcbLayerId::FCu))
    );
    emit!(
        o,
        format!("{p}is_covered.B"),
        v_opt_bool(ps.is_covered(PcbLayerId::BCu))
    );
    emit!(
        o,
        format!("{p}is_plugged.F"),
        v_opt_bool(ps.is_plugged(PcbLayerId::FCu))
    );
    emit!(
        o,
        format!("{p}is_plugged.B"),
        v_opt_bool(ps.is_plugged(PcbLayerId::BCu))
    );
    emit!(o, format!("{p}is_capped"), v_opt_bool(ps.is_capped()));
    emit!(o, format!("{p}is_filled"), v_opt_bool(ps.is_filled()));
    emit!(o, format!("{p}unique_layers"), v_layers(ps.unique_layers()));

    let Some(layers) = o.cond(&format!("{p}copper.*"), || ps.unique_layers()) else {
        return;
    };

    for l in layers {
        let lp = format!("{p}copper.{}.", v_layer(l));
        let Some(c) = o.cond(&format!("{lp}*"), || ps.copper_props(l)) else {
            continue;
        };
        emit!(o, format!("{lp}shape"), v_enum(c.shape.shape));
        emit!(o, format!("{lp}anchor_shape"), v_enum(c.shape.anchor_shape));
        emit!(o, format!("{lp}size"), v_vec(c.shape.size));
        emit!(o, format!("{lp}offset"), v_vec(c.shape.offset));
        emit!(
            o,
            format!("{lp}round_rect_radius_ratio"),
            v_f64(c.shape.round_rect_radius_ratio)
        );
        emit!(
            o,
            format!("{lp}chamfered_rect_ratio"),
            v_f64(c.shape.chamfered_rect_ratio)
        );
        emit!(
            o,
            format!("{lp}chamfered_rect_positions"),
            v_int(c.shape.chamfered_rect_positions)
        );
        emit!(
            o,
            format!("{lp}trapezoid_delta_size"),
            v_vec(c.shape.trapezoid_delta_size)
        );
        emit!(
            o,
            format!("{lp}zone_connection"),
            v_opt_enum(c.zone_connection)
        );
        emit!(
            o,
            format!("{lp}thermal_spoke_width"),
            v_opt_int(c.thermal_spoke_width)
        );
        emit!(
            o,
            format!("{lp}thermal_spoke_angle"),
            c.thermal_spoke_angle
                .map_or_else(|| "none".to_string(), |a| v_angle(a.angle()))
        );
        emit!(o, format!("{lp}thermal_gap"), v_opt_int(c.thermal_gap));
        emit!(o, format!("{lp}clearance"), v_opt_int(c.clearance));
        emit!(
            o,
            format!("{lp}round_rect_radius"),
            v_int(ps.round_rect_radius(l))
        );
        emit!(
            o,
            format!("{lp}thermal_spoke_angle_effective"),
            v_angle(ps.thermal_spoke_angle(l))
        );
        emit!(
            o,
            format!("{lp}primitives"),
            v_int(c.custom_shapes.len() as i64)
        );

        for (i, prim) in c.custom_shapes.iter().enumerate() {
            let pp = format!("{lp}primitive.{i}.");
            let s = &prim.shape;
            emit!(o, format!("{pp}shape"), v_enum(s.shape()));
            emit!(o, format!("{pp}start"), v_vec(s.start()));
            emit!(o, format!("{pp}end"), v_vec(s.end()));
            emit!(
                o,
                format!("{pp}width"),
                v_int(EdaShapeItem::width(prim, &no_ctx()))
            );
            emit!(o, format!("{pp}fill"), v_enum(s.fill_mode()));

            match s.shape() {
                ShapeT::Arc => emit!(o, format!("{pp}mid"), v_vec(s.arc_mid())),
                ShapeT::Bezier => emit!(
                    o,
                    format!("{pp}bezier"),
                    v_points(&[s.bezier_c1(), s.bezier_c2()])
                ),
                ShapeT::Poly => emit!(o, format!("{pp}poly"), v_polyset(s.poly_shape())),
                ShapeT::Rectangle => {
                    emit!(o, format!("{pp}corner_radius"), v_int(s.corner_radius()))
                }
                _ => {}
            }
        }
    }
}

/// Custom-shape primitives are inline items without a board; their `GetWidth()` never looks at
/// the context, but the Rust signature wants one.
fn no_ctx() -> crate::ctx::BoardCtx<'static> {
    static EMPTY: std::sync::OnceLock<crate::board::Board> = std::sync::OnceLock::new();
    crate::ctx::BoardCtx::bare(EMPTY.get_or_init(crate::board::Board::new))
}

// ---- track / arc / via --------------------------------------------------------------------

/// `DumpTrack`.
pub(crate) fn dump_track(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let AnyItem::Track(t) = it.any() else { return };
    let tr = t.as_track();

    emit!(o, "start", v_vec(tr.start()));
    emit!(o, "end", v_vec(tr.end()));

    if t.as_via().is_none() {
        emit!(o, "width", v_int(t.width()));
        emit!(o, "length", v_f64(t.length()));
    }

    emit!(o, "is_null", v_bool(t.is_null()));
    emit!(o, "has_solder_mask", v_bool(tr.has_solder_mask));
    emit!(
        o,
        "local_solder_mask_margin",
        v_opt_int(tr.solder_mask_margin)
    );
    // Non-virtual PCB_TRACK member called through a PCB_TRACK*: the track part also for vias.
    emit!(
        o,
        "solder_mask_expansion",
        v_int(match t.as_arc() {
            Some(a) => a.solder_mask_expansion(ctx),
            None => tr.solder_mask_expansion(ctx),
        })
    );

    if let Some(_a) = t.as_arc() {
        emit!(o, "mid", v_vec(_a.mid));
        emit!(o, "center", v_vec(it.as_dyn().center(ctx)));
        emit!(o, "radius", v_f64(_a.radius()));
        emit!(o, "angle", v_angle(_a.angle()));
        emit!(o, "angle_start", v_angle(_a.arc_angle_start()));
        emit!(o, "angle_end", v_angle(_a.arc_angle_end()));
        emit!(o, "ccw", v_bool(_a.is_ccw()));
        emit!(o, "degenerated", v_bool(_a.is_degenerated(5)));
    }

    if let Some(v) = t.as_via() {
        emit!(o, "via_type", v_enum(v.via_type));
        emit!(o, "top_layer", v_layer(v.top_layer()));
        emit!(o, "bottom_layer", v_layer(v.bottom_layer()));
        emit!(o, "drill", v_int(v.drill()));
        emit!(o, "drill_value", v_int(v.drill_value(ctx)));
        emit!(o, "front_width", v_int(v.front_width()));
        emit!(o, "micro", v_bool(v.is_micro_via()));
        emit!(o, "blind", v_bool(v.is_blind_via()));
        emit!(o, "buried", v_bool(v.is_buried_via()));
        emit!(o, "is_free", v_bool(v.is_free));
        emit!(o, "remove_unconnected", v_bool(v.remove_unconnected()));
        emit!(o, "keep_start_end", v_bool(v.keep_start_end()));

        emit!(o, "front_tenting", v_enum(v.front_tenting_mode()));
        emit!(o, "back_tenting", v_enum(v.back_tenting_mode()));
        emit!(o, "front_covering", v_enum(v.front_covering_mode()));
        emit!(o, "back_covering", v_enum(v.back_covering_mode()));
        emit!(o, "front_plugging", v_enum(v.front_plugging_mode()));
        emit!(o, "back_plugging", v_enum(v.back_plugging_mode()));
        emit!(o, "capping", v_enum(v.capping_mode()));
        emit!(o, "filling", v_enum(v.filling_mode()));

        emit!(o, "is_tented.F", v_bool(it.is_tented(PcbLayerId::FMask)));
        emit!(o, "is_tented.B", v_bool(it.is_tented(PcbLayerId::BMask)));

        emit!(
            o,
            "primary_drill_filled",
            v_opt_bool(v.primary_drill_filled())
        );
        emit!(
            o,
            "primary_drill_capped",
            v_opt_bool(v.primary_drill_capped())
        );
        emit!(
            o,
            "secondary_drill_size",
            v_opt_int(v.secondary_drill_size())
        );
        emit!(o, "tertiary_drill_size", v_opt_int(v.tertiary_drill_size()));
        emit!(o, "backdrill_mode", v_enum(v.backdrill_mode()));
        emit!(
            o,
            "bottom_backdrill_size",
            v_opt_int(v.bottom_backdrill_size())
        );
        emit!(
            o,
            "bottom_backdrill_layer",
            v_layer(v.bottom_backdrill_layer())
        );
        emit!(o, "top_backdrill_size", v_opt_int(v.top_backdrill_size()));
        emit!(o, "top_backdrill_layer", v_layer(v.top_backdrill_layer()));
        emit!(
            o,
            "front_post_machining",
            v_opt_enum(v.front_post_machining())
        );
        emit!(
            o,
            "back_post_machining",
            v_opt_enum(v.back_post_machining())
        );

        if it.has_board() {
            emit!(
                o,
                "valid_layer_pair",
                v_bool(v.has_valid_layer_pair(ctx.board.copper_layer_count()))
            );
        }

        dump_padstack(o, "padstack.", &v.padstack);

        for l in copper_layers(item_copper_count(it)) {
            let ln = v_layer(l);
            emit!(o, format!("on_layer.{ln}"), v_bool(it.is_on_layer(l)));

            if o.cond(&format!("*.{ln}"), || it.is_on_layer(l)) != Some(true) {
                continue;
            }

            emit!(o, format!("width.{ln}"), v_int(v.width(l)));
            emit!(o, format!("flash.{ln}"), v_bool(v.flash_layer(ctx, l)));
            dump_effective_shape(o, &format!("shape.{ln}"), it, l, Flashing::Default);
            dump_effective_shape(
                o,
                &format!("shape_flashed.{ln}"),
                it,
                l,
                Flashing::AlwaysFlashed,
            );
        }

        emit!(o, "hole_shape", {
            let s =
                it.effective_hole_shape(PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint);
            v_shape(s.as_deref().map(|s| s as &dyn kicad_geom::Shape))
        });

        dump_transformed_polys(o, it, PcbLayerId::FCu);
    } else {
        let layer = it.layer();
        dump_effective_shape(o, "shape", it, layer, Flashing::Default);
        dump_transformed_polys(o, it, layer);
    }
}

// ---- pad ----------------------------------------------------------------------------------

/// `DumpPad`.
pub(crate) fn dump_pad(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let AnyItem::Pad(pad) = it.any() else { return };
    let ps = &pad.padstack;

    emit!(o, "number", v_str(pad.number()));
    emit!(o, "pin_function", v_str(pad.pin_function()));
    emit!(o, "pin_type", v_str(pad.pin_type()));
    emit!(o, "attribute", v_enum(pad.attribute()));
    emit!(o, "property", v_enum(pad.property()));
    emit!(o, "padstack_mode", v_enum(ps.mode()));
    emit!(o, "position", v_vec(pad.position(ctx)));
    emit!(o, "library_position", v_vec(pad.lib_pos()));
    emit!(o, "orientation", v_angle(pad.orientation(ctx)));
    emit!(
        o,
        "fp_relative_orientation",
        v_angle(pad.fp_relative_orientation(ctx))
    );
    emit!(o, "primitive_lib_scale", {
        let (sx, sy) = pad.primitive_lib_scale(ctx);
        format!("{},{}", v_f64(sx), v_f64(sy))
    });
    emit!(o, "flipped", v_bool(pad.is_flipped(ctx)));
    emit!(o, "aperture", v_bool(pad.is_aperture_pad()));
    emit!(o, "npth_no_copper", v_bool(pad.is_npth_with_no_copper(ctx)));
    emit!(o, "no_connect", v_bool(pad.is_no_connect_pad()));
    emit!(o, "free", v_bool(pad.is_free_pad(ctx)));
    emit!(o, "can_have_number", v_bool(pad.can_have_number()));
    emit!(o, "principal_layer", v_layer(pad.principal_layer(ctx)));
    emit!(o, "drill_size", v_vec(pad.drill_size(ctx)));
    emit!(o, "drill_shape", v_enum(ps.drill_shape()));
    emit!(o, "primary_drill_start", v_layer(ps.drill().start));
    emit!(o, "primary_drill_end", v_layer(ps.drill().end));
    emit!(o, "secondary_drill_size", v_vec(ps.secondary_drill().size));
    emit!(o, "tertiary_drill_size", v_vec(ps.tertiary_drill().size));
    emit!(o, "backdrill_mode", v_enum(ps.backdrill_mode()));
    emit!(
        o,
        "front_post_machining",
        v_opt_enum(ps.front_post_machining().mode)
    );
    emit!(
        o,
        "back_post_machining",
        v_opt_enum(ps.back_post_machining().mode)
    );
    emit!(o, "pad_to_die_length", v_int(pad.pad_to_die_length()));
    emit!(o, "pad_to_die_delay", v_int(pad.pad_to_die_delay()));
    emit!(o, "local_clearance", v_opt_int(pad.local_clearance()));
    emit!(
        o,
        "local_solder_mask_margin",
        v_opt_int(ps.front_outer_layers().solder_mask_margin)
    );
    emit!(
        o,
        "local_solder_paste_margin",
        v_opt_int(ps.front_outer_layers().solder_paste_margin)
    );
    emit!(
        o,
        "local_solder_paste_margin_ratio",
        v_opt_f64(ps.front_outer_layers().solder_paste_margin_ratio)
    );
    emit!(
        o,
        "local_zone_connection",
        v_enum(pad.local_zone_connection())
    );
    emit!(
        o,
        "zone_connection_overrides",
        v_enum(pad.zone_connection_overrides(ctx, false).0)
    );
    emit!(
        o,
        "clearance_overrides",
        v_opt_int(pad.clearance_overrides(ctx, false).map(|c| c.0))
    );
    emit!(
        o,
        "local_thermal_spoke_width",
        v_opt_int(pad.local_thermal_spoke_width_override())
    );
    emit!(
        o,
        "local_spoke_width_override",
        v_int(pad.local_spoke_width_override(false).0)
    );
    emit!(o, "thermal_spoke_angle", v_angle(pad.thermal_spoke_angle()));
    emit!(o, "thermal_gap", v_int(pad.thermal_gap()));
    emit!(
        o,
        "local_thermal_gap",
        // The std::optional overload: m_padStack.ThermalGap() (F_Cu).
        v_opt_int(ps.thermal_gap(PcbLayerId::FCu))
    );
    emit!(
        o,
        "unconnected_layer_mode",
        v_enum(ps.unconnected_layer_mode())
    );
    emit!(
        o,
        "custom_shape_in_zone",
        v_enum(ps.custom_shape_in_zone_mode())
    );

    for l in [PcbLayerId::FMask, PcbLayerId::BMask] {
        emit!(
            o,
            format!("solder_mask_expansion.{}", v_layer(l)),
            v_int(pad.solder_mask_expansion(ctx, l))
        );
    }

    for l in [PcbLayerId::FPaste, PcbLayerId::BPaste] {
        emit!(
            o,
            format!("solder_paste_margin.{}", v_layer(l)),
            v_vec(pad.solder_paste_margin(ctx, l))
        );
    }

    dump_padstack(o, "padstack.", ps);

    if let Some(layers) = o.cond("layer.*", || ps.unique_layers()) {
        for l in layers {
            let lp = format!("layer.{}.", v_layer(l));
            emit!(o, format!("{lp}shape"), v_enum(pad.shape(l)));
            emit!(o, format!("{lp}anchor_shape"), v_enum(ps.anchor_shape(l)));
            emit!(o, format!("{lp}size"), v_vec(pad.size(ctx, l)));
            emit!(o, format!("{lp}offset"), v_vec(pad.offset(ctx, l)));
            emit!(o, format!("{lp}delta"), v_vec(pad.delta(l)));
            emit!(
                o,
                format!("{lp}round_rect_radius_ratio"),
                v_f64(ps.round_rect_radius_ratio(l))
            );
            emit!(
                o,
                format!("{lp}round_rect_corner_radius"),
                v_int(pad.round_rect_corner_radius(l))
            );
            emit!(o, format!("{lp}chamfer_ratio"), v_f64(ps.chamfer_ratio(l)));
            emit!(
                o,
                format!("{lp}chamfer_positions"),
                v_int(ps.chamfer_positions(l))
            );
        }
    }

    for l in copper_layers(item_copper_count(it)) {
        let ln = v_layer(l);
        emit!(o, format!("on_layer.{ln}"), v_bool(pad.is_on_layer(ctx, l)));
        emit!(o, format!("flash.{ln}"), v_bool(pad.flash_layer(ctx, l)));
        emit!(
            o,
            format!("flash_permitted.{ln}"),
            v_bool(pad.flash_layer_permitted(ctx, l))
        );

        if o.cond(&format!("*.{ln}"), || pad.is_on_layer(ctx, l)) != Some(true) {
            continue;
        }

        emit!(
            o,
            format!("own_clearance.{ln}"),
            v_int(pad.own_clearance(ctx, l, false).0)
        );
        dump_effective_shape(o, &format!("shape.{ln}"), it, l, Flashing::Default);
        dump_effective_shape(
            o,
            &format!("shape_flashed.{ln}"),
            it,
            l,
            Flashing::AlwaysFlashed,
        );
        o.guard("effective_polygon", |o| {
            emit!(
                o,
                format!("effective_polygon.{ln}"),
                v_polyset(&pad.effective_polygon(ctx, l, ErrorLoc::Inside))
            );
            emit!(
                o,
                format!("effective_polygon_out.{ln}"),
                v_polyset(&pad.effective_polygon(ctx, l, ErrorLoc::Outside))
            );
        });
    }

    if let Some(ls) = o.cond("shape.*", || it.layer_set()) {
        for l in ls.seq().iter().copied() {
            if kicad_core::layer_ids::is_copper_layer(l.as_i32()) {
                continue;
            }

            dump_effective_shape(
                o,
                &format!("shape.{}", v_layer(l)),
                it,
                l,
                Flashing::Default,
            );
        }
    }

    emit!(o, "hole_shape", {
        let s = it.effective_hole_shape(PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint);
        v_shape(s.as_deref().map(|s| s as &dyn kicad_geom::Shape))
    });

    for c in &POLY_COMBOS {
        emit!(o, format!("hole_poly.{}", c.name), {
            let mut buf = ShapePolySet::new();
            let ok =
                pad.transform_hole_to_polygon(ctx, &mut buf, c.clearance, c.max_error, c.error_loc);
            if ok {
                v_polyset(&buf)
            } else {
                "none".to_string()
            }
        });
    }

    if let Some(tl) = o.cond("tpoly.*", || {
        if pad.is_on_layer(ctx, PcbLayerId::FCu) {
            PcbLayerId::FCu
        } else {
            pad.principal_layer(ctx)
        }
    }) {
        dump_transformed_polys(o, it, tl);
    }
}

// ---- shape --------------------------------------------------------------------------------

/// `VEnum( LINE_STYLE )`: the magic_enum names (`DASHDOT`, not the derived `DASH_DOT`).
fn v_line_style(style: kicad_core::stroke_params::LineStyle) -> String {
    use kicad_core::stroke_params::LineStyle;

    match style {
        LineStyle::DashDot => "DASHDOT".into(),
        LineStyle::DashDotDot => "DASHDOTDOT".into(),
        other => v_enum(other),
    }
}

/// `DumpEdaShape`.
pub(crate) fn dump_eda_shape(o: &mut Out, p: &str, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let Some(h) = it.as_eda_shape() else { return };
    let s = h.eda_shape();
    let st = s.shape();
    emit!(o, format!("{p}shape"), v_enum(st));
    emit!(o, format!("{p}start"), v_vec(s.start()));
    emit!(o, format!("{p}end"), v_vec(s.end()));
    emit!(o, format!("{p}width"), v_int(h.width(ctx)));
    emit!(
        o,
        format!("{p}effective_width"),
        v_int(h.effective_width(ctx))
    );
    emit!(o, format!("{p}line_style"), v_line_style(s.line_style()));
    emit!(o, format!("{p}fill"), v_enum(s.fill_mode()));
    emit!(o, format!("{p}closed"), v_bool(s.is_closed()));
    emit!(
        o,
        format!("{p}hatch_line_width"),
        v_int(h.hatch_line_width(ctx))
    );
    emit!(
        o,
        format!("{p}hatch_line_spacing"),
        v_int(h.hatch_line_spacing(ctx))
    );

    match st {
        ShapeT::Arc => {
            emit!(o, format!("{p}center"), v_vec(s.center()));
            emit!(o, format!("{p}mid"), v_vec(s.arc_mid()));
            emit!(o, format!("{p}radius"), v_int(s.radius()));
            emit!(o, format!("{p}arc_angle"), v_angle(s.arc_angle()));
            emit!(o, format!("{p}arc_angles"), {
                let (a0, a1) = s.calc_arc_angles();
                format!("{},{}", v_angle(a0), v_angle(a1))
            });
            emit!(o, format!("{p}clockwise"), v_bool(s.is_clockwise_arc()));
            emit!(o, format!("{p}length"), v_f64(h.shape_length()));
        }
        ShapeT::Circle => {
            emit!(o, format!("{p}center"), v_vec(s.center()));
            emit!(o, format!("{p}radius"), v_int(s.radius()));
        }
        ShapeT::Rectangle => {
            emit!(o, format!("{p}corner_radius"), v_int(s.corner_radius()));
            emit!(o, format!("{p}rect_corners"), v_points(&s.rect_corners()));
            emit!(o, format!("{p}rect_size"), {
                let (w, hh) = h.rectangle_size();
                format!("{w},{hh}")
            });
        }
        ShapeT::Segment => {
            emit!(o, format!("{p}length"), v_f64(h.shape_length()));
            emit!(o, format!("{p}segment_angle"), v_angle(s.segment_angle()));
        }
        ShapeT::Bezier => {
            emit!(o, format!("{p}bezier_c1"), v_vec(s.bezier_c1()));
            emit!(o, format!("{p}bezier_c2"), v_vec(s.bezier_c2()));
            emit!(o, format!("{p}bezier_points"), v_points(s.bezier_points()));
        }
        ShapeT::Poly => {
            emit!(o, format!("{p}poly"), v_polyset(s.poly_shape()));
            emit!(o, format!("{p}point_count"), v_int(s.point_count()));
        }
        ShapeT::Ellipse | ShapeT::EllipseArc => {
            let e = s.ellipse();
            emit!(o, format!("{p}ellipse_center"), v_vec(e.center));
            emit!(o, format!("{p}ellipse_major"), v_int(e.major_radius));
            emit!(o, format!("{p}ellipse_minor"), v_int(e.minor_radius));
            emit!(o, format!("{p}ellipse_rotation"), v_angle(e.rotation));
            emit!(o, format!("{p}ellipse_start"), v_angle(e.start_angle));
            emit!(o, format!("{p}ellipse_end"), v_angle(e.end_angle));
        }
        ShapeT::Undefined => {}
    }

    if s.is_hatched_fill() {
        emit!(o, format!("{p}hatching"), v_polyset(&h.hatching(ctx)));
    }
}

/// `DumpShape` (PCB_SHAPE, textbox, table cell).
pub(crate) fn dump_shape(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let Some(s) = it.as_shape() else { return };

    dump_eda_shape(o, "", it);
    emit!(o, "has_solder_mask", v_bool(s.has_solder_mask()));
    emit!(
        o,
        "local_solder_mask_margin",
        v_opt_int(s.local_solder_mask_margin())
    );
    emit!(
        o,
        "solder_mask_expansion",
        v_int(s.solder_mask_expansion(ctx))
    );
    emit!(
        o,
        "proxy",
        v_bool(it.as_eda_shape().expect("shape").is_proxy_item())
    );
    emit!(o, "lib.shape", v_enum(s.lib_shape()));
    emit!(o, "lib.start", v_vec(s.lib_start()));
    emit!(o, "lib.end", v_vec(s.lib_end()));
    emit!(o, "lib.corner_radius", v_int(s.lib_corner_radius()));

    let st = s.shape.shape();

    if st == ShapeT::Arc {
        emit!(o, "lib.arc_mid", v_vec(s.lib_arc_mid()));
    }

    if st == ShapeT::Bezier {
        emit!(
            o,
            "lib.bezier",
            v_points(&[s.lib_bezier_c1(), s.lib_bezier_c2()])
        );
    }

    if st == ShapeT::Poly {
        emit!(o, "lib.poly", v_polyset(&s.lib_poly_shape()));
    }

    if matches!(st, ShapeT::Ellipse | ShapeT::EllipseArc) {
        emit!(
            o,
            "lib.ellipse",
            format!(
                "{} {} {} {} {} {}",
                v_vec(s.lib_ellipse_center()),
                s.lib_ellipse_major_radius(),
                s.lib_ellipse_minor_radius(),
                v_angle(s.lib_ellipse_rotation()),
                v_angle(s.lib_ellipse_start_angle()),
                v_angle(s.lib_ellipse_end_angle())
            )
        );
    }

    if st == ShapeT::Rectangle || (st == ShapeT::Poly && s.shape.poly_shape().total_vertices() > 0)
    {
        // GetCorners() is virtual (PCB_TEXTBOX overrides it).
        emit!(
            o,
            "corners",
            v_points(&match it.any() {
                AnyItem::Textbox(t) => t.corners(ctx),
                AnyItem::Tablecell(c) => c.corners(ctx),
                _ => s.corners(ctx),
            })
        );
    }

    if let Some(ls) = o.cond("shape.*|tpoly.*", || it.layer_set()) {
        for l in ls.seq().iter().copied() {
            dump_effective_shape(
                o,
                &format!("shape.{}", v_layer(l)),
                it,
                l,
                Flashing::Default,
            );
            dump_transformed_polys(o, it, l);
        }
    }
}

// ---- text ---------------------------------------------------------------------------------

/// `DumpText`.
pub(crate) fn dump_text(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let Some(t) = it.as_eda_text() else { return };
    let et = t.eda_text();

    emit!(o, "text", v_str(et.text()));
    emit!(
        o,
        "shown_text",
        v_str(&t.shown_text(ctx, ResolutionContext::Resolved, 0))
    );
    emit!(o, "text_pos", v_vec(t.text_pos(ctx)));
    emit!(o, "text_size", v_vec(t.text_size(ctx)));
    emit!(o, "text_thickness", v_int(t.text_thickness(ctx)));
    emit!(o, "auto_thickness", v_bool(t.auto_thickness(ctx)));
    emit!(
        o,
        "effective_pen_width",
        v_int(t.effective_text_pen_width(ctx, 0))
    );
    emit!(o, "text_angle", v_angle(t.text_angle(ctx)));
    emit!(o, "italic", v_bool(et.is_italic()));
    emit!(o, "bold", v_bool(et.is_bold()));
    emit!(o, "visible", v_bool(et.is_visible()));
    emit!(o, "mirrored", v_bool(et.is_mirrored()));
    emit!(o, "multiline", v_bool(et.is_multiline_allowed()));
    emit!(
        o,
        "h_justify",
        format!("GR_TEXT_H_ALIGN_{}", v_enum(et.attributes().halign))
    );
    emit!(
        o,
        "v_justify",
        format!("GR_TEXT_V_ALIGN_{}", v_enum(et.attributes().valign))
    );
    emit!(o, "keep_upright", v_bool(et.is_keep_upright()));
    emit!(o, "line_spacing", v_f64(et.line_spacing()));
    emit!(
        o,
        "font",
        v_str(et.attributes().font.as_deref().unwrap_or(""))
    );
    emit!(o, "default_formatting", v_bool(et.is_default_formatting()));
    emit!(o, "hyperlink", v_str(et.hyperlink()));
    emit!(o, "draw_pos", v_vec(t.draw_pos(ctx)));
    emit!(o, "draw_rotation", v_angle(t.draw_rotation(ctx)));
    emit!(o, "text_box", v_box(t.text_box(ctx, None)));
    emit!(o, "interline", v_int(t.interline(ctx)));

    let ty = it.type_();

    if let AnyItem::Field(f) = it.any() {
        emit!(o, "field.id", v_enum(f.field_id()));
        emit!(o, "field.name", v_str(&f.name(true)));
        emit!(o, "field.default_name", v_str(&f.name(true)));
        emit!(o, "field.raw_name", v_str(&f.name(false)));
        emit!(o, "field.ordinal", v_int(f.ordinal()));
        emit!(o, "field.mandatory", v_bool(f.is_mandatory()));
    }

    if let Some(tb) = it.as_textbox() {
        emit!(
            o,
            "textbox.margins",
            format!(
                "{},{},{},{}",
                tb.margin_left, tb.margin_top, tb.margin_right, tb.margin_bottom
            )
        );
        emit!(o, "textbox.border_enabled", v_bool(tb.border_enabled));
        emit!(o, "textbox.border_width", v_int(tb.border_width()));
        emit!(
            o,
            "textbox.corners",
            v_points(&match it.any() {
                AnyItem::Tablecell(c) => c.corners(ctx),
                _ => tb.corners(ctx),
            })
        );
    }

    if let AnyItem::Tablecell(c) = it.any() {
        emit!(
            o,
            "cell.row_col",
            format!("{},{}", c.row(ctx), c.column(ctx))
        );
        emit!(o, "cell.span", format!("{},{}", c.row_span(), c.col_span()));
    }

    emit!(o, "text_shape", {
        let s = t.effective_text_shape(ctx, false, None, kicad_geom::ANGLE_0);
        v_shape(Some(s.as_ref() as &dyn kicad_geom::Shape))
    });

    emit!(o, "text_polys.c0_e5000_in", {
        if matches!(ty, KicadT::PcbTextboxT | KicadT::PcbTablecellT) {
            // PCB_TEXTBOX::TransformTextToPolySet on the real host (a cell is its own host).
            let mut buf = ShapePolySet::new();

            match it.any() {
                AnyItem::Tablecell(c) => crate::pcb_textbox::transform_text_to_poly_set(
                    c,
                    ctx,
                    &mut buf,
                    0,
                    5000,
                    ErrorLoc::Inside,
                ),
                _ => it
                    .as_textbox()
                    .expect("textbox")
                    .transform_text_to_poly_set(ctx, &mut buf, 0, 5000, ErrorLoc::Inside),
            }

            v_polyset(&buf)
        } else {
            let mut buf = ShapePolySet::new();
            it.as_text().expect("text").transform_text_to_poly_set(
                ctx,
                &mut buf,
                0,
                5000,
                ErrorLoc::Inside,
            );
            v_polyset(&buf)
        }
    });

    if it.as_dyn().is_knockout() {
        if let Some(l) = o.cond("tpoly.*", || it.layer()) {
            dump_transformed_polys(o, it, l);
        }
    }

    if let Some(l) = o.cond("effective_shape", || it.layer()) {
        dump_effective_shape(o, "effective_shape", it, l, Flashing::Default);
    }
}

// ---- dimension ----------------------------------------------------------------------------

/// `DumpDimension`.
pub(crate) fn dump_dimension(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let AnyItem::Dimension(d) = it.any() else {
        return;
    };

    emit!(o, "start", v_vec(d.start(ctx)));
    emit!(o, "end", v_vec(d.end(ctx)));
    emit!(o, "lib_start", v_vec(d.lib_start()));
    emit!(o, "lib_end", v_vec(d.lib_end()));
    emit!(o, "override_text_enabled", v_bool(d.override_text_enabled));
    emit!(o, "override_text", v_str(d.override_text()));
    emit!(o, "value_mode", no_api("PCB_DIMENSION_BASE::GetValueMode"));
    emit!(o, "measured_value", v_int(d.measured_value));
    emit!(o, "value_text", v_str(&d.value_text()));
    emit!(o, "prefix", v_str(&d.prefix));
    emit!(o, "suffix", v_str(&d.suffix));
    emit!(o, "arrow_direction", v_enum(d.arrow_direction));
    emit!(o, "units", v_enum(d.units));
    emit!(o, "units_mode", v_enum(d.units_mode()));
    emit!(o, "units_format", v_enum(d.units_format));
    emit!(o, "precision", v_enum(d.precision));
    emit!(o, "suppress_zeroes", v_bool(d.suppress_zeroes));
    emit!(o, "keep_text_aligned", v_bool(d.keep_text_aligned));
    emit!(o, "text_position_mode", v_enum(d.text_position));
    emit!(o, "arrow_length", v_int(d.arrow_length));
    emit!(o, "extension_offset", v_int(d.extension_offset));
    emit!(o, "line_thickness", v_int(d.line_thickness));
    emit!(o, "shape_count", v_int(d.shapes.len() as i64));

    for (i, s) in d.shapes.iter().enumerate() {
        emit!(o, format!("shapes.{i}"), v_shape(Some(s.as_ref())));
    }

    match &d.kind {
        DimensionKind::Aligned(a) | DimensionKind::Orthogonal { aligned: a, .. } => {
            emit!(
                o,
                "crossbar",
                format!("{} {}", v_vec(a.cross_bar_start), v_vec(a.cross_bar_end))
            );
            emit!(o, "height", v_int(a.height));
            emit!(o, "extension_height", v_int(a.extension_height));
            emit!(o, "angle", v_f64(d.angle(ctx)));

            if let DimensionKind::Orthogonal { orientation, .. } = &d.kind {
                emit!(o, "orientation", v_enum(*orientation));
            }
        }
        DimensionKind::Radial { leader_length } => {
            emit!(o, "leader_length", v_int(*leader_length));
            emit!(o, "knee", v_vec(d.knee(ctx)));
        }
        DimensionKind::Leader { text_border } => {
            emit!(o, "text_border", v_enum(*text_border));
        }
        _ => {}
    }

    if let Some(l) = o.cond("tpoly.*", || it.layer()) {
        dump_transformed_polys(o, it, l);
    }
}

// ---- zone ---------------------------------------------------------------------------------

/// `DumpZone`.
pub(crate) fn dump_zone(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let AnyItem::Zone(z) = it.any() else { return };

    emit!(o, "name", v_str(z.zone_name()));
    emit!(o, "priority", v_int(z.assigned_priority()));
    emit!(o, "rule_area", v_bool(z.is_rule_area()));
    emit!(
        o,
        "placement_area_enabled",
        v_bool(z.placement_area_enabled)
    );
    emit!(o, "placement_area_source", v_str(&z.placement_area_source));
    emit!(
        o,
        "placement_area_source_type",
        v_enum(z.placement_area_source_type)
    );
    emit!(o, "keepout.zone_fills", v_bool(z.do_not_allow_zone_fills));
    emit!(o, "keepout.vias", v_bool(z.do_not_allow_vias));
    emit!(o, "keepout.tracks", v_bool(z.do_not_allow_tracks));
    emit!(o, "keepout.pads", v_bool(z.do_not_allow_pads));
    emit!(o, "keepout.footprints", v_bool(z.do_not_allow_footprints));
    emit!(o, "teardrop_area", v_bool(z.is_teardrop_area()));
    emit!(o, "teardrop_type", v_enum(z.teardrop_type));
    emit!(o, "fill_mode", v_enum(z.fill_mode()));
    emit!(o, "local_clearance", v_opt_int(z.local_clearance()));
    emit!(o, "min_thickness", v_int(z.min_thickness()));
    emit!(o, "pad_connection", v_enum(z.pad_connection()));
    emit!(o, "thermal_relief_gap", v_int(z.thermal_relief_gap));
    emit!(
        o,
        "thermal_relief_spoke_width",
        v_int(z.thermal_relief_spoke_width)
    );
    emit!(o, "hatch_thickness", v_int(z.hatch_thickness));
    emit!(o, "hatch_gap", v_int(z.hatch_gap));
    emit!(o, "hatch_orientation", v_angle(z.hatch_orientation()));
    emit!(o, "hatch_smoothing_level", v_int(z.hatch_smoothing_level));
    emit!(o, "hatch_smoothing_value", v_f64(z.hatch_smoothing_value));
    emit!(o, "hatch_hole_min_area", v_f64(z.hatch_hole_min_area));
    emit!(o, "hatch_border_algorithm", v_int(z.hatch_border_algorithm));
    emit!(o, "copper_thieving", v_bool(z.is_copper_thieving()));
    emit!(o, "thieving.pattern", v_enum(z.thieving_pattern()));
    let th = &z.thieving_settings;
    emit!(o, "thieving.element_size", v_int(th.element_size));
    emit!(o, "thieving.gap", v_int(th.gap));
    emit!(o, "thieving.line_width", v_int(th.line_width));
    emit!(o, "thieving.stagger", v_bool(th.stagger));
    emit!(o, "thieving.orientation", v_angle(th.orientation.angle()));
    emit!(o, "island_removal_mode", v_enum(z.island_removal_mode));
    emit!(o, "min_island_area", v_int(z.min_island_area));
    emit!(o, "corner_smoothing", v_enum(z.corner_smoothing_type));
    emit!(o, "corner_radius", v_int(z.corner_radius));
    emit!(o, "border_style", v_enum(z.border_style));
    emit!(o, "border_hatch_pitch", v_int(z.border_hatch_pitch));
    emit!(o, "is_filled", v_bool(z.is_filled()));
    emit!(o, "need_refill", v_bool(z.need_refill()));
    emit!(o, "num_corners", v_int(z.num_corners()));
    emit!(o, "first_layer", v_layer(z.first_layer()));
    emit!(o, "outline", v_polyset(&z.board_outline(ctx)));
    emit!(o, "library_outline", v_polyset(z.library_outline()));
    emit!(o, "hatch_lines", {
        let v: Vec<String> = z
            .border_hatch_lines
            .iter()
            .map(|s| format!("{}>{}", v_vec(s.a), v_vec(s.b)))
            .collect();
        format!("[{}]", v.join(" "))
    });

    if let Some(ls) = o.cond("filled.*|islands.*", || it.layer_set()) {
        for l in ls.seq().iter().copied() {
            let ln = v_layer(l);
            let Some(has) = o.cond(&format!("filled.{ln}|islands.{ln}"), || {
                z.has_filled_polys_for_layer(l)
            }) else {
                continue;
            };

            if !has {
                emit!(o, format!("filled.{ln}"), "none".to_string());
                continue;
            }

            let fill = z.filled_polys_list(l);
            emit!(o, format!("filled.{ln}"), v_polyset(&fill));
            emit!(o, format!("islands.{ln}"), {
                let s: String = (0..fill.outline_count())
                    .map(|i| if z.is_island(l, i) { '1' } else { '0' })
                    .collect();
                format!("[{s}]")
            });
        }
    }

    if let Some(Some(l)) = o.cond("shape.*", || {
        (!z.is_rule_area() && it.as_dyn().is_on_copper_layer(ctx)).then(|| z.first_layer())
    }) {
        dump_effective_shape(
            o,
            &format!("shape.{}", v_layer(l)),
            it,
            l,
            Flashing::Default,
        );
    }
}

// ---- footprint ----------------------------------------------------------------------------

/// `DumpFootprint`.
pub(crate) fn dump_footprint(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;
    let AnyItem::Footprint(fp) = it.any() else {
        return;
    };

    emit!(o, "fpid", v_str(&fp.fpid().format()));
    emit!(o, "reference", v_str(&fp.reference(ctx)));
    emit!(o, "value", v_str(&fp.value(ctx)));
    emit!(o, "position", v_vec(fp.position(ctx)));
    emit!(o, "orientation", v_angle(fp.orientation()));
    emit!(o, "layer", v_layer(fp.layer(ctx)));
    emit!(o, "side", no_api("FOOTPRINT::GetSide"));
    emit!(o, "flipped", v_bool(fp.is_flipped()));

    let tr = fp.transform();
    emit!(o, "transform.translate", v_vec(tr.translate()));
    emit!(o, "transform.rotate", v_angle(tr.rotate()));
    emit!(
        o,
        "transform.scale",
        format!("{},{}", v_f64(tr.scale_x()), v_f64(tr.scale_y()))
    );
    emit!(o, "transform.identity", v_bool(tr.is_identity()));

    emit!(o, "attributes", v_int(fp.attributes()));
    emit!(o, "footprint_type", v_enum(fp.footprint_type()));
    emit!(
        o,
        "likely_attribute",
        no_api("FOOTPRINT::GetLikelyAttribute")
    );
    emit!(o, "board_only", v_bool(fp.is_board_only()));
    emit!(
        o,
        "exclude_from_pos_files",
        v_bool(fp.is_excluded_from_pos_files())
    );
    emit!(o, "exclude_from_bom", v_bool(fp.is_excluded_from_bom()));
    emit!(
        o,
        "exclude_from_sim",
        no_api("FOOTPRINT::IsExcludedFromSim")
    );
    emit!(o, "dnp", v_bool(fp.is_dnp()));
    emit!(
        o,
        "allow_missing_courtyard",
        v_bool(fp.allow_missing_courtyard)
    );
    emit!(
        o,
        "allow_solder_mask_bridges",
        v_bool(fp.allow_solder_mask_bridges)
    );
    emit!(o, "net_tie", v_bool(fp.is_net_tie()));
    emit!(o, "placed", v_bool(fp.is_placed()));
    emit!(o, "local_clearance", v_opt_int(fp.local_clearance()));
    emit!(
        o,
        "local_solder_mask_margin",
        v_opt_int(fp.solder_mask_margin)
    );
    emit!(
        o,
        "local_solder_paste_margin",
        v_opt_int(fp.solder_paste_margin)
    );
    emit!(
        o,
        "local_solder_paste_margin_ratio",
        v_opt_f64(fp.solder_paste_margin_ratio)
    );
    emit!(
        o,
        "local_zone_connection",
        v_enum(fp.local_zone_connection())
    );
    emit!(o, "stackup_mode", v_enum(fp.stackup_mode));
    emit!(o, "stackup_layers", v_lset(&fp.stackup_layers));
    emit!(o, "private_layers", v_lset(&fp.private_layers));
    emit!(o, "path", v_str(&fp.path.as_string()));
    emit!(o, "sheetname", v_str(&fp.sheetname));
    emit!(o, "sheetfile", v_str(&fp.sheetfile));
    emit!(o, "filters", v_str(&fp.filters));
    emit!(o, "description", v_str(&fp.lib_description));
    emit!(o, "keywords", v_str(&fp.keywords));
    emit!(
        o,
        "duplicate_pad_numbers_are_jumpers",
        v_bool(fp.duplicate_pad_numbers_are_jumpers)
    );
    emit!(
        o,
        "file_format_version_at_load",
        v_int(fp.file_format_version_at_load)
    );
    emit!(o, "pad_count", no_api("FOOTPRINT::GetPadCount"));
    emit!(
        o,
        "has_through_hole_pads",
        no_api("FOOTPRINT::HasThroughHolePads")
    );
    emit!(
        o,
        "component_class",
        no_api("FOOTPRINT::GetComponentClassAsString")
    );
    emit!(o, "net_tie_pad_groups", {
        let v: Vec<String> = fp.net_tie_pad_groups.iter().map(|g| v_str(g)).collect();
        format!("[{}]", v.join(" "))
    });
    emit!(o, "jumper_pad_groups", {
        let v: Vec<String> = fp
            .jumper_pad_groups
            .groups
            .iter()
            .map(|g| {
                let n: Vec<String> = g.names.iter().map(|n| v_str(n)).collect();
                format!("{{{}}}", n.join(" "))
            })
            .collect();
        format!("[{}]", v.join(" "))
    });

    let board = ctx.board;

    if let Some(fp_id) = it.id().and_then(|id| id.typed()) {
        let fields: Vec<_> = board.footprint_fields(fp_id).collect();

        for (i, (fid, f)) in fields.into_iter().enumerate() {
            let p = format!("field.{i}.");
            let k = o.item_key(Some(fid.into()));
            emit!(o, format!("{p}key"), k);
            emit!(o, format!("{p}name"), v_str(&f.name(true)));
            emit!(o, format!("{p}text"), v_str(f.text.text.text()));
        }

        let pads: Vec<_> = board.footprint_pads(fp_id).collect();

        for (i, (pid, p)) in pads.into_iter().enumerate() {
            let k = o.item_key(Some(pid.into()));
            emit!(
                o,
                format!("pad.{}", 2 * i + 1),
                format!("{k} {}", v_str(p.number()))
            );
        }

        let drawings: Vec<_> = board.footprint_drawings(fp_id).map(|(id, _)| id).collect();

        for (i, did) in drawings.into_iter().enumerate() {
            let k = o.item_key(Some(did));
            emit!(o, format!("drawing.{}", 2 * i + 1), k);
        }

        let zones: Vec<_> = board.footprint_zones(fp_id).map(|(id, _)| id).collect();

        for (i, zid) in zones.into_iter().enumerate() {
            let k = o.item_key(Some(zid.into()));
            emit!(o, format!("zone.{}", 2 * i + 1), k);
        }
    }

    for (i, m) in fp.models.iter().enumerate() {
        let p = format!("model.{i}.");
        let v3 = |v: &kicad_geom::math::vector3::Vector3D| {
            format!("{},{},{}", v_f64(v.x), v_f64(v.y), v_f64(v.z))
        };
        emit!(o, format!("{p}filename"), v_str(&m.filename));
        emit!(o, format!("{p}offset"), v3(&m.offset));
        emit!(o, format!("{p}rotation"), v3(&m.rotation));
        emit!(o, format!("{p}scale"), v3(&m.scale));
        emit!(o, format!("{p}opacity"), v_f64(m.opacity));
        emit!(o, format!("{p}show"), v_bool(m.show));
    }

    for var in fp.variants.values() {
        emit!(
            o,
            format!("variant.{}.dnp", var.name),
            v_bool(fp.dnp_for_variant(&var.name))
        );
    }

    o.guard("courtyard", |o| {
        emit!(
            o,
            "courtyard.F",
            v_polyset(&fp.courtyard(ctx, PcbLayerId::FCrtYd))
        );
        emit!(
            o,
            "courtyard.B",
            v_polyset(&fp.courtyard(ctx, PcbLayerId::BCrtYd))
        );
    });
    o.guard("hull", |o| {
        emit!(
            o,
            "hull.F_Cu",
            v_polyset(&fp.bounding_hull_on_layer(ctx, PcbLayerId::FCu))
        );
        emit!(
            o,
            "hull.B_Cu",
            v_polyset(&fp.bounding_hull_on_layer(ctx, PcbLayerId::BCu))
        );
    });
    emit!(o, "area", no_api("FOOTPRINT::GetArea"));
}

/// Keep the trait in scope for `local_clearance()` on concrete types.
#[allow(dead_code)]
fn _uses(_: &dyn BoardConnectedItem, _: Vector2I, _: &Track) {}
