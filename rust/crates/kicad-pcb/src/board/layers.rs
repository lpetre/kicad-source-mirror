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
//! (`pcbnew/board.h`), and `pcbnew/layer_utils.cpp` (`LAYER_UTILS`). Package M22; M0d wrote the types, the field-read hooks the kernel needs
//! (`copper_layer_count`, `flip_layer`, `build_layer_env`) and the constructor part
//! (`recalc_opposites`).

use kicad_core::layer_ids::{
    self, copper_layer_to_ordinal, is_copper_layer, layer_name, to_layer_id,
};
use kicad_core::{LayerRange, Lseq, Lset, PCB_LAYER_ID_COUNT, PcbLayerId};

use crate::board::Board;
use crate::board_stackup::BoardStackup;
use crate::ctx::{BoardCtx, LayerEnv};
use crate::ids::FootprintId;
use crate::item::RecurseMode;

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

                    if other.opposite == ii && is_side(other.type_) && current.type_ != other.type_
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

    /// `BOARD::SetEnabledLayers`: the design settings' mask (which also recounts the copper
    /// and user-defined layers). `BumpDrillModelGeneration()` has no counterpart (the drill
    /// model of the drill maps is tier 2).
    pub fn set_enabled_layers(&mut self, layers: Lset) {
        self.design_settings_mut().set_enabled_layers(&layers);
    }

    /// `BOARD::IsLayerEnabled`.
    pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool {
        self.design_settings.is_layer_enabled(layer)
    }

    /// `BOARD::GetStandardLayerName`: the fixed name of the layer id (`LayerName`).
    pub fn standard_layer_name(layer: PcbLayerId) -> String {
        layer_name(layer)
    }

    /// `BOARD::GetLayerName`: the user name of an enabled layer, else the standard name.
    pub fn layer_name(&self, layer: PcbLayerId) -> String {
        // All layer names are stored in the BOARD.
        if self.is_layer_enabled(layer) {
            // Standard names were set in BOARD::BOARD() but they may be over-ridden by
            // BOARD::SetLayerName().  For copper layers, return the user defined layer name,
            // if it was set.  Otherwise return the Standard English layer name.
            if let Some(l) = self.layers.get(&layer.0) {
                if !l.user_name.is_empty() {
                    return l.user_name.clone();
                }
            }
        }

        Board::standard_layer_name(layer)
    }

    /// `BOARD::SetLayerName`: `false` for a name with a quote character or a disabled layer
    /// (an empty name clears the user name of any layer and also returns `false`, as in C++).
    /// `BumpDrillModelGeneration()` has no counterpart (tier 2 drill model).
    pub fn set_layer_name(&mut self, layer: PcbLayerId, name: &str) -> bool {
        if name.is_empty() {
            // If the name is empty, we clear the user name.
            self.layers.entry(layer.0).or_default().user_name.clear();
            self.layers_changed();
        } else {
            // no quote chars in the name allowed
            if name.contains('"') {
                return false;
            }

            if self.is_layer_enabled(layer) {
                self.layers.entry(layer.0).or_default().user_name = name.to_string();
                self.layers_changed();
                return true;
            }
        }

        false
    }

    /// `BOARD::GetLayerID`: the layer whose canonical or user name is `name` (`m_layers` in
    /// id order), else the layer whose standard name it is, else `UNDEFINED_LAYER`.
    pub fn layer_id(&self, name: &str) -> PcbLayerId {
        // Check the BOARD physical layer names.
        for (&layer_id, layer) in &self.layers {
            if layer.name == name || layer.user_name == name {
                return to_layer_id(layer_id);
            }
        }

        // Otherwise fall back to the system standard layer names for virtual layers.
        for layer in 0..PCB_LAYER_ID_COUNT {
            if Board::standard_layer_name(to_layer_id(layer)) == name {
                return to_layer_id(layer);
            }
        }

        PcbLayerId::UndefinedLayer
    }

    /// `BOARD::GetLayerType`: the stored type of an enabled layer; otherwise auxiliary for a
    /// non-copper layer from `User_1` on, signal for copper, else undefined.
    pub fn layer_type(&self, layer: PcbLayerId) -> LayerT {
        if self.is_layer_enabled(layer) {
            if let Some(l) = self.layers.get(&layer.0) {
                return l.type_;
            }
        }

        if layer.0 >= PcbLayerId::User1.0 && !is_copper_layer(layer) {
            LayerT::Aux
        } else if is_copper_layer(layer) {
            LayerT::Signal
        } else {
            LayerT::Undefined
        }
    }

    /// `BOARD::SetLayerType`: only for an enabled layer.
    pub fn set_layer_type(&mut self, layer: PcbLayerId, layer_type: LayerT) -> bool {
        if self.is_layer_enabled(layer) {
            self.layers.entry(layer.0).or_default().type_ = layer_type;
            self.layers_changed();
            return true;
        }

        false
    }

    /// `BOARD::SetLayerDescr`: store the `LAYER`, then `recalcOpposites()`.
    pub fn set_layer_descr(&mut self, layer: PcbLayerId, descr: Layer) -> bool {
        self.layers.insert(layer.0, descr);
        self.layers_changed();
        true
    }

    /// `recalcOpposites()` after an edit of `m_layers`, plus the Rust bookkeeping: the flip
    /// table feeds the `LayerEnv` snapshot (marked stale) and the time stamp moves on every
    /// edit (`board-model.md` 8.5).
    fn layers_changed(&mut self) {
        self.recalc_opposites();
        self.layer_env_stale = true;
        self.increment_time_stamp();
    }

    /// `BOARD::GetUserDefinedLayerCount`.
    pub fn user_defined_layer_count(&self) -> i32 {
        self.design_settings.user_defined_layer_count()
    }

    /// `BOARD::SetUserDefinedLayerCount`.
    pub fn set_user_defined_layer_count(&mut self, count: i32) {
        self.design_settings_mut()
            .set_user_defined_layer_count(count)
    }

    /// `BOARD::GetCopperLayerStackMaxId`: the id of the last copper layer of the stack in id
    /// order (`B_Cu` for two layers).
    pub fn copper_layer_stack_max_id(&self) -> PcbLayerId {
        let imax = self.copper_layer_count();

        // layers IDs are F_Cu, B_Cu, and even IDs values (imax values)
        if imax <= 2 {
            // at least 2 layers are expected
            return PcbLayerId::BCu;
        }

        // For a 4 layer, last ID is In2_Cu = 6 (IDs are 0, 2, 4, 6)
        PcbLayerId((imax - 1) * 2)
    }

    /// `BOARD::LayerDepth`: the number of stack positions between two copper layers.
    ///
    /// C++ throws `std::invalid_argument` (from `LAYER_RANGE`) for a non-copper layer; that is
    /// a programmer error, so it is a `debug_assert!` here and release builds return 0.
    pub fn layer_depth(&self, start: PcbLayerId, end: PcbLayerId) -> i32 {
        // Copper IDs are not in stack order (B_Cu=2, In1_Cu=4), so count in stack positions
        match LayerRange::new(start, end, self.copper_layer_count()) {
            Ok(range) => range.size() as i32 - 1,
            Err(_) => {
                debug_assert!(false, "BOARD::LayerDepth: {start:?} / {end:?} not copper");
                0
            }
        }
    }

    /// `BOARD::IsFrontLayer`: a front layer, or a user layer of type `LT_FRONT`.
    pub fn is_front_layer(&self, layer: PcbLayerId) -> bool {
        layer_ids::is_front_layer(layer) || self.layer_type(layer) == LayerT::Front
    }

    /// `BOARD::IsBackLayer`: a back layer, or a user layer of type `LT_BACK`.
    pub fn is_back_layer(&self, layer: PcbLayerId) -> bool {
        layer_ids::is_back_layer(layer) || self.layer_type(layer) == LayerT::Back
    }

    /// `BOARD::GetStackupOrDefault`: the settings' stack-up if the board has one, else a
    /// default list for the copper count.
    pub fn get_stackup_or_default(&self) -> BoardStackup {
        if self.design_settings.has_stackup {
            return self.design_settings.stackup.clone();
        }

        let mut stackup = BoardStackup::default();
        stackup.build_default_stackup_list(Some(&self.design_settings), self.copper_layer_count());
        stackup
    }
}

// ---- pcbnew/layer_utils.cpp (LAYER_UTILS) ---------------------------------------------------

/// `LAYER_UTILS::AccumulateNames`: the layer names joined by `", "` (the board's names when a
/// board is given, else the standard names).
pub fn accumulate_names(layers: &Lseq, board: Option<&Board>) -> String {
    let mut result = String::new();

    for &layer in &layers.0 {
        if !result.is_empty() {
            result += ", ";
        }

        match board {
            Some(board) => result += &board.layer_name(layer),
            None => result += &layer_name(layer),
        }
    }

    result
}

/// `LAYER_UTILS::GetAllFootprintLayers`: the union of the layer sets of every child of the
/// footprint (recursively).
pub fn get_all_footprint_layers(ctx: &BoardCtx<'_>, footprint: FootprintId) -> Lset {
    let mut used_layers = Lset::default();

    ctx.board
        .item_run_on_children(footprint.into(), RecurseMode::Recurse, &mut |child| {
            if let Some(item) = ctx.item(child) {
                used_layers |= &item.layer_set();
            }
        });

    used_layers
}

/// `LAYER_UTILS::GetUsedFootprintLayers`: like `get_all_footprint_layers`, but a pad on the
/// `*.Cu` wildcard pins only the copper layers its padstack defines explicitly.
pub fn get_used_footprint_layers(ctx: &BoardCtx<'_>, footprint: FootprintId) -> Lset {
    let mut used_layers = Lset::default();

    ctx.board
        .item_run_on_children(footprint.into(), RecurseMode::Recurse, &mut |child| {
            let Some(item) = ctx.item(child) else {
                return;
            };

            let mut item_layers = item.layer_set();

            // *.Cu wildcard pins no copper layer, an explicit padstack layer still does
            if let Some(pad) = item.as_pad() {
                let copper = &item_layers & Lset::all_cu_mask();

                // A blind span such as F.Cu..In2.Cu is a real layer choice, not the wildcard
                if copper.count() > 1 && copper == Lset::all_cu_mask_n(copper.count() as i32) {
                    for layer in copper.seq().0 {
                        if !pad.lib_padstack().has_explicit_definition_for_layer(layer) {
                            item_layers.reset(layer);
                        }
                    }
                }
            }

            used_layers |= &item_layers;
        });

    used_layers
}

/// `LAYER_UTILS::MinimalCopperLayerCount`: the smallest even copper count holding every inner
/// copper layer of `layers` (at least 2).
pub fn minimal_copper_layer_count(layers: &Lset) -> i32 {
    let inner_copper = layers & Lset::internal_cu_mask();
    let mut copper_count = 2;

    for layer in inner_copper.seq().0 {
        copper_count = copper_count.max(copper_layer_to_ordinal(layer) as i32 + 2);
    }

    // Stackups only come in even copper counts
    copper_count + copper_count % 2
}

/// `LAYER_UTILS::GetOrphanedFootprintLayers`: the used layers of the footprint that are neither
/// custom user layers, technical, user nor `Rescue`.
pub fn get_orphaned_footprint_layers(
    ctx: &BoardCtx<'_>,
    footprint: FootprintId,
    custom_user_layers: &Lset,
) -> Lset {
    let mut used_layers = get_used_footprint_layers(ctx, footprint);

    used_layers &= &!custom_user_layers;
    used_layers &= &!Lset::all_tech_mask();
    used_layers &= &!Lset::user_mask();

    // Rescue is a pseudo-layer used as a fallback for items referencing unknown layer
    // names at load time. It is not exposed in any layer-selection UI, so the user has no
    // way to "keep" it. Items on Rescue are an orphan state that predates any Footprint
    // Properties edit and are surfaced through library-parity DRC instead.
    used_layers.reset(PcbLayerId::Rescue);

    used_layers
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kicad_core::{EdaUnits, PageSizeType};

    use super::*;
    use crate::ids::{ItemKind, NetCode};
    use crate::test_support::rotated_flipped_scaled;

    /// `BOARD::BOARD()` (`pcbnew/board.cpp:99`): member initialisers, the `LAYER` table,
    /// `recalcOpposites()` and the board's default net class.
    #[test]
    fn board_constructor_defaults() {
        let board = Board::default();
        board.check_invariants().unwrap();

        assert_eq!(board.time_stamp(), 1);
        assert_eq!(board.paper.type_(), PageSizeType::A4);
        assert_eq!(board.user_units, EdaUnits::Mm);
        assert_eq!(board.outlines_chaining_epsilon, 10_000);
        assert_eq!(board.file_format_version_at_load, 2);
        assert_eq!(board.copper_layer_count(), 2);

        // LAYER table: every id, standard names, types, opposites from ::FlipLayer( 2 ).
        assert_eq!(board.layers.len(), PCB_LAYER_ID_COUNT as usize);
        let f_cu = &board.layers[&PcbLayerId::FCu.0];
        assert_eq!(f_cu.name, "F.Cu");
        assert_eq!(f_cu.type_, LayerT::Signal);
        assert_eq!(f_cu.number, 0);
        assert!(f_cu.visible);
        assert_eq!(board.layers[&PcbLayerId::User1.0].type_, LayerT::Aux);
        assert_eq!(board.layers[&PcbLayerId::FSilkS.0].type_, LayerT::Undefined);
        assert_eq!(board.flip_layer(PcbLayerId::FCu), PcbLayerId::BCu);
        assert_eq!(board.flip_layer(PcbLayerId::FSilkS), PcbLayerId::BSilkS);
        assert_eq!(board.flip_layer(PcbLayerId::In1Cu), PcbLayerId::In1Cu);
        assert_eq!(
            board.flip_layer(PcbLayerId::UndefinedLayer),
            PcbLayerId::UndefinedLayer
        );

        // The LayerEnv snapshot is derived from the settings (no longer "all layers").
        let env = board.build_layer_env();
        assert_eq!(*board.layer_env, env);
        assert_eq!(env.copper_layer_count, 2);
        assert!(!env.enabled_layers.contains(PcbLayerId::In1Cu));
        assert_eq!(env.flip_layer(PcbLayerId::BFab), PcbLayerId::FFab);

        // Net 0 keeps the NET_SETTINGS-constructor class; the settings get a new Default.
        let ns = &board.design_settings().net_settings;
        assert_eq!(ns.default_netclass.priority, -1);
        assert_eq!(
            ns.default_netclass.description,
            "This is the default net class."
        );
        let net0 = board.net_info().get_net_item(NetCode::UNCONNECTED).unwrap();
        assert_eq!(net0.netclass.as_ref().unwrap().priority, i32::MAX);
        assert_eq!(board.nets().count(), 1);
        assert!(!board.design_settings().use_custom_track_via_size());
        assert_eq!(board.component_class_manager().ticker(), 0);
        assert!(board.embedded_files().is_empty());
    }

    /// `BOARD::SetCopperLayerCount` re-runs `recalcOpposites()` and makes the environment of
    /// a footprint child see the new table.
    #[test]
    fn copper_layer_count_drives_flip_table() {
        let mut fx = rotated_flipped_scaled();
        let child = fx.child(ItemKind::Pad);
        let before = fx.board.time_stamp();

        fx.board.set_copper_layer_count(4);
        fx.board.check_invariants().unwrap();
        assert_ne!(fx.board.time_stamp(), before);
        assert_eq!(fx.board.copper_layer_count(), 4);
        assert_eq!(fx.board.flip_layer(PcbLayerId::In1Cu), PcbLayerId::In2Cu);
        assert!(fx.board.enabled_layers().contains(PcbLayerId::In2Cu));

        let env = fx.board.mut_env(child);
        assert_eq!(env.layers.copper_layer_count, 4);
        assert_eq!(env.layers.flip_layer(PcbLayerId::In2Cu), PcbLayerId::In1Cu);
        assert!(env.parent.is_some());

        let refreshed = fx.board.mut_env_refreshed(child).unwrap();
        assert!(Arc::ptr_eq(&refreshed.layers, &fx.board.layer_env));
        assert_eq!(fx.board.layer_env.copper_layer_count, 4);
        fx.board.check_invariants().unwrap();
    }

    /// `recalcOpposites()` pairs front / back user layers by the name after the first dot.
    #[test]
    fn recalc_opposites_pairs_user_layers() {
        let mut board = Board::default();
        let (a, b) = (PcbLayerId::User1.0, PcbLayerId::User1.0 + 4);
        for (id, t, name) in [(a, LayerT::Front, "F.Glue"), (b, LayerT::Back, "B.Glue")] {
            let l = board.layers.get_mut(&id).unwrap();
            l.type_ = t;
            l.user_name = name.to_string();
        }
        board.recalc_opposites();
        assert_eq!(board.flip_layer(PcbLayerId(a)), PcbLayerId(b));
        assert_eq!(board.flip_layer(PcbLayerId(b)), PcbLayerId(a));
        assert_eq!(after_first_dot("NoDot"), "");
        assert_eq!(LayerT::parse_type("auxiliary"), LayerT::Aux);
        assert_eq!(LayerT::Undefined.show_type(), "signal");
    }
}

/// Package M22 tests: the layer table queries of `BOARD` and `LAYER_UTILS`. Expected names and
/// types also come from `pcb-dump board --sections board` (the `board_section_oracle` test in
/// `board/queries.rs` replays those goldens).
#[cfg(test)]
mod m22_tests {
    use kicad_core::{Lseq, Lset, PcbLayerId};

    use super::*;

    #[test]
    fn layer_names_follow_enabled_state() {
        let mut board = Board::default();

        // Defaults: standard names (F.Silkscreen etc. are the standard names of LayerName).
        assert_eq!(board.layer_name(PcbLayerId::FCu), "F.Cu");
        assert_eq!(
            board.layer_name(PcbLayerId::FSilkS),
            layer_name(PcbLayerId::FSilkS)
        );

        // A disabled layer cannot be renamed and keeps its standard name.
        assert!(!board.is_layer_enabled(PcbLayerId::In1Cu));
        assert!(!board.set_layer_name(PcbLayerId::In1Cu, "Inner"));
        assert_eq!(board.layer_name(PcbLayerId::In1Cu), "In1.Cu");

        // Quotes are refused; a valid name is used and found by GetLayerID.
        assert!(!board.set_layer_name(PcbLayerId::FCu, "Top\"Side"));
        assert!(board.set_layer_name(PcbLayerId::FCu, "Top"));
        assert_eq!(board.layer_name(PcbLayerId::FCu), "Top");
        assert_eq!(board.layer_id("Top"), PcbLayerId::FCu);
        assert_eq!(board.layer_id("F.Cu"), PcbLayerId::FCu);
        assert_eq!(board.layer_id("B.Cu"), PcbLayerId::BCu);
        assert_eq!(board.layer_id("no such layer"), PcbLayerId::UndefinedLayer);

        // Clearing the name returns false (C++ quirk) but restores the standard name.
        assert!(!board.set_layer_name(PcbLayerId::FCu, ""));
        assert_eq!(board.layer_name(PcbLayerId::FCu), "F.Cu");
        board.check_invariants().unwrap();
    }

    #[test]
    fn layer_types_and_sides() {
        let mut board = Board::default();
        let user1 = PcbLayerId::User1;

        assert_eq!(board.layer_type(PcbLayerId::FCu), LayerT::Signal);
        assert_eq!(board.layer_type(PcbLayerId::In5Cu), LayerT::Signal); // disabled copper
        assert_eq!(board.layer_type(PcbLayerId::FSilkS), LayerT::Undefined);
        assert_eq!(board.layer_type(user1), LayerT::Aux);

        assert!(board.is_layer_enabled(user1));
        assert!(!board.is_front_layer(user1));
        assert!(board.set_layer_type(user1, LayerT::Front));
        assert!(board.is_front_layer(user1));
        assert!(!board.is_back_layer(user1));
        assert!(board.is_front_layer(PcbLayerId::FFab));
        assert!(board.is_back_layer(PcbLayerId::BCu));

        // Disabled layers cannot change type.
        assert!(!board.set_layer_type(PcbLayerId(PcbLayerId::User1.0 + 20), LayerT::Back));

        // SetLayerDescr re-runs recalcOpposites: a front / back pair of user layers.
        let user3 = PcbLayerId(user1.0 + 2);
        let mut descr = board.layers[&user3.0].clone();
        descr.type_ = LayerT::Back;
        assert!(board.set_layer_descr(user3, descr));
        assert_eq!(board.flip_layer(user1), user3);
        assert_eq!(board.build_layer_env().flip_layer(user1), user3);
        assert!(board.layer_env_stale);
    }

    #[test]
    fn copper_stack_queries() {
        let mut board = Board::default();
        assert_eq!(board.copper_layer_stack_max_id(), PcbLayerId::BCu);
        assert_eq!(board.layer_depth(PcbLayerId::FCu, PcbLayerId::BCu), 1);

        board.set_copper_layer_count(6);
        assert_eq!(board.copper_layer_stack_max_id(), PcbLayerId(10));
        assert_eq!(board.layer_depth(PcbLayerId::FCu, PcbLayerId::BCu), 5);
        assert_eq!(board.layer_depth(PcbLayerId::In1Cu, PcbLayerId::BCu), 4);
        assert_eq!(board.layer_depth(PcbLayerId::In2Cu, PcbLayerId::In1Cu), 1);

        // SetEnabledLayers recounts the copper layers and forces the mandatory ones.
        board.set_enabled_layers(Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]));
        assert_eq!(board.copper_layer_count(), 2);
        assert!(board.is_layer_enabled(PcbLayerId::EdgeCuts));
        assert!(!board.is_layer_enabled(PcbLayerId::In1Cu));
        assert_eq!(board.user_defined_layer_count(), 0);
        board.set_user_defined_layer_count(2);
        assert!(board.is_layer_enabled(PcbLayerId::User1));
    }

    #[test]
    fn stackup_or_default_without_stackup() {
        let board = Board::default();
        assert!(!board.design_settings().has_stackup);
        let stackup = board.get_stackup_or_default();
        let mut expected = BoardStackup::default();
        expected.build_default_stackup_list(Some(board.design_settings()), 2);
        assert_eq!(stackup.list.len(), expected.list.len());
    }

    #[test]
    fn layer_utils() {
        let board = Board::default();
        let seq = Lseq(vec![PcbLayerId::FCu, PcbLayerId::EdgeCuts]);
        assert_eq!(accumulate_names(&seq, Some(&board)), "F.Cu, Edge.Cuts");
        assert_eq!(accumulate_names(&seq, None), "F.Cu, Edge.Cuts");
        assert_eq!(accumulate_names(&Lseq(vec![]), None), "");

        assert_eq!(
            minimal_copper_layer_count(&Lset::from_layers(&[PcbLayerId::FCu])),
            2
        );
        assert_eq!(
            minimal_copper_layer_count(&Lset::from_layers(&[PcbLayerId::In1Cu])),
            4
        );
        assert_eq!(
            minimal_copper_layer_count(&Lset::from_layers(&[PcbLayerId::In2Cu])),
            4
        );
        assert_eq!(
            minimal_copper_layer_count(&Lset::from_layers(&[PcbLayerId::In3Cu])),
            6
        );
    }
}

/// Replays `tests/golden/m22_board_layers.txt`: the `layer.<L>.name` / `layer.<L>.type` and
/// `copper_layer_count` lines `pcb-dump board --sections board` printed for the boards named
/// in its `file|` lines (`tests/data/m22/` or `qa/data/pcbnew/`).
#[cfg(test)]
mod m22_oracle {
    use kicad_core::{Lset, PcbLayerId};

    use super::LayerT;
    use crate::board::Board;
    use crate::io::{ParseOptions, parse_board};

    /// `ENUM_TO_STRING( LAYER_T )` of the dump.
    fn cpp_name(t: LayerT) -> &'static str {
        match t {
            LayerT::Undefined => "LT_UNDEFINED",
            LayerT::Signal => "LT_SIGNAL",
            LayerT::Power => "LT_POWER",
            LayerT::Mixed => "LT_MIXED",
            LayerT::Jumper => "LT_JUMPER",
            LayerT::Aux => "LT_AUX",
            LayerT::Front => "LT_FRONT",
            LayerT::Back => "LT_BACK",
        }
    }

    fn load(name: &str) -> Board {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/m22")
            .join(format!("{name}.kicad_pcb"));
        let path = if local.exists() {
            local
        } else {
            kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"))
        };
        let text = std::fs::read_to_string(&path).unwrap();
        // Staging: constructs whose parser package has not landed are skipped; the layer table
        // is parsed by the board core (PCBIO-01).
        let options = ParseOptions {
            allow_pending: true,
            ..ParseOptions::default()
        };
        parse_board(&text, &path.to_string_lossy(), &options).unwrap()
    }

    #[test]
    fn board_layer_table_matches_reference() {
        let golden = include_str!("../../tests/golden/m22_board_layers.txt");
        let mut board: Option<Board> = None;
        let mut checked = 0;

        for line in golden.lines() {
            if line.starts_with('#') {
                continue;
            }

            if let Some(name) = line.strip_prefix("file|") {
                board = Some(load(name));
                continue;
            }

            let b = board.as_ref().unwrap();
            let (key, value) = line
                .strip_prefix("board|board|")
                .unwrap()
                .split_once('=')
                .unwrap();

            if key == "copper_layer_count" {
                assert_eq!(b.copper_layer_count().to_string(), value, "{line}");
            } else if let Some(rest) = key.strip_prefix("layer.") {
                let (layer, field) = rest.rsplit_once('.').unwrap();
                // The key is the canonical `LSET::Name`.
                let id = PcbLayerId(Lset::name_to_layer(layer));
                assert_ne!(id, PcbLayerId::UndefinedLayer, "{line}");

                match field {
                    "name" => assert_eq!(format!("\"{}\"", b.layer_name(id)), value, "{line}"),
                    "type" => assert_eq!(cpp_name(b.layer_type(id)), value, "{line}"),
                    _ => panic!("unexpected field {line}"),
                }
            } else {
                continue;
            }

            checked += 1;
        }

        assert!(checked > 200, "{checked}");
    }
}
