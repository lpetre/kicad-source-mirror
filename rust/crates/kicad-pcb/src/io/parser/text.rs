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

//! Port of the text part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-05): `parseEDA_TEXT`, `parseRenderCache`, `parsePCB_TEXT`,
//! `parsePCB_TEXT_effects`, `parsePCB_TEXTBOX`, `parseTextBoxContent`, `bakeTextBoxLib`.
//!
//! Text items are built as local values whose `base.parent` is set before parsing (the C++
//! passes the parent to the constructor), so the virtual setters run with the parent frame of
//! [`Board::child_env`](crate::board::Board::child_env) and the getters with a bare
//! [`BoardCtx`] over the board being filled (`board-model.md` 13).
//!
//! Callers of the C++ `parsePCB_TEXT( aParent, aBaseText )`:
//! - board `gr_text`: [`PcbParser::parse_pcb_text`] with `ParentRef::Board` and no base text;
//! - footprint `fp_text` (PCBIO-06): [`PcbParser::parse_pcb_text_item`], which can return a
//!   [`ParsedText::Field`] (legacy `reference` / `value`, hidden user text);
//! - dimension text (PCBIO-10): [`PcbParser::parse_pcb_text`] with the dimension's `PcbText`
//!   as base text (the only `aBaseText` the C++ passes is a `PCB_DIMENSION_BASE`).

use kicad_core::string_utils::convert_to_new_overbar_notation;
use kicad_core::stroke_params::StrokeParamsParser;
use kicad_core::{Color4D, GrTextHAlignT, GrTextVAlignT, LineStyle, PCB_IU_SCALE, StrokeParams};
use kicad_geom::{ANGLE_0, EdaAngle, ShapeLineChain, ShapePolySet, Vector2I};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::keywords::strokeparams_t;

use super::PcbParser;
use crate::ctx::{BoardCtx, MutEnv};
use crate::eda_shape::ShapeT;
use crate::eda_text;
use crate::footprint::Footprint;
use crate::ids::{ItemId, ItemKind};
use crate::item::ParentRef;
use crate::item_ref::AnyItemMut;
use crate::pcb_field::{FieldT, PcbField, get_user_field_name};
use crate::pcb_text::PcbText;
use crate::pcb_textbox::PcbTextbox;
use crate::traits::{BoardItem, BoardItemMut, EdaTextItem};

/// What `parsePCB_TEXT` returns: a `PCB_TEXT`, or a `PCB_FIELD` for footprint text that
/// became a field (legacy `fp_text reference` / `fp_text value`, hidden `fp_text user`).
#[derive(Clone, Debug)]
pub(crate) enum ParsedText {
    Text(PcbText),
    Field(PcbField),
}

/// The footprint `parent` names, if any (`dynamic_cast<FOOTPRINT*>( aParent )`).
fn parent_footprint_id(parent: ParentRef) -> Option<crate::ids::FootprintId> {
    match parent {
        ParentRef::Item(id) if id.kind() == ItemKind::Footprint => id.typed::<Footprint>(),
        _ => None,
    }
}

impl PcbParser {
    /// The `MutEnv` the virtual setters of a not yet added child of `parent` see.
    fn text_env(&self, parent: ParentRef) -> MutEnv {
        match parent {
            ParentRef::None => MutEnv::detached(),
            _ => self.board.child_env(parent),
        }
    }

    /// The `finishItem` step `if( EDA_TEXT* textItem = dynamic_cast<EDA_TEXT*>( aChild ) )
    /// textItem->ResolveFont( embeddedFonts );` for one child: texts, fields, dimensions, text
    /// boxes and table cells (`PCB_BARCODE` is no `EDA_TEXT`). Returns `ResolveFont`'s result,
    /// `false` for a child without text.
    pub(crate) fn resolve_child_font(&mut self, child: ItemId, embedded_fonts: &[String]) -> bool {
        let Some(item) = self.board.get_mut_raw(child) else {
            return false;
        };

        let text = match item {
            AnyItemMut::Text(x) => &mut x.text,
            AnyItemMut::Field(x) => &mut x.text.text,
            AnyItemMut::Dimension(x) => &mut x.text.text,
            AnyItemMut::Textbox(x) => &mut x.text,
            AnyItemMut::Tablecell(x) => &mut x.textbox.text,
            _ => return false,
        };

        text.resolve_font(Some(embedded_fonts))
    }

    /// Port of `parseEDA_TEXT( EDA_TEXT* aText )`: the content of `(effects ...)`; call with
    /// `CurTok() == T_effects`. `text` is the `EDA_TEXT` host (its virtual `SetTextSize` /
    /// `SetTextThickness` and the legacy bold migration go through it).
    pub(crate) fn parse_eda_text<H: EdaTextItem + BoardItem + ?Sized>(
        &mut self,
        text: &mut H,
    ) -> Result<(), IoError> {
        if self.lexer.cur_tok() != T::T_effects {
            debug_assert!(
                false,
                "Cannot parse {} as EDA_TEXT.",
                self.lexer.token_text(self.lexer.cur_tok())
            );
            return Ok(());
        }

        let env = self.text_env(text.base().parent);

        // These are not written out if center/center and/or no mirror,
        // so we have to make sure we start that way.
        // (these parameters will be set in T_justify section, when existing)
        text.eda_text_mut().set_horiz_justify(GrTextHAlignT::Center);
        text.eda_text_mut().set_vert_justify(GrTextVAlignT::Center);
        text.eda_text_mut().set_mirrored(false);

        // In version 20210606 the notation for overbars was changed from `~...~` to `~{...}`.
        // We need to convert the old syntax to the new one.
        if self.required_version < 20210606 {
            let converted = convert_to_new_overbar_notation(text.eda_text().text());
            text.eda_text_mut().set_text(&converted);
        }

        // Prior to v5.0 text size was omitted from file format if equal to 60mils
        // Now, it is always explicitly written to file
        let mut found_text_size = false;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_font => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            match token {
                                T::T_face => {
                                    self.lexer.need_symbol()?;
                                    let face = self.lexer.from_utf8();
                                    text.eda_text_mut().set_unresolved_font_name(&face);
                                    self.lexer.need_right()?;
                                }
                                T::T_size => {
                                    let y = self.parse_board_units_str("text height")?;
                                    let x = self.parse_board_units_str("text width")?;
                                    text.set_text_size(&env, Vector2I::new(x, y), true);
                                    self.lexer.need_right()?;

                                    found_text_size = true;
                                }
                                T::T_line_spacing => {
                                    let spacing = self.parse_double_expected("line spacing")?;
                                    text.eda_text_mut().set_line_spacing(spacing);
                                    self.lexer.need_right()?;
                                }
                                T::T_thickness => {
                                    let thickness = self.parse_board_units_str("text thickness")?;
                                    text.set_text_thickness(&env, thickness);
                                    self.lexer.need_right()?;
                                }
                                T::T_bold => {
                                    let bold = self.parse_maybe_absent_bool(true)?;
                                    text.eda_text_mut().set_bold_flag(bold);
                                }
                                T::T_italic => {
                                    let italic = self.parse_maybe_absent_bool(true)?;
                                    text.eda_text_mut().set_italic_flag(italic);
                                }
                                _ => {
                                    return Err(self.lexer.expecting_str(
                                        "face, size, line_spacing, thickness, bold, or italic",
                                    ));
                                }
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }
                }
                T::T_justify => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            let t = text.eda_text_mut();

                            match token {
                                T::T_left => t.set_horiz_justify(GrTextHAlignT::Left),
                                T::T_right => t.set_horiz_justify(GrTextHAlignT::Right),
                                T::T_top => t.set_vert_justify(GrTextVAlignT::Top),
                                T::T_bottom => t.set_vert_justify(GrTextVAlignT::Bottom),
                                T::T_mirror => t.set_mirrored(true),
                                _ => {
                                    return Err(self
                                        .lexer
                                        .expecting_str("left, right, top, bottom, or mirror"));
                                }
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }
                }
                T::T_hide => {
                    // In older files, the hide token appears bare, and indicates hide==true.
                    // In newer files, it will be an explicit bool in a list like (hide yes)
                    let hide = self.parse_maybe_absent_bool(true)?;
                    text.eda_text_mut().set_visible(!hide);
                }
                _ => return Err(self.lexer.expecting_str("font, justify, or hide")),
            }

            token = self.lexer.next_tok()?;
        }

        // Text size was not specified in file, force legacy default units
        // 60mils is 1.524mm
        if !found_text_size {
            let default_text_size = 1.524 * PCB_IU_SCALE.iu_per_mm;

            // VECTOR2I( double, double ): truncation
            text.set_text_size(
                &env,
                Vector2I::new(default_text_size as i32, default_text_size as i32),
                true,
            );
        }

        if self.required_version < 20260826 {
            let ctx = BoardCtx::bare(&self.board);
            eda_text::migrate_legacy_bold_stroke_width(text, &ctx, &env);
        }

        Ok(())
    }

    /// Port of `parseRenderCache( EDA_TEXT* text )`: `"text" angle (polygon (pts ..) ..) ..)`
    /// after the `render_cache` keyword. The cache is keyed on the text's current font
    /// (`GetFont()`, usually still unresolved here: `finishItem` -> `ResolveFont` re-keys a
    /// cache with glyphs to the resolved font).
    pub(crate) fn parse_render_cache<H: EdaTextItem + ?Sized>(
        &mut self,
        text: &mut H,
    ) -> Result<(), IoError> {
        self.lexer.need_symbol_or_number()?;
        let cache_text = self.lexer.from_utf8();
        let cache_angle = EdaAngle::from_degrees(self.parse_double_expected("render cache angle")?);

        let font = text
            .eda_text()
            .attributes
            .font
            .as_ref()
            .map(|_| text.eda_text().font_ref());

        text.eda_text_mut()
            .setup_render_cache(&cache_text, font, cache_angle, Vector2I::new(0, 0));

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            if token != T::T_polygon {
                return Err(self.lexer.expecting(T::T_polygon));
            }

            let mut poly = ShapePolySet::default();

            token = self.lexer.next_tok()?;

            while token != T::T_RIGHT {
                if token != T::T_LEFT {
                    return Err(self.lexer.expecting(T::T_LEFT));
                }

                token = self.lexer.next_tok()?;

                if token != T::T_pts {
                    return Err(self.lexer.expecting(T::T_pts));
                }

                let mut line_chain = ShapeLineChain::default();

                while self.lexer.next_tok()? != T::T_RIGHT {
                    self.parse_outline_points(&mut line_chain)?;
                }

                line_chain.set_closed(true);

                if poly.outline_count() == 0 {
                    poly.add_outline(&line_chain);
                } else {
                    poly.add_hole(&line_chain, -1);
                }

                token = self.lexer.next_tok()?;
            }

            text.eda_text_mut().add_render_cache_glyph(&poly);

            token = self.lexer.next_tok()?;
        }

        // Not in C++ (package FONT-RC-1): keep the stored glyphs as the stand-in for the
        // outline font engine; the cache itself is cleared by the load path as in C++.
        text.eda_text_mut().keep_file_render_cache();

        Ok(())
    }

    /// Port of `parsePCB_TEXT( BOARD_ITEM* aParent, PCB_TEXT* aBaseText )` for callers that
    /// get a `PCB_TEXT` back: board `gr_text` (`base_text == None`) and dimension text
    /// (`base_text` = the dimension's `PcbText`, already carrying the dimension's parent; the
    /// only `aBaseText` the C++ passes is a `PCB_DIMENSION_BASE`). Footprint text goes
    /// through [`parse_pcb_text_item`](Self::parse_pcb_text_item).
    pub(crate) fn parse_pcb_text(
        &mut self,
        parent: ParentRef,
        base_text: Option<PcbText>,
    ) -> Result<Option<PcbText>, IoError> {
        match self.parse_pcb_text_item(parent, base_text)? {
            ParsedText::Text(text) => Ok(Some(text)),
            ParsedText::Field(_) => Err(IoError::new(
                "parse_pcb_text: footprint field returned; use parse_pcb_text_item",
            )),
        }
    }

    /// Port of `parsePCB_TEXT( BOARD_ITEM* aParent, PCB_TEXT* aBaseText )`: `gr_text` /
    /// `fp_text`, call with `CurTok()` on the keyword.
    pub(crate) fn parse_pcb_text_item(
        &mut self,
        parent: ParentRef,
        base_text: Option<PcbText>,
    ) -> Result<ParsedText, IoError> {
        debug_assert!(
            matches!(self.lexer.cur_tok(), T::T_gr_text | T::T_fp_text),
            "Cannot parse {} as PCB_TEXT.",
            self.lexer.token_text(self.lexer.cur_tok())
        );

        let has_base_text = base_text.is_some();
        let parent_fp = parent_footprint_id(parent);

        let mut token = self.lexer.next_tok()?;

        // The parsed item: a PCB_TEXT, or the PCB_FIELD the constructor switch made.
        let mut field_id: Option<FieldT> = None;

        let mut text: PcbText = match (base_text, parent_fp) {
            (Some(text), _) => text,
            (None, Some(fp)) => {
                let fp_layer = self.board.footprint(fp).map(|f| f.base.layer);

                match token {
                    T::T_reference => field_id = Some(FieldT::Reference),
                    T::T_value => field_id = Some(FieldT::Value),
                    T::T_user => {}
                    _ => {
                        return Err(IoError::new(format!(
                            "Cannot handle footprint text type {}",
                            self.lexer.from_utf8()
                        )));
                    }
                }

                // PCB_FIELD( parentFP, id ) / PCB_TEXT( parentFP ): fresh UUID, footprint text
                // defaults.
                let mut t = PcbText::for_footprint(fp_layer);
                t.base.uuid = kicad_core::Kiid::new(&mut self.board.kiid_generator);
                t.base.parent = parent;

                token = self.lexer.next_tok()?;
                t
            }
            (None, None) => {
                let mut t = PcbText::new(&mut self.board.kiid_generator);
                t.base.parent = parent;
                t
            }
        };

        // Legacy bare locked token
        if token == T::T_locked {
            text.base.is_locked = true;
            token = self.lexer.next_tok()?;
        }

        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) && token != T::T_NUMBER {
            return Err(self.lexer.expecting_str("text value"));
        }

        let value = self
            .lexer
            .from_utf8()
            .replace("%V", "${VALUE}")
            .replace("%R", "${REFERENCE}");
        text.text.set_text(&value);

        self.lexer.need_left()?;

        self.parse_pcb_text_effects(&mut text, has_base_text)?;

        if let Some(fp) = parent_fp {
            // Convert hidden footprint text (which is no longer supported) into a hidden field
            if !text.text.is_visible() && field_id.is_none() && !has_base_text {
                let (field_count, ordinal) = match self.board.footprint(fp) {
                    Some(footprint) => (
                        footprint.fields.len() as i32,
                        footprint.next_field_ordinal(&BoardCtx::bare(&self.board)),
                    ),
                    None => (0, 0),
                };
                let field_name = get_user_field_name(field_count);

                // PCB_FIELD( const PCB_TEXT&, FIELD_T::USER, name ): a new UUID (assignment
                // does not copy it), the next ordinal, everything else copied.
                let mut field_text = text.clone();
                field_text.base.uuid = kicad_core::Kiid::new(&mut self.board.kiid_generator);
                let mut field = PcbField {
                    text: field_text,
                    id: FieldT::User,
                    ordinal: 0,
                    name: field_name,
                    private: false,
                };
                field.set_ordinal(ordinal, FieldT::User);

                return Ok(ParsedText::Field(field));
            }
        } else {
            // Hidden PCB text is no longer supported
            text.text.set_visible(true);
        }

        match field_id {
            // PCB_FIELD( parentFP, REFERENCE | VALUE ): ordinal 0, no name.
            Some(id) => Ok(ParsedText::Field(PcbField {
                text,
                id,
                ordinal: 0,
                name: String::new(),
                private: false,
            })),
            None => Ok(ParsedText::Text(text)),
        }
    }

    /// Port of `parsePCB_TEXT_effects( PCB_TEXT* aText, PCB_TEXT* aBaseText )`: the lists after
    /// the text value; returns whether `(effects ...)` was seen. `base_is_dimension` is
    /// `dynamic_cast<PCB_DIMENSION_BASE*>( aBaseText ) != nullptr`. The parent footprint is the
    /// one of `text.base.parent` (`aText->GetParent()`).
    pub(crate) fn parse_pcb_text_effects(
        &mut self,
        text: &mut PcbText,
        base_is_dimension: bool,
    ) -> Result<bool, IoError> {
        let parent_fp = parent_footprint_id(text.base.parent);
        let env = self.text_env(text.base.parent);
        let mut saw_effects = false;
        let mut has_angle = false; // Old files do not have a angle specified.
        // in this case it is 0 expected to be 0
        let mut has_pos = false;

        // By default, texts in footprints have a locked rotation (i.e. rot = -90 ... 90 deg)
        if parent_fp.is_some() {
            text.text.set_keep_upright(true);
        }

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_at => {
                    has_pos = true;
                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    let pt = Vector2I::new(x, y);

                    if parent_fp.is_some()
                        && self.required_version >= crate::io::FIRST_FP_AFFINE_TRANSFORM
                    {
                        text.set_lib_text_pos(pt);
                    } else {
                        text.set_text_pos(&env, pt);
                    }

                    token = self.lexer.next_tok()?;

                    if self.lexer.cur_tok() == T::T_NUMBER {
                        let angle = EdaAngle::from_degrees(self.parse_double()?);
                        text.set_text_angle(&env, angle);
                        has_angle = true;
                        token = self.lexer.next_tok()?;
                    }

                    // Legacy location of this token; presence implies true
                    if parent_fp.is_some() && self.lexer.cur_tok() == T::T_unlocked {
                        text.text.set_keep_upright(false);
                        token = self.lexer.next_tok()?;
                    }

                    if token != T::T_RIGHT {
                        return Err(self.lexer.expecting(T::T_RIGHT));
                    }
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(text, &env, layer);

                    token = self.lexer.next_tok()?;

                    if token == T::T_knockout {
                        text.base.is_knockout = true;
                        token = self.lexer.next_tok()?;
                    }

                    if token != T::T_RIGHT {
                        return Err(self.lexer.expecting(T::T_RIGHT));
                    }
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    text.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_hide => {
                    // In older files, the hide token appears bare, and indicates hide==true.
                    // In newer files, it will be an explicit bool in a list like (hide yes)
                    let hide = self.parse_maybe_absent_bool(true)?;

                    if parent_fp.is_some() {
                        text.text.set_visible(!hide);
                    } else {
                        return Err(self.lexer.expecting_str(
                            "layer, effects, locked, render_cache, uuid or tstamp",
                        ));
                    }
                }
                T::T_locked => {
                    // Newer list-enclosed locked
                    text.base.is_locked = self.parse_bool()?;
                    self.lexer.need_right()?;
                }
                // Confusingly, "unlocked" is not the opposite of "locked", but refers to "keep
                // upright"
                T::T_unlocked => {
                    if parent_fp.is_some() {
                        let unlocked = self.parse_bool()?;
                        text.text.set_keep_upright(!unlocked);
                    } else {
                        return Err(self
                            .lexer
                            .expecting_str("layer, effects, locked, render_cache or tstamp"));
                    }

                    self.lexer.need_right()?;
                }
                T::T_effects => {
                    saw_effects = true;
                    self.parse_eda_text(text)?;
                }
                T::T_render_cache => {
                    self.parse_render_cache(text)?;
                }
                T::T_custom_property => {
                    self.parse_custom_property_item(text)?;
                }
                _ => {
                    if parent_fp.is_some() {
                        return Err(self.lexer.expecting_str(
                            "layer, hide, effects, locked, render_cache, tstamp, uuid, or \
                             custom_property",
                        ));
                    } else {
                        return Err(self.lexer.expecting_str(
                            "layer, effects, locked, render_cache, tstamp, uuid, or \
                             custom_property",
                        ));
                    }
                }
            }

            token = self.lexer.next_tok()?;
        }

        // If there is no orientation defined, then it is the default value of 0 degrees.
        if !has_angle {
            text.set_text_angle(&env, ANGLE_0);
        }

        if let Some(fp) = parent_fp {
            let (fp_orientation, fp_position) = {
                let ctx = BoardCtx::bare(&self.board);

                match self.board.footprint(fp) {
                    Some(footprint) => (footprint.orientation(), footprint.position(&ctx)),
                    None => (ANGLE_0, Vector2I::default()),
                }
            };

            if !base_is_dimension && self.required_version < crate::io::FIRST_FP_AFFINE_TRANSFORM {
                // Legacy files stored an absolute board-frame angle and an FP-relative
                // position with the parent FP transform un-applied. Rotate and move
                // the text into board coordinates.
                let angle = {
                    let ctx = BoardCtx::bare(&self.board);
                    text.text_angle(&ctx)
                };
                text.set_text_angle(&env, angle - fp_orientation);
                text.rotate(&env, Vector2I::new(0, 0), fp_orientation);

                // Only move offset from parent position if we read a position from the file.
                // These positions are relative to the parent footprint. If we don't have a
                // position then the text defaults to the parent position and moving again will
                // double it.
                if has_pos {
                    text.move_by(&env, fp_position);
                }
            }

            if !base_is_dimension && self.required_version >= crate::io::FIRST_FP_AFFINE_TRANSFORM {
                let (size, auto_thickness, thickness) = {
                    let ctx = BoardCtx::bare(&self.board);
                    (
                        text.text_size(&ctx),
                        eda_text::auto_thickness(text, &ctx),
                        text.text_thickness(&ctx),
                    )
                };

                text.set_lib_text_size(size);

                if !auto_thickness {
                    text.set_lib_text_thickness(thickness);
                }
            }
        }

        // Not in C++ (package FONT-RC-1): the text is now in the state its stored render
        // cache was written for.
        {
            let ctx = BoardCtx::bare(&self.board);
            eda_text::seal_file_render_cache(text, &ctx);
        }

        Ok(saw_effects)
    }

    /// Port of `bakeTextBoxLib( PCB_TEXTBOX* aTextBox )`: lift disk-parsed values into
    /// `PCB_TEXTBOX` lib storage for new format files. Legacy files use a separate compat path
    /// in `parseTextBoxContent`.
    pub(crate) fn bake_text_box_lib(&mut self, textbox: &mut PcbTextbox) -> Result<(), IoError> {
        let parent = BoardItem::base(textbox).parent;
        let env = self.text_env(parent);

        let eda_angle = {
            let ctx = BoardCtx::bare(&self.board);
            eda_text::text_angle(textbox, &ctx)
        };
        textbox.set_lib_text_angle(eda_angle);

        match textbox.shape.shape.shape() {
            ShapeT::Rectangle => {
                let start = textbox.shape.shape.start();
                let end = textbox.shape.shape.end();
                textbox
                    .shape
                    .override_lib_coords(start, end, Vector2I::new(0, 0));
            }
            ShapeT::Poly => {
                let poly = textbox.shape.shape.poly_shape().clone();
                textbox.shape.override_lib_poly(poly);
            }
            _ => {}
        }

        // GetParentFootprint() walks the whole parent chain: a table cell's parent is its
        // table, whose parent is the footprint (`env.parent` is that footprint's frame).
        if env.parent.is_some() {
            textbox.on_footprint_transformed(&env);
        }

        // Sync the EDA_TEXT angle cache to the absolute board angle.
        let angle = {
            let ctx = BoardCtx::bare(&self.board);
            textbox.text_angle(&ctx)
        };
        eda_text::set_text_angle(textbox, &env, angle);

        // Not in C++ (package FONT-RC-1): the text box (or table cell) is now in the state its
        // stored render cache was written for.
        {
            let ctx = BoardCtx::bare(&self.board);
            eda_text::seal_file_render_cache(textbox, &ctx);
        }

        Ok(())
    }

    /// Port of `parsePCB_TEXTBOX( BOARD_ITEM* aParent )`: `gr_text_box` / `fp_text_box`.
    pub(crate) fn parse_pcb_textbox(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbTextbox>, IoError> {
        debug_assert!(
            matches!(self.lexer.cur_tok(), T::T_gr_text_box | T::T_fp_text_box),
            "Cannot parse {} as PCB_TEXTBOX.",
            self.lexer.token_text(self.lexer.cur_tok())
        );

        let mut textbox = PcbTextbox::new(&mut self.board.kiid_generator);
        BoardItem::base_mut(&mut textbox).parent = parent;

        self.parse_text_box_content(&mut textbox)?;
        self.bake_text_box_lib(&mut textbox)?;

        Ok(Some(textbox))
    }

    /// Port of `parseTextBoxContent( PCB_TEXTBOX* aTextBox )` (shared with table cells,
    /// PCBIO-12). `cell_span` is the `PCB_TABLECELL` part: `Some( &mut (col_span, row_span) )`
    /// for a cell, `None` for a text box.
    pub(crate) fn parse_text_box_content_with(
        &mut self,
        textbox: &mut PcbTextbox,
        mut cell_span: Option<&mut (i32, i32)>,
    ) -> Result<(), IoError> {
        let is_cell = cell_span.is_some();
        let env = self.text_env(BoardItem::base(textbox).parent);
        let mut stroke = StrokeParams::new(-1, LineStyle::Solid, Color4D::UNSPECIFIED);
        let mut found_margins = false;

        let mut token = self.lexer.next_tok()?;

        // Legacy locked
        if token == T::T_locked {
            BoardItem::base_mut(textbox).is_locked = true;
            token = self.lexer.next_tok()?;
        }

        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) && token != T::T_NUMBER {
            return Err(self.lexer.expecting_str("text value"));
        }

        let value = self.lexer.from_utf8();
        textbox.text.set_text(&value);

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_locked => {
                    BoardItem::base_mut(textbox).is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_start => {
                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    textbox.set_start(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;

                    self.lexer.need_left()?;
                    token = self.lexer.next_tok()?;

                    if token != T::T_end {
                        return Err(self.lexer.expecting(T::T_end));
                    }

                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    textbox.set_end(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_pts => {
                    textbox.set_shape(&env, ShapeT::Poly);
                    textbox.shape.shape.poly.remove_all_contours();
                    textbox.shape.shape.poly.new_outline();

                    while self.lexer.next_tok()? != T::T_RIGHT {
                        self.parse_outline_points(textbox.shape.shape.poly.outline(0))?;
                    }
                }
                T::T_angle => {
                    // Set the angle of the text only, the coordinates of the box (a polygon)
                    // are already at the right position, and must not be rotated
                    let angle =
                        EdaAngle::from_degrees(self.parse_double_expected("text box angle")?);
                    eda_text::set_text_angle(textbox, &env, angle);
                    self.lexer.need_right()?;
                }
                T::T_stroke => {
                    let parser = StrokeParamsParser::new(PCB_IU_SCALE.iu_per_mm as i32);

                    self.lexer
                        .with_sub_lexer::<strokeparams_t::T, _, _>(|sub| {
                            parser.parse_stroke(sub, &mut stroke)
                        })?;
                }
                T::T_border => {
                    textbox.border_enabled = self.parse_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_margins => {
                    let (left, top, right, bottom) = self.parse_margins()?;
                    textbox.margin_left = left;
                    textbox.margin_top = top;
                    textbox.margin_right = right;
                    textbox.margin_bottom = bottom;
                    found_margins = true;
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(textbox, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_knockout => {
                    BoardItem::base_mut(textbox).is_knockout = self.parse_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_span => {
                    if let Some(span) = cell_span.as_deref_mut() {
                        span.0 = self.parse_int_expected("column span")?;
                        span.1 = self.parse_int_expected("row span")?;
                    } else {
                        return Err(self.lexer.expecting_str(
                            "locked, start, pts, angle, width, stroke, border, margins, \
                             knockout, layer, effects, render_cache, uuid or tstamp",
                        ));
                    }

                    self.lexer.need_right()?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    BoardItem::base_mut(textbox).uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_effects => {
                    self.parse_eda_text(textbox)?;
                }
                T::T_render_cache => {
                    self.parse_render_cache(textbox)?;
                }
                T::T_custom_property => {
                    self.parse_custom_property_item(textbox)?;
                }
                _ => {
                    if is_cell {
                        return Err(self.lexer.expecting_str(
                            "locked, start, pts, angle, width, margins, knockout, layer, \
                             effects, span, render_cache, uuid, tstamp, or custom_property",
                        ));
                    } else {
                        return Err(self.lexer.expecting_str(
                            "locked, start, pts, angle, width, stroke, border, margins, \
                             knockout,layer, effects, render_cache, uuid, tstamp, or \
                             custom_property",
                        ));
                    }
                }
            }

            token = self.lexer.next_tok()?;
        }

        let stroke_width = stroke.width();
        textbox.set_stroke(&env, stroke);

        if self.required_version < 20230825 {
            // compat, we move to an explicit flag
            textbox.border_enabled = stroke_width >= 0;
        }

        if !found_margins {
            let margin = {
                let ctx = BoardCtx::bare(&self.board);
                textbox.legacy_text_margin(&ctx)
            };
            textbox.margin_left = margin;
            textbox.margin_top = margin;
            textbox.margin_right = margin;
            textbox.margin_bottom = margin;
        }

        Ok(())
    }

    /// Port of `parseTextBoxContent( PCB_TEXTBOX* )` for a text box (not a table cell).
    pub(crate) fn parse_text_box_content(
        &mut self,
        textbox: &mut PcbTextbox,
    ) -> Result<(), IoError> {
        self.parse_text_box_content_with(textbox, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::io::parser::ParseOptions;

    /// A parser positioned on the keyword of `item` (`(gr_text ...)` etc.), file version
    /// `version`.
    fn parser_on(item: &str, version: i32) -> PcbParser {
        let mut p = PcbParser::new(item, "", Board::new(), true, ParseOptions::default());
        p.required_version = version;
        p.lexer.need_left().unwrap();
        p.lexer.next_tok().unwrap();
        p
    }

    /// `finishItem` -> `ResolveFont( nullptr )` on a parsed text (the board package calls it
    /// after the whole board).
    fn resolved(mut text: PcbText) -> PcbText {
        text.text.resolve_font(None);
        text
    }

    const CACHED: &str = r#"(at 168.66 96.63 0) (layer "F.SilkS")
            (uuid "098c9e31-c76b-43cb-bd92-64c51eee7bbe")
            (effects (font (face "Ubuntu Sans") (size 0.8 0.7) (thickness 0.16) (bold yes)))
            (render_cache "SCL" 0 (polygon (pts (xy 1 1) (xy 2 1) (xy 2 2)))))"#;

    /// Board text: `parsePCB_TEXT` ends with `SetVisible( true )` ("Hidden PCB text is no
    /// longer supported"), and `EDA_TEXT::SetVisible` clears the render cache, so the C++
    /// drops the cache of every board-level text at load.
    #[test]
    fn board_text_drops_render_cache() {
        let mut p = parser_on(&format!("(gr_text \"SCL\" {CACHED}"), 20241229);
        let text = resolved(p.parse_pcb_text(ParentRef::Board, None).unwrap().unwrap());
        assert!(text.text.render_cache_data().is_none());
        assert_eq!(text.text.font_name(), "Ubuntu Sans");
        assert!(text.text.is_bold());
        assert_eq!(text.text.attributes.size, (700_000, 800_000));
        assert_eq!(text.text.attributes.stroke_width, 160_000);
    }

    /// Port of the parse half of `FootprintUserFieldKeepsAutoThickness`
    /// (`qa/tests/pcbnew/test_footprint_load_save.cpp`): a footprint user field whose effects
    /// carry no thickness keeps auto thickness even when the board default silk thickness is
    /// not zero. The save / reload round trip needs the footprint parser and writer (PCBIO-06).
    #[test]
    fn footprint_user_field_keeps_auto_thickness() {
        let item = "(property (at 0 0 0) (layer \"F.SilkS\") \
                    (uuid \"00000000-0000-0000-0000-000000000001\") \
                    (effects (font (size 1 1))))";
        let mut p = parser_on(item, 20261031);

        // Auto thickness only differs from the board default when that default is not zero
        p.board.design_settings_mut().text_thickness
            [crate::board_design_settings::LAYER_CLASS_SILK] = PCB_IU_SCALE.mm_to_iu(0.15);

        let fp = p.board.create_footprint();
        let mut field = PcbField::new(
            &mut p.board.kiid_generator,
            Some(kicad_core::PcbLayerId::FCu),
            FieldT::User,
            "Label",
        );
        field.text.base.parent = ParentRef::Item(fp.into());
        field.text.text.set_text("BezelSW0");

        p.parse_pcb_text_effects(&mut field.text, false).unwrap();

        let ctx = BoardCtx::bare(&p.board);
        assert!(eda_text::auto_thickness(&field, &ctx));
        assert_eq!(field.text.text.attributes.stroke_width, 0);
        assert_eq!(field.text.base.layer, kicad_core::PcbLayerId::FSilkS);
    }

    /// Footprint text loses its render cache as well: new files end with `SetLibTextSize`
    /// (`EDA_TEXT::SetTextSize` clears it), legacy files with `SetTextAngle` / `Rotate`.
    #[test]
    fn footprint_text_drops_render_cache() {
        for version in [20241229, 20261031] {
            let mut p = parser_on(&format!("(fp_text user \"SCL\" {CACHED}"), version);
            let fp = p.board.create_footprint();
            let parsed = p
                .parse_pcb_text_item(ParentRef::Item(fp.into()), None)
                .unwrap();
            let ParsedText::Text(text) = parsed else {
                panic!("expected a PCB_TEXT");
            };
            let text = resolved(text);
            assert!(text.text.render_cache_data().is_none());
            assert!(text.text.is_keep_upright());
            assert_eq!(text.base.layer, kicad_core::PcbLayerId::FSilkS);
        }
    }

    /// Package FONT-RC-1: the render cache stored in the file survives the load path's
    /// `ClearRenderCache()` as the outline-font stand-in, and `GetRenderCache` returns it while
    /// the text is unchanged.
    #[test]
    fn board_text_keeps_file_glyphs_as_stand_in() {
        let mut p = parser_on(&format!("(gr_text \"SCL\" {CACHED}"), 20241229);
        let mut text = resolved(p.parse_pcb_text(ParentRef::Board, None).unwrap().unwrap());
        let ctx = BoardCtx::bare(&p.board);

        assert!(text.text.render_cache_data().is_none());
        let cache = eda_text::render_cache(&text, &ctx, "SCL", Vector2I::default())
            .expect("stand-in glyphs");
        assert_eq!(cache.glyphs.len(), 1);
        let glyph = cache.glyphs[0].as_outline().unwrap();
        assert_eq!(
            glyph.poly.c_outline(0).c_point(0),
            Vector2I::new(1_000_000, 1_000_000)
        );
        assert_eq!(eda_text::outline_font_fallback(&text, &ctx), None);

        // Another resolved text, offset or angle: the C++ key does not match.
        assert!(eda_text::render_cache(&text, &ctx, "SCX", Vector2I::default()).is_none());
        assert!(eda_text::render_cache(&text, &ctx, "SCL", Vector2I::new(1, 0)).is_none());

        // A move translates the glyphs (EDA_TEXT::Offset on a live cache).
        let env = MutEnv::detached();
        text.move_by(&env, Vector2I::new(10, -20));
        let ctx = BoardCtx::bare(&p.board);
        let cache = eda_text::render_cache(&text, &ctx, "SCL", Vector2I::default()).unwrap();
        let glyph = cache.glyphs[0].as_outline().unwrap();
        assert_eq!(
            glyph.poly.c_outline(0).c_point(0),
            Vector2I::new(1_000_010, 999_980)
        );

        // A size edit (SetTextSize clears the cache in C++, which then regenerates it with
        // the font): no stand-in any more, the text falls back to the stroke font.
        let mut edited = text.clone();
        edited.set_text_size(&env, Vector2I::new(800_000, 800_000), true);
        assert!(eda_text::render_cache(&edited, &ctx, "SCL", Vector2I::default()).is_none());
        assert_eq!(
            eda_text::outline_font_fallback(&edited, &ctx).as_deref(),
            Some("Ubuntu Sans")
        );

        // So do a rotation and a justification change.
        let mut edited = text.clone();
        edited.set_text_angle(&env, EdaAngle::from_degrees(90.0));
        assert!(eda_text::render_cache(&edited, &ctx, "SCL", Vector2I::default()).is_none());
        let mut edited = text.clone();
        edited.text.set_horiz_justify(GrTextHAlignT::Left);
        assert!(eda_text::render_cache(&edited, &ctx, "SCL", Vector2I::default()).is_none());
    }

    /// Package FONT-RC-1: an outline-font text without a stored cache falls back to the
    /// stroke font and is reported.
    #[test]
    fn outline_text_without_cache_is_reported() {
        let item = "(gr_text \"SCL\" (at 1 2 0) (layer \"F.SilkS\") \
                    (effects (font (face \"Ubuntu Sans\") (size 1 1))))";
        let mut p = parser_on(item, 20241229);
        let text = resolved(p.parse_pcb_text(ParentRef::Board, None).unwrap().unwrap());
        let ctx = BoardCtx::bare(&p.board);
        assert!(text.text.file_render_cache().is_none());
        assert_eq!(
            eda_text::outline_font_fallback(&text, &ctx).as_deref(),
            Some("Ubuntu Sans")
        );
    }

    /// Package FONT-RC-1: footprint text keeps its stored glyphs too, for legacy and new
    /// files.
    #[test]
    fn footprint_text_keeps_file_glyphs_as_stand_in() {
        for version in [20241229, 20261031] {
            let mut p = parser_on(&format!("(fp_text user \"SCL\" {CACHED}"), version);
            let fp = p.board.create_footprint();
            let parsed = p
                .parse_pcb_text_item(ParentRef::Item(fp.into()), None)
                .unwrap();
            let ParsedText::Text(text) = parsed else {
                panic!("expected a PCB_TEXT");
            };
            let text = resolved(text);
            let ctx = BoardCtx::bare(&p.board);
            let cache = eda_text::render_cache(&text, &ctx, "SCL", Vector2I::default());
            assert_eq!(cache.map(|c| c.glyphs.len()), Some(1), "version {version}");
        }
    }
}
