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

//! Port of `pcbnew/drc/drc_test_provider_library_parity.cpp` (package DRC-LP): the library
//! parity test and `FOOTPRINT::FootprintNeedsUpdate` with its helpers.
//!
//! Errors generated:
//! - `DRCE_LIB_FOOTPRINT_ISSUES`
//! - `DRCE_LIB_FOOTPRINT_MISMATCH`
//!
//! The library footprint lives in its own holder board (as loaded by
//! `FOOTPRINT_LIBRARY_ADAPTER::LoadFootprint`); `temp->SetParent( GetBoard() )` gives that
//! holder the copper layer count, enabled layers and layer types of the board. The `dummy`
//! copy of the board footprint whose graphic items are normalised is a footprint of a clone
//! of the board, made once per run when the first footprint is compared.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::Kiid;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, PcbLayerId, layer_name};
use kicad_core::lset::Lset;
use kicad_core::string_utils::unescape_string;
use kicad_core::typeinfo::KicadT;
use kicad_core::units_provider::UnitsProvider;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::{Box2I, FlipDirection};
use kicad_pcb::board::Board;
use kicad_pcb::drc_api::{DRCE_LIB_FOOTPRINT_ISSUES, DRCE_LIB_FOOTPRINT_MISMATCH};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::footprint::compare::{
    footprint_cmp_drawings, footprint_cmp_pads, footprint_cmp_zones,
};
use kicad_pcb::footprint::footprint_attr::{
    FP_BOARD_ONLY, FP_DNP, FP_EXCLUDE_FROM_BOM, FP_EXCLUDE_FROM_POS_FILES, FP_EXCLUDE_FROM_SIM,
    FP_SMD, FP_THROUGH_HOLE,
};
use kicad_pcb::footprint::{Footprint, FootprintStackup};
use kicad_pcb::ids::{FootprintId, ItemId};
use kicad_pcb::item_ref::AnyItemMut;
use kicad_pcb::libraries::FootprintLibraryAdapter;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadDrillShape, PadShape};
use kicad_pcb::pcb_barcode::PcbBarcode;
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::zone::Zone;
use kicad_pcb::{AnyItem, BoardCtx};

use super::run_host::{RunHost, report_aux};
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// `BOARD_ITEM::COMPARE_FLAGS::DRC`.
pub const COMPARE_FLAGS_DRC: i32 = 0x01;
/// `BOARD_ITEM::COMPARE_FLAGS::INSTANCE_TO_INSTANCE`.
pub const COMPARE_FLAGS_INSTANCE_TO_INSTANCE: i32 = 0x02;

/// `EPSILON` of the `TEST_PT` macro (IU).
const EPSILON: i32 = 10;
/// `EPSILON_D` of the `TEST_D` macro.
const EPSILON_D: f64 = 0.000010;

/// The `REPORTER*` of the comparison functions: `None` is "DRC" mode (stop at the first
/// difference), `Some` is "Report" mode (check and report everything).
pub type CompareReporter<'a, 'b> = Option<&'a mut (dyn FnMut(&str) + 'b)>;

/// The state the `TEST*()` macros work on: `diff` and `aReporter`.
struct Cmp<'a, 'b> {
    diff: bool,
    reporter: CompareReporter<'a, 'b>,
}

impl Cmp<'_, '_> {
    /// One `TEST`-family check whose comparison found a difference (`differs`): set `diff`,
    /// report the (non-empty) message. Returns true when the caller must `return diff`
    /// (`diff && !aReporter`).
    fn check(&mut self, differs: bool, msg: impl FnOnce() -> String) -> bool {
        if differs {
            self.diff = true;

            if let Some(reporter) = self.reporter.as_mut() {
                let msg = msg();

                if !msg.is_empty() {
                    reporter(&msg);
                }
            }
        }

        self.diff && self.reporter.is_none()
    }

    /// `aReporter->Report( msg )` when there is a reporter.
    fn report(&mut self, msg: impl FnOnce() -> String) {
        if let Some(reporter) = self.reporter.as_mut() {
            reporter(&msg());
        }
    }
}

/// `TEST( a, b, msg )`.
macro_rules! test {
    ($c:expr, $a:expr, $b:expr, $msg:expr) => {
        if $c.check($a != $b, || $msg) {
            return $c.diff;
        }
    };
}

/// `TEST_PT( a, b, msg )`: a coordinate differs by more than `EPSILON`.
macro_rules! test_pt {
    ($c:expr, $a:expr, $b:expr, $msg:expr) => {{
        if $c.check(pt_differs($a, $b), || $msg) {
            return $c.diff;
        }
    }};
}

/// `TEST_D( a, b, msg )`: differs by more than `EPSILON_D`.
macro_rules! test_d {
    ($c:expr, $a:expr, $b:expr, $msg:expr) => {{
        let (a, b): (f64, f64) = ($a, $b);

        if $c.check((a - b).abs() > EPSILON_D, || $msg) {
            return $c.diff;
        }
    }};
}

/// `g_unitsProvider( pcbIUScale, EDA_UNITS::MM )`.
fn units_provider() -> UnitsProvider<'static> {
    UnitsProvider::new(&PCB_IU_SCALE, EdaUnits::Mm)
}

/// `ITEM_DESC( item )`: `GetItemDescription( &g_unitsProvider, true )`.
fn item_desc(ctx: &BoardCtx<'_>, id: ItemId) -> String {
    ctx.item(id)
        .map(|item| item.item_description(&units_provider(), true))
        .unwrap_or_default()
}

/// `PAD_DESC( pad )`: "Pad %s".
fn pad_desc(pad: &Pad) -> String {
    format!("Pad {}", pad.number())
}

/// `board ? board->GetLayerName( layer ) : LayerName( layer )`.
fn layer_name_of(board: Option<&Board>, layer: PcbLayerId) -> String {
    match board {
        Some(board) => board.layer_name(layer),
        None => layer_name(layer),
    }
}

/// Port of `getBoardNormalizedLayerSet`: the item's layers restricted to the board's enabled
/// layers.
fn board_normalized_layer_set(item_layers: &Lset, board: Option<&Board>) -> Lset {
    let mut lset = item_layers.clone();

    if let Some(board) = board {
        lset &= &board.enabled_layers();
    }

    lset
}

/// Port of `boardLayersMatchWithInnerLayerExpansion`.
fn board_layers_match_with_inner_layer_expansion(
    a_item: &Lset,
    b_lib: &Lset,
    allow_cu_expansion: bool,
) -> bool {
    if !allow_cu_expansion {
        return a_item == b_lib;
    }

    // first test non copper layers, these should exact match
    let non_cu_mask = Lset::all_non_cu_mask();

    if (a_item & &non_cu_mask) != (b_lib & &non_cu_mask) {
        return false;
    }

    // top and bottom copper must match exactly
    if (a_item & Lset::external_cu_mask()) != (b_lib & Lset::external_cu_mask()) {
        return false;
    }

    // extract the inner layers to compare prescence
    let a_inner = &(a_item & Lset::all_cu_mask()) & &!Lset::external_cu_mask();
    let b_inner = &(b_lib & Lset::all_cu_mask()) & &!Lset::external_cu_mask();

    let missing_inner_in_a = &b_inner & &!&a_inner;

    missing_inner_in_a.count() == 0
}

/// `abs( a.x - b.x ) > EPSILON || abs( a.y - b.y ) > EPSILON`.
fn pt_differs(a: Vector2I, b: Vector2I) -> bool {
    (i64::from(a.x) - i64::from(b.x)).abs() > i64::from(EPSILON)
        || (i64::from(a.y) - i64::from(b.y)).abs() > i64::from(EPSILON)
}

/// `BOX2I aRect( start, end - start ); aRect.Normalize();` -> (origin, end).
fn normalized_rect(start: Vector2I, end: Vector2I) -> (Vector2I, Vector2I) {
    let mut rect = Box2I::new(start, end - start);
    rect.normalize();
    (rect.position(), rect.end())
}

/// `( a - b ).EuclideanNorm() > pcbIUScale.mmToIU( 0.0005 )`: the arc mid points moved.
fn arc_mid_moved(a: Vector2I, b: Vector2I) -> bool {
    (a - b).euclidean_norm() > PCB_IU_SCALE.mm_to_iu(0.0005)
}

/// The `POLY` case of `primitiveNeedsUpdate` / `shapeNeedsUpdate` after the vertex count
/// test: the first contour of each polygon compared cyclically within `EPSILON`. A polygon
/// index that `b` lacks (`CPolygon( poly )` out of range, undefined in C++) is a difference.
fn polygons_differ(a: &kicad_geom::ShapePolySet, b: &kicad_geom::ShapePolySet) -> bool {
    for poly in 0..a.c_polygons().len() {
        let a_polygon = a.c_polygon(poly as i32);

        let Some(b_polygon) = b.c_polygons().get(poly) else {
            return true;
        };

        if a_polygon.is_empty()
            || b_polygon.is_empty()
            || !a_polygon[0].compare_geometry(&b_polygon[0], true, EPSILON)
        {
            return true;
        }
    }

    false
}

/// Port of `primitiveNeedsUpdate`: two custom pad primitives (DRC mode, no reporter).
fn primitive_needs_update(
    ctx_a: &BoardCtx<'_>,
    a: &PcbShape,
    ctx_b: &BoardCtx<'_>,
    b: &PcbShape,
) -> bool {
    let mut c = Cmp {
        diff: false,
        reporter: None,
    };

    test!(c, a.shape.shape(), b.shape.shape(), String::new());

    match a.shape.shape() {
        ShapeT::Rectangle => {
            let (a_origin, a_end) = normalized_rect(a.shape.start(), a.shape.end());
            let (b_origin, b_end) = normalized_rect(b.shape.start(), b.shape.end());

            test_pt!(c, a_origin, b_origin, String::new());
            test_pt!(c, a_end, b_end, String::new());
        }
        ShapeT::Segment | ShapeT::Circle => {
            test_pt!(c, a.shape.start(), b.shape.start(), String::new());
            test_pt!(c, a.shape.end(), b.shape.end(), String::new());
        }
        ShapeT::Arc => {
            test_pt!(c, a.shape.start(), b.shape.start(), String::new());
            test_pt!(c, a.shape.end(), b.shape.end(), String::new());

            // Arc center is calculated and so may have round-off errors when parents are
            // differentially rotated.
            if arc_mid_moved(a.shape.arc_mid(), b.shape.arc_mid()) {
                return true;
            }
        }
        ShapeT::Bezier => {
            test_pt!(c, a.shape.start(), b.shape.start(), String::new());
            test_pt!(c, a.shape.end(), b.shape.end(), String::new());
            test_pt!(c, a.shape.bezier_c1(), b.shape.bezier_c1(), String::new());
            test_pt!(c, a.shape.bezier_c2(), b.shape.bezier_c2(), String::new());
        }
        ShapeT::Ellipse | ShapeT::EllipseArc => {
            let (ea, eb) = (a.shape.ellipse(), b.shape.ellipse());

            test_pt!(c, ea.center, eb.center, String::new());
            test!(c, ea.major_radius, eb.major_radius, String::new());
            test!(c, ea.minor_radius, eb.minor_radius, String::new());
            test!(c, ea.rotation, eb.rotation, String::new());

            if a.shape.shape() == ShapeT::EllipseArc {
                test!(c, ea.start_angle, eb.start_angle, String::new());
                test!(c, ea.end_angle, eb.end_angle, String::new());
            }
        }
        ShapeT::Poly => {
            let (pa, pb) = (a.shape.poly_shape(), b.shape.poly_shape());

            test!(c, pa.total_vertices(), pb.total_vertices(), String::new());

            if polygons_differ(pa, pb) {
                c.diff = true;
                return c.diff;
            }
        }
        _ => {
            // UNIMPLEMENTED_FOR( a->SHAPE_T_asString() ): a log message only
        }
    }

    test!(c, a.stroke(ctx_a), b.stroke(ctx_b), String::new());
    test!(c, a.shape.fill_mode(), b.shape.fill_mode(), String::new());

    c.diff
}

/// Port of `padHasOverrides` (report mode only).
fn pad_has_overrides(a: &Pad, b: &Pad, reporter: &mut dyn FnMut(&str)) -> bool {
    let mut diff = false;
    let mut report = |s: &str| reporter(&s.replace("%s", &pad_desc(a)));

    if a.local_clearance().is_some() && a.local_clearance() != b.local_clearance() {
        diff = true;
        report("%s has clearance override.");
    }

    if a.local_solder_mask_margin().is_some()
        && a.local_solder_mask_margin() != b.local_solder_mask_margin()
    {
        diff = true;
        report("%s has solder mask expansion override.");
    }

    if a.local_solder_paste_margin().is_some()
        && a.local_solder_paste_margin() != b.local_solder_paste_margin()
    {
        diff = true;
        report("%s has solder paste clearance override.");
    }

    if a.local_solder_paste_margin_ratio().is_some()
        && a.local_solder_paste_margin_ratio() != b.local_solder_paste_margin_ratio()
    {
        diff = true;
        report("%s has solder paste clearance override.");
    }

    // The raw stored values (ZONE_CONNECTION compared as the enum's integers)
    if a.local_zone_connection_raw() != -1
        && a.local_zone_connection_raw() != b.local_zone_connection_raw()
    {
        diff = true;
        report("%s has zone connection override.");
    }

    if a.lib_padstack().thermal_gap(PcbLayerId::FCu).is_some() && a.thermal_gap() != b.thermal_gap()
    {
        diff = true;
        report("%s has thermal relief gap override.");
    }

    if a.local_thermal_spoke_width_override().is_some()
        && a.local_thermal_spoke_width_override() != b.local_thermal_spoke_width_override()
    {
        diff = true;
        report("%s has thermal relief spoke width override.");
    }

    if a.thermal_spoke_angle() != b.thermal_spoke_angle() {
        diff = true;
        report("%s has thermal relief spoke angle override.");
    }

    if a.custom_shape_in_zone_opt() != b.custom_shape_in_zone_opt() {
        diff = true;
        report("%s has zone knockout setting override.");
    }

    diff
}

/// Port of `padOrientationDiffers`: a half turn leaves a pad as it was only when the pad is
/// symmetric about its own centre, a circle ignores any turn, so a flip that lands on the
/// other of the two angles has not changed such a pad.
fn pad_orientation_differs(
    ctx_a: &BoardCtx<'_>,
    a: &Pad,
    ctx_b: &BoardCtx<'_>,
    b_lib: &Pad,
) -> bool {
    let a_deg = a.fp_relative_orientation(ctx_a).normalized().as_degrees();
    let b_deg = b_lib
        .fp_relative_orientation(ctx_b)
        .normalized()
        .as_degrees();
    let mut delta = (a_deg - b_deg).abs() % 360.0 /* std::fmod */;

    if delta > 180.0 {
        delta = 360.0 - delta;
    }

    if delta <= EPSILON_D {
        return false;
    }

    if a.lib_padstack().mode() != b_lib.lib_padstack().mode()
        || a.lib_padstack().drill().shape != b_lib.lib_padstack().drill().shape
    {
        return true;
    }

    // a slot turns with the pad even when the copper does not
    let round_drill = |pad: &Pad, ctx: &BoardCtx<'_>| {
        let size = pad.drill_size(ctx);
        pad.drill_shape() == PadDrillShape::Circle || size.x == size.y
    };

    let mut circular = round_drill(a, ctx_a) && round_drill(b_lib, ctx_b);
    let mut half_turn_symmetric = true;

    for layer in a.lib_padstack().unique_layers() {
        let shape = a.shape(layer);

        if shape != b_lib.shape(layer) {
            return true;
        }

        circular &= shape == PadShape::Circle;
        half_turn_symmetric &= matches!(
            shape,
            PadShape::Circle | PadShape::Rectangle | PadShape::Oval | PadShape::Roundrect
        );

        // an off-centre shape or a skewed one moves when the pad turns
        let zero = Vector2I::new(0, 0);

        if a.offset(ctx_a, layer) != zero
            || b_lib.offset(ctx_b, layer) != zero
            || a.delta(layer) != zero
            || b_lib.delta(layer) != zero
        {
            return true;
        }
    }

    !circular && !(half_turn_symmetric && (delta - 180.0).abs() <= EPSILON_D)
}

/// Port of `padNeedsUpdate`.
fn pad_needs_update(
    ctx_a: &BoardCtx<'_>,
    a: &Pad,
    ctx_b: &BoardCtx<'_>,
    b_lib: &Pad,
    reporter: CompareReporter<'_, '_>,
) -> bool {
    let mut c = Cmp {
        diff: false,
        reporter,
    };

    test!(
        c,
        a.pad_to_die_length(),
        b_lib.pad_to_die_length(),
        format!("{} pad to die length differs.", pad_desc(a))
    );
    test_pt!(
        c,
        a.fp_relative_position(ctx_a),
        b_lib.fp_relative_position(ctx_b),
        format!("{} position differs.", pad_desc(a))
    );

    test!(
        c,
        a.number(),
        b_lib.number(),
        format!("{} has different numbers.", pad_desc(a))
    );

    // These are assigned from the schematic and not from the library
    // TEST( a->GetPinFunction(), b->GetPinFunction() );
    // TEST( a->GetPinType(), b->GetPinType() );

    let mut layer_settings_differ = a.remove_unconnected() != b_lib.remove_unconnected();

    // NB: KeepTopBottom is undefined if RemoveUnconnected is NOT set.
    if a.remove_unconnected() {
        layer_settings_differ |= a.keep_top_bottom() != b_lib.keep_top_bottom();
    }

    let parent_fp = a.parent_footprint(ctx_a);
    let allow_expansion =
        parent_fp.is_some_and(|fp| fp.stackup_mode == FootprintStackup::ExpandInnerLayers);

    let board = Some(ctx_a.board);

    if layer_settings_differ
        || !board_layers_match_with_inner_layer_expansion(
            &board_normalized_layer_set(a.lib_padstack().layer_set(), board),
            &board_normalized_layer_set(b_lib.lib_padstack().layer_set(), board),
            allow_expansion,
        )
    {
        c.diff = true;

        if c.reporter.is_some() {
            c.report(|| format!("{} layers differ.", pad_desc(a)));
        } else {
            return true;
        }
    }

    test!(
        c,
        a.attribute(),
        b_lib.attribute(),
        format!("{} pad type differs.", pad_desc(a))
    );
    test!(
        c,
        a.property(),
        b_lib.property(),
        format!("{} fabrication property differs.", pad_desc(a))
    );

    // The pad orientation, for historical reasons is the pad rotation + parent rotation.
    if pad_orientation_differs(ctx_a, a, ctx_b, b_lib) {
        c.diff = true;

        if c.reporter.is_some() {
            c.report(|| format!("{} orientation differs.", pad_desc(a)));
        } else {
            return true;
        }
    }

    let layers = a.lib_padstack().unique_layers();

    for layer in layers {
        let layer_name = layer_name_of(board, layer);

        test!(
            c,
            a.shape(layer),
            b_lib.shape(layer),
            format!(
                "{} pad shape type differs on layer {}.",
                pad_desc(a),
                layer_name
            )
        );

        test!(
            c,
            a.size(ctx_a, layer),
            b_lib.size(ctx_b, layer),
            format!("{} size differs on layer {}.", pad_desc(a), layer_name)
        );

        test!(
            c,
            a.delta(layer),
            b_lib.delta(layer),
            format!(
                "{} trapezoid delta differs on layer {}.",
                pad_desc(a),
                layer_name
            )
        );

        if a.shape(layer) == PadShape::Roundrect || a.shape(layer) == PadShape::ChamferedRect {
            test_d!(
                c,
                a.round_rect_radius_ratio(layer),
                b_lib.round_rect_radius_ratio(layer),
                format!(
                    "{} rounded corners differ on layer {}.",
                    pad_desc(a),
                    layer_name
                )
            );
        }

        if a.shape(layer) == PadShape::ChamferedRect {
            test_d!(
                c,
                a.chamfer_rect_ratio(layer),
                b_lib.chamfer_rect_ratio(layer),
                format!(
                    "{} chamfered corner sizes differ on layer {}.",
                    pad_desc(a),
                    layer_name
                )
            );

            test!(
                c,
                a.chamfer_positions(layer),
                b_lib.chamfer_positions(layer),
                format!(
                    "{} chamfered corners differ on layer {}.",
                    pad_desc(a),
                    layer_name
                )
            );
        }

        test_pt!(
            c,
            a.offset(ctx_a, layer),
            b_lib.offset(ctx_b, layer),
            format!(
                "{} shape offset from hole differs on layer {}.",
                pad_desc(a),
                layer_name
            )
        );
    }

    test!(
        c,
        a.drill_shape(),
        b_lib.drill_shape(),
        format!("{} drill shape differs.", pad_desc(a))
    );
    test!(
        c,
        a.drill_size(ctx_a),
        b_lib.drill_size(ctx_b),
        format!("{} drill size differs.", pad_desc(a))
    );

    // Clearance and zone connection overrides are as likely to be set at the board level as
    // in the library.
    //
    // If we ignore them and someone *does* change one of them in the library, then stale
    // footprints won't be caught.
    //
    // On the other hand, if we report them then boards that override at the board level are
    // going to be VERY noisy.
    //
    // So we just do it when we have a reporter.
    if let Some(reporter) = c.reporter.as_deref_mut()
        && pad_has_overrides(a, b_lib, reporter)
    {
        c.diff = true;
    }

    let mut primitives_differ = false;
    let mut first_differing_layer = PcbLayerId::UndefinedLayer;

    a.lib_padstack().for_each_unique_layer(|layer| {
        let (pa, pb) = (
            a.lib_padstack().primitives(layer),
            b_lib.lib_padstack().primitives(layer),
        );

        if pa.len() != pb.len() {
            primitives_differ = true;
        } else {
            for ii in 0..pa.len() {
                if primitive_needs_update(ctx_a, &pa[ii], ctx_b, &pb[ii]) {
                    primitives_differ = true;
                    break;
                }
            }
        }

        if primitives_differ && first_differing_layer == PcbLayerId::UndefinedLayer {
            first_differing_layer = layer;
        }
    });

    if primitives_differ {
        c.diff = true;
        let layer_name = layer_name_of(board, first_differing_layer);

        if c.reporter.is_some() {
            c.report(|| {
                format!(
                    "{} shape primitives differ on layer {}.",
                    pad_desc(a),
                    layer_name
                )
            });
        } else {
            return true;
        }
    }

    c.diff
}

/// Port of `barcodeNeedsUpdate` (no reporter).
fn barcode_needs_update(
    ctx_a: &BoardCtx<'_>,
    curr: &PcbBarcode,
    ctx_b: &BoardCtx<'_>,
    reference: &PcbBarcode,
) -> bool {
    let mut c = Cmp {
        diff: false,
        reporter: None,
    };

    test!(c, curr.text(), reference.text(), String::new());
    test_pt!(
        c,
        curr.position(ctx_a),
        reference.position(ctx_b),
        String::new()
    );
    test!(c, curr.width, reference.width, String::new());
    test!(c, curr.height, reference.height, String::new());
    test!(
        c,
        curr.text_size(ctx_a),
        reference.text_size(ctx_b),
        String::new()
    );
    test!(c, curr.kind as i32, reference.kind as i32, String::new());
    test!(
        c,
        curr.error_correction as i32,
        reference.error_correction as i32,
        String::new()
    );

    c.diff
}

/// Port of `shapeNeedsUpdate`: `curr_shape` and `ref_shape` are expected to be normalized,
/// for a more reliable test.
fn shape_needs_update(
    ctx_a: &BoardCtx<'_>,
    curr: &PcbShape,
    ctx_b: &BoardCtx<'_>,
    reference: &PcbShape,
) -> bool {
    use kicad_pcb::traits::BoardItem;

    let mut c = Cmp {
        diff: false,
        reporter: None,
    };

    test!(
        c,
        curr.shape.shape(),
        reference.shape.shape(),
        String::new()
    );

    match curr.shape.shape() {
        ShapeT::Rectangle => {
            let (a_origin, a_end) = normalized_rect(curr.lib_start(), curr.lib_end());
            let (b_origin, b_end) = normalized_rect(reference.lib_start(), reference.lib_end());

            test_pt!(c, a_origin, b_origin, String::new());
            test_pt!(c, a_end, b_end, String::new());
        }
        ShapeT::Segment | ShapeT::Circle => {
            test_pt!(c, curr.lib_start(), reference.lib_start(), String::new());
            test_pt!(c, curr.lib_end(), reference.lib_end(), String::new());
        }
        ShapeT::Arc => {
            test_pt!(c, curr.lib_start(), reference.lib_start(), String::new());
            test_pt!(c, curr.lib_end(), reference.lib_end(), String::new());

            if arc_mid_moved(
                curr.library_arc_mid(ctx_a),
                reference.library_arc_mid(ctx_b),
            ) {
                return true;
            }
        }
        ShapeT::Bezier => {
            test_pt!(c, curr.lib_start(), reference.lib_start(), String::new());
            test_pt!(c, curr.lib_end(), reference.lib_end(), String::new());
            test_pt!(
                c,
                curr.library_bezier_c1(ctx_a),
                reference.library_bezier_c1(ctx_b),
                String::new()
            );
            test_pt!(
                c,
                curr.library_bezier_c2(ctx_a),
                reference.library_bezier_c2(ctx_b),
                String::new()
            );
        }
        ShapeT::Ellipse | ShapeT::EllipseArc => {
            let (ea, eb) = (curr.shape.ellipse(), reference.shape.ellipse());

            test_pt!(c, ea.center, eb.center, String::new());
            test!(c, ea.major_radius, eb.major_radius, String::new());
            test!(c, ea.minor_radius, eb.minor_radius, String::new());
            test!(c, ea.rotation, eb.rotation, String::new());

            if curr.shape.shape() == ShapeT::EllipseArc {
                test!(c, ea.start_angle, eb.start_angle, String::new());
                test!(c, ea.end_angle, eb.end_angle, String::new());
            }
        }
        ShapeT::Poly => {
            let a_lib = curr.library_poly_shape(ctx_a);
            let b_lib = reference.library_poly_shape(ctx_b);

            test!(
                c,
                a_lib.total_vertices(),
                b_lib.total_vertices(),
                String::new()
            );

            if polygons_differ(&a_lib, &b_lib) {
                c.diff = true;
                return c.diff;
            }
        }
        _ => {
            // UNIMPLEMENTED_FOR( curr_shape.SHAPE_T_asString() ): a log message only
        }
    }

    if curr.is_on_copper_layer(ctx_a) {
        test!(
            c,
            curr.stroke(ctx_a),
            reference.stroke(ctx_b),
            String::new()
        );
    }

    test!(
        c,
        curr.shape.fill_mode(),
        reference.shape.fill_mode(),
        String::new()
    );

    test!(c, curr.layer(ctx_a), reference.layer(ctx_b), String::new());

    c.diff
}

/// Port of `zoneNeedsUpdate`.
fn zone_needs_update(
    ctx_a: &BoardCtx<'_>,
    a_id: ItemId,
    a: &Zone,
    ctx_b: &BoardCtx<'_>,
    b: &Zone,
    reporter: CompareReporter<'_, '_>,
) -> bool {
    let mut c = Cmp {
        diff: false,
        reporter,
    };
    let desc = || item_desc(ctx_a, a_id);

    test!(
        c,
        a.corner_smoothing_type,
        b.corner_smoothing_type,
        format!("{} corner smoothing setting differs.", desc())
    );
    test!(
        c,
        a.corner_radius,
        b.corner_radius,
        format!("{} corner smoothing radius differs.", desc())
    );
    test!(
        c,
        a.zone_name(),
        b.zone_name(),
        format!("{} name differs.", desc())
    );
    test!(
        c,
        a.assigned_priority(),
        b.assigned_priority(),
        format!("{} priority differs.", desc())
    );

    test!(
        c,
        a.is_rule_area(),
        b.is_rule_area(),
        format!("{} keep-out property differs.", desc())
    );
    test!(
        c,
        a.do_not_allow_zone_fills,
        b.do_not_allow_zone_fills,
        format!("{} keep out zone fill setting differs.", desc())
    );
    test!(
        c,
        a.do_not_allow_footprints,
        b.do_not_allow_footprints,
        format!("{} keep out footprints setting differs.", desc())
    );
    test!(
        c,
        a.do_not_allow_pads,
        b.do_not_allow_pads,
        format!("{} keep out pads setting differs.", desc())
    );
    test!(
        c,
        a.do_not_allow_tracks,
        b.do_not_allow_tracks,
        format!("{} keep out tracks setting differs.", desc())
    );
    test!(
        c,
        a.do_not_allow_vias,
        b.do_not_allow_vias,
        format!("{} keep out vias setting differs.", desc())
    );

    // In1_Cu is used to indicate whether inner layer expansion is allowed on rulesets
    // Kind of annoying footprint pads use both in1_cu and have an stackup mode setting
    let b_layers = b.layer_set(ctx_b);
    let inner_layer_expansion_allowed = b_layers.contains(PcbLayerId::In1Cu);

    if !board_layers_match_with_inner_layer_expansion(
        &a.layer_set(ctx_a),
        &board_normalized_layer_set(&b_layers, Some(ctx_a.board)),
        inner_layer_expansion_allowed,
    ) {
        c.diff = true;

        if c.reporter.is_some() {
            c.report(|| format!("{} layers differ.", desc()));
        } else {
            return true;
        }
    }

    test!(
        c,
        a.pad_connection() as i32,
        b.pad_connection() as i32,
        format!("{} pad connection property differs.", desc())
    );
    test!(
        c,
        a.local_clearance(),
        b.local_clearance(),
        format!("{} local clearance differs.", desc())
    );
    test!(
        c,
        a.thermal_relief_gap(),
        b.thermal_relief_gap(),
        format!("{} thermal relief gap differs.", desc())
    );
    test!(
        c,
        a.thermal_relief_spoke_width(),
        b.thermal_relief_spoke_width(),
        format!("{} thermal relief spoke width differs.", desc())
    );

    test!(
        c,
        a.min_thickness(),
        b.min_thickness(),
        format!("{} min thickness differs.", desc())
    );

    test!(
        c,
        a.island_removal_mode,
        b.island_removal_mode,
        format!("{} remove islands setting differs.", desc())
    );
    test!(
        c,
        a.min_island_area,
        b.min_island_area,
        format!("{} minimum island size setting differs.", desc())
    );

    test!(
        c,
        a.fill_mode(),
        b.fill_mode(),
        format!("{} fill type differs.", desc())
    );
    test!(
        c,
        a.hatch_thickness,
        b.hatch_thickness,
        format!("{} hatch width differs.", desc())
    );
    test!(
        c,
        a.hatch_gap,
        b.hatch_gap,
        format!("{} hatch gap differs.", desc())
    );
    test_d!(
        c,
        a.hatch_orientation().as_degrees(),
        b.hatch_orientation().as_degrees(),
        format!("{} hatch orientation differs.", desc())
    );
    test!(
        c,
        a.hatch_smoothing_level,
        b.hatch_smoothing_level,
        format!("{} hatch smoothing level differs.", desc())
    );
    test!(
        c,
        a.hatch_smoothing_value,
        b.hatch_smoothing_value,
        format!("{} hatch smoothing amount differs.", desc())
    );
    test!(
        c,
        a.hatch_hole_min_area,
        b.hatch_hole_min_area,
        format!("{} minimum hatch hole setting differs.", desc())
    );

    // This is just a display property
    // TEST( a->GetHatchBorderAlgorithm(), b->GetHatchBorderAlgorithm() );

    let a_lib_outline = a.library_outline();
    let b_lib_outline = b.library_outline();

    test!(
        c,
        a_lib_outline.total_vertices(),
        b_lib_outline.total_vertices(),
        format!("{} outline corner count differs.", desc())
    );

    let mut corners_differ = a_lib_outline.outline_count() != b_lib_outline.outline_count();

    let mut poly = 0;

    while !corners_differ && poly < a_lib_outline.outline_count() {
        let a_polygon = a_lib_outline.c_polygon(poly);
        let b_polygon = b_lib_outline.c_polygon(poly);

        if a_polygon.len() != b_polygon.len() {
            corners_differ = true;
            break;
        }

        for contour in 0..a_polygon.len() {
            if corners_differ {
                break;
            }

            if !a_polygon[contour].compare_geometry(&b_polygon[contour], true, EPSILON) {
                corners_differ = true;
            }
        }

        poly += 1;
    }

    if corners_differ {
        c.diff = true;
        c.report(|| format!("{} corners differ.", desc()));
    }

    c.diff
}

/// Port of `stackupNeedsUpdate`: compare the stackup related settings of two footprints;
/// true if they differ.
fn stackup_needs_update(
    ctx_a: &BoardCtx<'_>,
    a: &Footprint,
    ctx_b: &BoardCtx<'_>,
    b: &Footprint,
    reporter: CompareReporter<'_, '_>,
) -> bool {
    let mut c = Cmp {
        diff: false,
        reporter,
    };

    test!(
        c,
        a.stackup_mode,
        b.stackup_mode,
        "Footprint stackup mode differs.".to_string()
    );

    let a_layers = kicad_pcb::board_item::layer_set(a, ctx_a);
    let b_layers = kicad_pcb::board_item::layer_set(b, ctx_b);

    test!(
        c,
        a_layers,
        b_layers,
        "Footprint layers differ.".to_string()
    );

    c.diff
}

/// Port of `footprintVsBoardStackup`: report board->footprint stackup differences. This is
/// not necessarily a comparison failure, but may be useful information for the user to see.
fn footprint_vs_board_stackup(
    fp: &Footprint,
    board: &Board,
    reporter: CompareReporter<'_, '_>,
) -> bool {
    if fp.stackup_mode == FootprintStackup::ExpandInnerLayers {
        return false;
    }

    let mut reporter = reporter;

    // Filter only layers that can differ between footprint and board
    let fp_layers = &fp.stackup_layers;
    let brd_layers =
        &board.enabled_layers() & &(Lset::all_cu_mask() | &Lset::user_defined_layers_mask(45));

    let mut mismatch = false;

    // Any layer in the FP and not on the board is flagged
    let only_in_fp = fp_layers & &!&brd_layers;

    if only_in_fp.count() > 0 {
        mismatch = true;

        if let Some(r) = reporter.as_mut() {
            r(&format!(
                "Footprint has {} layers not on board: {}",
                only_in_fp.count(),
                kicad_pcb::board::layers::accumulate_names(&only_in_fp.seq(), Some(board))
            ));
        }
    }

    // Only look at copper layers here: user layers on the board and not in the FP is normal
    let cu_only_in_board = &(&brd_layers & &!fp_layers) & Lset::all_cu_mask();

    if cu_only_in_board.count() > 0 {
        mismatch = true;

        if let Some(r) = reporter.as_mut() {
            r(&format!(
                "Board has {} copper layers not in footprint: {}",
                cu_only_in_board.count(),
                kicad_pcb::board::layers::accumulate_names(&cu_only_in_board.seq(), Some(board))
            ));
        }
    }

    mismatch
}

/// `temp->SetParent( GetBoard() )`: what the library footprint's mutators read through
/// `GetBoard()` (copper layer count, enabled layers, layer types).
fn adopt_board_layers(holder: &mut Board, board: &Board) {
    holder.set_copper_layer_count(board.copper_layer_count());
    holder.set_enabled_layers(board.enabled_layers());

    for layer in 0..PCB_LAYER_ID_COUNT {
        let layer = PcbLayerId(layer);
        let layer_type = board.layer_type(layer);

        if holder.layer_type(layer) != layer_type {
            holder.set_layer_type(layer, layer_type);
        }
    }
}

/// `for( BOARD_ITEM* item : fp->GraphicalItems() ) item->NormalizeForCompare();` on `board`.
fn normalize_graphical_items(board: &mut Board, fp: FootprintId) {
    let ids: Vec<ItemId> = board.footprint_drawings(fp).map(|(id, _)| id).collect();

    for id in ids {
        board.modify(id, |item, env| {
            // BOARD_ITEM::NormalizeForCompare() is Normalize(), a no-op but for shapes and
            // tables (whose cells the comparison does not read)
            if let AnyItemMut::Shape(shape) = item {
                shape.normalize_for_compare_env(env);
            }
        });
    }
}

/// The items of `GraphicalItems()` of type `kind` ordered as a
/// `std::set<BOARD_ITEM*, FOOTPRINT::cmp_drawings>`.
fn sorted_drawings(ctx: &BoardCtx<'_>, fp: FootprintId, kind: KicadT) -> Vec<ItemId> {
    let mut ids: Vec<ItemId> = ctx
        .board
        .footprint_drawings(fp)
        .map(|(id, _)| id)
        .filter(|&id| ctx.item(id).is_some_and(|item| item.type_() == kind))
        .collect();

    ids.sort_by(|&a, &b| footprint_cmp_drawings(ctx, a, b));
    ids
}

/// Port of `FOOTPRINT::FootprintNeedsUpdate( aLibFP, aCompareFlags, aReporter )`.
///
/// `ctx` / `fp_id` are the board footprint (`this`), `lib` the holder board of the library
/// footprint (consumed: it becomes the `temp` clone), `dummy_board` a clone of `ctx.board`
/// in which the footprint's graphic items are normalised (the `dummy` copy).
pub fn footprint_needs_update(
    ctx: &BoardCtx<'_>,
    fp_id: FootprintId,
    lib: Board,
    dummy_board: &mut Board,
    compare_flags: i32,
    reporter: CompareReporter<'_, '_>,
) -> bool {
    let board = ctx.board;
    let Some(this) = board.footprint(fp_id) else {
        return false;
    };

    let mut temp = lib;
    let Some(temp_fp) = temp.footprints().next().map(|(id, _)| id) else {
        return false;
    };

    adopt_board_layers(&mut temp, board);

    if compare_flags & COMPARE_FLAGS_INSTANCE_TO_INSTANCE == 0 {
        if this.is_flipped() != temp.footprint(temp_fp).is_some_and(Footprint::is_flipped) {
            temp.footprint_flip(temp_fp, Vector2I::new(0, 0), FlipDirection::TopBottom);
        }

        // Set rotation before trying to compare footprints, especially when rotation is not
        // multiple of 90 deg (some shapes like rectangles can be converted to polygons).
        // Moreover, NormalizeForCompare needs correct orientation to normalize some shapes
        // like rectangles
        let orientation = this.orientation();

        if temp.footprint(temp_fp).map(Footprint::orientation) != Some(orientation) {
            temp.footprint_set_orientation(temp_fp, orientation);
        }
    }

    normalize_graphical_items(&mut temp, temp_fp);

    let lib_ctx = BoardCtx::bare(&temp);
    let lib_fp = temp.footprint(temp_fp).expect("library footprint");

    let mut c = Cmp {
        diff: false,
        reporter,
    };

    // These checks don't set off errors, they're just informational
    footprint_vs_board_stackup(this, board, c.reporter.as_deref_mut());

    let attrs = this.attributes();
    let lib_attrs = lib_fp.attributes();
    let test_attr = |attr: i32| (attrs & attr) != (lib_attrs & attr);

    if c.check(test_attr(FP_THROUGH_HOLE | FP_SMD), || {
        "Footprint types differ.".to_string()
    }) {
        return c.diff;
    }

    test!(
        c,
        this.allow_solder_mask_bridges(),
        lib_fp.allow_solder_mask_bridges(),
        "'Allow bridged solder mask apertures between pads' settings differ.".to_string()
    );

    if compare_flags & COMPARE_FLAGS_DRC == 0 {
        // These tests are skipped for DRC: they are presumed to relate to a given design.
        for (attr, name) in [
            (FP_BOARD_ONLY, "Not in schematic"),
            (FP_EXCLUDE_FROM_POS_FILES, "Exclude from position files"),
            (FP_EXCLUDE_FROM_BOM, "Exclude from bill of materials"),
            (FP_EXCLUDE_FROM_SIM, "Exclude from simulation"),
            (FP_DNP, "Do not populate"),
        ] {
            if c.check(test_attr(attr), || format!("'{name}' settings differ.")) {
                return c.diff;
            }
        }
    }

    if stackup_needs_update(ctx, this, &lib_ctx, lib_fp, c.reporter.as_deref_mut()) {
        c.diff = true;
        c.report(|| "Footprint stackup differs.".to_string());
    }

    // Clearance and zone connection overrides are as likely to be set at the board level as
    // in the library. ... For report them as different, but we DON'T generate DRC errors on
    // them.
    if compare_flags & COMPARE_FLAGS_DRC == 0 {
        if this.local_clearance().is_some() && this.local_clearance() != lib_fp.local_clearance() {
            c.diff = true;
            c.report(|| "Pad clearance overridden.".to_string());
        }

        if this.solder_mask_margin.is_some() && this.solder_mask_margin != lib_fp.solder_mask_margin
        {
            c.diff = true;
            c.report(|| "Solder mask expansion overridden.".to_string());
        }

        if this.solder_paste_margin.is_some()
            && this.solder_paste_margin != lib_fp.solder_paste_margin
        {
            c.diff = true;
            c.report(|| "Solder paste absolute clearance overridden.".to_string());
        }

        if this.solder_paste_margin_ratio.is_some()
            && this.solder_paste_margin_ratio != lib_fp.solder_paste_margin_ratio
        {
            c.diff = true;
            c.report(|| "Solder paste relative clearance overridden.".to_string());
        }

        // ZONE_CONNECTION::INHERITED is -1
        if this.local_zone_connection_raw() != -1
            && this.local_zone_connection_raw() != lib_fp.local_zone_connection_raw()
        {
            c.diff = true;
            c.report(|| "Zone connection overridden.".to_string());
        }
    }

    test!(
        c,
        this.net_tie_pad_groups.len(),
        lib_fp.net_tie_pad_groups.len(),
        "Net tie pad groups differ.".to_string()
    );

    for ii in 0..this.net_tie_pad_groups.len() {
        test!(
            c,
            this.net_tie_pad_groups.get(ii),
            lib_fp.net_tie_pad_groups.get(ii),
            "Net tie pad groups differ.".to_string()
        );
    }

    // Text items are really problematic.  We don't want to test the reference, but after
    // that it gets messy.
    //
    // What about the value?  Depends on whether or not it's a singleton part.
    //
    // And what about other texts?  They might be added only to instances on the board, or
    // even changed for instances on the board.  Or they might want to be tested for
    // equality.
    //
    // Currently we punt and ignore all the text items.

    // Drawings and pads are also somewhat problematic as there's no guarantee that they'll
    // be in the same order in the two footprints.  Rather than building some sophisticated
    // hashing algorithm we use the footprint sorting functions to attempt to sort them in the
    // same order.

    // However FOOTPRINT::cmp_drawings uses PCB_SHAPE coordinates and other infos, so we have
    // already normalized graphic items in model footprint from library, so we need to
    // normalize graphic items in the footprint to test (*this). So normalize them using a
    // copy of this
    normalize_graphical_items(dummy_board, fp_id);
    let dummy_ctx = BoardCtx::bare(dummy_board);

    // ---- graphic shapes ----
    let a_shapes = sorted_drawings(&dummy_ctx, fp_id, KicadT::PcbShapeT);
    let b_shapes = sorted_drawings(&lib_ctx, temp_fp, KicadT::PcbShapeT);

    if a_shapes.len() != b_shapes.len() {
        c.diff = true;
        c.report(|| "Graphic item count differs.".to_string());
    } else {
        for (&a_id, &b_id) in a_shapes.iter().zip(&b_shapes) {
            // aShapes and bShapes are the tested footprint PCB_SHAPE and the model
            // PCB_SHAPE. These shapes are already normalized.
            let (Some(AnyItem::Shape(curr)), Some(AnyItem::Shape(test))) = (
                dummy_ctx.item(a_id).map(|i| i.any()),
                lib_ctx.item(b_id).map(|i| i.any()),
            ) else {
                continue;
            };

            if shape_needs_update(&dummy_ctx, curr, &lib_ctx, test) {
                c.diff = true;
                c.report(|| format!("{} differs.", item_desc(&dummy_ctx, a_id)));
            }
        }
    }

    // CHECKPOINT
    if c.diff && c.reporter.is_none() {
        return c.diff;
    }

    // ---- barcodes ----
    let a_barcodes = sorted_drawings(&dummy_ctx, fp_id, KicadT::PcbBarcodeT);
    let b_barcodes = sorted_drawings(&lib_ctx, temp_fp, KicadT::PcbBarcodeT);

    if a_barcodes.len() != b_barcodes.len() {
        c.diff = true;
        c.report(|| "Barcode count differs.".to_string());
    } else {
        for (&a_id, &b_id) in a_barcodes.iter().zip(&b_barcodes) {
            let (Some(AnyItem::Barcode(curr)), Some(AnyItem::Barcode(test))) = (
                dummy_ctx.item(a_id).map(|i| i.any()),
                lib_ctx.item(b_id).map(|i| i.any()),
            ) else {
                continue;
            };

            if barcode_needs_update(&dummy_ctx, curr, &lib_ctx, test) {
                c.diff = true;
                c.report(|| format!("{} differs.", item_desc(&dummy_ctx, a_id)));
            }
        }
    }

    // CHECKPOINT
    if c.diff && c.reporter.is_none() {
        return c.diff;
    }

    // ---- pads ----
    let mut a_pads: Vec<ItemId> = board
        .footprint_pads(fp_id)
        .map(|(id, _)| id.into())
        .collect();
    let mut b_lib_pads: Vec<ItemId> = temp
        .footprint_pads(temp_fp)
        .map(|(id, _)| id.into())
        .collect();
    a_pads.sort_by(|&x, &y| footprint_cmp_pads(ctx, x, y));
    b_lib_pads.sort_by(|&x, &y| footprint_cmp_pads(&lib_ctx, x, y));
    dedup_set(&mut a_pads, |&x, &y| footprint_cmp_pads(ctx, x, y));
    dedup_set(&mut b_lib_pads, |&x, &y| footprint_cmp_pads(&lib_ctx, x, y));

    if a_pads.len() != b_lib_pads.len() {
        c.diff = true;
        c.report(|| "Pad count differs.".to_string());
    } else {
        for (&a_id, &b_id) in a_pads.iter().zip(&b_lib_pads) {
            let (Some(a_pad), Some(b_pad)) = (
                ctx.item(a_id).and_then(|i| i.as_pad()),
                lib_ctx.item(b_id).and_then(|i| i.as_pad()),
            ) else {
                continue;
            };

            if pad_needs_update(ctx, a_pad, &lib_ctx, b_pad, c.reporter.as_deref_mut()) {
                c.diff = true;
            } else if let Some(r) = c.reporter.as_deref_mut()
                && pad_has_overrides(a_pad, b_pad, r)
            {
                c.diff = true;
            }
        }
    }

    // CHECKPOINT
    if c.diff && c.reporter.is_none() {
        return c.diff;
    }

    // ---- zones ----
    let mut a_zones: Vec<ItemId> = board
        .footprint_zones(fp_id)
        .map(|(id, _)| id.into())
        .collect();
    let mut b_zones: Vec<ItemId> = temp
        .footprint_zones(temp_fp)
        .map(|(id, _)| id.into())
        .collect();
    a_zones.sort_by(|&x, &y| footprint_cmp_zones(ctx, x, y));
    b_zones.sort_by(|&x, &y| footprint_cmp_zones(&lib_ctx, x, y));

    if a_zones.len() != b_zones.len() {
        c.diff = true;
        c.report(|| "Rule area count differs.".to_string());
    } else {
        for (&a_id, &b_id) in a_zones.iter().zip(&b_zones) {
            let (Some(a_zone), Some(b_zone)) = (
                ctx.item(a_id).and_then(|i| i.as_zone()),
                lib_ctx.item(b_id).and_then(|i| i.as_zone()),
            ) else {
                continue;
            };

            c.diff |= zone_needs_update(
                ctx,
                a_id,
                a_zone,
                &lib_ctx,
                b_zone,
                c.reporter.as_deref_mut(),
            );
        }
    }

    c.diff
}

/// The deduplication of a `std::set` built from a sorted list: elements the comparator
/// finds equivalent are dropped (it orders distinct items strictly, so none are).
fn dedup_set(ids: &mut Vec<ItemId>, cmp: impl Fn(&ItemId, &ItemId) -> Ordering) {
    ids.dedup_by(|a, b| cmp(a, b) == Ordering::Equal);
}

/// Port of `DRC_TEST_PROVIDER_LIBRARY_PARITY`.
pub struct DrcTestProviderLibraryParity<'a> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// `PROJECT_PCB::FootprintLibAdapter( project )` after `AsyncLoad()` +
    /// `BlockUntilLoaded()`; `None` behaves as an adapter without tables.
    pub libraries: Option<&'a FootprintLibraryAdapter>,
}

impl<'a> DrcTestProviderLibraryParity<'a> {
    /// `DRC_TEST_PROVIDER_LIBRARY_PARITY()`: not rule driven.
    pub fn new(libraries: Option<&'a FootprintLibraryAdapter>) -> Self {
        let mut base = DrcTestProviderBase::new("library_parity");
        base.is_rule_driven = false;

        DrcTestProviderLibraryParity { base, libraries }
    }

    /// A violation of `code` with message `msg` on `footprint` at its centre.
    fn report(
        &self,
        host: &mut RunHost<'_, '_>,
        code: i32,
        msg: &str,
        uuid: Option<Kiid>,
        centre: Vector2I,
    ) {
        let mut drc_item = item::create(code).expect("library parity DRC item");
        drc_item.set_error_message(msg);
        drc_item.set_item_ids(uuid.into_iter().collect());

        self.base.report_violation(
            host,
            Arc::new(drc_item),
            centre,
            PcbLayerId::UndefinedLayer.0,
            &no_path,
        );
    }

    /// Port of `DRC_TEST_PROVIDER_LIBRARY_PARITY::Run`: false when the run was cancelled.
    pub fn run(&self, _engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let board = ctx.board;

        if board.project.is_none() {
            report_aux(run, "No project loaded, skipping library parity tests.");
            return true; // Continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Loading footprint library table...")
        {
            return false; // DRC cancelled
        }

        let empty = FootprintLibraryAdapter::default();
        let adapter = self.libraries.unwrap_or(&empty);

        // std::map<LIB_ID, std::shared_ptr<FOOTPRINT>>: only successful loads are cached
        let mut lib_footprint_cache: BTreeMap<String, Board> = BTreeMap::new();
        let mut dummy_board: Option<Board> = None;
        let progress_delta = 250;
        let count = board.footprints().count();

        if !self
            .base
            .report_phase(&mut host, "Checking board footprints against library...")
        {
            return false;
        }

        for (ii, (fp_id, footprint)) in board.footprints().enumerate() {
            if host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_ISSUES)
                && host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_MISMATCH)
            {
                return true; // Continue with other tests
            }

            if !self
                .base
                .report_progress(&mut host, ii, count, progress_delta)
            {
                return false; // DRC cancelled
            }

            let fp_id_lib = footprint.fpid();
            let lib_name = fp_id_lib.lib_nickname();
            let fp_name = fp_id_lib.lib_item_name();

            if lib_name.is_empty() {
                // Not much we can do here
                continue;
            }

            let uuid = Some(footprint.base.uuid);
            let centre = footprint.center(ctx);

            let Some(lib_table_row) = adapter.get_row(lib_name) else {
                if !host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_ISSUES) {
                    let msg = format!(
                        "The current configuration does not include the footprint library '{}'",
                        unescape_string(lib_name)
                    );
                    self.report(&mut host, DRCE_LIB_FOOTPRINT_ISSUES, &msg, uuid, centre);
                }

                continue;
            };

            if !adapter.has_library(lib_name, true) {
                if !host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_ISSUES) {
                    let msg = format!(
                        "The footprint library '{}' is not enabled in the current configuration",
                        unescape_string(lib_name)
                    );
                    self.report(&mut host, DRCE_LIB_FOOTPRINT_ISSUES, &msg, uuid, centre);
                }

                continue;
            } else if !adapter.is_library_loaded(lib_name) {
                if !host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_ISSUES) {
                    let msg = format!(
                        "The footprint library '{}' was not found at '{}'",
                        unescape_string(lib_name),
                        adapter.manager.get_full_uri(lib_table_row, true)
                    );
                    self.report(&mut host, DRCE_LIB_FOOTPRINT_ISSUES, &msg, uuid, centre);
                }

                continue;
            }

            let cache_key = fp_id_lib.format();

            if !lib_footprint_cache.contains_key(&cache_key)
                && let Some(loaded) = adapter.load_footprint(lib_name, fp_name)
            {
                lib_footprint_cache.insert(cache_key.clone(), loaded);
            }

            match lib_footprint_cache.get(&cache_key) {
                None => {
                    if !host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_ISSUES) {
                        let msg =
                            format!("Footprint '{fp_name}' not found in library '{lib_name}'");
                        self.report(&mut host, DRCE_LIB_FOOTPRINT_ISSUES, &msg, uuid, centre);
                    }
                }
                Some(lib_footprint) => {
                    let dummy = dummy_board.get_or_insert_with(|| board.clone());

                    if footprint_needs_update(
                        ctx,
                        fp_id,
                        lib_footprint.clone(),
                        dummy,
                        COMPARE_FLAGS_DRC,
                        None,
                    ) && !host.is_error_limit_exceeded(DRCE_LIB_FOOTPRINT_MISMATCH)
                    {
                        let msg = format!(
                            "Footprint '{fp_name}' does not match copy in library '{lib_name}'"
                        );
                        self.report(&mut host, DRCE_LIB_FOOTPRINT_MISMATCH, &msg, uuid, centre);
                    }
                }
            }
        }

        true
    }
}

impl DrcTestProviderRun for DrcTestProviderLibraryParity<'_> {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(engine, run, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_layer_expansion() {
        let f = Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::FMask]);
        let mut with_inner = f.clone();
        with_inner.set(PcbLayerId::In1Cu);

        assert!(board_layers_match_with_inner_layer_expansion(&f, &f, false));
        assert!(!board_layers_match_with_inner_layer_expansion(
            &f,
            &with_inner,
            false
        ));
        // the library has an inner layer the item lacks
        assert!(!board_layers_match_with_inner_layer_expansion(
            &f,
            &with_inner,
            true
        ));
        // the item has more inner layers than the library
        assert!(board_layers_match_with_inner_layer_expansion(
            &with_inner,
            &f,
            true
        ));
    }

    #[test]
    fn epsilon_compare() {
        assert!(!pt_differs(Vector2I::new(0, 0), Vector2I::new(10, -10)));
        assert!(pt_differs(Vector2I::new(0, 0), Vector2I::new(11, 0)));
    }
}
