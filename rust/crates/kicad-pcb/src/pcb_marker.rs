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

//! Port of `pcbnew/pcb_marker.h` and `pcbnew/pcb_marker.cpp` (`PCB_MARKER : BOARD_ITEM,
//! MARKER_BASE`), `include/marker_base.h` / `common/marker_base.cpp` (`MARKER_BASE`) and the
//! data of `include/rc_item.h` / `common/rc_item.cpp` (`RC_ITEM`, the data part of
//! `DRC_ITEM`), `board-model-types.md` section 10. A marker is never written to the board
//! file.
//!
//! `DRC_ITEM::m_violatingRule` is a `DrcRuleInfo` copy (`drc_api.rs`); the test-provider back
//! pointer and `RC_ITEM::m_parent` (the marker owning the item) are dropped, as are
//! `MARKER_BASE::m_scalingFactor` and `m_shapeBoundingBox` (marker glyph drawing). Report
//! texts (`ShowReport`, JSON) belong to `kicad-drc`. Behaviour: package M23.

use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PCB_IU_SCALE, PcbLayerId, Severity};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::{
    Box2I, EdaAngle, FlipDirection, Shape, ShapeLineChain, ShapeNull, ShapePolySet, Vector2I,
};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::{self, DrcConstraintT, DrcRuleInfo};
use crate::item::{BoardItemBase, SharedShape};
use crate::pcb_shape::PcbShape;
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `PCB_MARKER` + `MARKER_BASE`.
#[derive(Clone, Debug)]
pub struct PcbMarker {
    pub base: BoardItemBase,
    pub pos: Vector2I,              // MARKER_BASE::m_Pos
    pub marker_type: MarkerT,       // m_markerType
    pub excluded: bool,             // m_excluded
    pub comment: String,            // m_comment
    pub rc_item: Arc<RcItem>,       // m_rcItem
    pub path_shapes: Vec<PcbShape>, // m_pathShapes (inline; shown on LAYER_DRC_SHAPES)
    pub path_start: Vector2I,       // m_pathStart
    pub path_end: Vector2I,         // m_pathEnd
    pub path_length: i32,           // m_pathLength
}

/// `MARKER_BASE::MARKER_T`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum MarkerT {
    #[default]
    Unspec = 0,
    Erc,
    Drc,
    DrawingSheet,
    Ratsnest,
    Parity,
    Simulation,
}

/// Data of `RC_ITEM` / `DRC_ITEM`. `kicad-drc` creates these; rule and test-provider back
/// pointers of `DRC_ITEM` are replaced by names.
#[derive(Clone, Debug, Default)]
pub struct RcItem {
    pub error_code: i32,       // m_errorCode (DRCE_*)
    pub error_message: String, // m_errorMessage
    pub error_title: String,   // m_errorTitle
    pub settings_key: String,  // m_settingsKey
    pub ids: Vec<Kiid>,        // m_ids (main item, aux item, ...)
    /// DRC_ITEM::m_violatingRule: name, severity and implicit flag of the rule
    /// (PCB_MARKER::GetSeverity uses the rule's severity before the board's per-code one).
    pub violating_rule: Option<DrcRuleInfo>,
}

// `RcItem::default()` is `RC_ITEM()`: error code 0, empty texts, no ids, no rule.

impl RcItem {
    /// `GetErrorCode()`.
    pub fn error_code(&self) -> i32 {
        self.error_code
    }

    /// `SetErrorCode()`.
    pub fn set_error_code(&mut self, code: i32) {
        self.error_code = code;
    }

    /// `SetErrorMessage()`.
    pub fn set_error_message(&mut self, message: &str) {
        self.error_message = message.to_string();
    }

    /// `GetErrorText( aTranslate )` (untranslated: `m_errorTitle`).
    pub fn error_text(&self) -> &str {
        &self.error_title
    }

    /// `GetErrorMessage( aTranslate )`: `m_errorMessage`, or the error text when it is empty.
    pub fn error_message(&self) -> String {
        if self.error_message.is_empty() {
            self.error_text().to_string()
        } else {
            self.error_message.clone()
        }
    }

    /// `SetErrorDetail( aMsg )`: `GetErrorText() + " " + aMsg`.
    pub fn set_error_detail(&mut self, msg: &str) {
        let message = format!("{} {}", self.error_text(), msg);
        self.set_error_message(&message);
    }

    /// `GetSettingsKey()`.
    pub fn settings_key(&self) -> &str {
        &self.settings_key
    }

    /// `SetItems( const std::vector<KIID>& )`.
    pub fn set_item_ids(&mut self, ids: Vec<Kiid>) {
        self.ids = ids;
    }

    /// `SetItems( const KIID& aItem, bItem, cItem, dItem )`: always four entries, nil for the
    /// missing ones.
    pub fn set_items(&mut self, a: Kiid, b: Kiid, c: Kiid, d: Kiid) {
        self.ids.clear();
        self.ids.push(a);
        self.ids.push(b);
        self.ids.push(c);
        self.ids.push(d);
    }

    /// `GetIDs()`.
    pub fn ids(&self) -> &[Kiid] {
        &self.ids
    }

    /// `GetMainItemID()`: `m_ids[0]` or `niluuid`.
    pub fn main_item_id(&self) -> Kiid {
        self.ids.first().copied().unwrap_or_default()
    }

    /// `GetAuxItemID()`: `m_ids[1]` or `niluuid`.
    pub fn aux_item_id(&self) -> Kiid {
        self.ids.get(1).copied().unwrap_or_default()
    }

    /// `GetAuxItem2ID()`: `m_ids[2]` or `niluuid`.
    pub fn aux_item2_id(&self) -> Kiid {
        self.ids.get(2).copied().unwrap_or_default()
    }

    /// `GetAuxItem3ID()`: `m_ids[3]` or `niluuid`.
    pub fn aux_item3_id(&self) -> Kiid {
        self.ids.get(3).copied().unwrap_or_default()
    }

    /// `DRC_ITEM::GetViolatingRule()`.
    pub fn violating_rule(&self) -> Option<&DrcRuleInfo> {
        self.violating_rule.as_ref()
    }
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbMarker {
    /// C++ has no default constructor: the `BOARD_ITEM( nullptr, PCB_MARKER_T, F_Cu )` and
    /// `MARKER_BASE( SCALING_FACTOR, aItem )` parts (type `MARKER_UNSPEC`, not excluded, no
    /// comment), an empty `RC_ITEM()`, position (0, 0), path length 0; nil UUID. Markers are
    /// built with `new`, which runs the constructor body.
    fn default() -> Self {
        PcbMarker {
            base: BoardItemBase::with_layer(PcbLayerId::FCu),
            pos: Vector2I::default(),
            marker_type: MarkerT::Unspec,
            excluded: false,
            comment: String::new(),
            rc_item: Arc::new(RcItem::default()),
            path_shapes: Vec::new(),
            path_start: Vector2I::default(),
            path_end: Vector2I::default(),
            path_length: 0,
        }
    }
}

impl PcbMarker {
    /// `new PCB_MARKER( aItem, aPosition, aLayer = F_Cu )` (`pcb_marker.cpp`): the marker type
    /// follows from the item's error code (`LAYER_DRAWINGSHEET`: drawing-sheet marker, the
    /// layer stays `F_Cu`; unconnected items: ratsnest; the footprint / net / schematic parity
    /// codes: parity; anything else: DRC), the layer is `ToLAYER_ID( aLayer )`; fresh UUID.
    pub fn new(
        kiid_generator: &mut KiidGenerator,
        item: Arc<RcItem>,
        position: Vector2I,
        layer: i32,
    ) -> Self {
        let mut marker = PcbMarker {
            rc_item: item,
            ..PcbMarker::default()
        };
        marker.base.uuid = Kiid::new(kiid_generator);

        if layer == kicad_core::layer_ids::LAYER_DRAWINGSHEET {
            marker.marker_type = MarkerT::DrawingSheet;
        } else {
            marker.marker_type = match marker.rc_item.error_code {
                drc_api::DRCE_UNCONNECTED_ITEMS => MarkerT::Ratsnest,
                drc_api::DRCE_MISSING_FOOTPRINT
                | drc_api::DRCE_DUPLICATE_FOOTPRINT
                | drc_api::DRCE_EXTRA_FOOTPRINT
                | drc_api::DRCE_NET_CONFLICT
                | drc_api::DRCE_SCHEMATIC_PARITY
                | drc_api::DRCE_SCHEMATIC_FIELDS_PARITY
                | drc_api::DRCE_FOOTPRINT_FILTERS => MarkerT::Parity,
                _ => MarkerT::Drc,
            };
            marker.base.layer = kicad_core::layer_ids::to_layer_id(layer);
        }

        marker.pos = position;
        marker
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbMarker {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbMarker {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbMarkerT
    }
    fn class_name(&self) -> &'static str {
        "PCB_MARKER"
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbMarker::position(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbMarker::center(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbMarker::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbMarker::effective_shape(self, ctx, layer, flash, usage)
    }
    fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        PcbMarker::transform_shape_to_polygon(
            self,
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        )
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbMarker::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbMarker::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbMarker::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbMarker {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbMarker::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbMarker::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbMarker::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbMarker::flip(self, env, centre, dir)
    }
}

// ---- inherent methods: MARKER_BASE (package M23) ------------------------------------------

impl PcbMarker {
    /// `MARKER_BASE::GetPos()`.
    pub fn pos(&self) -> Vector2I {
        self.pos
    }

    /// `MARKER_BASE::GetMarkerType()`.
    pub fn marker_type(&self) -> MarkerT {
        self.marker_type
    }

    /// `MARKER_BASE::SetMarkerType()`.
    pub fn set_marker_type(&mut self, marker_type: MarkerT) {
        self.marker_type = marker_type;
    }

    /// `MARKER_BASE::IsExcluded()`.
    pub fn is_excluded(&self) -> bool {
        self.excluded
    }

    /// `MARKER_BASE::SetExcluded( aExcluded, aComment = wxEmptyString )`.
    pub fn set_excluded(&mut self, excluded: bool, comment: &str) {
        self.excluded = excluded;
        self.comment = comment.to_string();
    }

    /// `MARKER_BASE::GetComment()`.
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /// `MARKER_BASE::GetRCItem()`.
    pub fn rc_item(&self) -> &Arc<RcItem> {
        &self.rc_item
    }

    /// `PCB_MARKER::GetSeverity()` (virtual in `MARKER_BASE`): exclusion, the generic
    /// warning / error codes, the violating rule's severity, else the board's per-code
    /// severity.
    pub fn severity(&self, ctx: &BoardCtx<'_>) -> Severity {
        if self.is_excluded() {
            return Severity::Exclusion;
        }

        let item = &self.rc_item;

        if item.error_code() == drc_api::DRCE_GENERIC_WARNING {
            return Severity::Warning;
        } else if item.error_code() == drc_api::DRCE_GENERIC_ERROR {
            return Severity::Error;
        }

        if let Some(rule) = item.violating_rule() {
            if rule.severity != Severity::Undefined {
                return rule.severity;
            }
        }

        ctx.board.design_settings().get_severity(item.error_code())
    }

    /// `MARKER_BASE::IsTreatedAsExcluded()`: `m_excluded || GetSeverity() ==
    /// RPT_SEVERITY_EXCLUSION`.
    pub fn is_treated_as_excluded(&self, ctx: &BoardCtx<'_>) -> bool {
        self.excluded || self.severity(ctx) == Severity::Exclusion
    }

    /// `MARKER_BASE::MarkerScale()`: `m_scalingFactor`, which is the constructor's
    /// `SCALING_FACTOR` until a GUI calls `SetZoom` (not ported).
    pub fn marker_scale(&self) -> i32 {
        PCB_IU_SCALE.mm_to_iu(PCB_MARKER_SCALING_FACTOR_MM)
    }

    /// `MARKER_BASE::GetBoundingBoxMarker()`: the glyph box (`m_shapeBoundingBox`, the extent
    /// of `MarkerShapeCorners`) scaled and placed at `m_Pos`.
    pub fn bounding_box_marker(&self) -> Box2I {
        let bbox = marker_shape_bounding_box();
        let scale = self.marker_scale();

        let mut pos = self.pos;
        pos += bbox.position() * scale;

        Box2I::new(pos, bbox.size() * scale)
    }

    /// `MARKER_BASE::HitTestMarker( aHitPosition, aAccuracy )`.
    pub fn hit_test_marker(&self, hit_position: Vector2I, accuracy: i32) -> bool {
        let bbox = self.bounding_box_marker().inflated(accuracy);

        // Fast hit test using boundary box. A finer test will be made if requested
        let mut hit = bbox.contains(hit_position);

        if hit {
            // Fine test
            let polygon = self.shape_to_polygon(-1);
            let rel_pos = hit_position - self.pos;
            hit = polygon.point_inside(rel_pos, accuracy, false);
        }

        hit
    }

    /// `MARKER_BASE::HitTestMarker( aRect, aContained, aAccuracy )`.
    pub fn hit_test_marker_rect(&self, rect: &Box2I, contained: bool, accuracy: i32) -> bool {
        let bbox = self.bounding_box_marker().inflated(accuracy);

        if contained {
            return rect.contains_box(&bbox);
        }

        rect.intersects(&bbox)
    }

    /// `MARKER_BASE::ShapeToPolygon( aPolygon, aScale )`: the marker glyph outline.
    /// A negative `scale` (the C++ default -1) uses `MarkerScale()`.
    pub fn shape_to_polygon(&self, scale: i32) -> ShapeLineChain {
        let scale = if scale < 0 {
            self.marker_scale()
        } else {
            scale
        };
        let mut polygon = ShapeLineChain::new();

        for corner in MARKER_SHAPE_CORNERS {
            polygon.append_point(corner * scale, false);
        }

        // Be sure aPolygon is seen as a closed polyline:
        polygon.set_closed(true);
        polygon
    }

    /// `PCB_MARKER::SetPath( aShapes, aStart, aEnd )`.
    pub fn set_path(&mut self, shapes: Vec<PcbShape>, start: Vector2I, end: Vector2I) {
        self.path_shapes = shapes;
        self.path_start = start;
        self.path_end = end;
    }

    /// `PCB_MARKER::GetPath()`.
    pub fn path(&self) -> &[PcbShape] {
        &self.path_shapes
    }
}

// ---- inherent methods: PCB_MARKER virtuals (package M23) ----------------------------------

impl PcbMarker {
    /// `PCB_MARKER::GetPosition()`: `m_Pos`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.pos
    }

    /// `PCB_MARKER::SetPosition()`: `m_Pos = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.pos = pos;
    }

    /// `PCB_MARKER::GetCenter()`: `GetPosition()`.
    pub fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        self.position(ctx)
    }

    /// `PCB_MARKER::Move()`: `m_Pos += aMoveVector`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.pos += delta;
    }

    /// `PCB_MARKER::Rotate()`.
    pub fn rotate(&mut self, _env: &MutEnv, _centre: Vector2I, _angle: EdaAngle) {
        // Marker geometry isn't user-editable
    }

    /// `PCB_MARKER::Flip()`.
    pub fn flip(&mut self, _env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {
        // Marker geometry isn't user-editable
    }

    /// `PCB_MARKER::GetBoundingBox()`: the glyph box merged with the path shapes' boxes.
    pub fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        let mut bbox = self.bounding_box_marker();

        for shape in &self.path_shapes {
            bbox.merge(&BoardItem::bounding_box(shape, ctx));
        }

        bbox
    }

    /// `PCB_MARKER::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let _ = (ctx, layer, flash);
        // Markers do not participate in the board geometry space, and therefore have no
        // effective shape.
        Arc::new(ShapeNull)
    }

    /// `PCB_MARKER::TransformShapeToPolygon()`.
    pub fn transform_shape_to_polygon(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
        ignore_line_width: bool,
    ) {
        // Markers do not participate in the board geometry space, and therefore have no shape.
        let _ = (
            ctx,
            buffer,
            layer,
            clearance,
            error,
            error_loc,
            ignore_line_width,
        );
    }

    /// `PCB_MARKER::HitTest( aPosition, aAccuracy )`: false for a ratsnest marker, else
    /// `HitTestMarker`.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        if self.marker_type == MarkerT::Ratsnest {
            false
        } else {
            self.hit_test_marker(pos, accuracy)
        }
    }

    /// `PCB_MARKER::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let _ = ctx;

        if self.marker_type == MarkerT::Ratsnest {
            return false;
        }

        self.hit_test_marker_rect(rect, contained, accuracy)
    }

    /// `PCB_MARKER::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        let _ = (ctx, units);
        let text = if full {
            self.rc_item.error_message()
        } else {
            self.rc_item.error_text().to_string()
        };

        format!("Marker ({text})")
    }
}

/// `SCALING_FACTOR` of `pcbnew/pcb_marker.cpp`: `pcbIUScale.mmToIU( 0.1625 )`.
const PCB_MARKER_SCALING_FACTOR_MM: f64 = 0.1625;

/// `MarkerShapeCorners` (`common/marker_base.cpp`): the corners of the marker glyph in
/// arbitrary units, scaled by `MarkerScale()`.
const MARKER_SHAPE_CORNERS: [Vector2I; 9] = [
    Vector2I::new(0, 0),
    Vector2I::new(8, 1),
    Vector2I::new(4, 3),
    Vector2I::new(13, 8),
    Vector2I::new(9, 9),
    Vector2I::new(8, 13),
    Vector2I::new(3, 4),
    Vector2I::new(1, 8),
    Vector2I::new(0, 0),
];

/// `MARKER_BASE::m_shapeBoundingBox` as the constructor computes it: origin = the minimum,
/// end = the maximum of the corners.
fn marker_shape_bounding_box() -> Box2I {
    let mut start = MARKER_SHAPE_CORNERS[0];
    let mut end = start;

    for point in &MARKER_SHAPE_CORNERS[1..] {
        start.x = start.x.min(point.x);
        start.y = start.y.min(point.y);
        end.x = end.x.max(point.x);
        end.y = end.y.max(point.y);
    }

    let mut bbox = Box2I::default();
    bbox.set_origin(start);
    bbox.set_end(end);
    bbox
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker_with(code: i32, rule: Option<Severity>) -> PcbMarker {
        let mut item = RcItem {
            error_code: code,
            error_title: "Title".to_string(),
            ..RcItem::default()
        };
        item.violating_rule = rule.map(|severity| DrcRuleInfo {
            severity,
            ..DrcRuleInfo::default()
        });
        let mut g = KiidGenerator::default();
        PcbMarker::new(&mut g, Arc::new(item), Vector2I::new(1000, 2000), 0)
    }

    /// `PCB_MARKER::GetSeverity` precedence: exclusion, the generic codes, the violating rule's
    /// own severity, then the board's per-code severity; `IsTreatedAsExcluded`.
    #[test]
    fn marker_severity_precedence() {
        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let code = drc_api::DRCE_CLEARANCE;
        let board_severity = board.design_settings().get_severity(code);

        assert_eq!(marker_with(code, None).severity(&ctx), board_severity);
        assert_eq!(
            marker_with(code, Some(Severity::Undefined)).severity(&ctx),
            board_severity
        );
        assert_eq!(
            marker_with(code, Some(Severity::Warning)).severity(&ctx),
            Severity::Warning
        );
        assert_eq!(
            marker_with(drc_api::DRCE_GENERIC_WARNING, Some(Severity::Error)).severity(&ctx),
            Severity::Warning
        );
        assert_eq!(
            marker_with(drc_api::DRCE_GENERIC_ERROR, None).severity(&ctx),
            Severity::Error
        );

        let mut m = marker_with(code, Some(Severity::Warning));
        assert!(!m.is_treated_as_excluded(&ctx));
        m.set_excluded(true, "ok");
        assert_eq!(m.severity(&ctx), Severity::Exclusion);
        assert!(m.is_treated_as_excluded(&ctx));
        assert!(marker_with(code, Some(Severity::Exclusion)).is_treated_as_excluded(&ctx));
    }

    /// The glyph: `MarkerShapeCorners` scaled by `mmToIU( 0.1625 )` = 162500, its box at
    /// `m_Pos`, the fine hit test inside the polygon, and the ratsnest exception.
    #[test]
    fn marker_glyph_box_and_hit_test() {
        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let m = marker_with(drc_api::DRCE_CLEARANCE, None);
        assert_eq!(m.marker_scale(), 162_500);
        assert_eq!(
            m.bounding_box(&ctx),
            Box2I::new(
                Vector2I::new(1000, 2000),
                Vector2I::new(13 * 162_500, 13 * 162_500)
            )
        );
        let chain = m.shape_to_polygon(-1);
        assert_eq!(chain.point_count(), 8); // SetClosed merges the repeated first point
        assert_eq!(chain.c_point(3), Vector2I::new(13 * 162_500, 8 * 162_500));

        // inside the arrow near its tip, outside in the empty corner of the box
        assert!(m.hit_test(
            &ctx,
            Vector2I::new(1000 + 4 * 162_500, 2000 + 4 * 162_500),
            0
        ));
        assert!(!m.hit_test(&ctx, Vector2I::new(1000 + 12 * 162_500, 2000 + 162_500), 0));

        let mut r = marker_with(drc_api::DRCE_UNCONNECTED_ITEMS, None);
        assert_eq!(r.marker_type(), MarkerT::Ratsnest);
        assert!(!r.hit_test(
            &ctx,
            Vector2I::new(1000 + 4 * 162_500, 2000 + 4 * 162_500),
            0
        ));
        let all = Box2I::new(
            Vector2I::new(0, 0),
            Vector2I::new(i32::MAX / 2, i32::MAX / 2),
        );
        assert!(!r.hit_test_rect(&ctx, &all, false, 0));
        assert!(m.hit_test_rect(&ctx, &all, true, 0));

        let units = UnitsProvider::new(&kicad_core::PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(r.item_description(&ctx, &units, false), "Marker (Title)");
        Arc::make_mut(&mut r.rc_item).set_error_detail("(detail)");
        assert_eq!(r.rc_item.error_message(), "Title (detail)");
        assert_eq!(
            r.item_description(&ctx, &units, true),
            "Marker (Title (detail))"
        );
    }

    /// `RC_ITEM()` (include/rc_item.h), `MARKER_BASE( .. )` (common/marker_base.cpp) and
    /// `PCB_MARKER( aItem, aPosition, aLayer )` (pcbnew/pcb_marker.cpp).
    #[test]
    fn marker_constructor_defaults() {
        let item = RcItem::default();
        assert_eq!(item.error_code(), 0);
        assert!(item.ids().is_empty());
        assert!(item.main_item_id() == Kiid::default());
        assert!(item.violating_rule().is_none());

        let m = PcbMarker::default();
        assert_eq!(m.marker_type(), MarkerT::Unspec);
        assert!(!m.is_excluded());
        assert!(m.comment().is_empty());
        assert_eq!(m.path_length, 0);
        assert!(m.path().is_empty());
        assert_eq!(m.base.layer, PcbLayerId::FCu);
        assert!(m.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&m), "PCB_MARKER");

        let mut kiid_generator = KiidGenerator::default();
        let mk = |kg: &mut KiidGenerator, code: i32, layer: i32| {
            let item = RcItem {
                error_code: code,
                ..RcItem::default()
            };
            PcbMarker::new(kg, Arc::new(item), Vector2I::new(1, 2), layer)
        };
        let m = mk(&mut kiid_generator, drc_api::DRCE_UNCONNECTED_ITEMS, 0);
        assert_eq!(m.marker_type(), MarkerT::Ratsnest);
        assert_eq!(m.pos(), Vector2I::new(1, 2));
        assert!(m.base.uuid != Kiid::default());
        let m = mk(&mut kiid_generator, drc_api::DRCE_SCHEMATIC_PARITY, 0);
        assert_eq!(m.marker_type(), MarkerT::Parity);
        let m = mk(
            &mut kiid_generator,
            drc_api::DRCE_UNCONNECTED_ITEMS + 1000,
            PcbLayerId::BCu.0,
        );
        assert_eq!(m.marker_type(), MarkerT::Drc);
        assert_eq!(m.base.layer, PcbLayerId::BCu);
        let m = mk(
            &mut kiid_generator,
            drc_api::DRCE_UNCONNECTED_ITEMS,
            kicad_core::layer_ids::LAYER_DRAWINGSHEET,
        );
        assert_eq!(m.marker_type(), MarkerT::DrawingSheet);
        assert_eq!(m.base.layer, PcbLayerId::FCu);
    }

    /// RC_ITEM::SetItems( KIID, .. ) always stores four ids; the marker's trivial mutators.
    #[test]
    fn rc_item_ids_and_marker_move() {
        let mut kiid_generator = KiidGenerator::default();
        let a = Kiid::new(&mut kiid_generator);
        let mut item = RcItem::default();
        item.set_items(a, Kiid::default(), Kiid::default(), Kiid::default());
        assert_eq!(item.ids().len(), 4);
        assert!(item.main_item_id() == a);
        assert!(item.aux_item_id() == Kiid::default());

        let mut m = PcbMarker::default();
        let env = MutEnv::detached();
        BoardItemMut::set_position(&mut m, &env, Vector2I::new(5, 6));
        BoardItemMut::move_by(&mut m, &env, Vector2I::new(-1, -1));
        assert_eq!(m.pos(), Vector2I::new(4, 5));
        m.set_excluded(true, "ok");
        assert!(m.is_excluded());
        assert_eq!(m.comment(), "ok");
    }
}
