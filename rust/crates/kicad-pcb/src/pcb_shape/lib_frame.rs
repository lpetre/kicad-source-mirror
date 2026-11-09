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

use kicad_core::StrokeParams;
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::{EdaAngle, ShapePolySet, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::eda_shape::ShapeT;
use crate::pcb_shape::PcbShape;

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

    /// `GetLibraryArcMid()`.
    pub fn lib_arc_mid(&self) -> Vector2I {
        crate::port_pending!("PCB_SHAPE::GetLibraryArcMid")
    }

    /// `GetLibraryBezierC1()`.
    pub fn lib_bezier_c1(&self) -> Vector2I {
        crate::port_pending!("PCB_SHAPE::GetLibraryBezierC1")
    }

    /// `GetLibraryBezierC2()`.
    pub fn lib_bezier_c2(&self) -> Vector2I {
        crate::port_pending!("PCB_SHAPE::GetLibraryBezierC2")
    }

    /// `GetLibraryPolyShape()`.
    pub fn lib_poly_shape(&self) -> ShapePolySet {
        crate::port_pending!("PCB_SHAPE::GetLibraryPolyShape")
    }

    /// `OverrideLibCoords( aStart, aEnd, aArcMid )` (parser: store lib-frame values directly).
    pub fn override_lib_coords(&mut self, start: Vector2I, end: Vector2I, arc_mid: Vector2I) {
        crate::port_pending!("PCB_SHAPE::OverrideLibCoords", start, end, arc_mid)
    }

    /// `OverrideLibCornerRadius()`.
    pub fn override_lib_corner_radius(&mut self, corner_radius: i32) {
        self.lib.corner_radius = corner_radius;
    }

    /// `OverrideLibBezier( aC1, aC2 )`.
    pub fn override_lib_bezier(&mut self, c1: Vector2I, c2: Vector2I) {
        crate::port_pending!("PCB_SHAPE::OverrideLibBezier", c1, c2)
    }

    /// `OverrideLibPoly()`.
    pub fn override_lib_poly(&mut self, poly: ShapePolySet) {
        self.lib.poly = poly;
    }

    /// `SetLibraryEllipse( aCenter, aMajor, aMinor, aRotation, aStartAngle, aEndAngle )`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_library_ellipse(
        &mut self,
        env: &MutEnv,
        center: Vector2I,
        major: i32,
        minor: i32,
        rotation: EdaAngle,
        start_angle: EdaAngle,
        end_angle: EdaAngle,
    ) {
        crate::port_pending!(
            "PCB_SHAPE::SetLibraryEllipse",
            env,
            center,
            major,
            minor,
            rotation,
            start_angle,
            end_angle
        )
    }

    /// `RebakeFromLib()`: board-frame `EDA_SHAPE` geometry from the lib members and the parent
    /// transform.
    pub fn rebake_from_lib(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_SHAPE::RebakeFromLib", env)
    }

    /// `RebakeWithScale( aScaleX, aScaleY )`.
    pub fn rebake_with_scale(&mut self, env: &MutEnv, scale_x: f64, scale_y: f64) {
        crate::port_pending!("PCB_SHAPE::RebakeWithScale", env, scale_x, scale_y)
    }

    /// `rebakeFromTransform( aXform )`.
    pub fn rebake_from_transform(&mut self, xform: &TransformTrs) {
        crate::port_pending!("PCB_SHAPE::rebakeFromTransform", xform)
    }

    /// `syncLibCoords()` (virtual, `PCB_TEXTBOX` overrides): lib members from the board-frame
    /// geometry and the parent transform.
    pub fn sync_lib_coords(&mut self, env: &MutEnv) {
        crate::port_pending!("PCB_SHAPE::syncLibCoords", env)
    }

    /// `Scale( aScale )`.
    pub fn scale(&mut self, env: &MutEnv, scale: f64) {
        crate::port_pending!("PCB_SHAPE::Scale", env, scale)
    }

    /// `GetCorners()` (virtual, introduced by `PCB_SHAPE`; `PCB_TEXTBOX` overrides).
    pub fn corners(&self, ctx: &BoardCtx<'_>) -> Vec<Vector2I> {
        crate::port_pending!("PCB_SHAPE::GetCorners", ctx)
    }

    /// `GetSolderMaskExpansion()`.
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>) -> i32 {
        crate::port_pending!("PCB_SHAPE::GetSolderMaskExpansion", ctx)
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

    /// `PCB_SHAPE::SetStart()`.
    pub fn set_start(&mut self, env: &MutEnv, start: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetStart", env, start)
    }

    /// `PCB_SHAPE::SetEnd()`.
    pub fn set_end(&mut self, env: &MutEnv, end: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetEnd", env, end)
    }

    /// `PCB_SHAPE::SetCenter()`.
    pub fn set_center(&mut self, env: &MutEnv, center: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetCenter", env, center)
    }

    /// `PCB_SHAPE::SetCornerRadius()`.
    pub fn set_corner_radius(&mut self, env: &MutEnv, corner_radius: i32) {
        crate::port_pending!("PCB_SHAPE::SetCornerRadius", env, corner_radius)
    }

    /// `PCB_SHAPE::SetWidth()`.
    pub fn set_width(&mut self, env: &MutEnv, width: i32) {
        crate::port_pending!("PCB_SHAPE::SetWidth", env, width)
    }

    /// `SetLibStrokeWidth()`.
    pub fn set_lib_stroke_width(&mut self, width: i32) {
        crate::port_pending!("PCB_SHAPE::SetLibStrokeWidth", width)
    }

    /// `PCB_SHAPE::SetStroke()`: `m_stroke = aStroke; SetWidth( aStroke.GetWidth() )`.
    pub fn set_stroke(&mut self, env: &MutEnv, stroke: StrokeParams) {
        crate::port_pending!("PCB_SHAPE::SetStroke", env, stroke)
    }

    /// `PCB_SHAPE::SetShape()`.
    pub fn set_shape(&mut self, env: &MutEnv, shape: ShapeT) {
        crate::port_pending!("PCB_SHAPE::SetShape", env, shape)
    }

    /// `PCB_SHAPE::SetArcGeometry( aStart, aMid, aEnd )`.
    pub fn set_arc_geometry(
        &mut self,
        env: &MutEnv,
        start: Vector2I,
        mid: Vector2I,
        end: Vector2I,
    ) {
        crate::port_pending!("PCB_SHAPE::SetArcGeometry", env, start, mid, end)
    }

    /// `PCB_SHAPE::SetArcAngle()`.
    pub fn set_arc_angle(&mut self, env: &MutEnv, angle: EdaAngle) {
        crate::port_pending!("PCB_SHAPE::SetArcAngle", env, angle)
    }

    /// `PCB_SHAPE::SetBezierC1()`.
    pub fn set_bezier_c1(&mut self, env: &MutEnv, pt: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetBezierC1", env, pt)
    }

    /// `PCB_SHAPE::SetBezierC2()`.
    pub fn set_bezier_c2(&mut self, env: &MutEnv, pt: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetBezierC2", env, pt)
    }

    /// `PCB_SHAPE::SetPolyShape()`.
    pub fn set_poly_shape(&mut self, env: &MutEnv, poly: ShapePolySet) {
        crate::port_pending!("PCB_SHAPE::SetPolyShape", env, poly)
    }

    /// `PCB_SHAPE::SetEllipseCenter()`.
    pub fn set_ellipse_center(&mut self, env: &MutEnv, pt: Vector2I) {
        crate::port_pending!("PCB_SHAPE::SetEllipseCenter", env, pt)
    }

    /// `PCB_SHAPE::SetEllipseMajorRadius()`.
    pub fn set_ellipse_major_radius(&mut self, env: &MutEnv, r: i32) {
        crate::port_pending!("PCB_SHAPE::SetEllipseMajorRadius", env, r)
    }

    /// `PCB_SHAPE::SetEllipseMinorRadius()`.
    pub fn set_ellipse_minor_radius(&mut self, env: &MutEnv, r: i32) {
        crate::port_pending!("PCB_SHAPE::SetEllipseMinorRadius", env, r)
    }

    /// `PCB_SHAPE::SetEllipseRotation()`.
    pub fn set_ellipse_rotation(&mut self, env: &MutEnv, a: EdaAngle) {
        crate::port_pending!("PCB_SHAPE::SetEllipseRotation", env, a)
    }

    /// `PCB_SHAPE::SetEllipseStartAngle()`.
    pub fn set_ellipse_start_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        crate::port_pending!("PCB_SHAPE::SetEllipseStartAngle", env, a)
    }

    /// `PCB_SHAPE::SetEllipseEndAngle()`.
    pub fn set_ellipse_end_angle(&mut self, env: &MutEnv, a: EdaAngle) {
        crate::port_pending!("PCB_SHAPE::SetEllipseEndAngle", env, a)
    }

    /// `PCB_SHAPE::SetIsProxyItem()`.
    pub fn set_is_proxy_item(&mut self, is_proxy: bool) {
        crate::port_pending!("PCB_SHAPE::SetIsProxyItem", is_proxy)
    }
}
