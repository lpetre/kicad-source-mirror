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

//! Port of the DRC run-time caches of `pcbnew/board.h` (`BOARD` members `m_DRCZones`,
//! `m_DRCCopperZones`, `m_DRCCopperZonesByLayer`, `m_DRCMaxClearance`,
//! `m_DRCMaxPhysicalClearance`, `m_CopperItemRTreeCache`, `m_CopperZoneRTreeCache` /
//! `GetCopperZoneRTree`, `m_ZoneIsolatedIslandsMap`, `m_SolderMaskBridges`, and the predicate
//! memo caches `m_Intersects*Cache`, `m_EnclosedByArea*Cache`, `m_ItemFieldCache`) and of the
//! part of `BOARD::IncrementTimeStamp()` (`pcbnew/board.cpp`) that clears them.
//!
//! In the Rust design these caches do not live in the board (`rust/docs/design/board-model.md`
//! sections 9 and 17): [`DrcBoardCaches`] is created per DRC run by the cache generator
//! ([`crate::cache_generator`]) and handed to the providers; a fresh value is the cleared C++
//! state. The predicate memo caches are written from inside rule evaluation and by the disallow
//! provider, so they are a separate value ([`DrcPredicateCaches`]) owned by whoever evaluates
//! rules. Caches that model code itself uses (`m_maxClearanceValue`, `m_ZonesByNameCache`,
//! `m_DeflatedZoneOutlineCache`, `m_StackedMicroviaCache`, `m_footprintCourtyardIndex`,
//! `m_LayerExpressionCache`, the board outline) are `kicad_pcb::Board` caches; the zone
//! bounding-box cache (`m_ZoneBBoxCache`) is the per-zone cache of `kicad_pcb::zone::Zone`
//! (`Zone::cache_bounding_box`, read through `bounding_box`).

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard};

use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_pcb::ctx::MutEnv;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::zone::{IsolatedIslands, Zone};
use kicad_pcb::zone_settings::ZoneBorderDisplayStyle;
use kicad_pcb::{Board, ItemId};

use crate::rtree::DrcRtree;

/// `std::map<ZONE*, std::map<PCB_LAYER_ID, ISOLATED_ISLANDS>> m_ZoneIsolatedIslandsMap`.
///
/// The C++ map is ordered by zone address and iterated by the connectivity provider (the order
/// of `isolated_copper` reports). `rust/docs/analysis/drc.md` 4.12 replaces the address order by
/// the board zone order, which is the insertion order of the cache generator; the map therefore
/// keeps its entries in insertion order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneIslandsMap {
    entries: Vec<(ZoneId, BTreeMap<PcbLayerId, IsolatedIslands>)>,
}

impl ZoneIslandsMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// `size()`: the number of zones.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `empty()`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `map[ zone ]`: the layer map of `zone`, inserted (at the end) when absent.
    pub fn entry(&mut self, zone: ZoneId) -> &mut BTreeMap<PcbLayerId, IsolatedIslands> {
        let pos = match self.entries.iter().position(|(z, _)| *z == zone) {
            Some(pos) => pos,
            None => {
                self.entries.push((zone, BTreeMap::new()));
                self.entries.len() - 1
            }
        };

        &mut self.entries[pos].1
    }

    /// `find( zone )`.
    pub fn get(&self, zone: ZoneId) -> Option<&BTreeMap<PcbLayerId, IsolatedIslands>> {
        self.entries
            .iter()
            .find(|(z, _)| *z == zone)
            .map(|(_, m)| m)
    }

    /// `find( zone )` for writing (`CONNECTIVITY_DATA::FillIsolatedIslandsMap`).
    pub fn get_mut(&mut self, zone: ZoneId) -> Option<&mut BTreeMap<PcbLayerId, IsolatedIslands>> {
        self.entries
            .iter_mut()
            .find(|(z, _)| *z == zone)
            .map(|(_, m)| m)
    }

    /// Iteration in the order of the map (see the type comment).
    pub fn iter(&self) -> impl Iterator<Item = (ZoneId, &BTreeMap<PcbLayerId, IsolatedIslands>)> {
        self.entries.iter().map(|(z, m)| (*z, m))
    }

    /// Iteration for writing, in the order of the map.
    pub fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (ZoneId, &mut BTreeMap<PcbLayerId, IsolatedIslands>)> {
        self.entries.iter_mut().map(|(z, m)| (*z, m))
    }
}

/// The DRC run-time caches of `BOARD`, filled by `DRC_CACHE_GENERATOR::Run()` and read by the
/// providers. A default value is the state `BOARD::IncrementTimeStamp()` leaves behind
/// (everything empty, both clearances 0, no copper item tree).
///
/// `Sync`: the providers only read it (the solder mask scratch zone has its own lock).
#[derive(Debug, Default)]
pub struct DrcBoardCaches {
    /// The board time stamp the caches were built at (`Board::time_stamp()` after the cache
    /// generator ran). Not a C++ member: the C++ caches are cleared by `IncrementTimeStamp()`;
    /// here a consumer compares the stamp instead (`board-model.md` 8.5).
    pub time_stamp: i32,
    /// `m_DRCMaxClearance`: the largest clearance of the board (broad-phase radius).
    pub max_clearance: i32,
    /// `m_DRCMaxPhysicalClearance`.
    pub max_physical_clearance: i32,
    /// `m_DRCZones`: the zones that are not rule areas; board zones, then footprint zones in
    /// footprint order.
    pub zones: Vec<ZoneId>,
    /// `m_DRCCopperZones`: those of [`DrcBoardCaches::zones`] on a copper layer of the board.
    pub copper_zones: Vec<ZoneId>,
    /// `m_DRCCopperZonesByLayer`: [`DrcBoardCaches::copper_zones`] per board copper layer, in
    /// the same order.
    pub copper_zones_by_layer: BTreeMap<PcbLayerId, Vec<ZoneId>>,
    /// `m_CopperItemRTreeCache` (`std::shared_ptr<DRC_RTREE>`; `None` = null pointer).
    pub copper_item_rtree: Option<Arc<DrcRtree>>,
    /// `m_CopperZoneRTreeCache`. The C++ `unordered_map` is iterated by the connection width
    /// provider; the port iterates in `ZoneId` order (`drc.md` 4.12, board-model.md 3.1 rule 2).
    pub copper_zone_rtrees: BTreeMap<ZoneId, DrcRtree>,
    /// `m_ZoneIsolatedIslandsMap`.
    pub zone_isolated_islands: ZoneIslandsMap,
    /// `m_SolderMaskBridges`: the scratch zone the solder mask provider builds the board's mask
    /// in (created lazily, see [`DrcBoardCaches::solder_mask_bridges`]).
    solder_mask_bridges: Mutex<Option<Zone>>,
}

impl DrcBoardCaches {
    /// The cleared state (`BOARD::IncrementTimeStamp()`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of the DRC part of `BOARD::IncrementTimeStamp()`: every cache emptied, both
    /// clearances 0, no copper item tree. `m_SolderMaskBridges` is a board member that the
    /// time stamp does not touch; it is kept.
    pub fn clear(&mut self) {
        self.time_stamp = 0;
        self.max_clearance = 0;
        self.max_physical_clearance = 0;
        self.zones.clear();
        self.copper_zones.clear();
        self.copper_zones_by_layer.clear();
        self.zone_isolated_islands.clear();
        self.copper_zone_rtrees.clear();
        self.copper_item_rtree = None;
    }

    /// Whether the caches were built for the current state of `board` (its time stamp did not
    /// move since).
    pub fn is_current(&self, board: &Board) -> bool {
        self.copper_item_rtree.is_some() && self.time_stamp == board.time_stamp()
    }

    /// `m_CopperItemRTreeCache`: the copper item tree. Panics when the cache generator has not
    /// run (C++: null pointer dereference).
    pub fn copper_item_rtree(&self) -> &DrcRtree {
        self.copper_item_rtree
            .as_deref()
            .expect("m_CopperItemRTreeCache used before DRC_CACHE_GENERATOR::Run")
    }

    /// Port of `BOARD::GetCopperZoneRTree( aZone )`: the filled-copper tree of a copper zone,
    /// `None` when the zone has none.
    pub fn copper_zone_rtree(&self, zone: ZoneId) -> Option<&DrcRtree> {
        self.copper_zone_rtrees.get(&zone)
    }

    /// `m_DRCCopperZonesByLayer[ aLayer ]` read without inserting (empty for a layer without
    /// copper zones).
    pub fn copper_zones_on_layer(&self, layer: PcbLayerId) -> &[ZoneId] {
        self.copper_zones_by_layer
            .get(&layer)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// `m_ZoneIsolatedIslandsMap.find( aZone )`.
    pub fn zone_islands(&self, zone: ZoneId) -> Option<&BTreeMap<PcbLayerId, IsolatedIslands>> {
        self.zone_isolated_islands.get(zone)
    }

    /// `m_SolderMaskBridges`: the scratch zone of the solder mask provider, created on first
    /// use as the `BOARD` constructor creates it. The lock is held while the guard lives.
    pub fn solder_mask_bridges(&self) -> MutexGuard<'_, Option<Zone>> {
        let mut guard = self
            .solder_mask_bridges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if guard.is_none() {
            *guard = Some(new_solder_mask_bridges());
        }

        guard
    }
}

/// The `m_SolderMaskBridges` zone of the `BOARD` constructor (`pcbnew/board.cpp`): invisible
/// border, layers F_Mask and B_Mask, an outline covering (almost) the whole coordinate range,
/// minimum thickness 0. `ZONE( this )` on a board under construction exports the default
/// `ZONE_SETTINGS`, which is what `Zone::default()` holds.
pub fn new_solder_mask_bridges() -> Zone {
    let env = MutEnv::detached();
    let mut zone = Zone::default();

    // SetHatchStyle( ZONE_BORDER_DISPLAY_STYLE::INVISIBLE_BORDER )
    zone.border_style = ZoneBorderDisplayStyle::InvisibleBorder;
    zone.set_layer_set(
        &env,
        Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]),
    );

    let infinity = (i32::MAX / 2) - PCB_IU_SCALE.mm_to_iu(1.0);
    let outline = zone.outline_mut(&env);
    outline.new_outline();
    outline.append(-infinity, -infinity, -1, -1, false);
    outline.append(-infinity, infinity, -1, -1, false);
    outline.append(infinity, infinity, -1, -1, false);
    outline.append(infinity, -infinity, -1, -1, false);
    zone.set_min_thickness(0);

    zone
}

/// `SHARDED_CACHE<KEY, VALUE>` (`include/core/sharded_cache.h`): a memo map that many DRC
/// worker threads read and write. The port runs sequentially; one lock is enough. Lookups only
/// (never iterated), so a `HashMap` is fine (board-model.md 3.1 rule 3).
#[derive(Debug)]
pub struct MemoCache<K, V> {
    map: Mutex<HashMap<K, V>>,
}

impl<K, V> Default for MemoCache<K, V> {
    fn default() -> Self {
        MemoCache {
            map: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash, V: Clone> MemoCache<K, V> {
    fn lock(&self) -> MutexGuard<'_, HashMap<K, V>> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `Get( aKey, aValue )`: the cached value, if any.
    pub fn get(&self, key: &K) -> Option<V> {
        self.lock().get(key).cloned()
    }

    /// `Set( aKey, aValue )`.
    pub fn set(&self, key: K, value: V) {
        self.lock().insert(key, value);
    }

    /// `Clear()`.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// `Empty()`.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

/// `PTR_PTR_CACHE_KEY` (`board.h`): (A, B).
pub type PtrPtrCacheKey = (ItemId, ItemId);

/// `PTR_PTR_LAYER_CACHE_KEY` (`board.h`): (A, B, layer).
pub type PtrPtrLayerCacheKey = (ItemId, ItemId, PcbLayerId);

/// `ITEM_SELECTOR_LAYER_CACHE_KEY` (`board.h`): the item, the selector argument (compared
/// verbatim), the layer and the constraint type (as `int`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemSelectorLayerCacheKey {
    /// `A`
    pub item: ItemId,
    /// `Selector`
    pub selector: String,
    /// `Layer`
    pub layer: PcbLayerId,
    /// `Constraint`
    pub constraint: i32,
}

/// `ITEM_FIELD_CACHE_KEY` (`board.h`). C++ keys by `std::hash` of the field name; the port
/// keys by the name itself (same lookups, without hash collisions).
pub type ItemFieldCacheKey = (ItemId, String);

/// The predicate memo caches of `BOARD` (`board.h`, "Run-time caches"), written by the PCB
/// expression functions (`pcbexpr_functions.cpp`) during rule evaluation and pre-filled by the
/// disallow provider. All cleared by `BOARD::IncrementTimeStamp()` ([`DrcPredicateCaches::clear`]).
/// Keys use item handles instead of pointers.
#[derive(Debug, Default)]
pub struct DrcPredicateCaches {
    /// `m_IntersectsCourtyardCache`
    pub intersects_courtyard: MemoCache<PtrPtrCacheKey, bool>,
    /// `m_IntersectsFCourtyardCache`
    pub intersects_f_courtyard: MemoCache<PtrPtrCacheKey, bool>,
    /// `m_IntersectsBCourtyardCache`
    pub intersects_b_courtyard: MemoCache<PtrPtrCacheKey, bool>,
    /// `m_IntersectsAreaCache`
    pub intersects_area: MemoCache<PtrPtrLayerCacheKey, bool>,
    /// `m_IntersectsKeepoutCache`
    pub intersects_keepout: MemoCache<PtrPtrLayerCacheKey, bool>,
    /// `m_EnclosedByAreaCache`
    pub enclosed_by_area: MemoCache<PtrPtrLayerCacheKey, bool>,
    /// `m_IntersectsCourtyardResultCache`
    pub intersects_courtyard_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_IntersectsFCourtyardResultCache`
    pub intersects_f_courtyard_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_IntersectsBCourtyardResultCache`
    pub intersects_b_courtyard_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_IntersectsAreaResultCache`
    pub intersects_area_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_IntersectsKeepoutResultCache`
    pub intersects_keepout_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_EnclosedByAreaResultCache`
    pub enclosed_by_area_result: MemoCache<ItemSelectorLayerCacheKey, bool>,
    /// `m_ItemFieldCache`
    pub item_field: MemoCache<ItemFieldCacheKey, String>,
}

impl DrcPredicateCaches {
    /// An empty set of caches.
    pub fn new() -> Self {
        Self::default()
    }

    /// The predicate part of `BOARD::IncrementTimeStamp()`: every cache cleared.
    pub fn clear(&self) {
        self.intersects_area.clear();
        self.intersects_keepout.clear();
        self.enclosed_by_area.clear();
        self.intersects_courtyard.clear();
        self.intersects_f_courtyard.clear();
        self.intersects_b_courtyard.clear();
        self.intersects_courtyard_result.clear();
        self.intersects_f_courtyard_result.clear();
        self.intersects_b_courtyard_result.clear();
        self.intersects_area_result.clear();
        self.intersects_keepout_result.clear();
        self.enclosed_by_area_result.clear();
        self.item_field.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::math::vector2::Vector2I;

    fn assert_sync<T: Sync + Send>() {}

    #[test]
    fn caches_are_sync() {
        assert_sync::<DrcBoardCaches>();
        assert_sync::<DrcPredicateCaches>();
    }

    #[test]
    fn solder_mask_bridges_like_board_constructor() {
        let caches = DrcBoardCaches::new();
        let guard = caches.solder_mask_bridges();
        let zone = guard.as_ref().unwrap();
        let infinity = (i32::MAX / 2) - 1_000_000;

        assert_eq!(zone.border_style, ZoneBorderDisplayStyle::InvisibleBorder);
        assert_eq!(zone.outline().total_vertices(), 4);
        assert_eq!(
            zone.outline().c_vertex_global(0),
            Vector2I::new(-infinity, -infinity)
        );
        assert_eq!(
            zone.outline().c_vertex_global(2),
            Vector2I::new(infinity, infinity)
        );
    }
}
