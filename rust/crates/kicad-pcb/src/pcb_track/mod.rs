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

//! Port of `pcbnew/pcb_track.h` and `pcbnew/pcb_track.cpp`: `PCB_TRACK`, `PCB_ARC`, `PCB_VIA`
//! and the `Track` enum that stands for a `PCB_TRACK*` (`board-model-types.md` section 2).
//!
//! This family is the pattern every item type follows (`board-model.md` 6.2, 16):
//!
//! * the data structs with `Default` = the C++ constructor values (nil UUID) and `new()`
//!   drawing a UUID;
//! * one `impl Trait for T` block per trait in this file, containing only one-line shims that
//!   forward to an inherent method of the same name and signature, for exactly the virtuals the
//!   C++ class overrides;
//! * a C++ class derived from another item class embeds the base struct (`PcbArc::track`,
//!   `PcbVia::track`) and has a shim for every virtual that it **or its base** overrides
//!   (a Rust `PcbArc` is not a `PcbTrack`, so nothing is inherited), each forwarding to its
//!   **own** inherent method. For an inherited virtual that method is the base body as it
//!   runs for the derived class (`PCB_TRACK::GetBoundingBox (PCB_ARC)`: the `PCB_TRACK` bodies
//!   test `Type()`); where a base body is identical for the derived class, the derived method
//!   calls the base's (`self.track.x( .. )`), and where the base body calls a virtual that the
//!   derived class overrides, the owning package writes the base body once as a generic
//!   function over the host and both call it;
//! * virtuals that the C++ class itself introduces (`PCB_TRACK::GetWidth`, `SetWidth`,
//!   `GetLength`, `GetWidthConstraint`) are not in the traits: they are inherent methods of
//!   each struct plus a dispatching method on the enum (`Track::width`), see "virtuals
//!   introduced by PCB_TRACK" below;
//! * the inherent methods in the module of the owning package: `PcbTrack` here (M16), `PcbArc`
//!   in `arc.rs` (M16), `PcbVia` in `via.rs` and its shapes in `shapes.rs` (M17); bodies are
//!   `port_pending!` stubs unless trivial.

pub mod arc;
pub mod shapes;
pub mod via;

use kicad_core::layer_ids::{is_copper_layer, is_solder_mask_layer};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::{
    EdaDataType, KicadT, Kiid, KiidGenerator, Lset, MAX_CU_LAYERS, MinOptMax, PCB_IU_SCALE,
    PcbLayerId, StrokeParams,
};
use kicad_geom::convert_basic_shapes_to_polygon::transform_oval_to_polygon;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::math::box2::box2i_safe_from_int;
use kicad_geom::{
    Box2I, EdaAngle, FlipDirection, Seg, ShapePolySet, ShapeSegment, Vector2I, Vector2L,
    rotate_point_around, test_segment_hit,
};
use std::cmp::Ordering;
use std::sync::Arc;

use crate::board_design_settings::BoardDesignSettings;
use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::{DrcConstraint, DrcConstraintT};
use crate::ids::{ItemId, NetCode, TrackId};
use crate::item::{BoardItemBase, ConnectedItemBase, RecurseMode, SharedShape};
use crate::item_ref::{AnyItem, ItemRef};
use crate::netclass::Netclass;
use crate::padstack::Padstack;
use crate::traits::{BoardConnectedItem, BoardItem, BoardItemMut, Flashing, Invalidate};
use crate::{board_connected_item, board_item};

/// Element type of `Board::tracks` (C++ `TRACKS` holds `PCB_TRACK*`, `PCB_ARC*` and
/// `PCB_VIA*`). `Via` is boxed so that a segment stays small (a via carries a whole padstack).
#[derive(Clone, Debug)]
pub enum Track {
    /// `PCB_TRACE_T`.
    Trace(PcbTrack),
    /// `PCB_ARC_T`.
    Arc(PcbArc),
    /// `PCB_VIA_T`.
    Via(Box<PcbVia>),
}

/// `PCB_TRACK`: a straight track segment, and the `PCB_TRACK` base of arcs and vias.
#[derive(Clone, Debug)]
pub struct PcbTrack {
    pub base: ConnectedItemBase,
    pub start: Vector2I,                 // m_Start
    pub end: Vector2I,                   // m_End
    pub has_solder_mask: bool,           // m_hasSolderMask
    pub solder_mask_margin: Option<i32>, // m_solderMaskMargin
    pub width: i32,                      // m_width
}

/// `PCB_ARC : PCB_TRACK`.
#[derive(Clone, Debug, Default)]
pub struct PcbArc {
    pub track: PcbTrack,
    pub mid: Vector2I, // m_Mid
}

/// `PCB_VIA : PCB_TRACK`.
#[derive(Clone, Debug)]
pub struct PcbVia {
    /// The `PCB_TRACK` part; `start == end == position`.
    pub track: PcbTrack,
    pub via_type: Viatype,  // m_viaType
    pub padstack: Padstack, // m_padStack
    pub is_free: bool,      // m_isFree
    /// m_zoneLayerOverrides (`std::array<std::atomic<..>, MAX_CU_LAYERS>`): plain data, written
    /// by the zone filler in its `&mut Board` phases.
    pub zone_layer_overrides: [ZoneLayerOverride; MAX_CU_LAYERS as usize],
}

/// `VIATYPE`; the values are part of the file format history.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum Viatype {
    Through = 4,
    Buried = 3,
    Blind = 2,
    Microvia = 1,
    NotDefined = 0,
}

/// `ZONE_LAYER_OVERRIDE` (`ZLO_*`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum ZoneLayerOverride {
    /// `ZLO_NONE`.
    #[default]
    None,
    /// `ZLO_FORCE_FLASHED`.
    ForceFlashed,
    /// `ZLO_FORCE_NO_ZONE_CONNECTION`.
    ForceNoZoneConnection,
}

/// `ENDPOINT_T`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum EndpointT {
    Start = 0,
    End = 1,
}

/// `TENTING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum TentingMode {
    #[default]
    FromBoard = 0,
    Tented = 1,
    NotTented = 2,
}

/// `COVERING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum CoveringMode {
    #[default]
    FromBoard = 0,
    Covered = 1,
    NotCovered = 2,
}

/// `PLUGGING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PluggingMode {
    #[default]
    FromBoard = 0,
    Plugged = 1,
    NotPlugged = 2,
}

/// `CAPPING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum CappingMode {
    #[default]
    FromBoard = 0,
    Capped = 1,
    NotCapped = 2,
}

/// `FILLING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum FillingMode {
    #[default]
    FromBoard = 0,
    Filled = 1,
    NotFilled = 2,
}

/// `UNDEFINED_DRILL_DIAMETER`.
pub const UNDEFINED_DRILL_DIAMETER: i32 = -1;

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbTrack {
    /// `PCB_TRACK( aParent )`: width 0.2 mm, no solder mask, nil UUID.
    fn default() -> Self {
        PcbTrack {
            base: ConnectedItemBase::default(),
            start: Vector2I::default(),
            end: Vector2I::default(),
            has_solder_mask: false,
            solder_mask_margin: None,
            width: PCB_IU_SCALE.mm_to_iu(0.2), // Gives a reasonable default width
        }
    }
}

impl PcbTrack {
    /// `new PCB_TRACK( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        let mut track = PcbTrack::default();
        track.base.item.uuid = Kiid::new(kiid_generator);
        track
    }
}

impl PcbArc {
    /// `new PCB_ARC( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        PcbArc {
            track: PcbTrack::new(kiid_generator),
            mid: Vector2I::default(),
        }
    }
}

impl Default for PcbVia {
    /// `PCB_VIA( aParent )`: through via F_Cu - B_Cu, default drill, circle padstack, no zone
    /// layer overrides, not free.
    fn default() -> Self {
        PcbVia {
            track: PcbTrack::default(),
            via_type: Viatype::Through,
            padstack: Padstack::for_new_via(),
            is_free: false,
            zone_layer_overrides: [ZoneLayerOverride::None; MAX_CU_LAYERS as usize],
        }
    }
}

impl PcbVia {
    /// `new PCB_VIA( aParent )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator) -> Self {
        PcbVia {
            track: PcbTrack::new(kiid_generator),
            ..PcbVia::default()
        }
    }
}

// ---- Track: the tagged PCB_TRACK* --------------------------------------------------------

/// Run `$e` with `$t` bound to the variant (`&PcbTrack`, `&PcbArc` or `&PcbVia`).
macro_rules! on_track {
    ($self:expr, $t:ident => $e:expr) => {
        match $self {
            Track::Trace($t) => $e,
            Track::Arc($t) => $e,
            Track::Via(via) => {
                let $t = &**via;
                $e
            }
        }
    };
}

/// Mutable form of `on_track!`.
macro_rules! on_track_mut {
    ($self:expr, $t:ident => $e:expr) => {
        match $self {
            Track::Trace($t) => $e,
            Track::Arc($t) => $e,
            Track::Via(via) => {
                let $t = &mut **via;
                $e
            }
        }
    };
}

impl Track {
    /// The `PCB_TRACK` base of any variant.
    pub fn as_track(&self) -> &PcbTrack {
        match self {
            Track::Trace(t) => t,
            Track::Arc(a) => &a.track,
            Track::Via(v) => &v.track,
        }
    }

    /// Mutable `PCB_TRACK` base of any variant.
    pub fn as_track_mut(&mut self) -> &mut PcbTrack {
        match self {
            Track::Trace(t) => t,
            Track::Arc(a) => &mut a.track,
            Track::Via(v) => &mut v.track,
        }
    }

    /// `dynamic_cast<PCB_ARC*>`.
    pub fn as_arc(&self) -> Option<&PcbArc> {
        match self {
            Track::Arc(a) => Some(a),
            _ => None,
        }
    }

    /// `dynamic_cast<PCB_VIA*>`.
    pub fn as_via(&self) -> Option<&PcbVia> {
        match self {
            Track::Via(v) => Some(v),
            _ => None,
        }
    }

    /// Mutable `dynamic_cast<PCB_VIA*>`.
    pub fn as_via_mut(&mut self) -> Option<&mut PcbVia> {
        match self {
            Track::Via(v) => Some(v),
            _ => None,
        }
    }

    /// `Type()`.
    pub fn type_(&self) -> KicadT {
        on_track!(self, t => BoardItem::type_(t))
    }

    // ---- virtuals introduced by PCB_TRACK (dispatch to the variant) ----

    /// `GetWidth()` (virtual in `PCB_TRACK`): the width of a segment or arc. `PCB_VIA`
    /// overrides it with an assertion ("called without a layer argument"); use
    /// `PcbVia::width( layer )` for vias.
    pub fn width(&self) -> i32 {
        match self {
            Track::Trace(t) => t.width(),
            Track::Arc(a) => a.track.width(),
            Track::Via(v) => v.width_no_layer(),
        }
    }

    /// `SetWidth( aWidth )` (virtual in `PCB_TRACK`; `PCB_VIA` overrides it like `GetWidth`).
    pub fn set_width(&mut self, width: i32) {
        match self {
            Track::Trace(t) => t.set_width(width),
            Track::Arc(a) => a.track.set_width(width),
            Track::Via(v) => v.set_width_no_layer(width),
        }
    }

    /// `GetLength()` (virtual in `PCB_TRACK`; `PCB_ARC` overrides it, a via inherits it).
    pub fn length(&self) -> f64 {
        match self {
            Track::Trace(t) => t.length(),
            Track::Arc(a) => a.length(),
            Track::Via(v) => v.track.length(),
        }
    }

    /// `GetWidthConstraint( aSource )` (virtual in `PCB_TRACK`; `PCB_VIA` overrides it, an arc
    /// inherits it): the value and, if asked, the name of the constraint.
    pub fn width_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        match self {
            Track::Trace(t) => t.width_constraint(ctx, want_source),
            Track::Arc(a) => a.width_constraint(ctx, want_source),
            Track::Via(v) => v.width_constraint(ctx, want_source),
        }
    }

    // ---- non-virtual PCB_TRACK members that look at Type() ----

    /// Port of `PCB_TRACK::IsNull`: a via, or a segment / arc with `m_Start == m_End`.
    pub fn is_null(&self) -> bool {
        self.type_() == KicadT::PcbViaT || self.as_track().start == self.as_track().end
    }

    /// Port of `PCB_TRACK::GetFriendlyName` as written (a `switch( Type() )`): "Track",
    /// "Track (arc)" or "Via".
    pub fn track_friendly_name(&self) -> String {
        match self.type_() {
            KicadT::PcbArcT => "Track (arc)".to_string(),
            KicadT::PcbViaT => "Via".to_string(),
            _ => "Track".to_string(),
        }
    }
}

/// Port of `PCB_TRACK::cmp_tracks::operator()` as an `Ordering` (the save order of
/// `BOARD::Tracks()`): net code, then `GetLayer()` (virtual: a via answers its start layer),
/// then `Type()`, then `m_Uuid`, then pointer order, which is `TrackId` order
/// (`board-model.md` 3.1). A stale handle sorts by `TrackId` only.
pub fn pcb_track_cmp_tracks(ctx: &BoardCtx<'_>, a: TrackId, b: TrackId) -> Ordering {
    let (Some(ta), Some(tb)) = (ctx.board.track(a), ctx.board.track(b)) else {
        return a.cmp(&b);
    };

    if ta.net_code() != tb.net_code() {
        return ta.net_code().cmp(&tb.net_code());
    }

    let (layer_a, layer_b) = (BoardItem::layer(ta, ctx), BoardItem::layer(tb, ctx));

    if layer_a != layer_b {
        return layer_a.cmp(&layer_b);
    }

    if ta.type_() != tb.type_() {
        return (ta.type_() as i32).cmp(&(tb.type_() as i32));
    }

    if BoardItem::uuid(ta) != BoardItem::uuid(tb) {
        return BoardItem::uuid(ta).cmp(BoardItem::uuid(tb));
    }

    a.cmp(&b)
}

/// `EDA_ITEM_FLAGS STARTPOINT` (`include/eda_item_flags.h`), returned by
/// `PcbTrack::is_point_on_ends`.
pub const STARTPOINT: u32 = 1 << 9;
/// `EDA_ITEM_FLAGS ENDPOINT`.
pub const ENDPOINT: u32 = 1 << 10;

/// Hands `this` of a track-family item to `f` as the `BOARD_ITEM*` the rule resolver gets:
/// the stored item when the value carries a live handle, otherwise a view of the copy that
/// `detached` builds (C++ passes the address of an unregistered value; its copy has the same
/// data and, like it, no handle).
fn with_this<R>(
    ctx: &BoardCtx<'_>,
    base: &BoardItemBase,
    detached: impl FnOnce() -> Track,
    f: impl FnOnce(ItemRef<'_>) -> R,
) -> R {
    if let Some(this) = base.id().and_then(|id| ctx.item(id)) {
        return f(this);
    }

    let copy = detached();
    f(ctx.view(AnyItem::Track(&copy)))
}

/// `DRC_CONSTRAINT::GetName()` (`GetRuleName( m_parentRule )`): the implicit rule's name,
/// "rule '<name>'" for a user rule, the constraint's own name without a rule.
fn constraint_name(constraint: &DrcConstraint) -> String {
    match &constraint.parent_rule {
        Some(rule) if rule.implicit => rule.name.clone(),
        Some(rule) => format!("rule '{}'", rule.name),
        None => constraint.name.clone(),
    }
}

/// Port of `PCB_TRACK::GetWidthConstraint( aSource )` for the segment or arc `track`
/// (`detached` builds `this` for an item without a handle).
fn width_constraint_of(
    track: &PcbTrack,
    ctx: &BoardCtx<'_>,
    want_source: bool,
    detached: impl FnOnce() -> Track,
) -> (MinOptMax<i32>, Option<String>) {
    let mut aggregate = DrcConstraint::default();

    if board_item::has_board(track, ctx) {
        if let Some(drc_engine) = ctx.rules {
            // Tracks are, at present, single layer
            let layer = track.base.item.layer;
            aggregate = with_this(ctx, &track.base.item, detached, |this| {
                drc_engine.eval_rules(
                    ctx,
                    DrcConstraintT::TrackWidthConstraint,
                    Some(this),
                    None,
                    layer,
                )
            });
        }
    }

    let source = want_source.then(|| constraint_name(&aggregate));

    (aggregate.value, source)
}

/// Port of `PCB_TRACK::GetSolderMaskExpansion` for the segment or arc `track`: the
/// `SOLDER_MASK_EXPANSION_CONSTRAINT` rule (if the engine has any), else the local margin,
/// else the board setting; a negative margin is limited to half the width.
fn solder_mask_expansion_of(
    track: &PcbTrack,
    ctx: &BoardCtx<'_>,
    detached: impl FnOnce() -> Track,
) -> i32 {
    let mut margin = 0;
    let board = board_item::has_board(track, ctx);
    let drc_engine = ctx.rules.filter(|engine| {
        board && engine.has_rules_for_constraint_type(DrcConstraintT::SolderMaskExpansionConstraint)
    });

    if let Some(drc_engine) = drc_engine {
        let layer = track.base.item.layer;
        let constraint = with_this(ctx, &track.base.item, detached, |this| {
            drc_engine.eval_rules(
                ctx,
                DrcConstraintT::SolderMaskExpansionConstraint,
                Some(this),
                None,
                layer,
            )
        });

        if constraint.value.has_opt() {
            margin = constraint.value.opt();
        }
    } else if let Some(local) = track.solder_mask_margin {
        margin = local;
    } else if board {
        margin = ctx.board.design_settings().solder_mask_expansion;
    }

    // Ensure the resulting mask opening has a non-negative size
    if margin < 0 {
        margin = margin.max(-track.width / 2);
    }

    margin
}

/// Port of `PCB_TRACK::GetBoundingBox` for a segment (`arc_bbox == None`: the box of the two
/// end points) or an arc (`Some( GetEffectiveShape()->BBox() )`), grown by the rounded-up
/// half width; `[pos, dim)` in nature, hence the `+1`.
fn bounding_box_of(track: &PcbTrack, arc_bbox: Option<Box2I>) -> Box2I {
    // end of track is round, this is its radius, rounded up
    let radius = (track.width + 1) / 2;
    let (mut ymax, mut xmax, mut ymin, mut xmin);

    if let Some(bbox) = arc_bbox {
        xmin = bbox.left();
        xmax = bbox.right();
        ymin = bbox.top();
        ymax = bbox.bottom();
    } else {
        ymax = track.start.y.max(track.end.y);
        xmax = track.start.x.max(track.end.x);

        ymin = track.start.y.min(track.end.y);
        xmin = track.start.x.min(track.end.x);
    }

    ymax += radius;
    xmax += radius;

    ymin -= radius;
    xmin -= radius;

    // return a rectangle which is [pos,dim) in nature.  therefore the +1
    box2i_safe_from_int(
        Vector2I::new(xmin, ymin),
        Vector2L::new(xmax as i64 - xmin as i64 + 1, ymax as i64 - ymin as i64 + 1),
    )
}

impl From<PcbTrack> for Track {
    fn from(t: PcbTrack) -> Self {
        Track::Trace(t)
    }
}

impl From<PcbArc> for Track {
    fn from(a: PcbArc) -> Self {
        Track::Arc(a)
    }
}

impl From<PcbVia> for Track {
    fn from(v: PcbVia) -> Self {
        Track::Via(Box::new(v))
    }
}

// ---- PcbTrack: delegation shims (frozen after M0) -----------------------------------------

impl Invalidate for PcbTrack {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbTrack {
    fn base(&self) -> &BoardItemBase {
        &self.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbTraceT
    }
    fn class_name(&self) -> &'static str {
        "PCB_TRACK"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.base)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbTrack::is_on_layer(self, ctx, layer)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbTrack::layer_set(self, ctx)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbTrack::is_on_copper_layer(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbTrack::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbTrack::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbTrack::effective_shape(self, ctx, layer, flash, usage)
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
        PcbTrack::transform_shape_to_polygon(
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
        PcbTrack::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbTrack::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbTrack::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbTrack::friendly_name(self, ctx)
    }
}

impl BoardConnectedItem for PcbTrack {}

impl BoardItemMut for PcbTrack {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbTrack::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbTrack::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbTrack::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTrack::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbTrack::mirror(self, env, centre, dir)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbTrack::set_layer_set(self, env, layers)
    }
}

// ---- PcbArc: delegation shims (frozen after M0) -------------------------------------------

impl Invalidate for PcbArc {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbArc {
    fn base(&self) -> &BoardItemBase {
        &self.track.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.track.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbArcT
    }
    fn class_name(&self) -> &'static str {
        "PCB_ARC"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.track.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.track.base)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbArc::is_on_layer(self, ctx, layer)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbArc::layer_set(self, ctx)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbArc::is_on_copper_layer(self, ctx)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbArc::position(self, ctx)
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbArc::center(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbArc::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbArc::effective_shape(self, ctx, layer, flash, usage)
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
        PcbArc::transform_shape_to_polygon(
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
        PcbArc::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbArc::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbArc::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbArc::friendly_name(self, ctx)
    }
}

impl BoardConnectedItem for PcbArc {}

impl BoardItemMut for PcbArc {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbArc::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbArc::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbArc::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbArc::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbArc::mirror(self, env, centre, dir)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbArc::set_layer_set(self, env, layers)
    }
}

// ---- PcbVia: delegation shims (frozen after M0) -------------------------------------------

impl Invalidate for PcbVia {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbVia {
    fn base(&self) -> &BoardItemBase {
        &self.track.base.item
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.track.base.item
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbViaT
    }
    fn class_name(&self) -> &'static str {
        "PCB_VIA"
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        Some(&self.track.base)
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        Some(&mut self.track.base)
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        PcbVia::layer(self, ctx)
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        PcbVia::layer_set(self, ctx)
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbVia::is_on_layer(self, ctx, layer)
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbVia::is_on_copper_layer(self, ctx)
    }
    fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        PcbVia::fits_enabled_layers(self, ctx, enabled, copper_layer_count)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbVia::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbVia::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbVia::effective_shape(self, ctx, layer, flash, usage)
    }
    fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        PcbVia::effective_hole_shape(self, ctx, layer, usage)
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
        PcbVia::transform_shape_to_polygon(
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
    fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbVia::has_hole(self, ctx)
    }
    fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        PcbVia::has_drilled_hole(self, ctx)
    }
    fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        PcbVia::is_tented(self, ctx, layer)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbVia::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbVia::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbVia::item_description(self, ctx, units, full)
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        PcbVia::friendly_name(self, ctx)
    }
    fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String {
        PcbVia::layer_mask_describe(self, ctx)
    }
}

impl BoardConnectedItem for PcbVia {}

impl BoardItemMut for PcbVia {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbVia::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbVia::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbVia::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbVia::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbVia::mirror(self, env, centre, dir)
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        PcbVia::set_layer(self, env, layer)
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        PcbVia::set_layer_set(self, env, layers)
    }
}

// ---- Track: dispatch to the variant (every trait method) --------------------------------

impl Invalidate for Track {
    fn invalidate_caches(&mut self) {
        on_track_mut!(self, t => Invalidate::invalidate_caches(t))
    }
}

impl BoardItem for Track {
    fn base(&self) -> &BoardItemBase {
        on_track!(self, t => BoardItem::base(t))
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        on_track_mut!(self, t => BoardItem::base_mut(t))
    }
    fn type_(&self) -> KicadT {
        on_track!(self, t => BoardItem::type_(t))
    }
    fn class_name(&self) -> &'static str {
        on_track!(self, t => BoardItem::class_name(t))
    }
    fn connected(&self) -> Option<&ConnectedItemBase> {
        on_track!(self, t => BoardItem::connected(t))
    }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> {
        on_track_mut!(self, t => BoardItem::connected_mut(t))
    }
    fn uuid(&self) -> &Kiid {
        on_track!(self, t => BoardItem::uuid(t))
    }
    fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> {
        on_track!(self, t => BoardItem::parent_group(t, ctx))
    }
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId {
        on_track!(self, t => BoardItem::layer(t, ctx))
    }
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset {
        on_track!(self, t => BoardItem::layer_set(t, ctx))
    }
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        on_track!(self, t => BoardItem::is_on_layer(t, ctx, layer))
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardItem::is_on_copper_layer(t, ctx))
    }
    fn is_side_specific(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardItem::is_side_specific(t, ctx))
    }
    fn is_layer_agnostic(&self) -> bool {
        on_track!(self, t => BoardItem::is_layer_agnostic(t))
    }
    fn fits_enabled_layers(
        &self,
        ctx: &BoardCtx<'_>,
        enabled: Lset,
        copper_layer_count: i32,
    ) -> bool {
        on_track!(self, t => BoardItem::fits_enabled_layers(t, ctx, enabled, copper_layer_count))
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        on_track!(self, t => BoardItem::position(t, ctx))
    }
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        on_track!(self, t => BoardItem::center(t, ctx))
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        on_track!(self, t => BoardItem::bounding_box(t, ctx))
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        on_track!(self, t => BoardItem::effective_shape(t, ctx, layer, flash, usage))
    }
    fn effective_hole_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        usage: DrcConstraintT,
    ) -> Option<Arc<ShapeSegment>> {
        on_track!(self, t => BoardItem::effective_hole_shape(t, ctx, layer, usage))
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
        on_track!(self, t => BoardItem::transform_shape_to_polygon(t, ctx, buffer, layer, clearance, error, error_loc, ignore_line_width))
    }
    fn transform_shape_to_poly_set(
        &self,
        ctx: &BoardCtx<'_>,
        buffer: &mut ShapePolySet,
        layer: PcbLayerId,
        clearance: i32,
        error: i32,
        error_loc: ErrorLoc,
    ) {
        on_track!(self, t => BoardItem::transform_shape_to_poly_set(t, ctx, buffer, layer, clearance, error, error_loc))
    }
    fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardItem::has_hole(t, ctx))
    }
    fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardItem::has_drilled_hole(t, ctx))
    }
    fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        on_track!(self, t => BoardItem::is_tented(t, ctx, layer))
    }
    fn has_line_stroke(&self) -> bool {
        on_track!(self, t => BoardItem::has_line_stroke(t))
    }
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams {
        on_track!(self, t => BoardItem::stroke(t, ctx))
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        on_track!(self, t => BoardItem::hit_test(t, ctx, pos, accuracy))
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        on_track!(self, t => BoardItem::hit_test_rect(t, ctx, rect, contained, accuracy))
    }
    fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardItem::is_locked(t, ctx))
    }
    fn is_knockout(&self) -> bool {
        on_track!(self, t => BoardItem::is_knockout(t))
    }
    fn run_on_children(
        &self,
        ctx: &BoardCtx<'_>,
        mode: RecurseMode,
        f: &mut dyn FnMut(ItemRef<'_>),
    ) {
        on_track!(self, t => BoardItem::run_on_children(t, ctx, mode, f))
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        on_track!(self, t => BoardItem::item_description(t, ctx, units, full))
    }
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String {
        on_track!(self, t => BoardItem::friendly_name(t, ctx))
    }
    fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String {
        on_track!(self, t => BoardItem::layer_mask_describe(t, ctx))
    }
}

impl BoardConnectedItem for Track {
    fn net_code(&self) -> NetCode {
        on_track!(self, t => BoardConnectedItem::net_code(t))
    }
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool {
        on_track!(self, t => BoardConnectedItem::is_connected(t, ctx))
    }
    fn effective_netclass<'a>(&self, ctx: &BoardCtx<'a>) -> &'a Netclass {
        on_track!(self, t => BoardConnectedItem::effective_netclass(t, ctx))
    }
    fn own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        on_track!(self, t => BoardConnectedItem::own_clearance(t, ctx, layer, want_source))
    }
    fn local_clearance(&self) -> Option<i32> {
        on_track!(self, t => BoardConnectedItem::local_clearance(t))
    }
    fn clearance_overrides(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> Option<(i32, Option<String>)> {
        on_track!(self, t => BoardConnectedItem::clearance_overrides(t, ctx, want_source))
    }
}

impl BoardItemMut for Track {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        on_track_mut!(self, t => BoardItemMut::set_position(t, env, pos))
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        on_track_mut!(self, t => BoardItemMut::move_by(t, env, delta))
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        on_track_mut!(self, t => BoardItemMut::rotate(t, env, centre, angle))
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        on_track_mut!(self, t => BoardItemMut::flip(t, env, centre, dir))
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        on_track_mut!(self, t => BoardItemMut::mirror(t, env, centre, dir))
    }
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId) {
        on_track_mut!(self, t => BoardItemMut::set_layer(t, env, layer))
    }
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        on_track_mut!(self, t => BoardItemMut::set_layer_set(t, env, layers))
    }
    fn normalize(&mut self) {
        on_track_mut!(self, t => BoardItemMut::normalize(t))
    }
    fn on_footprint_transformed(&mut self, env: &MutEnv) {
        on_track_mut!(self, t => BoardItemMut::on_footprint_transformed(t, env))
    }
    fn on_footprint_rescaled(
        &mut self,
        env: &MutEnv,
        ratio_x: f64,
        ratio_y: f64,
        linear_factor: f64,
        anchor: Vector2I,
        parent_rotate: EdaAngle,
    ) {
        on_track_mut!(self, t => BoardItemMut::on_footprint_rescaled(t, env, ratio_x, ratio_y, linear_factor, anchor, parent_rotate))
    }
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {
        on_track_mut!(self, t => BoardItemMut::style_from_settings(t, settings, check_side))
    }
}

// ---- PcbTrack: inherent methods (package M16) ---------------------------------------------

impl PcbTrack {
    /// `GetStart()`.
    pub fn start(&self) -> Vector2I {
        self.start
    }

    /// `SetStart()`.
    pub fn set_start(&mut self, start: Vector2I) {
        self.start = start;
    }

    /// `GetEnd()`.
    pub fn end(&self) -> Vector2I {
        self.end
    }

    /// `SetEnd()`.
    pub fn set_end(&mut self, end: Vector2I) {
        self.end = end;
    }

    /// `PCB_TRACK::GetWidth()` (vias override it with a per-layer padstack lookup).
    pub fn width(&self) -> i32 {
        self.width
    }

    /// `PCB_TRACK::SetWidth()`.
    pub fn set_width(&mut self, width: i32) {
        self.width = width;
    }

    /// `PCB_TRACK::GetLength()`: `m_Start.Distance( m_End )`.
    pub fn length(&self) -> f64 {
        self.start.distance(self.end)
    }

    /// `PCB_TRACK::GetWidthConstraint( aSource )`: `TRACK_WIDTH_CONSTRAINT` from `ctx.rules`
    /// on the track's layer, a null constraint without rules; (value, constraint name if
    /// asked).
    pub fn width_constraint(
        &self,
        ctx: &BoardCtx<'_>,
        want_source: bool,
    ) -> (MinOptMax<i32>, Option<String>) {
        width_constraint_of(self, ctx, want_source, || Track::Trace(self.clone()))
    }

    /// `SetStartX()`.
    pub fn set_start_x(&mut self, x: i32) {
        self.start.x = x;
    }

    /// `SetStartY()`.
    pub fn set_start_y(&mut self, y: i32) {
        self.start.y = y;
    }

    /// `SetEndX()`.
    pub fn set_end_x(&mut self, x: i32) {
        self.end.x = x;
    }

    /// `SetEndY()`.
    pub fn set_end_y(&mut self, y: i32) {
        self.end.y = y;
    }

    /// `GetEndPoint( aEndPoint )`: the start or the end point.
    pub fn end_point(&self, end_point: EndpointT) -> Vector2I {
        if end_point == EndpointT::Start {
            self.start
        } else {
            self.end
        }
    }

    /// `SetHasSolderMask()`.
    pub fn set_has_solder_mask(&mut self, val: bool) {
        self.has_solder_mask = val;
    }

    /// `HasSolderMask()`.
    pub fn has_solder_mask(&self) -> bool {
        self.has_solder_mask
    }

    /// `SetLocalSolderMaskMargin()`.
    pub fn set_local_solder_mask_margin(&mut self, margin: Option<i32>) {
        self.solder_mask_margin = margin;
    }

    /// `GetLocalSolderMaskMargin()`.
    pub fn local_solder_mask_margin(&self) -> Option<i32> {
        self.solder_mask_margin
    }

    /// Port of `PCB_TRACK::GetSolderMaskExpansion` for a segment (`PcbArc` has its own, so
    /// that the rule resolver sees the arc; `PCB_VIA` hides it with its own body).
    pub fn solder_mask_expansion(&self, ctx: &BoardCtx<'_>) -> i32 {
        solder_mask_expansion_of(self, ctx, || Track::Trace(self.clone()))
    }

    /// Port of `PCB_TRACK::IsNull` for a segment or arc: `m_Start == m_End` (`Track::is_null`
    /// also answers for vias).
    pub fn is_null(&self) -> bool {
        self.start == self.end
    }

    /// Port of `PCB_TRACK::IsPointOnEnds( point, min_dist )`: `STARTPOINT` / `ENDPOINT` bits
    /// for the ends within `min_dist` of `point` (exact match for 0, half the width for a
    /// negative value; the C++ default is 0).
    pub fn is_point_on_ends(&self, point: Vector2I, min_dist: i32) -> u32 {
        let mut result: u32 = 0;
        let mut min_dist = min_dist;

        if min_dist < 0 {
            min_dist = self.width / 2;
        }

        if min_dist == 0 {
            if self.start == point {
                result |= STARTPOINT;
            }

            if self.end == point {
                result |= ENDPOINT;
            }
        } else {
            let mut dist = self.start.distance(point);

            if min_dist as f64 >= dist {
                result |= STARTPOINT;
            }

            dist = self.end.distance(point);

            if min_dist as f64 >= dist {
                result |= ENDPOINT;
            }
        }

        result
    }

    /// Port of `PCB_TRACK::ApproxCollinear( aTrack )`: `SEG::ApproxCollinear` of the two
    /// start - end segments.
    pub fn approx_collinear(&self, track: &PcbTrack) -> bool {
        let a = Seg::new(self.start, self.end);
        let b = Seg::new(track.start(), track.end());
        a.approx_collinear(&b)
    }

    /// `PCB_TRACK::GetPosition()`: `m_Start`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        self.start
    }

    /// `PCB_TRACK::SetPosition()`: `m_Start = aPos`.
    pub fn set_position(&mut self, _env: &MutEnv, pos: Vector2I) {
        self.start = pos;
    }

    /// `PCB_TRACK::Move()`.
    pub fn move_by(&mut self, _env: &MutEnv, delta: Vector2I) {
        self.start += delta;
        self.end += delta;
    }

    /// `PCB_TRACK::IsOnCopperLayer()`: a track or a via is always on a copper layer.
    pub fn is_on_copper_layer(&self, _ctx: &BoardCtx<'_>) -> bool {
        true
    }

    /// Port of `PCB_TRACK::IsOnLayer`: the track's layer, and the mask layer of an outer
    /// copper layer when the track has a solder mask opening.
    pub fn is_on_layer(&self, _ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {
        let own = self.base.item.layer;

        if layer == own {
            return true;
        }

        if self.has_solder_mask
            && ((layer == PcbLayerId::FMask && own == PcbLayerId::FCu)
                || (layer == PcbLayerId::BMask && own == PcbLayerId::BCu))
        {
            return true;
        }

        false
    }

    /// Port of `PCB_TRACK::GetLayerSet`: `{ m_layer }`, plus the matching mask layer of an
    /// outer copper layer when the track has a solder mask opening.
    pub fn layer_set(&self, _ctx: &BoardCtx<'_>) -> Lset {
        let mut layermask = Lset::from_layers(&[self.base.item.layer]);

        if self.has_solder_mask {
            if layermask.contains(PcbLayerId::FCu) {
                layermask.set(PcbLayerId::FMask);
            } else if layermask.contains(PcbLayerId::BCu) {
                layermask.set(PcbLayerId::BMask);
            }
        }

        layermask
    }

    /// Port of `PCB_TRACK::SetLayerSet`: every copper layer of the set becomes the layer (the
    /// last one wins), any solder mask layer switches the mask opening on.
    pub fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {
        layers.run_on_layers(|layer| {
            if is_copper_layer(layer.0) {
                BoardItemMut::set_layer(self, env, layer);
            } else if is_solder_mask_layer(layer.0) {
                self.set_has_solder_mask(true);
            }
        });
    }

    /// Port of `PCB_TRACK::GetBoundingBox` (segment branch).
    pub fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        bounding_box_of(self, None)
    }

    /// Port of `PCB_TRACK::GetEffectiveShape`: a `SHAPE_SEGMENT` of the track width, plus
    /// twice the solder mask expansion on mask layers.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        _flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        let mut width = self.width;

        if is_solder_mask_layer(layer.0) {
            width += 2 * self.solder_mask_expansion(ctx);
        }

        Arc::new(ShapeSegment::new(self.start, self.end, width))
    }

    /// Port of `PCB_TRACK::TransformShapeToPolygon` (segment branch): `TransformOvalToPolygon`
    /// with the width plus twice the clearance (plus twice the solder mask expansion on mask
    /// layers).
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
        debug_assert!(
            !ignore_line_width,
            "IgnoreLineWidth has no meaning for tracks."
        );

        let mut width = self.width + (2 * clearance);

        if is_solder_mask_layer(layer.0) {
            width += 2 * self.solder_mask_expansion(ctx);
        }

        transform_oval_to_polygon(buffer, self.start, self.end, width, error, error_loc, 0);
    }

    /// Port of `PCB_TRACK::HitTest( aPosition, aAccuracy )`: `TestSegmentHit` with the
    /// accuracy plus half the width.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        test_segment_hit(pos, self.start, self.end, accuracy + (self.width / 2))
    }

    /// Port of `PCB_TRACK::HitTest( aRect, aContained, aAccuracy )`: both end points inside
    /// the inflated rectangle, or the segment intersecting it.
    pub fn hit_test_rect(
        &self,
        _ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        let mut arect = *rect;
        arect.inflate(accuracy);

        if contained {
            arect.contains(self.start) && arect.contains(self.end)
        } else {
            arect.intersects_segment(self.start, self.end)
        }
    }

    /// Port of `PCB_TRACK::Rotate`: start and end rotated about `centre`.
    pub fn rotate(&mut self, _env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        rotate_point_around(&mut self.start, centre, angle);
        rotate_point_around(&mut self.end, centre, angle);
    }

    /// Port of `PCB_TRACK::Flip`: mirror both ends about `centre` and move to the flipped
    /// layer (`GetFlippedLayer`, from the board's flip table in `env`).
    pub fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        if dir == FlipDirection::LeftRight {
            self.start.x = centre.x - (self.start.x - centre.x);
            self.end.x = centre.x - (self.end.x - centre.x);
        } else {
            self.start.y = centre.y - (self.start.y - centre.y);
            self.end.y = centre.y - (self.end.y - centre.y);
        }

        let layer = env.layers.flip_layer(self.base.item.layer);
        BoardItemMut::set_layer(self, env, layer);
    }

    /// Port of `PCB_TRACK::Mirror( aCentre, aFlipDirection )`: `Mirror( MIRROR_AXIS( .. ) )`.
    pub fn mirror(&mut self, _env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        self.mirror_seg(&board_item::mirror_axis(centre, dir));
    }

    /// Port of `PCB_TRACK::Mirror( const SEG& aAxis )`: reflect both ends.
    pub fn mirror_seg(&mut self, axis: &Seg) {
        self.start = axis.reflect_point(self.start);
        self.end = axis.reflect_point(self.end);
    }

    /// Port of `PCB_TRACK::GetItemDescription` (segment): "Track <net> on <layer>, length
    /// <len>" (English).
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        format!(
            "Track {} on {}, length {}",
            board_connected_item::netname_msg(self, ctx),
            board_item::layer_name(self, ctx),
            units.message_text_from_value(self.length(), true, EdaDataType::Distance)
        )
    }

    /// Port of `PCB_TRACK::GetFriendlyName` (segment): "Track".
    pub fn friendly_name(&self, _ctx: &BoardCtx<'_>) -> String {
        "Track".to_string()
    }

    /// Port of `PCB_TRACK::operator==( const PCB_TRACK& )`: start, end, layer, width, solder
    /// mask flag and margin.
    pub fn eq_item(&self, other: &PcbTrack) -> bool {
        self.start == other.start
            && self.end == other.end
            && self.base.item.layer == other.base.item.layer
            && self.width == other.width
            && self.has_solder_mask == other.has_solder_mask
            && self.solder_mask_margin == other.solder_mask_margin
    }

    /// Port of `PCB_TRACK::Similarity` for two segments (the C++ returns 0.0 for another
    /// type).
    pub fn similarity(&self, other: &PcbTrack) -> f64 {
        let mut similarity = 1.0;

        if self.base.item.layer != other.base.item.layer {
            similarity *= 0.9;
        }

        if self.width != other.width {
            similarity *= 0.9;
        }

        if self.start != other.start {
            similarity *= 0.9;
        }

        if self.end != other.end {
            similarity *= 0.9;
        }

        if self.has_solder_mask != other.has_solder_mask {
            similarity *= 0.9;
        }

        if self.solder_mask_margin != other.solder_mask_margin {
            similarity *= 0.9;
        }

        similarity
    }
}

#[cfg(test)]
mod tests {
    //! Package M16 tests. The golden table below was produced by the reference `pcb-dump`
    //! (`pcb-dump board --sections item,bbox,track`) on `qa/data/pcbnew/tracks_arcs_vias.kicad_pcb`
    //! upgraded to format 20241229 with `(pad_to_mask_clearance 0.07)` and eleven added
    //! tracks / arcs (solder mask openings with positive, negative and no local margin, a
    //! zero-length segment, a degenerated arc, half circles both ways, a nearly straight arc
    //! that the loader turns into a segment). The board parser does not load boards yet
    //! (`BOARD::SetLayerDescr` pending), so the tracks are rebuilt from the dumped members and
    //! every dumped derived value is asserted; polygons are compared by the FNV-1a hash of the
    //! dump's `P(..)` text (`PCB_DUMP_FORMAT.md`, `--hash`).
    //!
    //! Columns: uuid | T(race)/A(rc) | layer | start | end | mid | width | has_solder_mask |
    //! local_solder_mask_margin | net | bbox | length | shape | position | layers |
    //! solder_mask_expansion | is_null | radius | angle | angle_start | angle_end | ccw |
    //! degenerated | tpoly c0_e5000_in | tpoly c0_e1000_out | tpoly c100000_e5000_out
    //! (tpoly on the track's own layer: clearance, max error, ERROR_INSIDE / ERROR_OUTSIDE).

    use super::*;
    use crate::board::{AddMode, Board};
    use crate::drc_api::DrcRuleInfo;
    use crate::item::ParentRef;
    use crate::netinfo::NetinfoItem;
    use kicad_core::PCB_IU_SCALE;
    use kicad_geom::geometry::approximation::{ERROR_INSIDE, ERROR_OUTSIDE};
    use kicad_geom::{Shape, ShapeRef};

    const GOLDEN: &str = "\
3b5d67ef-2181-444a-b3d4-ffb387b384f4|T|F.Cu|29381843,35118156|29000000,35500000|-|250000|false|none|0|28875000,34993156,631844,631845|540008.25640447391|SH_SEGMENT(a=29381843,35118156 b=29000000,35500000 w=250000)|29381843,35118156|[F.Cu]|70000|false|-|-|-|-|-|-|40161538195e0967|b348f90ed2a87d86|1a65260c2f98dd97
5b12cfbe-66bd-4c13-ab5c-9ba796d7a78e|T|F.Cu|28000000,37914213|28000000,39000000|-|250000|false|none|0|27875000,37789213,250001,1335788|1085787|SH_SEGMENT(a=28000000,37914213 b=28000000,39000000 w=250000)|28000000,37914213|[F.Cu]|70000|false|-|-|-|-|-|-|961da9cd8a7af0d4|5a9e0588d2f002f5|6229ca8a11b0f113
7c0cb8dd-5fa9-4105-a133-e818c51ff27b|T|F.Cu|35500000,32500000|35635000,32365000|-|250000|false|none|0|35375000,32240000,385001,385001|190918.83092036785|SH_SEGMENT(a=35500000,32500000 b=35635000,32365000 w=250000)|35500000,32500000|[F.Cu]|70000|false|-|-|-|-|-|-|478fd5397d76504c|222609ab478757f2|4ec097ad7c91e2f5
a37ea611-411a-4068-b4fe-e6c0513cdcd5|T|F.Cu|28707107,40707107|29000000,41000000|-|250000|false|none|0|28582107,40582107,542894,542894|414213.25292414293|SH_SEGMENT(a=28707107,40707107 b=29000000,41000000 w=250000)|28707107,40707107|[F.Cu]|70000|false|-|-|-|-|-|-|42b930697d1e195c|17050b75527f9269|d84e6a98949c7787
b3d4fe9c-0354-4101-9923-bb4c27387bb8|T|F.Cu|35635000,32365000|38037500,32365000|-|250000|false|none|0|35510000,32240000,2652501,250001|2402500|SH_SEGMENT(a=35635000,32365000 b=38037500,32365000 w=250000)|35635000,32365000|[F.Cu]|70000|false|-|-|-|-|-|-|cefc7c9699079fb8|75b6af9edbe70531|d416025f9f4a99eb
6f36121a-dffc-4c29-8105-42f651bbab96|A|F.Cu|29000000,35500000|28000000,37914213|28259892,36607650|250000|false|none|0|27749999,35249999,1500003,2914216|2681516.5063899662|SH_ARC(A(29000000,35500000 28259892,36607650 28000000,37914213 w=250000))|31414216,37914215|[F.Cu]|70000|false|3414216.3025914161|-44.999954570598447|224.99998813366506|180.00003356306661|false|false|f5713012c3445bf8|6b1f8f72f5ba0917|3db02f37259b8235
8108710e-231b-4e6a-80d0-440fa9f48521|A|F.Cu|28000000,39000000|28707107,40707107|28183771,39923880|250000|false|none|0|27749999,38749999,1207110,2207110|1896119.2996676157|SH_ARC(A(28000000,39000000 28183771,39923880 28707107,40707107 w=250000))|30414211,39000001|[F.Cu]|70000|false|2414211.0000002072|-45.000057295839696|180.00002373271411|134.99996643687442|false|false|93bd22a36fa68adc|748ad4fe6877642d|660e416a5175baa0
8389b74e-f6e7-4c8a-9824-41098548832a|A|F.Cu|32962500,33635000|29381843,35118156|31024663,34020460|250000|false|none|0|29131842,33384999,4080660,1983159|3977109.4705328546|SH_ARC(A(32962500,33635000 31024663,34020460 29381843,35118156 w=250000))|32962502,38698821|[F.Cu]|70000|false|5063821.0000003949|-44.999929366188937|269.99997737053519|225.00004800434627|false|false|a9e9d4f83e0c49c1|d3410edd5beb0ed6|b6c7761a1d048153
38c52582-abda-4278-8b98-6af91cf553d4|T|B.Cu|35500000,37085787|35500000,32500000|-|250000|false|none|0|35375000,32375000,250001,4835788|4585787|SH_SEGMENT(a=35500000,37085787 b=35500000,32500000 w=250000)|35500000,37085787|[B.Cu]|70000|false|-|-|-|-|-|-|556b52313663ba28|939452be5263b365|015c6b5315997b3f
f0bf4a85-e807-4677-9e8a-00577e1137b4|T|B.Cu|29000000,41000000|30878680,41000000|-|250000|false|none|0|28875000,40875000,2128681,250001|1878680|SH_SEGMENT(a=29000000,41000000 b=30878680,41000000 w=250000)|29000000,41000000|[B.Cu]|70000|false|-|-|-|-|-|-|f65005186e48b180|db436221c616b65d|a36c9376d2d9e34b
825f1da2-6652-47c5-bf42-9dddc8293a36|A|B.Cu|34500000,39500000|35500000,37085787|35240108,38392350|250000|false|none|0|34249999,36835786,1500003,2914216|2681516.5063899658|SH_ARC(A(34500000,39500000 35240108,38392350 35500000,37085787 w=250000))|32085784,37085785|[B.Cu]|70000|false|3414216.3025914161|-44.99995457059844|44.999988133665049|3.3563066609186866e-05|false|false|af0e632f1f35784b|45d8ad815e8f9e8e|0eb20f842b82f1f0
e5273671-3aaa-4947-bdfd-49bb1c112cf7|A|B.Cu|30878680,41000000|34500000,39500000|32838524,40610163|250000|false|none|0|30628679,39249999,4121323,2000003|4022275.3221706199|SH_ARC(A(30878680,41000000 32838524,40610163 34500000,39500000 w=250000))|30878680,35878680|[B.Cu]|70000|false|5121320|-45|90|45|false|false|f81a306cdab8f467|f630e4eab17411c1|1111d15750123c8e
3548c1d0-b54d-42f8-a856-3a63a6d65fcb|T|F.Cu|42340283,24843644|42500000,25000000|-|250000|false|none|1|42215283,24718644,409718,406357|223509.9971477786|SH_SEGMENT(a=42340283,24843644 b=42500000,25000000 w=250000)|42340283,24843644|[F.Cu]|70000|false|-|-|-|-|-|-|6d73dbdb26bff359|077bf6ccb87fecbc|dce70b4123a4f449
49102581-8aff-4334-a69a-10e5e98c9e76|T|F.Cu|40966975,33635000|38037500,33635000|-|250000|false|none|1|37912500,33510000,3179476,250001|2929475|SH_SEGMENT(a=40966975,33635000 b=38037500,33635000 w=250000)|40966975,33635000|[F.Cu]|70000|false|-|-|-|-|-|-|5a8820568590f74c|67f45644cb5765b5|09819272a62dcd43
5d61f1c1-b5e5-4b52-98e1-cc149d9498a4|T|F.Cu|27000000,27585787|27000000,26500000|-|250000|false|none|1|26875000,26375000,250001,1335788|1085787|SH_SEGMENT(a=27000000,27585787 b=27000000,26500000 w=250000)|27000000,27585787|[F.Cu]|70000|false|-|-|-|-|-|-|c563d8c61f42cbfc|99636754f0a2966d|8c9cd9d19dbbe77b
639d51d3-270a-4c4e-a91b-f334ef31fa4b|T|F.Cu|27707107,24792893|28000000,24500000|-|250000|false|none|1|27582107,24375000,542894,542894|414213.25292414293|SH_SEGMENT(a=27707107,24792893 b=28000000,24500000 w=250000)|27707107,24792893|[F.Cu]|70000|false|-|-|-|-|-|-|e2e2af0395b0e472|689bae70d3ff6845|3617039ed006c67f
903d46c6-6a93-4262-9402-0df9b257439d|T|F.Cu|42939340,32560660|42500000,33000000|-|250000|false|none|1|42375000,32435660,689341,689341|621320.58649299562|SH_SEGMENT(a=42939340,32560660 b=42500000,33000000 w=250000)|42939340,32560660|[F.Cu]|70000|false|-|-|-|-|-|-|ccc495da9020e86f|4da6cc5a9cebf929|dfa98ce31535e9d2
92042dc3-6ea5-4329-9faa-8e27721114af|T|F.Cu|32962500,31095000|30643563,31095000|-|250000|false|none|1|30518563,30970000,2568938,250001|2318937|SH_SEGMENT(a=32962500,31095000 b=30643563,31095000 w=250000)|32962500,31095000|[F.Cu]|70000|false|-|-|-|-|-|-|569ddd38936918d8|55eb5d06845cf601|38d1fb6a676defff
98bdb7bb-500d-4ad5-af4c-47f43ad873c8|T|F.Cu|44000000,28621320|44000000,30000000|-|250000|false|none|1|43875000,28496320,250001,1628681|1378680|SH_SEGMENT(a=44000000,28621320 b=44000000,30000000 w=250000)|44000000,28621320|[F.Cu]|70000|false|-|-|-|-|-|-|4d98cb481e02eb4c|1176e19366fd6db5|16abe876b3c61a7b
ae2386da-5d7b-450d-b289-db8ed22fef26|T|F.Cu|30414213,23500000|38878680,23500000|-|500000|false|none|1|30164213,23250000,8964468,500001|8464467|SH_SEGMENT(a=30414213,23500000 b=38878680,23500000 w=500000)|30414213,23500000|[F.Cu]|70000|false|-|-|-|-|-|-|eb216a25bd85c9f4|78e5b4e8afa57a7a|afe566a8b66854a0
c23a80a7-5bfc-4b61-99b8-5197ce24db86|T|F.Cu|28131219,30128382|28000000,30000000|-|250000|false|none|1|27875000,29875000,381220,378383|183576.58860813381|SH_SEGMENT(a=28131219,30128382 b=28000000,30000000 w=250000)|28131219,30128382|[F.Cu]|70000|false|-|-|-|-|-|-|68ff770fc5d60d13|1ea1ffb3e6568913|e151dbe796510ba8
44781cdf-da73-4be3-a07f-9982ac604288|A|F.Cu|44000000,30000000|42939340,32560660|43724343,31385819|250000|false|none|1|42689339,29749999,1560663,3060663|2844177.961036548|SH_ARC(A(44000000,30000000 43724343,31385819 42939340,32560660 w=250000))|40378674,29999998|[F.Cu]|70000|false|3621326.0000005523|44.999923605743653|3.164353582807872e-05|44.999955249279481|true|false|10c3786965c66625|8b1e5797cef77861|4a456fd5299291a7
5a5e48fd-e572-44d2-9daf-bfb80a0236c7|A|F.Cu|38878680,23500000|42500000,25000000|40838524,23889837|250000|false|none|1|38628679,23249999,4121323,2000003|4022275.3221706199|SH_ARC(A(38878680,23500000 40838524,23889837 42500000,25000000 w=250000))|38878680,28621320|[F.Cu]|70000|false|5121320|45|270|315|true|false|cd820ac03544ffb6|6bb0bccdce8c5599|aafbacd45126edb6
5a89c2f4-8e99-4560-9ae7-f6b71e05c612|A|F.Cu|28000000,24500000|30414213,23500000|29107650,23759892|250000|false|none|1|27749999,23249999,2914216,1500003|2681516.5063899662|SH_ARC(A(28000000,24500000 29107650,23759892 30414213,23500000 w=250000))|30414215,26914216|[F.Cu]|70000|false|3414216.3025914161|44.999954570598447|225.00001186633494|269.99996643693339|true|false|d7172b0d3c9c98bd|a49dd0e0dc33e697|df65df0844ddd06b
68bb9197-0a77-47aa-92fd-cc6756859074|A|F.Cu|42500000,25000000|44000000,28621320|43610163,26661476|250000|false|none|1|42249999,24749999,2000003,4121323|4022275.2104029143|SH_ARC(A(42500000,25000000 43610163,26661476 44000000,28621320 w=250000))|38878680,28621320|[F.Cu]|70000|false|5121319.8576929374|45|315|0|true|false|f3ab5c5fbde759f5|f8814b8de8056850|644f62a90f71196d
bf406f8f-78e6-48b8-9e55-98983db24164|A|F.Cu|42500000,33000000|40966975,33635000|41796643,33469969|250000|false|none|1|40716974,32749999,2033028,1135003|1702763.0396132763|SH_ARC(A(42500000,33000000 41796643,33469969 40966975,33635000 w=250000))|40966977,31466981|[F.Cu]|70000|false|2168019.0896046096|45.000127604273423|44.999925251148532|90.000052855421956|true|false|5eea88080308215f|6dd30aea216aa2c8|8ba9e48ee0d813a0
cfb37722-e3d9-48ee-a976-00e6d7c02db7|A|F.Cu|30643563,31095000|28000000,30000000|29212877,30810419|250000|false|none|1|27749999,29749999,3143566,1595003|2936260.493673387|SH_ARC(A(30643563,31095000 29212877,30810419 28000000,30000000 w=250000))|30643562,27356441|[F.Cu]|70000|false|3738559.0000001336|45.000047836206036|89.99998467436798|135.00003251057402|true|false|c7a8d64fd9b16a51|552f1ee9b3759c74|140b80d818800d8e
e24967b1-33ba-4bad-9692-af4f89bbda0c|A|F.Cu|28000000,30000000|27000000,27585787|27259892,28892350|250000|false|none|1|26749999,27335786,1500003,2914216|2681516.5063899662|SH_ARC(A(28000000,30000000 27259892,28892350 27000000,27585787 w=250000))|30414216,27585785|[F.Cu]|70000|false|3414216.3025914161|44.999954570598447|135.00001186633494|179.99996643693339|true|false|fb595f6ac9bedc9a|6a5ed55d0fb7cc36|51aa5cc74b8ad5d4
f121685b-fe56-4138-9696-f5a078f65a96|A|F.Cu|27000000,26500000|27707107,24792893|27183771,25576120|250000|false|none|1|26749999,24542892,1207110,2207110|1896119.2996676157|SH_ARC(A(27000000,26500000 27183771,25576120 27707107,24792893 w=250000))|29414211,26499999|[F.Cu]|70000|false|2414211.0000002072|45.000057295839696|179.99997626728589|225.00003356312558|true|false|0bd0c20b77e1fe1e|8fe0ba51b8da61e3|2290d225de5ef922
00000000-0000-0000-0000-00000000a001|T|F.Cu|10000000,10000000|20000000,12500000|-|300000|true|50000|0|9850000,9850000,10300001,2800001|10307764.064044151|SH_SEGMENT(a=10000000,10000000 b=20000000,12500000 w=300000)|10000000,10000000|[F.Cu,F.Mask]|50000|false|-|-|-|-|-|-|7695df546312c2d2|744ee07b1072ecd6|4b380489aed0e0ea
00000000-0000-0000-0000-00000000a002|T|F.Cu|10000000,15000000|20000000,15000000|-|300000|true|-200000|0|9850000,14850000,10300001,300001|10000000|SH_SEGMENT(a=10000000,15000000 b=20000000,15000000 w=300000)|10000000,15000000|[F.Cu,F.Mask]|-150000|false|-|-|-|-|-|-|9a9ebc11e1874cb0|252e343a310f5ca3|1417b410c4d7ed2f
00000000-0000-0000-0000-00000000a003|T|B.Cu|10000000,20000000|10000000,30000000|-|250000|true|none|0|9875000,19875000,250001,10250001|10000000|SH_SEGMENT(a=10000000,20000000 b=10000000,30000000 w=250000)|10000000,20000000|[B.Cu,B.Mask]|70000|false|-|-|-|-|-|-|39c21e0984d04c00|1b2547d848fd5fcf|e0ae43425adeb4b5
00000000-0000-0000-0000-00000000a004|T|In1.Cu|12000000,20000000|12000000,20000000|-|250000|false|none|0|11875000,19875000,250001,250001|0|SH_SEGMENT(a=12000000,20000000 b=12000000,20000000 w=250000)|12000000,20000000|[In1.Cu]|70000|true|-|-|-|-|-|-|f3ddb5f97554d7df|7c42996e1b001ee9|7e8bbcfe766b758b
00000000-0000-0000-0000-00000000a005|T|B.Cu|-5000000,-5000000|-7300000,4100000|-|201000|false|none|0|-7400500,-5100500,2501001,9301001|9386160.0242058523|SH_SEGMENT(a=-5000000,-5000000 b=-7300000,4100000 w=201000)|-5000000,-5000000|[B.Cu]|70000|false|-|-|-|-|-|-|be2633dd64a94322|6427f0aa603fc4ab|1b09b1254fe1f7ed
00000000-0000-0000-0000-00000000a006|A|F.Cu|50000000,50000000|60000000,50000000|55000000,55000000|400000|true|25000|0|49599999,49599999,10800003,5800003|15707963.267948966|SH_ARC(A(50000000,50000000 55000000,55000000 60000000,50000000 w=400000))|55000000,50000000|[F.Cu,F.Mask]|25000|false|5000000|-180|180|0|false|false|5daaf25da1c1acfb|330b1da9a5520cc5|0d02974ea715f51e
00000000-0000-0000-0000-00000000a007|A|B.Cu|60000000,50000000|50000000,50000000|55000000,45000000|400000|true|none|0|49599999,44599999,10800003,5800003|15707963.267948966|SH_ARC(A(60000000,50000000 55000000,45000000 50000000,50000000 w=400000))|55000000,50000000|[B.Cu,B.Mask]|70000|false|5000000|-180|0|180|false|false|002ee531ac452519|bd93f9ad0e85dbbf|22c95f3d6e882d23
00000000-0000-0000-0000-00000000a008|A|F.Cu|70000000,70000000|70000004,70000001|70000002,70000003|200000|false|none|0|69799999,69799999,400007,400006|8.0615623023717742|SH_ARC(A(70000000,70000000 70000002,70000003 70000004,70000001 w=200000))|70000002,70000001|[F.Cu]|70000|false|2.2360679774997898|-206.565051177078|206.565051177078|0|false|true|42d3b36a3930cfd9|6a91f47dabc7b105|41104d5c2d3bc2c5
00000000-0000-0000-0000-00000000a009|T|F.Cu|80000000,80000000|90000000,80000000|-|200000|false|none|0|79900000,79900000,10200001,200001|10000000|SH_SEGMENT(a=80000000,80000000 b=90000000,80000000 w=200000)|80000000,80000000|[F.Cu]|70000|false|-|-|-|-|-|-|481ca60dbe18cec8|d8d4a6877c611db8|6d981ed73f388e2f
00000000-0000-0000-0000-00000000a010|A|B.Cu|100000000,100000000|99100000,97300000|101500000,98200000|150000|false|none|0|98949999,96835561,2728727,3314593|5748113.6114028227|SH_ARC(A(100000000,100000000 101500000,98200000 99100000,97300000 w=150000))|100021429,98492857|[B.Cu]|70000|false|1507295.3341963214|-218.49908416965047|90.814593261903227|232.31550909225274|false|false|3b6dc7122a0e0467|3a630fcf80eb542a|e0654dd49fa3de4a
00000000-0000-0000-0000-00000000a011|A|F.Cu|-20000000,-20000000|-20000000,0|-30000000,-10000000|150000|false|none|0|-30150001,-20150001,10300003,20300003|31415926.535897933|SH_ARC(A(-20000000,-20000000 -30000000,-10000000 -20000000,0 w=150000))|-20000000,-10000000|[F.Cu]|70000|false|10000000|-180|270|90|false|false|3a9332ce2a198969|19e9afd402b7f3f8|19404ddbf501645d
";

    fn vec(s: &str) -> Vector2I {
        let (x, y) = s.split_once(',').unwrap();
        Vector2I::new(x.parse().unwrap(), y.parse().unwrap())
    }

    fn layer(name: &str) -> PcbLayerId {
        PcbLayerId(Lset::name_to_layer(name))
    }

    fn v_points(p: &[Vector2I]) -> String {
        let pts: Vec<String> = p.iter().map(|v| format!("{},{}", v.x, v.y)).collect();
        format!("[{}]", pts.join(" "))
    }

    /// The dump's `VPolySet` text for polygons without arcs.
    fn v_polyset(s: &ShapePolySet) -> String {
        let chain = |c: &kicad_geom::ShapeLineChain| {
            assert_eq!(c.arc_count(), 0);
            format!(
                "C({} w={} n={} {})",
                if c.is_closed() { "closed" } else { "open" },
                c.width(),
                c.point_count(),
                v_points(c.c_points())
            )
        };
        let mut r = format!("P({}", s.outline_count());

        for i in 0..s.outline_count() {
            r.push_str(" {");
            r.push_str(&chain(s.c_outline(i)));

            for h in 0..s.hole_count(i) {
                r.push_str(" hole ");
                r.push_str(&chain(s.c_hole(i, h)));
            }

            r.push('}');
        }

        r.push(')');
        r
    }

    fn fnv1a(s: &str) -> String {
        let mut h: u64 = 0xcbf29ce484222325;

        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }

        format!("{h:016x}")
    }

    fn v_shape(s: &SharedShape) -> String {
        match s.as_shape_ref() {
            ShapeRef::Segment(g) => format!(
                "SH_SEGMENT(a={},{} b={},{} w={})",
                g.get_seg().a.x,
                g.get_seg().a.y,
                g.get_seg().b.x,
                g.get_seg().b.y,
                g.get_width()
            ),
            ShapeRef::Arc(a) => format!(
                "SH_ARC(A({},{} {},{} {},{} w={}))",
                a.get_p0().x,
                a.get_p0().y,
                a.get_arc_mid().x,
                a.get_arc_mid().y,
                a.get_p1().x,
                a.get_p1().y,
                a.get_width()
            ),
            _ => panic!("unexpected shape"),
        }
    }

    fn v_bool(b: bool) -> &'static str {
        if b { "true" } else { "false" }
    }

    /// `%.17g`-printed doubles round-trip: compare bit for bit.
    fn assert_f64(what: &str, got: f64, want: &str) {
        let want: f64 = want.parse().unwrap();
        assert_eq!(got.to_bits(), want.to_bits(), "{what}: {got} != {want}");
    }

    /// The golden board: every golden row added to a fresh board with the dumped members.
    fn golden_board() -> (Board, Vec<(TrackId, Vec<&'static str>)>) {
        let mut board = Board::new();
        board.design_settings_mut().solder_mask_expansion = PCB_IU_SCALE.mm_to_iu(0.07);
        let net1 = NetinfoItem::new(&mut board.kiid_generator, "net1", NetCode(1));
        board.add_net(net1);
        let mut rows = Vec::new();

        for line in GOLDEN.lines() {
            let f: Vec<&'static str> = line.split('|').collect();
            let mut t = PcbTrack::default();
            t.base.item.uuid = Kiid::parse(f[0]).unwrap();
            t.base.item.layer = layer(f[2]);
            t.base.net = NetCode(f[9].parse().unwrap());
            t.start = vec(f[3]);
            t.end = vec(f[4]);
            t.width = f[6].parse().unwrap();
            t.has_solder_mask = f[7] == "true";
            t.solder_mask_margin = (f[8] != "none").then(|| f[8].parse().unwrap());

            let track = if f[1] == "A" {
                Track::Arc(PcbArc {
                    track: t,
                    mid: vec(f[5]),
                })
            } else {
                Track::Trace(t)
            };

            let id = board.add_track(track, AddMode::Append).unwrap();
            rows.push((id, f));
        }

        board.check_invariants().unwrap();
        (board, rows)
    }

    #[test]
    fn golden_tracks_and_arcs_match_reference_dump() {
        let (board, rows) = golden_board();
        let ctx = BoardCtx::bare(&board);
        assert_eq!(rows.len(), 40);

        for (id, f) in rows {
            let t = board.track(id).unwrap();
            let uuid = f[0];
            let bb = t.bounding_box(&ctx);
            let want_bb: Vec<i64> = f[10].split(',').map(|v| v.parse().unwrap()).collect();
            assert_eq!(
                [
                    bb.position().x as i64,
                    bb.position().y as i64,
                    bb.width(),
                    bb.height()
                ],
                [want_bb[0], want_bb[1], want_bb[2], want_bb[3]],
                "{uuid} bbox"
            );
            assert_f64(&format!("{uuid} length"), t.length(), f[11]);
            let shape = t.effective_shape(
                &ctx,
                PcbLayerId::UndefinedLayer,
                Flashing::Default,
                DrcConstraintT::NullConstraint,
            );
            assert_eq!(v_shape(&shape), f[12], "{uuid} shape");
            assert_eq!(BoardItem::position(t, &ctx), vec(f[13]), "{uuid} position");
            let names: Vec<String> = t
                .layer_set(&ctx)
                .seq()
                .0
                .iter()
                .map(|l| Lset::name(*l))
                .collect();
            assert_eq!(format!("[{}]", names.join(",")), f[14], "{uuid} layers");
            assert_eq!(t.is_null().to_string(), f[16], "{uuid} is_null");

            match t {
                Track::Trace(s) => {
                    assert_eq!(
                        s.solder_mask_expansion(&ctx).to_string(),
                        f[15],
                        "{uuid} mask"
                    );
                }
                Track::Arc(a) => {
                    assert_eq!(
                        a.solder_mask_expansion(&ctx).to_string(),
                        f[15],
                        "{uuid} mask"
                    );
                    assert_eq!(BoardItem::center(t, &ctx), vec(f[13]), "{uuid} center");
                    assert_f64(&format!("{uuid} radius"), a.radius(), f[17]);
                    assert_f64(&format!("{uuid} angle"), a.angle().as_degrees(), f[18]);
                    assert_f64(
                        &format!("{uuid} start"),
                        a.arc_angle_start().as_degrees(),
                        f[19],
                    );
                    assert_f64(
                        &format!("{uuid} end"),
                        a.arc_angle_end().as_degrees(),
                        f[20],
                    );
                    assert_eq!(v_bool(a.is_ccw()), f[21], "{uuid} ccw");
                    assert_eq!(v_bool(a.is_degenerated(5)), f[22], "{uuid} degenerated");
                }
                Track::Via(_) => unreachable!(),
            }

            let own_layer = layer(f[2]);
            let combos = [
                (0, 5000, ERROR_INSIDE),
                (0, 1000, ERROR_OUTSIDE),
                (100000, 5000, ERROR_OUTSIDE),
            ];

            for (i, (clearance, error, loc)) in combos.into_iter().enumerate() {
                let mut poly = ShapePolySet::new();
                t.transform_shape_to_polygon(
                    &ctx, &mut poly, own_layer, clearance, error, loc, false,
                );
                assert_eq!(fnv1a(&v_polyset(&poly)), f[23 + i], "{uuid} tpoly {i}");
            }
        }
    }

    #[test]
    fn solder_mask_layers_grow_the_shapes() {
        let (board, rows) = golden_board();
        let ctx = BoardCtx::bare(&board);
        let (id, _) = rows.iter().find(|(_, f)| f[0].ends_with("a006")).unwrap();
        let t = board.track(*id).unwrap();
        // GetEffectiveShape on F.Mask: width + 2 * 25000 (local margin).
        let shape = t.effective_shape(
            &ctx,
            PcbLayerId::FMask,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        assert_eq!(shape.get_width(), 400000 + 50000);
        assert!(t.is_on_layer(&ctx, PcbLayerId::FMask));
        assert!(!t.is_on_layer(&ctx, PcbLayerId::BMask));
        // A negative margin is limited to half the width: -0.2 mm on a 0.3 mm track -> -0.15 mm.
        let (id, _) = rows.iter().find(|(_, f)| f[0].ends_with("a002")).unwrap();
        let t = board.track(*id).unwrap();
        let shape = t.effective_shape(
            &ctx,
            PcbLayerId::FMask,
            Flashing::Default,
            DrcConstraintT::NullConstraint,
        );
        assert_eq!(shape.get_width(), 0);
    }

    #[test]
    fn no_board_null_branches() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        // GetBoard() == nullptr: no board setting, only the local margin.
        let mut t = PcbTrack::default();
        assert_eq!(t.solder_mask_expansion(&ctx), 0);
        t.solder_mask_margin = Some(-500000);
        assert_eq!(t.solder_mask_expansion(&ctx), -100000);
        let (value, source) = t.width_constraint(&ctx, true);
        assert!(value.is_null() || !value.has_opt());
        assert_eq!(source.as_deref(), Some(""));
        assert_eq!(t.width_constraint(&ctx, false).1, None);
    }

    /// A rule resolver that answers fixed constraints and records what it was asked.
    struct Rules {
        seen: std::sync::Mutex<Vec<(DrcConstraintT, KicadT, PcbLayerId)>>,
    }

    impl crate::ctx::RuleResolver for Rules {
        fn eval_rules(
            &self,
            _ctx: &BoardCtx<'_>,
            constraint: DrcConstraintT,
            a: Option<ItemRef<'_>>,
            b: Option<ItemRef<'_>>,
            layer: PcbLayerId,
        ) -> DrcConstraint {
            assert!(b.is_none());
            self.seen
                .lock()
                .unwrap()
                .push((constraint, a.unwrap().type_(), layer));
            let mut c = DrcConstraint::new(constraint, "");
            c.value.set_opt(12345);
            c.parent_rule = Some(DrcRuleInfo {
                name: "width".to_string(),
                ..DrcRuleInfo::default()
            });
            c
        }
        fn has_rules_for_constraint_type(&self, _constraint: DrcConstraintT) -> bool {
            true
        }
        fn cached_own_clearance(
            &self,
            _ctx: &BoardCtx<'_>,
            _item: ItemRef<'_>,
            _layer: PcbLayerId,
            _want_source: bool,
        ) -> (i32, Option<String>) {
            (0, None)
        }
        fn query_worst_constraint(&self, _constraint: DrcConstraintT) -> Option<DrcConstraint> {
            None
        }
    }

    #[test]
    fn rules_see_the_real_item() {
        let mut board = Board::new();
        let mut arc = PcbArc::default();
        arc.track.base.item.layer = PcbLayerId::BCu;
        arc.track.solder_mask_margin = Some(1);
        let id = board.add_track(Track::Arc(arc), AddMode::Append).unwrap();
        let rules = Rules {
            seen: Default::default(),
        };
        let ctx = BoardCtx::bare(&board).with_rules(&rules);
        let t = board.track(id).unwrap();

        let (value, source) = t.width_constraint(&ctx, true);
        assert_eq!(value.opt(), 12345);
        assert_eq!(source.as_deref(), Some("rule 'width'"));
        // Rules win over the local margin.
        assert_eq!(t.as_arc().unwrap().solder_mask_expansion(&ctx), 12345);

        // A detached copy (no handle) still hands the resolver an arc.
        let copy = t.as_arc().unwrap().clone();
        let mut copy = copy;
        copy.track.base.item.id = None;
        copy.track.base.item.parent = ParentRef::Board;
        assert_eq!(copy.width_constraint(&ctx, false).0.opt(), 12345);

        let seen = rules.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(
            seen.iter()
                .all(|(_, t, l)| *t == KicadT::PcbArcT && *l == PcbLayerId::BCu)
        );
        assert_eq!(seen[1].0, DrcConstraintT::SolderMaskExpansionConstraint);
    }

    // ---- qa/tests/pcbnew/test_board_item.cpp (PCB_TRACK and PCB_ARC rows) ----

    /// `IterateOverPositionsAndReferences` (`qa/qa_utils/eda_item_test_utils.h`).
    fn iterate_positions(mut f: impl FnMut(Vector2I, Vector2I)) {
        let (xstep, ystep) = (1_000_000, 500_000); // schIUScale.mmToIU( 100 ), ( 50 )

        for pos_x in [-xstep, 0, xstep] {
            for pos_y in [-ystep, 0, ystep] {
                for ref_x in [-xstep, 0, xstep] {
                    for ref_y in [-ystep, 0, ystep] {
                        f(Vector2I::new(pos_x, pos_y), Vector2I::new(ref_x, ref_y));
                    }
                }
            }
        }
    }

    /// The two items `Instantiate( PCB_TRACE_T / PCB_ARC_T )` makes (parent: the board).
    fn instantiate() -> [Track; 2] {
        let mut t = PcbTrack::default();
        t.base.item.parent = ParentRef::Board;
        [
            Track::Trace(t.clone()),
            Track::Arc(PcbArc {
                track: t,
                mid: Vector2I::default(),
            }),
        ]
    }

    /// `CompareItems`: position and bounding box edges.
    fn compare_items(ctx: &BoardCtx<'_>, a: &Track, b: &Track) {
        assert_eq!(BoardItem::position(a, ctx), BoardItem::position(b, ctx));
        let (ba, bb) = (a.bounding_box(ctx), b.bounding_box(ctx));
        assert_eq!(
            (ba.top(), ba.left(), ba.bottom(), ba.right()),
            (bb.top(), bb.left(), bb.bottom(), bb.right())
        );
    }

    #[test]
    fn board_item_move_rotate_flip_are_identities() {
        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        let env = MutEnv::detached();

        for item in instantiate() {
            iterate_positions(|pos, r| {
                let mut original = item.clone();
                original.set_position(&env, pos);

                // Move
                let mut moved = original.clone();
                let before = BoardItem::position(&moved, &ctx);
                moved.move_by(&env, r);
                assert_eq!(BoardItem::position(&moved, &ctx), before + r);
                moved.move_by(&env, -r);
                compare_items(&ctx, &moved, &original);

                // Rotate: four 90 degree rotations
                let mut rotated = original.clone();
                for _ in 0..4 {
                    rotated.rotate(&env, r, EdaAngle::from_degrees(90.0));
                }
                compare_items(&ctx, &rotated, &original);

                // FlipLeftRight / FlipUpDown twice
                for dir in [FlipDirection::LeftRight, FlipDirection::TopBottom] {
                    let mut flipped = original.clone();
                    flipped.flip(&env, r, dir);
                    flipped.flip(&env, r, dir);
                    compare_items(&ctx, &flipped, &original);
                    assert_eq!(flipped.base().layer, original.base().layer);
                }
            });
        }
    }

    #[test]
    fn boardless_items_use_standard_layer_metadata() {
        let env = MutEnv::detached();
        let mut track = PcbTrack::default();
        track.base.item.layer = PcbLayerId::FCu;
        track.flip(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);
        assert_eq!(track.base.item.layer, PcbLayerId::BCu);

        let mut arc = PcbArc::default();
        arc.track.base.item.layer = PcbLayerId::FSilkS;
        arc.flip(&env, Vector2I::new(0, 0), FlipDirection::LeftRight);
        assert_eq!(arc.track.base.item.layer, PcbLayerId::BSilkS);
    }

    // ---- members without an oracle field: checked against the C++ bodies by hand ----

    #[test]
    fn point_on_ends_collinear_and_texts() {
        let mut t = PcbTrack::default();
        t.start = Vector2I::new(0, 0);
        t.end = Vector2I::new(1000, 0);
        t.width = 200;
        assert_eq!(t.is_point_on_ends(Vector2I::new(0, 0), 0), STARTPOINT);
        assert_eq!(t.is_point_on_ends(Vector2I::new(1, 0), 0), 0);
        assert_eq!(t.is_point_on_ends(Vector2I::new(1000, 100), -1), ENDPOINT);
        assert_eq!(t.is_point_on_ends(Vector2I::new(1000, 101), -1), 0);
        assert_eq!(
            t.is_point_on_ends(Vector2I::new(500, 0), 500),
            STARTPOINT | ENDPOINT
        );

        let mut u = t.clone();
        u.start = Vector2I::new(1000, 0);
        u.end = Vector2I::new(3000, 1);
        assert!(t.approx_collinear(&u));
        u.end = Vector2I::new(3000, 500);
        assert!(!t.approx_collinear(&u));

        let board = Board::new();
        let ctx = BoardCtx::bare(&board);
        assert_eq!(t.friendly_name(&ctx), "Track");
        assert_eq!(PcbArc::default().friendly_name(&ctx), "Track (arc)");
        let units = UnitsProvider::new(&PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(
            t.item_description(&ctx, &units, true),
            "Track [** NO BOARD DEFINED **] on F.Cu, length 0.0010 mm"
        );

        // Hit tests (PCB_TRACK::HitTest / PCB_ARC::HitTest).
        assert!(t.hit_test(&ctx, Vector2I::new(500, 100), 0));
        assert!(!t.hit_test(&ctx, Vector2I::new(500, 101), 0));
        assert!(t.hit_test(&ctx, Vector2I::new(500, 101), 1));
        let r = Box2I::new(Vector2I::new(-10, -10), Vector2L::new(1020, 20));
        assert!(t.hit_test_rect(&ctx, &r, true, 0));
        let r = Box2I::new(Vector2I::new(400, -50), Vector2L::new(10, 100));
        assert!(!t.hit_test_rect(&ctx, &r, true, 0));
        assert!(t.hit_test_rect(&ctx, &r, false, 0));

        let mut a = PcbArc::default();
        a.track.start = Vector2I::new(-1000, 0);
        a.mid = Vector2I::new(0, -1000);
        a.track.end = Vector2I::new(1000, 0);
        a.track.width = 100;
        assert!(a.hit_test(&ctx, Vector2I::new(0, -1050), 0));
        assert!(!a.hit_test(&ctx, Vector2I::new(0, -1051), 0));
        assert!(!a.hit_test(&ctx, Vector2I::new(0, 1000), 0));
        assert!(a.hit_test(&ctx, Vector2I::new(707, -707), 0));
    }

    #[test]
    fn layer_set_round_trip_and_create_from_three_points() {
        let env = MutEnv::detached();
        let mut t = PcbTrack::default();
        t.set_layer_set(
            &env,
            Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::BMask]),
        );
        assert_eq!(t.base.item.layer, PcbLayerId::BCu);
        assert!(t.has_solder_mask);
        // A solder mask opening only shows on the mask of the same side.
        t.base.item.layer = PcbLayerId::In1Cu;
        let board = Board::new();
        assert_eq!(
            t.layer_set(&BoardCtx::bare(&board)),
            Lset::from_layers(&[PcbLayerId::In1Cu])
        );

        let mut kiid_generator = KiidGenerator::default();
        let straight = PcbArc::create_from_three_points(
            &mut kiid_generator,
            Vector2I::new(0, 0),
            Vector2I::new(50, 50),
            Vector2I::new(100, 100),
        );
        assert_eq!(straight.type_(), KicadT::PcbTraceT);
        let bow = PcbArc::create_from_three_points(
            &mut kiid_generator,
            Vector2I::new(0, 0),
            Vector2I::new(PCB_IU_SCALE.mm_to_iu(100.0), PCB_IU_SCALE.mm_to_iu(5.0)),
            Vector2I::new(PCB_IU_SCALE.mm_to_iu(200.0), 0),
        );
        assert_eq!(bow.type_(), KicadT::PcbArcT);
        assert_eq!(bow.as_arc().unwrap().mid().y, PCB_IU_SCALE.mm_to_iu(5.0));
    }

    #[test]
    fn cmp_tracks_save_order() {
        let (board, rows) = golden_board();
        let ctx = BoardCtx::bare(&board);
        let mut ids: Vec<TrackId> = rows.iter().map(|(id, _)| *id).collect();
        ids.sort_by(|a, b| pcb_track_cmp_tracks(&ctx, *a, *b));

        let key = |id: &TrackId| {
            let t = board.track(*id).unwrap();
            (
                t.net_code(),
                BoardItem::layer(t, &ctx),
                t.type_() as i32,
                *BoardItem::uuid(t),
            )
        };

        for w in ids.windows(2) {
            assert!(key(&w[0]) < key(&w[1]));
        }

        assert_eq!(pcb_track_cmp_tracks(&ctx, ids[0], ids[0]), Ordering::Equal);
    }

    #[test]
    fn equality_and_similarity() {
        let mut a = PcbTrack::default();
        let mut b = a.clone();
        assert!(a.eq_item(&b));
        assert_eq!(a.similarity(&b), 1.0);
        b.width = 1;
        b.end = Vector2I::new(1, 1);
        assert!(!a.eq_item(&b));
        assert_eq!(a.similarity(&b), 1.0 * 0.9 * 0.9);
        a.solder_mask_margin = Some(0);
        let arc = PcbArc {
            track: a.clone(),
            mid: Vector2I::new(3, 3),
        };
        let mut arc2 = arc.clone();
        assert!(arc.eq_item(&arc2));
        arc2.mid = Vector2I::default();
        assert!(!arc.eq_item(&arc2));
        assert_eq!(arc.similarity(&arc2), 0.9);
    }
}
