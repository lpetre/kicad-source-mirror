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

//! Shared item data: the data members of `EDA_ITEM` + `BOARD_ITEM` (`BoardItemBase`) and of
//! `BOARD_CONNECTED_ITEM` (`ConnectedItemBase`), `EDA_ITEM_FLAGS`, `m_parent` (`ParentRef`)
//! and small shared enums (`board-model-types.md` sections 0 and 1).
//!
//! Ports the data parts of `include/eda_item.h`, `include/eda_item_flags.h`,
//! `include/board_item.h` and `pcbnew/board_connected_item.h`; the behaviour of those classes
//! is in `eda_item.rs`, `board_item.rs` and `board_connected_item.rs`.

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::{Kiid, PcbLayerId};
use kicad_geom::Shape;

use crate::ids::{ItemId, NetCode};
use crate::teardrop_parameters::TeardropParameters;
use crate::traits::BoardItem;

/// `std::shared_ptr<SHAPE>`.
pub type SharedShape = Arc<dyn Shape>;

/// `EDA_ITEM_FLAGS` (include/eda_item_flags.h); same bit values as C++. Selection, brightening
/// and rollover bits are not ported.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ItemFlags(pub u32);

impl ItemFlags {
    pub const IS_CHANGED: u32 = 1 << 0;
    pub const IS_LINKED: u32 = 1 << 1;
    pub const IN_EDIT: u32 = 1 << 2;
    pub const IS_MOVING: u32 = 1 << 3;
    pub const IS_NEW: u32 = 1 << 4;
    pub const IS_BROKEN: u32 = 1 << 5;
    pub const IS_DELETED: u32 = 1 << 7;
    pub const STRUCT_DELETED: u32 = 1 << 13;
    pub const CANDIDATE: u32 = 1 << 14;
    pub const SKIP_STRUCT: u32 = 1 << 15;
    pub const IS_PASTED: u32 = 1 << 17;
    pub const COURTYARD_CONFLICT: u32 = 1 << 19;
    pub const MALFORMED_F_COURTYARD: u32 = 1 << 20;
    pub const MALFORMED_B_COURTYARD: u32 = 1 << 21;
    pub const ROUTER_TRANSIENT: u32 = 1 << 22;
    pub const CONNECTIVITY_CANDIDATE: u32 = 1 << 23;
    pub const HOLE_PROXY: u32 = 1 << 24;
    pub const UR_TRANSIENT: u32 = 1 << 28;

    /// The mask of `EDA_ITEM::GetEditFlags()`.
    pub const EDIT_FLAGS: u32 = Self::IS_NEW
        | Self::IS_PASTED
        | Self::IS_MOVING
        | Self::IS_BROKEN
        | Self::IS_CHANGED
        | Self::STRUCT_DELETED;

    /// `EDA_ITEM::HasFlag`.
    pub fn has(self, flag: u32) -> bool {
        self.0 & flag == flag
    }

    /// `EDA_ITEM::SetFlags`.
    pub fn set(&mut self, flags: u32) {
        self.0 |= flags;
    }

    /// `EDA_ITEM::ClearFlags`.
    pub fn clear(&mut self, flags: u32) {
        self.0 &= !flags;
    }
}

/// `RECURSE_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RecurseMode {
    Recurse,
    NoRecurse,
}

/// `BOARD_USE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum BoardUse {
    #[default]
    Normal,
    FpHolder,
}

/// `EDA_ITEM::m_parent`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ParentRef {
    /// Detached (builder, dummy): C++ `m_parent == nullptr`, `GetBoard() == nullptr`.
    #[default]
    None,
    /// Direct child of the board.
    Board,
    /// Child of another item: footprint (pads, fields, graphics, zones, groups, points,
    /// constraints, bodies), table (cells), pad (inline custom-shape primitives), barcode
    /// (its text), generator (template items), marker (path shapes).
    Item(ItemId),
}

impl ParentRef {
    /// The parent item's handle, if the parent is an item.
    pub fn item(self) -> Option<ItemId> {
        match self {
            ParentRef::Item(id) => Some(id),
            _ => None,
        }
    }
}

/// Data members of `EDA_ITEM` + `BOARD_ITEM`. `Default` = the `BOARD_ITEM` constructor
/// (`m_layer( F_Cu )`, not knocked out, not locked) with a nil UUID.
#[derive(Clone, Debug)]
pub struct BoardItemBase {
    /// The item's own handle (`this`): set by `Board` on insertion and re-stamped after every
    /// `modify_*`; `None` for a value outside an arena. `Clone` keeps it; `detached_copy()`
    /// (the C++ copy constructor) clears it. Read with `base.id()`.
    pub(crate) id: Option<ItemId>,
    pub uuid: Kiid,                                  // EDA_ITEM::m_Uuid
    pub parent: ParentRef,                           // EDA_ITEM::m_parent
    pub group: Option<ItemId>,                       // EDA_ITEM::m_group (Group or Generator)
    pub flags: ItemFlags,                            // EDA_ITEM::m_flags
    pub custom_properties: BTreeMap<String, String>, // EDA_ITEM::m_customProperties
    pub layer: PcbLayerId,                           // BOARD_ITEM::m_layer
    pub is_knockout: bool,                           // BOARD_ITEM::m_isKnockout
    pub is_locked: bool,                             // BOARD_ITEM::m_isLocked
}

impl Default for BoardItemBase {
    fn default() -> Self {
        BoardItemBase::with_layer(PcbLayerId::FCu)
    }
}

impl BoardItemBase {
    /// `BOARD_ITEM( aParent, idtype, aLayer )` with a nil UUID and no parent.
    pub fn with_layer(layer: PcbLayerId) -> Self {
        BoardItemBase {
            id: None,
            uuid: Kiid::default(),
            parent: ParentRef::None,
            group: None,
            flags: ItemFlags::default(),
            custom_properties: BTreeMap::new(),
            layer,
            is_knockout: false,
            is_locked: false,
        }
    }

    /// The item's own handle (`None` outside an arena).
    pub fn id(&self) -> Option<ItemId> {
        self.id
    }
}

/// Data members of `BOARD_CONNECTED_ITEM`. `Default` = its constructor: net code 0
/// (`NETINFO_LIST::OrphanedItem()`), default teardrop parameters, local ratsnest visible.
#[derive(Clone, Debug)]
pub struct ConnectedItemBase {
    pub item: BoardItemBase,
    pub net: NetCode,                        // m_netinfo
    pub teardrop_params: TeardropParameters, // m_teardropParams
    pub local_ratsnest_visible: bool,        // m_localRatsnestVisible
}

impl Default for ConnectedItemBase {
    fn default() -> Self {
        ConnectedItemBase {
            item: BoardItemBase::default(),
            net: NetCode::UNCONNECTED,
            teardrop_params: TeardropParameters::default(),
            local_ratsnest_visible: true,
        }
    }
}

/// The C++ copy constructor (`PAD dummy( *pad )`): a *different* object with the same UUID and
/// parent. Never use a bare `clone()` for a dummy: it keeps `base.id`, and connectivity would
/// treat it as the original (`board-model.md` 3).
pub trait DetachedCopy: BoardItem + Clone {
    /// Clone with `base.id = None`.
    fn detached_copy(&self) -> Self {
        let mut copy = self.clone();
        copy.base_mut().id = None;
        copy
    }
}

impl<T: BoardItem + Clone> DetachedCopy for T {}
