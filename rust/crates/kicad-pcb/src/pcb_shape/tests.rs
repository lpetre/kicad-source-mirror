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

//! Tests of package M9 (`PCB_SHAPE`): ports of the shape cases of
//! `qa/tests/pcbnew/test_footprint_transform_sync.cpp` (named per test; the C++ footprint
//! cascades `FOOTPRINT::SetTransformScale` / `SetOrientation`, package M19, are reproduced by
//! writing the fixture footprint's transform and calling the child hook they call), unit
//! tests of the overrides, and the replay of the reference dump `tests/golden/m9_shapes.txt`.

use kicad_core::{KicadT, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{ANGLE_0, Box2I, EdaAngle, FlipDirection, Shape, ShapePolySet, Vector2I};

use super::*;
use crate::board::Board;
use crate::ctx::{BoardCtx, MutEnv};
use crate::eda_shape::{FillT, ShapeT};
use crate::ids::{ItemId, NetCode, ShapeId};
use crate::item::ParentRef;
use crate::test_support::{FrameFixture, board_with_footprint};
use crate::traits::EdaShapeItem;

fn v(x: i32, y: i32) -> Vector2I {
    Vector2I::new(x, y)
}

/// `FOOTPRINT fp( nullptr ); fp.SetPosition( 0, 0 )`: an identity-frame footprint.
fn identity_fp() -> FrameFixture {
    board_with_footprint(TransformTrs::default(), false)
}

/// `new PCB_SHAPE( &fp, aShape )`, configured through the PCB_SHAPE setters with the
/// footprint as parent, then `fp.Add( shape )`.
fn add_fp_shape(
    fx: &mut FrameFixture,
    shape: ShapeT,
    build: impl FnOnce(&mut PcbShape, &MutEnv),
) -> ShapeId {
    let parent = ParentRef::Item(fx.footprint.into());
    let mut s = PcbShape::with_shape(shape);
    s.base.item.parent = parent;
    let env = fx.board.child_env(parent);
    build(&mut s, &env);
    fx.add_child(s, |fp| &mut fp.drawings)
}

/// The child loop of `FOOTPRINT::SetTransformScale` / `SetOrientation` for one shape: the
/// footprint's transform changes, then the child's hook runs.
fn set_fp_transform(fx: &mut FrameFixture, id: ShapeId, xform: TransformTrs) {
    fx.board
        .footprints
        .get_mut(fx.footprint)
        .expect("fixture footprint")
        .transform = xform;
    fx.board
        .modify_shape(id, |s, env| {
            s.on_footprint_rescaled(env, 1.0, 1.0, 1.0, v(0, 0), ANGLE_0)
        })
        .expect("live shape");
}

fn xform(rotate: f64, sx: f64, sy: f64) -> TransformTrs {
    TransformTrs::new(v(0, 0), EdaAngle::from_degrees(rotate), sx, sy)
}

/// `NativeEllipseRebakesUnderUniformScale` (test_footprint_transform_sync.cpp).
#[test]
fn native_ellipse_rebakes_under_uniform_scale() {
    let mut fx = identity_fp();
    let id = add_fp_shape(&mut fx, ShapeT::Ellipse, |s, env| {
        s.set_ellipse_center(env, v(1000000, 0));
        s.set_ellipse_major_radius(env, 500000);
        s.set_ellipse_minor_radius(env, 250000);
        s.set_ellipse_rotation(env, ANGLE_0);
    });

    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 2.0));
    let e = &fx.board[id];

    assert_eq!(e.shape.shape, ShapeT::Ellipse);
    assert_eq!(e.shape.ellipse.center.x, 2000000);
    assert_eq!(e.shape.ellipse.major_radius, 1000000);
    assert_eq!(e.shape.ellipse.minor_radius, 500000);

    // Lib values should be unchanged.
    assert_eq!(e.lib_ellipse_center().x, 1000000);
    assert_eq!(e.lib_ellipse_major_radius(), 500000);
    assert_eq!(e.lib_ellipse_minor_radius(), 250000);
    fx.board.check_invariants().unwrap();
}

/// `PolyShapeScalesWithFootprint`.
#[test]
fn poly_shape_scales_with_footprint() {
    let mut fx = identity_fp();
    let id = add_fp_shape(&mut fx, ShapeT::Poly, |s, env| {
        let mut pts = ShapePolySet::new();
        pts.new_outline();

        for p in [v(0, 0), v(1000000, 0), v(1000000, 1000000), v(0, 1000000)] {
            pts.append_point(p, -1, -1);
        }

        s.set_poly_shape(env, pts);
    });

    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    let out = fx.board[id].shape.poly_shape();

    assert_eq!(out.outline_count(), 1);
    assert_eq!(out.c_outline(0).c_point(1).x, 2000000);
    assert_eq!(out.c_outline(0).c_point(2).x, 2000000);
    assert_eq!(out.c_outline(0).c_point(2).y, 1000000);
}

/// `BezierShapeScalesWithFootprint`.
#[test]
fn bezier_shape_scales_with_footprint() {
    let mut fx = identity_fp();
    let id = add_fp_shape(&mut fx, ShapeT::Bezier, |s, env| {
        s.set_start(env, v(0, 0));
        s.set_bezier_c1(env, v(1000000, 0));
        s.set_bezier_c2(env, v(1000000, 1000000));
        s.set_end(env, v(2000000, 1000000));
        s.shape
            .rebuild_bezier_to_segments_points_list(env.max_error);
    });

    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    let b = &fx.board[id].shape;

    assert_eq!(b.start().x, 0);
    assert_eq!(b.bezier_c1().x, 2000000);
    assert_eq!(b.bezier_c2().x, 2000000);
    assert_eq!(b.bezier_c2().y, 1000000);
    assert_eq!(b.end().x, 4000000);

    // Polyline cache must be rebuilt after scale
    let pts = b.bezier_points();
    assert!(pts.len() >= 2);
    assert!(pts.iter().map(|p| p.x).max().unwrap() > 3000000);
}

fn rect_shape(fx: &mut FrameFixture) -> ShapeId {
    add_fp_shape(fx, ShapeT::Rectangle, |s, env| {
        s.set_start(env, v(-1000000, -500000));
        s.set_end(env, v(1000000, 500000));
    })
}

/// `RectangleSurvivesScaleRotateRoundTrip`.
#[test]
fn rectangle_survives_scale_rotate_round_trip() {
    let mut fx = identity_fp();
    let id = rect_shape(&mut fx);
    let (lib_start0, lib_end0) = (fx.board[id].lib_start(), fx.board[id].lib_end());

    for x in [
        xform(0.0, 2.0, 1.0),
        xform(30.0, 2.0, 1.0),
        xform(30.0, 3.0, 1.0),
        xform(0.0, 3.0, 1.0),
        xform(0.0, 1.0, 1.0),
    ] {
        set_fp_transform(&mut fx, id, x);
    }

    let r = &fx.board[id];
    assert_eq!(r.lib_shape(), ShapeT::Rectangle);
    assert_eq!(r.shape.shape, ShapeT::Rectangle);
    assert_eq!((r.lib_start(), r.lib_end()), (lib_start0, lib_end0));
    assert_eq!((r.shape.start(), r.shape.end()), (lib_start0, lib_end0));
}

/// `RectangleSwapsDimsAt90`.
#[test]
fn rectangle_swaps_dims_at_90() {
    let mut fx = identity_fp();
    let id = rect_shape(&mut fx);
    set_fp_transform(&mut fx, id, xform(90.0, 1.0, 1.0));
    let r = &fx.board[id].shape;

    assert_eq!(r.shape, ShapeT::Rectangle);
    assert_eq!((r.end().x - r.start().x).abs(), 1000000);
    assert_eq!((r.end().y - r.start().y).abs(), 2000000);
}

/// `RectangleMorphsToPolyUnderNonCardinalRotation`.
#[test]
fn rectangle_morphs_to_poly_under_non_cardinal_rotation() {
    let mut fx = identity_fp();
    let id = rect_shape(&mut fx);
    set_fp_transform(&mut fx, id, xform(30.0, 1.0, 1.0));
    let r = &fx.board[id];

    assert_eq!(r.lib_shape(), ShapeT::Rectangle);
    assert_eq!(r.shape.shape, ShapeT::Poly);
    assert_eq!(r.shape.poly_shape().c_outline(0).point_count(), 4);
    assert_eq!(r.lib_start().x, -1000000);
    assert_eq!(r.lib_end().x, 1000000);
}

/// `RectangleFoldedBackFromPolySavesAtItsBoardPosition` (the model part: two 45 degree
/// turns make a polygon whose corners lie on the axes, `Normalize()` folds it back into a
/// rectangle with the same box; the save / reload half is the writer's).
#[test]
fn rectangle_folded_back_from_poly() {
    let env = MutEnv::detached();
    let mut rect = PcbShape::with_shape(ShapeT::Rectangle);
    rect.base.item.parent = ParentRef::Board;
    rect.set_layer(&env, PcbLayerId::FSilkS);
    rect.set_width(&env, 150000);
    rect.set_start(&env, v(100000000, 80000000));
    rect.set_end(&env, v(120000000, 90000000));

    let rot_center = v(105000000, 80000000);
    rect.rotate(&env, rot_center, EdaAngle::from_degrees(45.0));
    assert_eq!(rect.shape.shape, ShapeT::Poly);
    // the virtual SetShape( POLY ) of EDA_SHAPE::rotate reaches PCB_SHAPE::SetShape
    assert_eq!(rect.lib_shape(), ShapeT::Poly);
    rect.rotate(&env, rot_center, EdaAngle::from_degrees(45.0));
    rect.normalize();
    assert_eq!(rect.shape.shape, ShapeT::Rectangle);
    assert_eq!(rect.lib_shape(), ShapeT::Rectangle);

    rect.rotate(&env, rot_center, EdaAngle::from_degrees(90.0));
    rect.normalize();
    assert_eq!(rect.shape.shape, ShapeT::Rectangle);
    assert!(rect.shape.start().x <= rect.shape.end().x);
    assert!(rect.shape.start().y <= rect.shape.end().y);
    assert_eq!(
        (rect.lib_start(), rect.lib_end()),
        (rect.shape.start(), rect.shape.end())
    );
}

fn unit_ellipse(fx: &mut FrameFixture, rotation: f64) -> ShapeId {
    add_fp_shape(fx, ShapeT::Ellipse, |s, env| {
        s.set_ellipse_center(env, v(0, 0));
        s.set_ellipse_major_radius(env, 1000000);
        s.set_ellipse_minor_radius(env, 500000);
        s.set_ellipse_rotation(env, EdaAngle::from_degrees(rotation));
    })
}

/// `NativeEllipseRebakesUnderNonUniformScaleAxisAligned`.
#[test]
fn native_ellipse_rebakes_under_non_uniform_scale_axis_aligned() {
    let mut fx = identity_fp();
    let id = unit_ellipse(&mut fx, 0.0);
    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    let e = &fx.board[id].shape.ellipse;

    assert_eq!(e.major_radius, 2000000);
    assert_eq!(e.minor_radius, 500000);
    assert!(e.rotation.as_degrees().abs() < 1e-6);
}

/// Normalize a rotation to (-90, 90] (an ellipse rotated 180 degrees looks the same).
fn half_turn(mut deg: f64) -> f64 {
    while deg > 90.0 {
        deg -= 180.0;
    }

    while deg <= -90.0 {
        deg += 180.0;
    }

    deg
}

fn close(a: f64, b: f64, percent: f64) -> bool {
    (a - b).abs() <= b.abs() * percent / 100.0
}

/// `NativeEllipseSignUnderNonUniformScaleAndParentRotation`.
#[test]
fn native_ellipse_sign_under_non_uniform_scale_and_parent_rotation() {
    let mut fx = identity_fp();
    let id = unit_ellipse(&mut fx, 0.0);
    set_fp_transform(&mut fx, id, xform(30.0, 1.0, 1.0));
    set_fp_transform(&mut fx, id, xform(30.0, 2.0, 1.0));
    let e = &fx.board[id].shape.ellipse;

    assert!(close(e.major_radius as f64, 2000000.0, 0.1));
    assert!(close(e.minor_radius as f64, 500000.0, 0.1));
    assert!(close(half_turn(e.rotation.as_degrees()), -30.0, 0.1));
}

/// `NativeEllipseContinuousAtUniformLimit`.
#[test]
fn native_ellipse_continuous_at_uniform_limit() {
    let mut fx = identity_fp();
    let id = unit_ellipse(&mut fx, 0.0);
    set_fp_transform(&mut fx, id, xform(30.0, 1.0, 1.0));
    set_fp_transform(&mut fx, id, xform(30.0, 2.0, 2.0));
    let u = fx.board[id].shape.ellipse;
    set_fp_transform(&mut fx, id, xform(30.0, 2.0, 2.1));
    let n = fx.board[id].shape.ellipse;

    assert!(close(n.major_radius as f64, u.major_radius as f64, 1.0));
    assert!(close(n.minor_radius as f64, u.minor_radius as f64, 6.0));
    assert!(half_turn(n.rotation.as_degrees() - u.rotation.as_degrees()).abs() < 1.0);
}

/// `NativeEllipseTessellatesUnderNonUniformScaleRotated`.
#[test]
fn native_ellipse_tessellates_under_non_uniform_scale_rotated() {
    let mut fx = identity_fp();
    let id = unit_ellipse(&mut fx, 45.0);
    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));

    {
        let e = &fx.board[id];
        assert_eq!(e.shape.shape, ShapeT::Poly);
        assert_eq!(e.lib_shape(), ShapeT::Ellipse);
        assert!(e.shape.is_poly_shape_valid());
        assert_eq!(e.shape.poly_shape().outline_count(), 1);
        assert!(e.shape.poly_shape().c_outline(0).point_count() > 8);

        // Expected bbox after Sx=2, Sy=1 on a 1 x 0.5 mm ellipse at 45 deg:
        // ~3.162 mm wide, ~1.581 mm tall.
        let bbox = e.shape.poly_shape().bbox(0);
        assert!(close(bbox.width() as f64, 3162277.0, 1.0));
        assert!(close(bbox.height() as f64, 1581139.0, 1.0));
    }

    // Restore uniform scale, the native ellipse comes back.
    set_fp_transform(&mut fx, id, xform(0.0, 1.0, 1.0));
    let e = &fx.board[id].shape;
    assert_eq!(e.shape, ShapeT::Ellipse);
    assert_eq!(e.ellipse.major_radius, 1000000);
    assert_eq!(e.ellipse.minor_radius, 500000);
}

fn unit_circle(fx: &mut FrameFixture) -> ShapeId {
    add_fp_shape(fx, ShapeT::Circle, |s, env| {
        s.set_start(env, v(0, 0));
        s.set_end(env, v(1000000, 0));
    })
}

/// `NonUniformScalePromotesCircleToEllipse`.
#[test]
fn non_uniform_scale_promotes_circle_to_ellipse() {
    let mut fx = identity_fp();
    let id = unit_circle(&mut fx);
    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    let c = &fx.board[id].shape;

    assert_eq!(c.shape, ShapeT::Ellipse);
    assert_eq!(c.ellipse.major_radius, 2000000);
    assert_eq!(c.ellipse.minor_radius, 1000000);
    assert_eq!(c.ellipse.rotation.as_degrees(), 0.0);
    assert_eq!(c.ellipse.center, v(0, 0));
}

/// `CircleToEllipseRotatesWithParent`.
#[test]
fn circle_to_ellipse_rotates_with_parent() {
    let mut fx = identity_fp();
    let id = unit_circle(&mut fx);
    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    set_fp_transform(&mut fx, id, xform(30.0, 2.0, 1.0));

    // Parent rotated +30 in screen frame so ellipse math rotation = -30 deg.
    assert!(close(
        fx.board[id].shape.ellipse.rotation.as_degrees(),
        -30.0,
        0.001
    ));
}

/// `NonUniformScaleSwapsAxesWhenMinorWouldExceedMajor`.
#[test]
fn non_uniform_scale_swaps_axes_when_minor_would_exceed_major() {
    let mut fx = identity_fp();
    let id = unit_circle(&mut fx);
    set_fp_transform(&mut fx, id, xform(0.0, 1.0, 3.0));
    let c = &fx.board[id].shape;

    assert_eq!(c.shape, ShapeT::Ellipse);
    assert_eq!(c.ellipse.major_radius, 3000000);
    assert_eq!(c.ellipse.minor_radius, 1000000);
    assert_eq!(c.ellipse.rotation.as_degrees(), 90.0);
}

fn quarter_arc(fx: &mut FrameFixture) -> ShapeId {
    add_fp_shape(fx, ShapeT::Arc, |s, env| {
        // Quarter arc on a 1 mm circle at origin, (1,0) -> (0,1).
        s.set_arc_geometry(env, v(1000000, 0), v(707107, 707107), v(0, 1000000));
    })
}

/// `NonUniformScalePromotesArcToEllipseArc`.
#[test]
fn non_uniform_scale_promotes_arc_to_ellipse_arc() {
    let mut fx = identity_fp();
    let id = quarter_arc(&mut fx);
    set_fp_transform(&mut fx, id, xform(0.0, 2.0, 1.0));
    let a = &fx.board[id].shape;

    assert_eq!(a.shape, ShapeT::EllipseArc);
    assert_eq!(a.ellipse.major_radius, 2000000);
    assert_eq!(a.ellipse.minor_radius, 1000000);
    assert_eq!(a.ellipse.rotation.as_degrees(), 0.0);
    assert_eq!(a.ellipse.center, v(0, 0));
    assert!(a.ellipse.start_angle.as_degrees().abs() < 1e-6);
    assert!((a.ellipse.end_angle.as_degrees() - 90.0).abs() < 1e-4);
}

/// `UniformRescaleRestoresArc` and `UniformRescaleRestoresCircle`.
#[test]
fn uniform_rescale_restores_arc_and_circle() {
    let mut fx = identity_fp();
    let arc = quarter_arc(&mut fx);
    set_fp_transform(&mut fx, arc, xform(0.0, 2.0, 1.0));
    assert_eq!(fx.board[arc].shape.shape, ShapeT::EllipseArc);
    set_fp_transform(&mut fx, arc, xform(0.0, 2.0, 2.0));
    assert_eq!(fx.board[arc].shape.shape, ShapeT::Arc);

    let circle = unit_circle(&mut fx);
    set_fp_transform(&mut fx, circle, xform(0.0, 2.0, 1.0));
    assert_eq!(fx.board[circle].shape.shape, ShapeT::Ellipse);
    set_fp_transform(&mut fx, circle, xform(0.0, 2.0, 2.0));
    assert_eq!(fx.board[circle].shape.shape, ShapeT::Circle);
    fx.board.check_invariants().unwrap();
}

/// `PCB_SHAPE::SetWidth` stores the width in lib units: the footprint's linear scale is
/// stripped on write and re-applied by `GetWidth`.
#[test]
fn set_width_strips_the_footprint_scale() {
    let mut fx = board_with_footprint(xform(30.0, 1.5, 0.75), false);
    let id = add_fp_shape(&mut fx, ShapeT::Segment, |s, env| s.set_width(env, 225000));
    let ctx = BoardCtx::bare(&fx.board);
    let s = &fx.board[id];

    assert_eq!(s.shape.stroke.width(), 200000);
    assert_eq!(EdaShapeItem::width(s, &ctx), 225000);
}

/// `PCB_SHAPE::IsOnLayer` / `GetLayerSet` / `SetLayerSet` / `SetLayer` (net reset off
/// copper) / `IsConnected`.
#[test]
fn layers_mask_and_net() {
    let mut board = Board::new();
    let env = board.child_env(ParentRef::Board);
    let mut s = PcbShape::with_shape(ShapeT::Rectangle);
    s.set_layer_set(
        &env,
        Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::BMask]),
    );
    assert_eq!(s.base.item.layer, PcbLayerId::BCu);
    assert!(s.has_solder_mask());
    s.base.net = NetCode(3);

    let id = board.add_shape(s, crate::board::AddMode::Append);
    let ctx = BoardCtx::bare(&board);
    let s = &board[id];

    assert!(s.is_on_layer(&ctx, PcbLayerId::BMask));
    assert!(!s.is_on_layer(&ctx, PcbLayerId::FMask));
    assert_eq!(
        s.layer_set(&ctx),
        Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::BMask])
    );
    assert!(s.is_connected(&ctx));

    let mut off = s.clone();
    off.set_layer(&env, PcbLayerId::BSilkS);
    assert_eq!(off.base.net, NetCode::UNCONNECTED);
    assert_eq!(
        off.layer_set(&ctx),
        Lset::from_layers(&[PcbLayerId::BSilkS])
    );
    assert!(!off.is_on_layer(&ctx, PcbLayerId::BMask));
    assert!(!off.is_connected(&ctx));

    let fx = identity_fp();
    let ctx = BoardCtx::bare(&fx.board);
    let child = &fx.board[fx
        .child(crate::ids::ItemKind::Shape)
        .typed::<PcbShape>()
        .unwrap()];
    // Only board-level copper shapes are connectable
    assert!(!child.is_connected(&ctx));
}

/// `PCB_SHAPE::GetSolderMaskExpansion`: local margin, else the board's value, else 0 (no
/// board); a negative margin is limited to half the width unless the shape is filled.
#[test]
fn solder_mask_expansion() {
    let mut board = Board::new();
    board.design_settings_mut().solder_mask_expansion = 70000;
    let mut s = PcbShape::with_shape(ShapeT::Segment);
    let ctx = BoardCtx::bare(&board);
    s.shape.stroke.set_width(200000);
    assert_eq!(s.solder_mask_expansion(&ctx), 0);

    s.base.item.parent = ParentRef::Board;
    assert_eq!(s.solder_mask_expansion(&ctx), 70000);
    s.set_local_solder_mask_margin(Some(-300000));
    assert_eq!(s.solder_mask_expansion(&ctx), -100000);
    s.shape.set_fill_mode(FillT::FilledShape);
    assert_eq!(s.solder_mask_expansion(&ctx), -300000);
}

/// `PCB_SHAPE::GetEffectiveShape`: the mask layer of a copper shape with solder mask passes
/// a width widened by twice the expansion to `MakeEffectiveShapesWithLineEndings`, but (as in
/// the C++) that width only reaches the line endings: the body shapes come from
/// `makeEffectiveShapes()` / `GetEffectiveWidth()`, so a plain segment keeps its width.
#[test]
fn effective_shape_widens_on_mask() {
    let board = Board::new();
    let ctx = BoardCtx::bare(&board);
    let mut s = PcbShape::with_shape(ShapeT::Segment);
    s.base.item.parent = ParentRef::Board;
    s.shape.set_start(v(0, 0));
    s.shape.set_end(v(1000000, 0));
    s.shape.stroke.set_width(200000);
    s.set_has_solder_mask(true);
    s.set_local_solder_mask_margin(Some(50000));

    let usage = DrcConstraintT::NullConstraint;
    let cu = s.effective_shape(&ctx, PcbLayerId::FCu, Flashing::Default, usage);
    let mask = s.effective_shape(&ctx, PcbLayerId::FMask, Flashing::Default, usage);
    assert_eq!(cu.bbox(0).height(), 200000);
    assert_eq!(mask.bbox(0).height(), 200000);

    // Not on copper: no widening.
    s.base.item.layer = PcbLayerId::FSilkS;
    let silk = s.effective_shape(&ctx, PcbLayerId::FMask, Flashing::Default, usage);
    assert_eq!(silk.bbox(0).height(), 200000);
}

/// `PCB_SHAPE::GetItemDescription` / `GetFriendlyName` (English).
#[test]
fn item_description() {
    let board = Board::new();
    let ctx = BoardCtx::bare(&board);
    let units =
        kicad_core::units_provider::UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
    let mut s = PcbShape::with_shape(ShapeT::Arc);
    s.base.item.parent = ParentRef::Board;
    s.base.item.layer = PcbLayerId::FSilkS;
    assert_eq!(
        s.item_description(&ctx, &units, true),
        "Arc on F.Silkscreen"
    );

    s.base.item.layer = PcbLayerId::FCu;
    assert_eq!(
        s.item_description(&ctx, &units, true),
        "Arc [<no net>] on F.Cu"
    );

    s.shape.shape = ShapeT::Segment;
    s.shape.proxy_item = true;
    assert_eq!(s.friendly_name(&ctx), "Thermal Spoke Template");
}

/// `PCB_SHAPE::SetIsProxyItem`: a segment gets the thermal spoke width, other shapes 1 nm,
/// leaving proxy mode restores the default line width.
#[test]
fn set_is_proxy_item() {
    let mut s = PcbShape::with_shape(ShapeT::Segment);
    s.set_is_proxy_item(true);
    assert!(s.is_proxy_item());
    assert_eq!(s.shape.stroke.width(), 500000);
    s.set_is_proxy_item(false);
    assert_eq!(s.shape.stroke.width(), 100000);

    let mut r = PcbShape::with_shape(ShapeT::Rectangle);
    r.set_is_proxy_item(true);
    assert_eq!(r.shape.stroke.width(), 1);
}

/// `PCB_SHAPE::Flip` of a footprint child mirrors the library geometry about the library
/// frame centre and re-bakes; a board-level shape flips its board geometry. Both move to the
/// flipped layer.
#[test]
fn flip_board_and_footprint_shapes() {
    // board level: EDA_SHAPE::flip + sync
    let env = MutEnv::detached();
    let mut s = PcbShape::with_shape(ShapeT::Segment);
    s.base.item.layer = PcbLayerId::FSilkS;
    s.set_start(&env, v(1000, 2000));
    s.set_end(&env, v(5000, 2000));
    s.flip(&env, v(0, 0), FlipDirection::LeftRight);
    assert_eq!(s.base.item.layer, PcbLayerId::BSilkS);
    assert_eq!(
        (s.shape.start(), s.shape.end()),
        (v(-1000, 2000), v(-5000, 2000))
    );
    assert_eq!(
        (s.lib_start(), s.lib_end()),
        (v(-1000, 2000), v(-5000, 2000))
    );

    // footprint child: lib mirrored about the lib-frame image of the centre
    let mut fx = board_with_footprint(
        TransformTrs::new(v(10000000, 0), EdaAngle::from_degrees(90.0), 1.0, 1.0),
        false,
    );
    let id = add_fp_shape(&mut fx, ShapeT::Arc, |s, env| {
        s.set_arc_geometry(
            env,
            v(11000000, 0),
            v(10707107, 707107),
            v(10000000, 1000000),
        );
    });
    let (lib_start, lib_mid, lib_end) = {
        let a = &fx.board[id];
        (a.lib_start(), a.lib_arc_mid(), a.lib_end())
    };
    let xf = fx.board[fx.footprint].transform();
    let centre = v(10000000, 0);
    let lib_centre = xf.inverse_apply(centre);
    fx.board
        .modify_shape(id, |s, env| s.flip(env, centre, FlipDirection::TopBottom))
        .unwrap();

    let a = &fx.board[id];
    let m = |p: Vector2I| v(p.x, 2 * lib_centre.y - p.y);
    assert_eq!(a.lib_start(), m(lib_end));
    assert_eq!(a.lib_end(), m(lib_start));
    assert_eq!(a.lib_arc_mid(), m(lib_mid));
    assert_eq!(a.shape.start(), xf.apply(a.lib_start()));
    assert_eq!(a.shape.end(), xf.apply(a.lib_end()));
    assert_eq!(a.base.item.layer, PcbLayerId::BCu);
}

/// `transformPolyVertices`: a uniform transform keeps arcs as arcs, a skewing one moves the
/// vertices; the inverse undoes a uniform transform (up to rounding).
#[test]
fn transform_poly_vertices_keeps_arcs() {
    let mut poly = ShapePolySet::new();
    poly.new_outline();
    poly.append_point(v(0, 0), -1, -1);
    poly.append_arc(
        &kicad_geom::ShapeArc::from_start_mid_end(
            v(1000000, 0),
            v(1500000, 500000),
            v(1000000, 1000000),
            0,
        ),
        -1,
        -1,
        None,
    );
    poly.outline(0).set_closed(true);

    let mut uniform = poly.clone();
    let x = TransformTrs::new(v(100, 200), EdaAngle::from_degrees(30.0), 2.0, 2.0);
    lib_frame::transform_poly_vertices(&mut uniform, &x, false);
    assert_eq!(uniform.c_outline(0).arc_count(), 1);

    let mut skewed = poly.clone();
    let x = TransformTrs::new(v(0, 0), ANGLE_0, 2.0, 1.0);
    lib_frame::transform_poly_vertices(&mut skewed, &x, false);
    assert_eq!(
        skewed.c_outline(0).point_count(),
        poly.c_outline(0).point_count()
    );
    assert_eq!(skewed.c_outline(0).c_point(1), v(2000000, 0));
}

/// `PCB_SHAPE::cmp_drawings`: type, layer, shape, then UUID.
#[test]
fn cmp_drawings() {
    use std::cmp::Ordering;

    let mut board = Board::new();
    let mut a = PcbShape::new(&mut board.kiid_generator, ShapeT::Segment);
    a.base.item.layer = PcbLayerId::FSilkS;
    let mut b = PcbShape::new(&mut board.kiid_generator, ShapeT::Rectangle);
    b.base.item.layer = PcbLayerId::FSilkS;
    let ua = a.base.item.uuid;
    let ub = b.base.item.uuid;
    let ida: ItemId = board.add_shape(a, crate::board::AddMode::Append).into();
    let idb: ItemId = board.add_shape(b, crate::board::AddMode::Append).into();
    let ctx = BoardCtx::bare(&board);

    assert_eq!(
        shapes::pcb_shape_cmp_drawings(&ctx, ida, idb),
        Ordering::Less
    );
    assert_eq!(
        shapes::pcb_shape_cmp_drawings(&ctx, idb, ida),
        Ordering::Greater
    );
    assert_eq!(
        shapes::pcb_shape_cmp_drawings(&ctx, ida, ida),
        Ordering::Equal
    );
    let _ = (ua, ub, KicadT::PcbShapeT, Box2I::default());
}

// ---- replay of the reference dump (tests/oracle/m9_shapes_golden.sh) ----------------------

#[cfg(feature = "dump")]
mod oracle {
    use std::collections::{BTreeMap, BTreeSet};

    use kicad_core::Kiid;
    use kicad_geom::geometry::transform_trs::TransformTrs;
    use kicad_geom::{EdaAngle, Vector2I};

    use super::super::PcbShape;
    use crate::board::Board;
    use crate::ctx::BoardCtx;
    use crate::dump::format::{v_points, v_polyset, v_vec};
    use crate::dump::{DumpOptions, dump_board, fnv1a64, scan_file_uuids};
    use crate::eda_shape::ShapeT;
    use crate::io::ParseOptions;
    use crate::io::parser::PcbParser;
    use crate::item::ParentRef;
    use crate::item_ref::AnyItem;
    use crate::test_support::board_with_footprint;

    const GOLDEN: &str = include_str!("../../tests/golden/m9_shapes.txt");
    const SOURCE: &str = include_str!("../../tests/oracle/m9_shapes.kicad_pcb");

    /// `SECTION|KEY|FIELD` -> value of a dump text (data lines only), and the set of
    /// `SECTION|KEY|FIELD`s the Rust dump reports as pending.
    fn parse_dump(text: &str) -> (BTreeMap<String, String>, BTreeSet<String>) {
        let mut values = BTreeMap::new();
        let mut pending = BTreeSet::new();

        for line in text.lines() {
            let Some((lhs, value)) = line.split_once('=') else {
                continue;
            };

            if let Some(rest) = lhs.strip_prefix("pending|") {
                pending.insert(rest.to_string());
            } else if !lhs.starts_with("rust-error|") && !lhs.starts_with("file|") {
                values.insert(lhs.to_string(), value.to_string());
            }
        }

        (values, pending)
    }

    fn opts() -> DumpOptions {
        DumpOptions {
            sections: ["item", "bbox", "shape"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            hash: false,
            file_uuids: Some(scan_file_uuids(SOURCE.as_bytes())),
        }
    }

    fn parse_vec(s: &str) -> Vector2I {
        let (x, y) = s.split_once(',').unwrap();
        Vector2I::new(x.parse().unwrap(), y.parse().unwrap())
    }

    /// The footprint blocks of the oracle board: (uuid, fp_* lines).
    fn footprint_blocks() -> Vec<(String, Vec<&'static str>)> {
        let mut blocks = Vec::new();
        let mut current: Option<(String, Vec<&'static str>)> = None;

        for line in SOURCE.lines() {
            let t = line.trim();

            if t.starts_with("(footprint ") {
                current = Some((String::new(), Vec::new()));
            } else if let Some((uuid, lines)) = current.as_mut() {
                if uuid.is_empty() && t.starts_with("(uuid ") {
                    *uuid = t
                        .trim_start_matches("(uuid \"")
                        .trim_end_matches("\")")
                        .to_string();
                } else if t.starts_with("(fp_") {
                    lines.push(t);
                } else if line == "\t)" {
                    blocks.push(current.take().unwrap());
                }
            }
        }

        blocks
    }

    /// Replays `tests/golden/m9_shapes.txt`: the board-level shapes through the parser, the
    /// footprint shapes through the parser on fixture footprints carrying the oracle's
    /// transforms (the footprint parser is package M19). Every golden value of a shape
    /// must be reproduced, except fields the Rust dump reports as pending (other packages);
    /// `lib.arc_mid` / `lib.bezier` / `lib.poly` of footprint children are compared through
    /// the context-taking getters (`library_*`), since the dump calls the context-free ones.
    #[test]
    fn m9_shapes_match_oracle() {
        let (golden, _) = parse_dump(GOLDEN);
        let mut got: BTreeMap<String, String> = BTreeMap::new();
        let mut pending: BTreeSet<String> = BTreeSet::new();

        // Board-level shapes.
        let board = crate::io::parse_board(
            SOURCE,
            "",
            &ParseOptions {
                allow_pending: true,
            },
        )
        .expect("parse");
        let (values, p) = parse_dump(&dump_board(&board, &opts()));
        got.extend(values);
        pending.extend(p);

        // Footprint shapes.
        for (fp_uuid, lines) in footprint_blocks() {
            let field = |f: &str| golden[&format!("footprint|{fp_uuid}|{f}")].clone();
            let (sx, sy) = field("transform.scale")
                .split_once(',')
                .map(|(a, b)| (a.parse::<f64>().unwrap(), b.parse::<f64>().unwrap()))
                .unwrap();
            let xform = TransformTrs::new(
                parse_vec(&field("transform.translate")),
                EdaAngle::from_degrees(field("transform.rotate").parse().unwrap()),
                sx,
                sy,
            );
            let mut fx = board_with_footprint(xform, field("flipped") == "true");
            let parent = ParentRef::Item(fx.footprint.into());

            for line in lines {
                let board = std::mem::replace(&mut fx.board, Board::new());
                let mut p = PcbParser::new(line, "", board, true, ParseOptions::default());
                p.required_version = 20261031;
                p.lexer.need_left().unwrap();
                p.lexer.next_tok().unwrap();
                let shape = p.parse_pcb_shape(parent).expect("parse").expect("shape");
                fx.board = std::mem::replace(&mut p.board, Board::new());

                let uuid: Kiid = shape.base.item.uuid;
                let id = fx.add_child(shape, |fp| &mut fp.drawings);
                fx.board.rebind_item_uuid(id.into(), uuid);
            }

            fx.board.check_invariants().unwrap();
            let (values, p) = parse_dump(&dump_board(&fx.board, &opts()));
            got.extend(values);
            pending.extend(p);

            // The context-taking GetLibrary* getters.
            let ctx = BoardCtx::bare(&fx.board);

            for &id in &fx.board[fx.footprint].drawings {
                let Some(AnyItem::Shape(s)) = fx.board.get(id) else {
                    continue;
                };
                let key = format!("shape|{}|lib.", s.base.item.uuid.as_string());
                library_values(s, &ctx, &key, &mut got);
            }
        }

        let mut checked = 0;
        let mut skipped = Vec::new();
        let mut failures = Vec::new();

        for (key, want) in &golden {
            if key.starts_with("footprint|") {
                continue;
            }

            let Some(have) = got.get(key) else {
                if pending.contains(key) {
                    skipped.push(key.clone());
                } else {
                    failures.push(format!("{key}: missing (want {want})"));
                }
                continue;
            };

            let have = if want.starts_with('#') {
                format!("#{:016x}", fnv1a64(have.as_bytes()))
            } else {
                have.clone()
            };

            if &have != want {
                failures.push(format!("{key}: want {want}, got {have}"));
            }

            checked += 1;
        }

        eprintln!(
            "m9 oracle: {checked} values compared, {} pending (other packages)",
            skipped.len()
        );
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
        // Everything but the fields of other packages is compared.
        assert!(checked > 2000, "only {checked} values compared");
    }

    /// `GetLibraryArcMid` / `GetLibraryBezierC1/C2` / `GetLibraryPolyShape` as the C++ dump
    /// prints them, through the context-taking getters (overriding the dump's lines).
    fn library_values(
        s: &PcbShape,
        ctx: &BoardCtx<'_>,
        key: &str,
        got: &mut BTreeMap<String, String>,
    ) {
        match s.shape.shape() {
            ShapeT::Arc => {
                got.insert(format!("{key}arc_mid"), v_vec(s.library_arc_mid(ctx)));
            }
            ShapeT::Bezier => {
                got.insert(
                    format!("{key}bezier"),
                    v_points(&[s.library_bezier_c1(ctx), s.library_bezier_c2(ctx)]),
                );
            }
            ShapeT::Poly => {
                got.insert(format!("{key}poly"), v_polyset(&s.library_poly_shape(ctx)));
            }
            _ => {}
        }
    }
}
