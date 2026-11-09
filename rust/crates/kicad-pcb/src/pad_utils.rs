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

//! Port of `pcbnew/pad_utils.h` and `pcbnew/pad_utils.cpp` (namespace `PAD_UTILS`):
//! rounding-radius helpers, drill removal, attribute names and the pad consistency checks
//! (package M13).

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaDataType, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{Shape, ShapePolySet, Vector2I, ki_round};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::footprint::FootprintStackup;
use crate::pad::Pad;
use crate::padstack::{
    DrillProps, PadAttrib, PadDrillShape, PadProp, PadShape, Padstack, PostMachiningProps,
};

/// `PAD_UTILS::PAD_CHECK_CODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum PadCheckCode {
    PadSizeNotPositive,
    PthHoleTooSmall,
    PthHoleLeavesNoCopper,
    PthHoleNotInsideCopper,
    HoleNotInsidePad,
    NegativeLocalClearance,
    NegativeMaskClearanceTooLarge,
    NegativePasteMarginTooLarge,
    RoundrectRadiusNegative,
    RoundrectRadiusTooLarge,
    ChamferNegative,
    ChamferTooLarge,
    TrapezoidDeltaTooLarge,
    CustomShapeNotSinglePolygon,
    ThNoHole,
    PthNoOuterCopper,
    UnexpectedHole,
    MultipleCopperLayers,
    MaskWrongSide,
    PasteWrongSide,
    PadNoOuterCopper,
    ConnHasPaste,
    FiducialNotPlated,
    TestpointNotPlated,
    HeatsinkNotPlated,
    CastellatedNotPth,
    BgaNotSmd,
    MechanicalNotPth,
    PressfitNotPth,
    NonComponentNoCopper,
    NonComponentTechOnInner,
    NonComponentInnerNeedsCustomStackup,
}

/// `PAD_UTILS::PAD_CHECK_RESULT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadCheckResult {
    pub code: PadCheckCode,
    pub message: String,
}

/// Port of `PAD_UTILS::GetDefaultIpcRoundingRatio( aPad, aLayer )`: the IPC-7351C default
/// corner radius (25 % of the smaller pad dimension, at most 0.25 mm) as a ratio.
pub fn default_ipc_rounding_ratio(ctx: &BoardCtx<'_>, pad: &Pad, layer: PcbLayerId) -> f64 {
    let default_proportion = 0.25;
    let minimum_size_iu = f64::from(PCB_IU_SCALE.mm_to_iu(0.25));

    let size = pad.size(ctx, layer);
    let pad_min_size_iu = size.x.min(size.y);
    let candidate = f64::from(pad_min_size_iu) * default_proportion;
    // std::min( a, b ) = ( b < a ) ? b : a
    let default_radius_iu = if candidate < minimum_size_iu {
        candidate
    } else {
        minimum_size_iu
    };

    // Convert back to a ratio
    default_radius_iu / f64::from(pad_min_size_iu)
}

/// Port of `PAD_UTILS::PadHasMeaningfulRoundingRadius( aPad, aLayer )`: round rect and
/// chamfered rect shapes.
pub fn pad_has_meaningful_rounding_radius(
    _ctx: &BoardCtx<'_>,
    pad: &Pad,
    layer: PcbLayerId,
) -> bool {
    let shape = pad.shape(layer);
    shape == PadShape::Roundrect || shape == PadShape::ChamferedRect
}

/// Port of `PAD_UTILS::RemoveAllDrillsFromPadstack( aPadstack )`: every drill and
/// post-machining member value-initialised (`= {}`).
pub fn remove_all_drills_from_padstack(padstack: &mut Padstack) {
    *padstack.drill_mut() = DrillProps::default();
    *padstack.secondary_drill_mut() = DrillProps::default();
    *padstack.tertiary_drill_mut() = DrillProps::default();
    *padstack.front_post_machining_mut() = PostMachiningProps::default();
    *padstack.back_post_machining_mut() = PostMachiningProps::default();
}

/// Port of `PAD_UTILS::GetPadAttributeString( aAttrib )` (the untranslated UI string).
pub fn pad_attribute_string(attrib: PadAttrib) -> String {
    match attrib {
        PadAttrib::Pth => "Through-hole",
        PadAttrib::Smd => "SMD",
        PadAttrib::Npth => "NPTH",
        PadAttrib::Conn => "Connector",
        PadAttrib::NonComponent => "Non-component",
    }
    .to_string()
}

/// Shorthand for one reported problem.
fn report(handler: &mut dyn FnMut(&PadCheckResult), code: PadCheckCode, message: String) {
    handler(&PadCheckResult { code, message });
}

/// Port of the anonymous-namespace `checkPadLayer( aPad, aLayer, aUnitsProvider,
/// aForPadProperties, aErrorHandler )` of pad_utils.cpp.
fn check_pad_layer(
    ctx: &BoardCtx<'_>,
    pad: &Pad,
    layer: PcbLayerId,
    units: &UnitsProvider<'_>,
    for_pad_properties: bool,
    error_handler: &mut dyn FnMut(&PadCheckResult),
) {
    let padstack = pad.lib_padstack();
    let mut pad_size = pad.size(ctx, layer);

    if pad.shape(layer) == PadShape::Custom {
        // BOX2I::GetSize() is a VECTOR2L, converted with clamping
        pad_size = pad.bounding_box(ctx).size().cast::<i32>();
    } else if pad_size.x <= 0 || (pad_size.y <= 0 && pad.shape(layer) != PadShape::Circle) {
        report(
            error_handler,
            PadCheckCode::PadSizeNotPositive,
            "Pad must have a positive size".to_string(),
        );
    }

    // Test hole against pad shape
    if pad.is_on_copper_layer(ctx) && pad.drill_size(ctx).x > 0 {
        // Ensure the drill size can be handled in next calculations.
        // Use min size = 4 IU to be able to build a polygon from a hole shape
        let min_drill_size = 4;

        if pad.drill_size(ctx).x <= min_drill_size || pad.drill_size(ctx).y <= min_drill_size {
            report(
                error_handler,
                PadCheckCode::PthHoleTooSmall,
                format!(
                    "PTH pad hole size must be larger than {}",
                    units.string_from_value(f64::from(min_drill_size), true, EdaDataType::Distance)
                ),
            );
        }

        let mut pad_outline = ShapePolySet::default();

        pad.transform_shape_to_polygon(
            ctx,
            &mut pad_outline,
            layer,
            0,
            ctx.max_error(),
            ErrorLoc::Inside,
            false,
        );

        if pad.attribute() == PadAttrib::Pth {
            // Test if there is copper area outside hole
            let hole = pad
                .effective_hole_shape(
                    ctx,
                    PcbLayerId::UndefinedLayer,
                    DrcConstraintT::NullConstraint,
                )
                .expect("PAD::GetEffectiveHoleShape returns a shape");
            let mut hole_outline = ShapePolySet::default();

            transform_oval_to_polygon(
                &mut hole_outline,
                hole.get_seg().a,
                hole.get_seg().b,
                hole.get_width(),
                ctx.max_error(),
                ErrorLoc::Outside,
                0,
            );

            let mut copper = pad_outline.clone();
            copper.boolean_subtract(&hole_outline);

            if copper.is_empty() {
                report(
                    error_handler,
                    PadCheckCode::PthHoleLeavesNoCopper,
                    "PTH pad hole leaves no copper".to_string(),
                );
            } else if for_pad_properties {
                // Test if the pad hole is fully inside the copper area.  Note that we only run
                // this check for pad properties because we run the more complete annular ring
                // checker on the board (which handles multiple pads with the same name).
                hole_outline.boolean_subtract(&pad_outline);

                if !hole_outline.is_empty() {
                    report(
                        error_handler,
                        PadCheckCode::PthHoleNotInsideCopper,
                        "PTH pad hole not fully inside copper".to_string(),
                    );
                }
            }
        } else {
            // Test only if the pad hole's centre is inside the copper area
            if !pad_outline.collide_point(pad.position(ctx), 0, None, None) {
                report(
                    error_handler,
                    PadCheckCode::HoleNotInsidePad,
                    "pad hole not inside pad shape".to_string(),
                );
            }
        }
    }

    if pad.local_clearance().unwrap_or(0) < 0 {
        report(
            error_handler,
            PadCheckCode::NegativeLocalClearance,
            "negative local clearance values have no effect".to_string(),
        );
    }

    // Some pads need a negative solder mask clearance (mainly for BGA with small pads)
    // However the negative solder mask clearance must not create negative mask size
    // Therefore test for minimal acceptable negative value
    // (GetLocalSolderMaskMargin() = m_padStack.SolderMaskMargin(), default layer F_Cu)
    let solder_mask_margin = padstack.solder_mask_margin(PcbLayerId::FCu);

    if let Some(margin) = solder_mask_margin
        && margin < 0
    {
        let abs_margin = margin.wrapping_abs();

        if pad.shape(layer) == PadShape::Custom {
            for shape in padstack.primitives(layer) {
                let shape_bbox = shape.bounding_box(ctx);

                if i64::from(abs_margin) > shape_bbox.width()
                    || i64::from(abs_margin) > shape_bbox.height()
                {
                    report(
                        error_handler,
                        PadCheckCode::NegativeMaskClearanceTooLarge,
                        "negative solder mask clearance is larger than some shape primitives; \
                         results may be surprising"
                            .to_string(),
                    );

                    break;
                }
            }
        } else if abs_margin > pad_size.x || abs_margin > pad_size.y {
            report(
                error_handler,
                PadCheckCode::NegativeMaskClearanceTooLarge,
                "negative solder mask clearance is larger than pad; no solder mask will be \
                 generated"
                    .to_string(),
            );
        }
    }

    // Some pads need a positive solder paste clearance (mainly for BGA with small pads)
    // However, a positive value can create issues if the resulting shape is too big.
    // (like a solder paste creating a solder paste area on a neighbor pad or on the solder mask)
    // So we could ask for user to confirm the choice
    // For now we just check for disappearing paste
    let paste_margin = padstack.solder_paste_margin(PcbLayerId::FCu).unwrap_or(0);
    let mratio = padstack.solder_paste_margin_ratio(PcbLayerId::FCu);

    let paste_size = Vector2I::new(
        pad_size
            .x
            .wrapping_add(paste_margin)
            .wrapping_add(ki_round(f64::from(pad_size.x) * mratio.unwrap_or(0.0))),
        pad_size
            .y
            .wrapping_add(paste_margin)
            .wrapping_add(ki_round(f64::from(pad_size.y) * mratio.unwrap_or(0.0))),
    );

    if paste_size.x <= 0 || paste_size.y <= 0 {
        report(
            error_handler,
            PadCheckCode::NegativePasteMarginTooLarge,
            "negative solder paste margin is larger than pad; no solder paste mask will be \
             generated"
                .to_string(),
        );
    }

    if pad.shape(layer) == PadShape::Roundrect {
        if padstack.round_rect_radius_ratio(layer) < 0.0 {
            report(
                error_handler,
                PadCheckCode::RoundrectRadiusNegative,
                "negative corner radius is not allowed".to_string(),
            );
        } else if padstack.round_rect_radius_ratio(layer) > 50.0 {
            report(
                error_handler,
                PadCheckCode::RoundrectRadiusTooLarge,
                "corner size will make pad circular".to_string(),
            );
        }
    } else if pad.shape(layer) == PadShape::ChamferedRect {
        if padstack.chamfer_ratio(layer) < 0.0 {
            report(
                error_handler,
                PadCheckCode::ChamferNegative,
                "negative corner chamfer is not allowed".to_string(),
            );
        } else if padstack.chamfer_ratio(layer) > 50.0 {
            report(
                error_handler,
                PadCheckCode::ChamferTooLarge,
                "corner chamfer is too large".to_string(),
            );
        }
    } else if pad.shape(layer) == PadShape::Trapezoid {
        // GetDelta( aLayer ) reads the stored padstack; GetSize( aLayer ) is the board-frame
        // getter.
        let delta = padstack.trapezoid_delta_size(layer);
        let size = pad.size(ctx, layer);

        if (delta.x < 0 && delta.x < size.y.wrapping_neg())
            || (delta.x > 0 && delta.x > size.y)
            || (delta.y < 0 && delta.y < size.x.wrapping_neg())
            || (delta.y > 0 && delta.y > size.x)
        {
            report(
                error_handler,
                PadCheckCode::TrapezoidDeltaTooLarge,
                "trapezoid delta is too large".to_string(),
            );
        }
    }

    if pad.shape(layer) == PadShape::Custom {
        let mut merged_polygon = ShapePolySet::default();
        pad.merge_primitives_as_polygon(ctx, layer, &mut merged_polygon, ErrorLoc::Inside);

        if merged_polygon.outline_count() > 1 {
            report(
                error_handler,
                PadCheckCode::CustomShapeNotSinglePolygon,
                "custom pad shape must resolve to a single polygon".to_string(),
            );
        }
    }
}

/// Port of `PAD_UTILS::CheckPad( aPad, aUnitsProvider, aForPadProperties, aErrorHandler )`:
/// reports every consistency problem of the pad, per unique padstack layer first, then the
/// layer-set / attribute / property checks.
pub fn check_pad(
    ctx: &BoardCtx<'_>,
    pad: &Pad,
    units: &UnitsProvider<'_>,
    for_pad_properties: bool,
    error_handler: &mut dyn FnMut(&PadCheckResult),
) {
    let pad_type_string = pad_attribute_string(pad.attribute());

    // Check single-sided copper pads have mask and paste layers on the same side.
    // Applies to SMD, CONN, NON_COMPONENT
    let check_tech_layer_side = |error_handler: &mut dyn FnMut(&PadCheckResult)| {
        if (pad.is_on_layer(ctx, PcbLayerId::FCu) && pad.is_on_layer(ctx, PcbLayerId::BMask))
            || (pad.is_on_layer(ctx, PcbLayerId::BCu) && pad.is_on_layer(ctx, PcbLayerId::FMask))
        {
            report(
                error_handler,
                PadCheckCode::MaskWrongSide,
                format!(
                    "{pad_type_string} pad has copper and mask layers on different sides of the \
                     board"
                ),
            );
        }

        if (pad.is_on_layer(ctx, PcbLayerId::FCu) && pad.is_on_layer(ctx, PcbLayerId::BPaste))
            || (pad.is_on_layer(ctx, PcbLayerId::BCu) && pad.is_on_layer(ctx, PcbLayerId::FPaste))
        {
            report(
                error_handler,
                PadCheckCode::PasteWrongSide,
                format!(
                    "{pad_type_string} pad has copper and paste layers on different sides of the \
                     board"
                ),
            );
        }
    };

    pad.lib_padstack().for_each_unique_layer(|layer| {
        check_pad_layer(ctx, pad, layer, units, for_pad_properties, error_handler);
    });

    let padlayers_mask = pad.layer_set(ctx);
    let drill_size = pad.drill_size(ctx);

    if !padlayers_mask.contains(PcbLayerId::FCu)
        && !padlayers_mask.contains(PcbLayerId::BCu)
        && (drill_size.x != 0 || drill_size.y != 0)
        && pad.attribute() != PadAttrib::Npth
    {
        report(
            error_handler,
            PadCheckCode::PthNoOuterCopper,
            "plated through holes normally have a copper pad on at least one outer layer"
                .to_string(),
        );
    }

    if (pad.property() == PadProp::FiducialGlbl || pad.property() == PadProp::FiducialLocal)
        && pad.attribute() == PadAttrib::Npth
    {
        report(
            error_handler,
            PadCheckCode::FiducialNotPlated,
            "'fiducial' pads are normally plated".to_string(),
        );
    }

    if pad.property() == PadProp::Testpoint && pad.attribute() == PadAttrib::Npth {
        report(
            error_handler,
            PadCheckCode::TestpointNotPlated,
            "'testpoint' pads are normally plated".to_string(),
        );
    }

    if pad.property() == PadProp::Heatsink && pad.attribute() == PadAttrib::Npth {
        report(
            error_handler,
            PadCheckCode::HeatsinkNotPlated,
            "'heatsink' pads are normally plated".to_string(),
        );
    }

    if pad.property() == PadProp::Castellated && pad.attribute() != PadAttrib::Pth {
        report(
            error_handler,
            PadCheckCode::CastellatedNotPth,
            "'castellated' pads are normally PTH".to_string(),
        );
    }

    if pad.property() == PadProp::Bga && pad.attribute() != PadAttrib::Smd {
        report(
            error_handler,
            PadCheckCode::BgaNotSmd,
            "'BGA' property is for SMD pads".to_string(),
        );
    }

    if pad.property() == PadProp::Mechanical && pad.attribute() != PadAttrib::Pth {
        report(
            error_handler,
            PadCheckCode::MechanicalNotPth,
            "'mechanical' pads are normally PTH".to_string(),
        );
    }

    if pad.property() == PadProp::Pressfit
        && (pad.attribute() != PadAttrib::Pth || !pad.has_drilled_hole(ctx))
    {
        report(
            error_handler,
            PadCheckCode::PressfitNotPth,
            "'press-fit' pads are normally PTH with round holes".to_string(),
        );
    }

    // The SMD / CONN branch of the C++ switch (CONN falls through into SMD).
    let check_smd = |error_handler: &mut dyn FnMut(&PadCheckResult)| {
        if drill_size.x > 0 || drill_size.y > 0 {
            report(
                error_handler,
                PadCheckCode::UnexpectedHole,
                format!("{pad_type_string} pad has a hole"),
            );
        }

        let copper_layers = &padlayers_mask & Lset::all_cu_mask();

        if copper_layers.count() > 1 {
            report(
                error_handler,
                PadCheckCode::MultipleCopperLayers,
                format!("{pad_type_string} pad has more than one copper layer"),
            );
        }

        // SMD also includes aperture pads without copper on any layers
        if copper_layers.any() {
            if (&copper_layers & &Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu])).none() {
                report(
                    error_handler,
                    PadCheckCode::PadNoOuterCopper,
                    format!("{pad_type_string} pad has no outer layers"),
                );
            }

            check_tech_layer_side(error_handler);
        }
    };

    match pad.attribute() {
        // NPTH: Not plated, but through hole, a hole is expected
        // PTH: Pad through hole, a hole is also expected
        PadAttrib::Npth | PadAttrib::Pth => {
            if drill_size.x <= 0
                || (drill_size.y <= 0 && pad.lib_padstack().drill_shape() == PadDrillShape::Oblong)
            {
                report(error_handler, PadCheckCode::ThNoHole, String::new());
            }
        }

        // Connector pads are smd pads, just they do not have solder paste.
        PadAttrib::Conn => {
            if padlayers_mask.contains(PcbLayerId::BPaste)
                || padlayers_mask.contains(PcbLayerId::FPaste)
            {
                report(
                    error_handler,
                    PadCheckCode::ConnHasPaste,
                    "connector pads normally have no solder paste; use a SMD pad instead"
                        .to_string(),
                );
            }

            // KI_FALLTHROUGH
            check_smd(error_handler);
        }

        // SMD and Connector pads (One external copper layer only)
        PadAttrib::Smd => check_smd(error_handler),

        PadAttrib::NonComponent => {
            if drill_size.x > 0 || drill_size.y > 0 {
                report(
                    error_handler,
                    PadCheckCode::UnexpectedHole,
                    format!("{pad_type_string} pad has a hole"),
                );
            }

            let copper_layers = &padlayers_mask & Lset::all_cu_mask();

            if copper_layers.none() {
                report(
                    error_handler,
                    PadCheckCode::NonComponentNoCopper,
                    format!("{pad_type_string} pad has no copper layers"),
                );
                return;
            }

            if copper_layers.count() > 1 {
                report(
                    error_handler,
                    PadCheckCode::MultipleCopperLayers,
                    format!("{pad_type_string} pad has more than one copper layer"),
                );
            }

            let tech = Lset::from_layers(&[
                PcbLayerId::FMask,
                PcbLayerId::BMask,
                PcbLayerId::FPaste,
                PcbLayerId::BPaste,
            ]);

            if (&copper_layers & Lset::external_cu_mask()).none() && (&padlayers_mask & &tech).any()
            {
                report(
                    error_handler,
                    PadCheckCode::NonComponentTechOnInner,
                    format!(
                        "{pad_type_string} pad on an inner layer cannot have mask or paste layers"
                    ),
                );
            }

            check_tech_layer_side(error_handler);

            let parent_fp = pad.parent_footprint(ctx);

            if let Some(fp) = parent_fp
                && fp.stackup_mode == FootprintStackup::ExpandInnerLayers
                && (&copper_layers & Lset::internal_cu_mask()).any()
            {
                report(
                    error_handler,
                    PadCheckCode::NonComponentInnerNeedsCustomStackup,
                    format!(
                        "{pad_type_string} pad on inner layers requires a custom footprint stackup"
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::Vector2I;

    /// `PAD_UTILS::GetPadAttributeString` (pad_utils.cpp).
    #[test]
    fn pad_attribute_strings() {
        assert_eq!(pad_attribute_string(PadAttrib::Pth), "Through-hole");
        assert_eq!(pad_attribute_string(PadAttrib::Smd), "SMD");
        assert_eq!(pad_attribute_string(PadAttrib::Npth), "NPTH");
        assert_eq!(pad_attribute_string(PadAttrib::Conn), "Connector");
        assert_eq!(
            pad_attribute_string(PadAttrib::NonComponent),
            "Non-component"
        );
    }

    /// `PAD_UTILS::RemoveAllDrillsFromPadstack` value-initialises every drill (the padstack
    /// part of `SetAttributeClearsAllDrilling`, qa/tests/pcbnew/test_pad_setters.cpp).
    #[test]
    fn remove_all_drills() {
        let mut p = Padstack::default();
        p.drill_mut().size = Vector2I::new(100, 100);
        p.set_backdrill_size(true, Some(200));
        p.front_post_machining_mut().size = 300;

        remove_all_drills_from_padstack(&mut p);

        assert_eq!(*p.drill(), DrillProps::default());
        assert_eq!(p.drill().shape, PadDrillShape::Undefined);
        assert_eq!(p.drill().start, PcbLayerId::UndefinedLayer);
        assert_eq!(*p.secondary_drill(), DrillProps::default());
        assert_eq!(*p.front_post_machining(), PostMachiningProps::default());
    }
}
