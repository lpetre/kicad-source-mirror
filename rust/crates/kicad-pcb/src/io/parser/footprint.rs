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

//! Port of the footprint part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-06): `parseFOOTPRINT`, `parseFOOTPRINT_unchecked`, `parse3DModel`,
//! `parseFootprintStackup`, `parseFootprintVariant` (P:594-704, 954-1066, 6234-7317).
//!
//! The footprint is created in place (`Board::create_footprint`, `board-model.md` 8.1 / 13),
//! filled with `footprint_add` (children in file order, `ADD_MODE::APPEND`) and returned
//! unattached; the caller attaches it (`attach_footprint( fp, BulkAppend )` in `parseBOARD`).
//! Placement tokens (`at`, `transform`, `layer`) go through the placement cascades
//! (`Board::footprint_set_position` etc., M19), so children read before them are moved like
//! the C++ moves them.
//!
//! Item parsers of other packages are called with the footprint as parent; while one of them
//! is a staging stub (`ParseOptions::allow_pending`) its construct is skipped and nothing is
//! added.

use std::collections::BTreeSet;

use kicad_core::{Color4D, LibId, Lset, PcbLayerId};
use kicad_geom::{ANGLE_0, EdaAngle, Vector2I};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use super::text::ParsedText;
use crate::board::AddMode;
use crate::ctx::BoardCtx;
use crate::footprint::footprint_attr::{
    FP_BOARD_ONLY, FP_DNP, FP_EXCLUDE_FROM_BOM, FP_EXCLUDE_FROM_POS_FILES, FP_EXCLUDE_FROM_SIM,
    FP_SMD, FP_THROUGH_HOLE,
};
use crate::footprint::{Footprint, FootprintStackup, Fp3dmodel, FpUnitInfo, JumperGroup};
use crate::ids::{Body3dId, FootprintId, ItemId};
use crate::io::{FIRST_FP_AFFINE_TRANSFORM, LEGACY_NET_TIES, SEXPR_FOOTPRINT_LIB_FILE_VERSION};
use crate::item::ParentRef;
use crate::item_ref::OwnedItem;
use crate::padstack::PadAttrib;
use crate::pcb_3d_body::{ExtrusionMaterial, Pcb3dBody};
use crate::pcb_field::{FieldT, PcbField};
use crate::traits::BoardItemMut;
use crate::zone_settings::ZoneConnection;

/// `(ZONE_CONNECTION) aValue` for the values the enum has. The C++ cast keeps any other
/// integer, which the Rust enum cannot hold: such a value reads as `INHERITED`
/// (`PORT_STATUS.md`, PCBIO-06 deviations).
fn zone_connection_from_int(value: i32) -> ZoneConnection {
    match value {
        0 => ZoneConnection::None,
        1 => ZoneConnection::Thermal,
        2 => ZoneConnection::Full,
        3 => ZoneConnection::ThtThermal,
        _ => ZoneConnection::Inherited,
    }
}

impl PcbParser {
    /// The footprint being parsed (created by this parser, so always live).
    fn fp_mut(&mut self, fp: FootprintId) -> &mut Footprint {
        self.board
            .footprints
            .get_mut(fp)
            .expect("footprint created by the parser")
    }

    /// `FOOTPRINT::Add( aItem, ADD_MODE::APPEND, true )` of a parsed child. The C++ never
    /// refuses the kinds the parser hands it; a refusal here is a programmer error.
    fn add_fp_child(&mut self, fp: FootprintId, item: OwnedItem) -> Option<ItemId> {
        match self.board.footprint_add(fp, item, AddMode::Append) {
            Ok(id) => Some(id),
            Err(_) => {
                debug_assert!(false, "FOOTPRINT::Add refused a parsed child");
                None
            }
        }
    }

    /// Port of `parseFOOTPRINT( wxArrayString* aInitialComments )`: `parseFOOTPRINT_unchecked`
    /// with a `PARSE_ERROR` of a too recent footprint turned into a `FUTURE_FORMAT_ERROR`.
    ///
    /// Returns the created, unattached footprint; `None` only when the C++ `wxCHECK` fails
    /// (not positioned on `footprint` / `module`).
    pub(crate) fn parse_footprint(
        &mut self,
        initial_comments: Option<Vec<String>>,
    ) -> Result<Option<FootprintId>, IoError> {
        match self.parse_footprint_unchecked(initial_comments) {
            Ok(fp) => Ok(fp),
            Err(e) if e.as_parse_error().is_some() && self.too_recent => {
                let required = self.get_required_version()?;

                match e.to_future_format_error(&required, "") {
                    Some(future) => Err(future.into()),
                    None => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Port of `parseFOOTPRINT_unchecked( wxArrayString* aInitialComments )`.
    fn parse_footprint_unchecked(
        &mut self,
        initial_comments: Option<Vec<String>>,
    ) -> Result<Option<FootprintId>, IoError> {
        let cur = self.lexer.cur_tok();

        if cur != T::T_module && cur != T::T_footprint {
            debug_assert!(
                false,
                "Cannot parse {} as FOOTPRINT.",
                self.lexer.token_string(cur)
            );
            return Ok(None);
        }

        let mut fpid = LibId::new();
        let mut attributes: i32 = 0;
        let mut parsed_scale_x: f64 = 1.0;
        let mut parsed_scale_y: f64 = 1.0;

        let fp = self.board.create_footprint();
        let parent = ParentRef::Item(fp.into());

        self.fp_mut(fp).initial_comments = initial_comments;

        if self.board_present {
            let none = self
                .board
                .component_class_manager
                .none_component_class()
                .clone();
            self.fp_mut(fp).static_component_class = Some(none);
        }

        let mut token = self.lexer.next_tok()?;

        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) && token != T::T_NUMBER {
            return Err(self.lexer.expecting_str("symbol|number"));
        }

        let name = self.lexer.from_utf8();

        if !name.is_empty() && fpid.parse(&name, true) >= 0 {
            return Err(IoError::new(format!(
                "Invalid footprint ID in\nfile: {}\nline: {}\noffset: {}.",
                self.lexer.cur_source(),
                self.lexer.cur_line_number(),
                self.lexer.cur_offset()
            )));
        }

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_version => {
                    // Theoretically a footprint nested in a PCB could declare its own version,
                    // though as of writing this comment we don't do that. Just in case, take
                    // the greater version.
                    let expected = self.lexer.from_utf8();
                    let this_version = self.parse_int_expected(&expected)?;
                    self.lexer.need_right()?;
                    self.required_version = std::cmp::max(self.required_version, this_version);
                    self.too_recent = self.required_version > SEXPR_FOOTPRINT_LIB_FILE_VERSION;
                    // Bar token is known from this version
                    self.lexer.set_knows_bar(self.required_version >= 20240706);
                    self.fp_mut(fp).file_format_version_at_load = this_version;
                }
                T::T_generator => {
                    // We currently ignore the generator when parsing. It is included in the
                    // file for manual indication of where the footprint came from.
                    self.lexer.need_symbol()?;
                    self.lexer.need_right()?;
                }
                T::T_generator_version => {
                    self.lexer.need_symbol()?;
                    self.generator_version = self.lexer.from_utf8();
                    self.lexer.need_right()?;

                    // If the format includes a generator version, by this point we have enough
                    // info to do the version check here
                    if self.required_version > SEXPR_FOOTPRINT_LIB_FILE_VERSION {
                        return Err(kicad_sexpr::FutureFormatError::new(
                            &self.required_version.to_string(),
                            &self.generator_version,
                        )
                        .into());
                    }
                }
                T::T_locked => {
                    let locked = self.parse_maybe_absent_bool(true)?;
                    self.fp_mut(fp).base.is_locked = locked;
                }
                T::T_placed => {
                    let placed = self.parse_maybe_absent_bool(true)?;
                    self.fp_mut(fp).set_is_placed(placed);
                }
                T::T_layer => {
                    // Footprints can be only on the front side or the back side.
                    // but because we can find some stupid layer in file, ensure a
                    // acceptable layer is set for the footprint
                    let layer = self.parse_board_item_layer()?;
                    let side = if layer == PcbLayerId::BCu {
                        PcbLayerId::BCu
                    } else {
                        PcbLayerId::FCu
                    };
                    self.board.footprint_set_layer(fp, side);
                    self.lexer.need_right()?;
                }
                T::T_stackup => self.parse_footprint_stackup(fp)?,
                T::T_tedit => {
                    self.parse_hex()?;
                    self.lexer.need_right()?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    let uuid = self.cur_str_to_kiid();
                    // SetUuidDirect: the footprint is not indexed yet.
                    self.fp_mut(fp).base.uuid = uuid;
                    self.lexer.need_right()?;
                }
                T::T_at => {
                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    self.board.footprint_set_position(fp, Vector2I::new(x, y));
                    token = self.lexer.next_tok()?;

                    if token == T::T_NUMBER {
                        let angle = EdaAngle::from_degrees(self.parse_double()?);
                        self.board.footprint_set_orientation(fp, angle);
                        self.lexer.need_right()?;
                    } else if token != T::T_RIGHT {
                        return Err(self.lexer.expecting(T::T_RIGHT));
                    }
                }
                T::T_transform => {
                    let mut trs_translate = Vector2I::new(0, 0);
                    let mut trs_rotate = ANGLE_0;

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_translate => {
                                trs_translate.x = self.parse_board_units_str("translate X")?;
                                trs_translate.y = self.parse_board_units_str("translate Y")?;
                                self.lexer.need_right()?;
                            }
                            T::T_rotate => {
                                trs_rotate = EdaAngle::from_degrees(
                                    self.parse_double_expected("rotate angle")?,
                                );
                                self.lexer.need_right()?;
                            }
                            T::T_scale => {
                                parsed_scale_x = self.parse_double_expected("scale X")?;
                                parsed_scale_y = self.parse_double_expected("scale Y")?;

                                // Guard against corrupt files: a zero, negative, or non-finite
                                // scale would make the footprint transform degenerate.
                                if !parsed_scale_x.is_finite() || parsed_scale_x <= 0.0 {
                                    parsed_scale_x = 1.0;
                                }

                                if !parsed_scale_y.is_finite() || parsed_scale_y <= 0.0 {
                                    parsed_scale_y = 1.0;
                                }

                                self.lexer.need_right()?;
                            }
                            _ => {
                                return Err(self
                                    .lexer
                                    .expecting_str("translate, rotate, or scale"));
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }

                    self.board.footprint_set_position(fp, trs_translate);
                    self.board.footprint_set_orientation(fp, trs_rotate);
                    self.board
                        .footprint_set_transform_scale(fp, parsed_scale_x, parsed_scale_y);
                }
                T::T_descr => {
                    // some symbols can be 0508, so a number is also a symbol here
                    self.lexer.need_symbol_or_number()?;
                    let descr = self.lexer.from_utf8();
                    self.fp_mut(fp).lib_description = descr;
                    self.lexer.need_right()?;
                }
                T::T_tags => {
                    // some symbols can be 0508, so a number is also a symbol here
                    self.lexer.need_symbol_or_number()?;
                    let tags = self.lexer.from_utf8();
                    self.fp_mut(fp).keywords = tags;
                    self.lexer.need_right()?;
                }
                T::T_height => {
                    let height = self.parse_board_units_str("footprint height")?;
                    self.fp_mut(fp).height = height;
                    self.lexer.need_right()?;
                }
                T::T_property => self.parse_footprint_property(fp)?,
                T::T_path => {
                    // Paths can be numerical so a number is also a symbol here
                    self.lexer.need_symbol_or_number()?;
                    let text = self.lexer.from_utf8();
                    let path = kicad_core::KiidPath::from_string_or_random(
                        &text,
                        &mut self.board.kiid_generator,
                    );
                    self.fp_mut(fp).path = path;
                    self.lexer.need_right()?;
                }
                T::T_sheetname => {
                    self.lexer.need_symbol()?;
                    let sheetname = self.lexer.from_utf8();
                    self.fp_mut(fp).sheetname = sheetname;
                    self.lexer.need_right()?;
                }
                T::T_sheetfile => {
                    self.lexer.need_symbol()?;
                    let sheetfile = self.lexer.from_utf8();
                    self.fp_mut(fp).sheetfile = sheetfile;
                    self.lexer.need_right()?;
                }
                T::T_units => self.parse_footprint_units(fp)?,
                T::T_autoplace_cost90 | T::T_autoplace_cost180 => {
                    self.parse_int_expected("legacy auto-place cost")?;
                    self.lexer.need_right()?;
                }
                T::T_private_layers => {
                    let mut private_layers = Lset::new();

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        match self.layer_indices.get(self.lexer.cur_str()) {
                            Some(&layer) => {
                                private_layers.set(layer);
                            }
                            None => return Err(self.lexer.expecting_str("layer name")),
                        }

                        token = self.lexer.next_tok()?;
                    }

                    if self.required_version < 20220427 {
                        private_layers.set_value(PcbLayerId::EdgeCuts, false);
                        private_layers.set_value(PcbLayerId::Margin, false);
                    }

                    self.fp_mut(fp).private_layers = private_layers;
                }
                T::T_net_tie_pad_groups => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        let group = self.lexer.cur_str().to_string();
                        self.fp_mut(fp).net_tie_pad_groups.push(group);
                        token = self.lexer.next_tok()?;
                    }
                }
                T::T_duplicate_pad_numbers_are_jumpers => {
                    let value = self.parse_bool()?;
                    self.fp_mut(fp).duplicate_pad_numbers_are_jumpers = value;
                    self.lexer.need_right()?;
                }
                T::T_jumper_pad_groups => {
                    let mut names: BTreeSet<String> = BTreeSet::new();
                    let mut in_group = false;

                    token = self.lexer.next_tok()?;

                    while in_group || token != T::T_RIGHT {
                        match token {
                            T::T_LEFT => {
                                if in_group {
                                    return Err(self.lexer.expecting_str("list of pad names"));
                                }

                                in_group = true;
                            }
                            T::T_STRING => {
                                if !in_group {
                                    return Err(self.lexer.expecting_str("list of pad names"));
                                }

                                names.insert(self.lexer.from_utf8());
                            }
                            T::T_RIGHT => {
                                if let Some(group) = JumperGroup::make(std::mem::take(&mut names)) {
                                    self.fp_mut(fp).jumper_pad_groups.add(group);
                                }

                                names.clear();
                                in_group = false;
                            }
                            _ => return Err(self.lexer.expecting_str("list of pad names")),
                        }

                        token = self.lexer.next_tok()?;
                    }
                }
                T::T_solder_mask_margin => {
                    let value = self.parse_board_units_str("local solder mask margin value")?;
                    self.fp_mut(fp).solder_mask_margin = Some(value);
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && self.fp_mut(fp).solder_mask_margin == Some(0)
                    {
                        self.fp_mut(fp).solder_mask_margin = None;
                    }
                }
                T::T_solder_paste_margin => {
                    let value = self.parse_board_units_str("local solder paste margin value")?;
                    self.fp_mut(fp).solder_paste_margin = Some(value);
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && self.fp_mut(fp).solder_paste_margin == Some(0)
                    {
                        self.fp_mut(fp).solder_paste_margin = None;
                    }
                }
                // legacy token solder_paste_ratio
                T::T_solder_paste_ratio | T::T_solder_paste_margin_ratio => {
                    let value =
                        self.parse_double_expected("local solder paste margin ratio value")?;
                    self.fp_mut(fp).solder_paste_margin_ratio = Some(value);
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201
                        && self.fp_mut(fp).solder_paste_margin_ratio == Some(0.0)
                    {
                        self.fp_mut(fp).solder_paste_margin_ratio = None;
                    }
                }
                T::T_clearance => {
                    let value = self.parse_board_units_str("local clearance value")?;
                    self.fp_mut(fp).clearance = Some(value);
                    self.lexer.need_right()?;

                    // In pre-9.0 files "0" meant inherit.
                    if self.required_version <= 20240201 && self.fp_mut(fp).clearance == Some(0) {
                        self.fp_mut(fp).clearance = None;
                    }
                }
                T::T_zone_connect => {
                    let value = self.parse_int_expected("zone connection value")?;
                    self.fp_mut(fp).zone_connection = zone_connection_from_int(value);
                    self.lexer.need_right()?;
                }
                T::T_thermal_width | T::T_thermal_gap => {
                    // Interestingly, these have never been exposed in the GUI
                    self.parse_board_units_tok(token, kicad_core::EdaDataType::Distance)?;
                    self.lexer.need_right()?;
                }
                T::T_attr => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        match token {
                            // legacy token prior to version 20200826
                            T::T_virtual => {
                                attributes |= FP_EXCLUDE_FROM_POS_FILES | FP_EXCLUDE_FROM_BOM;
                            }
                            T::T_through_hole => attributes |= FP_THROUGH_HOLE,
                            T::T_smd => attributes |= FP_SMD,
                            T::T_board_only => attributes |= FP_BOARD_ONLY,
                            T::T_exclude_from_pos_files => attributes |= FP_EXCLUDE_FROM_POS_FILES,
                            T::T_exclude_from_bom => attributes |= FP_EXCLUDE_FROM_BOM,
                            T::T_exclude_from_sim => attributes |= FP_EXCLUDE_FROM_SIM,
                            T::T_allow_missing_courtyard => {
                                self.fp_mut(fp).allow_missing_courtyard = true;
                            }
                            T::T_dnp => attributes |= FP_DNP,
                            T::T_allow_soldermask_bridges => {
                                self.fp_mut(fp).allow_solder_mask_bridges = true;
                            }
                            _ => {
                                return Err(self.lexer.expecting_str(
                                    "through_hole, smd, virtual, board_only, \
                                     exclude_from_pos_files, exclude_from_bom, \
                                     exclude_from_sim or allow_solder_mask_bridges",
                                ));
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }

                    self.fp_mut(fp).set_attributes(attributes);
                }
                T::T_fp_text => match self.parse_pcb_text_item(parent, None)? {
                    ParsedText::Field(field) => match field.id {
                        FieldT::Reference | FieldT::Value => {
                            // footprint->Reference() = PCB_FIELD( *text, FIELD_T::REFERENCE );
                            // footprint->Reference().SetUuidDirect( text->m_Uuid );
                            // (the parsed field is that value: the text's UUID, ordinal 0, no
                            // name)
                            let target = {
                                let ctx = BoardCtx::bare(&self.board);
                                self.board
                                    .footprint(fp)
                                    .and_then(|f| f.field(&ctx, field.id))
                            };

                            if let Some(target) = target {
                                self.board.modify_field(target, |f, _| *f = field);
                            }
                        }
                        _ => {
                            // Fields other than reference and value aren't treated specially,
                            // and can be created if the fp_text was hidden on the board,
                            // so just add those to the footprint as normal.
                            self.add_fp_child(fp, OwnedItem::Field(Box::new(field)));
                        }
                    },
                    ParsedText::Text(text) => {
                        self.add_fp_child(fp, OwnedItem::Text(Box::new(text)));
                    }
                },
                T::T_fp_text_box => {
                    if let Some(textbox) = self.parse_pcb_textbox(parent)? {
                        self.add_fp_child(fp, OwnedItem::Textbox(Box::new(textbox)));
                    }
                }
                T::T_table => {
                    if let Some(table) = self.parse_pcb_table(parent)? {
                        self.board.attach_table(table, AddMode::Append);
                    }
                }
                T::T_fp_arc
                | T::T_fp_circle
                | T::T_fp_curve
                | T::T_fp_rect
                | T::T_fp_line
                | T::T_fp_poly
                | T::T_fp_ellipse
                | T::T_fp_ellipse_arc => {
                    if let Some(shape) = self.parse_pcb_shape(parent)? {
                        self.add_fp_child(fp, OwnedItem::Shape(Box::new(shape)));
                    }
                }
                T::T_image => {
                    if let Some(image) = self.parse_pcb_reference_image(parent)? {
                        self.add_fp_child(fp, OwnedItem::Image(Box::new(image)));
                    }
                }
                T::T_barcode => {
                    if let Some(barcode) = self.parse_pcb_barcode(parent)? {
                        self.add_fp_child(fp, OwnedItem::Barcode(Box::new(barcode)));
                    }
                }
                T::T_dimension => {
                    if let Some(dimension) = self.parse_dimension(parent)? {
                        self.add_fp_child(fp, OwnedItem::Dimension(Box::new(dimension)));
                    }
                }
                T::T_pad => {
                    if let Some(pad) = self.parse_pad(Some(fp))? {
                        self.add_fp_child(fp, OwnedItem::Pad(Box::new(pad)));
                    }
                }
                T::T_model => {
                    token = self.lexer.next_tok()?;

                    if token == T::T_LEFT {
                        // Typed model (model (type extruded) ...)
                        token = self.lexer.next_tok()?;

                        if token != T::T_type {
                            return Err(self.lexer.expecting(T::T_type));
                        }

                        self.lexer.need_symbol()?;

                        if self.lexer.cur_tok() == T::T_extruded {
                            self.lexer.need_right()?; // close (type extruded)
                            self.parse_extruded_body(fp)?;
                        } else {
                            return Err(self.lexer.expecting_str("extruded"));
                        }
                    } else {
                        // Reference model (model "filename" ...)
                        if let Some(model) = self.parse_3d_model(true)? {
                            // FOOTPRINT::Add3DModel: models without a file name are dropped.
                            if !model.filename.is_empty() {
                                self.fp_mut(fp).add_3d_model(model);
                            }
                        }
                    }
                }
                T::T_zone => {
                    if let Some(zone) = self.parse_zone(parent)? {
                        self.add_footprint_zone(fp, zone);
                    }
                }
                T::T_group => self.parse_group(parent)?,
                T::T_constraint => self.parse_constraint(parent)?,
                T::T_point => {
                    if let Some(point) = self.parse_pcb_point()? {
                        let file_pos = point.lib_pos();

                        if let Some(id) = self.add_fp_child(fp, OwnedItem::Point(point))
                            && let Some(point_id) = id.typed()
                        {
                            // PCB_POINT::SetParent (inside FOOTPRINT::Add) recomputes the
                            // board position from the library position.
                            self.board
                                .modify_point(point_id, |p, env| p.on_footprint_transformed(env));

                            // Legacy files stored footprint points in board frame
                            if self.required_version < FIRST_FP_AFFINE_TRANSFORM {
                                self.board
                                    .modify_point(point_id, |p, env| p.set_position(env, file_pos));
                            }
                        }
                    }
                }
                T::T_embedded_fonts => {
                    let mut files = std::mem::take(&mut self.fp_mut(fp).embedded_files);
                    let result = self.parse_footprint_embedded_fonts(&mut files);
                    self.fp_mut(fp).embedded_files = files;
                    result?;
                }
                T::T_embedded_files => {
                    let mut files = std::mem::take(&mut self.fp_mut(fp).embedded_files);
                    let result = self.parse_footprint_embedded_files(&mut files);
                    self.fp_mut(fp).embedded_files = files;
                    result?;
                }
                T::T_component_classes => {
                    // std::unordered_set<wxString>: only membership matters (the manager sorts
                    // the names).
                    let mut component_class_names: BTreeSet<String> = BTreeSet::new();

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        if token != T::T_class {
                            return Err(self.lexer.expecting(T::T_class));
                        }

                        self.lexer.need_symbol_or_number()?;
                        component_class_names.insert(self.lexer.cur_text().to_string());
                        self.lexer.need_right()?;

                        token = self.lexer.next_tok()?;
                    }

                    // footprint->SetTransientComponentClassNames( componentClassNames ): only
                    // read by the paste tool; the Rust footprint keeps no transient names.

                    if self.board_present {
                        // footprint->ResolveComponentClassNames( m_board, componentClassNames )
                        let class = self
                            .board
                            .component_class_manager
                            .get_effective_static_component_class(&component_class_names);
                        self.fp_mut(fp).static_component_class = Some(class);
                    }
                }
                T::T_variant => self.parse_footprint_variant(fp)?,
                T::T_custom_property => {
                    // parseCustomProperty( footprint ): EDA_ITEM::SetCustomProperty on the
                    // footprint (a container without BoardItemMut; only the base is touched).
                    self.lexer.need_symbol()?;
                    let key = self.lexer.from_utf8();
                    self.lexer.need_symbol()?;
                    let value = self.lexer.from_utf8();
                    self.fp_mut(fp).base.set_custom_property(&key, &value);
                    self.lexer.need_right()?;
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "at, descr, locked, placed, tedit, tstamp, uuid, variant, \
                         autoplace_cost90, autoplace_cost180, attr, clearance, embedded_files, \
                         fp_arc, fp_circle, fp_curve, fp_line, fp_poly, fp_rect, fp_text, pad, \
                         group, generator, model, path, solder_mask_margin, \
                         solder_paste_margin, solder_paste_margin_ratio, tags, thermal_gap, \
                         version, zone, zone_connect, component_classes, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if self.board_present {
            self.fix_up_pads_for_board(fp);
        }

        // In legacy files the lack of attributes indicated a through-hole component which was
        // by default excluded from pos files.  However there was a hack to look for SMD pads
        // and consider those "mislabeled through-hole components" and therefore include them
        // in place files.  We probably don't want to get into that game so we'll just include
        // them by default and let the user change it if required.
        if self.required_version < 20200826 && attributes == 0 {
            attributes |= FP_THROUGH_HOLE;
        }

        if self.required_version <= LEGACY_NET_TIES
            && self
                .board
                .footprint(fp)
                .is_some_and(|f| f.keywords.starts_with("net tie"))
        {
            let mut pad_group = String::new();

            for (_, pad) in self.board.footprint_pads(fp) {
                if !pad_group.is_empty() {
                    pad_group += ", ";
                }

                pad_group += pad.number();
            }

            if !pad_group.is_empty() {
                self.fp_mut(fp).net_tie_pad_groups.push(pad_group);
            }
        }

        let footprint = self.fp_mut(fp);
        footprint.set_attributes(attributes);
        footprint.fpid = fpid;

        Ok(Some(fp))
    }

    /// The `T_property` case of `parseFOOTPRINT_unchecked`: legacy special properties, the
    /// dropped `ki_fp_filters` / `Footprint` fields, an existing field or a new user field,
    /// then the text effects.
    fn parse_footprint_property(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let parent = ParentRef::Item(fp.into());

        self.lexer.need_symbol()?;
        let p_name = self.lexer.from_utf8();
        self.lexer.need_symbol()?;
        let p_value = self.lexer.from_utf8();

        // Prior to PCB fields, we used to use properties for special values instead of
        // using (keyword_example "value")
        if self.required_version < 20230620 {
            // Skip legacy non-field properties sent from symbols that should not be kept
            // in footprints.
            if p_name == "ki_keywords" || p_name == "ki_locked" {
                return self.lexer.need_right();
            }

            // Description from symbol (not the fooprint library description stored in
            // (descr) ) used to be stored as a reserved key value
            if p_name == "ki_description" {
                let target = {
                    let ctx = BoardCtx::bare(&self.board);
                    self.board
                        .footprint(fp)
                        .and_then(|f| f.field(&ctx, FieldT::Description))
                };

                if let Some(target) = target {
                    self.board
                        .modify_field(target, |f, _| f.text.text.set_text(&p_value));
                }

                return self.lexer.need_right();
            }

            // Sheet file and name used to be stored as properties invisible to the user
            if p_name == "Sheetfile" || p_name == "Sheet file" {
                self.fp_mut(fp).sheetfile = p_value;
                return self.lexer.need_right();
            }

            if p_name == "Sheetname" || p_name == "Sheet name" {
                self.fp_mut(fp).sheetname = p_value;
                return self.lexer.need_right();
            }
        }

        let fp_layer = self.fp_mut(fp).base.layer;

        // Where the parsed field goes once its effects are read.
        enum Target {
            /// `unusedField`: parsed, then dropped.
            Unused,
            /// An existing field of the footprint.
            Existing(crate::ids::FieldId),
            /// A new user field, added to the footprint.
            New,
        }

        let existing = {
            let ctx = BoardCtx::bare(&self.board);
            self.board
                .footprint(fp)
                .and_then(|f| f.field_by_name(&ctx, &p_name))
        };

        let (mut field, target) = if p_name == "ki_fp_filters" {
            // 8.0.0rc3 had a bug where these properties were mistakenly added to the footprint
            // as fields, this will remove them as fields but still correctly set the footprint
            // filters
            self.fp_mut(fp).filters = p_value.clone();

            // Use the text effect parsing function because it will handle ki_fp_filters as a
            // property with no text effects, but will also handle parsing the text effects.
            // We just drop the effects if they're present.
            let mut field = PcbField::new(
                &mut self.board.kiid_generator,
                Some(fp_layer),
                FieldT::User,
                "",
            );
            let ordinal = {
                let ctx = BoardCtx::bare(&self.board);
                self.board
                    .footprint(fp)
                    .map_or(0, |f| f.next_field_ordinal(&ctx))
            };
            field.set_ordinal(ordinal, FieldT::User);
            (field, Target::Unused)
        } else if p_name == "Footprint" {
            // Until V9, footprints had a Footprint field that usually (but not always)
            // duplicated the footprint's LIB_ID.  In V9 this was removed.  Parse it
            // like any other, but don't add it to anything.
            let field = PcbField::new(
                &mut self.board.kiid_generator,
                Some(fp_layer),
                FieldT::Footprint,
                "",
            );
            (field, Target::Unused)
        } else if let Some(existing) = existing {
            let mut field = self
                .board
                .field(existing)
                .cloned()
                .expect("field of the parsed footprint");
            field.text.text.set_text(&p_value);
            (field, Target::Existing(existing))
        } else {
            let mut field = PcbField::new(
                &mut self.board.kiid_generator,
                Some(fp_layer),
                FieldT::User,
                &p_name,
            );
            let ordinal = {
                let ctx = BoardCtx::bare(&self.board);
                self.board
                    .footprint(fp)
                    .map_or(0, |f| f.next_field_ordinal(&ctx))
            };
            field.set_ordinal(ordinal, FieldT::User);

            field.text.text.set_text(&p_value);
            let layer = if fp_layer == PcbLayerId::FCu {
                PcbLayerId::FFab
            } else {
                PcbLayerId::BFab
            };
            let env = self.board.child_env(parent);
            BoardItemMut::set_layer(&mut field.text, &env, layer);

            (field, Target::New)
        };

        field.text.base.parent = parent;

        // Hide the field by default if it is a legacy field that did not have
        // text effects applied, since hide is a negative effect
        if self.required_version < 20230620 {
            field.text.text.set_visible(false);
        } else {
            field.text.text.set_visible(true);
        }

        let saw_effects = self.parse_pcb_text_effects(&mut field.text, false)?;

        let new_field = matches!(target, Target::New);

        // Fields too old to carry effects have no style of their own, so give them the
        // board's. A file that did carry effects has said all it means to say.
        if new_field && !saw_effects && self.board_present {
            // m_board is null when reading a lib
            field.style_from_settings(
                &self.board.child_env(parent),
                self.board.design_settings(),
                true,
            );
        }

        match target {
            Target::Unused => {}
            Target::Existing(id) => {
                self.board.modify_field(id, |f, _| *f = field);
            }
            Target::New => {
                // footprint->Add( field ) (the C++ adds it before setting its text and
                // effects; fields are always appended and nothing reads the list meanwhile)
                self.add_fp_child(fp, OwnedItem::Field(Box::new(field)));
            }
        }

        Ok(())
    }

    /// The `T_units` case of `parseFOOTPRINT_unchecked`: `(units (unit (name "A") (pins "1"
    /// "2" ...)) ...)`; unknown entries are skipped.
    fn parse_footprint_units(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let mut unit_infos: Vec<FpUnitInfo> = Vec::new();

        // (units (unit (name "A") (pins "1" "2" ...)) ...)
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            if token == T::T_unit {
                let mut info = FpUnitInfo::default();

                token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    if token == T::T_LEFT {
                        token = self.lexer.next_tok()?;
                    }

                    if token == T::T_name {
                        self.lexer.need_symbol_or_number()?;
                        info.unit_name = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    } else if token == T::T_pins {
                        // Parse a flat list of quoted numbers or symbols until ')'
                        token = self.lexer.next_tok()?;

                        while token != T::T_RIGHT {
                            if token == T::T_STRING || token == T::T_NUMBER {
                                info.pins.push(self.lexer.from_utf8());
                            } else {
                                return Err(self.lexer.expecting_str("pin number"));
                            }

                            token = self.lexer.next_tok()?;
                        }
                    } else {
                        // Unknown sub-token inside unit; skip its list if any
                        self.skip_current()?;
                    }

                    token = self.lexer.next_tok()?;
                }

                unit_infos.push(info);
            } else {
                // Unknown entry under units; skip
                self.skip_current()?;
            }

            token = self.lexer.next_tok()?;
        }

        if !unit_infos.is_empty() {
            self.fp_mut(fp).unit_info = unit_infos;
        }

        Ok(())
    }

    /// The typed-model branch of the `T_model` case: `(model (type extruded) ...)` fills the
    /// footprint's first 3D body (`GetBody()`, or `EnsureBody()` adds one).
    fn parse_extruded_body(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let existing: Option<Body3dId> = self.board.footprint_bodies(fp).next().map(|(id, _)| id);

        let body_id = match existing {
            Some(id) => id,
            None => {
                // FOOTPRINT::EnsureBody(): new PCB_3D_BODY( this ), Add( body, APPEND )
                let body = Pcb3dBody::new(&mut self.board.kiid_generator);

                match self
                    .add_fp_child(fp, OwnedItem::Body3d(Box::new(body)))
                    .and_then(|id| id.typed::<Pcb3dBody>())
                {
                    Some(id) => id,
                    None => return Ok(()),
                }
            }
        };

        let mut body = self
            .board
            .body3d(body_id)
            .cloned()
            .expect("body of the parsed footprint");

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_hide => {
                    let hide = self.parse_maybe_absent_bool(true)?;
                    body.show = !hide;
                }
                T::T_uuid => {
                    self.lexer.next_tok()?;
                    body.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_overall_height => {
                    body.height = self.parse_board_units_str("overall height")?;
                    self.lexer.need_right()?;
                }
                T::T_body_pcb_gap => {
                    body.standoff = self.parse_board_units_str("body pcb gap")?;
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    self.lexer.need_symbol()?;
                    let layer_name = self.lexer.cur_text().to_string();

                    if layer_name == "auto" {
                        body.layer = PcbLayerId::UndefinedLayer;
                    } else if layer_name == "pad_bbox" {
                        body.layer = PcbLayerId::UnselectedLayer;
                    } else {
                        let layer = Lset::name_to_layer(&layer_name);

                        if layer >= 0 {
                            body.layer = PcbLayerId(layer);
                        }
                    }

                    self.lexer.need_right()?;
                }
                T::T_material => {
                    self.lexer.need_symbol()?;
                    let mat_name = self.lexer.cur_text();

                    body.material = match mat_name {
                        "matte" => ExtrusionMaterial::Matte,
                        "metal" => ExtrusionMaterial::Metal,
                        "copper" => ExtrusionMaterial::Copper,
                        _ => ExtrusionMaterial::Plastic,
                    };

                    self.lexer.need_right()?;
                }
                T::T_color => {
                    self.lexer.need_symbol_or_number()?;

                    if self.lexer.cur_text() == "unspecified" {
                        body.color = Color4D::UNSPECIFIED;
                    } else {
                        body.color.r = self.parse_double()?;
                        body.color.g = self.parse_double_expected("green")?;
                        body.color.b = self.parse_double_expected("blue")?;
                        body.color.a = self.parse_double_expected("alpha")?;
                    }

                    self.lexer.need_right()?;
                }
                T::T_offset => {
                    self.need_xyz()?;
                    body.offset.x = self.parse_double_expected("x value")?;
                    body.offset.y = self.parse_double_expected("y value")?;
                    body.offset.z = self.parse_double_expected("z value")?;
                    self.lexer.need_right()?;
                    self.lexer.need_right()?;
                }
                T::T_scale => {
                    self.need_xyz()?;
                    body.scale.x = self.parse_double_expected("x value")?;
                    body.scale.y = self.parse_double_expected("y value")?;
                    body.scale.z = self.parse_double_expected("z value")?;
                    self.lexer.need_right()?;
                    self.lexer.need_right()?;
                }
                T::T_rotate => {
                    self.need_xyz()?;
                    body.rotation.x = self.parse_double_expected("x value")?;
                    body.rotation.y = self.parse_double_expected("y value")?;
                    body.rotation.z = self.parse_double_expected("z value")?;
                    self.lexer.need_right()?;
                    self.lexer.need_right()?;
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "hide, uuid, overall_height, body_pcb_gap, layer, material, color, \
                         offset, scale, or rotate",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        // The C++ edits the body in place; the edits (UUID included: SetUuidDirect on an
        // unindexed child) are stored back here.
        self.board.modify_body3d(body_id, |b, _| *b = body);

        Ok(())
    }

    /// `NeedLEFT(); token = NextTok(); if( token != T_xyz ) Expecting( T_xyz );`
    fn need_xyz(&mut self) -> Result<(), IoError> {
        self.lexer.need_left()?;
        let token = self.lexer.next_tok()?;

        if token != T::T_xyz {
            return Err(self.lexer.expecting(T::T_xyz));
        }

        Ok(())
    }

    /// The `T_zone` case of `parseFOOTPRINT_unchecked`: zones without corners are dropped;
    /// legacy footprint zone outlines were in the board frame and are converted to the library
    /// frame; then `FOOTPRINT::Add`.
    fn add_footprint_zone(&mut self, fp: FootprintId, mut zone: crate::zone::Zone) {
        if zone.num_corners() == 0 {
            return;
        }

        // Legacy footprint zone outlines were in board frame. Convert to lib.
        if self.required_version < FIRST_FP_AFFINE_TRANSFORM {
            let xform = self.fp_mut(fp).transform();
            let env = self.board.child_env(ParentRef::Item(fp.into()));
            let poly = zone.outline_mut(&env);

            let mut moves = Vec::new();
            let mut it = poly.iterate_with_holes();

            while it.is_valid() {
                moves.push((it.get_index(), xform.inverse_apply(it.get())));
                it.advance();
            }

            for (index, pos) in moves {
                poly.set_vertex_index(&index, pos);
            }

            // Hatch lines were cached in board frame; rebuild in lib frame so the
            // footprint transform is not applied to them twice on render.
            zone.hatch_border(&env);
        }

        self.add_fp_child(fp, OwnedItem::Zone(Box::new(zone)));
    }

    /// Port of `FOOTPRINT::FixUpPadsForBoard( BOARD* aBoard )` (`pcbnew/footprint.cpp`), called
    /// at the end of `parseFOOTPRINT_unchecked` with a board: in `EXPAND_INNER_LAYERS` mode
    /// every PTH pad gets all copper layers of the board. Not on the footprint model yet
    /// (change request); the caller tests `aBoard`.
    fn fix_up_pads_for_board(&mut self, fp: FootprintId) {
        if self.fp_mut(fp).stackup_mode != FootprintStackup::ExpandInnerLayers {
            return;
        }

        let board_copper = Lset::all_cu_mask_n(self.board.copper_layer_count());

        let pth_pads: Vec<_> = self
            .board
            .footprint_pads(fp)
            .filter(|(_, pad)| pad.attribute() == PadAttrib::Pth)
            .map(|(id, _)| id)
            .collect();

        for pad in pth_pads {
            self.board.modify_pad(pad, |p, env| {
                let mut pad_layers = p.padstack.layer_set().clone();
                pad_layers |= &board_copper;
                p.set_layer_set(env, pad_layers);
            });
        }
    }

    /// Port of `parse3DModel( bool aFileNameAlreadyParsed )`: `(model "file" (hide ..)
    /// (opacity ..) (at (xyz ..)) (offset (xyz ..)) (scale (xyz ..)) (rotate (xyz ..)))`.
    pub(crate) fn parse_3d_model(
        &mut self,
        file_name_already_parsed: bool,
    ) -> Result<Option<Fp3dmodel>, IoError> {
        if !file_name_already_parsed {
            if self.lexer.cur_tok() != T::T_model {
                debug_assert!(
                    false,
                    "Cannot parse {} as FP_3DMODEL.",
                    self.lexer.token_string(self.lexer.cur_tok())
                );
                return Ok(None);
            }

            self.lexer.need_symbol_or_number()?;
        }

        let mut n3d = Fp3dmodel {
            filename: self.lexer.from_utf8(),
            ..Fp3dmodel::default()
        };

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_at => {
                    self.need_xyz()?;

                    // Note:
                    // Prior to KiCad v5, model offset was designated by "at",
                    // and the units were in inches.
                    // Now we use mm, but support reading of legacy files
                    // (the C++ multiplies by the float literal 25.4f)
                    let inch = 25.4_f32 as f64;
                    n3d.offset.x = self.parse_double_expected("x value")? * inch;
                    n3d.offset.y = self.parse_double_expected("y value")? * inch;
                    n3d.offset.z = self.parse_double_expected("z value")? * inch;

                    self.lexer.need_right()?; // xyz
                    self.lexer.need_right()?; // at
                }
                T::T_hide => {
                    // In older files, the hide token appears bare, and indicates hide==true.
                    // In newer files, it will be an explicit bool in a list like (hide yes)
                    let hide = self.parse_maybe_absent_bool(true)?;
                    n3d.show = !hide;
                }
                T::T_opacity => {
                    n3d.opacity = self.parse_double_expected("opacity value")?;
                    self.lexer.need_right()?;
                }
                T::T_offset => {
                    self.need_xyz()?;

                    // 3D model offset is in mm
                    n3d.offset.x = self.parse_double_expected("x value")?;
                    n3d.offset.y = self.parse_double_expected("y value")?;
                    n3d.offset.z = self.parse_double_expected("z value")?;

                    self.lexer.need_right()?; // xyz
                    self.lexer.need_right()?; // offset
                }
                T::T_scale => {
                    self.need_xyz()?;

                    n3d.scale.x = self.parse_double_expected("x value")?;
                    n3d.scale.y = self.parse_double_expected("y value")?;
                    n3d.scale.z = self.parse_double_expected("z value")?;

                    self.lexer.need_right()?; // xyz
                    self.lexer.need_right()?; // scale
                }
                T::T_rotate => {
                    self.need_xyz()?;

                    n3d.rotation.x = self.parse_double_expected("x value")?;
                    n3d.rotation.y = self.parse_double_expected("y value")?;
                    n3d.rotation.z = self.parse_double_expected("z value")?;

                    self.lexer.need_right()?; // xyz
                    self.lexer.need_right()?; // rotate
                }
                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("at, hide, opacity, offset, scale, or rotate"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(n3d))
    }

    /// Port of `parseFootprintStackup( FOOTPRINT& )`: `(stackup (layer "name") ...)` puts the
    /// footprint in custom-layers mode; the copper layers must be an even, contiguous set and
    /// no technical layer may be listed.
    pub(crate) fn parse_footprint_stackup(&mut self, fp: FootprintId) -> Result<(), IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_stackup,
            "Expected stackup token"
        );

        if self.lexer.cur_tok() != T::T_stackup {
            return Ok(());
        }

        // If we have a stackup list at all, we must be in custom layer mode
        let stackup_mode = FootprintStackup::CustomLayers;
        let mut layers = Lset::new();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if self.lexer.cur_tok() != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_layer => {
                    self.lexer.need_symbol_or_number()?;

                    match self.layer_indices.get(self.lexer.cur_str()) {
                        None => return Err(self.lexer.expecting_str("layer name")),
                        Some(&layer) => {
                            layers.set(layer);
                        }
                    }

                    self.lexer.need_right()?;
                }
                _ => return Err(self.lexer.expecting_str("layer")),
            }

            token = self.lexer.next_tok()?;
        }

        // Check that the copper layers are sensible and contiguous
        let mut got_cu_layers = layers.clone();
        got_cu_layers &= Lset::all_cu_mask();

        // Remove this check when we support odd copper layer stackups
        if got_cu_layers.count() % 2 != 0 {
            return Err(IoError::new(format!(
                "Invalid stackup in footprint: odd number of copper layers ({}).",
                got_cu_layers.count()
            )));
        }

        let expected_cu_layers = Lset::all_cu_mask_n(got_cu_layers.count() as i32);

        if got_cu_layers != expected_cu_layers {
            return Err(IoError::new(
                "Invalid stackup in footprint: copper layers are not contiguous.",
            ));
        }

        let mut tech_layers = layers.clone();
        tech_layers &= Lset::all_tech_mask();

        if tech_layers.count() > 0 {
            return Err(IoError::new(
                "Invalid stackup in footprint: technology layers are implicit in footprints \
                 and should not be specified in the stackup.",
            ));
        }

        // Set the mode first, so that the layer count is unlocked if needed
        let footprint = self.fp_mut(fp);
        footprint.set_stackup_mode(stackup_mode);
        footprint.set_stackup_layers(layers);

        Ok(())
    }

    /// Port of `parseFootprintVariant( FOOTPRINT* )`: `(variant (name "VariantA") (dnp yes)
    /// (exclude_from_bom yes) (exclude_from_sim yes) (exclude_from_pos_files yes) (field (name
    /// "Value") (value "100nF")))`. A variant without a name is ignored.
    pub(crate) fn parse_footprint_variant(&mut self, fp: FootprintId) -> Result<(), IoError> {
        let mut variant_name = String::new();
        let mut dnp: Option<bool> = None;
        let mut exclude_from_bom: Option<bool> = None;
        let mut exclude_from_sim: Option<bool> = None;
        let mut exclude_from_pos_files: Option<bool> = None;
        let mut fields: Vec<(String, String)> = Vec::new();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_name => {
                    self.lexer.need_symbol()?;
                    variant_name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }
                T::T_dnp => dnp = Some(self.parse_maybe_absent_bool(true)?),
                T::T_exclude_from_bom => {
                    exclude_from_bom = Some(self.parse_maybe_absent_bool(true)?);
                }
                T::T_exclude_from_sim => {
                    exclude_from_sim = Some(self.parse_maybe_absent_bool(true)?);
                }
                T::T_exclude_from_pos_files => {
                    exclude_from_pos_files = Some(self.parse_maybe_absent_bool(true)?);
                }
                T::T_field => {
                    let mut field_name = String::new();
                    let mut field_value = String::new();

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token == T::T_LEFT {
                            token = self.lexer.next_tok()?;
                        }

                        if token == T::T_name {
                            self.lexer.need_symbol()?;
                            field_name = self.lexer.from_utf8();
                            self.lexer.need_right()?;
                        } else if token == T::T_value {
                            self.lexer.need_symbol()?;
                            field_value = self.lexer.from_utf8();
                            self.lexer.need_right()?;
                        } else {
                            return Err(self.lexer.expecting_str("name or value"));
                        }

                        token = self.lexer.next_tok()?;
                    }

                    if !field_name.is_empty() {
                        fields.push((field_name, field_value));
                    }
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "name, dnp, exclude_from_bom, exclude_from_sim, exclude_from_pos_files, \
                         or field",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        if variant_name.is_empty() {
            return Ok(());
        }

        let Some(variant) = self.fp_mut(fp).add_variant(&variant_name) else {
            return Ok(());
        };

        if let Some(dnp) = dnp {
            variant.dnp = dnp;
        }

        if let Some(exclude) = exclude_from_bom {
            variant.excluded_from_bom = exclude;
        }

        if let Some(exclude) = exclude_from_sim {
            variant.excluded_from_sim = exclude;
        }

        if let Some(exclude) = exclude_from_pos_files {
            variant.excluded_from_pos_files = exclude;
        }

        for (field_name, field_value) in &fields {
            variant.set_field_value(field_name, field_value);
        }

        Ok(())
    }
}
