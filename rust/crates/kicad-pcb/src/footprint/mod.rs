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

//! Port of `pcbnew/footprint.h` and `pcbnew/footprint.cpp` (`FOOTPRINT`, `FP_3DMODEL`,
//! `FOOTPRINT_VARIANT`, the cache data structs) and of `include/jumper_group.h`,
//! `board-model-types.md` section 4.
//!
//! This file holds the data, the constructor, the delegation shims (frozen after M0), the
//! trivial accessors, fields / variants / attributes and the net-tie cache (package M19). The
//! placement cascades are `Board::footprint_*` in `transform.rs` (M19); bounding boxes,
//! hulls and courtyards in `geometry.rs`, comparators in `compare.rs` and the polygon
//! exports in `shapes.rs` (M20). A footprint is a container: it has no `BoardItemMut` impl,
//! its mutating virtuals cascade through `Board` (`board-model.md` 8.3).

pub mod compare;
pub mod geometry;
pub mod shapes;
pub mod transform;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use kicad_core::case_insensitive_map::CaseInsensitiveMap;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, KiidPath, LibId, Lset, PcbLayerId};
use kicad_geom::geometry::transform_trs::TransformTrs;
use kicad_geom::hash_128::Hash128;
use kicad_geom::math::vector3::Vector3D;
use kicad_geom::{Box2I, EdaAngle, ShapePolySet, Vector2I};

use crate::board::Board;
use crate::cache::{CacheCell, KeyedCache};
use crate::component_class::{ComponentClass, ComponentClassCache};
use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::embedded_files::EmbeddedFiles;
use crate::ids::{
    Body3dId, ConstraintId, FieldId, FootprintId, GroupId, ItemId, NetCode, PadId, PointId, ZoneId,
};
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::traits::{BoardItem, Flashing, Invalidate};
use crate::zone_settings::ZoneConnection;

/// `FOOTPRINT`.
#[derive(Clone, Debug)]
pub struct Footprint {
    /// `base.layer` is F_Cu or B_Cu (the side).
    pub base: BoardItemBase,

    // ---- children: order lists into the board arenas ------------------------------------
    // FOOTPRINT::Add inserts at the FRONT of every list unless the mode is exactly
    // ADD_MODE::APPEND; fields always push_back (footprint.cpp). pub(crate): only Board
    // methods edit them; read with board.footprint_pads(fp) etc.
    /// m_fields (std::deque); always appended.
    pub(crate) fields: VecDeque<FieldId>,
    /// m_drawings (Shape, Text, Textbox, Table, Dimension, Barcode, Image).
    pub(crate) drawings: VecDeque<ItemId>,
    /// m_pads
    pub(crate) pads: VecDeque<PadId>,
    /// m_zones (std::vector, inserted at begin())
    pub(crate) zones: VecDeque<ZoneId>,
    /// m_groups
    pub(crate) groups: VecDeque<GroupId>,
    /// m_constraints
    pub(crate) constraints: VecDeque<ConstraintId>,
    /// m_points
    pub(crate) points: VecDeque<PointId>,
    /// m_bodies
    pub(crate) bodies: VecDeque<Body3dId>,

    // ---- placement: written only by Board::footprint_* (board-model.md 8.3) --------------
    /// m_transform
    pub(crate) transform: TransformTrs,
    /// m_flipped
    pub(crate) flipped: bool,

    pub fpid: LibId,                                    // m_fpid
    pub attributes: i32,                                // m_attributes (FOOTPRINT_ATTR_T bits)
    pub fp_status: i32,                                 // m_fpStatus (FP_is_PLACED, FP_to_PLACE)
    pub file_format_version_at_load: i32,               // m_fileFormatVersionAtLoad
    pub variants: CaseInsensitiveMap<FootprintVariant>, // m_variants
    pub net_tie_pad_groups: Vec<String>,                // m_netTiePadGroups
    pub jumper_pad_groups: JumperGroupSet,              // m_jumperPadGroups
    pub duplicate_pad_numbers_are_jumpers: bool,        // m_duplicatePadNumbersAreJumpers
    pub allow_missing_courtyard: bool,                  // m_allowMissingCourtyard
    pub allow_solder_mask_bridges: bool,                // m_allowSolderMaskBridges

    pub zone_connection: ZoneConnection,  // m_zoneConnection
    pub clearance: Option<i32>,           // m_clearance
    pub solder_mask_margin: Option<i32>,  // m_solderMaskMargin
    pub solder_paste_margin: Option<i32>, // m_solderPasteMargin
    pub solder_paste_margin_ratio: Option<f64>, // m_solderPasteMarginRatio

    pub stackup_layers: Lset,           // m_stackupLayers
    pub stackup_mode: FootprintStackup, // m_stackupMode
    pub private_layers: Lset,           // m_privateLayers

    pub lib_description: String,               // m_libDescription
    pub keywords: String,                      // m_keywords
    pub path: KiidPath,                        // m_path
    pub sheetname: String,                     // m_sheetname
    pub sheetfile: String,                     // m_sheetfile
    pub filters: String,                       // m_filters
    pub models: Vec<Fp3dmodel>,                // m_3D_Drawings
    pub height: i32,                           // m_height
    pub initial_comments: Option<Vec<String>>, // m_initial_comments
    pub unit_info: Vec<FpUnitInfo>,            // m_unitInfo
    pub embedded_files: EmbeddedFiles,         // EMBEDDED_FILES base class
    /// m_componentClassCacheProxy->m_staticComponentClass
    pub static_component_class: Option<Arc<ComponentClass>>,

    // ---- derived ------------------------------------------------------------------------
    /// m_geometry_cache: three values each validated against its own board time stamp, as C++.
    pub(crate) geometry_cache: CacheCell<FootprintGeometryCacheData>,
    /// m_courtyard_cache: built lazily from the const GetCourtyard() in C++, hence a class-1
    /// cache; valid while the stored hashes equal the polygons' hashes.
    pub(crate) courtyard_cache: KeyedCache<FootprintCourtyardCacheData>,
    /// m_netTieCache (keyed by item pointer in C++): written by `Board::build_net_tie_cache`.
    pub(crate) net_tie_cache: BTreeMap<ItemId, BTreeSet<NetCode>>,
    /// (ticker, dynamic class, final class) of COMPONENT_CLASS_CACHE_PROXY
    pub(crate) component_class_cache: CacheCell<ComponentClassCache>,
}

/// `FOOTPRINT_ATTR_T` bits of `Footprint::attributes`.
pub mod footprint_attr {
    pub const FP_THROUGH_HOLE: i32 = 0x0001;
    pub const FP_SMD: i32 = 0x0002;
    pub const FP_EXCLUDE_FROM_POS_FILES: i32 = 0x0004;
    pub const FP_EXCLUDE_FROM_BOM: i32 = 0x0008;
    /// Footprint has no corresponding symbol.
    pub const FP_BOARD_ONLY: i32 = 0x0010;
    /// Footprint just added by netlist update.
    pub const FP_JUST_ADDED: i32 = 0x0020;
    pub const FP_DNP: i32 = 0x0040;
    pub const FP_EXCLUDE_FROM_SIM: i32 = 0x0080;
}

/// `FP_is_PLACED`: in autoplace, footprint automatically placed.
pub const FP_IS_PLACED: i32 = 0x02;
/// `FP_to_PLACE`: in autoplace, footprint waiting for autoplace.
pub const FP_TO_PLACE: i32 = 0x04;

/// `FOOTPRINT_TYPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FootprintType {
    Unspecified,
    ThroughHole,
    Smd,
}

/// `FOOTPRINT_STACKUP`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum FootprintStackup {
    /// The "normal" stackup handling: a single inner layer (In1) that rule areas expand to
    /// all inner layers of the host board.
    #[default]
    ExpandInnerLayers,
    /// Any number of copper layers, objects go to the matching inner layer of the board.
    CustomLayers,
}

/// `FP_3DMODEL`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fp3dmodel {
    /// m_Scale: 3D model scaling factor (dimensionless).
    pub scale: Vector3D,
    /// m_Rotation: 3D model rotation (degrees).
    pub rotation: Vector3D,
    /// m_Offset: 3D model offset (mm).
    pub offset: Vector3D,
    pub opacity: f64, // m_Opacity
    /// m_Filename: the 3D shape filename in the 3D library.
    pub filename: String,
    /// m_Show: include the model in rendering.
    pub show: bool,
}

impl Default for Fp3dmodel {
    /// `FP_3DMODEL()`.
    fn default() -> Self {
        Fp3dmodel {
            scale: Vector3D::new(1.0, 1.0, 1.0),
            rotation: Vector3D::new(0.0, 0.0, 0.0),
            offset: Vector3D::new(0.0, 0.0, 0.0),
            opacity: 1.0,
            filename: String::new(),
            show: true,
        }
    }
}

/// `FOOTPRINT_VARIANT`: per-variant overrides of DNP, exclusion flags and field values.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FootprintVariant {
    pub name: String,                  // m_name
    pub dnp: bool,                     // m_dnp
    pub excluded_from_bom: bool,       // m_excludedFromBOM
    pub excluded_from_sim: bool,       // m_excludedFromSim
    pub excluded_from_pos_files: bool, // m_excludedFromPosFiles
    /// m_fields: field value overrides for this variant.
    pub fields: BTreeMap<String, String>,
}

impl FootprintVariant {
    /// `FOOTPRINT_VARIANT( aName )`.
    pub fn new(name: &str) -> Self {
        FootprintVariant {
            name: name.to_string(),
            ..FootprintVariant::default()
        }
    }

    /// `GetFieldValue()`: empty if not overridden.
    pub fn field_value(&self, field_name: &str) -> String {
        self.fields.get(field_name).cloned().unwrap_or_default()
    }

    /// `SetFieldValue()`.
    pub fn set_field_value(&mut self, field_name: &str, value: &str) {
        self.fields
            .insert(field_name.to_string(), value.to_string());
    }

    /// `RemoveFieldValue()`.
    pub fn remove_field_value(&mut self, field_name: &str) {
        self.fields.remove(field_name);
    }

    /// `HasFieldValue()`.
    pub fn has_field_value(&self, field_name: &str) -> bool {
        self.fields.contains_key(field_name)
    }
}

/// `FOOTPRINT::FP_UNIT_INFO`: optional unit mapping of multi-unit symbols.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FpUnitInfo {
    /// m_unitName, e.g. "A".
    pub unit_name: String,
    /// m_pins: pin numbers in this unit.
    pub pins: Vec<String>,
}

/// `JUMPER_GROUP_SET` (`include/jumper_group.h`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JumperGroupSet {
    pub groups: Vec<JumperGroup>, // m_groups
}

/// `JUMPER_GROUP`: a set of pad numbers jumpered together.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JumperGroup {
    /// m_names (never empty).
    pub names: BTreeSet<String>,
}

/// `FOOTPRINT_GEOMETRY_CACHE_DATA`.
#[derive(Debug, Default)]
pub struct FootprintGeometryCacheData {
    pub bounding_box: Box2I,
    pub bounding_box_timestamp: i32,
    pub text_excluded_bbox: Box2I,
    pub text_excluded_bbox_timestamp: i32,
    pub hull: ShapePolySet,
    pub hull_timestamp: i32,
}

/// `FOOTPRINT_COURTYARD_CACHE_DATA`. The MALFORMED_F/B_COURTYARD item flags that
/// BuildCourtyardCaches sets live here (a `&self` build cannot write `base.flags`); consumers
/// read them through `fp.courtyard_flags(ctx)` where C++ tests `GetFlags() & MALFORMED_*`.
#[derive(Debug, Default)]
pub struct FootprintCourtyardCacheData {
    pub front: ShapePolySet,
    pub back: ShapePolySet,
    pub front_hash: Hash128,
    pub back_hash: Hash128,
    pub malformed_front: bool,
    pub malformed_back: bool,
}

// ---- constructor -------------------------------------------------------------------------

impl Default for Footprint {
    /// `FOOTPRINT( BOARD* parent )` with a nil UUID, **without** its four mandatory fields:
    /// the fields are arena items and are created by `Board::create_footprint` (M1,
    /// `board-model.md` 8.1). Side F_Cu, identity transform, no attributes, inherited zone
    /// connection, stackup layers { F_Cu, In1_Cu, B_Cu } in EXPAND_INNER_LAYERS mode.
    fn default() -> Self {
        Footprint {
            base: BoardItemBase::with_layer(PcbLayerId::FCu),
            fields: VecDeque::new(),
            drawings: VecDeque::new(),
            pads: VecDeque::new(),
            zones: VecDeque::new(),
            groups: VecDeque::new(),
            constraints: VecDeque::new(),
            points: VecDeque::new(),
            bodies: VecDeque::new(),
            transform: TransformTrs::default(),
            flipped: false,
            fpid: LibId::default(),
            attributes: 0,
            fp_status: 0,
            file_format_version_at_load: 0,
            variants: CaseInsensitiveMap::new(),
            net_tie_pad_groups: Vec::new(),
            jumper_pad_groups: JumperGroupSet::default(),
            duplicate_pad_numbers_are_jumpers: false,
            allow_missing_courtyard: false,
            allow_solder_mask_bridges: false,
            zone_connection: ZoneConnection::Inherited,
            clearance: None,
            solder_mask_margin: None,
            solder_paste_margin: None,
            solder_paste_margin_ratio: None,
            stackup_layers: Lset::from_layers(&[
                PcbLayerId::FCu,
                PcbLayerId::In1Cu,
                PcbLayerId::BCu,
            ]),
            stackup_mode: FootprintStackup::ExpandInnerLayers,
            private_layers: Lset::new(),
            lib_description: String::new(),
            keywords: String::new(),
            path: KiidPath::default(),
            sheetname: String::new(),
            sheetfile: String::new(),
            filters: String::new(),
            models: Vec::new(),
            height: 0,
            initial_comments: None,
            unit_info: Vec::new(),
            embedded_files: EmbeddedFiles::default(),
            static_component_class: None,
            geometry_cache: CacheCell::default(),
            courtyard_cache: KeyedCache::default(),
            net_tie_cache: BTreeMap::new(),
            component_class_cache: CacheCell::default(),
        }
    }
}

impl Footprint {
    /// The `FOOTPRINT` constructor values with a fresh UUID (still without the mandatory
    /// fields, see `Default`; `Board::create_footprint` is the full constructor).
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut footprint = Footprint::default();
        footprint.base.uuid = Kiid::new(kiid_generator);
        footprint
    }

    /// Drop the geometry caches that depend on the children (bounding boxes, hull,
    /// courtyard), called by `Board::modify_*` after a child changed.
    pub(crate) fn invalidate_geometry_caches(&mut self) {
        self.geometry_cache.clear();
        self.courtyard_cache.clear();
    }

    // ---- placement (read side; writers are Board::footprint_*) ---------------------------

    /// `GetTransform()`.
    pub fn transform(&self) -> TransformTrs {
        self.transform
    }

    /// `m_flipped`: whether the library frame is mirrored onto the board.
    pub fn flipped(&self) -> bool {
        self.flipped
    }

    /// `IsFlipped()`: the footprint is on the back side.
    pub fn is_flipped(&self) -> bool {
        self.base.layer == PcbLayerId::BCu
    }

    /// `GetPosition()`: the transform's translation.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.transform.translate()
    }

    /// `GetOrientation()`: the transform's rotation.
    pub fn orientation(&self) -> EdaAngle {
        self.transform.rotate()
    }

    /// `GetLayer()`: the side.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        self.base.layer
    }

    /// `IsLayerAgnostic()`: a footprint's children may sit on layers the board has
    /// disabled.
    pub fn is_layer_agnostic(&self) -> bool {
        true
    }

    /// `FitsEnabledLayers()`: only the mounting side is judged.
    pub fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        _enabled: Lset,
        _copper_layer_count: i32,
    ) -> bool {
        let layer = self.layer(ctx);
        layer == PcbLayerId::FCu || layer == PcbLayerId::BCu
    }

    /// `FOOTPRINT::IsOnLayer()`: the footprint or any child on `layer`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        crate::port_pending!("FOOTPRINT::IsOnLayer", ctx, layer)
    }

    /// `FOOTPRINT::RunOnChildren()`: fields, pads, zones, groups, bodies, constraints,
    /// points, drawings (+ their children when recursing).
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        crate::port_pending!("FOOTPRINT::RunOnChildren", ctx, mode, f)
    }

    /// `FOOTPRINT::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        crate::port_pending!("FOOTPRINT::GetItemDescription", ctx, units, full)
    }

    // ---- fields, attributes, variants (package M19) -------------------------------------

    /// `GetReference()`: the text of the reference field.
    pub fn reference(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("FOOTPRINT::GetReference", ctx)
    }

    /// `GetValue()`: the text of the value field.
    pub fn value(&self, ctx: &BoardCtx<'_>) -> String {
        crate::port_pending!("FOOTPRINT::GetValue", ctx)
    }

    /// `GetField( aFieldName )` (by name, case-sensitive like C++).
    pub fn field_by_name(&self, ctx: &BoardCtx<'_>, name: &str) -> Option<FieldId> {
        crate::port_pending!("FOOTPRINT::GetField (name)", ctx, name)
    }

    /// `GetFPID()`.
    pub fn fpid(&self) -> &LibId {
        &self.fpid
    }

    /// `GetAttributes()`.
    pub fn attributes(&self) -> i32 {
        self.attributes
    }

    /// `SetAttributes()`.
    pub fn set_attributes(&mut self, attributes: i32) {
        self.attributes = attributes;
    }

    /// `IsBoardOnly()`.
    pub fn is_board_only(&self) -> bool {
        self.attributes & footprint_attr::FP_BOARD_ONLY != 0
    }

    /// `IsExcludedFromPosFiles()`.
    pub fn is_excluded_from_pos_files(&self) -> bool {
        self.attributes & footprint_attr::FP_EXCLUDE_FROM_POS_FILES != 0
    }

    /// `IsExcludedFromBOM()`.
    pub fn is_excluded_from_bom(&self) -> bool {
        self.attributes & footprint_attr::FP_EXCLUDE_FROM_BOM != 0
    }

    /// `IsDNP()`.
    pub fn is_dnp(&self) -> bool {
        self.attributes & footprint_attr::FP_DNP != 0
    }

    /// `IsPlaced()`.
    pub fn is_placed(&self) -> bool {
        self.fp_status & FP_IS_PLACED != 0
    }

    /// `NeedsPlaced()`.
    pub fn needs_placed(&self) -> bool {
        self.fp_status & FP_TO_PLACE != 0
    }

    /// `GetFootprintType()` (from the attributes).
    pub fn footprint_type(&self) -> FootprintType {
        crate::port_pending!("FOOTPRINT::GetFootprintType")
    }

    /// `GetVariant( aVariantName )` (case-insensitive lookup).
    pub fn variant(&self, name: &str) -> Option<&FootprintVariant> {
        kicad_core::case_insensitive_map::ci_get(&self.variants, name)
    }

    /// `GetDNPForVariant( aVariantName )`.
    pub fn dnp_for_variant(&self, variant_name: &str) -> bool {
        crate::port_pending!("FOOTPRINT::GetDNPForVariant", variant_name)
    }

    /// `GetLocalClearance()`.
    pub fn local_clearance(&self) -> Option<i32> {
        self.clearance
    }

    /// `GetLocalZoneConnection()`.
    pub fn local_zone_connection(&self) -> ZoneConnection {
        self.zone_connection
    }

    // ---- net ties and jumpers (package M19) ---------------------------------------------

    /// `GetNetTieCache( aItem )`: the net codes a footprint item may short (empty if none).
    pub fn net_tie_cache(&self, item: ItemId) -> &BTreeSet<NetCode> {
        static EMPTY: BTreeSet<NetCode> = BTreeSet::new();
        self.net_tie_cache.get(&item).unwrap_or(&EMPTY)
    }

    /// `IsNetTie()`: the footprint has net-tie pad groups.
    pub fn is_net_tie(&self) -> bool {
        !self.net_tie_pad_groups.is_empty()
    }

    /// `MapPadNumbersToNetTieGroups()`: pad number -> group index (-1: none).
    pub fn map_pad_numbers_to_net_tie_groups(&self, ctx: &BoardCtx<'_>) -> BTreeMap<String, i32> {
        crate::port_pending!("FOOTPRINT::MapPadNumbersToNetTieGroups", ctx)
    }

    /// `GetNetTiePads( aPad )`.
    pub fn net_tie_pads(&self, ctx: &BoardCtx<'_>, pad: PadId) -> Vec<PadId> {
        crate::port_pending!("FOOTPRINT::GetNetTiePads", ctx, pad)
    }

    /// `GetJumperPadGroup( aPadNumber )`: the jumper group of a pad number.
    pub fn jumper_pad_group(&self, pad_number: &str) -> Option<&BTreeSet<String>> {
        crate::port_pending!("FOOTPRINT::GetJumperPadGroup", pad_number)
    }

    /// `GetEffectiveComponentClass()` (static class, or the dynamic one from the board's
    /// component class manager, cached in `component_class_cache`).
    pub fn component_class(&self, ctx: &BoardCtx<'_>) -> Option<Arc<ComponentClass>> {
        crate::port_pending!("FOOTPRINT::GetComponentClass", ctx)
    }
}

impl Board {
    /// `FOOTPRINT::BuildNetTieCache()`: cache, per footprint item, the net codes it may
    /// short through the net-tie pad groups (derived state: no invalidation, no time stamp).
    pub fn build_net_tie_cache(&mut self, fp: FootprintId) {
        crate::port_pending!("FOOTPRINT::BuildNetTieCache", fp)
    }
}

// ---- delegation shims (frozen after M0) --------------------------------------------------

impl Invalidate for Footprint {
    fn invalidate_caches(&mut self) {
        self.geometry_cache.clear();
        self.courtyard_cache.clear();
        self.component_class_cache.clear();
    }
}

impl BoardItem for Footprint {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbFootprintT
    }
    fn class_name(&self) -> &'static str {
        "FOOTPRINT"
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        Footprint::layer(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        Footprint::is_on_layer(self, ctx, layer)
    }
    fn is_layer_agnostic(&self) -> bool {
        Footprint::is_layer_agnostic(self)
    }
    fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        Footprint::fits_enabled_layers(self, ctx, enabled, copper_layer_count)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        Footprint::position(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        Footprint::center(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        Footprint::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        Footprint::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        Footprint::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        Footprint::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        Footprint::run_on_children(self, ctx, mode, f)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        Footprint::item_description(self, ctx, units, full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rotated_flipped_scaled;

    /// `FOOTPRINT::FOOTPRINT( BOARD* )` (footprint.cpp), without the mandatory fields, and
    /// `FP_3DMODEL()` / `FOOTPRINT_VARIANT()` (footprint.h).
    #[test]
    fn footprint_constructor_defaults() {
        let fp = Footprint::default();
        assert_eq!(fp.base.layer, PcbLayerId::FCu);
        assert!(fp.transform.is_identity() && !fp.flipped);
        assert_eq!(
            (fp.attributes, fp.fp_status, fp.file_format_version_at_load),
            (0, 0, 0)
        );
        assert!(!fp.duplicate_pad_numbers_are_jumpers);
        assert!(!fp.allow_missing_courtyard && !fp.allow_solder_mask_bridges);
        assert_eq!(fp.zone_connection, ZoneConnection::Inherited);
        assert_eq!(fp.clearance, None);
        assert_eq!(fp.solder_paste_margin_ratio, None);
        assert_eq!(
            fp.stackup_layers,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::BCu])
        );
        assert_eq!(fp.stackup_mode, FootprintStackup::ExpandInnerLayers);
        assert_eq!(fp.private_layers.count(), 0);
        assert!(fp.models.is_empty() && fp.initial_comments.is_none());
        assert_eq!(fp.height, 0);
        assert!(fp.fields.is_empty() && fp.pads.is_empty());
        assert!(fp.is_layer_agnostic() && !fp.is_flipped());

        let model = Fp3dmodel::default();
        assert_eq!(model.scale, Vector3D::new(1.0, 1.0, 1.0));
        assert_eq!((model.opacity, model.show), (1.0, true));

        let mut variant = FootprintVariant::new("V1");
        assert!(!variant.dnp && !variant.excluded_from_bom);
        variant.set_field_value("Value", "10k");
        assert_eq!(variant.field_value("Value"), "10k");
        assert_eq!(variant.field_value("X"), "");
    }

    #[test]
    fn footprint_fixture_reads() {
        let fx = rotated_flipped_scaled();
        fx.board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&fx.board);
        let fp = &fx.board[fx.footprint];
        assert_eq!(fp.position(&ctx), Vector2I::new(10_000_000, 20_000_000));
        assert_eq!(fp.orientation(), EdaAngle::from_degrees(30.0));
        assert!(fp.flipped() && fp.is_flipped());
        assert!(BoardItem::fits_enabled_layers(fp, &ctx, Lset::new(), 2));
        assert!(fp.net_tie_cache(fx.footprint.into()).is_empty());
    }
}
