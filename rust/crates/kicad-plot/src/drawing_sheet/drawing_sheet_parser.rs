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

//! Port of `common/drawing_sheet/drawing_sheet_parser.cpp` (`DRAWING_SHEET_PARSER`,
//! `convertLegacyVariableRefs`) with the tokens of `drawing_sheet.keywords`
//! (`kicad_sexpr::lexer::keywords::drawingsheet_t`) and `include/drawing_sheet/
//! ds_file_versions.h`: the `.kicad_wks` reader, including the legacy unversioned
//! `page_layout` / `drawing_sheet` forms and the `%` format codes.
//!
//! The `DS_DATA_MODEL` members of the file (`SetDefaultLayout`, `SetEmptyLayout`,
//! `SetPageLayout`, `LoadDrawingSheet`, ...) are in [`super::ds_data_model`].

use kicad_core::color4d::Color4D;
use kicad_core::string_utils::convert_to_new_overbar_notation;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT};
use kicad_geom::{EdaAngle, Vector2D};
use kicad_sexpr::lexer::keywords::drawingsheet_t::T;
use kicad_sexpr::{DsnLexer, FutureFormatError, IoError};

use super::ds_data_item::{
    CornerAnchor, DS_MAX_REPEAT_COUNT, DsDataItem, DsDataItemBitmap, DsDataItemKind, DsItemType,
    ImageProbe, PageOption, PointCoord,
};
use super::ds_data_model::DsDataModel;

/// `SEXPR_WORKSHEET_FILE_VERSION` (`ds_file_versions.h`): 20231118, generator version; V8
/// file format cleanup.
pub const SEXPR_WORKSHEET_FILE_VERSION: i32 = 20231118;

/// Port of `convertLegacyVariableRefs`: the legacy `%` format codes become text variables.
///
/// * `%%` = replaced by `%`
/// * `%K` = Kicad version
/// * `%Z` = paper format name (A4, USLetter)
/// * `%Y` = company name
/// * `%D` = date
/// * `%R` = revision
/// * `%S` = sheet number
/// * `%N` = number of sheets
/// * `%L` = layer name
/// * `%Cx` = comment (x = 0 to 9 to identify the comment)
/// * `%F` = filename
/// * `%P` = sheet path (sheet full name)
/// * `%T` = title
pub fn convert_legacy_variable_refs(a_textbase: &str) -> String {
    let text: Vec<char> = a_textbase.chars().collect();
    let mut msg = String::new();
    let mut ii: usize = 0;

    while ii < text.len() {
        if text[ii] != '%' {
            msg.push(text[ii]);
            ii += 1;
            continue;
        }

        ii += 1;

        if ii >= text.len() {
            break;
        }

        let format = text[ii];

        match format {
            '%' => msg.push('%'),
            'D' => msg.push_str("${ISSUE_DATE}"),
            'R' => msg.push_str("${REVISION}"),
            'K' => msg.push_str("${KICAD_VERSION}"),
            'Z' => msg.push_str("${PAPER}"),
            'S' => msg.push_str("${#}"),
            'N' => msg.push_str("${##}"),
            'F' => msg.push_str("${FILENAME}"),
            'L' => msg.push_str("${LAYER}"),
            'P' => msg.push_str("${SHEETPATH}"),
            'Y' => msg.push_str("${COMPANY}"),
            'T' => msg.push_str("${TITLE}"),
            'C' => {
                // format = aTextbase[++ii]: past the end wxString::operator[] reads the
                // terminating NUL (no comment)
                ii += 1;
                let format = text.get(ii).copied().unwrap_or('\0');

                match format {
                    '0' => msg.push_str("${COMMENT1}"),
                    '1' => msg.push_str("${COMMENT2}"),
                    '2' => msg.push_str("${COMMENT3}"),
                    '3' => msg.push_str("${COMMENT4}"),
                    '4' => msg.push_str("${COMMENT5}"),
                    '5' => msg.push_str("${COMMENT6}"),
                    '6' => msg.push_str("${COMMENT7}"),
                    '7' => msg.push_str("${COMMENT8}"),
                    '8' => msg.push_str("${COMMENT9}"),
                    _ => {}
                }
            }
            _ => {}
        }

        ii += 1;
    }

    msg
}

/// Port of `class DRAWING_SHEET_PARSER`: holds data and functions pertinent to parsing a
/// S-expression file for a `DS_DATA_MODEL`.
pub struct DrawingSheetParser {
    lexer: DsnLexer<T>,
    /// `m_requiredVersion`
    required_version: i32,
    /// `m_generatorVersion`
    generator_version: String,
    /// Reads the bitmaps' image headers (see [`ImageProbe`]).
    image_probe: Option<ImageProbe>,
}

impl DrawingSheetParser {
    /// Port of `DRAWING_SHEET_PARSER::DRAWING_SHEET_PARSER( const char* aLine, const wxString&
    /// aSource )`.
    pub fn new(a_line: &str, a_source: &str, image_probe: Option<ImageProbe>) -> Self {
        DrawingSheetParser {
            lexer: DsnLexer::<T>::new(a_line, a_source),
            required_version: 0,
            generator_version: String::new(),
            image_probe,
        }
    }

    fn unexpected(&self) -> IoError {
        self.lexer.unexpected_str(self.lexer.cur_text())
    }

    /// Port of `DRAWING_SHEET_PARSER::Parse( DS_DATA_MODEL* aLayout )`. As in C++ the items
    /// read before an error stay in the model.
    pub fn parse(&mut self, a_layout: &mut DsDataModel) -> Result<(), IoError> {
        self.lexer.need_left()?;
        let mut token = self.lexer.next_tok()?;

        self.parse_header(token)?;
        a_layout.set_file_format_version_at_load(self.required_version);

        // Sheets predating the generator token, or a reused model, must not inherit a stale
        // generator.
        a_layout.set_generator("pl_editor");

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_generator => {
                    self.lexer.need_symbol()?;
                    a_layout.set_generator(&self.lexer.from_utf8());
                    self.lexer.need_right()?;
                }

                T::T_generator_version => {
                    self.lexer.next_tok()?;
                    self.generator_version = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_setup => {
                    // Defines default values for graphic items.
                    // Check the version here, because the generator and generator_version (if
                    // available) will have been parsed by now given the order the formatter
                    // writes them in
                    if self.required_version > SEXPR_WORKSHEET_FILE_VERSION {
                        return Err(FutureFormatError::new(
                            &format!("{}", self.required_version),
                            &self.generator_version,
                        )
                        .into());
                    }

                    self.parse_setup(a_layout)?;
                }

                // An exception in the item parsers leaks the C++ item: it is not appended.
                T::T_line => {
                    let mut item = DsDataItem::new(DsItemType::Segment);
                    self.parse_graphic(&mut item)?;
                    a_layout.append(item);
                }

                T::T_rect => {
                    let mut item = DsDataItem::new(DsItemType::Rect);
                    self.parse_graphic(&mut item)?;
                    a_layout.append(item);
                }

                T::T_polygon => {
                    let mut item = DsDataItem::new_polygons();
                    self.parse_polygon(&mut item)?;
                    a_layout.append(item);
                }

                T::T_bitmap => {
                    let mut item = DsDataItem::new_bitmap(DsDataItemBitmap::default());
                    self.parse_bitmap(&mut item)?;

                    // Drop invalid bitmaps
                    if item.bitmap().is_some_and(|b| b.has_image) {
                        a_layout.append(item);
                    }
                }

                T::T_tbtext => {
                    self.lexer.need_symbol_or_number()?;
                    let mut item = DsDataItem::new_text(&convert_legacy_variable_refs(
                        &self.lexer.from_utf8(),
                    ));
                    self.parse_text(&mut item)?;
                    a_layout.append(item);
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseHeader`: the data specified at the very beginning
    /// of the file, like version and the application used to create this file.
    fn parse_header(&mut self, a_header_type: T) -> Result<(), IoError> {
        // The older files had no versioning and their first token after the initial left
        // parenthesis was a `page_layout` or `drawing_sheet` token. The newer files have
        // versions and have a `kicad_wks` token instead.
        if a_header_type == T::T_kicad_wks || a_header_type == T::T_drawing_sheet {
            self.lexer.need_left()?;

            let tok = self.lexer.next_tok()?;

            if tok == T::T_version {
                self.required_version = self.parse_int()?;
                self.lexer.need_right()?;
            } else {
                return Err(self.lexer.expecting(T::T_version));
            }
        } else {
            // We assign version 0 to files that were created before there was any versioning
            // of worksheets. The below line is not strictly necessary, as
            // `m_requiredVersion` is already initialized to 0 in the constructor.
            self.required_version = 0;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseSetup`.
    fn parse_setup(&mut self, a_layout: &mut DsDataModel) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            match token {
                T::T_LEFT => {}

                T::T_linewidth => {
                    a_layout.env.default_line_width = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_textsize => {
                    a_layout.env.default_text_size.x = self.parse_double()?;
                    a_layout.env.default_text_size.y = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_textlinewidth => {
                    a_layout.env.default_text_thickness = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_left_margin => {
                    let v = self.parse_double()?;
                    a_layout.set_left_margin(v);
                    self.lexer.need_right()?;
                }

                T::T_right_margin => {
                    let v = self.parse_double()?;
                    a_layout.set_right_margin(v);
                    self.lexer.need_right()?;
                }

                T::T_top_margin => {
                    let v = self.parse_double()?;
                    a_layout.set_top_margin(v);
                    self.lexer.need_right()?;
                }

                T::T_bottom_margin => {
                    let v = self.parse_double()?;
                    a_layout.set_bottom_margin(v);
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        // The file is well-formed.  If it has no further items, then that's the way the
        // user wants it.
        a_layout.allow_void_list(true);
        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parsePolygon`: a polygon item starting by
    /// `( polygon`; the list of corners is read by `parsePolyOutline`.
    fn parse_polygon(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_comment => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.info = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_pos => self.parse_coordinate(&mut a_item.pos)?,

                T::T_name => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_option => self.read_option(a_item)?,

                T::T_pts => {
                    self.parse_poly_outline(a_item)?;
                    a_item.close_contour();
                }

                T::T_rotate => {
                    let v = self.parse_double()?;

                    if let DsDataItemKind::Polygons(p) = &mut a_item.kind {
                        p.orient = EdaAngle::from_degrees(v);
                    }

                    self.lexer.need_right()?;
                }

                T::T_repeat => {
                    a_item.repeat_count = self.parse_int_range(1, DS_MAX_REPEAT_COUNT)?;
                    self.lexer.need_right()?;
                }

                T::T_incrx => {
                    a_item.increment_vector.x = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_incry => {
                    a_item.increment_vector.y = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_linewidth => {
                    a_item.line_width = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        a_item.set_bounding_box();
        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parsePolyOutline`: a list of corners starting by
    /// `( pts`.
    fn parse_poly_outline(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut corner = Vector2D::new(0.0, 0.0);
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_xy => {
                    corner.x = self.parse_double()?;
                    corner.y = self.parse_double()?;
                    a_item.append_corner(corner);
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The image of a bitmap item (`BITMAP_BASE::ReadImageFile( wxMemoryBuffer& )` /
    /// `SetImage`): true if the image could be read.
    fn read_image(&self, bitmap: &mut DsDataItemBitmap, data: Vec<u8>) -> bool {
        bitmap.image_data = data;

        let info = match self.image_probe {
            Some(probe) => probe(&bitmap.image_data),
            None => fallback_image_check(&bitmap.image_data),
        };

        match info {
            Some(info) if info.width != 0 && info.height != 0 => {
                bitmap.has_image = true;

                if self.image_probe.is_some() {
                    bitmap.image_info = Some(info);

                    // updatePPI(): a usable resolution replaces m_ppi (300 before)
                    bitmap.ppi = info.ppi;
                }

                true
            }
            _ => false,
        }
    }

    /// Port of `DRAWING_SHEET_PARSER::parseBitmap`: a bitmap item starting by `( bitmap`.
    fn parse_bitmap(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_name => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_pos => self.parse_coordinate(&mut a_item.pos)?,

                T::T_repeat => {
                    a_item.repeat_count = self.parse_int_range(1, DS_MAX_REPEAT_COUNT)?;
                    self.lexer.need_right()?;
                }

                T::T_incrx => {
                    a_item.increment_vector.x = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_incry => {
                    a_item.increment_vector.y = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_linewidth => {
                    a_item.line_width = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_scale => {
                    let v = self.parse_double()?;

                    if let DsDataItemKind::Bitmap(b) = &mut a_item.kind {
                        b.scale = v;
                    }

                    self.lexer.need_right()?;
                }

                T::T_comment => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.info = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_data => {
                    token = self.lexer.next_tok()?;
                    let mut data = String::new();

                    while token != T::T_RIGHT {
                        if !DsnLexer::<T>::is_symbol(token) {
                            return Err(self.lexer.expecting_str("base64 image data"));
                        }

                        data.push_str(&self.lexer.from_utf8());
                        token = self.lexer.next_tok()?;
                    }

                    // wxBase64Decode: an empty buffer on error
                    let buffer = wx_base64_decode(&data).unwrap_or_default();

                    let ok = match &mut a_item.kind {
                        DsDataItemKind::Bitmap(b) => {
                            let mut b2 = std::mem::take(b);
                            let ok = self.read_image(&mut b2, buffer);
                            *b = b2;
                            ok
                        }
                        _ => false,
                    };

                    if !ok {
                        return Err(IoError::new("Failed to read image data."));
                    }
                }

                T::T_pngdata => self.read_pngdata(a_item)?,

                T::T_option => self.read_option(a_item)?,

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::readPngdata` and the `BITMAP_BASE::LoadLegacyData` it
    /// calls: hexadecimal image data, one `(data "..")` per line.
    fn read_pngdata(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut tmp: Vec<String> = Vec::new();
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_data => {
                    self.lexer.need_symbol_or_number()?;
                    tmp.push(self.lexer.cur_str().to_string());
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        // LoadLegacyData: read the lines until "EndData" (strncasecmp of 4 chars), each byte
        // = 2 hexadecimal digits and a space between 2 bytes.
        let mut stream: Vec<u8> = Vec::new();
        let mut end_seen = false;

        for line in tmp
            .iter()
            .map(|s| s.as_str())
            .chain(std::iter::once("EndData"))
        {
            // A (data ..) token may hold line breaks: STRING_LINE_READER splits on them.
            for line in line.split_inclusive('\n') {
                if line.len() >= 4 && line.as_bytes()[..4].eq_ignore_ascii_case(b"EndD") {
                    end_seen = true;
                    break;
                }

                let bytes = line.as_bytes();
                let mut pos = 0usize;
                let mut len = bytes.len() as i64;

                while len > 0 {
                    match scanf_hex(&bytes[pos.min(bytes.len())..]) {
                        Some(value) => stream.push(value as u8),
                        None => break,
                    }

                    len -= 3;
                    pos += 3;
                }
            }

            if end_seen {
                break;
            }
        }

        if let DsDataItemKind::Bitmap(b) = &mut a_item.kind {
            // m_originalImage = new wxImage( *m_image ) even when LoadFile fails: a legacy
            // bitmap is never dropped; the resolution is not read (no updatePPI).
            b.image_data = stream;
            b.has_image = true;

            if let Some(probe) = self.image_probe {
                b.image_info = probe(&b.image_data);
            }
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::readOption`.
    fn read_option(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            match token {
                T::T_page1only => a_item.set_page1_option(PageOption::FirstPageOnly),
                T::T_notonpage1 => a_item.set_page1_option(PageOption::SubsequentPages),
                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseGraphic`: a graphic item starting by `(line` or
    /// `(rect`.
    fn parse_graphic(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            } else {
                // If another token than T_LEFT is read here, this is an error
                // however, due to a old bug in kicad, the token T_end can be found
                // without T_LEFT in a very few .wks files (perhaps only one in a demo).
                // So this ugly hack disables the error detection.
                if token != T::T_end {
                    return Err(self.unexpected());
                }
            }

            match token {
                T::T_comment => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.info = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_option => self.read_option(a_item)?,

                T::T_name => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_start => self.parse_coordinate(&mut a_item.pos)?,

                T::T_end => self.parse_coordinate(&mut a_item.end)?,

                T::T_repeat => {
                    a_item.repeat_count = self.parse_int_range(1, DS_MAX_REPEAT_COUNT)?;
                    self.lexer.need_right()?;
                }

                T::T_incrx => {
                    a_item.increment_vector.x = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_incry => {
                    a_item.increment_vector.y = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_linewidth => {
                    a_item.line_width = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseText`: a text item starting by `(tbtext`.
    fn parse_text(&mut self, a_item: &mut DsDataItem) -> Result<(), IoError> {
        if self.required_version < 20210606 {
            if let DsDataItemKind::Text(t) = &mut a_item.kind {
                t.text_base = convert_to_new_overbar_notation(&t.text_base);
            }
        }

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            match token {
                T::T_comment => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.info = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_option => self.read_option(a_item)?,

                T::T_name => {
                    self.lexer.need_symbol_or_number()?;
                    a_item.name = self.lexer.from_utf8();
                    self.lexer.need_right()?;
                }

                T::T_pos => self.parse_coordinate(&mut a_item.pos)?,

                T::T_repeat => {
                    a_item.repeat_count = self.parse_int_range(1, DS_MAX_REPEAT_COUNT)?;
                    self.lexer.need_right()?;
                }

                T::T_incrx => {
                    a_item.increment_vector.x = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_incry => {
                    a_item.increment_vector.y = self.parse_double()?;
                    self.lexer.need_right()?;
                }

                T::T_incrlabel => {
                    a_item.increment_label = self.parse_int_range(i32::MIN, i32::MAX)?;
                    self.lexer.need_right()?;
                }

                T::T_maxlen => {
                    let v = self.parse_double()?;
                    text_mut(a_item).bounding_box_size.x = v;
                    self.lexer.need_right()?;
                }

                T::T_maxheight => {
                    let v = self.parse_double()?;
                    text_mut(a_item).bounding_box_size.y = v;
                    self.lexer.need_right()?;
                }

                T::T_font => {
                    let mut face_name = String::new();
                    text_mut(a_item).text_color = Color4D::UNSPECIFIED;

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT && token != T::T_EOF {
                        match token {
                            T::T_LEFT => {}

                            T::T_face => {
                                self.lexer.need_symbol()?;
                                face_name = self.lexer.from_utf8();
                                self.lexer.need_right()?;
                            }

                            T::T_bold => text_mut(a_item).bold = true,

                            T::T_italic => text_mut(a_item).italic = true,

                            T::T_size => {
                                let x = self.parse_double()?;
                                text_mut(a_item).text_size.x = x;
                                let y = self.parse_double()?;
                                text_mut(a_item).text_size.y = y;
                                self.lexer.need_right()?;
                            }

                            T::T_color => {
                                let r = self.parse_int_range(0, 255)? as f64 / 255.0;
                                text_mut(a_item).text_color.r = r;
                                let g = self.parse_int_range(0, 255)? as f64 / 255.0;
                                text_mut(a_item).text_color.g = g;
                                let b = self.parse_int_range(0, 255)? as f64 / 255.0;
                                text_mut(a_item).text_color.b = b;
                                let a = self.parse_double()?.clamp(0.0, 1.0);
                                text_mut(a_item).text_color.a = a;
                                self.lexer.need_right()?;
                            }

                            T::T_linewidth => {
                                a_item.line_width = self.parse_double()?;
                                self.lexer.need_right()?;
                            }

                            _ => return Err(self.unexpected()),
                        }

                        token = self.lexer.next_tok()?;
                    }

                    // KIFONT::FONT::GetFont( faceName, bold, italic ): kept as the face name
                    if !face_name.is_empty() {
                        text_mut(a_item).font = Some(face_name);
                    }
                }

                T::T_justify => {
                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT && token != T::T_EOF {
                        let t = text_mut(a_item);

                        match token {
                            T::T_center => {
                                t.hjustify = GrTextHAlignT::Center;
                                t.vjustify = GrTextVAlignT::Center;
                            }
                            T::T_left => t.hjustify = GrTextHAlignT::Left,
                            T::T_right => t.hjustify = GrTextHAlignT::Right,
                            T::T_top => t.vjustify = GrTextVAlignT::Top,
                            T::T_bottom => t.vjustify = GrTextVAlignT::Bottom,
                            _ => return Err(self.unexpected()),
                        }

                        token = self.lexer.next_tok()?;
                    }
                }

                T::T_rotate => {
                    let v = self.parse_double()?;
                    text_mut(a_item).orient = v;
                    self.lexer.need_right()?;
                }

                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseCoordinate`.
    fn parse_coordinate(&mut self, a_coord: &mut PointCoord) -> Result<(), IoError> {
        a_coord.pos.x = self.parse_double()?;
        a_coord.pos.y = self.parse_double()?;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            match token {
                T::T_ltcorner => a_coord.anchor = CornerAnchor::LtCorner,
                T::T_lbcorner => a_coord.anchor = CornerAnchor::LbCorner,
                T::T_rbcorner => a_coord.anchor = CornerAnchor::RbCorner,
                T::T_rtcorner => a_coord.anchor = CornerAnchor::RtCorner,
                _ => return Err(self.unexpected()),
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseInt()`: `atoi( CurText() )` of the next token,
    /// which must be a number.
    fn parse_int(&mut self) -> Result<i32, IoError> {
        let token = self.lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(self.lexer.expecting(T::T_NUMBER));
        }

        Ok(self.lexer.parse_int())
    }

    /// Port of `DRAWING_SHEET_PARSER::parseInt( int aMin, int aMax )`: parse an integer and
    /// constrain it between two values.
    fn parse_int_range(&mut self, a_min: i32, a_max: i32) -> Result<i32, IoError> {
        let mut val = self.parse_int()?;

        if val < a_min {
            val = a_min;
        } else if val > a_max {
            val = a_max;
        }

        Ok(val)
    }

    /// Port of `DRAWING_SHEET_PARSER::parseDouble()`.
    fn parse_double(&mut self) -> Result<f64, IoError> {
        let token = self.lexer.next_tok()?;

        if token != T::T_NUMBER {
            return Err(self.lexer.expecting(T::T_NUMBER));
        }

        self.lexer.parse_double()
    }
}

/// The text members of a text item (the parser only calls this on `DS_DATA_ITEM_TEXT`).
fn text_mut(a_item: &mut DsDataItem) -> &mut super::ds_data_item::DsDataItemText {
    match &mut a_item.kind {
        DsDataItemKind::Text(t) => t,
        _ => unreachable!("parse_text is only called on text items"),
    }
}

/// `sscanf( line, "%X", &value ) == 1`: skips leading white space, reads an optional sign and
/// hexadecimal digits (an optional `0x` prefix), converted to `unsigned int`.
fn scanf_hex(s: &[u8]) -> Option<u32> {
    let mut i = 0;

    while i < s.len() && kicad_sexpr::lexer::c_numeric::is_c_space(s[i]) {
        i += 1;
    }

    let mut neg = false;

    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }

    if i + 1 < s.len() && s[i] == b'0' && (s[i + 1] == b'x' || s[i + 1] == b'X') {
        if i + 2 < s.len() && s[i + 2].is_ascii_hexdigit() {
            i += 2;
        }
    }

    let start = i;
    let mut value: u32 = 0;

    while i < s.len() && s[i].is_ascii_hexdigit() {
        let d = (s[i] as char).to_digit(16).unwrap_or(0);
        value = value.wrapping_mul(16).wrapping_add(d);
        i += 1;
    }

    if i == start {
        return None;
    }

    Some(if neg { value.wrapping_neg() } else { value })
}

/// `wxBase64Decode( const wxString& aSrc )` in the default strict mode: standard alphabet,
/// `=` padding only at the end, no white space; `None` on error.
pub fn wx_base64_decode(src: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = src.as_bytes();

    if bytes.len() % 4 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.len() / 4;

    for (n, chunk) in bytes.chunks(4).enumerate() {
        let last = n + 1 == chunks;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();

        if pad > 2 || (pad > 0 && !last) {
            return None;
        }

        let mut acc: u32 = 0;

        for &c in &chunk[..4 - pad] {
            acc = (acc << 6) | val(c)?;
        }

        acc <<= 6 * pad as u32;

        out.push((acc >> 16) as u8);

        if pad < 2 {
            out.push((acc >> 8) as u8);
        }

        if pad < 1 {
            out.push(acc as u8);
        }
    }

    Some(out)
}

/// Without an [`ImageProbe`]: accept the PNG and JPEG signatures (the formats KiCad writes);
/// the size stays unknown.
fn fallback_image_check(data: &[u8]) -> Option<super::ds_data_item::ImageInfo> {
    let png = data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let jpeg = data.starts_with(&[0xFF, 0xD8]);

    if png || jpeg {
        Some(super::ds_data_item::ImageInfo {
            width: 1,
            height: 1,
            ppi: 300,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_refs() {
        assert_eq!(convert_legacy_variable_refs("Size: %Z"), "Size: ${PAPER}");
        assert_eq!(
            convert_legacy_variable_refs("%C0 %C8 %C9"),
            "${COMMENT1} ${COMMENT9} "
        );
        assert_eq!(convert_legacy_variable_refs("100%%"), "100%");
        assert_eq!(convert_legacy_variable_refs("a%"), "a");
        assert_eq!(convert_legacy_variable_refs("%C"), "");
        assert_eq!(convert_legacy_variable_refs("%S/%N"), "${#}/${##}");
    }

    #[test]
    fn base64() {
        assert_eq!(wx_base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(wx_base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(wx_base64_decode("TQ==").unwrap(), b"M");
        assert!(wx_base64_decode("TQ=").is_none());
        assert!(wx_base64_decode("T=QA").is_none());
    }

    #[test]
    fn hex() {
        assert_eq!(scanf_hex(b"89 50"), Some(0x89));
        assert_eq!(scanf_hex(b" 4E"), Some(0x4e));
        assert_eq!(scanf_hex(b"zz"), None);
    }
}
