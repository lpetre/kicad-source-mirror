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

//! Port of `pcbnew/board.h` (`BOARD`): the struct, typed access, the order-list iterators in
//! C++ container order (`board-model.md` 10.1), the time stamp, `MutEnv` construction and
//! `check_invariants` (`board-model-types.md` section 14). The rest of `board.cpp` is split
//! over the sibling modules named in `board-model.md` 11.

pub mod compare;
pub mod containers;
pub mod index;
pub mod journal;
pub mod layers;
pub mod markers;
pub mod nets;
pub mod outline;
pub mod queries;
pub mod text_vars;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use kicad_core::{
    Color4D, EdaUnits, Kiid, KiidGenerator, PCB_IU_SCALE, PageInfo, PageSizeType, TitleBlock,
};
use kicad_geom::ShapePolySet;

use crate::arena::Arena;
use crate::board_design_settings::BoardDesignSettings;
use crate::cache::KeyedCache;
use crate::component_class::ComponentClassManager;
use crate::ctx::{LayerEnv, MutEnv, ParentFrame};
use crate::embedded_files::EmbeddedFiles;
use crate::footprint::Footprint;
use crate::footprint_courtyard_index::FootprintCourtyardIndex;
use crate::ids::{
    ArenaItem, ConstraintId, FootprintId, GeneratorId, GroupId, Id, ItemId, ItemKind, MarkerId,
    NetCode, PointId, TrackId, ZoneId,
};
use crate::item::{BoardItemBase, BoardUse, ParentRef};
use crate::item_ref::{AnyItem, AnyItemMut};
use crate::netclass::{NETCLASS_DEFAULT_NAME, Netclass};
use crate::netinfo::NetinfoList;
use crate::pcb_plot_params::PcbPlotParams;
use crate::traits::{BoardItem, Invalidate};

pub use containers::{AddMode, RemoveMode};
pub use journal::{Change, ChangeJournal, ChangeKind};
pub use layers::{Layer, LayerT};

/// Typed access to the arena of `T` (generated per kind by `board_items!`).
pub(crate) trait ArenaOf<T> {
    #[allow(dead_code)] // for the container packages (M1)
    fn arena(&self) -> &Arena<T>;
    fn arena_mut(&mut self) -> &mut Arena<T>;
}

/// `DEFAULT_CHAINING_EPSILON_MM` (`pcbnew/board.h`).
const DEFAULT_CHAINING_EPSILON_MM: f64 = 0.01;

/// `LEGACY_BOARD_FILE_VERSION` (`cmake/config.h.cmake`: 2; `BOARD::BOARD()` stores it).
const LEGACY_BOARD_FILE_VERSION: i32 = 2;

board_items!(gen_board, {
    /// `BOARD`. `Clone` copies every item (caches and journal start empty in the clone; every
    /// handle stays valid in it); `Sync`. C++ forbids copying a BOARD.
    #[derive(Clone)]
    #[allow(dead_code)] // `layers`: read by M22
    pub struct Board {
        // ---- board-level order lists (C++ member containers, C++ element order) ----
        pub(crate) marker_list: Vec<MarkerId>,     // m_markers
        pub(crate) drawing_list: VecDeque<ItemId>, // m_drawings
        pub(crate) footprint_list: VecDeque<FootprintId>, // m_footprints
        pub(crate) track_list: VecDeque<TrackId>,  // m_tracks
        pub(crate) group_list: Vec<GroupId>,       // m_groups
        pub(crate) constraint_list: Vec<ConstraintId>, // m_constraints
        pub(crate) zone_list: Vec<ZoneId>,         // m_zones
        pub(crate) generator_list: Vec<GeneratorId>, // m_generators
        pub(crate) point_list: Vec<PointId>,       // m_points
        /// m_itemIndex (lookup only, never iterated).
        pub(crate) uuid_index: HashMap<Kiid, ItemId>,

        // ---- nets ----
        pub(crate) net_info: NetinfoList,                // m_NetInfo
        pub net_chain_colors: BTreeMap<String, Color4D>, // m_netChainColors

        // ---- settings and metadata ----
        /// m_designSettings. Behind `design_settings()` / `design_settings_mut()`: the `_mut`
        /// form bumps the time stamp and marks `layer_env` stale.
        pub(crate) design_settings: BoardDesignSettings,
        pub(crate) layers: BTreeMap<i32, Layer>,  // m_layers
        pub paper: PageInfo,                      // m_paper
        pub titles: TitleBlock,                   // m_titles
        pub plot_options: PcbPlotParams,          // m_plotOptions
        pub properties: BTreeMap<String, String>, // m_properties
        pub current_variant: String,              // m_currentVariant
        pub variant_names: Vec<String>,           // m_variantNames
        pub variant_descriptions: BTreeMap<String, String>, // m_variantDescriptions
        pub embedded_files: EmbeddedFiles,        // EMBEDDED_FILES base + m_embedFonts
        pub component_class_manager: ComponentClassManager, // m_componentClassManager
        pub file_name: String,                    // m_fileName
        pub generator: String,                    // m_generator
        pub file_format_version_at_load: i32,     // m_fileFormatVersionAtLoad
        pub board_use: BoardUse,                  // m_boardUse
        pub user_units: EdaUnits,                 // m_userUnits
        pub outlines_chaining_epsilon: i32,       // m_outlinesChainingEpsilon
        pub legacy_teardrops: bool,               // m_legacyTeardrops
        pub legacy_design_settings_loaded: bool,  // m_LegacyDesignSettingsLoaded
        pub legacy_copper_edge_clearance_loaded: bool, // m_LegacyCopperEdgeClearanceLoaded
        pub legacy_netclasses_loaded: bool,       // m_LegacyNetclassesLoaded
        /// What C++ reaches through `m_project`: project text variables and the few local
        /// settings the core reads. `None` = "no project" (SynchronizeNetsAndNetClasses
        /// returns early).
        pub project: Option<BoardProject>,
        /// Host-provided clock for `CURRENT_DATE` / `CURRENT_TIME_*` (C++ reads the wall
        /// clock; library code has none). Empty by default.
        pub text_var_clock: kicad_core::title_block::TitleBlockClock,

        // ---- bookkeeping ----
        pub(crate) time_stamp: i32,          // m_timeStamp
        pub(crate) layer_env: Arc<LayerEnv>, // snapshot for MutEnv (board-model.md 8.2)
        pub(crate) layer_env_stale: bool,
        pub(crate) journal: ChangeJournal, // optional, off by default (board-model.md 8.5)
        /// `kicad_core::KiidGenerator` (seedable MT19937, `Clone`): source of UUIDs for new
        /// items; deterministic by default, hosts reseed it.
        pub kiid_generator: KiidGenerator,
        pub(crate) caches: BoardCaches,
    }
});

/// What C++ reaches through `BOARD::m_project`. Filled by `Board::set_project`
/// (`project/board_project.rs`, package SET-5) from a `project::Project`; the project's
/// `NET_SETTINGS` move into `design_settings.net_settings`.
#[derive(Clone, Debug)]
pub struct BoardProject {
    pub text_vars: BTreeMap<String, String>, // PROJECT::GetTextVars()
    pub prototype_zone_fill: bool,           // PROJECT_LOCAL_SETTINGS::m_PrototypeZoneFill
    pub design_rules_path: String,           // BOARD::GetDesignRulesPath()
    pub full_name: String,                   // PROJECT::GetProjectFullName()
    pub project_name: String,                // PROJECT::GetProjectName() (PROJECTNAME)
    pub visible_layers: kicad_core::Lset,    // PROJECT_LOCAL_SETTINGS::m_VisibleLayers
    pub board_drawing_sheet_file: String,    // PROJECT_FILE::m_BoardDrawingSheetFile
    /// PROJECT_FILE's JSON document (`m_internals`; parent of `board.design_settings`).
    pub document: kicad_core::json_settings::JsonSettingsInternals,
    pub file_synced: bool, // PROJECT_FILE::LoadFromFile result
}

impl Default for BoardProject {
    /// An empty project (no `.kicad_pro`, no `.kicad_prl`): every layer visible.
    fn default() -> Self {
        BoardProject {
            text_vars: BTreeMap::new(),
            prototype_zone_fill: false,
            design_rules_path: String::new(),
            full_name: String::new(),
            project_name: String::new(),
            visible_layers: kicad_core::Lset::all_layers_mask().clone(),
            board_drawing_sheet_file: String::new(),
            document: Default::default(),
            file_synced: false,
        }
    }
}

/// Run-time caches of BOARD that pcbnew code outside DRC uses. All cleared by
/// `increment_time_stamp()`; `Clone` gives empty caches.
#[derive(Clone, Default)]
#[allow(dead_code)] // filled by the packages that port the cached BOARD queries
pub(crate) struct BoardCaches {
    pub max_clearance_value: KeyedCache<i32>, // m_maxClearanceValue
    pub zones_by_name: KeyedCache<BTreeMap<String, Vec<ZoneId>>>, // m_ZonesByNameCache
    pub deflated_zone_outlines: KeyedCache<BTreeMap<ZoneId, Arc<ShapePolySet>>>, // m_DeflatedZoneOutlineCache
    pub stacked_microvias: KeyedCache<BTreeSet<TrackId>>, // m_StackedMicroviaCache
    pub footprint_courtyard_index: KeyedCache<Arc<FootprintCourtyardIndex>>, // m_footprintCourtyardIndex
    pub layer_expressions: KeyedCache<BTreeMap<String, kicad_core::Lset>>, // m_LayerExpressionCache
    pub board_outline: KeyedCache<Arc<ShapePolySet>>, // m_boardOutline + generation
}

impl Default for Board {
    /// `BOARD::BOARD()` (`pcbnew/board.cpp:99`): the member initialisers (A4 paper, mm,
    /// time stamp 1, the `BOARD_DESIGN_SETTINGS` and `NETINFO_LIST` constructors), the `LAYER`
    /// table with `recalcOpposites()`, the board's own `Default` net class, and the
    /// `LayerEnv` snapshot derived from all of that.
    fn default() -> Self {
        let mut board = Board {
            footprints: Arena::new(),
            pads: Arena::new(),
            tracks: Arena::new(),
            zones: Arena::new(),
            shapes: Arena::new(),
            texts: Arena::new(),
            fields: Arena::new(),
            textboxes: Arena::new(),
            tables: Arena::new(),
            tablecells: Arena::new(),
            dimensions: Arena::new(),
            barcodes: Arena::new(),
            images: Arena::new(),
            targets: Arena::new(),
            points: Arena::new(),
            groups: Arena::new(),
            generators: Arena::new(),
            markers: Arena::new(),
            constraints: Arena::new(),
            bodies: Arena::new(),
            grid_items: Arena::new(),
            drill_maps: Arena::new(),
            marker_list: Vec::new(),
            drawing_list: VecDeque::new(),
            footprint_list: VecDeque::new(),
            track_list: VecDeque::new(),
            group_list: Vec::new(),
            constraint_list: Vec::new(),
            zone_list: Vec::new(),
            generator_list: Vec::new(),
            point_list: Vec::new(),
            uuid_index: HashMap::new(),
            net_info: NetinfoList::default(),
            net_chain_colors: BTreeMap::new(),
            design_settings: BoardDesignSettings::default(),
            layers: BTreeMap::new(),
            paper: PageInfo::new(
                PageSizeType::A4,
                false,
                &kicad_core::page_info::CustomPageSize::default(),
            ),
            titles: TitleBlock::default(),
            plot_options: PcbPlotParams::default(),
            properties: BTreeMap::new(),
            current_variant: String::new(),
            variant_names: Vec::new(),
            variant_descriptions: BTreeMap::new(),
            embedded_files: EmbeddedFiles::default(),
            component_class_manager: ComponentClassManager::default(),
            file_name: String::new(),
            generator: String::new(),
            file_format_version_at_load: LEGACY_BOARD_FILE_VERSION,
            board_use: BoardUse::Normal,
            user_units: EdaUnits::Mm,
            outlines_chaining_epsilon: PCB_IU_SCALE.mm_to_iu(DEFAULT_CHAINING_EPSILON_MM),
            legacy_teardrops: false,
            legacy_design_settings_loaded: false,
            legacy_copper_edge_clearance_loaded: false,
            legacy_netclasses_loaded: false,
            project: None,
            text_var_clock: Default::default(),
            time_stamp: 1,
            // Replaced at the end of the constructor by the snapshot of the settings.
            layer_env: Arc::new(LayerEnv::no_board()),
            layer_env_stale: false,
            journal: ChangeJournal::default(),
            kiid_generator: KiidGenerator::default(),
            caches: BoardCaches::default(),
        };

        // m_NetInfo( this ): net 0 takes the default class of the NET_SETTINGS constructor,
        // which the constructor body replaces below (the net keeps the old one, as in C++).
        let initial_default = Arc::clone(board.design_settings.net_settings.default_netclass());

        if let Some(net) = board.net_info.nets.get_mut(&NetCode::UNCONNECTED) {
            net.netclass = Some(initial_default);
            // NETINFO_LIST::AppendNet via BOARD::Add sets the parent of every later net;
            // net 0 is a BOARD_ITEM of this board as well.
            net.base.parent = ParentRef::Board;
        }

        board.init_layers();

        // Initialize default netclass.
        let mut default_netclass = Netclass::new(NETCLASS_DEFAULT_NAME, true);
        default_netclass.description = "This is the default net class.".to_string();
        let bds = &mut board.design_settings;
        bds.net_settings
            .set_default_netclass(Arc::new(default_netclass));
        bds.set_use_custom_track_via_size(false);

        board.layer_env = Arc::new(board.build_layer_env());
        board
    }
}

impl Board {
    /// `new BOARD()`.
    pub fn new() -> Self {
        Self::default()
    }

    // ---- time stamp (board-model.md 8.5) ----

    /// `BOARD::GetTimeStamp()`: the one invalidation signal of every consumer.
    pub fn time_stamp(&self) -> i32 {
        self.time_stamp
    }

    /// `BOARD::IncrementTimeStamp()`; also clears `BoardCaches`. The stamp only ever has to
    /// differ from the one a consumer recorded, so it wraps instead of overflowing (the model
    /// bumps it on every edit, `board-model.md` 8.5).
    pub fn increment_time_stamp(&mut self) {
        self.time_stamp = self.time_stamp.wrapping_add(1);
        self.caches = BoardCaches::default();
    }

    // ---- design settings ----

    /// `GetDesignSettings()` (const).
    pub fn design_settings(&self) -> &BoardDesignSettings {
        &self.design_settings
    }

    /// `GetDesignSettings()` for writing: bumps the time stamp and marks the `LayerEnv`
    /// snapshot stale.
    pub fn design_settings_mut(&mut self) -> &mut BoardDesignSettings {
        self.increment_time_stamp();
        self.layer_env_stale = true;
        &mut self.design_settings
    }

    // ---- MutEnv (board-model.md 8.2) ----

    /// The footprint frame an item with parent `parent` sees (`GetParentFootprint()`:
    /// the first footprint on the parent chain, e.g. through a table for a cell).
    fn parent_frame(&self, parent: ParentRef) -> Option<ParentFrame> {
        let mut next = parent.item();

        while let Some(id) = next {
            if id.kind() == ItemKind::Footprint {
                let fid: FootprintId = id.typed()?;
                let fp = self.footprints.get(fid)?;
                return Some(ParentFrame {
                    footprint: fid,
                    transform: fp.transform,
                    layer: fp.base.layer,
                    flipped: fp.flipped,
                });
            }

            next = BoardItem::base(self.get(id)?.as_dyn()).parent.item();
        }

        None
    }

    fn env_for_parent(&self, parent: ParentRef, layers: Arc<LayerEnv>) -> MutEnv {
        if parent == ParentRef::None {
            return MutEnv::detached();
        }

        MutEnv {
            parent: self.parent_frame(parent),
            layers,
            max_error: self.design_settings.max_error(),
            board_use: self.board_use,
        }
    }

    /// The current `LayerEnv`, freshly built while the cached one is stale.
    fn current_layer_env(&self) -> Arc<LayerEnv> {
        if self.layer_env_stale {
            Arc::new(self.build_layer_env())
        } else {
            Arc::clone(&self.layer_env)
        }
    }

    /// Snapshot of what C++ mutators of `id` read through `GetParentFootprint()` /
    /// `GetBoard()`. A stale handle gets the board-level environment.
    pub fn mut_env(&self, id: ItemId) -> MutEnv {
        let parent = self
            .get(id)
            .map_or(ParentRef::Board, |item| item.base().parent);
        self.env_for_parent(parent, self.current_layer_env())
    }

    /// `mut_env` for the `&mut self` entry points: re-caches a stale `LayerEnv` first. `None`
    /// for a stale handle.
    pub(crate) fn mut_env_refreshed(&mut self, id: ItemId) -> Option<MutEnv> {
        if self.layer_env_stale {
            self.layer_env = Arc::new(self.build_layer_env());
            self.layer_env_stale = false;
        }

        let parent = self.get(id)?.base().parent;
        self.journal_note_before(id);
        Some(self.env_for_parent(parent, Arc::clone(&self.layer_env)))
    }

    /// Bookkeeping after a `modify_*`: time stamp, parent footprint geometry caches, journal.
    pub(crate) fn after_modify(&mut self, id: ItemId, parent: ParentRef) {
        self.increment_time_stamp();

        if let Some(frame) = self.parent_frame(parent) {
            if let Some(fp) = self.footprints.get_mut(frame.footprint) {
                fp.invalidate_geometry_caches();
            }
        }

        self.journal_record_modified(id);
    }

    /// Store `item` in its arena and stamp `base.id` and `base.parent` (crate-internal: the
    /// building block of `add_*`, `footprint_add`, `create_*` and the test fixtures). Does
    /// not touch order lists, the UUID index or the time stamp.
    pub(crate) fn insert_item<T: ArenaItem + BoardItem>(
        &mut self,
        item: T,
        parent: ParentRef,
    ) -> Id<T>
    where
        Board: ArenaOf<T>,
    {
        let id = self.arena_mut().insert(item);
        let base = self
            .arena_mut()
            .get_mut(id)
            .map(BoardItem::base_mut)
            .expect("just inserted");
        base.id = Some(id.into());
        base.parent = parent;
        id
    }

    // ---- iteration in C++ container order (board-model.md 10.1) ----

    /// `Tracks()`: deque order (a loaded board is file order; `Insert` prepends).
    pub fn tracks(&self) -> impl Iterator<Item = (TrackId, &crate::pcb_track::Track)> + '_ {
        self.track_list
            .iter()
            .filter_map(move |&id| self.tracks.get(id).map(|t| (id, t)))
    }

    /// `Footprints()`: deque order.
    pub fn footprints(&self) -> impl Iterator<Item = (FootprintId, &Footprint)> + '_ {
        self.footprint_list
            .iter()
            .filter_map(move |&id| self.footprints.get(id).map(|f| (id, f)))
    }

    /// `Drawings()`: deque order.
    pub fn drawings(&self) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_ {
        self.drawing_list
            .iter()
            .filter_map(move |&id| self.get(id).map(|item| (id, item)))
    }

    /// `Zones()`: always appended.
    pub fn zones(&self) -> impl Iterator<Item = (ZoneId, &crate::zone::Zone)> + '_ {
        self.zone_list
            .iter()
            .filter_map(move |&id| self.zones.get(id).map(|z| (id, z)))
    }

    /// `Groups()`.
    pub fn groups(&self) -> impl Iterator<Item = (GroupId, &crate::pcb_group::PcbGroup)> + '_ {
        self.group_list
            .iter()
            .filter_map(move |&id| self.groups.get(id).map(|g| (id, g)))
    }

    /// `Generators()`.
    pub fn generators(
        &self,
    ) -> impl Iterator<Item = (GeneratorId, &crate::pcb_generator::PcbGenerator)> + '_ {
        self.generator_list
            .iter()
            .filter_map(move |&id| self.generators.get(id).map(|g| (id, g)))
    }

    /// `Constraints()` (board level).
    pub fn constraints(
        &self,
    ) -> impl Iterator<Item = (ConstraintId, &crate::pcb_constraint::PcbConstraint)> + '_ {
        self.constraint_list
            .iter()
            .filter_map(move |&id| self.constraints.get(id).map(|c| (id, c)))
    }

    /// `Points()` (board level).
    pub fn board_points(
        &self,
    ) -> impl Iterator<Item = (PointId, &crate::pcb_point::PcbPoint)> + '_ {
        self.point_list
            .iter()
            .filter_map(move |&id| self.points.get(id).map(|p| (id, p)))
    }

    /// `Markers()`.
    pub fn markers(&self) -> impl Iterator<Item = (MarkerId, &crate::pcb_marker::PcbMarker)> + '_ {
        self.marker_list
            .iter()
            .filter_map(move |&id| self.markers.get(id).map(|m| (id, m)))
    }

    /// Items of one footprint child list, resolved through `arena`.
    fn fp_list<'a, T: 'a, L: 'a>(
        &'a self,
        fp: FootprintId,
        list: impl Fn(&'a Footprint) -> &'a L + 'a,
        arena: &'a Arena<T>,
    ) -> impl Iterator<Item = (Id<T>, &'a T)> + 'a
    where
        &'a L: IntoIterator<Item = &'a Id<T>>,
    {
        self.footprints.get(fp).into_iter().flat_map(move |f| {
            list(f)
                .into_iter()
                .filter_map(move |&id| arena.get(id).map(|x| (id, x)))
        })
    }

    /// `FOOTPRINT::Fields()`: always appended.
    pub fn footprint_fields(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (crate::ids::FieldId, &crate::pcb_field::PcbField)> + '_ {
        self.fp_list(fp, |f| &f.fields, &self.fields)
    }

    /// `FOOTPRINT::Pads()`.
    pub fn footprint_pads(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (crate::ids::PadId, &crate::pad::Pad)> + '_ {
        self.fp_list(fp, |f| &f.pads, &self.pads)
    }

    /// `FOOTPRINT::GraphicalItems()`.
    pub fn footprint_drawings(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_ {
        self.footprints.get(fp).into_iter().flat_map(move |f| {
            f.drawings
                .iter()
                .filter_map(move |&id| self.get(id).map(|item| (id, item)))
        })
    }

    /// `FOOTPRINT::Zones()`.
    pub fn footprint_zones(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (ZoneId, &crate::zone::Zone)> + '_ {
        self.fp_list(fp, |f| &f.zones, &self.zones)
    }

    /// `FOOTPRINT::Groups()`.
    pub fn footprint_groups(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (GroupId, &crate::pcb_group::PcbGroup)> + '_ {
        self.fp_list(fp, |f| &f.groups, &self.groups)
    }

    /// `FOOTPRINT::Points()`.
    pub fn footprint_points(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (PointId, &crate::pcb_point::PcbPoint)> + '_ {
        self.fp_list(fp, |f| &f.points, &self.points)
    }

    /// `FOOTPRINT::Constraints()`.
    pub fn footprint_constraints(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (ConstraintId, &crate::pcb_constraint::PcbConstraint)> + '_ {
        self.fp_list(fp, |f| &f.constraints, &self.constraints)
    }

    /// `FOOTPRINT::Bodies()`.
    pub fn footprint_bodies(
        &self,
        fp: FootprintId,
    ) -> impl Iterator<Item = (crate::ids::Body3dId, &crate::pcb_3d_body::Pcb3dBody)> + '_ {
        self.fp_list(fp, |f| &f.bodies, &self.bodies)
    }

    /// `PCB_TABLE::GetCells()`.
    pub fn table_cells(
        &self,
        t: crate::ids::TableId,
    ) -> impl Iterator<Item = (crate::ids::TablecellId, &crate::pcb_tablecell::PcbTablecell)> + '_
    {
        self.tables.get(t).into_iter().flat_map(move |table| {
            table
                .cells
                .iter()
                .filter_map(move |&id| self.tablecells.get(id).map(|c| (id, c)))
        })
    }

    /// `EDA_GROUP::GetItems()` of a group or generator (insertion order).
    pub fn group_members(&self, g: ItemId) -> impl Iterator<Item = ItemId> + '_ {
        let members: &[ItemId] = match self.get(g) {
            Some(AnyItem::Group(group)) => &group.members,
            Some(AnyItem::Generator(generator)) => &generator.group.members,
            _ => &[],
        };
        members.iter().copied()
    }

    /// Snapshot of `tracks()` for loops that mutate.
    pub fn track_ids(&self) -> Vec<TrackId> {
        self.tracks().map(|(id, _)| id).collect()
    }

    /// Snapshot of `footprints()` for loops that mutate.
    pub fn footprint_ids(&self) -> Vec<FootprintId> {
        self.footprints().map(|(id, _)| id).collect()
    }

    // ---- invariants (board-model.md 4) ----

    /// Verify the structural invariants (debug builds, every model test): order lists hold
    /// live ids and every live item is in exactly one list (no created-but-unattached container
    /// is left over); `parent` agrees with the containing list; `base.id` is the item's handle;
    /// `group` agrees with `members` and the group graph is acyclic; the UUID index points at
    /// live items carrying that UUID; every `net` is known, `UNCONNECTED` or `ORPHANED`.
    pub fn check_invariants(&self) -> Result<(), String> {
        // Owner of every listed id: the parent its list implies.
        let mut owner: HashMap<ItemId, ParentRef> = HashMap::new();
        let mut errors = Vec::new();

        let mut claim = |id: ItemId, parent: ParentRef, list: &str, errors: &mut Vec<String>| {
            if let Some(previous) = owner.insert(id, parent) {
                errors.push(format!(
                    "{id:?} is listed twice ({list}; earlier owner {previous:?})"
                ));
            }
        };

        macro_rules! board_list {
            ($list:expr, $name:literal) => {
                for &id in $list.iter() {
                    claim(id.into(), ParentRef::Board, $name, &mut errors);
                }
            };
        }

        board_list!(self.marker_list, "markers");
        board_list!(self.drawing_list, "drawings");
        board_list!(self.footprint_list, "footprints");
        board_list!(self.track_list, "tracks");
        board_list!(self.group_list, "groups");
        board_list!(self.constraint_list, "constraints");
        board_list!(self.zone_list, "zones");
        board_list!(self.generator_list, "generators");
        board_list!(self.point_list, "points");

        for (fid, fp) in self.footprints.iter() {
            let parent = ParentRef::Item(fid.into());

            macro_rules! fp_list {
                ($list:ident) => {
                    for &id in fp.$list.iter() {
                        claim(
                            id.into(),
                            parent,
                            concat!("footprint ", stringify!($list)),
                            &mut errors,
                        );
                    }
                };
            }

            fp_list!(fields);
            fp_list!(drawings);
            fp_list!(pads);
            fp_list!(zones);
            fp_list!(groups);
            fp_list!(constraints);
            fp_list!(points);
            fp_list!(bodies);
        }

        for (tid, table) in self.tables.iter() {
            for &cell in &table.cells {
                claim(
                    cell.into(),
                    ParentRef::Item(tid.into()),
                    "table cells",
                    &mut errors,
                );
            }
        }

        for &id in owner.keys() {
            if !self.contains(id) {
                errors.push(format!("order list holds stale handle {id:?}"));
            }
        }

        let mut live = HashSet::new();
        self.for_each_live_item(&mut |id, base| {
            live.insert(id);

            if base.id != Some(id) {
                errors.push(format!("{id:?}: base.id is {:?}", base.id));
            }

            match owner.get(&id) {
                None => errors.push(format!(
                    "{id:?} is in no order list (created but never attached?)"
                )),
                Some(parent) if *parent != base.parent => errors.push(format!(
                    "{id:?}: parent {:?} but listed under {parent:?}",
                    base.parent
                )),
                Some(_) => {}
            }

            if let Some(group) = base.group {
                let is_member = self.group_members(group).any(|m| m == id);

                if !matches!(group.kind(), ItemKind::Group | ItemKind::Generator) || !is_member {
                    errors.push(format!(
                        "{id:?}: group {group:?} does not list it as a member"
                    ));
                }
            }
        });

        // Group side of the relation, and acyclicity.
        for &g in live
            .iter()
            .filter(|id| matches!(id.kind(), ItemKind::Group | ItemKind::Generator))
        {
            for member in self.group_members(g) {
                match self.get(member) {
                    Some(item) if item.base().group == Some(g) => {}
                    _ => errors.push(format!(
                        "group {g:?}: member {member:?} is stale or names another group"
                    )),
                }
            }

            let mut current = Some(g);
            let mut steps = 0;

            while let Some(c) = current {
                steps += 1;

                if steps > live.len() + 1 {
                    errors.push(format!("group {g:?} is part of a group cycle"));
                    break;
                }

                current = self.get(c).and_then(|item| item.base().group);
            }
        }

        for (uuid, &id) in &self.uuid_index {
            match self.get(id) {
                Some(item) if item.base().uuid == *uuid => {}
                _ => errors.push(format!(
                    "UUID index: {uuid} -> {id:?} is stale or carries another UUID"
                )),
            }
        }

        for &id in &live {
            if let Some(connected) = self.get(id).and_then(|item| item.as_connected()) {
                let net = connected.net_code();

                if net != NetCode::UNCONNECTED
                    && net != NetCode::ORPHANED
                    && !self.net_info.nets.contains_key(&net)
                {
                    errors.push(format!("{id:?}: unknown net {net:?}"));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            errors.sort();
            Err(errors.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests;
