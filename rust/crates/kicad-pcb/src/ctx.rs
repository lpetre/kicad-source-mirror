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

//! Explicit context for item code (`board-model.md` sections 6.1, 7 and 8.2): `BoardCtx`
//! (what C++ reaches through `m_parent` / `GetBoard()` / `m_DRCEngine` / `GetConnectivity()`),
//! the upward hook traits `RuleResolver` and `ConnectivityQuery`, and the mutator snapshot
//! `MutEnv` / `ParentFrame` / `LayerEnv`.

use std::sync::Arc;

use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, flip_layer};
use kicad_core::{KicadT, Kiid, Lset, PcbLayerId};
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;
use kicad_geom::geometry::transform_trs::TransformTrs;

use crate::board::Board;
use crate::drc_api::{DrcConstraint, DrcConstraintT};
use crate::ids::{FootprintId, ItemId, NetCode};
use crate::item::BoardUse;
use crate::item_ref::{AnyItem, ItemRef};

/// Everything a read-only item method may reach besides `self` (C++ `GetBoard()`,
/// `bds.m_DRCEngine`, `GetBoard()->GetConnectivity()`). Cheap, `Copy`; passed as
/// `ctx: &BoardCtx<'_>`, always the first argument after `self`.
#[derive(Clone, Copy)]
pub struct BoardCtx<'a> {
    pub board: &'a Board,
    /// `None` reproduces the C++ `if( !m_DRCEngine )` branches (own clearance 0, mask expansion
    /// from item / footprint / board values, width constraint from the netclass, ...).
    pub rules: Option<&'a dyn RuleResolver>,
    /// `None` reproduces a board whose CONNECTIVITY_DATA is empty: conditionally flashed
    /// pads / vias are not connected on any layer (FlashLayer returns false where the C++ asks
    /// connectivity). Plotting copper and DRC must pass a real one. Note: an item whose parent
    /// chain does not reach the board (`ParentRef::None`) is "GetBoard() == nullptr" in C++ and
    /// takes the no-board branch (e.g. FlashLayer returns true) whatever the context holds.
    /// Connectivity's own `&mut self` methods (build, islands) necessarily run with `None`;
    /// that is equivalent because the C++ algorithm never calls FlashLayer from inside (it uses
    /// ConditionallyFlashed, the zone-layer overrides and ALWAYS_FLASHED shapes).
    pub connectivity: Option<&'a dyn ConnectivityQuery>,
}

impl<'a> BoardCtx<'a> {
    /// Context without hooks (parser, writer, simple tools).
    pub fn bare(board: &'a Board) -> Self {
        BoardCtx {
            board,
            rules: None,
            connectivity: None,
        }
    }

    /// The same context with a rule resolver (DRC engine).
    pub fn with_rules(self, r: &'a dyn RuleResolver) -> Self {
        BoardCtx {
            rules: Some(r),
            ..self
        }
    }

    /// The same context with a connectivity query.
    pub fn with_connectivity(self, c: &'a dyn ConnectivityQuery) -> Self {
        BoardCtx {
            connectivity: Some(c),
            ..self
        }
    }

    /// Handle -> `BOARD_ITEM*`; `None` for a stale handle.
    pub fn item(&'a self, id: ItemId) -> Option<ItemRef<'a>> {
        self.board.get(id).map(|item| ItemRef::new(self, item))
    }

    /// `BOARD::ResolveItem( aID )`; `None` for a nil or unknown UUID.
    pub fn resolve(&'a self, uuid: &Kiid) -> Option<ItemRef<'a>> {
        self.item(self.board.resolve_item(uuid)?)
    }

    /// `&item` -> `BOARD_ITEM*`: wraps any item reference. Used by item code to pass `this`
    /// (`ctx.view(AnyItem::Pad(self))`, identity from `base.id`) and by consumers for values
    /// outside the board (dummy pad of the filler, DRC helper shape: `id() == None`, but the
    /// `parent` still resolves, so a copy of a footprint pad keeps its board-frame geometry).
    pub fn view(&'a self, item: AnyItem<'a>) -> ItemRef<'a> {
        ItemRef::new(self, item)
    }

    /// `BOARD_ITEM::GetMaxError()` for an item on this board (`m_MaxError` of the design
    /// settings).
    pub fn max_error(&self) -> i32 {
        self.board.design_settings().max_error()
    }

    /// `BOARD_ITEM::BoardCopperLayerCount()` for an item on this board.
    pub fn copper_layer_count(&self) -> i32 {
        self.board.copper_layer_count()
    }

    /// `BOARD::FlipLayer( aLayer )`.
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId {
        self.board.flip_layer(layer)
    }
}

/// Implemented by `kicad_drc::DrcEngine`. `Sync` so that a `BoardCtx` can cross threads if the
/// optional `parallel` feature is ever added; all methods take `&self`. Engine caches use
/// `Mutex` and never hold a lock across an item call (`eval_rules` calls item methods, which
/// may call `cached_own_clearance`).
pub trait RuleResolver: Sync {
    /// `DRC_ENGINE::EvalRules( aConstraintType, a, b, aLayer )`.
    fn eval_rules(
        &self,
        ctx: &BoardCtx<'_>,
        constraint: DrcConstraintT,
        a: Option<ItemRef<'_>>,
        b: Option<ItemRef<'_>>,
        layer: PcbLayerId,
    ) -> DrcConstraint;

    /// `DRC_ENGINE::HasRulesForConstraintType`.
    fn has_rules_for_constraint_type(&self, constraint: DrcConstraintT) -> bool;

    /// `DRC_ENGINE::GetCachedOwnClearance`; (clearance, source text if asked).
    fn cached_own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>);

    /// `DRC_ENGINE::QueryWorstConstraint` (`BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue`).
    fn query_worst_constraint(&self, constraint: DrcConstraintT) -> Option<DrcConstraint>;
}

/// Implemented by `kicad_connectivity::ConnectivityData`.
pub trait ConnectivityQuery: Sync {
    /// `CONNECTIVITY_DATA::IsConnectedOnLayer` (`PAD::FlashLayer`, `PCB_VIA::FlashLayer`). The
    /// item passes itself (`ctx.view(AnyItem::Pad(self))`); an item without a handle (detached
    /// copy) has no entry in the item map, exactly like a C++ copy at another address: false.
    fn is_connected_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        types: &[KicadT],
    ) -> bool;

    /// `CONNECTIVITY_DATA::GetNodeCount` (`BOARD::GetNodesCount`, net statistics).
    fn node_count(&self, net: Option<NetCode>) -> u32;

    /// `CONNECTIVITY_DATA::GetPadCount`.
    fn pad_count(&self, net: Option<NetCode>) -> u32;
}

/// The board facts leaf mutators need (shared by every `MutEnv`): a snapshot of the design
/// settings and layers. `Board` marks it stale in `design_settings_mut()`, `set_layer_descr()`,
/// `set_copper_layer_count()`, `set_enabled_layers()`; the `&mut self` entry points
/// (`modify*`, cascades) re-cache it before use; `mut_env(&self)` / `child_env(&self)` build a
/// fresh uncached one while it is stale.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerEnv {
    pub copper_layer_count: i32,
    pub enabled_layers: Lset,
    /// `BOARD::FlipLayer` for every layer id (user-layer opposites included), indexed by layer
    /// id.
    pub flipped: Vec<PcbLayerId>,
}

impl LayerEnv {
    /// The C++ null-board answers: `BoardCopperLayerCount()` = 32, `BoardLayerSet()` =
    /// `LSET::AllLayersMask()`, `GetFlippedLayer()` = `::FlipLayer( aLayer )`. A board's own
    /// snapshot is always derived from its settings (`Board::build_layer_env`), also for a
    /// default-constructed board.
    pub fn no_board() -> Self {
        LayerEnv {
            copper_layer_count: 32,
            enabled_layers: Lset::all_layers_mask().clone(),
            flipped: (0..PCB_LAYER_ID_COUNT)
                .map(|l| flip_layer(PcbLayerId(l), 0))
                .collect(),
        }
    }

    /// The flipped layer of `layer` (`BOARD::FlipLayer`); layers outside the table are
    /// returned unchanged.
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId {
        usize::try_from(layer.0)
            .ok()
            .and_then(|i| self.flipped.get(i))
            .copied()
            .unwrap_or(layer)
    }
}

/// What a mutator of a footprint child reads from its parent footprint (frame of the parent).
#[derive(Clone, Copy, Debug)]
pub struct ParentFrame {
    pub footprint: FootprintId,
    pub transform: TransformTrs,
    pub layer: PcbLayerId,
    pub flipped: bool,
}

/// Snapshot of what C++ mutators read through `GetParentFootprint()` / `GetBoard()`.
#[derive(Clone, Debug)]
pub struct MutEnv {
    /// `None`: board-level (or detached) item.
    pub parent: Option<ParentFrame>,
    /// Copper count, enabled layers, `BOARD::FlipLayer` table.
    pub layers: Arc<LayerEnv>,
    pub max_error: i32,
    pub board_use: BoardUse,
}

impl MutEnv {
    /// No board: the C++ null-board defaults (`GetMaxError()` = `ARC_HIGH_DEF`).
    pub fn detached() -> MutEnv {
        MutEnv {
            parent: None,
            layers: Arc::new(LayerEnv::no_board()),
            max_error: ARC_HIGH_DEF,
            board_use: BoardUse::Normal,
        }
    }
}
