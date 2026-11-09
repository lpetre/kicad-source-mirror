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
//! accessors (package M0d), and the stack-up algorithms (package M6). Also ports
//! `pcbnew/board_stackup_manager/stackup_predefined_prms.h` / `.cpp` (material, colour and
//! copper finish tables). `FormatBoardStackup` (the writer side) belongs to the board I/O
//! package PCBIO-04.

use kicad_core::layer_ids::{is_copper_layer, is_copper_layer_lower_than};
use kicad_core::lset::Lset;
use kicad_core::string_utils::ui_double2_str;
use kicad_core::{Color4D, PCB_IU_SCALE, PcbLayerId};

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
    Constant = 0, // CONSTANT
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
    pub type_: BoardStackupItemType,               // m_Type
    pub layer_name: String,                        // m_LayerName
    pub type_name: String,                         // m_TypeName
    pub layer_id: PcbLayerId,                      // m_LayerId
    pub dielectric_layer_id: i32,                  // m_DielectricLayerId
    pub dielectric_prms_list: Vec<DielectricPrms>, // m_DielectricPrmsList
    pub enabled: bool,                             // m_enabled
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

    /// `BOARD_STACKUP_ITEM::AddDielectricPrms`: inserts a default `DIELECTRIC_PRMS` at
    /// `index`. C++ `emplace( begin() + idx )` is undefined for `idx > size()`; such a call
    /// is ignored here.
    pub fn add_dielectric_prms(&mut self, index: usize) {
        // add a DIELECTRIC_PRMS item to m_DielectricPrmsList
        debug_assert!(index <= self.dielectric_prms_list.len());

        if index <= self.dielectric_prms_list.len() {
            self.dielectric_prms_list
                .insert(index, DielectricPrms::default());
        }
    }

    /// `BOARD_STACKUP_ITEM::RemoveDielectricPrms`.
    pub fn remove_dielectric_prms(&mut self, index: usize) {
        // Remove a DIELECTRIC_PRMS item from m_DielectricPrmsList if possible
        if self.sublayers_count() < 2 || index >= self.sublayers_count() {
            return;
        }

        self.dielectric_prms_list.remove(index);
    }

    /// `BOARD_STACKUP_ITEM::IsEnabled`.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// `BOARD_STACKUP_ITEM::SetEnabled`.
    pub fn set_enabled(&mut self, enable: bool) {
        self.enabled = enable;
    }

    /// `BOARD_STACKUP_ITEM::GetType`.
    pub fn type_(&self) -> BoardStackupItemType {
        self.type_
    }

    /// `BOARD_STACKUP_ITEM::GetBrdLayerId`.
    pub fn brd_layer_id(&self) -> PcbLayerId {
        self.layer_id
    }

    /// `BOARD_STACKUP_ITEM::SetBrdLayerId`.
    pub fn set_brd_layer_id(&mut self, layer: PcbLayerId) {
        self.layer_id = layer;
    }

    /// `BOARD_STACKUP_ITEM::GetLayerName`.
    pub fn layer_name(&self) -> &str {
        &self.layer_name
    }

    /// `BOARD_STACKUP_ITEM::SetLayerName`.
    pub fn set_layer_name(&mut self, name: &str) {
        self.layer_name = name.to_string();
    }

    /// `BOARD_STACKUP_ITEM::GetTypeName`.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// `BOARD_STACKUP_ITEM::SetTypeName`.
    pub fn set_type_name(&mut self, name: &str) {
        self.type_name = name.to_string();
    }

    /// `BOARD_STACKUP_ITEM::GetDielectricLayerId`.
    pub fn dielectric_layer_id(&self) -> i32 {
        self.dielectric_layer_id
    }

    /// `BOARD_STACKUP_ITEM::SetDielectricLayerId`.
    pub fn set_dielectric_layer_id(&mut self, layer_id: i32) {
        self.dielectric_layer_id = layer_id;
    }

    /// `BOARD_STACKUP_ITEM::HasEpsilonRValue`.
    pub fn has_epsilon_r_value(&self) -> bool {
        self.type_ == BoardStackupItemType::Dielectric
            || self.type_ == BoardStackupItemType::Soldermask
    }

    /// `BOARD_STACKUP_ITEM::HasLossTangentValue`.
    pub fn has_loss_tangent_value(&self) -> bool {
        self.type_ == BoardStackupItemType::Dielectric
            || self.type_ == BoardStackupItemType::Soldermask
    }

    /// `BOARD_STACKUP_ITEM::HasSpecFreqValue`.
    pub fn has_spec_freq_value(&self) -> bool {
        self.type_ == BoardStackupItemType::Dielectric
    }

    /// `BOARD_STACKUP_ITEM::HasMaterialValue`: true if the material is specified.
    pub fn has_material_value(&self, sublayer: usize) -> bool {
        self.is_material_editable() && is_prm_specified(&self.material(sublayer))
    }

    /// `BOARD_STACKUP_ITEM::IsMaterialEditable`.
    pub fn is_material_editable(&self) -> bool {
        self.type_ == BoardStackupItemType::Dielectric
            || self.type_ == BoardStackupItemType::Soldermask
            || self.type_ == BoardStackupItemType::Silkscreen
    }

    /// `BOARD_STACKUP_ITEM::IsColorEditable`.
    pub fn is_color_editable(&self) -> bool {
        self.type_ == BoardStackupItemType::Dielectric
            || self.type_ == BoardStackupItemType::Soldermask
            || self.type_ == BoardStackupItemType::Silkscreen
    }

    /// `BOARD_STACKUP_ITEM::IsThicknessEditable`.
    pub fn is_thickness_editable(&self) -> bool {
        self.type_ == BoardStackupItemType::Copper
            || self.type_ == BoardStackupItemType::Dielectric
            || self.type_ == BoardStackupItemType::Soldermask
    }

    /// `BOARD_STACKUP_ITEM::FormatEpsilonR`: Epsilon R for print / display (no scientific
    /// notation).
    pub fn format_epsilon_r(&self, sublayer: usize) -> String {
        ui_double2_str(self.epsilon_r(sublayer))
    }

    /// `BOARD_STACKUP_ITEM::FormatLossTangent`: loss tangent for print / display (no
    /// scientific notation).
    pub fn format_loss_tangent(&self, sublayer: usize) -> String {
        ui_double2_str(self.loss_tangent(sublayer))
    }

    /// `BOARD_STACKUP_ITEM::FormatDielectricLayerName`: `"Dielectric %d"` (the untranslated
    /// text; the C++ passes it through `_()` for the dialogs).
    pub fn format_dielectric_layer_name(&self) -> String {
        format!("Dielectric {}", self.dielectric_layer_id())
    }
}

/// `BOARD_STACKUP`. `m_list` owns its items (C++ deep-copies them in the copy constructor).
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackup {
    pub finish_type: String,                                    // m_FinishType
    pub has_dielectric_constrains: bool,                        // m_HasDielectricConstrains
    pub has_thickness_constrains: bool,                         // m_HasThicknessConstrains
    pub edge_connector_constraints: BsEdgeConnectorConstraints, // m_EdgeConnectorConstraints
    pub edge_plating: bool,                                     // m_EdgePlating
    pub list: Vec<BoardStackupItem>,                            // m_list
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

    /// `BOARD_STACKUP::GetStackupLayer` (mutable overload; the C++ returns a non-const
    /// pointer).
    pub fn stackup_layer_mut(&mut self, index: usize) -> Option<&mut BoardStackupItem> {
        self.list.get_mut(index)
    }

    /// `BOARD_STACKUP::StackupAllowedBrdLayers`: the board layers allowed in the stackup
    /// list, i.e. the SilkS, Mask, Paste and all copper layers.
    pub fn stackup_allowed_brd_layers() -> Lset {
        let mut set = Lset::from_layers(&[
            PcbLayerId::FSilkS,
            PcbLayerId::FMask,
            PcbLayerId::FPaste,
            PcbLayerId::BSilkS,
            PcbLayerId::BMask,
            PcbLayerId::BPaste,
        ]);
        set |= Lset::external_cu_mask();
        set |= Lset::internal_cu_mask();
        set
    }

    /// `BOARD_STACKUP::BuildDefaultStackupList( aSettings, aActiveCopperLayersCount )`:
    /// appends (the C++ does not clear `m_list` first) a default stackup built from the
    /// settings, or a full 32 copper layer stackup when `settings` is `None`.
    pub fn build_default_stackup_list(
        &mut self,
        settings: Option<&BoardDesignSettings>,
        copper_layers: i32,
    ) {
        // Creates a default stackup, according to the current BOARD_DESIGN_SETTINGS settings.
        // Note: the m_TypeName string is made translatable using _HKI marker, but is not
        // translated when building the stackup.
        // It will be used as this in files, and can be translated only in dialog
        // if aSettings == NULL, build a full stackup (with 32 copper layers)
        let enabled_layer = match settings {
            Some(s) => s.enabled_layers().clone(),
            None => Self::stackup_allowed_brd_layers(),
        };
        let copper_layer_count = settings.map_or(32, |s| s.copper_layer_count());

        // We need to calculate a suitable dielectric layer thickness.
        // If no settings, and if aActiveCopperLayersCount is given, use it
        // (If no settings, and no aActiveCopperLayersCount, the full 32 layers are used)
        let mut active_cu_layer_count = copper_layer_count;

        if settings.is_none() && copper_layers > 0 {
            active_cu_layer_count = copper_layers;
        }

        let brd_thickness = settings.map_or(PCB_IU_SCALE.mm_to_iu(1.6), |s| s.board_thickness());
        let mut diel_thickness =
            brd_thickness - (BoardStackupItem::copper_default_thickness() * active_cu_layer_count);

        // Take in account the solder mask thickness:
        let sm_count = (&enabled_layer
            & &Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]))
            .count() as i32;
        diel_thickness -= BoardStackupItem::mask_default_thickness() * sm_count;
        diel_thickness /= 1.max(active_cu_layer_count - 1);

        let mut dielectric_idx: i32 = 0;

        // Add silk screen, solder mask and solder paste layers on top
        if enabled_layer.contains(PcbLayerId::FSilkS) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Silkscreen);
            item.set_brd_layer_id(PcbLayerId::FSilkS);
            item.set_type_name("Top Silk Screen");
            self.add(item);
        }

        if enabled_layer.contains(PcbLayerId::FPaste) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Solderpaste);
            item.set_brd_layer_id(PcbLayerId::FPaste);
            item.set_type_name("Top Solder Paste");
            self.add(item);
        }

        if enabled_layer.contains(PcbLayerId::FMask) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Soldermask);
            item.set_brd_layer_id(PcbLayerId::FMask);
            item.set_type_name("Top Solder Mask");
            self.add(item);
        }

        // Add copper and dielectric layers
        for layer in enabled_layer.cu_stack() {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Copper);
            item.set_brd_layer_id(layer);
            item.set_type_name(KEY_COPPER);
            self.add(item);

            if layer == PcbLayerId::BCu {
                break;
            }

            // Add the dielectric layer:
            let mut item = BoardStackupItem::new(BoardStackupItemType::Dielectric);
            item.set_thickness(diel_thickness, 0);
            item.set_dielectric_layer_id(dielectric_idx + 1);

            // Display a dielectric default layer name:
            if (dielectric_idx & 1) == 0 {
                item.set_type_name(KEY_CORE);
                item.set_material("FR4".to_string(), 0);
            } else {
                item.set_type_name(KEY_PREPREG);
                item.set_material("FR4".to_string(), 0);
            }

            self.add(item);
            dielectric_idx += 1;
        }

        // Add silk screen, solder mask and solder paste layers on bottom
        if enabled_layer.contains(PcbLayerId::BMask) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Soldermask);
            item.set_brd_layer_id(PcbLayerId::BMask);
            item.set_type_name("Bottom Solder Mask");
            self.add(item);
        }

        if enabled_layer.contains(PcbLayerId::BPaste) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Solderpaste);
            item.set_brd_layer_id(PcbLayerId::BPaste);
            item.set_type_name("Bottom Solder Paste");
            self.add(item);
        }

        if enabled_layer.contains(PcbLayerId::BSilkS) {
            let mut item = BoardStackupItem::new(BoardStackupItemType::Silkscreen);
            item.set_brd_layer_id(PcbLayerId::BSilkS);
            item.set_type_name("Bottom Silk Screen");
            self.add(item);
        }

        // Transfer other stackup settings from aSettings
        if let Some(s) = settings {
            let source_stackup = s.stackup_descriptor();
            self.edge_connector_constraints = source_stackup.edge_connector_constraints;
            self.has_dielectric_constrains = source_stackup.has_dielectric_constrains;
            self.edge_plating = source_stackup.edge_plating;
            self.finish_type = source_stackup.finish_type.clone();
        }
    }

    /// `BOARD_STACKUP::BuildBoardThicknessFromStackup`: the board thickness from the
    /// thickness of the items.
    pub fn build_board_thickness_from_stackup(&self) -> i32 {
        // return the board thickness from the thickness of BOARD_STACKUP_ITEM list
        let mut thickness = 0;

        for item in &self.list {
            if item.is_thickness_editable() && item.is_enabled() {
                thickness += item.thickness(0);

                // dielectric layers can have more than one main layer
                // add thickness of all sublayers
                for idx in 1..item.sublayers_count() {
                    thickness += item.thickness(idx);
                }
            }
        }

        thickness
    }

    /// `BOARD_STACKUP::GetLayerDistance`: distance between two copper layers (half of each
    /// internal copper end layer is counted).
    pub fn get_layer_distance(&self, a: PcbLayerId, b: PcbLayerId) -> i32 {
        debug_assert!(is_copper_layer(a) && is_copper_layer(b));

        let mut first_layer = a;
        let mut second_layer = b;

        if first_layer == second_layer {
            return 0;
        }

        // B_Cu is always the last copper layer but doesn't have the last numerical value
        if second_layer != PcbLayerId::BCu
            && (second_layer < first_layer || first_layer == PcbLayerId::BCu)
        {
            std::mem::swap(&mut first_layer, &mut second_layer);
        }

        let mut total = 0;
        let mut start = false;
        let mut half = false;

        for item in &self.list {
            // Will be UNDEFINED_LAYER for dielectrics
            let layer = item.brd_layer_id();

            if layer != PcbLayerId::UndefinedLayer && !is_copper_layer(layer) {
                continue; // Silk/mask layer
            }

            // Reached the start copper layer?  Start counting the next dielectric after it
            if !start && (layer != PcbLayerId::UndefinedLayer && layer == first_layer) {
                start = true;

                // Only count half of each internal copper layer
                if first_layer != PcbLayerId::FCu && first_layer != PcbLayerId::BCu {
                    half = true;
                }
            } else if !start {
                continue;
            }

            // Reached the stop copper layer?  we're done
            if start && (layer != PcbLayerId::UndefinedLayer && layer == second_layer) {
                // Only count half of each internal copper layer
                if second_layer != PcbLayerId::FCu && second_layer != PcbLayerId::BCu {
                    half = true;
                }
            }

            for sublayer in 0..item.sublayers_count() {
                let sub_thickness = item.thickness(sublayer);
                total += if half {
                    sub_thickness / 2
                } else {
                    sub_thickness
                };
            }

            half = false;

            if layer != PcbLayerId::UndefinedLayer && layer == second_layer {
                break;
            }
        }

        total
    }

    /// `BOARD_STACKUP::GetDrillDepth`. As in the C++, `a` only selects the walk direction.
    pub fn get_drill_depth(&self, a: PcbLayerId, b: PcbLayerId) -> i32 {
        let mut total = 0;
        let mut half = false;
        let mut done = false;

        // The C++ lambda `processStackupItem` capturing total / half / done by reference.
        let process_stackup_item =
            |item: &BoardStackupItem, total: &mut i32, half: &mut bool, done: &mut bool| {
                // Will be UNDEFINED_LAYER for dielectrics
                let layer = item.brd_layer_id();

                if layer != PcbLayerId::UndefinedLayer && !is_copper_layer(layer) {
                    return; // Silk/mask layer
                }

                if item.sublayers_count() > 0 {
                    for sublayer in 0..item.sublayers_count() {
                        let sub_thickness = item.thickness(sublayer);
                        *total += if *half {
                            sub_thickness / 2
                        } else {
                            sub_thickness
                        };
                    }
                } else if is_copper_layer(layer) {
                    *total += BoardStackupItem::copper_default_thickness();
                }

                if *half {
                    *done = true;
                } else if layer == b {
                    *half = true;
                }
            };

        if is_copper_layer_lower_than(b, a) {
            for item in &self.list {
                process_stackup_item(item, &mut total, &mut half, &mut done);

                if done {
                    return total;
                }
            }
        } else {
            for item in self.list.iter().rev() {
                process_stackup_item(item, &mut total, &mut half, &mut done);

                if done {
                    return total;
                }
            }
        }

        total
    }

    /// `BOARD_STACKUP::SynchronizeWithBoard`: rebuilds the list for the enabled layers of
    /// `settings`, keeping the existing items of layers that still exist. Returns true if
    /// layers were removed or added.
    ///
    /// The C++ is called on `aSettings->GetStackupDescriptor()` itself; in Rust the caller
    /// passes a copy of the settings (or synchronises a copy of the stackup).
    pub fn synchronize_with_board(&mut self, settings: &BoardDesignSettings) -> bool {
        let mut change = false;
        // Build the suitable stackup:
        let mut stackup = BoardStackup::default();
        stackup.build_default_stackup_list(Some(settings), 0);

        let same_layer = |first: &BoardStackupItem, second: &BoardStackupItem| {
            first.brd_layer_id() == second.brd_layer_id()
                && (first.brd_layer_id() != PcbLayerId::UndefinedLayer
                    || first.dielectric_layer_id() == second.dielectric_layer_id())
        };

        // First, find removed layers:
        for curr_item in &self.list {
            if !stackup.list.iter().any(|item| same_layer(curr_item, item)) {
                change = true;
                break;
            }
        }

        // Now initialize all stackup items to the initial values, when exist
        for item in stackup.list.iter_mut() {
            let initial = self.list.iter().find(|other| same_layer(item, other));

            match initial {
                Some(initial) => *item = initial.clone(),
                None => change = true,
            }
        }

        // Transfer layer settings:
        *self = stackup;

        // Transfer other stackup settings from aSettings
        let source_stackup = settings.stackup_descriptor();
        self.has_dielectric_constrains = source_stackup.has_dielectric_constrains;
        self.edge_connector_constraints = source_stackup.edge_connector_constraints;
        self.edge_plating = source_stackup.edge_plating;
        self.finish_type = source_stackup.finish_type.clone();

        change
    }
}

/// `IsPrmSpecified` (`board_stackup.cpp`): true if the parameter value is neither empty nor
/// `NotSpecifiedPrm()` (case-insensitive). Without translations the `wxGetTranslation`
/// comparison of the C++ is the same test.
pub fn is_prm_specified(prm_value: &str) -> bool {
    // return true if the param value is specified:
    !prm_value.is_empty() && !wx_cmp_no_case_eq(prm_value, NOT_SPECIFIED_PRM)
}

/// `wxString::CmpNoCase( b ) == 0`: per-character comparison after `towlower`. A character
/// whose Rust lower case is several characters maps to the first one (`U+0130` -> `i`, as
/// glibc `towlower` does).
fn wx_cmp_no_case_eq(a: &str, b: &str) -> bool {
    fn lower(c: char) -> char {
        c.to_lowercase().next().unwrap_or(c)
    }

    a.chars().count() == b.chars().count()
        && a.chars().zip(b.chars()).all(|(x, y)| lower(x) == lower(y))
}

// ---------------------------------------------------------------------------------------
// Port of `pcbnew/board_stackup_manager/stackup_predefined_prms.h` and `.cpp`.
// ---------------------------------------------------------------------------------------

/// `DEFAULT_SOLDERMASK_OPACITY`.
pub const DEFAULT_SOLDERMASK_OPACITY: f64 = 0.83;

/// `FAB_LAYER_COLOR`: a colour of the gerber job file / stackup dialog lists.
#[derive(Clone, Debug, PartialEq)]
pub struct FabLayerColor {
    /// m_colorName: the name (in job file) of the color. User values are the HTML encoded
    /// "#rrggbbaa" RGB hexa value.
    pub color_name: &'static str,
    /// m_color
    pub color: Color4D,
}

impl FabLayerColor {
    /// `FAB_LAYER_COLOR( aColorName, wxColor( r, g, b, a ) )`.
    const fn new(color_name: &'static str, r: u8, g: u8, b: u8, a: u8) -> Self {
        FabLayerColor {
            color_name,
            color: Color4D::new(
                r as f64 / 255.0,
                g as f64 / 255.0,
                b as f64 / 255.0,
                a as f64 / 255.0,
            ),
        }
    }

    /// `FAB_LAYER_COLOR::GetName`.
    pub fn name(&self) -> &'static str {
        self.color_name
    }

    /// `FAB_LAYER_COLOR::GetColor( aItemType )`: solder mask colours get the default
    /// opacity, the others are opaque.
    pub fn color(&self, item_type: BoardStackupItemType) -> Color4D {
        if item_type == BoardStackupItemType::Soldermask {
            self.color.with_alpha(DEFAULT_SOLDERMASK_OPACITY)
        } else {
            self.color.with_alpha(1.0)
        }
    }

    /// `FAB_LAYER_COLOR::GetColorAsString`: a colour name acceptable in a gerber job file:
    /// one of the normalized colour names, or `R<int>G<int>B<int>` (0 to 255).
    pub fn color_as_string(&self) -> String {
        if is_color_name_normalized(self.color_name) {
            return self.color_name.to_string();
        }

        format!(
            "R{}G{}B{}",
            (self.color.r * 255.0) as i32,
            (self.color.g * 255.0) as i32,
            (self.color.b * 255.0) as i32
        )
    }
}

/// `copperFinishType[]`: the standard copper finish type names. They are standard names in
/// .gbrjob files, so avoid changing them or ensure they are compatible with .gbrjob file spec.
static COPPER_FINISH_TYPE: [&str; 14] = [
    NOT_SPECIFIED_PRM,  // Not specified, not in .gbrjob file
    "ENIG",             // used in .gbrjob file
    "ENEPIG",           // used in .gbrjob file
    "HAL SnPb",         // used in .gbrjob file
    "HAL lead-free",    // used in .gbrjob file
    "Hard gold",        // used in .gbrjob file
    "Immersion tin",    // used in .gbrjob file
    "Immersion nickel", // used in .gbrjob file
    "Immersion silver", // used in .gbrjob file
    "Immersion gold",   // used in .gbrjob file
    "HT_OSP",           // used in .gbrjob file
    "OSP",              // used in .gbrjob file
    "None",             // used in .gbrjob file
    "User defined",     // keep this option at end
];

/// `gbrjobColors`: the available colours for solder mask and silkscreen. These names are
/// used in .gbrjob file, so they are not fully free. For other (user defined) colours the
/// value is the html colour syntax in .kicad_pcb files and R<int>G<int>B<int> in .gbrjob.
static GBRJOB_COLORS: [FabLayerColor; 9] = [
    FabLayerColor::new(NOT_SPECIFIED_PRM, 80, 80, 80, 255), // Not specified, not in .gbrjob
    FabLayerColor::new("Green", 60, 150, 80, 255),
    FabLayerColor::new("Red", 128, 0, 0, 255),
    FabLayerColor::new("Blue", 0, 0, 128, 255),
    FabLayerColor::new("Purple", 80, 0, 80, 255),
    FabLayerColor::new("Black", 20, 20, 20, 255),
    FabLayerColor::new("White", 200, 200, 200, 255),
    FabLayerColor::new("Yellow", 128, 128, 0, 255),
    FabLayerColor::new("User defined", 128, 128, 128, 255), // Free; the name is a dummy name
];

/// `dielectricColors`: used primarily as a source for the 3D renderer. They are written as
/// R<int>G<int>B<int> to the .gbrjob file.
static DIELECTRIC_COLORS: [FabLayerColor; 7] = [
    FabLayerColor::new(NOT_SPECIFIED_PRM, 80, 80, 80, 255),
    FabLayerColor::new("FR4 natural", 109, 116, 75, 212),
    FabLayerColor::new("PTFE natural", 252, 252, 250, 230),
    FabLayerColor::new("Polyimide", 205, 130, 0, 170),
    FabLayerColor::new("Phenolic natural", 92, 17, 6, 230),
    FabLayerColor::new("Aluminum", 213, 213, 213, 255),
    FabLayerColor::new("User defined", 128, 128, 128, 212),
];

/// `GetStandardCopperFinishes( aTranslate )`: always the untranslated names (no
/// translations in the port).
pub fn standard_copper_finishes() -> Vec<String> {
    COPPER_FINISH_TYPE.iter().map(|s| s.to_string()).collect()
}

/// `GetStandardColors( aType )`: empty for types without a colour list.
pub fn standard_colors(type_: BoardStackupItemType) -> &'static [FabLayerColor] {
    match type_ {
        BoardStackupItemType::Silkscreen => &GBRJOB_COLORS,
        BoardStackupItemType::Soldermask => &GBRJOB_COLORS,
        BoardStackupItemType::Dielectric => &DIELECTRIC_COLORS,
        _ => &[],
    }
}

/// `GetColorUserDefinedListIdx( aType )`: the index of the last ("User defined") entry;
/// -1 for an empty list (`size() - 1` converted to `int` in the C++).
pub fn color_user_defined_list_idx(type_: BoardStackupItemType) -> i32 {
    // this is the last item in list
    standard_colors(type_).len() as i32 - 1
}

/// `GetDefaultUserColor( aType )`. `None` for a type without a colour list (C++ indexes
/// an empty vector there).
pub fn default_user_color(type_: BoardStackupItemType) -> Option<Color4D> {
    standard_color(type_, color_user_defined_list_idx(type_))
}

/// `GetStandardColor( aType, aIdx )`; `None` for an index out of range.
pub fn standard_color(type_: BoardStackupItemType, idx: i32) -> Option<Color4D> {
    usize::try_from(idx)
        .ok()
        .and_then(|i| standard_colors(type_).get(i))
        .map(|c| c.color(type_))
}

/// `GetStandardColorName( aType, aIdx )`; `None` for an index out of range.
pub fn standard_color_name(type_: BoardStackupItemType, idx: i32) -> Option<&'static str> {
    usize::try_from(idx)
        .ok()
        .and_then(|i| standard_colors(type_).get(i))
        .map(|c| c.name())
}

/// `IsCustomColorIdx( aType, aIdx )`.
pub fn is_custom_color_idx(type_: BoardStackupItemType, idx: i32) -> bool {
    idx == color_user_defined_list_idx(type_)
}

/// `IsColorNameNormalized`: the colour names of the gerber job file specification (note:
/// "Purple" is not in the C++ list).
pub fn is_color_name_normalized(name: &str) -> bool {
    const LIST: [&str; 6] = ["Green", "Red", "Blue", "Black", "White", "Yellow"];

    LIST.iter()
        .any(|candidate| wx_cmp_no_case_eq(candidate, name))
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
        assert_eq!(
            s.edge_connector_constraints,
            BsEdgeConnectorConstraints::None
        );
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
