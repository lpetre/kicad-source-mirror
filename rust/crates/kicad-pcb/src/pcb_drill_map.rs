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

//! Port of `pcbnew/pcb_drill_map.h` and `pcbnew/pcb_drill_map.cpp` (`PCB_DRILL_MAP`: drill
//! symbols drawn on its layer; drawing them is tier 2) and of `DRILL_SPAN`
//! (`pcbnew/drill/drill_span.h`), `board-model-types.md` section 10. The rendering cache
//! (`m_outlineCache*`) is dropped. Behaviour: package M23.

use kicad_core::layer_ids::is_copper_layer_lower_than;
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{Box2I, EdaAngle, FlipDirection, Vector2I};

use crate::board_item;
use crate::ctx::{BoardCtx, MutEnv};
use crate::item::BoardItemBase;
use crate::traits::{BoardItem, BoardItemMut, Invalidate};

/// `DRILL_SYMBOL_PROFILE()::m_symbolSize` (`pcbnew/drill/drill_symbol_profile.cpp`), in mm.
const DEFAULT_DRILL_SYMBOL_SIZE_MM: f64 = 1.5;

/// `PCB_DRILL_MAP`.
#[derive(Clone, Debug)]
pub struct PcbDrillMap {
    /// `base.layer` = the map layer.
    pub base: BoardItemBase,
    pub offset: Vector2I,    // m_offset
    pub symbol_size: i32,    // m_symbolSize
    pub span: DrillSpan,     // m_span
    pub all_spans: bool,     // m_allSpans
    pub outline_slots: bool, // m_outlineSlots
    pub guide_cross: bool,   // m_guideCross
}

/// `DRILL_SPAN` (pcbnew/drill/drill_span.h).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillSpan {
    pub start_layer: PcbLayerId,  // m_StartLayer
    pub end_layer: PcbLayerId,    // m_EndLayer
    pub is_backdrill: bool,       // m_IsBackdrill
    pub is_non_plated_file: bool, // m_IsNonPlatedFile
}

impl Default for DrillSpan {
    /// `DRILL_SPAN()`: `F_Cu` to `B_Cu`, not a backdrill, plated.
    fn default() -> Self {
        DrillSpan {
            start_layer: PcbLayerId::FCu,
            end_layer: PcbLayerId::BCu,
            is_backdrill: false,
            is_non_plated_file: false,
        }
    }
}

impl DrillSpan {
    /// `DRILL_SPAN( aStartLayer, aEndLayer, aIsBackdrill, aIsNonPlated )`.
    pub fn new(
        start_layer: PcbLayerId,
        end_layer: PcbLayerId,
        is_backdrill: bool,
        is_non_plated_file: bool,
    ) -> Self {
        DrillSpan {
            start_layer,
            end_layer,
            is_backdrill,
            is_non_plated_file,
        }
    }

    /// `DrillStartLayer()`.
    pub fn drill_start_layer(&self) -> PcbLayerId {
        self.start_layer
    }

    /// `DrillEndLayer()`.
    pub fn drill_end_layer(&self) -> PcbLayerId {
        self.end_layer
    }

    /// `TopLayer()`: the upper of the two layers (`IsCopperLayerLowerThan`).
    pub fn top_layer(&self) -> PcbLayerId {
        // B_Cu (id=2) is numerically less than inner layers (id>=4), but is physically
        // at the bottom of the stack. Use IsCopperLayerLowerThan for correct ordering.
        if is_copper_layer_lower_than(self.start_layer, self.end_layer) {
            self.end_layer
        } else {
            self.start_layer
        }
    }

    /// `BottomLayer()`.
    pub fn bottom_layer(&self) -> PcbLayerId {
        if is_copper_layer_lower_than(self.start_layer, self.end_layer) {
            self.start_layer
        } else {
            self.end_layer
        }
    }

    /// `Pair()`: `( TopLayer(), BottomLayer() )`.
    pub fn pair(&self) -> (PcbLayerId, PcbLayerId) {
        (self.top_layer(), self.bottom_layer())
    }

    /// `operator<( const DRILL_SPAN& )`: top layer, bottom layer, backdrill first, non-plated
    /// first, then the stored start and end layers.
    pub fn less(&self, other: &DrillSpan) -> bool {
        if self.top_layer() != other.top_layer() {
            return self.top_layer() < other.top_layer();
        }

        if self.bottom_layer() != other.bottom_layer() {
            return self.bottom_layer() < other.bottom_layer();
        }

        if self.is_backdrill != other.is_backdrill {
            return self.is_backdrill && !other.is_backdrill;
        }

        if self.is_non_plated_file != other.is_non_plated_file {
            return self.is_non_plated_file && !other.is_non_plated_file;
        }

        if self.start_layer != other.start_layer {
            return self.start_layer < other.start_layer;
        }

        self.end_layer < other.end_layer
    }

    /// `operator<` (drill file / map order) as an `Ordering`: `Less` when `self < other`,
    /// `Greater` when `other < self`, else `Equal` (the order is total, see `less`).
    pub fn cmp(&self, other: &DrillSpan) -> std::cmp::Ordering {
        if self.less(other) {
            std::cmp::Ordering::Less
        } else if other.less(self) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbDrillMap {
    /// `PCB_DRILL_MAP( aParent )` (`pcb_drill_map.cpp`) without a board: symbol size from a
    /// default `DRILL_SYMBOL_PROFILE` (1.5 mm), offset (0, 0), default span, all spans,
    /// slots outlined, no guide cross, layer `F_Cu`; nil UUID. (With a board the C++
    /// constructor takes the board profile's symbol size; the parser always sets it from the
    /// file.)
    fn default() -> Self {
        PcbDrillMap {
            base: BoardItemBase::default(),
            offset: Vector2I::default(),
            symbol_size: PCB_IU_SCALE.mm_to_iu(DEFAULT_DRILL_SYMBOL_SIZE_MM),
            span: DrillSpan::default(),
            all_spans: true,
            outline_slots: true,
            guide_cross: false,
        }
    }
}

impl PcbDrillMap {
    /// `new PCB_DRILL_MAP( aParent )` (no-board branch): the constructor values with a fresh
    /// UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut map = PcbDrillMap::default();
        map.base.uuid = Kiid::new(kiid_generator);
        map
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbDrillMap {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbDrillMap {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbDrillMapT
    }
    fn class_name(&self) -> &'static str {
        "PCB_DRILL_MAP"
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbDrillMap::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbDrillMap::bounding_box(self, ctx)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbDrillMap::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbDrillMap::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbDrillMap::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbDrillMap {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbDrillMap::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbDrillMap::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbDrillMap::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbDrillMap::flip(self, env, centre, dir)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbDrillMap {
    /// `GetOffset()`.
    pub fn offset(&self) -> Vector2I {
        self.offset
    }

    /// `SetOffset()`.
    pub fn set_offset(&mut self, offset: Vector2I) {
        self.offset = offset;
    }

    /// `GetSymbolSize()`.
    pub fn symbol_size(&self) -> i32 {
        self.symbol_size
    }

    /// `SetSymbolSize()`.
    pub fn set_symbol_size(&mut self, size: i32) {
        self.symbol_size = size.max(1);
    }

    /// `GetSymbolExtent()`: `3 * m_symbolSize + 2 * width`, the width being the board drill
    /// symbol profile's `GetSymbolWidth()` (0 without a board).
    pub fn symbol_extent(&self, ctx: &BoardCtx<'_>) -> i32 {
        let width = if board_item::has_board(self, ctx) {
            ctx.board.design_settings().drill_symbol_width()
        } else {
            0
        };

        // Numeric marks can reach three character sizes either side of their hole.
        3 * self.symbol_size + 2 * width
    }

    /// `GetOutlineSlots()`.
    pub fn outline_slots(&self) -> bool {
        self.outline_slots
    }

    /// `SetOutlineSlots()`.
    pub fn set_outline_slots(&mut self, on: bool) {
        self.outline_slots = on;
    }

    /// `GetGuideCross()`.
    pub fn guide_cross(&self) -> bool {
        self.guide_cross
    }

    /// `SetGuideCross()`.
    pub fn set_guide_cross(&mut self, on: bool) {
        self.guide_cross = on;
    }

    /// `GetSpan()`.
    pub fn span(&self) -> &DrillSpan {
        &self.span
    }

    /// `SetSpan()`.
    pub fn set_span(&mut self, span: DrillSpan) {
        self.span = span;
    }

    /// `GetAllSpans()`.
    pub fn all_spans(&self) -> bool {
        self.all_spans
    }

    /// `SetAllSpans()`.
    pub fn set_all_spans(&mut self, all: bool) {
        self.all_spans = all;
    }

    /// `PCB_DRILL_MAP::GetPosition()`: `m_offset`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.offset
    }

    /// `PCB_DRILL_MAP::SetPosition()`: `m_offset = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.offset = pos;
    }

    /// `PCB_DRILL_MAP::Move()`: `m_offset += aMoveVector`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.offset += delta;
    }

    /// `PCB_DRILL_MAP::Rotate()`.
    pub fn rotate(&mut self, _env: &MutEnv, _centre: Vector2I, _angle: EdaAngle) {
        // The marks sit at their own holes and the offset is a displacement from each one, so
        // there is no geometry here to turn
    }

    /// `PCB_DRILL_MAP::Flip()`.
    pub fn flip(&mut self, env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {
        // The offset is a displacement of every mark from its own hole, so there is nothing to
        // mirror. The side the map documents is still the side it belongs on
        // SetLayer( GetFlippedLayer( GetLayer() ) ): BOARD_ITEM::SetLayer.
        self.base.layer = env.layers.flip_layer(self.base.layer);
    }

    /// `PCB_DRILL_MAP::GetBoundingBox()`: the hole extent of the board's
    /// `DRILL_SYMBOL_CACHE` inflated by `GetSymbolExtent()`, merged with the board edges box,
    /// moved by the offset. Not ported yet: the drill symbol cache is a drill-writer cache
    /// (class 3, `board-model.md` 9) of the tier-2 drill symbol package.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        crate::port_pending!("PCB_DRILL_MAP::GetBoundingBox (DRILL_SYMBOL_CACHE)", ctx)
    }

    /// `PCB_DRILL_MAP::HitTest( aPosition, aAccuracy )`: false without a board, else the marks
    /// of the board's `DRILL_SYMBOL_CACHE` and the board outline. Not ported yet (see
    /// `bounding_box`).
    pub fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        if !board_item::has_board(self, ctx) {
            return false;
        }

        crate::port_pending!("PCB_DRILL_MAP::HitTest (DRILL_SYMBOL_CACHE)", pos, accuracy)
    }

    /// `PCB_DRILL_MAP::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut r = *rect;
        r.inflate(accuracy);

        if contained {
            return r.contains_box(&self.bounding_box(ctx));
        }

        r.intersects(&self.bounding_box(ctx))
    }

    /// `PCB_DRILL_MAP::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = units;
        format!("Drill Map on {}", board_item::layer_name(self, ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DRILL_SPAN::TopLayer` / `BottomLayer` / `operator<` (drill/drill_span.h).
    #[test]
    fn drill_span_order() {
        let span = |a, b, back, np| DrillSpan::new(a, b, back, np);
        let through = span(PcbLayerId::FCu, PcbLayerId::BCu, false, false);
        assert_eq!(through.top_layer(), PcbLayerId::FCu);
        assert_eq!(through.bottom_layer(), PcbLayerId::BCu);
        let reversed = span(PcbLayerId::BCu, PcbLayerId::FCu, false, false);
        assert_eq!(reversed.pair(), (PcbLayerId::FCu, PcbLayerId::BCu));
        let buried = span(PcbLayerId::In1Cu, PcbLayerId::In2Cu, false, false);
        assert_eq!(buried.top_layer(), PcbLayerId::In1Cu);
        let back = span(PcbLayerId::BCu, PcbLayerId::In2Cu, true, false);
        assert_eq!(back.top_layer(), PcbLayerId::In2Cu);
        assert_eq!(back.bottom_layer(), PcbLayerId::BCu);

        use std::cmp::Ordering::*;
        // same pair: backdrill first, then non-plated, then stored start layer
        assert_eq!(
            span(PcbLayerId::FCu, PcbLayerId::BCu, true, false).cmp(&through),
            Less
        );
        assert_eq!(
            span(PcbLayerId::FCu, PcbLayerId::BCu, false, true).cmp(&through),
            Less
        );
        assert_eq!(through.cmp(&reversed), Less); // F_Cu (0) < B_Cu (2)
        assert_eq!(through.cmp(&through.clone()), Equal);
        assert_eq!(buried.cmp(&through), Greater); // top In1_Cu (4) > F_Cu (0)
    }

    /// `SetSymbolSize` clamps at 1; `Flip` flips the layer only; the description.
    #[test]
    fn drill_map_setters_and_flip() {
        let mut m = PcbDrillMap::default();
        m.set_symbol_size(-5);
        assert_eq!(m.symbol_size(), 1);
        m.base.layer = PcbLayerId::FFab;
        m.set_offset(Vector2I::new(3, 4));
        let env = MutEnv::detached();
        m.flip(&env, Vector2I::new(100, 100), FlipDirection::LeftRight);
        m.rotate(&env, Vector2I::new(100, 100), EdaAngle::default());
        assert_eq!(m.base.layer, PcbLayerId::BFab);
        assert_eq!(m.offset(), Vector2I::new(3, 4));

        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        assert!(!m.hit_test(&ctx, Vector2I::new(0, 0), 0)); // no board: false
        let units =
            kicad_core::units_provider::UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(
            m.item_description(&ctx, &units, false),
            "Drill Map on B.Fab"
        );
    }

    /// `PCB_DRILL_MAP::PCB_DRILL_MAP( aParent )` (pcbnew/pcb_drill_map.cpp, no-board branch),
    /// `DRILL_SPAN()` (drill_span.h), `DRILL_SYMBOL_PROFILE()` (drill_symbol_profile.cpp).
    #[test]
    fn drill_map_constructor_defaults() {
        let m = PcbDrillMap::default();
        assert_eq!(m.offset(), Vector2I::new(0, 0));
        assert_eq!(m.symbol_size(), 1_500_000);
        assert_eq!(*m.span(), DrillSpan::default());
        assert_eq!(m.span.start_layer, PcbLayerId::FCu);
        assert_eq!(m.span.end_layer, PcbLayerId::BCu);
        assert!(!m.span.is_backdrill && !m.span.is_non_plated_file);
        assert!(m.all_spans() && m.outline_slots() && !m.guide_cross());
        assert_eq!(m.base.layer, PcbLayerId::FCu);
        assert!(m.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&m), "PCB_DRILL_MAP");

        let mut m = m;
        let env = MutEnv::detached();
        BoardItemMut::set_position(&mut m, &env, Vector2I::new(2, 3));
        BoardItemMut::move_by(&mut m, &env, Vector2I::new(1, 1));
        assert_eq!(m.offset, Vector2I::new(3, 4));
    }
}
