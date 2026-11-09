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

//! Port of `pcbnew/board_stackup_manager/board_stackup.h` and `board_stackup.cpp`
//! (`BOARD_STACKUP`, `BOARD_STACKUP_ITEM`, `DIELECTRIC_PRMS`): data, constructor defaults and
//! accessors (package M0d). The stack-up algorithms are package M6.

use kicad_core::{PCB_IU_SCALE, PcbLayerId};

use crate::board_design_settings::BoardDesignSettings;

/// `KEY_COPPER` (`stackup_predefined_prms.h`).
pub const KEY_COPPER: &str = "copper";
/// `KEY_CORE`.
pub const KEY_CORE: &str = "core";
/// `KEY_PREPREG`.
pub const KEY_PREPREG: &str = "prepreg";
/// `NotSpecifiedPrm()` (untranslated: the keyword written to files).
pub const NOT_SPECIFIED_PRM: &str = "Not specified";
/// `DEFAULT_EPSILON_R_SOLDERMASK`.
pub const DEFAULT_EPSILON_R_SOLDERMASK: f64 = 3.3;
/// `DEFAULT_EPSILON_R_SILKSCREEN`.
pub const DEFAULT_EPSILON_R_SILKSCREEN: f64 = 1.0;

/// `BOARD_STACKUP_ITEM_TYPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum BoardStackupItemType {
    #[default]
    Undefined = 0, // BS_ITEM_TYPE_UNDEFINED
    Copper,      // BS_ITEM_TYPE_COPPER
    Dielectric,  // BS_ITEM_TYPE_DIELECTRIC
    Solderpaste, // BS_ITEM_TYPE_SOLDERPASTE
    Soldermask,  // BS_ITEM_TYPE_SOLDERMASK
    Silkscreen,  // BS_ITEM_TYPE_SILKSCREEN
}

/// `BS_EDGE_CONNECTOR_CONSTRAINTS`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum BsEdgeConnectorConstraints {
    #[default]
    None = 0, // BS_EDGE_CONNECTOR_NONE
    InUse,    // BS_EDGE_CONNECTOR_IN_USE
    Bevelled, // BS_EDGE_CONNECTOR_BEVELLED
}

/// `DIELECTRIC_MODEL` (`common/transline_calculations/transline_calculation_base.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum DielectricModel {
    #[default]
    Constant = 0,     // CONSTANT
    DjordjevicSarkar, // DJORDJEVIC_SARKAR
}

/// `DIELECTRIC_PRMS`.
#[derive(Clone, Debug, PartialEq)]
pub struct DielectricPrms {
    pub material: String,                  // m_Material
    pub thickness: i32,                    // m_Thickness
    pub thickness_locked: bool,            // m_ThicknessLocked
    pub epsilon_r: f64,                    // m_EpsilonR
    pub loss_tangent: f64,                 // m_LossTangent
    pub spec_freq: f64,                    // m_SpecFreq
    pub dielectric_model: DielectricModel, // m_DielectricModel
    pub color: String,                     // m_Color
}

impl Default for DielectricPrms {
    /// `DIELECTRIC_PRMS::DIELECTRIC_PRMS()`.
    fn default() -> Self {
        DielectricPrms {
            material: String::new(),
            thickness: 0,
            thickness_locked: false,
            epsilon_r: 1.0,
            loss_tangent: 0.0,
            spec_freq: 0.0,
            dielectric_model: DielectricModel::Constant,
            color: String::new(),
        }
    }
}

/// `BOARD_STACKUP_ITEM`.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackupItem {
    pub type_: BoardStackupItemType,             // m_Type
    pub layer_name: String,                      // m_LayerName
    pub type_name: String,                       // m_TypeName
    pub layer_id: PcbLayerId,                    // m_LayerId
    pub dielectric_layer_id: i32,                // m_DielectricLayerId
    pub dielectric_prms_list: Vec<DielectricPrms>, // m_DielectricPrmsList
    pub enabled: bool,                           // m_enabled
}

impl Default for BoardStackupItem {
    /// `BOARD_STACKUP_ITEM( BS_ITEM_TYPE_UNDEFINED )` (C++ has no default constructor).
    fn default() -> Self {
        BoardStackupItem::new(BoardStackupItemType::Undefined)
    }
}

macro_rules! sublayer_accessors {
    ($($get:ident, $set:ident, $field:ident: $ty:ty, $cxx_get:literal, $cxx_set:literal;)*) => {
        $(
            #[doc = concat!("`BOARD_STACKUP_ITEM::", $cxx_get, "( aDielectricSubLayer )` (the default value for an index C++ asserts on).")]
            pub fn $get(&self, sublayer: usize) -> $ty {
                self.dielectric_prms_list
                    .get(sublayer)
                    .map_or_else(|| DielectricPrms::default().$field, |p| p.$field.clone())
            }

            #[doc = concat!("`BOARD_STACKUP_ITEM::", $cxx_set, "( value, aDielectricSubLayer )`: ignored for an index out of range.")]
            pub fn $set(&mut self, value: $ty, sublayer: usize) {
                if let Some(p) = self.dielectric_prms_list.get_mut(sublayer) {
                    p.$field = value;
                }
            }
        )*
    };
}

impl BoardStackupItem {
    /// `BOARD_STACKUP_ITEM::BOARD_STACKUP_ITEM( aType )` (`board_stackup.cpp:54`): one
    /// default `DIELECTRIC_PRMS`, `UNDEFINED_LAYER`, dielectric id 1, enabled, and the usual
    /// values of the type.
    pub fn new(type_: BoardStackupItemType) -> Self {
        let mut item = BoardStackupItem {
            type_,
            layer_name: String::new(),
            type_name: String::new(),
            layer_id: PcbLayerId::UndefinedLayer,
            dielectric_layer_id: 1,
            dielectric_prms_list: vec![DielectricPrms::default()],
            enabled: true,
        };

        match type_ {
            BoardStackupItemType::Copper => {
                item.type_name = KEY_COPPER.to_string();
                item.set_thickness(Self::copper_default_thickness(), 0);
            }
            BoardStackupItemType::Dielectric => {
                item.type_name = KEY_CORE.to_string(); // or prepreg
                item.set_color(NOT_SPECIFIED_PRM.to_string(), 0);
                item.set_material("FR4".to_string(), 0); // or other dielectric name
                item.set_loss_tangent(0.02, 0); // for FR4
                item.set_epsilon_r(4.5, 0); // for FR4
                item.set_spec_freq(1e9, 0); // for FR4
                item.set_dielectric_model(DielectricModel::Constant, 0);
            }
            BoardStackupItemType::Solderpaste => {
                item.type_name = "solderpaste".to_string();
            }
            BoardStackupItemType::Soldermask => {
                item.type_name = "soldermask".to_string();
                item.set_color(NOT_SPECIFIED_PRM.to_string(), 0);
                item.set_material(NOT_SPECIFIED_PRM.to_string(), 0);
                item.set_thickness(Self::mask_default_thickness(), 0);
                item.set_epsilon_r(DEFAULT_EPSILON_R_SOLDERMASK, 0);
            }
            BoardStackupItemType::Silkscreen => {
                item.type_name = "silkscreen".to_string();
                item.set_color(NOT_SPECIFIED_PRM.to_string(), 0);
                item.set_material(NOT_SPECIFIED_PRM.to_string(), 0);
                item.set_epsilon_r(DEFAULT_EPSILON_R_SILKSCREEN, 0);
            }
            BoardStackupItemType::Undefined => {}
        }

        item
    }

    /// `BOARD_STACKUP_ITEM::GetCopperDefaultThickness`: 0.035 mm.
    pub fn copper_default_thickness() -> i32 {
        PCB_IU_SCALE.mm_to_iu(0.035)
    }

    /// `BOARD_STACKUP_ITEM::GetMaskDefaultThickness`: 0.01 mm.
    pub fn mask_default_thickness() -> i32 {
        PCB_IU_SCALE.mm_to_iu(0.01)
    }

    /// `BOARD_STACKUP_ITEM::GetSublayersCount`.
    pub fn sublayers_count(&self) -> usize {
        self.dielectric_prms_list.len()
    }

    sublayer_accessors! {
        material, set_material, material: String, "GetMaterial", "SetMaterial";
        thickness, set_thickness, thickness: i32, "GetThickness", "SetThickness";
        is_thickness_locked, set_thickness_locked, thickness_locked: bool, "IsThicknessLocked", "SetThicknessLocked";
        epsilon_r, set_epsilon_r, epsilon_r: f64, "GetEpsilonR", "SetEpsilonR";
        loss_tangent, set_loss_tangent, loss_tangent: f64, "GetLossTangent", "SetLossTangent";
        spec_freq, set_spec_freq, spec_freq: f64, "GetSpecFreq", "SetSpecFreq";
        dielectric_model, set_dielectric_model, dielectric_model: DielectricModel, "GetDielectricModel", "SetDielectricModel";
        color, set_color, color: String, "GetColor", "SetColor";
    }

    /// `BOARD_STACKUP_ITEM::AddDielectricPrms`.
    pub fn add_dielectric_prms(&mut self, index: usize) {
        crate::port_pending!("BOARD_STACKUP_ITEM::AddDielectricPrms", index)
    }

    /// `BOARD_STACKUP_ITEM::RemoveDielectricPrms`.
    pub fn remove_dielectric_prms(&mut self, index: usize) {
        crate::port_pending!("BOARD_STACKUP_ITEM::RemoveDielectricPrms", index)
    }

    /// `BOARD_STACKUP_ITEM::HasEpsilonRValue`.
    pub fn has_epsilon_r_value(&self) -> bool {
        crate::port_pending!("BOARD_STACKUP_ITEM::HasEpsilonRValue")
    }

    /// `BOARD_STACKUP_ITEM::HasLossTangentValue`.
    pub fn has_loss_tangent_value(&self) -> bool {
        crate::port_pending!("BOARD_STACKUP_ITEM::HasLossTangentValue")
    }

    /// `BOARD_STACKUP_ITEM::HasMaterialValue`.
    pub fn has_material_value(&self, sublayer: usize) -> bool {
        crate::port_pending!("BOARD_STACKUP_ITEM::HasMaterialValue", sublayer)
    }

    /// `BOARD_STACKUP_ITEM::IsThicknessEditable`.
    pub fn is_thickness_editable(&self) -> bool {
        crate::port_pending!("BOARD_STACKUP_ITEM::IsThicknessEditable")
    }

    /// `BOARD_STACKUP_ITEM::FormatEpsilonR`.
    pub fn format_epsilon_r(&self, sublayer: usize) -> String {
        crate::port_pending!("BOARD_STACKUP_ITEM::FormatEpsilonR", sublayer)
    }

    /// `BOARD_STACKUP_ITEM::FormatLossTangent`.
    pub fn format_loss_tangent(&self, sublayer: usize) -> String {
        crate::port_pending!("BOARD_STACKUP_ITEM::FormatLossTangent", sublayer)
    }

    /// `BOARD_STACKUP_ITEM::FormatDielectricLayerName`.
    pub fn format_dielectric_layer_name(&self) -> String {
        crate::port_pending!("BOARD_STACKUP_ITEM::FormatDielectricLayerName")
    }
}

/// `BOARD_STACKUP`. `m_list` owns its items (C++ deep-copies them in the copy constructor).
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackup {
    pub finish_type: String,               // m_FinishType
    pub has_dielectric_constrains: bool,   // m_HasDielectricConstrains
    pub has_thickness_constrains: bool,    // m_HasThicknessConstrains
    pub edge_connector_constraints: BsEdgeConnectorConstraints, // m_EdgeConnectorConstraints
    pub edge_plating: bool,                // m_EdgePlating
    pub list: Vec<BoardStackupItem>,       // m_list
}

impl Default for BoardStackup {
    /// `BOARD_STACKUP::BOARD_STACKUP()` (`board_stackup.cpp:396`).
    fn default() -> Self {
        BoardStackup {
            finish_type: "None".to_string(), // undefined finish type
            has_dielectric_constrains: false,
            has_thickness_constrains: false,
            edge_connector_constraints: BsEdgeConnectorConstraints::None,
            edge_plating: false,
            list: Vec::new(),
        }
    }
}

impl BoardStackup {
    /// `BOARD_STACKUP::GetList`.
    pub fn list(&self) -> &[BoardStackupItem] {
        &self.list
    }

    /// `BOARD_STACKUP::GetCount`.
    pub fn count(&self) -> usize {
        self.list.len()
    }

    /// `BOARD_STACKUP::Add`.
    pub fn add(&mut self, item: BoardStackupItem) {
        self.list.push(item);
    }

    /// `BOARD_STACKUP::RemoveAll`.
    pub fn remove_all(&mut self) {
        self.list.clear();
    }

    /// `BOARD_STACKUP::GetStackupLayer`.
    pub fn stackup_layer(&self, index: usize) -> Option<&BoardStackupItem> {
        self.list.get(index)
    }

    /// `BOARD_STACKUP::StackupAllowedBrdLayers`.
    pub fn stackup_allowed_brd_layers() -> kicad_core::Lset {
        crate::port_pending!("BOARD_STACKUP::StackupAllowedBrdLayers")
    }

    /// `BOARD_STACKUP::BuildDefaultStackupList( aSettings, aActiveCopperLayersCount )`.
    pub fn build_default_stackup_list(
        &mut self,
        settings: Option<&BoardDesignSettings>,
        copper_layers: i32,
    ) {
        crate::port_pending!(
            "BOARD_STACKUP::BuildDefaultStackupList",
            settings,
            copper_layers
        )
    }

    /// `BOARD_STACKUP::BuildBoardThicknessFromStackup`.
    pub fn build_board_thickness_from_stackup(&self) -> i32 {
        crate::port_pending!("BOARD_STACKUP::BuildBoardThicknessFromStackup")
    }

    /// `BOARD_STACKUP::GetLayerDistance`.
    pub fn get_layer_distance(&self, a: PcbLayerId, b: PcbLayerId) -> i32 {
        crate::port_pending!("BOARD_STACKUP::GetLayerDistance", a, b)
    }

    /// `BOARD_STACKUP::GetDrillDepth`.
    pub fn get_drill_depth(&self, a: PcbLayerId, b: PcbLayerId) -> i32 {
        crate::port_pending!("BOARD_STACKUP::GetDrillDepth", a, b)
    }

    /// `BOARD_STACKUP::SynchronizeWithBoard`.
    pub fn synchronize_with_board(&mut self, settings: &BoardDesignSettings) -> bool {
        crate::port_pending!("BOARD_STACKUP::SynchronizeWithBoard", settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BOARD_STACKUP::BOARD_STACKUP` (`board_stackup.cpp:396`) and the per-type
    /// `BOARD_STACKUP_ITEM::BOARD_STACKUP_ITEM` values (`board_stackup.cpp:54-103`).
    #[test]
    fn constructor_defaults() {
        let s = BoardStackup::default();
        assert_eq!(s.finish_type, "None");
        assert_eq!(s.edge_connector_constraints, BsEdgeConnectorConstraints::None);
        assert!(s.list.is_empty() && !s.edge_plating);

        let cu = BoardStackupItem::new(BoardStackupItemType::Copper);
        assert_eq!(cu.type_name, "copper");
        assert_eq!(cu.thickness(0), 35_000);
        assert_eq!(cu.layer_id, PcbLayerId::UndefinedLayer);
        assert_eq!(cu.dielectric_layer_id, 1);
        assert!(cu.enabled);
        assert_eq!(cu.epsilon_r(0), 1.0);

        let core = BoardStackupItem::new(BoardStackupItemType::Dielectric);
        assert_eq!(core.type_name, "core");
        assert_eq!(core.material(0), "FR4");
        assert_eq!(core.color(0), "Not specified");
        assert_eq!(core.epsilon_r(0), 4.5);
        assert_eq!(core.loss_tangent(0), 0.02);
        assert_eq!(core.spec_freq(0), 1e9);
        assert_eq!(core.thickness(0), 0);

        let mask = BoardStackupItem::new(BoardStackupItemType::Soldermask);
        assert_eq!(mask.thickness(0), 10_000);
        assert_eq!(mask.epsilon_r(0), 3.3);
        assert_eq!(mask.material(0), "Not specified");

        let silk = BoardStackupItem::new(BoardStackupItemType::Silkscreen);
        assert_eq!(silk.type_name, "silkscreen");
        assert_eq!(silk.epsilon_r(0), 1.0);

        let mut paste = BoardStackupItem::new(BoardStackupItemType::Solderpaste);
        assert_eq!(paste.type_name, "solderpaste");
        paste.set_thickness(5, 3); // out of range: ignored
        assert_eq!(paste.thickness(0), 0);
        assert_eq!(paste.sublayers_count(), 1);
    }
}
