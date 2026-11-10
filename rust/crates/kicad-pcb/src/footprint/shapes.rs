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

//! Port of `FOOTPRINT::TransformPadsToPolySet` and `TransformFPShapesToPolySet` of
//! `pcbnew/footprint.cpp` (package M20).

use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{ShapePolySet, Vector2I};

use crate::ctx::BoardCtx;
use crate::footprint::Footprint;
use crate::item::DetachedCopy;
use crate::pad::Pad;
use crate::padstack::PadShape;
use crate::traits::{BoardItem, EdaTextItem};

impl Footprint {
    /// `FOOTPRINT::TransformPadsToPolySet( aBuffer, aLayer, aClearance, aMaxError, aErrorLoc )`:
    /// the flashed pads of `layer` (every unique padstack layer for `UNDEFINED_LAYER`), with
    /// the mask expansion / paste margin added on the mask and paste layers.
    pub fn transform_pads_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        max_error: i32,
        error_loc: ErrorLoc,
    ) {
        let mut process_pad = |pad: &Pad, pad_layer: PcbLayerId| {
            let mut clearance = Vector2I::new(clearance, clearance);

            match layer {
                PcbLayerId::FMask | PcbLayerId::BMask => {
                    clearance.x += pad.solder_mask_expansion(ctx, pad_layer);
                    clearance.y += pad.solder_mask_expansion(ctx, pad_layer);
                }
                PcbLayerId::FPaste | PcbLayerId::BPaste => {
                    clearance += pad.solder_paste_margin(ctx, pad_layer);
                }
                _ => {}
            }

            // Our standard TransformShapeToPolygon() routines can't handle differing x:y
            // clearance values (which get generated when a relative paste margin is used with
            // an oblong pad). So we apply this huge hack and fake a larger pad to run the
            // transform on. Of course being a hack it falls down when dealing with custom shape
            // pads (where the size is only the size of the anchor), so for those we punt and
            // just use clearance.x.

            if (clearance.x < 0 || clearance.x != clearance.y)
                && pad.shape(pad_layer) != PadShape::Custom
            {
                let dummy_size = pad.size(ctx, pad_layer) + clearance + clearance;

                if dummy_size.x <= 0 || dummy_size.y <= 0 {
                    return;
                }

                // PAD dummy( *pad ): same parent, so the board frame follows the footprint.
                let mut dummy = pad.detached_copy();
                let env = ctx.board.child_env(BoardItem::base(pad).parent);
                dummy.set_size(&env, pad_layer, dummy_size);
                dummy.transform_shape_to_polygon(
                    ctx, buffer, pad_layer, 0, max_error, error_loc, false,
                );
            } else {
                pad.transform_shape_to_polygon(
                    ctx,
                    buffer,
                    pad_layer,
                    clearance.x,
                    max_error,
                    error_loc,
                    false,
                );
            }
        };

        for &id in &self.pads {
            let pad = &ctx.board[id];

            if !pad.flash_layer(ctx, layer) {
                continue;
            }

            if layer == PcbLayerId::UndefinedLayer {
                pad.padstack.for_each_unique_layer(|l| process_pad(pad, l));
            } else {
                process_pad(pad, layer);
            }
        }
    }

    /// `FOOTPRINT::TransformFPShapesToPolySet( aBuffer, aLayer, aClearance, aError, aErrorLoc,
    /// aIncludeText, aIncludeShapes, aIncludePrivateItems )` (C++ defaults true, true, false).
    #[allow(clippy::too_many_arguments)]
    pub fn transform_fp_shapes_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        include_text: bool,
        include_shapes: bool,
        include_private_items: bool,
    ) {
        for &id in &self.drawings {
            let Some(item) = ctx.item(id) else { continue };
            let item_layer = item.layer();

            if self.private_layers.contains(item_layer) && !include_private_items {
                continue;
            }

            let on_layer = layer == PcbLayerId::UndefinedLayer || item_layer == layer;

            if item.type_() == KicadT::PcbTextT && include_text && on_layer {
                if let Some(text) = item.as_text() {
                    text.transform_text_to_poly_set(ctx, buffer, clearance, error, error_loc);
                }
            }

            if item.type_() == KicadT::PcbTextboxT && include_text && on_layer {
                if let Some(textbox) = item.as_textbox() {
                    // border
                    if textbox.is_border_enabled() {
                        // textbox->PCB_SHAPE::TransformShapeToPolygon( .. ): the PCB_SHAPE body
                        // (EDA_SHAPE::TransformShapeToPolygon without fill) on the textbox.
                        crate::eda_shape::shape_transform_to_polygon(
                            textbox, ctx, buffer, 0, error, error_loc, false, false,
                        );
                    }

                    // text
                    textbox.transform_text_to_poly_set(ctx, buffer, 0, error, error_loc);
                }
            }

            if (item.type_() == KicadT::PcbShapeT || item.type_() == KicadT::PcbBarcodeT)
                && include_shapes
                && on_layer
            {
                item.as_dyn()
                    .transform_shape_to_poly_set(ctx, buffer, layer, 0, error, error_loc);
            }
        }

        if include_text {
            for &id in &self.fields {
                let field = &ctx.board[id];

                if (layer == PcbLayerId::UndefinedLayer || field.layer(ctx) == layer)
                    && EdaTextItem::eda_text(field).is_visible()
                {
                    field.transform_text_to_poly_set(ctx, buffer, clearance, error, error_loc);
                }
            }
        }
    }
}
