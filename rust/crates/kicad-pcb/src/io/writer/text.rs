// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-05: `format( const PCB_TEXT* )`, `format( const PCB_TEXTBOX* )` and
//! `formatRenderCache` (W:573-598, 2619-2773), plus `EDA_TEXT::Format` (`common/eda_text.cpp`).
//!
//! Entry points for the other packages: [`PcbWriter::format_field`] (the headerless body of a
//! footprint `(property ...)`, PCBIO-06), [`PcbWriter::format_pcb_text_with`] (dimension text,
//! PCBIO-10: `format( static_cast<const PCB_TEXT*>( aDimension ) )`) and
//! [`PcbWriter::format_textbox_with`] (table cells, PCBIO-12).

use kicad_core::eda_units::format_angle;
use kicad_core::string_utils::format_double2_str;
use kicad_core::text_vars::ResolutionContext;
use kicad_core::{Color4D, GrTextHAlignT, GrTextVAlignT, PCB_IU_SCALE};
use kicad_geom::{EdaAngle, Vector2I, ki_round};
use kicad_font::Glyph;
use kicad_sexpr::{IoError, format_bool, format_custom_properties, format_uuid};

use super::{CTL_OMIT_COLOR, CTL_OMIT_HYPERLINK, PcbWriter, format_iu, format_iu_pt};
use crate::board_item;
use crate::eda_shape::ShapeT;
use crate::eda_text::{self, EdaText};
use crate::ids::{FieldId, ItemId, TextId, TextboxId};
use crate::pcb_field::PcbField;
use crate::pcb_text::PcbText;
use crate::pcb_textbox::PcbTextbox;
use crate::traits::{BoardItem, EdaTextItem};

impl PcbWriter<'_> {
    /// Port of `format( const PCB_TEXT* aText )`: `(gr_text ..)` / `(fp_text ..)`.
    pub(crate) fn format_text(&mut self, text: TextId) -> Result<(), IoError> {
        let Some(t) = self.src.text(text) else {
            return Ok(());
        };

        self.format_pcb_text_with(t, t, None, false)
    }

    /// Port of `format( const PCB_TEXT* aText )` for a `PCB_FIELD`: the headerless field body
    /// (`(at ..)`, layer, hide, uuid, effects, render cache, custom properties), only valid
    /// inside the `(property ...)` its caller opened (PCBIO-06).
    pub(crate) fn format_field(&mut self, field: FieldId) -> Result<(), IoError> {
        let Some(f) = self.src.field(field) else {
            return Ok(());
        };

        self.format_pcb_text_with(f, &f.text, Some(f), false)
    }

    /// The body of `format( const PCB_TEXT* aText )`. `host` is the item whose virtuals the
    /// C++ reaches (the text, the field or the dimension), `text` its `PCB_TEXT` part, `field`
    /// set when the item is a `PCB_FIELD` (`Type() == PCB_FIELD_T`) and `is_dimension` when it
    /// is a `PCB_DIMENSION_BASE` (always formatted as `gr_text`).
    pub(crate) fn format_pcb_text_with<H: EdaTextItem + BoardItem + ?Sized>(
        &mut self,
        host: &H,
        text: &PcbText,
        field: Option<&PcbField>,
        is_dimension: bool,
    ) -> Result<(), IoError> {
        let ctx = self.ctx.clone();
        let mut parent_fp = board_item::parent_footprint(text, &ctx);
        let prefix;
        let mut type_ = "";
        let mut pos = host.text_pos(&ctx);

        // Always format dimension text as gr_text
        if is_dimension {
            parent_fp = None;
        }

        if let Some(fp) = parent_fp {
            prefix = "fp";
            type_ = "user";

            pos = fp.transform().inverse_apply(pos);
        } else {
            prefix = "gr";
        }

        if field.is_none() {
            let quoted = self.out.quotew(text.text.text());
            self.out
                .print(format_args!("({prefix}_text {type_} {quoted}"))?;
        }

        if host.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        self.out.print(format_args!(
            "(at {} {})",
            format_iu_pt(pos),
            format_angle(host.text_angle(&ctx).as_degrees())
        ))?;

        if parent_fp.is_some() && !text.text.is_keep_upright() {
            format_bool(self.out, "unlocked", true)?;
        }

        self.format_layer(host.layer(&ctx), host.base().is_knockout)?;

        if let Some(f) = field {
            if !f.text.text.is_visible() {
                format_bool(self.out, "hide", true)?;
            }
        }

        format_uuid(self.out, &host.base().uuid.as_string())?;

        // Currently, texts have no specific color and no hyperlink.
        // so ensure they are never written in kicad_pcb file
        let ctl_flags = CTL_OMIT_COLOR | CTL_OMIT_HYPERLINK;

        if parent_fp.is_some() {
            // Footprint text accessors return board-frame sizes, so format the stored
            // library-frame attributes (EDA_TEXT libText; libText.SetAttributes( .. ))
            let attrs = &text.text.attributes;
            let size = Vector2I::new(attrs.size.0, attrs.size.1);
            let thickness = attrs.stroke_width;
            format_eda_text(
                self,
                &text.text,
                size,
                thickness,
                "",
                ctl_flags,
            )?;
        } else {
            format_eda_text(
                self,
                &text.text,
                host.text_size(&ctx),
                host.text_thickness(&ctx),
                text.text.hyperlink(),
                ctl_flags,
            )?;
        }

        if has_outline_font(host.eda_text()) {
            self.format_render_cache_of(host)?;
        }

        format_custom_properties(self.out, &host.base().custom_properties)?;

        if field.is_none() {
            self.out.print(format_args!(")"))?;
        }

        Ok(())
    }

    /// Port of `format( const PCB_TEXTBOX* aTextBox )`: `(gr_text_box ..)` / `(fp_text_box ..)`.
    pub(crate) fn format_textbox(&mut self, textbox: TextboxId) -> Result<(), IoError> {
        let Some(tb) = self.src.textbox(textbox) else {
            return Ok(());
        };

        self.format_textbox_with(tb, tb, None)
    }

    /// The body of `format( const PCB_TEXTBOX* aTextBox )`. `host` is the item whose virtuals
    /// the C++ reaches (the text box or the table cell), `textbox` its `PCB_TEXTBOX` part and
    /// `cell_span` the `(col_span, row_span)` of a `PCB_TABLECELL` (PCBIO-12).
    pub(crate) fn format_textbox_with<H: EdaTextItem + BoardItem + ?Sized>(
        &mut self,
        host: &H,
        textbox: &PcbTextbox,
        cell_span: Option<(i32, i32)>,
    ) -> Result<(), IoError> {
        let ctx = self.ctx.clone();
        let parent_fp = board_item::parent_footprint(host, &ctx);

        let keyword = if cell_span.is_some() {
            "table_cell"
        } else if parent_fp.is_some() {
            "fp_text_box"
        } else {
            "gr_text_box"
        };
        let quoted = self.out.quotew(textbox.text.text());
        self.out.print(format_args!("({keyword} {quoted}"))?;

        if host.is_locked(&ctx) {
            format_bool(self.out, "locked", true)?;
        }

        // Use the lib-frame shape. The board-frame shape can flip to POLY for
        // rendering when the FP rotation is non-cardinal.
        let lib_shape = textbox.shape.lib_shape();

        if lib_shape == ShapeT::Rectangle {
            self.out.print(format_args!(
                "(start {}) (end {})",
                format_iu_pt(textbox.shape.lib_start()),
                format_iu_pt(textbox.shape.lib_end())
            ))?;
        } else if lib_shape == ShapeT::Poly {
            // Fall back to the runtime polygon if the lib copy was never seeded.
            let lib_poly = textbox.shape.lib_poly();
            let have_lib_poly = lib_poly.outline_count() > 0;
            let poly = if have_lib_poly {
                lib_poly
            } else {
                textbox.shape.shape.poly_shape()
            };

            if poly.outline_count() > 0 {
                self.format_poly_pts(
                    poly.c_outline(0),
                    if have_lib_poly { None } else { parent_fp },
                )?;
            }
        } else {
            // UNIMPLEMENTED_FOR( aTextBox->SHAPE_T_asString() ): an assertion only, nothing is
            // written.
        }

        self.out.print(format_args!(
            "(margins {} {} {} {})",
            format_iu(textbox.margin_left),
            format_iu(textbox.margin_top),
            format_iu(textbox.margin_right),
            format_iu(textbox.margin_bottom)
        ))?;

        if let Some((col_span, row_span)) = cell_span {
            self.out
                .print(format_args!("(span {col_span} {row_span})"))?;
        }

        let mut angle: EdaAngle = host.text_angle(&ctx);

        if let Some(fp) = parent_fp {
            angle -= fp.orientation();
            angle.normalize720();
        }

        if !angle.is_zero() {
            self.out
                .print(format_args!("(angle {})", format_angle(angle.as_degrees())))?;
        }

        self.format_layer(host.layer(&ctx), false)?;

        format_uuid(self.out, &host.base().uuid.as_string())?;

        format_eda_text(
            self,
            &textbox.text,
            host.text_size(&ctx),
            host.text_thickness(&ctx),
            textbox.text.hyperlink(),
            0,
        )?;

        if cell_span.is_none() {
            format_bool(self.out, "border", textbox.border_enabled)?;
            textbox.stroke(&ctx).format(self.out, &PCB_IU_SCALE)?;
        }

        format_bool(self.out, "knockout", host.base().is_knockout)?;

        if has_outline_font(host.eda_text()) {
            self.format_render_cache_of(host)?;
        }

        format_custom_properties(self.out, &host.base().custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }

    /// Port of `formatRenderCache( const EDA_TEXT* aText )`: `(render_cache ..)` of the text
    /// item `text` (any item with an `EDA_TEXT`).
    pub(crate) fn format_render_cache(&mut self, text: ItemId) -> Result<(), IoError> {
        let ctx = self.ctx.clone();

        match ctx.item(text) {
            Some(item) => match item.as_eda_text() {
                Some(host) => self.format_render_cache_of(host),
                None => Ok(()),
            },
            None => Ok(()),
        }
    }

    /// Port of `formatRenderCache( const EDA_TEXT* aText )` on the text's host.
    ///
    /// `GetRenderCache( GetFont(), resolvedText )` regenerates the cache with the outline font
    /// engine when the stored cache does not match; that engine is not ported (`kicad-font`
    /// D1), so the glyphs come from the cache loaded from the file only (`PORT_STATUS.md`).
    pub(crate) fn format_render_cache_of<H: EdaTextItem + ?Sized>(
        &mut self,
        host: &H,
    ) -> Result<(), IoError> {
        let ctx = self.ctx.clone();
        let resolved_text = host.shown_text(&ctx, ResolutionContext::ForCanvas, 0);
        let cache = eda_text::render_cache(host, &ctx, &resolved_text, Vector2I::new(0, 0));

        let quoted = self.out.quotew(&resolved_text);
        self.out.print(format_args!(
            "(render_cache {} {}",
            quoted,
            format_angle(host.draw_rotation(&ctx).as_degrees())
        ))?;

        // CALLBACK_GAL( empty_opts, polygon callback ): outline glyphs are fractured when they
        // have holes and each outline is handed to the callback; SetLineWidth only matters to
        // stroke glyphs, which an outline-font cache does not hold.
        if let Some(cache) = cache {
            for glyph in &cache.glyphs {
                match glyph {
                    Glyph::Outline(outline) => {
                        let mut glyph_poly = outline.poly.clone();

                        if glyph_poly.has_holes() {
                            glyph_poly.fracture(true);
                        }

                        for ii in 0..glyph_poly.outline_count() {
                            self.out.print(format_args!("(polygon"))?;
                            self.format_poly_pts(glyph_poly.c_outline(ii), None)?;
                            self.out.print(format_args!(")"))?;
                        }
                    }
                    Glyph::Stroke(_) => {
                        // Not reachable: render caches hold outline glyphs only.
                        debug_assert!(false, "stroke glyph in a render cache");
                    }
                }
            }
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }
}

/// `aText->GetFont() && aText->GetFont()->IsOutline()`.
fn has_outline_font(text: &EdaText) -> bool {
    text.attributes.font.is_some() && text.draw_font_lookup().is_outline_unavailable()
}

/// Port of `EDA_TEXT::Format( OUTPUTFORMATTER* aFormatter, int aControlBits )`: `(effects
/// (font ..) [(justify ..)] [(href ..)])`. `size` and `thickness` are what the virtual
/// `GetTextSize()` / `GetTextThickness()` return on the formatted object (for footprint text the
/// C++ formats a plain `EDA_TEXT` holding the library-frame attributes), `hyperlink` its
/// `GetHyperlink()`.
fn format_eda_text(
    writer: &mut PcbWriter<'_>,
    text: &EdaText,
    size: Vector2I,
    thickness: i32,
    hyperlink: &str,
    control_bits: i32,
) -> Result<(), IoError> {
    let out = &mut *writer.out;

    out.print(format_args!("(effects"))?;

    out.print(format_args!("(font"))?;

    if text.attributes.font.is_some() && !text.font_name().is_empty() {
        let quoted = out.quotew(text.font_name());
        out.print(format_args!("(face {quoted})"))?;
    }

    // Text size
    out.print(format_args!("(size {} {})", format_iu(size.y), format_iu(size.x)))?;

    if text.line_spacing() != 1.0 {
        out.print(format_args!(
            "(line_spacing {})",
            format_double2_str(text.line_spacing())
        ))?;
    }

    // GetAutoThickness(): GetTextThickness() == 0
    if thickness != 0 {
        out.print(format_args!("(thickness {})", format_iu(thickness)))?;
    }

    if text.is_bold() {
        format_bool(out, "bold", true)?;
    }

    if text.is_italic() {
        format_bool(out, "italic", true)?;
    }

    let color = &text.attributes.color;

    if (control_bits & CTL_OMIT_COLOR) == 0 && *color != Color4D::UNSPECIFIED {
        out.print(format_args!(
            "(color {} {} {} {})",
            ki_round(color.r * 255.0),
            ki_round(color.g * 255.0),
            ki_round(color.b * 255.0),
            format_double2_str(color.a)
        ))?;
    }

    out.print(format_args!(")"))?; // (font

    if text.is_mirrored()
        || text.horiz_justify() != GrTextHAlignT::Center
        || text.vert_justify() != GrTextVAlignT::Center
    {
        out.print(format_args!("(justify"))?;

        if text.horiz_justify() != GrTextHAlignT::Center {
            out.print(format_args!(
                "{}",
                if text.horiz_justify() == GrTextHAlignT::Left {
                    " left"
                } else {
                    " right"
                }
            ))?;
        }

        if text.vert_justify() != GrTextVAlignT::Center {
            out.print(format_args!(
                "{}",
                if text.vert_justify() == GrTextVAlignT::Top {
                    " top"
                } else {
                    " bottom"
                }
            ))?;
        }

        if text.is_mirrored() {
            out.print(format_args!(" mirror"))?;
        }

        out.print(format_args!(")"))?; // (justify
    }

    if (control_bits & CTL_OMIT_HYPERLINK) == 0 && !hyperlink.is_empty() {
        let quoted = out.quotew(hyperlink);
        out.print(format_args!("(href {quoted})"))?;
    }

    out.print(format_args!(")"))?; // (effects
    Ok(())
}
