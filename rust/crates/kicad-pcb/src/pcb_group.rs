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

//! Port of `pcbnew/pcb_group.h` and `pcbnew/pcb_group.cpp` and `common/eda_group.h` /
//! `common/eda_group.cpp` (`PCB_GROUP : BOARD_ITEM, EDA_GROUP`), `board-model-types.md`
//! section 9.
//!
//! Membership is written on both sides only by `Board` (`add_to_group`, `remove_from_group`,
//! `board-model.md` 4); `EDA_GROUP::AddItem` / `RemoveItem` / `RemoveAll` therefore live there.
//! A group is a container: its mutating virtuals (`Move`, `Rotate`, `Flip`, `Mirror`,
//! `SetPosition`, `SetLocked`) cascade through `Board::move_item` etc. (`board-model.md` 8.3),
//! so it has no `BoardItemMut` impl. Behaviour belongs to package M23 (inherent stubs below).

use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, LibId, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{Box2I, ShapeCompound, Vector2I};

use crate::ctx::BoardCtx;
use crate::drc_api::DrcConstraintT;
use crate::ids::ItemId;
use crate::item::{BoardItemBase, RecurseMode, SharedShape};
use crate::item_ref::ItemRef;
use crate::traits::{BoardItem, Flashing, Invalidate};

/// `PCB_GROUP : BOARD_ITEM, EDA_GROUP`.
#[derive(Clone, Debug)]
pub struct PcbGroup {
    pub base: BoardItemBase,
    pub name: String, // EDA_GROUP::m_name
    /// EDA_GROUP::m_items (insertion ordered, unique; C++ `unordered_set`, see PORT_STATUS.md).
    pub members: Vec<ItemId>,
    pub design_block_lib_id: LibId, // EDA_GROUP::m_designBlockLibId
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbGroup {
    /// `PCB_GROUP( aParent )` (`pcb_group.cpp`): `BOARD_ITEM( aParent, PCB_GROUP_T )`, i.e. layer
    /// `F_Cu`; no name, no members, no design block link; nil UUID.
    fn default() -> Self {
        PcbGroup {
            base: BoardItemBase::default(),
            name: String::new(),
            members: Vec::new(),
            design_block_lib_id: LibId::default(),
        }
    }
}

impl PcbGroup {
    /// `new PCB_GROUP( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut group = PcbGroup::default();
        group.base.uuid = Kiid::new(kiid_generator);
        group
    }

    /// `PCB_GROUP( aParent, idtype, aLayer )` (protected, used by `PCB_GENERATOR`): the group
    /// part of a derived item on `layer`, nil UUID.
    pub fn with_layer(layer: PcbLayerId) -> Self {
        PcbGroup {
            base: BoardItemBase::with_layer(layer),
            ..PcbGroup::default()
        }
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbGroup {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbGroup {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbGroupT
    }
    fn class_name(&self) -> &'static str {
        "PCB_GROUP"
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbGroup::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbGroup::layer_set(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbGroup::is_on_layer(self, ctx, layer)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbGroup::is_on_copper_layer(self, ctx)
    }
    fn is_layer_agnostic(&self) -> bool {
        PcbGroup::is_layer_agnostic(self)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbGroup::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbGroup::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbGroup::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbGroup::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbGroup::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        PcbGroup::run_on_children(self, ctx, mode, f)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbGroup::item_description(self, ctx, units, full)
    }
}

// ---- inherent methods: EDA_GROUP (package M23) --------------------------------------------

impl PcbGroup {
    /// `EDA_GROUP::GetName()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `EDA_GROUP::SetName()`.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// `EDA_GROUP::GetItems()` as handles, in insertion order.
    pub fn items(&self) -> &[ItemId] {
        &self.members
    }

    /// `EDA_GROUP::ContainsItem( aItem )`.
    pub fn contains_item(&self, item: ItemId) -> bool {
        self.members.contains(&item)
    }

    /// `EDA_GROUP::GetGroupMemberIds()`: the members' UUIDs (C++ iterates the
    /// `unordered_set`; here insertion order, see PORT_STATUS.md).
    pub fn group_member_ids(&self, ctx: &BoardCtx<'_>) -> Vec<Kiid> {
        self.members
            .iter()
            .filter_map(|&member| ctx.item(member))
            .map(|item| item.uuid())
            .collect()
    }

    /// `PCB_GROUP::GetBoardItems()`: the members that are board items (all of them: a Rust
    /// member is always a board item), resolved; stale handles are skipped. C++ iterates an
    /// `unordered_set` here; the Rust order is insertion order (PORT_STATUS.md).
    pub fn board_items<'a>(&self, ctx: &'a BoardCtx<'a>) -> Vec<ItemRef<'a>> {
        self.members
            .iter()
            .filter_map(|&member| ctx.item(member))
            .collect()
    }

    /// `EDA_GROUP::HasDesignBlockLink()`: `m_designBlockLibId.IsValid()`.
    pub fn has_design_block_link(&self) -> bool {
        self.design_block_lib_id.is_valid()
    }

    /// `EDA_GROUP::SetDesignBlockLibId()`.
    pub fn set_design_block_lib_id(&mut self, lib_id: LibId) {
        self.design_block_lib_id = lib_id;
    }

    /// `EDA_GROUP::GetDesignBlockLibId()`.
    pub fn design_block_lib_id(&self) -> &LibId {
        &self.design_block_lib_id
    }
}

// ---- inherent methods: PCB_GROUP virtuals (package M23) -----------------------------------

impl PcbGroup {
    /// `PCB_GROUP::GetLayer()`: "isn't well-defined" (`wxFAIL_MSG`), `UNDEFINED_LAYER`.
    pub fn layer(&self, _ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbLayerId::UndefinedLayer
    }

    /// `PCB_GROUP::GetLayerSet()`: the union of the members' layer sets.
    pub fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        let mut set = Lset::new();

        for item in self.board_items(ctx) {
            set |= item.layer_set();
        }

        set
    }

    /// `PCB_GROUP::IsOnLayer()`: true if any member is on `layer`.
    pub fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        // A group is on a layer if any item is on the layer
        self.board_items(ctx)
            .iter()
            .any(|item| item.is_on_layer(layer))
    }

    /// `PCB_GROUP::IsOnCopperLayer()`: false.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        false
    }

    /// `PCB_GROUP::IsLayerAgnostic()`: true.
    pub fn is_layer_agnostic(&self) -> bool {
        true
    }

    /// `PCB_GROUP::GetPosition()`: `GetBoundingBox().Centre()`.
    pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.bounding_box(ctx).centre()
    }

    /// `PCB_GROUP::GetBoundingBox()`: members merged (footprints with their text-including
    /// box), inflated by 0.25 mm.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut bbox = Box2I::default();

        for item in self.board_items(ctx) {
            if let Some(fp) = item.as_footprint() {
                bbox.merge(&fp.bounding_box_with_text(ctx, true));
            } else {
                bbox.merge(&item.bounding_box());
            }
        }

        bbox.inflate(PCB_IU_SCALE.mm_to_iu(0.25)); // Give a min size to the bbox

        bbox
    }

    /// `PCB_GROUP::GetEffectiveShape()`: a compound of the members' effective shapes.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        let mut shape = ShapeCompound::new();

        for item in self.board_items(ctx) {
            shape.add_shape(item.effective_shape(layer, flash, usage).clone_shape());
        }

        Arc::new(shape)
    }

    /// `PCB_GROUP::HitTest( aPosition, aAccuracy )`: false (groups are selected by promoting
    /// a selection of one of their children).
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, _pos: Vector2I, _accuracy: i32) -> bool {
        false
    }

    /// `PCB_GROUP::HitTest( aRect, aContained, aAccuracy )`: false.
    pub fn hit_test_rect(
        &self,
        _ctx: &BoardCtx<'_>,
        _rect: &Box2I,
        _contained: bool,
        _accuracy: i32,
    ) -> bool {
        false
    }

    /// `PCB_GROUP::RunOnChildren( aFunction, aMode )`: every member, recursing into member
    /// groups and generators for `RECURSE`.
    pub fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        for item in self.board_items(ctx) {
            f(item);

            if mode == RecurseMode::Recurse
                && (item.type_() == KicadT::PcbGroupT || item.type_() == KicadT::PcbGeneratorT)
            {
                item.as_dyn().run_on_children(ctx, RecurseMode::Recurse, f);
            }
        }
    }

    /// `PCB_GROUP::GetItemDescription()`: "Unnamed Group, %zu members" / "Group '%s', %zu
    /// members".
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = (ctx, units);

        if self.name.is_empty() {
            format!("Unnamed Group, {} members", self.members.len())
        } else {
            format!("Group '{}', {} members", self.name, self.members.len())
        }
    }

    /// `PCB_GROUP::operator==( const PCB_GROUP& )`: same member count and the same member UUIDs
    /// (compared in UUID order, `EDA_ITEM_SET`).
    pub fn eq_item(&self, ctx: &BoardCtx<'_>, other: &PcbGroup) -> bool {
        if self.members.len() != other.members.len() {
            return false;
        }

        let mut mine = self.group_member_ids(ctx);
        let mut theirs = other.group_member_ids(ctx);
        mine.sort();
        theirs.sort();
        mine == theirs
    }
}

// ---- free functions: PCB_GROUP statics -----------------------------------------------------

/// `getClosestGroup( aItem, isFootprintEditor )` (pcb_group.cpp): outside the footprint editor
/// an item inside a footprint answers with the footprint's group, otherwise with its own.
fn closest_group(item: ItemRef<'_>, is_footprint_editor: bool) -> Option<ItemId> {
    if !is_footprint_editor {
        if let Some(parent) = item.parent() {
            if parent.type_() == KicadT::PcbFootprintT {
                return parent.parent_group().and_then(|g| g.id());
            }
        }
    }

    item.parent_group().and_then(|g| g.id())
}

/// `getNestedGroup( aItem, aScope, isFootprintEditor )`: the top level group inside the
/// `scope` group (`None` = no scope), or `None`.
fn nested_group(
    ctx: &BoardCtx<'_>,
    item: ItemRef<'_>,
    scope: Option<ItemId>,
    is_footprint_editor: bool,
) -> Option<ItemId> {
    let mut group = closest_group(item, is_footprint_editor);

    if group == scope {
        return None;
    }

    while let Some(g) = group {
        let Some(parent) = ctx
            .item(g)
            .and_then(|g| g.parent_group())
            .and_then(|p| p.id())
        else {
            break;
        };

        if Some(parent) == scope {
            break;
        }

        group = Some(parent);
    }

    group
}

/// `PCB_GROUP::TopLevelGroup( aItem, aScope, isFootprintEditor )`: the top level group inside
/// the `scope` group, or `None`.
pub fn top_level_group(
    ctx: &BoardCtx<'_>,
    item: ItemRef<'_>,
    scope: Option<ItemId>,
    is_footprint_editor: bool,
) -> Option<ItemId> {
    nested_group(ctx, item, scope, is_footprint_editor)
}

/// `PCB_GROUP::WithinScope( aItem, aScope, isFootprintEditor )`.
pub fn within_scope(
    ctx: &BoardCtx<'_>,
    item: ItemRef<'_>,
    scope: Option<ItemId>,
    is_footprint_editor: bool,
) -> bool {
    let group = closest_group(item, is_footprint_editor);

    if group.is_some() && group == scope {
        return true;
    }

    let nested = nested_group(ctx, item, scope, is_footprint_editor);

    match nested
        .and_then(|n| ctx.item(n))
        .and_then(|n| n.parent_group())
    {
        Some(parent) => parent.id() == scope,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_GROUP::PCB_GROUP( BOARD_ITEM* aParent )` (pcbnew/pcb_group.cpp): BOARD_ITEM
    /// defaults (F_Cu), empty EDA_GROUP.
    #[test]
    fn group_constructor_defaults() {
        let g = PcbGroup::default();
        assert_eq!(g.base.layer, PcbLayerId::FCu);
        assert!(g.base.uuid == Kiid::default());
        assert!(g.name.is_empty());
        assert!(g.members.is_empty());
        assert!(!g.has_design_block_link());
        assert_eq!(BoardItem::type_(&g), KicadT::PcbGroupT);
        assert_eq!(BoardItem::class_name(&g), "PCB_GROUP");

        let mut kiid_generator = KiidGenerator::default();
        let g = PcbGroup::new(&mut kiid_generator);
        assert!(g.base.uuid != Kiid::default());
    }

    /// The constant `PCB_GROUP` overrides through the trait, on a footprint child.
    #[test]
    fn group_in_footprint_constant_virtuals() {
        let mut fx = rotated_flipped_scaled();
        fx.board.check_invariants().unwrap();
        let mut g = PcbGroup::default();
        g.set_name("G1");
        let id = fx.add_child(g, |fp| &mut fp.groups);
        fx.board.check_invariants().unwrap();
        assert!(fx.child(ItemKind::Group).kind() == ItemKind::Group);

        let ctx = BoardCtx::bare(&fx.board);
        let g: &dyn BoardItem = &fx.board[id];
        assert_eq!(g.layer(&ctx), PcbLayerId::UndefinedLayer);
        assert!(g.is_layer_agnostic());
        assert!(!g.is_on_copper_layer(&ctx));
        assert!(!g.hit_test(&ctx, Vector2I::new(0, 0), 0));
        assert!(!g.hit_test_rect(&ctx, &Box2I::default(), false, 0));
        assert_eq!(fx.board[id].name(), "G1");
    }
}
