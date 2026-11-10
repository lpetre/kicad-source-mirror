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

//! Port of the shape part of `pcbnew/pad.cpp`: `BuildEffectiveShapes` / `buildEffectiveShape`,
//! `BuildEffectivePolygon`, `GetEffectiveShape`, `GetEffectiveHoleShape`,
//! `GetEffectivePolygon`, `GetBoundingRadius`, `GetBoundingBox` (both overloads),
//! `HasCopperRemoved`, `TransformShapeToPolygon`, `TransformHoleToPolygon`, `getMaskShape`,
//! `MergePrimitivesAsPolygon` and the `HitTest` overloads (package M15). Results are cached in
//! `Pad::caches` (`board-model.md` 9), validated against `PadCacheKey` (parent transform, flip,
//! maximum error) and dropped by `set_dirty()` where C++ calls `SetDirty()`.

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::PcbLayerId;
use kicad_core::layer_ids::is_solder_mask_layer;
use kicad_geom::convert_basic_shapes_to_polygon::{
    transform_circle_to_polygon, transform_oval_to_polygon,
    transform_round_chamfered_rect_to_polygon, transform_trapezoid_to_polygon,
};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::{
    Box2I, Seg, Shape, ShapeCircle, ShapeCompound, ShapeLineChain, ShapeNull, ShapePolySet,
    ShapeRect, ShapeSegment, ShapeSimple, Vector2I, ki_round, rotate_point,
};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::eda_shape::shapes::EdaShapeShapesExt;
use crate::item::SharedShape;
use crate::pad::{Pad, PadCacheKey, PadEffectivePolygons, PadEffectiveShapes, PadMaskShapes};
use crate::padstack::{PadAttrib, PadDrillShape, PadShape};
use crate::traits::Flashing;

/// Index of an `ERROR_LOC` in `PadCaches::polygons` (`m_polyDirty[ aErrorLoc ]`).
fn error_loc_index(error_loc: ErrorLoc) -> usize {
    match error_loc {
        ErrorLoc::Outside => 0,
        ErrorLoc::Inside => 1,
    }
}

impl Pad {
    // ---- caches --------------------------------------------------------------------------

    /// Every input of the pad caches besides the pad itself: the parent footprint's transform
    /// and flip, and `GetMaxError()`.
    fn cache_key(&self, ctx: &BoardCtx<'_>) -> PadCacheKey {
        let fp = self.parent_footprint(ctx);

        PadCacheKey {
            transform: fp.map(|fp| fp.transform()),
            flipped: fp.is_some_and(|fp| fp.flipped),
            max_error: crate::board_item::max_error(self, ctx),
        }
    }

    /// The effective shapes, the hole shape and the bounding box, built by
    /// `BuildEffectiveShapes` when the cache is empty ("dirty") or stale.
    fn effective_shapes(&self, ctx: &BoardCtx<'_>) -> Arc<PadEffectiveShapes> {
        let key = self.cache_key(ctx);

        self.caches
            .shapes
            .get_or_build(|c| c.key == key, || self.build_effective_shapes(ctx, key))
    }

    /// Port of `PAD::BuildEffectiveShapes()`.
    fn build_effective_shapes(&self, ctx: &BoardCtx<'_>, key: PadCacheKey) -> PadEffectiveShapes {
        let mut bounding_box = Box2I::default();
        let mut shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>> = BTreeMap::new();

        // m_maskShapes.clear()
        self.caches.mask_shapes.invalidate();

        self.padstack.for_each_unique_layer(|layer| {
            let layer_shape = self.build_effective_shape(ctx, layer);
            bounding_box.merge(&layer_shape.bbox(0));
            shapes.insert(layer, Arc::new(layer_shape));
        });

        // Hole shape
        let half_size = self.drill_size(ctx) / 2;
        let half_width;
        let mut half_len = Vector2I::new(0, 0);

        if self.padstack.drill().shape == PadDrillShape::Circle {
            half_width = half_size.x;
        } else {
            half_width = half_size.x.min(half_size.y);
            half_len = Vector2I::new(half_size.x - half_width, half_size.y - half_width);
        }

        rotate_point(&mut half_len, self.orientation(ctx));

        let pos = self.position(ctx);
        let hole_shape = Arc::new(ShapeSegment::new(
            pos - half_len,
            pos + half_len,
            half_width * 2,
        ));

        bounding_box.merge(&hole_shape.bbox(0));

        if self.padstack.drill().shape == PadDrillShape::Circle {
            let max_hole = self.padstack.max_hole_size();
            bounding_box.merge(&Box2I::by_center(pos, Vector2I::new(max_hole, max_hole)));
        }

        PadEffectiveShapes {
            key,
            bounding_box,
            shapes,
            hole_shape,
        }
    }

    /// Port of `PAD::buildEffectiveShape( aLayer )`: the effective shape of one padstack layer
    /// (the caller stores it). Shape order inside the compound as C++: the body (circle,
    /// segment, rectangle / polygon, or the anchor of a custom pad), then for a rounded
    /// rectangle the four edge segments, then the custom primitives.
    fn build_effective_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> ShapeCompound {
        let mut compound = ShapeCompound::new();

        let shape_pos = self.shape_pos(ctx, layer); // Fetch only once; rotation involves trig
        let mut effective_shape = self.shape(layer);
        let size = self.size(ctx, layer);
        let orientation = self.orientation(ctx);

        if effective_shape == PadShape::Custom {
            effective_shape = self.anchor_pad_shape(layer);
        }

        match effective_shape {
            PadShape::Circle => {
                compound.add_shape(Box::new(ShapeCircle::new(shape_pos, size.x / 2)));
            }

            PadShape::Oval => {
                if size.x == size.y {
                    // the oval pad is in fact a circle
                    compound.add_shape(Box::new(ShapeCircle::new(shape_pos, size.x / 2)));
                } else {
                    let half_size = size / 2;
                    let half_width = half_size.x.min(half_size.y);
                    let mut half_len =
                        Vector2I::new(half_size.x - half_width, half_size.y - half_width);
                    rotate_point(&mut half_len, orientation);
                    compound.add_shape(Box::new(ShapeSegment::new(
                        shape_pos - half_len,
                        shape_pos + half_len,
                        half_width * 2,
                    )));
                }
            }

            PadShape::Rectangle | PadShape::Trapezoid | PadShape::Roundrect => 'body: {
                let r = if effective_shape == PadShape::Roundrect {
                    self.round_rect_corner_radius(layer)
                } else {
                    0
                };
                let mut half_size = Vector2I::new(size.x / 2, size.y / 2);
                let mut trap_delta = Vector2I::new(0, 0);

                if r != 0 {
                    half_size -= Vector2I::new(r, r);

                    // Avoid degenerated shapes (0 length segments) that always create issues
                    // For roundrect pad very near a circle, use only a circle
                    let min_len = kicad_core::PCB_IU_SCALE.mm_to_iu(0.0001);

                    if half_size.x < min_len && half_size.y < min_len {
                        compound.add_shape(Box::new(ShapeCircle::new(shape_pos, r)));
                        break 'body;
                    }
                } else if effective_shape == PadShape::Trapezoid {
                    trap_delta = self.padstack.trapezoid_delta_size(layer) / 2;
                }

                let mut corners = ShapeLineChain::new();

                corners.append_xy(
                    -half_size.x - trap_delta.y,
                    half_size.y + trap_delta.x,
                    false,
                );
                corners.append_xy(
                    half_size.x + trap_delta.y,
                    half_size.y - trap_delta.x,
                    false,
                );
                corners.append_xy(
                    half_size.x - trap_delta.y,
                    -half_size.y + trap_delta.x,
                    false,
                );
                corners.append_xy(
                    -half_size.x + trap_delta.y,
                    -half_size.y - trap_delta.x,
                    false,
                );

                Shape::rotate(&mut corners, orientation, Vector2I::new(0, 0));
                Shape::move_(&mut corners, shape_pos);

                // GAL renders rectangles faster than 4-point polygons so it's worth checking if
                // our body shape is a rectangle.
                let c = |i: i32| corners.c_point(i);

                if corners.point_count() == 4
                    && ((c(0).y == c(1).y && c(1).x == c(2).x && c(2).y == c(3).y && c(3).x == c(0).x)
                        || (c(0).x == c(1).x
                            && c(1).y == c(2).y
                            && c(2).x == c(3).x
                            && c(3).y == c(0).y))
                {
                    let width = (c(2).x - c(0).x).abs();
                    let height = (c(2).y - c(0).y).abs();
                    let pos = Vector2I::new(c(2).x.min(c(0).x), c(2).y.min(c(0).y));

                    compound.add_shape(Box::new(ShapeRect::from_pos_size(pos, width, height)));
                } else {
                    compound.add_shape(Box::new(ShapeSimple::from_line_chain(&corners)));
                }

                if r != 0 {
                    compound.add_shape(Box::new(ShapeSegment::new(c(0), c(1), r * 2)));
                    compound.add_shape(Box::new(ShapeSegment::new(c(1), c(2), r * 2)));
                    compound.add_shape(Box::new(ShapeSegment::new(c(2), c(3), r * 2)));
                    compound.add_shape(Box::new(ShapeSegment::new(c(3), c(0), r * 2)));
                }
            }

            PadShape::ChamferedRect => {
                let mut outline = ShapePolySet::new();

                transform_round_chamfered_rect_to_polygon(
                    &mut outline,
                    shape_pos,
                    self.size(ctx, layer),
                    orientation,
                    self.round_rect_corner_radius(layer),
                    self.chamfer_rect_ratio(layer),
                    self.chamfer_positions(layer),
                    0,
                    crate::board_item::max_error(self, ctx),
                    ErrorLoc::Inside,
                );

                // C++ reads COutline( 0 ) unconditionally (undefined behaviour on an empty
                // set); a degenerate outline adds nothing here.
                if outline.outline_count() > 0 {
                    compound.add_shape(Box::new(ShapeSimple::from_line_chain(outline.c_outline(0))));
                }
            }

            // wxFAIL_MSG( "PAD::buildEffectiveShapes: Unsupported pad shape" ): no body.
            PadShape::Custom => {}
        }

        if self.shape(layer) == PadShape::Custom {
            for primitive in self.padstack.primitives(layer) {
                if !primitive.is_proxy_item() {
                    for mut shape in primitive.make_effective_shapes_owned(ctx, false) {
                        shape.rotate(orientation, Vector2I::new(0, 0));
                        shape.move_(shape_pos);
                        compound.add_shape(shape);
                    }
                }
            }
        }

        compound
    }

    /// The effective polygons of every unique layer for one `ERROR_LOC`, built by
    /// `BuildEffectivePolygon` when the cache is empty or stale.
    fn effective_polygons(
        &self,
        ctx: &BoardCtx<'_>,
        error_loc: ErrorLoc,
    ) -> Arc<PadEffectivePolygons> {
        let key = self.cache_key(ctx);

        self.caches.polygons[error_loc_index(error_loc)].get_or_build(
            |c| c.key == key,
            || self.build_effective_polygon(ctx, key, error_loc),
        )
    }

    /// Port of `PAD::BuildEffectivePolygon( aErrorLoc )`. The bounding radius is only
    /// computed for `ERROR_OUTSIDE` (C++ keeps it in `m_effectiveBoundingRadius`; the
    /// `ERROR_INSIDE` entry carries 0 and is never read).
    fn build_effective_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        key: PadCacheKey,
        error_loc: ErrorLoc,
    ) -> PadEffectivePolygons {
        // Only calculate this once, not for both ERROR_INSIDE and ERROR_OUTSIDE
        let do_bounding_radius = error_loc == ErrorLoc::Outside;
        let max_error = crate::board_item::max_error(self, ctx);
        let mut polygons: BTreeMap<PcbLayerId, Arc<ShapePolySet>> = BTreeMap::new();
        let mut unique_layers: Vec<PcbLayerId> = Vec::new();

        self.padstack.for_each_unique_layer(|layer| {
            let mut effective_polygon = ShapePolySet::new();
            self.transform_shape_to_polygon(
                ctx,
                &mut effective_polygon,
                layer,
                0,
                max_error,
                error_loc,
                false,
            );
            polygons.insert(layer, Arc::new(effective_polygon));
            unique_layers.push(layer);
        });

        let mut bounding_radius = 0;

        if do_bounding_radius {
            let position = self.position(ctx);

            for layer in &unique_layers {
                let effective_polygon = &polygons[layer];

                for cnt in 0..effective_polygon.outline_count() {
                    let poly = effective_polygon.c_outline(cnt);

                    for ii in 0..poly.point_count() {
                        // KiROUND of the integer EuclideanNorm(): the identity.
                        let dist = (poly.c_point(ii) - position).euclidean_norm();
                        bounding_radius = bounding_radius.max(dist);
                    }
                }
            }

            let drill_size = self.drill_size(ctx);
            bounding_radius = bounding_radius.max(ki_round(f64::from(drill_size.x) / 2.0));
            bounding_radius = bounding_radius.max(ki_round(f64::from(drill_size.y) / 2.0));
        }

        PadEffectivePolygons {
            key,
            polygons,
            bounding_radius,
        }
    }

    // ---- bounding box and radius --------------------------------------------------------

    /// `PAD::GetBoundingBox()`: the effective bounding box (from the shape cache).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        self.effective_shapes(ctx).bounding_box
    }

    /// `PAD::GetBoundingBox( aLayer )`: the bounding box of the effective shape of `layer`
    /// (thermal spokes are built on it). C++ rebuilds `buildEffectiveShape( aLayer )` into the
    /// draw cache; the value stored there is identical to the cached one (or lands under a key
    /// `GetEffectiveShape` never reads), so the port only computes it.
    pub fn bounding_box_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> Box2I {
        self.build_effective_shape(ctx, layer).bbox(0)
    }

    /// `PAD::GetBoundingRadius()`: the largest distance from the pad position to a vertex of
    /// the `ERROR_OUTSIDE` effective polygons, at least the drill radius.
    pub fn bounding_radius(&self, ctx: &BoardCtx<'_>) -> i32 {
        self.effective_polygons(ctx, ErrorLoc::Outside)
            .bounding_radius
    }

    /// `PAD::HasCopperRemoved( aLayer )`: a backdrill / post-machining knockout at least as
    /// large as the pad.
    pub fn has_copper_removed(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let knockout = self.backdrill_or_post_machining_knockout(ctx, layer);

        knockout > 0 && i64::from(knockout) >= 2_i64 * i64::from(self.bounding_radius(ctx))
    }

    // ---- effective shapes ---------------------------------------------------------------

    /// `PAD::GetEffectiveShape( aLayer, aFlash, aUsage )`.
    ///
    /// Edge.Cuts: the hole of a PTH / NPTH pad, else a null shape. A PTH pad not flashed on
    /// `layer`: its hole. Otherwise the cached shape of `Padstack().EffectiveLayerFor( layer )`,
    /// replaced on a solder mask layer with a non-zero expansion by the mask shape, plus the
    /// hole for physical / silk clearance on a backdrilled or post-machined layer.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        if layer == PcbLayerId::EdgeCuts {
            let mut effective_compound = ShapeCompound::new();

            if self.attribute == PadAttrib::Pth || self.attribute == PadAttrib::Npth {
                let hole: SharedShape = self.hole_shape_for(ctx, layer, usage);
                effective_compound.add_shape_shared(&hole);
            } else {
                effective_compound.add_shape(Box::new(ShapeNull::new()));
            }

            return Arc::new(effective_compound);
        }

        if self.attribute == PadAttrib::Pth {
            let flashed = match flash {
                Flashing::NeverFlashed => false,
                Flashing::AlwaysFlashed => true,
                Flashing::Default => self.flash_layer(ctx, layer),
            };

            if !flashed {
                let mut effective_compound = ShapeCompound::new();
                let hole: SharedShape = self.hole_shape_for(ctx, layer, usage);
                effective_compound.add_shape_shared(&hole);
                return Arc::new(effective_compound);
            }
        }

        let draw_cache = self.effective_shapes(ctx);

        // A normal padstack keeps one shape, under ALL_LAYERS, so the cache accepts only the
        // remapped layer.  The machining tests below still use the layer that the caller
        // asked for
        let effective_layer = self.padstack.effective_layer_for(layer);

        // wxCHECK_MSG( ..contains( effectiveLayer ), nullptr, "Missing shape in
        // PAD::GetEffectiveShape" ): C++ returns a null pointer, the port a null shape.
        let Some(cached) = draw_cache.shapes.get(&effective_layer) else {
            debug_assert!(false, "Missing shape in PAD::GetEffectiveShape for layer {effective_layer:?}.");
            return Arc::new(ShapeNull::new());
        };

        let mut shape: SharedShape = cached.clone();

        if is_solder_mask_layer(layer) {
            let expansion = self.solder_mask_expansion(ctx, layer);

            if expansion != 0 {
                shape = self.mask_shape(ctx, layer, expansion);
            }
        }

        // In some cases we want to add in any backdrill or post-machining
        if (usage == DrcConstraintT::PhysicalClearanceConstraint
            || usage == DrcConstraintT::SilkClearanceConstraint)
            && self.is_backdrilled_or_post_machined(ctx, layer)
        {
            let mut effective_compound = ShapeCompound::new();
            effective_compound.add_shape_shared(&shape);
            let hole: SharedShape = self.hole_shape_for(ctx, layer, usage);
            effective_compound.add_shape_shared(&hole);
            return Arc::new(effective_compound);
        }

        shape
    }

    /// `GetEffectiveHoleShape( aLayer, aUsage )` as a shape pointer (always present for a pad).
    fn hole_shape_for(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, usage: DrcConstraintT) -> SharedShape {
        self.hole_segment(ctx, layer, usage)
    }

    /// Port of the body of `PAD::GetEffectiveHoleShape( aLayer, aUsage )`.
    fn hole_segment(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Arc<ShapeSegment> {
        let draw_cache = self.effective_shapes(ctx);

        let machined = |usage_matches: bool| {
            usage_matches && self.is_backdrilled_or_post_machined(ctx, layer)
        };

        if usage == DrcConstraintT::HoleToHoleConstraint
            || machined(usage == DrcConstraintT::HoleClearanceConstraint)
            || machined(usage == DrcConstraintT::AnnularWidthConstraint)
            || machined(usage == DrcConstraintT::PhysicalClearanceConstraint)
            || machined(usage == DrcConstraintT::SilkClearanceConstraint)
        {
            let max_hole_size = self.padstack.max_hole_size();

            // If it's bigger than the pad's hole, return it
            if max_hole_size > draw_cache.hole_shape.get_width() {
                let position = self.position(ctx);
                return Arc::new(ShapeSegment::new(position, position, max_hole_size));
            }
        }

        draw_cache.hole_shape.clone()
    }

    /// `PAD::GetEffectiveHoleShape( aLayer, aUsage )`: the drilled hole as a segment (a
    /// zero-length one for a round hole); for hole-to-hole checks, and for hole / annular /
    /// physical / silk checks on a backdrilled or post-machined layer, the largest hole of the
    /// padstack if it is bigger. Never `None` for a pad.
    pub fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        Some(self.hole_segment(ctx, layer, usage))
    }

    /// `PAD::GetEffectivePolygon( aLayer, aErrorLoc )`: the cached polygon of
    /// `Padstack().EffectiveLayerFor( aLayer )`, built with `GetMaxError()`.
    pub fn effective_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        error_loc: ErrorLoc,
    ) -> Arc<ShapePolySet> {
        let polygons = self.effective_polygons(ctx, error_loc);
        let layer = self.padstack.effective_layer_for(layer);

        // C++ `m_effectivePolygons.at( aLayer )` throws for a layer without an entry, which
        // EffectiveLayerFor never yields; the port answers an empty set instead of panicking.
        match polygons.polygons.get(&layer) {
            Some(polygon) => polygon.clone(),
            None => Arc::new(ShapePolySet::new()),
        }
    }

    /// `PAD::getMaskShape( aLayer, aExpansion )`: the pad shape on `layer` inflated by
    /// `aExpansion` (`ERROR_OUTSIDE`, `GetMaxError()`), one `SHAPE_SIMPLE` per outline. Cached
    /// per layer together with the expansion it was built for (`m_maskShapes`).
    pub fn mask_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        expansion: i32,
    ) -> Arc<ShapeCompound> {
        let key = self.cache_key(ctx);
        let cached = self.caches.mask_shapes.peek().filter(|c| c.key == key);

        if let Some(cached) = &cached
            && let Some((cached_expansion, shape)) = cached.by_layer.get(&layer)
            && *cached_expansion == expansion
        {
            return shape.clone();
        }

        let mut aperture = ShapePolySet::new();
        self.transform_shape_to_polygon(
            ctx,
            &mut aperture,
            layer,
            expansion,
            crate::board_item::max_error(self, ctx),
            ErrorLoc::Outside,
            false,
        );

        let mut shape = ShapeCompound::new();

        for ii in 0..aperture.outline_count() {
            shape.add_shape(Box::new(ShapeSimple::from_line_chain(aperture.c_outline(ii))));
        }

        let shape = Arc::new(shape);
        let mut by_layer = cached.map(|c| c.by_layer.clone()).unwrap_or_default();
        by_layer.insert(layer, (expansion, shape.clone()));
        self.caches
            .mask_shapes
            .store(PadMaskShapes { key, by_layer });

        shape
    }

    // ---- polygons -----------------------------------------------------------------------

    /// `PAD::TransformShapeToPolygon( aBuffer, aLayer, aClearance, aMaxError, aErrorLoc,
    /// ignoreLineWidth )`: appends the pad shape of `layer` inflated by `clearance` (C++
    /// ignores `ignoreLineWidth`).
    #[allow(clippy::too_many_arguments)]
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        _ignore_line_width: bool,
    ) {
        debug_assert!(
            layer != PcbLayerId::UndefinedLayer,
            "UNDEFINED_LAYER is no longer allowed for PAD::TransformShapeToPolygon"
        );

        let mut clearance = clearance;
        let max_error = error;

        // minimal segment count to approximate a circle to create the polygonal pad shape
        // This minimal value is mainly for very small pads, like SM0402.
        // Most of time pads are using the segment count given by aError value.
        const PAD_MIN_SEG_PER_CIRCLE_COUNT: i32 = 16;
        let pad_size = self.size(ctx, layer);
        let dx = pad_size.x / 2;
        let dy = pad_size.y / 2;

        // Note: for pad having a shape offset, the pad position is NOT the shape position
        let pad_shape_pos = self.shape_pos(ctx, layer);
        let shape = self.shape(layer);

        match shape {
            PadShape::Circle | PadShape::Oval => {
                // Note: dx == dy is not guaranteed for circle pads in legacy boards
                if dx == dy || shape == PadShape::Circle {
                    transform_circle_to_polygon(
                        buffer,
                        pad_shape_pos,
                        dx + clearance,
                        max_error,
                        error_loc,
                        PAD_MIN_SEG_PER_CIRCLE_COUNT,
                    );
                } else {
                    let half_width = dx.min(dy);
                    let mut delta = Vector2I::new(dx - half_width, dy - half_width);

                    rotate_point(&mut delta, self.orientation(ctx));

                    transform_oval_to_polygon(
                        buffer,
                        pad_shape_pos - delta,
                        pad_shape_pos + delta,
                        (half_width + clearance) * 2,
                        max_error,
                        error_loc,
                        PAD_MIN_SEG_PER_CIRCLE_COUNT,
                    );
                }
            }

            PadShape::Trapezoid | PadShape::Rectangle => {
                let trap_delta = self.padstack.trapezoid_delta_size(layer);
                let ddx = if shape == PadShape::Trapezoid {
                    trap_delta.x / 2
                } else {
                    0
                };
                let ddy = if shape == PadShape::Trapezoid {
                    trap_delta.y / 2
                } else {
                    0
                };

                let mut outline = ShapePolySet::new();
                transform_trapezoid_to_polygon(
                    &mut outline,
                    pad_shape_pos,
                    pad_size,
                    self.orientation(ctx),
                    ddx,
                    ddy,
                    clearance,
                    max_error,
                    error_loc,
                );
                buffer.append_set(&outline);
            }

            PadShape::ChamferedRect | PadShape::Roundrect => {
                let do_chamfer = shape == PadShape::ChamferedRect;

                let mut outline = ShapePolySet::new();
                transform_round_chamfered_rect_to_polygon(
                    &mut outline,
                    pad_shape_pos,
                    pad_size,
                    self.orientation(ctx),
                    self.round_rect_corner_radius(layer),
                    if do_chamfer {
                        self.chamfer_rect_ratio(layer)
                    } else {
                        0.0
                    },
                    if do_chamfer {
                        self.chamfer_positions(layer)
                    } else {
                        0
                    },
                    clearance,
                    max_error,
                    error_loc,
                );
                buffer.append_set(&outline);
            }

            PadShape::Custom => {
                let mut outline = ShapePolySet::new();
                self.merge_primitives_as_polygon(ctx, layer, &mut outline, error_loc);
                Shape::rotate(&mut outline, self.orientation(ctx), Vector2I::new(0, 0));
                Shape::move_(&mut outline, pad_shape_pos);

                if clearance > 0 || error_loc == ErrorLoc::Outside {
                    if error_loc == ErrorLoc::Outside {
                        clearance += max_error;
                    }

                    outline.inflate(clearance, CornerStrategy::RoundAllCorners, max_error, false);
                    outline.fracture(true);
                } else if clearance < 0 {
                    // Negative clearances are primarily for drawing solder paste layer, so we
                    // don't worry ourselves overly about which side the error is on.

                    // aClearance is negative so this is actually a deflate
                    outline.inflate(clearance, CornerStrategy::AllowAcuteCorners, max_error, false);
                    outline.fracture(true);
                }

                buffer.append_set(&outline);
            }
        }
    }

    /// `PAD::TransformHoleToPolygon( aBuffer, aClearance, aError, aErrorLoc )`: appends the
    /// hole (as an oval of the hole width + 2 * clearance); false if the pad has no hole.
    pub fn transform_hole_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) -> bool {
        let drillsize = self.drill_size(ctx);

        if drillsize.x == 0 || drillsize.y == 0 {
            return false;
        }

        let slot = self.hole_segment(ctx, PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint);

        transform_oval_to_polygon(
            buffer,
            slot.get_seg().a,
            slot.get_seg().b,
            slot.get_width() + clearance * 2,
            error,
            error_loc,
            0,
        );

        true
    }

    /// `PAD::MergePrimitivesAsPolygon( aLayer, aMergedPolygon, aErrorLoc )`: the anchor shape
    /// (always at 0,0; rectangle or circle) merged with the polygons of the non-proxy custom
    /// primitives of `layer`, in the pad frame, built with `GetMaxError()`.
    pub fn merge_primitives_as_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        merged_polygon: &mut ShapePolySet,
        error_loc: ErrorLoc,
    ) {
        merged_polygon.remove_all_contours();

        let max_error = crate::board_item::max_error(self, ctx);

        // Add the anchor pad shape in aMergedPolygon, others in aux_polyset:
        // The anchor pad is always at 0,0
        let pad_size = self.size(ctx, layer);

        match self.anchor_pad_shape(layer) {
            PadShape::Rectangle => {
                let rect = ShapeRect::from_xywh(
                    -pad_size.x / 2,
                    -pad_size.y / 2,
                    pad_size.x,
                    pad_size.y,
                );
                merged_polygon.add_outline(&rect.outline());
            }

            _ => {
                transform_circle_to_polygon(
                    merged_polygon,
                    Vector2I::new(0, 0),
                    pad_size.x / 2,
                    max_error,
                    error_loc,
                    0,
                );
            }
        }

        let mut polyset = ShapePolySet::new();

        for primitive in self.padstack.primitives(layer) {
            if !primitive.is_proxy_item() {
                primitive.transform_shape_to_polygon(
                    ctx,
                    &mut polyset,
                    PcbLayerId::UndefinedLayer,
                    0,
                    max_error,
                    error_loc,
                    false,
                );
            }
        }

        polyset.simplify();

        // Merge all polygons with the initial pad anchor shape
        if polyset.outline_count() > 0 {
            merged_polygon.boolean_add(&polyset);
            merged_polygon.fracture(true);
        }
    }

    // ---- hit tests ----------------------------------------------------------------------

    /// `PAD::HitTest( aPosition, aAccuracy, aLayer )`.
    pub fn hit_test_layer(
        &self,
        ctx: &BoardCtx<'_>,
        pos: Vector2I,
        accuracy: i32,
        layer: PcbLayerId,
    ) -> bool {
        if !self.is_on_layer(ctx, layer) {
            return false;
        }

        let delta = pos - self.position(ctx);
        let bounding_radius = self.bounding_radius(ctx) + accuracy;

        if delta.squared_euclidean_norm() > Seg::square(bounding_radius) {
            return false;
        }

        self.effective_polygon(ctx, layer, ErrorLoc::Inside)
            .contains(pos, -1, accuracy, false)
    }

    /// `PAD::HitTest( aPosition, aAccuracy )`.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        let delta = pos - self.position(ctx);
        let bounding_radius = self.bounding_radius(ctx) + accuracy;

        if delta.squared_euclidean_norm() > Seg::square(bounding_radius) {
            return false;
        }

        let mut contains = false;

        self.padstack.for_each_unique_layer(|l| {
            if contains {
                return;
            }

            if self
                .effective_polygon(ctx, l, ErrorLoc::Inside)
                .contains(pos, -1, accuracy, false)
            {
                contains = true;
            }
        });

        contains |= self
            .hole_segment(ctx, PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint)
            .collide_point(pos, accuracy, None, None);

        contains
    }

    /// `PAD::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.normalize();
        arect.inflate(accuracy);

        let bbox = self.bounding_box(ctx);

        if contained {
            return arect.contains_box(&bbox);
        }

        // Fast test: if aRect is outside the polygon bounding box,
        // rectangles cannot intersect
        if !arect.intersects(&bbox) {
            return false;
        }

        let mut hit = false;

        self.padstack.for_each_unique_layer(|layer| {
            if hit {
                return;
            }

            let poly = self.effective_polygon(ctx, layer, ErrorLoc::Inside);
            let count = poly.total_vertices();

            for ii in 0..count {
                let vertex = poly.c_vertex_global(ii);
                let vertex_next = poly.c_vertex_global((ii + 1) % count);

                // Test if the point is within aRect
                if arect.contains(vertex) {
                    hit = true;
                    break;
                }

                // Test if this edge intersects aRect
                if arect.intersects_segment(vertex, vertex_next) {
                    hit = true;
                    break;
                }
            }
        });

        if !hit {
            let rect = ShapeRect::from_box(&arect);
            hit |= self
                .hole_segment(ctx, PcbLayerId::UndefinedLayer, DrcConstraintT::NullConstraint)
                .collide(&rect, 0, None, None);
        }

        hit
    }
}
