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
//! The subclass overrides of `GetBoundingBox`, `GetPosition`, `GetLayer`, `HitTest` and
//! `GetItemDescription` are reached through the same shims: the inherent methods below
//! dispatch on `generator_type` once M23 ports them.

use std::collections::BTreeMap;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, Lset, PcbLayerId};
use kicad_geom::{Box2I, ShapeLineChain, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::{ItemRef, OwnedItem};
use crate::pcb_group::PcbGroup;
use crate::traits::{BoardItem, Flashing, Invalidate};

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
    pub fn normalize_properties(&mut self) {
        crate::port_pending!("PCB_TUNING_PATTERN::SetProperties / GetProperties")
    }

    /// `PCB_GENERATOR::GetLayer()`: `m_layer`; `PCB_TUNING_PATTERN` answers with the layer of
    /// its first member track.
    pub fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        crate::port_pending!("PCB_GENERATOR::GetLayer", ctx)
    }

    /// `PCB_GENERATOR::GetLayerSet()`: `PCB_GROUP::GetLayerSet() | LSET( { GetLayer() } )`.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        crate::port_pending!("PCB_GENERATOR::GetLayerSet", ctx)
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
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        crate::port_pending!("PCB_GENERATOR::GetPosition", ctx)
    }

    /// `PCB_GENERATOR::GetBoundingBox()`: an empty box; every subclass overrides it.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_GENERATOR::GetBoundingBox", ctx)
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
        crate::port_pending!("PCB_GENERATOR::HitTest", ctx, pos, accuracy)
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
        crate::port_pending!(
            "PCB_GENERATOR::HitTest (rect)",
            ctx,
            rect,
            contained,
            accuracy
        )
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
        full: bool,
    ) -> String {
        crate::port_pending!("PCB_GENERATOR::GetItemDescription", ctx, units, full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
