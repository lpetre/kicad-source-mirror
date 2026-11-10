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

//! Port of the board skeleton of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`
//! (package PCBIO-01): `IsValidBoardHeader`, `Parse`, `finishItem`, `parseBOARD`,
//! `parseBOARD_unchecked` with its post-processing, `parseHeader`, `parseGeneralSection`,
//! `parsePAGE_INFO`, `parseTITLE_BLOCK`, `parseLayer`, `createOldLayerMapping`,
//! `parseLayers` and `parseNETINFO_ITEM`.

use std::collections::{BTreeMap, HashMap};

use kicad_core::page_info::{CustomPageSize, MAX_PAGE_SIZE_PCBNEW_MM, MIN_PAGE_SIZE_MM};
use kicad_core::{EdaDataType, Lset, PageInfo, PageSizeType, PcbLayerId, TitleBlock};
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::PcbParser;
use crate::board::{AddMode, Layer, LayerT};
use crate::ctx::BoardCtx;
use crate::ids::{FootprintId, ItemId, NetCode};
use crate::item::{ParentRef, RecurseMode};
use crate::item_ref::AnyItem;
use crate::pcb_generated_table::GeneratedTableKind;
use crate::pcb_track::Track;

use super::super::{BOARD_FILE_HOST_VERSION, SEXPR_BOARD_FILE_VERSION};

/// What `Parse()` returned (the C++ `BOARD_ITEM*`): the board (held by the parser) or a
/// footprint created in the parser's board (not attached).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedItem {
    /// `parseBOARD()`: the parser's board.
    Board,
    /// `parseFOOTPRINT()`: a created, filled, unattached footprint.
    Footprint(FootprintId),
    /// Staging only (`ParseOptions::allow_pending`): a top-level footprint that a staging stub
    /// skipped; not a board.
    SkippedFootprint,
}

impl PcbParser {
    /// Port of `IsValidBoardHeader`: partially parse the input and check if it matches the
    /// expected header `(kicad_pcb`.
    pub fn is_valid_board_header(&mut self) -> Result<bool, IoError> {
        self.group_infos.clear();
        self.constraint_infos.clear();

        // See Parse() - FOOTPRINTS can be prefixed with an initial block of single line
        // comments, eventually BOARD might be the same
        self.lexer.read_comment_lines()?;

        if self.lexer.cur_tok() != T::T_LEFT {
            return Ok(false);
        }

        if self.lexer.next_tok()? != T::T_kicad_pcb {
            return Ok(false);
        }

        Ok(true)
    }

    /// Port of `PCB_IO_KICAD_SEXPR_PARSER::Parse`: a board (`kicad_pcb`) or a footprint
    /// (`footprint` / legacy `module`, unlocked), then `finishItem`.
    pub fn parse(&mut self) -> Result<ParsedItem, IoError> {
        self.group_infos.clear();
        self.constraint_infos.clear();

        // FOOTPRINTS can be prefixed with an initial block of single line comments and these
        // are kept for Format() so they round trip in s-expression form.  BOARDs might
        // eventually do the same, but currently do not.
        let initial_comments = self.lexer.read_comment_lines()?;

        let token = self.lexer.cur_tok();

        if token == T::T_EOF {
            return Err(self.lexer.unexpected(token));
        }

        if token != T::T_LEFT {
            return Err(self.lexer.expecting(T::T_LEFT));
        }

        let item = match self.lexer.next_tok()? {
            T::T_kicad_pcb => {
                // The parser always owns a board (`m_board == nullptr` => `new BOARD()`).
                self.board_present = true;
                self.parse_board()?;
                ParsedItem::Board
            }
            // legacy token `module`
            T::T_module | T::T_footprint => match self.parse_footprint(initial_comments)? {
                Some(fp) => {
                    // Locking a footprint has no meaning outside of a board.
                    self.board
                        .modify_footprint(fp, |fp, _| fp.base.is_locked = false);
                    ParsedItem::Footprint(fp)
                }
                // Staging (PCBIO-06): the footprint was skipped; there is nothing to finish.
                None => return Ok(ParsedItem::SkippedFootprint),
            },
            _ => {
                let err = format!("Unknown token '{}'", self.lexer.from_utf8());
                return Err(self.lexer.parse_error(err));
            }
        };

        self.finish_item(item)?;

        Ok(item)
    }

    /// Port of `finishItem`: resolve the fonts, groups and constraints of a fully parsed board
    /// or footprint, then clear the deferred lists (the next footprint in a file must not
    /// resolve against them).
    pub(crate) fn finish_item(&mut self, item: ParsedItem) -> Result<(), IoError> {
        let parent = match item {
            ParsedItem::Board => ParentRef::Board,
            ParsedItem::Footprint(fp) => ParentRef::Item(fp.into()),
            ParsedItem::SkippedFootprint => return Ok(()),
        };

        let embedded_fonts: Vec<String> = match item {
            ParsedItem::Board => self.board.embedded_files.update_font_files().to_vec(),
            ParsedItem::Footprint(fp) => match self.board.footprints.get_mut(fp) {
                Some(footprint) => footprint.embedded_files.update_font_files().to_vec(),
                None => Vec::new(),
            },
            ParsedItem::SkippedFootprint => Vec::new(),
        };

        // aItem->RunOnChildren( ResolveFont, RECURSE )
        let mut children: Vec<ItemId> = Vec::new();

        match item {
            ParsedItem::Board => self
                .board
                .run_on_children(RecurseMode::Recurse, &mut |id| children.push(id)),
            ParsedItem::Footprint(fp) => {
                let ctx = BoardCtx::bare(&self.board);

                if let Some(footprint) = self.board.footprints.get(fp) {
                    footprint.run_on_children(&ctx, RecurseMode::Recurse, &mut |child| {
                        children.extend(child.id())
                    });
                }
            }
            ParsedItem::SkippedFootprint => {}
        }

        for child in children {
            // if( EDA_TEXT* textItem = dynamic_cast<EDA_TEXT*>( aChild ) )
            //     textItem->ResolveFont( embeddedFonts );
            self.resolve_child_font(child, &embedded_fonts);
        }

        self.resolve_groups(parent)?;
        self.resolve_constraints(parent)?;

        // The next footprint in a file must not resolve against these
        self.group_infos.clear();
        self.generator_infos.clear();
        self.constraint_infos.clear();

        Ok(())
    }

    /// Port of `parseBOARD`: `parseBOARD_unchecked`, with a `PARSE_ERROR` of a too recent file
    /// turned into a `FUTURE_FORMAT_ERROR` (one that already is one keeps its text).
    fn parse_board(&mut self) -> Result<(), IoError> {
        match self.parse_board_unchecked() {
            Ok(()) => Ok(()),
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

    /// The `checkVersion` lambda of `parseBOARD_unchecked`.
    fn check_board_version(&self) -> Result<(), IoError> {
        if self.required_version > SEXPR_BOARD_FILE_VERSION {
            return Err(kicad_sexpr::FutureFormatError::new(
                &self.required_version.to_string(),
                &self.generator_version,
            )
            .into());
        }

        Ok(())
    }

    /// Port of `parseBOARD_unchecked`: the header, the token loop adding items in file order
    /// with `ADD_MODE::BULK_APPEND`, and the post-processing.
    fn parse_board_unchecked(&mut self) -> Result<(), IoError> {
        // A duplicate key keeps the first value (std::map::insert).
        let mut properties: BTreeMap<String, String> = BTreeMap::new();

        self.parse_header()?;

        // FinalizeBulkAdd( bulkAddedItems ) only notifies BOARD_LISTENERs and bumps the drill
        // model generation; the Rust board has neither, so the list is not collected.

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            if token == T::T_page && self.required_version <= 20200119 {
                token = T::T_paper;
            }

            match token {
                // legacy token
                T::T_host => {
                    self.lexer.need_symbol()?;
                    self.board.generator = self.lexer.from_utf8();

                    // Older formats included build data
                    if self.required_version < BOARD_FILE_HOST_VERSION {
                        self.lexer.need_symbol()?;
                    }

                    self.lexer.need_right()?;
                }
                T::T_generator => {
                    self.lexer.need_symbol()?;
                    self.board.generator = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }
                T::T_generator_version => {
                    self.lexer.need_symbol()?;
                    self.generator_version = self.lexer.from_utf8();
                    self.lexer.need_right()?;

                    // If the format includes a generator version, by this point we have enough
                    // info to do the version check here
                    self.check_board_version()?;
                }
                T::T_general => {
                    // Do another version check here, for older files that do not include
                    // generator_version
                    self.check_board_version()?;

                    self.parse_general_section()?;
                }
                T::T_paper => self.parse_page_info()?,
                T::T_title_block => self.parse_title_block()?,
                T::T_layers => self.parse_layers()?,
                T::T_setup => self.parse_setup()?,
                T::T_property => {
                    let (name, value) = self.parse_board_property()?;
                    properties.entry(name).or_insert(value);
                }
                T::T_variants => self.parse_variants()?,
                T::T_net => self.parse_netinfo_item()?,
                T::T_net_chains => self.parse_net_chains_section()?,
                T::T_net_class => {
                    self.parse_netclass()?;
                    self.board.legacy_netclasses_loaded = true;
                }
                T::T_gr_arc
                | T::T_gr_curve
                | T::T_gr_line
                | T::T_gr_poly
                | T::T_gr_circle
                | T::T_gr_rect
                | T::T_gr_ellipse
                | T::T_gr_ellipse_arc => {
                    if let Some(item) = self.parse_pcb_shape(ParentRef::Board)? {
                        self.board.add_shape(item, AddMode::BulkAppend);
                    }
                }
                T::T_image => {
                    if let Some(item) = self.parse_pcb_reference_image(ParentRef::Board)? {
                        self.board.add_image(item, AddMode::BulkAppend);
                    }
                }
                T::T_barcode => {
                    if let Some(item) = self.parse_pcb_barcode(ParentRef::Board)? {
                        self.board.add_barcode(item, AddMode::BulkAppend);
                    }
                }
                T::T_gr_text => {
                    if let Some(item) = self.parse_pcb_text(ParentRef::Board, None)? {
                        self.board.add_text(item, AddMode::BulkAppend);
                    }
                }
                T::T_gr_text_box => {
                    if let Some(item) = self.parse_pcb_textbox(ParentRef::Board)? {
                        self.board.add_textbox(item, AddMode::BulkAppend);
                    }
                }
                T::T_drill_chart => {
                    let table = self.parse_generated_table(
                        GeneratedTableKind::drill_chart(),
                        ParentRef::Board,
                    )?;

                    if let Some(table) = table {
                        self.board.attach_table(table, AddMode::BulkAppend);
                    }
                }
                T::T_stackup_table => {
                    let table = self.parse_generated_table(
                        GeneratedTableKind::StackupTable,
                        ParentRef::Board,
                    )?;

                    if let Some(table) = table {
                        self.board.attach_table(table, AddMode::BulkAppend);
                    }
                }
                T::T_drill_map => {
                    if let Some(item) = self.parse_pcb_drill_map(ParentRef::Board)? {
                        self.board.add_drill_map(item, AddMode::BulkAppend);
                    }
                }
                T::T_table => {
                    if let Some(table) = self.parse_pcb_table(ParentRef::Board)? {
                        self.board.attach_table(table, AddMode::BulkAppend);
                    }
                }
                T::T_dimension => {
                    if let Some(item) = self.parse_dimension(ParentRef::Board)? {
                        self.board.add_dimension(item, AddMode::BulkAppend);
                    }
                }
                // legacy token `module`
                T::T_module | T::T_footprint => {
                    if let Some(fp) = self.parse_footprint(None)? {
                        self.board.attach_footprint(fp, AddMode::BulkAppend);
                    }
                }
                T::T_segment => {
                    if let Some(track) = self.parse_pcb_track()? {
                        self.add_parsed_track(track);
                    }
                }
                T::T_arc => {
                    if let Some(arc) = self.parse_arc()? {
                        self.add_parsed_track(arc);
                    }
                }
                T::T_group => self.parse_group(ParentRef::Board)?,
                T::T_constraint => self.parse_constraint(ParentRef::Board)?,
                T::T_generated => self.parse_generator(ParentRef::Board)?,
                T::T_via => {
                    if let Some(via) = self.parse_pcb_via()? {
                        self.add_parsed_track(via);
                    }
                }
                T::T_zone => {
                    if let Some(zone) = self.parse_zone(ParentRef::Board)? {
                        // Zones with no outline vertices are degenerate and can cause crashes
                        // elsewhere. Silently discard them. (ZONE::GetNumCorners() ==
                        // Outline()->TotalVertices(); a board zone's outline is its library
                        // outline.)
                        if zone.library_outline().total_vertices() != 0 {
                            self.board.add_zone(zone, AddMode::BulkAppend);
                        }
                    }
                }
                T::T_target => {
                    if let Some(item) = self.parse_pcb_target()? {
                        self.board.add_target(item, AddMode::BulkAppend);
                    }
                }
                T::T_point => {
                    if let Some(item) = self.parse_pcb_point()? {
                        self.board.add_point(item, AddMode::BulkAppend);
                    }
                }
                T::T_grid_item => {
                    if let Some(item) = self.parse_pcb_grid_item()? {
                        self.board.add_grid_item(item, AddMode::BulkAppend);
                    }
                }
                T::T_embedded_fonts => self.parse_board_embedded_fonts()?,
                T::T_embedded_files => self.parse_board_embedded_files()?,
                _ => {
                    let err = format!("Unknown token '{}'", self.lexer.from_utf8());
                    return Err(self.lexer.parse_error(err));
                }
            }

            token = self.lexer.next_tok()?;
        }

        self.board.properties = properties;

        // Re-assemble any barcodes now that board properties (text variables) are available.
        // When barcodes are parsed, AssembleBarcode() is called before board properties are
        // set, so text variables in human-readable text remain unexpanded. Re-assembling now
        // ensures variables like ${PART_NUMBER} are properly expanded in the displayed text.
        self.reassemble_barcodes();

        if !self.undefined_layers.is_empty() {
            // The GUI asks the user (m_queryUserCallback) whether to rescue the items to the
            // Cmts.User layer; headless the load fails. (The C++ computes the destination layer
            // name with GetLayerName before testing for the GUI; it is only used in the
            // dialog.)
            let undefined_layer_names = self
                .undefined_layers
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");

            return Err(IoError::new(format!(
                "One or more items were found on undefined layers ({undefined_layer_names}). \
                 Open the board in the PCB Editor to resolve."
            )));
        }

        // Clear unused zone data
        let zone_ids: Vec<_> = self.board.zones().map(|(id, _)| id).collect();

        if !zone_ids.is_empty() {
            let layers = self.board.enabled_layers();

            // z->SetLayerSetAndRemoveUnusedFills( z->GetLayerSet() & layers ) for every zone
            // (PCBIO-08; derived state, no time-stamp bump, like C++).
            for id in zone_ids {
                if let Some(z) = self.board.zones.get_mut(id) {
                    let zone_layers = z.layer_set.clone() & &layers;
                    z.set_layer_set_and_remove_unused_fills(zone_layers);
                }
            }
        }

        // Appended items get fresh UUIDs, so remap the links read in this parse and leave
        // existing zones alone. Without append mode (PCBIO-16) m_resetKIIDMap is empty and
        // no link set is replaced, so the loop over m_linkedZones changes nothing.
        debug_assert!(!self.append_to_existing && self.reset_kiid_map.is_empty());

        // Ensure all footprints have their embedded data from the board
        self.board.fixup_embedded_data();

        // Remap terminal pad UUIDs through the reset map (append mode only, empty here), then
        // net->ResolveTerminalPads( m_board ) for every net.
        self.board.resolve_net_chain_terminal_pads();

        // m_board->RefreshDrillSymbolLayers(): view-only state (the layers and placements the
        // pads and vias report to the view); the Rust board has no such fields.

        Ok(())
    }

    /// `m_board->Add( track, ADD_MODE::BULK_APPEND, true )` for a parsed segment, arc or via.
    /// The parser returns no track on a non-copper layer, so the board's refusal (`Err`) is
    /// not expected; such a track is dropped like the C++ parser drops it.
    fn add_parsed_track(&mut self, track: Track) {
        let _ = self.board.add_track(track, AddMode::BulkAppend);
    }

    /// The `AssembleBarcode()` loops of `parseBOARD_unchecked`: board drawings, then the
    /// graphical items of every footprint.
    fn reassemble_barcodes(&mut self) {
        let ctx = BoardCtx::bare(&self.board);

        for (_, item) in self.board.drawings() {
            if let AnyItem::Barcode(barcode) = item {
                barcode.assemble_barcode(&ctx);
            }
        }

        for (fp, _) in self.board.footprints() {
            for (_, item) in self.board.footprint_drawings(fp) {
                if let AnyItem::Barcode(barcode) = item {
                    barcode.assemble_barcode(&ctx);
                }
            }
        }
    }

    /// Port of `parseHeader`: `(version N)` (if the first list is something else its keyword
    /// is swallowed and the version becomes 20201115), the too-recent flag, the bar token and
    /// the board's `SetFileFormatVersionAtLoad`.
    fn parse_header(&mut self) -> Result<(), IoError> {
        debug_assert!(
            self.lexer.cur_tok() == T::T_kicad_pcb,
            "Cannot parse {} as a header.",
            self.lexer.token_string(self.lexer.cur_tok())
        );

        self.lexer.need_left()?;

        let tok = self.lexer.next_tok()?;

        if tok == T::T_version {
            let expected = self.lexer.from_utf8();
            self.required_version = self.parse_int_expected(&expected)?;
            self.lexer.need_right()?;
        } else {
            // Last version before we started writing version #s in footprint files as well as
            // board files.
            self.required_version = 20201115;
        }

        self.too_recent = self.required_version > SEXPR_BOARD_FILE_VERSION;

        // Prior to this, bar was a valid string char for unquoted strings.
        self.lexer.set_knows_bar(self.required_version >= 20240706);

        self.board.file_format_version_at_load = self.required_version;

        Ok(())
    }

    /// Port of `parseGeneralSection`: `(thickness mm)`, `(legacy_teardrops [yes|no])`; every
    /// other entry is skipped if it holds only symbols and numbers.
    fn parse_general_section(&mut self) -> Result<(), IoError> {
        debug_assert!(self.lexer.cur_tok() == T::T_general);

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_thickness => {
                    let thickness =
                        self.parse_board_units_tok(T::T_thickness, EdaDataType::Distance)?;

                    // SetBoardThickness
                    self.board.design_settings_mut().board_thickness = thickness;
                    self.lexer.need_right()?;
                }
                T::T_legacy_teardrops => {
                    self.board.legacy_teardrops = self.parse_maybe_absent_bool(true)?;
                }
                _ => {
                    // Skip everything else.
                    loop {
                        token = self.lexer.next_tok()?;

                        if token == T::T_RIGHT {
                            break;
                        }

                        if !kicad_sexpr::DsnLexer::<T>::is_symbol(token) && token != T::T_NUMBER {
                            return Err(self.lexer.expecting_str("symbol or number"));
                        }
                    }
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parsePAGE_INFO`: `"A4" | "User" w h [portrait])`, user sizes in mm clamped to
    /// `MIN_PAGE_SIZE_MM .. MAX_PAGE_SIZE_PCBNEW_MM`.
    #[allow(clippy::manual_clamp)] // the C++ comparison chain
    fn parse_page_info(&mut self) -> Result<(), IoError> {
        debug_assert!(
            (self.lexer.cur_tok() == T::T_page && self.required_version <= 20200119)
                || self.lexer.cur_tok() == T::T_paper
        );

        let mut page_info = PageInfo::default();

        self.lexer.need_symbol()?;

        let page_type = self.lexer.from_utf8();

        // PAGE_INFO::SetType( wxString ) with the default IsPortrait = false; a User page takes
        // the process-wide custom size (s_user_width / s_user_height, never changed by the CLI).
        if !page_info.set_type_by_name(&page_type, false, &CustomPageSize::default()) {
            let err = format!("Page type '{}' is not valid.", self.lexer.from_utf8());
            return Err(self.lexer.parse_error(err));
        }

        if page_info.type_() == PageSizeType::User {
            let mut width = self.parse_double_expected("width")?; // width in mm

            // Perform some controls to avoid crashes if the size is edited by hands
            if width < MIN_PAGE_SIZE_MM {
                width = MIN_PAGE_SIZE_MM;
            } else if width > MAX_PAGE_SIZE_PCBNEW_MM {
                width = MAX_PAGE_SIZE_PCBNEW_MM;
            }

            let mut height = self.parse_double_expected("height")?; // height in mm

            if height < MIN_PAGE_SIZE_MM {
                height = MIN_PAGE_SIZE_MM;
            } else if height > MAX_PAGE_SIZE_PCBNEW_MM {
                height = MAX_PAGE_SIZE_PCBNEW_MM;
            }

            page_info.set_width_mm(width);
            page_info.set_height_mm(height);
        }

        let token = self.lexer.next_tok()?;

        if token == T::T_portrait {
            page_info.set_portrait(true);
            self.lexer.need_right()?;
        } else if token != T::T_RIGHT {
            return Err(self.lexer.expecting_str("portrait|)"));
        }

        // SetPageSettings
        self.board.paper = page_info;

        Ok(())
    }

    /// Port of `parseTITLE_BLOCK`: `(title ..) (date ..) (rev ..) (company ..) (comment N ..)`
    /// with N in 1..9.
    fn parse_title_block(&mut self) -> Result<(), IoError> {
        debug_assert!(self.lexer.cur_tok() == T::T_title_block);

        let mut title_block = TitleBlock::default();
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_title => {
                    self.lexer.next_tok()?;
                    title_block.set_title(&self.lexer.from_utf8());
                }
                T::T_date => {
                    self.lexer.next_tok()?;
                    title_block.set_date(&self.lexer.from_utf8());
                }
                T::T_rev => {
                    self.lexer.next_tok()?;
                    title_block.set_revision(&self.lexer.from_utf8());
                }
                T::T_company => {
                    self.lexer.next_tok()?;
                    title_block.set_company(&self.lexer.from_utf8());
                }
                T::T_comment => {
                    let comment_number = self.parse_int_expected("comment")?;

                    if !(1..=9).contains(&comment_number) {
                        let err =
                            format!("{comment_number} is not a valid title block comment number");
                        return Err(self.lexer.parse_error(err));
                    }

                    self.lexer.next_tok()?;
                    title_block.set_comment((comment_number - 1) as usize, &self.lexer.from_utf8());
                }
                _ => {
                    return Err(self
                        .lexer
                        .expecting_str("title, date, rev, company, or comment"));
                }
            }

            self.lexer.need_right()?;

            token = self.lexer.next_tok()?;
        }

        // SetTitleBlock
        self.board.titles = title_block;

        Ok(())
    }

    /// Port of `parseLayer( LAYER* aLayer )`: `(N "name" type ["user name" | hide])`, called
    /// with `CurTok() == T_LEFT`. The number is kept but not used by `parseLayers`.
    fn parse_layer(&mut self, layer: &mut Layer) -> Result<(), IoError> {
        let mut user_name = String::new();
        let mut is_visible = true;

        // aLayer->clear()
        *layer = Layer::default();

        if self.lexer.cur_tok() != T::T_LEFT {
            return Err(self.lexer.expecting(T::T_LEFT));
        }

        // this layer_num is not used, we DO depend on LAYER_T however.
        let layer_num = self.parse_int_expected("layer index")?;

        self.lexer.need_symbol_or_number()?;
        let name = self.lexer.cur_text().to_string();

        self.lexer.need_symbol()?;
        let type_ = self.lexer.cur_text().to_string();

        let token = self.lexer.next_tok()?;

        // @todo Figure out why we are looking for a hide token in the layer definition.
        if token == T::T_hide {
            is_visible = false;
            self.lexer.need_right()?;
        } else if token == T::T_STRING {
            user_name = self.lexer.cur_text().to_string();
            self.lexer.need_right()?;
        } else if token != T::T_RIGHT {
            return Err(self.lexer.expecting_str("hide, user defined name, or )"));
        }

        layer.type_ = LayerT::parse_type(&type_);
        layer.number = layer_num;
        layer.visible = is_visible;

        if self.required_version >= 20200922 {
            layer.user_name = user_name;
            layer.name = name;
        } else {
            // Older versions didn't have a dedicated user name field
            layer.user_name = name.clone();
            layer.name = name;
        }

        Ok(())
    }

    /// Port of `createOldLayerMapping`: the layer names of the (short-lived) bug where they
    /// were translated, to the English names. Only Italian, Polish and French, the only
    /// languages that mapped the layer names as of cc2022b1ac739aa673d2a0b7a2047638aa7a47b3
    /// (kicad-i18n) when the bug was fixed in KiCad source.
    fn create_old_layer_mapping() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            // Italian
            ("Adesivo.Retro", "B.Adhes"),
            ("Adesivo.Fronte", "F.Adhes"),
            ("Pasta.Retro", "B.Paste"),
            ("Pasta.Fronte", "F.Paste"),
            ("Serigrafia.Retro", "B.SilkS"),
            ("Serigrafia.Fronte", "F.SilkS"),
            ("Maschera.Retro", "B.Mask"),
            ("Maschera.Fronte", "F.Mask"),
            ("Grafica", "Dwgs.User"),
            ("Commenti", "Cmts.User"),
            ("Eco1", "Eco1.User"),
            ("Eco2", "Eco2.User"),
            ("Contorno.scheda", "Edge.Cuts"),
            // Polish
            ("Kleju_Dolna", "B.Adhes"),
            ("Kleju_Gorna", "F.Adhes"),
            ("Pasty_Dolna", "B.Paste"),
            ("Pasty_Gorna", "F.Paste"),
            ("Opisowa_Dolna", "B.SilkS"),
            ("Opisowa_Gorna", "F.SilkS"),
            ("Maski_Dolna", "B.Mask"),
            ("Maski_Gorna", "F.Mask"),
            ("Rysunkowa", "Dwgs.User"),
            ("Komentarzy", "Cmts.User"),
            ("ECO1", "Eco1.User"),
            ("ECO2", "Eco2.User"),
            ("Krawedziowa", "Edge.Cuts"),
            // French
            ("Dessous.Adhes", "B.Adhes"),
            ("Dessus.Adhes", "F.Adhes"),
            ("Dessous.Pate", "B.Paste"),
            ("Dessus.Pate", "F.Paste"),
            ("Dessous.SilkS", "B.SilkS"),
            ("Dessus.SilkS", "F.SilkS"),
            ("Dessous.Masque", "B.Mask"),
            ("Dessus.Masque", "F.Mask"),
            ("Dessin.User", "Dwgs.User"),
            ("Contours.Ci", "Edge.Cuts"),
        ])
    }

    /// Port of `parseLayers`: the copper stack (identified by position: first = F.Cu, last =
    /// B.Cu, inner ones by canonical name or `(index + 1) * 2`; it ends at the first layer whose
    /// type is not a copper type), then the technical and user layers by name (translated
    /// legacy names accepted), the layer descriptors, the layer maps, the copper count (even,
    /// >= 2) and the enabled layers.
    fn parse_layers(&mut self) -> Result<(), IoError> {
        debug_assert!(self.lexer.cur_tok() == T::T_layers);

        let mut visible_layers = Lset::new();
        let mut enabled_layers = Lset::new();
        let mut copper_layer_count: i32 = 0;
        let mut layer = Layer::default();
        let mut any_hidden = false;

        let v3_layer_names = Self::create_old_layer_mapping();
        let mut cu: Vec<Layer> = Vec::new();

        // (Append mode only: the destination layers before the append, PCBIO-16.)

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            self.parse_layer(&mut layer)?;

            if layer.type_ == LayerT::Undefined {
                // it's a non-copper layer
                break;
            }

            cu.push(layer.clone()); // it's copper

            token = self.lexer.next_tok()?;
        }

        // All Cu layers are parsed, but not the non-cu layers here.

        // The original *.kicad_pcb file format and the inverted
        // Cu stack format both have all the Cu layers first, so use this
        // trick to handle either. The layer number in the (layers ..)
        // s-expression element are ignored.
        if !cu.is_empty() {
            // Rework the layer numbers, which changed when the Cu stack
            // was flipped.  So we instead use position in the list.
            for i in 1..cu.len().saturating_sub(1) {
                let mut tmp_layer = Lset::name_to_layer(&cu[i].name);

                if tmp_layer < 0 {
                    tmp_layer = (i as i32 + 1) * 2;
                }

                cu[i].number = tmp_layer;
            }

            cu[0].number = PcbLayerId::FCu.0;
            let last = cu.len() - 1;
            cu[last].number = PcbLayerId::BCu.0;

            for cu_layer in &cu {
                enabled_layers.set(PcbLayerId(cu_layer.number));

                if cu_layer.visible {
                    visible_layers.set(PcbLayerId(cu_layer.number));
                } else {
                    any_hidden = true;
                }

                // (m_preserveDestinationStackup is false: append mode only.)
                self.board
                    .set_layer_descr(PcbLayerId(cu_layer.number), cu_layer.clone());

                let name = cu_layer.name.clone();

                self.layer_indices
                    .insert(name.clone(), PcbLayerId(cu_layer.number));
                self.layer_masks
                    .insert(name, Lset::from_layers(&[PcbLayerId(cu_layer.number)]));
            }

            copper_layer_count = cu.len() as i32;
        }

        // process non-copper layers
        while token != T::T_RIGHT {
            let found = match self.layer_indices.get(&layer.name) {
                Some(&id) => Some((layer.name.clone(), id)),
                None => v3_layer_names.get(layer.name.as_str()).and_then(|english| {
                    self.layer_indices
                        .get(*english)
                        .map(|&id| ((*english).to_string(), id))
                }),
            };

            let Some((key, id)) = found else {
                return Err(IoError::new(format!(
                    "Layer '{}' in file '{}' at line {} is not in fixed layer hash.",
                    layer.name,
                    self.lexer.cur_source(),
                    self.lexer.cur_line_number()
                )));
            };

            if key != layer.name {
                // If we are here, then we have found a translated layer name.  Put it in the
                // maps so that items on this layer get the appropriate layer ID number.
                self.layer_indices.insert(layer.name.clone(), id);
                self.layer_masks
                    .insert(layer.name.clone(), Lset::from_layers(&[id]));
                layer.name = key;
            }

            layer.number = id.0;
            enabled_layers.set(PcbLayerId(layer.number));

            if layer.visible {
                visible_layers.set(PcbLayerId(layer.number));
            } else {
                any_hidden = true;
            }

            self.board.set_layer_descr(id, layer.clone());

            token = self.lexer.next_tok()?;

            if token != T::T_LEFT {
                break;
            }

            self.parse_layer(&mut layer)?;
        }

        // We need at least 2 copper layers and there must be an even number of them.
        if copper_layer_count < 2 || (copper_layer_count % 2) != 0 {
            let err = format!("{copper_layer_count} is not a valid layer count");
            return Err(self.lexer.parse_error(err));
        }

        self.board.set_copper_layer_count(copper_layer_count);
        self.board.set_enabled_layers(enabled_layers);

        // Only set this if any layers were explicitly marked as hidden.  Otherwise, we want to
        // leave this alone; default visibility will show everything. (m_LegacyVisibleLayers is
        // GUI state that is never written back and has no field on the Rust board.)
        let _ = (any_hidden, visible_layers);

        Ok(())
    }

    /// Port of `parseNETINFO_ITEM`: a legacy net declaration `(net N "name")` (files before
    /// 20251028). Net 0 should be already in the list; any other net is found or added by name
    /// with the requested code (`AppendNet` keeps it only if it is the next code), and the
    /// file code -> board code mapping is recorded.
    fn parse_netinfo_item(&mut self) -> Result<(), IoError> {
        debug_assert!(self.lexer.cur_tok() == T::T_net);

        let net_code = self.parse_int_expected("net number")?;

        self.lexer.need_symbol_or_number()?;
        let mut name = self.lexer.from_utf8();

        // Convert overbar syntax from `~...~` to `~{...}`.  These were left out of the first
        // merge so the version is a bit later.
        if self.required_version < 20210606 {
            name = kicad_core::string_utils::convert_to_new_overbar_notation(&name);
        }

        self.lexer.need_right()?;

        // net 0 should be already in list, so store this net
        // if it is not the net 0, or if the net 0 does not exists.
        // (TODO: a better test.)
        if net_code > NetCode::UNCONNECTED.0 || self.board.find_net(NetCode::UNCONNECTED).is_none()
        {
            let code = self.board.find_or_add_net(&name, Some(NetCode(net_code)));

            // Store the new code mapping
            self.push_value_into_map(net_code, code.0);
        }

        Ok(())
    }
}
