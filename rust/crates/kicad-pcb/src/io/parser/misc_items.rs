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

//! Port of the remaining item parsers of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-12): `parsePCB_REFERENCE_IMAGE` (P:4459-4584), `parsePCB_BARCODE`
//! (P:4821-4965), `parsePCB_TABLECELL` (P:5003), `parseTableBodyToken`, `parseTableBody`,
//! `parseDrillSpanBody`, `parsePCB_DRILL_MAP`, `parseGeneratedTable`,
//! `parseGeneratedTableExtra`, `parsePCB_TABLE` (P:5188-5764), `parsePCB_POINT`,
//! `parsePCB_TARGET`, `parsePCB_GRID_ITEM` (P:10483-10734); plus `DocumentationLayers()` of
//! `pcbnew/board_tables/generated_table_schema.cpp`.
//!
//! Tables are created in place (`create_table` / `table_add_cell`) and returned unattached;
//! the caller attaches them (`attach_table`). The C++ `unique_ptr` deletes a table whose parse
//! fails; here it stays unattached in the arena of a board whose load failed anyway.

use kicad_core::color4d::Color4D;
use kicad_core::{Kiid, LineStyle, Lset, PCB_IU_SCALE, PcbLayerId, StrokeParams};
use kicad_geom::{DEGREES_T, EdaAngle, Vector2I};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::{PcbLexer, T};

use super::PcbParser;
use crate::ctx::{BoardCtx, MutEnv};
use crate::embedded_files::wx_base64_decode;
use crate::footprint::Footprint;
use crate::ids::TableId;
use crate::item::ParentRef;
use crate::pcb_barcode::{BarcodeEccT, BarcodeT, PcbBarcode};
use crate::pcb_drill_map::{DrillSpan, PcbDrillMap};
use crate::pcb_generated_table::{
    GeneratedTableColumn, GeneratedTableData, GeneratedTableKind, GeneratedTableSchema,
    drill_chart_schema, generated_table_align_from_token, generated_table_units_from_token,
    stackup_table_schema,
};
use crate::pcb_grid_item::{PcbGridAffects, PcbGridItem, PcbGridType};
use crate::pcb_point::PcbPoint;
use crate::pcb_reference_image::PcbReferenceImage;
use crate::pcb_shape::DEFAULT_LINE_WIDTH;
use crate::pcb_table::PcbTable;
use crate::pcb_tablecell::PcbTablecell;
use crate::pcb_target::PcbTarget;
use crate::traits::{BoardItem, BoardItemMut};

/// Port of `DocumentationLayers()` (`pcbnew/board_tables/generated_table_schema.cpp`): the
/// layers a generated table or drill map may live on. Manufacturing layers would be corrupted.
///
/// Excludes copper, silkscreen, mask, paste, adhesive, Edge.Cuts, Margin, courtyard and rescue
/// layers. Table artwork and hole symbols must not alter fabrication outputs or DRC
/// constraints. (Belongs to the table model, M11; kept here until it exists there.)
pub(crate) fn documentation_layers() -> Lset {
    // Drops mask, paste, adhesive and silk, which print on the finished board, plus the
    // courtyard layers, which DRC reads as constraints
    let mut layers = &Lset::all_non_cu_mask() & &!Lset::all_board_tech_mask();

    layers.reset(PcbLayerId::EdgeCuts);
    layers.reset(PcbLayerId::Margin);
    layers.reset(PcbLayerId::FCrtYd);
    layers.reset(PcbLayerId::BCrtYd);
    layers.reset(PcbLayerId::Rescue);

    layers
}

/// `aTable->Schema()`: `DrillChartSchema()` or `StackupTableSchema()`.
fn schema_of(kind: &GeneratedTableKind) -> GeneratedTableSchema {
    match kind {
        GeneratedTableKind::DrillChart { .. } => drill_chart_schema(),
        GeneratedTableKind::StackupTable => stackup_table_schema(),
    }
}

/// `std::isnormal( v )`.
fn is_normal(v: f64) -> bool {
    v.is_normal()
}

impl PcbParser {
    /// The `MutEnv` the setters of a not yet added item with parent `parent` see.
    fn misc_item_env(&self, parent: ParentRef) -> MutEnv {
        match parent {
            ParentRef::None => MutEnv::detached(),
            _ => self.board.child_env(parent),
        }
    }

    /// `if( token != T_LEFT ) Expecting( T_LEFT );`
    fn misc_need_left_tok(&self, token: T) -> Result<(), IoError> {
        if token != T::T_LEFT {
            return Err(self.lexer.expecting(T::T_LEFT));
        }

        Ok(())
    }

    /// Port of `parsePCB_REFERENCE_IMAGE( BOARD_ITEM* aParent )`.
    pub(crate) fn parse_pcb_reference_image(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbReferenceImage>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_image,
            "Cannot parse {} as a reference image.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_image {
            return Ok(None);
        }

        let mut bitmap = PcbReferenceImage::new(&mut self.board.kiid_generator, PcbLayerId::FCu);
        bitmap.base.parent = parent;
        let env = self.misc_item_env(parent);

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.misc_need_left_tok(token)?;

            token = self.lexer.next_tok()?;

            match token {
                T::T_at => {
                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    bitmap.set_position(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(&mut bitmap, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_rotate => {
                    let rotation = self.parse_double_expected("image rotation")?;
                    bitmap.set_orientation(EdaAngle::new(rotation, DEGREES_T));
                    self.lexer.need_right()?;
                }
                T::T_opacity => {
                    let opacity = self.parse_double_expected("image opacity")?;
                    bitmap.set_opacity(opacity);
                    self.lexer.need_right()?;
                }
                T::T_scale => {
                    let scale = self.parse_double_expected("image scale factor")?;
                    bitmap.set_image_scale(scale);

                    if !is_normal(bitmap.image_scale()) {
                        bitmap.set_image_scale(1.0);
                    }

                    self.lexer.need_right()?;
                }
                T::T_data => {
                    token = self.lexer.next_tok()?;

                    let mut data = String::new();

                    while token != T::T_RIGHT {
                        if !PcbLexer::is_symbol(token) {
                            return Err(self.lexer.expecting_str("base64 image data"));
                        }

                        data.push_str(&self.lexer.from_utf8());
                        token = self.lexer.next_tok()?;
                    }

                    // wxBase64Decode returns an empty buffer for invalid input
                    let buffer = wx_base64_decode(&data).unwrap_or_default();

                    if !bitmap.read_image_file(&buffer) {
                        return Err(IoError::new("Failed to read image data."));
                    }
                }
                T::T_locked => {
                    // This has only ever been (locked yes) format
                    let locked = self.parse_bool()?;
                    bitmap.base.is_locked = locked;

                    self.lexer.need_right()?;
                }
                T::T_uuid => {
                    self.lexer.next_tok()?;
                    bitmap.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut bitmap)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "at, layer, rotate, opacity, scale, data, locked, uuid, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        // Before 20260623 the PPI was computed from the embedded resolution but pixels/cm was
        // truncated to an integer, so the stored scale compensated for the wrong PPI.  Re-scale
        // to the corrected PPI to preserve the rendered size of existing boards.
        if self.required_version < 20260623 {
            let legacy_ppi = bitmap.legacy_ppi();

            if legacy_ppi > 0 && bitmap.ppi() != legacy_ppi {
                bitmap.set_image_scale(
                    bitmap.image_scale() * bitmap.ppi() as f64 / legacy_ppi as f64,
                );
            }
        }

        Ok(Some(bitmap))
    }

    /// Port of `parsePCB_BARCODE( BOARD_ITEM* aParent )`.
    ///
    /// The setters that end in `AssembleBarcode()` in C++ (`SetLayer`, `SetTextSize`,
    /// `SetShowText`, `SetIsKnockout`) only refill the symbol cache, which the port keys and
    /// rebuilds on demand; the final `AssembleBarcode()` is called like the C++ does.
    pub(crate) fn parse_pcb_barcode(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbBarcode>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_barcode,
            "Cannot parse {} as PCB_BARCODE.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_barcode {
            return Ok(None);
        }

        let mut barcode = PcbBarcode::new(&mut self.board.kiid_generator);
        barcode.base.parent = parent;
        let env = self.misc_item_env(parent);

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.misc_need_left_tok(token)?;

            token = self.lexer.next_tok()?;

            match token {
                T::T_at => {
                    let x = self.parse_board_units_str("X coordinate")?;
                    let y = self.parse_board_units_str("Y coordinate")?;
                    barcode.set_position(&env, Vector2I::new(x, y));
                    self.lexer.next_tok()?;

                    if self.lexer.cur_tok() == T::T_NUMBER {
                        let degrees = self.parse_double()?;
                        barcode.set_orientation(&env, degrees);
                    }

                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    barcode.set_layer(&env, layer);
                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let w = self.parse_board_units_str("barcode width")?;
                    let h = self.parse_board_units_str("barcode height")?;
                    barcode.width = w;
                    barcode.height = h;
                    self.lexer.need_right()?;
                }
                T::T_text => {
                    if self.lexer.next_tok()? != T::T_STRING {
                        return Err(self.lexer.expecting(T::T_STRING));
                    }

                    let text = self.lexer.from_utf8();
                    barcode.set_text(&text);
                    self.lexer.need_right()?;
                }
                T::T_text_height => {
                    let h = self.parse_board_units_str("barcode text height")?;
                    barcode.set_text_size(&env, h);

                    self.lexer.need_right()?;
                }
                T::T_type => {
                    self.lexer.need_symbol()?;

                    let kind = match self.lexer.cur_text() {
                        "code39" => BarcodeT::Code39,
                        "code128" => BarcodeT::Code128,
                        "datamatrix" | "data_matrix" => BarcodeT::DataMatrix,
                        "qr" | "qrcode" => BarcodeT::QrCode,
                        "microqr" | "micro_qr" => BarcodeT::MicroQrCode,
                        _ => return Err(self.lexer.expecting_str("barcode type")),
                    };

                    barcode.set_kind(kind);

                    self.lexer.need_right()?;
                }
                T::T_ecc_level => {
                    self.lexer.need_symbol()?;

                    let ecc = match self.lexer.cur_text() {
                        "L" | "l" => BarcodeEccT::L,
                        "M" | "m" => BarcodeEccT::M,
                        "Q" | "q" => BarcodeEccT::Q,
                        "H" | "h" => BarcodeEccT::H,
                        _ => return Err(self.lexer.expecting_str("ecc level")),
                    };

                    barcode.set_error_correction(ecc);

                    self.lexer.need_right()?;
                }
                T::T_locked => {
                    barcode.base.is_locked = self.parse_maybe_absent_bool(true)?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    barcode.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_hide => {
                    let hide = self.parse_bool()?;
                    barcode.set_show_text(!hide);
                    self.lexer.need_right()?;
                }
                T::T_knockout => {
                    let knockout = self.parse_bool()?;
                    barcode.set_is_knockout(knockout);
                    self.lexer.need_right()?;
                }
                T::T_margins => {
                    let margin_x = self.parse_board_units_str("margin X")?;
                    let margin_y = self.parse_board_units_str("margin Y")?;
                    barcode.margin = Vector2I::new(margin_x, margin_y);
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut barcode)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "at, layer, size, text, text_height, type, ecc_level, locked, hide, \
                         knockout, margins, uuid, or custom_property",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        {
            let ctx = BoardCtx::bare(&self.board);
            barcode.assemble_barcode(&ctx);
        }

        Ok(Some(barcode))
    }

    /// Port of `parsePCB_TABLECELL( BOARD_ITEM* aParent )`: the cell of `table`, not yet
    /// added (the caller does `table->AddCell( cell )`).
    fn parse_pcb_tablecell(&mut self, table: TableId) -> Result<Option<PcbTablecell>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_table_cell,
            "Cannot parse {} as a table cell.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_table_cell {
            return Ok(None);
        }

        // PCB_TABLECELL( aParentTable ): mirrored like the parent footprint or, without one, by
        // the side of the table's layer
        let (table_parent, table_layer) = {
            let t = &self.board[table];
            (t.base.parent, t.base.layer)
        };

        let parent_fp = table_parent
            .item()
            .and_then(|p| p.typed::<Footprint>())
            .filter(|fp| self.board.footprint(*fp).is_some());

        let mirrored = if let Some(fp) = parent_fp {
            self.board[fp].is_flipped()
        } else if self.parent_reaches_board(table_parent) {
            self.board.is_back_layer(table_layer)
        } else {
            kicad_core::layer_ids::is_back_layer(table_layer)
        };

        let mut cell = PcbTablecell::new(&mut self.board.kiid_generator, mirrored);
        BoardItem::base_mut(&mut cell.textbox).parent = ParentRef::Item(table.into());

        let mut span = (cell.col_span, cell.row_span);
        self.parse_text_box_content_with(&mut cell.textbox, Some(&mut span))?;
        cell.col_span = span.0;
        cell.row_span = span.1;

        self.bake_text_box_lib(&mut cell.textbox)?;

        Ok(Some(cell))
    }

    /// `parseCustomProperty( table )` for a table already in the board arena.
    fn parse_table_custom_property(&mut self, table: TableId) -> Result<(), IoError> {
        self.lexer.need_symbol()?;
        let key = self.lexer.from_utf8();
        self.lexer.need_symbol()?;
        let value = self.lexer.from_utf8();
        self.board
            .modify_table(table, |t, _| t.base.set_custom_property(&key, &value));
        self.lexer.need_right()
    }

    /// `table->SetUuidDirect( CurStrToKIID() )` after `NextTok()`.
    fn parse_table_uuid(&mut self, table: TableId) -> Result<(), IoError> {
        self.lexer.next_tok()?;
        let uuid: Kiid = self.cur_str_to_kiid();
        self.board.modify_table(table, |t, _| t.base.uuid = uuid);
        self.lexer.need_right()
    }

    /// `STROKE_PARAMS stroke( -1, LINE_STYLE::SOLID ); strokeParser.ParseStroke( stroke );`
    fn parse_table_stroke(&mut self) -> Result<StrokeParams, IoError> {
        let mut stroke = StrokeParams::new(-1, LineStyle::Solid, Color4D::UNSPECIFIED);
        self.parse_stroke(&mut stroke)?;
        Ok(stroke)
    }

    /// Port of `parseTableBodyToken( PCB_TABLE* aTable, T aToken, bool aAllowIdentity )`.
    ///
    /// A drill chart offers its own tokens first and shares the rest, so identity is accepted
    /// only for the plain table form.
    fn parse_table_body_token(
        &mut self,
        table: TableId,
        token: T,
        allow_identity: bool,
    ) -> Result<bool, IoError> {
        match token {
            T::T_column_count => {
                let count = self.parse_int_expected("column count")?;
                self.board
                    .modify_table(table, |t, _| t.set_col_count(count));
                self.lexer.need_right()?;
                Ok(true)
            }
            T::T_uuid => {
                if !allow_identity {
                    return Err(self.lexer.expecting_str("table geometry without identity"));
                }

                self.parse_table_uuid(table)?;
                Ok(true)
            }
            T::T_locked => {
                if !allow_identity {
                    return Err(self.lexer.expecting_str("table geometry without identity"));
                }

                let locked = self.parse_bool()?;
                self.board
                    .modify_table(table, |t, _| t.base.is_locked = locked);
                self.lexer.need_right()?;
                Ok(true)
            }
            T::T_angle => {
                // legacy token no longer used
                self.lexer.need_right()?;
                Ok(true)
            }
            T::T_layer => {
                if !allow_identity {
                    return Err(self.lexer.expecting_str("table geometry without identity"));
                }

                let layer = self.parse_board_item_layer()?;
                self.board.set_item_layer(table.into(), layer);
                self.lexer.need_right()?;
                Ok(true)
            }
            T::T_column_widths => {
                let mut col = 0;

                while self.lexer.next_tok()? != T::T_RIGHT {
                    let width = self.parse_board_units()?;
                    self.board
                        .modify_table(table, |t, _| t.set_col_width(col, width));
                    col += 1;
                }

                Ok(true)
            }
            T::T_row_heights => {
                let mut row = 0;

                while self.lexer.next_tok()? != T::T_RIGHT {
                    let height = self.parse_board_units()?;
                    self.board
                        .modify_table(table, |t, _| t.set_row_height(row, height));
                    row += 1;
                }

                Ok(true)
            }
            T::T_cells => {
                let mut token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    self.misc_need_left_tok(token)?;

                    token = self.lexer.next_tok()?;

                    if token != T::T_table_cell {
                        return Err(self.lexer.expecting_str("table_cell"));
                    }

                    if let Some(cell) = self.parse_pcb_tablecell(table)? {
                        self.board.table_add_cell(table, cell);
                    }

                    token = self.lexer.next_tok()?;
                }

                Ok(true)
            }
            T::T_border => {
                let mut token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    self.misc_need_left_tok(token)?;

                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_external => {
                            let on = self.parse_bool()?;
                            self.board
                                .modify_table(table, |t, _| t.stroke_external = on);
                            self.lexer.need_right()?;
                        }
                        T::T_header => {
                            let on = self.parse_bool()?;
                            self.board
                                .modify_table(table, |t, _| t.stroke_header_separator = on);
                            self.lexer.need_right()?;
                        }
                        T::T_stroke => {
                            let stroke = self.parse_table_stroke()?;
                            self.board
                                .modify_table(table, |t, _| t.border_stroke = stroke);
                        }
                        _ => return Err(self.lexer.expecting_str("external, header or stroke")),
                    }

                    token = self.lexer.next_tok()?;
                }

                Ok(true)
            }
            T::T_separators => {
                let mut token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    self.misc_need_left_tok(token)?;

                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_rows => {
                            let on = self.parse_bool()?;
                            self.board.modify_table(table, |t, _| t.stroke_rows = on);
                            self.lexer.need_right()?;
                        }
                        T::T_cols => {
                            let on = self.parse_bool()?;
                            self.board.modify_table(table, |t, _| t.stroke_columns = on);
                            self.lexer.need_right()?;
                        }
                        T::T_stroke => {
                            let stroke = self.parse_table_stroke()?;
                            self.board
                                .modify_table(table, |t, _| t.separators_stroke = stroke);
                        }
                        _ => return Err(self.lexer.expecting_str("rows, cols, or stroke")),
                    }

                    token = self.lexer.next_tok()?;
                }

                Ok(true)
            }
            T::T_custom_property => {
                self.parse_table_custom_property(table)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Port of `parseTableBody( PCB_TABLE* aTable, bool aAllowIdentity )`.
    fn parse_table_body(&mut self, table: TableId, allow_identity: bool) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.misc_need_left_tok(token)?;

            token = self.lexer.next_tok()?;

            if !self.parse_table_body_token(table, token, allow_identity)? {
                return Err(self.lexer.expecting_str(
                    "columns, layer, col_widths, row_heights, border, separators, header or cells",
                ));
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parsePCB_TABLE( BOARD_ITEM* aParent )`: a created, unattached table.
    pub(crate) fn parse_pcb_table(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<TableId>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_table,
            "Cannot parse {} as a table.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_table {
            return Ok(None);
        }

        // new PCB_TABLE( aParent, -1 )
        let mut table = PcbTable::with_line_width(-1);
        table.base.uuid = Kiid::new(&mut self.board.kiid_generator);
        let id = self.board.create_table(table, parent);

        self.parse_table_body(id, true)?;

        Ok(Some(id))
    }

    /// `GENERATED_TABLE_DATA` of a created table (`aTable` as a `PCB_GENERATED_TABLE`).
    fn generated_mut<R>(
        &mut self,
        table: TableId,
        f: impl FnOnce(&mut GeneratedTableData) -> R,
    ) -> Option<R> {
        self.board
            .modify_table(table, |t, _| t.generated.as_deref_mut().map(f))
            .flatten()
    }

    /// Port of `parseGeneratedTable( std::unique_ptr<PCB_GENERATED_TABLE> aTable )`:
    /// `drill_chart` / `stackup_table`, created (`new PCB_DRILL_CHART( m_board )` /
    /// `new PCB_STACKUP_TABLE( m_board )`), unattached.
    pub(crate) fn parse_generated_table(
        &mut self,
        kind: GeneratedTableKind,
        parent: ParentRef,
    ) -> Result<Option<TableId>, IoError> {
        // The constructor of the concrete class: PCB_DRILL_CHART uses 0.1 mm strokes and applies
        // the default template, PCB_STACKUP_TABLE DEFAULT_LINE_WIDTH and the schema defaults.
        let (line_width, data) = match kind {
            GeneratedTableKind::DrillChart { .. } => (
                PCB_IU_SCALE.mm_to_iu(0.1),
                GeneratedTableData::new_drill_chart(),
            ),
            GeneratedTableKind::StackupTable => (
                PCB_IU_SCALE.mm_to_iu(DEFAULT_LINE_WIDTH),
                GeneratedTableData::new_stackup_table(),
            ),
        };

        let mut table = PcbTable::with_generated(line_width, data);
        table.base.uuid = Kiid::new(&mut self.board.kiid_generator);
        let schema = schema_of(&kind);
        let id = self.board.create_table(table, parent);

        // The file's columns replace the defaults a new table starts with rather than adding to
        // them
        let mut columns: Vec<GeneratedTableColumn> = Vec::new();

        // The writer omits units and precision equal to these, whatever the constructor chose
        let default_units = schema.default_units;
        let default_precision = schema.default_precision;
        self.generated_mut(id, |g| {
            g.units = default_units;
            g.set_precision(default_precision);
        });

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.misc_need_left_tok(token)?;

            token = self.lexer.next_tok()?;

            match token {
                T::T_uuid => self.parse_table_uuid(id)?,
                T::T_locked => {
                    let locked = self.parse_bool()?;
                    self.board
                        .modify_table(id, |t, _| t.base.is_locked = locked);
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    self.board.set_item_layer(id.into(), layer);
                    self.lexer.need_right()?;
                }
                T::T_units => {
                    self.lexer.need_symbol_or_number()?;

                    if let Some(units) = generated_table_units_from_token(&self.lexer.from_utf8()) {
                        self.generated_mut(id, |g| g.units = units);
                    }

                    self.lexer.need_right()?;
                }
                T::T_precision => {
                    let precision = self.parse_int_expected("generated table precision")?;
                    self.generated_mut(id, |g| g.set_precision(precision));
                    self.lexer.need_right()?;
                }
                T::T_column => {
                    // Not GENERATED_TABLE_COLUMN's default 0 (a schema's first id), or an
                    // unrecognized id token that never sets it would silently become a real
                    // column instead of being dropped by Validate() below as the unknown id it
                    // actually is
                    let mut col = GeneratedTableColumn {
                        id: -1,
                        ..GeneratedTableColumn::default()
                    };

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        self.misc_need_left_tok(token)?;

                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_id => {
                                self.lexer.need_symbol_or_number()?;

                                if let Some(def) = schema.find_token(&self.lexer.from_utf8()) {
                                    // The writer's own default is this literal, never
                                    // DefaultColumn()'s translated text, so a heading it left
                                    // out reads back the same regardless of which locale is
                                    // active now
                                    col.id = def.id;
                                    col.heading = def.heading.to_string();
                                    col.align = def.align;
                                }

                                self.lexer.need_right()?;
                            }
                            T::T_name => {
                                self.lexer.need_symbol_or_number()?;
                                col.heading = self.lexer.from_utf8();
                                self.lexer.need_right()?;
                            }
                            T::T_justify => {
                                self.lexer.need_symbol_or_number()?;
                                if let Some(align) =
                                    generated_table_align_from_token(&self.lexer.from_utf8())
                                {
                                    col.align = align;
                                }
                                self.lexer.need_right()?;
                            }
                            T::T_width => {
                                col.width =
                                    self.parse_board_units_str("generated table column width")?;
                                self.lexer.need_right()?;
                            }
                            T::T_hide => {
                                col.visible = !self.parse_bool()?;
                                self.lexer.need_right()?;
                            }
                            _ => self.skip_current()?,
                        }

                        token = self.lexer.next_tok()?;
                    }

                    columns.push(col);
                }
                T::T_row_keys => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        self.misc_need_left_tok(token)?;

                        token = self.lexer.next_tok()?;

                        if token == T::T_key {
                            let row = self.parse_int_expected("row key row")?;

                            self.lexer.need_symbol_or_number()?;
                            let key = self.lexer.cur_text().to_string();
                            self.generated_mut(id, |g| {
                                g.row_keys.insert(row, key);
                            });
                        }

                        self.lexer.need_right()?;

                        token = self.lexer.next_tok()?;
                    }
                }
                _ => {
                    if !self.parse_generated_table_extra(id, token)? {
                        // A generated table is a table, so whatever is left is the geometry and
                        // cells it shares with one rather than something to skip
                        if !self.parse_table_body_token(id, token, false)? {
                            self.skip_current()?;
                        }
                    }
                }
            }

            token = self.lexer.next_tok()?;
        }

        if columns.is_empty() {
            columns = schema.defaults();
        }

        let any_shown = |columns: &[GeneratedTableColumn]| columns.iter().any(|c| c.visible);

        let friendly_name = {
            let ctx = BoardCtx::bare(&self.board);
            BoardItem::friendly_name(&self.board[id], &ctx)
        };

        // No columns leaves every later consumer dividing by the column count. Repeats and
        // implausible widths reach table geometry
        if self.board[id].col_count() < 1 || !schema.validate(&mut columns) || !any_shown(&columns)
        {
            return Err(IoError::new(format!(
                "Invalid {friendly_name}: bad column set"
            )));
        }

        self.generated_mut(id, |g| g.restore_columns(&columns));

        // Copper, silkscreen, mask, paste, adhesive, Edge.Cuts, Margin and courtyard are all
        // manufacturing inputs that table artwork would corrupt rather than document
        if !documentation_layers().contains(self.board[id].base.layer) {
            return Err(IoError::new(format!(
                "Invalid {friendly_name}: not on a documentation layer"
            )));
        }

        Ok(Some(id))
    }

    /// Port of `parseGeneratedTableExtra( PCB_GENERATED_TABLE* aTable, T aToken )`: the
    /// tokens only one kind of generated table carries (the drill chart's filter, totals and
    /// row shapes). Returns whether `token` was one of them.
    fn parse_generated_table_extra(&mut self, table: TableId, token: T) -> Result<bool, IoError> {
        let is_drill_chart = matches!(
            self.board[table].generated.as_deref().map(|g| &g.kind),
            Some(GeneratedTableKind::DrillChart { .. })
        );

        if !is_drill_chart {
            return Ok(false);
        }

        /// `static_cast<PCB_DRILL_CHART*>( aTable )` members.
        fn with_chart(
            parser: &mut PcbParser,
            table: TableId,
            f: impl FnOnce(
                &mut crate::pcb_generated_table::DrillChartFilter,
                &mut bool,
                &mut std::collections::BTreeMap<i32, i32>,
                &mut i32,
            ),
        ) {
            parser.generated_mut(table, |g| {
                if let GeneratedTableKind::DrillChart {
                    filter,
                    show_totals,
                    row_shapes,
                    symbol_column,
                } = &mut g.kind
                {
                    f(filter, show_totals, row_shapes, symbol_column);
                }
            });
        }

        match token {
            T::T_filter => {
                let mut token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    self.misc_need_left_tok(token)?;

                    token = self.lexer.next_tok()?;

                    match token {
                        T::T_plated
                        | T::T_npth
                        | T::T_vias
                        | T::T_slots
                        | T::T_backdrill
                        | T::T_castellated => {
                            let on = self.parse_bool()?;
                            with_chart(self, table, |filter, _, _, _| match token {
                                T::T_plated => filter.plated = on,
                                T::T_npth => filter.non_plated = on,
                                T::T_vias => filter.vias = on,
                                T::T_slots => filter.slots = on,
                                T::T_backdrill => filter.backdrills = on,
                                _ => filter.castellated = on,
                            });
                            self.lexer.need_right()?;
                        }
                        _ => self.skip_current()?,
                    }

                    token = self.lexer.next_tok()?;
                }

                Ok(true)
            }
            T::T_totals => {
                // chart->RestoreShowTotals( parseBool() )
                let on = self.parse_bool()?;
                with_chart(self, table, |_, show_totals, _, _| *show_totals = on);
                self.lexer.need_right()?;
                Ok(true)
            }
            T::T_row_shapes => {
                let mut token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    self.misc_need_left_tok(token)?;

                    token = self.lexer.next_tok()?;

                    if token == T::T_column {
                        let column = self.parse_int_expected("symbol column")?;
                        with_chart(self, table, |_, _, _, symbol_column| {
                            *symbol_column = column
                        });
                    } else if token == T::T_shape {
                        let row = self.parse_int_expected("row shape row")?;
                        let shape = self.parse_int_expected("row shape index")?;
                        with_chart(self, table, |_, _, row_shapes, _| {
                            row_shapes.insert(row, shape);
                        });
                    }

                    self.lexer.need_right()?;

                    token = self.lexer.next_tok()?;
                }

                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Port of `parseDrillSpanBody`: call with the start layer name as the current token;
    /// reads the end layer name and the flags up to and including the closing `)`.
    pub(crate) fn parse_drill_span_body(&mut self) -> Result<Option<DrillSpan>, IoError> {
        let name = self.lexer.from_utf8();
        let start_layer = PcbLayerId::from(Lset::name_to_layer(&name));

        self.lexer.need_symbol_or_number()?;
        let name = self.lexer.from_utf8();
        let end_layer = PcbLayerId::from(Lset::name_to_layer(&name));

        let mut backdrill = false;
        let mut non_plated = false;

        // The flags are what tell a backdrill span apart from the primary span sharing its layer
        // pair, so a map without them comes back pointing at the wrong holes
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_backdrill {
                backdrill = true;
            } else if token == T::T_npth {
                non_plated = true;
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(DrillSpan::new(
            start_layer,
            end_layer,
            backdrill,
            non_plated,
        )))
    }

    /// Port of `parsePCB_DRILL_MAP( BOARD_ITEM* aParent )`.
    pub(crate) fn parse_pcb_drill_map(
        &mut self,
        parent: ParentRef,
    ) -> Result<Option<PcbDrillMap>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_drill_map,
            "Cannot parse {} as a drill map.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_drill_map {
            return Ok(None);
        }

        // PCB_DRILL_MAP( aParent ): the symbol size of the board's drill symbol profile when
        // there is a board. The profile is kept verbatim (tier-2 drill symbol package), so the
        // default profile's size is used (PORT_STATUS.md, PCBIO-12 known gaps).
        let mut map = PcbDrillMap::new(&mut self.board.kiid_generator);
        map.base.parent = parent;
        let env = self.misc_item_env(parent);

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.misc_need_left_tok(token)?;

            token = self.lexer.next_tok()?;

            match token {
                T::T_uuid => {
                    self.lexer.next_tok()?;
                    map.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_locked => {
                    map.base.is_locked = self.parse_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(&mut map, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_offset => {
                    let x = self.parse_board_units_str("drill map x offset")?;
                    let y = self.parse_board_units_str("drill map y offset")?;
                    map.set_offset(Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let size = self.parse_board_units_str("drill map symbol size")?;
                    map.set_symbol_size(size);
                    self.lexer.need_right()?;
                }
                T::T_span => {
                    self.lexer.next_tok()?;

                    if self.lexer.cur_tok() == T::T_all {
                        map.set_all_spans(true);
                        self.lexer.need_right()?;
                    } else {
                        map.set_all_spans(false);

                        if let Some(span) = self.parse_drill_span_body()? {
                            map.set_span(span);
                        }
                    }
                }
                T::T_outline_slots => {
                    let on = self.parse_bool()?;
                    map.set_outline_slots(on);
                    self.lexer.need_right()?;
                }
                T::T_guide_cross => {
                    let on = self.parse_bool()?;
                    map.set_guide_cross(on);
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut map)?,
                _ => self.skip_current()?,
            }

            token = self.lexer.next_tok()?;
        }

        if !documentation_layers().contains(map.base.layer) {
            return Err(IoError::new(
                "Invalid drill map: not on a documentation layer",
            ));
        }

        Ok(Some(map))
    }

    /// Port of `parsePCB_POINT`.
    pub(crate) fn parse_pcb_point(&mut self) -> Result<Option<PcbPoint>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_point,
            "Cannot parse {} as PCB_POINT.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_point {
            return Ok(None);
        }

        // new PCB_POINT( nullptr )
        let mut point = PcbPoint::new(&mut self.board.kiid_generator);
        let env = MutEnv::detached();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_at => {
                    let x = self.parse_board_units_str("point x position")?;
                    let y = self.parse_board_units_str("point y position")?;
                    point.set_position(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let size = self.parse_board_units_str("point size")?;
                    point.set_size(size);
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(&mut point, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_uuid => {
                    self.lexer.next_tok()?;
                    point.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut point)?,
                _ => return Err(self.lexer.expecting_str("at, size, layer or uuid")),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(point))
    }

    /// Port of `parsePCB_TARGET`.
    pub(crate) fn parse_pcb_target(&mut self) -> Result<Option<PcbTarget>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_target,
            "Cannot parse {} as PCB_TARGET.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_target {
            return Ok(None);
        }

        // new PCB_TARGET( nullptr )
        let mut target = PcbTarget::new(&mut self.board.kiid_generator);
        let env = MutEnv::detached();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_x => target.set_shape(1),
                T::T_plus => target.set_shape(0),
                T::T_at => {
                    let x = self.parse_board_units_str("target x position")?;
                    let y = self.parse_board_units_str("target y position")?;
                    target.set_position(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_size => {
                    let size = self.parse_board_units_str("target size")?;
                    target.set_size(size);
                    self.lexer.need_right()?;
                }
                T::T_width => {
                    let width = self.parse_board_units_str("target thickness")?;
                    target.set_width(width);
                    self.lexer.need_right()?;
                }
                T::T_layer => {
                    let layer = self.parse_board_item_layer()?;
                    BoardItemMut::set_layer(&mut target, &env, layer);
                    self.lexer.need_right()?;
                }
                T::T_tstamp | T::T_uuid => {
                    self.lexer.next_tok()?;
                    target.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut target)?,
                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("x, plus, at, size, width, layer, uuid, or tstamp"));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(target))
    }

    /// Port of `parsePCB_GRID_ITEM`.
    pub(crate) fn parse_pcb_grid_item(&mut self) -> Result<Option<PcbGridItem>, IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_grid_item,
            "Cannot parse {} as PCB_GRID_ITEM.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        if self.lexer.cur_tok() != T::T_grid_item {
            return Ok(None);
        }

        // new PCB_GRID_ITEM( nullptr )
        let mut griditem = PcbGridItem::new(&mut self.board.kiid_generator);
        let env = MutEnv::detached();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_xy => griditem.set_grid_item_type(PcbGridType::Cartesian),
                T::T_polar => griditem.set_grid_item_type(PcbGridType::Polar),
                T::T_at => {
                    let x = self.parse_board_units_str("grid_item x position")?;
                    let y = self.parse_board_units_str("grid_item y position")?;
                    griditem.set_position(&env, Vector2I::new(x, y));
                    self.lexer.need_right()?;
                }
                T::T_spacing => {
                    // Writer emits the grid type before extent/spacing, so the type is set
                    // here. Polar y is an angle; cartesian y is a length.
                    if griditem.grid_item_type() == PcbGridType::Polar {
                        let radius = self.parse_board_units_str("grid_item radius spacing")?;
                        // SetRadiusSpacing( aR ): m_spacing.x = std::max( aR, 1 )
                        griditem.spacing.x = std::cmp::max(radius, 1);
                        let phi = self.parse_double_expected("grid_item phi spacing")?;
                        griditem.set_phi_spacing_degrees(phi);
                    } else {
                        let x = self.parse_board_units_str("grid_item x spacing")?;
                        let y = self.parse_board_units_str("grid_item y spacing")?;
                        griditem.set_spacing(Vector2I::new(x, y));
                    }

                    self.lexer.need_right()?;
                }
                T::T_extent => {
                    if griditem.grid_item_type() == PcbGridType::Polar {
                        let radius = self.parse_board_units_str("grid_item radius extent")?;
                        // SetRadiusExtent( aR ): m_extent.x = std::abs( aR ) (INT_MIN stays
                        // INT_MIN on the reference platform)
                        griditem.extent.x = radius.wrapping_abs();
                        let phi = self.parse_double_expected("grid_item phi extent")?;
                        griditem.set_phi_extent_degrees(phi);
                    } else {
                        let x = self.parse_board_units_str("grid_item x extent")?;
                        let y = self.parse_board_units_str("grid_item y extent")?;
                        griditem.set_extent(Vector2I::new(x, y));
                    }

                    self.lexer.need_right()?;
                }
                T::T_angle => {
                    let degrees = self.parse_double_expected("grid_item orientation")?;
                    // SetOrientationDegrees( aDeg ): m_orientation = EDA_ANGLE( aDeg, DEGREES_T )
                    griditem.set_orientation(EdaAngle::new(degrees, DEGREES_T));
                    self.lexer.need_right()?;
                }
                T::T_priority => {
                    let priority = self.parse_int_expected("grid_item priority")?;
                    griditem.set_assigned_priority(priority as u32);
                    self.lexer.need_right()?;
                }
                T::T_tick_interval => {
                    let interval = self.parse_int_expected("grid_item tick_interval")?;
                    griditem.set_tick_interval(interval as u32);
                    self.lexer.need_right()?;
                }
                T::T_affects => {
                    // (affects (cursor yes|no) (routing yes|no) (placement yes|no))
                    let mut aff = PcbGridAffects::default();
                    aff.set_all(false);

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting_str("cursor, routing or placement"));
                        }

                        token = self.lexer.next_tok()?;

                        let target = match token {
                            T::T_cursor => &mut aff.cursor,
                            T::T_routing => &mut aff.routing,
                            T::T_placement => &mut aff.placement,
                            _ => {
                                return Err(self
                                    .lexer
                                    .expecting_str("cursor, routing or placement"));
                            }
                        };

                        *target = self.parse_bool()?;
                        self.lexer.need_right()?;

                        token = self.lexer.next_tok()?;
                    }

                    griditem.affects = aff;
                }
                T::T_locked => {
                    griditem.base.is_locked = self.parse_bool()?;
                    self.lexer.need_right()?;
                }
                T::T_uuid => {
                    self.lexer.next_tok()?;
                    griditem.base.uuid = self.cur_str_to_kiid();
                    self.lexer.need_right()?;
                }
                T::T_custom_property => self.parse_custom_property_item(&mut griditem)?,
                _ => {
                    return Err(self.lexer.expecting_str(
                        "xy, polar, at, spacing, extent, angle, priority, tick_interval, \
                         affects, locked or uuid",
                    ));
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(Some(griditem))
    }
}
