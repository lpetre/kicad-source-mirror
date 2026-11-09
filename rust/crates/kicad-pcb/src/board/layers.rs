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

//! Port of the layer functions of `pcbnew/board.cpp` (`GetCopperLayerCount`, `FlipLayer`,
//! `recalcOpposites`, `GetLayerName`, `SetLayerDescr`, ...) and of `LAYER` / `LAYER_T`
//! (`pcbnew/board.h`). Package M22; M0d wrote the types, the field-read hooks the kernel needs
//! (`copper_layer_count`, `flip_layer`, `build_layer_env`) and the constructor part
//! (`recalc_opposites`).

use kicad_core::layer_ids::{is_copper_layer, layer_name};
use kicad_core::{Lset, PCB_LAYER_ID_COUNT, PcbLayerId};

use crate::board::Board;
use crate::board_stackup::BoardStackup;
use crate::ctx::LayerEnv;

/// `LAYER_T`; the values are written to files.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum LayerT {
    #[default]
    Undefined = -1,
    Signal = 0,
    Power,
    Mixed,
    Jumper,
    Aux,
    Front,
    Back,
}

impl LayerT {
    /// `LAYER::ShowType`.
    pub fn show_type(self) -> &'static str {
        match self {
            LayerT::Power => "power",
            LayerT::Mixed => "mixed",
            LayerT::Jumper => "jumper",
            LayerT::Aux => "auxiliary",
            LayerT::Front => "front",
            LayerT::Back => "back",
            LayerT::Signal | LayerT::Undefined => "signal",
        }
    }

    /// `LAYER::ParseType`; `Undefined` for an unknown name.
    pub fn parse_type(name: &str) -> LayerT {
        match name {
            "signal" => LayerT::Signal,
            "power" => LayerT::Power,
            "mixed" => LayerT::Mixed,
            "jumper" => LayerT::Jumper,
            "auxiliary" => LayerT::Aux,
            "front" => LayerT::Front,
            "back" => LayerT::Back,
            _ => LayerT::Undefined,
        }
    }
}

/// `LAYER` (`pcbnew/board.h`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub name: String,      // m_name: the canonical name (LSET::Name)
    pub user_name: String, // m_userName
    pub type_: LayerT,     // m_type
    pub visible: bool,     // m_visible
    pub number: i32,       // m_number: the layer id
    pub opposite: i32,     // m_opposite: similar layer on the opposite side, if any
}

impl Default for Layer {
    /// `LAYER::LAYER()` = `clear()`: signal, visible, number 0, empty names, opposite =
    /// number.
    fn default() -> Self {
        Layer {
            name: String::new(),
            user_name: String::new(),
            type_: LayerT::Signal,
            visible: true,
            number: 0,
            opposite: 0,
        }
    }
}

/// `wxString::AfterFirst( '.' )`: the text after the first `.`, empty when there is none.
fn after_first_dot(s: &str) -> &str {
    s.split_once('.').map_or("", |(_, rest)| rest)
}

impl Board {
    /// `BOARD::GetCopperLayerCount()`: the design settings' count.
    pub fn copper_layer_count(&self) -> i32 {
        self.design_settings.copper_layer_count()
    }

    /// `BOARD::FlipLayer( aLayer )`: the `m_opposite` of the layer's `LAYER` entry, or the
    /// layer itself when there is none.
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId {
        self.layers
            .get(&layer.0)
            .map_or(layer, |l| PcbLayerId(l.opposite))
    }

    /// Build the `LayerEnv` snapshot from the current settings and layers
    /// (`board-model-types.md` 0: copper count, enabled layers, `FlipLayer` table).
    pub(crate) fn build_layer_env(&self) -> LayerEnv {
        LayerEnv {
            copper_layer_count: self.copper_layer_count(),
            enabled_layers: self.design_settings.enabled_layers().clone(),
            flipped: (0..PCB_LAYER_ID_COUNT)
                .map(|l| self.flip_layer(PcbLayerId(l)))
                .collect(),
        }
    }

    /// The `m_layers` initialisation of `BOARD::BOARD()`: every layer gets its standard name
    /// and type (copper: signal; odd ids from `User_1`: auxiliary; else undefined), then
    /// `recalcOpposites()`. `m_number` keeps the `LAYER()` value 0, as in C++.
    pub(crate) fn init_layers(&mut self) {
        for layer in 0..PCB_LAYER_ID_COUNT {
            let entry = self.layers.entry(layer).or_default();
            entry.name = Board::standard_layer_name(PcbLayerId(layer));

            entry.type_ = if is_copper_layer(layer) {
                LayerT::Signal
            } else if layer >= PcbLayerId::User1.0 && (layer & 1) != 0 {
                LayerT::Aux
            } else {
                LayerT::Undefined
            };
        }

        self.recalc_opposites();
    }

    /// `BOARD::recalcOpposites()`. `std::map::operator[]` inserts a default `LAYER` for a
    /// missing key; `entry().or_default()` does the same.
    pub(crate) fn recalc_opposites(&mut self) {
        let copper_count = self.copper_layer_count();

        for layer in PcbLayerId::FCu.0..PCB_LAYER_ID_COUNT {
            self.layers.entry(layer).or_default().opposite =
                kicad_core::layer_ids::flip_layer(PcbLayerId(layer), copper_count).0;
        }

        let is_side = |t: LayerT| t == LayerT::Front || t == LayerT::Back;

        // Match up similary-named front/back user layers
        let mut layer = PcbLayerId::User1.0;

        while layer <= PCB_LAYER_ID_COUNT {
            let current = self.layers.entry(layer).or_default().clone();

            if current.opposite == layer && is_side(current.type_) {
                let principal_name = after_first_dot(&current.user_name).to_string();
                let mut ii = layer + 2;

                while ii <= PCB_LAYER_ID_COUNT {
                    let other = self.layers.entry(ii).or_default().clone();

                    if other.opposite == ii
                        && is_side(other.type_)
                        && current.type_ != other.type_
                    {
                        let candidate = after_first_dot(&other.user_name);

                        if !candidate.is_empty() && candidate == principal_name {
                            self.layers.entry(layer).or_default().opposite = ii;
                            self.layers.entry(ii).or_default().opposite = layer;
                            break;
                        }
                    }

                    ii += 2;
                }
            }

            layer += 2;
        }

        // Match up non-custom-named consecutive front/back user layer pairs
        let mut layer = PcbLayerId::User1.0;

        while layer < PCB_LAYER_ID_COUNT - 2 {
            let next = layer + 2;
            let a = self.layers.entry(layer).or_default().clone();
            let b = self.layers.entry(next).or_default().clone();

            // ignore already-matched layers and pairs that aren't consecutive front/back
            if a.opposite == layer
                && b.opposite == next
                && a.type_ == LayerT::Front
                && b.type_ == LayerT::Back
                && a.user_name != a.name
                && b.user_name != b.name
            {
                self.layers.entry(layer).or_default().opposite = next;
                self.layers.entry(next).or_default().opposite = layer;
            }

            layer += 2;
        }
    }

    /// `BOARD::SetCopperLayerCount`: the settings' count, `recalcOpposites()`, and the
    /// `LayerEnv` snapshot marked stale (and the time stamp bumped, `board-model.md` 8.5).
    pub fn set_copper_layer_count(&mut self, count: i32) {
        self.design_settings_mut().set_copper_layer_count(count);
        self.recalc_opposites();
    }

    /// `BOARD::GetEnabledLayers`.
    pub fn enabled_layers(&self) -> Lset {
        self.design_settings.enabled_layers().clone()
    }

    /// `BOARD::SetEnabledLayers`.
    pub fn set_enabled_layers(&mut self, layers: Lset) {
        crate::port_pending!("BOARD::SetEnabledLayers", layers)
    }

    /// `BOARD::IsLayerEnabled`.
    pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool {
        crate::port_pending!("BOARD::IsLayerEnabled", layer)
    }

    /// `BOARD::GetStandardLayerName`: the fixed name of the layer id (`LayerName`).
    pub fn standard_layer_name(layer: PcbLayerId) -> String {
        layer_name(layer)
    }

    /// `BOARD::GetLayerName`: the user name of an enabled layer, else the standard name.
    pub fn layer_name(&self, layer: PcbLayerId) -> String {
        crate::port_pending!("BOARD::GetLayerName", layer)
    }

    /// `BOARD::GetLayerID`.
    pub fn layer_id(&self, name: &str) -> PcbLayerId {
        crate::port_pending!("BOARD::GetLayerID", name)
    }

    /// `BOARD::GetLayerType`.
    pub fn layer_type(&self, layer: PcbLayerId) -> LayerT {
        crate::port_pending!("BOARD::GetLayerType", layer)
    }

    /// `BOARD::SetLayerDescr`.
    pub fn set_layer_descr(&mut self, layer: PcbLayerId, descr: Layer) -> bool {
        crate::port_pending!("BOARD::SetLayerDescr", layer, descr)
    }

    /// `BOARD::GetCopperLayerStackMaxId`.
    pub fn copper_layer_stack_max_id(&self) -> PcbLayerId {
        crate::port_pending!("BOARD::GetCopperLayerStackMaxId")
    }

    /// `BOARD::LayerDepth`.
    pub fn layer_depth(&self, start: PcbLayerId, end: PcbLayerId) -> i32 {
        crate::port_pending!("BOARD::LayerDepth", start, end)
    }

    /// `BOARD::IsFrontLayer`.
    pub fn is_front_layer(&self, layer: PcbLayerId) -> bool {
        crate::port_pending!("BOARD::IsFrontLayer", layer)
    }

    /// `BOARD::IsBackLayer`.
    pub fn is_back_layer(&self, layer: PcbLayerId) -> bool {
        crate::port_pending!("BOARD::IsBackLayer", layer)
    }

    /// `BOARD::GetStackupOrDefault`: the settings' stack-up if the board has one, else a
    /// default list for the copper count.
    pub fn get_stackup_or_default(&self) -> BoardStackup {
        crate::port_pending!("BOARD::GetStackupOrDefault")
    }
}
