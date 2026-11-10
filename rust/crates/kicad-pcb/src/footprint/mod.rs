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

use kicad_core::case_insensitive_map::{
    CaseInsensitiveKey, CaseInsensitiveMap, case_insensitive_cmp,
};
use kicad_core::layer_ids::{is_copper_layer, is_valid_layer};
use kicad_core::lib_id::get_lib_filename_forbidden_chars;
use kicad_core::string_utils::{get_default_variant_name, get_trailing_int};
use kicad_core::text_vars::ResolutionContext;
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
use crate::ids::ItemKind;
use crate::ids::{
    Body3dId, ConstraintId, FieldId, FootprintId, GroupId, ItemId, NetCode, PadId, PointId, ZoneId,
};
use crate::item::{BoardItemBase, BoardUse, ItemFlags, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::padstack::{PadAttrib, PadProp};
use crate::pcb_field::FieldT;
use crate::traits::{BoardConnectedItem, BoardItem, Flashing, Invalidate};
use crate::zone_settings::ZoneConnection;
use crate::{board_connected_item, board_item};

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
#[derive(Clone, Debug, Default)]
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

    /// `FOOTPRINT::IsOnLayer()`: any pad, zone, field or drawing on `layer` (in that order,
    /// short-circuiting like `std::ranges::any_of`).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let on = |id: ItemId| ctx.item(id).is_some_and(|item| item.is_on_layer(layer));

        self.pads.iter().any(|&p| on(p.into()))
            || self.zones.iter().any(|&z| on(z.into()))
            || self.fields.iter().any(|&f| on(f.into()))
            || self.drawings.iter().any(|&d| on(d))
    }

    /// `FOOTPRINT::RunOnChildren()`: fields, pads, zones, groups, bodies, constraints,
    /// points, drawings (+ their children when recursing).
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        let visit = |id: ItemId, f: &mut dyn FnMut(ItemRef<'_>)| {
            if let Some(item) = ctx.item(id) {
                f(item);
            }
        };

        for &field in &self.fields {
            visit(field.into(), f);
        }

        for &pad in &self.pads {
            visit(pad.into(), f);
        }

        for &zone in &self.zones {
            visit(zone.into(), f);
        }

        for &group in &self.groups {
            visit(group.into(), f);
        }

        for &body in &self.bodies {
            visit(body.into(), f);
        }

        for &constraint in &self.constraints {
            visit(constraint.into(), f);
        }

        for &point in &self.points {
            visit(point.into(), f);
        }

        for &drawing in &self.drawings {
            if let Some(item) = ctx.item(drawing) {
                f(item);

                if mode == RecurseMode::Recurse {
                    item.as_dyn().run_on_children(ctx, RecurseMode::Recurse, f);
                }
            }
        }
    }

    /// `FOOTPRINT::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        _units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let mut reference = self.reference(ctx);

        if reference.is_empty() {
            reference = "<no reference designator>".to_string();
        }

        format!("Footprint {reference}")
    }

    /// `FOOTPRINT::DisambiguateItemDescription()`: the description and the library id.
    pub fn disambiguate_item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        format!(
            "{} ({})",
            self.item_description(ctx, units, full),
            self.fpid_as_string()
        )
    }

    /// `GetFPIDAsString()`.
    pub fn fpid_as_string(&self) -> String {
        self.fpid.format()
    }

    // ---- fields (package M19) -------------------------------------------------------------

    /// `GetField( FIELD_T )` (const overload): the first field with this id, `None` when it is
    /// missing (the non-const overload creates it; see `PORT_STATUS.md`).
    pub fn field(&self, ctx: &BoardCtx<'_>, field_type: FieldT) -> Option<FieldId> {
        self.fields.iter().copied().find(|&id| {
            ctx.board
                .field(id)
                .is_some_and(|f| f.field_id() == field_type)
        })
    }

    /// The text of a mandatory field (`Reference().GetText()` etc.); empty when the field is
    /// missing, where C++ dereferences a null pointer.
    fn field_text(&self, ctx: &BoardCtx<'_>, field_type: FieldT) -> String {
        self.field(ctx, field_type)
            .and_then(|id| ctx.board.field(id))
            .map(|f| f.text.text.text().to_string())
            .unwrap_or_default()
    }

    /// `GetReference()`: the text of the reference field.
    pub fn reference(&self, ctx: &BoardCtx<'_>) -> String {
        self.field_text(ctx, FieldT::Reference)
    }

    /// `GetValue()`: the text of the value field.
    pub fn value(&self, ctx: &BoardCtx<'_>) -> String {
        self.field_text(ctx, FieldT::Value)
    }

    /// `IsAnnotated()`: a reference that is not empty and does not end in `?`.
    pub fn is_annotated(&self, ctx: &BoardCtx<'_>) -> bool {
        let reference = self.reference(ctx);
        !reference.is_empty() && !reference.ends_with('?')
    }

    /// `GetField( aFieldName )` (by `GetName()`, case-sensitive like C++).
    pub fn field_by_name(&self, ctx: &BoardCtx<'_>, name: &str) -> Option<FieldId> {
        self.fields
            .iter()
            .copied()
            .find(|&id| ctx.board.field(id).is_some_and(|f| f.name(true) == name))
    }

    /// `HasField( aFieldName )`.
    pub fn has_field(&self, ctx: &BoardCtx<'_>, name: &str) -> bool {
        self.field_by_name(ctx, name).is_some()
    }

    /// `GetFields( aVector, aVisibleOnly )`: the fields (only the visible, non-empty ones if
    /// asked) sorted by ordinal with the libstdc++ `std::sort`.
    pub fn fields_sorted(&self, ctx: &BoardCtx<'_>, visible_only: bool) -> Vec<FieldId> {
        let mut fields: Vec<(FieldId, i32)> = Vec::new();

        for &id in &self.fields {
            let Some(field) = ctx.board.field(id) else {
                continue;
            };

            if visible_only && (!field.text.text.is_visible() || field.text.text.text().is_empty())
            {
                continue;
            }

            fields.push((id, field.ordinal()));
        }

        kicad_geom::stdsort::std_sort(&mut fields, |lhs, rhs| lhs.1 < rhs.1);
        fields.into_iter().map(|(id, _)| id).collect()
    }

    /// `GetNextFieldOrdinal()`: one more than the largest field ordinal, at least 42.
    pub fn next_field_ordinal(&self, ctx: &BoardCtx<'_>) -> i32 {
        let mut ordinal = 42; // Arbitrarily larger than any mandatory FIELD_T id

        for &id in &self.fields {
            if let Some(field) = ctx.board.field(id) {
                ordinal = std::cmp::max(ordinal, field.ordinal() + 1);
            }
        }

        ordinal
    }

    // ---- attributes (package M19) ---------------------------------------------------------

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

    /// Set or clear one `FOOTPRINT_ATTR_T` bit (the bodies of the `Set*` attribute setters).
    fn set_attribute_bit(&mut self, bit: i32, on: bool) {
        if on {
            self.attributes |= bit;
        } else {
            self.attributes &= !bit;
        }
    }

    /// `IsBoardOnly()`.
    pub fn is_board_only(&self) -> bool {
        self.attributes & footprint_attr::FP_BOARD_ONLY != 0
    }

    /// `SetBoardOnly()`.
    pub fn set_board_only(&mut self, is_board_only: bool) {
        self.set_attribute_bit(footprint_attr::FP_BOARD_ONLY, is_board_only);
    }

    /// `IsExcludedFromPosFiles()`.
    pub fn is_excluded_from_pos_files(&self) -> bool {
        self.attributes & footprint_attr::FP_EXCLUDE_FROM_POS_FILES != 0
    }

    /// `SetExcludedFromPosFiles()`.
    pub fn set_excluded_from_pos_files(&mut self, exclude: bool) {
        self.set_attribute_bit(footprint_attr::FP_EXCLUDE_FROM_POS_FILES, exclude);
    }

    /// `IsExcludedFromBOM()`.
    pub fn is_excluded_from_bom(&self) -> bool {
        self.attributes & footprint_attr::FP_EXCLUDE_FROM_BOM != 0
    }

    /// `SetExcludedFromBOM()`.
    pub fn set_excluded_from_bom(&mut self, exclude: bool) {
        self.set_attribute_bit(footprint_attr::FP_EXCLUDE_FROM_BOM, exclude);
    }

    /// `IsExcludedFromSim()`.
    pub fn is_excluded_from_sim(&self) -> bool {
        self.attributes & footprint_attr::FP_EXCLUDE_FROM_SIM != 0
    }

    /// `SetExcludedFromSim()`.
    pub fn set_excluded_from_sim(&mut self, exclude: bool) {
        self.set_attribute_bit(footprint_attr::FP_EXCLUDE_FROM_SIM, exclude);
    }

    /// `IsDNP()`.
    pub fn is_dnp(&self) -> bool {
        self.attributes & footprint_attr::FP_DNP != 0
    }

    /// `SetDNP()`.
    pub fn set_dnp(&mut self, dnp: bool) {
        self.set_attribute_bit(footprint_attr::FP_DNP, dnp);
    }

    /// `IsPlaced()`.
    pub fn is_placed(&self) -> bool {
        self.fp_status & FP_IS_PLACED != 0
    }

    /// `SetIsPlaced()`.
    pub fn set_is_placed(&mut self, is_placed: bool) {
        if is_placed {
            self.fp_status |= FP_IS_PLACED;
        } else {
            self.fp_status &= !FP_IS_PLACED;
        }
    }

    /// `NeedsPlaced()`.
    pub fn needs_placed(&self) -> bool {
        self.fp_status & FP_TO_PLACE != 0
    }

    /// `SetNeedsPlaced()`.
    pub fn set_needs_placed(&mut self, needs_placed: bool) {
        if needs_placed {
            self.fp_status |= FP_TO_PLACE;
        } else {
            self.fp_status &= !FP_TO_PLACE;
        }
    }

    /// `IsConflicting()`: the `COURTYARD_CONFLICT` flag.
    pub fn is_conflicting(&self) -> bool {
        self.base.flags.has(ItemFlags::COURTYARD_CONFLICT)
    }

    /// `AllowMissingCourtyard()`.
    pub fn allow_missing_courtyard(&self) -> bool {
        self.allow_missing_courtyard
    }

    /// `AllowSolderMaskBridges()`.
    pub fn allow_solder_mask_bridges(&self) -> bool {
        self.allow_solder_mask_bridges
    }

    /// `GetDuplicatePadNumbersAreJumpers()`.
    pub fn duplicate_pad_numbers_are_jumpers(&self) -> bool {
        self.duplicate_pad_numbers_are_jumpers
    }

    /// `GetFootprintType()` (from the attributes).
    pub fn footprint_type(&self) -> FootprintType {
        if self.attributes & footprint_attr::FP_SMD != 0 {
            return FootprintType::Smd;
        }

        if self.attributes & footprint_attr::FP_THROUGH_HOLE != 0 {
            return FootprintType::ThroughHole;
        }

        FootprintType::Unspecified
    }

    /// `SetFootprintType()`.
    pub fn set_footprint_type(&mut self, mounting_style: FootprintType) {
        self.attributes &= !(footprint_attr::FP_THROUGH_HOLE | footprint_attr::FP_SMD);

        if mounting_style == FootprintType::ThroughHole {
            self.attributes |= footprint_attr::FP_THROUGH_HOLE;
        } else if mounting_style == FootprintType::Smd {
            self.attributes |= footprint_attr::FP_SMD;
        }
    }

    /// `GetTypeName()` (untranslated).
    pub fn type_name(&self) -> &'static str {
        match self.footprint_type() {
            FootprintType::Smd => "SMD",
            FootprintType::ThroughHole => "Through hole",
            FootprintType::Unspecified => "Unspecified",
        }
    }

    /// `GetLikelyAttribute()`: `FP_THROUGH_HOLE` with any plated through-hole pad, else
    /// `FP_SMD` with an SMD pad on copper, else 0 (fiducial, heatsink, castellated and
    /// mechanical pads are ignored).
    pub fn likely_attribute(&self, ctx: &BoardCtx<'_>) -> i32 {
        let mut smd_count = 0;
        let mut tht_count = 0;

        for &id in &self.pads {
            let Some(pad) = ctx.board.pad(id) else {
                continue;
            };

            match pad.property() {
                PadProp::FiducialGlbl | PadProp::FiducialLocal => continue,
                PadProp::Heatsink | PadProp::Castellated | PadProp::Mechanical => continue,
                PadProp::None | PadProp::Bga | PadProp::Testpoint | PadProp::Pressfit => {}
            }

            match pad.attribute() {
                PadAttrib::Pth => tht_count += 1,
                PadAttrib::Smd => {
                    if BoardItem::is_on_copper_layer(pad, ctx) {
                        smd_count += 1;
                    }
                }
                PadAttrib::NonComponent | PadAttrib::Conn | PadAttrib::Npth => {}
            }
        }

        // Footprints with plated through-hole pads should usually be marked through hole even
        // if they also have SMD because they might not be auto-placed.  Exceptions to this
        // might be shielded
        if tht_count > 0 {
            return footprint_attr::FP_THROUGH_HOLE;
        }

        if smd_count > 0 {
            return footprint_attr::FP_SMD;
        }

        0
    }

    /// `HasThroughHolePads()`: a PTH or NPTH pad.
    pub fn has_through_hole_pads(&self, ctx: &BoardCtx<'_>) -> bool {
        self.pads.iter().any(|&id| {
            ctx.board
                .pad(id)
                .is_some_and(|p| matches!(p.attribute(), PadAttrib::Pth | PadAttrib::Npth))
        })
    }

    /// `SetStackupMode()`: EXPAND_INNER_LAYERS resets the stackup layers to
    /// { F_Cu, In1_Cu, B_Cu }.
    pub fn set_stackup_mode(&mut self, mode: FootprintStackup) {
        self.stackup_mode = mode;

        if self.stackup_mode == FootprintStackup::ExpandInnerLayers {
            // Reset the stackup layers to the default values
            self.stackup_layers =
                Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::BCu]);
        }
    }

    /// `SetStackupLayers()`: only in CUSTOM_LAYERS mode (`wxCHECK2`).
    pub fn set_stackup_layers(&mut self, layers: Lset) {
        debug_assert!(self.stackup_mode == FootprintStackup::CustomLayers);

        if self.stackup_mode == FootprintStackup::CustomLayers {
            self.stackup_layers = layers;
        }
    }

    /// `GetLocalClearance( aSource )`: the override and, if set and asked for, its source text.
    pub fn local_clearance_with_source(&self, ctx: &BoardCtx<'_>) -> (Option<i32>, Option<String>) {
        let source = self
            .clearance
            .map(|_| format!("footprint {}", self.reference(ctx)));
        (self.clearance, source)
    }

    /// `GetZoneConnectionOverrides( aSource )`.
    pub fn zone_connection_overrides(
        &self,
        ctx: &BoardCtx<'_>,
    ) -> (ZoneConnection, Option<String>) {
        let source = (self.zone_connection != ZoneConnection::Inherited)
            .then(|| format!("footprint {}", self.reference(ctx)));
        (self.zone_connection, source)
    }

    /// `GetLocalClearance()`.
    pub fn local_clearance(&self) -> Option<i32> {
        self.clearance
    }

    /// `GetLocalZoneConnection()`.
    pub fn local_zone_connection(&self) -> ZoneConnection {
        self.zone_connection
    }

    /// `Add3DModel( a3DModel )`.
    pub fn add_3d_model(&mut self, model: Fp3dmodel) {
        self.models.push(model);
    }

    /// `GetSide()`: the footprint layer when a pad, drawing or zone is on a side-specific
    /// layer; `UNDEFINED_LAYER` for an unsided footprint and in a footprint holder.
    pub fn side(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        if board_item::has_board(self, ctx) && ctx.board.is_footprint_holder() {
            return PcbLayerId::UndefinedLayer;
        }

        let side_mask = Lset::side_specific_mask();

        // Test pads first; they're the most likely to return a quick answer.
        for &id in &self.pads {
            if let Some(item) = ctx.item(id.into()) {
                if (side_mask & &item.layer_set()).any() {
                    return self.base.layer;
                }
            }
        }

        for &id in &self.drawings {
            if let Some(item) = ctx.item(id) {
                let layer = item.layer();

                if is_valid_layer(layer) && side_mask.contains(layer) {
                    return self.base.layer;
                }
            }
        }

        for &id in &self.zones {
            if let Some(item) = ctx.item(id.into()) {
                if (side_mask & &item.layer_set()).any() {
                    return self.base.layer;
                }
            }
        }

        PcbLayerId::UndefinedLayer
    }

    /// `IsWithinSchematicSheet( aSheetPath )`: `m_path` (written by the netlist without the
    /// root sheet) starts with the sheet path minus its root.
    pub fn is_within_schematic_sheet(&self, sheet_path: &KiidPath) -> bool {
        if sheet_path.is_empty() {
            return false;
        }

        let sheet = &sheet_path[1..];
        self.path.len() + 1 >= sheet_path.len()
            && self.path.len() >= sheet.len()
            && self.path[..sheet.len()] == *sheet
    }

    // ---- variants (package M19) -----------------------------------------------------------

    /// `GetVariant( aVariantName )` (case-insensitive lookup).
    pub fn variant(&self, name: &str) -> Option<&FootprintVariant> {
        kicad_core::case_insensitive_map::ci_get(&self.variants, name)
    }

    /// `GetVariant( aVariantName )` (non-const).
    pub fn variant_mut(&mut self, name: &str) -> Option<&mut FootprintVariant> {
        kicad_core::case_insensitive_map::ci_get_mut(&mut self.variants, name)
    }

    /// `SetVariant( aVariant )`: ignored for an empty or the default name; an existing entry
    /// keeps its stored spelling.
    pub fn set_variant(&mut self, variant: &FootprintVariant) {
        if variant.name.is_empty() || same_no_case(&variant.name, &get_default_variant_name()) {
            return;
        }

        if let Some((key, value)) = self
            .variants
            .iter_mut()
            .find(|(k, _)| same_no_case(k.as_str(), &variant.name))
        {
            let mut updated = variant.clone();
            updated.name = key.as_str().to_string();
            *value = updated;
            return;
        }

        self.variants
            .insert(CaseInsensitiveKey(variant.name.clone()), variant.clone());
    }

    /// `AddVariant( aVariantName )`: the existing variant, or a new one initialised from the
    /// footprint's DNP / exclusion attributes. `None` (and a `wxASSERT`) for an empty or the
    /// default name.
    pub fn add_variant(&mut self, name: &str) -> Option<&mut FootprintVariant> {
        if name.is_empty() || same_no_case(name, &get_default_variant_name()) {
            debug_assert!(false, "Variant name cannot be empty or default.");
            return None;
        }

        if self.variant(name).is_none() {
            let mut variant = FootprintVariant::new(name);
            variant.dnp = self.is_dnp();
            variant.excluded_from_bom = self.is_excluded_from_bom();
            variant.excluded_from_sim = self.is_excluded_from_sim();
            variant.excluded_from_pos_files = self.is_excluded_from_pos_files();
            self.variants
                .insert(CaseInsensitiveKey(name.to_string()), variant);
        }

        self.variant_mut(name)
    }

    /// `DeleteVariant( aVariantName )` (case-insensitive).
    pub fn delete_variant(&mut self, name: &str) {
        self.variants.remove(&CaseInsensitiveKey(name.to_string()));
    }

    /// `RenameVariant( aOldName, aNewName )`: refused for an empty or the default new name, an
    /// unknown old name, or a new name taken by another variant; a no-op when the stored
    /// spelling already equals the new name.
    pub fn rename_variant(&mut self, old_name: &str, new_name: &str) {
        if new_name.is_empty() || same_no_case(new_name, &get_default_variant_name()) {
            return;
        }

        let old_key = CaseInsensitiveKey(old_name.to_string());
        let new_key = CaseInsensitiveKey(new_name.to_string());

        let Some((stored_old, _)) = self.variants.get_key_value(&old_key) else {
            return;
        };

        let stored_old = stored_old.as_str().to_string();

        // existingIt != it: a different entry under the new name
        if self.variants.contains_key(&new_key) && !same_no_case(&stored_old, new_name) {
            return;
        }

        if stored_old == new_name {
            return;
        }

        let Some(mut variant) = self.variants.remove(&old_key) else {
            return;
        };

        variant.name = new_name.to_string();
        self.variants.insert(new_key, variant);
    }

    /// `HasVariant( aVariantName )`.
    pub fn has_variant(&self, name: &str) -> bool {
        self.variant(name).is_some()
    }

    /// The variant to consult for `aVariantName`: `None` for the empty or default name and
    /// for an unknown variant (the callers fall back to the footprint attributes).
    fn variant_for_query(&self, variant_name: &str) -> Option<&FootprintVariant> {
        // Empty variant name means default
        if variant_name.is_empty() || same_no_case(variant_name, &get_default_variant_name()) {
            return None;
        }

        self.variant(variant_name)
    }

    /// `GetDNPForVariant( aVariantName )`.
    pub fn dnp_for_variant(&self, variant_name: &str) -> bool {
        self.variant_for_query(variant_name)
            .map_or_else(|| self.is_dnp(), |v| v.dnp)
    }

    /// `GetExcludedFromBOMForVariant( aVariantName )`.
    pub fn excluded_from_bom_for_variant(&self, variant_name: &str) -> bool {
        self.variant_for_query(variant_name)
            .map_or_else(|| self.is_excluded_from_bom(), |v| v.excluded_from_bom)
    }

    /// `GetExcludedFromSimForVariant( aVariantName )`.
    pub fn excluded_from_sim_for_variant(&self, variant_name: &str) -> bool {
        self.variant_for_query(variant_name)
            .map_or_else(|| self.is_excluded_from_sim(), |v| v.excluded_from_sim)
    }

    /// `GetExcludedFromPosFilesForVariant( aVariantName )`.
    pub fn excluded_from_pos_files_for_variant(&self, variant_name: &str) -> bool {
        self.variant_for_query(variant_name).map_or_else(
            || self.is_excluded_from_pos_files(),
            |v| v.excluded_from_pos_files,
        )
    }

    /// `GetFieldValueForVariant( aVariantName, aFieldName )`: the variant override if there
    /// is one, else the text of the field with that name, else empty.
    pub fn field_value_for_variant(
        &self,
        ctx: &BoardCtx<'_>,
        variant_name: &str,
        field_name: &str,
    ) -> String {
        // Check variant-specific override first
        if !variant_name.is_empty() && !same_no_case(variant_name, &get_default_variant_name()) {
            if let Some(variant) = self.variant(variant_name) {
                if variant.has_field_value(field_name) {
                    return variant.field_value(field_name);
                }
            }
        }

        // Fall back to default field value
        self.field_by_name(ctx, field_name)
            .and_then(|id| ctx.board.field(id))
            .map(|f| f.text.text.text().to_string())
            .unwrap_or_default()
    }

    // ---- text variables (package M19) -----------------------------------------------------

    /// `GetContextualTextVars( aVars )`.
    pub fn contextual_text_vars(&self, vars: &mut Vec<String>) {
        for var in [
            "REFERENCE",
            "VALUE",
            "LAYER",
            "FOOTPRINT_LIBRARY",
            "FOOTPRINT_NAME",
            "SHORT_NET_NAME(<pad_number>)",
            "NET_NAME(<pad_number>)",
            "NET_CLASS(<pad_number>)",
            "PIN_NAME(<pad_number>)",
            "EXCLUDE_FROM_BOM",
            "EXCLUDE_FROM_BOARD",
            "EXCLUDE_FROM_SIM",
            "EXCLUDE_FROM_POS_FILES",
            "DNP",
        ] {
            vars.push(var.to_string());
        }
    }

    /// `ResolveTextVar( token, aDepth )`: with the board's current variant.
    pub fn resolve_text_var(&self, ctx: &BoardCtx<'_>, token: &mut String, depth: i32) -> bool {
        let variant = if board_item::has_board(self, ctx) {
            ctx.board.get_current_variant().to_string()
        } else {
            String::new()
        };

        self.resolve_text_var_for_variant(ctx, token, &variant, depth)
    }

    /// `ResolveTextVar( token, aVariantName, aDepth )`: the footprint variables, the fields
    /// by name, then the board's variables. `${PROPERTY.<name>}` (the property manager) is not
    /// ported and is left unresolved here (see `PORT_STATUS.md`).
    pub fn resolve_text_var_for_variant(
        &self,
        ctx: &BoardCtx<'_>,
        token: &mut String,
        variant_name: &str,
        depth: i32,
    ) -> bool {
        let has_board = board_item::has_board(self, ctx);

        if has_board && ctx.board.board_use == BoardUse::FpHolder {
            return false;
        }

        let variant = variant_name;

        let shown_field = |field_type: FieldT| {
            self.field(ctx, field_type)
                .and_then(|id| ctx.board.field(id))
                .map(|f| {
                    f.shown_text_for_variant(ctx, variant, ResolutionContext::Internal, depth + 1)
                })
        };

        match token.as_str() {
            "REFERENCE" => {
                *token = shown_field(FieldT::Reference).unwrap_or_default();
                return true;
            }
            "VALUE" => {
                *token = shown_field(FieldT::Value).unwrap_or_default();
                return true;
            }
            "LAYER" => {
                *token = board_item::layer_name(self, ctx);
                return true;
            }
            "FOOTPRINT_LIBRARY" => {
                *token = self.fpid.lib_nickname().to_string();
                return true;
            }
            "FOOTPRINT_NAME" => {
                *token = self.fpid.lib_item_name().to_string();
                return true;
            }
            _ => {}
        }

        if token.starts_with("SHORT_NET_NAME(")
            || token.starts_with("NET_NAME(")
            || token.starts_with("NET_CLASS(")
            || token.starts_with("PIN_NAME(")
        {
            // AfterFirst( '(' ), then BeforeLast( ')' ) (the whole string when there is none)
            let after = token.split_once('(').map_or("", |(_, rest)| rest);
            let pad_number = after.rsplit_once(')').map_or(after, |(head, _)| head);

            for &id in &self.pads {
                let Some(pad) = ctx.board.pad(id) else {
                    continue;
                };

                if pad.number() == pad_number {
                    *token = if token.starts_with("SHORT_NET_NAME") {
                        board_connected_item::short_netname(pad, ctx).to_string()
                    } else if token.starts_with("NET_NAME") {
                        board_connected_item::netname(pad, ctx).to_string()
                    } else if token.starts_with("NET_CLASS") {
                        board_connected_item::net_class_name(pad, ctx)
                    } else {
                        pad.pin_function().to_string()
                    };

                    return true;
                }
            }
        } else if token == "EXCLUDE_FROM_BOM" {
            *token = if self.excluded_from_bom_for_variant(variant) {
                "Excluded from BOM".to_string()
            } else {
                String::new()
            };
            return true;
        } else if token == "EXCLUDE_FROM_POS_FILES" {
            *token = if self.excluded_from_pos_files_for_variant(variant) {
                "Excluded from position files".to_string()
            } else {
                String::new()
            };
            return true;
        } else if token == "EXCLUDE_FROM_BOARD" {
            // Footprints are never excluded from board by definition
            token.clear();
            return true;
        } else if token == "EXCLUDE_FROM_SIM" {
            *token = if self.excluded_from_sim_for_variant(variant) {
                "Excluded from simulation".to_string()
            } else {
                String::new()
            };
            return true;
        } else if token == "DNP" {
            *token = if self.dnp_for_variant(variant) {
                "DNP".to_string()
            } else {
                String::new()
            };
            return true;
        } else if let Some(field) = self
            .field_by_name(ctx, token)
            .and_then(|id| ctx.board.field(id))
        {
            *token =
                field.shown_text_for_variant(ctx, variant, ResolutionContext::Internal, depth + 1);
            return true;
        } else if token.starts_with("PROPERTY.") {
            // The great property resolver (PROPERTY_MANAGER): not ported, unresolved.
            return false;
        }

        if has_board && ctx.board.resolve_text_var(token, variant_name, depth + 1) {
            return true;
        }

        false
    }

    // ---- pads (package M19) ---------------------------------------------------------------

    /// `FindPadByNumber( aPadNumber, aSearchAfterMe )`: the first pad with this number (after
    /// `search_after_me` when given).
    pub fn find_pad_by_number(
        &self,
        ctx: &BoardCtx<'_>,
        pad_number: &str,
        search_after_me: Option<PadId>,
    ) -> Option<PadId> {
        let mut can_select = search_after_me.is_none();

        for &id in &self.pads {
            if !can_select && Some(id) == search_after_me {
                can_select = true;
                continue;
            }

            if can_select && ctx.board.pad(id).is_some_and(|p| p.number() == pad_number) {
                return Some(id);
            }
        }

        None
    }

    /// `FindPadByUuid( aUuid )`.
    pub fn find_pad_by_uuid(&self, ctx: &BoardCtx<'_>, uuid: &Kiid) -> Option<PadId> {
        self.pads.iter().copied().find(|&id| {
            ctx.board
                .pad(id)
                .is_some_and(|p| BoardItem::uuid(p) == uuid)
        })
    }

    /// `GetPad( aPosition, aLayerMask )`: the first pad on one of `layer_mask` hit at `pos`.
    pub fn get_pad_at(
        &self,
        ctx: &BoardCtx<'_>,
        pos: Vector2I,
        layer_mask: &Lset,
    ) -> Option<PadId> {
        for &id in &self.pads {
            let Some(pad) = ctx.board.pad(id) else {
                continue;
            };

            // ... and on the correct layer.
            if !(&pad.layer_set(ctx) & layer_mask).any() {
                continue;
            }

            if pad.hit_test(ctx, pos, 0) {
                return Some(id);
            }
        }

        None
    }

    /// `GetPads( aPadNumber, aIgnore )`: every pad with this number except `ignore`.
    pub fn pads_by_number(
        &self,
        ctx: &BoardCtx<'_>,
        pad_number: &str,
        ignore: Option<PadId>,
    ) -> Vec<PadId> {
        self.pads
            .iter()
            .copied()
            .filter(|&id| {
                Some(id) != ignore && ctx.board.pad(id).is_some_and(|p| p.number() == pad_number)
            })
            .collect()
    }

    /// `GetPadCount()`.
    pub fn pad_count(&self) -> u32 {
        self.pads.len() as u32
    }

    /// `GetUniquePadNumbers()`: the numbers of the named pads on copper.
    pub fn unique_pad_numbers(&self, ctx: &BoardCtx<'_>) -> BTreeSet<String> {
        let mut used_numbers = BTreeSet::new();

        // Create a set of used pad numbers
        for &id in &self.pads {
            let Some(pad) = ctx.board.pad(id) else {
                continue;
            };

            // Skip pads not on copper layers (used to build complex
            // solder paste shapes for instance)
            if (&pad.layer_set(ctx) & Lset::all_cu_mask()).none() {
                continue;
            }

            // Skip pads with no name, because they are usually "mechanical"
            // pads, not "electrical" pads
            if pad.number().is_empty() {
                continue;
            }

            used_numbers.insert(pad.number().to_string());
        }

        used_numbers
    }

    /// `GetUniquePadCount()`: the number of unique pad numbers.
    pub fn unique_pad_count(&self, ctx: &BoardCtx<'_>) -> u32 {
        self.unique_pad_numbers(ctx).len() as u32
    }

    /// `GetNumberedPadCount()`: the unique "electrical" pad numbers on copper (non-NPTH):
    /// numeric, up to two letters plus digits, or two such joined by `_` (counted twice).
    pub fn numbered_pad_count(&self, ctx: &BoardCtx<'_>) -> u32 {
        let is_electrical_pad_number = |num: &str| -> bool {
            if num.is_empty() {
                return false;
            }

            let chars: Vec<char> = num.chars().collect();

            // Walk past an optional alphabetic prefix of at most two characters.
            let mut i = 0;

            while i < chars.len() && wx_is_alpha(chars[i]) {
                i += 1;
            }

            // Prefix must be 0-2 letters; anything longer is not a pin number.
            if i > 2 {
                return false;
            }

            // The remainder must be non-empty and consist entirely of digits.
            if i == chars.len() {
                return false; // no digits at all (e.g. "MP", "GND")
            }

            chars[i..].iter().all(|&c| wx_is_digit(c))
        };

        let mut counted = BTreeSet::new();

        for &id in &self.pads {
            let Some(pad) = ctx.board.pad(id) else {
                continue;
            };

            // Must be on at least one copper layer.
            if (&pad.layer_set(ctx) & Lset::all_cu_mask()).none() {
                continue;
            }

            // Skip NPTH (mechanical holes).
            if pad.attribute() == PadAttrib::Npth {
                continue;
            }

            let num = pad.number();

            if is_electrical_pad_number(num) {
                counted.insert(num.to_string());
            } else if num.contains('_') {
                let (first, second) = num.split_once('_').unwrap_or((num, ""));

                if is_electrical_pad_number(first) && is_electrical_pad_number(second) {
                    counted.insert(first.to_string());
                    counted.insert(second.to_string());
                }
            }
        }

        counted.len() as u32
    }

    /// `GetNextPadNumber( aLastPadNumber )`: the prefix of `last_pad_number` with the first
    /// unused number from its trailing integer on.
    pub fn next_pad_number(&self, ctx: &BoardCtx<'_>, last_pad_number: &str) -> String {
        let used_numbers: BTreeSet<&str> = self
            .pads
            .iter()
            .filter_map(|&id| ctx.board.pad(id).map(|p| p.number()))
            .collect();

        // Pad numbers aren't technically reference designators, but the formatting is close
        // enough for these to give us what we need.
        let prefix = get_ref_des_prefix(last_pad_number);
        let mut num = get_trailing_int(last_pad_number);

        while used_numbers.contains(format!("{prefix}{num}").as_str()) {
            num = num.wrapping_add(1);
        }

        format!("{prefix}{num}")
    }

    // ---- net ties and jumpers (package M19) ---------------------------------------------

    /// `GetNetTieCache( aItem )`: the net codes a footprint item may short (empty if none).
    pub fn net_tie_cache(&self, item: ItemId) -> &BTreeSet<NetCode> {
        static EMPTY: BTreeSet<NetCode> = BTreeSet::new();
        self.net_tie_cache.get(&item).unwrap_or(&EMPTY)
    }

    /// `IsNetTie()`: the footprint has a non-empty net-tie pad group.
    pub fn is_net_tie(&self) -> bool {
        self.net_tie_pad_groups
            .iter()
            .any(|group| !group.is_empty())
    }

    /// `MapPadNumbersToNetTieGroups()`: pad number -> group index (-1: none). Every pad
    /// number of the footprint is a key; the groups are comma-separated lists of pad numbers
    /// (`\` escapes the next character), each entry trimmed.
    pub fn map_pad_numbers_to_net_tie_groups(&self, ctx: &BoardCtx<'_>) -> BTreeMap<String, i32> {
        let mut pad_number_to_group_idx_map = BTreeMap::new();

        for &id in &self.pads {
            if let Some(pad) = ctx.board.pad(id) {
                pad_number_to_group_idx_map.insert(pad.number().to_string(), -1);
            }
        }

        for (ii, group) in self.net_tie_pad_groups.iter().enumerate() {
            let group_idx = ii as i32;
            let mut process_pad = |pad: &str| {
                let pad = wx_trim(pad);

                if !pad.is_empty() {
                    pad_number_to_group_idx_map.insert(pad.to_string(), group_idx);
                }
            };

            let mut esc = false;
            let mut pad = String::new();

            for ch in group.chars() {
                if esc {
                    esc = false;
                    pad.push(ch);
                    continue;
                }

                match ch {
                    '\\' => esc = true,
                    ',' => {
                        process_pad(&pad);
                        pad.clear();
                    }
                    _ => pad.push(ch),
                }
            }

            process_pad(&pad);
        }

        pad_number_to_group_idx_map
    }

    /// `GetNetTiePads( aPad )`: the footprint's pads in the net-tie group of `pad` (`pad`
    /// included); empty when it is in none. A number missing from the map counts as group 0
    /// (`std::map::operator[]`).
    pub fn net_tie_pads(&self, ctx: &BoardCtx<'_>, pad: PadId) -> Vec<PadId> {
        // First build a map from pad numbers to allowed-shorting-group indexes.  This ends up
        // being something like O(3n), but it still beats O(n^2) for large numbers of pads.
        let pad_to_net_tie_group_map = self.map_pad_numbers_to_net_tie_groups(ctx);
        let group_of = |number: &str| pad_to_net_tie_group_map.get(number).copied().unwrap_or(0);
        let group_idx = ctx.board.pad(pad).map_or(0, |p| group_of(p.number()));
        let mut other_pads = Vec::new();

        if group_idx >= 0 {
            for &id in &self.pads {
                if ctx
                    .board
                    .pad(id)
                    .is_some_and(|p| group_of(p.number()) == group_idx)
                {
                    other_pads.push(id);
                }
            }
        }

        other_pads
    }

    /// `JumperPadGroups().FindContaining( aPadNumber )`: the jumper group of a pad number.
    pub fn jumper_pad_group(&self, pad_number: &str) -> Option<&BTreeSet<String>> {
        self.jumper_pad_groups
            .find_containing(pad_number)
            .map(|group| &group.names)
    }

    // ---- component class (package M19) ----------------------------------------------------

    /// `GetStaticComponentClass()`.
    pub fn static_component_class(&self) -> Option<&Arc<ComponentClass>> {
        self.static_component_class.as_ref()
    }

    /// `GetComponentClass()` (`COMPONENT_CLASS_CACHE_PROXY::GetComponentClass`) from `&self`:
    /// the cached final class while the manager's ticker has not moved, else the class
    /// recomputed against a copy of the board's manager (the effective class the C++ registers
    /// in the manager on the way is not kept; use `Board::footprint_component_class` to keep
    /// it). The dynamic assignment rules are matched with their compiled DRC conditions
    /// (`COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`); the cache cell is not held while they are
    /// evaluated.
    pub fn component_class(&self, ctx: &BoardCtx<'_>) -> Option<Arc<ComponentClass>> {
        let manager = ctx.board.component_class_manager();

        let cached = self.component_class_cache.with(|cache| {
            (manager.ticker <= cache.last_ticker_value).then(|| cache.final_component_class.clone())
        });

        if let Some(final_class) = cached {
            return final_class;
        }

        let mut scratch = manager.clone();
        let mut cache = self.component_class_cache.with(|cache| cache.clone());
        let result = scratch.get_component_class(
            &mut cache,
            self.static_component_class.as_deref(),
            |rule| rule.matches(self, ctx),
        );
        self.component_class_cache.with(|c| *c = cache);
        result
    }

    /// `GetComponentClassAsString()`: the class name, empty for the empty class.
    pub fn component_class_as_string(&self, ctx: &BoardCtx<'_>) -> String {
        match self.component_class(ctx) {
            Some(class) if !class.is_empty() => class.name().to_string(),
            _ => String::new(),
        }
    }

    /// `InvalidateComponentClassCache()`.
    pub fn invalidate_component_class_cache(&mut self) {
        self.component_class_cache.clear();
    }

    /// `IsLibNameValid( aName )`: no character of `GetLibFilenameForbiddenChars()`.
    pub fn is_lib_name_valid(name: &str) -> bool {
        let invalids = string_lib_name_invalid_chars(false);
        !name.chars().any(|c| invalids.contains(c))
    }
}

/// `FOOTPRINT::StringLibNameInvalidChars( aUserReadable )`.
pub fn string_lib_name_invalid_chars(user_readable: bool) -> &'static str {
    if user_readable {
        "% $ < > 'tab' 'return' 'line feed' \\ \" / :"
    } else {
        get_lib_filename_forbidden_chars()
    }
}

/// `aName.CmpNoCase( aOther ) == 0`.
fn same_no_case(a: &str, b: &str) -> bool {
    case_insensitive_cmp(a, b).is_eq()
}

/// `wxString::Trim( true ).Trim( false )`: strips `wxSafeIsspace` characters (ASCII space,
/// `\t`, `\n`, `\v`, `\f`, `\r`) from both ends.
fn wx_trim(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

/// `wxIsalpha` in the C locale.
fn wx_is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// `wxIsdigit` in the C locale.
fn wx_is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// `UTIL::GetRefDesPrefix( aRefDes )` (`common/refdes_utils.cpp`): the string without its
/// trailing digits and question marks.
pub fn get_ref_des_prefix(ref_des: &str) -> &str {
    // find the first non-digit, non-question-mark character from the back
    let end = ref_des
        .char_indices()
        .rev()
        .find(|&(_, c)| c != '?' && !c.is_ascii_digit())
        .map_or(0, |(i, c)| i + c.len_utf8());

    &ref_des[..end]
}

impl JumperGroup {
    /// `JUMPER_GROUP::Make( aNames )`: blank names dropped; `None` when nothing is left.
    pub fn make(mut names: BTreeSet<String>) -> Option<JumperGroup> {
        // A blank name can never resolve to a pin or pad.
        names.retain(|name| !name.is_empty());

        if names.is_empty() {
            return None;
        }

        Some(JumperGroup { names })
    }

    /// `JUMPER_GROUP::Contains( aName )`.
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// `GetNames()`.
    pub fn names(&self) -> &BTreeSet<String> {
        &self.names
    }
}

impl JumperGroupSet {
    /// `JUMPER_GROUP_SET::Add( JUMPER_GROUP )`.
    pub fn add(&mut self, group: JumperGroup) {
        self.groups.push(group);
    }

    /// `JUMPER_GROUP_SET::Add( std::set<wxString> )`: through `JUMPER_GROUP::Make`.
    pub fn add_names(&mut self, names: BTreeSet<String>) {
        if let Some(group) = JumperGroup::make(names) {
            self.add(group);
        }
    }

    /// `Clear()`.
    pub fn clear(&mut self) {
        self.groups.clear();
    }

    /// `GetAll()`.
    pub fn all(&self) -> &[JumperGroup] {
        &self.groups
    }

    /// `IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// `FindContaining( aName )`: the first group holding `name`.
    pub fn find_containing(&self, name: &str) -> Option<&JumperGroup> {
        self.groups.iter().find(|group| group.contains(name))
    }
}

impl Board {
    /// `FOOTPRINT::BuildNetTieCache()`: cache, per footprint item, the net codes it may
    /// short through the net-tie pad groups (derived state: no invalidation, no time stamp).
    /// The pads of one group map to each other's nets; a copper `PCB_SHAPE` of the footprint
    /// (on an enabled layer) touching such a pad inherits the pad's set.
    pub fn build_net_tie_cache(&mut self, fp: FootprintId) {
        let Some(cache) = self.compute_net_tie_cache(fp) else {
            return;
        };

        if let Some(footprint) = self.footprints.get_mut(fp) {
            footprint.net_tie_cache = cache;
        }
    }

    /// The body of `BuildNetTieCache` against the unmodified board.
    fn compute_net_tie_cache(
        &self,
        fp: FootprintId,
    ) -> Option<BTreeMap<ItemId, BTreeSet<NetCode>>> {
        let ctx = BoardCtx::bare(self);
        let footprint = self.footprints.get(fp)?;
        let mut cache: BTreeMap<ItemId, BTreeSet<NetCode>> = BTreeMap::new();
        let map = footprint.map_pad_numbers_to_net_tie_groups(&ctx);
        let mut layer_shapes: BTreeMap<PcbLayerId, Vec<ItemId>> = BTreeMap::new();
        let has_board = board_item::has_board(footprint, &ctx);
        let enabled_layers = self.enabled_layers();

        for &item in &footprint.drawings {
            if item.kind() != ItemKind::Shape {
                continue;
            }

            let Some(item_ref) = ctx.item(item) else {
                continue;
            };

            for layer in item_ref.layer_set().iter() {
                if !is_copper_layer(layer) {
                    continue;
                }

                if has_board && !enabled_layers.contains(layer) {
                    continue;
                }

                layer_shapes.entry(layer).or_default().push(item);
            }
        }

        let pads: Vec<PadId> = footprint.pads.iter().copied().collect();
        let group_of = |id: PadId| {
            self.pads
                .get(id)
                .and_then(|p| map.get(p.number()).copied())
                .filter(|&g| g >= 0)
        };
        let net_of = |id: PadId| {
            self.pads
                .get(id)
                .map_or(NetCode::ORPHANED, BoardConnectedItem::net_code)
        };

        for (ii, &pad) in pads.iter().enumerate() {
            let mut has_nettie = false;

            let Some(group) = group_of(pad) else {
                continue;
            };

            for (jj, &other) in pads.iter().enumerate() {
                if jj == ii {
                    continue;
                }

                let Some(other_group) = group_of(other) else {
                    continue;
                };

                if other_group == group {
                    let pad_net = net_of(pad);
                    let other_net = net_of(other);
                    let pad_set = cache.entry(pad.into()).or_default();
                    pad_set.insert(pad_net);
                    pad_set.insert(other_net);
                    let other_set = cache.entry(other.into()).or_default();
                    other_set.insert(other_net);
                    other_set.insert(pad_net);
                    has_nettie = true;
                }
            }

            if !has_nettie {
                continue;
            }

            let Some(pad_ref) = self.pads.get(pad) else {
                continue;
            };

            for (&layer, shapes) in &layer_shapes {
                let pad_shape = BoardItem::effective_shape(
                    pad_ref,
                    &ctx,
                    layer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );

                for &other_shape in shapes {
                    let Some(shape_ref) = ctx.item(other_shape) else {
                        continue;
                    };
                    let shape = shape_ref.effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );

                    if pad_shape.collide(&*shape, 0, None, None) {
                        let nettie = cache.entry(pad.into()).or_default().clone();
                        cache.entry(other_shape).or_default().extend(nettie);
                    }
                }
            }
        }

        Some(cache)
    }

    /// `FOOTPRINT::GetComponentClass()` with the manager updated in place (the effective
    /// class the C++ creates on the way is registered like C++). `rule_matches` is
    /// `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches( footprint )` (the compiled DRC condition,
    /// kicad-drc).
    pub fn footprint_component_class(
        &mut self,
        fp: FootprintId,
        rule_matches: impl FnMut(&crate::component_class::ComponentClassAssignmentRule) -> bool,
    ) -> Option<Arc<ComponentClass>> {
        let footprint = self.footprints.get(fp)?;
        let static_class = footprint.static_component_class.clone();
        let mut cache = footprint.component_class_cache.with(|c| c.clone());
        let result = self.component_class_manager.get_component_class(
            &mut cache,
            static_class.as_deref(),
            rule_matches,
        );

        if let Some(footprint) = self.footprints.get(fp) {
            footprint.component_class_cache.with(|c| *c = cache);
        }

        result
    }

    /// `FOOTPRINT::FixUuids()`: replace the nil UUIDs of the fields, pads, drawings, groups,
    /// zones and points by fresh ones (`ResetUuidDirect`); true when one was replaced. Goes
    /// through `rebind_item_uuid`, which writes the UUID directly when the old one is not
    /// indexed (the parser's case: an unattached footprint) and re-binds it otherwise.
    pub fn footprint_fix_uuids(&mut self, fp: FootprintId) -> bool {
        let Some(footprint) = self.footprints.get(fp) else {
            return false;
        };

        // replace null UUIDs if any by a valid uuid
        let mut item_list: Vec<ItemId> = Vec::new();
        item_list.extend(footprint.fields.iter().map(|&id| ItemId::from(id)));
        item_list.extend(footprint.pads.iter().map(|&id| ItemId::from(id)));
        item_list.extend(footprint.drawings.iter().copied());
        // Note: one cannot fix null UUIDs inside the group, but it should not happen
        // because null uuids can be found in old footprints, therefore without group
        item_list.extend(footprint.groups.iter().map(|&id| ItemId::from(id)));
        // Probably not needed, because old fp do not have zones. But just in case.
        item_list.extend(footprint.zones.iter().map(|&id| ItemId::from(id)));
        // Ditto
        item_list.extend(footprint.points.iter().map(|&id| ItemId::from(id)));

        let mut changed = false;

        for item in item_list {
            if self.uuid_of(item) == Some(Kiid::nil()) {
                let new_uuid = Kiid::new(&mut self.kiid_generator);
                self.rebind_item_uuid(item, new_uuid);
                changed = true;
            }
        }

        changed
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

    /// A board with one attached footprint (`new FOOTPRINT( &board ); board.Add( fp )`).
    fn board_with_fp() -> (Board, FootprintId) {
        let mut board = Board::new();
        let fp = board.create_footprint();
        board.attach_footprint(fp, crate::board::AddMode::Append);
        (board, fp)
    }

    /// A pad with `number` on `layers` (APPEND).
    fn add_pad(
        board: &mut Board,
        fp: FootprintId,
        number: &str,
        layers: Lset,
        net: NetCode,
    ) -> PadId {
        let env = board.child_env(crate::item::ParentRef::Item(fp.into()));
        let mut pad = crate::pad::Pad::new(&mut board.kiid_generator);
        BoardItem::base_mut(&mut pad).parent = crate::item::ParentRef::Item(fp.into());
        pad.set_number(number);
        pad.set_layer_set(&env, layers);
        pad.connected_mut().expect("connected item").net = net;
        board
            .footprint_add(
                fp,
                crate::item_ref::OwnedItem::Pad(Box::new(pad)),
                crate::board::AddMode::Append,
            )
            .unwrap_or_else(|_| panic!("pad refused"))
            .typed()
            .unwrap()
    }

    /// test_variant.cpp `FootprintVariantBasics`.
    #[test]
    fn footprint_variant_basics() {
        let mut variant = FootprintVariant::new("TestVariant");
        assert_eq!(variant.name, "TestVariant");
        assert!(!variant.dnp && !variant.excluded_from_bom);
        assert!(!variant.excluded_from_sim && !variant.excluded_from_pos_files);
        assert!(variant.fields.is_empty());
        variant.dnp = true;
        variant.set_field_value("Value", "100R");
        assert!(variant.has_field_value("Value"));
        assert_eq!(variant.field_value("Value"), "100R");
    }

    /// test_variant.cpp `FootprintDNPForVariant`, `VariantCaseInsensitivity`,
    /// `EmptyVariantReturnsBase`, `UnknownVariantReturnsBase` and the BOM / sim / position
    /// file exclusion cases.
    #[test]
    fn footprint_dnp_and_exclusions_for_variant() {
        let mut fp = Footprint::default();
        fp.set_attributes(0);
        assert!(!fp.dnp_for_variant("") && !fp.dnp_for_variant("Production"));
        fp.set_attributes(footprint_attr::FP_DNP);
        assert!(fp.dnp_for_variant("") && fp.dnp_for_variant("Production"));

        let mut prod = FootprintVariant::new("Production");
        prod.dnp = false;
        prod.excluded_from_bom = true;
        prod.excluded_from_sim = true;
        prod.excluded_from_pos_files = true;
        fp.set_variant(&prod);

        assert!(fp.dnp_for_variant(""));
        assert!(fp.dnp_for_variant("< Default >"));
        assert!(!fp.dnp_for_variant("Production"));
        assert!(!fp.dnp_for_variant("production"));
        assert!(!fp.dnp_for_variant("PrOdUcTiOn"));
        assert!(fp.dnp_for_variant("Debug"));
        assert!(fp.dnp_for_variant("NonExistentVariant"));
        assert!(
            fp.excluded_from_bom_for_variant("PRODUCTION") && !fp.excluded_from_bom_for_variant("")
        );
        assert!(
            fp.excluded_from_sim_for_variant("Production") && !fp.excluded_from_sim_for_variant("")
        );
        assert!(fp.excluded_from_pos_files_for_variant("Production"));
        assert!(!fp.excluded_from_pos_files_for_variant("Debug"));

        // SetVariant on an existing entry keeps the stored spelling
        let mut lower = FootprintVariant::new("production");
        lower.dnp = true;
        fp.set_variant(&lower);
        assert_eq!(fp.variants.len(), 1);
        assert_eq!(fp.variant("PRODUCTION").unwrap().name, "Production");
        assert!(fp.dnp_for_variant("Production"));

        // the default name and the empty name are refused
        fp.set_variant(&FootprintVariant::new("< Default >"));
        fp.set_variant(&FootprintVariant::new(""));
        assert_eq!(fp.variants.len(), 1);
    }

    /// test_variant.cpp `RenameVariantPreservesData` / `VariantDeletionClearsRegistry`
    /// (footprint side) and `AddVariant`.
    #[test]
    fn add_rename_delete_variant() {
        let mut fp = Footprint::default();
        fp.set_attributes(footprint_attr::FP_DNP | footprint_attr::FP_EXCLUDE_FROM_SIM);
        let added = fp.add_variant("Alpha").unwrap();
        assert!(added.dnp && added.excluded_from_sim && !added.excluded_from_bom);
        added.set_field_value("Value", "1k");
        assert!(fp.add_variant("ALPHA").is_some());
        assert_eq!(fp.variants.len(), 1);

        fp.add_variant("Beta");
        fp.rename_variant("alpha", "Beta"); // taken by another variant: refused
        assert!(fp.has_variant("Alpha"));
        fp.rename_variant("alpha", "< Default >"); // refused
        fp.rename_variant("missing", "Gamma"); // unknown: nothing
        fp.rename_variant("alpha", "ALPHA"); // same entry, new spelling
        assert_eq!(fp.variant("alpha").unwrap().name, "ALPHA");
        fp.rename_variant("ALPHA", "Gamma");
        assert!(!fp.has_variant("Alpha"));
        let gamma = fp.variant("gamma").unwrap();
        assert_eq!(
            (gamma.name.as_str(), gamma.field_value("Value").as_str()),
            ("Gamma", "1k")
        );

        fp.delete_variant("GAMMA");
        assert!(!fp.has_variant("Gamma") && fp.has_variant("Beta"));
    }

    /// test_variant.cpp `FootprintVariantTextVarResolution` and the field-value fallback of
    /// `GetFieldValueForVariant`.
    #[test]
    fn footprint_variant_text_var_resolution() {
        let (mut board, fp) = board_with_fp();
        board.add_variant("Production");
        board.set_current_variant("Production");
        let ctx = BoardCtx::bare(&board);
        let value_field = board[fp].field(&ctx, FieldT::Value).unwrap();
        board.modify_field(value_field, |f, _| f.text.text.set_text("10k"));
        board.modify_footprint(fp, |f, _| {
            f.set_attributes(0);
            let mut prod = FootprintVariant::new("Production");
            prod.dnp = true;
            prod.excluded_from_sim = true;
            prod.set_field_value("Value", "22k");
            f.set_variant(&prod);
        });

        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];
        let resolve = |token: &str, variant: Option<&str>| {
            let mut t = token.to_string();
            let ok = match variant {
                Some(v) => f.resolve_text_var_for_variant(&ctx, &mut t, v, 0),
                None => f.resolve_text_var(&ctx, &mut t, 0),
            };
            (ok, t)
        };

        assert_eq!(resolve("DNP", None), (true, "DNP".to_string()));
        assert_eq!(resolve("DNP", Some("")), (true, String::new()));
        assert_eq!(
            resolve("EXCLUDE_FROM_SIM", None),
            (true, "Excluded from simulation".to_string())
        );
        assert_eq!(resolve("EXCLUDE_FROM_SIM", Some("")), (true, String::new()));
        assert_eq!(resolve("EXCLUDE_FROM_BOARD", None), (true, String::new()));
        assert_eq!(resolve("EXCLUDE_FROM_BOM", None), (true, String::new()));
        assert_eq!(resolve("LAYER", None), (true, "F.Cu".to_string()));
        assert!(!resolve("NOT_A_VAR", Some("")).0);
        assert_eq!(
            f.field_value_for_variant(&ctx, "production", "Value"),
            "22k"
        );
        assert_eq!(f.field_value_for_variant(&ctx, "", "Value"), "10k");
        assert_eq!(f.field_value_for_variant(&ctx, "Debug", "Value"), "10k");
        assert_eq!(f.field_value_for_variant(&ctx, "Production", "Missing"), "");
    }

    /// Fields: mandatory lookup, by name, ordinals, `GetNextFieldOrdinal`, descriptions.
    #[test]
    fn fields_and_descriptions() {
        let (mut board, fp) = board_with_fp();
        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];
        assert!(f.field(&ctx, FieldT::Reference).is_some());
        assert_eq!(f.field_by_name(&ctx, "Value"), f.field(&ctx, FieldT::Value));
        assert!(f.has_field(&ctx, "Datasheet") && !f.has_field(&ctx, "datasheet"));
        assert_eq!(f.next_field_ordinal(&ctx), 42);
        assert_eq!(f.reference(&ctx), "");
        let units = UnitsProvider::new(
            &kicad_core::base_units::PCB_IU_SCALE,
            kicad_core::EdaUnits::Mm,
        );
        assert_eq!(
            f.item_description(&ctx, &units, true),
            "Footprint <no reference designator>"
        );
        let sorted = f.fields_sorted(&ctx, false);
        assert_eq!(sorted.len(), 4);
        // ordinals 1 (Reference), 2 (Value), 4 (Datasheet), 5 (Description)
        assert_eq!(sorted[0], f.field(&ctx, FieldT::Reference).unwrap());

        let reference = f.field(&ctx, FieldT::Reference).unwrap();
        board.modify_field(reference, |x, _| x.text.text.set_text("U7?"));
        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];
        assert_eq!(f.reference(&ctx), "U7?");
        assert!(!f.is_annotated(&ctx));
        assert_eq!(f.item_description(&ctx, &units, false), "Footprint U7?");
        // only the visible, non-empty fields: the reference (the value is empty)
        assert_eq!(f.fields_sorted(&ctx, true), vec![reference]);
    }

    /// Pad queries: FindPadByNumber (incl. search-after), FindPadByUuid, GetPads,
    /// GetUniquePadNumbers, GetNumberedPadCount, GetNextPadNumber, GetSide.
    #[test]
    fn pad_queries() {
        let (mut board, fp) = board_with_fp();
        let cu = Lset::from_layers(&[PcbLayerId::FCu]);
        let p1 = add_pad(&mut board, fp, "1", cu.clone(), NetCode(0));
        let p2 = add_pad(&mut board, fp, "2", cu.clone(), NetCode(0));
        let p2b = add_pad(&mut board, fp, "2", cu.clone(), NetCode(0));
        add_pad(&mut board, fp, "", cu.clone(), NetCode(0));
        add_pad(&mut board, fp, "A12_B1", cu.clone(), NetCode(0));
        add_pad(&mut board, fp, "MP", cu.clone(), NetCode(0));
        add_pad(
            &mut board,
            fp,
            "PASTE",
            Lset::from_layers(&[PcbLayerId::FPaste]),
            NetCode(0),
        );
        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];

        assert_eq!(f.find_pad_by_number(&ctx, "2", None), Some(p2));
        assert_eq!(f.find_pad_by_number(&ctx, "2", Some(p2)), Some(p2b));
        assert_eq!(f.find_pad_by_number(&ctx, "1", Some(p2)), None);
        let uuid = *BoardItem::uuid(&board[p1]);
        assert_eq!(f.find_pad_by_uuid(&ctx, &uuid), Some(p1));
        assert_eq!(f.pads_by_number(&ctx, "2", Some(p2)), vec![p2b]);
        assert_eq!(f.pad_count(), 7);
        let unique: Vec<String> = f.unique_pad_numbers(&ctx).into_iter().collect();
        assert_eq!(unique, ["1", "2", "A12_B1", "MP"]);
        assert_eq!(f.unique_pad_count(&ctx), 4);
        // "1", "2", "A12", "B1"
        assert_eq!(f.numbered_pad_count(&ctx), 4);
        assert_eq!(f.next_pad_number(&ctx, "1"), "3");
        assert_eq!(f.next_pad_number(&ctx, "MP"), "MP0");
        assert_eq!(f.next_pad_number(&ctx, "A7"), "A7");
        assert_eq!(f.side(&ctx), PcbLayerId::FCu);
        assert_eq!(f.likely_attribute(&ctx), footprint_attr::FP_THROUGH_HOLE);
        assert!(f.has_through_hole_pads(&ctx));

        let (board2, fp2) = board_with_fp();
        let ctx2 = BoardCtx::bare(&board2);
        // the mandatory fields are on F.SilkS / F.Fab, which are side specific, but GetSide
        // only looks at pads, drawings and zones
        assert_eq!(board2[fp2].side(&ctx2), PcbLayerId::UndefinedLayer);
        assert_eq!(board2[fp2].likely_attribute(&ctx2), 0);
    }

    /// `MapPadNumbersToNetTieGroups`, `GetNetTiePads`, `IsNetTie`, `BuildNetTieCache` (pads
    /// only) and the jumper groups.
    #[test]
    fn net_tie_groups_and_jumpers() {
        let (mut board, fp) = board_with_fp();
        let cu = Lset::from_layers(&[PcbLayerId::FCu]);
        let p1 = add_pad(&mut board, fp, "1", cu.clone(), NetCode(1));
        let p2 = add_pad(&mut board, fp, "2", cu.clone(), NetCode(2));
        let p3 = add_pad(&mut board, fp, "3", cu.clone(), NetCode(3));
        let pc = add_pad(&mut board, fp, "a,b", cu.clone(), NetCode(4));
        board.modify_footprint(fp, |f, _| {
            f.net_tie_pad_groups = vec![String::new()];
        });
        assert!(!board[fp].is_net_tie());
        board.modify_footprint(fp, |f, _| {
            f.net_tie_pad_groups = vec![" 1 ,\t2".to_string(), "a\\,b, X ,".to_string()];
        });

        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];
        assert!(f.is_net_tie());
        let map: Vec<(String, i32)> = f
            .map_pad_numbers_to_net_tie_groups(&ctx)
            .into_iter()
            .collect();
        let expected: Vec<(String, i32)> = [("1", 0), ("2", 0), ("3", -1), ("X", 1), ("a,b", 1)]
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();
        assert_eq!(map, expected);
        assert_eq!(f.net_tie_pads(&ctx, p1), vec![p1, p2]);
        assert_eq!(f.net_tie_pads(&ctx, p3), Vec::<PadId>::new());
        assert_eq!(f.net_tie_pads(&ctx, pc), vec![pc]);

        board.build_net_tie_cache(fp);
        let f = &board[fp];
        let nets = |id: PadId| {
            f.net_tie_cache(id.into())
                .iter()
                .map(|n| n.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(nets(p1), vec![1, 2]);
        assert_eq!(nets(p2), vec![1, 2]);
        assert!(nets(p3).is_empty() && nets(pc).is_empty());

        let mut jumpers = JumperGroupSet::default();
        jumpers.add_names(["".to_string()].into_iter().collect());
        assert!(jumpers.is_empty());
        jumpers.add_names(
            ["1".to_string(), "3".to_string(), String::new()]
                .into_iter()
                .collect(),
        );
        assert_eq!(jumpers.all().len(), 1);
        assert_eq!(jumpers.all()[0].names().len(), 2);
        board.modify_footprint(fp, |f, _| f.jumper_pad_groups = jumpers.clone());
        assert!(
            board[fp]
                .jumper_pad_group("3")
                .is_some_and(|g| g.contains("1"))
        );
        assert!(board[fp].jumper_pad_group("2").is_none());
    }

    /// `RunOnChildren` order, `IsOnLayer`, `FixUuids`, the attribute setters,
    /// `IsLibNameValid`, `GetRefDesPrefix`, `IsWithinSchematicSheet`.
    #[test]
    fn children_layers_uuids_and_helpers() {
        let (mut board, fp) = board_with_fp();
        let pad = add_pad(
            &mut board,
            fp,
            "1",
            Lset::from_layers(&[PcbLayerId::BCu]),
            NetCode(0),
        );
        let ctx = BoardCtx::bare(&board);
        let f = &board[fp];
        let mut kinds = Vec::new();
        f.run_on_children(&ctx, RecurseMode::Recurse, &mut |item| {
            kinds.push(item.type_())
        });
        assert_eq!(kinds.len(), 5);
        assert_eq!(kinds[4], KicadT::PcbPadT);
        assert!(f.is_on_layer(&ctx, PcbLayerId::BCu)); // the pad
        assert!(f.is_on_layer(&ctx, PcbLayerId::FSilkS)); // the reference field
        assert!(!f.is_on_layer(&ctx, PcbLayerId::In1Cu));

        assert!(!board.footprint_fix_uuids(fp));
        let nil_field = board[fp].fields[2];
        board.rebind_item_uuid(nil_field.into(), Kiid::nil());
        assert!(board.footprint_fix_uuids(fp));
        assert_ne!(*BoardItem::uuid(&board[nil_field]), Kiid::nil());
        assert_ne!(*BoardItem::uuid(&board[pad]), Kiid::nil());
        board.check_invariants().unwrap();

        let mut f = Footprint::default();
        f.set_dnp(true);
        f.set_excluded_from_bom(true);
        f.set_board_only(true);
        f.set_excluded_from_pos_files(true);
        f.set_excluded_from_sim(true);
        assert_eq!(f.attributes(), 0x40 | 0x08 | 0x10 | 0x04 | 0x80);
        f.set_dnp(false);
        assert!(!f.is_dnp() && f.is_excluded_from_sim());
        f.set_footprint_type(FootprintType::Smd);
        assert_eq!(
            (f.footprint_type(), f.type_name()),
            (FootprintType::Smd, "SMD")
        );
        f.set_footprint_type(FootprintType::Unspecified);
        assert_eq!(f.footprint_type(), FootprintType::Unspecified);
        f.set_is_placed(true);
        f.set_needs_placed(true);
        f.set_is_placed(false);
        assert!(!f.is_placed() && f.needs_placed());
        f.set_stackup_mode(FootprintStackup::CustomLayers);
        f.set_stackup_layers(Lset::from_layers(&[PcbLayerId::FCu]));
        assert_eq!(f.stackup_layers.count(), 1);
        f.set_stackup_mode(FootprintStackup::ExpandInnerLayers);
        assert_eq!(f.stackup_layers.count(), 3);

        assert!(Footprint::is_lib_name_valid("R_0603"));
        assert!(!Footprint::is_lib_name_valid("a/b") && !Footprint::is_lib_name_valid("x:y"));
        assert_eq!(get_ref_des_prefix("U12"), "U");
        assert_eq!(get_ref_des_prefix("R?"), "R");
        assert_eq!(get_ref_des_prefix("12"), "");

        let a = Kiid::parse("aaaaaaaa-0000-4000-8000-000000000001").unwrap();
        let b = Kiid::parse("aaaaaaaa-0000-4000-8000-000000000002").unwrap();
        let c = Kiid::parse("aaaaaaaa-0000-4000-8000-000000000003").unwrap();
        f.path = KiidPath(vec![b, c]);
        assert!(f.is_within_schematic_sheet(&KiidPath(vec![a, b])));
        assert!(f.is_within_schematic_sheet(&KiidPath(vec![a])));
        assert!(!f.is_within_schematic_sheet(&KiidPath(vec![a, c])));
        assert!(!f.is_within_schematic_sheet(&KiidPath(vec![])));
    }

    /// `GetComponentClass` without assignment rules: the static class (or the empty class).
    #[test]
    fn component_class_without_rules() {
        let (board, fp) = board_with_fp();
        let ctx = BoardCtx::bare(&board);
        let class = board[fp].component_class(&ctx).unwrap();
        assert!(class.is_empty());
        assert_eq!(board[fp].component_class_as_string(&ctx), "");
    }
}
