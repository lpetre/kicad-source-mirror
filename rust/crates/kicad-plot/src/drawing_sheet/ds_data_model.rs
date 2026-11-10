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

//! Port of `common/drawing_sheet/ds_data_model.cpp`, `include/drawing_sheet/ds_data_model.h`
//! and the `DS_DATA_MODEL` members defined in `drawing_sheet_parser.cpp` (`SetDefaultLayout`,
//! `SetEmptyLayout`, `EmptyLayoutName`, `SetPageLayout`, `LoadDrawingSheet`).
//!
//! The C++ keeps the layout in use in the singleton `DS_DATA_MODEL::GetTheInstance()`
//! (loaded by the board loader, read by the plotters and DRC). The port has no singleton:
//! the host owns a [`DsDataModel`] and passes it to the draw list. Reading files is the
//! host's job too: [`DsDataModel::load_drawing_sheet`] takes what the host found
//! ([`DrawingSheetFile`]). `ds_data_model_io.cpp` (writing sheets) has no caller in the port
//! and is not ported.

use kicad_core::page_info::PageInfo;
use kicad_geom::Vector2D;

use super::drawing_sheet_default_description::{DEFAULT_DRAWING_SHEET, EMPTY_DRAWING_SHEET};
use super::drawing_sheet_parser::DrawingSheetParser;
use super::ds_data_item::{DsDataItem, DsDrawEnv, ImageProbe};

/// `MILS_TO_MM`: `25.4 / 1000`.
const MILS_TO_MM: f64 = 25.4 / 1000.0;

/// What the host found for `DS_DATA_MODEL::LoadDrawingSheet( aFullFileName, ... )`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingSheetFile<'a> {
    /// `aFullFileName.IsEmpty()`: the default layout is used.
    NoFileName,
    /// `!wxFileExists( aFullFileName )`
    NotFound,
    /// The file exists but `wxFFile` could not open it.
    CannotOpen,
    /// The file content and its full name (the parser's source name).
    Content {
        /// The resolved file name.
        file_name: &'a str,
        /// The bytes of the file.
        data: &'a [u8],
    },
}

/// Port of `class DS_DATA_MODEL`: the list of [`DsDataItem`]s of a drawing sheet and the
/// draw environment of the page being drawn.
#[derive(Clone, Debug)]
pub struct DsDataModel {
    /// `m_WSunits2Iu`, `m_RB_Corner`, `m_LT_Corner`, `m_DefaultLineWidth`,
    /// `m_DefaultTextSize`, `m_DefaultTextThickness`, `m_EditMode`
    pub env: DsDrawEnv,
    /// `m_list`
    list: Vec<DsDataItem>,
    /// `m_allowVoidList`: if false, the default drawing sheet will be loaded the first time
    /// `DS_DRAW_ITEM_LIST::BuildDrawItemsList` is run (useful mainly for drawing sheet
    /// editor)
    allow_void_list: bool,
    /// `m_fileFormatVersionAtLoad`
    file_format_version_at_load: i32,
    /// `m_generator`: tool name written in the generator field
    generator: String,
    /// `m_leftMargin`: the left page margin in mm
    left_margin: f64,
    /// `m_rightMargin`: the right page margin in mm
    right_margin: f64,
    /// `m_topMargin`: the top page margin in mm
    top_margin: f64,
    /// `m_bottomMargin`: the bottom page margin in mm
    bottom_margin: f64,
    /// Not in C++: reads the image headers of bitmap items (`wxImage::LoadFile`), see
    /// [`ImageProbe`].
    pub image_probe: Option<ImageProbe>,
}

impl Default for DsDataModel {
    /// Port of `DS_DATA_MODEL::DS_DATA_MODEL()`.
    fn default() -> Self {
        DsDataModel {
            env: DsDrawEnv::default(),
            list: Vec::new(),
            allow_void_list: false,
            file_format_version_at_load: 0,
            generator: "pl_editor".to_string(),
            left_margin: 10.0,   // the left page margin in mm
            right_margin: 10.0,  // the right page margin in mm
            top_margin: 10.0,    // the top page margin in mm
            bottom_margin: 10.0, // the bottom page margin in mm
            image_probe: None,
        }
    }
}

impl DsDataModel {
    /// A model holding the default layout (`SetDefaultLayout` on a new model).
    pub fn with_default_layout() -> Self {
        let mut model = Self::default();
        model.set_default_layout();
        model
    }

    /// Port of `DS_DATA_MODEL::GetFileFormatVersionAtLoad`.
    pub fn file_format_version_at_load(&self) -> i32 {
        self.file_format_version_at_load
    }

    /// Port of `DS_DATA_MODEL::SetFileFormatVersionAtLoad`.
    pub fn set_file_format_version_at_load(&mut self, a_version: i32) {
        self.file_format_version_at_load = a_version;
    }

    /// Port of `DS_DATA_MODEL::GetGenerator`.
    pub fn generator(&self) -> &str {
        &self.generator
    }

    /// Port of `DS_DATA_MODEL::SetGenerator`.
    pub fn set_generator(&mut self, a_generator: &str) {
        self.generator = a_generator.to_string();
    }

    /// Port of `DS_DATA_MODEL::GetLeftMargin`.
    pub fn left_margin(&self) -> f64 {
        self.left_margin
    }

    /// Port of `DS_DATA_MODEL::SetLeftMargin`.
    pub fn set_left_margin(&mut self, a_margin: f64) {
        self.left_margin = a_margin;
    }

    /// Port of `DS_DATA_MODEL::GetRightMargin`.
    pub fn right_margin(&self) -> f64 {
        self.right_margin
    }

    /// Port of `DS_DATA_MODEL::SetRightMargin`.
    pub fn set_right_margin(&mut self, a_margin: f64) {
        self.right_margin = a_margin;
    }

    /// Port of `DS_DATA_MODEL::GetTopMargin`.
    pub fn top_margin(&self) -> f64 {
        self.top_margin
    }

    /// Port of `DS_DATA_MODEL::SetTopMargin`.
    pub fn set_top_margin(&mut self, a_margin: f64) {
        self.top_margin = a_margin;
    }

    /// Port of `DS_DATA_MODEL::GetBottomMargin`.
    pub fn bottom_margin(&self) -> f64 {
        self.bottom_margin
    }

    /// Port of `DS_DATA_MODEL::SetBottomMargin`.
    pub fn set_bottom_margin(&mut self, a_margin: f64) {
        self.bottom_margin = a_margin;
    }

    /// Port of `DS_DATA_MODEL::SetupDrawEnvironment( const PAGE_INFO& aPageInfo, double
    /// aMilsToIU )`.
    pub fn setup_draw_environment(&mut self, a_page_info: &PageInfo, a_mils_to_iu: f64) {
        self.env.ws_units2_iu = a_mils_to_iu / MILS_TO_MM;

        // Left top corner position
        let lt_corner = Vector2D::new(self.left_margin(), self.top_margin());
        self.env.lt_corner = lt_corner;

        // Right bottom corner position
        let (size_x, size_y) = a_page_info.size_mils();
        let rb_corner = Vector2D::new(
            (size_x * MILS_TO_MM) - self.right_margin(),
            (size_y * MILS_TO_MM) - self.bottom_margin(),
        );
        self.env.rb_corner = rb_corner;
    }

    /// Port of `DS_DATA_MODEL::AllowVoidList`.
    pub fn allow_void_list(&mut self, allow: bool) {
        self.allow_void_list = allow;
    }

    /// Port of `DS_DATA_MODEL::VoidListAllowed`.
    pub fn void_list_allowed(&self) -> bool {
        self.allow_void_list
    }

    /// Port of `DS_DATA_MODEL::ClearList`.
    pub fn clear_list(&mut self) {
        self.list.clear();
    }

    /// Port of `DS_DATA_MODEL::Append`.
    pub fn append(&mut self, a_item: DsDataItem) {
        self.list.push(a_item);
    }

    /// Port of `DS_DATA_MODEL::Remove` (by index).
    pub fn remove(&mut self, a_idx: usize) {
        if a_idx < self.list.len() {
            self.list.remove(a_idx);
        }
    }

    /// Port of `DS_DATA_MODEL::GetItem`.
    pub fn item(&self, a_idx: u32) -> Option<&DsDataItem> {
        self.list.get(a_idx as usize)
    }

    /// Port of `DS_DATA_MODEL::GetItems`.
    pub fn items(&self) -> &[DsDataItem] {
        &self.list
    }

    /// Port of `DS_DATA_MODEL::GetItems` (mutable).
    pub fn items_mut(&mut self) -> &mut Vec<DsDataItem> {
        &mut self.list
    }

    /// The draw environment and the item list, borrowed separately (the C++ items read the
    /// environment from the singleton while the list is walked).
    pub fn env_and_items_mut(&mut self) -> (&DsDrawEnv, &mut Vec<DsDataItem>) {
        (&self.env, &mut self.list)
    }

    /// Port of `DS_DATA_MODEL::GetCount`.
    pub fn count(&self) -> u32 {
        self.list.len() as u32
    }

    /// Port of `DS_DATA_MODEL::SetDefaultLayout`: fill the list with the default drawing
    /// sheet.
    pub fn set_default_layout(&mut self) {
        self.set_page_layout(DEFAULT_DRAWING_SHEET, false, "default page");
    }

    /// Port of `DS_DATA_MODEL::DefaultLayout`: the S-expression text of the default sheet.
    pub fn default_layout() -> &'static str {
        DEFAULT_DRAWING_SHEET
    }

    /// Port of `DS_DATA_MODEL::SetEmptyLayout`: fill the list with an empty drawing sheet.
    pub fn set_empty_layout(&mut self) {
        self.set_page_layout(EMPTY_DRAWING_SHEET, false, "empty page");
    }

    /// Port of `DS_DATA_MODEL::EmptyLayout`: the S-expression text of the empty sheet.
    pub fn empty_layout() -> &'static str {
        EMPTY_DRAWING_SHEET
    }

    /// Port of `DS_DATA_MODEL::EmptyLayoutName`: the sheet name `LoadFromName` maps to the
    /// empty layout.
    pub fn empty_layout_name() -> &'static str {
        "empty.kicad_wks"
    }

    /// Port of `DS_DATA_MODEL::SetPageLayout( const char* aPageLayout, bool aAppend, const
    /// wxString& aSource )`: populate the list from a S-expression description. Errors are
    /// ignored ("best efforts"); the items read before the error stay. As in C++ `a_source`
    /// is not used (the parser source is `Sexpr_string`).
    pub fn set_page_layout(&mut self, a_page_layout: &str, append: bool, a_source: &str) {
        let _ = a_source;

        if !append {
            self.clear_list();
        }

        let mut parser = DrawingSheetParser::new(
            nul_terminated(a_page_layout),
            "Sexpr_string",
            self.image_probe,
        );

        // best efforts
        let _ = parser.parse(self);
    }

    /// Port of `DS_DATA_MODEL::LoadDrawingSheet( const wxString& aFullFileName, wxString*
    /// aMsg, bool aAppend )`: populate the list from a `.kicad_wks` file the host has read.
    ///
    /// Returns `Err( aMsg )` where the C++ returns false (the texts of the C++ messages,
    /// untranslated).
    pub fn load_drawing_sheet(
        &mut self,
        a_file: DrawingSheetFile<'_>,
        a_append: bool,
    ) -> Result<(), String> {
        if !a_append {
            match a_file {
                DrawingSheetFile::NoFileName => {
                    self.set_default_layout();
                    return Ok(()); // we assume its fine / default init
                }
                DrawingSheetFile::NotFound => {
                    self.set_default_layout();
                    return Err("File not found.".to_string());
                }
                _ => {}
            }
        }

        let (file_name, data) = match a_file {
            DrawingSheetFile::Content { file_name, data } => (file_name, data),
            // wxFFile( empty or missing name ) fails to open
            _ => {
                if !a_append {
                    self.set_default_layout();
                }

                return Err("File could not be opened.".to_string());
            }
        };

        // buffer[filelen] = 0; the parser reads the buffer as a C string
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        let text = String::from_utf8_lossy(&data[..end]).into_owned();

        if !a_append {
            self.clear_list();
        }

        let mut parser = DrawingSheetParser::new(&text, file_name, self.image_probe);

        parser.parse(self).map_err(|ioe| ioe.what().to_string())
    }
}

/// The text up to the first NUL (a `const char*` argument).
fn nul_terminated(s: &str) -> &str {
    match s.find('\0') {
        Some(end) => &s[..end],
        None => s,
    }
}
