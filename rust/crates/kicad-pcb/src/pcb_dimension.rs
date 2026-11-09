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

//! Port of `pcbnew/pcb_dimension.h` and `pcbnew/pcb_dimension.cpp` (`PCB_DIMENSION_BASE and the five PCB_DIM_* classes`).
//!
//! Placeholder written by M0a; the type package M0c fills in the fields, `Default` and the
//! delegation shims (`board-model.md` 6.2, 16). The `DIM_*` enums below are complete already
//! because `BoardDesignSettings` (package M0d) holds four of them.

use crate::eda_text::EdaText;
use crate::item::BoardItemBase;
use crate::pcb_text::PcbText;
use crate::traits::BoardItemMut;
use crate::traits::EdaTextItem;
use crate::traits::{BoardItem, Invalidate};
use kicad_core::KicadT;

/// `DIM_UNITS_FORMAT`: how to display the units in a dimension's text; the values are written
/// to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimUnitsFormat {
    /// `1234.0`
    NoSuffix = 0,
    /// `1234.0 mm`
    BareSuffix,
    /// `1234.0 (mm)`
    ParenSuffix,
}

/// `DIM_PRECISION`; the values are written to files. The names are the digit patterns of the
/// C++ enumerators (`V_*`: 0.00 / 0 / 0.0 depending on the units).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimPrecision {
    X = 0,
    X_X,
    X_XX,
    X_XXX,
    X_XXXX,
    X_XXXXX,
    V_VV,
    V_VVV,
    V_VVVV,
    V_VVVVV,
}

/// `DIM_TEXT_POSITION`: where to place the text on a dimension; the values are written to
/// files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimTextPosition {
    /// Text appears outside the dimension line (default).
    Outside = 0,
    /// Text appears in line with the dimension line.
    Inline,
    /// Text placement is manually set by the user.
    Manual,
}

/// `DIM_UNITS_MODE`: the units selection as stored in the file (`EDA_UNITS` alone does not
/// cut it).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimUnitsMode {
    Inch = 0,
    Mils,
    Mm,
    Automatic,
}

/// `DIM_ARROW_DIRECTION`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimArrowDirection {
    /// `>-----<`
    Inward = 0,
    /// `<----->`
    Outward,
}

/// `DIM_TEXT_BORDER`: frame to show around dimension text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum DimTextBorder {
    None = 0,
    Rectangle,
    Circle,
    Roundrect,
}

/// `PCB_DIMENSION_BASE and the five PCB_DIM_* classes`.
///
/// PLACEHOLDER (M0a): M0c replaces the body of this struct with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values; the fields named here are structural (the kernel
/// relies on them) and stay.
#[derive(Clone, Debug, Default)]
pub struct PcbDimension {
    /// PCB_DIMENSION_BASE : PCB_TEXT.
    pub text: PcbText,
}

impl Invalidate for PcbDimension {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbDimension {
    fn base(&self) -> &BoardItemBase {
        &self.text.base
    }

    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.text.base
    }

    fn type_(&self) -> KicadT {
        KicadT::PcbDimAlignedT
    }

    fn class_name(&self) -> &'static str {
        "PCB_DIM_ALIGNED"
    }
}

impl BoardItemMut for PcbDimension {}

/// A dimension is its own `EDA_TEXT` host (PCB_DIMENSION_BASE : PCB_TEXT), so that
/// `ItemRef::as_eda_text()` and the generic `eda_text` functions see the dimension and not
/// the embedded `PcbText`. M0c adds a shim for every `EDA_TEXT` virtual that PCB_TEXT
/// overrides, each forwarding to an inherent `PcbDimension` method.
impl EdaTextItem for PcbDimension {
    fn eda_text(&self) -> &EdaText {
        &self.text.text
    }

    fn eda_text_mut(&mut self) -> &mut EdaText {
        &mut self.text.text
    }
}
