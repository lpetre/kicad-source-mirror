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

//! The drawing sheet (frame references and title block) of plots and DRC: port of
//! `common/drawing_sheet/` (`ds_data_model.cpp`, `ds_data_item.cpp`, `ds_draw_item.cpp`,
//! `drawing_sheet_parser.cpp`, `drawing_sheet_default_description.cpp`, the text expansion
//! of `ds_painter.cpp`) and of `STRING_INCREMENTER` (`common/increment.cpp`).
//!
//! Use: load a [`DsDataModel`] (default sheet, empty sheet or a `.kicad_wks` text), build a
//! [`DsDrawItemList`] for a page and title block, then plot it
//! ([`crate::common_plot_functions::plot_drawing_sheet`]) or read its texts (DRC).

pub mod drawing_sheet_default_description;
pub mod drawing_sheet_parser;
pub mod ds_data_item;
pub mod ds_data_model;
pub mod ds_draw_item;
pub mod increment;

pub use ds_data_item::{
    CornerAnchor, DsDataItem, DsDataItemKind, DsDrawEnv, DsItemType, ImageInfo, ImageProbe,
    PageOption,
};
pub use ds_data_model::{DrawingSheetFile, DsDataModel};
pub use ds_draw_item::{DsDrawItem, DsDrawItemList, DsDrawItemShape, DsDrawItemText};

use kicad_core::page_info::PageInfo;
use kicad_core::text_vars::{ResolutionContext, TextVarResolver};
use kicad_core::title_block::{TitleBlock, TitleBlockClock};
use kicad_geom::Vector2I;

/// Not in C++: the drawing sheet a host plots and checks (the singleton
/// `DS_DATA_MODEL::GetTheInstance()` that `BOARD_LOADER` loads from the project's drawing
/// sheet file) and the `GetBaseVersion()` text of `${KICAD_VERSION}`.
///
/// The default holds an empty model, which builds the default drawing sheet on first use
/// (`DS_DRAW_ITEM_LIST::BuildDrawItemsList`), as the C++ singleton does when no sheet was
/// loaded.
#[derive(Clone, Debug, Default)]
pub struct DrawingSheetEnv {
    /// The drawing sheet.
    pub model: DsDataModel,
    /// `GetBaseVersion()` when a `PGM_BASE` exists (kicad-cli), `None` otherwise.
    pub base_version: Option<String>,
}

impl PartialEq for DrawingSheetEnv {
    fn eq(&self, other: &Self) -> bool {
        self.model.items() == other.model.items()
            && self.model.void_list_allowed() == other.model.void_list_allowed()
            && self.model.env == other.model.env
            && self.base_version == other.base_version
    }
}

impl Eq for DrawingSheetEnv {}

/// One text of the drawing sheet as `DRC_TEST_PROVIDER_MISC::testTextVars` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrcDrawingSheetText {
    /// `DS_DRAW_ITEM_TEXT::GetText()`
    pub text: String,
    /// `DS_DRAW_ITEM_TEXT::GetShownText( FOR_ERC_DRC )`
    pub shown_text: String,
    /// `DS_DRAW_ITEM_TEXT::GetPosition()`
    pub position: Vector2I,
}

/// The draw item list of `DRC_TEST_PROVIDER_MISC::testTextVars` (`drc_test_provider_misc.cpp`):
/// `DS_DRAW_ITEM_LIST( pcbIUScale, FOR_ERC_DRC )` with page "1" of 1, file name
/// "dummyFilename", sheet name "dummySheet", layer "dummyLayer" and the board's project, built
/// for the page and title block of the engine's drawing sheet (`DS_PROXY_VIEW_ITEM`: the
/// board's page settings and title block). Returns its text items in list order.
pub fn drc_drawing_sheet_texts(
    sheet: &mut DrawingSheetEnv,
    a_page_info: &PageInfo,
    a_title_block: &TitleBlock,
    a_project: Option<&TextVarResolver<'_>>,
    a_clock: &TitleBlockClock,
) -> Vec<DrcDrawingSheetText> {
    let mut draw_items = DsDrawItemList::new_pcb(ResolutionContext::ForErcDrc);

    draw_items.set_page_number("1");
    draw_items.set_sheet_count(1);
    draw_items.set_file_name("dummyFilename");
    draw_items.set_sheet_name("dummySheet");
    draw_items.set_sheet_layer("dummyLayer");
    draw_items.set_project(a_project);
    draw_items.set_kicad_version(sheet.base_version.as_deref());
    draw_items.set_clock(a_clock);
    draw_items.build_draw_items_list(&mut sheet.model, a_page_info, a_title_block);

    draw_items
        .items()
        .iter()
        .filter_map(|item| match &item.shape {
            DsDrawItemShape::Text(text) => Some(DrcDrawingSheetText {
                text: text.text.clone(),
                shown_text: text.get_shown_text(ResolutionContext::ForErcDrc),
                position: text.pos,
            }),
            _ => None,
        })
        .collect()
}
