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

//! Port of `pcbnew/pcb_generator.h` and `pcbnew/pcb_generator.cpp` (`PCB_GENERATOR :
//! PCB_GROUP`) and the data part of the generator subclasses in `pcbnew/generators/`
//! (`PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_VIA_STITCH`), `board-model-types.md` section 9.
//!
//! The subclasses keep their typed members only as the property map (`GetProperties()` /
//! `SetProperties()`); `PCB_GENERATOR_POLY::m_outline` is represented by its property entry.
//! The tuning-pattern normalisation between millimetre doubles and IU integers is
//! `PcbGenerator::normalize_properties` (M23). Regeneration is router work (tier 3). Like a
//! group, a generator cascades its mutating virtuals through `Board` (no `BoardItemMut`).
//!
//! The subclass overrides of `GetBoundingBox`, `GetPosition`, `GetLayer`, `HitTest`,
//! `GetItemDescription` and `GetFriendlyName` are reached through the same shims: the inherent
//! methods below dispatch on `generator_type` (`friendly_name` does; the others once M23
//! ports them). `tools/check_item_shims.py` reads the subclass headers
//! (`pcbnew/generators/*.h`) for the shim list.

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::base_units::PCB_IU_PER_MM;
use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, is_copper_layer, to_layer_id};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::cmath::{self, f64_to_i64_x86};
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::{ARC_LOW_DEF, shape_hit_test};
use kicad_geom::math::util::{ki_round_i64, ki_round_to};
use kicad_geom::trigo::deg2rad;
use kicad_geom::{Box2I, Shape, ShapeLineChain, ShapePolySet, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::{ItemRef, OwnedItem};
use crate::pcb_group::PcbGroup;
use crate::traits::{BoardItem, Flashing, Invalidate};

/// `PCB_TUNING_PATTERN::GENERATOR_TYPE` (`generators/pcb_tuning_pattern.cpp`).
pub const GENERATOR_TYPE_TUNING_PATTERN: &str = "tuning_pattern";
/// `PCB_VIA_STACK::GENERATOR_TYPE` (`generators/pcb_via_stack.cpp`).
pub const GENERATOR_TYPE_VIA_STACK: &str = "via_stack";
/// `PCB_VIA_STITCH::GENERATOR_TYPE` (`generators/pcb_via_stitch.cpp`).
pub const GENERATOR_TYPE_VIA_STITCH: &str = "via_stitch";

/// `PCB_GENERATOR : PCB_GROUP` (tuning patterns, via stacks, via stitching).
#[derive(Clone, Debug)]
pub struct PcbGenerator {
    /// The `PCB_GROUP` part; `group.base.layer` is PCB_GENERATOR::m_layer.
    pub group: PcbGroup,
    /// m_generatorType ("tuning_pattern", "via_stack", "via_stitch", ...).
    pub generator_type: String,
    pub origin: Vector2I, // m_origin
    pub is_dirty: bool,   // m_isDirty
    /// What `GetProperties()` returns / `SetProperties()` receives (STRING_ANY_MAP, ordered by
    /// key).
    pub properties: BTreeMap<String, GeneratorProp>,
    /// GetTemplateItems(): inline, `parent == ParentRef::Item(generator)`.
    pub templates: Vec<(String, OwnedItem)>,
}

/// The typed members of a generator subclass ([`PcbGenerator::subclass_data`]).
#[derive(Clone, Debug)]
pub enum GeneratorData {
    /// `PCB_TUNING_PATTERN`.
    TuningPattern(TuningPatternData),
    /// `PCB_VIA_STACK`.
    ViaStack(ViaStackData),
    /// `PCB_VIA_STITCH`.
    ViaStitch(ViaStitchData),
    /// A plain `PCB_GENERATOR`: `m_origin` only.
    Base(Vector2I),
}

impl GeneratorData {
    /// `PCB_GENERATOR::m_origin` of the subclass.
    pub fn origin_mut(&mut self) -> &mut Vector2I {
        match self {
            GeneratorData::TuningPattern(d) => &mut d.origin,
            GeneratorData::ViaStack(d) => &mut d.origin,
            GeneratorData::ViaStitch(d) => &mut d.origin,
            GeneratorData::Base(origin) => origin,
        }
    }
}

/// One value of the generator property map (`STRING_ANY_MAP` entry as the file stores it).
#[derive(Clone, Debug)]
pub enum GeneratorProp {
    /// Written with "{:.10g}".
    Number(f64),
    Bool(bool),
    /// `(key (xy x y))`.
    Vector2I(Vector2I),
    /// `(key (pts ...))`.
    LineChain(ShapeLineChain),
    /// `(key (cells (ij i j) ...))`.
    Cells(Vec<Vector2I>),
    String(String),
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbGenerator {
    /// `PCB_GENERATOR( aParent, aLayer )` (`pcb_generator.cpp`) with `aLayer = F_Cu`, the
    /// default of every registered subclass (`PCB_TUNING_PATTERN( nullptr, F_Cu, .. )`, `new T`
    /// in `GENERATORS_MGR::REGISTER`): empty type, origin (0, 0), not dirty; nil UUID.
    fn default() -> Self {
        PcbGenerator::with_layer(PcbLayerId::FCu)
    }
}

impl PcbGenerator {
    /// `PCB_GENERATOR( aParent, aLayer )` with a nil UUID.
    pub fn with_layer(layer: PcbLayerId) -> Self {
        PcbGenerator {
            group: PcbGroup::with_layer(layer),
            generator_type: String::new(),
            origin: Vector2I::default(),
            is_dirty: false,
            properties: BTreeMap::new(),
            templates: Vec::new(),
        }
    }

    /// `new PCB_GENERATOR( aParent, aLayer )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator, layer: PcbLayerId) -> Self {
        let mut generator = PcbGenerator::with_layer(layer);
        generator.group.base.uuid = Kiid::new(kiid_generator);
        generator
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbGenerator {
    fn invalidate_caches(&mut self) {
        Invalidate::invalidate_caches(&mut self.group);
    }
}

impl BoardItem for PcbGenerator {
    fn base(&self) -> &BoardItemBase {
        &self.group.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.group.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbGeneratorT
    }
    fn class_name(&self) -> &'static str {
        "PCB_GENERATOR"
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbGenerator::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbGenerator::layer_set(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbGenerator::is_on_layer(self, ctx, layer)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbGenerator::is_on_copper_layer(self, ctx)
    }
    fn is_layer_agnostic(&self) -> bool {
        PcbGenerator::is_layer_agnostic(self)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbGenerator::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbGenerator::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbGenerator::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbGenerator::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbGenerator::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        PcbGenerator::run_on_children(self, ctx, mode, f)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbGenerator::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbGenerator::friendly_name(self, ctx)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbGenerator {
    /// `GetGeneratorType()` (the subclasses return their `GENERATOR_TYPE`, which is what
    /// `m_generatorType` holds).
    pub fn generator_type(&self) -> &str {
        &self.generator_type
    }

    /// `IsDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// `MarkDirty()`.
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    /// `ClearDirty()`.
    pub fn clear_dirty(&mut self) {
        self.is_dirty = false;
    }

    /// `GetTemplateItems()`.
    pub fn template_items(&self) -> &[(String, OwnedItem)] {
        &self.templates
    }

    /// The subclass `SetProperties( GetProperties() )` round trip of
    /// `generators/pcb_tuning_pattern.cpp` (and the via stack / stitch counterparts): converts
    /// the parsed property values between millimetre doubles and IU integers exactly as the C++
    /// typed members do, so that a re-save prints what C++ prints (`board-model-types.md` 9).
    ///
    /// `SetProperties` also writes the members that are not in the map: `m_origin` (every
    /// subclass) and, for a via stack with a `start_layer`, the generator layer
    /// (`SetStartLayer` calls `SetLayer`). Call it where the C++ loader calls
    /// `gen->SetProperties()`, i.e. after the layer from the file has been set.
    pub fn normalize_properties(&mut self) {
        let map = PropertyMap(&self.properties);

        let normalized = match self.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => {
                let data = TuningPatternData::from_properties(&map);
                self.origin = data.origin;
                data.to_properties()
            }
            GENERATOR_TYPE_VIA_STACK => {
                let mut layer = self.group.base.layer;
                let data = ViaStackData::from_properties(&map, &mut layer);
                self.group.base.layer = layer;
                self.origin = data.origin;
                data.to_properties()
            }
            GENERATOR_TYPE_VIA_STITCH => {
                let data = ViaStitchData::from_properties(&map);
                self.origin = data.origin;
                data.to_properties()
            }
            _ => {
                // PCB_GENERATOR::SetProperties / GetProperties
                let mut origin = Vector2I::default();
                map.get_to_vector(KEY_ORIGIN, &mut origin);
                self.origin = origin;
                let mut props = PropertySink::default();
                props.set(KEY_ORIGIN, GeneratorProp::Vector2I(self.origin));
                props.0
            }
        };

        self.properties = normalized;
    }

    /// `GENERATORS_MGR::Instance().CreateFromType( aType )`: a default-constructed registered
    /// subclass (`PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_VIA_STITCH`: type, display name
    /// `m_name`, layer `F_Cu`) with a nil UUID, `None` for an unknown type (the C++ loader then
    /// throws "Cannot create generated object of type '%s'"). The property map is empty; the
    /// constructor values of the typed members are what `normalize_properties` falls back to.
    pub fn create_from_type(generator_type: &str) -> Option<PcbGenerator> {
        let name = match generator_type {
            GENERATOR_TYPE_TUNING_PATTERN => "Tuning Pattern",
            GENERATOR_TYPE_VIA_STACK => "Microvia Stack",
            GENERATOR_TYPE_VIA_STITCH => "Via Stitching",
            _ => return None,
        };

        let mut generator = PcbGenerator::with_layer(PcbLayerId::FCu);
        generator.generator_type = generator_type.to_string();
        generator.group.name = name.to_string();

        Some(generator)
    }

    /// `LayerFollowsMembers()`: true, `PCB_VIA_STACK` false (its layer is the start layer). The
    /// loader sets the layer of a generator for which this is true from its first member track.
    pub fn layer_follows_members(&self) -> bool {
        self.generator_type != GENERATOR_TYPE_VIA_STACK
    }

    /// The typed members of a `PCB_TUNING_PATTERN` as `SetProperties` reads them from the
    /// property map; `None` for another generator type.
    pub fn tuning_pattern_data(&self) -> Option<TuningPatternData> {
        (self.generator_type == GENERATOR_TYPE_TUNING_PATTERN)
            .then(|| TuningPatternData::from_properties(&PropertyMap(&self.properties)))
    }

    /// The typed members of a `PCB_VIA_STACK` (see `tuning_pattern_data`).
    pub fn via_stack_data(&self) -> Option<ViaStackData> {
        let mut layer = self.group.base.layer;
        (self.generator_type == GENERATOR_TYPE_VIA_STACK)
            .then(|| ViaStackData::from_properties(&PropertyMap(&self.properties), &mut layer))
    }

    /// The typed members of a `PCB_VIA_STITCH` (see `tuning_pattern_data`).
    pub fn via_stitch_data(&self) -> Option<ViaStitchData> {
        (self.generator_type == GENERATOR_TYPE_VIA_STITCH)
            .then(|| ViaStitchData::from_properties(&PropertyMap(&self.properties)))
    }

    /// The typed members of the subclass (package FIX-EDIT-1): what the mutating virtuals of
    /// `pcbnew/generators/*.h` change. Write them back with [`PcbGenerator::set_subclass_data`].
    pub fn subclass_data(&self) -> GeneratorData {
        if let Some(data) = self.tuning_pattern_data() {
            GeneratorData::TuningPattern(data)
        } else if let Some(data) = self.via_stack_data() {
            GeneratorData::ViaStack(data)
        } else if let Some(data) = self.via_stitch_data() {
            GeneratorData::ViaStitch(data)
        } else {
            GeneratorData::Base(self.origin)
        }
    }

    /// Stores the typed members: `m_origin` and the property map the C++ `GetProperties()`
    /// returns for them (what the writer prints).
    pub fn set_subclass_data(&mut self, data: &GeneratorData) {
        match data {
            GeneratorData::TuningPattern(d) => {
                self.origin = d.origin;
                self.properties = d.to_properties();
            }
            GeneratorData::ViaStack(d) => {
                self.origin = d.origin;
                self.properties = d.to_properties();
            }
            GeneratorData::ViaStitch(d) => {
                self.origin = d.origin;
                self.properties = d.to_properties();
            }
            GeneratorData::Base(origin) => {
                // PCB_GENERATOR::GetProperties
                self.origin = *origin;
                let mut props = PropertySink::default();
                props.set(KEY_ORIGIN, GeneratorProp::Vector2I(self.origin));
                self.properties = props.0;
            }
        }
    }

    /// `PCB_GENERATOR::GetLayer()`: `m_layer`; `PCB_TUNING_PATTERN` answers with the layer of
    /// its first member track.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        if self.generator_type == GENERATOR_TYPE_TUNING_PATTERN {
            for item in self.group.board_items(ctx) {
                // dynamic_cast<PCB_TRACK*>: segments, arcs and vias
                if item.as_track().is_some() {
                    return item.layer();
                }
            }
        }

        self.group.base.layer
    }

    /// `PCB_GENERATOR::GetLayerSet()`: `PCB_GROUP::GetLayerSet() | LSET( { GetLayer() } )`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        let mut set = self.group.layer_set(ctx);
        set.set(self.layer(ctx));
        set
    }

    /// `PCB_GROUP::IsOnLayer()` (inherited).
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        self.group.is_on_layer(ctx, layer)
    }

    /// `PCB_GROUP::IsOnCopperLayer()` (inherited): false.
    pub fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        self.group.is_on_copper_layer(ctx)
    }

    /// `PCB_GROUP::IsLayerAgnostic()` (inherited): true.
    pub fn is_layer_agnostic(&self) -> bool {
        self.group.is_layer_agnostic()
    }

    /// `PCB_GENERATOR::GetPosition()`: `m_origin`; `PCB_TUNING_PATTERN` and `PCB_VIA_STITCH`
    /// override it.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        if self.generator_type == GENERATOR_TYPE_VIA_STITCH {
            // PCB_VIA_STITCH::GetPosition: the first outline vertex. An empty outline has no
            // vertex 0 (e.g. a freshly constructed generator before the outline is set); fall
            // back to the anchor origin rather than throwing.
            if let Some(GeneratorProp::LineChain(outline)) = self.properties.get(KEY_OUTLINE) {
                if outline.point_count() > 0 {
                    return outline.c_point(0);
                }
            }
        }

        self.origin
    }

    /// `PCB_GENERATOR::GetBoundingBox()`: an empty box; every subclass overrides it
    /// (`PCB_TUNING_PATTERN`: the outline merged with the members' box, `PCB_VIA_STACK`: the
    /// members' box, `PCB_VIA_STITCH`: the outline box).
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        match self.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => {
                let data = TuningPatternData::from_properties(&PropertyMap(&self.properties));
                let mut bbox = data.outline().bbox(0);

                // The outline is synthesized from the baseline and the meander settings, so it
                // can both be empty and under-cover the copper.  PCB_GROUP inflates
                // unconditionally, so skip it when there are no members rather than absorb a
                // half-millimetre box at the origin.
                if !self.group.members.is_empty() {
                    bbox.merge(&self.group.bounding_box(ctx));
                }

                bbox
            }
            // The PCB_GENERATOR base returns an empty box, which breaks click selection and
            // the view index. Use the member union like a plain group.
            GENERATOR_TYPE_VIA_STACK => self.group.bounding_box(ctx),
            GENERATOR_TYPE_VIA_STITCH => {
                let data = ViaStitchData::from_properties(&PropertyMap(&self.properties));
                data.outline_set().bbox(0)
            }
            _ => Box2I::default(),
        }
    }

    /// `PCB_GROUP::GetEffectiveShape()` (inherited): a compound of the members' shapes.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        self.group.effective_shape(ctx, layer, flash, usage)
    }

    /// `PCB_GROUP::HitTest( aPosition, aAccuracy )` (inherited: false); every subclass
    /// overrides it.
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        match self.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => {
                let data = TuningPatternData::from_properties(&PropertyMap(&self.properties));

                if data.outline().collide_point(pos, accuracy, None, None) {
                    return true;
                }

                // Members of a generator are never selectable on their own, so the pattern
                // has to answer for any copper the outline misses
                self.group
                    .board_items(ctx)
                    .iter()
                    .any(|item| item.as_dyn().hit_test(ctx, pos, accuracy))
            }
            // Generator members are not selectable on their own, the selection tool expects
            // the generator itself to hit test. Delegate to the members.
            GENERATOR_TYPE_VIA_STACK => self
                .group
                .board_items(ctx)
                .iter()
                .any(|item| item.as_dyn().hit_test(ctx, pos, accuracy)),
            GENERATOR_TYPE_VIA_STITCH => {
                let data = ViaStitchData::from_properties(&PropertyMap(&self.properties));
                data.outline_set().collide_point(pos, accuracy, None, None)
            }
            _ => self.group.hit_test(ctx, pos, accuracy),
        }
    }

    /// `PCB_GROUP::HitTest( aRect, aContained, aAccuracy )` (inherited: false); every
    /// subclass overrides it.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let tuning = self.generator_type == GENERATOR_TYPE_TUNING_PATTERN;

        if !tuning && self.generator_type != GENERATOR_TYPE_VIA_STACK {
            // PCB_VIA_STITCH and the base inherit PCB_GROUP::HitTest: false
            return self.group.hit_test_rect(ctx, rect, contained, accuracy);
        }

        let mut any_items = false;

        for item in self.group.board_items(ctx) {
            any_items = true;

            if contained {
                if !item.as_dyn().hit_test_rect(ctx, rect, true, accuracy) {
                    return false;
                }
            } else if item.as_dyn().hit_test_rect(ctx, rect, false, accuracy) {
                return true;
            }
        }

        if !tuning {
            // PCB_VIA_STACK
            return any_items && contained;
        }

        if any_items {
            return contained;
        }

        let mut sel = *rect;

        if accuracy != 0 {
            sel.inflate(accuracy);
        }

        // Convert selection box to polygon for accurate hit testing against the actual outline
        let sel_poly = ShapeLineChain::from_points(
            &[
                sel.origin(),
                Vector2I::new(sel.right(), sel.top()),
                sel.end(),
                Vector2I::new(sel.left(), sel.bottom()),
            ],
            true,
        );

        let data = TuningPatternData::from_properties(&PropertyMap(&self.properties));
        shape_hit_test(&sel_poly, &data.outline(), contained)
    }

    /// `PCB_GROUP::RunOnChildren()` (inherited).
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        self.group.run_on_children(ctx, mode, f)
    }

    /// `PCB_GENERATOR::GetItemDescription()`: "Generator"; the subclasses return "Tuning
    /// Pattern", ... .
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = (ctx, units);

        match self.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => "Tuning Pattern",
            GENERATOR_TYPE_VIA_STACK => "Microvia Stack",
            GENERATOR_TYPE_VIA_STITCH => "Via Stitching",
            _ => "Generator",
        }
        .to_string()
    }

    /// `GetFriendlyName()`: overridden by every registered subclass
    /// (`PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_VIA_STITCH`; the untranslated strings);
    /// `PCB_GENERATOR` itself inherits `EDA_ITEM::GetFriendlyName`.
    pub fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        match self.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => "Tuning Pattern".to_string(),
            GENERATOR_TYPE_VIA_STACK => "Microvia Stack".to_string(),
            GENERATOR_TYPE_VIA_STITCH => "Via Stitching".to_string(),
            _ => crate::board_item::friendly_name(self, ctx),
        }
    }
}

// ---- STRING_ANY_MAP semantics ------------------------------------------------------------

/// `pcbIUScale.IU_PER_MM`: the scale of the generator property maps (`STRING_ANY_MAP(
/// pcbIUScale.IU_PER_MM )` in the loader and in `PCB_GENERATOR::GetProperties`).
const PROPS_IU_SCALE: f64 = PCB_IU_PER_MM;

const KEY_ORIGIN: &str = "origin";
const KEY_OUTLINE: &str = "outline";

/// Read side of `STRING_ANY_MAP` (`include/string_any_map.h`) with the `wxAny::GetAs`
/// conversions of the reference build (wx 3.2, measured with a driver against
/// `libwx_baseu-3.2`): a number converts to an integer by truncation after a range check
/// against `long long` (80.5 -> 80, -2.5 -> -2, 1e19 fails) and then `static_cast` to the
/// target width (3e9 -> -1294967296 for an `int`); a number never converts to `bool`; a bool
/// converts to an integer (0 / 1) and to the strings "true" / "false"; a string converts to an
/// integer or a number when it parses completely, to `bool` for "yes" / "true" / "1" and
/// "no" / "false" / "0"; a number converts to a string with `wxString::FromCDouble( v, 14 )`.
struct PropertyMap<'a>(&'a BTreeMap<String, GeneratorProp>);

impl PropertyMap<'_> {
    /// `wxAny::GetAs( long long* )`.
    fn any_as_i64(value: &GeneratorProp) -> Option<i64> {
        match value {
            GeneratorProp::Number(v) => {
                let v = *v;

                if v < i64::MIN as f64 || v > i64::MAX as f64 {
                    return None;
                }

                // (wxAnyBaseIntType) value; 2^63 and NaN pass the range check and give the
                // x86 result of the out-of-range conversion.
                Some(f64_to_i64_x86(v))
            }
            GeneratorProp::Bool(b) => Some(i64::from(*b)),
            GeneratorProp::String(text) => text.parse::<i64>().ok(),
            _ => None,
        }
    }

    /// `wxAny::GetAs( double* )`.
    fn any_as_f64(value: &GeneratorProp) -> Option<f64> {
        match value {
            GeneratorProp::Number(v) => Some(*v),
            GeneratorProp::String(text) => text.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// `wxAny::GetAs( bool* )`.
    fn any_as_bool(value: &GeneratorProp) -> Option<bool> {
        match value {
            GeneratorProp::Bool(b) => Some(*b),
            GeneratorProp::String(text) => match text.to_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// `wxAny::GetAs( wxString* )`.
    fn any_as_string(value: &GeneratorProp) -> Option<String> {
        match value {
            GeneratorProp::String(text) => Some(text.clone()),
            GeneratorProp::Number(v) => Some(format!("{v:.14}")),
            GeneratorProp::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
            _ => None,
        }
    }

    /// `get_to( aKey, int& )`.
    fn get_to_i32(&self, key: &str, var: &mut i32) -> bool {
        match self.0.get(key).and_then(Self::any_as_i64) {
            Some(v) => {
                *var = v as i32; // static_cast<int>
                true
            }
            None => false,
        }
    }

    /// `get_to( aKey, bool& )`.
    fn get_to_bool(&self, key: &str, var: &mut bool) -> bool {
        match self.0.get(key).and_then(Self::any_as_bool) {
            Some(v) => {
                *var = v;
                true
            }
            None => false,
        }
    }

    /// `get_to( aKey, wxString& )`.
    fn get_to_string(&self, key: &str, var: &mut String) -> bool {
        match self.0.get(key).and_then(Self::any_as_string) {
            Some(v) => {
                *var = v;
                true
            }
            None => false,
        }
    }

    /// `get_to( aKey, VECTOR2I& )`.
    fn get_to_vector(&self, key: &str, var: &mut Vector2I) -> bool {
        match self.0.get(key) {
            Some(GeneratorProp::Vector2I(v)) => {
                *var = *v;
                true
            }
            _ => false,
        }
    }

    /// `get_opt<int>( aKey )`.
    fn get_opt_i32(&self, key: &str) -> Option<i32> {
        let mut v = 0;
        self.get_to_i32(key, &mut v).then_some(v)
    }

    /// `get_opt<SHAPE_LINE_CHAIN>( aKey )`.
    fn get_opt_chain(&self, key: &str) -> Option<ShapeLineChain> {
        match self.0.get(key) {
            Some(GeneratorProp::LineChain(chain)) => Some(chain.clone()),
            _ => None,
        }
    }

    /// `get_opt<std::vector<VECTOR2I>>( aKey )`.
    fn get_opt_cells(&self, key: &str) -> Option<Vec<Vector2I>> {
        match self.0.get(key) {
            Some(GeneratorProp::Cells(cells)) => Some(cells.clone()),
            _ => None,
        }
    }

    /// `get_to_iu( aKey, T& )` for an integral `T`: a number is scaled to IU and rounded with
    /// `KiROUND<double, T>` ("most millimetre values are not exact doubles, so truncating would
    /// load them 1 IU short"); anything else goes through `GetAs`.
    fn get_to_iu_i64(&self, key: &str, var: &mut i64) -> bool {
        let Some(value) = self.0.get(key) else {
            return false;
        };

        if let GeneratorProp::Number(_) = value {
            let Some(number) = Self::any_as_f64(value) else {
                return false;
            };

            *var = ki_round_i64(number * PROPS_IU_SCALE);
            return true;
        }

        match Self::any_as_i64(value) {
            Some(v) => {
                *var = v;
                true
            }
            None => false,
        }
    }

    /// `get_to_iu( aKey, int& )`.
    fn get_to_iu_i32(&self, key: &str, var: &mut i32) -> bool {
        let Some(value) = self.0.get(key) else {
            return false;
        };

        if let GeneratorProp::Number(number) = value {
            *var = ki_round_to::<i32, f64>(*number * PROPS_IU_SCALE);
            return true;
        }

        match Self::any_as_i64(value) {
            Some(v) => {
                *var = v as i32;
                true
            }
            None => false,
        }
    }
}

/// Write side of `STRING_ANY_MAP`: `set` is `emplace` (an existing key is kept), `set_iu`
/// stores `aVar / m_iuScale` as a double.
#[derive(Default)]
struct PropertySink(BTreeMap<String, GeneratorProp>);

impl PropertySink {
    /// `set( aKey, aVar )`.
    fn set(&mut self, key: &str, value: GeneratorProp) {
        self.0.entry(key.to_string()).or_insert(value);
    }

    /// `set( aKey, int / bool / wxString )` shorthands.
    fn set_number(&mut self, key: &str, value: f64) {
        self.set(key, GeneratorProp::Number(value));
    }

    fn set_bool(&mut self, key: &str, value: bool) {
        self.set(key, GeneratorProp::Bool(value));
    }

    fn set_string(&mut self, key: &str, value: &str) {
        self.set(key, GeneratorProp::String(value.to_string()));
    }

    /// `set_iu( aKey, aVar )` for an `int` or `long long` value.
    fn set_iu(&mut self, key: &str, value: i64) {
        self.set(key, GeneratorProp::Number(value as f64 / PROPS_IU_SCALE));
    }
}

// ---- PCB_TUNING_PATTERN ------------------------------------------------------------------

/// `LENGTH_TUNING_MODE` (`pcbnew/generators/pcb_tuning_pattern.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthTuningMode {
    Single,
    DiffPair,
    DiffPairSkew,
}

/// `PNS::MEANDER_PLACER_BASE::TUNING_STATUS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuningStatus {
    TooShort,
    TooLong,
    Tuned,
}

/// `PNS::MEANDER_SIDE_LEFT` / `DEFAULT` / `RIGHT` (-1 / 0 / 1).
pub const MEANDER_SIDE_LEFT: i32 = -1;
pub const MEANDER_SIDE_DEFAULT: i32 = 0;
pub const MEANDER_SIDE_RIGHT: i32 = 1;

/// `PNS::MEANDER_SETTINGS::DEFAULT_LENGTH_TOLERANCE` (`router/pns_meander.cpp`): 0.1 mm.
pub const DEFAULT_LENGTH_TOLERANCE: i64 = 100_000;
/// `MEANDER_SETTINGS::LENGTH_UNCONSTRAINED`: `1000000 * pcbIUScale.IU_PER_MM`.
pub const LENGTH_UNCONSTRAINED: i64 = 1_000_000_000_000;
/// `MEANDER_SETTINGS::DEFAULT_DELAY_TOLERANCE`: `0.1 * pcbIUScale.IU_PER_PS`.
pub const DEFAULT_DELAY_TOLERANCE: i64 = 100_000;
/// `MEANDER_SETTINGS::DELAY_UNCONSTRAINED`: `1000000 * pcbIUScale.IU_PER_PS`.
pub const DELAY_UNCONSTRAINED: i64 = 1_000_000_000_000;
/// `MEANDER_SETTINGS::SKEW_UNCONSTRAINED`: `std::numeric_limits<int>::max()`.
pub const SKEW_UNCONSTRAINED: i32 = i32::MAX;

/// The members of `PNS::MEANDER_SETTINGS` that a tuning pattern stores in its properties.
/// The `MINOPTMAX` values always have all three parts set (the setters set them), so they are
/// `(min, opt, max)` triples.
#[derive(Clone, Debug, PartialEq)]
pub struct MeanderSettingsData {
    pub min_amplitude: i32,                   // m_minAmplitude
    pub max_amplitude: i32,                   // m_maxAmplitude
    pub spacing: i32,                         // m_spacing
    pub target_length: (i64, i64, i64),       // m_targetLength
    pub target_length_delay: (i64, i64, i64), // m_targetLengthDelay
    pub target_skew: (i32, i32, i32),         // m_targetSkew
    pub override_custom_rules: bool,          // m_overrideCustomRules
    /// m_cornerStyle == MEANDER_STYLE_ROUND (else MEANDER_STYLE_CHAMFER).
    pub corner_style_round: bool,
    pub corner_radius_percentage: i32, // m_cornerRadiusPercentage
    pub single_sided: bool,            // m_singleSided
    pub initial_side: i32,             // m_initialSide (MEANDER_SIDE_*)
    pub is_time_domain: bool,          // m_isTimeDomain
}

impl Default for MeanderSettingsData {
    /// `MEANDER_SETTINGS::MEANDER_SETTINGS()` (`router/pns_meander.cpp`).
    fn default() -> Self {
        let mut settings = MeanderSettingsData {
            min_amplitude: 200_000,
            max_amplitude: 1_000_000,
            spacing: 600_000,
            target_length: (0, 0, 0),
            target_length_delay: (0, 0, 0),
            target_skew: (0, 0, 0),
            override_custom_rules: false,
            corner_style_round: true,
            corner_radius_percentage: 80,
            single_sided: false,
            initial_side: MEANDER_SIDE_LEFT,
            is_time_domain: false,
        };
        settings.set_target_length(LENGTH_UNCONSTRAINED);
        settings.set_target_length_delay(DELAY_UNCONSTRAINED);
        settings.set_target_skew(0);
        settings
    }
}

impl MeanderSettingsData {
    /// `MEANDER_SETTINGS::SetTargetLength( long long aOpt )`.
    pub fn set_target_length(&mut self, opt: i64) {
        self.target_length.1 = opt;

        if opt == LENGTH_UNCONSTRAINED {
            self.target_length.0 = 0;
            self.target_length.2 = opt;
        } else {
            self.target_length.0 = opt.wrapping_sub(DEFAULT_LENGTH_TOLERANCE);
            self.target_length.2 = opt.wrapping_add(DEFAULT_LENGTH_TOLERANCE);
        }
    }

    /// `MEANDER_SETTINGS::SetTargetLengthDelay( long long aOpt )`.
    pub fn set_target_length_delay(&mut self, opt: i64) {
        self.target_length_delay.1 = opt;

        if opt == DELAY_UNCONSTRAINED {
            self.target_length_delay.0 = 0;
            self.target_length_delay.2 = opt;
        } else {
            self.target_length_delay.0 = opt.wrapping_sub(DEFAULT_DELAY_TOLERANCE);
            self.target_length_delay.2 = opt.wrapping_add(DEFAULT_DELAY_TOLERANCE);
        }
    }

    /// `MEANDER_SETTINGS::SetTargetSkew( int aOpt )`: the tolerance is a `long long`, the
    /// result is converted back to `int`.
    pub fn set_target_skew(&mut self, opt: i32) {
        self.target_skew.1 = opt;

        if opt == SKEW_UNCONSTRAINED {
            self.target_skew.0 = 0;
            self.target_skew.2 = opt;
        } else {
            self.target_skew.0 = (i64::from(opt) - DEFAULT_LENGTH_TOLERANCE) as i32;
            self.target_skew.2 = (i64::from(opt) + DEFAULT_LENGTH_TOLERANCE) as i32;
        }
    }
}

/// The typed members of `PCB_TUNING_PATTERN` that its properties carry (plus `m_origin`).
#[derive(Clone, Debug)]
pub struct TuningPatternData {
    pub origin: Vector2I,                          // PCB_GENERATOR::m_origin
    pub end: Vector2I,                             // m_end
    pub settings: MeanderSettingsData,             // m_settings
    pub base_line: Option<ShapeLineChain>,         // m_baseLine
    pub base_line_coupled: Option<ShapeLineChain>, // m_baseLineCoupled
    pub track_width: i32,                          // m_trackWidth
    pub diff_pair_gap: i32,                        // m_diffPairGap
    pub tuning_mode: LengthTuningMode,             // m_tuningMode
    pub last_net_name: String,                     // m_lastNetName
    pub tuning_length: i64,                        // m_tuningLength
    pub tuning_status: TuningStatus,               // m_tuningStatus
}

impl Default for TuningPatternData {
    /// `PCB_TUNING_PATTERN( nullptr, F_Cu, SINGLE )` (what `GENERATORS_MGR` creates):
    /// `m_end = ( 10 mm, 0 )`, `m_initialSide = MEANDER_SIDE_LEFT`, status `TUNED`.
    fn default() -> Self {
        TuningPatternData {
            origin: Vector2I::default(),
            end: Vector2I::new(PCB_IU_SCALE.mm_to_iu(10.0), 0),
            settings: MeanderSettingsData::default(),
            base_line: None,
            base_line_coupled: None,
            track_width: 0,
            diff_pair_gap: 0,
            tuning_mode: LengthTuningMode::Single,
            last_net_name: String::new(),
            tuning_length: 0,
            tuning_status: TuningStatus::Tuned,
        }
    }
}

/// `tuningFromString()` (`wxFAIL_MSG` and `SINGLE` for an unknown token).
fn tuning_from_string(text: &str) -> LengthTuningMode {
    match text {
        "single" => LengthTuningMode::Single,
        "diff_pair" => LengthTuningMode::DiffPair,
        "diff_pair_skew" => LengthTuningMode::DiffPairSkew,
        _ => LengthTuningMode::Single,
    }
}

/// `tuningToString()`.
fn tuning_to_string(mode: LengthTuningMode) -> &'static str {
    match mode {
        LengthTuningMode::Single => "single",
        LengthTuningMode::DiffPair => "diff_pair",
        LengthTuningMode::DiffPairSkew => "diff_pair_skew",
    }
}

/// `sideFromString()` (`MEANDER_SIDE_DEFAULT` for an unknown token, also for "").
fn side_from_string(text: &str) -> i32 {
    match text {
        "default" => MEANDER_SIDE_DEFAULT,
        "left" => MEANDER_SIDE_LEFT,
        "right" => MEANDER_SIDE_RIGHT,
        _ => MEANDER_SIDE_DEFAULT,
    }
}

/// `sideToString()` (`wxFAIL` and "" outside the enum).
fn side_to_string(side: i32) -> &'static str {
    match side {
        MEANDER_SIDE_DEFAULT => "default",
        MEANDER_SIDE_LEFT => "left",
        MEANDER_SIDE_RIGHT => "right",
        _ => "",
    }
}

/// `statusFromString()` (`TUNED` for an unknown token).
fn status_from_string(text: &str) -> TuningStatus {
    match text {
        "too_long" => TuningStatus::TooLong,
        "too_short" => TuningStatus::TooShort,
        _ => TuningStatus::Tuned,
    }
}

/// `statusToString()`.
fn status_to_string(status: TuningStatus) -> &'static str {
    match status {
        TuningStatus::TooLong => "too_long",
        TuningStatus::TooShort => "too_short",
        TuningStatus::Tuned => "tuned",
    }
}

impl TuningPatternData {
    /// `PCB_TUNING_PATTERN::SetProperties( aProps )` on a default-constructed pattern.
    ///
    /// The C++ reads the length / delay / skew targets through the uninitialised locals
    /// `long long val` and `int int_val`: a key that is missing leaves the value of the last
    /// key read, which is what the reference build then stores (a file without
    /// `target_delay` gets the delay target from `target_length_max`; checked against
    /// `kicad-cli pcb upgrade` on `tuning_generators_load_save.kicad_pcb`). Here both locals
    /// start at 0, which only differs where the C++ reads an indeterminate value.
    fn from_properties(props: &PropertyMap<'_>) -> TuningPatternData {
        let mut data = TuningPatternData::default();

        // PCB_GENERATOR::SetProperties
        props.get_to_vector(KEY_ORIGIN, &mut data.origin);

        let mut tuning_mode = String::new();
        props.get_to_string("tuning_mode", &mut tuning_mode);
        data.tuning_mode = tuning_from_string(&tuning_mode);

        let mut side = String::new();
        props.get_to_string("initial_side", &mut side);
        data.settings.initial_side = side_from_string(&side);

        let mut status = String::new();
        props.get_to_string("last_status", &mut status);
        data.tuning_status = status_from_string(&status);

        props.get_to_bool("is_time_domain", &mut data.settings.is_time_domain);

        props.get_to_vector("end", &mut data.end);
        props.get_to_i32(
            "corner_radius_percent",
            &mut data.settings.corner_radius_percentage,
        );
        props.get_to_bool("single_sided", &mut data.settings.single_sided);
        // aProps.get_to( "side", m_settings.m_initialSide ): wxAny cannot convert any stored
        // value to the MEANDER_SIDE enum, so this never changes anything.

        let mut rounded = false;
        props.get_to_bool("rounded", &mut rounded);
        data.settings.corner_style_round = rounded;

        let mut val: i64 = 0;

        props.get_to_iu_i64("target_length", &mut val);
        data.settings.set_target_length(val);

        if props.get_to_iu_i64("target_length_min", &mut val) {
            data.settings.target_length.0 = val;
        }

        if props.get_to_iu_i64("target_length_max", &mut val) {
            data.settings.target_length.2 = val;
        }

        props.get_to_iu_i64("target_delay", &mut val);
        data.settings.set_target_length_delay(val);

        if props.get_to_iu_i64("target_delay_min", &mut val) {
            data.settings.target_length_delay.0 = val;
        }

        if props.get_to_iu_i64("target_delay_max", &mut val) {
            data.settings.target_length_delay.2 = val;
        }

        let mut int_val: i32 = 0;

        props.get_to_iu_i32("target_skew", &mut int_val);
        data.settings.set_target_skew(int_val);

        if props.get_to_iu_i32("target_skew_min", &mut int_val) {
            data.settings.target_skew.0 = int_val;
        }

        if props.get_to_iu_i32("target_skew_max", &mut int_val) {
            data.settings.target_skew.2 = int_val;
        }

        props.get_to_iu_i32("max_amplitude", &mut data.settings.max_amplitude);
        props.get_to_iu_i32("min_amplitude", &mut data.settings.min_amplitude);
        props.get_to_iu_i32("min_spacing", &mut data.settings.spacing);
        props.get_to_iu_i32("last_track_width", &mut data.track_width);
        props.get_to_iu_i32("last_diff_pair_gap", &mut data.diff_pair_gap);
        props.get_to_iu_i64("last_tuning_length", &mut data.tuning_length);
        props.get_to_bool(
            "override_custom_rules",
            &mut data.settings.override_custom_rules,
        );

        props.get_to_string("last_netname", &mut data.last_net_name);

        if let Some(base_line) = props.get_opt_chain("base_line") {
            data.base_line = Some(base_line);
        }

        if let Some(base_line_coupled) = props.get_opt_chain("base_line_coupled") {
            data.base_line_coupled = Some(base_line_coupled);
        }

        // rebuildTuningInfo(): m_tuningInfo is not a property.
        data
    }

    /// `PCB_TUNING_PATTERN::GetProperties()`.
    fn to_properties(&self) -> BTreeMap<String, GeneratorProp> {
        let mut props = PropertySink::default();

        // PCB_GENERATOR::GetProperties
        props.set(KEY_ORIGIN, GeneratorProp::Vector2I(self.origin));

        let settings = &self.settings;
        props.set_string("tuning_mode", tuning_to_string(self.tuning_mode));
        props.set_string("initial_side", side_to_string(settings.initial_side));
        props.set_string("last_status", status_to_string(self.tuning_status));
        props.set_bool("is_time_domain", settings.is_time_domain);

        props.set("end", GeneratorProp::Vector2I(self.end));
        props.set_number(
            "corner_radius_percent",
            f64::from(settings.corner_radius_percentage),
        );
        props.set_bool("single_sided", settings.single_sided);
        props.set_bool("rounded", settings.corner_style_round);

        props.set_iu("max_amplitude", i64::from(settings.max_amplitude));
        props.set_iu("min_amplitude", i64::from(settings.min_amplitude));
        props.set_iu("min_spacing", i64::from(settings.spacing));
        props.set_iu("target_length_min", settings.target_length.0);
        props.set_iu("target_length", settings.target_length.1);
        props.set_iu("target_length_max", settings.target_length.2);
        props.set_iu("target_delay_min", settings.target_length_delay.0);
        props.set_iu("target_delay", settings.target_length_delay.1);
        props.set_iu("target_delay_max", settings.target_length_delay.2);
        props.set_iu("target_skew_min", i64::from(settings.target_skew.0));
        props.set_iu("target_skew", i64::from(settings.target_skew.1));
        props.set_iu("target_skew_max", i64::from(settings.target_skew.2));
        props.set_iu("last_track_width", i64::from(self.track_width));
        props.set_iu("last_diff_pair_gap", i64::from(self.diff_pair_gap));
        props.set_iu("last_tuning_length", self.tuning_length);

        props.set_string("last_netname", &self.last_net_name);
        props.set_bool("override_custom_rules", settings.override_custom_rules);

        if let Some(base_line) = &self.base_line {
            props.set("base_line", GeneratorProp::LineChain(base_line.clone()));
        }

        if let Some(base_line_coupled) = &self.base_line_coupled {
            props.set(
                "base_line_coupled",
                GeneratorProp::LineChain(base_line_coupled.clone()),
            );
        }

        props.0
    }

    /// `PCB_TUNING_PATTERN::getOutline()`: the area the meanders may occupy, synthesised from
    /// the baseline(s) and the meander settings; an empty chain without a baseline.
    pub fn outline(&self) -> ShapeLineChain {
        let Some(base_line) = &self.base_line else {
            return ShapeLineChain::new();
        };

        let settings = &self.settings;
        let mut clamped_max_amplitude = settings.max_amplitude;
        let baseline_offset = if self.tuning_mode == LengthTuningMode::DiffPair {
            (self.diff_pair_gap + self.track_width) / 2
        } else {
            0
        };

        let min_allowed_amplitude = if settings.corner_style_round {
            baseline_offset + self.track_width
        } else {
            let correction =
                (f64::from(self.track_width) * cmath::tan(1.0 - cmath::tan(deg2rad(22.5)))) as i32;
            baseline_offset + correction
        };

        clamped_max_amplitude = clamped_max_amplitude.max(min_allowed_amplitude);

        if settings.single_sided {
            let cl_base = base_line.clone();

            if self.tuning_mode != LengthTuningMode::DiffPair {
                let amplitude = clamped_max_amplitude
                    + ki_round_to::<i32, f64>(f64::from(self.track_width) / 2.0);

                let mut chain = ShapeLineChain::new();

                if let Some((left, right)) = cl_base.offset_line(
                    amplitude,
                    CornerStrategy::RoundAllCorners,
                    ARC_LOW_DEF,
                    true,
                ) {
                    chain.append_chain(if settings.initial_side >= 0 {
                        &right
                    } else {
                        &left
                    });
                    chain.append_chain(&cl_base.reverse());
                    chain.set_closed(true);

                    return chain;
                }
            } else if let Some(cl_coupled) = &self.base_line_coupled {
                let amplitude = clamped_max_amplitude
                    + self.track_width
                    + ki_round_to::<i32, f64>(f64::from(self.diff_pair_gap) / 2.0);

                let mut chain1 = ShapeLineChain::new();
                let mut chain2 = ShapeLineChain::new();
                let half_width = ki_round_to::<i32, f64>(f64::from(self.track_width) / 2.0);

                for (line, chain) in [(&cl_base, &mut chain1), (cl_coupled, &mut chain2)] {
                    if let Some((left, right)) = line.offset_line(
                        amplitude,
                        CornerStrategy::RoundAllCorners,
                        ARC_LOW_DEF,
                        true,
                    ) {
                        if settings.initial_side >= 0 {
                            chain.append_chain(&right);
                        } else {
                            chain.append_chain(&left);
                        }

                        if let Some((left, right)) = line.offset_line(
                            half_width,
                            CornerStrategy::RoundAllCorners,
                            ARC_LOW_DEF,
                            true,
                        ) {
                            if settings.initial_side >= 0 {
                                chain.append_chain(&left.reverse());
                            } else {
                                chain.append_chain(&right.reverse());
                            }
                        }

                        chain.set_closed(true);
                    }
                }

                let mut merged = ShapePolySet::new();
                merged.boolean_add_sets(
                    &ShapePolySet::from_outline(&chain1),
                    &ShapePolySet::from_outline(&chain2),
                );

                if merged.outline_count() > 0 {
                    return merged.c_outline(0).clone();
                }
            }
        }

        // Not single-sided / fallback
        let amplitude = if self.tuning_mode == LengthTuningMode::DiffPair {
            clamped_max_amplitude + self.diff_pair_gap / 2 + self.track_width
        } else {
            clamped_max_amplitude + ki_round_to::<i32, f64>(f64::from(self.track_width) / 2.0)
        };

        let mut poly = ShapePolySet::new();
        poly.offset_line_chain(
            base_line,
            amplitude,
            CornerStrategy::RoundAllCorners,
            ARC_LOW_DEF,
            false,
        );

        if self.tuning_mode == LengthTuningMode::DiffPair {
            if let Some(coupled) = &self.base_line_coupled {
                let mut poly_coupled = ShapePolySet::new();
                poly_coupled.offset_line_chain(
                    coupled,
                    amplitude,
                    CornerStrategy::RoundAllCorners,
                    ARC_LOW_DEF,
                    false,
                );

                poly.clear_arcs();
                poly_coupled.clear_arcs();

                let mut merged = ShapePolySet::new();
                merged.boolean_add_sets(&poly, &poly_coupled);

                if merged.outline_count() > 0 {
                    return merged.c_outline(0).clone();
                }
            }
        }

        if poly.outline_count() > 0 {
            return poly.c_outline(0).clone();
        }

        ShapeLineChain::new()
    }
}

// ---- PCB_VIA_STACK -----------------------------------------------------------------------

/// `VIA_STACK_STYLE` (`generators/pcb_via_stack.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViaStackStyle {
    Stacked,
    Staggered,
}

/// The typed members of `PCB_VIA_STACK` that its properties carry (plus `m_origin`).
#[derive(Clone, Debug)]
pub struct ViaStackData {
    pub origin: Vector2I,             // PCB_GENERATOR::m_origin
    pub start_layer: PcbLayerId,      // m_startLayer
    pub end_layer: PcbLayerId,        // m_endLayer
    pub style: ViaStackStyle,         // m_style
    pub pitch: i32,                   // m_pitch
    pub filled: bool,                 // m_filled
    pub capped: bool,                 // m_capped
    pub via_size: i32,                // m_viaSize
    pub via_drill: i32,               // m_viaDrill
    pub use_netclass: bool,           // m_useNetclass
    pub preset_name: String,          // m_presetName
    pub hops: Option<ShapeLineChain>, // m_hops
}

impl Default for ViaStackData {
    /// `PCB_VIA_STACK( nullptr, F_Cu )`.
    fn default() -> Self {
        ViaStackData {
            origin: Vector2I::default(),
            start_layer: PcbLayerId::FCu,
            end_layer: PcbLayerId::In1Cu,
            style: ViaStackStyle::Stacked,
            pitch: 0,
            filled: true,
            capped: false,
            via_size: 0,
            via_drill: 0,
            use_netclass: false,
            preset_name: String::new(),
            hops: None,
        }
    }
}

/// `layerFromName( aName, aFallback )` (pcb_via_stack.cpp): resolve a canonical copper layer
/// name back to its id without needing a board.
fn layer_from_name(name: &str, fallback: PcbLayerId) -> PcbLayerId {
    let layer = Lset::name_to_layer(name);

    if layer >= 0 && layer < PCB_LAYER_ID_COUNT && (layer & 1) == 0 && is_copper_layer(layer) {
        return to_layer_id(layer);
    }

    fallback
}

impl ViaStackData {
    /// Port of the static `PCB_VIA_STACK::IsSpanValid( aBoard, aStart, aEnd )`: both ends are
    /// copper layers enabled on the board and they differ (`false` without a board).
    pub fn is_span_valid(
        board: Option<&crate::board::Board>,
        start: PcbLayerId,
        end: PcbLayerId,
    ) -> bool {
        let Some(board) = board else {
            return false;
        };

        let enabled = Lset::all_cu_mask_n(board.copper_layer_count());

        let present = |layer: PcbLayerId| {
            layer.0 >= 0 && layer.0 < PCB_LAYER_ID_COUNT && enabled.contains(layer)
        };

        present(start) && present(end) && start != end
    }

    /// `PCB_VIA_STACK::SetProperties( aProps )` on a default-constructed stack; `layer` is the
    /// generator's `m_layer`, which `SetStartLayer` also sets.
    fn from_properties(props: &PropertyMap<'_>, layer: &mut PcbLayerId) -> ViaStackData {
        let mut data = ViaStackData::default();

        // PCB_GENERATOR::SetProperties
        props.get_to_vector(KEY_ORIGIN, &mut data.origin);

        let mut layer_name = String::new();

        if props.get_to_string("start_layer", &mut layer_name) {
            // SetStartLayer(): m_startLayer = aLayer; SetLayer( aLayer )
            data.start_layer = layer_from_name(&layer_name, data.start_layer);
            *layer = data.start_layer;
        }

        if props.get_to_string("end_layer", &mut layer_name) {
            data.end_layer = layer_from_name(&layer_name, data.end_layer);
        }

        let mut style = String::new();

        if props.get_to_string("style", &mut style) {
            // styleFromString()
            data.style = if style == "staggered" {
                ViaStackStyle::Staggered
            } else {
                ViaStackStyle::Stacked
            };
        }

        props.get_to_bool("filled", &mut data.filled);
        props.get_to_bool("capped", &mut data.capped);
        props.get_to_bool("use_netclass", &mut data.use_netclass);
        props.get_to_string("preset", &mut data.preset_name);

        props.get_to_iu_i32("pitch", &mut data.pitch);
        props.get_to_iu_i32("via_size", &mut data.via_size);
        props.get_to_iu_i32("via_drill", &mut data.via_drill);

        if let Some(hops) = props.get_opt_chain("hops") {
            data.hops = Some(hops);
        }

        data
    }

    /// `PCB_VIA_STACK::GetProperties()`.
    fn to_properties(&self) -> BTreeMap<String, GeneratorProp> {
        let mut props = PropertySink::default();

        // PCB_GENERATOR::GetProperties
        props.set(KEY_ORIGIN, GeneratorProp::Vector2I(self.origin));

        props.set_string("start_layer", &Lset::name(self.start_layer));
        props.set_string("end_layer", &Lset::name(self.end_layer));
        // styleToString()
        props.set_string(
            "style",
            if self.style == ViaStackStyle::Staggered {
                "staggered"
            } else {
                "stacked"
            },
        );
        props.set_bool("filled", self.filled);
        props.set_bool("capped", self.capped);
        props.set_bool("use_netclass", self.use_netclass);
        props.set_string("preset", &self.preset_name);

        props.set_iu("pitch", i64::from(self.pitch));
        props.set_iu("via_size", i64::from(self.via_size));
        props.set_iu("via_drill", i64::from(self.via_drill));

        if let Some(hops) = &self.hops {
            props.set("hops", GeneratorProp::LineChain(hops.clone()));
        }

        props.0
    }
}

// ---- PCB_VIA_STITCH ----------------------------------------------------------------------

/// The typed members of `PCB_VIA_STITCH` (and its `PCB_GENERATOR_POLY` outline) that its
/// properties carry, plus `m_origin`. `layout` and `mode` keep the integer values of
/// `PCB_VIA_STITCH_LAYOUT` (PLAIN, STAGGERED, POISSON) and `PCB_VIA_STITCH_MODE` (STITCH,
/// GUARD), which is what C++ stores after a `static_cast` of the file value.
#[derive(Clone, Debug, Default)]
pub struct ViaStitchData {
    pub origin: Vector2I,                         // PCB_GENERATOR::m_origin
    pub pitch: i32,                               // m_pitch
    pub layout: i32,                              // m_layout
    pub mode: i32,                                // m_mode
    pub seed: u32,                                // m_seed
    pub origin_offset: Vector2I,                  // m_originOffset
    pub last_net_name: String,                    // m_lastNetName
    pub last_guarded_net_name: String,            // m_lastGuardedNetName
    pub excluded_cells: BTreeSet<(i32, i32)>,     // m_excludedCells (std::set<VECTOR2I>, x then y)
    pub excluded_positions: BTreeSet<(i32, i32)>, // m_excludedPositions
    /// PCB_GENERATOR_POLY::m_outline: the single outline (closed), `None` = no contour.
    pub outline: Option<ShapeLineChain>,
}

impl ViaStitchData {
    /// `PCB_VIA_STITCH::SetProperties( aProps )` on a default-constructed generator.
    fn from_properties(props: &PropertyMap<'_>) -> ViaStitchData {
        let mut data = ViaStitchData::default();

        // PCB_GENERATOR::SetProperties
        props.get_to_vector(KEY_ORIGIN, &mut data.origin);

        props.get_to_iu_i32("pitch", &mut data.pitch);

        if let Some(layout) = props.get_opt_i32("layout") {
            data.layout = layout;
        }

        if let Some(mode) = props.get_opt_i32("mode") {
            data.mode = mode;
        }

        if let Some(seed) = props.get_opt_i32("seed") {
            data.seed = seed as u32;
        }

        props.get_to_iu_i32("origin_offset_x", &mut data.origin_offset.x);
        props.get_to_iu_i32("origin_offset_y", &mut data.origin_offset.y);

        // m_childGridConfig is derived, not a property.

        // Restore net name; net code is resolved from the board in EditStart()/Update()
        props.get_to_string("net_name", &mut data.last_net_name);
        props.get_to_string("guarded_net_name", &mut data.last_guarded_net_name);

        if let Some(cells) = props.get_opt_cells("excluded_grid_cells") {
            data.excluded_cells
                .extend(cells.iter().map(|cell| (cell.x, cell.y)));
        }

        if let Some(chain) = props.get_opt_chain("excluded_positions") {
            for i in 0..chain.point_count() {
                let p = chain.c_point(i);
                data.excluded_positions.insert((p.x, p.y));
            }
        }

        // Restore the single zone outline
        if let Some(mut outline) = props.get_opt_chain(KEY_OUTLINE) {
            // A chain parsed from the file is a bare point list; the closed flag is not
            // serialized and AddOutline() asserts on open chains
            outline.set_closed(true);
            data.outline = Some(outline);
        }

        data
    }

    /// `PCB_GENERATOR_POLY::m_outline` as a poly set (`RemoveAllContours` + `AddOutline`).
    pub fn outline_set(&self) -> ShapePolySet {
        let mut set = ShapePolySet::new();

        if let Some(outline) = &self.outline {
            set.add_outline(outline);
        }

        set
    }

    /// `PCB_VIA_STITCH::GetProperties()`.
    fn to_properties(&self) -> BTreeMap<String, GeneratorProp> {
        let mut props = PropertySink::default();

        // PCB_GENERATOR::GetProperties
        props.set(KEY_ORIGIN, GeneratorProp::Vector2I(self.origin));

        props.set_iu("pitch", i64::from(self.pitch));
        props.set_number("layout", f64::from(self.layout));
        props.set_number("mode", f64::from(self.mode));
        props.set_number("seed", f64::from(self.seed as i32));
        props.set_iu("origin_offset_x", i64::from(self.origin_offset.x));
        props.set_iu("origin_offset_y", i64::from(self.origin_offset.y));

        // Save the net by name so it survives across sessions (net codes are session-local)
        if !self.last_net_name.is_empty() {
            props.set_string("net_name", &self.last_net_name);
        }

        if !self.last_guarded_net_name.is_empty() {
            props.set_string("guarded_net_name", &self.last_guarded_net_name);
        }

        if !self.excluded_cells.is_empty() {
            let cells = self
                .excluded_cells
                .iter()
                .map(|&(x, y)| Vector2I::new(x, y))
                .collect();
            props.set("excluded_grid_cells", GeneratorProp::Cells(cells));
        }

        if !self.excluded_positions.is_empty() {
            let mut chain = ShapeLineChain::new();

            for &(x, y) in &self.excluded_positions {
                chain.append_point(Vector2I::new(x, y), false);
            }

            props.set("excluded_positions", GeneratorProp::LineChain(chain));
        }

        // Serialize the single zone outline as a SHAPE_LINE_CHAIN.
        // The file format natively supports SHAPE_LINE_CHAIN but not SHAPE_POLY_SET directly.
        if let Some(outline) = &self.outline {
            props.set(KEY_OUTLINE, GeneratorProp::LineChain(outline.clone()));
        }

        props.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- differential test against the reference build ------------------------------------

    /// A minimal s-expression reader for the golden records (test only).
    #[derive(Debug, Clone)]
    enum Sx {
        Atom(String),
        Str(String),
        List(Vec<Sx>),
    }

    fn sx_parse(text: &str) -> Sx {
        fn parse(chars: &[char], i: &mut usize) -> Sx {
            while chars[*i].is_whitespace() {
                *i += 1;
            }

            if chars[*i] == '(' {
                *i += 1;
                let mut items = Vec::new();

                loop {
                    while chars[*i].is_whitespace() {
                        *i += 1;
                    }

                    if chars[*i] == ')' {
                        *i += 1;
                        return Sx::List(items);
                    }

                    items.push(parse(chars, i));
                }
            } else if chars[*i] == '"' {
                *i += 1;
                let mut text = String::new();

                while chars[*i] != '"' {
                    if chars[*i] == '\\' {
                        *i += 1;
                    }

                    text.push(chars[*i]);
                    *i += 1;
                }

                *i += 1;
                Sx::Str(text)
            } else {
                let mut text = String::new();

                while !chars[*i].is_whitespace() && chars[*i] != '(' && chars[*i] != ')' {
                    text.push(chars[*i]);
                    *i += 1;
                }

                Sx::Atom(text)
            }
        }

        let chars: Vec<char> = text.chars().collect();
        parse(&chars, &mut 0)
    }

    /// `parseBoardUnits()` of the test values (all in range).
    fn board_units(text: &str) -> i32 {
        ki_round_to::<i32, f64>(text.parse::<f64>().unwrap() * PCB_IU_PER_MM)
    }

    fn xy(item: &Sx) -> Vector2I {
        match item {
            Sx::List(v) => match (&v[0], &v[1], &v[2]) {
                (Sx::Atom(t), Sx::Atom(x), Sx::Atom(y)) if t == "xy" => {
                    Vector2I::new(board_units(x), board_units(y))
                }
                _ => panic!("not an xy: {item:?}"),
            },
            _ => panic!("not an xy: {item:?}"),
        }
    }

    /// The `(generated ...)` block as the C++ loader records it (`parseGENERATOR`): the layer
    /// and the property map with the raw text of every number (for the `{:.10g}` comparison).
    struct ParsedGenerator {
        generator_type: String,
        layer: String,
        properties: BTreeMap<String, GeneratorProp>,
        number_text: BTreeMap<String, String>,
    }

    fn parse_generator_block(text: &str) -> ParsedGenerator {
        let Sx::List(items) = sx_parse(text) else {
            panic!()
        };

        let mut parsed = ParsedGenerator {
            generator_type: String::new(),
            layer: String::new(),
            properties: BTreeMap::new(),
            number_text: BTreeMap::new(),
        };

        for item in &items[1..] {
            let Sx::List(entry) = item else { continue };
            let Sx::Atom(key) = &entry[0] else { panic!() };

            match key.as_str() {
                "uuid" | "id" | "name" | "locked" | "members" | "templates" | "property" => {}
                "type" => {
                    let Sx::Atom(t) = &entry[1] else { panic!() };
                    parsed.generator_type = t.clone();
                }
                "layer" => {
                    let (Sx::Str(l) | Sx::Atom(l)) = &entry[1] else {
                        panic!()
                    };
                    parsed.layer = l.clone();
                }
                _ => {
                    // emplace: the first entry of a key wins
                    if parsed.properties.contains_key(key) {
                        continue;
                    }

                    let value = match &entry[1] {
                        Sx::Atom(a) if a == "yes" => GeneratorProp::Bool(true),
                        Sx::Atom(a) if a == "no" => GeneratorProp::Bool(false),
                        Sx::Atom(a) => {
                            parsed.number_text.insert(key.clone(), a.clone());
                            GeneratorProp::Number(a.parse::<f64>().unwrap())
                        }
                        Sx::Str(t) => GeneratorProp::String(t.clone()),
                        Sx::List(sub) => match &sub[0] {
                            Sx::Atom(t) if t == "xy" => GeneratorProp::Vector2I(xy(&entry[1])),
                            Sx::Atom(t) if t == "pts" => {
                                let points: Vec<Vector2I> = sub[1..].iter().map(xy).collect();
                                GeneratorProp::LineChain(ShapeLineChain::from_points(
                                    &points, false,
                                ))
                            }
                            Sx::Atom(t) if t == "cells" => GeneratorProp::Cells(
                                sub[1..]
                                    .iter()
                                    .map(|c| match c {
                                        Sx::List(ij) => match (&ij[1], &ij[2]) {
                                            (Sx::Atom(i), Sx::Atom(j)) => Vector2I::new(
                                                i.parse().unwrap(),
                                                j.parse().unwrap(),
                                            ),
                                            _ => panic!(),
                                        },
                                        _ => panic!(),
                                    })
                                    .collect(),
                            ),
                            _ => panic!("unexpected property {key}"),
                        },
                    };

                    parsed.properties.insert(key.clone(), value);
                }
            }
        }

        parsed
    }

    fn chain_points(chain: &ShapeLineChain) -> Vec<Vector2I> {
        (0..chain.point_count()).map(|i| chain.c_point(i)).collect()
    }

    /// `PCB_GENERATOR::SetProperties` + `GetProperties` (what `kicad-cli pcb upgrade` writes)
    /// for every generator of 15 corpus boards (36 generators: 27 tuning patterns of every
    /// mode and file vintage, 4 via stacks, 5 via stitches). Golden:
    /// `tests/golden/m23_generator_properties.txt`, regenerated by the `.sh` next to it. Each
    /// number is compared as the writer prints it (`fmt::format( "{:.10g}" )`).
    #[test]
    fn normalize_properties_matches_the_oracle() {
        let golden = include_str!("../tests/golden/m23_generator_properties.txt");
        let lines: Vec<&str> = golden.lines().collect();
        let mut checked = 0;
        let mut i = 0;

        while i < lines.len() {
            let header = lines[i];
            assert!(header.starts_with("G|"), "{header}");
            let input = lines[i + 1].strip_prefix("I|").unwrap();
            let output = lines[i + 2];
            i += 3;

            let Some(output) = output.strip_prefix("O|") else {
                // dropped by the writer (an empty tuning pattern / via stack)
                continue;
            };

            let parsed_in = parse_generator_block(input);
            let expected = parse_generator_block(output);

            let mut generator =
                PcbGenerator::create_from_type(&parsed_in.generator_type).expect(header);
            // gen->SetLayer( genInfo->layer ); gen->SetProperties( genInfo->properties )
            generator.group.base.layer =
                kicad_core::layer_ids::to_layer_id(Lset::name_to_layer(&parsed_in.layer));
            generator.properties = parsed_in.properties.clone();
            generator.normalize_properties();

            let keys: Vec<&String> = generator.properties.keys().collect();
            let expected_keys: Vec<&String> = expected.properties.keys().collect();
            assert_eq!(keys, expected_keys, "{header}");

            for (key, value) in &generator.properties {
                let want = &expected.properties[key];

                match (value, want) {
                    (GeneratorProp::Number(v), GeneratorProp::Number(_)) => {
                        assert_eq!(
                            &kicad_sexpr::format_g(*v, 10),
                            &expected.number_text[key],
                            "{header} {key}"
                        );
                    }
                    (GeneratorProp::Bool(a), GeneratorProp::Bool(b)) => {
                        assert_eq!(a, b, "{header} {key}")
                    }
                    (GeneratorProp::String(a), GeneratorProp::String(b)) => {
                        assert_eq!(a, b, "{header} {key}")
                    }
                    (GeneratorProp::Vector2I(a), GeneratorProp::Vector2I(b)) => {
                        assert_eq!(a, b, "{header} {key}")
                    }
                    (GeneratorProp::LineChain(a), GeneratorProp::LineChain(b)) => {
                        assert_eq!(chain_points(a), chain_points(b), "{header} {key}")
                    }
                    (GeneratorProp::Cells(a), GeneratorProp::Cells(b)) => {
                        assert_eq!(a, b, "{header} {key}")
                    }
                    _ => panic!("{header} {key}: {value:?} vs {want:?}"),
                }
            }

            // (origin) is a property; the via stack's layer follows its start layer.
            if generator.generator_type == GENERATOR_TYPE_VIA_STACK {
                assert_eq!(
                    Lset::name(generator.group.base.layer),
                    expected.layer,
                    "{header}"
                );
            }

            checked += 1;
        }

        assert_eq!(checked, 35);
    }

    /// `wxAny::GetAs` conversions as measured on the reference build (wx 3.2).
    #[test]
    fn wx_any_conversions() {
        let mut map = BTreeMap::new();
        for (key, value) in [
            ("a", 80.5),
            ("b", -2.5),
            ("c", 3e9),
            ("d", 1e19),
            ("e", 0.7),
        ] {
            map.insert(key.to_string(), GeneratorProp::Number(value));
        }
        map.insert("f".to_string(), GeneratorProp::Bool(true));
        map.insert("g".to_string(), GeneratorProp::String("yes".to_string()));
        map.insert("h".to_string(), GeneratorProp::Number(1.5));
        let props = PropertyMap(&map);

        let int = |key: &str| props.get_opt_i32(key);
        assert_eq!(int("a"), Some(80));
        assert_eq!(int("b"), Some(-2));
        assert_eq!(int("c"), Some(-1_294_967_296));
        assert_eq!(int("d"), None);
        assert_eq!(int("e"), Some(0));
        assert_eq!(int("f"), Some(1));

        let mut b = true;
        assert!(!props.get_to_bool("h", &mut b));
        assert!(b);
        assert!(props.get_to_bool("g", &mut b));
        assert!(b);

        let mut text = String::from("unchanged");
        assert!(props.get_to_string("h", &mut text));
        assert_eq!(text, "1.50000000000000");
        assert!(props.get_to_string("f", &mut text));
        assert_eq!(text, "true");

        // get_to_iu rounds (KiROUND), get_to truncates
        let mut v = 0;
        map.insert("mm".to_string(), GeneratorProp::Number(0.1));
        let props = PropertyMap(&map);
        assert!(props.get_to_iu_i32("mm", &mut v));
        assert_eq!(v, 100_000);
    }

    /// `PCB_VIA_STITCH::SetProperties` keeps the excluded cells and positions in
    /// `std::set<VECTOR2I>` (lexicographic, deduplicated); `GetProperties` writes them sorted.
    #[test]
    fn via_stitch_exclusions_are_sorted_sets() {
        let mut g = PcbGenerator::create_from_type(GENERATOR_TYPE_VIA_STITCH).unwrap();
        g.properties.insert(
            "excluded_grid_cells".to_string(),
            GeneratorProp::Cells(vec![
                Vector2I::new(3, 1),
                Vector2I::new(1, 2),
                Vector2I::new(1, 1),
                Vector2I::new(3, 1),
            ]),
        );
        g.properties
            .insert("seed".to_string(), GeneratorProp::Number(4_294_967_295.0));
        g.normalize_properties();

        let GeneratorProp::Cells(cells) = &g.properties["excluded_grid_cells"] else {
            panic!()
        };
        assert_eq!(
            cells,
            &vec![
                Vector2I::new(1, 1),
                Vector2I::new(1, 2),
                Vector2I::new(3, 1)
            ]
        );
        // static_cast<uint32_t>( int ) of the truncated value, written back as an int
        let GeneratorProp::Number(seed) = g.properties["seed"] else {
            panic!()
        };
        assert_eq!(seed, -1.0);
        assert!(!g.properties.contains_key("outline"));
        assert!(!g.properties.contains_key("net_name"));
    }

    // ---- model behaviour against pcb-dump ---------------------------------------------------

    /// `tests/data/m23/m23_items.kicad_pcb` built by hand in the model (the loader's steps for
    /// groups and generators: create, layer, `SetProperties`, members, layer from the first
    /// member track). Expected values: `pcb-dump board --sections item,bbox,misc` of the
    /// reference build on a copy of that file.
    #[test]
    fn m23_items_board_matches_pcb_dump() {
        use crate::board::{AddMode, Board};
        use crate::ctx::MutEnv;
        use crate::ids::ItemId;
        use crate::pcb_group::PcbGroup;
        use crate::pcb_point::PcbPoint;
        use crate::pcb_target::PcbTarget;
        use crate::pcb_track::{PcbTrack, Track};

        let uuid = |n: u32| Kiid::parse(&format!("00000000-0000-0000-0000-{n:012}")).unwrap();
        let mut board = Board::new();

        let segment = |board: &mut Board, n, a: (f64, f64), b: (f64, f64), w, layer| {
            let mut t = PcbTrack::new(&mut board.kiid_generator);
            t.start = Vector2I::new(PCB_IU_SCALE.mm_to_iu(a.0), PCB_IU_SCALE.mm_to_iu(a.1));
            t.end = Vector2I::new(PCB_IU_SCALE.mm_to_iu(b.0), PCB_IU_SCALE.mm_to_iu(b.1));
            t.width = PCB_IU_SCALE.mm_to_iu(w);
            t.base.item.layer = layer;
            t.base.item.uuid = uuid(n);
            ItemId::from(
                board
                    .add_track(Track::Trace(t), AddMode::BulkAppend)
                    .unwrap(),
            )
        };
        let t1 = segment(
            &mut board,
            1,
            (10.0, 10.0),
            (20.0, 10.0),
            0.25,
            PcbLayerId::FCu,
        );
        let t2 = segment(
            &mut board,
            2,
            (20.0, 10.0),
            (20.0, 15.5),
            0.3,
            PcbLayerId::BCu,
        );
        let t3 = segment(
            &mut board,
            3,
            (30.0, 20.0),
            (40.0, 20.0),
            0.2,
            PcbLayerId::BCu,
        );
        let t4 = segment(
            &mut board,
            4,
            (50.0, 30.0),
            (60.0, 31.0),
            0.2,
            PcbLayerId::FCu,
        );

        let mut target = PcbTarget::default();
        target.base.uuid = uuid(5);
        target.pos = Vector2I::new(5_000_000, 6_000_000);
        target.size = 4_000_000;
        target.line_width = 150_000;
        let target: ItemId = board.add_target(target, AddMode::BulkAppend).into();

        let mut target_x = PcbTarget::default();
        target_x.base.uuid = uuid(6);
        target_x.shape = 1;
        target_x.base.layer = PcbLayerId::FSilkS;
        target_x.pos = Vector2I::new(7_500_000, 8_250_000);
        target_x.size = 3_000_000;
        let target_x: ItemId = board.add_target(target_x, AddMode::BulkAppend).into();

        let mut point = PcbPoint::default();
        point.base.uuid = uuid(7);
        point.base.layer = PcbLayerId::FSilkS;
        point.set_size(1_500_000);
        point.set_position(&MutEnv::detached(), Vector2I::new(12_300_000, 4_560_000));
        let point: ItemId = board.add_point(point, AddMode::BulkAppend).into();

        let mut g1 = PcbGroup::default();
        g1.base.uuid = uuid(10);
        g1.set_name("G1");
        let g1: ItemId = board.add_group(g1, AddMode::BulkAppend).into();
        let mut g2 = PcbGroup::default();
        g2.base.uuid = uuid(11);
        let g2: ItemId = board.add_group(g2, AddMode::BulkAppend).into();

        let generator = |board: &mut Board, n, gen_type: &str, props: &str| -> ItemId {
            let parsed = parse_generator_block(props);
            let mut g = PcbGenerator::create_from_type(gen_type).unwrap();
            g.group.base.uuid = uuid(n);
            g.group.base.layer = PcbLayerId::FCu;
            g.properties = parsed.properties;
            g.normalize_properties();
            board.add_generator(g, AddMode::BulkAppend).into()
        };
        let p1 = generator(
            &mut board,
            20,
            GENERATOR_TYPE_TUNING_PATTERN,
            "(generated (base_line (pts (xy 30 20) (xy 40 20))) (corner_radius_percent 80) \
             (end (xy 40 20)) (initial_side \"left\") (last_diff_pair_gap 0.18) \
             (last_netname \"N1\") (last_status \"tuned\") (last_track_width 0.2) \
             (max_amplitude 1.5) (min_amplitude 0.2) (min_spacing 0.6) (origin (xy 30 20)) \
             (override_custom_rules no) (rounded yes) (single_sided no) (target_length 12) \
             (tuning_mode \"single\"))",
        );
        let p2 = generator(
            &mut board,
            21,
            GENERATOR_TYPE_TUNING_PATTERN,
            "(generated (base_line (pts (xy 50 30) (xy 55 32) (xy 60 31))) (end (xy 60 31)) \
             (initial_side \"right\") (last_track_width 0.2) (max_amplitude 1) \
             (origin (xy 50 30)) (rounded no) (single_sided yes) (target_length 12) \
             (tuning_mode \"single\"))",
        );
        let stitch = generator(
            &mut board,
            22,
            GENERATOR_TYPE_VIA_STITCH,
            "(generated (layout 1) (mode 0) (origin (xy 1 2)) \
             (outline (pts (xy 70 70) (xy 80 70) (xy 80 78) (xy 70 79))) (pitch 1.5) (seed 7))",
        );

        board.add_to_group(g1, t1);
        board.add_to_group(g1, t2);
        board.add_to_group(g1, target);
        board.add_to_group(g2, g1);
        board.add_to_group(g2, point);
        board.add_to_group(p1, t3);
        board.add_to_group(p2, t4);

        // resolveGroups: a generator whose layer follows its members takes the layer of its
        // first member track.
        for p in [p1, p2] {
            let layer = BoardCtx::bare(&board).item(p).unwrap().layer();
            let id = p.typed::<PcbGenerator>().unwrap();
            board.modify_generator(id, |g, _| g.group.base.layer = layer);
        }

        board.check_invariants().unwrap();
        let ctx = BoardCtx::bare(&board);
        let b = |x, y, w, h| Box2I::new(Vector2I::new(x, y), Vector2I::new(w, h));
        let item = |id: ItemId| ctx.item(id).unwrap();

        // targets and point
        assert_eq!(
            item(target).bounding_box(),
            b(3_000_000, 4_000_000, 4_000_000, 4_000_000)
        );
        assert_eq!(
            item(target_x).bounding_box(),
            b(6_000_000, 6_750_000, 3_000_000, 3_000_000)
        );
        assert_eq!(
            item(point).bounding_box(),
            b(11_550_000, 3_810_000, 1_500_000, 1_500_000)
        );
        assert_eq!(item(point).position(), Vector2I::new(12_300_000, 4_560_000));

        // groups (nested: G1 inside the unnamed group)
        assert_eq!(
            item(g1).bounding_box(),
            b(2_750_000, 3_750_000, 17_650_001, 12_150_001)
        );
        assert_eq!(item(g1).position(), Vector2I::new(11_575_000, 9_825_000));
        assert_eq!(
            item(g1).layer_set(),
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::EdgeCuts])
        );
        assert_eq!(
            item(g2).bounding_box(),
            b(2_500_000, 3_500_000, 18_150_001, 12_650_001)
        );
        assert_eq!(item(g2).position(), Vector2I::new(11_575_000, 9_825_000));
        assert_eq!(
            item(g2).layer_set(),
            Lset::from_layers(&[
                PcbLayerId::FCu,
                PcbLayerId::BCu,
                PcbLayerId::FSilkS,
                PcbLayerId::EdgeCuts
            ])
        );
        assert!(item(g2).is_on_layer(PcbLayerId::FSilkS));
        assert!(!item(g2).is_on_layer(PcbLayerId::BSilkS));

        // generators: tuning pattern on the B.Cu member, outline-based boxes
        assert_eq!(item(p1).layer(), PcbLayerId::BCu);
        assert_eq!(item(p1).layer_set(), Lset::from_layers(&[PcbLayerId::BCu]));
        assert_eq!(item(p1).position(), Vector2I::new(30_000_000, 20_000_000));
        assert_eq!(
            item(p1).bounding_box(),
            b(29_650_000, 18_400_000, 10_700_001, 3_200_000)
        );
        assert_eq!(item(p2).layer(), PcbLayerId::FCu);
        assert_eq!(
            item(p2).bounding_box(),
            b(49_591_470, 29_650_000, 10_758_531, 3_429_088)
        );
        assert_eq!(item(stitch).layer(), PcbLayerId::FCu);
        assert_eq!(
            item(stitch).position(),
            Vector2I::new(70_000_000, 70_000_000)
        );
        assert_eq!(
            item(stitch).bounding_box(),
            b(70_000_000, 70_000_000, 10_000_000, 9_000_000)
        );

        // misc: names, members, descriptions
        assert_eq!(item(p1).as_group().unwrap().name(), "Tuning Pattern");
        assert_eq!(item(stitch).as_group().unwrap().name(), "Via Stitching");
        let units =
            kicad_core::units_provider::UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(
            item(g1).item_description(&units, false),
            "Group 'G1', 3 members"
        );
        assert_eq!(
            item(g2).item_description(&units, false),
            "Unnamed Group, 2 members"
        );
        assert_eq!(item(p1).item_description(&units, false), "Tuning Pattern");

        // group hit tests are always false; the tuning pattern answers on its outline
        assert!(
            !item(g1)
                .as_dyn()
                .hit_test(&ctx, Vector2I::new(15_000_000, 10_000_000), 0)
        );
        assert!(
            item(p1)
                .as_dyn()
                .hit_test(&ctx, Vector2I::new(35_000_000, 21_000_000), 0)
        );
        assert!(
            !item(p1)
                .as_dyn()
                .hit_test(&ctx, Vector2I::new(35_000_000, 22_000_000), 0)
        );

        // RunOnChildren( RECURSE ) of the outer group reaches G1's members
        let mut seen = Vec::new();
        item(g2)
            .as_dyn()
            .run_on_children(&ctx, RecurseMode::Recurse, &mut |child| {
                seen.push(child.id().unwrap())
            });
        assert_eq!(seen, vec![g1, t1, t2, target, point]);

        // TopLevelGroup / WithinScope
        assert_eq!(
            crate::pcb_group::top_level_group(&ctx, item(t1), None, false),
            Some(g2)
        );
        assert_eq!(
            crate::pcb_group::top_level_group(&ctx, item(t1), Some(g2), false),
            Some(g1)
        );
        assert!(crate::pcb_group::within_scope(
            &ctx,
            item(t1),
            Some(g1),
            false
        ));
        assert!(crate::pcb_group::within_scope(
            &ctx,
            item(g1),
            Some(g2),
            false
        ));
    }

    /// `PCB_GENERATOR::PCB_GENERATOR( aParent, aLayer )` (pcbnew/pcb_generator.cpp):
    /// `m_generatorType()`, `m_origin()`, `m_isDirty( false )`, layer from the argument.
    #[test]
    fn generator_constructor_defaults() {
        let g = PcbGenerator::default();
        assert_eq!(g.group.base.layer, PcbLayerId::FCu);
        assert!(g.group.base.uuid == Kiid::default());
        assert!(g.generator_type().is_empty());
        assert_eq!(g.origin, Vector2I::new(0, 0));
        assert!(!g.is_dirty());
        assert!(g.properties.is_empty());
        assert!(g.template_items().is_empty());
        assert!(g.group.members.is_empty());
        assert_eq!(BoardItem::type_(&g), KicadT::PcbGeneratorT);
        assert_eq!(BoardItem::class_name(&g), "PCB_GENERATOR");
        assert!(BoardItem::is_layer_agnostic(&g));

        let mut kiid_generator = KiidGenerator::default();
        let mut g = PcbGenerator::new(&mut kiid_generator, PcbLayerId::BCu);
        assert_eq!(g.group.base.layer, PcbLayerId::BCu);
        assert!(g.group.base.uuid != Kiid::default());
        g.mark_dirty();
        assert!(g.is_dirty());
        g.clear_dirty();
        assert!(!g.is_dirty());
    }

    /// `GetFriendlyName()` of the generator subclasses (`pcbnew/generators/*.h`), reached
    /// through the trait.
    #[test]
    fn friendly_name_of_the_subclasses() {
        let board = crate::board::Board::new();
        let ctx = &BoardCtx::bare(&board);

        for (generator_type, name) in [
            (GENERATOR_TYPE_TUNING_PATTERN, "Tuning Pattern"),
            (GENERATOR_TYPE_VIA_STACK, "Microvia Stack"),
            (GENERATOR_TYPE_VIA_STITCH, "Via Stitching"),
        ] {
            let mut g = PcbGenerator::default();
            g.generator_type = generator_type.to_string();
            assert_eq!(BoardItem::friendly_name(&g, ctx), name);
        }
    }
}
