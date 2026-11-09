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

//! Port of `common/eda_item.cpp` and the inline bodies of `include/eda_item.h` that the board
//! model uses: the `KICAD_T` type descriptions (`EDA_ITEM_DESC` / `GetTypeDesc`), the flag
//! helpers and the custom user properties (`m_customProperties`) of `BoardItemBase`.

use std::collections::BTreeMap;

use kicad_core::KicadT;

use crate::item::{BoardItemBase, ItemFlags};

/// Port of `EDA_ITEM::GetTypeDesc` for the PCB types: the `ENUM_MAP<KICAD_T>` strings of
/// `EDA_ITEM_DESC` (English; `wxGetTranslation` is not applied). A type without an entry maps
/// to the empty string, as `ENUM_MAP::ToString` does for an unknown value.
pub fn type_desc(t: KicadT) -> &'static str {
    match t {
        KicadT::NotUsed => "<not used>",
        KicadT::PcbFootprintT => "Footprint",
        KicadT::PcbPadT => "Pad",
        KicadT::PcbShapeT => "Graphic",
        KicadT::PcbReferenceImageT => "Reference Image",
        KicadT::PcbGeneratorT => "Generator",
        KicadT::PcbFieldT => "Text",
        KicadT::PcbTextT => "Text",
        KicadT::PcbTextboxT => "Text Box",
        KicadT::PcbTableT => "Table",
        KicadT::PcbDrillChartT => "Drill Chart",
        KicadT::PcbDrillMapT => "Drill Map",
        KicadT::PcbStackupTableT => "Stackup Table",
        KicadT::PcbTablecellT => "Table Cell",
        KicadT::PcbTraceT => "Track",
        KicadT::PcbArcT => "Track",
        KicadT::PcbViaT => "Via",
        KicadT::PcbMarkerT => "Marker",
        KicadT::PcbDimAlignedT => "Dimension",
        KicadT::PcbDimOrthogonalT => "Dimension",
        KicadT::PcbDimCenterT => "Dimension",
        KicadT::PcbDimRadialT => "Dimension",
        KicadT::PcbDimLeaderT => "Leader",
        KicadT::PcbTargetT => "Target",
        KicadT::PcbPointT => "Point",
        KicadT::PcbZoneT => "Zone",
        KicadT::PcbItemListT => "ItemList",
        KicadT::PcbNetinfoT => "NetInfo",
        KicadT::PcbGroupT => "Group",
        KicadT::PcbBarcodeT => "Barcode",
        KicadT::PcbGridItemT => "Grid",
        KicadT::Pcb3dBodyT => "3D Body",
        _ => "",
    }
}

/// `wxString::CmpNoCase( .. ) == 0`: equal after lower-casing every character (one-to-one
/// lower-case mapping, as `wxTolower`).
fn eq_no_case(a: &str, b: &str) -> bool {
    fn lower(c: char) -> char {
        let mut it = c.to_lowercase();

        match (it.next(), it.next()) {
            (Some(l), None) => l,
            _ => c,
        }
    }

    a.chars().map(lower).eq(b.chars().map(lower))
}

/// The `EDA_ITEM` flag and custom-property members (data in `item.rs`).
impl BoardItemBase {
    /// `EDA_ITEM::SetFlags`.
    pub fn set_flags(&mut self, flags: u32) {
        self.flags.set(flags);
    }

    /// `EDA_ITEM::ClearFlags`.
    pub fn clear_flags(&mut self, flags: u32) {
        self.flags.clear(flags);
    }

    /// `EDA_ITEM::HasFlag`.
    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags.has(flag)
    }

    /// `EDA_ITEM::GetFlags`.
    pub fn get_flags(&self) -> u32 {
        self.flags.0
    }

    /// `EDA_ITEM::GetEditFlags`.
    pub fn edit_flags(&self) -> u32 {
        self.flags.0 & ItemFlags::EDIT_FLAGS
    }

    /// `EDA_ITEM::ClearEditFlags`.
    pub fn clear_edit_flags(&mut self) {
        self.flags.clear(ItemFlags::EDIT_FLAGS);
    }

    /// `EDA_ITEM::IsNew`.
    pub fn is_new(&self) -> bool {
        self.flags.has(ItemFlags::IS_NEW)
    }

    /// `EDA_ITEM::IsModified`.
    pub fn is_modified(&self) -> bool {
        self.flags.has(ItemFlags::IS_CHANGED)
    }

    /// `EDA_ITEM::IsMoving`.
    pub fn is_moving(&self) -> bool {
        self.flags.has(ItemFlags::IS_MOVING)
    }

    /// `EDA_ITEM::GetCustomProperties`.
    pub fn custom_properties(&self) -> &BTreeMap<String, String> {
        &self.custom_properties
    }

    /// `EDA_ITEM::SetCustomProperties`: clear, then `SetCustomProperty` for each entry in key
    /// order (so keys differing only in case collapse onto the first one).
    pub fn set_custom_properties(&mut self, props: &BTreeMap<String, String>) {
        self.custom_properties.clear();

        for (key, value) in props {
            self.set_custom_property(key, value);
        }
    }

    /// `EDA_ITEM::SetCustomProperty`: overwrite the value of an existing key that matches
    /// case-insensitively (the stored key keeps its spelling), else insert.
    pub fn set_custom_property(&mut self, key: &str, value: &str) {
        for (k, v) in self.custom_properties.iter_mut() {
            if eq_no_case(k, key) {
                *v = value.to_string();
                return;
            }
        }

        self.custom_properties
            .insert(key.to_string(), value.to_string());
    }

    /// `EDA_ITEM::HasCustomProperties`.
    pub fn has_custom_properties(&self) -> bool {
        !self.custom_properties.is_empty()
    }

    /// `EDA_ITEM::ClearCustomProperties`.
    pub fn clear_custom_properties(&mut self) {
        self.custom_properties.clear();
    }

    /// `EDA_ITEM::RemoveCustomProperty` (exact key; the descriptor cache is not ported).
    pub fn remove_custom_property(&mut self, key: &str) {
        self.custom_properties.remove(key);
    }

    /// `EDA_ITEM::GetCustomProperty`: the value of the exact key.
    pub fn custom_property(&self, key: &str) -> Option<&str> {
        self.custom_properties.get(key).map(String::as_str)
    }
}
