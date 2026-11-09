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

//! Port of `include/page_info.h` and `common/page_info.cpp`: the paper size table and
//! `PAGE_INFO` (page type, size in mils, orientation).
//!
//! The C++ keeps the size of the next "User" page in the static members `s_user_width` /
//! `s_user_height`.  Global mutable state is replaced by an explicit [`CustomPageSize`] value
//! passed to the functions that read it; [`CustomPageSize::default`] holds the C++ initial
//! values (17000 x 11000 mils).  `wxPaperSize` is kept as [`WxPaperSize`] (only the ids the
//! table uses).

use kicad_sexpr::{IoError, OutputFormatter};

use crate::eda_units::mm2mils;
use crate::string_utils::format_double2_str;

/// Min page size for clamping, in mils.
pub const MIN_PAGE_SIZE_MILS: i32 = 1000;
/// Max page size for clamping in Pcbnew, in mils.
pub const MAX_PAGE_SIZE_PCBNEW_MILS: i32 = 48000;
/// Max page size for clamping in Eeschema, in mils.
pub const MAX_PAGE_SIZE_EESCHEMA_MILS: i32 = 120000;
/// Min page size for clamping, in mm.
pub const MIN_PAGE_SIZE_MM: f64 = 25.4;
/// Max page size for clamping in Pcbnew, in mm.
pub const MAX_PAGE_SIZE_PCBNEW_MM: f64 = 48000.0 * 0.0254;
/// Max page size for clamping in Eeschema, in mm.
pub const MAX_PAGE_SIZE_EESCHEMA_MM: f64 = 120000.0 * 0.0254;

/// Port of `enum class PAGE_SIZE_TYPE`.  Do not rename: the names are saved to file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(missing_docs)]
pub enum PageSizeType {
    A5,
    A4,
    A3,
    A2,
    A1,
    A0,
    A,
    B,
    C,
    D,
    E,
    GERBER,
    USLetter,
    USLegal,
    USLedger,
    User,
}

impl PageSizeType {
    /// All types, in enum order.
    pub const ALL: [PageSizeType; 16] = [
        PageSizeType::A5,
        PageSizeType::A4,
        PageSizeType::A3,
        PageSizeType::A2,
        PageSizeType::A1,
        PageSizeType::A0,
        PageSizeType::A,
        PageSizeType::B,
        PageSizeType::C,
        PageSizeType::D,
        PageSizeType::E,
        PageSizeType::GERBER,
        PageSizeType::USLetter,
        PageSizeType::USLegal,
        PageSizeType::USLedger,
        PageSizeType::User,
    ];

    /// `magic_enum::enum_name`: the enumerator name, as written to files.
    pub fn name(self) -> &'static str {
        match self {
            PageSizeType::A5 => "A5",
            PageSizeType::A4 => "A4",
            PageSizeType::A3 => "A3",
            PageSizeType::A2 => "A2",
            PageSizeType::A1 => "A1",
            PageSizeType::A0 => "A0",
            PageSizeType::A => "A",
            PageSizeType::B => "B",
            PageSizeType::C => "C",
            PageSizeType::D => "D",
            PageSizeType::E => "E",
            PageSizeType::GERBER => "GERBER",
            PageSizeType::USLetter => "USLetter",
            PageSizeType::USLegal => "USLegal",
            PageSizeType::USLedger => "USLedger",
            PageSizeType::User => "User",
        }
    }

    /// `magic_enum::enum_cast<PAGE_SIZE_TYPE>( name, magic_enum::case_insensitive )`
    /// (ASCII case folding).
    pub fn from_name(name: &str) -> Option<PageSizeType> {
        Self::ALL
            .iter()
            .copied()
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }
}

/// The `wxPaperSize` ids used by the page table, with their wxWidgets 3.2 values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
#[allow(missing_docs)]
pub enum WxPaperSize {
    None = 0,
    A5 = 14,
    A4 = 3,
    A3 = 12,
    A2 = 64,
    A1 = 118,
    A0 = 117,
    Letter = 1,
    Tabloid = 8,
    CSheet = 4,
    DSheet = 5,
    ESheet = 6,
    Legal = 2,
}

/// The size of the next "User" page (`PAGE_INFO::s_user_width` / `s_user_height`), in mils.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomPageSize {
    /// `s_user_width`
    pub width_mils: f64,
    /// `s_user_height`
    pub height_mils: f64,
}

impl Default for CustomPageSize {
    fn default() -> Self {
        CustomPageSize {
            width_mils: 17000.0,
            height_mils: 11000.0,
        }
    }
}

impl CustomPageSize {
    /// Port of `PAGE_INFO::SetCustomWidthMils`.
    pub fn set_width_mils(&mut self, width_in_mils: f64) {
        self.width_mils = clamp_width(width_in_mils);
    }

    /// Port of `PAGE_INFO::SetCustomHeightMils`.
    pub fn set_height_mils(&mut self, height_in_mils: f64) {
        self.height_mils = clamp_height(height_in_mils);
    }
}

fn clamp_width(width_in_mils: f64) -> f64 {
    if width_in_mils < 10.0 {
        10.0
    } else {
        width_in_mils
    }
}

fn clamp_height(height_in_mils: f64) -> f64 {
    if height_in_mils < 10.0 {
        10.0
    } else {
        height_in_mils
    }
}

/// Port of `class PAGE_INFO`: a paper type and size (in mils) with its orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    /// paper type: A4, A3, etc.
    type_: PageSizeType,
    /// mils (x, y)
    size: (f64, f64),
    /// true if portrait, false if landscape
    portrait: bool,
    /// wx' style paper id.
    paper_id: WxPaperSize,
    /// more human friendly description of page size
    description: &'static str,
}

/// `MMsize( x, y )`
fn mm_size(x: f64, y: f64) -> (f64, f64) {
    (mm2mils(x) as f64, mm2mils(y) as f64)
}

/// Port of `PAGE_INFO::standardPageSizes`.  All MUST be defined as landscape.
fn standard_page_sizes() -> [PageInfo; 16] {
    use PageSizeType as T;
    use WxPaperSize as P;

    [
        PageInfo::raw(mm_size(210.0, 148.0), T::A5, P::A5, "A5 148 x 210mm"),
        PageInfo::raw(mm_size(297.0, 210.0), T::A4, P::A4, "A4 210 x 297mm"),
        PageInfo::raw(mm_size(420.0, 297.0), T::A3, P::A3, "A3 297 x 420mm"),
        PageInfo::raw(mm_size(594.0, 420.0), T::A2, P::A2, "A2 420 x 594mm"),
        PageInfo::raw(mm_size(841.0, 594.0), T::A1, P::A1, "A1 594 x 841mm"),
        PageInfo::raw(mm_size(1189.0, 841.0), T::A0, P::A0, "A0 841 x 1189mm"),
        PageInfo::raw((11000.0, 8500.0), T::A, P::Letter, "A 8.5 x 11in"),
        PageInfo::raw((17000.0, 11000.0), T::B, P::Tabloid, "B 11 x 17in"),
        PageInfo::raw((22000.0, 17000.0), T::C, P::CSheet, "C 17 x 22in"),
        PageInfo::raw((34000.0, 22000.0), T::D, P::DSheet, "D 22 x 34in"),
        PageInfo::raw((44000.0, 34000.0), T::E, P::ESheet, "E 34 x 44in"),
        // US paper sizes
        PageInfo::raw((32000.0, 32000.0), T::GERBER, P::None, ""),
        PageInfo::raw((17000.0, 11000.0), T::User, P::None, "User (Custom)"),
        PageInfo::raw(
            (11000.0, 8500.0),
            T::USLetter,
            P::Letter,
            "US Letter 8.5 x 11in",
        ),
        PageInfo::raw(
            (14000.0, 8500.0),
            T::USLegal,
            P::Legal,
            "US Legal 8.5 x 14in",
        ),
        PageInfo::raw(
            (17000.0, 11000.0),
            T::USLedger,
            P::Tabloid,
            "US Ledger 11 x 17in",
        ),
    ]
}

impl Default for PageInfo {
    /// `PAGE_INFO()`: A3 landscape.
    fn default() -> Self {
        PageInfo::new(PageSizeType::A3, false, &CustomPageSize::default())
    }
}

impl PageInfo {
    /// The protected constructor `PAGE_INFO( const VECTOR2D&, const PAGE_SIZE_TYPE&,
    /// wxPaperSize, const wxString& )`.
    fn raw(
        size_mils: (f64, f64),
        type_: PageSizeType,
        paper_id: WxPaperSize,
        description: &'static str,
    ) -> Self {
        let mut p = PageInfo {
            type_,
            size: size_mils,
            portrait: false,
            paper_id,
            description,
        };
        p.update_portrait();
        p
    }

    /// Port of `PAGE_INFO( PAGE_SIZE_TYPE aType, bool IsPortrait )`; `custom` is the size used
    /// for `User` pages.
    pub fn new(type_: PageSizeType, is_portrait: bool, custom: &CustomPageSize) -> Self {
        let mut p = PageInfo {
            type_: PageSizeType::A4,
            size: (custom.width_mils, custom.height_mils),
            portrait: false,
            paper_id: WxPaperSize::None,
            description: "",
        };

        p.set_type(type_, is_portrait, custom);
        p
    }

    /// Port of `PAGE_INFO::GetPageFormatsList`.
    pub fn get_page_formats_list() -> Vec<PageInfo> {
        standard_page_sizes().to_vec()
    }

    fn update_portrait(&mut self) {
        // update m_portrait based on orientation of m_size.x and m_size.y
        self.portrait = self.size.1 > self.size.0;
    }

    /// Port of `PAGE_INFO::SetType( PAGE_SIZE_TYPE, bool )`.
    pub fn set_type(
        &mut self,
        type_: PageSizeType,
        is_portrait: bool,
        custom: &CustomPageSize,
    ) -> bool {
        let mut rc = true;

        match standard_page_sizes().into_iter().find(|p| p.type_ == type_) {
            Some(found) => *self = found,
            None => rc = false,
        }

        if type_ == PageSizeType::User {
            self.type_ = PageSizeType::User;
            self.paper_id = WxPaperSize::None;
            self.size = (custom.width_mils, custom.height_mils);

            self.update_portrait();
        }

        if is_portrait {
            // all private PAGE_INFOs are landscape, must swap x and y
            self.size = (self.size.1, self.size.0);
            self.update_portrait();
        }

        rc
    }

    /// Port of `PAGE_INFO::SetType( const wxString&, bool )`: the type by (case-insensitive)
    /// name; false if unknown.
    pub fn set_type_by_name(
        &mut self,
        page_size: &str,
        is_portrait: bool,
        custom: &CustomPageSize,
    ) -> bool {
        match PageSizeType::from_name(page_size) {
            Some(t) => self.set_type(t, is_portrait, custom),
            None => false,
        }
    }

    /// `GetType`
    pub fn type_(&self) -> PageSizeType {
        self.type_
    }

    /// Port of `GetTypeAsString`.
    pub fn type_as_string(&self) -> &'static str {
        self.type_.name()
    }

    /// `GetPageFormatDescription` (untranslated)
    pub fn page_format_description(&self) -> &'static str {
        self.description
    }

    /// `IsDefault`: A3 landscape.
    pub fn is_default(&self) -> bool {
        self.type_ == PageSizeType::A3 && !self.portrait
    }

    /// Port of `IsCustom`.
    pub fn is_custom(&self) -> bool {
        self.type_ == PageSizeType::User
    }

    /// Port of `SetPortrait`.
    pub fn set_portrait(&mut self, is_portrait: bool) {
        if self.portrait != is_portrait {
            // swap x and y in m_size
            self.size = (self.size.1, self.size.0);

            self.portrait = is_portrait;

            // margins are not touched, do that if you want
        }
    }

    /// `IsPortrait`
    pub fn is_portrait(&self) -> bool {
        self.portrait
    }

    /// `GetPaperId`
    pub fn paper_id(&self) -> WxPaperSize {
        self.paper_id
    }

    /// `SetWidthMM`
    pub fn set_width_mm(&mut self, width_in_mm: f64) {
        self.set_width_mils(width_in_mm * 1000.0 / 25.4);
    }

    /// Port of `SetWidthMils`: also makes the page a `User` page.
    pub fn set_width_mils(&mut self, width_in_mils: f64) {
        if self.size.0 != width_in_mils {
            self.size.0 = clamp_width(width_in_mils);

            self.type_ = PageSizeType::User;
            self.paper_id = WxPaperSize::None;

            self.update_portrait();
        }
    }

    /// `GetWidthMils`
    pub fn width_mils(&self) -> f64 {
        self.size.0
    }

    /// `GetWidthMM`
    pub fn width_mm(&self) -> f64 {
        self.size.0 * 25.4 / 1000.0
    }

    /// `SetHeightMM`
    pub fn set_height_mm(&mut self, height_in_mm: f64) {
        self.set_height_mils(height_in_mm * 1000.0 / 25.4);
    }

    /// Port of `SetHeightMils`: also makes the page a `User` page.
    pub fn set_height_mils(&mut self, height_in_mils: f64) {
        if self.size.1 != height_in_mils {
            self.size.1 = clamp_height(height_in_mils);

            self.type_ = PageSizeType::User;
            self.paper_id = WxPaperSize::None;

            self.update_portrait();
        }
    }

    /// `GetHeightMils`
    pub fn height_mils(&self) -> f64 {
        self.size.1
    }

    /// `GetHeightMM`
    pub fn height_mm(&self) -> f64 {
        self.size.1 * 25.4 / 1000.0
    }

    /// `GetSizeMils` (x, y)
    pub fn size_mils(&self) -> (f64, f64) {
        self.size
    }

    /// `GetWidthIU`: `aIUScale * GetWidthMils()` truncated to int.
    pub fn width_iu(&self, iu_scale: f64) -> i32 {
        (iu_scale * self.width_mils()) as i32
    }

    /// `GetHeightIU`
    pub fn height_iu(&self, iu_scale: f64) -> i32 {
        (iu_scale * self.height_mils()) as i32
    }

    /// `GetSizeIU`
    pub fn size_iu(&self, iu_scale: f64) -> (f64, f64) {
        (
            self.width_iu(iu_scale) as f64,
            self.height_iu(iu_scale) as f64,
        )
    }

    /// Port of `PAGE_INFO::Format`: `(paper "A4")`, `(paper "User" W H)` (mm) or
    /// `(paper "A4" portrait)`.
    pub fn format(&self, formatter: &mut dyn OutputFormatter) -> Result<(), IoError> {
        let quoted = formatter.quotew(self.type_.name());
        formatter.print(format_args!("(paper {quoted}"))?;

        // The page dimensions are only required for user defined page sizes.
        // Internally, the page size is in mils
        if self.type_ == PageSizeType::User {
            formatter.print(format_args!(
                " {} {}",
                format_double2_str(self.width_mils() * 25.4 / 1000.0),
                format_double2_str(self.height_mils() * 25.4 / 1000.0)
            ))?;
        }

        if !self.is_custom() && self.is_portrait() {
            formatter.print(format_args!(" portrait"))?;
        }

        formatter.print(format_args!(")"))?;
        Ok(())
    }
}
