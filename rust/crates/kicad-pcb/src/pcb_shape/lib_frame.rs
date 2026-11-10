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

//! Port of the library-frame machinery of `pcbnew/pcb_shape.cpp` (`syncLibCoords`,
//! `RebakeFromLib`, `OverrideLib*`, `GetLibrary*`) and of the `EDA_SHAPE` setter overrides of
//! `PCB_SHAPE`, which call `syncLibCoords()` after the `EDA_SHAPE` body. Signatures: package
//! M0c; bodies: package M9. Trivial lib-frame accessors are real.

use kicad_core::{PCB_IU_SCALE, StrokeParams};
use kicad_geom::cmath::{self, f64_to_i32_x86};
use kicad_geom::convert_basic_shapes_to_polygon::transform_round_chamfered_rect_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::geometry::shape_ellipse::ShapeEllipse;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{
    ANGLE_0, ANGLE_360, Box2I, EdaAngle, EdaAngleT, ShapeArc, ShapeLineChain, ShapePolySet,
    Vector2I, calc_arc_center, ki_round,
};

use crate::board_item;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::ShapeT;
use crate::ids::ItemKind;
use crate::item_ref::AnyItem;
use crate::pcb_shape::{DEFAULT_LINE_WIDTH, PcbShape};
use crate::zone_settings::ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM;

impl PcbShape {
    /// `GetLibraryStart()`.
    pub fn lib_start(&self) -> Vector2I {
        self.lib.start
    }

    /// `GetLibraryEnd()`.
    pub fn lib_end(&self) -> Vector2I {
        self.lib.end
    }

    /// `GetLibraryShape()`.
    pub fn lib_shape(&self) -> ShapeT {
        self.lib.shape
    }

    /// `GetLibraryCornerRadius()`.
    pub fn lib_corner_radius(&self) -> i32 {
        self.lib.corner_radius
    }

    /// `GetLibraryEllipseCenter()`.
    pub fn lib_ellipse_center(&self) -> Vector2I {
        self.lib.ellipse_center
    }

    /// `GetLibraryEllipseMajorRadius()`.
    pub fn lib_ellipse_major_radius(&self) -> i32 {
        self.lib.ellipse_major_radius
    }

    /// `GetLibraryEllipseMinorRadius()`.
    pub fn lib_ellipse_minor_radius(&self) -> i32 {
        self.lib.ellipse_minor_radius
    }

    /// `GetLibraryEllipseRotation()`.
    pub fn lib_ellipse_rotation(&self) -> EdaAngle {
        self.lib.ellipse_rotation
    }

    /// `GetLibraryEllipseStartAngle()`.
    pub fn lib_ellipse_start_angle(&self) -> EdaAngle {
        self.lib.ellipse_start_angle
    }

    /// `GetLibraryEllipseEndAngle()`.
    pub fn lib_ellipse_end_angle(&self) -> EdaAngle {
        self.lib.ellipse_end_angle
    }

    /// `GetLibPoly()`.
    pub fn lib_poly(&self) -> &ShapePolySet {
        &self.lib.poly
    }

    /// `GetLibraryArcMid()` without a context. Exact when the library shape is an arc
    /// (`m_libArcMid`) and for a shape without a parent footprint (board-level shape, pad
    /// primitive: `GetArcMid()`); for any other footprint child it returns `m_libArcMid`
    /// where the C++ returns `InverseApply( GetArcMid() )`. The faithful getter is
    /// [`PcbShape::library_arc_mid`].
    pub fn lib_arc_mid(&self) -> Vector2I {
        if self.lib.shape == ShapeT::Arc || self.has_parent_footprint_link() {
            return self.lib.arc_mid;
        }

        self.shape.arc_mid()
    }

    /// `GetLibraryBezierC1()` without a context. Exact for a pad primitive (`m_libBezierC1`)
    /// and a shape without parent (`GetBezierC1()`); for a footprint child it returns
    /// `m_libBezierC1` where the C++ returns `InverseApply( GetBezierC1() )` (they differ by
    /// the rounding of the transform). The faithful getter is
    /// [`PcbShape::library_bezier_c1`].
    pub fn lib_bezier_c1(&self) -> Vector2I {
        if self.parent_is_pad() || self.has_parent_footprint_link() {
            return self.lib.bezier_c1;
        }

        self.shape.bezier_c1()
    }

    /// `GetLibraryBezierC2()` without a context; see [`PcbShape::lib_bezier_c1`]. The
    /// faithful getter is [`PcbShape::library_bezier_c2`].
    pub fn lib_bezier_c2(&self) -> Vector2I {
        if self.parent_is_pad() || self.has_parent_footprint_link() {
            return self.lib.bezier_c2;
        }

        self.shape.bezier_c2()
    }

    /// `GetLibraryPolyShape()` without a context; see [`PcbShape::lib_bezier_c1`]. The
    /// faithful getter is [`PcbShape::library_poly_shape`].
    pub fn lib_poly_shape(&self) -> ShapePolySet {
        if self.parent_is_pad() || self.has_parent_footprint_link() {
            return self.lib.poly.clone();
        }

        self.shape.poly_shape().clone()
    }

    /// Whether the parent link is an item other than a pad (a footprint, or a table that may
    /// sit in a footprint): the context-free stand-in for `transformFp() != nullptr`.
    fn has_parent_footprint_link(&self) -> bool {
        self.base.item.parent.item().is_some() && !self.parent_is_pad()
    }

    /// Port of `PCB_SHAPE::GetLibraryArcMid()`.
    pub fn library_arc_mid(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        if self.lib.shape == ShapeT::Arc {
            return self.lib.arc_mid;
        }

        if let Some(fp) = self.transform_fp(ctx) {
            return fp.transform().inverse_apply(self.shape.arc_mid());
        }

        self.shape.arc_mid()
    }

    /// Port of `PCB_SHAPE::GetLibraryBezierC1()`.
    pub fn library_bezier_c1(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        if self.parent_is_pad() {
            return self.lib.bezier_c1;
        }

        if let Some(fp) = self.transform_fp(ctx) {
            return fp.transform().inverse_apply(self.shape.bezier_c1());
        }

        self.shape.bezier_c1()
    }

    /// Port of `PCB_SHAPE::GetLibraryBezierC2()`.
    pub fn library_bezier_c2(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        if self.parent_is_pad() {
            return self.lib.bezier_c2;
        }

        if let Some(fp) = self.transform_fp(ctx) {
            return fp.transform().inverse_apply(self.shape.bezier_c2());
        }

        self.shape.bezier_c2()
    }

    /// Port of `PCB_SHAPE::GetLibraryPolyShape()`.
    pub fn library_poly_shape(&self, ctx: &BoardCtx<'_>) -> ShapePolySet {
        if self.parent_is_pad() {
            return self.lib.poly.clone();
        }

        let mut poly = self.shape.poly_shape().clone();

        if let Some(fp) = self.transform_fp(ctx) {
            let xform = fp.transform();

            transform_poly_vertices(&mut poly, &xform, true);
        }

        poly
    }

    /// `OverrideLibCoords( aStart, aEnd, aArcMid )` (parser: store lib-frame values directly).
    pub fn override_lib_coords(&mut self, start: Vector2I, end: Vector2I, arc_mid: Vector2I) {
        self.lib.start = start;
        self.lib.end = end;

        if self.lib.shape == ShapeT::Arc {
            self.lib.arc_mid = arc_mid;
        }
    }

    /// `OverrideLibCornerRadius()`.
    pub fn override_lib_corner_radius(&mut self, corner_radius: i32) {
        self.lib.corner_radius = corner_radius;
    }

    /// `OverrideLibBezier( aC1, aC2 )`.
    pub fn override_lib_bezier(&mut self, c1: Vector2I, c2: Vector2I) {
        self.lib.bezier_c1 = c1;
        self.lib.bezier_c2 = c2;
    }

    /// `OverrideLibPoly()`.
    pub fn override_lib_poly(&mut self, poly: ShapePolySet) {
        self.lib.poly = poly;
    }

    /// `SetLibraryEllipse( aCenter, aMajor, aMinor, aRotation, aStartAngle, aEndAngle )`: the
    /// lib members and the (non-virtual) `EDA_SHAPE` ellipse setters; no `syncLibCoords()`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_library_ellipse(
        &mut self,
        _env: &MutEnv,
        center: Vector2I,
        major: i32,
        minor: i32,
        rotation: EdaAngle,
        start_angle: EdaAngle,
        end_angle: EdaAngle,
    ) {
        self.lib.ellipse_center = center;
        self.lib.ellipse_major_radius = major;
        self.lib.ellipse_minor_radius = minor;
        self.lib.ellipse_rotation = rotation;
        self.lib.ellipse_start_angle = start_angle;
        self.lib.ellipse_end_angle = end_angle;

        self.shape.set_ellipse_center(center);
        self.shape.set_ellipse_major_radius(major);
        self.shape.set_ellipse_minor_radius(minor);
        self.shape.set_ellipse_rotation(rotation);
        self.shape.set_ellipse_start_angle(start_angle);
        self.shape.set_ellipse_end_angle(end_angle);
    }

    /// Port of `PCB_SHAPE::RebakeFromLib()`: board-frame `EDA_SHAPE` geometry from the lib
    /// members and the parent footprint's transform (nothing without a footprint, e.g. for a
    /// pad primitive).
    pub fn rebake_from_lib(&mut self, env: &MutEnv) {
        if self.parent_is_pad() {
            return;
        }

        let Some(frame) = env.parent else {
            return;
        };

        self.rebake_from_transform_err(&frame.transform, env.max_error);
    }

    /// Port of `PCB_SHAPE::RebakeWithScale( aScaleX, aScaleY )` (pad primitives: the pad's
    /// `GetPrimitiveLibScale()`).
    pub fn rebake_with_scale(&mut self, env: &MutEnv, scale_x: f64, scale_y: f64) {
        let mut xform = TransformTrs::default();
        xform.set_scale(scale_x, scale_y);
        self.rebake_from_transform_err(&xform, env.max_error);
    }

    /// `rebakeFromTransform( aXform )` with `getMaxError()` = `ARC_HIGH_DEF` (the value of a
    /// shape without a board, and the default `m_MaxError`). The model's callers
    /// (`rebake_from_lib`, `rebake_with_scale`, `flip`) pass the board's value instead.
    pub fn rebake_from_transform(&mut self, xform: &TransformTrs) {
        self.rebake_from_transform_err(xform, ARC_HIGH_DEF);
    }

    /// Port of `PCB_SHAPE::rebakeFromTransform( aXform )`; `max_error` is `getMaxError()`.
    pub(crate) fn rebake_from_transform_err(&mut self, xform: &TransformTrs, max_error: i32) {
        let non_uniform = !xform.is_uniform_scale();
        let lib = self.lib.clone();

        if lib.shape == ShapeT::Circle {
            let lib_center = lib.start;
            let lib_radius = (lib.end - lib.start).euclidean_norm();
            let new_center = xform.apply(lib_center);

            if non_uniform {
                let mut major_radius = ki_round(lib_radius as f64 * xform.scale_x()).abs();
                let mut minor_radius = ki_round(lib_radius as f64 * xform.scale_y()).abs();

                let mut rotation = -xform.rotate();

                if minor_radius > major_radius {
                    std::mem::swap(&mut major_radius, &mut minor_radius);
                    rotation += EdaAngle::from_degrees(90.0);
                }

                // The PCB_SHAPE setters (each re-syncs the library frame; a non-uniform
                // transform always comes with a footprint or a scaled pad frame).
                self.shape.shape = ShapeT::Ellipse;
                self.shape.set_ellipse_center(new_center);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_major_radius(major_radius);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_minor_radius(minor_radius);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_rotation(rotation);
                self.sync_lib_coords_xform(xform, true);
            } else {
                self.shape.shape = ShapeT::Circle;
                self.shape.set_start(new_center);
                self.shape.set_end(xform.apply(lib.end));
            }

            return;
        }

        if lib.shape == ShapeT::Arc {
            let lib_center = calc_arc_center(lib.start, lib.arc_mid, lib.end);
            let lib_radius = (lib.start - lib_center).euclidean_norm();
            let new_center = xform.apply(lib_center);

            if non_uniform {
                let mut major_radius = ki_round(lib_radius as f64 * xform.scale_x()).abs();
                let mut minor_radius = ki_round(lib_radius as f64 * xform.scale_y()).abs();

                let mut rotation = -xform.rotate();
                let mut start_angle = EdaAngle::from_vector(lib.start - lib_center);
                let mid_angle = EdaAngle::from_vector(lib.arc_mid - lib_center);
                let mut end_angle = EdaAngle::from_vector(lib.end - lib_center);

                let wrap = |mut a: EdaAngle, base: EdaAngle| {
                    while a < base {
                        a += ANGLE_360;
                    }

                    a
                };

                if wrap(mid_angle, start_angle) > wrap(end_angle, start_angle) {
                    std::mem::swap(&mut start_angle, &mut end_angle);
                }

                while end_angle < start_angle {
                    end_angle += ANGLE_360;
                }

                if minor_radius > major_radius {
                    std::mem::swap(&mut major_radius, &mut minor_radius);
                    rotation += EdaAngle::from_degrees(90.0);
                    start_angle -= EdaAngle::from_degrees(90.0);
                    end_angle -= EdaAngle::from_degrees(90.0);
                }

                self.shape.shape = ShapeT::EllipseArc;
                self.shape.set_ellipse_center(new_center);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_major_radius(major_radius);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_minor_radius(minor_radius);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_rotation(rotation);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_start_angle(start_angle);
                self.sync_lib_coords_xform(xform, true);
                self.shape.set_ellipse_end_angle(end_angle);
                self.sync_lib_coords_xform(xform, true);
            } else {
                self.shape.shape = ShapeT::Arc;
                self.shape.set_arc_geometry(
                    xform.apply(lib.start),
                    xform.apply(lib.arc_mid),
                    xform.apply(lib.end),
                );
            }

            return;
        }

        if lib.shape == ShapeT::Ellipse || lib.shape == ShapeT::EllipseArc {
            // The linear transform preserves ellipse-ness only when scale is uniform
            // or the lib ellipse's axes are aligned with the scale axes (cardinal lib
            // rotation). Otherwise the result is a sheared shape that no standard
            // ellipse can represent; tessellate to POLY in that case.
            let keep_native = xform.is_uniform_scale() || lib.ellipse_rotation.is_cardinal();

            if keep_native {
                self.shape.shape = lib.shape;
                self.shape
                    .set_ellipse_center(xform.apply(lib.ellipse_center));

                if xform.is_uniform_scale() {
                    let abs_scale = xform.scale_x().abs();
                    self.shape.set_ellipse_major_radius(ki_round(
                        lib.ellipse_major_radius as f64 * abs_scale,
                    ));
                    self.shape.set_ellipse_minor_radius(ki_round(
                        lib.ellipse_minor_radius as f64 * abs_scale,
                    ));
                    self.shape
                        .set_ellipse_rotation(lib.ellipse_rotation - xform.rotate());

                    if lib.shape == ShapeT::EllipseArc {
                        self.shape.set_ellipse_start_angle(lib.ellipse_start_angle);
                        self.shape.set_ellipse_end_angle(lib.ellipse_end_angle);
                    }
                } else {
                    let be = decompose_board_ellipse(
                        xform,
                        lib.ellipse_major_radius,
                        lib.ellipse_minor_radius,
                        lib.ellipse_rotation,
                    );

                    self.shape.set_ellipse_major_radius(ki_round(be.major));
                    self.shape.set_ellipse_minor_radius(ki_round(be.minor));
                    self.shape.set_ellipse_rotation(be.rotation);

                    if lib.shape == ShapeT::EllipseArc {
                        self.shape
                            .set_ellipse_start_angle(lib.ellipse_start_angle + be.start_shift);
                        self.shape
                            .set_ellipse_end_angle(lib.ellipse_end_angle + be.start_shift);
                    }
                }
            } else {
                let is_arc = lib.shape == ShapeT::EllipseArc;

                let lib_ellipse = if is_arc {
                    ShapeEllipse::new_arc(
                        lib.ellipse_center,
                        lib.ellipse_major_radius,
                        lib.ellipse_minor_radius,
                        lib.ellipse_rotation,
                        lib.ellipse_start_angle,
                        lib.ellipse_end_angle,
                    )
                } else {
                    ShapeEllipse::new(
                        lib.ellipse_center,
                        lib.ellipse_major_radius,
                        lib.ellipse_minor_radius,
                        lib.ellipse_rotation,
                    )
                };

                let chain = lib_ellipse.convert_to_polyline(max_error);

                self.shape.shape = ShapeT::Poly;
                let poly = &mut self.shape.poly;
                poly.remove_all_contours();
                poly.new_outline();

                for ii in 0..chain.point_count() {
                    poly.append_point(xform.apply(chain.c_point(ii)), -1, -1);
                }

                poly.outline(0).set_closed(!is_arc);
                self.shape.set_hatching_dirty();
            }

            return;
        }

        if lib.shape == ShapeT::Rectangle {
            let c1 = xform.apply(lib.start);
            let c2 = xform.apply(Vector2I::new(lib.end.x, lib.start.y));
            let c3 = xform.apply(lib.end);
            let c4 = xform.apply(Vector2I::new(lib.start.x, lib.end.y));

            if xform.rotate().is_cardinal() && (lib.corner_radius == 0 || xform.is_uniform_scale())
            {
                self.shape.shape = ShapeT::Rectangle;
                let mut bbox = Box2I::new(c1, Vector2I::new(0, 0));
                bbox.merge_point(c2);
                bbox.merge_point(c3);
                bbox.merge_point(c4);
                self.shape.set_start(bbox.origin());
                self.shape.set_end(bbox.end());

                // EDA_SHAPE::SetCornerRadius( int ): the double is truncated.
                self.shape.set_corner_radius(f64_to_i32_x86(
                    xform.apply_linear_scale(lib.corner_radius as f64),
                ));
            } else {
                self.shape.shape = ShapeT::Poly;
                let poly = &mut self.shape.poly;
                poly.remove_all_contours();

                if lib.corner_radius > 0 {
                    let mut rounded = ShapePolySet::new();
                    let lib_center = (lib.start + lib.end) / 2;
                    let lib_size = Vector2I::new(
                        (lib.end.x - lib.start.x).abs(),
                        (lib.end.y - lib.start.y).abs(),
                    );

                    transform_round_chamfered_rect_to_polygon(
                        &mut rounded,
                        lib_center,
                        lib_size,
                        ANGLE_0,
                        lib.corner_radius,
                        0.0,
                        0,
                        0,
                        max_error,
                        ErrorLoc::Inside,
                    );

                    poly.new_outline();

                    let outline = rounded.c_outline(0);

                    for ii in 0..outline.point_count() {
                        poly.append_point(xform.apply(outline.c_point(ii)), -1, -1);
                    }
                } else {
                    poly.new_outline();
                    poly.append_point(c1, -1, -1);
                    poly.append_point(c2, -1, -1);
                    poly.append_point(c3, -1, -1);
                    poly.append_point(c4, -1, -1);
                }

                self.shape.set_hatching_dirty();
                self.shape.set_start(c1);
                self.shape.set_end(c3);
            }

            return;
        }

        if lib.shape == ShapeT::Poly {
            let mut poly = lib.poly.clone();

            transform_poly_vertices(&mut poly, xform, false);
            self.shape.poly = poly;
            self.shape.set_hatching_dirty();

            // m_libStart/m_libEnd are not seeded for POLY, skip the start/end fall-through
            // below.
            return;
        }

        self.shape.set_start(xform.apply(lib.start));
        self.shape.set_end(xform.apply(lib.end));

        if lib.shape == ShapeT::Bezier {
            self.shape.set_bezier_c1(xform.apply(lib.bezier_c1));
            self.shape.set_bezier_c2(xform.apply(lib.bezier_c2));
            self.shape.rebuild_bezier_to_segments_points_list(max_error);
        }
    }

    /// Whether the direct parent is a pad (`GetParent()->Type() == PCB_PAD_T`): a pad
    /// primitive, stored in the pad frame.
    pub(crate) fn parent_is_pad(&self) -> bool {
        self.base
            .item
            .parent
            .item()
            .is_some_and(|id| id.kind() == ItemKind::Pad)
    }

    /// The `xform` / `hasXform` pair `syncLibCoords()` computes, as a mutator sees it: the
    /// pad's `GetPrimitiveLibScale()` for a pad primitive (`MutEnv::pad_primitive_scale`,
    /// set by the pad for its primitives), else the parent footprint's transform
    /// (`transformFp()`), else none.
    pub(crate) fn sync_frame(&self, env: &MutEnv) -> (TransformTrs, bool) {
        if self.parent_is_pad() {
            let (sx, sy) = env.pad_primitive_scale;
            let mut xform = TransformTrs::default();
            xform.set_scale(sx, sy);
            let has_xform = sx != 1.0 || sy != 1.0;
            return (xform, has_xform);
        }

        match env.parent {
            Some(frame) => (frame.transform, true),
            None => (TransformTrs::default(), false),
        }
    }

    /// Port of `PCB_SHAPE::syncLibCoords()` (virtual, `PCB_TEXTBOX` overrides): lib members
    /// from the board-frame geometry and the parent transform.
    pub fn sync_lib_coords(&mut self, env: &MutEnv) {
        let (xform, has_xform) = self.sync_frame(env);
        self.sync_lib_coords_xform(&xform, has_xform);
    }

    /// The body of `syncLibCoords()` for a given `xform` / `hasXform`.
    pub(crate) fn sync_lib_coords_xform(&mut self, xform: &TransformTrs, has_xform: bool) {
        let es = &self.shape;
        let lib = &mut self.lib;

        if es.shape == ShapeT::Ellipse || es.shape == ShapeT::EllipseArc {
            let e = &es.ellipse;

            if has_xform {
                lib.ellipse_center = xform.inverse_apply(e.center);

                if xform.is_uniform_scale() {
                    let inv_scale = 1.0 / xform.scale_x().abs();
                    lib.ellipse_major_radius = ki_round(e.major_radius as f64 * inv_scale);
                    lib.ellipse_minor_radius = ki_round(e.minor_radius as f64 * inv_scale);
                    lib.ellipse_rotation = e.rotation + xform.rotate();
                    lib.ellipse_start_angle = e.start_angle;
                    lib.ellipse_end_angle = e.end_angle;
                } else {
                    // Non-uniform scale shears the ellipse, so invert the whole board ellipse
                    // to lib.
                    let le = compose_lib_ellipse(
                        xform,
                        e.major_radius as f64,
                        e.minor_radius as f64,
                        e.rotation,
                    );
                    lib.ellipse_major_radius = ki_round(le.major);
                    lib.ellipse_minor_radius = ki_round(le.minor);
                    lib.ellipse_rotation = le.rotation;
                    lib.ellipse_start_angle = e.start_angle + le.start_shift;
                    lib.ellipse_end_angle = e.end_angle + le.start_shift;
                }
            } else {
                lib.ellipse_center = e.center;
                lib.ellipse_major_radius = e.major_radius;
                lib.ellipse_minor_radius = e.minor_radius;
                lib.ellipse_rotation = e.rotation;
                lib.ellipse_start_angle = e.start_angle;
                lib.ellipse_end_angle = e.end_angle;
            }

            return;
        }

        if es.shape == ShapeT::Poly {
            lib.poly = es.poly_shape().clone();

            if has_xform {
                transform_poly_vertices(&mut lib.poly, xform, true);
            }

            return;
        }

        if has_xform {
            lib.start = xform.inverse_apply(es.start());
            lib.end = xform.inverse_apply(es.end());

            if es.shape == ShapeT::Rectangle {
                // We assume xform is uniform scale.  If not, we should have been turned into a
                // polygon.
                lib.corner_radius =
                    f64_to_i32_x86(xform.inverse_apply_linear_scale(es.corner_radius() as f64));
            }

            if es.shape == ShapeT::Arc {
                lib.arc_mid = xform.inverse_apply(es.arc_mid());
            }

            if es.shape == ShapeT::Bezier {
                lib.bezier_c1 = xform.inverse_apply(es.bezier_c1());
                lib.bezier_c2 = xform.inverse_apply(es.bezier_c2());
            }
        } else {
            lib.start = es.start();
            lib.end = es.end();

            if es.shape == ShapeT::Rectangle {
                lib.corner_radius = es.corner_radius();
            }

            if es.shape == ShapeT::Arc {
                lib.arc_mid = es.arc_mid();
            }

            if es.shape == ShapeT::Bezier {
                lib.bezier_c1 = es.bezier_c1();
                lib.bezier_c2 = es.bezier_c2();
            }
        }
    }

    /// Port of `PCB_SHAPE::Scale( aScale )`: `EDA_SHAPE::scale()` only (no lib-frame sync).
    pub fn scale(&mut self, env: &MutEnv, scale: f64) {
        self.shape.scale(scale, env.max_error);
    }

    /// Port of `PCB_SHAPE::GetCorners()` (virtual, introduced by `PCB_SHAPE`; `PCB_TEXTBOX`
    /// overrides): the 4 corners of a rectangle or rotated rectangle (stored as a poly).
    /// `UNIMPLEMENTED_FOR` other shapes (nothing; then the padding below reads the last
    /// point of an empty list, which is UB in C++: the port returns an empty list).
    pub fn corners(&self, _ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        let mut pts: Vec<Vector2I> = Vec::new();

        if self.shape.shape() == ShapeT::Rectangle {
            pts = self.shape.rect_corners();
        } else if self.shape.shape() == ShapeT::Poly {
            let poly = self.shape.poly_shape();

            for ii in 0..poly.outline_count() {
                pts.extend_from_slice(poly.c_outline(ii).c_points());
            }
        }

        if pts.is_empty() {
            return pts;
        }

        while pts.len() < 4 {
            let last = pts[pts.len() - 1];
            pts.push(last + Vector2I::new(10, 10));
        }

        pts
    }

    /// `GetSolderMaskExpansion()`.
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>) -> i32 {
        let mut margin = 0;
        let has_board = board_item::has_board(self, ctx);
        let drc_engine = ctx.rules.filter(|engine| {
            has_board
                && engine
                    .has_rules_for_constraint_type(DrcConstraintT::SolderMaskExpansionConstraint)
        });

        if let Some(drc_engine) = drc_engine {
            let this = ctx.view(AnyItem::Shape(self));
            let constraint = drc_engine.eval_rules(
                ctx,
                DrcConstraintT::SolderMaskExpansionConstraint,
                Some(this),
                None,
                self.base.item.layer,
            );

            if constraint.value.has_opt() {
                margin = constraint.value.opt();
            }
        } else if let Some(local) = self.solder_mask_margin {
            margin = local;
        } else if has_board {
            margin = ctx.board.design_settings().solder_mask_expansion;
        }

        // Ensure the resulting mask opening has a non-negative size
        if margin < 0 && !self.shape.is_solid_fill() {
            margin = margin.max(-self.width(ctx) / 2);
        }

        margin
    }

    /// `HasSolderMask()`.
    pub fn has_solder_mask(&self) -> bool {
        self.has_solder_mask
    }

    /// `SetHasSolderMask()`.
    pub fn set_has_solder_mask(&mut self, value: bool) {
        self.has_solder_mask = value;
    }

    /// `GetLocalSolderMaskMargin()`.
    pub fn local_solder_mask_margin(&self) -> Option<i32> {
        self.solder_mask_margin
    }

    /// `SetLocalSolderMaskMargin()`.
    pub fn set_local_solder_mask_margin(&mut self, margin: Option<i32>) {
        self.solder_mask_margin = margin;
    }

    // ---- EDA_SHAPE setter overrides: the EDA_SHAPE body, then syncLibCoords() ----

    /// Port of `PCB_SHAPE::SetStart()`.
    pub fn set_start(&mut self, env: &MutEnv, start: Vector2I) {
        self.shape.set_start(start);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEnd()`.
    pub fn set_end(&mut self, env: &MutEnv, end: Vector2I) {
        self.shape.set_end(end);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetCenter()`. (The `EDA_SHAPE` body routes a circle through the
    /// virtual `SetStart` / `SetEnd`, whose syncs are superseded by the final one.)
    pub fn set_center(&mut self, env: &MutEnv, center: Vector2I) {
        self.shape.set_center(center);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetCornerRadius()`.
    pub fn set_corner_radius(&mut self, env: &MutEnv, corner_radius: i32) {
        self.shape.set_corner_radius(corner_radius);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetWidth()`: the stroke width is stored in lib units, so the
    /// parent footprint's linear scale is stripped on write.
    pub fn set_width(&mut self, env: &MutEnv, width: i32) {
        let s = if self.parent_is_pad() {
            1.0
        } else {
            env_scale_linear(env)
        };

        if s == 1.0 {
            self.shape.stroke.set_width(width);
        } else {
            self.shape.stroke.set_width(ki_round(width as f64 / s));
        }

        self.shape.set_hatching_dirty();
    }

    /// Port of `PCB_SHAPE::SetLibStrokeWidth()`.
    pub fn set_lib_stroke_width(&mut self, width: i32) {
        self.shape.stroke.set_width(width);
        self.shape.set_hatching_dirty();
    }

    /// Port of `PCB_SHAPE::SetStroke()`: `m_stroke = aStroke; SetWidth( aStroke.GetWidth() )`.
    pub fn set_stroke(&mut self, env: &MutEnv, stroke: StrokeParams) {
        let width = stroke.width();
        self.shape.stroke = stroke;
        self.set_width(env, width);
    }

    /// Port of `PCB_SHAPE::SetShape()`: the `EDA_SHAPE` body and `m_libShape = aShape` (no
    /// sync).
    pub fn set_shape(&mut self, _env: &MutEnv, shape: ShapeT) {
        self.shape.set_shape(shape);
        self.lib.shape = shape;
    }

    /// Port of `PCB_SHAPE::SetArcGeometry( aStart, aMid, aEnd )`.
    pub fn set_arc_geometry(
        &mut self,
        env: &MutEnv,
        start: Vector2I,
        mid: Vector2I,
        end: Vector2I,
    ) {
        self.shape.set_arc_geometry(start, mid, end);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetArcAngle()` (header): `EDA_SHAPE::SetArcAngle`, then sync.
    /// `EDA_SHAPE::SetArcAngle( aAngle )` is `SetArcAngleAndEnd( aAngle, false )`.
    pub fn set_arc_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        self.shape.set_arc_angle_and_end(angle, false);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetArcAngleAndEnd( aAngle, aCheckNegativeAngle )` (header).
    pub fn set_arc_angle_and_end(
        &mut self,
        env: &MutEnv,
        angle: EdaAngle,
        check_negative_angle: bool,
    ) {
        self.shape
            .set_arc_angle_and_end(angle, check_negative_angle);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetBezierC1()`.
    pub fn set_bezier_c1(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.set_bezier_c1(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetBezierC2()`.
    pub fn set_bezier_c2(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.set_bezier_c2(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetPolyShape()`.
    pub fn set_poly_shape(&mut self, env: &MutEnv, poly: ShapePolySet) {
        self.shape.set_poly_shape(poly);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseCenter()`.
    pub fn set_ellipse_center(&mut self, env: &MutEnv, pt: Vector2I) {
        self.shape.set_ellipse_center(pt);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseMajorRadius()`.
    pub fn set_ellipse_major_radius(&mut self, env: &MutEnv, r: i32) {
        self.shape.set_ellipse_major_radius(r);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseMinorRadius()`.
    pub fn set_ellipse_minor_radius(&mut self, env: &MutEnv, r: i32) {
        self.shape.set_ellipse_minor_radius(r);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseRotation()`.
    pub fn set_ellipse_rotation(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.set_ellipse_rotation(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseStartAngle()`.
    pub fn set_ellipse_start_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.set_ellipse_start_angle(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetEllipseEndAngle()`.
    pub fn set_ellipse_end_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        self.shape.set_ellipse_end_angle(a);
        self.sync_lib_coords(env);
    }

    /// Port of `PCB_SHAPE::SetIsProxyItem()` without an environment: the width change goes
    /// through `SetWidth()` with no footprint scale (exact for pad primitives, the only
    /// proxy items, and for board-level shapes). The pad lookup of the C++ (`IsEntered()` pad
    /// of a footprint-holder board, i.e. the footprint editor's pad edit mode) never finds
    /// a pad here.
    pub fn set_is_proxy_item(&mut self, is_proxy: bool) {
        let env = MutEnv::detached();
        self.set_is_proxy_item_env(&env, is_proxy, None);
    }

    /// Port of `PCB_SHAPE::SetIsProxyItem( aIsProxy )`; `entered_pad_spoke_width` is the
    /// `GetLocalThermalSpokeWidthOverride()` of the pad being edited (`IsEntered()`), if any.
    pub fn set_is_proxy_item_env(
        &mut self,
        env: &MutEnv,
        is_proxy: bool,
        entered_pad_spoke_width: Option<i32>,
    ) {
        if is_proxy && !self.shape.proxy_item {
            if self.shape.shape() == ShapeT::Segment {
                if let Some(width) = entered_pad_spoke_width {
                    self.set_width(env, width);
                } else {
                    self.set_width(
                        env,
                        PCB_IU_SCALE.mm_to_iu(ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM),
                    );
                }
            } else {
                self.set_width(env, 1);
            }
        } else if self.shape.proxy_item && !is_proxy {
            self.set_width(env, PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH));
        }

        self.shape.proxy_item = is_proxy;
    }
}

/// `fpScaleLinear( transformFp() )` as a mutator sees it (the parent frame of `env`).
fn env_scale_linear(env: &MutEnv) -> f64 {
    match env.parent {
        None => 1.0,
        Some(frame) => (frame.transform.scale_x() + frame.transform.scale_y()) * 0.5,
    }
}

/// Port of `transformPolyVertices( aPoly, aXform, aInverse )` (static, pcbnew/pcb_shape.cpp).
///
/// Move every vertex of the polygon through the transform. SetVertex turns the arc a vertex
/// belongs to into segments, so a uniform transform maps each arc as an arc and only a skewing
/// one is left to flatten them.
pub(crate) fn transform_poly_vertices(
    poly: &mut ShapePolySet,
    xform: &TransformTrs,
    inverse: bool,
) {
    let map = |pt: Vector2I| {
        if inverse {
            xform.inverse_apply(pt)
        } else {
            xform.apply(pt)
        }
    };

    if !xform.is_uniform_scale() {
        let mut indices = Vec::new();
        let mut it = poly.iterate_with_holes();

        while it.is_valid() {
            indices.push(it.get_index());
            it.advance();
        }

        for index in indices {
            let pt = poly.c_vertex_index(index);
            poly.set_vertex_index(&index, map(pt));
        }

        return;
    }

    // a mirroring transform reverses the sweep of an arc
    let mirrored = xform.scale_x() * xform.scale_y() < 0.0;

    let transform_chain = |chain: &mut ShapeLineChain| {
        let mut mapped = ShapeLineChain::new();
        let count = chain.point_count();
        let mut i = 0;

        while i < count {
            let arc_index = chain.arc_index(i as usize);

            if arc_index < 0 {
                mapped.append_point(map(chain.c_point(i)), false);
                i += 1;
                continue;
            }

            let arc = chain.arc(arc_index as usize);
            let sweep = arc.get_central_angle();

            // three points fix an arc exactly, except a full circle whose ends coincide
            if arc.get_p0() != arc.get_p1() {
                mapped.append_arc(&ShapeArc::from_start_mid_end(
                    map(arc.get_p0()),
                    map(arc.get_arc_mid()),
                    map(arc.get_p1()),
                    0,
                ));
            } else {
                mapped.append_arc(&ShapeArc::from_center_start_angle(
                    map(arc.get_center()),
                    map(arc.get_p0()),
                    if mirrored { -sweep } else { sweep },
                    0,
                ));
            }

            while i + 1 < count && chain.arc_index((i + 1) as usize) == arc_index {
                i += 1;
            }

            i += 1;
        }

        mapped.set_closed(chain.is_closed());
        *chain = mapped;
    };

    for i in 0..poly.outline_count() {
        transform_chain(poly.outline(i));

        for j in 0..poly.hole_count(i) {
            transform_chain(poly.hole(i, j));
        }
    }
}

/// `BOARD_ELLIPSE` (anonymous namespace, pcbnew/pcb_shape.cpp).
struct BoardEllipse {
    major: f64,
    minor: f64,
    rotation: EdaAngle,
    start_shift: EdaAngle,
}

/// Port of `decompose2x2( m00, m01, m10, m11 )`: SVD of a 2x2 matrix into ellipse major /
/// minor / rotation and the arc angle shift.
fn decompose_2x2(m00: f64, m01: f64, m10: f64, m11: f64) -> BoardEllipse {
    let a = m00 * m00 + m10 * m10;
    let b = m00 * m01 + m10 * m11;
    let c = m01 * m01 + m11 * m11;

    let diff = a - c;
    let rad = cmath::hypot(diff, 2.0 * b);
    let lambda1 = (a + c + rad) * 0.5;
    let lambda2 = (a + c - rad) * 0.5;

    let sigma1 = 0.0_f64.max(lambda1).sqrt();
    let sigma2 = 0.0_f64.max(lambda2).sqrt();

    let mut v0x;
    let mut v0y;

    if b.abs() > 1e-12 {
        v0x = lambda1 - c;
        v0y = b;
    } else if a >= c {
        v0x = 1.0;
        v0y = 0.0;
    } else {
        v0x = 0.0;
        v0y = 1.0;
    }

    let vn = cmath::hypot(v0x, v0y);
    v0x /= vn;
    v0y /= vn;

    let mut u0x = 1.0;
    let mut u0y = 0.0;

    if sigma1 > 1e-12 {
        u0x = (m00 * v0x + m01 * v0y) / sigma1;
        u0y = (m10 * v0x + m11 * v0y) / sigma1;
    }

    BoardEllipse {
        major: sigma1,
        minor: sigma2,
        rotation: EdaAngle::new(cmath::atan2(u0y, u0x), EdaAngleT::Radians),
        start_shift: EdaAngle::new(-cmath::atan2(v0y, v0x), EdaAngleT::Radians),
    }
}

/// Port of `decomposeBoardEllipse( aXform, aLibMajor, aLibMinor, aLibRotation )`: board
/// ellipse from a lib ellipse, `M = R(theta) * diag(sx, sy) * R(phi) * diag(a, b)`.
fn decompose_board_ellipse(
    xform: &TransformTrs,
    lib_major: i32,
    lib_minor: i32,
    lib_rotation: EdaAngle,
) -> BoardEllipse {
    let sx = xform.scale_x();
    let sy = xform.scale_y();
    // Lib rotation and xform rotation use opposite signs, negate to match.
    let theta = -xform.rotate().as_radians();
    let phi = lib_rotation.as_radians();
    let c_theta = cmath::cos(theta);
    let s_theta = cmath::sin(theta);
    let c_phi = cmath::cos(phi);
    let s_phi = cmath::sin(phi);
    let a = lib_major as f64;
    let b = lib_minor as f64;

    let m00 = a * (sx * c_theta * c_phi - sy * s_theta * s_phi);
    let m01 = -b * (sx * c_theta * s_phi + sy * s_theta * c_phi);
    let m10 = a * (sx * s_theta * c_phi + sy * c_theta * s_phi);
    let m11 = b * (-sx * s_theta * s_phi + sy * c_theta * c_phi);

    decompose_2x2(m00, m01, m10, m11)
}

/// Port of `composeLibEllipse( aXform, aBoardMajor, aBoardMinor, aBoardRotation )`: the
/// inverse of `decomposeBoardEllipse`, lib ellipse from a board ellipse.
fn compose_lib_ellipse(
    xform: &TransformTrs,
    board_major: f64,
    board_minor: f64,
    board_rotation: EdaAngle,
) -> BoardEllipse {
    let sx = xform.scale_x();
    let sy = xform.scale_y();
    let theta = -xform.rotate().as_radians();
    let beta = board_rotation.as_radians();

    let li00 = cmath::cos(theta) / sx;
    let li01 = cmath::sin(theta) / sx;
    let li10 = -cmath::sin(theta) / sy;
    let li11 = cmath::cos(theta) / sy;

    let e00 = board_major * cmath::cos(beta);
    let e01 = -board_minor * cmath::sin(beta);
    let e10 = board_major * cmath::sin(beta);
    let e11 = board_minor * cmath::cos(beta);

    decompose_2x2(
        li00 * e00 + li01 * e10,
        li00 * e01 + li01 * e11,
        li10 * e00 + li11 * e10,
        li10 * e01 + li11 * e11,
    )
}
